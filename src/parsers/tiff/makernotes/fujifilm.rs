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

/// The `OTHER` fallbacks of `%FujiFilm`'s settings tables, hand-written.
mod print_conv;
/// `%FujiFilm` binary sub-tables, generated from ExifTool's own hashes.
pub mod settings_tables;

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

use super::shared::MakerNoteParser;
use super::shared::array_extractors::{
    extract_i16_array, extract_i32_array, extract_rational_array, extract_u16_array,
    extract_u32_array,
};
use super::shared::binary_subdir::{BinaryTable, decode_binary_subdir};
use crate::const_decoder;
use crate::core::formatters::numeric_precision::perl_number;
use crate::core::value_formatter::format_rational_as_decimal;
use settings_tables::{
    FUJIFILM_AFCSETTINGS, FUJIFILM_DRIVESETTINGS, FUJIFILM_FOCUSSETTINGS, FUJIFILM_PRIORITYSETTINGS,
};

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
const FUJI_SHUTTER_TYPE: u16 = 0x1050;

// Film Simulation and Color Tags
//
// NOTE: 0x1400-0x1407 were previously off by one relative to ExifTool's
// FujiFilm.pm (e.g. DynamicRange was mapped to 0x1402 instead of 0x1400),
// which cascaded into every tag from DynamicRange through
// MaxApertureAtMaxFocal being misread. Verified against ExifTool 13.59.
const FUJI_FILM_MODE: u16 = 0x1401;
const FUJI_DYNAMIC_RANGE: u16 = 0x1400;
const FUJI_DYNAMIC_RANGE_SETTING: u16 = 0x1402;
const FUJI_DEVELOPMENT_DYNAMIC_RANGE: u16 = 0x1403;
const FUJI_MIN_FOCAL_LENGTH: u16 = 0x1404;
const FUJI_MAX_FOCAL_LENGTH: u16 = 0x1405;
const FUJI_MAX_APERTURE_AT_MIN_FOCAL: u16 = 0x1406;
const FUJI_MAX_APERTURE_AT_MAX_FOCAL: u16 = 0x1407;

// Advanced Camera Settings
const FUJI_AUTO_DYNAMIC_RANGE: u16 = 0x140B;
const FUJI_FACES_DETECTED: u16 = 0x4100;
const FUJI_FACE_POSITIONS: u16 = 0x4103;
const FUJI_FACE_REC_INFO: u16 = 0x4282;
// NOTE: 0x1100/0x1101 were previously mapped to the non-existent "ShutterType"
// and "BurstMode" tags. ExifTool's FujiFilm.pm defines 0x1100 as
// AutoBracketing and 0x1101 as SequenceNumber; there is no ShutterType or
// BurstMode tag at these IDs (ShutterType is actually at 0x1050, which is
// not currently handled here).
const FUJI_AUTO_BRACKETING: u16 = 0x1100;
const FUJI_SEQUENCE_NUMBER: u16 = 0x1101;
const FUJI_EXPOSURE_COUNT: u16 = 0x1032;
const FUJI_BLUR_WARNING: u16 = 0x1300;
const FUJI_FOCUS_WARNING: u16 = 0x1301;
const FUJI_EXPOSURE_WARNING: u16 = 0x1302;

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
//
// FujiFilm.pm:828 (`0x1436 => { Name => 'ImageGeneration', ...}`) -- this was
// previously 0x1047, which is GrainEffectRoughness's real tag ID (see the note
// on FUJI_GRAIN_EFFECT_ROUGHNESS below); the swap meant ImageGeneration was
// never matched in the parse loop at all (0x1047's real bytes were read only
// under GrainEffectRoughness's wrong ID, 0x1046), and GrainEffectRoughness's
// real bytes at 0x1047 were never read under any name. Verified against
// combined-samples/FujiFilm/FujiFilmGFX100II.jpg: exiftool -G1 -s -a reports
// `[FujiFilm] ImageGeneration : Re-developed from RAW`.
const FUJI_IMAGE_GENERATION: u16 = 0x1436;
const FUJI_RATING: u16 = 0x1431;
const FUJI_IMAGE_COUNT: u16 = 0x1438;
const FUJI_DRIVE_MODE: u16 = 0x1039;

// ===== NEW TAGS - Additional MakerNotes coverage =====

// Additional Image Quality Tags
const FUJI_WHITE_BALANCE_FINE_TUNE: u16 = 0x100A;
const FUJI_NOISE_REDUCTION: u16 = 0x100B;
const FUJI_HIGH_ISO_NOISE_REDUCTION: u16 = 0x100E;
const FUJI_AF_MODE: u16 = 0x1022;
const FUJI_EXR_MODE_SETTING: u16 = 0x1034; // Note: maps to 0x1034 (EXR_MODE is 0x1034 in original)
const FUJI_LENS_MODULATION_OPTIMIZER: u16 = 0x1045;
// FujiFilm.pm:492 (`0x1047 => { Name => 'GrainEffectRoughness', ...}`) -- was
// 0x1046 (an ID FujiFilm.pm has no entry for at all), which read one byte
// short of the real field and produced garbage (verified: oxidex printed
// "Unknown (1)" against GFX100II.jpg where exiftool reports "Off"). See the
// FUJI_IMAGE_GENERATION note above for the other half of this swap.
const FUJI_GRAIN_EFFECT_ROUGHNESS: u16 = 0x1047;
const FUJI_COLOR_CHROME_EFFECT: u16 = 0x1048;
const FUJI_BW_ADJUSTMENT: u16 = 0x1049;
/// FujiFilm.pm:524-532 (`0x104c => { Name => "GrainEffectSize", ...}`, its own
/// distinct `PrintConv` -- 0/16/32, not GrainEffectRoughness's 0/32/64).
const FUJI_GRAIN_EFFECT_SIZE: u16 = 0x104C;
const FUJI_CROP_MODE: u16 = 0x104D;
const FUJI_COLOR_CHROME_FX_BLUE: u16 = 0x104E;
/// FujiFilm.pm:871-872, :876-878. Found on newer bodies (GFX100 II, X-M5,
/// X-E5, ...); absent from older MakerNotes entirely.
const FUJI_FUJI_MODEL: u16 = 0x1447;
const FUJI_FUJI_MODEL2: u16 = 0x1448;
const FUJI_WB_RED: u16 = 0x144A;
const FUJI_WB_GREEN: u16 = 0x144B;
const FUJI_WB_BLUE: u16 = 0x144C;

// Packed settings words, each a `SubDirectory` over a ProcessBinaryData table
// in `%FujiFilm::Main` -- see `fujifilm_binary_subdir`.
const FUJI_PRIORITY_SETTINGS: u16 = 0x102B; // FujiFilm.pm:341
const FUJI_FOCUS_SETTINGS: u16 = 0x102D; // FujiFilm.pm:345
const FUJI_AFC_SETTINGS: u16 = 0x102E; // FujiFilm.pm:349

// Shooting Mode Tags
const FUJI_DRIVE_SETTINGS: u16 = 0x1103; // FujiFilm.pm:609
const FUJI_PIXEL_SHIFT_SHOTS: u16 = 0x1105;
const FUJI_PIXEL_SHIFT_OFFSET_NEW: u16 = 0x1106;
const FUJI_PANORAMA_ANGLE: u16 = 0x1153;
const FUJI_PANORAMA_DIRECTION: u16 = 0x1154;

// Advanced Filter Tags
const FUJI_ADVANCED_FILTER: u16 = 0x1201;
const FUJI_COLOR_MODE: u16 = 0x1210;

// Additional Dynamic Range Tags
const FUJI_IMAGE_STABILIZATION: u16 = 0x1422;
const FUJI_SCENE_RECOGNITION: u16 = 0x1425;
const FUJI_DRANGE_PRIORITY: u16 = 0x1443;
const FUJI_DRANGE_PRIORITY_AUTO: u16 = 0x1444;
const FUJI_DRANGE_PRIORITY_FIXED: u16 = 0x1445;

// Video Tags
const FUJI_VIDEO_RECORDING_MODE: u16 = 0x3803;
const FUJI_PERIPHERAL_LIGHTING: u16 = 0x3804;
const FUJI_VIDEO_COMPRESSION: u16 = 0x3806;
const FUJI_FRAME_RATE: u16 = 0x3820;
const FUJI_FRAME_WIDTH: u16 = 0x3821;
const FUJI_FRAME_HEIGHT: u16 = 0x3822;

// Additional Face Detection Tags
const FUJI_FACE_ELEMENT_SELECTED: u16 = 0x4005;
const FUJI_NUM_FACE_ELEMENTS: u16 = 0x4200;
const FUJI_FACE_ELEMENT_TYPES: u16 = 0x4201;
const FUJI_FACE_ELEMENT_POSITIONS: u16 = 0x4203;

// Fujifilm MakerNote header signature
// Fujifilm uses "FUJIFILM" followed by IFD offset
const FUJIFILM_HEADER: &[u8] = b"FUJIFILM";

// ============================================================================
// DECODERS - Fujifilm Value Decoders
// ============================================================================
// Following the shared decoder pattern from canon.rs and sony.rs
// Each decoder is a constant that implements the Decode trait

// Decodes Fujifilm quality setting to human-readable string
const_decoder!(pub
    DECODE_QUALITY, i32, [
        (1, "F (Fine)"),
        (2, "N (Normal)"),
        (3, "Fine"),
        (4, "Normal"),
        (5, "Fine+RAW"),
        (6, "Normal+RAW"),
    ]
);

// Decodes Fujifilm white balance setting to human-readable string
const_decoder!(pub
    DECODE_WHITE_BALANCE, i32, [
        (0x0000, "Auto"),
        (0x0001, "Auto (white priority)"),
        (0x0002, "Auto (ambiance priority)"),
        (0x0100, "Daylight"),
        (0x0200, "Cloudy"),
        (0x0300, "Daylight Fluorescent"),
        (0x0301, "Day White Fluorescent"),
        (0x0302, "White Fluorescent"),
        (0x0303, "Warm White Fluorescent"),
        (0x0304, "Living Room Warm White Fluorescent"),
        (0x0400, "Incandescent"),
        (0x0500, "Flash"),
        (0x0600, "Underwater"),
        (0x0F00, "Custom"),
        (0x0F01, "Custom2"),
        (0x0F02, "Custom3"),
        (0x0F03, "Custom4"),
        (0x0F04, "Custom5"),
        (0x0FF0, "Kelvin"),
    ]
);

// Decodes Fujifilm focus mode to human-readable string
const_decoder!(pub
    DECODE_FOCUS_MODE, i32, [
        (0, "Auto"),
        (1, "Manual"),
        (2, "AF-S (Single)"),
        (3, "AF-C (Continuous)"),
        (4, "AF-A (Automatic)"),
    ]
);

// Decodes Fujifilm flash mode to human-readable string.
//
// FujiFilm.pm:277-307, `0x1010 => { Name => 'FujiFlashMode', PrintHex => 1,
// PrintConv => { ... } }`, transcribed verbatim. Two things this table used to
// get wrong:
//
//   * value 3 is spelled `'Red-eye reduction'` (FujiFilm.pm:285) with a
//     lowercase "reduction". FujiFilm.jpg prints exactly that under
//     `exiftool -G1 -s`; oxidex printed `Red-eye Reduction`. (Nikon.pm:7157
//     is where the title-cased spelling lives, and it belongs to a different
//     tag.)
//   * everything above 4 was missing, so an X-T2/GFX-era body reported
//     `Unknown (32768)` where ExifTool prints `Not Attached`
//     (FujiFilm.pm:289, verified on FujiFilmGFX100II.jpg).
const_decoder!(pub
    DECODE_FLASH_MODE, i32, [
        (0, "Auto"),
        (1, "On"),
        (2, "Off"),
        (3, "Red-eye reduction"),
        (4, "External"),
        (16, "Commander"),
        (0x8000, "Not Attached"),
        (0x8120, "TTL"),
        (0x8320, "TTL Auto - Did not fire"),
        (0x9840, "Manual"),
        (0x9860, "Flash Commander"),
        (0x9880, "Multi-flash"),
        (0xa920, "1st Curtain (front)"),
        (0xaa20, "TTL Slow - 1st Curtain (front)"),
        (0xab20, "TTL Auto - 1st Curtain (front)"),
        (0xad20, "TTL - Red-eye Flash - 1st Curtain (front)"),
        (0xae20, "TTL Slow - Red-eye Flash - 1st Curtain (front)"),
        (0xaf20, "TTL Auto - Red-eye Flash - 1st Curtain (front)"),
        (0xc920, "2nd Curtain (rear)"),
        (0xca20, "TTL Slow - 2nd Curtain (rear)"),
        (0xcb20, "TTL Auto - 2nd Curtain (rear)"),
        (0xcd20, "TTL - Red-eye Flash - 2nd Curtain (rear)"),
        (0xce20, "TTL Slow - Red-eye Flash - 2nd Curtain (rear)"),
        (0xcf20, "TTL Auto - Red-eye Flash - 2nd Curtain (rear)"),
        (0xe920, "High Speed Sync (HSS)"),
    ]
);

