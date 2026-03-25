//! HTTP stream reader — manages the background thread that fetches audio data
//! from an internet radio stream and feeds it into a ring buffer.
//!
//! The ring buffer decouples network I/O (bursty, high-latency) from the audio
//! thread (real-time, latency-sensitive). The audio thread reads from the
//! consumer side via `StreamBufReader`, which implements `Read`.

use std::io::{self, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use ringbuf::{
    traits::{Consumer, Observer, Producer, Split},
    HeapRb,
};

use crate::audio::error::AudioError;
use crate::audio::icy::{IcyMetadata, IcyReader};

/// Default ring buffer size: 256 KB ≈ 16 seconds at 128 kbps.
const DEFAULT_BUFFER_SIZE: usize = 256 * 1024;

/// Maximum number of reconnection attempts before giving up.
const MAX_RECONNECT_ATTEMPTS: usize = 20;

/// Information extracted from the HTTP response headers.
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub content_type: String,
    pub station_name: Option<String>,
    pub genre: Option<String>,
    pub bitrate: Option<u32>,
    pub url: String,
    pub metaint: Option<usize>,
}

/// Manages the HTTP reader thread and provides the consumer end of the buffer.
pub struct StreamReader {
    /// Consumer half of the ring buffer — taken by `take_consumer()`.
    consumer: Option<ringbuf::HeapCons<u8>>,
    /// Shared ICY metadata state.
    pub icy_metadata: Arc<Mutex<IcyMetadata>>,
    /// Stream info from HTTP headers.
    pub stream_info: StreamInfo,
    /// Signals the reader thread to stop.
    stop: Arc<AtomicBool>,
    /// Whether the reader thread is currently connected.
    pub connected: Arc<AtomicBool>,
    /// Reader thread handle.
    thread: Option<thread::JoinHandle<()>>,
}

impl StreamReader {
    /// Connect to a radio stream URL and start the reader thread.
    ///
    /// This is called on the main thread (in a Tauri command handler).
    /// It makes the initial HTTP connection, extracts headers, and spawns
    /// the background reader thread.
    pub fn connect(url: &str) -> Result<Self, AudioError> {
        let response = ureq::get(url)
            .header("Icy-MetaData", "1")
            .header("User-Agent", "RetroAmp/0.1")
            .config()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(15)))
            .timeout_recv_body(None)
            .build()
            .call()
            .map_err(|e| AudioError::ConnectionFailed(format!("{e}")))?;

        // Extract ICY and content headers.
        let headers = response.headers();

        let content_type = header_str(headers, "content-type")
            .unwrap_or_else(|| "audio/mpeg".to_string());

        let metaint: Option<usize> = header_str(headers, "icy-metaint")
            .and_then(|v| v.parse().ok());

        let station_name = header_str(headers, "icy-name");

        let genre = header_str(headers, "icy-genre");

        let bitrate: Option<u32> = header_str(headers, "icy-br")
            .and_then(|v| v.parse().ok());

        let stream_info = StreamInfo {
            content_type,
            station_name,
            genre,
            bitrate,
            url: url.to_string(),
            metaint,
        };

        // Create the ring buffer.
        let rb = HeapRb::<u8>::new(DEFAULT_BUFFER_SIZE);
        let (producer, consumer) = rb.split();

        // Shared state.
        let icy_metadata = Arc::new(Mutex::new(IcyMetadata::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let connected = Arc::new(AtomicBool::new(true));

        // Wrap the response body, optionally with ICY metadata stripping.
        let reader: Box<dyn Read + Send> = if let Some(mi) = metaint {
            Box::new(IcyReader::new(
                response.into_body().into_reader(),
                mi,
                Arc::clone(&icy_metadata),
            ))
        } else {
            Box::new(response.into_body().into_reader())
        };

        // Spawn the reader thread.
        let thread = {
            let stop = Arc::clone(&stop);
            let connected = Arc::clone(&connected);
            let url = url.to_string();
            let icy_metadata = Arc::clone(&icy_metadata);
            let metaint = stream_info.metaint;

            thread::Builder::new()
                .name("radio-reader".into())
                .spawn(move || {
                    reader_thread_main(reader, producer, stop, connected, url, icy_metadata, metaint);
                })
                .map_err(|e| AudioError::Network(format!("failed to spawn reader thread: {e}")))?
        };

        Ok(Self {
            consumer: Some(consumer),
            icy_metadata,
            stream_info,
            stop,
            connected,
            thread: Some(thread),
        })
    }

    /// Take the ring buffer consumer. Called once when constructing RadioSource.
    pub fn take_consumer(&mut self) -> Option<ringbuf::HeapCons<u8>> {
        self.consumer.take()
    }

    /// Stop the reader thread.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for StreamReader {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The reader thread's main loop.
///
/// Reads from the HTTP stream and pushes bytes into the ring buffer.
/// On disconnect, attempts reconnection with exponential backoff.
fn reader_thread_main(
    mut reader: Box<dyn Read + Send>,
    mut producer: ringbuf::HeapProd<u8>,
    stop: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    url: String,
    icy_metadata: Arc<Mutex<IcyMetadata>>,
    metaint: Option<usize>,
) {
    let mut buf = [0u8; 8192];
    let mut reconnect_attempts = 0u32;

    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }

        match reader.read(&mut buf) {
            Ok(0) => {
                // Connection closed — attempt reconnect.
                connected.store(false, Ordering::Relaxed);
                match attempt_reconnect(
                    &url,
                    &icy_metadata,
                    metaint,
                    &mut reconnect_attempts,
                    &stop,
                ) {
                    Some(new_reader) => {
                        reader = new_reader;
                        connected.store(true, Ordering::Relaxed);
                    }
                    None => return, // Gave up or stopped.
                }
            }
            Ok(n) => {
                reconnect_attempts = 0;

                // Push bytes to ring buffer with back-pressure.
                let mut offset = 0;
                while offset < n {
                    let writable = producer.vacant_len();
                    if writable == 0 {
                        // Buffer full — wait briefly for consumer to drain.
                        thread::sleep(Duration::from_millis(5));
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        continue;
                    }
                    let to_write = (n - offset).min(writable);
                    producer.push_slice(&buf[offset..offset + to_write]);
                    offset += to_write;
                }
            }
            Err(e) => {
                log::warn!("[radio] stream read error: {e}");
                connected.store(false, Ordering::Relaxed);
                match attempt_reconnect(
                    &url,
                    &icy_metadata,
                    metaint,
                    &mut reconnect_attempts,
                    &stop,
                ) {
                    Some(new_reader) => {
                        reader = new_reader;
                        connected.store(true, Ordering::Relaxed);
                    }
                    None => return,
                }
            }
        }
    }
}

