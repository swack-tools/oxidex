//! Fujifilm MakerNote Parser
//!
//! Parses Fujifilm-specific EXIF MakerNote tags containing camera settings,
//! lens information, film simulation modes, and other proprietary metadata.
//!
//! Supports both X-series mirrorless cameras and GFX medium format cameras.
//!
//! Based on ExifTool's Fujifilm.pm module.

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

use super::fujifilm_lens_database::lookup_lens_name;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use super::shared::MakerNoteParser;

// ===== Fujifilm MakerNote Tag IDs =====
// Based on ExifTool Fujifilm.pm tag definitions

// Basic Camera Information Tags
const FUJI_VERSION: u16 = 0x0000;
const FUJI_SERIAL_NUMBER: u16 = 0x0010;
const FUJI_QUALITY: u16 = 0x1000;
const FUJI_SHARPNESS: u16 = 0x1001;
const FUJI_WHITE_BALANCE: u16 = 0x1002;
const FUJI_SATURATION: u16 = 0x1003;
const FUJI_CONTRAST: u16 = 0x1004;
const FUJI_COLOR_TEMPERATURE: u16 = 0x1005;
const FUJI_CONTRAST_DETECTION_AF: u16 = 0x1006;
const FUJI_FLASH_MODE: u16 = 0x1010;
const FUJI_FLASH_EV: u16 = 0x1011;
const FUJI_MACRO: u16 = 0x1020;
const FUJI_FOCUS_MODE: u16 = 0x1021;
const FUJI_FOCUS_PIXEL: u16 = 0x1023;
const FUJI_SLOW_SYNC: u16 = 0x1030;
const FUJI_PICTURE_MODE: u16 = 0x1031;
const FUJI_EXR_AUTO: u16 = 0x1033;
const FUJI_EXR_MODE: u16 = 0x1034;
const FUJI_SHADOW_TONE: u16 = 0x1040;
const FUJI_HIGHLIGHT_TONE: u16 = 0x1041;
const FUJI_DIGITAL_ZOOM: u16 = 0x1044;
const FUJI_LENS_MODEL_NAME: u16 = 0x1050;

// Film Simulation and Color Tags
const FUJI_FILM_MODE: u16 = 0x1401;
const FUJI_DYNAMIC_RANGE: u16 = 0x1402;
const FUJI_DYNAMIC_RANGE_SETTING: u16 = 0x1403;
const FUJI_DEVELOPMENT_DYNAMIC_RANGE: u16 = 0x1404;
const FUJI_MIN_FOCAL_LENGTH: u16 = 0x1405;
const FUJI_MAX_FOCAL_LENGTH: u16 = 0x1406;
const FUJI_MAX_APERTURE_AT_MIN_FOCAL: u16 = 0x1407;
const FUJI_MAX_APERTURE_AT_MAX_FOCAL: u16 = 0x1408;

// Advanced Camera Settings
const FUJI_AUTO_DYNAMIC_RANGE: u16 = 0x140B;
const FUJI_FACES_DETECTED: u16 = 0x4100;
const FUJI_FACE_POSITIONS: u16 = 0x4103;
const FUJI_FACE_REC_INFO: u16 = 0x4282;
const FUJI_SHUTTER_TYPE: u16 = 0x1100;
const FUJI_BURST_MODE: u16 = 0x1101;
const FUJI_SEQUENCE_NUMBER: u16 = 0x1103;
const FUJI_BLUR_WARNING: u16 = 0x1300;
const FUJI_FOCUS_WARNING: u16 = 0x1301;
const FUJI_EXPOSURE_WARNING: u16 = 0x1302;
const FUJI_DYNAMIC_RANGE_WARNING: u16 = 0x1304;

// RAF (RAW) Image Tags
const FUJI_RAW_IMAGE_FULL_SIZE: u16 = 0xF000;
const FUJI_RAW_IMAGE_FULL_WIDTH: u16 = 0xF001;
const FUJI_RAW_IMAGE_FULL_HEIGHT: u16 = 0xF002;
const FUJI_RAW_IMAGE_ASPECT_RATIO: u16 = 0xF003;

