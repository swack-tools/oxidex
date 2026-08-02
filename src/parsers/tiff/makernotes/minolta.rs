//! Minolta MakerNote parser
//!
//! Parses Minolta (and early Konica Minolta) camera-specific EXIF MakerNote
//! tags. Minolta was a major camera manufacturer from 1985-2006, later merged
//! with Konica before Sony acquired the camera division in 2006 - which is why
//! this table is reachable two ways: directly, from a Minolta body, and as a
//! sub-directory of the Sony MakerNote on the DSLR-A100.
//!
//! Like Sony's, a Minolta MakerNote is an IFD whose entry offsets are relative
//! to the TIFF header, so anything longer than four bytes needs `data_base` to
//! be resolvable at all.
//!
//! Four of its sub-directories are decoded only for the DSLR-A100, whose
//! layout no other body shares; their tables live in
//! [`super::minolta_a100_tables`] and are read by the same binary-data
//! interpreter Sony's enciphered blocks use.

use crate::core::{MetadataMap, TagValue};
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
use std::collections::HashMap;

use super::minolta_a100_tables as a100;
use super::minolta_lens_database::lookup_minolta_lens;
use super::minolta_tables::CAMERA_SETTINGS;
use super::shared::MakerNoteParser;
use super::shared::print_im::decode_print_im_from_ifd;
use super::sony::binary::{lookup, print_float, unknown, unknown_hex};
use super::sony::binary_data;
use super::sony::value::SonyValue;

// ============================================================================
// Main table PrintConv hashes (Image::ExifTool::Minolta::Main)
// ============================================================================

static SCENE_MODE: &[(i64, &str)] = &[
    (0, "Standard"),
    (1, "Portrait"),
    (2, "Text"),
    (3, "Night Scene"),
    (4, "Sunset"),
    (5, "Sports"),
    (6, "Landscape"),
    (7, "Night Portrait"),
    (8, "Macro"),
    (9, "Super Macro"),
    (16, "Auto"),
    (17, "Night View/Portrait"),
    (18, "Sweep Panorama"),
    (19, "Handheld Night Shot"),
    (20, "Anti Motion Blur"),
    (21, "Cont. Priority AE"),
    (22, "Auto+"),
    (23, "3D Sweep Panorama"),
    (24, "Superior Auto"),
    (25, "High Sensitivity"),
    (26, "Fireworks"),
    (27, "Food"),
    (28, "Pet"),
    (33, "HDR"),
    (65535, "n/a"),
];

/// `ColorMode` (0x0101) as read on a Minolta body. Sony bodies take a
/// different list, but they reach this table only through the DSLR-A100's
/// nested MakerNote, where 0x0101 is not written.
static COLOR_MODE: &[(i64, &str)] = &[
    (0, "Natural color"),
    (1, "Black & White"),
    (2, "Vivid color"),
    (3, "Solarization"),
    (4, "Adobe RGB"),
    (5, "Sepia"),
    (9, "Natural"),
    (12, "Portrait"),
    (13, "Natural sRGB"),
    (14, "Natural+ sRGB"),
    (15, "Landscape"),
    (16, "Evening"),
    (17, "Night Scene"),
    (18, "Night Portrait"),
    (132, "Embed Adobe RGB"),
];

static MINOLTA_QUALITY: &[(i64, &str)] = &[
    (0, "Raw"),
    (1, "Super Fine"),
    (2, "Fine"),
    (3, "Standard"),
    (4, "Economy"),
    (5, "Extra fine"),
];

static TELECONVERTER: &[(i64, &str)] = &[
    (0, "None"),
    (4, "Minolta/Sony AF 1.4x APO (D) (0x04)"),
    (5, "Minolta/Sony AF 2x APO (D) (0x05)"),
    (72, "Minolta/Sony AF 2x APO (D)"),
    (80, "Minolta AF 2x APO II"),
    (96, "Minolta AF 2x APO"),
    (136, "Minolta/Sony AF 1.4x APO (D)"),
    (144, "Minolta AF 1.4x APO II"),
    (160, "Minolta AF 1.4x APO"),
];

