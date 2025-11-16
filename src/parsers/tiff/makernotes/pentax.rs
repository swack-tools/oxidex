//! Pentax MakerNote Parser
//!
//! Parses Pentax-specific EXIF MakerNote tags containing camera settings,
//! lens information, image quality parameters, and other proprietary metadata.
//!
//! Supports all Pentax DSLR and mirrorless cameras including:
//! - K-series DSLRs (K-1, K-3, K-5, K-7, K-x, K-r, etc.)
//! - Q-series mirrorless (Q, Q7, Q10, Q-S1)
//! - istD/ist series legacy DSLRs
//!
//! Based on ExifTool's Pentax.pm module.

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

use super::pentax_lens_database::lookup_lens_name;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use super::shared::MakerNoteParser;

// ===== Pentax MakerNote Tag IDs =====
// Based on ExifTool Pentax.pm tag definitions

// Basic Camera Information Tags
const PENTAX_VERSION: u16 = 0x0000;
const PENTAX_PENTAX_MODEL_TYPE: u16 = 0x0001;
const PENTAX_PREVIEW_IMAGE_SIZE: u16 = 0x0002;
const PENTAX_PREVIEW_IMAGE_LENGTH: u16 = 0x0003;
const PENTAX_PREVIEW_IMAGE_START: u16 = 0x0004;
const PENTAX_PENTAX_MODEL_ID: u16 = 0x0005;
const PENTAX_DATE: u16 = 0x0006;
const PENTAX_TIME: u16 = 0x0007;
const PENTAX_QUALITY: u16 = 0x0008;
const PENTAX_PENTAX_IMAGE_SIZE: u16 = 0x0009;
const PENTAX_PICTURE_MODE: u16 = 0x000B;
const PENTAX_FLASH_MODE: u16 = 0x000C;
const PENTAX_FOCUS_MODE: u16 = 0x000D;
const PENTAX_AF_POINT_SELECTED: u16 = 0x000E;
const PENTAX_AF_POINT_IN_FOCUS: u16 = 0x000F;

// Image Quality and Processing
const PENTAX_ISO_SPEED: u16 = 0x0014;
const PENTAX_METERING_MODE: u16 = 0x0017;
const PENTAX_AUTO_BRACKETING: u16 = 0x0018;
const PENTAX_WHITE_BALANCE: u16 = 0x0019;
const PENTAX_WHITE_BALANCE_MODE: u16 = 0x001A;
const PENTAX_BLUE_BALANCE: u16 = 0x001B;
const PENTAX_RED_BALANCE: u16 = 0x001C;
const PENTAX_FOCAL_LENGTH: u16 = 0x001D;
const PENTAX_DIGITAL_ZOOM: u16 = 0x001E;
const PENTAX_SATURATION: u16 = 0x001F;
const PENTAX_CONTRAST: u16 = 0x0020;
const PENTAX_SHARPNESS: u16 = 0x0021;
const PENTAX_WORLD_TIME_LOCATION: u16 = 0x0022;
const PENTAX_HOMETOWN_CITY: u16 = 0x0023;
const PENTAX_DESTINATION_CITY: u16 = 0x0024;
const PENTAX_HOMETOWN_DST: u16 = 0x0025;
const PENTAX_DESTINATION_DST: u16 = 0x0026;

// Image Processing and Effects
const PENTAX_IMAGE_PROCESSING: u16 = 0x0032;
const PENTAX_PICTURE_MODE_2: u16 = 0x0033;
const PENTAX_DRIVE_MODE: u16 = 0x0034;
const PENTAX_COLOR_SPACE: u16 = 0x0037;
const PENTAX_IMAGE_AREA_OFFSET: u16 = 0x0038;
const PENTAX_RAW_IMAGE_SIZE: u16 = 0x0039;
const PENTAX_SHAKE_REDUCTION_INFO: u16 = 0x003C;
const PENTAX_SHUTTER_COUNT: u16 = 0x003D;
const PENTAX_FACE_INFO: u16 = 0x0047;
const PENTAX_RAW_DEVELOPMENT_PARAMS: u16 = 0x004D;

