//! `Olympus::TextInfo` -- the MakerNote 0x0208 sub-directory.
//!
//! Older Olympus bodies store a short ASCII record inside the MakerNote:
//!
//! ```text
//! [pictureInfo] Resolution=3 [Camera Info] Type=SR951\0
//! ```
//!
//! ExifTool runs it through the same scanner as the JPEG APP12 "Picture Info"
//! segment:
//!
//! ```text
//! Olympus.pm:1573  %Image::ExifTool::Olympus::TextInfo = (
//! Olympus.pm:1574      PROCESS_PROC => \&Image::ExifTool::APP12::ProcessAPP12,
//! Olympus.pm:1576          This information is in text format (similar to APP12 information, but with
//! Olympus.pm:1577          spaces instead of linefeeds).
//! ```
//!
//! Those spaces are why this needs the real scanner rather than the
//! delimiter-splitting readers in `parsers::jpeg::app_segments`: the whole
//! record above is a single "line", so splitting on CR/LF/NUL yields one token
//! and no tags at all.
//!
//! The table itself names two fields and accepts anything else:
//!
//! ```text
//! Olympus.pm:1578          any information found here will be extracted, even if the tag is not listed.
//! Olympus.pm:1581      Resolution => { },
//! Olympus.pm:1582      Type => {
//! Olympus.pm:1583          Name => 'CameraType',
//! Olympus.pm:1585          DataMember => 'CameraType',
//! Olympus.pm:1586          RawConv => '$self->{CameraType} = $val',
//! Olympus.pm:1588          PrintConv => \%olympusCameraTypes,
//! ```

use std::collections::HashMap;

use super::ifd::list_lookup_or_unknown;
use super::lookups::CAMERA_TYPE2;

/// Perl's `\w` under ASCII semantics.
fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// One character of ExifTool's `[\w#-]+` field-name class.
fn is_name_byte(b: u8) -> bool {
    is_word(b) || b == b'#' || b == b'-'
}

/// Perl's `\s`: space, tab, newline, form feed, carriage return and (since
/// 5.18) vertical tab.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// A `[\w#-]+=` run starting at `at`.
fn field_name_ends_with_eq(data: &[u8], at: usize) -> bool {
    let mut j = at;
    while j < data.len() && is_name_byte(data[j]) {
        j += 1;
    }
    j > at && j < data.len() && data[j] == b'='
}

/// ExifTool's value terminator: `(?=\s*([\n\r\0]|[\w#-]+=|\[|$))`.
///
/// The lookahead is zero-width, so only whether *some* `\s*` length satisfies
/// it matters; this tries them shortest-first rather than Perl's greedy order.
fn value_ends_at(data: &[u8], at: usize) -> bool {
    let mut s = at;
    loop {
        // `$` -- end of string, or immediately before a string-final newline.
        if s >= data.len() {
            return true;
        }
        if matches!(data[s], b'\n' | b'\r' | 0) || data[s] == b'[' {
            return true;
        }
        if field_name_ends_with_eq(data, s) {
            return true;
        }
        if is_space(data[s]) {
            s += 1;
        } else {
            return false;
        }
    }
}

/// Port of ExifTool's `ProcessAPP12` scanner.
///
/// ```text
/// APP12.pm:262     while ($$dataPt =~ /(\[.*?\]|[\w#-]+=[\x20-\x7e]+?(?=\s*([\n\r\0]|[\w#-]+=|\[|$)))/g) {
/// ```
///
/// Section headers (`[Camera Info]`) are consumed and dropped -- ExifTool uses
/// them only to pick a family-2 group for dynamically added tags, which is not
/// modelled here. The returned pairs are in the order they appear.
pub fn scan(data: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        // `\[.*?\]` -- non-greedy, and `.` never matches a newline.
        if data[pos] == b'[' {
            if let Some(end) = (pos + 1..data.len())
                .take_while(|&i| data[i] != b'\n')
                .find(|&i| data[i] == b']')
            {
                pos = end + 1;
                continue;
            }
        }

        // `[\w#-]+=` -- the greedy run must land exactly on the '='; shorter
        // backtracked runs always end on another name byte, never on '='.
        let mut name_end = pos;
        while name_end < data.len() && is_name_byte(data[name_end]) {
            name_end += 1;
        }
        if name_end > pos && name_end < data.len() && data[name_end] == b'=' {
            // `[\x20-\x7e]+?` -- at least one printable byte, grown until the
            // terminator lookahead succeeds.
            let value_start = name_end + 1;
            let mut value_end = value_start;
            let matched = loop {
                if value_end >= data.len() || !(0x20..=0x7e).contains(&data[value_end]) {
                    break None;
                }
                value_end += 1;
                if value_ends_at(data, value_end) {
                    break Some(value_end);
                }
            };
            if let Some(value_end) = matched {
                out.push((
                    String::from_utf8_lossy(&data[pos..name_end]).into_owned(),
                    String::from_utf8_lossy(&data[value_start..value_end]).into_owned(),
                ));
                pos = value_end;
                continue;
            }
        }

        pos += 1;
    }
    out
}

