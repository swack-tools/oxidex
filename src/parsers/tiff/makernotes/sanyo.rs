//! Sanyo MakerNote parser
//!
//! Parses Sanyo digital camera-specific EXIF MakerNote tags.
//! Sanyo was known for the Xacti series of dual-camera/camcorder devices
//! and waterproof/ruggedized cameras.
//!
//! Tag IDs, names and PrintConv maps are transcribed from ExifTool's
//! `Sanyo.pm` `%Image::ExifTool::Sanyo::Main` (see `registries::sanyo` and
//! the comments below), verified against the pinned 13.59 oracle on
//! `combined-samples/Sanyo.jpg`.
//!
//! ## Tag Structure
//! `MakerNoteSanyo` (`MakerNotes.pm:982-991`) is a standard TIFF IFD
//! starting after an 8-byte `"SANYO\0"` + 2-byte pad header
//! (`Start => '$valuePtr + 8'`), byte order resolved per-file
//! (`ByteOrder => 'Unknown'`).

#![allow(dead_code)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::sanyo::sanyo_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::print_im::decode_print_im_from_ifd;
use super::shared::tag_registry::TagRegistry;

const SANYO_MAKER_NOTE_OFFSET: u16 = 0x00FF;
const SANYO_QUALITY: u16 = 0x0201;

// ============================================================================
// Helper Functions
// ============================================================================

fn extract_u16_value(entry: &IfdEntry, _data: &[u8], byte_order: ByteOrder) -> Option<u16> {
    if entry.value_count != 1 {
        return None;
    }
    let value = match byte_order {
        ByteOrder::LittleEndian => (entry.value_offset & 0xFFFF) as u16,
        ByteOrder::BigEndian => ((entry.value_offset >> 16) & 0xFFFF) as u16,
    };
    Some(value)
}

/// Sanyo.pm:46-77 (`SanyoQuality`, `Flags => 'PrintHex'`). The unmapped
/// fallback is `ExifTool.pm`'s `Unknown (0x%x)` form (`ExifTool.pm:3631`) --
/// the hex variant `PrintHex` selects, not the decimal `Unknown (n)` every
/// other tag in this file falls back to.
fn decode_sanyo_quality(value: u16) -> String {
    let level = match value & 0xFF00 {
        0x0000 => "Normal",
        0x0100 => "Fine",
        0x0200 => "Super Fine",
        _ => return format!("Unknown (0x{value:x})"),
    };
    let detail = match value & 0x00FF {
        0 => "Very Low",
        1 => "Low",
        2 => "Medium Low",
        3 => "Medium",
        4 => "Medium High",
        5 => "High",
        6 => "Very High",
        7 => "Super High",
        _ => return format!("Unknown (0x{value:x})"),
    };
    format!("{level}/{detail}")
}

// ============================================================================
// Tag Registry
// ============================================================================

// Lazy-initialized tag registry using centralized registry function
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(sanyo_registry);

// ============================================================================
// Parser Implementation
// ============================================================================

/// Parser for Sanyo MakerNotes
pub struct SanyoParser;

impl Default for SanyoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SanyoParser {
    /// Creates a new SanyoParser instance
    pub fn new() -> Self {
        SanyoParser
    }

    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        // Sanyo.pm:29-33: `MakerNoteOffset` is `int32u`, not `int16u` --
        // every other scalar tag this parser reads is a `u16` extracted via
        // `extract_u16_value`, which would misread (or, for a genuinely
        // 4-byte value, simply not apply since `value_count` stays 1 but the
        // field is twice as wide) this one.
        if entry.tag_id == SANYO_MAKER_NOTE_OFFSET && entry.value_count == 1 {
            tags.insert(
                "Sanyo:MakerNoteOffset".to_string(),
                entry.value_offset.to_string(),
            );
            return;
        }

        let Some(value) = extract_u16_value(entry, data, byte_order) else {
            return;
        };

        if entry.tag_id == SANYO_QUALITY {
            tags.insert(
                "Sanyo:SanyoQuality".to_string(),
                decode_sanyo_quality(value),
            );
            return;
        }

        let Some(tag_name) = TAG_REGISTRY.get_tag_name(entry.tag_id) else {
            return;
        };
        let formatted_value = TAG_REGISTRY.decode_u16(entry.tag_id, value);
        tags.insert(format!("Sanyo:{tag_name}"), formatted_value);
    }
}

impl MakerNoteParser for SanyoParser {
    fn manufacturer_name(&self) -> &'static str {
        "Sanyo"
    }

    fn tag_prefix(&self) -> &'static str {
        "Sanyo:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        data.starts_with(b"SANYO\0") || data.len() >= 2
    }

    fn parse_with_context(
        &self,
        ctx: &crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext<'_>,
        byte_order: ByteOrder,
        _model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        let ifd_at = usize::from(ctx.payload().starts_with(b"SANYO\0")) * 8;
        if let Some(version) = decode_print_im_from_ifd(ctx, ifd_at, byte_order) {
            tags.insert("PrintIM:PrintIMVersion".to_string(), version);
        }
        let Some(ifd) = ctx.payload().get(ifd_at..) else {
            return Ok(());
        };
        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 500,
        };
        parse_ifd_entries(ifd, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 500,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })?;
        Ok(())
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanyo_quality_decoder() {
        assert_eq!(decode_sanyo_quality(0x0106), "Fine/Very High");
        assert_eq!(decode_sanyo_quality(0x0000), "Normal/Very Low");
        assert_eq!(decode_sanyo_quality(0x0207), "Super Fine/Super High");
        assert_eq!(decode_sanyo_quality(0x0300), "Unknown (0x300)");
    }

    #[test]
    fn test_sanyo_parser_trait() {
        let parser = SanyoParser::new();
        assert_eq!(parser.manufacturer_name(), "Sanyo");
        assert_eq!(parser.tag_prefix(), "Sanyo:");
    }

    #[test]
    fn test_parse_optical_zoom_on() {
        let parser = SanyoParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x19, 0x02]); // Tag 0x0219 OpticalZoomOn
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (On)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Sanyo:OpticalZoomOn"), Some(&"On".to_string()));
    }

    #[test]
    fn test_parse_maker_note_offset() {
        let parser = SanyoParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0xFF, 0x00]); // Tag 0x00ff MakerNoteOffset
        data.extend_from_slice(&[0x04, 0x00]); // Type: LONG
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&(1076u32).to_le_bytes()); // Value: 1076

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(
            tags.get("Sanyo:MakerNoteOffset"),
            Some(&"1076".to_string())
        );
    }

    #[test]
    fn test_tag_registry() {
        assert_eq!(TAG_REGISTRY.get_tag_name(0x0218), Some("FlickerReduce"));
        assert!(TAG_REGISTRY.has_tag(0x021F));
    }
}
