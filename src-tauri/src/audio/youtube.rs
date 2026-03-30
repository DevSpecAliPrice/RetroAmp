//! YouTube audio source — streams audio from YouTube videos.
//!
//! Architecture:
//!   [Download thread]
//!     yt-dlp resolves stream URL → ureq downloads bytes →
//!       ├─ SharedBuf (in-memory, for immediate streaming playback)
//!       └─ temp file on disk (for seeking once download completes)
//!   [Audio thread]
//!     SeekableStreamBuf → Symphonia decode → AudioBuffer
//!
//! Playback starts immediately from the streaming buffer. Once the full file
//! is downloaded to disk, seeking becomes available — the Symphonia pipeline
//! is rebuilt from the temp file (which gives Symphonia the full seek index).

use std::io::{self, Read, Seek, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::TimeBase;

use crate::audio::error::AudioError;
use crate::audio::source::{
    AudioBuffer, AudioSource, SourceCapabilities, SourceState, TrackMetadata,
};

/// Pre-buffer threshold before starting decode (bytes).
const PRE_BUFFER_BYTES: usize = 16384;

/// Maximum time `next_buffer()` spends trying to decode before yielding.
const MAX_DECODE_LOOP_SECS: f64 = 2.0;

// ---------------------------------------------------------------------------
// Streaming buffer — for immediate playback
// ---------------------------------------------------------------------------

/// Shared append-only byte buffer. The download thread pushes bytes; the audio
/// thread reads from a cursor position via SeekableStreamBuf.
struct SharedBuf {
    data: Vec<u8>,
    finished: bool,
}

/// A read-only view over SharedBuf for Symphonia. Not truly seekable (reports
/// is_seekable=false) so Symphonia won't try to read the Cues element during
/// probing, keeping startup fast.
struct StreamingBufReader {
    buf: Arc<Mutex<SharedBuf>>,
    pos: u64,
    stop: Arc<AtomicBool>,
}

impl StreamingBufReader {
    fn new(buf: Arc<Mutex<SharedBuf>>, stop: Arc<AtomicBool>) -> Self {
        Self { buf, pos: 0, stop }
    }
}

impl Read for StreamingBufReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return Ok(0);
            }
            let guard = self.buf.lock().unwrap();
            let available = guard.data.len() as u64;
            if self.pos < available {
                let start = self.pos as usize;
                let end = (start + out.len()).min(guard.data.len());
                let n = end - start;
                out[..n].copy_from_slice(&guard.data[start..end]);
                drop(guard);
                self.pos += n as u64;
                return Ok(n);
            }
            if guard.finished {
                return Ok(0);
            }
            drop(guard);
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Seek for StreamingBufReader {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "stream not seekable"))
    }
}

impl symphonia::core::io::MediaSource for StreamingBufReader {
    fn is_seekable(&self) -> bool {
        false
    }
    fn byte_len(&self) -> Option<u64> {
        None
    }
}

// ---------------------------------------------------------------------------
// YouTubeSource
// ---------------------------------------------------------------------------

/// Metadata sent from the download thread.
struct StreamMeta {
    duration_secs: Option<f64>,
    content_type: String,
}

pub struct YouTubeSource {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    metadata: TrackMetadata,
    state: SourceState,
    sample_buf: Option<SampleBuffer<f32>>,
    time_base: Option<TimeBase>,
    current_ts: u64,
    /// Hint extension for Symphonia probing (e.g. "webm", "m4a").
    ext_hint: String,
    /// Temp file path — written by the download thread.
    temp_path: PathBuf,
    /// Set to true by the download thread when temp file is complete.
    temp_ready: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    _download_thread: Option<thread::JoinHandle<()>>,
}

