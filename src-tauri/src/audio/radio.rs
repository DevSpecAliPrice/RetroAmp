//! Radio audio source — decodes internet radio streams via HTTP.
//!
//! Implements the `AudioSource` trait for internet radio. Audio data flows
//! from an HTTP stream through a ring buffer into Symphonia for decoding.
//! ICY metadata updates are reflected in `metadata()` calls.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::error::AudioError;
use crate::audio::icy::{self, IcyMetadata};
use crate::audio::source::{AudioBuffer, AudioSource, SourceCapabilities, SourceState, TrackMetadata};
use crate::audio::stream_reader::{StreamBufReader, StreamReader};

pub struct RadioSource {
    /// Symphonia format reader.
    format: Box<dyn FormatReader>,
    /// Symphonia decoder.
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    /// Audio track ID within the stream.
    track_id: u32,

    // Audio format info.
    sample_rate: u32,
    channels: u16,

    // Station metadata from HTTP headers.
    station_name: Option<String>,
    genre: Option<String>,
    bitrate: Option<u32>,
    #[allow(dead_code)]
    url: String,

    /// Shared ICY metadata — updated by reader thread, read here.
    icy_metadata: Arc<Mutex<IcyMetadata>>,

    /// Current source state.
    state: SourceState,

    /// Accumulated playback time (there's no "position" in a live stream).
    elapsed: Duration,

    /// Whether the reader thread is connected.
    connected: Arc<AtomicBool>,

    /// Stream reader — holds the reader thread handle. Dropped when RadioSource
    /// is dropped, which stops the thread.
    _reader: StreamReader,

    /// Reusable sample buffer — avoids allocation per decode call.
    sample_buf: Option<SampleBuffer<f32>>,
}

impl RadioSource {
    /// Connect to a radio stream and prepare for decoding.
    ///
    /// This is called on the main thread. It:
    /// 1. Makes the HTTP connection and starts the reader thread.
    /// 2. Waits for enough data to buffer.
    /// 3. Probes the stream with Symphonia to detect the format.
    /// 4. Creates a decoder.
    pub fn connect(url: &str) -> Result<Self, AudioError> {
        Self::connect_with_name(url, None)
    }

    /// Connect with an explicit station name (used when the UI knows the name
    /// before the stream provides ICY headers).
    pub fn connect_with_name(url: &str, display_name: Option<&str>) -> Result<Self, AudioError> {
        let mut reader = StreamReader::connect(url)?;

        let consumer = reader
            .take_consumer()
            .ok_or_else(|| AudioError::Network("consumer already taken".into()))?;

        let mut stream_info = reader.stream_info.clone();
        // Override station name if caller provided one.
        if let Some(name) = display_name {
            stream_info.station_name = Some(name.to_string());
        }
        let icy_metadata = Arc::clone(&reader.icy_metadata);
        let connected = Arc::clone(&reader.connected);

        // Wait for the ring buffer to accumulate some data before probing.
        // Symphonia's probe needs real audio bytes; if it gets 0 it sees EOF.
        {
            use ringbuf::traits::Observer;
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                if consumer.occupied_len() >= 16384 {
                    break; // 16KB buffered — enough for probing.
                }
                if std::time::Instant::now() > deadline {
                    if consumer.occupied_len() > 0 {
                        break; // Some data arrived — try with what we have.
                    }
                    return Err(AudioError::ConnectionFailed(
                        "timed out waiting for stream data".into(),
                    ));
                }
                if !connected.load(std::sync::atomic::Ordering::Relaxed) && consumer.occupied_len() == 0 {
                    return Err(AudioError::ConnectionFailed(
                        "stream disconnected before any data received".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            log::info!("[radio] pre-buffered {} bytes", consumer.occupied_len());
        }

        // Wrap the consumer as a MediaSource → MediaSourceStream.
        let buf_reader = StreamBufReader::new(consumer);
        let mss = MediaSourceStream::new(Box::new(buf_reader), Default::default());

        // Provide a format hint based on the Content-Type header.
        let mut hint = Hint::new();
        match stream_info.content_type.as_str() {
            s if s.contains("mpeg") => {
                hint.with_extension("mp3");
            }
            s if s.contains("aac") || s.contains("aacp") => {
                hint.with_extension("aac");
            }
            s if s.contains("ogg") => {
                hint.with_extension("ogg");
            }
            s if s.contains("flac") => {
                hint.with_extension("flac");
            }
            _ => {
                // Default to MP3 — most common for internet radio.
                hint.with_extension("mp3");
            }
        };

        // Probe the stream to detect format and codec.
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| AudioError::Decode(format!("failed to probe stream: {e}")))?;

        let format = probed.format;

        // Find the audio track.
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(AudioError::NoTrack)?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();

        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| AudioError::Decode("no sample rate in stream codec params".into()))?;

        let channels = codec_params
            .channels
            .map(|ch| ch.count() as u16)
            .unwrap_or(2);

        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| AudioError::UnsupportedFormat(format!("no decoder for stream: {e}")))?;

        log::info!(
            "[radio] connected to {} — {}Hz {}ch, content-type: {}",
            stream_info.station_name.as_deref().unwrap_or(url),
            sample_rate,
            channels,
            stream_info.content_type,
        );

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
            station_name: stream_info.station_name,
            genre: stream_info.genre,
            bitrate: stream_info.bitrate,
            url: url.to_string(),
            icy_metadata,
            state: SourceState::Ready,
            elapsed: Duration::ZERO,
            connected,
            _reader: reader,
            sample_buf: None,
        })
    }
}

