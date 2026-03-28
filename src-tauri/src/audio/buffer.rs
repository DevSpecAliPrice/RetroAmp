//! In-memory audio source — plays recorded radio tracks from a `Vec<u8>` buffer.
//!
//! `BufferSource` implements `AudioSource` backed by a `Cursor<Vec<u8>>` instead
//! of an HTTP stream. It reuses the same dual decode pipeline (Symphonia probe
//! vs. ADTS) as `RadioSource`, but is seekable and has a finite duration.

use std::io::{self, Cursor, Read, Seek};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{
    CodecParameters, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_NULL,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, Packet};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::adts::{self, AdtsFrameReader};
use crate::audio::error::AudioError;
use crate::audio::recorder::TrackMeta;
use crate::audio::source::{
    AudioBuffer, AudioSource, SourceCapabilities, SourceState, TrackMetadata,
};

/// Samples per AAC frame (LC-AAC = 1024).
const AAC_SAMPLES_PER_FRAME: u64 = 1024;

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
    Adts {
        frame_reader: AdtsFrameReader<Cursor<Vec<u8>>>,
        decoder: Box<dyn symphonia::core::codecs::Decoder>,
        timestamp: u64,
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
            build_adts_pipeline(data, content_type)?
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

            DecodePipeline::Adts {
                frame_reader,
                decoder,
                timestamp,
            } => loop {
                match frame_reader.next_frame() {
                    Ok(Some(data)) => {
                        let ts = *timestamp;
                        *timestamp += AAC_SAMPLES_PER_FRAME;
                        let packet =
                            Packet::new_from_slice(0, ts, AAC_SAMPLES_PER_FRAME, &data);
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
                    Ok(None) => {
                        self.state = SourceState::Finished;
                        return Ok(None);
                    }
                    Err(_) => {
                        self.state = SourceState::Finished;
                        return Ok(None);
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

fn build_adts_pipeline(
    data: Vec<u8>,
    content_type: &str,
) -> Result<(DecodePipeline, u32, u16), AudioError> {
    let hdr = adts::detect_adts(&data).ok_or_else(|| {
        AudioError::Decode("no valid ADTS header found in buffer".into())
    })?;

    let sample_rate = hdr.sample_rate;
    let channels = hdr.channels as u16;
    let asc = adts::build_audio_specific_config(&hdr);

    let mut params = CodecParameters::new();
    params
        .for_codec(CODEC_TYPE_AAC)
        .with_sample_rate(sample_rate);

    let ch_layout = match hdr.channels {
        1 => symphonia::core::audio::Channels::FRONT_CENTRE,
        _ => {
            symphonia::core::audio::Channels::FRONT_LEFT
                | symphonia::core::audio::Channels::FRONT_RIGHT
        }
    };
    params.with_channels(ch_layout).with_extra_data(asc);

    let decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| {
            AudioError::UnsupportedFormat(format!(
                "AAC decoder unavailable (content-type: {content_type}): {e}"
            ))
        })?;

    let cursor = Cursor::new(data);
    let frame_reader = AdtsFrameReader::new(cursor);

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

    let decoder = symphonia::default::get_codecs()
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
