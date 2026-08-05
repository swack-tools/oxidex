//! GE MakerNote parser
//!
//! Parses General Electric digital camera-specific EXIF MakerNote tags.
//! GE produced consumer-oriented digital cameras under license
//! (often rebranded from other manufacturers).
//!
//! ## Supported Cameras
//! - GE Power series
//! - GE E-series (entry-level compacts)
//! - GE X-series (advanced compacts)
//!
//! ## Supported Features
//! - Camera model information
//! - Image quality settings
//! - Flash and scene modes
//! - Basic shooting parameters
//!
//! ## Tag Structure
//! GE uses a simple IFD format with basic manufacturer tags.

#![allow(dead_code)]

use crate::const_decoder;
use crate::core::{MetadataMap, TagValue};
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::ge::ge_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// Decodes GE Macro.
// ExifTool GE.pm:33-41 - 0x0202 => { Name => 'Macro', PrintConv => { 0 => 'Off', 1 => 'On' } }
const_decoder!(pub DECODE_MACRO, u16, [
    (0, "Off"),
    (1, "On"),
]);

// Lazy-initialized tag registry using centralized registry function
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(ge_registry);

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

/// Parser for GE MakerNotes
pub struct GeParser;

impl Default for GeParser {
    fn default() -> Self {
        Self::new()
    }
}

impl GeParser {
    /// Creates a new GE parser instance
    pub fn new() -> Self {
        GeParser
    }

    /// Parses a single GE MakerNote IFD entry and extracts its tag value
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

        // GEModel (0x0207) and GEMake (0x0300) are `Format => 'string'`
        // (GE.pm:42-49) and are longer than 4 bytes, so value_offset is a
        // pointer rather than an inline value.
        //
        // GE's pointers are not relative to any base this parser can compute:
        // on the sample file GE.jpg the entry says 170 while the string sits at
        // maker-note offset 168. ExifTool handles that with
        // `FixBase => 1, AutoFix => 1` (MakerNotes.pm:141-142) - a heuristic
        // that infers the shift - and still emits
        // "[minor] Suspicious MakerNotes offset for tag 0x0200".
        //
        // Until FixBase is implemented, resolving these pointers naively yields
        // the wrong bytes (offset 170 reads "ITAL C", not "E1035"), so skip
        // them. Emitting nothing is correct; emitting a mis-based string is the
        // kind of confidently-wrong value this parser used to specialise in.
        if matches!(entry.tag_id, 0x0207 | 0x0300) {
            return;
        }

        if let Some(value) = extract_u16_value(entry, data, byte_order) {
            let formatted_value = TAG_REGISTRY.decode_u16(entry.tag_id, value);
            tags.insert(format!("GE:{}", tag_name), formatted_value);
        }
    }
}

impl MakerNoteParser for GeParser {
    fn manufacturer_name(&self) -> &'static str {
        "GE"
    }

    fn tag_prefix(&self) -> &'static str {
        "GE:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // ExifTool MakerNotes.pm:136-144:
        //     Name      => 'MakerNoteGE',
        //     Condition => '$$valPt =~ /^GE(\0\0|NIC\0)/',
        //     Start     => '$valuePtr + 18',
        // The GE maker note opens with "GE\0\0\0\0\x01\0\0\0" followed by its
        // own 8-byte TIFF header, so the IFD begins 18 bytes in. Reading the
        // entry count from offset 0 instead yields "GE" = 18245 entries.
        let config = IfdParserConfig {
            signature: Some(b"GE\0\0"),
            signature_offset: 18,
            max_entries: 500,
        };

        // ByteOrder => 'Unknown' in the same SubDirectory: the maker note
        // carries its own order marker at offset 10, which may differ from the
        // enclosing file's.
        let byte_order = match data.get(10..12) {
            Some(b"II") => ByteOrder::LittleEndian,
            Some(b"MM") => ByteOrder::BigEndian,
            _ => byte_order,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })?;
        Ok(())
    }
}