// Decodes Fujifilm Sharpness (tag 0x1001) to human-readable string. Per
// ExifTool's FujiFilm.pm PrintHex table -- note this is NOT a simple linear
// scale (e.g. raw 3 means "0 (normal)", not "+3 (Hard)").
const_decoder!(pub
    DECODE_SHARPNESS, i32, [
        (0x00, "-4 (softest)"),
        (0x01, "-3 (very soft)"),
        (0x02, "-2 (soft)"),
        (0x03, "0 (normal)"),
        (0x04, "+2 (hard)"),
        (0x05, "+3 (very hard)"),
        (0x06, "+4 (hardest)"),
        (0x82, "-1 (medium soft)"),
        (0x84, "+1 (medium hard)"),
        (0x8000, "Film Simulation"),
        (0xFFFF, "n/a"),
    ]
);

// Decodes Fujifilm Saturation (tag 0x1003) to human-readable string. Per
// ExifTool's FujiFilm.pm PrintHex table.
const_decoder!(pub
    DECODE_SATURATION, i32, [
        (0x000, "0 (normal)"),
        (0x080, "+1 (medium high)"),
        (0x0c0, "+3 (very high)"),
        (0x0e0, "+4 (highest)"),
        (0x100, "+2 (high)"),
        (0x180, "-1 (medium low)"),
        (0x200, "Low"),
        (0x300, "None (B&W)"),
        (0x301, "B&W Red Filter"),
        (0x302, "B&W Yellow Filter"),
        (0x303, "B&W Green Filter"),
        (0x310, "B&W Sepia"),
        (0x400, "-2 (low)"),
        (0x4c0, "-3 (very low)"),
        (0x4e0, "-4 (lowest)"),
        (0x500, "Acros"),
        (0x501, "Acros Red Filter"),
        (0x502, "Acros Yellow Filter"),
        (0x503, "Acros Green Filter"),
        (0x8000, "Film Simulation"),
    ]
);

// Decodes Fujifilm Contrast (tag 0x1004) to human-readable string. Per
// ExifTool's FujiFilm.pm PrintHex table.
const_decoder!(pub
    DECODE_CONTRAST, i32, [
        (0x000, "Normal"),
        (0x080, "Medium High"),
        (0x100, "High"),
        (0x180, "Medium Low"),
        (0x200, "Low"),
        (0x8000, "Film Simulation"),
    ]
);

// Decodes Fujifilm film simulation mode to human-readable string
const_decoder!(pub
    DECODE_FILM_MODE, i32, [
        (0x0000, "F0/Standard (Provia)"),
        (0x0100, "F1/Studio Portrait"),
        (0x0110, "F1a/Studio Portrait Enhanced Saturation"),
        (0x0120, "F1b/Studio Portrait Smooth Skin Tone (Astia)"),
        (0x0130, "F1c/Studio Portrait Increased Sharpness"),
        (0x0200, "F2/Fujichrome (Velvia)"),
        (0x0300, "F3/Studio Portrait Ex"),
        (0x0400, "F4/Velvia"),
        (0x0500, "Pro Neg. Std"),
        (0x0501, "Pro Neg. Hi"),
        (0x0600, "Classic Chrome"),
        (0x0700, "Eterna"),
        (0x0800, "Classic Negative"),
        (0x0900, "Bleach Bypass"),
        (0x0A00, "Nostalgic Neg"),
        (0x0B00, "Reala ACE"),
    ]
);

// Decodes Fujifilm DynamicRange (tag 0x1400) to human-readable string.
// Per ExifTool's FujiFilm.pm: 1 => 'Standard', 3 => 'Wide'.
const_decoder!(pub
    DECODE_DYNAMIC_RANGE, i32, [
        (1, "Standard"),
        (3, "Wide"),
    ]
);

// Decodes Fujifilm DynamicRangeSetting (tag 0x1402) to human-readable string.
// Per ExifTool's FujiFilm.pm PrintHex table.
const_decoder!(pub
    DECODE_DYNAMIC_RANGE_SETTING, i32, [
        (0x000, "Auto"),
        (0x001, "Manual"),
        (0x100, "Standard (100%)"),
        (0x200, "Wide1 (230%)"),
        (0x201, "Wide2 (400%)"),
        (0x8000, "Film Simulation"),
    ]
);

// Decodes Fujifilm shutter type to human-readable string
const_decoder!(pub
    DECODE_SHUTTER_TYPE, i32, [
        (0, "Mechanical"),
        (1, "Electronic"),
        (2, "Electronic (long shutter speed)"),
        (3, "Electronic Front Curtain"),
    ]
);

// Decodes Fujifilm picture mode (tag 0x1031) to human-readable string.
// Per ExifTool's FujiFilm.pm PrintHex table (values 0x0-0x1c, 0x30, 0x40,
// 0x100, 0x200, 0x300).
const_decoder!(pub
    DECODE_PICTURE_MODE, i32, [
        (0x0000, "Auto"),
        (0x0001, "Portrait"),
        (0x0002, "Landscape"),
        (0x0003, "Macro"),
        (0x0004, "Sports"),
        (0x0005, "Night Scene"),
        (0x0006, "Program AE"),
        (0x0007, "Natural Light"),
        (0x0008, "Anti-blur"),
        (0x0009, "Beach & Snow"),
        (0x000A, "Sunset"),
        (0x000B, "Museum"),
        (0x000C, "Party"),
        (0x000D, "Flower"),
        (0x000E, "Text"),
        (0x000F, "Natural Light & Flash"),
        (0x0010, "Beach"),
        (0x0011, "Snow"),
        (0x0012, "Fireworks"),
        (0x0013, "Underwater"),
        (0x0014, "Portrait with Skin Correction"),
        (0x0016, "Panorama"),
        (0x0017, "Night (tripod)"),
        (0x0018, "Pro Low-light"),
        (0x0019, "Pro Focus"),
        (0x001A, "Portrait 2"),
        (0x001B, "Dog Face Detection"),
        (0x001C, "Cat Face Detection"),
        (0x0030, "HDR"),
        (0x0040, "Advanced Filter"),
        (0x0100, "Aperture-priority AE"),
        (0x0200, "Shutter speed priority AE"),
        (0x0300, "Manual"),
    ]
);

// Decodes Fujifilm drive mode to human-readable string
const_decoder!(pub
    DECODE_DRIVE_MODE, i32, [
        (0, "Single Frame"),
        (1, "Continuous Low"),
        (2, "Continuous High"),
        (3, "Bracketing"),
        (4, "Self-timer"),
        (5, "Remote"),
        (6, "Interval Timer"),
    ]
);

// Decodes Fujifilm EXR mode to human-readable string
const_decoder!(pub
    DECODE_EXR_MODE, i32, [
        (256, "HR (High Resolution)"),
        (512, "SN (Signal to Noise priority)"),
        (768, "DR (Dynamic Range priority)"),
    ]
);

// Decodes boolean/off-on value to human-readable string
const_decoder!(pub
    DECODE_OFF_ON, i32, [
        (0, "Off"),
        (1, "On"),
    ]
);

// The three "warning" tags at 0x1300/0x1301/0x1302 are *not* one shared
// on/off flag. FujiFilm.pm gives each its own two-entry PrintConv and only
// BlurWarning uses the word "None" at all:
//
//     0x1300 BlurWarning     0 => 'None', 1 => 'Blur Warning'   (:688-695)
//     0x1301 FocusWarning    0 => 'Good', 1 => 'Out of focus'   (:696-702)
//     0x1302 ExposureWarning 0 => 'Good', 1 => 'Bad exposure'   (:703-709)
//
// oxidex used to print a single invented `None`/`Warning` pair for all three,
// which disagreed with ExifTool on 777 corpus files -- every file carrying
// 0x1301 or 0x1302 at all, plus the 60 that set BlurWarning.
const_decoder!(pub
    DECODE_BLUR_WARNING, i32, [
        (0, "None"),
        (1, "Blur Warning"),
    ]
);

const_decoder!(pub
    DECODE_FOCUS_WARNING, i32, [
        (0, "Good"),
        (1, "Out of focus"),
    ]
);

const_decoder!(pub
    DECODE_EXPOSURE_WARNING, i32, [
        (0, "Good"),
        (1, "Bad exposure"),
    ]
);

// EXRAuto (0x1033) is not an off/on flag either: FujiFilm.pm:617-624 spells
// it 0 => 'Auto', 1 => 'Manual'. Decoding it through DECODE_OFF_ON printed
// "Off"/"On" on all 7 EXR-capable corpus files.
const_decoder!(pub
    DECODE_EXR_AUTO, i32, [
        (0, "Auto"),
        (1, "Manual"),
    ]
);

// ===== NEW DECODERS =====

// Decodes AF mode
const_decoder!(pub
    DECODE_AF_MODE, i32, [
        (0, "No"),
        (1, "Single Point"),
        (256, "Zone"),
        (512, "Wide/Tracking"),
    ]
);

// Decodes noise reduction (tag 0x100b). Per ExifTool's FujiFilm.pm:
// 0x40 => 'Low', 0x80 => 'Normal', 0x100 => 'n/a'.
const_decoder!(pub
    DECODE_NOISE_REDUCTION, i32, [
        (0x40, "Low"),
        (0x80, "Normal"),
        (0x100, "n/a"),
    ]
);

// Decodes tag 0x100e, which ExifTool also names NoiseReduction (FujiFilm.pm
// declares both 0x100b and 0x100e with `Name => 'NoiseReduction'`). Values
// per ExifTool's PrintConv, keyed on the raw int16u.
const_decoder!(pub
    DECODE_NOISE_REDUCTION_0X100E, i32, [
        (0x000, "0 (normal)"),
        (0x100, "+2 (strong)"),
        (0x180, "+1 (medium strong)"),
        (0x1c0, "+3 (very strong)"),
        (0x1e0, "+4 (strongest)"),
        (0x200, "-2 (weak)"),
        (0x280, "-1 (medium weak)"),
        (0x2c0, "-3 (very weak)"),
        (0x2e0, "-4 (weakest)"),
    ]
);

// Decodes grain effect roughness / Color Chrome levels
const_decoder!(pub
    DECODE_EFFECT_STRENGTH, i32, [
        (0, "Off"),
        (32, "Weak"),
        (64, "Strong"),
    ]
);

// Decodes GrainEffectSize (tag 0x104c). FujiFilm.pm:527-531 -- a different
// value set from DECODE_EFFECT_STRENGTH above.
const_decoder!(pub
    DECODE_GRAIN_EFFECT_SIZE, i32, [
        (0, "Off"),
        (16, "Small"),
        (32, "Large"),
    ]
);

// Decodes ImageGeneration (tag 0x1436). FujiFilm.pm:824-831.
const_decoder!(pub
    DECODE_IMAGE_GENERATION, i32, [
        (0, "Original Image"),
        (1, "Re-developed from RAW"),
    ]
);

