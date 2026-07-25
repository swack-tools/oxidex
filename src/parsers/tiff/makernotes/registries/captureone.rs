//! Capture One Pro tag registry
//!
//! Centralized tag definitions for Capture One Pro MakerNote parser.
//! This registry provides O(1) tag lookups and automatic value decoding,
//! eliminating code duplication and simplifying parser logic.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;
use once_cell::sync::Lazy;

// ============================================================================
// Shared Decoders
// ============================================================================

// Decoder for style type values
// Maps numeric style type codes to descriptive strings indicating
// whether the style is built-in, user-created, or custom.
const_decoder! {
    pub STYLE_TYPE,
    i16,
    [
        (0, "None"),
        (1, "Built-in"),
        (2, "User"),
        (3, "Custom"),
    ]
}

// Decoder for base characteristics settings
// Maps numeric base characteristic codes to their descriptive names.
// Base characteristics control the fundamental tone curve of the image.
const_decoder! {
    pub BASE_CHAR,
    i16,
    [
        (0, "Film Standard"),
        (1, "Film Extra Shadow"),
        (2, "Film High Contrast"),
        (3, "Generic"),
        (4, "Linear Scientific"),
        (5, "Auto"),
    ]
}

// Decoder for color space values
// Maps numeric color space codes to standard color space names.
const_decoder! {
    pub COLOR_SPACE,
    i16,
    [
        (0, "sRGB"),
        (1, "Adobe RGB"),
        (2, "ProPhoto RGB"),
        (3, "Wide Gamut RGB"),
        (4, "Display P3"),
    ]
}

// Decoder for color tag values
// Maps numeric color tag codes to color names used for image labeling.
const_decoder! {
    pub COLOR_TAG,
    i16,
    [
        (0, "None"),
        (1, "Red"),
        (2, "Orange"),
        (3, "Yellow"),
        (4, "Green"),
        (5, "Blue"),
        (6, "Purple"),
    ]
}

// Decoder for Yes/No boolean values
// Maps 0/1 values to "No"/"Yes" strings for boolean tag values.
const_decoder! {
    pub YES_NO,
    i16,
    [
        (0, "No"),
        (1, "Yes"),
    ]
}

// ============================================================================
// Custom Formatter Functions
// ============================================================================

/// Formats exposure value in EV units
///
/// Converts the raw exposure value (in tenths of EV) to a human-readable
/// exposure compensation string with a +/- prefix.
///
/// # Arguments
/// * `value` - Exposure in tenths of EV
///
/// # Returns
/// Formatted exposure string (e.g., "+1.5 EV", "-0.5 EV")
pub fn format_exposure(value: i16) -> String {
    let ev = value as f64 / 10.0;
    if ev >= 0.0 {
        format!("+{:.1} EV", ev)
    } else {
        format!("{:.1} EV", ev)
    }
}

/// Formats percentage adjustment values
///
/// Formats adjustment values with a +/- prefix to indicate direction
/// of adjustment from the neutral point.
///
/// # Arguments
/// * `value` - Adjustment value (-100 to +100)
///
/// # Returns
/// Formatted percentage string with +/- prefix (e.g., "+25", "-50")
pub fn format_percentage(value: i16) -> String {
    if value >= 0 {
        format!("+{}", value)
    } else {
        format!("{}", value)
    }
}

/// Formats white balance Kelvin temperature
///
/// Converts the raw Kelvin value to the actual temperature by multiplying by 10.
/// Returns "Auto" for values <= 0.
///
/// # Arguments
/// * `value` - Temperature in units of 10K
///
/// # Returns
/// Formatted Kelvin string (e.g., "5500 K", "6500 K") or "Auto"
pub fn format_kelvin(value: i16) -> String {
    if value <= 0 {
        return "Auto".to_string();
    }
    format!("{} K", value * 10)
}

/// Formats tint adjustment values
///
/// Formats tint adjustment with a +/- prefix to indicate direction
/// (green/magenta shift).
///
/// # Arguments
/// * `value` - Tint value
///
/// # Returns
/// Formatted tint string with +/- prefix
pub fn format_tint(value: i16) -> String {
    if value >= 0 {
        format!("+{}", value)
    } else {
        format!("{}", value)
    }
}

/// Formats sharpening radius values
///
/// Converts the raw radius value to actual radius by dividing by 10.
///
/// # Arguments
/// * `value` - Radius value in tenths
///
/// # Returns
/// Formatted radius string (e.g., "1.5", "2.0")
pub fn format_radius(value: i16) -> String {
    let radius = value as f64 / 10.0;
    format!("{:.1}", radius)
}

