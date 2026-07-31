//! Canon MakerNote parser
//!
//! Parses Canon-specific EXIF MakerNote tags containing camera settings,
//! lens information, focus data, and other proprietary metadata.

#![allow(dead_code)]
#![allow(unused_imports)]

// Submodules for extended tag parsing
pub mod af_info;
pub mod camera_info;
pub mod color_data;
pub mod lens_info;

use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use crate::parsers::tiff::makernotes::shared::ifd_parser_base::{
    IfdParserConfig, parse_ifd_entries,
};
use nom::{
    IResult,
    combinator::map,
    multi::count,
    number::complete::{be_u16, be_u32, le_u16, le_u32},
};
use std::collections::HashMap;

use super::canon_lens_database::lookup_lens_name;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_i16_array;
use super::shared::value_extractors::{extract_inline_value, extract_integer_value};
use crate::bitfield_decoder;
use crate::const_decoder;

/// Canon-specific i16 array extractor that handles UNDEFINED (7) field type.
/// Canon MakerNotes often store i16 arrays with field_type 7 (UNDEFINED) instead of 3 (SHORT).
/// This function accepts both types while the standard extract_i16_array only accepts SHORT.
///
/// The `base_offset` parameter is the TIFF offset where the MakerNote data starts.
/// Canon MakerNote value_offsets are TIFF-relative, so we need to subtract the base
/// to get the position within the data slice.
fn extract_canon_i16_array_with_base(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
    base_offset: u32,
) -> Option<Vec<i16>> {
    // Accept both SHORT (3) and UNDEFINED (7) field types
    // Canon stores CameraSettings, ShotInfo, etc. as UNDEFINED but they contain i16 arrays
    if entry.field_type != 3 && entry.field_type != 7 {
        return None;
    }

    if entry.value_count == 0 {
        return None;
    }

    // For UNDEFINED type, value_count is byte count, not element count
    // For SHORT type, value_count is element count
    let (count, bytes_needed) = if entry.field_type == 7 {
        // UNDEFINED: value_count is bytes, so elements = bytes / 2
        let byte_count = entry.value_count as usize;
        (byte_count / 2, byte_count)
    } else {
        // SHORT: value_count is elements
        let element_count = entry.value_count as usize;
        (element_count, element_count * 2)
    };

    if count == 0 {
        return None;
    }

    // Inline: ≤2 shorts fit in 4-byte value_offset field
    if bytes_needed <= 4 {
        let mut result = Vec::with_capacity(count);
        let bytes = match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };

        let reader = EndianReader::new(&bytes, byte_order.to_io_byte_order());
        for i in 0..count {
            if let Some(value) = reader.i16_at(i * 2) {
                result.push(value);
            }
        }
        return Some(result);
    }

    // Offset-based: Canon MakerNote offsets are TIFF-relative
    // Adjust by subtracting the MakerNote base offset to get position in data slice
    let tiff_offset = entry.value_offset;
    if tiff_offset < base_offset {
        return None; // Offset is before MakerNote start, invalid
    }
    let relative_offset = (tiff_offset - base_offset) as usize;

    if relative_offset + bytes_needed > data.len() {
        return None;
    }

    let array_data = &data[relative_offset..relative_offset + bytes_needed];
    let reader = EndianReader::new(array_data, byte_order.to_io_byte_order());
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        if let Some(value) = reader.i16_at(i * 2) {
            result.push(value);
        }
    }
    Some(result)
}

/// Calculates the MakerNote base offset by examining the IFD structure.
/// The base offset is needed to convert TIFF-relative value_offsets to positions
/// within the MakerNote data slice.
///
/// Canon MakerNotes use TIFF-relative offsets, meaning the value_offset field
/// in each IFD entry contains an offset from the start of the entire TIFF file,
/// not from the start of the MakerNote. To correctly extract values, we need
/// to calculate: position_in_slice = value_offset - base_offset
///
/// The algorithm works by:
/// 1. Finding the first IFD entry with offset-based data (size > 4 bytes)
/// 2. Knowing the data starts right after the IFD header in the slice
/// 3. Calculating: base_offset = value_offset - expected_position_in_slice
fn calculate_makernote_base(data: &[u8], byte_order: ByteOrder) -> Option<u32> {
    if data.len() < 2 {
        return None;
    }

    let reader = EndianReader::new(data, byte_order.to_io_byte_order());
    let entry_count = reader.u16_at(0)? as usize;

    if entry_count == 0 || entry_count > 100 {
        return None;
    }

    // Calculate IFD header size: 2 bytes (entry count) + 12 bytes per entry + 4 bytes (next IFD pointer)
    // Canon MakerNote data values start right after this header
    let header_size = 2 + entry_count * 12 + 4;

    if header_size > data.len() {
        return None;
    }

    // Iterate through entries to find one with offset-based data
    // Entry format: [tag_id:2][field_type:2][value_count:4][value_offset:4]
    for i in 0..entry_count {
        let entry_offset = 2 + i * 12;
        if entry_offset + 12 > data.len() {
            break;
        }

        let field_type = reader.u16_at(entry_offset + 2)?;
        let value_count = reader.u32_at(entry_offset + 4)?;
        let value_offset = reader.u32_at(entry_offset + 8)?;

        // Calculate byte size based on field type
        // Reference: TIFF specification field types
        let type_size = match field_type {
            1 => 1,        // BYTE
            2 => 1,        // ASCII
            3 => 2,        // SHORT
            4 => 4,        // LONG
            5 => 8,        // RATIONAL
            6 => 1,        // SBYTE
            7 => 1,        // UNDEFINED
            8 => 2,        // SSHORT
            9 => 4,        // SLONG
            10 => 8,       // SRATIONAL
            11 => 4,       // FLOAT
            12 => 8,       // DOUBLE
            _ => continue, // Unknown type, skip
        };

        let total_size = type_size * value_count as usize;

        // If data is offset-based (>4 bytes), use the value_offset to calculate base
        if total_size > 4 && value_offset > 0 {
            // The value_offset is TIFF-relative
            // The data should be at position (header_size or later) in our slice
            // base_offset = value_offset - position_in_slice
            //
            // We need to find where this entry's data actually is in the slice.
            // Canon typically stores data sequentially after the IFD header.
            // For the first offset-based entry, its data starts at header_size.
            //
            // So: base_offset = value_offset - header_size
            if value_offset as usize >= header_size {
                return Some(value_offset - header_size as u32);
            }
        }
    }

    // If no offset-based entries found, return None (fallback will use 0)
    None
}

/// Legacy wrapper for extract_canon_i16_array without base offset (for test compatibility)
#[allow(dead_code)]
fn extract_canon_i16_array(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<i16>> {
    // For legacy calls, try to calculate base offset
    if let Some(base) = calculate_makernote_base(data, byte_order) {
        extract_canon_i16_array_with_base(entry, data, byte_order, base)
    } else {
        // Fallback: assume offsets are relative to data slice (original behavior)
        extract_canon_i16_array_with_base(entry, data, byte_order, 0)
    }
}

/// Extracts a string value from a Canon MakerNote IFD entry.
///
/// Canon MakerNotes use TIFF-relative offsets, so we need to calculate the
/// base offset and subtract it from the value_offset to get the position
/// within the MakerNote data slice.
///
/// # Parameters
/// - `entry`: The IFD entry containing the tag metadata
/// - `data`: The MakerNote data slice (after any signature)
/// - `byte_order`: Byte order for parsing
/// - `base_offset`: The calculated base offset to subtract from value_offset
///
/// # Returns
/// The extracted string value, or None if extraction fails
fn extract_canon_string_with_base(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
    base_offset: u32,
) -> Option<String> {
    // Only handle ASCII type (2)
    if entry.field_type != 2 {
        return None;
    }

    let byte_count = entry.value_count as usize;
    if byte_count == 0 {
        return None;
    }

    // For inline strings (<=4 bytes), value is stored in value_offset field
    if byte_count <= 4 {
        let bytes = match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };
        let s = String::from_utf8_lossy(&bytes[..byte_count])
            .trim_end_matches('\0')
            .trim()
            .to_string();
        return if s.is_empty() { None } else { Some(s) };
    }

    // For offset-based strings, adjust the offset using base_offset
    let tiff_offset = entry.value_offset;
    if tiff_offset < base_offset {
        // Offset is before MakerNote start - might be inline or invalid
        return None;
    }

    let relative_offset = (tiff_offset - base_offset) as usize;
    if relative_offset + byte_count > data.len() {
        return None;
    }

    let bytes = &data[relative_offset..relative_offset + byte_count];
    // A TIFF ASCII value ends at its first NUL; Canon pads the remainder of the fixed
    // 32-byte OwnerName/ImageType slots with unrelated bytes, so trimming only trailing
    // NULs leaks that padding into the value (ExifTool prints "unknown", not
    // "unknown\0\x01\0\0\0<...").
    let terminated = match bytes.iter().position(|&b| b == 0) {
        Some(nul) => &bytes[..nul],
        None => bytes,
    };
    let s = String::from_utf8_lossy(terminated).trim().to_string();

    if s.is_empty() { None } else { Some(s) }
}

