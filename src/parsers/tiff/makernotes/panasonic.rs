//! Panasonic MakerNote Parser
//!
//! Parses Panasonic-specific EXIF MakerNote tags containing camera settings,
//! lens information, film simulation modes, and other proprietary metadata.
//!
//! Supports both Lumix Micro Four Thirds (M43) cameras and full-frame L-mount cameras.
//!
//! Based on ExifTool's Panasonic.pm module.
//!
//! ## Architecture
//! This module has been refactored to use the shared MakerNotes framework,
//! reducing code duplication by using:
//! - **const_decoder!** macros for declarative value decoders
//! - **Shared IFD parsing** utilities to eliminate duplicate parsing code
//! - **Generic decoders** for common patterns (ON_OFF, etc.)
//!
//! ## Code Duplication Reduction
//! This refactoring eliminates decoder function duplication while maintaining
//! 100% functionality and test coverage.

#![allow(dead_code)]
#![allow(unused_imports)]

// Submodules for extended tag parsing
pub mod extended;

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

use super::panasonic_lens_database::lookup_lens_name;
use super::shared::MakerNoteParser;

// Import declarative decoder macros
use crate::const_decoder;

// Import registry
use super::registries::panasonic::panasonic_registry;

// Panasonic MakerNote header signature
// Panasonic uses "Panasonic\0\0\0" header (12 bytes)
const PANASONIC_HEADER: &[u8] = b"Panasonic\0\0\0";

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================
// Using const_decoder! macro to eliminate decoder function duplication

// Quality mode decoder - maps values to image quality settings
const_decoder!(pub QUALITY,
    i32,
    [
        (1, "Economy"),
        (2, "Normal"),
        (3, "Fine"),
        (4, "Super Fine"),
        (5, "Extra Fine"),
        (6, "RAW"),
        (7, "RAW + Fine"),
        (8, "RAW + Normal"),
        (9, "Motion Picture"),
    ]
);

// White balance decoder - maps values to white balance presets
const_decoder!(pub WHITE_BALANCE,
    i32,
    [
        (1, "Auto"),
        (2, "Daylight"),
        (3, "Cloudy"),
        (4, "Incandescent"),
        (5, "Manual"),
        (8, "Flash"),
        (10, "Black & White"),
        (11, "Manual 2"),
        (12, "Shade"),
        (13, "Kelvin"),
        (14, "Manual 3"),
        (15, "Manual 4"),
        (16, "Manual 5"),
        (17, "PC"),
    ]
);

// Focus mode decoder - maps values to autofocus modes
const_decoder!(pub FOCUS_MODE,
    i32,
    [
        (1, "Auto"),
        (2, "Manual"),
        (4, "AF-S (Single)"),
        (5, "AF-C (Continuous)"),
        (6, "AF-F (Flexible)"),
        (16, "MF (Manual Focus)"),
    ]
);

// AF area mode decoder - maps values to AF area selection modes
const_decoder!(pub AF_AREA_MODE,
    i32,
    [
        (0, "Face Detection"),
        (1, "49-Area"),
        (2, "Tracking"),
        (3, "1-Area"),
        (4, "Pinpoint"),
        (8, "Multi"),
        (16, "1-Area (high speed)"),
        (17, "49-Area (high speed)"),
        (18, "Tracking (high speed)"),
        (32, "1-Area (video)"),
    ]
);

// Image stabilization decoder - maps values to IS modes
const_decoder!(pub IMAGE_STABILIZATION,
    i32,
    [
        (2, "Mode 1"),
        (3, "Off"),
        (4, "Mode 2"),
        (6, "Mode 3"),
        (34, "Mode 1 (video)"),
        (35, "Off (video)"),
        (36, "Mode 2 (video)"),
    ]
);

// Shooting mode decoder - maps values to shooting scene modes
const_decoder!(pub SHOOTING_MODE,
    i32,
    [
        (1, "Normal"),
        (2, "Portrait"),
        (3, "Scenery"),
        (4, "Sports"),
        (5, "Night Portrait"),
        (6, "Program"),
        (7, "Aperture Priority"),
        (8, "Shutter Priority"),
        (9, "Macro"),
        (10, "Spot"),
        (11, "Manual"),
        (12, "Movie Preview"),
        (13, "Panning"),
        (14, "Simple"),
        (15, "Color Effects"),
        (18, "Panorama"),
        (19, "Glass Through"),
        (20, "HDR"),
    ]
);

