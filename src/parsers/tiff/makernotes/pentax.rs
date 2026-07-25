//! Pentax MakerNote Parser
//!
//! Parses Pentax-specific EXIF MakerNote tags containing camera settings,
//! lens information, image quality parameters, and other proprietary metadata.
//!
//! Supports all Pentax DSLR and mirrorless cameras including:
//! - K-series DSLRs (K-1, K-3, K-5, K-7, K-x, K-r, etc.)
//! - Q-series mirrorless (Q, Q7, Q10, Q-S1)
//! - istD/ist series legacy DSLRs
//!
//! Based on ExifTool's Pentax.pm module.
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

use super::pentax_lens_database::{lookup_lens_name, lookup_lens_type_pair};
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use super::shared::generic_decoders::ON_OFF;

// Import declarative decoder macros
use crate::const_decoder;

// Import registry
use super::registries::pentax::pentax_registry;

// Pentax MakerNote header signatures
// Pentax typically uses "AOC\0" (4 bytes) or no header
const PENTAX_HEADER_AOC: &[u8] = b"AOC\0";
const PENTAX_HEADER_PENTAX: &[u8] = b"PENTAX \0";

// ============================================================================
// Tag ID Constants
// ============================================================================
// These constants define the tag IDs for all Pentax MakerNote tags.
// They are used for pattern matching in the parse function.

// Basic Camera Info (0x0000-0x000F)
const PENTAX_VERSION: u16 = 0x0000;
const PENTAX_PENTAX_MODEL_TYPE: u16 = 0x0001;
const PENTAX_PREVIEW_IMAGE_SIZE: u16 = 0x0002;
const PENTAX_PREVIEW_IMAGE_LENGTH: u16 = 0x0003;
const PENTAX_PREVIEW_IMAGE_START: u16 = 0x0004;
const PENTAX_PENTAX_MODEL_ID: u16 = 0x0005;
const PENTAX_DATE: u16 = 0x0006;
const PENTAX_TIME: u16 = 0x0007;
const PENTAX_QUALITY: u16 = 0x0008;
const PENTAX_PENTAX_IMAGE_SIZE: u16 = 0x0009;
const PENTAX_PICTURE_MODE: u16 = 0x000B;
const PENTAX_FLASH_MODE: u16 = 0x000C;
const PENTAX_FOCUS_MODE: u16 = 0x000D;
const PENTAX_AF_POINT_SELECTED: u16 = 0x000E;
const PENTAX_AF_POINT_IN_FOCUS: u16 = 0x000F;

// Focus and Exposure (0x0010-0x001F)
const PENTAX_FOCUS_POSITION: u16 = 0x0010;
const PENTAX_EXPOSURE_TIME: u16 = 0x0012;
const PENTAX_FNUMBER: u16 = 0x0013;
const PENTAX_ISO_SPEED: u16 = 0x0014;
const PENTAX_LIGHT_READING: u16 = 0x0015;
const PENTAX_EXPOSURE_COMPENSATION: u16 = 0x0016;
const PENTAX_METERING_MODE: u16 = 0x0017;
const PENTAX_AUTO_BRACKETING: u16 = 0x0018;
const PENTAX_WHITE_BALANCE: u16 = 0x0019;
const PENTAX_WHITE_BALANCE_MODE: u16 = 0x001A;
const PENTAX_BLUE_BALANCE: u16 = 0x001B;
const PENTAX_RED_BALANCE: u16 = 0x001C;
const PENTAX_FOCAL_LENGTH: u16 = 0x001D;
const PENTAX_DIGITAL_ZOOM: u16 = 0x001E;
const PENTAX_SATURATION: u16 = 0x001F;

// Image Adjustments (0x0020-0x002F)
const PENTAX_CONTRAST: u16 = 0x0020;
const PENTAX_SHARPNESS: u16 = 0x0021;
const PENTAX_WORLD_TIME_LOCATION: u16 = 0x0022;
const PENTAX_HOMETOWN_CITY: u16 = 0x0023;
const PENTAX_DESTINATION_CITY: u16 = 0x0024;
const PENTAX_HOMETOWN_DST: u16 = 0x0025;
const PENTAX_DESTINATION_DST: u16 = 0x0026;
const PENTAX_DSP_FIRMWARE_VERSION: u16 = 0x0027;
const PENTAX_CPU_FIRMWARE_VERSION: u16 = 0x0028;
const PENTAX_FRAME_NUMBER: u16 = 0x0029;
const PENTAX_EFFECTIVE_LV: u16 = 0x002D;

// Camera Settings (0x0030-0x004F)
const PENTAX_IMAGE_PROCESSING: u16 = 0x0032;
const PENTAX_PICTURE_MODE2: u16 = 0x0033;
const PENTAX_DRIVE_MODE: u16 = 0x0034;
const PENTAX_SENSOR_SIZE: u16 = 0x0035;
const PENTAX_COLOR_SPACE: u16 = 0x0037;
const PENTAX_IMAGE_AREA_OFFSET: u16 = 0x0038;
const PENTAX_RAW_IMAGE_SIZE: u16 = 0x0039;
const PENTAX_BATTERY_LEVEL: u16 = 0x003B;
const PENTAX_AF_POINTS_IN_FOCUS_2: u16 = 0x003C;
const PENTAX_DATA_SCALING: u16 = 0x003D;
const PENTAX_PREVIEW_IMAGE_BORDERS: u16 = 0x003E;
const PENTAX_LENS_TYPE: u16 = 0x003F;
const PENTAX_SENSITIVITY_ADJUST: u16 = 0x0040;
const PENTAX_IMAGE_EDIT_COUNT: u16 = 0x0041;
const PENTAX_CAMERA_TEMPERATURE: u16 = 0x0047;
const PENTAX_AE_LOCK: u16 = 0x0048;
const PENTAX_NOISE_REDUCTION: u16 = 0x0049;
const PENTAX_FLASH_EXPOSURE_COMP: u16 = 0x004D;
const PENTAX_IMAGE_TONE: u16 = 0x004F;

// Color and Processing (0x0050-0x006F)
const PENTAX_COLOR_TEMPERATURE: u16 = 0x0050;
const PENTAX_SHAKE_REDUCTION: u16 = 0x005C;
const PENTAX_SHUTTER_COUNT: u16 = 0x005D;
const PENTAX_FACE_INFO: u16 = 0x0060;
const PENTAX_RAW_DEVELOPMENT_PROCESS: u16 = 0x0062;
const PENTAX_HUE: u16 = 0x0067;
const PENTAX_AWB_INFO: u16 = 0x0068;
const PENTAX_DYNAMIC_RANGE_EXPANSION: u16 = 0x0069;
const PENTAX_TIME_INFO: u16 = 0x006B;
const PENTAX_HIGH_LOW_KEY_ADJ: u16 = 0x006C;
const PENTAX_CONTRAST_HIGHLIGHT: u16 = 0x006D;
const PENTAX_CONTRAST_SHADOW: u16 = 0x006E;
const PENTAX_CONTRAST_HIGHLIGHT_SHADOW_ADJ: u16 = 0x006F;

// Advanced Features (0x0070-0x009F)
const PENTAX_FINE_SHARPNESS: u16 = 0x0070;
const PENTAX_HIGH_ISO_NOISE_REDUCTION: u16 = 0x0071;
const PENTAX_AF_ADJUSTMENT: u16 = 0x0072;
const PENTAX_MONOCHROME_FILTER_EFFECT: u16 = 0x0073;
const PENTAX_MONOCHROME_TONING: u16 = 0x0074;
const PENTAX_FACE_DETECT: u16 = 0x0076;
const PENTAX_FACE_DETECT_FRAME_SIZE: u16 = 0x0077;
const PENTAX_SHADOW_CORRECTION: u16 = 0x0079;
const PENTAX_ISO_AUTO_PARAMETERS: u16 = 0x007A;
const PENTAX_CROSS_PROCESS: u16 = 0x007B;
const PENTAX_LENS_CORR: u16 = 0x007D;
const PENTAX_WHITE_LEVEL: u16 = 0x007E;
const PENTAX_LENS_INFO: u16 = 0x007F;
const PENTAX_AF_INFO: u16 = 0x0080;
const PENTAX_ASPECT_RATIO: u16 = 0x0082;
const PENTAX_HDR: u16 = 0x0085;
const PENTAX_PIXEL_SHIFT_RESOLUTION: u16 = 0x0086;
const PENTAX_SHUTTER_TYPE: u16 = 0x0087;
const PENTAX_NEUTRAL_DENSITY_FILTER: u16 = 0x0088;
const PENTAX_ISO2: u16 = 0x008B;
const PENTAX_INTERVAL_SHOOTING: u16 = 0x0092;
const PENTAX_SKIN_TONE_CORRECTION: u16 = 0x0095;
const PENTAX_CLARITY_CONTROL: u16 = 0x0096;
const PENTAX_LENS_MODEL: u16 = 0x009F;

// Extended (0x02xx) tags, mostly used by AVI videos and newer DSLRs.
const PENTAX_CAMERA_SETTINGS: u16 = 0x0205;
const PENTAX_AE_INFO: u16 = 0x0206;
const PENTAX_LENS_INFO_207: u16 = 0x0207;
const PENTAX_CAMERA_INFO: u16 = 0x0215;
const PENTAX_COLOR_INFO: u16 = 0x0222;
const PENTAX_SERIAL_NUMBER: u16 = 0x0229;
const PENTAX_ARTIST: u16 = 0x022E;
const PENTAX_COPYRIGHT: u16 = 0x022F;
const PENTAX_FIRMWARE_VERSION_VIDEO: u16 = 0x0230;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================
// Using const_decoder! macro to eliminate decoder function duplication

// Quality setting decoder - maps numeric values to quality modes
const_decoder!(pub QUALITY,
    i32,
    [
        (0, "Good"),
        (1, "Better"),
        (2, "Best"),
        (3, "TIFF"),
        (4, "RAW"),
        (5, "Premium"),
        (6, "RAW + JPEG"),
        (7, "RAW + Premium"),
        (8, "RAW + Better"),
        (9, "RAW + Good"),
    ]
);

// Picture mode decoder - maps values to shooting scene modes
const_decoder!(pub PICTURE_MODE,
    i32,
    [
        (0, "Program"),
        (1, "Shutter Priority"),
        (2, "Aperture Priority"),
        (3, "Manual"),
        (4, "Portrait"),
        (5, "Landscape"),
        (6, "Macro"),
        (7, "Sport"),
        (8, "Night Scene Portrait"),
        (9, "No Flash"),
        (10, "Night Scene"),
        (11, "Surf & Snow"),
        (12, "Text"),
        (13, "Sunset"),
        (14, "Kids"),
        (15, "Pet"),
        (16, "Candlelight"),
        (17, "Museum"),
        (18, "Food"),
        (19, "Stage Lighting"),
        (20, "Night Snap"),
        (21, "Blue Sky"),
        (22, "Forest"),
    ]
);

// Flash mode decoder - maps values to flash modes
const_decoder!(pub FLASH_MODE,
    i32,
    [
        (0, "Auto"),
        (1, "Flash On"),
        (2, "Flash Off"),
        (3, "Red-eye Reduction"),
        (4, "Auto + Red-eye"),
        (5, "On + Red-eye"),
        (6, "Wireless"),
        (7, "Slow-sync"),
        (8, "Trailing-curtain Sync"),
    ]
);

// Focus mode decoder - maps values to autofocus modes
const_decoder!(pub FOCUS_MODE,
    i32,
    [
        (0, "Normal (AF)"),
        (1, "Macro (AF)"),
        (2, "Manual"),
        (3, "AF-S (Single)"),
        (4, "AF-C (Continuous)"),
        (5, "AF-A (Auto)"),
    ]
);

