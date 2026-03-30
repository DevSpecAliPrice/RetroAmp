//! The audio engine — orchestrates the full playback pipeline.
//!
//! Owns the audio thread, manages the current source, and drives the pipeline:
//!   AudioSource → [Resample if needed] → EQ → FFT → Output
//!
//! When a new track loads, the engine reconfigures the output device to match
//! the source's native sample rate (bit-perfect path). If the device doesn't
//! support that rate, it falls back to resampling.

use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::audio::eq::{EqSettings, Equalizer};
use crate::audio::error::AudioError;
use crate::audio::fft::{FftAnalyser, FftData};
use crate::audio::output::{OutputManager, BUFFER_SECS_LOCAL, BUFFER_SECS_STREAM};
use crate::audio::resample::AudioResampler;
use crate::audio::source::{AudioSource, TrackMetadata};

/// Commands sent from the main thread to the audio thread.
pub enum EngineCommand {
    Play(Box<dyn AudioSource>),
    Pause,
    Resume,
    Stop,
    Seek(Duration),
    SetEq(EqSettings),
    SetVolume(f32),
    SetBalance(f32),
    Shutdown,
}

/// Playback state reported to the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Current state snapshot for the frontend to display.
#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub state: PlaybackState,
    pub position: Option<f64>,
    pub duration: Option<f64>,
    pub metadata: Option<TrackMetadata>,
    pub volume: f32,
    pub balance: f32,
    /// Whether the current source supports seeking.
    pub can_seek: bool,
    /// Whether the current source has a known duration.
    pub has_duration: bool,
    /// Whether the current source is an internet stream.
    pub is_stream: bool,
}

/// Events emitted by the audio engine.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// The current source finished playing (reached end of data).
    TrackFinished,
}

/// What the engine should do after a fade-out completes.
enum FadeAction {
    /// No pending action.
    None,
    /// Stop playback and clear the source.
    Stop,
    /// Pause playback (keep source intact).
    Pause,
    /// Switch to a new source (stored separately) and fade in.
    Switch,
}

/// Duration of micro-fades in seconds (~15ms — inaudible but eliminates clicks).
const FADE_SECS: f32 = 0.015;

/// Handle held by the main thread to control the audio engine.
///
/// All methods are non-blocking — they send commands to the audio thread.
pub struct AudioEngine {
    command_tx: mpsc::Sender<EngineCommand>,
    status: Arc<Mutex<EngineStatus>>,
    fft_data: Arc<Mutex<FftData>>,
    event_rx: Mutex<mpsc::Receiver<EngineEvent>>,
    _thread: thread::JoinHandle<()>,
}

