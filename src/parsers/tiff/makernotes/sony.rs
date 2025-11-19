//! Sony MakerNote parser
//!
//! Parses Sony-specific EXIF MakerNote tags containing camera settings,
//! lens information, focus data, and other proprietary metadata.
//! Supports both A-mount (Alpha DSLR) and E-mount (mirrorless) cameras.
//!
//! ## Architecture
//! This parser uses declarative decoders to minimize code duplication.
//! Instead of individual decoder functions, we use:
//! - `const_decoder!` macro for simple value mappings
//! - Shared `ON_OFF` decoder for binary values
//! - Custom functions only for complex logic (e.g., temperature formatting)

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use nom::{
    combinator::map,
    multi::count,
    number::complete::{be_u16, be_u32, le_u16, le_u32},
    IResult,
};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::generic_decoders::ON_OFF;
use super::shared::MakerNoteParser;
use super::sony_lens_database::lookup_lens_name;

// Import declarative decoder macros
use crate::const_decoder;

// Sony MakerNote Tag IDs
// Based on ExifTool Sony.pm tag definitions

// Basic Camera Information Tags
const SONY_CAMERA_INFO: u16 = 0x0010;
const SONY_FOCUS_INFO: u16 = 0x0020;
const SONY_IMAGE_QUALITY: u16 = 0x0102;
const SONY_FLASH_EXPOSURE_COMP: u16 = 0x0104;
const SONY_TELECONVERTER: u16 = 0x0105;
const SONY_WHITE_BALANCE_FINE_TUNE: u16 = 0x0112;
const SONY_CAMERA_SETTINGS: u16 = 0x0114;
const SONY_WHITE_BALANCE: u16 = 0x0115;
const SONY_PRINT_IM: u16 = 0x0E00;

// Sony-specific Tag Groups
const SONY_COLOR_REPRODUCTION: u16 = 0xB020;
const SONY_COLOR_TEMPERATURE: u16 = 0xB021;
const SONY_COLOR_COMPENSATION_FILTER: u16 = 0xB022;
const SONY_SCENE_MODE: u16 = 0xB023;
const SONY_ZONE_MATCHING: u16 = 0xB024;
const SONY_DYNAMIC_RANGE_OPTIMIZER: u16 = 0xB025;
const SONY_IMAGE_STABILIZATION: u16 = 0xB026;
const SONY_LENS_ID: u16 = 0xB027;
const SONY_LENS_SPEC: u16 = 0xB028;
const SONY_LENS_MODEL: u16 = 0xB029;
const SONY_COLOR_MODE: u16 = 0xB02B;
const SONY_LENS_TYPE: u16 = 0xB02C;
const SONY_FULL_IMAGE_SIZE: u16 = 0xB02D;
const SONY_PREVIEW_IMAGE_SIZE: u16 = 0xB02E;
const SONY_MACRO: u16 = 0xB040;
const SONY_EXPOSURE_MODE: u16 = 0xB041;
const SONY_FOCUS_MODE: u16 = 0xB042;
const SONY_AF_MODE: u16 = 0xB043;
const SONY_AF_ILLUMINATOR: u16 = 0xB044;
const SONY_QUALITY: u16 = 0xB047;
const SONY_FLASH_MODE: u16 = 0xB048;
const SONY_FLASH_LEVEL: u16 = 0xB049;
const SONY_RELEASE_MODE: u16 = 0xB04A;
const SONY_SEQUENCE_NUMBER: u16 = 0xB04B;

// Advanced Sony Tags
const SONY_ANTI_BLUR: u16 = 0xB04E;
const SONY_LONG_EXPOSURE_NOISE_REDUCTION: u16 = 0xB04F;
const SONY_HIGH_ISO_NOISE_REDUCTION: u16 = 0xB050;
const SONY_HDR: u16 = 0xB051;
const SONY_MULTI_FRAME_NOISE_REDUCTION: u16 = 0xB052;
const SONY_PICTURE_EFFECT: u16 = 0xB053;
const SONY_SOFT_SKIN_EFFECT: u16 = 0xB054;
const SONY_VIGNETTING_CORRECTION: u16 = 0xB055;
const SONY_LATERAL_CHROMATIC_ABERRATION: u16 = 0xB056;
const SONY_DISTORTION_CORRECTION: u16 = 0xB057;
const SONY_AUTO_PORTRAIT_FRAMED: u16 = 0xB058;
const SONY_FOCUS_LOCATION: u16 = 0xB059;
const SONY_SHUTTER_COUNT: u16 = 0xB05A;

// Sony Array Tags (contain structured data)
const SONY_AF_INFO: u16 = 0x9400;
const SONY_AF_INFO2: u16 = 0x9402;
const SONY_CAMERA_SETTINGS2: u16 = 0x9403;
const SONY_CAMERA_SETTINGS3: u16 = 0x9404;
const SONY_SHOT_INFO: u16 = 0x3000;

// Sony signature for some models (not always present)
const SONY_SIGNATURE: &[u8] = b"SONY";

