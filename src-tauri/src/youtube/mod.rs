//! YouTube Music integration — API client and Tauri commands.
//!
//! The audio source itself lives in `audio::youtube` (same level as local/radio).
//! This module handles YouTube Music browsing: search, albums, artists, playlists,
//! and the Tauri command layer.

pub mod api;
pub mod commands;
pub mod types;
pub mod ytdlp;
