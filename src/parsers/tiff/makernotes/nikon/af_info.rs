//! Nikon `AFInfo` (MakerNote tag 0x0088) parser.
//!
//! Transcribed from `Image::ExifTool::Nikon::AFInfo`. The block is only four
//! bytes: two single-byte enums followed by an `int16u` bitmask of the AF points
//! that achieved focus.
//!
//! The 16-bit field's byte order is chosen by camera model, not by the
//! surrounding TIFF: ExifTool selects `BigEndian` when `Model =~ /^NIKON D/i`
//! and `LittleEndian` otherwise (see the two `0x0088` conditions in
//! `Nikon::Main`). Getting this wrong turns `Center` into `[8]`, so the model
//! is threaded in from the dispatcher rather than guessed.

use std::collections::HashMap;

use super::value_reader::{decode_bits, read_u16};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// `Image::ExifTool::Nikon::AFInfo` index 0.
fn af_area_mode(value: u8) -> String {
    match value {
        0 => "Single Area".to_string(),
        1 => "Dynamic Area".to_string(),
        2 => "Dynamic Area (closest subject)".to_string(),
        3 => "Group Dynamic".to_string(),
        4 => "Single Area (wide)".to_string(),
        5 => "Dynamic Area (wide)".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// `Image::ExifTool::Nikon::AFInfo` index 1.
fn af_point(value: u8) -> String {
    match value {
        0 => "Center".to_string(),
        1 => "Top".to_string(),
        2 => "Bottom".to_string(),
        3 => "Mid-left".to_string(),
        4 => "Mid-right".to_string(),
        5 => "Upper-left".to_string(),
        6 => "Upper-right".to_string(),
        7 => "Lower-left".to_string(),
        8 => "Lower-right".to_string(),
        9 => "Far Left".to_string(),
        10 => "Far Right".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// ExifTool `%afPoints11`, the 11-point bitmask shared by the D70-era bodies.
fn af_points_in_focus(value: u16) -> String {
    match value {
        0 => "(none)".to_string(),
        0x7ff => "All 11 Points".to_string(),
        _ => decode_bits(
            value as u32,
            16,
            &[
                (0, "Center"),
                (1, "Top"),
                (2, "Bottom"),
                (3, "Mid-left"),
                (4, "Mid-right"),
                (5, "Upper-left"),
                (6, "Upper-right"),
                (7, "Lower-left"),
                (8, "Lower-right"),
                (9, "Far Left"),
                (10, "Far Right"),
            ],
        ),
    }
}

/// Byte order ExifTool uses for `AFInfo`'s 16-bit field, given the camera model.
pub fn af_info_byte_order(model: Option<&str>) -> ByteOrder {
    let is_nikon_dslr = model.is_some_and(|m| {
        let m = m.trim();
        m.len() >= 7 && m[..7].eq_ignore_ascii_case("NIKON D")
    });
    if is_nikon_dslr {
        ByteOrder::BigEndian
    } else {
        ByteOrder::LittleEndian
    }
}

/// Parse a Nikon `AFInfo` block into `Nikon:`-prefixed tags.
pub fn parse_af_info(data: &[u8], order: ByteOrder, tags: &mut HashMap<String, String>) {
    if let Some(&raw) = data.first() {
        tags.insert("Nikon:AFAreaMode".to_string(), af_area_mode(raw));
    }
    if let Some(&raw) = data.get(1) {
        tags.insert("Nikon:AFPoint".to_string(), af_point(raw));
    }
    if let Some(raw) = read_u16(data, 2, order) {
        tags.insert("Nikon:AFPointsInFocus".to_string(), af_points_in_focus(raw));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_d70_sample_block() {
        // MakerNote 0x0088 from Nikon.nef, stored inline as 00 00 00 01.
        let mut tags = HashMap::new();
        parse_af_info(&[0x00, 0x00, 0x00, 0x01], ByteOrder::BigEndian, &mut tags);
        assert_eq!(tags.get("Nikon:AFAreaMode").unwrap(), "Single Area");
        assert_eq!(tags.get("Nikon:AFPoint").unwrap(), "Center");
        assert_eq!(tags.get("Nikon:AFPointsInFocus").unwrap(), "Center");
    }

    #[test]
    fn byte_order_follows_the_camera_model() {
        assert_eq!(af_info_byte_order(Some("NIKON D70")), ByteOrder::BigEndian);
        assert_eq!(af_info_byte_order(Some("nikon d850")), ByteOrder::BigEndian);
        assert_eq!(
            af_info_byte_order(Some("COOLPIX P7100")),
            ByteOrder::LittleEndian
        );
        assert_eq!(af_info_byte_order(Some("E5000")), ByteOrder::LittleEndian);
        assert_eq!(af_info_byte_order(None), ByteOrder::LittleEndian);
        // "NIKON Z 9" is not a D-series body.
        assert_eq!(
            af_info_byte_order(Some("NIKON Z 9")),
            ByteOrder::LittleEndian
        );
    }

    #[test]
    fn multiple_focus_points_are_joined_like_exiftool() {
        assert_eq!(af_points_in_focus(0), "(none)");
        assert_eq!(af_points_in_focus(0x7ff), "All 11 Points");
        assert_eq!(af_points_in_focus(0b101), "Center, Bottom");
    }

    #[test]
    fn unknown_codes_report_themselves() {
        // A wrong label is worse than no label: unknown enums keep their code.
        assert_eq!(af_area_mode(9), "Unknown (9)");
        assert_eq!(af_point(12), "Unknown (12)");
        // Bit 11 has no name in %afPoints11.
        assert_eq!(af_points_in_focus(1 << 11), "[11]");
    }

    #[test]
    fn short_blocks_emit_only_the_fields_present() {
        let mut tags = HashMap::new();
        parse_af_info(&[0x01], ByteOrder::BigEndian, &mut tags);
        assert_eq!(tags.get("Nikon:AFAreaMode").unwrap(), "Dynamic Area");
        assert_eq!(tags.len(), 1);
    }
}