// Contrast mode decoder - maps values to contrast settings
const_decoder!(pub CONTRAST_MODE,
    i32,
    [
        (0, "Normal"),
        (1, "Low"),
        (2, "High"),
        (3, "Medium Low"),
        (4, "Medium High"),
        (5, "High+"),
        (7, "Lowest"),
        (256, "Low"),
        (272, "Standard"),
        (288, "High"),
    ]
);

// Film mode (Photo Style) decoder - maps values to picture styles
const_decoder!(pub FILM_MODE,
    i32,
    [
        (1, "Standard"),
        (2, "Dynamic"),
        (3, "Nature"),
        (4, "Smooth"),
        (5, "Standard (B&W)"),
        (6, "Dynamic (B&W)"),
        (7, "Smooth (B&W)"),
        (9, "Scenery"),
        (10, "Portrait"),
        (11, "Monochrome"),
        (12, "Natural"),
        (13, "Vivid"),
        (14, "Flat"),
        (15, "Landscape"),
        (16, "Monochrome High Contrast"),
        (17, "Blue Filter"),
        (18, "Sepia"),
        (19, "Nostalgic"),
        (20, "Old Days"),
        (21, "High Contrast B&W"),
        (22, "Cinelike D"),
        (23, "Cinelike V"),
        (24, "Like 709"),
        (25, "V-Log"),
        (26, "V-Log L"),
    ]
);

// Noise reduction decoder - maps values to NR settings
const_decoder!(pub NOISE_REDUCTION,
    i32,
    [
        (0, "Standard"),
        (1, "Low (-1)"),
        (2, "High (+1)"),
        (3, "Lowest (-2)"),
        (4, "Highest (+2)"),
    ]
);

// Intelligent auto mode decoder - maps values to iA modes
const_decoder!(pub INTELLIGENT_AUTO,
    i32,
    [
        (0, "Off"),
        (1, "On"),
        (2, "On (macro)"),
        (3, "On (portrait)"),
        (4, "On (scenery)"),
        (5, "On (night portrait)"),
        (6, "On (night scenery)"),
        (7, "On (backlight portrait)"),
    ]
);

// HDR mode decoder - maps values to HDR settings
const_decoder!(pub HDR,
    i32,
    [
        (0, "Off"),
        (1, "HDR (1 EV)"),
        (2, "HDR (2 EV)"),
        (3, "HDR (3 EV)"),
        (100, "HDR Auto"),
    ]
);

// Photo style decoder - maps values to photo style presets
const_decoder!(pub PHOTO_STYLE,
    i32,
    [
        (0, "Standard"),
        (1, "Vivid"),
        (2, "Natural"),
        (3, "Monochrome"),
        (4, "Scenery"),
        (5, "Portrait"),
        (6, "Custom"),
        (7, "Cinelike D"),
        (8, "Cinelike V"),
        (9, "Like 709"),
        (10, "V-Log"),
        (11, "V-Log L"),
    ]
);

// Macro mode decoder - maps values to macro mode settings
const_decoder!(pub MACRO_MODE, i32, [(1, "On"), (2, "Off"),]);

// Rotation decoder - maps values to image rotation
const_decoder!(pub ROTATION,
    i32,
    [(1, "0°"), (3, "180°"), (6, "90° CW"), (8, "270° CW"),]
);

// Internal ND filter decoder - maps values to ND filter settings
const_decoder!(pub INTERNAL_ND_FILTER,
    i32,
    [(0, "Off"), (1, "On"), (2, "Auto"),]
);

// Intelligent exposure decoder - maps values to iExposure modes
const_decoder!(pub INTELLIGENT_EXPOSURE,
    i32,
    [(0, "Off"), (1, "Low"), (2, "Standard"), (3, "High"),]
);

// Intelligent resolution decoder - maps values to iResolution modes
const_decoder!(pub INTELLIGENT_RESOLUTION,
    i32,
    [
        (0, "Off"),
        (1, "Low"),
        (2, "Standard"),
        (3, "High"),
        (4, "Extended"),
    ]
);

// Intelligent D-range decoder - maps values to iDynamic modes
const_decoder!(pub INTELLIGENT_D_RANGE,
    i32,
    [(0, "Off"), (1, "Low"), (2, "Standard"), (3, "High"),]
);

// Long exposure noise reduction decoder
const_decoder!(pub LONG_EXPOSURE_NR, i32, [(1, "On"), (2, "Off"),]);

// Burst mode decoder - maps values to burst shooting modes
const_decoder!(pub BURST_MODE,
    i32,
    [
        (0, "Off"),
        (1, "Low/High Speed"),
        (2, "Infinite"),
        (4, "Unlimited"),
    ]
);