static IMAGE_STABILIZATION_0107: &[(i64, &str)] = &[(1, "Off"), (5, "On")];

static RAW_AND_JPG_RECORDING: &[(i64, &str)] = &[(0, "Off"), (1, "On")];

static ZONE_MATCHING: &[(i64, &str)] = &[(0, "ISO Setting Used"), (1, "High Key"), (2, "Low Key")];

static IMAGE_STABILIZATION_A100: &[(i64, &str)] = &[(0, "Off"), (1, "On")];

static WHITE_BALANCE_0115: &[(i64, &str)] = &[
    (0, "Auto"),
    (1, "Color Temperature/Color Filter"),
    (16, "Daylight"),
    (32, "Cloudy"),
    (48, "Shade"),
    (64, "Tungsten"),
    (80, "Flash"),
    (96, "Fluorescent"),
    (112, "Custom"),
];

// ============================================================================
// Main table
// ============================================================================

/// How a Minolta `Main` entry prints.
enum Print {
    /// The integer itself.
    Int,
    /// `PrintConv` hash; misses print `Unknown (N)`.
    Map(&'static [(i64, &'static str)]),
    /// The same, `PrintHex`.
    MapHex(&'static [(i64, &'static str)]),
    /// A rational, printed as a plain number.
    Rational,
    /// An ASCII/undef string.
    Text,
    /// A 32-bit value reinterpreted as signed.
    Signed32,
    /// A lens id resolved through the shared Minolta/Sony lens table.
    LensType,
}

struct MainTag {
    id: u16,
    name: &'static str,
    print: Print,
}

/// `Image::ExifTool::Minolta::Main`, restricted to the scalar tags.
///
/// Deliberately absent:
/// * 0x0081 `PreviewImage` and 0x0088 `PreviewImageStart` - the preview lives
///   outside the MakerNote, and ExifTool absolutises the start offset by
///   adding the TIFF header's file position, which a MakerNote parser cannot
///   see. Reporting the stored 13030 where ExifTool reports 13042 would be a
///   wrong value rather than a missing one.
/// * 0x0103 `MinoltaQuality`/`MinoltaImageSize` - model-conditional in a way
///   none of the corpus files exercise.
/// * The sub-directory tags 0x0001/0x0003/0x0004 (`CameraSettings` variants),
///   0x0010/0x0018/0x0020 (the A100 blocks) and 0x0114 - handled separately or
///   not yet decoded.
static MAIN_TABLE: &[MainTag] = &[
    MainTag {
        id: 0x0000,
        name: "MakerNoteVersion",
        print: Print::Text,
    },
    MainTag {
        id: 0x0040,
        name: "CompressedImageSize",
        print: Print::Int,
    },
    MainTag {
        id: 0x0089,
        name: "PreviewImageLength",
        print: Print::Int,
    },
    MainTag {
        id: 0x0100,
        name: "SceneMode",
        print: Print::Map(SCENE_MODE),
    },
    MainTag {
        id: 0x0101,
        name: "ColorMode",
        print: Print::Map(COLOR_MODE),
    },
    MainTag {
        id: 0x0102,
        name: "MinoltaQuality",
        print: Print::Map(MINOLTA_QUALITY),
    },
    MainTag {
        id: 0x0104,
        name: "FlashExposureComp",
        print: Print::Rational,
    },
    MainTag {
        id: 0x0105,
        name: "Teleconverter",
        print: Print::MapHex(TELECONVERTER),
    },
    MainTag {
        id: 0x0107,
        name: "ImageStabilization",
        print: Print::Map(IMAGE_STABILIZATION_0107),
    },
    MainTag {
        id: 0x0109,
        name: "RawAndJpgRecording",
        print: Print::Map(RAW_AND_JPG_RECORDING),
    },
    MainTag {
        id: 0x010a,
        name: "ZoneMatching",
        print: Print::Map(ZONE_MATCHING),
    },
    MainTag {
        id: 0x010b,
        name: "ColorTemperature",
        print: Print::Int,
    },
    MainTag {
        id: 0x010c,
        name: "LensType",
        print: Print::LensType,
    },
    MainTag {
        id: 0x0111,
        name: "ColorCompensationFilter",
        print: Print::Int,
    },
    MainTag {
        id: 0x0112,
        name: "WhiteBalanceFineTune",
        print: Print::Signed32,
    },
    MainTag {
        id: 0x0113,
        name: "ImageStabilization",
        print: Print::Map(IMAGE_STABILIZATION_A100),
    },
    MainTag {
        id: 0x0115,
        name: "WhiteBalance",
        print: Print::MapHex(WHITE_BALANCE_0115),
    },
];

