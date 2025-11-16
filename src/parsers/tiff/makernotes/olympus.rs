//! Olympus MakerNote Parser
//!
//! Parses Olympus-specific EXIF MakerNote tags containing camera settings,
//! lens information, image quality parameters, and other proprietary metadata.
//!
//! Supports both Four Thirds (E-series DSLRs) and Micro Four Thirds (OM-D, PEN) cameras.
//!
//! Based on ExifTool's Olympus.pm module.

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

use super::olympus_lens_database::lookup_lens_name;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use super::shared::MakerNoteParser;

// ===== Olympus MakerNote Tag IDs =====
// Based on ExifTool Olympus.pm tag definitions

// Basic Camera Information Tags
const OLYMPUS_CAMERA_SETTINGS: u16 = 0x0003;
const OLYMPUS_EQUIPMENT: u16 = 0x0201;
const OLYMPUS_CAMERA_SETTINGS_2: u16 = 0x0202;
const OLYMPUS_RAW_DEVELOPMENT: u16 = 0x0203;
const OLYMPUS_IMAGE_PROCESSING: u16 = 0x0204;
const OLYMPUS_FOCUS_INFO: u16 = 0x0205;
const OLYMPUS_RAW_INFO: u16 = 0x0207;
const OLYMPUS_MAIN_INFO: u16 = 0x0208;

// Simple tags
const OLYMPUS_SPECIAL_MODE: u16 = 0x0000;
const OLYMPUS_JPEG_QUALITY: u16 = 0x0001;
const OLYMPUS_MACRO_MODE: u16 = 0x0002;
const OLYMPUS_DIGITAL_ZOOM: u16 = 0x0004;
const OLYMPUS_SOFTWARE_RELEASE: u16 = 0x0005;
const OLYMPUS_PICT_INFO: u16 = 0x0006;
const OLYMPUS_CAMERA_ID: u16 = 0x0007;
const OLYMPUS_IMAGE_WIDTH: u16 = 0x0008;
const OLYMPUS_IMAGE_HEIGHT: u16 = 0x0009;
const OLYMPUS_ORIGINAL_MANUFACTURER_MODEL: u16 = 0x000A;
const OLYMPUS_PREVIEW_IMAGE: u16 = 0x0100;
const OLYMPUS_THUMBNAIL_IMAGE: u16 = 0x0104;
const OLYMPUS_BODY_FIRMWARE_VERSION: u16 = 0x0404;
const OLYMPUS_LENS_MODEL: u16 = 0x0206;

// Olympus MakerNote header signature
// Olympus uses "OLYMPUS\0II" or "OLYMPUS\0MM" (10 bytes) followed by offset
const OLYMPUS_HEADER: &[u8] = b"OLYMPUS\0II";
const OLYMPUS_HEADER_BE: &[u8] = b"OLYMPUS\0MM";

