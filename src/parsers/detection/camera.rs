//! Camera format detection
//!
//! Handles detection of proprietary camera formats like Casio CAM.

use crate::core::{FileFormat, FileReader};

/// Detect Casio CAM proprietary format
///
/// Casio CAM files have a 70-byte proprietary header followed by JPEG data.
/// The header contains "MM" marker at offset 2.
///
/// # Arguments
///
/// * `data` - Initial magic bytes buffer
/// * `reader` - File reader for additional validation
///
/// # Returns
///
/// `Some(FileFormat::CasioCAM)` if detected, `None` otherwise
pub fn detect_casio_cam(data: &[u8], reader: &dyn FileReader) -> Option<FileFormat> {
    if reader.size() <= 73 {
        return None;
    }

    // Check for JPEG at offset 70
    if let Ok(header_check) = reader.read(70, 3)
        && header_check.starts_with(&[0xFF, 0xD8, 0xFF])
    {
        // Verify "MM" marker at offset 2
        if data.len() >= 4 && data[2] == 0x4D && data[3] == 0x4D {
            return Some(FileFormat::CasioCAM);
        }
    }

    None
}
