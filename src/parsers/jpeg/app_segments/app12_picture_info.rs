//! APP12 "Picture Info" parser (ExifTool `Image::ExifTool::APP12::PictureInfo`)
//!
//! ExifTool routes every non-"Ducky" JPEG APP12 segment through
//! `ProcessAPP12` (ExifTool.pm:8345), which scans the whole payload with a
//! single regular expression looking for section headers (`[camera info]`)
//! and `tag=value` pairs. Agfa and Polaroid cameras are the usual writers,
//! but the parser is deliberately format-agnostic: any `tag=value` pair found
//! in the segment is extracted, whether or not it has a table entry.
//!
//! # Format
//!
//! ```text
//! Agfa Gevaert   \0 721\r\n
//! [picture info]\r\n
//! TimeDate=915117440\r\n
//! Shutter=6460\r\n
//! ...
//! [camera info]\r\n
//! Type=SR84\r\n
//! Serial#=#00000001\r\n
//! ```
//!
//! Leading vendor banners, trailing binary blocks and unknown sections are
//! simply skipped -- only text that matches the scan pattern is extracted.
//!
//! # Tag naming
//!
//! Keys with an entry in ExifTool's `%APP12::PictureInfo` table get that
//! entry's name and conversions. Everything else is named by ExifTool's
//! generic `AddTagToTable` rules (ExifTool.pm:9232-9244): drop characters
//! outside `[-_a-zA-Z0-9]`, capitalise the first letter, and prefix `Tag`
//! when the result is shorter than two characters or does not start with a
//! letter. That is why `Q=96` surfaces as `TagQ` and `s0=...` as `S0`.
//!
//! All tags are emitted under the `APP12:` family -- ExifTool's family-0
//! group for this table (family 1 is `PictureInfo`).

use crate::core::MetadataMap;
use crate::core::TagValue;
use crate::core::formatters::exif_print_conv::print_exposure_time_micros_str as print_exposure_time;
use crate::error::Result;
use crate::io::timestamp::unix_to_iso8601;

/// Minimum length required before a segment is worth scanning
/// (the shortest possible pair, e.g. "A=B").
const MIN_SEGMENT_LENGTH: usize = 3;

/// True for the characters in ExifTool's `[\w#-]` tag-name class.
fn is_tag_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'#' || b == b'-'
}

/// True for the characters in ExifTool's `[\x20-\x7e]` value class.
fn is_value_char(b: u8) -> bool {
    (0x20..=0x7e).contains(&b)
}

/// True for Perl's `\s` class.
fn is_perl_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0x0b)
}

/// Parses an APP12 "Picture Info" segment.
///
/// # Arguments
///
/// * `data` - Raw APP12 segment payload (after the marker and length bytes)
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Tags found in the segment, keyed `APP12:<Name>`.
///   An empty map when the segment holds no recognisable `tag=value` text,
///   which is what ExifTool reports for such a segment.
/// * `Err(ExifToolError)` - If the segment is too short to hold any pair
///
/// # Errors
///
/// Returns an error if the segment is shorter than the shortest possible
/// `tag=value` pair.
///
/// # Example
///
/// ```ignore
/// let data = b"[camera info]\r\nType=SR84\r\nVersion=v84-71\r\n";
/// let metadata = parse_app12_picture_info(data)?;
/// assert_eq!(metadata.get_string("APP12:CameraType"), Some("SR84"));
/// ```
pub fn parse_app12_picture_info(data: &[u8]) -> Result<MetadataMap> {
    if data.len() < MIN_SEGMENT_LENGTH {
        return Err(crate::error::ExifToolError::parse_error(format!(
            "APP12 Picture Info segment too short: {} bytes (minimum {} required)",
            data.len(),
            MIN_SEGMENT_LENGTH
        )));
    }

    let mut metadata = MetadataMap::new();
    for (key, value) in scan_pairs(data) {
        let (name, value) = convert_tag(&key, &value);
        metadata.insert(format!("APP12:{}", name), value);
    }
    Ok(metadata)
}

