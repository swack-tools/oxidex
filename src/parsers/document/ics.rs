//! ICS (iCalendar) format parser
//!
//! Parses ICS (iCalendar) files to extract calendar metadata

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// Parser for ICS (iCalendar) files
///
/// Extracts metadata from ICS calendar files including version, product ID,
/// calendar method, and counts of events and todos.
pub struct ICSParser;

impl ICSParser {
    /// Verifies the ICS file by checking for "BEGIN:VCALENDAR" and "VERSION:" markers
    pub fn verify_signature(data: &[u8]) -> bool {
        if let Ok(text) = std::str::from_utf8(data) {
            // ICS files must start with BEGIN:VCALENDAR and contain VERSION
            text.contains("BEGIN:VCALENDAR") && text.contains("VERSION:")
        } else {
            false
        }
    }

    /// Extracts a simple value from ICS format (KEY:VALUE)
    fn extract_value(text: &str, key: &str) -> Option<String> {
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with(key) && trimmed.contains(':') {
                if let Some(value) = trimmed.strip_prefix(key) {
                    if let Some(val) = value.strip_prefix(':') {
                        return Some(val.trim().to_string());
                    }
                }
            }
        }
        None
    }

    /// Counts occurrences of a component type (e.g., VEVENT, VTODO)
    fn count_component(text: &str, component: &str) -> i64 {
        let begin_marker = format!("BEGIN:{}", component);
        text.lines()
            .filter(|line| line.trim() == begin_marker)
            .count() as i64
    }

    /// Extracts the first date found in the calendar
    fn extract_first_date(text: &str) -> Option<String> {
        // Look for DTSTART, DTEND, or other date fields
        let date_keys = ["DTSTART", "DTEND", "DTSTAMP", "CREATED", "LAST-MODIFIED"];

        for line in text.lines() {
            let trimmed = line.trim();
            for date_key in &date_keys {
                if trimmed.starts_with(date_key) && trimmed.contains(':') {
                    if let Some(value) = trimmed.split(':').nth(1) {
                        // Extract just the date part (YYYYMMDD or YYYYMMDDTHHMMSS)
                        let date_str = value.trim();
                        if !date_str.is_empty() && (date_str.len() == 8 || date_str.contains('T')) {
                            return Some(date_str.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Extracts the last date found in the calendar
    fn extract_last_date(text: &str) -> Option<String> {
        // Look for DTSTART, DTEND, or other date fields (in reverse)
        let date_keys = ["DTSTART", "DTEND", "DTSTAMP", "CREATED", "LAST-MODIFIED"];
        let mut last_date: Option<String> = None;

        for line in text.lines() {
            let trimmed = line.trim();
            for date_key in &date_keys {
                if trimmed.starts_with(date_key) && trimmed.contains(':') {
                    if let Some(value) = trimmed.split(':').nth(1) {
                        let date_str = value.trim();
                        if !date_str.is_empty() && (date_str.len() == 8 || date_str.contains('T')) {
                            last_date = Some(date_str.to_string());
                        }
                    }
                }
            }
        }
        last_date
    }
}

impl FormatParser for ICSParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Read file data
        let file_size = reader.size() as usize;
        let data = reader.read(0, file_size)?;

        // Verify ICS signature
        if !Self::verify_signature(data) {
            return Err(ExifToolError::parse_error("Invalid ICS signature"));
        }

        // Convert to UTF-8 string
        let text = std::str::from_utf8(data)
            .map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in ICS file"))?;

        let mut metadata = MetadataMap::new();

        // Set basic file info
        metadata.insert("FileType".to_string(), TagValue::String("ICS".to_string()));
        metadata.insert("FileSize".to_string(), TagValue::Integer(file_size as i64));
        metadata.insert(
            "MIMEType".to_string(),
            TagValue::String("text/calendar".to_string()),
        );

        // Extract VERSION (ICS:Version) - Worker 27 requirement
        if let Some(version) = Self::extract_value(text, "VERSION") {
            metadata.insert("ICS:Version".to_string(), TagValue::new_string(version));
        }

        // Extract PRODID (ICS:ProductID) - Worker 27 requirement
        if let Some(prodid) = Self::extract_value(text, "PRODID") {
            metadata.insert("ICS:ProductID".to_string(), TagValue::new_string(prodid));
        }

        // Extract CALSCALE (ICS:CalScale) - Worker 27 requirement
        if let Some(calscale) = Self::extract_value(text, "CALSCALE") {
            metadata.insert("ICS:CalScale".to_string(), TagValue::new_string(calscale));
        }

        // Extract METHOD (ICS:Method) - Worker 27 requirement
        if let Some(method) = Self::extract_value(text, "METHOD") {
            metadata.insert("ICS:Method".to_string(), TagValue::new_string(method));
        }

        // Count VEVENT entries (ICS:EventCount) - Worker 27 requirement
        let event_count = Self::count_component(text, "VEVENT");
        if event_count > 0 {
            metadata.insert(
                "ICS:EventCount".to_string(),
                TagValue::new_integer(event_count),
            );
        }

        // Count VTODO entries (ICS:TodoCount) - Worker 27 requirement
        let todo_count = Self::count_component(text, "VTODO");
        if todo_count > 0 {
            metadata.insert(
                "ICS:TodoCount".to_string(),
                TagValue::new_integer(todo_count),
            );
        }

        // Extract first date (ICS:FirstDate) - Worker 27 requirement
        if let Some(first_date) = Self::extract_first_date(text) {
            metadata.insert(
                "ICS:FirstDate".to_string(),
                TagValue::new_string(first_date),
            );
        }

        // Extract last date (ICS:LastDate) - Worker 27 requirement
        if let Some(last_date) = Self::extract_last_date(text) {
            metadata.insert("ICS:LastDate".to_string(), TagValue::new_string(last_date));
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::ICS)
    }
}

/// Parses metadata from ICS files.
///
/// This is a convenience wrapper around ICSParser that provides a functional API.
pub fn parse_ics_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = ICSParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::BufferedReader;

    #[test]
    fn test_ics_basic_parsing() {
        let ics_data = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//Test//EN\r\nCALSCALE:GREGORIAN\r\nMETHOD:PUBLISH\r\nBEGIN:VEVENT\r\nDTSTART:20240101T120000Z\r\nDTEND:20240101T130000Z\r\nSUMMARY:Test Event\r\nEND:VEVENT\r\nEND:VCALENDAR";

        let reader = BufferedReader::from_bytes(ics_data);
        let parser = ICSParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(metadata.get("FileType").unwrap().as_string(), Some("ICS"));
        assert_eq!(
            metadata.get("ICS:Version").unwrap().as_string(),
            Some("2.0")
        );
        assert_eq!(
            metadata.get("ICS:ProductID").unwrap().as_string(),
            Some("-//Test//Test//EN")
        );
        assert_eq!(
            metadata.get("ICS:CalScale").unwrap().as_string(),
            Some("GREGORIAN")
        );
        assert_eq!(
            metadata.get("ICS:Method").unwrap().as_string(),
            Some("PUBLISH")
        );
        assert_eq!(
            metadata.get("ICS:EventCount").unwrap().as_integer(),
            Some(1)
        );
    }

    #[test]
    fn test_ics_invalid() {
        let invalid_data = b"Not an ICS file";
        let reader = BufferedReader::from_bytes(invalid_data);
        let parser = ICSParser;

        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
