//! Sony MakerNote parser
//!
//! Parses Sony-specific EXIF MakerNote tags containing camera settings,
//! lens information, focus data, and other proprietary metadata.
//! Supports both A-mount (Alpha DSLR) and E-mount (mirrorless) cameras.

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
use super::sony_lens_database::lookup_lens_name;

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

/// Decodes Sony drive mode value to human-readable string
fn decode_drive_mode(value: i16) -> String {
    match value {
        0 => "Single Frame".to_string(),
        1 => "Continuous High".to_string(),
        2 => "Self-timer".to_string(),
        3 => "Continuous Bracketing".to_string(),
        4 => "Single Bracketing".to_string(),
        5 => "Continuous Low".to_string(),
        6 => "White Balance Bracketing Low".to_string(),
        7 => "DRO Bracketing Low".to_string(),
        8 => "Continuous Mid".to_string(),
        9 => "Continuous High+".to_string(),
        10 => "Single Silent".to_string(),
        11 => "Continuous Silent".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony white balance mode to human-readable string
fn decode_white_balance(value: i16) -> String {
    match value {
        0 => "Auto".to_string(),
        4 => "Custom".to_string(),
        5 => "Daylight".to_string(),
        6 => "Cloudy".to_string(),
        7 => "Tungsten".to_string(),
        8 => "Flash".to_string(),
        9 => "Fluorescent".to_string(),
        10 => "Shade".to_string(),
        11 => "Color Temperature/Color Filter".to_string(),
        12 => "Custom 1".to_string(),
        13 => "Custom 2".to_string(),
        14 => "Custom 3".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony focus mode to human-readable string
fn decode_focus_mode(value: i16) -> String {
    match value {
        0 => "Manual".to_string(),
        1 => "AF-S (Single)".to_string(),
        2 => "AF-C (Continuous)".to_string(),
        3 => "AF-A (Automatic)".to_string(),
        4 => "DMF (Direct Manual Focus)".to_string(),
        5 => "AF-D (Depth)".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony AF area mode to human-readable string
fn decode_af_area_mode(value: i16) -> String {
    match value {
        0 => "Wide".to_string(),
        1 => "Spot".to_string(),
        2 => "Local".to_string(),
        3 => "Flexible Spot".to_string(),
        4 => "Zone".to_string(),
        5 => "Expand Flexible Spot".to_string(),
        6 => "Lock-on AF".to_string(),
        7 => "Tracking".to_string(),
        8 => "Eye AF".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony metering mode to human-readable string
fn decode_metering_mode(value: i16) -> String {
    match value {
        0 => "Multi-segment".to_string(),
        1 => "Center-weighted average".to_string(),
        2 => "Spot".to_string(),
        3 => "Average".to_string(),
        4 => "Highlight-weighted".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony exposure mode to human-readable string
fn decode_exposure_mode(value: i16) -> String {
    match value {
        0 => "Program AE".to_string(),
        1 => "Aperture Priority".to_string(),
        2 => "Shutter Priority".to_string(),
        3 => "Manual".to_string(),
        4 => "Auto".to_string(),
        5 => "iAuto".to_string(),
        6 => "Superior Auto".to_string(),
        7 => "iAuto+".to_string(),
        8 => "Portrait".to_string(),
        9 => "Landscape".to_string(),
        10 => "Twilight".to_string(),
        11 => "Sports".to_string(),
        12 => "Macro".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony quality setting to human-readable string
fn decode_quality(value: i16) -> String {
    match value {
        0 => "RAW".to_string(),
        1 => "Super Fine".to_string(),
        2 => "Fine".to_string(),
        3 => "Standard".to_string(),
        4 => "Economy".to_string(),
        5 => "Extra Fine".to_string(),
        6 => "RAW + JPEG".to_string(),
        7 => "Compressed RAW".to_string(),
        8 => "Compressed RAW + JPEG".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony flash mode to human-readable string
fn decode_flash_mode(value: i16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "Fill-flash".to_string(),
        2 => "Rear Sync".to_string(),
        3 => "Wireless".to_string(),
        4 => "Off".to_string(),
        5 => "Slow Sync".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony release mode to human-readable string
fn decode_release_mode(value: i16) -> String {
    match value {
        0 => "Normal".to_string(),
        1 => "Continuous".to_string(),
        2 => "Continuous Speed Priority".to_string(),
        3 => "Continuous Low".to_string(),
        5 => "Single Frame".to_string(),
        6 => "Continuous High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony color mode to human-readable string
fn decode_color_mode(value: i16) -> String {
    match value {
        0 => "Standard".to_string(),
        1 => "Vivid".to_string(),
        2 => "Portrait".to_string(),
        3 => "Landscape".to_string(),
        4 => "Sunset".to_string(),
        5 => "Night View/Portrait".to_string(),
        6 => "Black & White".to_string(),
        7 => "Adobe RGB".to_string(),
        8 => "Neutral".to_string(),
        9 => "Clear".to_string(),
        10 => "Deep".to_string(),
        11 => "Light".to_string(),
        12 => "Autumn".to_string(),
        13 => "Sepia".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony Dynamic Range Optimizer setting
fn decode_dro(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "DRO Auto".to_string(),
        2 => "DRO Lv1".to_string(),
        3 => "DRO Lv2".to_string(),
        4 => "DRO Lv3".to_string(),
        5 => "DRO Lv4".to_string(),
        6 => "DRO Lv5".to_string(),
        16 => "HDR Auto".to_string(),
        17 => "HDR 1.0 EV".to_string(),
        18 => "HDR 2.0 EV".to_string(),
        19 => "HDR 3.0 EV".to_string(),
        20 => "HDR 4.0 EV".to_string(),
        21 => "HDR 5.0 EV".to_string(),
        22 => "HDR 6.0 EV".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony noise reduction setting
fn decode_noise_reduction(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Low".to_string(),
        2 => "Normal".to_string(),
        3 => "High".to_string(),
        4 => "Auto".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony image stabilization setting
fn decode_image_stabilization(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "On".to_string(),
        2 => "On (Shooting)".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Sony HDR setting
fn decode_hdr(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Auto".to_string(),
        2 => "1.0 EV".to_string(),
        3 => "2.0 EV".to_string(),
        4 => "3.0 EV".to_string(),
        5 => "4.0 EV".to_string(),
        6 => "5.0 EV".to_string(),
        7 => "6.0 EV".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Converts Sony tag ID to human-readable tag name
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
fn extract_string_value(entry: &IfdEntry, data: &[u8]) -> Option<String> {
    // Type 2 = ASCII string
    if entry.field_type != 2 {
        return None;
    }

    let value_bytes = if entry.value_count <= 4 {
        // Inline value (stored in value_offset field)
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
fn extract_inline_value(value_offset: u32, count: usize, _byte_order: ByteOrder) -> Vec<u8> {
    let bytes = value_offset.to_le_bytes();
    bytes[..std::cmp::min(count, 4)].to_vec()
}

/// Extracts integer value from IFD entry
fn extract_integer_value(entry: &IfdEntry) -> Option<String> {
    // Type 3 = SHORT (u16), Type 4 = LONG (u32)
    if entry.field_type == 3 {
        // SHORT
        let value = (entry.value_offset & 0xFFFF) as u16;
        Some(value.to_string())
    } else if entry.field_type == 4 {
        // LONG
        Some(entry.value_offset.to_string())
    } else {
        None
    }
}

/// Parses IFD entries from raw data
fn parse_ifd_entries(
    input: &[u8],
    entry_count: u16,
    byte_order: ByteOrder,
) -> IResult<&[u8], Vec<IfdEntry>> {
    let entry_parser = |i| parse_ifd_entry(i, byte_order);
    count(entry_parser, entry_count as usize)(input)
}

/// Parses a single IFD entry (12 bytes)
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

/// Parses Sony MakerNote data into a map of tag names to values.
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
pub fn parse_sony_makernote(data: &[u8], byte_order: ByteOrder) -> Result<HashMap<String, String>> {
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

            // Simple integer tags
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
                    // Extract drive mode
                    if let Some(&drive_mode) = array.get(CAMERA_SETTINGS_DRIVE_MODE) {
                        tags.insert("Sony:DriveMode".to_string(), decode_drive_mode(drive_mode));
                    }

                    // Extract white balance mode
                    if let Some(&wb_mode) = array.get(CAMERA_SETTINGS_WHITE_BALANCE_MODE) {
                        tags.insert(
                            "Sony:WhiteBalanceMode".to_string(),
                            decode_white_balance(wb_mode),
                        );
                    }

                    // Extract focus mode
                    if let Some(&focus_mode) = array.get(CAMERA_SETTINGS_FOCUS_MODE) {
                        tags.insert("Sony:FocusMode".to_string(), decode_focus_mode(focus_mode));
                    }

                    // Extract AF area mode
                    if let Some(&af_area) = array.get(CAMERA_SETTINGS_AF_AREA_MODE) {
                        tags.insert("Sony:AFAreaMode".to_string(), decode_af_area_mode(af_area));
                    }

                    // Extract metering mode
                    if let Some(&metering) = array.get(CAMERA_SETTINGS_METERING_MODE) {
                        tags.insert(
                            "Sony:MeteringMode".to_string(),
                            decode_metering_mode(metering),
                        );
                    }

                    // Extract ISO setting
                    if let Some(&iso) = array.get(CAMERA_SETTINGS_ISO_SETTING) {
                        if iso > 0 {
                            tags.insert("Sony:ISO".to_string(), iso.to_string());
                        }
                    }

                    // Extract Dynamic Range Optimizer
                    if let Some(&dro) = array.get(CAMERA_SETTINGS_DYNAMIC_RANGE_OPTIMIZER) {
                        tags.insert("Sony:DynamicRangeOptimizer".to_string(), decode_dro(dro));
                    }

                    // Extract image stabilization
                    if let Some(&is) = array.get(CAMERA_SETTINGS_IMAGE_STABILIZATION) {
                        tags.insert(
                            "Sony:ImageStabilization".to_string(),
                            decode_image_stabilization(is),
                        );
                    }

                    // Extract color mode
                    if let Some(&color) = array.get(CAMERA_SETTINGS_COLOR_MODE) {
                        tags.insert("Sony:ColorMode".to_string(), decode_color_mode(color));
                    }

                    // Extract long exposure noise reduction
                    if let Some(&long_nr) = array.get(CAMERA_SETTINGS_LONG_EXPOSURE_NR) {
                        tags.insert(
                            "Sony:LongExposureNoiseReduction".to_string(),
                            decode_noise_reduction(long_nr),
                        );
                    }

                    // Extract high ISO noise reduction
                    if let Some(&high_iso_nr) = array.get(CAMERA_SETTINGS_HIGH_ISO_NR) {
                        tags.insert(
                            "Sony:HighISONoiseReduction".to_string(),
                            decode_noise_reduction(high_iso_nr),
                        );
                    }

                    // Extract Auto HDR
                    if let Some(&hdr) = array.get(CAMERA_SETTINGS_AUTO_HDR) {
                        tags.insert("Sony:AutoHDR".to_string(), decode_hdr(hdr));
                    }
                }
            }

            // AFInfo array - autofocus information
            SONY_AF_INFO | SONY_AF_INFO2 => {
                if let Some(array) = extract_i16_array(&entry, data, byte_order) {
                    // Extract AF point selected
                    if let Some(&af_point) = array.get(AF_INFO_AF_POINT_SELECTED) {
                        if af_point >= 0 {
                            tags.insert("Sony:AFPointSelected".to_string(), af_point.to_string());
                        }
                    }

                    // Extract AF points in focus
                    if let Some(&af_points) = array.get(AF_INFO_AF_POINTS_IN_FOCUS) {
                        if af_points > 0 {
                            tags.insert("Sony:AFPointsInFocus".to_string(), af_points.to_string());
                        }
                    }

                    // Extract face detection info
                    if let Some(&face_detect) = array.get(AF_INFO_FACE_DETECTION) {
                        if face_detect > 0 {
                            tags.insert(
                                "Sony:FaceDetection".to_string(),
                                if face_detect == 1 { "Yes" } else { "No" }.to_string(),
                            );
                        }
                    }

                    // Extract number of faces detected
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
                    // Extract white balance
                    if let Some(&wb) = array.get(SHOT_INFO_WHITE_BALANCE) {
                        tags.insert(
                            "Sony:ShotInfoWhiteBalance".to_string(),
                            decode_white_balance(wb),
                        );
                    }

                    // Extract color temperature
                    if let Some(&temp) = array.get(SHOT_INFO_COLOR_TEMPERATURE) {
                        if temp > 0 {
                            tags.insert("Sony:ColorTemperature".to_string(), format!("{} K", temp));
                        }
                    }

                    // Extract saturation
                    if let Some(&sat) = array.get(SHOT_INFO_SATURATION) {
                        tags.insert("Sony:Saturation".to_string(), sat.to_string());
                    }

                    // Extract contrast
                    if let Some(&contrast) = array.get(SHOT_INFO_CONTRAST) {
                        tags.insert("Sony:Contrast".to_string(), contrast.to_string());
                    }

                    // Extract sharpness
                    if let Some(&sharp) = array.get(SHOT_INFO_SHARPNESS) {
                        tags.insert("Sony:Sharpness".to_string(), sharp.to_string());
                    }

                    // Extract brightness
                    if let Some(&bright) = array.get(SHOT_INFO_BRIGHTNESS) {
                        tags.insert("Sony:Brightness".to_string(), bright.to_string());
                    }

                    // Extract flash mode
                    if let Some(&flash) = array.get(SHOT_INFO_FLASH_MODE) {
                        tags.insert(
                            "Sony:ShotInfoFlashMode".to_string(),
                            decode_flash_mode(flash),
                        );
                    }
                }
            }

            _ => {
                // Unknown tag - skip
            }
        }
    }

    Ok(tags)
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
        assert_eq!(decode_drive_mode(0), "Single Frame");
        assert_eq!(decode_drive_mode(1), "Continuous High");
        assert_eq!(decode_drive_mode(5), "Continuous Low");
        assert_eq!(decode_drive_mode(11), "Continuous Silent");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(decode_white_balance(0), "Auto");
        assert_eq!(decode_white_balance(5), "Daylight");
        assert_eq!(decode_white_balance(7), "Tungsten");
        assert_eq!(decode_white_balance(8), "Flash");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(decode_focus_mode(0), "Manual");
        assert_eq!(decode_focus_mode(1), "AF-S (Single)");
        assert_eq!(decode_focus_mode(2), "AF-C (Continuous)");
        assert_eq!(decode_focus_mode(4), "DMF (Direct Manual Focus)");
    }

    #[test]
    fn test_decode_af_area_mode() {
        assert_eq!(decode_af_area_mode(0), "Wide");
        assert_eq!(decode_af_area_mode(1), "Spot");
        assert_eq!(decode_af_area_mode(3), "Flexible Spot");
        assert_eq!(decode_af_area_mode(8), "Eye AF");
    }

    #[test]
    fn test_decode_metering_mode() {
        assert_eq!(decode_metering_mode(0), "Multi-segment");
        assert_eq!(decode_metering_mode(1), "Center-weighted average");
        assert_eq!(decode_metering_mode(2), "Spot");
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(decode_quality(0), "RAW");
        assert_eq!(decode_quality(2), "Fine");
        assert_eq!(decode_quality(6), "RAW + JPEG");
        assert_eq!(decode_quality(8), "Compressed RAW + JPEG");
    }

    #[test]
    fn test_decode_dro() {
        assert_eq!(decode_dro(0), "Off");
        assert_eq!(decode_dro(1), "DRO Auto");
        assert_eq!(decode_dro(5), "DRO Lv4");
        assert_eq!(decode_dro(16), "HDR Auto");
        assert_eq!(decode_dro(19), "HDR 3.0 EV");
    }

    #[test]
    fn test_sony_tag_to_name() {
        assert_eq!(sony_tag_to_name(SONY_LENS_MODEL), "Sony:LensModel");
        assert_eq!(sony_tag_to_name(SONY_QUALITY), "Sony:Quality");
        assert_eq!(sony_tag_to_name(SONY_FOCUS_MODE), "Sony:FocusMode");
        assert_eq!(sony_tag_to_name(SONY_SHUTTER_COUNT), "Sony:ShutterCount");
    }
}
