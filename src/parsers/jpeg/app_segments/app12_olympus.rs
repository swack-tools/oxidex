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
    "Macro",
    "Zoom",
    "Resolution",
    "ImageSize",
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
/// On success, tags are prefixed with "Olympus:" (e.g., "Olympus:CameraType").
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
/// assert_eq!(metadata.get_string("Olympus:Type"), Some("OLYMPUS DIGITAL CAMERA"));
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
/// stores them in the metadata map with the "Olympus:" prefix.
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
            // Normalize the tag name and add to metadata
            let tag_name = normalize_tag_name(&key);
            let tag_value = parse_tag_value(&tag_name, &value);

            // The legacy Picture Info field "Type" is the camera model.
            // ExifTool exposes it in the APP12 group as CameraType. This
            // parser handles identifier-less Picture Info records (including
            // Agfa SR84 data), so the canonical tag must be emitted here
            // rather than only by the AGFA-identified parser.
            if tag_name == "CameraType" {
                metadata.insert(
                    "APP12:CameraType".to_string(),
                    TagValue::String(value.clone()),
                );
            }

            // Identifier-less Picture Info records, including the legacy Agfa
            // variant used by ExifTool.jpg, are routed through this parser
            // rather than the Agfa-specific parser. ExifTool exposes ID as a
            // textual tag in the APP12 group.
            if key.eq_ignore_ascii_case("ID") {
                metadata.insert("APP12:ID".to_string(), TagValue::String(value.clone()));
            }

            // Olympus Picture Info calls this field ExposureBias. ExifTool
            // exposes it as APP12:ExposureCompensation. Preserve the textual
            // representation because these records include the explicit sign
            // and precision (for example, "+2.0").
            if tag_name == "ExposureCompensation" {
                metadata.insert(
                    "APP12:ExposureCompensation".to_string(),
                    TagValue::String(value.clone()),
                );
            }

            // ExposureTime in Picture Info is already stored in ExifTool's
            // display form (for example, "1/155"). Expose the canonical
            // APP12 tag and preserve the fraction exactly.
            if tag_name == "ExposureTime" {
                metadata.insert(
                    "APP12:ExposureTime".to_string(),
                    TagValue::String(value.clone()),
                );
            }

            // FNumber is a standard Picture Info field exposed by ExifTool in
            // the APP12 group. Reuse the parsed value so decimal apertures
            // retain the same numeric representation as the compatibility
            // Olympus tag emitted below.
            if key.eq_ignore_ascii_case("FNumber") {
                metadata.insert("APP12:FNumber".to_string(), tag_value.clone());
                metadata.insert("APP12:Fnumber".to_string(), tag_value.clone());
            }

            // ExifTool's APP12 Picture Info table defines Resolution and
            // ImageSize as two distinct tags (Image::ExifTool::APP12),
            // not one renamed to the other. Resolution has no PrintConv
            // and is exposed verbatim; identifier-less legacy records
            // (including Agfa SR84) use this same field and code path.
            if key.eq_ignore_ascii_case("Resolution") {
                metadata.insert(
                    "APP12:Resolution".to_string(),
                    TagValue::String(value.clone()),
                );
            }

            // ImageSize stores a dash-delimited width-height pair (for
            // example "1280-1024"); ExifTool's PrintConv translates every
            // '-' to 'x' (`$val=~tr/-/x/;$val`) to produce the "1280x1024"
            // display form.
            if key.eq_ignore_ascii_case("ImageSize") {
                metadata.insert(
                    "APP12:ImageSize".to_string(),
                    TagValue::String(value.replace('-', "x")),
                );
            }

            // Flash is part of ExifTool's JPEG Picture Info table and belongs
            // to the APP12 group. Preserve its display-ready textual value,
            // such as "Off", while retaining the Olympus compatibility tag.
            if key.eq_ignore_ascii_case("Flash") {
                metadata.insert("APP12:Flash".to_string(), TagValue::String(value.clone()));
            }

            // The source field in JPEG Picture Info records is normally named
            // TimeDate. ExifTool renames this to DateTimeOriginal and exposes
            // it in the APP12 group. Also accept DateTimeOriginal directly
            // for variants which already use the normalized field name.
            if tag_name == "DateTimeOriginal"
                || key.eq_ignore_ascii_case("TimeDate")
                || key.eq_ignore_ascii_case("DateTimeOriginal")
            {
                metadata.insert(
                    "APP12:DateTimeOriginal".to_string(),
                    TagValue::String(normalize_picture_info_datetime(&value)),
                );
            }

            // ColorMode is part of ExifTool's JPEG Picture Info table, whose
            // tags belong to the APP12 group. Keep the Olympus-prefixed tag
            // emitted below for compatibility and also emit the canonical
            // ExifTool tag. Picture Info normally stores this as an integer;
            // retain unexpected non-numeric values rather than dropping them.
            if key.eq_ignore_ascii_case("ColorMode") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:ColorMode".to_string(), app12_value);
            }

            // ExifTool exposes these Olympus diagnostic fields in the APP12
            // group rather than as an Olympus maker-note tag.
            if key.eq_ignore_ascii_case("CAM4") || key.eq_ignore_ascii_case("CAM6") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));
                let app12_tag = if key.eq_ignore_ascii_case("CAM4") {
                    "APP12:CAM4"
                } else {
                    "APP12:CAM6"
                };

                metadata.insert(app12_tag.to_string(), app12_value);
            }

            // ExifTool exposes this Olympus diagnostic field using its
            // original name in the APP12 group.
            if key.eq_ignore_ascii_case("CAM5") {
                let cam5_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:CAM5".to_string(), cam5_value);
            }

            // ExifTool exposes the Olympus EXP1 diagnostic field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("EXP1") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:EXP1".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus EXP2 diagnostic field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("EXP2") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:EXP2".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus EXP3 diagnostic field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("EXP3") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:EXP3".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus IMbb diagnostic field in the
            // APP12 group using its original mixed-case name.
            if key.eq_ignore_ascii_case("IMbb") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:IMbb".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus IMbg diagnostic field in the
            // APP12 group using its original mixed-case name.
            if key.eq_ignore_ascii_case("IMbg") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:IMbg".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus IMgb diagnostic field in the
            // APP12 group using its original mixed-case name.
            if key.eq_ignore_ascii_case("IMgb") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:IMgb".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus IMgr diagnostic field in the
            // APP12 group using its original mixed-case name.
            if key.eq_ignore_ascii_case("IMgr") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:IMgr".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus IMrg diagnostic field in the
            // APP12 group using its original mixed-case name.
            if key.eq_ignore_ascii_case("IMrg") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:IMrg".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus IMbr diagnostic field in the
            // APP12 group using its original mixed-case name.
            if key.eq_ignore_ascii_case("IMbr") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:IMbr".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus IMrb diagnostic field in the
            // APP12 group using its original mixed-case name.
            if key.eq_ignore_ascii_case("IMrb") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:IMrb".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus IMrr diagnostic field in the APP12
            // group using its original mixed-case name.
            if key.eq_ignore_ascii_case("IMrr") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:IMrr".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus IMgg diagnostic field in the APP12
            // group using its original mixed-case name.
            if key.eq_ignore_ascii_case("IMgg") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:IMgg".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus FCS diagnostic fields in the APP12
            // group using their original names.
            if key.eq_ignore_ascii_case("FCS1")
                || key.eq_ignore_ascii_case("FCS2")
                || key.eq_ignore_ascii_case("FCS3")
                || key.eq_ignore_ascii_case("FCS4")
                || key.eq_ignore_ascii_case("FCS5")
                || key.eq_ignore_ascii_case("FCS6")
                || key.eq_ignore_ascii_case("FCS7")
            {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));
                let app12_tag = if key.eq_ignore_ascii_case("FCS1") {
                    "APP12:FCS1"
                } else if key.eq_ignore_ascii_case("FCS2") {
                    "APP12:FCS2"
                } else if key.eq_ignore_ascii_case("FCS3") {
                    "APP12:FCS3"
                } else if key.eq_ignore_ascii_case("FCS4") {
                    "APP12:FCS4"
                } else if key.eq_ignore_ascii_case("FCS5") {
                    "APP12:FCS5"
                } else if key.eq_ignore_ascii_case("FCS6") {
                    "APP12:FCS6"
                } else {
                    "APP12:FCS7"
                };

                metadata.insert(app12_tag.to_string(), app12_value);
            }

            // ExifTool exposes the continuous-take diagnostic field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("ContTake") {
                let app12_value = value
                    .parse::<i64>()
                    .map(TagValue::Integer)
                    .unwrap_or_else(|_| TagValue::String(value.clone()));

                metadata.insert("APP12:ContTake".to_string(), app12_value);
            }

            metadata.insert(format!("Olympus:{}", tag_name), tag_value);

            // ExifTool exposes the Olympus diagnostic CAM1 field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("CAM1") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:CAM1".to_string(), app12_value);
            }

            // ExifTool's Olympus Picture Info table exposes CAM2 using its
            // original name in the APP12 group. Keep the Olympus-prefixed
            // value above for compatibility while also emitting the canonical
            // ExifTool tag.
            if key.eq_ignore_ascii_case("CAM2") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:CAM2".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus CAM7 diagnostic field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("CAM7") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:CAM7".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus diagnostic CAM8 field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("CAM8") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:CAM8".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus diagnostic CAM9 field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("CAM9") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:CAM9".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus diagnostic CAM3 field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("CAM3") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:CAM3".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus diagnostic COLOR1 field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("COLOR1") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:COLOR1".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus diagnostic COLOR2 field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("COLOR2") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:COLOR2".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus diagnostic COLOR3 field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("COLOR3") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:COLOR3".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus diagnostic COLOR4 field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("COLOR4") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:COLOR4".to_string(), app12_value);
            }

            // ExifTool exposes the Olympus diagnostic CAM9 field in the
            // APP12 group using its original name.
            if key.eq_ignore_ascii_case("CAM9") {
                let app12_value = match value.parse::<i64>() {
                    Ok(number) => TagValue::Integer(number),
                    Err(_) => TagValue::String(value.clone()),
                };
                metadata.insert("APP12:CAM9".to_string(), app12_value);
            }
        }
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
        let metadata =
            parse_app12_olympus(b"OLYMPUS DIGITAL CAMERA\0[picture info]\r\nExposureBias=+2.0\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(
            metadata.get_string("APP12:ExposureCompensation"),
            Some("+2.0")
        );
    }

    #[test]
    fn test_olympus_color_mode_app12_tag() {
        let metadata =
            parse_app12_olympus(b"OLYMPUS OPTICAL CO.,LTD.\0[picture info]\r\nColorMode=1\r\n")
                .expect("Olympus Picture Info should parse");

        assert_eq!(metadata.get_integer("APP12:ColorMode"), Some(1));
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
    fn test_picture_info_timedate_ctime_format() {
        let metadata = parse_app12_olympus(b"[picture info]\rTimeDate=Thu Dec 31 15:17:20 1998\r")
            .expect("Picture Info TimeDate should parse");

        assert_eq!(
            metadata.get_string("APP12:DateTimeOriginal"),
            Some("1998:12:31 15:17:20")
        );
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
            metadata.get_string("Olympus:CameraType"),
            Some("OLYMPUS DIGITAL CAMERA")
        );
        assert_eq!(
            metadata.get_string("Olympus:ImageResolution"),
            Some("2048x1536")
        );
        assert_eq!(metadata.get_string("Olympus:Macro"), Some("Off"));
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
        assert!(metadata.contains_key("Olympus:CameraID"));
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

        // Check rational exposure time
        if let Some(TagValue::Rational {
            numerator,
            denominator,
        }) = metadata.get("Olympus:ExposureTime")
        {
            assert_eq!(*numerator, 1);
            assert_eq!(*denominator, 250);
        } else {
            panic!("Expected Rational value for ExposureTime");
        }

        // Check float aperture
        assert_eq!(metadata.get_float("Olympus:FNumber"), Some(2.8));

        // Check integer ISO
        assert_eq!(metadata.get_integer("Olympus:ISO"), Some(400));
    }

    /// Test parsing flash modes
    #[test]
    fn test_parse_flash_modes() {
        let data = b"OLYMPUS\rFlash=On";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.get_string("Olympus:Flash"), Some("Fired"));
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
        assert!(!metadata.contains_key("Olympus:[picture info]"));
        assert_eq!(
            metadata.get_string("Olympus:CameraType"),
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
            metadata.get_string("Olympus:CameraType"),
            Some("OLYMPUS DIGITAL CAMERA")
        );
        assert_eq!(metadata.get_integer("Olympus:Zoom"), Some(3));
    }

    /// Test parsing with quoted values
    #[test]
    fn test_quoted_values() {
        let data = b"OLYMPUS\rModel=\"C-5050Z\"\rMake='OLYMPUS'";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(metadata.get_string("Olympus:Model"), Some("C-5050Z"));
        assert_eq!(metadata.get_string("Olympus:Make"), Some("OLYMPUS"));
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

        assert_eq!(
            metadata.get_string("Olympus:ImageResolution"),
            Some("2560x1920")
        );
    }

    /// Test null byte delimiter handling
    #[test]
    fn test_null_byte_delimiters() {
        let data = b"OLYMPUS\x00Type=Test Camera\x00ISO=200";
        let result = parse_app12_olympus(data);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata.get_string("Olympus:CameraType"),
            Some("Test Camera")
        );
        assert_eq!(metadata.get_integer("Olympus:ISO"), Some(200));
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
}
