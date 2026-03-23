//! CPAL audio output — sends processed PCM samples to the system audio device.
//!
//! The output can be reconfigured to match the source's sample rate. When a
//! new track loads at a different rate, the engine drops the old output and
//! opens a new one at the requested rate. If the device doesn't support the
//! rate, the engine falls back to resampling.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, SampleRate, Stream, StreamConfig, SupportedStreamConfigRange};
use std::sync::{Arc, Mutex};

use crate::audio::error::AudioError;

/// A ring buffer that the audio engine writes to and CPAL reads from.
/// This decouples the engine's processing rate from CPAL's callback rate.
pub struct OutputBuffer {
    buffer: Vec<f32>,
    read_pos: usize,
    write_pos: usize,
    capacity: usize,
}

impl OutputBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0.0; capacity],
            read_pos: 0,
            write_pos: 0,
            capacity,
        }
    }

    pub fn available(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.capacity - self.read_pos + self.write_pos
        }
    }

    pub fn free_space(&self) -> usize {
        self.capacity - 1 - self.available()
    }

    pub fn write(&mut self, samples: &[f32]) -> usize {
        let to_write = samples.len().min(self.free_space());
        for &sample in &samples[..to_write] {
            self.buffer[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % self.capacity;
        }
        to_write
    }

    pub fn read(&mut self, output: &mut [f32]) -> usize {
        let to_read = output.len().min(self.available());
        for sample in &mut output[..to_read] {
            *sample = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % self.capacity;
        }
        for sample in &mut output[to_read..] {
            *sample = 0.0;
        }
        to_read
    }
}

/// Describes the output device and its configuration.
#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub sample_rate: u32,
    pub channels: u16,
}

/// Handle to the audio output stream.
pub struct AudioOutput {
    _stream: Stream,
    buffer: Arc<Mutex<OutputBuffer>>,
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
    pub fn write(&self, samples: &[f32]) -> usize {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.write(samples)
        } else {
            0
        }
    }

    /// How many samples can be written without blocking.
    pub fn free_space(&self) -> usize {
        if let Ok(buf) = self.buffer.lock() {
            buf.free_space()
        } else {
            0
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

        self.build_stream(supported.into())
    }

    /// Open the output at a specific sample rate. If the device doesn't
    /// support it, returns Err so the caller can fall back to resampling.
    pub fn open_at_rate(&self, desired_rate: u32) -> Result<AudioOutput, AudioError> {
        let desired = SampleRate(desired_rate);

        // Find an F32 config range that includes the desired rate.
        let supported_configs: Vec<SupportedStreamConfigRange> = self
            .device
            .supported_output_configs()
            .map_err(|e| AudioError::Output(format!("failed to query configs: {e}")))?
            .collect();

        let matching_range = supported_configs.iter().find(|range| {
            range.sample_format() == SampleFormat::F32
                && range.min_sample_rate() <= desired
                && range.max_sample_rate() >= desired
        });

        if let Some(range) = matching_range {
            let config: StreamConfig = range.with_sample_rate(desired).into();
            self.build_stream(config)
        } else {
            Err(AudioError::Output(format!(
                "device does not support {desired_rate}Hz output"
            )))
        }
    }

    /// Build and start a CPAL output stream with the given config.
    fn build_stream(&self, config: StreamConfig) -> Result<AudioOutput, AudioError> {
        // Size the ring buffer for ~200ms of audio.
        let buffer_size = (config.sample_rate.0 as usize) * (config.channels as usize) / 5;
        let buffer = Arc::new(Mutex::new(OutputBuffer::new(buffer_size)));
        let buffer_for_callback = Arc::clone(&buffer);

        let err_callback = |err: cpal::StreamError| {
            log::error!("audio output stream error: {err}");
        };

        let stream = self
            .device
            .build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    if let Ok(mut buf) = buffer_for_callback.lock() {
                        buf.read(data);
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
            buffer,
            config,
        })
    }
}
