//! Cross-format timestamp conversion utilities.
//!
//! Provides conversions from various platform-specific timestamp formats
//! to ISO 8601 strings and Unix timestamps.

/// Seconds between Unix epoch (1970-01-01) and Windows FILETIME epoch (1601-01-01)
const FILETIME_UNIX_DIFF: i64 = 11_644_473_600;

/// Seconds between Unix epoch (1970-01-01) and Mac/QuickTime epoch (1904-01-01)
const MAC_UNIX_DIFF: i64 = 2_082_844_800;

/// Converts Windows FILETIME to ISO 8601 string.
///
/// FILETIME is the number of 100-nanosecond intervals since 1601-01-01 00:00:00 UTC.
///
/// # Example
///
/// ```
/// use oxidex::io::timestamp::filetime_to_iso8601;
///
/// // 2024-01-15 12:30:00 UTC as FILETIME
/// let filetime = 133500402000000000u64;
/// let iso = filetime_to_iso8601(filetime);
/// assert!(iso.is_some());
/// ```
pub fn filetime_to_iso8601(filetime: u64) -> Option<String> {
    let unix = filetime_to_unix(filetime)?;
    Some(unix_to_iso8601(unix))
}

/// Converts Windows FILETIME to Unix timestamp (seconds since 1970-01-01).
///
/// Returns `None` if the FILETIME represents a date before 1970-01-01.
pub fn filetime_to_unix(filetime: u64) -> Option<i64> {
    // Convert 100-nanosecond intervals to seconds
    let seconds_since_1601 = (filetime / 10_000_000) as i64;
    let unix_time = seconds_since_1601 - FILETIME_UNIX_DIFF;

    // Return None for dates before Unix epoch
    if unix_time < 0 {
        return None;
    }

    Some(unix_time)
}

/// Converts Mac/QuickTime timestamp to ISO 8601 string.
///
/// Mac time is the number of seconds since 1904-01-01 00:00:00 UTC.
/// Used in QuickTime/MP4 files, HFS/HFS+ filesystems, and classic Mac OS.
///
/// # Example
///
/// ```
/// use oxidex::io::timestamp::mac_time_to_iso8601;
///
/// // Some time after 1970
/// let mac_time = 3_600_000_000u64;
/// let iso = mac_time_to_iso8601(mac_time);
/// assert!(iso.is_some());
/// ```
pub fn mac_time_to_iso8601(mac_time: u64) -> Option<String> {
    let unix = mac_time_to_unix(mac_time)?;
    Some(unix_to_iso8601(unix))
}

/// Converts Mac/QuickTime timestamp to Unix timestamp.
///
/// Returns `None` if the Mac time represents a date before 1970-01-01.
pub fn mac_time_to_unix(mac_time: u64) -> Option<i64> {
    let unix_time = (mac_time as i64) - MAC_UNIX_DIFF;

    // Return None for dates before Unix epoch
    if unix_time < 0 {
        return None;
    }

    Some(unix_time)
}

/// Converts Unix timestamp to ISO 8601 string.
///
/// # Example
///
/// ```
/// use oxidex::io::timestamp::unix_to_iso8601;
///
/// let iso = unix_to_iso8601(0);
/// assert_eq!(iso, "1970-01-01T00:00:00Z");
///
/// let iso = unix_to_iso8601(1705320000);
/// assert_eq!(iso, "2024-01-15T12:00:00Z");
/// ```
pub fn unix_to_iso8601(unix_time: i64) -> String {
    let (year, month, day, hour, minute, second) = unix_to_datetime(unix_time);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

/// Converts Unix timestamp to date/time components.
/// Returns (year, month, day, hour, minute, second).
fn unix_to_datetime(unix_time: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Days since Unix epoch
    let days = unix_time / 86400;
    let time_of_day = (unix_time % 86400) as u32;

    // Time components
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    // Convert days to date using a simplified algorithm
    // Based on the algorithms from Howard Hinnant's date library
    let z = days + 719468; // Days since 0000-03-01
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32; // Day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // Year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // Day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // Month prime [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // Day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // Month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    (y as i32, m, d, hour, minute, second)
}

/// Parses EXIF-format datetime string to Unix timestamp.
///
/// EXIF datetime format: "YYYY:MM:DD HH:MM:SS"
///
/// # Example
///
/// ```
/// use oxidex::io::timestamp::exif_datetime_to_unix;
///
/// let unix = exif_datetime_to_unix("2024:01:15 14:30:00");
/// assert!(unix.is_some());
/// ```
pub fn exif_datetime_to_unix(datetime: &str) -> Option<i64> {
    // Expected format: "YYYY:MM:DD HH:MM:SS"
    if datetime.len() < 19 {
        return None;
    }

    let bytes = datetime.as_bytes();

    // Validate separators: colons at positions 4, 7, 13, 16; space at 10
    if bytes[4] != b':'
        || bytes[7] != b':'
        || bytes[10] != b' '
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }

    let year: i32 = datetime.get(0..4)?.parse().ok()?;
    let month: u32 = datetime.get(5..7)?.parse().ok()?;
    let day: u32 = datetime.get(8..10)?.parse().ok()?;
    let hour: u32 = datetime.get(11..13)?.parse().ok()?;
    let minute: u32 = datetime.get(14..16)?.parse().ok()?;
    let second: u32 = datetime.get(17..19)?.parse().ok()?;

    datetime_to_unix(year, month, day, hour, minute, second)
}

