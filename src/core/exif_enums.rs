//! EXIF enumeration value decoding
//!
//! This module maps numeric EXIF tag values to human-readable strings,
//! matching ExifTool's output format. It handles both simple enum lookups
//! and complex bitmap decoding (like Flash values).
//!
//! The enums are stored in static `LazyLock<HashMap>` structures for efficient
//! lookup at runtime with minimal initialization overhead.
//!
//! # Supported Tags
//!
//! - ColorSpace (0xA001): sRGB, Adobe RGB, Uncalibrated
//! - Contrast (0xA408): Normal, Low, High
//! - CustomRendered (0xA401): Normal, Custom
//! - ExposureMode (0xA402): Auto, Manual, Auto bracket
//! - Flash (0x9209): Complex bitmap with fired/return/mode/function/red-eye
//! - GainControl (0xA407): None, Low/High gain up/down
//! - LightSource (0x9208): Daylight, Fluorescent, Tungsten, etc.
//! - MeteringMode (0x9207): Average, Center-weighted, Spot, Multi-segment, etc.
//! - Saturation (0xA409): Normal, Low, High
//! - SceneCaptureType (0xA406): Standard, Landscape, Portrait, Night
//! - SensingMethod (0xA217): One-chip, Two-chip, Three-chip color area, etc.
//! - Sharpness (0xA40A): Normal, Soft, Hard
//! - SubjectDistanceRange (0xA40C): Unknown, Macro, Close, Distant
//! - WhiteBalance (0xA403): Auto, Manual
//! - Orientation (0x0112): Horizontal, Rotate 90/180/270 CW, Mirror variants

use std::collections::HashMap;
use std::sync::LazyLock;

// =============================================================================
// Static Enum Lookup Tables
// =============================================================================

/// ColorSpace (tag 0xA001) - Color space information
///
/// Indicates the color space used by the image data. sRGB is the standard
/// for most digital cameras and web images.
pub static COLOR_SPACE: LazyLock<HashMap<u32, &'static str>> =
    LazyLock::new(|| HashMap::from([(1, "sRGB"), (2, "Adobe RGB"), (0xFFFF, "Uncalibrated")]));

/// Contrast (tag 0xA408) - Contrast processing applied
///
/// Indicates the direction of contrast processing applied by the camera.
pub static CONTRAST: LazyLock<HashMap<u32, &'static str>> =
    LazyLock::new(|| HashMap::from([(0, "Normal"), (1, "Low"), (2, "High")]));

/// CustomRendered (tag 0xA401) - Custom image processing
///
/// Indicates whether special processing was applied to the image data.
/// Extended values (2+) are from Apple's HDR/Portrait processing.
pub static CUSTOM_RENDERED: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (0, "Normal"),
        (1, "Custom"),
        (2, "HDR (no original saved)"),
        (3, "HDR (original saved)"),
        (4, "Original (for HDR)"),
        (6, "Panorama"),
        (7, "Portrait HDR"),
        (8, "Portrait"),
    ])
});

/// ExposureMode (tag 0xA402) - Exposure mode setting
///
/// Indicates the exposure mode set when the image was shot.
pub static EXPOSURE_MODE: LazyLock<HashMap<u32, &'static str>> =
    LazyLock::new(|| HashMap::from([(0, "Auto"), (1, "Manual"), (2, "Auto bracket")]));

/// GainControl (tag 0xA407) - Gain control applied
///
/// Indicates the degree of overall image gain adjustment.
pub static GAIN_CONTROL: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (0, "None"),
        (1, "Low gain up"),
        (2, "High gain up"),
        (3, "Low gain down"),
        (4, "High gain down"),
    ])
});

