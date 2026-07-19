//! Date/Time shifting operations for metadata tags
//!
//! This module implements date shifting functionality compatible with ExifTool syntax:
//! - Add offset: `-AllDates+=1:2:3 4:5:6` (add 1 year, 2 months, 3 days, 4 hours, 5 minutes, 6 seconds)
//! - Subtract offset: `-EXIF:DateTime-=0:0:5 0:0:0` (subtract 5 days)
//! - Set absolute: `-EXIF:DateTime=2025:01:15 10:30:00` (set to specific date/time)

use super::operations::{read_metadata, write_metadata};
use super::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use chrono::{DateTime, Duration, Months, NaiveDateTime, Utc};
use std::path::Path;

/// Operation type for date shifting
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShiftOperation {
    /// Add offset to existing date/time
    Add,
    /// Subtract offset from existing date/time
    Subtract,
    /// Set absolute date/time value
    Set,
}

/// Represents a date/time offset with years, months, days, hours, minutes, seconds
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DateOffset {
    /// Number of years in the offset
    pub years: u32,
    /// Number of months in the offset
    pub months: u32,
    /// Number of days in the offset
    pub days: i64,
    /// Number of hours in the offset
    pub hours: i64,
    /// Number of minutes in the offset
    pub minutes: i64,
    /// Number of seconds in the offset
    pub seconds: i64,
}

impl DateOffset {
    /// Creates a new DateOffset with all fields set to zero
    pub fn zero() -> Self {
        Self {
            years: 0,
            months: 0,
            days: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }

    /// Creates a DateOffset from parsed components
    pub fn new(years: u32, months: u32, days: i64, hours: i64, minutes: i64, seconds: i64) -> Self {
        Self {
            years,
            months,
            days,
            hours,
            minutes,
            seconds,
        }
    }
}

/// EXIF date/time tags that oxidex can shift in place.
///
/// These are exactly the three tags ExifTool's "AllDates" shortcut covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifDateTag {
    /// IFD0 tag 0x0132 — ExifTool name "ModifyDate" (EXIF spec: "DateTime")
    ModifyDate,
    /// ExifIFD tag 0x9003 — "DateTimeOriginal"
    DateTimeOriginal,
    /// ExifIFD tag 0x9004 — ExifTool name "CreateDate" (EXIF spec: "DateTimeDigitized")
    CreateDate,
}

impl ExifDateTag {
    /// The EXIF/TIFF tag ID.
    pub fn tag_id(self) -> u16 {
        match self {
            ExifDateTag::ModifyDate => 0x0132,
            ExifDateTag::DateTimeOriginal => 0x9003,
            ExifDateTag::CreateDate => 0x9004,
        }
    }

    /// The group-prefixed key oxidex uses for this tag in a MetadataMap.
    pub fn key(self) -> &'static str {
        match self {
            ExifDateTag::ModifyDate => "IFD0:ModifyDate",
            ExifDateTag::DateTimeOriginal => "ExifIFD:DateTimeOriginal",
            ExifDateTag::CreateDate => "ExifIFD:DateTimeDigitized",
        }
    }
}

/// Resolves a user-supplied tag pattern to the EXIF date tags it names.
///
/// Accepts ExifTool conventions: bare names ("DateTimeOriginal"), name
/// aliases ("DateTime" for ModifyDate, "DateTimeDigitized" for CreateDate),
/// the "EXIF:" family, oxidex's internal groups ("IFD0:", "ExifIFD:"), and
/// the "AllDates" shortcut. Matching is ASCII case-insensitive.
///
/// Returns `None` when the pattern does not name a known EXIF date/time tag.
pub fn resolve_exif_targets(pattern: &str) -> Option<Vec<ExifDateTag>> {
    let lowered = pattern.to_ascii_lowercase();
    if lowered == "alldates" {
        return Some(vec![
            ExifDateTag::ModifyDate,
            ExifDateTag::DateTimeOriginal,
            ExifDateTag::CreateDate,
        ]);
    }

    let (family, name) = match lowered.split_once(':') {
        Some((f, n)) => (Some(f), n),
        None => (None, lowered.as_str()),
    };

    let tag = match name {
        "modifydate" | "datetime" => ExifDateTag::ModifyDate,
        "datetimeoriginal" => ExifDateTag::DateTimeOriginal,
        "createdate" | "datetimedigitized" => ExifDateTag::CreateDate,
        _ => return None,
    };

    let family_ok = match family {
        None => true,
        Some("exif") => true,
        Some("ifd0") => tag == ExifDateTag::ModifyDate,
        Some("exififd") => tag != ExifDateTag::ModifyDate,
        Some(_) => false,
    };
    if family_ok { Some(vec![tag]) } else { None }
}