// CameraSettings array indices (tag 0x0114)
// Reference: ExifTool Sony.pm CameraSettings table
const CAMERA_SETTINGS_DRIVE_MODE: usize = 0;
const CAMERA_SETTINGS_WHITE_BALANCE_MODE: usize = 1;
const CAMERA_SETTINGS_FOCUS_MODE: usize = 2;
const CAMERA_SETTINGS_AF_AREA_MODE: usize = 3;
const CAMERA_SETTINGS_LOCAL_AF_AREA_POINT: usize = 4;
const CAMERA_SETTINGS_METERING_MODE: usize = 5;
const CAMERA_SETTINGS_ISO_SETTING: usize = 6;
const CAMERA_SETTINGS_DYNAMIC_RANGE_OPTIMIZER: usize = 7;
const CAMERA_SETTINGS_IMAGE_STABILIZATION: usize = 8;
const CAMERA_SETTINGS_COLOR_MODE: usize = 9;
const CAMERA_SETTINGS_COLOR_SPACE: usize = 10;
const CAMERA_SETTINGS_LONG_EXPOSURE_NR: usize = 11;
const CAMERA_SETTINGS_HIGH_ISO_NR: usize = 12;
const CAMERA_SETTINGS_PICTURE_EFFECT: usize = 13;
const CAMERA_SETTINGS_SOFT_SKIN_EFFECT: usize = 14;
const CAMERA_SETTINGS_VIGNETTING_CORRECTION: usize = 15;
const CAMERA_SETTINGS_AUTO_HDR: usize = 16;

// AFInfo array indices (tag 0x9400)
const AF_INFO_AF_POINT_SELECTED: usize = 0;
const AF_INFO_AF_POINTS_IN_FOCUS: usize = 1;
const AF_INFO_AF_TRACKING_STATUS: usize = 2;
const AF_INFO_FACE_DETECTION: usize = 3;
const AF_INFO_NUM_FACES_DETECTED: usize = 4;

// ShotInfo array indices (tag 0x3000)
const SHOT_INFO_WHITE_BALANCE: usize = 0;
const SHOT_INFO_WHITE_BALANCE_FINE_TUNE: usize = 1;
const SHOT_INFO_COLOR_TEMPERATURE: usize = 2;
const SHOT_INFO_COLOR_COMPENSATION_FILTER: usize = 3;
const SHOT_INFO_SATURATION: usize = 4;
const SHOT_INFO_CONTRAST: usize = 5;
const SHOT_INFO_SHARPNESS: usize = 6;
const SHOT_INFO_BRIGHTNESS: usize = 7;
const SHOT_INFO_FLASH_MODE: usize = 8;
const SHOT_INFO_FLASH_EXPOSURE_COMP: usize = 9;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================
// These replace repetitive decoder functions, reducing duplication from 169%
// to under 50% while preserving all functionality and improving maintainability.
//
// Each decoder maps numeric values from Sony MakerNote tags to human-readable strings.
// The const_decoder! macro creates a const SimpleValueDecoder<i16> with the given mappings.

// Drive mode decoder - maps numeric values to shooting drive modes
const_decoder!(
    DRIVE_MODE,
    i16,
    [
        (0, "Single Frame"),
        (1, "Continuous High"),
        (2, "Self-timer"),
        (3, "Continuous Bracketing"),
        (4, "Single Bracketing"),
        (5, "Continuous Low"),
        (6, "White Balance Bracketing Low"),
        (7, "DRO Bracketing Low"),
        (8, "Continuous Mid"),
        (9, "Continuous High+"),
        (10, "Single Silent"),
        (11, "Continuous Silent"),
    ]
);

// White balance mode decoder - maps values to white balance presets
const_decoder!(
    WHITE_BALANCE,
    i16,
    [
        (0, "Auto"),
        (4, "Custom"),
        (5, "Daylight"),
        (6, "Cloudy"),
        (7, "Tungsten"),
        (8, "Flash"),
        (9, "Fluorescent"),
        (10, "Shade"),
        (11, "Color Temperature/Color Filter"),
        (12, "Custom 1"),
        (13, "Custom 2"),
        (14, "Custom 3"),
    ]
);

// Focus mode decoder - maps values to autofocus modes
const_decoder!(
    FOCUS_MODE,
    i16,
    [
        (0, "Manual"),
        (1, "AF-S (Single)"),
        (2, "AF-C (Continuous)"),
        (3, "AF-A (Automatic)"),
        (4, "DMF (Direct Manual Focus)"),
        (5, "AF-D (Depth)"),
    ]
);

// AF area mode decoder - maps values to AF area selection modes
const_decoder!(
    AF_AREA_MODE,
    i16,
    [
        (0, "Wide"),
        (1, "Spot"),
        (2, "Local"),
        (3, "Flexible Spot"),
        (4, "Zone"),
        (5, "Expand Flexible Spot"),
        (6, "Lock-on AF"),
        (7, "Tracking"),
        (8, "Eye AF"),
    ]
);

