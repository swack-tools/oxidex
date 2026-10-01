//! Date/Time shifting operations for metadata tags
//!
//! This module implements date shifting functionality compatible with ExifTool syntax:
//! - Add offset: `-AllDates+=1:2:3 4:5:6` (add 1 year, 2 months, 3 days, 4 hours, 5 minutes, 6 seconds)
//! - Subtract offset: `-EXIF:ModifyDate-=0:0:5 0:0:0` (subtract 5 days)
//! - Set absolute: `-EXIF:ModifyDate=2025:01:15 10:30:00` (set to specific date/time)

use super::operations::{read_metadata, write_metadata};
use super::tag_value::TagValue;
use crate::core::{FileFormat, FileReader};
use crate::error::{ExifToolError, Result};
use crate::io::MMapReader;
use crate::parsers::detection::detect_format;
use chrono::{DateTime, Duration, Months, NaiveDate, NaiveDateTime, Utc};
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
            ExifDateTag::CreateDate => "ExifIFD:CreateDate",
        }
    }
}

/// Resolves a user-supplied tag pattern to the EXIF date tags it names.
///
/// Accepts ExifTool conventions: pinned ExifTool 13.59's tag names
/// ("DateTimeOriginal"), bare or under the "EXIF:", "IFD0:" or "ExifIFD:"
/// group, and the "AllDates" shortcut. Matching is ASCII case-insensitive.
///
/// The EXIF specification's names for tags 0x0132 and 0x9004 ("DateTime",
/// "DateTimeDigitized") are not 13.59's: grouped, 13.59 answers "Sorry,
/// IFD0:DateTimeDigitized doesn't exist or isn't writable" and changes
/// nothing, and bare, it looks for XMP-exif:DateTimeDigitized (t/images
/// Canon.jpg: "1 image files unchanged"). Neither shifts an EXIF date, so
/// neither resolves here (codex pre-review of #964).
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
        "modifydate" => ExifDateTag::ModifyDate,
        "datetimeoriginal" => ExifDateTag::DateTimeOriginal,
        "createdate" => ExifDateTag::CreateDate,
        _ => return None,
    };

    // Naming either of IFD0/ExifIFD shifts the tag's copies in both: pinned
    // ExifTool 13.59's `%crossDelete` keeps (and shifts) the copy in the
    // other directory when the new value is a shift (WriteExif.pl
    // 13.59:1259), so `-IFD0:CreateDate+=1` on t/images Canon.jpg shifts
    // its ExifIFD CreateDate.
    let family_ok = matches!(family, None | Some("exif" | "ifd0" | "exififd"));
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

/// Canonical date/time tag names shifted by the "AllDates" pattern on
/// formats that use the metadata-map write path (PNG, PDF). Lowercase.
const ALL_DATES_NAMES: &[&str] = &[
    "modifydate",
    "datetime",
    "datetimeoriginal",
    "createdate",
    "datetimedigitized",
    "creationtime",
    "contentcreatedate",
];

/// Returns true when a metadata key matches a user-supplied tag pattern.
///
/// A bare pattern ("DateTimeOriginal") matches the name part of any
/// group-prefixed key. A qualified pattern matches its exact group, except
/// the family-0 `XMP` group also matches an internally namespace-prefixed
/// `XMP-<namespace>` key. Comparison is ASCII case-insensitive.
fn key_matches_pattern(key: &str, pattern: &str) -> bool {
    if key.eq_ignore_ascii_case(pattern) {
        return true;
    }
    let Some((key_group, key_name)) = key.split_once(':') else {
        return false;
    };
    match pattern.split_once(':') {
        None => key_name.eq_ignore_ascii_case(pattern),
        Some((pattern_group, pattern_name)) => {
            key_name.eq_ignore_ascii_case(pattern_name)
                && (key_group.eq_ignore_ascii_case(pattern_group)
                    || (pattern_group.eq_ignore_ascii_case("XMP")
                        && key_group
                            .get(..4)
                            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("XMP-"))))
        }
    }
}

/// ExifTool prepends the operation sign before validating a time shift.
/// A second sign makes one or two numeric components a timezone shift;
/// larger signed additions are invalid, while larger subtractions are no-ops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SignedAllDatesShift {
    Invalid,
    Unchanged,
    TimezoneOnly,
}