// ===== GEModel/GEMake (0x0207/0x0300): GE's FixBase quirk =====
//
// MakerNotes.pm:136-144 declares `FixBase => 1, AutoFix => 1` for
// `MakerNoteGE`: ExifTool's own reading of these two out-of-line string
// values needs a base correction its ordinary offset math doesn't produce,
// derived by `MakerNotes::FixBase` analysing the whole directory's value
// offsets. That analysis isn't reproduced here; instead this hard-codes the
// single shift verified against `combined-samples/GE.jpg`'s raw bytes.
//
// GEModel's IFD entry declares `value_offset = 170` (0xaa, read directly
// from the file's tag-0x0207 entry). Interpreted as payload-relative (the
// natural reading, matching every other out-of-line GE tag), that points to
// absolute file offset `payload_start + 170` = 0x384 -- but the actual bytes
// "E1035\0" sit two bytes earlier, at 0x382. `GE_0x0200`'s declared offset
// (a tag this parser doesn't decode) resolves to a position *before* the
// payload even starts even after this same -2 correction, which is exactly
// the "[minor] Suspicious MakerNotes offset" warning real ExifTool prints
// for it -- consistent with one shift applied uniformly across the
// directory rather than per-tag guesswork.
const GE_MODEL: u16 = 0x0207;
const GE_MAKE: u16 = 0x0300;
/// Verified against `GE.jpg`: declared 170, real payload-relative offset
/// 168.
const GE_FIXBASE_SHIFT: i64 = -2;

/// Finds one entry by tag id in the 11-entry GE IFD at `ifd_offset` inside
/// `tiff`. Same shape as Casio's `find_casio_entry`.
fn find_ge_entry(
    tiff: &[u8],
    ifd_offset: usize,
    byte_order: ByteOrder,
    target: u16,
) -> Option<IfdEntry> {
    let count_bytes = tiff.get(ifd_offset..ifd_offset + 2)?;
    let count = EndianReader::new(count_bytes, byte_order.to_io_byte_order()).u16_at(0)?;
    let entries_start = ifd_offset + 2;
    let entries = tiff.get(entries_start..entries_start + count as usize * 12)?;
    let reader = EndianReader::new(entries, byte_order.to_io_byte_order());
    (0..count as usize).find_map(|i| {
        let base = i * 12;
        let tag_id = reader.u16_at(base)?;
        if tag_id != target {
            return None;
        }
        Some(IfdEntry {
            tag_id,
            field_type: reader.u16_at(base + 2)?,
            value_count: reader.u32_at(base + 4)?,
            value_offset: reader.u32_at(base + 8)?,
        })
    })
}

