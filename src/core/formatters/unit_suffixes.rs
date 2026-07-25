//! Unit suffix formatting for EXIF metadata values.
//!
//! This module provides formatting functions that add appropriate unit suffixes
//! to metadata values to match ExifTool's output format. For example:
//! - FocalLength: "31" -> "31 mm"
//! - SubjectDistance: "2.5" -> "2.5 m"
//! - GPSAltitude: "117" -> "117 m"
//!
//! The formatter is designed to be idempotent - if a value already has the
//! correct unit suffix, it will not be duplicated.

// ============================================================================
// TAG CATEGORY DEFINITIONS
// ============================================================================

/// Tags that require "mm" (millimeter) suffix for focal length measurements.
///
/// These tags represent optical focal lengths and should be displayed with
/// "mm" suffix to match ExifTool's standard output format.
const MM_SUFFIX_TAGS: &[&str] = &[
    "FocalLength",
    "FocalLengthIn35mmFormat",
    "FocalLength35efl",
    "FocalLengthIn35mmFilm",
];

/// Tags that require "m" (meter) suffix for distance/altitude measurements.
///
/// These tags represent physical distances or altitudes in meters and should
/// be displayed with "m" suffix to match ExifTool's output format.
const METER_SUFFIX_TAGS: &[&str] = &["SubjectDistance", "GPSAltitude", "HyperfocalDistance"];

/// Tags that may require "s" (seconds) suffix for time measurements.
///
/// Note: ExifTool only adds "s" suffix for exposure times >= 1 second.
/// Fractional exposure times (e.g., "1/125") are displayed without suffix.
const SECONDS_SUFFIX_TAGS: &[&str] = &["ExposureTime", "ShutterSpeedValue"];

// ============================================================================
// MAIN FORMATTING FUNCTION
// ============================================================================

/// Format a metadata value with the appropriate unit suffix based on tag name.
///
/// This function examines the tag name and appends the correct unit suffix
/// (mm, m, or s) to match ExifTool's output format. It handles:
///
/// - **FocalLength tags**: Appends " mm" suffix
/// - **Distance/altitude tags**: Appends " m" suffix
/// - **ExposureTime**: Appends " s" only for values >= 1 second
///
/// The function is idempotent - if the value already has the correct suffix,
/// it will not be duplicated. It also handles fully-qualified tag names
/// (e.g., "EXIF:FocalLength") by extracting the base tag name.
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally prefixed with group (e.g., "EXIF:FocalLength")
/// * `value` - The formatted value string to append suffix to
///
/// # Returns
///
/// The value with appropriate unit suffix, or unchanged if:
/// - No suffix is needed for this tag
/// - The value already has the correct suffix
/// - The value format doesn't warrant a suffix (e.g., fractional exposure times)
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::unit_suffixes::format_with_unit;
///
/// // Focal length tags get "mm" suffix
/// assert_eq!(format_with_unit("FocalLength", "6.0"), "6.0 mm");
/// assert_eq!(format_with_unit("FocalLengthIn35mmFormat", "31"), "31 mm");
/// assert_eq!(format_with_unit("EXIF:FocalLength", "50"), "50 mm");
///
/// // Distance tags get "m" suffix
/// assert_eq!(format_with_unit("SubjectDistance", "2.5"), "2.5 m");
/// assert_eq!(format_with_unit("GPSAltitude", "117"), "117 m");
///
/// // Exposure time only gets "s" suffix for values >= 1 second
/// assert_eq!(format_with_unit("ExposureTime", "2"), "2 s");
/// assert_eq!(format_with_unit("ExposureTime", "1/125"), "1/125"); // No suffix for fractions
///
/// // Other tags remain unchanged
/// assert_eq!(format_with_unit("ISO", "400"), "400");
/// ```
pub fn format_with_unit(tag_name: &str, value: &str) -> String {
    // Extract the base tag name by taking the part after the last colon.
    // This handles fully-qualified names like "EXIF:FocalLength" or "Composite:FocalLengthIn35mmFormat"
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);

    // Handle millimeter suffix for focal length tags
    if MM_SUFFIX_TAGS.contains(&base_name) {
        return format_with_mm_suffix(value);
    }

    // Handle meter suffix for distance/altitude tags
    if METER_SUFFIX_TAGS.contains(&base_name) {
        return format_with_meter_suffix(value);
    }

    // Handle seconds suffix for exposure time tags (only for >= 1 second)
    if SECONDS_SUFFIX_TAGS.contains(&base_name) {
        return format_exposure_time_with_suffix(value);
    }

    // No suffix needed for this tag - return value unchanged
    value.to_string()
}

