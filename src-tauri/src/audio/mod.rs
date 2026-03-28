//! Audio subsystem — the core audio pipeline and source abstraction.
//!
//! This module owns all audio processing:
//!   - `source`: The AudioSource trait and types (all sources implement this)
//!   - `engine`: The audio engine that orchestrates playback
//!   - `eq`: 10-band biquad equalizer
//!   - `fft`: FFT analysis for visualisation data
//!   - `output`: CPAL audio output
//!   - `error`: Error types for the audio subsystem

pub mod adts;
pub mod buffer;
pub mod engine;
pub mod eq;
pub mod error;
pub mod fft;
pub mod icy;
pub mod local;
pub mod playlist_parser;
pub mod output;
pub mod radio;
pub mod recorder;
pub mod resample;
pub mod source;
pub mod spotify;
pub mod stream_reader;
