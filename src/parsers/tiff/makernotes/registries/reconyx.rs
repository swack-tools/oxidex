//! Reconyx Wildlife Camera tag registry
//!
//! This module provides TagRegistry definitions for Reconyx MakerNotes.
//! Reconyx specializes in motion-triggered wildlife and trail cameras
//! with comprehensive environmental and sensor metadata.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

// Decodes Reconyx trigger mode
const_decoder!(
    TRIGGER_MODE,
    i16,
    [
        (0, "Time Lapse"),
        (1, "Motion Detection"),
        (2, "Time Lapse + Motion"),
    ]
);

// Decodes moon phase
const_decoder!(
    MOON_PHASE,
    i16,
    [
        (0, "New Moon"),
        (1, "Waxing Crescent"),
        (2, "First Quarter"),
        (3, "Waxing Gibbous"),
        (4, "Full Moon"),
        (5, "Waning Gibbous"),
        (6, "Last Quarter"),
        (7, "Waning Crescent"),
    ]
);

// ============================================================================
// Tag Registry Factory Function
// ============================================================================

/// Create Reconyx tag registry with all tag definitions
///
/// This registry provides declarative definitions of all Reconyx MakerNote tags
/// including camera identification, trigger modes, environmental conditions,
/// battery information, and motion detection data.
///
/// # Returns
/// A fully configured TagRegistry ready for Reconyx MakerNote parsing
pub fn reconyx_registry() -> TagRegistry {
    TagRegistry::new()
        // Camera identification
        .register_raw(0x0001, "Model")
        .register_raw(0x0002, "SerialNumber")
        .register_raw(0x0003, "FirmwareVersion")
        // Trigger and sequence information
        .register_simple_i16(0x0100, "TriggerMode", &TRIGGER_MODE)
        .register_raw(0x0101, "SequenceNumber")
        .register_raw(0x0102, "EventNumber")
        // Environmental conditions
        .register_raw(0x0103, "Temperature")
        .register_raw(0x0104, "BatteryVoltage")
        .register_simple_i16(0x0105, "MoonPhase", &MOON_PHASE)
        // Timing information
        .register_raw(0x0106, "TimelapseInterval")
        // Sensor and detection
        .register_raw(0x0107, "PIRReadings")
        .register_raw(0x0108, "FlashOutput")
        .register_raw(0x0109, "SensorSensitivity")
        .register_raw(0x010A, "MotionDetectLevel")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = reconyx_registry();
        assert!(registry.has_tag(0x0001)); // Model
        assert!(registry.has_tag(0x0100)); // TriggerMode
        assert!(registry.has_tag(0x0105)); // MoonPhase
    }

    #[test]
    fn test_tag_names() {
        let registry = reconyx_registry();
        assert_eq!(registry.get_tag_name(0x0001), Some("Model"));
        assert_eq!(registry.get_tag_name(0x0100), Some("TriggerMode"));
        assert_eq!(registry.get_tag_name(0x0105), Some("MoonPhase"));
    }
}
