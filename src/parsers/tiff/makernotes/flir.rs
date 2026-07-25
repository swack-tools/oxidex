//! FLIR Thermal Imaging Camera MakerNote parser
//!
//! Parses FLIR-specific EXIF MakerNote tags from thermal imaging cameras.
//! Contains temperature measurement data, thermal scales, emissivity settings,
//! and radiometric calibration information.
//!
//! ## Supported Models
//! - FLIR E-Series (E4, E5, E6, E8)
//! - FLIR T-Series (T420, T440, T540)
//! - FLIR P-Series (P620, P640, P660)
//! - FLIR A-Series (A65, A615, A655sc)
//! - FLIR AX8 (thermal/visual)
//! - FLIR ONE (smartphone attachment)
//! - FLIR C-Series (C2, C3, C5)
//! - FLIR i-Series (i3, i5, i7)
//!
//! ## Key Features
//! - Temperature measurement ranges (min/max)
//! - Emissivity setting (0.0-1.0)
//! - Reflected apparent temperature
//! - Atmospheric temperature
//! - Distance to object
//! - Relative humidity
//! - Thermal color palette
//! - Measurement spots (center, hot spot, cold spot)
//! - Temperature scale (Celsius/Fahrenheit/Kelvin)
//! - Calibration data
//! - Planck constants for radiometric conversion
//!
//! ## Architecture
//! FLIR stores thermal data in both MakerNotes and APP1 segments.
//! Raw thermal data is typically stored separately from the visible image.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_string};
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};

// Import registry module
use super::registries::flir_registry;

use crate::const_decoder;

// FLIR MakerNote Tag IDs
/// Camera model
pub const FLIR_MODEL: u16 = 0x0001;
/// Serial number
pub const FLIR_SERIAL: u16 = 0x0002;
/// Firmware version
pub const FLIR_FIRMWARE: u16 = 0x0003;
/// Minimum temperature in scene (K)
pub const FLIR_TEMPERATURE_MIN: u16 = 0x0100;
/// Maximum temperature in scene (K)
pub const FLIR_TEMPERATURE_MAX: u16 = 0x0101;
/// Center spot temperature (K)
pub const FLIR_TEMPERATURE_CENTER: u16 = 0x0102;
/// Emissivity (0-100, scale: 0.01)
pub const FLIR_EMISSIVITY: u16 = 0x0103;
/// Reflected apparent temp (K)
pub const FLIR_REFLECTED_TEMP: u16 = 0x0104;
/// Atmospheric temperature (K)
pub const FLIR_ATMOSPHERIC_TEMP: u16 = 0x0105;
/// Distance to object (meters)
pub const FLIR_DISTANCE: u16 = 0x0106;
/// Relative humidity (%)
pub const FLIR_HUMIDITY: u16 = 0x0107;
/// Color palette code
pub const FLIR_PALETTE: u16 = 0x0108;
/// Palette method (linear/histogram)
pub const FLIR_PALETTE_METHOD: u16 = 0x0109;
/// Palette stretch mode
pub const FLIR_PALETTE_STRETCH: u16 = 0x010A;
/// Camera range min (K)
pub const FLIR_TEMPERATURE_RANGE_MIN: u16 = 0x010B;
/// Camera range max (K)
pub const FLIR_TEMPERATURE_RANGE_MAX: u16 = 0x010C;
/// Atmospheric transmission (0-100)
pub const FLIR_ATMOSPHERIC_TRANS: u16 = 0x010D;
/// External optics temp (K)
pub const FLIR_EXTERNAL_OPTICS_TEMP: u16 = 0x010E;
/// External optics transmission
pub const FLIR_EXTERNAL_OPTICS_TRANS: u16 = 0x010F;
/// IR window temperature (K)
pub const FLIR_IR_WINDOW_TEMP: u16 = 0x0110;
/// IR window transmission
pub const FLIR_IR_WINDOW_TRANS: u16 = 0x0111;
/// Planck R1 constant
pub const FLIR_PLANCK_R1: u16 = 0x0112;
/// Planck R2 constant
pub const FLIR_PLANCK_R2: u16 = 0x0113;
/// Planck B constant
pub const FLIR_PLANCK_B: u16 = 0x0114;
/// Planck F constant
pub const FLIR_PLANCK_F: u16 = 0x0115;
/// Planck O constant (offset)
pub const FLIR_PLANCK_O: u16 = 0x0116;
/// Camera internal min temp (K)
pub const FLIR_CAMERA_TEMP_MIN: u16 = 0x0117;
/// Camera internal max temp (K)
pub const FLIR_CAMERA_TEMP_MAX: u16 = 0x0118;
/// Image type (thermal/visible)
pub const FLIR_IMAGE_TYPE: u16 = 0x0119;
/// Last calibration date
pub const FLIR_CALIBRATION_DATE: u16 = 0x011A;
/// Focus distance (meters)
pub const FLIR_FOCUS_DISTANCE: u16 = 0x011B;
/// Lens model identifier
pub const FLIR_LENS_MODEL: u16 = 0x011C;
/// Peak temperature in frame (K)
pub const FLIR_PEAK_TEMP: u16 = 0x011D;
/// Valley (coldest) temp (K)
pub const FLIR_VALLEY_TEMP: u16 = 0x011E;
/// Measurement mode
pub const FLIR_MEASUREMENT_MODE: u16 = 0x011F;
/// Display unit (C/F/K)
pub const FLIR_TEMPERATURE_UNIT: u16 = 0x0120;
/// Isotherm lower limit (K)
pub const FLIR_ISOTHERM_MIN: u16 = 0x0121;
/// Isotherm upper limit (K)
pub const FLIR_ISOTHERM_MAX: u16 = 0x0122;
/// Isotherm mode enabled
pub const FLIR_ISOTHERM_ENABLED: u16 = 0x0123;
/// Auto level/span mode
pub const FLIR_LEVEL_SPAN_AUTO: u16 = 0x0124;
/// Gain mode (auto/manual)
pub const FLIR_GAIN_MODE: u16 = 0x0125;
/// Frame rate (Hz)
pub const FLIR_FRAME_RATE: u16 = 0x0126;