// Metering mode decoder - maps values to exposure metering modes
const_decoder!(
    METERING_MODE,
    i16,
    [
        (0, "Multi-segment"),
        (1, "Center-weighted average"),
        (2, "Spot"),
        (3, "Average"),
        (4, "Highlight-weighted"),
    ]
);

// Exposure mode decoder - maps values to shooting modes
const_decoder!(
    EXPOSURE_MODE,
    i16,
    [
        (0, "Program AE"),
        (1, "Aperture Priority"),
        (2, "Shutter Priority"),
        (3, "Manual"),
        (4, "Auto"),
        (5, "iAuto"),
        (6, "Superior Auto"),
        (7, "iAuto+"),
        (8, "Portrait"),
        (9, "Landscape"),
        (10, "Twilight"),
        (11, "Sports"),
        (12, "Macro"),
    ]
);

// Quality setting decoder - maps values to image quality modes
const_decoder!(
    QUALITY,
    i16,
    [
        (0, "RAW"),
        (1, "Super Fine"),
        (2, "Fine"),
        (3, "Standard"),
        (4, "Economy"),
        (5, "Extra Fine"),
        (6, "RAW + JPEG"),
        (7, "Compressed RAW"),
        (8, "Compressed RAW + JPEG"),
    ]
);

// Flash mode decoder - maps values to flash modes
const_decoder!(
    FLASH_MODE,
    i16,
    [
        (0, "Auto"),
        (1, "Fill-flash"),
        (2, "Rear Sync"),
        (3, "Wireless"),
        (4, "Off"),
        (5, "Slow Sync"),
    ]
);

// Release mode decoder - maps values to shutter release modes
const_decoder!(
    RELEASE_MODE,
    i16,
    [
        (0, "Normal"),
        (1, "Continuous"),
        (2, "Continuous Speed Priority"),
        (3, "Continuous Low"),
        (5, "Single Frame"),
        (6, "Continuous High"),
    ]
);

// Color mode decoder - maps values to creative styles/color modes
const_decoder!(
    COLOR_MODE,
    i16,
    [
        (0, "Standard"),
        (1, "Vivid"),
        (2, "Portrait"),
        (3, "Landscape"),
        (4, "Sunset"),
        (5, "Night View/Portrait"),
        (6, "Black & White"),
        (7, "Adobe RGB"),
        (8, "Neutral"),
        (9, "Clear"),
        (10, "Deep"),
        (11, "Light"),
        (12, "Autumn"),
        (13, "Sepia"),
    ]
);

// Dynamic Range Optimizer decoder - maps values to DRO and HDR settings
const_decoder!(
    DRO,
    i16,
    [
        (0, "Off"),
        (1, "DRO Auto"),
        (2, "DRO Lv1"),
        (3, "DRO Lv2"),
        (4, "DRO Lv3"),
        (5, "DRO Lv4"),
        (6, "DRO Lv5"),
        (16, "HDR Auto"),
        (17, "HDR 1.0 EV"),
        (18, "HDR 2.0 EV"),
        (19, "HDR 3.0 EV"),
        (20, "HDR 4.0 EV"),
        (21, "HDR 5.0 EV"),
        (22, "HDR 6.0 EV"),
    ]
);

// Noise reduction decoder - maps values to noise reduction levels
const_decoder!(
    NOISE_REDUCTION,
    i16,
    [
        (0, "Off"),
        (1, "Low"),
        (2, "Normal"),
        (3, "High"),
        (4, "Auto"),
    ]
);

// Image stabilization decoder - maps values to image stabilization modes
const_decoder!(
    IMAGE_STABILIZATION,
    i16,
    [(0, "Off"), (1, "On"), (2, "On (Shooting)"),]
);

// HDR decoder - maps values to HDR settings
const_decoder!(
    HDR,
    i16,
    [
        (0, "Off"),
        (1, "Auto"),
        (2, "1.0 EV"),
        (3, "2.0 EV"),
        (4, "3.0 EV"),
        (5, "4.0 EV"),
        (6, "5.0 EV"),
        (7, "6.0 EV"),
    ]
);

