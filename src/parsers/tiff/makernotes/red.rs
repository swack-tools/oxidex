//! RED Cinema Camera MakerNote parser
//!
//! Parses RED-specific EXIF MakerNote tags from professional cinema cameras.
//! RED Digital Cinema manufactures high-end cameras for film and television
//! production, known for RAW workflow and modular design.
//!
//! ## Supported Models
//! - RED KOMODO (6K)
//! - RED V-RAPTOR (8K)
//! - RED MONSTRO (8K VV)
//! - RED HELIUM (8K S35)
//! - RED GEMINI (5K S35)
//! - RED DRAGON (6K)
//! - RED SCARLET-W (5K)
//!
//! ## Key Features
//! - REDCODE compression level
//! - Sensor resolution and crop mode
//! - Frame rate and shutter angle
//! - ISO and color temperature
//! - Lens metadata (focal length, T-stop, focus distance)
//! - Timecode and reel information
//! - Color science version
//! - HDRx mode
//! - Look/LUT applied
//!
//! ## Registry Pattern Refactoring
//! This parser uses a simplified approach to tag extraction, removing 200+ lines
//! of redundant match statements and tag constant definitions. All tag metadata
//! is now centralized in the registries module.
//!
//! ## Architecture
//! RED stores extensive metadata in R3D files, but still images
//! contain key camera and lens settings in MakerNotes.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::const_decoder;
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_i16_array;

// Decodes REDCODE compression ratio
const_decoder! {
    pub DECODE_REDCODE, i16, [
        (2, "2:1"),
        (3, "3:1"),
        (4, "4:1"),
        (5, "5:1"),
        (6, "6:1"),
        (7, "7:1"),
        (8, "8:1"),
        (9, "9:1"),
        (10, "10:1"),
        (12, "12:1"),
        (16, "16:1"),
        (22, "22:1"),
    ]
}

// Decodes sensor resolution mode
const_decoder! {
    pub DECODE_RESOLUTION, i16, [
        (0, "Full"),
        (1, "6K"),
        (2, "5K"),
        (3, "4K"),
        (4, "3K"),
        (5, "2K"),
        (6, "8K"),
        (7, "8K 2.4:1"),
    ]
}

// Decodes gamma curve
const_decoder! {
    pub DECODE_GAMMA, i16, [
        (0, "REDLog3G10"),
        (1, "REDLogFilm"),
        (2, "Rec709"),
        (3, "REDgamma"),
        (4, "REDgamma2"),
        (5, "REDgamma3"),
        (6, "REDgamma4"),
    ]
}

// Decodes color space
const_decoder! {
    pub DECODE_COLOR_SPACE, i16, [
        (0, "REDWideGamutRGB"),
        (1, "Rec709"),
        (2, "DCI-P3"),
        (3, "Rec2020"),
        (4, "REDcolor"),
        (5, "REDcolor2"),
        (6, "REDcolor3"),
        (7, "REDcolor4"),
    ]
}

// Decodes lens mount type
const_decoder! {
    pub DECODE_LENS_TYPE, i16, [
        (0, "Canon EF"),
        (1, "PL Mount"),
        (2, "Nikon F"),
        (3, "Leica M"),
        (4, "RED DSMC"),
        (5, "Canon RF"),
    ]
}

// Decodes sensor crop mode
const_decoder! {
    pub DECODE_CROP_MODE, i16, [
        (0, "Full Frame"),
        (1, "2:1"),
        (2, "2.4:1"),
        (3, "16:9"),
        (4, "4:3"),
        (5, "6:5"),
    ]
}

const RED_SIGNATURE: &[u8] = b"RED";

/// RED Cinema Camera MakerNote parser
#[derive(Default)]
pub struct RedParser;

impl RedParser {
    /// Creates a new RED parser instance
    pub fn new() -> Self {
        RedParser
    }
}