// Camera Settings array indices (tag 0x0003)
const CS_PREVIEW_IMAGE_VALID: usize = 0;
const CS_PREVIEW_IMAGE_START: usize = 1;
const CS_PREVIEW_IMAGE_LENGTH: usize = 2;
const CS_EXPOSURE_MODE: usize = 3;
const CS_AE_LOCK: usize = 4;
const CS_METERING_MODE: usize = 5;
const CS_MACRO_MODE: usize = 6;
const CS_FOCUS_MODE: usize = 7;
const CS_FOCUS_PROCESS: usize = 8;
const CS_AF_SEARCH: usize = 9;
const CS_AF_AREAS: usize = 10;
const CS_AF_POINT_SELECTED: usize = 11;
const CS_EXPOSURE_COMPENSATION: usize = 12;
const CS_CENTER_WEIGHTED_AREA: usize = 13;
const CS_AE_BRACKET_STEP: usize = 14;
const CS_AE_BRACKET_XVAL: usize = 15;
const CS_FLASH_MODE: usize = 16;
const CS_FLASH_EXPOSURE_COMP: usize = 17;
const CS_FLASH_REMOTE_CONTROL: usize = 18;
const CS_FLASH_CONTROL_MODE: usize = 19;
const CS_FLASH_INTENSITY: usize = 20;
const CS_WHITE_BALANCE: usize = 21;
const CS_WHITE_BALANCE_TEMPERATURE: usize = 22;
const CS_WHITE_BALANCE_BRACKET: usize = 23;
const CS_CUSTOM_SATURATION: usize = 24;
const CS_MODIFIED_SATURATION: usize = 25;
const CS_CONTRAST_SETTING: usize = 26;
const CS_SHARPNESS_SETTING: usize = 27;
const CS_COLOR_SPACE: usize = 28;
const CS_SCENE_MODE: usize = 29;
const CS_NOISE_REDUCTION: usize = 30;
const CS_DISTORTION_CORRECTION: usize = 31;
const CS_SHADING_COMPENSATION: usize = 32;
const CS_COMPRESSION_FACTOR: usize = 33;
const CS_GRADATION: usize = 34;
const CS_PICTURE_MODE: usize = 35;
const CS_PICTURE_MODE_SATURATION: usize = 36;
const CS_PICTURE_MODE_CONTRAST: usize = 37;
const CS_PICTURE_MODE_SHARPNESS: usize = 38;
const CS_PICTURE_MODE_BW_FILTER: usize = 39;
const CS_PICTURE_MODE_TONE: usize = 40;
const CS_NOISE_FILTER: usize = 41;
const CS_ART_FILTER: usize = 42;
const CS_MAGIC_FILTER: usize = 43;
const CS_PICTURE_MODE_EFFECT: usize = 44;
const CS_TONE_CURVE: usize = 45;
const CS_TONE_LEVEL: usize = 46;
const CS_SHARPNESS_FACTOR: usize = 47;
const CS_WB_FRB_BRACKET: usize = 48;

// Equipment array indices (tag 0x0201)
const EQ_VERSION: usize = 0;
const EQ_SERIAL_NUMBER: usize = 1;
const EQ_INTERNAL_SERIAL_NUMBER: usize = 2;
const EQ_FOCAL_PLANE_DIAGONAL: usize = 3;
const EQ_BODY_FIRMWARE_VERSION: usize = 4;
const EQ_LENS_TYPE: usize = 5;
const EQ_LENS_SERIAL_NUMBER: usize = 6;
const EQ_LENS_MODEL: usize = 7;
const EQ_LENS_FIRMWARE_VERSION: usize = 8;
const EQ_MAX_APERTURE_AT_MIN_FOCAL: usize = 9;
const EQ_MAX_APERTURE_AT_MAX_FOCAL: usize = 10;
const EQ_MIN_FOCAL_LENGTH: usize = 11;
const EQ_MAX_FOCAL_LENGTH: usize = 12;
const EQ_MAX_APERTURE: usize = 13;
const EQ_LENS_PROPERTIES: usize = 14;
const EQ_EXTENDER: usize = 15;
const EQ_FLASH_TYPE: usize = 16;
const EQ_FLASH_MODEL: usize = 17;
const EQ_FLASH_FIRMWARE_VERSION: usize = 18;
const EQ_FLASH_SERIAL_NUMBER: usize = 19;

/// Decodes Olympus quality mode to human-readable string
fn decode_quality(value: i32) -> &'static str {
    match value {
        1 => "SQ (Standard Quality)",
        2 => "HQ (High Quality)",
        3 => "SHQ (Super High Quality)",
        4 => "RAW",
        5 => "SQ (Low)",
        6 => "SQ (Medium)",
        _ => "Unknown",
    }
}

/// Decodes Olympus exposure mode to human-readable string
fn decode_exposure_mode(value: i32) -> &'static str {
    match value {
        1 => "Manual",
        2 => "Program",
        3 => "Aperture Priority",
        4 => "Shutter Priority",
        5 => "Program Shift",
        _ => "Unknown",
    }
}

/// Decodes Olympus metering mode to human-readable string
fn decode_metering_mode(value: i32) -> &'static str {
    match value {
        2 => "Center Weighted",
        3 => "Spot",
        5 => "ESP (Evaluative)",
        261 => "Pattern+AF",
        515 => "Spot+Highlight Control",
        1027 => "Spot+Shadow Control",
        _ => "Unknown",
    }
}

