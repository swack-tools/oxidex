//! Sigma tag registry
//!
//! Registry of all Sigma MakerNote tags with their metadata and decoders.
//! Supports Sigma SD series, DP series, and fp/fp L cameras.

use super::super::shared::generic_decoders::SimpleValueDecoder;
use super::super::shared::tag_registry::TagRegistry;

// ============================================================================
// Sigma Decoders
// ============================================================================
// Inline decoders to avoid cross-module import issues

/// Decoder for Sigma resolution modes
const DECODE_RESOLUTION_MODE: SimpleValueDecoder<i32> =
    SimpleValueDecoder::new(&[(0, "Low"), (1, "Medium"), (2, "High"), (3, "Ultra High")]);

/// Decoder for Sigma autofocus modes
const DECODE_AF_MODE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Manual"),
    (1, "AF-S (Single)"),
    (2, "AF-C (Continuous)"),
    (3, "AF-A (Auto)"),
]);

/// Decoder for Sigma white balance settings
const DECODE_WHITE_BALANCE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Auto"),
    (1, "Daylight"),
    (2, "Shade"),
    (3, "Cloudy"),
    (4, "Tungsten"),
    (5, "Fluorescent"),
    (6, "Flash"),
    (7, "Custom"),
    (8, "Color Temperature"),
]);

/// Decoder for Sigma exposure modes
const DECODE_EXPOSURE_MODE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Auto"),
    (1, "Program"),
    (2, "Aperture Priority"),
    (3, "Shutter Priority"),
    (4, "Manual"),
]);

/// Decoder for Sigma metering modes
const DECODE_METERING_MODE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Unknown"),
    (1, "Multi-segment"),
    (2, "Center-weighted Average"),
    (3, "Spot"),
    (4, "Average"),
]);

/// Decoder for Sigma drive modes
const DECODE_DRIVE_MODE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Single"),
    (1, "Continuous"),
    (2, "Self-Timer"),
    (3, "Self-Timer (Multiple)"),
    (4, "Bracket"),
    (5, "Mirror Lock-up"),
]);

/// Decoder for Sigma flash modes
const DECODE_FLASH_MODE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Off"),
    (1, "Auto"),
    (2, "On"),
    (3, "Red-eye Reduction"),
    (4, "Fill Flash"),
    (5, "Slow Sync"),
    (6, "Rear Curtain"),
    (7, "Wireless"),
]);

/// Decoder for Sigma image quality settings
const DECODE_QUALITY: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Low"),
    (1, "Medium"),
    (2, "High"),
    (3, "RAW"),
    (4, "RAW + JPEG"),
]);

/// Decoder for Sigma color modes
const DECODE_COLOR_MODE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Standard"),
    (1, "Vivid"),
    (2, "Neutral"),
    (3, "Portrait"),
    (4, "Landscape"),
    (5, "Monochrome"),
    (6, "Sepia"),
    (7, "FOV Classic Blue"),
    (8, "FOV Classic Yellow"),
]);

/// Decoder for Sigma color space settings
const DECODE_COLOR_SPACE: SimpleValueDecoder<i32> =
    SimpleValueDecoder::new(&[(0, "sRGB"), (1, "Adobe RGB")]);

/// Decoder for Sigma picture styles
const DECODE_PICTURE_STYLE: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[
    (0, "Standard"),
    (1, "Vivid"),
    (2, "Neutral"),
    (3, "Portrait"),
    (4, "Landscape"),
    (5, "Monochrome"),
]);

// Wrapper functions to convert SimpleValueDecoder to function pointers
fn decode_resolution_mode(value: i32) -> String {
    DECODE_RESOLUTION_MODE.decode(value)
}
fn decode_af_mode(value: i32) -> String {
    DECODE_AF_MODE.decode(value)
}
fn decode_white_balance(value: i32) -> String {
    DECODE_WHITE_BALANCE.decode(value)
}
fn decode_exposure_mode(value: i32) -> String {
    DECODE_EXPOSURE_MODE.decode(value)
}
fn decode_metering_mode(value: i32) -> String {
    DECODE_METERING_MODE.decode(value)
}
fn decode_drive_mode(value: i32) -> String {
    DECODE_DRIVE_MODE.decode(value)
}
fn decode_flash_mode(value: i32) -> String {
    DECODE_FLASH_MODE.decode(value)
}
fn decode_quality(value: i32) -> String {
    DECODE_QUALITY.decode(value)
}
fn decode_color_mode(value: i32) -> String {
    DECODE_COLOR_MODE.decode(value)
}
fn decode_color_space(value: i32) -> String {
    DECODE_COLOR_SPACE.decode(value)
}
fn decode_picture_style(value: i32) -> String {
    DECODE_PICTURE_STYLE.decode(value)
}