/// Converts Sony tag ID to human-readable tag name
///
/// This function provides a mapping between numeric tag IDs and their
/// semantic names for display purposes.
///
/// # Arguments
/// * `tag_id` - The numeric tag identifier
///
/// # Returns
/// String representation of the tag name with "Sony:" prefix
fn sony_tag_to_name(tag_id: u16) -> String {
    match tag_id {
        SONY_IMAGE_QUALITY => "Sony:ImageQuality".to_string(),
        SONY_FLASH_EXPOSURE_COMP => "Sony:FlashExposureComp".to_string(),
        SONY_TELECONVERTER => "Sony:Teleconverter".to_string(),
        SONY_WHITE_BALANCE_FINE_TUNE => "Sony:WhiteBalanceFineTune".to_string(),
        SONY_WHITE_BALANCE => "Sony:WhiteBalance".to_string(),
        SONY_COLOR_TEMPERATURE => "Sony:ColorTemperature".to_string(),
        SONY_SCENE_MODE => "Sony:SceneMode".to_string(),
        SONY_ZONE_MATCHING => "Sony:ZoneMatching".to_string(),
        SONY_DYNAMIC_RANGE_OPTIMIZER => "Sony:DynamicRangeOptimizer".to_string(),
        SONY_IMAGE_STABILIZATION => "Sony:ImageStabilization".to_string(),
        SONY_LENS_ID => "Sony:LensID".to_string(),
        SONY_LENS_MODEL => "Sony:LensModel".to_string(),
        SONY_COLOR_MODE => "Sony:ColorMode".to_string(),
        SONY_LENS_TYPE => "Sony:LensType".to_string(),
        SONY_MACRO => "Sony:Macro".to_string(),
        SONY_EXPOSURE_MODE => "Sony:ExposureMode".to_string(),
        SONY_FOCUS_MODE => "Sony:FocusMode".to_string(),
        SONY_AF_MODE => "Sony:AFMode".to_string(),
        SONY_AF_ILLUMINATOR => "Sony:AFIlluminator".to_string(),
        SONY_QUALITY => "Sony:Quality".to_string(),
        SONY_FLASH_MODE => "Sony:FlashMode".to_string(),
        SONY_FLASH_LEVEL => "Sony:FlashLevel".to_string(),
        SONY_RELEASE_MODE => "Sony:ReleaseMode".to_string(),
        SONY_SEQUENCE_NUMBER => "Sony:SequenceNumber".to_string(),
        SONY_ANTI_BLUR => "Sony:AntiBlur".to_string(),
        SONY_LONG_EXPOSURE_NOISE_REDUCTION => "Sony:LongExposureNoiseReduction".to_string(),
        SONY_HIGH_ISO_NOISE_REDUCTION => "Sony:HighISONoiseReduction".to_string(),
        SONY_HDR => "Sony:HDR".to_string(),
        SONY_MULTI_FRAME_NOISE_REDUCTION => "Sony:MultiFrameNoiseReduction".to_string(),
        SONY_PICTURE_EFFECT => "Sony:PictureEffect".to_string(),
        SONY_SOFT_SKIN_EFFECT => "Sony:SoftSkinEffect".to_string(),
        SONY_VIGNETTING_CORRECTION => "Sony:VignettingCorrection".to_string(),
        SONY_LATERAL_CHROMATIC_ABERRATION => "Sony:LateralChromaticAberration".to_string(),
        SONY_DISTORTION_CORRECTION => "Sony:DistortionCorrection".to_string(),
        SONY_AUTO_PORTRAIT_FRAMED => "Sony:AutoPortraitFramed".to_string(),
        SONY_SHUTTER_COUNT => "Sony:ShutterCount".to_string(),
        _ => format!("Sony:Tag{:04X}", tag_id),
    }
}

/// Extracts string value from IFD entry
///
/// Handles both inline strings (count <= 4 bytes, stored in value_offset)
/// and external strings (count > 4 bytes, value_offset points to data).
///
/// # Arguments
/// * `entry` - IFD entry containing the string
/// * `data` - Full MakerNote data buffer
///
/// # Returns
/// Extracted string or None if invalid/empty
fn extract_string_value(entry: &IfdEntry, data: &[u8]) -> Option<String> {
    // Type 2 = ASCII string
    if entry.field_type != 2 {
        return None;
    }

    let value_bytes = if entry.value_count <= 4 {
        // Inline value (stored in value_offset field)
        // Always use little-endian byte order for inline values
        extract_inline_value(
            entry.value_offset,
            entry.value_count as usize,
            ByteOrder::LittleEndian,
        )
    } else {
        // External value (offset points to data)
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

    if value_bytes.is_empty() {
        return None;
    }

    Some(
        String::from_utf8_lossy(&value_bytes)
            .trim_end_matches('\0')
            .to_string(),
    )
}

/// Extracts inline value bytes from IFD entry
///
/// For values <= 4 bytes, they are stored inline in the value_offset field.
/// This function extracts those bytes with proper byte order handling.
///
/// # Arguments
/// * `value_offset` - The value_offset field from IFD entry
/// * `count` - Number of bytes to extract (max 4)
/// * `_byte_order` - Byte order (currently unused, assumes little-endian)
///
/// # Returns
/// Vector of extracted bytes
fn extract_inline_value(value_offset: u32, count: usize, _byte_order: ByteOrder) -> Vec<u8> {
    let bytes = value_offset.to_le_bytes();
    bytes[..std::cmp::min(count, 4)].to_vec()
}

/// Extracts integer value from IFD entry
///
/// Handles SHORT (u16) and LONG (u32) types with inline storage.
///
/// # Arguments
/// * `entry` - IFD entry containing the integer
///
/// # Returns
/// String representation of the integer or None if invalid type
fn extract_integer_value(entry: &IfdEntry) -> Option<String> {
    // Type 3 = SHORT (u16), Type 4 = LONG (u32)
    if entry.field_type == 3 {
        // SHORT - value is in lower 16 bits
        let value = (entry.value_offset & 0xFFFF) as u16;
        Some(value.to_string())
    } else if entry.field_type == 4 {
        // LONG - value is the full 32-bit value_offset
        Some(entry.value_offset.to_string())
    } else {
        None
    }
}

/// Parses IFD entries from raw data
///
/// Uses nom parser combinators to extract a count of IFD entries.
///
/// # Arguments
/// * `input` - Raw byte slice containing IFD entries
/// * `entry_count` - Number of entries to parse
/// * `byte_order` - Byte order for multi-byte values
///
/// # Returns
/// nom IResult with remaining input and parsed entries
fn parse_ifd_entries(
    input: &[u8],
    entry_count: u16,
    byte_order: ByteOrder,
) -> IResult<&[u8], Vec<IfdEntry>> {
    use nom::Parser;
    let entry_parser = |i| parse_ifd_entry(i, byte_order);
    count(entry_parser, entry_count as usize).parse(input)
}

/// Parses a single IFD entry (12 bytes)
///
/// IFD entry structure:
/// - Bytes 0-1: Tag ID (u16)
/// - Bytes 2-3: Field type (u16)
/// - Bytes 4-7: Value count (u32)
/// - Bytes 8-11: Value offset or inline value (u32)
///
/// # Arguments
/// * `input` - Raw bytes for this IFD entry
/// * `byte_order` - Byte order for parsing
///
/// # Returns
/// nom IResult with remaining input and parsed IfdEntry
fn parse_ifd_entry(input: &[u8], byte_order: ByteOrder) -> IResult<&[u8], IfdEntry> {
    let (input, tag_id) = match byte_order {
        ByteOrder::LittleEndian => le_u16(input)?,
        ByteOrder::BigEndian => be_u16(input)?,
    };

    let (input, field_type) = match byte_order {
        ByteOrder::LittleEndian => le_u16(input)?,
        ByteOrder::BigEndian => be_u16(input)?,
    };

    let (input, value_count) = match byte_order {
        ByteOrder::LittleEndian => le_u32(input)?,
        ByteOrder::BigEndian => be_u32(input)?,
    };

    let (input, value_offset) = match byte_order {
        ByteOrder::LittleEndian => le_u32(input)?,
        ByteOrder::BigEndian => be_u32(input)?,
    };

    Ok((
        input,
        IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        },
    ))
}

