//! Olympus Digital Speech Standard (DSS) metadata parser.
//!
//! ExifTool routes standalone DSS files through `Olympus::DSS`, whose
//! generated binary table declares Model, StartTime, EndTime, Duration and
//! Comment. This parser reads exactly those assigned fields.

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;

const DSS_SIGNATURE: &[u8] = b"\x02dss";
const DS2_SIGNATURE: &[u8] = b"\x03ds2";
const DSS_PROCESS_PROBE_LEN: u64 = 69;
const DSS_EXIFTOOL_READ_LEN: u64 = 898;

/// Extract the Olympus DSS fields using ExifTool's declared `Olympus::DSS`
/// binary layout (`Olympus.pm`, `%Image::ExifTool::Olympus::DSS`).
pub fn parse_dss_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < DSS_PROCESS_PROBE_LEN {
        return Err("DSS file is too short for the Olympus DSS header".to_string());
    }

    let read_len = reader.size().min(DSS_EXIFTOOL_READ_LEN) as usize;
    let data = reader
        .read(0, read_len)
        .map_err(|error| error.to_string())?;
    if !data.starts_with(DSS_SIGNATURE) && !data.starts_with(DS2_SIGNATURE) {
        return Err("invalid DSS signature".to_string());
    }

    // Olympus.pm's `StartTime`/`EndTime` carry a `ValueConv` this schema does
    // not reproduce (12-digit `YYMMDDhhmmss` -> `20YY:MM:DD hh:mm:ss`), so
    // `Field::omitted.value_conv` is set; `format_dss_datetime` below is the
    // hand-verified equivalent, and these citations are RawAccess's required
    // acknowledgment.
    const START_TIME_CITATION: PerlCitation = PerlCitation {
        module: "Olympus",
        table: "DSS",
        tag: "StartTime",
        lines: "ValueConv, Olympus.pm",
    };
    const END_TIME_CITATION: PerlCitation = PerlCitation {
        module: "Olympus",
        table: "DSS",
        tag: "EndTime",
        lines: "ValueConv, Olympus.pm",
    };
    // Olympus.pm's `Duration` ValueConv turns a 6-digit `hhmmss` string into
    // seconds (`($1 * 60 + $2) * 60 + $3`, undef when the pattern misses) and
    // its PrintConv renders that through `Image::ExifTool::ConvertDuration`;
    // `format_dss_duration` below reproduces both, byte-for-byte against the
    // pinned 13.59 (`Olympus.pm` DSS table; `ExifTool.pm` `sub
    // ConvertDuration`).
    const DURATION_CITATION: PerlCitation = PerlCitation {
        module: "Olympus",
        table: "DSS",
        tag: "Duration",
        lines: "ValueConv + ConvertDuration, Olympus.pm / ExifTool.pm",
    };

    let table = find_table("Olympus", "DSS").ok_or("missing Olympus::DSS table")?;
    let decode = decode_binary_table(table, &data, ByteOrder::Little);

    let mut metadata = MetadataMap::new();
    for decoded in decode.fields() {
        match decoded.field.name {
            // Model (and Comment when present) have no omitted semantics:
            // the generated table's own emit path is the whole conversion.
            "Model" | "Comment" => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(format!("Olympus:{}", decoded.field.name), value);
                }
            }
            name @ ("StartTime" | "EndTime") => {
                let citation = if name == "StartTime" {
                    &START_TIME_CITATION
                } else {
                    &END_TIME_CITATION
                };
                let value = RawAccess::new(decoded, Acknowledged::VALUE_CONV, citation).and_then(
                    |access| match access.raw() {
                        DecodedValue::String(value) => Some(value.clone()),
                        _ => None,
                    },
                );
                if let Some(value) = value {
                    metadata.insert(
                        format!("Olympus:{name}"),
                        TagValue::new_string(format_dss_datetime(&value)),
                    );
                }
            }
            "Duration" => {
                let value = RawAccess::new(decoded, Acknowledged::VALUE_CONV, &DURATION_CITATION)
                    .and_then(|access| match access.raw() {
                        DecodedValue::String(value) => format_dss_duration(value),
                        _ => None,
                    });
                if let Some(value) = value {
                    metadata.insert("Olympus:Duration".to_string(), TagValue::new_string(value));
                }
            }
            _ => {}
        }
    }
    Ok(metadata)
}

/// Mirrors `Olympus.pm`'s `ValueConv`: transform a 12-digit `YYMMDDhhmmss`
/// string into `20YY:MM:DD hh:mm:ss`; otherwise leave the raw string intact.
fn format_dss_datetime(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() == 12 && bytes.iter().all(u8::is_ascii_digit) {
        format!(
            "20{}:{}:{} {}:{}:{}",
            &value[0..2],
            &value[2..4],
            &value[4..6],
            &value[6..8],
            &value[8..10],
            &value[10..12],
        )
    } else {
        value.to_string()
    }
}