/// Create and return the Sigma tag registry
///
/// This registry contains all known Sigma MakerNote tags including:
/// - Basic camera settings (drive mode, focus, exposure)
/// - Image quality parameters (resolution, contrast, sharpness)
/// - Lens information (type, ID, model)
/// - Foveon X3 sensor specific tags
/// - Advanced features (flash, calibration)
pub fn sigma_registry() -> TagRegistry {
    TagRegistry::new()
        // Basic Camera Information
        .register_raw(0x0002, "SerialNumber")
        .register_i32(0x0003, "DriveMode", decode_drive_mode)
        .register_i32(0x0004, "ResolutionMode", decode_resolution_mode)
        .register_i32(0x0005, "AFMode", decode_af_mode)
        .register_raw(0x0006, "FocusSetting")
        .register_i32(0x0007, "WhiteBalance", decode_white_balance)
        .register_i32(0x0008, "ExposureMode", decode_exposure_mode)
        .register_i32(0x0009, "MeteringMode", decode_metering_mode)
        .register_raw(0x000A, "LensRange")
        .register_i32(0x000B, "ColorSpace", decode_color_space)
        .register_raw(0x000C, "ExposureCompensation")
        .register_raw(0x000D, "Contrast")
        .register_raw(0x000E, "Shadow")
        .register_raw(0x000F, "Highlight")
        .register_raw(0x0010, "Saturation")
        .register_raw(0x0011, "Sharpness")
        .register_raw(0x0012, "FillLight")
        .register_raw(0x0014, "ColorAdjustment")
        .register_raw(0x0015, "AdjustmentMode")
        // Image Quality and Processing
        .register_i32(0x0016, "Quality", decode_quality)
        .register_raw(0x0017, "Firmware")
        .register_raw(0x0018, "Software")
        .register_raw(0x0019, "AutoBracket")
        // Lens Information
        .register_raw(0x001A, "LensType")
        .register_raw(0x001B, "LensID") // Used for lens database lookup
        .register_raw(0x001C, "LensModel")
        // Camera-Specific Settings
        .register_raw(0x001D, "CameraTemperature")
        .register_i32(0x001E, "ColorMode", decode_color_mode)
        .register_i32(0x001F, "PictureStyle", decode_picture_style)
        // Foveon X3 Sensor Specific
        .register_raw(0x0020, "X3FillLight")
        .register_raw(0x0021, "ColorHue")
        .register_raw(0x0022, "HueAdjustment")
        // Advanced Features
        .register_raw(0x0030, "ShutterCount")
        .register_i32(0x0031, "FlashMode", decode_flash_mode)
        .register_raw(0x0032, "FlashExposureComp")
        .register_raw(0x0033, "FlashMeteringMode")
        // File Format and Compression
        .register_raw(0x0040, "FileFormat")
        .register_raw(0x0041, "Compression")
        // Calibration and Corrections
        .register_raw(0x0050, "Calibration")
        .register_raw(0x0051, "DustRemovalData")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = sigma_registry();

        // Verify key tags are registered
        assert!(registry.has_tag(0x0002)); // SerialNumber
        assert!(registry.has_tag(0x0008)); // ExposureMode
        assert!(registry.has_tag(0x001B)); // LensID
        assert!(registry.has_tag(0x0030)); // ShutterCount
    }

    #[test]
    fn test_registry_tag_names() {
        let registry = sigma_registry();

        // Verify tag names
        assert_eq!(registry.get_tag_name(0x0002), Some("SerialNumber"));
        assert_eq!(registry.get_tag_name(0x0003), Some("DriveMode"));
        assert_eq!(registry.get_tag_name(0x001B), Some("LensID"));
        assert_eq!(registry.get_tag_name(0x0050), Some("Calibration"));
    }

    #[test]
    fn test_unknown_tag() {
        let registry = sigma_registry();
        assert!(!registry.has_tag(0xFFFF));
        assert_eq!(registry.get_tag_name(0xFFFF), None);
    }
}
