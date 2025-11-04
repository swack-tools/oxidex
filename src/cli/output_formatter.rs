//! Output formatting for JSON/CSV/human-readable formats
//!
//! This module handles formatting metadata output in various formats.
//!
//! # Examples
//!
//! ```
//! use exiftool_rs::cli::output_formatter::{OutputFormatter, HumanReadableFormatter, JsonFormatter};
//! use exiftool_rs::core::metadata_map::MetadataMap;
//! use exiftool_rs::core::tag_value::TagValue;
//!
//! let mut metadata = MetadataMap::new();
//! metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
//! metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
//!
//! // Human-readable format
//! let human_formatter = HumanReadableFormatter;
//! let output = human_formatter.format(&metadata, None);
//! println!("{}", output);
//!
//! // JSON format
//! let json_formatter = JsonFormatter;
//! let json_output = json_formatter.format(&metadata, None);
//! println!("{}", json_output);
//! ```

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;
use crate::parsers::tiff::tiff_enums::tiff_enum_to_string;
use csv::Writer;

/// Trait for formatting metadata into different output formats
///
/// This trait defines a common interface for all output formatters,
/// allowing the CLI to select the appropriate formatter based on user preferences.
pub trait OutputFormatter {
    /// Formats the given metadata into a string representation
    ///
    /// # Arguments
    ///
    /// * `metadata` - The metadata map to format
    /// * `filter_tags` - Optional list of tag names to include in output.
    ///   If None, all tags are included.
    ///
    /// # Returns
    ///
    /// A formatted string representation of the metadata
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String;
}

/// Formats metadata in human-readable key-value format
///
/// Output format: "Tag: Value\n" for each tag, sorted alphabetically by tag name.
///
/// # Examples
///
/// ```
/// use exiftool_rs::cli::output_formatter::{OutputFormatter, HumanReadableFormatter};
/// use exiftool_rs::core::metadata_map::MetadataMap;
/// use exiftool_rs::core::tag_value::TagValue;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
/// metadata.insert("EXIF:ISO", TagValue::new_integer(400));
///
/// let formatter = HumanReadableFormatter;
/// let output = formatter.format(&metadata, None);
/// // Output:
/// // EXIF:ISO: 400
/// // EXIF:Make: Canon
/// ```
pub struct HumanReadableFormatter;

impl OutputFormatter for HumanReadableFormatter {
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String {
        if metadata.is_empty() {
            return String::new();
        }

        // Collect tags into a vector for sorting
        let mut tags: Vec<_> = metadata.iter().collect();

        // Filter tags if a filter is provided
        if let Some(filter) = filter_tags {
            tags.retain(|(name, _)| filter.contains(name));
            if tags.is_empty() {
                return String::new();
            }
        }

        // Sort tags alphabetically by name
        tags.sort_by_key(|(name, _)| *name);

        // Format each tag as "Tag: Value\n"
        let mut output = String::new();
        for (tag_name, tag_value) in tags {
            let formatted_value = format_tag_value(tag_name, tag_value);
            output.push_str(&format!("{}: {}\n", tag_name, formatted_value));
        }

        output
    }
}

/// Formats metadata as JSON
///
/// Uses `serde_json` to serialize the metadata map into a JSON string.
/// The output is pretty-printed for readability.
///
/// # Examples
///
/// ```
/// use exiftool_rs::cli::output_formatter::{OutputFormatter, JsonFormatter};
/// use exiftool_rs::core::metadata_map::MetadataMap;
/// use exiftool_rs::core::tag_value::TagValue;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
///
/// let formatter = JsonFormatter;
/// let json = formatter.format(&metadata, None);
/// // JSON output can be parsed by jq or any JSON parser
/// ```
pub struct JsonFormatter;

