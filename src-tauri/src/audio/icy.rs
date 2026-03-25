//! ICY (Shoutcast/Icecast) inline metadata parser.
//!
//! Shoutcast-compatible servers embed metadata within the audio stream at a
//! fixed byte interval (`icy-metaint`). This module provides `IcyReader`, a
//! `Read` wrapper that transparently strips metadata blocks from the stream
//! and exposes the parsed metadata through a shared `Arc<Mutex<IcyMetadata>>`.

use std::io::{self, Read};
use std::sync::{Arc, Mutex};

/// Parsed ICY metadata — updated by the reader thread, read by the audio thread.
#[derive(Debug, Clone, Default)]
pub struct IcyMetadata {
    /// The current stream title (typically "Artist - Title").
    pub stream_title: Option<String>,
    /// The stream URL, if provided.
    pub stream_url: Option<String>,
}

/// Wraps a `Read` and transparently strips inline ICY metadata blocks.
///
/// ICY metadata is inserted every `metaint` audio bytes. The format is:
///   1. One byte: `length`. The actual metadata block is `length * 16` bytes.
///   2. If `length == 0`, there's no metadata this interval.
///   3. The metadata block is a null-padded string like:
///      `StreamTitle='Artist - Title';StreamUrl='http://...';`
///
/// The caller only ever sees audio bytes — metadata is consumed internally.
pub struct IcyReader<R: Read> {
    inner: R,
    metaint: usize,
    bytes_until_meta: usize,
    metadata: Arc<Mutex<IcyMetadata>>,
}

impl<R: Read> IcyReader<R> {
    /// Create a new ICY reader.
    ///
    /// `metaint` is from the `icy-metaint` HTTP response header.
    /// `metadata` is the shared state that will be updated when metadata changes.
    pub fn new(inner: R, metaint: usize, metadata: Arc<Mutex<IcyMetadata>>) -> Self {
        Self {
            inner,
            metaint,
            bytes_until_meta: metaint,
            metadata,
        }
    }
}

impl<R: Read> Read for IcyReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.bytes_until_meta == 0 {
            // Time to read the metadata block.
            self.read_metadata_block()?;
            self.bytes_until_meta = self.metaint;
        }

        // Only read up to the next metadata boundary.
        let max_read = buf.len().min(self.bytes_until_meta);
        let n = self.inner.read(&mut buf[..max_read])?;
        self.bytes_until_meta -= n;
        Ok(n)
    }
}

impl<R: Read> IcyReader<R> {
    /// Read and parse a single ICY metadata block from the stream.
    fn read_metadata_block(&mut self) -> io::Result<()> {
        // Read the 1-byte length field.
        let mut len_byte = [0u8; 1];
        self.inner.read_exact(&mut len_byte)?;
        let block_len = len_byte[0] as usize * 16;

        if block_len == 0 {
            return Ok(());
        }

        // Read the metadata block.
        let mut block = vec![0u8; block_len];
        self.inner.read_exact(&mut block)?;

        // Parse the metadata string (try UTF-8, fall back to Latin-1).
        let text = match std::str::from_utf8(&block) {
            Ok(s) => s.trim_end_matches('\0').to_string(),
            Err(_) => block
                .iter()
                .take_while(|&&b| b != 0)
                .map(|&b| b as char)
                .collect(),
        };

        if text.is_empty() {
            return Ok(());
        }

        // Parse key-value pairs and update shared metadata.
        let parsed = parse_icy_string(&text);
        if let Ok(mut meta) = self.metadata.lock() {
            if parsed.stream_title.is_some() {
                meta.stream_title = parsed.stream_title;
            }
            if parsed.stream_url.is_some() {
                meta.stream_url = parsed.stream_url;
            }
        }

        Ok(())
    }
}

/// Parse an ICY metadata string like `StreamTitle='Artist - Title';StreamUrl='...';`
fn parse_icy_string(s: &str) -> IcyMetadata {
    let mut meta = IcyMetadata::default();

    // Extract values between ='...' for each known key.
    for key in ["StreamTitle", "StreamUrl"] {
        let prefix = format!("{key}='");
        if let Some(start) = s.find(&prefix) {
            let value_start = start + prefix.len();
            if let Some(end) = s[value_start..].find("';") {
                let value = &s[value_start..value_start + end];
                if !value.is_empty() {
                    match key {
                        "StreamTitle" => meta.stream_title = Some(value.to_string()),
                        "StreamUrl" => meta.stream_url = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }
    }

    meta
}

/// Split a stream title into (title, artist).
///
/// ICY stream titles are typically formatted as "Artist - Title".
/// If the separator isn't found, the entire string becomes the title.
pub fn split_stream_title(raw: &str) -> (Option<String>, Option<String>) {
    if let Some(pos) = raw.find(" - ") {
        let artist = raw[..pos].trim().to_string();
        let title = raw[pos + 3..].trim().to_string();
        if artist.is_empty() {
            (Some(title), None)
        } else {
            (Some(title), Some(artist))
        }
    } else {
        (Some(raw.trim().to_string()), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stream_title() {
        let meta = parse_icy_string("StreamTitle='Led Zeppelin - Stairway to Heaven';");
        assert_eq!(
            meta.stream_title.as_deref(),
            Some("Led Zeppelin - Stairway to Heaven")
        );
    }

    #[test]
    fn parse_stream_title_and_url() {
        let meta = parse_icy_string(
            "StreamTitle='Pink Floyd - Comfortably Numb';StreamUrl='http://example.com';",
        );
        assert_eq!(
            meta.stream_title.as_deref(),
            Some("Pink Floyd - Comfortably Numb")
        );
        assert_eq!(meta.stream_url.as_deref(), Some("http://example.com"));
    }

    #[test]
    fn parse_empty_title() {
        let meta = parse_icy_string("StreamTitle='';");
        assert!(meta.stream_title.is_none());
    }

    #[test]
    fn split_artist_title() {
        let (title, artist) = split_stream_title("Pink Floyd - Comfortably Numb");
        assert_eq!(title.as_deref(), Some("Comfortably Numb"));
        assert_eq!(artist.as_deref(), Some("Pink Floyd"));
    }

    #[test]
    fn split_title_only() {
        let (title, artist) = split_stream_title("Just a title");
        assert_eq!(title.as_deref(), Some("Just a title"));
        assert!(artist.is_none());
    }

    #[test]
    fn icy_reader_strips_metadata() {
        // Simulate a stream: 4 audio bytes, then a metadata block, then 4 more.
        let metaint = 4;
        // Metadata: length=1 (16 bytes), content = "StreamTitle='X';" padded with nulls.
        let meta_content = b"StreamTitle='X';";
        let mut meta_block = vec![1u8]; // length byte = 1 → 16 bytes
        meta_block.extend_from_slice(meta_content);
        meta_block.resize(1 + 16, 0); // pad to 16 bytes

        let mut stream = Vec::new();
        stream.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4 audio bytes
        stream.extend_from_slice(&meta_block); // metadata block
        stream.extend_from_slice(&[0xEE, 0xFF, 0x11, 0x22]); // 4 more audio bytes

        let shared = Arc::new(Mutex::new(IcyMetadata::default()));
        let mut reader = IcyReader::new(&stream[..], metaint, Arc::clone(&shared));

        let mut out = vec![0u8; 8];
        let mut total = 0;
        loop {
            match reader.read(&mut out[total..]) {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(_) => break,
            }
        }

        assert_eq!(total, 8);
        assert_eq!(&out, &[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x11, 0x22]);

        let meta = shared.lock().unwrap();
        assert_eq!(meta.stream_title.as_deref(), Some("X"));
    }
}