// Face detection decoder - maps values to face detection on/off
const_decoder!(pub FACE_DETECTION, i32, [(0, "Off"), (1, "On"),]);

// ============================================================================
// Additional Decoders for Extended Tag Coverage
// ============================================================================
// These decoders handle additional Panasonic MakerNote tags for improved
// ExifTool compatibility. Tag IDs are from ExifTool's Panasonic.pm module.

// Audio recording mode decoder (tag 0x0020)
const_decoder!(pub AUDIO, i32, [(1, "Yes"), (2, "No"), (3, "Stereo"),]);

// Color effect decoder (tag 0x0028)
const_decoder!(pub COLOR_EFFECT, i32,
    [(1, "Off"), (2, "Warm"), (3, "Cool"), (4, "Black & White"),
     (5, "Sepia"), (6, "Happy"), (8, "Vivid"),]
);

// Self timer mode decoder (tag 0x002E)
const_decoder!(pub SELF_TIMER_MODE, i32,
    [(1, "Off"), (2, "10 s"), (3, "2 s"), (4, "10 s / 3 shots"),]
);

// AF assist lamp decoder (tag 0x0031)
const_decoder!(pub AF_ASSIST_LAMP, i32,
    [(1, "Fired"), (2, "Enabled but Not Used"),
     (3, "Disabled but Required"), (4, "Disabled and Not Required"),]
);

// Optical zoom mode decoder (tag 0x0034)
const_decoder!(pub OPTICAL_ZOOM_MODE, i32, [(1, "Standard"), (2, "Extended"),]);

// Conversion lens decoder (tag 0x0035)
const_decoder!(pub CONVERSION_LENS, i32,
    [(1, "Off"), (2, "Wide"), (3, "Telephoto"), (4, "Macro"),]
);

// World time location decoder (tag 0x003A)
const_decoder!(pub WORLD_TIME_LOCATION, i32, [(1, "Home"), (2, "Destination"),]);

// Text stamp decoder (tag 0x003B, 0x003E, 0x8008, 0x8009)
const_decoder!(pub TEXT_STAMP, i32, [(1, "Off"), (2, "On"),]);

// Advanced scene type decoder (tag 0x003D)
const_decoder!(pub ADVANCED_SCENE_TYPE, i32,
    [(1, "Normal"), (2, "Outdoor/Illuminations/Flower/HDR Art"),
     (3, "Indoor/Architecture/Objects/HDR B&W"), (4, "Creative"), (5, "Auto"),
     (7, "Expressive"), (8, "Retro"), (9, "Pure"), (10, "Elegant"),
     (12, "Monochrome"), (13, "Dynamic Art"), (14, "Silhouette"),]
);

// Bracket settings decoder (tag 0x0045)
const_decoder!(pub BRACKET_SETTINGS, i32,
    [(0, "No Bracket"), (1, "3 Images, Sequence 0/-/+"), (2, "3 Images, Sequence -/0/+"),
     (3, "5 Images, Sequence 0/-/+"), (4, "5 Images, Sequence -/0/+"),
     (5, "7 Images, Sequence 0/-/+"), (6, "7 Images, Sequence -/0/+"),]
);

// Flash curtain decoder (tag 0x0048)
const_decoder!(pub FLASH_CURTAIN, i32, [(0, "n/a"), (1, "1st"), (2, "2nd"),]);

// Flash warning decoder (tag 0x0062)
const_decoder!(pub FLASH_WARNING, i32,
    [(0, "No"), (1, "Yes (flash required but disabled)"),]
);

// Burst speed decoder (tag 0x0077)
const_decoder!(pub BURST_SPEED, i32, [(0, "Low"), (1, "Mid"), (2, "High"),]);

// Clear retouch decoder (tag 0x007C)
const_decoder!(pub CLEAR_RETOUCH, i32, [(0, "Off"), (1, "On"),]);

// Shading compensation decoder (tag 0x008A)
const_decoder!(pub SHADING_COMPENSATION, i32, [(0, "Off"), (1, "On"),]);

// Sweep panorama direction decoder (tag 0x0093)
const_decoder!(pub SWEEP_PANORAMA_DIRECTION, i32,
    [(0, "Off"), (1, "Left to Right"), (2, "Right to Left"),
     (3, "Top to Bottom"), (4, "Bottom to Top"),]
);

// Timer recording decoder (tag 0x0096)
const_decoder!(pub TIMER_RECORDING, i32,
    [(0, "Off"), (1, "Time Lapse"), (2, "Stop-motion Animation"),]
);