/// The two tag ids that carry a `CameraSettings` block.
const TAG_CAMERA_SETTINGS_OLD: u16 = 0x0001;
const TAG_CAMERA_SETTINGS: u16 = 0x0003;

/// The four `Minolta::Main` sub-directories ExifTool decodes only for the Sony
/// DSLR-A100, with the byte order each one declares.
///
/// All four are gated on `$$self{Model} eq "DSLR-A100"`; nothing else writes
/// them in a shape these tables describe, so decoding them on another body
/// would produce confident nonsense rather than nothing.
const A100_SUBDIRS: &[(u16, usize, ByteOrder)] = &[
    (0x0010, a100::idx::CAMERAINFOA100, ByteOrder::LittleEndian),
    (0x0018, a100::idx::ISINFOA100, ByteOrder::BigEndian),
    (0x0020, a100::idx::WBINFOA100, ByteOrder::BigEndian),
    (0x0114, a100::idx::CAMERASETTINGSA100, ByteOrder::BigEndian),
];

fn render(print: &Print, value: &SonyValue<'_>) -> Option<String> {
    match print {
        Print::Int => value.first_int().map(|v| v.to_string()),
        Print::Map(m) => {
            let raw = value.first_int()?;
            Some(lookup(m, raw).unwrap_or_else(|| unknown(raw)))
        }
        Print::MapHex(m) => {
            let raw = value.first_int()?;
            Some(lookup(m, raw).unwrap_or_else(|| unknown_hex(raw)))
        }
        Print::Rational => value.rational(0).map(print_float),
        Print::Text => value.string(),
        Print::Signed32 => value.first_int_as::<i32>().map(|v| v.to_string()),
        Print::LensType => {
            let raw = value.first_int()?;
            Some(lookup_minolta_lens(u16::try_from(raw).ok()?).unwrap_or_else(|| unknown(raw)))
        }
    }
}