// Lens and Focus Information
const PENTAX_LENS_TYPE: u16 = 0x003F;
const PENTAX_LENS_INFO: u16 = 0x007F;
const PENTAX_AF_INFO: u16 = 0x0080;
const PENTAX_LENS_MODEL: u16 = 0x009F;

// Advanced Features
const PENTAX_CAMERA_TEMPERATURE: u16 = 0x0047;
const PENTAX_BATTERY_LEVEL: u16 = 0x003B;
const PENTAX_PIXEL_SHIFT_RESOLUTION: u16 = 0x0086;
const PENTAX_CAMERA_INFO: u16 = 0x0215;
const PENTAX_BATTERY_INFO: u16 = 0x0216;

// Custom Settings
const PENTAX_CUSTOM_FUNCTIONS: u16 = 0x0050;
const PENTAX_AE_INFO: u16 = 0x0218;
const PENTAX_FLASH_INFO: u16 = 0x0219;

// Pentax MakerNote header signatures
// Pentax typically uses "AOC\0" (4 bytes) or no header
const PENTAX_HEADER_AOC: &[u8] = b"AOC\0";
const PENTAX_HEADER_PENTAX: &[u8] = b"PENTAX \0";

/// Checks if the provided data has a valid Pentax MakerNote header
///
/// # Arguments
/// * `data` - Raw MakerNote data to validate
///
/// # Returns
/// * `true` if data contains a valid Pentax header or appears to be Pentax MakerNote data
/// * `false` otherwise
pub fn is_pentax_makernote(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    // Check for AOC header (most common)
    if data.len() >= 4 && &data[0..4] == PENTAX_HEADER_AOC {
        return true;
    }

    // Check for PENTAX header (some models)
    if data.len() >= 8 && &data[0..8] == PENTAX_HEADER_PENTAX {
        return true;
    }

    // Some Pentax cameras have no header, just start with IFD
    // We'll validate by checking if first two bytes form a reasonable entry count
    if data.len() >= 2 {
        let entry_count = u16::from_le_bytes([data[0], data[1]]);
        // Reasonable entry count: 1-200 entries
        if entry_count > 0 && entry_count < 200 {
            return true;
        }
    }

    false
}

/// Decodes Pentax quality setting to human-readable string
fn decode_quality(value: i32) -> &'static str {
    match value {
        0 => "Good",
        1 => "Better",
        2 => "Best",
        3 => "TIFF",
        4 => "RAW",
        5 => "Premium",
        6 => "RAW + JPEG",
        7 => "RAW + Premium",
        8 => "RAW + Better",
        9 => "RAW + Good",
        _ => "Unknown",
    }
}

/// Decodes Pentax picture mode to human-readable string
fn decode_picture_mode(value: i32) -> &'static str {
    match value {
        0 => "Program",
        1 => "Shutter Priority",
        2 => "Aperture Priority",
        3 => "Manual",
        4 => "Portrait",
        5 => "Landscape",
        6 => "Macro",
        7 => "Sport",
        8 => "Night Scene Portrait",
        9 => "No Flash",
        10 => "Night Scene",
        11 => "Surf & Snow",
        12 => "Text",
        13 => "Sunset",
        14 => "Kids",
        15 => "Pet",
        16 => "Candlelight",
        17 => "Museum",
        18 => "Food",
        19 => "Stage Lighting",
        20 => "Night Snap",
        21 => "Blue Sky",
        22 => "Forest",
        _ => "Unknown",
    }
}

/// Decodes Pentax flash mode to human-readable string
fn decode_flash_mode(value: i32) -> &'static str {
    match value {
        0 => "Auto",
        1 => "Flash On",
        2 => "Flash Off",
        3 => "Red-eye Reduction",
        4 => "Auto + Red-eye",
        5 => "On + Red-eye",
        6 => "Wireless",
        7 => "Slow-sync",
        8 => "Trailing-curtain Sync",
        _ => "Unknown",
    }
}