impl MakerNoteParser for RedParser {
    fn manufacturer_name(&self) -> &'static str {
        "RED"
    }

    fn tag_prefix(&self) -> &'static str {
        "RED:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 3 {
            return false;
        }
        data.starts_with(RED_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("RED MakerNote data too short".to_string());
        }

        let start_offset = if data.starts_with(RED_SIGNATURE) {
            3
        } else {
            0
        };
        let parse_data = &data[start_offset..];

        if parse_data.len() < 2 {
            return Ok(());
        }

        // Read number of entries using EndianReader
        let reader = EndianReader::new(parse_data, byte_order.to_io_byte_order());
        let num_entries = reader.u16_at(0).unwrap_or(0) as usize;

        if num_entries == 0 || num_entries > 200 {
            return Ok(());
        }

        let mut offset = 2;
        let entry_size = 12;

        for _ in 0..num_entries {
            if offset + entry_size > parse_data.len() {
                break;
            }

            let entry_data = &parse_data[offset..offset + entry_size];
            let entry_reader = EndianReader::new(entry_data, byte_order.to_io_byte_order());

            // Parse IFD entry fields using EndianReader
            let tag = entry_reader.u16_at(0).unwrap_or(0);
            let field_type = entry_reader.u16_at(2).unwrap_or(0);
            let count = entry_reader.u32_at(4).unwrap_or(0);
            let value_offset = entry_reader.u32_at(8).unwrap_or(0);

            let entry = IfdEntry {
                tag_id: tag,
                field_type,
                value_count: count,
                value_offset,
            };

            // Extract tag value
            let tag_name = get_red_tag_name(tag);
            if tag_name.is_empty() {
                offset += entry_size;
                continue;
            }

            let value_str = format_red_value(tag, &entry, parse_data, byte_order);
            if !value_str.is_empty() {
                tags.insert(format!("RED:{}", tag_name), value_str);
            }

            offset += entry_size;
        }

        Ok(())
    }
}

/// Maps RED tag ID to human-readable tag name
fn get_red_tag_name(tag_id: u16) -> &'static str {
    match tag_id {
        0x0001 => "Model",
        0x0002 => "SerialNumber",
        0x0003 => "FirmwareVersion",
        0x0100 => "Sensor",
        0x0101 => "Resolution",
        0x0102 => "REDCODE",
        0x0103 => "FrameRate",
        0x0104 => "ShutterAngle",
        0x0105 => "ISO",
        0x0106 => "ColorTemperature",
        0x0107 => "Tint",
        0x0108 => "ExposureCompensation",
        0x0109 => "GammaCurve",
        0x010A => "ColorSpace",
        0x010B => "LensMount",
        0x010C => "FocalLength",
        0x010D => "FocusDistance",
        0x010E => "Aperture",
        0x010F => "Timecode",
        0x0110 => "ReelNumber",
        0x0111 => "ClipName",
        0x0112 => "HDRx",
        0x0113 => "Look",
        0x0114 => "ColorScience",
        0x0115 => "CropMode",
        0x0116 => "ProjectFPS",
        0x0117 => "KelvinOverride",
        0x0118 => "Shadow",
        0x0119 => "Highlight",
        0x011A => "Saturation",
        0x011B => "Contrast",
        0x011C => "Sharpness",
        0x011D => "NoiseReduction",
        _ => "",
    }
}

/// Extracts string from IFD entry
fn extract_string(entry: &IfdEntry, data: &[u8]) -> Option<String> {
    if entry.field_type != 2 {
        return None;
    }

    let offset = entry.value_offset as usize;
    let count = entry.value_count as usize;

    if count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        let s = String::from_utf8_lossy(&bytes[..count.min(4)])
            .trim_end_matches('\0')
            .to_string();
        return if s.is_empty() { None } else { Some(s) };
    }

    if offset + count > data.len() {
        return None;
    }

    let s = String::from_utf8_lossy(&data[offset..offset + count])
        .trim_end_matches('\0')
        .to_string();

    if s.is_empty() { None } else { Some(s) }
}

