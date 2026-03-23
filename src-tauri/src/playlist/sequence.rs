//! Playback sequence — shuffle and repeat logic.
//!
//! Determines which track plays next based on the current mode. The sequence
//! is separate from the playlist data model so the same playlist can be
//! played in different modes without modifying the track order.

use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShuffleMode {
    Off,
    /// Shuffle all tracks. Maintains a shuffled order so prev/next are
    /// consistent, and reshuffles when the cycle completes.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    Off,
    /// Repeat the current track indefinitely.
    Track,
    /// Repeat the entire playlist when it reaches the end.
    Playlist,
}

/// Manages the playback order for a playlist.
pub struct PlaybackSequence {
    pub shuffle: ShuffleMode,
    pub repeat: RepeatMode,
    /// When shuffle is on, this holds the shuffled index order.
    shuffle_order: Vec<usize>,
    /// Position within the shuffle order.
    shuffle_position: usize,
}

impl PlaybackSequence {
    pub fn new() -> Self {
        Self {
            shuffle: ShuffleMode::Off,
            repeat: RepeatMode::Off,
            shuffle_order: Vec::new(),
            shuffle_position: 0,
        }
    }

    /// Generate a new shuffle order for the given track count, optionally
    /// placing `current_index` first so the currently playing track doesn't
    /// change when shuffle is toggled on.
    pub fn reshuffle(&mut self, track_count: usize, current_index: Option<usize>) {
        let mut rng = rand::rng();
        self.shuffle_order = (0..track_count).collect();
        self.shuffle_order.shuffle(&mut rng);

        // Move the current track to position 0 so it stays playing.
        if let Some(current) = current_index {
            if let Some(pos) = self.shuffle_order.iter().position(|&i| i == current) {
                self.shuffle_order.swap(0, pos);
            }
            self.shuffle_position = 0;
        } else {
            self.shuffle_position = 0;
        }
    }

    /// Determine the next track index to play.
    ///
    /// Returns `None` if playback should stop (e.g. end of playlist with
    /// repeat off).
    pub fn next_index(
        &mut self,
        current_index: usize,
        track_count: usize,
    ) -> Option<usize> {
        if track_count == 0 {
            return None;
        }

        if self.repeat == RepeatMode::Track {
            return Some(current_index);
        }

        match self.shuffle {
            ShuffleMode::Off => {
                let next = current_index + 1;
                if next < track_count {
                    Some(next)
                } else if self.repeat == RepeatMode::Playlist {
                    Some(0)
                } else {
                    None // End of playlist, no repeat.
                }
            }
            ShuffleMode::All => {
                self.shuffle_position += 1;
                if self.shuffle_position < self.shuffle_order.len() {
                    Some(self.shuffle_order[self.shuffle_position])
                } else if self.repeat == RepeatMode::Playlist {
                    // Reshuffle and start again.
                    self.reshuffle(track_count, None);
                    self.shuffle_order.first().copied()
                } else {
                    None
                }
            }
        }
    }

    /// Determine the previous track index.
    pub fn previous_index(
        &mut self,
        current_index: usize,
        track_count: usize,
    ) -> Option<usize> {
        if track_count == 0 {
            return None;
        }

        if self.repeat == RepeatMode::Track {
            return Some(current_index);
        }

        match self.shuffle {
            ShuffleMode::Off => {
                if current_index > 0 {
                    Some(current_index - 1)
                } else if self.repeat == RepeatMode::Playlist {
                    Some(track_count - 1)
                } else {
                    None
                }
            }
            ShuffleMode::All => {
                if self.shuffle_position > 0 {
                    self.shuffle_position -= 1;
                    Some(self.shuffle_order[self.shuffle_position])
                } else {
                    None
                }
            }
        }
    }

    /// Update the shuffle position to point at the given playlist index.
    /// Called when a track is played directly (e.g. double-clicked).
    pub fn set_current(&mut self, playlist_index: usize) {
        if self.shuffle == ShuffleMode::All {
            if let Some(pos) = self.shuffle_order.iter().position(|&i| i == playlist_index) {
                self.shuffle_position = pos;
            }
        }
    }

    /// Notify that the track count changed (tracks added/removed).
    /// Rebuilds shuffle order if needed.
    pub fn track_count_changed(&mut self, new_count: usize, current_index: Option<usize>) {
        if self.shuffle == ShuffleMode::All {
            self.reshuffle(new_count, current_index);
        }
    }
}