/// Extracts a string value from a Canon MakerNote IFD entry.
///
/// This is a convenience wrapper that calculates the base offset automatically.
fn extract_canon_string(entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> Option<String> {
    if let Some(base) = calculate_makernote_base(data, byte_order) {
        extract_canon_string_with_base(entry, data, byte_order, base)
    } else {
        // Fallback: try with base_offset = 0 (original behavior)
        extract_canon_string_with_base(entry, data, byte_order, 0)
    }
}

/// Renders a bitmask the way ExifTool's `DecodeBits($val, undef, 16)` does.
///
/// Reference: `Image::ExifTool::DecodeBits` (ExifTool.pm:6362). With no lookup table the
/// set bit numbers are emitted in ascending order joined by `,`, words are 16 bits wide
/// and word `w` contributes bits `w*16 .. w*16+15`, and an empty result prints `(none)`:
///
/// ```text
///     return '(none)' unless @bitList;
///     return join($lookup ? ', ' : ',', @bitList);
/// ```
fn decode_bits_16(words: &[i16]) -> String {
    let mut bits: Vec<String> = Vec::new();
    for (word_index, &word) in words.iter().enumerate() {
        for bit in 0..16usize {
            if (word as u16) & (1u16 << bit) != 0 {
                bits.push((word_index * 16 + bit).to_string());
            }
        }
    }
    if bits.is_empty() {
        "(none)".to_string()
    } else {
        bits.join(",")
    }
}

/// Joins a slice of AF coordinates the way ExifTool prints an `int16s[n]` value.
fn join_i16_slice(values: &[i16]) -> String {
    values
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Drops the stray leading element from a Canon binary record that was read one slot early.
///
/// Every `%Canon` record with `FIRST_ENTRY => 1` (CameraSettings, ShotInfo, Processing,
/// SensorInfo, MeasuredColor, FileInfo) and every `ColorData*` table opens with its own
/// size in bytes; ExifTool spells that out at Canon.pm:7444 — `# 0x00: size of record in
/// bytes`. A correctly located record therefore always satisfies
/// `record[0] == record.len() * 2`.
///
/// On a large share of Canon JPEGs the MakerNote base this parser computes lands two bytes
/// low, so the size word turns up at index 1 and every documented index is shifted by one.
/// That single fault is why a 20D reported `SensorWidth 34` — the SensorInfo record's own
/// byte count — instead of 3596, and why `ControlMode` read the slot before the real one.
/// When the size word is unambiguously at index 1, dropping the leading element restores
/// ExifTool's numbering for every index the callers below use.
///
/// The test is deliberately one-sided: when index 0 already holds the size the record is
/// returned untouched, so a correctly read record can never be shifted by this function.
/// Realignment costs the record's final element, which no index used here reaches.
fn realign_length_prefixed_record(array: Vec<i16>) -> Vec<i16> {
    let declares_size =
        |slot: usize| array.get(slot).map(|&v| v as u16 as usize) == Some(array.len() * 2);
    if !declares_size(0) && declares_size(1) {
        return array[1..].to_vec();
    }
    array
}

// Canon MakerNote Tag IDs
const CANON_CAMERA_SETTINGS: u16 = 0x0001;
const CANON_FOCAL_LENGTH: u16 = 0x0002;
const CANON_SHOT_INFO: u16 = 0x0004;
const CANON_PANORAMA: u16 = 0x0005;
const CANON_IMAGE_TYPE: u16 = 0x0006;
const CANON_FIRMWARE_VERSION: u16 = 0x0007;
const CANON_FILE_NUMBER: u16 = 0x0008;
const CANON_OWNER_NAME: u16 = 0x0009;
const CANON_SERIAL_NUMBER: u16 = 0x000C;
const CANON_CAMERA_INFO: u16 = 0x000D;
const CANON_CUSTOM_FUNCTIONS: u16 = 0x000F;
const CANON_MODEL_ID: u16 = 0x0010;
const CANON_FLASH_INFO: u16 = 0x0003;
const CANON_AF_INFO: u16 = 0x0012;
const CANON_SERIAL_NUMBER_FORMAT: u16 = 0x0015;
const CANON_AF_INFO2: u16 = 0x0026;
const CANON_FILE_INFO: u16 = 0x0093;
const CANON_LENS_MODEL: u16 = 0x0095;
const CANON_INTERNAL_SERIAL_NUMBER: u16 = 0x0096;
const CANON_PROCESSING_INFO: u16 = 0x00A0;
const CANON_MEASURED_COLOR: u16 = 0x00AA;
const CANON_COLOR_SPACE: u16 = 0x00B4;
const CANON_VRD_OFFSET: u16 = 0x00D0;
/// ExifTool Canon.pm:1965 — `0xe0 => { Name => 'SensorInfo', ... }`
const CANON_SENSOR_INFO: u16 = 0x00E0;
/// ExifTool Canon.pm:1607 — `0x13 => { Name => 'ThumbnailImageValidArea', ... }`
const CANON_THUMBNAIL_IMAGE_VALID_AREA: u16 = 0x0013;
/// ExifTool Canon.pm:1785 — `0x83 => { Name => 'OriginalDecisionDataOffset', ... }`
const CANON_ORIGINAL_DECISION_DATA_OFFSET: u16 = 0x0083;
/// ExifTool Canon.pm:1972 — `0x4001 => [ ... ColorData1..ColorData12 ... ]`
const CANON_COLOR_DATA: u16 = 0x4001;

// Canon signature (not always present)
const CANON_SIGNATURE: &[u8] = b"Canon";

// CameraSettings array (tag 0x0001) indices
// Array contains ~50 values with camera settings
// Reference: ExifTool Canon.pm CameraSettings table
const CAMERA_SETTINGS_MACRO_MODE: usize = 1;
const CAMERA_SETTINGS_SELF_TIMER: usize = 2;
const CAMERA_SETTINGS_QUALITY: usize = 3;
const CAMERA_SETTINGS_FLASH_MODE: usize = 4;
const CAMERA_SETTINGS_DRIVE_MODE: usize = 5;
const CAMERA_SETTINGS_FOCUS_MODE: usize = 7;
const CAMERA_SETTINGS_RECORD_MODE: usize = 9;
const CAMERA_SETTINGS_IMAGE_SIZE: usize = 10;
const CAMERA_SETTINGS_EASY_MODE: usize = 11;
const CAMERA_SETTINGS_DIGITAL_ZOOM: usize = 12;
const CAMERA_SETTINGS_CONTRAST: usize = 13;
const CAMERA_SETTINGS_SATURATION: usize = 14;
const CAMERA_SETTINGS_SHARPNESS: usize = 15;
const CAMERA_SETTINGS_ISO: usize = 16;
const CAMERA_SETTINGS_METERING_MODE: usize = 17;
const CAMERA_SETTINGS_FOCUS_RANGE: usize = 18;
// Alias for backward compatibility with tests
const CAMERA_SETTINGS_FOCUS_TYPE: usize = 18;
const CAMERA_SETTINGS_AF_POINT: usize = 19;
const CAMERA_SETTINGS_EXPOSURE_MODE: usize = 20;
const CAMERA_SETTINGS_LENS_TYPE: usize = 22;
const CAMERA_SETTINGS_MAX_FOCAL_LENGTH: usize = 23;
const CAMERA_SETTINGS_MIN_FOCAL_LENGTH: usize = 24;
const CAMERA_SETTINGS_FOCAL_UNITS: usize = 25;
const CAMERA_SETTINGS_MAX_APERTURE: usize = 26;
const CAMERA_SETTINGS_MIN_APERTURE: usize = 27;
/// ExifTool `%Canon::CameraSettings` key 28 (Canon.pm:2553) — `Name => 'FlashModel'`,
/// `Mask => 0x7f`, `RawConv => '$val == 127 ? undef : $val'`. There is no `FlashActivity`
/// key in this table.
const CAMERA_SETTINGS_FLASH_MODEL: usize = 28;
const CAMERA_SETTINGS_FLASH_BITS: usize = 29;
const CAMERA_SETTINGS_FOCUS_CONTINUOUS: usize = 32;
const CAMERA_SETTINGS_AE_SETTING: usize = 33;
/// ExifTool `%Canon::CameraSettings` key 35 (Canon.pm:2645) — `DisplayAperture`, not 40.
const CAMERA_SETTINGS_DISPLAY_APERTURE: usize = 35;
const CAMERA_SETTINGS_ZOOM_SOURCE_WIDTH: usize = 36;
const CAMERA_SETTINGS_ZOOM_TARGET_WIDTH: usize = 37;
const CAMERA_SETTINGS_SPOT_METERING_MODE: usize = 39;
/// ExifTool `%Canon::CameraSettings` key 40 (Canon.pm:2651) — `PhotoEffect`.
const CAMERA_SETTINGS_PHOTO_EFFECT: usize = 40;
/// ExifTool `%Canon::CameraSettings` key 41 (Canon.pm:2668) — `ManualFlashOutput`.
const CAMERA_SETTINGS_MANUAL_FLASH_OUTPUT: usize = 41;
/// ExifTool `%Canon::CameraSettings` key 42 (Canon.pm:2681) — `ColorTone`.
const CAMERA_SETTINGS_COLOR_TONE: usize = 42;

// ShotInfo array (tag 0x0004) indices
// Reference: ExifTool Canon.pm ShotInfo table
const SHOT_INFO_AUTO_ISO: usize = 1;
const SHOT_INFO_BASE_ISO: usize = 2;
const SHOT_INFO_MEASURED_EV: usize = 3;
const SHOT_INFO_TARGET_APERTURE: usize = 4;
const SHOT_INFO_TARGET_EXPOSURE_TIME: usize = 5;
// Alias for backward compatibility with tests
const SHOT_INFO_TARGET_SHUTTER_SPEED: usize = 5;
const SHOT_INFO_EXPOSURE_COMPENSATION: usize = 6;
const SHOT_INFO_WHITE_BALANCE: usize = 7;
const SHOT_INFO_SLOW_SHUTTER: usize = 8;
const SHOT_INFO_SEQUENCE_NUMBER: usize = 9;
const SHOT_INFO_OPTICAL_ZOOM_CODE: usize = 10;
const SHOT_INFO_FLASH_GUIDE_NUMBER: usize = 13;
const SHOT_INFO_AF_POINTS_IN_FOCUS: usize = 14;
// Alias for backward compatibility with tests
const SHOT_INFO_AF_POINTS_USED: usize = 14;
const SHOT_INFO_FLASH_EXPOSURE_COMP: usize = 15;
const SHOT_INFO_AUTO_EXPOSURE_BRACKETING: usize = 16;
const SHOT_INFO_AEB_BRACKET_VALUE: usize = 17;
const SHOT_INFO_CONTROL_MODE: usize = 18;
const SHOT_INFO_FOCUS_DISTANCE_UPPER: usize = 19;
// Alias for backward compatibility with tests
const SHOT_INFO_SUBJECT_DISTANCE: usize = 19;
const SHOT_INFO_FOCUS_DISTANCE_LOWER: usize = 20;
/// ExifTool `%Canon::ShotInfo` key 21 (Canon.pm:2956) — `FNumber`.
const SHOT_INFO_FNUMBER: usize = 21;
/// ExifTool `%Canon::ShotInfo` key 22 (Canon.pm:2965) — `ExposureTime`.
const SHOT_INFO_EXPOSURE_TIME: usize = 22;
/// ExifTool `%Canon::ShotInfo` key 23 (Canon.pm:3001) — `MeasuredEV2`.
const SHOT_INFO_MEASURED_EV2: usize = 23;
const SHOT_INFO_BULB_DURATION: usize = 24;
/// ExifTool `%Canon::ShotInfo` key 27 — `Name => 'AutoRotate'` (Canon.pm:3022).
const SHOT_INFO_AUTO_ROTATE: usize = 27;
/// ExifTool `%Canon::ShotInfo` key 28 (Canon.pm:3033) — `NDFilter`.
const SHOT_INFO_ND_FILTER: usize = 28;
/// ExifTool `%Canon::ShotInfo` key 29 (Canon.pm:3037) — `SelfTimer2`.
const SHOT_INFO_SELF_TIMER2: usize = 29;

// FileInfo array indices (tag 0x0093)
//
// ExifTool `%Image::ExifTool::Canon::FileInfo` (Canon.pm:6842), `FORMAT => 'int16s'`:
//
// ```text
//     1 => [ { Name => 'FileNumber', ... Format => 'int32u', ... } ... ],
//     3 => { Name => 'BracketMode', PrintConv => { 0 => 'Off', 1 => 'AEB', ... } },
//     4 => 'BracketValue', #PH
//     5 => 'BracketShotNumber', #PH
// ```
const FILE_INFO_FILE_NUMBER: usize = 1;
// NOTE: the two indices below are a legacy heuristic that has no counterpart in
// `%Canon::FileInfo` (key 1 is a 4-byte int32u spanning int16 slots 1-2, and slot 3 is
// BracketMode). Kept for the models where oxidex has no better source, but suppressed
// on the bodies where key 1 is known to be FileNumber.
const FILE_INFO_SHUTTER_COUNT_LOW: usize = 2;
const FILE_INFO_SHUTTER_COUNT_HIGH: usize = 3;
const FILE_INFO_BRACKET_MODE: usize = 3;
const FILE_INFO_BRACKET_VALUE: usize = 4;
const FILE_INFO_BRACKET_SHOT_NUMBER: usize = 5;
/// ExifTool `%Canon::FileInfo` key 8 (Canon.pm:6948) — `LongExposureNoiseReduction2`.
const FILE_INFO_LONG_EXPOSURE_NR2: usize = 8;
/// ExifTool `%Canon::FileInfo` key 9 (Canon.pm:6963) — `WBBracketMode`.
const FILE_INFO_WB_BRACKET_MODE: usize = 9;
/// ExifTool `%Canon::FileInfo` key 12 (Canon.pm:6971) — `WBBracketValueAB`.
const FILE_INFO_WB_BRACKET_VALUE_AB: usize = 12;
/// ExifTool `%Canon::FileInfo` key 13 (Canon.pm:6972) — `WBBracketValueGM`.
const FILE_INFO_WB_BRACKET_VALUE_GM: usize = 13;
/// ExifTool `%Canon::FileInfo` key 14 (Canon.pm:6973) — `FilterEffect`.
const FILE_INFO_FILTER_EFFECT: usize = 14;
/// ExifTool `%Canon::FileInfo` key 15 (Canon.pm:6984) — `ToningEffect`.
const FILE_INFO_TONING_EFFECT: usize = 15;

// SensorInfo array indices (tag 0x00E0)
//
// ExifTool `%Image::ExifTool::Canon::SensorInfo` (Canon.pm:7409), `FORMAT => 'int16s'`,
// `FIRST_ENTRY => 1` — entry N lives at byte offset 2*N, so the raw int16 index equals
// the Perl key:
//
// ```text
//     9 => { Name => 'BlackMaskLeftBorder', ... },
//     10 => 'BlackMaskTopBorder', #22
//     11 => 'BlackMaskRightBorder', #22
//     12 => 'BlackMaskBottomBorder', #22
// ```
const SENSOR_INFO_SENSOR_WIDTH: usize = 1;
const SENSOR_INFO_SENSOR_HEIGHT: usize = 2;
const SENSOR_INFO_SENSOR_LEFT_BORDER: usize = 5;
const SENSOR_INFO_SENSOR_TOP_BORDER: usize = 6;
const SENSOR_INFO_SENSOR_RIGHT_BORDER: usize = 7;
const SENSOR_INFO_SENSOR_BOTTOM_BORDER: usize = 8;
const SENSOR_INFO_BLACK_MASK_LEFT_BORDER: usize = 9;
const SENSOR_INFO_BLACK_MASK_TOP_BORDER: usize = 10;
const SENSOR_INFO_BLACK_MASK_RIGHT_BORDER: usize = 11;
const SENSOR_INFO_BLACK_MASK_BOTTOM_BORDER: usize = 12;

// AFInfo sequence indices (tag 0x0012)
//
// ExifTool `%Image::ExifTool::Canon::AFInfo` (Canon.pm:6432) is a *serial* record
// (`PROCESS_PROC => \&ProcessSerialData`, `FORMAT => 'int16u'`) with no leading length
// word (Canon.pm:1602 "this record does not begin with a length word"). Keys 0..7 are
// scalars, so the raw int16 index equals the Perl key up to and including key 7:
//
// ```text
//     0 => { Name => 'NumAFPoints', },
//     1 => { Name => 'ValidAFPoints', ... },
//     2 => { Name => 'CanonImageWidth', ... },
//     3 => { Name => 'CanonImageHeight', ... },
//     4 => { Name => 'AFImageWidth', ... },
//     5 => 'AFImageHeight',
//     6 => 'AFAreaWidth',
//     7 => 'AFAreaHeight',
//     8 => { Name => 'AFAreaXPositions', Format => 'int16s[$val{0}]', },
//     9 => { Name => 'AFAreaYPositions', Format => 'int16s[$val{0}]', },
//     10 => { Name => 'AFPointsInFocus', Format => 'int16s[int(($val{0}+15)/16)]', ... },
// ```
const AF_INFO_NUM_AF_POINTS: usize = 0;
const AF_INFO_VALID_AF_POINTS: usize = 1;
const AF_INFO_CANON_IMAGE_WIDTH: usize = 2;
const AF_INFO_CANON_IMAGE_HEIGHT: usize = 3;
const AF_INFO_AF_IMAGE_WIDTH: usize = 4;
const AF_INFO_AF_IMAGE_HEIGHT: usize = 5;
const AF_INFO_AF_AREA_WIDTH: usize = 6;
const AF_INFO_AF_AREA_HEIGHT: usize = 7;
/// First variable-length slot of `%Canon::AFInfo` (Perl key 8, `AFAreaXPositions`).
const AF_INFO_VARIABLE_START: usize = 8;

// AFInfo2 sequence indices (tag 0x0026)
//
// ExifTool `%Image::ExifTool::Canon::AFInfo2` (Canon.pm:6503), also serial
// (`PROCESS_PROC => \&ProcessSerialData`, `FORMAT => 'int16u'`). Keys 0..7 are scalars:
//
// ```text
//     0 => { Name => 'AFInfoSize', Unknown => 1, ... },
//     1 => { Name => 'AFAreaMode', PrintConv => { ... } },
//     2 => { Name => 'NumAFPoints', RawConv => '$$self{NumAFPoints} = $val', },
//     3 => { Name => 'ValidAFPoints', ... },
//     4 => { Name => 'CanonImageWidth', ... },
//     5 => { Name => 'CanonImageHeight', ... },
//     6 => { Name => 'AFImageWidth', ... },
//     7 => 'AFImageHeight',
//     8 => { Name => 'AFAreaWidths', Format => 'int16s[$val{2}]', },
//     9 => { Name => 'AFAreaHeights', Format => 'int16s[$val{2}]', },
//     10 => { Name => 'AFAreaXPositions', Format => 'int16s[$val{2}]', },
//     11 => { Name => 'AFAreaYPositions', Format => 'int16s[$val{2}]', },
//     12 => { Name => 'AFPointsInFocus', Format => 'int16s[int(($val{2}+15)/16)]', ... },
// ```
const AF_INFO2_AF_AREA_MODE: usize = 1;
const AF_INFO2_NUM_AF_POINTS: usize = 2;
const AF_INFO2_AF_IMAGE_WIDTH: usize = 6;
const AF_INFO2_AF_IMAGE_HEIGHT: usize = 7;
/// First variable-length slot of `%Canon::AFInfo2` (Perl key 8, `AFAreaWidths`).
const AF_INFO2_VARIABLE_START: usize = 8;

// FlashInfo array indices (tag 0x0003)
const FLASH_INFO_FLASH_GUIDE_NUMBER: usize = 0;
const FLASH_INFO_FLASH_THRESHOLD: usize = 1;

// ProcessingInfo array indices (tag 0x00A0)
const PROCESSING_INFO_TONE_CURVE: usize = 1;
const PROCESSING_INFO_SHARPNESS: usize = 2;
const PROCESSING_INFO_SHARPNESS_FREQ: usize = 3;
const PROCESSING_INFO_SENSOR_RED_LEVEL: usize = 4;
const PROCESSING_INFO_SENSOR_BLUE_LEVEL: usize = 5;
const PROCESSING_INFO_WHITE_BALANCE_RED: usize = 6;
const PROCESSING_INFO_WHITE_BALANCE_BLUE: usize = 7;
const PROCESSING_INFO_WHITE_BALANCE: usize = 8;
const PROCESSING_INFO_COLOR_TEMPERATURE: usize = 9;
const PROCESSING_INFO_PICTURE_STYLE: usize = 10;
const PROCESSING_INFO_DIGITAL_GAIN: usize = 11;
const PROCESSING_INFO_WB_SHIFT_AB: usize = 12;
const PROCESSING_INFO_WB_SHIFT_GM: usize = 13;

// MeasuredColor array indices (tag 0x00AA)
//
// ExifTool `%Image::ExifTool::Canon::MeasuredColor` (Canon.pm:7294), `FORMAT => 'int16u'`,
// `FIRST_ENTRY => 1` — the only named key is a 4-element array:
//
// ```text
//     1 => { Name => 'MeasuredRGGB', Format => 'int16u[4]' },
// ```
const MEASURED_COLOR_RGGB: usize = 1;

// ColorData1 indices (tag 0x4001 with an element count of 582 — 20D and 350D)
//
// ExifTool `%Image::ExifTool::Canon::ColorData1` (Canon.pm:7435), `FORMAT => 'int16s'`,
// `FIRST_ENTRY => 0`, so the raw int16 index equals the Perl key. Every `WB_RGGBLevels*`
// key is `Format => 'int16s[4]'` and each is followed 4 slots later by its `ColorTemp*`.
const COLOR_DATA1_ELEMENT_COUNT: usize = 582;
/// `(WB_RGGBLevels* index, ColorTemp* index, suffix)` for each preset in ColorData1.
const COLOR_DATA1_WB_PRESETS: &[(usize, usize, &str)] = &[
    (0x19, 0x1d, "AsShot"),
    (0x1e, 0x22, "Auto"),
    (0x23, 0x27, "Daylight"),
    (0x28, 0x2c, "Shade"),
    (0x2d, 0x31, "Cloudy"),
    (0x32, 0x36, "Tungsten"),
    (0x37, 0x3b, "Fluorescent"),
    (0x3c, 0x40, "Flash"),
    (0x41, 0x45, "Custom1"),
    (0x46, 0x4a, "Custom2"),
];

// ============================================================================
// DECODERS - Canon Value Decoders
// ============================================================================
// Using const_decoder! macro for declarative, zero-overhead value decoding

// Canon macro mode decoder
// Used for MacroMode in CameraSettings (index 1)
// Reference: ExifTool Canon.pm MacroMode table
// Value 0 = "Off" (no macro), 1 = "Macro" (macro mode active), 2 = "Normal"
// Public to allow re-use in registry module
const_decoder!(pub MACRO_MODE, i16, [(0, "Off"), (1, "Macro"), (2, "Normal"),]);

// Canon quality setting decoder
// Public to allow re-use in registry module
const_decoder!(
    pub QUALITY,
    i16,
    [
        (-1, "n/a"),
        (1, "Economy"),
        (2, "Normal"),
        (3, "Fine"),
        (4, "RAW"),
        (5, "Superfine"),
        (7, "CRAW"),
        (130, "Normal Movie"),
        (131, "Movie (2)"),
        (132, "Movie (3)"),
        (133, "Movie (4)"),
    ]
);

// Canon flash mode decoder
// Public to allow re-use in registry module
const_decoder!(
    pub FLASH_MODE,
    i16,
    [
        (0, "Off"),
        (1, "Auto"),
        (2, "On"),
        (3, "Red-eye Reduction"),
        (4, "Slow Sync"),
        (5, "Auto + Red-eye Reduction"),
        (6, "On + Red-eye Reduction"),
        (16, "External Flash"),
    ]
);

// Canon drive mode decoder
// Public to allow re-use in registry module
const_decoder!(
    pub DRIVE_MODE,
    i16,
    [
        (0, "Single"),
        (1, "Continuous"),
        (2, "Movie"),
        (4, "Continuous, Speed Priority"),
        (5, "Continuous, Low"),
        (6, "Continuous, High"),
    ]
);

// Canon focus mode decoder
// Public to allow re-use in registry module
const_decoder!(
    pub FOCUS_MODE,
    i16,
    [
        (0, "One-shot AF"),
        (1, "AI Servo AF"),
        (2, "AI Focus AF"),
        (3, "Manual Focus (3)"),
        (4, "Single"),
        (5, "Continuous"),
        (6, "Manual Focus (6)"),
        (16, "Pan Focus"),
    ]
);

// Canon metering mode decoder
// Public to allow re-use in registry module
const_decoder!(
    pub METERING_MODE,
    i16,
    [
        (3, "Evaluative"),
        (4, "Partial"),
        (5, "Center-weighted Average"),
    ]
);

// Canon exposure mode decoder
//
// ExifTool `%Image::ExifTool::Canon::CameraSettings` key 20 (Canon.pm:2485). The labels
// are ExifTool's verbatim - "Shutter speed priority AE" and "Aperture-priority AE", not
// the shortened forms other manufacturers use.
const_decoder!(
    pub EXPOSURE_MODE,
    i16,
    [
        (0, "Easy"),
        (1, "Program AE"),
        (2, "Shutter speed priority AE"),
        (3, "Aperture-priority AE"),
        (4, "Manual"),
        (5, "Depth-of-field AE"),
        (6, "M-Dep"),
        (7, "Bulb"),
        (8, "Flexible-priority AE"),
    ]
);

// Canon color space decoder
// Used for ColorSpace tag (0x00B4)
const_decoder!(
    pub COLOR_SPACE,
    i32,
    [
        (1, "sRGB"),
        (2, "Adobe RGB"),
        (65535, "Uncalibrated"),
    ]
);

// Canon picture style decoder
// Used for PictureStyle in ProcessingInfo
const_decoder!(
    pub PICTURE_STYLE,
    i32,
    [
        // ExifTool `%pictureStyles` (Canon.pm:1118) starts at 0x00 - the "ColorMatrix"
        // codes below 0x21 are part of the same table.
        (0x0000, "None"),
        (0x0001, "Standard"),
        (0x0002, "Portrait"),
        (0x0003, "High Saturation"),
        (0x0004, "Adobe RGB"),
        (0x0005, "Low Saturation"),
        (0x0006, "CM Set 1"),
        (0x0007, "CM Set 2"),
        (0x0021, "User Def. 1"),
        (0x0022, "User Def. 2"),
        (0x0023, "User Def. 3"),
        (0x0041, "PC 1"),
        (0x0042, "PC 2"),
        (0x0043, "PC 3"),
        (0x0081, "Standard"),
        (0x0082, "Portrait"),
        (0x0083, "Landscape"),
        (0x0084, "Neutral"),
        (0x0085, "Faithful"),
        (0x0086, "Monochrome"),
        (0x0087, "Auto"),
        (0x0088, "Fine Detail"),
        (0x00ff, "n/a"),
        (0xffff, "n/a"),
    ]
);

// Canon tone curve decoder
// Used for ToneCurve in ProcessingInfo
const_decoder!(
    pub TONE_CURVE,
    i32,
    [
        (0, "Standard"),
        (1, "Manual"),
        (2, "Custom"),
    ]
);

// Canon record mode decoder
// Used for RecordMode in CameraSettings (index 9)
const_decoder!(
    pub RECORD_MODE,
    i16,
    [
        (0, "n/a"),
        (1, "JPEG"),
        (2, "CRW+THM"),
        (3, "AVI+THM"),
        (4, "TIF"),
        (5, "TIF+JPEG"),
        (6, "CR2"),
        (7, "CR2+JPEG"),
        (9, "MOV"),
        (10, "MP4"),
        (11, "CRM"),
        (12, "CR3"),
        (13, "CR3+JPEG"),
        (14, "HIF"),
        (15, "CR3+HIF"),
    ]
);

// Canon image size decoder
// Used for CanonImageSize in CameraSettings (index 10)
const_decoder!(
    pub CANON_IMAGE_SIZE,
    i16,
    [
        (-1, "n/a"),
        (0, "Large"),
        (1, "Medium"),
        (2, "Small"),
        (5, "Medium 1"),
        (6, "Medium 2"),
        (7, "Medium 3"),
        (8, "Postcard"),
        (9, "Widescreen"),
        (10, "Medium Widescreen"),
        (14, "Small 1"),
        (15, "Small 2"),
        (16, "Small 3"),
        (128, "640x480 Movie"),
        (129, "Medium Movie"),
        (130, "Small Movie"),
        (137, "1280x720 Movie"),
        (142, "1920x1080 Movie"),
        (143, "4096x2160 Movie"),
    ]
);

// Canon easy mode decoder (scene modes)
// Used for EasyMode in CameraSettings (index 11)
const_decoder!(
    pub EASY_MODE,
    i16,
    [
        (0, "Full Auto"),
        (1, "Manual"),
        (2, "Landscape"),
        (3, "Fast Shutter"),
        (4, "Slow Shutter"),
        (5, "Night"),
        (6, "Gray Scale"),
        (7, "Sepia"),
        (8, "Portrait"),
        (9, "Sports"),
        (10, "Macro"),
        (11, "Black & White"),
        (12, "Pan Focus"),
        (13, "Vivid"),
        (14, "Neutral"),
        (15, "Flash Off"),
        (16, "Long Shutter"),
        (17, "Super Macro"),
        (18, "Foliage"),
        (19, "Indoor"),
        (20, "Fireworks"),
        (21, "Beach"),
        (22, "Underwater"),
        (23, "Snow"),
        (24, "Kids & Pets"),
        (25, "Night Snapshot"),
        (26, "Digital Macro"),
        (27, "My Colors"),
        (28, "Movie Snap"),
        (29, "Super Macro 2"),
        (30, "Color Accent"),
        (31, "Color Swap"),
        (32, "Aquarium"),
        (33, "ISO 3200"),
        (34, "ISO 6400"),
        (35, "Creative Light Effect"),
        (36, "Easy"),
        (37, "Quick Shot"),
        (38, "Creative Auto"),
        (39, "Zoom Blur"),
        (40, "Low Light"),
        (41, "Nostalgic"),
        (42, "Super Vivid"),
        (43, "Poster Effect"),
        (44, "Face Self-timer"),
        (45, "Smile"),
        (46, "Wink Self-timer"),
        (47, "Fisheye Effect"),
        (48, "Miniature Effect"),
        (49, "High-speed Burst"),
        (50, "Best Image Selection"),
        (51, "High Dynamic Range"),
        (52, "Handheld Night Scene"),
        (53, "Movie Digest"),
        (54, "Live View Control"),
        (55, "Discreet"),
        (56, "Blur Reduction"),
        (57, "Monochrome"),
        (58, "Toy Camera Effect"),
        (59, "Scene Intelligent Auto"),
        (60, "High-speed Burst HQ"),
        (61, "Smooth Skin"),
        (62, "Soft Focus"),
        (257, "Spotlight"),
        (258, "Night 2"),
        (259, "Night+"),
        (260, "Super Night"),
        (261, "Sunset"),
        (263, "Night Scene"),
        (264, "Surface"),
        (265, "Low Light 2"),
    ]
);

// Canon digital zoom decoder
// Used for DigitalZoom in CameraSettings (index 12)
// Reference: ExifTool Canon.pm DigitalZoom table
// Note: -1 indicates "Off" (not available), 0 indicates "None" (not used)
const_decoder!(
    pub DIGITAL_ZOOM,
    i16,
    [
        (-1, "Off"),
        (0, "None"),
        (1, "2x"),
        (2, "4x"),
        (3, "Other"),
    ]
);

// Canon focus range decoder
// Used for FocusRange in CameraSettings (index 18)
const_decoder!(
    pub FOCUS_RANGE,
    i16,
    [
        (0, "Manual"),
        (1, "Auto"),
        (2, "Not Known"),
        (3, "Macro"),
        (4, "Very Close"),
        (5, "Close"),
        (6, "Middle Range"),
        (7, "Far Range"),
        (8, "Pan Focus"),
        (9, "Super Macro"),
        (10, "Infinity"),
    ]
);

// Canon AF point selected decoder
// Used for AFPoint in CameraSettings (index 19)
const_decoder!(
    pub AF_POINT,
    i16,
    [
        (0x2005, "Manual AF point selection"),
        (0x3000, "None (MF)"),
        (0x3001, "Auto AF point selection"),
        (0x3002, "Right"),
        (0x3003, "Center"),
        (0x3004, "Left"),
        (0x4001, "Auto AF point selection"),
        (0x4006, "Face Detect"),
    ]
);

// Canon AE setting decoder
// Used for AESetting in CameraSettings (index 33)
const_decoder!(
    pub AE_SETTING,
    i16,
    [
        (0, "Normal AE"),
        (1, "Exposure Compensation"),
        (2, "AE Lock"),
        (3, "AE Lock + Exposure Compensation"),
        (4, "No AE"),
    ]
);

// Canon spot metering mode decoder
// Used for SpotMeteringMode in CameraSettings (index 39)
const_decoder!(
    pub SPOT_METERING_MODE,
    i16,
    [
        (0, "Center"),
        (1, "AF Point"),
    ]
);

// Canon focus continuous decoder
// Used for FocusContinuous in CameraSettings (index 32)
const_decoder!(
    pub FOCUS_CONTINUOUS,
    i16,
    [
        (0, "Single"),
        (1, "Continuous"),
        (8, "Manual"),
    ]
);

// Canon flash bits bitfield decoder
// Used for FlashBits in CameraSettings (index 29)
// Each bit represents a flash feature/state
bitfield_decoder!(
    pub FLASH_BITS,
    [
        (0x0001, "Manual"),
        (0x0002, "TTL"),
        (0x0004, "A-TTL"),
        (0x0008, "E-TTL"),
        (0x0010, "FP Sync"),
        (0x0020, "2nd Curtain"),
        (0x0040, "High-speed Sync"),
        (0x0080, "Built-in"),
        (0x0100, "External"),
    ]
);

// Canon slow shutter decoder
// Used for SlowShutter in ShotInfo (index 8)
const_decoder!(
    pub SLOW_SHUTTER,
    i16,
    [
        (0, "Off"),
        (1, "Night Scene"),
        (2, "On"),
        (3, "None"),
    ]
);

// Canon control mode decoder
//
// ExifTool `%Image::ExifTool::Canon::ShotInfo` key 18 (Canon.pm:2925):
//
// ```text
//     18 => { #22
//         Name => 'ControlMode',
//         PrintConv => {
//             0 => 'n/a',
//             1 => 'Camera Local Control',
//             # 2 - have seen this for EOS M studio picture
//             3 => 'Computer Remote Control',
//         },
//     },
// ```
const_decoder!(
    pub CONTROL_MODE,
    i16,
    [
        (0, "n/a"),
        (1, "Camera Local Control"),
        (3, "Computer Remote Control"),
    ]
);

// Canon external flash model decoder
//
// ExifTool `%flashModel` (Canon.pm:1028), used by `%Canon::CameraSettings` key 28 after
// masking with 0x7f. Code 127 is discarded by that key's `RawConv`, and code 1 is
// deliberately absent from the table upstream.
const_decoder!(
    pub FLASH_MODEL,
    i16,
    [
        (0, "n/a"),
        (4, "Speedlite 540EZ"),
        (5, "Speedlite 380EX"),
        (6, "Speedlite 550EX"),
        (8, "Speedlite ST-E2"),
        (9, "Speedlite MR-14EX"),
        (12, "Speedlite 580EX"),
        (13, "Speedlite 430EX"),
        (17, "Speedlite 580EX II"),
        (18, "Speedlite 430EX II"),
        (22, "Speedlite 600EX-RT"),
        (23, "Speedlite 600EX II-RT"),
        (24, "Speedlite 90EX"),
        (25, "Speedlite 430EX III-RT"),
        (31, "Speedlite EL-1 ver2"),
        (33, "Speedlite EL-5"),
        (34, "Speedlite EL-10"),
    ]
);

// Canon photo effect decoder
// ExifTool `%Canon::CameraSettings` key 40 (Canon.pm:2651)
const_decoder!(
    pub PHOTO_EFFECT,
    i16,
    [
        (0, "Off"),
        (1, "Vivid"),
        (2, "Neutral"),
        (3, "Smooth"),
        (4, "Sepia"),
        (5, "B&W"),
        (6, "Custom"),
        (100, "My Color Data"),
    ]
);

// Canon manual flash output decoder
// ExifTool `%Canon::CameraSettings` key 41 (Canon.pm:2668), `PrintHex => 1`
const_decoder!(
    pub MANUAL_FLASH_OUTPUT,
    i32,
    [
        (0, "n/a"),
        (0x500, "Full"),
        (0x502, "Medium"),
        (0x504, "Low"),
        (0x7fff, "n/a"),
    ]
);

// Canon sharpness frequency decoder
// ExifTool `%Canon::Processing` key 3 (Canon.pm:7220)
const_decoder!(
    pub SHARPNESS_FREQUENCY,
    i16,
    [
        (0, "n/a"),
        (1, "Lowest"),
        (2, "Low"),
        (3, "Standard"),
        (4, "High"),
        (5, "Highest"),
    ]
);

// Canon long-exposure noise reduction (second flavour) decoder
// ExifTool `%Canon::FileInfo` key 8 (Canon.pm:6948)
const_decoder!(
    pub LONG_EXPOSURE_NOISE_REDUCTION2,
    i16,
    [(0, "Off"), (1, "On (1D)"), (3, "On"), (4, "Auto"),]
);

// Canon white-balance bracket mode decoder
// ExifTool `%Canon::FileInfo` key 9 (Canon.pm:6963)
const_decoder!(
    pub WB_BRACKET_MODE,
    i16,
    [(0, "Off"), (1, "On (shift AB)"), (2, "On (shift GM)"),]
);

// Canon monochrome filter effect decoder
// ExifTool `%Canon::FileInfo` key 14 (Canon.pm:6973)
const_decoder!(
    pub FILTER_EFFECT,
    i16,
    [
        (0, "None"),
        (1, "Yellow"),
        (2, "Orange"),
        (3, "Red"),
        (4, "Green"),
    ]
);

// Canon monochrome toning effect decoder
// ExifTool `%Canon::FileInfo` key 15 (Canon.pm:6984)
const_decoder!(
    pub TONING_EFFECT,
    i16,
    [
        (0, "None"),
        (1, "Sepia"),
        (2, "Blue"),
        (3, "Purple"),
        (4, "Green"),
    ]
);

// Canon D30/D60/PowerShot AF-points-in-focus code decoder
//
// ExifTool `%Canon::ShotInfo` key 14 (Canon.pm:2884) is a `PrintHex` lookup, not a
// bitmask; its `RawConv` drops 0 so the caller must skip that value.
const_decoder!(
    pub SHOT_INFO_AF_POINTS_IN_FOCUS_CODES,
    i32,
    [
        (0x3000, "None (MF)"),
        (0x3001, "Right"),
        (0x3002, "Center"),
        (0x3003, "Center+Right"),
        (0x3004, "Left"),
        (0x3005, "Left+Right"),
        (0x3006, "Left+Center"),
        (0x3007, "All"),
    ]
);

// Canon ND filter decoder
// ExifTool `%Canon::ShotInfo` key 28 (Canon.pm:3033)
const_decoder!(pub ND_FILTER, i16, [(-1, "n/a"), (0, "Off"), (1, "On"),]);

// Canon auto exposure bracketing decoder
// ExifTool `%Canon::ShotInfo` key 16 (Canon.pm:2907)
const_decoder!(
    pub AUTO_EXPOSURE_BRACKETING,
    i16,
    [
        (-1, "On"),
        (0, "Off"),
        (1, "On (shot 1)"),
        (2, "On (shot 2)"),
        (3, "On (shot 3)"),
    ]
);

// Canon serial number display format decoder
// ExifTool Canon.pm:1615 — `0x15 => { Name => 'SerialNumberFormat', PrintHex => 1, ... }`
const_decoder!(
    pub SERIAL_NUMBER_FORMAT,
    i64,
    [(0x9000_0000, "Format 1"), (0xa000_0000, "Format 2"),]
);

// ----------------------------------------------------------------------------
// CanonCustom::Functions350D (CanonCustom.pm:809)
// ----------------------------------------------------------------------------
// Selected by Canon.pm:1542 when `$$self{Model} =~ /\b(350D|REBEL XT|Kiss Digital N)\b/`.
// Every key is an `int8u` produced by `ProcessCanonCustom` (CanonCustom.pm:2772), which
// reads one int16 per entry and splits it into `tag = $val >> 8` / `value = $val & 0xff`.

const_decoder!(
    pub CC350D_SET_BUTTON_CROSS_KEYS_FUNC,
    i16,
    [
        (0, "Normal"),
        (1, "Set: Quality"),
        (2, "Set: Parameter"),
        (3, "Set: Playback"),
        (4, "Cross keys: AF point select"),
    ]
);

const_decoder!(
    pub CC350D_LONG_EXPOSURE_NOISE_REDUCTION,
    i16,
    [(0, "Off"), (1, "On"),]
);

const_decoder!(
    pub CC350D_FLASH_SYNC_SPEED_AV,
    i16,
    [(0, "Auto"), (1, "1/200 Fixed"),]
);

const_decoder!(
    pub CC350D_SHUTTER_AE_LOCK,
    i16,
    [
        (0, "AF/AE lock"),
        (1, "AE lock/AF"),
        (2, "AF/AF lock, No AE lock"),
        (3, "AE/AF, No AE lock"),
    ]
);

const_decoder!(
    pub CC350D_AF_ASSIST_BEAM,
    i16,
    [
        (0, "Emits"),
        (1, "Does not emit"),
        (2, "Only ext. flash emits"),
    ]
);

const_decoder!(
    pub CC350D_EXPOSURE_LEVEL_INCREMENTS,
    i16,
    [(0, "1/3 Stop"), (1, "1/2 Stop"),]
);

const_decoder!(pub CC350D_MIRROR_LOCKUP, i16, [(0, "Disable"), (1, "Enable"),]);

const_decoder!(pub CC350D_ETTL_II, i16, [(0, "Evaluative"), (1, "Average"),]);

const_decoder!(
    pub CC350D_SHUTTER_CURTAIN_SYNC,
    i16,
    [(0, "1st-curtain sync"), (1, "2nd-curtain sync"),]
);

// Canon AutoRotate decoder
//
// ExifTool `%Image::ExifTool::Canon::ShotInfo` key 27 (Canon.pm:3022):
//
// ```text
//     27 => {
//         Name => 'AutoRotate',
//         RawConv => '$val >= 0 ? $val : undef',
//         PrintConv => {
//            -1 => 'n/a', # (set to -1 when rotated by Canon software)
//             0 => 'None',
//             1 => 'Rotate 90 CW',
//             2 => 'Rotate 180',
//             3 => 'Rotate 270 CW',
//         },
//     },
// ```
//
// The `-1 => 'n/a'` entry is unreachable: `RawConv` discards every negative value before
// `PrintConv` runs, so the caller must skip negatives rather than map them.
const_decoder!(
    pub AUTO_ROTATE,
    i16,
    [
        (0, "None"),
        (1, "Rotate 90 CW"),
        (2, "Rotate 180"),
        (3, "Rotate 270 CW"),
    ]
);

// Canon BracketMode decoder
//
// ExifTool `%Image::ExifTool::Canon::FileInfo` key 3 (Canon.pm:6929):
//
// ```text
//     3 => { #PH
//         Name => 'BracketMode',
//         PrintConv => {
//             0 => 'Off',
//             1 => 'AEB',
//             2 => 'FEB',
//             3 => 'ISO',
//             4 => 'WB',
//         },
//     },
// ```
const_decoder!(
    pub BRACKET_MODE,
    i16,
    [(0, "Off"), (1, "AEB"), (2, "FEB"), (3, "ISO"), (4, "WB"),]
);

// Canon AFAreaMode decoder (AFInfo2 key 1)
//
// ExifTool `%Image::ExifTool::Canon::AFInfo2` key 1 (Canon.pm:6517):
//
// ```text
//     1 => {
//         Name => 'AFAreaMode',
//         PrintConv => {
//             0 => 'Off (Manual Focus)',
//             1 => 'AF Point Expansion (surround)', #PH
//             2 => 'Single-point AF',
//             # 3 - n/a
//             4 => 'Auto', #forum6237 (AiAF on A570IS)
//             5 => 'Face Detect AF',
//             6 => 'Face + Tracking', #PH (NC, EOS M, live view)
//             7 => 'Zone AF', #46
//             8 => 'AF Point Expansion (4 point)', #46/PH/forum6237
//             9 => 'Spot AF', #46
//             10 => 'AF Point Expansion (8 point)', #forum6237
//             11 => 'Flexizone Multi (49 point)', #PH (NC, EOS M, live view; 750D 49 points)
//             12 => 'Flexizone Multi (9 point)', #PH (750D, 9 points)
//             13 => 'Flexizone Single', #PH (EOS M default, live view) ...
//             14 => 'Large Zone AF', #PH/forum6237 (7DmkII)
//             16 => 'Large Zone AF (vertical)', #forum16223
//             17 => 'Large Zone AF (horizontal)', #forum16223
//             19 => 'Flexible Zone AF 1', #github268 (R7)
//             20 => 'Flexible Zone AF 2', #github268 (R7)
//             21 => 'Flexible Zone AF 3', #github268 (R7)
//             22 => 'Whole Area AF', #github268 (R7)
//         },
//     },
// ```
const_decoder!(
    pub AF_AREA_MODE,
    i16,
    [
        (0, "Off (Manual Focus)"),
        (1, "AF Point Expansion (surround)"),
        (2, "Single-point AF"),
        (4, "Auto"),
        (5, "Face Detect AF"),
        (6, "Face + Tracking"),
        (7, "Zone AF"),
        (8, "AF Point Expansion (4 point)"),
        (9, "Spot AF"),
        (10, "AF Point Expansion (8 point)"),
        (11, "Flexizone Multi (49 point)"),
        (12, "Flexizone Multi (9 point)"),
        (13, "Flexizone Single"),
        (14, "Large Zone AF"),
        (16, "Large Zone AF (vertical)"),
        (17, "Large Zone AF (horizontal)"),
        (19, "Flexible Zone AF 1"),
        (20, "Flexible Zone AF 2"),
        (21, "Flexible Zone AF 3"),
        (22, "Whole Area AF"),
    ]
);

// Canon white balance decoder for ShotInfo
// More detailed than standard EXIF white balance
const_decoder!(
    pub WHITE_BALANCE,
    i16,
    [
        (0, "Auto"),
        (1, "Daylight"),
        (2, "Cloudy"),
        (3, "Tungsten"),
        (4, "Fluorescent"),
        (5, "Flash"),
        (6, "Custom"),
        (7, "Black & White"),
        (8, "Shade"),
        (9, "Manual Temperature (Kelvin)"),
        (10, "PC Set 1"),
        (11, "PC Set 2"),
        (12, "PC Set 3"),
        (14, "Daylight Fluorescent"),
        (15, "Custom 1"),
        (16, "Custom 2"),
        (17, "Underwater"),
        (18, "Custom 3"),
        (19, "Custom 4"),
        (20, "PC Set 4"),
        (21, "PC Set 5"),
        (23, "Auto (Ambience Priority)"),
    ]
);

// Canon Contrast decoder
// Used for Contrast in CameraSettings (index 13)
// Reference: ExifTool Canon.pm Contrast table
// Canon uses signed values: 0=Normal, negative=Low, positive=High
const_decoder!(
    pub CONTRAST,
    i16,
    [
        (-2, "Very Low"),
        (-1, "Low"),
        (0, "Normal"),
        (1, "High"),
        (2, "Very High"),
    ]
);

// Canon Saturation decoder
// Used for Saturation in CameraSettings (index 14)
// Reference: ExifTool Canon.pm Saturation table
// Canon uses signed values: 0=Normal, negative=Low, positive=High
const_decoder!(
    pub SATURATION,
    i16,
    [
        (-2, "Very Low"),
        (-1, "Low"),
        (0, "Normal"),
        (1, "High"),
        (2, "Very High"),
    ]
);

// Canon Sharpness decoder
// Used for Sharpness in CameraSettings (index 15)
// Reference: ExifTool Canon.pm Sharpness table
// Canon uses signed values: 0=Normal, negative=Soft, positive=Sharp
const_decoder!(
    pub SHARPNESS,
    i16,
    [
        (-2, "Very Soft"),
        (-1, "Soft"),
        (0, "Normal"),
        (1, "Sharp"),
        (2, "Very Sharp"),
    ]
);

// Canon FocalType decoder
// Used for FocalType in FocalLength array (index 0)
// Reference: ExifTool Canon.pm FocalType table
// Describes whether lens is fixed focal length or zoom
const_decoder!(
    pub FOCAL_TYPE,
    i16,
    [
        (0, "Unknown"),
        (1, "Fixed"),
        (2, "Zoom"),
        (3, "Fixed"),  // Alternative encoding for fixed lens
    ]
);

// ============================================================================
// APEX CONVERSION HELPERS
// ============================================================================
// Canon stores aperture and shutter speed values in APEX format.
// APEX (Additive System of Photographic Exposure) uses logarithmic scales.

/// Converts a Canon APEX aperture value to an f-number string.
///
/// Canon stores aperture as raw value that needs conversion using the formula:
/// f-number = sqrt(2) ^ (apex_value / 32)
///
/// # Parameters
/// - `apex_value`: The raw APEX aperture value from Canon MakerNote
///
/// # Returns
/// A formatted f-number string (e.g., "f/2.8", "f/5.6")
///
/// # Example
/// ```ignore
/// let aperture = apex_to_aperture(160); // Returns "f/5.6"
/// ```
pub fn apex_to_aperture(apex_value: i16) -> String {
    if apex_value == 0 {
        return "n/a".to_string();
    }

    // Canon formula: f-number = 2^(apex/64)
    // Some cameras use apex/32, we'll use the most common: apex/64
    let f_number = 2.0_f64.powf(apex_value as f64 / 64.0);

    // Format with appropriate precision
    if f_number < 10.0 {
        format!("f/{:.1}", f_number)
    } else {
        format!("f/{:.0}", f_number)
    }
}

/// Converts a Canon APEX shutter speed value to an exposure time string.
///
/// Canon stores shutter speed as raw value that needs conversion using the formula:
/// exposure_time = 2 ^ (-apex_value / 32)
///
/// # Parameters
/// - `apex_value`: The raw APEX shutter speed value from Canon MakerNote
///
/// # Returns
/// A formatted exposure time string (e.g., "1/250", "1/60", "2 sec")
///
/// # Example
/// ```ignore
/// let shutter = apex_to_exposure_time(256); // Returns "1/256" approximately
/// ```
pub fn apex_to_exposure_time(apex_value: i16) -> String {
    if apex_value == 0 {
        return "n/a".to_string();
    }

    // Canon formula: exposure = 2^(-apex/32)
    let exposure_time = 2.0_f64.powf(-(apex_value as f64) / 32.0);

    // Format based on the exposure time value
    if exposure_time >= 1.0 {
        // 1 second or longer
        if exposure_time == exposure_time.round() {
            format!("{} sec", exposure_time as i32)
        } else {
            format!("{:.1} sec", exposure_time)
        }
    } else if exposure_time >= 0.5 {
        // Between 0.5 and 1 second - show as fraction
        let denominator = (1.0 / exposure_time).round() as i32;
        format!("1/{}", denominator)
    } else {
        // Faster than 0.5 second - calculate as 1/x
        let denominator = (1.0 / exposure_time).round() as i32;
        format!("1/{}", denominator)
    }
}

/// Formats a focal length value with units.
///
/// Takes a raw focal length value and the focal units per mm,
/// and returns a formatted string like "50 mm" or "24.0 mm".
///
/// # Parameters
/// - `raw_value`: The raw focal length value from Canon MakerNote
/// - `focal_units`: The units per mm (typically 1, but can be other values)
///
/// # Returns
/// A formatted focal length string with "mm" suffix
pub fn format_focal_length(raw_value: i16, focal_units: i16) -> String {
    if focal_units == 0 || raw_value == 0 {
        return "n/a".to_string();
    }

    let focal_length_mm = raw_value as f64 / focal_units as f64;

    // Format with appropriate precision
    if focal_length_mm == focal_length_mm.round() {
        format!("{} mm", focal_length_mm as i32)
    } else {
        format!("{:.1} mm", focal_length_mm)
    }
}

/// Converts a Canon APEX-style value to an EV (exposure value) string.
///
/// Canon stores many exposure-related values in a scaled format where the
/// raw value needs to be divided by 32 to get the actual EV value.
///
/// # Parameters
/// - `value`: The raw APEX-encoded value from the ShotInfo array
///
/// # Returns
/// A formatted string with the EV value to 1 decimal place, with sign prefix
/// (e.g., "+1.5", "-0.7", "0.0")
fn apex_to_ev(value: i16) -> String {
    // Canon APEX values are scaled by 32 (5 bits of fraction)
    let ev = value as f64 / 32.0;
    if ev >= 0.0 {
        format!("+{:.1}", ev)
    } else {
        format!("{:.1}", ev)
    }
}

/// Converts a Canon hex-based EV code to a number.
///
/// ExifTool `Image::ExifTool::Canon::CanonEv` (Canon.pm:10648):
///
/// ```text
///     my $frac = $val & 0x1f;
///     $val -= $frac;                       # remove fraction
///     if ($frac == 0x0c) { $frac = 0x20 / 3; }        # 1/3 stop
///     elsif ($frac == 0x14) { $frac = 0x40 / 3; }     # 2/3 stop
///     return $sign * ($val + $frac) / 0x20;
/// ```
pub fn canon_ev(value: i32) -> f64 {
    let sign = if value < 0 { -1.0 } else { 1.0 };
    let magnitude = value.unsigned_abs();
    let raw_frac = magnitude & 0x1f;
    let whole = (magnitude - raw_frac) as f64;
    let frac = match raw_frac {
        0x0c => 0x20 as f64 / 3.0,
        0x14 => 0x40 as f64 / 3.0,
        other => other as f64,
    };
    sign * (whole + frac) / 32.0
}

/// Renders a number the way Perl's `sprintf("%.2g", $val)` does.
///
/// Used by every `%Canon` aperture key (`MaxAperture`, `TargetAperture`, `FNumber`, ...).
/// Values large enough for `%g` to switch to exponential notation are printed in full
/// instead, because an aperture is never displayed as `1.3e+02`.
fn format_g2(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return "0".to_string();
    }
    let exponent = value.abs().log10().floor() as i32;
    let decimals = (1 - exponent).max(0) as usize;
    let rendered = format!("{:.*}", decimals, value);
    if rendered.contains('.') {
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        rendered
    }
}

/// Renders an exposure time the way ExifTool does.
///
/// ExifTool `Image::ExifTool::Exif::PrintExposureTime` (Exif.pm):
///
/// ```text
///     if ($secs < 0.25001 and $secs > 0) {
///         return sprintf("1/%d",int(0.5 + 1/$secs));
///     }
///     $_ = sprintf("%.1f",$secs);
///     s/\.0$//;
/// ```
fn print_exposure_time(seconds: f64) -> String {
    if seconds > 0.0 && seconds < 0.250_01 {
        return format!("1/{}", (0.5 + 1.0 / seconds) as i64);
    }
    let rendered = format!("{:.1}", seconds);
    rendered.strip_suffix(".0").unwrap_or(&rendered).to_string()
}

/// Renders an EV offset the way ExifTool does.
///
/// ExifTool `Image::ExifTool::Exif::PrintFraction` (Exif.pm): exact integers print as
/// `%+d`, halves as `%+d/2`, thirds as `%+d/3`, anything else as `%+.3g`, and zero as a
/// bare `0`.
fn print_fraction(value: f64) -> String {
    let value = value * 1.00001; // ExifTool's round-off guard
    if value == 0.0 {
        return "0".to_string();
    }
    let truncated = value.trunc();
    if truncated != 0.0 && truncated / value > 0.999 {
        return format!("{:+}", truncated as i64);
    }
    let halves = (value * 2.0).trunc();
    if halves != 0.0 && halves / (value * 2.0) > 0.999 {
        return format!("{:+}/2", halves as i64);
    }
    let thirds = (value * 3.0).trunc();
    if thirds != 0.0 && thirds / (value * 3.0) > 0.999 {
        return format!("{:+}/3", thirds as i64);
    }
    let magnitude = format_significant_3(value.abs());
    if value < 0.0 {
        format!("-{}", magnitude)
    } else {
        format!("+{}", magnitude)
    }
}

/// Renders a positive number with 3 significant digits, `%g`-style (no trailing zeros).
fn format_significant_3(value: f64) -> String {
    if value == 0.0 || !value.is_finite() {
        return "0".to_string();
    }
    let exponent = value.abs().log10().floor() as i32;
    let decimals = (2 - exponent).max(0) as usize;
    let rendered = format!("{:.*}", decimals, value);
    if rendered.contains('.') {
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        rendered
    }
}

/// Renders a Canon "parameter" value (Contrast/Saturation/ColorTone style).
///
/// ExifTool `%Image::ExifTool::Exif::printParameter` (Exif.pm:317) maps 0 to `Normal` and
/// defers everything else to `PrintParameter` (Exif.pm:5533), which prefixes positive
/// values with `+` and re-signs anything above 0xfff0.
fn print_parameter(value: i16) -> String {
    let raw = value as u16 as i32;
    if raw == 0 {
        return "Normal".to_string();
    }
    if raw > 0xfff0 {
        return (raw - 0x10000).to_string();
    }
    format!("+{}", raw)
}

/// Renders `%Canon::FileInfo` key 1 / Canon tag 0x0008 as `directory-file`.
///
/// ExifTool Canon.pm:1264 — `PrintConv => '$_=$val,s/(\d+)(\d{4})/$1-$2/,$_'`: the last
/// four digits are the file number, everything before them the directory number.
fn format_canon_file_number(value: u32) -> String {
    let digits = value.to_string();
    if digits.len() > 4 {
        let split = digits.len() - 4;
        format!("{}-{}", &digits[..split], &digits[split..])
    } else {
        digits
    }
}

/// Renders a number the way Perl interpolates it into a string (no trailing zeros).
fn format_perl_number(value: f64) -> String {
    let rendered = format!("{:.6}", value);
    let trimmed = rendered.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Decodes `%Canon::CameraSettings` key 16, `CameraISO`.
///
/// ExifTool `Image::ExifTool::Canon::CameraISO` (Canon.pm:10464): 0x7fff means "not
/// present" (the key's `RawConv` drops it), bit 0x4000 marks a literal ISO in the low 14
/// bits, and everything else is a small lookup code.
fn camera_iso(value: i16) -> Option<String> {
    let raw = value as u16;
    if raw == 0x7fff {
        return None;
    }
    if raw & 0x4000 != 0 {
        return Some((raw & 0x3fff).to_string());
    }
    Some(match raw {
        0 => "n/a".to_string(),
        14 => "Auto High".to_string(),
        15 => "Auto".to_string(),
        16 => "50".to_string(),
        17 => "100".to_string(),
        18 => "200".to_string(),
        19 => "400".to_string(),
        20 => "800".to_string(),
        other => format!("Unknown ({})", other),
    })
}

/// True for the bodies whose `%Canon::FileInfo` key 1 is a 20D/350D-style `FileNumber`.
///
/// ExifTool Canon.pm:6850 — `Condition => '$$self{Model} =~ /\b(20D|350D|REBEL XT|Kiss
/// Digital N)\b/'`. The same set selects `%Canon::ShotInfo` key 22's first `ExposureTime`
/// variant (Canon.pm:2968) and excludes `%Canon::Processing` key 2's `Sharpness`
/// (Canon.pm:7217).
fn is_20d_or_350d(model: &str) -> bool {
    ["20D", "350D", "REBEL XT", "Kiss Digital N"]
        .iter()
        .any(|needle| has_word(model, needle))
}

/// True for the bodies that select `%CanonCustom::Functions350D`.
///
/// ExifTool Canon.pm:1542 — `Condition => '$$self{Model} =~ /\b(350D|REBEL XT|Kiss
/// Digital N)\b/'`. The 400D shares most of the layout but redefines keys 0 and 1, so it
/// must not fall through to this table.
fn is_350d_custom_functions(model: &str) -> bool {
    ["350D", "REBEL XT", "Kiss Digital N"]
        .iter()
        .any(|needle| has_word(model, needle))
}

/// True for the bodies whose `%Canon::FocalLength` keys 2 and 3 hold real focal plane
/// sizes.
///
/// ExifTool Canon.pm:2735:
///
/// ```text
///     $$self{Model} !~ /EOS/ or
///     $$self{Model} =~ /\b(1DS?|5D|D30|D60|10D|20D|30D|K236)$/ or
///     $$self{Model} =~ /\b((300D|350D|400D) DIGITAL|REBEL( XTi?)?|Kiss Digital( [NX])?)$/
/// ```
fn focal_plane_size_supported(model: &str) -> bool {
    if !model.contains("EOS") {
        return true;
    }
    const TRAILING: &[&str] = &["1D", "1DS", "5D", "D30", "D60", "10D", "20D", "30D", "K236"];
    if TRAILING
        .iter()
        .any(|suffix| model.ends_with(suffix) && has_word(model, suffix))
    {
        return true;
    }
    // Every alternative in this branch is anchored to the end of the model name by
    // ExifTool's trailing `$`, so a body such as "Canon EOS REBEL T3i" must NOT match
    // "REBEL" — its focal plane slots hold something else entirely.
    const REBEL_FAMILY: &[&str] = &[
        "300D DIGITAL",
        "350D DIGITAL",
        "400D DIGITAL",
        "REBEL",
        "REBEL XT",
        "REBEL XTi",
        "Kiss Digital",
        "Kiss Digital N",
        "Kiss Digital X",
    ];
    REBEL_FAMILY
        .iter()
        .any(|needle| model.ends_with(needle) && has_word(model, needle))
}

/// Perl `\b...\b` word-boundary containment for the model-name conditions above.
fn has_word(haystack: &str, needle: &str) -> bool {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut start = 0;
    while let Some(found) = haystack[start..].find(needle) {
        let begin = start + found;
        let end = begin + needle.len();
        let before_ok = begin == 0 || !is_word(haystack[..begin].chars().next_back().unwrap());
        let after_ok = end == haystack.len() || !is_word(haystack[end..].chars().next().unwrap());
        if before_ok && after_ok {
            return true;
        }
        start = begin + 1;
        if start >= haystack.len() {
            break;
        }
    }
    false
}

/// Formats a Canon focus distance value to a human-readable string.
///
/// Canon stores focus distance in centimeters. A value of 0xFFFF (65535 or
/// -1 as signed) indicates infinity focus. A value of 0 also indicates infinity.
///
/// # Parameters
/// - `value`: The raw focus distance value from the ShotInfo array (in centimeters)
///
/// # Returns
/// A formatted string with the distance in meters (e.g., "1.50 m", "7.82 m")
/// or "inf" for infinity focus
fn format_focus_distance(value: i16) -> String {
    // ExifTool `%Canon::ShotInfo` keys 19/20 declare `Format => 'int16u'`, so 0xFFFF is
    // 65535 cm, not -1 cm:
    //
    // ```text
    //     ValueConv => '$val / 100',
    //     PrintConv => '$val > 655.345 ? "inf" : "$val m"',
    // ```
    let distance_m = (value as u16) as f64 / 100.0;
    if distance_m > 655.345 {
        return "inf".to_string();
    }
    format!("{} m", format_perl_number(distance_m))
}

/// Decodes the AF points in focus bitfield to a human-readable string.
///
/// Canon stores which AF points were used for focus as a bitmask, where
/// each bit represents a specific AF point. This function converts that
/// bitmask to a comma-separated list of point numbers.
///
/// # Parameters
/// - `value`: The raw bitfield value from the ShotInfo array
///
/// # Returns
/// A comma-separated string of AF point numbers that were in focus
/// (e.g., "Center", "1,2,5", "Center, 1"), or "None" if no points selected
fn decode_af_points_in_focus(value: i16) -> String {
    if value == 0 {
        return "None".to_string();
    }

    let mut points = Vec::new();
    for bit in 0..16 {
        if (value & (1 << bit)) != 0 {
            // AF point numbering typically starts at 1 for display
            // Bit 0 is often the center point
            if bit == 0 {
                points.push("Center".to_string());
            } else {
                points.push(format!("{}", bit));
            }
        }
    }

    if points.is_empty() {
        "None".to_string()
    } else {
        points.join(", ")
    }
}

// ============================================================================
// CANON MODEL ID DECODER
// ============================================================================
// Maps Canon Model ID (tag 0x0010) to human-readable camera model names.
// The model ID is a 32-bit unsigned integer that uniquely identifies each
// Canon camera model. Values typically follow patterns:
// - 0x01XXXXXX: PowerShot series and early cameras
// - 0x80XXXXXX: EOS series digital SLRs and mirrorless cameras
//
// Reference: ExifTool Canon.pm CanonModelID table

/// Decodes a Canon Model ID to the corresponding camera model name.
///
/// Canon cameras store a numeric model identifier in the MakerNotes which
/// uniquely identifies the camera model. This function translates that
/// numeric ID into a human-readable camera name, matching ExifTool's output.
///
/// # Parameters
/// - `model_id`: The raw 32-bit Canon Model ID value
///
/// # Returns
/// A string containing the camera model name. For unknown IDs, returns
/// "Unknown ({id})" where {id} is the decimal value.
///
/// # Examples
/// ```
/// use oxidex::parsers::tiff::makernotes::canon::decode_canon_model_id;
///
/// // PowerShot S40 has model ID 0x1110000 (17891328 decimal)
/// assert_eq!(decode_canon_model_id(0x1110000), "PowerShot S40");
/// assert_eq!(decode_canon_model_id(17891328), "PowerShot S40");
///
/// // EOS 5D Mark III
/// assert_eq!(decode_canon_model_id(0x80000281), "EOS 5D Mark III");
/// ```
pub fn decode_canon_model_id(model_id: u32) -> String {
    match model_id {
        // ====================================================================
        // PowerShot Series and Early Digital Cameras
        // ====================================================================
        // These cameras use model IDs in the 0x01XXXXXX range
        0x1010000 => "PowerShot A30".to_string(),
        0x1040000 => "PowerShot S300 / Digital IXUS 300 / IXY Digital 300".to_string(),
        0x1060000 => "PowerShot A20".to_string(),
        0x1080000 => "PowerShot A10".to_string(),
        0x1090000 => "PowerShot S110 / Digital IXUS v / IXY Digital 200".to_string(),
        0x1100000 => "PowerShot G2".to_string(),
        0x1110000 => "PowerShot S40".to_string(), // 17891328 decimal
        0x1120000 => "PowerShot S30".to_string(),
        0x1130000 => "PowerShot A40".to_string(),
        0x1140000 => "EOS D30".to_string(),
        0x1150000 => "PowerShot A100".to_string(),
        0x1160000 => "PowerShot S200 / Digital IXUS v2 / IXY Digital 200a".to_string(),
        0x1170000 => "PowerShot A200".to_string(),
        0x1180000 => "PowerShot S330 / Digital IXUS 330 / IXY Digital 300a".to_string(),
        0x1190000 => "PowerShot G3".to_string(),
        0x1210000 => "PowerShot S45".to_string(),
        0x1230000 => "PowerShot SD100 / Digital IXUS II / IXY Digital 30".to_string(),

        // Later PowerShot compact cameras (0x03XXXXXX range)
        0x3160000 => "PowerShot A1300".to_string(), // 51773440 decimal

        // ====================================================================
        // EOS Series Digital SLR and Mirrorless Cameras
        // ====================================================================
        // Professional and consumer EOS cameras use model IDs in the 0x80XXXXXX range
        0x80000001 => "EOS-1D".to_string(),
        0x80000167 => "EOS-1DS".to_string(),
        0x80000168 => "EOS 10D".to_string(),
        0x80000169 => "EOS-1D Mark III".to_string(),
        0x80000170 => "EOS Digital Rebel / 300D / Kiss Digital".to_string(),
        0x80000174 => "EOS-1D Mark II".to_string(),
        0x80000175 => "EOS 20D".to_string(),
        0x80000176 => "EOS Digital Rebel XSi / 450D / Kiss X2".to_string(),
        0x80000188 => "EOS-1Ds Mark II".to_string(),
        0x80000189 => "EOS Digital Rebel XT / 350D / Kiss Digital N".to_string(),
        0x80000190 => "EOS 40D".to_string(),
        0x80000213 => "EOS 5D".to_string(),
        0x80000215 => "EOS-1Ds Mark III".to_string(),
        0x80000218 => "EOS 5D Mark II".to_string(),
        0x80000250 => "EOS 7D".to_string(),
        0x80000252 => "EOS 500D / Rebel T1i / Kiss X3".to_string(),
        0x80000254 => "EOS 1000D / Rebel XS / Kiss F".to_string(),
        0x80000261 => "EOS 50D".to_string(),
        0x80000269 => "EOS-1D X".to_string(),
        0x80000270 => "EOS 550D / Rebel T2i / Kiss X4".to_string(),
        0x80000271 => "EOS-1D Mark IV".to_string(),
        0x80000281 => "EOS 5D Mark III".to_string(),
        0x80000285 => "EOS 600D / Rebel T3i / Kiss X5".to_string(),
        0x80000286 => "EOS 60D".to_string(),
        0x80000287 => "EOS 1100D / Rebel T3 / Kiss X50".to_string(),
        0x80000288 => "EOS 650D / Rebel T4i / Kiss X6i".to_string(),
        0x80000289 => "EOS 6D".to_string(),
        0x80000301 => "EOS 700D / Rebel T5i / Kiss X7i".to_string(),
        0x80000302 => "EOS 100D / Rebel SL1 / Kiss X7".to_string(),
        0x80000324 => "EOS 70D".to_string(),
        0x80000325 => "EOS 760D / Rebel T6s / 8000D".to_string(),
        0x80000326 => "EOS 750D / Rebel T6i / Kiss X8i".to_string(),
        0x80000327 => "EOS M3".to_string(),
        0x80000328 => "EOS-1D C".to_string(),
        0x80000331 => "EOS 80D".to_string(),
        0x80000346 => "EOS 5D Mark IV".to_string(),
        0x80000347 => "EOS-1D X Mark II".to_string(),
        0x80000350 => "EOS 5DS".to_string(),
        0x80000351 => "EOS 5DS R".to_string(),
        0x80000393 => "EOS 6D Mark II".to_string(),
        0x80000401 => "EOS 77D / 9000D".to_string(),
        0x80000404 => "EOS R5".to_string(),
        0x80000405 => "EOS R6".to_string(),
        0x80000406 => "EOS-1D X Mark III".to_string(),

        // Unknown model ID - return formatted string with the raw value
        _ => format!("Unknown ({})", model_id),
    }
}

/// Decodes Canon model ID to camera type (Compact, EOS, etc.).
///
/// # Parameters
/// - `model_id`: The Canon model ID value from tag 0x0010
///
/// # Returns
/// Camera type as a string: "Compact", "EOS Mid-range", "EOS High-end", etc.
pub fn decode_camera_type(model_id: u32) -> String {
    // EOS cameras use model IDs starting with 0x80000000
    // PowerShot and other compact cameras use model IDs in 0x01000000 range
    if model_id >= 0x80000000 {
        // EOS DSLR/Mirrorless cameras
        match model_id {
            // Professional 1-series bodies
            0x80000001 | // EOS-1D
            0x80000167 | // EOS-1Ds
            0x80000168 | // EOS-1D Mark II
            0x80000169 | // EOS-1Ds Mark II
            0x80000170 | // EOS-1D Mark II N
            0x80000174 | // EOS-1D Mark III
            0x80000175 | // EOS-1Ds Mark III
            0x80000269 | // EOS-1D Mark IV
            0x80000281 | // EOS-1D X
            0x80000285 | // EOS-1D X Mark II (duplicate ID handling)
            0x80000302 | // EOS-1D C
            0x80000324 | // EOS-1D C (duplicate)
            0x80000328 | // EOS-1D C (duplicate)
            0x80000347 | // EOS-1D X Mark II
            0x80000406   // EOS-1D X Mark III
            => "EOS High-end".to_string(),

            // All other EOS models are mid-range or entry-level
            _ => "EOS Mid-range".to_string(),
        }
    } else if (0x01000000..0x02000000).contains(&model_id) {
        // PowerShot and compact cameras
        "Compact".to_string()
    } else {
        // Unknown camera type
        "Unknown".to_string()
    }
}

/// Represents a Canon MakerNote tag value
#[derive(Debug, Clone, PartialEq)]
pub enum CanonTagValue {
    /// Single integer value
    Integer(i32),
    /// String value (model name, firmware, etc.)
    String(String),
    /// Array of integers (camera settings, shot info)
    IntArray(Vec<i16>),
}

/// Maps Canon MakerNote tag IDs to human-readable tag names.
///
/// # Parameters
/// - `tag_id`: The Canon-specific tag ID
///
/// # Returns
/// Tag name in the format "Canon:TagName"
///
/// # Example
/// ```
/// use oxidex::parsers::tiff::makernotes::canon::canon_tag_to_name;
/// assert_eq!(canon_tag_to_name(0x0001), "Canon:CameraSettings");
/// ```
pub fn canon_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        CANON_CAMERA_SETTINGS => "CameraSettings",
        CANON_FOCAL_LENGTH => "FocalLength",
        CANON_FLASH_INFO => "FlashInfo",
        CANON_SHOT_INFO => "ShotInfo",
        CANON_PANORAMA => "Panorama",
        CANON_IMAGE_TYPE => "ImageType",
        CANON_FIRMWARE_VERSION => "FirmwareVersion",
        CANON_FILE_NUMBER => "FileNumber",
        CANON_OWNER_NAME => "OwnerName",
        CANON_SERIAL_NUMBER => "SerialNumber",
        CANON_CAMERA_INFO => "CameraInfo",
        CANON_CUSTOM_FUNCTIONS => "CustomFunctions",
        CANON_MODEL_ID => "CanonModelID",
        CANON_AF_INFO => "AFInfo",
        CANON_SERIAL_NUMBER_FORMAT => "SerialNumberFormat",
        CANON_AF_INFO2 => "AFInfo2",
        CANON_FILE_INFO => "FileInfo",
        CANON_LENS_MODEL => "LensModel",
        CANON_INTERNAL_SERIAL_NUMBER => "InternalSerialNumber",
        CANON_PROCESSING_INFO => "ProcessingInfo",
        CANON_MEASURED_COLOR => "MeasuredColor",
        CANON_COLOR_SPACE => "ColorSpace",
        CANON_VRD_OFFSET => "VRDOffset",
        _ => return format!("Canon:Unknown-{:#06X}", tag_id),
    };

    format!("Canon:{}", tag_name)
}

