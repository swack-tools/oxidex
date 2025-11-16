//! Leica MakerNote Parser
//!
//! Parses Leica-specific EXIF MakerNote tags containing camera settings,
//! lens information, image quality parameters, and other proprietary metadata.
//!
//! Supports Leica digital cameras including:
//! - M-series digital rangefinders (M8, M9, M10, M11, M Monochrom)
//! - SL-series mirrorless (SL, SL2, SL2-S)
//! - Q-series fixed-lens compacts (Q, Q2, Q2 Monochrom)
//! - CL/TL mirrorless cameras
//!
//! Based on ExifTool's Leica.pm module.

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

use super::leica_lens_database::lookup_lens_name;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use super::shared::MakerNoteParser;

// ===== Leica MakerNote Tag IDs =====
// Based on ExifTool Leica.pm tag definitions

// Basic Camera Information Tags
const LEICA_QUALITY: u16 = 0x0003;
const LEICA_USER_PROFILE: u16 = 0x0004;
const LEICA_SERIAL_NUMBER: u16 = 0x0005;
const LEICA_WHITE_BALANCE: u16 = 0x0006;
const LEICA_EXTERNAL_SENSOR_BRIGHTNESS_VALUE: u16 = 0x0008;
const LEICA_MEASURED_LV: u16 = 0x0009;
const LEICA_APPROXIMATE_F_NUMBER: u16 = 0x000A;
const LEICA_CAMERA_TEMPERATURE: u16 = 0x000B;
const LEICA_COLOR_TEMPERATURE: u16 = 0x000C;
const LEICA_WB_RED_LEVEL: u16 = 0x000D;
const LEICA_WB_GREEN_LEVEL: u16 = 0x000E;
const LEICA_WB_BLUE_LEVEL: u16 = 0x000F;

// Image Processing
const LEICA_SHARPENING: u16 = 0x0010;
const LEICA_CONTRAST: u16 = 0x0011;
const LEICA_SATURATION: u16 = 0x0012;
const LEICA_LENS_ID: u16 = 0x0013;
const LEICA_LENS_TYPE: u16 = 0x0014;
const LEICA_LENS_MODEL: u16 = 0x0015;

// Camera Settings
const LEICA_ORIGINAL_FILE_NAME: u16 = 0x001D;
const LEICA_ORIGINAL_DIRECTORY: u16 = 0x001E;
const LEICA_EXPOSURE_MODE: u16 = 0x0020;
const LEICA_METERING_MODE: u16 = 0x0021;
const LEICA_FILM_MODE: u16 = 0x0022;
const LEICA_WB_MODE: u16 = 0x0023;
const LEICA_APEX_BRIGHTNESS: u16 = 0x0024;
const LEICA_FLASH_MODE: u16 = 0x0025;
const LEICA_FLASH_ENERGY: u16 = 0x0026;
const LEICA_INTERNAL_SERIAL_NUMBER: u16 = 0x0027;

// Lens Information
const LEICA_FOCAL_LENGTH_35MM: u16 = 0x0030;
const LEICA_LENS_SERIAL_NUMBER: u16 = 0x0031;
const LEICA_CONTRAST_DETECT_AF: u16 = 0x0032;
const LEICA_SHUTTER_COUNT: u16 = 0x0034;
const LEICA_FOCUS_DISTANCE: u16 = 0x0035;

// M-Series Specific Tags
const LEICA_FRAME_SELECTOR: u16 = 0x0040;
const LEICA_BASE_ISO: u16 = 0x0041;
const LEICA_IMAGE_ID: u16 = 0x0042;
const LEICA_USER_COMMENT: u16 = 0x0043;

// SL-Series Specific Tags
const LEICA_PICTURE_CONTROL: u16 = 0x0050;
const LEICA_AF_POINT: u16 = 0x0051;
const LEICA_AF_MODE: u16 = 0x0052;
const LEICA_IMAGE_STABILIZATION: u16 = 0x0053;
const LEICA_DIGITAL_ZOOM: u16 = 0x0054;

// Advanced Features
const LEICA_DNG_VERSION: u16 = 0x0060;
const LEICA_CROP_MODE: u16 = 0x0061;
const LEICA_PERSPECTIVE_CONTROL: u16 = 0x0062;
const LEICA_CAMERA_PITCH_ANGLE: u16 = 0x0063;
const LEICA_CAMERA_ROLL_ANGLE: u16 = 0x0064;