/// Decodes Pentax focus mode to human-readable string
fn decode_focus_mode(value: i32) -> &'static str {
    match value {
        0 => "Normal (AF)",
        1 => "Macro (AF)",
        2 => "Manual",
        3 => "AF-S (Single)",
        4 => "AF-C (Continuous)",
        5 => "AF-A (Auto)",
        _ => "Unknown",
    }
}

/// Decodes Pentax metering mode to human-readable string
fn decode_metering_mode(value: i32) -> &'static str {
    match value {
        0 => "Multi-segment",
        1 => "Center-weighted Average",
        2 => "Spot",
        3 => "Average",
        4 => "Highlight-weighted",
        _ => "Unknown",
    }
}

/// Decodes Pentax white balance setting to human-readable string
fn decode_white_balance(value: i32) -> &'static str {
    match value {
        0 => "Auto",
        1 => "Daylight",
        2 => "Shade",
        3 => "Cloudy",
        4 => "Tungsten",
        5 => "Fluorescent",
        6 => "Manual",
        7 => "Daylight Fluorescent",
        8 => "Day White Fluorescent",
        9 => "White Fluorescent",
        10 => "Flash",
        11 => "Cloudy Fluorescent",
        14 => "Multi Auto",
        15 => "Color Temperature Enhancement",
        _ => "Unknown",
    }
}

/// Decodes Pentax white balance mode to human-readable string
fn decode_white_balance_mode(value: i32) -> &'static str {
    match value {
        1 => "Auto (Daylight)",
        2 => "Auto (Shade)",
        3 => "Auto (Flash)",
        4 => "Auto (Tungsten)",
        6 => "Auto (Daylight Fluorescent)",
        7 => "Auto (Day White Fluorescent)",
        8 => "Auto (White Fluorescent)",
        10 => "Auto (Flash)",
        _ => "Manual",
    }
}

/// Decodes Pentax drive mode to human-readable string
fn decode_drive_mode(value: i32) -> &'static str {
    match value {
        0 => "Single-frame",
        1 => "Continuous",
        2 => "Self-timer (12s)",
        3 => "Self-timer (2s)",
        4 => "Remote",
        5 => "Exposure Bracketing",
        6 => "Multiple Exposure",
        7 => "Remote (3s delay)",
        8 => "Continuous (Hi)",
        9 => "Continuous (Lo)",
        10 => "Continuous (Med)",
        11 => "Interval Shooting",
        12 => "Interval Composite",
        _ => "Unknown",
    }
}

/// Decodes Pentax color space to human-readable string
fn decode_color_space(value: i32) -> &'static str {
    match value {
        0 => "sRGB",
        1 => "Adobe RGB",
        _ => "Unknown",
    }
}

/// Decodes Pentax saturation setting to human-readable string
fn decode_saturation(value: i32) -> &'static str {
    match value {
        0 => "Low",
        1 => "Normal",
        2 => "High",
        3 => "Med Low",
        4 => "Med High",
        5 => "Very High",
        6 => "Very Low",
        7 => "Off (B&W)",
        _ => "Unknown",
    }
}

/// Decodes Pentax contrast setting to human-readable string
fn decode_contrast(value: i32) -> &'static str {
    match value {
        0 => "Low",
        1 => "Normal",
        2 => "High",
        3 => "Med Low",
        4 => "Med High",
        5 => "Very High",
        6 => "Very Low",
        _ => "Unknown",
    }
}

/// Decodes Pentax sharpness setting to human-readable string
fn decode_sharpness(value: i32) -> &'static str {
    match value {
        0 => "Soft",
        1 => "Normal",
        2 => "Hard",
        3 => "Med Soft",
        4 => "Med Hard",
        5 => "Very Hard",
        6 => "Very Soft",
        _ => "Unknown",
    }
}

