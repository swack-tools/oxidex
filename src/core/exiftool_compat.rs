//! ExifTool Compatibility Formatting Module
//!
//! This module provides a single entry point for transforming raw parsed metadata values
//! into ExifTool-compatible formatted strings. ExifTool is the de facto standard for
//! metadata extraction, and maintaining output compatibility ensures users can rely on
//! consistent behavior when migrating from or comparing against ExifTool.
//!
//! # Architecture
//!
//! The formatting pipeline consists of:
//!
//! 1. **`format_for_exiftool()`** - Main entry point that iterates over all metadata
//!    and applies tag-specific formatting rules.
//!
//! 2. **`format_tag_value()`** - Dispatch function that examines tag names and routes
//!    values to the appropriate formatter based on a priority-ordered rule set.
//!
//! 3. **Helper functions** - Tag name classification utilities that determine which
//!    formatting rules apply to each tag.
//!
//! # Formatting Rules (in priority order)
//!
//! 1. GPS latitude/longitude references (N/S/E/W -> North/South/East/West)
//! 2. GPS direction references (T/M -> True North/Magnetic North)
//! 3. GPS speed/distance references (K/M/N -> km/h/mph/knots)
//! 4. GPS status tags (GPSStatus, GPSMeasureMode, GPSDifferential)
//! 5. GPS altitude reference (binary 0x00/0x01 -> Above/Below Sea Level)
//! 6. GPS processing method (binary data with encoding prefix)
//! 7. Binary decoders (CFAPattern, SceneType, version bytes)
//! 8. APP14 flags (APP14Flags0/APP14Flags1: 0 -> "(none)")
//! 9. Enum tags (ExposureProgram integer -> string description)
//! 10. ICC_Profile matrix tags (5 decimal precision, MeasurementFlare with % suffix)
//! 11. Integer precision tags (ReferenceBlackWhite: whole numbers)
//! 12. Three decimal precision tags (YCbCrCoefficients)
//! 13. UserComment (binary data with encoding prefix)
//! 14. ThumbnailImage (binary -> "(Binary data X bytes, use -b option to extract)")
//! 15. Percentage tags (Quality, MeasurementFlare: append %)
//! 16. Unit suffixes (FocalLength -> "X mm", GPSAltitude -> "X m")
//! 17. Special values (infinity -> "undef", -0 -> "0")
//! 18. Default: return original value unchanged
//!
//! # Example
//!
//! ```rust,ignore
//! use oxidex::core::{MetadataMap, TagValue};
//! use oxidex::core::exiftool_compat::format_for_exiftool;
//!
//! let mut metadata = MetadataMap::new();
//! metadata.insert("EXIF:GPSLatitudeRef", TagValue::String("N".to_string()));
//! metadata.insert("EXIF:FocalLength", TagValue::String("50".to_string()));
//!
//! let formatted = format_for_exiftool(&metadata);
//!
//! assert_eq!(formatted.get_string("EXIF:GPSLatitudeRef"), Some("North"));
//! assert_eq!(formatted.get_string("EXIF:FocalLength"), Some("50 mm"));
//! ```

use crate::core::binary_decoders::decode_user_comment;
use crate::core::formatters::gps_speed_ref::format_gps_dest_distance_ref;
use crate::core::formatters::gps_status::{
    format_gps_differential, format_gps_measure_mode, format_gps_status,
};
use crate::core::formatters::{
    decode_cfa_pattern, decode_gps_processing_method, decode_scene_type, decode_version_bytes,
    format_color_space, format_components_configuration, format_compression, format_contrast,
    format_custom_rendered, format_exposure_mode, format_exposure_program, format_file_source,
    format_flash, format_gain_control, format_gps_altitude_ref, format_gps_direction_ref,
    format_gps_lat_ref, format_gps_lon_ref, format_gps_speed_ref, format_icc_value,
    format_integer_precision_values, format_interop_index, format_light_source,
    format_metering_mode, format_orientation, format_resolution_unit, format_saturation,
    format_scene_capture_type, format_sensing_method, format_sharpness,
    format_subject_distance_range, format_three_decimal_values, format_white_balance,
    format_with_unit, format_ycbcr_positioning, is_icc_matrix_tag, is_integer_precision_tag,
    is_three_decimal_tag,
};
use crate::core::{MetadataMap, TagValue};

// =============================================================================
// MAIN PUBLIC API
// =============================================================================

/// Transforms all values in a MetadataMap to ExifTool-compatible formatted strings.
///
/// This is the main entry point for ExifTool compatibility formatting. It iterates
/// over every tag in the input metadata and applies the appropriate formatting rule
/// based on the tag name. Values that don't match any formatting rule are passed
/// through unchanged.
///
/// # Arguments
///
/// * `metadata` - Reference to the source MetadataMap containing raw parsed values
///
/// # Returns
///
/// A new MetadataMap with all values formatted for ExifTool compatibility.
/// Tag names are preserved exactly as they appear in the input.
///
/// # Performance
///
/// This function creates a new MetadataMap rather than modifying in place to
/// maintain immutability semantics. For large metadata sets, the overhead is
/// minimal compared to the formatting operations themselves.
///
/// # Example
///
/// ```rust,ignore
/// use oxidex::core::{MetadataMap, TagValue};
/// use oxidex::core::exiftool_compat::format_for_exiftool;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:ExposureProgram", TagValue::Integer(2));
/// metadata.insert("GPS:GPSLatitudeRef", TagValue::String("N".to_string()));
///
/// let formatted = format_for_exiftool(&metadata);
///
/// // ExposureProgram 2 -> "Program AE"
/// assert_eq!(formatted.get_string("EXIF:ExposureProgram"), Some("Program AE"));
/// // GPSLatitudeRef "N" -> "North"
/// assert_eq!(formatted.get_string("GPS:GPSLatitudeRef"), Some("North"));
/// ```
pub fn format_for_exiftool(metadata: &MetadataMap) -> MetadataMap {
    let mut result = MetadataMap::with_capacity(metadata.len());

    for (tag_name, value) in metadata.iter() {
        let formatted_value = format_tag_value(tag_name, value);
        result.insert(tag_name.clone(), formatted_value);
    }

    result
}

// =============================================================================
// TAG VALUE DISPATCH
// =============================================================================

