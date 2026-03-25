//! M3U and PLS playlist file parser.
//!
//! Parses `.m3u`, `.m3u8`, and `.pls` files to extract stream URLs.
//! Used when a user opens a playlist file (e.g. from a radio station website)
//! or when a stream URL resolves to a playlist format.

/// A single entry extracted from a playlist file.
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    pub url: String,
    pub title: Option<String>,
}

/// Check if a path/URL looks like a playlist file.
pub fn is_playlist_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".m3u")
        || lower.ends_with(".m3u8")
        || lower.ends_with(".pls")
}

/// Auto-detect format and parse a playlist file's contents.
pub fn parse_playlist(content: &str) -> Vec<PlaylistEntry> {
    let trimmed = content.trim();
    if trimmed.starts_with("[playlist]") || trimmed.starts_with("[Playlist]") {
        parse_pls(trimmed)
    } else {
        parse_m3u(trimmed)
    }
}

/// Parse M3U/M3U8 content.
///
/// M3U format:
/// ```text
/// #EXTM3U
/// #EXTINF:-1,Station Name
/// http://stream.example.com:8000/live
/// ```
fn parse_m3u(content: &str) -> Vec<PlaylistEntry> {
    let mut entries = Vec::new();
    let mut pending_title: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            // Format: #EXTINF:duration,Title
            if let Some((_dur, title)) = rest.split_once(',') {
                let title = title.trim();
                if !title.is_empty() {
                    pending_title = Some(title.to_string());
                }
            }
        } else if line.starts_with('#') {
            // Other comment/directive — skip.
            continue;
        } else {
            // This is a URL or file path.
            entries.push(PlaylistEntry {
                url: line.to_string(),
                title: pending_title.take(),
            });
        }
    }

    entries
}

/// Parse PLS content.
///
/// PLS format:
/// ```text
/// [playlist]
/// numberofentries=2
/// File1=http://stream.example.com:8000/live
/// Title1=Station Name
/// Length1=-1
/// File2=http://another.stream/radio
/// Title2=Another Station
/// Length2=-1
/// ```
fn parse_pls(content: &str) -> Vec<PlaylistEntry> {
    let mut files: Vec<(usize, String)> = Vec::new();
    let mut titles: Vec<(usize, String)> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        let lower = line.to_lowercase();

        if let Some(rest) = lower.strip_prefix("file") {
            if let Some((num_str, _value)) = rest.split_once('=') {
                if let Ok(num) = num_str.parse::<usize>() {
                    // Use the original line to preserve URL casing.
                    let value = &line[line.find('=').unwrap() + 1..];
                    files.push((num, value.to_string()));
                }
            }
        } else if let Some(rest) = lower.strip_prefix("title") {
            if let Some((num_str, _)) = rest.split_once('=') {
                if let Ok(num) = num_str.parse::<usize>() {
                    let value = &line[line.find('=').unwrap() + 1..];
                    titles.push((num, value.to_string()));
                }
            }
        }
    }

    // Sort by entry number.
    files.sort_by_key(|(n, _)| *n);

    files
        .into_iter()
        .map(|(num, url)| {
            let title = titles
                .iter()
                .find(|(n, _)| *n == num)
                .map(|(_, t)| t.clone())
                .filter(|t| !t.is_empty());
            PlaylistEntry { url, title }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_m3u_basic() {
        let content = "\
#EXTM3U
#EXTINF:-1,Radio Paradise
http://stream.radioparadise.com/aac-320
#EXTINF:-1,SomaFM Groove Salad
http://ice1.somafm.com/groovesalad-256-mp3
";
        let entries = parse_playlist(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title.as_deref(), Some("Radio Paradise"));
        assert_eq!(entries[0].url, "http://stream.radioparadise.com/aac-320");
        assert_eq!(entries[1].title.as_deref(), Some("SomaFM Groove Salad"));
    }

    #[test]
    fn parse_pls_basic() {
        let content = "\
[playlist]
numberofentries=2
File1=http://stream1.example.com/radio
Title1=Station One
Length1=-1
File2=http://stream2.example.com/radio
Title2=Station Two
Length2=-1
";
        let entries = parse_playlist(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "http://stream1.example.com/radio");
        assert_eq!(entries[0].title.as_deref(), Some("Station One"));
        assert_eq!(entries[1].url, "http://stream2.example.com/radio");
        assert_eq!(entries[1].title.as_deref(), Some("Station Two"));
    }

    #[test]
    fn parse_m3u_no_extinf() {
        let content = "http://stream.example.com/radio\n";
        let entries = parse_playlist(content);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].url, "http://stream.example.com/radio");
        assert!(entries[0].title.is_none());
    }
}
