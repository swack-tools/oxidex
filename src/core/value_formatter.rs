//! Value formatting to match ExifTool conventions
//!
//! This module provides formatting functions for various types of metadata values
//! to ensure they match ExifTool's output format exactly, including:
//! - File sizes (e.g., "2.1 kB" not "2 kB")
//! - EXIF dates (YYYY:MM:DD HH:MM:SS)
//! - ISO 8601 to EXIF-style date conversion
//! - IPTC dates (YYYYMMDD -> YYYY:MM:DD)
//! - IPTC times (HHMMSS±HHMM -> HH:MM:SS±HH:MM)
//! - Rational numbers with tag-specific formatting
//! - Rational-to-decimal conversion for specific tags (ApertureValue, FocalLength, etc.)
//! - Unit suffix formatting (mm for focal lengths, m for distances/altitudes)

/// Format file size like ExifTool (e.g., "2.1 kB" not "2 kB")
///
/// ExifTool uses decimal (base-10) units, not binary (base-2) units.
/// - 1 kB = 1000 bytes (not 1024)
/// - 1 MB = 1,000,000 bytes
/// - 1 GB = 1,000,000,000 bytes
///
/// Small files (< 1000 bytes) show exact byte count.
/// Larger files show one decimal place.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_file_size;
///
/// assert_eq!(format_file_size(500), "500 bytes");
/// assert_eq!(format_file_size(2100), "2.1 kB");
/// assert_eq!(format_file_size(1_500_000), "1.5 MB");
/// assert_eq!(format_file_size(2_500_000_000), "2.5 GB");
/// ```
pub fn format_file_size(bytes: u64) -> String {
    if bytes < 1000 {
        format!("{} bytes", bytes)
    } else if bytes < 1_000_000 {
        format!("{:.1} kB", bytes as f64 / 1000.0)
    } else if bytes < 1_000_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    }
}

/// Format EXIF date/time to ExifTool format (YYYY:MM:DD HH:MM:SS)
///
/// ExifTool uses colons in dates, not dashes.
///
/// # Examples
///
/// ```
/// use oxidex::chrono::{TimeZone, Utc};
/// use oxidex::core::value_formatter::format_exif_datetime;
///
/// let dt = Utc.with_ymd_and_hms(2002, 6, 20, 2, 11, 11).unwrap();
/// assert_eq!(format_exif_datetime(&dt), "2002:06:20 02:11:11");
/// ```
pub fn format_exif_datetime(dt: &chrono::DateTime<chrono::Utc>) -> String {
    dt.format("%Y:%m:%d %H:%M:%S").to_string()
}

/// Format IPTC date from raw format (YYYYMMDD -> YYYY:MM:DD)
///
/// IPTC stores dates as 8-digit strings without separators.
/// ExifTool displays them with colon separators.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_iptc_date;
///
/// assert_eq!(format_iptc_date("20020620"), "2002:06:20");
/// assert_eq!(format_iptc_date("19991231"), "1999:12:31");
/// assert_eq!(format_iptc_date("invalid"), "invalid"); // Preserves invalid input
/// ```
pub fn format_iptc_date(raw: &str) -> String {
    if raw.len() == 8 {
        format!("{}:{}:{}", &raw[0..4], &raw[4..6], &raw[6..8])
    } else {
        raw.to_string()
    }
}

/// Format IPTC time from raw format (HHMMSS±HHMM -> HH:MM:SS±HH:MM)
///
/// IPTC stores times as 6-digit strings (HHMMSS) optionally followed by
/// timezone offset (±HHMM). ExifTool displays them with colon separators.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_iptc_time;
///
/// assert_eq!(format_iptc_time("021111+0100"), "02:11:11+01:00");
/// assert_eq!(format_iptc_time("143000-0500"), "14:30:00-05:00");
/// assert_eq!(format_iptc_time("120000"), "12:00:00"); // No timezone
/// assert_eq!(format_iptc_time("bad"), "bad"); // Preserves invalid input
/// ```
pub fn format_iptc_time(raw: &str) -> String {
    if raw.len() >= 6 {
        let base = format!("{}:{}:{}", &raw[0..2], &raw[2..4], &raw[4..6]);
        if raw.len() >= 11 {
            // Format: HHMMSS±HHMM -> HH:MM:SS±HH:MM
            // Extract timezone: ±HHMM at positions 6-11
            let tz_sign = &raw[6..7];
            let tz_hours = &raw[7..9];
            let tz_mins = &raw[9..11];
            format!("{}{}{}:{}", base, tz_sign, tz_hours, tz_mins)
        } else {
            base
        }
    } else {
        raw.to_string()
    }
}

/// Format IPTC urgency value with human-readable description.
///
/// IPTC urgency is a single digit 1-8, where 1 is most urgent and 8 is least.
/// ExifTool displays this with a parenthetical description for boundary values.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_iptc_urgency;
///
/// assert_eq!(format_iptc_urgency("1"), "1 (most urgent)");
/// assert_eq!(format_iptc_urgency("5"), "5 (normal)");
/// assert_eq!(format_iptc_urgency("8"), "8 (least urgent)");
/// assert_eq!(format_iptc_urgency("3"), "3");
/// assert_eq!(format_iptc_urgency("invalid"), "invalid"); // Preserves invalid input
/// ```
pub fn format_iptc_urgency(raw: &str) -> String {
    match raw.trim() {
        "1" => "1 (most urgent)".to_string(),
        "5" => "5 (normal)".to_string(),
        "8" => "8 (least urgent)".to_string(),
        _ => raw.to_string(),
    }
}