/// Represents a Canon MakerNote parser
pub struct CanonParser;

impl MakerNoteParser for CanonParser {
    fn manufacturer_name(&self) -> &'static str {
        "Canon"
    }

    fn tag_prefix(&self) -> &'static str {
        "Canon:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_canon_makernote(data)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // Call the existing parse_canon_makernote function and handle Result conversion
        match parse_canon_makernote_impl(data, byte_order) {
            Ok(parsed_tags) => {
                tags.extend(parsed_tags);
                Ok(())
            }
            Err(e) => Err(format!("Canon MakerNote parse error: {}", e)),
        }
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }
}

/// Checks if data appears to be a Canon MakerNote.
///
/// Canon MakerNotes may optionally start with "Canon" signature,
/// but always contain a valid IFD structure.
///
/// # Parameters
/// - `data`: Raw byte data to check
///
/// # Returns
/// `true` if the data appears to be a Canon MakerNote, `false` otherwise
pub fn is_canon_makernote(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    // Check for optional Canon signature
    if data.starts_with(CANON_SIGNATURE) {
        return true;
    }

    // Check if it looks like an IFD (starts with entry count)
    // Valid IFD has at least 2 bytes for entry count
    // Try both little-endian and big-endian interpretations
    if data.len() >= 2 {
        let le_reader = EndianReader::little_endian(data);
        let be_reader = EndianReader::big_endian(data);
        let entry_count_le = le_reader.u16_at(0).unwrap_or(0);
        let entry_count_be = be_reader.u16_at(0).unwrap_or(0);

        // Reasonable entry count (Canon typically has 1-100 entries)
        // Accept if either byte order yields a reasonable count
        let is_reasonable = |count: u16| count > 0 && count < 200;

        return is_reasonable(entry_count_le) || is_reasonable(entry_count_be);
    }

    false
}

