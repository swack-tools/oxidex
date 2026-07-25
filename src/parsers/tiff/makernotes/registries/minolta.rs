//! Minolta tag registry
//!
//! Registry of all Minolta MakerNote tags with their metadata and decoders.
//! Supports Minolta Maxxum/Dynax series and DiMAGE digital cameras.

use super::super::shared::tag_registry::TagRegistry;

// Re-export decoders from minolta.rs
use super::super::minolta::{
    DECODE_COLOR_MODE, DECODE_EXPOSURE_MODE, DECODE_FLASH_MODE, DECODE_FOCUS_MODE,
    DECODE_IMAGE_QUALITY, DECODE_SCENE_MODE, DECODE_WHITE_BALANCE,
};

// Wrapper functions to convert SimpleValueDecoder to function pointers
fn decode_image_quality(value: u16) -> String {
    DECODE_IMAGE_QUALITY.decode(value)
}
fn decode_flash_mode(value: u16) -> String {
    DECODE_FLASH_MODE.decode(value)
}
fn decode_white_balance(value: u16) -> String {
    DECODE_WHITE_BALANCE.decode(value)
}
fn decode_focus_mode(value: u16) -> String {
    DECODE_FOCUS_MODE.decode(value)
}
fn decode_color_mode(value: u16) -> String {
    DECODE_COLOR_MODE.decode(value)
}
fn decode_exposure_mode(value: u16) -> String {
    DECODE_EXPOSURE_MODE.decode(value)
}
fn decode_scene_mode(value: u16) -> String {
    DECODE_SCENE_MODE.decode(value)
}

/// Create and return the Minolta tag registry
///
/// This registry contains all known Minolta MakerNote tags including:
/// - Camera settings (old and new formats)
/// - Image quality and size parameters
/// - Flash settings and compensation
/// - Focus and exposure modes
/// - Lens information with database lookup support
/// - Color and scene modes
pub fn minolta_registry() -> TagRegistry {
    TagRegistry::new()
        // Camera Settings
        .register_raw(0x0001, "CameraSettingsOld")
        .register_raw(0x0003, "CameraSettings")
        // Image Parameters
        .register_raw(0x0040, "ImageSize")
        .register_u16(0x0041, "ImageQuality", decode_image_quality)
        // Flash Settings
        .register_u16(0x0042, "FlashMode", decode_flash_mode)
        .register_raw(0x0043, "FlashExposureComp")
        // Lens and Optical
        .register_raw(0x0044, "Teleconverter")
        .register_raw(0x0054, "LensID") // Used for lens database lookup
        .register_raw(0x0055, "MinFocalLength")
        .register_raw(0x0056, "MaxFocalLength")
        // White Balance and Color
        .register_u16(0x0045, "WhiteBalance", decode_white_balance)
        .register_u16(0x0050, "ColorMode", decode_color_mode)
        .register_raw(0x0046, "Brightness")
        .register_raw(0x004E, "Saturation")
        // Focus Settings
        .register_u16(0x0047, "FocusMode", decode_focus_mode)
        .register_raw(0x0048, "FocusDistance")
        .register_raw(0x0059, "AFPoints")
        // Zoom and Macro
        .register_raw(0x004A, "ZoomPosition")
        .register_raw(0x004B, "MacroMode")
        // Image Enhancement
        .register_raw(0x004C, "Sharpness")
        .register_raw(0x004D, "Contrast")
        // Shooting Modes
        .register_u16(0x0052, "SceneMode", decode_scene_mode)
        .register_u16(0x0053, "ExposureMode", decode_exposure_mode)
        // Camera Information
        .register_raw(0x0058, "FirmwareVersion")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = minolta_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0041)); // ImageQuality
        assert!(registry.has_tag(0x0042)); // FlashMode
        assert!(registry.has_tag(0x0054)); // LensID
        assert!(registry.has_tag(0x0053)); // ExposureMode
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = minolta_registry();

        assert_eq!(registry.get_tag_name(0x0041), Some("ImageQuality"));
        assert_eq!(registry.get_tag_name(0x0054), Some("LensID"));
        assert_eq!(registry.get_tag_name(0x0058), Some("FirmwareVersion"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = minolta_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