// Metering mode decoder - maps values to exposure metering modes
const_decoder!(pub METERING_MODE,
    i32,
    [
        (0, "Multi-segment"),
        (1, "Center-weighted Average"),
        (2, "Spot"),
        (3, "Average"),
        (4, "Highlight-weighted"),
    ]
);

// White balance decoder - maps values to white balance presets
// (matches ExifTool's Pentax::Main tag 0x0019 PrintConv)
const_decoder!(pub WHITE_BALANCE,
    i32,
    [
        (0, "Auto"),
        (1, "Daylight"),
        (2, "Shade"),
        (3, "Fluorescent"),
        (4, "Tungsten"),
        (5, "Manual"),
        (6, "Daylight Fluorescent"),
        (7, "Day White Fluorescent"),
        (8, "White Fluorescent"),
        (9, "Flash"),
        (10, "Cloudy"),
        (11, "Warm White Fluorescent"),
        (14, "Multi Auto"),
        (15, "Color Temperature Enhancement"),
        (17, "Kelvin"),
        (0xfffe, "Unknown"),
        (0xffff, "User-Selected"),
    ]
);

// White balance mode decoder - maps values to WB modes
const_decoder!(pub WHITE_BALANCE_MODE,
    i32,
    [
        (1, "Auto (Daylight)"),
        (2, "Auto (Shade)"),
        (3, "Auto (Flash)"),
        (4, "Auto (Tungsten)"),
        (6, "Auto (Daylight Fluorescent)"),
        (7, "Auto (Day White Fluorescent)"),
        (8, "Auto (White Fluorescent)"),
        (10, "Auto (Flash)"),
    ]
);

// Drive mode decoder - maps values to drive/shooting modes
const_decoder!(pub DRIVE_MODE,
    i32,
    [
        (0, "Single-frame"),
        (1, "Continuous"),
        (2, "Self-timer (12s)"),
        (3, "Self-timer (2s)"),
        (4, "Remote"),
        (5, "Exposure Bracketing"),
        (6, "Multiple Exposure"),
        (7, "Remote (3s delay)"),
        (8, "Continuous (Hi)"),
        (9, "Continuous (Lo)"),
        (10, "Continuous (Med)"),
        (11, "Interval Shooting"),
        (12, "Interval Composite"),
    ]
);

// Color space decoder - maps values to color space settings
const_decoder!(pub COLOR_SPACE, i32, [(0, "sRGB"), (1, "Adobe RGB"),]);

// Saturation decoder - maps values to saturation settings
// (matches ExifTool's Pentax::Main tag 0x001f PrintConv)
const_decoder!(pub SATURATION,
    i32,
    [
        (0, "-2 (low)"),
        (1, "0 (normal)"),
        (2, "+2 (high)"),
        (3, "-1 (medium low)"),
        (4, "+1 (medium high)"),
        (5, "-3 (very low)"),
        (6, "+3 (very high)"),
        (7, "-4 (minimum)"),
        (8, "+4 (maximum)"),
    ]
);

// Contrast decoder - maps values to contrast settings
// (matches ExifTool's Pentax::Main tag 0x0020 PrintConv)
const_decoder!(pub CONTRAST,
    i32,
    [
        (0, "-2 (low)"),
        (1, "0 (normal)"),
        (2, "+2 (high)"),
        (3, "-1 (medium low)"),
        (4, "+1 (medium high)"),
        (5, "-3 (very low)"),
        (6, "+3 (very high)"),
        (7, "-4 (minimum)"),
        (8, "+4 (maximum)"),
    ]
);

// Sharpness decoder - maps values to sharpness settings
// (matches ExifTool's Pentax::Main tag 0x0021 PrintConv)
const_decoder!(pub SHARPNESS,
    i32,
    [
        (0, "-2 (soft)"),
        (1, "0 (normal)"),
        (2, "+2 (hard)"),
        (3, "-1 (medium soft)"),
        (4, "+1 (medium hard)"),
        (5, "-3 (very soft)"),
        (6, "+3 (very hard)"),
        (7, "-4 (minimum)"),
        (8, "+4 (maximum)"),
    ]
);

// Shake reduction decoder - maps values to SR/stabilization modes
// Matches ExifTool's Pentax::SRInfo tag 1 (ShakeReduction) PrintConv.
const_decoder!(pub SHAKE_REDUCTION,
    i32,
    [
        (0, "Off"),
        (1, "On"),
        (4, "Off (4)"),
        (5, "On but Disabled"),
        (6, "On (Video)"),
        (7, "On (7)"),
        (15, "On (15)"),
        (39, "On (mode 2)"),
        (135, "On (135)"),
        (167, "On (mode 1)"),
    ]
);

// Image size decoder - maps values to resolution presets
const_decoder!(pub IMAGE_SIZE,
    i32,
    [
        (0, "640x480"),
        (1, "Full"),
        (2, "1024x768"),
        (3, "1280x960"),
        (4, "1600x1200"),
        (5, "2048x1536"),
        (8, "2560x1920"),
        (9, "3072x2304"),
        (10, "3264x2448"),
        (19, "320x240"),
        (20, "2288x1712"),
        (21, "2592x1944"),
        (22, "2304x1728"),
        (23, "3056x2296"),
        (25, "2816x2212"),
        (27, "3648x2736"),
        (36, "3008x2008"),
    ]
);

// Auto bracketing decoder - maps values to bracketing modes
const_decoder!(pub AUTO_BRACKETING, i32, [(0, "Off"), (1, "On"),]);

// World time location decoder - maps values to time zone selection
const_decoder!(pub WORLD_TIME_LOCATION,
    i32,
    [(0, "Hometown"), (1, "Destination"),]
);

// Pixel shift resolution decoder - maps values to PSR modes
const_decoder!(pub PIXEL_SHIFT_RESOLUTION,
    i32,
    [(0, "Off"), (1, "On"), (2, "On (Motion Correction)"),]
);

// DST (Daylight Saving Time) decoder
const_decoder!(pub DST, i32, [(0, "No"), (1, "Yes"),]);

// Image tone decoder
const_decoder!(pub IMAGE_TONE, i32, [
    (0, "Natural"), (1, "Bright"), (2, "Portrait"), (3, "Landscape"),
    (4, "Vibrant"), (5, "Monochrome"), (6, "Muted"), (7, "Reversal Film"),
    (8, "Bleach Bypass"), (9, "Radiant"), (10, "Cross Processing"),
    (11, "Flat"), (12, "Auto"),
]);

// Noise reduction decoder
const_decoder!(pub NOISE_REDUCTION, i32, [
    (0, "Off"), (1, "On (Weak)"), (2, "On"), (3, "On (Strong)"), (4, "Auto"),
]);

// High ISO noise reduction decoder
const_decoder!(pub HIGH_ISO_NOISE_REDUCTION, i32, [
    (0, "Off"), (1, "Weakest"), (2, "Weak"), (3, "Medium"),
    (4, "Strong"), (5, "Strongest"), (6, "Auto"),
]);

// AE Lock decoder
const_decoder!(pub AE_LOCK, i32, [(0, "Off"), (1, "On"),]);

// Dynamic range expansion decoder
const_decoder!(pub DYNAMIC_RANGE_EXPANSION, i32, [(0, "Off"), (1, "On"), (2, "Auto"),]);

// HDR decoder
const_decoder!(pub HDR, i32, [
    (0, "Off"), (1, "HDR Auto"), (2, "HDR 1"), (3, "HDR 2"),
    (4, "HDR 3"), (5, "Advanced HDR"),
]);

// Shadow correction decoder
const_decoder!(pub SHADOW_CORRECTION, i32, [
    (0, "Off"), (1, "On (Weak)"), (2, "On"), (3, "On (Strong)"), (4, "Auto"),
]);

// Fine sharpness decoder
const_decoder!(pub FINE_SHARPNESS, i32, [(0, "Off"), (1, "On"),]);

// Shutter type decoder
const_decoder!(pub SHUTTER_TYPE, i32, [(0, "Mechanical"), (1, "Electronic"),]);

// Neutral density filter decoder
const_decoder!(pub NEUTRAL_DENSITY_FILTER, i32, [(0, "Off"), (1, "On"),]);

// Monochrome filter effect decoder
const_decoder!(pub MONOCHROME_FILTER_EFFECT, i32, [
    (0, "None"), (1, "Yellow"), (2, "Orange"), (3, "Red"), (4, "Magenta"),
    (5, "Blue"), (6, "Cyan"), (7, "Green"), (8, "Yellow-green"), (9, "Infrared"),
]);

// Monochrome toning decoder
const_decoder!(pub MONOCHROME_TONING, i32, [
    (0, "None"), (1, "Sepia"), (2, "Blue"), (3, "Purple"), (4, "Green"),
]);

// Face detect decoder
const_decoder!(pub FACE_DETECT, i32, [(0, "Off"), (1, "On"), (256, "On (Smile/Blink)"),]);

// Cross process decoder
const_decoder!(pub CROSS_PROCESS, i32, [
    (0, "Off"), (1, "Random"), (2, "Preset 1"), (3, "Preset 2"), (4, "Preset 3"),
    (16, "Favorite 1"), (17, "Favorite 2"), (18, "Favorite 3"),
]);

// Aspect ratio decoder
const_decoder!(pub ASPECT_RATIO, i32, [(0, "4:3"), (1, "3:2"), (2, "16:9"), (3, "1:1"),]);

// Clarity control decoder
const_decoder!(pub CLARITY_CONTROL, i32, [
    (-4, "Very Low"), (-3, "Low 3"), (-2, "Low 2"), (-1, "Low 1"), (0, "Off"),
    (1, "High 1"), (2, "High 2"), (3, "High 3"), (4, "Very High"),
]);

// Skin tone correction decoder
const_decoder!(pub SKIN_TONE_CORRECTION, i32, [
    (0, "Off"), (1, "On (Type 1)"), (2, "On (Type 2)"),
]);

// Bleach bypass toning decoder
const_decoder!(pub BLEACH_BYPASS_TONING, i32, [
    (0, "Off"), (1, "Green"), (2, "Yellow"), (3, "Orange"),
]);

// Raw development process decoder
const_decoder!(pub RAW_DEVELOPMENT_PROCESS, i32, [
    (1, "Ver. 1"), (2, "Ver. 2"), (3, "Ver. 3"), (4, "Ver. 4"),
    (5, "Ver. 5"), (6, "Ver. 6"), (7, "Ver. 7"),
]);

// Lens correction decoder
const_decoder!(pub LENS_CORR, i32, [
    (0, "Off"), (1, "Distortion"), (2, "Chromatic Aberration"),
    (3, "Distortion + CA"), (4, "Peripheral Illumination"),
    (5, "Distortion + PI"), (6, "CA + PI"), (7, "Distortion + CA + PI"),
    (8, "Diffraction"),
]);

/// Checks if the provided data has a valid Pentax MakerNote header
///
/// # Arguments
/// * `data` - Raw MakerNote data to validate
///
/// # Returns
/// * `true` if data contains a valid Pentax header or appears to be Pentax MakerNote data
/// * `false` otherwise
pub fn is_pentax_makernote(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    // Check for AOC header (most common)
    if data.len() >= 4 && &data[0..4] == PENTAX_HEADER_AOC {
        return true;
    }

    // Check for PENTAX header (some models)
    if data.len() >= 8 && &data[0..8] == PENTAX_HEADER_PENTAX {
        return true;
    }

    // Some Pentax cameras have no header, just start with IFD
    // We'll validate by checking if first two bytes form a reasonable entry count
    if data.len() >= 2 {
        let reader = EndianReader::little_endian(data);
        let entry_count = reader.u16_at(0).unwrap_or(0);
        // Reasonable entry count: 1-200 entries
        if entry_count > 0 && entry_count < 200 {
            return true;
        }
    }

    false
}

