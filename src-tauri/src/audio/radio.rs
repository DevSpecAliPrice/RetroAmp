//! Radio audio source — decodes internet radio streams via HTTP.
//!
//! Implements the `AudioSource` trait for internet radio. Audio data flows
//! from an HTTP stream through a ring buffer into Symphonia for decoding.
//! ICY metadata updates are reflected in `metadata()` calls.
//!
//! Two decode paths are supported:
//! - **Symphonia probe**: for formats Symphonia can auto-detect (MP3, OGG, FLAC, …)
//! - **ADTS direct**: for raw AAC/AAC+ streams (ADTS framing) which Symphonia
//!   lacks a native format reader for. We parse ADTS headers ourselves and feed
//!   raw AAC payloads to Symphonia's AAC decoder.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{
    CodecParameters, CodecType, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_ALAC,
    CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_NULL, CODEC_TYPE_VORBIS,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, Packet};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::adts::{self, AdtsFrameReader};
use crate::audio::error::AudioError;
use crate::audio::icy::{self, IcyMetadata};
use crate::audio::recorder::RadioRecorder;
use crate::audio::source::{
    AudioBuffer, AudioSource, SourceCapabilities, SourceState, TrackMetadata,
};
use crate::audio::stream_reader::{StreamBufReader, StreamReader};

/// Maximum time `next_buffer()` will spend trying to decode before yielding
/// back to the engine (so it can process commands like Play/Stop).
const MAX_DECODE_LOOP_SECS: f64 = 2.0;

/// After this many consecutive `next_buffer()` calls that fail to produce audio,
/// give up and signal the track as finished.
const MAX_CONSECUTIVE_EMPTY: u32 = 5;

/// Samples per AAC frame (LC-AAC = 1024).
const AAC_SAMPLES_PER_FRAME: u64 = 1024;

// ---------------------------------------------------------------------------
// Decode pipeline — Symphonia probe vs. our ADTS parser
// ---------------------------------------------------------------------------

enum DecodePipeline {
    Symphonia {
        format: Box<dyn FormatReader>,
        decoder: Box<dyn symphonia::core::codecs::Decoder>,
        track_id: u32,
    },
    Adts {
        frame_reader: AdtsFrameReader<StreamBufReader>,
        decoder: Box<dyn symphonia::core::codecs::Decoder>,
        timestamp: u64,
    },
}

// ---------------------------------------------------------------------------
// RadioSource
// ---------------------------------------------------------------------------

pub struct RadioSource {
    pipeline: DecodePipeline,

    sample_rate: u32,
    channels: u16,

    station_name: Option<String>,
    genre: Option<String>,
    bitrate: Option<u32>,
    #[allow(dead_code)]
    url: String,

    icy_metadata: Arc<Mutex<IcyMetadata>>,
    state: SourceState,
    elapsed: Duration,
    connected: Arc<AtomicBool>,
    reader_alive: Arc<AtomicBool>,
    _reader: StreamReader,

    recorder: Option<Arc<RadioRecorder>>,

    sample_buf: Option<SampleBuffer<f32>>,
    consecutive_empty: u32,
}

impl RadioSource {
    pub fn connect(url: &str) -> Result<Self, AudioError> {
        Self::connect_with_name(url, None)
    }

    pub fn connect_with_name(url: &str, display_name: Option<&str>) -> Result<Self, AudioError> {
        Self::connect_inner(url, display_name, None)
    }

    /// Connect with a recorder for track buffering/saving.
    pub fn connect_with_name_and_recorder(
        url: &str,
        display_name: Option<&str>,
        recorder: Arc<RadioRecorder>,
    ) -> Result<Self, AudioError> {
        Self::connect_inner(url, display_name, Some(recorder))
    }

    /// Get the recorder, if one was provided during connect.
    pub fn recorder(&self) -> Option<Arc<RadioRecorder>> {
        self.recorder.clone()
    }

