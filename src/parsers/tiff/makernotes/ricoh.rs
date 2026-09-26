//! Ricoh MakerNote parser
//!
//! Parses Ricoh digital camera-specific EXIF MakerNote tags.
//! Ricoh (and later Pentax Ricoh) produced compact cameras and
//! specialized models like the GR series and Theta 360 cameras.
//!
//! ## Supported Cameras
//! - GR Digital series (advanced compact)
//! - Caplio series (consumer compact)
//! - CX series (high-zoom compact)
//!
//! ## Supported Features
//! - Camera model and settings
//! - Exposure and focus modes
//! - Image quality settings
//! - Flash and white balance
//! - Special shooting modes
//!
//! ## Tag Structure
//! Ricoh uses a standard IFD format similar to Pentax.

#![allow(dead_code)]

use crate::core::tag_occurrence::intern;
use crate::core::{Instance, Provenance, TagOccurrence};
use crate::core::{MetadataMap, TagValue};
use crate::exiftool_tables::Ctx;
use crate::exiftool_tables::session::Session;
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::ricoh::ricoh_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries, resolve_byte_order_at};
use super::shared::print_im::decode_print_im_from_ifd;
use super::shared::tag_registry::TagRegistry;

// ============================================================================
// Ricoh MakerNote Tag IDs (for parsing reference)
// ============================================================================
// Tag definitions are centralized in the registry (registries/ricoh.rs)
// These constants are retained for parse_entry() to identify special handling

const RICOH_FOCUS_MODE: u16 = 0x001D;
const RICOH_ISO_SETTING: u16 = 0x0022;
const RICOH_SHARPNESS: u16 = 0x0035;

// Static registry instance for efficient tag lookup and decoding
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(ricoh_registry);

/// MakerNotes.pm:924-938. Preserve the source's case-sensitive Make gate and
/// padded TIFF probe; Ricoh Imaging bodies otherwise reach Pentax by Make.
pub(crate) fn is_type2_selector(make: &str, model: Option<&str>, data: &[u8]) -> bool {
    let make = make.strip_prefix("PENTAX ").unwrap_or(make);
    if !make.starts_with("RICOH") {
        return false;
    }
    if model == Some("RICOH WG-M1") {
        return true;
    }
    let Some(header) = data.get(..12) else {
        return false;
    };
    (header[..8] == *b"MM\0*\0\0\0\x08" && header[8] == 0 && header[10..12] == [0, 0])
        || (header[..8] == *b"II*\0\x08\0\0\0" && header[9..12] == [0, 0, 0])
}

/// MakerNotes.pm:1742-1758 `ProcessKodakPatch` rewrites the count two bytes
/// forward, then starts `ProcessExif` there. Values remain MakerNote-relative
/// because Ricoh2 declares `Base => '$start - 8'`.
fn parse_ricoh_type2_rows(
    ctx: &MakerNoteContext<'_>,
    inherited_order: ByteOrder,
) -> Vec<(String, TagOccurrence)> {
    let payload = ctx.payload();
    // MakerNotes.pm:937 `ByteOrder => 'Unknown'` is resolved by Exif.pm:6982-6993
    // from the int16u at `$valuePtr + 8`, before the patch below moves the
    // count. This also covers the headerless WG-M1 model override.
    let order = resolve_byte_order_at(payload, 8, inherited_order);
    let reader = EndianReader::new(payload, order.to_io_byte_order());
    let Some(first) = reader.u16_at(8) else {
        return Vec::new();
    };
    let Some(second) = reader.u16_at(10) else {
        return Vec::new();
    };
    let count = usize::from(if first != 0 { first } else { second });
    if count == 0 {
        return Vec::new();
    }
    let Some(end) = count
        .checked_mul(12)
        .and_then(|bytes| 12usize.checked_add(bytes))
    else {
        return Vec::new();
    };
    if end > payload.len() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for index in 0..count {
        let start = 12 + index * 12;
        let entry = &payload[start..start + 12];
        let entry_reader = EndianReader::new(entry, order.to_io_byte_order());
        let Some(id) = entry_reader.u16_at(0) else {
            continue;
        };
        let expected_type = match id {
            0x0207 => 2, // Ricoh.pm:462-464 string
            0x0300 => 7, // Ricoh.pm:469-473 undef + trailing-space ValueConv
            _ => continue,
        };
        if entry_reader.u16_at(2) != Some(expected_type) {
            continue;
        }
        let Some(size) = entry_reader.u32_at(4).map(|n| n as usize) else {
            continue;
        };
        let bytes = if size <= 4 {
            &entry[8..8 + size]
        } else {
            let Some(offset) = entry_reader.u32_at(8).map(|n| n as usize) else {
                continue;
            };
            let Some(value_end) = offset.checked_add(size) else {
                continue;
            };
            if offset < end && value_end > 10 {
                continue; // Conservatively refuse Exif.pm:6549's suspect IFD overlap.
            }
            let Some(bytes) = ctx.window().get(offset..value_end) else {
                continue;
            };
            bytes
        };
        let (name, raw, value) = if id == 0x0207 {
            let terminated = bytes.split(|byte| *byte == 0).next().unwrap_or_default();
            let Ok(text) = std::str::from_utf8(terminated) else {
                continue;
            };
            let value = TagValue::new_string(text);
            ("RicohModel", value.clone(), value)
        } else {
            let Ok(text) = std::str::from_utf8(bytes) else {
                continue;
            };
            (
                "RicohMake",
                TagValue::Binary(bytes.to_vec()),
                TagValue::new_string(text.trim_end_matches(' ')),
            )
        };
        rows.push((
            format!("Ricoh:{name}"),
            TagOccurrence {
                id: crate::core::TagId::Numeric(id),
                name: intern(name),
                group0: intern("MakerNotes"),
                group1: intern("Ricoh"),
                group2: Some(intern("Camera")),
                instance: Instance::default(),
                raw: raw.clone(),
                stored: Some(raw),
                value: Some(value.clone()),
                print: Some(value),
                priority: 1,
                is_list: false,
                order: 0,
                origin: Provenance {
                    module: Some("Ricoh"),
                    table: Some("Type2"),
                    byte_range: None,
                },
            },
        ));
    }
    rows
}

