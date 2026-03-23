//! The AudioSource trait — the core abstraction that all audio sources implement.
//!
//! Local files, internet radio streams, and Spotify each provide a different
//! implementation of this trait. The audio engine consumes AudioSource without
//! knowing or caring which concrete type is behind it.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::audio::error::AudioError;

/// Metadata about a track or stream.
///
/// Not all fields apply to all source types — a radio stream won't have a
/// duration, and a local file won't have a station name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<Duration>,
    pub sample_rate: u32,
    pub channels: u16,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_number: Option<u32>,
    pub cover_art: Option<Vec<u8>>,
}

/// The current state of an audio source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceState {
    /// Source is ready but not yet producing audio.
    Ready,
    /// Source is actively producing audio frames.
    Playing,
    /// Source has reached the end of its content (e.g. end of file).
    /// Not applicable to infinite streams like radio.
    Finished,
    /// Source encountered an error and cannot continue.
    Error,
}

/// A buffer of decoded PCM audio samples.
///
/// Samples are interleaved: for stereo, the layout is [L, R, L, R, ...].
/// All audio flows through the engine as f32 samples normalised to [-1.0, 1.0].
#[derive(Debug)]
pub struct AudioBuffer {
    /// Interleaved f32 samples.
    pub samples: Vec<f32>,
    /// Sample rate of this buffer.
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo, etc.).
    pub channels: u16,
}

/// Describes what a source is capable of.
///
/// Used by the engine to decide what UI controls to expose — e.g. a radio
/// stream can't seek, so the seek bar should be disabled.
#[derive(Debug, Clone, Copy)]
pub struct SourceCapabilities {
    /// Whether the source supports seeking to an arbitrary position.
    pub can_seek: bool,
    /// Whether the source has a known, finite duration.
    pub has_duration: bool,
    /// Whether the source provides metadata that may update during playback
    /// (e.g. ICY metadata on a radio stream).
    pub has_dynamic_metadata: bool,
}

/// The core trait that every audio source must implement.
///
/// The audio engine calls `next_buffer()` in a loop on its audio thread to
/// pull decoded PCM data. Implementations are responsible for decoding their
/// source format into f32 PCM — the engine handles everything from that point
/// on (EQ, FFT, output).
pub trait AudioSource: Send {
    /// Return metadata about the current track or stream.
    fn metadata(&self) -> Result<TrackMetadata, AudioError>;

    /// Return the current state of the source.
    fn state(&self) -> SourceState;

    /// Return the capabilities of this source type.
    fn capabilities(&self) -> SourceCapabilities;

    /// Fill the next buffer of decoded PCM audio.
    ///
    /// Called repeatedly by the audio engine's playback thread. Implementations
    /// should decode enough audio to fill a reasonable buffer (e.g. 1024–4096
    /// frames) and return it. Return `Ok(None)` when the source has no more
    /// data (end of file).
    ///
    /// This method is called on the audio thread — it must not block for long
    /// periods or allocate excessively.
    fn next_buffer(&mut self) -> Result<Option<AudioBuffer>, AudioError>;

    /// Seek to the given position. Returns an error if the source doesn't
    /// support seeking.
    fn seek(&mut self, position: Duration) -> Result<(), AudioError>;

    /// Return the current playback position, if known.
    fn position(&self) -> Option<Duration>;
}
