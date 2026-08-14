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

/// Groups whose `FocalLength` is EXIF tag 0x920a, whose PrintConv is
/// `sprintf("%.1f mm",$val)` (Exif.pm:2401) -- one forced decimal, always.
///
/// Deliberately an explicit allow-list of family-0/family-1 group names rather
/// than a check on the bare tag name: the maker-note tables named
/// `FocalLength` print a bare `"$val mm"` with no decimal forced (Canon.pm:3138
/// and :3176, `[Canon] FocalLength : 34 mm`), so rounding every `FocalLength`
/// would trade one wrong rendering for another. `ExifIFD` is the group oxidex
/// emits, `EXIF` the family-0 name the comparison harness normalizes it to,
/// and the remaining IFD names cover a 0x920a written outside the ExifIFD.
const EXIF_FOCAL_LENGTH_GROUPS: &[&str] = &[
    "EXIF",
    "ExifIFD",
    "IFD0",
    "IFD1",
    "IFD2",
    "IFD3",
    "SubIFD",
    "InteropIFD",
];

/// Tags that require "m" (meter) suffix for distance/altitude measurements.
///
/// These tags represent physical distances or altitudes in meters and should
/// be displayed with "m" suffix to match ExifTool's output format.
const METER_SUFFIX_TAGS: &[&str] = &["SubjectDistance", "GPSAltitude", "HyperfocalDistance"];

// There is deliberately no seconds-suffix table here.
//
// Both EXIF time tags render through `PrintExposureTime`, not through a unit
// suffix: `0x829a ExposureTime` (Exif.pm:1826) and `0x9201 ShutterSpeedValue`
// (Exif.pm:2324) each carry
// `PrintConv => 'Image::ExifTool::Exif::PrintExposureTime($val)'`, and that
// sub (Exif.pm:5606) ends
//
// ```text
//     $_ = sprintf("%.1f",$secs);
//     s/\.0$//;
//     return $_;
// ```
//
// with no unit anywhere in it. Calling the installed 13.55 Perl confirms it:
// `PrintExposureTime(2)` is `"2"`, `(1.5)` is `"1.5"`, `(30)` is `"30"` --
// never `"2 s"`. An earlier table here appended `" s"` to every value >= 1
// second and had unit tests asserting that invented output.

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
/// - The value already names its unit (`"50 mm"`, or a composite that renders
///   its own, such as `"4.2 mm (35 mm equivalent: 26.0 mm)"`)
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::unit_suffixes::format_with_unit;
///
/// // Focal length tags get "mm" suffix
/// assert_eq!(format_with_unit("FocalLength", "6.0"), "6.0 mm");
/// assert_eq!(format_with_unit("FocalLengthIn35mmFormat", "31"), "31 mm");
///
/// // ...but EXIF's own FocalLength (0x920a) forces one decimal place
/// assert_eq!(format_with_unit("EXIF:FocalLength", "50"), "50.0 mm");
///
/// // Distance tags get "m" suffix
/// assert_eq!(format_with_unit("SubjectDistance", "2.5"), "2.5 m");
/// assert_eq!(format_with_unit("GPSAltitude", "117"), "117 m");
///
/// // Time tags never gain a unit -- PrintExposureTime emits none
/// assert_eq!(format_with_unit("ExposureTime", "2"), "2");
/// assert_eq!(format_with_unit("ExposureTime", "1/125"), "1/125");
///
/// // A value that already names its unit is left alone
/// assert_eq!(
///     format_with_unit("Composite:FocalLength35efl", "4.2 mm (35 mm equivalent: 26.0 mm)"),
///     "4.2 mm (35 mm equivalent: 26.0 mm)"
/// );
///
/// // Other tags remain unchanged
/// assert_eq!(format_with_unit("ISO", "400"), "400");
/// ```
pub fn format_with_unit(tag_name: &str, value: &str) -> String {
    // Extract the base tag name by taking the part after the last colon.
    // This handles fully-qualified names like "EXIF:FocalLength" or "Composite:FocalLengthIn35mmFormat"
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);

    // `Composite:GPSAltitude` is not an EXIF altitude missing its unit: its own
    // PrintConv (GPS.pm:423-431) renders the unit and the sea-level reference
    // together -- "207 m Above Sea Level" -- so the meter suffix is already
    // there, just not at the end of the string. `format_with_meter_suffix`
    // tests `ends_with(" m")` and so appended a second one, giving
    // "207 m Above Sea Level m" against the pinned oracle's "207 m Above Sea
    // Level". This is the same shape as the `contains(" mm")` guard
    // `format_with_mm_suffix` already carries for `Composite:FocalLength35efl`
    // (see its doc comment); a `contains(" m")` test is not usable for meters,
    // because " m" is a prefix of " mm" and of every word starting with m.
    if base_name == "GPSAltitude" && tag_name.starts_with("Composite:") {
        return value.to_string();
    }

    // EXIF's own FocalLength (0x920a) forces exactly one decimal place --
    // Exif.pm:2401 `PrintConv => 'sprintf("%.1f mm",$val)'`. That is what
    // makes `exiftool -G1 -s` print `15.0 mm`, `70.0 mm` and `7.2 mm` where a
    // plain "append mm" renders `15 mm`, `70 mm` and `7.203125 mm`.
    if base_name == "FocalLength" && is_exif_focal_length_group(tag_name) {
        return format_focal_length_mm(value);
    }

    // Handle millimeter suffix for focal length tags
    if MM_SUFFIX_TAGS.contains(&base_name) {
        return format_with_mm_suffix(value);
    }

    // Handle meter suffix for distance/altitude tags
    if METER_SUFFIX_TAGS.contains(&base_name) {
        return format_with_meter_suffix(value);
    }

    // No suffix needed for this tag - return value unchanged
    value.to_string()
}