/// Walks the payload the way ExifTool's `ProcessAPP12` regex does, returning
/// each `tag=value` pair in order.
///
/// The Perl pattern is
/// `(\[.*?\]|[\w#-]+=[\x20-\x7e]+?(?=\s*([\n\r\0]|[\w#-]+=|\[|$)))` scanned
/// with `/g`: at every position it first tries a bracketed section header,
/// then a tag/value pair whose value is taken lazily up to the first point
/// where the next thing is a line break, a NUL, another `tag=`, a new
/// section, or the end of the data. Section headers only influence the
/// family-2 group in ExifTool, so they are recognised (to keep the scan
/// position in sync) but otherwise discarded.
fn scan_pairs(data: &[u8]) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut i = 0usize;

    while i < data.len() {
        // Alternative 1: "[section]" -- non-greedy, and `.` never matches a
        // newline, so the closing bracket has to appear on the same line.
        if data[i] == b'[' {
            let rest = &data[i + 1..];
            match rest.iter().position(|&b| b == b']' || b == b'\n') {
                Some(off) if rest[off] == b']' => i += off + 2,
                _ => i += 1,
            }
            continue;
        }

        // Alternative 2: "tag=value".
        if !is_tag_char(data[i]) {
            i += 1;
            continue;
        }
        // `[\w#-]+` is greedy; a shorter prefix is always followed by another
        // tag character rather than '=', so only the maximal run can match.
        let key_end = run_end(data, i);
        if data.get(key_end) != Some(&b'=') {
            // No '=' after the run: every start position inside the run fails
            // the same way, and the terminator itself is not a tag character.
            i = key_end.max(i + 1);
            continue;
        }

        let value_start = key_end + 1;
        match match_value(data, value_start) {
            Some(value_end) => {
                let key = String::from_utf8_lossy(&data[i..key_end]).into_owned();
                let value = String::from_utf8_lossy(&data[value_start..value_end]).into_owned();
                pairs.push((key, value));
                i = value_end;
            }
            None => i = key_end + 1,
        }
    }

    pairs
}

/// Exclusive end of the `[\w#-]+` run starting at `start`.
fn run_end(data: &[u8], start: usize) -> usize {
    data[start..]
        .iter()
        .position(|&b| !is_tag_char(b))
        .map_or(data.len(), |off| start + off)
}

/// Lazily matches `[\x20-\x7e]+?(?=\s*([\n\r\0]|[\w#-]+=|\[|$))` starting at
/// `start`, returning the exclusive end offset of the value.
fn match_value(data: &[u8], start: usize) -> Option<usize> {
    let mut end = start;
    while end < data.len() && is_value_char(data[end]) {
        end += 1;
        if lookahead_ok(data, end) {
            return Some(end);
        }
    }
    None
}

/// Evaluates the regex lookahead `\s*([\n\r\0]|[\w#-]+=|\[|$)` at `pos`.
fn lookahead_ok(data: &[u8], pos: usize) -> bool {
    let mut p = pos;
    while p < data.len() && is_perl_space(data[p]) {
        // `\s*` is greedy, but Perl backtracks, so a match is accepted as
        // soon as ANY amount of whitespace is followed by one of the
        // alternatives; each intermediate position has to be tested too.
        if alternation_ok(data, p) {
            return true;
        }
        p += 1;
    }
    alternation_ok(data, p)
}

/// Evaluates `([\n\r\0]|[\w#-]+=|\[|$)` at `pos`.
///
/// Perl's `$` (without `/m`) matches at the end of the string or just before
/// a string-final newline; the newline case is already covered by the
/// `[\n\r\0]` alternative.
fn alternation_ok(data: &[u8], pos: usize) -> bool {
    let Some(&b) = data.get(pos) else {
        return true; // end of data -> `$`
    };
    if matches!(b, b'\n' | b'\r' | 0 | b'[') {
        return true;
    }
    if !is_tag_char(b) {
        return false;
    }
    data.get(run_end(data, pos)) == Some(&b'=')
}