/// Decodes Olympus focus mode to human-readable string
fn decode_focus_mode(value: i32) -> &'static str {
    match value {
        0 => "Single AF",
        1 => "Sequential Shooting AF",
        2 => "Continuous AF",
        3 => "Manual Focus",
        4 => "Super AF",
        5 => "AF-C",
        10 => "MF",
        _ => "Unknown",
    }
}

/// Decodes Olympus white balance to human-readable string
fn decode_white_balance(value: i32) -> &'static str {
    match value {
        0 => "Auto",
        1 => "Auto (Keep Warm Color Off)",
        16 => "7500K (Fine Weather with Shade)",
        17 => "6000K (Cloudy)",
        18 => "5300K (Fine Weather)",
        20 => "3000K (Tungsten)",
        21 => "3600K (Evening Sunlight)",
        22 => "Auto Setup",
        23 => "5500K (Flash)",
        33 => "6600K (Daylight Fluorescent)",
        34 => "4500K (Neutral White Fluorescent)",
        35 => "4000K (Cool White Fluorescent)",
        36 => "White Fluorescent",
        48 => "3600K (Tungsten)",
        67 => "Underwater",
        256 => "One Touch WB 1",
        257 => "One Touch WB 2",
        258 => "One Touch WB 3",
        259 => "One Touch WB 4",
        512 => "Custom WB 1",
        513 => "Custom WB 2",
        514 => "Custom WB 3",
        515 => "Custom WB 4",
        _ => "Unknown",
    }
}

/// Decodes Olympus flash mode to human-readable string
fn decode_flash_mode(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "On",
        2 => "Fill-in",
        3 => "Red-eye",
        4 => "Slow Sync",
        5 => "Forced On",
        6 => "2nd Curtain",
        _ => "Unknown",
    }
}

/// Decodes Olympus scene mode to human-readable string
fn decode_scene_mode(value: i32) -> &'static str {
    match value {
        0 => "Standard",
        6 => "Auto",
        7 => "Sport",
        8 => "Portrait",
        9 => "Landscape",
        10 => "Night Scene",
        11 => "Self Portrait",
        12 => "Panorama",
        13 => "2 in 1",
        14 => "Movie",
        15 => "Landscape+Portrait",
        16 => "Night+Portrait",
        17 => "Indoor",
        18 => "Fireworks",
        19 => "Sunset",
        20 => "Beauty Skin",
        21 => "Macro",
        22 => "Super Macro",
        23 => "Food",
        24 => "Documents",
        25 => "Museum",
        26 => "Shoot & Select",
        27 => "Beach & Snow",
        28 => "Self Portrait+Self Timer",
        29 => "Candle",
        30 => "Available Light",
        31 => "Behind Glass",
        32 => "My Mode",
        33 => "Pet",
        34 => "Underwater Wide",
        35 => "Underwater Macro",
        36 => "Shoot & Select 1",
        37 => "Shoot & Select 2",
        38 => "Digital Image Stabilization",
        39 => "Face Portrait",
        40 => "Pet Portrait",
        41 => "Smile Shot",
        42 => "Quick Shutter",
        _ => "Unknown",
    }
}

/// Decodes Olympus picture mode to human-readable string
fn decode_picture_mode(value: i32) -> &'static str {
    match value {
        1 => "Vivid",
        2 => "Natural",
        3 => "Muted",
        4 => "Portrait",
        5 => "i-Enhance",
        6 => "Color Creator",
        7 => "Custom",
        8 => "e-Portrait",
        9 => "Color Profile 1",
        10 => "Color Profile 2",
        11 => "Color Profile 3",
        12 => "Monochrome Profile 1",
        13 => "Monochrome Profile 2",
        14 => "Monochrome Profile 3",
        256 => "Monotone",
        512 => "Sepia",
        _ => "Unknown",
    }
}

