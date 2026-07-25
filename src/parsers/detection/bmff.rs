//! BMFF (ISO Base Media File Format) variant detection
//!
//! Handles detection of BMFF-based formats including QuickTime/MP4,
//! Canon CR3, AVIF, and HEIF/HEIC.

use crate::core::FileFormat;
use crate::parsers::raw;

use super::helpers::matches_at_offset;

/// Detect ISO Base Media File Format (BMFF) variants
///
/// Many modern formats use the BMFF container with "ftyp" at offset 4.
/// This function checks the brand identifier to distinguish between:
/// - Canon CR3 (ftypcrx)
/// - AVIF (ftypavif)
/// - HEIF/HEIC (ftyp + HEIF brands)
/// - Generic QuickTime/MP4
///
/// Classic QuickTime files may start with "moov", "wide", "free", "skip", or "mdat"
/// atoms at offset 4 instead of "ftyp".
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 12 bytes recommended)
///
/// # Returns
///
/// `Some(FileFormat)` if BMFF variant detected, `None` otherwise
pub fn detect_bmff_variants(data: &[u8]) -> Option<FileFormat> {
    if data.len() < 8 {
        return None;
    }

    // Check for ftyp (modern MP4/QuickTime)
    if matches_at_offset(data, b"ftyp", 4) {
        return detect_ftyp_brand(data);
    }

    // Check for classic QuickTime atoms
    // Classic QuickTime files may start with moov, wide, free, skip, or mdat
    let atom_type = &data[4..8];
    match atom_type {
        b"moov" | b"wide" | b"free" | b"skip" | b"mdat" | b"pnot" => {
            return Some(FileFormat::QuickTime);
        }
        _ => {}
    }

    None
}

/// Detect format from ftyp brand identifier
fn detect_ftyp_brand(data: &[u8]) -> Option<FileFormat> {
    // Check brand identifier at offset 8
    if data.len() < 12 {
        // Has ftyp but not enough bytes for brand
        return Some(FileFormat::QuickTime);
    }

    let brand = &data[8..12];

    // Canon CR3
    if brand == b"crx " {
        return Some(FileFormat::CameraRaw(raw::RawFormat::CanonCR3));
    }

    // AVIF
    if brand == b"avif" {
        return Some(FileFormat::AVIF);
    }

    // HEIF brands
    let heif_brands = [
        b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1",
    ];

    if heif_brands.iter().any(|b| brand == *b) {
        return Some(FileFormat::HEIF);
    }

    // Default to QuickTime/MP4
    Some(FileFormat::QuickTime)
}
