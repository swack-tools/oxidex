//! Photo Mechanic MakerNote parser
//!
//! Parses Photo Mechanic workflow and IPTC metadata stored in MakerNotes.
//! Photo Mechanic is a professional photo browser and workflow tool
//! used by sports photographers, photojournalists, and agencies.
//!
//! ## Supported Versions
//! - Photo Mechanic 6 (current)
//! - Photo Mechanic Plus
//! - Photo Mechanic 5
//! - Photo Mechanic 4.6
//!
//! ## Key Features
//! - IPTC workflow status
//! - Star ratings and color classes
//! - Keywords and categories
//! - Caption and headline
//! - Copyright and credit information
//! - Location metadata (city, state, country)
//! - Person/subject identification
//! - Batch tagging metadata
//! - Code replacement variables
//! - Structured keywords
//! - Ingestion settings
//! - Contact sheet information
//! - FTP upload metadata
//!
//! ## Architecture
//! Photo Mechanic stores workflow metadata in IPTC-compatible
//! formats within MakerNotes for rapid access during culling
//! and selection workflows.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// Photo Mechanic MakerNote Tag IDs
const PM_VERSION: u16 = 0x0001; // Photo Mechanic version
const PM_RATING: u16 = 0x0010; // Star rating (0-5)
const PM_COLOR_CLASS: u16 = 0x0011; // Color class (0-8)
const PM_TAGGED: u16 = 0x0012; // Tagged flag
const PM_CAPTION: u16 = 0x0020; // Caption/Description
const PM_HEADLINE: u16 = 0x0021; // Headline
const PM_KEYWORDS: u16 = 0x0022; // Keywords (semicolon-separated)
const PM_CATEGORIES: u16 = 0x0023; // Categories
const PM_SUPP_CATEGORIES: u16 = 0x0024; // Supplemental categories
const PM_COPYRIGHT: u16 = 0x0030; // Copyright notice
const PM_CREDIT: u16 = 0x0031; // Credit
const PM_BYLINE: u16 = 0x0032; // By-line (photographer)
const PM_BYLINE_TITLE: u16 = 0x0033; // By-line title
const PM_SOURCE: u16 = 0x0034; // Source
const PM_OBJECT_NAME: u16 = 0x0035; // Object name (title)
const PM_CITY: u16 = 0x0040; // City
const PM_PROVINCE_STATE: u16 = 0x0041; // Province/State
const PM_COUNTRY_NAME: u16 = 0x0042; // Country name
const PM_COUNTRY_CODE: u16 = 0x0043; // Country code (ISO)
const PM_LOCATION: u16 = 0x0044; // Sub-location
const PM_PERSON_SHOWN: u16 = 0x0050; // Person shown in image
const PM_EVENT: u16 = 0x0051; // Event
const PM_SUBJECT_CODE: u16 = 0x0052; // Subject reference code
const PM_INSTRUCTIONS: u16 = 0x0060; // Special instructions
const PM_TRANSMISSION_REF: u16 = 0x0061; // Transmission reference
const PM_URGENCY: u16 = 0x0062; // Urgency (1-8)
const PM_JOB_ID: u16 = 0x0070; // Job identifier
const PM_EDIT_STATUS: u16 = 0x0071; // Edit status
const PM_FIXTURE_ID: u16 = 0x0072; // Fixture identifier
const PM_CONTACT: u16 = 0x0080; // Contact information
const PM_WEBSITE: u16 = 0x0081; // Creator's website
const PM_EMAIL: u16 = 0x0082; // Creator's email
const PM_PHONE: u16 = 0x0083; // Creator's phone
const PM_USAGE_TERMS: u16 = 0x0090; // Usage terms/rights
const PM_WEB_STATEMENT: u16 = 0x0091; // Web statement of rights
const PM_INGESTION_TIME: u16 = 0x00A0; // Ingestion timestamp
const PM_CODE_REPLACEMENT: u16 = 0x00B0; // Code replacement applied
const PM_STRUCTURED_KEYWORDS: u16 = 0x00B1; // Structured keywords count
const PM_STATIONERY_APPLIED: u16 = 0x00C0; // Stationery pad applied
const PM_STATIONERY_NAME: u16 = 0x00C1; // Stationery pad name

// Photo Mechanic signature
const PHOTOMECHANIC_SIGNATURE: &[u8] = b"PhotoMech";