// Decodes crop mode
const_decoder!(pub
    DECODE_CROP_MODE, i32, [
        (0, "n/a"),
        (1, "Full-frame on GFX"),
        (2, "Sports Finder Mode"),
        (4, "Electronic Shutter 1.25x Crop"),
        (8, "Digital Tele-Conv"),
    ]
);

// Decodes auto bracketing (tag 0x1100). Per ExifTool's FujiFilm.pm
// (non-X-T3 models, which is the more common variant): 0 => 'Off',
// 1 => 'On', 2 => 'No flash & flash', 6 => 'Pixel Shift'.
const_decoder!(pub
    DECODE_AUTO_BRACKETING, i32, [
        (0, "Off"),
        (1, "On"),
        (2, "No flash & flash"),
        (6, "Pixel Shift"),
    ]
);

// Decodes panorama direction
const_decoder!(pub
    DECODE_PANORAMA_DIRECTION, i32, [
        (1, "Right"),
        (2, "Left"),
        (3, "Up"),
        (4, "Down"),
    ]
);

// Decodes AdvancedFilter (tag 0x1201). FujiFilm.pm:651-675 keys this on the
// *high* half of a 32-bit value (0x10000 .. 0x130002), not on 0..0x10; the
// dense 0-based table that used to live here shared not one id with
// ExifTool's, so every filtered frame printed a label ExifTool never emits.
const_decoder!(pub
    DECODE_ADVANCED_FILTER, i32, [
        (0x10000, "Pop Color"),
        (0x20000, "Hi Key"),
        (0x30000, "Toy Camera"),
        (0x40000, "Miniature"),
        (0x50000, "Dynamic Tone"),
        (0x60001, "Partial Color Red"),
        (0x60002, "Partial Color Yellow"),
        (0x60003, "Partial Color Green"),
        (0x60004, "Partial Color Blue"),
        (0x60005, "Partial Color Orange"),
        (0x60006, "Partial Color Purple"),
        (0x70000, "Soft Focus"),
        (0x90000, "Low Key"),
        (0x100000, "Light Leak"),
        (0x130000, "Expired Film Green"),
        (0x130001, "Expired Film Red"),
        (0x130002, "Expired Film Neutral"),
    ]
);

// Decodes color mode
const_decoder!(pub
    DECODE_COLOR_MODE, i32, [
        (0x00, "Standard"),
        (0x10, "Chrome"),
        (0x30, "B & W"),
    ]
);

// Decodes ImageStabilization (tag 0x1422), element 0 of the 3x int16u array:
// the IS system in use. FujiFilm.pm:790-800 (the first of the two hashrefs in
// the array PrintConv at line 794). There is no 256 key in ExifTool's table --
// that entry, and the other four labels here, were invented; the real map is
// 0/1/2/3/258/512 only.
const_decoder!(pub
    DECODE_IMAGE_STABILIZATION, i32, [
        (0, "None"),
        (1, "Optical"), //PH FujiFilm.pm:796
        (2, "Sensor-shift"), //PH FujiFilm.pm:797 (now IBIS/OIS, ref forum13708)
        (3, "OIS Lens"), //forum9815 FujiFilm.pm:798 (optical+sensor?)
        (258, "IBIS/OIS + DIS"), //forum13708 FujiFilm.pm:799 (digital on top of IBIS/OIS)
        (512, "Digital"), //PH FujiFilm.pm:800
    ]
);

// Decodes ImageStabilization (tag 0x1422), element 1 of the 3x int16u array:
// the IS mode. FujiFilm.pm:801-804 (the second hashref in the array
// PrintConv). Element 2 (a frame/lens-shake counter) has no PrintConv in
// ExifTool and is rendered as the raw int16u.
const_decoder!(pub
    DECODE_IMAGE_STABILIZATION_MODE, i32, [
        (0, "Off"),
        (1, "On (mode 1, continuous)"),
        (2, "On (mode 2, shooting only)"),
    ]
);

// Decodes scene recognition
const_decoder!(pub
    DECODE_SCENE_RECOGNITION, i32, [
        (0, "Unrecognized"),
        (0x100, "Portrait Image"),
        (0x103, "Night Portrait"),
        (0x105, "Backlit Portrait"),
        (0x200, "Landscape Image"),
        (0x300, "Night Scene"),
        (0x400, "Macro"),
    ]
);

// D-Range priority is three tags with three different tables, not one
// (FujiFilm.pm:795-819):
//
//     0x1443 DRangePriority      0 => 'Auto',  1 => 'Fixed'
//     0x1444 DRangePriorityAuto  1 => 'Weak',  2 => 'Strong', 3 => 'Plus'
//     0x1445 DRangePriorityFixed 1 => 'Weak',  2 => 'Strong'
//
// Decoding all three through one Auto/Weak/Strong map printed "Weak" where
// ExifTool prints "Fixed".
const_decoder!(pub
    DECODE_DRANGE_PRIORITY, i32, [
        (0, "Auto"),
        (1, "Fixed"),
    ]
);

const_decoder!(pub
    DECODE_DRANGE_PRIORITY_AUTO, i32, [
        (1, "Weak"),
        (2, "Strong"),
        (3, "Plus"),
    ]
);

const_decoder!(pub
    DECODE_DRANGE_PRIORITY_FIXED, i32, [
        (1, "Weak"),
        (2, "Strong"),
    ]
);

// Decodes video recording mode
const_decoder!(pub
    DECODE_VIDEO_RECORDING_MODE, i32, [
        (0x00, "Normal"),
        (0x10, "F-log"),
        (0x20, "HLG"),
        (0x30, "F-log2"),
    ]
);

// Decodes video compression
const_decoder!(pub
    DECODE_VIDEO_COMPRESSION, i32, [
        (1, "Log GOP"),
        (2, "All Intra"),
    ]
);

/// Represents a Fujifilm MakerNote parser
pub struct FujifilmParser;

