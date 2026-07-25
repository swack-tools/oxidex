//! HP tag registry
//!
//! Registry of all HP MakerNote tags with their metadata and decoders.
//! Supports HP PhotoSmart series digital cameras.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from hp.rs
use super::super::hp::{DECODE_COLOR_MODE, DECODE_QUALITY};

// Wrapper functions to convert SimpleValueDecoder to function pointers
fn decode_quality(value: u16) -> String {
    DECODE_QUALITY.decode(value)
}
fn decode_color_mode(value: u16) -> String {
    DECODE_COLOR_MODE.decode(value)
}

/// Create and return the HP tag registry
///
/// This registry contains all known HP MakerNote tags including:
/// - Image quality settings
/// - Color mode selection
/// - Flash and exposure modes
/// - Sharpness and white balance settings
pub fn hp_registry() -> TagRegistry {
    TagRegistry::new()
        // Image Quality and Color
        .register_u16(0x0003, "Quality", decode_quality)
        .register_u16(0x0005, "ColorMode", decode_color_mode)
        // Flash and Exposure
        .register_raw(0x0007, "FlashMode")
        // Image Enhancement
        .register_raw(0x000B, "Sharpness")
        // White Balance
        .register_raw(0x0009, "WhiteBalance")
        // Model Information
        .register_raw(0x0001, "Model")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = hp_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0003)); // Quality
        assert!(registry.has_tag(0x0005)); // ColorMode
        assert!(registry.has_tag(0x0007)); // FlashMode
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = hp_registry();

        assert_eq!(registry.get_tag_name(0x0003), Some("Quality"));
        assert_eq!(registry.get_tag_name(0x0005), Some("ColorMode"));
        assert_eq!(registry.get_tag_name(0x000B), Some("Sharpness"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = hp_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