/// Decodes Pentax shake reduction info to human-readable string
fn decode_shake_reduction(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "On",
        2 => "On (Video)",
        3 => "On (2-axis)",
        4 => "On (3-axis)",
        5 => "On (4-axis)",
        6 => "On (5-axis)",
        _ => "Unknown",
    }
}

/// Represents a Pentax MakerNote parser
pub struct PentaxParser;

impl MakerNoteParser for PentaxParser {
    fn manufacturer_name(&self) -> &'static str {
        "Pentax"
    }

    fn tag_prefix(&self) -> &'static str {
        "Pentax:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_pentax_makernote(data)
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        if data.is_empty() {
            return Ok(());
        }

        // Validate Pentax header and determine IFD offset
        let ifd_offset = if data.len() >= 4 && &data[0..4] == PENTAX_HEADER_AOC {
            // AOC header: skip 6 bytes (AOC\0 + 2-byte offset)
            6
        } else if data.len() >= 8 && &data[0..8] == PENTAX_HEADER_PENTAX {
            // PENTAX header: skip 8 bytes
            8
        } else {
            // No header, IFD starts immediately
            0
        };

        if data.len() <= ifd_offset + 2 {
            return Ok(());
        }

        let ifd_data = &data[ifd_offset..];

        // Parse IFD entry count
        let entry_count = match byte_order {
            ByteOrder::LittleEndian => {
                if ifd_data.len() < 2 {
                    return Ok(());
                }
                u16::from_le_bytes([ifd_data[0], ifd_data[1]])
            }
            ByteOrder::BigEndian => {
                if ifd_data.len() < 2 {
                    return Ok(());
                }
                u16::from_be_bytes([ifd_data[0], ifd_data[1]])
            }
        };

        // Sanity check on entry count
        if entry_count == 0 || entry_count > 200 {
            return Ok(());
        }

        // Parse IFD entries
        let entries_start = &ifd_data[2..];
        let entries = match parse_ifd_entries(entries_start, entry_count, byte_order) {
            Ok((_, entries)) => entries,
            Err(_) => return Ok(()), // Return empty on parse failure
        };

        // Extract tags from entries
        for entry in entries {
            match entry.tag_id {
                // Simple string tags
                PENTAX_VERSION => {
                    if let Some(value) = extract_string_value(&entry, data, ifd_offset) {
                        tags.insert("Pentax:Version".to_string(), value);
                    }
                }

                PENTAX_DATE => {
                    if let Some(value) = extract_string_value(&entry, data, ifd_offset) {
                        tags.insert("Pentax:Date".to_string(), value);
                    }
                }

                PENTAX_TIME => {
                    if let Some(value) = extract_string_value(&entry, data, ifd_offset) {
                        tags.insert("Pentax:Time".to_string(), value);
                    }
                }

                PENTAX_LENS_MODEL => {
                    if let Some(value) = extract_string_value(&entry, data, ifd_offset) {
                        tags.insert("Pentax:LensModel".to_string(), value);
                    }
                }

                // Quality mode
                PENTAX_QUALITY => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:Quality".to_string(),
                        decode_quality(value).to_string(),
                    );
                }

                // Picture mode
                PENTAX_PICTURE_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:PictureMode".to_string(),
                        decode_picture_mode(value).to_string(),
                    );
                }

                // Flash mode
                PENTAX_FLASH_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:FlashMode".to_string(),
                        decode_flash_mode(value).to_string(),
                    );
                }

                // Focus mode
                PENTAX_FOCUS_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:FocusMode".to_string(),
                        decode_focus_mode(value).to_string(),
                    );
                }

                // AF point selected
                PENTAX_AF_POINT_SELECTED => {
                    let value = entry.value_offset as i32;
                    if (0..=65535).contains(&value) {
                        tags.insert("Pentax:AFPointSelected".to_string(), value.to_string());
                    }
                }

                // AF point in focus
                PENTAX_AF_POINT_IN_FOCUS => {
                    let value = entry.value_offset as i32;
                    if (0..=65535).contains(&value) {
                        tags.insert("Pentax:AFPointInFocus".to_string(), value.to_string());
                    }
                }

                // ISO speed
                PENTAX_ISO_SPEED => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:ISO".to_string(), value.to_string());
                }

                // Metering mode
                PENTAX_METERING_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:MeteringMode".to_string(),
                        decode_metering_mode(value).to_string(),
                    );
                }

                // White balance
                PENTAX_WHITE_BALANCE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:WhiteBalance".to_string(),
                        decode_white_balance(value).to_string(),
                    );
                }

                // White balance mode
                PENTAX_WHITE_BALANCE_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:WhiteBalanceMode".to_string(),
                        decode_white_balance_mode(value).to_string(),
                    );
                }

                // Blue balance
                PENTAX_BLUE_BALANCE => {
                    let value = entry.value_offset as i32;
                    tags.insert("Pentax:BlueBalance".to_string(), value.to_string());
                }

                // Red balance
                PENTAX_RED_BALANCE => {
                    let value = entry.value_offset as i32;
                    tags.insert("Pentax:RedBalance".to_string(), value.to_string());
                }

                // Focal length
                PENTAX_FOCAL_LENGTH => {
                    let value = entry.value_offset;
                    tags.insert(
                        "Pentax:FocalLength".to_string(),
                        format!("{:.1} mm", value as f32 / 100.0),
                    );
                }

                // Digital zoom
                PENTAX_DIGITAL_ZOOM => {
                    let value = entry.value_offset;
                    if value > 0 {
                        tags.insert(
                            "Pentax:DigitalZoom".to_string(),
                            format!("{:.2}x", value as f32 / 100.0),
                        );
                    }
                }

                // Saturation
                PENTAX_SATURATION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:Saturation".to_string(),
                        decode_saturation(value).to_string(),
                    );
                }

                // Contrast
                PENTAX_CONTRAST => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:Contrast".to_string(),
                        decode_contrast(value).to_string(),
                    );
                }

                // Sharpness
                PENTAX_SHARPNESS => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:Sharpness".to_string(),
                        decode_sharpness(value).to_string(),
                    );
                }

                // Drive mode
                PENTAX_DRIVE_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:DriveMode".to_string(),
                        decode_drive_mode(value).to_string(),
                    );
                }

                // Color space
                PENTAX_COLOR_SPACE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:ColorSpace".to_string(),
                        decode_color_space(value).to_string(),
                    );
                }

                // Shutter count
                PENTAX_SHUTTER_COUNT => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:ShutterCount".to_string(), value.to_string());
                }

                // Lens type and lookup
                PENTAX_LENS_TYPE => {
                    let lens_id = entry.value_offset as u16;
                    if let Some(lens_name) = lookup_lens_name(lens_id) {
                        tags.insert("Pentax:LensType".to_string(), lens_name);
                    } else {
                        tags.insert(
                            "Pentax:LensType".to_string(),
                            format!("Unknown ({})", lens_id),
                        );
                    }
                }

                // Model type
                PENTAX_PENTAX_MODEL_TYPE => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:ModelType".to_string(), value.to_string());
                }

                // Model ID
                PENTAX_PENTAX_MODEL_ID => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:ModelID".to_string(), value.to_string());
                }

                // Image size
                PENTAX_PENTAX_IMAGE_SIZE => {
                    let value = entry.value_offset as i32;
                    let size_str = match value {
                        0 => "640x480",
                        1 => "Full",
                        2 => "1024x768",
                        3 => "1280x960",
                        4 => "1600x1200",
                        5 => "2048x1536",
                        8 => "2560x1920",
                        9 => "3072x2304",
                        10 => "3264x2448",
                        19 => "320x240",
                        20 => "2288x1712",
                        21 => "2592x1944",
                        22 => "2304x1728",
                        23 => "3056x2296",
                        25 => "2816x2212",
                        27 => "3648x2736",
                        36 => "3008x2008",
                        _ => "Unknown",
                    };
                    tags.insert("Pentax:ImageSize".to_string(), size_str.to_string());
                }

                // Auto bracketing
                PENTAX_AUTO_BRACKETING => {
                    let value = entry.value_offset as i32;
                    let bracket_str = match value {
                        0 => "Off",
                        1 => "On",
                        _ => "Unknown",
                    };
                    tags.insert("Pentax:AutoBracketing".to_string(), bracket_str.to_string());
                }

                // Preview image info (just extract dimensions)
                PENTAX_PREVIEW_IMAGE_SIZE => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:PreviewImageSize".to_string(), value.to_string());
                }

                PENTAX_PREVIEW_IMAGE_LENGTH => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:PreviewImageLength".to_string(), value.to_string());
                }

                // Camera temperature
                PENTAX_CAMERA_TEMPERATURE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:CameraTemperature".to_string(),
                        format!("{}°C", value),
                    );
                }

                // Shake reduction info
                PENTAX_SHAKE_REDUCTION_INFO => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Pentax:ShakeReduction".to_string(),
                        decode_shake_reduction(value).to_string(),
                    );
                }

                // Pixel shift resolution (for K-1 and newer high-res shot mode)
                PENTAX_PIXEL_SHIFT_RESOLUTION => {
                    let value = entry.value_offset as i32;
                    let psr_str = match value {
                        0 => "Off",
                        1 => "On",
                        2 => "On (Motion Correction)",
                        _ => "Unknown",
                    };
                    tags.insert(
                        "Pentax:PixelShiftResolution".to_string(),
                        psr_str.to_string(),
                    );
                }

                // Battery level
                PENTAX_BATTERY_LEVEL => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:BatteryLevel".to_string(), format!("{}%", value));
                }

                // Hometown and destination cities (for world time feature)
                PENTAX_HOMETOWN_CITY => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:HometownCity".to_string(), value.to_string());
                }

                PENTAX_DESTINATION_CITY => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:DestinationCity".to_string(), value.to_string());
                }

                // World time location
                PENTAX_WORLD_TIME_LOCATION => {
                    let value = entry.value_offset as i32;
                    let location = match value {
                        0 => "Hometown",
                        1 => "Destination",
                        _ => "Unknown",
                    };
                    tags.insert("Pentax:WorldTimeLocation".to_string(), location.to_string());
                }

                // Picture mode 2 (extended picture modes)
                PENTAX_PICTURE_MODE_2 => {
                    let value = entry.value_offset as i32;
                    tags.insert("Pentax:PictureMode2".to_string(), value.to_string());
                }

                _ => {
                    // For unknown tags, we can optionally store them for debugging
                    // Uncomment if you want to see all unknown tags:
                    // tags.insert(
                    //     format!("Pentax:Unknown-{:#06X}", entry.tag_id),
                    //     entry.value_offset.to_string(),
                    // );
                }
            }
        }

        Ok(())
    }
}