/// Applies ExifTool's `%APP12::PictureInfo` name and value conversions.
///
/// Returns the displayed tag name and the converted value. Keys without a
/// table entry fall through to `generic_tag_name`.
fn convert_tag(key: &str, value: &str) -> (String, TagValue) {
    match key {
        // ValueConv => '$val=~s/^[A-Za-z ]*//;$val' (Agfa leads with an 'F'),
        // PrintConv => 'sprintf("%.1f",$val)'
        "FNumber" => {
            let stripped = value.trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == ' ');
            ("FNumber".to_string(), print_float_1(stripped))
        }
        "Aperture" => ("Aperture".to_string(), print_float_1(value)),
        // ValueConv => '$val=~/^\d+$/ ? ConvertUnixTime($val) : $val'
        "TimeDate" => (
            "DateTimeOriginal".to_string(),
            TagValue::String(convert_unix_time(value)),
        ),
        // ValueConv => '$val * 1e-6', PrintConv => PrintExposureTime
        "Shutter" | "shtr" => (
            "ExposureTime".to_string(),
            TagValue::String(print_exposure_time(value)),
        ),
        "Serial#" => (
            "SerialNumber".to_string(),
            TagValue::String(value.to_string()),
        ),
        "Flash" | "Macro" => (key.to_string(), print_off_on(value)),
        "Ytarget" => ("YTarget".to_string(), TagValue::String(value.to_string())),
        "ylevel" => ("YLevel".to_string(), TagValue::String(value.to_string())),
        "ExpBias" => (
            "ExposureCompensation".to_string(),
            TagValue::String(value.to_string()),
        ),
        "FWare" => (
            "FirmwareVersion".to_string(),
            TagValue::String(value.to_string()),
        ),
        // PrintConv => '$val=~tr/-/x/;$val'
        "ImageSize" => (
            "ImageSize".to_string(),
            TagValue::String(value.replace('-', "x")),
        ),
        "Type" => (
            "CameraType".to_string(),
            TagValue::String(value.to_string()),
        ),
        // Table entries with no conversion; listed explicitly so they can
        // never be reached through the generic-naming path.
        "StrobeTime" | "FocusPos" | "FocusMode" | "Quality" | "Resolution" | "Protect"
        | "ContTake" | "ColorMode" | "Zoom" | "ZoomPos" | "LightS" | "Version" | "ID" => {
            (key.to_string(), TagValue::String(value.to_string()))
        }
        _ => (generic_tag_name(key), TagValue::String(value.to_string())),
    }
}

/// ExifTool's naming rules for a key with no table entry
/// (`ProcessAPP12`'s `ucfirst $tag` plus `AddTagToTable`, ExifTool.pm:9234).
fn generic_tag_name(key: &str) -> String {
    // tr/-_a-zA-Z0-9//dc -- delete every character outside the class
    let mut name: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    // ucfirst
    if !name.is_empty() {
        name[..1].make_ascii_uppercase();
    }
    // 'tag names must be at least 2 characters long and prefer them to start
    // with a letter'
    if name.len() < 2 || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name.insert_str(0, "Tag");
    }
    name
}

/// `sprintf("%.1f", $val)`; a non-numeric value is passed through unchanged
/// rather than being coerced to a number.
fn print_float_1(value: &str) -> TagValue {
    match value.trim().parse::<f64>() {
        Ok(v) => TagValue::String(format!("{:.1}", v)),
        Err(_) => TagValue::String(value.to_string()),
    }
}

/// `PrintConv => { 0 => 'Off', 1 => 'On' }`. A code with no entry reports
/// itself as ExifTool does rather than being rounded to a neighbour.
fn print_off_on(value: &str) -> TagValue {
    match value.trim() {
        "0" => TagValue::String("Off".to_string()),
        "1" => TagValue::String("On".to_string()),
        other => TagValue::String(format!("Unknown ({})", other)),
    }
}