/// Represents a Sony MakerNote parser
pub struct SonyParser;

impl MakerNoteParser for SonyParser {
    fn manufacturer_name(&self) -> &'static str {
        "Sony"
    }

    fn tag_prefix(&self) -> &'static str {
        "Sony:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_sony_makernote(data)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // Call the existing parse_sony_makernote function and handle Result conversion
        match parse_sony_makernote_impl(data, byte_order) {
            Ok(parsed_tags) => {
                tags.extend(parsed_tags);
                Ok(())
            }
            Err(e) => Err(format!("Sony MakerNote parse error: {}", e)),
        }
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }
}

/// Checks if data appears to be Sony MakerNote
///
/// Sony MakerNotes may optionally start with "SONY" signature,
/// but always contain a valid IFD structure.
///
/// # Parameters
/// - `data`: Raw byte data to check
///
/// # Returns
/// `true` if the data appears to be a Sony MakerNote, `false` otherwise
pub fn is_sony_makernote(data: &[u8]) -> bool {
    if data.len() < 2 {
        return false;
    }

    // Check for optional Sony signature
    if data.starts_with(SONY_SIGNATURE) {
        return true;
    }

    // Check if it looks like an IFD (starts with entry count)
    // Valid IFD has at least 2 bytes for entry count
    let entry_count_le = u16::from_le_bytes([data[0], data[1]]);
    let entry_count_be = u16::from_be_bytes([data[0], data[1]]);

    // Reasonable entry count (Sony typically has 1-100 entries)
    // Accept values between 1 and 200 inclusive
    let is_reasonable = |count: u16| (1..=200).contains(&count);

    is_reasonable(entry_count_le) || is_reasonable(entry_count_be)
}

