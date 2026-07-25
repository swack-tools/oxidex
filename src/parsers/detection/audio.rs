//! Audio format detection
//!
//! Handles detection of audio formats including MP3, AAC, and OGG/Opus.

use crate::core::FileFormat;

use super::helpers::matches_at_offset;

/// Detect MP3 format via MPEG sync pattern
///
/// MP3 files without ID3 tags start with MPEG frame sync bytes.
/// Valid sync: 0xFF followed by 0xEx where x is not E or F (UTF-16 BOM)
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 2 bytes)
///
/// # Returns
///
/// `true` if MPEG sync pattern detected
pub fn is_mp3_sync(data: &[u8]) -> bool {
    data.len() >= 2
        && data[0] == 0xFF
        && (data[1] & 0xE0) == 0xE0
        && data[1] != 0xFE
        && data[1] != 0xFF
}

/// Detect AAC format via ADTS sync word
///
/// AAC files use ADTS framing with sync word 0xFFF in first 12 bits.
/// Common patterns: 0xFF 0xF1 or 0xFF 0xF9
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 2 bytes)
///
/// # Returns
///
/// `true` if ADTS sync pattern detected
pub fn is_aac_adts(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0xFF && (data[1] == 0xF1 || data[1] == 0xF9)
}

/// Detect Opus audio within OGG container
///
/// Opus uses OGG container with "OpusHead" signature at offset 28.
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 36 bytes)
///
/// # Returns
///
/// `Some(FileFormat::OPUS)` if Opus detected, `Some(FileFormat::OGG)` for generic OGG
pub fn detect_ogg_variant(data: &[u8]) -> Option<FileFormat> {
    if data.len() >= 36 && matches_at_offset(data, b"OpusHead", 28) {
        Some(FileFormat::OPUS)
    } else {
        Some(FileFormat::OGG)
    }
}
