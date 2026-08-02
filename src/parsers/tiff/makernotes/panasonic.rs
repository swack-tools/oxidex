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

use super::makernote_context::MakerNoteContext;
use super::shared::MakerNoteParser;
use super::shared::binary_subdir::{BinaryTable, decode_binary_subdir};
use super::shared::ifd_parser_base::resolve_byte_order_at;
use face_tables::{PANASONIC_FACEDETINFO, PANASONIC_FACERECINFO};

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

/// Returns the byte offset of this payload's IFD, or `None` if neither the
/// Panasonic nor the unnumbered-Leica header matches.
fn panasonic_ifd_offset(data: &[u8]) -> Option<usize> {
    if data.len() >= 12 && &data[0..12] == PANASONIC_HEADER {
        Some(12)
    } else if data.len() >= 8 && &data[0..8] == LEICA_UNNUMBERED_HEADER {
        Some(8)
    } else {
        None
    }
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
        // Panasonic header: "Panasonic\0\0\0" (12 bytes), or the unnumbered
        // "LEICA\0\0\0" (8 bytes) a bare-Make "LEICA" body writes for the
        // same Panasonic::Main table.
        panasonic_ifd_offset(data).is_some()
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_impl(data, byte_order, None, None, tags)
    }

    fn parse_with_model(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_impl(data, byte_order, model, None, tags)
    }

    /// Panasonic's out-of-line value offsets are measured from the enclosing
    /// TIFF header, not from the MakerNote payload -- `PanasonicDC-G9.jpg`
    /// stores `LensType`'s 34 string bytes at TIFF offset 3414 while the
    /// payload itself begins at TIFF offset 1314.  Resolving them needs the
    /// enclosing block and the payload's position in it, which only
    /// `parse_with_context` has; `parse`/`parse_with_model` keep the old
    /// payload-relative arithmetic for a caller that holds no enclosing block.
    fn parse_with_context(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_impl(
            ctx.window(),
            byte_order,
            model,
            ctx.payload_tiff_offset(),
            tags,
        )
    }
}

impl PanasonicParser {
    /// Shared implementation behind [`MakerNoteParser::parse`] and
    /// [`MakerNoteParser::parse_with_model`].
    #[allow(clippy::too_many_arguments)]
    fn parse_impl(
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
        let data_base = value_base(data_base, model);

        // Extract tags from entries
        for entry in entries {
            self.parse_entry(
                &entry, data, ifd_offset, data_base, byte_order, model, &registry, tags,
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
        data_base: Option<u32>,
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
            }
            return;
        }

        // Special handling for string tags (must read from data buffer)
        // These tags contain text data that needs to be extracted from the makernote
        match tag_id {
            // Basic info strings.  0x0051 LensType, 0x0052 LensSerialNumber
            // and 0x0053 AccessoryType are all `Writable => 'string'` in
            // %Panasonic::Main (Panasonic.pm:943, :949 and :955) -- there is no
            // Panasonic lens-id table in ExifTool, the tag holds the name
            // itself.
            0x0026 | 0x0051 | 0x0052 | 0x0053 | 0x0054 |
            // Supplementary info strings (Title, BabyName)
            0x0065 | 0x0066 |
            // Location-related strings
            0x0067 | 0x0069 | 0x006B | 0x006D | 0x006F | 0x0080 => {
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
            _ => {}
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

        // Standard registry-based decoding for enumerated and simple integer tags
        if let Some(tag_name) = registry.get_tag_name(tag_id) {
            let value = entry.value_offset as i32;
            let decoded = registry.decode_i32(tag_id, value);
            tags.insert(format!("Panasonic:{}", tag_name), decoded);
        }
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
/// TIFF offset 3414 while the MakerNote payload starts at 1314.  When
/// `data_base` is known, `full_data` is the payload-onwards window and the
/// value sits `value_offset - data_base` bytes into it.
///
/// Without an enclosing block there is nothing to measure from, so the old
/// payload-relative arithmetic is kept rather than guessing a base: see
/// [`MakerNoteContext::payload_tiff_offset`].
fn resolve_value_offset(
    entry: &IfdEntry,
    ifd_offset: usize,
    data_base: Option<u32>,
) -> Option<usize> {
    match data_base {
        Some(base) => entry.value_offset.checked_sub(base).map(|v| v as usize),
        None => ifd_offset.checked_add(entry.value_offset as usize),
    }
}

/// The TIFF-relative start this MakerNote's out-of-line value offsets are
/// measured from.
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
/// The adjustment only applies to the TIFF-relative form. With no enclosing
/// block `data_base` is `None` and the offsets are already payload-relative, so
/// there is no base to correct.
fn value_base(data_base: Option<u32>, model: Option<&str>) -> Option<u32> {
    if model == Some("DC-FT7") {
        return data_base.map(|base| base.saturating_sub(12));
    }
    data_base
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
    data_base: Option<u32>,
) -> Option<String> {
    let byte_count = entry.value_count as usize;

    // For inline strings (≤4 bytes), value is in value_offset field
    if byte_count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        return exiftool_string(&bytes[0..byte_count]);
    }

    // For longer strings, read from offset
    let abs_offset = resolve_value_offset(entry, ifd_offset, data_base)?;

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
    data_base: Option<u32>,
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
    let abs_offset = resolve_value_offset(entry, ifd_offset, data_base)?;
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

/// Extracts an entry's value as a list of numeric components, honoring the
/// entry's field type: int8u/undef entries yield one component per byte,
/// int16u entries one per 16-bit word. Returns None for unsupported types or
/// unreachable out-of-line values.
fn extract_component_values(
    entry: &IfdEntry,
    full_data: &[u8],
    ifd_offset: usize,
    data_base: Option<u32>,
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
                let abs_offset = resolve_value_offset(entry, ifd_offset, data_base)?;
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
        _ => None,
    }
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

    #[test]
    fn test_decode_focus_mode() {
        assert_eq!(FOCUS_MODE.decode(4), "AF-S (Single)");
        assert_eq!(FOCUS_MODE.decode(5), "AF-C (Continuous)");
        assert_eq!(FOCUS_MODE.decode(16), "MF (Manual Focus)");
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
    /// shifting any other body's base would move every out-of-line value.
    #[test]
    fn test_only_the_dc_ft7_shifts_its_value_base() {
        assert_eq!(value_base(Some(1269), Some("DC-FT7")), Some(1257));
        assert_eq!(value_base(Some(1368), Some("DC-S1")), Some(1368));
        assert_eq!(value_base(Some(1368), None), Some(1368));
        // With no enclosing block there is no TIFF-relative base to correct.
        assert_eq!(value_base(None, Some("DC-FT7")), None);
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