impl MakerNoteParser for FujifilmParser {
    fn manufacturer_name(&self) -> &'static str {
        // ExifTool spells the group with a capital F on both halves:
        // MakerNotes.pm:121 `Name => 'MakerNoteFujiFilm'`, which is what the
        // family-1 group is named after, and `exiftool -a -G1 -s` prints
        // `[FujiFilm]` for every Fuji sample in the corpus.
        "FujiFilm"
    }

    fn tag_prefix(&self) -> &'static str {
        "FujiFilm:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Fujifilm MakerNotes start with "FUJIFILM" (8 bytes) followed by offset
        data.len() >= 12 && &data[0..8] == FUJIFILM_HEADER
    }

    fn parse(
        &self,
        data: &[u8],
        _byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        if data.is_empty() {
            return Ok(());
        }

        // Validate Fujifilm header
        if !self.validate_header(data) {
            return Err("Invalid Fujifilm MakerNote header".to_string());
        }

        // CRITICAL: Fujifilm MakerNotes ALWAYS use little-endian byte order,
        // regardless of the main EXIF byte order. This is a Fujifilm-specific
        // quirk that differs from most other camera manufacturers.
        let fuji_byte_order = ByteOrder::LittleEndian;

        // Fujifilm header structure:
        // - Bytes 0-7: "FUJIFILM" signature
        // - Bytes 8-11: IFD offset (4 bytes, little-endian, typically 0x0C = 12)
        // - Byte 12+: IFD data starts

        // Read IFD offset using little-endian byte order
        let reader = EndianReader::new(data, fuji_byte_order.to_io_byte_order());
        let ifd_offset = reader.u32_at(8).unwrap_or(0) as usize;

        // Fujifilm offsets are relative to the MakerNote start
        if ifd_offset >= data.len() {
            return Ok(());
        }

        let ifd_data = &data[ifd_offset..];

        // Parse IFD entry count using little-endian byte order
        if ifd_data.len() < 2 {
            return Ok(());
        }

        let ifd_reader = EndianReader::new(ifd_data, fuji_byte_order.to_io_byte_order());
        let entry_count = ifd_reader.u16_at(0).unwrap_or(0);

        // Parse IFD entries (always little-endian for Fujifilm)
        let entries_start = &ifd_data[2..];
        let entries = match parse_ifd_entries(entries_start, entry_count, fuji_byte_order) {
            Ok((_, entries)) => entries,
            Err(_) => return Ok(()), // Return empty on parse failure
        };

        // Extract tags from entries
        for entry in entries {
            // Binary sub-directories. `%FujiFilm::Main` gives these four tags a
            // `SubDirectory => { TagTable => ... }` with no Condition and no
            // Start/Base/ByteOrder override (FujiFilm.pm:341, :345, :349, :609),
            // so ExifTool descends into the record and reports its fields, and
            // reports nothing for the tag itself.
            if let Some(table) = fujifilm_binary_subdir(entry.tag_id) {
                if let Some(record) = entry_bytes(&entry, data) {
                    decode_binary_subdir(table, &record, fuji_byte_order, "FujiFilm", tags);
                }
                continue;
            }

            match entry.tag_id {
                // String tags
                FUJI_VERSION => {
                    if let Some(value) = extract_string_value(&entry, data) {
                        let tag_name = fujifilm_tag_to_name(entry.tag_id);
                        tags.insert(tag_name, value);
                    }
                }

                // InternalSerialNumber (tag 0x0010): a string with a
                // model-specific PrintConv that decodes an embedded
                // hex-encoded body number and manufacture date.
                FUJI_SERIAL_NUMBER => {
                    if let Some(raw) = extract_string_value_raw(&entry, data) {
                        tags.insert(
                            "FujiFilm:InternalSerialNumber".to_string(),
                            decode_internal_serial_number(&raw),
                        );
                    }
                }

                // Simple integer tags
                FUJI_SEQUENCE_NUMBER | FUJI_FRAME_NUMBER | FUJI_IMAGE_COUNT | FUJI_RATING
                | FUJI_EXPOSURE_COUNT => {
                    let value = entry.value_offset;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, value.to_string());
                }

                // Quality (tag 0x1000) is stored as a raw string (e.g.
                // "NORMAL "), not an enumerated int16u -- unlike most other
                // tags in this range, it has no numeric PrintConv table.
                FUJI_QUALITY => {
                    if let Some(value) = extract_string_value_raw(&entry, data) {
                        tags.insert("FujiFilm:Quality".to_string(), value);
                    }
                }

                FUJI_WHITE_BALANCE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:WhiteBalance".to_string(),
                        DECODE_WHITE_BALANCE.decode(value).to_string(),
                    );
                }

                FUJI_FOCUS_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:FocusMode".to_string(),
                        DECODE_FOCUS_MODE.decode(value).to_string(),
                    );
                }

                // ExifTool names this tag "FujiFlashMode", not "FlashMode".
                FUJI_FLASH_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:FujiFlashMode".to_string(),
                        DECODE_FLASH_MODE.decode(value).to_string(),
                    );
                }

                FUJI_FILM_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:FilmMode".to_string(),
                        DECODE_FILM_MODE.decode(value).to_string(),
                    );
                }

                // DynamicRange (0x1400) and DynamicRangeSetting (0x1402) are
                // distinct tags with distinct PrintConv tables; they were
                // previously conflated into a single "DynamicRange" tag.
                FUJI_DYNAMIC_RANGE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:DynamicRange".to_string(),
                        DECODE_DYNAMIC_RANGE.decode(value).to_string(),
                    );
                }

                FUJI_DYNAMIC_RANGE_SETTING => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:DynamicRangeSetting".to_string(),
                        DECODE_DYNAMIC_RANGE_SETTING.decode(value).to_string(),
                    );
                }

                FUJI_DEVELOPMENT_DYNAMIC_RANGE => {
                    let value = entry.value_offset;
                    tags.insert(
                        "FujiFilm:DevelopmentDynamicRange".to_string(),
                        value.to_string(),
                    );
                }

                FUJI_AUTO_BRACKETING => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:AutoBracketing".to_string(),
                        DECODE_AUTO_BRACKETING.decode(value).to_string(),
                    );
                }

                FUJI_PICTURE_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:PictureMode".to_string(),
                        DECODE_PICTURE_MODE.decode(value).to_string(),
                    );
                }

                FUJI_DRIVE_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:DriveMode".to_string(),
                        DECODE_DRIVE_MODE.decode(value).to_string(),
                    );
                }

                FUJI_EXR_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:EXRMode".to_string(),
                        DECODE_EXR_MODE.decode(value).to_string(),
                    );
                }

                // Sharpness/Saturation/Contrast each use their own PrintHex
                // table (not a simple linear scale).
                FUJI_SHARPNESS => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:Sharpness".to_string(),
                        DECODE_SHARPNESS.decode(value).to_string(),
                    );
                }

                FUJI_SATURATION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:Saturation".to_string(),
                        DECODE_SATURATION.decode(value).to_string(),
                    );
                }

                FUJI_CONTRAST => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:Contrast".to_string(),
                        DECODE_CONTRAST.decode(value).to_string(),
                    );
                }

                // ShadowTone/HighlightTone (tags 0x1040/0x1041):
                // FujiFilm.pm:439-480 -- a hash PrintConv with named
                // breakpoints at every multiple of 16 cameras actually write,
                // plus an `OTHER` fallback (`-$val/16`) for anything else.
                // This printed the bare signed raw value (e.g. "+0") instead
                // of ExifTool's named strings (e.g. "0 (normal)") -- verified
                // wrong against FujiFilmGFX100II.jpg.
                FUJI_SHADOW_TONE | FUJI_HIGHLIGHT_TONE => {
                    let value = entry.value_offset as i32;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, decode_fuji_tone(value));
                }

                FUJI_COLOR_TEMPERATURE => {
                    let value = entry.value_offset;
                    if value > 0 {
                        tags.insert(
                            "FujiFilm:ColorTemperature".to_string(),
                            format!("{} K", value),
                        );
                    }
                }

                FUJI_FACES_DETECTED => {
                    let value = entry.value_offset;
                    tags.insert("FujiFilm:FacesDetected".to_string(), value.to_string());
                }

                // Boolean/On-Off tags
                FUJI_MACRO | FUJI_SLOW_SYNC => {
                    let value = entry.value_offset as i32;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, DECODE_OFF_ON.decode(value).to_string());
                }

                // AutoDynamicRange (tag 0x140b): FujiFilm.pm:785-790 --
                // `PrintConv => '"$val%"'`, not an Off/On enum (it was
                // previously grouped with FUJI_MACRO/FUJI_SLOW_SYNC above and
                // decoded through DECODE_OFF_ON, which is a different tag's
                // conversion entirely -- verified wrong against
                // FujiFilmFinePixF300EXR.jpg, where exiftool prints
                // `AutoDynamicRange : 200%` and oxidex printed `Unknown
                // (200)`).
                FUJI_AUTO_DYNAMIC_RANGE => {
                    let value = entry.value_offset as i32;
                    tags.insert("FujiFilm:AutoDynamicRange".to_string(), format!("{value}%"));
                }

                FUJI_EXR_AUTO => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:EXRAuto".to_string(),
                        DECODE_EXR_AUTO.decode(value).to_string(),
                    );
                }

                // 0x1050 is ShutterType (FujiFilm.pm:553-562, `#forum6109`),
                // an int16u with a four-entry PrintConv. oxidex used to read
                // it as a string and print it under the name
                // "LensModelName", which appears in no ExifTool source file;
                // on the 43 corpus files that carry the tag ExifTool prints
                // `FujiFilm:ShutterType`.
                FUJI_SHUTTER_TYPE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:ShutterType".to_string(),
                        DECODE_SHUTTER_TYPE.decode(value).to_string(),
                    );
                }

                // Warning flags -- one table each, see the decoders above.
                FUJI_BLUR_WARNING => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:BlurWarning".to_string(),
                        DECODE_BLUR_WARNING.decode(value).to_string(),
                    );
                }

                FUJI_FOCUS_WARNING => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:FocusWarning".to_string(),
                        DECODE_FOCUS_WARNING.decode(value).to_string(),
                    );
                }

                FUJI_EXPOSURE_WARNING => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:ExposureWarning".to_string(),
                        DECODE_EXPOSURE_WARNING.decode(value).to_string(),
                    );
                }

                // Lens focal length information: stored as rational64s (8
                // bytes, read via the value offset), with no unit suffix in
                // ExifTool's output (e.g. "70", not "70.0 mm").
                FUJI_MIN_FOCAL_LENGTH | FUJI_MAX_FOCAL_LENGTH => {
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    if let Some(rationals) = extract_rational_array(&entry, data, fuji_byte_order)
                        && let Some(&(num, denom)) = rationals.first()
                    {
                        tags.insert(
                            tag_name,
                            format_rational_as_decimal(num as i32 as i64, denom as i32 as i64),
                        );
                    }
                }

                // Max aperture at min/max focal length: also rational64s,
                // with no unit suffix (e.g. "2.8", not "f/2.8").
                FUJI_MAX_APERTURE_AT_MIN_FOCAL | FUJI_MAX_APERTURE_AT_MAX_FOCAL => {
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    if let Some(rationals) = extract_rational_array(&entry, data, fuji_byte_order)
                        && let Some(&(num, denom)) = rationals.first()
                    {
                        tags.insert(
                            tag_name,
                            format_rational_as_decimal(num as i32 as i64, denom as i32 as i64),
                        );
                    }
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
                    tags.insert("FujiFilm:DigitalZoom".to_string(), format!("{:.2}x", value));
                }

                // Flash exposure compensation: rational64s (8 bytes, read
                // via the value offset), printed as a plain decimal with no
                // sign or unit suffix (e.g. "0", "-0.7"), matching ExifTool.
                FUJI_FLASH_EV => {
                    if let Some(rationals) = extract_rational_array(&entry, data, fuji_byte_order)
                        && let Some(&(num, denom)) = rationals.first()
                    {
                        tags.insert(
                            "FujiFilm:FlashExposureComp".to_string(),
                            format_rational_as_decimal(num as i32 as i64, denom as i32 as i64),
                        );
                    }
                }

                // Focus pixel coordinates (array)
                // FocusPixel (tag 0x1023): FujiFilm.pm:353-357 -- `Count =>
                // 2`, no PrintConv, so ExifTool's default array rendering is
                // the two numbers space-joined ("2597 1159"). 2 * int16u is
                // exactly 4 bytes, so this is always inline in the entry's
                // own `value_offset` field, never out-of-line --
                // `extract_u16_array` (== `extract_array::<u16>`)
                // unconditionally treats `value_offset` as an offset *into*
                // `data`, which is wrong for an inline value and silently
                // drops the tag (verified: absent from oxidex's output for
                // FujiFilmA100.jpg, where exiftool prints `FocusPixel : 1824
                // 1368`). Read the two halves directly instead, the same way
                // Casio's `extract_u16_value` does for its own inline pairs.
                FUJI_FOCUS_PIXEL => {
                    let bytes = match fuji_byte_order {
                        ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
                        ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
                    };
                    let reader = EndianReader::new(&bytes, fuji_byte_order.to_io_byte_order());
                    if let (Some(x), Some(y)) = (reader.u16_at(0), reader.u16_at(2)) {
                        tags.insert("FujiFilm:FocusPixel".to_string(), format!("{x} {y}"));
                    }
                }

                // FacePositions (tag 0x4103): FujiFilm.pm:941-953 -- `Count
                // => -1`, no PrintConv. ExifTool's default array rendering is
                // every left/top/right/bottom coordinate (across however many
                // faces) space-joined, e.g. "643 482 1393 1232" for one face.
                // Verified against combined-samples/FujiFilm's samples with
                // face detection active.
                FUJI_FACE_POSITIONS => {
                    if let Some(array) = extract_u16_array(&entry, data, fuji_byte_order)
                        && !array.is_empty()
                    {
                        let joined = array
                            .iter()
                            .map(u16::to_string)
                            .collect::<Vec<_>>()
                            .join(" ");
                        tags.insert("FujiFilm:FacePositions".to_string(), joined);
                    }
                }

                // ===== NEW TAG HANDLING =====

                // AF Mode
                FUJI_AF_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:AFMode".to_string(),
                        DECODE_AF_MODE.decode(value).to_string(),
                    );
                }

                // Noise reduction tags
                FUJI_NOISE_REDUCTION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:NoiseReduction".to_string(),
                        DECODE_NOISE_REDUCTION.decode(value).to_string(),
                    );
                }

                // ExifTool names 0x100e NoiseReduction too (a second,
                // X100-era tag); it is NOT "HighISONoiseReduction", which
                // FujiFilm.pm never defines.
                FUJI_HIGH_ISO_NOISE_REDUCTION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:NoiseReduction".to_string(),
                        DECODE_NOISE_REDUCTION_0X100E.decode(value).to_string(),
                    );
                }

                // White balance fine tune: int32s[2] (Red, Blue), stored via
                // the value offset since 2*4=8 bytes exceeds the 4-byte
                // inline threshold.
                FUJI_WHITE_BALANCE_FINE_TUNE => {
                    if let Some(values) = extract_i32_array(&entry, data, fuji_byte_order)
                        && values.len() >= 2
                    {
                        tags.insert(
                            "FujiFilm:WhiteBalanceFineTune".to_string(),
                            format!("Red {:+}, Blue {:+}", values[0], values[1]),
                        );
                    }
                }

                // Lens Modulation Optimizer
                FUJI_LENS_MODULATION_OPTIMIZER => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:LensModulationOptimizer".to_string(),
                        DECODE_OFF_ON.decode(value).to_string(),
                    );
                }

                // Grain Effect Roughness
                FUJI_GRAIN_EFFECT_ROUGHNESS => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:GrainEffectRoughness".to_string(),
                        DECODE_EFFECT_STRENGTH.decode(value).to_string(),
                    );
                }

                // Grain Effect Size (tag 0x104c): FujiFilm.pm:524-532.
                FUJI_GRAIN_EFFECT_SIZE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:GrainEffectSize".to_string(),
                        DECODE_GRAIN_EFFECT_SIZE.decode(value).to_string(),
                    );
                }

                // Image Generation (tag 0x1436): FujiFilm.pm:824-831.
                FUJI_IMAGE_GENERATION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:ImageGeneration".to_string(),
                        DECODE_IMAGE_GENERATION.decode(value).to_string(),
                    );
                }

                // FujiModel/FujiModel2 (tags 0x1447/0x1448): FujiFilm.pm:871-872
                // -- plain strings, no PrintConv. ExifTool's ReadValue only
                // truncates a `string[n]` at the first NUL (never trims
                // whitespace), so this uses the raw extractor the same way
                // FUJI_QUALITY does above.
                FUJI_FUJI_MODEL | FUJI_FUJI_MODEL2 => {
                    if let Some(value) = extract_string_value_raw(&entry, data) {
                        let tag_name = fujifilm_tag_to_name(entry.tag_id);
                        tags.insert(tag_name, value);
                    }
                }

                // WBRed/WBGreen/WBBlue (tags 0x144a/0x144b/0x144c):
                // FujiFilm.pm:876-878 -- plain int16u, no PrintConv.
                FUJI_WB_RED | FUJI_WB_GREEN | FUJI_WB_BLUE => {
                    let value = entry.value_offset as i32;
                    let tag_name = fujifilm_tag_to_name(entry.tag_id);
                    tags.insert(tag_name, value.to_string());
                }

                // FileSource (tag 0x8000): FujiFilm.pm:1003-1006 -- a plain
                // string (e.g. "135_C", "APS_H"), unrelated to the
                // standard-EXIF FileSource byte tag ExifTool prints under
                // group ExifIFD. Verified against
                // combined-samples/FujiFilm/FujiFilmSP-2000.jpg: exiftool
                // -G1 -s -a reports `[FujiFilm] FileSource : 135_C`.
                FUJI_FILE_SOURCE => {
                    if let Some(value) = extract_string_value_raw(&entry, data) {
                        tags.insert("FujiFilm:FileSource".to_string(), value);
                    }
                }

                // OrderNumber (tag 0x8002): FujiFilm.pm:1007-1010 -- plain
                // int32u, no PrintConv.
                FUJI_ORDER_NUMBER => {
                    tags.insert(
                        "FujiFilm:OrderNumber".to_string(),
                        entry.value_offset.to_string(),
                    );
                }

                // Color Chrome Effect
                FUJI_COLOR_CHROME_EFFECT => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:ColorChromeEffect".to_string(),
                        DECODE_EFFECT_STRENGTH.decode(value).to_string(),
                    );
                }

                // B&W Adjustment
                FUJI_BW_ADJUSTMENT => {
                    let value = entry.value_offset as i32;
                    tags.insert("FujiFilm:BWAdjustment".to_string(), format!("{:+}", value));
                }

                // Crop Mode
                FUJI_CROP_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:CropMode".to_string(),
                        DECODE_CROP_MODE.decode(value).to_string(),
                    );
                }

                // Color Chrome FX Blue
                FUJI_COLOR_CHROME_FX_BLUE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:ColorChromeFXBlue".to_string(),
                        DECODE_EFFECT_STRENGTH.decode(value).to_string(),
                    );
                }

                // Pixel Shift
                FUJI_PIXEL_SHIFT_SHOTS => {
                    let value = entry.value_offset;
                    tags.insert("FujiFilm:PixelShiftShots".to_string(), value.to_string());
                }

                FUJI_PIXEL_SHIFT_OFFSET_NEW => {
                    if let Some(array) = extract_u16_array(&entry, data, fuji_byte_order)
                        && array.len() >= 2
                    {
                        tags.insert(
                            "FujiFilm:PixelShiftOffset".to_string(),
                            format!("X:{} Y:{}", array[0], array[1]),
                        );
                    }
                }

                // Panorama tags
                //
                // FujiFilm.pm:637-640 declares 0x1153 as a bare
                // `{ Name => 'PanoramaAngle', Writable => 'int16u' }` -- no
                // PrintConv, so ExifTool prints the number and nothing else
                // (`[FujiFilm] PanoramaAngle : 360` on
                // FujiFilmFinePixS9200S9250S9150.jpg). The " deg" suffix was
                // oxidex's own invention.
                FUJI_PANORAMA_ANGLE => {
                    let value = entry.value_offset;
                    tags.insert("FujiFilm:PanoramaAngle".to_string(), value.to_string());
                }

                FUJI_PANORAMA_DIRECTION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:PanoramaDirection".to_string(),
                        DECODE_PANORAMA_DIRECTION.decode(value).to_string(),
                    );
                }

                // Advanced Filter
                FUJI_ADVANCED_FILTER => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:AdvancedFilter".to_string(),
                        DECODE_ADVANCED_FILTER.decode(value).to_string(),
                    );
                }

                // Color Mode
                FUJI_COLOR_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:ColorMode".to_string(),
                        DECODE_COLOR_MODE.decode(value).to_string(),
                    );
                }

                // Image Stabilization (tag 0x1422). FujiFilm.pm:790-806: a 3x
                // int16u array (Count => 3). 3 * 2 = 6 bytes never fits in the
                // 4-byte inline value_offset field, so value_offset is always
                // a pointer into `data` -- reading it directly as a scalar
                // (the old code) decoded the file offset as if it were the
                // tag value. Element 0 and element 1 each have their own
                // PrintConv hash (array PrintConv); element 2 has none and
                // prints as the raw number, joined with "; " to match
                // ExifTool's list rendering.
                FUJI_IMAGE_STABILIZATION => {
                    if let Some(array) = extract_u16_array(&entry, data, fuji_byte_order)
                        && array.len() >= 3
                    {
                        let parts = [
                            DECODE_IMAGE_STABILIZATION.decode(array[0] as i32),
                            DECODE_IMAGE_STABILIZATION_MODE.decode(array[1] as i32),
                            array[2].to_string(),
                        ];
                        tags.insert("FujiFilm:ImageStabilization".to_string(), parts.join("; "));
                    }
                }

                // Scene Recognition
                FUJI_SCENE_RECOGNITION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:SceneRecognition".to_string(),
                        DECODE_SCENE_RECOGNITION.decode(value).to_string(),
                    );
                }

                // D-Range Priority tags
                FUJI_DRANGE_PRIORITY => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:DRangePriority".to_string(),
                        DECODE_DRANGE_PRIORITY.decode(value).to_string(),
                    );
                }

                FUJI_DRANGE_PRIORITY_AUTO => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:DRangePriorityAuto".to_string(),
                        DECODE_DRANGE_PRIORITY_AUTO.decode(value).to_string(),
                    );
                }

                FUJI_DRANGE_PRIORITY_FIXED => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:DRangePriorityFixed".to_string(),
                        DECODE_DRANGE_PRIORITY_FIXED.decode(value).to_string(),
                    );
                }

                // Video tags
                FUJI_VIDEO_RECORDING_MODE => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:VideoRecordingMode".to_string(),
                        DECODE_VIDEO_RECORDING_MODE.decode(value).to_string(),
                    );
                }

                FUJI_PERIPHERAL_LIGHTING => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:PeripheralLighting".to_string(),
                        DECODE_OFF_ON.decode(value).to_string(),
                    );
                }

                FUJI_VIDEO_COMPRESSION => {
                    let value = entry.value_offset as i32;
                    tags.insert(
                        "FujiFilm:VideoCompression".to_string(),
                        DECODE_VIDEO_COMPRESSION.decode(value).to_string(),
                    );
                }

                FUJI_FRAME_RATE => {
                    let value = entry.value_offset as f32 / 1000.0;
                    tags.insert(
                        "FujiFilm:FrameRate".to_string(),
                        format!("{:.3} fps", value),
                    );
                }

                FUJI_FRAME_WIDTH => {
                    let value = entry.value_offset;
                    tags.insert("FujiFilm:FrameWidth".to_string(), format!("{} px", value));
                }

                FUJI_FRAME_HEIGHT => {
                    let value = entry.value_offset;
                    tags.insert("FujiFilm:FrameHeight".to_string(), format!("{} px", value));
                }

                // FaceElementSelected (tag 0x4005): FujiFilm.pm:931-935 --
                // `Count => 4`, int16u, no PrintConv. 4 * 2 = 8 bytes, always
                // out-of-line (never fits the entry's own 4-byte field), so
                // `extract_u16_array`'s offset-only read is correct here
                // (unlike FocusPixel above, which is inline). Verified
                // against FujiFilmA220A230.jpg: exiftool prints
                // "1633 1125 2516 2012".
                FUJI_FACE_ELEMENT_SELECTED => {
                    if let Some(array) = extract_u16_array(&entry, data, fuji_byte_order)
                        && !array.is_empty()
                    {
                        let joined = array
                            .iter()
                            .map(u16::to_string)
                            .collect::<Vec<_>>()
                            .join(" ");
                        tags.insert("FujiFilm:FaceElementSelected".to_string(), joined);
                    }
                }

                FUJI_NUM_FACE_ELEMENTS => {
                    let value = entry.value_offset;
                    tags.insert("FujiFilm:NumFaceElements".to_string(), value.to_string());
                }

                // FaceElementTypes (tag 0x4201): FujiFilm.pm:954-981 --
                // `Writable => 'int8u'`, but this is a genuine per-entry TIFF
                // IFD (not a ProcessBinaryData record), so what actually gets
                // read is whatever type+count the file's own IFD entry
                // declares -- verified on FujiFilmFinePixZ900EXR.jpg, whose
                // entry is `int16u[1]` (field_type SHORT), not `int8u`.
                // `extract_uint_array` below reads either width; each value
                // (regardless of width) is looked up in the same PrintConv
                // map, joined by ", " ('REPEAT' PrintConv over an array).
                FUJI_FACE_ELEMENT_TYPES => {
                    let width = match entry.field_type {
                        1 => Some(1usize), // BYTE
                        3 => Some(2usize), // SHORT
                        _ => None,
                    };
                    if let Some(width) = width
                        && let Some(values) =
                            extract_uint_array(&entry, data, fuji_byte_order, width)
                        && !values.is_empty()
                    {
                        let types: Vec<String> = values
                            .iter()
                            .map(|&v| decode_face_element_type(v))
                            .collect();
                        tags.insert("FujiFilm:FaceElementTypes".to_string(), types.join(", "));
                    }
                }

                // FaceElementPositions (tag 0x4203): FujiFilm.pm:988-994 --
                // same shape as FacePositions above (`Count => -1`, no
                // PrintConv, space-joined coordinates).
                FUJI_FACE_ELEMENT_POSITIONS => {
                    if let Some(array) = extract_u16_array(&entry, data, fuji_byte_order)
                        && !array.is_empty()
                    {
                        let joined = array
                            .iter()
                            .map(u16::to_string)
                            .collect::<Vec<_>>()
                            .join(" ");
                        tags.insert("FujiFilm:FaceElementPositions".to_string(), joined);
                    }
                }

                // Other tags - skip unknown tags
                _ => continue,
            }
        }

        Ok(())
    }
}

