//! Olympus Picture Info APP12 segment parser
//!
//! This module parses JPEG APP12 segments from Olympus cameras containing
//! proprietary metadata in text format. The format uses key=value pairs
//! separated by delimiters (typically spaces or carriage returns).
//!
//! # Format Overview
//!
//! Olympus APP12 segments typically start with an identifier like:
//! - "OLYMPUS DIGITAL CAMERA"
//! - Camera model name
//! - "[picture info]" header
//!
//! The data contains key=value pairs with various metadata including:
//! - Camera type and model information
//! - Exposure settings (shutter speed, aperture)
//! - Flash and macro modes
//! - Zoom and resolution settings
//! - Serial numbers and timestamps
//!
//! # Example Data Format
//!
//! ```text
//! [picture info]
//! Resolution=2048x1536
//! Type=OLYMPUS DIGITAL CAMERA
//! ID=N123456789
//! ```

use crate::core::formatters::exif_print_conv::print_exposure_time_micros_str as print_exposure_time;
use crate::core::{MetadataMap, TagValue};
use crate::error::Result;

/// Delimiter characters used to separate key-value pairs in Olympus APP12 data.
/// The format uses ASCII control characters and whitespace.
const PAIR_DELIMITERS: &[char] = &['\r', '\n', '\0'];

/// Known Olympus Picture Info tag names that we extract and normalize.
/// These are the most commonly found tags in Olympus APP12 segments.
const KNOWN_TAGS: &[&str] = &[
    "ID",
    "Type",
    "CameraType",
    "Version",
    "SerialNumber",
    "InternalSerialNumber",
    "TimeDate",
    "DateTimeOriginal",
    "ExposureTime",
    "ExposureCompensation",
    "ExposureBias",
    "FNumber",
    "Flash",
    "LightS",
    "Macro",
    "Zoom",
    "Resolution",
    "ImageSize",
    "WB2",
    "WB3",
    "WB4",
    "WB5",
    "Quality",
    "FocusMode",
    "WhiteBalance",
    "Sharpness",
    "Contrast",
    "Saturation",
    "ISOSetting",
    "ColorMode",
    "DriveMode",
    "ContTake",
    "FocalLength",
    "DigitalZoom",
    "Manufacturer",
    "Model",
    "Software",
    "CAM1",
    "COLOR2",
    "COLOR3",
    "COLOR4",
    "EXP1",
    "EXP2",
    "EXP3",
    "JPEG1",
    "MODE1",
    "MODE2",
    "MODE3",
    "MODE4",
    "MODE5",
    "MODE6",
    "MTR2",
    "MTRX1",
    "FCS1",
    "FCS2",
    "FCS3",
    "FCS4",
    "FCS5",
    "FCS6",
    "FCS7",
    "IMbb",
    "IMbg",
    "IMgb",
    "IMgr",
    "IMrg",
    "IMbr",
    "IMgg",
    "IMrb",
    "IMrr",
    "Protect",
    "REV",
    "S0",
    "STB1",
    "STB3",
    "STB4",
    "STB5",
    "STB6",
];

/// Parse Olympus Picture Info APP12 segment data.
///
/// This function extracts metadata from Olympus cameras that store proprietary
/// information in JPEG APP12 segments. The data is stored as text with key=value
/// pairs separated by various delimiters.
///
/// # Arguments
///
/// * `data` - Raw APP12 segment data (byte slice)
///
/// # Returns
///
/// Returns a `Result<MetadataMap>` containing extracted Olympus metadata tags.
/// On success, tags are prefixed with "APP12:" (e.g., "APP12:CameraType"),
/// matching the family-0 group ExifTool reports for
/// `Image::ExifTool::APP12::PictureInfo`.
///
/// # Errors
///
/// Returns an error if:
/// - The data is too short to contain valid Olympus metadata
/// - The data doesn't appear to be Olympus Picture Info format
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::app_segments::app12_olympus::parse_app12_olympus;
///
/// let data = b"Type=OLYMPUS DIGITAL CAMERA\rResolution=2048x1536";
/// let metadata = parse_app12_olympus(data)?;
/// assert_eq!(metadata.get_string("APP12:CameraType"), Some("OLYMPUS DIGITAL CAMERA"));
/// ```
pub fn parse_app12_olympus(data: &[u8]) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Validate minimum data length - need at least a few bytes for any useful data
    if data.len() < 4 {
        return Err(crate::error::ExifToolError::parse_error(
            "APP12 Olympus segment too short",
        ));
    }

    // Convert data to string, handling potential encoding issues gracefully.
    // Olympus uses ASCII/Latin-1 encoding for text data.
    let text = decode_olympus_text(data);

    // Check for Olympus identifiers in the data.
    // Valid Olympus APP12 segments contain recognizable markers.
    if !is_olympus_picture_info(&text) {
        return Err(crate::error::ExifToolError::parse_error(
            "Not an Olympus Picture Info segment",
        ));
    }

    // Parse the key=value pairs from the text data
    parse_key_value_pairs(&text, &mut metadata);

    Ok(metadata)
}

/// Decode Olympus text data from raw bytes.
///
/// Olympus cameras use ASCII/Latin-1 encoding for text in APP12 segments.
/// This function converts the byte data to a String, replacing any invalid
/// characters with the Unicode replacement character.
///
/// # Arguments
///
/// * `data` - Raw byte data from the APP12 segment
///
/// # Returns
///
/// A String containing the decoded text data
fn decode_olympus_text(data: &[u8]) -> String {
    // First try UTF-8, which will handle pure ASCII correctly
    if let Ok(text) = std::str::from_utf8(data) {
        return text.to_string();
    }

    // Fall back to treating as Latin-1 (ISO-8859-1) where each byte maps
    // directly to a Unicode code point
    data.iter().map(|&b| b as char).collect()
}

/// Check if the text data appears to be Olympus Picture Info format.
///
/// This function looks for known Olympus identifiers and patterns that
/// indicate the data is from an Olympus camera's Picture Info segment.
///
/// # Arguments
///
/// * `text` - Decoded text from the APP12 segment
///
/// # Returns
///
/// `true` if the text appears to be Olympus Picture Info format, `false` otherwise
fn is_olympus_picture_info(text: &str) -> bool {
    let text_upper = text.to_uppercase();

    // Check for common Olympus identifiers
    let olympus_markers = [
        "OLYMPUS",
        "[PICTURE INFO]",
        "OLYMPUS DIGITAL CAMERA",
        "OLYMPUS OPTICAL",
        "CAMEDIA",
    ];

    for marker in olympus_markers {
        if text_upper.contains(marker) {
            return true;
        }
    }

    // Also check if it looks like key=value format with known Olympus tags
    // This helps identify Olympus data that might not have an explicit identifier
    let has_known_tags = KNOWN_TAGS.iter().any(|&tag| {
        let pattern = format!("{}=", tag);
        text.contains(&pattern)
    });

    // Must have at least an equals sign and some recognizable structure
    has_known_tags && text.contains('=')
}