// Shutter type decoder (tag 0x009F)
const_decoder!(pub SHUTTER_TYPE, i32,
    [(0, "Mechanical"), (1, "Electronic"), (2, "Hybrid"),]
);

// Touch AE decoder (tag 0x00AB)
const_decoder!(pub TOUCH_AE, i32, [(0, "Off"), (1, "On"),]);

/// Represents a Panasonic MakerNote parser
pub struct PanasonicParser;

impl MakerNoteParser for PanasonicParser {
    fn manufacturer_name(&self) -> &'static str {
        "Panasonic"
    }

    fn tag_prefix(&self) -> &'static str {
        "Panasonic:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Panasonic header: "Panasonic\0\0\0" (12 bytes)
        data.len() >= 12 && &data[0..12] == PANASONIC_HEADER
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

        // Validate Panasonic header
        if !self.validate_header(data) {
            return Err("Invalid Panasonic MakerNote header".to_string());
        }

        // Skip 12-byte header to IFD
        let ifd_offset = 12;

        if data.len() <= ifd_offset + 2 {
            return Ok(());
        }

        let ifd_data = &data[ifd_offset..];

        // Parse IFD entry count using EndianReader
        let reader = EndianReader::new(ifd_data, byte_order.to_io_byte_order());
        let entry_count = reader.u16_at(0).unwrap_or(0);

        // Parse IFD entries
        let entries_start = &ifd_data[2..];
        let entries = match parse_ifd_entries(entries_start, entry_count, byte_order) {
            Ok((_, entries)) => entries,
            Err(_) => return Ok(()), // Return empty on parse failure
        };

        // Get registry for tag definitions
        let registry = panasonic_registry();

        // Extract tags from entries
        for entry in entries {
            self.parse_entry(&entry, data, ifd_offset, &registry, tags);
        }

        Ok(())
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }
}

impl PanasonicParser {
    /// Parse a single IFD entry using registry-based tag definitions
    ///
    /// Uses the Panasonic tag registry to determine tag names and apply value decoders.
    /// Special cases (lens lookups, custom formatting) are handled inline.
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        ifd_offset: usize,
        registry: &super::shared::tag_registry::TagRegistry,
        tags: &mut HashMap<String, String>,
    ) {
        let tag_id = entry.tag_id;

        // Special handling for string tags (must read from data buffer)
        // These tags contain text data that needs to be extracted from the makernote
        match tag_id {
            // Basic info strings
            0x0001 | 0x0002 | 0x0025 | 0x0026 | 0x0052 | 0x0054 |
            // Supplementary info strings (BabyAge, Title, BabyName)
            0x0033 | 0x0065 | 0x0066 |
            // Location-related strings
            0x0067 | 0x0069 | 0x006B | 0x006D | 0x006F | 0x0080 => {
                if let Some(value) = extract_string_value(entry, data, ifd_offset)
                    && let Some(tag_name) = registry.get_tag_name(tag_id) {
                        tags.insert(format!("Panasonic:{}", tag_name), value);
                    }
                return;
            }
            _ => {}
        }

        // Special case: Lens type requires database lookup
        if tag_id == 0x0051 {
            // PANA_LENS_TYPE
            let lens_id = entry.value_offset as u16;
            if let Some(lens_name) = lookup_lens_name(lens_id) {
                tags.insert("Panasonic:LensType".to_string(), lens_name);
            } else {
                tags.insert(
                    "Panasonic:LensType".to_string(),
                    format!("Unknown ({})", lens_id),
                );
            }
            return;
        }

        // Special case: Flash Bias requires EV formatting
        if tag_id == 0x0024 {
            // PANA_FLASH_BIAS
            let value = entry.value_offset as i32;
            if let Some(tag_name) = registry.get_tag_name(tag_id) {
                tags.insert(
                    format!("Panasonic:{}", tag_name),
                    format!("{:.1} EV", value as f32 / 10.0),
                );
            }
            return;
        }

        // Special case: Roll and Pitch angles require degree formatting
        if tag_id == 0x008D || tag_id == 0x008E {
            // PANA_ROLL_ANGLE, PANA_PITCH_ANGLE
            let value = entry.value_offset as i32;
            if let Some(tag_name) = registry.get_tag_name(tag_id) {
                tags.insert(
                    format!("Panasonic:{}", tag_name),
                    format!("{:.1}°", value as f32 / 10.0),
                );
            }
            return;
        }

        // Special case: Self Timer requires unit formatting
        if tag_id == 0x002E {
            // PANA_SELF_TIMER
            let value = entry.value_offset;
            if let Some(tag_name) = registry.get_tag_name(tag_id) {
                tags.insert(format!("Panasonic:{}", tag_name), format!("{} s", value));
            }
            return;
        }

        // Special case: Color Temp Kelvin requires unit formatting
        if tag_id == 0x0044 {
            // PANA_COLOR_TEMP_KELVIN
            let value = entry.value_offset;
            if let Some(tag_name) = registry.get_tag_name(tag_id) {
                tags.insert(format!("Panasonic:{}", tag_name), format!("{} K", value));
            }
            return;
        }

        // Standard registry-based decoding for enumerated and simple integer tags
        if let Some(tag_name) = registry.get_tag_name(tag_id) {
            let value = entry.value_offset as i32;
            let decoded = registry.decode_i32(tag_id, value);
            tags.insert(format!("Panasonic:{}", tag_name), decoded);
        }
    }
}

