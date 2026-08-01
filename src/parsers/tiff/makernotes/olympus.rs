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
pub mod lookups;
pub mod tables;

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
pub use super::shared::table_ifd as ifd;

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
// Type 1 (older cameras): "OLYMP\0" followed by a two-byte version. ExifTool's
// MakerNoteOlympus condition is just /^OLYMP\0/ -- the older constants here
// spelled 8-byte headers as 7-byte literals ("OLYMP\x00\x01"), so the
// `data[0..8] == LITERAL` comparison could never be true and every type-1
// Olympus JPEG (163 of the 315 in the corpus) was rejected outright.
const OLYMPUS_HEADER_TYPE1: &[u8] = b"OLYMP\x00";

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

        // Check for Type 1 headers: "OLYMP\0" plus a two-byte version.
        if data.len() >= 8 && &data[0..6] == OLYMPUS_HEADER_TYPE1 {
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
        self.parse_with_model(data, byte_order, None, tags)
    }

    fn parse_with_model(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_located(data, byte_order, model, None, tags)
    }

    fn parse_with_context(
        &self,
        ctx: &crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // `window()` starts on the same byte as `payload()` but reaches to the
        // end of the enclosing TIFF block, which is where the older `OLYMP\0`
        // MakerNotes keep their values -- OlympusD450Z.jpg declares 406 bytes
        // and addresses its CameraID a further kilobyte along.
        self.parse_located(
            ctx.window(),
            byte_order,
            model,
            ctx.payload_tiff_offset(),
            tags,
        )
    }
}

impl OlympusParser {
    fn parse_located(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        model: Option<&str>,
        data_base: Option<u32>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        if data.is_empty() {
            return Ok(());
        }

        // Validate Olympus header
        if !self.validate_header(data) {
            return Err("Invalid Olympus MakerNote header".to_string());
        }

        let (ifd_start, effective_byte_order) = detect_header_type_and_offsets(data, byte_order)?;

        let Some(entries) = ifd::read_ifd(data, ifd_start, effective_byte_order) else {
            return Ok(());
        };

        // Type 2 ("OLYMPUS\0II") stores value offsets relative to the
        // MakerNote itself (ExifTool: `Base => '$start - 12'`), so no
        // correction is needed. Type 1 ("OLYMP\0") measures them from the TIFF
        // header, so the correction is exactly minus the block's own
        // TIFF-relative offset -- and when the caller could not supply it,
        // out-of-line values stay unread. A structural guess is not safe here:
        // OlympusFE100.jpg keeps its values inside the MakerNote (correction
        // -1298) while OlympusD450Z.jpg keeps them outside it (real offset 892,
        // values at 1440+ past the 406-byte block), and the two are
        // indistinguishable from the slice alone.
        let base: Option<i64> = if ifd_start == 8 {
            data_base.map(|off| -(i64::from(off)))
        } else {
            Some(0)
        };

        ifd::walk_directory(
            data,
            ifd_start,
            base,
            effective_byte_order,
            "Olympus",
            tables::MAIN,
            tags,
        );

        for entry in &entries {
            let table: &[ifd::TagDef] = match entry.tag_id {
                OLYMPUS_EQUIPMENT_SUBIFD => tables::EQUIPMENT,
                OLYMPUS_CAMERA_SETTINGS_SUBIFD => tables::CAMERA_SETTINGS,
                OLYMPUS_RAW_DEVELOPMENT_SUBIFD => tables::RAW_DEVELOPMENT,
                OLYMPUS_RAW_DEV2_SUBIFD => tables::RAW_DEVELOPMENT2,
                OLYMPUS_IMAGE_PROCESSING_SUBIFD => tables::IMAGE_PROCESSING,
                OLYMPUS_FOCUS_INFO_SUBIFD => tables::FOCUS_INFO,
                OLYMPUS_RAW_INFO_SUBIFD => tables::RAW_INFO,
                _ => continue,
            };
            // Sub-IFD pointers are ordinary LONG/IFD values, so the pointer
            // itself is subject to the same base correction as any other
            // offset.
            // A sub-IFD pointer is an ordinary LONG value and needs the same
            // base correction as any other offset. Type-2 MakerNotes (the only
            // ones with sub-IFDs) always resolve, so `base` is Some here.
            let Some(start) = base
                .and_then(|b| i64::from(entry.value_offset).checked_add(b))
                .filter(|s| *s >= 0)
                .map(|s| s as usize)
            else {
                continue;
            };
            ifd::walk_directory(
                data,
                start,
                base,
                effective_byte_order,
                "Olympus",
                table,
                tags,
            );

            if entry.tag_id == OLYMPUS_FOCUS_INFO_SUBIFD {
                parse_focus_info_sensor_temperature(
                    data,
                    start,
                    base,
                    effective_byte_order,
                    model,
                    tags,
                );
            }
        }

        Ok(())
    }
}