/// Decodes Olympus art filter to human-readable string
fn decode_art_filter(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "Soft Focus",
        2 => "Pop Art",
        3 => "Pale & Light Color",
        4 => "Light Tone",
        5 => "Pin Hole",
        6 => "Grainy Film",
        9 => "Diorama",
        10 => "Cross Process",
        12 => "Fish Eye",
        13 => "Drawing",
        14 => "Gentle Sepia",
        15 => "Pale & Light Color II",
        16 => "Pop Art II",
        17 => "Pin Hole II",
        18 => "Pin Hole III",
        19 => "Grainy Film II",
        20 => "Dramatic Tone",
        21 => "Punk",
        22 => "Soft Focus 2",
        23 => "Sparkle",
        24 => "Watercolor",
        25 => "Key Line",
        26 => "Key Line II",
        27 => "Miniature",
        28 => "Reflection",
        29 => "Fragmented",
        31 => "Cross Process II",
        32 => "Gentle Sepia II",
        33 => "Dramatic Tone II",
        34 => "Vintage",
        35 => "Vintage II",
        36 => "Vintage III",
        37 => "Partial Color",
        38 => "Partial Color II",
        39 => "Partial Color III",
        _ => "Unknown",
    }
}

/// Decodes Olympus noise reduction to human-readable string
fn decode_noise_reduction(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "Noise Reduction",
        2 => "Noise Filter",
        3 => "Noise Reduction + Noise Filter",
        4 => "Noise Filter (ISO Boost)",
        5 => "Noise Reduction + Noise Filter (ISO Boost)",
        _ => "Unknown",
    }
}

/// Decodes Olympus color space to human-readable string
fn decode_color_space(value: i32) -> &'static str {
    match value {
        0 => "sRGB",
        1 => "Adobe RGB",
        2 => "Pro Photo RGB",
        _ => "Unknown",
    }
}

/// Decodes Olympus macro mode to human-readable string
fn decode_macro_mode(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "On",
        2 => "Super Macro",
        _ => "Unknown",
    }
}

/// Decodes boolean off/on value
fn decode_off_on(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "On",
        _ => "Unknown",
    }
}

/// Represents an Olympus MakerNote parser
pub struct OlympusParser;

impl MakerNoteParser for OlympusParser {
    fn manufacturer_name(&self) -> &'static str {
        "Olympus"
    }

    fn tag_prefix(&self) -> &'static str {
        "Olympus:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Olympus MakerNotes start with "OLYMPUS\0II" or "OLYMPUS\0MM" (10 bytes)
        data.len() >= 10 && (&data[0..10] == OLYMPUS_HEADER || &data[0..10] == OLYMPUS_HEADER_BE)
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

        // Validate Olympus header
        if !self.validate_header(data) {
            return Err("Invalid Olympus MakerNote header".to_string());
        }

        // Olympus MakerNotes structure:
        // - 8 bytes: "OLYMPUS\0"
        // - 2 bytes: byte order marker ("II" or "MM")
        // - 2 bytes: offset to IFD (relative to byte order marker)
        // The IFD offset is relative to position 8 (after "OLYMPUS\0")

        let ifd_offset_pos = 10;
        if data.len() <= ifd_offset_pos + 2 {
            return Ok(());
        }

        // Read IFD offset (2 bytes at position 10-11)
        let ifd_offset = match byte_order {
            ByteOrder::LittleEndian => {
                u16::from_le_bytes([data[ifd_offset_pos], data[ifd_offset_pos + 1]]) as usize
            }
            ByteOrder::BigEndian => {
                u16::from_be_bytes([data[ifd_offset_pos], data[ifd_offset_pos + 1]]) as usize
            }
        };

        // IFD offset is relative to position 8 (after "OLYMPUS\0")
        let abs_ifd_offset = 8 + ifd_offset;

        if data.len() <= abs_ifd_offset + 2 {
            return Ok(());
        }

        let ifd_data = &data[abs_ifd_offset..];

        // Parse IFD entry count
        let entry_count = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([ifd_data[0], ifd_data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([ifd_data[0], ifd_data[1]]),
        };

        // Parse IFD entries
        let entries_start = &ifd_data[2..];
        let entries = match parse_ifd_entries(entries_start, entry_count, byte_order) {
            Ok((_, entries)) => entries,
            Err(_) => return Ok(()), // Return empty on parse failure
        };

        // Extract tags from entries
        for entry in entries {
            match entry.tag_id {
                // Simple numeric tags
                OLYMPUS_JPEG_QUALITY => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Olympus:Quality".to_string(),
                        decode_quality(value).to_string(),
                    );
                }

                OLYMPUS_MACRO_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Olympus:MacroMode".to_string(),
                        decode_macro_mode(value).to_string(),
                    );
                }

                OLYMPUS_DIGITAL_ZOOM => {
                    let value = entry.value_offset as i32;
                    if value != 0 {
                        tags.insert("Olympus:DigitalZoom".to_string(), value.to_string());
                    }
                }

                // Simple string tags
                OLYMPUS_SOFTWARE_RELEASE | OLYMPUS_CAMERA_ID | OLYMPUS_BODY_FIRMWARE_VERSION => {
                    if let Some(value) = extract_string_value(&entry, data, 8) {
                        let tag_name = olympus_tag_to_name(entry.tag_id);
                        tags.insert(tag_name, value);
                    }
                }

                // Camera Settings array (0x0003)
                OLYMPUS_CAMERA_SETTINGS => {
                    if let Some(array) = extract_i32_array(&entry, data, 8, byte_order) {
                        parse_camera_settings(&array, tags);
                    }
                }

                // Equipment array (0x0201)
                OLYMPUS_EQUIPMENT => {
                    if let Some(array) = extract_u8_array(&entry, data, 8) {
                        parse_equipment(&array, tags, byte_order);
                    }
                }

                _ => {
                    // Skip unknown tags or tags we don't parse yet
                }
            }
        }

        Ok(())
    }
}