/// Formats a single tag value based on the tag name.
///
/// This function implements the priority-ordered dispatch logic that determines
/// which formatter to apply to a given tag value. The dispatch order is designed
/// to handle the most specific cases first, falling back to more general rules.
///
/// # Dispatch Priority
///
/// 1. GPS string references (GPSLatitudeRef, GPSLongitudeRef)
/// 2. GPS direction refs (GPSImgDirectionRef, GPSDestBearingRef, GPSTrackRef)
/// 3. GPS speed/distance refs (GPSSpeedRef, GPSDestDistanceRef)
/// 4. GPS status tags (GPSStatus, GPSMeasureMode, GPSDifferential)
/// 5. GPS altitude ref (GPSAltitudeRef for binary 0x00/0x01)
/// 6. GPS processing method (GPSProcessingMethod for binary data)
/// 7. Binary decoders (CFAPattern, SceneType, version tags)
/// 8. APP14 flags (APP14Flags0, APP14Flags1: 0 -> "(none)")
/// 9. Enum tags (ExposureProgram)
/// 10. ICC_Profile matrix tags (5 decimal precision for color matrices, white points, etc.)
/// 11. Integer precision tags (ReferenceBlackWhite: whole numbers)
/// 12. Three decimal precision tags (YCbCrCoefficients)
/// 13. UserComment (binary data with encoding prefix)
/// 14. ThumbnailImage (binary -> "(Binary data X bytes, use -b option to extract)")
/// 15. Percentage tags (Quality, MeasurementFlare: append %)
/// 16. Unit suffixes (FocalLength, GPSAltitude)
/// 17. Special values (infinity -> "undef", -0 -> "0")
/// 18. Default: return original value unchanged
///
/// # Arguments
///
/// * `tag_name` - The full tag name, optionally with family prefix (e.g., "EXIF:FocalLength")
/// * `value` - The raw TagValue to format
///
/// # Returns
///
/// A new TagValue containing the formatted result. If no formatting rule applies,
/// returns a clone of the original value.
pub fn format_tag_value(tag_name: &str, value: &TagValue) -> TagValue {
    let base_name = strip_family_prefix(tag_name);

    // ---------------------------------------------------------------------
    // Rule 1: GPS Latitude/Longitude References
    // Convert single-character direction codes to full names
    // ---------------------------------------------------------------------
    if is_gps_lat_ref(base_name)
        && let Some(s) = value.as_string()
    {
        return TagValue::String(format_gps_lat_ref(s));
    }

    if is_gps_lon_ref(base_name)
        && let Some(s) = value.as_string()
    {
        return TagValue::String(format_gps_lon_ref(s));
    }

    // ---------------------------------------------------------------------
    // Rule 2: GPS Direction References (True North / Magnetic North)
    // ---------------------------------------------------------------------
    if is_gps_direction_ref(base_name)
        && let Some(s) = value.as_string()
    {
        return TagValue::String(format_gps_direction_ref(s));
    }

    // ---------------------------------------------------------------------
    // Rule 3: GPS Speed and Distance References
    // ---------------------------------------------------------------------
    if is_gps_speed_ref(base_name)
        && let Some(s) = value.as_string()
        && let Some(formatted) = format_gps_speed_ref(s)
    {
        return TagValue::String(formatted);
    }

    if is_gps_dest_distance_ref(base_name)
        && let Some(s) = value.as_string()
        && let Some(formatted) = format_gps_dest_distance_ref(s)
    {
        return TagValue::String(formatted);
    }

    // ---------------------------------------------------------------------
    // Rule 4: GPS Status Tags (GPSStatus, GPSMeasureMode, GPSDifferential)
    // ---------------------------------------------------------------------
    if is_gps_status_tag(base_name) {
        // Handle string values (e.g., "A", "V", "2", "3", "0", "1")
        if let Some(s) = value.as_string() {
            let formatted = match base_name {
                "GPSStatus" => format_gps_status(s),
                "GPSMeasureMode" => format_gps_measure_mode(s),
                "GPSDifferential" => format_gps_differential(s),
                _ => None,
            };
            if let Some(f) = formatted {
                return TagValue::String(f);
            }
        }
        // Handle integer values for GPSDifferential (0 -> "No Correction", 1 -> "Differential Corrected")
        if base_name == "GPSDifferential"
            && let Some(i) = value.as_integer()
        {
            let formatted = match i {
                0 => Some("No Correction".to_string()),
                1 => Some("Differential Corrected".to_string()),
                _ => None,
            };
            if let Some(f) = formatted {
                return TagValue::String(f);
            }
        }
    }

    // ---------------------------------------------------------------------
    // Rule 5: GPS Altitude Reference (binary 0x00/0x01)
    // ---------------------------------------------------------------------
    if is_gps_altitude_ref(base_name) {
        // Handle string values ("0", "1", "\x00", "\x01")
        if let Some(s) = value.as_string()
            && let Some(formatted) = format_gps_altitude_ref(s)
        {
            return TagValue::String(formatted);
        }
        // Handle binary values (single byte)
        if let TagValue::Binary(data) = value
            && !data.is_empty()
        {
            // Convert first byte to string for the formatter
            let byte_str = match data[0] {
                0 => "0",
                1 => "1",
                _ => return value.clone(),
            };
            if let Some(formatted) = format_gps_altitude_ref(byte_str) {
                return TagValue::String(formatted);
            }
        }
        // Handle integer values
        if let Some(i) = value.as_integer() {
            let int_str = match i {
                0 => "0",
                1 => "1",
                _ => return value.clone(),
            };
            if let Some(formatted) = format_gps_altitude_ref(int_str) {
                return TagValue::String(formatted);
            }
        }
    }

    // ---------------------------------------------------------------------
    // Rule 6: GPS Processing Method (binary data with encoding prefix)
    // ---------------------------------------------------------------------
    if is_gps_processing_method(base_name)
        && let TagValue::Binary(data) = value
    {
        let decoded = decode_gps_processing_method(data);
        if !decoded.is_empty() {
            return TagValue::String(decoded);
        }
    }

    // ---------------------------------------------------------------------
    // Rule 7: Binary Decoders (CFAPattern, SceneType, version bytes)
    // ---------------------------------------------------------------------
    if is_cfa_pattern(base_name)
        && let TagValue::Binary(data) = value
    {
        return TagValue::String(decode_cfa_pattern(data));
    }

    if is_scene_type(base_name) {
        if let TagValue::Binary(data) = value {
            let decoded = decode_scene_type(data);
            if !decoded.is_empty() {
                return TagValue::String(decoded);
            }
        }
        // Also handle integer values for SceneType
        if let Some(i) = value.as_integer() {
            if i == 1 {
                return TagValue::String("Directly photographed".to_string());
            } else {
                return TagValue::String(format!("Unknown ({})", i));
            }
        }
    }

    if is_version_tag(base_name)
        && let TagValue::Binary(data) = value
    {
        let decoded = decode_version_bytes(data);
        if !decoded.is_empty() {
            return TagValue::String(decoded);
        }
    }

    // GPSVersionID uses a different format than the ASCII-digit version tags:
    // the 4 raw bytes are joined as dot-separated decimal values (e.g. "2.2.0.0").
    if is_gps_version_id(base_name)
        && let TagValue::Binary(data) = value
    {
        return TagValue::String(format_gps_version_id(data));
    }

    // ---------------------------------------------------------------------
    // Rule 8: APP14 Flags (APP14Flags0, APP14Flags1)
    // ExifTool shows "(none)" for value 0, otherwise shows the value
    // ---------------------------------------------------------------------
    if is_app14_flags_tag(base_name)
        && let Some(i) = value.as_integer()
        && i == 0
    {
        return TagValue::String("(none)".to_string());
    }
    // Non-zero values are returned as-is (pass through to default)

    // ---------------------------------------------------------------------
    // Rule 9: Enum Tags (ExposureProgram and other EXIF enum tags)
    // Convert integer enum values to human-readable strings
    // ---------------------------------------------------------------------
    if is_exposure_program(base_name)
        && let Some(i) = value.as_integer()
    {
        // ExposureProgram values are typically small positive integers
        // Safe to cast from i64 to u32 for the formatter
        let formatted = format_exposure_program(i as u32);
        return TagValue::String(formatted);
    }

    // ColorSpace enum (1=sRGB, 65535=Uncalibrated)
    if base_name == "ColorSpace"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_color_space(i));
    }

    // MeteringMode enum (0-6, 255)
    if base_name == "MeteringMode"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_metering_mode(i));
    }

    // LightSource enum (0-24, 255)
    if base_name == "LightSource"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_light_source(i));
    }

    // Flash enum (complex bitfield)
    if base_name == "Flash"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_flash(i));
    }

    // ExposureMode enum (0=Auto, 1=Manual, 2=Auto bracket)
    if base_name == "ExposureMode"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_exposure_mode(i));
    }

    // WhiteBalance enum (0=Auto, 1=Manual)
    if base_name == "WhiteBalance"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_white_balance(i));
    }

    // SceneCaptureType enum (0-3)
    if base_name == "SceneCaptureType"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_scene_capture_type(i));
    }

    // Contrast enum (0=Normal, 1=Low, 2=High)
    if base_name == "Contrast"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_contrast(i));
    }

    // Saturation enum (0=Normal, 1=Low, 2=High)
    if base_name == "Saturation"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_saturation(i));
    }

    // Sharpness enum (0=Normal, 1=Soft, 2=Hard)
    if base_name == "Sharpness"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_sharpness(i));
    }

    // GainControl enum (0-4)
    if base_name == "GainControl"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_gain_control(i));
    }

    // FileSource enum (1-3)
    if base_name == "FileSource"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_file_source(i));
    }

    // SensingMethod enum (1-8)
    if base_name == "SensingMethod"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_sensing_method(i));
    }

    // Compression enum (1-65535)
    if base_name == "Compression"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_compression(i));
    }

    // Orientation enum (1-8)
    if base_name == "Orientation"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_orientation(i));
    }

    // ResolutionUnit enum (1-3)
    if base_name == "ResolutionUnit"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_resolution_unit(i));
    }

    // YCbCrPositioning enum (1=Centered, 2=Co-sited)
    if base_name == "YCbCrPositioning"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_ycbcr_positioning(i));
    }

    // CustomRendered enum (0-8)
    if base_name == "CustomRendered"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_custom_rendered(i));
    }

    // SubjectDistanceRange enum (0-3)
    if base_name == "SubjectDistanceRange"
        && let Some(i) = value.as_integer()
    {
        return TagValue::String(format_subject_distance_range(i));
    }

    // InteropIndex (R98=sRGB, THM=thumbnail, R03=Adobe RGB)
    if base_name == "InteropIndex"
        && let Some(s) = value.as_string()
    {
        return TagValue::String(format_interop_index(s));
    }

    // ComponentsConfiguration binary data
    if base_name == "ComponentsConfiguration"
        && let TagValue::Binary(data) = value
    {
        return TagValue::String(format_components_configuration(data));
    }

    // ---------------------------------------------------------------------
    // Rule 10: ICC_Profile Matrix Tags (5 decimal precision)
    // Format float values in ICC profile tags with up to 5 decimal places.
    // MeasurementFlare requires a "%" suffix after formatting.
    // ---------------------------------------------------------------------
    if is_icc_matrix_tag(base_name) {
        // Handle string values that contain space-separated floats
        // (e.g., "0.1491851806640625 0.0632171630859375 0.74456787109375")
        if let Some(s) = value.as_string() {
            let formatted = format_icc_string_values(s, base_name);
            return TagValue::String(formatted);
        }
        // Handle single float values
        if let Some(f) = value.as_float() {
            let formatted = format_icc_value(f);
            // Add "%" suffix for MeasurementFlare
            if base_name == "MeasurementFlare" {
                return TagValue::String(format!("{}%", formatted));
            }
            return TagValue::String(formatted);
        }
    }

    // ---------------------------------------------------------------------
    // Rule 11: Integer Precision Tags (ReferenceBlackWhite)
    // Format whole numbers without decimal places (0, 255, 128 not 0.0, 255.0)
    // ---------------------------------------------------------------------
    if is_integer_precision_tag(base_name)
        && let Some(s) = value.as_string()
    {
        let formatted = format_integer_precision_values(s);
        return TagValue::String(formatted);
    }

    // ---------------------------------------------------------------------
    // Rule 12: Three Decimal Precision Tags (YCbCrCoefficients)
    // Format with 3 decimal places (0.299 0.587 0.114 not 0.2990000000...)
    // ---------------------------------------------------------------------
    if is_three_decimal_tag(base_name)
        && let Some(s) = value.as_string()
    {
        let formatted = format_three_decimal_values(s);
        return TagValue::String(formatted);
    }

    // ---------------------------------------------------------------------
    // Rule 13: UserComment (decode binary text encoding)
    // UserComment has 8-byte encoding prefix (ASCII/UNICODE/JIS) + text
    // ---------------------------------------------------------------------
    if is_user_comment(base_name)
        && let TagValue::Binary(data) = value
        && let Some(decoded) = decode_user_comment(data)
    {
        // Only return if we got meaningful text (not empty or just nulls)
        if !decoded.is_empty() {
            return TagValue::String(decoded);
        }
    }

    // ---------------------------------------------------------------------
    // Rule 14: ThumbnailImage (format binary thumbnail data)
    // Format thumbnail images with ExifTool-compatible message
    // ---------------------------------------------------------------------
    if is_thumbnail_image(base_name)
        && let TagValue::Binary(data) = value
    {
        return TagValue::String(format!(
            "(Binary data {} bytes, use -b option to extract)",
            data.len()
        ));
    }

    // ---------------------------------------------------------------------
    // Rule 15: Percentage Tags (Quality, MeasurementFlare)
    // Append "%" suffix to numeric values representing percentages
    // Note: MeasurementFlare is also handled in the ICC matrix rule above,
    // but Quality (from Ducky segment) is handled here for integer values.
    // ---------------------------------------------------------------------
    // Note: only Ducky's "Quality" tag (or a bare, family-less "Quality")
    // gets a "%" suffix -- other formats that happen to share the tag name
    // (e.g. RIFF/AVI's numeric stream Quality, unrelated to percentages)
    // must NOT be reformatted here.
    let quality_percentage_applies =
        base_name == "MeasurementFlare" || tag_name == "Ducky:Quality" || tag_name == "Quality";
    if quality_percentage_applies && is_percentage_tag(base_name) {
        if let Some(i) = value.as_integer() {
            return TagValue::String(format!("{}%", i));
        }
        if let Some(f) = value.as_float() {
            // Format floats: remove trailing zeros for clean output
            // e.g., 84.0 -> "84%", 84.5 -> "84.5%"
            let formatted = if f.fract() == 0.0 {
                format!("{}%", f as i64)
            } else {
                format!("{}%", f)
            };
            return TagValue::String(formatted);
        }
    }

    // ---------------------------------------------------------------------
    // Rule 16: Unit Suffixes (FocalLength -> mm, GPSAltitude -> m)
    // ---------------------------------------------------------------------
    if is_unit_suffix_tag(base_name) {
        // For string values, apply unit suffix directly
        if let Some(s) = value.as_string() {
            return TagValue::String(format_with_unit(tag_name, s));
        }
        // For numeric values, convert to string first then apply suffix
        if let Some(i) = value.as_integer() {
            let formatted = format_with_unit(tag_name, &i.to_string());
            return TagValue::String(formatted);
        }
        if let Some(f) = value.as_float() {
            // Format floats reasonably - avoid excessive decimal places
            let float_str = if f.fract() == 0.0 {
                format!("{:.0}", f)
            } else {
                format!("{}", f)
            };
            let formatted = format_with_unit(tag_name, &float_str);
            return TagValue::String(formatted);
        }
        // Handle Rational values
        if let TagValue::Rational {
            numerator,
            denominator,
        } = value
            && *denominator != 0
        {
            let float_val = *numerator as f64 / *denominator as f64;
            let float_str = if float_val.fract() == 0.0 {
                format!("{:.0}", float_val)
            } else {
                format!("{}", float_val)
            };
            let formatted = format_with_unit(tag_name, &float_str);
            return TagValue::String(formatted);
        }
    }

    // ---------------------------------------------------------------------
    // Rule 17: Special Values (infinity -> "undef", -0 -> "0")
    // Handle special float/rational values that result from invalid/undefined data.
    // GPS tags like GPSDestBearing/GPSDestDistance produce infinity when
    // the denominator is 0. ExifTool displays "undef" for these cases.
    // Also handles string representations ("inf", "-0") for values already
    // converted to string.
    // ---------------------------------------------------------------------
    if let Some(f) = value.as_float()
        && let Some(formatted) = format_special_float_values(f)
    {
        return TagValue::String(formatted);
    }
    // Handle Rational values with denominator 0 (would produce infinity)
    if let TagValue::Rational { denominator, .. } = value
        && *denominator == 0
    {
        return TagValue::String("undef".to_string());
    }
    // Also handle string representations of special values
    if let Some(s) = value.as_string() {
        if s == "inf" || s == "-inf" || s == "Infinity" || s == "-Infinity" {
            return TagValue::String("undef".to_string());
        }
        if s == "-0" || s == "-0.0" {
            return TagValue::String("0".to_string());
        }
    }

    // ---------------------------------------------------------------------
    // Rule 18: XMP Boolean Formatting
    // ExifTool uses lowercase 'true'/'false' for XMP boolean values
    // Some parsers output title-case 'True'/'False' which we normalize here
    // ---------------------------------------------------------------------
    if tag_name.starts_with("XMP")
        && let Some(s) = value.as_string()
    {
        if s == "True" {
            return TagValue::String("true".to_string());
        }
        if s == "False" {
            return TagValue::String("false".to_string());
        }
    }

    // ---------------------------------------------------------------------
    // Rule 19: XMP LensInfo Formatting
    // ExifTool formats LensInfo as "45-100mm f/4" instead of raw rationals
    // Format: "{min}-{max}mm f/{f_min}[-{f_max}]" or "{focal}mm f/{f}" for primes
    // ---------------------------------------------------------------------
    if base_name == "LensInfo"
        && tag_name.starts_with("XMP")
        && let Some(s) = value.as_string()
        && let Some(formatted) = format_xmp_lens_info(s)
    {
        return TagValue::String(formatted);
    }

    // ---------------------------------------------------------------------
    // Rule 20: Default - Return original value unchanged
    // ---------------------------------------------------------------------
    value.clone()
}

