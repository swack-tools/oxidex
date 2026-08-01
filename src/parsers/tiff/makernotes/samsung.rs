//! Samsung MakerNote parser
//!
//! Parses Samsung Galaxy-specific EXIF MakerNote tags containing computational
//! photography settings, AI features, and Samsung-exclusive camera modes.
//!
//! ## Supported Features
//! - Scene Optimizer AI detection
//! - Single Take mode information
//! - Expert RAW processing data
//! - Multi-Frame Processing details
//! - Director's View settings
//! - Pro mode parameters
//! - Object tracking data
//! - Night mode settings
//!
//! ## Architecture
//! Samsung's MakerNotes use a proprietary binary format with Samsung-specific tags.
//! Many Galaxy devices include extensive AI processing metadata and multi-camera
//! coordination data.
//!
//! ## Code Organization
//! This parser uses the TagRegistry pattern to eliminate repetitive match arms.
//! All tag definitions and decoders are centralized in the registries::samsung module,
//! reducing code duplication and improving maintainability.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::registries::samsung::samsung_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_i16_value;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::table_ifd;

// Type2 tables, ported from Image::ExifTool::Samsung::Type2.
pub mod lookups;
pub mod type2;

// Samsung signature for validation
const SAMSUNG_SIGNATURE: &[u8] = b"Samsung";

/// Samsung MakerNote parser implementation
pub struct SamsungParser;

impl Default for SamsungParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SamsungParser {
    /// Creates a new Samsung parser instance
    pub fn new() -> Self {
        SamsungParser
    }

    /// Parse a single IFD entry and extract tag value using the registry
    ///
    /// This method uses the TagRegistry pattern to eliminate repetitive match arms.
    /// All tag definitions and decoders are accessed through the centralized registry,
    /// reducing code duplication and improving maintainability.
    ///
    /// # Arguments
    /// * `entry` - IFD entry to parse
    /// * `data` - Full MakerNote data buffer
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - HashMap to insert extracted tags into
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        let registry = samsung_registry();

        // Check if this tag is registered
        if !registry.has_tag(entry.tag_id) {
            // Unknown tag - skip silently for forward compatibility
            return;
        }

        // Get the tag name from registry
        let tag_name = registry.get_tag_name(entry.tag_id).unwrap();
        let full_tag_name = format!("Samsung:{}", tag_name);

        // Extract i16 value (most Samsung tags use i16)
        if let Some(value) = extract_i16_value(entry, data, byte_order) {
            // Use registry to decode the value
            let decoded = registry.decode_i16(entry.tag_id, value);
            tags.insert(full_tag_name, decoded);
        }
    }
}

impl MakerNoteParser for SamsungParser {
    fn manufacturer_name(&self) -> &'static str {
        "Samsung"
    }

    fn tag_prefix(&self) -> &'static str {
        "Samsung:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Samsung's "Type2" MakerNote (ExifTool's Image::ExifTool::Samsung::Type2)
        // is a bare TIFF IFD at offset 0 whose value offsets are relative to the
        // MakerNote itself. It carries no signature, so the older code -- which
        // required a literal "Samsung" prefix and then read every entry as an
        // i16 -- produced nothing at all for the NX bodies and Galaxy phones.
        let ifd_start = if data.len() >= 8 && &data[0..7] == SAMSUNG_SIGNATURE {
            8
        } else {
            0
        };
        table_ifd::walk_directory(
            data,
            ifd_start,
            Some(0),
            byte_order,
            "Samsung",
            type2::TYPE2,
            tags,
        );
        parse_orientation_info(data, ifd_start, byte_order, tags);
        parse_picture_wizard(data, ifd_start, byte_order, tags);
        Ok(())
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Accept data with or without Samsung signature
        if data.len() >= 7 && &data[0..7] == SAMSUNG_SIGNATURE {
            return true;
        }

        // Otherwise accept anything that looks like a bare IFD. The entry
        // count has to be read in the container's byte order, which this trait
        // method does not receive -- checking only little-endian rejected every
        // big-endian Samsung (the NX bodies and most Galaxy phones), whose
        // 41-entry directory reads as 10496 the wrong way round.
        if data.len() >= 2 {
            let count_le = EndianReader::little_endian(data).u16_at(0).unwrap_or(0);
            let count_be = EndianReader::big_endian(data).u16_at(0).unwrap_or(0);
            if (1..500).contains(&count_le) || (1..500).contains(&count_be) {
                return true;
            }
        }

        false
    }
}

