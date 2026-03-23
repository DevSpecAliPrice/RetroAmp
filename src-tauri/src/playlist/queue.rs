//! Play-next queue — a separate ordered list of tracks that take priority
//! over the normal playlist sequence.
//!
//! When the user right-clicks a track and says "Play Next", it goes here.
//! The engine checks the queue before consulting the playlist sequence.

use std::collections::VecDeque;

use crate::playlist::track::TrackId;

pub struct Queue {
    items: VecDeque<TrackId>,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            items: VecDeque::new(),
        }
    }

    /// Add a track to the end of the queue.
    pub fn add(&mut self, id: TrackId) {
        self.items.push_back(id);
    }

    /// Add a track to play immediately next (front of queue).
    pub fn add_next(&mut self, id: TrackId) {
        self.items.push_front(id);
    }

    /// Take the next track from the queue, removing it.
    pub fn take_next(&mut self) -> Option<TrackId> {
        self.items.pop_front()
    }

    /// Peek at the next track without removing it.
    pub fn peek_next(&self) -> Option<&TrackId> {
        self.items.front()
    }

    /// Remove specific tracks from the queue.
    pub fn remove_tracks(&mut self, ids: &[TrackId]) {
        self.items.retain(|id| !ids.contains(id));
    }

    /// Clear the entire queue.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}