/// LightSource (tag 0x9208) - Kind of light source
///
/// Indicates the kind of light source used when the image was captured.
/// Values 17-24 are CIE standard illuminants.
pub static LIGHT_SOURCE: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (0, "Unknown"),
        (1, "Daylight"),
        (2, "Fluorescent"),
        (3, "Tungsten (Incandescent)"),
        (4, "Flash"),
        (9, "Fine Weather"),
        (10, "Cloudy"),
        (11, "Shade"),
        (12, "Daylight Fluorescent"),   // D 5700-7100K
        (13, "Day White Fluorescent"),  // N 4600-5500K
        (14, "Cool White Fluorescent"), // W 3800-4500K
        (15, "White Fluorescent"),      // WW 3250-3800K
        (17, "Standard Light A"),
        (18, "Standard Light B"),
        (19, "Standard Light C"),
        (20, "D55"),
        (21, "D65"),
        (22, "D75"),
        (23, "D50"),
        (24, "ISO Studio Tungsten"),
        (255, "Other"),
    ])
});

/// MeteringMode (tag 0x9207) - Metering mode
///
/// Indicates the metering mode used to determine exposure.
pub static METERING_MODE: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (0, "Unknown"),
        (1, "Average"),
        (2, "Center-weighted average"),
        (3, "Spot"),
        (4, "Multi-spot"),
        (5, "Multi-segment"),
        (6, "Partial"),
        (255, "Other"),
    ])
});

/// Saturation (tag 0xA409) - Saturation processing applied
///
/// Indicates the direction of saturation processing applied by the camera.
pub static SATURATION: LazyLock<HashMap<u32, &'static str>> =
    LazyLock::new(|| HashMap::from([(0, "Normal"), (1, "Low"), (2, "High")]));

/// SceneCaptureType (tag 0xA406) - Scene capture type
///
/// Indicates the type of scene that was shot, useful for automatic
/// scene selection in playback.
pub static SCENE_CAPTURE_TYPE: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (0, "Standard"),
        (1, "Landscape"),
        (2, "Portrait"),
        (3, "Night"),
    ])
});

/// SensingMethod (tag 0xA217) - Image sensor type
///
/// Indicates the image sensor type on the camera or input device.
pub static SENSING_METHOD: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (1, "Not defined"),
        (2, "One-chip color area"),
        (3, "Two-chip color area"),
        (4, "Three-chip color area"),
        (5, "Color sequential area"),
        (7, "Trilinear"),
        (8, "Color sequential linear"),
    ])
});

/// Sharpness (tag 0xA40A) - Sharpness processing applied
///
/// Indicates the direction of sharpness processing applied by the camera.
pub static SHARPNESS: LazyLock<HashMap<u32, &'static str>> =
    LazyLock::new(|| HashMap::from([(0, "Normal"), (1, "Soft"), (2, "Hard")]));

/// SubjectDistanceRange (tag 0xA40C) - Subject distance range
///
/// Indicates the distance to the subject, as a category.
pub static SUBJECT_DISTANCE_RANGE: LazyLock<HashMap<u32, &'static str>> =
    LazyLock::new(|| HashMap::from([(0, "Unknown"), (1, "Macro"), (2, "Close"), (3, "Distant")]));

/// WhiteBalance (tag 0xA403) - White balance mode
///
/// Indicates the white balance mode set when the image was shot.
pub static WHITE_BALANCE: LazyLock<HashMap<u32, &'static str>> =
    LazyLock::new(|| HashMap::from([(0, "Auto"), (1, "Manual")]));

/// Orientation (tag 0x0112) - Image orientation
///
/// Indicates the orientation of the image with respect to the rows and columns.
/// The value describes the position of row 0 and column 0 relative to the
/// visual content.
pub static ORIENTATION: LazyLock<HashMap<u32, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        (1, "Horizontal (normal)"),
        (2, "Mirror horizontal"),
        (3, "Rotate 180"),
        (4, "Mirror vertical"),
        (5, "Mirror horizontal and rotate 270 CW"),
        (6, "Rotate 90 CW"),
        (7, "Mirror horizontal and rotate 90 CW"),
        (8, "Rotate 270 CW"),
    ])
});

// =============================================================================
// Flash Bitmap Decoding
// =============================================================================

