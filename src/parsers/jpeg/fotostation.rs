//! FotoWare FotoStation trailer parser
//! (`Image::ExifTool::FotoStation::Main`)
//!
//! FotoStation appends its records AFTER the JPEG's EOI marker. Each record
//! ends with a 10-byte footer -- `tag` (int16u), `size` (int32u, covering the
//! record AND the footer) and the constant signature `0xa1b2c3d4` -- and the
//! records chain backwards: the previous record's footer ends where this
//! record begins.
//!
//! ExifTool reaches the chain by peeling the trailers that follow it off the
//! end of the file one at a time and passing the accumulated offset to
//! `ProcessFotoStation`. oxidex has no trailer chain yet, so this module
//! instead scans backwards from the end of the file for the outermost valid
//! footer and then walks the chain from there. The signature, the record
//! size and the tag id are all validated, so unrelated trailing data is not
//! mistaken for a record.
//!
//! Only the SoftEdit record (tag 0x02) produces tags here: tag 0x01 is IPTC,
//! which the IPTC parser owns, and tags 0x03/0x04 are preview images.

use crate::core::{MetadataMap, TagValue};
use crate::parsers::jpeg::app_segments::perl_number;

/// Constant that ends every FotoStation record footer.
const FOTOSTATION_SIGNATURE: [u8; 4] = [0xa1, 0xb2, 0xc3, 0xd4];

/// Bytes in a record footer: int16u tag, int32u size, int32u signature.
const FOOTER_LENGTH: usize = 10;

/// `FotoStation::Main` tag id for the SoftEdit (soft crop) record.
const TAG_SOFT_EDIT: u16 = 0x02;

/// Highest tag id defined in `%FotoStation::Main` (0x01 IPTC, 0x02 SoftEdit,
/// 0x03 ThumbnailImage, 0x04 PreviewImage).
const MAX_TAG: u16 = 0x04;

/// Extracts FotoStation trailer tags from a whole JPEG file.
///
/// # Arguments
///
/// * `file` - The complete file contents
///
/// # Returns
///
/// A metadata map keyed `FotoStation:<Name>`; empty when the file carries no
/// FotoStation trailer.
pub fn parse_fotostation_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(mut end) = find_outermost_footer_end(file) else {
        return metadata;
    };

    // Walk the chain backwards; each record's start is the next footer's end.
    while let Some((tag, start)) = read_footer(file, end) {
        if tag == TAG_SOFT_EDIT {
            parse_soft_edit(&file[start..end - FOOTER_LENGTH], &mut metadata);
        }
        end = start;
    }

    metadata
}

/// Finds the end offset of the last (outermost) valid record footer.
///
/// Scanning backwards mirrors ExifTool, which always works inwards from the
/// end of the file, so the record found first is the last one written.
fn find_outermost_footer_end(file: &[u8]) -> Option<usize> {
    if file.len() < FOOTER_LENGTH {
        return None;
    }
    // A footer ending at `end` puts its signature at end-4..end.
    (FOOTER_LENGTH..=file.len()).rev().find(|&end| {
        file[end - 4..end] == FOTOSTATION_SIGNATURE && read_footer(file, end).is_some()
    })
}

/// Validates the footer ending at `end`, returning its tag id and the start
/// offset of the record it terminates.
fn read_footer(file: &[u8], end: usize) -> Option<(u16, usize)> {
    let footer = file.get(end.checked_sub(FOOTER_LENGTH)?..end)?;
    if footer[6..10] != FOTOSTATION_SIGNATURE {
        return None;
    }
    let tag = u16::from_be_bytes([footer[0], footer[1]]);
    let size = u32::from_be_bytes([footer[2], footer[3], footer[4], footer[5]]) as usize;
    // ExifTool requires `$size >= 10` and a successful seek to -$size.
    if size < FOOTER_LENGTH || size > end || tag == 0 || tag > MAX_TAG {
        return None;
    }
    Some((tag, end - size))
}