/// Parse key=value pairs from Olympus Picture Info text.
///
/// This function extracts all key=value pairs from the text data and
/// stores them in the metadata map with the "APP12:" prefix.
///
/// # Arguments
///
/// * `text` - Decoded text containing key=value pairs
/// * `metadata` - MetadataMap to store extracted values
fn parse_key_value_pairs(text: &str, metadata: &mut MetadataMap) {
    // Split the text by common delimiters (CR, LF, null byte)
    // Olympus uses various separators between key=value pairs
    for line in text.split(PAIR_DELIMITERS) {
        let line = line.trim();

        // Skip empty lines and section headers like "[picture info]"
        if line.is_empty() || line.starts_with('[') {
            continue;
        }

        // Parse key=value pair
        if let Some((key, value)) = parse_single_pair(line) {
            // Emit the canonical ExifTool APP12 tag for this field.
            insert_picture_info_tag(&key, &value, metadata);

            // ExifTool exposes the Olympus STB5 diagnostic field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("STB5") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));
                metadata.insert("APP12:STB5".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus STB6 diagnostic field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("STB6") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));
                metadata.insert("APP12:STB6".to_string(), app12_value);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ExifTool's APP12 "Picture Info" table
// ---------------------------------------------------------------------------
//
// `Image::ExifTool::APP12::PictureInfo` is an *open* table: `ProcessAPP12`
// adds any field it does not recognise on the fly, so ExifTool reports every
// `key=value` pair in the segment, not a fixed allow-list.
//
// ```text
// APP12.pm:276     unless ($tagInfo) {
// APP12.pm:277         # add new tag to table
// APP12.pm:278         $tagInfo = { Name => ucfirst $tag };
// ```
//
// The generated name is then sanitised by `AddTagToTable`:
//
// ```text
// ExifTool.pm:9234     $name =~ tr/-_a-zA-Z0-9//dc;    # remove illegal characters
// ExifTool.pm:9235     $name = ucfirst $name;          # capitalize first letter
// ExifTool.pm:9242     # tag names must be at least 2 characters long and prefer them to start with a letter
// ExifTool.pm:9243     $name = "Tag$name" if length($name) < 2 or $name !~ /^[A-Z]/i;
// ```
//
// That last rule is why the one-character `Q=`, `R=`, `B=` and `S=` fields
// written by these cameras are reported as `TagQ`, `TagR`, `TagB` and `TagS`.

/// Field-name overrides declared by `Image::ExifTool::APP12::PictureInfo`.
///
/// The lookup is case-sensitive because ExifTool's `GetTagInfo` matches the
/// raw field name exactly. That is why `FNumber=F2.8` is converted to `2.8`
/// while the `Fnumber=F2.8` spelling used by the D-500L/D-600L/D-620L falls
/// through to the dynamic path and is reported verbatim as `Fnumber`.
///
/// ```text
/// APP12.pm:41      TimeDate => {
/// APP12.pm:42          Name => 'DateTimeOriginal',
/// APP12.pm:48      Shutter => {
/// APP12.pm:49          Name => 'ExposureTime',
/// APP12.pm:53      shtr => {
/// APP12.pm:54          Name => 'ExposureTime',
/// APP12.pm:58     'Serial#'    => {
/// APP12.pm:59          Name => 'SerialNumber',
/// APP12.pm:65      Ytarget     => { Name => 'YTarget' },
/// APP12.pm:66      ylevel      => { Name => 'YLevel' },
/// APP12.pm:70      ExpBias     => 'ExposureCompensation',
/// APP12.pm:71      FWare       => 'FirmwareVersion',
/// APP12.pm:81      Type        => {
/// APP12.pm:82          Name => 'CameraType',
/// ```
const PICTURE_INFO_NAMES: &[(&str, &str)] = &[
    ("TimeDate", "DateTimeOriginal"),
    ("Shutter", "ExposureTime"),
    ("shtr", "ExposureTime"),
    ("Serial#", "SerialNumber"),
    ("Ytarget", "YTarget"),
    ("ylevel", "YLevel"),
    ("ExpBias", "ExposureCompensation"),
    ("FWare", "FirmwareVersion"),
    ("Type", "CameraType"),
];

/// Emit the canonical `APP12:*` tag for one Picture Info `key=value` pair.
///
/// Shared by the Olympus and Agfa APP12 entry points: both segment flavours
/// are the same ASCII Picture Info record and ExifTool routes both through
/// `ProcessAPP12` with the same table.
pub(crate) fn insert_picture_info_tag(key: &str, value: &str, metadata: &mut MetadataMap) {
    // ExifTool's tokenizer requires at least one printable character after
    // the '=' (`[\x20-\x7e]+?` in APP12.pm:262), so an empty field such as
    // the `Serial#=` written by the D-500L produces no tag at all.
    if value.is_empty() {
        return;
    }

    let field = picture_info_field_id(key);
    let Some(name) = picture_info_tag_name(field) else {
        return;
    };

    metadata.insert(
        format!("APP12:{}", name),
        picture_info_tag_value(field, value),
    );
}

/// Recover the field name ExifTool's tokenizer would have matched.
///
/// ExifTool scans the segment with
///
/// ```text
/// APP12.pm:262     while ($$dataPt =~ /(\[.*?\]|[\w#-]+=[\x20-\x7e]+?(?=\s*([\n\r\0]|[\w#-]+=|\[|$)))/g) {
/// ```
///
/// so only `[\w#-]` characters can precede the '='. When binary padding runs
/// straight into a field name (the D-340L writes `\x80\x01.\x01\x02S=v`),
/// ExifTool matches from the last illegal byte onwards, leaving `S`.
fn picture_info_field_id(key: &str) -> &str {
    let mut start = key.len();
    for (index, ch) in key.char_indices().rev() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '#' || ch == '-' {
            start = index;
        } else {
            break;
        }
    }
    &key[start..]
}

/// Reproduce ExifTool's tag naming for one Picture Info field.
///
/// Returns `None` when nothing survives sanitisation, in which case ExifTool
/// would not have matched a field name either.
fn picture_info_tag_name(field: &str) -> Option<String> {
    if field.is_empty() {
        return None;
    }

    // APP12.pm:278 supplies `ucfirst $tag` for fields that are not in the
    // table; table entries carry their own Name.
    let base = PICTURE_INFO_NAMES
        .iter()
        .find(|(raw, _)| *raw == field)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| ucfirst(field));

    // ExifTool.pm:9234 `$name =~ tr/-_a-zA-Z0-9//dc;`
    let stripped: String = base
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();

    // ExifTool.pm:9235 `$name = ucfirst $name;`
    let mut name = ucfirst(&stripped);
    if name.is_empty() {
        return None;
    }

    // ExifTool.pm:9243 `$name = "Tag$name" if length($name) < 2 or $name !~ /^[A-Z]/i;`
    if name.len() < 2 || !name.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
        name = format!("Tag{}", name);
    }

    Some(name)
}