/// Format IPTC CodedCharacterSet from ISO 2022 escape sequence to human-readable.
///
/// IPTC uses ISO 2022 escape sequences to indicate character encoding.
/// The most common is ESC %G (0x1B 0x25 0x47) which means UTF-8.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_iptc_coded_charset;
///
/// assert_eq!(format_iptc_coded_charset(&[0x1B, 0x25, 0x47]), "UTF8");
/// assert_eq!(format_iptc_coded_charset(&[0x1B, 0x2E, 0x41]), "ISO-8859-1");
/// assert_eq!(format_iptc_coded_charset(b"other"), "other");
/// ```
pub fn format_iptc_coded_charset(data: &[u8]) -> String {
    // ESC %G = UTF-8 (ISO 2022 escape sequence)
    if data == [0x1B, 0x25, 0x47] {
        return "UTF8".to_string();
    }
    // ESC .A = ISO-8859-1 (Latin-1)
    if data == [0x1B, 0x2E, 0x41] {
        return "ISO-8859-1".to_string();
    }
    // ESC .B = ISO-8859-2 (Latin-2)
    if data == [0x1B, 0x2E, 0x42] {
        return "ISO-8859-2".to_string();
    }
    // ESC .C = ISO-8859-3 (Latin-3)
    if data == [0x1B, 0x2E, 0x43] {
        return "ISO-8859-3".to_string();
    }
    // ESC .D = ISO-8859-4 (Latin-4)
    if data == [0x1B, 0x2E, 0x44] {
        return "ISO-8859-4".to_string();
    }
    // ESC .E = ISO-8859-5 (Cyrillic)
    if data == [0x1B, 0x2E, 0x45] {
        return "ISO-8859-5".to_string();
    }
    // ESC .F = ISO-8859-6 (Arabic)
    if data == [0x1B, 0x2E, 0x46] {
        return "ISO-8859-6".to_string();
    }
    // ESC .G = ISO-8859-7 (Greek)
    if data == [0x1B, 0x2E, 0x47] {
        return "ISO-8859-7".to_string();
    }
    // ESC .H = ISO-8859-8 (Hebrew)
    if data == [0x1B, 0x2E, 0x48] {
        return "ISO-8859-8".to_string();
    }
    // Fallback: try to decode as string
    String::from_utf8_lossy(data).trim().to_string()
}

/// Format IPTC record version from raw bytes.
///
/// IPTC record versions are stored as 2-byte big-endian integers.
/// ExifTool displays them as decimal numbers.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_iptc_record_version;
///
/// assert_eq!(format_iptc_record_version(&[0x00, 0x04]), "4");
/// assert_eq!(format_iptc_record_version(&[0x00, 0x02]), "2");
/// assert_eq!(format_iptc_record_version(&[0x01, 0x00]), "256");
/// ```
pub fn format_iptc_record_version(data: &[u8]) -> String {
    if data.len() >= 2 {
        let version = u16::from_be_bytes([data[0], data[1]]);
        version.to_string()
    } else if data.len() == 1 {
        data[0].to_string()
    } else {
        String::new()
    }
}

