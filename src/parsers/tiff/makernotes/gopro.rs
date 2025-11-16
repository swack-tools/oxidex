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

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// GoPro MakerNote Tag IDs
const GOPRO_VERSION: u16 = 0x0001; // Firmware version
const GOPRO_MODEL: u16 = 0x0002; // Camera model
const GOPRO_SERIAL: u16 = 0x0003; // Serial number
const GOPRO_RESOLUTION: u16 = 0x0100; // Video/Photo resolution
const GOPRO_FRAME_RATE: u16 = 0x0101; // Frame rate (fps)
const GOPRO_FOV: u16 = 0x0102; // Field of view
const GOPRO_LOW_LIGHT: u16 = 0x0103; // Low light mode
const GOPRO_PROTUNE: u16 = 0x0104; // Protune enabled
const GOPRO_WHITE_BALANCE: u16 = 0x0105; // White balance setting
const GOPRO_COLOR: u16 = 0x0106; // Color profile (flat/GoPro)
const GOPRO_SHARPNESS: u16 = 0x0107; // Sharpness level
const GOPRO_CONTRAST: u16 = 0x0108; // Contrast (Protune)
const GOPRO_SATURATION: u16 = 0x0109; // Saturation (Protune)
const GOPRO_ISO_MIN: u16 = 0x010A; // Minimum ISO (Protune)
const GOPRO_ISO_MAX: u16 = 0x010B; // Maximum ISO (Protune)
const GOPRO_EXPOSURE: u16 = 0x010C; // Exposure compensation
const GOPRO_SHUTTER: u16 = 0x010D; // Shutter speed
const GOPRO_METERING: u16 = 0x010E; // Metering mode
const GOPRO_SPOT_METER: u16 = 0x010F; // Spot meter area
const GOPRO_EIS: u16 = 0x0110; // Electronic Image Stabilization
const GOPRO_HYPERSMOOTH: u16 = 0x0111; // HyperSmooth level
const GOPRO_BOOST: u16 = 0x0112; // HyperSmooth Boost
const GOPRO_STABILIZATION_MODE: u16 = 0x0113; // Stabilization mode
const GOPRO_AUTO_BOOST: u16 = 0x0114; // Auto Boost mode
const GOPRO_SUPER_PHOTO: u16 = 0x0115; // SuperPhoto mode
const GOPRO_HDR: u16 = 0x0116; // HDR photo mode
const GOPRO_DIGITAL_ZOOM: u16 = 0x0117; // Digital zoom level
const GOPRO_RAW_AUDIO: u16 = 0x0118; // Raw audio enabled
const GOPRO_WIND_NOISE: u16 = 0x0119; // Wind noise reduction
const GOPRO_TIMEWARP_SPEED: u16 = 0x011A; // TimeWarp speed multiplier
const GOPRO_VIDEO_ENCODING: u16 = 0x011B; // Video codec (H.264/H.265)
const GOPRO_BIT_RATE: u16 = 0x011C; // Video bitrate
const GOPRO_ORIENTATION: u16 = 0x011D; // Camera orientation
const GOPRO_GPS_FIX: u16 = 0x011E; // GPS fix status
const GOPRO_LENS_MODEL: u16 = 0x011F; // Lens model identifier
const GOPRO_NIGHT_PHOTO: u16 = 0x0120; // Night photo mode
const GOPRO_BURST_RATE: u16 = 0x0121; // Burst photo rate
const GOPRO_LIVE_BURST: u16 = 0x0122; // Live burst mode
const GOPRO_TIMELAPSE_INTERVAL: u16 = 0x0123; // Time lapse interval (ms)
const GOPRO_NIGHT_LAPSE_INTERVAL: u16 = 0x0124; // Night lapse interval
const GOPRO_LOOP_DURATION: u16 = 0x0125; // Loop recording duration

// GoPro signature
const GOPRO_SIGNATURE: &[u8] = b"GoPro";