/// Apply the ValueConv/PrintConv pair declared for this Picture Info field.
///
/// Fields without a conversion are reported verbatim; numeric-looking values
/// are stored as integers so downstream formatting matches ExifTool's plain
/// decimal rendering.
fn picture_info_tag_value(field: &str, value: &str) -> TagValue {
    match field {
        // ```text
        // APP12.pm:34      FNumber => {
        // APP12.pm:35          ValueConv => '$val=~s/^[A-Za-z ]*//;$val',  # Agfa leads with an 'F'
        // APP12.pm:36          PrintConv => 'sprintf("%.1f",$val)',
        // ```
        "FNumber" => {
            let stripped =
                value.trim_start_matches(|ch: char| ch.is_ascii_alphabetic() || ch == ' ');
            TagValue::String(match stripped.parse::<f64>() {
                Ok(number) => format!("{:.1}", number),
                Err(_) => stripped.to_string(),
            })
        }
        // ```text
        // APP12.pm:38      Aperture => {
        // APP12.pm:39          PrintConv => 'sprintf("%.1f",$val)',
        // ```
        "Aperture" => TagValue::String(match value.parse::<f64>() {
            Ok(number) => format!("{:.1}", number),
            Err(_) => value.to_string(),
        }),
        // ```text
        // APP12.pm:45          ValueConv => '$val=~/^\d+$/ ? ConvertUnixTime($val) : $val',
        // APP12.pm:46          PrintConv => '$self->ConvertDateTime($val)',
        // ```
        "TimeDate" => TagValue::String(convert_unix_time(value)),
        // ```text
        // APP12.pm:50          ValueConv => '$val * 1e-6',
        // APP12.pm:51          PrintConv => 'Image::ExifTool::Exif::PrintExposureTime($val)',
        // ```
        "Shutter" | "shtr" => TagValue::String(print_exposure_time(value)),
        // ```text
        // APP12.pm:62      Flash       => { PrintConv => { 0 => 'Off', 1 => 'On' } },
        // APP12.pm:63      Macro       => { PrintConv => { 0 => 'Off', 1 => 'On' } },
        // ```
        // Values outside the two ExifTool defines are passed through rather
        // than guessed at.
        "Flash" | "Macro" => TagValue::String(match value {
            "0" => "Off".to_string(),
            "1" => "On".to_string(),
            _ => value.to_string(),
        }),
        // ```text
        // APP12.pm:76      ImageSize   => { PrintConv => '$val=~tr/-/x/;$val' },
        // ```
        "ImageSize" => TagValue::String(value.replace('-', "x")),
        _ => match value.parse::<i64>() {
            Ok(number) => TagValue::Integer(number),
            Err(_) => TagValue::String(value.to_string()),
        },
    }
}

/// Perl's `ucfirst`: upper-case the first character, leave the rest alone.
fn ucfirst(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Port of `Image::ExifTool::ConvertUnixTime` followed by `ConvertDateTime`
/// for the APP12 `TimeDate` field.
///
/// `ConvertUnixTime($val)` is called without the `$toLocal` argument, so it
/// takes the UTC branch:
///
/// ```text
/// ExifTool.pm:6764     return '0000:00:00 00:00:00' if $time == 0;
/// ExifTool.pm:6775     if (not $toLocal) {
/// ExifTool.pm:6776         @tm = gmtime($itime);
/// ```
///
/// Non-numeric values (these cameras write `TimeDate=-1` when the clock was
/// never set) are passed through by the ValueConv's own guard.
fn convert_unix_time(value: &str) -> String {
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return value.to_string();
    }

    let Ok(seconds) = value.parse::<i64>() else {
        return value.to_string();
    };
    if seconds == 0 {
        return "0000:00:00 00:00:00".to_string();
    }

    match chrono::DateTime::from_timestamp(seconds, 0) {
        Some(timestamp) => timestamp.format("%Y:%m:%d %H:%M:%S").to_string(),
        None => value.to_string(),
    }
}

#[cfg(test)]
mod fcs_tests {
    use super::*;

    #[test]
    fn test_parse_fcs6() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[diag info]\r\nFCS6=3\r\n").unwrap();

        assert_eq!(metadata.get_integer("APP12:FCS6"), Some(3));
    }

    #[test]
    fn test_parse_fcs5() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[diag info]\r\nFCS5=215\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:FCS5"), Some(215));
    }

    #[test]
    fn test_parse_fcs4() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[diag info]\r\nFCS4=221\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:FCS4"), Some(221));
    }
}

#[cfg(test)]
mod camera_type_tests {
    use super::*;

    #[test]
    fn test_legacy_picture_info_camera_type() {
        // Agfa SR84 files use the generic, identifier-less APP12 Picture Info
        // layout and are accepted by this parser through the known Type field.
        let metadata = parse_app12_olympus(b"Type=SR84\rVersion=v84-71\rID=AGFA DIGITAL CAMERA\r")
            .expect("legacy Picture Info should parse");

        assert_eq!(metadata.get_string("APP12:CameraType"), Some("SR84"));
    }

