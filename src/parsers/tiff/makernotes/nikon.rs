//! Nikon MakerNote parser
//!
//! Parses Nikon-specific EXIF MakerNote tags containing camera settings,
//! lens information, autofocus data, and other proprietary metadata.
//!
//! Supports Nikon Type 2 (IFD-based) and Type 3 (IFD-based with header) formats.

#![allow(dead_code)]
#![allow(unused_imports)]

// Submodules for extended tag parsing
pub mod af_info;
pub mod color_balance;
pub mod lens_data;
pub mod shot_info;
pub mod sub_ifds;
pub mod value_reader;

use super::nikon_capture_data;
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use crate::parsers::tiff::makernotes::shared::ifd_parser_base::{
    IfdParserConfig, parse_ifd_entries,
};
use crate::parsers::tiff::makernotes::shared::value_extractors::{
    extract_string_value, extract_string_with_offset,
};
use nom::{
    IResult,
    combinator::map,
    multi::count,
    number::complete::{be_u16, be_u32, le_u16, le_u32},
};
use std::collections::HashMap;
use value_reader::{
    ascii_value, binary_placeholder, decode_bits, format_number, format_string, print_fraction,
    print_lens_info, rational_value, read_u16, value_bytes,
};

use super::nikon_lens_database::lookup_lens_name;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};

// Nikon MakerNote Tag IDs (from ExifTool Nikon.pm)
/// Nikon Capture NX edit history (`NikonCaptureData`), a record stream
/// rather than an IFD.
const NIKON_CAPTURE_DATA: u16 = 0x0E01;

const NIKON_VERSION: u16 = 0x0001;
const NIKON_ISO_SPEED: u16 = 0x0002;
const NIKON_COLOR_MODE: u16 = 0x0003;
const NIKON_QUALITY: u16 = 0x0004;
const NIKON_WHITE_BALANCE: u16 = 0x0005;
const NIKON_SHARPNESS: u16 = 0x0006;
const NIKON_FOCUS_MODE: u16 = 0x0007;
const NIKON_FLASH_SETTING: u16 = 0x0008;
const NIKON_FLASH_TYPE: u16 = 0x0009;
const NIKON_WHITE_BALANCE_FINE: u16 = 0x000B;
/// `WB_RBLevels`, four rationals (ref `Nikon::Main` 0x000c, D1X).
const NIKON_WB_RB_LEVELS: u16 = 0x000C;
const NIKON_PROGRAM_SHIFT: u16 = 0x000D;
const NIKON_EXPOSURE_DIFF: u16 = 0x000E;
const NIKON_ISO_SELECTION: u16 = 0x000F;
/// Sub-IFD holding the JPEG preview (`Nikon::PreviewIFD`).
const NIKON_PREVIEW_IFD: u16 = 0x0011;
/// Offsets recorded by Nikon Capture (`Nikon::CaptureOffsets`).
const NIKON_CAPTURE_OFFSETS: u16 = 0x0E0E;
/// Sub-IFD written by Nikon Scan (`Nikon::Scan`).
const NIKON_SCAN_IFD: u16 = 0x0E10;
const NIKON_LENS_TYPE: u16 = 0x0083;
const NIKON_LENS: u16 = 0x0084;
const NIKON_FLASH_MODE: u16 = 0x0087;
const NIKON_SHOOTING_MODE: u16 = 0x0089;
const NIKON_LENS_FSTOPS: u16 = 0x008B;
const NIKON_CONTRAST_CURVE: u16 = 0x008C;
const NIKON_COLOR_HUE: u16 = 0x008D;
const NIKON_SCENE_MODE: u16 = 0x008F;
const NIKON_LIGHT_SOURCE: u16 = 0x0090;
const NIKON_SHOT_INFO: u16 = 0x0091; // Array tag - camera settings
const NIKON_HUE_ADJUSTMENT: u16 = 0x0092;
const NIKON_NEF_COMPRESSION: u16 = 0x0093;
const NIKON_SATURATION: u16 = 0x0094;
const NIKON_NOISE_REDUCTION: u16 = 0x0095;
const NIKON_NEF_LINEAR_ZOOM: u16 = 0x0096;
const NIKON_COLOR_BALANCE_A: u16 = 0x0097; // Array tag
const NIKON_LENS_DATA: u16 = 0x0098; // Array tag - lens information
const NIKON_RAW_IMAGE_CENTER: u16 = 0x0099;
const NIKON_SENSOR_PIXEL_SIZE: u16 = 0x009A;
const NIKON_SCENE_ASSIST: u16 = 0x009C;
const NIKON_RETOUCH_HISTORY: u16 = 0x009E;
const NIKON_SERIAL_NUMBER: u16 = 0x001D;
/// `SerialNumber` again: 0x001d is used as the decryption key, 0x00a0 is the
/// one D-series bodies actually write (ref `Nikon::Main`).
const NIKON_SERIAL_NUMBER_ALT: u16 = 0x00A0;
const NIKON_IMAGE_DATA_SIZE: u16 = 0x00A2;
const NIKON_IMAGE_COUNT: u16 = 0x00A5;
const NIKON_DELETED_IMAGE_COUNT: u16 = 0x00A6;
const NIKON_SHUTTER_COUNT: u16 = 0x00A7;
const NIKON_FLASH_INFO: u16 = 0x00A8; // Array tag
const NIKON_IMAGE_OPTIMIZATION: u16 = 0x00A9;
const NIKON_TONE_COMP: u16 = 0x0081;
// The 0x00b0-0x00b8 block below used to be mapped to ColorSpace, VRInfo,
// MultiExposure, ActiveD-Lighting, PictureControl, WorldTime, ISOInfo,
// VignetteControl and DistortionControl. Only MultiExposure was right: in
// `Nikon::Main` those ids are MultiExposure (0xb0), HighISONoiseReduction
// (0xb1), ToningEffect (0xb3), AFInfo2 (0xb7) and FileInfo (0xb8), with 0xb2,
// 0xb4 and 0xb5 unassigned. The settings tags actually live down in the
// 0x001e-0x002a range, which is where they are keyed from now.
const NIKON_MULTI_EXPOSURE: u16 = 0x00B0;
const NIKON_SATURATION_TEXT: u16 = 0x00AA; // Saturation as text
const NIKON_VARI_PROGRAM: u16 = 0x00AB; // VariProgram
const NIKON_IMAGE_PROCESSING: u16 = 0x001A; // Image processing
const NIKON_WORLD_TIME: u16 = 0x0024;
const NIKON_VR_INFO: u16 = 0x001F;
const NIKON_FLASH_EXPOSURE_COMP: u16 = 0x0012; // Flash exposure compensation
const NIKON_EXTERNAL_FLASH_COMP: u16 = 0x0017; // External flash exposure compensation
const NIKON_FLASH_BRACKET_VALUE: u16 = 0x0018; // Flash exposure bracket value
const NIKON_EXPOSURE_BRACKET_VALUE: u16 = 0x0019; // Exposure bracket value
const NIKON_COLOR_SPACE: u16 = 0x001E;
const NIKON_IMAGE_AUTH: u16 = 0x0020; // Image authentication
const NIKON_ACTIVE_D_LIGHTING: u16 = 0x0022;
const NIKON_PICTURE_CONTROL_DATA: u16 = 0x0023; // Picture control data
const NIKON_VIGNETTE_CONTROL: u16 = 0x002A;
const NIKON_AF_INFO: u16 = 0x0088; // AF Info
const NIKON_AUTO_BRACKET_RELEASE: u16 = 0x008A; // Auto bracket release
const NIKON_MANUAL_FOCUS_DIST: u16 = 0x0085; // Manual focus distance
const NIKON_DIGITAL_ZOOM: u16 = 0x0086; // Digital zoom
const NIKON_CROP_HI_SPEED: u16 = 0x001B; // Crop Hi Speed
const NIKON_EXPOSURE_TUNING: u16 = 0x001C; // Exposure Tuning
const NIKON_ISO_SETTING: u16 = 0x0013; // ISO Setting
const NIKON_IMAGE_BOUNDARY: u16 = 0x0016; // Image Boundary
const NIKON_IMAGE_ADJUSTMENT: u16 = 0x0080; // Image Adjustment
const NIKON_AUX_LENS: u16 = 0x0082; // Auxiliary Lens
// Note: 0x00B0=ColorSpace, 0x00B7=VignetteControl, 0x00B8=DistortionControl are primary
// (HIGH_ISO_NR, AF_INFO2, FILE_INFO are alternate names for same tag IDs)

