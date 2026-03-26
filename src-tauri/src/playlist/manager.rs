//! Playlist manager — owns the track list, handles add/remove/reorder,
//! and coordinates with the playback sequence and audio engine.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

use crate::playlist::queue::Queue;
use crate::playlist::sequence::{PlaybackSequence, RepeatMode, ShuffleMode};
use crate::playlist::track::{Track, TrackId};

/// Global track ID counter.
static NEXT_TRACK_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> TrackId {
    NEXT_TRACK_ID.fetch_add(1, Ordering::Relaxed)
}

/// A serializable snapshot of the playlist state for the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct PlaylistState {
    pub tracks: Vec<PlaylistEntry>,
    pub current_index: Option<usize>,
    pub current_track_id: Option<TrackId>,
    pub shuffle: ShuffleMode,
    pub repeat: RepeatMode,
    pub total_duration: Option<f64>,
    pub track_count: usize,
}

/// A single entry in the playlist, formatted for the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct PlaylistEntry {
    pub id: TrackId,
    pub display_name: String,
    pub duration: String,
    pub is_current: bool,
    pub is_selected: bool,
    pub is_stream: bool,
}

/// The playlist manager.
pub struct PlaylistManager {
    tracks: Vec<Track>,
    current_index: Option<usize>,
    selected: Vec<TrackId>,
    sequence: PlaybackSequence,
    queue: Queue,
}

