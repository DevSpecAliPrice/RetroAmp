//! Sample rate conversion using rubato.
//!
//! Sits in the audio pipeline between the source and the EQ. When the source's
//! native sample rate differs from the output device's rate, the resampler
//! converts so that downstream processing (EQ, FFT, output) always operates
//! at the output rate.

use audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};

use crate::audio::error::AudioError;

/// Wraps rubato's FFT resampler with interleaved I/O and internal buffering.
///
/// Accepts variable-length interleaved f32 input, handles the chunking that
/// rubato requires internally, and returns resampled interleaved f32 output.
pub struct AudioResampler {
    resampler: Fft<f32>,
    channels: usize,
    chunk_size: usize,
    /// Per-channel input accumulation buffer.
    input_buf: Vec<Vec<f32>>,
}

impl AudioResampler {
    /// Create a new resampler for converting from `from_rate` to `to_rate`.
    pub fn new(from_rate: u32, to_rate: u32, channels: u16) -> Result<Self, AudioError> {
        let channels = channels as usize;
        let chunk_size = 1024;

        let resampler = Fft::<f32>::new(
            from_rate as usize,
            to_rate as usize,
            chunk_size,
            2, // sub-chunks for better frequency resolution
            channels,
            FixedSync::Input,
        )
        .map_err(|e| AudioError::Output(format!("failed to create resampler: {e}")))?;

        Ok(Self {
            resampler,
            channels,
            chunk_size,
            input_buf: vec![Vec::new(); channels],
        })
    }

    /// Resample a buffer of interleaved f32 samples.
    ///
    /// Accumulates input internally and produces output whenever full chunks
    /// have been processed. May return an empty Vec if not enough input has
    /// accumulated yet.
    pub fn process(&mut self, interleaved_input: &[f32]) -> Result<Vec<f32>, AudioError> {
        // De-interleave input into per-channel buffers.
        let frame_count = interleaved_input.len() / self.channels;
        for frame in 0..frame_count {
            for ch in 0..self.channels {
                self.input_buf[ch].push(interleaved_input[frame * self.channels + ch]);
            }
        }

        let mut output_interleaved: Vec<f32> = Vec::new();

        // Process as many complete chunks as we have accumulated.
        while self.input_buf[0].len() >= self.chunk_size {
            // Build per-channel slices of exactly chunk_size for this iteration,
            // then drain them from the accumulation buffer afterwards.
            let chunk_vecs: Vec<Vec<f32>> = self
                .input_buf
                .iter()
                .map(|ch_buf| ch_buf[..self.chunk_size].to_vec())
                .collect();

            // Drain consumed frames.
            for ch_buf in &mut self.input_buf {
                ch_buf.drain(..self.chunk_size);
            }

            // Wrap in rubato's adapter.
            let adapter =
                SequentialSliceOfVecs::new(&chunk_vecs, self.channels, self.chunk_size)
                    .map_err(|e| AudioError::Output(format!("adapter error: {e}")))?;

            // Run the resampler.
            let resampled = self
                .resampler
                .process(&adapter, 0, None)
                .map_err(|e| AudioError::Output(format!("resample error: {e}")))?;

            // Extract the interleaved data from the output.
            let data = resampled.take_data();
            output_interleaved.extend_from_slice(&data);
        }

        Ok(output_interleaved)
    }
}