// FLIR signature
const FLIR_SIGNATURE: &[u8] = b"FLIR";

// Decodes FLIR color palette type
const_decoder!(pub DECODE_PALETTE, i16, [
    (0, "Iron"),
    (1, "Rainbow"),
    (2, "White Hot"),
    (3, "Black Hot"),
    (4, "Arctic"),
    (5, "Lava"),
    (6, "Gray"),
    (7, "Rainbow HC"),
    (8, "Ironbow"),
    (9, "Medical"),
    (10, "Fusion"),
]);

// Decodes palette method
const_decoder!(pub DECODE_PALETTE_METHOD, i16, [
    (0, "Linear"),
    (1, "Histogram Equalization"),
    (2, "Adaptive"),
]);

// Decodes palette stretch mode
const_decoder!(pub DECODE_PALETTE_STRETCH, i16, [
    (0, "Manual"),
    (1, "Automatic"),
    (2, "Lock Range"),
]);

// Decodes image type
const_decoder!(pub DECODE_IMAGE_TYPE, i16, [
    (0, "Thermal"),
    (1, "Visual"),
    (2, "Thermal + Visual (PIP)"),
    (3, "Thermal + Visual (Blend)"),
]);

// Decodes temperature unit
const_decoder!(pub DECODE_TEMPERATURE_UNIT, i16, [
    (0, "°C"),
    (1, "°F"),
    (2, "K"),
]);

// Decodes measurement mode
const_decoder!(pub DECODE_MEASUREMENT_MODE, i16, [
    (0, "Spot Meter"),
    (1, "Area"),
    (2, "Line"),
    (3, "Delta T"),
]);

// Decodes gain mode
const_decoder!(pub DECODE_GAIN_MODE, i16, [
    (0, "Automatic"),
    (1, "Manual Low"),
    (2, "Manual High"),
]);

/// Converts Kelvin to Celsius
///
/// # Arguments
/// * `kelvin` - Temperature in Kelvin (scaled by 100)
///
/// # Returns
/// Formatted Celsius string
fn kelvin_to_celsius(kelvin: i16) -> String {
    let k = kelvin as f64 / 100.0;
    let c = k - 273.15;
    format!("{:.2}°C", c)
}

/// Converts Kelvin (i16) to Celsius with full precision
///
/// # Arguments
/// * `kelvin` - Temperature in Kelvin (scaled)
///
/// # Returns
/// Formatted Celsius string
fn format_temperature_kelvin(kelvin: i16) -> String {
    kelvin_to_celsius(kelvin)
}

/// Formats emissivity value
///
/// # Arguments
/// * `value` - Emissivity (0-100, scale: 0.01)
///
/// # Returns
/// Formatted emissivity (0.00-1.00)
fn format_emissivity(value: i16) -> String {
    let emissivity = value as f64 / 100.0;
    format!("{:.2}", emissivity)
}

/// Formats distance
///
/// # Arguments
/// * `value` - Distance in centimeters
///
/// # Returns
/// Formatted distance in meters
fn format_distance(value: i16) -> String {
    let meters = value as f64 / 100.0;
    if meters < 1.0 {
        format!("{:.0} cm", value)
    } else {
        format!("{:.2} m", meters)
    }
}

/// Formats humidity percentage
///
/// # Arguments
/// * `value` - Humidity percentage
///
/// # Returns
/// Formatted humidity string
fn format_humidity(value: i16) -> String {
    if !(0..=100).contains(&value) {
        return "Unknown".to_string();
    }
    format!("{}%", value)
}

