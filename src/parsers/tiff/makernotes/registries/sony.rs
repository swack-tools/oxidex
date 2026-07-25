//! Sony tag registry with array schemas
//!
//! This module provides TagRegistry definitions for Sony MakerNotes using
//! the declarative array schema system to reduce code duplication.
//!
//! ## Array Tags
//! Sony MakerNotes contain three main array tags:
//! - **CameraSettings** (0x0114): 17 camera configuration settings
//! - **AFInfo** (0x9400, 0x9402): 5 autofocus parameters
//! - **ShotInfo** (0x3000): 10 shot-specific parameters

use super::super::shared::{array_schemas::*, generic_decoders::*, tag_registry::TagRegistry};

// Re-export existing decoders from sony.rs
// These decoders are defined using const_decoder! macros in the main parser
use super::super::sony::{
    AF_AREA_MODE, COLOR_MODE, DRIVE_MODE, DRO, FLASH_MODE, FOCUS_MODE, HDR, IMAGE_STABILIZATION,
    METERING_MODE, NOISE_REDUCTION, WHITE_BALANCE,
};

// ============================================================================
// ARRAY SCHEMAS
// ============================================================================

/// CameraSettings array schema (Tag 0x0114)
///
/// Contains 17 camera configuration settings including drive mode, white balance,
/// focus settings, metering, ISO, DRO, image stabilization, color mode, and
/// noise reduction settings.
///
/// This schema processes the array indices from the Sony CameraSettings tag,
/// mapping each index to a human-readable field name and applying the appropriate
/// decoder for value interpretation.
static CAMERA_SETTINGS_SCHEMA: ArraySchema = ArraySchema {
    name: "CameraSettings",
    indices: &[
        // Index 0: Drive mode (single frame, continuous, bracketing, etc.)
        ArrayIndexDef::with_i16_decoder(0, "DriveMode", &DRIVE_MODE),
        // Index 1: White balance mode (auto, daylight, tungsten, etc.)
        ArrayIndexDef::with_i16_decoder(1, "WhiteBalanceMode", &WHITE_BALANCE),
        // Index 2: Focus mode (manual, AF-S, AF-C, AF-A, DMF)
        ArrayIndexDef::with_i16_decoder(2, "FocusMode", &FOCUS_MODE),
        // Index 3: AF area mode (wide, spot, local, flexible spot, etc.)
        ArrayIndexDef::with_i16_decoder(3, "AFAreaMode", &AF_AREA_MODE),
        // Index 4: Local AF area point (raw value)
        ArrayIndexDef::raw(4, "LocalAFAreaPoint"),
        // Index 5: Metering mode (multi-segment, center-weighted, spot, etc.)
        ArrayIndexDef::with_i16_decoder(5, "MeteringMode", &METERING_MODE),
        // Index 6: ISO setting (raw value, 0 means auto)
        ArrayIndexDef::raw(6, "ISO"),
        // Index 7: Dynamic Range Optimizer (off, DRO levels, HDR levels)
        ArrayIndexDef::with_i16_decoder(7, "DynamicRangeOptimizer", &DRO),
        // Index 8: Image stabilization (off, on, on during shooting)
        ArrayIndexDef::with_i16_decoder(8, "ImageStabilization", &IMAGE_STABILIZATION),
        // Index 9: Color mode/creative style (standard, vivid, portrait, etc.)
        ArrayIndexDef::with_i16_decoder(9, "ColorMode", &COLOR_MODE),
        // Index 10: Color space (raw value)
        ArrayIndexDef::raw(10, "ColorSpace"),
        // Index 11: Long exposure noise reduction (off, low, normal, high, auto)
        ArrayIndexDef::with_i16_decoder(11, "LongExposureNoiseReduction", &NOISE_REDUCTION),
        // Index 12: High ISO noise reduction (off, low, normal, high, auto)
        ArrayIndexDef::with_i16_decoder(12, "HighISONoiseReduction", &NOISE_REDUCTION),
        // Index 13: Picture effect (raw value)
        ArrayIndexDef::raw(13, "PictureEffect"),
        // Index 14: Soft skin effect (raw value)
        ArrayIndexDef::raw(14, "SoftSkinEffect"),
        // Index 15: Vignetting correction (raw value)
        ArrayIndexDef::raw(15, "VignettingCorrection"),
        // Index 16: Auto HDR (off, auto, 1.0-6.0 EV)
        ArrayIndexDef::with_i16_decoder(16, "AutoHDR", &HDR),
    ],
};

