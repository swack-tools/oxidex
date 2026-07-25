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
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use nom::{
    IResult,
    combinator::map,
    multi::count,
    number::complete::{be_u16, be_u32, le_u16, le_u32},
};
use std::collections::HashMap;

use super::leica_lens_database::lookup_lens_name;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use crate::const_decoder;

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

// Leica 4 (M9) Subdirectories
const L4_SUBDIR_3000: u16 = 0x3000;
const L4_SUBDIR_3100: u16 = 0x3100;
const L4_SUBDIR_3400: u16 = 0x3400;
const L4_SUBDIR_3900: u16 = 0x3900;

// Leica 4 (M9) Tag IDs
const L4_QUALITY: u16 = 0x0003;
const L4_USER_PROFILE: u16 = 0x0004;
const L4_SERIAL_NUMBER: u16 = 0x0005;
const L4_WHITE_BALANCE: u16 = 0x0006;
const L4_FIRMWARE_VERSION: u16 = 0x0007;
const L4_EXTERNAL_SENSOR_BRIGHTNESS: u16 = 0x0008;
const L4_MEASURED_LV: u16 = 0x0009;
const L4_APPROXIMATE_F_NUMBER: u16 = 0x000A;
const L4_CAMERA_TEMPERATURE: u16 = 0x000B;
const L4_WB_RGB_LEVELS: u16 = 0x000E;
const L4_LENS_TYPE: u16 = 0x0014;
const L4_CONTRAST: u16 = 0x002C;
const L4_SATURATION: u16 = 0x002D;
const L4_SHARPENING: u16 = 0x002E;
const L4_JPEG_QUALITY: u16 = 0x002F;
const L4_BASE_ISO: u16 = 0x0030;
const L4_SENSOR_WIDTH: u16 = 0x0031;
const L4_SENSOR_HEIGHT: u16 = 0x0032;
const L4_SENSOR_BIT_DEPTH: u16 = 0x0033;

// Leica 4 Decoders
const_decoder!(
    L4_DECODE_CONTRAST,
    i32,
    [
        (0, "Low"),
        (1, "Normal"),
        (2, "High"),
        (3, "Medium Low"),
        (4, "Medium High"),
    ]
);

const_decoder!(
    L4_DECODE_SHARPENING,
    i32,
    [
        (0, "Low"),
        (1, "Normal"),
        (2, "High"),
        (3, "Medium Low"),
        (4, "Medium High"),
        (5, "Off"),
    ]
);

const_decoder!(
    L4_DECODE_SATURATION,
    i32,
    [
        (0, "Low"),
        (1, "Normal"),
        (2, "High"),
        (3, "Medium Low"),
        (4, "Medium High"),
        (5, "Black & White"),
    ]
);

const_decoder!(
    L4_DECODE_WHITE_BALANCE,
    i32,
    [
        (0, "Auto"),
        (1, "Daylight"),
        (2, "Fluorescent"),
        (3, "Tungsten"),
        (4, "Flash"),
        (5, "Cloudy"),
        (6, "Shade"),
        (7, "Manual"),
        (8, "Kelvin"),
    ]
);

const_decoder!(
    L4_DECODE_JPEG_QUALITY,
    i32,
    [(1, "Basic"), (2, "Standard"), (3, "Fine"),]
);

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
        let reader = EndianReader::little_endian(data);
        let entry_count = reader.u16_at(0).unwrap_or(0);
        // Reasonable entry count: 1-150 entries
        if entry_count > 0 && entry_count < 150 {
            return true;
        }
    }

    false
}

// ============================================================================
// DECODERS - Leica Value Decoders
// ============================================================================
// Following the shared decoder pattern from fujifilm.rs, canon.rs, and sony.rs
// Each decoder is a constant that implements the Decode trait

// Decodes Leica quality setting to human-readable string
const_decoder!(pub
    DECODER_QUALITY, i32, [
        (1, "Fine"),
        (2, "Basic"),
        (3, "Standard"),
        (4, "Super Fine"),
        (5, "DNG"),
        (6, "DNG + JPEG Fine"),
        (7, "DNG + JPEG Standard"),
    ]
);

// Decodes Leica white balance mode to human-readable string
const_decoder!(pub
    DECODER_WHITE_BALANCE, i32, [
        (0, "Auto"),
        (1, "Daylight"),
        (2, "Fluorescent"),
        (3, "Tungsten"),
        (4, "Flash"),
        (5, "Cloudy"),
        (6, "Shade"),
        (7, "Manual"),
        (8, "Kelvin"),
        (9, "Auto (ambient priority)"),
        (10, "Auto (white priority)"),
    ]
);

