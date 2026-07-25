//! Lytro light field camera tag registry
//!
//! Registry of all Lytro MakerNote tags with their metadata and decoders.
//! Supports light field cameras including original Lytro, Lytro ILLUM, and Lytro Cinema.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// Re-export tag constants from lytro.rs
use super::super::lytro::{
    LYTRO_DEPTH_MAP_ENABLED, LYTRO_DEPTH_MAX, LYTRO_DEPTH_MIN, LYTRO_EXPOSURE_DURATION,
    LYTRO_FOCUS_DEPTH, LYTRO_IMAGE_ORIENTATION, LYTRO_ISO_SPEED, LYTRO_MICROLENS_PITCH,
    LYTRO_MICROLENS_ROTATION, LYTRO_PERSPECTIVE_SHIFT, LYTRO_RAW_DATA_SIZE, LYTRO_REFOCUS_RANGE,
    LYTRO_SENSOR_RESOLUTION, LYTRO_TEMPERATURE, LYTRO_ZOOM_FACTOR,
};

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

// Decoder for sensor resolution
const_decoder!(
    SENSOR_RESOLUTION,
    i16,
    [
        (0, "Standard (1080x1080)"),
        (1, "High (2450x1634)"),
        (2, "Ultra (3280x3280)"),
    ]
);

// Decoder for image orientation
const_decoder!(
    IMAGE_ORIENTATION,
    i16,
    [
        (0, "Horizontal"),
        (1, "Rotate 90 CW"),
        (2, "Rotate 180"),
        (3, "Rotate 270 CW"),
    ]
);

// ============================================================================
// Custom Formatter Functions
// ============================================================================
// These functions handle values that require mathematical transformations
// or special formatting logic that can't be handled by simple const decoders.

/// Formats depth value
fn format_depth(value: i16) -> String {
    if value < 1000 {
        format!("{} mm", value)
    } else {
        format!("{:.2} m", value as f64 / 1000.0)
    }
}

/// Formats microlens pitch
fn format_microlens_pitch(value: i16) -> String {
    format!("{} µm", value)
}

/// Formats microlens rotation angle
fn format_rotation(value: i16) -> String {
    format!("{:.2}°", value as f64 / 100.0)
}

/// Formats exposure duration
fn format_exposure(value: i16) -> String {
    if value < 1000 {
        format!("{} ms", value)
    } else {
        format!("{:.2} s", value as f64 / 1000.0)
    }
}

/// Formats zoom factor
fn format_zoom(value: i16) -> String {
    format!("{:.2}x", value as f64 / 100.0)
}

/// Formats temperature
fn format_temperature(value: i16) -> String {
    format!("{}°C", value)
}

/// Formats raw data size
fn format_data_size(value: i16) -> String {
    if value < 1024 {
        format!("{} MB", value)
    } else {
        format!("{:.2} GB", value as f64 / 1024.0)
    }
}

/// Decodes yes/no boolean values
fn decode_yes_no(value: i16) -> String {
    if value != 0 {
        "Yes".to_string()
    } else {
        "No".to_string()
    }
}

/// Formats ISO speed value
fn format_iso(value: i16) -> String {
    value.to_string()
}

// ============================================================================
// Tag Registry
// ============================================================================

/// Create and return the Lytro tag registry
///
/// This registry contains all known Lytro MakerNote tags including:
/// - Light field data version and processing information
/// - Microlens array specifications
/// - Depth mapping and focus plane data
/// - Sensor and acquisition settings
/// - Calibration information
pub fn lytro_registry() -> TagRegistry {
    TagRegistry::with_capacity(16)
        // Microlens array specifications
        .register_i16(
            LYTRO_MICROLENS_PITCH,
            "MicrolensPitch",
            format_microlens_pitch,
        )
        .register_i16(
            LYTRO_MICROLENS_ROTATION,
            "MicrolensRotation",
            format_rotation,
        )
        // Depth mapping parameters
        .register_i16(LYTRO_DEPTH_MIN, "DepthMin", format_depth)
        .register_i16(LYTRO_DEPTH_MAX, "DepthMax", format_depth)
        .register_i16(LYTRO_FOCUS_DEPTH, "FocusDepth", format_depth)
        .register_i16(LYTRO_REFOCUS_RANGE, "RefocusRange", format_depth)
        // Sensor and image properties
        .register_simple_i16(
            LYTRO_SENSOR_RESOLUTION,
            "SensorResolution",
            &SENSOR_RESOLUTION,
        )
        .register_simple_i16(
            LYTRO_IMAGE_ORIENTATION,
            "ImageOrientation",
            &IMAGE_ORIENTATION,
        )
        // Acquisition settings
        .register_i16(LYTRO_EXPOSURE_DURATION, "ExposureDuration", format_exposure)
        .register_i16(LYTRO_ISO_SPEED, "ISO", format_iso)
        .register_i16(LYTRO_ZOOM_FACTOR, "ZoomFactor", format_zoom)
        // Processing and capability flags
        .register_i16(LYTRO_DEPTH_MAP_ENABLED, "DepthMapEnabled", decode_yes_no)
        .register_i16(
            LYTRO_PERSPECTIVE_SHIFT,
            "PerspectiveShiftCapable",
            decode_yes_no,
        )
        // Sensor monitoring
        .register_i16(LYTRO_TEMPERATURE, "SensorTemperature", format_temperature)
        // Data size information
        .register_i16(LYTRO_RAW_DATA_SIZE, "RawDataSize", format_data_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = lytro_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(LYTRO_DEPTH_MIN));
        assert!(registry.has_tag(LYTRO_SENSOR_RESOLUTION));
        assert!(registry.has_tag(LYTRO_ZOOM_FACTOR));
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = lytro_registry();

        assert_eq!(registry.get_tag_name(LYTRO_DEPTH_MIN), Some("DepthMin"));
        assert_eq!(
            registry.get_tag_name(LYTRO_SENSOR_RESOLUTION),
            Some("SensorResolution")
        );
        assert_eq!(registry.get_tag_name(LYTRO_ZOOM_FACTOR), Some("ZoomFactor"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = lytro_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }

    #[test]
    fn test_format_depth() {
        assert_eq!(format_depth(500), "500 mm");
        assert_eq!(format_depth(2500), "2.50 m");
    }

    #[test]
    fn test_format_zoom() {
        assert_eq!(format_zoom(100), "1.00x");
        assert_eq!(format_zoom(800), "8.00x");
    }

    #[test]
    fn test_format_microlens_pitch() {
        assert_eq!(format_microlens_pitch(14), "14 µm");
    }

    #[test]
    fn test_format_rotation() {
        assert_eq!(format_rotation(0), "0.00°");
        assert_eq!(format_rotation(450), "4.50°");
    }
}