/// Parses a Minolta MakerNote IFD found at `ifd_index` inside `data`.
///
/// `data_base` is the TIFF-relative offset of `data[0]`, the same convention
/// [`MakerNoteParser::parse_with_context`] uses. Returns the tags in two
/// tiers: the `Main` table's, which carry ExifTool's default priority, and the
/// `CameraSettings` table's, which is `PRIORITY => 0`.
///
/// This is the shared entry point for a standalone Minolta MakerNote and for
/// the one the Sony DSLR-A100 nests inside its own.
pub fn parse_minolta_ifd(
    data: &[u8],
    ifd_index: usize,
    byte_order: ByteOrder,
    data_base: Option<u32>,
    sony_host: bool,
    model: Option<&str>,
) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let mut main = Vec::new();
    let mut sub_dir = Vec::new();
    let is_a100 = model == Some("DSLR-A100");
    let mut a100_ctx = binary_data::Ctx::new(model, None);

    let Some(ifd) = data.get(ifd_index..) else {
        return (main, sub_dir);
    };
    if ifd.len() < 2 {
        return (main, sub_dir);
    }
    let reader = crate::io::EndianReader::new(ifd, byte_order.to_io_byte_order());
    let Some(count) = reader.u16_at(0) else {
        return (main, sub_dir);
    };
    if count == 0 || count > 200 {
        return (main, sub_dir);
    }

    for i in 0..count as usize {
        let base = 2 + i * 12;
        let (Some(tag_id), Some(field_type), Some(value_count), Some(value_offset)) = (
            reader.u16_at(base),
            reader.u16_at(base + 2),
            reader.u32_at(base + 4),
            reader.u32_at(base + 8),
        ) else {
            break;
        };
        let entry = IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        };
        let Some(value) = resolve(data, &entry, byte_order, data_base) else {
            continue;
        };

        if is_a100
            && let Some((_, table, order)) = A100_SUBDIRS.iter().find(|(id, _, _)| *id == tag_id)
        {
            let mut found = Vec::new();
            binary_data::process(
                a100::TABLES,
                *table,
                value.bytes(),
                *order,
                &mut a100_ctx,
                &mut found,
            );
            // All four tables are PRIORITY => 0, the same tier CameraSettings
            // reports under.
            sub_dir.extend(
                found
                    .into_iter()
                    .map(|f| (format!("Minolta:{}", f.name), f.value)),
            );
            continue;
        }

        if matches!(tag_id, TAG_CAMERA_SETTINGS_OLD | TAG_CAMERA_SETTINGS) {
            let mut tags = HashMap::new();
            CAMERA_SETTINGS.extract(value.bytes(), "Minolta", &mut tags);
            sub_dir.extend(tags);
            continue;
        }

        // ExifTool switches 0x0101's PrintConv on the *Make*: a Sony body
        // reading this table through the DSLR-A100's nested MakerNote gets the
        // Sony ColorMode list, not Minolta's. Sony's own 0xb029 supplies that
        // value, so leave it to the host rather than print the wrong list.
        if sony_host && tag_id == 0x0101 {
            continue;
        }

        if let Some(tag) = MAIN_TABLE.iter().find(|t| t.id == tag_id)
            && let Some(printed) = render(&tag.print, &value)
        {
            main.push((format!("Minolta:{}", tag.name), printed));
        }
    }

    (main, sub_dir)
}

/// Resolves one IFD entry to its bytes, inline or via `data_base`.
fn resolve<'a>(
    data: &'a [u8],
    entry: &IfdEntry,
    byte_order: ByteOrder,
    data_base: Option<u32>,
) -> Option<SonyValue<'a>> {
    let size = match entry.field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    };
    let total = size * entry.value_count as usize;
    if total <= 4 {
        let inline = match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };
        return Some(SonyValue::new(
            entry.field_type,
            entry.value_count,
            inline[..total].to_vec(),
            byte_order,
        ));
    }
    let index = entry.value_offset.checked_sub(data_base?)? as usize;
    let bytes = data.get(index..index.checked_add(total)?)?;
    Some(SonyValue::new(
        entry.field_type,
        entry.value_count,
        bytes,
        byte_order,
    ))
}

/// Minolta MakerNote parser implementation
pub struct MinoltaParser;

impl Default for MinoltaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MinoltaParser {
    /// Creates a new Minolta parser instance
    pub fn new() -> Self {
        MinoltaParser
    }
}

