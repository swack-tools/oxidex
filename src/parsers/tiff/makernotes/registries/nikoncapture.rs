//! Nikon Capture NX-D/ViewNX-i tag registry
//!
//! Centralized tag definitions for Nikon Capture NX-D and ViewNX-i MakerNote parser.
//! This registry provides O(1) tag lookups and automatic value decoding,
//! eliminating code duplication and simplifying parser logic.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;
use once_cell::sync::Lazy;

// ============================================================================
// Shared Decoders
// ============================================================================

// Decoder for Picture Control presets
// Maps numeric Picture Control codes to their descriptive names.
const_decoder! {
    pub PICTURE_CONTROL,
    i16,
    [
        (0, "None"),
        (1, "Standard"),
        (2, "Neutral"),
        (3, "Vivid"),
        (4, "Monochrome"),
        (5, "Portrait"),
        (6, "Landscape"),
        (7, "Flat"),
        (8, "Creative"),
        (100, "Custom"),
    ]
}

// Decoder for Active D-Lighting strength levels
// Maps numeric Active D-Lighting codes to their strength descriptions.
const_decoder! {
    pub ACTIVE_D_LIGHTING,
    i16,
    [
        (0, "Off"),
        (1, "Low"),
        (2, "Normal"),
        (3, "High"),
        (4, "Extra High"),
        (5, "Auto"),
    ]
}

// Decoder for Vignette Control strength levels
// Maps numeric Vignette Control codes to their strength descriptions.
const_decoder! {
    pub VIGNETTE_CONTROL,
    i16,
    [
        (0, "Off"),
        (1, "Low"),
        (2, "Normal"),
        (3, "High"),
    ]
}

// Decoder for monochrome filter effects
// Maps numeric filter effect codes to color filter names.
const_decoder! {
    pub FILTER_EFFECT,
    i16,
    [
        (0, "None"),
        (1, "Yellow"),
        (2, "Orange"),
        (3, "Red"),
        (4, "Green"),
    ]
}

// Decoder for monochrome toning effects
// Maps numeric toning effect codes to their descriptive names.
const_decoder! {
    pub TONING_EFFECT,
    i16,
    [
        (0, "None"),
        (1, "Blue"),
        (2, "Red"),
        (3, "Yellow"),
        (4, "Green"),
        (5, "Blue-Green"),
        (6, "Blue-Purple"),
        (7, "Red-Purple"),
        (8, "Sepia"),
    ]
}

// Decoder for noise reduction strength levels
// Maps numeric noise reduction codes to their strength descriptions.
const_decoder! {
    pub NOISE_REDUCTION,
    i16,
    [
        (0, "Off"),
        (1, "Low"),
        (2, "Medium"),
        (3, "High"),
    ]
}

// Decoder for white balance modes
// Maps numeric white balance codes to mode descriptions.
const_decoder! {
    pub WHITE_BALANCE,
    i16,
    [
        (0, "Auto"),
        (1, "Daylight"),
        (2, "Cloudy"),
        (3, "Shade"),
        (4, "Tungsten"),
        (5, "Fluorescent"),
        (6, "Flash"),
        (7, "Custom"),
        (8, "Preset"),
    ]
}

// Decoder for label colors
// Maps numeric label codes to color names for image labeling.
const_decoder! {
    pub LABEL,
    i16,
    [
        (0, "None"),
        (1, "Red"),
        (2, "Yellow"),
        (3, "Green"),
        (4, "Blue"),
        (5, "Purple"),
    ]
}

// Decoder for On/Off boolean values
// Maps 0/1 values to "Off"/"On" strings for boolean tag values.
const_decoder! {
    pub ON_OFF,
    i16,
    [
        (0, "Off"),
        (1, "On"),
    ]
}

// ============================================================================
// Custom Formatter Functions
// ============================================================================

/// Formats adjustment level (-20 to +20)
///
/// Formats adjustment values with a +/- prefix to indicate direction
/// of adjustment from the neutral point.
///
/// # Arguments
/// * `value` - Adjustment value
///
/// # Returns
/// Formatted adjustment string with +/- prefix
pub fn format_adjustment(value: i16) -> String {
    if value >= 0 {
        format!("+{}", value)
    } else {
        format!("{}", value)
    }
}

