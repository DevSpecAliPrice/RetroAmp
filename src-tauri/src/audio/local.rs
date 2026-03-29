//! Local file audio source — decodes audio files from disk via Symphonia.
//!
//! This is the first and most fundamental AudioSource implementation. It
//! handles MP3, FLAC, AAC, OGG Vorbis, WAV, ALAC, and any other format
//! Symphonia supports.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey, Value};
use symphonia::core::probe::Hint;
use symphonia::core::units::TimeBase;

use crate::audio::error::AudioError;
use crate::audio::source::{AudioBuffer, AudioSource, SourceCapabilities, SourceState, TrackMetadata};

pub struct LocalFileSource {
    #[allow(dead_code)]
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    channels: u16,
    duration: Option<Duration>,
    metadata: TrackMetadata,
    state: SourceState,
    time_base: Option<TimeBase>,
    /// Timestamp of the last decoded packet, in TimeBase units.
    current_ts: u64,
    /// Reusable sample buffer — avoids allocation per decode call.
    sample_buf: Option<SampleBuffer<f32>>,
}

impl LocalFileSource {
    /// Open a local audio file and prepare it for decoding.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AudioError> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;

        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        // Provide a hint based on the file extension so Symphonia can skip
        // format detection when possible.
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| AudioError::Decode(format!("failed to probe {}: {e}", path.display())))?;

        let mut probed = probed;

        // Extract metadata tags from the probe result (e.g. ID3 headers).
        let (probe_tags, probe_visuals) = extract_from_metadata(probed.metadata.get());

        let mut format = probed.format;

        // Find the first audio track.
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(AudioError::NoTrack)?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let time_base = codec_params.time_base;

        // Extract sample rate and channel count from the codec parameters.
        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| AudioError::Decode("no sample rate in codec params".into()))?;

        let channels = codec_params
            .channels
            .map(|ch| ch.count() as u16)
            .unwrap_or(2);

        // Compute duration from the total number of frames if available.
        let duration = match (codec_params.n_frames, time_base) {
            (Some(n_frames), Some(tb)) => {
                let time = tb.calc_time(n_frames);
                Some(
                    Duration::from_secs(time.seconds)
                        + Duration::from_nanos((time.frac * 1_000_000_000.0) as u64),
                )
            }
            _ => None,
        };

        // Also check format-level metadata (some formats embed tags in the
        // stream rather than in headers).
        let (format_tags, format_visuals) = extract_from_metadata(Some(format.metadata()));

        // Merge: probe tags take priority, then format tags.
        let all_tags = if probe_tags.is_empty() {
            format_tags
        } else {
            probe_tags
        };
        let all_visuals = if probe_visuals.is_empty() {
            format_visuals
        } else {
            probe_visuals
        };

        // Compute average bitrate from file size and duration.
        let bitrate = match (std::fs::metadata(&path).ok().map(|m| m.len()), duration) {
            (Some(size), Some(dur)) if dur.as_secs_f64() > 0.0 => {
                Some((size as f64 * 8.0 / dur.as_secs_f64() / 1000.0) as u32)
            }
            _ => None,
        };

        let mut metadata = build_metadata(&all_tags, &all_visuals, sample_rate, channels, duration, bitrate);

        // Fall back to the filename as title when no tags are embedded (e.g. WAV).
        if metadata.title.is_none() {
            metadata.title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string());
        }

        // Create the decoder.
        let decoder = crate::audio::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| {
                AudioError::UnsupportedFormat(format!(
                    "no decoder for {}: {e}",
                    path.display()
                ))
            })?;

        Ok(Self {
            path,
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
            duration,
            metadata,
            state: SourceState::Ready,
            time_base,
            current_ts: 0,
            sample_buf: None,
        })
    }
}

impl AudioSource for LocalFileSource {
    fn metadata(&self) -> Result<TrackMetadata, AudioError> {
        Ok(self.metadata.clone())
    }

    fn state(&self) -> SourceState {
        self.state
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            can_seek: true,
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

        loop {
            // Read the next packet from the format reader.
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // End of stream.
                    self.state = SourceState::Finished;
                    return Ok(None);
                }
                Err(e) => {
                    self.state = SourceState::Error;
                    return Err(AudioError::Decode(format!("packet read error: {e}")));
                }
            };

            // Skip packets that don't belong to our audio track.
            if packet.track_id() != self.track_id {
                continue;
            }

            // Track the current timestamp for position reporting.
            self.current_ts = packet.ts();

