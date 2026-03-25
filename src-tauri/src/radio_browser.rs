//! Radio Browser API client — discovers internet radio stations via
//! the free radio-browser.info API.
//!
//! All functions are synchronous (using ureq) and should be called from
//! a background thread or via `tauri::async_runtime::spawn_blocking`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

const API_BASE: &str = "https://de1.api.radio-browser.info";
const USER_AGENT: &str = "RetroAmp/0.1";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// A station returned by the Radio Browser API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiStation {
    pub name: String,
    pub url: String,
    pub url_resolved: String,
    pub favicon: String,
    pub country: String,
    pub countrycode: String,
    pub tags: String,
    pub codec: String,
    pub bitrate: u32,
    pub clickcount: u64,
    pub votes: i64,
    pub lastcheckok: u32,
}

/// Search stations by name.
pub fn search(query: &str, limit: usize) -> Result<Vec<ApiStation>, String> {
    let url = format!(
        "{API_BASE}/json/stations/search?name={}&limit={limit}&hidebroken=true&order=clickcount&reverse=true",
        urlencoded(query),
    );
    fetch(&url)
}

/// Get stations by tag (genre).
pub fn by_tag(tag: &str, limit: usize) -> Result<Vec<ApiStation>, String> {
    let url = format!(
        "{API_BASE}/json/stations/bytag/{}?limit={limit}&hidebroken=true&order=clickcount&reverse=true",
        urlencoded(tag),
    );
    fetch(&url)
}

/// Get the most popular stations by click count.
pub fn top_stations(limit: usize) -> Result<Vec<ApiStation>, String> {
    let url = format!(
        "{API_BASE}/json/stations/topclick?limit={limit}&hidebroken=true",
    );
    fetch(&url)
}

/// Fetch a URL and parse the JSON response as a list of stations.
fn fetch(url: &str) -> Result<Vec<ApiStation>, String> {
    let response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .config()
        .timeout_connect(Some(DEFAULT_TIMEOUT))
        .timeout_recv_response(Some(DEFAULT_TIMEOUT))
        .build()
        .call()
        .map_err(|e| format!("Radio Browser API error: {e}"))?;

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("failed to read API response: {e}"))?;

    let stations: Vec<ApiStation> =
        serde_json::from_str(&body).map_err(|e| format!("failed to parse API response: {e}"))?;

    Ok(stations)
}

/// Simple percent-encoding for query parameters.
fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}
