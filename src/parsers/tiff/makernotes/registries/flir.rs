//! FLIR thermal camera tag registry
//!
//! Registry of all FLIR MakerNote tags with their metadata and decoders.
//! Supports thermal imaging cameras from E-Series, T-Series, P-Series, and other FLIR models.

use super::super::shared::tag_registry::TagRegistry;

// Re-export tag constants from flir.rs
use super::super::flir::{
    FLIR_ATMOSPHERIC_TEMP, FLIR_ATMOSPHERIC_TRANS, FLIR_CAMERA_TEMP_MAX, FLIR_CAMERA_TEMP_MIN,
    FLIR_DISTANCE, FLIR_EMISSIVITY, FLIR_EXTERNAL_OPTICS_TEMP, FLIR_EXTERNAL_OPTICS_TRANS,
    FLIR_FOCUS_DISTANCE, FLIR_FRAME_RATE, FLIR_GAIN_MODE, FLIR_HUMIDITY, FLIR_IMAGE_TYPE,
    FLIR_IR_WINDOW_TEMP, FLIR_IR_WINDOW_TRANS, FLIR_ISOTHERM_ENABLED, FLIR_ISOTHERM_MAX,
    FLIR_ISOTHERM_MIN, FLIR_LEVEL_SPAN_AUTO, FLIR_MEASUREMENT_MODE, FLIR_PALETTE,
    FLIR_PALETTE_METHOD, FLIR_PALETTE_STRETCH, FLIR_PEAK_TEMP, FLIR_PLANCK_B, FLIR_PLANCK_F,
    FLIR_PLANCK_O, FLIR_PLANCK_R1, FLIR_PLANCK_R2, FLIR_REFLECTED_TEMP, FLIR_TEMPERATURE_CENTER,
    FLIR_TEMPERATURE_MAX, FLIR_TEMPERATURE_MIN, FLIR_TEMPERATURE_RANGE_MAX,
    FLIR_TEMPERATURE_RANGE_MIN, FLIR_TEMPERATURE_UNIT, FLIR_VALLEY_TEMP,
};

// Re-export decoders from flir.rs
use super::super::flir::{
    DECODE_GAIN_MODE, DECODE_IMAGE_TYPE, DECODE_MEASUREMENT_MODE, DECODE_PALETTE,
    DECODE_PALETTE_METHOD, DECODE_PALETTE_STRETCH, DECODE_TEMPERATURE_UNIT,
};

// ============================================================================
// Custom Formatter Functions
// ============================================================================
// These functions handle values that require mathematical transformations
// or special formatting logic that can't be handled by simple const decoders.

/// Converts Kelvin to Celsius
fn kelvin_to_celsius(kelvin: i16) -> String {
    let k = kelvin as f64 / 100.0;
    let c = k - 273.15;
    format!("{:.2}°C", c)
}

/// Formats emissivity value
fn format_emissivity(value: i16) -> String {
    let emissivity = value as f64 / 100.0;
    format!("{:.2}", emissivity)
}

/// Formats distance
fn format_distance(value: i16) -> String {
    let meters = value as f64 / 100.0;
    if meters < 1.0 {
        format!("{:.0} cm", value)
    } else {
        format!("{:.2} m", meters)
    }
}

/// Formats humidity percentage
fn format_humidity(value: i16) -> String {
    if !(0..=100).contains(&value) {
        return "Unknown".to_string();
    }
    format!("{}%", value)
}

/// Formats transmission percentage
fn format_transmission(value: i16) -> String {
    if !(0..=100).contains(&value) {
        return "Unknown".to_string();
    }
    format!("{}%", value)
}

/// Formats Planck constant
fn format_planck_constant(value: i16) -> String {
    // Planck constants are typically stored as scaled integers
    format!("{}", value)
}

/// Formats frame rate
fn format_frame_rate(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    format!("{} Hz", value)
}

/// Decodes yes/no boolean values
fn decode_yes_no(value: i16) -> String {
    if value != 0 {
        "Yes".to_string()
    } else {
        "No".to_string()
    }
}

// ============================================================================
// Tag Registry
// ============================================================================