/// Convert ISO 8601 date to EXIF-style date format.
///
/// This function transforms ISO 8601 formatted dates (with 'T' separator and dashes)
/// to EXIF-style format (with colons in date and space separator).
///
/// # Parameters
///
/// * `iso_date` - The ISO 8601 formatted date string to convert
/// * `preserve_timezone` - If true, appends timezone offset (for XMP dates).
///   If false, strips timezone (for basic EXIF dates).
///
/// # Format Conversion
///
/// - Input:  `2001-05-19T18:36:41+00:00`
/// - Output: `2001:05:19 18:36:41` (preserve_timezone = false)
/// - Output: `2001:05:19 18:36:41+00:00` (preserve_timezone = true)
///
/// For dates with subseconds:
/// - Input:  `2003-03-03T03:33:33.333+03:00`
/// - Output: `2003:03:03 03:33:33.333+03:00` (preserve_timezone = true)
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_date_exif_style;
///
/// // Basic EXIF date (no timezone preserved)
/// assert_eq!(
///     format_date_exif_style("2001-05-19T18:36:41+00:00", false),
///     "2001:05:19 18:36:41"
/// );
///
/// // XMP date with subseconds and timezone preserved
/// assert_eq!(
///     format_date_exif_style("2003-03-03T03:33:33.333+03:00", true),
///     "2003:03:03 03:33:33.333+03:00"
/// );
///
/// // Non-ISO format passes through unchanged
/// assert_eq!(
///     format_date_exif_style("2001:05:19 18:36:41", false),
///     "2001:05:19 18:36:41"
/// );
/// ```
pub fn format_date_exif_style(iso_date: &str, preserve_timezone: bool) -> String {
    // Quick check: ISO 8601 dates must have 'T' separator at position 10
    // Format: YYYY-MM-DDTHH:MM:SS...
    // Positions: 0123456789...
    if iso_date.len() < 19 {
        return iso_date.to_string();
    }

    let bytes = iso_date.as_bytes();

    // Validate basic ISO 8601 structure:
    // - Position 4 and 7 should be '-'
    // - Position 10 should be 'T'
    // - Position 13 and 16 should be ':'
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return iso_date.to_string();
    }

    // Extract date and time components
    let year = &iso_date[0..4];
    let month = &iso_date[5..7];
    let day = &iso_date[8..10];
    let hour = &iso_date[11..13];
    let min = &iso_date[14..16];
    let sec = &iso_date[17..19];

    // Build the base EXIF-style date/time string
    let mut result = format!("{}:{}:{} {}:{}:{}", year, month, day, hour, min, sec);

    // Parse the remainder after seconds (position 19 onwards)
    // This may contain: subseconds (.xxx), timezone (Z or +HH:MM/-HH:MM), or both
    let remainder = &iso_date[19..];

    if remainder.is_empty() {
        return result;
    }

    // Check for subseconds (starts with '.')
    let (subseconds, tz_start) = if let Some(after_dot) = remainder.strip_prefix('.') {
        // Find where subseconds end (at timezone start or end of string)
        let subsec_end = after_dot
            .find(['+', '-', 'Z'])
            .map(|pos| pos + 1) // +1 to include the '.' prefix
            .unwrap_or(remainder.len());
        (Some(&remainder[..subsec_end]), subsec_end)
    } else {
        (None, 0)
    };

    // Append subseconds if present
    if let Some(subsec) = subseconds {
        result.push_str(subsec);
    }

    // Handle timezone if preserve_timezone is true
    if preserve_timezone && tz_start < remainder.len() {
        let tz_str = &remainder[tz_start..];
        // Skip 'Z' (UTC indicator) - ExifTool typically doesn't include Z
        if !tz_str.is_empty() && tz_str != "Z" {
            result.push_str(tz_str);
        }
    }

    result
}

/// Tags that use EXIF-style date format (no T separator, colons in date).
///
/// These tags should have their ISO 8601 dates converted to EXIF-style
/// format without preserving timezone information.
pub const EXIF_DATE_TAGS: &[&str] = &[
    "CreateDate",
    "DateTimeOriginal",
    "ModifyDate",
    "DateTimeDigitized",
    "DateTime",
    "DateTimeCreated",
    "GPSDateStamp",
];

/// Tags that preserve timezone in EXIF-style format (XMP dates).
///
/// These XMP date tags should have their ISO 8601 dates converted to
/// EXIF-style format while preserving subseconds and timezone information.
pub const XMP_DATE_TAGS: &[&str] = &["XMP:ModifyDate", "XMP:CreateDate", "XMP:MetadataDate"];

/// Format rational number as ExifTool does
///
/// Different tags have different formatting conventions:
/// - ExposureTime: Display as fraction (1/125) or decimal for >= 1 second
/// - FNumber: Display as decimal with one place (f/2.8)
/// - Other rationals: Display as fraction (num/denom)
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_rational;
///
/// // Exposure time as fraction
/// assert_eq!(format_rational(1, 125, "ExposureTime"), "1/125");
///
/// // Exposure time >= 1 second
/// assert_eq!(format_rational(2, 1, "ExposureTime"), "2.0");
///
/// // F-number as decimal
/// assert_eq!(format_rational(28, 10, "FNumber"), "2.8");
///
/// // Unknown tag as fraction
/// assert_eq!(format_rational(3, 2, "SomeTag"), "3/2");
///
/// // Division by zero
/// assert_eq!(format_rational(1, 0, "AnyTag"), "undef");
/// ```
pub fn format_rational(num: i32, denom: i32, tag_name: &str) -> String {
    if denom == 0 {
        return "undef".to_string();
    }

    // Some tags have special formatting
    match tag_name {
        "ExposureTime" => {
            let val = num as f64 / denom as f64;
            if val >= 1.0 {
                // Show as decimal for exposure >= 1 second
                format!("{:.1}", val)
            } else if num == 1 {
                // Show as simple fraction for 1/x
                format!("1/{}", denom)
            } else {
                // Show as approximate fraction
                format!("1/{:.0}", 1.0 / val)
            }
        }
        "FNumber" => {
            // F-number shown as decimal
            format!("{:.1}", num as f64 / denom as f64)
        }
        _ => {
            // Default: show as fraction
            format!("{}/{}", num, denom)
        }
    }
}

/// Tags that should be formatted as decimal values instead of raw rationals.
///
/// These tags represent measurements (aperture, focal length, resolution, etc.)
/// where ExifTool displays the computed decimal value rather than the raw
/// numerator/denominator fraction (e.g., "3.5" instead of "350/100").
///
/// This list is used by formatting logic to determine when to apply
/// [`format_rational_as_decimal`] instead of showing raw rational values.
pub const DECIMAL_RATIONAL_TAGS: &[&str] = &[
    "ApertureValue",
    "BrightnessValue",
    "CompressedBitsPerPixel",
    "DigitalZoomRatio",
    "ExposureCompensation",
    "ExposureBiasValue",
    "FNumber",
    "FocalLength",
    "FocalPlaneXResolution",
    "FocalPlaneYResolution",
    "Gamma",
    "MaxApertureValue",
    "SubjectDistance",
    "XResolution",
    "YResolution",
];