// Nikon header signatures
const NIKON_HEADER_TYPE2: &[u8] = b"Nikon\0\x02\x10\x00\x00";
const NIKON_HEADER_TYPE3: &[u8] = b"Nikon\0\x02\x00\x00\x00";

// ShotInfo array indices (varies by camera model, these are common positions)
const SHOT_INFO_VERSION: usize = 0;
const SHOT_INFO_SHUTTER_COUNT: usize = 1;
const SHOT_INFO_AF_POINT_USED: usize = 2;
const SHOT_INFO_VIBRATION_REDUCTION: usize = 4;
const SHOT_INFO_AUTO_ISO: usize = 6;
const SHOT_INFO_COLOR_MODE: usize = 10;

// LensData array indices (Type 1 - D1X, D1H, D100)
const LENS_DATA_VERSION: usize = 0;
const LENS_DATA_EXIT_PUPIL_POSITION: usize = 1;
const LENS_DATA_AF_APERTURE: usize = 2;
const LENS_DATA_FOCUS_POSITION: usize = 4;
const LENS_DATA_FOCUS_DISTANCE: usize = 5;
const LENS_DATA_FOCAL_LENGTH: usize = 6;
const LENS_DATA_LENS_ID: usize = 7;
const LENS_DATA_LENS_FSTOPS: usize = 8;
const LENS_DATA_MIN_FOCAL_LENGTH: usize = 9;
const LENS_DATA_MAX_FOCAL_LENGTH: usize = 10;
const LENS_DATA_MAX_APERTURE_AT_MIN_FOCAL: usize = 11;
const LENS_DATA_MAX_APERTURE_AT_MAX_FOCAL: usize = 12;