/// Converts date/time components to Unix timestamp.
fn datetime_to_unix(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<i64> {
    // Validate ranges
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    // Algorithm from Howard Hinnant's date library
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = if month <= 2 { month + 12 } else { month } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (m - 3) + 2) / 5 + day as i64 - 1;
    let doe = yoe as i64 * 365 + yoe as i64 / 4 - yoe as i64 / 100 + doy;
    let days = era * 146097 + doe - 719468;

    Some(days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unix_to_iso8601() {
        assert_eq!(unix_to_iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(unix_to_iso8601(86400), "1970-01-02T00:00:00Z");
        // 2024-01-15 12:00:00 UTC = 1705320000
        assert_eq!(unix_to_iso8601(1705320000), "2024-01-15T12:00:00Z");
    }

    #[test]
    fn test_filetime_to_unix() {
        // 1970-01-01 00:00:00 as FILETIME
        let filetime_1970 = 116444736000000000u64;
        assert_eq!(filetime_to_unix(filetime_1970), Some(0));

        // Before Unix epoch
        assert_eq!(filetime_to_unix(0), None);
    }

    #[test]
    fn test_filetime_to_iso8601() {
        let filetime_1970 = 116444736000000000u64;
        assert_eq!(
            filetime_to_iso8601(filetime_1970),
            Some("1970-01-01T00:00:00Z".to_string())
        );
    }

    #[test]
    fn test_mac_time_to_unix() {
        // 1970-01-01 as Mac time
        let mac_1970 = MAC_UNIX_DIFF as u64;
        assert_eq!(mac_time_to_unix(mac_1970), Some(0));

        // Before Unix epoch
        assert_eq!(mac_time_to_unix(0), None);
    }

    #[test]
    fn test_mac_time_to_iso8601() {
        let mac_1970 = MAC_UNIX_DIFF as u64;
        assert_eq!(
            mac_time_to_iso8601(mac_1970),
            Some("1970-01-01T00:00:00Z".to_string())
        );
    }

    #[test]
    fn test_exif_datetime_to_unix() {
        // Verify roundtrip: parse and convert back
        let unix = exif_datetime_to_unix("2024:01:15 12:00:00");
        assert!(unix.is_some());
        assert_eq!(unix_to_iso8601(unix.unwrap()), "2024-01-15T12:00:00Z");

        assert_eq!(exif_datetime_to_unix("1970:01:01 00:00:00"), Some(0));

        // Invalid formats
        assert_eq!(exif_datetime_to_unix("invalid"), None);
        assert_eq!(exif_datetime_to_unix("2024-01-15 12:00:00"), None); // Wrong separator
    }

    #[test]
    fn test_unix_to_datetime_roundtrip() {
        // Test various dates roundtrip through the conversion
        let test_cases = [
            (0, (1970, 1, 1, 0, 0, 0)),
            (86400, (1970, 1, 2, 0, 0, 0)),
            (1705320000, (2024, 1, 15, 12, 0, 0)), // 2024-01-15 12:00:00 UTC
        ];

        for (unix, expected) in test_cases {
            let result = unix_to_datetime(unix);
            assert_eq!(result, expected, "Failed for unix={}", unix);
        }
    }

    #[test]
    fn test_filetime_various_dates() {
        // Test that a recent FILETIME converts to a reasonable date
        // 2020-01-01 00:00:00 UTC = Unix 1577836800
        // FILETIME = (1577836800 + 11644473600) * 10_000_000
        let unix_2020 = 1577836800i64;
        let filetime_2020 = ((unix_2020 + FILETIME_UNIX_DIFF) * 10_000_000) as u64;
        let result = filetime_to_iso8601(filetime_2020);
        assert!(result.is_some());
        assert!(result.unwrap().starts_with("2020-01-01"));
    }

    #[test]
    fn test_mac_time_various_dates() {
        // 2020-01-01 00:00:00 as Mac time (seconds since 1904-01-01)
        let mac_2020 = 3_660_595_200u64; // Approximate
        let result = mac_time_to_iso8601(mac_2020);
        assert!(result.is_some());
    }

    #[test]
    fn test_exif_datetime_edge_cases() {
        // Minimum valid date
        assert!(exif_datetime_to_unix("1970:01:01 00:00:00").is_some());

        // Invalid month
        assert_eq!(exif_datetime_to_unix("2024:13:01 00:00:00"), None);

        // Invalid day
        assert_eq!(exif_datetime_to_unix("2024:01:32 00:00:00"), None);

        // Invalid hour
        assert_eq!(exif_datetime_to_unix("2024:01:01 25:00:00"), None);

        // Too short
        assert_eq!(exif_datetime_to_unix("2024:01:01"), None);
    }

    #[test]
    fn test_leap_year_handling() {
        // Feb 29 on leap year (2024)
        let unix = exif_datetime_to_unix("2024:02:29 12:00:00");
        assert!(unix.is_some());

        // Verify the date converts back correctly
        if let Some(ts) = unix {
            let iso = unix_to_iso8601(ts);
            assert!(iso.contains("2024-02-29"));
        }
    }

    #[test]
    fn test_end_of_year() {
        // Dec 31, 23:59:59
        let unix = exif_datetime_to_unix("2024:12:31 23:59:59");
        assert!(unix.is_some());

        if let Some(ts) = unix {
            let iso = unix_to_iso8601(ts);
            assert!(iso.contains("2024-12-31"));
            assert!(iso.contains("23:59:59"));
        }
    }

    #[test]
    fn test_datetime_to_unix_invalid_ranges() {
        // Year before 1970
        assert_eq!(super::datetime_to_unix(1969, 12, 31, 23, 59, 59), None);

        // Month 0
        assert_eq!(super::datetime_to_unix(2024, 0, 1, 0, 0, 0), None);

        // Day 0
        assert_eq!(super::datetime_to_unix(2024, 1, 0, 0, 0, 0), None);
    }
}
