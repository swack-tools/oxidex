//! vCard (VCF) contact format parser
//!
//! This parser extracts metadata from vCard files, which are text-based
//! contact information files following the vCard standard (RFC 6350).
//!
//! # Format Structure
//!
//! vCard files begin with "BEGIN:VCARD" and contain key-value pairs
//! for contact information such as name, email, telephone, etc.
//!
//! Tag names below match ExifTool's `Image::ExifTool::VCard::Main` table
//! (lib/Image/ExifTool/VCard.pm, ExifTool 13.59) so that oxidex output is
//! byte-for-byte comparable to `exiftool` for these fields.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// VCF signature: "BEGIN:VCARD"
const VCF_SIGNATURE: &[u8] = b"BEGIN:VCARD";

/// VCF/vCard parser for extracting metadata from contact files
pub struct VCFParser;

/// Maps an uppercased vCard property name to the ExifTool tag name it is
/// documented under in `VCard::Main`. `None` values are handled specially
/// below (they need value transforms), everything else is a straight
/// name (and, for base values, value) mapping.
///
/// Ref: lib/Image/ExifTool/VCard.pm %Image::ExifTool::VCard::Main (v1.07)
fn main_table_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "VERSION" => "VCardVersion",
        "FN" => "FormattedName",
        "N" => "Name",
        "ORG" => "Organization",
        "TITLE" => "JobTitle",
        "EMAIL" => "Email",
        "TEL" => "Telephone",
        "ADR" => "Address",
        "URL" => "URL",
        "NOTE" => "Note",
        "UID" => "UID",
        "REV" => "Revision",
        "NICKNAME" => "Nickname",
        "GENDER" => "Gender",
        "ANNIVERSARY" => "Anniversary",
        "IMPP" => "IMPP",
        "LANG" => "Language",
        "LOGO" => "Logo",
        "PRODID" => "Software",
        "SOUND" => "Sound",
        // Time-group / geo tags handled separately below because they need
        // value conversion (%timeInfo / geo: prefix strip), not just renaming.
        "BDAY" | "TZ" | "GEO" | "PHOTO" => return None,
        _ => return None,
    })
}

/// Splits a vCard content line into its `group.name *(";" param)` prefix and
/// its value, at the first ":" that is not inside a double-quoted parameter
/// value.
///
/// RFC 6350 section 3.3 allows parameter values to be quoted strings
/// (`DQUOTE *QSAFE-CHAR DQUOTE`) which may themselves contain ":", ";", and
/// ",". A naive `str::split_once(':')` finds the first colon anywhere in the
/// line, so a line like:
///   `ADR;type=OTHER;GEO="geo:12.3457,78.910";LABEL=Test\nLabel:;;Other Rd....`
/// gets split inside the quoted GEO parameter value instead of at the real
/// property/value boundary, corrupting the extracted value. This walks the
/// line tracking quote state so the split lands on the correct colon.
fn split_property_line(line: &str) -> Option<(&str, &str)> {
    let mut in_quotes = false;
    for (idx, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                return Some((&line[..idx], &line[idx + 1..]));
            }
            _ => {}
        }
    }
    None
}

/// Converts a vCard/iCalendar date-time value to ExifTool's EXIF-style
/// separators, matching the `%timeInfo` ValueConv in VCard.pm:
///   YYYYMMDDThhmmss(Z?) -> YYYY:MM:DD hh:mm:ss(Z?)
///   YYYYMMDD            -> YYYY:MM:DD
///   YYYY-MM-DD           -> YYYY:MM:DD
fn convert_vcard_time(val: &str) -> String {
    let bytes: Vec<char> = val.chars().collect();
    // YYYYMMDDThhmmssZ?
    if bytes.len() >= 15
        && bytes[..8].iter().all(|c| c.is_ascii_digit())
        && bytes[8] == 'T'
        && bytes[9..15].iter().all(|c| c.is_ascii_digit())
    {
        let date = &val[0..8];
        let time = &val[9..15];
        let rest = &val[15..];
        return format!(
            "{}:{}:{} {}:{}:{}{}",
            &date[0..4],
            &date[4..6],
            &date[6..8],
            &time[0..2],
            &time[2..4],
            &time[4..6],
            rest
        );
    }
    // YYYYMMDD
    if bytes.len() == 8 && bytes.iter().all(|c| c.is_ascii_digit()) {
        return format!("{}:{}:{}", &val[0..4], &val[4..6], &val[6..8]);
    }
    // YYYY-MM-DD
    if bytes.len() == 10
        && bytes[4] == '-'
        && bytes[7] == '-'
        && bytes[0..4].iter().all(|c| c.is_ascii_digit())
        && bytes[5..7].iter().all(|c| c.is_ascii_digit())
        && bytes[8..10].iter().all(|c| c.is_ascii_digit())
    {
        return format!("{}:{}:{}", &val[0..4], &val[5..7], &val[8..10]);
    }
    val.to_string()
}