// File and Image Information
const FUJI_FILE_SOURCE: u16 = 0x8000;
const FUJI_ORDER_NUMBER: u16 = 0x8002;
const FUJI_FRAME_NUMBER: u16 = 0x8003;
const FUJI_PARALLAX: u16 = 0xB211;

// Advanced Features
const FUJI_IMAGE_GENERATION: u16 = 0x1047;
const FUJI_RATING: u16 = 0x1431;
const FUJI_IMAGE_COUNT: u16 = 0x1438;
const FUJI_DRIVE_MODE: u16 = 0x1039;
const FUJI_PIXEL_SHIFT_OFFSET: u16 = 0x9650;

// Fujifilm MakerNote header signature
// Fujifilm uses "FUJIFILM" followed by IFD offset
const FUJIFILM_HEADER: &[u8] = b"FUJIFILM";

/// Decodes Fujifilm quality setting to human-readable string
fn decode_quality(value: i32) -> &'static str {
    match value {
        1 => "F (Fine)",
        2 => "N (Normal)",
        3 => "Fine",
        4 => "Normal",
        5 => "Fine+RAW",
        6 => "Normal+RAW",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm white balance setting to human-readable string
fn decode_white_balance(value: i32) -> &'static str {
    match value {
        0x0000 => "Auto",
        0x0001 => "Auto (White Priority)",
        0x0002 => "Auto (Ambience Priority)",
        0x0100 => "Daylight",
        0x0200 => "Cloudy",
        0x0300 => "Daylight Fluorescent",
        0x0301 => "Day White Fluorescent",
        0x0302 => "White Fluorescent",
        0x0303 => "Warm White Fluorescent",
        0x0304 => "Living Room Warm White Fluorescent",
        0x0400 => "Incandescent",
        0x0500 => "Flash",
        0x0600 => "Underwater",
        0x0F00 => "Custom",
        0x0F01 => "Custom2",
        0x0F02 => "Custom3",
        0x0F03 => "Custom4",
        0x0F04 => "Custom5",
        0x0FF0 => "Kelvin",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm focus mode to human-readable string
fn decode_focus_mode(value: i32) -> &'static str {
    match value {
        0 => "Auto",
        1 => "Manual",
        2 => "AF-S (Single)",
        3 => "AF-C (Continuous)",
        4 => "AF-A (Automatic)",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm flash mode to human-readable string
fn decode_flash_mode(value: i32) -> &'static str {
    match value {
        0 => "Auto",
        1 => "On",
        2 => "Off",
        3 => "Red-eye Reduction",
        4 => "External",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm film simulation mode to human-readable string
fn decode_film_mode(value: i32) -> &'static str {
    match value {
        0x0000 => "F0/Standard (Provia)",
        0x0100 => "F1/Studio Portrait",
        0x0110 => "F1a/Studio Portrait Enhanced Saturation",
        0x0120 => "F1b/Studio Portrait Smooth Skin Tone",
        0x0130 => "F1c/Studio Portrait Increased Sharpness",
        0x0200 => "F2/Fujichrome (Velvia)",
        0x0300 => "F3/Studio Portrait Ex",
        0x0400 => "F4/Velvia",
        0x0500 => "Pro Neg. Std",
        0x0501 => "Pro Neg. Hi",
        0x0600 => "Classic Chrome",
        0x0700 => "Eterna",
        0x0800 => "Classic Negative",
        0x0900 => "Bleach Bypass",
        0x0A00 => "Nostalgic Neg.",
        0x0B00 => "Eterna Bleach Bypass",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm dynamic range setting to human-readable string
fn decode_dynamic_range(value: i32) -> &'static str {
    match value {
        1 => "Standard (100%)",
        2 => "Wide 1 (230%)",
        3 => "Wide 2 (400%)",
        4 => "Auto",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm shutter type to human-readable string
fn decode_shutter_type(value: i32) -> &'static str {
    match value {
        0 => "Mechanical",
        1 => "Electronic",
        2 => "Electronic (Silent)",
        3 => "Mechanical + Electronic",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm burst mode to human-readable string
fn decode_burst_mode(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "On (Low Speed)",
        2 => "On (High Speed)",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm picture mode to human-readable string
fn decode_picture_mode(value: i32) -> &'static str {
    match value {
        0x0000 => "Auto",
        0x0001 => "Portrait",
        0x0002 => "Landscape",
        0x0003 => "Macro",
        0x0004 => "Sports",
        0x0005 => "Night Scene",
        0x0006 => "Program AE",
        0x0007 => "Aperture Priority AE",
        0x0008 => "Shutter Priority AE",
        0x0009 => "Manual",
        0x000A => "Portrait Enhancer",
        0x000B => "Natural Light",
        0x000D => "Beach",
        0x000E => "Snow",
        0x000F => "Fireworks",
        0x0010 => "Underwater",
        0x0011 => "Museum",
        0x0012 => "Party",
        0x0013 => "Flower",
        0x0014 => "Text",
        0x0018 => "Sunset",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm drive mode to human-readable string
fn decode_drive_mode(value: i32) -> &'static str {
    match value {
        0 => "Single Frame",
        1 => "Continuous Low",
        2 => "Continuous High",
        3 => "Bracketing",
        4 => "Self-timer",
        5 => "Remote",
        6 => "Interval Timer",
        _ => "Unknown",
    }
}

/// Decodes Fujifilm EXR mode to human-readable string
fn decode_exr_mode(value: i32) -> &'static str {
    match value {
        256 => "HR (High Resolution)",
        512 => "SN (Signal-to-Noise Priority)",
        768 => "DR (Dynamic Range Priority)",
        _ => "Unknown",
    }
}

/// Decodes boolean/off-on value to human-readable string
fn decode_off_on(value: i32) -> &'static str {
    match value {
        0 => "Off",
        1 => "On",
        _ => "Unknown",
    }
}

/// Represents a Fujifilm MakerNote parser
pub struct FujifilmParser;

impl MakerNoteParser for FujifilmParser {
    fn manufacturer_name(&self) -> &'static str {
        "Fujifilm"
    }

    fn tag_prefix(&self) -> &'static str {
        "Fujifilm:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Fujifilm MakerNotes start with "FUJIFILM" (8 bytes) followed by offset
        data.len() >= 12 && &data[0..8] == FUJIFILM_HEADER
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

        // Validate Fujifilm header
        if !self.validate_header(data) {
            return Err("Invalid Fujifilm MakerNote header".to_string());
        }

        // Fujifilm header structure:
        // - Bytes 0-7: "FUJIFILM" signature
        // - Bytes 8-11: IFD offset (4 bytes, always 0x0000000C = 12)
        // - Byte 12+: IFD data starts

        // Read IFD offset (should be 12, but we'll use it anyway)
        let ifd_offset = match byte_order {
            ByteOrder::LittleEndian => {
                u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize
            }
            ByteOrder::BigEndian => {
                u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize
            }
        };

        // Fujifilm offsets are relative to the MakerNote start
        if ifd_offset >= data.len() {
            return Ok(());
        }

        let ifd_data = &data[ifd_offset..];

        // Parse IFD entry count
        if ifd_data.len() < 2 {
            return Ok(());
        }

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
                // String tags
                FUJI_VERSION | FUJI_SERIAL_NUMBER | FUJI_LENS_MODEL_NAME => {
                    if let Some(value) = extract_string_value(&entry, data) {
                        let tag_name = fujifilm_tag_to_name(entry.tag_id);
                        tags.insert(tag_name, value);
                    }
                }

                // Simple integer tags
                FUJI_SEQUENCE_NUMBER | FUJI_FRAME_NUMBER | FUJI_IMAGE_COUNT | FUJI_RATING => {
                    let value = entry.value_offset;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, value.to_string());
                }

                // Enumerated value tags with decoders
                FUJI_QUALITY => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:Quality".to_string(),
                        decode_quality(value).to_string(),
                    );
                }

                FUJI_WHITE_BALANCE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:WhiteBalance".to_string(),
                        decode_white_balance(value).to_string(),
                    );
                }

                FUJI_FOCUS_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:FocusMode".to_string(),
                        decode_focus_mode(value).to_string(),
                    );
                }

                FUJI_FLASH_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:FlashMode".to_string(),
                        decode_flash_mode(value).to_string(),
                    );
                }

                FUJI_FILM_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:FilmMode".to_string(),
                        decode_film_mode(value).to_string(),
                    );
                }

                FUJI_DYNAMIC_RANGE | FUJI_DYNAMIC_RANGE_SETTING => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:DynamicRange".to_string(),
                        decode_dynamic_range(value).to_string(),
                    );
                }

                FUJI_SHUTTER_TYPE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:ShutterType".to_string(),
                        decode_shutter_type(value).to_string(),
                    );
                }

                FUJI_BURST_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:BurstMode".to_string(),
                        decode_burst_mode(value).to_string(),
                    );
                }

                FUJI_PICTURE_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:PictureMode".to_string(),
                        decode_picture_mode(value).to_string(),
                    );
                }

                FUJI_DRIVE_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:DriveMode".to_string(),
                        decode_drive_mode(value).to_string(),
                    );
                }

                FUJI_EXR_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "Fujifilm:EXRMode".to_string(),
                        decode_exr_mode(value).to_string(),
                    );
                }

                // Simple numeric tags with units
                FUJI_SHARPNESS | FUJI_SATURATION | FUJI_CONTRAST => {
                    let value = entry.value_offset as i32;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    // Fujifilm uses scale: 0=normal, +values=more, -values=less
                    let description = match value {
                        v if v < 0 => format!("{} (Soft)", v),
                        0 => "0 (Normal)".to_string(),
                        v => format!("{} (Hard)", v),
                    };
                    tags.insert(tag_name, description);
                }

                FUJI_SHADOW_TONE | FUJI_HIGHLIGHT_TONE => {
                    let value = entry.value_offset as i32;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    // Shadow/Highlight tone: -64 to +64 (0 = standard)
                    tags.insert(tag_name, format!("{:+}", value));
                }

                FUJI_COLOR_TEMPERATURE => {
                    let value = entry.value_offset;
                    if value > 0 {
                        tags.insert(
                            "Fujifilm:ColorTemperature".to_string(),
                            format!("{} K", value),
                        );
                    }
                }

                FUJI_FACES_DETECTED => {
                    let value = entry.value_offset;
                    tags.insert("Fujifilm:FacesDetected".to_string(), value.to_string());
                }

                // Boolean/On-Off tags
                FUJI_MACRO | FUJI_SLOW_SYNC | FUJI_EXR_AUTO | FUJI_AUTO_DYNAMIC_RANGE => {
                    let value = entry.value_offset as i32;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, decode_off_on(value).to_string());
                }

                // Warning flags
                FUJI_BLUR_WARNING
                | FUJI_FOCUS_WARNING
                | FUJI_EXPOSURE_WARNING
                | FUJI_DYNAMIC_RANGE_WARNING => {
                    let value = entry.value_offset as i32;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    let warning = if value == 0 { "None" } else { "Warning" };
                    tags.insert(tag_name, warning.to_string());
                }

                // Lens focal length information
                FUJI_MIN_FOCAL_LENGTH | FUJI_MAX_FOCAL_LENGTH => {
                    let value = entry.value_offset as f32 / 10.0; // Stored in 0.1mm units
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, format!("{:.1} mm", value));
                }

                FUJI_MAX_APERTURE_AT_MIN_FOCAL | FUJI_MAX_APERTURE_AT_MAX_FOCAL => {
                    let value = entry.value_offset as f32 / 100.0; // Stored in 0.01 units
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, format!("f/{:.1}", value));
                }

                // RAW image dimensions
                FUJI_RAW_IMAGE_FULL_WIDTH | FUJI_RAW_IMAGE_FULL_HEIGHT => {
                    let value = entry.value_offset;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, format!("{} px", value));
                }

                // Digital zoom
                FUJI_DIGITAL_ZOOM => {
                    let value = entry.value_offset as f32 / 100.0; // Stored as percentage
                    tags.insert("Fujifilm:DigitalZoom".to_string(), format!("{:.2}x", value));
                }

                // Flash exposure compensation
                FUJI_FLASH_EV => {
                    // Flash EV stored as signed value in units of 1/3 EV
                    let raw_value = entry.value_offset as i32;
                    let ev_value = raw_value as f32 / 3.0;
                    tags.insert(
                        "Fujifilm:FlashExposureComp".to_string(),
                        format!("{:+.1} EV", ev_value),
                    );
                }

                // Focus pixel coordinates (array)
                FUJI_FOCUS_PIXEL => {
                    if let Some(array) = extract_u16_array(&entry, data, byte_order) {
                        if array.len() >= 2 {
                            tags.insert(
                                "Fujifilm:FocusPixel".to_string(),
                                format!("X:{} Y:{}", array[0], array[1]),
                            );
                        }
                    }
                }

                // Face positions (array) - complex structure, basic extraction
                FUJI_FACE_POSITIONS => {
                    if let Some(array) = extract_u16_array(&entry, data, byte_order) {
                        if !array.is_empty() {
                            tags.insert(
                                "Fujifilm:FacePositions".to_string(),
                                format!("{} coordinates", array.len() / 4),
                            );
                        }
                    }
                }

                // Other tags - skip for now
                _ => continue,
            }
        }

        Ok(())
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }
}