/// Decodes color class
///
/// # Arguments
/// * `value` - Color class code
///
/// # Returns
/// Human-readable color class
fn decode_color_class(value: i16) -> String {
    match value {
        0 => "None".to_string(),
        1 => "Red".to_string(),
        2 => "Yellow".to_string(),
        3 => "Green".to_string(),
        4 => "Blue".to_string(),
        5 => "Purple".to_string(),
        6 => "Orange".to_string(),
        7 => "Gray".to_string(),
        8 => "White".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes urgency level
///
/// # Arguments
/// * `value` - Urgency code (1-8)
///
/// # Returns
/// Human-readable urgency level
fn decode_urgency(value: i16) -> String {
    match value {
        1 => "High (1)".to_string(),
        2 => "2".to_string(),
        3 => "3".to_string(),
        4 => "4".to_string(),
        5 => "Normal (5)".to_string(),
        6 => "6".to_string(),
        7 => "7".to_string(),
        8 => "Low (8)".to_string(),
        _ => "None".to_string(),
    }
}

/// Decodes edit status
///
/// # Arguments
/// * `value` - Edit status code
///
/// # Returns
/// Human-readable edit status
fn decode_edit_status(value: i16) -> String {
    match value {
        0 => "Original".to_string(),
        1 => "Edited".to_string(),
        2 => "Selected".to_string(),
        3 => "Rejected".to_string(),
        4 => "For Review".to_string(),
        5 => "Approved".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Formats star rating
///
/// # Arguments
/// * `value` - Rating (0-5)
///
/// # Returns
/// Formatted rating string
fn format_rating(value: i16) -> String {
    if !(0..=5).contains(&value) {
        return "None".to_string();
    }
    if value == 0 {
        "None".to_string()
    } else {
        format!("{} stars", value)
    }
}

/// Extracts an ASCII string from IFD entry
///
/// # Arguments
/// * `entry` - IFD entry containing the string
/// * `data` - Raw MakerNote data
///
/// # Returns
/// Extracted string or None if extraction fails
fn extract_string(entry: &IfdEntry, data: &[u8]) -> Option<String> {
    if entry.field_type != 2 {
        return None;
    }

    let offset = entry.value_offset as usize;
    let count = entry.value_count as usize;

    if count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        let s = String::from_utf8_lossy(&bytes[..count.min(4)])
            .trim_end_matches('\0')
            .to_string();
        return if s.is_empty() { None } else { Some(s) };
    }

    if offset + count > data.len() {
        return None;
    }

    let s = String::from_utf8_lossy(&data[offset..offset + count])
        .trim_end_matches('\0')
        .to_string();

    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Photo Mechanic MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct PhotoMechanicParser;

impl PhotoMechanicParser {
    /// Creates a new Photo Mechanic parser instance
    pub fn new() -> Self {
        PhotoMechanicParser
    }
}

impl MakerNoteParser for PhotoMechanicParser {
    fn manufacturer_name(&self) -> &'static str {
        "Photo Mechanic"
    }

    fn tag_prefix(&self) -> &'static str {
        "PhotoMechanic:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 9 {
            return false;
        }
        data.starts_with(PHOTOMECHANIC_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("Photo Mechanic MakerNote data too short".to_string());
        }

        // Skip Photo Mechanic signature if present
        let start_offset = if data.starts_with(PHOTOMECHANIC_SIGNATURE) {
            9
        } else {
            0
        };
        let parse_data = &data[start_offset..];

        if parse_data.len() < 2 {
            return Ok(());
        }

        // Read number of entries
        let num_entries = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([parse_data[0], parse_data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([parse_data[0], parse_data[1]]),
        } as usize;

        if num_entries == 0 || num_entries > 200 {
            return Ok(());
        }

        let mut offset = 2;
        let entry_size = 12;

        for _ in 0..num_entries {
            if offset + entry_size > parse_data.len() {
                break;
            }

            let entry_data = &parse_data[offset..offset + entry_size];

            let tag = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[0], entry_data[1]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[0], entry_data[1]]),
            };

            let field_type = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry_data[2], entry_data[3]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry_data[2], entry_data[3]]),
            };

            let count = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([entry_data[4], entry_data[5], entry_data[6], entry_data[7]])
                }
            };

            let value_offset = match byte_order {
                ByteOrder::LittleEndian => u32::from_le_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
                ByteOrder::BigEndian => u32::from_be_bytes([
                    entry_data[8],
                    entry_data[9],
                    entry_data[10],
                    entry_data[11],
                ]),
            };

            let entry = IfdEntry {
                tag_id: tag,
                field_type,
                value_count: count,
                value_offset,
            };

            // Extract value based on tag type
            match tag {
                PM_VERSION | PM_CAPTION | PM_HEADLINE | PM_KEYWORDS | PM_CATEGORIES
                | PM_SUPP_CATEGORIES | PM_COPYRIGHT | PM_CREDIT | PM_BYLINE | PM_BYLINE_TITLE
                | PM_SOURCE | PM_OBJECT_NAME | PM_CITY | PM_PROVINCE_STATE | PM_COUNTRY_NAME
                | PM_COUNTRY_CODE | PM_LOCATION | PM_PERSON_SHOWN | PM_EVENT | PM_SUBJECT_CODE
                | PM_INSTRUCTIONS | PM_TRANSMISSION_REF | PM_JOB_ID | PM_FIXTURE_ID
                | PM_CONTACT | PM_WEBSITE | PM_EMAIL | PM_PHONE | PM_USAGE_TERMS
                | PM_WEB_STATEMENT | PM_STATIONERY_NAME => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            PM_VERSION => "Version",
                            PM_CAPTION => "Caption",
                            PM_HEADLINE => "Headline",
                            PM_KEYWORDS => "Keywords",
                            PM_CATEGORIES => "Category",
                            PM_SUPP_CATEGORIES => "SupplementalCategories",
                            PM_COPYRIGHT => "CopyrightNotice",
                            PM_CREDIT => "Credit",
                            PM_BYLINE => "ByLine",
                            PM_BYLINE_TITLE => "ByLineTitle",
                            PM_SOURCE => "Source",
                            PM_OBJECT_NAME => "ObjectName",
                            PM_CITY => "City",
                            PM_PROVINCE_STATE => "ProvinceState",
                            PM_COUNTRY_NAME => "CountryName",
                            PM_COUNTRY_CODE => "CountryCode",
                            PM_LOCATION => "SubLocation",
                            PM_PERSON_SHOWN => "PersonShown",
                            PM_EVENT => "Event",
                            PM_SUBJECT_CODE => "SubjectCode",
                            PM_INSTRUCTIONS => "SpecialInstructions",
                            PM_TRANSMISSION_REF => "TransmissionReference",
                            PM_JOB_ID => "JobID",
                            PM_FIXTURE_ID => "FixtureID",
                            PM_CONTACT => "Contact",
                            PM_WEBSITE => "CreatorWebsite",
                            PM_EMAIL => "CreatorEmail",
                            PM_PHONE => "CreatorPhone",
                            PM_USAGE_TERMS => "UsageTerms",
                            PM_WEB_STATEMENT => "WebStatementURL",
                            PM_STATIONERY_NAME => "StationeryName",
                            _ => continue,
                        };
                        tags.insert(format!("PhotoMechanic:{}", tag_name), s);
                    }
                }

                _ => {
                    // Try to extract as i16 array
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                PM_RATING => ("Rating", format_rating(val)),
                                PM_COLOR_CLASS => ("ColorClass", decode_color_class(val)),
                                PM_TAGGED => {
                                    ("Tagged", if val != 0 { "Yes" } else { "No" }.to_string())
                                }
                                PM_URGENCY => ("Urgency", decode_urgency(val)),
                                PM_EDIT_STATUS => ("EditStatus", decode_edit_status(val)),
                                PM_CODE_REPLACEMENT => (
                                    "CodeReplacementApplied",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                PM_STRUCTURED_KEYWORDS => {
                                    ("StructuredKeywordCount", val.to_string())
                                }
                                PM_STATIONERY_APPLIED => (
                                    "StationeryApplied",
                                    if val != 0 { "Yes" } else { "No" }.to_string(),
                                ),
                                _ => continue,
                            };
                            tags.insert(format!("PhotoMechanic:{}", tag_name), formatted_value);
                        }
                    }
                }
            }

            offset += entry_size;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_photomechanic_parser_creation() {
        let parser = PhotoMechanicParser::new();
        assert_eq!(parser.manufacturer_name(), "Photo Mechanic");
        assert_eq!(parser.tag_prefix(), "PhotoMechanic:");
    }

    #[test]
    fn test_decode_color_class() {
        assert_eq!(decode_color_class(0), "None");
        assert_eq!(decode_color_class(1), "Red");
        assert_eq!(decode_color_class(3), "Green");
    }

    #[test]
    fn test_decode_urgency() {
        assert_eq!(decode_urgency(1), "High (1)");
        assert_eq!(decode_urgency(5), "Normal (5)");
        assert_eq!(decode_urgency(8), "Low (8)");
    }

    #[test]
    fn test_decode_edit_status() {
        assert_eq!(decode_edit_status(0), "Original");
        assert_eq!(decode_edit_status(2), "Selected");
        assert_eq!(decode_edit_status(5), "Approved");
    }

    #[test]
    fn test_format_rating() {
        assert_eq!(format_rating(0), "None");
        assert_eq!(format_rating(3), "3 stars");
        assert_eq!(format_rating(5), "5 stars");
    }

    #[test]
    fn test_validate_header() {
        let parser = PhotoMechanicParser::new();
        let valid_header = b"PhotoMech\x00\x01";
        assert!(parser.validate_header(valid_header));
    }
}