pub(crate) fn signed_alldates_shift(
    tag_pattern: &str,
    offset_or_value: &str,
    op: ShiftOperation,
) -> Option<SignedAllDatesShift> {
    if !tag_pattern.eq_ignore_ascii_case("AllDates") || op == ShiftOperation::Set {
        return None;
    }
    let operand = offset_or_value.trim();
    if !operand.starts_with(['+', '-']) {
        return None;
    }
    // Shift.pl::CheckShift removes the operation sign, then SplitTime treats
    // this remaining signed word as a timezone. The signed branch accepts
    // one or two numeric captures, rejects more than two, and rejects an
    // additional numeric word because its timezone slot is already filled.
    // SplitTime's numeric capture is /(?=\d|\.\d)\d*(?:\.\d*)?/g.
    let numeric_captures = |word: &str| {
        let bytes = word.as_bytes();
        let mut at = 0;
        let mut count = 0;
        while at < bytes.len() {
            let starts = bytes[at].is_ascii_digit()
                || (bytes[at] == b'.' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit));
            if !starts {
                at += 1;
                continue;
            }
            count += 1;
            while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
            if bytes.get(at) == Some(&b'.') {
                at += 1;
                while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                    at += 1;
                }
            }
        }
        count
    };
    let mut words = operand.split_whitespace();
    let first = words.next().unwrap_or("");
    let first_count = numeric_captures(first);
    let invalid =
        first_count == 0 || first_count > 2 || words.any(|word| numeric_captures(word) != 0);
    Some(if invalid {
        if op == ShiftOperation::Add {
            SignedAllDatesShift::Invalid
        } else {
            SignedAllDatesShift::Unchanged
        }
    } else {
        SignedAllDatesShift::TimezoneOnly
    })
}

