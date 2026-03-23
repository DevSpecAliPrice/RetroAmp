//! Audio subsystem — the core audio pipeline and source abstraction.
//!
//! This module owns all audio processing:
//!   - `source`: The AudioSource trait and types (all sources implement this)
//!   - `engine`: The audio engine that orchestrates playback
//!   - `eq`: 10-band biquad equalizer
//!   - `fft`: FFT analysis for visualisation data
//!   - `output`: CPAL audio output
//!   - `error`: Error types for the audio subsystem

pub mod engine;
pub mod eq;
pub mod error;
pub mod fft;
pub mod local;
pub mod output;
pub mod resample;
pub mod source;