impl AudioSource for RadioSource {
    fn metadata(&self) -> Result<TrackMetadata, AudioError> {
        // Read current ICY metadata (lock held very briefly).
        let icy = self
            .icy_metadata
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let (title, artist) = match &icy.stream_title {
            Some(raw) => icy::split_stream_title(raw),
            None => (self.station_name.clone(), None),
        };

        Ok(TrackMetadata {
            title: title.or(self.station_name.clone()),
            artist,
            album: self.station_name.clone(),
            duration: None,
            sample_rate: self.sample_rate,
            channels: self.channels,
            bitrate: self.bitrate,
            genre: self.genre.clone(),
            year: None,
            track_number: None,
            cover_art: None,
        })
    }

    fn state(&self) -> SourceState {
        self.state
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            can_seek: false,
            has_duration: false,
            has_dynamic_metadata: true,
        }
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBuffer>, AudioError> {
        if self.state == SourceState::Error {
            return Ok(None);
        }

        self.state = SourceState::Playing;

        loop {
            match self.format.next_packet() {
                Ok(packet) => {
                    // Skip packets that don't belong to our audio track.
                    if packet.track_id() != self.track_id {
                        continue;
                    }

                    // Decode the packet.
                    let decoded = match self.decoder.decode(&packet) {
                        Ok(decoded) => decoded,
                        Err(SymphoniaError::DecodeError(e)) => {
                            // Recoverable — skip this packet.
                            log::debug!("[radio] decode error (skipping): {e}");
                            continue;
                        }
                        Err(e) => {
                            // For streams, most decode errors are transient.
                            log::warn!("[radio] decode error: {e}");
                            continue;
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
                    if sample_buf.capacity() < num_frames {
                        self.sample_buf =
                            Some(SampleBuffer::<f32>::new(num_frames as u64, spec));
                    }
                    let sample_buf = self.sample_buf.as_mut().unwrap();

                    // Convert decoded audio to interleaved f32.
                    sample_buf.copy_interleaved_ref(decoded);
                    let samples = sample_buf.samples().to_vec();

                    // Track elapsed playback time.
                    let frame_duration =
                        Duration::from_secs_f64(num_frames as f64 / self.sample_rate as f64);
                    self.elapsed += frame_duration;

                    return Ok(Some(AudioBuffer {
                        samples,
                        sample_rate: self.sample_rate,
                        channels: self.channels,
                    }));
                }
                Err(SymphoniaError::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    // Buffer underrun or stream temporarily ended.
                    // Do NOT return Ok(None) — that would trigger TrackFinished.
                    if !self.connected.load(Ordering::Relaxed) {
                        // Reader is disconnected and reconnecting — return silence.
                        let silence_len =
                            (self.sample_rate as usize / 100) * self.channels as usize;
                        return Ok(Some(AudioBuffer {
                            samples: vec![0.0; silence_len],
                            sample_rate: self.sample_rate,
                            channels: self.channels,
                        }));
                    }
                    // Buffer just temporarily empty — brief wait then retry.
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => {
                    log::error!("[radio] stream error: {e}");
                    // For streams, return silence rather than dying on transient errors.
                    let silence_len =
                        (self.sample_rate as usize / 100) * self.channels as usize;
                    return Ok(Some(AudioBuffer {
                        samples: vec![0.0; silence_len],
                        sample_rate: self.sample_rate,
                        channels: self.channels,
                    }));
                }
            }
        }
    }

    fn seek(&mut self, _position: Duration) -> Result<(), AudioError> {
        Err(AudioError::SeekNotSupported)
    }

    fn position(&self) -> Option<Duration> {
        Some(self.elapsed)
    }
}