/// Formats film grain size values
///
/// Maps numeric grain size codes to descriptive size names.
///
/// # Arguments
/// * `value` - Grain size code (0-2)
///
/// # Returns
/// Human-readable grain size ("Fine", "Medium", "Coarse")
pub fn format_grain_size(value: i16) -> String {
    match value {
        0 => "Fine".to_string(),
        1 => "Medium".to_string(),
        2 => "Coarse".to_string(),
        _ => format!("{}", value),
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

// ============================================================================
// Tag Registry
// ============================================================================

/// Static tag registry for Capture One MakerNote tags
///
/// This registry centralizes all tag definitions and their decoders,
/// providing O(1) tag lookups and automatic value formatting.
///
/// The registry uses once_cell::sync::Lazy for thread-safe lazy initialization,
/// ensuring it's built only once on first access.
///
/// ## Tag Organization:
/// - **Basic adjustments**: Exposure, contrast, brightness, saturation, etc.
/// - **Color grading**: Shadows, midtones, highlights color adjustments
/// - **Skin tone**: Hue, saturation, lightness for skin tones
/// - **Lens corrections**: Distortion, chromatic aberration, vignetting
/// - **Sharpening**: Amount, radius, threshold, halo suppression
/// - **Noise reduction**: Luminance, color, detail preservation
/// - **Film grain**: Amount, size, roughness
/// - **Layer counts**: Local adjustments, layers, masks
/// - **Tool usage**: Curve, levels, color editor flags
/// - **Base characteristics**: Film, generic, linear response curves
/// - **Metadata**: Rating, color tag, session info
pub static CAPTUREONE_TAGS: Lazy<TagRegistry> = Lazy::new(|| {
    TagRegistry::with_capacity(60)
        // Decoder tags - using shared decoders
        .register_simple_i16(0x0011, "StyleType", &STYLE_TYPE)
        .register_simple_i16(0x00B0, "BaseCharacteristicsFilm", &BASE_CHAR)
        .register_simple_i16(0x00B1, "BaseCharacteristicsGeneric", &BASE_CHAR)
        .register_simple_i16(0x00B2, "BaseCharacteristicsLinear", &BASE_CHAR)
        .register_simple_i16(0x00C1, "ColorSpace", &COLOR_SPACE)
        .register_simple_i16(0x00E1, "ColorTag", &COLOR_TAG)
        .register_simple_i16(0x00A0, "CurveAdjusted", &YES_NO)
        .register_simple_i16(0x00A1, "LevelsAdjusted", &YES_NO)
        .register_simple_i16(0x00A2, "ColorEditorAdjusted", &YES_NO)
        .register_simple_i16(0x00D2, "TetheredCapture", &YES_NO)
        // Custom formatter tags
        .register_i16(0x0020, "Exposure", format_exposure)
        .register_i16(0x0021, "Contrast", format_percentage)
        .register_i16(0x0022, "Brightness", format_percentage)
        .register_i16(0x0023, "Saturation", format_percentage)
        .register_i16(0x0024, "HDR", format_percentage)
        .register_i16(0x0025, "Clarity", format_percentage)
        .register_i16(0x0026, "Structure", format_percentage)
        .register_i16(0x0027, "Vibrance", format_percentage)
        .register_i16(0x0030, "WhiteBalanceKelvin", format_kelvin)
        .register_i16(0x0031, "Tint", format_tint)
        .register_i16(0x0040, "ColorGradingShadows", format_percentage)
        .register_i16(0x0041, "ColorGradingMidtones", format_percentage)
        .register_i16(0x0042, "ColorGradingHighlights", format_percentage)
        .register_i16(0x0043, "SkinToneHue", format_percentage)
        .register_i16(0x0044, "SkinToneSaturation", format_percentage)
        .register_i16(0x0045, "SkinToneLightness", format_percentage)
        .register_i16(0x0050, "LensDistortion", format_percentage)
        .register_i16(0x0051, "ChromaticAberration", format_percentage)
        .register_i16(0x0052, "Vignetting", format_percentage)
        .register_i16(0x0053, "PurpleFringing", format_percentage)
        .register_i16(0x0054, "LightFalloff", format_percentage)
        .register_i16(0x0060, "SharpeningAmount", format_percentage)
        .register_i16(0x0061, "SharpeningRadius", format_radius)
        .register_i16(0x0063, "SharpeningHalo", format_percentage)
        .register_i16(0x0070, "NoiseReductionLuminance", format_percentage)
        .register_i16(0x0071, "NoiseReductionColor", format_percentage)
        .register_i16(0x0072, "NoiseReductionDetail", format_percentage)
        .register_i16(0x0080, "FilmGrainAmount", format_percentage)
        .register_i16(0x0081, "FilmGrainSize", format_grain_size)
        .register_i16(0x0082, "FilmGrainRoughness", format_percentage)
        .register_i16(0x00E0, "Rating", format_rating)
        // Raw count tags - no decoding needed
        .register_raw(0x0062, "SharpeningThreshold")
        .register_raw(0x0090, "LocalAdjustmentCount")
        .register_raw(0x0091, "LayerCount")
        .register_raw(0x0092, "MaskCount")
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_style_type() {
        assert_eq!(STYLE_TYPE.decode(1), "Built-in");
        assert_eq!(STYLE_TYPE.decode(2), "User");
        assert_eq!(STYLE_TYPE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_decode_color_space() {
        assert_eq!(COLOR_SPACE.decode(0), "sRGB");
        assert_eq!(COLOR_SPACE.decode(2), "ProPhoto RGB");
        assert_eq!(COLOR_SPACE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_format_exposure() {
        assert_eq!(format_exposure(15), "+1.5 EV");
        assert_eq!(format_exposure(-10), "-1.0 EV");
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(format_percentage(25), "+25");
        assert_eq!(format_percentage(-50), "-50");
    }

    #[test]
    fn test_format_kelvin() {
        assert_eq!(format_kelvin(550), "5500 K");
        assert_eq!(format_kelvin(650), "6500 K");
        assert_eq!(format_kelvin(0), "Auto");
    }

    #[test]
    fn test_format_rating() {
        assert_eq!(format_rating(0), "None");
        assert_eq!(format_rating(3), "3 stars");
        assert_eq!(format_rating(5), "5 stars");
    }

    #[test]
    fn test_decode_color_tag() {
        assert_eq!(COLOR_TAG.decode(1), "Red");
        assert_eq!(COLOR_TAG.decode(4), "Green");
        assert_eq!(COLOR_TAG.decode(99), "Unknown (99)");
    }
}
