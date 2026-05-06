//! In-memory audio source — plays recorded radio tracks from a `Vec<u8>` buffer.
//!
//! `BufferSource` implements `AudioSource` backed by a `Cursor<Vec<u8>>` instead
//! of an HTTP stream. It reuses the same dual decode pipeline (Symphonia probe
//! vs. fdk-aac for ADTS) as `RadioSource`, but is seekable and has a finite
//! duration.

use std::io::{self, Cursor, Read, Seek};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, Packet};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::error::AudioError;
use crate::audio::recorder::TrackMeta;
use crate::audio::source::{
    AudioBuffer, AudioSource, SourceCapabilities, SourceState, TrackMetadata,
};

/// Largest output buffer fdk-aac can request — see `radio.rs` for rationale.
const FDK_PCM_BUF_SAMPLES: usize = 8 * 2048;

// ---------------------------------------------------------------------------
// MemoryMediaSource — Cursor wrapper implementing Symphonia's MediaSource
// ---------------------------------------------------------------------------

struct MemoryMediaSource(Cursor<Vec<u8>>);

impl Read for MemoryMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for MemoryMediaSource {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }
}

impl symphonia::core::io::MediaSource for MemoryMediaSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.0.get_ref().len() as u64)
    }
}

// ---------------------------------------------------------------------------
// Decode pipeline (duplicated from radio.rs to avoid coupling issues)
// ---------------------------------------------------------------------------

enum DecodePipeline {
    Symphonia {
        format: Box<dyn FormatReader>,
        decoder: Box<dyn symphonia::core::codecs::Decoder>,
        track_id: u32,
    },
    FdkAac {
        cursor: Cursor<Vec<u8>>,
        decoder: fdk_aac::dec::Decoder,
        pending_input: Vec<u8>,
        pcm_buf: Vec<i16>,
        initial_buffer: Option<AudioBuffer>,
    },
}

// ---------------------------------------------------------------------------
// BufferSource
// ---------------------------------------------------------------------------

pub struct BufferSource {
    pipeline: DecodePipeline,
    sample_rate: u32,
    channels: u16,
    state: SourceState,
    elapsed: Duration,
    duration: Option<Duration>,
    metadata: TrackMetadata,
    sample_buf: Option<SampleBuffer<f32>>,
}

impl BufferSource {
    /// Create a source from recorded track data.
    pub fn from_track_data(
        data: Vec<u8>,
        content_type: &str,
        is_adts: bool,
        bitrate: Option<u32>,
        meta: TrackMeta,
    ) -> Result<Self, AudioError> {
        // Estimate duration from byte size and bitrate.
        let duration = bitrate.and_then(|br| {
            if br > 0 {
                Some(Duration::from_secs_f64(
                    data.len() as f64 / (br as f64 * 1000.0 / 8.0),
                ))
            } else {
                None
            }
        });

        let (pipeline, sample_rate, channels) = if is_adts {
            build_fdk_aac_pipeline(data, content_type)?
        } else {
            build_symphonia_pipeline(data, content_type)?
        };

        let metadata = TrackMetadata {
            title: meta.title,
            artist: meta.artist,
            album: meta.station_name,
            duration,
            sample_rate,
            channels,
            bitrate,
            genre: None,
            year: None,
            track_number: None,
            cover_art: None,
        };

        Ok(Self {
            pipeline,
            sample_rate,
            channels,
            state: SourceState::Ready,
            elapsed: Duration::ZERO,
            duration,
            metadata,
            sample_buf: None,
        })
    }
}

impl AudioSource for BufferSource {
    fn metadata(&self) -> Result<TrackMetadata, AudioError> {
        Ok(self.metadata.clone())
    }