/// Public function to parse Panasonic MakerNotes
pub fn parse_panasonic_makernotes(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = PanasonicParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("Panasonic MakerNotes parse error: {}", e);
    }
}

/// Check if data contains Panasonic MakerNote header
pub fn is_panasonic_makernote(data: &[u8]) -> bool {
    let parser = PanasonicParser;
    parser.validate_header(data)
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
    )
    .parse(input)
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
    )
    .parse(input)
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
    // Panasonic offsets are relative to IFD start (after header)
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
        assert_eq!(QUALITY.decode(2), "Normal");
        assert_eq!(QUALITY.decode(3), "Fine");
        assert_eq!(QUALITY.decode(6), "RAW");
        assert_eq!(QUALITY.decode(7), "RAW + Fine");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(WHITE_BALANCE.decode(1), "Auto");
        assert_eq!(WHITE_BALANCE.decode(2), "Daylight");
        assert_eq!(WHITE_BALANCE.decode(13), "Kelvin");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(FOCUS_MODE.decode(4), "AF-S (Single)");
        assert_eq!(FOCUS_MODE.decode(5), "AF-C (Continuous)");
        assert_eq!(FOCUS_MODE.decode(16), "MF (Manual Focus)");
    }

    #[test]
    fn test_decode_film_mode() {
        assert_eq!(FILM_MODE.decode(1), "Standard");
        assert_eq!(FILM_MODE.decode(22), "Cinelike D");
        assert_eq!(FILM_MODE.decode(23), "Cinelike V");
        assert_eq!(FILM_MODE.decode(25), "V-Log");
    }

    #[test]
    fn test_decode_shooting_mode() {
        assert_eq!(SHOOTING_MODE.decode(6), "Program");
        assert_eq!(SHOOTING_MODE.decode(7), "Aperture Priority");
        assert_eq!(SHOOTING_MODE.decode(11), "Manual");
    }

    #[test]
    fn test_decode_hdr() {
        assert_eq!(HDR.decode(0), "Off");
        assert_eq!(HDR.decode(1), "HDR (1 EV)");
        assert_eq!(HDR.decode(100), "HDR Auto");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = PanasonicParser;
        assert_eq!(parser.manufacturer_name(), "Panasonic");
        assert_eq!(parser.tag_prefix(), "Panasonic:");
    }

    #[test]
    fn test_validate_header() {
        let parser = PanasonicParser;

        let valid_header = b"Panasonic\0\0\0extra_data_here";
        assert!(parser.validate_header(valid_header));

        let invalid_header = b"Canon\0\0\0";
        assert!(!parser.validate_header(invalid_header));

        let too_short = b"Panasonic";
        assert!(!parser.validate_header(too_short));
    }

    #[test]
    fn test_lens_lookup() {
        let parser = PanasonicParser;

        // Test M43 lens lookup
        assert!(parser.lookup_lens(32).is_some());
        assert_eq!(
            parser.lookup_lens(32),
            Some("Leica DG Nocticron 42.5mm f/1.2 ASPH. POWER O.I.S.".to_string())
        );

        // Test L-mount lens lookup
        assert!(parser.lookup_lens(103).is_some());
        assert_eq!(
            parser.lookup_lens(103),
            Some("Lumix S Pro 24-70mm f/2.8".to_string())
        );

        // Test unknown lens
        assert_eq!(parser.lookup_lens(65000), None);
    }

    #[test]
    fn test_is_panasonic_makernote() {
        let valid_data = b"Panasonic\0\0\0some_data";
        assert!(is_panasonic_makernote(valid_data));

        let invalid_data = b"Nikon\0\0\0";
        assert!(!is_panasonic_makernote(invalid_data));
    }
}