/// Maps Pentax tag ID to human-readable tag name
fn pentax_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        PENTAX_VERSION => "Version",
        PENTAX_PENTAX_MODEL_TYPE => "ModelType",
        PENTAX_PENTAX_MODEL_ID => "ModelID",
        PENTAX_DATE => "Date",
        PENTAX_TIME => "Time",
        PENTAX_QUALITY => "Quality",
        PENTAX_PENTAX_IMAGE_SIZE => "ImageSize",
        PENTAX_PICTURE_MODE => "PictureMode",
        PENTAX_FLASH_MODE => "FlashMode",
        PENTAX_FOCUS_MODE => "FocusMode",
        PENTAX_AF_POINT_SELECTED => "AFPointSelected",
        PENTAX_AF_POINT_IN_FOCUS => "AFPointInFocus",
        PENTAX_ISO_SPEED => "ISO",
        PENTAX_METERING_MODE => "MeteringMode",
        PENTAX_WHITE_BALANCE => "WhiteBalance",
        PENTAX_WHITE_BALANCE_MODE => "WhiteBalanceMode",
        PENTAX_SATURATION => "Saturation",
        PENTAX_CONTRAST => "Contrast",
        PENTAX_SHARPNESS => "Sharpness",
        PENTAX_DRIVE_MODE => "DriveMode",
        PENTAX_COLOR_SPACE => "ColorSpace",
        PENTAX_LENS_TYPE => "LensType",
        PENTAX_LENS_MODEL => "LensModel",
        PENTAX_SHUTTER_COUNT => "ShutterCount",
        _ => return format!("Pentax:Unknown-{:#06X}", tag_id),
    };

    format!("Pentax:{}", tag_name)
}

