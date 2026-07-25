//! Fujifilm tag registry
//!
//! This module provides TagRegistry definitions for Fujifilm MakerNotes.
//! Fujifilm uses a comprehensive tag structure with simple value types
//! (strings, integers, and enumerated values) and some array-based tags.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from fujifilm.rs
// These decoders are defined using const_decoder! macros in the main parser
use super::super::fujifilm::{
    DECODE_DRIVE_MODE, DECODE_DYNAMIC_RANGE, DECODE_DYNAMIC_RANGE_SETTING, DECODE_EXR_MODE,
    DECODE_FILM_MODE, DECODE_FLASH_MODE, DECODE_FOCUS_MODE, DECODE_OFF_ON, DECODE_PICTURE_MODE,
    DECODE_QUALITY, DECODE_WHITE_BALANCE,
};

// ============================================================================
// TAG REGISTRY
// ============================================================================

/// Create Fujifilm tag registry with all tag definitions
///
/// This registry provides a centralized, declarative definition of all Fujifilm
/// MakerNote tags including:
/// - Simple string tags (version, serial number, lens model name)
/// - Simple integer tags (color temperature, focal lengths, apertures, ratings)
/// - Enumerated tags with decoders (quality, white balance, focus mode, etc.)
///
/// # Returns
/// A fully configured TagRegistry ready for Fujifilm MakerNote parsing
pub fn fujifilm_registry() -> TagRegistry {
    TagRegistry::new()
        // String tags
        .register_string_tag(0x0000, "Version")
        .register_string_tag(0x0010, "SerialNumber")
        .register_string_tag(0x1050, "LensModelName")
        // Enumerated tags with decoders
        .register_enum_tag_required(0x1000, "Quality", &DECODE_QUALITY)
        .register_enum_tag(0x1001, "Sharpness", None)
        .register_enum_tag_required(0x1002, "WhiteBalance", &DECODE_WHITE_BALANCE)
        .register_enum_tag(0x1003, "Saturation", None)
        .register_enum_tag(0x1004, "Contrast", None)
        .register_enum_tag_required(0x1010, "FlashMode", &DECODE_FLASH_MODE)
        .register_enum_tag_required(0x1020, "Macro", &DECODE_OFF_ON)
        .register_enum_tag_required(0x1021, "FocusMode", &DECODE_FOCUS_MODE)
        .register_enum_tag_required(0x1030, "SlowSync", &DECODE_OFF_ON)
        .register_enum_tag_required(0x1031, "PictureMode", &DECODE_PICTURE_MODE)
        .register_enum_tag(0x1033, "EXRAuto", None)
        .register_enum_tag_required(0x1034, "EXRMode", &DECODE_EXR_MODE)
        .register_enum_tag_required(0x1039, "DriveMode", &DECODE_DRIVE_MODE)
        .register_enum_tag(0x1300, "BlurWarning", None)
        .register_enum_tag(0x1301, "FocusWarning", None)
        .register_enum_tag(0x1302, "ExposureWarning", None)
        .register_enum_tag(0x1304, "DynamicRangeWarning", None)
        .register_enum_tag_required(0x1401, "FilmMode", &DECODE_FILM_MODE)
        .register_enum_tag_required(0x1400, "DynamicRange", &DECODE_DYNAMIC_RANGE)
        .register_enum_tag_required(0x1402, "DynamicRangeSetting", &DECODE_DYNAMIC_RANGE_SETTING)
        .register_integer_tag(0x1403, "DevelopmentDynamicRange", None)
        .register_enum_tag(0x140B, "AutoDynamicRange", None)
        // Simple integer/numeric tags
        .register_integer_tag(0x1005, "ColorTemperature", None)
        .register_integer_tag(0x1006, "ContrastDetectionAF", None)
        .register_integer_tag(0x1011, "FlashEV", None)
        .register_integer_tag(0x1023, "FocusPixel", None)
        .register_integer_tag(0x1040, "ShadowTone", None)
        .register_integer_tag(0x1041, "HighlightTone", None)
        .register_integer_tag(0x1044, "DigitalZoom", None)
        .register_integer_tag(0x1047, "ImageGeneration", None)
        .register_integer_tag(0x1100, "AutoBracketing", None)
        .register_integer_tag(0x1101, "SequenceNumber", None)
        .register_integer_tag(0x1032, "ExposureCount", None)
        .register_integer_tag(0x1404, "MinFocalLength", None)
        .register_integer_tag(0x1405, "MaxFocalLength", None)
        .register_integer_tag(0x1406, "MaxApertureAtMinFocal", None)
        .register_integer_tag(0x1407, "MaxApertureAtMaxFocal", None)
        .register_integer_tag(0x1431, "Rating", None)
        .register_integer_tag(0x1438, "ImageCount", None)
        .register_integer_tag(0x4100, "FacesDetected", None)
        .register_integer_tag(0x8000, "FileSource", None)
        .register_integer_tag(0x8002, "OrderNumber", None)
        .register_integer_tag(0x8003, "FrameNumber", None)
        .register_integer_tag(0xB211, "Parallax", None)
        .register_integer_tag(0xF000, "RAWImageFullSize", None)
        .register_integer_tag(0xF001, "RAWImageFullWidth", None)
        .register_integer_tag(0xF002, "RAWImageFullHeight", None)
        .register_integer_tag(0xF003, "RAWImageAspectRatio", None)
        .register_integer_tag(0x9650, "PixelShiftOffset", None)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let _registry = fujifilm_registry();
        // Verify registry can be created successfully
        assert!(true, "Registry created successfully");
    }

    #[test]
    fn test_registry_has_tags() {
        let registry = fujifilm_registry();
        // Verify registry contains some expected tags
        assert!(!registry.is_empty(), "Registry should have tags");
    }
}