/// Maps Fujifilm MakerNote tag IDs to human-readable tag names
fn fujifilm_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        FUJI_VERSION => "Version",
        FUJI_SERIAL_NUMBER => "SerialNumber",
        FUJI_QUALITY => "Quality",
        FUJI_SHARPNESS => "Sharpness",
        FUJI_WHITE_BALANCE => "WhiteBalance",
        FUJI_SATURATION => "Saturation",
        FUJI_CONTRAST => "Contrast",
        FUJI_COLOR_TEMPERATURE => "ColorTemperature",
        FUJI_FLASH_MODE => "FlashMode",
        FUJI_FLASH_EV => "FlashExposureComp",
        FUJI_MACRO => "Macro",
        FUJI_FOCUS_MODE => "FocusMode",
        FUJI_FOCUS_PIXEL => "FocusPixel",
        FUJI_SLOW_SYNC => "SlowSync",
        FUJI_PICTURE_MODE => "PictureMode",
        FUJI_EXR_AUTO => "EXRAuto",
        FUJI_EXR_MODE => "EXRMode",
        FUJI_SHADOW_TONE => "ShadowTone",
        FUJI_HIGHLIGHT_TONE => "HighlightTone",
        FUJI_DIGITAL_ZOOM => "DigitalZoom",
        FUJI_LENS_MODEL_NAME => "LensModelName",
        FUJI_FILM_MODE => "FilmMode",
        FUJI_DYNAMIC_RANGE => "DynamicRange",
        FUJI_DYNAMIC_RANGE_SETTING => "DynamicRangeSetting",
        FUJI_MIN_FOCAL_LENGTH => "MinFocalLength",
        FUJI_MAX_FOCAL_LENGTH => "MaxFocalLength",
        FUJI_MAX_APERTURE_AT_MIN_FOCAL => "MaxApertureAtMinFocal",
        FUJI_MAX_APERTURE_AT_MAX_FOCAL => "MaxApertureAtMaxFocal",
        FUJI_AUTO_DYNAMIC_RANGE => "AutoDynamicRange",
        FUJI_FACES_DETECTED => "FacesDetected",
        FUJI_FACE_POSITIONS => "FacePositions",
        FUJI_SHUTTER_TYPE => "ShutterType",
        FUJI_BURST_MODE => "BurstMode",
        FUJI_SEQUENCE_NUMBER => "SequenceNumber",
        FUJI_BLUR_WARNING => "BlurWarning",
        FUJI_FOCUS_WARNING => "FocusWarning",
        FUJI_EXPOSURE_WARNING => "ExposureWarning",
        FUJI_DYNAMIC_RANGE_WARNING => "DynamicRangeWarning",
        FUJI_RAW_IMAGE_FULL_WIDTH => "RawImageFullWidth",
        FUJI_RAW_IMAGE_FULL_HEIGHT => "RawImageFullHeight",
        FUJI_FRAME_NUMBER => "FrameNumber",
        FUJI_IMAGE_COUNT => "ImageCount",
        FUJI_DRIVE_MODE => "DriveMode",
        FUJI_RATING => "Rating",
        _ => return format!("Fujifilm:Unknown-{:#06X}", tag_id),
    };

    format!("Fujifilm:{}", tag_name)
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
///
/// Handles both inline strings (≤4 bytes) and offset-based strings
fn extract_string_value(entry: &IfdEntry, full_data: &[u8]) -> Option<String> {
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
    // Fujifilm offsets are relative to MakerNote start
    let offset = entry.value_offset as usize;

    if offset + byte_count <= full_data.len() {
        let bytes = &full_data[offset..offset + byte_count];
        let s = std::str::from_utf8(bytes)
            .ok()?
            .trim_end_matches('\0')
            .trim();
        return Some(s.to_string());
    }

    None
}

