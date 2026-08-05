//! Casio MakerNote parser
//!
//! Parses Casio digital camera-specific EXIF MakerNote tags.
//! Casio was known for the Exilim series of ultra-compact digital cameras
//! with high-speed capture and unique features.
//!
//! ## Supported Cameras
//! - Exilim series (EX-Z, EX-S, EX-F)
//! - QV series (early digital cameras)
//! - GV series (with LCD viewfinder)
//!
//! ## Supported Features
//! - High-speed burst mode settings
//! - Best Shot scene selection
//! - Continuous shooting modes
//! - Image quality and sharpness
//! - Flash and focus settings
//! - Color mode and effects
//! - Digital zoom information
//!
//! ## Tag Structure
//! Casio uses a standard IFD format with manufacturer-specific tag IDs.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::core::formatters::numeric_precision::perl_number;
use crate::core::{MetadataMap, TagValue};
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::registries::casio::casio_registry;
use super::shared::MakerNoteParser;
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// ===== Casio MakerNote Tag IDs =====
// Tag definitions are now centralized in the registry.
// See registries/casio.rs for the complete tag registry.

// Tag ID constants for special tag handling
const CASIO_RECORDING_MODE: u16 = 0x0001;
const CASIO_QUALITY: u16 = 0x0002;
const CASIO_FOCUS_MODE: u16 = 0x0003;
const CASIO_FLASH_MODE: u16 = 0x0004;
const CASIO_FLASH_INTENSITY: u16 = 0x0005;
const CASIO_WHITE_BALANCE: u16 = 0x0007;
const CASIO_DIGITAL_ZOOM: u16 = 0x000A;
const CASIO_SHARPNESS: u16 = 0x000B;
const CASIO_CONTRAST: u16 = 0x000C;
const CASIO_SATURATION: u16 = 0x000D;
const CASIO_OBJECT_DISTANCE: u16 = 0x0006;
const CASIO_CCD_SENSITIVITY: u16 = 0x0014;
const CASIO_COLOR_MODE: u16 = 0x0015;
const CASIO_ENHANCEMENT: u16 = 0x0016;
const CASIO_CONTINUOUS_MODE: u16 = 0x001A;
const CASIO_BEST_SHOT_MODE: u16 = 0x001B;
const CASIO_SLOW_SHUTTER: u16 = 0x0020;

// Static registry instance for efficient tag lookup and decoding
static TAG_REGISTRY: Lazy<TagRegistry> = Lazy::new(casio_registry);

/// Extracts a 16-bit unsigned value from IFD entry
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

/// Casio MakerNote parser implementation
pub struct CasioParser;

impl Default for CasioParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CasioParser {
    /// Creates a new Casio parser instance
    pub fn new() -> Self {
        CasioParser
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

        // Extract and format the value based on tag type
        let formatted_value = match entry.tag_id {
            // Binary on/off tags
            CASIO_CONTINUOUS_MODE | CASIO_SLOW_SHUTTER => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    if value > 0 {
                        "On".to_string()
                    } else {
                        "Off".to_string()
                    }
                } else {
                    return;
                }
            }
            // Casio.pm:32-46 (Main::0x0001) -- PrintConv map. #4-suffixed
            // entries (7/10/15/16) are alternate raw values ExifTool folds
            // into the same three names as the primary 2/3/4 entries.
            CASIO_RECORDING_MODE => {
                let Some(value) = extract_u16_value(entry, data, byte_order) else {
                    return;
                };
                match value {
                    1 => "Single Shutter".to_string(),
                    2 => "Panorama".to_string(),
                    3 => "Night Scene".to_string(),
                    4 => "Portrait".to_string(),
                    5 => "Landscape".to_string(),
                    7 => "Panorama".to_string(),
                    10 => "Night Scene".to_string(),
                    15 => "Portrait".to_string(),
                    16 => "Landscape".to_string(),
                    other => other.to_string(),
                }
            }
            // Casio.pm:98-105 (Main::0x0006) -- int32u, ValueConv '$val /
            // 1000', PrintConv '"$val m"'. Count is 1 so the whole 4-byte
            // value lives directly in `value_offset` (already read in this
            // entry's byte order by the shared IFD-entry reader).
            CASIO_OBJECT_DISTANCE => {
                if entry.value_count != 1 {
                    return;
                }
                format!("{} m", perl_number(entry.value_offset as f64 / 1000.0))
            }
            // All other tags use raw value as string
            _ => {
                if let Some(value) = extract_u16_value(entry, data, byte_order) {
                    value.to_string()
                } else {
                    return;
                }
            }
        };

        tags.insert(format!("Casio:{}", tag_name), formatted_value);
    }
}

