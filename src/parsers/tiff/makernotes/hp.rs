//! HP MakerNote parser
//!
//! Parses HP PhotoSmart digital camera-specific EXIF MakerNote tags.
//! HP (Hewlett-Packard) produced the PhotoSmart series of digital cameras
//! in the early 2000s before exiting the camera market.
//!
//! ## Supported Cameras
//! - PhotoSmart series (consumer point-and-shoot)
//! - PhotoSmart Pro series (prosumer models)
//!
//! ## Supported Features
//! - Camera model and firmware
//! - Image quality and size settings
//! - Flash and exposure modes
//! - Color settings
//! - Special effects
//!
//! ## Tag Structure
//! HP uses a simple proprietary tag structure.

#![allow(dead_code)]

use crate::core::tag_occurrence::intern;
use crate::core::{Instance, Provenance, TagOccurrence};
use crate::exiftool_tables::session::Session;
use crate::exiftool_tables::{Ctx, Dir, find_table, process_binary_data};
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::makernote_context::MakerNoteContext;
use super::registries::hp::hp_registry;
use super::shared::MakerNoteParser;
use super::shared::engine_value::engine_value_text;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// ExifTool's HP::Main (HP.pm:21-38) contains only 0x0e00 PrintIM, which has no
// PrintConv - so there is nothing here to decode. The Quality / ColorMode
// decoders that used to live here were not traceable to HP.pm.

// Lazy-initialized tag registry using centralized registry function
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(hp_registry);

/// MakerNotes.pm:206-214: HP Type4 is selected by its bytes, independent of
/// EXIF Make. The source's character class includes the literal `|` byte.
pub(crate) fn is_type4(data: &[u8]) -> bool {
    data.get(..4) == Some(b"IIII")
        && matches!(data.get(4), Some(4 | 5 | b'|'))
        && data.get(5) == Some(&0)
}

// Extracts a u16 value from an IFD entry's value_offset field
// This handles the case where the value is stored inline in the offset field
// rather than as a pointer to external data
fn extract_u16_value(entry: &IfdEntry, _data: &[u8], byte_order: ByteOrder) -> Option<u16> {
    if entry.value_count != 1 {
        return None;
    }
    // Extract the u16 value from the appropriate bytes of the u32 value_offset
    // based on byte order. Little endian uses lower 16 bits, big endian uses upper 16 bits
    let value = match byte_order {
        ByteOrder::LittleEndian => (entry.value_offset & 0xFFFF) as u16,
        ByteOrder::BigEndian => ((entry.value_offset >> 16) & 0xFFFF) as u16,
    };
    Some(value)
}

/// Parser for HP MakerNotes
pub struct HpParser;

impl Default for HpParser {
    fn default() -> Self {
        Self::new()
    }
}

impl HpParser {
    /// Creates a new HP parser instance
    pub fn new() -> Self {
        HpParser
    }

    fn type4_rows(
        &self,
        data: &[u8],
        cond_ctx: &mut Ctx<'_>,
    ) -> Vec<crate::exiftool_tables::Emitted> {
        let mut rows = Vec::new();
        if let Some(table) = find_table("HP", "Type4") {
            process_binary_data(
                table,
                Dir::whole(data, ByteOrder::LittleEndian.to_io_byte_order()),
                cond_ctx,
                &mut rows,
            );
        }
        rows
    }

    /// Parses a single HP MakerNote IFD entry and extracts its tag value
    /// Uses centralized registry for tag metadata and decoding
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        if let Some(value) = extract_u16_value(entry, data, byte_order) {
            let tag_name = match TAG_REGISTRY.get_tag_name(entry.tag_id) {
                Some(name) => name,
                None => return,
            };

            // Try registry decoding first
            let formatted_value = TAG_REGISTRY.decode_u16(entry.tag_id, value);

            // Fallback for tags without decoder in registry
            let formatted_value = if formatted_value == value.to_string() {
                match entry.tag_id {
                    0x0007 => {
                        let mode = if value > 0 { "On" } else { "Off" };
                        mode.to_string()
                    }
                    0x000B => value.to_string(),
                    _ => formatted_value,
                }
            } else {
                formatted_value
            };

            tags.insert(format!("HP:{}", tag_name), formatted_value);
        }
    }
}

impl MakerNoteParser for HpParser {
    fn manufacturer_name(&self) -> &'static str {
        "HP"
    }

    fn tag_prefix(&self) -> &'static str {
        "HP:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if is_type4(data) {
            let mut members = HashMap::new();
            let mut cond_ctx = Ctx::new(&mut members);
            for row in self.type4_rows(data, &mut cond_ctx) {
                if let Some(text) = engine_value_text(&row.value) {
                    tags.insert(format!("{}:{}", row.group1, row.name), text);
                }
            }
            return Ok(());
        }
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

    fn parse_with_context_and_values_and_session_and_occurrences(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        _session: &mut Session,
        cond_ctx: &mut Ctx<'_>,
        tags: &mut HashMap<String, String>,
        _value_forms: &mut HashMap<String, String>,
        occurrences: &mut Vec<(String, TagOccurrence)>,
    ) -> Result<(), String> {
        if !is_type4(ctx.payload()) {
            return self.parse_with_model(ctx.payload(), byte_order, model, tags);
        }
        for row in self.type4_rows(ctx.payload(), cond_ctx) {
            occurrences.push(type4_occurrence(row));
        }
        Ok(())
    }
}

