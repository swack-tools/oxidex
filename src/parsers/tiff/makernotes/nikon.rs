//! Nikon MakerNote parser
//!
//! Parses Nikon-specific EXIF MakerNote tags containing camera settings,
//! lens information, autofocus data, and other proprietary metadata.
//!
//! Supports Nikon Type 2 (IFD-based) and Type 3 (IFD-based with header) formats.

#![allow(dead_code)]
#![allow(unused_imports)]

// Submodules for extended tag parsing
pub mod color_balance;
pub mod lens_data;
pub mod shot_info;

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

use super::nikon_lens_database::lookup_lens_name;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};

// Nikon MakerNote Tag IDs (from ExifTool Nikon.pm)
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
const NIKON_COLOR_BALANCE: u16 = 0x000C;
const NIKON_PROGRAM_SHIFT: u16 = 0x000F;
const NIKON_EXPOSURE_DIFF: u16 = 0x0010;
const NIKON_ISO_SELECTION: u16 = 0x0011; // Also NIKON_PREVIEW_IFD
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
const NIKON_IMAGE_DATA_SIZE: u16 = 0x00A2;
const NIKON_IMAGE_COUNT: u16 = 0x00A5;
const NIKON_DELETED_IMAGE_COUNT: u16 = 0x00A6;
const NIKON_SHUTTER_COUNT: u16 = 0x00A7;
const NIKON_FLASH_INFO: u16 = 0x00A8; // Array tag
const NIKON_IMAGE_OPTIMIZATION: u16 = 0x00A9;
const NIKON_TONE_COMP: u16 = 0x0081;
const NIKON_COLOR_SPACE: u16 = 0x00B0;
const NIKON_VR_INFO: u16 = 0x00B1; // Array tag - vibration reduction
const NIKON_ACTIVE_D_LIGHTING: u16 = 0x00B3;
const NIKON_PICTURE_CONTROL: u16 = 0x00B4; // Array tag
const NIKON_WORLD_TIME: u16 = 0x00B5;
const NIKON_ISO_INFO: u16 = 0x00B6; // Array tag
const NIKON_VIGNETTE_CONTROL: u16 = 0x00B7;
const NIKON_DISTORTION_CONTROL: u16 = 0x00B8;
const NIKON_SATURATION_TEXT: u16 = 0x00AA; // Saturation as text
const NIKON_VARI_PROGRAM: u16 = 0x00AB; // VariProgram
const NIKON_IMAGE_PROCESSING: u16 = 0x001A; // Image processing
const NIKON_DISTORT_INFO: u16 = 0x002B; // Distortion info
const NIKON_WORLD_TIME_ALT: u16 = 0x0024; // World time alternate
const NIKON_ISO_INFO_ALT: u16 = 0x0025; // ISO info alternate
const NIKON_VR_INFO_ALT: u16 = 0x001F; // VR info alternate
const NIKON_FLASH_EXPOSURE_COMP: u16 = 0x0012; // Flash exposure compensation
const NIKON_EXTERNAL_FLASH_COMP: u16 = 0x0017; // External flash exposure compensation
const NIKON_FLASH_BRACKET_VALUE: u16 = 0x0018; // Flash exposure bracket value
const NIKON_EXPOSURE_BRACKET_VALUE: u16 = 0x0019; // Exposure bracket value
const NIKON_COLOR_SPACE_ALT: u16 = 0x001E; // Color space alternate
const NIKON_IMAGE_AUTH: u16 = 0x0020; // Image authentication
const NIKON_ACTIVE_D_LIGHTING_ALT: u16 = 0x0022; // Active D-Lighting alternate
const NIKON_PICTURE_CONTROL_DATA: u16 = 0x0023; // Picture control data
const NIKON_VIGNETTE_CONTROL_ALT: u16 = 0x0026; // Vignette control alternate
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
const NIKON_MULTI_EXPOSURE: u16 = 0x00B2; // Multi Exposure
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