impl MakerNoteParser for CasioParser {
    fn manufacturer_name(&self) -> &'static str {
        "Casio"
    }

    fn tag_prefix(&self) -> &'static str {
        "Casio:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // `MakerNoteCasio2` ("QVC\0"/"DCI\0"-signed, `Casio::Type2`) is a
        // completely different tag table from `Casio::Main` (headerless
        // "Type1") this registry/TAG_REGISTRY was built for -- its own 0x0002
        // is `PreviewImageSize`, not Main's 0x0002 `Quality`, for one. Walking
        // it here under Type1's tag names would print a real ExifTool tag
        // name next to a value read under the wrong table's meaning, which is
        // worse than omitting it. `parse_casio_type2_extra_tags`
        // (`tiff_helpers.rs`) covers this table's tags this crate actually
        // implements, reading against the full TIFF block Type2's
        // out-of-line values need; nothing else in `Casio::Type2` is
        // extracted yet.
        if data.starts_with(b"QVC\0") || data.starts_with(b"DCI\0") {
            return Ok(());
        }

        // Casio MakerNotes typically start immediately with IFD entries
        // No header is used, so signature is None
        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 500,
        };

        // Parse IFD entries using the shared parser
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.parse_entry(entry, parse_data, byte_order, tags);
        })
    }
}

// ===== Casio Type2 PreviewImage (0x2000) =====
//
// `Casio::Type2` (the "QVC\0"/"DCI\0"-signed MakerNote,
// `MakerNotes.pm:81-91`, `Start => '$valuePtr + 6'`) declares its
// `PreviewImage` at 0x2000 via `%Image::ExifTool::previewImageTagInfo`
// (`Casio.pm:402-407`, itself `ExifTool.pm:1268-1280`): a direct inline
// value (`RawConv => 'ValidateImage(...)'`, no `IsOffset`/`OffsetPair`),
// unlike the offset-pair mechanism Olympus/Minolta use. `Casio::Main` (the
// older, header-less "Type1" table `MakerNoteCasio` routes to) has no 0x2000
// entry at all, so this only ever fires for a Type2 payload.
//
// `ValidateImage` only rejects a bad-magic value when the tag was
// specifically requested (`$$self{REQ_TAG_LOOKUP}`, `ExifTool.pm:6425`) --
// irrelevant to a full default dump, which is what oxidex's `-j -e` output
// and the tag-comparison harness both drive. So the only failure mode that
// matters here is the declared value running past the end of the file, which
// `ExifTool.pm`'s `ExtractBinary` (~9832) answers with the declared-length
// placeholder string rather than an omission -- the same contract Task 1
// established for `IFD2:PreviewImage` and Task 3 for Sony's 0x2001, verified
// below against `Casio2.jpg`.
//
// `MakerNoteCasio2` has no `Base` override, so 0x2000's value offset is
// TIFF-relative like Sigma's and Sony's -- read directly against `ctx.tiff()`.

/// `Start => '$valuePtr + 6'`: past the 4-byte "QVC\0"/"DCI\0" signature and
/// a 2-byte pad, the IFD itself begins.
const CASIO_TYPE2_IFD_START: usize = 6;
const CASIO_PREVIEW_IMAGE: u16 = 0x2000;