impl MakerNoteParser for MinoltaParser {
    fn manufacturer_name(&self) -> &'static str {
        "Minolta"
    }

    fn tag_prefix(&self) -> &'static str {
        "Minolta:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        self.parse_with_context(
            &crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext::detached(data),
            byte_order,
            None,
            tags,
        )
    }

    fn parse_with_context(
        &self,
        ctx: &crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext<'_>,
        byte_order: ByteOrder,
        _model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // See `SonyParser::parse_with_context`: `payload_tiff_offset` is the
        // `data_base` an entry's TIFF-relative offset is measured against, and
        // is `None` rather than 0 when there is no enclosing block.
        let data = ctx.payload();
        let data_base = ctx.payload_tiff_offset();
        if data.len() < 2 {
            return Err("Minolta MakerNote data too short".to_string());
        }
        // A Minolta MakerNote has no header: the IFD starts at byte 0.
        let (main, sub_dir) = parse_minolta_ifd(data, 0, byte_order, data_base, false, _model);

        // ExifTool prefers the higher-priority Main entry when both tables
        // define a name, and the first-extracted copy among equals.
        for (key, value) in sub_dir.into_iter().chain(main) {
            tags.insert(key, value);
        }
        if let Some(version) = decode_print_im_from_ifd(ctx, 0, byte_order) {
            tags.insert("PrintIM:PrintIMVersion".to_string(), version);
        }
        Ok(())
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_minolta_lens(lens_id)
    }
}

// ============================================================================
// PreviewImage (0x0081 direct value, or 0x0088/0x0089 offset pair)
// ============================================================================
//
// `Minolta::Main` (Minolta.pm:691-968) declares `PreviewImage` two ways,
// and a real body writes at most one:
//
// * 0x0081 (Minolta.pm:765-770): `%Image::ExifTool::previewImageTagInfo`
//   directly -- "JPEG preview found in DiMAGE 7 images", `Groups => { 2 =>
//   'Preview' }`, `Permanent => 1`. A direct inline value, TIFF-relative like
//   Sigma's/Casio's, no `IsOffset`/`OffsetPair`.
// * 0x0088/0x0089 (Minolta.pm:771-789): plain `PreviewImageStart`
//   (`IsOffset`, `OffsetPair => 0x0089`) / `PreviewImageLength`
//   (`OffsetPair => 0x0088`), both `DataTag => 'PreviewImage'`. Neither tag
//   is itself named `PreviewImage`; the value comes from `Exif.pm`'s generic
//   `%Image::ExifTool::Exif::Composite::PreviewImage` (Exif.pm:5018-5057),
//   which requires *any* `PreviewImageStart`+`PreviewImageLength` pair by
//   name and calls `ExtractImage`. Verified against `Minolta.jpg`
//   (`DiMAGE 7i`): its MakerNote carries 0x0088/0x0089 only (raw stored
//   start 13030, `exiftool -v3` shows `Tag 0x0088`/`0x0089`), and the
//   composite's absolutised display (13042) is `13030 + tiff_base(12)` --
//   the same TIFF-relative addressing `MAIN_TABLE`'s doc comment already
//   established for why 0x0088 stays out of the string-map dispatcher.
//
// Neither path has a `PreviewImageValid`-style gate the way Olympus's
// `CameraSettings` composite does, so the only omission case is `raw`
// (whichever path supplies it) being empty; an out-of-bounds declared range
// shows `ExtractBinary`'s declared-length placeholder, per this plan's
// established default-dump contract (Task 1/3/4).

const MINOLTA_PREVIEW_IMAGE: u16 = 0x0081;
const MINOLTA_PREVIEW_IMAGE_START: u16 = 0x0088;
const MINOLTA_PREVIEW_IMAGE_LENGTH: u16 = 0x0089;