/// A separate parser keeps Ricoh::Main's normal fallback from accidentally
/// treating an unmatched or lower-case Make as the source's Ricoh::Type2.
pub struct RicohType2Parser;

impl MakerNoteParser for RicohType2Parser {
    fn manufacturer_name(&self) -> &'static str {
        "Ricoh"
    }

    fn tag_prefix(&self) -> &'static str {
        "Ricoh:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        for (key, row) in parse_ricoh_type2_rows(&MakerNoteContext::detached(data), byte_order) {
            if let Some(TagValue::String(value)) = row.print {
                tags.insert(key, value);
            }
        }
        Ok(())
    }

    fn parse_with_context_and_values_and_session_and_occurrences(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        _model: Option<&str>,
        _session: &mut Session,
        _cond_ctx: &mut Ctx<'_>,
        _tags: &mut HashMap<String, String>,
        _value_forms: &mut HashMap<String, String>,
        occurrences: &mut Vec<(String, TagOccurrence)>,
    ) -> Result<(), String> {
        occurrences.extend(parse_ricoh_type2_rows(ctx, byte_order));
        Ok(())
    }
}

/// Extracts a 16-bit unsigned value from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the value
/// * `byte_order` - Byte order for interpreting multi-byte values
///
/// # Returns
/// The extracted u16 value, or None if the entry doesn't contain exactly one value
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

/// Ricoh MakerNote parser implementation
pub struct RicohParser;

impl Default for RicohParser {
    fn default() -> Self {
        Self::new()
    }
}

impl RicohParser {
    /// Creates a new RicohParser instance
    pub fn new() -> Self {
        RicohParser
    }

    /// Parse a single IFD entry and extract tag value
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
        // Get tag name from registry
        let tag_name = match TAG_REGISTRY.get_tag_name(entry.tag_id) {
            Some(name) => name,
            None => return, // Unknown tag, skip it
        };

        // Extract u16 value for all registered tags
        let value = match extract_u16_value(entry, data, byte_order) {
            Some(v) => v,
            None => return,
        };

        // Format value based on tag type and registered decoders
        let formatted_value = match entry.tag_id {
            // Tags with registry-based decoders (shooting mode, flash mode, white balance)
            0x0005 | 0x000C | 0x001E => TAG_REGISTRY.decode_u16(entry.tag_id, value),

            // Focus mode: manual binary decode
            RICOH_FOCUS_MODE => {
                if value == 0 {
                    "Auto".to_string()
                } else {
                    "Manual".to_string()
                }
            }

            // Numeric tags: ISO, Sharpness
            RICOH_ISO_SETTING | RICOH_SHARPNESS => value.to_string(),

            // Unknown tag handling (shouldn't reach here due to registry check)
            _ => return,
        };