/// Parses IFD entries in the specified byte order
fn parse_ifd_entries(
    input: &[u8],
    entry_count: u16,
    byte_order: ByteOrder,
) -> IResult<&[u8], Vec<IfdEntry>> {
    match byte_order {
        ByteOrder::LittleEndian => count(parse_ifd_entry_le, entry_count as usize)(input),
        ByteOrder::BigEndian => count(parse_ifd_entry_be, entry_count as usize)(input),
    }
}

/// Parses a single IFD entry in little-endian byte order
fn parse_ifd_entry_le(input: &[u8]) -> IResult<&[u8], IfdEntry> {
    map(
        |input| {
            let (input, tag_id) = le_u16(input)?;
            let (input, field_type) = le_u16(input)?;
            let (input, value_count) = le_u32(input)?;
            let (input, value_offset) = le_u32(input)?;
            Ok((input, (tag_id, field_type, value_count, value_offset)))
        },
        |(tag_id, field_type, value_count, value_offset)| IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        },
    )(input)
}

/// Parses a single IFD entry in big-endian byte order
fn parse_ifd_entry_be(input: &[u8]) -> IResult<&[u8], IfdEntry> {
    map(
        |input| {
            let (input, tag_id) = be_u16(input)?;
            let (input, field_type) = be_u16(input)?;
            let (input, value_count) = be_u32(input)?;
            let (input, value_offset) = be_u32(input)?;
            Ok((input, (tag_id, field_type, value_count, value_offset)))
        },
        |(tag_id, field_type, value_count, value_offset)| IfdEntry {
            tag_id,
            field_type,
            value_count,
            value_offset,
        },
    )(input)
}