// ============================================================================
// SUFFIX-SPECIFIC FORMATTING FUNCTIONS
// ============================================================================

/// Format a value with " mm" suffix if not already present.
///
/// This function ensures idempotency by checking whether the value
/// already ends with " mm" before appending the suffix.
///
/// # Arguments
///
/// * `value` - The numeric value string (e.g., "31", "6.0")
///
/// # Returns
///
/// The value with " mm" suffix appended, or unchanged if already present.
fn format_with_mm_suffix(value: &str) -> String {
    // Avoid duplicating suffix if already present
    if value.ends_with(" mm") {
        return value.to_string();
    }
    format!("{} mm", value)
}

/// Format a value with " m" suffix if not already present.
///
/// This function ensures idempotency by checking whether the value
/// already ends with " m" (but not " mm") before appending the suffix.
///
/// # Arguments
///
/// * `value` - The numeric value string (e.g., "2.5", "117")
///
/// # Returns
///
/// The value with " m" suffix appended, or unchanged if already present.
fn format_with_meter_suffix(value: &str) -> String {
    // Avoid duplicating suffix if already present.
    // Note: We need to be careful not to match " mm" when checking for " m"
    if value.ends_with(" m") && !value.ends_with(" mm") {
        return value.to_string();
    }
    format!("{} m", value)
}

/// Format exposure time with " s" suffix only for values >= 1 second.
///
/// ExifTool's convention is to display:
/// - Fractional exposure times (< 1 second) as fractions without suffix: "1/125"
/// - Whole or decimal exposure times (>= 1 second) with "s" suffix: "2 s", "1.5 s"
///
/// This function parses the value to determine whether it represents a time
/// >= 1 second and adds the suffix accordingly.
///
/// # Arguments
///
/// * `value` - The exposure time value string (e.g., "1/125", "2", "1.5")
///
/// # Returns
///
/// The value with " s" suffix if >= 1 second, or unchanged otherwise.
fn format_exposure_time_with_suffix(value: &str) -> String {
    // Avoid duplicating suffix if already present
    if value.ends_with(" s") || value.ends_with(" sec") || value.ends_with("s") {
        return value.to_string();
    }

    // Fractional values (containing "/") represent times < 1 second
    // and should not get a suffix per ExifTool convention
    if value.contains('/') {
        return value.to_string();
    }

    // Try to parse as a numeric value to determine if >= 1 second
    // Handle both integer ("2") and decimal ("1.5") formats
    if let Ok(numeric_value) = value.parse::<f64>()
        && numeric_value >= 1.0
    {
        return format!("{} s", value);
    }

    // Unable to parse or value < 1 second - return unchanged
    value.to_string()
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Check if a tag should have a unit suffix applied.
///
/// This is useful for determining whether additional formatting is needed
/// for a particular tag's value before calling [`format_with_unit`].
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally prefixed with group (e.g., "EXIF:FocalLength")
///
/// # Returns
///
/// `true` if the tag should have a unit suffix (mm, m, or s), `false` otherwise.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::unit_suffixes::needs_unit_suffix;
///
/// assert!(needs_unit_suffix("FocalLength"));
/// assert!(needs_unit_suffix("FocalLengthIn35mmFormat"));
/// assert!(needs_unit_suffix("SubjectDistance"));
/// assert!(needs_unit_suffix("GPSAltitude"));
/// assert!(needs_unit_suffix("ExposureTime"));
///
/// // With group prefix
/// assert!(needs_unit_suffix("EXIF:FocalLength"));
///
/// // Tags that don't need suffix
/// assert!(!needs_unit_suffix("ISO"));
/// assert!(!needs_unit_suffix("Model"));
/// ```
pub fn needs_unit_suffix(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    MM_SUFFIX_TAGS.contains(&base_name)
        || METER_SUFFIX_TAGS.contains(&base_name)
        || SECONDS_SUFFIX_TAGS.contains(&base_name)
}

/// Get the unit suffix string for a given tag, if applicable.
///
/// This function returns the raw unit suffix string (without leading space)
/// for a given tag name, or `None` if no suffix applies.
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally prefixed with group
///
/// # Returns
///
/// The unit suffix ("mm", "m", or "s") or `None` if no suffix applies.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::unit_suffixes::get_unit_suffix;
///
/// assert_eq!(get_unit_suffix("FocalLength"), Some("mm"));
/// assert_eq!(get_unit_suffix("SubjectDistance"), Some("m"));
/// assert_eq!(get_unit_suffix("ExposureTime"), Some("s"));
/// assert_eq!(get_unit_suffix("ISO"), None);
/// ```
pub fn get_unit_suffix(tag_name: &str) -> Option<&'static str> {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);

    if MM_SUFFIX_TAGS.contains(&base_name) {
        Some("mm")
    } else if METER_SUFFIX_TAGS.contains(&base_name) {
        Some("m")
    } else if SECONDS_SUFFIX_TAGS.contains(&base_name) {
        Some("s")
    } else {
        None
    }
}