    fn state(&self) -> SourceState {
        self.state
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            can_seek: false, // Seeking in ADTS/streamed MP3 is complex; skip for now.
            has_duration: self.duration.is_some(),
            has_dynamic_metadata: false,
            is_network_source: false,
        }
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBuffer>, AudioError> {
        if self.state == SourceState::Finished || self.state == SourceState::Error {
            return Ok(None);
        }
        self.state = SourceState::Playing;

        match &mut self.pipeline {
            DecodePipeline::Symphonia {
                format,
                decoder,
                track_id,
            } => {
                let tid = *track_id;
                loop {
                    match format.next_packet() {
                        Ok(packet) => {
                            if packet.track_id() != tid {
                                continue;
                            }
                            if let Some(buf) = decode_packet(
                                decoder.as_mut(),
                                &packet,
                                &mut self.sample_buf,
                                &mut self.elapsed,
                                self.sample_rate,
                                self.channels,
                            ) {
                                return Ok(Some(buf));
                            }
                            continue;
                        }
                        Err(SymphoniaError::IoError(ref e))
                            if e.kind() == io::ErrorKind::UnexpectedEof =>
                        {
                            self.state = SourceState::Finished;
                            return Ok(None);
                        }
                        Err(_) => {
                            self.state = SourceState::Finished;
                            return Ok(None);
                        }
                    }
                }
            }

            DecodePipeline::FdkAac {
                cursor,
                decoder,
                pending_input,
                pcm_buf,
                initial_buffer,
            } => {
                if let Some(buf) = initial_buffer.take() {
                    self.elapsed += Duration::from_secs_f64(
                        (buf.samples.len() / buf.channels.max(1) as usize) as f64
                            / buf.sample_rate as f64,
                    );
                    return Ok(Some(buf));
                }

                loop {
                    match decoder.decode_frame(pcm_buf.as_mut_slice()) {
                        Ok(()) => {
                            let info = decoder.stream_info();
                            let n_ch = info.numChannels.max(0) as usize;
                            let frames = info.frameSize.max(0) as usize;
                            if n_ch == 0 || frames == 0 {
                                continue;
                            }
                            let n_samples = frames * n_ch;
                            let samples: Vec<f32> = pcm_buf[..n_samples]
                                .iter()
                                .map(|&s| s as f32 / 32768.0)
                                .collect();
                            let actual_rate = info.sampleRate.max(1) as u32;
                            self.elapsed += Duration::from_secs_f64(
                                frames as f64 / actual_rate as f64,
                            );
                            return Ok(Some(AudioBuffer {
                                samples,
                                sample_rate: actual_rate,
                                channels: n_ch as u16,
                            }));
                        }
                        Err(e) if e == fdk_aac::dec::DecoderError::NOT_ENOUGH_BITS
                            || e == fdk_aac::dec::DecoderError::TRANSPORT_SYNC_ERROR =>
                        {
                            if !refill_from_cursor(cursor, decoder, pending_input)? {
                                self.state = SourceState::Finished;
                                return Ok(None);
                            }
                        }
                        Err(e) => {
                            log::debug!("[buffer] fdk-aac: {e}");
                        }
                    }
                }
            }
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
// Helpers
// ---------------------------------------------------------------------------

fn decode_packet(
    decoder: &mut dyn symphonia::core::codecs::Decoder,
    packet: &Packet,
    sample_buf: &mut Option<SampleBuffer<f32>>,
    elapsed: &mut Duration,
    sample_rate: u32,
    channels: u16,
) -> Option<AudioBuffer> {
    let decoded = match decoder.decode(packet) {
        Ok(d) => d,
        Err(_) => return None,
    };

    let spec = *decoded.spec();
    let num_frames = decoded.capacity();
    if num_frames == 0 {
        return None;
    }

    let sbuf = sample_buf.get_or_insert_with(|| {
        SampleBuffer::<f32>::new(num_frames as u64, spec)
    });
    if sbuf.capacity() < num_frames {
        *sample_buf = Some(SampleBuffer::<f32>::new(num_frames as u64, spec));
    }
    let sbuf = sample_buf.as_mut().unwrap();

    sbuf.copy_interleaved_ref(decoded);
    let samples = sbuf.samples().to_vec();

    *elapsed += Duration::from_secs_f64(num_frames as f64 / sample_rate as f64);

    Some(AudioBuffer {
        samples,
        sample_rate,
        channels,
    })
}

// ---------------------------------------------------------------------------
// Pipeline builders
// ---------------------------------------------------------------------------

fn build_fdk_aac_pipeline(
    data: Vec<u8>,
    content_type: &str,
) -> Result<(DecodePipeline, u32, u16), AudioError> {
    let mut cursor = Cursor::new(data);
    let mut decoder = fdk_aac::dec::Decoder::new(fdk_aac::dec::Transport::Adts);

    decoder
        .set_min_output_channels(2)
        .map_err(|e| AudioError::Decode(format!("fdk-aac set_min channels: {e}")))?;
    decoder
        .set_max_output_channels(2)
        .map_err(|e| AudioError::Decode(format!("fdk-aac set_max channels: {e}")))?;

    let mut pending_input: Vec<u8> = Vec::with_capacity(8192);
    let mut pcm_buf: Vec<i16> = vec![0; FDK_PCM_BUF_SAMPLES];

    let mut bytes_fed = 0usize;
    const PROBE_BYTE_BUDGET: usize = 256 * 1024;

    let initial_buffer = loop {
        match decoder.decode_frame(pcm_buf.as_mut_slice()) {
            Ok(()) => {
                let info = decoder.stream_info();
                let n_ch = info.numChannels.max(0) as usize;
                let frames = info.frameSize.max(0) as usize;
                if n_ch == 0 || frames == 0 {
                    bytes_fed += refill_from_cursor_or_err(
                        &mut cursor,
                        &mut decoder,
                        &mut pending_input,
                        bytes_fed,
                        PROBE_BYTE_BUDGET,
                    )?;
                    continue;
                }
                let n_samples = frames * n_ch;
                let samples: Vec<f32> = pcm_buf[..n_samples]
                    .iter()
                    .map(|&s| s as f32 / 32768.0)
                    .collect();
                let sample_rate = info.sampleRate.max(1) as u32;
                let channels = n_ch as u16;
                break AudioBuffer {
                    samples,
                    sample_rate,
                    channels,
                };
            }
            Err(e) if e == fdk_aac::dec::DecoderError::NOT_ENOUGH_BITS
                || e == fdk_aac::dec::DecoderError::TRANSPORT_SYNC_ERROR =>
            {
                bytes_fed += refill_from_cursor_or_err(
                    &mut cursor,
                    &mut decoder,
                    &mut pending_input,
                    bytes_fed,
                    PROBE_BYTE_BUDGET,
                )?;
            }
            Err(e) => {
                log::debug!("[buffer] fdk-aac probe (recoverable): {e}");
                bytes_fed += refill_from_cursor_or_err(
                    &mut cursor,
                    &mut decoder,
                    &mut pending_input,
                    bytes_fed,
                    PROBE_BYTE_BUDGET,
                )
                .map_err(|inner| match inner {
                    AudioError::ConnectionFailed(_) => AudioError::Decode(format!(
                        "fdk-aac probe failed (content-type: {content_type}): {e}"
                    )),
                    other => other,
                })?;
            }
        }
    };

    let sample_rate = initial_buffer.sample_rate;
    let channels = initial_buffer.channels;

    Ok((
        DecodePipeline::FdkAac {
            cursor,
            decoder,
            pending_input,
            pcm_buf,
            initial_buffer: Some(initial_buffer),
        },
        sample_rate,
        channels,
    ))
}

/// Steady-state cursor refill: reads more bytes from `cursor` if the
/// pending buffer is empty, then feeds them to the decoder. Returns
/// `Ok(true)` if any bytes were consumed by the decoder, `Ok(false)` on
/// EOF (cursor exhausted and pending buffer drained).
fn refill_from_cursor(
    cursor: &mut Cursor<Vec<u8>>,
    decoder: &mut fdk_aac::dec::Decoder,
    pending_input: &mut Vec<u8>,
) -> Result<bool, AudioError> {
    if pending_input.is_empty() {
        let mut buf = [0u8; 8192];
        let n = cursor
            .read(&mut buf)
            .map_err(|e| AudioError::Decode(format!("cursor read: {e}")))?;
        if n == 0 {
            return Ok(false);
        }
        pending_input.extend_from_slice(&buf[..n]);
    }
    let consumed = decoder
        .fill(pending_input)
        .map_err(|e| AudioError::Decode(format!("fdk-aac fill: {e}")))?;
    pending_input.drain(..consumed);
    Ok(consumed > 0)
}

/// Probe-time refill — like `refill_from_cursor`, but enforces a byte
/// budget so a corrupt input can't loop forever, and returns an explicit
/// error on EOF (since we expect the probe to succeed on a recorded track).
fn refill_from_cursor_or_err(
    cursor: &mut Cursor<Vec<u8>>,
    decoder: &mut fdk_aac::dec::Decoder,
    pending_input: &mut Vec<u8>,
    bytes_fed_so_far: usize,
    budget: usize,
) -> Result<usize, AudioError> {
    if bytes_fed_so_far > budget {
        return Err(AudioError::Decode(
            "fdk-aac probe ran out of byte budget without a valid frame".into(),
        ));
    }
    if pending_input.is_empty() {
        let mut buf = [0u8; 8192];
        let n = cursor
            .read(&mut buf)
            .map_err(|e| AudioError::Decode(format!("cursor read: {e}")))?;
        if n == 0 {
            return Err(AudioError::ConnectionFailed(
                "buffer ended before fdk-aac could lock onto a frame".into(),
            ));
        }
        pending_input.extend_from_slice(&buf[..n]);
    }
    let consumed = decoder
        .fill(pending_input)
        .map_err(|e| AudioError::Decode(format!("fdk-aac fill: {e}")))?;
    pending_input.drain(..consumed);
    Ok(consumed)
}

fn build_symphonia_pipeline(
    data: Vec<u8>,
    content_type: &str,
) -> Result<(DecodePipeline, u32, u16), AudioError> {
    let source = MemoryMediaSource(Cursor::new(data));
    let mss = MediaSourceStream::new(Box::new(source), Default::default());

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
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| {
            AudioError::Decode(format!(
                "could not identify buffer format (content-type: {content_type}): {e}"
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
        .ok_or_else(|| AudioError::Decode("no sample rate in buffer codec params".into()))?;
    let channels = codec_params
        .channels
        .map(|ch| ch.count() as u16)
        .unwrap_or(2);

    let decoder = crate::audio::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| {
            AudioError::UnsupportedFormat(format!(
                "unsupported codec in buffer (content-type: {content_type}): {e}"
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
