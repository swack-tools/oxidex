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

/// Format file size like ExifTool (e.g., "2.1 kB", "12 kB", "1500 bytes").
///
/// ExifTool uses decimal (base-10) units, not binary (base-2) units:
/// 1 kB = 1000 bytes, 1 MB = 1,000,000 bytes, 1 GB = 1,000,000,000 bytes.
///
/// The unit does **not** change at each power of 1000. ExifTool holds each unit
/// until the value reaches 2000 of it, and only prints a decimal place while the
/// value is under 10 of that unit. So 1500 bytes stays "1500 bytes", 10,000
/// bytes is "10 kB" (no decimal), and 1,000,000 bytes is "1000 kB" rather than
/// "1.0 MB". Ported verbatim from the `ByteUnit ne 'Binary'` branch of
/// `ConvertFileSize` in ExifTool.pm (lib/Image/ExifTool.pm:6863-6869):
///
/// ```text
/// $val < 2000 and return "$val bytes";
/// $val < 10000 and return sprintf('%.1f kB', $val / 1000);
/// $val < 2000000 and return sprintf('%.0f kB', $val / 1000);
/// $val < 10000000 and return sprintf('%.1f MB', $val / 1000000);
/// $val < 2000000000 and return sprintf('%.0f MB', $val / 1000000);
/// $val < 10000000000 and return sprintf('%.1f GB', $val / 1000000000);
/// return sprintf('%.0f GB', $val / 1000000000);
/// ```
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_file_size;
///
/// assert_eq!(format_file_size(500), "500 bytes");
/// assert_eq!(format_file_size(1500), "1500 bytes");
/// assert_eq!(format_file_size(2100), "2.1 kB");
/// assert_eq!(format_file_size(12_379), "12 kB");
/// assert_eq!(format_file_size(1_500_000), "1500 kB");
/// assert_eq!(format_file_size(2_500_000_000), "2.5 GB");
/// ```
pub fn format_file_size(bytes: u64) -> String {
    // One implementation of the branch chain above, shared with the
    // generated tables' `PrintConv` for `Palm::MOBI` UncompressedTextLength
    // (`Palm.pm:121-124`, `PrintConv => \&Image::ExifTool::ConvertFileSize`).
    // Two copies of a conversion drift silently; this one is the port that
    // `tools/exiftool-tables/verify_exprs.py` checks against the pinned
    // Perl, so it is the one that stays.
    crate::exiftool_tables::exprs::convert_file_size(bytes as f64)
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
/// // Non-ASCII input of exactly 8 *bytes* is passed through, not sliced
/// assert_eq!(format_iptc_date("日本ab"), "日本ab");
/// ```
pub fn format_iptc_date(raw: &str) -> String {
    // `len()` is a byte count, so an 8-byte non-ASCII value ("日本ab" is 8
    // bytes) reaches the slices below with index 4 landing inside a multi-byte
    // sequence, and panics. A real IPTC date is ASCII digits, so anything else
    // is invalid input and is passed through unchanged, exactly like the
    // "invalid" case above.
    if raw.len() == 8 && raw.is_ascii() {
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
/// // Non-ASCII input is passed through, not sliced by byte index
/// assert_eq!(format_iptc_time("日本語"), "日本語");
/// ```
pub fn format_iptc_time(raw: &str) -> String {
    // See `format_iptc_date`: the length checks below are byte counts, so a
    // non-ASCII value long enough to pass them would be sliced at 2/4/6/7/9/11
    // and panic mid-character. Real IPTC times are ASCII.
    if raw.len() >= 6 && raw.is_ascii() {
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
/// This is ExifTool's shared PrintConv table for `IPTC:Urgency` (2:10) and
/// `IPTC:EnvelopePriority` (1:60) -- IPTC.pm gives both the identical hash, so
/// both print the same way. Digits without a description print bare, and a
/// value outside the table falls through unchanged.
///
/// # Examples
///
/// ```
/// use oxidex::core::value_formatter::format_iptc_urgency;
///
/// assert_eq!(format_iptc_urgency("0"), "0 (reserved)");
/// assert_eq!(format_iptc_urgency("1"), "1 (most urgent)");
/// assert_eq!(format_iptc_urgency("5"), "5 (normal urgency)");
/// assert_eq!(format_iptc_urgency("8"), "8 (least urgent)");
/// assert_eq!(format_iptc_urgency("9"), "9 (user-defined priority)");
/// assert_eq!(format_iptc_urgency("3"), "3");
/// assert_eq!(format_iptc_urgency("invalid"), "invalid"); // Preserves invalid input
/// ```
pub fn format_iptc_urgency(raw: &str) -> String {
    match raw.trim() {
        "0" => "0 (reserved)".to_string(),
        "1" => "1 (most urgent)".to_string(),
        "5" => "5 (normal urgency)".to_string(),
        "8" => "8 (least urgent)".to_string(),
        "9" => "9 (user-defined priority)".to_string(),
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
/// assert_eq!(format_iptc_coded_charset(b"other"), "o t h e r");
/// // Unrecognised escape sequences are spelled out the way ExifTool's
/// // PrintCodedCharset does (IPTC.pm:980-988).
/// assert_eq!(
///     format_iptc_coded_charset(&[0x1B, 0x28, 0x42, 0x1B, 0x26, 0x40]),
///     "ESC ( B, ESC & @"
/// );
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
    // Fallback: ExifTool's PrintCodedCharset (IPTC.pm:980-988) spells the raw
    // ISO 2022 sequence out rather than dropping it -- every byte gets a
    // leading space, each ESC-introduced run becomes ", ESC", and the leading
    // separator is trimmed. `1b 28 42 1b 26 40` therefore prints as
    // "ESC ( B, ESC & @".
    let mut spaced = String::with_capacity(data.len() * 2);
    for &byte in data {
        if byte == 0x1B {
            spaced.push_str(", ESC");
        } else {
            spaced.push(' ');
            spaced.push(byte as char);
        }
    }
    spaced
        .strip_prefix(", ")
        .or_else(|| spaced.strip_prefix(' '))
        .unwrap_or(&spaced)
        .to_string()
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
    // - The whole date/time head must be ASCII, so that the byte indices used
    //   below are all char boundaries. The separator checks alone are not
    //   enough: they pin bytes 4/7/10/13/16, but `&iso_date[17..19]` and
    //   `&iso_date[19..]` would still cut through a multi-byte character
    //   starting at byte 17 (e.g. "0000-00-00T00:00:€") and panic.
    if bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || !bytes[..19].is_ascii()
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

// `format_with_unit` / `needs_unit_suffix` and their MM/METER/SECONDS tables
// used to live here as a second, divergent implementation. The shipped one is
// `crate::core::formatters::unit_suffixes`, which `exiftool_compat` has always
// called; this copy never applied EXIF 0x920a's `sprintf("%.1f mm")`
// (Exif.pm:2401) and appended units unconditionally, so it doubled the suffix
// on any value that already carried one. Its tests asserted that output.

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

    /// Every expectation below was read back from `exiftool -s -FileSize` on a
    /// file of exactly that byte length (ExifTool 13.55), not derived from the
    /// Perl by hand. The interesting property is that the unit steps at 2000 of
    /// the unit -- not at 1000 -- and drops the decimal place above 10 of it.
    #[test]
    fn test_file_size_formatting_matches_exiftool() {
        // Held as raw bytes right up to 2000 (ExifTool.pm:6863).
        assert_eq!(format_file_size(0), "0 bytes");
        assert_eq!(format_file_size(1), "1 bytes");
        assert_eq!(format_file_size(500), "500 bytes");
        assert_eq!(format_file_size(999), "999 bytes");
        assert_eq!(format_file_size(1000), "1000 bytes");
        assert_eq!(format_file_size(1500), "1500 bytes");
        assert_eq!(format_file_size(1999), "1999 bytes");

        // 2000..10000: one decimal place (ExifTool.pm:6864).
        assert_eq!(format_file_size(2000), "2.0 kB");
        assert_eq!(format_file_size(2100), "2.1 kB");
        assert_eq!(format_file_size(9999), "10.0 kB");

        // 10000..2e6: kB with no decimal place (ExifTool.pm:6865).
        assert_eq!(format_file_size(10_000), "10 kB");
        assert_eq!(format_file_size(12_379), "12 kB");
        assert_eq!(format_file_size(999_999), "1000 kB");
        assert_eq!(format_file_size(1_000_000), "1000 kB");
        assert_eq!(format_file_size(1_999_999), "2000 kB");

        // 2e6..1e7: MB with one decimal place (ExifTool.pm:6866).
        assert_eq!(format_file_size(2_000_000), "2.0 MB");
        assert_eq!(format_file_size(2_500_000), "2.5 MB");
        assert_eq!(format_file_size(9_999_999), "10.0 MB");

        // 1e7..2e9: MB with no decimal place (ExifTool.pm:6867).
        assert_eq!(format_file_size(10_000_000), "10 MB");
        assert_eq!(format_file_size(1_000_000_000), "1000 MB");
        assert_eq!(format_file_size(1_999_999_999), "2000 MB");

        // 2e9..1e10: GB with one decimal place (ExifTool.pm:6868).
        assert_eq!(format_file_size(2_000_000_000), "2.0 GB");
        assert_eq!(format_file_size(2_500_000_000), "2.5 GB");
        assert_eq!(format_file_size(9_999_999_999), "10.0 GB");

        // >= 1e10: GB with no decimal place (ExifTool.pm:6869).
        assert_eq!(format_file_size(10_000_000_000), "10 GB");
        assert_eq!(format_file_size(12_500_000_000), "12 GB");
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

    /// Regression: these three formatters index by *byte* behind *byte*-length
    /// guards, so non-ASCII input long enough to pass the guard used to panic
    /// with "byte index N is not a char boundary". Metadata text is routinely
    /// non-ASCII (CJK filenames and descriptions, lossy-decoded MakerNotes), so
    /// each case below uses genuinely multi-byte text -- an ASCII string of the
    /// same length takes the identical path and cannot reproduce the bug.
    #[test]
    fn test_date_formatters_reject_non_ascii_instead_of_splitting_chars() {
        // 8 bytes, but index 4 is inside the second 3-byte character.
        let date = "日本ab";
        assert_eq!(date.len(), 8);
        assert!(!date.is_char_boundary(4));
        assert_eq!(format_iptc_date(date), date);

        // 9 bytes (passes `len() >= 6`), index 2 is inside the first character.
        let time = "日本語";
        assert!(time.len() >= 6);
        assert!(!time.is_char_boundary(2));
        assert_eq!(format_iptc_time(time), time);

        // 11+ bytes so the timezone branch is reached too.
        let time_tz = "日本語かなカナ";
        assert!(time_tz.len() >= 11);
        assert_eq!(format_iptc_time(time_tz), time_tz);

        // Passes every ISO-8601 separator check (bytes 4/7/10/13/16), yet byte
        // 19 is inside the 3-byte '€' that starts at byte 17.
        let iso = "0000-00-00T00:00:\u{20ac}";
        assert!(iso.len() >= 19);
        assert_eq!(iso.as_bytes()[10], b'T');
        assert!(!iso.is_char_boundary(19));
        assert_eq!(format_date_exif_style(iso, false), iso);
        assert_eq!(format_date_exif_style(iso, true), iso);
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