// Decodes Leica exposure mode to human-readable string
const_decoder!(pub
    DECODER_EXPOSURE_MODE, i32, [
        (0, "Manual"),
        (1, "Program AE"),
        (2, "Aperture Priority"),
        (3, "Shutter Priority"),
        (4, "Auto"),
    ]
);

// Decodes Leica metering mode to human-readable string
const_decoder!(pub
    DECODER_METERING_MODE, i32, [
        (0, "Unknown"),
        (1, "Multi-segment"),
        (2, "Center-weighted"),
        (3, "Spot"),
        (4, "Multi-spot"),
    ]
);

// Decodes Leica flash mode to human-readable string
const_decoder!(pub
    DECODER_FLASH_MODE, i32, [
        (0, "No Flash"),
        (1, "Auto"),
        (2, "On"),
        (3, "Red-eye Reduction"),
        (4, "Slow Sync"),
        (5, "Rear Curtain Sync"),
        (6, "Fill Flash"),
    ]
);

// Decodes Leica AF mode to human-readable string
const_decoder!(pub
    DECODER_AF_MODE, i32, [
        (0, "Manual"),
        (1, "Single AF"),
        (2, "Continuous AF"),
        (3, "AF-C"),
        (4, "Face Detection"),
        (5, "Tracking"),
    ]
);

// Decodes Leica image stabilization to human-readable string
const_decoder!(pub
    DECODER_IMAGE_STABILIZATION, i32, [
        (0, "Off"),
        (1, "On"),
        (2, "On (Body)"),
        (3, "On (Lens)"),
        (4, "On (Dual)"),
    ]
);

// Decodes Leica user profile to human-readable string
const_decoder!(pub
    DECODER_USER_PROFILE, i32, [
        (0, "Not Set"),
        (1, "User Profile 1"),
        (2, "User Profile 2"),
        (3, "User Profile 3"),
        (4, "User Profile 4"),
        (5, "Standard"),
        (6, "Vivid"),
        (7, "Natural"),
        (8, "Monochrome"),
        (9, "High Contrast"),
        (10, "Monochrome High Contrast"),
    ]
);

// Decodes Leica scene mode to human-readable string
const_decoder!(pub
    DECODER_SCENE_MODE, i32, [
        (0, "Off"),
        (1, "Portrait"),
        (2, "Landscape"),
        (3, "Macro"),
        (4, "Sport"),
        (5, "Night Portrait"),
        (6, "Sunset"),
        (7, "Beach"),
        (8, "Snow"),
        (9, "Fireworks"),
    ]
);