/// Shifts date/time tags in a file's metadata.
///
/// # Arguments
///
/// * `path` - Path to the file to modify
/// * `tag_pattern` - "AllDates", a bare tag name ("DateTimeOriginal"), or a
///   group-prefixed name ("EXIF:DateTimeOriginal", "IFD0:ModifyDate")
/// * `offset_or_value` - ExifTool-style shift string (see [`parse_offset`]),
///   or an absolute "YYYY:MM:DD HH:MM:SS" for the Set operation
/// * `op` - Operation type (Add, Subtract, or Set)
///
/// # Behavior by format
///
/// * **JPEG**: the EXIF date values are patched in place — only the 19 ASCII
///   characters of each target value change, every other byte of the file is
///   preserved. Supported tags: AllDates, ModifyDate, DateTimeOriginal,
///   CreateDate.
/// * **Other formats** (PNG, PDF): tags are shifted through the metadata map
///   and rewritten with [`write_metadata`].
///
/// # Divergences from ExifTool
///
/// * A shift that matches no tags leaves the file unchanged and returns
///   `Ok(())`, as exiftool's `0 image files updated` / `1 image files
///   unchanged` (exit 0) does.
/// * During a multi-tag shift (AllDates), tags whose current value cannot be
///   parsed or shifted are skipped with a warning, matching ExifTool.
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::date_shift::{shift_metadata_dates, ShiftOperation};
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Subtract 1 hour from DateTimeOriginal (ExifTool: -DateTimeOriginal-=1:00:00)
/// shift_metadata_dates(
///     Path::new("photo.jpg"),
///     "DateTimeOriginal",
///     "1:00:00",
///     ShiftOperation::Subtract
/// )?;
///
/// // Add 1 day to all date tags
/// shift_metadata_dates(Path::new("photo.jpg"), "AllDates", "0:0:1 0", ShiftOperation::Add)?;
/// # Ok(())
/// # }
/// ```
pub fn shift_metadata_dates(
    path: &Path,
    tag_pattern: &str,
    offset_or_value: &str,
    op: ShiftOperation,
) -> Result<()> {
    let signed_shift = signed_alldates_shift(tag_pattern, offset_or_value, op);
    if signed_shift == Some(SignedAllDatesShift::Invalid) {
        return Err(ExifToolError::parse_error(format!(
            "Invalid shift string ({offset_or_value}) for IFD0:ModifyDate"
        )));
    }
    // An absolute set of an EXIF date in a JPEG, TIFF or PNG is an ordinary
    // write: ExifTool writes it to the tag's directory and deletes the copy
    // in the other of IFD0/ExifIFD (`writers::exif_cross_delete`), where
    // this path only patched the copies the file already held.
    if op == ShiftOperation::Set
        && let Some(keys) =
            crate::writers::exif_cross_delete::date_set_keys(&read_metadata(path)?, tag_pattern, {
                let reader = MMapReader::new(path)?;
                let format = detect_format(&reader)?;
                crate::core::operations::is_surgical_tiff_target(format, &reader)
            })?
    {
        return crate::core::operations::set_exif_dates(path, &keys, offset_or_value);
    }
    // So is one named by its IFD0 or ExifIFD group (and `EXIF:ModifyDate`),
    // exactly as the CLI routes it (`CliArgs::parse_date_shift`): the
    // in-place route below finds the tag in either directory, so
    // `IFD0:CreateDate` set to a date on a JPEG whose ExifIFD holds
    // CreateDate patched that copy and reported success, where pinned
    // ExifTool 13.59 creates `[IFD0] CreateDate` and deletes the ExifIFD one
    // (codex pre-review of #964).
    if op == ShiftOperation::Set
        && resolve_exif_targets(tag_pattern).is_some()
        && tag_pattern.split_once(':').is_some_and(|(group, name)| {
            group.eq_ignore_ascii_case("IFD0")
                || group.eq_ignore_ascii_case("ExifIFD")
                || (group.eq_ignore_ascii_case("EXIF") && name.eq_ignore_ascii_case("ModifyDate"))
        })
    {
        let value = crate::cli::value_parser::parse_cli_tag_value(tag_pattern, offset_or_value)?;
        return crate::core::operations::modify_tag(path, tag_pattern, value).map(|_| ());
    }
    let spec = if signed_shift.is_none() {
        Some(build_shift_spec(offset_or_value, op)?)
    } else {
        None
    };

    let (format, classic_tiff_raw, walkable_tiff_raw) = {
        let reader = MMapReader::new(path)?;
        let format = detect_format(&reader)?;
        // The map and in-place shift routes can both decide that no date is
        // present without entering the PNG writer. Pinned ExifTool still
        // checks carried chunk CRCs before that no-op decision.
        if format == FileFormat::PNG {
            crate::writers::png_writer::refuse_bad_chunk_crcs(&reader)?;
        }
        let walkable_tiff_raw = matches!(format, FileFormat::CameraRaw(_))
            && crate::core::operations::is_surgical_tiff_target(format, &reader);
        let classic_tiff_raw = walkable_tiff_raw
            && reader
                .read(0, 4)
                .is_ok_and(|header| matches!(header, b"II\x2a\x00" | b"MM\x00\x2a"));
        (format, classic_tiff_raw, walkable_tiff_raw)
    };

    if signed_shift == Some(SignedAllDatesShift::Unchanged) {
        return Ok(());
    }
    if signed_shift == Some(SignedAllDatesShift::TimezoneOnly) {
        return shift_signed_alldates_timezone_only(path, format, offset_or_value, op);
    }
    let spec = spec.expect("a regular shift has a parsed spec");
    if format == FileFormat::JPEG {
        return shift_jpeg_dates(path, tag_pattern, offset_or_value, &spec);
    }
    // A TIFF's or PNG's EXIF dates shift in place too, every IFD0/ExifIFD
    // copy of each: the map route below sees only the copy the reader
    // reports, and its write moved the other one away as a set would
    // (review of #964, PRRT_kwDOQNbr5M6mTAHI). The map route then shifts
    // the file's other date rows (XMP, ...) as before.
    //
    // Both phases run on one private copy, which replaces the file only when
    // both succeed: the in-place phase committed to `path` before the map
    // phase could still refuse (a non-EXIF CreateDate that is not a date),
    // so a direct caller got `Err` with the EXIF copies already shifted
    // (review of #964, discussion_r4112777222).
    if walkable_tiff_raw && !classic_tiff_raw && resolve_exif_targets(tag_pattern).is_some() {
        return Err(ExifToolError::unsupported_format(
            "Date shifts for this TIFF-derived RAW header are not supported; nothing was written",
        ));
    }
    if (matches!(format, FileFormat::TIFF | FileFormat::PNG) || classic_tiff_raw)
        && let Some(targets) = resolve_exif_targets(tag_pattern)
    {
        return crate::core::write_transaction::transact(path, |scratch| {
            let shifted =
                crate::writers::exif_inplace::shift_tiff_png_exif_dates(scratch, &targets, &spec)?;
            shift_map_dates_after_exif(scratch, tag_pattern, offset_or_value, &spec, shifted)
        })
        .map(|_| ());
    }
    shift_map_dates(path, tag_pattern, offset_or_value, &spec)
}