/// Decode Flash value (tag 0x9209) - bitmap decoding
///
/// The Flash tag is a complex bitmap where different bits indicate different
/// aspects of the flash status. This function decodes all bits and returns
/// a human-readable string matching ExifTool's output format.
///
/// # Bitmap Structure
///
/// | Bits  | Description                                      |
/// |-------|--------------------------------------------------|
/// | 0     | Flash fired (0 = No, 1 = Yes)                    |
/// | 1-2   | Return detection (0 = No strobe, 2 = Not detected, 3 = Detected) |
/// | 3-4   | Flash mode (0 = Unknown, 1 = On, 2 = Off, 3 = Auto) |
/// | 5     | Flash function (0 = Present, 1 = No flash function) |
/// | 6     | Red-eye reduction (0 = No, 1 = Yes)              |
///
/// # Examples
///
/// ```
/// use oxidex::core::exif_enums::decode_flash;
///
/// assert_eq!(decode_flash(0), "No Flash");
/// assert_eq!(decode_flash(1), "Fired");
/// assert_eq!(decode_flash(0x18), "Auto, Did not fire"); // auto mode, not fired
/// assert_eq!(decode_flash(0x19), "Auto, Fired"); // auto mode, fired
/// ```
pub fn decode_flash(value: u32) -> String {
    // Extract individual bit fields from the flash bitmap
    let fired = (value & 0x01) != 0; // Bit 0: flash fired
    let return_val = (value >> 1) & 0x03; // Bits 1-2: strobe return detection
    let mode = (value >> 3) & 0x03; // Bits 3-4: flash mode
    let function = (value >> 5) & 0x01; // Bit 5: flash function present
    let red_eye = (value >> 6) & 0x01; // Bit 6: red-eye reduction

    // Special case: no flash function
    if function == 1 {
        return "No flash function".to_string();
    }

    let mut parts = Vec::new();

    // Flash mode first (if known), then fired status
    // This matches ExifTool's format: "Mode, Fired/Did not fire"
    match mode {
        1 => {
            // Compulsory flash mode (On)
            parts.push("On");
            if fired {
                parts.push("Fired");
            } else {
                parts.push("Did not fire");
            }
        }
        2 => {
            // Compulsory suppression mode (Off)
            parts.push("Off");
            if fired {
                parts.push("Fired");
            } else {
                parts.push("Did not fire");
            }
        }
        3 => {
            // Auto mode
            parts.push("Auto");
            if fired {
                parts.push("Fired");
            } else {
                parts.push("Did not fire");
            }
        }
        _ => {
            // Unknown mode (0) - just show fired status
            if fired {
                parts.push("Fired");
            } else {
                parts.push("No Flash");
            }
        }
    }

    // Red-eye reduction mode
    if red_eye == 1 {
        parts.push("Red-eye reduction");
    }

    // Strobe return detection status (only meaningful if flash was fired)
    match return_val {
        2 => parts.push("Return not detected"),
        3 => parts.push("Return detected"),
        _ => {} // 0 = no strobe return detection function, 1 = reserved
    }

    parts.join(", ")
}

// =============================================================================
// Master Decode Function
// =============================================================================