/// Represents a Pentax MakerNote parser
pub struct PentaxParser;

impl MakerNoteParser for PentaxParser {
    fn manufacturer_name(&self) -> &'static str {
        "Pentax"
    }

    fn tag_prefix(&self) -> &'static str {
        "Pentax:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_pentax_makernote(data)
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

        // Validate Pentax header and determine IFD offset.
        // Note: the "PENTAX \0" header (used e.g. by the `hymn`/`mknt` chunks in AVI
        // videos, and MakerNotePentax5 in JPEG) is followed by a 2-byte byte-order
        // marker ("MM"/"II") and the IFD begins at offset 10 (see ExifTool
        // MakerNotes.pm / Pentax::AVI). The marker may differ from the container's
        // overall byte order, so detect and use it here.
        let mut byte_order = byte_order;
        let ifd_offset = if data.len() >= 4 && &data[0..4] == PENTAX_HEADER_AOC {
            // AOC header: skip 6 bytes (AOC\0 + 2-byte offset)
            6
        } else if data.len() >= 8 && &data[0..8] == PENTAX_HEADER_PENTAX {
            // PENTAX header: skip 8 bytes for the header, plus a 2-byte byte-order
            // marker; the IFD itself starts at offset 10.
            if data.len() >= 10 {
                if &data[8..10] == b"MM" {
                    byte_order = ByteOrder::BigEndian;
                } else if &data[8..10] == b"II" {
                    byte_order = ByteOrder::LittleEndian;
                }
            }
            10
        } else {
            // No header, IFD starts immediately
            0
        };

        if data.len() <= ifd_offset + 2 {
            return Ok(());
        }

        let ifd_data = &data[ifd_offset..];

        // Parse IFD entry count using EndianReader
        if ifd_data.len() < 2 {
            return Ok(());
        }
        let reader = EndianReader::new(ifd_data, byte_order.to_io_byte_order());
        let entry_count = reader.u16_at(0).unwrap_or(0);

        // Sanity check on entry count
        if entry_count == 0 || entry_count > 200 {
            return Ok(());
        }

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
                PENTAX_VERSION => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() == 4 {
                        tags.insert(
                            "Pentax:PentaxVersion".to_string(),
                            format!("{}.{}.{}.{}", raw[0], raw[1], raw[2], raw[3]),
                        );
                    }
                }

                PENTAX_DATE => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() == 4 {
                        // Year is always stored big-endian regardless of the
                        // MakerNote's overall byte order.
                        let year = u16::from_be_bytes([raw[0], raw[1]]);
                        tags.insert(
                            "Pentax:Date".to_string(),
                            format!("{:04}:{:02}:{:02}", year, raw[2], raw[3]),
                        );
                    }
                }

                PENTAX_TIME => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 3 {
                        tags.insert(
                            "Pentax:Time".to_string(),
                            format!("{:02}:{:02}:{:02}", raw[0], raw[1], raw[2]),
                        );
                    }
                }

                PENTAX_LENS_MODEL => {
                    if let Some(value) = extract_string_value(&entry, data, ifd_offset) {
                        tags.insert("Pentax:LensModel".to_string(), value);
                    }
                }

                // Decoded value tags using const decoders
                PENTAX_QUALITY => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:Quality".to_string(), QUALITY.decode(value));
                }

                PENTAX_PICTURE_MODE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:PictureMode".to_string(), PICTURE_MODE.decode(value));
                }

                PENTAX_FLASH_MODE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:FlashMode".to_string(), FLASH_MODE.decode(value));
                }

                PENTAX_FOCUS_MODE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:FocusMode".to_string(), FOCUS_MODE.decode(value));
                }

                PENTAX_METERING_MODE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:MeteringMode".to_string(),
                        METERING_MODE.decode(value),
                    );
                }

                PENTAX_WHITE_BALANCE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:WhiteBalance".to_string(),
                        WHITE_BALANCE.decode(value),
                    );
                }

                PENTAX_WHITE_BALANCE_MODE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:WhiteBalanceMode".to_string(),
                        WHITE_BALANCE_MODE.decode(value),
                    );
                }

                PENTAX_SATURATION => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:Saturation".to_string(), SATURATION.decode(value));
                }

                PENTAX_CONTRAST => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:Contrast".to_string(), CONTRAST.decode(value));
                }

                PENTAX_SHARPNESS => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:Sharpness".to_string(), SHARPNESS.decode(value));
                }

                PENTAX_DRIVE_MODE => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 4 {
                        let parts = [
                            decode_drive_mode_byte0(raw[0]),
                            decode_drive_mode_byte1(raw[1]),
                            decode_drive_mode_byte2(raw[2]),
                            decode_drive_mode_byte3(raw[3]),
                        ];
                        tags.insert("Pentax:DriveMode".to_string(), parts.join("; "));
                    }
                }

                PENTAX_COLOR_SPACE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:ColorSpace".to_string(), COLOR_SPACE.decode(value));
                }

                // Note: Former SHAKE_REDUCTION_INFO at 0x003C is now AF_POINTS_IN_FOCUS_2
                // Shake reduction is now at 0x005C - handled below
                PENTAX_PENTAX_IMAGE_SIZE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:ImageSize".to_string(), IMAGE_SIZE.decode(value));
                }

                PENTAX_AUTO_BRACKETING => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:AutoBracketing".to_string(),
                        AUTO_BRACKETING.decode(value),
                    );
                }

                PENTAX_WORLD_TIME_LOCATION => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:WorldTimeLocation".to_string(),
                        WORLD_TIME_LOCATION.decode(value),
                    );
                }

                PENTAX_PIXEL_SHIFT_RESOLUTION => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:PixelShiftResolution".to_string(),
                        PIXEL_SHIFT_RESOLUTION.decode(value),
                    );
                }

                // Numeric value tags (no decoding needed)
                PENTAX_AF_POINT_SELECTED => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    if (0..=65535).contains(&value) {
                        tags.insert("Pentax:AFPointSelected".to_string(), value.to_string());
                    }
                }

                PENTAX_AF_POINT_IN_FOCUS => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    if (0..=65535).contains(&value) {
                        tags.insert("Pentax:AFPointInFocus".to_string(), value.to_string());
                    }
                }

                PENTAX_ISO_SPEED => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:ISO".to_string(), value.to_string());
                }

                PENTAX_BLUE_BALANCE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:BlueBalance".to_string(), value.to_string());
                }

                PENTAX_RED_BALANCE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:RedBalance".to_string(), value.to_string());
                }

                PENTAX_FOCAL_LENGTH => {
                    let value = entry.value_offset;
                    tags.insert(
                        "Pentax:FocalLength".to_string(),
                        format!("{:.1} mm", value as f32 / 100.0),
                    );
                }

                PENTAX_DIGITAL_ZOOM => {
                    let value = entry.value_offset;
                    if value > 0 {
                        tags.insert(
                            "Pentax:DigitalZoom".to_string(),
                            format!("{:.2}x", value as f32 / 100.0),
                        );
                    }
                }

                PENTAX_SHUTTER_COUNT => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:ShutterCount".to_string(), value.to_string());
                }

                // 0x003f "LensRec" subdirectory: LensType (2 bytes: series, sub-id)
                // followed by one or two unknown bytes, then ExtenderStatus at
                // offset 3 (see ExifTool Pentax::LensRec).
                PENTAX_LENS_TYPE => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 2 {
                        let series = raw[0];
                        let sub_id = raw[1] as u16;
                        let name = lookup_lens_type_pair(series, sub_id)
                            .unwrap_or_else(|| format!("Unknown ({} {})", series, sub_id));
                        tags.insert("Pentax:LensType".to_string(), name);
                    }
                    if raw.len() >= 4 {
                        let extender = if raw[3] == 0 {
                            "Not attached"
                        } else {
                            "Attached"
                        };
                        tags.insert("Pentax:ExtenderStatus".to_string(), extender.to_string());
                    }
                }

                PENTAX_PENTAX_MODEL_TYPE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:PentaxModelType".to_string(), value.to_string());
                }

                PENTAX_PENTAX_MODEL_ID => {
                    let value = entry.value_offset;
                    let name = pentax_model_id_name(value)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("Unknown ({:#x})", value));
                    tags.insert("Pentax:PentaxModelID".to_string(), name);
                }

                PENTAX_PREVIEW_IMAGE_SIZE => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:PreviewImageSize".to_string(), value.to_string());
                }

                PENTAX_PREVIEW_IMAGE_LENGTH => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:PreviewImageLength".to_string(), value.to_string());
                }

                PENTAX_CAMERA_TEMPERATURE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:CameraTemperature".to_string(),
                        format!("{} C", value),
                    );
                }

                PENTAX_BATTERY_LEVEL => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:BatteryLevel".to_string(), format!("{}%", value));
                }

                PENTAX_HOMETOWN_CITY => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:HometownCity".to_string(), value.to_string());
                }

                PENTAX_DESTINATION_CITY => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:DestinationCity".to_string(), value.to_string());
                }

                // NOTE: despite the constant name, tag 0x0033 is ExifTool's
                // "PictureMode" (a 3-byte array); the unrelated "PictureMode2"
                // tag comes from the CameraSettings (0x0205) binary subdirectory.
                PENTAX_PICTURE_MODE2 => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 3 {
                        if let Some(value) = decode_picture_mode_0x0033(raw[0], raw[1], raw[2]) {
                            tags.insert("Pentax:PictureMode".to_string(), value);
                        }
                    }
                }

                // Focus and Exposure tags
                PENTAX_FOCUS_POSITION => {
                    let value = entry.value_offset;
                    tags.insert("Pentax:FocusPosition".to_string(), value.to_string());
                }
                PENTAX_EXPOSURE_TIME => {
                    tags.insert(
                        "Pentax:ExposureTime".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_FNUMBER => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:FNumber".to_string(),
                        format!("{:.1}", value as f32 / 10.0),
                    );
                }
                PENTAX_LIGHT_READING => {
                    tags.insert(
                        "Pentax:LightReading".to_string(),
                        (entry.value_offset as i32).to_string(),
                    );
                }
                PENTAX_EXPOSURE_COMPENSATION => {
                    let raw = extract_value_as_i32(&entry, byte_order);
                    let value = (raw - 50) as f32 / 10.0;
                    let formatted = if value == 0.0 {
                        "0".to_string()
                    } else {
                        format!("{:+.1}", value)
                    };
                    tags.insert("Pentax:ExposureCompensation".to_string(), formatted);
                }

                // Image Adjustments
                PENTAX_HOMETOWN_DST => {
                    tags.insert(
                        "Pentax:HometownDST".to_string(),
                        DST.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_DESTINATION_DST => {
                    tags.insert(
                        "Pentax:DestinationDST".to_string(),
                        DST.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_DSP_FIRMWARE_VERSION => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if let Some(value) = decode_firmware_id(&raw) {
                        tags.insert("Pentax:DSPFirmwareVersion".to_string(), value);
                    }
                }
                PENTAX_CPU_FIRMWARE_VERSION => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if let Some(value) = decode_firmware_id(&raw) {
                        tags.insert("Pentax:CPUFirmwareVersion".to_string(), value);
                    }
                }
                PENTAX_FRAME_NUMBER => {
                    tags.insert(
                        "Pentax:FrameNumber".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_EFFECTIVE_LV => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:EffectiveLV".to_string(),
                        format!("{:.1}", value as f32 / 10.0),
                    );
                }

                // Camera Settings
                PENTAX_IMAGE_PROCESSING => {
                    tags.insert(
                        "Pentax:ImageProcessing".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_SENSOR_SIZE => {
                    tags.insert(
                        "Pentax:SensorSize".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_IMAGE_AREA_OFFSET => {
                    tags.insert(
                        "Pentax:ImageAreaOffset".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_RAW_IMAGE_SIZE => {
                    tags.insert(
                        "Pentax:RawImageSize".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_AF_POINTS_IN_FOCUS_2 => {
                    tags.insert(
                        "Pentax:AFPointsInFocus2".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_DATA_SCALING => {
                    tags.insert(
                        "Pentax:DataScaling".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_PREVIEW_IMAGE_BORDERS => {
                    tags.insert(
                        "Pentax:PreviewImageBorders".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_SENSITIVITY_ADJUST => {
                    tags.insert(
                        "Pentax:SensitivityAdjust".to_string(),
                        (entry.value_offset as i32).to_string(),
                    );
                }
                PENTAX_IMAGE_EDIT_COUNT => {
                    tags.insert(
                        "Pentax:ImageEditCount".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_AE_LOCK => {
                    tags.insert(
                        "Pentax:AELock".to_string(),
                        AE_LOCK.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_NOISE_REDUCTION => {
                    tags.insert(
                        "Pentax:NoiseReduction".to_string(),
                        NOISE_REDUCTION.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_FLASH_EXPOSURE_COMP => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:FlashExposureComp".to_string(),
                        format!("{:+.1} EV", value as f32 / 10.0),
                    );
                }
                PENTAX_IMAGE_TONE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:ImageTone".to_string(), IMAGE_TONE.decode(value));
                }

                // Color and Processing
                PENTAX_COLOR_TEMPERATURE => {
                    tags.insert(
                        "Pentax:ColorTemperature".to_string(),
                        format!("{}K", entry.value_offset),
                    );
                }
                // 0x005c "ShakeReductionInfo" subdirectory (SRInfo table): only
                // handle the 4-byte (count==4) form used by most DSLRs.
                PENTAX_SHAKE_REDUCTION => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 4 {
                        tags.insert(
                            "Pentax:SRResult".to_string(),
                            decode_sr_result_bitmask(raw[0]),
                        );
                        tags.insert(
                            "Pentax:ShakeReduction".to_string(),
                            SHAKE_REDUCTION.decode(raw[1] as i32),
                        );
                        let half_press = raw[2] as f64 / 60.0;
                        let suffix = if half_press > 254.5 / 60.0 {
                            " or longer"
                        } else {
                            ""
                        };
                        tags.insert(
                            "Pentax:SRHalfPressTime".to_string(),
                            format!("{:.2} s{}", half_press, suffix),
                        );
                        let focal = if raw[3] & 1 != 0 {
                            raw[3] as u32 * 4
                        } else {
                            raw[3] as u32 / 2
                        };
                        tags.insert("Pentax:SRFocalLength".to_string(), format!("{} mm", focal));
                    }
                }
                PENTAX_FACE_INFO => {
                    tags.insert(
                        "Pentax:FaceInfo".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_RAW_DEVELOPMENT_PROCESS => {
                    tags.insert(
                        "Pentax:RawDevelopmentProcess".to_string(),
                        RAW_DEVELOPMENT_PROCESS.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_HUE => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert("Pentax:Hue".to_string(), decode_hue(value));
                }
                PENTAX_AWB_INFO => {
                    tags.insert("Pentax:AWBInfo".to_string(), entry.value_offset.to_string());
                }
                PENTAX_DYNAMIC_RANGE_EXPANSION => {
                    tags.insert(
                        "Pentax:DynamicRangeExpansion".to_string(),
                        DYNAMIC_RANGE_EXPANSION.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_TIME_INFO => {
                    if let Some(value) = extract_string_value(&entry, data, ifd_offset) {
                        tags.insert("Pentax:TimeInfo".to_string(), value);
                    }
                }
                PENTAX_HIGH_LOW_KEY_ADJ => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 4 {
                        let (b0, b1) = match byte_order {
                            ByteOrder::BigEndian => (
                                i16::from_be_bytes([raw[0], raw[1]]),
                                i16::from_be_bytes([raw[2], raw[3]]),
                            ),
                            ByteOrder::LittleEndian => (
                                i16::from_le_bytes([raw[0], raw[1]]),
                                i16::from_le_bytes([raw[2], raw[3]]),
                            ),
                        };
                        let value = if b1 == 0 && (-4..=4).contains(&b0) {
                            b0.to_string()
                        } else {
                            format!("{} {}", b0, b1)
                        };
                        tags.insert("Pentax:HighLowKeyAdj".to_string(), value);
                    }
                }
                PENTAX_CONTRAST_HIGHLIGHT => {
                    tags.insert(
                        "Pentax:ContrastHighlight".to_string(),
                        (entry.value_offset as i32).to_string(),
                    );
                }
                PENTAX_CONTRAST_SHADOW => {
                    tags.insert(
                        "Pentax:ContrastShadow".to_string(),
                        (entry.value_offset as i32).to_string(),
                    );
                }
                PENTAX_CONTRAST_HIGHLIGHT_SHADOW_ADJ => {
                    tags.insert(
                        "Pentax:ContrastHighlightShadowAdj".to_string(),
                        (entry.value_offset as i32).to_string(),
                    );
                }

                // Advanced Features
                PENTAX_FINE_SHARPNESS => {
                    tags.insert(
                        "Pentax:FineSharpness".to_string(),
                        FINE_SHARPNESS.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_HIGH_ISO_NOISE_REDUCTION => {
                    tags.insert(
                        "Pentax:HighISONoiseReduction".to_string(),
                        HIGH_ISO_NOISE_REDUCTION.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_AF_ADJUSTMENT => {
                    tags.insert(
                        "Pentax:AFAdjustment".to_string(),
                        (entry.value_offset as i32).to_string(),
                    );
                }
                PENTAX_MONOCHROME_FILTER_EFFECT => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    let decoded = if value == 0xffff {
                        "None".to_string()
                    } else {
                        MONOCHROME_FILTER_EFFECT.decode(value)
                    };
                    tags.insert("Pentax:MonochromeFilterEffect".to_string(), decoded);
                }
                PENTAX_MONOCHROME_TONING => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    let decoded = if value == 0xffff {
                        "None".to_string()
                    } else {
                        MONOCHROME_TONING.decode(value)
                    };
                    tags.insert("Pentax:MonochromeToning".to_string(), decoded);
                }
                PENTAX_FACE_DETECT => {
                    tags.insert(
                        "Pentax:FaceDetect".to_string(),
                        FACE_DETECT.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_FACE_DETECT_FRAME_SIZE => {
                    tags.insert(
                        "Pentax:FaceDetectFrameSize".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_SHADOW_CORRECTION => {
                    tags.insert(
                        "Pentax:ShadowCorrection".to_string(),
                        SHADOW_CORRECTION.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_ISO_AUTO_PARAMETERS => {
                    tags.insert(
                        "Pentax:ISOAutoParameters".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_CROSS_PROCESS => {
                    let value = extract_value_as_i32(&entry, byte_order);
                    tags.insert(
                        "Pentax:CrossProcess".to_string(),
                        CROSS_PROCESS.decode(value),
                    );
                }
                PENTAX_LENS_CORR => {
                    tags.insert(
                        "Pentax:LensCorr".to_string(),
                        LENS_CORR.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_WHITE_LEVEL => {
                    tags.insert(
                        "Pentax:WhiteLevel".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_LENS_INFO => {
                    tags.insert(
                        "Pentax:LensInfo".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_AF_INFO => {
                    tags.insert("Pentax:AFInfo".to_string(), entry.value_offset.to_string());
                }
                PENTAX_ASPECT_RATIO => {
                    tags.insert(
                        "Pentax:AspectRatio".to_string(),
                        ASPECT_RATIO.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_HDR => {
                    tags.insert(
                        "Pentax:HDR".to_string(),
                        HDR.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_SHUTTER_TYPE => {
                    tags.insert(
                        "Pentax:ShutterType".to_string(),
                        SHUTTER_TYPE.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_NEUTRAL_DENSITY_FILTER => {
                    tags.insert(
                        "Pentax:NeutralDensityFilter".to_string(),
                        NEUTRAL_DENSITY_FILTER.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_ISO2 => {
                    tags.insert("Pentax:ISO2".to_string(), entry.value_offset.to_string());
                }
                PENTAX_INTERVAL_SHOOTING => {
                    tags.insert(
                        "Pentax:IntervalShooting".to_string(),
                        entry.value_offset.to_string(),
                    );
                }
                PENTAX_SKIN_TONE_CORRECTION => {
                    tags.insert(
                        "Pentax:SkinToneCorrection".to_string(),
                        SKIN_TONE_CORRECTION.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_CLARITY_CONTROL => {
                    tags.insert(
                        "Pentax:ClarityControl".to_string(),
                        CLARITY_CONTROL.decode(entry.value_offset as i32),
                    );
                }
                PENTAX_PREVIEW_IMAGE_START => {
                    tags.insert(
                        "Pentax:PreviewImageStart".to_string(),
                        entry.value_offset.to_string(),
                    );
                }

                // 0x0205 "CameraSettings" binary subdirectory. Only the
                // count<25 (non-K-01) layout is currently decoded.
                PENTAX_CAMERA_SETTINGS => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 11 && raw.len() < 25 {
                        tags.insert(
                            "Pentax:PictureMode2".to_string(),
                            decode_picture_mode2(raw[0]),
                        );
                        tags.insert(
                            "Pentax:ProgramLine".to_string(),
                            decode_program_line(raw[1] & 0x03),
                        );
                        tags.insert(
                            "Pentax:EVSteps".to_string(),
                            if raw[1] & 0x20 != 0 {
                                "1/3 EV Steps"
                            } else {
                                "1/2 EV Steps"
                            }
                            .to_string(),
                        );
                        tags.insert(
                            "Pentax:E-DialInProgram".to_string(),
                            if raw[1] & 0x40 != 0 {
                                "P Shift"
                            } else {
                                "Tv or Av"
                            }
                            .to_string(),
                        );
                        tags.insert(
                            "Pentax:ApertureRingUse".to_string(),
                            if raw[1] & 0x80 != 0 {
                                "Permitted"
                            } else {
                                "Prohibited"
                            }
                            .to_string(),
                        );
                        tags.insert(
                            "Pentax:FlashOptions".to_string(),
                            decode_flash_options((raw[2] & 0xf0) >> 4),
                        );
                        tags.insert(
                            "Pentax:MeteringMode2".to_string(),
                            decode_metering_mode2_bitmask(raw[2] & 0x0f),
                        );
                        tags.insert(
                            "Pentax:AFPointMode".to_string(),
                            decode_af_point_mode_bitmask((raw[3] & 0xf0) >> 4),
                        );
                        tags.insert(
                            "Pentax:FocusMode2".to_string(),
                            decode_focus_mode2(raw[3] & 0x0f),
                        );
                        if raw.len() >= 6 {
                            let sel = match byte_order {
                                ByteOrder::BigEndian => u16::from_be_bytes([raw[4], raw[5]]),
                                ByteOrder::LittleEndian => u16::from_le_bytes([raw[4], raw[5]]),
                            };
                            tags.insert(
                                "Pentax:AFPointSelected2".to_string(),
                                decode_af_point_selected2_bitmask(sel),
                            );
                        }
                        if raw.len() >= 7 {
                            let ev = pentax_ev(raw[6] as i32 - 32);
                            let iso_floor =
                                (100.0 * (ev * std::f64::consts::LN_2).exp() + 0.5) as i64;
                            tags.insert("Pentax:ISOFloor".to_string(), iso_floor.to_string());
                        }
                        if raw.len() >= 8 {
                            tags.insert(
                                "Pentax:DriveMode2".to_string(),
                                decode_drive_mode2_bitmask(raw[7]),
                            );
                        }
                        if raw.len() >= 9 {
                            tags.insert(
                                "Pentax:ExposureBracketStepSize".to_string(),
                                decode_exposure_bracket_step_size(raw[8]),
                            );
                        }
                        if raw.len() >= 10 {
                            tags.insert(
                                "Pentax:BracketShotNumber".to_string(),
                                decode_bracket_shot_number(raw[9]),
                            );
                        }
                        tags.insert(
                            "Pentax:WhiteBalanceSet".to_string(),
                            decode_white_balance_set((raw[10] & 0xf0) >> 4),
                        );
                        tags.insert(
                            "Pentax:MultipleExposureSet".to_string(),
                            if raw[10] & 0x0f != 0 { "On" } else { "Off" }.to_string(),
                        );
                    }
                }

                // 0x0206 "AEInfo" binary subdirectory (auto-exposure info for
                // most Pentax DSLR models). Field offsets from 8 onward are
                // shifted by 1 byte for models with a 24/25-byte record
                // (matching ExifTool's AEFlags `Hook`).
                PENTAX_AE_INFO => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() <= 25 && raw.len() != 21 && raw.len() >= 7 {
                        let shift: usize = if raw.len() > 20 { 1 } else { 0 };
                        let exposure_time = |b: u8| {
                            print_exposure_time(
                                24.0 * (-((b as f64) - 32.0) * std::f64::consts::LN_2 / 8.0).exp(),
                            )
                        };
                        tags.insert("Pentax:AEExposureTime".to_string(), exposure_time(raw[0]));
                        tags.insert(
                            "Pentax:AEAperture".to_string(),
                            format!("{:.1}", ae_aperture_from_raw(raw[1] as i32)),
                        );
                        let iso =
                            100.0 * ((raw[2] as f64 - 32.0) * std::f64::consts::LN_2 / 8.0).exp();
                        tags.insert(
                            "Pentax:AE_ISO".to_string(),
                            format!("{}", (iso + 0.5) as i64),
                        );
                        tags.insert(
                            "Pentax:AEXv".to_string(),
                            format_pentax_float((raw[3] as f64 - 64.0) / 8.0),
                        );
                        tags.insert(
                            "Pentax:AEBXv".to_string(),
                            format_pentax_float((raw[4] as i8) as f64 / 8.0),
                        );
                        tags.insert(
                            "Pentax:AEMinExposureTime".to_string(),
                            exposure_time(raw[5]),
                        );
                        tags.insert(
                            "Pentax:AEProgramMode".to_string(),
                            decode_ae_program_mode(raw[6]),
                        );

                        let idx = |base: usize| base + shift;
                        if raw.len() > idx(8) {
                            let v = raw[idx(8)];
                            tags.insert(
                                "Pentax:AEApertureSteps".to_string(),
                                if v == 255 {
                                    "n/a".to_string()
                                } else {
                                    v.to_string()
                                },
                            );
                        }
                        if raw.len() > idx(9) {
                            tags.insert(
                                "Pentax:AEMaxAperture".to_string(),
                                format!("{:.1}", ae_aperture_from_raw(raw[idx(9)] as i32)),
                            );
                        }
                        if raw.len() > idx(10) {
                            tags.insert(
                                "Pentax:AEMaxAperture2".to_string(),
                                format!("{:.1}", ae_aperture_from_raw(raw[idx(10)] as i32)),
                            );
                        }
                        if raw.len() > idx(11) {
                            tags.insert(
                                "Pentax:AEMinAperture".to_string(),
                                format!("{:.0}", ae_aperture_from_raw(raw[idx(11)] as i32)),
                            );
                        }
                        if raw.len() > idx(12) {
                            tags.insert(
                                "Pentax:AEMeteringMode".to_string(),
                                decode_ae_metering_mode_bitmask(raw[idx(12)]),
                            );
                        }
                        if raw.len() > idx(13) {
                            let b = raw[idx(13)];
                            tags.insert(
                                "Pentax:AEWhiteBalance".to_string(),
                                decode_ae_white_balance((b & 0xf0) >> 4),
                            );
                            tags.insert(
                                "Pentax:AEMeteringMode2".to_string(),
                                decode_metering_mode2_bitmask(b & 0x0f),
                            );
                        }
                        if raw.len() > idx(14) {
                            let ev = pentax_ev(raw[idx(14)] as i8 as i32);
                            let formatted = if ev == 0.0 {
                                "0".to_string()
                            } else {
                                format!("{:+.1}", ev)
                            };
                            tags.insert("Pentax:FlashExposureCompSet".to_string(), formatted);
                        }
                        if raw.len() > idx(21) {
                            let v = raw[idx(21)];
                            tags.insert(
                                "Pentax:LevelIndicator".to_string(),
                                if v == 90 {
                                    "n/a".to_string()
                                } else {
                                    v.to_string()
                                },
                            );
                        }
                    }
                }

                // 0x0207 "LensInfo"/"LensInfo2" + nested "LensData" binary
                // subdirectories. Only the LensInfo2 (K10D/K20D-style, 17-byte
                // LensData) layout used by most DSLRs is decoded.
                PENTAX_LENS_INFO_207 => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 4 {
                        let series = raw[0] & 0x0f;
                        let sub_id = (raw[2] as u16) * 256 + raw[3] as u16;
                        if let Some(name) = lookup_lens_type_pair(series, sub_id) {
                            tags.insert("Pentax:LensType".to_string(), name);
                        }
                    }
                    if raw.len() >= 4 + 17 {
                        let ld = &raw[4..4 + 17];
                        tags.insert(
                            "Pentax:AutoAperture".to_string(),
                            if ld[0] & 0x01 == 0 { "On" } else { "Off" }.to_string(),
                        );
                        tags.insert(
                            "Pentax:MinAperture".to_string(),
                            decode_min_aperture_index((ld[0] & 0x06) >> 1).to_string(),
                        );
                        let fstops_masked = ((ld[0] & 0x70) >> 4) as i32;
                        let fstops = 5 + (fstops_masked ^ 0x07) / 2;
                        tags.insert("Pentax:LensFStops".to_string(), fstops.to_string());
                        tags.insert(
                            "Pentax:MinFocusDistance".to_string(),
                            decode_min_focus_distance((ld[3] & 0xf8) >> 3),
                        );
                        tags.insert(
                            "Pentax:FocusRangeIndex".to_string(),
                            decode_focus_range_index(ld[3] & 0x07),
                        );
                        let focal_raw = ld[9] as i32;
                        let focal =
                            10.0 * (focal_raw >> 2) as f64 * 4f64.powi((focal_raw & 0x03) - 2);
                        tags.insert(
                            "Pentax:LensFocalLength".to_string(),
                            format!("{:.1} mm", focal),
                        );
                        let nominal_max = 2f64.powf(((ld[10] & 0xf0) >> 4) as f64 / 4.0);
                        tags.insert(
                            "Pentax:NominalMaxAperture".to_string(),
                            format!("{:.1}", nominal_max),
                        );
                        let nominal_min = 2f64.powf(((ld[10] & 0x0f) as f64 + 10.0) / 4.0);
                        tags.insert(
                            "Pentax:NominalMinAperture".to_string(),
                            format!("{:.0}", nominal_min),
                        );
                        let max_ap_raw = ld[14] & 0x7f;
                        if max_ap_raw > 1 {
                            let max_ap = 2f64.powf((max_ap_raw as f64 - 1.0) / 32.0);
                            tags.insert("Pentax:MaxAperture".to_string(), format!("{:.1}", max_ap));
                        }
                    }
                }

                // 0x0215 "CameraInfo" binary subdirectory (all int32u fields).
                PENTAX_CAMERA_INFO => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 20 {
                        let read_u32 = |b: &[u8]| -> u32 {
                            match byte_order {
                                ByteOrder::BigEndian => {
                                    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
                                }
                                ByteOrder::LittleEndian => {
                                    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
                                }
                            }
                        };
                        let model_id = read_u32(&raw[0..4]);
                        if let Some(name) = pentax_model_id_name(model_id) {
                            tags.insert("Pentax:PentaxModelID".to_string(), name.to_string());
                        }
                        let manufacture_date = read_u32(&raw[4..8]);
                        let date_str = manufacture_date.to_string();
                        let formatted = if date_str.len() == 8 {
                            format!(
                                "{}:{}:{}",
                                &date_str[0..4],
                                &date_str[4..6],
                                &date_str[6..8]
                            )
                        } else {
                            format!("Unknown ({})", manufacture_date)
                        };
                        tags.insert("Pentax:ManufactureDate".to_string(), formatted);
                        let major = read_u32(&raw[8..12]);
                        let minor = read_u32(&raw[12..16]);
                        tags.insert(
                            "Pentax:ProductionCode".to_string(),
                            format!("{}.{}", major, minor),
                        );
                        let serial = read_u32(&raw[16..20]);
                        tags.insert(
                            "Pentax:InternalSerialNumber".to_string(),
                            serial.to_string(),
                        );
                    }
                }

                // 0x0222 "ColorInfo" binary subdirectory (all int8s fields).
                PENTAX_COLOR_INFO => {
                    let raw = inline_or_offset_bytes(&entry, data, ifd_offset, byte_order);
                    if raw.len() >= 18 {
                        tags.insert("Pentax:WBShiftAB".to_string(), (raw[16] as i8).to_string());
                        tags.insert("Pentax:WBShiftGM".to_string(), (raw[17] as i8).to_string());
                    }
                }

                PENTAX_SERIAL_NUMBER => {
                    if let Some(value) =
                        extract_raw_string_preserve_spaces(&entry, data, ifd_offset, byte_order)
                    {
                        tags.insert("Pentax:SerialNumber".to_string(), value);
                    }
                }
                PENTAX_ARTIST => {
                    if let Some(value) =
                        extract_raw_string_preserve_spaces(&entry, data, ifd_offset, byte_order)
                    {
                        tags.insert("Pentax:Artist".to_string(), value);
                    }
                }
                PENTAX_COPYRIGHT => {
                    if let Some(value) =
                        extract_raw_string_preserve_spaces(&entry, data, ifd_offset, byte_order)
                    {
                        tags.insert("Pentax:Copyright".to_string(), value);
                    }
                }
                PENTAX_FIRMWARE_VERSION_VIDEO => {
                    if let Some(value) =
                        extract_raw_string_preserve_spaces(&entry, data, ifd_offset, byte_order)
                    {
                        tags.insert("Pentax:FirmwareVersion".to_string(), value);
                    }
                }

                _ => {
                    // Unknown tags are silently ignored
                }
            }
        }

        Ok(())
    }
}

/// Maps Pentax tag ID to human-readable tag name
///
/// This function provides consistent tag naming for Pentax MakerNote tags
fn pentax_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        0x0000 => "Version",
        0x0001 => "ModelType",
        0x0005 => "ModelID",
        0x0006 => "Date",
        0x0007 => "Time",
        0x0008 => "Quality",
        0x0009 => "ImageSize",
        0x000B => "PictureMode",
        0x000C => "FlashMode",
        0x000D => "FocusMode",
        0x000E => "AFPointSelected",
        0x000F => "AFPointInFocus",
        0x0014 => "ISO",
        0x0017 => "MeteringMode",
        0x0019 => "WhiteBalance",
        0x001A => "WhiteBalanceMode",
        0x001F => "Saturation",
        0x0020 => "Contrast",
        0x0021 => "Sharpness",
        0x0034 => "DriveMode",
        0x0037 => "ColorSpace",
        0x003F => "LensType",
        0x009F => "LensModel",
        0x003D => "ShutterCount",
        _ => return format!("Pentax:Unknown-{:#06X}", tag_id),
    };

    format!("Pentax:{}", tag_name)
}

/// Parses IFD entries in the specified byte order
///
/// This function handles parsing multiple IFD entries based on byte order
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
/// IFD entries are 12 bytes: tag_id (2), field_type (2), value_count (4), value_offset (4)
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
/// IFD entries are 12 bytes: tag_id (2), field_type (2), value_count (4), value_offset (4)
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

/// Returns the byte size of a single element of the given TIFF/EXIF field
/// type (e.g. SHORT/int16u is 2 bytes, LONG/int32u is 4 bytes). Unrecognized
/// types are assumed to be 1 byte/element (BYTE/ASCII/UNDEF/SBYTE).
fn tiff_field_type_size(field_type: u16) -> usize {
    match field_type {
        3 | 8 => 2,       // SHORT / SSHORT
        4 | 9 | 11 => 4,  // LONG / SLONG / FLOAT
        5 | 10 | 12 => 8, // RATIONAL / SRATIONAL / DOUBLE
        _ => 1,           // BYTE / ASCII / UNDEF / SBYTE
    }
}

/// Returns the raw bytes for an IFD entry's value, whether stored inline
/// (≤4 bytes, in `value_offset`) or at an offset relative to `ifd_offset`
/// within `full_data`. Returns an empty Vec if the offset-based value is out
/// of bounds.
fn inline_or_offset_bytes(
    entry: &IfdEntry,
    full_data: &[u8],
    _ifd_offset: usize,
    byte_order: ByteOrder,
) -> Vec<u8> {
    let count = entry.value_count as usize * tiff_field_type_size(entry.field_type);
    if count == 0 {
        return Vec::new();
    }
    if count <= 4 {
        let bytes = match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };
        bytes[0..count].to_vec()
    } else {
        // Offsets for values stored outside the IFD entry are relative to the
        // start of the raw MakerNote data (i.e. the "PENTAX \0" header, before
        // the 10-byte Start adjustment) -- this is ExifTool's "$start" prior
        // to applying `Start`, used for both the `Base => '$start'` (AVI) and
        // `Base => '$start - 10'` (JPEG MakerNotePentax5) conventions, which
        // both resolve to the same absolute base (`full_data[0]`).
        let abs_offset = entry.value_offset as usize;
        if abs_offset + count <= full_data.len() {
            full_data[abs_offset..abs_offset + count].to_vec()
        } else {
            Vec::new()
        }
    }
}

/// Extracts a string value, trimming only the trailing NUL terminator(s) but
/// preserving any interior/trailing whitespace (unlike [`extract_string_value`],
/// which also trims whitespace). Used for tags such as FirmwareVersion where
/// ExifTool preserves trailing spaces.
fn extract_raw_string_preserve_spaces(
    entry: &IfdEntry,
    full_data: &[u8],
    ifd_offset: usize,
    byte_order: ByteOrder,
) -> Option<String> {
    let bytes = inline_or_offset_bytes(entry, full_data, ifd_offset, byte_order);
    if bytes.is_empty() {
        return None;
    }
    let s = std::str::from_utf8(&bytes).ok()?;
    Some(s.trim_end_matches('\0').to_string())
}

/// Formats a bitmask value the way ExifTool's generic BITMASK PrintConv does:
/// named bits are printed by name, unnamed set bits are printed as "[N]", and
/// entries are joined with ", ". If the raw value is zero and `zero_label` is
/// provided, that label is used instead.
fn format_bitmask(raw: u32, zero_label: Option<&str>, named: &[(u8, &str)]) -> String {
    if raw == 0 {
        if let Some(z) = zero_label {
            return z.to_string();
        }
        return "0".to_string();
    }
    let mut parts = Vec::new();
    for bit in 0..32u8 {
        if raw & (1u32 << bit) != 0 {
            if let Some((_, name)) = named.iter().find(|(b, _)| *b == bit) {
                parts.push((*name).to_string());
            } else {
                parts.push(format!("[{}]", bit));
            }
        }
    }
    parts.join(", ")
}

fn decode_sr_result_bitmask(raw: u8) -> String {
    format_bitmask(
        raw as u32,
        Some("Not stabilized"),
        &[(0, "Stabilized"), (6, "Not ready")],
    )
}

/// Trims a float the way Perl's default number stringification would (no
/// trailing ".0", no unnecessary precision beyond what's needed).
fn format_pentax_float(v: f64) -> String {
    if (v - v.round()).abs() < 1e-6 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{:.3}", v);
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    }
}

/// ExifTool's `PentaxEv()`: converts a raw hex-based EV code (modulo 8) into
/// an EV value, correcting for the fact that 1/3-stop increments don't divide
/// evenly by 8.
fn pentax_ev(val: i32) -> f64 {
    let mut v = val as f64;
    if val & 1 != 0 {
        let sign: f64 = if val < 0 { -1.0 } else { 1.0 };
        let frac = ((val as f64) * sign) as i64 & 0x07;
        if frac == 3 {
            v += sign * (8.0 / 3.0 - frac as f64);
        } else if frac == 5 {
            v += sign * (16.0 / 3.0 - frac as f64);
        }
    }
    v / 8.0
}

/// ExifTool's `PrintExposureTime()`.
fn print_exposure_time(secs: f64) -> String {
    if secs > 0.0 && secs < 0.25001 {
        format!("1/{}", (0.5 + 1.0 / secs).floor() as i64)
    } else {
        let s = format!("{:.1}", secs);
        if let Some(stripped) = s.strip_suffix(".0") {
            stripped.to_string()
        } else {
            s
        }
    }
}

/// EV-based aperture formula shared by AEAperture/AEMaxAperture/AEMaxAperture2/
/// AEMinAperture: `2**((raw-68)/16)`.
fn ae_aperture_from_raw(raw: i32) -> f64 {
    2f64.powf((raw as f64 - 68.0) / 16.0)
}

fn decode_hue(raw: i32) -> String {
    match raw {
        0 => "-2".to_string(),
        1 => "Normal".to_string(),
        2 => "2".to_string(),
        3 => "-1".to_string(),
        4 => "1".to_string(),
        5 => "-3".to_string(),
        6 => "3".to_string(),
        7 => "-4".to_string(),
        8 => "4".to_string(),
        65535 => "None".to_string(),
        other => other.to_string(),
    }
}

/// Decodes the Pentax "firmware ID" encoding used for DSPFirmwareVersion and
/// CPUFirmwareVersion: each byte is bitwise-inverted, then formatted as
/// "A.BB.CC.DD".
fn decode_firmware_id(raw: &[u8]) -> Option<String> {
    if raw.len() != 4 {
        return None;
    }
    let a: Vec<u8> = raw.iter().map(|b| b ^ 0xff).collect();
    Some(format!("{}.{:02}.{:02}.{:02}", a[0], a[1], a[2], a[3]))
}

/// PictureMode (tag 0x0033): 3-byte array where the first two bytes are
/// joined for lookup and the third is the EV-step-size sub-mode.
fn decode_picture_mode_0x0033(b0: u8, b1: u8, b2: u8) -> Option<String> {
    let program: &str = match (b0, b1) {
        (0, 0) => Some("Program"),
        (0, 1) => Some("Hi-speed Program"),
        (0, 2) => Some("DOF Program"),
        (0, 3) => Some("MTF Program"),
        (0, 4) => Some("Standard"),
        (0, 5) => Some("Portrait"),
        (0, 6) => Some("Landscape"),
        (0, 7) => Some("Macro"),
        (0, 8) => Some("Sport"),
        (0, 9) => Some("Night Scene Portrait"),
        (0, 10) => Some("No Flash"),
        (0, 11) => Some("Night Scene"),
        (0, 12) => Some("Surf & Snow"),
        (0, 13) => Some("Text"),
        (0, 14) => Some("Sunset"),
        (0, 15) => Some("Kids"),
        (0, 16) => Some("Pet"),
        (0, 17) => Some("Candlelight"),
        (0, 18) => Some("Museum"),
        (1, 4) => Some("Auto PICT (Standard)"),
        (1, 5) => Some("Auto PICT (Portrait)"),
        (1, 6) => Some("Auto PICT (Landscape)"),
        (1, 7) => Some("Auto PICT (Macro)"),
        (1, 8) => Some("Auto PICT (Sport)"),
        (2, 0) => Some("Program (HyP)"),
        (2, 1) => Some("Hi-speed Program (HyP)"),
        (2, 2) => Some("DOF Program (HyP)"),
        (2, 3) => Some("MTF Program (HyP)"),
        (3, 0) => Some("Green Mode"),
        (4, 0) => Some("Shutter Speed Priority"),
        (5, 0) => Some("Aperture Priority"),
        (6, 0) => Some("Program Tv Shift"),
        (7, 0) => Some("Program Av Shift"),
        (8, 0) => Some("Manual"),
        (9, 0) => Some("Bulb"),
        (10, 0) => Some("Aperture Priority, Off-Auto-Aperture"),
        (11, 0) => Some("Manual, Off-Auto-Aperture"),
        (12, 0) => Some("Bulb, Off-Auto-Aperture"),
        (13, 0) => Some("Shutter & Aperture Priority AE"),
        (15, 0) => Some("Sensitivity Priority AE"),
        (16, 0) => Some("Flash X-Sync Speed AE"),
        (19, 0) => Some("Astrotracer"),
        (249, 0) => Some("Movie (TAv)"),
        (250, 0) => Some("Movie (TAv, Auto Aperture)"),
        (251, 0) => Some("Movie (Manual)"),
        (252, 0) => Some("Movie (Manual, Auto Aperture)"),
        (253, 0) => Some("Movie (Av)"),
        (254, 0) => Some("Movie (Av, Auto Aperture)"),
        (255, 0) => Some("Movie (P, Auto Aperture)"),
        (255, 4) => Some("Video (4)"),
        _ => None,
    }?;
    let step = match b2 {
        0 => "1/2 EV steps",
        1 => "1/3 EV steps",
        _ => return Some(program.to_string()),
    };
    Some(format!("{}; {}", program, step))
}

fn decode_drive_mode_byte0(b: u8) -> String {
    match b {
        0 => "Single-frame".to_string(),
        1 => "Continuous".to_string(),
        2 => "Continuous (Lo)".to_string(),
        3 => "Burst".to_string(),
        4 => "Continuous (Medium)".to_string(),
        5 => "Continuous (Low)".to_string(),
        255 => "Video".to_string(),
        other => other.to_string(),
    }
}

fn decode_drive_mode_byte1(b: u8) -> String {
    match b {
        0 => "No Timer".to_string(),
        1 => "Self-timer (12 s)".to_string(),
        2 => "Self-timer (2 s)".to_string(),
        15 => "Video".to_string(),
        16 => "Mirror Lock-up".to_string(),
        255 => "n/a".to_string(),
        other => other.to_string(),
    }
}

fn decode_drive_mode_byte2(b: u8) -> String {
    match b {
        0 => "Shutter Button".to_string(),
        1 => "Remote Control (3 s delay)".to_string(),
        2 => "Remote Control".to_string(),
        4 => "Remote Continuous Shooting".to_string(),
        other => other.to_string(),
    }
}

fn decode_drive_mode_byte3(b: u8) -> String {
    match b {
        0x00 => "Single Exposure".to_string(),
        0x01 => "Multiple Exposure".to_string(),
        0x02 => "Composite Average".to_string(),
        0x03 => "Composite Additive".to_string(),
        0x04 => "Composite Bright".to_string(),
        0x08 => "Interval Shooting".to_string(),
        0x0a => "Interval Composite Average".to_string(),
        0x0b => "Interval Composite Additive".to_string(),
        0x0c => "Interval Composite Bright".to_string(),
        0x0f => "Interval Movie".to_string(),
        0x10 => "HDR".to_string(),
        0x20 => "HDR Strong 1".to_string(),
        0x30 => "HDR Strong 2".to_string(),
        0x40 => "HDR Strong 3".to_string(),
        0x50 => "HDR Manual".to_string(),
        0xe0 => "HDR Auto".to_string(),
        0xff => "Video".to_string(),
        other => other.to_string(),
    }
}

// ----------------------------------------------------------------------------
// CameraSettings (0x0205) sub-fields
// ----------------------------------------------------------------------------

fn decode_picture_mode2(b: u8) -> String {
    match b {
        0 => "Scene Mode".to_string(),
        1 => "Auto PICT".to_string(),
        2 => "Program AE".to_string(),
        3 => "Green Mode".to_string(),
        4 => "Shutter Speed Priority".to_string(),
        5 => "Aperture Priority".to_string(),
        6 => "Program Tv Shift".to_string(),
        7 => "Program Av Shift".to_string(),
        8 => "Manual".to_string(),
        9 => "Bulb".to_string(),
        10 => "Aperture Priority, Off-Auto-Aperture".to_string(),
        11 => "Manual, Off-Auto-Aperture".to_string(),
        12 => "Bulb, Off-Auto-Aperture".to_string(),
        13 => "Shutter & Aperture Priority AE".to_string(),
        15 => "Sensitivity Priority AE".to_string(),
        16 => "Flash X-Sync Speed AE".to_string(),
        other => other.to_string(),
    }
}

fn decode_program_line(b: u8) -> String {
    match b {
        0 => "Normal".to_string(),
        1 => "Hi Speed".to_string(),
        2 => "Depth".to_string(),
        3 => "MTF".to_string(),
        other => other.to_string(),
    }
}

fn decode_flash_options(b: u8) -> String {
    match b {
        0 => "Normal".to_string(),
        1 => "Red-eye reduction".to_string(),
        2 => "Auto".to_string(),
        3 => "Auto, Red-eye reduction".to_string(),
        5 => "Wireless (Master)".to_string(),
        6 => "Wireless (Control)".to_string(),
        8 => "Slow-sync".to_string(),
        9 => "Slow-sync, Red-eye reduction".to_string(),
        10 => "Trailing-curtain Sync".to_string(),
        other => other.to_string(),
    }
}

fn decode_metering_mode2_bitmask(b: u8) -> String {
    format_bitmask(
        b as u32,
        Some("Multi-segment"),
        &[(0, "Center-weighted average"), (1, "Spot")],
    )
}

fn decode_af_point_mode_bitmask(b: u8) -> String {
    format_bitmask(
        b as u32,
        Some("Auto"),
        &[(0, "Select"), (1, "Fixed Center")],
    )
}

fn decode_focus_mode2(b: u8) -> String {
    match b {
        0 => "Manual".to_string(),
        1 => "AF-S".to_string(),
        2 => "AF-C".to_string(),
        3 => "AF-A".to_string(),
        other => other.to_string(),
    }
}

fn decode_af_point_selected2_bitmask(v: u16) -> String {
    format_bitmask(
        v as u32,
        Some("Auto"),
        &[
            (0, "Upper-left"),
            (1, "Top"),
            (2, "Upper-right"),
            (3, "Left"),
            (4, "Mid-left"),
            (5, "Center"),
            (6, "Mid-right"),
            (7, "Right"),
            (8, "Lower-left"),
            (9, "Bottom"),
            (10, "Lower-right"),
        ],
    )
}

fn decode_drive_mode2_bitmask(b: u8) -> String {
    format_bitmask(
        b as u32,
        Some("Single-frame"),
        &[
            (0, "Continuous"),
            (1, "Continuous (Lo)"),
            (2, "Self-timer (12 s)"),
            (3, "Self-timer (2 s)"),
            (4, "Remote Control (3 s delay)"),
            (5, "Remote Control"),
            (6, "Exposure Bracket"),
            (7, "Multiple Exposure"),
        ],
    )
}

fn decode_exposure_bracket_step_size(b: u8) -> String {
    match b {
        3 => "0.3".to_string(),
        4 => "0.5".to_string(),
        5 => "0.7".to_string(),
        8 => "1.0".to_string(),
        11 => "1.3".to_string(),
        12 => "1.5".to_string(),
        13 => "1.7".to_string(),
        16 => "2.0".to_string(),
        other => other.to_string(),
    }
}

fn decode_bracket_shot_number(b: u8) -> String {
    match b {
        0 => "n/a".to_string(),
        0x02 => "1 of 2".to_string(),
        0x12 => "2 of 2".to_string(),
        0x03 => "1 of 3".to_string(),
        0x13 => "2 of 3".to_string(),
        0x23 => "3 of 3".to_string(),
        0x05 => "1 of 5".to_string(),
        0x15 => "2 of 5".to_string(),
        0x25 => "3 of 5".to_string(),
        0x35 => "4 of 5".to_string(),
        0x45 => "5 of 5".to_string(),
        other => format!("0x{:02x}", other),
    }
}

fn decode_white_balance_set(b: u8) -> String {
    match b {
        0 => "Auto".to_string(),
        1 => "Daylight".to_string(),
        2 => "Shade".to_string(),
        3 => "Cloudy".to_string(),
        4 => "Daylight Fluorescent".to_string(),
        5 => "Day White Fluorescent".to_string(),
        6 => "White Fluorescent".to_string(),
        7 => "Tungsten".to_string(),
        8 => "Flash".to_string(),
        9 => "Manual".to_string(),
        12 => "Set Color Temperature 1".to_string(),
        13 => "Set Color Temperature 2".to_string(),
        14 => "Set Color Temperature 3".to_string(),
        other => other.to_string(),
    }
}

// ----------------------------------------------------------------------------
// AEInfo (0x0206) sub-fields
// ----------------------------------------------------------------------------

fn decode_ae_program_mode(b: u8) -> String {
    match b {
        0 => "M, P or TAv".to_string(),
        1 => "Av, B or X".to_string(),
        2 => "Tv".to_string(),
        3 => "Sv or Green Mode".to_string(),
        8 => "Hi-speed Program".to_string(),
        11 => "Hi-speed Program (P-Shift)".to_string(),
        16 => "DOF Program".to_string(),
        19 => "DOF Program (P-Shift)".to_string(),
        24 => "MTF Program".to_string(),
        27 => "MTF Program (P-Shift)".to_string(),
        35 => "Standard".to_string(),
        43 => "Portrait".to_string(),
        51 => "Landscape".to_string(),
        59 => "Macro".to_string(),
        67 => "Sport".to_string(),
        75 => "Night Scene Portrait".to_string(),
        83 => "No Flash".to_string(),
        91 => "Night Scene".to_string(),
        99 => "Surf & Snow".to_string(),
        104 => "Night Snap".to_string(),
        107 => "Text".to_string(),
        115 => "Sunset".to_string(),
        123 => "Kids".to_string(),
        131 => "Pet".to_string(),
        139 => "Candlelight".to_string(),
        144 => "SCN".to_string(),
        147 => "Museum".to_string(),
        160 => "Program".to_string(),
        184 => "Shallow DOF Program".to_string(),
        216 => "HDR".to_string(),
        other => other.to_string(),
    }
}

fn decode_ae_white_balance(b: u8) -> String {
    match b {
        0 => "Standard".to_string(),
        1 => "Daylight".to_string(),
        2 => "Shade".to_string(),
        3 => "Cloudy".to_string(),
        4 => "Daylight Fluorescent".to_string(),
        5 => "Day White Fluorescent".to_string(),
        6 => "White Fluorescent".to_string(),
        7 => "Tungsten".to_string(),
        8 => "Unknown".to_string(),
        other => other.to_string(),
    }
}

fn decode_ae_metering_mode_bitmask(b: u8) -> String {
    format_bitmask(
        b as u32,
        Some("Multi-segment"),
        &[(4, "Center-weighted average"), (5, "Spot")],
    )
}

// ----------------------------------------------------------------------------
// LensData (nested under LensInfo, 0x0207) sub-fields
// ----------------------------------------------------------------------------

fn decode_min_aperture_index(v: u8) -> u32 {
    match v {
        0 => 22,
        1 => 32,
        2 => 45,
        3 => 16,
        _ => 0,
    }
}

fn decode_min_focus_distance(v: u8) -> String {
    match v {
        0 => "0.13-0.19 m".to_string(),
        1 => "0.20-0.24 m".to_string(),
        2 => "0.25-0.28 m".to_string(),
        3 => "0.28-0.30 m".to_string(),
        4 => "0.35-0.38 m".to_string(),
        5 => "0.40-0.45 m".to_string(),
        6 => "0.49-0.50 m".to_string(),
        7 => "0.6 m".to_string(),
        8 => "0.7 m".to_string(),
        9 => "0.8-0.9 m".to_string(),
        10 => "1.0 m".to_string(),
        11 => "1.1-1.2 m".to_string(),
        12 => "1.4-1.5 m".to_string(),
        13 => "1.5 m".to_string(),
        14 => "2.0 m".to_string(),
        15 => "2.0-2.1 m".to_string(),
        16 => "2.1 m".to_string(),
        17 => "2.2-2.9 m".to_string(),
        18 => "3.0 m".to_string(),
        19 => "4-5 m".to_string(),
        20 => "5.6 m".to_string(),
        other => format!("Unknown ({})", other),
    }
}

fn decode_focus_range_index(v: u8) -> String {
    match v {
        7 => "0 (very close)".to_string(),
        6 => "1 (close)".to_string(),
        4 => "2".to_string(),
        5 => "3".to_string(),
        1 => "4".to_string(),
        0 => "5".to_string(),
        2 => "6 (far)".to_string(),
        3 => "7 (very far)".to_string(),
        other => other.to_string(),
    }
}

/// Pentax model ID (tag 0x0005/0x0215 offset 0) name lookup.
fn pentax_model_id_name(id: u32) -> Option<&'static str> {
    const TABLE: &[(u32, &str)] = &[
        (0x0000d, "Optio 330/430"),
        (0x12926, "Optio 230"),
        (0x12958, "Optio 330GS"),
        (0x12962, "Optio 450/550"),
        (0x1296c, "Optio S"),
        (0x12971, "Optio S V1.01"),
        (0x12994, "*ist D"),
        (0x129b2, "Optio 33L"),
        (0x129bc, "Optio 33LF"),
        (0x129c6, "Optio 33WR/43WR/555"),
        (0x129d5, "Optio S4"),
        (0x12a02, "Optio MX"),
        (0x12a0c, "Optio S40"),
        (0x12a16, "Optio S4i"),
        (0x12a34, "Optio 30"),
        (0x12a52, "Optio S30"),
        (0x12a66, "Optio 750Z"),
        (0x12a70, "Optio SV"),
        (0x12a75, "Optio SVi"),
        (0x12a7a, "Optio X"),
        (0x12a8e, "Optio S5i"),
        (0x12a98, "Optio S50"),
        (0x12aa2, "*ist DS"),
        (0x12ab6, "Optio MX4"),
        (0x12ac0, "Optio S5n"),
        (0x12aca, "Optio WP"),
        (0x12afc, "Optio S55"),
        (0x12b10, "Optio S5z"),
        (0x12b1a, "*ist DL"),
        (0x12b24, "Optio S60"),
        (0x12b2e, "Optio S45"),
        (0x12b38, "Optio S6"),
        (0x12b4c, "Optio WPi"),
        (0x12b56, "BenQ DC X600"),
        (0x12b60, "*ist DS2"),
        (0x12b62, "Samsung GX-1S"),
        (0x12b6a, "Optio A10"),
        (0x12b7e, "*ist DL2"),
        (0x12b80, "Samsung GX-1L"),
        (0x12b9c, "K100D"),
        (0x12b9d, "K110D"),
        (0x12ba2, "K100D Super"),
        (0x12bb0, "Optio T10/T20"),
        (0x12be2, "Optio W10"),
        (0x12bf6, "Optio M10"),
        (0x12c1e, "K10D"),
        (0x12c20, "Samsung GX10"),
        (0x12c28, "Optio S7"),
        (0x12c2d, "Optio L20"),
        (0x12c32, "Optio M20"),
        (0x12c3c, "Optio W20"),
        (0x12c46, "Optio A20"),
        (0x12c78, "Optio E30"),
        (0x12c7d, "Optio E35"),
        (0x12c82, "Optio T30"),
        (0x12c8c, "Optio M30"),
        (0x12c91, "Optio L30"),
        (0x12c96, "Optio W30"),
        (0x12ca0, "Optio A30"),
        (0x12cb4, "Optio E40"),
        (0x12cbe, "Optio M40"),
        (0x12cc3, "Optio L40"),
        (0x12cc5, "Optio L36"),
        (0x12cc8, "Optio Z10"),
        (0x12cd2, "K20D"),
        (0x12cd4, "Samsung GX20"),
        (0x12cdc, "Optio S10"),
        (0x12ce6, "Optio A40"),
        (0x12cf0, "Optio V10"),
        (0x12cfa, "K200D"),
        (0x12d04, "Optio S12"),
        (0x12d0e, "Optio E50"),
        (0x12d18, "Optio M50"),
        (0x12d22, "Optio L50"),
        (0x12d2c, "Optio V20"),
        (0x12d40, "Optio W60"),
        (0x12d4a, "Optio M60"),
        (0x12d68, "Optio E60/M90"),
        (0x12d72, "K2000"),
        (0x12d73, "K-m"),
        (0x12d86, "Optio P70"),
        (0x12d90, "Optio L70"),
        (0x12d9a, "Optio E70"),
        (0x12dae, "X70"),
        (0x12db8, "K-7"),
        (0x12dcc, "Optio W80"),
        (0x12dea, "Optio P80"),
        (0x12df4, "Optio WS80"),
        (0x12dfe, "K-x"),
        (0x12e08, "645D"),
        (0x12e12, "Optio E80"),
        (0x12e30, "Optio W90"),
        (0x12e3a, "Optio I-10"),
        (0x12e44, "Optio H90"),
        (0x12e4e, "Optio E90"),
        (0x12e58, "X90"),
        (0x12e6c, "K-r"),
        (0x12e76, "K-5"),
        (0x12e8a, "Optio RS1000/RS1500"),
        (0x12e94, "Optio RZ10"),
        (0x12e9e, "Optio LS1000"),
        (0x12ebc, "Optio WG-1 GPS"),
        (0x12ed0, "Optio S1"),
        (0x12ee4, "Q"),
        (0x12ef8, "K-01"),
        (0x12f0c, "Optio RZ18"),
        (0x12f16, "Optio VS20"),
        (0x12f2a, "Optio WG-2 GPS"),
        (0x12f48, "Optio LS465"),
        (0x12f52, "K-30"),
        (0x12f5c, "X-5"),
        (0x12f66, "Q10"),
        (0x12f70, "K-5 II"),
        (0x12f71, "K-5 II s"),
        (0x12f7a, "Q7"),
        (0x12f84, "MX-1"),
        (0x12f8e, "WG-3 GPS"),
        (0x12f98, "WG-3"),
        (0x12fa2, "WG-10"),
        (0x12fb6, "K-50"),
        (0x12fc0, "K-3"),
        (0x12fca, "K-500"),
        (0x12fe8, "WG-4"),
        (0x12fde, "WG-4 GPS"),
        (0x13006, "WG-20"),
        (0x13010, "645Z"),
        (0x1301a, "K-S1"),
        (0x13024, "K-S2"),
        (0x1302e, "Q-S1"),
        (0x13056, "WG-30"),
        (0x1307e, "WG-30W"),
        (0x13088, "WG-5 GPS"),
        (0x13092, "K-1"),
        (0x1309c, "K-3 II"),
        (0x131f0, "WG-M2"),
        (0x1320e, "GR III"),
        (0x13222, "K-70"),
        (0x1322c, "KP"),
        (0x13240, "K-1 Mark II"),
        (0x13254, "K-3 Mark III"),
        (0x13290, "WG-70"),
        (0x1329a, "GR IIIx"),
        (0x132b8, "KF"),
        (0x132d6, "K-3 Mark III Monochrome"),
        (0x132e0, "GR IV"),
        (0x13330, "GR IV Monochrome"),
    ];
    TABLE.iter().find(|(v, _)| *v == id).map(|(_, name)| *name)
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_quality() {
        assert_eq!(QUALITY.decode(2), "Best");
        assert_eq!(QUALITY.decode(4), "RAW");
        assert_eq!(QUALITY.decode(6), "RAW + JPEG");
    }

    #[test]
    fn test_decode_picture_mode() {
        assert_eq!(PICTURE_MODE.decode(0), "Program");
        assert_eq!(PICTURE_MODE.decode(2), "Aperture Priority");
        assert_eq!(PICTURE_MODE.decode(3), "Manual");
        assert_eq!(PICTURE_MODE.decode(5), "Landscape");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(FOCUS_MODE.decode(2), "Manual");
        assert_eq!(FOCUS_MODE.decode(3), "AF-S (Single)");
        assert_eq!(FOCUS_MODE.decode(4), "AF-C (Continuous)");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(WHITE_BALANCE.decode(0), "Auto");
        assert_eq!(WHITE_BALANCE.decode(1), "Daylight");
        assert_eq!(WHITE_BALANCE.decode(5), "Manual");
        assert_eq!(WHITE_BALANCE.decode(9), "Flash");
    }

    #[test]
    fn test_decode_drive_mode() {
        assert_eq!(DRIVE_MODE.decode(0), "Single-frame");
        assert_eq!(DRIVE_MODE.decode(1), "Continuous");
        assert_eq!(DRIVE_MODE.decode(5), "Exposure Bracketing");
    }

    #[test]
    fn test_decode_saturation() {
        assert_eq!(SATURATION.decode(0), "-2 (low)");
        assert_eq!(SATURATION.decode(1), "0 (normal)");
        assert_eq!(SATURATION.decode(2), "+2 (high)");
    }

    #[test]
    fn test_decode_contrast() {
        assert_eq!(CONTRAST.decode(0), "-2 (low)");
        assert_eq!(CONTRAST.decode(1), "0 (normal)");
        assert_eq!(CONTRAST.decode(2), "+2 (high)");
    }

    #[test]
    fn test_decode_sharpness() {
        assert_eq!(SHARPNESS.decode(0), "-2 (soft)");
        assert_eq!(SHARPNESS.decode(1), "0 (normal)");
        assert_eq!(SHARPNESS.decode(2), "+2 (hard)");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = PentaxParser;
        assert_eq!(parser.manufacturer_name(), "Pentax");
        assert_eq!(parser.tag_prefix(), "Pentax:");
    }

    #[test]
    fn test_validate_header_aoc() {
        let parser = PentaxParser;

        let valid_header = b"AOC\0extra_data_here";
        assert!(parser.validate_header(valid_header));

        let invalid_header = b"Canon\0\0\0";
        assert!(!parser.validate_header(invalid_header));
    }

    #[test]
    fn test_validate_header_pentax() {
        let parser = PentaxParser;

        let valid_header = b"PENTAX \0more_data";
        assert!(parser.validate_header(valid_header));
    }

    #[test]
    fn test_lens_lookup() {
        let parser = PentaxParser;

        // Test classic lens
        assert!(parser.lookup_lens(2).is_some());
        assert_eq!(
            parser.lookup_lens(2),
            Some("SMC Pentax-K 50mm f/1.4".to_string())
        );

        // Test Limited lens
        assert!(parser.lookup_lens(56).is_some());
        assert_eq!(
            parser.lookup_lens(56),
            Some("SMC Pentax-FA 77mm f/1.8 Limited".to_string())
        );

        // Test modern D FA lens
        assert!(parser.lookup_lens(122).is_some());
        assert_eq!(
            parser.lookup_lens(122),
            Some("HD Pentax-D FA 24-70mm f/2.8 ED SDM WR".to_string())
        );

        // Test unknown lens
        assert_eq!(parser.lookup_lens(65000), None);
    }

    #[test]
    fn test_pentax_tag_to_name() {
        assert_eq!(pentax_tag_to_name(0x0000), "Pentax:Version");
        assert_eq!(pentax_tag_to_name(0x003F), "Pentax:LensType");
        assert_eq!(pentax_tag_to_name(0x0008), "Pentax:Quality");
    }

    #[test]
    fn test_is_pentax_makernote() {
        let valid_data_aoc = b"AOC\0some_data";
        assert!(is_pentax_makernote(valid_data_aoc));

        let valid_data_pentax = b"PENTAX \0data";
        assert!(is_pentax_makernote(valid_data_pentax));

        let invalid_data = b"Nikon\0\0\0";
        assert!(!is_pentax_makernote(invalid_data));
    }
}

// ============================================================================
// Value Extraction Helpers for Byte Order Handling
// ============================================================================

fn extract_u8_value(entry: &IfdEntry, byte_order: ByteOrder) -> u8 {
    match byte_order {
        ByteOrder::BigEndian => ((entry.value_offset >> 24) & 0xFF) as u8,
        ByteOrder::LittleEndian => (entry.value_offset & 0xFF) as u8,
    }
}

fn extract_u16_value(entry: &IfdEntry, byte_order: ByteOrder) -> u16 {
    match byte_order {
        ByteOrder::BigEndian => ((entry.value_offset >> 16) & 0xFFFF) as u16,
        ByteOrder::LittleEndian => (entry.value_offset & 0xFFFF) as u16,
    }
}

fn extract_value_as_i32(entry: &IfdEntry, byte_order: ByteOrder) -> i32 {
    match entry.field_type {
        1 => extract_u8_value(entry, byte_order) as i32,
        3 => extract_u16_value(entry, byte_order) as i32,
        6 => extract_u8_value(entry, byte_order) as i8 as i32,
        8 => extract_u16_value(entry, byte_order) as i16 as i32,
        _ => entry.value_offset as i32,
    }
}

#[allow(dead_code)]
fn extract_value_as_u32(entry: &IfdEntry, byte_order: ByteOrder) -> u32 {
    match entry.field_type {
        1 | 6 => extract_u8_value(entry, byte_order) as u32,
        3 | 8 => extract_u16_value(entry, byte_order) as u32,
        _ => entry.value_offset,
    }
}