fn date_text_has_timezone(text: &str) -> bool {
    let (date, has_zone) = if let Some(date) = text.strip_suffix('Z') {
        (date, true)
    } else if text.len() >= 6 {
        let Some(date) = text.get(..text.len() - 6) else {
            return false;
        };
        let suffix = &text[text.len() - 6..];
        let bytes = suffix.as_bytes();
        let valid_zone = matches!(bytes[0], b'+' | b'-')
            && bytes[1..3].iter().all(u8::is_ascii_digit)
            && bytes[3] == b':'
            && bytes[4..6].iter().all(u8::is_ascii_digit);
        (date, valid_zone)
    } else {
        return false;
    };
    if !has_zone {
        return false;
    }
    let (whole, fraction_ok) = match date.split_once('.') {
        Some((whole, fraction)) => (
            whole,
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit()),
        ),
        None => (date, true),
    };
    fraction_ok
        && (NaiveDateTime::parse_from_str(whole, "%Y:%m:%d %H:%M:%S").is_ok()
            || NaiveDateTime::parse_from_str(whole, "%Y-%m-%dT%H:%M:%S").is_ok()
            || NaiveDate::parse_from_str(whole, "%Y:%m:%d").is_ok()
            || NaiveDate::parse_from_str(whole, "%Y-%m-%d").is_ok())
}

/// A compact signed AllDates operand is a timezone shift in native Shift.pl,
/// so EXIF date strings without zones are left untouched. The current writer
/// cannot persist timezone-bearing XMP/PDF/PNG date rows. Refuse those rows
/// atomically instead of reporting an unchanged success for a missed edit.
/// The selected 11.78 PNG AddChunks guard is the sole supported exception:
/// it creates absent PNG:CreateDate with the literal operation sign and
/// operand, even though no date or timezone value was present to shift.
fn shift_signed_alldates_timezone_only(
    path: &Path,
    format: FileFormat,
    offset_or_value: &str,
    op: ShiftOperation,
) -> Result<()> {
    let mut metadata = read_metadata(path)?;
    let timezoned = metadata.iter().find(|(key, value)| {
        let name = key.rsplit_once(':').map_or(key.as_str(), |(_, name)| name);
        ALL_DATES_NAMES.contains(&name.to_ascii_lowercase().as_str())
            && value.as_string().is_some_and(date_text_has_timezone)
    });
    if let Some((key, _)) = timezoned {
        return Err(ExifToolError::unsupported_format(format!(
            "Timezone-only AllDates shift cannot write {key}; nothing was written"
        )));
    }
    if format == FileFormat::PNG
        && crate::writers::generated_png_shift_contract::PNG_ABSENT_CREATE_DATE_SHIFT_LITERAL
        && !metadata.contains_key("PNG:CreateDate")
    {
        if png_has_create_date_chunk(path)? {
            return Err(ExifToolError::parse_error(
                "Cannot shift an unreadable existing PNG create-date chunk",
            ));
        }
        let sign = if op == ShiftOperation::Add { '+' } else { '-' };
        metadata.insert(
            "PNG:CreateDate",
            TagValue::new_string(format!("{sign}{offset_or_value}")),
        );
        write_metadata(path, &metadata)?;
    }
    Ok(())
}

/// JPEG path: patch EXIF date/time values in place. Never rewrites the EXIF
/// segment, so binary tags are preserved byte-for-byte.
fn shift_jpeg_dates(
    path: &Path,
    tag_pattern: &str,
    offset_or_value: &str,
    spec: &ShiftSpec,
) -> Result<()> {
    let Some(targets) = resolve_exif_targets(tag_pattern) else {
        // These names are not ExifTool aliases for EXIF's writable dates.
        // A JPEG with no matching XMP row treats either shift as unchanged;
        // a matching non-EXIF row is handled by the ordinary map route.
        if ["DateTime", "DateTimeDigitized"]
            .iter()
            .any(|name| tag_pattern.eq_ignore_ascii_case(name))
        {
            return shift_map_dates(path, tag_pattern, offset_or_value, spec);
        }
        return Err(ExifToolError::parse_error(format!(
            "Shifting tag '{}' is not supported for JPEG. Supported: AllDates, \
             ModifyDate, DateTimeOriginal, CreateDate",
            tag_pattern
        )));
    };
    // A shift that finds nothing to shift leaves the file as it is: pinned
    // 13.59's `-DateTimeOriginal+=1` on a file without one is `0 image
    // files updated` / `1 image files unchanged`, exit 0.
    crate::writers::exif_inplace::shift_jpeg_exif_dates(path, &targets, spec)?;
    Ok(())
}

