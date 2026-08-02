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

/// The expression `PrintConv`s those tables need, hand-written.
mod print_conv;
/// `%Pentax` binary sub-tables, generated from ExifTool's own hashes.
pub mod subdir_tables;
/// The `ValueConv` computations those tables need, hand-written.
mod value_conv;

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

use super::pentax_lens_database::lookup_lens_type_pair;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_u16_array, extract_u32_array};
use super::shared::binary_subdir::{self, BinaryTable, Cond, ModelPat};
use super::shared::generic_decoders::ON_OFF;
use super::shared::tag_priority::insert_low_priority;
use subdir_tables::{
    PENTAX_AFINFO, PENTAX_AWBINFO, PENTAX_BATTERYINFO, PENTAX_EVSTEPINFO, PENTAX_FACEINFO,
    PENTAX_FACEPOS, PENTAX_FACESIZE, PENTAX_FILTERINFO, PENTAX_FLASHINFO, PENTAX_KELVINWB,
    PENTAX_LENSCORR, PENTAX_LENSINFOQ, PENTAX_LEVELINFO, PENTAX_SHOTINFO, PENTAX_SRINFO2,
    PENTAX_TEMPINFO, PENTAX_TIMEINFO, PENTAX_WBLEVELS,
};

// Import declarative decoder macros
use crate::const_decoder;

// Import registry
use super::registries::pentax::pentax_registry;
use crate::core::formatters::exif_print_conv::print_exposure_time;

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

// `%Pentax::Main` ids whose entry is a `SubDirectory` over a
// ProcessBinaryData table -- see `pentax_binary_subdir`.
const PENTAX_FLASH_INFO: u16 = 0x0208; // Pentax.pm:2847
const PENTAX_KELVIN_WB: u16 = 0x0221; // Pentax.pm:2992
const PENTAX_EV_STEP_INFO: u16 = 0x0224; // Pentax.pm:3001
const PENTAX_FACE_POS: u16 = 0x0227; // Pentax.pm:3010
const PENTAX_FACE_SIZE: u16 = 0x0228; // Pentax.pm:3015
const PENTAX_LEVEL_INFO: u16 = 0x022B; // Pentax.pm:3044
const PENTAX_WB_LEVELS: u16 = 0x022D; // Pentax.pm:3048
const PENTAX_LENS_INFO_Q: u16 = 0x0239; // Pentax.pm:3095
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
const PENTAX_BATTERY_INFO: u16 = 0x0216; // Pentax.pm:2945
const PENTAX_AF_INFO_RECORD: u16 = 0x021F; // Pentax.pm:2980
const PENTAX_COLOR_INFO: u16 = 0x0222;
const PENTAX_SHOT_INFO: u16 = 0x0226; // Pentax.pm:3011
const PENTAX_FILTER_INFO: u16 = 0x022A; // Pentax.pm:3030
const PENTAX_TEMP_INFO: u16 = 0x03FF; // Pentax.pm:3126
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
// FocusMode (0x000D), transcribed from Pentax.pm:1165-1206 (the non-Asahi
// variant, which is every body in the corpus). ExifTool's table is PrintHex
// and sparse -- 0..0x0c, then 0x10-0x12, 0x20-0x21, 0x110-0x120, and a
// macro-flagged block at 0x8003-0x800b. The six dense entries that used to
// live here shared their ids with ExifTool's but agreed with it on none of
// them: id 2 was "Manual" here and "Infinity" in ExifTool, 3 was
// "AF-S (Single)" here and "Manual" there.
const_decoder!(pub FOCUS_MODE,
    i32,
    [
        (0x0, "Normal"),
        (0x1, "Macro"),
        (0x2, "Infinity"),
        (0x3, "Manual"),
        (0x4, "Super Macro"),
        (0x5, "Pan Focus"),
        (0x6, "Auto-area"),
        (0x7, "Zone Select"),
        (0x8, "Select"),
        (0x9, "Pinpoint"),
        (0xa, "Tracking"),
        (0xb, "Continuous"),
        (0xc, "Snap"),
        (0x10, "AF-S (Focus-priority)"),
        (0x11, "AF-C (Focus-priority)"),
        (0x12, "AF-A (Focus-priority)"),
        (0x20, "Contrast-detect (Focus-priority)"),
        (0x21, "Tracking Contrast-detect (Focus-priority)"),
        (0x110, "AF-S (Release-priority)"),
        (0x111, "AF-C (Release-priority)"),
        (0x112, "AF-A (Release-priority)"),
        (0x120, "Contrast-detect (Release-priority)"),
        (0x8003, "Manual (Macro)"),
        (0x8006, "Auto-area (Macro)"),
        (0x8007, "Zone Select (Macro)"),
        (0x8008, "Select (Macro)"),
        (0x8009, "Pinpoint (Macro)"),
        (0x800a, "Tracking (Macro)"),
        (0x800b, "Continuous (Macro)"),
    ]
);

