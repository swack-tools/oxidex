//! GoPro Action Camera MakerNote parser
//!
//! Parses GoPro-specific EXIF MakerNote tags from HERO series action cameras.
//! Contains camera settings, Protune parameters, video stabilization info,
//! frame rates, and action-specific metadata.
//!
//! ## Supported Models
//! - HERO4 (Black, Silver)
//! - HERO5 (Black, Session)
//! - HERO6 Black
//! - HERO7 (Black, Silver, White)
//! - HERO8 Black
//! - HERO9 Black
//! - HERO10 Black
//! - HERO11 Black
//! - HERO12 Black
//! - MAX (360 camera)
//! - Fusion (360 camera)
//!
//! ## Key Features
//! - Protune settings (color profile, sharpness, white balance)
//! - HyperSmooth stabilization mode and level
//! - Frame rate and resolution
//! - Field of view (FOV)
//! - Low light mode
//! - Auto boost settings
//! - SuperPhoto mode
//! - TimeWarp speed
//! - Video encoding details
//! - Audio settings (wind noise reduction, raw audio)
//!
//! ## Architecture
//! GoPro uses a proprietary binary format with GPMF (GoPro Metadata Format)
//! in video files, but still images contain simplified metadata in MakerNotes.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_string};
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

// Import registry module
use super::registries::gopro_registry;

// Import macros for declarative decoder definitions
use crate::const_decoder;

// GoPro MakerNote Tag IDs
/// Firmware version
pub const GOPRO_VERSION: u16 = 0x0001;
/// Camera model
pub const GOPRO_MODEL: u16 = 0x0002;
/// Serial number
pub const GOPRO_SERIAL: u16 = 0x0003;
/// Video/Photo resolution
pub const GOPRO_RESOLUTION: u16 = 0x0100;
/// Frame rate (fps)
pub const GOPRO_FRAME_RATE: u16 = 0x0101;
/// Field of view
pub const GOPRO_FOV: u16 = 0x0102;
/// Low light mode
pub const GOPRO_LOW_LIGHT: u16 = 0x0103;
/// Protune enabled
pub const GOPRO_PROTUNE: u16 = 0x0104;
/// White balance setting
pub const GOPRO_WHITE_BALANCE: u16 = 0x0105;
/// Color profile (flat/GoPro)
pub const GOPRO_COLOR: u16 = 0x0106;
/// Sharpness level
pub const GOPRO_SHARPNESS: u16 = 0x0107;
/// Contrast (Protune)
pub const GOPRO_CONTRAST: u16 = 0x0108;
/// Saturation (Protune)
pub const GOPRO_SATURATION: u16 = 0x0109;
/// Minimum ISO (Protune)
pub const GOPRO_ISO_MIN: u16 = 0x010A;
/// Maximum ISO (Protune)
pub const GOPRO_ISO_MAX: u16 = 0x010B;
/// Exposure compensation
pub const GOPRO_EXPOSURE: u16 = 0x010C;
/// Shutter speed
pub const GOPRO_SHUTTER: u16 = 0x010D;
/// Metering mode
pub const GOPRO_METERING: u16 = 0x010E;
/// Spot meter area
pub const GOPRO_SPOT_METER: u16 = 0x010F;
/// Electronic Image Stabilization
pub const GOPRO_EIS: u16 = 0x0110;
/// HyperSmooth level
pub const GOPRO_HYPERSMOOTH: u16 = 0x0111;
/// HyperSmooth Boost
pub const GOPRO_BOOST: u16 = 0x0112;
/// Stabilization mode
pub const GOPRO_STABILIZATION_MODE: u16 = 0x0113;
/// Auto Boost mode
pub const GOPRO_AUTO_BOOST: u16 = 0x0114;
/// SuperPhoto mode
pub const GOPRO_SUPER_PHOTO: u16 = 0x0115;
/// HDR photo mode
pub const GOPRO_HDR: u16 = 0x0116;
/// Digital zoom level
pub const GOPRO_DIGITAL_ZOOM: u16 = 0x0117;
/// Raw audio enabled
pub const GOPRO_RAW_AUDIO: u16 = 0x0118;
/// Wind noise reduction
pub const GOPRO_WIND_NOISE: u16 = 0x0119;
/// TimeWarp speed multiplier
pub const GOPRO_TIMEWARP_SPEED: u16 = 0x011A;
/// Video codec (H.264/H.265)
pub const GOPRO_VIDEO_ENCODING: u16 = 0x011B;
/// Video bitrate
pub const GOPRO_BIT_RATE: u16 = 0x011C;
/// Camera orientation
pub const GOPRO_ORIENTATION: u16 = 0x011D;
/// GPS fix status
pub const GOPRO_GPS_FIX: u16 = 0x011E;
/// Lens model identifier
pub const GOPRO_LENS_MODEL: u16 = 0x011F;
/// Night photo mode
pub const GOPRO_NIGHT_PHOTO: u16 = 0x0120;
/// Burst photo rate
pub const GOPRO_BURST_RATE: u16 = 0x0121;
/// Live burst mode
pub const GOPRO_LIVE_BURST: u16 = 0x0122;
/// Time lapse interval (ms)
pub const GOPRO_TIMELAPSE_INTERVAL: u16 = 0x0123;
/// Night lapse interval
pub const GOPRO_NIGHT_LAPSE_INTERVAL: u16 = 0x0124;
/// Loop recording duration
pub const GOPRO_LOOP_DURATION: u16 = 0x0125;