/// Maps Fujifilm MakerNote tag IDs to human-readable tag names
fn fujifilm_tag_to_name(tag_id: u16) -> String {
    let tag_name = match tag_id {
        FUJI_VERSION => "Version",
        FUJI_SERIAL_NUMBER => "InternalSerialNumber",
        FUJI_QUALITY => "Quality",
        FUJI_SHARPNESS => "Sharpness",
        FUJI_WHITE_BALANCE => "WhiteBalance",
        FUJI_SATURATION => "Saturation",
        FUJI_CONTRAST => "Contrast",
        FUJI_COLOR_TEMPERATURE => "ColorTemperature",
        FUJI_FLASH_MODE => "FujiFlashMode",
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
        FUJI_SHUTTER_TYPE => "ShutterType",
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
        FUJI_AUTO_BRACKETING => "AutoBracketing",
        FUJI_SEQUENCE_NUMBER => "SequenceNumber",
        FUJI_EXPOSURE_COUNT => "ExposureCount",
        FUJI_BLUR_WARNING => "BlurWarning",
        FUJI_FOCUS_WARNING => "FocusWarning",
        FUJI_EXPOSURE_WARNING => "ExposureWarning",
        FUJI_RAW_IMAGE_FULL_WIDTH => "RawImageFullWidth",
        FUJI_RAW_IMAGE_FULL_HEIGHT => "RawImageFullHeight",
        FUJI_FRAME_NUMBER => "FrameNumber",
        FUJI_IMAGE_COUNT => "ImageCount",
        FUJI_DRIVE_MODE => "DriveMode",
        FUJI_RATING => "Rating",
        FUJI_FUJI_MODEL => "FujiModel",
        FUJI_FUJI_MODEL2 => "FujiModel2",
        FUJI_WB_RED => "WBRed",
        FUJI_WB_GREEN => "WBGreen",
        FUJI_WB_BLUE => "WBBlue",
        _ => return format!("FujiFilm:Unknown-{:#06X}", tag_id),
    };

    format!("FujiFilm:{}", tag_name)
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

/// Extracts a string value from an IFD entry without trimming internal or
/// trailing whitespace (only null terminators are stripped).
///
/// Some Fujifilm string tags (e.g. Quality, stored as `"NORMAL \0"`) include
/// a meaningful trailing space that ExifTool preserves in its output;
/// [`extract_string_value`] would incorrectly strip it via `.trim()`.
fn extract_string_value_raw(entry: &IfdEntry, full_data: &[u8]) -> Option<String> {
    let byte_count = entry.value_count as usize;

    if byte_count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        let s = std::str::from_utf8(&bytes[0..byte_count])
            .ok()?
            .trim_end_matches('\0');
        return Some(s.to_string());
    }

    let offset = entry.value_offset as usize;

    if offset + byte_count <= full_data.len() {
        let bytes = &full_data[offset..offset + byte_count];
        let s = std::str::from_utf8(bytes).ok()?.trim_end_matches('\0');
        return Some(s.to_string());
    }

    None
}

