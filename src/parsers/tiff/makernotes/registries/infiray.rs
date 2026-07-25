//! InfiRay tag registry
//!
//! Registry of all InfiRay Thermal Camera MakerNote tags with their metadata and decoders.
//! Supports InfiRay P2 Pro, T2 Pro, T3 Series, and other thermal imaging cameras.

use super::super::shared::tag_registry::TagRegistry;

/// Create and return the InfiRay tag registry
///
/// This registry contains all known InfiRay MakerNote tags including:
/// - Device information (model, serial, firmware)
/// - Temperature measurements (min/max/center)
/// - Thermal imaging settings (emissivity, palette)
/// - Measurement range and atmospheric parameters
/// - Image enhancement and display modes
pub fn infiray_registry() -> TagRegistry {
    TagRegistry::new()
        // Device Information
        .register_raw(0x0001, "Model")
        .register_raw(0x0002, "SerialNumber")
        .register_raw(0x0003, "FirmwareVersion")
        // Temperature Measurements
        .register_raw(0x0100, "TemperatureMin")
        .register_raw(0x0101, "TemperatureMax")
        .register_raw(0x0102, "TemperatureCenter")
        // Thermal Settings
        .register_raw(0x0103, "Emissivity")
        .register_raw(0x0104, "Distance")
        .register_raw(0x0105, "Palette")
        // Measurement Range
        .register_raw(0x0106, "RangeMin")
        .register_raw(0x0107, "RangeMax")
        // Atmospheric Parameters
        .register_raw(0x0108, "AtmosphericTemp")
        .register_raw(0x0109, "Humidity")
        // Image Enhancement
        .register_raw(0x010A, "Enhancement")
        .register_raw(0x010B, "DigitalZoom")
        .register_raw(0x010C, "Contrast")
        .register_raw(0x010D, "Brightness")
        .register_raw(0x010E, "Sharpness")
        // Display Modes
        .register_raw(0x010F, "SpotMeter")
        .register_raw(0x0110, "Isotherm")
        .register_raw(0x0111, "TemperatureUnit")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = infiray_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0001)); // Model
        assert!(registry.has_tag(0x0100)); // TemperatureMin
        assert!(registry.has_tag(0x0102)); // TemperatureCenter
        assert!(registry.has_tag(0x0105)); // Palette
        assert!(registry.has_tag(0x010A)); // Enhancement
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = infiray_registry();

        assert_eq!(registry.get_tag_name(0x0001), Some("Model"));
        assert_eq!(registry.get_tag_name(0x0100), Some("TemperatureMin"));
        assert_eq!(registry.get_tag_name(0x0103), Some("Emissivity"));
        assert_eq!(registry.get_tag_name(0x010B), Some("DigitalZoom"));
        assert_eq!(registry.get_tag_name(0x0111), Some("TemperatureUnit"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = infiray_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