impl AudioEngine {
    /// Start the audio engine. Spawns the audio thread, which opens the
    /// default output device internally (CPAL streams are not Send).
    pub fn new() -> Result<Self, AudioError> {
        let (command_tx, command_rx) = mpsc::channel::<EngineCommand>();
        let (init_tx, init_rx) = mpsc::channel::<Result<(), AudioError>>();
        let (event_tx, event_rx) = mpsc::channel::<EngineEvent>();

        let status = Arc::new(Mutex::new(EngineStatus {
            state: PlaybackState::Stopped,
            position: None,
            duration: None,
            metadata: None,
            volume: 1.0,
            balance: 0.0,
            can_seek: false,
            has_duration: false,
            is_stream: false,
        }));

        let fft_data = Arc::new(Mutex::new(FftData {
            magnitudes: vec![0.0; 512],
            waveform: vec![0.0; 1024],
            sample_rate: 44100,
        }));

        let status_for_thread = Arc::clone(&status);
        let fft_for_thread = Arc::clone(&fft_data);

        let thread = thread::Builder::new()
            .name("retroamp-audio".into())
            .spawn(move || {
                audio_thread(command_rx, init_tx, event_tx, status_for_thread, fft_for_thread);
            })
            .map_err(|e| AudioError::Output(format!("failed to spawn audio thread: {e}")))?;

        match init_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(AudioError::Output(
                    "audio thread exited during initialisation".into(),
                ))
            }
        }

        Ok(Self {
            command_tx,
            status,
            fft_data,
            event_rx: Mutex::new(event_rx),
            _thread: thread,
        })
    }

    pub fn play(&self, source: Box<dyn AudioSource>) {
        let _ = self.command_tx.send(EngineCommand::Play(source));
    }

    pub fn pause(&self) {
        let _ = self.command_tx.send(EngineCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.command_tx.send(EngineCommand::Resume);
    }

    pub fn stop(&self) {
        let _ = self.command_tx.send(EngineCommand::Stop);
    }

    pub fn seek(&self, position: Duration) {
        let _ = self.command_tx.send(EngineCommand::Seek(position));
    }

    pub fn set_eq(&self, settings: EqSettings) {
        let _ = self.command_tx.send(EngineCommand::SetEq(settings));
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.command_tx.send(EngineCommand::SetVolume(volume));
    }

    pub fn set_balance(&self, balance: f32) {
        let _ = self.command_tx.send(EngineCommand::SetBalance(balance));
    }

    pub fn status(&self) -> EngineStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn fft_data(&self) -> FftData {
        self.fft_data.lock().unwrap().clone()
    }

    pub fn shutdown(&self) {
        let _ = self.command_tx.send(EngineCommand::Shutdown);
    }

    /// Try to receive a pending engine event (non-blocking).
    /// Used by the auto-advance listener.
    pub fn try_recv_event(&self) -> Option<EngineEvent> {
        if let Ok(rx) = self.event_rx.lock() {
            rx.try_recv().ok()
        } else {
            None
        }
    }
}