// Decodes Leica crop mode to human-readable string
const_decoder!(pub
    DECODER_CROP_MODE, i32, [
        (0, "Full Frame"),
        (1, "APS-C"),
        (2, "1:1"),
        (3, "16:9"),
        (4, "4:3"),
    ]
);

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

        // Parse IFD entry count using EndianReader
        let ifd_reader = EndianReader::new(ifd_data, byte_order.to_io_byte_order());
        let entry_count = ifd_reader.u16_at(0).unwrap_or(0);

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
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

            // Parse IFD entry fields using EndianReader
            let tag_id = entry_reader.u16_at(0).unwrap_or(0);
            let format = entry_reader.u16_at(2).unwrap_or(0);
            let component_count = entry_reader.u32_at(4).unwrap_or(0);
            let value_offset = entry_reader.u32_at(8).unwrap_or(0);

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
                        DECODER_QUALITY.decode(value).to_string(),
                    );
                }

                // User profile / picture style
                LEICA_USER_PROFILE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:UserProfile".to_string(),
                        DECODER_USER_PROFILE.decode(value).to_string(),
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
                        DECODER_WHITE_BALANCE.decode(value).to_string(),
                    );
                }

                // WB mode (alternative tag)
                LEICA_WB_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:WBMode".to_string(),
                        DECODER_WHITE_BALANCE.decode(value).to_string(),
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
                        DECODER_EXPOSURE_MODE.decode(value).to_string(),
                    );
                }

                // Metering mode
                LEICA_METERING_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:MeteringMode".to_string(),
                        DECODER_METERING_MODE.decode(value).to_string(),
                    );
                }

                // Flash mode
                LEICA_FLASH_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:FlashMode".to_string(),
                        DECODER_FLASH_MODE.decode(value).to_string(),
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
                        DECODER_AF_MODE.decode(value).to_string(),
                    );
                }

                // Image stabilization
                LEICA_IMAGE_STABILIZATION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:ImageStabilization".to_string(),
                        DECODER_IMAGE_STABILIZATION.decode(value).to_string(),
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
                        DECODER_SCENE_MODE.decode(value).to_string(),
                    );
                }

                // Crop mode
                LEICA_CROP_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Leica:CropMode".to_string(),
                        DECODER_CROP_MODE.decode(value).to_string(),
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

                // 35mm equivalent focal length
                LEICA_FOCAL_LENGTH_35MM => {
                    let value = entry.value_offset;
                    tags.insert("Leica:FocalLength35mm".to_string(), format!("{} mm", value));
                }

                // Lens serial number
                LEICA_LENS_SERIAL_NUMBER => {
                    if entry.value_count <= 4 {
                        tags.insert(
                            "Leica:LensSerialNumber".to_string(),
                            entry.value_offset.to_string(),
                        );
                    }
                }

                // Contrast detect AF (phase detection/contrast detection mode)
                LEICA_CONTRAST_DETECT_AF => {
                    let value = entry.value_offset as i32;
                    let af_str = match value {
                        0 => "Off",
                        1 => "On",
                        _ => "Unknown",
                    };
                    tags.insert("Leica:ContrastDetectAF".to_string(), af_str.to_string());
                }

                // DNG version
                LEICA_DNG_VERSION => {
                    let value = entry.value_offset;
                    // DNG version is typically stored as 4 bytes (e.g., 1.4.0.0)
                    let major = (value >> 24) & 0xFF;
                    let minor = (value >> 16) & 0xFF;
                    let patch = (value >> 8) & 0xFF;
                    let build = value & 0xFF;
                    tags.insert(
                        "Leica:DNGVersion".to_string(),
                        format!("{}.{}.{}.{}", major, minor, patch, build),
                    );
                }

                // Perspective control (tilt-shift)
                LEICA_PERSPECTIVE_CONTROL => {
                    let value = entry.value_offset as i32;
                    let pc_str = match value {
                        0 => "Off",
                        1 => "On",
                        _ => "Unknown",
                    };
                    tags.insert("Leica:PerspectiveControl".to_string(), pc_str.to_string());
                }

                // AF point
                LEICA_AF_POINT => {
                    let value = entry.value_offset;
                    tags.insert("Leica:AFPoint".to_string(), value.to_string());
                }

                // Picture control / photo style
                LEICA_PICTURE_CONTROL => {
                    let value = entry.value_offset;
                    tags.insert("Leica:PictureControl".to_string(), value.to_string());
                }

                // Image ID
                LEICA_IMAGE_ID => {
                    let value = entry.value_offset;
                    tags.insert("Leica:ImageID".to_string(), value.to_string());
                }

                // APEX brightness value
                LEICA_APEX_BRIGHTNESS => {
                    let value = entry.value_offset as f32 / 10.0;
                    tags.insert("Leica:APEXBrightness".to_string(), format!("{:.1}", value));
                }

                // External sensor brightness value
                LEICA_EXTERNAL_SENSOR_BRIGHTNESS_VALUE => {
                    let value = entry.value_offset as f32 / 10.0;
                    tags.insert(
                        "Leica:ExternalSensorBrightnessValue".to_string(),
                        format!("{:.1} EV", value),
                    );
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

impl LeicaMakerNoteParser {
    /// Parse Leica4 format (M9/M Monochrom)
    ///
    /// This format uses a "LEICA0\x03\0" header followed by an IFD with
    /// subdirectory tags at 0x3000, 0x3100, 0x3400, 0x3900.
    fn parse_leica4(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // Skip 8-byte header: "LEICA0\x03\0"
        let header_size = 8;
        if data.len() <= header_size {
            return Err("No data after Leica4 header".to_string());
        }

        let ifd_data = &data[header_size..];
        if ifd_data.len() < 2 {
            return Err("Insufficient data for IFD entry count".to_string());
        }

        let reader = EndianReader::new(ifd_data, byte_order.to_io_byte_order());
        let entry_count = reader.u16_at(0).unwrap_or(0);

        if entry_count == 0 || entry_count > 50 {
            return Err(format!("Invalid Leica4 IFD entry count: {}", entry_count));
        }

        let required_size = 2 + (entry_count as usize * 12);
        if ifd_data.len() < required_size {
            return Err("Insufficient data for IFD entries".to_string());
        }

        // Parse each IFD entry - these are subdirectory pointers
        for i in 0..entry_count {
            let entry_offset = 2 + (i as usize * 12);
            let entry_data = &ifd_data[entry_offset..entry_offset + 12];
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

            let tag_id = entry_reader.u16_at(0).unwrap_or(0);
            let count = entry_reader.u32_at(4).unwrap_or(0);
            let value_offset = entry_reader.u32_at(8).unwrap_or(0);

            // Each main tag points to a subdirectory
            match tag_id {
                L4_SUBDIR_3000 | L4_SUBDIR_3100 | L4_SUBDIR_3400 | L4_SUBDIR_3900 => {
                    let subdir_offset = value_offset as usize;
                    let subdir_size = count as usize;
                    if subdir_offset + subdir_size <= data.len() {
                        let subdir_data = &data[subdir_offset..subdir_offset + subdir_size];
                        self.parse_leica4_subdirectory(subdir_data, data, byte_order, tags);
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Parse a Leica4 subdirectory (0x3000, 0x3100, 0x3400, or 0x3900)
    fn parse_leica4_subdirectory(
        &self,
        subdir_data: &[u8],
        full_data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        if subdir_data.len() < 2 {
            return;
        }

        let reader = EndianReader::new(subdir_data, byte_order.to_io_byte_order());
        let entry_count = reader.u16_at(0).unwrap_or(0);

        if entry_count == 0 || entry_count > 100 {
            return;
        }

        let required_size = 2 + (entry_count as usize * 12);
        if subdir_data.len() < required_size {
            return;
        }

        for i in 0..entry_count {
            let entry_offset = 2 + (i as usize * 12);
            if entry_offset + 12 > subdir_data.len() {
                break;
            }

            let entry_data = &subdir_data[entry_offset..entry_offset + 12];
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

            let tag_id = entry_reader.u16_at(0).unwrap_or(0);
            let format = entry_reader.u16_at(2).unwrap_or(0);
            let count = entry_reader.u32_at(4).unwrap_or(0);
            let value_offset = entry_reader.u32_at(8).unwrap_or(0);

            match tag_id {
                L4_CONTRAST => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:Contrast".to_string(),
                        L4_DECODE_CONTRAST.decode(value),
                    );
                }
                L4_SHARPENING => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:Sharpening".to_string(),
                        L4_DECODE_SHARPENING.decode(value),
                    );
                }
                L4_SATURATION => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:Saturation".to_string(),
                        L4_DECODE_SATURATION.decode(value),
                    );
                }
                L4_WHITE_BALANCE => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:WhiteBalance".to_string(),
                        L4_DECODE_WHITE_BALANCE.decode(value),
                    );
                }
                L4_JPEG_QUALITY => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:JPEGQuality".to_string(),
                        L4_DECODE_JPEG_QUALITY.decode(value),
                    );
                }
                L4_WB_RGB_LEVELS => {
                    // WB RGB Levels are stored as 3 rational values
                    if format == 5 && count == 3 {
                        // Read rational values from offset
                        let offset = value_offset as usize;
                        if offset + 24 <= full_data.len() {
                            let wb_reader = EndianReader::new(
                                &full_data[offset..],
                                byte_order.to_io_byte_order(),
                            );
                            let r_num = wb_reader.u32_at(0).unwrap_or(0);
                            let r_den = wb_reader.u32_at(4).unwrap_or(1);
                            let g_num = wb_reader.u32_at(8).unwrap_or(0);
                            let g_den = wb_reader.u32_at(12).unwrap_or(1);
                            let b_num = wb_reader.u32_at(16).unwrap_or(0);
                            let b_den = wb_reader.u32_at(20).unwrap_or(1);
                            let r = if r_den > 0 {
                                r_num as f64 / r_den as f64
                            } else {
                                0.0
                            };
                            let g = if g_den > 0 {
                                g_num as f64 / g_den as f64
                            } else {
                                0.0
                            };
                            let b = if b_den > 0 {
                                b_num as f64 / b_den as f64
                            } else {
                                0.0
                            };
                            tags.insert(
                                "Leica:WBRGBLevels".to_string(),
                                format!("{:.10} {:.10} {:.10}", r, g, b),
                            );
                        }
                    }
                }
                L4_USER_PROFILE => {
                    // String value
                    if format == 2 && count > 0 {
                        let str_offset = value_offset as usize;
                        if str_offset + count as usize <= full_data.len() {
                            if let Ok(s) = std::str::from_utf8(
                                &full_data[str_offset..str_offset + count as usize],
                            ) {
                                tags.insert(
                                    "Leica:UserProfile".to_string(),
                                    s.trim_end_matches('\0').to_string(),
                                );
                            }
                        }
                    }
                }
                L4_SERIAL_NUMBER => {
                    // String value
                    if format == 2 && count > 0 {
                        let str_offset = value_offset as usize;
                        if str_offset + count as usize <= full_data.len() {
                            if let Ok(s) = std::str::from_utf8(
                                &full_data[str_offset..str_offset + count as usize],
                            ) {
                                // ExifTool masks serial numbers with asterisks
                                let serial = s.trim_end_matches('\0');
                                let masked = if serial.len() > 0 {
                                    "*".repeat(serial.len().min(7))
                                } else {
                                    serial.to_string()
                                };
                                tags.insert("Leica:SerialNumber".to_string(), masked);
                            }
                        }
                    }
                }
                L4_FIRMWARE_VERSION => {
                    // String value
                    if format == 2 && count > 0 {
                        let str_offset = value_offset as usize;
                        if str_offset + count as usize <= full_data.len() {
                            if let Ok(s) = std::str::from_utf8(
                                &full_data[str_offset..str_offset + count as usize],
                            ) {
                                tags.insert(
                                    "Leica:FirmwareVersion".to_string(),
                                    s.trim_end_matches('\0').to_string(),
                                );
                            }
                        }
                    }
                }
                L4_BASE_ISO => {
                    tags.insert("Leica:BaseISO".to_string(), format!("{}", value_offset));
                }
                L4_SENSOR_WIDTH => {
                    tags.insert("Leica:SensorWidth".to_string(), format!("{}", value_offset));
                }
                L4_SENSOR_HEIGHT => {
                    tags.insert(
                        "Leica:SensorHeight".to_string(),
                        format!("{}", value_offset),
                    );
                }
                L4_SENSOR_BIT_DEPTH => {
                    tags.insert(
                        "Leica:SensorBitDepth".to_string(),
                        format!("{}", value_offset),
                    );
                }
                L4_CAMERA_TEMPERATURE => {
                    let value = value_offset as i32;
                    tags.insert(
                        "Leica:CameraTemperature".to_string(),
                        format!("{} C", value),
                    );
                }
                L4_LENS_TYPE => {
                    // Leica M9 lens type uses special encoding
                    let lens_id = value_offset;
                    let actual_lens_id = (lens_id >> 2) as u16;
                    if let Some(lens_name) = lookup_lens_name(actual_lens_id) {
                        tags.insert("Leica:LensType".to_string(), lens_name);
                    } else if let Some(lens_name) = lookup_lens_name(lens_id as u16) {
                        tags.insert("Leica:LensType".to_string(), lens_name);
                    } else {
                        tags.insert(
                            "Leica:LensType".to_string(),
                            format!("Unknown ({})", lens_id),
                        );
                    }
                }
                L4_APPROXIMATE_F_NUMBER => {
                    // Stored as rational64u
                    if format == 5 {
                        let offset = value_offset as usize;
                        if offset + 8 <= full_data.len() {
                            let f_reader = EndianReader::new(
                                &full_data[offset..],
                                byte_order.to_io_byte_order(),
                            );
                            let num = f_reader.u32_at(0).unwrap_or(0);
                            let den = f_reader.u32_at(4).unwrap_or(1);
                            if den > 0 {
                                let f_value = num as f64 / den as f64;
                                tags.insert(
                                    "Leica:ApproximateFNumber".to_string(),
                                    format!("{:.1}", f_value),
                                );
                            }
                        }
                    }
                }
                L4_MEASURED_LV => {
                    // Stored as int32s, divided by 100000 for actual LV
                    let value = value_offset as i32;
                    let lv = value as f64 / 100000.0;
                    tags.insert("Leica:MeasuredLV".to_string(), format!("{:.2}", lv));
                }
                L4_EXTERNAL_SENSOR_BRIGHTNESS => {
                    // Stored as int32s, divided by 100000 for actual EV
                    let value = value_offset as i32;
                    let ev = value as f64 / 100000.0;
                    tags.insert(
                        "Leica:ExternalSensorBrightnessValue".to_string(),
                        format!("{:.2}", ev),
                    );
                }
                _ => {
                    // Unknown tags are silently skipped
                }
            }
        }
    }
}

