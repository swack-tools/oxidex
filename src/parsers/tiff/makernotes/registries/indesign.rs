//! Adobe InDesign tag registry
//!
//! This module provides TagRegistry definitions for InDesign MakerNotes.
//! InDesign stores document placement and layout metadata for embedded images.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

// Decodes color space
const_decoder!(
    DECODE_COLOR_SPACE,
    i16,
    [(0, "RGB"), (1, "CMYK"), (2, "Lab"), (3, "Grayscale"),]
);

// Decodes text wrap type
const_decoder!(
    DECODE_TEXT_WRAP,
    i16,
    [
        (0, "None"),
        (1, "Bounding Box"),
        (2, "Object Shape"),
        (3, "Jump Object"),
        (4, "Jump to Next Column"),
    ]
);

// Decodes frame fitting option
const_decoder!(
    DECODE_FRAME_FITTING,
    i16,
    [
        (0, "None"),
        (1, "Fill Frame Proportionally"),
        (2, "Fit Content Proportionally"),
        (3, "Fit Content to Frame"),
        (4, "Center Content"),
    ]
);

// Decodes print setting
const_decoder!(
    DECODE_PRINT_SETTING,
    i16,
    [(0, "Default"), (1, "Print"), (2, "Non-Print"),]
);

// ============================================================================
// Tag Registry Factory Function
// ============================================================================

/// Create InDesign tag registry with all tag definitions
///
/// This registry provides declarative definitions of all InDesign MakerNote tags
/// including document info, placement coordinates, scaling, rotation, color management,
/// layer visibility, text wrapping, and frame fitting.
///
/// # Returns
/// A fully configured TagRegistry ready for InDesign MakerNote parsing
pub fn indesign_registry() -> TagRegistry {
    TagRegistry::new()
        // Version and document
        .register_raw(0x0001, "Version")
        .register_raw(0x0010, "DocumentName")
        // Page information
        .register_raw(0x0011, "PageNumber")
        .register_raw(0x0012, "PageWidth")
        .register_raw(0x0013, "PageHeight")
        // Frame position and dimensions
        .register_raw(0x0020, "XPosition")
        .register_raw(0x0021, "YPosition")
        .register_raw(0x0022, "FrameWidth")
        .register_raw(0x0023, "FrameHeight")
        // Scaling and transformation
        .register_raw(0x0030, "ScaleX")
        .register_raw(0x0031, "ScaleY")
        .register_raw(0x0032, "Rotation")
        // Resolution information
        .register_raw(0x0040, "EffectivePPIX")
        .register_raw(0x0041, "EffectivePPIY")
        // Color information
        .register_simple_i16(0x0050, "ColorSpace", &DECODE_COLOR_SPACE)
        .register_raw(0x0051, "ColorProfile")
        // Layer information
        .register_raw(0x0060, "LayerName")
        .register_raw(0x0061, "LayerVisible")
        .register_raw(0x0062, "MasterPage")
        // Text and frame properties
        .register_raw(0x0070, "TextWrap")
        .register_simple_i16(0x0071, "TextWrapType", &DECODE_TEXT_WRAP)
        .register_simple_i16(0x0072, "FrameFitting", &DECODE_FRAME_FITTING)
        // Print and output settings
        .register_simple_i16(0x0080, "PrintSetting", &DECODE_PRINT_SETTING)
        .register_raw(0x0081, "OutputIntent")
        .register_raw(0x0082, "SpreadNumber")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = indesign_registry();
        assert!(registry.has_tag(0x0001)); // Version
        assert!(registry.has_tag(0x0050)); // ColorSpace
        assert!(registry.has_tag(0x0071)); // TextWrapType
    }

    #[test]
    fn test_tag_names() {
        let registry = indesign_registry();
        assert_eq!(registry.get_tag_name(0x0001), Some("Version"));
        assert_eq!(registry.get_tag_name(0x0050), Some("ColorSpace"));
        assert_eq!(registry.get_tag_name(0x0071), Some("TextWrapType"));
    }

    #[test]
    fn test_decoders() {
        assert_eq!(DECODE_COLOR_SPACE.decode(0), "RGB");
        assert_eq!(DECODE_COLOR_SPACE.decode(1), "CMYK");
        assert_eq!(DECODE_TEXT_WRAP.decode(1), "Bounding Box");
        assert_eq!(DECODE_FRAME_FITTING.decode(1), "Fill Frame Proportionally");
        assert_eq!(DECODE_PRINT_SETTING.decode(1), "Print");
    }
}
