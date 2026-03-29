//! YouTube audio source — streams audio from YouTube videos.
//!
//! Implements the `AudioSource` trait for YouTube tracks. Audio data flows from
//! yt-dlp (URL resolution) → ureq (HTTP download) → ring buffer → Symphonia
//! (decode) → AudioBuffer, following the same pattern as RadioSource.
//!
//! Architecture:
//!   [Background thread]
//!     yt-dlp resolves stream URL → ureq downloads bytes → HeapRb<u8>
//!   [Audio thread]
//!     HeapRb<u8> consumer → StreamBufReader → Symphonia decode → AudioBuffer

use std::io::Read;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ringbuf::{
    traits::{Observer, Producer, Split},
    HeapRb,
};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::error::AudioError;
use crate::audio::source::{
    AudioBuffer, AudioSource, SourceCapabilities, SourceState, TrackMetadata,
};
use crate::audio::stream_reader::StreamBufReader;

/// Ring buffer size: 256 KB ≈ 16 seconds at 128 kbps.
const BUFFER_SIZE: usize = 256 * 1024;

/// Pre-buffer threshold before starting decode (bytes).
const PRE_BUFFER_BYTES: usize = 16384;

/// Maximum time `next_buffer()` spends trying to decode before yielding.
const MAX_DECODE_LOOP_SECS: f64 = 2.0;

/// After this many consecutive empty decode attempts, give up.
const MAX_CONSECUTIVE_EMPTY: u32 = 5;

struct DecodePipeline {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
}

/// Metadata sent from the download thread back to the constructor.
struct StreamMeta {
    duration_secs: Option<f64>,
    content_type: String,
}

pub struct YouTubeSource {
    pipeline: DecodePipeline,
    sample_rate: u32,
    channels: u16,
    metadata: TrackMetadata,
    duration: Option<Duration>,
    state: SourceState,
    position_samples: u64,
    reader_alive: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    sample_buf: Option<SampleBuffer<f32>>,
    consecutive_empty: u32,
    _download_thread: Option<thread::JoinHandle<()>>,
}

impl YouTubeSource {
    /// Create a new YouTubeSource for a given video ID.
    ///
    /// Spawns a background thread that uses yt-dlp to resolve the stream URL,
    /// then downloads audio bytes via ureq into a ring buffer.
    pub fn new(video_id: &str, metadata: TrackMetadata) -> Result<Self, AudioError> {
        let url = format!("https://www.youtube.com/watch?v={video_id}");

        let rb = HeapRb::<u8>::new(BUFFER_SIZE);
        let (producer, consumer) = rb.split();

        let stop = Arc::new(AtomicBool::new(false));
        let reader_alive = Arc::new(AtomicBool::new(true));

        // Channel for the download thread to send back stream metadata.
        let (meta_tx, meta_rx) = mpsc::channel::<Result<StreamMeta, String>>();

        let download_thread = {
            let stop = Arc::clone(&stop);
            let reader_alive = Arc::clone(&reader_alive);

            thread::Builder::new()
                .name("youtube-download".into())
                .spawn(move || {
                    download_thread_main(url, producer, stop, meta_tx);
                    reader_alive.store(false, Ordering::Relaxed);
                })
                .map_err(|e| AudioError::Network(format!("failed to spawn download thread: {e}")))?
        };

        // Wait for the download thread to resolve stream metadata.
        let stream_meta = meta_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| {
                AudioError::ConnectionFailed(
                    "timeout waiting for YouTube stream metadata".into(),
                )
            })?
            .map_err(|e| AudioError::Network(e))?;

        let duration = stream_meta.duration_secs.map(Duration::from_secs_f64);
        let content_type = stream_meta.content_type;

        let mut metadata = metadata;
        if metadata.duration.is_none() {
            metadata.duration = duration;
        }

        // Pre-buffer before probing.
        {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            loop {
                if consumer.occupied_len() >= PRE_BUFFER_BYTES {
                    break;
                }
                if std::time::Instant::now() > deadline {
                    if consumer.occupied_len() > 0 {
                        break;
                    }
                    return Err(AudioError::ConnectionFailed(
                        "timed out waiting for YouTube stream data".into(),
                    ));
                }
                if !reader_alive.load(Ordering::Relaxed) && consumer.occupied_len() == 0 {
                    return Err(AudioError::ConnectionFailed(
                        "download thread exited before any data received".into(),
                    ));
                }
                thread::sleep(Duration::from_millis(50));
            }
            log::info!("[youtube] pre-buffered {} bytes", consumer.occupied_len());
        }

