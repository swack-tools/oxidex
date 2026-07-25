//! GoPro tag registry
//!
//! Registry of all GoPro MakerNote tags with their metadata and decoders.
//! Supports HERO series action cameras from HERO4 through HERO12 Black.

use super::super::shared::generic_decoders::{ON_OFF, YES_NO};
use super::super::shared::tag_registry::TagRegistry;

// Re-export tag constants from gopro.rs
use super::super::gopro::{
    GOPRO_AUTO_BOOST, GOPRO_BIT_RATE, GOPRO_BOOST, GOPRO_BURST_RATE, GOPRO_COLOR, GOPRO_CONTRAST,
    GOPRO_DIGITAL_ZOOM, GOPRO_EIS, GOPRO_EXPOSURE, GOPRO_FOV, GOPRO_FRAME_RATE, GOPRO_GPS_FIX,
    GOPRO_HDR, GOPRO_HYPERSMOOTH, GOPRO_ISO_MAX, GOPRO_ISO_MIN, GOPRO_LIVE_BURST,
    GOPRO_LOOP_DURATION, GOPRO_LOW_LIGHT, GOPRO_METERING, GOPRO_NIGHT_LAPSE_INTERVAL,
    GOPRO_NIGHT_PHOTO, GOPRO_ORIENTATION, GOPRO_PROTUNE, GOPRO_RAW_AUDIO, GOPRO_RESOLUTION,
    GOPRO_SATURATION, GOPRO_SHARPNESS, GOPRO_SHUTTER, GOPRO_SPOT_METER, GOPRO_SUPER_PHOTO,
    GOPRO_TIMELAPSE_INTERVAL, GOPRO_TIMEWARP_SPEED, GOPRO_VIDEO_ENCODING, GOPRO_WHITE_BALANCE,
    GOPRO_WIND_NOISE,
};

// Re-export decoders from gopro.rs
use super::super::gopro::{
    BURST_RATE, COLOR_PROFILE, CONTRAST, FOV, HYPERSMOOTH, METERING, NIGHT_PHOTO, ORIENTATION,
    RESOLUTION, SATURATION, SHARPNESS, SUPER_PHOTO, VIDEO_ENCODING, WHITE_BALANCE,
};

// ============================================================================
// Custom Formatter Functions
// ============================================================================
// These functions handle values that require mathematical transformations
// or special formatting logic that can't be handled by simple const decoders.

/// Formats frame rate value
fn format_frame_rate(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    format!("{} fps", value)
}

/// Formats exposure compensation value
fn format_exposure(value: i16) -> String {
    let ev = value as f64 / 10.0;
    if ev >= 0.0 {
        format!("+{:.1} EV", ev)
    } else {
        format!("{:.1} EV", ev)
    }
}

/// Formats shutter speed value
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
fn format_digital_zoom(value: i16) -> String {
    if value <= 100 {
        return "1.0x".to_string();
    }
    let zoom = value as f64 / 100.0;
    format!("{:.1}x", zoom)
}

/// Formats TimeWarp speed multiplier
fn format_timewarp_speed(value: i16) -> String {
    if value <= 0 {
        return "Auto".to_string();
    }
    format!("{}x", value)
}

/// Formats time lapse interval
fn format_interval(value: i16) -> String {
    if value < 1000 {
        format!("{} ms", value)
    } else {
        let seconds = value as f64 / 1000.0;
        format!("{:.1} s", seconds)
    }
}

/// Formats video bitrate
fn format_bitrate(value: i16) -> String {
    if value <= 0 {
        return "Auto".to_string();
    }
    format!("{} Mbps", value)
}

/// Formats loop duration
fn format_loop_duration(value: i16) -> String {
    format!("{} min", value)
}

/// Decodes boolean value to ON/OFF
fn decode_on_off(value: i16) -> String {
    ON_OFF.decode(if value != 0 { 1 } else { 0 })
}

/// Decodes boolean value to YES/NO
fn decode_yes_no(value: i16) -> String {
    YES_NO.decode(if value != 0 { 1 } else { 0 })
}

// ============================================================================
// Tag Registry
// ============================================================================