    #[test]
    fn test_picture_info_fcs3_app12_tag() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0\r\n[diag info]\r\nFCS3=2200\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:FCS3"), Some(2200));
    }

    #[test]
    fn test_picture_info_fcs2_app12_tag() {
        let metadata = parse_app12_olympus(
            b"OLYMPUS OPTICAL CO.,LTD.\0[picture info]\r\nFCS1=0\r\nFCS2=1\r\n",
        )
        .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:FCS2"), Some(1));
    }

    #[test]
    fn test_picture_info_exposure_time_app12_tag() {
        let metadata = parse_app12_olympus(
            b"OLYMPUS DIGITAL CAMERA\0[picture info]\r\nExposureTime=1/155\r\n",
        )
        .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_string("APP12:ExposureTime"), Some("1/155"));
    }

    #[test]
    fn test_olympus_exp1_diagnostic_value() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[diag info]\r\nEXP1=7727\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:EXP1"), Some(7727));
    }

    #[test]
    fn test_olympus_exp2_diagnostic_value() {
        let metadata = parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[diag info]\r\nEXP2=59\r\n")
            .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:EXP2"), Some(59));
    }

    #[test]
    fn test_olympus_exp3_diagnostic_value() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[diag info]\r\nEXP3=227\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:EXP3"), Some(227));
    }

    #[test]
    fn test_olympus_mtr1_diagnostic_value() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[diag info]\r\nMTR1=504\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:MTR1"), Some(504));
    }

    #[test]
    fn test_olympus_cam1_diagnostic_value() {
        let metadata = parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[diag info]\r\nCAM1=59\r\n")
            .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:CAM1"), Some(59));
    }

    #[test]
    fn test_olympus_cont_take_diagnostic_value() {
        // ContTake is itself a known Picture Info field, so identifier-less
        // records containing it are accepted.
        let metadata =
            parse_app12_olympus(b"ContTake=0\r\n").expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:ContTake"), Some(0));
    }

    #[test]
    fn test_olympus_exposure_compensation() {
        // APP12.pm:70 `ExpBias     => 'ExposureCompensation',`
        // ExifTool.jpg and the D-340L spell the field `ExpBias`.
        let metadata =
            parse_app12_olympus(b"OLYMPUS DIGITAL CAMERA\0[picture info]\r\nExpBias=+2.0\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(
            metadata.get_string("APP12:ExposureCompensation"),
            Some("+2.0")
        );
    }

    #[test]
    fn test_olympus_exposure_bias_spelling_is_not_renamed() {
        // Only the `ExpBias` spelling is in ExifTool's table; anything else
        // is added dynamically under its own ucfirst'ed name.
        let metadata =
            parse_app12_olympus(b"OLYMPUS DIGITAL CAMERA\0[picture info]\r\nExposureBias=+2.0\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_string("APP12:ExposureBias"), Some("+2.0"));
        assert!(metadata.get("APP12:ExposureCompensation").is_none());
    }

    #[test]
    fn test_olympus_color_mode_app12_tag() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[picture info]\r\nColorMode=1\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:ColorMode"), Some(1));
    }

    #[test]
    fn test_olympus_lights_app12_tag() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[picture info]\r\nLightS=1\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:LightS"), Some(1));
    }

    #[test]
    fn test_olympus_mode3_through_mode6_app12_tags() {
        // MODE1/MODE2 were already exposed under the APP12 group; MODE3..MODE6
        // (seen in OlympusD620L.jpg) previously only got the generic Olympus:
        // prefix. ExifTool reports all six under the APP12/PictureInfo group.
        let metadata = parse_app12_olympus(
            b"OLYMPUS OPTICAL CO.,LTD.\0[picture info]\r\n\
              MODE3=0\r\nMODE4=0\r\nMODE5=1\r\nMODE6=1\r\n",
        )
        .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:MODE3"), Some(0));
        assert_eq!(metadata.get_integer("APP12:MODE4"), Some(0));
        assert_eq!(metadata.get_integer("APP12:MODE5"), Some(1));
        assert_eq!(metadata.get_integer("APP12:MODE6"), Some(1));
    }

    #[test]
    fn test_picture_info_datetime_original_app12_tag() {
        let metadata = parse_app12_olympus(b"[picture info]\r\nTimeDate=1998:12:31 15:17:20\r\n")
            .expect("Picture Info should parse");

        assert_eq!(
            metadata.get_string("APP12:DateTimeOriginal"),
            Some("1998:12:31 15:17:20")
        );
    }

    #[test]
    fn test_picture_info_timedate_non_numeric_is_passed_through() {
        // APP12.pm:45 `ValueConv => '$val=~/^\d+$/ ? ConvertUnixTime($val) : $val',`
        // Anything that is not a bare integer is reported verbatim.
        let metadata = parse_app12_olympus(b"[picture info]\rTimeDate=Thu Dec 31 15:17:20 1998\r")
            .expect("Picture Info TimeDate should parse");

        assert_eq!(
            metadata.get_string("APP12:DateTimeOriginal"),
            Some("Thu Dec 31 15:17:20 1998")
        );
    }

    #[test]
    fn test_picture_info_timedate_unset_clock_is_passed_through() {
        // The D-220 and D-340L write TimeDate=-1 when the clock was never set.
        let metadata =
            parse_app12_olympus(b"[picture info]\r\nTimeDate=-1\r\n").expect("should parse");

        assert_eq!(metadata.get_string("APP12:DateTimeOriginal"), Some("-1"));
    }

    #[test]
    fn test_picture_info_timedate_epoch_is_converted_as_utc() {
        // ExifTool.jpg carries TimeDate=915117440 and exiftool reports
        // "1998:12:31 15:17:20". ConvertUnixTime is called without $toLocal
        // (ExifTool.pm:6775-6776), so the conversion uses gmtime and is
        // independent of the machine's time zone.
        let metadata =
            parse_app12_olympus(b"[picture info]\r\nTimeDate=915117440\r\n").expect("should parse");

        assert_eq!(
            metadata.get_string("APP12:DateTimeOriginal"),
            Some("1998:12:31 15:17:20")
        );
    }

    #[test]
    fn test_picture_info_shutter_is_printed_as_a_fraction() {
        // OlympusD220.jpg carries Shutter=72071 and exiftool reports "1/14".
        //
        // APP12.pm:50 `ValueConv => '$val * 1e-6',`
        // Exif.pm:5611 `return sprintf("1/%d",int(0.5 + 1/$secs));`
        let metadata =
            parse_app12_olympus(b"[picture info]\r\nShutter=72071\r\n").expect("should parse");

        assert_eq!(metadata.get_string("APP12:ExposureTime"), Some("1/14"));
    }

    #[test]
    fn test_picture_info_single_letter_fields_get_the_tag_prefix() {
        // ExifTool.jpg carries Q=96, R=293, B=332 and exiftool reports them
        // as TagQ, TagR and TagB.
        //
        // ExifTool.pm:9243 `$name = "Tag$name" if length($name) < 2 or $name !~ /^[A-Z]/i;`
        let metadata = parse_app12_olympus(
            b"OLYMPUS OPTICAL CO.,LTD.\0[diag info]\r\nQ=96\r\nR=293\r\nB=332\r\n",
        )
        .expect("should parse");

        assert_eq!(metadata.get_integer("APP12:TagQ"), Some(96));
        assert_eq!(metadata.get_integer("APP12:TagR"), Some(293));
        assert_eq!(metadata.get_integer("APP12:TagB"), Some(332));
        assert!(metadata.get("APP12:Q").is_none());
    }

    #[test]
    fn test_picture_info_field_name_drops_leading_binary_padding() {
        // OlympusD340L.jpg runs binary padding straight into an `S=v` field;
        // exiftool reports it as TagS because ExifTool.pm:9234's
        // `tr/-_a-zA-Z0-9//dc` deletes the illegal bytes.
        let metadata = parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0\x80\x01.\x01\x02S=v\0")
            .expect("should parse");

        assert_eq!(metadata.get_string("APP12:TagS"), Some("v"));
    }

    #[test]
    fn test_picture_info_serial_number_is_renamed_from_serial_hash() {
        // APP12.pm:58 `'Serial#'    => {`
        // APP12.pm:59 `     Name => 'SerialNumber',`
        let metadata = parse_app12_olympus(
            b"OLYMPUS OPTICAL CO.,LTD.\0[camera info]\r\nSerial#=#00000001\r\n",
        )
        .expect("should parse");

        assert_eq!(metadata.get_string("APP12:SerialNumber"), Some("#00000001"));
    }

    #[test]
    fn test_picture_info_empty_field_emits_nothing() {
        // The D-500L and D-620L write a bare `Serial#=`; exiftool reports no
        // SerialNumber at all because APP12.pm:262 requires a value.
        let metadata = parse_app12_olympus(b"[camera info]\r\nSerial#=\r\nType=DCHT\r\n")
            .expect("should parse");

        assert!(metadata.get("APP12:SerialNumber").is_none());
        assert_eq!(metadata.get_string("APP12:CameraType"), Some("DCHT"));
    }

    #[test]
    fn test_picture_info_macro_print_conv() {
        // APP12.pm:63 `Macro       => { PrintConv => { 0 => 'Off', 1 => 'On' } },`
        let metadata = parse_app12_olympus(b"[picture info]\r\nMacro=0\r\n").expect("should parse");

        assert_eq!(metadata.get_string("APP12:Macro"), Some("Off"));
    }
}