/// Decodes Nikon flash mode to human-readable string (`Nikon::Main` 0x0087).
fn decode_flash_mode(value: i32) -> String {
    match value {
        0 => "Did Not Fire".to_string(),
        1 => "Fired, Manual".to_string(),
        3 => "Not Ready".to_string(),
        7 => "Fired, External".to_string(),
        8 => "Fired, Commander Mode".to_string(),
        9 => "Fired, TTL Mode".to_string(),
        18 => "LED Light".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// `Nikon::Main` 0x0083 `LensType`: a bit field, then a sequence of rewrites
/// that reorder the flags into Nikon's own naming order (`E` first, `1` first,
/// `FT-1` last) and collapse `D G` to plain `G`.
fn decode_lens_type(value: u32) -> String {
    if value == 0 {
        return "AF".to_string();
    }
    let mut s = decode_bits(
        value,
        8,
        &[
            (0, "MF"),
            (1, "D"),
            (2, "G"),
            (3, "VR"),
            (4, "1"),
            (5, "FT-1"),
            (6, "E"),
            (7, "AF-P"),
        ],
    );
    // s/,//g; s/\bD G\b/G/;
    s = s.replace(',', "");
    s = replace_bounded_once(&s, "D G", "G");
    // s/ E\b// and s/^(G )?/E /;
    if let Some(stripped) = remove_bounded_once(&s, " E") {
        s = match stripped.strip_prefix("G ") {
            Some(rest) => format!("E {}", rest),
            None => format!("E {}", stripped),
        };
    }
    // s/ 1// and $_ = "1 $_";
    if let Some(stripped) = remove_plain_once(&s, " 1") {
        s = format!("1 {}", stripped);
    }
    // s/FT-1 // and $_ .= ' FT-1';
    if let Some(stripped) = remove_plain_once(&s, "FT-1 ") {
        s = format!("{} FT-1", stripped);
    }
    s
}

/// `Nikon::Main` 0x0089 `ShootingMode`: a bit field with a "Single-Frame"
/// prefix when none of the release-mode bits (0x87) are set.
///
/// Bit 5 is model-dependent: the D70 uses it for "Unused LE-NR Slowdown" and
/// every other body for "Auto ISO".
fn decode_shooting_mode(value: u32, model: Option<&str>) -> String {
    let is_d70 = model.is_some_and(|m| {
        m.match_indices("D70").any(|(idx, _)| {
            !m[idx + 3..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    });
    let bit5 = if is_d70 {
        "Unused LE-NR Slowdown"
    } else {
        "Auto ISO"
    };
    let labels = [
        (0u32, "Continuous"),
        (1, "Delay"),
        (2, "PC Control"),
        (3, "Self-timer"),
        (4, "Exposure Bracketing"),
        (5, bit5),
        (6, "White-Balance Bracketing"),
        (7, "IR Control"),
        (8, "D-Lighting Bracketing"),
        (11, "Pre-capture"),
    ];
    if value & 0x87 == 0 {
        if value == 0 {
            return "Single-Frame".to_string();
        }
        return format!("Single-Frame, {}", decode_bits(value, 32, &labels));
    }
    decode_bits(value, 32, &labels)
}

/// Perl `s/\bNEEDLE\b/REPLACEMENT/`: replace the first whole-word occurrence.
fn replace_bounded_once(haystack: &str, needle: &str, replacement: &str) -> String {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut search = 0;
    while let Some(rel) = haystack[search..].find(needle) {
        let idx = search + rel;
        let end = idx + needle.len();
        let before_ok = haystack[..idx]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word(c));
        let after_ok = haystack[end..].chars().next().is_none_or(|c| !is_word(c));
        if before_ok && after_ok {
            return format!("{}{}{}", &haystack[..idx], replacement, &haystack[end..]);
        }
        search = idx + 1;
    }
    haystack.to_string()
}

/// Perl `s/NEEDLE\b//`: remove the first occurrence followed by a word
/// boundary. Returns `None` when nothing matched, mirroring the `and` guard
/// that Perl puts on the result of the substitution.
fn remove_bounded_once(haystack: &str, needle: &str) -> Option<String> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut search = 0;
    while let Some(rel) = haystack[search..].find(needle) {
        let idx = search + rel;
        let end = idx + needle.len();
        if haystack[end..].chars().next().is_none_or(|c| !is_word(c)) {
            return Some(format!("{}{}", &haystack[..idx], &haystack[end..]));
        }
        search = idx + 1;
    }
    None
}

/// Perl `s/NEEDLE//` with no boundary assertions.
fn remove_plain_once(haystack: &str, needle: &str) -> Option<String> {
    let idx = haystack.find(needle)?;
    Some(format!(
        "{}{}",
        &haystack[..idx],
        &haystack[idx + needle.len()..]
    ))
}

/// ExifTool `%cropHiSpeed` (`Nikon::Main` 0x001b).
fn decode_crop_hi_speed(value: u16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "1.3x Crop".to_string(),
        2 => "DX Crop".to_string(),
        3 => "5:4 Crop".to_string(),
        4 => "3:2 Crop".to_string(),
        6 => "16:9 Crop".to_string(),
        8 => "2.7x Crop".to_string(),
        9 => "DX Movie 16:9 Crop".to_string(),
        10 => "1.3x Movie Crop".to_string(),
        11 => "FX Uncropped".to_string(),
        12 => "DX Uncropped".to_string(),
        13 => "2.8x Movie Crop".to_string(),
        14 => "1.4x Movie Crop".to_string(),
        15 => "1.5x Movie Crop".to_string(),
        17 => "FX 1:1 Crop".to_string(),
        18 => "DX 1:1 Crop".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// ExifTool's `my ($a,$b,$c)=unpack("c3",$val); $c ? $a*($b/$c) : 0`.
///
/// Nikon packs several EV-style values as a signed byte triple: numerator,
/// multiplier and divisor. Used by ProgramShift, ExposureDifference and the
/// flash compensations.
fn nikon_signed_ev(bytes: &[u8]) -> Option<f64> {
    if bytes.len() < 3 {
        return None;
    }
    let a = bytes[0] as i8 as f64;
    let b = bytes[1] as i8 as f64;
    let c = bytes[2] as i8 as f64;
    Some(if c != 0.0 { a * (b / c) } else { 0.0 })
}

/// The unsigned variant, `unpack("C3",$val)`, used by `LensFStops` (0x008b).
fn nikon_unsigned_ratio(bytes: &[u8]) -> Option<f64> {
    if bytes.len() < 3 {
        return None;
    }
    let (a, b, c) = (bytes[0] as f64, bytes[1] as f64, bytes[2] as f64);
    Some(if c != 0.0 { a * (b / c) } else { 0.0 })
}

/// `Nikon::Main` 0x001e `ColorSpace`.
fn decode_color_space(value: u32) -> String {
    match value {
        1 => "sRGB".to_string(),
        2 => "Adobe RGB".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// `Nikon::Main` 0x0022 `ActiveD-Lighting`.
fn decode_active_d_lighting(value: u32) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Low".to_string(),
        3 => "Normal".to_string(),
        5 => "High".to_string(),
        7 => "Extra High".to_string(),
        8 => "Extra High 1".to_string(),
        9 => "Extra High 2".to_string(),
        10 => "Extra High 3".to_string(),
        11 => "Extra High 4".to_string(),
        0xFFFF => "Auto".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// `Nikon::Main` 0x002a `VignetteControl`. Note the gaps: the levels are
/// 0/1/3/5, not 0/1/2/3.
fn decode_vignette_control(value: u32) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Low".to_string(),
        3 => "Normal".to_string(),
        5 => "High".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// Represents a Nikon MakerNote parser
pub struct NikonParser;

impl MakerNoteParser for NikonParser {
    fn manufacturer_name(&self) -> &'static str {
        "Nikon"
    }

    fn tag_prefix(&self) -> &'static str {
        "Nikon:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Nikon Type 2/3 headers start with "Nikon\0"
        data.len() >= 6 && &data[0..6] == b"Nikon\0"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_with_model(data, byte_order, None, tags)
    }

    fn parse_with_model(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        if data.is_empty() {
            return Ok(());
        }

        // Validate Nikon header
        if !self.validate_header(data) {
            return Err("Invalid Nikon MakerNote header".to_string());
        }

        // Nikon Type 2/3 MakerNotes have an embedded TIFF structure after the Nikon header
        // Structure: "Nikon\0" (6 bytes) + version (4 bytes) + TIFF header + IFD
        // The TIFF header contains its own byte order indicator and IFD offset

        // Skip Nikon-specific header (10 bytes: "Nikon\0" + 4-byte version)
        let tiff_start = 10;

        if data.len() < tiff_start + 8 {
            return Ok(());
        }

        // Parse embedded TIFF byte order from bytes 10-11
        let tiff_data = &data[tiff_start..];
        let tiff_byte_order = if tiff_data.len() >= 2 {
            if &tiff_data[0..2] == b"MM" {
                ByteOrder::BigEndian
            } else if &tiff_data[0..2] == b"II" {
                ByteOrder::LittleEndian
            } else {
                return Err("Invalid TIFF byte order in Nikon MakerNote".to_string());
            }
        } else {
            byte_order // Fallback to provided byte order
        };

        // Read IFD offset from TIFF header (bytes 4-7 of TIFF structure)
        let ifd_offset_in_tiff = if tiff_byte_order == ByteOrder::BigEndian {
            u32::from_be_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]]) as usize
        } else {
            u32::from_le_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]]) as usize
        };

        // IFD offset is relative to the start of the TIFF structure (byte 10 in full data)
        let ifd_absolute = tiff_start + ifd_offset_in_tiff;

        if data.len() <= ifd_absolute + 2 {
            return Ok(());
        }

        let config = IfdParserConfig {
            signature: None,
            signature_offset: 0,
            max_entries: 200,
        };

        // Every value inside this IFD -- inline or out-of-line -- is resolved
        // through `value_bytes`, which understands that offsets are relative to
        // the embedded TIFF header at `tiff_start` and that values of four
        // bytes or fewer live in the entry itself. The MakerNote's own byte
        // order (`tiff_byte_order`) governs, not the enclosing file's.
        let order = tiff_byte_order;
        let bytes_of = |entry: &IfdEntry| value_bytes(entry, data, tiff_start, order);

        // Read an entry that holds a single unsigned integer.
        let scalar_of = |entry: &IfdEntry| -> Option<u32> {
            let bytes = bytes_of(entry)?;
            match bytes.len() {
                1 => Some(bytes[0] as u32),
                2 => read_u16(&bytes, 0, order).map(u32::from),
                4 => value_reader::read_u32(&bytes, 0, order),
                _ => None,
            }
        };

        // Read an entry that holds a single signed 16-bit integer.
        let scalar_i16_of = |entry: &IfdEntry| -> Option<i16> {
            let bytes = bytes_of(entry)?;
            read_u16(&bytes, 0, order).map(|v| v as i16)
        };

        // Read every int16u in an entry as a space-separated list, which is
        // how ExifTool renders multi-count integer tags before PrintConv.
        let u16_list_of = |entry: &IfdEntry| -> Option<String> {
            let bytes = bytes_of(entry)?;
            let values: Vec<String> = (0..bytes.len() / 2)
                .filter_map(|i| read_u16(&bytes, i * 2, order))
                .map(|v| v.to_string())
                .collect();
            if values.is_empty() {
                None
            } else {
                Some(values.join(" "))
            }
        };

        // Read a string tag and apply the Nikon main table's PRINT_CONV.
        let string_of = |entry: &IfdEntry| -> Option<String> {
            let bytes = bytes_of(entry)?;
            Some(format_string(&ascii_value(&bytes)))
        };

        // Parse IFD entries starting at the IFD location
        // Pass the full 'data' buffer so that offset calculations work correctly
        let _ = parse_ifd_entries(&data[ifd_absolute..], order, &config, |entry, _ifd_data| {
            match entry.tag_id {
                // Nikon Capture NX edit history. Not an IFD -- a stream of
                // variable-length records with 32-bit ids, always
                // little-endian, walked by nikon_capture_data. It is
                // reached only from here: the existing NikonCaptureParser
                // is dispatched on a MakerNote *signature*, which no NEF
                // presents, so it never ran.
                NIKON_CAPTURE_DATA => {
                    let start = tiff_start + entry.value_offset as usize;
                    let end = start.saturating_add(entry.value_count as usize);
                    if let Some(block) = data.get(start..end) {
                        nikon_capture_data::parse_nikon_capture_data(block, tags);
                    }
                }

                // undef[4]. Binary on early Coolpix models, ASCII digits on
                // everything else; PrintConv turns "0210" into "2.10".
                NIKON_VERSION => {
                    if let Some(bytes) = bytes_of(entry)
                        && bytes.len() >= 4
                    {
                        let raw = if bytes[0] <= 0x09 {
                            bytes[..4].iter().map(|b| b.to_string()).collect::<String>()
                        } else {
                            ascii_value(&bytes)
                        };
                        let printed =
                            if raw.len() >= 2 && raw[..2].chars().all(|c| c.is_ascii_digit()) {
                                let joined = format!("{}.{}", &raw[..2], &raw[2..]);
                                joined.strip_prefix('0').unwrap_or(&joined).to_string()
                            } else {
                                raw
                            };
                        tags.insert("Nikon:MakerNoteVersion".to_string(), printed);
                    }
                }

                // int16u[2]: "0 200" for a plain ISO, "1 200" for a Hi mode.
                // RawConv drops the all-zero form the D300 writes for LO ISO.
                NIKON_ISO_SPEED => {
                    if let Some(list) = u16_list_of(entry)
                        && list != "0 0"
                    {
                        let printed = if let Some(rest) = list.strip_prefix("0 ") {
                            rest.to_string()
                        } else if let Some(rest) = list.strip_prefix("1 ") {
                            format!("Hi {}", rest)
                        } else {
                            list
                        };
                        tags.insert("Nikon:ISO".to_string(), printed);
                    }
                }

                NIKON_SHUTTER_COUNT => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert("Nikon:ShutterCount".to_string(), value.to_string());
                    }
                }

                NIKON_IMAGE_COUNT => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert("Nikon:ImageCount".to_string(), value.to_string());
                    }
                }

                NIKON_FLASH_MODE => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert(
                            "Nikon:FlashMode".to_string(),
                            decode_flash_mode(value as i32),
                        );
                    }
                }

                NIKON_SHOOTING_MODE => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert(
                            "Nikon:ShootingMode".to_string(),
                            decode_shooting_mode(value, model),
                        );
                    }
                }

                NIKON_COLOR_SPACE => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert("Nikon:ColorSpace".to_string(), decode_color_space(value));
                    }
                }

                NIKON_ACTIVE_D_LIGHTING => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert(
                            "Nikon:ActiveD-Lighting".to_string(),
                            decode_active_d_lighting(value),
                        );
                    }
                }

                NIKON_VIGNETTE_CONTROL => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert(
                            "Nikon:VignetteControl".to_string(),
                            decode_vignette_control(value),
                        );
                    }
                }

                NIKON_LENS_TYPE => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert("Nikon:LensType".to_string(), decode_lens_type(value));
                    }
                }

                // rational64u[4]: short focal, long focal, aperture at each.
                NIKON_LENS => {
                    if let Some(bytes) = bytes_of(entry) {
                        let values: Vec<f64> = (0..4)
                            .filter_map(|i| rational_value(&bytes, i, order, false))
                            .collect();
                        if let Some(printed) = print_lens_info(&values) {
                            tags.insert("Nikon:Lens".to_string(), printed);
                        }
                    }
                }

                // Flat binary block; layout keys off its own version string.
                NIKON_LENS_DATA => {
                    if let Some(bytes) = bytes_of(entry) {
                        lens_data::parse_lens_data(&bytes, tags);
                    }
                }

                // Four bytes: two enums plus an int16u bitmask whose byte order
                // is chosen by camera model rather than by the TIFF header.
                NIKON_AF_INFO => {
                    if let Some(bytes) = bytes_of(entry) {
                        af_info::parse_af_info(&bytes, af_info::af_info_byte_order(model), tags);
                    }
                }

                NIKON_PREVIEW_IFD => {
                    sub_ifds::parse_preview_ifd(
                        data,
                        tiff_start,
                        entry.value_offset as usize,
                        order,
                        tags,
                    );
                }

                NIKON_SCAN_IFD => {
                    sub_ifds::parse_scan_ifd(
                        data,
                        tiff_start,
                        entry.value_offset as usize,
                        order,
                        tags,
                    );
                }

                NIKON_CAPTURE_OFFSETS => {
                    if let Some(bytes) = bytes_of(entry) {
                        sub_ifds::parse_capture_offsets(&bytes, order, tags);
                    }
                }

                // ColorBalance (0x0097). The block's leading version string
                // selects the layout; only 0103 (D70/D70s) is stored in the
                // clear at a known offset. Every 02xx variant is encrypted with
                // a SerialNumber/ShutterCount key, so nothing is emitted for
                // them rather than a plausible-looking guess.
                NIKON_COLOR_BALANCE_A => {
                    if let Some(bytes) = bytes_of(entry)
                        && bytes.len() >= 4
                        && &bytes[..4] == b"0103"
                    {
                        let levels: Vec<String> = (0..4)
                            .filter_map(|i| read_u16(&bytes, 20 + i * 2, order))
                            .map(|v| v.to_string())
                            .collect();
                        if levels.len() == 4 {
                            tags.insert("Nikon:WB_RGBGLevels".to_string(), levels.join(" "));
                        }
                    }
                }

                // rational64u[4]
                NIKON_WB_RB_LEVELS => {
                    if let Some(bytes) = bytes_of(entry) {
                        let values: Vec<String> = (0..4)
                            .filter_map(|i| rational_value(&bytes, i, order, false))
                            .map(format_number)
                            .collect();
                        if values.len() == 4 {
                            tags.insert("Nikon:WB_RBLevels".to_string(), values.join(" "));
                        }
                    }
                }

                // Binary blobs reported only as a byte count unless -b is used.
                NIKON_CONTRAST_CURVE => {
                    if let Some(len) = value_reader::value_len(entry) {
                        tags.insert("Nikon:ContrastCurve".to_string(), binary_placeholder(len));
                    }
                }

                NIKON_NEF_LINEAR_ZOOM => {
                    if let Some(len) = value_reader::value_len(entry) {
                        tags.insert(
                            "Nikon:NEFLinearizationTable".to_string(),
                            binary_placeholder(len),
                        );
                    }
                }

                // int16u[2]
                NIKON_RAW_IMAGE_CENTER => {
                    if let Some(list) = u16_list_of(entry) {
                        tags.insert("Nikon:RawImageCenter".to_string(), list);
                    }
                }

                // rational64u[2], printed as "W x H um"
                NIKON_SENSOR_PIXEL_SIZE => {
                    if let Some(bytes) = bytes_of(entry) {
                        let values: Vec<String> = (0..2)
                            .filter_map(|i| rational_value(&bytes, i, order, false))
                            .map(format_number)
                            .collect();
                        if values.len() == 2 {
                            tags.insert(
                                "Nikon:SensorPixelSize".to_string(),
                                format!("{} x {} um", values[0], values[1]),
                            );
                        }
                    }
                }

                // String tags. The Nikon main table's PRINT_CONV (FormatString)
                // restores the mixed case behind Nikon's all-caps values.
                NIKON_QUALITY
                | NIKON_WHITE_BALANCE
                | NIKON_SHARPNESS
                | NIKON_FOCUS_MODE
                | NIKON_FLASH_SETTING
                | NIKON_ISO_SELECTION
                | NIKON_SERIAL_NUMBER
                | NIKON_SERIAL_NUMBER_ALT
                | NIKON_IMAGE_OPTIMIZATION
                | NIKON_SATURATION_TEXT
                | NIKON_VARI_PROGRAM
                | NIKON_COLOR_MODE
                | NIKON_SCENE_MODE
                | NIKON_LIGHT_SOURCE
                | NIKON_NOISE_REDUCTION
                | NIKON_TONE_COMP
                | NIKON_COLOR_HUE
                | NIKON_IMAGE_PROCESSING
                | NIKON_SCENE_ASSIST
                | NIKON_FLASH_TYPE => {
                    if let Some(value) = string_of(entry) {
                        tags.insert(nikon_tag_to_name(entry.tag_id), value);
                    }
                }

                NIKON_DELETED_IMAGE_COUNT => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert("Nikon:DeletedImageCount".to_string(), value.to_string());
                    }
                }

                NIKON_IMAGE_DATA_SIZE => {
                    if let Some(value) = scalar_of(entry) {
                        tags.insert("Nikon:ImageDataSize".to_string(), value.to_string());
                    }
                }

                NIKON_WHITE_BALANCE_FINE => {
                    if let Some(value) = scalar_i16_of(entry) {
                        tags.insert("Nikon:WhiteBalanceFineTune".to_string(), value.to_string());
                    }
                }

                // undef[4] byte triples, rendered as fractions.
                NIKON_PROGRAM_SHIFT => {
                    if let Some(bytes) = bytes_of(entry)
                        && let Some(value) = nikon_signed_ev(&bytes)
                    {
                        tags.insert("Nikon:ProgramShift".to_string(), print_fraction(value));
                    }
                }

                NIKON_EXPOSURE_DIFF => {
                    if let Some(bytes) = bytes_of(entry)
                        && let Some(value) = nikon_signed_ev(&bytes)
                    {
                        // PrintConv: $val ? sprintf("%+.1f",$val) : 0
                        let printed = if value == 0.0 {
                            "0".to_string()
                        } else {
                            format!("{:+.1}", value)
                        };
                        tags.insert("Nikon:ExposureDifference".to_string(), printed);
                    }
                }

                NIKON_FLASH_EXPOSURE_COMP => {
                    if let Some(bytes) = bytes_of(entry)
                        && let Some(value) = nikon_signed_ev(&bytes)
                    {
                        tags.insert("Nikon:FlashExposureComp".to_string(), print_fraction(value));
                    }
                }

                NIKON_EXTERNAL_FLASH_COMP => {
                    if let Some(bytes) = bytes_of(entry)
                        && let Some(value) = nikon_signed_ev(&bytes)
                    {
                        tags.insert(
                            "Nikon:ExternalFlashExposureComp".to_string(),
                            print_fraction(value),
                        );
                    }
                }

                NIKON_FLASH_BRACKET_VALUE => {
                    if let Some(bytes) = bytes_of(entry)
                        && let Some(value) = nikon_signed_ev(&bytes)
                    {
                        tags.insert(
                            "Nikon:FlashExposureBracketValue".to_string(),
                            format!("{:.1}", value),
                        );
                    }
                }

                NIKON_EXPOSURE_TUNING => {
                    if let Some(bytes) = bytes_of(entry)
                        && let Some(value) = nikon_signed_ev(&bytes)
                    {
                        tags.insert("Nikon:ExposureTuning".to_string(), print_fraction(value));
                    }
                }

                // rational64s
                NIKON_EXPOSURE_BRACKET_VALUE => {
                    if let Some(bytes) = bytes_of(entry) {
                        let printed = match rational_value(&bytes, 0, order, true) {
                            Some(value) => print_fraction(value),
                            // 0/0 is what a Z9 writes for the C30/C60/C90 modes.
                            None => "n/a".to_string(),
                        };
                        tags.insert("Nikon:ExposureBracketValue".to_string(), printed);
                    }
                }

                NIKON_HUE_ADJUSTMENT => {
                    if let Some(value) = scalar_i16_of(entry) {
                        tags.insert("Nikon:HueAdjustment".to_string(), value.to_string());
                    }
                }

                NIKON_SATURATION => {
                    if let Some(value) = scalar_i16_of(entry) {
                        tags.insert("Nikon:SaturationAdj".to_string(), value.to_string());
                    }
                }

                // undef[4] unsigned byte triple: 64/12 = 5.33 stops.
                NIKON_LENS_FSTOPS => {
                    if let Some(bytes) = bytes_of(entry)
                        && let Some(value) = nikon_unsigned_ratio(&bytes)
                    {
                        tags.insert("Nikon:LensFStops".to_string(), format!("{:.2}", value));
                    }
                }

                NIKON_NEF_COMPRESSION => {
                    if let Some(value) = scalar_of(entry) {
                        let mode = match value {
                            1 => "Lossy (type 1)".to_string(),
                            2 => "Uncompressed".to_string(),
                            3 => "Lossless".to_string(),
                            4 => "Lossy (type 2)".to_string(),
                            5 => "Striped packed 12 bits".to_string(),
                            6 => "Uncompressed (reduced to 12 bit)".to_string(),
                            7 => "Unpacked 12 bits".to_string(),
                            8 => "Small".to_string(),
                            9 => "Packed 12 bits".to_string(),
                            10 => "Packed 14 bits".to_string(),
                            13 => "High Efficiency".to_string(),
                            14 => "High Efficiency*".to_string(),
                            other => format!("Unknown ({})", other),
                        };
                        tags.insert("Nikon:NEFCompression".to_string(), mode);
                    }
                }

                NIKON_IMAGE_AUTH => {
                    if let Some(value) = scalar_of(entry) {
                        let status = if value == 0 { "Off" } else { "On" };
                        tags.insert("Nikon:ImageAuthentication".to_string(), status.to_string());
                    }
                }

                // int16u[2], same "0 <iso>" shape as tag 0x0002.
                NIKON_ISO_SETTING => {
                    if let Some(list) = u16_list_of(entry) {
                        let printed = list.strip_prefix("0 ").unwrap_or(&list).to_string();
                        tags.insert("Nikon:ISOSetting".to_string(), printed);
                    }
                }

                NIKON_FLASH_INFO => {
                    if let Some(bytes) = bytes_of(entry)
                        && bytes.len() >= 4
                    {
                        tags.insert(
                            "Nikon:FlashInfoVersion".to_string(),
                            ascii_value(&bytes[..4]),
                        );
                    }
                }

                NIKON_WORLD_TIME => {
                    if let Some(offset_minutes) = scalar_i16_of(entry) {
                        let hours = offset_minutes / 60;
                        let minutes = (offset_minutes % 60).abs();
                        let sign = if offset_minutes >= 0 { "+" } else { "-" };
                        tags.insert(
                            "Nikon:WorldTime".to_string(),
                            format!("UTC{}{:02}:{:02}", sign, hours.abs(), minutes),
                        );
                    }
                }

                NIKON_VR_INFO => {
                    if let Some(bytes) = bytes_of(entry)
                        && bytes.len() >= 4
                    {
                        tags.insert("Nikon:VRInfoVersion".to_string(), ascii_value(&bytes[..4]));
                    }
                }

                NIKON_MULTI_EXPOSURE => {
                    if let Some(bytes) = bytes_of(entry)
                        && bytes.len() >= 4
                    {
                        tags.insert(
                            "Nikon:MultiExposureVersion".to_string(),
                            ascii_value(&bytes[..4]),
                        );
                    }
                    if let Some(bytes) = bytes_of(entry)
                        && let Some(raw) = value_reader::read_u32(&bytes, 4, order)
                    {
                        let mode = match raw {
                            0 => "Off".to_string(),
                            1 => "Multiple Exposure".to_string(),
                            2 => "Image Overlay".to_string(),
                            3 => "HDR".to_string(),
                            other => format!("Unknown ({})", other),
                        };
                        tags.insert("Nikon:MultiExposureMode".to_string(), mode);
                    }
                }

                NIKON_IMAGE_BOUNDARY => {
                    if let Some(list) = u16_list_of(entry) {
                        tags.insert("Nikon:ImageBoundary".to_string(), list);
                    }
                }

                // int16u[7]: mode, then the source and cropped dimensions and
                // the crop origin. ExifTool's %cropHiSpeed OTHER handler spells
                // all seven out; with any other count only the mode is named.
                NIKON_CROP_HI_SPEED => {
                    if let Some(bytes) = bytes_of(entry) {
                        let values: Vec<u16> = (0..bytes.len() / 2)
                            .filter_map(|i| read_u16(&bytes, i * 2, order))
                            .collect();
                        if let Some(&raw) = values.first() {
                            let mode = decode_crop_hi_speed(raw);
                            let printed = if values.len() == 7 {
                                format!(
                                    "{} ({}x{} cropped to {}x{} at pixel {},{})",
                                    mode,
                                    values[1],
                                    values[2],
                                    values[3],
                                    values[4],
                                    values[5],
                                    values[6]
                                )
                            } else {
                                mode
                            };
                            tags.insert("Nikon:CropHiSpeed".to_string(), printed);
                        }
                    }
                }

                NIKON_PICTURE_CONTROL_DATA => {
                    if let Some(bytes) = bytes_of(entry)
                        && bytes.len() >= 4
                    {
                        tags.insert(
                            "Nikon:PictureControlVersion".to_string(),
                            ascii_value(&bytes[..4]),
                        );
                    }
                }

                // Skip unrecognized tags silently
                _ => {}
            }
        });

        Ok(())
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }
}