/// `ConvertUnixTime($val)` for an all-digit value, otherwise the raw string.
///
/// ExifTool's `ConvertUnixTime` without the `$isLocal` flag uses `gmtime`,
/// and the default `ConvertDateTime` prints "YYYY:MM:DD HH:MM:SS".
fn convert_unix_time(value: &str) -> String {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return value.to_string();
    }
    let Ok(secs) = value.parse::<i64>() else {
        return value.to_string();
    };
    // "1998-12-31T15:17:20Z" -> "1998:12:31 15:17:20"
    unix_to_iso8601(secs)
        .replace('-', ":")
        .replace('T', " ")
        .replace('Z', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Agfa APP12 segment from ExifTool's own combined-samples
    /// ExifTool.jpg, trimmed but otherwise verbatim (vendor banner,
    /// sections and all).
    const AGFA_SEGMENT: &[u8] = b"Agfa Gevaert   \x00 721\r\n\
[picture info]\r\n\
TimeDate=915117440\r\n\
Shutter=6460\r\n\
Flash=0\r\n\
Resolution=5\r\n\
Protect=0\r\n\
ContTake=0\r\n\
ImageSize=1280-960\r\n\
ColorMode=1\r\n\
FNumber=F11\r\n\
Zoom=2.1\r\n\
Macro=0\r\n\
LightS=0\r\n\
ExpBias=+2.0\r\n\
[camera info]\r\n\
Type=SR84\r\n\
Serial#=#00000001\r\n\
Version=v84-71\r\n\
ID=AGFA DIGITAL CAMERA\r\n\
[diag info]\r\n\
PicLen=561039\r\n\
ThmLen=3802\r\n\
Q=96\r\n\
R=293\r\n\
B=332\r\n\
s0=1e8,0,11b0,6f72\r\n\
T0=11b15600,1290000\r\n\
[end]\r\n";

    /// Every assertion below was taken from
    /// `exiftool -s -G0 combined-samples/ExifTool.jpg` (ExifTool 13.55).
    #[test]
    fn test_agfa_segment_matches_exiftool() {
        let m = parse_app12_picture_info(AGFA_SEGMENT).unwrap();

        // Table entries with conversions
        assert_eq!(
            m.get_string("APP12:DateTimeOriginal"),
            Some("1998:12:31 15:17:20")
        );
        assert_eq!(m.get_string("APP12:ExposureTime"), Some("1/155"));
        assert_eq!(m.get_string("APP12:FNumber"), Some("11.0"));
        assert_eq!(m.get_string("APP12:ImageSize"), Some("1280x960"));
        assert_eq!(m.get_string("APP12:Flash"), Some("Off"));
        assert_eq!(m.get_string("APP12:Macro"), Some("Off"));
        assert_eq!(m.get_string("APP12:CameraType"), Some("SR84"));
        assert_eq!(m.get_string("APP12:SerialNumber"), Some("#00000001"));
        assert_eq!(m.get_string("APP12:ExposureCompensation"), Some("+2.0"));

        // Table entries without conversions
        assert_eq!(m.get_string("APP12:Resolution"), Some("5"));
        assert_eq!(m.get_string("APP12:Protect"), Some("0"));
        assert_eq!(m.get_string("APP12:ContTake"), Some("0"));
        assert_eq!(m.get_string("APP12:ColorMode"), Some("1"));
        assert_eq!(m.get_string("APP12:Zoom"), Some("2.1"));
        assert_eq!(m.get_string("APP12:LightS"), Some("0"));
        assert_eq!(m.get_string("APP12:Version"), Some("v84-71"));
        assert_eq!(m.get_string("APP12:ID"), Some("AGFA DIGITAL CAMERA"));

        // Keys with no table entry, named by AddTagToTable's generic rules
        assert_eq!(m.get_string("APP12:PicLen"), Some("561039"));
        assert_eq!(m.get_string("APP12:ThmLen"), Some("3802"));
        assert_eq!(m.get_string("APP12:TagQ"), Some("96"));
        assert_eq!(m.get_string("APP12:TagR"), Some("293"));
        assert_eq!(m.get_string("APP12:TagB"), Some("332"));
        assert_eq!(m.get_string("APP12:S0"), Some("1e8,0,11b0,6f72"));
        assert_eq!(m.get_string("APP12:T0"), Some("11b15600,1290000"));

        // The vendor banner and the section headers are not tags
        assert!(m.get("APP12:Agfa").is_none());
        assert!(m.get("APP12:Gevaert").is_none());
    }

    #[test]
    fn test_generic_tag_naming_rules() {
        // ucfirst
        assert_eq!(generic_tag_name("s0"), "S0");
        assert_eq!(generic_tag_name("picLen"), "PicLen");
        // shorter than two characters -> "Tag" prefix
        assert_eq!(generic_tag_name("Q"), "TagQ");
        assert_eq!(generic_tag_name("b"), "TagB");
        // does not start with a letter -> "Tag" prefix
        assert_eq!(generic_tag_name("3d"), "Tag3d");
        // illegal characters are dropped before the length test
        assert_eq!(generic_tag_name("a#"), "TagA");
    }

    #[test]
    fn test_unknown_enum_codes_report_themselves() {
        let m = parse_app12_picture_info(b"Flash=7\r\nMacro=9\r\n").unwrap();
        assert_eq!(m.get_string("APP12:Flash"), Some("Unknown (7)"));
        assert_eq!(m.get_string("APP12:Macro"), Some("Unknown (9)"));
    }

    #[test]
    fn test_values_may_contain_spaces() {
        // ExifTool's lazy value match runs to the last printable character
        // before the line break, so embedded spaces are kept.
        let m = parse_app12_picture_info(b"ID=AGFA DIGITAL CAMERA\r\nType=SR84\r\n").unwrap();
        assert_eq!(m.get_string("APP12:ID"), Some("AGFA DIGITAL CAMERA"));
        assert_eq!(m.get_string("APP12:CameraType"), Some("SR84"));
    }

    #[test]
    fn test_binary_tail_is_ignored() {
        // Real Agfa segments end with a binary "[file info]" block; it has no
        // '=' so nothing there can match the scan pattern.
        let mut data = b"[picture info]\r\nZoom=2.1\r\n[file info]\x00".to_vec();
        data.extend_from_slice(&[0xa0, 0x00, 0xff, 0x8b, 0x95, 0x80, 0x00, 0x08]);
        let m = parse_app12_picture_info(&data).unwrap();
        assert_eq!(m.get_string("APP12:Zoom"), Some("2.1"));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn test_shutter_alias_and_exposure_rounding() {
        // 'shtr' is the same tag as 'Shutter' in ExifTool's table.
        let m = parse_app12_picture_info(b"shtr=6460\r\n").unwrap();
        assert_eq!(m.get_string("APP12:ExposureTime"), Some("1/155"));
        // Exposures at or above 1/4 s print as seconds, not a fraction.
        let m = parse_app12_picture_info(b"Shutter=500000\r\n").unwrap();
        assert_eq!(m.get_string("APP12:ExposureTime"), Some("0.5"));
        let m = parse_app12_picture_info(b"Shutter=1000000\r\n").unwrap();
        assert_eq!(m.get_string("APP12:ExposureTime"), Some("1"));
    }

    #[test]
    fn test_non_numeric_timedate_passes_through() {
        // ValueConv only converts an all-digit value.
        let m = parse_app12_picture_info(b"TimeDate=2005:01:02 03:04:05\r\n").unwrap();
        assert_eq!(
            m.get_string("APP12:DateTimeOriginal"),
            Some("2005:01:02 03:04:05")
        );
    }

    #[test]
    fn test_segment_without_pairs_yields_nothing() {
        // ExifTool reports no tags (and no Picture Info dump type) when the
        // scan finds nothing.
        let m = parse_app12_picture_info(b"\x00\x01\x02\x03\x04\x05\x06\x07").unwrap();
        assert!(m.is_empty());
    }

    #[test]
    fn test_segment_too_short() {
        assert!(parse_app12_picture_info(b"A=").is_err());
    }

    #[test]
    fn test_unterminated_section_header_does_not_swallow_pairs() {
        // `.` never matches a newline, so an unclosed '[' cannot consume the
        // following line.
        let m = parse_app12_picture_info(b"[unclosed\r\nZoom=2.1\r\n").unwrap();
        assert_eq!(m.get_string("APP12:Zoom"), Some("2.1"));
    }
}