/// Decodes Nikon quality setting to human-readable string
fn decode_quality(value: i32) -> &'static str {
    match value {
        1 => "VGA Basic",
        2 => "VGA Normal",
        3 => "VGA Fine",
        4 => "SXGA Basic",
        5 => "SXGA Normal",
        6 => "SXGA Fine",
        7 => "XGA Basic",
        8 => "XGA Normal",
        9 => "XGA Fine",
        10 => "UXGA Basic",
        11 => "UXGA Normal",
        12 => "UXGA Fine",
        _ => "Unknown",
    }
}

/// Decodes Nikon white balance setting to human-readable string
fn decode_white_balance(value: i32) -> &'static str {
    match value {
        0 => "Auto",
        1 => "Daylight",
        2 => "Incandescent",
        3 => "Fluorescent",
        4 => "Cloudy",
        5 => "Speedlight",
        6 => "Custom",
        7 => "Shade",
        8 => "Kelvin",
        _ => "Unknown",
    }
}

/// Decodes Nikon focus mode to human-readable string
fn decode_focus_mode(value: i32) -> &'static str {
    match value {
        0 => "AF-S",
        1 => "AF-C",
        2 => "AF-A",
        3 => "MF (Manual)",
        4 => "AF-S (Single)",
        5 => "AF-C (Continuous)",
        _ => "Unknown",
    }
}

/// Decodes Nikon flash setting to human-readable string
fn decode_flash_setting(value: i32) -> &'static str {
    match value {
        0 => "Normal",
        1 => "Red-eye Reduction",
        2 => "Rear Curtain",
        3 => "Slow Sync",
        4 => "Red-eye + Slow",
        5 => "Rear + Slow",
        6 => "Off",
        _ => "Unknown",
    }
}

/// Decodes Nikon flash mode to human-readable string
fn decode_flash_mode(value: i32) -> &'static str {
    match value {
        0 => "Did Not Fire",
        1 => "Fired, Manual",
        3 => "Not Ready",
        7 => "Fired, External",
        8 => "Fired, Commander Mode",
        9 => "Fired, TTL Mode",
        _ => "Unknown",
    }
}

/// Decodes Nikon shooting mode to human-readable string
fn decode_shooting_mode(value: i32) -> &'static str {
    match value {
        0 => "Single Frame",
        1 => "Continuous",
        2 => "Self-timer",
        3 => "Delayed Remote",
        4 => "Quick-Response Remote",
        5 => "Self-timer (Mirror Up)",
        6 => "Interval Timer",
        _ => "Unknown",
    }
}

/// Decodes Nikon color space to human-readable string
fn decode_color_space(value: i32) -> &'static str {
    match value {
        1 => "sRGB",
        2 => "Adobe RGB",
        _ => "Unknown",
    }
}

/// Decodes Nikon Active D-Lighting setting to human-readable string
fn decode_active_d_lighting(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "Low",
        3 => "Normal",
        5 => "High",
        7 => "Extra High",
        8 => "Extra High 1",
        9 => "Extra High 2",
        0xFFFF => "Auto",
        _ => "Unknown",
    }
}

