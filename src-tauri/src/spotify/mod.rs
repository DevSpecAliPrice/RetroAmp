//! Spotify integration — authentication, Web API client, and Tauri commands.
//!
//! The audio source itself lives in `audio::spotify` (same level as local/radio).
//! This module handles everything outside the audio pipeline: OAuth login,
//! credential management, Spotify Web API browsing, and the Tauri command layer.

pub mod api;
pub mod auth;
pub mod commands;
pub mod types;