// Q-Series Specific
const LEICA_MACRO_MODE: u16 = 0x0070;
const LEICA_SCENE_MODE: u16 = 0x0071;

// Leica MakerNote header signature
// Leica typically uses "LEICA\0\0\0" or "LEICA CAMERA AG" headers
const LEICA_HEADER_SHORT: &[u8] = b"LEICA\0\0\0";
const LEICA_HEADER_LONG: &[u8] = b"LEICA CAMERA AG";

/// Checks if the provided data has a valid Leica MakerNote header
///
/// # Arguments
/// * `data` - Raw MakerNote data to validate
///
/// # Returns
/// * `true` if data contains a valid Leica header
/// * `false` otherwise
pub fn is_leica_makernote(data: &[u8]) -> bool {
    if data.len() < 8 {
        return false;
    }

    // Check for short LEICA header (8 bytes)
    if data.len() >= 8 && &data[0..8] == LEICA_HEADER_SHORT {
        return true;
    }

    // Check for long LEICA CAMERA AG header (15 bytes)
    if data.len() >= 15 && &data[0..15] == LEICA_HEADER_LONG {
        return true;
    }

    // Some Leica cameras may have minimal or no header
    // Check if first two bytes could be a valid IFD entry count
    if data.len() >= 2 {
        let entry_count = u16::from_le_bytes([data[0], data[1]]);
        // Reasonable entry count: 1-150 entries
        if entry_count > 0 && entry_count < 150 {
            return true;
        }
    }

    false
}

/// Decodes Leica quality setting to human-readable string
fn decode_quality(value: i32) -> &'static str {
    match value {
        1 => "Fine",
        2 => "Basic",
        3 => "Standard",
        4 => "Super Fine",
        5 => "DNG",
        6 => "DNG + JPEG Fine",
        7 => "DNG + JPEG Standard",
        _ => "Unknown",
    }
}

/// Decodes Leica white balance mode to human-readable string
fn decode_white_balance(value: i32) -> &'static str {
    match value {
        0 => "Auto",
        1 => "Daylight",
        2 => "Fluorescent",
        3 => "Tungsten",
        4 => "Flash",
        5 => "Cloudy",
        6 => "Shade",
        7 => "Manual",
        8 => "Kelvin",
        9 => "Auto (ambient priority)",
        10 => "Auto (white priority)",
        _ => "Unknown",
    }
}

/// Decodes Leica exposure mode to human-readable string
fn decode_exposure_mode(value: i32) -> &'static str {
    match value {
        0 => "Manual",
        1 => "Program AE",
        2 => "Aperture Priority",
        3 => "Shutter Priority",
        4 => "Auto",
        _ => "Unknown",
    }
}

/// Decodes Leica metering mode to human-readable string
fn decode_metering_mode(value: i32) -> &'static str {
    match value {
        0 => "Unknown",
        1 => "Multi-segment",
        2 => "Center-weighted",
        3 => "Spot",
        4 => "Multi-spot",
        _ => "Unknown",
    }
}

/// Decodes Leica flash mode to human-readable string
fn decode_flash_mode(value: i32) -> &'static str {
    match value {
        0 => "No Flash",
        1 => "Auto",
        2 => "On",
        3 => "Red-eye Reduction",
        4 => "Slow Sync",
        5 => "Rear Curtain Sync",
        6 => "Fill Flash",
        _ => "Unknown",
    }
}

/// Decodes Leica AF mode to human-readable string
fn decode_af_mode(value: i32) -> &'static str {
    match value {
        0 => "Manual",
        1 => "Single AF",
        2 => "Continuous AF",
        3 => "AF-C",
        4 => "Face Detection",
        5 => "Tracking",
        _ => "Unknown",
    }
}

/// Decodes Leica image stabilization to human-readable string
fn decode_image_stabilization(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "On",
        2 => "On (Body)",
        3 => "On (Lens)",
        4 => "On (Dual)",
        _ => "Unknown",
    }
}

/// Decodes Leica user profile to human-readable string
fn decode_user_profile(value: i32) -> &'static str {
    match value {
        0 => "Not Set",
        1 => "User Profile 1",
        2 => "User Profile 2",
        3 => "User Profile 3",
        4 => "User Profile 4",
        5 => "Standard",
        6 => "Vivid",
        7 => "Natural",
        8 => "Monochrome",
        9 => "High Contrast",
        10 => "Monochrome High Contrast",
        _ => "Unknown",
    }
}

