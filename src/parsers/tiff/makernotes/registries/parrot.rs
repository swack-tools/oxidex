//! Parrot Drone tag registry
//!
//! This module provides TagRegistry definitions for Parrot MakerNotes.
//! Parrot drones store comprehensive flight and camera metadata including
//! GPS coordinates, gimbal angles, battery status, and flight modes.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

const_decoder!(
    PARROT_FLIGHT_MODE,
    i16,
    [
        (0, "Manual"),
        (1, "GPS"),
        (2, "Follow Me"),
        (3, "Return Home"),
    ]
);

/// Create Parrot tag registry with all tag definitions
///
/// This registry provides declarative definitions of all Parrot MakerNote tags
/// including drone identification, GPS coordinates, altitude, speed, gimbal angles,
/// battery level, WiFi signal strength, and flight mode information.
///
/// Note: The registries here are simplified. Full decoding requires custom formatters
/// for GPS coordinates, altitude (cm -> m), speed (1/10 m/s), and gimbal angles (decidegrees).
/// These are handled in the parse_entry method of ParrotParser.
///
/// # Returns
/// A fully configured TagRegistry ready for Parrot MakerNote parsing
pub fn parrot_registry() -> TagRegistry {
    TagRegistry::new()
        // Drone Identification (u32 tags, raw values)
        .register_raw(0x0001, "Model")
        .register_raw(0x0002, "SerialNumber")
        .register_raw(0x0003, "Version")
        // GPS Information (i32 tags, require conversion: value / 10,000,000 = degrees)
        .register_raw(0x0100, "GPSLatitude")
        .register_raw(0x0101, "GPSLongitude")
        // Altitude and Speed (i16 tags, require conversion)
        .register_raw(0x0102, "Altitude")
        .register_raw(0x0103, "Speed")
        // Direction (i16 tag, degrees)
        .register_raw(0x0104, "Direction")
        // Gimbal Angles (i16 tags, decidegrees -> degrees)
        .register_raw(0x0105, "GimbalPitch")
        .register_raw(0x0106, "GimbalRoll")
        .register_raw(0x0107, "GimbalYaw")
        // System Status (i16 tags)
        .register_raw(0x0108, "BatteryLevel")
        .register_raw(0x0109, "WiFiSignal")
        // Flight Information (i16 tag with decoder)
        .register_simple_i16(0x010A, "FlightMode", &PARROT_FLIGHT_MODE)
        // Home Distance (i16 tag, meters)
        .register_raw(0x010B, "HomeDistance")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = parrot_registry();
        assert!(registry.has_tag(0x0001)); // Model
        assert!(registry.has_tag(0x0100)); // GPSLatitude
        assert!(registry.has_tag(0x010A)); // FlightMode
    }

    #[test]
    fn test_tag_names() {
        let registry = parrot_registry();
        assert_eq!(registry.get_tag_name(0x0001), Some("Model"));
        assert_eq!(registry.get_tag_name(0x0100), Some("GPSLatitude"));
        assert_eq!(registry.get_tag_name(0x010A), Some("FlightMode"));
    }
}