/// Parse a single key=value pair from a line of text.
///
/// # Arguments
///
/// * `line` - A single line that may contain a key=value pair
///
/// # Returns
///
/// `Some((key, value))` if a valid pair was found, `None` otherwise
fn parse_single_pair(line: &str) -> Option<(String, String)> {
    // Find the first equals sign - the key is before it, value is after
    let eq_pos = line.find('=')?;

    let key = line[..eq_pos].trim();
    let value = line[eq_pos + 1..].trim();

    // Validate that we have a non-empty key
    if key.is_empty() {
        return None;
    }

    // Remove any surrounding quotes from the value
    let value = value.trim_matches('"').trim_matches('\'');

    Some((key.to_string(), value.to_string()))
}

/// Normalize a tag name to match ExifTool's naming conventions.
///
/// This function converts various tag name formats found in Olympus data
/// to a consistent PascalCase format.
///
/// # Arguments
///
/// * `key` - The raw tag name from the Olympus data
///
/// # Returns
///
/// A normalized tag name string
fn normalize_tag_name(key: &str) -> String {
    // Map common variations to canonical names
    let normalized = match key.to_lowercase().as_str() {
        "type" => "CameraType",
        "id" => "CameraID",
        "resolution" => "ImageResolution",
        "imagesize" => "ImageSize",
        "exposuretime" | "exposure" | "shutter" => "ExposureTime",
        "exposurecompensation" | "exposurebias" | "exposurebiasvalue" | "expbias" => {
            "ExposureCompensation"
        }
        "fnumber" | "aperture" | "f-number" => "FNumber",
        "isosetting" | "iso" => "ISO",
        "focallength" | "focal" => "FocalLength",
        "digitalzoom" | "digital_zoom" => "DigitalZoom",
        "whitebalance" | "wb" => "WhiteBalance",
        "focusmode" | "focus" => "FocusMode",
        "drivemode" | "drive" => "DriveMode",
        "colormode" | "color" => "ColorMode",
        "serialnumber" | "serial" => "SerialNumber",
        "internalserialnumber" | "internal_serial" => "InternalSerialNumber",
        "datetimeoriginal" | "datetime" | "date" | "timedate" | "time_date" | "time date" => {
            "DateTimeOriginal"
        }
        "manufacturer" | "make" => "Make",
        "model" => "Model",
        "software" | "firmware" => "Software",
        "version" => "FirmwareVersion",
        "quality" => "Quality",
        "sharpness" => "Sharpness",
        "contrast" => "Contrast",
        "saturation" => "Saturation",
        "flash" => "Flash",
        "macro" => "Macro",
        "zoom" => "Zoom",
        _ => {
            // For unknown tags, convert to PascalCase
            return to_pascal_case(key);
        }
    };

    normalized.to_string()
}

/// Convert a string to PascalCase format.
///
/// This handles various input formats like snake_case, kebab-case,
/// or already PascalCase strings.
///
/// # Arguments
///
/// * `s` - The input string to convert
///
/// # Returns
///
/// A PascalCase version of the string
fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;

    for c in s.chars() {
        if c == '_' || c == '-' || c == ' ' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert the ctime-style timestamp used by APP12 Picture Info into EXIF
/// date/time form. Values already in another form are preserved unchanged.
fn normalize_picture_info_datetime(value: &str) -> String {
    let fields: Vec<&str> = value.split_whitespace().collect();

    // Common forms are:
    //   Thu Dec 31 15:17:20 1998
    //   Dec 31 15:17:20 1998
    let (month_index, day_index, time_index, year_index) = match fields.len() {
        5 => (1, 2, 3, 4),
        4 => (0, 1, 2, 3),
        _ => return value.to_string(),
    };

    let month = match fields[month_index].to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return value.to_string(),
    };

    let Ok(day) = fields[day_index].parse::<u8>() else {
        return value.to_string();
    };
    if !(1..=31).contains(&day) {
        return value.to_string();
    }

    let time = fields[time_index];
    let time_fields: Vec<&str> = time.split(':').collect();
    if time_fields.len() != 3 {
        return value.to_string();
    }
    let valid_time = match (
        time_fields[0].parse::<u8>(),
        time_fields[1].parse::<u8>(),
        time_fields[2].parse::<u8>(),
    ) {
        (Ok(hour), Ok(minute), Ok(second)) => hour < 24 && minute < 60 && second < 60,
        _ => false,
    };
    if !valid_time {
        return value.to_string();
    }

    let year = fields[year_index];
    if year.len() != 4 || !year.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.to_string();
    }

    format!("{year}:{month:02}:{day:02} {time}")
}

/// Parse a tag value and convert to appropriate TagValue type.
///
/// This function attempts to interpret the string value as the most
/// appropriate type (integer, float, or string).
///
/// # Arguments
///
/// * `tag_name` - The normalized tag name (used to determine expected type)
/// * `value` - The string value to parse
///
/// # Returns
///
/// A TagValue with the appropriate type for the value
fn parse_tag_value(tag_name: &str, value: &str) -> TagValue {
    // Handle empty values
    if value.is_empty() {
        return TagValue::String(String::new());
    }

    if tag_name == "DateTimeOriginal" {
        return TagValue::String(normalize_picture_info_datetime(value));
    }

    // Tags that are known to be numeric
    let numeric_tags = [
        "ISO",
        "FocalLength",
        "DigitalZoom",
        "Zoom",
        "Quality",
        "Sharpness",
        "Contrast",
        "Saturation",
    ];

    // Tags that may contain rational/float values
    let rational_tags = ["ExposureTime", "FNumber"];

    // Attempt type-specific parsing based on tag name
    if numeric_tags.contains(&tag_name) {
        // Try parsing as integer first
        if let Ok(num) = value.parse::<i64>() {
            return TagValue::Integer(num);
        }
        // Try parsing as float
        if let Ok(num) = value.parse::<f64>() {
            return TagValue::Float(num);
        }
    }

    if rational_tags.contains(&tag_name) {
        // Handle rational values like "1/250" or decimal like "2.8"
        if let Some(rational) = parse_rational_value(value) {
            return rational;
        }
    }

    // Handle flash mode values
    if tag_name == "Flash" {
        return parse_flash_value(value);
    }

    // Handle macro mode values
    if tag_name == "Macro" {
        return parse_boolean_value(value);
    }

    // Default to string
    TagValue::String(value.to_string())
}

/// Parse a rational number value from string.
///
/// Handles formats like "1/250" (fraction) or "2.8" (decimal).
///
/// # Arguments
///
/// * `value` - The string value to parse
///
/// # Returns
///
/// `Some(TagValue)` if parsing succeeded, `None` otherwise
fn parse_rational_value(value: &str) -> Option<TagValue> {
    // Check for fraction format "numerator/denominator"
    if let Some(slash_pos) = value.find('/') {
        let numerator_str = value[..slash_pos].trim();
        let denominator_str = value[slash_pos + 1..].trim();

        if let (Ok(num), Ok(denom)) = (numerator_str.parse::<i32>(), denominator_str.parse::<i32>())
            && denom != 0
        {
            return Some(TagValue::Rational {
                numerator: num,
                denominator: denom,
            });
        }
    }

    // Check for decimal format
    if let Ok(f) = value.parse::<f64>() {
        return Some(TagValue::Float(f));
    }

    None
}

