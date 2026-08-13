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
/// `%Panasonic` binary sub-tables, generated from ExifTool's own hashes.
pub mod face_tables;

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

use super::makernote_context::{MakerNoteContext, value_overlaps_directory};
use super::shared::MakerNoteParser;
use super::shared::binary_subdir::{BinaryTable, decode_binary_subdir};
use super::shared::ifd_parser_base::resolve_byte_order_at;
use face_tables::{
    PANASONIC_FACEDETINFO, PANASONIC_FACERECINFO, PANASONIC_TIMEINFO, PANASONIC_TYPE2,
};

// Import declarative decoder macros
use crate::const_decoder;

// Import registry
use super::registries::panasonic::panasonic_registry;

// Panasonic MakerNote header signature
// Panasonic uses "Panasonic\0\0\0" header (12 bytes)
const PANASONIC_HEADER: &[u8] = b"Panasonic\0\0\0";

/// The unnumbered `MakerNoteLeica` header (MakerNotes.pm:599-604): cameras
/// whose `Make` is the bare string `"LEICA"` (not the `"Leica Camera AG"`
/// prefix the `Leica2`..`Leica10` layouts key on) write "LEICA\0\0\0" and
/// point ExifTool at `Panasonic::Main` -- the same table and tag group as a
/// real Panasonic body -- with the IFD starting 8 bytes in, not 12.
const LEICA_UNNUMBERED_HEADER: &[u8] = b"LEICA\0\0\0";

/// The `MakerNoteLeica10` header (MakerNotes.pm:724-731). Leica's Panasonic-built
/// compacts -- the D-Lux 7, D-Lux 8 and V-Lux 5 -- sign "LEICA CAMERA AG\0" and
/// ExifTool points them at `Panasonic::Main`, the same table and "Panasonic:"
/// group a real Panasonic body uses:
///
/// ```text
///     Name      => 'MakerNoteLeica10', # used by the D-Lux7
///     Condition => '$$valPt =~ /^LEICA CAMERA AG\0/',
///     TagTable  => 'Image::ExifTool::Panasonic::Main',
///     Start     => '$valuePtr + 18',
/// ```
///
/// The signature is 16 bytes and the IFD begins at 18, so two pad bytes sit
/// between them (`LeicaD-Lux7.jpg` reads `...41 47 00 00 00 9d 00` -- "AG\0",
/// `00 00`, then the 157-entry count). `MakerNoteLeica10` declares no `Base`,
/// so its out-of-line value offsets are measured from the enclosing TIFF header
/// exactly as `MakerNotePanasonic`'s are and need no adjustment here.
const LEICA10_HEADER: &[u8] = b"LEICA CAMERA AG\0";

/// True for a `MakerNoteLeica10` payload -- the signature ExifTool routes to
/// `Panasonic::Main` rather than to any of the `Leica2`..`Leica9` tables.
pub fn is_leica10_makernote(data: &[u8]) -> bool {
    data.len() >= LEICA10_HEADER.len() + 2 && data.starts_with(LEICA10_HEADER)
}

/// Returns the byte offset of this payload's IFD, or `None` if none of the
/// Panasonic, unnumbered-Leica or Leica10 headers matches.
fn panasonic_ifd_offset(data: &[u8]) -> Option<usize> {
    if data.len() >= 12 && &data[0..12] == PANASONIC_HEADER {
        Some(12)
    } else if data.len() >= 8 && &data[0..8] == LEICA_UNNUMBERED_HEADER {
        Some(8)
    } else if is_leica10_makernote(data) {
        Some(18)
    } else {
        None
    }
}

/// `MakerNotePanasonic2` is the legacy fixed `Panasonic::Type2` record.
/// MakerNotes.pm:743-750 selects it from a `MKE` signature and forces little
/// endian, independent of the enclosing TIFF's byte order.
fn is_panasonic_type2_makernote(data: &[u8]) -> bool {
    data.starts_with(b"MKE")
}

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================
// Using const_decoder! macro to eliminate decoder function duplication

// Image quality decoder (tag 0x0001) - ExifTool Panasonic.pm ImageQuality PrintConv
const_decoder!(pub IMAGE_QUALITY,
    i32,
    [
        (1, "TIFF"),
        (2, "High"),
        (3, "Normal"),
        (6, "Very High"),
        (7, "RAW"),
        (9, "Motion Picture"),
        (11, "Full HD Movie"),
        (12, "4k Movie"),
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

// FocusMode (tag 0x0007), transcribed from Panasonic.pm:290-302. The
// AF-S/AF-C/AF-F labels live at 6/7/8; 4 and 5 are 'Auto, Focus button'
// and 'Auto, Continuous'. oxidex used to put AF-S/AF-C/AF-F at 4/5/6 and
// invent an entry at 16, so a G-series body reported the wrong AF mode.
const_decoder!(pub FOCUS_MODE,
    i32,
    [
        (1, "Auto"),
        (2, "Manual"),
        (4, "Auto, Focus button"),
        (5, "Auto, Continuous"),
        (6, "AF-S"),
        (7, "AF-C"),
        (8, "AF-F"),
    ]
);

// AF area mode (tag 0x000F) is an int8u pair; see decode_af_area_mode below.

// Image stabilization decoder (tag 0x001A) - ExifTool Panasonic.pm ImageStabilization PrintConv
const_decoder!(pub IMAGE_STABILIZATION,
    i32,
    [
        (2, "On, Optical"),
        (3, "Off"),
        (4, "On, Mode 2"),
        (5, "On, Optical Panning"),
        (6, "On, Body-only"),
        (7, "On, Body-only Panning"),
        (9, "Dual IS"),
        (10, "Dual IS Panning"),
        (11, "Dual2 IS"),
        (12, "Dual2 IS Panning"),
    ]
);

// ShootingMode (tag 0x001F) and, via decode_scene_mode, SceneMode
// (0x8001). Panasonic.pm's %shootingMode runs to 92; the table here used
// to stop at 20 and spell 18/19/20 as Panorama/Glass Through/HDR, which
// ExifTool calls Fireworks/Party/Snow -- and Panorama/Glass Through/HDR
// are really 62/63/64.
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
        (16, "Self Portrait"),
        (17, "Economy"),
        (18, "Fireworks"),
        (19, "Party"),
        (20, "Snow"),
        (21, "Night Scenery"),
        (22, "Food"),
        (23, "Baby"),
        (24, "Soft Skin"),
        (25, "Candlelight"),
        (26, "Starry Night"),
        (27, "High Sensitivity"),
        (28, "Panorama Assist"),
        (29, "Underwater"),
        (30, "Beach"),
        (31, "Aerial Photo"),
        (32, "Sunset"),
        (33, "Pet"),
        (34, "Intelligent ISO"),
        (35, "Clipboard"),
        (36, "High Speed Continuous Shooting"),
        (37, "Intelligent Auto"),
        (39, "Multi-aspect"),
        (41, "Transform"),
        (42, "Flash Burst"),
        (43, "Pin Hole"),
        (44, "Film Grain"),
        (45, "My Color"),
        (46, "Photo Frame"),
        (48, "Movie"),
        (51, "HDR"),
        (52, "Peripheral Defocus"),
        (55, "Handheld Night Shot"),
        (57, "3D"),
        (59, "Creative Control"),
        (60, "Intelligent Auto Plus"),
        (62, "Panorama"),
        (63, "Glass Through"),
        (64, "HDR"),
        (66, "Digital Filter"),
        (67, "Clear Portrait"),
        (68, "Silky Skin"),
        (69, "Backlit Softness"),
        (70, "Clear in Backlight"),
        (71, "Relaxing Tone"),
        (72, "Sweet Child's Face"),
        (73, "Distinct Scenery"),
        (74, "Bright Blue Sky"),
        (75, "Romantic Sunset Glow"),
        (76, "Vivid Sunset Glow"),
        (77, "Glistening Water"),
        (78, "Clear Nightscape"),
        (79, "Cool Night Sky"),
        (80, "Warm Glowing Nightscape"),
        (81, "Artistic Nightscape"),
        (82, "Glittering Illuminations"),
        (83, "Clear Night Portrait"),
        (84, "Soft Image of a Flower"),
        (85, "Appetizing Food"),
        (86, "Cute Dessert"),
        (87, "Freeze Animal Motion"),
        (88, "Clear Sports Shot"),
        (89, "Monochrome"),
        (90, "Creative Control"),
        (92, "Handheld Night Shot"),
    ]
);

// ContrastMode (tag 0x002C), the variant Panasonic.pm:411-431 applies to
// everything but the FX10/G1/L1/L10/LC80/GF*/G2/TZ10/ZS7 and DC- bodies.
const_decoder!(pub CONTRAST_MODE,
    i32,
    [
        (0, "Normal"),
        (1, "Low"),
        (2, "High"),
        (5, "Normal 2"),
        (6, "Medium Low"),
        (7, "Medium High"),
        (13, "High Dynamic"),
        (24, "Dynamic Range (film-like)"),
        (46, "Match Filter Effects Toy"),
        (55, "Match Photo Style L. Monochrome"),
        (256, "Low"),
        (272, "Normal"),
        (288, "High"),
    ]
);