impl VCFParser {
    /// Verifies VCF signature by checking for "BEGIN:VCARD" at the start of the file
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - File has valid VCF signature
    /// * `Ok(false)` - File does not have VCF signature
    /// * `Err(ExifToolError)` - I/O error reading file
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 11 {
            return Ok(false);
        }
        let header = reader.read(0, 11)?;
        Ok(header == VCF_SIGNATURE)
    }

    /// Parse vCard content to extract metadata, using ExifTool's VCard::Main
    /// tag names (see `main_table_name`) wherever we can derive the value with
    /// certainty.
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted vCard metadata
    /// * `Err(ExifToolError)` - Parse error or invalid UTF-8
    pub fn parse_vcard_content(reader: &dyn FileReader) -> Result<MetadataMap> {
        let size = reader.size() as usize;
        // Read first 8KB to avoid loading huge files entirely into memory
        let content = reader.read(0, size.min(8192))?;

        let text = std::str::from_utf8(content)
            .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8: {}", e)))?;

        let mut metadata = MetadataMap::new();
        let mut has_photo = false;
        let mut has_organization = false;
        let mut has_email = false;
        let mut has_phone = false;
        let mut has_address = false;
        let mut has_url = false;
        let mut vcard_count = 0;

        // Count vCARDs and collect feature flags
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed == "BEGIN:VCARD" {
                vcard_count += 1;
            }
        }

        // Parse vCard line by line
        for line in text.lines() {
            if let Some((raw_key, raw_value)) = split_property_line(line) {
                // Strip any ";PARAM=..." group parameters from the key so
                // "TEL;TYPE=CELL" still matches on "TEL". We don't yet fold
                // the TYPE into the tag name the way ExifTool does (e.g.
                // "TelephoneCell") -- only the base tag is emitted.
                let key_base = raw_key.split(';').next().unwrap_or(raw_key).trim();
                let key = key_base.to_ascii_uppercase();
                let value = raw_value.trim();

                match key.as_str() {
                    "VERSION" => {
                        metadata.insert(
                            "VCardVersion".to_string(),
                            TagValue::String(value.to_string()),
                        );
                        // Add VCF:Version for Worker 28 compatibility
                        metadata.insert(
                            "VCF:Version".to_string(),
                            TagValue::new_string(value.to_string()),
                        );
                    }
                    "BDAY" => {
                        metadata.insert(
                            "Birthday".to_string(),
                            TagValue::String(convert_vcard_time(value)),
                        );
                    }
                    "TZ" => {
                        metadata
                            .insert("TimeZone".to_string(), TagValue::String(value.to_string()));
                    }
                    "GEO" => {
                        // VCard 4.0 prefixes with "geo:"; ValueConv strips it.
                        let stripped = value.strip_prefix("geo:").unwrap_or(value);
                        metadata.insert(
                            "Geolocation".to_string(),
                            TagValue::String(stripped.to_string()),
                        );
                    }
                    "PHOTO" => {
                        has_photo = true;
                    }
                    "ORG" => {
                        has_organization = true;
                        metadata.insert(
                            "Organization".to_string(),
                            TagValue::String(value.to_string()),
                        );
                    }
                    "ADR" => {
                        has_address = true;
                        metadata.insert("Address".to_string(), TagValue::String(value.to_string()));
                    }
                    "URL" => {
                        has_url = true;
                        metadata.insert("URL".to_string(), TagValue::String(value.to_string()));
                    }
                    "EMAIL" => {
                        has_email = true;
                        metadata.insert("Email".to_string(), TagValue::String(value.to_string()));
                    }
                    "TEL" => {
                        has_phone = true;
                        metadata
                            .insert("Telephone".to_string(), TagValue::String(value.to_string()));
                    }
                    _ => {
                        if let Some(name) = main_table_name(&key) {
                            metadata.insert(name.to_string(), TagValue::String(value.to_string()));
                        }
                    }
                }
            }
        }

        // Add Worker 28 tags for vCard properties
        metadata.insert(
            "VCF:Count".to_string(),
            TagValue::new_integer(vcard_count as i64),
        );

        metadata.insert(
            "VCF:HasPhoto".to_string(),
            TagValue::new_string(if has_photo { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasOrganization".to_string(),
            TagValue::new_string(if has_organization { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasEmail".to_string(),
            TagValue::new_string(if has_email { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasPhone".to_string(),
            TagValue::new_string(if has_phone { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasAddress".to_string(),
            TagValue::new_string(if has_address { "true" } else { "false" }),
        );

        metadata.insert(
            "VCF:HasURL".to_string(),
            TagValue::new_string(if has_url { "true" } else { "false" }),
        );

        Ok(metadata)
    }
}

impl FormatParser for VCFParser {
    /// Parses a VCF file and extracts metadata
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Successfully extracted metadata including FileType, FileSize, and vCard fields
    /// * `Err(ExifToolError)` - Invalid signature or parse error
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid VCF signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert(
            "FileType".to_string(),
            TagValue::String("vCard".to_string()),
        );

        // Parse vCard content and merge with basic metadata
        let vcard_metadata = Self::parse_vcard_content(reader)?;
        for (key, value) in vcard_metadata {
            metadata.insert(key, value);
        }

        Ok(metadata)
    }

    /// Indicates whether this parser supports the given file format
    ///
    /// # Arguments
    ///
    /// * `format` - FileFormat to check
    ///
    /// # Returns
    ///
    /// * `true` if format is VCF
    /// * `false` otherwise
    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::VCF)
    }
}

/// Parses metadata from VCF files.
///
/// This is a convenience function that creates a VCFParser and invokes it.
///
/// # Arguments
///
/// * `reader` - FileReader implementation for accessing file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_vcf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = VCFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_vcard_time_handles_date_only() {
        assert_eq!(convert_vcard_time("19900101"), "1990:01:01");
        assert_eq!(convert_vcard_time("1990-01-01"), "1990:01:01");
    }

    #[test]
    fn convert_vcard_time_handles_datetime() {
        assert_eq!(
            convert_vcard_time("20200101T000000Z"),
            "2020:01:01 00:00:00Z"
        );
    }

    #[test]
    fn main_table_name_maps_known_tags() {
        assert_eq!(main_table_name("TITLE"), Some("JobTitle"));
        assert_eq!(main_table_name("PRODID"), Some("Software"));
        assert_eq!(main_table_name("UID"), Some("UID"));
        assert_eq!(main_table_name("BOGUS"), None);
    }

    #[test]
    fn split_property_line_ignores_colons_inside_quoted_params() {
        // RFC 6350 quoted parameter values may contain ":" (e.g. a "geo:"
        // URI), which must not be mistaken for the property/value delimiter.
        let line = r#"ADR;type=OTHER;GEO="geo:12.3457,78.910";LABEL=Test\nLabel:;;Other Rd.;City;ON;K0K0K0;Canada"#;
        let (key, value) = split_property_line(line).expect("line should split");
        assert_eq!(
            key,
            r#"ADR;type=OTHER;GEO="geo:12.3457,78.910";LABEL=Test\nLabel"#
        );
        assert_eq!(value, ";;Other Rd.;City;ON;K0K0K0;Canada");
    }

    #[test]
    fn split_property_line_handles_plain_lines() {
        assert_eq!(
            split_property_line("TEL;type=CELL:555-0000"),
            Some(("TEL;type=CELL", "555-0000"))
        );
        assert_eq!(split_property_line("no-colon-here"), None);
    }
}