/// Internal implementation of Canon MakerNote parsing.
///
/// This parser extracts tags from Canon MakerNotes including simple tags
/// (strings and integers) and complex array tags (CameraSettings, ShotInfo, etc.).
///
/// # Parameters
/// - `data`: Raw MakerNote data (may include Canon signature)
/// - `byte_order`: Byte order for parsing (usually matches TIFF header)
///
/// # Returns
/// HashMap of tag names to string values
///
/// # Errors
/// Returns error if IFD parsing fails or data is invalid
fn parse_canon_makernote_impl(
    data: &[u8],
    byte_order: ByteOrder,
) -> Result<HashMap<String, String>> {
    if data.is_empty() {
        return Ok(HashMap::new());
    }

    let mut tags = HashMap::new();

    let config = IfdParserConfig {
        signature: Some(CANON_SIGNATURE),
        signature_offset: CANON_SIGNATURE.len(),
        max_entries: 200,
    };

    // Several `%Canon` keys are model-conditional (`%Canon::FileInfo` key 1,
    // `%Canon::ShotInfo` key 22, `%Canon::Processing` key 2, `%Canon::FocalLength` keys
    // 2-3, and the `CustomFunctions*` table selection). ExifTool reads `$$self{Model}`
    // from IFD0, which the MakerNote dispatcher does not pass down; CanonImageType
    // (MakerNote tag 0x0006) carries the same body name, so resolve it up front rather
    // than relying on the order entries happen to arrive in.
    let mut model = String::new();
    let _ = parse_ifd_entries(data, byte_order, &config, |entry, ifd_data| {
        if entry.tag_id == CANON_IMAGE_TYPE
            && let Some(value) = extract_canon_string(entry, ifd_data, byte_order)
        {
            model = value;
        }
    });
    let model = model;

    // FocalUnits (`%Canon::CameraSettings` key 25) divides `%Canon::FocalLength` key 1.
    let mut focal_units: i16 = 1;

    // Use shared IFD parser
    // Note: we don't propagate errors here to maintain existing behavior of
    // returning whatever tags we found even if parsing isn't perfect
    let _ = parse_ifd_entries(data, byte_order, &config, |entry, ifd_data| {
        match entry.tag_id {
            // Simple string tags (Phase 1)
            // Canon MakerNotes use TIFF-relative offsets, so we use extract_canon_string
            // which properly calculates and applies the base offset
            CANON_IMAGE_TYPE | CANON_FIRMWARE_VERSION | CANON_OWNER_NAME => {
                if let Some(value) = extract_canon_string(entry, ifd_data, byte_order) {
                    let tag_name = canon_tag_to_name(entry.tag_id);
                    tags.insert(tag_name.clone(), value.clone());

                    // Add ExifTool-compatible aliases for compatibility
                    match entry.tag_id {
                        CANON_FIRMWARE_VERSION => {
                            tags.insert("Canon:CanonFirmwareVersion".to_string(), value);
                        }
                        CANON_IMAGE_TYPE => {
                            tags.insert("Canon:CanonImageType".to_string(), value);
                        }
                        _ => {}
                    }
                }
            }

            // SerialNumber (tag 0x000C) is an int32u, not a string.
            //
            // ExifTool Canon.pm:1299 (the fall-through variant used by every body except
            // the D30 and the EOS-1D family):
            //
            // ```text
            //     Name => 'SerialNumber',
            //     Writable => 'int32u',
            //     PrintConv => 'sprintf("%.10u",$val)',
            // ```
            CANON_SERIAL_NUMBER => {
                let serial = entry.value_offset;
                let rendered = if has_word(&model, "EOS D30") {
                    format!("{:04x}{:05}", serial >> 16, serial & 0xffff)
                } else if model.contains("EOS-1D") {
                    format!("{:06}", serial)
                } else {
                    format!("{:010}", serial)
                };
                tags.insert("Canon:SerialNumber".to_string(), rendered);
            }

            // SerialNumberFormat (tag 0x0015) - int32u display-format selector
            CANON_SERIAL_NUMBER_FORMAT => {
                tags.insert(
                    "Canon:SerialNumberFormat".to_string(),
                    SERIAL_NUMBER_FORMAT.decode(entry.value_offset as i64),
                );
            }

            // ThumbnailImageValidArea (tag 0x0013) - int16u[4] crop box
            CANON_THUMBNAIL_IMAGE_VALID_AREA => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order)
                    && array.len() >= 4
                {
                    tags.insert(
                        "Canon:ThumbnailImageValidArea".to_string(),
                        join_i16_slice(&array[..4]),
                    );
                }
            }

            // OriginalDecisionDataOffset (tag 0x0083) and VRDOffset (tag 0x00D0) are
            // plain int32u offsets that ExifTool reports verbatim.
            CANON_ORIGINAL_DECISION_DATA_OFFSET => {
                tags.insert(
                    "Canon:OriginalDecisionDataOffset".to_string(),
                    entry.value_offset.to_string(),
                );
            }
            CANON_VRD_OFFSET => {
                tags.insert(
                    "Canon:VRDOffset".to_string(),
                    entry.value_offset.to_string(),
                );
            }

            // Canon Model ID - decode to camera model name
            // The model ID is stored as a 32-bit integer that maps to specific camera models
            CANON_MODEL_ID => {
                // The value_offset contains the model ID directly for LONG type (4 bytes)
                let model_id = entry.value_offset;
                let model_name = decode_canon_model_id(model_id);
                tags.insert("Canon:CanonModelID".to_string(), model_name);

                // Also output CameraType based on model ID
                let camera_type = decode_camera_type(model_id);
                tags.insert("Canon:CameraType".to_string(), camera_type);
            }

            // FileNumber (tag 0x0008) - int32u, ExifTool Canon.pm:1260 renders it as
            // `directory-file` via `s/(\d+)(\d{4})/$1-$2/`.
            CANON_FILE_NUMBER => {
                tags.insert(
                    "Canon:FileNumber".to_string(),
                    format_canon_file_number(entry.value_offset),
                );
            }

            // CameraSettings array (Phase 2)
            // Reference: ExifTool Canon.pm CameraSettings table
            CANON_CAMERA_SETTINGS => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order)
                    .map(realign_length_prefixed_record)
                {
                    // Extract specific settings from array using const decoders
                    // Note: All tag names use "Canon:" prefix for consistency

                    // MacroMode (index 1) - Macro shooting mode
                    if array.len() > CAMERA_SETTINGS_MACRO_MODE {
                        tags.insert(
                            "Canon:MacroMode".to_string(),
                            MACRO_MODE.decode(array[CAMERA_SETTINGS_MACRO_MODE]),
                        );
                    }

                    // SelfTimer (index 2) - Self-timer delay in 1/10 seconds
                    if array.len() > CAMERA_SETTINGS_SELF_TIMER {
                        let self_timer = array[CAMERA_SETTINGS_SELF_TIMER];
                        if self_timer > 0 {
                            // Convert from 1/10 seconds to more readable format
                            let seconds = self_timer as f64 / 10.0;
                            tags.insert(
                                "Canon:SelfTimer".to_string(),
                                format!("{:.1} sec", seconds),
                            );
                        } else {
                            tags.insert("Canon:SelfTimer".to_string(), "Off".to_string());
                        }
                    }

                    // Quality (index 3) - Image quality setting
                    if array.len() > CAMERA_SETTINGS_QUALITY {
                        tags.insert(
                            "Canon:Quality".to_string(),
                            QUALITY.decode(array[CAMERA_SETTINGS_QUALITY]),
                        );
                    }

                    // CanonFlashMode (index 4) - Flash mode setting
                    // Also output as Canon:FlashMode for backward compatibility
                    if array.len() > CAMERA_SETTINGS_FLASH_MODE {
                        let flash_mode = FLASH_MODE.decode(array[CAMERA_SETTINGS_FLASH_MODE]);
                        tags.insert("Canon:CanonFlashMode".to_string(), flash_mode.clone());
                        tags.insert("Canon:FlashMode".to_string(), flash_mode);
                    }

                    // ContinuousDrive (index 5) - Drive mode setting
                    // Also output as Canon:DriveMode for backward compatibility
                    if array.len() > CAMERA_SETTINGS_DRIVE_MODE {
                        let drive_mode = DRIVE_MODE.decode(array[CAMERA_SETTINGS_DRIVE_MODE]);
                        tags.insert("Canon:ContinuousDrive".to_string(), drive_mode.clone());
                        tags.insert("Canon:DriveMode".to_string(), drive_mode);
                    }

                    // FocusMode (index 7) - Focus mode setting
                    if array.len() > CAMERA_SETTINGS_FOCUS_MODE {
                        tags.insert(
                            "Canon:FocusMode".to_string(),
                            FOCUS_MODE.decode(array[CAMERA_SETTINGS_FOCUS_MODE]),
                        );
                    }

                    // RecordMode (index 9) - Recording format
                    if array.len() > CAMERA_SETTINGS_RECORD_MODE {
                        tags.insert(
                            "Canon:RecordMode".to_string(),
                            RECORD_MODE.decode(array[CAMERA_SETTINGS_RECORD_MODE]),
                        );
                    }

                    // CanonImageSize (index 10) - Image size setting
                    if array.len() > CAMERA_SETTINGS_IMAGE_SIZE {
                        tags.insert(
                            "Canon:CanonImageSize".to_string(),
                            CANON_IMAGE_SIZE.decode(array[CAMERA_SETTINGS_IMAGE_SIZE]),
                        );
                    }

                    // EasyMode (index 11) - Scene mode / Easy mode setting
                    if array.len() > CAMERA_SETTINGS_EASY_MODE {
                        tags.insert(
                            "Canon:EasyMode".to_string(),
                            EASY_MODE.decode(array[CAMERA_SETTINGS_EASY_MODE]),
                        );
                    }

                    // DigitalZoom (index 12) - Digital zoom setting
                    if array.len() > CAMERA_SETTINGS_DIGITAL_ZOOM {
                        tags.insert(
                            "Canon:DigitalZoom".to_string(),
                            DIGITAL_ZOOM.decode(array[CAMERA_SETTINGS_DIGITAL_ZOOM]),
                        );
                    }

                    // Contrast (index 13) - Contrast adjustment value
                    // Uses decoder to convert signed value to human-readable string
                    if array.len() > CAMERA_SETTINGS_CONTRAST {
                        tags.insert(
                            "Canon:Contrast".to_string(),
                            CONTRAST.decode(array[CAMERA_SETTINGS_CONTRAST]),
                        );
                    }

                    // Saturation (index 14) - Saturation adjustment value
                    // Uses decoder to convert signed value to human-readable string
                    if array.len() > CAMERA_SETTINGS_SATURATION {
                        tags.insert(
                            "Canon:Saturation".to_string(),
                            SATURATION.decode(array[CAMERA_SETTINGS_SATURATION]),
                        );
                    }

                    // Sharpness (index 15) - Sharpness adjustment value
                    // Output raw numeric value to match ExifTool
                    if array.len() > CAMERA_SETTINGS_SHARPNESS {
                        tags.insert(
                            "Canon:Sharpness".to_string(),
                            array[CAMERA_SETTINGS_SHARPNESS].to_string(),
                        );
                    }

                    // CameraISO (index 16). ExifTool's RawConv drops the 0x7fff
                    // "not present" sentinel instead of reporting it as an ISO.
                    if let Some(&raw_iso) = array.get(CAMERA_SETTINGS_ISO)
                        && let Some(iso) = camera_iso(raw_iso)
                    {
                        tags.insert("Canon:CameraISO".to_string(), iso);
                    }

                    // MeteringMode (index 17) - Metering mode setting
                    if array.len() > CAMERA_SETTINGS_METERING_MODE {
                        tags.insert(
                            "Canon:MeteringMode".to_string(),
                            METERING_MODE.decode(array[CAMERA_SETTINGS_METERING_MODE]),
                        );
                    }

                    // FocusRange (index 18) - Focus range/type setting
                    if array.len() > CAMERA_SETTINGS_FOCUS_RANGE {
                        tags.insert(
                            "Canon:FocusRange".to_string(),
                            FOCUS_RANGE.decode(array[CAMERA_SETTINGS_FOCUS_RANGE]),
                        );
                    }

                    // AFPoint (index 19). ExifTool's `RawConv => '$val==0 ? undef : $val'`
                    // suppresses the tag entirely on bodies that leave the slot at zero.
                    if let Some(&af_point) = array.get(CAMERA_SETTINGS_AF_POINT)
                        && af_point != 0
                    {
                        tags.insert("Canon:AFPoint".to_string(), AF_POINT.decode(af_point));
                    }

                    // CanonExposureMode (index 20) - Exposure mode setting
                    // Also output as Canon:ExposureMode for backward compatibility
                    if array.len() > CAMERA_SETTINGS_EXPOSURE_MODE {
                        let exposure_mode =
                            EXPOSURE_MODE.decode(array[CAMERA_SETTINGS_EXPOSURE_MODE]);
                        tags.insert("Canon:CanonExposureMode".to_string(), exposure_mode.clone());
                        tags.insert("Canon:ExposureMode".to_string(), exposure_mode);
                    }

                    // LensType (index 22) - Lens type ID
                    if array.len() > CAMERA_SETTINGS_LENS_TYPE {
                        let lens_id = array[CAMERA_SETTINGS_LENS_TYPE];
                        if lens_id > 0 {
                            // Try to look up lens name from database
                            if let Some(lens_name) = lookup_lens_name(lens_id as u16) {
                                tags.insert("Canon:LensType".to_string(), lens_name);
                            } else {
                                tags.insert(
                                    "Canon:LensType".to_string(),
                                    format!("Unknown ({})", lens_id),
                                );
                            }
                        } else {
                            // For compact cameras or fixed lenses, output "n/a"
                            tags.insert("Canon:LensType".to_string(), "n/a".to_string());
                        }
                    }

                    // Get focal units for focal length calculations (index 25)
                    focal_units = if array.len() > CAMERA_SETTINGS_FOCAL_UNITS {
                        let units = array[CAMERA_SETTINGS_FOCAL_UNITS];
                        if units > 0 { units } else { 1 }
                    } else {
                        1
                    };

                    // FocalUnits (index 25) - Units per mm for focal length
                    if array.len() > CAMERA_SETTINGS_FOCAL_UNITS {
                        tags.insert(
                            "Canon:FocalUnits".to_string(),
                            format!("{}/mm", focal_units),
                        );
                    }

                    // MaxFocalLength (index 23) - Maximum focal length
                    if array.len() > CAMERA_SETTINGS_MAX_FOCAL_LENGTH {
                        tags.insert(
                            "Canon:MaxFocalLength".to_string(),
                            format_focal_length(
                                array[CAMERA_SETTINGS_MAX_FOCAL_LENGTH],
                                focal_units,
                            ),
                        );
                    }

                    // MinFocalLength (index 24) - Minimum focal length
                    if array.len() > CAMERA_SETTINGS_MIN_FOCAL_LENGTH {
                        tags.insert(
                            "Canon:MinFocalLength".to_string(),
                            format_focal_length(
                                array[CAMERA_SETTINGS_MIN_FOCAL_LENGTH],
                                focal_units,
                            ),
                        );
                    }

                    // MaxAperture (index 26) - Maximum aperture (APEX value)
                    if array.len() > CAMERA_SETTINGS_MAX_APERTURE {
                        tags.insert(
                            "Canon:MaxAperture".to_string(),
                            apex_to_aperture(array[CAMERA_SETTINGS_MAX_APERTURE]),
                        );
                    }

                    // MinAperture (index 27) - Minimum aperture (APEX value)
                    if array.len() > CAMERA_SETTINGS_MIN_APERTURE {
                        tags.insert(
                            "Canon:MinAperture".to_string(),
                            apex_to_aperture(array[CAMERA_SETTINGS_MIN_APERTURE]),
                        );
                    }

                    // FlashModel (index 28). ExifTool masks with 0x7f and discards the
                    // "no information" code 127; there is no FlashActivity key here.
                    if let Some(&raw_flash_model) = array.get(CAMERA_SETTINGS_FLASH_MODEL) {
                        let masked = raw_flash_model & 0x7f;
                        if masked != 127 {
                            tags.insert("Canon:FlashModel".to_string(), FLASH_MODEL.decode(masked));
                        }
                    }

                    // FlashBits (index 29) - Flash features bitfield
                    if array.len() > CAMERA_SETTINGS_FLASH_BITS {
                        let flash_bits = array[CAMERA_SETTINGS_FLASH_BITS] as u32;
                        tags.insert("Canon:FlashBits".to_string(), FLASH_BITS.decode(flash_bits));
                    }

                    // FocusContinuous (index 32) - Continuous focus setting
                    if array.len() > CAMERA_SETTINGS_FOCUS_CONTINUOUS {
                        tags.insert(
                            "Canon:FocusContinuous".to_string(),
                            FOCUS_CONTINUOUS.decode(array[CAMERA_SETTINGS_FOCUS_CONTINUOUS]),
                        );
                    }

                    // AESetting (index 33). `RawConv => '$val==-1 ? undef : $val'` — an
                    // absent setting must not surface as "Unknown (-1)".
                    if let Some(&ae_setting) = array.get(CAMERA_SETTINGS_AE_SETTING)
                        && ae_setting != -1
                    {
                        tags.insert("Canon:AESetting".to_string(), AE_SETTING.decode(ae_setting));
                    }

                    // DisplayAperture (index 35). ExifTool `%Canon::CameraSettings` key 35
                    // (Canon.pm:2645) is `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => '$val / 10'` and *no* PrintConv, so the value prints as a
                    // bare number ("3.9"), never with an "f/" prefix.
                    if let Some(&display_aperture) = array.get(CAMERA_SETTINGS_DISPLAY_APERTURE)
                        && display_aperture != 0
                    {
                        tags.insert(
                            "Canon:DisplayAperture".to_string(),
                            format_perl_number(display_aperture as f64 / 10.0),
                        );
                    }

                    // ZoomSourceWidth (index 36) / ZoomTargetWidth (index 37). ExifTool
                    // has no RawConv on these keys, so a zero width is still reported.
                    if let Some(&width) = array.get(CAMERA_SETTINGS_ZOOM_SOURCE_WIDTH) {
                        tags.insert("Canon:ZoomSourceWidth".to_string(), width.to_string());
                    }
                    if let Some(&width) = array.get(CAMERA_SETTINGS_ZOOM_TARGET_WIDTH) {
                        tags.insert("Canon:ZoomTargetWidth".to_string(), width.to_string());
                    }

                    // SpotMeteringMode (index 39). `RawConv => '$val==-1 ? undef : $val'`.
                    if let Some(&spot) = array.get(CAMERA_SETTINGS_SPOT_METERING_MODE)
                        && spot != -1
                    {
                        tags.insert(
                            "Canon:SpotMeteringMode".to_string(),
                            SPOT_METERING_MODE.decode(spot),
                        );
                    }

                    // PhotoEffect (index 40). `RawConv => '$val==-1 ? undef : $val'`.
                    if let Some(&photo_effect) = array.get(CAMERA_SETTINGS_PHOTO_EFFECT)
                        && photo_effect != -1
                    {
                        tags.insert(
                            "Canon:PhotoEffect".to_string(),
                            PHOTO_EFFECT.decode(photo_effect),
                        );
                    }

                    // ManualFlashOutput (index 41) - PrintHex lookup, 0x7fff means n/a
                    if let Some(&manual_flash) = array.get(CAMERA_SETTINGS_MANUAL_FLASH_OUTPUT) {
                        tags.insert(
                            "Canon:ManualFlashOutput".to_string(),
                            MANUAL_FLASH_OUTPUT.decode(manual_flash as u16 as i32),
                        );
                    }

                    // ColorTone (index 42). `RawConv => '$val == 0x7fff ? undef : $val'`,
                    // then `%Image::ExifTool::Exif::printParameter`.
                    if let Some(&color_tone) = array.get(CAMERA_SETTINGS_COLOR_TONE)
                        && color_tone as u16 != 0x7fff
                    {
                        tags.insert("Canon:ColorTone".to_string(), print_parameter(color_tone));
                    }
                }
            }

            // ShotInfo array (Phase 2) - Extended extraction
            // Extracts all available fields from the Canon ShotInfo array
            CANON_SHOT_INFO => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order)
                    .map(realign_length_prefixed_record)
                {
                    // AutoISO (index 1). ExifTool `%Canon::ShotInfo` key 1 (Canon.pm:2778):
                    // `ValueConv => 'exp($val/32*log(2))*100'`, `PrintConv => '%.0f'`.
                    // The slot is a log-scale code, never a literal ISO speed.
                    if let Some(&auto_iso) = array.get(SHOT_INFO_AUTO_ISO) {
                        let value = (auto_iso as f64 / 32.0 * std::f64::consts::LN_2).exp() * 100.0;
                        tags.insert("Canon:AutoISO".to_string(), format!("{:.0}", value));
                    }

                    // BaseISO (index 2). `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => 'exp($val/32*log(2))*100/32'`, `PrintConv => '%.0f'`.
                    if let Some(&base_iso) = array.get(SHOT_INFO_BASE_ISO)
                        && base_iso != 0
                    {
                        let value =
                            (base_iso as f64 / 32.0 * std::f64::consts::LN_2).exp() * 100.0 / 32.0;
                        tags.insert("Canon:BaseISO".to_string(), format!("{:.0}", value));
                    }

                    // MeasuredEV (index 3). ExifTool `%Canon::ShotInfo` key 3
                    // (Canon.pm:2794): `ValueConv => '$val / 32 + 5'`, `PrintConv =>
                    // '%.2f'`. The +5 offset is not optional — without it every EOS body
                    // reports a light value 5 stops too dark.
                    if let Some(&measured_ev) = array.get(SHOT_INFO_MEASURED_EV) {
                        let ev = measured_ev as f64 / 32.0 + 5.0;
                        tags.insert("Canon:MeasuredEV".to_string(), format!("{:.2}", ev));
                    }

                    // TargetAperture (index 4) - convert APEX to f-number
                    if array.len() > SHOT_INFO_TARGET_APERTURE {
                        tags.insert(
                            "Canon:TargetAperture".to_string(),
                            apex_to_aperture(array[SHOT_INFO_TARGET_APERTURE]),
                        );
                    }

                    // TargetExposureTime (index 5) - convert APEX to fractional time
                    if array.len() > SHOT_INFO_TARGET_EXPOSURE_TIME {
                        tags.insert(
                            "Canon:TargetExposureTime".to_string(),
                            apex_to_exposure_time(array[SHOT_INFO_TARGET_EXPOSURE_TIME]),
                        );
                    }

                    // ExposureCompensation (index 6) - CanonEv + PrintFraction
                    if let Some(&comp) = array.get(SHOT_INFO_EXPOSURE_COMPENSATION) {
                        tags.insert(
                            "Canon:ExposureCompensation".to_string(),
                            print_fraction(canon_ev(comp as i32)),
                        );
                    }

                    // WhiteBalance (index 7) - use decoder
                    if array.len() > SHOT_INFO_WHITE_BALANCE {
                        tags.insert(
                            "Canon:WhiteBalance".to_string(),
                            WHITE_BALANCE.decode(array[SHOT_INFO_WHITE_BALANCE]),
                        );
                    }

                    // SlowShutter (index 8) - use decoder
                    if array.len() > SHOT_INFO_SLOW_SHUTTER {
                        tags.insert(
                            "Canon:SlowShutter".to_string(),
                            SLOW_SHUTTER.decode(array[SHOT_INFO_SLOW_SHUTTER]),
                        );
                    }

                    // SequenceNumber (index 9) - direct value
                    if array.len() > SHOT_INFO_SEQUENCE_NUMBER {
                        tags.insert(
                            "Canon:SequenceNumber".to_string(),
                            array[SHOT_INFO_SEQUENCE_NUMBER].to_string(),
                        );
                    }

                    // OpticalZoomCode (index 10). ExifTool `PrintConv => '$val == 8 ?
                    // "n/a" : $val'` — every EOS body writes 8 here, which is a sentinel
                    // rather than a zoom step.
                    if let Some(&zoom_code) = array.get(SHOT_INFO_OPTICAL_ZOOM_CODE) {
                        tags.insert(
                            "Canon:OpticalZoomCode".to_string(),
                            if zoom_code == 8 {
                                "n/a".to_string()
                            } else {
                                zoom_code.to_string()
                            },
                        );
                    }

                    // FlashGuideNumber (index 13). `RawConv => '$val==-1 ? undef : $val'`,
                    // `ValueConv => '$val / 32'`.
                    if let Some(&guide_number) = array.get(SHOT_INFO_FLASH_GUIDE_NUMBER)
                        && guide_number != -1
                    {
                        tags.insert(
                            "Canon:FlashGuideNumber".to_string(),
                            format_perl_number(guide_number as f64 / 32.0),
                        );
                    }

                    // AFPointsInFocus (index 14). `RawConv => '$val==0 ? undef : $val'`
                    // plus a PrintHex lookup — this slot is a code, not a bitmask, and is
                    // only meaningful on the D30/D60 and some PowerShot bodies.
                    if let Some(&af_points) = array.get(SHOT_INFO_AF_POINTS_IN_FOCUS)
                        && af_points != 0
                    {
                        tags.insert(
                            "Canon:AFPointsInFocus".to_string(),
                            SHOT_INFO_AF_POINTS_IN_FOCUS_CODES.decode(af_points as u16 as i32),
                        );
                    }

                    // FlashExposureComp (index 15) - CanonEv + PrintFraction
                    if let Some(&flash_comp) = array.get(SHOT_INFO_FLASH_EXPOSURE_COMP) {
                        tags.insert(
                            "Canon:FlashExposureComp".to_string(),
                            print_fraction(canon_ev(flash_comp as i32)),
                        );
                    }

                    // AutoExposureBracketing (index 16) - enumeration, not an EV offset
                    if let Some(&aeb) = array.get(SHOT_INFO_AUTO_EXPOSURE_BRACKETING) {
                        tags.insert(
                            "Canon:AutoExposureBracketing".to_string(),
                            AUTO_EXPOSURE_BRACKETING.decode(aeb),
                        );
                    }

                    // AEBBracketValue (index 17) - CanonEv + PrintFraction
                    if let Some(&aeb_value) = array.get(SHOT_INFO_AEB_BRACKET_VALUE) {
                        tags.insert(
                            "Canon:AEBBracketValue".to_string(),
                            print_fraction(canon_ev(aeb_value as i32)),
                        );
                    }

                    // ControlMode (index 18) - use decoder
                    if array.len() > SHOT_INFO_CONTROL_MODE {
                        tags.insert(
                            "Canon:ControlMode".to_string(),
                            CONTROL_MODE.decode(array[SHOT_INFO_CONTROL_MODE]),
                        );
                    }

                    // FocusDistanceUpper (index 19) / FocusDistanceLower (index 20).
                    // ExifTool: "FocusDistance tags are only extracted if
                    // FocusDistanceUpper is non-zero" — key 19's RawConv returns undef on
                    // zero and key 20 is conditional on it.
                    let focus_distance_upper = array
                        .get(SHOT_INFO_FOCUS_DISTANCE_UPPER)
                        .copied()
                        .unwrap_or(0);
                    if focus_distance_upper != 0 {
                        tags.insert(
                            "Canon:FocusDistanceUpper".to_string(),
                            format_focus_distance(focus_distance_upper),
                        );
                        if let Some(&lower) = array.get(SHOT_INFO_FOCUS_DISTANCE_LOWER) {
                            tags.insert(
                                "Canon:FocusDistanceLower".to_string(),
                                format_focus_distance(lower),
                            );
                        }
                    }

                    // FNumber (index 21). `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => 'exp(CanonEv($val)*log(2)/2)'`, `PrintConv => '%.2g'`.
                    if let Some(&f_number) = array.get(SHOT_INFO_FNUMBER)
                        && f_number != 0
                    {
                        let value =
                            (canon_ev(f_number as i32) * std::f64::consts::LN_2 / 2.0).exp();
                        tags.insert("Canon:FNumber".to_string(), format_g2(value));
                    }

                    // ExposureTime (index 22). ExifTool has two variants of this key: the
                    // 20D/350D encoding carries an extra *1000/32 factor (Canon.pm:2965).
                    if let Some(&exposure_time) = array.get(SHOT_INFO_EXPOSURE_TIME)
                        && exposure_time != 0
                    {
                        let base = (-canon_ev(exposure_time as i32) * std::f64::consts::LN_2).exp();
                        let seconds = if is_20d_or_350d(&model) {
                            base * 1000.0 / 32.0
                        } else {
                            base
                        };
                        tags.insert(
                            "Canon:ExposureTime".to_string(),
                            print_exposure_time(seconds),
                        );
                    }

                    // MeasuredEV2 (index 23). `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => '$val / 8 - 6'` (no PrintConv).
                    if let Some(&measured_ev2) = array.get(SHOT_INFO_MEASURED_EV2)
                        && measured_ev2 != 0
                    {
                        tags.insert(
                            "Canon:MeasuredEV2".to_string(),
                            format_perl_number(measured_ev2 as f64 / 8.0 - 6.0),
                        );
                    }

                    // BulbDuration (index 24). `ValueConv => '$val / 10'`.
                    if let Some(&duration) = array.get(SHOT_INFO_BULB_DURATION) {
                        tags.insert(
                            "Canon:BulbDuration".to_string(),
                            format_perl_number(duration as f64 / 10.0),
                        );
                    }

                    // AutoRotate (index 27). ExifTool's RawConv drops negative values.
                    if let Some(&auto_rotate) = array.get(SHOT_INFO_AUTO_ROTATE)
                        && auto_rotate >= 0
                    {
                        tags.insert(
                            "Canon:AutoRotate".to_string(),
                            AUTO_ROTATE.decode(auto_rotate),
                        );
                    }

                    // NDFilter (index 28)
                    if let Some(&nd_filter) = array.get(SHOT_INFO_ND_FILTER) {
                        tags.insert("Canon:NDFilter".to_string(), ND_FILTER.decode(nd_filter));
                    }

                    // SelfTimer2 (index 29). `RawConv => '$val >= 0 ? $val : undef'`,
                    // `ValueConv => '$val / 10'`.
                    if let Some(&self_timer2) = array.get(SHOT_INFO_SELF_TIMER2)
                        && self_timer2 >= 0
                    {
                        tags.insert(
                            "Canon:SelfTimer2".to_string(),
                            format_perl_number(self_timer2 as f64 / 10.0),
                        );
                    }
                }
            }

            // FocalLength array (Phase 2)
            // Contains focal type, focal length and (on supported bodies) focal plane size
            CANON_FOCAL_LENGTH => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order) {
                    // FocalType (key 0). `RawConv => '$val ? $val : undef'`.
                    if let Some(&focal_type) = array.first()
                        && focal_type != 0
                    {
                        tags.insert("Canon:FocalType".to_string(), FOCAL_TYPE.decode(focal_type));
                    }
                    // FocalLength (key 1). `RawConv => '$val ? $val : undef'`,
                    // `ValueConv => '$val / $$self{FocalUnits}'`, `PrintConv => '"$val mm"'`.
                    if let Some(&focal_length) = array.get(1)
                        && focal_length != 0
                    {
                        tags.insert(
                            "Canon:FocalLength".to_string(),
                            format_focal_length(focal_length, focal_units),
                        );
                    }
                    // FocalPlaneXSize / FocalPlaneYSize (keys 2 and 3), in 1/1000 inch.
                    // ExifTool only trusts these on the bodies listed in its Condition,
                    // and drops implausibly small values via `$val < 40 ? undef : $val`.
                    if focal_plane_size_supported(&model) {
                        for (index, name) in [
                            (2usize, "Canon:FocalPlaneXSize"),
                            (3usize, "Canon:FocalPlaneYSize"),
                        ] {
                            if let Some(&raw) = array.get(index) {
                                let thousandths = raw as u16 as f64;
                                if thousandths >= 40.0 {
                                    tags.insert(
                                        name.to_string(),
                                        format!("{:.2} mm", thousandths * 25.4 / 1000.0),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // LensModel tag (Phase 3) - ASCII string containing lens name
            CANON_LENS_MODEL => {
                // LensModel is an ASCII string tag
                if entry.field_type == 2 {
                    // ASCII type
                    let value_bytes = if entry.value_count <= 4 {
                        // Inline value
                        extract_inline_value(
                            entry.value_offset,
                            entry.value_count as usize,
                            byte_order,
                        )
                    } else {
                        // External value
                        if (entry.value_offset as usize) < data.len() {
                            let end = std::cmp::min(
                                (entry.value_offset as usize) + (entry.value_count as usize),
                                data.len(),
                            );
                            data[entry.value_offset as usize..end].to_vec()
                        } else {
                            Vec::new()
                        }
                    };

                    if !value_bytes.is_empty() {
                        let lens_model = String::from_utf8_lossy(&value_bytes)
                            .trim_end_matches('\0')
                            .to_string();
                        if !lens_model.is_empty() {
                            tags.insert("Canon:LensModel".to_string(), lens_model);
                        }
                    }
                }
            }

            // FileInfo array (Phase 3)
            CANON_FILE_INFO => {
                // FileInfo is a SHORT array
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order)
                    .map(realign_length_prefixed_record)
                {
                    // FileNumber (Perl key 1) is an int32u spanning int16 slots 1-2 on the
                    // 20D/350D family, with the bit layout documented at Canon.pm:6862:
                    //
                    // ```text
                    //   31....24 23....16 15.....8 7......0
                    //   00000000 ffffffff DDDDDDDD ddFFFFFF
                    // ```
                    let file_number_is_known = is_20d_or_350d(&model);
                    if file_number_is_known
                        && let (Some(&low), Some(&high)) = (array.get(1), array.get(2))
                    {
                        let raw = ((high as u16 as u32) << 16) | (low as u16 as u32);
                        let value = ((raw & 0xffc0) >> 6) * 10000
                            + ((raw >> 16) & 0xff)
                            + ((raw & 0x3f) << 8);
                        tags.insert(
                            "Canon:FileNumber".to_string(),
                            format_canon_file_number(value),
                        );
                    }

                    // BracketMode (Perl key 3)
                    if let Some(&bracket_mode) = array.get(FILE_INFO_BRACKET_MODE) {
                        tags.insert(
                            "Canon:BracketMode".to_string(),
                            BRACKET_MODE.decode(bracket_mode),
                        );
                    }

                    // BracketValue (Perl key 4)
                    if let Some(&bracket_value) = array.get(FILE_INFO_BRACKET_VALUE) {
                        tags.insert("Canon:BracketValue".to_string(), bracket_value.to_string());
                    }

                    // BracketShotNumber (Perl key 5)
                    if let Some(&bracket_shot) = array.get(FILE_INFO_BRACKET_SHOT_NUMBER) {
                        tags.insert(
                            "Canon:BracketShotNumber".to_string(),
                            bracket_shot.to_string(),
                        );
                    }

                    // LongExposureNoiseReduction2 (Perl key 8). `RawConv => '$val<0 ? undef'`.
                    if let Some(&long_exposure_nr) = array.get(FILE_INFO_LONG_EXPOSURE_NR2)
                        && long_exposure_nr >= 0
                    {
                        tags.insert(
                            "Canon:LongExposureNoiseReduction2".to_string(),
                            LONG_EXPOSURE_NOISE_REDUCTION2.decode(long_exposure_nr),
                        );
                    }

                    // WBBracketMode (Perl key 9)
                    if let Some(&wb_bracket_mode) = array.get(FILE_INFO_WB_BRACKET_MODE) {
                        tags.insert(
                            "Canon:WBBracketMode".to_string(),
                            WB_BRACKET_MODE.decode(wb_bracket_mode),
                        );
                    }

                    // WBBracketValueAB / WBBracketValueGM (Perl keys 12 and 13)
                    if let Some(&value_ab) = array.get(FILE_INFO_WB_BRACKET_VALUE_AB) {
                        tags.insert("Canon:WBBracketValueAB".to_string(), value_ab.to_string());
                    }
                    if let Some(&value_gm) = array.get(FILE_INFO_WB_BRACKET_VALUE_GM) {
                        tags.insert("Canon:WBBracketValueGM".to_string(), value_gm.to_string());
                    }

                    // FilterEffect / ToningEffect (Perl keys 14 and 15).
                    // Both have `RawConv => '$val==-1 ? undef : $val'`.
                    if let Some(&filter_effect) = array.get(FILE_INFO_FILTER_EFFECT)
                        && filter_effect != -1
                    {
                        tags.insert(
                            "Canon:FilterEffect".to_string(),
                            FILTER_EFFECT.decode(filter_effect),
                        );
                    }
                    if let Some(&toning_effect) = array.get(FILE_INFO_TONING_EFFECT)
                        && toning_effect != -1
                    {
                        tags.insert(
                            "Canon:ToningEffect".to_string(),
                            TONING_EFFECT.decode(toning_effect),
                        );
                    }

                    // Legacy ShutterCount heuristic: slots 2-3 have no counterpart in
                    // %Canon::FileInfo, so only keep it where key 1 is not a FileNumber.
                    if !file_number_is_known
                        && let (Some(&low), Some(&high)) = (
                            array.get(FILE_INFO_SHUTTER_COUNT_LOW),
                            array.get(FILE_INFO_SHUTTER_COUNT_HIGH),
                        )
                    {
                        let shutter_count = ((high as u32) << 16) | (low as u32 & 0xFFFF);
                        if shutter_count > 0 {
                            tags.insert(
                                "Canon:ShutterCount".to_string(),
                                shutter_count.to_string(),
                            );
                        }
                    }
                }
            }

            // ProcessingInfo (tag 0x00A0) - ExifTool `%Image::ExifTool::Canon::Processing`
            // (Canon.pm:7201), `FORMAT => 'int16s'`, `FIRST_ENTRY => 1`.
            CANON_PROCESSING_INFO => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order)
                    .map(realign_length_prefixed_record)
                {
                    if let Some(&tone_curve) = array.get(PROCESSING_INFO_TONE_CURVE) {
                        tags.insert(
                            "Canon:ToneCurve".to_string(),
                            TONE_CURVE.decode(tone_curve as i32),
                        );
                    }

                    // Key 2 (Sharpness) is deliberately left alone: it is excluded on the
                    // 20D/350D by ExifTool's Condition and carries `Priority => 0`
                    // elsewhere, so CameraSettings key 15 stays authoritative.

                    if let Some(&frequency) = array.get(PROCESSING_INFO_SHARPNESS_FREQ) {
                        tags.insert(
                            "Canon:SharpnessFrequency".to_string(),
                            SHARPNESS_FREQUENCY.decode(frequency),
                        );
                    }

                    for (index, name) in [
                        (PROCESSING_INFO_SENSOR_RED_LEVEL, "Canon:SensorRedLevel"),
                        (PROCESSING_INFO_SENSOR_BLUE_LEVEL, "Canon:SensorBlueLevel"),
                        (PROCESSING_INFO_WHITE_BALANCE_RED, "Canon:WhiteBalanceRed"),
                        (PROCESSING_INFO_WHITE_BALANCE_BLUE, "Canon:WhiteBalanceBlue"),
                        (PROCESSING_INFO_COLOR_TEMPERATURE, "Canon:ColorTemperature"),
                        (PROCESSING_INFO_WB_SHIFT_AB, "Canon:WBShiftAB"),
                        (PROCESSING_INFO_WB_SHIFT_GM, "Canon:WBShiftGM"),
                    ] {
                        if let Some(&value) = array.get(index) {
                            tags.insert(name.to_string(), value.to_string());
                        }
                    }

                    // WhiteBalance (key 8). `RawConv => '$val < 0 ? undef : $val'` — the
                    // -32768 sentinel means "not recorded here".
                    if let Some(&white_balance) = array.get(PROCESSING_INFO_WHITE_BALANCE)
                        && white_balance >= 0
                    {
                        tags.insert(
                            "Canon:WhiteBalance".to_string(),
                            WHITE_BALANCE.decode(white_balance),
                        );
                    }

                    if let Some(&picture_style) = array.get(PROCESSING_INFO_PICTURE_STYLE) {
                        tags.insert(
                            "Canon:PictureStyle".to_string(),
                            PICTURE_STYLE.decode(picture_style as u16 as i32),
                        );
                    }

                    // DigitalGain (key 11). `ValueConv => '$val / 10'`.
                    if let Some(&digital_gain) = array.get(PROCESSING_INFO_DIGITAL_GAIN) {
                        tags.insert(
                            "Canon:DigitalGain".to_string(),
                            format_perl_number(digital_gain as f64 / 10.0),
                        );
                    }
                }
            }

            // MeasuredColor (tag 0x00AA) - ExifTool `%Canon::MeasuredColor` key 1 is a
            // single `int16u[4]` value, not four scalars.
            CANON_MEASURED_COLOR => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order)
                    .map(realign_length_prefixed_record)
                    && array.len() >= MEASURED_COLOR_RGGB + 4
                {
                    tags.insert(
                        "Canon:MeasuredRGGB".to_string(),
                        array[MEASURED_COLOR_RGGB..MEASURED_COLOR_RGGB + 4]
                            .iter()
                            .map(|v| (*v as u16).to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }
            }

            // ColorData (tag 0x4001) - only the 582-element ColorData1 layout (20D and
            // 350D) is implemented; every other element count selects a different
            // ExifTool table whose indices do not line up.
            CANON_COLOR_DATA => {
                if let Some(raw_record) = extract_canon_i16_array(entry, ifd_data, byte_order) {
                    // ExifTool picks the ColorData table by the record's declared element
                    // count, so that count is read before any realignment shortens it.
                    let declared_elements = raw_record.len();
                    let array = realign_length_prefixed_record(raw_record);
                    if declared_elements == COLOR_DATA1_ELEMENT_COUNT {
                        for &(levels_index, temp_index, suffix) in COLOR_DATA1_WB_PRESETS {
                            if let Some(levels) = array.get(levels_index..levels_index + 4) {
                                tags.insert(
                                    format!("Canon:WB_RGGBLevels{}", suffix),
                                    join_i16_slice(levels),
                                );
                            }
                            if let Some(&temperature) = array.get(temp_index) {
                                tags.insert(
                                    format!("Canon:ColorTemp{}", suffix),
                                    temperature.to_string(),
                                );
                            }
                        }
                    }
                }
            }

            // AFInfo (tag 0x0012) - autofocus information used by older Canon models
            CANON_AF_INFO => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order) {
                    let num_points = array.get(AF_INFO_NUM_AF_POINTS).copied().unwrap_or(0);
                    if num_points > 0 {
                        tags.insert("Canon:NumAFPoints".to_string(), num_points.to_string());
                    }

                    // ValidAFPoints (key 1), CanonImageWidth (key 2), CanonImageHeight
                    // (key 3) - scalars ExifTool reports alongside the AF geometry.
                    if let Some(&valid_points) = array.get(AF_INFO_VALID_AF_POINTS) {
                        tags.insert("Canon:ValidAFPoints".to_string(), valid_points.to_string());
                    }
                    if let Some(&width) = array.get(AF_INFO_CANON_IMAGE_WIDTH) {
                        tags.insert("Canon:CanonImageWidth".to_string(), width.to_string());
                    }
                    if let Some(&height) = array.get(AF_INFO_CANON_IMAGE_HEIGHT) {
                        tags.insert("Canon:CanonImageHeight".to_string(), height.to_string());
                    }

                    if let Some(&width) = array.get(AF_INFO_AF_IMAGE_WIDTH)
                        && width > 0
                    {
                        tags.insert("Canon:AFImageWidth".to_string(), width.to_string());
                    }
                    if let Some(&height) = array.get(AF_INFO_AF_IMAGE_HEIGHT)
                        && height > 0
                    {
                        tags.insert("Canon:AFImageHeight".to_string(), height.to_string());
                    }
                    if let Some(&area_width) = array.get(AF_INFO_AF_AREA_WIDTH) {
                        tags.insert("Canon:AFAreaWidth".to_string(), area_width.to_string());
                    }
                    if let Some(&area_height) = array.get(AF_INFO_AF_AREA_HEIGHT) {
                        tags.insert("Canon:AFAreaHeight".to_string(), area_height.to_string());
                    }

                    // Keys 8+ are variable-length: AFAreaXPositions[n], AFAreaYPositions[n]
                    // then AFPointsInFocus as ceil(n/16) 16-bit words.
                    if num_points > 0 {
                        let n = num_points as usize;
                        let x_start = AF_INFO_VARIABLE_START;
                        let y_start = x_start + n;
                        let focus_start = y_start + n;
                        let focus_words = n.div_ceil(16);

                        if array.len() >= y_start {
                            tags.insert(
                                "Canon:AFAreaXPositions".to_string(),
                                join_i16_slice(&array[x_start..y_start]),
                            );
                        }
                        if array.len() >= focus_start {
                            tags.insert(
                                "Canon:AFAreaYPositions".to_string(),
                                join_i16_slice(&array[y_start..focus_start]),
                            );
                        }
                        if array.len() >= focus_start + focus_words {
                            tags.insert(
                                "Canon:AFPointsInFocus".to_string(),
                                decode_bits_16(&array[focus_start..focus_start + focus_words]),
                            );
                        }
                    }
                }
            }

            // AFInfo2 (tag 0x0026) - autofocus information used by newer Canon models
            CANON_AF_INFO2 => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order) {
                    if let Some(&mode) = array.get(AF_INFO2_AF_AREA_MODE) {
                        tags.insert("Canon:AFAreaMode".to_string(), AF_AREA_MODE.decode(mode));
                    }

                    let num_points = array.get(AF_INFO2_NUM_AF_POINTS).copied().unwrap_or(0);
                    if num_points > 0 {
                        tags.insert("Canon:NumAFPoints".to_string(), num_points.to_string());
                    }

                    if let Some(&width) = array.get(AF_INFO2_AF_IMAGE_WIDTH)
                        && width > 0
                    {
                        tags.insert("Canon:AFImageWidth".to_string(), width.to_string());
                    }
                    if let Some(&height) = array.get(AF_INFO2_AF_IMAGE_HEIGHT)
                        && height > 0
                    {
                        tags.insert("Canon:AFImageHeight".to_string(), height.to_string());
                    }

                    // Keys 8+ are variable-length: AFAreaWidths[n], AFAreaHeights[n],
                    // AFAreaXPositions[n], AFAreaYPositions[n], then AFPointsInFocus as
                    // ceil(n/16) 16-bit words.
                    if num_points > 0 {
                        let n = num_points as usize;
                        let widths_start = AF_INFO2_VARIABLE_START;
                        let heights_start = widths_start + n;
                        let x_start = heights_start + n;
                        let y_start = x_start + n;
                        let focus_start = y_start + n;
                        let focus_words = n.div_ceil(16);

                        if array.len() >= heights_start {
                            tags.insert(
                                "Canon:AFAreaWidths".to_string(),
                                join_i16_slice(&array[widths_start..heights_start]),
                            );
                        }
                        if array.len() >= x_start {
                            tags.insert(
                                "Canon:AFAreaHeights".to_string(),
                                join_i16_slice(&array[heights_start..x_start]),
                            );
                        }
                        if array.len() >= y_start {
                            tags.insert(
                                "Canon:AFAreaXPositions".to_string(),
                                join_i16_slice(&array[x_start..y_start]),
                            );
                        }
                        if array.len() >= focus_start {
                            tags.insert(
                                "Canon:AFAreaYPositions".to_string(),
                                join_i16_slice(&array[y_start..focus_start]),
                            );
                        }
                        if array.len() >= focus_start + focus_words {
                            tags.insert(
                                "Canon:AFPointsInFocus".to_string(),
                                decode_bits_16(&array[focus_start..focus_start + focus_words]),
                            );
                        }
                    }
                }
            }

            // CustomFunctions (tag 0x000F).
            //
            // ExifTool Canon.pm:1500 picks a per-body table; only
            // `%CanonCustom::Functions350D` (CanonCustom.pm:809) is implemented here, so
            // the record is skipped on every other body rather than decoded with the
            // wrong labels. `ProcessCanonCustom` (CanonCustom.pm:2772) reads one int16
            // per entry after a leading byte-length word and splits it into
            // `tag = $val >> 8` / `value = $val & 0xff`.
            CANON_CUSTOM_FUNCTIONS => {
                if is_350d_custom_functions(&model)
                    && let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order)
                    && array.first().map(|&len| len as u16 as usize) == Some(array.len() * 2)
                {
                    for &word in &array[1..] {
                        let raw = word as u16;
                        let function = raw >> 8;
                        let value = (raw & 0xff) as i16;
                        let (name, rendered) = match function {
                            0 => (
                                "Canon:SetButtonCrossKeysFunc",
                                CC350D_SET_BUTTON_CROSS_KEYS_FUNC.decode(value),
                            ),
                            1 => (
                                "Canon:LongExposureNoiseReduction",
                                CC350D_LONG_EXPOSURE_NOISE_REDUCTION.decode(value),
                            ),
                            2 => (
                                "Canon:FlashSyncSpeedAv",
                                CC350D_FLASH_SYNC_SPEED_AV.decode(value),
                            ),
                            3 => ("Canon:Shutter-AELock", CC350D_SHUTTER_AE_LOCK.decode(value)),
                            4 => ("Canon:AFAssistBeam", CC350D_AF_ASSIST_BEAM.decode(value)),
                            5 => (
                                "Canon:ExposureLevelIncrements",
                                CC350D_EXPOSURE_LEVEL_INCREMENTS.decode(value),
                            ),
                            6 => ("Canon:MirrorLockup", CC350D_MIRROR_LOCKUP.decode(value)),
                            7 => ("Canon:ETTLII", CC350D_ETTL_II.decode(value)),
                            8 => (
                                "Canon:ShutterCurtainSync",
                                CC350D_SHUTTER_CURTAIN_SYNC.decode(value),
                            ),
                            _ => continue,
                        };
                        tags.insert(name.to_string(), rendered);
                    }
                }
            }

            // SensorInfo (tag 0x00E0) - sensor dimensions, image borders, black mask
            CANON_SENSOR_INFO => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order)
                    .map(realign_length_prefixed_record)
                {
                    for (index, name) in [
                        (SENSOR_INFO_SENSOR_WIDTH, "Canon:SensorWidth"),
                        (SENSOR_INFO_SENSOR_HEIGHT, "Canon:SensorHeight"),
                        (SENSOR_INFO_SENSOR_LEFT_BORDER, "Canon:SensorLeftBorder"),
                        (SENSOR_INFO_SENSOR_TOP_BORDER, "Canon:SensorTopBorder"),
                        (SENSOR_INFO_SENSOR_RIGHT_BORDER, "Canon:SensorRightBorder"),
                        (SENSOR_INFO_SENSOR_BOTTOM_BORDER, "Canon:SensorBottomBorder"),
                        (
                            SENSOR_INFO_BLACK_MASK_LEFT_BORDER,
                            "Canon:BlackMaskLeftBorder",
                        ),
                        (
                            SENSOR_INFO_BLACK_MASK_TOP_BORDER,
                            "Canon:BlackMaskTopBorder",
                        ),
                        (
                            SENSOR_INFO_BLACK_MASK_RIGHT_BORDER,
                            "Canon:BlackMaskRightBorder",
                        ),
                        (
                            SENSOR_INFO_BLACK_MASK_BOTTOM_BORDER,
                            "Canon:BlackMaskBottomBorder",
                        ),
                    ] {
                        if let Some(&value) = array.get(index) {
                            tags.insert(name.to_string(), value.to_string());
                        }
                    }
                }
            }

            // Other array tags - skip for now (will add in future phases)
            _ => {}
        }
    });

    Ok(tags)
}