    fn connect_inner(
        url: &str,
        display_name: Option<&str>,
        recorder: Option<Arc<RadioRecorder>>,
    ) -> Result<Self, AudioError> {
        let mut reader = match &recorder {
            Some(rec) => StreamReader::connect_with_recorder(url, Arc::clone(rec))?,
            None => StreamReader::connect(url)?,
        };

        let consumer = reader
            .take_consumer()
            .ok_or_else(|| AudioError::Network("consumer already taken".into()))?;

        let mut stream_info = reader.stream_info.clone();
        if let Some(name) = display_name {
            stream_info.station_name = Some(name.to_string());
        }
        let icy_metadata = Arc::clone(&reader.icy_metadata);
        let connected = Arc::clone(&reader.connected);
        let reader_alive = Arc::clone(&reader.reader_alive);
        let stop_flag = reader.stop_flag();

        // Pre-buffer.
        {
            use ringbuf::traits::Observer;
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                if consumer.occupied_len() >= 16384 {
                    break;
                }
                if std::time::Instant::now() > deadline {
                    if consumer.occupied_len() > 0 {
                        break;
                    }
                    return Err(AudioError::ConnectionFailed(
                        "timed out waiting for stream data".into(),
                    ));
                }
                if !connected.load(Ordering::Relaxed) && consumer.occupied_len() == 0 {
                    return Err(AudioError::ConnectionFailed(
                        "stream disconnected before any data received".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            log::info!("[radio] pre-buffered {} bytes", consumer.occupied_len());
        }

        // Detect ADTS before Symphonia's probe. ADTS sync (0xFFF, layer=00)
        // overlaps with MP3 sync (0xFFE), so Symphonia's MP3 demuxer falsely
        // accepts AAC streams. Since Symphonia has no ADTS format reader, we
        // parse ADTS ourselves and feed raw AAC to Symphonia's AAC decoder.
        let adts_header = {
            use ringbuf::traits::Consumer;
            let (s1, s2) = consumer.as_slices();
            let peek_len = 4096.min(s1.len() + s2.len());
            let mut peek = Vec::with_capacity(peek_len);
            peek.extend_from_slice(&s1[..peek_len.min(s1.len())]);
            if peek.len() < peek_len {
                peek.extend_from_slice(&s2[..(peek_len - peek.len()).min(s2.len())]);
            }
            adts::detect_adts(&peek)
        };

        let content_type_str = stream_info.content_type.clone();
        let is_adts = adts_header.is_some();

        // Inform the recorder of the true stream format (after ADTS detection).
        if let Some(ref recorder) = recorder {
            recorder.set_stream_info(
                &content_type_str,
                is_adts,
                stream_info.bitrate,
                stream_info.station_name.as_deref(),
                stream_info.metaint.is_some(),
            );
        }

        let (pipeline, sample_rate, channels) = if let Some(ref hdr) = adts_header {
            build_adts_pipeline(
                consumer,
                stop_flag,
                Arc::clone(&reader_alive),
                hdr,
                &content_type_str,
            )?
        } else {
            build_symphonia_pipeline(
                consumer,
                stop_flag,
                Arc::clone(&reader_alive),
                &content_type_str,
            )?
        };

        log::info!(
            "[radio] connected to {} — {}Hz {}ch, {}, content-type: {}",
            stream_info.station_name.as_deref().unwrap_or(url),
            sample_rate,
            channels,
            match &pipeline {
                DecodePipeline::Adts { .. } => "codec: AAC (ADTS)",
                DecodePipeline::Symphonia { .. } => "probed by Symphonia",
            },
            content_type_str,
        );

        Ok(Self {
            pipeline,
            sample_rate,
            channels,
            station_name: stream_info.station_name,
            genre: stream_info.genre,
            bitrate: stream_info.bitrate,
            url: url.to_string(),
            icy_metadata,
            state: SourceState::Ready,
            elapsed: Duration::ZERO,
            connected,
            reader_alive,
            _reader: reader,
            recorder,
            sample_buf: None,
            consecutive_empty: 0,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers (associated functions taking disjoint field refs to avoid borrow
// conflicts with the pipeline match)
// ---------------------------------------------------------------------------

fn make_silence(sample_rate: u32, channels: u16) -> AudioBuffer {
    let len = (sample_rate as usize / 100) * channels as usize;
    AudioBuffer {
        samples: vec![0.0; len],
        sample_rate,
        channels,
    }
}

/// Decode a packet, convert to interleaved f32, update bookkeeping.
fn try_decode(
    decoder: &mut dyn symphonia::core::codecs::Decoder,
    packet: &Packet,
    sample_buf: &mut Option<SampleBuffer<f32>>,
    elapsed: &mut Duration,
    consecutive_empty: &mut u32,
) -> Option<AudioBuffer> {
    let decoded = match decoder.decode(packet) {
        Ok(d) => d,
        Err(SymphoniaError::DecodeError(e)) => {
            log::debug!("[radio] decode error (skipping): {e}");
            return None;
        }
        Err(e) => {
            log::warn!("[radio] decode error: {e}");
            return None;
        }
    };

    let spec = *decoded.spec();
    let num_frames = decoded.capacity();
    if num_frames == 0 {
        return None;
    }

    // Use the actual decoded rate/channels — for HE-AAC with SBR, the
    // decoder may output at double the ADTS base rate.
    let actual_rate = spec.rate;
    let actual_channels = spec.channels.count() as u16;

    let sbuf = sample_buf.get_or_insert_with(|| {
        SampleBuffer::<f32>::new(num_frames as u64, spec)
    });
    if sbuf.capacity() < num_frames {
        *sample_buf = Some(SampleBuffer::<f32>::new(num_frames as u64, spec));
    }
    let sbuf = sample_buf.as_mut().unwrap();

    sbuf.copy_interleaved_ref(decoded);
    let samples = sbuf.samples().to_vec();

    *elapsed += Duration::from_secs_f64(num_frames as f64 / actual_rate as f64);
    *consecutive_empty = 0;

    Some(AudioBuffer {
        samples,
        sample_rate: actual_rate,
        channels: actual_channels,
    })
}

/// Handle decode-loop timeout: yield silence or give up.
fn yield_or_give_up(
    consecutive_empty: &mut u32,
    state: &mut SourceState,
    sample_rate: u32,
    channels: u16,
) -> Result<Option<AudioBuffer>, AudioError> {
    *consecutive_empty += 1;
    if *consecutive_empty >= MAX_CONSECUTIVE_EMPTY {
        log::error!(
            "[radio] no valid audio after {} attempts — giving up",
            *consecutive_empty,
        );
        *state = SourceState::Error;
        return Ok(None);
    }
    log::warn!(
        "[radio] no audio decoded in {MAX_DECODE_LOOP_SECS}s, yielding ({}/{})",
        *consecutive_empty,
        MAX_CONSECUTIVE_EMPTY,
    );
    Ok(Some(make_silence(sample_rate, channels)))
}

// ---------------------------------------------------------------------------
// AudioSource implementation
// ---------------------------------------------------------------------------

impl AudioSource for RadioSource {
    fn metadata(&self) -> Result<TrackMetadata, AudioError> {
        let icy = self.icy_metadata.lock().unwrap_or_else(|e| e.into_inner());
        let (title, artist) = match &icy.stream_title {
            Some(raw) => icy::split_stream_title(raw),
            None => (self.station_name.clone(), None),
        };
        Ok(TrackMetadata {
            title: title.or(self.station_name.clone()),
            artist,
            album: self.station_name.clone(),
            duration: None,
            sample_rate: self.sample_rate,
            channels: self.channels,
            bitrate: self.bitrate,
            genre: self.genre.clone(),
            year: None,
            track_number: None,
            cover_art: None,
        })
    }

    fn state(&self) -> SourceState {
        self.state
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            can_seek: false,
            has_duration: false,
            has_dynamic_metadata: true,
            is_network_source: true,
        }
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBuffer>, AudioError> {
        if self.state == SourceState::Error {
            return Ok(None);
        }
        self.state = SourceState::Playing;

        let deadline =
            std::time::Instant::now() + Duration::from_secs_f64(MAX_DECODE_LOOP_SECS);

        match &mut self.pipeline {
            DecodePipeline::Symphonia {
                format,
                decoder,
                track_id,
            } => {
                let tid = *track_id;
                loop {
                    if std::time::Instant::now() > deadline {
                        return yield_or_give_up(
                            &mut self.consecutive_empty,
                            &mut self.state,
                            self.sample_rate,
                            self.channels,
                        );
                    }
                    match format.next_packet() {
                        Ok(packet) => {
                            if packet.track_id() != tid {
                                continue;
                            }
                            if let Some(buf) = try_decode(
                                decoder.as_mut(),
                                &packet,
                                &mut self.sample_buf,
                                &mut self.elapsed,
                                &mut self.consecutive_empty,
                            ) {
                                return Ok(Some(buf));
                            }
                            continue;
                        }
                        Err(SymphoniaError::IoError(ref e))
                            if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                        {
                            if !self.reader_alive.load(Ordering::Relaxed) {
                                log::warn!("[radio] reader thread exited — ending stream");
                                self.state = SourceState::Error;
                                return Ok(None);
                            }
                            if !self.connected.load(Ordering::Relaxed) {
                                return Ok(Some(make_silence(
                                    self.sample_rate,
                                    self.channels,
                                )));
                            }
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(e) => {
                            log::error!("[radio] stream error: {e}");
                            if !self.reader_alive.load(Ordering::Relaxed) {
                                self.state = SourceState::Error;
                                return Ok(None);
                            }
                            return Ok(Some(make_silence(self.sample_rate, self.channels)));
                        }
                    }
                }
            }

            DecodePipeline::Adts {
                frame_reader,
                decoder,
                timestamp,
            } => loop {
                if std::time::Instant::now() > deadline {
                    return yield_or_give_up(
                        &mut self.consecutive_empty,
                        &mut self.state,
                        self.sample_rate,
                        self.channels,
                    );
                }
                match frame_reader.next_frame() {
                    Ok(Some(data)) => {
                        let ts = *timestamp;
                        *timestamp += AAC_SAMPLES_PER_FRAME;
                        let packet =
                            Packet::new_from_slice(0, ts, AAC_SAMPLES_PER_FRAME, &data);
                        if let Some(buf) = try_decode(
                            decoder.as_mut(),
                            &packet,
                            &mut self.sample_buf,
                            &mut self.elapsed,
                            &mut self.consecutive_empty,
                        ) {
                            return Ok(Some(buf));
                        }
                        continue;
                    }
                    Ok(None) | Err(_) => {
                        if !self.reader_alive.load(Ordering::Relaxed) {
                            log::warn!("[radio] reader thread exited — ending stream");
                            self.state = SourceState::Error;
                            return Ok(None);
                        }
                        if !self.connected.load(Ordering::Relaxed) {
                            return Ok(Some(make_silence(
                                self.sample_rate,
                                self.channels,
                            )));
                        }
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                }
            },
        }
    }

    fn seek(&mut self, _position: Duration) -> Result<(), AudioError> {
        Err(AudioError::SeekNotSupported)
    }

    fn position(&self) -> Option<Duration> {
        Some(self.elapsed)
    }
}

// ---------------------------------------------------------------------------
// Pipeline builders
// ---------------------------------------------------------------------------

fn build_adts_pipeline(
    consumer: ringbuf::HeapCons<u8>,
    stop_flag: Arc<AtomicBool>,
    reader_alive: Arc<AtomicBool>,
    hdr: &adts::AdtsHeader,
    content_type: &str,
) -> Result<(DecodePipeline, u32, u16), AudioError> {
    let sample_rate = hdr.sample_rate;
    let channels = hdr.channels as u16;

    // Note: HE-AAC (AAC+) uses SBR which ideally doubles the output rate.
    // However, Symphonia's AAC decoder does NOT implement SBR — it decodes
    // only the base AAC-LC layer and outputs at the ADTS base rate. Using
    // the base rate here is correct; the audio will play but without the
    // SBR high-frequency enhancement.
    if content_type.to_lowercase().contains("aacp") && sample_rate <= 24000 {
        log::info!(
            "[radio] HE-AAC stream at {}Hz (SBR not supported by decoder, \
             playing base layer only)",
            sample_rate,
        );
    }

    let asc = adts::build_audio_specific_config(hdr);

    let mut params = CodecParameters::new();
    params.for_codec(CODEC_TYPE_AAC).with_sample_rate(sample_rate);

    let ch_layout = match hdr.channels {
        1 => symphonia::core::audio::Channels::FRONT_CENTRE,
        _ => {
            symphonia::core::audio::Channels::FRONT_LEFT
                | symphonia::core::audio::Channels::FRONT_RIGHT
        }
    };
    params.with_channels(ch_layout).with_extra_data(asc);

    let decoder = crate::audio::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| {
            AudioError::UnsupportedFormat(format!(
                "AAC decoder unavailable (content-type: {content_type}): {e}"
            ))
        })?;

    let buf_reader = StreamBufReader::new(consumer, stop_flag, reader_alive);
    let frame_reader = AdtsFrameReader::new(buf_reader);

    Ok((
        DecodePipeline::Adts {
            frame_reader,
            decoder,
            timestamp: 0,
        },
        sample_rate,
        channels,
    ))
}

fn build_symphonia_pipeline(
    consumer: ringbuf::HeapCons<u8>,
    stop_flag: Arc<AtomicBool>,
    reader_alive: Arc<AtomicBool>,
    content_type: &str,
) -> Result<(DecodePipeline, u32, u16), AudioError> {
    let buf_reader = StreamBufReader::new(consumer, stop_flag, reader_alive);
    let mss = MediaSourceStream::new(Box::new(buf_reader), Default::default());

    let mut hint = Hint::new();
    let ct = content_type.to_lowercase();
    if ct.contains("mpeg") && !ct.contains("mpegurl") {
        hint.with_extension("mp3");
    } else if ct.contains("ogg") {
        hint.with_extension("ogg");
    } else if ct.contains("flac") {
        hint.with_extension("flac");
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| {
            AudioError::Decode(format!(
                "could not identify stream format (content-type: {content_type}): {e}"
            ))
        })?;

    let format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(AudioError::NoTrack)?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| AudioError::Decode("no sample rate in stream codec params".into()))?;
    let channels = codec_params.channels.map(|ch| ch.count() as u16).unwrap_or(2);

    let decoder = crate::audio::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| {
            AudioError::UnsupportedFormat(format!(
                "unsupported codec in stream (content-type: {content_type}): {e}"
            ))
        })?;

    Ok((
        DecodePipeline::Symphonia {
            format,
            decoder,
            track_id,
        },
        sample_rate,
        channels,
    ))
}

#[allow(dead_code)]
fn codec_name(ct: CodecType) -> &'static str {
    match ct {
        CODEC_TYPE_MP3 => "MP3",
        CODEC_TYPE_AAC => "AAC",
        CODEC_TYPE_VORBIS => "Vorbis",
        CODEC_TYPE_FLAC => "FLAC",
        CODEC_TYPE_ALAC => "ALAC",
        CODEC_TYPE_NULL => "null",
        _ => "unknown",
    }
}