// ============================================================================
// SUFFIX-SPECIFIC FORMATTING FUNCTIONS
// ============================================================================

/// Format a value with " mm" suffix if it does not already carry one.
///
/// # Why this tests `contains` and not `ends_with`
///
/// `Composite:FocalLength35efl` arrives here already fully rendered. Its
/// PrintConv (Exif.pm:4720) is
///
/// ```text
/// $val[1] ? sprintf("%.1f mm (35 mm equivalent: %.1f mm)", $val[0], $val)
///         : sprintf("%.1f mm", $val)
/// ```
///
/// so the two-argument form ends in `")"`, not `" mm"`. An `ends_with(" mm")`
/// guard therefore missed it and appended a second unit, and
/// `exiftool -a -G1 -s` disagreed on 43 of the 148 files in the sample corpus:
///
/// ```text
/// exiftool: 4.2 mm (35 mm equivalent: 26.0 mm)
/// oxidex:   4.2 mm (35 mm equivalent: 26.0 mm) mm
/// ```
///
/// Any value that already names millimetres anywhere is already carrying its
/// unit; a bare number ("31", "50-200") never contains `" mm"` and still gets
/// one appended.
fn format_with_mm_suffix(value: &str) -> String {
    if value.contains(" mm") {
        return value.to_string();
    }
    format!("{} mm", value)
}

/// True when `tag_name`'s group is one whose `FocalLength` is EXIF 0x920a.
///
/// An unqualified `"FocalLength"` is NOT treated as EXIF's: the maker-note
/// parsers hand their already-rendered `"34 mm"` through this same function,
/// and there is nothing in a bare tag name to tell the two apart.
fn is_exif_focal_length_group(tag_name: &str) -> bool {
    match tag_name.rsplit_once(':') {
        Some((group, _)) => EXIF_FOCAL_LENGTH_GROUPS.contains(&group),
        None => false,
    }
}