/// Reads `count` (`entry.value_count`) unsigned integers of `width` bytes (1
/// or 2) from an IFD entry, handling both the inline case (the whole array
/// fits in the entry's own 4-byte `value_offset` field) and the out-of-line
/// case, widened to `u32`.
///
/// The generic `extract_array`/`extract_u16_array` this file otherwise uses
/// only handles the out-of-line case -- correct for an array that is always
/// larger than 4 bytes (`FacePositions`, `Count => -1`), wrong for one that
/// can be small enough to be inline (`FaceElementTypes` at `Count => 1`,
/// verified against `FujiFilmFinePixZ900EXR.jpg`).
fn extract_uint_array(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
    width: usize,
) -> Option<Vec<u32>> {
    let count = entry.value_count as usize;
    if count == 0 || width == 0 {
        return None;
    }
    let total = count.checked_mul(width)?;
    let src: std::borrow::Cow<'_, [u8]> = if total <= 4 {
        let bytes = match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };
        std::borrow::Cow::Owned(bytes[..total].to_vec())
    } else {
        let offset = entry.value_offset as usize;
        std::borrow::Cow::Borrowed(data.get(offset..offset.checked_add(total)?)?)
    };
    let reader = EndianReader::new(&src, byte_order.to_io_byte_order());
    (0..count)
        .map(|i| match width {
            1 => src.get(i).map(|&b| u32::from(b)),
            2 => reader.u16_at(i * 2).map(u32::from),
            _ => None,
        })
        .collect()
}

/// FujiFilm.pm:439-480 (`ShadowTone`/`HighlightTone`'s shared `PrintConv`
/// shape -- two separate hashes with identical keys/values). The named
/// breakpoints take priority; `OTHER` (`-$val/16`) covers anything else a
/// camera might write outside them.
fn decode_fuji_tone(value: i32) -> String {
    match value {
        -64 => "+4 (hardest)".to_string(),
        -48 => "+3 (very hard)".to_string(),
        -32 => "+2 (hard)".to_string(),
        -16 => "+1 (medium hard)".to_string(),
        0 => "0 (normal)".to_string(),
        16 => "-1 (medium soft)".to_string(),
        32 => "-2 (soft)".to_string(),
        other => perl_number(f64::from(-other) / 16.0),
    }
}

/// FujiFilm.pm:954-981 (`FaceElementTypes`'s `PrintConv`, `'REPEAT'`'d over
/// the array). An unlisted value prints ExifTool's default `Unknown (n)`
/// (`ExifTool.pm:3633`), the same fallback `exiftool_tables::PrintConv`
/// documents.
fn decode_face_element_type(value: u32) -> String {
    match value {
        1 => "Face".to_string(),
        2 => "Left Eye".to_string(),
        3 => "Right Eye".to_string(),
        7 => "Body".to_string(),
        8 => "Head".to_string(),
        9 => "Both Eyes".to_string(),
        11 => "Bike".to_string(),
        12 => "Body of Car".to_string(),
        13 => "Front of Car".to_string(),
        14 => "Animal Body".to_string(),
        15 => "Animal Head".to_string(),
        16 => "Animal Face".to_string(),
        17 => "Animal Left Eye".to_string(),
        18 => "Animal Right Eye".to_string(),
        19 => "Bird Body".to_string(),
        20 => "Bird Head".to_string(),
        21 => "Bird Left Eye".to_string(),
        22 => "Bird Right Eye".to_string(),
        23 => "Aircraft Body".to_string(),
        25 => "Aircraft Cockpit".to_string(),
        26 => "Train Front".to_string(),
        27 => "Train Cockpit".to_string(),
        28 => "Animal Head (28)".to_string(),
        29 => "Animal Body (29)".to_string(),
        other => format!("Unknown ({other})"),
    }
}

/// Decodes Fujifilm's InternalSerialNumber (tag 0x0010) using the same
/// heuristic as ExifTool's FujiFilm.pm PrintConv.
///
/// The raw string ends with a hex-encoded camera body number followed by a
/// 6-digit manufacture date (`yymmdd`) and a fixed 12-character trailer. For
/// example, the raw string `"FPX20582698 592D313134360702198C0020100A84"`
/// decodes to `"FPX20582698 Y-1146 2007:02:19 8C0020100A84"`.
///
/// Falls back to the (already-trimmed) raw string unchanged if it doesn't
/// match the expected shape (e.g. some models use a slightly different
/// layout that ExifTool handles via a separate substitution, which is not
/// replicated here).
fn decode_internal_serial_number(raw: &str) -> String {
    let trimmed = raw.trim_end_matches(['\0', ' ', '\t', '\r', '\n']);
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() < 18 {
        return trimmed.to_string();
    }

    let split_at = chars.len() - 18;
    let prefix_chars = &chars[..split_at];
    let suffix: String = chars[split_at..].iter().collect();

    let yy = &suffix[0..2];
    let mm = &suffix[2..4];
    let dd = &suffix[4..6];
    let rest12 = &suffix[6..18];

    let (Some(_yy_num), Some(mm_num), Some(dd_num)) = (
        yy.parse::<u32>().ok(),
        mm.parse::<u32>().ok(),
        dd.parse::<u32>().ok(),
    ) else {
        return trimmed.to_string();
    };
    if !(1..=12).contains(&mm_num) || !(1..=31).contains(&dd_num) {
        return trimmed.to_string();
    }
    let yy_num: u32 = yy.parse().unwrap_or(0);

    // group2: the maximal suffix of the prefix consisting only of hex digits
    // (mirrors the greedy `[0-9a-fA-F]*` capture in ExifTool's regex, given
    // the lazy prefix capture ahead of it).
    let mut hex_start = prefix_chars.len();
    while hex_start > 0 && prefix_chars[hex_start - 1].is_ascii_hexdigit() {
        hex_start -= 1;
    }
    let group1: String = prefix_chars[..hex_start].iter().collect();
    let hex_run: Vec<char> = prefix_chars[hex_start..].to_vec();

    // pack('H*', ...): decode pairs of hex digits into bytes. A trailing
    // lone hex digit is treated as a high nibble with an implicit zero low
    // nibble, matching Perl's pack behavior for odd-length hex strings.
    let mut decoded_bytes = Vec::with_capacity(hex_run.len().div_ceil(2));
    let mut i = 0;
    while i < hex_run.len() {
        let hi = hex_run[i].to_digit(16).unwrap_or(0);
        let lo = if i + 1 < hex_run.len() {
            hex_run[i + 1].to_digit(16).unwrap_or(0)
        } else {
            0
        };
        decoded_bytes.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    let sn: String = decoded_bytes
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            }
        })
        .collect();

    let year = if yy_num < 70 {
        yy_num + 2000
    } else {
        yy_num + 1900
    };

    format!("{}{} {}:{}:{} {}", group1, sn, year, mm, dd, rest12)
}

/// Public function to parse Fujifilm MakerNotes
///
/// This is the main entry point for parsing Fujifilm MakerNote data.
///
/// # Parameters
/// - `data`: Raw MakerNote data (including Fujifilm header)
/// - `byte_order`: Byte order for parsing multi-byte values
/// - `tags`: HashMap to populate with extracted tags
/// The `%FujiFilm::Main` tags whose ExifTool entry is a `SubDirectory` over a
/// `ProcessBinaryData` table, and the table each one selects.
///
/// All four are packed settings words: one `int16u` or `int32u` holding several
/// nibble-wide fields that ExifTool splits with `Mask`. Reading the word as a
/// value would report a single meaningless number; not reading it at all, which
/// is what happened before, reports nothing.
const fn fujifilm_binary_subdir(tag_id: u16) -> Option<&'static BinaryTable> {
    match tag_id {
        FUJI_PRIORITY_SETTINGS => Some(&FUJIFILM_PRIORITYSETTINGS),
        FUJI_FOCUS_SETTINGS => Some(&FUJIFILM_FOCUSSETTINGS),
        FUJI_AFC_SETTINGS => Some(&FUJIFILM_AFCSETTINGS),
        FUJI_DRIVE_SETTINGS => Some(&FUJIFILM_DRIVESETTINGS),
        _ => None,
    }
}

/// The raw bytes of an entry's value.
///
/// Same offset convention as [`extract_string_value`]: up to four bytes live in
/// the entry's own value field, in the MakerNote's (always little-endian) byte
/// order, and anything longer is at an offset measured from the MakerNote start.
/// All four settings tags are a single `int16u`/`int32u` and so always inline,
/// but a record read from an offset is the general shape of a sub-directory.
fn entry_bytes(entry: &IfdEntry, full_data: &[u8]) -> Option<Vec<u8>> {
    let byte_count = (entry.value_count as usize)
        .checked_mul(ifd_type_size(entry.field_type))
        .filter(|n| *n > 0)?;
    if byte_count <= 4 {
        return Some(entry.value_offset.to_le_bytes()[..byte_count].to_vec());
    }
    let offset = entry.value_offset as usize;
    full_data
        .get(offset..offset.checked_add(byte_count)?)
        .map(<[u8]>::to_vec)
}

/// Bytes per element of a TIFF field type, or 0 for one this reader does not
/// know -- the caller then produces nothing rather than a mis-sized record.
const fn ifd_type_size(field_type: u16) -> usize {
    match field_type {
        1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,         // SHORT, SSHORT
        4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
        _ => 0,
    }
}