/// Format a rational value (numerator/denominator) as a decimal string.
///
/// This function converts rational numbers to their decimal representation,
/// matching ExifTool's output format for tags like ApertureValue, FocalLength,
/// and XResolution. The formatting follows these rules:
///
/// - Division by zero returns "inf"
/// - Integer results display without decimal point (e.g., "72" not "72.0")
/// - Decimal results use up to 6 decimal places with trailing zeros trimmed
///   (e.g., "3.5" not "3.500000")
///
/// # Arguments
///
/// * `numerator` - The numerator of the rational value
/// * `denominator` - The denominator of the rational value
///
/// # Returns
///
/// A string representation of the decimal value.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_rational_as_decimal;
///
/// // Standard decimal conversion
/// assert_eq!(format_rational_as_decimal(360, 100), "3.6");
/// assert_eq!(format_rational_as_decimal(350, 100), "3.5");
///
/// // Integer results (no decimal point)
/// assert_eq!(format_rational_as_decimal(1, 1), "1");
/// assert_eq!(format_rational_as_decimal(72, 1), "72");
///
/// // Zero numerator
/// assert_eq!(format_rational_as_decimal(0, 100), "0");
///
/// // Division by zero
/// assert_eq!(format_rational_as_decimal(1, 0), "inf");
/// ```
pub fn format_rational_as_decimal(numerator: i64, denominator: i64) -> String {
    // Handle division by zero - return "inf" to indicate undefined/infinite value
    if denominator == 0 {
        return "inf".to_string();
    }

    let value = numerator as f64 / denominator as f64;

    // ExifTool displays clean integers without a decimal point
    // (e.g., "72" for XResolution, not "72.0")
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        // Format with up to 9 decimal places, then trim trailing zeros
        // ExifTool uses 9+ decimal precision for many rational values
        // This ensures we get "3.5" instead of "3.500000000" while still
        // preserving precision for values that need it
        let formatted = format!("{:.9}", value);
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

/// Check if a tag name should be formatted as a decimal rational.
///
/// This is a convenience function to determine if a given tag name
/// is in the [`DECIMAL_RATIONAL_TAGS`] list.
///
/// # Arguments
///
/// * `tag_name` - The name of the tag to check
///
/// # Returns
///
/// `true` if the tag should be formatted as a decimal, `false` otherwise.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::is_decimal_rational_tag;
///
/// assert!(is_decimal_rational_tag("FocalLength"));
/// assert!(is_decimal_rational_tag("XResolution"));
/// assert!(!is_decimal_rational_tag("ExposureTime"));
/// assert!(!is_decimal_rational_tag("UnknownTag"));
/// ```
pub fn is_decimal_rational_tag(tag_name: &str) -> bool {
    DECIMAL_RATIONAL_TAGS.contains(&tag_name)
}

/// Tags that need "mm" suffix (focal length related)
///
/// These tags represent focal lengths and should be displayed with "mm" suffix
/// to match ExifTool's output format.
pub const MM_SUFFIX_TAGS: &[&str] = &[
    "FocalLength",
    "FocalLengthIn35mmFormat",
    "FocalLength35efl",
    "FocalLengthIn35mmFilm",
];

/// Tags that need "m" suffix (distance/altitude measurements)
///
/// These tags represent distances or altitudes in meters and should be
/// displayed with "m" suffix to match ExifTool's output format.
pub const METER_SUFFIX_TAGS: &[&str] = &["SubjectDistance", "GPSAltitude", "HyperfocalDistance"];

/// Tags that need "s" suffix (time measurements)
///
/// These tags represent time durations in seconds. Note that ExifTool
/// doesn't always add "s" to ExposureTime, so we handle this carefully.
pub const SECONDS_SUFFIX_TAGS: &[&str] = &["ExposureTime", "ShutterSpeedValue"];

/// Format a value with the appropriate unit suffix based on tag name
///
/// This function examines the tag name and appends the correct unit suffix
/// (mm, m, or s) to match ExifTool's output format. It handles fully-qualified
/// tag names (e.g., "EXIF:FocalLength") by extracting the base name.
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally prefixed with group (e.g., "EXIF:FocalLength")
/// * `value` - The formatted value string to append suffix to
///
/// # Returns
///
/// The value with appropriate unit suffix, or unchanged if no suffix is needed.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_with_unit;
///
/// assert_eq!(format_with_unit("FocalLength", "6.0"), "6.0 mm");
/// assert_eq!(format_with_unit("SubjectDistance", "2.5"), "2.5 m");
/// assert_eq!(format_with_unit("EXIF:FocalLength", "50"), "50 mm");
/// assert_eq!(format_with_unit("SomeOtherTag", "123"), "123");
/// ```
pub fn format_with_unit(tag_name: &str, value: &str) -> String {
    // Extract the base tag name (after the colon if present)
    // This handles fully-qualified names like "EXIF:FocalLength"
    let base_name = tag_name.split(':').next_back().unwrap_or(tag_name);

    if MM_SUFFIX_TAGS.contains(&base_name) {
        format!("{} mm", value)
    } else if METER_SUFFIX_TAGS.contains(&base_name) {
        format!("{} m", value)
    } else if SECONDS_SUFFIX_TAGS.contains(&base_name) {
        // ExifTool doesn't always add "s" to ExposureTime - it depends on context.
        // Only add "s" if it's not already present.
        if !value.ends_with('s') && !value.ends_with("sec") {
            // Note: ExifTool typically shows exposure as "1/125" without suffix,
            // but ShutterSpeedValue may include "s". For now, we don't add suffix
            // automatically to match the most common ExifTool behavior.
            value.to_string()
        } else {
            value.to_string()
        }
    } else {
        value.to_string()
    }
}

/// Check if a tag should have a unit suffix
///
/// This is useful for determining whether additional formatting is needed
/// for a particular tag's value.
///
/// # Arguments
///
/// * `tag_name` - The tag name, optionally prefixed with group (e.g., "EXIF:FocalLength")
///
/// # Returns
///
/// `true` if the tag should have a unit suffix (mm or m), `false` otherwise.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::needs_unit_suffix;
///
/// assert!(needs_unit_suffix("FocalLength"));
/// assert!(needs_unit_suffix("EXIF:SubjectDistance"));
/// assert!(!needs_unit_suffix("ISO"));
/// assert!(!needs_unit_suffix("Model"));
/// ```
pub fn needs_unit_suffix(tag_name: &str) -> bool {
    let base_name = tag_name.split(':').next_back().unwrap_or(tag_name);
    MM_SUFFIX_TAGS.contains(&base_name) || METER_SUFFIX_TAGS.contains(&base_name)
}

// ============================================================================
// GPS REFERENCE VALUE FORMATTING
// ============================================================================

/// Format GPS reference values to human-readable descriptions.
///
/// GPS tags store reference values as single characters or numeric codes,
/// but ExifTool displays them as human-readable descriptions. This function
/// converts the raw values to match ExifTool's output format.
///
/// # Arguments
///
/// * `tag_name` - The tag name (e.g., "GPSLatitudeRef", "GPS:GPSAltitudeRef")
/// * `value` - The raw value (string or numeric)
///
/// # Returns
///
/// The human-readable description, or None if no mapping exists.
pub fn format_gps_reference(tag_name: &str, value: &str) -> Option<String> {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);

    match base_name {
        "GPSLatitudeRef" | "GPSDestLatitudeRef" => match value.trim() {
            "N" => Some("North".to_string()),
            "S" => Some("South".to_string()),
            _ => None,
        },
        "GPSLongitudeRef" | "GPSDestLongitudeRef" => match value.trim() {
            "E" => Some("East".to_string()),
            "W" => Some("West".to_string()),
            _ => None,
        },
        "GPSAltitudeRef" => match value.trim() {
            "0" | "\x00" => Some("Above Sea Level".to_string()),
            "1" | "\x01" => Some("Below Sea Level".to_string()),
            _ => None,
        },
        "GPSImgDirectionRef" | "GPSDestBearingRef" | "GPSTrackRef" => match value.trim() {
            "T" => Some("True North".to_string()),
            "M" => Some("Magnetic North".to_string()),
            _ => None,
        },
        "GPSSpeedRef" => match value.trim() {
            "K" => Some("km/h".to_string()),
            "M" => Some("mph".to_string()),
            "N" => Some("knots".to_string()),
            _ => None,
        },
        "GPSDestDistanceRef" => match value.trim() {
            "K" => Some("Kilometers".to_string()),
            "M" => Some("Miles".to_string()),
            "N" => Some("Nautical Miles".to_string()),
            _ => None,
        },
        "GPSMeasureMode" => match value.trim() {
            "2" => Some("2-Dimensional Measurement".to_string()),
            "3" => Some("3-Dimensional Measurement".to_string()),
            _ => None,
        },
        "GPSStatus" => match value.trim() {
            "A" => Some("Measurement Active".to_string()),
            "V" => Some("Measurement Void".to_string()),
            _ => None,
        },
        "GPSDifferential" => match value.trim() {
            "0" => Some("No Correction".to_string()),
            "1" => Some("Differential Corrected".to_string()),
            _ => None,
        },
        _ => None,
    }
}