fn minolta_type_size(field_type: u16) -> Option<usize> {
    Some(match field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

/// Finds one entry by tag id in the IFD at `ifd_offset` inside `data`.
fn find_minolta_entry(
    data: &[u8],
    ifd_offset: usize,
    byte_order: ByteOrder,
    target: u16,
) -> Option<IfdEntry> {
    let count_bytes = data.get(ifd_offset..ifd_offset + 2)?;
    let count = EndianReader::new(count_bytes, byte_order.to_io_byte_order()).u16_at(0)?;
    let entries_start = ifd_offset + 2;
    let entries = data.get(entries_start..entries_start + count as usize * 12)?;
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

fn insert_binary_or_placeholder(
    tiff: &[u8],
    offset: usize,
    total: usize,
    metadata: &mut MetadataMap,
) {
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

/// Extracts Minolta's `PreviewImage` into `metadata`, from whichever of the
/// two mechanisms `Minolta::Main` supplies. See the module section doc above.
pub fn parse_minolta_preview_image_tag(
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    let payload = ctx.payload();
    let tiff = ctx.tiff();

    // 0x0081: a direct inline value, tried first since a real body writes at
    // most one of the two mechanisms and this one is the tag actually named
    // `PreviewImage`.
    if let Some(entry) = find_minolta_entry(payload, 0, byte_order, MINOLTA_PREVIEW_IMAGE)
        && let Some(elem_size) = minolta_type_size(entry.field_type)
        && let Some(total) = elem_size.checked_mul(entry.value_count as usize)
        && total > 4
    {
        insert_binary_or_placeholder(tiff, entry.value_offset as usize, total, metadata);
        return;
    }

    // 0x0088/0x0089: the generic offset-pair composite.
    let Some(start_entry) = find_minolta_entry(payload, 0, byte_order, MINOLTA_PREVIEW_IMAGE_START)
    else {
        return;
    };
    let Some(length_entry) =
        find_minolta_entry(payload, 0, byte_order, MINOLTA_PREVIEW_IMAGE_LENGTH)
    else {
        return;
    };
    let total = length_entry.value_offset as usize;
    if total == 0 {
        return;
    }
    insert_binary_or_placeholder(tiff, start_entry.value_offset as usize, total, metadata);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minolta_parser_trait() {
        let parser = MinoltaParser::new();
        assert_eq!(parser.manufacturer_name(), "Minolta");
        assert_eq!(parser.tag_prefix(), "Minolta:");
    }

    #[test]
    fn maker_note_version_is_read_as_text() {
        // Tag 0x0000 is undef[4] holding "MLT0".
        let mut data = Vec::new();
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0x0000u16.to_le_bytes());
        data.extend_from_slice(&7u16.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(b"MLT0");
        data.extend_from_slice(&0u32.to_le_bytes());

        let mut tags = HashMap::new();
        MinoltaParser::new()
            .parse(&data, ByteOrder::LittleEndian, &mut tags)
            .unwrap();
        assert_eq!(
            tags.get("Minolta:MakerNoteVersion"),
            Some(&"MLT0".to_string())
        );
    }

    #[test]
    fn test_lens_lookup() {
        let parser = MinoltaParser::new();
        // Ids come from %sonyLensTypes, which Minolta shares.
        assert_eq!(
            parser.lookup_lens(1),
            Some("Minolta AF 80-200mm F2.8 HS-APO G".to_string())
        );
        assert_eq!(parser.lookup_lens(64000), None);
    }
}

#[cfg(test)]
mod minolta_preview_image_tests {
    use super::*;
    use crate::core::MetadataMap;

    /// Builds a synthetic TIFF block holding a Minolta MakerNote at
    /// `payload_offset` (no header, IFD at byte 0) with a single 0x0088/
    /// 0x0089 offset-pair entry, and (when `place_bytes` is `Some`) the real
    /// preview bytes at `value_offset` (TIFF-relative).
    fn build_tiff_with_minolta_offset_pair(
        payload_offset: usize,
        length: u32,
        value_offset: u32,
        place_bytes: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut tiff = vec![0u8; payload_offset];
        tiff[0..2].copy_from_slice(b"II");

        let mut payload = Vec::new();
        payload.extend_from_slice(&2u16.to_le_bytes()); // 2 entries
        // 0x0088 PreviewImageStart
        payload.extend_from_slice(&MINOLTA_PREVIEW_IMAGE_START.to_le_bytes());
        payload.extend_from_slice(&4u16.to_le_bytes()); // type: LONG
        payload.extend_from_slice(&1u32.to_le_bytes()); // count: 1
        payload.extend_from_slice(&value_offset.to_le_bytes());
        // 0x0089 PreviewImageLength
        payload.extend_from_slice(&MINOLTA_PREVIEW_IMAGE_LENGTH.to_le_bytes());
        payload.extend_from_slice(&4u16.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&length.to_le_bytes());
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
    fn offset_pair_in_bounds_becomes_binary() {
        let payload_offset = 20usize;
        let preview_bytes: Vec<u8> = (0..26u8).collect();
        let tiff = build_tiff_with_minolta_offset_pair(
            payload_offset,
            preview_bytes.len() as u32,
            13030,
            Some(&preview_bytes),
        );
        let payload_len = tiff.len() - payload_offset;
        let ctx = MakerNoteContext::in_tiff(&tiff, payload_offset, payload_len, 12);

        let mut metadata = MetadataMap::new();
        parse_minolta_preview_image_tag(&ctx, ByteOrder::LittleEndian, &mut metadata);

        assert_eq!(
            metadata.get("MakerNotes:PreviewImage"),
            Some(&TagValue::new_binary(preview_bytes))
        );
    }

    #[test]
    fn offset_pair_out_of_bounds_shows_placeholder_not_omission() {
        let payload_offset = 20usize;
        // Mirrors Minolta.jpg's real declared length (26) at an offset that
        // runs past the end of this (deliberately short) synthetic buffer.
        let tiff = build_tiff_with_minolta_offset_pair(payload_offset, 895146, 13030, None);
        let payload_len = tiff.len() - payload_offset;
        let ctx = MakerNoteContext::in_tiff(&tiff, payload_offset, payload_len, 12);

        let mut metadata = MetadataMap::new();
        parse_minolta_preview_image_tag(&ctx, ByteOrder::LittleEndian, &mut metadata);

        assert_eq!(
            metadata.get("MakerNotes:PreviewImage"),
            Some(&TagValue::new_string(
                "(Binary data 895146 bytes, use -b option to extract)"
            ))
        );
    }

    #[test]
    fn offset_pair_zero_length_is_omitted() {
        let payload_offset = 20usize;
        let tiff = build_tiff_with_minolta_offset_pair(payload_offset, 0, 13030, None);
        let payload_len = tiff.len() - payload_offset;
        let ctx = MakerNoteContext::in_tiff(&tiff, payload_offset, payload_len, 12);

        let mut metadata = MetadataMap::new();
        parse_minolta_preview_image_tag(&ctx, ByteOrder::LittleEndian, &mut metadata);

        assert_eq!(metadata.get("MakerNotes:PreviewImage"), None);
    }

    #[test]
    fn direct_0x0081_value_wins_over_offset_pair() {
        let payload_offset = 20usize;
        let preview_bytes: Vec<u8> = (0..10u8).collect();

        let mut tiff = vec![0u8; payload_offset];
        tiff[0..2].copy_from_slice(b"II");

        let mut payload = Vec::new();
        payload.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
        payload.extend_from_slice(&MINOLTA_PREVIEW_IMAGE.to_le_bytes()); // 0x0081
        payload.extend_from_slice(&7u16.to_le_bytes()); // type: undef
        payload.extend_from_slice(&(preview_bytes.len() as u32).to_le_bytes());
        payload.extend_from_slice(&200u32.to_le_bytes()); // value offset
        payload.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(&payload);

        let end = 200 + preview_bytes.len();
        tiff.resize(tiff.len().max(end), 0);
        tiff[200..end].copy_from_slice(&preview_bytes);

        let payload_len = tiff.len() - payload_offset;
        let ctx = MakerNoteContext::in_tiff(&tiff, payload_offset, payload_len, 12);

        let mut metadata = MetadataMap::new();
        parse_minolta_preview_image_tag(&ctx, ByteOrder::LittleEndian, &mut metadata);

        assert_eq!(
            metadata.get("MakerNotes:PreviewImage"),
            Some(&TagValue::new_binary(preview_bytes))
        );
    }
}
