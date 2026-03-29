//! Spotify audio source — bridges librespot's audio output into RetroAmp's
//! AudioSource pipeline via a ring buffer.
//!
//! Architecture:
//!   librespot Player → RetroAmpSink → ring buffer → SpotifySource → engine pipeline
//!
//! The SpotifyPlayer manages the librespot Session and Player lifecycle and is
//! stored as Arc<SpotifyPlayer> in Tauri managed state.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;

use librespot::core::config::SessionConfig;
use librespot::core::session::Session;
use librespot::core::spotify_uri::SpotifyUri;
use librespot::playback::audio_backend::{Sink, SinkResult};
use librespot::playback::config::{Bitrate, PlayerConfig};
use librespot::playback::convert::Converter;
use librespot::playback::decoder::AudioPacket;
use librespot::playback::mixer::VolumeGetter;
use librespot::playback::player::{Player, PlayerEvent};

use crate::audio::error::AudioError;
use crate::audio::source::{AudioBuffer, AudioSource, SourceCapabilities, SourceState, TrackMetadata};

/// Ring buffer size in f32 samples. 64K samples = ~0.74s at 44100Hz stereo.
const RING_BUFFER_SAMPLES: usize = 65536;

/// Maximum samples to read per next_buffer() call (4096 frames * 2 channels).
const READ_CHUNK_SAMPLES: usize = 4096 * 2;

// ---------------------------------------------------------------------------
// RetroAmpSink — custom librespot Sink that writes f32 into a ring buffer
// ---------------------------------------------------------------------------

struct RetroAmpSink {
    producer: ringbuf::HeapProd<f32>,
    active: Arc<AtomicBool>,
}

