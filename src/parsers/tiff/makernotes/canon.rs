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
    let s = String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .trim()
        .to_string();

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
const CAMERA_SETTINGS_FLASH_ACTIVITY: usize = 28;
const CAMERA_SETTINGS_FLASH_BITS: usize = 29;
const CAMERA_SETTINGS_FOCUS_CONTINUOUS: usize = 32;
const CAMERA_SETTINGS_AE_SETTING: usize = 33;
const CAMERA_SETTINGS_ZOOM_SOURCE_WIDTH: usize = 36;
const CAMERA_SETTINGS_ZOOM_TARGET_WIDTH: usize = 37;
const CAMERA_SETTINGS_SPOT_METERING_MODE: usize = 39;
const CAMERA_SETTINGS_DISPLAY_APERTURE: usize = 40;

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
const SHOT_INFO_BULB_DURATION: usize = 24;

// FileInfo array indices (tag 0x0093)
const FILE_INFO_FILE_NUMBER: usize = 1;
const FILE_INFO_SHUTTER_COUNT_LOW: usize = 2;
const FILE_INFO_SHUTTER_COUNT_HIGH: usize = 3;
const FILE_INFO_BRACKET_MODE: usize = 4;
const FILE_INFO_BRACKET_VALUE: usize = 5;
const FILE_INFO_LENS_ID: usize = 6;