/// A fully parsed shift request: a relative offset or an absolute value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShiftSpec {
    /// Add or subtract a relative offset. `op` is only ever Add or Subtract.
    Relative {
        /// The parsed offset amount
        offset: DateOffset,
        /// The effective direction after folding in any leading sign
        op: ShiftOperation,
    },
    /// Set to an absolute date/time (the `=` operation).
    Absolute(DateTime<Utc>),
}

/// Builds a ShiftSpec from an operation and its argument string, folding a
/// leading `-` on the shift string into the operation direction.
pub fn build_shift_spec(offset_or_value: &str, op: ShiftOperation) -> Result<ShiftSpec> {
    if op == ShiftOperation::Set {
        return Ok(ShiftSpec::Absolute(parse_absolute_datetime(
            offset_or_value,
        )?));
    }
    let (offset, negated) = parse_offset(offset_or_value)?;
    let effective = match (op, negated) {
        (ShiftOperation::Add, true) => ShiftOperation::Subtract,
        (ShiftOperation::Subtract, true) => ShiftOperation::Add,
        (other, _) => other,
    };
    Ok(ShiftSpec::Relative {
        offset,
        op: effective,
    })
}

/// Applies a ShiftSpec to a date/time value.
pub fn apply_spec(dt: DateTime<Utc>, spec: &ShiftSpec) -> Result<DateTime<Utc>> {
    match spec {
        ShiftSpec::Absolute(value) => Ok(*value),
        ShiftSpec::Relative { offset, op } => apply_shift(dt, offset, *op),
    }
}

/// Parses an ExifTool-style shift string.
///
/// Grammar (matches `Image::ExifTool::Shift.pl`, verified against ExifTool 13.55):
/// - Optional leading `+` or `-`; `-` negates the whole shift (the returned
///   bool is `true`), which callers apply by flipping Add and Subtract.
/// - One or two space-separated parts, each 1-3 colon-separated non-negative
///   integers.
/// - Two parts are `DATE TIME`. DATE is right-justified: `D`, `M:D`, or
///   `Y:M:D`. TIME is left-justified: `H`, `H:M`, or `H:M:S`.
/// - A single part is a TIME shift (`H`, `H:M`, or `H:M:S`) because every tag
///   this module shifts is a full date-time value.
///
/// # Examples
///
/// - `"1:00:00"` -> 1 hour
/// - `"1:30"` -> 1 hour 30 minutes
/// - `"0:0:1 0:0:0"` -> 1 day
/// - `"1:2 3"` -> 1 month, 2 days, 3 hours
/// - `"-1"` -> 1 hour, negated
pub fn parse_offset(s: &str) -> Result<(DateOffset, bool)> {
    let invalid = || {
        ExifToolError::parse_error(format!(
            "Invalid shift string '{}': expected 'TIME' or 'DATE TIME' with 1-3 \
             numbers per part (e.g., '1:30' for 1.5 hours, '0:0:1 12' for 1 day 12 hours)",
            s
        ))
    };

    let trimmed = s.trim();
    let (negated, rest) = match trimmed.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };

    let parts: Vec<&str> = rest.split_whitespace().collect();
    let (date_part, time_part) = match parts.as_slice() {
        [time] => (None, *time),
        [date, time] => (Some(*date), *time),
        _ => return Err(invalid()),
    };

    let parse_components = |part: &str| -> Result<Vec<u32>> {
        let fields: Vec<&str> = part.split(':').collect();
        if fields.is_empty() || fields.len() > 3 {
            return Err(invalid());
        }
        fields
            .iter()
            .map(|f| f.parse::<u32>().map_err(|_| invalid()))
            .collect()
    };

    let mut offset = DateOffset::zero();
    if let Some(date) = date_part {
        // Right-justified: the last number is always days
        let mut values = parse_components(date)?.into_iter().rev();
        offset.days = values.next().unwrap_or(0) as i64;
        offset.months = values.next().unwrap_or(0);
        offset.years = values.next().unwrap_or(0);
    }
    // Left-justified: the first number is always hours
    let mut values = parse_components(time_part)?.into_iter();
    offset.hours = values.next().unwrap_or(0) as i64;
    offset.minutes = values.next().unwrap_or(0) as i64;
    offset.seconds = values.next().unwrap_or(0) as i64;

    Ok((offset, negated))
}