        // Build Symphonia pipeline.
        let buf_reader =
            StreamBufReader::new(consumer, Arc::clone(&stop), Arc::clone(&reader_alive));
        let mss = MediaSourceStream::new(Box::new(buf_reader), Default::default());

        let mut hint = Hint::new();
        let ct = content_type.to_lowercase();
        if ct.contains("mp4") || ct.contains("m4a") {
            hint.with_extension("m4a");
        } else if ct.contains("webm") {
            hint.with_extension("webm");
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| {
                AudioError::Decode(format!(
                    "could not identify YouTube stream format (content-type: {content_type}): {e}"
                ))
            })?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(AudioError::NoTrack)?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| AudioError::Decode("no sample rate in YouTube stream".into()))?;
        let channels = codec_params
            .channels
            .map(|ch| ch.count() as u16)
            .unwrap_or(2);

        let decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| {
                AudioError::UnsupportedFormat(format!(
                    "unsupported codec in YouTube stream (content-type: {content_type}): {e}"
                ))
            })?;

        log::info!(
            "[youtube] connected — {}Hz {}ch, content-type: {}",
            sample_rate,
            channels,
            content_type,
        );

        metadata.sample_rate = sample_rate;
        metadata.channels = channels;

        Ok(Self {
            pipeline: DecodePipeline {
                format,
                decoder,
                track_id,
            },
            sample_rate,
            channels,
            metadata,
            duration,
            state: SourceState::Ready,
            position_samples: 0,
            reader_alive,
            stop,
            sample_buf: None,
            consecutive_empty: 0,
            _download_thread: Some(download_thread),
        })
    }
}

impl Drop for YouTubeSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self._download_thread.take() {
            let _ = thread.join();
        }
    }
}

// ---------------------------------------------------------------------------
// AudioSource implementation
// ---------------------------------------------------------------------------

impl AudioSource for YouTubeSource {
    fn metadata(&self) -> Result<TrackMetadata, AudioError> {
        Ok(self.metadata.clone())
    }

    fn state(&self) -> SourceState {
        self.state
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            can_seek: false,
            has_duration: self.duration.is_some(),
            has_dynamic_metadata: false,
            is_network_source: true,
        }
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBuffer>, AudioError> {
        if self.state == SourceState::Error || self.state == SourceState::Finished {
            return Ok(None);
        }
        self.state = SourceState::Playing;

        let deadline =
            std::time::Instant::now() + Duration::from_secs_f64(MAX_DECODE_LOOP_SECS);
        let tid = self.pipeline.track_id;

        loop {
            if std::time::Instant::now() > deadline {
                return self.yield_or_give_up();
            }
            match self.pipeline.format.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != tid {
                        continue;
                    }
                    match self.pipeline.decoder.decode(&packet) {
                        Ok(decoded) => {
                            let spec = *decoded.spec();
                            let num_frames = decoded.capacity();
                            if num_frames == 0 {
                                continue;
                            }

                            let sbuf = self.sample_buf.get_or_insert_with(|| {
                                SampleBuffer::<f32>::new(num_frames as u64, spec)
                            });
                            if sbuf.capacity() < num_frames {
                                self.sample_buf =
                                    Some(SampleBuffer::<f32>::new(num_frames as u64, spec));
                            }
                            let sbuf = self.sample_buf.as_mut().unwrap();
                            sbuf.copy_interleaved_ref(decoded);
                            let samples = sbuf.samples().to_vec();

                            self.position_samples += num_frames as u64;
                            self.consecutive_empty = 0;

                            return Ok(Some(AudioBuffer {
                                samples,
                                sample_rate: spec.rate,
                                channels: spec.channels.count() as u16,
                            }));
                        }
                        Err(SymphoniaError::DecodeError(e)) => {
                            log::debug!("[youtube] decode error (skipping): {e}");
                            continue;
                        }
                        Err(e) => {
                            log::warn!("[youtube] decode error: {e}");
                            continue;
                        }
                    }
                }
                Err(SymphoniaError::IoError(ref e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    if !self.reader_alive.load(Ordering::Relaxed) {
                        log::info!("[youtube] stream finished");
                        self.state = SourceState::Finished;
                        return Ok(None);
                    }
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => {
                    log::error!("[youtube] stream error: {e}");
                    if !self.reader_alive.load(Ordering::Relaxed) {
                        self.state = SourceState::Finished;
                        return Ok(None);
                    }
                    return Ok(Some(make_silence(self.sample_rate, self.channels)));
                }
            }
        }
    }

    fn seek(&mut self, _position: Duration) -> Result<(), AudioError> {
        Err(AudioError::SeekNotSupported)
    }