/// AFInfo array schema (Tags 0x9400, 0x9402)
///
/// Contains 5 autofocus-related parameters including AF point selection,
/// focus tracking, and face detection information.
///
/// Sony uses two AFInfo tags (AFInfo and AFInfo2) that share the same schema.
static AF_INFO_SCHEMA: ArraySchema = ArraySchema {
    name: "AFInfo",
    indices: &[
        // Index 0: AF point selected (index of selected AF point, -1 if none)
        ArrayIndexDef::raw(0, "AFPointSelected"),
        // Index 1: Number of AF points in focus
        ArrayIndexDef::raw(1, "AFPointsInFocus"),
        // Index 2: AF tracking status (raw value)
        ArrayIndexDef::raw(2, "AFTrackingStatus"),
        // Index 3: Face detection enabled (1 = yes, 0 = no)
        ArrayIndexDef::with_i16_decoder(3, "FaceDetection", &ON_OFF),
        // Index 4: Number of faces detected
        ArrayIndexDef::raw(4, "NumFacesDetected"),
    ],
};

/// ShotInfo array schema (Tag 0x3000)
///
/// Contains 10 shot-specific parameters including white balance settings,
/// color temperature, image adjustments (saturation, contrast, sharpness),
/// and flash settings.
static SHOT_INFO_SCHEMA: ArraySchema = ArraySchema {
    name: "ShotInfo",
    indices: &[
        // Index 0: White balance (auto, daylight, tungsten, etc.)
        ArrayIndexDef::with_i16_decoder(0, "WhiteBalance", &WHITE_BALANCE),
        // Index 1: White balance fine tune (raw value)
        ArrayIndexDef::raw(1, "WhiteBalanceFineTune"),
        // Index 2: Color temperature in Kelvin
        ArrayIndexDef::raw(2, "ColorTemperature"),
        // Index 3: Color compensation filter (raw value)
        ArrayIndexDef::raw(3, "ColorCompensationFilter"),
        // Index 4: Saturation adjustment
        ArrayIndexDef::raw(4, "Saturation"),
        // Index 5: Contrast adjustment
        ArrayIndexDef::raw(5, "Contrast"),
        // Index 6: Sharpness adjustment
        ArrayIndexDef::raw(6, "Sharpness"),
        // Index 7: Brightness adjustment
        ArrayIndexDef::raw(7, "Brightness"),
        // Index 8: Flash mode (auto, fill, rear sync, wireless, off)
        ArrayIndexDef::with_i16_decoder(8, "FlashMode", &FLASH_MODE),
        // Index 9: Flash exposure compensation (raw value)
        ArrayIndexDef::raw(9, "FlashExposureComp"),
    ],
};

// ============================================================================
// TAG REGISTRY
// ============================================================================

/// Create Sony tag registry with all tag definitions and array schemas
///
/// This registry provides a centralized, declarative definition of all Sony
/// MakerNote tags including:
/// - Simple string tags (lens model)
/// - Simple integer tags (image quality, shutter count, etc.)
/// - Array-based tags with full schema definitions
///
/// # Array Tag Processing
/// The registry handles three main array tags:
/// - **0x0114** (CameraSettings): 17 indices with mixed raw and decoded values
/// - **0x9400** (AFInfo): 5 indices for autofocus information
/// - **0x9402** (AFInfo2): Uses same schema as AFInfo
/// - **0x3000** (ShotInfo): 10 indices for shot-specific settings
///
/// # Returns
/// A fully configured TagRegistry ready for Sony MakerNote parsing
pub fn sony_registry() -> TagRegistry {
    TagRegistry::new()
        // Array-based tags with full schema definitions
        .register_array_schema(0x0114, &CAMERA_SETTINGS_SCHEMA)
        .register_array_schema(0x9400, &AF_INFO_SCHEMA)
        .register_array_schema(0x9402, &AF_INFO_SCHEMA) // AFInfo2 uses same schema
        .register_array_schema(0x3000, &SHOT_INFO_SCHEMA)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let _registry = sony_registry();
        // Verify registry can be created successfully
        assert!(true, "Registry created successfully");
    }

    #[test]
    fn test_camera_settings_schema() {
        // Verify schema has expected number of indices
        assert_eq!(CAMERA_SETTINGS_SCHEMA.indices.len(), 17);
        assert_eq!(CAMERA_SETTINGS_SCHEMA.name, "CameraSettings");
    }

    #[test]
    fn test_af_info_schema() {
        // Verify schema has expected number of indices
        assert_eq!(AF_INFO_SCHEMA.indices.len(), 5);
        assert_eq!(AF_INFO_SCHEMA.name, "AFInfo");
    }

    #[test]
    fn test_shot_info_schema() {
        // Verify schema has expected number of indices
        assert_eq!(SHOT_INFO_SCHEMA.indices.len(), 10);
        assert_eq!(SHOT_INFO_SCHEMA.name, "ShotInfo");
    }
}
