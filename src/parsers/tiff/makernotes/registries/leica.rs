//! Leica tag registry
//!
//! This module provides TagRegistry definitions for Leica MakerNotes.
//! Leica uses a comprehensive tag structure with simple value types
//! (strings, integers, and enumerated values).

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from leica.rs
// These decoders are defined using const_decoder! macros in the main parser
use super::super::leica::{
    DECODER_AF_MODE, DECODER_CROP_MODE, DECODER_EXPOSURE_MODE, DECODER_FLASH_MODE,
    DECODER_IMAGE_STABILIZATION, DECODER_METERING_MODE, DECODER_QUALITY, DECODER_SCENE_MODE,
    DECODER_USER_PROFILE, DECODER_WHITE_BALANCE,
};

// ============================================================================
// TAG REGISTRY
// ============================================================================

/// Create Leica tag registry with all tag definitions
///
/// This registry provides a centralized, declarative definition of all Leica
/// MakerNote tags including:
/// - Simple string tags (serial numbers, lens model, file names, directories)
/// - Simple integer/numeric tags (temperatures, RGB levels, shutter count, angles)
/// - Enumerated tags with decoders (quality, white balance, exposure, flash, etc.)
///
/// Supports Leica M-series, SL-series, Q-series, and CL/TL cameras.
///
/// # Returns
/// A fully configured TagRegistry ready for Leica MakerNote parsing
pub fn leica_registry() -> TagRegistry {
    TagRegistry::new()
        // String tags
        .register_string_tag(0x0005, "SerialNumber")
        .register_string_tag(0x0015, "LensModel")
        .register_string_tag(0x001D, "OriginalFileName")
        .register_string_tag(0x001E, "OriginalDirectory")
        .register_string_tag(0x0027, "InternalSerialNumber")
        .register_string_tag(0x0031, "LensSerialNumber")
        .register_string_tag(0x0043, "UserComment")
        // Enumerated tags with decoders
        .register_enum_tag_required(0x0003, "Quality", &DECODER_QUALITY)
        .register_enum_tag_required(0x0004, "UserProfile", &DECODER_USER_PROFILE)
        .register_enum_tag_required(0x0006, "WhiteBalance", &DECODER_WHITE_BALANCE)
        .register_enum_tag_required(0x0020, "ExposureMode", &DECODER_EXPOSURE_MODE)
        .register_enum_tag_required(0x0021, "MeteringMode", &DECODER_METERING_MODE)
        .register_enum_tag(0x0022, "FilmMode", None)
        .register_enum_tag(0x0023, "WBMode", None)
        .register_enum_tag_required(0x0025, "FlashMode", &DECODER_FLASH_MODE)
        .register_enum_tag(0x0032, "ContrastDetectAF", None)
        .register_enum_tag(0x0050, "PictureControl", None)
        .register_enum_tag(0x0051, "AFPoint", None)
        .register_enum_tag_required(0x0052, "AFMode", &DECODER_AF_MODE)
        .register_enum_tag_required(0x0053, "ImageStabilization", &DECODER_IMAGE_STABILIZATION)
        .register_enum_tag(0x0054, "DigitalZoom", None)
        .register_enum_tag(0x0060, "DNGVersion", None)
        .register_enum_tag_required(0x0061, "CropMode", &DECODER_CROP_MODE)
        .register_enum_tag(0x0062, "PerspectiveControl", None)
        .register_enum_tag(0x0070, "MacroMode", None)
        .register_enum_tag_required(0x0071, "SceneMode", &DECODER_SCENE_MODE)
        // Simple integer/numeric tags
        .register_integer_tag(0x0008, "ExternalSensorBrightnessValue", None)
        .register_integer_tag(0x0009, "MeasuredLV", None)
        .register_integer_tag(0x000A, "ApproximateFNumber", None)
        .register_integer_tag(0x000B, "CameraTemperature", None)
        .register_integer_tag(0x000C, "ColorTemperature", None)
        .register_integer_tag(0x000D, "WBRedLevel", None)
        .register_integer_tag(0x000E, "WBGreenLevel", None)
        .register_integer_tag(0x000F, "WBBlueLevel", None)
        .register_integer_tag(0x0010, "Sharpening", None)
        .register_integer_tag(0x0011, "Contrast", None)
        .register_integer_tag(0x0012, "Saturation", None)
        .register_integer_tag(0x0013, "LensID", None)
        .register_integer_tag(0x0014, "LensType", None)
        .register_integer_tag(0x0024, "APEXBrightness", None)
        .register_integer_tag(0x0026, "FlashEnergy", None)
        .register_integer_tag(0x0030, "FocalLength35mm", None)
        .register_integer_tag(0x0034, "ShutterCount", None)
        .register_integer_tag(0x0035, "FocusDistance", None)
        .register_integer_tag(0x0040, "FrameSelector", None)
        .register_integer_tag(0x0041, "BaseISO", None)
        .register_integer_tag(0x0042, "ImageID", None)
        .register_integer_tag(0x0063, "CameraPitchAngle", None)
        .register_integer_tag(0x0064, "CameraRollAngle", None)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let _registry = leica_registry();
        // Verify registry can be created successfully
        assert!(true, "Registry created successfully");
    }

    #[test]
    fn test_registry_has_tags() {
        let registry = leica_registry();
        // Verify registry contains some expected tags
        assert!(!registry.is_empty(), "Registry should have tags");
    }
}