    fn position(&self) -> Option<Duration> {
        if self.sample_rate == 0 {
            return None;
        }
        Some(Duration::from_secs_f64(
            self.position_samples as f64 / self.sample_rate as f64,
        ))
    }
}

impl YouTubeSource {
    fn yield_or_give_up(&mut self) -> Result<Option<AudioBuffer>, AudioError> {
        self.consecutive_empty += 1;
        if self.consecutive_empty >= MAX_CONSECUTIVE_EMPTY {
            log::error!(
                "[youtube] no valid audio after {} attempts — giving up",
                self.consecutive_empty,
            );
            self.state = SourceState::Error;
            return Ok(None);
        }
        Ok(Some(make_silence(self.sample_rate, self.channels)))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_silence(sample_rate: u32, channels: u16) -> AudioBuffer {
    AudioBuffer {
        samples: vec![0.0; (sample_rate as usize / 100) * channels as usize],
        sample_rate,
        channels,
    }
}

/// Use yt-dlp to resolve stream URL and metadata for a YouTube video.
///
/// Returns (stream_url, duration_secs, content_type) or an error string.
/// yt-dlp handles all anti-bot measures (PO tokens, signature deobfuscation).
///
/// Uses the yt-dlp binary manager to find or auto-download yt-dlp.
fn resolve_with_ytdlp(url: &str) -> Result<(String, Option<f64>, String), String> {
    let ytdlp = crate::youtube::ytdlp::ensure_available()?;

    // Get both the stream URL and metadata in a single call via JSON dump.
    // This is faster than two separate yt-dlp invocations.
    let output = Command::new(&ytdlp)
        .args([
            "-f", "bestaudio[ext=m4a]/bestaudio",
            "-j", // Print JSON info (includes the URL)
            "--no-warnings",
            "--no-playlist",
            url,
        ])
        .output()
        .map_err(|e| format!("yt-dlp failed to run: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp failed: {}", stderr.trim()));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse yt-dlp JSON: {e}"))?;

    let stream_url = json
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "yt-dlp JSON missing 'url' field".to_string())?;

    if stream_url.is_empty() {
        return Err("yt-dlp returned empty URL".into());
    }

    let duration_secs = json.get("duration").and_then(|v| v.as_f64());

    let content_type = json
        .get("ext")
        .and_then(|v| v.as_str())
        .map(|ext| match ext {
            "m4a" | "mp4" => "audio/mp4".to_string(),
            "webm" => "audio/webm".to_string(),
            "ogg" | "opus" => "audio/ogg".to_string(),
            other => format!("audio/{other}"),
        })
        .unwrap_or_else(|| "audio/mp4".to_string());

    log::info!(
        "[youtube] yt-dlp resolved: duration={:?}s, type={}, url_len={}",
        duration_secs,
        content_type,
        stream_url.len(),
    );

    Ok((stream_url, duration_secs, content_type))
}

/// Background thread: resolves the stream URL via yt-dlp, then downloads
/// audio bytes via ureq into the ring buffer.
fn download_thread_main(
    url: String,
    mut producer: ringbuf::HeapProd<u8>,
    stop: Arc<AtomicBool>,
    meta_tx: mpsc::Sender<Result<StreamMeta, String>>,
) {
    // Resolve stream URL and metadata via yt-dlp.
    let (stream_url, duration_secs, content_type) = match resolve_with_ytdlp(&url) {
        Ok(result) => result,
        Err(e) => {
            let _ = meta_tx.send(Err(e));
            return;
        }
    };

    // Send metadata back to the constructor.
    if meta_tx
        .send(Ok(StreamMeta {
            duration_secs,
            content_type,
        }))
        .is_err()
    {
        return;
    }

    // Download the audio stream via ureq.
    let response = match ureq::get(&stream_url)
        .header("User-Agent", "Mozilla/5.0")
        .config()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_body(None)
        .build()
        .call()
    {
        Ok(resp) => resp,
        Err(e) => {
            log::error!("[youtube] failed to connect to stream: {e}");
            return;
        }
    };

    let mut reader = response.into_body().into_reader();
    let mut buf = [0u8; 8192];

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }

        match reader.read(&mut buf) {
            Ok(0) => {
                log::info!("[youtube] download complete (EOF)");
                return;
            }
            Ok(n) => {
                let mut offset = 0;
                while offset < n {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let writable = producer.vacant_len();
                    if writable == 0 {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    let to_write = (n - offset).min(writable);
                    producer.push_slice(&buf[offset..offset + to_write]);
                    offset += to_write;
                }
            }
            Err(e) => {
                log::error!("[youtube] stream read error: {e}");
                return;
            }
        }
    }
}
