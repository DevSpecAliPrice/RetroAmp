//! 10-band biquad equalizer operating on the audio pipeline.
//!
//! Each band is a second-order IIR (biquad) peaking EQ filter. The 10 centre
//! frequencies match the classic Winamp bands:
//!   60, 170, 310, 600, 1k, 3k, 6k, 12k, 14k, 16k Hz
//!
//! Gains are adjustable per-band in the range [-12.0, +12.0] dB. A gain of
//! 0.0 dB means the band passes through unmodified.

use serde::{Deserialize, Serialize};

/// The 10 standard Winamp EQ centre frequencies in Hz.
pub const BAND_FREQUENCIES: [f32; 10] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];

/// Default Q factor for peaking EQ filters. This value produces the gentle,
/// overlapping curves characteristic of a graphic equalizer.
const DEFAULT_Q: f32 = 1.4;

/// Gain limits in dB.
const MIN_GAIN_DB: f32 = -12.0;
const MAX_GAIN_DB: f32 = 12.0;

/// Coefficients for a single second-order biquad filter.
#[derive(Debug, Clone, Copy)]
struct BiquadCoefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// Per-channel state for a single biquad filter (two samples of history).
#[derive(Debug, Clone, Copy, Default)]
struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

/// A single EQ band: coefficients + per-channel filter state.
#[derive(Debug, Clone)]
struct EqBand {
    frequency: f32,
    gain_db: f32,
    coefficients: BiquadCoefficients,
    /// One state per channel (index 0 = left, 1 = right, etc.)
    state: Vec<BiquadState>,
}

/// The user-facing gain settings for the EQ, serializable for persistence
/// and for sending to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqSettings {
    /// Per-band gains in dB. Length must be 10.
    pub gains: [f32; 10],
    /// Whether the EQ is enabled. When disabled, audio passes through
    /// unmodified without processing overhead.
    pub enabled: bool,
    /// Preamp gain in dB, applied before the EQ bands.
    pub preamp: f32,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            gains: [0.0; 10],
            enabled: true,
            preamp: 0.0,
        }
    }
}

/// The 10-band equalizer processor.
///
/// Constructed with a sample rate and channel count. Call `process()` to
/// apply EQ to a buffer of interleaved f32 samples in-place.
pub struct Equalizer {
    bands: Vec<EqBand>,
    sample_rate: f32,
    channels: u16,
    enabled: bool,
    preamp_linear: f32,
}

impl Equalizer {
    /// Create a new equalizer for the given sample rate and channel count.
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        let sample_rate_f = sample_rate as f32;
        let bands = BAND_FREQUENCIES
            .iter()
            .map(|&freq| {
                let coefficients = compute_peaking_eq(sample_rate_f, freq, DEFAULT_Q, 0.0);
                EqBand {
                    frequency: freq,
                    gain_db: 0.0,
                    coefficients,
                    state: vec![BiquadState::default(); channels as usize],
                }
            })
            .collect();

        Self {
            bands,
            sample_rate: sample_rate_f,
            channels,
            enabled: true,
            preamp_linear: 1.0,
        }
    }

    /// Apply new EQ settings. Recomputes filter coefficients for any bands
    /// whose gain has changed.
    pub fn apply_settings(&mut self, settings: &EqSettings) {
        self.enabled = settings.enabled;
        self.preamp_linear = db_to_linear(settings.preamp);

        for (i, band) in self.bands.iter_mut().enumerate() {
            let new_gain = settings.gains[i].clamp(MIN_GAIN_DB, MAX_GAIN_DB);
            if (new_gain - band.gain_db).abs() > f32::EPSILON {
                band.gain_db = new_gain;
                band.coefficients =
                    compute_peaking_eq(self.sample_rate, band.frequency, DEFAULT_Q, new_gain);
            }
        }
    }

    /// Reconfigure the equalizer for a new sample rate and/or channel count.
    /// This recomputes all filter coefficients and resets filter state.
    pub fn reconfigure(&mut self, sample_rate: u32, channels: u16) {
        self.sample_rate = sample_rate as f32;
        self.channels = channels;

        for band in &mut self.bands {
            band.coefficients =
                compute_peaking_eq(self.sample_rate, band.frequency, DEFAULT_Q, band.gain_db);
            band.state = vec![BiquadState::default(); channels as usize];
        }
    }

    /// Process a buffer of interleaved f32 samples in-place.
    ///
    /// If the EQ is disabled, this is a no-op.
    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.enabled {
            return;
        }

        let channels = self.channels as usize;

        // Apply preamp
        if (self.preamp_linear - 1.0).abs() > f32::EPSILON {
            for sample in samples.iter_mut() {
                *sample *= self.preamp_linear;
            }
        }

        // Apply each EQ band in series
        for band in &mut self.bands {
            if band.gain_db.abs() < 0.01 {
                // Skip bands at ~0 dB — they're unity gain
                continue;
            }

            let c = &band.coefficients;
            for frame_start in (0..samples.len()).step_by(channels) {
                for ch in 0..channels {
                    let idx = frame_start + ch;
                    if idx >= samples.len() {
                        break;
                    }
                    let state = &mut band.state[ch];
                    let x0 = samples[idx];
                    let y0 = c.b0 * x0 + c.b1 * state.x1 + c.b2 * state.x2
                        - c.a1 * state.y1
                        - c.a2 * state.y2;
                    state.x2 = state.x1;
                    state.x1 = x0;
                    state.y2 = state.y1;
                    state.y1 = y0;
                    samples[idx] = y0;
                }
            }
        }
    }
}

/// Compute biquad coefficients for a peaking EQ filter.
///
/// Reference: Audio EQ Cookbook by Robert Bristow-Johnson.
fn compute_peaking_eq(sample_rate: f32, freq: f32, q: f32, gain_db: f32) -> BiquadCoefficients {
    let a = db_to_linear(gain_db / 2.0);
    let w0 = 2.0 * std::f32::consts::PI * freq / sample_rate;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = sin_w0 / (2.0 * q);

    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cos_w0;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cos_w0;
    let a2 = 1.0 - alpha / a;

    // Normalise by a0
    BiquadCoefficients {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: a1 / a0,
        a2: a2 / a0,
    }
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