/// Parses Camera Settings array (tag 0x0003)
fn parse_camera_settings(array: &[i32], tags: &mut HashMap<String, String>) {
    // Exposure mode
    if let Some(&value) = array.get(CS_EXPOSURE_MODE) {
        tags.insert(
            "Olympus:ExposureMode".to_string(),
            decode_exposure_mode(value).to_string(),
        );
    }

    // Metering mode
    if let Some(&value) = array.get(CS_METERING_MODE) {
        tags.insert(
            "Olympus:MeteringMode".to_string(),
            decode_metering_mode(value).to_string(),
        );
    }

    // Focus mode
    if let Some(&value) = array.get(CS_FOCUS_MODE) {
        tags.insert(
            "Olympus:FocusMode".to_string(),
            decode_focus_mode(value).to_string(),
        );
    }

    // White balance
    if let Some(&value) = array.get(CS_WHITE_BALANCE) {
        tags.insert(
            "Olympus:WhiteBalance".to_string(),
            decode_white_balance(value).to_string(),
        );
    }

    // White balance temperature
    if let Some(&value) = array.get(CS_WHITE_BALANCE_TEMPERATURE) {
        if value > 0 {
            tags.insert(
                "Olympus:WhiteBalanceTemperature".to_string(),
                value.to_string(),
            );
        }
    }

    // Flash mode
    if let Some(&value) = array.get(CS_FLASH_MODE) {
        tags.insert(
            "Olympus:FlashMode".to_string(),
            decode_flash_mode(value).to_string(),
        );
    }

    // Flash exposure compensation
    if let Some(&value) = array.get(CS_FLASH_EXPOSURE_COMP) {
        if value != 0 {
            // Value is in 1/3 EV steps
            let ev = value as f32 / 3.0;
            tags.insert(
                "Olympus:FlashExposureComp".to_string(),
                format!("{:.2}", ev),
            );
        }
    }

    // Contrast setting
    if let Some(&value) = array.get(CS_CONTRAST_SETTING) {
        if value != 0 {
            tags.insert("Olympus:Contrast".to_string(), value.to_string());
        }
    }

    // Sharpness setting
    if let Some(&value) = array.get(CS_SHARPNESS_SETTING) {
        if value != 0 {
            tags.insert("Olympus:Sharpness".to_string(), value.to_string());
        }
    }

    // Saturation
    if let Some(&value) = array.get(CS_CUSTOM_SATURATION) {
        if value != 0 {
            tags.insert("Olympus:Saturation".to_string(), value.to_string());
        }
    }

    // Color space
    if let Some(&value) = array.get(CS_COLOR_SPACE) {
        tags.insert(
            "Olympus:ColorSpace".to_string(),
            decode_color_space(value).to_string(),
        );
    }

    // Scene mode
    if let Some(&value) = array.get(CS_SCENE_MODE) {
        tags.insert(
            "Olympus:SceneMode".to_string(),
            decode_scene_mode(value).to_string(),
        );
    }

    // Noise reduction
    if let Some(&value) = array.get(CS_NOISE_REDUCTION) {
        tags.insert(
            "Olympus:NoiseReduction".to_string(),
            decode_noise_reduction(value).to_string(),
        );
    }

    // Picture mode
    if let Some(&value) = array.get(CS_PICTURE_MODE) {
        tags.insert(
            "Olympus:PictureMode".to_string(),
            decode_picture_mode(value).to_string(),
        );
    }

    // Art filter
    if let Some(&value) = array.get(CS_ART_FILTER) {
        if value != 0 {
            tags.insert(
                "Olympus:ArtFilter".to_string(),
                decode_art_filter(value).to_string(),
            );
        }
    }

    // Distortion correction
    if let Some(&value) = array.get(CS_DISTORTION_CORRECTION) {
        tags.insert(
            "Olympus:DistortionCorrection".to_string(),
            decode_off_on(value).to_string(),
        );
    }

    // Shading compensation
    if let Some(&value) = array.get(CS_SHADING_COMPENSATION) {
        tags.insert(
            "Olympus:ShadingCompensation".to_string(),
            decode_off_on(value).to_string(),
        );
    }
}

