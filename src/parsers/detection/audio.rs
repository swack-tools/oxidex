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
/// The second byte also carries the version and layer fields, and both have
/// codes the MPEG audio spec marks reserved: version `01` and layer `00`.
/// They have to be rejected here, because an ADTS (AAC) frame header is
/// *defined* to carry layer `00` -- so a bare sync test claims every AAC
/// file for MP3, and this check runs first. That sent AAC.aac to the MP3
/// parser, which produced nothing, leaving the AAC parser unreachable and
/// its four ExifTool tags permanently missing.
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
        // Layer `00` is reserved (and is what ADTS always uses).
        && (data[1] & 0x06) != 0x00
        // Version `01` is reserved.
        && (data[1] & 0x18) != 0x08
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adts_headers_are_not_mp3_sync() {
        // `detect_format` tests MP3 before AAC, so these two must be
        // mutually exclusive or every ADTS file resolves to MP3 and the
        // AAC parser is never reached.
        for second in [0xF0u8, 0xF1, 0xF9] {
            assert!(
                !is_mp3_sync(&[0xFF, second]),
                "0xFF {second:#04X} is an ADTS header (layer 00), not MPEG audio"
            );
        }
        assert!(is_aac_adts(&[0xFF, 0xF1]));
        assert!(is_aac_adts(&[0xFF, 0xF9]));
    }

    #[test]
    fn real_mpeg_layers_still_sync() {
        // MPEG1 Layer III, MPEG2 Layer III, MPEG1 Layer II, MPEG1 Layer I.
        for second in [0xFBu8, 0xF3, 0xFD, 0xFC] {
            assert!(is_mp3_sync(&[0xFF, second]), "0xFF {second:#04X}");
        }
    }

    #[test]
    fn reserved_mpeg_version_is_rejected() {
        // Version `01` is reserved: 0xFF 0xEB has sync but no valid version.
        assert!(!is_mp3_sync(&[0xFF, 0xEB]));
    }
}