// GoPro signature
const GOPRO_SIGNATURE: &[u8] = b"GoPro";

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================
// These replace 23 repetitive decoder functions, dramatically reducing
// duplication from 181% to under 50% while maintaining all functionality.

// Field of View decoder - GoPro's FOV options
const_decoder!(pub
    FOV,
    i16,
    [
        (0, "Wide"),
        (1, "Medium"),
        (2, "Narrow"),
        (3, "Linear"),
        (4, "SuperView"),
        (5, "Max SuperView"),
        (6, "HyperView"),
    ]
);

// White Balance decoder - Temperature presets and Auto/Native modes
const_decoder!(pub
    WHITE_BALANCE,
    i16,
    [
        (0, "Auto"),
        (1, "3000K"),
        (2, "4000K"),
        (3, "4500K"),
        (4, "5000K"),
        (5, "5500K"),
        (6, "6000K"),
        (7, "6500K"),
        (8, "Native"),
    ]
);

// Color Profile decoder - GoPro's color modes
const_decoder!(pub
    COLOR_PROFILE,
    i16,
    [
        (0, "GoPro Color"),
        (1, "Flat"),
        (2, "Vibrant"),
        (3, "Natural"),
    ]
);

// Sharpness Level decoder - Low/Medium/High
const_decoder!(pub SHARPNESS, i16, [(0, "Low"), (1, "Medium"), (2, "High"),]);

// Contrast Level decoder - Protune contrast range
const_decoder!(pub
    CONTRAST,
    i16,
    [
        (-2, "Very Low"),
        (-1, "Low"),
        (0, "Normal"),
        (1, "High"),
        (2, "Very High"),
    ]
);

// Saturation Level decoder - Same range as contrast
const_decoder!(pub
    SATURATION,
    i16,
    [
        (-2, "Very Low"),
        (-1, "Low"),
        (0, "Normal"),
        (1, "High"),
        (2, "Very High"),
    ]
);

// Metering Mode decoder - Center/Spot/Matrix
const_decoder!(pub METERING, i16, [(0, "Center"), (1, "Spot"), (2, "Matrix"),]);

// HyperSmooth Level decoder - Off through Auto Boost
const_decoder!(pub
    HYPERSMOOTH,
    i16,
    [
        (0, "Off"),
        (1, "On"),
        (2, "High"),
        (3, "Boost"),
        (4, "Auto Boost"),
    ]
);

// Video Resolution decoder - 4K, 5K, and specialty modes
const_decoder!(pub
    RESOLUTION,
    i16,
    [
        (0, "4K"),
        (1, "2.7K"),
        (2, "2.7K 4:3"),
        (3, "1440p"),
        (4, "1080p"),
        (5, "720p"),
        (6, "5.3K"),
        (7, "5K"),
        (8, "4K 4:3"),
        (9, "2K"),
        (10, "5.3K 4:3"),
    ]
);

// Video Encoding decoder - Codec options
const_decoder!(pub
    VIDEO_ENCODING,
    i16,
    [
        (0, "H.264"),
        (1, "H.265 (HEVC)"),
        (2, "H.264 High Profile"),
        (3, "H.265 10-bit"),
    ]
);

// SuperPhoto Mode decoder - Off/Auto/HDR
const_decoder!(pub SUPER_PHOTO, i16, [(0, "Off"), (1, "Auto"), (2, "HDR"),]);

// Night Photo Mode decoder - Exposure time options
const_decoder!(pub
    NIGHT_PHOTO,
    i16,
    [
        (0, "Off"),
        (1, "Auto"),
        (2, "2s"),
        (3, "5s"),
        (4, "10s"),
        (5, "15s"),
        (6, "20s"),
        (7, "30s"),
    ]
);