/// Parses an EXIF DateTime string into a chrono::DateTime<Utc>
///
/// EXIF format: "2025:01:15 10:30:00" (YYYY:MM:DD HH:MM:SS)
pub fn parse_absolute_datetime(s: &str) -> Result<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .map_err(|e| ExifToolError::parse_error(format!("Invalid DateTime '{}': {}", s, e)))?;

    Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc))
}

/// Formats a DateTime to EXIF format string "YYYY:MM:DD HH:MM:SS"
pub fn format_exif_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y:%m:%d %H:%M:%S").to_string()
}

/// Applies a date/time shift operation to a DateTime value
///
/// # Arguments
///
/// * `dt` - The original DateTime to shift
/// * `offset` - The offset to apply
/// * `op` - The operation type (Add, Subtract, or Set)
///
/// # Returns
///
/// The shifted DateTime or an error if overflow occurs
pub fn apply_shift(
    dt: DateTime<Utc>,
    offset: &DateOffset,
    op: ShiftOperation,
) -> Result<DateTime<Utc>> {
    match op {
        ShiftOperation::Set => {
            // For Set operation, the offset is not used - the caller should parse the absolute value
            // This case should not be reached if used correctly
            Err(ExifToolError::parse_error(
                "apply_shift called with Set operation - use parse_absolute_datetime instead",
            ))
        }
        ShiftOperation::Add => {
            // Add offset to datetime
            let mut result = dt;

            // Add years and months (using chrono::Months for proper overflow handling)
            let total_months = (offset.years * 12) + offset.months;
            if total_months > 0 {
                result = result
                    .checked_add_months(Months::new(total_months))
                    .ok_or_else(|| {
                        ExifToolError::parse_error("Date overflow when adding months")
                    })?;
            }

            // Add days, hours, minutes, seconds (using chrono::Duration)
            let duration = Duration::days(offset.days)
                + Duration::hours(offset.hours)
                + Duration::minutes(offset.minutes)
                + Duration::seconds(offset.seconds);

            result = result.checked_add_signed(duration).ok_or_else(|| {
                ExifToolError::parse_error("Date overflow when adding time offset")
            })?;

            Ok(result)
        }
        ShiftOperation::Subtract => {
            // Subtract offset from datetime
            let mut result = dt;

            // Subtract years and months (using chrono::Months for proper overflow handling)
            let total_months = (offset.years * 12) + offset.months;
            if total_months > 0 {
                result = result
                    .checked_sub_months(Months::new(total_months))
                    .ok_or_else(|| {
                        ExifToolError::parse_error("Date underflow when subtracting months")
                    })?;
            }

            // Subtract days, hours, minutes, seconds (using chrono::Duration)
            let duration = Duration::days(offset.days)
                + Duration::hours(offset.hours)
                + Duration::minutes(offset.minutes)
                + Duration::seconds(offset.seconds);

            result = result.checked_sub_signed(duration).ok_or_else(|| {
                ExifToolError::parse_error("Date underflow when subtracting time offset")
            })?;

            Ok(result)
        }
    }
}