impl YouTubeSource {
    pub fn new(video_id: &str, metadata: TrackMetadata) -> Result<Self, AudioError> {
        let url = format!("https://www.youtube.com/watch?v={video_id}");

        let shared_buf = Arc::new(Mutex::new(SharedBuf {
            data: Vec::with_capacity(256 * 1024),
            finished: false,
        }));

        let stop = Arc::new(AtomicBool::new(false));
        let temp_ready = Arc::new(AtomicBool::new(false));

        let (meta_tx, meta_rx) = mpsc::channel::<Result<StreamMeta, String>>();

        // Temp file path for background download.
        let temp_path = std::env::temp_dir().join(format!(
            "retroamp_yt_{}_{}.tmp",
            video_id,
            std::process::id(),
        ));

        {
            let stop = Arc::clone(&stop);
            let buf = Arc::clone(&shared_buf);
            let temp_ready = Arc::clone(&temp_ready);
            let temp_path = temp_path.clone();

            thread::Builder::new()
                .name("youtube-download".into())
                .spawn(move || {
                    download_thread_main(url, buf, stop, meta_tx, temp_path, temp_ready);
                })
                .map_err(|e| AudioError::Network(format!("spawn download thread: {e}")))?;
        }

        // Wait for stream metadata.
        let stream_meta = meta_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| AudioError::ConnectionFailed("timeout waiting for metadata".into()))?
            .map_err(AudioError::Network)?;

        let duration = stream_meta.duration_secs.map(Duration::from_secs_f64);
        let content_type = stream_meta.content_type;

        let ext_hint = match content_type.as_str() {
            "audio/mp4" => "m4a",
            "audio/webm" => "webm",
            "audio/ogg" => "ogg",
            _ => "webm",
        }
        .to_string();

        let mut metadata = metadata;
        if metadata.duration.is_none() {
            metadata.duration = duration;
        }

        // Pre-buffer before probing.
        {
            let deadline = std::time::Instant::now() + Duration::from_secs(15);
            loop {
                let guard = shared_buf.lock().unwrap();
                let len = guard.data.len();
                let finished = guard.finished;
                drop(guard);

                if len >= PRE_BUFFER_BYTES {
                    break;
                }
                if std::time::Instant::now() > deadline {
                    if len > 0 { break; }
                    return Err(AudioError::ConnectionFailed(
                        "timed out waiting for YouTube stream data".into(),
                    ));
                }
                if finished && len == 0 {
                    return Err(AudioError::ConnectionFailed(
                        "download thread exited before any data received".into(),
                    ));
                }
                thread::sleep(Duration::from_millis(50));
            }
            log::info!(
                "[youtube] pre-buffered {} bytes",
                shared_buf.lock().unwrap().data.len()
            );
        }

        // Build streaming Symphonia pipeline (non-seekable — fast startup).
        let buf_reader = StreamingBufReader::new(Arc::clone(&shared_buf), Arc::clone(&stop));
        let mss = MediaSourceStream::new(Box::new(buf_reader), Default::default());

