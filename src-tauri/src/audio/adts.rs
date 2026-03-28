//! ADTS (Audio Data Transport Stream) frame parser for raw AAC internet radio.
//!
//! Many internet radio stations stream AAC/AAC+ as raw ADTS frames over HTTP.
//! Symphonia lacks a native ADTS format reader, so we parse ADTS headers
//! ourselves and feed raw AAC payloads to Symphonia's AAC decoder.

use std::io::{self, Read};

/// ADTS sample rate table (frequency index → Hz).
const SAMPLE_RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000,
    11025, 8000, 7350,
];

/// Parsed ADTS frame header.
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
                // Verify: the next frame should also start with ADTS sync.
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

/// Build an ISO 14496-3 AudioSpecificConfig from ADTS header parameters.
///
/// For LC-AAC, the ASC is 2 bytes:
///   5 bits: audioObjectType  (profile + 1; LC = 2)
///   4 bits: samplingFrequencyIndex
///   4 bits: channelConfiguration
///   3 bits: padding zeros
pub fn build_audio_specific_config(header: &AdtsHeader) -> Box<[u8]> {
    let obj_type = (header.profile + 1) as u16;
    let freq_idx = header.freq_index as u16;
    let chan_cfg = header.channels as u16;
    let val: u16 = (obj_type << 11) | (freq_idx << 7) | (chan_cfg << 3);
    Box::new([(val >> 8) as u8, (val & 0xFF) as u8])
}

/// Reads ADTS frames from a byte stream and returns raw AAC payloads.
pub struct AdtsFrameReader<R: Read> {
    reader: R,
    /// Reusable read buffer.
    buf: Vec<u8>,
}

impl<R: Read> AdtsFrameReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buf: vec![0u8; 8192],
        }
    }

    /// Read the next ADTS frame's raw AAC payload (header stripped).
    ///
    /// Returns `Ok(None)` on clean EOF, `Err` on I/O or sync errors.
    pub fn next_frame(&mut self) -> io::Result<Option<Vec<u8>>> {
        // Read the 7-byte fixed header.
        if !self.fill_exact(7)? {
            return Ok(None); // EOF
        }

        if !is_adts_sync(self.buf[0], self.buf[1]) {
            return self.resync();
        }

        let header = match parse_header(&self.buf[..7]) {
            Some(h) => h,
            None => return self.resync(),
        };

        // Read CRC bytes if present.
        if header.header_size == 9 {
            self.reader.read_exact(&mut self.buf[7..9])?;
        }

        let payload_len = header.frame_length - header.header_size;
        if payload_len == 0 {
            return self.next_frame();
        }

        // Read the AAC payload.
        if self.buf.len() < payload_len {
            self.buf.resize(payload_len, 0);
        }
        self.reader.read_exact(&mut self.buf[..payload_len])?;

        Ok(Some(self.buf[..payload_len].to_vec()))
    }

    /// Try to read exactly `n` bytes into `self.buf`. Returns `false` on clean
    /// EOF (first byte read returned 0), propagates other errors.
    fn fill_exact(&mut self, n: usize) -> io::Result<bool> {
        if self.buf.len() < n {
            self.buf.resize(n, 0);
        }
        // Read first byte separately to distinguish EOF from mid-read error.
        match self.reader.read(&mut self.buf[..1]) {
            Ok(0) => return Ok(false),
            Ok(_) => {}
            Err(e) => return Err(e),
        }
        if n > 1 {
            self.reader.read_exact(&mut self.buf[1..n])?;
        }
        Ok(true)
    }

    /// Scan forward for the next ADTS sync word and return that frame.
    fn resync(&mut self) -> io::Result<Option<Vec<u8>>> {
        log::debug!("[adts] lost sync, scanning...");
        let mut prev = 0u8;
        let mut one = [0u8; 1];

        for _ in 0..65536 {
            match self.reader.read(&mut one) {
                Ok(0) => return Ok(None), // EOF during resync
                Ok(_) => {}
                Err(e) => return Err(e),
            }

            if prev == 0xFF && is_adts_sync(0xFF, one[0]) {
                self.buf[0] = 0xFF;
                self.buf[1] = one[0];
                self.reader.read_exact(&mut self.buf[2..7])?;

                if let Some(header) = parse_header(&self.buf[..7]) {
                    if header.header_size == 9 {
                        self.reader.read_exact(&mut self.buf[7..9])?;
                    }
                    let payload_len = header.frame_length - header.header_size;
                    if payload_len > 0 {
                        if self.buf.len() < payload_len {
                            self.buf.resize(payload_len, 0);
                        }
                        self.reader.read_exact(&mut self.buf[..payload_len])?;
                        return Ok(Some(self.buf[..payload_len].to_vec()));
                    }
                }
            }
            prev = one[0];
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "could not find ADTS sync after 64 KB",
        ))
    }
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
        header[1] = 0xF1; // sync + ID=0 + layer=00 + protection=1
        header[2] = (1 << 6) | (4 << 2); // profile=1(LC), freq=4(44100)
        header[3] = (0 << 7) | (2 << 6) | ((100 >> 11) as u8 & 0x03); // private=0, chan=2, frame_len high
        header[4] = ((100 >> 3) & 0xFF) as u8;
        header[5] = ((100 & 0x07) << 5) as u8 | 0x1F; // frame_len low + buffer_fullness high
        header[6] = 0xFC; // buffer_fullness low + num_frames=0

        let parsed = parse_header(&header).unwrap();
        assert_eq!(parsed.profile, 1); // LC
        assert_eq!(parsed.sample_rate, 44100);
        assert_eq!(parsed.channels, 2);
        assert_eq!(parsed.frame_length, 100);
        assert_eq!(parsed.header_size, 7); // protection_absent=1
    }

    #[test]
    fn audio_specific_config() {
        let header = AdtsHeader {
            profile: 1, // LC
            freq_index: 4, // 44100
            sample_rate: 44100,
            channels: 2,
            frame_length: 0,
            header_size: 7,
        };
        let asc = build_audio_specific_config(&header);
        // audioObjectType=2 (LC, profile+1), freq_index=4, channels=2
        // 00010 0100 0010 000 = 0x1210
        assert_eq!(&*asc, &[0x12, 0x10]);
    }
}