/// Maps Nikon MakerNote tag IDs to human-readable tag names
///
/// Returns tags with "Nikon:" family prefix per ExifTool convention.
fn nikon_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        // Basic tags (0x0001-0x001F)
        NIKON_VERSION => "MakerNoteVersion",
        NIKON_ISO_SPEED => "ISO",
        NIKON_COLOR_MODE => "ColorMode",
        NIKON_QUALITY => "Quality",
        NIKON_WHITE_BALANCE => "WhiteBalance",
        NIKON_SHARPNESS => "Sharpness",
        NIKON_FOCUS_MODE => "FocusMode",
        NIKON_FLASH_SETTING => "FlashSetting",
        NIKON_FLASH_TYPE => "FlashType",
        NIKON_WHITE_BALANCE_FINE => "WhiteBalanceFineTune",
        NIKON_WB_RB_LEVELS => "WB_RBLevels",
        NIKON_PROGRAM_SHIFT => "ProgramShift",
        NIKON_EXPOSURE_DIFF => "ExposureDifference",
        NIKON_ISO_SELECTION => "ISOSelection",
        NIKON_FLASH_EXPOSURE_COMP => "FlashExposureComp",
        NIKON_ISO_SETTING => "ISOSetting",
        NIKON_IMAGE_BOUNDARY => "ImageBoundary",
        NIKON_EXTERNAL_FLASH_COMP => "ExternalFlashExposureComp",
        NIKON_FLASH_BRACKET_VALUE => "FlashExposureBracketValue",
        NIKON_EXPOSURE_BRACKET_VALUE => "ExposureBracketValue",
        NIKON_IMAGE_PROCESSING => "ImageProcessing",
        NIKON_CROP_HI_SPEED => "CropHiSpeed",
        NIKON_EXPOSURE_TUNING => "ExposureTuning",
        NIKON_SERIAL_NUMBER => "SerialNumber",
        NIKON_SERIAL_NUMBER_ALT => "SerialNumber",
        NIKON_IMAGE_AUTH => "ImageAuthentication",
        NIKON_PICTURE_CONTROL_DATA => "PictureControlData",

        // Tone & Color (0x0080-0x0082)
        NIKON_IMAGE_ADJUSTMENT => "ImageAdjustment",
        NIKON_TONE_COMP => "ToneComp",
        NIKON_AUX_LENS => "AuxiliaryLens",

        // Lens & AF (0x0083-0x008F)
        NIKON_LENS_TYPE => "LensType",
        NIKON_LENS => "Lens",
        NIKON_MANUAL_FOCUS_DIST => "ManualFocusDistance",
        NIKON_DIGITAL_ZOOM => "DigitalZoom",
        NIKON_FLASH_MODE => "FlashMode",
        NIKON_AF_INFO => "AFInfo",
        NIKON_SHOOTING_MODE => "ShootingMode",
        NIKON_AUTO_BRACKET_RELEASE => "AutoBracketRelease",
        NIKON_LENS_FSTOPS => "LensFStops",
        NIKON_CONTRAST_CURVE => "ContrastCurve",
        NIKON_COLOR_HUE => "ColorHue",
        NIKON_SCENE_MODE => "SceneMode",

        // Processing (0x0090-0x009E)
        NIKON_LIGHT_SOURCE => "LightSource",
        NIKON_SHOT_INFO => "ShotInfo",
        NIKON_HUE_ADJUSTMENT => "HueAdjustment",
        NIKON_NEF_COMPRESSION => "NEFCompression",
        NIKON_SATURATION => "SaturationAdj",
        NIKON_NOISE_REDUCTION => "NoiseReduction",
        NIKON_NEF_LINEAR_ZOOM => "NEFLinearizationTable",
        NIKON_COLOR_BALANCE_A => "ColorBalance",
        NIKON_LENS_DATA => "LensData",
        NIKON_RAW_IMAGE_CENTER => "RawImageCenter",
        NIKON_SENSOR_PIXEL_SIZE => "SensorPixelSize",
        NIKON_SCENE_ASSIST => "SceneAssist",
        NIKON_RETOUCH_HISTORY => "RetouchHistory",

        // File info (0x00A0-0x00AF)
        NIKON_IMAGE_DATA_SIZE => "ImageDataSize",
        NIKON_IMAGE_COUNT => "ImageCount",
        NIKON_DELETED_IMAGE_COUNT => "DeletedImageCount",
        NIKON_SHUTTER_COUNT => "ShutterCount",
        NIKON_FLASH_INFO => "FlashInfo",
        NIKON_IMAGE_OPTIMIZATION => "ImageOptimization",
        NIKON_SATURATION_TEXT => "Saturation",
        NIKON_VARI_PROGRAM => "VariProgram",

        // Advanced (0x00B0-0x00B8)
        NIKON_COLOR_SPACE => "ColorSpace",
        NIKON_VR_INFO => "VRInfo",
        NIKON_MULTI_EXPOSURE => "MultiExposure",
        NIKON_ACTIVE_D_LIGHTING => "ActiveD-Lighting",
        NIKON_WORLD_TIME => "WorldTime",
        NIKON_VIGNETTE_CONTROL => "VignetteControl",

        _ => return format!("Nikon:Unknown-{:#06X}", tag_id),
    };

    format!("Nikon:{}", tag_name)
}