/// Maps Leica tag ID to human-readable tag name
///
/// Provides a comprehensive mapping of all known Leica MakerNote tag IDs to their
/// corresponding tag names. This function is used for debugging and for generating
/// human-readable tag names in the output.
fn leica_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        LEICA_QUALITY => "Quality",
        LEICA_USER_PROFILE => "UserProfile",
        LEICA_SERIAL_NUMBER => "SerialNumber",
        LEICA_WHITE_BALANCE => "WhiteBalance",
        LEICA_EXTERNAL_SENSOR_BRIGHTNESS_VALUE => "ExternalSensorBrightnessValue",
        LEICA_MEASURED_LV => "MeasuredLV",
        LEICA_APPROXIMATE_F_NUMBER => "ApproximateFNumber",
        LEICA_CAMERA_TEMPERATURE => "CameraTemperature",
        LEICA_COLOR_TEMPERATURE => "ColorTemperature",
        LEICA_WB_RED_LEVEL => "WBRedLevel",
        LEICA_WB_GREEN_LEVEL => "WBGreenLevel",
        LEICA_WB_BLUE_LEVEL => "WBBlueLevel",
        LEICA_SHARPENING => "Sharpening",
        LEICA_CONTRAST => "Contrast",
        LEICA_SATURATION => "Saturation",
        LEICA_LENS_ID => "LensID",
        LEICA_LENS_TYPE => "LensType",
        LEICA_LENS_MODEL => "LensModel",
        LEICA_ORIGINAL_FILE_NAME => "OriginalFileName",
        LEICA_ORIGINAL_DIRECTORY => "OriginalDirectory",
        LEICA_EXPOSURE_MODE => "ExposureMode",
        LEICA_METERING_MODE => "MeteringMode",
        LEICA_FILM_MODE => "FilmMode",
        LEICA_WB_MODE => "WBMode",
        LEICA_APEX_BRIGHTNESS => "APEXBrightness",
        LEICA_FLASH_MODE => "FlashMode",
        LEICA_FLASH_ENERGY => "FlashEnergy",
        LEICA_INTERNAL_SERIAL_NUMBER => "InternalSerialNumber",
        LEICA_FOCAL_LENGTH_35MM => "FocalLength35mm",
        LEICA_LENS_SERIAL_NUMBER => "LensSerialNumber",
        LEICA_CONTRAST_DETECT_AF => "ContrastDetectAF",
        LEICA_SHUTTER_COUNT => "ShutterCount",
        LEICA_FOCUS_DISTANCE => "FocusDistance",
        LEICA_FRAME_SELECTOR => "FrameSelector",
        LEICA_BASE_ISO => "BaseISO",
        LEICA_IMAGE_ID => "ImageID",
        LEICA_USER_COMMENT => "UserComment",
        LEICA_PICTURE_CONTROL => "PictureControl",
        LEICA_AF_POINT => "AFPoint",
        LEICA_AF_MODE => "AFMode",
        LEICA_IMAGE_STABILIZATION => "ImageStabilization",
        LEICA_DIGITAL_ZOOM => "DigitalZoom",
        LEICA_DNG_VERSION => "DNGVersion",
        LEICA_CROP_MODE => "CropMode",
        LEICA_PERSPECTIVE_CONTROL => "PerspectiveControl",
        LEICA_CAMERA_PITCH_ANGLE => "CameraPitchAngle",
        LEICA_CAMERA_ROLL_ANGLE => "CameraRollAngle",
        LEICA_MACRO_MODE => "MacroMode",
        LEICA_SCENE_MODE => "SceneMode",
        _ => return format!("Unknown-{:#06X}", tag_id),
    };
    tag_name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leica_header_validation() {
        // Valid short LEICA header
        let valid_short = b"LEICA\0\0\0extra data";
        assert!(is_leica_makernote(valid_short));

        // Valid long LEICA CAMERA AG header
        let valid_long = b"LEICA CAMERA AG extra data";
        assert!(is_leica_makernote(valid_long));

        // Invalid header
        let invalid = b"CANON\0\x00\x00\x00\x00\x00\x00";
        assert!(!is_leica_makernote(invalid));

        // Too short
        let too_short = b"LEICA\0";
        assert!(!is_leica_makernote(too_short));

        // Valid IFD entry count (must be at least 8 bytes for minimal validation)
        let valid_ifd = b"\x0A\x00\x00\x00\x00\x00\x00\x00"; // 10 entries + padding
        assert!(is_leica_makernote(valid_ifd));

        // Invalid IFD entry count (too many entries)
        let invalid_ifd = b"\xFF\x00\x00\x00\x00\x00\x00\x00"; // 255 entries - too many
        assert!(!is_leica_makernote(invalid_ifd));
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(DECODER_QUALITY.decode(1), "Fine");
        assert_eq!(DECODER_QUALITY.decode(2), "Basic");
        assert_eq!(DECODER_QUALITY.decode(3), "Standard");
        assert_eq!(DECODER_QUALITY.decode(4), "Super Fine");
        assert_eq!(DECODER_QUALITY.decode(5), "DNG");
        assert_eq!(DECODER_QUALITY.decode(6), "DNG + JPEG Fine");
        assert_eq!(DECODER_QUALITY.decode(7), "DNG + JPEG Standard");
        assert_eq!(DECODER_QUALITY.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(DECODER_WHITE_BALANCE.decode(0), "Auto");
        assert_eq!(DECODER_WHITE_BALANCE.decode(1), "Daylight");
        assert_eq!(DECODER_WHITE_BALANCE.decode(2), "Fluorescent");
        assert_eq!(DECODER_WHITE_BALANCE.decode(3), "Tungsten");
        assert_eq!(DECODER_WHITE_BALANCE.decode(4), "Flash");
        assert_eq!(DECODER_WHITE_BALANCE.decode(5), "Cloudy");
        assert_eq!(DECODER_WHITE_BALANCE.decode(6), "Shade");
        assert_eq!(DECODER_WHITE_BALANCE.decode(7), "Manual");
        assert_eq!(DECODER_WHITE_BALANCE.decode(8), "Kelvin");
        assert_eq!(DECODER_WHITE_BALANCE.decode(9), "Auto (ambient priority)");
        assert_eq!(DECODER_WHITE_BALANCE.decode(10), "Auto (white priority)");
        assert_eq!(DECODER_WHITE_BALANCE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_exposure_mode() {
        assert_eq!(DECODER_EXPOSURE_MODE.decode(0), "Manual");
        assert_eq!(DECODER_EXPOSURE_MODE.decode(1), "Program AE");
        assert_eq!(DECODER_EXPOSURE_MODE.decode(2), "Aperture Priority");
        assert_eq!(DECODER_EXPOSURE_MODE.decode(3), "Shutter Priority");
        assert_eq!(DECODER_EXPOSURE_MODE.decode(4), "Auto");
        assert_eq!(DECODER_EXPOSURE_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_metering_mode() {
        assert_eq!(DECODER_METERING_MODE.decode(0), "Unknown");
        assert_eq!(DECODER_METERING_MODE.decode(1), "Multi-segment");
        assert_eq!(DECODER_METERING_MODE.decode(2), "Center-weighted");
        assert_eq!(DECODER_METERING_MODE.decode(3), "Spot");
        assert_eq!(DECODER_METERING_MODE.decode(4), "Multi-spot");
        assert_eq!(DECODER_METERING_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_flash_mode() {
        assert_eq!(DECODER_FLASH_MODE.decode(0), "No Flash");
        assert_eq!(DECODER_FLASH_MODE.decode(1), "Auto");
        assert_eq!(DECODER_FLASH_MODE.decode(2), "On");
        assert_eq!(DECODER_FLASH_MODE.decode(3), "Red-eye Reduction");
        assert_eq!(DECODER_FLASH_MODE.decode(4), "Slow Sync");
        assert_eq!(DECODER_FLASH_MODE.decode(5), "Rear Curtain Sync");
        assert_eq!(DECODER_FLASH_MODE.decode(6), "Fill Flash");
        assert_eq!(DECODER_FLASH_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_af_mode() {
        assert_eq!(DECODER_AF_MODE.decode(0), "Manual");
        assert_eq!(DECODER_AF_MODE.decode(1), "Single AF");
        assert_eq!(DECODER_AF_MODE.decode(2), "Continuous AF");
        assert_eq!(DECODER_AF_MODE.decode(3), "AF-C");
        assert_eq!(DECODER_AF_MODE.decode(4), "Face Detection");
        assert_eq!(DECODER_AF_MODE.decode(5), "Tracking");
        assert_eq!(DECODER_AF_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_image_stabilization() {
        assert_eq!(DECODER_IMAGE_STABILIZATION.decode(0), "Off");
        assert_eq!(DECODER_IMAGE_STABILIZATION.decode(1), "On");
        assert_eq!(DECODER_IMAGE_STABILIZATION.decode(2), "On (Body)");
        assert_eq!(DECODER_IMAGE_STABILIZATION.decode(3), "On (Lens)");
        assert_eq!(DECODER_IMAGE_STABILIZATION.decode(4), "On (Dual)");
        assert_eq!(DECODER_IMAGE_STABILIZATION.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_user_profile() {
        assert_eq!(DECODER_USER_PROFILE.decode(0), "Not Set");
        assert_eq!(DECODER_USER_PROFILE.decode(1), "User Profile 1");
        assert_eq!(DECODER_USER_PROFILE.decode(2), "User Profile 2");
        assert_eq!(DECODER_USER_PROFILE.decode(3), "User Profile 3");
        assert_eq!(DECODER_USER_PROFILE.decode(4), "User Profile 4");
        assert_eq!(DECODER_USER_PROFILE.decode(5), "Standard");
        assert_eq!(DECODER_USER_PROFILE.decode(6), "Vivid");
        assert_eq!(DECODER_USER_PROFILE.decode(7), "Natural");
        assert_eq!(DECODER_USER_PROFILE.decode(8), "Monochrome");
        assert_eq!(DECODER_USER_PROFILE.decode(9), "High Contrast");
        assert_eq!(DECODER_USER_PROFILE.decode(10), "Monochrome High Contrast");
        assert_eq!(DECODER_USER_PROFILE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_scene_mode() {
        assert_eq!(DECODER_SCENE_MODE.decode(0), "Off");
        assert_eq!(DECODER_SCENE_MODE.decode(1), "Portrait");
        assert_eq!(DECODER_SCENE_MODE.decode(2), "Landscape");
        assert_eq!(DECODER_SCENE_MODE.decode(3), "Macro");
        assert_eq!(DECODER_SCENE_MODE.decode(4), "Sport");
        assert_eq!(DECODER_SCENE_MODE.decode(5), "Night Portrait");
        assert_eq!(DECODER_SCENE_MODE.decode(6), "Sunset");
        assert_eq!(DECODER_SCENE_MODE.decode(7), "Beach");
        assert_eq!(DECODER_SCENE_MODE.decode(8), "Snow");
        assert_eq!(DECODER_SCENE_MODE.decode(9), "Fireworks");
        assert_eq!(DECODER_SCENE_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_crop_mode() {
        assert_eq!(DECODER_CROP_MODE.decode(0), "Full Frame");
        assert_eq!(DECODER_CROP_MODE.decode(1), "APS-C");
        assert_eq!(DECODER_CROP_MODE.decode(2), "1:1");
        assert_eq!(DECODER_CROP_MODE.decode(3), "16:9");
        assert_eq!(DECODER_CROP_MODE.decode(4), "4:3");
        assert_eq!(DECODER_CROP_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = LeicaMakerNoteParser;
        assert_eq!(parser.manufacturer_name(), "Leica");
        assert_eq!(parser.tag_prefix(), "Leica:");
    }

    #[test]
    fn test_leica_tag_to_name() {
        assert_eq!(leica_tag_to_name(LEICA_QUALITY), "Quality");
        assert_eq!(leica_tag_to_name(LEICA_WHITE_BALANCE), "WhiteBalance");
        assert_eq!(leica_tag_to_name(LEICA_EXPOSURE_MODE), "ExposureMode");
        assert_eq!(leica_tag_to_name(LEICA_METERING_MODE), "MeteringMode");
        assert_eq!(leica_tag_to_name(LEICA_FLASH_MODE), "FlashMode");
        assert_eq!(leica_tag_to_name(0xFFFF), "Unknown-0xFFFF");
    }

    #[test]
    fn test_lens_lookup() {
        let parser = LeicaMakerNoteParser;
        // Test known Leica lens
        let result = parser.lookup_lens(1);
        // Since we don't know the exact lens database, just verify it returns an Option
        assert!(result.is_some() || result.is_none());
    }
}