        tags.insert(format!("Ricoh:{}", tag_name), formatted_value);
    }
}

impl MakerNoteParser for RicohParser {
    fn manufacturer_name(&self) -> &'static str {
        "Ricoh"
    }

    fn tag_prefix(&self) -> &'static str {
        "Ricoh:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        data.starts_with(b"RICOH\0") || data.len() >= 2
    }

    fn parse_with_context(
        &self,
        ctx: &crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext<'_>,
        byte_order: ByteOrder,
        _model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // MakerNotes.pm:898-949: every `MakerNoteRicoh*` variant that
        // reaches `Ricoh::Main` (there's a separate `MakerNoteRicohPentax`
        // that goes to `Pentax::Main` instead, dispatched before this parser
        // is even chosen) declares `Start => '$valuePtr + 8'` regardless of
        // which of the condition's alternatives ("Ricoh", "      ", TIFF
        // magic) matched -- so this is unconditional, not a per-signature
        // offset the way Casio's Type1/Type2 split is.
        //
        // The previous check required an exact `b"RICOH\0"` (6 bytes,
        // upper-case, NUL-terminated) prefix. Real payloads don't
        // necessarily look like that: `combined-samples/Ricoh2.jpg` starts
        // "Ricoh\xcf\0\0" (mixed case, no NUL after "Ricoh"), which the
        // check rejected, leaving `ifd_at` at 0 -- the IFD entry-count read
        // then landed on `"Ri"` as a big-endian u16 (0x5269 = 21097),
        // rejected by `parse_ifd_entries`'s `max_entries` bound, so the
        // whole MakerNote produced nothing.
        let ifd_at = 8;
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
        })
    }
}

// ===== Ricoh::ImageInfo (Main 0x1001) and Ricoh::Subdir (Main 0x2001) =====
//
// Both are `SubDirectory`s whose value is out-of-line and addressed
// TIFF-relative (`Ricoh::Main` has no `Base` override), so -- like Casio's
// `PreviewImage`/Type2 extras -- they need `ctx.tiff()` rather than the
// trait's payload-only `parse()`/`parse_entry()`. `Ricoh::ImageInfo`
// (Ricoh.pm:482-514) itself is `ProcessBinaryData`, already carried in full
// by `exiftool_tables::find_table("Ricoh","ImageInfo")`; `Ricoh::Subdir`
// (Ricoh.pm:608-627) is a second, nested TIFF IFD starting 20 bytes into
// its parent entry's value (`Start => '$valuePtr + 20'`, past a
// `"[Ricoh Camera Info]"` header, `ByteOrder => 'BigEndian'` explicitly --
// not inherited from the enclosing directory).

const RICOH_MAIN_IMAGE_INFO: u16 = 0x1001;
const RICOH_MAIN_SUBDIR: u16 = 0x2001;
const RICOH_SUBDIR_MANUFACTURE_DATE1: u16 = 0x0004;
const RICOH_SUBDIR_MANUFACTURE_DATE2: u16 = 0x0005;
/// `Start => '$valuePtr + 20'`: past the 20-byte `"[Ricoh Camera Info]"`
/// header (verified against `Ricoh2.jpg`'s raw bytes: the string is exactly
/// 20 characters).
const RICOH_SUBDIR_HEADER_LEN: usize = 20;