/// Decodes Leica scene mode to human-readable string
fn decode_scene_mode(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "Portrait",
        2 => "Landscape",
        3 => "Macro",
        4 => "Sport",
        5 => "Night Portrait",
        6 => "Sunset",
        7 => "Beach",
        8 => "Snow",
        9 => "Fireworks",
        _ => "Unknown",
    }
}

/// Decodes Leica crop mode to human-readable string
fn decode_crop_mode(value: i32) -> &'static str {
    match value {
        0 => "Full Frame",
        1 => "APS-C",
        2 => "1:1",
        3 => "16:9",
        4 => "4:3",
        _ => "Unknown",
    }
}

/// Leica MakerNote Parser
///
/// Implements the MakerNoteParser trait for Leica cameras.
pub struct LeicaMakerNoteParser;

impl MakerNoteParser for LeicaMakerNoteParser {
    fn manufacturer_name(&self) -> &'static str {
        "Leica"
    }

    fn tag_prefix(&self) -> &'static str {
        "Leica:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_leica_makernote(data)
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
        // Validate minimum data length
        if data.len() < 8 {
            return Err("Leica MakerNote data too short".to_string());
        }

        // Skip header if present
        let offset = if data.len() >= 8 && &data[0..8] == LEICA_HEADER_SHORT {
            8 // Skip "LEICA\0\0\0"
        } else if data.len() >= 15 && &data[0..15] == LEICA_HEADER_LONG {
            15 // Skip "LEICA CAMERA AG"
        } else {
            0 // No header, start directly with IFD
        };

        // Ensure we have enough data after the header
        if offset >= data.len() {
            return Err("No data after Leica header".to_string());
        }

        let ifd_data = &data[offset..];

        // Parse IFD entry count
        if ifd_data.len() < 2 {
            return Err("Insufficient data for IFD entry count".to_string());
        }

        let entry_count = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([ifd_data[0], ifd_data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([ifd_data[0], ifd_data[1]]),
        };

        // Validate entry count is reasonable
        if entry_count == 0 || entry_count > 200 {
            return Err(format!("Invalid Leica IFD entry count: {}", entry_count));
        }

        // Each IFD entry is 12 bytes: 2 (tag) + 2 (type) + 4 (count) + 4 (value/offset)
        let required_size = 2 + (entry_count as usize * 12);
        if ifd_data.len() < required_size {
            return Err(format!(
                "Insufficient data for {} IFD entries (need {}, have {})",
                entry_count,
                required_size,
                ifd_data.len()
            ));
        }

        // Parse each IFD entry
        for i in 0..entry_count {
            let entry_offset = 2 + (i as usize * 12);
            let entry_data = &ifd_data[entry_offset..entry_offset + 12];

            // Parse IFD entry fields
            let tag_id = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[0], entry_data[1]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[0], entry_data[1]]),
            };

            let format = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[2], entry_data[3]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[2], entry_data[3]]),
            };

            let component_count = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
            };

            let value_offset = match byte_order {
                ByteOrder::LittleEndian => u32::from_le_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
                ByteOrder::BigEndian => u32::from_be_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
            };

            // Create IfdEntry for this tag
            let entry = IfdEntry {
                tag_id,
                field_type: format,
                value_count: component_count,
                value_offset,
            };

            // Extract and decode tag values based on tag ID
            match tag_id {
                // Quality setting
                LEICA_QUALITY => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:Quality".to_string(),
                        decode_quality(value).to_string(),
                    );
                }

                // User profile / picture style
                LEICA_USER_PROFILE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:UserProfile".to_string(),
                        decode_user_profile(value).to_string(),
                    );
                }

                // Serial number (if stored as value, not offset)
                LEICA_SERIAL_NUMBER => {
                    if entry.value_count <= 4 {
                        tags.insert(
                            "Leica:SerialNumber".to_string(),
                            entry.value_offset.to_string(),
                        );
                    }
                }

                // White balance mode
                LEICA_WHITE_BALANCE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:WhiteBalance".to_string(),
                        decode_white_balance(value).to_string(),
                    );
                }

                // WB mode (alternative tag)
                LEICA_WB_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:WBMode".to_string(),
                        decode_white_balance(value).to_string(),
                    );
                }

                // Color temperature in Kelvin
                LEICA_COLOR_TEMPERATURE => {
                    let value = entry.value_offset;
                    tags.insert("Leica:ColorTemperature".to_string(), format!("{}K", value));
                }

                // WB RGB levels
                LEICA_WB_RED_LEVEL => {
                    let value = entry.value_offset;
                    tags.insert("Leica:WBRedLevel".to_string(), value.to_string());
                }

                LEICA_WB_GREEN_LEVEL => {
                    let value = entry.value_offset;
                    tags.insert("Leica:WBGreenLevel".to_string(), value.to_string());
                }

                LEICA_WB_BLUE_LEVEL => {
                    let value = entry.value_offset;
                    tags.insert("Leica:WBBlueLevel".to_string(), value.to_string());
                }

                // Camera temperature
                LEICA_CAMERA_TEMPERATURE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:CameraTemperature".to_string(),
                        format!("{}°C", value),
                    );
                }

                // Image processing parameters
                LEICA_SHARPENING => {
                    let value = entry.value_offset as i32;
                    tags.insert("Leica:Sharpening".to_string(), value.to_string());
                }

                LEICA_CONTRAST => {
                    let value = entry.value_offset as i32;
                    tags.insert("Leica:Contrast".to_string(), value.to_string());
                }

                LEICA_SATURATION => {
                    let value = entry.value_offset as i32;
                    tags.insert("Leica:Saturation".to_string(), value.to_string());
                }

                // Lens information
                LEICA_LENS_ID => {
                    let lens_id = entry.value_offset as u16;
                    tags.insert("Leica:LensID".to_string(), lens_id.to_string());

                    // Look up lens name from database
                    if let Some(lens_name) = lookup_lens_name(lens_id) {
                        tags.insert("Leica:LensModel".to_string(), lens_name);
                    }
                }

                LEICA_LENS_TYPE => {
                    let value = entry.value_offset;
                    tags.insert("Leica:LensType".to_string(), value.to_string());
                }

                // Exposure mode
                LEICA_EXPOSURE_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:ExposureMode".to_string(),
                        decode_exposure_mode(value).to_string(),
                    );
                }

                // Metering mode
                LEICA_METERING_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:MeteringMode".to_string(),
                        decode_metering_mode(value).to_string(),
                    );
                }

                // Flash mode
                LEICA_FLASH_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:FlashMode".to_string(),
                        decode_flash_mode(value).to_string(),
                    );
                }

                // Flash energy
                LEICA_FLASH_ENERGY => {
                    let value = entry.value_offset;
                    tags.insert("Leica:FlashEnergy".to_string(), value.to_string());
                }

                // Internal serial number
                LEICA_INTERNAL_SERIAL_NUMBER => {
                    if entry.value_count <= 4 {
                        tags.insert(
                            "Leica:InternalSerialNumber".to_string(),
                            entry.value_offset.to_string(),
                        );
                    }
                }

                // Shutter count
                LEICA_SHUTTER_COUNT => {
                    let value = entry.value_offset;
                    tags.insert("Leica:ShutterCount".to_string(), value.to_string());
                }

                // Focus distance
                LEICA_FOCUS_DISTANCE => {
                    let value = entry.value_offset;
                    tags.insert("Leica:FocusDistance".to_string(), format!("{} mm", value));
                }

                // AF mode
                LEICA_AF_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:AFMode".to_string(),
                        decode_af_mode(value).to_string(),
                    );
                }

                // Image stabilization
                LEICA_IMAGE_STABILIZATION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:ImageStabilization".to_string(),
                        decode_image_stabilization(value).to_string(),
                    );
                }

                // Digital zoom
                LEICA_DIGITAL_ZOOM => {
                    let value = entry.value_offset;
                    if value > 100 {
                        tags.insert("Leica:DigitalZoom".to_string(), format!("{}%", value / 100));
                    } else if value > 0 {
                        tags.insert(
                            "Leica:DigitalZoom".to_string(),
                            format!("{}.{}x", value / 10, value % 10),
                        );
                    }
                }

                // Macro mode (Q-series)
                LEICA_MACRO_MODE => {
                    let value = entry.value_offset as i32;
                    let macro_str = match value {
                        0 => "Off",
                        1 => "On",
                        _ => "Unknown",
                    };
                    tags.insert("Leica:MacroMode".to_string(), macro_str.to_string());
                }

                // Scene mode
                LEICA_SCENE_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:SceneMode".to_string(),
                        decode_scene_mode(value).to_string(),
                    );
                }

                // Crop mode
                LEICA_CROP_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:CropMode".to_string(),
                        decode_crop_mode(value).to_string(),
                    );
                }

                // Base ISO
                LEICA_BASE_ISO => {
                    let value = entry.value_offset;
                    tags.insert("Leica:BaseISO".to_string(), value.to_string());
                }

                // Measured light value (EV)
                LEICA_MEASURED_LV => {
                    let value = entry.value_offset as f32 / 10.0;
                    tags.insert("Leica:MeasuredLV".to_string(), format!("{:.1} EV", value));
                }

                // Approximate F-number
                LEICA_APPROXIMATE_F_NUMBER => {
                    let value = entry.value_offset as f32 / 10.0;
                    tags.insert(
                        "Leica:ApproximateFNumber".to_string(),
                        format!("f/{:.1}", value),
                    );
                }

                // Film mode / simulation
                LEICA_FILM_MODE => {
                    let value = entry.value_offset;
                    tags.insert("Leica:FilmMode".to_string(), value.to_string());
                }

                // Frame selector (M-series)
                LEICA_FRAME_SELECTOR => {
                    let value = entry.value_offset;
                    tags.insert("Leica:FrameSelector".to_string(), value.to_string());
                }

                // Camera pitch/roll angles (SL-series leveling)
                LEICA_CAMERA_PITCH_ANGLE => {
                    let value = entry.value_offset as i32;
                    tags.insert("Leica:CameraPitchAngle".to_string(), format!("{}°", value));
                }

                LEICA_CAMERA_ROLL_ANGLE => {
                    let value = entry.value_offset as i32;
                    tags.insert("Leica:CameraRollAngle".to_string(), format!("{}°", value));
                }

                _ => {
                    // Unknown tags - optionally store for debugging
                    // Uncomment to see all unknown tags:
                    // tags.insert(
                    //     format!("Leica:Unknown-{:#06X}", entry.tag_id),
                    //     entry.value_offset.to_string(),
                    // );
                }
            }
        }

        Ok(())
    }
}

