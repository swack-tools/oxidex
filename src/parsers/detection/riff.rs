//! RIFF format detection
//!
//! Handles detection of RIFF-based formats including WAV, AVI, and WebP.

use crate::core::FileFormat;

/// Detect RIFF-based formats
///
/// RIFF (Resource Interchange File Format) is used by multiple formats:
/// - WAV audio (RIFF...WAVE)
/// - AVI video (RIFF...AVI )
/// - WebP image (RIFF...WEBP)
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 12 bytes recommended)
///
/// # Returns
///
/// `Some(FileFormat)` if RIFF format detected, `None` otherwise
pub fn detect_riff_formats(data: &[u8]) -> Option<FileFormat> {
    if data.len() < 12 || !data.starts_with(b"RIFF") {
        return None;
    }

    let format_type = &data[8..12];
    match format_type {
        b"WAVE" => Some(FileFormat::WAV),
        b"AVI " => Some(FileFormat::AVI),
        b"WEBP" => Some(FileFormat::WebP),
        _ => None,
    }
}