            // Decode the packet.
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                Err(SymphoniaError::DecodeError(e)) => {
                    // Decode errors on individual packets are recoverable —
                    // skip and try the next packet.
                    log::warn!("decode error (skipping packet): {e}");
                    continue;
                }
                Err(e) => {
                    self.state = SourceState::Error;
                    return Err(AudioError::Decode(format!("decoder error: {e}")));
                }
            };

            let spec = *decoded.spec();
            let num_frames = decoded.capacity();

            if num_frames == 0 {
                continue;
            }

            // Ensure the sample buffer exists and has enough capacity.
            let sample_buf = self.sample_buf.get_or_insert_with(|| {
                SampleBuffer::<f32>::new(num_frames as u64, spec)
            });

            // If the spec changed (shouldn't happen within a track, but be safe)
            // or the buffer is too small, recreate it.
            if sample_buf.capacity() < num_frames {
                self.sample_buf = Some(SampleBuffer::<f32>::new(num_frames as u64, spec));
            }

            let sample_buf = self.sample_buf.as_mut().unwrap();

            // Convert decoded audio to interleaved f32.
            sample_buf.copy_interleaved_ref(decoded);

            let samples = sample_buf.samples().to_vec();

            return Ok(Some(AudioBuffer {
                samples,
                sample_rate: self.sample_rate,
                channels: self.channels,
            }));
        }
    }

    fn seek(&mut self, position: Duration) -> Result<(), AudioError> {
        let seconds = position.as_secs();
        let frac = position.subsec_nanos() as f64 / 1_000_000_000.0;

        let seek_to = SeekTo::Time {
            time: symphonia::core::units::Time::new(seconds, frac),
            track_id: Some(self.track_id),
        };

        self.format
            .seek(SeekMode::Coarse, seek_to)
            .map_err(|e| AudioError::Decode(format!("seek error: {e}")))?;

        // Reset the decoder to clear any buffered state.
        self.decoder.reset();

        self.state = SourceState::Playing;

        Ok(())
    }

    fn position(&self) -> Option<Duration> {
        self.time_base.map(|tb| {
            let time = tb.calc_time(self.current_ts);
            Duration::from_secs(time.seconds)
                + Duration::from_nanos((time.frac * 1_000_000_000.0) as u64)
        })
    }
}

// ---------------------------------------------------------------------------
// Metadata extraction helpers
// ---------------------------------------------------------------------------

struct ExtractedTag {
    std_key: Option<StandardTagKey>,
    value: String,
}

/// Extract tags and visuals from a Symphonia Metadata view.
/// Works for both probe metadata and format-reader metadata.
fn extract_from_metadata(
    metadata: Option<symphonia::core::meta::Metadata<'_>>,
) -> (Vec<ExtractedTag>, Vec<Vec<u8>>) {
    let Some(metadata) = metadata else {
        return (vec![], vec![]);
    };

    let Some(rev) = metadata.current() else {
        return (vec![], vec![]);
    };

    let tags = rev
        .tags()
        .iter()
        .map(|tag| ExtractedTag {
            std_key: tag.std_key,
            value: tag_value_to_string(&tag.value),
        })
        .collect();

    let visuals = rev.visuals().iter().map(|v| v.data.to_vec()).collect();

    (tags, visuals)
}

fn tag_value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::UnsignedInt(n) => n.to_string(),
        Value::SignedInt(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Flag => String::new(),
        Value::Binary(_) => String::new(),
    }
}

fn build_metadata(
    tags: &[ExtractedTag],
    visuals: &[Vec<u8>],
    sample_rate: u32,
    channels: u16,
    duration: Option<Duration>,
    bitrate: Option<u32>,
) -> TrackMetadata {
    let mut metadata = TrackMetadata {
        title: None,
        artist: None,
        album: None,
        duration,
        sample_rate,
        channels,
        bitrate,
        genre: None,
        year: None,
        track_number: None,
        cover_art: None,
    };

    for tag in tags {
        if let Some(std_key) = tag.std_key {
            match std_key {
                StandardTagKey::TrackTitle => {
                    metadata.title = Some(tag.value.clone());
                }
                StandardTagKey::Artist | StandardTagKey::AlbumArtist => {
                    // Prefer Artist over AlbumArtist, but take either.
                    if metadata.artist.is_none() || std_key == StandardTagKey::Artist {
                        metadata.artist = Some(tag.value.clone());
                    }
                }
                StandardTagKey::Album => {
                    metadata.album = Some(tag.value.clone());
                }
                StandardTagKey::Genre => {
                    metadata.genre = Some(tag.value.clone());
                }
                StandardTagKey::Date | StandardTagKey::OriginalDate => {
                    // Extract the 4-digit year from a date string (e.g. "2024-01-15").
                    if metadata.year.is_none() {
                        if let Some(year_str) = tag.value.get(..4) {
                            metadata.year = year_str.parse::<u32>().ok();
                        }
                    }
                }
                StandardTagKey::TrackNumber => {
                    // Handle "3/12" format — take just the track number.
                    let num_str = tag.value.split('/').next().unwrap_or("");
                    metadata.track_number = num_str.trim().parse::<u32>().ok();
                }
                _ => {}
            }
        }
    }

    // Take the first visual (cover art) if available.
    if let Some(art) = visuals.first() {
        metadata.cover_art = Some(art.clone());
    }

    metadata
}