/// Non-JPEG path: shift date/time tags through the metadata map (PNG, PDF).
fn shift_map_dates(
    path: &Path,
    tag_pattern: &str,
    offset_or_value: &str,
    spec: &ShiftSpec,
) -> Result<()> {
    shift_map_dates_after_exif(path, tag_pattern, offset_or_value, spec, None)
}

/// [`shift_map_dates`]; `exif_shifted` is the number of IFD0/ExifIFD values
/// already shifted in place, whose rows are then left alone.
fn shift_map_dates_after_exif(
    path: &Path,
    tag_pattern: &str,
    offset_or_value: &str,
    spec: &ShiftSpec,
    exif_shifted: Option<usize>,
) -> Result<()> {
    let mut metadata = read_metadata(path)?;
    let all_dates = tag_pattern.eq_ignore_ascii_case("AllDates");
    let is_png = matches!(detect_format(&MMapReader::new(path)?)?, FileFormat::PNG);

    let keys: Vec<String> = metadata.iter().map(|(k, _)| k.clone()).collect();
    let mut modified = 0;
    for key in keys {
        let matches = if all_dates {
            // Filesystem dates are never shifted by AllDates
            !key.starts_with("File:")
                && key.split_once(':').map_or_else(
                    || ALL_DATES_NAMES.contains(&key.to_ascii_lowercase().as_str()),
                    |(_, name)| ALL_DATES_NAMES.contains(&name.to_ascii_lowercase().as_str()),
                )
        } else {
            key_matches_pattern(&key, tag_pattern)
        };
        let exif_row = key
            .split_once(':')
            .is_some_and(|(group, _)| matches!(group, "IFD0" | "ExifIFD" | "EXIF"));
        if !matches || (exif_shifted.is_some() && exif_row) {
            continue;
        }
        // A date the reader keeps as text (a PNG `tIME` row) shifts too, as
        // in 13.59 -- when the text is a plain `YYYY:MM:DD HH:MM:SS` date.
        let held = metadata.get(&key).and_then(|value| match value {
            TagValue::String(text) if text.len() == 19 => parse_absolute_datetime(text).ok(),
            other => other.as_datetime().copied(),
        });
        let Some(dt) = held else {
            if !all_dates {
                return Err(ExifToolError::parse_error(format!(
                    "Tag '{}' is not a DateTime tag",
                    key
                )));
            }
            continue;
        };
        let new_dt = apply_spec(dt, spec)?;
        metadata.insert(key, TagValue::new_datetime(new_dt));
        modified += 1;
    }

    // In the selected 11.78 WritePNG.pl, AddChunks accepts the negative
    // "unknown" IsOverwriting result for an absent shifted PNG text tag.
    // SetNewValue leaves the signed shift operand as its new value, and the
    // create-date inverse leaves it literal. Later AddChunks requires > 0,
    // so it does not create this tag. This applies only to the PNG text
    // CreateDate target, never an absent EXIF date.
    if is_png
        && crate::writers::generated_png_shift_contract::PNG_ABSENT_CREATE_DATE_SHIFT_LITERAL
        && (all_dates || key_matches_pattern("PNG:CreateDate", tag_pattern))
        && !metadata.contains_key("PNG:CreateDate")
        && let ShiftSpec::Relative { op, .. } = spec
    {
        if png_has_create_date_chunk(path)? {
            return Err(ExifToolError::parse_error(
                "Cannot shift an unreadable existing PNG create-date chunk",
            ));
        }
        let shift = match op {
            ShiftOperation::Add => format!("+{}", offset_or_value.trim_start_matches(['+', '-'])),
            ShiftOperation::Subtract => {
                format!("-{}", offset_or_value.trim_start_matches(['+', '-']))
            }
            ShiftOperation::Set => unreachable!(),
        };
        metadata.insert("PNG:CreateDate", TagValue::new_string(shift));
        modified += 1;
    }

    if modified == 0 {
        // Nothing to shift: the file is left as it is (13.59: `unchanged`).
        return Ok(());
    }
    write_metadata(path, &metadata)?;
    Ok(())
}