/// The audio thread's main loop.
fn audio_thread(
    command_rx: mpsc::Receiver<EngineCommand>,
    init_tx: mpsc::Sender<Result<(), AudioError>>,
    event_tx: mpsc::Sender<EngineEvent>,
    status: Arc<Mutex<EngineStatus>>,
    fft_data: Arc<Mutex<FftData>>,
) {
    // Create the output manager (holds the device reference).
    let output_manager = match OutputManager::new() {
        Ok(m) => m,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    // Open at the device's default rate initially.
    let mut output = match output_manager.open_default() {
        Ok(o) => o,
        Err(e) => {
            let _ = init_tx.send(Err(e));
            return;
        }
    };

    let _ = init_tx.send(Ok(()));

    let mut current_rate = output.config().sample_rate;
    let mut current_channels = output.config().channels;
    eprintln!("[retroamp] output device opened: {current_rate}Hz, {current_channels}ch");

    if let Ok(mut fft) = fft_data.lock() {
        fft.sample_rate = current_rate;
    }

    let mut source: Option<Box<dyn AudioSource>> = None;
    let mut resampler: Option<AudioResampler> = None;
    let mut playback_state = PlaybackState::Stopped;
    let mut volume: f32 = 1.0;
    let mut balance: f32 = 0.0;
    let mut current_is_stream = false;
    let mut eq = Equalizer::new(current_rate, current_channels);
    let mut fft_analyser = FftAnalyser::new();

    // Micro-fade state: ~15ms gain ramp to eliminate clicks on transitions.
    let mut fade_gain: f32 = 1.0;
    let mut fade_target: f32 = 1.0;
    let mut fade_step: f32 = 1.0 / (current_rate as f32 * FADE_SECS);
    let mut fade_action = FadeAction::None;
    let mut pending_source: Option<Box<dyn AudioSource>> = None;
    let mut activate_pending = false;

    loop {
        while let Ok(cmd) = command_rx.try_recv() {
            match cmd {
                EngineCommand::Play(new_source) => {
                    pending_source = Some(new_source);
                    if playback_state == PlaybackState::Playing && fade_gain > 0.01 {
                        // Fade out current track, then switch.
                        fade_target = 0.0;
                        fade_step = 1.0 / (current_rate as f32 * FADE_SECS);
                        fade_action = FadeAction::Switch;
                    } else {
                        // Not audibly playing — activate immediately with fade-in.
                        activate_pending = true;
                    }
                }
                EngineCommand::Pause => {
                    if playback_state == PlaybackState::Playing {
                        pending_source = None;
                        activate_pending = false;
                        if fade_gain > 0.01 {
                            fade_target = 0.0;
                            fade_step = 1.0 / (current_rate as f32 * FADE_SECS);
                            fade_action = FadeAction::Pause;
                        } else {
                            playback_state = PlaybackState::Paused;
                            fade_action = FadeAction::None;
                            update_status(&status, &source, playback_state, volume, balance);
                        }
                    }
                }
                EngineCommand::Resume => {
                    if playback_state == PlaybackState::Paused {
                        playback_state = PlaybackState::Playing;
                        fade_gain = 0.0;
                        fade_target = 1.0;
                        fade_step = 1.0 / (current_rate as f32 * FADE_SECS);
                        update_status(&status, &source, playback_state, volume, balance);
                    }
                }
                EngineCommand::Stop => {
                    pending_source = None;
                    activate_pending = false;
                    match playback_state {
                        PlaybackState::Playing => {
                            if fade_gain > 0.01 {
                                fade_target = 0.0;
                                fade_step = 1.0 / (current_rate as f32 * FADE_SECS);
                                fade_action = FadeAction::Stop;
                            } else {
                                source = None;
                                resampler = None;
                                playback_state = PlaybackState::Stopped;
                                fade_gain = 1.0;
                                fade_target = 1.0;
                                fade_action = FadeAction::None;
                                update_status(&status, &source, playback_state, volume, balance);
                            }
                        }
                        PlaybackState::Paused => {
                            source = None;
                            resampler = None;
                            playback_state = PlaybackState::Stopped;
                            fade_gain = 1.0;
                            fade_target = 1.0;
                            fade_action = FadeAction::None;
                            update_status(&status, &source, playback_state, volume, balance);
                        }
                        _ => {}
                    }
                }
                EngineCommand::Seek(pos) => {
                    if let Some(src) = source.as_mut() {
                        if let Err(e) = src.seek(pos) {
                            log::warn!("seek failed: {e}");
                        }
                    }
                }
                EngineCommand::SetEq(settings) => {
                    eq.apply_settings(&settings);
                }
                EngineCommand::SetVolume(v) => {
                    volume = v.clamp(0.0, 1.0);
                    update_status(&status, &source, playback_state, volume, balance);
                }
                EngineCommand::SetBalance(b) => {
                    balance = b.clamp(-1.0, 1.0);
                    update_status(&status, &source, playback_state, volume, balance);
                }
                EngineCommand::Shutdown => {
                    return;
                }
            }
        }

        // Activate a pending source (deferred from Play command or fade-out switch).
        if activate_pending {
            activate_pending = false;
            if let Some(new_source) = pending_source.take() {
                resampler = None;
                let caps = new_source.capabilities();
                let is_stream = caps.has_dynamic_metadata;
                let is_network = caps.is_network_source;
                let buffer_secs = if is_network { BUFFER_SECS_STREAM } else { BUFFER_SECS_LOCAL };

                if let Ok(meta) = new_source.metadata() {
                    let source_rate = meta.sample_rate;
                    eprintln!(
                        "[retroamp] source: {}Hz, {}ch | output: {current_rate}Hz | stream: {is_stream}",
                        source_rate, meta.channels
                    );

                    // Reopen output if sample rate changed or buffer sizing changed
                    // (local ↔ stream).
                    //
                    // Only reconfigure the output to "standard" rates (≥ 32 kHz).
                    // Switching to unusual rates like 22050 Hz causes audible pops
                    // from the hardware/PipeWire resetting. For non-standard rates
                    // we keep the current output rate and resample instead.
                    let rate_is_standard = source_rate >= 32000;
                    let need_rate_change = source_rate != current_rate && rate_is_standard;
                    let need_buffer_change = is_stream != current_is_stream;

                    if need_rate_change || need_buffer_change {
                        // Fill remaining output buffer with silence before
                        // tearing down the CPAL stream — prevents pops from
                        // abrupt mid-sample cutoff on rate changes.
                        output.flush_with_silence();

                        let target_rate = if rate_is_standard { source_rate } else { current_rate };
                        eprintln!("[retroamp] reconfiguring output (rate or buffer change)");
                        match output_manager.open_at_rate(target_rate, meta.channels, buffer_secs) {
                            Ok(new_output) => {
                                output = new_output;
                                current_rate = target_rate;
                                current_channels = output.config().channels;
                                current_is_stream = is_stream;
                                eq.reconfigure(current_rate, current_channels);
                                eprintln!(
                                    "[retroamp] output reconfigured to {target_rate}Hz, buffer {:.0}ms",
                                    buffer_secs * 1000.0
                                );
                                log::info!(
                                    "output reconfigured to {target_rate}Hz, buffer {:.0}ms",
                                    buffer_secs * 1000.0
                                );
                            }
                            Err(ref e) => {
                                eprintln!("[retroamp] reconfigure failed: {e}, falling back to resampler");
                                log::info!(
                                    "device doesn't support {target_rate}Hz, \
                                     resampling to {current_rate}Hz"
                                );
                                if need_buffer_change {
                                    match output_manager.open_at_rate(current_rate, current_channels, buffer_secs) {
                                        Ok(new_output) => {
                                            output = new_output;
                                            current_is_stream = is_stream;
                                            eprintln!(
                                                "[retroamp] buffer resized to {:.0}ms (keeping {current_rate}Hz)",
                                                buffer_secs * 1000.0
                                            );
                                        }
                                        Err(_) => {}
                                    }
                                }
                            }
                        }
                    }

                    // Create resampler if the source rate doesn't match the output.
                    if source_rate != current_rate {
                        match AudioResampler::new(source_rate, current_rate, meta.channels) {
                            Ok(r) => {
                                eprintln!("[retroamp] resampler: {source_rate}Hz → {current_rate}Hz");
                                resampler = Some(r);
                            }
                            Err(e) => {
                                eprintln!("[retroamp] resampler FAILED: {e}");
                                log::error!("failed to create resampler: {e}");
                            }
                        }
                    }
                }

                source = Some(new_source);
                playback_state = PlaybackState::Playing;
                fade_gain = 0.0;
                fade_target = 1.0;
                fade_step = 1.0 / (current_rate as f32 * FADE_SECS);
                update_status(&status, &source, playback_state, volume, balance);
            }
        }

        if playback_state != PlaybackState::Playing {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        // Pull audio from the source.
        let buffer = match source.as_mut() {
            Some(src) => match src.next_buffer() {
                Ok(Some(buf)) => buf,
                Ok(None) => {
                    // Source exhausted — notify for auto-advance.
                    playback_state = PlaybackState::Stopped;
                    source = None;
                    resampler = None;
                    update_status(&status, &source, playback_state, volume, balance);
                    let _ = event_tx.send(EngineEvent::TrackFinished);
                    continue;
                }
                Err(e) => {
                    log::error!("source error: {e}");
                    playback_state = PlaybackState::Stopped;
                    source = None;
                    resampler = None;
                    update_status(&status, &source, playback_state, volume, balance);
                    let _ = event_tx.send(EngineEvent::TrackFinished);
                    continue;
                }
            },
            None => {
                playback_state = PlaybackState::Stopped;
                update_status(&status, &source, playback_state, volume, balance);
                continue;
            }
        };

        // Resample only if the device couldn't match the source rate.
        let mut samples = if let Some(ref mut r) = resampler {
            match r.process(&buffer.samples) {
                Ok(resampled) => {
                    if resampled.is_empty() {
                        continue;
                    }
                    resampled
                }
                Err(e) => {
                    log::error!("resample error: {e}");
                    continue;
                }
            }
        } else {
            buffer.samples
        };

        // Everything from here is at current_rate (either native or resampled).
        eq.process(&mut samples);

        fft_analyser.process(&samples, current_channels);
        if let Ok(mut fft) = fft_data.lock() {
            *fft = fft_analyser.current_data(current_rate);
        }

        // Apply volume and balance (pan). For stereo interleaved [L, R, L, R, ...]:
        // balance: -1.0 = full left, 0.0 = center, 1.0 = full right.
        let left_gain = volume * if balance > 0.0 { 1.0 - balance } else { 1.0 };
        let right_gain = volume * if balance < 0.0 { 1.0 + balance } else { 1.0 };
        let needs_gain = (left_gain - 1.0).abs() > f32::EPSILON
            || (right_gain - 1.0).abs() > f32::EPSILON;

        if needs_gain && current_channels >= 2 {
            for frame in samples.chunks_exact_mut(current_channels as usize) {
                frame[0] *= left_gain;
                frame[1] *= right_gain;
            }
        } else if needs_gain {
            // Mono — just apply volume, balance has no effect.
            for sample in &mut samples {
                *sample *= volume;
            }
        }

        // Apply micro-fade envelope to eliminate clicks on transitions.
        if (fade_gain - fade_target).abs() > f32::EPSILON {
            let frame_size = current_channels as usize;
            for frame in samples.chunks_exact_mut(frame_size) {
                if fade_target > fade_gain {
                    fade_gain = (fade_gain + fade_step).min(1.0);
                } else {
                    fade_gain = (fade_gain - fade_step).max(0.0);
                }
                for s in frame.iter_mut() {
                    *s *= fade_gain;
                }
            }

            // Fade-out completed — execute the pending action.
            if fade_gain <= 0.0 {
                match fade_action {
                    FadeAction::Stop => {
                        source = None;
                        resampler = None;
                        playback_state = PlaybackState::Stopped;
                        fade_gain = 1.0;
                        fade_target = 1.0;
                        update_status(&status, &source, playback_state, volume, balance);
                    }
                    FadeAction::Pause => {
                        playback_state = PlaybackState::Paused;
                        update_status(&status, &source, playback_state, volume, balance);
                    }
                    FadeAction::Switch => {
                        activate_pending = true;
                    }
                    FadeAction::None => {}
                }
                fade_action = FadeAction::None;
            }
        } else if fade_gain < 1.0 - f32::EPSILON {
            // Stable at sub-unity gain (e.g. mid-switch gap) — silence output.
            for s in samples.iter_mut() {
                *s *= fade_gain;
            }
        }

        // Write ALL samples to the output, waiting for space as needed.
        // Never discard — dropping samples causes clicks at every boundary.
        let mut write_offset = 0;
        while write_offset < samples.len() {
            let written = output.write(&samples[write_offset..]);
            write_offset += written;
            if write_offset < samples.len() {
                thread::sleep(Duration::from_micros(500));
            }
        }

        update_status(&status, &source, playback_state, volume, balance);
    }
}

fn update_status(
    status: &Arc<Mutex<EngineStatus>>,
    source: &Option<Box<dyn AudioSource>>,
    state: PlaybackState,
    volume: f32,
    balance: f32,
) {
    if let Ok(mut s) = status.lock() {
        s.state = state;
        s.volume = volume;
        s.balance = balance;
        if let Some(src) = source {
            let caps = src.capabilities();
            s.can_seek = caps.can_seek;
            s.has_duration = caps.has_duration;
            s.is_stream = caps.has_dynamic_metadata;
            s.position = src.position().map(|d| d.as_secs_f64());
            if let Ok(meta) = src.metadata() {
                s.duration = meta.duration.map(|d| d.as_secs_f64());
                s.metadata = Some(meta);
            }
        } else {
            s.position = None;
            s.duration = None;
            s.metadata = None;
            s.can_seek = false;
            s.has_duration = false;
            s.is_stream = false;
        }
    }
}