// ============================================================================
// UNIT TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // Tests for FocalLength tags (mm suffix)
    // ------------------------------------------------------------------------

    #[test]
    fn test_focal_length_basic() {
        // Basic focal length values should get "mm" suffix
        assert_eq!(format_with_unit("FocalLength", "50"), "50 mm");
        assert_eq!(format_with_unit("FocalLength", "6.0"), "6.0 mm");
        assert_eq!(format_with_unit("FocalLength", "24.5"), "24.5 mm");
    }

    #[test]
    fn test_focal_length_in_35mm_format() {
        // FocalLengthIn35mmFormat is the specific tag mentioned in the task
        assert_eq!(format_with_unit("FocalLengthIn35mmFormat", "31"), "31 mm");
        assert_eq!(format_with_unit("FocalLengthIn35mmFormat", "75"), "75 mm");
        assert_eq!(format_with_unit("FocalLengthIn35mmFormat", "100"), "100 mm");
    }

    #[test]
    fn test_focal_length_35efl_variant() {
        // FocalLength35efl is an alternate name used in some contexts
        assert_eq!(format_with_unit("FocalLength35efl", "24"), "24 mm");
        assert_eq!(format_with_unit("FocalLength35efl", "35"), "35 mm");
    }

    #[test]
    fn test_focal_length_in_35mm_film() {
        // FocalLengthIn35mmFilm is another variant
        assert_eq!(format_with_unit("FocalLengthIn35mmFilm", "50"), "50 mm");
        assert_eq!(format_with_unit("FocalLengthIn35mmFilm", "200"), "200 mm");
    }

    #[test]
    fn test_focal_length_with_group_prefix() {
        // Fully-qualified tag names with group prefix should work
        assert_eq!(format_with_unit("EXIF:FocalLength", "50"), "50 mm");
        assert_eq!(
            format_with_unit("Composite:FocalLengthIn35mmFormat", "35"),
            "35 mm"
        );
        assert_eq!(format_with_unit("MakerNotes:FocalLength", "85"), "85 mm");
    }

    #[test]
    fn test_focal_length_already_has_suffix() {
        // Should not duplicate suffix if already present
        assert_eq!(format_with_unit("FocalLength", "50 mm"), "50 mm");
        assert_eq!(
            format_with_unit("FocalLengthIn35mmFormat", "31 mm"),
            "31 mm"
        );
    }

    // ------------------------------------------------------------------------
    // Tests for SubjectDistance tag (m suffix)
    // ------------------------------------------------------------------------

    #[test]
    fn test_subject_distance_basic() {
        assert_eq!(format_with_unit("SubjectDistance", "2.5"), "2.5 m");
        assert_eq!(format_with_unit("SubjectDistance", "10"), "10 m");
        assert_eq!(format_with_unit("SubjectDistance", "0.5"), "0.5 m");
    }

    #[test]
    fn test_subject_distance_with_group_prefix() {
        assert_eq!(format_with_unit("EXIF:SubjectDistance", "3.0"), "3.0 m");
    }

    #[test]
    fn test_subject_distance_already_has_suffix() {
        assert_eq!(format_with_unit("SubjectDistance", "2.5 m"), "2.5 m");
    }

    // ------------------------------------------------------------------------
    // Tests for GPSAltitude tag (m suffix)
    // ------------------------------------------------------------------------

    #[test]
    fn test_gps_altitude_basic() {
        assert_eq!(format_with_unit("GPSAltitude", "117"), "117 m");
        assert_eq!(format_with_unit("GPSAltitude", "0"), "0 m");
        assert_eq!(format_with_unit("GPSAltitude", "1500.5"), "1500.5 m");
    }

    #[test]
    fn test_gps_altitude_with_group_prefix() {
        assert_eq!(format_with_unit("GPS:GPSAltitude", "100"), "100 m");
        assert_eq!(format_with_unit("EXIF:GPSAltitude", "250"), "250 m");
    }

    #[test]
    fn test_gps_altitude_already_has_suffix() {
        assert_eq!(format_with_unit("GPSAltitude", "117 m"), "117 m");
    }

    // ------------------------------------------------------------------------
    // Tests for ExposureTime tag (conditional s suffix)
    // ------------------------------------------------------------------------

    #[test]
    fn test_exposure_time_one_second_or_more() {
        // Values >= 1 second should get "s" suffix
        assert_eq!(format_with_unit("ExposureTime", "1"), "1 s");
        assert_eq!(format_with_unit("ExposureTime", "2"), "2 s");
        assert_eq!(format_with_unit("ExposureTime", "1.5"), "1.5 s");
        assert_eq!(format_with_unit("ExposureTime", "30"), "30 s");
    }

    #[test]
    fn test_exposure_time_fractions() {
        // Fractional exposure times (< 1 second) should NOT get suffix
        assert_eq!(format_with_unit("ExposureTime", "1/125"), "1/125");
        assert_eq!(format_with_unit("ExposureTime", "1/1000"), "1/1000");
        assert_eq!(format_with_unit("ExposureTime", "1/60"), "1/60");
        assert_eq!(format_with_unit("ExposureTime", "1/4"), "1/4");
    }

    #[test]
    fn test_exposure_time_decimal_less_than_one() {
        // Decimal values < 1 second should NOT get suffix
        assert_eq!(format_with_unit("ExposureTime", "0.5"), "0.5");
        assert_eq!(format_with_unit("ExposureTime", "0.25"), "0.25");
        assert_eq!(format_with_unit("ExposureTime", "0.001"), "0.001");
    }

    #[test]
    fn test_exposure_time_with_group_prefix() {
        assert_eq!(format_with_unit("EXIF:ExposureTime", "2"), "2 s");
        assert_eq!(format_with_unit("EXIF:ExposureTime", "1/250"), "1/250");
    }

    #[test]
    fn test_exposure_time_already_has_suffix() {
        // Should not duplicate suffix if already present
        assert_eq!(format_with_unit("ExposureTime", "2 s"), "2 s");
        assert_eq!(format_with_unit("ExposureTime", "1 sec"), "1 sec");
        assert_eq!(format_with_unit("ExposureTime", "2s"), "2s");
    }

    // ------------------------------------------------------------------------
    // Tests for other tags (no suffix)
    // ------------------------------------------------------------------------

    #[test]
    fn test_no_suffix_for_other_tags() {
        // Tags not in our lists should remain unchanged
        assert_eq!(format_with_unit("ISO", "400"), "400");
        assert_eq!(format_with_unit("ImageWidth", "1920"), "1920");
        assert_eq!(format_with_unit("Model", "Canon EOS R5"), "Canon EOS R5");
        assert_eq!(format_with_unit("Make", "Nikon"), "Nikon");
        assert_eq!(format_with_unit("Orientation", "1"), "1");
        assert_eq!(format_with_unit("FNumber", "2.8"), "2.8");
        assert_eq!(format_with_unit("ApertureValue", "3.5"), "3.5");
    }

    #[test]
    fn test_no_suffix_with_group_prefix() {
        assert_eq!(format_with_unit("EXIF:ISO", "800"), "800");
        assert_eq!(format_with_unit("EXIF:Model", "Canon"), "Canon");
    }

    // ------------------------------------------------------------------------
    // Tests for needs_unit_suffix function
    // ------------------------------------------------------------------------

    #[test]
    fn test_needs_unit_suffix_mm_tags() {
        assert!(needs_unit_suffix("FocalLength"));
        assert!(needs_unit_suffix("FocalLengthIn35mmFormat"));
        assert!(needs_unit_suffix("FocalLength35efl"));
        assert!(needs_unit_suffix("FocalLengthIn35mmFilm"));
        assert!(needs_unit_suffix("EXIF:FocalLength"));
    }

    #[test]
    fn test_needs_unit_suffix_meter_tags() {
        assert!(needs_unit_suffix("SubjectDistance"));
        assert!(needs_unit_suffix("GPSAltitude"));
        assert!(needs_unit_suffix("HyperfocalDistance"));
        assert!(needs_unit_suffix("GPS:GPSAltitude"));
    }

    #[test]
    fn test_needs_unit_suffix_seconds_tags() {
        assert!(needs_unit_suffix("ExposureTime"));
        assert!(needs_unit_suffix("ShutterSpeedValue"));
        assert!(needs_unit_suffix("EXIF:ExposureTime"));
    }

    #[test]
    fn test_needs_unit_suffix_other_tags() {
        assert!(!needs_unit_suffix("ISO"));
        assert!(!needs_unit_suffix("Model"));
        assert!(!needs_unit_suffix("FNumber"));
        assert!(!needs_unit_suffix("ImageWidth"));
        assert!(!needs_unit_suffix(""));
    }

    // ------------------------------------------------------------------------
    // Tests for get_unit_suffix function
    // ------------------------------------------------------------------------

    #[test]
    fn test_get_unit_suffix_mm() {
        assert_eq!(get_unit_suffix("FocalLength"), Some("mm"));
        assert_eq!(get_unit_suffix("FocalLengthIn35mmFormat"), Some("mm"));
        assert_eq!(get_unit_suffix("EXIF:FocalLength"), Some("mm"));
    }

    #[test]
    fn test_get_unit_suffix_meter() {
        assert_eq!(get_unit_suffix("SubjectDistance"), Some("m"));
        assert_eq!(get_unit_suffix("GPSAltitude"), Some("m"));
        assert_eq!(get_unit_suffix("GPS:GPSAltitude"), Some("m"));
    }

    #[test]
    fn test_get_unit_suffix_seconds() {
        assert_eq!(get_unit_suffix("ExposureTime"), Some("s"));
        assert_eq!(get_unit_suffix("ShutterSpeedValue"), Some("s"));
    }

    #[test]
    fn test_get_unit_suffix_none() {
        assert_eq!(get_unit_suffix("ISO"), None);
        assert_eq!(get_unit_suffix("Model"), None);
        assert_eq!(get_unit_suffix("FNumber"), None);
    }

    // ------------------------------------------------------------------------
    // Edge case tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_empty_value() {
        assert_eq!(format_with_unit("FocalLength", ""), " mm");
        assert_eq!(format_with_unit("ISO", ""), "");
    }

    #[test]
    fn test_whitespace_in_value() {
        assert_eq!(format_with_unit("FocalLength", "50 "), "50  mm");
        assert_eq!(format_with_unit("SubjectDistance", " 2.5"), " 2.5 m");
    }

    #[test]
    fn test_special_characters_in_value() {
        // Values with special characters should still work
        assert_eq!(format_with_unit("FocalLength", "50-200"), "50-200 mm");
        assert_eq!(format_with_unit("GPSAltitude", "-100"), "-100 m");
    }

    #[test]
    fn test_meter_suffix_not_confused_with_mm() {
        // Ensure "m" suffix detection doesn't match "mm"
        // This is an edge case where value already has " mm" but we're checking for " m"
        let value = "50 mm";
        // If we mistakenly apply meter suffix to a value already having "mm", it should not match
        // This is handled by the fact that we check the specific tag name first
        assert_eq!(format_with_unit("FocalLength", value), "50 mm");
    }
}