impl Sink for RetroAmpSink {
    fn start(&mut self) -> SinkResult<()> {
        self.active.store(true, Ordering::Release);
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        self.active.store(false, Ordering::Release);
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        if let AudioPacket::Samples(samples) = packet {
            let f32_samples = converter.f64_to_f32(&samples);
            // Push as many samples as possible. If the buffer is full, spin
            // briefly — the audio engine should be draining it in real-time.
            let mut offset = 0;
            let mut spins = 0;
            while offset < f32_samples.len() {
                let pushed = self.producer.push_slice(&f32_samples[offset..]);
                offset += pushed;
                if offset < f32_samples.len() {
                    spins += 1;
                    if spins > 2000 {
                        // Consumer is not draining — drop remaining to avoid deadlock.
                        log::warn!("spotify sink: ring buffer full, dropping {} samples", f32_samples.len() - offset);
                        break;
                    }
                    std::thread::sleep(Duration::from_micros(100));
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SpotifySource — AudioSource implementation reading from the ring buffer
// ---------------------------------------------------------------------------

pub struct SpotifySource {
    consumer: ringbuf::HeapCons<f32>,
    metadata: Arc<Mutex<TrackMetadata>>,
    position_ms: Arc<AtomicU64>,
    _duration_ms: Arc<AtomicU64>,
    sink_active: Arc<AtomicBool>,
    /// Reference to the Player so we can drop it when the source is dropped.
    /// This stops librespot from continuing to write to the ring buffer.
    _player: Arc<Player>,
    state: SourceState,
    /// Count of consecutive empty reads (for underrun detection).
    consecutive_empty: u32,
}

impl AudioSource for SpotifySource {
    fn metadata(&self) -> Result<TrackMetadata, AudioError> {
        let meta = self.metadata.lock().map_err(|e| {
            AudioError::Decode(format!("failed to lock spotify metadata: {e}"))
        })?;
        Ok(meta.clone())
    }

    fn state(&self) -> SourceState {
        self.state
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities {
            can_seek: true,
            has_duration: true,
            has_dynamic_metadata: false,
            is_network_source: true,
        }
    }

    fn next_buffer(&mut self) -> Result<Option<AudioBuffer>, AudioError> {
        if self.state == SourceState::Finished || self.state == SourceState::Error {
            return Ok(None);
        }
        self.state = SourceState::Playing;

        let available = self.consumer.occupied_len();

        if available == 0 {
            if !self.sink_active.load(Ordering::Acquire) {
                // Sink has stopped and buffer is empty — track is done.
                self.state = SourceState::Finished;
                return Ok(None);
            }

            // Buffer underrun — sink is still active, yield a small silence
            // buffer to keep the audio thread running without signalling
            // end-of-track.
            self.consecutive_empty += 1;
            if self.consecutive_empty > 200 {
                // ~2 seconds of continuous underrun — something is wrong.
                log::warn!("spotify source: prolonged underrun, signalling end");
                self.state = SourceState::Error;
                return Ok(None);
            }
            let silence = vec![0.0f32; 1024]; // ~5.8ms at 44100Hz stereo
            return Ok(Some(AudioBuffer {
                samples: silence,
                sample_rate: 44100,
                channels: 2,
            }));
        }

        self.consecutive_empty = 0;

        let to_read = available.min(READ_CHUNK_SAMPLES);
        let mut buf = vec![0.0f32; to_read];
        let read = self.consumer.pop_slice(&mut buf);
        buf.truncate(read);

        Ok(Some(AudioBuffer {
            samples: buf,
            sample_rate: 44100,
            channels: 2,
        }))
    }

    fn seek(&mut self, _position: Duration) -> Result<(), AudioError> {
        // Seeking is handled by the SpotifyPlayer calling Player::seek().
        // We can't do it from here because we don't have the Player reference.
        // The caller (engine/commands) should call SpotifyPlayer::seek() instead.
        Err(AudioError::SeekNotSupported)
    }

    fn position(&self) -> Option<Duration> {
        let ms = self.position_ms.load(Ordering::Relaxed);
        if ms == u64::MAX {
            None
        } else {
            Some(Duration::from_millis(ms))
        }
    }
}

// ---------------------------------------------------------------------------
// NoOpVolume — provides unity gain (RetroAmp handles volume itself)
// ---------------------------------------------------------------------------

struct NoOpVolume;

impl VolumeGetter for NoOpVolume {
    fn attenuation_factor(&self) -> f64 {
        1.0
    }
}

// ---------------------------------------------------------------------------
// SpotifyPlayer — manages the librespot Session and Player lifecycle
// ---------------------------------------------------------------------------

/// Stored OAuth token for Web API requests and session authentication.
#[derive(Debug, Clone)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: std::time::Instant,
}

/// Manages Spotify integration: OAuth tokens for Web API, and librespot
/// Session/Player for audio playback.
///
/// Design: The OAuth token is the source of truth for "connected" state.
/// librespot Sessions are created on-demand when playing a track (not kept
/// alive persistently), avoiding session lifecycle issues.
pub struct SpotifyPlayer {
    /// A connected librespot Session for audio playback. Created during login
    /// or auto-reconnect (where Tokio is available), reused for all tracks.
    playback_session: Mutex<Option<Session>>,
    /// The currently active librespot Player (one per track).
    player: Mutex<Option<Arc<Player>>>,
    /// OAuth token for authentication and Web API requests.
    api_token: Mutex<Option<StoredToken>>,
    /// Username from the last successful login (cached for display).
    username: Mutex<Option<String>>,
    /// Account type from the last successful login (cached for display).
    account_type: Mutex<Option<String>>,
    /// Shared metadata for the currently playing track.
    current_metadata: Arc<Mutex<TrackMetadata>>,
    /// Current playback position in milliseconds (updated by librespot events).
    position_ms: Arc<AtomicU64>,
    /// Duration of the current track in milliseconds.
    duration_ms: Arc<AtomicU64>,
    /// Whether the sink is actively producing audio.
    sink_active: Arc<AtomicBool>,
    /// Path to the librespot cache directory for credentials and audio.
    cache_dir: Option<PathBuf>,
}

impl SpotifyPlayer {
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self {
            playback_session: Mutex::new(None),
            player: Mutex::new(None),
            api_token: Mutex::new(None),
            username: Mutex::new(None),
            account_type: Mutex::new(None),
            current_metadata: Arc::new(Mutex::new(TrackMetadata {
                title: None,
                artist: None,
                album: None,
                duration: None,
                sample_rate: 44100,
                channels: 2,
                bitrate: Some(320),
                genre: None,
                year: None,
                track_number: None,
                cover_art: None,
            })),
            position_ms: Arc::new(AtomicU64::new(u64::MAX)),
            duration_ms: Arc::new(AtomicU64::new(u64::MAX)),
            sink_active: Arc::new(AtomicBool::new(false)),
            cache_dir,
        }
    }

    /// Store an OAuth token after successful login or token refresh.
    pub fn set_api_token(&self, token: StoredToken) {
        if let Ok(mut guard) = self.api_token.lock() {
            *guard = Some(token);
        }
    }

    /// Get the current OAuth access token for Web API requests.
    pub fn api_access_token(&self) -> Option<String> {
        self.api_token.lock().ok()?.as_ref().map(|t| t.access_token.clone())
    }

    /// Whether we have a valid OAuth token (i.e. "connected").
    pub fn is_connected(&self) -> bool {
        self.api_token.lock().ok()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    /// Store user info after login (for display in the UI).
    pub fn set_user_info(&self, username: String, account_type: Option<String>) {
        if let Ok(mut g) = self.username.lock() { *g = Some(username); }
        if let Ok(mut g) = self.account_type.lock() { *g = account_type; }
    }

    /// Get the cached username.
    pub fn username(&self) -> Option<String> {
        self.username.lock().ok()?.clone()
    }

    /// Get the cached account type.
    pub fn account_type(&self) -> Option<String> {
        self.account_type.lock().ok()?.clone()
    }

    /// Store a connected librespot Session for playback. Must be created and
    /// connected in a Tokio context (during login or auto-reconnect).
    pub fn set_playback_session(&self, session: Session) {
        if let Ok(mut g) = self.playback_session.lock() { *g = Some(session); }
    }

    /// Get the cached playback session.
    fn playback_session(&self) -> Option<Session> {
        self.playback_session.lock().ok()?.clone()
    }

    /// Get the cache directory path.
    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
    }

    /// Disconnect from Spotify. Clears the token and cached credentials.
    pub fn disconnect(&self) -> Result<(), String> {
        if let Ok(mut g) = self.player.lock() { *g = None; }
        if let Ok(mut g) = self.playback_session.lock() { *g = None; }
        if let Ok(mut g) = self.api_token.lock() { *g = None; }
        if let Ok(mut g) = self.username.lock() { *g = None; }
        if let Ok(mut g) = self.account_type.lock() { *g = None; }
        if let Some(dir) = &self.cache_dir {
            crate::spotify::auth::clear_cached_credentials(dir);
        }
        log::info!("Spotify disconnected");
        Ok(())
    }

    /// Load a Spotify track and return an AudioSource that produces its audio.
    /// Uses the pre-connected playback session (created during login).
    pub fn load_track(&self, track_uri: &str) -> Result<Box<dyn AudioSource>, String> {
        // Drop the previous Player first — it holds a Sink that writes to the
        // old ring buffer, and would keep running in the background otherwise.
        if let Ok(mut g) = self.player.lock() { *g = None; }

        let session = self.playback_session()
            .ok_or("Spotify playback session not available — please log in again")?;

        let uri = SpotifyUri::from_uri(track_uri)
            .map_err(|e| format!("Invalid Spotify URI '{track_uri}': {e}"))?;

        // Create a new ring buffer for this track.
        let rb = HeapRb::<f32>::new(RING_BUFFER_SAMPLES);
        let (producer, consumer) = rb.split();

        let sink_active = Arc::clone(&self.sink_active);
        let metadata = Arc::clone(&self.current_metadata);
        let position_ms = Arc::clone(&self.position_ms);
        let duration_ms = Arc::clone(&self.duration_ms);

        // Reset state for the new track. Start with sink_active=true so that
        // SpotifySource doesn't signal end-of-track before librespot starts
        // writing. The sink's stop() will set it to false when the track ends.
        self.sink_active.store(true, Ordering::Release);
        self.position_ms.store(0, Ordering::Release);
        self.duration_ms.store(u64::MAX, Ordering::Release);

        // Create the librespot Player with our custom sink.
        let sink_active_for_sink = Arc::clone(&sink_active);
        let player_config = PlayerConfig {
            bitrate: Bitrate::Bitrate320,
            ..PlayerConfig::default()
        };

        let player = Player::new(
            player_config,
            session,
            Box::new(NoOpVolume),
            move || -> Box<dyn Sink> {
                Box::new(RetroAmpSink {
                    producer,
                    active: sink_active_for_sink,
                })
            },
        );

        // Start the event processing thread.
        let mut event_channel = player.get_player_event_channel();
        let meta_for_events = Arc::clone(&metadata);
        let pos_for_events = Arc::clone(&position_ms);
        let dur_for_events = Arc::clone(&duration_ms);
        std::thread::Builder::new()
            .name("retroamp-spotify-events".into())
            .spawn(move || {
                while let Some(event) = event_channel.blocking_recv() {
                    match event {
                        PlayerEvent::Playing { position_ms: pos, .. } => {
                            pos_for_events.store(pos as u64, Ordering::Release);
                        }
                        PlayerEvent::Paused { position_ms: pos, .. } => {
                            pos_for_events.store(pos as u64, Ordering::Release);
                        }
                        PlayerEvent::PositionChanged { position_ms: pos, .. }
                        | PlayerEvent::PositionCorrection { position_ms: pos, .. }
                        | PlayerEvent::Seeked { position_ms: pos, .. } => {
                            pos_for_events.store(pos as u64, Ordering::Release);
                        }
                        PlayerEvent::TrackChanged { audio_item } => {
                            let duration = Duration::from_millis(audio_item.duration_ms as u64);
                            dur_for_events.store(audio_item.duration_ms as u64, Ordering::Release);

                            // Extract artist and album from UniqueFields.
                            let (artist, album) = match &audio_item.unique_fields {
                                librespot::metadata::audio::item::UniqueFields::Track {
                                    artists, album, ..
                                } => {
                                    let artist_str = artists
                                        .0
                                        .iter()
                                        .map(|a| a.name.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    (Some(artist_str), Some(album.clone()))
                                }
                                _ => (None, None),
                            };

                            if let Ok(mut meta) = meta_for_events.lock() {
                                meta.title = Some(audio_item.name.clone());
                                meta.artist = artist;
                                meta.album = album;
                                meta.duration = Some(duration);
                                meta.sample_rate = 44100;
                                meta.channels = 2;
                                meta.bitrate = Some(320);
                            }
                        }
                        PlayerEvent::EndOfTrack { .. }
                        | PlayerEvent::Stopped { .. } => {
                            // The sink's stop() will set sink_active to false,
                            // which SpotifySource uses to signal track end.
                            break;
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn spotify event thread: {e}"))?;

        // Tell librespot to load and start playing the track.
        player.load(uri, true, 0);

        // Store the player reference and also pass it to SpotifySource so the
        // Player is dropped when the engine drops the source (stops playback).
        let player_clone = Arc::clone(&player);
        if let Ok(mut guard) = self.player.lock() {
            *guard = Some(player);
        }

        // Return the AudioSource that reads from the consumer side.
        Ok(Box::new(SpotifySource {
            consumer,
            metadata,
            position_ms,
            _duration_ms: duration_ms,
            sink_active,
            _player: player_clone,
            state: SourceState::Ready,
            consecutive_empty: 0,
        }))
    }

    /// Seek the current Spotify track to the given position.
    pub fn seek(&self, position_ms: u32) {
        if let Ok(guard) = self.player.lock() {
            if let Some(ref player) = *guard {
                player.seek(position_ms);
            }
        }
    }
}