/// Maps Leica tag ID to human-readable tag name
fn leica_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        LEICA_QUALITY => "Quality",
        LEICA_USER_PROFILE => "UserProfile",
        LEICA_SERIAL_NUMBER => "SerialNumber",
        LEICA_WHITE_BALANCE => "WhiteBalance",
        LEICA_COLOR_TEMPERATURE => "ColorTemperature",
        LEICA_WB_RED_LEVEL => "WBRedLevel",
        LEICA_WB_GREEN_LEVEL => "WBGreenLevel",
        LEICA_WB_BLUE_LEVEL => "WBBlueLevel",
        LEICA_CAMERA_TEMPERATURE => "CameraTemperature",
        LEICA_SHARPENING => "Sharpening",
        LEICA_CONTRAST => "Contrast",
        LEICA_SATURATION => "Saturation",
        LEICA_LENS_ID => "LensID",
        LEICA_LENS_TYPE => "LensType",
        LEICA_LENS_MODEL => "LensModel",
        LEICA_EXPOSURE_MODE => "ExposureMode",
        LEICA_METERING_MODE => "MeteringMode",
        LEICA_FLASH_MODE => "FlashMode",
        LEICA_FLASH_ENERGY => "FlashEnergy",
        LEICA_SHUTTER_COUNT => "ShutterCount",
        LEICA_FOCUS_DISTANCE => "FocusDistance",
        LEICA_AF_MODE => "AFMode",
        LEICA_IMAGE_STABILIZATION => "ImageStabilization",
        LEICA_DIGITAL_ZOOM => "DigitalZoom",
        LEICA_MACRO_MODE => "MacroMode",
        LEICA_SCENE_MODE => "SceneMode",
        LEICA_CROP_MODE => "CropMode",
        LEICA_BASE_ISO => "BaseISO",
        LEICA_MEASURED_LV => "MeasuredLV",
        LEICA_APPROXIMATE_F_NUMBER => "ApproximateFNumber",
        LEICA_FILM_MODE => "FilmMode",
        LEICA_FRAME_SELECTOR => "FrameSelector",
        LEICA_CAMERA_PITCH_ANGLE => "CameraPitchAngle",
        LEICA_CAMERA_ROLL_ANGLE => "CameraRollAngle",
        LEICA_INTERNAL_SERIAL_NUMBER => "InternalSerialNumber",
        LEICA_WB_MODE => "WBMode",
        LEICA_LENS_SERIAL_NUMBER => "LensSerialNumber",
        LEICA_ORIGINAL_FILE_NAME => "OriginalFileName",
        LEICA_ORIGINAL_DIRECTORY => "OriginalDirectory",
        _ => return format!("Unknown-{:#06X}", tag_id),
    };
    tag_name.to_string()
}
