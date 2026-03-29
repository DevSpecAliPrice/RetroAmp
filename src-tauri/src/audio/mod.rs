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
pub mod opus;
pub mod playlist_parser;
pub mod output;
pub mod radio;
pub mod recorder;
pub mod resample;
pub mod source;
pub mod spotify;
pub mod stream_reader;
pub mod youtube;

use std::sync::OnceLock;
use symphonia::core::codecs::CodecRegistry;

/// Global codec registry with all Symphonia codecs + our Opus decoder.
///
/// Use this instead of `symphonia::default::get_codecs()` everywhere
/// to ensure Opus support is available for all audio sources.
pub fn get_codecs() -> &'static CodecRegistry {
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        // Register all built-in Symphonia codecs.
        symphonia::default::register_enabled_codecs(&mut registry);
        // Register our Opus decoder (wraps libopus via audiopus).
        registry.register_all::<opus::OpusDecoder>();
        registry
    })
}
