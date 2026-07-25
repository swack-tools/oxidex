//! Kodak tag registry
//!
//! Registry of all Kodak MakerNote tags with their metadata and decoders.
//! Supports Kodak EasyShare and DCS professional digital cameras.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from kodak.rs (if they exist)

/// Create and return the Kodak tag registry
///
/// This registry contains all known Kodak MakerNote tags including:
/// - Camera model and firmware
/// - Image quality and burst mode
/// - Shutter, focus, and flash modes
/// - White balance and color settings
/// - Scene modes and exposure compensation
pub fn kodak_registry() -> TagRegistry {
    TagRegistry::new()
        // Camera Information
        .register_raw(0x0001, "Model")
        .register_raw(0x0025, "Firmware")
        .register_raw(0x0029, "TimeZone")
        // Image Quality
        .register_raw(0x0009, "Quality")
        .register_raw(0x000A, "BurstMode")
        // Shooting Modes
        .register_raw(0x000C, "ShutterMode")
        .register_raw(0x000D, "FocusMode")
        .register_raw(0x0020, "SceneMode")
        // Flash Settings
        .register_raw(0x0010, "FlashMode")
        .register_raw(0x0011, "FlashFired")
        // White Balance and Color
        .register_raw(0x000E, "WhiteBalance")
        .register_raw(0x001A, "ColorMode")
        // Image Parameters
        .register_raw(0x001C, "Sharpness")
        .register_raw(0x001D, "Saturation")
        .register_raw(0x001E, "Contrast")
        // Exposure Settings
        .register_raw(0x0014, "ISOSetting")
        .register_raw(0x0022, "ExposureBias")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = kodak_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0001)); // Model
        assert!(registry.has_tag(0x0009)); // Quality
        assert!(registry.has_tag(0x0010)); // FlashMode
        assert!(registry.has_tag(0x0020)); // SceneMode
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = kodak_registry();

        assert_eq!(registry.get_tag_name(0x0001), Some("Model"));
        assert_eq!(registry.get_tag_name(0x000E), Some("WhiteBalance"));
        assert_eq!(registry.get_tag_name(0x0025), Some("Firmware"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = kodak_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