/// TIFF field type byte widths this decoder needs (a subset of the full TIFF
/// type table; `PreviewImage` is always `undef`/type 7, but this stays
/// general the way Sony's `type_size` does).
fn casio_type_size(field_type: u16) -> Option<usize> {
    Some(match field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

/// Finds one entry by tag id in the IFD at `ifd_offset` inside `tiff`.
fn find_casio_entry(
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

/// Extracts Casio Type2's `PreviewImage` (0x2000) into `metadata`, when `ctx`
/// holds a "QVC\0"/"DCI\0"-signed payload. A no-op for a Type1 ("Main")
/// payload, which has no 0x2000 tag to find.
///
/// See the module doc comment above for the source citations this follows.
pub fn parse_casio_preview_image_tag(
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    let payload = ctx.payload();
    if !(payload.starts_with(b"QVC\0") || payload.starts_with(b"DCI\0")) {
        return;
    }

    let ifd_offset = ctx.payload_offset() + CASIO_TYPE2_IFD_START;
    let tiff = ctx.tiff();
    let Some(entry) = find_casio_entry(tiff, ifd_offset, byte_order, CASIO_PREVIEW_IMAGE) else {
        return;
    };
    let Some(elem_size) = casio_type_size(entry.field_type) else {
        return;
    };
    let Some(total) = elem_size.checked_mul(entry.value_count as usize) else {
        return;
    };
    // A value this small lives inline in the entry itself, never a JPEG
    // preview; ExifTool only ever addresses this tag out-of-line.
    if total <= 4 {
        return;
    }

    let offset = entry.value_offset as usize;
    match offset
        .checked_add(total)
        .and_then(|end| tiff.get(offset..end))
    {
        Some(bytes) => {
            metadata.insert(
                "MakerNotes:PreviewImage",
                TagValue::new_binary(bytes.to_vec()),
            );
        }
        None => {
            metadata.insert(
                "MakerNotes:PreviewImage",
                TagValue::new_string(format!(
                    "(Binary data {total} bytes, use -b option to extract)"
                )),
            );
        }
    }
}

// ===== Casio Type2 extra tags (PreviewImageSize, FlashDistance,
// HometownCity, FirmwareDate) =====
//
// Like 0x2000 above, these need `ctx.tiff()` rather than the trait's
// payload-only `parse()`: `FirmwareDate` (0x2001, `undef[18]`) and
// `HometownCity` (0x3006, `string[24]`) are both stored out-of-line
// (`Casio.pm:405-436,576-579`), and their value offsets are TIFF-relative
// like `PreviewImage`'s (no `Base` override on `MakerNoteCasio2`). Folding
// them into the ordinary IFD walk in `parse()` would only see the declared
// MakerNote block, which -- per `MakerNoteContext`'s module doc -- routinely
// doesn't reach an out-of-line value.

const CASIO_TYPE2_PREVIEW_IMAGE_SIZE: u16 = 0x0002;
const CASIO_TYPE2_FIRMWARE_DATE: u16 = 0x2001;
const CASIO_TYPE2_FLASH_DISTANCE: u16 = 0x2034;
const CASIO_TYPE2_HOMETOWN_CITY: u16 = 0x3006;

/// Unpacks the two `int16u` values `PreviewImageSize` (`Casio.pm:280-286`)
/// packs into one 4-byte inline entry, in the entry's own byte order.
/// `extract_u16_value` (used by the Type1/Main path above) already does this
/// for the first half; this is that plus the second.
fn casio_u16_pair(entry: &IfdEntry, byte_order: ByteOrder) -> (u16, u16) {
    match byte_order {
        ByteOrder::LittleEndian => (
            (entry.value_offset & 0xFFFF) as u16,
            ((entry.value_offset >> 16) & 0xFFFF) as u16,
        ),
        ByteOrder::BigEndian => (
            ((entry.value_offset >> 16) & 0xFFFF) as u16,
            (entry.value_offset & 0xFFFF) as u16,
        ),
    }
}

/// `Casio.pm:411-436` (Type2::0x2001 `FirmwareDate`): an 18-byte fixed
/// `YYMM\0\0DDHH\0\0mm\0\0\0\0` ASCII-digit layout (no seconds field --
/// that's Main::0x0015's variant, one byte longer in its no-seconds-omitted
/// form). Falls back to ExifTool's `Unknown (...)` form, nulls rendered as
/// `.` with trailing dots stripped, when the bytes don't match.
fn casio_firmware_date_type2(bytes: &[u8]) -> String {
    fn digit2(b: &[u8]) -> Option<&str> {
        if b.len() == 2 && b[0].is_ascii_digit() && b[1].is_ascii_digit() {
            std::str::from_utf8(b).ok()
        } else {
            None
        }
    }
    if bytes.len() == 18
        && bytes[4] == 0
        && bytes[5] == 0
        && bytes[10] == 0
        && bytes[11] == 0
        && bytes[14..18] == [0, 0, 0, 0]
        && let (Some(yy), Some(mo), Some(dd), Some(hh), Some(mi)) = (
            digit2(&bytes[0..2]),
            digit2(&bytes[2..4]),
            digit2(&bytes[6..8]),
            digit2(&bytes[8..10]),
            digit2(&bytes[12..14]),
        )
    {
        let yy_val: u32 = yy.parse().unwrap_or(0);
        let year = if yy_val < 70 { 2000 + yy_val } else { 1900 + yy_val };
        return format!("{year}:{mo}:{dd} {hh}:{mi}");
    }
    let mut unknown: String = bytes
        .iter()
        .map(|&b| if b == 0 { '.' } else { b as char })
        .collect();
    while unknown.ends_with('.') {
        unknown.pop();
    }
    format!("Unknown ({unknown})")
}

/// Extracts Casio Type2's `PreviewImageSize`, `FlashDistance`,
/// `HometownCity` and `FirmwareDate` into `metadata`, when `ctx` holds a
/// "QVC\0"/"DCI\0"-signed payload. A no-op for a Type1 ("Main") payload or
/// when a given tag is absent from this particular file's IFD.
pub fn parse_casio_type2_extra_tags(
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    let payload = ctx.payload();
    if !(payload.starts_with(b"QVC\0") || payload.starts_with(b"DCI\0")) {
        return;
    }

    let ifd_offset = ctx.payload_offset() + CASIO_TYPE2_IFD_START;
    let tiff = ctx.tiff();

    if let Some(entry) =
        find_casio_entry(tiff, ifd_offset, byte_order, CASIO_TYPE2_PREVIEW_IMAGE_SIZE)
        && entry.value_count == 2
    {
        let (w, h) = casio_u16_pair(&entry, byte_order);
        metadata.insert("Casio:PreviewImageSize", TagValue::new_string(format!("{w}x{h}")));
    }

    if let Some(entry) =
        find_casio_entry(tiff, ifd_offset, byte_order, CASIO_TYPE2_FLASH_DISTANCE)
        && let Some(value) = extract_u16_value(&entry, &[], byte_order)
    {
        metadata.insert("Casio:FlashDistance", TagValue::new_string(value.to_string()));
    }

    if let Some(entry) =
        find_casio_entry(tiff, ifd_offset, byte_order, CASIO_TYPE2_HOMETOWN_CITY)
    {
        let total = entry.value_count as usize;
        let offset = entry.value_offset as usize;
        if let Some(bytes) = offset.checked_add(total).and_then(|end| tiff.get(offset..end)) {
            let text = match bytes.iter().position(|&b| b == 0) {
                Some(nul) => &bytes[..nul],
                None => bytes,
            };
            metadata.insert(
                "Casio:HometownCity",
                TagValue::new_string(String::from_utf8_lossy(text).into_owned()),
            );
        }
    }

    if let Some(entry) =
        find_casio_entry(tiff, ifd_offset, byte_order, CASIO_TYPE2_FIRMWARE_DATE)
    {
        let total = entry.value_count as usize;
        let offset = entry.value_offset as usize;
        if let Some(bytes) = offset.checked_add(total).and_then(|end| tiff.get(offset..end)) {
            metadata.insert(
                "Casio:FirmwareDate",
                TagValue::new_string(casio_firmware_date_type2(bytes)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_casio_parser_trait() {
        let parser = CasioParser::new();
        assert_eq!(parser.manufacturer_name(), "Casio");
        assert_eq!(parser.tag_prefix(), "Casio:");
    }

    #[test]
    fn test_parse_quality_tag() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x02, 0x00]); // Tag: CASIO_QUALITY (0x0002)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x02, 0x00, 0x00, 0x00]); // Value: 2 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:Quality"), Some(&"2".to_string()));
    }

    #[test]
    fn test_parse_focus_mode_tag() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x03, 0x00]); // Tag: CASIO_FOCUS_MODE (0x0003)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:FocusMode"), Some(&"1".to_string()));
    }

    #[test]
    fn test_parse_continuous_mode_on() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x1A, 0x00]); // Tag: CASIO_CONTINUOUS_MODE (0x001A)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Value: 1

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:ContinuousMode"), Some(&"On".to_string()));
    }

    #[test]
    fn test_parse_continuous_mode_off() {
        let parser = CasioParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&[0x1A, 0x00]); // Tag: CASIO_CONTINUOUS_MODE (0x001A)
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Value: 0

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(tags.get("Casio:ContinuousMode"), Some(&"Off".to_string()));
    }
}