/// Create and return the FLIR tag registry
///
/// This registry contains all known FLIR MakerNote tags including:
/// - Temperature measurement ranges (min/max/center)
/// - Thermal scales and color palettes
/// - Emissivity and environmental settings
/// - Radiometric calibration constants
/// - Image processing modes and parameters
pub fn flir_registry() -> TagRegistry {
    TagRegistry::with_capacity(32)
        // Temperature measurements - all converted from Kelvin to Celsius
        .register_i16(FLIR_TEMPERATURE_MIN, "TemperatureMin", kelvin_to_celsius)
        .register_i16(FLIR_TEMPERATURE_MAX, "TemperatureMax", kelvin_to_celsius)
        .register_i16(
            FLIR_TEMPERATURE_CENTER,
            "TemperatureCenter",
            kelvin_to_celsius,
        )
        .register_i16(
            FLIR_REFLECTED_TEMP,
            "ReflectedTemperature",
            kelvin_to_celsius,
        )
        .register_i16(
            FLIR_ATMOSPHERIC_TEMP,
            "AtmosphericTemperature",
            kelvin_to_celsius,
        )
        .register_i16(
            FLIR_EXTERNAL_OPTICS_TEMP,
            "ExternalOpticsTemperature",
            kelvin_to_celsius,
        )
        .register_i16(
            FLIR_IR_WINDOW_TEMP,
            "IRWindowTemperature",
            kelvin_to_celsius,
        )
        .register_i16(
            FLIR_CAMERA_TEMP_MIN,
            "CameraInternalTempMin",
            kelvin_to_celsius,
        )
        .register_i16(
            FLIR_CAMERA_TEMP_MAX,
            "CameraInternalTempMax",
            kelvin_to_celsius,
        )
        .register_i16(FLIR_PEAK_TEMP, "PeakTemperature", kelvin_to_celsius)
        .register_i16(FLIR_VALLEY_TEMP, "ValleyTemperature", kelvin_to_celsius)
        .register_i16(FLIR_ISOTHERM_MIN, "IsothermMin", kelvin_to_celsius)
        .register_i16(FLIR_ISOTHERM_MAX, "IsothermMax", kelvin_to_celsius)
        .register_i16(FLIR_TEMPERATURE_RANGE_MIN, "RangeMin", kelvin_to_celsius)
        .register_i16(FLIR_TEMPERATURE_RANGE_MAX, "RangeMax", kelvin_to_celsius)
        // Emissivity and optical properties
        .register_i16(FLIR_EMISSIVITY, "Emissivity", format_emissivity)
        .register_i16(FLIR_DISTANCE, "Distance", format_distance)
        .register_i16(FLIR_FOCUS_DISTANCE, "FocusDistance", format_distance)
        // Environmental parameters
        .register_i16(FLIR_HUMIDITY, "RelativeHumidity", format_humidity)
        .register_i16(
            FLIR_ATMOSPHERIC_TRANS,
            "AtmosphericTransmission",
            format_transmission,
        )
        .register_i16(
            FLIR_EXTERNAL_OPTICS_TRANS,
            "ExternalOpticsTransmission",
            format_transmission,
        )
        .register_i16(
            FLIR_IR_WINDOW_TRANS,
            "IRWindowTransmission",
            format_transmission,
        )
        // Radiometric calibration constants
        .register_i16(FLIR_PLANCK_R1, "PlanckR1", format_planck_constant)
        .register_i16(FLIR_PLANCK_R2, "PlanckR2", format_planck_constant)
        .register_i16(FLIR_PLANCK_B, "PlanckB", format_planck_constant)
        .register_i16(FLIR_PLANCK_F, "PlanckF", format_planck_constant)
        .register_i16(FLIR_PLANCK_O, "PlanckO", format_planck_constant)
        // Display and processing modes
        .register_simple_i16(FLIR_PALETTE, "Palette", &DECODE_PALETTE)
        .register_simple_i16(FLIR_PALETTE_METHOD, "PaletteMethod", &DECODE_PALETTE_METHOD)
        .register_simple_i16(
            FLIR_PALETTE_STRETCH,
            "PaletteStretch",
            &DECODE_PALETTE_STRETCH,
        )
        .register_simple_i16(FLIR_IMAGE_TYPE, "ImageType", &DECODE_IMAGE_TYPE)
        .register_simple_i16(
            FLIR_MEASUREMENT_MODE,
            "MeasurementMode",
            &DECODE_MEASUREMENT_MODE,
        )
        .register_simple_i16(
            FLIR_TEMPERATURE_UNIT,
            "TemperatureUnit",
            &DECODE_TEMPERATURE_UNIT,
        )
        .register_simple_i16(FLIR_GAIN_MODE, "GainMode", &DECODE_GAIN_MODE)
        // Boolean flags
        .register_i16(FLIR_ISOTHERM_ENABLED, "IsothermEnabled", decode_yes_no)
        .register_i16(FLIR_LEVEL_SPAN_AUTO, "LevelSpanAuto", decode_yes_no)
        // Acquisition parameters
        .register_i16(FLIR_FRAME_RATE, "FrameRate", format_frame_rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = flir_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(FLIR_TEMPERATURE_MIN));
        assert!(registry.has_tag(FLIR_EMISSIVITY));
        assert!(registry.has_tag(FLIR_PALETTE));
        assert!(registry.has_tag(FLIR_MEASUREMENT_MODE));
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = flir_registry();

        assert_eq!(
            registry.get_tag_name(FLIR_TEMPERATURE_MIN),
            Some("TemperatureMin")
        );
        assert_eq!(registry.get_tag_name(FLIR_EMISSIVITY), Some("Emissivity"));
        assert_eq!(registry.get_tag_name(FLIR_PALETTE), Some("Palette"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = flir_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }

    #[test]
    fn test_format_emissivity() {
        assert_eq!(format_emissivity(95), "0.95");
        assert_eq!(format_emissivity(100), "1.00");
        assert_eq!(format_emissivity(50), "0.50");
    }

    #[test]
    fn test_format_distance() {
        assert_eq!(format_distance(50), "50 cm");
        assert_eq!(format_distance(150), "1.50 m");
        assert_eq!(format_distance(1000), "10.00 m");
    }

    #[test]
    fn test_format_humidity() {
        assert_eq!(format_humidity(50), "50%");
        assert_eq!(format_humidity(100), "100%");
        assert_eq!(format_humidity(0), "0%");
    }

    #[test]
    fn test_kelvin_to_celsius() {
        assert_eq!(kelvin_to_celsius(29315), "20.00°C"); // 293.15K = 20°C
        assert_eq!(kelvin_to_celsius(27315), "0.00°C"); // 273.15K = 0°C
    }
}