/// Parses Canon MakerNote data into a map of tag names to values.
///
/// This is the public API that delegates to the CanonParser trait implementation.
///
/// # Parameters
/// - `data`: Raw MakerNote data (may include Canon signature)
/// - `byte_order`: Byte order for parsing (usually matches TIFF header)
/// - `tags`: Mutable reference to HashMap to populate with extracted tags
///
/// # Example
/// ```ignore
/// use std::collections::HashMap;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
///
/// let mut tags = HashMap::new();
/// parse_canon_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
/// ```
pub fn parse_canon_makernotes(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = CanonParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("Canon MakerNotes parse error: {}", e);
    }
}

/// Extracts inline value bytes from the value_offset field.
///
/// For values that fit in 4 bytes or less, they are stored directly
/// in the value_offset field rather than at an external offset.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canon_tag_ids() {
        assert_eq!(CANON_CAMERA_SETTINGS, 0x0001);
        assert_eq!(CANON_FOCAL_LENGTH, 0x0002);
        assert_eq!(CANON_SHOT_INFO, 0x0004);
        assert_eq!(CANON_MODEL_ID, 0x0010);
    }

    #[test]
    fn test_canon_signature() {
        assert_eq!(CANON_SIGNATURE, b"Canon");
    }

    #[test]
    fn test_canon_tag_to_name() {
        assert_eq!(canon_tag_to_name(0x0001), "Canon:CameraSettings");
        assert_eq!(canon_tag_to_name(0x0002), "Canon:FocalLength");
        assert_eq!(canon_tag_to_name(0x0004), "Canon:ShotInfo");
        assert_eq!(canon_tag_to_name(0x0006), "Canon:ImageType");
        assert_eq!(canon_tag_to_name(0x0007), "Canon:FirmwareVersion");
        assert_eq!(canon_tag_to_name(0x0010), "Canon:CanonModelID");

        // Unknown tag
        assert_eq!(canon_tag_to_name(0xFFFF), "Canon:Unknown-0xFFFF");
    }

    #[test]
    fn test_is_canon_makernote() {
        // With Canon signature
        let data_with_sig = b"Canon\x00\x01\x00\x02\x00";
        assert!(is_canon_makernote(data_with_sig));

        // Without signature (starts with IFD)
        let data_without_sig = b"\x00\x01\x00\x02\x00";
        assert!(is_canon_makernote(data_without_sig));

        // Invalid data
        let invalid_data = b"Nikon";
        assert!(!is_canon_makernote(invalid_data));
    }

    #[test]
    fn test_parse_canon_makernote_basic() {
        // Create minimal Canon MakerNote with signature
        let mut data = Vec::new();

        // Canon signature (optional)
        data.extend_from_slice(b"Canon");

        // Simple IFD with one entry (little-endian format)
        data.extend_from_slice(&[
            0x01, 0x00, // Number of entries: 1 (little-endian)
            // Entry 1: ImageType (0x0006)
            0x06, 0x00, // Tag ID: 0x0006 (little-endian)
            0x02, 0x00, // Type: 2 = ASCII string (little-endian)
            0x0B, 0x00, 0x00, 0x00, // Count: 11 bytes (little-endian)
            0x12, 0x00, 0x00, 0x00, // Offset to data: 0x12 (18 bytes from IFD start)
            // Next IFD offset
            0x00, 0x00, 0x00, 0x00,
            // String data at offset 0x12 from IFD start (= byte 23 from data start)
            b'I', b'M', b'G', b':', b'E', b'O', b'S', b' ', b'R', b'5', 0x00,
        ]);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian);
        assert!(result.is_ok());

        let tags = result.unwrap();
        assert!(!tags.is_empty());
        assert_eq!(tags.get("Canon:ImageType"), Some(&"IMG:EOS R5".to_string()));
    }

    #[test]
    fn test_extract_i16_array_inline() {
        // Test inline array (count * 2 <= 4 bytes)
        let entry = IfdEntry {
            tag_id: CANON_FOCAL_LENGTH,
            field_type: 3, // SHORT
            value_count: 2,
            value_offset: 0x0064_0032, // Two shorts: 50, 100 (little-endian)
        };

        let result = extract_i16_array(&entry, &[], ByteOrder::LittleEndian);
        assert_eq!(result, Some(vec![50, 100]));
    }

    #[test]
    fn test_extract_i16_array_offset() {
        // Test offset-based array (count * 2 > 4 bytes)
        let entry = IfdEntry {
            tag_id: CANON_CAMERA_SETTINGS,
            field_type: 3, // SHORT
            value_count: 4,
            value_offset: 0, // Offset to data
        };

        // Data at offset 0: [1, 2, 3, 4] as little-endian shorts
        let data = vec![
            0x01, 0x00, // 1
            0x02, 0x00, // 2
            0x03, 0x00, // 3
            0x04, 0x00, // 4
        ];

        let result = extract_i16_array(&entry, &data, ByteOrder::LittleEndian);
        assert_eq!(result, Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn test_extract_i16_array_big_endian() {
        let entry = IfdEntry {
            tag_id: CANON_CAMERA_SETTINGS,
            field_type: 3,
            value_count: 3, // Use 3 values to force offset-based reading (>4 bytes)
            value_offset: 0,
        };

        // Big-endian data: [256, 512, 768]
        let data = vec![
            0x01, 0x00, // 256 (big-endian)
            0x02, 0x00, // 512 (big-endian)
            0x03, 0x00, // 768 (big-endian)
        ];

        let result = extract_i16_array(&entry, &data, ByteOrder::BigEndian);
        assert_eq!(result, Some(vec![256, 512, 768]));
    }

    #[test]
    fn test_camera_settings_indices() {
        // Verify key CameraSettings array indices are defined correctly
        assert_eq!(CAMERA_SETTINGS_MACRO_MODE, 1);
        assert_eq!(CAMERA_SETTINGS_SELF_TIMER, 2);
        assert_eq!(CAMERA_SETTINGS_QUALITY, 3);
        assert_eq!(CAMERA_SETTINGS_FLASH_MODE, 4);
        assert_eq!(CAMERA_SETTINGS_DRIVE_MODE, 5);
        assert_eq!(CAMERA_SETTINGS_FOCUS_MODE, 7);
        assert_eq!(CAMERA_SETTINGS_IMAGE_SIZE, 10);
        assert_eq!(CAMERA_SETTINGS_EASY_MODE, 11);
        assert_eq!(CAMERA_SETTINGS_CONTRAST, 13);
        assert_eq!(CAMERA_SETTINGS_SATURATION, 14);
        assert_eq!(CAMERA_SETTINGS_SHARPNESS, 15);
        assert_eq!(CAMERA_SETTINGS_ISO, 16);
        assert_eq!(CAMERA_SETTINGS_METERING_MODE, 17);
        assert_eq!(CAMERA_SETTINGS_FOCUS_TYPE, 18);
        assert_eq!(CAMERA_SETTINGS_AF_POINT, 19);
        assert_eq!(CAMERA_SETTINGS_EXPOSURE_MODE, 20);
        assert_eq!(CAMERA_SETTINGS_FLASH_MODEL, 28);
        assert_eq!(CAMERA_SETTINGS_FOCUS_CONTINUOUS, 32);
    }

    #[test]
    fn test_decode_macro_mode() {
        assert_eq!(MACRO_MODE.decode(1), "Macro");
        assert_eq!(MACRO_MODE.decode(2), "Normal");
        assert_eq!(MACRO_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(QUALITY.decode(2), "Normal");
        assert_eq!(QUALITY.decode(3), "Fine");
        assert_eq!(QUALITY.decode(5), "Superfine");
        assert_eq!(QUALITY.decode(130), "Normal Movie");
        assert_eq!(QUALITY.decode(131), "Movie (2)");
        assert_eq!(QUALITY.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_flash_mode() {
        assert_eq!(FLASH_MODE.decode(0), "Off");
        assert_eq!(FLASH_MODE.decode(1), "Auto");
        assert_eq!(FLASH_MODE.decode(2), "On");
        assert_eq!(FLASH_MODE.decode(3), "Red-eye Reduction");
        assert_eq!(FLASH_MODE.decode(4), "Slow Sync");
        assert_eq!(FLASH_MODE.decode(5), "Auto + Red-eye Reduction");
        assert_eq!(FLASH_MODE.decode(6), "On + Red-eye Reduction");
        assert_eq!(FLASH_MODE.decode(16), "External Flash");
        assert_eq!(FLASH_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_drive_mode() {
        assert_eq!(DRIVE_MODE.decode(0), "Single");
        assert_eq!(DRIVE_MODE.decode(1), "Continuous");
        assert_eq!(DRIVE_MODE.decode(2), "Movie");
        assert_eq!(DRIVE_MODE.decode(4), "Continuous, Speed Priority");
        assert_eq!(DRIVE_MODE.decode(5), "Continuous, Low");
        assert_eq!(DRIVE_MODE.decode(6), "Continuous, High");
        assert_eq!(DRIVE_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(FOCUS_MODE.decode(0), "One-shot AF");
        assert_eq!(FOCUS_MODE.decode(1), "AI Servo AF");
        assert_eq!(FOCUS_MODE.decode(2), "AI Focus AF");
        assert_eq!(FOCUS_MODE.decode(3), "Manual Focus (3)");
        assert_eq!(FOCUS_MODE.decode(4), "Single");
        assert_eq!(FOCUS_MODE.decode(5), "Continuous");
        assert_eq!(FOCUS_MODE.decode(6), "Manual Focus (6)");
        assert_eq!(FOCUS_MODE.decode(16), "Pan Focus");
        assert_eq!(FOCUS_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_metering_mode() {
        assert_eq!(METERING_MODE.decode(3), "Evaluative");
        assert_eq!(METERING_MODE.decode(4), "Partial");
        assert_eq!(METERING_MODE.decode(5), "Center-weighted Average");
        assert_eq!(METERING_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_exposure_mode() {
        assert_eq!(EXPOSURE_MODE.decode(0), "Easy");
        assert_eq!(EXPOSURE_MODE.decode(1), "Program AE");
        assert_eq!(EXPOSURE_MODE.decode(2), "Shutter speed priority AE");
        assert_eq!(EXPOSURE_MODE.decode(3), "Aperture-priority AE");
        assert_eq!(EXPOSURE_MODE.decode(4), "Manual");
        assert_eq!(EXPOSURE_MODE.decode(5), "Depth-of-field AE");
        assert_eq!(EXPOSURE_MODE.decode(6), "M-Dep");
        assert_eq!(EXPOSURE_MODE.decode(7), "Bulb");
        assert_eq!(EXPOSURE_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_parse_camera_settings_array() {
        // Create Canon MakerNote with CameraSettings array
        let mut data = Vec::new();

        // Canon signature
        data.extend_from_slice(b"Canon");

        // IFD: 1 entry (CameraSettings)
        data.extend_from_slice(&[0x01, 0x00]); // Entry count (LE)

        // IFD Entry for CameraSettings (tag 0x0001)
        data.extend_from_slice(&[0x01, 0x00]); // Tag: CameraSettings
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x15, 0x00, 0x00, 0x00]); // Count: 21 values
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23 (5 sig + 2 count + 12 entry + 4 next = 23)

        // Next IFD offset
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        // CameraSettings array data at offset 20 (21 i16 values)
        let settings: Vec<i16> = vec![
            21, // [0] Array length
            2,  // [1] Macro mode: Normal
            0,  // [2] Self-timer: Off
            3,  // [3] Quality: Fine
            2,  // [4] Flash mode: On
            0,  // [5] Drive mode: Single
            0,  // [6] (unused)
            0,  // [7] Focus mode: One-shot AF
            0,  // [8] (unused)
            0,  // [9] (unused)
            1,  // [10] Image size: Large
            0,  // [11] Easy mode: Off
            0,  // [12] (unused)
            0,  // [13] Contrast: Normal
            0,  // [14] Saturation: Normal
            0,  // [15] Sharpness: Normal
            19, // [16] CameraISO code 19 -> ISO 400 (ExifTool Canon.pm:10475)
            3,  // [17] Metering mode: Evaluative
            0,  // [18] Focus type
            0,  // [19] AF point
            1,  // [20] Exposure mode: Program AE
        ];

        for value in settings {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // Verify extracted values
        assert_eq!(result.get("Canon:MacroMode"), Some(&"Normal".to_string()));
        assert_eq!(result.get("Canon:Quality"), Some(&"Fine".to_string()));
        assert_eq!(result.get("Canon:FlashMode"), Some(&"On".to_string()));
        assert_eq!(result.get("Canon:DriveMode"), Some(&"Single".to_string()));
        assert_eq!(
            result.get("Canon:FocusMode"),
            Some(&"One-shot AF".to_string())
        );
        assert_eq!(
            result.get("Canon:MeteringMode"),
            Some(&"Evaluative".to_string())
        );
        assert_eq!(
            result.get("Canon:ExposureMode"),
            Some(&"Program AE".to_string())
        );
        // `%Canon::CameraSettings` key 16 is CameraISO, whose ValueConv is a lookup
        // (Canon.pm:10464) - the slot is a code, not a literal speed, and there is no
        // `ISO` key in this table.
        assert_eq!(result.get("Canon:CameraISO"), Some(&"400".to_string()));
        assert_eq!(result.get("Canon:ISO"), None);
    }

    #[test]
    fn test_shot_info_indices() {
        assert_eq!(SHOT_INFO_AUTO_ISO, 1);
        assert_eq!(SHOT_INFO_BASE_ISO, 2);
        assert_eq!(SHOT_INFO_MEASURED_EV, 3);
        assert_eq!(SHOT_INFO_TARGET_APERTURE, 4);
        assert_eq!(SHOT_INFO_TARGET_SHUTTER_SPEED, 5);
        assert_eq!(SHOT_INFO_WHITE_BALANCE, 7);
        assert_eq!(SHOT_INFO_SLOW_SHUTTER, 8);
        assert_eq!(SHOT_INFO_SEQUENCE_NUMBER, 9);
        assert_eq!(SHOT_INFO_FLASH_GUIDE_NUMBER, 13);
        assert_eq!(SHOT_INFO_AF_POINTS_USED, 14);
        assert_eq!(SHOT_INFO_FLASH_EXPOSURE_COMP, 15);
        assert_eq!(SHOT_INFO_AUTO_EXPOSURE_BRACKETING, 16);
        assert_eq!(SHOT_INFO_SUBJECT_DISTANCE, 19);
    }

    #[test]
    fn test_parse_shot_info_array() {
        // Build test data without Canon signature for simpler offset calculation
        // IFD structure: entry_count(2) + entry(12) + next_ifd(4) = 18 bytes header
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // ShotInfo tag (0x0004)
        data.extend_from_slice(&[0x04, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]); // Count: 20
        data.extend_from_slice(&[0x12, 0x00, 0x00, 0x00]); // Offset: 18 (right after IFD header)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // ShotInfo array (20 values) starts at offset 18
        let shot_info: Vec<i16> = vec![
            20,  // [0] Array length
            100, // [1] Auto ISO
            100, // [2] Base ISO
            128, // [3] Measured EV
            160, // [4] Target aperture (f/5.6)
            96,  // [5] Target shutter speed (1/60)
            0,   // [6] (unused)
            0,   // [7] White balance: Auto
            0,   // [8] Slow shutter: Off
            0,   // [9] Sequence number
            0, 0, 0, 0, // [10-13]
            0, // [14] AF points used
            0, // [15] Flash exposure comp
            0, // [16] Auto exposure bracketing
            0, 0,    // [17-18]
            1000, // [19] Focus distance upper (cm) = 10.00 m
        ];

        for value in shot_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // AutoISO/BaseISO are log-scale codes: ExifTool applies
        // `exp($val/32*log(2))*100` and `.../32` respectively (Canon.pm:2778, 2789),
        // so raw 100 is ISO 872 / ISO 27, not ISO 100.
        assert_eq!(result.get("Canon:AutoISO"), Some(&"872".to_string()));
        assert_eq!(result.get("Canon:BaseISO"), Some(&"27".to_string()));
        // MeasuredEV carries ExifTool's empirical +5 offset (`$val / 32 + 5`).
        assert_eq!(result.get("Canon:MeasuredEV"), Some(&"9.00".to_string()));
        assert_eq!(
            result.get("Canon:TargetAperture"),
            Some(&"f/5.7".to_string())
        );
        assert_eq!(
            result.get("Canon:TargetExposureTime"),
            Some(&"1/8".to_string())
        );
        // `PrintConv => '$val > 655.345 ? "inf" : "$val m"'` interpolates the number
        // without padding, so 1000 cm prints as "10 m".
        assert_eq!(
            result.get("Canon:FocusDistanceUpper"),
            Some(&"10 m".to_string())
        );
    }

    /// `%Canon::ShotInfo` key 27 is AutoRotate; keys 26 and 28 are CameraType and
    /// NDFilter, so an off-by-one lands on a neighbour with a different meaning.
    #[test]
    fn test_parse_shot_info_auto_rotate() {
        let mut shot_info = vec![0i16; 30];
        shot_info[0] = 30;
        shot_info[26] = 252; // CameraType
        shot_info[27] = 2; // AutoRotate -> 'Rotate 180'
        shot_info[28] = -1; // NDFilter
        let data = canon_makernote_with_short_array(0x0004, &shot_info);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();
        assert_eq!(
            result.get("Canon:AutoRotate"),
            Some(&"Rotate 180".to_string())
        );

        // ExifTool's RawConv discards negatives before PrintConv, so -1 emits nothing.
        shot_info[27] = -1;
        let data = canon_makernote_with_short_array(0x0004, &shot_info);
        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();
        assert_eq!(result.get("Canon:AutoRotate"), None);
    }

    #[test]
    fn test_parse_focal_length_array() {
        // Build test data without Canon signature for simpler offset calculation
        // IFD structure: entry_count(2) + entry(12) + next_ifd(4) = 18 bytes header
        let mut data = Vec::new();
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // FocalLength tag (0x0002)
        data.extend_from_slice(&[0x02, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
        data.extend_from_slice(&[0x12, 0x00, 0x00, 0x00]); // Offset: 18 (right after IFD header)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // FocalLength array: [focal_type, focal_length, focal_plane_x_size, focal_plane_y_size]
        // focal_type: 2 (35mm equivalent available)
        // focal_length: 50mm (stored as 50)
        // focal_units: typically stored separately
        let focal_data: Vec<i16> = vec![2, 50, 0, 0];

        for value in focal_data {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // FocalType value 2 is decoded to "Zoom" using FOCAL_TYPE decoder
        assert_eq!(result.get("Canon:FocalType"), Some(&"Zoom".to_string()));
        assert_eq!(result.get("Canon:FocalLength"), Some(&"50 mm".to_string()));
    }

    #[test]
    fn test_parse_lens_model_tag() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // LensModel tag (0x0095)
        data.extend_from_slice(&[0x95, 0x00]); // Tag
        data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
        data.extend_from_slice(&[0x1E, 0x00, 0x00, 0x00]); // Count: 30 chars
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // Lens model string: "Canon EF 24-70mm f/2.8L II USM\0"
        let lens_name = b"Canon EF 24-70mm f/2.8L II USM\0";
        data.extend_from_slice(lens_name);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:LensModel"),
            Some(&"Canon EF 24-70mm f/2.8L II USM".to_string())
        );
    }

    /// Byte-for-byte the CanonFileInfo record of
    /// `/tmp/oxidex-exiftool-cache/combined-samples/CanonRaw.cr2` (Canon EOS 350D), as
    /// dumped by `exiftool -v3`:
    ///
    /// ```text
    ///   | | |     - Tag 0x0093 (32 bytes, int16u[16] read as undef[32]):
    ///   | | |         0558: 20 00 00 19 18 00 00 00 00 00 00 00 ff ff ff ff
    ///   | | |         0568: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    /// ```
    ///
    /// `%Canon::FileInfo` has no LensID key - key 6 is RawJpgQuality - so a lens name
    /// must never be sourced from this record.
    #[test]
    fn test_parse_file_info_350d() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x02, 0x00]); // 2 entries

        // CanonImageType (0x0006) - selects the 20D/350D FileNumber variant
        data.extend_from_slice(&[0x06, 0x00]); // Tag
        data.extend_from_slice(&[0x02, 0x00]); // Type: ASCII
        data.extend_from_slice(&[0x18, 0x00, 0x00, 0x00]); // Count: 24
        data.extend_from_slice(&[0x23, 0x00, 0x00, 0x00]); // Offset: 35 (5 sig + 30 IFD)
        // FileInfo tag (0x0093)
        data.extend_from_slice(&[0x93, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]); // Count: 16
        data.extend_from_slice(&[0x3B, 0x00, 0x00, 0x00]); // Offset: 59 (35 + 24)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        let mut model = b"Canon EOS 350D DIGITAL".to_vec();
        model.resize(24, 0);
        data.extend_from_slice(&model);

        let file_info: Vec<i16> = vec![
            0x0020, // [0] record length in bytes
            0x1900, // [1] FileNumber low half (int32u spans slots 1-2)
            0x0018, // [2] FileNumber high half
            0,      // [3] BracketMode
            0,      // [4] BracketValue
            0,      // [5] BracketShotNumber
            -1,     // [6] RawJpgQuality (dropped: <= 0)
            -1,     // [7] RawJpgSize (dropped: < 0)
            0,      // [8] LongExposureNoiseReduction2
            0,      // [9] WBBracketMode
            0, 0, // [10-11]
            0, // [12] WBBracketValueAB
            0, // [13] WBBracketValueGM
            0, // [14] FilterEffect
            0, // [15] ToningEffect
        ];

        for value in file_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // Literal strings below are exactly what `exiftool -s` prints for this record.
        assert_eq!(
            result.get("Canon:FileNumber"),
            Some(&"100-0024".to_string())
        );
        assert_eq!(result.get("Canon:BracketMode"), Some(&"Off".to_string()));
        assert_eq!(
            result.get("Canon:LongExposureNoiseReduction2"),
            Some(&"Off".to_string())
        );
        assert_eq!(result.get("Canon:WBBracketMode"), Some(&"Off".to_string()));
        assert_eq!(result.get("Canon:WBBracketValueAB"), Some(&"0".to_string()));
        assert_eq!(result.get("Canon:WBBracketValueGM"), Some(&"0".to_string()));
        assert_eq!(result.get("Canon:FilterEffect"), Some(&"None".to_string()));
        assert_eq!(result.get("Canon:ToningEffect"), Some(&"None".to_string()));
        // %Canon::FileInfo defines neither of these - they must not be invented.
        assert_eq!(result.get("Canon:LensType"), None);
        assert_eq!(result.get("Canon:ShutterCount"), None);
    }

    /// Wraps a single Canon MakerNote IFD entry holding a SHORT array.
    fn canon_makernote_with_short_array(tag: u16, values: &[i16]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&(values.len() as u32).to_le_bytes());
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD
        for value in values {
            data.extend_from_slice(&value.to_le_bytes());
        }
        data
    }

    /// Byte-for-byte the CanonAFInfo record of
    /// `/tmp/oxidex-exiftool-cache/combined-samples/CanonRaw.cr2` (Canon EOS 350D), as
    /// dumped by `exiftool -v3`:
    ///
    /// ```text
    ///   | | |     - Tag 0x0012 (48 bytes, int16u[24] read as undef[48]):
    ///   | | |         0520: 07 00 07 00 80 0d 00 09 80 0d 00 09 bd 00 bc 00
    ///   | | |         0530: 00 00 2b fb 1a fd 00 00 e6 02 d5 04 00 00 97 fd
    ///   | | |         0540: 00 00 00 00 00 00 00 00 00 00 69 02 08 00 ff ff
    /// ```
    #[test]
    fn test_parse_af_info_array() {
        let af_info: Vec<i16> = vec![
            7, 7, 3456, 2304, 3456, 2304, 189, 188, // keys 0-7
            0, -1237, -742, 0, 742, 1237, 0, // key 8: AFAreaXPositions[7]
            -617, 0, 0, 0, 0, 0, 617, // key 9: AFAreaYPositions[7]
            8,   // key 10: AFPointsInFocus (bit 3)
            -1,  // key 11
        ];
        let data = canon_makernote_with_short_array(0x0012, &af_info);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // Literal strings below are exactly what
        // `exiftool -s -G1 CanonRaw.cr2` prints for this record.
        assert_eq!(result.get("Canon:NumAFPoints"), Some(&"7".to_string()));
        assert_eq!(result.get("Canon:AFImageWidth"), Some(&"3456".to_string()));
        assert_eq!(result.get("Canon:AFImageHeight"), Some(&"2304".to_string()));
        assert_eq!(result.get("Canon:AFAreaWidth"), Some(&"189".to_string()));
        assert_eq!(result.get("Canon:AFAreaHeight"), Some(&"188".to_string()));
        assert_eq!(
            result.get("Canon:AFAreaXPositions"),
            Some(&"0 -1237 -742 0 742 1237 0".to_string())
        );
        assert_eq!(
            result.get("Canon:AFAreaYPositions"),
            Some(&"-617 0 0 0 0 0 617".to_string())
        );
        assert_eq!(result.get("Canon:AFPointsInFocus"), Some(&"3".to_string()));
        // %Canon::AFInfo has no AFPointsSelected key - it must not be invented.
        assert_eq!(result.get("Canon:AFPointsSelected"), None);
    }

    /// Mirrors the AFInfo2 record of
    /// `/tmp/oxidex-exiftool-cache/combined-samples/Canon1DmkIII.jpg`, whose
    /// `exiftool -s` output is `AFAreaMode: Single-point AF`, `NumAFPoints: 45`,
    /// `AFImageWidth: 3888`, `AFImageHeight: 2592`, `AFPointsInFocus: 13`.
    #[test]
    fn test_parse_af_info2_array() {
        let n = 45usize;
        let mut af_info2: Vec<i16> = vec![
            0,    // key 0: AFInfoSize
            2,    // key 1: AFAreaMode -> 'Single-point AF'
            45,   // key 2: NumAFPoints
            45,   // key 3: ValidAFPoints
            3888, // key 4: CanonImageWidth
            2592, // key 5: CanonImageHeight
            3888, // key 6: AFImageWidth
            2592, // key 7: AFImageHeight
        ];
        af_info2.extend(std::iter::repeat_n(112i16, n)); // key 8: AFAreaWidths
        af_info2.extend(std::iter::repeat_n(168i16, n)); // key 9: AFAreaHeights
        af_info2.extend(std::iter::repeat_n(-625i16, n)); // key 10: AFAreaXPositions
        af_info2.extend(std::iter::repeat_n(-554i16, n)); // key 11: AFAreaYPositions
        af_info2.extend_from_slice(&[0x2000, 0x0000, 0x0000]); // key 12: bit 13 set

        let data = canon_makernote_with_short_array(0x0026, &af_info2);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:AFAreaMode"),
            Some(&"Single-point AF".to_string())
        );
        assert_eq!(result.get("Canon:NumAFPoints"), Some(&"45".to_string()));
        assert_eq!(result.get("Canon:AFImageWidth"), Some(&"3888".to_string()));
        assert_eq!(result.get("Canon:AFImageHeight"), Some(&"2592".to_string()));
        assert_eq!(
            result.get("Canon:AFAreaWidths"),
            Some(&vec!["112"; n].join(" "))
        );
        assert_eq!(
            result.get("Canon:AFAreaHeights"),
            Some(&vec!["168"; n].join(" "))
        );
        assert_eq!(result.get("Canon:AFPointsInFocus"), Some(&"13".to_string()));
    }

    /// Byte-for-byte the SensorInfo record of `CanonRaw.cr2`, as dumped by
    /// `exiftool -v3`:
    ///
    /// ```text
    ///   | | |     - Tag 0x00e0 (34 bytes, int16u[17] read as undef[34]):
    ///   | | |         059e: 22 00 bc 0d 18 09 01 00 01 00 34 00 13 00 b3 0d
    ///   | | |         05ae: 12 09 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    ///   | | |         05be: 00 00
    /// ```
    #[test]
    fn test_parse_sensor_info_black_mask_borders() {
        let sensor_info: Vec<i16> = vec![
            34, 3516, 2328, 1, 1, 52, 19, 3507, 2322, // keys 0-8
            11, 22, 33, 44, // keys 9-12: BlackMask left/top/right/bottom
            0, 0, 0, 0,
        ];
        let data = canon_makernote_with_short_array(0x00E0, &sensor_info);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:BlackMaskLeftBorder"),
            Some(&"11".to_string())
        );
        assert_eq!(
            result.get("Canon:BlackMaskTopBorder"),
            Some(&"22".to_string())
        );
        assert_eq!(
            result.get("Canon:BlackMaskRightBorder"),
            Some(&"33".to_string())
        );
        assert_eq!(
            result.get("Canon:BlackMaskBottomBorder"),
            Some(&"44".to_string())
        );
    }

    /// Byte-for-byte the CanonFileInfo record of `CanonRaw.cr2`, as dumped by
    /// `exiftool -v3`:
    ///
    /// ```text
    ///   | | |     - Tag 0x0093 (32 bytes, int16u[16] read as undef[32]):
    ///   | | |         0558: 20 00 00 19 18 00 00 00 00 00 00 00 ff ff ff ff
    ///   | | |         0568: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
    /// ```
    ///
    /// with the three bracket slots given distinct values so an off-by-one cannot pass.
    #[test]
    fn test_parse_file_info_bracket_slots() {
        let file_info: Vec<i16> = vec![
            32, 0x1900, 0x0018, // keys 0-2 (key 1 is the int32u FileNumber)
            1,      // key 3: BracketMode -> 'AEB'
            7,      // key 4: BracketValue
            9,      // key 5: BracketShotNumber
            -1, -1, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let data = canon_makernote_with_short_array(0x0093, &file_info);

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(result.get("Canon:BracketMode"), Some(&"AEB".to_string()));
        assert_eq!(result.get("Canon:BracketValue"), Some(&"7".to_string()));
        assert_eq!(
            result.get("Canon:BracketShotNumber"),
            Some(&"9".to_string())
        );
    }

    #[test]
    fn test_decode_bits_16_matches_exiftool() {
        // ExifTool.pm DecodeBits: no lookup -> bit numbers joined by ',', '(none)' if empty.
        assert_eq!(decode_bits_16(&[8]), "3");
        assert_eq!(decode_bits_16(&[0]), "(none)");
        assert_eq!(decode_bits_16(&[0x0000, 0x0020]), "21");
        assert_eq!(decode_bits_16(&[0x0003]), "0,1");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = CanonParser;
        assert_eq!(parser.manufacturer_name(), "Canon");
        assert_eq!(parser.tag_prefix(), "Canon:");
    }

    #[test]
    fn test_validate_header() {
        let parser = CanonParser;

        // Test with Canon signature
        let with_signature = b"Canon\x00\x01\x00extra";
        assert!(parser.validate_header(with_signature));

        // Test without signature but valid IFD (reasonable entry count)
        let without_signature = b"\x05\x00extra_data_here_to_make_it_longer";
        assert!(parser.validate_header(without_signature));

        // Test invalid data (unreasonable entry count)
        let invalid = b"\xFF\xFF";
        assert!(!parser.validate_header(invalid));

        // Test too short data
        let too_short = b"\x01";
        assert!(!parser.validate_header(too_short));
    }

    #[test]
    fn test_lens_lookup() {
        let parser = CanonParser;

        // Test EF lens lookup
        assert!(parser.lookup_lens(368).is_some());
        assert_eq!(
            parser.lookup_lens(368),
            Some("Canon EF 24-70mm f/2.8L II USM".to_string())
        );

        // Test unknown lens
        assert_eq!(parser.lookup_lens(65000), None);
    }

    // ========================================================================
    // Tests for newly added tags (Phase 4 - Extended Canon MakerNotes)
    // ========================================================================

    #[test]
    fn test_decode_color_space() {
        assert_eq!(COLOR_SPACE.decode(1), "sRGB");
        assert_eq!(COLOR_SPACE.decode(2), "Adobe RGB");
        assert_eq!(COLOR_SPACE.decode(65535), "Uncalibrated");
        assert_eq!(COLOR_SPACE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_picture_style() {
        assert_eq!(PICTURE_STYLE.decode(0x0081), "Standard");
        assert_eq!(PICTURE_STYLE.decode(0x0082), "Portrait");
        assert_eq!(PICTURE_STYLE.decode(0x0083), "Landscape");
        assert_eq!(PICTURE_STYLE.decode(0x0084), "Neutral");
        assert_eq!(PICTURE_STYLE.decode(0x0085), "Faithful");
        assert_eq!(PICTURE_STYLE.decode(0x0086), "Monochrome");
        assert_eq!(PICTURE_STYLE.decode(0x0087), "Auto");
        assert_eq!(PICTURE_STYLE.decode(0x0088), "Fine Detail");
        assert_eq!(PICTURE_STYLE.decode(0x0021), "User Def. 1");
    }

    #[test]
    fn test_decode_tone_curve() {
        assert_eq!(TONE_CURVE.decode(0), "Standard");
        assert_eq!(TONE_CURVE.decode(1), "Manual");
        assert_eq!(TONE_CURVE.decode(2), "Custom");
        assert_eq!(TONE_CURVE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_canon_tag_to_name_extended() {
        // Test new tags added in Phase 4
        assert_eq!(canon_tag_to_name(0x0003), "Canon:FlashInfo");
        assert_eq!(canon_tag_to_name(0x0012), "Canon:AFInfo");
        assert_eq!(canon_tag_to_name(0x0015), "Canon:SerialNumberFormat");
        assert_eq!(canon_tag_to_name(0x0026), "Canon:AFInfo2");
        assert_eq!(canon_tag_to_name(0x0093), "Canon:FileInfo");
        assert_eq!(canon_tag_to_name(0x0095), "Canon:LensModel");
        assert_eq!(canon_tag_to_name(0x0096), "Canon:InternalSerialNumber");
        assert_eq!(canon_tag_to_name(0x00A0), "Canon:ProcessingInfo");
        assert_eq!(canon_tag_to_name(0x00AA), "Canon:MeasuredColor");
        assert_eq!(canon_tag_to_name(0x00B4), "Canon:ColorSpace");
        assert_eq!(canon_tag_to_name(0x00D0), "Canon:VRDOffset");
    }

    #[test]
    fn test_flash_info_indices() {
        // Verify FlashInfo array indices
        assert_eq!(FLASH_INFO_FLASH_GUIDE_NUMBER, 0);
        assert_eq!(FLASH_INFO_FLASH_THRESHOLD, 1);
    }

    #[test]
    fn test_processing_info_indices() {
        // Verify ProcessingInfo array indices
        assert_eq!(PROCESSING_INFO_TONE_CURVE, 1);
        assert_eq!(PROCESSING_INFO_SHARPNESS, 2);
        assert_eq!(PROCESSING_INFO_SHARPNESS_FREQ, 3);
        assert_eq!(PROCESSING_INFO_SENSOR_RED_LEVEL, 4);
        assert_eq!(PROCESSING_INFO_SENSOR_BLUE_LEVEL, 5);
        assert_eq!(PROCESSING_INFO_WHITE_BALANCE_RED, 6);
        assert_eq!(PROCESSING_INFO_WHITE_BALANCE_BLUE, 7);
        assert_eq!(PROCESSING_INFO_WHITE_BALANCE, 8);
        assert_eq!(PROCESSING_INFO_COLOR_TEMPERATURE, 9);
        assert_eq!(PROCESSING_INFO_PICTURE_STYLE, 10);
        assert_eq!(PROCESSING_INFO_DIGITAL_GAIN, 11);
        assert_eq!(PROCESSING_INFO_WB_SHIFT_AB, 12);
        assert_eq!(PROCESSING_INFO_WB_SHIFT_GM, 13);
    }

    /// `%Canon::MeasuredColor` has a single named key: `1 => MeasuredRGGB`, an
    /// `int16u[4]`. There are no separate red/green/blue/temperature keys, and
    /// `FIRST_ENTRY => 1` means the array does not start at index 0.
    #[test]
    fn test_measured_color_indices() {
        assert_eq!(MEASURED_COLOR_RGGB, 1);
    }

    /// ExifTool Canon.pm:2735 anchors every alternative of the FocalPlane*Size
    /// `Condition` to the end of the model name:
    ///
    /// ```text
    ///     $$self{Model} !~ /EOS/ or
    ///     $$self{Model} =~ /\b(1DS?|5D|D30|D60|10D|20D|30D|K236)$/ or
    ///     $$self{Model} =~ /\b((300D|350D|400D) DIGITAL|REBEL( XTi?)?|Kiss Digital( [NX])?)$/
    /// ```
    ///
    /// Dropping the anchor would hand every later Rebel a focal plane size read out of
    /// slots that hold something else on those bodies.
    #[test]
    fn test_focal_plane_size_supported_is_end_anchored() {
        // Non-EOS bodies always qualify.
        assert!(focal_plane_size_supported("Canon PowerShot S30"));
        // Listed bodies, at the end of the name.
        assert!(focal_plane_size_supported("Canon EOS 350D DIGITAL"));
        assert!(focal_plane_size_supported("Canon EOS DIGITAL REBEL XT"));
        assert!(focal_plane_size_supported("Canon EOS Kiss Digital N"));
        assert!(focal_plane_size_supported("Canon EOS 5D"));
        assert!(focal_plane_size_supported("Canon EOS 20D"));
        // Same tokens, but not at the end: ExifTool's `$` rejects these.
        assert!(!focal_plane_size_supported("Canon EOS REBEL T3i"));
        assert!(!focal_plane_size_supported("Canon EOS Kiss Digital X4"));
        assert!(!focal_plane_size_supported("Canon EOS 5D Mark III"));
        assert!(!focal_plane_size_supported("Canon EOS 350D DIGITAL X"));
        // Unlisted EOS bodies never qualify.
        assert!(!focal_plane_size_supported("Canon EOS R5"));
        assert!(!focal_plane_size_supported("Canon EOS 40D"));
    }

    /// The 20D/350D family selects `%Canon::FileInfo` key 1's `FileNumber` variant and
    /// `%Canon::ShotInfo` key 22's first `ExposureTime` variant (Canon.pm:6850, 2968);
    /// `%CanonCustom::Functions350D` (Canon.pm:1542) excludes the 20D from that set.
    #[test]
    fn test_model_conditions() {
        for model in [
            "Canon EOS 20D",
            "Canon EOS 350D DIGITAL",
            "Canon EOS DIGITAL REBEL XT",
            "Canon EOS Kiss Digital N",
        ] {
            assert!(is_20d_or_350d(model), "{model}");
        }
        assert!(!is_20d_or_350d("Canon EOS 30D"));
        assert!(!is_20d_or_350d("Canon EOS 400D DIGITAL"));

        assert!(is_350d_custom_functions("Canon EOS 350D DIGITAL"));
        assert!(is_350d_custom_functions("Canon EOS DIGITAL REBEL XT"));
        // The 400D redefines keys 0 and 1, and the 20D uses a different table entirely.
        assert!(!is_350d_custom_functions("Canon EOS 20D"));
        assert!(!is_350d_custom_functions("Canon EOS 400D DIGITAL"));
    }

    /// `has_word` stands in for Perl's `\b`, so a model token must not match inside a
    /// longer alphanumeric run.
    #[test]
    fn test_has_word_respects_boundaries() {
        assert!(has_word("Canon EOS 5D", "5D"));
        assert!(has_word("Canon EOS 5D Mark III", "5D"));
        assert!(!has_word("Canon EOS 15D", "5D"));
        assert!(!has_word("Canon EOS 5DS", "5D"));
    }

    // TODO: Enable these tests once ProcessingInfo array parsing is implemented
    // These tests verify correct parsing of Canon ProcessingInfo, MeasuredColor,
    // and FlashInfo arrays. Currently disabled as the parser doesn't extract
    // individual fields from these arrays.
    /*
    #[test]
    fn test_parse_processing_info_array() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // ProcessingInfo tag (0x00A0)
        data.extend_from_slice(&[0xA0, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]); // Count: 16
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // ProcessingInfo array (16 values)
        let processing_info: Vec<i16> = vec![
            16,     // [0] Array length
            0,      // [1] Tone curve: Standard
            3,      // [2] Sharpness: 3
            1,      // [3] Sharpness frequency: 1
            0,      // [4] Sensor red level
            0,      // [5] Sensor blue level
            0,      // [6] WB red
            0,      // [7] WB blue
            0,      // [8] White balance
            5500,   // [9] Color temperature: 5500K
            0x0081, // [10] Picture style: Standard
            0,      // [11] Digital gain
            0,      // [12] WB shift A-B
            0,      // [13] WB shift G-M
            0, 0, // [14-15] padding
        ];

        for value in processing_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(result.get("Canon:ToneCurve"), Some(&"Standard".to_string()));
        assert_eq!(result.get("Canon:Sharpness"), Some(&"3".to_string()));
        assert_eq!(
            result.get("Canon:SharpnessFrequency"),
            Some(&"1".to_string())
        );
        assert_eq!(
            result.get("Canon:ColorTemperature"),
            Some(&"5500 K".to_string())
        );
        assert_eq!(
            result.get("Canon:PictureStyle"),
            Some(&"Standard".to_string())
        );
    }

    #[test]
    fn test_parse_measured_color_array() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // MeasuredColor tag (0x00AA)
        data.extend_from_slice(&[0xAA, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // MeasuredColor array: [red, green, blue, temperature]
        let measured_color: Vec<i16> = vec![
            1024, // Red
            1000, // Green
            980,  // Blue
            5200, // Color temperature in K
        ];

        for value in measured_color {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:MeasuredRGGB_R"),
            Some(&"1024".to_string())
        );
        assert_eq!(
            result.get("Canon:MeasuredRGGB_G"),
            Some(&"1000".to_string())
        );
        assert_eq!(result.get("Canon:MeasuredRGGB_B"), Some(&"980".to_string()));
        assert_eq!(
            result.get("Canon:MeasuredColorTemperature"),
            Some(&"5200 K".to_string())
        );
    }

    #[test]
    fn test_parse_flash_info_array() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // FlashInfo tag (0x0003)
        data.extend_from_slice(&[0x03, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Count: 4
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // FlashInfo array: [guide_number, threshold, ...]
        let flash_info: Vec<i16> = vec![
            14,  // Guide number
            256, // Threshold
            0,   // unused
            0,   // unused
        ];

        for value in flash_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(
            result.get("Canon:FlashGuideNumber"),
            Some(&"14".to_string())
        );
        assert_eq!(result.get("Canon:FlashThreshold"), Some(&"256".to_string()));
    }
    */
}
