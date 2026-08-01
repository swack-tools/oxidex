//! APP8 InfiRay isothermal parser (`Image::ExifTool::InfiRay::Isothermal`)
//!
//! JPEG APP8 normally carries a SPIFF header. InfiRay's IJPEG SDK reuses the
//! marker for a four-float isothermal record with no identifier of its own:
//! ExifTool only reads it once an APP2 segment matching `/^....IJPEG\0/` has
//! set `$$self{HasIJPEG}`, and then only when the segment is at least 32
//! bytes long (ExifTool.pm:8228 in 13.55, :8251 in 13.59).
//!
//! Values are little-endian (`SetByteOrder('II')`) and the table's numeric
//! keys are byte offsets, since `%InfiRay::Isothermal` declares no `FORMAT`.
//! The field list itself lives in [`super::infiray_tables`], generated from
//! ExifTool's own hash; this module is the APP8 entry point onto it.

use super::infiray::read_record;
use super::infiray_tables::{ISOTHERMAL, ISOTHERMAL_MIN_LENGTH};
use crate::core::MetadataMap;

// Spelled as a literal rather than aliased to the generated constant because
// cbindgen exports this one to `api/oxidex.h`, and the doc comment goes with
// it -- editing either churns the header. The assertion below is what keeps
// the literal equal to the generated value.
/// Minimum APP8 payload length before ExifTool reads an isothermal record.
pub const INFIRAY_ISOTHERMAL_MIN_LENGTH: usize = 32;
const _: () = assert!(INFIRAY_ISOTHERMAL_MIN_LENGTH == ISOTHERMAL_MIN_LENGTH);

/// Parses an InfiRay APP8 isothermal record.
///
/// # Arguments
///
/// * `data` - Raw APP8 segment payload (no identifier precedes the record)
///
/// # Returns
///
/// A metadata map keyed `APP8:<Name>`; empty when the payload is shorter
/// than ExifTool's 32-byte gate.
pub fn parse_infiray_isothermal(data: &[u8]) -> MetadataMap {
    if data.len() < INFIRAY_ISOTHERMAL_MIN_LENGTH {
        return MetadataMap::new();
    }
    read_record("APP8", data, ISOTHERMAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The APP8 payload of combined-samples/InfiRay.jpg. Expected values from
    /// `exiftool -G0 -s combined-samples/InfiRay.jpg` (ExifTool 13.55).
    #[test]
    fn test_infiray_jpg_isothermal_matches_exiftool() {
        let mut data = vec![
            0x00, 0x00, 0xa0, 0x42, // 80.0
            0x00, 0x00, 0xa0, 0xc1, // -20.0
            0x00, 0x00, 0xa0, 0x42, // 80.0
            0x00, 0x00, 0xa0, 0xc1, // -20.0
        ];
        data.resize(INFIRAY_ISOTHERMAL_MIN_LENGTH, 0);

        let m = parse_infiray_isothermal(&data);
        assert_eq!(m.get_string("APP8:IsothermalMax"), Some("80"));
        assert_eq!(m.get_string("APP8:IsothermalMin"), Some("-20"));
        assert_eq!(m.get_string("APP8:ChromaBarMax"), Some("80"));
        assert_eq!(m.get_string("APP8:ChromaBarMin"), Some("-20"));
        assert_eq!(m.len(), 4);
    }

    #[test]
    fn test_short_payload_yields_nothing() {
        // ExifTool's gate is `$length >= 32`; a shorter APP8 is not read.
        let data = vec![0u8; INFIRAY_ISOTHERMAL_MIN_LENGTH - 1];
        assert!(parse_infiray_isothermal(&data).is_empty());
    }
}