/// Perl's `/<needle>\b/` against a model string: `needle` must appear and must
/// not be followed by another word character. `E-1` therefore matches `E-1`
/// but not `E-10` or `E-100RS`.
fn model_word_matches(model: &str, needle: &str) -> bool {
    let bytes = model.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = model[from..].find(needle) {
        let end = from + rel + needle.len();
        let next_is_word = bytes
            .get(end)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_');
        if !next_is_word {
            return true;
        }
        from += rel + 1;
    }
    false
}

/// FocusInfo 0x1500 SensorTemperature has two ExifTool conversions selected by
/// the camera model and the value count:
/// the multi-value / E-1 / E-M5 form prints `"<vals> C"` with a trailing
/// `" 0 0"` removed, everything else prints `sprintf("%.1f C", 84 - 3*$val/26)`.
fn parse_focus_info_sensor_temperature(
    data: &[u8],
    ifd_start: usize,
    base: Option<i64>,
    order: ByteOrder,
    model: Option<&str>,
    tags: &mut HashMap<String, String>,
) {
    let Some(entries) = ifd::read_ifd(data, ifd_start, order) else {
        return;
    };
    let Some(entry) = entries.iter().find(|e| e.tag_id == 0x1500) else {
        return;
    };
    let Some(val) = ifd::decode_entry(data, entry, base, order, None) else {
        return;
    };
    let model = model.unwrap_or("").trim();
    let multi_form =
        entry.count != 1 || model_word_matches(model, "E-1") || model_word_matches(model, "E-M5");
    let printed = if multi_form {
        let raw = val.print_raw();
        format!("{} C", raw.strip_suffix(" 0 0").unwrap_or(&raw))
    } else {
        let Some(n) = val.first_int() else { return };
        format!("{:.1} C", 84.0 - 3.0 * n as f64 / 26.0)
    };
    tags.insert("Olympus:SensorTemperature".to_string(), printed);
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
/// Tuple of (ifd_start_offset, effective_byte_order)
///
/// ExifTool's `MakerNoteOlympus2` uses `Start => '$valuePtr + 12'` -- the
/// 12-byte header is `"OLYMPUS\0"` (8) + `"II"`/`"MM"` (2) + a version word
/// (2). The version word is NOT an IFD offset: reading it as one landed the
/// directory at byte 11, where the entry count decodes as 1792 and the whole
/// MakerNote was discarded as implausible, which is why Olympus JPEGs used to
/// yield no MakerNotes tags at all.
fn detect_header_type_and_offsets(
    data: &[u8],
    default_byte_order: ByteOrder,
) -> std::result::Result<(usize, ByteOrder), String> {
    // Check Type 2 headers first (they're longer and more specific)
    if data.len() >= 12 {
        if &data[0..10] == OLYMPUS_HEADER {
            return Ok((12, ByteOrder::LittleEndian));
        }
        if &data[0..10] == OLYMPUS_HEADER_BE {
            return Ok((12, ByteOrder::BigEndian));
        }
    }

    // Check Type 1 headers: ExifTool's `Start => '$valuePtr + 8'`.
    if data.len() >= 8 && &data[0..6] == OLYMPUS_HEADER_TYPE1 {
        return Ok((8, default_byte_order));
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