pub fn parse_fujifilm_makernotes(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = FujifilmParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("FujiFilm MakerNotes parse error: {}", e);
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

/// Staleness/consistency test (tag-machinery overhaul Step 16): registers the
/// Stage 1 Step 2 fact named in `OVERHAUL_PROGRESS.md` -- `ImageStabilization`
/// (tag 0x1422)'s two element hashes -- against `dump_tables.pl`'s output for
/// the pinned ExifTool tree.
///
/// Calls the real production decoders, `DECODE_IMAGE_STABILIZATION` and
/// `DECODE_IMAGE_STABILIZATION_MODE`, for every key ExifTool's current
/// `FujiFilm.pm:790-804` declares.
///
/// Fixture: `tools/exiftool-tables/fixtures/fujifilm_image_stabilization.json`.
#[cfg(test)]
mod staleness_tests {
    use super::*;
    use std::collections::BTreeMap;

    const FIXTURE: &str = include_str!(
        "../../../../tools/exiftool-tables/fixtures/fujifilm_image_stabilization.json"
    );

    #[derive(serde::Deserialize)]
    struct Fixture {
        element0: BTreeMap<String, String>,
        element1: BTreeMap<String, String>,
    }

    #[test]
    fn image_stabilization_matches_fujifilm_pm() {
        let f: Fixture =
            serde_json::from_str(FIXTURE).expect("fujifilm_image_stabilization.json is valid JSON");
        assert_eq!(
            f.element0.len(),
            6,
            "ImageStabilization element-0 map size changed"
        );
        assert_eq!(
            f.element1.len(),
            3,
            "ImageStabilization element-1 map size changed"
        );

        let mut mismatches = Vec::new();
        for (k, expected) in &f.element0 {
            let id: i32 = k.parse().expect("fixture key is not an integer");
            let got = DECODE_IMAGE_STABILIZATION.decode(id);
            if &got != expected {
                mismatches.push(format!(
                    "element0 id {id}: got {got:?}, ExifTool says {expected:?}"
                ));
            }
        }
        for (k, expected) in &f.element1 {
            let id: i32 = k.parse().expect("fixture key is not an integer");
            let got = DECODE_IMAGE_STABILIZATION_MODE.decode(id);
            if &got != expected {
                mismatches.push(format!(
                    "element1 id {id}: got {got:?}, ExifTool says {expected:?}"
                ));
            }
        }
        assert!(
            mismatches.is_empty(),
            "FujiFilm ImageStabilization decoders have drifted from FujiFilm.pm:790-804:\n  {}",
            mismatches.join("\n  ")
        );
    }
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
        assert_eq!(FUJI_AUTO_BRACKETING, 0x1100);
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
        assert_eq!(fujifilm_tag_to_name(0x0000), "FujiFilm:Version");
        assert_eq!(fujifilm_tag_to_name(0x1000), "FujiFilm:Quality");
        assert_eq!(fujifilm_tag_to_name(0x1002), "FujiFilm:WhiteBalance");
        assert_eq!(fujifilm_tag_to_name(0x1401), "FujiFilm:FilmMode");
        assert_eq!(fujifilm_tag_to_name(0xFFFF), "FujiFilm:Unknown-0xFFFF");
    }

    #[test]
    fn test_decode_quality() {
        assert_eq!(DECODE_QUALITY.decode(1), "F (Fine)");
        assert_eq!(DECODE_QUALITY.decode(3), "Fine");
        assert_eq!(DECODE_QUALITY.decode(5), "Fine+RAW");
        assert_eq!(DECODE_QUALITY.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(DECODE_WHITE_BALANCE.decode(0x0000), "Auto");
        assert_eq!(DECODE_WHITE_BALANCE.decode(0x0100), "Daylight");
        assert_eq!(DECODE_WHITE_BALANCE.decode(0x0200), "Cloudy");
        assert_eq!(DECODE_WHITE_BALANCE.decode(0x0400), "Incandescent");
        assert_eq!(DECODE_WHITE_BALANCE.decode(0x9999), "Unknown (39321)");
    }

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(DECODE_FOCUS_MODE.decode(0), "Auto");
        assert_eq!(DECODE_FOCUS_MODE.decode(1), "Manual");
        assert_eq!(DECODE_FOCUS_MODE.decode(2), "AF-S (Single)");
        assert_eq!(DECODE_FOCUS_MODE.decode(3), "AF-C (Continuous)");
        assert_eq!(DECODE_FOCUS_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_film_mode() {
        assert_eq!(DECODE_FILM_MODE.decode(0x0000), "F0/Standard (Provia)");
        assert_eq!(DECODE_FILM_MODE.decode(0x0200), "F2/Fujichrome (Velvia)");
        assert_eq!(DECODE_FILM_MODE.decode(0x0600), "Classic Chrome");
        assert_eq!(DECODE_FILM_MODE.decode(0x0700), "Eterna");
        assert_eq!(DECODE_FILM_MODE.decode(0x0800), "Classic Negative");
        assert_eq!(DECODE_FILM_MODE.decode(0x9999), "Unknown (39321)");
    }

    #[test]
    fn test_decode_dynamic_range() {
        assert_eq!(DECODE_DYNAMIC_RANGE.decode(1), "Standard");
        assert_eq!(DECODE_DYNAMIC_RANGE.decode(3), "Wide");
        assert_eq!(DECODE_DYNAMIC_RANGE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_dynamic_range_setting() {
        assert_eq!(DECODE_DYNAMIC_RANGE_SETTING.decode(0x000), "Auto");
        assert_eq!(DECODE_DYNAMIC_RANGE_SETTING.decode(0x001), "Manual");
        assert_eq!(
            DECODE_DYNAMIC_RANGE_SETTING.decode(0x100),
            "Standard (100%)"
        );
        assert_eq!(DECODE_DYNAMIC_RANGE_SETTING.decode(0x201), "Wide2 (400%)");
    }

    // Every string below was read out of ExifTool's own PrintConv hash
    // (dumped from the Perl symbol table, ExifTool 13.59) -- not out of an
    // earlier version of this file. The ids chosen are the ones the two
    // tables used to disagree on, so a revert to the old labels fails here.
    #[test]
    fn test_decode_shutter_type() {
        assert_eq!(DECODE_SHUTTER_TYPE.decode(0), "Mechanical");
        assert_eq!(DECODE_SHUTTER_TYPE.decode(1), "Electronic");
        assert_eq!(
            DECODE_SHUTTER_TYPE.decode(2),
            "Electronic (long shutter speed)"
        );
        assert_eq!(DECODE_SHUTTER_TYPE.decode(3), "Electronic Front Curtain");
        assert_eq!(DECODE_SHUTTER_TYPE.decode(99), "Unknown (99)");
    }

    /// FujiFilm.pm:688-709. Three tags, three tables; only BlurWarning ever
    /// prints the word "None", and neither of the other two prints
    /// "Warning".
    #[test]
    fn test_warning_tags_do_not_share_one_table() {
        assert_eq!(DECODE_BLUR_WARNING.decode(0), "None");
        assert_eq!(DECODE_BLUR_WARNING.decode(1), "Blur Warning");

        assert_eq!(DECODE_FOCUS_WARNING.decode(0), "Good");
        assert_eq!(DECODE_FOCUS_WARNING.decode(1), "Out of focus");

        assert_eq!(DECODE_EXPOSURE_WARNING.decode(0), "Good");
        assert_eq!(DECODE_EXPOSURE_WARNING.decode(1), "Bad exposure");

        // The invented pair this replaced.
        for d in [
            &DECODE_BLUR_WARNING,
            &DECODE_FOCUS_WARNING,
            &DECODE_EXPOSURE_WARNING,
        ] {
            assert_ne!(d.decode(1), "Warning");
        }
        assert_ne!(DECODE_FOCUS_WARNING.decode(0), "None");
        assert_ne!(DECODE_EXPOSURE_WARNING.decode(0), "None");
    }

    /// 0x1033 is Auto/Manual (FujiFilm.pm:617-624), not an off/on flag.
    #[test]
    fn test_decode_exr_auto_is_not_off_on() {
        assert_eq!(DECODE_EXR_AUTO.decode(0), "Auto");
        assert_eq!(DECODE_EXR_AUTO.decode(1), "Manual");
        assert_ne!(DECODE_EXR_AUTO.decode(0), DECODE_OFF_ON.decode(0));
        assert_ne!(DECODE_EXR_AUTO.decode(1), DECODE_OFF_ON.decode(1));
    }

    /// FujiFilm.pm:543-552. Value 0 is "n/a" and 1 is a GFX-only full-frame
    /// marker; the old table started the crop labels one slot early.
    #[test]
    fn test_decode_crop_mode() {
        assert_eq!(DECODE_CROP_MODE.decode(0), "n/a");
        assert_eq!(DECODE_CROP_MODE.decode(1), "Full-frame on GFX");
        assert_eq!(DECODE_CROP_MODE.decode(2), "Sports Finder Mode");
        assert_eq!(DECODE_CROP_MODE.decode(4), "Electronic Shutter 1.25x Crop");
        assert_eq!(DECODE_CROP_MODE.decode(8), "Digital Tele-Conv");
    }

    /// FujiFilm.pm:641-650: 2 is Left and 3 is Up, not the other way round.
    #[test]
    fn test_decode_panorama_direction() {
        assert_eq!(DECODE_PANORAMA_DIRECTION.decode(1), "Right");
        assert_eq!(DECODE_PANORAMA_DIRECTION.decode(2), "Left");
        assert_eq!(DECODE_PANORAMA_DIRECTION.decode(3), "Up");
        assert_eq!(DECODE_PANORAMA_DIRECTION.decode(4), "Down");
    }

    /// FujiFilm.pm:651-675 keys AdvancedFilter on the high half of a 32-bit
    /// value. The old 0..0x10 table shared no id with ExifTool's at all.
    #[test]
    fn test_decode_advanced_filter() {
        assert_eq!(DECODE_ADVANCED_FILTER.decode(0x10000), "Pop Color");
        assert_eq!(DECODE_ADVANCED_FILTER.decode(0x40000), "Miniature");
        assert_eq!(
            DECODE_ADVANCED_FILTER.decode(0x60003),
            "Partial Color Green"
        );
        assert_eq!(
            DECODE_ADVANCED_FILTER.decode(0x130002),
            "Expired Film Neutral"
        );
        // Nothing lives at the low ids the old table used.
        for id in 0..=0x10 {
            assert!(DECODE_ADVANCED_FILTER.decode(id).starts_with("Unknown ("));
        }
    }

    /// Three D-Range tags, three tables (FujiFilm.pm:795-819).
    #[test]
    fn test_drange_priority_tags_do_not_share_one_table() {
        assert_eq!(DECODE_DRANGE_PRIORITY.decode(0), "Auto");
        assert_eq!(DECODE_DRANGE_PRIORITY.decode(1), "Fixed");
        assert_eq!(DECODE_DRANGE_PRIORITY_AUTO.decode(3), "Plus");
        assert_eq!(DECODE_DRANGE_PRIORITY_FIXED.decode(2), "Strong");
        assert_ne!(DECODE_DRANGE_PRIORITY.decode(1), "Weak");
    }

    /// FujiFilm.pm:820-838. VideoRecordingMode is PrintHex with a 0x10 step;
    /// VideoCompression is Log GOP / All Intra, not a codec name.
    #[test]
    fn test_decode_video_tables() {
        assert_eq!(DECODE_VIDEO_RECORDING_MODE.decode(0x00), "Normal");
        assert_eq!(DECODE_VIDEO_RECORDING_MODE.decode(0x10), "F-log");
        assert_eq!(DECODE_VIDEO_RECORDING_MODE.decode(0x20), "HLG");
        assert_eq!(DECODE_VIDEO_RECORDING_MODE.decode(0x30), "F-log2");

        assert_eq!(DECODE_VIDEO_COMPRESSION.decode(1), "Log GOP");
        assert_eq!(DECODE_VIDEO_COMPRESSION.decode(2), "All Intra");
        assert_eq!(DECODE_VIDEO_COMPRESSION.decode(3), "Unknown (3)");
    }

    /// FujiFilm.pm:676-687. B&W sits at 0x30 and ExifTool spells it with
    /// spaces around the ampersand.
    #[test]
    fn test_decode_color_mode() {
        assert_eq!(DECODE_COLOR_MODE.decode(0x00), "Standard");
        assert_eq!(DECODE_COLOR_MODE.decode(0x10), "Chrome");
        assert_eq!(DECODE_COLOR_MODE.decode(0x30), "B & W");
        assert_eq!(DECODE_COLOR_MODE.decode(0x20), "Unknown (32)");
    }

    /// FujiFilm.pm:725-745 and :206-233 -- exact spellings ExifTool prints.
    #[test]
    fn test_spellings_match_exiftool_exactly() {
        assert_eq!(DECODE_FILM_MODE.decode(0x0B00), "Reala ACE");
        assert_eq!(DECODE_FILM_MODE.decode(0x0A00), "Nostalgic Neg");
        assert_eq!(
            DECODE_FILM_MODE.decode(0x0120),
            "F1b/Studio Portrait Smooth Skin Tone (Astia)"
        );
        assert_eq!(DECODE_WHITE_BALANCE.decode(1), "Auto (white priority)");
        assert_eq!(DECODE_WHITE_BALANCE.decode(2), "Auto (ambiance priority)");
        assert_eq!(
            DECODE_EXR_MODE.decode(0x200),
            "SN (Signal to Noise priority)"
        );
        assert_eq!(DECODE_EXR_MODE.decode(0x300), "DR (Dynamic Range priority)");
        assert_eq!(DECODE_SCENE_RECOGNITION.decode(0x100), "Portrait Image");
        assert_eq!(DECODE_SCENE_RECOGNITION.decode(0x200), "Landscape Image");
    }

    /// 0x1050 is ShutterType, and 0x1304 is GEImageSize on GE bodies only --
    /// neither is "LensModelName" or "DynamicRangeWarning", names that appear
    /// in no ExifTool source file.
    #[test]
    fn test_fabricated_tag_names_are_gone() {
        assert_eq!(fujifilm_tag_to_name(0x1050), "FujiFilm:ShutterType");
        assert_eq!(fujifilm_tag_to_name(0x1304), "FujiFilm:Unknown-0x1304");
    }

    #[test]
    fn test_decode_picture_mode() {
        assert_eq!(DECODE_PICTURE_MODE.decode(0x0000), "Auto");
        assert_eq!(DECODE_PICTURE_MODE.decode(0x0001), "Portrait");
        assert_eq!(DECODE_PICTURE_MODE.decode(0x0002), "Landscape");
        assert_eq!(DECODE_PICTURE_MODE.decode(0x0006), "Program AE");
        assert_eq!(DECODE_PICTURE_MODE.decode(0x0009), "Beach & Snow");
        assert_eq!(DECODE_PICTURE_MODE.decode(0x0300), "Manual");
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = FujifilmParser;
        assert_eq!(parser.manufacturer_name(), "FujiFilm");
        assert_eq!(parser.tag_prefix(), "FujiFilm:");
    }

    #[test]
    fn test_decode_off_on() {
        assert_eq!(DECODE_OFF_ON.decode(0), "Off");
        assert_eq!(DECODE_OFF_ON.decode(1), "On");
        assert_eq!(DECODE_OFF_ON.decode(2), "Unknown (2)");
    }

    #[test]
    fn test_decode_drive_mode() {
        assert_eq!(DECODE_DRIVE_MODE.decode(0), "Single Frame");
        assert_eq!(DECODE_DRIVE_MODE.decode(1), "Continuous Low");
        assert_eq!(DECODE_DRIVE_MODE.decode(2), "Continuous High");
        assert_eq!(DECODE_DRIVE_MODE.decode(4), "Self-timer");
    }

    #[test]
    fn test_decode_exr_mode() {
        assert_eq!(DECODE_EXR_MODE.decode(256), "HR (High Resolution)");
        assert_eq!(DECODE_EXR_MODE.decode(512), "SN (Signal to Noise priority)");
        assert_eq!(DECODE_EXR_MODE.decode(768), "DR (Dynamic Range priority)");
    }

    /// Exactly the four tags with a `SubDirectory` select a table, and no
    /// neighbour does -- binding one to the wrong id would print a real
    /// ExifTool name over an unrelated word.
    #[test]
    fn test_only_the_four_settings_tags_select_a_table() {
        for tag in [0x102Bu16, 0x102D, 0x102E, 0x1103] {
            assert!(fujifilm_binary_subdir(tag).is_some(), "{tag:#06x}");
        }
        for tag in [0x102Au16, 0x102C, 0x102F, 0x1102, 0x1104, 0x1105] {
            assert!(fujifilm_binary_subdir(tag).is_none(), "{tag:#06x}");
        }
    }

    fn decode_settings(table: &BinaryTable, record: &[u8]) -> HashMap<String, String> {
        let mut tags = HashMap::new();
        decode_binary_subdir(
            table,
            record,
            ByteOrder::LittleEndian,
            "FujiFilm",
            &mut tags,
        );
        tags
    }

    /// `combined-samples/FujiFilm/FujiFilmX-S20.jpg`: the exact record bytes
    /// `exiftool -v3` prints for tags 0x102b/0x102d/0x102e/0x1103, and the exact
    /// values `exiftool -a -G1 -s` reports for them.
    ///
    /// This body is the one in the corpus that exercises `AFAreaZoneSize`'s
    /// `OTHER` sub: 0x102d is `01 01 63 00`, whose 0xff0000 field is 0x63, and
    /// ExifTool prints `3 x 3` -- `$val & 0x0f` and `$val >> 5`, which a `>> 4`
    /// would render `3 x 6`.
    #[test]
    fn test_settings_match_exiftool_on_x_s20_bytes() {
        let tags = decode_settings(&FUJIFILM_PRIORITYSETTINGS, &[0x12, 0x00]);
        assert_eq!(tags["FujiFilm:AF-SPriority"], "Focus");
        assert_eq!(tags["FujiFilm:AF-CPriority"], "Release");

        let tags = decode_settings(&FUJIFILM_FOCUSSETTINGS, &[0x01, 0x01, 0x63, 0x00]);
        assert_eq!(tags["FujiFilm:FocusMode2"], "AF-S");
        assert_eq!(tags["FujiFilm:PreAF"], "Off");
        assert_eq!(tags["FujiFilm:AFAreaMode"], "Zone");
        assert_eq!(tags["FujiFilm:AFAreaPointSize"], "n/a");
        assert_eq!(tags["FujiFilm:AFAreaZoneSize"], "3 x 3");

        let tags = decode_settings(&FUJIFILM_AFCSETTINGS, &[0x02, 0x01, 0x00, 0x00]);
        assert_eq!(tags["FujiFilm:AF-CSetting"], "Set 1 (multi-purpose)");
        assert_eq!(tags["FujiFilm:AF-CTrackingSensitivity"], "2");
        assert_eq!(tags["FujiFilm:AF-CSpeedTrackingSensitivity"], "0");
        assert_eq!(tags["FujiFilm:AF-CZoneAreaSwitching"], "Auto");

        let tags = decode_settings(&FUJIFILM_DRIVESETTINGS, &[0x00, 0x00, 0x00, 0x00]);
        assert_eq!(tags["FujiFilm:DriveMode"], "Single");
        assert_eq!(tags["FujiFilm:DriveSpeed"], "n/a");
    }

    /// `combined-samples/FujiFilm/FujiFilmGFX50S_II.jpg` tag 0x102d, the corpus
    /// case for `AFAreaPointSize`'s `OTHER` sub: 0x40 in the 0xf000 field is 4,
    /// not one of the hash's keys, so ExifTool prints the number itself.
    #[test]
    fn test_af_area_point_size_falls_through_to_the_number() {
        let tags = decode_settings(&FUJIFILM_FOCUSSETTINGS, &[0x01, 0x40, 0x00, 0x00]);
        assert_eq!(tags["FujiFilm:AFAreaPointSize"], "4");
        assert_eq!(tags["FujiFilm:AFAreaMode"], "Single Point");
        assert_eq!(tags["FujiFilm:AFAreaZoneSize"], "n/a");
    }

    /// `combined-samples/FujiFilm/FujiFilmX-H2S.jpg` tag 0x102d: the low nibble
    /// is the whole of `FocusMode2`, so an unmasked read of the int32u would
    /// report 514 instead of `AF-C`.
    #[test]
    fn test_masks_split_one_word_into_its_fields() {
        let tags = decode_settings(&FUJIFILM_FOCUSSETTINGS, &[0x02, 0x02, 0x00, 0x00]);
        assert_eq!(tags["FujiFilm:FocusMode2"], "AF-C");
        assert_eq!(tags["FujiFilm:AFAreaMode"], "Wide/Tracking");
    }

    // ImageStabilization (tag 0x1422). FujiFilm.pm:790-806 (ExifTool 13.59).
    //
    //     0x1422 => {
    //         Name => 'ImageStabilization',
    //         Writable => 'int16u',
    //         Count => 3,
    //         PrintConv => [{
    //             0 => 'None',
    //             1 => 'Optical', #PH
    //             2 => 'Sensor-shift', #PH (now IBIS/OIS, ref forum13708)
    //             3 => 'OIS Lens', #forum9815 (optical+sensor?)
    //             258 => 'IBIS/OIS + DIS', #forum13708 (digital on top of IBIS/OIS)
    //             512 => 'Digital', #PH
    //         },{
    //             0 => 'Off',
    //             1 => 'On (mode 1, continuous)',
    //             2 => 'On (mode 2, shooting only)',
    //         }],
    //     },
    //
    // verified against the pinned oracle (13.59) on
    // stage1-samples/FujiFilm/FujiFilmX-S10.jpg:
    //   `[FujiFilm]      ImageStabilization              : OIS Lens; On (mode 1, continuous); 0`

    /// Element 0 -- the IS system (FujiFilm.pm:795-800). There is no 256 key
    /// in ExifTool's hash; that entry (and the other four labels) were
    /// invented in an earlier revision of this file.
    #[test]
    fn test_decode_image_stabilization_system() {
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(0), "None");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(1), "Optical");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(2), "Sensor-shift");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(3), "OIS Lens");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(258), "IBIS/OIS + DIS");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(512), "Digital");
        // 256 was the invented key this replaced; it is not in FujiFilm.pm
        // and must not decode to any of the real labels.
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(256), "Unknown (256)");
    }

    /// Element 1 -- the IS mode (FujiFilm.pm:801-804).
    #[test]
    fn test_decode_image_stabilization_mode() {
        assert_eq!(DECODE_IMAGE_STABILIZATION_MODE.decode(0), "Off");
        assert_eq!(
            DECODE_IMAGE_STABILIZATION_MODE.decode(1),
            "On (mode 1, continuous)"
        );
        assert_eq!(
            DECODE_IMAGE_STABILIZATION_MODE.decode(2),
            "On (mode 2, shooting only)"
        );
    }

    /// End-to-end pointer decode: 6 bytes (3x int16u) never fit in the 4-byte
    /// inline `value_offset` field, so `value_offset` must be read as a
    /// pointer into the MakerNote body, not as the tag's value itself (the
    /// bug this test pins). Bytes are hand-embedded, not read from any /tmp
    /// path:
    //
    //   offset  0..8   "FUJIFILM"
    //   offset  8..12  IFD offset = 12                (u32 LE)
    //   offset 12..14  entry_count = 1                (u16 LE)
    //   offset 14..16  tag_id = 0x1422                (u16 LE)
    //   offset 16..18  field_type = 3 (SHORT)         (u16 LE)
    //   offset 18..22  value_count = 3                (u32 LE)
    //   offset 22..26  value_offset = 26 (pointer)     (u32 LE)
    //   offset 26..32  array data: 3, 1, 0             (3x u16 LE)
    //
    // array data (3, 1, 0) mirrors the oracle-observed
    // FujiFilmX-S10.jpg encoding: element 0 = 3 ("OIS Lens"), element 1 = 1
    // ("On (mode 1, continuous)"), element 2 = 0 (no PrintConv, raw number).
    #[test]
    fn test_image_stabilization_reads_pointer_not_inline_value() {
        let data: &[u8] = &[
            0x46, 0x55, 0x4a, 0x49, 0x46, 0x49, 0x4c, 0x4d, // "FUJIFILM"
            0x0c, 0x00, 0x00, 0x00, // IFD offset = 12
            0x01, 0x00, // entry_count = 1
            0x22, 0x14, // tag_id = 0x1422
            0x03, 0x00, // field_type = SHORT
            0x03, 0x00, 0x00, 0x00, // value_count = 3
            0x1a, 0x00, 0x00, 0x00, // value_offset = 26
            0x03, 0x00, 0x01, 0x00, 0x00, 0x00, // array data: 3, 1, 0
        ];

        let parser = FujifilmParser;
        let mut tags = HashMap::new();
        parser
            .parse(data, ByteOrder::LittleEndian, &mut tags)
            .expect("synthetic FujiFilm MakerNote should parse");

        assert_eq!(
            tags["FujiFilm:ImageStabilization"],
            "OIS Lens; On (mode 1, continuous); 0"
        );
    }

    /// Never-approximate rule (AGENTS.md): if the pointed-to array data is
    /// truncated (value_offset run off the end of the buffer),
    /// `extract_u16_array` returns `None` and the tag must be omitted
    /// entirely -- not inserted with a value read from garbage/out-of-bounds
    /// memory, and not partially filled from 1 or 2 of the 3 elements.
    #[test]
    fn test_image_stabilization_omits_when_pointer_is_out_of_bounds() {
        let data: &[u8] = &[
            0x46, 0x55, 0x4a, 0x49, 0x46, 0x49, 0x4c, 0x4d, // "FUJIFILM"
            0x0c, 0x00, 0x00, 0x00, // IFD offset = 12
            0x01, 0x00, // entry_count = 1
            0x22, 0x14, // tag_id = 0x1422
            0x03, 0x00, // field_type = SHORT
            0x03, 0x00, 0x00, 0x00, // value_count = 3
            0xff, 0x00, 0x00, 0x00, // value_offset = 255 -- past end of buffer
        ];

        let parser = FujifilmParser;
        let mut tags = HashMap::new();
        parser
            .parse(data, ByteOrder::LittleEndian, &mut tags)
            .expect("a truncated array pointer must not fail the whole parse");

        assert!(
            !tags.contains_key("FujiFilm:ImageStabilization"),
            "out-of-bounds array pointer must omit the tag, not approximate one"
        );
    }
}
