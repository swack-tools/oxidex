//! JVC MakerNote parser
//!
//! Parses JVC digital camera-specific EXIF MakerNote tags.
//! JVC (Victor Company of Japan) produced digital cameras and camcorders,
//! particularly known for their video-focused features.
//!
//! ## Supported Cameras
//! - GC series (digital cameras)
//! - Everio series (hybrid photo/video cameras)
//!
//! ## Supported Features
//! - Camera model and firmware
//! - Image quality settings
//! - Focus and flash modes
//! - Color and scene modes
//!
//! ## Tag Structure
//! JVC uses a simple IFD format with basic tag structure.

#![allow(dead_code)]

use crate::const_decoder;
use crate::core::tag_occurrence::intern;
use crate::core::{Instance, Provenance, TagOccurrence, TagValue};
use crate::exiftool_tables::Ctx;
use crate::exiftool_tables::session::Session;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::makernote_context::MakerNoteContext;
use super::registries::jvc::jvc_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// Decodes JVC image quality.
// ExifTool JVC.pm:32-41 - 0x0003 => { Name => 'Quality',
//   PrintConv => { 0 => 'Low', 1 => 'Normal', 2 => 'Fine' } }
const_decoder!(pub DECODE_QUALITY, u16, [
    (0, "Low"),
    (1, "Normal"),
    (2, "Fine"),
]);

// Lazy-initialized tag registry using centralized registry function
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(jvc_registry);

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

