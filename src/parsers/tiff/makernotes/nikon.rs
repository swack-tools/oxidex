//! Nikon MakerNote parser
//!
//! Parses Nikon-specific EXIF MakerNote tags containing camera settings,
//! lens information, autofocus data, and other proprietary metadata.
//!
//! Supports Nikon Type 2 (IFD-based) and Type 3 (IFD-based with header) formats.

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

use super::nikon_lens_database::lookup_lens_name;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use super::shared::MakerNoteParser;

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
const NIKON_ISO_SELECTION: u16 = 0x0011;
const NIKON_PREVIEW_IFD: u16 = 0x0011;
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

        // Determine header type and skip to IFD
        // Type 2: "Nikon\0\x02\x10\x00\x00" (10 bytes header)
        // Type 3: "Nikon\0\x02\x00\x00\x00" (10 bytes header)
        let ifd_offset = if data.len() >= 10 {
            10 // Skip 10-byte header
        } else {
            6 // Fallback to just skip "Nikon\0"
        };

        if data.len() <= ifd_offset + 2 {
            return Ok(());
        }

        let ifd_data = &data[ifd_offset..];

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
                // Simple string tags
                NIKON_VERSION | NIKON_SERIAL_NUMBER => {
                    if let Some(value) = extract_string_value(&entry, data, ifd_offset) {
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
                    if let Some(array) = extract_u16_array(&entry, data, byte_order) {
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
                    if let Some(array) = extract_u16_array(&entry, data, byte_order) {
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
                    if let Some(array) = extract_u16_array(&entry, data, byte_order) {
                        if array.len() >= 4 {
                            tags.insert(
                                "Nikon:WB_RBLevels".to_string(),
                                format!("{} {}", array[0], array[1]),
                            );
                        }
                    }
                }

                // Other array tags - skip for now or add basic extraction
                _ => continue,
            }
        }

        Ok(())
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }
}

/// Maps Nikon MakerNote tag IDs to human-readable tag names
fn nikon_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        NIKON_VERSION => "Version",
        NIKON_ISO_SPEED => "ISOSpeed",
        NIKON_COLOR_MODE => "ColorMode",
        NIKON_QUALITY => "Quality",
        NIKON_WHITE_BALANCE => "WhiteBalance",
        NIKON_SHARPNESS => "Sharpness",
        NIKON_FOCUS_MODE => "FocusMode",
        NIKON_FLASH_SETTING => "FlashSetting",
        NIKON_FLASH_TYPE => "FlashType",
        NIKON_SERIAL_NUMBER => "SerialNumber",
        NIKON_SHUTTER_COUNT => "ShutterCount",
        NIKON_LENS_DATA => "LensData",
        NIKON_SHOT_INFO => "ShotInfo",
        NIKON_COLOR_SPACE => "ColorSpace",
        NIKON_ACTIVE_D_LIGHTING => "ActiveDLighting",
        NIKON_VIGNETTE_CONTROL => "VignetteControl",
        _ => return format!("Nikon:Unknown-{:#06X}", tag_id),
    };

    format!("Nikon:{}", tag_name)
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
    // Nikon offsets are relative to IFD start (after header)
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
        assert_eq!(nikon_tag_to_name(0x0001), "Nikon:Version");
        assert_eq!(nikon_tag_to_name(0x0002), "Nikon:ISOSpeed");
        assert_eq!(nikon_tag_to_name(0x0004), "Nikon:Quality");
        assert_eq!(nikon_tag_to_name(0x00A7), "Nikon:ShutterCount");
        assert_eq!(nikon_tag_to_name(0xFFFF), "Nikon:Unknown-0xFFFF");
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