fn png_has_create_date_chunk(path: &Path) -> Result<bool> {
    use crate::parsers::png::chunk_parser::parse_chunk;

    let reader = MMapReader::new(path)?;
    let mut offset = 8;
    while offset < reader.size() {
        let (next, chunk) = parse_chunk(&reader, offset)?;
        if chunk.is_text_chunk() && chunk.data.starts_with(b"create-date\0") {
            return Ok(true);
        }
        if chunk.chunk_type == *b"IEND" {
            break;
        }
        offset = next;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone, Timelike};

    #[test]
    fn png_absent_create_date_shift_follows_selected_addchunks_guard() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/png/sample.png");
        for (operation, literal) in [
            (ShiftOperation::Add, "+1:0:0 0:0:0"),
            (ShiftOperation::Subtract, "-1:0:0 0:0:0"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("sample.png");
            std::fs::copy(&fixture, &file).unwrap();
            assert!(
                read_metadata(&file)
                    .unwrap()
                    .get("PNG:CreateDate")
                    .is_none()
            );
            shift_metadata_dates(&file, "AllDates", "1:0:0 0:0:0", operation).unwrap();
            let actual = read_metadata(&file)
                .unwrap()
                .get_string("PNG:CreateDate")
                .map(str::to_owned);
            let expected =
                crate::writers::generated_png_shift_contract::PNG_ABSENT_CREATE_DATE_SHIFT_LITERAL
                    .then_some(literal.to_owned());
            assert_eq!(
                actual, expected,
                "the selected AddChunks guard controls absent PNG CreateDate"
            );
        }
    }

    #[test]
    fn signed_alldates_shift_operands_do_not_change_the_file() {
        // Pinned 11.78 and 13.59 both refuse +=-1:0:0 with an invalid-shift
        // warning; -=-1:0:0 reports unchanged. Neither shifts an EXIF date.
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/png/sample.png");
        for (operation, expects_error) in [
            (ShiftOperation::Add, true),
            (ShiftOperation::Subtract, false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("sample.png");
            std::fs::copy(&fixture, &file).unwrap();
            let original = std::fs::read(&file).unwrap();
            let result = shift_metadata_dates(&file, "AllDates", "-1:0:0 0:0:0", operation);
            assert_eq!(result.is_err(), expects_error);
            assert_eq!(std::fs::read(&file).unwrap(), original);
        }
    }

    #[test]
    fn signed_alldates_compact_grammar_matches_native() {
        for operand in ["-1", "-1:2", "-1.5", "--1", "+-1"] {
            assert_eq!(
                signed_alldates_shift("AllDates", operand, ShiftOperation::Add),
                Some(SignedAllDatesShift::TimezoneOnly),
                "{operand}"
            );
        }
        for operand in ["-garbage", "--garbage", "+1:0:0", "-1:0:0 0:0:0"] {
            assert_eq!(
                signed_alldates_shift("AllDates", operand, ShiftOperation::Add),
                Some(SignedAllDatesShift::Invalid),
                "{operand}"
            );
            assert_eq!(
                signed_alldates_shift("AllDates", operand, ShiftOperation::Subtract),
                Some(SignedAllDatesShift::Unchanged),
                "{operand}"
            );
        }
    }

    #[test]
    fn signed_compact_alldates_refuses_unwritable_timezone_atomically() {
        let dir = tempfile::tempdir().unwrap();
        for value in ["2020:01:02 03:04:05+02:00", "2020:01:02 03:04:05.123+02:00"] {
            let file = dir.path().join("dates.xmp");
            let xmp = format!(
                r#"<?xpacket begin="" id=""?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/"
 xmp:CreateDate="{value}" />
</rdf:RDF><?xpacket end="w"?>"#
            );
            std::fs::write(&file, &xmp).unwrap();
            assert_eq!(
                read_metadata(&file).unwrap().get_string("XMP:CreateDate"),
                Some(value)
            );
            let error =
                shift_metadata_dates(&file, "AllDates", "-1", ShiftOperation::Add).unwrap_err();
            assert!(error.to_string().contains("XMP:CreateDate"), "{error}");
            assert_eq!(std::fs::read(&file).unwrap(), xmp.as_bytes());
        }
    }

    #[test]
    fn signed_compact_alldates_preserves_selected_png_literal() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/png/sample.png");
        for (op, literal) in [
            (ShiftOperation::Add, "+-1"),
            (ShiftOperation::Subtract, "--1"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("sample.png");
            std::fs::copy(&fixture, &file).unwrap();
            shift_metadata_dates(&file, "AllDates", "-1", op).unwrap();
            let expected =
                crate::writers::generated_png_shift_contract::PNG_ABSENT_CREATE_DATE_SHIFT_LITERAL
                    .then_some(literal);
            assert_eq!(
                read_metadata(&file).unwrap().get_string("PNG:CreateDate"),
                expected
            );
        }
    }

    #[test]
    fn signed_alldates_noop_still_checks_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.png");
        assert!(
            shift_metadata_dates(
                &missing,
                "AllDates",
                "-1:0:0 0:0:0",
                ShiftOperation::Subtract,
            )
            .is_err()
        );

        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/png/sample.png");
        let mut bad_crc = std::fs::read(fixture).unwrap();
        assert_eq!(&bad_crc[12..16], b"IHDR");
        bad_crc[16] ^= 1;
        let file = dir.path().join("bad-crc.png");
        std::fs::write(&file, &bad_crc).unwrap();
        assert!(
            shift_metadata_dates(&file, "AllDates", "-1:0:0 0:0:0", ShiftOperation::Subtract,)
                .is_err()
        );
        assert_eq!(std::fs::read(file).unwrap(), bad_crc);
    }

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
        // ExifTool's names resolve; the EXIF spec's names for 0x0132 and
        // 0x9004 are not pinned 13.59's and shift no EXIF date there
        // (`-ExifIFD:DateTime+=1` and `-IFD0:DateTimeDigitized+=1` on
        // t/images Canon.jpg: "doesn't exist or isn't writable"; bare
        // `-DateTime+=1`: "1 image files unchanged").
        assert_eq!(
            resolve_exif_targets("ModifyDate"),
            Some(vec![ExifDateTag::ModifyDate])
        );
        assert_eq!(
            resolve_exif_targets("CreateDate"),
            Some(vec![ExifDateTag::CreateDate])
        );
        for alias in [
            "DateTime",
            "DateTimeDigitized",
            "EXIF:DateTime",
            "EXIF:DateTimeDigitized",
            "IFD0:DateTime",
            "IFD0:DateTimeDigitized",
            "ExifIFD:DateTime",
            "ExifIFD:DateTimeDigitized",
        ] {
            assert_eq!(resolve_exif_targets(alias), None, "{alias}");
        }
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
        // The other directory's name selects the tag too: a shift covers its
        // copies in both IFD0 and ExifIFD (WriteExif.pl 13.59:1259).
        assert_eq!(
            resolve_exif_targets("IFD0:DateTimeOriginal"),
            Some(vec![ExifDateTag::DateTimeOriginal])
        );
        assert_eq!(
            resolve_exif_targets("ExifIFD:ModifyDate"),
            Some(vec![ExifDateTag::ModifyDate])
        );
        assert_eq!(resolve_exif_targets("IFD1:ModifyDate"), None);
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
    fn test_key_matches_pattern() {
        // Bare pattern matches any family, case-insensitively
        assert!(key_matches_pattern("XMP:CreateDate", "createdate"));
        assert!(key_matches_pattern("PDF:CreateDate", "CreateDate"));
        // Prefixed pattern must match the whole key
        assert!(key_matches_pattern("XMP:CreateDate", "xmp:createdate"));
        assert!(!key_matches_pattern("PDF:CreateDate", "XMP:CreateDate"));
        // The public family-0 XMP spelling also selects an internally
        // namespace-prefixed XMP property; a family-1 spelling stays exact.
        assert!(key_matches_pattern(
            "XMP-exif:DateTimeOriginal",
            "xmp:datetimeoriginal"
        ));
        assert!(key_matches_pattern(
            "XMP-exif:DateTimeOriginal",
            "xMp-ExIf:DaTeTiMeOrIgInAl"
        ));
        assert!(!key_matches_pattern(
            "XMP-exif:DateTimeOriginal",
            "XMP-tiff:DateTimeOriginal"
        ));
        assert!(!key_matches_pattern(
            "ExifIFD:DateTimeOriginal",
            "XMP:DateTimeOriginal"
        ));
        assert!(!key_matches_pattern(
            "XMP-exif:DateTimeOriginal",
            "EXIF:DateTimeOriginal"
        ));
        // Name-only mismatch
        assert!(!key_matches_pattern("XMP:ModifyDate", "CreateDate"));
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