// Metering mode decoder - maps values to exposure metering modes
// MeteringMode (0x0017), Pentax.pm:1364-1374. ExifTool spells value 1 with a
// lower-case "average", and its fourth entry is Highlight at 6 -- there is
// nothing at 3 or 4.
const_decoder!(pub METERING_MODE,
    i32,
    [
        (0, "Multi-segment"),
        (1, "Center-weighted average"),
        (2, "Spot"),
        (6, "Highlight"),
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
// WhiteBalanceMode (0x001A), Pentax.pm:1385-1400. Value 10 is Cloudy, not a
// second Flash, and the two 0xfffe/0xffff sentinels were missing -- a
// user-set white balance reported "Unknown (65535)".
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
        (10, "Auto (Cloudy)"),
        (0xfffe, "Unknown"),
        (0xffff, "User-Selected"),
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

// NoiseReduction (0x0049), Pentax.pm:2183-2188: a plain Off/On.
const_decoder!(pub NOISE_REDUCTION, i32, [(0, "Off"), (1, "On"),]);

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
// ShutterType (0x0087), Pentax.pm:2649-2655: value 0 is "Normal".
const_decoder!(pub SHUTTER_TYPE, i32, [(0, "Normal"), (1, "Electronic"),]);

// Neutral density filter decoder
const_decoder!(pub NEUTRAL_DENSITY_FILTER, i32, [(0, "Off"), (1, "On"),]);

// MonochromeFilterEffect (0x0073), Pentax.pm:2456-2469. The old table was
// off by one -- 1 was "Yellow" here and "Green" in ExifTool, and so on down
// -- and "None" is 0xffff, not 0.
const_decoder!(pub MONOCHROME_FILTER_EFFECT, i32, [
    (1, "Green"),
    (2, "Yellow"),
    (3, "Orange"),
    (4, "Red"),
    (5, "Magenta"),
    (6, "Blue"),
    (7, "Cyan"),
    (8, "Infrared"),
    (65535, "None"),
]);

// MonochromeToning (0x0074), Pentax.pm:2470-2484: a -4..+4 scale, not a set
// of colour names.
const_decoder!(pub MONOCHROME_TONING, i32, [
    (0, "-4"),
    (1, "-3"),
    (2, "-2"),
    (3, "-1"),
    (4, "0"),
    (5, "1"),
    (6, "2"),
    (7, "3"),
    (8, "4"),
    (65535, "None"),
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

// RawDevelopmentProcess (0x0062), Pentax.pm:2251-2277. ExifTool names each
// version after the bodies that use it, and there is no version 2.
const_decoder!(pub RAW_DEVELOPMENT_PROCESS, i32, [
    (1, "1 (K10D,K200D,K2000,K-m)"),
    (3, "3 (K20D)"),
    (4, "4 (K-7)"),
    (5, "5 (K-x)"),
    (6, "6 (645D)"),
    (7, "7 (K-r)"),
    (8, "8 (K-5,K-5II,K-5IIs)"),
    (9, "9 (Q)"),
    (10, "10 (K-01,K-30,K-50,K-500)"),
    (11, "11 (Q10)"),
    (12, "12 (MX-1,Q-S1,Q7)"),
    (13, "13 (K-3,K-3II)"),
    (14, "14 (645Z)"),
    (15, "15 (K-S1,K-S2)"),
    (16, "16 (K-1)"),
    (17, "17 (K-70)"),
    (18, "18 (KP)"),
    (19, "19 (GR III)"),
    (20, "20 (K-3III)"),
    (21, "21 (K-3IIIMonochrome)"),
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
///
/// `ricoh_make` is ExifTool's `$$self{Make} =~ /^RICOH/` (Pentax.pm:3032), the
/// test that picks `FilterInfo`'s byte order. It is a property of the file's
/// IFD0, not of the MakerNote, so it is fixed when the dispatcher chooses this
/// parser -- the same body writes the record the other way round under the
/// other brand, and guessing from the model would be inventing the answer for
/// every Ricoh-branded Pentax.
#[derive(Default)]
pub struct PentaxParser {
    pub ricoh_make: bool,
}

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

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_located(data, byte_order, None, None, tags)
    }

    fn parse_with_context(
        &self,
        ctx: &crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // "AOC\0" MakerNotes address their values from the TIFF header and can
        // point past their own declared end, so decode over the enclosing
        // block's window rather than the payload alone.
        self.parse_located(
            ctx.window(),
            byte_order,
            ctx.payload_tiff_offset(),
            model,
            tags,
        )?;

        // 0x0004 PreviewImageStart is `IsOffset`, so it needs its directory's
        // base added (ExifTool.pm:10133). Which base depends on the header:
        // the "AOC\0" form (MakerNotes.pm:769) declares no `Base` and inherits
        // the enclosing TIFF header's file offset, while the "PENTAX \0" form
        // (MakerNotePentax5, MakerNotes.pm:825) re-bases onto its own first
        // byte with `Base => '$start - 10'`. See `absolutise_is_offset`.
        let base = if ctx.payload().starts_with(PENTAX_HEADER_PENTAX) {
            ctx.payload_base()
        } else {
            ctx.tiff_base()
        };
        crate::parsers::tiff::makernotes::makernote_context::absolutise_is_offset(
            tags,
            base,
            &["Pentax:PreviewImageStart"],
        );
        Ok(())
    }
}

impl PentaxParser {
    #[allow(clippy::too_many_lines)]
    fn parse_located(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        data_base: Option<u32>,
        model: Option<&str>,
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
        // See `inline_or_offset_bytes`: only the "AOC\0" form measures its value
        // offsets from the TIFF header.
        let mut value_base: i64 = 0;
        let is_aoc_header = data.len() >= 4 && &data[0..4] == PENTAX_HEADER_AOC;
        let ifd_offset = if is_aoc_header {
            value_base = data_base.map_or(0, |o| -(i64::from(o)));
            // AOC header: "AOC\0" plus a 2-byte byte-order marker, IFD at 6.
            // The marker can disagree with the container -- SamsungGX-1L.jpg
            // and SamsungGX-1S.jpg are little-endian JPEGs carrying an "AOC\0MM"
            // MakerNote -- and reading it in the container's order byte-swapped
            // every value (ISO 200 came out as 589824).
            if data.len() >= 6 {
                if &data[4..6] == b"MM" {
                    byte_order = ByteOrder::BigEndian;
                } else if &data[4..6] == b"II" {
                    byte_order = ByteOrder::LittleEndian;
                }
            }
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

        // TIFF left-justifies a value that fits in the entry's 4-byte field, so
        // in a big-endian directory a 2-byte SHORT occupies the HIGH half of
        // that word. Reading the word as the value therefore returns the value
        // shifted left by 16 -- Pentax645D.jpg reported ISO 200 as 589824
        // (0x090000) and Tokyo's HometownCity code 0x38 as 3670016. Right-align
        // every inline value once, here, so the ~100 places below that read
        // `entry.value_offset` as a number see the number the camera wrote.
        let entries: Vec<IfdEntry> = entries
            .into_iter()
            .map(|entry| right_align_inline_value(entry, byte_order))
            .collect();

        // "AOC\0" MakerNotes declare no Base of their own, so their offsets
        // are read against the enclosing TIFF header -- but real files move
        // the block during a re-save without correcting those offsets. Apply
        // ExifTool's FixBase correction (see `pentax_fix_base`) before using
        // `value_base` to resolve any out-of-line value below.
        if is_aoc_header && let Some(data_pos) = data_base.map(i64::from) {
            value_base += pentax_fix_base(&entries, ifd_offset, data_pos);
        }

        // Pentax::Main entries the match below never registered, added first so
        // the hand-written arms keep ownership of anything they do produce.
        super::pentax_supplement::add_supplemental_tags(
            data, ifd_offset, value_base, byte_order, tags,
        );

        // ExifTool's `$$self{...}` slots, shared across this file's directories:
        // `FaceInfo` sets `FacesDetected` and `FacePos`/`FaceSize` are gated on it.
        let mut members = binary_subdir::Members::new();

        // Extract tags from entries
        for entry in entries {
            if let Some((table, order)) =
                pentax_binary_subdir(&entry, model, self.ricoh_make, &members, byte_order)
            {
                let record = inline_or_offset_bytes(&entry, data, value_base, byte_order);
                if !record.is_empty() {
                    binary_subdir::decode_binary_subdir_with(
                        table,
                        &record,
                        order,
                        "Pentax",
                        model,
                        &mut members,
                        tags,
                    );
                }
                continue;
            }

            match entry.tag_id {
                // Simple string tags
                PENTAX_VERSION => {
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
                    if raw.len() == 4 {
                        tags.insert(
                            "Pentax:PentaxVersion".to_string(),
                            format!("{}.{}.{}.{}", raw[0], raw[1], raw[2], raw[3]),
                        );
                    }
                }

                PENTAX_DATE => {
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
                    if raw.len() >= 3 {
                        tags.insert(
                            "Pentax:Time".to_string(),
                            format!("{:02}:{:02}:{:02}", raw[0], raw[1], raw[2]),
                        );
                    }
                }

                PENTAX_LENS_MODEL => {
                    if let Some(value) = extract_string_value(&entry, data, value_base) {
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
                    if raw.len() >= 2 {
                        let series = raw[0];
                        let sub_id = raw[1] as u16;
                        let name = lookup_lens_type_pair(series, sub_id)
                            .unwrap_or_else(|| format!("Unknown ({} {})", series, sub_id));
                        // `%Pentax::LensRec` key 0 is `Priority => 0`
                        // (Pentax.pm:4202), as is the 0x0207 copy below. Between
                        // two 0-priority instances ExifTool keeps the first
                        // (ExifTool.pm:9541-9551), and 0x003f is read first.
                        insert_low_priority(tags, "Pentax:LensType".to_string(), name);
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
                    if let Some(value) = decode_firmware_id(&raw) {
                        tags.insert("Pentax:DSPFirmwareVersion".to_string(), value);
                    }
                }
                PENTAX_CPU_FIRMWARE_VERSION => {
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
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
                    if let Some(value) = extract_string_value(&entry, data, value_base) {
                        tags.insert("Pentax:TimeInfo".to_string(), value);
                    }
                }
                PENTAX_HIGH_LOW_KEY_ADJ => {
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
                    if raw.len() >= 4 {
                        let series = raw[0] & 0x0f;
                        let sub_id = (raw[2] as u16) * 256 + raw[3] as u16;
                        if let Some(name) = lookup_lens_type_pair(series, sub_id) {
                            // `Priority => 0` in every `%Pentax::LensInfo*`
                            // (Pentax.pm:4226, :4248, :4284, :4320, :4357): the
                            // 0x003f copy read earlier is the one that prints.
                            // PentaxK100D.jpg carries `0 0 0 0` here, which
                            // decodes to a real lens name ("M-42 or No Lens")
                            // and so overwrote the correct one silently.
                            insert_low_priority(tags, "Pentax:LensType".to_string(), name);
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
                        // `%Pentax::LensData` key 9 is `Priority => 0`
                        // (Pentax.pm:4506).
                        insert_low_priority(
                            tags,
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
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
                            // `%Pentax::CameraInfo` key 0 is `Priority => 0`,
                            // with ExifTool's own reason on the line:
                            // "(Optio SVi uses incorrect Optio SV ID here)"
                            // (Pentax.pm:4723). The 0x0005 copy read earlier is
                            // the one that prints.
                            insert_low_priority(
                                tags,
                                "Pentax:PentaxModelID".to_string(),
                                name.to_string(),
                            );
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
                    let raw = inline_or_offset_bytes(&entry, data, value_base, byte_order);
                    if raw.len() >= 18 {
                        tags.insert("Pentax:WBShiftAB".to_string(), (raw[16] as i8).to_string());
                        tags.insert("Pentax:WBShiftGM".to_string(), (raw[17] as i8).to_string());
                    }
                }

                PENTAX_SERIAL_NUMBER => {
                    if let Some(value) =
                        extract_raw_string_preserve_spaces(&entry, data, value_base, byte_order)
                    {
                        tags.insert("Pentax:SerialNumber".to_string(), value);
                    }
                }
                PENTAX_ARTIST => {
                    if let Some(value) =
                        extract_raw_string_preserve_spaces(&entry, data, value_base, byte_order)
                    {
                        tags.insert("Pentax:Artist".to_string(), value);
                    }
                }
                PENTAX_COPYRIGHT => {
                    if let Some(value) =
                        extract_raw_string_preserve_spaces(&entry, data, value_base, byte_order)
                    {
                        tags.insert("Pentax:Copyright".to_string(), value);
                    }
                }
                PENTAX_FIRMWARE_VERSION_VIDEO => {
                    if let Some(value) =
                        extract_raw_string_preserve_spaces(&entry, data, value_base, byte_order)
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
fn extract_string_value(entry: &IfdEntry, full_data: &[u8], value_base: i64) -> Option<String> {
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
    let Some(abs_offset) = i64::from(offset as u32)
        .checked_add(value_base)
        .filter(|o| *o >= 0)
        .map(|o| o as usize)
    else {
        return None;
    };

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
/// Right-align an inline value inside the entry's 4-byte field.
///
/// A big-endian directory stores a value shorter than four bytes in the HIGH
/// bytes of the field, so the parsed `u32` is the value shifted left. Shifting
/// it back means every numeric read below is the camera's number, and
/// [`inline_or_offset_bytes`] takes the matching low-end slice.
fn right_align_inline_value(entry: IfdEntry, byte_order: ByteOrder) -> IfdEntry {
    if byte_order != ByteOrder::BigEndian {
        return entry;
    }
    let size = (entry.value_count as usize).saturating_mul(tiff_field_type_size(entry.field_type));
    if size == 0 || size >= 4 {
        return entry;
    }
    IfdEntry {
        value_offset: entry.value_offset >> (8 * (4 - size)),
        ..entry
    }
}

/// The `%Pentax::Main` tags whose ExifTool entry is a `SubDirectory` over a
/// `ProcessBinaryData` table this reader can transcribe, the table each one
/// selects, and the byte order to read the record in.
///
/// Several ids are a list of alternatives rather than one entry, and ExifTool
/// takes the first whose `Condition` holds. A record that matches none of them
/// produces nothing here rather than a guess -- `%Pentax` has an
/// `...Unknown` companion table for exactly those, and ExifTool reports no
/// named tags from it either.
fn pentax_binary_subdir(
    entry: &IfdEntry,
    model: Option<&str>,
    ricoh_make: bool,
    members: &binary_subdir::Members,
    byte_order: ByteOrder,
) -> Option<(&'static BinaryTable, ByteOrder)> {
    let count = entry.value_count;
    // `$$self{FacesDetected}`, set by `%Pentax::FaceInfo` field 0
    // (Pentax.pm:3265) and read by 0x0227 and 0x0228.
    let faces_detected = members.get("FacesDetected").copied().unwrap_or(0);
    let table = match entry.tag_id {
        // 0x005c is `[{ Condition => '$count == 4' => SRInfo }, { SRInfo2 }]`
        // (Pentax.pm:2258-2267). The count==4 branch is decoded by the
        // hand-written arm below, which predates this table.
        PENTAX_SHAKE_REDUCTION if count != 4 => &PENTAX_SRINFO2,
        PENTAX_FACE_INFO => &PENTAX_FACEINFO, // Pentax.pm:2293
        PENTAX_AWB_INFO => &PENTAX_AWBINFO,   // Pentax.pm:2343
        PENTAX_TIME_INFO => &PENTAX_TIMEINFO, // Pentax.pm:2366
        PENTAX_LENS_CORR => &PENTAX_LENSCORR, // Pentax.pm:2580
        // `Condition => '$count == 27'`, else `FlashInfoUnknown` (Pentax.pm:2847-2856).
        PENTAX_FLASH_INFO if count == 27 => &PENTAX_FLASHINFO,
        PENTAX_KELVIN_WB => &PENTAX_KELVINWB, // Pentax.pm:2992
        // `Drop => 200`: ExifTool discards the tag entirely above 200 bytes
        // (40 kB in the Pentax Q), so there is nothing to descend into.
        PENTAX_EV_STEP_INFO if count <= 200 => &PENTAX_EVSTEPINFO,
        // `Condition => '$$self{FacesDetected}'` -- "ignore if no faces to
        // decode" (Pentax.pm:3012, :3017).
        PENTAX_FACE_POS if faces_detected != 0 => &PENTAX_FACEPOS,
        PENTAX_FACE_SIZE if faces_detected != 0 => &PENTAX_FACESIZE,
        // 0x022b is `LevelInfoK3III` when the model matches, else `LevelInfo`
        // (Pentax.pm:3039-3046). `LevelInfoK3III` is a different layout, not
        // transcribed here, so that body descends into nothing rather than into
        // the wrong table.
        PENTAX_LEVEL_INFO if !is_k3_mark_iii(model) => &PENTAX_LEVELINFO,
        // `Condition => '$count == 100'` (Pentax.pm:3050).
        PENTAX_WB_LEVELS if count == 100 => &PENTAX_WBLEVELS,
        PENTAX_LENS_INFO_Q => &PENTAX_LENSINFOQ, // Pentax.pm:3095
        PENTAX_SHOT_INFO => &PENTAX_SHOTINFO,    // Pentax.pm:3011
        // 0x03ff is `TempInfo` on the listed bodies and `UnknownInfo` -- a table
        // with no tags in it -- on every other (Pentax.pm:3126-3134).
        PENTAX_TEMP_INFO if TEMP_INFO_MODELS.holds(model) => &PENTAX_TEMPINFO,
        // These three declare `ByteOrder` on the SubDirectory, so they are read
        // big-endian whatever the MakerNote's own order is. ExifTool's reason,
        // at Pentax.pm:2983-2986: "Most of these subdirectories are 'undef'
        // format, and as such the byte ordering is not changed when changed via
        // the Pentax software (which will write a little-endian TIFF on an Intel
        // system)." 0x0216 repeats the warning -- "have seen makernotes changed
        // to little-endian in DNG!" (Pentax.pm:2949).
        PENTAX_BATTERY_INFO => return Some((&PENTAX_BATTERYINFO, ByteOrder::BigEndian)),
        PENTAX_AF_INFO_RECORD => return Some((&PENTAX_AFINFO, ByteOrder::BigEndian)),
        // 0x022a is one table read either way round: `LittleEndian` when
        // `$$self{Make} =~ /^RICOH/`, `BigEndian` otherwise
        // (Pentax.pm:3030-3042). The brand, not the model, decides.
        PENTAX_FILTER_INFO => {
            let order = if ricoh_make {
                ByteOrder::LittleEndian
            } else {
                ByteOrder::BigEndian
            };
            return Some((&PENTAX_FILTERINFO, order));
        }
        _ => return None,
    };
    // The remaining `SubDirectory` entries carry no `ByteOrder` override, so
    // each record is read in the MakerNote's own order.
    Some((table, byte_order))
}

/// ExifTool's `$$self{Model} =~ /K-3 Mark III/` (Pentax.pm:3041).
fn is_k3_mark_iii(model: Option<&str>) -> bool {
    model.is_some_and(|m| m.contains("K-3 Mark III"))
}

/// `Condition => '$$self{Model} =~ /K-(01|3|30|5|50|500)\b/'` on the 0x03ff
/// `TempInfo` alternative (Pentax.pm:3129), expanded the same way
/// `codegen_subdirs.py` expands the ones inside a table.
///
/// The `\b` is what keeps `K-5` off a `K-50`, and it is also why a K-3 Mark III
/// *is* included: "K-3" there is followed by a space.
static TEMP_INFO_MODELS: Cond = Cond::Model {
    any_of: &[
        ModelPat {
            text: "K-01",
            word_end: true,
        },
        ModelPat {
            text: "K-3",
            word_end: true,
        },
        ModelPat {
            text: "K-30",
            word_end: true,
        },
        ModelPat {
            text: "K-5",
            word_end: true,
        },
        ModelPat {
            text: "K-50",
            word_end: true,
        },
        ModelPat {
            text: "K-500",
            word_end: true,
        },
    ],
    none_of: &[],
};

/// ExifTool's `FixBase` (MakerNotes.pm:1282), restricted to the case that
/// applies to Pentax's "AOC\0" MakerNote: not the Canon TIFF-footer variant,
/// and not entry-based (Pentax forces absolute addressing regardless of what
/// the entry-based heuristic below would otherwise conclude --
/// `GetMakerNoteOffset`'s `$relative = 0`, MakerNotes.pm:1220).
///
/// A "AOC\0" MakerNote declares no `Base` of its own (MakerNotes.pm:769-779),
/// so its value offsets are meant to be read relative to the enclosing TIFF
/// header. Real files routinely move the MakerNote block during a re-save
/// without correcting those offsets: `Pentax_istD.jpg`'s `0x0216` entry
/// declares offset 2100, but the six bytes it names live at file offset
/// 2114 -- the block's *true* base is 14 bytes from the TIFF header, not the
/// 30 a literal read of the offset gives. ExifTool recovers the correction
/// by comparing where the IFD's value block actually starts against where
/// the entries say it should (MakerNotes.pm:1340-1381); this ports that.
///
/// `dir_start` is the IFD's offset within the payload/window (`ifd_offset`
/// in [`PentaxParser::parse_located`], e.g. 6 for "AOC\0MM"); `data_pos` is
/// the payload's distance from the TIFF header (`data_base` as `i64`).
/// Returns the amount to add to `value_base`; 0 when the declared offsets
/// already look right (the common case).
fn pentax_fix_base(entries: &[IfdEntry], dir_start: usize, data_pos: i64) -> i64 {
    // ExifTool's @formatSize (Exif.pm:82), indexed 1..13 -- the only formats
    // GetValueBlocks considers (MakerNotes.pm:1249).
    const FORMAT_SIZE: [u32; 14] = [0, 1, 1, 2, 4, 8, 1, 1, 2, 4, 8, 4, 8, 4];

    // GetValueBlocks (MakerNotes.pm:1241): longest out-of-line value block at
    // each distinct declared offset. Inline values (size <= 4) carry no
    // offset information and are skipped, as ExifTool does.
    let mut val_block: std::collections::BTreeMap<u32, u32> = std::collections::BTreeMap::new();
    for entry in entries {
        let format = entry.field_type;
        if !(1..=13).contains(&format) {
            break; // MakerNotes.pm:1249 stops scanning entirely, not just this entry.
        }
        let size = entry
            .value_count
            .saturating_mul(FORMAT_SIZE[format as usize]);
        if size <= 4 {
            continue;
        }
        let slot = val_block.entry(entry.value_offset).or_insert(0);
        if size > *slot {
            *slot = size;
        }
    }
    let Some(&first) = val_block.keys().next() else {
        return 0;
    };

    // Walk offsets in ascending order, finding the true minimum
    // (MakerNotes.pm:1344-1381): a jump of a full IFD length or more that
    // lands near the expected start overrides the running minimum, and any
    // offset below 12 is treated as garbage rather than trusted.
    let ifd_len = 2 + 12 * entries.len() as i64;
    let ifd_end = dir_start as i64 + ifd_len;
    let expected = data_pos + ifd_end + 4; // Pentax's normal offset, MakerNotes.pm:1216.

    let mut min_pt = i64::from(first);
    let mut last: Option<i64> = None;
    for (&val_ptr, &size) in &val_block {
        let val_ptr = i64::from(val_ptr);
        if let Some(last_end) = last {
            let gap = val_ptr - last_end;
            if gap >= ifd_len && (val_ptr - expected).abs() <= 4 {
                min_pt = val_ptr;
            }
            if min_pt < 12 {
                min_pt = val_ptr;
            }
        }
        last = Some(val_ptr + i64::from(size));
    }

    let diff = (min_pt - data_pos) - ifd_end;
    // Pentax's only declared normal offset is 4 (MakerNotes.pm:1216); 0 is
    // always allowed too (MakerNotes.pm:1444).
    if diff == 0 || diff == 4 {
        return 0;
    }
    4 - diff
}

fn inline_or_offset_bytes(
    entry: &IfdEntry,
    full_data: &[u8],
    value_base: i64,
    byte_order: ByteOrder,
) -> Vec<u8> {
    let count = entry.value_count as usize * tiff_field_type_size(entry.field_type);
    if count == 0 {
        return Vec::new();
    }
    if count <= 4 {
        // `right_align_inline_value` has already moved a short big-endian value
        // into the low bytes, so take it from that end.
        match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes()[0..count].to_vec(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes()[4 - count..4].to_vec(),
        }
    } else {
        // Where an out-of-line value offset points depends on the header:
        // "PENTAX \0" MakerNotes declare `Base => '$start - 10'`, so offsets
        // are relative to the MakerNote itself (`value_base` 0), while "AOC\0"
        // MakerNotes have no Base override and measure from the TIFF header,
        // so `value_base` is minus the block's own TIFF-relative offset. Using
        // 0 for both put SamsungGX20.jpg's WhitePoint 672 bytes past its real
        // position and printed neighbouring bytes as the value.
        let Some(abs_offset) = i64::from(entry.value_offset)
            .checked_add(value_base)
            .filter(|o| *o >= 0)
            .map(|o| o as usize)
        else {
            return Vec::new();
        };
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
    value_base: i64,
    byte_order: ByteOrder,
) -> Option<String> {
    let bytes = inline_or_offset_bytes(entry, full_data, value_base, byte_order);
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

    /// A "PENTAX \0" MakerNote block carrying `entries` (tag, type, count,
    /// value-or-offset) big-endian, plus `trailer` laid down at offset 64.
    ///
    /// Offsets in a "PENTAX \0" MakerNote are measured from the block itself
    /// (`Base => '$start - 10'`), so a value at index 64 of the returned buffer
    /// is addressed as 64.
    #[cfg(test)]
    fn pentax_block(entries: &[(u16, u16, u32, u32)], trailer: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(PENTAX_HEADER_PENTAX); // 0..8
        out.extend_from_slice(b"MM"); // 8..10
        out.extend_from_slice(&(entries.len() as u16).to_be_bytes()); // 10..12
        for &(tag, ftype, count, value) in entries {
            out.extend_from_slice(&tag.to_be_bytes());
            out.extend_from_slice(&ftype.to_be_bytes());
            out.extend_from_slice(&count.to_be_bytes());
            out.extend_from_slice(&value.to_be_bytes());
        }
        out.extend_from_slice(&0u32.to_be_bytes()); // next-IFD pointer
        out.resize(64, 0);
        out.extend_from_slice(trailer);
        out
    }

    /// ExifTool's `Priority => 0`, end to end through the parse loop.
    ///
    /// Both of these ids are read by the same walk, in ascending order, and
    /// both write a tag the `Main` table already reported. Before the priority
    /// rule was applied they simply overwrote it, which is not a missing tag
    /// or a crash -- it is a real ExifTool tag name carrying the wrong value.
    ///
    /// The bytes are the ones `exiftool -v3` prints for two corpus files:
    ///
    /// * `Pentax/PentaxK100D.jpg` -- `LensRec` (0x003f) holds `7 244` and
    ///   `LensInfo` (0x0207) holds `0 0 0 0`. Both tables are `Priority => 0`
    ///   (Pentax.pm:4202, :4248), so ExifTool keeps the first (ExifTool.pm:9541-9551)
    ///   and reports `smc PENTAX-DA 21mm F3.2 AL Limited`. `0 0` is a real lens
    ///   id, not a null -- it decodes to `M-42 or No Lens`.
    /// * `Pentax/PentaxOptioSVi.jpg` -- `Main` 0x0005 holds 76405 and
    ///   `CameraInfo` (0x0215) holds 76400. ExifTool marks only the latter
    ///   `Priority => 0`, with its reason on the line: "(Optio SVi uses
    ///   incorrect Optio SV ID here)" (Pentax.pm:4723). ExifTool reports
    ///   `Optio SVi`.
    #[test]
    fn low_priority_subdirectory_does_not_overwrite_the_main_table() {
        let mut camera_info = Vec::new();
        camera_info.extend_from_slice(&76_400u32.to_be_bytes()); // PentaxModelID
        camera_info.extend_from_slice(&20_040_101u32.to_be_bytes()); // ManufactureDate
        camera_info.extend_from_slice(&1u32.to_be_bytes()); // ProductionCode major
        camera_info.extend_from_slice(&0u32.to_be_bytes()); // ProductionCode minor
        camera_info.extend_from_slice(&7u32.to_be_bytes()); // InternalSerialNumber

        let data = pentax_block(
            &[
                // 0x0005 PentaxModelID, LONG: 76405 -> "Optio SVi"
                (0x0005, 4, 1, 76_405),
                // 0x003f LensRec, 4 BYTEs `07 f4 00 00` -> LensType "7 244"
                (0x003F, 1, 4, 0x07F4_0000),
                // 0x0207 LensInfo, 4 BYTEs `00 00 00 00` -> LensType "0 0"
                (0x0207, 1, 4, 0x0000_0000),
                // 0x0215 CameraInfo, 5 LONGs at offset 64
                (0x0215, 4, 5, 64),
            ],
            &camera_info,
        );

        let mut tags = HashMap::new();
        PentaxParser::default()
            .parse(&data, ByteOrder::BigEndian, &mut tags)
            .expect("Pentax MakerNote should parse");

        assert_eq!(
            tags.get("Pentax:LensType").map(String::as_str),
            Some("smc PENTAX-DA 21mm F3.2 AL Limited"),
            "0x0207 LensInfo is Priority => 0 and must not overwrite 0x003f LensRec"
        );
        assert_eq!(
            tags.get("Pentax:PentaxModelID").map(String::as_str),
            Some("Optio SVi"),
            "0x0215 CameraInfo is Priority => 0 and must not overwrite Main 0x0005"
        );
    }

    /// The rule suppresses a clobber, not the tag: with no `Main` copy present,
    /// the `Priority => 0` sub-directory value is still the one reported.
    #[test]
    fn low_priority_subdirectory_still_reports_when_it_is_the_only_source() {
        let mut camera_info = Vec::new();
        camera_info.extend_from_slice(&76_400u32.to_be_bytes());
        camera_info.resize(20, 0);

        let data = pentax_block(&[(0x0215, 4, 5, 64)], &camera_info);

        let mut tags = HashMap::new();
        PentaxParser::default()
            .parse(&data, ByteOrder::BigEndian, &mut tags)
            .expect("Pentax MakerNote should parse");

        assert_eq!(
            tags.get("Pentax:PentaxModelID").map(String::as_str),
            Some("Optio SV"),
        );
    }

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

    // Every string asserted below was read out of ExifTool's own PrintConv
    // hash, dumped from the Perl symbol table (ExifTool 13.59) -- not out of
    // an earlier revision of this file. The ids are the ones oxidex and
    // ExifTool used to disagree on, so restoring a table fails the test.

    /// FocusMode (0x000D), Pentax.pm:1165-1206. Not a near-miss: the old
    /// six-entry table shared ids 0..5 with ExifTool's and agreed on none of
    /// them, and it had no entry at all for the 0x10/0x110 blocks that every
    /// DSLR in the corpus actually writes.
    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(FOCUS_MODE.decode(0), "Normal");
        assert_eq!(FOCUS_MODE.decode(1), "Macro");
        assert_eq!(FOCUS_MODE.decode(2), "Infinity");
        assert_eq!(FOCUS_MODE.decode(3), "Manual");
        assert_eq!(FOCUS_MODE.decode(4), "Super Macro");
        assert_eq!(FOCUS_MODE.decode(5), "Pan Focus");
        assert_eq!(FOCUS_MODE.decode(0x10), "AF-S (Focus-priority)");
        assert_eq!(FOCUS_MODE.decode(0x11), "AF-C (Focus-priority)");
        assert_eq!(FOCUS_MODE.decode(0x110), "AF-S (Release-priority)");
        assert_eq!(
            FOCUS_MODE.decode(0x120),
            "Contrast-detect (Release-priority)"
        );
        assert_eq!(FOCUS_MODE.decode(0x8003), "Manual (Macro)");
        assert_eq!(FOCUS_MODE.decode(0x800b), "Continuous (Macro)");
        // The invented labels this replaced appear nowhere in the table now.
        for id in 0..=0x8100 {
            let s = FOCUS_MODE.decode(id);
            assert!(
                !s.contains("(Single)") && !s.contains("(Continuous)"),
                "{id:#x} -> {s}"
            );
        }
    }

    /// MeteringMode (0x0017), Pentax.pm:1364-1374: lower-case "average", and
    /// Highlight sits at 6 with nothing at 3 or 4.
    #[test]
    fn test_decode_metering_mode() {
        assert_eq!(METERING_MODE.decode(0), "Multi-segment");
        assert_eq!(METERING_MODE.decode(1), "Center-weighted average");
        assert_eq!(METERING_MODE.decode(2), "Spot");
        assert_eq!(METERING_MODE.decode(6), "Highlight");
        assert_eq!(METERING_MODE.decode(3), "Unknown (3)");
        assert_eq!(METERING_MODE.decode(4), "Unknown (4)");
    }

    /// WhiteBalanceMode (0x001A), Pentax.pm:1385-1400.
    #[test]
    fn test_decode_white_balance_mode() {
        assert_eq!(WHITE_BALANCE_MODE.decode(10), "Auto (Cloudy)");
        assert_eq!(WHITE_BALANCE_MODE.decode(0xfffe), "Unknown");
        assert_eq!(WHITE_BALANCE_MODE.decode(0xffff), "User-Selected");
    }

    /// RawDevelopmentProcess (0x0062), Pentax.pm:2251-2277: each version is
    /// named after the bodies that use it, and there is no version 2.
    #[test]
    fn test_decode_raw_development_process() {
        assert_eq!(
            RAW_DEVELOPMENT_PROCESS.decode(1),
            "1 (K10D,K200D,K2000,K-m)"
        );
        assert_eq!(RAW_DEVELOPMENT_PROCESS.decode(6), "6 (645D)");
        assert_eq!(RAW_DEVELOPMENT_PROCESS.decode(16), "16 (K-1)");
        assert_eq!(RAW_DEVELOPMENT_PROCESS.decode(21), "21 (K-3IIIMonochrome)");
        assert_eq!(RAW_DEVELOPMENT_PROCESS.decode(2), "Unknown (2)");
    }

    /// ShutterType (0x0087) and NoiseReduction (0x0049): Pentax.pm:2649 and
    /// :2183. Neither has the extra strength levels oxidex used to print.
    #[test]
    fn test_decode_shutter_type_and_noise_reduction() {
        assert_eq!(SHUTTER_TYPE.decode(0), "Normal");
        assert_eq!(SHUTTER_TYPE.decode(1), "Electronic");
        assert_eq!(NOISE_REDUCTION.decode(0), "Off");
        assert_eq!(NOISE_REDUCTION.decode(1), "On");
        assert_eq!(NOISE_REDUCTION.decode(2), "Unknown (2)");
    }

    /// MonochromeFilterEffect (0x0073) and MonochromeToning (0x0074):
    /// Pentax.pm:2456-2484. The filter table was off by one and "None" is
    /// 0xffff; toning is a -4..+4 scale, not a set of colour names.
    #[test]
    fn test_decode_monochrome_tables() {
        assert_eq!(MONOCHROME_FILTER_EFFECT.decode(1), "Green");
        assert_eq!(MONOCHROME_FILTER_EFFECT.decode(2), "Yellow");
        assert_eq!(MONOCHROME_FILTER_EFFECT.decode(8), "Infrared");
        assert_eq!(MONOCHROME_FILTER_EFFECT.decode(0xffff), "None");
        assert_eq!(MONOCHROME_FILTER_EFFECT.decode(0), "Unknown (0)");

        assert_eq!(MONOCHROME_TONING.decode(0), "-4");
        assert_eq!(MONOCHROME_TONING.decode(4), "0");
        assert_eq!(MONOCHROME_TONING.decode(8), "4");
        assert_eq!(MONOCHROME_TONING.decode(0xffff), "None");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(WHITE_BALANCE.decode(0), "Auto");
        assert_eq!(WHITE_BALANCE.decode(1), "Daylight");
        assert_eq!(WHITE_BALANCE.decode(5), "Manual");
        assert_eq!(WHITE_BALANCE.decode(9), "Flash");
    }

    /// `Pentax_istD.jpg`'s actual 73-entry "AOC\0" IFD (dumped verbatim from
    /// the file's bytes). Its out-of-line entries are all wrong under a
    /// literal TIFF-header-relative read -- ExifTool's `FixBase` finds the
    /// block's true base is 14, not the 30 the TIFF header sits at. Verified
    /// against `exiftool -v4`: LensInfo's (0x0207) 36 bytes live at file
    /// offset 1918 (= 30 + 1904 - 16) and BatteryInfo's (0x0216) 6 bytes at
    /// 2114 (= 30 + 2100 - 16); the byte ranges this test's corrected offsets
    /// resolve to were diffed against ExifTool's raw dump and match exactly.
    #[test]
    fn test_pentax_fix_base_recovers_istd_offset() {
        let entries = vec![
            IfdEntry {
                tag_id: 0x0001,
                field_type: 3,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x0002,
                field_type: 3,
                value_count: 2,
                value_offset: 41943520,
            },
            IfdEntry {
                tag_id: 0x0003,
                field_type: 4,
                value_count: 1,
                value_offset: 40648,
            },
            IfdEntry {
                tag_id: 0x0004,
                field_type: 4,
                value_count: 1,
                value_offset: 6236,
            },
            IfdEntry {
                tag_id: 0x0005,
                field_type: 4,
                value_count: 1,
                value_offset: 76180,
            },
            IfdEntry {
                tag_id: 0x0006,
                field_type: 7,
                value_count: 4,
                value_offset: 131270929,
            },
            IfdEntry {
                tag_id: 0x0007,
                field_type: 7,
                value_count: 3,
                value_offset: 169878272,
            },
            IfdEntry {
                tag_id: 0x0008,
                field_type: 3,
                value_count: 1,
                value_offset: 131072,
            },
            IfdEntry {
                tag_id: 0x0009,
                field_type: 3,
                value_count: 2,
                value_offset: 2359296,
            },
            IfdEntry {
                tag_id: 0x000a,
                field_type: 3,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x000c,
                field_type: 3,
                value_count: 1,
                value_offset: 65536,
            },
            IfdEntry {
                tag_id: 0x000d,
                field_type: 3,
                value_count: 1,
                value_offset: 1048576,
            },
            IfdEntry {
                tag_id: 0x000e,
                field_type: 3,
                value_count: 1,
                value_offset: 4294836224,
            },
            IfdEntry {
                tag_id: 0x0012,
                field_type: 4,
                value_count: 1,
                value_offset: 1111,
            },
            IfdEntry {
                tag_id: 0x0013,
                field_type: 3,
                value_count: 1,
                value_offset: 2949120,
            },
            IfdEntry {
                tag_id: 0x0014,
                field_type: 3,
                value_count: 1,
                value_offset: 589824,
            },
            IfdEntry {
                tag_id: 0x0016,
                field_type: 3,
                value_count: 1,
                value_offset: 3276800,
            },
            IfdEntry {
                tag_id: 0x0017,
                field_type: 3,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x0018,
                field_type: 3,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x0019,
                field_type: 3,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x001a,
                field_type: 3,
                value_count: 1,
                value_offset: 65536,
            },
            IfdEntry {
                tag_id: 0x001d,
                field_type: 4,
                value_count: 1,
                value_offset: 2800,
            },
            IfdEntry {
                tag_id: 0x001f,
                field_type: 3,
                value_count: 2,
                value_offset: 65536,
            },
            IfdEntry {
                tag_id: 0x0020,
                field_type: 3,
                value_count: 2,
                value_offset: 65536,
            },
            IfdEntry {
                tag_id: 0x0021,
                field_type: 3,
                value_count: 2,
                value_offset: 65536,
            },
            IfdEntry {
                tag_id: 0x0022,
                field_type: 3,
                value_count: 1,
                value_offset: 65536,
            },
            IfdEntry {
                tag_id: 0x0023,
                field_type: 3,
                value_count: 1,
                value_offset: 1310720,
            },
            IfdEntry {
                tag_id: 0x0024,
                field_type: 3,
                value_count: 1,
                value_offset: 3014656,
            },
            IfdEntry {
                tag_id: 0x0025,
                field_type: 3,
                value_count: 1,
                value_offset: 65536,
            },
            IfdEntry {
                tag_id: 0x0026,
                field_type: 3,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x0027,
                field_type: 7,
                value_count: 4,
                value_offset: 4278190074,
            },
            IfdEntry {
                tag_id: 0x0028,
                field_type: 7,
                value_count: 4,
                value_offset: 4278190074,
            },
            IfdEntry {
                tag_id: 0x0029,
                field_type: 4,
                value_count: 1,
                value_offset: 464,
            },
            IfdEntry {
                tag_id: 0x002b,
                field_type: 4,
                value_count: 1,
                value_offset: 40960,
            },
            IfdEntry {
                tag_id: 0x002c,
                field_type: 4,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x002d,
                field_type: 3,
                value_count: 1,
                value_offset: 671088640,
            },
            IfdEntry {
                tag_id: 0x0033,
                field_type: 1,
                value_count: 3,
                value_offset: 33554432,
            },
            IfdEntry {
                tag_id: 0x0034,
                field_type: 1,
                value_count: 4,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x0035,
                field_type: 3,
                value_count: 2,
                value_offset: 787226451,
            },
            IfdEntry {
                tag_id: 0x0036,
                field_type: 3,
                value_count: 1,
                value_offset: 8388608,
            },
            IfdEntry {
                tag_id: 0x0037,
                field_type: 3,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x003a,
                field_type: 3,
                value_count: 1,
                value_offset: 1181089792,
            },
            IfdEntry {
                tag_id: 0x003c,
                field_type: 7,
                value_count: 4,
                value_offset: 2097184,
            },
            IfdEntry {
                tag_id: 0x003d,
                field_type: 3,
                value_count: 1,
                value_offset: 536870912,
            },
            IfdEntry {
                tag_id: 0x003e,
                field_type: 1,
                value_count: 4,
                value_offset: 437911552,
            },
            IfdEntry {
                tag_id: 0x003f,
                field_type: 1,
                value_count: 2,
                value_offset: 69599232,
            },
            IfdEntry {
                tag_id: 0x0047,
                field_type: 6,
                value_count: 1,
                value_offset: 452984832,
            },
            IfdEntry {
                tag_id: 0x0048,
                field_type: 3,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x0049,
                field_type: 3,
                value_count: 1,
                value_offset: 0,
            },
            IfdEntry {
                tag_id: 0x0200,
                field_type: 3,
                value_count: 4,
                value_offset: 1808,
            },
            IfdEntry {
                tag_id: 0x0201,
                field_type: 3,
                value_count: 4,
                value_offset: 1816,
            },
            IfdEntry {
                tag_id: 0x0202,
                field_type: 3,
                value_count: 4,
                value_offset: 1824,
            },
            IfdEntry {
                tag_id: 0x0203,
                field_type: 8,
                value_count: 9,
                value_offset: 1832,
            },
            IfdEntry {
                tag_id: 0x0204,
                field_type: 8,
                value_count: 9,
                value_offset: 1852,
            },
            IfdEntry {
                tag_id: 0x0205,
                field_type: 7,
                value_count: 16,
                value_offset: 1872,
            },
            IfdEntry {
                tag_id: 0x0206,
                field_type: 7,
                value_count: 14,
                value_offset: 1888,
            },
            IfdEntry {
                tag_id: 0x0207,
                field_type: 7,
                value_count: 36,
                value_offset: 1904,
            },
            IfdEntry {
                tag_id: 0x0208,
                field_type: 7,
                value_count: 28,
                value_offset: 1940,
            },
            IfdEntry {
                tag_id: 0x0209,
                field_type: 7,
                value_count: 16,
                value_offset: 1968,
            },
            IfdEntry {
                tag_id: 0x020a,
                field_type: 7,
                value_count: 16,
                value_offset: 1984,
            },
            IfdEntry {
                tag_id: 0x020b,
                field_type: 7,
                value_count: 16,
                value_offset: 2000,
            },
            IfdEntry {
                tag_id: 0x020d,
                field_type: 3,
                value_count: 4,
                value_offset: 2016,
            },
            IfdEntry {
                tag_id: 0x020e,
                field_type: 3,
                value_count: 4,
                value_offset: 2024,
            },
            IfdEntry {
                tag_id: 0x020f,
                field_type: 3,
                value_count: 4,
                value_offset: 2032,
            },
            IfdEntry {
                tag_id: 0x0210,
                field_type: 3,
                value_count: 4,
                value_offset: 2040,
            },
            IfdEntry {
                tag_id: 0x0211,
                field_type: 3,
                value_count: 4,
                value_offset: 2048,
            },
            IfdEntry {
                tag_id: 0x0212,
                field_type: 3,
                value_count: 4,
                value_offset: 2056,
            },
            IfdEntry {
                tag_id: 0x0213,
                field_type: 3,
                value_count: 4,
                value_offset: 2064,
            },
            IfdEntry {
                tag_id: 0x0214,
                field_type: 3,
                value_count: 4,
                value_offset: 2072,
            },
            IfdEntry {
                tag_id: 0x0215,
                field_type: 4,
                value_count: 5,
                value_offset: 2080,
            },
            IfdEntry {
                tag_id: 0x0216,
                field_type: 7,
                value_count: 6,
                value_offset: 2100,
            },
            IfdEntry {
                tag_id: 0x03ff,
                field_type: 3,
                value_count: 16,
                value_offset: 2108,
            },
            IfdEntry {
                tag_id: 0x0402,
                field_type: 1,
                value_count: 4096,
                value_offset: 2140,
            },
        ];
        assert_eq!(entries.len(), 73);

        // dir_start = 6 ("AOC\0" + "MM"), data_pos = 934 - 30 = 904 (payload
        // start minus TIFF header, both real file offsets in istD.jpg).
        let fix = pentax_fix_base(&entries, 6, 904);
        assert_eq!(fix, -16);

        let value_base = -904 + fix;
        assert_eq!(1904 + value_base, 1918 - 934); // LensInfo, window-relative
        assert_eq!(2100 + value_base, 2114 - 934); // BatteryInfo, window-relative
    }

    #[test]
    fn test_extract_value_uses_right_aligned_inline_values() {
        let entry = IfdEntry {
            tag_id: PENTAX_QUALITY,
            field_type: 1,
            value_count: 1,
            value_offset: 0x0300_0000,
        };

        let entry = right_align_inline_value(entry, ByteOrder::BigEndian);
        assert_eq!(extract_value_as_i32(&entry, ByteOrder::BigEndian), 3);

        let entry = IfdEntry {
            tag_id: PENTAX_WHITE_BALANCE,
            field_type: 3,
            value_count: 1,
            value_offset: 0x0009_0000,
        };

        let entry = right_align_inline_value(entry, ByteOrder::BigEndian);
        assert_eq!(extract_value_as_i32(&entry, ByteOrder::BigEndian), 9);

        let entry = IfdEntry {
            tag_id: PENTAX_WHITE_BALANCE,
            field_type: 3,
            value_count: 1,
            value_offset: 9,
        };
        assert_eq!(extract_value_as_i32(&entry, ByteOrder::LittleEndian), 9);
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
        let parser = PentaxParser::default();
        assert_eq!(parser.manufacturer_name(), "Pentax");
        assert_eq!(parser.tag_prefix(), "Pentax:");
    }

    #[test]
    fn test_validate_header_aoc() {
        let parser = PentaxParser::default();

        let valid_header = b"AOC\0extra_data_here";
        assert!(parser.validate_header(valid_header));

        let invalid_header = b"Canon\0\0\0";
        assert!(!parser.validate_header(invalid_header));
    }

    #[test]
    fn test_validate_header_pentax() {
        let parser = PentaxParser::default();

        let valid_header = b"PENTAX \0more_data";
        assert!(parser.validate_header(valid_header));
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

    /// The `%Pentax::Main` `Condition`s that pick a table, exactly as
    /// ExifTool writes them -- a record that matches none must descend into
    /// nothing rather than into a neighbour's layout.
    #[test]
    fn test_subdir_conditions_match_exiftool() {
        fn entry(tag_id: u16, value_count: u32) -> IfdEntry {
            IfdEntry {
                tag_id,
                field_type: 7,
                value_count,
                value_offset: 0,
            }
        }
        let none = binary_subdir::Members::new();
        let mut faces = binary_subdir::Members::new();
        faces.insert("FacesDetected", 2);
        let le = ByteOrder::LittleEndian;
        let pick = |e: &IfdEntry, model, m: &binary_subdir::Members| {
            pentax_binary_subdir(e, model, false, m, le).map(|(t, _)| t.name)
        };

        // 0x005c: `$count == 4` is SRInfo, handled elsewhere; anything else SRInfo2.
        assert_eq!(pick(&entry(0x005C, 4), None, &none), None);
        assert_eq!(pick(&entry(0x005C, 2), None, &none), Some("SRInfo2"));
        // 0x0208: `$count == 27`, else FlashInfoUnknown (no named tags).
        assert_eq!(pick(&entry(0x0208, 27), None, &none), Some("FlashInfo"));
        assert_eq!(pick(&entry(0x0208, 26), None, &none), None);
        // 0x022d: `$count == 100`.
        assert_eq!(pick(&entry(0x022D, 100), None, &none), Some("WBLevels"));
        assert_eq!(pick(&entry(0x022D, 96), None, &none), None);
        // 0x0224: `Drop => 200` discards the tag above 200 bytes.
        assert_eq!(pick(&entry(0x0224, 200), None, &none), Some("EVStepInfo"));
        assert_eq!(pick(&entry(0x0224, 40000), None, &none), None);
        // 0x0227/0x0228: `$$self{FacesDetected}`, set by 0x0060.
        assert_eq!(pick(&entry(0x0227, 32), None, &none), None);
        assert_eq!(pick(&entry(0x0227, 32), None, &faces), Some("FacePos"));
        assert_eq!(pick(&entry(0x0228, 32), None, &faces), Some("FaceSize"));
        // 0x022b: the K-3 Mark III has a different layout, not transcribed.
        assert_eq!(
            pick(&entry(0x022B, 8), Some("K-5"), &none),
            Some("LevelInfo")
        );
        assert_eq!(
            pick(&entry(0x022B, 8), Some("PENTAX K-3 Mark III"), &none),
            None
        );
    }

    /// Decodes `record` the way the dispatcher would, for a named model.
    fn decode_for(table: &BinaryTable, record: &[u8], model: &str) -> HashMap<String, String> {
        let mut tags = HashMap::new();
        let mut members = binary_subdir::Members::new();
        binary_subdir::decode_binary_subdir_with(
            table,
            record,
            ByteOrder::BigEndian,
            "Pentax",
            Some(model),
            &mut members,
            &mut tags,
        );
        tags
    }

    /// `combined-samples/Pentax/PentaxK10D.jpg` tag 0x0216: the exact 6 record
    /// bytes `exiftool -v3` prints, against the exact values `exiftool -a -G1
    /// -s` reports for that file.
    ///
    /// Every alternative in `%Pentax::BatteryInfo` is `Condition`-guarded, so
    /// this is the test that the model test picks ExifTool's branch: byte 2 is
    /// `BodyBatteryADNoLoad` with the K10D's calibration `PrintConv` here, and
    /// two bytes of `BodyBatteryVoltage1` on a K-5.
    #[test]
    fn test_battery_info_matches_exiftool_on_k10d_bytes() {
        let tags = decode_for(
            &PENTAX_BATTERYINFO,
            &[0x02, 0x41, 0xa5, 0xa0, 0x05, 0x01],
            "PENTAX K10D",
        );
        assert_eq!(tags["Pentax:PowerSource"], "Body Battery");
        assert_eq!(tags["Pentax:BodyBatteryState"], "Full");
        assert_eq!(tags["Pentax:GripBatteryState"], "Empty or Missing");
        assert_eq!(tags["Pentax:BodyBatteryADNoLoad"], "165 (7.3V, 28%)");
        assert_eq!(tags["Pentax:BodyBatteryADLoad"], "160 (7.0V, 23%)");
        assert_eq!(tags["Pentax:GripBatteryADNoLoad"], "5");
        assert_eq!(tags["Pentax:GripBatteryADLoad"], "1");
        // The K10D has no voltage reading at all -- those alternatives belong
        // to other bodies, and emitting one here would put a plausible number
        // under a real tag name.
        assert!(!tags.contains_key("Pentax:BodyBatteryVoltage1"));
    }

    /// The same table over `combined-samples/Pentax/PentaxK-5IIs.jpg`'s own
    /// 0x0216 bytes: the *third* alternative of key 2 now applies, so bytes 2-3
    /// are one `int16u` of centivolts rather than two independent A/D readings.
    #[test]
    fn test_battery_info_matches_exiftool_on_k5iis_bytes() {
        let tags = decode_for(
            &PENTAX_BATTERYINFO,
            &[
                0xf2, 0x50, 0x02, 0xae, 0x02, 0x8f, 0x02, 0xc4, 0x02, 0xa4, 0x00, 0x00,
            ],
            "PENTAX K-5 II s",
        );
        assert_eq!(tags["Pentax:PowerSource"], "Body Battery");
        assert_eq!(tags["Pentax:BodyBatteryState"], "Full");
        assert_eq!(tags["Pentax:BodyBatteryVoltage1"], "6.86 V");
        assert_eq!(tags["Pentax:BodyBatteryVoltage2"], "6.55 V");
        assert_eq!(tags["Pentax:BodyBatteryVoltage3"], "7.08 V");
        assert_eq!(tags["Pentax:BodyBatteryVoltage4"], "6.76 V");
        assert!(!tags.contains_key("Pentax:BodyBatteryADNoLoad"));
        assert!(!tags.contains_key("Pentax:GripBatteryADNoLoad"));
    }

    /// `combined-samples/Pentax/PentaxK10D.jpg` tag 0x021f, all 12 bytes.
    ///
    /// `AFIntegrationTime` is the `ValueConv`-then-expression-`PrintConv` case:
    /// the raw 0 doubles to 0 ms, and ExifTool prints the number bare rather
    /// than as "0.0".
    #[test]
    fn test_af_info_matches_exiftool_on_k10d_bytes() {
        let tags = decode_for(
            &PENTAX_AFINFO,
            &[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x0f,
            ],
            "PENTAX K10D",
        );
        assert_eq!(tags["Pentax:AFPredictor"], "15");
        assert_eq!(tags["Pentax:AFDefocus"], "10");
        assert_eq!(tags["Pentax:AFIntegrationTime"], "0 ms");
        assert_eq!(tags["Pentax:AFPointsInFocus"], "Lower-right, Mid-right");
    }

    /// `combined-samples/Pentax/PentaxK-5IIs.jpg` tags 0x03ff, 0x0226 and
    /// 0x022a -- the first 24 bytes of the 256-byte `TempInfo` record, all 11
    /// of `ShotInfo`, and the leading zeros of `FilterInfo`.
    #[test]
    fn test_temp_shot_and_filter_info_match_exiftool_on_k5iis_bytes() {
        let temp = decode_for(
            &PENTAX_TEMPINFO,
            &[
                0x00, 0x04, 0x00, 0x01, 0x82, 0x3f, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0xcd,
                0x00, 0xcd, 0x00, 0x00, 0x00, 0xfa, 0x00, 0x14, 0x00, 0x14,
            ],
            "PENTAX K-5 II s",
        );
        assert_eq!(temp["Pentax:SensorTemperature"], "20.5 C");
        assert_eq!(temp["Pentax:SensorTemperature2"], "20.5 C");
        assert_eq!(temp["Pentax:CameraTemperature4"], "20 C");
        assert_eq!(temp["Pentax:CameraTemperature5"], "20 C");

        let shot = decode_for(
            &PENTAX_SHOTINFO,
            &[
                0xf0, 0x10, 0x7b, 0xff, 0xaa, 0xfc, 0x0e, 0x06, 0x00, 0x00, 0x00,
            ],
            "PENTAX K-5 II s",
        );
        assert_eq!(shot["Pentax:CameraOrientation"], "Horizontal (normal)");

        let filter = decode_for(&PENTAX_FILTERINFO, &[0x00; 8], "PENTAX K-5 II s");
        assert_eq!(filter["Pentax:SourceDirectoryIndex"], "0");
        assert_eq!(filter["Pentax:SourceFileIndex"], "0");
    }

    /// The 0x03ff alternative list: `TempInfo` only on the bodies ExifTool
    /// names, and `\b` is what keeps a K-5 pattern off a K-50.
    #[test]
    fn test_temp_info_model_gate_respects_word_boundaries() {
        let e = IfdEntry {
            tag_id: PENTAX_TEMP_INFO,
            field_type: 7,
            value_count: 256,
            value_offset: 0,
        };
        let none = binary_subdir::Members::new();
        let pick = |model| {
            pentax_binary_subdir(&e, model, false, &none, ByteOrder::BigEndian).map(|(t, _)| t.name)
        };
        assert_eq!(pick(Some("PENTAX K-5 II s")), Some("TempInfo"));
        assert_eq!(pick(Some("PENTAX K-50")), Some("TempInfo"));
        assert_eq!(pick(Some("PENTAX K-500")), Some("TempInfo"));
        // Not in the list: 0x03ff is `UnknownInfo`, which declares no tags.
        assert_eq!(pick(Some("PENTAX K-7")), None);
        assert_eq!(pick(Some("PENTAX K10D")), None);
        assert_eq!(pick(None), None);
    }

    /// 0x022a is one table read either way round, chosen by `$$self{Make}`
    /// rather than the model (Pentax.pm:3030-3042).
    #[test]
    fn test_filter_info_byte_order_follows_the_brand() {
        let e = IfdEntry {
            tag_id: PENTAX_FILTER_INFO,
            field_type: 7,
            value_count: 345,
            value_offset: 0,
        };
        let none = binary_subdir::Members::new();
        let order = |ricoh| {
            pentax_binary_subdir(
                &e,
                Some("PENTAX K-5 II s"),
                ricoh,
                &none,
                ByteOrder::BigEndian,
            )
            .map(|(_, o)| o)
        };
        assert_eq!(order(false), Some(ByteOrder::BigEndian));
        assert_eq!(order(true), Some(ByteOrder::LittleEndian));
    }

    /// `combined-samples/Pentax/PentaxK-5.jpg` tag 0x022b: the exact 8 record
    /// bytes `exiftool -v3` prints, and the exact values `exiftool -a -G1 -s`
    /// reports. This is the `ValueConv` case -- `RollAngle` is an int8s of 1
    /// and prints -0.5, not 1.
    #[test]
    fn test_level_info_matches_exiftool_on_k5_bytes() {
        let mut tags = HashMap::new();
        let mut members = binary_subdir::Members::new();
        binary_subdir::decode_binary_subdir_with(
            &PENTAX_LEVELINFO,
            &[0x21, 0x01, 0xf6, 0x00, 0x12, 0xfc, 0x01, 0x00],
            ByteOrder::BigEndian,
            "Pentax",
            None,
            &mut members,
            &mut tags,
        );
        assert_eq!(tags["Pentax:LevelOrientation"], "Horizontal (normal)");
        assert_eq!(tags["Pentax:CompositionAdjust"], "Composition Adjust");
        assert_eq!(tags["Pentax:RollAngle"], "-0.5");
        assert_eq!(tags["Pentax:PitchAngle"], "5");
        assert_eq!(tags["Pentax:CompositionAdjustX"], "4");
        assert_eq!(tags["Pentax:CompositionAdjustY"], "-1");
        assert_eq!(tags["Pentax:CompositionAdjustRotation"], "0");
    }

    /// `combined-samples/Pentax/PentaxK-5.jpg` tag 0x006b, 4 bytes: `TimeInfo`
    /// packs four fields into two bytes with `Mask`, so an unmasked read would
    /// report one number where ExifTool reports four tags.
    #[test]
    fn test_time_info_masks_match_exiftool_on_k5_bytes() {
        let mut tags = HashMap::new();
        let mut members = binary_subdir::Members::new();
        binary_subdir::decode_binary_subdir_with(
            &PENTAX_TIMEINFO,
            &[0x00, 0x00, 0x0b, 0x0b],
            ByteOrder::BigEndian,
            "Pentax",
            None,
            &mut members,
            &mut tags,
        );
        assert_eq!(tags["Pentax:WorldTimeLocation"], "Hometown");
        assert_eq!(tags["Pentax:HometownDST"], "No");
        assert_eq!(tags["Pentax:DestinationDST"], "No");
        assert_eq!(tags["Pentax:HometownCity"], "Toronto");
        assert_eq!(tags["Pentax:DestinationCity"], "Toronto");
    }
}

// ============================================================================
// Value Extraction Helpers for Byte Order Handling
// ============================================================================

fn extract_u8_value(entry: &IfdEntry, _byte_order: ByteOrder) -> u8 {
    // `right_align_inline_value` normalizes both byte orders before tag
    // dispatch, so inline BYTE values always occupy the low byte here.
    (entry.value_offset & 0xFF) as u8
}

fn extract_u16_value(entry: &IfdEntry, _byte_order: ByteOrder) -> u16 {
    // `right_align_inline_value` normalizes both byte orders before tag
    // dispatch, so inline SHORT values always occupy the low two bytes here.
    (entry.value_offset & 0xFFFF) as u16
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