/// Create and return the GoPro tag registry
///
/// This registry contains all known GoPro MakerNote tags including:
/// - Video resolution and frame rate settings
/// - Field of view and lens settings
/// - Protune parameters (white balance, color, sharpness, etc.)
/// - Stabilization and auto-enhancement modes
/// - Photo modes (burst, night, time lapse)
/// - Audio settings
pub fn gopro_registry() -> TagRegistry {
    TagRegistry::with_capacity(40)
        // Simple i16 decoders - enum-like value mappings
        .register_simple_i16(GOPRO_RESOLUTION, "Resolution", &RESOLUTION)
        .register_simple_i16(GOPRO_FOV, "FieldOfView", &FOV)
        .register_simple_i16(GOPRO_WHITE_BALANCE, "WhiteBalance", &WHITE_BALANCE)
        .register_simple_i16(GOPRO_COLOR, "ColorProfile", &COLOR_PROFILE)
        .register_simple_i16(GOPRO_SHARPNESS, "Sharpness", &SHARPNESS)
        .register_simple_i16(GOPRO_CONTRAST, "Contrast", &CONTRAST)
        .register_simple_i16(GOPRO_SATURATION, "Saturation", &SATURATION)
        .register_simple_i16(GOPRO_METERING, "MeteringMode", &METERING)
        .register_simple_i16(GOPRO_HYPERSMOOTH, "HyperSmooth", &HYPERSMOOTH)
        .register_simple_i16(GOPRO_VIDEO_ENCODING, "VideoEncoding", &VIDEO_ENCODING)
        .register_simple_i16(GOPRO_SUPER_PHOTO, "SuperPhoto", &SUPER_PHOTO)
        .register_simple_i16(GOPRO_NIGHT_PHOTO, "NightPhoto", &NIGHT_PHOTO)
        .register_simple_i16(GOPRO_BURST_RATE, "BurstRate", &BURST_RATE)
        .register_simple_i16(GOPRO_ORIENTATION, "Orientation", &ORIENTATION)
        // ON/OFF boolean tags - use helper function for boolean conversion
        .register_i16(GOPRO_LOW_LIGHT, "LowLight", decode_on_off)
        .register_i16(GOPRO_PROTUNE, "Protune", decode_on_off)
        .register_i16(GOPRO_SPOT_METER, "SpotMeter", decode_on_off)
        .register_i16(GOPRO_EIS, "EIS", decode_on_off)
        .register_i16(GOPRO_BOOST, "Boost", decode_on_off)
        .register_i16(GOPRO_AUTO_BOOST, "AutoBoost", decode_on_off)
        .register_i16(GOPRO_HDR, "HDR", decode_on_off)
        .register_i16(GOPRO_RAW_AUDIO, "RawAudio", decode_on_off)
        .register_i16(GOPRO_WIND_NOISE, "WindNoiseReduction", decode_on_off)
        .register_i16(GOPRO_LIVE_BURST, "LiveBurst", decode_on_off)
        // YES/NO boolean tag
        .register_i16(GOPRO_GPS_FIX, "GPSFix", decode_yes_no)
        // Custom i16 decoders - require mathematical transformations
        .register_i16(GOPRO_FRAME_RATE, "FrameRate", format_frame_rate)
        .register_i16(GOPRO_EXPOSURE, "ExposureCompensation", format_exposure)
        .register_i16(GOPRO_SHUTTER, "ShutterSpeed", format_shutter)
        .register_i16(GOPRO_DIGITAL_ZOOM, "DigitalZoom", format_digital_zoom)
        .register_i16(GOPRO_TIMEWARP_SPEED, "TimeWarpSpeed", format_timewarp_speed)
        .register_i16(GOPRO_BIT_RATE, "BitRate", format_bitrate)
        .register_i16(
            GOPRO_TIMELAPSE_INTERVAL,
            "TimelapseInterval",
            format_interval,
        )
        .register_i16(
            GOPRO_NIGHT_LAPSE_INTERVAL,
            "NightLapseInterval",
            format_interval,
        )
        .register_i16(GOPRO_LOOP_DURATION, "LoopDuration", format_loop_duration)
        // Raw value tags - no decoding needed, displayed as-is
        .register_raw(GOPRO_ISO_MIN, "ISOMin")
        .register_raw(GOPRO_ISO_MAX, "ISOMax")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = gopro_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(GOPRO_RESOLUTION));
        assert!(registry.has_tag(GOPRO_FOV));
        assert!(registry.has_tag(GOPRO_HYPERSMOOTH));
        assert!(registry.has_tag(GOPRO_FRAME_RATE));
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = gopro_registry();

        assert_eq!(registry.get_tag_name(GOPRO_RESOLUTION), Some("Resolution"));
        assert_eq!(registry.get_tag_name(GOPRO_FOV), Some("FieldOfView"));
        assert_eq!(
            registry.get_tag_name(GOPRO_HYPERSMOOTH),
            Some("HyperSmooth")
        );
    }

    #[test]
    fn test_unknown_tag() {
        let registry = gopro_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
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
}