/// Perl's `ucfirst`: upper-case the first character, leave the rest alone.
fn ucfirst(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// The name ExifTool gives a field the table does not list.
///
/// ```text
/// APP12.pm:278             $tagInfo = { Name => ucfirst $tag };
/// ExifTool.pm:9234     $name =~ tr/-_a-zA-Z0-9//dc;    # remove illegal characters
/// ExifTool.pm:9235     $name = ucfirst $name;          # capitalize first letter
/// ExifTool.pm:9243     $name = "Tag$name" if length($name) < 2 or $name !~ /^[A-Z]/i;
/// ```
fn dynamic_tag_name(field: &str) -> Option<String> {
    let stripped: String = ucfirst(field)
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    let mut name = ucfirst(&stripped);
    if name.is_empty() {
        return None;
    }
    if name.len() < 2 || !name.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
        name = format!("Tag{}", name);
    }
    Some(name)
}

/// Extract one `Olympus::TextInfo` record into `Olympus:*` tags.
///
/// Returns the raw `Type` value, which ExifTool records as the `CameraType`
/// data member and later consults when converting `Quality`.
pub fn parse(data: &[u8], tags: &mut HashMap<String, String>) -> Option<String> {
    let mut camera_type = None;
    for (field, value) in scan(data) {
        match field.as_str() {
            // `Type => { Name => 'CameraType', PrintConv => \%olympusCameraTypes }`
            "Type" => {
                tags.insert(
                    "Olympus:CameraType".to_string(),
                    list_lookup_or_unknown(CAMERA_TYPE2, &value),
                );
                camera_type = Some(value);
            }
            // `Resolution => { }` -- no conversion of any kind.
            "Resolution" => {
                tags.insert("Olympus:Resolution".to_string(), value);
            }
            _ => {
                if let Some(name) = dynamic_tag_name(&field) {
                    tags.insert(format!("Olympus:{}", name), value);
                }
            }
        }
    }
    camera_type
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_the_space_separated_olympus_record() {
        // OlympusC2000Z.jpg, MakerNote tag 0x0208.
        let pairs = scan(b"[pictureInfo] Resolution=3 [Camera Info] Type=SR951\0");
        assert_eq!(
            pairs,
            vec![
                ("Resolution".to_string(), "3".to_string()),
                ("Type".to_string(), "SR951".to_string()),
            ]
        );
    }

    #[test]
    fn a_bracket_terminates_a_value_without_intervening_space() {
        // OlympusSX351 bodies write "Resolution=11[Camera Info]" with no gap;
        // the `\[` branch of the lookahead is what stops the value at "11".
        let pairs = scan(b"[pictureInfo] Resolution=11[Camera Info] Type=SX351\0");
        assert_eq!(pairs[0], ("Resolution".to_string(), "11".to_string()));
        assert_eq!(pairs[1], ("Type".to_string(), "SX351".to_string()));
    }

    #[test]
    fn values_may_contain_spaces() {
        // `[\x20-\x7e]+?` admits spaces, so only the lookahead ends the value.
        let pairs = scan(b"ID=OLYMPUS DIGITAL CAMERA\0");
        assert_eq!(
            pairs,
            vec![("ID".to_string(), "OLYMPUS DIGITAL CAMERA".to_string())]
        );
    }

    #[test]
    fn trailing_nul_padding_is_not_part_of_the_value() {
        let pairs = scan(b"[Camera Info] Type=D4406\0\0\0\0\0\0\0\0\0");
        assert_eq!(pairs, vec![("Type".to_string(), "D4406".to_string())]);
    }

    #[test]
    fn an_empty_field_produces_no_pair() {
        // `[\x20-\x7e]+?` needs at least one printable byte.
        assert_eq!(scan(b"Serial#=\0Type=DCHT\0").len(), 1);
    }

    #[test]
    fn type_is_renamed_and_converted_through_the_camera_type_hash() {
        let mut tags = HashMap::new();
        let member = parse(
            b"[pictureInfo] Resolution=3 [Camera Info] Type=SR951\0",
            &mut tags,
        );

        assert_eq!(
            tags.get("Olympus:CameraType").map(String::as_str),
            Some("C2000Z")
        );
        assert_eq!(
            tags.get("Olympus:Resolution").map(String::as_str),
            Some("3")
        );
        // The data member is the raw field value, not the converted name.
        assert_eq!(member.as_deref(), Some("SR951"));
    }

    #[test]
    fn an_unlisted_body_code_prints_exiftools_unknown_form() {
        let mut tags = HashMap::new();
        parse(b"[Camera Info] Type=ZZ999\0", &mut tags);
        assert_eq!(
            tags.get("Olympus:CameraType").map(String::as_str),
            Some("Unknown (ZZ999)")
        );
    }

    #[test]
    fn unlisted_fields_are_added_under_their_ucfirst_name() {
        let mut tags = HashMap::new();
        parse(b"[pictureInfo] shtr=1000 Q=96\0", &mut tags);
        assert_eq!(tags.get("Olympus:Shtr").map(String::as_str), Some("1000"));
        // ExifTool.pm:9243 prefixes names shorter than two characters.
        assert_eq!(tags.get("Olympus:TagQ").map(String::as_str), Some("96"));
    }
}