// =============================================================================
// HELPER FUNCTIONS - Special Value Formatting
// =============================================================================

/// Formats special float values (infinity, negative zero) to match ExifTool output.
///
/// When GPS data has invalid rational values (e.g., denominator = 0), OxiDex
/// computes infinity. ExifTool instead shows "undef" for these cases. This
/// function handles these special cases to maintain ExifTool compatibility.
///
/// # Arguments
///
/// * `value` - The float value to check
///
/// # Returns
///
/// * `Some("undef")` - If the value is positive or negative infinity
/// * `Some("0")` - If the value is negative zero (-0.0)
/// * `None` - If the value is a normal number that should be formatted normally
///
/// # Why This Matters
///
/// GPS tags like GPSDestBearing and GPSDestDistance store values as rational
/// numbers (numerator/denominator). When the denominator is 0, division produces
/// infinity. ExifTool recognizes this as invalid data and displays "undef".
/// Similarly, negative zero can occur in edge cases and should normalize to "0".
///
/// # Examples
///
/// ```rust,ignore
/// assert_eq!(format_special_float_values(f64::INFINITY), Some("undef".to_string()));
/// assert_eq!(format_special_float_values(f64::NEG_INFINITY), Some("undef".to_string()));
/// assert_eq!(format_special_float_values(-0.0), Some("0".to_string()));
/// assert_eq!(format_special_float_values(42.5), None);
/// ```
fn format_special_float_values(value: f64) -> Option<String> {
    // Check for infinity (positive or negative) - indicates invalid rational (div by zero)
    if value.is_infinite() {
        return Some("undef".to_string());
    }

    // Check for negative zero - normalize to "0"
    // Note: -0.0 == 0.0 in Rust, so we use is_sign_negative() to detect it
    if value == 0.0 && value.is_sign_negative() {
        return Some("0".to_string());
    }

    // Normal value - no special formatting needed
    None
}

