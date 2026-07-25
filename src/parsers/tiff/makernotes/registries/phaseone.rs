//! Phase One tag registry
//!
//! This module provides TagRegistry definitions for Phase One MakerNotes.
//! Phase One digital backs include extensive metadata about cameras, lenses,
//! and image processing settings.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from phaseone.rs
use super::super::phaseone::{
    DECODER_DRIVE_MODE, DECODER_EXPOSURE_MODE, DECODER_FLASH_MODE, DECODER_FOCUS_MODE,
    DECODER_METERING_MODE, DECODER_OFF_ON, DECODER_SYSTEM_TYPE, DECODER_WHITE_BALANCE,
};

/// Create Phase One tag registry with all tag definitions
///
/// This registry provides declarative definitions of all Phase One MakerNote tags
/// including camera information, sensor settings, exposure controls, image quality
/// parameters, and lens information.
///
/// # Returns
/// A fully configured TagRegistry ready for Phase One MakerNote parsing
pub fn phaseone_registry() -> TagRegistry {
    TagRegistry::new()
        // Basic Camera Information (mostly u32 raw values)
        .register_raw(0x0106, "Format")
        .register_raw(0x0107, "SerialNumber")
        .register_raw(0x0108, "SoftwareVersion")
        .register_simple_i32(0x0109, "SystemType", &DECODER_SYSTEM_TYPE)
        .register_raw(0x010A, "FirmwareVersion")
        .register_raw(0x010E, "SensorWidth")
        .register_raw(0x010F, "SensorHeight")
        .register_raw(0x0110, "SensorBitDepth")
        .register_raw(0x0111, "ImageWidth")
        .register_raw(0x0112, "ImageHeight")
        // Lens Information (mostly raw values)
        .register_raw(0x0211, "LensID")
        .register_raw(0x0212, "LensModel")
        .register_raw(0x0213, "LensSerialNumber")
        .register_raw(0x0214, "FocalLength")
        .register_raw(0x0215, "FocusDistance")
        // Exposure Settings (mostly raw, some with decoders)
        .register_raw(0x0401, "ISO")
        .register_raw(0x0402, "ShutterSpeed")
        .register_raw(0x0403, "Aperture")
        .register_raw(0x0404, "ExposureCompensation")
        .register_simple_i32(0x0405, "ExposureMode", &DECODER_EXPOSURE_MODE)
        .register_simple_i32(0x0406, "MeteringMode", &DECODER_METERING_MODE)
        .register_simple_i32(0x0407, "FlashMode", &DECODER_FLASH_MODE)
        // Image Quality and Processing
        .register_simple_i32(0x0412, "WhiteBalance", &DECODER_WHITE_BALANCE)
        .register_raw(0x0413, "ColorTemperature")
        .register_raw(0x0414, "Tint")
        .register_raw(0x0415, "Contrast")
        .register_raw(0x0416, "Saturation")
        .register_raw(0x0417, "Sharpness")
        .register_raw(0x0418, "NoiseReduction")
        .register_raw(0x0419, "HighISONoiseReduction")
        // Color Profile and Calibration
        .register_raw(0x0420, "CameraProfile")
        .register_raw(0x0421, "ColorMatrix")
        .register_raw(0x0422, "ColorProfile")
        // Capture Settings
        .register_simple_i32(0x0500, "DriveMode", &DECODER_DRIVE_MODE)
        .register_simple_i32(0x0501, "FocusMode", &DECODER_FOCUS_MODE)
        .register_simple_i32(0x0502, "MirrorLockup", &DECODER_OFF_ON)
        .register_simple_i32(0x0503, "LiveView", &DECODER_OFF_ON)
        // Advanced Features
        .register_raw(0x0600, "ShutterCount")
        .register_raw(0x0601, "SensorTemperature")
        .register_simple_i32(0x0602, "PixelShift", &DECODER_OFF_ON)
        .register_simple_i32(0x0603, "FocusStacking", &DECODER_OFF_ON)
        .register_simple_i32(0x0604, "LongExposureNR", &DECODER_OFF_ON)
        // IIQ (Intelligent Image Quality) Specific
        .register_raw(0x0700, "IIQVersion")
        .register_raw(0x0701, "DynamicRange")
        .register_raw(0x0702, "HighlightRecovery")
        .register_raw(0x0703, "ShadowRecovery")
        // Digital Back Metadata
        .register_raw(0x0800, "BackSerialNumber")
        .register_raw(0x0801, "BackType")
        .register_raw(0x0802, "SensorID")
        .register_simple_i32(0x0803, "SensorCleaning", &DECODER_OFF_ON)
        // Tethered Capture
        .register_raw(0x0900, "CaptureStyle")
        .register_raw(0x0901, "CameraSettings")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = phaseone_registry();
        assert!(registry.has_tag(0x0106)); // Format
        assert!(registry.has_tag(0x0109)); // SystemType
        assert!(registry.has_tag(0x0405)); // ExposureMode
    }

    #[test]
    fn test_tag_names() {
        let registry = phaseone_registry();
        assert_eq!(registry.get_tag_name(0x0106), Some("Format"));
        assert_eq!(registry.get_tag_name(0x0109), Some("SystemType"));
        assert_eq!(registry.get_tag_name(0x0405), Some("ExposureMode"));
    }
}