impl PlaylistManager {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            current_index: None,
            selected: Vec::new(),
            sequence: PlaybackSequence::new(),
            queue: Queue::new(),
        }
    }

    // -- Track management --

    /// Add a track at the end of the playlist. Returns the assigned TrackId.
    pub fn add_track(&mut self, path: impl Into<String>) -> TrackId {
        let id = next_id();
        let track = Track::from_path(id, path);
        self.tracks.push(track);
        self.sequence
            .track_count_changed(self.tracks.len(), self.current_index);
        id
    }

    /// Add multiple tracks at the end.
    pub fn add_tracks(&mut self, paths: Vec<String>) -> Vec<TrackId> {
        let ids: Vec<TrackId> = paths
            .into_iter()
            .map(|path| {
                let id = next_id();
                self.tracks.push(Track::from_path(id, path));
                id
            })
            .collect();
        self.sequence
            .track_count_changed(self.tracks.len(), self.current_index);
        ids
    }

    /// Remove tracks by ID.
    pub fn remove_tracks(&mut self, ids: &[TrackId]) {
        let current_id = self.current_track().map(|t| t.id);
        self.tracks.retain(|t| !ids.contains(&t.id));
        self.selected.retain(|id| !ids.contains(id));
        self.queue.remove_tracks(ids);

        // Update current_index if it was affected.
        if let Some(cid) = current_id {
            self.current_index = self.tracks.iter().position(|t| t.id == cid);
        }
        self.sequence
            .track_count_changed(self.tracks.len(), self.current_index);
    }

    /// Remove selected tracks.
    pub fn remove_selected(&mut self) {
        let selected = self.selected.clone();
        self.remove_tracks(&selected);
    }

    /// Clear the entire playlist.
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.current_index = None;
        self.selected.clear();
        self.queue.clear();
        self.sequence.track_count_changed(0, None);
    }

    /// Move a track from one position to another.
    pub fn move_track(&mut self, from: usize, to: usize) {
        if from >= self.tracks.len() || to >= self.tracks.len() {
            return;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);

        // Update current_index to follow the playing track.
        if let Some(idx) = self.current_index {
            if idx == from {
                self.current_index = Some(to);
            } else if from < idx && to >= idx {
                self.current_index = Some(idx - 1);
            } else if from > idx && to <= idx {
                self.current_index = Some(idx + 1);
            }
        }
    }

    /// Sort the playlist by display name.
    pub fn sort_by_title(&mut self) {
        let current_id = self.current_track().map(|t| t.id);
        self.tracks.sort_by(|a, b| a.display_name().cmp(&b.display_name()));
        if let Some(cid) = current_id {
            self.current_index = self.tracks.iter().position(|t| t.id == cid);
        }
    }

    /// Reverse the playlist order.
    pub fn reverse(&mut self) {
        let current_id = self.current_track().map(|t| t.id);
        self.tracks.reverse();
        if let Some(cid) = current_id {
            self.current_index = self.tracks.iter().position(|t| t.id == cid);
        }
    }

    /// Randomize the playlist order.
    pub fn randomize(&mut self) {
        use rand::seq::SliceRandom;
        let current_id = self.current_track().map(|t| t.id);
        let mut rng = rand::rng();
        self.tracks.shuffle(&mut rng);
        if let Some(cid) = current_id {
            self.current_index = self.tracks.iter().position(|t| t.id == cid);
        }
    }

    // -- Metadata --

    /// Update a track's metadata (called after tag loading completes).
    pub fn update_metadata(&mut self, id: TrackId, meta: &crate::audio::source::TrackMetadata) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == id) {
            track.title = meta.title.clone();
            track.artist = meta.artist.clone();
            track.album = meta.album.clone();
            track.genre = meta.genre.clone();
            track.year = meta.year;
            track.track_number = meta.track_number;
            track.duration = meta.duration;
            track.sample_rate = Some(meta.sample_rate);
            track.channels = Some(meta.channels);
            track.metadata_loaded = true;
        }
    }

    /// Set the station name for a radio stream track. This name is always
    /// used for display in the playlist, regardless of ICY metadata updates.
    pub fn update_display_name(&mut self, id: TrackId, name: &str) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == id) {
            track.station_name = Some(name.to_string());
        }
    }

    // -- Selection --

    pub fn select_track(&mut self, id: TrackId) {
        self.selected = vec![id];
    }

    pub fn toggle_select(&mut self, id: TrackId) {
        if let Some(pos) = self.selected.iter().position(|&sid| sid == id) {
            self.selected.remove(pos);
        } else {
            self.selected.push(id);
        }
    }

    pub fn select_all(&mut self) {
        self.selected = self.tracks.iter().map(|t| t.id).collect();
    }

    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    pub fn invert_selection(&mut self) {
        let all_ids: Vec<TrackId> = self.tracks.iter().map(|t| t.id).collect();
        self.selected = all_ids
            .into_iter()
            .filter(|id| !self.selected.contains(id))
            .collect();
    }

    // -- Playback navigation --

    /// Get the currently playing track.
    pub fn current_track(&self) -> Option<&Track> {
        self.current_index.and_then(|i| self.tracks.get(i))
    }

    /// Set the current track by index (e.g. user double-clicked a track).
    pub fn play_index(&mut self, index: usize) -> Option<&Track> {
        if index < self.tracks.len() {
            self.current_index = Some(index);
            self.sequence.set_current(index);
            self.tracks.get(index)
        } else {
            None
        }
    }

    /// Set the current track by ID.
    pub fn play_track(&mut self, id: TrackId) -> Option<&Track> {
        if let Some(index) = self.tracks.iter().position(|t| t.id == id) {
            self.play_index(index)
        } else {
            None
        }
    }

    /// Advance to the next track. Checks the queue first, then the
    /// playback sequence. Returns the next track, or None if playback
    /// should stop.
    pub fn next_track(&mut self) -> Option<&Track> {
        // Queue takes priority.
        if let Some(queued_id) = self.queue.take_next() {
            if let Some(index) = self.tracks.iter().position(|t| t.id == queued_id) {
                self.current_index = Some(index);
                self.sequence.set_current(index);
                return self.tracks.get(index);
            }
        }

        // Otherwise consult the sequence.
        let current = self.current_index.unwrap_or(0);
        if let Some(next_idx) = self.sequence.next_index(current, self.tracks.len()) {
            self.current_index = Some(next_idx);
            self.tracks.get(next_idx)
        } else {
            self.current_index = None;
            None
        }
    }

    /// Go to the previous track.
    pub fn previous_track(&mut self) -> Option<&Track> {
        let current = self.current_index.unwrap_or(0);
        if let Some(prev_idx) = self.sequence.previous_index(current, self.tracks.len()) {
            self.current_index = Some(prev_idx);
            self.tracks.get(prev_idx)
        } else {
            None
        }
    }

    /// Peek at what the next track will be (for gapless pre-loading)
    /// without actually advancing.
    pub fn peek_next(&self) -> Option<&Track> {
        if let Some(queued_id) = self.queue.peek_next() {
            return self.tracks.iter().find(|t| t.id == *queued_id);
        }

        // Clone the sequence to peek without mutating.
        let current = self.current_index.unwrap_or(0);
        // For peeking, we can use a simple check without mutating shuffle state.
        match self.sequence.shuffle {
            ShuffleMode::Off => {
                let next = current + 1;
                if next < self.tracks.len() {
                    self.tracks.get(next)
                } else if self.sequence.repeat == RepeatMode::Playlist {
                    self.tracks.first()
                } else {
                    None
                }
            }
            ShuffleMode::All => {
                // Can't easily peek into shuffle without mutating.
                // Return None for now — gapless will work in sequential mode.
                None
            }
        }
    }

    // -- Shuffle / Repeat --

    pub fn set_shuffle(&mut self, mode: ShuffleMode) {
        self.sequence.shuffle = mode;
        if mode == ShuffleMode::All {
            self.sequence
                .reshuffle(self.tracks.len(), self.current_index);
        }
    }

    pub fn toggle_shuffle(&mut self) {
        let new_mode = match self.sequence.shuffle {
            ShuffleMode::Off => ShuffleMode::All,
            ShuffleMode::All => ShuffleMode::Off,
        };
        self.set_shuffle(new_mode);
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.sequence.repeat = mode;
    }

    pub fn cycle_repeat(&mut self) {
        self.sequence.repeat = match self.sequence.repeat {
            RepeatMode::Off => RepeatMode::Playlist,
            _ => RepeatMode::Off,
        };
    }

    // -- Queue --

    /// Add a track to the play-next queue.
    pub fn queue_track(&mut self, id: TrackId) {
        self.queue.add(id);
    }

    // -- State snapshot --

    /// Build a state snapshot for the frontend.
    pub fn state(&self) -> PlaylistState {
        let total_duration: Option<f64> = {
            let sum: f64 = self
                .tracks
                .iter()
                .filter_map(|t| t.duration.map(|d| d.as_secs_f64()))
                .sum();
            if sum > 0.0 { Some(sum) } else { None }
        };

        let entries = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, track)| PlaylistEntry {
                id: track.id,
                display_name: track.display_name(),
                duration: track.duration_display(),
                is_current: self.current_index == Some(i),
                is_selected: self.selected.contains(&track.id),
                is_stream: track.is_stream,
            })
            .collect();

        PlaylistState {
            tracks: entries,
            current_index: self.current_index,
            current_track_id: self.current_track().map(|t| t.id),
            shuffle: self.sequence.shuffle,
            repeat: self.sequence.repeat,
            total_duration,
            track_count: self.tracks.len(),
        }
    }

    // -- Accessors --

    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    pub fn get_track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn get_track_by_index(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }
}