// Film mode decoder (tag 0x0042) - ExifTool Panasonic.pm FilmMode PrintConv
const_decoder!(pub FILM_MODE,
    i32,
    [
        (0, "n/a"),
        (1, "Standard (color)"),
        (2, "Dynamic (color)"),
        (3, "Nature (color)"),
        (4, "Smooth (color)"),
        (5, "Standard (B&W)"),
        (6, "Dynamic (B&W)"),
        (7, "Smooth (B&W)"),
        (10, "Nostalgic"),
        (11, "Vibrant"),
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

// HDR (tag 0x009E), Panasonic.pm:1611-1622: the EV steps are 100/200/300
// and the Auto variants are 0x8064-based. The old 0/1/2/3/100 table
// matched ExifTool on the single value 0.
const_decoder!(pub HDR,
    i32,
    [
        (0, "Off"),
        (100, "1 EV"),
        (200, "2 EV"),
        (300, "3 EV"),
        (32868, "1 EV (Auto)"),
        (32968, "2 EV (Auto)"),
        (33068, "3 EV (Auto)"),
    ]
);

// PhotoStyle (tag 0x0089), Panasonic.pm:1329-1345. The old table was
// shifted a slot against ExifTool's at every id it shared -- 0 was
// 'Standard' where ExifTool says 'Auto', 1 'Vivid' where ExifTool says
// 'Standard or Custom' -- so it agreed with ExifTool on nothing.
const_decoder!(pub PHOTO_STYLE,
    i32,
    [
        (0, "Auto"),
        (1, "Standard or Custom"),
        (2, "Vivid"),
        (3, "Natural"),
        (4, "Monochrome"),
        (5, "Scenery"),
        (6, "Portrait"),
        (8, "Cinelike D"),
        (9, "Cinelike V"),
        (11, "L. Monochrome"),
        (12, "Like709"),
        (15, "L. Monochrome D"),
        (17, "V-Log"),
        (18, "Cinelike D2"),
    ]
);

// MacroMode (tag 0x001C), Panasonic.pm:265-273.
const_decoder!(pub MACRO_MODE, i32, [
    (1, "On"),
    (2, "Off"),
    (257, "Tele-Macro"),
    (513, "Macro Zoom"),
]);

// Rotation decoder (tag 0x0030) - ExifTool Panasonic.pm Rotation PrintConv
const_decoder!(pub ROTATION,
    i32,
    [
        (1, "Horizontal (normal)"),
        (3, "Rotate 180"),
        (6, "Rotate 90 CW"),
        (8, "Rotate 270 CW"),
    ]
);

// Color mode decoder (tag 0x0032) - ExifTool Panasonic.pm ColorMode PrintConv
const_decoder!(pub COLOR_MODE,
    i32,
    [(0, "Normal"), (1, "Natural"), (2, "Vivid"),]
);

// InternalNDFilter (0x009D) has no PrintConv in ExifTool: Panasonic.pm:1247
// declares only `Writable => 'rational64u'`, so the value is reported as-is
// (no Off/On/Auto decoder -- one used to live here but was invented). The
// rational64u decode itself is handled in `parse_entry`'s tag_id match, next
// to ClearRetouchValue, since it needs the out-of-line numerator/denominator
// bytes rather than a scalar decoder.

// CameraOrientation decoder (tag 0x008F), Panasonic.pm:1188-1199. Registered
// with no decoder at all, so oxidex printed the raw int8u ("0") where
// ExifTool prints "Normal".
const_decoder!(pub CAMERA_ORIENTATION,
    i32,
    [
        (0, "Normal"),
        (1, "Rotate CW"),
        (2, "Rotate 180"),
        (3, "Rotate CCW"),
        (4, "Tilt Upwards"),
        (5, "Tilt Downwards"),
    ]
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
const_decoder!(pub LONG_EXPOSURE_NR, i32, [(1, "Off"), (2, "On"),]);

// BurstMode (tag 0x002A), Panasonic.pm:398-410. Value 1 is a plain 'On',
// not 'Low/High Speed', and 2 is AEB rather than 'Infinite'.
const_decoder!(pub BURST_MODE,
    i32,
    [
        (0, "Off"),
        (1, "On"),
        (2, "Auto Exposure Bracketing (AEB)"),
        (3, "Focus Bracketing"),
        (4, "Unlimited"),
        (8, "White Balance Bracketing"),
        (17, "On (with flash)"),
        (18, "Aperture Bracketing"),
    ]
);

// `FaceDetection` was an invented name: `%Panasonic::Main` has no tag by that
// name at any id, and the 0x004e it was bound to is the `FaceDetInfo`
// sub-directory pointer, whose value is an offset -- so every file printed
// `Unknown (<offset>)`. See `panasonic_binary_subdir`.

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

// Self timer decoder (tag 0x002E) - ExifTool Panasonic.pm SelfTimer PrintConv
const_decoder!(pub SELF_TIMER, i32,
    [
        (0, "Off (0)"),
        (1, "Off"),
        (2, "10 s"),
        (3, "2 s"),
        (4, "10 s / 3 pictures"),
        (258, "2 s after shutter pressed"),
        (266, "10 s after shutter pressed"),
        (778, "3 photos after 10 s"),
    ]
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

// AdvancedSceneType (tag 0x003D) has no PrintConv in ExifTool Panasonic.pm;
// it is reported as its raw numeric value.

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

// BurstSpeed (0x0077) has no PrintConv in ExifTool: Panasonic.pm:1094 declares
// `Writable => 'int16u'` with `Notes => 'images per second'`. The Low/Mid/High
// decoder that used to live here was invented and printed "Low" for 0 fps.

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

// Panasonic.pm:1165-1443. Leica's D-Lux 8 reaches these through
// MakerNoteLeica10, which ExifTool routes to Panasonic::Main.
const_decoder!(pub VIDEO_BURST_RESOLUTION, i32, [(1, "Off or 4K"), (4, "6K"),]);
const_decoder!(pub RED_EYE_REMOVAL, i32, [(0, "Off"), (1, "On"),]);
const_decoder!(pub VIDEO_BURST_MODE, i32,
    [(0x01, "Off"), (0x04, "Post Focus"), (0x18, "4K Burst"),
     (0x28, "4K Burst (Start/Stop)"), (0x48, "4K Pre-burst"),
     (0x108, "Loop Recording"), (0x810, "6K Burst"),
     (0x820, "6K Burst (Start/Stop)"), (0x408, "Focus Stacking"),
     (0x1001, "High Resolution Mode"),]
);
const_decoder!(pub LONG_EXPOSURE_NR_USED, i32, [(1, "No"), (2, "Yes"),]);
const_decoder!(pub VIDEO_PREBURST, i32, [(0, "No"), (1, "4K or 6K"),]);
const_decoder!(pub SENSOR_TYPE, i32, [(0, "Multi-aspect"), (1, "Standard"),]);
const_decoder!(pub MONOCHROME_GRAIN_EFFECT, i32,
    [(0, "Off"), (1, "Low"), (2, "Standard"), (3, "High"),]
);

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
        // Panasonic header: "Panasonic\0\0\0" (12 bytes), or the unnumbered
        // "LEICA\0\0\0" (8 bytes) a bare-Make "LEICA" body writes for the
        // same Panasonic::Main table.
        panasonic_ifd_offset(data).is_some() || is_panasonic_type2_makernote(data)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_impl_with_tiff(data, byte_order, None, None, tags)
    }

    fn parse_with_model(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_impl_with_tiff(data, byte_order, model, None, tags)
    }

    /// Panasonic's out-of-line value offsets are measured from the enclosing
    /// TIFF header, not from the MakerNote payload -- `PanasonicDC-G9.jpg`
    /// stores `LensType`'s 34 string bytes at TIFF offset 3414 while the
    /// payload itself begins at TIFF offset 1314. `PanasonicDMC-GH4.jpg` goes
    /// the other way: `InternalNDFilter` (0x009d) and `ClearRetouchValue`
    /// (0x00a3) both point to TIFF offset 0x113c/0x114c, *before* the payload
    /// at 0x1370. Resolving either direction needs the enclosing block, which
    /// only `parse_with_context` has; `parse`/`parse_with_model` keep the old
    /// payload-relative arithmetic for a caller that holds no enclosing block.
    fn parse_with_context(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // A `detached` context (`ctx.is_located()` false) has `tiff() ==
        // payload()` -- no wider block was ever established, e.g. the
        // MakerNote inside `Panasonic.rw2`'s embedded `JpgFromRaw` preview
        // document. Treating that payload-only slice as TIFF-relative and
        // indexing `value_offset` into it directly reads the wrong bytes:
        // `InternalSerialNumber`/`AFPointPosition`/`BabyAge` all came back
        // wrong when this passed `Some(ctx.tiff())` unconditionally. Only a
        // genuinely located context makes `value_offset` a TIFF-relative
        // index; otherwise keep the old ifd_offset-relative fallback (`None`,
        // handled the same as `parse`/`parse_with_model`). `payload_offset()`
        // rides along so `parse_impl_with_tiff` can locate this MakerNote's
        // own entry list within `tiff()`, for the "Suspicious offset" guard
        // in `resolve_value_offset`.
        let full_tiff = ctx.is_located().then(|| (ctx.tiff(), ctx.payload_offset()));
        self.parse_impl_with_tiff(ctx.window(), byte_order, model, full_tiff, tags)
    }
}

impl PanasonicParser {
    /// Shared implementation behind [`MakerNoteParser::parse`],
    /// [`MakerNoteParser::parse_with_model`] and
    /// [`MakerNoteParser::parse_with_context`].
    ///
    /// `full_tiff` is the enclosing TIFF block (index 0 = the TIFF header)
    /// paired with the payload's TIFF-relative start, when a
    /// [`MakerNoteContext`] is available and located; `None` for a caller
    /// that holds only the MakerNote payload. `data` is always the
    /// payload/window used for the structural header and IFD-entry parsing
    /// below, which is payload-relative either way; only out-of-line *value*
    /// reads need the wider, TIFF-relative block, and only when it exists.
    #[allow(clippy::too_many_arguments)]
    fn parse_impl_with_tiff(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        model: Option<&str>,
        full_tiff: Option<(&[u8], usize)>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        if data.is_empty() {
            return Ok(());
        }

        // `MakerNotePanasonic2` does not contain an IFD.  ExifTool routes a
        // `MKE*` payload directly to `Panasonic::Type2` and explicitly fixes
        // its byte order to little endian (MakerNotes.pm:743-750).
        if is_panasonic_type2_makernote(data) {
            decode_binary_subdir(
                &PANASONIC_TYPE2,
                data,
                ByteOrder::LittleEndian,
                "Panasonic",
                tags,
            );
            return Ok(());
        }

        // Validate Panasonic header
        let Some(ifd_offset) = panasonic_ifd_offset(data) else {
            return Err("Invalid Panasonic MakerNote header".to_string());
        };

        if data.len() <= ifd_offset + 2 {
            return Ok(());
        }

        // `MakerNotePanasonic` declares `ByteOrder => 'Unknown'`
        // (MakerNotes.pm:733-741), so the directory is written in the camera's
        // own endianness and not necessarily the enclosing TIFF's. ExifTool
        // resolves it from the entry count (Exif.pm:6886-6893) and applies the
        // result to the entries *and* their values (`SetByteOrder`,
        // Exif.pm:7078), so it has to be settled before anything is read.
        let byte_order = resolve_byte_order_at(data, ifd_offset, byte_order);

        let ifd_data = &data[ifd_offset..];

        // Parse IFD entry count using EndianReader
        let reader = EndianReader::new(ifd_data, byte_order.to_io_byte_order());
        let entry_count = reader.u16_at(0).unwrap_or(0);

        // Parse IFD entries
        let entries_start = &ifd_data[2..];
        let entries = match parse_ifd_entries(entries_start, entry_count, byte_order) {
            Ok((_, entries)) => entries,
            // A directory that does not read is reported, not swallowed.
            // ExifTool warns ("Bad MakerNotes directory") and carries on with
            // the rest of the file, so this must stay non-fatal -- and every
            // caller of the dispatcher treats an `Err` exactly that way,
            // printing `Warning: Failed to parse MakerNote for <Make>` and
            // continuing. Returning `Ok(())` instead made a failed directory
            // indistinguishable from a file that simply has no MakerNote,
            // which is why the byte-order class this function now resolves
            // went unreported for its entire existence: a coverage report
            // cannot count a difference that produces no output at all.
            Err(_) => {
                return Err(format!(
                    "Panasonic MakerNote IFD at offset {ifd_offset} declares {entry_count} \
                     entries ({} bytes) but only {} bytes follow",
                    entry_count as usize * 12,
                    entries_start.len(),
                ));
            }
        };

        // Get registry for tag definitions
        let registry = panasonic_registry();

        // When the enclosing TIFF block is available, out-of-line values are
        // read from it directly (index = value_offset, plus the DC-FT7
        // correction) rather than from the payload window: a value_offset can
        // legitimately address bytes *before* the payload starts --
        // `PanasonicDMC-GH4.jpg`'s InternalNDFilter/ClearRetouchValue do
        // exactly that -- and only the whole block, not the forward-only
        // window, can reach them. See `resolve_value_offset` and
        // `value_offset_correction`.
        let (value_data, data_base): (&[u8], Option<TiffValueBase>) = match full_tiff {
            Some((tiff, payload_offset)) => {
                let dir_start = payload_offset + ifd_offset;
                let dir_end = dir_start + 2 + entry_count as usize * 12;
                (
                    tiff,
                    Some(TiffValueBase {
                        correction: value_offset_correction(model),
                        dir_start,
                        dir_end,
                    }),
                )
            }
            None => (data, None),
        };

        // Extract tags from entries
        for entry in entries {
            self.parse_entry(
                &entry, value_data, ifd_offset, data_base, byte_order, model, &registry, tags,
            );
        }

        Ok(())
    }

    /// Parse a single IFD entry using registry-based tag definitions
    ///
    /// Uses the Panasonic tag registry to determine tag names and apply value decoders.
    /// Special cases (lens lookups, custom formatting) are handled inline.
    #[allow(clippy::too_many_arguments)]
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        ifd_offset: usize,
        data_base: Option<TiffValueBase>,
        byte_order: ByteOrder,
        model: Option<&str>,
        registry: &super::shared::tag_registry::TagRegistry,
        tags: &mut HashMap<String, String>,
    ) {
        let tag_id = entry.tag_id;

        // Binary sub-directories. `%Panasonic::Main` gives 0x4e and 0x61 a
        // `SubDirectory => { TagTable => ... }` (Panasonic.pm:935-940 and
        // :1007-1011), so ExifTool descends and reports the record's fields --
        // the pointer itself is never a value. Neither tag has a Start, Base or
        // ByteOrder override, so the directory is exactly the tag's own bytes.
        if let Some(table) = panasonic_binary_subdir(tag_id) {
            if let Some(record) = extract_raw_bytes(entry, data, ifd_offset, data_base, byte_order)
            {
                decode_binary_subdir(table, &record, byte_order, "Panasonic", tags);
                if tag_id == 0x2003 {
                    decode_panasonic_datetime(&record, tags);
                }
            }
            return;
        }

        // Special handling for string tags (must read from data buffer)
        // These tags contain text data that needs to be extracted from the makernote
        match tag_id {
            // OutputLUT is flagged `Binary => 1` in Panasonic.pm:1310-1317.
            // Its raw type varies by body, so preserve ExifTool's standard
            // declared-byte-count placeholder instead of rendering samples.
            0x00A7 => {
                if let Some(bytes) = extract_raw_bytes(entry, data, ifd_offset, data_base, byte_order) {
                    tags.insert(
                        "Panasonic:OutputLUT".to_string(),
                        crate::cli::output_formatter::binary_placeholder(bytes.len()),
                    );
                }
                return;
            }
            // DataDump is a Binary undef payload. ExifTool prints its byte
            // count unless -b is requested, never the payload or offset.
            0x0021 => {
                if let Some(bytes) = extract_raw_bytes(entry, data, ifd_offset, data_base, byte_order) {
                    tags.insert(
                        "Panasonic:DataDump".to_string(),
                        crate::cli::output_formatter::binary_placeholder(bytes.len()),
                    );
                }
                return;
            }
            // Basic info strings.  0x0051 LensType, 0x0052 LensSerialNumber
            // and 0x0053 AccessoryType are all `Writable => 'string'` in
            // %Panasonic::Main (Panasonic.pm:943, :949 and :955) -- there is no
            // Panasonic lens-id table in ExifTool, the tag holds the name
            // itself.
            0x0026 | 0x0051 | 0x0052 | 0x0053 | 0x0054 |
            // Supplementary info strings (Title, BabyName)
            0x0065 | 0x0066 |
            // Location-related strings
            0x0067 | 0x0069 | 0x006B | 0x006D | 0x006F | 0x0080 |
            // LUT names are ordinary null-terminated strings.  They may be
            // empty, which is still a present ExifTool tag and must not be
            // dropped by a truthiness check.
            0x00F1 | 0x00F4 => {
                if let Some(value) = extract_string_value(entry, data, ifd_offset, data_base)
                    && let Some(tag_name) = registry.get_tag_name(tag_id) {
                        tags.insert(format!("Panasonic:{}", tag_name), value);
                    }
                return;
            }
            // BabyAge: ExifTool maps the "9999:99:99 00:00:00" sentinel to "(not set)"
            0x0033 => {
                if let Some(value) = extract_string_value(entry, data, ifd_offset, data_base) {
                    tags.insert("Panasonic:BabyAge".to_string(), format_baby_age(&value));
                }
                return;
            }
            // InternalSerialNumber: ExifTool reformats "(F35) 2008:07:01 no. 0058".
            // Match the pattern on the raw bytes first: the trailing bytes of the
            // 16-byte field are often not valid UTF-8, which must not hide a
            // well-formed serial prefix.
            0x0025 => {
                if let Some(bytes) = extract_raw_bytes(entry, data, ifd_offset, data_base, byte_order) {
                    let prefix = String::from_utf8_lossy(&bytes);
                    let formatted = format_internal_serial_number(&prefix);
                    if formatted != prefix {
                        tags.insert("Panasonic:InternalSerialNumber".to_string(), formatted);
                        return;
                    }
                }
                if let Some(value) = extract_string_value(entry, data, ifd_offset, data_base) {
                    tags.insert(
                        "Panasonic:InternalSerialNumber".to_string(),
                        format_internal_serial_number(&value),
                    );
                }
                return;
            }
            // FirmwareVersion: undef; binary versions are rendered as dotted bytes
            0x0002 => {
                if let Some(bytes) = extract_raw_bytes(entry, data, ifd_offset, data_base, byte_order) {
                    tags.insert(
                        "Panasonic:FirmwareVersion".to_string(),
                        format_firmware_version(&bytes),
                    );
                }
                return;
            }
            // MakerNoteVersion: undef bytes shown as text (e.g. "0130"); out-of-line
            // or non-text values keep the raw number, as before this conversion existed
            0x8000 => {
                let printed = if entry.value_count <= 4 {
                    extract_raw_bytes(entry, data, ifd_offset, data_base, byte_order)
                        .and_then(|bytes| undef_bytes_to_string(&bytes))
                } else {
                    None
                };
                tags.insert(
                    "Panasonic:MakerNoteVersion".to_string(),
                    printed.unwrap_or_else(|| entry.value_offset.to_string()),
                );
                return;
            }
            // AFAreaMode: an int8u pair decoded as "a b" (model-conditional for the DMC-FZ10)
            0x000F => {
                let printed =
                    match extract_component_values(entry, data, ifd_offset, data_base, byte_order) {
                        Some(values) => decode_af_area_mode(&values, model),
                        // Value unreachable (out-of-line offset outside this
                        // MakerNote slice): keep the raw field, as before
                        None => format!("Unknown ({})", entry.value_offset),
                    };
                tags.insert("Panasonic:AFAreaMode".to_string(), printed);
                return;
            }
            // TimeSincePowerOn: centiseconds rendered as "[DD days ]HH:MM:SS.ss"
            0x0029 => {
                tags.insert(
                    "Panasonic:TimeSincePowerOn".to_string(),
                    format_time_since_power_on(entry.value_offset),
                );
                return;
            }
            // TravelDay: 65535 means "n/a"
            0x0036 => {
                let value = inline_u16_value(entry, byte_order);
                let printed = if value == 0xFFFF {
                    "n/a".to_string()
                } else {
                    value.to_string()
                };
                tags.insert("Panasonic:TravelDay".to_string(), printed);
                return;
            }
            // Contrast / Saturation / Sharpness: int16s with ExifTool's printParameter
            // conversion (0 => "Normal", positive => "+N", negative => "-N")
            0x0039 | 0x0040 | 0x0041 => {
                let value = inline_u16_value(entry, byte_order) as i16;
                if let Some(tag_name) = registry.get_tag_name(tag_id) {
                    tags.insert(format!("Panasonic:{}", tag_name), print_parameter(value));
                }
                return;
            }
            // FlashBias: int16s thirds of EV rendered as a signed fraction ("0", "+1/3", ...)
            0x0024 => {
                let value = inline_u16_value(entry, byte_order) as i16;
                tags.insert(
                    "Panasonic:FlashBias".to_string(),
                    print_fraction(f64::from(value) / 3.0),
                );
                return;
            }
            // ImageQuality: int16u enum; read the inline SHORT respecting byte order
            // (big-endian files store it in the high half of the value field)
            0x0001 => {
                let value = i32::from(inline_u16_value(entry, byte_order));
                tags.insert(
                    "Panasonic:ImageQuality".to_string(),
                    IMAGE_QUALITY.decode(value),
                );
                return;
            }
            // AFPointPosition: two rational64u values (X, Y AF-area center)
            0x004D => {
                let printed = extract_rational_values(entry, data, ifd_offset, data_base, byte_order)
                    .and_then(|pairs| decode_af_point_position(&pairs));
                if let Some(printed) = printed {
                    tags.insert("Panasonic:AFPointPosition".to_string(), printed);
                }
                return;
            }
            // The newer full-frame MakerNote range uses two encodings that
            // differ from the generic scalar path: LensTypeModel is a SHORT
            // with Panasonic.pm's byte-swapped hex ValueConv, while the two
            // adjustment values are signed rational64 records.
            0x00C5 | 0x00E4 => {
                let raw = inline_u16_value(entry, byte_order);
                if raw != 0 {
                    tags.insert(
                        "Panasonic:LensTypeModel".to_string(),
                        format!("{:02x} {:02x}", raw as u8, (raw >> 8) as u8),
                    );
                }
                return;
            }
            0x00D6 => {
                if let Some(value) = extract_rational_values(entry, data, ifd_offset, data_base, byte_order)
                    .and_then(|values| values.first().copied())
                    .and_then(|(num, den)| format_rational64s(num as i32, den as i32))
                {
                    tags.insert("Panasonic:NoiseReductionStrength".to_string(), value);
                }
                return;
            }
            0x00DE => {
                if let Some(value) = extract_rational_values(entry, data, ifd_offset, data_base, byte_order)
                    .and_then(|values| {
                        values
                            .iter()
                            .map(|&(num, den)| format_rational64u(num, den))
                            .collect::<Option<Vec<_>>>()
                    })
                {
                    tags.insert("Panasonic:AFAreaSize".to_string(), value.join(" "));
                }
                return;
            }
            // InternalNDFilter: plain rational64u, no ValueConv/PrintConv
            // (Panasonic.pm:1247-1250). Count is 1, so the 8-byte value never
            // fits inline in the IFD entry's value_offset field -- it's an
            // out-of-line pointer that must be dereferenced, the same as
            // ClearRetouchValue below. Falling through to the generic
            // registry path (as register_raw did with no match arm here)
            // printed that raw pointer as an integer instead of the
            // numerator/denominator it points to: on
            // `combined-samples/Leica/LeicaD-Lux8.jpg` oxidex read "4816"
            // (the pointer) where ExifTool's `-v3` shows the entry is 0/128
            // ("InternalNDFilter = 0 (0/128)"), i.e. GetRational64u(0, 128).
            0x009D => {
                let printed = extract_rational_values(entry, data, ifd_offset, data_base, byte_order)
                    .and_then(|pairs| pairs.first().copied())
                    .and_then(|(num, den)| format_rational64u(num, den));
                if let Some(printed) = printed {
                    tags.insert("Panasonic:InternalNDFilter".to_string(), printed);
                }
                return;
            }
            // ClearRetouchValue: plain rational64u, no ValueConv/PrintConv
            0x00A3 => {
                let printed = extract_rational_values(entry, data, ifd_offset, data_base, byte_order)
                    .and_then(|pairs| pairs.first().copied())
                    .and_then(|(num, den)| format_rational64u(num, den));
                if let Some(printed) = printed {
                    tags.insert("Panasonic:ClearRetouchValue".to_string(), printed);
                }
                return;
            }
            // HighlightShadow is signed SHORT[2] (Panasonic.pm:1330-1335).
            0x00AD => {
                if let Some(values) = extract_component_values(
                    entry, data, ifd_offset, data_base, byte_order,
                ) {
                    tags.insert(
                        "Panasonic:HighlightShadow".to_string(),
                        values
                            .iter()
                            .map(|&value| (value as u16 as i16).to_string())
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }
                return;
            }
            // PostFocusMerging is LONG[2], whose sole PrintConv recognizes
            // the pair `0 0` (Panasonic.pm:1392-1396).
            0x00BF => {
                if let Some(values) = extract_component_values(
                    entry, data, ifd_offset, data_base, byte_order,
                ) {
                    let printed = if values.as_slice() == [0, 0] {
                        "Post Focus Auto Merging or None".to_string()
                    } else {
                        values
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    tags.insert("Panasonic:PostFocusMerging".to_string(), printed);
                }
                return;
            }
            _ => {}
        }

        // Accelerometer axes and WBShiftAB/WBShiftGM: int16u on the wire, but
        // Format => int16s overrides how the SHORT is interpreted
        // (Panasonic.pm:878-889 WBShiftAB/WBShiftGM, :1170-1187 accelerometer
        // axes). No ValueConv/PrintConv -- the signed value prints as-is. The
        // Format override means the wire field type (SHORT, unsigned) cannot
        // tell the generic fallback below to sign-interpret these, so they
        // stay a hand-picked case like RollAngle/PitchAngle just below.
        if matches!(
            tag_id,
            0x0046 | 0x0047 | 0x008B | 0x008C | 0x008D | 0x008E | 0x00BD
        ) {
            let value = inline_u16_value(entry, byte_order) as i16;
            if let Some(tag_name) = registry.get_tag_name(tag_id) {
                tags.insert(format!("Panasonic:{}", tag_name), value.to_string());
            }
            return;
        }

        // WBShiftCreativeControl's wire field is BYTE but Panasonic.pm reads
        // it as int8s. The generic scalar path cannot infer that override.
        if tag_id == 0x0092 {
            let value = inline_i8_value(entry, byte_order);
            if let Some(tag_name) = registry.get_tag_name(tag_id) {
                tags.insert(format!("Panasonic:{}", tag_name), value.to_string());
            }
            return;
        }

        // RollAngle / PitchAngle: same int16u-wire/int16s-Format override,
        // plus ValueConv '$val/10' (RollAngle, Panasonic.pm:1200-1207) and
        // '-$val/10' (PitchAngle, Panasonic.pm:1208-1215). The negation is
        // done on the integer before dividing so a zero reading prints "0",
        // not "-0".
        if tag_id == 0x0090 || tag_id == 0x0091 {
            let raw = i32::from(inline_u16_value(entry, byte_order) as i16);
            let value = if tag_id == 0x0091 { -raw } else { raw };
            if let Some(tag_name) = registry.get_tag_name(tag_id)
                && let Some(printed) = sprintf_g(f64::from(value) / 10.0, 10)
            {
                tags.insert(format!("Panasonic:{}", tag_name), printed);
            }
            return;
        }

        // Standard registry-based decoding for enumerated and simple integer tags
        if let Some(tag_name) = registry.get_tag_name(tag_id) {
            let value = inline_scalar_i32(entry, byte_order);
            let decoded = registry.decode_i32(tag_id, value);
            tags.insert(format!("Panasonic:{}", tag_name), decoded);
        }
    }
}

/// Byte-order-correct scalar read for the generic registry fallback.
///
/// A count==1 SHORT (unsigned, field type 3) stores its 2-byte value in the
/// first half of the IFD entry's 4-byte value field -- the high half once
/// parsed as a big-endian u32, the low half once parsed as little-endian.
/// Reading the whole `value_offset` unconditionally, as the fallback used to,
/// decodes a big-endian reading of 1 as 65536: on
/// `combined-samples/Panasonic/PanasonicDMC-LC40.jpg`, whose MakerNote
/// directory resolves big-endian, `exiftool -G1 -s` reports
/// `[Panasonic] WhiteBalance : Auto` (tag 0x0003, raw value 1) where the old
/// fallback printed `Unknown (65536)`.
///
/// `MakerNotePanasonic` is `ByteOrder => 'Unknown'` (MakerNotes.pm:733-741),
/// so any tag reached only through this fallback -- not one of the small set
/// of tags with a bespoke Format-override case above -- is exposed to this
/// bug the moment its directory resolves big-endian. Driving the read off
/// `field_type`/`value_count` instead of a hand-maintained tag list means a
/// newly registered int16u/int16s tag is byte-order-correct without anyone
/// having to remember to add it anywhere.
///
/// SSHORT (field type 8) count==1 entries are sign-extended after the same
/// half-selection, matching `inline_u16_value`. Every other field type/count
/// combination -- LONG/SLONG, or any count > 1 -- already fills the whole
/// 4-byte value field, which the IFD parser has already decoded using the
/// directory's byte order, so `value_offset` needs no further adjustment.
fn inline_scalar_i32(entry: &IfdEntry, byte_order: ByteOrder) -> i32 {
    if entry.value_count == 1 && matches!(entry.field_type, 3 | 8) {
        let raw = inline_u16_value(entry, byte_order);
        if entry.field_type == 8 {
            i32::from(raw as i16)
        } else {
            i32::from(raw)
        }
    } else {
        entry.value_offset as i32
    }
}

/// The `%Panasonic::Main` tags whose ExifTool entry is a `SubDirectory` over a
/// `ProcessBinaryData` table, and the table each one selects.
///
/// Both were previously read as scalars: 0x004e under the name `FaceDetection`,
/// which is not a tag `%Panasonic::Main` has at any id, and 0x0061 as a raw
/// `FaceRecInfo` number, which is the offset of the record rather than anything
/// in it. Descending replaces both with the fields ExifTool actually reports.
const fn panasonic_binary_subdir(tag_id: u16) -> Option<&'static BinaryTable> {
    match tag_id {
        0x004E => Some(&PANASONIC_FACEDETINFO),
        0x0061 => Some(&PANASONIC_FACERECINFO),
        0x2003 => Some(&PANASONIC_TIMEINFO),
        _ => None,
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

/// Index into the parser's data slice for an out-of-line value.
///
/// ExifTool resolves a MakerNote value offset against the enclosing TIFF block
/// (`Exif.pm`'s `$base + $valuePtr`), and Panasonic's offsets are measured from
/// the TIFF header: `PanasonicDC-G9.jpg` puts `LensType`'s 34 string bytes at
/// TIFF offset 3414 while the MakerNote payload starts at 1314, and
/// `PanasonicDMC-GH4.jpg`'s `InternalNDFilter`/`ClearRetouchValue` point to
/// TIFF offset 0x113c/0x114c while the payload starts at 0x1370 -- *before*
/// it. When `data_base` is known, `full_data` is the whole enclosing TIFF
/// block and the value sits at `value_offset + data_base.correction` bytes
/// into it directly: no payload-relative arithmetic to underflow on a
/// backward-pointing offset like the GH4's.
///
/// Two reads ExifTool calls "Suspicious" and drops rather than follows
/// (`Exif.pm:6538-6549`), both ported here:
///
/// * A value below TIFF offset 8 addresses the TIFF header itself (the
///   2-byte byte-order mark, the 2-byte magic, the 4-byte first-IFD offset)
///   rather than real tag data -- `$valuePtr < 8 and not
///   $$dirInfo{ZeroOffsetOK}`. `PanasonicDMC-LZ20.jpg`'s
///   `InternalSerialNumber` resolves to TIFF offset 0, landing on the "II*\0"
///   signature itself; read at face value that comes out as the string
///   `"II*"`, a plausible-looking value that is actually the file's own
///   magic bytes.
/// * A value that lands back inside the MakerNote's own entry list is
///   `$valuePtr < $dirEnd and $valuePtr+$size > $dirStart` --
///   `PanasonicDMC-GH4.jpg`'s own `DataDump` points there.
///
/// Both are confirmed against `exiftool -G1 -s -a`, which omits the tag from
/// non-verbose output entirely rather than printing the bytes found; reading
/// [`TiffValueBase::dir_start`]/`dir_end` (or offset 0) at face value would
/// report whatever those unrelated bytes happen to look like as the tag's
/// real value, so both checks apply before any byte is read.
///
/// Without an enclosing block there is nothing to measure from, so the old
/// payload-relative arithmetic is kept rather than guessing a base: see
/// [`MakerNoteContext::payload_tiff_offset`].
fn resolve_value_offset(
    entry: &IfdEntry,
    ifd_offset: usize,
    data_base: Option<TiffValueBase>,
    value_size: usize,
) -> Option<usize> {
    match data_base {
        Some(base) => {
            let value_start = entry.value_offset.checked_add(base.correction)? as usize;
            if value_start < 8
                || value_overlaps_directory(value_start, value_size, base.dir_start, base.dir_end)
            {
                return None;
            }
            Some(value_start)
        }
        None => ifd_offset.checked_add(entry.value_offset as usize),
    }
}

/// Where an out-of-line Panasonic MakerNote value is read from, when the
/// enclosing TIFF block is available. See [`resolve_value_offset`].
#[derive(Clone, Copy)]
struct TiffValueBase {
    /// Added to `entry.value_offset` to get a TIFF-relative index -- 0 for
    /// every body except the DC-FT7, see [`value_offset_correction`].
    correction: u32,
    /// This MakerNote IFD's own entry-list bounds, TIFF-relative.
    dir_start: usize,
    dir_end: usize,
}

/// The correction one body's out-of-line value offsets need before they can
/// be read as TIFF-relative, in bytes.
///
/// One body needs its own number. `MakerNotes.pm` gives the DC-FT7 a dispatch
/// entry of its own, `MakerNotePanasonic3` (MakerNotes.pm:751-761), identical to
/// `MakerNotePanasonic` except for `Base => 12, # crazy!` -- and
/// `MakerNotePanasonic` is written to exclude it explicitly
/// (`$$self{Model} ne "DC-FT7"`, MakerNotes.pm:735). The body writes every
/// out-of-line pointer 12 bytes short, so reading one at face value lands in the
/// tail of the *previous* tag's data. That is not a value that looks wrong: the
/// 42-byte `FaceDetInfo` record read 12 bytes early reports
/// `NumFacePositions = 320`, a number no camera would print but nothing
/// downstream can flag, in place of the 0 ExifTool reports.
///
/// The correction only applies when reading against the TIFF-relative form
/// (`resolve_value_offset`'s `Some` branch). With no enclosing block the
/// offsets are already payload-relative and this is never consulted.
fn value_offset_correction(model: Option<&str>) -> u32 {
    if model == Some("DC-FT7") { 12 } else { 0 }
}

/// Decode `Panasonic::TimeInfo` entry 0 exactly as its ExifTool 13.59
/// `ValueConv`: `unpack "H4H2H2H2H2H2H2"`.  The bytes are packed BCD-like
/// date/time digits; formatting their hexadecimal nibbles is intentional, and
/// not a numeric date conversion.  Its `RawConv` suppresses a record that
/// starts with NUL.
fn decode_panasonic_datetime(record: &[u8], tags: &mut HashMap<String, String>) {
    let Some(bytes) = record.get(..8) else {
        return;
    };
    if bytes[0] == 0 {
        return;
    }
    tags.insert(
        "Panasonic:PanasonicDateTime".to_string(),
        format!(
            "{:02x}{:02x}:{:02x}:{:02x} {:02x}:{:02x}:{:02x}.{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ),
    );
}

/// A `string` value the way ExifTool reads one.
///
/// `Exif.pm` ends a string at its first NUL, and every Panasonic string tag
/// then carries `ValueConv => '$val=~s/ +$//'` -- trailing *spaces* only, and
/// only after the NUL cut.  `PanasonicDMC-G80.jpg`'s `LensType` field is
/// `"LUMIX G VARIO 12-35/F2.8 \0\0\0\0\0\0 \0\0"`: a space follows the NUL run,
/// so trimming NULs from the end and whitespace after that leaves the six NULs
/// embedded in the middle of the value.
fn exiftool_string(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s = std::str::from_utf8(&bytes[..end]).ok()?;
    Some(s.trim_end_matches(' ').to_string())
}

/// Extracts string value from IFD entry
///
/// Handles both inline strings (≤4 bytes) and offset-based strings
fn extract_string_value(
    entry: &IfdEntry,
    full_data: &[u8],
    ifd_offset: usize,
    data_base: Option<TiffValueBase>,
) -> Option<String> {
    let byte_count = entry.value_count as usize;

    // For inline strings (≤4 bytes), value is in value_offset field
    if byte_count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        return exiftool_string(&bytes[0..byte_count]);
    }

    // For longer strings, read from offset
    let abs_offset = resolve_value_offset(entry, ifd_offset, data_base, byte_count)?;

    if abs_offset + byte_count <= full_data.len() {
        return exiftool_string(&full_data[abs_offset..abs_offset + byte_count]);
    }

    None
}

/// Extracts the raw value bytes of an IFD entry (int8u/undef style counts)
///
/// Handles both inline values (≤4 bytes, stored in the value field in the
/// file's byte order) and offset-based values, using the same offset
/// convention as [`extract_string_value`].
fn extract_raw_bytes(
    entry: &IfdEntry,
    full_data: &[u8],
    ifd_offset: usize,
    data_base: Option<TiffValueBase>,
    byte_order: ByteOrder,
) -> Option<Vec<u8>> {
    let byte_count = entry.value_count as usize;

    if byte_count <= 4 {
        let bytes = match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };
        return Some(bytes[0..byte_count].to_vec());
    }

    // For longer values, read from offset (relative to IFD start, as in
    // extract_string_value)
    let abs_offset = resolve_value_offset(entry, ifd_offset, data_base, byte_count)?;
    full_data
        .get(abs_offset..abs_offset + byte_count)
        .map(|b| b.to_vec())
}

/// Numeric value of an entry that is nominally a single int16u/int16s.
///
/// A count==1 SHORT/SSHORT stores its value in the FIRST two bytes of the
/// 4-byte value field, which after parsing the field as a u32 is the high half
/// on big-endian files and the low half on little-endian files. Anything else
/// keeps the low 16 bits of the parsed field (the pre-existing behavior).
fn inline_u16_value(entry: &IfdEntry, byte_order: ByteOrder) -> u16 {
    if entry.value_count == 1 && matches!(entry.field_type, 3 | 8) {
        match byte_order {
            ByteOrder::LittleEndian => (entry.value_offset & 0xFFFF) as u16,
            ByteOrder::BigEndian => (entry.value_offset >> 16) as u16,
        }
    } else {
        (entry.value_offset & 0xFFFF) as u16
    }
}

/// Numeric value of a single inline BYTE/SBYTE, accounting for TIFF byte
/// order just as [`inline_u16_value`] does for SHORT values.
fn inline_i8_value(entry: &IfdEntry, byte_order: ByteOrder) -> i8 {
    let byte = match byte_order {
        ByteOrder::LittleEndian => entry.value_offset as u8,
        ByteOrder::BigEndian => (entry.value_offset >> 24) as u8,
    };
    byte as i8
}

/// Extracts an entry's value as a list of numeric components, honoring the
/// entry's field type: int8u/undef entries yield one component per byte,
/// int16u entries one per 16-bit word. Returns None for unsupported types or
/// unreachable out-of-line values.
fn extract_component_values(
    entry: &IfdEntry,
    full_data: &[u8],
    ifd_offset: usize,
    data_base: Option<TiffValueBase>,
    byte_order: ByteOrder,
) -> Option<Vec<u32>> {
    let count = entry.value_count as usize;
    match entry.field_type {
        // BYTE / UNDEF: one byte per component
        1 | 7 => extract_raw_bytes(entry, full_data, ifd_offset, data_base, byte_order)
            .map(|bytes| bytes.iter().map(|&b| u32::from(b)).collect()),
        // SHORT: two bytes per component
        3 => {
            let byte_len = count.checked_mul(2)?;
            let bytes: Vec<u8> = if byte_len <= 4 {
                let field = match byte_order {
                    ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
                    ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
                };
                field[0..byte_len].to_vec()
            } else {
                let abs_offset = resolve_value_offset(entry, ifd_offset, data_base, byte_len)?;
                full_data.get(abs_offset..abs_offset + byte_len)?.to_vec()
            };
            Some(
                bytes
                    .chunks_exact(2)
                    .map(|c| match byte_order {
                        ByteOrder::LittleEndian => u32::from(u16::from_le_bytes([c[0], c[1]])),
                        ByteOrder::BigEndian => u32::from(u16::from_be_bytes([c[0], c[1]])),
                    })
                    .collect(),
            )
        }
        // LONG: four bytes per component.
        4 => {
            let byte_len = count.checked_mul(4)?;
            let bytes: Vec<u8> = if byte_len <= 4 {
                let field = match byte_order {
                    ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
                    ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
                };
                field[0..byte_len].to_vec()
            } else {
                let abs_offset = resolve_value_offset(entry, ifd_offset, data_base, byte_len)?;
                full_data.get(abs_offset..abs_offset + byte_len)?.to_vec()
            };
            Some(
                bytes
                    .chunks_exact(4)
                    .map(|c| match byte_order {
                        ByteOrder::LittleEndian => u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                        ByteOrder::BigEndian => u32::from_be_bytes([c[0], c[1], c[2], c[3]]),
                    })
                    .collect(),
            )
        }
        _ => None,
    }
}

/// Extracts a `rational64u[count]` entry as `(numerator, denominator)` pairs,
/// honoring byte order. Always out-of-line: 8 bytes per component is never
/// ≤4, so this never hits the inline-value path `extract_raw_bytes` has for
/// smaller types.
fn extract_rational_values(
    entry: &IfdEntry,
    full_data: &[u8],
    ifd_offset: usize,
    data_base: Option<TiffValueBase>,
    byte_order: ByteOrder,
) -> Option<Vec<(u32, u32)>> {
    let count = entry.value_count as usize;
    let byte_len = count.checked_mul(8)?;
    let abs_offset = resolve_value_offset(entry, ifd_offset, data_base, byte_len)?;
    let bytes = full_data.get(abs_offset..abs_offset + byte_len)?;
    Some(
        bytes
            .chunks_exact(8)
            .map(|c| match byte_order {
                ByteOrder::LittleEndian => (
                    u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                    u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                ),
                ByteOrder::BigEndian => (
                    u32::from_be_bytes([c[0], c[1], c[2], c[3]]),
                    u32::from_be_bytes([c[4], c[5], c[6], c[7]]),
                ),
            })
            .collect(),
    )
}

/// `sprintf("%.*g", precision, value)`, ExifTool's `RoundFloat` (ExifTool.pm:5960-5964
/// is exactly this call with precision 10) and the literal `%.2g` in
/// AFPointPosition's PrintConv (Panasonic.pm:924-929). `PanasonicDMC-LX7.jpg`'s
/// AFPointPosition reads out-of-range (its second rational runs into the next
/// tag's bytes) and ExifTool's own %.2g still renders it in scientific
/// notation ("3.7e+02"), so that branch is real, not a theoretical corner.
fn sprintf_g(value: f64, precision: usize) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    if value == 0.0 {
        return Some("0".to_string());
    }
    let precision = precision.max(1);
    let neg = value.is_sign_negative();
    let magnitude = value.abs();

    let sci = format!("{:.*e}", precision - 1, magnitude);
    let (mantissa_str, exp_str) = sci.split_once('e')?;
    let exponent: i32 = exp_str.parse().ok()?;

    let sign = if neg { "-" } else { "" };
    if exponent < -4 || exponent >= precision as i32 {
        let mantissa = trim_trailing_zeros(mantissa_str);
        let exp_sign = if exponent < 0 { '-' } else { '+' };
        return Some(format!("{sign}{mantissa}e{exp_sign}{:02}", exponent.abs()));
    }

    let decimals = (precision as i32 - 1 - exponent).max(0) as usize;
    let fixed = format!("{:.*}", decimals, magnitude);
    let trimmed = trim_trailing_zeros(&fixed);
    Some(format!("{sign}{trimmed}"))
}

/// Strips a trailing `.` and any trailing zeros after it, the `%g` rule that
/// distinguishes it from `%f` (`sprintf_g`'s only caller of this helper cares
/// about the digits, not the string boundaries -- no `#` flag support needed).
fn trim_trailing_zeros(s: &str) -> &str {
    if !s.contains('.') {
        return s;
    }
    s.trim_end_matches('0').trim_end_matches('.')
}

/// `GetRational64u` (ExifTool.pm:6114-6120): `$denom or return $numer ? 'inf' : 'undef'`,
/// otherwise `RoundFloat($numer/$denom, 10)`.
fn format_rational64u(numerator: u32, denominator: u32) -> Option<String> {
    if denominator == 0 {
        return Some(if numerator != 0 { "inf" } else { "undef" }.to_string());
    }
    sprintf_g(f64::from(numerator) / f64::from(denominator), 10)
}

/// Signed counterpart of [`format_rational64u`] for Panasonic's image-style
/// adjustment records, which Panasonic.pm declares `rational64s`.
fn format_rational64s(numerator: i32, denominator: i32) -> Option<String> {
    if denominator == 0 {
        return Some(if numerator != 0 { "inf" } else { "undef" }.to_string());
    }
    sprintf_g(f64::from(numerator) / f64::from(denominator), 10)
}

/// AFPointPosition (tag 0x004D), Panasonic.pm:916-935: two `rational64u`
/// values (X, Y AF-area center, each 0.0-1.0), formatted through
/// ```perl
/// return 'none' if $val eq '16777216 16777216';
/// return 'n/a' if $val =~ /^4194303\.9/;
/// my @a = split ' ', $val;
/// sprintf("%.2g %.2g", @a);
/// ```
/// where `$val` is the two `GetRational64u` results joined by a space.
fn decode_af_point_position(pairs: &[(u32, u32)]) -> Option<String> {
    let [(nx, dx), (ny, dy)] = pairs else {
        return None;
    };
    let sx = format_rational64u(*nx, *dx)?;
    let sy = format_rational64u(*ny, *dy)?;
    let joined = format!("{sx} {sy}");
    if joined == "16777216 16777216" {
        return Some("none".to_string());
    }
    if joined.starts_with("4194303.9") {
        return Some("n/a".to_string());
    }
    let gx = af_point_component_g(*nx, *dx)?;
    let gy = af_point_component_g(*ny, *dy)?;
    Some(format!("{gx} {gy}"))
}

/// One AFPointPosition component through the `%.2g` reformat
/// (Panasonic.pm:924-929's `sprintf("%.2g %.2g", @a)`). Perl's `sprintf`
/// coerces a non-numeric string argument to a number: `"undef"` (this tag's
/// `GetRational64u` zero-denominator, zero-numerator case --
/// `PanasonicDMC-GH3.jpg`'s raw `0/0 0/0`) numifies to 0 with exactly the
/// warning ExifTool emits (`Argument "undef" isn't numeric in sprintf`), and
/// `"inf"` is Perl's own stringification of the special float Infinity,
/// unaffected by `%g`'s precision and rendered `"Inf"`
/// (`PanasonicDMC-FZ200.jpg`'s `8388608/0 0/4294901760` -> `"Inf 0"`).
fn af_point_component_g(numerator: u32, denominator: u32) -> Option<String> {
    if denominator == 0 {
        return Some(if numerator != 0 { "Inf" } else { "0" }.to_string());
    }
    sprintf_g(f64::from(numerator) / f64::from(denominator), 2)
}

/// Renders undef bytes as text, trimming trailing NULs (e.g. MakerNoteVersion "0130")
fn undef_bytes_to_string(bytes: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(bytes).ok()?.trim_end_matches('\0');
    Some(s.to_string())
}

/// FirmwareVersion (0x0002) rendering, from ExifTool Panasonic.pm:
/// ValueConv: `$val=~/[\0-\x2f]/ ? join(" ",unpack("C*",$val)) : $val`
/// PrintConv: `$val=~tr/ /./; $val`
///
/// A value containing any byte <= 0x2F is binary: its bytes are printed as
/// decimal numbers joined with dots (`[0,1,0,0]` -> "0.1.0.0"). Otherwise the
/// value is text, with spaces replaced by dots.
fn format_firmware_version(bytes: &[u8]) -> String {
    if bytes.iter().any(|&b| b <= 0x2F) {
        bytes
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(".")
    } else {
        String::from_utf8_lossy(bytes).replace(' ', ".")
    }
}

/// InternalSerialNumber (0x0025) rendering, from ExifTool Panasonic.pm:
/// `return $val unless $val=~/^([A-Z][0-9A-Z]{2})(\d{2})(\d{2})(\d{2})(\d{4})/;
///  my $yr = $2 + ($2 < 70 ? 2000 : 1900);
///  return "($1) $yr:$3:$4 no. $5";`
fn format_internal_serial_number(value: &str) -> String {
    let b = value.as_bytes();
    let matches = b.len() >= 13
        && b[0].is_ascii_uppercase()
        && b[1..3]
            .iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && b[3..13].iter().all(|c| c.is_ascii_digit());
    if !matches {
        return value.to_string();
    }
    let yy: u32 = value[3..5].parse().unwrap_or(0);
    let year = yy + if yy < 70 { 2000 } else { 1900 };
    format!(
        "({}) {}:{}:{} no. {}",
        &value[0..3],
        year,
        &value[5..7],
        &value[7..9],
        &value[9..13]
    )
}

/// BabyAge (0x0033) rendering, from ExifTool Panasonic.pm:
/// `$val eq "9999:99:99 00:00:00" ? "(not set)" : $val`
fn format_baby_age(value: &str) -> String {
    if value == "9999:99:99 00:00:00" {
        "(not set)".to_string()
    } else {
        value.to_string()
    }
}

/// TimeSincePowerOn (0x0029) rendering, from ExifTool Panasonic.pm: the raw
/// value counts 1/100 s; printed as "[DD days ]HH:MM:SS.ss" (63308 -> "00:10:33.08")
fn format_time_since_power_on(centiseconds: u32) -> String {
    let mut cs = u64::from(centiseconds);
    let mut prefix = String::new();
    const DAY_CS: u64 = 24 * 3600 * 100;
    if cs >= DAY_CS {
        let days = cs / DAY_CS;
        prefix = format!("{} days ", days);
        cs -= days * DAY_CS;
    }
    let hours = cs / 360_000;
    cs %= 360_000;
    let minutes = cs / 6_000;
    cs %= 6_000;
    format!(
        "{}{:02}:{:02}:{:02}.{:02}",
        prefix,
        hours,
        minutes,
        cs / 100,
        cs % 100
    )
}

/// ExifTool's Image::ExifTool::Exif::PrintFraction, used for FlashBias:
/// prints a signed value in thirds ("0", "+1", "+1/3", "-2/3", ...)
fn print_fraction(value: f64) -> String {
    crate::core::formatters::exif_print_conv::print_fraction(value)
}

/// ExifTool's %Image::ExifTool::Exif::printParameter conversion, used for
/// Contrast/Saturation/Sharpness: 0 => "Normal", positive => "+N", negative => "-N"
fn print_parameter(value: i16) -> String {
    if value == 0 {
        "Normal".to_string()
    } else if value > 0 {
        format!("+{}", value)
    } else {
        value.to_string()
    }
}

/// AFAreaMode (0x000F) int8u pair table from ExifTool Panasonic.pm ("other
/// models" variant), keyed as "first second"
const AF_AREA_MODE_PAIRS: &[(u32, u32, &str)] = &[
    (0, 1, "9-area"),
    (0, 16, "3-area (high speed)"),
    (0, 23, "23-area"),
    (0, 49, "49-area"),
    (0, 225, "225-area"),
    (1, 0, "Spot Focusing"),
    (1, 1, "5-area"),
    (16, 0, "1-area"),
    (16, 16, "1-area (high speed)"),
    (16, 32, "1-area +"),
    (16, 225, "225-area 2"),
    (17, 0, "Full Area"),
    (32, 0, "Tracking"),
    (32, 1, "3-area (left)?"),
    (32, 2, "3-area (center)?"),
    (32, 3, "3-area (right)?"),
    (32, 16, "Zone"),
    (32, 18, "Zone (horizontal/vertical)"),
    (64, 0, "Face Detect"),
    (64, 1, "Face Detect (animal detect on)"),
    (64, 2, "Face Detect (animal detect off)"),
    (128, 0, "Pinpoint focus"),
    (240, 0, "Tracking"),
];

/// Decodes AFAreaMode (0x000F), an int8u pair, per ExifTool Panasonic.pm.
///
/// The DMC-FZ10 uses its own two-entry table (condition
/// `$$self{Model} =~ /DMC-FZ10\b/`); every other model uses the general table.
/// A one-value entry decodes via the single '16' => 'Normal?' mapping. Values
/// of any other shape miss the table and are shown ExifTool-style as
/// "Unknown (a b ...)".
fn decode_af_area_mode(values: &[u32], model: Option<&str>) -> String {
    let unknown = || {
        format!(
            "Unknown ({})",
            values
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    if is_dmc_fz10(model) {
        return match values {
            [0, 1] => "Spot Mode On".to_string(),
            [0, 16] => "Spot Mode Off".to_string(),
            _ => unknown(),
        };
    }
    match values {
        // (only mode for DMC-LC20)
        [16] => "Normal?".to_string(),
        [a, b] => AF_AREA_MODE_PAIRS
            .iter()
            .find(|(pa, pb, _)| pa == a && pb == b)
            .map(|(_, _, name)| (*name).to_string())
            .unwrap_or_else(unknown),
        _ => unknown(),
    }
}

/// Matches ExifTool's `/DMC-FZ10\b/` model condition: "DMC-FZ10" followed by a
/// word boundary (so DMC-FZ100 and DC-FZ10002 do not match)
fn is_dmc_fz10(model: Option<&str>) -> bool {
    let Some(model) = model else { return false };
    let needle = "DMC-FZ10";
    let mut start = 0;
    while let Some(pos) = model[start..].find(needle) {
        let end = start + pos + needle.len();
        let boundary = model[end..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        if boundary {
            return true;
        }
        start = end;
    }
    false
}

#[cfg(test)]
mod byte_order_tests {
    use super::*;

    /// One-entry Panasonic MakerNote holding `ImageQuality = 2` ("High"),
    /// written in `order`. The header is the 12-byte "Panasonic\0\0\0" the
    /// real cameras write.
    fn makernote(order: ByteOrder) -> Vec<u8> {
        let mut d = b"Panasonic\0\0\0".to_vec();
        let (u16b, u32b): (fn(u16) -> [u8; 2], fn(u32) -> [u8; 4]) = match order {
            ByteOrder::LittleEndian => (u16::to_le_bytes, u32::to_le_bytes),
            ByteOrder::BigEndian => (u16::to_be_bytes, u32::to_be_bytes),
        };
        d.extend_from_slice(&u16b(1)); // entry count
        d.extend_from_slice(&u16b(0x0001)); // ImageQuality
        d.extend_from_slice(&u16b(3)); // int16u
        d.extend_from_slice(&u32b(1)); // count 1
        // A count==1 SHORT lives in the first two bytes of the value field.
        d.extend_from_slice(&u16b(2));
        d.extend_from_slice(&[0, 0]);
        d.extend_from_slice(&u32b(0)); // next-IFD pointer
        d
    }

    fn parse(
        data: &[u8],
        outer: ByteOrder,
    ) -> std::result::Result<HashMap<String, String>, String> {
        let mut tags = HashMap::new();
        PanasonicParser.parse(data, outer, &mut tags)?;
        Ok(tags)
    }

    /// Control: when the MakerNote and the enclosing TIFF agree, nothing moves.
    /// This is the case for the 300-odd Panasonic files that already worked,
    /// and the conservative predicate must leave them exactly as they were.
    #[test]
    fn matching_byte_order_is_left_alone() {
        for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let tags = parse(&makernote(order), order).unwrap();
            assert_eq!(
                tags.get("Panasonic:ImageQuality").map(String::as_str),
                Some("High"),
                "control failed for {order:?}"
            );
        }
    }

    /// `MakerNotePanasonic` is `ByteOrder => 'Unknown'`, so the directory may be
    /// written the other way round from the file that contains it. Ten of the
    /// eleven affected corpus files are this case: a big-endian ("MM") TIFF
    /// carrying a little-endian MakerNote.
    #[test]
    fn little_endian_makernote_in_big_endian_file_is_read() {
        let tags = parse(&makernote(ByteOrder::LittleEndian), ByteOrder::BigEndian).unwrap();
        assert_eq!(
            tags.get("Panasonic:ImageQuality").map(String::as_str),
            Some("High"),
        );
    }

    /// The mirror case, which `PanasonicDMC-LC5.jpg` is: an "II" file carrying
    /// a big-endian MakerNote. The resolved order has to reach the *values*
    /// too, not just the entry layout -- ExifTool does this with `SetByteOrder`
    /// (Exif.pm:7078) -- so a count==1 SHORT must still be read out of the high
    /// half of its value field here.
    #[test]
    fn big_endian_makernote_in_little_endian_file_is_read() {
        let tags = parse(&makernote(ByteOrder::BigEndian), ByteOrder::LittleEndian).unwrap();
        assert_eq!(
            tags.get("Panasonic:ImageQuality").map(String::as_str),
            Some("High"),
        );
    }

    /// A directory that cannot be read is reported. It must not be fatal --
    /// ExifTool warns and reads the rest of the file -- but returning `Ok(())`
    /// made it invisible, which is indistinguishable from a file that has no
    /// MakerNote at all and is why this whole class went uncounted.
    #[test]
    fn unreadable_directory_is_reported_not_swallowed() {
        let mut d = b"Panasonic\0\0\0".to_vec();
        // 0x2020 = 8224: high byte equals the low byte, so ExifTool's predicate
        // does NOT swap (it needs high > low). The count is simply wrong, and
        // 8224 entries do not fit in the 24 bytes that follow.
        d.extend_from_slice(&0x2020u16.to_le_bytes());
        d.extend_from_slice(&[0u8; 24]);

        let err = parse(&d, ByteOrder::LittleEndian).unwrap_err();
        assert!(err.contains("8224"), "error must name the bad count: {err}");
        assert!(
            err.contains("Panasonic"),
            "error must name the directory: {err}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_image_quality() {
        assert_eq!(IMAGE_QUALITY.decode(2), "High");
        assert_eq!(IMAGE_QUALITY.decode(3), "Normal");
        assert_eq!(IMAGE_QUALITY.decode(7), "RAW");
        assert_eq!(IMAGE_QUALITY.decode(12), "4k Movie");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(WHITE_BALANCE.decode(1), "Auto");
        assert_eq!(WHITE_BALANCE.decode(2), "Daylight");
        assert_eq!(WHITE_BALANCE.decode(13), "Kelvin");
    }

    // Every string asserted below was read out of ExifTool's own PrintConv
    // hash, dumped from the Perl symbol table (ExifTool 13.59). The ids are
    // the ones oxidex and ExifTool used to disagree on, so reverting a table
    // fails the test rather than passing it.

    /// Panasonic.pm:290-302. The AF-x labels sit at 6/7/8, not 4/5/6, and 4
    /// and 5 are the two "Auto," variants. There is no entry at 16 at all.
    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(FOCUS_MODE.decode(1), "Auto");
        assert_eq!(FOCUS_MODE.decode(2), "Manual");
        assert_eq!(FOCUS_MODE.decode(4), "Auto, Focus button");
        assert_eq!(FOCUS_MODE.decode(5), "Auto, Continuous");
        assert_eq!(FOCUS_MODE.decode(6), "AF-S");
        assert_eq!(FOCUS_MODE.decode(7), "AF-C");
        assert_eq!(FOCUS_MODE.decode(8), "AF-F");
        assert_eq!(FOCUS_MODE.decode(16), "Unknown (16)");
    }

    #[test]
    fn test_decode_film_mode() {
        assert_eq!(FILM_MODE.decode(0), "n/a");
        assert_eq!(FILM_MODE.decode(1), "Standard (color)");
        assert_eq!(FILM_MODE.decode(5), "Standard (B&W)");
        assert_eq!(FILM_MODE.decode(11), "Vibrant");
    }

    #[test]
    fn test_decode_image_stabilization() {
        assert_eq!(IMAGE_STABILIZATION.decode(2), "On, Optical");
        assert_eq!(IMAGE_STABILIZATION.decode(4), "On, Mode 2");
        assert_eq!(IMAGE_STABILIZATION.decode(9), "Dual IS");
        assert_eq!(IMAGE_STABILIZATION.decode(34), "Unknown (34)");
    }

    #[test]
    fn test_decode_rotation() {
        assert_eq!(ROTATION.decode(1), "Horizontal (normal)");
        assert_eq!(ROTATION.decode(3), "Rotate 180");
        assert_eq!(ROTATION.decode(6), "Rotate 90 CW");
        assert_eq!(ROTATION.decode(8), "Rotate 270 CW");
    }

    #[test]
    fn test_decode_self_timer() {
        assert_eq!(SELF_TIMER.decode(1), "Off");
        assert_eq!(SELF_TIMER.decode(2), "10 s");
        assert_eq!(SELF_TIMER.decode(4), "10 s / 3 pictures");
        assert_eq!(SELF_TIMER.decode(778), "3 photos after 10 s");
    }

    #[test]
    fn test_decode_color_mode() {
        assert_eq!(COLOR_MODE.decode(0), "Normal");
        assert_eq!(COLOR_MODE.decode(1), "Natural");
        assert_eq!(COLOR_MODE.decode(2), "Vivid");
    }

    #[test]
    fn test_format_firmware_version() {
        // Binary version bytes are joined with dots (Panasonic.rw2 / Panasonic.jpg)
        assert_eq!(format_firmware_version(&[0, 1, 0, 0]), "0.1.0.0");
        assert_eq!(format_firmware_version(&[0, 1, 0, 8]), "0.1.0.8");
        // Pure text stays text (no byte <= 0x2F)
        assert_eq!(format_firmware_version(b"0130"), "0130");
    }

    #[test]
    fn test_format_internal_serial_number() {
        // Panasonic.rw2's serial
        assert_eq!(
            format_internal_serial_number("F350807010058"),
            "(F35) 2008:07:01 no. 0058"
        );
        // Panasonic.jpg's serial (digits allowed in the series code)
        assert_eq!(
            format_internal_serial_number("S000407190102"),
            "(S00) 2004:07:19 no. 0102"
        );
        // Trailing junk after the 13 significant characters is ignored
        assert_eq!(
            format_internal_serial_number("F350807010058\u{10}"),
            "(F35) 2008:07:01 no. 0058"
        );
        // Two-digit years >= 70 are 19xx
        assert_eq!(
            format_internal_serial_number("XYZ9901011234"),
            "(XYZ) 1999:01:01 no. 1234"
        );
        // Non-matching values pass through unchanged
        assert_eq!(format_internal_serial_number("hello"), "hello");
        assert_eq!(format_internal_serial_number(""), "");
    }

    #[test]
    fn test_format_baby_age() {
        assert_eq!(format_baby_age("9999:99:99 00:00:00"), "(not set)");
        assert_eq!(
            format_baby_age("2008:07:01 12:00:00"),
            "2008:07:01 12:00:00"
        );
    }

    #[test]
    fn test_format_time_since_power_on() {
        // Panasonic.rw2: 63308 cs = 10 min 33.08 s
        assert_eq!(format_time_since_power_on(63308), "00:10:33.08");
        // Panasonic.jpg: 696 cs = 6.96 s
        assert_eq!(format_time_since_power_on(696), "00:00:06.96");
        assert_eq!(format_time_since_power_on(0), "00:00:00.00");
        // Day rollover keeps ExifTool's "N days " prefix
        assert_eq!(
            format_time_since_power_on(24 * 3600 * 100 + 123_456),
            "1 days 00:20:34.56"
        );
    }

    #[test]
    fn test_print_fraction() {
        assert_eq!(print_fraction(0.0), "0");
        assert_eq!(print_fraction(3.0 / 3.0), "+1");
        assert_eq!(print_fraction(1.0 / 3.0), "+1/3");
        assert_eq!(print_fraction(2.0 / 3.0), "+2/3");
        assert_eq!(print_fraction(-3.0 / 3.0), "-1");
        assert_eq!(print_fraction(-1.0 / 3.0), "-1/3");
        assert_eq!(print_fraction(1.5), "+3/2");
    }

    #[test]
    fn test_print_parameter() {
        assert_eq!(print_parameter(0), "Normal");
        assert_eq!(print_parameter(1), "+1");
        assert_eq!(print_parameter(-1), "-1");
    }

    #[test]
    fn test_decode_af_area_mode() {
        // Panasonic.rw2: '16 0'
        assert_eq!(decode_af_area_mode(&[16, 0], None), "1-area");
        // Panasonic.jpg: '0 1'
        assert_eq!(decode_af_area_mode(&[0, 1], None), "9-area");
        assert_eq!(decode_af_area_mode(&[0, 49], None), "49-area");
        assert_eq!(decode_af_area_mode(&[16], None), "Normal?");
        assert_eq!(decode_af_area_mode(&[99, 99], None), "Unknown (99 99)");
        // DMC-FZ10 uses its own table
        assert_eq!(
            decode_af_area_mode(&[0, 16], Some("DMC-FZ10")),
            "Spot Mode Off"
        );
        assert_eq!(
            decode_af_area_mode(&[0, 1], Some("DMC-FZ10")),
            "Spot Mode On"
        );
        // The FZ10 condition is a word-boundary match, so FZ100 is not FZ10
        assert_eq!(
            decode_af_area_mode(&[0, 16], Some("DMC-FZ100")),
            "3-area (high speed)"
        );
        // SHORT-typed pairs keep their 16-bit components (DMC-LZ20)
        assert_eq!(decode_af_area_mode(&[16384, 0], None), "Unknown (16384 0)");
    }

    #[test]
    fn test_inline_u16_value() {
        // Little-endian SHORT count 1: value in the low half
        let le = IfdEntry {
            tag_id: 0x01,
            field_type: 3,
            value_count: 1,
            value_offset: 0x0000_0002,
        };
        assert_eq!(inline_u16_value(&le, ByteOrder::LittleEndian), 2);
        // Big-endian SHORT count 1: value in the high half (DMC-LC20 ImageQuality)
        let be = IfdEntry {
            tag_id: 0x01,
            field_type: 3,
            value_count: 1,
            value_offset: 0x0002_0000,
        };
        assert_eq!(inline_u16_value(&be, ByteOrder::BigEndian), 2);
    }

    #[test]
    fn test_undef_bytes_to_string() {
        assert_eq!(undef_bytes_to_string(b"0130"), Some("0130".to_string()));
        assert_eq!(undef_bytes_to_string(b"0131\0\0"), Some("0131".to_string()));
    }

    /// Panasonic.pm %shootingMode. The table used to stop at 20 with three
    /// invented labels there; ExifTool runs to 92 and calls 18/19/20
    /// Fireworks/Party/Snow.
    #[test]
    fn test_decode_shooting_mode() {
        assert_eq!(SHOOTING_MODE.decode(6), "Program");
        assert_eq!(SHOOTING_MODE.decode(7), "Aperture Priority");
        assert_eq!(SHOOTING_MODE.decode(11), "Manual");
        assert_eq!(SHOOTING_MODE.decode(18), "Fireworks");
        assert_eq!(SHOOTING_MODE.decode(19), "Party");
        assert_eq!(SHOOTING_MODE.decode(20), "Snow");
        assert_eq!(SHOOTING_MODE.decode(37), "Intelligent Auto");
        assert_eq!(SHOOTING_MODE.decode(60), "Intelligent Auto Plus");
        assert_eq!(SHOOTING_MODE.decode(62), "Panorama");
        assert_eq!(SHOOTING_MODE.decode(92), "Handheld Night Shot");
    }

    /// Panasonic.pm:1611-1622: the EV steps are 100/200/300 and the Auto
    /// variants are 0x8064-based, not 1/2/3/100.
    #[test]
    fn test_decode_hdr() {
        assert_eq!(HDR.decode(0), "Off");
        assert_eq!(HDR.decode(100), "1 EV");
        assert_eq!(HDR.decode(200), "2 EV");
        assert_eq!(HDR.decode(32968), "2 EV (Auto)");
        assert_eq!(HDR.decode(1), "Unknown (1)");
    }

    /// Panasonic.pm:1329-1345. Every id shifted by one against ExifTool's --
    /// oxidex called 0 "Standard" where ExifTool calls it "Auto", and so on
    /// down the table, so a PhotoStyle was almost never reported correctly.
    #[test]
    fn test_decode_photo_style() {
        assert_eq!(PHOTO_STYLE.decode(0), "Auto");
        assert_eq!(PHOTO_STYLE.decode(1), "Standard or Custom");
        assert_eq!(PHOTO_STYLE.decode(2), "Vivid");
        assert_eq!(PHOTO_STYLE.decode(4), "Monochrome");
        assert_eq!(PHOTO_STYLE.decode(11), "L. Monochrome");
        assert_eq!(PHOTO_STYLE.decode(12), "Like709");
        assert_eq!(PHOTO_STYLE.decode(17), "V-Log");
        assert_eq!(PHOTO_STYLE.decode(7), "Unknown (7)");
    }

    /// Panasonic.pm:672-676 reads 1 => Off, 2 => On. The two labels used to
    /// be the other way round, so every long exposure reported the opposite
    /// of what the camera recorded.
    #[test]
    fn test_long_exposure_nr_is_not_inverted() {
        assert_eq!(LONG_EXPOSURE_NR.decode(1), "Off");
        assert_eq!(LONG_EXPOSURE_NR.decode(2), "On");
    }

    /// Panasonic.pm:411-431 (the non-GF/G2/TZ10 variant).
    #[test]
    fn test_decode_contrast_mode() {
        assert_eq!(CONTRAST_MODE.decode(0), "Normal");
        assert_eq!(CONTRAST_MODE.decode(5), "Normal 2");
        assert_eq!(CONTRAST_MODE.decode(6), "Medium Low");
        assert_eq!(CONTRAST_MODE.decode(7), "Medium High");
        assert_eq!(CONTRAST_MODE.decode(272), "Normal");
        assert_eq!(CONTRAST_MODE.decode(3), "Unknown (3)");
    }

    /// Panasonic.pm:398-410: 1 is a plain "On".
    #[test]
    fn test_decode_burst_mode() {
        assert_eq!(BURST_MODE.decode(0), "Off");
        assert_eq!(BURST_MODE.decode(1), "On");
        assert_eq!(BURST_MODE.decode(2), "Auto Exposure Bracketing (AEB)");
        assert_eq!(BURST_MODE.decode(3), "Focus Bracketing");
        assert_eq!(BURST_MODE.decode(17), "On (with flash)");
    }

    /// Panasonic.pm:265-273 has the two zoom-macro values as well.
    #[test]
    fn test_decode_macro_mode() {
        assert_eq!(MACRO_MODE.decode(1), "On");
        assert_eq!(MACRO_MODE.decode(2), "Off");
        assert_eq!(MACRO_MODE.decode(257), "Tele-Macro");
        assert_eq!(MACRO_MODE.decode(513), "Macro Zoom");
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
    fn test_is_panasonic_makernote() {
        let valid_data = b"Panasonic\0\0\0some_data";
        assert!(is_panasonic_makernote(valid_data));

        let invalid_data = b"Nikon\0\0\0";
        assert!(!is_panasonic_makernote(invalid_data));
    }

    /// 0x004e and 0x0061 must select a table, and every other id must not --
    /// binding one to a neighbour would report a real ExifTool name over the
    /// wrong record.
    #[test]
    fn test_only_the_two_subdirectory_tags_select_a_table() {
        assert!(panasonic_binary_subdir(0x004E).is_some());
        assert!(panasonic_binary_subdir(0x0061).is_some());
        for tag in [0x004Du16, 0x004F, 0x0060, 0x0062, 0x003F] {
            assert!(
                panasonic_binary_subdir(tag).is_none(),
                "tag {tag:#06x} must not descend"
            );
        }
    }

    /// `MakerNotePanasonic3` applies to the DC-FT7 and to nothing else --
    /// correcting any other body's offsets would move every out-of-line value.
    #[test]
    fn test_only_the_dc_ft7_gets_a_value_offset_correction() {
        assert_eq!(value_offset_correction(Some("DC-FT7")), 12);
        assert_eq!(value_offset_correction(Some("DC-S1")), 0);
        assert_eq!(value_offset_correction(None), 0);
    }

    /// `sprintf("%.*g", precision, value)` (`RoundFloat` and AFPointPosition's
    /// PrintConv both call this). Panasonic.pm:1200-1215 / :924-929.
    #[test]
    fn test_sprintf_g() {
        assert_eq!(sprintf_g(0.0, 10).unwrap(), "0");
        assert_eq!(sprintf_g(-1.0, 10).unwrap(), "-1");
        assert_eq!(sprintf_g(44.1, 10).unwrap(), "44.1");
        assert_eq!(sprintf_g(16777216.0, 10).unwrap(), "16777216");
        // 4294967295/1024, RoundFloat'd to 10 sig figs -- the AFPointPosition
        // "n/a" sentinel is a prefix match on exactly this string.
        assert_eq!(sprintf_g(4294967295.0 / 1024.0, 10).unwrap(), "4194303.999");
        assert_eq!(sprintf_g(0.5, 2).unwrap(), "0.5");
        // Scientific-notation branch (exponent >= precision or < -4):
        // combined-samples/Panasonic/PanasonicDMC-LX7.jpg's AFPointPosition
        // reads out of the documented 0.0-1.0 range (its second rational
        // runs into the next tag's bytes), and ExifTool's own %.2g still
        // renders that in scientific notation.
        assert_eq!(sprintf_g(130.0, 2).unwrap(), "1.3e+02");
        assert_eq!(sprintf_g(365.7209298, 2).unwrap(), "3.7e+02");
        assert_eq!(sprintf_g(0.000_012_34, 2).unwrap(), "1.2e-05");
        assert_eq!(sprintf_g(-0.000_012_34, 2).unwrap(), "-1.2e-05");
    }

    /// `GetRational64u` (ExifTool.pm:6114-6120): a zero denominator is
    /// "inf" if the numerator is nonzero, "undef" if it's also zero.
    #[test]
    fn test_format_rational64u() {
        assert_eq!(format_rational64u(0, 0).unwrap(), "undef");
        assert_eq!(format_rational64u(5, 0).unwrap(), "inf");
        assert_eq!(format_rational64u(128, 256).unwrap(), "0.5");
        assert_eq!(format_rational64u(0, 1).unwrap(), "0");
    }

    /// AFPointPosition (Panasonic.pm:916-935): the two sentinel raw values
    /// and the ordinary %.2g-formatted case, all keyed on the exact
    /// `GetRational64u`-rounded string the real PrintConv branches on.
    #[test]
    fn test_decode_af_point_position() {
        // combined-samples/Leica/LeicaD-Lux7.jpg: 128/256 128/256 -> "0.5 0.5"
        assert_eq!(
            decode_af_point_position(&[(128, 256), (128, 256)]).unwrap(),
            "0.5 0.5"
        );
        // 16777216/1 both components is the documented "none" sentinel.
        assert_eq!(
            decode_af_point_position(&[(16_777_216, 1), (16_777_216, 1)]).unwrap(),
            "none"
        );
        // 4294967295/1024 is the documented "n/a" sentinel (rounds to a
        // string starting "4194303.9").
        assert_eq!(
            decode_af_point_position(&[(4_294_967_295, 1024), (4_294_967_295, 1024)]).unwrap(),
            "n/a"
        );
        // combined-samples/Panasonic/PanasonicDMC-GH3.jpg: raw 0/0 0/0.
        // Neither component is a defined sentinel, so the %.2g reformat
        // runs on two GetRational64u "undef" strings, which Perl's sprintf
        // numifies to 0.
        assert_eq!(decode_af_point_position(&[(0, 0), (0, 0)]).unwrap(), "0 0");
        // combined-samples/Panasonic/PanasonicDMC-FZ200.jpg: 8388608/0 0/4294901760.
        // The first component is "inf" (nonzero numerator, zero
        // denominator), which Perl's sprintf renders "Inf" regardless of
        // the %.2g precision; the second is the "undef" case above.
        assert_eq!(
            decode_af_point_position(&[(8_388_608, 0), (0, 4_294_901_760)]).unwrap(),
            "Inf 0"
        );
    }

    /// `combined-samples/Leica/LeicaD-Lux7.jpg`'s MakerNote, byte-for-byte:
    /// AFPointPosition's 16 bytes from `exiftool -v3`, InternalNDFilter's
    /// 0/128 and ClearRetouchValue's 0/0 (both also `exiftool -v3`), and the
    /// accelerometer/orientation/angle SHORTs/BYTE built from the signed
    /// values `exiftool -G1 -s` reports for the same file. LeicaD-Lux8.jpg's
    /// InternalNDFilter is the identical 8 bytes (`exiftool -v3`:
    /// `InternalNDFilter = 0 (0/128)` at both tag 0x009d entries), so this
    /// fixture also stands in for that sample.
    #[test]
    fn test_matches_exiftool_on_leica_d_lux7_bytes() {
        let mut data = Vec::new();
        data.extend_from_slice(PANASONIC_HEADER); // 12 bytes, ifd_offset == 12
        data.extend_from_slice(&9u16.to_le_bytes()); // entry_count

        // Each entry: tag_id, field_type, value_count, value_offset (all LE).
        // Out-of-line offsets are relative to ifd_offset (12), per
        // resolve_value_offset's data_base=None branch.
        let entries_start = 14usize; // ifd_offset(12) + entry_count field(2)
        let entry_bytes = 9 * 12;
        let af_point_offset = entries_start + entry_bytes; // absolute
        let internal_nd_filter_offset = af_point_offset + 16; // absolute
        let clear_retouch_offset = internal_nd_filter_offset + 8; // absolute

        let mut push_entry = |tag_id: u16, field_type: u16, count: u32, value: u32| {
            data.extend_from_slice(&tag_id.to_le_bytes());
            data.extend_from_slice(&field_type.to_le_bytes());
            data.extend_from_slice(&count.to_le_bytes());
            data.extend_from_slice(&value.to_le_bytes());
        };
        push_entry(0x004D, 5, 2, (af_point_offset - 12) as u32); // AFPointPosition
        push_entry(0x008D, 3, 1, 0xFFFD); // AccelerometerX: -3
        push_entry(0x008E, 3, 1, 0xFF4E); // AccelerometerY: -178
        push_entry(0x008C, 3, 1, 183); // AccelerometerZ: 183
        push_entry(0x008F, 1, 1, 0); // CameraOrientation: Normal
        push_entry(0x0090, 3, 1, 0xFFF6); // RollAngle: raw -10 -> -1
        push_entry(0x0091, 3, 1, 0xFE47); // PitchAngle: raw -441 -> 44.1
        push_entry(0x009D, 5, 1, (internal_nd_filter_offset - 12) as u32); // InternalNDFilter
        push_entry(0x00A3, 5, 1, (clear_retouch_offset - 12) as u32); // ClearRetouchValue

        // AFPointPosition's rational64u[2]: 128/256, 128/256 (0.5, 0.5).
        data.extend_from_slice(&[
            0x80, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x01,
            0x00, 0x00,
        ]);
        // InternalNDFilter's rational64u: 0/128 -> "0" (real LeicaD-Lux7.jpg
        // and LeicaD-Lux8.jpg bytes, `exiftool -v3`).
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00]);
        // ClearRetouchValue's rational64u: 0/0 -> "undef".
        data.extend_from_slice(&[0u8; 8]);

        let mut tags = HashMap::new();
        parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);

        assert_eq!(tags.get("Panasonic:AFPointPosition").unwrap(), "0.5 0.5");
        assert_eq!(tags.get("Panasonic:InternalNDFilter").unwrap(), "0");
        assert_eq!(tags.get("Panasonic:AccelerometerX").unwrap(), "-3");
        assert_eq!(tags.get("Panasonic:AccelerometerY").unwrap(), "-178");
        assert_eq!(tags.get("Panasonic:AccelerometerZ").unwrap(), "183");
        assert_eq!(tags.get("Panasonic:CameraOrientation").unwrap(), "Normal");
        assert_eq!(tags.get("Panasonic:RollAngle").unwrap(), "-1");
        assert_eq!(tags.get("Panasonic:PitchAngle").unwrap(), "44.1");
        assert_eq!(tags.get("Panasonic:ClearRetouchValue").unwrap(), "undef");
    }

    /// MergedImages (0x0076) is `Writable => 'int16u'` with no PrintConv
    /// (Panasonic.pm:1089-1093), so ExifTool prints the SHORT itself.
    ///
    /// `exiftool -a -G1 -s -MergedImages -BurstSpeed -ExifByteOrder` on the
    /// pinned 13.59 reports, for `combined-samples/Leica/LeicaD-LUX6.jpg`:
    ///
    /// ```text
    /// [Panasonic]     MergedImages                    : 3
    /// [Panasonic]     BurstSpeed                      : 0
    /// [File]          ExifByteOrder                   : Little-endian (Intel, II)
    /// ```
    ///
    /// and `-v3` on the same file shows the entry it comes from:
    ///
    /// ```text
    /// 71) MergedImages = 3
    ///     - Tag 0x0076 (2 bytes, int16u[1]):
    ///         083a: 03 00                                           [..]
    /// ```
    ///
    /// The reading must not be mistaken for an enum: "3" is a count of merged
    /// frames, and the neighbouring BurstSpeed proves a zero SHORT prints "0"
    /// rather than being suppressed.
    #[test]
    fn merged_images_prints_the_raw_int16u_count() {
        // Little-endian, as LeicaD-LUX6.jpg stores it: 0x0076, int16u[1], 3.
        let mut data = Vec::new();
        data.extend_from_slice(PANASONIC_HEADER);
        data.extend_from_slice(&2u16.to_le_bytes()); // entry_count
        for (tag_id, value) in [(0x0076u16, 3u32), (0x0077, 0)] {
            data.extend_from_slice(&tag_id.to_le_bytes());
            data.extend_from_slice(&3u16.to_le_bytes()); // int16u
            data.extend_from_slice(&1u32.to_le_bytes()); // count
            data.extend_from_slice(&value.to_le_bytes());
        }

        let mut tags = HashMap::new();
        parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
        assert_eq!(tags.get("Panasonic:MergedImages").unwrap(), "3");
        assert_eq!(tags.get("Panasonic:BurstSpeed").unwrap(), "0");
    }

    /// A big-endian inline SHORT lives in the *high* half of the 4-byte value
    /// field, so reading `value_offset` as a u32 would report 3 as 196608.
    ///
    /// `MakerNotePanasonic` is `ByteOrder => 'Unknown'` (MakerNotes.pm:733-741),
    /// so the directory's endianness is resolved from its entry count rather
    /// than inherited from the enclosing TIFF: nine corpus files carrying
    /// MergedImages have big-endian EXIF, yet every one of their *MakerNote*
    /// directories resolves little-endian. `combined-samples/Panasonic/
    /// PanasonicDMC-GH3.jpg` is the clearest case -- pinned 13.59 reports
    ///
    /// ```text
    /// [Panasonic]     MergedImages                    : 0
    /// [Panasonic]     BurstSpeed                      : 6
    /// [File]          ExifByteOrder                   : Big-endian (Motorola, MM)
    /// ```
    ///
    /// and oxidex already prints BurstSpeed 6 there through the plain-u32 path,
    /// which it could only do by reading the directory little-endian.
    ///
    /// So no corpus file exercises a big-endian directory for *MergedImages*
    /// specifically, and this fixture is constructed. But the underlying bug
    /// is corpus-visible for other tags reached only through the generic
    /// fallback: see `white_balance_reads_the_high_half_of_a_big_endian_short`
    /// below, built from real `PanasonicDMC-LC40.jpg` bytes whose MakerNote
    /// directory does resolve big-endian. It is worth keeping this
    /// MergedImages case anyway, now exercising the fallback's field-type
    /// dispatch (`inline_scalar_i32`) rather than a since-removed hand-picked
    /// tag list: on a big-endian directory the plain-u32 path silently yields
    /// 196608 for a reading of 3, which is the failure mode that produces a
    /// confident wrong number rather than a missing tag.
    #[test]
    fn merged_images_reads_the_high_half_of_a_big_endian_short() {
        let mut data = Vec::new();
        data.extend_from_slice(PANASONIC_HEADER);
        data.extend_from_slice(&1u16.to_be_bytes()); // entry_count
        data.extend_from_slice(&0x0076u16.to_be_bytes());
        data.extend_from_slice(&3u16.to_be_bytes()); // int16u
        data.extend_from_slice(&1u32.to_be_bytes()); // count
        data.extend_from_slice(&3u16.to_be_bytes()); // high half of value field
        data.extend_from_slice(&[0, 0]); // low half, unused

        let mut tags = HashMap::new();
        parse_panasonic_makernotes(&data, ByteOrder::BigEndian, &mut tags);
        assert_eq!(tags.get("Panasonic:MergedImages").unwrap(), "3");
    }

    /// `combined-samples/Panasonic/PanasonicDMC-LC40.jpg` carries a real
    /// big-endian Panasonic MakerNote directory (its entry count, byte 0x0b,
    /// only parses as a plausible IFD entry count -- 11 -- when read
    /// big-endian; little-endian gives 2816). Its WhiteBalance entry is tag
    /// 0x0003, SHORT, count 1, raw bytes `00 01` -- decoded value 1. Pinned
    /// 13.59's `-v3` on the file shows:
    ///
    /// ```text
    /// 2)  WhiteBalance = 1
    ///     - Tag 0x0003 (2 bytes, int16u[1]):
    ///         0424: 00 01                                           [..]
    /// ```
    ///
    /// and `-G1 -s` prints `[Panasonic] WhiteBalance : Auto`.
    ///
    /// WhiteBalance is an enum tag (`register_enum_tag_required`), reached
    /// only through the generic fallback -- it was never in the old
    /// `matches!` allowlist, unlike MergedImages above. Before
    /// `inline_scalar_i32`, the fallback read `entry.value_offset` whole:
    /// `00 01 00 00` parsed big-endian is 65536, and
    /// `WHITE_BALANCE.decode(65536)` has no match, so oxidex printed
    /// `Unknown (65536)` where ExifTool prints `Auto`. This is not a
    /// hypothetical: it is `PanasonicDMC-LC40.jpg`'s pre-fix oxidex output.
    #[test]
    fn white_balance_reads_the_high_half_of_a_big_endian_short() {
        let mut data = Vec::new();
        data.extend_from_slice(PANASONIC_HEADER);
        data.extend_from_slice(&1u16.to_be_bytes()); // entry_count
        data.extend_from_slice(&0x0003u16.to_be_bytes()); // WhiteBalance
        data.extend_from_slice(&3u16.to_be_bytes()); // SHORT
        data.extend_from_slice(&1u32.to_be_bytes()); // count
        data.extend_from_slice(&1u16.to_be_bytes()); // high half: 1 = Auto
        data.extend_from_slice(&[0, 0]); // low half, unused

        let mut tags = HashMap::new();
        parse_panasonic_makernotes(&data, ByteOrder::BigEndian, &mut tags);
        assert_eq!(tags.get("Panasonic:WhiteBalance").unwrap(), "Auto");
    }

    /// WBShiftAB/WBShiftGM (0x0046/0x0047) are `Writable => 'int16u'` but
    /// `Format => 'int16s'` overrides the read (Panasonic.pm:878-889): the
    /// wire field type is unsigned SHORT, so the generic fallback's
    /// field-type dispatch cannot tell them apart from a plain int16u tag --
    /// they stay a hand-picked case (`matches!(tag_id, 0x0046 | 0x0047 | ...)`)
    /// rather than folding into `inline_scalar_i32`.
    ///
    /// `combined-samples/Panasonic/PanasonicDMC-G3.jpg` (little-endian
    /// MakerNote) has both non-zero: pinned 13.59's `-v3` shows
    ///
    /// ```text
    /// 45) WBShiftAB = 6
    ///     - Tag 0x0046 (2 bytes, int16u[1] read as int16s[1]):
    ///         06ba: 06 00                                           [..]
    /// 46) WBShiftGM = -3
    ///     - Tag 0x0047 (2 bytes, int16u[1] read as int16s[1]):
    ///         06c6: fd ff                                           [..]
    /// ```
    ///
    /// and `-G1 -s` prints `WBShiftAB : 6`, `WBShiftGM : -3`. WBShiftGM's
    /// `fd ff` is exactly the byte pattern the generic fallback would widen to
    /// 65533 if it treated the field as unsigned, so this also proves the
    /// sign-extension half of the fix, independent of byte order.
    #[test]
    fn wb_shift_reads_int16s_format_override_on_int16u_wire() {
        let mut data = Vec::new();
        data.extend_from_slice(PANASONIC_HEADER);
        data.extend_from_slice(&2u16.to_le_bytes()); // entry_count
        data.extend_from_slice(&0x0046u16.to_le_bytes()); // WBShiftAB
        data.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        data.extend_from_slice(&1u32.to_le_bytes()); // count
        data.extend_from_slice(&6u16.to_le_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(&0x0047u16.to_le_bytes()); // WBShiftGM
        data.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        data.extend_from_slice(&1u32.to_le_bytes()); // count
        data.extend_from_slice(&0xFFFDu16.to_le_bytes()); // -3 as u16
        data.extend_from_slice(&[0, 0]);

        let mut tags = HashMap::new();
        parse_panasonic_makernotes(&data, ByteOrder::LittleEndian, &mut tags);
        assert_eq!(tags.get("Panasonic:WBShiftAB").unwrap(), "6");
        assert_eq!(tags.get("Panasonic:WBShiftGM").unwrap(), "-3");
    }

    /// Unit-level coverage of `inline_scalar_i32` itself, complementing the
    /// end-to-end fixtures above: SSHORT (field type 8) count-1 entries must
    /// sign-extend after the same half-selection `inline_u16_value` performs,
    /// and any entry that isn't a count-1 SHORT/SSHORT (a LONG, or a count > 1
    /// array) must pass `value_offset` through unchanged -- the IFD parser
    /// already decoded that full 4-byte field using the directory's byte
    /// order, so no half-selection is needed.
    #[test]
    fn test_inline_scalar_i32() {
        // Big-endian SSHORT count 1, negative: high half sign-extends.
        let be_sshort = IfdEntry {
            tag_id: 0x00,
            field_type: 8,
            value_count: 1,
            value_offset: 0xFFFD_0000,
        };
        assert_eq!(inline_scalar_i32(&be_sshort, ByteOrder::BigEndian), -3);

        // Little-endian SSHORT count 1, negative: low half sign-extends.
        let le_sshort = IfdEntry {
            tag_id: 0x00,
            field_type: 8,
            value_count: 1,
            value_offset: 0x0000_FFFD,
        };
        assert_eq!(inline_scalar_i32(&le_sshort, ByteOrder::LittleEndian), -3);

        // A LONG (field type 4) fills the whole 4-byte field regardless of
        // count; value_offset passes through unchanged.
        let long_entry = IfdEntry {
            tag_id: 0x00,
            field_type: 4,
            value_count: 1,
            value_offset: 0x0001_0000,
        };
        assert_eq!(
            inline_scalar_i32(&long_entry, ByteOrder::BigEndian),
            0x0001_0000
        );

        // A SHORT with count > 1 is an out-of-line array, not an inline
        // scalar; value_offset (the pointer) passes through unchanged.
        let short_array = IfdEntry {
            tag_id: 0x00,
            field_type: 3,
            value_count: 2,
            value_offset: 0x0000_1234,
        };
        assert_eq!(
            inline_scalar_i32(&short_array, ByteOrder::BigEndian),
            0x0000_1234
        );
    }

    /// `combined-samples/Panasonic/PanasonicDC-FZ80.jpg`'s MakerNote: every
    /// value in this group is zero/Off/Normal, exercising the non-negative,
    /// non-sentinel path the Leica fixture above doesn't.
    #[test]
    fn test_matches_exiftool_on_panasonic_fz80_zero_values() {
        assert_eq!(decode_af_point_position(&[(0, 1), (0, 1)]).unwrap(), "0 0");
        assert_eq!(format_rational64u(0, 1).unwrap(), "0");
        // PitchAngle's negation must not turn a zero reading into "-0".
        let raw = 0i32;
        assert_eq!(sprintf_g(f64::from(-raw) / 10.0, 10).unwrap(), "0");
    }

    /// `4294967295/1024` is only a sentinel inside AFPointPosition's own
    /// PrintConv (Panasonic.pm:924-929) -- `format_rational64u` on its own,
    /// as `ClearRetouchValue` and every other bare rational64u tag use it,
    /// must still report the real quotient rather than special-casing it.
    #[test]
    fn test_format_rational64u_does_not_apply_af_point_position_sentinels() {
        assert_eq!(
            format_rational64u(4_294_967_295, 1024).unwrap(),
            "4194303.999"
        );
    }

    /// `combined-samples/Panasonic/PanasonicDC-S1.jpg`, MakerNote tag 0x004e:
    /// the exact 42 bytes `exiftool -v3` prints, and the exact values
    /// `exiftool -a -G1 -s` reports for them.
    #[test]
    fn test_face_det_info_matches_exiftool_on_dc_s1_bytes() {
        let record: [u8; 42] = [
            0x02, 0x00, 0x2e, 0x00, 0x51, 0x00, 0x1b, 0x00, 0x1b, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut tags = HashMap::new();
        decode_binary_subdir(
            &PANASONIC_FACEDETINFO,
            &record,
            ByteOrder::LittleEndian,
            "Panasonic",
            &mut tags,
        );
        assert_eq!(tags.get("Panasonic:NumFacePositions").unwrap(), "2");
        assert_eq!(tags.get("Panasonic:Face1Position").unwrap(), "46 81 27 27");
        assert_eq!(tags.get("Panasonic:Face2Position").unwrap(), "0 0 0 0");
        // NumFacePositions is 2, so ExifTool's RawConv gate suppresses 3..5
        // even though the record has room for them and they read as zeros.
        assert!(!tags.contains_key("Panasonic:Face3Position"));
        assert!(!tags.contains_key("Panasonic:Face4Position"));
        assert!(!tags.contains_key("Panasonic:Face5Position"));
    }

    /// `combined-samples/Panasonic/PanasonicDMC-GF5.jpg`, MakerNote tag 0x0061:
    /// the record's 148 real bytes. The body wrote a pointer into its PrintIM
    /// block, so the "names" are junk -- but ExifTool reports exactly these
    /// values, and matching it means matching them too. This is also the case
    /// that proves the byte-indexed table: `FaceRecInfo` has no `FORMAT`, so
    /// key 4 is byte 4 and key 24 is byte 24, not words.
    #[test]
    fn test_face_rec_info_matches_exiftool_on_gf5_bytes() {
        let record: [u8; 148] = [
            0x00, 0x04, 0x52, 0x39, 0x38, 0x00, 0x00, 0x02, 0x00, 0x07, 0x00, 0x00, 0x00, 0x04,
            0x30, 0x31, 0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x50, 0x72, 0x69, 0x6e, 0x74, 0x49, 0x4d, 0x00,
            0x30, 0x32, 0x35, 0x30, 0x00, 0x00, 0x0e, 0x00, 0x01, 0x00, 0x16, 0x00, 0x16, 0x00,
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x64, 0x00, 0x00, 0x00, 0x07, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0b, 0x00, 0xac, 0x00, 0x00, 0x00,
            0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0e, 0x00,
            0xc4, 0x00, 0x00, 0x00, 0x00, 0x01, 0x05, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x00,
            0x00, 0x00, 0x10, 0x01, 0x80, 0x00, 0x00, 0x00, 0x09, 0x11, 0x00, 0x00, 0x10, 0x27,
            0x00, 0x00, 0x0b, 0x0f, 0x00, 0x00, 0x10, 0x27,
        ];
        let mut tags = HashMap::new();
        decode_binary_subdir(
            &PANASONIC_FACERECINFO,
            &record,
            ByteOrder::LittleEndian,
            "Panasonic",
            &mut tags,
        );
        assert_eq!(tags.get("Panasonic:FacesRecognized").unwrap(), "1024");
        assert_eq!(tags.get("Panasonic:RecognizedFace1Name").unwrap(), "8");
        assert_eq!(
            tags.get("Panasonic:RecognizedFace1Position").unwrap(),
            "0 0 0 2560"
        );
        assert_eq!(tags.get("Panasonic:RecognizedFace1Age").unwrap(), "");
        assert_eq!(tags.get("Panasonic:RecognizedFace2Name").unwrap(), "\u{16}");
        assert_eq!(
            tags.get("Panasonic:RecognizedFace2Position").unwrap(),
            "0 8 0 0"
        );
        assert_eq!(
            tags.get("Panasonic:RecognizedFace3Position").unwrap(),
            "0 257 1 0"
        );
    }
}