/// Expand `OrientationInfo` (0x0011), a three-element `rational64s` block
/// written by the Gear 360. `YawAngle` is `Unknown => 1` in ExifTool and is
/// therefore not printed.
fn parse_orientation_info(
    data: &[u8],
    ifd_start: usize,
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let Some(entries) = table_ifd::read_ifd(data, ifd_start, byte_order) else {
        return;
    };
    let Some(entry) = entries.iter().find(|e| e.tag_id == 0x0011) else {
        return;
    };
    let Some(val) = table_ifd::decode_entry(
        data,
        entry,
        Some(0),
        byte_order,
        Some(table_ifd::ftype::SRATIONAL),
    ) else {
        return;
    };
    let table_ifd::OlyVal::Rat(r) = &val else {
        return;
    };
    for (idx, name) in [(1usize, "PitchAngle"), (2, "RollAngle")] {
        if let Some(&(n, d)) = r.get(idx) {
            tags.insert(format!("Samsung:{}", name), table_ifd::print_rational(n, d));
        }
    }
}

/// Expand `PictureWizard` (0x0021), a five-element `int16u` block. ExifTool's
/// ValueConv subtracts 4 from saturation, sharpness and contrast.
fn parse_picture_wizard(
    data: &[u8],
    ifd_start: usize,
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let Some(entries) = table_ifd::read_ifd(data, ifd_start, byte_order) else {
        return;
    };
    let Some(entry) = entries.iter().find(|e| e.tag_id == 0x0021) else {
        return;
    };
    let Some(val) = table_ifd::decode_entry(data, entry, Some(0), byte_order, None) else {
        return;
    };
    let Some(v) = val.ints() else { return };
    // ExifTool's PictureWizard table is exactly five int16u fields; on a block
    // of any other size its ProcessBinaryData yields nothing and the raw tag is
    // printed empty (SamsungEK-GN120.jpg).
    if v.len() != 5 {
        return;
    }
    tags.insert(
        "Samsung:PictureWizardMode".to_string(),
        table_ifd::lookup_or_unknown(type2::PICTURE_WIZARD_MODE, v[0]),
    );
    tags.insert("Samsung:PictureWizardColor".to_string(), v[1].to_string());
    for (idx, name) in [
        (2usize, "PictureWizardSaturation"),
        (3, "PictureWizardSharpness"),
        (4, "PictureWizardContrast"),
    ] {
        tags.insert(format!("Samsung:{}", name), (v[idx] - 4).to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::super::registries::samsung::{
        LENS_TYPE, PORTRAIT_EFFECT, SCENE_OPTIMIZER, SCENE_TYPE, SINGLE_TAKE, decode_zoom_level,
    };
    use super::super::shared::generic_decoders::ON_OFF;
    use super::*;

    #[test]
    fn test_decode_scene_optimizer() {
        assert_eq!(SCENE_OPTIMIZER.decode(0), "Off");
        assert_eq!(SCENE_OPTIMIZER.decode(1), "On");
        assert_eq!(SCENE_OPTIMIZER.decode(2), "Auto");
    }

    #[test]
    fn test_decode_scene_type() {
        assert_eq!(SCENE_TYPE.decode(0), "None");
        assert_eq!(SCENE_TYPE.decode(1), "Food");
        assert_eq!(SCENE_TYPE.decode(7), "Night");
    }

    #[test]
    fn test_decode_single_take() {
        assert_eq!(SINGLE_TAKE.decode(0), "Off");
        assert_eq!(SINGLE_TAKE.decode(1), "Recording");
    }

    #[test]
    fn test_decode_portrait_effect() {
        assert_eq!(PORTRAIT_EFFECT.decode(0), "None");
        assert_eq!(PORTRAIT_EFFECT.decode(1), "Blur");
        assert_eq!(PORTRAIT_EFFECT.decode(4), "Color Point");
    }

    #[test]
    fn test_decode_lens_type() {
        assert_eq!(LENS_TYPE.decode(0), "Wide (Main)");
        assert_eq!(LENS_TYPE.decode(1), "Ultra Wide");
        assert_eq!(LENS_TYPE.decode(5), "Telephoto 10x");
    }

    #[test]
    fn test_decode_zoom_level() {
        assert_eq!(decode_zoom_level(10), "1.0x");
        assert_eq!(decode_zoom_level(100), "10.0x");
        assert_eq!(decode_zoom_level(35), "3.5x");
    }

    #[test]
    fn test_on_off_decoder() {
        assert_eq!(ON_OFF.decode(0), "Off");
        assert_eq!(ON_OFF.decode(1), "On");
    }

    #[test]
    fn test_samsung_parser_trait() {
        let parser = SamsungParser::new();
        assert_eq!(parser.manufacturer_name(), "Samsung");
        assert_eq!(parser.tag_prefix(), "Samsung:");
    }

    #[test]
    fn test_validate_header_with_signature() {
        let parser = SamsungParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(b"Samsung");
        data.extend_from_slice(&[0x00]); // Padding
        data.extend_from_slice(&[0x05, 0x00]); // 5 entries

        assert!(parser.validate_header(&data));
    }

    // TODO: These tests require the Samsung registry to properly parse tags.
    // Currently disabled due to IFD data offset calculation issues.
    /*
    #[test]
    fn test_parse_scene_optimizer_tag() {
        let parser = SamsungParser::new();
        let mut data = Vec::new();

        // Samsung signature (7 bytes) + padding byte
        data.extend_from_slice(b"Samsung\0");

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // Scene Optimizer tag entry (tag=0x0001, type=3 (SHORT), count=1, value=1 (On))
        data.extend_from_slice(&[0x01, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Samsung:SceneOptimizer"), Some(&"On".to_string()));
    }

    #[test]
    fn test_registry_based_parsing_all_tags() {
        // This test verifies the TagRegistry pattern works for all tag types
        let parser = SamsungParser::new();
        let mut data = Vec::new();

        // Samsung signature (7 bytes) + padding byte
        data.extend_from_slice(b"Samsung\0");

        // Create IFD with multiple entries
        data.extend_from_slice(&[0x05, 0x00]); // 5 entries

        // 1. Scene Optimizer (custom decoder)
        data.extend_from_slice(&[0x01, 0x00]); // Tag 0x0001
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (Auto)

        // 2. Scene Type (custom decoder)
        data.extend_from_slice(&[0x02, 0x00]); // Tag 0x0002
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Food)

        // 3. Expert RAW (binary on/off)
        data.extend_from_slice(&[0x08, 0x00]); // Tag 0x0008
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (On)

        // 4. Single Take Frame (raw value)
        data.extend_from_slice(&[0x06, 0x00]); // Tag 0x0006
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x05, 0x00, 0x00, 0x00]); // Value: 5

        // 5. Zoom Level (custom function decoder)
        data.extend_from_slice(&[0x1E, 0x00]); // Tag 0x001E
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x1E, 0x00, 0x00, 0x00]); // Value: 30 (3.0x)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(
            tags.get("Samsung:SceneOptimizer"),
            Some(&"Auto".to_string())
        );
        assert_eq!(tags.get("Samsung:SceneType"), Some(&"Food".to_string()));
        assert_eq!(tags.get("Samsung:ExpertRAW"), Some(&"On".to_string()));
        assert_eq!(tags.get("Samsung:SingleTakeFrame"), Some(&"5".to_string()));
        assert_eq!(tags.get("Samsung:ZoomLevel"), Some(&"3.0x".to_string()));
    }
    */
}