/// Attempt to reconnect to the stream with exponential backoff.
///
/// Returns the new reader on success, or `None` if we should give up.
fn attempt_reconnect(
    url: &str,
    icy_metadata: &Arc<Mutex<IcyMetadata>>,
    metaint: Option<usize>,
    attempts: &mut u32,
    stop: &Arc<AtomicBool>,
) -> Option<Box<dyn Read + Send>> {
    for _ in 0..MAX_RECONNECT_ATTEMPTS {
        if stop.load(Ordering::Relaxed) {
            return None;
        }

        *attempts += 1;
        let delay = Duration::from_secs((*attempts as u64).min(30));
        log::info!(
            "[radio] reconnecting in {}s (attempt {})...",
            delay.as_secs(),
            attempts
        );
        thread::sleep(delay);

        if stop.load(Ordering::Relaxed) {
            return None;
        }

        match make_stream_request(url, icy_metadata, metaint) {
            Ok(reader) => {
                log::info!("[radio] reconnected successfully");
                *attempts = 0;
                return Some(reader);
            }
            Err(e) => {
                log::warn!("[radio] reconnect failed: {e}");
            }
        }
    }

    log::error!("[radio] gave up reconnecting after {MAX_RECONNECT_ATTEMPTS} attempts");
    None
}

/// Make a fresh HTTP request to the stream URL.
fn make_stream_request(
    url: &str,
    icy_metadata: &Arc<Mutex<IcyMetadata>>,
    metaint: Option<usize>,
) -> Result<Box<dyn Read + Send>, AudioError> {
    let response = ureq::get(url)
        .header("Icy-MetaData", "1")
        .header("User-Agent", "RetroAmp/0.1")
        .config()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(15)))
        .timeout_recv_body(None)
        .build()
        .call()
        .map_err(|e| AudioError::ConnectionFailed(format!("{e}")))?;

    // Re-read metaint from the new response (server may change it, though unlikely).
    let new_metaint: Option<usize> = header_str(response.headers(), "icy-metaint")
        .and_then(|v| v.parse().ok())
        .or(metaint);

    let reader: Box<dyn Read + Send> = if let Some(mi) = new_metaint {
        Box::new(IcyReader::new(
            response.into_body().into_reader(),
            mi,
            Arc::clone(icy_metadata),
        ))
    } else {
        Box::new(response.into_body().into_reader())
    };

    Ok(reader)
}

/// Extract a header value as a String from an http::HeaderMap.
fn header_str(headers: &ureq::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// A `MediaSource` adapter over the ring buffer consumer for the audio thread.
///
/// When the buffer is empty (network stall), briefly spin-waits before
/// returning 0 bytes. This keeps the audio thread responsive while giving
/// the network a chance to catch up.
///
/// The inner consumer is wrapped in a Mutex to satisfy `Sync` (required by
/// Symphonia's `MediaSource`). Since only the audio thread accesses this,
/// the lock is never contended.
pub struct StreamBufReader {
    consumer: Mutex<ringbuf::HeapCons<u8>>,
}

// Safety: StreamBufReader is only accessed from the audio thread. The Mutex
// provides the Sync guarantee that MediaSource requires.
unsafe impl Sync for StreamBufReader {}

impl StreamBufReader {
    pub fn new(consumer: ringbuf::HeapCons<u8>) -> Self {
        Self {
            consumer: Mutex::new(consumer),
        }
    }
}

impl Read for StreamBufReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut consumer = self.consumer.lock().unwrap();

        // Try reading immediately.
        let n = consumer.pop_slice(buf);
        if n > 0 {
            return Ok(n);
        }

        // Buffer empty — spin-wait up to ~200ms. This is long enough for
        // network jitter but short enough to keep the audio thread responsive
        // (a ~200ms stall produces a brief silence gap, not a hang).
        for _ in 0..100 {
            drop(consumer);
            thread::sleep(Duration::from_millis(2));
            consumer = self.consumer.lock().unwrap();
            let n = consumer.pop_slice(buf);
            if n > 0 {
                return Ok(n);
            }
        }

        // Still empty — return 0. Symphonia will see this as EOF;
        // the RadioSource handles it by returning silence.
        Ok(0)
    }
}

impl io::Seek for StreamBufReader {
    fn seek(&mut self, _pos: io::SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "stream is not seekable"))
    }
}

impl symphonia::core::io::MediaSource for StreamBufReader {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}