        let mut hint = Hint::new();
        hint.with_extension(&ext_hint);

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| AudioError::Decode(format!(
                "could not identify format (content-type: {content_type}): {e}"
            )))?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(AudioError::NoTrack)?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let time_base = codec_params.time_base;
        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| AudioError::Decode("no sample rate".into()))?;
        let channels = codec_params
            .channels
            .map(|ch| ch.count() as u16)
            .unwrap_or(2);

        if metadata.duration.is_none() {
            if let (Some(n_frames), Some(tb)) = (codec_params.n_frames, time_base) {
                let time = tb.calc_time(n_frames);
                metadata.duration = Some(
                    Duration::from_secs(time.seconds)
                        + Duration::from_nanos((time.frac * 1_000_000_000.0) as u64),
                );
            }
        }

        let decoder = crate::audio::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| AudioError::UnsupportedFormat(format!(
                "unsupported codec (content-type: {content_type}): {e}"
            )))?;

        log::info!(
            "[youtube] connected — {}Hz {}ch, content-type: {}",
            sample_rate, channels, content_type,
        );

        metadata.sample_rate = sample_rate;
        metadata.channels = channels;

        Ok(Self {
            format,
            decoder,
            track_id,
            metadata,
            state: SourceState::Ready,
            sample_buf: None,
            time_base,
            current_ts: 0,
            ext_hint,
            temp_path,
            temp_ready,
            stop,
            _download_thread: None, // detached
        })
    }

    /// Rebuild the Symphonia pipeline from the completed temp file.
    /// This gives us a fully seekable format reader (with Cues loaded).
    fn rebuild_from_file(&mut self) -> Result<(), AudioError> {
        let file = std::fs::File::open(&self.temp_path)
            .map_err(|e| AudioError::Decode(format!("open temp file: {e}")))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        hint.with_extension(&self.ext_hint);

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| AudioError::Decode(format!("re-probe temp file: {e}")))?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(AudioError::NoTrack)?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        self.time_base = codec_params.time_base;

        let decoder = crate::audio::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| AudioError::Decode(format!("re-create decoder: {e}")))?;

        self.format = format;
        self.decoder = decoder;
        self.track_id = track_id;
        self.sample_buf = None;

        log::info!("[youtube] rebuilt pipeline from temp file for seeking");
        Ok(())
    }

    /// Path to the downloaded temp file (available once download completes).
    pub fn temp_file_path(&self) -> Option<&PathBuf> {
        if self.temp_ready.load(Ordering::Relaxed) {
            Some(&self.temp_path)
        } else {
            None
        }
    }
}