/// List of GPS reference tag names that should have their values formatted.
pub const GPS_REFERENCE_TAGS: &[&str] = &[
    "GPSLatitudeRef",
    "GPSLongitudeRef",
    "GPSAltitudeRef",
    "GPSImgDirectionRef",
    "GPSDestBearingRef",
    "GPSTrackRef",
    "GPSSpeedRef",
    "GPSDestDistanceRef",
    "GPSMeasureMode",
    "GPSStatus",
    "GPSDifferential",
    "GPSDestLatitudeRef",
    "GPSDestLongitudeRef",
];

/// Check if a tag name is a GPS reference tag that needs formatting.
pub fn is_gps_reference_tag(tag_name: &str) -> bool {
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    GPS_REFERENCE_TAGS.contains(&base_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_size_formatting() {
        // Bytes (< 1000)
        assert_eq!(format_file_size(0), "0 bytes");
        assert_eq!(format_file_size(1), "1 bytes");
        assert_eq!(format_file_size(500), "500 bytes");
        assert_eq!(format_file_size(999), "999 bytes");

        // Kilobytes (1000 - 999,999)
        assert_eq!(format_file_size(1000), "1.0 kB");
        assert_eq!(format_file_size(1500), "1.5 kB");
        assert_eq!(format_file_size(2100), "2.1 kB");
        assert_eq!(format_file_size(10_000), "10.0 kB");
        assert_eq!(format_file_size(999_999), "1000.0 kB");

        // Megabytes (1,000,000 - 999,999,999)
        assert_eq!(format_file_size(1_000_000), "1.0 MB");
        assert_eq!(format_file_size(1_500_000), "1.5 MB");
        assert_eq!(format_file_size(10_000_000), "10.0 MB");

        // Gigabytes (>= 1,000,000,000)
        assert_eq!(format_file_size(1_000_000_000), "1.0 GB");
        assert_eq!(format_file_size(2_500_000_000), "2.5 GB");
    }

    #[test]
    fn test_iptc_date_formatting() {
        // Valid dates
        assert_eq!(format_iptc_date("20020620"), "2002:06:20");
        assert_eq!(format_iptc_date("19991231"), "1999:12:31");
        assert_eq!(format_iptc_date("20250101"), "2025:01:01");

        // Invalid dates (preserved as-is)
        assert_eq!(format_iptc_date("2002620"), "2002620");
        assert_eq!(format_iptc_date("200206200"), "200206200");
        assert_eq!(format_iptc_date("invalid"), "invalid");
        assert_eq!(format_iptc_date(""), "");
    }

    #[test]
    fn test_iptc_time_formatting() {
        // With timezone
        assert_eq!(format_iptc_time("021111+0100"), "02:11:11+01:00");
        assert_eq!(format_iptc_time("143000-0500"), "14:30:00-05:00");
        assert_eq!(format_iptc_time("235959+0000"), "23:59:59+00:00");

        // Without timezone
        assert_eq!(format_iptc_time("120000"), "12:00:00");
        assert_eq!(format_iptc_time("000000"), "00:00:00");

        // Invalid times (preserved as-is)
        assert_eq!(format_iptc_time("12345"), "12345");
        assert_eq!(format_iptc_time("bad"), "bad");
        assert_eq!(format_iptc_time(""), "");
    }

    #[test]
    fn test_rational_formatting() {
        // ExposureTime - fractions
        assert_eq!(format_rational(1, 125, "ExposureTime"), "1/125");
        assert_eq!(format_rational(1, 1000, "ExposureTime"), "1/1000");

        // ExposureTime - >= 1 second
        assert_eq!(format_rational(2, 1, "ExposureTime"), "2.0");
        assert_eq!(format_rational(5, 2, "ExposureTime"), "2.5");

        // FNumber
        assert_eq!(format_rational(28, 10, "FNumber"), "2.8");
        assert_eq!(format_rational(56, 10, "FNumber"), "5.6");
        assert_eq!(format_rational(8, 1, "FNumber"), "8.0");

        // Other tags (default to fraction)
        assert_eq!(format_rational(3, 2, "SomeTag"), "3/2");
        assert_eq!(format_rational(100, 1, "OtherTag"), "100/1");

        // Division by zero
        assert_eq!(format_rational(1, 0, "ExposureTime"), "undef");
        assert_eq!(format_rational(1, 0, "FNumber"), "undef");
        assert_eq!(format_rational(1, 0, "AnyTag"), "undef");
    }

    #[test]
    fn test_exif_datetime_formatting() {
        use chrono::{TimeZone, Utc};

        let dt = Utc.with_ymd_and_hms(2002, 6, 20, 2, 11, 11).unwrap();
        assert_eq!(format_exif_datetime(&dt), "2002:06:20 02:11:11");

        let dt2 = Utc.with_ymd_and_hms(2025, 12, 31, 23, 59, 59).unwrap();
        assert_eq!(format_exif_datetime(&dt2), "2025:12:31 23:59:59");

        let dt3 = Utc.with_ymd_and_hms(1999, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(format_exif_datetime(&dt3), "1999:01:01 00:00:00");
    }

    #[test]
    fn test_rational_as_decimal_formatting() {
        // Standard decimal conversions - these are the primary use cases
        // for tags like ApertureValue, FocalLength, etc.
        assert_eq!(format_rational_as_decimal(360, 100), "3.6");
        assert_eq!(format_rational_as_decimal(350, 100), "3.5");
        assert_eq!(format_rational_as_decimal(22, 10), "2.2");

        // Integer results should display without decimal point
        // (e.g., XResolution of 72/1 should be "72", not "72.0")
        assert_eq!(format_rational_as_decimal(1, 1), "1");
        assert_eq!(format_rational_as_decimal(3053, 1), "3053");
        assert_eq!(format_rational_as_decimal(72, 1), "72");

        // Zero numerator
        assert_eq!(format_rational_as_decimal(0, 100), "0");

        // Division by zero returns "inf"
        assert_eq!(format_rational_as_decimal(1, 0), "inf");

        // Negative values (for tags like ExposureCompensation)
        assert_eq!(format_rational_as_decimal(-100, 100), "-1");
        assert_eq!(format_rational_as_decimal(-150, 100), "-1.5");

        // Precision edge cases - ensure trailing zeros are trimmed
        // Using 9 decimal places for ExifTool compatibility
        assert_eq!(format_rational_as_decimal(1, 3), "0.333333333"); // Repeating decimal
        assert_eq!(format_rational_as_decimal(1, 4), "0.25");
        assert_eq!(format_rational_as_decimal(1, 8), "0.125");
    }

    #[test]
    fn test_is_decimal_rational_tag() {
        // Tags that should be formatted as decimals
        assert!(is_decimal_rational_tag("ApertureValue"));
        assert!(is_decimal_rational_tag("FocalLength"));
        assert!(is_decimal_rational_tag("XResolution"));
        assert!(is_decimal_rational_tag("YResolution"));
        assert!(is_decimal_rational_tag("FNumber"));
        assert!(is_decimal_rational_tag("ExposureCompensation"));
        assert!(is_decimal_rational_tag("ExposureBiasValue"));

        // Tags that should NOT be formatted as decimals
        assert!(!is_decimal_rational_tag("ExposureTime"));
        assert!(!is_decimal_rational_tag("ShutterSpeed"));
        assert!(!is_decimal_rational_tag("UnknownTag"));
        assert!(!is_decimal_rational_tag(""));
    }

    #[test]
    fn test_decimal_rational_tags_list() {
        // Verify the list contains expected tags
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"ApertureValue"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"BrightnessValue"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"CompressedBitsPerPixel"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"DigitalZoomRatio"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"ExposureCompensation"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"ExposureBiasValue"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"FNumber"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"FocalLength"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"FocalPlaneXResolution"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"FocalPlaneYResolution"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"Gamma"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"MaxApertureValue"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"SubjectDistance"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"XResolution"));
        assert!(DECIMAL_RATIONAL_TAGS.contains(&"YResolution"));

        // Verify expected count
        assert_eq!(DECIMAL_RATIONAL_TAGS.len(), 15);
    }

    #[test]
    fn test_mm_suffix() {
        // FocalLength tags should get "mm" suffix
        assert_eq!(format_with_unit("FocalLength", "6.0"), "6.0 mm");
        assert_eq!(format_with_unit("EXIF:FocalLength", "50"), "50 mm");
        assert_eq!(format_with_unit("FocalLengthIn35mmFormat", "75"), "75 mm");
        assert_eq!(format_with_unit("FocalLength35efl", "24"), "24 mm");
        assert_eq!(format_with_unit("FocalLengthIn35mmFilm", "100"), "100 mm");

        // With group prefix
        assert_eq!(
            format_with_unit("Composite:FocalLengthIn35mmFormat", "35"),
            "35 mm"
        );
    }

    #[test]
    fn test_meter_suffix() {
        // Distance/altitude tags should get "m" suffix
        assert_eq!(format_with_unit("SubjectDistance", "2.5"), "2.5 m");
        assert_eq!(format_with_unit("GPSAltitude", "117"), "117 m");
        assert_eq!(format_with_unit("HyperfocalDistance", "5.2"), "5.2 m");

        // With group prefix
        assert_eq!(format_with_unit("EXIF:GPSAltitude", "0"), "0 m");
        assert_eq!(format_with_unit("EXIF:SubjectDistance", "10"), "10 m");
    }

    #[test]
    fn test_no_suffix_for_other_tags() {
        // Tags that don't need any suffix should remain unchanged
        assert_eq!(format_with_unit("ISO", "400"), "400");
        assert_eq!(format_with_unit("ImageWidth", "1920"), "1920");
        assert_eq!(format_with_unit("EXIF:Model", "Canon"), "Canon");
        assert_eq!(format_with_unit("Make", "Nikon"), "Nikon");
        assert_eq!(format_with_unit("Orientation", "1"), "1");

        // ExposureTime doesn't add "s" in our current implementation
        assert_eq!(format_with_unit("ExposureTime", "1/125"), "1/125");
        assert_eq!(format_with_unit("ShutterSpeedValue", "1/250"), "1/250");
    }

    #[test]
    fn test_needs_unit_suffix() {
        // Focal length tags need suffix
        assert!(needs_unit_suffix("FocalLength"));
        assert!(needs_unit_suffix("FocalLengthIn35mmFormat"));
        assert!(needs_unit_suffix("FocalLength35efl"));
        assert!(needs_unit_suffix("FocalLengthIn35mmFilm"));

        // Distance/altitude tags need suffix
        assert!(needs_unit_suffix("SubjectDistance"));
        assert!(needs_unit_suffix("GPSAltitude"));
        assert!(needs_unit_suffix("HyperfocalDistance"));

        // With group prefix - should still work
        assert!(needs_unit_suffix("EXIF:FocalLength"));
        assert!(needs_unit_suffix("EXIF:SubjectDistance"));
        assert!(needs_unit_suffix("GPS:GPSAltitude"));

        // Tags that don't need suffix
        assert!(!needs_unit_suffix("ISO"));
        assert!(!needs_unit_suffix("Model"));
        assert!(!needs_unit_suffix("ImageWidth"));
        assert!(!needs_unit_suffix("ExposureTime")); // Not in mm or m suffix list
        assert!(!needs_unit_suffix(""));
    }

    #[test]
    fn test_suffix_tags_lists() {
        // Verify MM_SUFFIX_TAGS contains expected entries
        assert!(MM_SUFFIX_TAGS.contains(&"FocalLength"));
        assert!(MM_SUFFIX_TAGS.contains(&"FocalLengthIn35mmFormat"));
        assert!(MM_SUFFIX_TAGS.contains(&"FocalLength35efl"));
        assert!(MM_SUFFIX_TAGS.contains(&"FocalLengthIn35mmFilm"));
        assert_eq!(MM_SUFFIX_TAGS.len(), 4);

        // Verify METER_SUFFIX_TAGS contains expected entries
        assert!(METER_SUFFIX_TAGS.contains(&"SubjectDistance"));
        assert!(METER_SUFFIX_TAGS.contains(&"GPSAltitude"));
        assert!(METER_SUFFIX_TAGS.contains(&"HyperfocalDistance"));
        assert_eq!(METER_SUFFIX_TAGS.len(), 3);

        // Verify SECONDS_SUFFIX_TAGS contains expected entries
        assert!(SECONDS_SUFFIX_TAGS.contains(&"ExposureTime"));
        assert!(SECONDS_SUFFIX_TAGS.contains(&"ShutterSpeedValue"));
        assert_eq!(SECONDS_SUFFIX_TAGS.len(), 2);
    }

    #[test]
    fn test_exif_date_formatting() {
        // Basic EXIF date (no timezone preserved)
        assert_eq!(
            format_date_exif_style("2001-05-19T18:36:41+00:00", false),
            "2001:05:19 18:36:41"
        );

        // With timezone stripped
        assert_eq!(
            format_date_exif_style("2024-12-07T10:30:00-08:00", false),
            "2024:12:07 10:30:00"
        );

        // ISO date without timezone
        assert_eq!(
            format_date_exif_style("2020-06-15T14:22:33", false),
            "2020:06:15 14:22:33"
        );
    }

    #[test]
    fn test_xmp_date_formatting_with_subseconds() {
        // XMP date with subseconds and timezone preserved
        assert_eq!(
            format_date_exif_style("2003-03-03T03:33:33.333+03:00", true),
            "2003:03:03 03:33:33.333+03:00"
        );

        // XMP date with longer subseconds
        assert_eq!(
            format_date_exif_style("2023-11-25T12:34:56.123456+05:30", true),
            "2023:11:25 12:34:56.123456+05:30"
        );

        // XMP date with negative timezone
        assert_eq!(
            format_date_exif_style("2022-08-10T09:15:00.5-07:00", true),
            "2022:08:10 09:15:00.5-07:00"
        );
    }

    #[test]
    fn test_date_formatting_passthrough() {
        // Non-ISO format should pass through unchanged
        assert_eq!(
            format_date_exif_style("2001:05:19 18:36:41", false),
            "2001:05:19 18:36:41"
        );

        // Already formatted EXIF date
        assert_eq!(
            format_date_exif_style("2024:12:07 10:30:00", false),
            "2024:12:07 10:30:00"
        );

        // Short strings should pass through
        assert_eq!(format_date_exif_style("invalid", false), "invalid");
        assert_eq!(format_date_exif_style("", false), "");
        assert_eq!(format_date_exif_style("2001-05-19", false), "2001-05-19");
    }

    #[test]
    fn test_date_formatting_utc_indicator() {
        // Z timezone indicator should be stripped (ExifTool doesn't include Z)
        assert_eq!(
            format_date_exif_style("2021-01-01T00:00:00Z", false),
            "2021:01:01 00:00:00"
        );

        // With preserve_timezone, Z should still be stripped
        assert_eq!(
            format_date_exif_style("2021-01-01T00:00:00Z", true),
            "2021:01:01 00:00:00"
        );
    }

    #[test]
    fn test_date_tags_lists() {
        // Verify EXIF_DATE_TAGS contains expected entries
        assert!(EXIF_DATE_TAGS.contains(&"CreateDate"));
        assert!(EXIF_DATE_TAGS.contains(&"DateTimeOriginal"));
        assert!(EXIF_DATE_TAGS.contains(&"ModifyDate"));
        assert!(EXIF_DATE_TAGS.contains(&"DateTimeDigitized"));
        assert!(EXIF_DATE_TAGS.contains(&"DateTime"));
        assert!(EXIF_DATE_TAGS.contains(&"DateTimeCreated"));
        assert!(EXIF_DATE_TAGS.contains(&"GPSDateStamp"));
        assert_eq!(EXIF_DATE_TAGS.len(), 7);

        // Verify XMP_DATE_TAGS contains expected entries
        assert!(XMP_DATE_TAGS.contains(&"XMP:ModifyDate"));
        assert!(XMP_DATE_TAGS.contains(&"XMP:CreateDate"));
        assert!(XMP_DATE_TAGS.contains(&"XMP:MetadataDate"));
        assert_eq!(XMP_DATE_TAGS.len(), 3);
    }
}
