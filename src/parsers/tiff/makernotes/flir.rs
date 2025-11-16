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

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// FLIR MakerNote Tag IDs
const FLIR_MODEL: u16 = 0x0001; // Camera model
const FLIR_SERIAL: u16 = 0x0002; // Serial number
const FLIR_FIRMWARE: u16 = 0x0003; // Firmware version
const FLIR_TEMPERATURE_MIN: u16 = 0x0100; // Minimum temperature in scene (K)
const FLIR_TEMPERATURE_MAX: u16 = 0x0101; // Maximum temperature in scene (K)
const FLIR_TEMPERATURE_CENTER: u16 = 0x0102; // Center spot temperature (K)
const FLIR_EMISSIVITY: u16 = 0x0103; // Emissivity (0-100, scale: 0.01)
const FLIR_REFLECTED_TEMP: u16 = 0x0104; // Reflected apparent temp (K)
const FLIR_ATMOSPHERIC_TEMP: u16 = 0x0105; // Atmospheric temperature (K)
const FLIR_DISTANCE: u16 = 0x0106; // Distance to object (meters)
const FLIR_HUMIDITY: u16 = 0x0107; // Relative humidity (%)
const FLIR_PALETTE: u16 = 0x0108; // Color palette code
const FLIR_PALETTE_METHOD: u16 = 0x0109; // Palette method (linear/histogram)
const FLIR_PALETTE_STRETCH: u16 = 0x010A; // Palette stretch mode
const FLIR_TEMPERATURE_RANGE_MIN: u16 = 0x010B; // Camera range min (K)
const FLIR_TEMPERATURE_RANGE_MAX: u16 = 0x010C; // Camera range max (K)
const FLIR_ATMOSPHERIC_TRANS: u16 = 0x010D; // Atmospheric transmission (0-100)
const FLIR_EXTERNAL_OPTICS_TEMP: u16 = 0x010E; // External optics temp (K)
const FLIR_EXTERNAL_OPTICS_TRANS: u16 = 0x010F; // External optics transmission
const FLIR_IR_WINDOW_TEMP: u16 = 0x0110; // IR window temperature (K)
const FLIR_IR_WINDOW_TRANS: u16 = 0x0111; // IR window transmission
const FLIR_PLANCK_R1: u16 = 0x0112; // Planck R1 constant
const FLIR_PLANCK_R2: u16 = 0x0113; // Planck R2 constant
const FLIR_PLANCK_B: u16 = 0x0114; // Planck B constant
const FLIR_PLANCK_F: u16 = 0x0115; // Planck F constant
const FLIR_PLANCK_O: u16 = 0x0116; // Planck O constant (offset)
const FLIR_CAMERA_TEMP_MIN: u16 = 0x0117; // Camera internal min temp (K)
const FLIR_CAMERA_TEMP_MAX: u16 = 0x0118; // Camera internal max temp (K)
const FLIR_IMAGE_TYPE: u16 = 0x0119; // Image type (thermal/visible)
const FLIR_CALIBRATION_DATE: u16 = 0x011A; // Last calibration date
const FLIR_FOCUS_DISTANCE: u16 = 0x011B; // Focus distance (meters)
const FLIR_LENS_MODEL: u16 = 0x011C; // Lens model identifier
const FLIR_PEAK_TEMP: u16 = 0x011D; // Peak temperature in frame (K)
const FLIR_VALLEY_TEMP: u16 = 0x011E; // Valley (coldest) temp (K)
const FLIR_MEASUREMENT_MODE: u16 = 0x011F; // Measurement mode
const FLIR_TEMPERATURE_UNIT: u16 = 0x0120; // Display unit (C/F/K)
const FLIR_ISOTHERM_MIN: u16 = 0x0121; // Isotherm lower limit (K)
const FLIR_ISOTHERM_MAX: u16 = 0x0122; // Isotherm upper limit (K)
const FLIR_ISOTHERM_ENABLED: u16 = 0x0123; // Isotherm mode enabled
const FLIR_LEVEL_SPAN_AUTO: u16 = 0x0124; // Auto level/span mode
const FLIR_GAIN_MODE: u16 = 0x0125; // Gain mode (auto/manual)
const FLIR_FRAME_RATE: u16 = 0x0126; // Frame rate (Hz)

// FLIR signature
const FLIR_SIGNATURE: &[u8] = b"FLIR";