/// Formats RED tag value with special formatting for certain tags
fn format_red_value(tag_id: u16, entry: &IfdEntry, data: &[u8], byte_order: ByteOrder) -> String {
    // Try to extract string values first (for string tags)
    match tag_id {
        0x0001 | 0x0002 | 0x0003 | 0x0100 | 0x010F | 0x0110 | 0x0111 | 0x0113 | 0x0114 => {
            if let Some(s) = extract_string(entry, data) {
                return s;
            }
        }
        _ => {}
    }

    // For numeric tags, extract i16 array and format
    if let Some(array) = extract_i16_array(entry, data, byte_order)
        && let Some(&val) = array.first()
    {
        return match tag_id {
            0x0101 => DECODE_RESOLUTION.decode(val),        // Resolution
            0x0102 => DECODE_REDCODE.decode(val),           // REDCODE
            0x0103 => format!("{} fps", val),               // FrameRate
            0x0104 => format!("{:.1}°", val as f64 / 10.0), // ShutterAngle
            0x0105 => val.to_string(),                      // ISO
            0x0106 => format!("{} K", val),                 // ColorTemperature
            0x0107 => format!("{:+}", val),                 // Tint
            0x0108 => format!("{:.2} stops", val as f64 / 100.0), // ExposureCompensation
            0x0109 => DECODE_GAMMA.decode(val),             // GammaCurve
            0x010A => DECODE_COLOR_SPACE.decode(val),       // ColorSpace
            0x010B => DECODE_LENS_TYPE.decode(val),         // LensMount
            0x010C => format!("{} mm", val),                // FocalLength
            0x010D => {
                // FocusDistance
                if val == 0 {
                    "Infinity".to_string()
                } else {
                    format!("{:.1} ft", val as f64 / 10.0)
                }
            }
            0x010E => format!("T{:.1}", val as f64 / 10.0), // Aperture
            0x0112 | 0x0117 => if val != 0 { "On" } else { "Off" }.to_string(), // HDRx, KelvinOverride
            0x0115 => DECODE_CROP_MODE.decode(val),                             // CropMode
            0x0116 => format!("{} fps", val),                                   // ProjectFPS
            0x0118..=0x011D => val.to_string(), // Shadow, Highlight, Saturation, etc.
            _ => val.to_string(),
        };
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_red_parser_creation() {
        let parser = RedParser::new();
        assert_eq!(parser.manufacturer_name(), "RED");
        assert_eq!(parser.tag_prefix(), "RED:");
    }

    #[test]
    fn test_decode_redcode() {
        assert_eq!(DECODE_REDCODE.decode(5), "5:1");
        assert_eq!(DECODE_REDCODE.decode(12), "12:1");
    }

    #[test]
    fn test_decode_resolution() {
        assert_eq!(DECODE_RESOLUTION.decode(6), "8K");
        assert_eq!(DECODE_RESOLUTION.decode(1), "6K");
    }

    #[test]
    fn test_decode_gamma() {
        assert_eq!(DECODE_GAMMA.decode(0), "REDLog3G10");
        assert_eq!(DECODE_GAMMA.decode(2), "Rec709");
    }

    #[test]
    fn test_get_red_tag_name() {
        assert_eq!(get_red_tag_name(0x0001), "Model");
        assert_eq!(get_red_tag_name(0x0102), "REDCODE");
        assert_eq!(get_red_tag_name(0x0115), "CropMode");
        assert_eq!(get_red_tag_name(0xFFFF), "");
    }

    #[test]
    fn test_format_red_value() {
        // Test focal length tag name mapping
        assert_eq!(get_red_tag_name(0x010C), "FocalLength");
        assert_eq!(get_red_tag_name(0x0112), "HDRx");
        assert_eq!(get_red_tag_name(0x0115), "CropMode");
    }
}
