//! ADTS (Audio Data Transport Stream) sniffer for raw AAC internet radio.
//!
//! We sniff for ADTS sync words to decide whether to route a stream
//! through libfdk-aac (which handles ADTS framing internally) or through
//! Symphonia's probe (for MP3, OGG, FLAC, etc.). ADTS sync (0xFFF, layer=00)
//! overlaps with MP3 sync (0xFFE), so without this check Symphonia's MP3
//! demuxer falsely accepts AAC streams.

/// ADTS sample rate table (frequency index → Hz).
const SAMPLE_RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000,
    11025, 8000, 7350,
];

/// Parsed ADTS frame header. Most fields are kept for diagnostics; only
/// `sample_rate` and `channels` are read by current callers.
#[derive(Debug, Clone)]
pub struct AdtsHeader {
    /// AAC profile (0=Main, 1=LC, 2=SSR, 3=LTP).
    pub profile: u8,
    /// Sample rate frequency index.
    pub freq_index: u8,
    /// Actual sample rate in Hz.
    pub sample_rate: u32,
    /// Channel configuration (1–7).
    pub channels: u8,
    /// Total frame length in bytes (including header).
    pub frame_length: usize,
    /// Header size: 7 bytes (no CRC) or 9 bytes (with CRC).
    pub header_size: usize,
}

/// Check if two bytes form an ADTS sync word (0xFFF with Layer=00).
///
/// ADTS: byte0=0xFF, byte1 bits 7-4 = 1111, bits 2-1 (layer) = 00.
/// MP3:  byte0=0xFF, byte1 bits 7-5 = 111,  bits 2-1 (layer) != 00.
#[inline]
pub fn is_adts_sync(b0: u8, b1: u8) -> bool {
    b0 == 0xFF && (b1 & 0xF6) == 0xF0
}

/// Detect ADTS from a byte slice (e.g. the pre-buffered radio data).
///
/// Scans for two consecutive valid ADTS frames (to avoid false positives)
/// and returns the parsed header of the first.
pub fn detect_adts(data: &[u8]) -> Option<AdtsHeader> {
    let limit = data.len().min(4096);
    for i in 0..limit.saturating_sub(7) {
        if is_adts_sync(data[i], data[i + 1]) {
            if let Some(header) = parse_header(&data[i..]) {
                let next = i + header.frame_length;
                if next + 2 <= data.len()
                    && is_adts_sync(data[next], data[next + 1])
                {
                    return Some(header);
                }
            }
        }
    }
    None
}

/// Parse an ADTS header from the given bytes (need at least 7).
pub fn parse_header(bytes: &[u8]) -> Option<AdtsHeader> {
    if bytes.len() < 7 || !is_adts_sync(bytes[0], bytes[1]) {
        return None;
    }

    let protection_absent = bytes[1] & 0x01;
    let header_size = if protection_absent == 1 { 7 } else { 9 };

    let profile = (bytes[2] >> 6) & 0x03;
    let freq_index = (bytes[2] >> 2) & 0x0F;
    let channels = ((bytes[2] & 0x01) << 2) | ((bytes[3] >> 6) & 0x03);

    let frame_length = (((bytes[3] & 0x03) as usize) << 11)
        | ((bytes[4] as usize) << 3)
        | ((bytes[5] >> 5) as usize);

    let sample_rate = SAMPLE_RATES.get(freq_index as usize).copied()?;

    if frame_length < header_size || frame_length > 8192 || channels == 0 {
        return None;
    }

    Some(AdtsHeader {
        profile,
        freq_index,
        sample_rate,
        channels,
        frame_length,
        header_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adts_sync_detection() {
        // ADTS MPEG-4, no CRC
        assert!(is_adts_sync(0xFF, 0xF1));
        // ADTS MPEG-2, no CRC
        assert!(is_adts_sync(0xFF, 0xF9));
        // MP3 MPEG1 Layer III — NOT ADTS
        assert!(!is_adts_sync(0xFF, 0xFB));
        // MP3 MPEG2 Layer III — NOT ADTS
        assert!(!is_adts_sync(0xFF, 0xF3));
        // Random byte — NOT ADTS
        assert!(!is_adts_sync(0xFF, 0x00));
    }

    #[test]
    fn parse_header_basic() {
        // Construct a minimal valid ADTS header:
        // sync=0xFFF, ID=0(MPEG4), layer=00, protection_absent=1
        // profile=1(LC), freq_index=4(44100), private=0, channels=2
        // frame_length=100, buffer_fullness=0x7FF, num_frames=0
        let mut header = [0u8; 7];
        header[0] = 0xFF;
        header[1] = 0xF1;
        header[2] = (1 << 6) | (4 << 2);
        header[3] = (0 << 7) | (2 << 6) | ((100 >> 11) as u8 & 0x03);
        header[4] = ((100 >> 3) & 0xFF) as u8;
        header[5] = ((100 & 0x07) << 5) as u8 | 0x1F;
        header[6] = 0xFC;

        let parsed = parse_header(&header).unwrap();
        assert_eq!(parsed.profile, 1);
        assert_eq!(parsed.sample_rate, 44100);
        assert_eq!(parsed.channels, 2);
        assert_eq!(parsed.frame_length, 100);
        assert_eq!(parsed.header_size, 7);
    }
}