/// `%FotoStation::SoftEdit`: `FORMAT => 'int32s'`, so the numeric keys are
/// INDICES of four bytes, not byte offsets, and `ProcessFotoStation` reads
/// them big-endian (`SetByteOrder('MM')`).
fn parse_soft_edit(record: &[u8], metadata: &mut MetadataMap) {
    let at = |index: usize| -> Option<i32> {
        record
            .get(index * 4..index * 4 + 4)
            .map(|b| i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let mut put = |name: &str, value: TagValue| {
        metadata.insert(format!("FotoStation:{}", name), value);
    };

    if let Some(v) = at(0) {
        put("OriginalImageWidth", TagValue::Integer(v as i64));
    }
    if let Some(v) = at(1) {
        put("OriginalImageHeight", TagValue::Integer(v as i64));
    }
    if let Some(v) = at(2) {
        put("ColorPlanes", TagValue::Integer(v as i64));
    }
    // ValueConv => '$val / 1000'
    if let Some(v) = at(3) {
        put(
            "XYResolution",
            TagValue::String(perl_number(v as f64 / 1000.0)),
        );
    }
    // ValueConv => '$val ? 360 - $val / 100 : 0'
    // (stored as degrees CCW * 100, reported as degrees CW)
    if let Some(v) = at(4) {
        let degrees = if v == 0 {
            0.0
        } else {
            360.0 - v as f64 / 100.0
        };
        put("Rotation", TagValue::String(perl_number(degrees)));
    }
    // Index 5 is the 0x11222211 validity check, which has no tag.
    // Indices 6-9: ValueConv '$val / 1000', PrintConv '"$val%"'
    for (index, name) in [
        (6, "CropLeft"),
        (7, "CropTop"),
        (8, "CropRight"),
        (9, "CropBottom"),
    ] {
        if let Some(v) = at(index) {
            put(
                name,
                TagValue::String(format!("{}%", perl_number(v as f64 / 1000.0))),
            );
        }
    }
    // ValueConv => '-$val / 100'
    if let Some(v) = at(11) {
        put(
            "CropRotation",
            TagValue::String(perl_number(-(v as f64) / 100.0)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wraps `body` in a FotoStation record footer.
    fn record(tag: u16, body: &[u8]) -> Vec<u8> {
        let mut out = body.to_vec();
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&((body.len() + FOOTER_LENGTH) as u32).to_be_bytes());
        out.extend_from_slice(&FOTOSTATION_SIGNATURE);
        out
    }

    fn i32s(values: &[i32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// The SoftEdit body of combined-samples/ExifTool.jpg, byte for byte.
    fn exiftool_jpg_soft_edit() -> Vec<u8> {
        i32s(&[
            16,         // OriginalImageWidth
            16,         // OriginalImageHeight
            3,          // ColorPlanes
            9000,       // XYResolution
            0,          // Rotation
            0x11222211, // validity check
            24557,      // CropLeft
            21250,      // CropTop
            30676,      // CropRight
            86250,      // CropBottom
            0,          // (unused)
            0,          // CropRotation
        ])
    }

    /// Every assertion comes from `exiftool -G0 -s combined-samples/
    /// ExifTool.jpg` (ExifTool 13.55).
    #[test]
    fn test_exiftool_jpg_trailer_matches_exiftool() {
        // The real file chains an IPTC record (tag 1) before the SoftEdit
        // record (tag 2), and other trailers follow both.
        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        file.extend_from_slice(&record(0x01, b"\x1c\x02\x00\x00\x02\x00"));
        file.extend_from_slice(&record(0x02, &exiftool_jpg_soft_edit()));

        let m = parse_fotostation_trailer(&file);
        assert_eq!(m.get_integer("FotoStation:OriginalImageWidth"), Some(16));
        assert_eq!(m.get_integer("FotoStation:OriginalImageHeight"), Some(16));
        assert_eq!(m.get_integer("FotoStation:ColorPlanes"), Some(3));
        assert_eq!(m.get_string("FotoStation:XYResolution"), Some("9"));
        assert_eq!(m.get_string("FotoStation:Rotation"), Some("0"));
        assert_eq!(m.get_string("FotoStation:CropLeft"), Some("24.557%"));
        assert_eq!(m.get_string("FotoStation:CropTop"), Some("21.25%"));
        assert_eq!(m.get_string("FotoStation:CropRight"), Some("30.676%"));
        assert_eq!(m.get_string("FotoStation:CropBottom"), Some("86.25%"));
        assert_eq!(m.get_string("FotoStation:CropRotation"), Some("0"));
        // The IPTC record (tag 1) belongs to the IPTC parser, and the
        // validity-check word at index 5 is not a tag.
        assert_eq!(m.len(), 10);
    }

    #[test]
    fn test_rotation_and_crop_rotation_conversions() {
        let mut body = exiftool_jpg_soft_edit();
        // 9000 = 90 degrees CCW, reported as 270 degrees CW
        body[16..20].copy_from_slice(&9000i32.to_be_bytes());
        // CropRotation is negated and scaled: 4500 -> -45
        body[44..48].copy_from_slice(&4500i32.to_be_bytes());
        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        file.extend_from_slice(&record(0x02, &body));

        let m = parse_fotostation_trailer(&file);
        assert_eq!(m.get_string("FotoStation:Rotation"), Some("270"));
        assert_eq!(m.get_string("FotoStation:CropRotation"), Some("-45"));
    }

    #[test]
    fn test_file_without_trailer_yields_nothing() {
        assert!(parse_fotostation_trailer(b"\xff\xd8\xff\xd9 no trailer here").is_empty());
        assert!(parse_fotostation_trailer(b"").is_empty());
    }

    #[test]
    fn test_signature_without_a_valid_footer_is_ignored() {
        // The magic alone is not enough: the tag must be one FotoStation
        // defines and the size must fit inside the file.
        let mut file = vec![0u8; 40];
        file[30..34].copy_from_slice(&0x99u16.to_be_bytes()[..].repeat(2)); // bogus tag
        file[36..40].copy_from_slice(&FOTOSTATION_SIGNATURE);
        assert!(parse_fotostation_trailer(&file).is_empty());

        // A size larger than everything before the footer is rejected too.
        let mut file = vec![0u8; 40];
        file[30..32].copy_from_slice(&TAG_SOFT_EDIT.to_be_bytes());
        file[32..36].copy_from_slice(&9999u32.to_be_bytes());
        file[36..40].copy_from_slice(&FOTOSTATION_SIGNATURE);
        assert!(parse_fotostation_trailer(&file).is_empty());
    }

    #[test]
    fn test_truncated_soft_edit_record_drops_later_tags() {
        let body = i32s(&[16, 16, 3]);
        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        file.extend_from_slice(&record(0x02, &body));

        let m = parse_fotostation_trailer(&file);
        assert_eq!(m.get_integer("FotoStation:ColorPlanes"), Some(3));
        assert!(m.get("FotoStation:XYResolution").is_none());
        assert!(m.get("FotoStation:CropLeft").is_none());
    }
}
