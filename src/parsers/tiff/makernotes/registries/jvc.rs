//! JVC tag registry
//!
//! Registry of all JVC MakerNote tags with their metadata and decoders.
//! Supports JVC GC and Everio series digital cameras.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from jvc.rs
use super::super::jvc::{DECODE_FOCUS_MODE, DECODE_QUALITY};

// Wrapper functions to convert SimpleValueDecoder to function pointers
fn decode_quality(value: u16) -> String {
    DECODE_QUALITY.decode(value)
}
fn decode_focus_mode(value: u16) -> String {
    DECODE_FOCUS_MODE.decode(value)
}

/// Create and return the JVC tag registry
///
/// This registry contains all known JVC MakerNote tags including:
/// - Image quality settings
/// - Focus mode selection
/// - Flash modes
/// - Color and white balance settings
/// - Sharpness adjustments
pub fn jvc_registry() -> TagRegistry {
    TagRegistry::new()
        // Image Quality and Modes
        .register_u16(0x0001, "Quality", decode_quality)
        // Focus Settings
        .register_u16(0x0002, "FocusMode", decode_focus_mode)
        // Flash and Exposure
        .register_raw(0x0003, "FlashMode")
        // White Balance
        .register_raw(0x0004, "WhiteBalance")
        // Image Enhancement
        .register_raw(0x0005, "Sharpness")
        .register_raw(0x0006, "ColorMode")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = jvc_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0001)); // Quality
        assert!(registry.has_tag(0x0002)); // FocusMode
        assert!(registry.has_tag(0x0003)); // FlashMode
        assert!(registry.has_tag(0x0006)); // ColorMode
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = jvc_registry();

        assert_eq!(registry.get_tag_name(0x0001), Some("Quality"));
        assert_eq!(registry.get_tag_name(0x0002), Some("FocusMode"));
        assert_eq!(registry.get_tag_name(0x0004), Some("WhiteBalance"));
        assert_eq!(registry.get_tag_name(0x0005), Some("Sharpness"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = jvc_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