/// Formats exposure compensation in EV units
///
/// Converts the raw exposure value (in thirds of EV) to a human-readable
/// exposure compensation string with a +/- prefix.
///
/// # Arguments
/// * `value` - Exposure in thirds of EV
///
/// # Returns
/// Formatted exposure compensation string (e.g., "+1.0 EV", "-2.0 EV")
pub fn format_exposure_comp(value: i16) -> String {
    let ev = value as f64 / 3.0;
    if ev >= 0.0 {
        format!("+{:.1} EV", ev)
    } else {
        format!("{:.1} EV", ev)
    }
}

/// Formats straighten angle in degrees
///
/// Converts the raw angle value (in tenths of a degree) to a formatted
/// angle string with a +/- prefix.
///
/// # Arguments
/// * `value` - Angle in tenths of degree
///
/// # Returns
/// Formatted angle string (e.g., "+1.5°", "-2.5°", "0°")
pub fn format_straighten(value: i16) -> String {
    let angle = value as f64 / 10.0;
    if angle.abs() < 0.1 {
        return "0°".to_string();
    }
    if angle >= 0.0 {
        format!("+{:.1}°", angle)
    } else {
        format!("{:.1}°", angle)
    }
}

/// Formats rating values
///
/// Converts numeric rating (0-5) to a star rating string.
/// Returns "None" for 0 or out-of-range values.
///
/// # Arguments
/// * `value` - Rating (0-5)
///
/// # Returns
/// Formatted rating string (e.g., "3 stars", "5 stars") or "None"
pub fn format_rating(value: i16) -> String {
    if !(0..=5).contains(&value) {
        return "None".to_string();
    }
    if value == 0 {
        "None".to_string()
    } else {
        format!("{} stars", value)
    }
}

/// Formats edit status
///
/// Converts edit status value to a descriptive string.
///
/// # Arguments
/// * `value` - Edit status (0 = original, non-zero = edited)
///
/// # Returns
/// "Original" or "Edited"
pub fn format_edit_status(value: i16) -> String {
    if value != 0 {
        "Edited".to_string()
    } else {
        "Original".to_string()
    }
}

// ============================================================================
// Tag Registry
// ============================================================================

