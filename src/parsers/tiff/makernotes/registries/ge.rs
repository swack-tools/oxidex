//! GE tag registry
//!
//! Registry of all GE MakerNote tags with their metadata and decoders.
//! Supports GE Power, E-series, and X-series digital cameras.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from ge.rs
use super::super::ge::{DECODE_QUALITY, DECODE_SCENE_MODE};

// Wrapper functions to convert SimpleValueDecoder to function pointers
fn decode_quality(value: u16) -> String {
    DECODE_QUALITY.decode(value)
}
fn decode_scene_mode(value: u16) -> String {
    DECODE_SCENE_MODE.decode(value)
}

/// Create and return the GE tag registry
///
/// This registry contains all known GE MakerNote tags including:
/// - Image quality settings
/// - Scene mode selection
/// - Focus and flash modes
/// - White balance settings
pub fn ge_registry() -> TagRegistry {
    TagRegistry::new()
        // Image Quality
        .register_u16(0x0001, "Quality", decode_quality)
        // Focus Settings
        .register_raw(0x0002, "FocusMode")
        // Flash and Exposure
        .register_raw(0x0003, "FlashMode")
        // Scene Mode
        .register_u16(0x0004, "SceneMode", decode_scene_mode)
        // White Balance
        .register_raw(0x0005, "WhiteBalance")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = ge_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0001)); // Quality
        assert!(registry.has_tag(0x0002)); // FocusMode
        assert!(registry.has_tag(0x0003)); // FlashMode
        assert!(registry.has_tag(0x0004)); // SceneMode
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = ge_registry();

        assert_eq!(registry.get_tag_name(0x0001), Some("Quality"));
        assert_eq!(registry.get_tag_name(0x0002), Some("FocusMode"));
        assert_eq!(registry.get_tag_name(0x0003), Some("FlashMode"));
        assert_eq!(registry.get_tag_name(0x0004), Some("SceneMode"));
        assert_eq!(registry.get_tag_name(0x0005), Some("WhiteBalance"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = ge_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