/// Public function to parse Nikon MakerNotes
///
/// This is the main entry point for parsing Nikon MakerNote data.
///
/// # Parameters
/// - `data`: Raw MakerNote data (including Nikon header)
/// - `byte_order`: Byte order for parsing multi-byte values
/// - `tags`: HashMap to populate with extracted tags
pub fn parse_nikon_makernotes(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = NikonParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("Nikon MakerNotes parse error: {}", e);
    }
}

/// Checks if data appears to be a Nikon MakerNote
///
/// # Parameters
/// - `data`: Raw byte data to check
///
/// # Returns
/// `true` if the data appears to be a Nikon MakerNote, `false` otherwise
pub fn is_nikon_makernote(data: &[u8]) -> bool {
    data.len() >= 6 && &data[0..6] == b"Nikon\0"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nikon_tag_ids() {
        assert_eq!(NIKON_VERSION, 0x0001);
        assert_eq!(NIKON_ISO_SPEED, 0x0002);
        assert_eq!(NIKON_QUALITY, 0x0004);
        assert_eq!(NIKON_WHITE_BALANCE, 0x0005);
        assert_eq!(NIKON_SHUTTER_COUNT, 0x00A7);
    }

    #[test]
    fn test_nikon_header_validation() {
        let parser = NikonParser;

        // Valid Type 2 header
        let valid_type2 = b"Nikon\0\x02\x10\x00\x00";
        assert!(parser.validate_header(valid_type2));

        // Valid Type 3 header
        let valid_type3 = b"Nikon\0\x02\x00\x00\x00";
        assert!(parser.validate_header(valid_type3));

        // Invalid header
        let invalid = b"Canon\0\x00\x00";
        assert!(!parser.validate_header(invalid));

        // Too short
        let too_short = b"Nikon";
        assert!(!parser.validate_header(too_short));
    }

    #[test]
    fn test_is_nikon_makernote() {
        assert!(is_nikon_makernote(b"Nikon\0\x02\x10\x00\x00"));
        assert!(is_nikon_makernote(b"Nikon\0extra data"));
        assert!(!is_nikon_makernote(b"Canon\0"));
        assert!(!is_nikon_makernote(b"Nikon")); // Too short
    }

    #[test]
    fn test_nikon_tag_to_name() {
        assert_eq!(nikon_tag_to_name(0x0001), "Nikon:MakerNoteVersion");
        assert_eq!(nikon_tag_to_name(0x0002), "Nikon:ISO");
        assert_eq!(nikon_tag_to_name(0x0004), "Nikon:Quality");
        assert_eq!(nikon_tag_to_name(0x00A7), "Nikon:ShutterCount");
        // Note: Some tags may match all values if constants conflict
        // 0xFFFF may not return Unknown if caught by earlier pattern
    }

    // Nikon tags 0x0004 (Quality), 0x0005 (WhiteBalance), 0x0006 (Sharpness),
    // 0x0007 (FocusMode) and 0x0008 (FlashSetting) are `Writable => 'string'`
    // in Nikon.pm for every model, so the numeric decode tables that used to
    // live here were decoding the entry's value-offset field. They are gone;
    // `format_string` is the table's real PrintConv and is tested in
    // `value_reader`.

    #[test]
    fn test_decode_flash_mode() {
        assert_eq!(decode_flash_mode(0), "Did Not Fire");
        assert_eq!(decode_flash_mode(9), "Fired, TTL Mode");
        assert_eq!(decode_flash_mode(18), "LED Light");
        // An unrecognised code reports itself rather than a neighbour's label.
        assert_eq!(decode_flash_mode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_lens_type() {
        // Ground truth produced by running Nikon.pm's own PrintConv.
        assert_eq!(decode_lens_type(0x00), "AF");
        assert_eq!(decode_lens_type(0x01), "MF");
        assert_eq!(decode_lens_type(0x02), "D");
        assert_eq!(decode_lens_type(0x04), "G");
        assert_eq!(decode_lens_type(0x06), "G"); // "D G" collapses to "G"
        assert_eq!(decode_lens_type(0x08), "VR");
        assert_eq!(decode_lens_type(0x0a), "D VR");
        assert_eq!(decode_lens_type(0x0e), "G VR");
        assert_eq!(decode_lens_type(0x10), "1");
        assert_eq!(decode_lens_type(0x16), "1 G");
        assert_eq!(decode_lens_type(0x20), "FT-1");
        assert_eq!(decode_lens_type(0x26), "G FT-1");
        assert_eq!(decode_lens_type(0x30), "1 FT-1");
        assert_eq!(decode_lens_type(0x36), "1 G FT-1");
        assert_eq!(decode_lens_type(0x46), "E G");
        assert_eq!(decode_lens_type(0x80), "AF-P");
        assert_eq!(decode_lens_type(0x86), "G AF-P");
        assert_eq!(decode_lens_type(0xc6), "E AF-P");
    }

    #[test]
    fn test_decode_shooting_mode() {
        assert_eq!(decode_shooting_mode(0, Some("NIKON D70")), "Single-Frame");
        assert_eq!(decode_shooting_mode(0x01, Some("NIKON D70")), "Continuous");
        // Bit 5 is model-dependent, and bits 0/1/2/7 suppress the prefix.
        assert_eq!(
            decode_shooting_mode(0x20, Some("NIKON D70")),
            "Single-Frame, Unused LE-NR Slowdown"
        );
        assert_eq!(
            decode_shooting_mode(0x20, Some("NIKON D200")),
            "Single-Frame, Auto ISO"
        );
        // D70s is a different body: \b in Nikon.pm's /D70\b/ excludes it.
        assert_eq!(
            decode_shooting_mode(0x20, Some("NIKON D70s")),
            "Single-Frame, Auto ISO"
        );
        // Bit 9 has no label: it must report itself.
        assert_eq!(
            decode_shooting_mode(0x200, Some("NIKON D70")),
            "Single-Frame, [9]"
        );
    }

    #[test]
    fn test_nikon_byte_triples() {
        // ProgramShift "\0\x01\x06\0" -> 0 * (1/6) = 0
        assert_eq!(nikon_signed_ev(&[0x00, 0x01, 0x06, 0x00]), Some(0.0));
        // -6 * (1/6) = -1 EV
        assert_eq!(nikon_signed_ev(&[0xfa, 0x01, 0x06, 0x00]), Some(-1.0));
        // A zero divisor yields 0 rather than a division by zero.
        assert_eq!(nikon_signed_ev(&[0x40, 0x01, 0x00, 0x00]), Some(0.0));
        assert_eq!(nikon_signed_ev(&[0x00, 0x01]), None);
        // LensFStops uses the UNSIGNED triple: 64 * (1/12) = 5.33.
        let stops = nikon_unsigned_ratio(&[0x40, 0x01, 0x0c, 0x00]).unwrap();
        assert_eq!(format!("{:.2}", stops), "5.33");
    }

    #[test]
    fn test_decode_color_space() {
        assert_eq!(decode_color_space(1), "sRGB");
        assert_eq!(decode_color_space(2), "Adobe RGB");
        assert_eq!(decode_color_space(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_active_d_lighting() {
        assert_eq!(decode_active_d_lighting(0), "Off");
        assert_eq!(decode_active_d_lighting(1), "Low");
        assert_eq!(decode_active_d_lighting(3), "Normal");
        assert_eq!(decode_active_d_lighting(11), "Extra High 4");
        assert_eq!(decode_active_d_lighting(0xFFFF), "Auto");
        assert_eq!(decode_active_d_lighting(2), "Unknown (2)");
    }

    #[test]
    fn test_decode_vignette_control() {
        // Nikon.pm skips 2 and 4: Normal is 3 and High is 5.
        assert_eq!(decode_vignette_control(0), "Off");
        assert_eq!(decode_vignette_control(1), "Low");
        assert_eq!(decode_vignette_control(3), "Normal");
        assert_eq!(decode_vignette_control(5), "High");
        assert_eq!(decode_vignette_control(2), "Unknown (2)");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = NikonParser;
        assert_eq!(parser.manufacturer_name(), "Nikon");
        assert_eq!(parser.tag_prefix(), "Nikon:");
    }

    #[test]
    fn test_lens_lookup() {
        let parser = NikonParser;

        // Test F-mount lens lookup
        assert!(parser.lookup_lens(147).is_some());
        assert_eq!(
            parser.lookup_lens(147),
            Some("Nikkor AF-S 24-70mm f/2.8G ED".to_string())
        );

        // Test Z-mount lens lookup
        assert_eq!(
            parser.lookup_lens(177),
            Some("Nikkor Z 50mm f/1.8 S".to_string())
        );

        // Test unknown lens
        assert_eq!(parser.lookup_lens(65000), None);
    }
}
