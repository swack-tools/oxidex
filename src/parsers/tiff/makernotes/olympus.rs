//! Olympus MakerNote Parser
//!
//! Parses Olympus-specific EXIF MakerNote tags containing camera settings,
//! lens information, image quality parameters, and other proprietary metadata.
//!
//! Supports both Four Thirds (E-series DSLRs) and Micro Four Thirds (OM-D, PEN) cameras.
//!
//! Based on ExifTool's Olympus.pm module.
//!
//! ## Architecture
//! This parser uses the shared MakerNote framework to eliminate code duplication:
//! - **Registry system** for centralized tag definitions and array schemas
//! - **const_decoder!** macros for declarative value decoders
//! - **Generic decoders** (ON_OFF) for common patterns
//! - **Shared extractors** for common array extraction logic
//!
//! The parser uses `olympus_registry()` from the registries module to process
//! standard tags and array-based tag structures, reducing duplication.

#![allow(dead_code)]
#![allow(unused_imports)]

// Submodules for extended tag parsing
pub mod camera_settings;

use crate::const_decoder;
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

use super::olympus_lens_database::{get_lens_database, lookup_lens_name};
use super::registries::olympus::olympus_registry;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_i32_array, extract_u16_array};
use super::shared::generic_decoders::ON_OFF;

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

// Olympus MakerNote header signatures
// Type 2 (newer cameras): "OLYMPUS\0II" or "OLYMPUS\0MM" (10 bytes) followed by offset
const OLYMPUS_HEADER: &[u8] = b"OLYMPUS\0II";
const OLYMPUS_HEADER_BE: &[u8] = b"OLYMPUS\0MM";
// Type 1 (older cameras): "OLYMP\x00\x01" or "OLYMP\x00\x02" (8 bytes)
const OLYMPUS_HEADER_TYPE1_V1: &[u8] = b"OLYMP\x00\x01";
const OLYMPUS_HEADER_TYPE1_V2: &[u8] = b"OLYMP\x00\x02";

// Sub-IFD pointer tag IDs - these point to nested IFD structures
const OLYMPUS_EQUIPMENT_SUBIFD: u16 = 0x2010;
const OLYMPUS_CAMERA_SETTINGS_SUBIFD: u16 = 0x2020;
const OLYMPUS_RAW_DEVELOPMENT_SUBIFD: u16 = 0x2030;
const OLYMPUS_RAW_DEV2_SUBIFD: u16 = 0x2031;
const OLYMPUS_IMAGE_PROCESSING_SUBIFD: u16 = 0x2040;
const OLYMPUS_FOCUS_INFO_SUBIFD: u16 = 0x2050;
const OLYMPUS_RAW_INFO_SUBIFD: u16 = 0x3000;
const OLYMPUS_MAIN_INFO_SUBIFD: u16 = 0x4000;

// Note: Array index constants were previously used here for Camera Settings (0x0003)
// and Equipment (0x0201) arrays, but are now handled by the registry system.
// See registries/olympus.rs for the centralized array schema definitions.

// ============================================================================
// Decoder Definitions using const_decoder! macro
// ============================================================================
// These replace individual decoder functions, dramatically reducing code duplication

// Olympus quality mode decoder
const_decoder!(
    pub QUALITY_DECODER,
    i32,
    [
        (1, "SQ (Standard Quality)"),
        (2, "HQ (High Quality)"),
        (3, "SHQ (Super High Quality)"),
        (4, "RAW"),
        (5, "SQ (Low)"),
        (6, "SQ (Medium)"),
    ]
);

// Olympus exposure mode decoder
const_decoder!(
    pub EXPOSURE_MODE_DECODER,
    i32,
    [
        (1, "Manual"),
        (2, "Program"),
        (3, "Aperture Priority"),
        (4, "Shutter Priority"),
        (5, "Program Shift"),
    ]
);

// Olympus metering mode decoder
const_decoder!(
    pub METERING_MODE_DECODER,
    i32,
    [
        (2, "Center Weighted"),
        (3, "Spot"),
        (5, "ESP (Evaluative)"),
        (261, "Pattern+AF"),
        (515, "Spot+Highlight Control"),
        (1027, "Spot+Shadow Control"),
    ]
);

