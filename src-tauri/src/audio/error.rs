//! Audio subsystem error types.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("failed to decode audio: {0}")]
    Decode(String),

    #[error("unsupported audio format: {0}")]
    UnsupportedFormat(String),

    #[error("failed to open file: {0}")]
    FileOpen(#[from] std::io::Error),

    #[error("audio output error: {0}")]
    Output(String),

    #[error("seek not supported by this source")]
    SeekNotSupported,

    #[error("seek position out of range")]
    SeekOutOfRange,

    #[error("no audio track found in file")]
    NoTrack,

    #[error("source is in an error state")]
    SourceError,
}