/// Public function to parse Fujifilm MakerNotes
///
/// This is the main entry point for parsing Fujifilm MakerNote data.
///
/// # Parameters
/// - `data`: Raw MakerNote data (including Fujifilm header)
/// - `byte_order`: Byte order for parsing multi-byte values
/// - `tags`: HashMap to populate with extracted tags
pub fn parse_fujifilm_makernotes(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = FujifilmParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("Fujifilm MakerNotes parse error: {}", e);
    }
}

/// Checks if data appears to be a Fujifilm MakerNote
///
/// # Parameters
/// - `data`: Raw byte data to check
///
/// # Returns
/// `true` if the data appears to be a Fujifilm MakerNote, `false` otherwise
pub fn is_fujifilm_makernote(data: &[u8]) -> bool {
    data.len() >= 12 && &data[0..8] == FUJIFILM_HEADER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fujifilm_tag_ids() {
        assert_eq!(FUJI_VERSION, 0x0000);
        assert_eq!(FUJI_QUALITY, 0x1000);
        assert_eq!(FUJI_WHITE_BALANCE, 0x1002);
        assert_eq!(FUJI_FILM_MODE, 0x1401);
        assert_eq!(FUJI_SHUTTER_TYPE, 0x1100);
    }

    #[test]
    fn test_fujifilm_header_validation() {
        let parser = FujifilmParser;

        // Valid Fujifilm header
        let valid_header = b"FUJIFILM\x0C\x00\x00\x00extra data";
        assert!(parser.validate_header(valid_header));

        // Invalid header (wrong signature)
        let invalid = b"CANON\0\x00\x00\x00\x00\x00\x00";
        assert!(!parser.validate_header(invalid));

        // Too short
        let too_short = b"FUJIFILM\x0C";
        assert!(!parser.validate_header(too_short));
    }

    #[test]
    fn test_is_fujifilm_makernote() {
        assert!(is_fujifilm_makernote(b"FUJIFILM\x0C\x00\x00\x00test"));
        assert!(!is_fujifilm_makernote(b"NIKON\0\x00\x00"));
        assert!(!is_fujifilm_makernote(b"FUJIFILM\x0C")); // Too short
    }

    #[test]
    fn test_fujifilm_tag_to_name() {
        assert_eq!(fujifilm_tag_to_name(0x0000), "Fujifilm:Version");
        assert_eq!(fujifilm_tag_to_name(0x1000), "Fujifilm:Quality");
        assert_eq!(fujifilm_tag_to_name(0x1002), "Fujifilm:WhiteBalance");
        assert_eq!(fujifilm_tag_to_name(0x1401), "Fujifilm:FilmMode");
        assert_eq!(fujifilm_tag_to_name(0xFFFF), "Fujifilm:Unknown-0xFFFF");
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(decode_quality(1), "F (Fine)");
        assert_eq!(decode_quality(3), "Fine");
        assert_eq!(decode_quality(5), "Fine+RAW");
        assert_eq!(decode_quality(99), "Unknown");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(decode_white_balance(0x0000), "Auto");
        assert_eq!(decode_white_balance(0x0100), "Daylight");
        assert_eq!(decode_white_balance(0x0200), "Cloudy");
        assert_eq!(decode_white_balance(0x0400), "Incandescent");
        assert_eq!(decode_white_balance(0x9999), "Unknown");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(decode_focus_mode(0), "Auto");
        assert_eq!(decode_focus_mode(1), "Manual");
        assert_eq!(decode_focus_mode(2), "AF-S (Single)");
        assert_eq!(decode_focus_mode(3), "AF-C (Continuous)");
        assert_eq!(decode_focus_mode(99), "Unknown");
    }

    #[test]
    fn test_decode_film_mode() {
        assert_eq!(decode_film_mode(0x0000), "F0/Standard (Provia)");
        assert_eq!(decode_film_mode(0x0200), "F2/Fujichrome (Velvia)");
        assert_eq!(decode_film_mode(0x0600), "Classic Chrome");
        assert_eq!(decode_film_mode(0x0700), "Eterna");
        assert_eq!(decode_film_mode(0x0800), "Classic Negative");
        assert_eq!(decode_film_mode(0x9999), "Unknown");
    }

    #[test]
    fn test_decode_dynamic_range() {
        assert_eq!(decode_dynamic_range(1), "Standard (100%)");
        assert_eq!(decode_dynamic_range(2), "Wide 1 (230%)");
        assert_eq!(decode_dynamic_range(3), "Wide 2 (400%)");
        assert_eq!(decode_dynamic_range(4), "Auto");
        assert_eq!(decode_dynamic_range(99), "Unknown");
    }

    #[test]
    fn test_decode_shutter_type() {
        assert_eq!(decode_shutter_type(0), "Mechanical");
        assert_eq!(decode_shutter_type(1), "Electronic");
        assert_eq!(decode_shutter_type(2), "Electronic (Silent)");
        assert_eq!(decode_shutter_type(99), "Unknown");
    }

    #[test]
    fn test_decode_picture_mode() {
        assert_eq!(decode_picture_mode(0x0000), "Auto");
        assert_eq!(decode_picture_mode(0x0001), "Portrait");
        assert_eq!(decode_picture_mode(0x0002), "Landscape");
        assert_eq!(decode_picture_mode(0x0006), "Program AE");
        assert_eq!(decode_picture_mode(0x0009), "Manual");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = FujifilmParser;
        assert_eq!(parser.manufacturer_name(), "Fujifilm");
        assert_eq!(parser.tag_prefix(), "Fujifilm:");
    }

    #[test]
    fn test_lens_lookup() {
        let parser = FujifilmParser;

        // Test XF lens lookup
        assert!(parser.lookup_lens(35).is_some());
        assert_eq!(parser.lookup_lens(35), Some("XF 35mm f/1.4 R".to_string()));

        // Test GF lens lookup
        assert_eq!(
            parser.lookup_lens(63),
            Some("GF 63mm f/2.8 R WR".to_string())
        );

        // Test unknown lens
        assert_eq!(parser.lookup_lens(65000), None);
    }

    #[test]
    fn test_decode_off_on() {
        assert_eq!(decode_off_on(0), "Off");
        assert_eq!(decode_off_on(1), "On");
        assert_eq!(decode_off_on(2), "Unknown");
    }

    #[test]
    fn test_decode_drive_mode() {
        assert_eq!(decode_drive_mode(0), "Single Frame");
        assert_eq!(decode_drive_mode(1), "Continuous Low");
        assert_eq!(decode_drive_mode(2), "Continuous High");
        assert_eq!(decode_drive_mode(4), "Self-timer");
    }

    #[test]
    fn test_decode_exr_mode() {
        assert_eq!(decode_exr_mode(256), "HR (High Resolution)");
        assert_eq!(decode_exr_mode(512), "SN (Signal-to-Noise Priority)");
        assert_eq!(decode_exr_mode(768), "DR (Dynamic Range Priority)");
    }
}