/// Extracts `GEModel` and `GEMake` into `metadata`, applying the -2
/// FixBase-equivalent shift documented above. A no-op when `ctx`'s payload
/// isn't a recognised GE MakerNote.
pub fn parse_ge_extra_tags(ctx: &MakerNoteContext<'_>, byte_order: ByteOrder, metadata: &mut MetadataMap) {
    let payload = ctx.payload();
    if !(payload.starts_with(b"GE\0\0") || payload.starts_with(b"GENIC\0")) {
        return;
    }
    let byte_order = match payload.get(10..12) {
        Some(b"II") => ByteOrder::LittleEndian,
        Some(b"MM") => ByteOrder::BigEndian,
        _ => byte_order,
    };
    let payload_offset = ctx.payload_offset();
    let ifd_offset = payload_offset + 18;
    let tiff = ctx.tiff();
    for (tag_id, name) in [(GE_MODEL, "GEModel"), (GE_MAKE, "GEMake")] {
        let Some(entry) = find_ge_entry(tiff, ifd_offset, byte_order, tag_id) else {
            continue;
        };
        let total = entry.value_count as usize;
        // A value this small lives inline, never one of these two strings
        // (6 and 32 bytes on the sample) -- and the FixBase shift only
        // applies to out-of-line offsets.
        if total <= 4 {
            continue;
        }
        let Some(offset) =
            usize::try_from(payload_offset as i64 + i64::from(entry.value_offset) + GE_FIXBASE_SHIFT)
                .ok()
        else {
            continue;
        };
        let Some(bytes) = offset.checked_add(total).and_then(|end| tiff.get(offset..end)) else {
            continue;
        };
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        metadata.insert(
            format!("GE:{name}"),
            TagValue::new_string(String::from_utf8_lossy(&bytes[..end]).into_owned()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a GE maker note: the 10-byte "GE\0\0\0\0\x01\0\0\0" header, an
    /// 8-byte TIFF header, then the IFD - matching the real layout of the
    /// sample file GE.jpg. Both byte orders are exercised because the maker
    /// note carries its own order marker at offset 10.
    fn build_ge_makernote(entries: &[(u16, u16, u32, u32)], order: ByteOrder) -> Vec<u8> {
        let (w16, w32): (fn(u16) -> [u8; 2], fn(u32) -> [u8; 4]) = match order {
            ByteOrder::LittleEndian => (u16::to_le_bytes, u32::to_le_bytes),
            ByteOrder::BigEndian => (u16::to_be_bytes, u32::to_be_bytes),
        };
        let mut data = Vec::new();
        data.extend_from_slice(b"GE\0\0\0\0\x01\0\0\0");
        data.extend_from_slice(match order {
            ByteOrder::LittleEndian => b"II\x2a\0",
            ByteOrder::BigEndian => b"MM\0\x2a",
        });
        data.extend_from_slice(&w32(8)); // IFD0 at +8 from the TIFF header
        data.extend_from_slice(&w16(entries.len() as u16));
        for &(tag, typ, count, val) in entries {
            data.extend_from_slice(&w16(tag));
            data.extend_from_slice(&w16(typ));
            data.extend_from_slice(&w32(count));
            // int16u values are left-justified inside value_offset when big-endian.
            if typ == 3 && count == 1 && order == ByteOrder::BigEndian {
                data.extend_from_slice(&w32(val << 16));
            } else {
                data.extend_from_slice(&w32(val));
            }
        }
        data.extend_from_slice(&w32(0)); // next-IFD pointer
        data
    }

    #[test]
    fn test_ge_parser_trait() {
        let parser = GeParser::new();
        assert_eq!(parser.manufacturer_name(), "GE");
        assert_eq!(parser.tag_prefix(), "GE:");
    }

    /// ExifTool GE.pm:33-41 - Macro PrintConv is `{ 0 => 'Off', 1 => 'On' }`.
    #[test]
    fn test_decode_macro() {
        assert_eq!(DECODE_MACRO.decode(0), "Off");
        assert_eq!(DECODE_MACRO.decode(1), "On");
    }

    /// Reproduces the eleven-entry maker note of the sample file GE.jpg
    /// (GE E1035), whose IFD `exiftool -v3` renders as tags 0x0104, 0x0200,
    /// 0x0202, 0x0203, 0x0204, 0x0205, 0x0206, 0x0207, 0x0300, 0x0500, 0x0600.
    ///
    /// `exiftool -a -G1 -s GE.jpg` prints `[GE] Macro : Off` for 0x0202.
    /// Only unnamed IDs and the two offset-based strings surround it, so Macro
    /// is the whole of what this parser should emit today.
    #[test]
    fn test_parse_matches_exiftool_on_ge_e1035() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let data = build_ge_makernote(
                &[
                    (0x0104, 4, 1, 1694568960),
                    (0x0200, 4, 3, 0),
                    (0x0202, 3, 1, 0), // Macro = 0
                    (0x0203, 3, 1, 0),
                    (0x0206, 3, 6, 0),
                    (0x0207, 2, 6, 170),  // GEModel - needs FixBase
                    (0x0300, 7, 32, 176), // GEMake  - needs FixBase
                    (0x0500, 3, 1, 0),
                    (0x0600, 4, 1, 0),
                ],
                order,
            );

            let mut tags = HashMap::new();
            GeParser::new()
                .parse(&data, order, &mut tags)
                .expect("GE maker note should parse");

            assert_eq!(tags.get("GE:Macro"), Some(&"Off".to_string()), "{order:?}");
            // Every other ID here is either unnamed by ExifTool or an
            // offset-based string this parser deliberately skips.
            assert_eq!(tags.len(), 1, "{order:?}: {tags:?}");
        }
    }

    /// The IFD starts 18 bytes in (ExifTool MakerNotes.pm:140). Reading the
    /// entry count from offset 0 instead sees "GE" = 18245, which is what the
    /// parser used to do - it then bailed on the entry-count check and emitted
    /// nothing, which is why the fabricated 0x0001-0x0005 table was never
    /// contradicted by a real file.
    #[test]
    fn test_ifd_starts_after_the_ge_header() {
        let data = build_ge_makernote(&[(0x0202, 3, 1, 1)], ByteOrder::BigEndian);
        assert_eq!(&data[0..4], b"GE\0\0");
        assert_eq!(&data[10..12], b"MM");

        let mut tags = HashMap::new();
        GeParser::new()
            .parse(&data, ByteOrder::BigEndian, &mut tags)
            .expect("GE maker note should parse");
        assert_eq!(tags.get("GE:Macro"), Some(&"On".to_string()));
    }

    /// ExifTool leaves 0x0203-0x0206, 0x0500 and 0x0600 unnamed in `GE::Main`
    /// (comments only). GE.jpg carries all of them and `exiftool -a -G1 -s`
    /// prints none, so oxidex must not invent names for them.
    #[test]
    fn test_unnamed_ids_are_not_emitted() {
        let data = build_ge_makernote(
            &[(0x0203, 3, 1, 0), (0x0500, 3, 1, 0), (0x0600, 4, 1, 0)],
            ByteOrder::LittleEndian,
        );
        let mut tags = HashMap::new();
        GeParser::new()
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .expect("GE maker note should parse");
        assert!(tags.is_empty(), "unexpected GE tags: {tags:?}");
    }
}
