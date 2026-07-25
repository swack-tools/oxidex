//! RED Cinema Camera tag registry
//!
//! This module provides TagRegistry definitions for RED MakerNotes.
//! RED cameras store comprehensive metadata about resolution, frame rate,
//! color science, lens settings, and sensor parameters.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from red.rs
use super::super::red::{
    DECODE_COLOR_SPACE, DECODE_CROP_MODE, DECODE_GAMMA, DECODE_LENS_TYPE, DECODE_REDCODE,
    DECODE_RESOLUTION,
};

/// Create RED tag registry with all tag definitions
///
/// This registry provides declarative definitions of all RED MakerNote tags
/// including camera model, sensor settings, frame rate, color science,
/// lens information, timecode, and image adjustments.
///
/// # Returns
/// A fully configured TagRegistry ready for RED MakerNote parsing
pub fn red_registry() -> TagRegistry {
    TagRegistry::new()
        // Camera Information (string tags, raw values)
        .register_raw(0x0001, "Model")
        .register_raw(0x0002, "SerialNumber")
        .register_raw(0x0003, "FirmwareVersion")
        // Sensor Information (string and i16 tags)
        .register_raw(0x0100, "Sensor")
        .register_simple_i16(0x0101, "Resolution", &DECODE_RESOLUTION)
        .register_simple_i16(0x0102, "REDCODE", &DECODE_REDCODE)
        // Frame and Exposure Settings (i16 tags, most raw except where noted)
        .register_raw(0x0103, "FrameRate")
        .register_raw(0x0104, "ShutterAngle")
        .register_raw(0x0105, "ISO")
        // Color and Tone Mapping
        .register_raw(0x0106, "ColorTemperature")
        .register_raw(0x0107, "Tint")
        .register_raw(0x0108, "ExposureCompensation")
        .register_simple_i16(0x0109, "GammaCurve", &DECODE_GAMMA)
        .register_simple_i16(0x010A, "ColorSpace", &DECODE_COLOR_SPACE)
        // Lens Information
        .register_simple_i16(0x010B, "LensMount", &DECODE_LENS_TYPE)
        .register_raw(0x010C, "FocalLength")
        .register_raw(0x010D, "FocusDistance")
        .register_raw(0x010E, "Aperture")
        // Timecode and Clip Information (string tags, raw values)
        .register_raw(0x010F, "Timecode")
        .register_raw(0x0110, "ReelNumber")
        .register_raw(0x0111, "ClipName")
        // Advanced Features
        .register_raw(0x0112, "HDRx")
        .register_raw(0x0113, "Look")
        .register_raw(0x0114, "ColorScience")
        .register_simple_i16(0x0115, "CropMode", &DECODE_CROP_MODE)
        .register_raw(0x0116, "ProjectFPS")
        .register_raw(0x0117, "KelvinOverride")
        // Image Adjustments (i16 tags, raw values)
        .register_raw(0x0118, "Shadow")
        .register_raw(0x0119, "Highlight")
        .register_raw(0x011A, "Saturation")
        .register_raw(0x011B, "Contrast")
        .register_raw(0x011C, "Sharpness")
        .register_raw(0x011D, "NoiseReduction")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = red_registry();
        assert!(registry.has_tag(0x0001)); // Model
        assert!(registry.has_tag(0x0101)); // Resolution
        assert!(registry.has_tag(0x0109)); // GammaCurve
    }

    #[test]
    fn test_tag_names() {
        let registry = red_registry();
        assert_eq!(registry.get_tag_name(0x0001), Some("Model"));
        assert_eq!(registry.get_tag_name(0x0101), Some("Resolution"));
        assert_eq!(registry.get_tag_name(0x0109), Some("GammaCurve"));
    }
}