/// Parses Equipment array (tag 0x0201)
fn parse_equipment(array: &[u8], tags: &mut HashMap<String, String>, byte_order: ByteOrder) {
    // Equipment array is complex - contains version, serial numbers, lens info, flash info

    // Serial number (8 bytes starting at offset 2)
    if array.len() >= 10 {
        let serial_bytes = &array[2..10];
        if let Ok(serial) = std::str::from_utf8(serial_bytes) {
            let serial_str = serial.trim_end_matches('\0').trim();
            if !serial_str.is_empty() {
                tags.insert("Olympus:SerialNumber".to_string(), serial_str.to_string());
            }
        }
    }

    // Body firmware version (5 bytes starting at offset 10)
    if array.len() >= 15 {
        let fw_bytes = &array[10..15];
        if let Ok(fw) = std::str::from_utf8(fw_bytes) {
            let fw_str = fw.trim_end_matches('\0').trim();
            if !fw_str.is_empty() {
                tags.insert(
                    "Olympus:BodyFirmwareVersion".to_string(),
                    fw_str.to_string(),
                );
            }
        }
    }

    // Lens type (2 bytes at offset 16)
    if array.len() >= 18 {
        let lens_id = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([array[16], array[17]]),
            ByteOrder::BigEndian => u16::from_be_bytes([array[16], array[17]]),
        };

        if lens_id != 0 {
            if let Some(lens_name) = lookup_lens_name(lens_id) {
                tags.insert("Olympus:LensType".to_string(), lens_name);
            } else {
                tags.insert("Olympus:LensID".to_string(), lens_id.to_string());
            }
        }
    }

    // Lens serial number (at offset 18, varies by length)
    if array.len() >= 26 {
        let lens_serial_bytes = &array[18..26];
        if let Ok(lens_serial) = std::str::from_utf8(lens_serial_bytes) {
            let lens_serial_str = lens_serial.trim_end_matches('\0').trim();
            if !lens_serial_str.is_empty() {
                tags.insert(
                    "Olympus:LensSerialNumber".to_string(),
                    lens_serial_str.to_string(),
                );
            }
        }
    }

    // Min focal length (2 bytes at offset 56)
    if array.len() >= 58 {
        let min_focal = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([array[56], array[57]]),
            ByteOrder::BigEndian => u16::from_be_bytes([array[56], array[57]]),
        };
        if min_focal > 0 {
            tags.insert(
                "Olympus:MinFocalLength".to_string(),
                format!("{} mm", min_focal),
            );
        }
    }

    // Max focal length (2 bytes at offset 58)
    if array.len() >= 60 {
        let max_focal = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([array[58], array[59]]),
            ByteOrder::BigEndian => u16::from_be_bytes([array[58], array[59]]),
        };
        if max_focal > 0 {
            tags.insert(
                "Olympus:MaxFocalLength".to_string(),
                format!("{} mm", max_focal),
            );
        }
    }

    // Max aperture at min focal (2 bytes at offset 52)
    if array.len() >= 54 {
        let max_ap_min = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([array[52], array[53]]),
            ByteOrder::BigEndian => u16::from_be_bytes([array[52], array[53]]),
        };
        if max_ap_min > 0 {
            let f_stop = (max_ap_min as f32) / 10.0;
            tags.insert(
                "Olympus:MaxApertureAtMinFocal".to_string(),
                format!("f/{:.1}", f_stop),
            );
        }
    }

    // Max aperture at max focal (2 bytes at offset 54)
    if array.len() >= 56 {
        let max_ap_max = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([array[54], array[55]]),
            ByteOrder::BigEndian => u16::from_be_bytes([array[54], array[55]]),
        };
        if max_ap_max > 0 {
            let f_stop = (max_ap_max as f32) / 10.0;
            tags.insert(
                "Olympus:MaxApertureAtMaxFocal".to_string(),
                format!("f/{:.1}", f_stop),
            );
        }
    }
}