/// Extracts string value from IFD entry
///
/// Handles both inline strings (≤4 bytes) and offset-based strings
fn extract_string_value(entry: &IfdEntry, full_data: &[u8], ifd_offset: usize) -> Option<String> {
    let byte_count = entry.value_count as usize;

    // For inline strings (≤4 bytes), value is in value_offset field
    if byte_count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        let s = std::str::from_utf8(&bytes[0..byte_count])
            .ok()?
            .trim_end_matches('\0')
            .trim();
        return Some(s.to_string());
    }

    // For longer strings, read from offset
    let offset = entry.value_offset as usize;
    let abs_offset = ifd_offset + offset;

    if abs_offset + byte_count <= full_data.len() {
        let bytes = &full_data[abs_offset..abs_offset + byte_count];
        let s = std::str::from_utf8(bytes)
            .ok()?
            .trim_end_matches('\0')
            .trim();
        return Some(s.to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_quality() {
        assert_eq!(decode_quality(2), "Best");
        assert_eq!(decode_quality(4), "RAW");
        assert_eq!(decode_quality(6), "RAW + JPEG");
    }

    #[test]
    fn test_decode_picture_mode() {
        assert_eq!(decode_picture_mode(0), "Program");
        assert_eq!(decode_picture_mode(2), "Aperture Priority");
        assert_eq!(decode_picture_mode(3), "Manual");
        assert_eq!(decode_picture_mode(5), "Landscape");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(decode_focus_mode(2), "Manual");
        assert_eq!(decode_focus_mode(3), "AF-S (Single)");
        assert_eq!(decode_focus_mode(4), "AF-C (Continuous)");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(decode_white_balance(0), "Auto");
        assert_eq!(decode_white_balance(1), "Daylight");
        assert_eq!(decode_white_balance(6), "Manual");
    }

    #[test]
    fn test_decode_drive_mode() {
        assert_eq!(decode_drive_mode(0), "Single-frame");
        assert_eq!(decode_drive_mode(1), "Continuous");
        assert_eq!(decode_drive_mode(5), "Exposure Bracketing");
    }

    #[test]
    fn test_decode_saturation() {
        assert_eq!(decode_saturation(0), "Low");
        assert_eq!(decode_saturation(1), "Normal");
        assert_eq!(decode_saturation(2), "High");
    }

    #[test]
    fn test_decode_contrast() {
        assert_eq!(decode_contrast(0), "Low");
        assert_eq!(decode_contrast(1), "Normal");
        assert_eq!(decode_contrast(2), "High");
    }

    #[test]
    fn test_decode_sharpness() {
        assert_eq!(decode_sharpness(0), "Soft");
        assert_eq!(decode_sharpness(1), "Normal");
        assert_eq!(decode_sharpness(2), "Hard");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = PentaxParser;
        assert_eq!(parser.manufacturer_name(), "Pentax");
        assert_eq!(parser.tag_prefix(), "Pentax:");
    }

    #[test]
    fn test_validate_header_aoc() {
        let parser = PentaxParser;

        let valid_header = b"AOC\0extra_data_here";
        assert!(parser.validate_header(valid_header));

        let invalid_header = b"Canon\0\0\0";
        assert!(!parser.validate_header(invalid_header));
    }

    #[test]
    fn test_validate_header_pentax() {
        let parser = PentaxParser;

        let valid_header = b"PENTAX \0more_data";
        assert!(parser.validate_header(valid_header));
    }

    #[test]
    fn test_lens_lookup() {
        let parser = PentaxParser;

        // Test classic lens
        assert!(parser.lookup_lens(2).is_some());
        assert_eq!(
            parser.lookup_lens(2),
            Some("SMC Pentax-K 50mm f/1.4".to_string())
        );

        // Test Limited lens
        assert!(parser.lookup_lens(56).is_some());
        assert_eq!(
            parser.lookup_lens(56),
            Some("SMC Pentax-FA 77mm f/1.8 Limited".to_string())
        );

        // Test modern D FA lens
        assert!(parser.lookup_lens(122).is_some());
        assert_eq!(
            parser.lookup_lens(122),
            Some("HD Pentax-D FA 24-70mm f/2.8 ED SDM WR".to_string())
        );

        // Test unknown lens
        assert_eq!(parser.lookup_lens(65000), None);
    }

    #[test]
    fn test_pentax_tag_to_name() {
        assert_eq!(pentax_tag_to_name(PENTAX_VERSION), "Pentax:Version");
        assert_eq!(pentax_tag_to_name(PENTAX_LENS_TYPE), "Pentax:LensType");
        assert_eq!(pentax_tag_to_name(PENTAX_QUALITY), "Pentax:Quality");
    }

    #[test]
    fn test_is_pentax_makernote() {
        let valid_data_aoc = b"AOC\0some_data";
        assert!(is_pentax_makernote(valid_data_aoc));

        let valid_data_pentax = b"PENTAX \0data";
        assert!(is_pentax_makernote(valid_data_pentax));

        let invalid_data = b"Nikon\0\0\0";
        assert!(!is_pentax_makernote(invalid_data));
    }
}
