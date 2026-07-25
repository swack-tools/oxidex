//! Casio tag registry
//!
//! Registry of all Casio MakerNote tags with their metadata and decoders.
//! Supports Casio Exilim and QV-series digital cameras.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from casio.rs (if they exist)

/// Create and return the Casio tag registry
///
/// This registry contains all known Casio MakerNote tags including:
/// - Recording mode and image quality
/// - Focus and flash modes
/// - White balance and color settings
/// - Digital zoom and enhancements
/// - Continuous shooting and Best Shot modes
pub fn casio_registry() -> TagRegistry {
    TagRegistry::new()
        // Recording Settings
        .register_raw(0x0001, "RecordingMode")
        .register_raw(0x0002, "Quality")
        .register_raw(0x001A, "ContinuousMode")
        .register_raw(0x001B, "BestShotMode")
        .register_raw(0x0020, "SlowShutter")
        // Focus and Flash
        .register_raw(0x0003, "FocusMode")
        .register_raw(0x0004, "FlashMode")
        .register_raw(0x0005, "FlashIntensity")
        // White Balance and Color
        .register_raw(0x0007, "WhiteBalance")
        .register_raw(0x0015, "ColorMode")
        .register_raw(0x0017, "ColorFilter")
        // Zoom and Image Enhancement
        .register_raw(0x000A, "DigitalZoom")
        .register_raw(0x0016, "Enhancement")
        // Image Quality Parameters
        .register_raw(0x000B, "Sharpness")
        .register_raw(0x000C, "Contrast")
        .register_raw(0x000D, "Saturation")
        // Camera Settings
        .register_raw(0x0014, "CCDSensitivity")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = casio_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0001)); // RecordingMode
        assert!(registry.has_tag(0x0002)); // Quality
        assert!(registry.has_tag(0x0004)); // FlashMode
        assert!(registry.has_tag(0x001B)); // BestShotMode
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = casio_registry();

        assert_eq!(registry.get_tag_name(0x0001), Some("RecordingMode"));
        assert_eq!(registry.get_tag_name(0x0007), Some("WhiteBalance"));
        assert_eq!(registry.get_tag_name(0x001B), Some("BestShotMode"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = casio_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