#[cfg(test)]
mod casio_preview_image_tests {
    use super::*;
    use crate::core::MetadataMap;

    /// Builds a synthetic TIFF block holding a Casio Type2 ("QVC\0"-signed)
    /// MakerNote at `payload_offset`, one IFD entry (0x2000, undef,
    /// `preview_len` declared bytes) whose value offset is TIFF-relative, and
    /// (when `place_bytes` is `Some`) the real preview bytes at that offset.
    fn build_tiff_with_casio_type2_preview(
        payload_offset: usize,
        preview_len: u32,
        value_offset: u32,
        place_bytes: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut tiff = vec![0u8; payload_offset];
        tiff[0..2].copy_from_slice(b"II");

        let mut payload = Vec::new();
        payload.extend_from_slice(b"QVC\0"); // 4-byte signature
        payload.extend_from_slice(&[0u8, 0u8]); // 2-byte pad -> IFD at +6
        payload.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
        payload.extend_from_slice(&CASIO_PREVIEW_IMAGE.to_le_bytes()); // tag id
        payload.extend_from_slice(&7u16.to_le_bytes()); // type: undef
        payload.extend_from_slice(&preview_len.to_le_bytes()); // count (bytes)
        payload.extend_from_slice(&value_offset.to_le_bytes()); // value offset
        payload.extend_from_slice(&0u32.to_le_bytes()); // next IFD offset

        tiff.extend_from_slice(&payload);

        if let Some(bytes) = place_bytes {
            let end = value_offset as usize + bytes.len();
            if tiff.len() < end {
                tiff.resize(end, 0);
            }
            tiff[value_offset as usize..end].copy_from_slice(bytes);
        }
        tiff
    }