/// Decodes Nikon vignette control to human-readable string
fn decode_vignette_control(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "Low",
        2 => "Normal",
        3 => "High",
        _ => "Unknown",
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

        // Parse IFD entries starting at the IFD location
        // Pass the full 'data' buffer so that offset calculations work correctly
        let _ = parse_ifd_entries(
            &data[ifd_absolute..],
            tiff_byte_order,
            &config,
            |entry, _ifd_data| {
                // Offsets in Nikon MakerNote IFD entries are relative to the embedded TIFF structure
                // which starts at byte 10 (tiff_start) in the full data buffer

                match entry.tag_id {
                    // Simple string tags
                    NIKON_VERSION | NIKON_SERIAL_NUMBER => {
                        // String offsets are relative to the TIFF header (byte 10)
                        if let Some(value) = extract_string_with_offset(entry, data, tiff_start) {
                            let tag_name = nikon_tag_to_name(entry.tag_id);
                            tags.insert(tag_name, value);
                        }
                    }

                    // Simple integer tags
                    NIKON_ISO_SPEED => {
                        let value = entry.value_offset as i32;
                        tags.insert("Nikon:ISOSpeed".to_string(), format!("ISO {}", value));
                    }

                    NIKON_SHUTTER_COUNT => {
                        let value = entry.value_offset;
                        tags.insert("Nikon:ShutterCount".to_string(), value.to_string());
                    }

                    NIKON_IMAGE_COUNT => {
                        let value = entry.value_offset;
                        tags.insert("Nikon:ImageCount".to_string(), value.to_string());
                    }

                    // Enumerated values
                    NIKON_QUALITY => {
                        let value = entry.value_offset as i32;
                        tags.insert(
                            "Nikon:Quality".to_string(),
                            decode_quality(value).to_string(),
                        );
                    }

                    NIKON_WHITE_BALANCE => {
                        let value = entry.value_offset as i32;
                        tags.insert(
                            "Nikon:WhiteBalance".to_string(),
                            decode_white_balance(value).to_string(),
                        );
                    }

                    NIKON_FOCUS_MODE => {
                        let value = entry.value_offset as i32;
                        tags.insert(
                            "Nikon:FocusMode".to_string(),
                            decode_focus_mode(value).to_string(),
                        );
                    }

                    NIKON_FLASH_SETTING => {
                        let value = entry.value_offset as i32;
                        tags.insert(
                            "Nikon:FlashSetting".to_string(),
                            decode_flash_setting(value).to_string(),
                        );
                    }

                    NIKON_FLASH_MODE => {
                        let value = entry.value_offset as i32;
                        tags.insert(
                            "Nikon:FlashMode".to_string(),
                            decode_flash_mode(value).to_string(),
                        );
                    }

                    NIKON_SHOOTING_MODE => {
                        let value = entry.value_offset as i32;
                        tags.insert(
                            "Nikon:ShootingMode".to_string(),
                            decode_shooting_mode(value).to_string(),
                        );
                    }

                    NIKON_COLOR_SPACE => {
                        let value = entry.value_offset as i32;
                        tags.insert(
                            "Nikon:ColorSpace".to_string(),
                            decode_color_space(value).to_string(),
                        );
                    }

                    NIKON_ACTIVE_D_LIGHTING => {
                        let value = entry.value_offset as i32;
                        tags.insert(
                            "Nikon:ActiveDLighting".to_string(),
                            decode_active_d_lighting(value).to_string(),
                        );
                    }

                    NIKON_VIGNETTE_CONTROL => {
                        let value = entry.value_offset as i32;
                        tags.insert(
                            "Nikon:VignetteControl".to_string(),
                            decode_vignette_control(value).to_string(),
                        );
                    }

                    // Lens information (simple format)
                    NIKON_LENS_TYPE => {
                        let value = entry.value_offset;
                        tags.insert("Nikon:LensType".to_string(), format!("0x{:02X}", value));
                    }

                    // LensData array (complex)
                    NIKON_LENS_DATA => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order) {
                            // Extract lens ID and look up lens name
                            if array.len() > LENS_DATA_LENS_ID {
                                let lens_id = array[LENS_DATA_LENS_ID];
                                if let Some(lens_name) = lookup_lens_name(lens_id) {
                                    tags.insert("Nikon:LensID".to_string(), lens_name);
                                } else {
                                    tags.insert(
                                        "Nikon:LensID".to_string(),
                                        format!("Unknown ({})", lens_id),
                                    );
                                }
                            }

                            // Extract focal length
                            if array.len() > LENS_DATA_FOCAL_LENGTH {
                                let focal_length = array[LENS_DATA_FOCAL_LENGTH];
                                tags.insert(
                                    "Nikon:FocalLength".to_string(),
                                    format!("{} mm", focal_length),
                                );
                            }

                            // Extract focus distance
                            if array.len() > LENS_DATA_FOCUS_DISTANCE {
                                let focus_distance = array[LENS_DATA_FOCUS_DISTANCE];
                                tags.insert(
                                    "Nikon:FocusDistance".to_string(),
                                    format!("{} mm", focus_distance),
                                );
                            }

                            // Extract aperture range
                            if array.len() > LENS_DATA_MAX_APERTURE_AT_MIN_FOCAL {
                                let max_aperture_min = array[LENS_DATA_MAX_APERTURE_AT_MIN_FOCAL];
                                tags.insert(
                                    "Nikon:MaxApertureAtMinFocal".to_string(),
                                    format!("f/{:.1}", max_aperture_min as f32 / 10.0),
                                );
                            }

                            if array.len() > LENS_DATA_MAX_APERTURE_AT_MAX_FOCAL {
                                let max_aperture_max = array[LENS_DATA_MAX_APERTURE_AT_MAX_FOCAL];
                                tags.insert(
                                    "Nikon:MaxApertureAtMaxFocal".to_string(),
                                    format!("f/{:.1}", max_aperture_max as f32 / 10.0),
                                );
                            }
                        }
                    }

                    // ShotInfo array
                    NIKON_SHOT_INFO => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order) {
                            // Version
                            if !array.is_empty() {
                                tags.insert(
                                    "Nikon:ShotInfoVersion".to_string(),
                                    format!("{}", array[SHOT_INFO_VERSION]),
                                );
                            }

                            // Shutter count (alternative location)
                            if array.len() > SHOT_INFO_SHUTTER_COUNT {
                                let shutter_count = array[SHOT_INFO_SHUTTER_COUNT];
                                if shutter_count > 0 {
                                    tags.insert(
                                        "Nikon:ShotInfoShutterCount".to_string(),
                                        shutter_count.to_string(),
                                    );
                                }
                            }

                            // AF point used
                            if array.len() > SHOT_INFO_AF_POINT_USED {
                                let af_point = array[SHOT_INFO_AF_POINT_USED];
                                tags.insert("Nikon:AFPointUsed".to_string(), af_point.to_string());
                            }

                            // Vibration reduction
                            if array.len() > SHOT_INFO_VIBRATION_REDUCTION {
                                let vr = array[SHOT_INFO_VIBRATION_REDUCTION];
                                let vr_status = if vr == 0 { "Off" } else { "On" };
                                tags.insert(
                                    "Nikon:VibrationReduction".to_string(),
                                    vr_status.to_string(),
                                );
                            }

                            // Auto ISO
                            if array.len() > SHOT_INFO_AUTO_ISO {
                                let auto_iso = array[SHOT_INFO_AUTO_ISO];
                                if auto_iso > 0 {
                                    tags.insert(
                                        "Nikon:AutoISO".to_string(),
                                        format!("ISO {}", auto_iso),
                                    );
                                }
                            }
                        }
                    }

                    // ColorBalance array (white balance RGB coefficients)
                    NIKON_COLOR_BALANCE_A => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && array.len() >= 4
                        {
                            tags.insert(
                                "Nikon:WB_RBLevels".to_string(),
                                format!("{} {}", array[0], array[1]),
                            );
                        }
                    }

                    // Additional string tags
                    NIKON_IMAGE_OPTIMIZATION
                    | NIKON_SATURATION_TEXT
                    | NIKON_VARI_PROGRAM
                    | NIKON_COLOR_MODE
                    | NIKON_SCENE_MODE
                    | NIKON_LIGHT_SOURCE
                    | NIKON_NOISE_REDUCTION
                    | NIKON_TONE_COMP
                    | NIKON_COLOR_HUE
                    | NIKON_IMAGE_PROCESSING
                    | NIKON_PICTURE_CONTROL
                    | NIKON_SCENE_ASSIST
                    | NIKON_RETOUCH_HISTORY
                    | NIKON_FLASH_TYPE => {
                        if let Some(value) = extract_string_with_offset(entry, data, tiff_start) {
                            let tag_name = nikon_tag_to_name(entry.tag_id);
                            tags.insert(tag_name, value);
                        }
                    }

                    // Additional integer tags
                    NIKON_DELETED_IMAGE_COUNT => {
                        let value = entry.value_offset;
                        tags.insert("Nikon:DeletedImageCount".to_string(), value.to_string());
                    }

                    NIKON_IMAGE_DATA_SIZE => {
                        let value = entry.value_offset;
                        tags.insert("Nikon:ImageDataSize".to_string(), value.to_string());
                    }

                    NIKON_WHITE_BALANCE_FINE => {
                        let value = entry.value_offset as i32;
                        tags.insert("Nikon:WhiteBalanceFineTune".to_string(), value.to_string());
                    }

                    NIKON_PROGRAM_SHIFT => {
                        let value = entry.value_offset as i32;
                        tags.insert("Nikon:ProgramShift".to_string(), value.to_string());
                    }

                    NIKON_EXPOSURE_DIFF => {
                        let value = entry.value_offset as i32;
                        tags.insert("Nikon:ExposureDifference".to_string(), value.to_string());
                    }

                    NIKON_FLASH_EXPOSURE_COMP => {
                        let value = entry.value_offset as i32;
                        let ev = value as f32 / 6.0;
                        tags.insert(
                            "Nikon:FlashExposureComp".to_string(),
                            format!("{:+.1} EV", ev),
                        );
                    }

                    NIKON_EXTERNAL_FLASH_COMP => {
                        let value = entry.value_offset as i32;
                        let ev = value as f32 / 6.0;
                        tags.insert(
                            "Nikon:ExternalFlashExposureComp".to_string(),
                            format!("{:+.1} EV", ev),
                        );
                    }

                    NIKON_FLASH_BRACKET_VALUE => {
                        let value = entry.value_offset as i32;
                        let ev = value as f32 / 6.0;
                        tags.insert(
                            "Nikon:FlashExposureBracketValue".to_string(),
                            format!("{:+.1} EV", ev),
                        );
                    }

                    NIKON_EXPOSURE_BRACKET_VALUE => {
                        let value = entry.value_offset as i32;
                        let ev = value as f32 / 6.0;
                        tags.insert(
                            "Nikon:ExposureBracketValue".to_string(),
                            format!("{:+.1} EV", ev),
                        );
                    }

                    NIKON_EXPOSURE_TUNING => {
                        let value = entry.value_offset as i32;
                        let ev = value as f32 / 6.0;
                        tags.insert("Nikon:ExposureTuning".to_string(), format!("{:+.1} EV", ev));
                    }

                    NIKON_HUE_ADJUSTMENT => {
                        let value = entry.value_offset as i32;
                        tags.insert("Nikon:HueAdjustment".to_string(), format!("{}", value));
                    }

                    NIKON_SATURATION => {
                        let value = entry.value_offset as i32;
                        tags.insert("Nikon:SaturationLevel".to_string(), format!("{}", value));
                    }

                    NIKON_SHARPNESS => {
                        let value = entry.value_offset as i32;
                        tags.insert("Nikon:Sharpness".to_string(), format!("{}", value));
                    }

                    NIKON_LENS_FSTOPS => {
                        let value = entry.value_offset as f32 / 12.0;
                        tags.insert("Nikon:LensFStops".to_string(), format!("{:.1}", value));
                    }

                    NIKON_NEF_COMPRESSION => {
                        let value = entry.value_offset as i32;
                        let mode = match value {
                            1 => "Lossy (type 1)",
                            2 => "Uncompressed",
                            3 => "Lossless",
                            4 => "Lossy (type 2)",
                            5 => "Striped Lossless",
                            6 => "High Efficiency",
                            7 => "High Efficiency*",
                            _ => "Unknown",
                        };
                        tags.insert("Nikon:NEFCompression".to_string(), mode.to_string());
                    }

                    NIKON_IMAGE_AUTH => {
                        let value = entry.value_offset as i32;
                        let status = if value == 0 { "Off" } else { "On" };
                        tags.insert("Nikon:ImageAuthentication".to_string(), status.to_string());
                    }

                    NIKON_ISO_SELECTION => {
                        let value = entry.value_offset as i32;
                        let selection = if value == 0 { "Auto" } else { "Manual" };
                        tags.insert("Nikon:ISOSelection".to_string(), selection.to_string());
                    }

                    NIKON_ISO_SETTING => {
                        let value = entry.value_offset as i32;
                        if value > 0 {
                            tags.insert("Nikon:ISOSetting".to_string(), format!("ISO {}", value));
                        }
                    }

                    NIKON_DISTORTION_CONTROL | NIKON_DISTORT_INFO => {
                        let value = entry.value_offset as i32;
                        let mode = match value {
                            0 => "Off",
                            1 => "On",
                            2 => "On (Cannot Disable)",
                            _ => "Unknown",
                        };
                        tags.insert("Nikon:DistortionControl".to_string(), mode.to_string());
                    }

                    // Note: HIGH_ISO_NR (0x00B0) removed - same tag ID as ColorSpace, handled above

                    // Array tags
                    NIKON_AF_INFO => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && !array.is_empty()
                        {
                            tags.insert("Nikon:AFInfo".to_string(), format!("{}", array[0]));
                        }
                    }

                    NIKON_FLASH_INFO => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && !array.is_empty()
                        {
                            tags.insert(
                                "Nikon:FlashInfoVersion".to_string(),
                                format!("{}", array[0]),
                            );
                        }
                    }

                    NIKON_WORLD_TIME | NIKON_WORLD_TIME_ALT => {
                        let offset_minutes = entry.value_offset as i32;
                        let hours = offset_minutes / 60;
                        let minutes = (offset_minutes % 60).abs();
                        let sign = if offset_minutes >= 0 { "+" } else { "-" };
                        tags.insert(
                            "Nikon:WorldTime".to_string(),
                            format!("UTC{}{:02}:{:02}", sign, hours.abs(), minutes),
                        );
                    }

                    NIKON_ISO_INFO | NIKON_ISO_INFO_ALT => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && !array.is_empty()
                        {
                            tags.insert("Nikon:ISOExpansion".to_string(), format!("{}", array[0]));
                        }
                    }

                    NIKON_VR_INFO | NIKON_VR_INFO_ALT => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && !array.is_empty()
                        {
                            tags.insert("Nikon:VRInfoVersion".to_string(), format!("{}", array[0]));
                            if array.len() > 1 {
                                let vr_mode = match array[1] {
                                    0 => "Off",
                                    1 => "Normal",
                                    2 => "Active",
                                    3 => "Sport",
                                    _ => "Unknown",
                                };
                                tags.insert("Nikon:VRMode".to_string(), vr_mode.to_string());
                            }
                        }
                    }

                    NIKON_MULTI_EXPOSURE => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && !array.is_empty()
                        {
                            let mode = match array[0] {
                                0 => "Off",
                                1 => "Multiple Exposure",
                                2 => "Image Overlay",
                                3 => "HDR",
                                _ => "Unknown",
                            };
                            tags.insert("Nikon:MultiExposureMode".to_string(), mode.to_string());
                        }
                    }

                    NIKON_IMAGE_BOUNDARY => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && array.len() >= 4
                        {
                            tags.insert(
                                "Nikon:ImageBoundary".to_string(),
                                format!("{} {} {} {}", array[0], array[1], array[2], array[3]),
                            );
                        }
                    }

                    NIKON_CROP_HI_SPEED => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && !array.is_empty()
                        {
                            let mode = match array[0] {
                                0 => "Off",
                                1 => "1.3x Crop",
                                2 => "DX Crop",
                                3 => "5:4 Crop",
                                4 => "1:1 Crop",
                                _ => "Unknown Crop",
                            };
                            tags.insert("Nikon:CropHiSpeed".to_string(), mode.to_string());
                        }
                    }

                    NIKON_COLOR_BALANCE => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && array.len() >= 4
                        {
                            tags.insert(
                                "Nikon:ColorBalance".to_string(),
                                format!("{} {} {} {}", array[0], array[1], array[2], array[3]),
                            );
                        }
                    }

                    NIKON_PICTURE_CONTROL_DATA => {
                        if let Some(array) = extract_u16_array(entry, data, byte_order)
                            && !array.is_empty()
                        {
                            tags.insert(
                                "Nikon:PictureControlVersion".to_string(),
                                format!("{}", array[0]),
                            );
                        }
                    }

                    NIKON_SENSOR_PIXEL_SIZE => {
                        let value = entry.value_offset;
                        tags.insert(
                            "Nikon:SensorPixelSize".to_string(),
                            format!("0x{:08X}", value),
                        );
                    }

                    // Skip unrecognized tags silently
                    _ => {}
                }
            },
        );

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
        NIKON_FOCUS_MODE => "Focus",
        NIKON_FLASH_SETTING => "FlashSetting",
        NIKON_FLASH_TYPE => "FlashType",
        NIKON_WHITE_BALANCE_FINE => "WhiteBalanceFineTune",
        NIKON_COLOR_BALANCE => "WBRBLevels",
        NIKON_PROGRAM_SHIFT => "ProgramShift",
        NIKON_EXPOSURE_DIFF => "ExposureDifference",
        NIKON_ISO_SELECTION => "ISOSelection",
        // Note: PREVIEW_IFD = 0x0011 same as ISO_SELECTION, handled above
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
        NIKON_COLOR_SPACE_ALT => "ColorSpace",
        NIKON_VR_INFO_ALT => "VRInfo",
        NIKON_IMAGE_AUTH => "ImageAuthentication",
        NIKON_ACTIVE_D_LIGHTING_ALT => "ActiveD-Lighting",
        NIKON_PICTURE_CONTROL_DATA => "PictureControlData",
        NIKON_WORLD_TIME_ALT => "WorldTime",
        NIKON_ISO_INFO_ALT => "ISOInfo",
        NIKON_VIGNETTE_CONTROL_ALT => "VignetteControl",
        NIKON_DISTORT_INFO => "DistortInfo",

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
        NIKON_SATURATION => "Saturation",
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
        NIKON_COLOR_SPACE => "ColorSpace", // 0x00B0 (also HIGH_ISO_NR)
        NIKON_VR_INFO => "VRInfo",
        NIKON_MULTI_EXPOSURE => "MultiExposure",
        NIKON_ACTIVE_D_LIGHTING => "ActiveD-Lighting",
        NIKON_PICTURE_CONTROL => "PictureControl",
        NIKON_WORLD_TIME => "WorldTime",
        NIKON_ISO_INFO => "ISOInfo",
        NIKON_VIGNETTE_CONTROL => "VignetteControl", // 0x00B7 (also AF_INFO2)
        NIKON_DISTORTION_CONTROL => "DistortionControl", // 0x00B8 (also FILE_INFO)

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

    #[test]
    fn test_decode_quality() {
        assert_eq!(decode_quality(1), "VGA Basic");
        assert_eq!(decode_quality(6), "SXGA Fine");
        assert_eq!(decode_quality(12), "UXGA Fine");
        assert_eq!(decode_quality(99), "Unknown");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(decode_white_balance(0), "Auto");
        assert_eq!(decode_white_balance(1), "Daylight");
        assert_eq!(decode_white_balance(5), "Speedlight");
        assert_eq!(decode_white_balance(99), "Unknown");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(decode_focus_mode(0), "AF-S");
        assert_eq!(decode_focus_mode(1), "AF-C");
        assert_eq!(decode_focus_mode(3), "MF (Manual)");
        assert_eq!(decode_focus_mode(99), "Unknown");
    }

    #[test]
    fn test_decode_flash_setting() {
        assert_eq!(decode_flash_setting(0), "Normal");
        assert_eq!(decode_flash_setting(2), "Rear Curtain");
        assert_eq!(decode_flash_setting(6), "Off");
        assert_eq!(decode_flash_setting(99), "Unknown");
    }

    #[test]
    fn test_decode_color_space() {
        assert_eq!(decode_color_space(1), "sRGB");
        assert_eq!(decode_color_space(2), "Adobe RGB");
        assert_eq!(decode_color_space(99), "Unknown");
    }

    #[test]
    fn test_decode_active_d_lighting() {
        assert_eq!(decode_active_d_lighting(0), "Off");
        assert_eq!(decode_active_d_lighting(1), "Low");
        assert_eq!(decode_active_d_lighting(3), "Normal");
        assert_eq!(decode_active_d_lighting(0xFFFF), "Auto");
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