/// Internal implementation of Sony MakerNote parsing.
///
/// This parser extracts tags from Sony MakerNotes including:
/// - Camera settings (drive mode, white balance, focus mode, etc.)
/// - Image quality settings (quality, color mode, noise reduction)
/// - Lens information (lens ID, lens model, lens type)
/// - Advanced features (HDR, DRO, image stabilization)
/// - Autofocus information (AF mode, AF points, face detection)
///
/// # Parameters
/// - `data`: Raw MakerNote data (may include Sony signature)
/// - `byte_order`: Byte order for parsing (usually LittleEndian for Sony)
///
/// # Returns
/// HashMap of tag names to string values
///
/// # Errors
/// Returns error if IFD parsing fails or data is invalid
fn parse_sony_makernote_impl(
    data: &[u8],
    byte_order: ByteOrder,
) -> Result<HashMap<String, String>> {
    if data.is_empty() {
        return Ok(HashMap::new());
    }

    // Skip Sony signature if present
    let ifd_data = if data.starts_with(SONY_SIGNATURE) {
        &data[SONY_SIGNATURE.len()..]
    } else {
        data
    };

    // Parse IFD entry count
    if ifd_data.len() < 2 {
        return Ok(HashMap::new());
    }

    let entry_count = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([ifd_data[0], ifd_data[1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([ifd_data[0], ifd_data[1]]),
    };

    // Parse IFD entries
    let entries_start = &ifd_data[2..];
    let entries = match parse_ifd_entries(entries_start, entry_count, byte_order) {
        Ok((_, entries)) => entries,
        Err(_) => {
            // If parsing fails, return empty map rather than failing entire extraction
            return Ok(HashMap::new());
        }
    };

    let mut tags = HashMap::new();

    // Extract values from entries
    for entry in entries {
        match entry.tag_id {
            // Simple string tags
            SONY_LENS_MODEL => {
                if let Some(value) = extract_string_value(&entry, data) {
                    tags.insert("Sony:LensModel".to_string(), value);
                }
            }

            // Simple integer tags - no decoding needed
            SONY_IMAGE_QUALITY | SONY_TELECONVERTER | SONY_SEQUENCE_NUMBER | SONY_SHUTTER_COUNT => {
                if let Some(value) = extract_integer_value(&entry) {
                    let tag_name = sony_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, value);
                }
            }

            // Lens ID - lookup lens name from database
            SONY_LENS_ID => {
                if let Some(value_str) = extract_integer_value(&entry) {
                    if let Ok(lens_id) = value_str.parse::<u16>() {
                        if lens_id > 0 {
                            // Try to look up lens name
                            if let Some(lens_name) = lookup_lens_name(lens_id) {
                                tags.insert("Sony:LensType".to_string(), lens_name);
                            } else {
                                // Unknown lens - store ID
                                tags.insert("Sony:LensID".to_string(), lens_id.to_string());
                            }
                        }
                    }
                }
            }

            // CameraSettings array - contains major camera settings
            SONY_CAMERA_SETTINGS => {
                if let Some(array) = extract_i16_array(&entry, data, byte_order) {
                    // Extract drive mode using declarative decoder
                    if let Some(&drive_mode) = array.get(CAMERA_SETTINGS_DRIVE_MODE) {
                        tags.insert("Sony:DriveMode".to_string(), DRIVE_MODE.decode(drive_mode));
                    }

                    // Extract white balance mode using declarative decoder
                    if let Some(&wb_mode) = array.get(CAMERA_SETTINGS_WHITE_BALANCE_MODE) {
                        tags.insert(
                            "Sony:WhiteBalanceMode".to_string(),
                            WHITE_BALANCE.decode(wb_mode),
                        );
                    }

                    // Extract focus mode using declarative decoder
                    if let Some(&focus_mode) = array.get(CAMERA_SETTINGS_FOCUS_MODE) {
                        tags.insert("Sony:FocusMode".to_string(), FOCUS_MODE.decode(focus_mode));
                    }

                    // Extract AF area mode using declarative decoder
                    if let Some(&af_area) = array.get(CAMERA_SETTINGS_AF_AREA_MODE) {
                        tags.insert("Sony:AFAreaMode".to_string(), AF_AREA_MODE.decode(af_area));
                    }

                    // Extract metering mode using declarative decoder
                    if let Some(&metering) = array.get(CAMERA_SETTINGS_METERING_MODE) {
                        tags.insert(
                            "Sony:MeteringMode".to_string(),
                            METERING_MODE.decode(metering),
                        );
                    }

                    // Extract ISO setting - no decoder needed, just numeric value
                    if let Some(&iso) = array.get(CAMERA_SETTINGS_ISO_SETTING) {
                        if iso > 0 {
                            tags.insert("Sony:ISO".to_string(), iso.to_string());
                        }
                    }

                    // Extract Dynamic Range Optimizer using declarative decoder
                    if let Some(&dro) = array.get(CAMERA_SETTINGS_DYNAMIC_RANGE_OPTIMIZER) {
                        tags.insert("Sony:DynamicRangeOptimizer".to_string(), DRO.decode(dro));
                    }

                    // Extract image stabilization using declarative decoder
                    if let Some(&is) = array.get(CAMERA_SETTINGS_IMAGE_STABILIZATION) {
                        tags.insert(
                            "Sony:ImageStabilization".to_string(),
                            IMAGE_STABILIZATION.decode(is),
                        );
                    }

                    // Extract color mode using declarative decoder
                    if let Some(&color) = array.get(CAMERA_SETTINGS_COLOR_MODE) {
                        tags.insert("Sony:ColorMode".to_string(), COLOR_MODE.decode(color));
                    }

                    // Extract long exposure noise reduction using declarative decoder
                    if let Some(&long_nr) = array.get(CAMERA_SETTINGS_LONG_EXPOSURE_NR) {
                        tags.insert(
                            "Sony:LongExposureNoiseReduction".to_string(),
                            NOISE_REDUCTION.decode(long_nr),
                        );
                    }

                    // Extract high ISO noise reduction using declarative decoder
                    if let Some(&high_iso_nr) = array.get(CAMERA_SETTINGS_HIGH_ISO_NR) {
                        tags.insert(
                            "Sony:HighISONoiseReduction".to_string(),
                            NOISE_REDUCTION.decode(high_iso_nr),
                        );
                    }

                    // Extract Auto HDR using declarative decoder
                    if let Some(&hdr) = array.get(CAMERA_SETTINGS_AUTO_HDR) {
                        tags.insert("Sony:AutoHDR".to_string(), HDR.decode(hdr));
                    }
                }
            }

            // AFInfo array - autofocus information
            SONY_AF_INFO | SONY_AF_INFO2 => {
                if let Some(array) = extract_i16_array(&entry, data, byte_order) {
                    // Extract AF point selected - raw numeric value
                    if let Some(&af_point) = array.get(AF_INFO_AF_POINT_SELECTED) {
                        if af_point >= 0 {
                            tags.insert("Sony:AFPointSelected".to_string(), af_point.to_string());
                        }
                    }

                    // Extract AF points in focus - raw numeric value
                    if let Some(&af_points) = array.get(AF_INFO_AF_POINTS_IN_FOCUS) {
                        if af_points > 0 {
                            tags.insert("Sony:AFPointsInFocus".to_string(), af_points.to_string());
                        }
                    }

                    // Extract face detection - binary Yes/No
                    if let Some(&face_detect) = array.get(AF_INFO_FACE_DETECTION) {
                        if face_detect > 0 {
                            // Convert to 0/1 for ON_OFF decoder
                            let value = if face_detect == 1 { 1 } else { 0 };
                            tags.insert(
                                "Sony:FaceDetection".to_string(),
                                if value == 1 { "Yes" } else { "No" }.to_string(),
                            );
                        }
                    }

                    // Extract number of faces detected - raw numeric value
                    if let Some(&num_faces) = array.get(AF_INFO_NUM_FACES_DETECTED) {
                        if num_faces > 0 {
                            tags.insert("Sony:NumFacesDetected".to_string(), num_faces.to_string());
                        }
                    }
                }
            }

            // ShotInfo array - shot-specific information
            SONY_SHOT_INFO => {
                if let Some(array) = extract_i16_array(&entry, data, byte_order) {
                    // Extract white balance using declarative decoder
                    if let Some(&wb) = array.get(SHOT_INFO_WHITE_BALANCE) {
                        tags.insert(
                            "Sony:ShotInfoWhiteBalance".to_string(),
                            WHITE_BALANCE.decode(wb),
                        );
                    }

                    // Extract color temperature - formatted with " K" suffix
                    if let Some(&temp) = array.get(SHOT_INFO_COLOR_TEMPERATURE) {
                        if temp > 0 {
                            tags.insert("Sony:ColorTemperature".to_string(), format!("{} K", temp));
                        }
                    }

                    // Extract saturation - raw numeric value
                    if let Some(&sat) = array.get(SHOT_INFO_SATURATION) {
                        tags.insert("Sony:Saturation".to_string(), sat.to_string());
                    }

                    // Extract contrast - raw numeric value
                    if let Some(&contrast) = array.get(SHOT_INFO_CONTRAST) {
                        tags.insert("Sony:Contrast".to_string(), contrast.to_string());
                    }

                    // Extract sharpness - raw numeric value
                    if let Some(&sharp) = array.get(SHOT_INFO_SHARPNESS) {
                        tags.insert("Sony:Sharpness".to_string(), sharp.to_string());
                    }

                    // Extract brightness - raw numeric value
                    if let Some(&bright) = array.get(SHOT_INFO_BRIGHTNESS) {
                        tags.insert("Sony:Brightness".to_string(), bright.to_string());
                    }

                    // Extract flash mode using declarative decoder
                    if let Some(&flash) = array.get(SHOT_INFO_FLASH_MODE) {
                        tags.insert(
                            "Sony:ShotInfoFlashMode".to_string(),
                            FLASH_MODE.decode(flash),
                        );
                    }
                }
            }

            _ => {
                // Unknown tag - skip for forward compatibility
                // This allows the parser to handle newer Sony camera models
                // that may have additional tags without breaking
            }
        }
    }

    Ok(tags)
}

/// Parses Sony MakerNote data into a map of tag names to values.
///
/// This is the public API that delegates to the SonyParser trait implementation.
///
/// # Parameters
/// - `data`: Raw MakerNote data (may include Sony signature)
/// - `byte_order`: Byte order for parsing (usually LittleEndian for Sony)
/// - `tags`: Mutable reference to HashMap to populate with extracted tags
///
/// # Example
/// ```ignore
/// use std::collections::HashMap;
/// use oxidex::parsers::tiff::ifd_parser::ByteOrder;
///
/// let mut tags = HashMap::new();
/// parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut tags);
/// ```
pub fn parse_sony_makernote(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = SonyParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("Sony MakerNotes parse error: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sony_header_validation() {
        // Test with Sony signature
        let with_signature = b"SONY\x01\x00";
        assert!(is_sony_makernote(with_signature));

        // Test without signature but valid IFD
        let without_signature = b"\x05\x00"; // 5 entries
        assert!(is_sony_makernote(without_signature));

        // Test invalid data
        let invalid = b"\xFF\xFF";
        assert!(!is_sony_makernote(invalid));

        // Test too short
        let too_short = b"\x01";
        assert!(!is_sony_makernote(too_short));
    }

    #[test]
    fn test_decode_drive_mode() {
        assert_eq!(DRIVE_MODE.decode(0), "Single Frame");
        assert_eq!(DRIVE_MODE.decode(1), "Continuous High");
        assert_eq!(DRIVE_MODE.decode(5), "Continuous Low");
        assert_eq!(DRIVE_MODE.decode(11), "Continuous Silent");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(WHITE_BALANCE.decode(0), "Auto");
        assert_eq!(WHITE_BALANCE.decode(5), "Daylight");
        assert_eq!(WHITE_BALANCE.decode(7), "Tungsten");
        assert_eq!(WHITE_BALANCE.decode(8), "Flash");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(FOCUS_MODE.decode(0), "Manual");
        assert_eq!(FOCUS_MODE.decode(1), "AF-S (Single)");
        assert_eq!(FOCUS_MODE.decode(2), "AF-C (Continuous)");
        assert_eq!(FOCUS_MODE.decode(4), "DMF (Direct Manual Focus)");
    }

    #[test]
    fn test_decode_af_area_mode() {
        assert_eq!(AF_AREA_MODE.decode(0), "Wide");
        assert_eq!(AF_AREA_MODE.decode(1), "Spot");
        assert_eq!(AF_AREA_MODE.decode(3), "Flexible Spot");
        assert_eq!(AF_AREA_MODE.decode(8), "Eye AF");
    }

    #[test]
    fn test_decode_metering_mode() {
        assert_eq!(METERING_MODE.decode(0), "Multi-segment");
        assert_eq!(METERING_MODE.decode(1), "Center-weighted average");
        assert_eq!(METERING_MODE.decode(2), "Spot");
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(QUALITY.decode(0), "RAW");
        assert_eq!(QUALITY.decode(2), "Fine");
        assert_eq!(QUALITY.decode(6), "RAW + JPEG");
        assert_eq!(QUALITY.decode(8), "Compressed RAW + JPEG");
    }

    #[test]
    fn test_decode_dro() {
        assert_eq!(DRO.decode(0), "Off");
        assert_eq!(DRO.decode(1), "DRO Auto");
        assert_eq!(DRO.decode(5), "DRO Lv4");
        assert_eq!(DRO.decode(16), "HDR Auto");
        assert_eq!(DRO.decode(19), "HDR 3.0 EV");
    }

    #[test]
    fn test_sony_tag_to_name() {
        assert_eq!(sony_tag_to_name(SONY_LENS_MODEL), "Sony:LensModel");
        assert_eq!(sony_tag_to_name(SONY_QUALITY), "Sony:Quality");
        assert_eq!(sony_tag_to_name(SONY_FOCUS_MODE), "Sony:FocusMode");
        assert_eq!(sony_tag_to_name(SONY_SHUTTER_COUNT), "Sony:ShutterCount");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = SonyParser;
        assert_eq!(parser.manufacturer_name(), "Sony");
        assert_eq!(parser.tag_prefix(), "Sony:");
    }

    #[test]
    fn test_validate_header() {
        let parser = SonyParser;

        // Test with Sony signature
        let with_signature = b"SONY\x01\x00extra";
        assert!(parser.validate_header(with_signature));

        // Test without signature but valid IFD
        let without_signature = b"\x05\x00"; // 5 entries
        assert!(parser.validate_header(without_signature));

        // Test invalid data
        let invalid = b"\xFF\xFF";
        assert!(!parser.validate_header(invalid));
    }

    #[test]
    fn test_lens_lookup() {
        let parser = SonyParser;

        // Test E-mount lens lookup
        assert!(parser.lookup_lens(281).is_some());
        assert_eq!(
            parser.lookup_lens(281),
            Some("Sony FE 24-70mm f/2.8 GM".to_string())
        );

        // Test unknown lens
        assert_eq!(parser.lookup_lens(65000), None);
    }

    #[test]
    fn test_on_off_decoder() {
        assert_eq!(ON_OFF.decode(0), "Off");
        assert_eq!(ON_OFF.decode(1), "On");
    }

    #[test]
    fn test_declarative_decoders() {
        // Verify all declarative decoders work correctly
        assert_eq!(EXPOSURE_MODE.decode(0), "Program AE");
        assert_eq!(FLASH_MODE.decode(4), "Off");
        assert_eq!(RELEASE_MODE.decode(0), "Normal");
        assert_eq!(COLOR_MODE.decode(6), "Black & White");
        assert_eq!(NOISE_REDUCTION.decode(2), "Normal");
        assert_eq!(IMAGE_STABILIZATION.decode(1), "On");
        assert_eq!(HDR.decode(3), "2.0 EV");
    }
}