// Olympus focus mode decoder
const_decoder!(
    pub FOCUS_MODE_DECODER,
    i32,
    [
        (0, "Single AF"),
        (1, "Sequential Shooting AF"),
        (2, "Continuous AF"),
        (3, "Manual Focus"),
        (4, "Super AF"),
        (5, "AF-C"),
        (10, "MF"),
    ]
);

// Olympus white balance decoder
const_decoder!(
    pub WHITE_BALANCE_DECODER,
    i32,
    [
        (0, "Auto"),
        (1, "Auto (Keep Warm Color Off)"),
        (16, "7500K (Fine Weather with Shade)"),
        (17, "6000K (Cloudy)"),
        (18, "5300K (Fine Weather)"),
        (20, "3000K (Tungsten)"),
        (21, "3600K (Evening Sunlight)"),
        (22, "Auto Setup"),
        (23, "5500K (Flash)"),
        (33, "6600K (Daylight Fluorescent)"),
        (34, "4500K (Neutral White Fluorescent)"),
        (35, "4000K (Cool White Fluorescent)"),
        (36, "White Fluorescent"),
        (48, "3600K (Tungsten)"),
        (67, "Underwater"),
        (256, "One Touch WB 1"),
        (257, "One Touch WB 2"),
        (258, "One Touch WB 3"),
        (259, "One Touch WB 4"),
        (512, "Custom WB 1"),
        (513, "Custom WB 2"),
        (514, "Custom WB 3"),
        (515, "Custom WB 4"),
    ]
);

// Olympus flash mode decoder
const_decoder!(
    pub FLASH_MODE_DECODER,
    i32,
    [
        (0, "Off"),
        (1, "On"),
        (2, "Fill-in"),
        (3, "Red-eye"),
        (4, "Slow Sync"),
        (5, "Forced On"),
        (6, "2nd Curtain"),
    ]
);

// Olympus scene mode decoder
const_decoder!(
    pub SCENE_MODE_DECODER,
    i32,
    [
        (0, "Standard"),
        (6, "Auto"),
        (7, "Sport"),
        (8, "Portrait"),
        (9, "Landscape"),
        (10, "Night Scene"),
        (11, "Self Portrait"),
        (12, "Panorama"),
        (13, "2 in 1"),
        (14, "Movie"),
        (15, "Landscape+Portrait"),
        (16, "Night+Portrait"),
        (17, "Indoor"),
        (18, "Fireworks"),
        (19, "Sunset"),
        (20, "Beauty Skin"),
        (21, "Macro"),
        (22, "Super Macro"),
        (23, "Food"),
        (24, "Documents"),
        (25, "Museum"),
        (26, "Shoot & Select"),
        (27, "Beach & Snow"),
        (28, "Self Portrait+Self Timer"),
        (29, "Candle"),
        (30, "Available Light"),
        (31, "Behind Glass"),
        (32, "My Mode"),
        (33, "Pet"),
        (34, "Underwater Wide"),
        (35, "Underwater Macro"),
        (36, "Shoot & Select 1"),
        (37, "Shoot & Select 2"),
        (38, "Digital Image Stabilization"),
        (39, "Face Portrait"),
        (40, "Pet Portrait"),
        (41, "Smile Shot"),
        (42, "Quick Shutter"),
    ]
);

// Olympus picture mode decoder
const_decoder!(
    pub PICTURE_MODE_DECODER,
    i32,
    [
        (1, "Vivid"),
        (2, "Natural"),
        (3, "Muted"),
        (4, "Portrait"),
        (5, "i-Enhance"),
        (6, "Color Creator"),
        (7, "Custom"),
        (8, "e-Portrait"),
        (9, "Color Profile 1"),
        (10, "Color Profile 2"),
        (11, "Color Profile 3"),
        (12, "Monochrome Profile 1"),
        (13, "Monochrome Profile 2"),
        (14, "Monochrome Profile 3"),
        (256, "Monotone"),
        (512, "Sepia"),
    ]
);