/// Static tag registry for Nikon Capture MakerNote tags
///
/// This registry centralizes all tag definitions and their decoders,
/// providing O(1) tag lookups and automatic value formatting.
///
/// The registry uses once_cell::sync::Lazy for thread-safe lazy initialization,
/// ensuring it's built only once on first access.
///
/// ## Tag Organization:
/// - **Picture Control**: Preset selection and adjustments
/// - **Basic adjustments**: Sharpening, contrast, brightness, saturation, hue
/// - **Filter effects**: Color filters and toning for monochrome
/// - **Advanced processing**: Active D-Lighting, Vignette Control, distortion
/// - **Chromatic aberration**: Lateral and axial correction
/// - **Color tools**: Color Booster and Color Control Points
/// - **Noise reduction**: Luminance, edge, and color NR
/// - **Sharpening**: Unsharp mask settings
/// - **Geometry**: Straighten and crop
/// - **Retouch**: History, red-eye, dust removal
/// - **White balance**: Mode and fine-tuning
/// - **Exposure**: Compensation value
/// - **High ISO/Long exposure**: Noise reduction settings
/// - **Metadata**: Rating, label color, edit status
pub static NIKONCAPTURE_TAGS: Lazy<TagRegistry> = Lazy::new(|| {
    TagRegistry::with_capacity(60)
        // Decoder tags - using shared decoders
        .register_simple_i16(0x0012, "PictureControlAdjust", &PICTURE_CONTROL)
        .register_simple_i16(0x0018, "FilterEffect", &FILTER_EFFECT)
        .register_simple_i16(0x0019, "ToningEffect", &TONING_EFFECT)
        .register_simple_i16(0x0020, "ActiveDLighting", &ACTIVE_D_LIGHTING)
        .register_simple_i16(0x0021, "VignetteControl", &VIGNETTE_CONTROL)
        .register_simple_i16(0x0022, "AutoDistortion", &ON_OFF)
        .register_simple_i16(0x0023, "LateralChromaticAberration", &ON_OFF)
        .register_simple_i16(0x0024, "AxialChromaticAberration", &ON_OFF)
        .register_simple_i16(0x0030, "ColorBooster", &ON_OFF)
        .register_simple_i16(0x0040, "NoiseReduction", &NOISE_REDUCTION)
        .register_simple_i16(0x0041, "EdgeNoiseReduction", &NOISE_REDUCTION)
        .register_simple_i16(0x0042, "ColorNoiseReduction", &NOISE_REDUCTION)
        .register_simple_i16(0x0050, "UnsharpMask", &ON_OFF)
        .register_simple_i16(0x0072, "ImageDustOff", &ON_OFF)
        .register_simple_i16(0x0080, "WhiteBalanceMode", &WHITE_BALANCE)
        .register_simple_i16(0x0090, "HighISONR", &NOISE_REDUCTION)
        .register_simple_i16(0x0091, "LongExposureNR", &ON_OFF)
        .register_simple_i16(0x00A1, "Label", &LABEL)
        // Custom formatter tags
        .register_i16(0x0013, "Sharpening", format_adjustment)
        .register_i16(0x0014, "Contrast", format_adjustment)
        .register_i16(0x0015, "Brightness", format_adjustment)
        .register_i16(0x0016, "Saturation", format_adjustment)
        .register_i16(0x0017, "HueAdjustment", format_adjustment)
        .register_i16(0x001A, "ToningSaturation", format_adjustment)
        .register_i16(0x0032, "ColorBoosterLevel", format_adjustment)
        .register_i16(0x0051, "UnsharpAmount", format_adjustment)
        .register_i16(0x0060, "Straighten", format_straighten)
        .register_i16(0x0081, "WhiteBalanceFine", format_adjustment)
        .register_i16(0x0082, "ExposureComp", format_exposure_comp)
        .register_i16(0x00A0, "Rating", format_rating)
        .register_i16(0x00B0, "EditStatus", format_edit_status)
        // Raw count tags - no decoding needed
        .register_raw(0x0033, "ColorControlPoints")
        .register_raw(0x0052, "UnsharpRadius")
        .register_raw(0x0053, "UnsharpThreshold")
        .register_raw(0x0070, "RetouchHistoryCount")
        .register_raw(0x0071, "RedEyeCorrection")
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_picture_control() {
        assert_eq!(PICTURE_CONTROL.decode(1), "Standard");
        assert_eq!(PICTURE_CONTROL.decode(3), "Vivid");
        assert_eq!(PICTURE_CONTROL.decode(4), "Monochrome");
        assert_eq!(PICTURE_CONTROL.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_active_d_lighting() {
        assert_eq!(ACTIVE_D_LIGHTING.decode(0), "Off");
        assert_eq!(ACTIVE_D_LIGHTING.decode(3), "High");
        assert_eq!(ACTIVE_D_LIGHTING.decode(5), "Auto");
        assert_eq!(ACTIVE_D_LIGHTING.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_vignette_control() {
        assert_eq!(VIGNETTE_CONTROL.decode(0), "Off");
        assert_eq!(VIGNETTE_CONTROL.decode(2), "Normal");
        assert_eq!(VIGNETTE_CONTROL.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_format_adjustment() {
        assert_eq!(format_adjustment(10), "+10");
        assert_eq!(format_adjustment(-5), "-5");
    }

    #[test]
    fn test_format_exposure_comp() {
        assert_eq!(format_exposure_comp(3), "+1.0 EV");
        assert_eq!(format_exposure_comp(-6), "-2.0 EV");
    }

    #[test]
    fn test_format_straighten() {
        assert_eq!(format_straighten(15), "+1.5°");
        assert_eq!(format_straighten(-25), "-2.5°");
        assert_eq!(format_straighten(0), "0°");
    }

    #[test]
    fn test_decode_filter_effect() {
        assert_eq!(FILTER_EFFECT.decode(1), "Yellow");
        assert_eq!(FILTER_EFFECT.decode(3), "Red");
        assert_eq!(FILTER_EFFECT.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_format_rating() {
        assert_eq!(format_rating(0), "None");
        assert_eq!(format_rating(3), "3 stars");
        assert_eq!(format_rating(5), "5 stars");
    }
}