/// Parse flash mode value to a descriptive string.
///
/// # Arguments
///
/// * `value` - The raw flash value from Olympus data
///
/// # Returns
///
/// A TagValue containing the interpreted flash mode
fn parse_flash_value(value: &str) -> TagValue {
    // Normalize the value for comparison
    let value_lower = value.to_lowercase();

    let description = match value_lower.as_str() {
        "0" | "off" | "no" | "false" => "Off",
        "1" | "on" | "yes" | "true" | "fired" => "Fired",
        "2" | "auto" => "Auto",
        "3" | "redeye" | "red-eye" => "Red-eye Reduction",
        "4" | "slow" => "Slow Sync",
        "5" | "auto_redeye" => "Auto, Red-eye Reduction",
        "fill" | "fill-in" => "Fill Flash",
        "force" | "forced" => "Forced On",
        _ => value, // Return original if not recognized
    };

    TagValue::String(description.to_string())
}

/// Parse a boolean-like value to a descriptive string.
///
/// # Arguments
///
/// * `value` - The raw value from Olympus data
///
/// # Returns
///
/// A TagValue containing "On" or "Off" (or the original value if not recognized)
fn parse_boolean_value(value: &str) -> TagValue {
    let value_lower = value.to_lowercase();

    let description = match value_lower.as_str() {
        "0" | "off" | "no" | "false" | "normal" => "Off",
        "1" | "on" | "yes" | "true" | "macro" => "On",
        _ => value,
    };

    TagValue::String(description.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_app12_color4() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\r\n[diag info]\r\nCOLOR4=5\r\n";

        let metadata = parse_app12_olympus(data).expect("Olympus APP12 data should parse");

        assert_eq!(metadata.get_integer("APP12:COLOR4"), Some(5));
    }

    /// Test parsing basic Olympus Picture Info data with camera type
    #[test]
    fn test_parse_basic_olympus_data() {
        let data = b"Type=OLYMPUS DIGITAL CAMERA\rResolution=2048x1536\rMacro=Off";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata.get_string("APP12:CameraType"),
            Some("OLYMPUS DIGITAL CAMERA")
        );
        assert_eq!(metadata.get_string("APP12:Resolution"), Some("2048x1536"));
        assert_eq!(metadata.get_string("APP12:Macro"), Some("Off"));
    }

    /// Test the diagnostic CAM4 field exposed by ExifTool as APP12:CAM4.
    #[test]
    fn test_parse_app12_cam4() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\
                     [picture info]\r\n\
                     Type=DCHT\r\n\
                     [diag info]\r\n\
                     CAM4=32\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(metadata.get_integer("APP12:CAM4"), Some(32));
    }

    /// Test the diagnostic CAM6 field exposed by ExifTool as APP12:CAM6.
    #[test]
    fn test_parse_app12_cam6() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\
                     [picture info]\r\n\
                     Type=DCHT\r\n\
                     [diag info]\r\n\
                     CAM4=32\r\n\
                     CAM5=224\r\n\
                     CAM6=80\r\n\
                     CAM7=86\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(metadata.get_integer("APP12:CAM6"), Some(80));
    }

    /// Test the diagnostic CAM5 field exposed by ExifTool as APP12:CAM5.
    #[test]
    fn test_parse_app12_cam5() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\
                     [picture info]\r\n\
                     Type=DCHT\r\n\
                     [diag info]\r\n\
                     CAM4=32\r\n\
                     CAM5=224\r\n\
                     CAM6=80\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(metadata.get_integer("APP12:CAM5"), Some(224));
    }

    /// Test the diagnostic CAM8 field exposed by ExifTool as APP12:CAM8.
    #[test]
    fn test_parse_app12_cam8() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\
                     [picture info]\r\n\
                     Type=DCHT\r\n\
                     [diag info]\r\n\
                     CAM8=143\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(metadata.get_integer("APP12:CAM8"), Some(143));
    }

    /// Test the diagnostic CAM9 field exposed by ExifTool as APP12:CAM9.
    #[test]
    fn test_parse_app12_cam9() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\
                     [picture info]\r\n\
                     Type=DCHT\r\n\
                     [diag info]\r\n\
                     CAM9=0\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(metadata.get_integer("APP12:CAM9"), Some(0));
    }

    /// Test parsing data with ID tag
    #[test]
    fn test_parse_camera_id() {
        let data = b"ID=OLYMPUS DIGITAL CAMERA\rID=N123456789";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        // The second ID value should overwrite the first
        assert!(metadata.contains_key("APP12:ID"));
    }

    /// Test ExifTool-compatible extraction of CAM2 from diagnostic information.
    #[test]
    fn test_parse_app12_cam2() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\
                     [diag info]\r\n\
                     CAM1=59\r\n\
                     CAM2=56\r\n\
                     CAM3=160\r\n";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.get_integer("APP12:CAM2"), Some(56));
    }

    /// Test ExifTool-compatible extraction of CAM7 from diagnostic information.
    #[test]
    fn test_parse_app12_cam7() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\
                     [picture info]\r\n\
                     Type=DCHT\r\n\
                     [diag info]\r\n\
                     CAM6=80\r\n\
                     CAM7=86\r\n\
                     CAM8=143\r\n\
                     [end]\r\n\0";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.get_integer("APP12:CAM7"), Some(86));
    }

    /// Test ExifTool-compatible extraction of CAM3 from diagnostic information.
    #[test]
    fn test_parse_app12_cam3() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\0\
                     [diag info]\r\n\
                     CAM1=59\r\n\
                     CAM2=56\r\n\
                     CAM3=160\r\n\
                     CAM4=32\r\n";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.get_integer("APP12:CAM3"), Some(160));
    }

    /// Test parsing exposure settings
    #[test]
    fn test_parse_exposure_settings() {
        let data = b"OLYMPUS\rExposureTime=1/250\rFNumber=2.8\rISO=400";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        // `ExposureTime` is not a field name in %APP12::PictureInfo (only the
        // `Shutter`/`shtr` fields carry that Name), so it takes the dynamic
        // path and is reported verbatim.
        assert_eq!(metadata.get_string("APP12:ExposureTime"), Some("1/250"));

        // APP12.pm:36 `PrintConv => 'sprintf("%.1f",$val)'`
        assert_eq!(metadata.get_string("APP12:FNumber"), Some("2.8"));

        // Check integer ISO
        assert_eq!(metadata.get_integer("APP12:ISO"), Some(400));
    }

    /// Test parsing flash modes
    #[test]
    fn test_parse_flash_modes() {
        let data = b"OLYMPUS\rFlash=On";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        // APP12.pm:62 `Flash => { PrintConv => { 0 => 'Off', 1 => 'On' } }`:
        // a value outside the two defines is passed through unchanged.
        assert_eq!(metadata.get_string("APP12:Flash"), Some("On"));
    }

    /// Test that non-Olympus data is rejected
    #[test]
    fn test_reject_non_olympus_data() {
        let data = b"SomeOtherManufacturer\rRandomData=123";
        let result = parse_app12_olympus(data);

        assert!(result.is_err());
    }

    /// Test handling of empty data
    #[test]
    fn test_empty_data_rejected() {
        let data = b"";
        let result = parse_app12_olympus(data);

        assert!(result.is_err());
    }

    /// Test handling of too short data
    #[test]
    fn test_short_data_rejected() {
        let data = b"XY";
        let result = parse_app12_olympus(data);

        assert!(result.is_err());
    }

    /// Test parsing with section headers
    #[test]
    fn test_parse_with_section_header() {
        let data = b"[picture info]\rType=OLYMPUS DIGITAL CAMERA\rQuality=SHQ";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        // Section headers should be skipped
        assert!(!metadata.contains_key("APP12:[picture info]"));
        assert_eq!(
            metadata.get_string("APP12:CameraType"),
            Some("OLYMPUS DIGITAL CAMERA")
        );
    }

    /// Test parsing with newline delimiters
    #[test]
    fn test_newline_delimiters() {
        let data = b"Type=OLYMPUS DIGITAL CAMERA\nResolution=1024x768\nZoom=3";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata.get_string("APP12:CameraType"),
            Some("OLYMPUS DIGITAL CAMERA")
        );
        assert_eq!(metadata.get_integer("APP12:Zoom"), Some(3));
    }

    /// Test parsing with quoted values
    #[test]
    fn test_quoted_values() {
        let data = b"OLYMPUS\rModel=\"C-5050Z\"\rMake='OLYMPUS'";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.get_string("APP12:Model"), Some("C-5050Z"));
        assert_eq!(metadata.get_string("APP12:Make"), Some("OLYMPUS"));
    }

    /// Test normalize_tag_name function
    #[test]
    fn test_normalize_tag_name() {
        assert_eq!(normalize_tag_name("type"), "CameraType");
        assert_eq!(normalize_tag_name("ID"), "CameraID");
        assert_eq!(normalize_tag_name("isosetting"), "ISO");
        assert_eq!(normalize_tag_name("unknown_tag"), "UnknownTag");
        assert_eq!(normalize_tag_name("custom-tag"), "CustomTag");
    }

    /// Test to_pascal_case function
    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("snake_case"), "SnakeCase");
        assert_eq!(to_pascal_case("kebab-case"), "KebabCase");
        assert_eq!(to_pascal_case("already_Pascal"), "AlreadyPascal");
        assert_eq!(to_pascal_case("with spaces"), "WithSpaces");
    }

    /// Test parse_rational_value function
    #[test]
    fn test_parse_rational_value() {
        // Fraction format
        let result = parse_rational_value("1/125");
        assert!(matches!(
            result,
            Some(TagValue::Rational {
                numerator: 1,
                denominator: 125
            })
        ));

        // Decimal format
        let result = parse_rational_value("5.6");
        assert!(matches!(result, Some(TagValue::Float(f)) if (f - 5.6).abs() < 0.001));

        // Invalid format
        let result = parse_rational_value("invalid");
        assert!(result.is_none());
    }

    /// Test parse_flash_value function
    #[test]
    fn test_parse_flash_value() {
        assert_eq!(parse_flash_value("0"), TagValue::String("Off".to_string()));
        assert_eq!(
            parse_flash_value("1"),
            TagValue::String("Fired".to_string())
        );
        assert_eq!(
            parse_flash_value("auto"),
            TagValue::String("Auto".to_string())
        );
        assert_eq!(
            parse_flash_value("unknown"),
            TagValue::String("unknown".to_string())
        );
    }

    /// Test parse_boolean_value function
    #[test]
    fn test_parse_boolean_value() {
        assert_eq!(
            parse_boolean_value("0"),
            TagValue::String("Off".to_string())
        );
        assert_eq!(parse_boolean_value("1"), TagValue::String("On".to_string()));
        assert_eq!(
            parse_boolean_value("on"),
            TagValue::String("On".to_string())
        );
        assert_eq!(
            parse_boolean_value("off"),
            TagValue::String("Off".to_string())
        );
    }

    /// Test handling of CAMEDIA cameras
    #[test]
    fn test_camedia_camera() {
        let data = b"CAMEDIA C-5050Z\rResolution=2560x1920";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.get_string("APP12:Resolution"), Some("2560x1920"));
    }

    /// Test null byte delimiter handling
    #[test]
    fn test_null_byte_delimiters() {
        let data = b"OLYMPUS\x00Type=Test Camera\x00ISO=200";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.get_string("APP12:CameraType"), Some("Test Camera"));
        assert_eq!(metadata.get_integer("APP12:ISO"), Some(200));
    }

    /// Test decode_olympus_text with valid UTF-8
    #[test]
    fn test_decode_olympus_text_utf8() {
        let data = b"OLYMPUS TEST";
        let result = decode_olympus_text(data);
        assert_eq!(result, "OLYMPUS TEST");
    }

    /// Test decode_olympus_text with Latin-1 characters
    #[test]
    fn test_decode_olympus_text_latin1() {
        // Latin-1 character 0xE9 (e with acute accent)
        let data: &[u8] = &[0x4F, 0x4C, 0x59, 0x4D, 0x50, 0x55, 0x53, 0xE9];
        let result = decode_olympus_text(data);
        // Should contain the Latin-1 character converted to Unicode
        assert!(result.starts_with("OLYMPUS"));
    }

    /// Test is_olympus_picture_info detection
    #[test]
    fn test_is_olympus_picture_info() {
        assert!(is_olympus_picture_info("OLYMPUS DIGITAL CAMERA"));
        assert!(is_olympus_picture_info("[picture info]\nType=test"));
        assert!(is_olympus_picture_info("CAMEDIA C-5050Z"));
        assert!(is_olympus_picture_info("Type=camera\nID=test"));
        assert!(!is_olympus_picture_info("Canon Camera"));
        assert!(!is_olympus_picture_info("random data"));
    }

    // ------------------------------------------------------------------
    // APP12 Protect / REV / S0 / STB1 / STB3 / STB4 regression tests
    //
    // These six keys were wired in commit bbbdd410, whose `Sample:` trailer
    // named /tmp/oxidex-exiftool-cache/combined-samples/ExifTool.jpg. Measured
    // 2026-07-26 with a release build of that commit, ExifTool.jpg produces
    // ZERO tags containing the substring "APP12" -- its payload begins
    // "Agfa Gevaert   \0", and process_app12_segments (src/core/jpeg_helpers.rs
    // ~line 570) routes on case-sensitive byte compares against b"OLYM.." and
    // b"AGFA", so the segment is dropped before this module is ever called.
    // The cited sample therefore exercised none of the six keys, and the green
    // "recheck-pass gaps=6->1" came entirely from OTHER files in the corpus.
    //
    // Ground truth below is `exiftool 13.55 -G0 -s -a` on the five Olympus
    // D-series samples that actually reach this parser, run 2026-07-26. The
    // fixture bytes are lifted verbatim from those files' APP12 payloads
    // (dumped by walking the JPEG marker chain), and every expectation is a
    // LITERAL from ExifTool's output rather than a reference back to the
    // constant under test.
    //
    // ExifTool provenance for the names:
    //   Protect -- explicit entry in %Image::ExifTool::APP12::PictureInfo,
    //              APP12.pm line 74: `Protect     => { },`. A bare `{ }` means
    //              no Name override AND no PrintConv, so the raw value is
    //              printed verbatim.
    //   REV / S0 / STB1 / STB3 / STB4 -- no table entry anywhere in the
    //              ExifTool distribution (ripgrep for `REV\s*=>` and for
    //              `STB[0-9]` over lib/Image/ExifTool/ both return no hits).
    //              They are added at runtime by sub ProcessAPP12, APP12.pm
    //              line 278: `$tagInfo = { Name => ucfirst $tag };`.
    //              That is why lowercase on-disk `s0=` surfaces as `S0`.
    // ------------------------------------------------------------------

    /// APP12:Protect. Fixture is the head of OlympusD620L.jpg's APP12 payload.
    /// exiftool 13.55 reports `[APP12] Protect : 0` on all five D-series
    /// samples (D220, D320L, D340L, D500L, D620L).
    #[test]
    fn test_app12_protect_matches_exiftool_d620l() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\x001031\r\n\
                     [picture info]\r\n\
                     TimeDate=883639173\r\n\
                     Resolution=3\r\n\
                     Protect=0\r\n\
                     ContTake=0\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(metadata.get_integer("APP12:Protect"), Some(0));
    }

    /// APP12:Protect carries NO PrintConv in APP12.pm (`Protect => { }`), so a
    /// non-zero value must pass through as the raw number. The whole corpus
    /// only ever holds Protect=0, which makes every non-zero value a blind
    /// spot the sample cannot cover -- exactly the hole that let the RAR
    /// host-OS catch-all ("Unknown" replacing real data) through on 2026-07-26.
    #[test]
    fn test_app12_protect_has_no_printconv_so_nonzero_passes_through() {
        for raw in [1_i64, 2, 255] {
            let data = format!(
                "OLYMPUS OPTICAL CO.,LTD.\0 697\r\n\
                 [picture info]\r\n\
                 Protect={}\r\n\
                 [end]\r\n\0",
                raw
            );

            let metadata = parse_app12_olympus(data.as_bytes()).unwrap();

            assert_eq!(
                metadata.get_integer("APP12:Protect"),
                Some(raw),
                "Protect={} must stay the raw number; APP12.pm defines no PrintConv for it",
                raw
            );
            assert_eq!(
                metadata.get_string("APP12:Protect"),
                None,
                "Protect={} must not be substituted with a stand-in label",
                raw
            );
        }
    }

    /// APP12:REV. Fixture is the `[diag info]` head of OlympusD620L.jpg, the
    /// only file in the corpus carrying a REV record. exiftool 13.55 reports
    /// `[APP12] REV : DCPT`, and `-v3` prints `[adding APP12:REV]` because the
    /// name comes from ProcessAPP12's `ucfirst $tag` fallback, not a table.
    #[test]
    fn test_app12_rev_matches_exiftool_d620l() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\x001031\r\n\
                     [camera info]\r\n\
                     Type=DCHT\r\n\
                     Version=v01-02\r\n\
                     [diag info]\r\n\
                     REV=DCPT\r\n\
                     IMgg=35931\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(metadata.get_string("APP12:REV"), Some("DCPT"));
    }

    /// APP12:S0. Fixture is the `s0=` record from OlympusD220.jpg, verbatim.
    /// Note the on-disk key is LOWERCASE; ExifTool's `ucfirst $tag` is what
    /// turns it into the uppercase `S0` reported by `exiftool -G0 -s -a`.
    /// The expected string is byte-for-byte ExifTool's output for that file.
    #[test]
    fn test_app12_s0_matches_exiftool_d220() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.   \x00 697\r\n\
                     [diag info]\r\n\
                     PicLen=87648\r\n\
                     ThmLen=4016\r\n\
                     s0=8259,0,14bfe,a184,11987,1e4f1,0,7c0000,40b60000,\
                     56a05e6,616061a,5fb0581,b738,13c0038,d7\r\n\
                     T0=3e2,0,0,16788,92,11e6b\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(
            metadata.get_string("APP12:S0"),
            Some(
                "8259,0,14bfe,a184,11987,1e4f1,0,7c0000,40b60000,\
                 56a05e6,616061a,5fb0581,b738,13c0038,d7"
            )
        );
    }

    /// APP12:STB1 / STB3 / STB4. Fixture is the STB block from
    /// OlympusD500L.jpg, verbatim. exiftool 13.55 reports STB1 : 139,
    /// STB3 : 262, STB4 : 14 for that file -- three distinct literals, so a
    /// swapped or fabricated mapping cannot pass by coincidence.
    #[test]
    fn test_app12_stb_values_match_exiftool_d500l() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\x00 991\r\n\
                     [diag info]\r\n\
                     EXP3=237\r\n\
                     STB1=139\r\n\
                     STB2=0\r\n\
                     STB3=262\r\n\
                     STB4=14\r\n\
                     STB5=0\r\n\
                     CAM1=33\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(metadata.get_integer("APP12:STB1"), Some(139));
        assert_eq!(metadata.get_integer("APP12:STB3"), Some(262));
        assert_eq!(metadata.get_integer("APP12:STB4"), Some(14));
    }

    /// The STB keys have no ExifTool table entry at all, hence no PrintConv,
    /// so every value must survive as the raw number. Both corpus files that
    /// carry STB records hold only 0 and three small values (139/262/14), so
    /// the sample can never demonstrate that a large or unusual value is not
    /// remapped. Pin it here instead.
    #[test]
    fn test_app12_stb_has_no_printconv_so_arbitrary_values_pass_through() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\x00 991\r\n\
                     [diag info]\r\n\
                     STB1=65535\r\n\
                     STB3=1\r\n\
                     STB4=4294967295\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        assert_eq!(metadata.get_integer("APP12:STB1"), Some(65535));
        assert_eq!(metadata.get_integer("APP12:STB3"), Some(1));
        assert_eq!(metadata.get_integer("APP12:STB4"), Some(4_294_967_295));
    }

    /// The failure mode that motivated this whole review: a tag being invented
    /// for input that does not contain it. None of the six keys may appear
    /// when the payload has no such record. OlympusD500L.jpg, for instance,
    /// carries STB records but no REV and no s0 -- and exiftool 13.55 emits
    /// neither for that file.
    #[test]
    fn test_app12_wired_keys_are_not_invented_when_absent() {
        let data = b"OLYMPUS OPTICAL CO.,LTD.\x00 991\r\n\
                     [picture info]\r\n\
                     Resolution=2\r\n\
                     ColorMode=1\r\n\
                     [camera info]\r\n\
                     Type=DCHC\r\n\
                     [end]\r\n\0";

        let metadata = parse_app12_olympus(data).unwrap();

        for key in [
            "APP12:Protect",
            "APP12:REV",
            "APP12:S0",
            "APP12:STB1",
            "APP12:STB3",
            "APP12:STB4",
            "APP12:STB5",
            "APP12:STB6",
        ] {
            assert!(
                !metadata.contains_key(key),
                "{} was emitted for a payload that contains no such record",
                key
            );
        }
    }
}
