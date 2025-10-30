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
            let formatted_value = format_tag_value(tag_value);
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
        let metadata_to_serialize = if let Some(filter) = filter_tags {
            let filtered: MetadataMap = metadata
                .iter()
                .filter(|(name, _)| filter.contains(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            filtered
        } else {
            metadata.clone()
        };

        // Serialize to pretty JSON wrapped in an array for Perl ExifTool compatibility
        // Perl ExifTool outputs: [{...}] (array with one object per file)
        // This allows processing multiple files with consistent JSON structure
        match serde_json::to_string_pretty(&vec![metadata_to_serialize]) {
            Ok(json) => json,
            Err(e) => {
                // Fallback error message if serialization fails
                format!("[{{\"error\": \"Failed to serialize metadata: {}\"}}]", e)
            }
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
            let formatted_value = format_tag_value(tag_value);
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
fn format_tag_value(value: &TagValue) -> String {
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
        assert_eq!(format_tag_value(&value), "Test String");
    }

    #[test]
    fn test_format_tag_value_integer() {
        let value = TagValue::new_integer(42);
        assert_eq!(format_tag_value(&value), "42");
    }

    #[test]
    fn test_format_tag_value_float() {
        let value = TagValue::new_float(2.8);
        assert_eq!(format_tag_value(&value), "2.8");
    }

    #[test]
    fn test_format_tag_value_rational() {
        let value = TagValue::new_rational(1, 125);
        assert_eq!(format_tag_value(&value), "1/125");
    }

    #[test]
    fn test_format_tag_value_binary() {
        let value = TagValue::new_binary(vec![0x00, 0x01, 0x02, 0x03, 0x04]);
        assert_eq!(format_tag_value(&value), "(Binary, 5 bytes)");
    }

    #[test]
    fn test_format_tag_value_datetime() {
        let dt = Utc.with_ymd_and_hms(2023, 12, 25, 10, 30, 45).unwrap();
        let value = TagValue::new_datetime(dt);
        assert_eq!(format_tag_value(&value), "2023-12-25T10:30:45+00:00");
    }

    #[test]
    fn test_format_tag_value_struct() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert("key".to_string(), TagValue::new_string("value"));
        let value = TagValue::new_struct(map);
        assert_eq!(format_tag_value(&value), "(Structured data)");
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
