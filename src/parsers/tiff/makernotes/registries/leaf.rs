//! Leaf tag registry
//!
//! This module provides TagRegistry definitions for Leaf MakerNotes.
//! Leaf digital backs (later acquired by Phase One) store comprehensive
//! metadata about camera settings, lens information, and image processing.

use super::super::shared::tag_registry::TagRegistry;

/// Create Leaf tag registry with all tag definitions
///
/// This registry provides declarative definitions of all Leaf MakerNote tags
/// including digital back information, image quality settings, lens metadata,
/// color calibration, and exposure parameters.
///
/// # Returns
/// A fully configured TagRegistry ready for Leaf MakerNote parsing
pub fn leaf_registry() -> TagRegistry {
    TagRegistry::new()
        // Digital Back Information (raw string values)
        .register_raw(0x0001, "BackModel")
        .register_raw(0x0002, "SerialNumber")
        // Image Dimensions (raw u32 values)
        .register_raw(0x0003, "ImageWidth")
        .register_raw(0x0004, "ImageHeight")
        // Image Quality (raw u16 value)
        .register_raw(0x0005, "BitDepth")
        // Exposure Settings (raw u16 values)
        .register_raw(0x0006, "ISOSpeed")
        .register_raw(0x0007, "ShutterSpeed")
        .register_raw(0x0008, "Aperture")
        // Lens Information (raw u16 values)
        .register_raw(0x0009, "FocalLength")
        .register_raw(0x000A, "LensID")
        // Color and Processing (raw u16 values and strings)
        .register_raw(0x000B, "WhiteBalance")
        .register_raw(0x000C, "ColorSpace")
        .register_raw(0x000D, "CalibrationProfile")
        // Firmware (raw string value)
        .register_raw(0x000E, "FirmwareVersion")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = leaf_registry();
        assert!(registry.has_tag(0x0001)); // BackModel
        assert!(registry.has_tag(0x000A)); // LensID
        assert!(registry.has_tag(0x000B)); // WhiteBalance
    }

    #[test]
    fn test_tag_names() {
        let registry = leaf_registry();
        assert_eq!(registry.get_tag_name(0x0001), Some("BackModel"));
        assert_eq!(registry.get_tag_name(0x000A), Some("LensID"));
        assert_eq!(registry.get_tag_name(0x000B), Some("WhiteBalance"));
    }
}
