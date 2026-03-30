//! FFT computation for spectrum analysis and visualisation data.
//!
//! Runs on the audio thread as part of the pipeline. Computes an FFT on each
//! buffer of audio, extracts magnitude data, and packages it for delivery to
//! the WebView via Tauri events.
//!
//! The frontend receives a `Vec<f32>` of frequency bin magnitudes at ~60fps,
//! which drives both the in-skin spectrum analyser and Butterchurn (Milkdrop).

use rustfft::{num_complex::Complex, FftPlanner};
use serde::Serialize;

/// Number of FFT bins. 1024 gives 512 usable frequency bins — more than
/// enough for a spectrum analyser (Winamp used ~76 bars) and Butterchurn.
const FFT_SIZE: usize = 1024;

/// Data payload sent to the frontend for visualisation.
#[derive(Debug, Clone, Serialize)]
pub struct FftData {
    /// Magnitude values for each frequency bin, normalised to [0.0, 1.0].
    /// Length is FFT_SIZE / 2 (512 bins for 1024-point FFT).
    pub magnitudes: Vec<f32>,
    /// Time-domain waveform (mono, pre-window) for visualisers that need
    /// oscilloscope-style data. Length is FFT_SIZE (1024 samples, [-1.0, 1.0]).
    pub waveform: Vec<f32>,
    /// The sample rate the FFT was computed at, so the frontend can map
    /// bin indices to frequencies if needed.
    pub sample_rate: u32,
}

/// The FFT analyser. Maintains a plan and scratch buffer so we don't
/// re-allocate on every frame.
pub struct FftAnalyser {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    /// Hann window coefficients, precomputed.
    window: Vec<f32>,
    /// Scratch buffer for FFT input (windowed samples).
    input_buffer: Vec<Complex<f32>>,
    /// Scratch buffer used internally by rustfft.
    scratch: Vec<Complex<f32>>,
    /// The most recently computed magnitudes.
    current_magnitudes: Vec<f32>,
    /// Pre-window mono samples from the last processed buffer (for waveform visualisation).
    current_waveform: Vec<f32>,
    /// Smoothing factor for magnitude values (0.0 = no smoothing, 1.0 = frozen).
    /// A small amount of smoothing prevents the spectrum from flickering.
    smoothing: f32,
}

impl FftAnalyser {
    pub fn new() -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let scratch_len = fft.get_inplace_scratch_len();

        // Precompute Hann window
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / FFT_SIZE as f32).cos())
            })
            .collect();

        Self {
            fft,
            window,
            input_buffer: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            scratch: vec![Complex::new(0.0, 0.0); scratch_len],
            current_magnitudes: vec![0.0; FFT_SIZE / 2],
            current_waveform: vec![0.0; FFT_SIZE],
            smoothing: 0.3,
        }
    }

    /// Process a buffer of interleaved audio samples and update the internal
    /// magnitude state.
    ///
    /// For multi-channel audio, this mixes down to mono before computing FFT.
    /// Only the most recent FFT_SIZE samples in the buffer are used.
    pub fn process(&mut self, samples: &[f32], channels: u16) {
        let channels = channels as usize;
        if channels == 0 || samples.is_empty() {
            return;
        }

        let frame_count = samples.len() / channels;
        if frame_count == 0 {
            return;
        }

        // Mix down to mono and take the last FFT_SIZE frames.
        // Store the raw mono samples (pre-window) for waveform visualisation,
        // then apply the Hann window for FFT input.
        let start_frame = frame_count.saturating_sub(FFT_SIZE);
        for i in 0..FFT_SIZE {
            let frame_idx = start_frame + i;
            if frame_idx < frame_count {
                let mut mono = 0.0_f32;
                for ch in 0..channels {
                    mono += samples[frame_idx * channels + ch];
                }
                mono /= channels as f32;
                self.current_waveform[i] = mono;
                self.input_buffer[i] = Complex::new(mono * self.window[i], 0.0);
            } else {
                self.current_waveform[i] = 0.0;
                self.input_buffer[i] = Complex::new(0.0, 0.0);
            }
        }

        // Run FFT in-place
        self.fft
            .process_with_scratch(&mut self.input_buffer, &mut self.scratch);

        // Extract magnitudes from the first half (positive frequencies) and
        // apply smoothing
        let half = FFT_SIZE / 2;
        let scale = 2.0 / FFT_SIZE as f32;
        for i in 0..half {
            let raw_magnitude = self.input_buffer[i].norm() * scale;
            self.current_magnitudes[i] = self.current_magnitudes[i] * self.smoothing
                + raw_magnitude * (1.0 - self.smoothing);
        }
    }

    /// Return the current FFT data, ready to send to the frontend.
    pub fn current_data(&self, sample_rate: u32) -> FftData {
        FftData {
            magnitudes: self.current_magnitudes.clone(),
            waveform: self.current_waveform.clone(),
            sample_rate,
        }
    }
}