/// Formats XMP LensInfo from rational string to human-readable format.
///
/// XMP stores LensInfo as space-separated rationals like "4500/100 10000/100 400/100 400/100"
/// which ExifTool formats as "45-100mm f/4" for user-friendly display.
///
/// # Format Rules
/// - Prime lens (same min/max focal): "{focal}mm f/{f}"
/// - Zoom with constant aperture: "{min}-{max}mm f/{f}"
/// - Zoom with variable aperture: "{min}-{max}mm f/{f_min}-{f_max}"
///
/// # Arguments
///
/// * `value` - The raw XMP LensInfo string with space-separated rationals
///
/// # Returns
///
/// * `Some(formatted)` - If parsing succeeds
/// * `None` - If parsing fails (returns original value unchanged)
fn format_xmp_lens_info(value: &str) -> Option<String> {
    // Parse space-separated rationals: "4500/100 10000/100 400/100 400/100"
    let parts: Vec<&str> = value.split_whitespace().collect();
    if parts.len() != 4 {
        return None;
    }

    // Parse each rational (numerator/denominator)
    // Returns Some(value) for valid rationals, None for 0/0 (unknown)
    let parse_rational = |s: &str| -> Option<f64> {
        let r: Vec<&str> = s.split('/').collect();
        if r.len() == 2 {
            let num: f64 = r[0].parse().ok()?;
            let den: f64 = r[1].parse().ok()?;
            if den > 0.0 {
                return Some(num / den);
            }
            // 0/0 means unknown/unspecified
            if num == 0.0 && den == 0.0 {
                return None;
            }
        }
        None
    };

    let min_focal = parse_rational(parts[0])?;
    let max_focal = parse_rational(parts[1])?;
    // Apertures can be 0/0 (unknown)
    let f_at_min = parse_rational(parts[2]);
    let f_at_max = parse_rational(parts[3]);

    // Format focal length (integer if whole number, else one decimal)
    let format_focal = |f: f64| -> String {
        if (f.fract()).abs() < 0.01 {
            format!("{:.0}", f)
        } else {
            format!("{:.1}", f)
        }
    };

    // Format f-number
    let format_f = |f: f64| -> String {
        if (f.fract()).abs() < 0.01 {
            format!("{:.0}", f)
        } else {
            format!("{:.1}", f)
        }
    };

    // Build result
    let focal_str = if (min_focal - max_focal).abs() < 0.1 {
        // Prime lens
        format_focal(min_focal)
    } else {
        // Zoom lens
        format!("{}-{}", format_focal(min_focal), format_focal(max_focal))
    };

    // Handle unknown apertures (0/0 -> "?")
    let f_str = match (f_at_min, f_at_max) {
        (None, _) | (_, None) => "?".to_string(),
        (Some(f_min), Some(f_max)) => {
            if (f_min - f_max).abs() < 0.01 {
                format_f(f_min)
            } else {
                format!("{}-{}", format_f(f_min), format_f(f_max))
            }
        }
    };

    Some(format!("{}mm f/{}", focal_str, f_str))
}

// =============================================================================
// HELPER FUNCTIONS - Tag Name Classification
// =============================================================================

/// Strips the family/group prefix from a tag name.
///
/// Tag names may include a family prefix separated by a colon (e.g., "EXIF:Make",
/// "GPS:GPSLatitude", "XMP:Creator"). This function extracts just the base tag
/// name for comparison against formatting rules.
///
/// # Arguments
///
/// * `tag_name` - The full tag name, possibly with a family prefix
///
/// # Returns
///
/// The base tag name without any prefix. If there's no colon, returns the
/// original string unchanged.
///
/// # Examples
///
/// ```rust,ignore
/// assert_eq!(strip_family_prefix("EXIF:FocalLength"), "FocalLength");
/// assert_eq!(strip_family_prefix("GPS:GPSLatitude"), "GPSLatitude");
/// assert_eq!(strip_family_prefix("FocalLength"), "FocalLength");
/// ```
pub fn strip_family_prefix(tag_name: &str) -> &str {
    // Find the last colon to handle nested prefixes like "Composite:EXIF:Tag"
    tag_name.rsplit(':').next().unwrap_or(tag_name)
}

/// Checks if the tag is a GPS latitude reference (GPSLatitudeRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a latitude reference
pub fn is_gps_lat_ref(base_name: &str) -> bool {
    base_name == "GPSLatitudeRef"
}

/// Checks if the tag is a GPS longitude reference (GPSLongitudeRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a longitude reference
pub fn is_gps_lon_ref(base_name: &str) -> bool {
    base_name == "GPSLongitudeRef"
}

/// Checks if the tag is a GPS direction reference.
///
/// Direction reference tags indicate whether a direction measurement is relative
/// to True North or Magnetic North. Applicable tags:
/// - GPSImgDirectionRef
/// - GPSDestBearingRef
/// - GPSTrackRef
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a direction reference
pub fn is_gps_direction_ref(base_name: &str) -> bool {
    matches!(
        base_name,
        "GPSImgDirectionRef" | "GPSDestBearingRef" | "GPSTrackRef"
    )
}

/// Checks if the tag is a GPS speed reference (GPSSpeedRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a speed reference
pub fn is_gps_speed_ref(base_name: &str) -> bool {
    base_name == "GPSSpeedRef"
}

/// Checks if the tag is a GPS destination distance reference (GPSDestDistanceRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a distance reference
pub fn is_gps_dest_distance_ref(base_name: &str) -> bool {
    base_name == "GPSDestDistanceRef"
}

/// Checks if the tag is a GPS status-related tag.
///
/// Status tags include:
/// - GPSStatus (measurement active/void)
/// - GPSMeasureMode (2D/3D measurement)
/// - GPSDifferential (differential correction applied)
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a GPS status value
pub fn is_gps_status_tag(base_name: &str) -> bool {
    matches!(
        base_name,
        "GPSStatus" | "GPSMeasureMode" | "GPSDifferential"
    )
}

/// Checks if the tag is a GPS altitude reference (GPSAltitudeRef).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as an altitude reference
pub fn is_gps_altitude_ref(base_name: &str) -> bool {
    base_name == "GPSAltitudeRef"
}

/// Checks if the tag is GPS processing method (GPSProcessingMethod).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as GPS processing method binary data
pub fn is_gps_processing_method(base_name: &str) -> bool {
    base_name == "GPSProcessingMethod"
}

/// Checks if the tag is a CFA pattern (CFAPattern).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as CFA pattern binary data
pub fn is_cfa_pattern(base_name: &str) -> bool {
    // Also handle alternate spellings
    matches!(base_name, "CFAPattern" | "CFAPattern2")
}

/// Checks if the tag is scene type (SceneType).
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as scene type
pub fn is_scene_type(base_name: &str) -> bool {
    base_name == "SceneType"
}

/// Checks if the tag is a version tag that stores data as ASCII bytes.
///
/// Version tags include:
/// - InteropVersion
/// - ExifVersion
/// - FlashpixVersion
///
/// Note: `GPSVersionID` is *not* one of these -- unlike the tags above, its
/// 4 raw bytes are not ASCII digit characters. ExifTool prints it by joining
/// the 4 raw byte values with dots (e.g. `[2, 2, 0, 0]` -> `"2.2.0.0"`); see
/// [`is_gps_version_id`] / [`format_gps_version_id`] for that formatting.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as version bytes
pub fn is_version_tag(base_name: &str) -> bool {
    matches!(
        base_name,
        "InteropVersion" | "ExifVersion" | "FlashpixVersion"
    )
}

/// Checks if the tag is `GPSVersionID`.
///
/// Unlike the ASCII-digit version tags (`ExifVersion`, `InteropVersion`,
/// `FlashpixVersion`), `GPSVersionID`'s 4 raw bytes are small integers
/// (typically `[2, 2, 0, 0]`) that ExifTool prints by joining the decimal
/// byte values with dots, e.g. `"2.2.0.0"`.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded with [`format_gps_version_id`]
pub fn is_gps_version_id(base_name: &str) -> bool {
    base_name == "GPSVersionID"
}