/// Decodes GoPro field of view setting
///
/// # Arguments
/// * `value` - FOV code
///
/// # Returns
/// Human-readable FOV description
fn decode_fov(value: i16) -> String {
    match value {
        0 => "Wide".to_string(),
        1 => "Medium".to_string(),
        2 => "Narrow".to_string(),
        3 => "Linear".to_string(),
        4 => "SuperView".to_string(),
        5 => "Max SuperView".to_string(),
        6 => "HyperView".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes white balance mode
///
/// # Arguments
/// * `value` - White balance code
///
/// # Returns
/// Human-readable white balance mode
fn decode_white_balance(value: i16) -> String {
    match value {
        0 => "Auto".to_string(),
        1 => "3000K".to_string(),
        2 => "4000K".to_string(),
        3 => "4500K".to_string(),
        4 => "5000K".to_string(),
        5 => "5500K".to_string(),
        6 => "6000K".to_string(),
        7 => "6500K".to_string(),
        8 => "Native".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes color profile setting
///
/// # Arguments
/// * `value` - Color profile code
///
/// # Returns
/// Human-readable color profile
fn decode_color_profile(value: i16) -> String {
    match value {
        0 => "GoPro Color".to_string(),
        1 => "Flat".to_string(),
        2 => "Vibrant".to_string(),
        3 => "Natural".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes sharpness level
///
/// # Arguments
/// * `value` - Sharpness code
///
/// # Returns
/// Human-readable sharpness level
fn decode_sharpness(value: i16) -> String {
    match value {
        0 => "Low".to_string(),
        1 => "Medium".to_string(),
        2 => "High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes Protune contrast level
///
/// # Arguments
/// * `value` - Contrast value (-2 to +2)
///
/// # Returns
/// Formatted contrast string
fn decode_contrast(value: i16) -> String {
    match value {
        -2 => "Very Low".to_string(),
        -1 => "Low".to_string(),
        0 => "Normal".to_string(),
        1 => "High".to_string(),
        2 => "Very High".to_string(),
        _ => format!("{}", value),
    }
}

/// Decodes saturation level
///
/// # Arguments
/// * `value` - Saturation value (-2 to +2)
///
/// # Returns
/// Formatted saturation string
fn decode_saturation(value: i16) -> String {
    match value {
        -2 => "Very Low".to_string(),
        -1 => "Low".to_string(),
        0 => "Normal".to_string(),
        1 => "High".to_string(),
        2 => "Very High".to_string(),
        _ => format!("{}", value),
    }
}

/// Decodes metering mode
///
/// # Arguments
/// * `value` - Metering code
///
/// # Returns
/// Human-readable metering mode
fn decode_metering(value: i16) -> String {
    match value {
        0 => "Center".to_string(),
        1 => "Spot".to_string(),
        2 => "Matrix".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes HyperSmooth level
///
/// # Arguments
/// * `value` - HyperSmooth code
///
/// # Returns
/// Human-readable HyperSmooth level
fn decode_hypersmooth(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "On".to_string(),
        2 => "High".to_string(),
        3 => "Boost".to_string(),
        4 => "Auto Boost".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes video resolution
///
/// # Arguments
/// * `value` - Resolution code
///
/// # Returns
/// Human-readable resolution
fn decode_resolution(value: i16) -> String {
    match value {
        0 => "4K".to_string(),
        1 => "2.7K".to_string(),
        2 => "2.7K 4:3".to_string(),
        3 => "1440p".to_string(),
        4 => "1080p".to_string(),
        5 => "720p".to_string(),
        6 => "5.3K".to_string(),
        7 => "5K".to_string(),
        8 => "4K 4:3".to_string(),
        9 => "2K".to_string(),
        10 => "5.3K 4:3".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes frame rate
///
/// # Arguments
/// * `value` - Frame rate value
///
/// # Returns
/// Formatted frame rate string
fn decode_frame_rate(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    format!("{} fps", value)
}

/// Decodes video encoding codec
///
/// # Arguments
/// * `value` - Codec code
///
/// # Returns
/// Human-readable codec name
fn decode_video_encoding(value: i16) -> String {
    match value {
        0 => "H.264".to_string(),
        1 => "H.265 (HEVC)".to_string(),
        2 => "H.264 High Profile".to_string(),
        3 => "H.265 10-bit".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes SuperPhoto mode
///
/// # Arguments
/// * `value` - SuperPhoto code
///
/// # Returns
/// Human-readable SuperPhoto mode
fn decode_super_photo(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Auto".to_string(),
        2 => "HDR".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes night photo mode
///
/// # Arguments
/// * `value` - Night photo code
///
/// # Returns
/// Human-readable night mode
fn decode_night_photo(value: i16) -> String {
    match value {
        0 => "Off".to_string(),
        1 => "Auto".to_string(),
        2 => "2s".to_string(),
        3 => "5s".to_string(),
        4 => "10s".to_string(),
        5 => "15s".to_string(),
        6 => "20s".to_string(),
        7 => "30s".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes burst rate
///
/// # Arguments
/// * `value` - Burst rate code
///
/// # Returns
/// Human-readable burst rate
fn decode_burst_rate(value: i16) -> String {
    match value {
        0 => "3/1s".to_string(),
        1 => "5/1s".to_string(),
        2 => "10/1s".to_string(),
        3 => "10/2s".to_string(),
        4 => "10/3s".to_string(),
        5 => "30/1s".to_string(),
        6 => "30/2s".to_string(),
        7 => "30/3s".to_string(),
        8 => "30/6s".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes camera orientation
///
/// # Arguments
/// * `value` - Orientation code
///
/// # Returns
/// Human-readable orientation
fn decode_orientation(value: i16) -> String {
    match value {
        0 => "Up".to_string(),
        1 => "Down".to_string(),
        2 => "Auto".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Formats exposure compensation
///
/// # Arguments
/// * `value` - EV value in tenths
///
/// # Returns
/// Formatted EV string
fn format_exposure(value: i16) -> String {
    let ev = value as f64 / 10.0;
    if ev >= 0.0 {
        format!("+{:.1} EV", ev)
    } else {
        format!("{:.1} EV", ev)
    }
}

/// Formats shutter speed
///
/// # Arguments
/// * `value` - Shutter speed as 1/n or exposure time in ms
///
/// # Returns
/// Formatted shutter speed string
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
/// Formatted zoom string
fn format_digital_zoom(value: i16) -> String {
    if value <= 100 {
        return "1.0x".to_string();
    }
    let zoom = value as f64 / 100.0;
    format!("{:.1}x", zoom)
}

/// Formats TimeWarp speed
///
/// # Arguments
/// * `value` - Speed multiplier
///
/// # Returns
/// Formatted speed string
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
/// Formatted interval string
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
/// Formatted bitrate string
fn format_bitrate(value: i16) -> String {
    if value <= 0 {
        return "Auto".to_string();
    }
    format!("{} Mbps", value)
}

/// Extracts an ASCII string from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the string
/// * `data` - Raw MakerNote data
///
/// # Returns
/// Extracted string or None if extraction fails
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

    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// GoPro MakerNote parser implementing the MakerNoteParser trait
/// Default implementation for parser
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
        if data.len() < 8 {
            return Err("GoPro MakerNote data too short".to_string());
        }

        // Skip GoPro signature if present
        let start_offset = if data.starts_with(GOPRO_SIGNATURE) {
            5
        } else {
            0
        };
        let parse_data = &data[start_offset..];

        if parse_data.len() < 2 {
            return Ok(());
        }

        // Read number of entries
        let num_entries = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([parse_data[0], parse_data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([parse_data[0], parse_data[1]]),
        } as usize;

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

            let tag = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[0], entry_data[1]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[0], entry_data[1]]),
            };

            let field_type = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[2], entry_data[3]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[2], entry_data[3]]),
            };

            let count = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
            };

            let value_offset = match byte_order {
                ByteOrder::LittleEndian => u32::from_le_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
                ByteOrder::BigEndian => u32::from_be_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
            };

            let entry = IfdEntry {
                tag_id: tag,
                field_type,
                value_count: count,
                value_offset,
            };

            // Extract value based on tag type
            match tag {
                GOPRO_VERSION | GOPRO_MODEL | GOPRO_SERIAL | GOPRO_LENS_MODEL => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            GOPRO_VERSION => "Version",
                            GOPRO_MODEL => "Model",
                            GOPRO_SERIAL => "SerialNumber",
                            GOPRO_LENS_MODEL => "LensModel",
                            _ => continue,
                        };
                        tags.insert(format!("GoPro:{}", tag_name), s);
                    }
                }

                _ => {
                    // Try to extract as i16 array
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                GOPRO_RESOLUTION => ("Resolution", decode_resolution(val)),
                                GOPRO_FRAME_RATE => ("FrameRate", decode_frame_rate(val)),
                                GOPRO_FOV => ("FieldOfView", decode_fov(val)),
                                GOPRO_LOW_LIGHT => (
                                    "LowLight",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_PROTUNE => (
                                    "Protune",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_WHITE_BALANCE => ("WhiteBalance", decode_white_balance(val)),
                                GOPRO_COLOR => ("ColorProfile", decode_color_profile(val)),
                                GOPRO_SHARPNESS => ("Sharpness", decode_sharpness(val)),
                                GOPRO_CONTRAST => ("Contrast", decode_contrast(val)),
                                GOPRO_SATURATION => ("Saturation", decode_saturation(val)),
                                GOPRO_ISO_MIN => ("ISOMin", val.to_string()),
                                GOPRO_ISO_MAX => ("ISOMax", val.to_string()),
                                GOPRO_EXPOSURE => ("ExposureCompensation", format_exposure(val)),
                                GOPRO_SHUTTER => ("ShutterSpeed", format_shutter(val)),
                                GOPRO_METERING => ("MeteringMode", decode_metering(val)),
                                GOPRO_SPOT_METER => (
                                    "SpotMeter",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_EIS => (
                                    "EIS",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_HYPERSMOOTH => ("HyperSmooth", decode_hypersmooth(val)),
                                GOPRO_BOOST => (
                                    "Boost",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_AUTO_BOOST => (
                                    "AutoBoost",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_SUPER_PHOTO => ("SuperPhoto", decode_super_photo(val)),
                                GOPRO_HDR => (
                                    "HDR",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_DIGITAL_ZOOM => ("DigitalZoom", format_digital_zoom(val)),
                                GOPRO_RAW_AUDIO => (
                                    "RawAudio",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_WIND_NOISE => (
                                    "WindNoiseReduction",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_TIMEWARP_SPEED => {
                                    ("TimeWarpSpeed", format_timewarp_speed(val))
                                }
                                GOPRO_VIDEO_ENCODING => {
                                    ("VideoEncoding", decode_video_encoding(val))
                                }
                                GOPRO_BIT_RATE => ("BitRate", format_bitrate(val)),
                                GOPRO_ORIENTATION => ("Orientation", decode_orientation(val)),
                                GOPRO_GPS_FIX => (
                                    "GPSFix",
                                    if val != 0 {
                                        "Yes".to_string()
                                    } else {
                                        "No".to_string()
                                    },
                                ),
                                GOPRO_NIGHT_PHOTO => ("NightPhoto", decode_night_photo(val)),
                                GOPRO_BURST_RATE => ("BurstRate", decode_burst_rate(val)),
                                GOPRO_LIVE_BURST => (
                                    "LiveBurst",
                                    if val != 0 {
                                        "On".to_string()
                                    } else {
                                        "Off".to_string()
                                    },
                                ),
                                GOPRO_TIMELAPSE_INTERVAL => {
                                    ("TimelapseInterval", format_interval(val))
                                }
                                GOPRO_NIGHT_LAPSE_INTERVAL => {
                                    ("NightLapseInterval", format_interval(val))
                                }
                                GOPRO_LOOP_DURATION => ("LoopDuration", format!("{} min", val)),
                                _ => continue,
                            };
                            tags.insert(format!("GoPro:{}", tag_name), formatted_value);
                        }
                    }
                }
            }

            offset += entry_size;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gopro_parser_creation() {
        let parser = GoProParser::new();
        assert_eq!(parser.manufacturer_name(), "GoPro");
        assert_eq!(parser.tag_prefix(), "GoPro:");
    }

    #[test]
    fn test_decode_fov() {
        assert_eq!(decode_fov(0), "Wide");
        assert_eq!(decode_fov(3), "Linear");
        assert_eq!(decode_fov(4), "SuperView");
    }

    #[test]
    fn test_decode_white_balance() {
        assert_eq!(decode_white_balance(0), "Auto");
        assert_eq!(decode_white_balance(7), "6500K");
        assert_eq!(decode_white_balance(8), "Native");
    }

    #[test]
    fn test_decode_color_profile() {
        assert_eq!(decode_color_profile(0), "GoPro Color");
        assert_eq!(decode_color_profile(1), "Flat");
    }

    #[test]
    fn test_decode_hypersmooth() {
        assert_eq!(decode_hypersmooth(0), "Off");
        assert_eq!(decode_hypersmooth(3), "Boost");
        assert_eq!(decode_hypersmooth(4), "Auto Boost");
    }

    #[test]
    fn test_decode_resolution() {
        assert_eq!(decode_resolution(0), "4K");
        assert_eq!(decode_resolution(6), "5.3K");
        assert_eq!(decode_resolution(10), "5.3K 4:3");
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
        assert_eq!(decode_burst_rate(5), "30/1s");
        assert_eq!(decode_burst_rate(2), "10/1s");
    }

    #[test]
    fn test_decode_night_photo() {
        assert_eq!(decode_night_photo(0), "Off");
        assert_eq!(decode_night_photo(4), "10s");
        assert_eq!(decode_night_photo(7), "30s");
    }
}
