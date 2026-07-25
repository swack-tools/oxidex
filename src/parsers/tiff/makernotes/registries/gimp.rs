//! GIMP (GNU Image Manipulation Program) tag registry
//!
//! This module provides TagRegistry definitions for GIMP MakerNotes.
//! GIMP is a free and open-source raster graphics editor that stores
//! comprehensive editing metadata in TIFF MakerNotes.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

// Decodes layer modes bitmask - combines multiple modes
const_decoder!(
    DECODE_LAYER_MODES,
    i16,
    [
        (0, "None"),
        (1, "Normal"),
        (2, "Dissolve"),
        (4, "Multiply"),
        (8, "Screen"),
        (16, "Overlay"),
        (32, "Difference"),
        (64, "Addition"),
        (128, "Subtract"),
        (256, "Darken Only"),
        (512, "Lighten Only"),
    ]
);

// Decodes selection type
const_decoder!(
    DECODE_SELECTION_TYPE,
    i16,
    [
        (0, "None"),
        (1, "Rectangle"),
        (2, "Ellipse"),
        (3, "Free"),
        (4, "Fuzzy"),
        (5, "By Color"),
        (6, "Path"),
    ]
);

// ============================================================================
// Tag Registry Factory Function
// ============================================================================

/// Create GIMP tag registry with all tag definitions
///
/// This registry provides declarative definitions of all GIMP MakerNote tags
/// including layer count, filter information, adjustment flags, selection data,
/// and undo/plugin history.
///
/// # Returns
/// A fully configured TagRegistry ready for GIMP MakerNote parsing
pub fn gimp_registry() -> TagRegistry {
    TagRegistry::new()
        // Version
        .register_raw(0x0001, "Version")
        // Layer and composition info
        .register_raw(0x0010, "LayerCount")
        .register_simple_i16(0x0011, "LayerModes", &DECODE_LAYER_MODES)
        .register_raw(0x0012, "FilterCount")
        .register_raw(0x0013, "FiltersApplied")
        .register_raw(0x0014, "ToolsUsed")
        // Color adjustment flags
        .register_raw(0x0020, "CurvesAdjusted")
        .register_raw(0x0021, "LevelsAdjusted")
        .register_raw(0x0022, "HueSaturationAdjusted")
        .register_raw(0x0023, "BrightnessContrastAdjusted")
        .register_raw(0x0024, "ColorBalanceAdjusted")
        .register_raw(0x0025, "ThresholdApplied")
        .register_raw(0x0026, "PosterizeApplied")
        .register_raw(0x0027, "DesaturateApplied")
        // Selection and channel info
        .register_raw(0x0030, "SelectionActive")
        .register_simple_i16(0x0031, "SelectionType", &DECODE_SELECTION_TYPE)
        .register_raw(0x0032, "PathCount")
        .register_raw(0x0033, "ChannelCount")
        .register_raw(0x0034, "AlphaChannel")
        // Undo and plugin history
        .register_raw(0x0040, "UndoLevels")
        .register_raw(0x0041, "PluginCount")
        .register_raw(0x0042, "ScriptFuCount")
        // Filter-specific counts
        .register_raw(0x0050, "BlurFilterCount")
        .register_raw(0x0051, "SharpenFilterCount")
        .register_raw(0x0052, "NoiseFilterCount")
        .register_raw(0x0053, "DistortFilterCount")
        .register_raw(0x0054, "EdgeDetectCount")
        .register_raw(0x0055, "EnhanceFilterCount")
        .register_raw(0x0056, "RenderFilterCount")
        // Parasites (metadata attachments)
        .register_raw(0x0060, "ParasitesCount")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = gimp_registry();
        assert!(registry.has_tag(0x0001)); // Version
        assert!(registry.has_tag(0x0010)); // LayerCount
        assert!(registry.has_tag(0x0031)); // SelectionType
    }

    #[test]
    fn test_tag_names() {
        let registry = gimp_registry();
        assert_eq!(registry.get_tag_name(0x0001), Some("Version"));
        assert_eq!(registry.get_tag_name(0x0010), Some("LayerCount"));
        assert_eq!(registry.get_tag_name(0x0031), Some("SelectionType"));
    }

    #[test]
    fn test_decoder_layer_modes() {
        assert_eq!(DECODE_LAYER_MODES.decode(1), "Normal");
        assert_eq!(DECODE_LAYER_MODES.decode(4), "Multiply");
    }

    #[test]
    fn test_decoder_selection_type() {
        assert_eq!(DECODE_SELECTION_TYPE.decode(1), "Rectangle");
        assert_eq!(DECODE_SELECTION_TYPE.decode(2), "Ellipse");
    }
}