/// Common DateTime tags that are shifted when using "AllDates" pattern
const ALL_DATES_TAGS: &[&str] = &[
    "EXIF:DateTime",
    "EXIF:DateTimeOriginal",
    "EXIF:DateTimeDigitized",
    "XMP:CreateDate",
    "XMP:ModifyDate",
    "PDF:CreateDate",
    "PDF:ModifyDate",
    "QuickTime:ContentCreateDate",
    "QuickTime:CreateDate",
    "QuickTime:ModifyDate",
];

/// Shifts date/time tags in a file's metadata
///
/// # Arguments
///
/// * `path` - Path to the file to modify
/// * `tag_pattern` - Tag pattern to match ("AllDates" for all DateTime tags, or specific tag name)
/// * `offset_or_value` - Offset string in "Y:M:D H:M:S" format, or absolute datetime for Set operation
/// * `op` - Operation type (Add, Subtract, or Set)
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::date_shift::{shift_metadata_dates, ShiftOperation};
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Add 1 day to all date tags
/// shift_metadata_dates(
///     Path::new("photo.jpg"),
///     "AllDates",
///     "0:0:1 0:0:0",
///     ShiftOperation::Add
/// )?;
///
/// // Subtract 1 month from DateTimeOriginal only
/// shift_metadata_dates(
///     Path::new("photo.jpg"),
///     "EXIF:DateTimeOriginal",
///     "0:1:0 0:0:0",
///     ShiftOperation::Subtract
/// )?;
///
/// // Set DateTime to specific value
/// shift_metadata_dates(
///     Path::new("photo.jpg"),
///     "EXIF:DateTime",
///     "2025:01:15 10:30:00",
///     ShiftOperation::Set
/// )?;
/// # Ok(())
/// # }
/// ```
pub fn shift_metadata_dates(
    path: &Path,
    tag_pattern: &str,
    offset_or_value: &str,
    op: ShiftOperation,
) -> Result<()> {
    // Step 1: Read existing metadata from file
    let mut metadata = read_metadata(path)?;

    // Step 2: Parse offset or absolute value based on operation
    let (offset, op) = if op == ShiftOperation::Set {
        (None, op)
    } else {
        let (parsed, negated) = parse_offset(offset_or_value)?;
        let effective = match (op, negated) {
            (ShiftOperation::Add, true) => ShiftOperation::Subtract,
            (ShiftOperation::Subtract, true) => ShiftOperation::Add,
            (other, _) => other,
        };
        (Some(parsed), effective)
    };

    let absolute_value = if op == ShiftOperation::Set {
        Some(parse_absolute_datetime(offset_or_value)?)
    } else {
        None
    };

    // Step 3: Determine which tags to shift
    let mut modified_count = 0;

    if tag_pattern.eq_ignore_ascii_case("AllDates") {
        // Shift all DateTime tags that exist in the metadata
        for tag_name in ALL_DATES_TAGS {
            if let Some(tag_value) = metadata.get(tag_name)
                && let Some(dt) = tag_value.as_datetime()
            {
                // Apply shift
                let new_dt = if let Some(abs) = absolute_value {
                    abs
                } else {
                    apply_shift(*dt, offset.as_ref().unwrap(), op)?
                };

                // Update the tag
                metadata.insert((*tag_name).to_string(), TagValue::new_datetime(new_dt));
                modified_count += 1;
            }
        }

        // Also check for any other DateTime tags in the metadata
        let all_keys: Vec<String> = metadata.iter().map(|(k, _)| k.clone()).collect();
        for tag_name in all_keys {
            // Skip tags we already processed
            if ALL_DATES_TAGS.contains(&tag_name.as_str()) {
                continue;
            }

            if let Some(tag_value) = metadata.get(&tag_name)
                && let Some(dt) = tag_value.as_datetime()
            {
                // Apply shift
                let new_dt = if let Some(abs) = absolute_value {
                    abs
                } else {
                    apply_shift(*dt, offset.as_ref().unwrap(), op)?
                };

                // Update the tag
                metadata.insert(tag_name, TagValue::new_datetime(new_dt));
                modified_count += 1;
            }
        }
    } else {
        // Shift specific tag only
        if let Some(tag_value) = metadata.get(tag_pattern) {
            if let Some(dt) = tag_value.as_datetime() {
                // Apply shift
                let new_dt = if let Some(abs) = absolute_value {
                    abs
                } else {
                    apply_shift(*dt, offset.as_ref().unwrap(), op)?
                };

                // Update the tag
                metadata.insert(tag_pattern.to_string(), TagValue::new_datetime(new_dt));
                modified_count += 1;
            } else {
                return Err(ExifToolError::parse_error(format!(
                    "Tag '{}' is not a DateTime tag",
                    tag_pattern
                )));
            }
        } else {
            return Err(ExifToolError::parse_error(format!(
                "Tag '{}' not found in metadata",
                tag_pattern
            )));
        }
    }

    // Check if any tags were modified
    if modified_count == 0 {
        return Err(ExifToolError::parse_error(format!(
            "No DateTime tags found matching pattern '{}'",
            tag_pattern
        )));
    }

    // Step 4: Write modified metadata back to file
    write_metadata(path, &metadata)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone, Timelike};

    #[test]
    fn test_parse_offset_full_form() {
        let (offset, neg) = parse_offset("1:2:3 4:5:6").unwrap();
        assert!(!neg);
        assert_eq!(offset, DateOffset::new(1, 2, 3, 4, 5, 6));
    }

    #[test]
    fn test_parse_offset_single_number_is_hours() {
        let (offset, neg) = parse_offset("1").unwrap();
        assert!(!neg);
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 0, 0));
    }

    #[test]
    fn test_parse_offset_time_is_left_justified() {
        // ExifTool: '1:30' means 1 hour 30 minutes, NOT 1 minute 30 seconds
        let (offset, _) = parse_offset("1:30").unwrap();
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 30, 0));
    }

    #[test]
    fn test_parse_offset_three_part_time() {
        let (offset, _) = parse_offset("0:0:30").unwrap();
        assert_eq!(offset, DateOffset::new(0, 0, 0, 0, 0, 30));
    }

    #[test]
    fn test_parse_offset_issue_14_form() {
        // The exact string from GitHub issue #14
        let (offset, neg) = parse_offset("1:00:00").unwrap();
        assert!(!neg);
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 0, 0));
    }

    #[test]
    fn test_parse_offset_date_is_right_justified() {
        // ExifTool: date part '1:2' means 1 month 2 days, NOT 1 year 2 months
        let (offset, _) = parse_offset("1:2 3").unwrap();
        assert_eq!(offset, DateOffset::new(0, 1, 2, 3, 0, 0));
    }

    #[test]
    fn test_parse_offset_two_arg_full_date() {
        let (offset, _) = parse_offset("1:0:0 0:0:0").unwrap();
        assert_eq!(offset, DateOffset::new(1, 0, 0, 0, 0, 0));
    }

    #[test]
    fn test_parse_offset_leading_minus_sets_negated() {
        let (offset, neg) = parse_offset("-1").unwrap();
        assert!(neg);
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 0, 0));
    }

    #[test]
    fn test_parse_offset_leading_plus_ignored() {
        let (offset, neg) = parse_offset("+1:30").unwrap();
        assert!(!neg);
        assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 30, 0));
    }

    #[test]
    fn test_parse_offset_invalid() {
        assert!(parse_offset("").is_err());
        assert!(parse_offset("1:2:3:4").is_err()); // too many numbers in one part
        assert!(parse_offset("1:2:3:4:5:6").is_err());
        assert!(parse_offset("1:2:3 4:5:6 7").is_err()); // three parts
        assert!(parse_offset("abc").is_err());
        assert!(parse_offset("1:2:3 4:x").is_err());
        assert!(parse_offset("1:").is_err()); // empty component
    }

    #[test]
    fn test_parse_absolute_datetime_valid() {
        let dt = parse_absolute_datetime("2025:01:15 10:30:00").unwrap();
        assert_eq!(dt.year(), 2025);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 15);
        assert_eq!(dt.hour(), 10);
        assert_eq!(dt.minute(), 30);
        assert_eq!(dt.second(), 0);
    }

    #[test]
    fn test_parse_absolute_datetime_invalid() {
        let result = parse_absolute_datetime("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_format_exif_datetime() {
        let dt = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let formatted = format_exif_datetime(&dt);
        assert_eq!(formatted, "2025:01:15 10:30:00");
    }

    #[test]
    fn test_apply_shift_add_days() {
        let dt = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let offset = DateOffset::new(0, 0, 1, 0, 0, 0);
        let result = apply_shift(dt, &offset, ShiftOperation::Add).unwrap();

        assert_eq!(result.year(), 2025);
        assert_eq!(result.month(), 1);
        assert_eq!(result.day(), 16);
        assert_eq!(result.hour(), 10);
        assert_eq!(result.minute(), 30);
    }

    #[test]
    fn test_apply_shift_add_months() {
        let dt = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let offset = DateOffset::new(0, 1, 0, 0, 0, 0);
        let result = apply_shift(dt, &offset, ShiftOperation::Add).unwrap();

        assert_eq!(result.year(), 2025);
        assert_eq!(result.month(), 2);
        assert_eq!(result.day(), 15);
    }

    #[test]
    fn test_apply_shift_add_years() {
        let dt = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let offset = DateOffset::new(1, 0, 0, 0, 0, 0);
        let result = apply_shift(dt, &offset, ShiftOperation::Add).unwrap();

        assert_eq!(result.year(), 2026);
        assert_eq!(result.month(), 1);
        assert_eq!(result.day(), 15);
    }

    #[test]
    fn test_apply_shift_subtract_days() {
        let dt = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let offset = DateOffset::new(0, 0, 5, 0, 0, 0);
        let result = apply_shift(dt, &offset, ShiftOperation::Subtract).unwrap();

        assert_eq!(result.year(), 2025);
        assert_eq!(result.month(), 1);
        assert_eq!(result.day(), 10);
    }

    #[test]
    fn test_apply_shift_add_hours() {
        let dt = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let offset = DateOffset::new(0, 0, 0, 6, 30, 0);
        let result = apply_shift(dt, &offset, ShiftOperation::Add).unwrap();

        assert_eq!(result.hour(), 17);
        assert_eq!(result.minute(), 0);
    }

    #[test]
    fn test_apply_shift_month_overflow() {
        // January 31 + 1 month = February 28 (or 29 in leap years)
        let dt = Utc.with_ymd_and_hms(2025, 1, 31, 10, 30, 0).unwrap();
        let offset = DateOffset::new(0, 1, 0, 0, 0, 0);
        let result = apply_shift(dt, &offset, ShiftOperation::Add).unwrap();

        // chrono handles this by clamping to the last day of the month
        assert_eq!(result.year(), 2025);
        assert_eq!(result.month(), 2);
        assert!(result.day() <= 28);
    }

    #[test]
    fn test_apply_shift_complex_offset() {
        let dt = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let offset = DateOffset::new(1, 2, 3, 4, 5, 6);
        let result = apply_shift(dt, &offset, ShiftOperation::Add).unwrap();

        // 1 year + 2 months = 14 months = 1 year 2 months
        // From 2025-01-15 -> 2026-03-15 (after adding 14 months)
        // Then add 3 days -> 2026-03-18
        // Then add 4:05:06 -> 14:35:06
        assert_eq!(result.year(), 2026);
        assert_eq!(result.month(), 3);
        assert_eq!(result.day(), 18);
        assert_eq!(result.hour(), 14);
        assert_eq!(result.minute(), 35);
        assert_eq!(result.second(), 6);
    }

    #[test]
    fn test_resolve_bare_name() {
        assert_eq!(
            resolve_exif_targets("DateTimeOriginal"),
            Some(vec![ExifDateTag::DateTimeOriginal])
        );
        assert_eq!(
            resolve_exif_targets("datetimeoriginal"),
            Some(vec![ExifDateTag::DateTimeOriginal])
        );
    }

    #[test]
    fn test_resolve_aliases() {
        // ExifTool's names and the EXIF spec's names both resolve
        assert_eq!(
            resolve_exif_targets("ModifyDate"),
            Some(vec![ExifDateTag::ModifyDate])
        );
        assert_eq!(
            resolve_exif_targets("DateTime"),
            Some(vec![ExifDateTag::ModifyDate])
        );
        assert_eq!(
            resolve_exif_targets("CreateDate"),
            Some(vec![ExifDateTag::CreateDate])
        );
        assert_eq!(
            resolve_exif_targets("DateTimeDigitized"),
            Some(vec![ExifDateTag::CreateDate])
        );
    }

    #[test]
    fn test_resolve_group_prefixes() {
        assert_eq!(
            resolve_exif_targets("EXIF:DateTimeOriginal"),
            Some(vec![ExifDateTag::DateTimeOriginal])
        );
        assert_eq!(
            resolve_exif_targets("ExifIFD:DateTimeOriginal"),
            Some(vec![ExifDateTag::DateTimeOriginal])
        );
        assert_eq!(
            resolve_exif_targets("IFD0:ModifyDate"),
            Some(vec![ExifDateTag::ModifyDate])
        );
        // Wrong group for the tag: DateTimeOriginal lives in ExifIFD, not IFD0
        assert_eq!(resolve_exif_targets("IFD0:DateTimeOriginal"), None);
        // Unknown group
        assert_eq!(resolve_exif_targets("XMP:CreateDate"), None);
    }

    #[test]
    fn test_resolve_alldates() {
        assert_eq!(
            resolve_exif_targets("AllDates"),
            Some(vec![
                ExifDateTag::ModifyDate,
                ExifDateTag::DateTimeOriginal,
                ExifDateTag::CreateDate,
            ])
        );
        assert_eq!(
            resolve_exif_targets("alldates"),
            resolve_exif_targets("AllDates")
        );
    }

    #[test]
    fn test_resolve_unknown_returns_none() {
        assert_eq!(resolve_exif_targets("Artist"), None);
        assert_eq!(resolve_exif_targets("GPSDateStamp"), None);
    }

    #[test]
    fn test_build_shift_spec_relative_negated() {
        let spec = build_shift_spec("-1", ShiftOperation::Subtract).unwrap();
        // Subtracting a negative shift adds
        match spec {
            ShiftSpec::Relative { offset, op } => {
                assert_eq!(op, ShiftOperation::Add);
                assert_eq!(offset, DateOffset::new(0, 0, 0, 1, 0, 0));
            }
            other => panic!("expected Relative, got {:?}", other),
        }
    }

    #[test]
    fn test_build_shift_spec_absolute() {
        let spec = build_shift_spec("2030:01:02 03:04:05", ShiftOperation::Set).unwrap();
        match spec {
            ShiftSpec::Absolute(dt) => {
                assert_eq!(format_exif_datetime(&dt), "2030:01:02 03:04:05");
            }
            other => panic!("expected Absolute, got {:?}", other),
        }
    }

    #[test]
    fn test_apply_spec_relative_and_absolute() {
        let dt = Utc.with_ymd_and_hms(2025, 6, 10, 12, 0, 0).unwrap();
        let relative = ShiftSpec::Relative {
            offset: DateOffset::new(0, 0, 0, 1, 0, 0),
            op: ShiftOperation::Subtract,
        };
        assert_eq!(
            format_exif_datetime(&apply_spec(dt, &relative).unwrap()),
            "2025:06:10 11:00:00"
        );
        let absolute = ShiftSpec::Absolute(Utc.with_ymd_and_hms(2030, 1, 2, 3, 4, 5).unwrap());
        assert_eq!(
            format_exif_datetime(&apply_spec(dt, &absolute).unwrap()),
            "2030:01:02 03:04:05"
        );
    }
}