/// Renders EXIF 0x920a's `sprintf("%.1f mm",$val)` (Exif.pm:2401).
///
/// Idempotent: a value already carrying the suffix is left alone, so a
/// maker-note-style `"34 mm"` that reaches here through some other path is not
/// re-parsed. An unparseable value falls back to plain suffixing rather than
/// being dropped or zeroed.
fn format_focal_length_mm(value: &str) -> String {
    if value.ends_with(" mm") {
        return value.to_string();
    }
    match value.trim().parse::<f64>() {
        Ok(v) => format!("{:.1} mm", v),
        Err(_) => format_with_mm_suffix(value),
    }
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
/// `true` if the tag should have a unit suffix (mm or m), `false` otherwise.
///
/// `ExposureTime` and `ShutterSpeedValue` are deliberately absent: they render
/// through `PrintExposureTime`, which emits no unit at all.
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
///
/// // With group prefix
/// assert!(needs_unit_suffix("EXIF:FocalLength"));
///
/// // Tags that don't need suffix
/// assert!(!needs_unit_suffix("ExposureTime"));
/// assert!(!needs_unit_suffix("ISO"));
/// assert!(!needs_unit_suffix("Model"));
/// ```
pub fn needs_unit_suffix(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    MM_SUFFIX_TAGS.contains(&base_name) || METER_SUFFIX_TAGS.contains(&base_name)
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
/// The unit suffix ("mm" or "m") or `None` if no suffix applies.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::unit_suffixes::get_unit_suffix;
///
/// assert_eq!(get_unit_suffix("FocalLength"), Some("mm"));
/// assert_eq!(get_unit_suffix("SubjectDistance"), Some("m"));
/// assert_eq!(get_unit_suffix("ExposureTime"), None);
/// assert_eq!(get_unit_suffix("ISO"), None);
/// ```
pub fn get_unit_suffix(tag_name: &str) -> Option<&'static str> {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);

    if MM_SUFFIX_TAGS.contains(&base_name) {
        Some("mm")
    } else if METER_SUFFIX_TAGS.contains(&base_name) {
        Some("m")
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

    /// EXIF 0x920a forces one decimal: Exif.pm:2401
    /// `PrintConv => 'sprintf("%.1f mm",$val)'`.
    ///
    /// Each pair below is a real corpus case where `exiftool -G1 -s` prints
    /// the left value and oxidex printed the right one before this rule:
    /// CanonRaw.cr3 15.0/15, DNG.dng 55.0/55, Nikon.nef 18.0/18,
    /// FujiFilm.raf 70.0/70, Minolta.mrw 7.2/7.203125.
    #[test]
    fn test_exif_focal_length_forces_one_decimal() {
        assert_eq!(format_with_unit("ExifIFD:FocalLength", "15"), "15.0 mm");
        assert_eq!(format_with_unit("EXIF:FocalLength", "55"), "55.0 mm");
        assert_eq!(format_with_unit("ExifIFD:FocalLength", "18"), "18.0 mm");
        assert_eq!(format_with_unit("ExifIFD:FocalLength", "70"), "70.0 mm");
        // sprintf rounds, it does not truncate
        assert_eq!(
            format_with_unit("ExifIFD:FocalLength", "7.203125"),
            "7.2 mm"
        );
        assert_eq!(format_with_unit("ExifIFD:FocalLength", "10.093"), "10.1 mm");
        assert_eq!(format_with_unit("ExifIFD:FocalLength", "1.48"), "1.5 mm");
        // Already-correct renderings are unchanged
        assert_eq!(format_with_unit("ExifIFD:FocalLength", "3.3"), "3.3 mm");
        assert_eq!(format_with_unit("ExifIFD:FocalLength", "24.2"), "24.2 mm");
    }

    /// The maker-note tables named `FocalLength` print `"$val mm"` with no
    /// forced decimal (Canon.pm:3138, Canon.pm:3176 --
    /// `[Canon] FocalLength : 34 mm`), so the EXIF rule must not reach them.
    #[test]
    fn test_makernote_focal_length_keeps_bare_value() {
        assert_eq!(format_with_unit("Canon:FocalLength", "34"), "34 mm");
        assert_eq!(format_with_unit("MakerNotes:FocalLength", "55"), "55 mm");
        assert_eq!(format_with_unit("Nikon:FocalLength", "18.3 mm"), "18.3 mm");
        // A bare, group-less name cannot be identified as EXIF's, and the
        // maker-note parsers are the ones that pass values that way.
        assert_eq!(format_with_unit("FocalLength", "50"), "50 mm");
    }

    /// EXIF's FocalLengthIn35mmFormat is a DIFFERENT PrintConv --
    /// Exif.pm:2842 `PrintConv => '"$val mm"'`, an int16u with no decimal
    /// (`[ExifIFD] FocalLengthIn35mmFormat : 77 mm`). Only 0x920a rounds.
    #[test]
    fn test_focal_length_in_35mm_format() {
        assert_eq!(
            format_with_unit("ExifIFD:FocalLengthIn35mmFormat", "77"),
            "77 mm"
        );
        assert_eq!(
            format_with_unit("EXIF:FocalLengthIn35mmFormat", "105"),
            "105 mm"
        );
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
        assert_eq!(format_with_unit("EXIF:FocalLength", "50"), "50.0 mm");
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

    /// `Composite:FocalLength35efl`'s PrintConv (Exif.pm:4720) already renders
    /// both millimetre figures itself, so the string it hands over ends in
    /// `")"` rather than `" mm"`. Appending a second unit here is what made
    /// `exiftool -a -G1 -s` disagree with oxidex on 43 of 148 sample files.
    #[test]
    fn composite_focal_length_35efl_keeps_its_own_units() {
        assert_eq!(
            format_with_unit(
                "Composite:FocalLength35efl",
                "4.2 mm (35 mm equivalent: 26.0 mm)"
            ),
            "4.2 mm (35 mm equivalent: 26.0 mm)"
        );
        assert_eq!(
            format_with_unit("FocalLength35efl", "15.4 mm (35 mm equivalent: 75.1 mm)"),
            "15.4 mm (35 mm equivalent: 75.1 mm)"
        );
        // The single-argument branch of the same PrintConv.
        assert_eq!(
            format_with_unit("Composite:FocalLength35efl", "50.0 mm"),
            "50.0 mm"
        );
        // A bare number still gets its unit.
        assert_eq!(format_with_unit("FocalLength35efl", "24"), "24 mm");
        assert_eq!(format_with_unit("FocalLength", "50-200"), "50-200 mm");
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
    // ExposureTime / ShutterSpeedValue carry no unit at all
    // ------------------------------------------------------------------------

    /// `PrintExposureTime` (Exif.pm:5606) is the PrintConv for both 0x829a and
    /// 0x9201 and it never emits a unit -- the installed 13.55 Perl returns
    /// `"2"` for 2 seconds, not `"2 s"`. These values must pass through
    /// untouched, whatever their magnitude.
    #[test]
    fn exposure_time_never_gains_a_seconds_suffix() {
        for v in ["1", "2", "1.5", "30", "1/125", "1/1000", "0.5", "0.001"] {
            assert_eq!(format_with_unit("ExposureTime", v), v);
            assert_eq!(format_with_unit("EXIF:ExposureTime", v), v);
            assert_eq!(format_with_unit("ShutterSpeedValue", v), v);
        }
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
    fn test_needs_unit_suffix_other_tags() {
        // Time tags render through PrintExposureTime, which emits no unit.
        assert!(!needs_unit_suffix("ExposureTime"));
        assert!(!needs_unit_suffix("ShutterSpeedValue"));
        assert!(!needs_unit_suffix("EXIF:ExposureTime"));
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
    fn test_get_unit_suffix_none() {
        assert_eq!(get_unit_suffix("ExposureTime"), None);
        assert_eq!(get_unit_suffix("ShutterSpeedValue"), None);
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
