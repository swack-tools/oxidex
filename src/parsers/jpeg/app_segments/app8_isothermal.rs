//! APP8 InfiRay isothermal parser (`Image::ExifTool::InfiRay::Isothermal`)
//!
//! JPEG APP8 normally carries a SPIFF header. InfiRay's IJPEG SDK reuses the
//! marker for a four-float isothermal record with no identifier of its own:
//! ExifTool only reads it once an APP2 segment matching `/^....IJPEG\0/` has
//! set `$$self{HasIJPEG}`, and then only when the segment is at least 32
//! bytes long (ExifTool.pm:8215).
//!
//! Values are little-endian (`SetByteOrder('II')`) and the table's numeric
//! keys are byte offsets, since `%InfiRay::Isothermal` declares no `FORMAT`.

use super::perl_number;
use crate::core::{MetadataMap, TagValue};

/// Minimum APP8 payload length before ExifTool reads an isothermal record.
pub const INFIRAY_ISOTHERMAL_MIN_LENGTH: usize = 32;

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
    let mut metadata = MetadataMap::new();
    if data.len() < INFIRAY_ISOTHERMAL_MIN_LENGTH {
        return metadata;
    }

    const FIELDS: [(usize, &str); 4] = [
        (0x00, "IsothermalMax"),
        (0x04, "IsothermalMin"),
        (0x08, "ChromaBarMax"),
        (0x0c, "ChromaBarMin"),
    ];
    for (offset, name) in FIELDS {
        let Some(bytes) = data.get(offset..offset + 4) else {
            continue;
        };
        let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        metadata.insert(
            format!("APP8:{}", name),
            TagValue::String(perl_number(value as f64)),
        );
    }

    metadata
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