// Burst Rate decoder - Photos per second
const_decoder!(pub
    BURST_RATE,
    i16,
    [
        (0, "3/1s"),
        (1, "5/1s"),
        (2, "10/1s"),
        (3, "10/2s"),
        (4, "10/3s"),
        (5, "30/1s"),
        (6, "30/2s"),
        (7, "30/3s"),
        (8, "30/6s"),
    ]
);

// Camera Orientation decoder - Up/Down/Auto
const_decoder!(pub ORIENTATION, i16, [(0, "Up"), (1, "Down"), (2, "Auto"),]);

// Note: Tag registry has been moved to registries/gopro.rs for centralized management.
// The registry is now created on-demand using gopro_registry() function.

// ============================================================================
// Custom Value Formatters
// ============================================================================
// These functions handle values that require mathematical transformations
// or special formatting logic that can't be handled by simple const decoders.

/// Formats frame rate value
///
/// # Arguments
/// * `value` - Frame rate in fps
///
/// # Returns
/// Formatted string with "fps" suffix, or "Unknown" if invalid
fn format_frame_rate(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    format!("{} fps", value)
}

/// Formats exposure compensation value
///
/// # Arguments
/// * `value` - EV value in tenths (e.g., 15 = +1.5 EV)
///
/// # Returns
/// Formatted EV string with sign
fn format_exposure(value: i16) -> String {
    let ev = value as f64 / 10.0;
    if ev >= 0.0 {
        format!("+{:.1} EV", ev)
    } else {
        format!("{:.1} EV", ev)
    }
}

/// Formats shutter speed value
///
/// # Arguments
/// * `value` - Shutter speed as 1/n (if < 1000) or ms (if >= 1000)
///
/// # Returns
/// Formatted shutter speed string or "Auto"
fn format_shutter(value: i16) -> String {
    if value <= 0 {
        return "Auto".to_string();
    }
    if value < 1000 {
        format!("1/{} s", value)
    } else {
        let seconds = value as f64 / 1000.0;
        format!("{:.1} s", seconds)
    }
}

/// Formats digital zoom level
///
/// # Arguments
/// * `value` - Zoom level as percentage (100 = 1.0x)
///
/// # Returns
/// Formatted zoom string with 'x' suffix
fn format_digital_zoom(value: i16) -> String {
    if value <= 100 {
        return "1.0x".to_string();
    }
    let zoom = value as f64 / 100.0;
    format!("{:.1}x", zoom)
}

/// Formats TimeWarp speed multiplier
///
/// # Arguments
/// * `value` - Speed multiplier (e.g., 2 = 2x)
///
/// # Returns
/// Formatted speed string with 'x' suffix or "Auto"
fn format_timewarp_speed(value: i16) -> String {
    if value <= 0 {
        return "Auto".to_string();
    }
    format!("{}x", value)
}

/// Formats time lapse interval
///
/// # Arguments
/// * `value` - Interval in milliseconds
///
/// # Returns
/// Formatted interval in ms or seconds
fn format_interval(value: i16) -> String {
    if value < 1000 {
        format!("{} ms", value)
    } else {
        let seconds = value as f64 / 1000.0;
        format!("{:.1} s", seconds)
    }
}

/// Formats video bitrate
///
/// # Arguments
/// * `value` - Bitrate in Mbps
///
/// # Returns
/// Formatted bitrate string with "Mbps" suffix or "Auto"
fn format_bitrate(value: i16) -> String {
    if value <= 0 {
        return "Auto".to_string();
    }
    format!("{} Mbps", value)
}

/// Formats loop duration
///
/// # Arguments
/// * `value` - Duration in minutes
///
/// # Returns
/// Formatted duration string with "min" suffix
fn format_loop_duration(value: i16) -> String {
    format!("{} min", value)
}

/// GoPro MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct GoProParser;

impl GoProParser {
    /// Creates a new GoPro parser instance
    pub fn new() -> Self {
        GoProParser
    }
}