// Olympus art filter decoder
const_decoder!(
    pub ART_FILTER_DECODER,
    i32,
    [
        (0, "Off"),
        (1, "Soft Focus"),
        (2, "Pop Art"),
        (3, "Pale & Light Color"),
        (4, "Light Tone"),
        (5, "Pin Hole"),
        (6, "Grainy Film"),
        (9, "Diorama"),
        (10, "Cross Process"),
        (12, "Fish Eye"),
        (13, "Drawing"),
        (14, "Gentle Sepia"),
        (15, "Pale & Light Color II"),
        (16, "Pop Art II"),
        (17, "Pin Hole II"),
        (18, "Pin Hole III"),
        (19, "Grainy Film II"),
        (20, "Dramatic Tone"),
        (21, "Punk"),
        (22, "Soft Focus 2"),
        (23, "Sparkle"),
        (24, "Watercolor"),
        (25, "Key Line"),
        (26, "Key Line II"),
        (27, "Miniature"),
        (28, "Reflection"),
        (29, "Fragmented"),
        (31, "Cross Process II"),
        (32, "Gentle Sepia II"),
        (33, "Dramatic Tone II"),
        (34, "Vintage"),
        (35, "Vintage II"),
        (36, "Vintage III"),
        (37, "Partial Color"),
        (38, "Partial Color II"),
        (39, "Partial Color III"),
    ]
);

// Olympus noise reduction decoder
const_decoder!(
    pub NOISE_REDUCTION_DECODER,
    i32,
    [
        (0, "Off"),
        (1, "Noise Reduction"),
        (2, "Noise Filter"),
        (3, "Noise Reduction + Noise Filter"),
        (4, "Noise Filter (ISO Boost)"),
        (5, "Noise Reduction + Noise Filter (ISO Boost)"),
    ]
);

// Olympus color space decoder
const_decoder!(
    pub COLOR_SPACE_DECODER,
    i32,
    [(0, "sRGB"), (1, "Adobe RGB"), (2, "Pro Photo RGB"),]
);

// Olympus macro mode decoder
const_decoder!(
    pub MACRO_MODE_DECODER,
    i32,
    [(0, "Off"), (1, "On"), (2, "Super Macro"),]
);