impl OutputFormatter for JsonFormatter {
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String {
        // If filter is specified, create a new filtered metadata map
        let metadata_to_filter = if let Some(filter) = filter_tags {
            let filtered: MetadataMap = metadata
                .iter()
                .filter(|(name, _)| filter.contains(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            filtered
        } else {
            metadata.clone()
        };

        // Convert MetadataMap to a simple HashMap for Perl ExifTool-compatible JSON output
        // Unwrap TagValue enum to produce flat values like {"EXIF:Make": "Canon"}
        // instead of {"EXIF:Make": {"type": "String", "value": "Canon"}}
        let mut json_map = serde_json::Map::new();

        for (tag_name, tag_value) in metadata_to_filter.iter() {
            let json_value = tag_value_to_json(Some(tag_name.as_str()), tag_value);
            json_map.insert(tag_name.clone(), json_value);
        }

        // Serialize to pretty JSON wrapped in an array for Perl ExifTool compatibility
        // Perl ExifTool outputs: [{...}] (array with one object per file)
        // This allows processing multiple files with consistent JSON structure
        match serde_json::to_string_pretty(&vec![json_map]) {
            Ok(json) => json,
            Err(e) => {
                // Fallback error message if serialization fails
                format!("[{{\"error\": \"Failed to serialize metadata: {}\"}}]", e)
            }
        }
    }
}

/// Converts a TagValue to a serde_json::Value for Perl ExifTool-compatible output
///
/// This unwraps the TagValue enum and produces simple JSON values:
/// - String → JSON string
/// - Integer → JSON number
/// - Float → JSON number
/// - Rational → JSON string "numerator/denominator"
/// - Binary → JSON string "(Binary, N bytes)"
/// - DateTime → JSON string (EXIF format: "YYYY:MM:DD HH:MM:SS")
/// - Struct → JSON object (recursive)
fn tag_value_to_json(tag_name: Option<&str>, value: &TagValue) -> serde_json::Value {
    if let Some(name) = tag_name {
        if let Some(label) = friendly_enum_name(name, value) {
            return serde_json::Value::String(label);
        }
    }

    match value {
        TagValue::String(s) => serde_json::Value::String(s.clone()),
        TagValue::Integer(i) => serde_json::json!(*i),
        TagValue::Float(f) => serde_json::json!(*f),
        TagValue::Rational {
            numerator,
            denominator,
        } => {
            // Normalize rational display to match Perl ExifTool
            if *denominator == 0 {
                // Invalid rational, output as string
                serde_json::Value::String(format!("{}/0", numerator))
            } else if *denominator == 1 {
                // Output as integer string (e.g., "100/1" → "100")
                serde_json::Value::String(format!("{}", numerator))
            } else if *numerator == 0 {
                // Zero rational
                serde_json::Value::String("0".to_string())
            } else {
                // Check if this should be output as a decimal number (like Perl ExifTool does for FNumber)
                // For typical aperture/focal length values, output as decimal
                let decimal = *numerator as f64 / *denominator as f64;
                if decimal < 1000.0 && decimal.fract() != 0.0 {
                    // This looks like an aperture or similar value, output as JSON Number
                    if let Some(num) = serde_json::Number::from_f64(decimal) {
                        return serde_json::Value::Number(num);
                    }
                }
                // Otherwise keep as fraction string
                serde_json::Value::String(format!("{}/{}", numerator, denominator))
            }
        }
        TagValue::Binary(bytes) => {
            serde_json::Value::String(format!("(Binary, {} bytes)", bytes.len()))
        }
        TagValue::DateTime(dt) => {
            // Format as EXIF DateTime: "YYYY:MM:DD HH:MM:SS"
            // This matches Perl ExifTool's output format
            serde_json::Value::String(dt.format("%Y:%m:%d %H:%M:%S").to_string())
        }
        TagValue::Struct(map) => {
            let mut obj = serde_json::Map::new();
            for (key, val) in map.iter() {
                obj.insert(key.clone(), tag_value_to_json(None, val));
            }
            serde_json::Value::Object(obj)
        }
        TagValue::Array(values) => {
            let array: Vec<serde_json::Value> = values
                .iter()
                .map(|v| tag_value_to_json(tag_name, v))
                .collect();
            serde_json::Value::Array(array)
        }
    }
}

/// Formats metadata as CSV
///
/// Output format: Two-column CSV with "Tag" and "Value" headers.
/// Each metadata entry becomes a row with the tag name and its formatted value.
/// The CSV is RFC 4180 compliant and parseable by standard tools (Excel, pandas).
///
/// # Examples
///
/// ```
/// use exiftool_rs::cli::output_formatter::{OutputFormatter, CsvFormatter};
/// use exiftool_rs::core::metadata_map::MetadataMap;
/// use exiftool_rs::core::tag_value::TagValue;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
/// metadata.insert("EXIF:ISO", TagValue::new_integer(400));
///
/// let formatter = CsvFormatter;
/// let csv = formatter.format(&metadata, None);
/// // Output:
/// // Tag,Value
/// // EXIF:ISO,400
/// // EXIF:Make,Canon
/// ```
pub struct CsvFormatter;

impl OutputFormatter for CsvFormatter {
    fn format(&self, metadata: &MetadataMap, filter_tags: Option<&[String]>) -> String {
        if metadata.is_empty() {
            return String::new();
        }

        // Collect tags into a vector for sorting
        let mut tags: Vec<_> = metadata.iter().collect();

        // Filter tags if a filter is provided
        if let Some(filter) = filter_tags {
            tags.retain(|(name, _)| filter.contains(name));
            if tags.is_empty() {
                return String::new();
            }
        }

        // Sort tags alphabetically by name
        tags.sort_by_key(|(name, _)| *name);

        // Create CSV writer that writes to a Vec<u8> buffer
        let mut wtr = Writer::from_writer(vec![]);

        // Write header row
        if wtr.write_record(["Tag", "Value"]).is_err() {
            return String::from("Tag,Value\n");
        }

        // Write data rows
        for (tag_name, tag_value) in tags {
            let formatted_value = format_tag_value(tag_name, tag_value);
            if wtr.write_record([tag_name, &formatted_value]).is_err() {
                // Skip this record if write fails, but continue
                continue;
            }
        }

        // Flush the writer and get the buffer
        if wtr.flush().is_err() {
            return String::from("Tag,Value\n");
        }

        let data = match wtr.into_inner() {
            Ok(buffer) => buffer,
            Err(_) => return String::from("Tag,Value\n"),
        };

        // Convert bytes to UTF-8 string
        String::from_utf8(data).unwrap_or_else(|_| String::from("Tag,Value\n"))
    }
}

/// Helper function to format a TagValue for human-readable display
///
/// Converts each TagValue variant into a clean string representation
/// without the enum structure (e.g., "Canon" instead of "String(\"Canon\")").
fn format_tag_value(tag_name: &str, value: &TagValue) -> String {
    if let Some(label) = friendly_enum_name(tag_name, value) {
        return label;
    }

    match value {
        TagValue::String(s) => s.clone(),
        TagValue::Integer(i) => i.to_string(),
        TagValue::Float(f) => f.to_string(),
        TagValue::Rational {
            numerator,
            denominator,
        } => format!("{}/{}", numerator, denominator),
        TagValue::Binary(bytes) => format!("(Binary, {} bytes)", bytes.len()),
        TagValue::DateTime(dt) => dt.to_rfc3339(),
        TagValue::Struct(_) => "(Structured data)".to_string(),
        TagValue::Array(values) => {
            let formatted: Vec<String> = values
                .iter()
                .map(|v| format_tag_value(tag_name, v))
                .collect();
            format!("[{}]", formatted.join(", "))
        }
    }
}

/// Resolves TIFF enumeration names while leaving raw numeric values intact.
///
/// This looks up the tag descriptor to retrieve the numeric tag ID and uses
/// the TIFF enum table to translate well-known values (e.g., Orientation).
fn friendly_enum_name(tag_name: &str, value: &TagValue) -> Option<String> {
    let tag_id = lookup_tiff_enum_tag_id(tag_name)?;

    match value {
        TagValue::Integer(i) => tiff_enum_to_string(tag_id, *i),
        _ => None,
    }
}

/// Maps canonical tag names to their numeric TIFF tag IDs for enum resolution.
fn lookup_tiff_enum_tag_id(tag_name: &str) -> Option<u16> {
    match tag_name {
        // Orientation (tag 0x0112)
        "IFD0:Orientation" | "IFD1:Orientation" | "IFD2:Orientation" | "EXIF:Orientation" => {
            Some(0x0112)
        }

        // Compression (tag 0x0103)
        "IFD0:Compression" | "IFD1:Compression" | "IFD2:Compression" | "EXIF:Compression" => {
            Some(0x0103)
        }

        // PhotometricInterpretation (tag 0x0106)
        "IFD0:PhotometricInterpretation"
        | "IFD1:PhotometricInterpretation"
        | "IFD2:PhotometricInterpretation"
        | "EXIF:PhotometricInterpretation" => Some(0x0106),

        // PlanarConfiguration (tag 0x011C)
        "IFD0:PlanarConfiguration"
        | "IFD1:PlanarConfiguration"
        | "IFD2:PlanarConfiguration"
        | "EXIF:PlanarConfiguration" => Some(0x011C),

        // ResolutionUnit (tag 0x0128)
        "IFD0:ResolutionUnit"
        | "IFD1:ResolutionUnit"
        | "IFD2:ResolutionUnit"
        | "EXIF:ResolutionUnit" => Some(0x0128),

        // FillOrder (tag 0x010A)
        "IFD0:FillOrder" | "IFD1:FillOrder" | "IFD2:FillOrder" | "EXIF:FillOrder" => Some(0x010A),

        // SampleFormat (tag 0x0153)
        "IFD0:SampleFormat" | "IFD1:SampleFormat" | "IFD2:SampleFormat" | "EXIF:SampleFormat" => {
            Some(0x0153)
        }

        // YCbCrPositioning (tag 0x0213)
        "IFD0:YCbCrPositioning"
        | "IFD1:YCbCrPositioning"
        | "IFD2:YCbCrPositioning"
        | "EXIF:YCbCrPositioning" => Some(0x0213),

        // ExtraSamples (tag 0x0152)
        "IFD0:ExtraSamples" | "IFD1:ExtraSamples" | "IFD2:ExtraSamples" | "EXIF:ExtraSamples" => {
            Some(0x0152)
        }

        // SubfileType (tag 0x00FE)
        "IFD0:SubfileType" | "IFD1:SubfileType" | "IFD2:SubfileType" | "EXIF:SubfileType" => {
            Some(0x00FE)
        }

        // ColorSpace (tag 0xA001)
        "ExifIFD:ColorSpace" | "EXIF:ColorSpace" => Some(0xA001),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_human_readable_formatter_empty_metadata() {
        let metadata = MetadataMap::new();
        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);
        assert_eq!(output, "");
    }

    #[test]
    fn test_human_readable_formatter_single_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);
        assert_eq!(output, "EXIF:Make: Canon\n");
    }

