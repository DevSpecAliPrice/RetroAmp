//! CPAL audio output — sends processed PCM samples to the system audio device.
//!
//! Uses a lock-free ring buffer between the engine thread (producer) and
//! CPAL's real-time audio callback (consumer). The callback must never block —
//! a mutex here would cause priority inversion and audible glitches.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SampleRate, Stream, StreamConfig, SupportedStreamConfigRange};
use ringbuf::{
    traits::{Consumer, Observer, Producer, Split},
    HeapRb,
};

use crate::audio::error::AudioError;

/// Output buffer duration for local files — small for responsive controls.
pub const BUFFER_SECS_LOCAL: f32 = 0.1;
/// Output buffer duration for internet streams — larger to absorb network
/// and scheduling jitter without underruns.
pub const BUFFER_SECS_STREAM: f32 = 0.5;

/// Describes the output device and its configuration.
#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub sample_rate: u32,
    pub channels: u16,
}

/// Handle to the audio output stream.
///
/// The engine thread writes samples via the producer half of a lock-free
/// ring buffer. CPAL's callback reads from the consumer half. No mutex
/// is ever held during audio I/O.
pub struct AudioOutput {
    _stream: Stream,
    producer: ringbuf::HeapProd<f32>,
    config: StreamConfig,
}

impl AudioOutput {
    /// Get the output configuration (sample rate, channels).
    pub fn config(&self) -> OutputConfig {
        OutputConfig {
            sample_rate: self.config.sample_rate.0,
            channels: self.config.channels,
        }
    }

    /// Write processed audio samples to the output buffer.
    /// Returns the number of samples actually written.
    pub fn write(&mut self, samples: &[f32]) -> usize {
        self.producer.push_slice(samples)
    }

    /// How many samples can be written without blocking.
    pub fn free_space(&self) -> usize {
        self.producer.vacant_len()
    }

    /// Fill remaining output buffer with silence to prevent pops when the
    /// stream is about to be dropped/reconfigured. The CPAL callback will
    /// play this silence before the stream is torn down.
    pub fn flush_with_silence(&mut self) {
        let free = self.producer.vacant_len();
        if free > 0 {
            let silence = vec![0.0f32; free];
            self.producer.push_slice(&silence);
        }
    }
}

/// Manages the output device. Lives on the audio thread and can reconfigure
/// the stream to match a source's sample rate.
pub struct OutputManager {
    device: Device,
}

impl OutputManager {
    /// Create a new output manager using the default audio device.
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| AudioError::Output("no output device available".into()))?;

        Ok(Self { device })
    }

    /// Open the output at the device's default sample rate.
    pub fn open_default(&self) -> Result<AudioOutput, AudioError> {
        let supported = self
            .device
            .default_output_config()
            .map_err(|e| AudioError::Output(format!("failed to get output config: {e}")))?;

        self.build_stream(supported.into(), BUFFER_SECS_LOCAL)
    }

    /// Open the output at a specific sample rate and channel count. If the
    /// device doesn't support the rate with at least the requested channels,
    /// returns Err so the caller can fall back to resampling.
    pub fn open_at_rate(
        &self,
        desired_rate: u32,
        desired_channels: u16,
        buffer_secs: f32,
    ) -> Result<AudioOutput, AudioError> {
        let desired = SampleRate(desired_rate);

        let supported_configs: Vec<SupportedStreamConfigRange> = self
            .device
            .supported_output_configs()
            .map_err(|e| AudioError::Output(format!("failed to query configs: {e}")))?
            .collect();

        let rate_matches: Vec<&SupportedStreamConfigRange> = supported_configs
            .iter()
            .filter(|range| {
                range.sample_format() == SampleFormat::F32
                    && range.min_sample_rate() <= desired
                    && range.max_sample_rate() >= desired
            })
            .collect();

        let best = rate_matches
            .iter()
            .copied()
            .filter(|r| r.channels() >= desired_channels)
            .min_by_key(|r| r.channels())
            .or_else(|| rate_matches.iter().copied().max_by_key(|r| r.channels()));

        if let Some(range) = best {
            let config: StreamConfig = range.with_sample_rate(desired).into();
            self.build_stream(config, buffer_secs)
        } else {
            Err(AudioError::Output(format!(
                "device does not support {desired_rate}Hz output"
            )))
        }
    }

    /// Build and start a CPAL output stream with the given config.
    fn build_stream(&self, config: StreamConfig, buffer_secs: f32) -> Result<AudioOutput, AudioError> {
        let buffer_samples =
            (config.sample_rate.0 as f32 * buffer_secs) as usize * config.channels as usize;

        let rb = HeapRb::<f32>::new(buffer_samples);
        let (producer, mut consumer) = rb.split();

        let err_callback = |err: cpal::StreamError| {
            log::error!("audio output stream error: {err}");
        };

        let stream = self
            .device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    // Lock-free read from the ring buffer. If there aren't
                    // enough samples, the remainder is filled with silence.
                    let read = consumer.pop_slice(data);
                    for sample in &mut data[read..] {
                        *sample = 0.0;
                    }
                },
                err_callback,
                None,
            )
            .map_err(|e| AudioError::Output(format!("failed to build output stream: {e}")))?;

        stream
            .play()
            .map_err(|e| AudioError::Output(format!("failed to start output stream: {e}")))?;

        log::info!(
            "audio output opened: {}Hz, {} channels",
            config.sample_rate.0,
            config.channels
        );

        Ok(AudioOutput {
            _stream: stream,
            producer,
            config,
        })
    }
}