/// Converts Olympus tag ID to tag name string
fn olympus_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        OLYMPUS_SPECIAL_MODE => "SpecialMode",
        OLYMPUS_JPEG_QUALITY => "Quality",
        OLYMPUS_MACRO_MODE => "MacroMode",
        OLYMPUS_DIGITAL_ZOOM => "DigitalZoom",
        OLYMPUS_SOFTWARE_RELEASE => "SoftwareRelease",
        OLYMPUS_CAMERA_ID => "CameraID",
        OLYMPUS_IMAGE_WIDTH => "ImageWidth",
        OLYMPUS_IMAGE_HEIGHT => "ImageHeight",
        OLYMPUS_BODY_FIRMWARE_VERSION => "BodyFirmwareVersion",
        OLYMPUS_LENS_MODEL => "LensModel",
        _ => return format!("Olympus:Unknown-{:#06X}", tag_id),
    };

    format!("Olympus:{}", tag_name)
}

/// Parses IFD entries in the specified byte order
fn parse_ifd_entries(
    input: &[u8],
    entry_count: u16,
    byte_order: ByteOrder,
) -> IResult<&[u8], Vec<IfdEntry>> {
    use nom::Parser;
    match byte_order {
        ByteOrder::LittleEndian => count(parse_ifd_entry_le, entry_count as usize).parse(input),
        ByteOrder::BigEndian => count(parse_ifd_entry_be, entry_count as usize).parse(input),
    }
}

/// Parses a single IFD entry in little-endian byte order
fn parse_ifd_entry_le(input: &[u8]) -> IResult<&[u8], IfdEntry> {
    use nom::Parser;
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
    ).parse(input)
}

/// Parses a single IFD entry in big-endian byte order
fn parse_ifd_entry_be(input: &[u8]) -> IResult<&[u8], IfdEntry> {
    use nom::Parser;
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
    ).parse(input)
}

/// Extracts string value from IFD entry
fn extract_string_value(entry: &IfdEntry, full_data: &[u8], base_offset: usize) -> Option<String> {
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
    let offset = (entry.value_offset as usize) + base_offset;

    if offset + byte_count <= full_data.len() {
        let bytes = &full_data[offset..offset + byte_count];
        let s = std::str::from_utf8(bytes)
            .ok()?
            .trim_end_matches('\0')
            .trim();
        Some(s.to_string())
    } else {
        None
    }
}