/// Builds the `HP::Type4` occurrence for one engine row. `raw` is the
/// PrintConv'd display form, as `MetadataMap::get` and serialization project
/// through it; the file's typed value belongs only in `stored`.
fn type4_occurrence(row: crate::exiftool_tables::Emitted) -> (String, TagOccurrence) {
    let key = format!("{}:{}", row.group1, row.name);
    let value = row.value_conv.clone().unwrap_or_else(|| row.value.clone());
    (
        key,
        TagOccurrence {
            id: row.source_id,
            name: intern(row.name),
            group0: intern(row.group0),
            group1: intern(row.group1),
            // HP.pm:65-69 overrides Type4's Camera group for this
            // field. Binary engine rows currently carry the table
            // group2, so retain the field override at this adapter.
            group2: Some(intern(if row.name == "CameraDateTime" {
                "Time"
            } else {
                row.group2
            })),
            instance: Instance::default(),
            raw: row.value.clone(),
            value: Some(value),
            print: Some(row.value),
            stored: Some(row.stored),
            priority: i16::from(!(row.low_priority || row.avoid)),
            is_list: row.is_list,
            order: 0,
            origin: Provenance {
                module: Some(row.module),
                table: Some(row.table),
                byte_range: None,
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hp_parser_trait() {
        let parser = HpParser::new();
        assert_eq!(parser.manufacturer_name(), "HP");
        assert_eq!(parser.tag_prefix(), "HP:");
    }

    /// The previous tests asserted `HP:Quality == "Fine"` for tag 0x0003 and a
    /// `DECODE_QUALITY` table of Normal/Fine/Superfine. None of that is in
    /// ExifTool's `HP.pm`: `HP::Main` holds only 0x0e00 PrintIM, and the
    /// Type4/Type6 tables are binary-data offsets, not IFD tag IDs.
    ///
    /// Until a real HP table is implemented, the parser must emit nothing
    /// rather than invented names.
    #[test]
    fn test_emits_no_fabricated_tags() {
        let parser = HpParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // entry_count = 1
        data.extend_from_slice(&[0x03, 0x00]); // tag = 0x0003
        data.extend_from_slice(&[0x03, 0x00]); // field_type = SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // value_count = 1
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // value = 2

        let mut tags = HashMap::new();
        parser
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .expect("HP maker note should parse");
        assert!(tags.is_empty(), "unexpected HP tags: {tags:?}");
    }
    #[test]
    fn type4_source_signatures_emit_camera_datetime() {
        for signature in [4u8, b'|'] {
            let mut data = vec![0_u8; 118];
            data[..4].copy_from_slice(b"IIII");
            data[4] = signature;
            data[20..40].copy_from_slice(b"2216/02/28 03:49:48\0");
            assert!(is_type4(&data));
            let mut tags = HashMap::new();
            HpParser::new()
                .parse(&data, ByteOrder::LittleEndian, &mut tags)
                .expect("HP Type4 parses");
            assert_eq!(
                tags.get("HP:CameraDateTime"),
                Some(&"2216/02/28 03:49:48".to_string()),
            );
        }
    }

    /// HP.pm Type4 0x10 ExposureTime: int32u microseconds, ValueConv
    /// `$val / 1e6`, PrintConv `PrintExposureTime`. Stored 10000 must reach
    /// `raw` as the display form "1/100", not the stored integer.
    #[test]
    fn test_type4_occurrence_raw_is_the_display_form_not_the_stored_form() {
        use crate::core::TagValue;

        let mut data = b"IIII\x04\x00".to_vec();
        data.resize(0x80, 0);
        data[0x10..0x14].copy_from_slice(&10_000u32.to_le_bytes());
        assert!(is_type4(&data));

        let mut members = HashMap::new();
        let mut cond_ctx = Ctx::new(&mut members);
        let row = HpParser::new()
            .type4_rows(&data, &mut cond_ctx)
            .into_iter()
            .find(|row| row.name == "ExposureTime")
            .expect("HP::Type4 emits ExposureTime");
        assert_eq!(row.stored, TagValue::Integer(10_000));

        let (key, occurrence) = type4_occurrence(row);
        assert_eq!(key, "HP:ExposureTime");
        assert_eq!(
            occurrence.raw,
            TagValue::new_string("1/100"),
            "raw must be the PrintConv'd display form, not the stored integer"
        );
        assert_eq!(occurrence.stored, Some(TagValue::Integer(10_000)));
    }
}
