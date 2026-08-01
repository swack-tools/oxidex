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

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::minolta_lens_database::lookup_minolta_lens;
use super::minolta_tables::CAMERA_SETTINGS;
use super::shared::MakerNoteParser;
use super::shared::print_im::decode_print_im_from_ifd;
use super::sony::binary::{lookup, print_float, unknown, unknown_hex};
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
) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let mut main = Vec::new();
    let mut sub_dir = Vec::new();

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
        let (main, sub_dir) = parse_minolta_ifd(data, 0, byte_order, data_base, false);

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