/// Formats `GPSVersionID` raw bytes as dot-separated decimal values.
///
/// ExifTool prints the 4 raw bytes (e.g. `[2, 2, 0, 0]`) as `"2.2.0.0"`,
/// not as a concatenated ASCII digit string like the other EXIF version tags.
pub fn format_gps_version_id(data: &[u8]) -> String {
    data.iter()
        .map(|b| b.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// Checks if the tag is an APP14 flags tag (APP14Flags0, APP14Flags1).
///
/// These tags are used in JPEG APP14 (Adobe) segments to store processing flags.
/// ExifTool displays "(none)" when the value is 0, indicating no flags are set.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as an APP14 flags value
pub fn is_app14_flags_tag(base_name: &str) -> bool {
    matches!(base_name, "APP14Flags0" | "APP14Flags1")
}

/// Checks if the tag is ExposureProgram.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as an exposure program enum
pub fn is_exposure_program(base_name: &str) -> bool {
    base_name == "ExposureProgram"
}

/// Checks if the tag requires a unit suffix.
///
/// Tags that require unit suffixes:
/// - FocalLength, FocalLengthIn35mmFormat -> "mm"
/// - GPSAltitude, SubjectDistance -> "m"
///
/// Note: GPSAltitude is handled here for the unit suffix, not the reference value.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should have a unit suffix appended
pub fn is_unit_suffix_tag(base_name: &str) -> bool {
    matches!(
        base_name,
        "FocalLength"
            | "FocalLengthIn35mmFormat"
            | "FocalLength35efl"
            | "FocalLengthIn35mmFilm"
            | "GPSAltitude"
            | "SubjectDistance"
            | "HyperfocalDistance"
    )
}

/// Checks if the tag represents a percentage value that needs "%" suffix.
///
/// Percentage tags include:
/// - Quality (from Ducky segment in JPEG files) - image quality setting
/// - MeasurementFlare (from ICC_Profile) - flare measurement percentage
///
/// These tags store numeric values representing percentages, and ExifTool
/// displays them with a "%" suffix for clarity (e.g., "84" becomes "84%").
///
/// Note: MeasurementFlare is also handled by the ICC matrix formatting rule,
/// but is included here for consistency and to handle integer values.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should have a "%" suffix appended to numeric values
pub fn is_percentage_tag(base_name: &str) -> bool {
    // Quality from Ducky segment and MeasurementFlare from ICC_Profile need % suffix.
    // Note: MeasurementFlare strings are handled by ICC matrix rule first,
    // but integers fall through to this rule for the % suffix.
    matches!(base_name, "Quality" | "MeasurementFlare")
}

/// Checks if the tag is UserComment.
///
/// UserComment (tag 0x9286) stores text with an 8-byte encoding prefix
/// (ASCII, UNICODE, JIS) followed by the actual text content. This needs
/// special decoding to extract the human-readable text.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be decoded as UserComment
pub fn is_user_comment(base_name: &str) -> bool {
    base_name == "UserComment"
}

/// Checks if a tag name refers to a thumbnail image.
///
/// ThumbnailImage tags should be formatted with the ExifTool-compatible
/// message "(Binary data X bytes, use -b option to extract)" instead of
/// just showing the raw binary data.
///
/// # Arguments
///
/// * `base_name` - The tag name without family prefix
///
/// # Returns
///
/// `true` if this tag should be formatted as a thumbnail image
pub fn is_thumbnail_image(base_name: &str) -> bool {
    base_name == "ThumbnailImage"
}

// =============================================================================
// HELPER FUNCTIONS - Value Formatting
// =============================================================================

/// Formats space-separated float values in an ICC profile string with 5 decimal precision.
///
/// ICC profile matrix tags often contain multiple space-separated float values
/// (e.g., "0.1491851806640625 0.0632171630859375 0.74456787109375"). This function
/// parses each value, formats it with up to 5 decimal places, and reassembles
/// the string with spaces.
///
/// For MeasurementFlare, a "%" suffix is appended to the formatted result.
///
/// # Arguments
///
/// * `value` - The string containing space-separated float values
/// * `base_name` - The base tag name (used to detect MeasurementFlare for % suffix)
///
/// # Returns
///
/// A formatted string with each float value limited to 5 decimal places.
/// If parsing fails for any value, the original token is preserved.
///
/// # Examples
///
/// ```rust,ignore
/// // Matrix column with 3 values
/// let result = format_icc_string_values("0.1491851806640625 0.0632171630859375 0.74456787109375", "BlueMatrixColumn");
/// assert_eq!(result, "0.14919 0.06322 0.74457");
///
/// // MeasurementFlare with % suffix
/// let result = format_icc_string_values("0.01", "MeasurementFlare");
/// assert_eq!(result, "0.01%");
/// ```
fn format_icc_string_values(value: &str, base_name: &str) -> String {
    // Split the string by whitespace and format each numeric value
    let formatted_parts: Vec<String> = value
        .split_whitespace()
        .map(|part| {
            // Try to parse as f64 and format with 5 decimal precision
            if let Ok(f) = part.parse::<f64>() {
                format_icc_value(f)
            } else {
                // If parsing fails, keep the original value
                part.to_string()
            }
        })
        .collect();

    let result = formatted_parts.join(" ");

    // Add "%" suffix for MeasurementFlare
    if base_name == "MeasurementFlare" {
        format!("{}%", result)
    } else {
        result
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // strip_family_prefix tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_strip_family_prefix_with_prefix() {
        assert_eq!(strip_family_prefix("EXIF:Make"), "Make");
        assert_eq!(strip_family_prefix("GPS:GPSLatitude"), "GPSLatitude");
        assert_eq!(strip_family_prefix("XMP:Creator"), "Creator");
        assert_eq!(strip_family_prefix("IPTC:Keywords"), "Keywords");
    }

    #[test]
    fn test_strip_family_prefix_without_prefix() {
        assert_eq!(strip_family_prefix("Make"), "Make");
        assert_eq!(strip_family_prefix("FocalLength"), "FocalLength");
    }

    #[test]
    fn test_strip_family_prefix_nested() {
        // Should take the last segment after the final colon
        assert_eq!(strip_family_prefix("Composite:EXIF:Tag"), "Tag");
    }

    #[test]
    fn test_strip_family_prefix_empty() {
        assert_eq!(strip_family_prefix(""), "");
    }

    // -------------------------------------------------------------------------
    // GPS Latitude/Longitude Reference tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_lat_ref_formatting() {
        let value = TagValue::String("N".to_string());
        let formatted = format_tag_value("GPS:GPSLatitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("North"));

        let value = TagValue::String("S".to_string());
        let formatted = format_tag_value("GPSLatitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("South"));
    }

    #[test]
    fn test_gps_lon_ref_formatting() {
        let value = TagValue::String("E".to_string());
        let formatted = format_tag_value("GPS:GPSLongitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("East"));

        let value = TagValue::String("W".to_string());
        let formatted = format_tag_value("GPSLongitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("West"));
    }

    // -------------------------------------------------------------------------
    // GPS Direction Reference tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_direction_ref_formatting() {
        let value = TagValue::String("T".to_string());
        let formatted = format_tag_value("GPS:GPSImgDirectionRef", &value);
        assert_eq!(formatted.as_string(), Some("True North"));

        let value = TagValue::String("M".to_string());
        let formatted = format_tag_value("GPSTrackRef", &value);
        assert_eq!(formatted.as_string(), Some("Magnetic North"));

        let value = TagValue::String("T".to_string());
        let formatted = format_tag_value("GPSDestBearingRef", &value);
        assert_eq!(formatted.as_string(), Some("True North"));
    }

    // -------------------------------------------------------------------------
    // GPS Speed/Distance Reference tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_speed_ref_formatting() {
        let value = TagValue::String("K".to_string());
        let formatted = format_tag_value("GPS:GPSSpeedRef", &value);
        assert_eq!(formatted.as_string(), Some("km/h"));

        let value = TagValue::String("M".to_string());
        let formatted = format_tag_value("GPSSpeedRef", &value);
        assert_eq!(formatted.as_string(), Some("mph"));

        let value = TagValue::String("N".to_string());
        let formatted = format_tag_value("GPSSpeedRef", &value);
        assert_eq!(formatted.as_string(), Some("knots"));
    }

    #[test]
    fn test_gps_dest_distance_ref_formatting() {
        let value = TagValue::String("K".to_string());
        let formatted = format_tag_value("GPS:GPSDestDistanceRef", &value);
        assert_eq!(formatted.as_string(), Some("Kilometers"));

        let value = TagValue::String("M".to_string());
        let formatted = format_tag_value("GPSDestDistanceRef", &value);
        assert_eq!(formatted.as_string(), Some("Miles"));

        let value = TagValue::String("N".to_string());
        let formatted = format_tag_value("GPSDestDistanceRef", &value);
        assert_eq!(formatted.as_string(), Some("Nautical Miles"));
    }

    // -------------------------------------------------------------------------
    // GPS Status Tags tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_status_formatting() {
        let value = TagValue::String("A".to_string());
        let formatted = format_tag_value("GPS:GPSStatus", &value);
        assert_eq!(formatted.as_string(), Some("Measurement Active"));

        let value = TagValue::String("V".to_string());
        let formatted = format_tag_value("GPSStatus", &value);
        assert_eq!(formatted.as_string(), Some("Measurement Void"));
    }

    #[test]
    fn test_gps_measure_mode_formatting() {
        let value = TagValue::String("2".to_string());
        let formatted = format_tag_value("GPS:GPSMeasureMode", &value);
        assert_eq!(formatted.as_string(), Some("2-Dimensional Measurement"));

        let value = TagValue::String("3".to_string());
        let formatted = format_tag_value("GPSMeasureMode", &value);
        assert_eq!(formatted.as_string(), Some("3-Dimensional Measurement"));
    }

    #[test]
    fn test_gps_differential_formatting() {
        let value = TagValue::String("0".to_string());
        let formatted = format_tag_value("GPS:GPSDifferential", &value);
        assert_eq!(formatted.as_string(), Some("No Correction"));

        let value = TagValue::String("1".to_string());
        let formatted = format_tag_value("GPSDifferential", &value);
        assert_eq!(formatted.as_string(), Some("Differential Corrected"));
    }

    #[test]
    fn test_gps_differential_from_integer() {
        // Test integer 0 -> "No Correction"
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("GPS:GPSDifferential", &value);
        assert_eq!(formatted.as_string(), Some("No Correction"));

        // Test integer 1 -> "Differential Corrected"
        let value = TagValue::Integer(1);
        let formatted = format_tag_value("GPSDifferential", &value);
        assert_eq!(formatted.as_string(), Some("Differential Corrected"));

        // Test unknown integer value - should pass through unchanged
        let value = TagValue::Integer(2);
        let formatted = format_tag_value("GPSDifferential", &value);
        assert_eq!(formatted.as_integer(), Some(2));
    }

    // -------------------------------------------------------------------------
    // GPS Altitude Reference tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_altitude_ref_from_string() {
        let value = TagValue::String("0".to_string());
        let formatted = format_tag_value("GPS:GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Above Sea Level"));

        let value = TagValue::String("1".to_string());
        let formatted = format_tag_value("GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Below Sea Level"));
    }

    #[test]
    fn test_gps_altitude_ref_from_binary() {
        let value = TagValue::Binary(vec![0]);
        let formatted = format_tag_value("GPS:GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Above Sea Level"));

        let value = TagValue::Binary(vec![1]);
        let formatted = format_tag_value("GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Below Sea Level"));
    }

    #[test]
    fn test_gps_altitude_ref_from_integer() {
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("GPS:GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Above Sea Level"));

        let value = TagValue::Integer(1);
        let formatted = format_tag_value("GPSAltitudeRef", &value);
        assert_eq!(formatted.as_string(), Some("Below Sea Level"));
    }

    // -------------------------------------------------------------------------
    // GPS Processing Method tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_gps_processing_method_formatting() {
        // ASCII-encoded "GPS" method
        let data = b"ASCII\0\0\0GPS\0\0\0\0\0".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("GPS:GPSProcessingMethod", &value);
        assert_eq!(formatted.as_string(), Some("GPS"));
    }

    // -------------------------------------------------------------------------
    // Binary Decoder tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_cfa_pattern_formatting() {
        // 2x2 RGGB Bayer pattern
        let data = vec![0, 2, 0, 2, 0, 1, 1, 2];
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:CFAPattern", &value);
        assert_eq!(formatted.as_string(), Some("[Red,Green][Green,Blue]"));
    }

    #[test]
    fn test_scene_type_formatting() {
        // Binary value 1 = "Directly photographed"
        let value = TagValue::Binary(vec![1]);
        let formatted = format_tag_value("EXIF:SceneType", &value);
        assert_eq!(formatted.as_string(), Some("Directly photographed"));
    }

    #[test]
    fn test_scene_type_from_integer() {
        let value = TagValue::Integer(1);
        let formatted = format_tag_value("EXIF:SceneType", &value);
        assert_eq!(formatted.as_string(), Some("Directly photographed"));

        let value = TagValue::Integer(5);
        let formatted = format_tag_value("SceneType", &value);
        assert_eq!(formatted.as_string(), Some("Unknown (5)"));
    }

    #[test]
    fn test_version_bytes_formatting() {
        let data = b"0100".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:InteropVersion", &value);
        assert_eq!(formatted.as_string(), Some("0100"));

        let data = b"0232".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("ExifVersion", &value);
        assert_eq!(formatted.as_string(), Some("0232"));

        let data = b"0100".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("FlashpixVersion", &value);
        assert_eq!(formatted.as_string(), Some("0100"));

        // GPSVersionID is NOT decoded like the ASCII-digit version tags above:
        // its 4 raw bytes are small integers joined with dots (e.g. "2.2.0.0"),
        // not a concatenated ASCII digit string.
        let data = vec![2u8, 2, 0, 0];
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("GPSVersionID", &value);
        assert_eq!(formatted.as_string(), Some("2.2.0.0"));
    }

    // -------------------------------------------------------------------------
    // APP14 Flags tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_app14_flags_zero_returns_none() {
        // APP14Flags0 with value 0 should return "(none)"
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("JPEG:APP14Flags0", &value);
        assert_eq!(formatted.as_string(), Some("(none)"));

        // APP14Flags1 with value 0 should return "(none)"
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("APP14Flags1", &value);
        assert_eq!(formatted.as_string(), Some("(none)"));
    }

    #[test]
    fn test_app14_flags_nonzero_passes_through() {
        // Non-zero APP14Flags0 should pass through unchanged
        let value = TagValue::Integer(1);
        let formatted = format_tag_value("JPEG:APP14Flags0", &value);
        assert_eq!(formatted.as_integer(), Some(1));

        // Non-zero APP14Flags1 should pass through unchanged
        let value = TagValue::Integer(42);
        let formatted = format_tag_value("APP14Flags1", &value);
        assert_eq!(formatted.as_integer(), Some(42));
    }

    #[test]
    fn test_is_app14_flags_tag() {
        assert!(is_app14_flags_tag("APP14Flags0"));
        assert!(is_app14_flags_tag("APP14Flags1"));
        assert!(!is_app14_flags_tag("APP14Flags2"));
        assert!(!is_app14_flags_tag("APP14"));
        assert!(!is_app14_flags_tag("Flags0"));
    }

    // -------------------------------------------------------------------------
    // Enum Tag tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_exposure_program_formatting() {
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("EXIF:ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Not Defined"));

        let value = TagValue::Integer(1);
        let formatted = format_tag_value("ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Manual"));

        let value = TagValue::Integer(2);
        let formatted = format_tag_value("ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Program AE"));

        let value = TagValue::Integer(3);
        let formatted = format_tag_value("ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Aperture-priority AE"));

        let value = TagValue::Integer(99);
        let formatted = format_tag_value("ExposureProgram", &value);
        assert_eq!(formatted.as_string(), Some("Unknown (99)"));
    }

    #[test]
    fn test_xmp_boolean_formatting() {
        // XMP boolean values should be lowercase to match ExifTool
        let value = TagValue::String("True".to_string());
        let formatted = format_tag_value("XMP:AlreadyApplied", &value);
        assert_eq!(formatted.as_string(), Some("true"));

        let value = TagValue::String("False".to_string());
        let formatted = format_tag_value("XMP-crs:HasCrop", &value);
        assert_eq!(formatted.as_string(), Some("false"));

        // Already lowercase should stay unchanged
        let value = TagValue::String("true".to_string());
        let formatted = format_tag_value("XMP:Tagged", &value);
        assert_eq!(formatted.as_string(), Some("true"));

        // Non-boolean XMP values should be unchanged
        let value = TagValue::String("Normal".to_string());
        let formatted = format_tag_value("XMP:ProcessVersion", &value);
        assert_eq!(formatted.as_string(), Some("Normal"));
    }

    #[test]
    fn test_xmp_lens_info_formatting() {
        // Zoom lens with constant aperture: 45-100mm f/4
        let value = TagValue::String("4500/100 10000/100 400/100 400/100".to_string());
        let formatted = format_tag_value("XMP:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("45-100mm f/4"));

        // Prime lens: 50mm f/1.8
        let value = TagValue::String("500/10 500/10 18/10 18/10".to_string());
        let formatted = format_tag_value("XMP:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("50mm f/1.8"));

        // Variable aperture zoom: 18-55mm f/3.5-5.6
        let value = TagValue::String("1800/100 5500/100 350/100 560/100".to_string());
        let formatted = format_tag_value("XMP:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("18-55mm f/3.5-5.6"));

        // Unknown aperture (0/0): 50mm f/?
        let value = TagValue::String("50/1 50/1 0/0 0/0".to_string());
        let formatted = format_tag_value("XMP:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("50mm f/?"));

        // Non-XMP LensInfo should not be formatted
        let value = TagValue::String("24/1 70/1 28/10 28/10".to_string());
        let formatted = format_tag_value("EXIF:LensInfo", &value);
        assert_eq!(formatted.as_string(), Some("24/1 70/1 28/10 28/10"));
    }

    // -------------------------------------------------------------------------
    // Unit Suffix tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_focal_length_unit_suffix() {
        let value = TagValue::String("50".to_string());
        let formatted = format_tag_value("EXIF:FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("50 mm"));

        let value = TagValue::String("31".to_string());
        let formatted = format_tag_value("FocalLengthIn35mmFormat", &value);
        assert_eq!(formatted.as_string(), Some("31 mm"));
    }

    #[test]
    fn test_focal_length_from_integer() {
        let value = TagValue::Integer(50);
        let formatted = format_tag_value("EXIF:FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("50 mm"));
    }

    #[test]
    fn test_focal_length_from_float() {
        let value = TagValue::Float(50.0);
        let formatted = format_tag_value("EXIF:FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("50 mm"));

        let value = TagValue::Float(35.5);
        let formatted = format_tag_value("FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("35.5 mm"));
    }

    #[test]
    fn test_focal_length_from_rational() {
        let value = TagValue::Rational {
            numerator: 500,
            denominator: 10,
        };
        let formatted = format_tag_value("EXIF:FocalLength", &value);
        assert_eq!(formatted.as_string(), Some("50 mm"));
    }

    #[test]
    fn test_gps_altitude_unit_suffix() {
        let value = TagValue::String("117".to_string());
        let formatted = format_tag_value("GPS:GPSAltitude", &value);
        assert_eq!(formatted.as_string(), Some("117 m"));
    }

    #[test]
    fn test_subject_distance_unit_suffix() {
        let value = TagValue::String("2.5".to_string());
        let formatted = format_tag_value("EXIF:SubjectDistance", &value);
        assert_eq!(formatted.as_string(), Some("2.5 m"));
    }

    // -------------------------------------------------------------------------
    // Default behavior tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_unknown_tag_passes_through() {
        let value = TagValue::String("Canon".to_string());
        let formatted = format_tag_value("EXIF:Make", &value);
        assert_eq!(formatted.as_string(), Some("Canon"));

        let value = TagValue::Integer(400);
        let formatted = format_tag_value("ISO", &value);
        assert_eq!(formatted.as_integer(), Some(400));
    }

    // -------------------------------------------------------------------------
    // format_for_exiftool integration tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_for_exiftool_basic() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::String("Canon".to_string()));
        metadata.insert("EXIF:ExposureProgram", TagValue::Integer(2));
        metadata.insert("GPS:GPSLatitudeRef", TagValue::String("N".to_string()));
        metadata.insert("EXIF:FocalLength", TagValue::String("50".to_string()));

        let formatted = format_for_exiftool(&metadata);

        // Make should pass through unchanged
        assert_eq!(formatted.get_string("EXIF:Make"), Some("Canon"));
        // ExposureProgram should be formatted
        assert_eq!(
            formatted.get_string("EXIF:ExposureProgram"),
            Some("Program AE")
        );
        // GPSLatitudeRef should be formatted
        assert_eq!(formatted.get_string("GPS:GPSLatitudeRef"), Some("North"));
        // FocalLength should have unit suffix
        assert_eq!(formatted.get_string("EXIF:FocalLength"), Some("50 mm"));
    }

    #[test]
    fn test_format_for_exiftool_preserves_count() {
        let mut metadata = MetadataMap::new();
        metadata.insert("Tag1", TagValue::String("Value1".to_string()));
        metadata.insert("Tag2", TagValue::Integer(42));
        metadata.insert("Tag3", TagValue::Float(3.14));

        let formatted = format_for_exiftool(&metadata);

        assert_eq!(formatted.len(), 3);
    }

    #[test]
    fn test_format_for_exiftool_empty() {
        let metadata = MetadataMap::new();
        let formatted = format_for_exiftool(&metadata);
        assert!(formatted.is_empty());
    }

    // -------------------------------------------------------------------------
    // Percentage Tag tests (Quality, MeasurementFlare)
    // -------------------------------------------------------------------------

    #[test]
    fn test_quality_percentage_from_integer() {
        // Ducky:Quality with integer value should have "%" suffix
        let value = TagValue::Integer(84);
        let formatted = format_tag_value("Ducky:Quality", &value);
        assert_eq!(formatted.as_string(), Some("84%"));

        // Without family prefix
        let value = TagValue::Integer(100);
        let formatted = format_tag_value("Quality", &value);
        assert_eq!(formatted.as_string(), Some("100%"));

        // Zero value
        let value = TagValue::Integer(0);
        let formatted = format_tag_value("Quality", &value);
        assert_eq!(formatted.as_string(), Some("0%"));
    }

    #[test]
    fn test_quality_percentage_from_float() {
        // Quality with float value should have "%" suffix
        let value = TagValue::Float(84.0);
        let formatted = format_tag_value("Ducky:Quality", &value);
        assert_eq!(formatted.as_string(), Some("84%"));

        // Fractional float value
        let value = TagValue::Float(75.5);
        let formatted = format_tag_value("Quality", &value);
        assert_eq!(formatted.as_string(), Some("75.5%"));
    }

    #[test]
    fn test_measurement_flare_percentage_from_integer() {
        // MeasurementFlare with integer value should have "%" suffix
        let value = TagValue::Integer(1);
        let formatted = format_tag_value("ICC_Profile:MeasurementFlare", &value);
        assert_eq!(formatted.as_string(), Some("1%"));
    }

    #[test]
    fn test_is_percentage_tag() {
        assert!(is_percentage_tag("Quality"));
        // MeasurementFlare is included for integer values (floats handled by ICC matrix rule)
        assert!(is_percentage_tag("MeasurementFlare"));
        assert!(!is_percentage_tag("FocalLength"));
        assert!(!is_percentage_tag("ISO"));
        assert!(!is_percentage_tag("Make"));
    }

    // -------------------------------------------------------------------------
    // Helper function classification tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_is_gps_lat_lon_ref() {
        assert!(is_gps_lat_ref("GPSLatitudeRef"));
        assert!(is_gps_lon_ref("GPSLongitudeRef"));
        assert!(!is_gps_lat_ref("GPSLongitudeRef"));
        assert!(!is_gps_lon_ref("GPSLatitudeRef"));
    }

    #[test]
    fn test_is_gps_direction_ref() {
        assert!(is_gps_direction_ref("GPSImgDirectionRef"));
        assert!(is_gps_direction_ref("GPSDestBearingRef"));
        assert!(is_gps_direction_ref("GPSTrackRef"));
        assert!(!is_gps_direction_ref("GPSLatitudeRef"));
        assert!(!is_gps_direction_ref("GPSSpeedRef"));
    }

    #[test]
    fn test_is_gps_status_tag() {
        assert!(is_gps_status_tag("GPSStatus"));
        assert!(is_gps_status_tag("GPSMeasureMode"));
        assert!(is_gps_status_tag("GPSDifferential"));
        assert!(!is_gps_status_tag("GPSAltitude"));
    }

    #[test]
    fn test_is_version_tag() {
        assert!(is_version_tag("InteropVersion"));
        assert!(is_version_tag("ExifVersion"));
        assert!(is_version_tag("FlashpixVersion"));
        // GPSVersionID uses a distinct dot-separated decimal format; see
        // `is_gps_version_id` / `format_gps_version_id`.
        assert!(!is_version_tag("GPSVersionID"));
        assert!(!is_version_tag("SomeOtherVersion"));
    }

    #[test]
    fn test_is_unit_suffix_tag() {
        assert!(is_unit_suffix_tag("FocalLength"));
        assert!(is_unit_suffix_tag("FocalLengthIn35mmFormat"));
        assert!(is_unit_suffix_tag("GPSAltitude"));
        assert!(is_unit_suffix_tag("SubjectDistance"));
        assert!(!is_unit_suffix_tag("ISO"));
        assert!(!is_unit_suffix_tag("Make"));
    }

    // -------------------------------------------------------------------------
    // Special Float Value tests (infinity, negative zero)
    // -------------------------------------------------------------------------

    #[test]
    fn test_format_special_float_values_infinity() {
        // Positive infinity should return "undef"
        assert_eq!(
            format_special_float_values(f64::INFINITY),
            Some("undef".to_string())
        );

        // Negative infinity should also return "undef"
        assert_eq!(
            format_special_float_values(f64::NEG_INFINITY),
            Some("undef".to_string())
        );
    }

    #[test]
    fn test_format_special_float_values_negative_zero() {
        // Negative zero should return "0"
        assert_eq!(format_special_float_values(-0.0), Some("0".to_string()));
    }

    #[test]
    fn test_format_special_float_values_normal() {
        // Normal values should return None
        assert_eq!(format_special_float_values(0.0), None);
        assert_eq!(format_special_float_values(42.5), None);
        assert_eq!(format_special_float_values(-123.456), None);
        assert_eq!(format_special_float_values(f64::MIN), None);
        assert_eq!(format_special_float_values(f64::MAX), None);
    }

    #[test]
    fn test_infinity_float_formats_to_undef() {
        // Test that TagValue::Float with infinity formats to "undef"
        let value = TagValue::Float(f64::INFINITY);
        let formatted = format_tag_value("EXIF:GPSDestBearing", &value);
        assert_eq!(formatted.as_string(), Some("undef"));

        let value = TagValue::Float(f64::NEG_INFINITY);
        let formatted = format_tag_value("EXIF:GPSDestDistance", &value);
        assert_eq!(formatted.as_string(), Some("undef"));
    }

    #[test]
    fn test_negative_zero_float_formats_to_zero() {
        // Test that TagValue::Float with -0.0 formats to "0"
        let value = TagValue::Float(-0.0);
        let formatted = format_tag_value("EXIF:ExposureIndex", &value);
        assert_eq!(formatted.as_string(), Some("0"));
    }

    // -------------------------------------------------------------------------
    // ICC_Profile Matrix Tag tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_icc_profile_blue_matrix_column_precision() {
        // Test the exact case from the issue: too many decimal places
        // OxiDex was showing: 0.1491851806640625 0.0632171630859375 0.74456787109375
        // ExifTool shows:     0.14919 0.06322 0.74457
        let value =
            TagValue::String("0.1491851806640625 0.0632171630859375 0.74456787109375".to_string());
        let formatted = format_tag_value("ICC_Profile:BlueMatrixColumn", &value);
        assert_eq!(formatted.as_string(), Some("0.14919 0.06322 0.74457"));
    }

    #[test]
    fn test_icc_profile_red_matrix_column() {
        let value = TagValue::String("0.43604 0.22249 0.01392".to_string());
        let formatted = format_tag_value("ICC_Profile:RedMatrixColumn", &value);
        assert_eq!(formatted.as_string(), Some("0.43604 0.22249 0.01392"));
    }

    #[test]
    fn test_icc_profile_green_matrix_column() {
        // Values with trailing zeros should be trimmed
        let value = TagValue::String("0.38512 0.71690 0.09706".to_string());
        let formatted = format_tag_value("GreenMatrixColumn", &value);
        assert_eq!(formatted.as_string(), Some("0.38512 0.7169 0.09706"));
    }

    #[test]
    fn test_icc_profile_media_white_point() {
        let value = TagValue::String("0.95047 1 1.08883".to_string());
        let formatted = format_tag_value("ICC_Profile:MediaWhitePoint", &value);
        assert_eq!(formatted.as_string(), Some("0.95047 1 1.08883"));
    }

    #[test]
    fn test_icc_profile_luminance() {
        let value = TagValue::String("76.03647".to_string());
        let formatted = format_tag_value("ICC_Profile:Luminance", &value);
        assert_eq!(formatted.as_string(), Some("76.03647"));
    }

    #[test]
    fn test_icc_profile_connection_space_illuminant() {
        // Whole number 1.0 should be trimmed to "1"
        let value = TagValue::String("0.9642 1.0 0.82491".to_string());
        let formatted = format_tag_value("ConnectionSpaceIlluminant", &value);
        assert_eq!(formatted.as_string(), Some("0.9642 1 0.82491"));
    }

    #[test]
    fn test_icc_profile_viewing_cond_illuminant() {
        let value = TagValue::String("19.6445 20.3718 16.8089".to_string());
        let formatted = format_tag_value("ViewingCondIlluminant", &value);
        assert_eq!(formatted.as_string(), Some("19.6445 20.3718 16.8089"));
    }

    #[test]
    fn test_format_icc_string_values_helper() {
        // Test the helper function directly
        let result = format_icc_string_values(
            "0.1491851806640625 0.0632171630859375 0.74456787109375",
            "BlueMatrixColumn",
        );
        assert_eq!(result, "0.14919 0.06322 0.74457");

        // Test with non-numeric content (should preserve)
        let result = format_icc_string_values("abc 1.5 def", "SomeTag");
        assert_eq!(result, "abc 1.5 def");
    }

    #[test]
    fn test_icc_matrix_tag_recognition() {
        // Test that is_icc_matrix_tag correctly identifies ICC profile tags
        assert!(is_icc_matrix_tag("BlueMatrixColumn"));
        assert!(is_icc_matrix_tag("RedMatrixColumn"));
        assert!(is_icc_matrix_tag("GreenMatrixColumn"));
        assert!(is_icc_matrix_tag("MediaWhitePoint"));
        assert!(is_icc_matrix_tag("MeasurementFlare"));
        assert!(is_icc_matrix_tag("ICC_Profile:Luminance"));
        assert!(!is_icc_matrix_tag("FocalLength"));
        assert!(!is_icc_matrix_tag("GPSAltitude"));
    }

    // -------------------------------------------------------------------------
    // ReferenceBlackWhite Integer Precision tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_reference_black_white_integer_formatting() {
        // ReferenceBlackWhite should display integers without decimals
        // ExifTool: "0 255 128 255 128 255"
        // OxiDex before fix: "0.0000000000 255.0000000000 128.0000000000..."
        let value = TagValue::String(
            "0.0000000000 255.0000000000 128.0000000000 255.0000000000 128.0000000000 255.0000000000"
                .to_string(),
        );
        let formatted = format_tag_value("EXIF:ReferenceBlackWhite", &value);
        assert_eq!(formatted.as_string(), Some("0 255 128 255 128 255"));

        // Without family prefix
        let value = TagValue::String("0.0 128.0 255.0".to_string());
        let formatted = format_tag_value("ReferenceBlackWhite", &value);
        assert_eq!(formatted.as_string(), Some("0 128 255"));
    }

    #[test]
    fn test_reference_black_white_with_fractional_values() {
        // If any values are non-integer, preserve minimal decimals
        let value = TagValue::String("0.5 255.0 128.25".to_string());
        let formatted = format_tag_value("EXIF:ReferenceBlackWhite", &value);
        assert_eq!(formatted.as_string(), Some("0.5 255 128.25"));
    }

    // -------------------------------------------------------------------------
    // YCbCrCoefficients Three Decimal Precision tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_ycbcr_coefficients_three_decimal_formatting() {
        // YCbCrCoefficients should display with 3 decimal places
        // ExifTool: "0.299 0.587 0.114"
        // OxiDex before fix: "0.2990000000 0.5870000000 0.1140000000"
        let value = TagValue::String("0.2990000000 0.5870000000 0.1140000000".to_string());
        let formatted = format_tag_value("EXIF:YCbCrCoefficients", &value);
        assert_eq!(formatted.as_string(), Some("0.299 0.587 0.114"));

        // Without family prefix
        let formatted = format_tag_value("YCbCrCoefficients", &value);
        assert_eq!(formatted.as_string(), Some("0.299 0.587 0.114"));
    }

    #[test]
    fn test_ycbcr_coefficients_trimmed_zeros() {
        // Values with fewer decimals should still trim trailing zeros
        let value = TagValue::String("0.5 1.0 0.25".to_string());
        let formatted = format_tag_value("EXIF:YCbCrCoefficients", &value);
        assert_eq!(formatted.as_string(), Some("0.5 1 0.25"));
    }

    // -------------------------------------------------------------------------
    // UserComment Decoding tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_user_comment_ascii_decoding() {
        // UserComment with ASCII encoding should be decoded to text
        // ExifTool shows: "GCM_TAG"
        // OxiDex before fix showed: "[Binary data]"
        let data = b"ASCII\0\0\0GCM_TAG".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:UserComment", &value);
        assert_eq!(formatted.as_string(), Some("GCM_TAG"));
    }

    #[test]
    fn test_user_comment_unicode_decoding() {
        // UserComment with UNICODE encoding
        let mut data = b"UNICODE\0".to_vec();
        // "Hi" in UTF-16LE: H=0x48, i=0x69
        data.extend_from_slice(&[0x48, 0x00, 0x69, 0x00, 0x00, 0x00]);
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("UserComment", &value);
        assert_eq!(formatted.as_string(), Some("Hi"));
    }

    #[test]
    fn test_user_comment_empty_stays_binary() {
        // Empty UserComment should not produce a string result
        let data = b"ASCII\0\0\0".to_vec();
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:UserComment", &value);
        // Empty decoded content falls through to default (binary returned as-is)
        assert!(matches!(formatted, TagValue::Binary(_)));
    }

    #[test]
    fn test_is_user_comment() {
        assert!(is_user_comment("UserComment"));
        assert!(!is_user_comment("Comment"));
        assert!(!is_user_comment("ImageDescription"));
    }

    // -------------------------------------------------------------------------
    // ThumbnailImage Formatting tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_thumbnail_image_binary_formatting() {
        // ThumbnailImage should be formatted with ExifTool-compatible message
        // ExifTool shows: "(Binary data 5448 bytes, use -b option to extract)"
        let data = vec![0xFF, 0xD8, 0xFF, 0xE0]; // Start of JPEG
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("EXIF:ThumbnailImage", &value);
        assert_eq!(
            formatted.as_string(),
            Some("(Binary data 4 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn test_thumbnail_image_large_binary() {
        // Test with a larger thumbnail
        let data = vec![0u8; 5448]; // 5448 bytes like in the example
        let value = TagValue::Binary(data);
        let formatted = format_tag_value("ThumbnailImage", &value);
        assert_eq!(
            formatted.as_string(),
            Some("(Binary data 5448 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn test_is_thumbnail_image() {
        assert!(is_thumbnail_image("ThumbnailImage"));
        assert!(!is_thumbnail_image("PreviewImage"));
        assert!(!is_thumbnail_image("JpgFromRaw"));
    }
}