    #[test]
    fn casio_type2_preview_image_in_bounds_becomes_binary() {
        let payload_offset = 20usize;
        let preview_bytes: Vec<u8> = (0..26u8).collect();
        let value_offset = 100u32;
        let tiff = build_tiff_with_casio_type2_preview(
            payload_offset,
            preview_bytes.len() as u32,
            value_offset,
            Some(&preview_bytes),
        );
        let payload_len = tiff.len() - payload_offset;
        let ctx = MakerNoteContext::in_tiff(&tiff, payload_offset, payload_len, 12);

        let mut metadata = MetadataMap::new();
        parse_casio_preview_image_tag(&ctx, ByteOrder::LittleEndian, &mut metadata);

        assert_eq!(
            metadata.get("MakerNotes:PreviewImage"),
            Some(&TagValue::new_binary(preview_bytes))
        );
    }

    #[test]
    fn casio_type2_preview_image_out_of_bounds_shows_placeholder_not_omission() {
        let payload_offset = 20usize;
        // Declares 895146 bytes (an arbitrary large, real-shaped declared
        // length) at an offset that runs past the end of the buffer -- the
        // same class of truncated-corpus case Task 1/3/4 verified.
        let tiff = build_tiff_with_casio_type2_preview(payload_offset, 895146, 100, None);
        let payload_len = tiff.len() - payload_offset;
        let ctx = MakerNoteContext::in_tiff(&tiff, payload_offset, payload_len, 12);

        let mut metadata = MetadataMap::new();
        parse_casio_preview_image_tag(&ctx, ByteOrder::LittleEndian, &mut metadata);

        assert_eq!(
            metadata.get("MakerNotes:PreviewImage"),
            Some(&TagValue::new_string(
                "(Binary data 895146 bytes, use -b option to extract)"
            ))
        );
    }

    #[test]
    fn casio_type1_payload_is_a_no_op() {
        // Type1 ("Main") MakerNotes have no signature and no 0x2000 tag at
        // all -- this should never fire for one.
        let payload = vec![0u8; 16];
        let ctx = MakerNoteContext::detached(&payload);
        let mut metadata = MetadataMap::new();
        parse_casio_preview_image_tag(&ctx, ByteOrder::LittleEndian, &mut metadata);
        assert_eq!(metadata.get("MakerNotes:PreviewImage"), None);
    }
}
