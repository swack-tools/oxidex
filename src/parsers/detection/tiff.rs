//! TIFF variant detection
//!
//! Handles detection of TIFF-based formats including standard TIFF,
//! Canon CR2/CRW, Panasonic RW2, and Olympus ORF.

use crate::core::FileFormat;
use crate::parsers::raw;

use super::helpers::matches_at_offset;

/// Detect TIFF-based formats
///
/// TIFF and TIFF-based raw camera formats share similar byte-order markers
/// but differ in magic numbers. This function consolidates the detection
/// logic for all TIFF variants.
///
/// # TIFF Variants
///
/// - Standard TIFF: II/MM + 0x002A (42)
/// - Panasonic RW2: II + 0x0055 (85)
/// - Olympus ORF: II + "RO" or "RS", MM + "OR"
/// - Canon CR2: II + 0x002A + "CR\x02\x00" at offset 8
/// - Canon CRW: II + 0x001A + "HEAPCCDR" at offset 6
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 16 bytes recommended)
///
/// # Returns
///
/// `Some(FileFormat)` if TIFF variant detected, `None` otherwise
pub fn detect_tiff_variants(data: &[u8]) -> Option<FileFormat> {
    if data.len() < 4 {
        return None;
    }

    // Canon CR2: Little-endian TIFF with "CR\x02\x00" at offset 8
    if data.len() >= 12
        && data.starts_with(&[0x49, 0x49, 0x2A, 0x00])
        && matches_at_offset(data, b"CR\x02\x00", 8)
    {
        return Some(FileFormat::CameraRaw(raw::RawFormat::CanonCR2));
    }

    // Canon CRW: CIFF format with "II\x1a\x00" + "HEAPCCDR"
    if data.len() >= 14
        && matches_at_offset(data, &[0x49, 0x49, 0x1A, 0x00], 0)
        && matches_at_offset(data, b"HEAPCCDR", 6)
    {
        return Some(FileFormat::CameraRaw(raw::RawFormat::CanonCRW));
    }

    // All TIFF variants (group by byte order for efficiency)
    let tiff_signatures = [
        ([0x49, 0x49, 0x2A, 0x00], "standard LE"),
        ([0x49, 0x49, 0x55, 0x00], "Panasonic RW2"),
        ([0x49, 0x49, 0x52, 0x4F], "Olympus ORF (RO)"),
        ([0x49, 0x49, 0x52, 0x53], "Olympus ORF (RS)"),
        ([0x4D, 0x4D, 0x00, 0x2A], "standard BE"),
        ([0x4D, 0x4D, 0x4F, 0x52], "Olympus ORF (OR)"),
    ];

    for (sig, _desc) in &tiff_signatures {
        if data.starts_with(sig) {
            return Some(FileFormat::TIFF);
        }
    }

    None
}