impl MakerNoteParser for GoProParser {
    fn manufacturer_name(&self) -> &'static str {
        "GoPro"
    }

    fn tag_prefix(&self) -> &'static str {
        "GoPro:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 5 {
            return false;
        }
        data.starts_with(GOPRO_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Configure IFD parser for GoPro MakerNote format
        // GoPro uses a 5-byte signature "GoPro" followed by standard IFD structure
        let config = IfdParserConfig {
            signature: Some(GOPRO_SIGNATURE),
            signature_offset: 5,
            max_entries: 200,
        };

        // Create registry on-demand
        let registry = gopro_registry();

        // Use shared IFD parser to eliminate boilerplate
        // The callback processes each parsed IFD entry with tag-specific logic
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            // Extract value based on tag type
            // String tags - firmware version, model, serial, lens
            if matches!(
                entry.tag_id,
                GOPRO_VERSION | GOPRO_MODEL | GOPRO_SERIAL | GOPRO_LENS_MODEL
            ) {
                if let Some(s) = extract_string(entry, parse_data, byte_order) {
                    let tag_name = match entry.tag_id {
                        GOPRO_VERSION => "Version",
                        GOPRO_MODEL => "Model",
                        GOPRO_SERIAL => "SerialNumber",
                        GOPRO_LENS_MODEL => "LensModel",
                        _ => return,
                    };
                    tags.insert(format!("GoPro:{}", tag_name), s);
                }
            } else {
                // Try to extract as i16 array - most GoPro tags use this type
                if let Some(array) = extract_i16_array(entry, parse_data, byte_order)
                    && let Some(&val) = array.first()
                {
                    // Registry lookup: get tag name and decode value in one step
                    if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                        let formatted_value = registry.decode_i16(entry.tag_id, val);
                        tags.insert(format!("GoPro:{}", tag_name), formatted_value);
                    }
                }
            }
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::shared::generic_decoders::{ON_OFF, YES_NO};
    use super::*;

    #[test]
    fn test_gopro_parser_creation() {
        let parser = GoProParser::new();
        assert_eq!(parser.manufacturer_name(), "GoPro");
        assert_eq!(parser.tag_prefix(), "GoPro:");
    }

    #[test]
    fn test_decode_fov() {
        assert_eq!(FOV.decode(0), "Wide");
        assert_eq!(FOV.decode(3), "Linear");
        assert_eq!(FOV.decode(4), "SuperView");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(WHITE_BALANCE.decode(0), "Auto");
        assert_eq!(WHITE_BALANCE.decode(7), "6500K");
        assert_eq!(WHITE_BALANCE.decode(8), "Native");
    }

    #[test]
    fn test_decode_color_profile() {
        assert_eq!(COLOR_PROFILE.decode(0), "GoPro Color");
        assert_eq!(COLOR_PROFILE.decode(1), "Flat");
    }

    #[test]
    fn test_decode_hypersmooth() {
        assert_eq!(HYPERSMOOTH.decode(0), "Off");
        assert_eq!(HYPERSMOOTH.decode(3), "Boost");
        assert_eq!(HYPERSMOOTH.decode(4), "Auto Boost");
    }

    #[test]
    fn test_decode_resolution() {
        assert_eq!(RESOLUTION.decode(0), "4K");
        assert_eq!(RESOLUTION.decode(6), "5.3K");
        assert_eq!(RESOLUTION.decode(10), "5.3K 4:3");
    }

    #[test]
    fn test_format_exposure() {
        assert_eq!(format_exposure(15), "+1.5 EV");
        assert_eq!(format_exposure(-10), "-1.0 EV");
        assert_eq!(format_exposure(0), "+0.0 EV");
    }

    #[test]
    fn test_format_digital_zoom() {
        assert_eq!(format_digital_zoom(100), "1.0x");
        assert_eq!(format_digital_zoom(200), "2.0x");
    }

    #[test]
    fn test_format_timewarp_speed() {
        assert_eq!(format_timewarp_speed(0), "Auto");
        assert_eq!(format_timewarp_speed(2), "2x");
        assert_eq!(format_timewarp_speed(30), "30x");
    }

    #[test]
    fn test_decode_burst_rate() {
        assert_eq!(BURST_RATE.decode(5), "30/1s");
        assert_eq!(BURST_RATE.decode(2), "10/1s");
    }

    #[test]
    fn test_decode_night_photo() {
        assert_eq!(NIGHT_PHOTO.decode(0), "Off");
        assert_eq!(NIGHT_PHOTO.decode(4), "10s");
        assert_eq!(NIGHT_PHOTO.decode(7), "30s");
    }

    #[test]
    fn test_on_off_decoder() {
        assert_eq!(ON_OFF.decode(0), "Off");
        assert_eq!(ON_OFF.decode(1), "On");
    }

    #[test]
    fn test_yes_no_decoder() {
        assert_eq!(YES_NO.decode(0), "No");
        assert_eq!(YES_NO.decode(1), "Yes");
    }
}