/// Mirrors `Olympus.pm`'s Duration `ValueConv` -- the first 6 digits of the
/// field are `hhmmss`, converted to seconds, undef (here `None`) when the
/// pattern does not match -- followed by its `PrintConv`,
/// `Image::ExifTool::ConvertDuration`.
fn format_dss_duration(value: &str) -> Option<String> {
    // Perl's /(\d{2})(\d{2})(\d{2})/ is unanchored: it locks onto the first
    // run of six digits anywhere in the string.
    let bytes = value.as_bytes();
    let start = bytes
        .windows(6)
        .position(|window| window.iter().all(u8::is_ascii_digit))?;
    let digits = &value[start..start + 6];
    let hours: i64 = digits[0..2].parse().ok()?;
    let minutes: i64 = digits[2..4].parse().ok()?;
    let seconds: i64 = digits[4..6].parse().ok()?;
    Some(convert_duration((hours * 60 + minutes) * 60 + seconds))
}

/// `Image::ExifTool::ConvertDuration` (ExifTool.pm, pinned 13.59) for the
/// non-negative integer seconds `Olympus.pm`'s Duration ValueConv produces:
///
/// ```perl
/// return '0 s' if $time == 0;
/// return sprintf("%.2f s", $time) if $time < 30;
/// $time += 0.5;   # to round off to nearest second
/// my $h = int($time / 3600); $time -= $h * 3600;
/// my $m = int($time / 60);   $time -= $m * 60;
/// if ($h > 24) { my $d = int($h / 24); $h -= $d * 24; $sign = "$sign$d days "; }
/// return sprintf("$sign%d:%.2d:%.2d", $h, $m, int($time));
/// ```
fn convert_duration(total_seconds: i64) -> String {
    if total_seconds == 0 {
        return "0 s".to_string();
    }
    let (sign, magnitude) = if total_seconds > 0 {
        ("", total_seconds)
    } else {
        ("-", -total_seconds)
    };
    if magnitude < 30 {
        return format!("{sign}{magnitude}.00 s");
    }
    // ExifTool adds 0.5 then truncates; for the integer input this ValueConv
    // produces, that leaves int($time) == the seconds remainder exactly.
    let mut hours = magnitude / 3600;
    let remainder = magnitude % 3600;
    let minutes = remainder / 60;
    let seconds = remainder % 60;
    if hours > 24 {
        let days = hours / 24;
        hours -= days * 24;
        return format!("{sign}{days} days {hours}:{minutes:02}:{seconds:02}");
    }
    format!("{sign}{hours}:{minutes:02}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// The 80-byte `t/images/Olympus.dss` sample, transcribed: signature,
    /// Model at 12 (`DS2300` + 10 spaces), StartTime `051116135242` at 38,
    /// EndTime `051116135253` at 50, Duration `000010` at 62.
    fn olympus_dss_sample() -> Vec<u8> {
        let mut data = vec![0u8; 80];
        data[0..4].copy_from_slice(b"\x02dss");
        data[12..28].copy_from_slice(b"DS2300          ");
        data[38..50].copy_from_slice(b"051116135242");
        data[50..62].copy_from_slice(b"051116135253");
        data[62..68].copy_from_slice(b"000010");
        data
    }

    /// Pins the whole `Olympus.dss` decode against the pinned oracle:
    /// `exiftool -G1 -s` reports `Model DS2300`, `StartTime 2005:11:16
    /// 13:52:42`, `EndTime 2005:11:16 13:52:53`, `Duration 10.00 s`
    /// (13.59; JSON keeps Model's trailing pad spaces).
    #[test]
    fn test_olympus_dss_sample_fields() {
        let reader = TestReader::new(olympus_dss_sample());
        let metadata = parse_dss_metadata(&reader).unwrap();
        assert_eq!(
            metadata.get_string("Olympus:Model"),
            Some("DS2300          ")
        );
        assert_eq!(
            metadata.get_string("Olympus:StartTime"),
            Some("2005:11:16 13:52:42")
        );
        assert_eq!(
            metadata.get_string("Olympus:EndTime"),
            Some("2005:11:16 13:52:53")
        );
        assert_eq!(metadata.get_string("Olympus:Duration"), Some("10.00 s"));
    }

    #[test]
    fn test_format_dss_datetime_passthrough_on_non_digits() {
        assert_eq!(format_dss_datetime("not a date"), "not a date");
    }

    /// `ConvertDuration` branch coverage: zero, the sub-30 s sprintf, the
    /// H:MM:SS form, and the day-carrying form (25 h -> "1 days 1:00:00",
    /// ExifTool prints the plural unconditionally).
    #[test]
    fn test_convert_duration_branches() {
        assert_eq!(convert_duration(0), "0 s");
        assert_eq!(convert_duration(10), "10.00 s");
        assert_eq!(convert_duration(29), "29.00 s");
        assert_eq!(convert_duration(30), "0:00:30");
        assert_eq!(convert_duration(3661), "1:01:01");
        assert_eq!(convert_duration(25 * 3600), "1 days 1:00:00");
    }

    #[test]
    fn test_format_dss_duration_rejects_non_digits() {
        assert_eq!(format_dss_duration("ab12cd"), None);
        assert_eq!(format_dss_duration(""), None);
    }
}