/// Decodes FLIR color palette type
///
/// # Arguments
/// * `value` - Palette code
///
/// # Returns
/// Human-readable palette name
fn decode_palette(value: i16) -> String {
    match value {
        0 => "Iron".to_string(),
        1 => "Rainbow".to_string(),
        2 => "White Hot".to_string(),
        3 => "Black Hot".to_string(),
        4 => "Arctic".to_string(),
        5 => "Lava".to_string(),
        6 => "Gray".to_string(),
        7 => "Rainbow HC".to_string(),
        8 => "Ironbow".to_string(),
        9 => "Medical".to_string(),
        10 => "Fusion".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes palette method
///
/// # Arguments
/// * `value` - Method code
///
/// # Returns
/// Human-readable method
fn decode_palette_method(value: i16) -> String {
    match value {
        0 => "Linear".to_string(),
        1 => "Histogram Equalization".to_string(),
        2 => "Adaptive".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes palette stretch mode
///
/// # Arguments
/// * `value` - Stretch code
///
/// # Returns
/// Human-readable stretch mode
fn decode_palette_stretch(value: i16) -> String {
    match value {
        0 => "Manual".to_string(),
        1 => "Automatic".to_string(),
        2 => "Lock Range".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes image type
///
/// # Arguments
/// * `value` - Image type code
///
/// # Returns
/// Human-readable image type
fn decode_image_type(value: i16) -> String {
    match value {
        0 => "Thermal".to_string(),
        1 => "Visual".to_string(),
        2 => "Thermal + Visual (PIP)".to_string(),
        3 => "Thermal + Visual (Blend)".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes temperature unit
///
/// # Arguments
/// * `value` - Unit code
///
/// # Returns
/// Unit symbol
fn decode_temperature_unit(value: i16) -> String {
    match value {
        0 => "°C".to_string(),
        1 => "°F".to_string(),
        2 => "K".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes measurement mode
///
/// # Arguments
/// * `value` - Mode code
///
/// # Returns
/// Human-readable mode
fn decode_measurement_mode(value: i16) -> String {
    match value {
        0 => "Spot Meter".to_string(),
        1 => "Area".to_string(),
        2 => "Line".to_string(),
        3 => "Delta T".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes gain mode
///
/// # Arguments
/// * `value` - Gain code
///
/// # Returns
/// Human-readable gain mode
fn decode_gain_mode(value: i16) -> String {
    match value {
        0 => "Automatic".to_string(),
        1 => "Manual Low".to_string(),
        2 => "Manual High".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

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
    if value < 0 || value > 100 {
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
    if value < 0 || value > 100 {
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
        if data.len() < 8 {
            return Err("FLIR MakerNote data too short".to_string());
        }

        // Skip FLIR signature if present
        let start_offset = if data.starts_with(FLIR_SIGNATURE) {
            4
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

            match tag {
                FLIR_MODEL
                | FLIR_SERIAL
                | FLIR_FIRMWARE
                | FLIR_LENS_MODEL
                | FLIR_CALIBRATION_DATE => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            FLIR_MODEL => "Model",
                            FLIR_SERIAL => "SerialNumber",
                            FLIR_FIRMWARE => "FirmwareVersion",
                            FLIR_LENS_MODEL => "LensModel",
                            FLIR_CALIBRATION_DATE => "CalibrationDate",
                            _ => continue,
                        };
                        tags.insert(format!("FLIR:{}", tag_name), s);
                    }
                }

                _ => {
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                FLIR_TEMPERATURE_MIN => {
                                    ("TemperatureMin", format_temperature_kelvin(val))
                                }
                                FLIR_TEMPERATURE_MAX => {
                                    ("TemperatureMax", format_temperature_kelvin(val))
                                }
                                FLIR_TEMPERATURE_CENTER => {
                                    ("TemperatureCenter", format_temperature_kelvin(val))
                                }
                                FLIR_EMISSIVITY => ("Emissivity", format_emissivity(val)),
                                FLIR_REFLECTED_TEMP => {
                                    ("ReflectedTemperature", format_temperature_kelvin(val))
                                }
                                FLIR_ATMOSPHERIC_TEMP => {
                                    ("AtmosphericTemperature", format_temperature_kelvin(val))
                                }
                                FLIR_DISTANCE => ("Distance", format_distance(val)),
                                FLIR_HUMIDITY => ("RelativeHumidity", format_humidity(val)),
                                FLIR_PALETTE => ("Palette", decode_palette(val)),
                                FLIR_PALETTE_METHOD => {
                                    ("PaletteMethod", decode_palette_method(val))
                                }
                                FLIR_PALETTE_STRETCH => {
                                    ("PaletteStretch", decode_palette_stretch(val))
                                }
                                FLIR_TEMPERATURE_RANGE_MIN => {
                                    ("RangeMin", format_temperature_kelvin(val))
                                }
                                FLIR_TEMPERATURE_RANGE_MAX => {
                                    ("RangeMax", format_temperature_kelvin(val))
                                }
                                FLIR_ATMOSPHERIC_TRANS => {
                                    ("AtmosphericTransmission", format_transmission(val))
                                }
                                FLIR_EXTERNAL_OPTICS_TEMP => {
                                    ("ExternalOpticsTemperature", format_temperature_kelvin(val))
                                }
                                FLIR_EXTERNAL_OPTICS_TRANS => {
                                    ("ExternalOpticsTransmission", format_transmission(val))
                                }
                                FLIR_IR_WINDOW_TEMP => {
                                    ("IRWindowTemperature", format_temperature_kelvin(val))
                                }
                                FLIR_IR_WINDOW_TRANS => {
                                    ("IRWindowTransmission", format_transmission(val))
                                }
                                FLIR_PLANCK_R1 => ("PlanckR1", format_planck_constant(val)),
                                FLIR_PLANCK_R2 => ("PlanckR2", format_planck_constant(val)),
                                FLIR_PLANCK_B => ("PlanckB", format_planck_constant(val)),
                                FLIR_PLANCK_F => ("PlanckF", format_planck_constant(val)),
                                FLIR_PLANCK_O => ("PlanckO", format_planck_constant(val)),
                                FLIR_CAMERA_TEMP_MIN => {
                                    ("CameraInternalTempMin", format_temperature_kelvin(val))
                                }
                                FLIR_CAMERA_TEMP_MAX => {
                                    ("CameraInternalTempMax", format_temperature_kelvin(val))
                                }
                                FLIR_IMAGE_TYPE => ("ImageType", decode_image_type(val)),
                                FLIR_FOCUS_DISTANCE => ("FocusDistance", format_distance(val)),
                                FLIR_PEAK_TEMP => {
                                    ("PeakTemperature", format_temperature_kelvin(val))
                                }
                                FLIR_VALLEY_TEMP => {
                                    ("ValleyTemperature", format_temperature_kelvin(val))
                                }
                                FLIR_MEASUREMENT_MODE => {
                                    ("MeasurementMode", decode_measurement_mode(val))
                                }
                                FLIR_TEMPERATURE_UNIT => {
                                    ("TemperatureUnit", decode_temperature_unit(val))
                                }
                                FLIR_ISOTHERM_MIN => {
                                    ("IsothermMin", format_temperature_kelvin(val))
                                }
                                FLIR_ISOTHERM_MAX => {
                                    ("IsothermMax", format_temperature_kelvin(val))
                                }
                                FLIR_ISOTHERM_ENABLED => (
                                    "IsothermEnabled",
                                    if val != 0 {
                                        "Yes".to_string()
                                    } else {
                                        "No".to_string()
                                    },
                                ),
                                FLIR_LEVEL_SPAN_AUTO => (
                                    "LevelSpanAuto",
                                    if val != 0 {
                                        "Yes".to_string()
                                    } else {
                                        "No".to_string()
                                    },
                                ),
                                FLIR_GAIN_MODE => ("GainMode", decode_gain_mode(val)),
                                FLIR_FRAME_RATE => ("FrameRate", format_frame_rate(val)),
                                _ => continue,
                            };
                            tags.insert(format!("FLIR:{}", tag_name), formatted_value);
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
    fn test_flir_parser_creation() {
        let parser = FlirParser::new();
        assert_eq!(parser.manufacturer_name(), "FLIR");
        assert_eq!(parser.tag_prefix(), "FLIR:");
    }

    #[test]
    fn test_decode_palette() {
        assert_eq!(decode_palette(0), "Iron");
        assert_eq!(decode_palette(2), "White Hot");
        assert_eq!(decode_palette(5), "Lava");
    }

    #[test]
    fn test_decode_image_type() {
        assert_eq!(decode_image_type(0), "Thermal");
        assert_eq!(decode_image_type(2), "Thermal + Visual (PIP)");
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
        assert_eq!(decode_temperature_unit(0), "°C");
        assert_eq!(decode_temperature_unit(1), "°F");
        assert_eq!(decode_temperature_unit(2), "K");
    }

    #[test]
    fn test_decode_gain_mode() {
        assert_eq!(decode_gain_mode(0), "Automatic");
        assert_eq!(decode_gain_mode(1), "Manual Low");
        assert_eq!(decode_gain_mode(2), "Manual High");
    }
}
