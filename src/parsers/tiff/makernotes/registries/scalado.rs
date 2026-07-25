//! Scalado Mobile Photo Editor tag registry
//!
//! This module provides TagRegistry definitions for Scalado MakerNotes.
//! Scalado specializes in mobile photo editing metadata.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

// Decodes Scalado filter type
const_decoder!(
    DECODE_FILTER_TYPE,
    i16,
    [
        (0, "None"),
        (1, "Vintage"),
        (2, "Sepia"),
        (3, "Black & White"),
        (4, "Cool"),
        (5, "Warm"),
        (6, "Vivid"),
        (7, "Soft"),
    ]
);

// Decodes Scalado auto-enhance level
const_decoder!(
    DECODE_AUTO_ENHANCE,
    i16,
    [(0, "Off"), (1, "Low"), (2, "Medium"), (3, "High"),]
);

// ============================================================================
// Tag Registry Factory Function
// ============================================================================

/// Create Scalado tag registry with all tag definitions
///
/// This registry provides declarative definitions of all Scalado MakerNote tags
/// including filters, auto-enhance, red-eye reduction, brightness/contrast/saturation,
/// crop, straighten, face detection, panorama, HDR, and touch-up metadata.
///
/// # Returns
/// A fully configured TagRegistry ready for Scalado MakerNote parsing
pub fn scalado_registry() -> TagRegistry {
    TagRegistry::new()
        // Version
        .register_raw(0x0001, "Version")
        // Filter and enhancement
        .register_simple_i16(0x0010, "FilterType", &DECODE_FILTER_TYPE)
        .register_simple_i16(0x0011, "AutoEnhance", &DECODE_AUTO_ENHANCE)
        .register_raw(0x0012, "RedEyeReduction")
        // Adjustments
        .register_raw(0x0020, "Brightness")
        .register_raw(0x0021, "Contrast")
        .register_raw(0x0022, "Saturation")
        // Crop and straighten
        .register_raw(0x0030, "CropApplied")
        .register_raw(0x0031, "StraightenAngle")
        // Face and special features
        .register_raw(0x0040, "FacesDetected")
        .register_raw(0x0041, "Panorama")
        .register_raw(0x0042, "HDR")
        .register_raw(0x0043, "TouchupCount")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = scalado_registry();
        assert!(registry.has_tag(0x0001)); // Version
        assert!(registry.has_tag(0x0010)); // FilterType
        assert!(registry.has_tag(0x0011)); // AutoEnhance
    }

    #[test]
    fn test_tag_names() {
        let registry = scalado_registry();
        assert_eq!(registry.get_tag_name(0x0001), Some("Version"));
        assert_eq!(registry.get_tag_name(0x0010), Some("FilterType"));
        assert_eq!(registry.get_tag_name(0x0011), Some("AutoEnhance"));
    }
}