// AFInfo array indices
const AF_INFO_NUM_AF_POINTS: usize = 1;
const AF_INFO_IMAGE_WIDTH: usize = 2;
const AF_INFO_IMAGE_HEIGHT: usize = 3;
const AF_INFO_AREA_WIDTH: usize = 4;
const AF_INFO_AREA_HEIGHT: usize = 5;
const AF_INFO_POINTS_IN_FOCUS: usize = 8;
const AF_INFO_POINTS_SELECTED: usize = 9;

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
const MEASURED_COLOR_RED: usize = 0;
const MEASURED_COLOR_GREEN: usize = 1;
const MEASURED_COLOR_BLUE: usize = 2;
const MEASURED_COLOR_TEMPERATURE: usize = 3;

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
// Public to allow re-use in registry module
const_decoder!(
    pub EXPOSURE_MODE,
    i16,
    [
        (0, "Easy"),
        (1, "Program AE"),
        (2, "Shutter Priority"),
        (3, "Aperture Priority"),
        (4, "Manual"),
        (5, "Depth-of-field AE"),
        (6, "M-Dep"),
        (7, "Bulb"),
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
// Used for ControlMode in ShotInfo (index 18)
const_decoder!(
    pub CONTROL_MODE,
    i16,
    [
        (0, "Camera Local Control"),
        (3, "Computer Remote Control"),
        (4, "Camera Remote Control"),
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
    // 0xFFFF (-1 as i16) or 0 indicates infinity
    if value == -1 || value == 0 {
        return "inf".to_string();
    }
    // Canon focus distance is stored in centimeters, convert to meters
    let distance_m = (value as f64) / 100.0;
    format!("{:.2} m", distance_m)
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

    // Use shared IFD parser
    // Note: we don't propagate errors here to maintain existing behavior of
    // returning whatever tags we found even if parsing isn't perfect
    let _ = parse_ifd_entries(data, byte_order, &config, |entry, ifd_data| {
        match entry.tag_id {
            // Simple string tags (Phase 1)
            // Canon MakerNotes use TIFF-relative offsets, so we use extract_canon_string
            // which properly calculates and applies the base offset
            CANON_IMAGE_TYPE | CANON_FIRMWARE_VERSION | CANON_OWNER_NAME | CANON_SERIAL_NUMBER => {
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

            // Simple integer tags (Phase 1)
            CANON_FILE_NUMBER => {
                if let Some(value) = extract_integer_value(entry) {
                    let tag_name = canon_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, value);
                }
            }

            // CameraSettings array (Phase 2)
            // Reference: ExifTool Canon.pm CameraSettings table
            CANON_CAMERA_SETTINGS => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order) {
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

                    // ISO (index 16) - ISO speed setting
                    if array.len() > CAMERA_SETTINGS_ISO {
                        tags.insert(
                            "Canon:ISO".to_string(),
                            array[CAMERA_SETTINGS_ISO].to_string(),
                        );
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

                    // AFPoint (index 19) - AF point selected
                    if array.len() > CAMERA_SETTINGS_AF_POINT {
                        tags.insert(
                            "Canon:AFPoint".to_string(),
                            AF_POINT.decode(array[CAMERA_SETTINGS_AF_POINT]),
                        );
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
                    let focal_units = if array.len() > CAMERA_SETTINGS_FOCAL_UNITS {
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

                    // FlashActivity (index 28) - Flash fired indicator
                    if array.len() > CAMERA_SETTINGS_FLASH_ACTIVITY {
                        let flash_activity = array[CAMERA_SETTINGS_FLASH_ACTIVITY];
                        tags.insert(
                            "Canon:FlashActivity".to_string(),
                            if flash_activity == 0 {
                                "Did not fire".to_string()
                            } else {
                                "Fired".to_string()
                            },
                        );
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

                    // AESetting (index 33) - Auto exposure setting
                    if array.len() > CAMERA_SETTINGS_AE_SETTING {
                        tags.insert(
                            "Canon:AESetting".to_string(),
                            AE_SETTING.decode(array[CAMERA_SETTINGS_AE_SETTING]),
                        );
                    }

                    // ZoomSourceWidth (index 36) - Digital zoom source width
                    if array.len() > CAMERA_SETTINGS_ZOOM_SOURCE_WIDTH {
                        let width = array[CAMERA_SETTINGS_ZOOM_SOURCE_WIDTH];
                        if width > 0 {
                            tags.insert("Canon:ZoomSourceWidth".to_string(), width.to_string());
                        }
                    }

                    // ZoomTargetWidth (index 37) - Digital zoom target width
                    if array.len() > CAMERA_SETTINGS_ZOOM_TARGET_WIDTH {
                        let width = array[CAMERA_SETTINGS_ZOOM_TARGET_WIDTH];
                        if width > 0 {
                            tags.insert("Canon:ZoomTargetWidth".to_string(), width.to_string());
                        }
                    }

                    // SpotMeteringMode (index 39) - Spot metering point
                    if array.len() > CAMERA_SETTINGS_SPOT_METERING_MODE {
                        tags.insert(
                            "Canon:SpotMeteringMode".to_string(),
                            SPOT_METERING_MODE.decode(array[CAMERA_SETTINGS_SPOT_METERING_MODE]),
                        );
                    }

                    // DisplayAperture (index 40) - Displayed aperture * 10
                    if array.len() > CAMERA_SETTINGS_DISPLAY_APERTURE {
                        let display_aperture = array[CAMERA_SETTINGS_DISPLAY_APERTURE];
                        if display_aperture > 0 {
                            // Convert from f-number * 10 to actual f-number
                            let f_number = display_aperture as f64 / 10.0;
                            tags.insert(
                                "Canon:DisplayAperture".to_string(),
                                format!("f/{:.1}", f_number),
                            );
                        }
                    }
                }
            }

            // ShotInfo array (Phase 2) - Extended extraction
            // Extracts all available fields from the Canon ShotInfo array
            CANON_SHOT_INFO => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order) {
                    // AutoISO (index 1) - direct value, with fallback to CameraSettings ISO
                    if array.len() > SHOT_INFO_AUTO_ISO {
                        let auto_iso = array[SHOT_INFO_AUTO_ISO];
                        // If AutoISO is 0, try to use CameraSettings ISO if available
                        if auto_iso > 0 {
                            tags.insert("Canon:AutoISO".to_string(), auto_iso.to_string());
                        } else if let Some(iso) = tags.get("Canon:ISO") {
                            tags.insert("Canon:AutoISO".to_string(), iso.clone());
                        }
                    }

                    // BaseISO (index 2) - may need conversion from APEX or different index
                    // Note: For some camera models, BaseISO encoding varies
                    if array.len() > SHOT_INFO_BASE_ISO {
                        let base_iso_raw = array[SHOT_INFO_BASE_ISO];
                        // Try to interpret the value - if it matches common ISO values, use it
                        // Otherwise try fallback to CameraSettings ISO
                        let base_iso = if base_iso_raw > 0 && base_iso_raw < 1000 {
                            base_iso_raw.to_string()
                        } else if let Some(iso) = tags.get("Canon:ISO") {
                            iso.clone()
                        } else {
                            base_iso_raw.to_string()
                        };
                        tags.insert("Canon:BaseISO".to_string(), base_iso.clone());

                        // CameraISO - use AutoISO if > 0, otherwise BaseISO
                        if array.len() > SHOT_INFO_AUTO_ISO && array[SHOT_INFO_AUTO_ISO] > 0 {
                            tags.insert(
                                "Canon:CameraISO".to_string(),
                                array[SHOT_INFO_AUTO_ISO].to_string(),
                            );
                        } else {
                            tags.insert("Canon:CameraISO".to_string(), base_iso);
                        }
                    }

                    // MeasuredEV (index 3) - format as EV value
                    // Use unsigned value and different scaling for MeasuredEV
                    if array.len() > SHOT_INFO_MEASURED_EV {
                        let raw_value = array[SHOT_INFO_MEASURED_EV] as u16;
                        let ev = raw_value as f64 / 32.0;
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

                    // ExposureCompensation (index 6) - format as EV
                    if array.len() > SHOT_INFO_EXPOSURE_COMPENSATION {
                        tags.insert(
                            "Canon:ExposureCompensation".to_string(),
                            apex_to_ev(array[SHOT_INFO_EXPOSURE_COMPENSATION]),
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

                    // OpticalZoomCode (index 10) - direct value
                    if array.len() > SHOT_INFO_OPTICAL_ZOOM_CODE {
                        tags.insert(
                            "Canon:OpticalZoomCode".to_string(),
                            array[SHOT_INFO_OPTICAL_ZOOM_CODE].to_string(),
                        );
                    }

                    // FlashGuideNumber (index 13) - direct value
                    if array.len() > SHOT_INFO_FLASH_GUIDE_NUMBER {
                        tags.insert(
                            "Canon:FlashGuideNumber".to_string(),
                            array[SHOT_INFO_FLASH_GUIDE_NUMBER].to_string(),
                        );
                    }

                    // AFPointsInFocus (index 14) - bitfield decoder
                    if array.len() > SHOT_INFO_AF_POINTS_IN_FOCUS {
                        tags.insert(
                            "Canon:AFPointsInFocus".to_string(),
                            decode_af_points_in_focus(array[SHOT_INFO_AF_POINTS_IN_FOCUS]),
                        );
                    }

                    // FlashExposureComp (index 15) - format as EV
                    if array.len() > SHOT_INFO_FLASH_EXPOSURE_COMP {
                        tags.insert(
                            "Canon:FlashExposureComp".to_string(),
                            apex_to_ev(array[SHOT_INFO_FLASH_EXPOSURE_COMP]),
                        );
                    }

                    // AutoExposureBracketing (index 16) - format as EV
                    if array.len() > SHOT_INFO_AUTO_EXPOSURE_BRACKETING {
                        tags.insert(
                            "Canon:AutoExposureBracketing".to_string(),
                            apex_to_ev(array[SHOT_INFO_AUTO_EXPOSURE_BRACKETING]),
                        );
                    }

                    // AEBBracketValue (index 17) - format as EV
                    if array.len() > SHOT_INFO_AEB_BRACKET_VALUE {
                        tags.insert(
                            "Canon:AEBBracketValue".to_string(),
                            apex_to_ev(array[SHOT_INFO_AEB_BRACKET_VALUE]),
                        );
                    }

                    // ControlMode (index 18) - use decoder
                    if array.len() > SHOT_INFO_CONTROL_MODE {
                        tags.insert(
                            "Canon:ControlMode".to_string(),
                            CONTROL_MODE.decode(array[SHOT_INFO_CONTROL_MODE]),
                        );
                    }

                    // FocusDistanceUpper (index 19) - format as "X m"
                    if array.len() > SHOT_INFO_FOCUS_DISTANCE_UPPER {
                        tags.insert(
                            "Canon:FocusDistanceUpper".to_string(),
                            format_focus_distance(array[SHOT_INFO_FOCUS_DISTANCE_UPPER]),
                        );
                    }

                    // FocusDistanceLower (index 20) - format as "X m"
                    if array.len() > SHOT_INFO_FOCUS_DISTANCE_LOWER {
                        tags.insert(
                            "Canon:FocusDistanceLower".to_string(),
                            format_focus_distance(array[SHOT_INFO_FOCUS_DISTANCE_LOWER]),
                        );
                    }

                    // BulbDuration (index 24) - direct value in seconds
                    if array.len() > SHOT_INFO_BULB_DURATION {
                        let duration = array[SHOT_INFO_BULB_DURATION];
                        if duration > 0 {
                            tags.insert(
                                "Canon:BulbDuration".to_string(),
                                format!("{} s", duration),
                            );
                        }
                    }
                }
            }

            // FocalLength array (Phase 2)
            // Contains focal type (Fixed/Zoom) and focal length value
            CANON_FOCAL_LENGTH => {
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order) {
                    // array[0] = focal type (Fixed, Zoom, etc.)
                    // Uses FOCAL_TYPE decoder to convert numeric value to string
                    if !array.is_empty() {
                        tags.insert("Canon:FocalType".to_string(), FOCAL_TYPE.decode(array[0]));
                    }
                    // array[1] = focal length in mm
                    if array.len() > 1 {
                        tags.insert("Canon:FocalLength".to_string(), format!("{} mm", array[1]));
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

            // FileInfo array (Phase 3) - contains lens ID and shutter count
            CANON_FILE_INFO => {
                // FileInfo is a SHORT array
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order) {
                    // Extract lens ID (index 6)
                    if let Some(&lens_id) = array.get(FILE_INFO_LENS_ID) {
                        if lens_id > 0 {
                            // Look up lens name from database
                            if let Some(lens_name) = lookup_lens_name(lens_id as u16) {
                                tags.insert("Canon:LensType".to_string(), lens_name);
                            } else {
                                // Unknown lens - store ID
                                tags.insert("Canon:LensID".to_string(), lens_id.to_string());
                            }
                        } else {
                            // For compact cameras or fixed lenses, output "n/a"
                            tags.insert("Canon:LensType".to_string(), "n/a".to_string());
                        }
                    }

                    // Extract shutter count (combine low and high words)
                    if let (Some(&low), Some(&high)) = (
                        array.get(FILE_INFO_SHUTTER_COUNT_LOW),
                        array.get(FILE_INFO_SHUTTER_COUNT_HIGH),
                    ) {
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

            // AFInfo array (Phase 3) - autofocus point information
            CANON_AF_INFO | CANON_AF_INFO2 => {
                // AFInfo is a SHORT array
                if let Some(array) = extract_canon_i16_array(entry, ifd_data, byte_order) {
                    // Number of AF points
                    if let Some(&num_points) = array.get(AF_INFO_NUM_AF_POINTS)
                        && num_points > 0
                    {
                        tags.insert("Canon:NumAFPoints".to_string(), num_points.to_string());
                    }

                    // AF area dimensions
                    if let Some(&width) = array.get(AF_INFO_IMAGE_WIDTH)
                        && width > 0
                    {
                        tags.insert("Canon:AFImageWidth".to_string(), width.to_string());
                    }
                    if let Some(&height) = array.get(AF_INFO_IMAGE_HEIGHT)
                        && height > 0
                    {
                        tags.insert("Canon:AFImageHeight".to_string(), height.to_string());
                    }

                    // AF points in focus (bitmask)
                    if let Some(&points_in_focus) = array.get(AF_INFO_POINTS_IN_FOCUS) {
                        tags.insert(
                            "Canon:AFPointsInFocus".to_string(),
                            points_in_focus.to_string(),
                        );
                    }

                    // AF points selected (bitmask)
                    if let Some(&points_selected) = array.get(AF_INFO_POINTS_SELECTED) {
                        tags.insert(
                            "Canon:AFPointsSelected".to_string(),
                            points_selected.to_string(),
                        );
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
        assert_eq!(CAMERA_SETTINGS_FLASH_ACTIVITY, 28);
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
        assert_eq!(EXPOSURE_MODE.decode(2), "Shutter Priority");
        assert_eq!(EXPOSURE_MODE.decode(3), "Aperture Priority");
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
            80, // [16] ISO: 80
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
        assert_eq!(result.get("Canon:ISO"), Some(&"80".to_string()));
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

        assert_eq!(result.get("Canon:AutoISO"), Some(&"100".to_string()));
        assert_eq!(result.get("Canon:BaseISO"), Some(&"100".to_string()));
        assert_eq!(result.get("Canon:MeasuredEV"), Some(&"4.00".to_string()));
        assert_eq!(
            result.get("Canon:TargetAperture"),
            Some(&"f/5.7".to_string())
        );
        assert_eq!(
            result.get("Canon:TargetExposureTime"),
            Some(&"1/8".to_string())
        );
        assert_eq!(
            result.get("Canon:FocusDistanceUpper"),
            Some(&"10.00 m".to_string())
        );
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

    #[test]
    fn test_parse_file_info_with_lens_id() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // FileInfo tag (0x0093)
        data.extend_from_slice(&[0x93, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x10, 0x00, 0x00, 0x00]); // Count: 16
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // FileInfo array (16 values)
        // Based on ExifTool Canon.pm: LensID is at index 6
        let file_info: Vec<i16> = vec![
            16,  // [0] Array length
            0,   // [1] File number
            0,   // [2] Shutter count low
            0,   // [3] Shutter count high
            0,   // [4] Bracket mode
            0,   // [5] Bracket value
            368, // [6] LensID: Canon EF 24-70mm f/2.8L II USM
            0, 0, 0, 0, 0, 0, 0, 0, 0, // [7-15]
        ];

        for value in file_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        // Should extract lens name from database
        assert_eq!(
            result.get("Canon:LensType"),
            Some(&"Canon EF 24-70mm f/2.8L II USM".to_string())
        );
    }

    #[test]
    fn test_parse_af_info_array() {
        let mut data = Vec::new();
        data.extend_from_slice(b"Canon");
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // AFInfo tag (0x0012 or 0x0026)
        data.extend_from_slice(&[0x26, 0x00]); // Tag: AFInfo2
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x14, 0x00, 0x00, 0x00]); // Count: 20
        data.extend_from_slice(&[0x17, 0x00, 0x00, 0x00]); // Offset: 23
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // Next IFD

        // AFInfo array
        // Based on ExifTool: NumAFPoints at index 1, AFImageWidth at 2, AFImageHeight at 3
        let af_info: Vec<i16> = vec![
            20,     // [0] Array length
            45,     // [1] NumAFPoints (e.g., 45-point AF system)
            5568,   // [2] AFImageWidth
            3712,   // [3] AFImageHeight
            9,      // [4] AFAreaWidth
            9,      // [5] AFAreaHeight
            2784,   // [6] AFAreaXPositions (center)
            1856,   // [7] AFAreaYPositions (center)
            0x0001, // [8] AFPointsInFocus (bit 0 set = center point)
            0x0001, // [9] AFPointsSelected
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // [10-19]
        ];

        for value in af_info {
            data.extend_from_slice(&value.to_le_bytes());
        }

        let result = parse_canon_makernote_impl(&data, ByteOrder::LittleEndian).unwrap();

        assert_eq!(result.get("Canon:NumAFPoints"), Some(&"45".to_string()));
        assert_eq!(result.get("Canon:AFImageWidth"), Some(&"5568".to_string()));
        assert_eq!(result.get("Canon:AFImageHeight"), Some(&"3712".to_string()));
        assert_eq!(result.get("Canon:AFPointsInFocus"), Some(&"1".to_string()));
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

    #[test]
    fn test_measured_color_indices() {
        // Verify MeasuredColor array indices
        assert_eq!(MEASURED_COLOR_RED, 0);
        assert_eq!(MEASURED_COLOR_GREEN, 1);
        assert_eq!(MEASURED_COLOR_BLUE, 2);
        assert_eq!(MEASURED_COLOR_TEMPERATURE, 3);
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