// Reads an IFD entry's bytes verbatim, keeping interior NULs.
//
// `extract_string` stops at the first NUL when the value is stored inline,
// which would truncate CPUVersions; ExifTool hands the whole buffer to its
// ValueConv instead.
fn extract_raw_bytes(entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> Option<Vec<u8>> {
    if entry.value_count == 0 {
        return None;
    }
    let bytes = if entry.value_count <= 4 {
        (0..entry.value_count as usize)
            .map(|i| match byte_order {
                ByteOrder::LittleEndian => ((entry.value_offset >> (i * 8)) & 0xFF) as u8,
                ByteOrder::BigEndian => ((entry.value_offset >> (24 - i * 8)) & 0xFF) as u8,
            })
            .collect::<Vec<u8>>()
    } else {
        let offset = entry.value_offset as usize;
        let end = offset.checked_add(entry.value_count as usize)?;
        data.get(offset..end)?.to_vec()
    };
    Some(bytes)
}

fn extract_raw_string(entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> Option<String> {
    extract_raw_bytes(entry, data, byte_order)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

// Applies ExifTool's CPUVersions ValueConv (JVC.pm:28-31):
//
//     ValueConv => '$_=$val; s/(\s*\0)+$//; s/(\s*\0)+/, /g; $_'
//
// i.e. drop trailing runs of "optional whitespace then NUL", then join the
// remaining such runs with ", ".
fn convert_cpu_versions(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();

    // Find where the trailing (\s*\0)+ run begins.
    let mut end = chars.len();
    loop {
        let mut i = end;
        while i > 0 && chars[i - 1] == '\0' {
            let mut j = i - 1;
            while j > 0 && chars[j - 1].is_whitespace() {
                j -= 1;
            }
            i = j;
        }
        if i == end {
            break;
        }
        end = i;
    }

    let mut out = String::new();
    let mut i = 0;
    while i < end {
        // Try to match a (\s*\0)+ separator starting at i.
        let mut j = i;
        let mut matched = false;
        loop {
            let mut k = j;
            while k < end && chars[k].is_whitespace() && chars[k] != '\0' {
                k += 1;
            }
            if k < end && chars[k] == '\0' {
                j = k + 1;
                matched = true;
            } else {
                break;
            }
        }
        if matched {
            out.push_str(", ");
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// Parser for JVC MakerNotes
pub struct JvcParser;

impl Default for JvcParser {
    fn default() -> Self {
        Self::new()
    }
}

impl JvcParser {
    /// Creates a new JVC parser instance
    pub fn new() -> Self {
        JvcParser
    }

    /// Walk the MakerNote directory in `directory_data`, resolving its
    /// out-of-line TIFF entry values through `value_data`. `CPUVersions` in
    /// JVC.jpg is at a TIFF-relative offset, so the declared MakerNote alone
    /// does not provide the base that ExifTool's directory inherits.
    fn parse_note(
        &self,
        directory_data: &[u8],
        value_data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
        mut occurrences: Option<&mut Vec<(String, TagOccurrence)>>,
    ) -> Result<(), String> {
        let config = IfdParserConfig {
            // MakerNotes.pm selects `MakerNoteJVC` on `^JVC ` and declares
            // `Start => '$valuePtr + 4'`. Headerless synthetic/legacy notes
            // retain the shared parser's normal zero-offset behavior.
            signature: Some(b"JVC "),
            signature_offset: 4,
            max_entries: 500,
        };

        parse_ifd_entries(directory_data, byte_order, &config, |entry, _| {
            if let Some(row) = self.canonical_entry(entry, value_data, byte_order)
                && let Some(rows) = occurrences.as_deref_mut()
            {
                rows.push(row);
                return;
            }
            self.parse_entry(entry, value_data, byte_order, tags);
        })
    }

    fn canonical_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
    ) -> Option<(String, TagOccurrence)> {
        let (name, raw, value, print) = match entry.tag_id {
            0x0002 => {
                let bytes = extract_raw_bytes(entry, data, byte_order)?;
                let converted =
                    TagValue::new_string(convert_cpu_versions(&String::from_utf8_lossy(&bytes)));
                (
                    "CPUVersions",
                    TagValue::Binary(bytes),
                    converted.clone(),
                    converted,
                )
            }
            0x0003 => {
                let value = extract_u16_value(entry, data, byte_order)?;
                let raw = TagValue::Integer(i64::from(value));
                (
                    "Quality",
                    raw.clone(),
                    raw,
                    TagValue::new_string(TAG_REGISTRY.decode_u16(entry.tag_id, value)),
                )
            }
            _ => return None,
        };
        Some((
            format!("JVC:{name}"),
            TagOccurrence {
                id: crate::core::TagId::Numeric(entry.tag_id),
                name: intern(name),
                group0: intern("MakerNotes"),
                group1: intern("JVC"),
                group2: Some(intern("Camera")),
                instance: Instance::default(),
                stored: Some(raw.clone()),
                raw,
                value: Some(value),
                print: Some(print),
                priority: 1,
                is_list: false,
                order: 0,
                origin: Provenance {
                    module: Some("JVC"),
                    table: Some("Main"),
                    byte_range: None,
                },
            },
        ))
    }

    /// Parses a single JVC MakerNote IFD entry and extracts its tag value
    /// Uses centralized registry for tag metadata and decoding
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        let tag_name = match TAG_REGISTRY.get_tag_name(entry.tag_id) {
            Some(name) => name,
            None => return,
        };

        // CPUVersions (0x0002) is a NUL-separated string list.
        if entry.tag_id == 0x0002 {
            if let Some(raw) = extract_raw_string(entry, data, byte_order) {
                tags.insert(format!("JVC:{}", tag_name), convert_cpu_versions(&raw));
            }
            return;
        }

        if let Some(value) = extract_u16_value(entry, data, byte_order) {
            let formatted_value = TAG_REGISTRY.decode_u16(entry.tag_id, value);
            tags.insert(format!("JVC:{}", tag_name), formatted_value);
        }
    }
}

impl MakerNoteParser for JvcParser {
    fn manufacturer_name(&self) -> &'static str {
        "JVC"
    }

    fn tag_prefix(&self) -> &'static str {
        "JVC:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        self.parse_note(data, data, byte_order, tags, None)
    }

    fn parse_with_context(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        _model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        self.parse_note(ctx.payload(), ctx.tiff(), byte_order, tags, None)
    }

    fn parse_with_context_and_values_and_session_and_occurrences(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        _model: Option<&str>,
        _session: &mut Session,
        _cond_ctx: &mut Ctx<'_>,
        tags: &mut HashMap<String, String>,
        _value_forms: &mut HashMap<String, String>,
        occurrences: &mut Vec<(String, TagOccurrence)>,
    ) -> Result<(), String> {
        self.parse_note(
            ctx.payload(),
            ctx.tiff(),
            byte_order,
            tags,
            Some(occurrences),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a maker-note IFD in the requested byte order so both branches of
    /// `extract_u16_value` / `extract_raw_string` are exercised.
    fn build_ifd(entries: &[(u16, u16, u32, u32)], tail: &[u8], order: ByteOrder) -> Vec<u8> {
        let w16 = |v: u16| -> [u8; 2] {
            match order {
                ByteOrder::LittleEndian => v.to_le_bytes(),
                ByteOrder::BigEndian => v.to_be_bytes(),
            }
        };
        let w32 = |v: u32| -> [u8; 4] {
            match order {
                ByteOrder::LittleEndian => v.to_le_bytes(),
                ByteOrder::BigEndian => v.to_be_bytes(),
            }
        };
        let mut data = Vec::new();
        data.extend_from_slice(&w16(entries.len() as u16));
        for &(tag, typ, count, val) in entries {
            data.extend_from_slice(&w16(tag));
            data.extend_from_slice(&w16(typ));
            data.extend_from_slice(&w32(count));
            if typ == 3 && count == 1 && order == ByteOrder::BigEndian {
                data.extend_from_slice(&w32(val << 16));
            } else {
                data.extend_from_slice(&w32(val));
            }
        }
        data.extend_from_slice(&w32(0)); // next-IFD pointer
        data.extend_from_slice(tail);
        data
    }

    /// ExifTool JVC.pm:34-39 - `{ 0 => 'Low', 1 => 'Normal', 2 => 'Fine' }`.
    /// The previous table claimed 0 => "Standard" and 2 => "Super Fine", which
    /// appear nowhere in JVC.pm.
    #[test]
    fn test_decode_quality() {
        assert_eq!(DECODE_QUALITY.decode(0), "Low");
        assert_eq!(DECODE_QUALITY.decode(1), "Normal");
        assert_eq!(DECODE_QUALITY.decode(2), "Fine");
    }

    #[test]
    fn test_jvc_parser_trait() {
        let parser = JvcParser::new();
        assert_eq!(parser.manufacturer_name(), "JVC");
        assert_eq!(parser.tag_prefix(), "JVC:");
    }

    /// ExifTool JVC.pm:28-31:
    /// `ValueConv => '$_=$val; s/(\s*\0)+$//; s/(\s*\0)+/, /g; $_'`
    #[test]
    fn test_convert_cpu_versions_matches_exiftool_valueconv() {
        // The literal 70-byte payload shape of the corpus file JVC.jpg.
        let mut raw = String::new();
        raw.push_str("CPU1 2.00\0");
        raw.push_str("0\0");
        raw.push_str("CPU2 0496\0");
        raw.push('0');
        raw.push_str(&"\0".repeat(47));

        // `exiftool -a -G1 -s JVC.jpg` prints exactly this.
        assert_eq!(convert_cpu_versions(&raw), "CPU1 2.00, 0, CPU2 0496, 0");

        // A trailing "space then NUL" run must also be stripped, not turned
        // into a trailing ", " - that is what `s/(\s*\0)+$//` is for.
        assert_eq!(convert_cpu_versions("A\0B \0\0"), "A, B");
        assert_eq!(convert_cpu_versions("A"), "A");
    }

    /// Reproduces the maker note of the corpus file `JVC.jpg` (JVC GR-DV500).
    ///
    /// Ground truth, `exiftool -a -G1 -s JVC.jpg`:
    /// ```text
    /// [JVC]           CPUVersions                     : CPU1 2.00, 0, CPU2 0496, 0
    /// [JVC]           Quality                         : Normal
    /// ```
    #[test]
    fn test_parse_matches_exiftool_on_jvc_gr_dv500() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let mut tail = Vec::new();
            tail.extend_from_slice(b"CPU1 2.00\0");
            tail.extend_from_slice(b"0\0");
            tail.extend_from_slice(b"CPU2 0496\0");
            tail.push(b'0');
            tail.extend_from_slice(&[0u8; 47]);

            // 2 (count) + 3*12 (entries) + 4 (next-IFD) = 42
            let data = build_ifd(
                &[
                    (0x0001, 3, 1, 2),   // unnamed by ExifTool (JVC.pm:26)
                    (0x0002, 2, 70, 42), // CPUVersions
                    (0x0003, 3, 1, 1),   // Quality = 1
                ],
                &tail,
                order,
            );

            let mut tags = HashMap::new();
            JvcParser::new()
                .parse(&data, order, &mut tags)
                .expect("JVC maker note should parse");

            assert_eq!(
                tags.get("JVC:CPUVersions"),
                Some(&"CPU1 2.00, 0, CPU2 0496, 0".to_string()),
                "{order:?}"
            );
            assert_eq!(
                tags.get("JVC:Quality"),
                Some(&"Normal".to_string()),
                "{order:?}"
            );
            // ExifTool emits exactly two JVC tags for this file; 0x0001 is
            // deliberately unnamed and must not be reported as Quality.
            assert_eq!(tags.len(), 2, "{order:?}: {tags:?}");
        }
    }
}