/// Formats transmission percentage
///
/// # Arguments
/// * `value` - Transmission (0-100)
///
/// # Returns
/// Formatted transmission string
fn format_transmission(value: i16) -> String {
    if !(0..=100).contains(&value) {
        return "Unknown".to_string();
    }
    format!("{}%", value)
}

/// Formats Planck constant
///
/// # Arguments
/// * `value` - Planck constant as integer
///
/// # Returns
/// Formatted scientific notation
fn format_planck_constant(value: i16) -> String {
    // Planck constants are typically stored as scaled integers
    format!("{}", value)
}

/// Formats frame rate
///
/// # Arguments
/// * `value` - Frame rate in Hz
///
/// # Returns
/// Formatted frame rate string
fn format_frame_rate(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    format!("{} Hz", value)
}

// Note: extract_string is imported from super::shared::array_extractors above
// and reused here. No local redefinition needed.

/// FLIR Thermal Camera MakerNote parser
/// Default implementation for parser
#[derive(Default)]
pub struct FlirParser;

impl FlirParser {
    /// Creates a new FLIR parser instance
    pub fn new() -> Self {
        FlirParser
    }
}

impl MakerNoteParser for FlirParser {
    fn manufacturer_name(&self) -> &'static str {
        "FLIR"
    }

    fn tag_prefix(&self) -> &'static str {
        "FLIR:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 4 {
            return false;
        }
        data.starts_with(FLIR_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Configure IFD parser for FLIR MakerNote format
        let config = IfdParserConfig {
            signature: Some(FLIR_SIGNATURE),
            signature_offset: 4,
            max_entries: 200,
        };

        // Create registry on-demand
        let registry = flir_registry();

        // Use shared IFD parser to eliminate boilerplate
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            // Handle string tags
            if matches!(
                entry.tag_id,
                FLIR_MODEL | FLIR_SERIAL | FLIR_FIRMWARE | FLIR_LENS_MODEL | FLIR_CALIBRATION_DATE
            ) {
                if let Some(s) = extract_string(entry, parse_data, byte_order) {
                    let tag_name = match entry.tag_id {
                        FLIR_MODEL => "Model",
                        FLIR_SERIAL => "SerialNumber",
                        FLIR_FIRMWARE => "FirmwareVersion",
                        FLIR_LENS_MODEL => "LensModel",
                        FLIR_CALIBRATION_DATE => "CalibrationDate",
                        _ => return,
                    };
                    tags.insert(format!("FLIR:{}", tag_name), s);
                }
            } else {
                // Try to extract as i16 array
                if let Some(array) = extract_i16_array(entry, parse_data, byte_order)
                    && let Some(&val) = array.first()
                {
                    // Registry lookup: get tag name and decode value
                    if let Some(tag_name) = registry.get_tag_name(entry.tag_id) {
                        let formatted_value = registry.decode_i16(entry.tag_id, val);
                        tags.insert(format!("FLIR:{}", tag_name), formatted_value);
                    }
                }
            }
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flir_parser_creation() {
        let parser = FlirParser::new();
        assert_eq!(parser.manufacturer_name(), "FLIR");
        assert_eq!(parser.tag_prefix(), "FLIR:");
    }

    #[test]
    fn test_decode_palette() {
        assert_eq!(DECODE_PALETTE.decode(0), "Iron");
        assert_eq!(DECODE_PALETTE.decode(2), "White Hot");
        assert_eq!(DECODE_PALETTE.decode(5), "Lava");
        assert_eq!(DECODE_PALETTE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_image_type() {
        assert_eq!(DECODE_IMAGE_TYPE.decode(0), "Thermal");
        assert_eq!(DECODE_IMAGE_TYPE.decode(2), "Thermal + Visual (PIP)");
        assert_eq!(DECODE_IMAGE_TYPE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_kelvin_to_celsius() {
        assert_eq!(kelvin_to_celsius(29315), "20.00°C"); // 293.15K = 20°C
        assert_eq!(kelvin_to_celsius(27315), "0.00°C"); // 273.15K = 0°C
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
    fn test_format_transmission() {
        assert_eq!(format_transmission(85), "85%");
        assert_eq!(format_transmission(100), "100%");
    }

    #[test]
    fn test_decode_temperature_unit() {
        assert_eq!(DECODE_TEMPERATURE_UNIT.decode(0), "°C");
        assert_eq!(DECODE_TEMPERATURE_UNIT.decode(1), "°F");
        assert_eq!(DECODE_TEMPERATURE_UNIT.decode(2), "K");
        assert_eq!(DECODE_TEMPERATURE_UNIT.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_gain_mode() {
        assert_eq!(DECODE_GAIN_MODE.decode(0), "Automatic");
        assert_eq!(DECODE_GAIN_MODE.decode(1), "Manual Low");
        assert_eq!(DECODE_GAIN_MODE.decode(2), "Manual High");
        assert_eq!(DECODE_GAIN_MODE.decode(99), "Unknown (99)");
    }
}