    #[test]
    fn test_human_readable_formatter_multiple_tags_sorted() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);

        // Tags should be sorted alphabetically
        assert_eq!(
            output,
            "EXIF:ISO: 400\nEXIF:Make: Canon\nEXIF:Model: EOS 5D\n"
        );
    }

    #[test]
    fn test_human_readable_formatter_all_value_types() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(800));
        metadata.insert("EXIF:FNumber", TagValue::new_float(2.8));
        metadata.insert("EXIF:ExposureTime", TagValue::new_rational(1, 100));
        metadata.insert(
            "EXIF:ThumbnailData",
            TagValue::new_binary(vec![0xFF, 0xD8, 0xFF, 0xE0]),
        );

        let dt = Utc.with_ymd_and_hms(2023, 6, 15, 12, 30, 0).unwrap();
        metadata.insert("EXIF:DateTime", TagValue::new_datetime(dt));

        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);

        // Verify all types are formatted correctly
        assert!(output.contains("EXIF:Make: Canon"));
        assert!(output.contains("EXIF:ISO: 800"));
        assert!(output.contains("EXIF:FNumber: 2.8"));
        assert!(output.contains("EXIF:ExposureTime: 1/100"));
        assert!(output.contains("EXIF:ThumbnailData: (Binary, 4 bytes)"));
        assert!(output.contains("EXIF:DateTime: 2023-06-15T12:30:00+00:00"));
    }

    #[test]
    fn test_human_readable_formatter_with_filter() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = HumanReadableFormatter;
        let filter = vec!["EXIF:Make".to_string(), "EXIF:ISO".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // Only filtered tags should appear
        assert!(output.contains("EXIF:Make: Canon"));
        assert!(output.contains("EXIF:ISO: 400"));
        assert!(!output.contains("EXIF:Model"));
    }

    #[test]
    fn test_human_readable_formatter_resolves_orientation_enum() {
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Orientation", TagValue::new_integer(6));

        let formatter = HumanReadableFormatter;
        let output = formatter.format(&metadata, None);

        assert!(output.contains("IFD0:Orientation: Rotate 90 CW"));
    }

    #[test]
    fn test_human_readable_formatter_filter_nonexistent_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = HumanReadableFormatter;
        let filter = vec!["EXIF:NonExistent".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // No matching tags, should return empty string
        assert_eq!(output, "");
    }

    #[test]
    fn test_json_formatter_empty_metadata() {
        let metadata = MetadataMap::new();
        let formatter = JsonFormatter;
        let output = formatter.format(&metadata, None);

        // Should be valid JSON array (Perl ExifTool compatibility)
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
        assert!(parsed[0].is_object());
        assert_eq!(parsed[0].as_object().unwrap().len(), 0);
    }

    #[test]
    fn test_json_formatter_basic() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = JsonFormatter;
        let output = formatter.format(&metadata, None);

        // Verify it's valid JSON array
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);

        // Verify content (note: TagValue serializes with type/value structure)
        let obj = parsed[0].as_object().unwrap();
        assert!(obj.contains_key("EXIF:Make"));
        assert!(obj.contains_key("EXIF:ISO"));
    }

    #[test]
    fn test_json_formatter_valid_json_structure() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS R5"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(800));
        metadata.insert("EXIF:FNumber", TagValue::new_float(2.8));

        let formatter = JsonFormatter;
        let output = formatter.format(&metadata, None);

        // Verify it's parseable by serde_json (same as jq would use)
        let result: Result<serde_json::Value, _> = serde_json::from_str(&output);
        assert!(result.is_ok(), "JSON should be valid and parseable");

        let parsed = result.unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
        assert_eq!(parsed[0].as_object().unwrap().len(), 4);
    }

    #[test]
    fn test_json_formatter_applies_enum_print_conversion() {
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Orientation", TagValue::new_integer(1));

        let formatter = JsonFormatter;
        let output = formatter.format(&metadata, None);

        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        let value = parsed[0]
            .as_object()
            .and_then(|obj| obj.get("IFD0:Orientation"))
            .and_then(|v| v.as_str());

        assert_eq!(value, Some("Horizontal (normal)"));
    }

    #[test]
    fn test_json_formatter_with_filter() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = JsonFormatter;
        let filter = vec!["EXIF:Make".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // Verify only filtered tag is in JSON
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        let obj = parsed[0].as_object().unwrap();
        assert_eq!(obj.len(), 1);
        assert!(obj.contains_key("EXIF:Make"));
        assert!(!obj.contains_key("EXIF:Model"));
        assert!(!obj.contains_key("EXIF:ISO"));
    }

    #[test]
    fn test_json_formatter_filter_empty_result() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = JsonFormatter;
        let filter = vec!["EXIF:NonExistent".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // Should be valid JSON array with empty object
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
        assert_eq!(parsed[0].as_object().unwrap().len(), 0);
    }

    #[test]
    fn test_format_tag_value_string() {
        let value = TagValue::new_string("Test String");
        assert_eq!(format_tag_value("EXIF:Make", &value), "Test String");
    }

    #[test]
    fn test_format_tag_value_integer() {
        let value = TagValue::new_integer(42);
        assert_eq!(format_tag_value("EXIF:ISO", &value), "42");
    }

    #[test]
    fn test_format_tag_value_float() {
        let value = TagValue::new_float(2.8);
        assert_eq!(format_tag_value("EXIF:FNumber", &value), "2.8");
    }

    #[test]
    fn test_format_tag_value_rational() {
        let value = TagValue::new_rational(1, 125);
        assert_eq!(format_tag_value("EXIF:ExposureTime", &value), "1/125");
    }

    #[test]
    fn test_format_tag_value_binary() {
        let value = TagValue::new_binary(vec![0x00, 0x01, 0x02, 0x03, 0x04]);
        assert_eq!(
            format_tag_value("EXIF:MakerNote", &value),
            "(Binary, 5 bytes)"
        );
    }

    #[test]
    fn test_format_tag_value_orientation_enum() {
        let value = TagValue::new_integer(1);
        assert_eq!(
            format_tag_value("IFD0:Orientation", &value),
            "Horizontal (normal)"
        );
    }

    #[test]
    fn test_format_tag_value_datetime() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 45).unwrap();
        let value = TagValue::new_datetime(dt);
        assert_eq!(
            format_tag_value("EXIF:DateTime", &value),
            "2023-12-25T10:30:45+00:00"
        );
    }

    #[test]
    fn test_format_tag_value_struct() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert("key".to_string(), TagValue::new_string("value"));
        let value = TagValue::new_struct(map);
        assert_eq!(
            format_tag_value("XMP-dc:Subject", &value),
            "(Structured data)"
        );
    }

    // CSV Formatter Tests
    #[test]
    fn test_csv_formatter_empty_metadata() {
        let metadata = MetadataMap::new();
        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);
        assert_eq!(output, "");
    }

    #[test]
    fn test_csv_formatter_single_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify CSV format
        assert!(output.starts_with("Tag,Value\n"));
        assert!(output.contains("EXIF:Make,Canon"));

        // Verify it's parseable as CSV
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_csv_formatter_multiple_tags_sorted() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify header
        assert!(output.starts_with("Tag,Value\n"));

        // Parse CSV to verify structure
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 3);

        // Verify tags are sorted alphabetically
        assert_eq!(records[0].get(0), Some("EXIF:ISO"));
        assert_eq!(records[0].get(1), Some("400"));
        assert_eq!(records[1].get(0), Some("EXIF:Make"));
        assert_eq!(records[1].get(1), Some("Canon"));
        assert_eq!(records[2].get(0), Some("EXIF:Model"));
        assert_eq!(records[2].get(1), Some("EOS 5D"));
    }

    #[test]
    fn test_csv_formatter_resolves_orientation_enum() {
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Orientation", TagValue::new_integer(3));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        assert!(output.contains("IFD0:Orientation,Rotate 180"));
    }

    #[test]
    fn test_csv_formatter_all_value_types() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(800));
        metadata.insert("EXIF:FNumber", TagValue::new_float(2.8));
        metadata.insert("EXIF:ExposureTime", TagValue::new_rational(1, 100));
        metadata.insert(
            "EXIF:ThumbnailData",
            TagValue::new_binary(vec![0xFF, 0xD8, 0xFF, 0xE0]),
        );

        let dt = Utc.with_ymd_and_hms(2023, 6, 15, 12, 30, 0).unwrap();
        metadata.insert("EXIF:DateTime", TagValue::new_datetime(dt));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify all types are formatted correctly in CSV
        assert!(output.contains("EXIF:Make,Canon"));
        assert!(output.contains("EXIF:ISO,800"));
        assert!(output.contains("EXIF:FNumber,2.8"));
        assert!(output.contains("EXIF:ExposureTime,1/100"));
        assert!(output.contains("EXIF:ThumbnailData,\"(Binary, 4 bytes)\""));
        assert!(output.contains("EXIF:DateTime,2023-06-15T12:30:00+00:00"));

        // Verify it's valid parseable CSV
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records.len(), 6);
    }

    #[test]
    fn test_csv_formatter_with_filter() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS 5D"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(400));

        let formatter = CsvFormatter;
        let filter = vec!["EXIF:Make".to_string(), "EXIF:ISO".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // Verify only filtered tags appear
        assert!(output.contains("EXIF:Make,Canon"));
        assert!(output.contains("EXIF:ISO,400"));
        assert!(!output.contains("EXIF:Model"));

        // Verify CSV structure
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().collect();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_csv_formatter_filter_nonexistent_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let formatter = CsvFormatter;
        let filter = vec!["EXIF:NonExistent".to_string()];
        let output = formatter.format(&metadata, Some(&filter));

        // No matching tags, should return empty string
        assert_eq!(output, "");
    }

    #[test]
    fn test_csv_formatter_special_characters() {
        let mut metadata = MetadataMap::new();
        // Test comma in value (should be quoted by csv crate)
        metadata.insert("EXIF:Artist", TagValue::new_string("Doe, John"));
        // Test quotes in value (should be escaped)
        metadata.insert("EXIF:Copyright", TagValue::new_string("Copyright \"2023\""));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify CSV handles special characters correctly
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let records: Vec<_> = rdr.records().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 2);

        // CSV reader should correctly parse values with commas and quotes
        assert!(records.iter().any(|r| r.get(1) == Some("Doe, John")));
        assert!(records
            .iter()
            .any(|r| r.get(1) == Some("Copyright \"2023\"")));
    }

    #[test]
    fn test_csv_formatter_valid_csv_structure() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("EXIF:Model", TagValue::new_string("EOS R5"));
        metadata.insert("EXIF:ISO", TagValue::new_integer(800));
        metadata.insert("EXIF:FNumber", TagValue::new_float(2.8));

        let formatter = CsvFormatter;
        let output = formatter.format(&metadata, None);

        // Verify it's parseable by csv crate (same as Excel/pandas would use)
        // Check headers
        let mut rdr = csv::Reader::from_reader(output.as_bytes());
        let headers = rdr.headers().unwrap();
        assert_eq!(headers.len(), 2);
        assert_eq!(headers.get(0), Some("Tag"));
        assert_eq!(headers.get(1), Some("Value"));

        // Check records
        let records: Vec<_> = rdr.records().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 4);
    }
}