// ============================================================================
// Olympus MakerNote Parser Implementation
// ============================================================================

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
        // Check for Type 2 headers (10 bytes): "OLYMPUS\0II" or "OLYMPUS\0MM"
        if data.len() >= 10 && (&data[0..10] == OLYMPUS_HEADER || &data[0..10] == OLYMPUS_HEADER_BE)
        {
            return true;
        }

        // Check for Type 1 headers (8 bytes): "OLYMP\x00\x01" or "OLYMP\x00\x02"
        if data.len() >= 8
            && (&data[0..8] == OLYMPUS_HEADER_TYPE1_V1 || &data[0..8] == OLYMPUS_HEADER_TYPE1_V2)
        {
            return true;
        }

        false
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

        // Detect header type and determine parsing parameters
        let (ifd_start, base_offset, effective_byte_order) =
            detect_header_type_and_offsets(data, byte_order)?;

        if data.len() <= ifd_start + 2 {
            return Ok(());
        }

        let ifd_data = &data[ifd_start..];

        // Parse IFD entry count using EndianReader
        let ifd_reader = EndianReader::new(ifd_data, effective_byte_order.to_io_byte_order());
        let entry_count = ifd_reader.u16_at(0).unwrap_or(0);

        // Sanity check on entry count
        if entry_count > 500 || entry_count == 0 {
            return Ok(());
        }

        // Parse IFD entries
        let entries_start = &ifd_data[2..];
        let entries = match parse_ifd_entries(entries_start, entry_count, effective_byte_order) {
            Ok((_, entries)) => entries,
            Err(_) => return Ok(()), // Return empty on parse failure
        };

        // Get the Olympus registry for tag definitions and array schemas
        let registry = olympus_registry();

        // Extract tags from entries
        for entry in entries {
            match entry.tag_id {
                // Camera Settings array (0x0003) - i32 array with 49 indices
                OLYMPUS_CAMERA_SETTINGS => {
                    if let Some(array) =
                        extract_i32_array_with_base(&entry, data, effective_byte_order, base_offset)
                    {
                        registry.decode_array_i32(OLYMPUS_CAMERA_SETTINGS, &array, "Olympus", tags);
                    }
                }

                // Equipment array (0x0201) - byte array with complex internal structure
                OLYMPUS_EQUIPMENT => {
                    if let Some(array) = extract_u8_array(&entry, data, base_offset) {
                        // Equipment array has complex byte-level parsing that requires special handling
                        // beyond what the registry array schema provides. Use the specialized helper.
                        use super::registries::olympus::process_equipment_with_lens;
                        process_equipment_with_lens(
                            &array,
                            "Olympus",
                            get_lens_database(),
                            effective_byte_order,
                            tags,
                        );
                    }
                }

                // Sub-IFD pointers - parse nested IFD structures
                OLYMPUS_EQUIPMENT_SUBIFD
                | OLYMPUS_CAMERA_SETTINGS_SUBIFD
                | OLYMPUS_RAW_DEVELOPMENT_SUBIFD
                | OLYMPUS_RAW_DEV2_SUBIFD
                | OLYMPUS_IMAGE_PROCESSING_SUBIFD
                | OLYMPUS_FOCUS_INFO_SUBIFD
                | OLYMPUS_RAW_INFO_SUBIFD
                | OLYMPUS_MAIN_INFO_SUBIFD => {
                    // Parse sub-IFD at the offset specified by the entry
                    let sub_ifd_offset = (entry.value_offset as usize) + base_offset;
                    let sub_ifd_name = get_sub_ifd_name(entry.tag_id);
                    parse_sub_ifd(
                        data,
                        sub_ifd_offset,
                        base_offset,
                        effective_byte_order,
                        sub_ifd_name,
                        tags,
                    );
                }

                // All other registered tags - handled through the registry
                _ => {
                    if registry.has_tag(entry.tag_id) {
                        // String-type tags (ASCII)
                        if entry.field_type == 2 {
                            // ASCII field type
                            if let Some(value) = extract_string_value(&entry, data, base_offset)
                                && let Some(tag_name) = registry.get_tag_name(entry.tag_id)
                            {
                                tags.insert(format!("Olympus:{}", tag_name), value);
                            }
                        } else {
                            // Numeric tags that don't require special handling
                            let value = entry.value_offset as i32;
                            if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                                let decoded = registry.decode_i32(entry.tag_id, value);
                                tags.insert(format!("Olympus:{}", tag_name), decoded);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Converts Olympus tag ID to tag name string
///
/// # Arguments
/// * `tag_id` - The Olympus tag ID
///
/// # Returns
/// String in format "Olympus:TagName" or "Olympus:Unknown-0xXXXX"
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

/// Detects the Olympus MakerNote header type and returns parsing parameters
///
/// # Arguments
/// * `data` - The raw MakerNote data
/// * `default_byte_order` - Default byte order from TIFF header
///
/// # Returns
/// Tuple of (ifd_start_offset, base_offset_for_values, effective_byte_order)
fn detect_header_type_and_offsets(
    data: &[u8],
    default_byte_order: ByteOrder,
) -> std::result::Result<(usize, usize, ByteOrder), String> {
    // Check Type 2 headers first (they're longer and more specific)
    if data.len() >= 12 {
        if &data[0..10] == OLYMPUS_HEADER {
            // Type 2 Little Endian: Read IFD offset from bytes 10-11
            let reader = EndianReader::new(data, crate::io::ByteOrder::Little);
            let ifd_offset = reader.u16_at(10).unwrap_or(3) as usize;
            // IFD offset is relative to position 8 (after "OLYMPUS\0")
            return Ok((8 + ifd_offset, 8, ByteOrder::LittleEndian));
        }
        if &data[0..10] == OLYMPUS_HEADER_BE {
            // Type 2 Big Endian: Read IFD offset from bytes 10-11
            let reader = EndianReader::new(data, crate::io::ByteOrder::Big);
            let ifd_offset = reader.u16_at(10).unwrap_or(3) as usize;
            return Ok((8 + ifd_offset, 8, ByteOrder::BigEndian));
        }
    }

    // Check Type 1 headers
    if data.len() >= 8
        && (&data[0..8] == OLYMPUS_HEADER_TYPE1_V1 || &data[0..8] == OLYMPUS_HEADER_TYPE1_V2)
    {
        // Type 1: IFD starts immediately after 8-byte header
        // Offsets are typically TIFF-relative, but we treat base_offset as 0
        // since the data slice we receive starts at MakerNote position
        return Ok((8, 0, default_byte_order));
    }

    Err("Invalid Olympus MakerNote header".to_string())
}

/// Returns the sub-IFD name prefix for a given tag ID
fn get_sub_ifd_name(tag_id: u16) -> &'static str {
    match tag_id {
        OLYMPUS_EQUIPMENT_SUBIFD => "Equipment",
        OLYMPUS_CAMERA_SETTINGS_SUBIFD => "CameraSettings",
        OLYMPUS_RAW_DEVELOPMENT_SUBIFD => "RawDevelopment",
        OLYMPUS_RAW_DEV2_SUBIFD => "RawDev2",
        OLYMPUS_IMAGE_PROCESSING_SUBIFD => "ImageProcessing",
        OLYMPUS_FOCUS_INFO_SUBIFD => "FocusInfo",
        OLYMPUS_RAW_INFO_SUBIFD => "RawInfo",
        OLYMPUS_MAIN_INFO_SUBIFD => "MainInfo",
        _ => "Unknown",
    }
}

/// Parses a sub-IFD at the given offset and extracts tags with the sub-IFD name prefix
///
/// Olympus cameras store detailed metadata in nested sub-IFDs pointed to by
/// tags 0x2010-0x4000. This function parses those sub-IFDs and outputs tags
/// with the format "Olympus:SubIFDName:TagName".
fn parse_sub_ifd(
    data: &[u8],
    offset: usize,
    base_offset: usize,
    byte_order: ByteOrder,
    sub_ifd_name: &str,
    tags: &mut HashMap<String, String>,
) {
    // Validate offset
    if offset + 2 > data.len() {
        return;
    }

    let ifd_data = &data[offset..];
    let reader = EndianReader::new(ifd_data, byte_order.to_io_byte_order());
    let entry_count = reader.u16_at(0).unwrap_or(0);

    // Sanity check
    if entry_count > 500 || entry_count == 0 {
        return;
    }

    // Check if we have enough data for entries
    if ifd_data.len() < 2 + (entry_count as usize * 12) {
        return;
    }

    let entries_start = &ifd_data[2..];
    let entries = match parse_ifd_entries(entries_start, entry_count, byte_order) {
        Ok((_, entries)) => entries,
        Err(_) => return,
    };

    // Get the main registry; sub-IFD specific registries are used for full tag coverage
    let registry = olympus_registry();

    // Extract tags with sub-IFD prefix
    for entry in entries {
        if registry.has_tag(entry.tag_id) {
            if entry.field_type == 2 {
                // ASCII string
                if let Some(value) = extract_string_value(&entry, data, base_offset)
                    && let Some(tag_name) = registry.get_tag_name(entry.tag_id)
                {
                    tags.insert(format!("Olympus:{}:{}", sub_ifd_name, tag_name), value);
                }
            } else {
                // Numeric value
                let value = entry.value_offset as i32;
                if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                    let decoded = registry.decode_i32(entry.tag_id, value);
                    tags.insert(format!("Olympus:{}:{}", sub_ifd_name, tag_name), decoded);
                }
            }
        }
    }
}

/// Extract i32 array with configurable base offset support
///
/// Generic version that accepts base_offset as parameter, for use with
/// both Type 1 and Type 2 header formats.
fn extract_i32_array_with_base(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
    base_offset: usize,
) -> Option<Vec<i32>> {
    let absolute_offset = (entry.value_offset as usize) + base_offset;
    if absolute_offset > data.len() {
        return None;
    }

    let adjusted_entry = IfdEntry {
        tag_id: entry.tag_id,
        field_type: entry.field_type,
        value_count: entry.value_count,
        value_offset: absolute_offset as u32,
    };

    extract_i32_array(&adjusted_entry, data, byte_order)
}

/// Parses IFD entries in the specified byte order
///
/// # Arguments
/// * `input` - Input byte slice containing IFD entries
/// * `entry_count` - Number of entries to parse
/// * `byte_order` - Byte order for parsing (LittleEndian or BigEndian)
///
/// # Returns
/// IResult with remaining input and vector of parsed IFD entries
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
///
/// Each entry is 12 bytes:
/// - 2 bytes: tag ID
/// - 2 bytes: field type
/// - 4 bytes: value count
/// - 4 bytes: value offset
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
///
/// Each entry is 12 bytes:
/// - 2 bytes: tag ID
/// - 2 bytes: field type
/// - 4 bytes: value count
/// - 4 bytes: value offset
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
/// Handles both inline strings (<=4 bytes in value_offset) and
/// longer strings stored at an offset in the data.
///
/// # Arguments
/// * `entry` - The IFD entry containing string metadata
/// * `full_data` - Complete MakerNote data buffer
/// * `base_offset` - Base offset for calculating absolute positions
///
/// # Returns
/// Some(String) if extraction succeeds, None otherwise
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

/// Extract i32 array with base offset support
///
/// Wrapper around the shared extract_i32_array that handles base offset.
/// Olympus MakerNotes have a base offset of 8 bytes ("OLYMPUS\0").
///
/// # Arguments
/// * `entry` - The IFD entry containing array metadata
/// * `data` - Complete MakerNote data buffer
/// * `byte_order` - Byte order for parsing integers
///
/// # Returns
/// Some(Vec<i32>) if extraction succeeds, None otherwise
fn extract_i32_array_with_offset(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
) -> Option<Vec<i32>> {
    // For Olympus, the value_offset is relative to offset 8 (after "OLYMPUS\0")
    // Create a new entry with absolute offset for the shared extractor
    let absolute_offset = (entry.value_offset as usize) + 8;
    if absolute_offset > data.len() {
        return None;
    }

    // Use the shared extractor with the adjusted offset
    let adjusted_entry = IfdEntry {
        tag_id: entry.tag_id,
        field_type: entry.field_type,
        value_count: entry.value_count,
        value_offset: absolute_offset as u32,
    };

    extract_i32_array(&adjusted_entry, data, byte_order)
}

/// Extracts u8 array from IFD entry
///
/// Reads a sequence of bytes from the MakerNote data.
/// Used for Equipment array and other byte-level structures.
///
/// # Arguments
/// * `entry` - The IFD entry containing array metadata
/// * `full_data` - Complete MakerNote data buffer
/// * `base_offset` - Base offset for calculating absolute positions
///
/// # Returns
/// Some(Vec<u8>) if extraction succeeds, None otherwise
fn extract_u8_array(entry: &IfdEntry, full_data: &[u8], base_offset: usize) -> Option<Vec<u8>> {
    let count = entry.value_count as usize;
    let offset = (entry.value_offset as usize) + base_offset;

    if offset + count > full_data.len() {
        return None;
    }

    Some(full_data[offset..offset + count].to_vec())
}

// ============================================================================
// Unit Tests
// ============================================================================

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
        assert_eq!(QUALITY_DECODER.decode(1), "SQ (Standard Quality)");
        assert_eq!(QUALITY_DECODER.decode(2), "HQ (High Quality)");
        assert_eq!(QUALITY_DECODER.decode(3), "SHQ (Super High Quality)");
        assert_eq!(QUALITY_DECODER.decode(4), "RAW");
    }

    #[test]
    fn test_decode_exposure_mode() {
        assert_eq!(EXPOSURE_MODE_DECODER.decode(1), "Manual");
        assert_eq!(EXPOSURE_MODE_DECODER.decode(2), "Program");
        assert_eq!(EXPOSURE_MODE_DECODER.decode(3), "Aperture Priority");
        assert_eq!(EXPOSURE_MODE_DECODER.decode(4), "Shutter Priority");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(FOCUS_MODE_DECODER.decode(0), "Single AF");
        assert_eq!(FOCUS_MODE_DECODER.decode(2), "Continuous AF");
        assert_eq!(FOCUS_MODE_DECODER.decode(3), "Manual Focus");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(WHITE_BALANCE_DECODER.decode(0), "Auto");
        assert_eq!(WHITE_BALANCE_DECODER.decode(18), "5300K (Fine Weather)");
        assert_eq!(WHITE_BALANCE_DECODER.decode(23), "5500K (Flash)");
    }

    #[test]
    fn test_decode_scene_mode() {
        assert_eq!(SCENE_MODE_DECODER.decode(0), "Standard");
        assert_eq!(SCENE_MODE_DECODER.decode(8), "Portrait");
        assert_eq!(SCENE_MODE_DECODER.decode(9), "Landscape");
        assert_eq!(SCENE_MODE_DECODER.decode(21), "Macro");
        assert_eq!(SCENE_MODE_DECODER.decode(22), "Super Macro");
    }

    #[test]
    fn test_decode_picture_mode() {
        assert_eq!(PICTURE_MODE_DECODER.decode(1), "Vivid");
        assert_eq!(PICTURE_MODE_DECODER.decode(2), "Natural");
        assert_eq!(PICTURE_MODE_DECODER.decode(5), "i-Enhance");
    }

    #[test]
    fn test_decode_art_filter() {
        assert_eq!(ART_FILTER_DECODER.decode(0), "Off");
        assert_eq!(ART_FILTER_DECODER.decode(2), "Pop Art");
        assert_eq!(ART_FILTER_DECODER.decode(9), "Diorama");
        assert_eq!(ART_FILTER_DECODER.decode(24), "Watercolor");
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