/// Master function to decode EXIF enum values by tag ID
///
/// This function provides a single entry point for decoding any supported
/// EXIF enumeration value. It takes the tag ID and raw integer value,
/// returning the human-readable string if the tag/value combination is known.
///
/// # Arguments
///
/// * `tag_id` - The EXIF tag ID (e.g., 0xA001 for ColorSpace)
/// * `value` - The raw integer value to decode
///
/// # Returns
///
/// * `Some(String)` - The decoded human-readable value
/// * `None` - If the tag is not supported or the value is unknown
///
/// # Examples
///
/// ```
/// use oxidex::core::exif_enums::decode_exif_enum;
///
/// // ColorSpace
/// assert_eq!(decode_exif_enum(0xA001, 1), Some("sRGB".to_string()));
///
/// // Orientation
/// assert_eq!(decode_exif_enum(0x0112, 6), Some("Rotate 90 CW".to_string()));
///
/// // Unknown tag returns None
/// assert_eq!(decode_exif_enum(0x9999, 1), None);
/// ```
pub fn decode_exif_enum(tag_id: u16, value: u32) -> Option<String> {
    match tag_id {
        // EXIF IFD tags
        0xA001 => COLOR_SPACE.get(&value).map(|s| s.to_string()),
        0xA408 => CONTRAST.get(&value).map(|s| s.to_string()),
        0xA401 => CUSTOM_RENDERED.get(&value).map(|s| s.to_string()),
        0xA402 => EXPOSURE_MODE.get(&value).map(|s| s.to_string()),
        0x9209 => Some(decode_flash(value)), // Flash always returns a value
        0xA407 => GAIN_CONTROL.get(&value).map(|s| s.to_string()),
        0x9208 => LIGHT_SOURCE.get(&value).map(|s| s.to_string()),
        0x9207 => METERING_MODE.get(&value).map(|s| s.to_string()),
        0xA409 => SATURATION.get(&value).map(|s| s.to_string()),
        0xA406 => SCENE_CAPTURE_TYPE.get(&value).map(|s| s.to_string()),
        0xA217 => SENSING_METHOD.get(&value).map(|s| s.to_string()),
        0xA40A => SHARPNESS.get(&value).map(|s| s.to_string()),
        0xA40C => SUBJECT_DISTANCE_RANGE.get(&value).map(|s| s.to_string()),
        0xA403 => WHITE_BALANCE.get(&value).map(|s| s.to_string()),

        // IFD0 tags (also used in EXIF)
        0x0112 => ORIENTATION.get(&value).map(|s| s.to_string()),

        _ => None,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_space() {
        assert_eq!(decode_exif_enum(0xA001, 1), Some("sRGB".to_string()));
        assert_eq!(decode_exif_enum(0xA001, 2), Some("Adobe RGB".to_string()));
        assert_eq!(
            decode_exif_enum(0xA001, 0xFFFF),
            Some("Uncalibrated".to_string())
        );
        // Unknown value returns None
        assert_eq!(decode_exif_enum(0xA001, 999), None);
    }

    #[test]
    fn test_contrast() {
        assert_eq!(decode_exif_enum(0xA408, 0), Some("Normal".to_string()));
        assert_eq!(decode_exif_enum(0xA408, 1), Some("Low".to_string()));
        assert_eq!(decode_exif_enum(0xA408, 2), Some("High".to_string()));
    }

    #[test]
    fn test_custom_rendered() {
        assert_eq!(decode_exif_enum(0xA401, 0), Some("Normal".to_string()));
        assert_eq!(decode_exif_enum(0xA401, 1), Some("Custom".to_string()));
    }

    #[test]
    fn test_exposure_mode() {
        assert_eq!(decode_exif_enum(0xA402, 0), Some("Auto".to_string()));
        assert_eq!(decode_exif_enum(0xA402, 1), Some("Manual".to_string()));
        assert_eq!(
            decode_exif_enum(0xA402, 2),
            Some("Auto bracket".to_string())
        );
    }

    #[test]
    fn test_gain_control() {
        assert_eq!(decode_exif_enum(0xA407, 0), Some("None".to_string()));
        assert_eq!(decode_exif_enum(0xA407, 1), Some("Low gain up".to_string()));
        assert_eq!(
            decode_exif_enum(0xA407, 2),
            Some("High gain up".to_string())
        );
        assert_eq!(
            decode_exif_enum(0xA407, 3),
            Some("Low gain down".to_string())
        );
        assert_eq!(
            decode_exif_enum(0xA407, 4),
            Some("High gain down".to_string())
        );
    }

    #[test]
    fn test_light_source() {
        assert_eq!(decode_exif_enum(0x9208, 0), Some("Unknown".to_string()));
        assert_eq!(decode_exif_enum(0x9208, 1), Some("Daylight".to_string()));
        assert_eq!(decode_exif_enum(0x9208, 2), Some("Fluorescent".to_string()));
        assert_eq!(
            decode_exif_enum(0x9208, 3),
            Some("Tungsten (Incandescent)".to_string())
        );
        assert_eq!(decode_exif_enum(0x9208, 21), Some("D65".to_string()));
        assert_eq!(decode_exif_enum(0x9208, 255), Some("Other".to_string()));
    }

    #[test]
    fn test_metering_mode() {
        assert_eq!(decode_exif_enum(0x9207, 0), Some("Unknown".to_string()));
        assert_eq!(decode_exif_enum(0x9207, 1), Some("Average".to_string()));
        assert_eq!(
            decode_exif_enum(0x9207, 2),
            Some("Center-weighted average".to_string())
        );
        assert_eq!(decode_exif_enum(0x9207, 3), Some("Spot".to_string()));
        assert_eq!(decode_exif_enum(0x9207, 4), Some("Multi-spot".to_string()));
        assert_eq!(
            decode_exif_enum(0x9207, 5),
            Some("Multi-segment".to_string())
        );
        assert_eq!(decode_exif_enum(0x9207, 6), Some("Partial".to_string()));
        assert_eq!(decode_exif_enum(0x9207, 255), Some("Other".to_string()));
    }

    #[test]
    fn test_saturation() {
        assert_eq!(decode_exif_enum(0xA409, 0), Some("Normal".to_string()));
        assert_eq!(decode_exif_enum(0xA409, 1), Some("Low".to_string()));
        assert_eq!(decode_exif_enum(0xA409, 2), Some("High".to_string()));
    }

    #[test]
    fn test_scene_capture_type() {
        assert_eq!(decode_exif_enum(0xA406, 0), Some("Standard".to_string()));
        assert_eq!(decode_exif_enum(0xA406, 1), Some("Landscape".to_string()));
        assert_eq!(decode_exif_enum(0xA406, 2), Some("Portrait".to_string()));
        assert_eq!(decode_exif_enum(0xA406, 3), Some("Night".to_string()));
    }

    #[test]
    fn test_sensing_method() {
        assert_eq!(decode_exif_enum(0xA217, 1), Some("Not defined".to_string()));
        assert_eq!(
            decode_exif_enum(0xA217, 2),
            Some("One-chip color area".to_string())
        );
        assert_eq!(
            decode_exif_enum(0xA217, 3),
            Some("Two-chip color area".to_string())
        );
        assert_eq!(
            decode_exif_enum(0xA217, 4),
            Some("Three-chip color area".to_string())
        );
        assert_eq!(
            decode_exif_enum(0xA217, 5),
            Some("Color sequential area".to_string())
        );
        assert_eq!(decode_exif_enum(0xA217, 7), Some("Trilinear".to_string()));
        assert_eq!(
            decode_exif_enum(0xA217, 8),
            Some("Color sequential linear".to_string())
        );
    }

    #[test]
    fn test_sharpness() {
        assert_eq!(decode_exif_enum(0xA40A, 0), Some("Normal".to_string()));
        assert_eq!(decode_exif_enum(0xA40A, 1), Some("Soft".to_string()));
        assert_eq!(decode_exif_enum(0xA40A, 2), Some("Hard".to_string()));
    }

    #[test]
    fn test_subject_distance_range() {
        assert_eq!(decode_exif_enum(0xA40C, 0), Some("Unknown".to_string()));
        assert_eq!(decode_exif_enum(0xA40C, 1), Some("Macro".to_string()));
        assert_eq!(decode_exif_enum(0xA40C, 2), Some("Close".to_string()));
        assert_eq!(decode_exif_enum(0xA40C, 3), Some("Distant".to_string()));
    }

    #[test]
    fn test_white_balance() {
        assert_eq!(decode_exif_enum(0xA403, 0), Some("Auto".to_string()));
        assert_eq!(decode_exif_enum(0xA403, 1), Some("Manual".to_string()));
    }

    #[test]
    fn test_orientation() {
        assert_eq!(
            decode_exif_enum(0x0112, 1),
            Some("Horizontal (normal)".to_string())
        );
        assert_eq!(
            decode_exif_enum(0x0112, 2),
            Some("Mirror horizontal".to_string())
        );
        assert_eq!(decode_exif_enum(0x0112, 3), Some("Rotate 180".to_string()));
        assert_eq!(
            decode_exif_enum(0x0112, 4),
            Some("Mirror vertical".to_string())
        );
        assert_eq!(
            decode_exif_enum(0x0112, 5),
            Some("Mirror horizontal and rotate 270 CW".to_string())
        );
        assert_eq!(
            decode_exif_enum(0x0112, 6),
            Some("Rotate 90 CW".to_string())
        );
        assert_eq!(
            decode_exif_enum(0x0112, 7),
            Some("Mirror horizontal and rotate 90 CW".to_string())
        );
        assert_eq!(
            decode_exif_enum(0x0112, 8),
            Some("Rotate 270 CW".to_string())
        );
    }

    #[test]
    fn test_flash_decoding() {
        // Basic states (unknown mode)
        assert_eq!(decode_flash(0), "No Flash");
        assert_eq!(decode_flash(1), "Fired");

        // Flash with auto mode (bits 3-4 = 0b11 = 3, shifted left 3 = 0x18)
        // 0x18 = 0b00011000 = not fired + auto mode (bits 3-4)
        assert_eq!(decode_flash(0x18), "Auto, Did not fire");
        // 0x19 = 0b00011001 = fired (bit 0) + auto mode (bits 3-4)
        assert_eq!(decode_flash(0x19), "Auto, Fired");

        // Flash off mode (bits 3-4 = 0b10 = 2, shifted left 3 = 0x10)
        // 0x10 = 0b00010000 = not fired + off mode
        assert_eq!(decode_flash(0x10), "Off, Did not fire");
        // 0x14 = 0b00010100 = not fired + off mode + return not detected
        assert_eq!(decode_flash(0x14), "Off, Did not fire, Return not detected");

        // Flash on mode (bits 3-4 = 0b01 = 1, shifted left 3 = 0x08)
        // 0x08 = 0b00001000 = not fired + on mode
        assert_eq!(decode_flash(0x08), "On, Did not fire");
        // 0x09 = 0b00001001 = fired + on mode
        assert_eq!(decode_flash(0x09), "On, Fired");

        // Return detected (bits 1-2 = 0b11 = 3) - unknown mode
        // 0x07 = 0b00000111 = fired + return detected
        assert_eq!(decode_flash(0x07), "Fired, Return detected");

        // Return not detected (bits 1-2 = 0b10 = 2) - unknown mode
        // 0x05 = 0b00000101 = fired + return not detected
        assert_eq!(decode_flash(0x05), "Fired, Return not detected");

        // No flash function (bit 5)
        // 0x20 = 0b00100000 = no flash function
        assert_eq!(decode_flash(0x20), "No flash function");

        // Red-eye reduction (bit 6) - unknown mode
        // 0x41 = 0b01000001 = fired + red-eye reduction
        assert_eq!(decode_flash(0x41), "Fired, Red-eye reduction");

        // Complex: auto + fired + red-eye
        // 0x59 = 0b01011001 = fired + auto + red-eye
        assert_eq!(decode_flash(0x59), "Auto, Fired, Red-eye reduction");

        // Complex: auto + fired + return detected
        // 0x1F = 0b00011111 = fired + auto + return detected
        assert_eq!(decode_flash(0x1F), "Auto, Fired, Return detected");

        // Complex: on + red-eye
        // 0x49 = 0b01001001 = fired + on + red-eye
        assert_eq!(decode_flash(0x49), "On, Fired, Red-eye reduction");
    }

    #[test]
    fn test_unknown_tag() {
        // Unknown tag ID should return None
        assert_eq!(decode_exif_enum(0x9999, 1), None);
        assert_eq!(decode_exif_enum(0x0000, 0), None);
    }

    #[test]
    fn test_unknown_value_for_known_tag() {
        // Known tag with unknown value should return None
        assert_eq!(decode_exif_enum(0xA408, 99), None); // Contrast
        assert_eq!(decode_exif_enum(0x9207, 100), None); // MeteringMode
    }
}