/// Extracts i32 array from IFD entry
fn extract_i32_array(
    entry: &IfdEntry,
    full_data: &[u8],
    base_offset: usize,
    byte_order: ByteOrder,
) -> Option<Vec<i32>> {
    let count = entry.value_count as usize;
    let offset = (entry.value_offset as usize) + base_offset;

    // Each i32 is 4 bytes
    if offset + (count * 4) > full_data.len() {
        return None;
    }

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let pos = offset + (i * 4);
        let value = match byte_order {
            ByteOrder::LittleEndian => i32::from_le_bytes([
                full_data[pos],
                full_data[pos + 1],
                full_data[pos + 2],
                full_data[pos + 3],
            ]),
            ByteOrder::BigEndian => i32::from_be_bytes([
                full_data[pos],
                full_data[pos + 1],
                full_data[pos + 2],
                full_data[pos + 3],
            ]),
        };
        result.push(value);
    }

    Some(result)
}

/// Extracts u8 array from IFD entry
fn extract_u8_array(entry: &IfdEntry, full_data: &[u8], base_offset: usize) -> Option<Vec<u8>> {
    let count = entry.value_count as usize;
    let offset = (entry.value_offset as usize) + base_offset;

    if offset + count > full_data.len() {
        return None;
    }

    Some(full_data[offset..offset + count].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_manufacturer_name() {
        let parser = OlympusParser;
        assert_eq!(parser.manufacturer_name(), "Olympus");
    }

    #[test]
    fn test_parser_tag_prefix() {
        let parser = OlympusParser;
        assert_eq!(parser.tag_prefix(), "Olympus:");
    }

    #[test]
    fn test_validate_header_valid_le() {
        let parser = OlympusParser;
        let header = b"OLYMPUS\0II\x03\x00";
        assert!(parser.validate_header(header));
    }

    #[test]
    fn test_validate_header_valid_be() {
        let parser = OlympusParser;
        let header = b"OLYMPUS\0MM\x00\x03";
        assert!(parser.validate_header(header));
    }

    #[test]
    fn test_validate_header_invalid() {
        let parser = OlympusParser;
        let header = b"NIKON\0\0\0";
        assert!(!parser.validate_header(header));
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(decode_quality(1), "SQ (Standard Quality)");
        assert_eq!(decode_quality(2), "HQ (High Quality)");
        assert_eq!(decode_quality(3), "SHQ (Super High Quality)");
        assert_eq!(decode_quality(4), "RAW");
    }

    #[test]
    fn test_decode_exposure_mode() {
        assert_eq!(decode_exposure_mode(1), "Manual");
        assert_eq!(decode_exposure_mode(2), "Program");
        assert_eq!(decode_exposure_mode(3), "Aperture Priority");
        assert_eq!(decode_exposure_mode(4), "Shutter Priority");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(decode_focus_mode(0), "Single AF");
        assert_eq!(decode_focus_mode(2), "Continuous AF");
        assert_eq!(decode_focus_mode(3), "Manual Focus");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(decode_white_balance(0), "Auto");
        assert_eq!(decode_white_balance(18), "5300K (Fine Weather)");
        assert_eq!(decode_white_balance(23), "5500K (Flash)");
    }

    #[test]
    fn test_decode_scene_mode() {
        assert_eq!(decode_scene_mode(0), "Standard");
        assert_eq!(decode_scene_mode(8), "Portrait");
        assert_eq!(decode_scene_mode(9), "Landscape");
        assert_eq!(decode_scene_mode(21), "Macro");
        assert_eq!(decode_scene_mode(22), "Super Macro");
    }

    #[test]
    fn test_decode_picture_mode() {
        assert_eq!(decode_picture_mode(1), "Vivid");
        assert_eq!(decode_picture_mode(2), "Natural");
        assert_eq!(decode_picture_mode(5), "i-Enhance");
    }

    #[test]
    fn test_decode_art_filter() {
        assert_eq!(decode_art_filter(0), "Off");
        assert_eq!(decode_art_filter(2), "Pop Art");
        assert_eq!(decode_art_filter(9), "Diorama");
        assert_eq!(decode_art_filter(24), "Watercolor");
    }

    #[test]
    fn test_lens_lookup() {
        let parser = OlympusParser;
        assert_eq!(
            parser.lookup_lens(48),
            Some("M.Zuiko Digital ED 12-40mm f/2.8 PRO".to_string())
        );
    }
}