impl Drop for YouTubeSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Clean up temp file.
        let _ = std::fs::remove_file(&self.temp_path);
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
            can_seek: self.temp_ready.load(Ordering::Relaxed),
            has_duration: self.metadata.duration.is_some(),
            has_dynamic_metadata: false,
            is_network_source: true,
        }
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBuffer>, AudioError> {
        if self.state == SourceState::Error || self.state == SourceState::Finished {
            return Ok(None);
        }
        self.state = SourceState::Playing;

        let deadline = std::time::Instant::now() + Duration::from_secs_f64(MAX_DECODE_LOOP_SECS);

        loop {
            if std::time::Instant::now() > deadline {
                return Ok(None);
            }
            match self.format.next_packet() {
                Ok(packet) => {
                    if packet.track_id() != self.track_id {
                        continue;
                    }
                    self.current_ts = packet.ts();
                    match self.decoder.decode(&packet) {
                        Ok(decoded) => {
                            let spec = *decoded.spec();
                            let frames = decoded.frames();
                            if frames == 0 {
                                continue;
                            }
                            let capacity = decoded.capacity();
                            let sbuf = self.sample_buf.get_or_insert_with(|| {
                                SampleBuffer::<f32>::new(capacity as u64, spec)
                            });
                            if sbuf.capacity() < capacity {
                                self.sample_buf =
                                    Some(SampleBuffer::<f32>::new(capacity as u64, spec));
                            }
                            let sbuf = self.sample_buf.as_mut().unwrap();
                            sbuf.copy_interleaved_ref(decoded);

                            return Ok(Some(AudioBuffer {
                                samples: sbuf.samples().to_vec(),
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
                    self.state = SourceState::Finished;
                    return Ok(None);
                }
                Err(e) => {
                    log::error!("[youtube] stream error: {e}");
                    self.state = SourceState::Error;
                    return Ok(None);
                }
            }
        }
    }

    fn seek(&mut self, position: Duration) -> Result<(), AudioError> {
        if !self.temp_ready.load(Ordering::Relaxed) {
            return Err(AudioError::SeekNotSupported);
        }

        // Rebuild the pipeline from the temp file (fully seekable).
        // We must rebuild because the current streaming pipeline has no Cues.
        self.rebuild_from_file()?;

        let seconds = position.as_secs();
        let frac = position.subsec_nanos() as f64 / 1_000_000_000.0;

        self.format
            .seek(
                SeekMode::Coarse,
                SeekTo::Time {
                    time: symphonia::core::units::Time::new(seconds, frac),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| AudioError::Decode(format!("seek error: {e}")))?;

        self.decoder.reset();
        self.state = SourceState::Playing;

        Ok(())
    }

    fn position(&self) -> Option<Duration> {
        self.time_base.map(|tb| {
            let time = tb.calc_time(self.current_ts);
            Duration::from_secs(time.seconds)
                + Duration::from_nanos((time.frac * 1_000_000_000.0) as u64)
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Use yt-dlp to resolve stream URL and metadata for a YouTube video.
fn resolve_with_ytdlp(url: &str) -> Result<(String, Option<f64>, String), String> {
    let cfg = crate::config::AppConfig::load();

    let ytdlp = match &cfg.youtube.ytdlp_path {
        Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
        _ => crate::youtube::ytdlp::ensure_available()?,
    };

    let format_selector = match cfg.youtube.quality.as_str() {
        "low" => "worstaudio",
        _ => "bestaudio",
    };

    let output = Command::new(&ytdlp)
        .args([
            "-f", format_selector,
            "-j",
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
        duration_secs, content_type, stream_url.len(),
    );

    Ok((stream_url, duration_secs, content_type))
}

/// Download thread: resolves the stream URL, then downloads bytes into both
/// the in-memory SharedBuf (for streaming playback) and a temp file on disk
/// (for seeking once download completes).
fn download_thread_main(
    url: String,
    buf: Arc<Mutex<SharedBuf>>,
    stop: Arc<AtomicBool>,
    meta_tx: mpsc::Sender<Result<StreamMeta, String>>,
    temp_path: PathBuf,
    temp_ready: Arc<AtomicBool>,
) {
    let (stream_url, duration_secs, content_type) = match resolve_with_ytdlp(&url) {
        Ok(result) => result,
        Err(e) => {
            let _ = meta_tx.send(Err(e));
            buf.lock().unwrap().finished = true;
            return;
        }
    };

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
            log::error!("[youtube] failed to connect: {e}");
            let _ = meta_tx.send(Err(format!("failed to connect: {e}")));
            buf.lock().unwrap().finished = true;
            return;
        }
    };

    if meta_tx
        .send(Ok(StreamMeta { duration_secs, content_type }))
        .is_err()
    {
        buf.lock().unwrap().finished = true;
        return;
    }

    // Open temp file for background download.
    let mut temp_file = match std::fs::File::create(&temp_path) {
        Ok(f) => Some(f),
        Err(e) => {
            log::warn!("[youtube] could not create temp file (seeking disabled): {e}");
            None
        }
    };

    let mut reader = response.into_body().into_reader();
    let mut tmp = [0u8; 8192];

    loop {
        if stop.load(Ordering::Relaxed) {
            buf.lock().unwrap().finished = true;
            return;
        }

        match reader.read(&mut tmp) {
            Ok(0) => {
                buf.lock().unwrap().finished = true;

                // Flush and close temp file, then signal ready.
                if let Some(mut f) = temp_file.take() {
                    if f.flush().is_ok() {
                        drop(f);
                        let size = std::fs::metadata(&temp_path)
                            .map(|m| m.len())
                            .unwrap_or(0);
                        log::info!(
                            "[youtube] download complete: {} bytes ({:.1} MB) — seeking enabled",
                            size, size as f64 / 1_048_576.0,
                        );
                        temp_ready.store(true, Ordering::Release);
                    }
                }
                return;
            }
            Ok(n) => {
                buf.lock().unwrap().data.extend_from_slice(&tmp[..n]);

                // Also write to temp file (best-effort — don't fail playback).
                if let Some(ref mut f) = temp_file {
                    if f.write_all(&tmp[..n]).is_err() {
                        log::warn!("[youtube] temp file write failed, disabling seek");
                        temp_file = None;
                        let _ = std::fs::remove_file(&temp_path);
                    }
                }
            }
            Err(e) => {
                log::error!("[youtube] stream read error: {e}");
                buf.lock().unwrap().finished = true;
                return;
            }
        }
    }
}