/// Finds one entry by tag id in the IFD at `ifd_offset` inside `tiff`. Same
/// shape as Casio's `find_casio_entry`.
fn find_ricoh_entry(
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

/// Decodes `Ricoh::ImageInfo` (Main 0x1001) into `metadata`: `RicohImageWidth`,
/// `RicohImageHeight` (plain `int16u`, no `PrintConv`) and `RicohDate`
/// (Ricoh.pm:499-511, `ValueConv => 'sprintf("%.2x%.2x:%.2x:%.2x
/// %.2x:%.2x:%.2x", split(' ', $val))'` over 7 raw bytes -- each byte
/// printed as two hex digits, which for the BCD-like values a camera
/// actually writes reads as a plain decimal date/time).
fn parse_ricoh_image_info(
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    let ifd_offset = ctx.payload_offset() + 8;
    let tiff = ctx.tiff();
    let Some(entry) = find_ricoh_entry(tiff, ifd_offset, byte_order, RICOH_MAIN_IMAGE_INFO) else {
        return;
    };
    // The int16u alternative (Ricoh GR's `ExposureProgram`) shares this tag
    // ID; `ImageInfo`'s own Condition is `$format ne "int16u"`
    // (Ricoh.pm:91-93). SHORT is TIFF type 3.
    if entry.field_type == 3 {
        return;
    }
    let offset = entry.value_offset as usize;
    let Some(record) = tiff.get(offset..offset + entry.value_count as usize) else {
        return;
    };
    let reader = EndianReader::new(record, byte_order.to_io_byte_order());
    if let Some(v) = reader.u16_at(0) {
        metadata.insert("Ricoh:RicohImageWidth", TagValue::new_string(v.to_string()));
    }
    if let Some(v) = reader.u16_at(2) {
        metadata.insert(
            "Ricoh:RicohImageHeight",
            TagValue::new_string(v.to_string()),
        );
    }
    if let Some(bytes) = record.get(6..13) {
        let date = format!(
            "{:02x}{:02x}:{:02x}:{:02x} {:02x}:{:02x}:{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6]
        );
        metadata.insert("Ricoh:RicohDate", TagValue::new_string(date));
    }
}

/// Decodes `Ricoh::Subdir` (Main 0x2001) into `metadata`: `ManufactureDate1`
/// (0x0004) and `ManufactureDate2` (0x0005), plain `string[20]` with no
/// `ValueConv`/`PrintConv` -- ExifTool's `ReadValue` truncates a `string[n]`
/// at the first NUL and does nothing else (no trailing-space trim), and
/// neither field contains one on `Ricoh2.jpg`, so the full 20 bytes (date
/// text plus trailing space padding) is the printed value.
///
/// `model` selects which of `Ricoh::Main`'s three `Condition`-guarded
/// alternatives at 0x2001 applies (Ricoh.pm:397-433). Two of them
/// (`RicohSubdir`, `RicohSubdirIFD`) address the nested IFD's own
/// out-of-line values the ordinary TIFF-relative way; the third
/// (`RicohRR1Subdir`, matched only on `Model =~ /^Caplio RR1\b/`) declares
/// `Base => '$start-20'`, i.e. those same offsets are relative to this
/// entry's own (unshifted) value pointer instead. Verified against both
/// corpus files: `Ricoh2.jpg` (a non-RR1 model) needs base 0;
/// `Ricoh.jpg` (`Model: Caplio RR1`) needs base = the 0x2001 entry's raw
/// `value_offset` -- using base 0 for it read `ManufactureDate1` as
/// "RICOH      " (unrelated file bytes 20-ish bytes off from the real
/// string).
fn parse_ricoh_subdir(
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    model: Option<&str>,
    metadata: &mut MetadataMap,
) {
    let ifd_offset = ctx.payload_offset() + 8;
    let tiff = ctx.tiff();
    let Some(entry) = find_ricoh_entry(tiff, ifd_offset, byte_order, RICOH_MAIN_SUBDIR) else {
        return;
    };
    let Some(subdir_ifd_offset) =
        (entry.value_offset as usize).checked_add(RICOH_SUBDIR_HEADER_LEN)
    else {
        return;
    };
    let is_rr1 = model.is_some_and(|m| {
        m.starts_with("Caplio RR1")
            && m["Caplio RR1".len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
    });
    let nested_base: i64 = if is_rr1 {
        i64::from(entry.value_offset)
    } else {
        0
    };
    // Ricoh.pm:608-627: `ByteOrder => 'BigEndian'`, independent of the
    // enclosing Main directory's own byte order.
    let sub_order = ByteOrder::BigEndian;
    for (tag_id, name) in [
        (RICOH_SUBDIR_MANUFACTURE_DATE1, "ManufactureDate1"),
        (RICOH_SUBDIR_MANUFACTURE_DATE2, "ManufactureDate2"),
    ] {
        let Some(entry) = find_ricoh_entry(tiff, subdir_ifd_offset, sub_order, tag_id) else {
            continue;
        };
        let total = entry.value_count as usize;
        let Some(offset) = usize::try_from(nested_base + i64::from(entry.value_offset)).ok() else {
            continue;
        };
        let Some(bytes) = offset
            .checked_add(total)
            .and_then(|end| tiff.get(offset..end))
        else {
            continue;
        };
        let text = match bytes.iter().position(|&b| b == 0) {
            Some(nul) => &bytes[..nul],
            None => bytes,
        };
        metadata.insert(
            format!("Ricoh:{name}"),
            TagValue::new_string(String::from_utf8_lossy(text).into_owned()),
        );
    }
}

/// Extracts `Ricoh::ImageInfo` and `Ricoh::Subdir` tags into `metadata`,
/// when `ctx` holds a Ricoh MakerNote payload.
pub fn parse_ricoh_extra_tags(
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    model: Option<&str>,
    metadata: &mut MetadataMap,
) {
    parse_ricoh_image_info(ctx, byte_order, metadata);
    parse_ricoh_subdir(ctx, byte_order, model, metadata);
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ricoh_parser_trait() {
        let parser = RicohParser::new();
        assert_eq!(parser.manufacturer_name(), "Ricoh");
        assert_eq!(parser.tag_prefix(), "Ricoh:");
    }

    #[test]
    fn test_parse_shooting_mode() {
        let parser = RicohParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x05, 0x00]); // Tag: ShootingMode (0x0005)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Ricoh:ShootingMode"), Some(&"Program".to_string()));
    }

    #[test]
    fn test_parse_focus_mode() {
        let parser = RicohParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x1D, 0x00]); // Tag: FocusMode (0x001D)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (Manual)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(result.is_ok());
        assert_eq!(tags.get("Ricoh:FocusMode"), Some(&"Manual".to_string()));
    }

    /// A headerless WG-M1 Ricoh2 note: two little-endian entries whose count
    /// sits at `count_at` (8 = before the padding, 10 = after it).
    fn wg_m1_little_endian_note(count_at: usize) -> Vec<u8> {
        let mut note = vec![0; 100];
        note[..8].copy_from_slice(b"WG-M1foo");
        note[count_at..count_at + 2].copy_from_slice(&2u16.to_le_bytes());
        note[12..14].copy_from_slice(&0x0207u16.to_le_bytes());
        note[14..16].copy_from_slice(&2u16.to_le_bytes());
        note[16..20].copy_from_slice(&4u32.to_le_bytes());
        note[20..24].copy_from_slice(b"ABCD");
        note[24..26].copy_from_slice(&0x0300u16.to_le_bytes());
        note[26..28].copy_from_slice(&7u16.to_le_bytes());
        note[28..32].copy_from_slice(&8u32.to_le_bytes());
        note[32..36].copy_from_slice(&80u32.to_le_bytes());
        note[80..88].copy_from_slice(b"Make    ");
        note
    }

    fn type2_prints(note: &[u8], inherited: ByteOrder) -> HashMap<String, TagValue> {
        parse_ricoh_type2_rows(&MakerNoteContext::detached(note), inherited)
            .into_iter()
            .filter_map(|(key, row)| row.print.map(|print| (key, print)))
            .collect()
    }

    #[test]
    fn ricoh_type2_resolves_unknown_order_against_a_big_endian_tiff() {
        // MakerNotes.pm:937 `ByteOrder => 'Unknown'`: Exif.pm:6982-6993 reads
        // the int16u at `$valuePtr + 8` in the enclosing order and flips when
        // it is not a plausible entry count (0x0200 read as MM here).
        let prints = type2_prints(&wg_m1_little_endian_note(8), ByteOrder::BigEndian);
        assert_eq!(
            prints.get("Ricoh:RicohModel"),
            Some(&TagValue::String("ABCD".into()))
        );
        assert_eq!(
            prints.get("Ricoh:RicohMake"),
            Some(&TagValue::String("Make".into()))
        );
    }

    #[test]
    fn ricoh_type2_order_check_precedes_the_kodak_patch() {
        // The order test runs before ProcessKodakPatch (MakerNotes.pm:1742)
        // moves the count, so a zero first word keeps the inherited MM order;
        // the patched count 0x0200 then overruns the note and nothing is read.
        let prints = type2_prints(&wg_m1_little_endian_note(10), ByteOrder::BigEndian);
        assert!(prints.is_empty(), "{prints:?}");
    }

    #[test]
    fn test_tag_registry() {
        assert_eq!(TAG_REGISTRY.get_tag_name(0x0005), Some("ShootingMode"));
        assert!(TAG_REGISTRY.has_tag(0x000C));
        assert_eq!(TAG_REGISTRY.get_tag_name(0x001E), Some("WhiteBalance"));
    }
}
