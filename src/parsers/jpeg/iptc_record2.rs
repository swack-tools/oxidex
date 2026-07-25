//! IPTC Record 2 (Application Record) Parser
//!
//! This module handles parsing of IPTC-IIM Record 2 (Application Record) tags.
//! Record 2 contains editorial metadata as defined in the IPTC-IIM specification.
//!
//! # Application Record Tags
//!
//! Record 2 contains editorial metadata about the content:
//! - RecordVersion (0): Version of the IIM Application Record
//! - ObjectName (5): Short title or caption headline
//! - EditStatus (7): Status of editorial object
//! - EditorialUpdate (8): Editorial update indicator
//! - Urgency (10): Editorial urgency
//! - Subject (12): Reference to subject matter
//! - Category (15): Editorial category
//! - SupplementalCategories (20): Additional categories
//! - FixtureIdentifier (22): Identifier for recurring events
//! - Keywords (25): Controlled keywords
//! - LocationCode (26): Code for object location
//! - LocationName (27): Name for object location
//! - ReleaseDate (30): Date object may be published
//! - ReleaseTime (35): Time object may be published
//! - ExpirationDate (37): Date object becomes non-current
//! - ExpirationTime (38): Time object becomes non-current
//! - SpecialInstructions (40): Editorial instructions
//! - ActionAdvised (42): Instructions on object use
//! - ReferenceService (45): Reference to another service
//! - ReferenceDate (47): Date of referenced service
//! - ReferenceNumber (50): Number from referenced service
//! - DateCreated (55): Date of content creation
//! - TimeCreated (60): Time of content creation
//! - DigitalCreationDate (62): Date content was digitalized
//! - DigitalCreationTime (63): Time content was digitalized
//! - OriginatingProgram (65): Program used to create object
//! - ProgramVersion (70): Version of originating program
//! - ObjectCycle (75): Object frequency code
//! - ByLine (80): Name of content creator
//! - ByLineTitle (85): Title of content creator
//! - City (90): City where object was created
//! - SubLocation (92): Sublocation where object was created
//! - Province-State (95): State/province where object was created
//! - Country-PrimaryLocationCode (100): Country ISO code
//! - Country-PrimaryLocationName (101): Country name
//! - OriginalTransmissionReference (103): Reference number for transmission
//! - Headline (105): Publishable headline
//! - Credit (110): Credit line for content
//! - Source (115): Source of content
//! - CopyrightNotice (116): Copyright notice
//! - Contact (118): Contact information
//! - Caption-Abstract (120): Brief description of content
//! - Writer-Editor (122): Name of person editing content
//! - ImageType (130): Type of image content
//! - ImageOrientation (131): Orientation of image
//! - LanguageIdentifier (135): Language of content
//! - AudioType (150): Type of audio content
//! - AudioDuration (151): Duration of audio
//! - AudioOutcue (152): End cue of audio
//! - VideoType (160): Type of video content
//!
//! # References
//!
//! - IPTC-IIM Specification: https://iptc.org/standards/iim/
//! - ExifTool IPTC tag documentation

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;
use crate::core::value_formatter::{format_iptc_date, format_iptc_time, format_iptc_urgency};

// =============================================================================
// CONSTANTS
// =============================================================================

/// IPTC tag marker byte (0x1C) that precedes every IPTC dataset
const IPTC_TAG_MARKER: u8 = 0x1C;

/// Record number for Application Record (Record 2)
const APPLICATION_RECORD_NUMBER: u8 = 2;

/// Minimum size for an IPTC dataset header (marker + record + dataset + length)
const MIN_DATASET_SIZE: usize = 5;

// =============================================================================
// IPTC RECORD 2 TAG DEFINITIONS
// =============================================================================

/// Dataset number for RecordVersion (IPTC Record 2, Tag 0)
const DATASET_RECORD_VERSION: u8 = 0;

/// Dataset number for ObjectName (IPTC Record 2, Tag 5)
const DATASET_OBJECT_NAME: u8 = 5;

/// Dataset number for EditStatus (IPTC Record 2, Tag 7)
const DATASET_EDIT_STATUS: u8 = 7;

/// Dataset number for EditorialUpdate (IPTC Record 2, Tag 8)
const DATASET_EDITORIAL_UPDATE: u8 = 8;

/// Dataset number for Urgency (IPTC Record 2, Tag 10)
const DATASET_URGENCY: u8 = 10;

/// Dataset number for Subject (IPTC Record 2, Tag 12) - repeatable
const DATASET_SUBJECT: u8 = 12;

/// Dataset number for Category (IPTC Record 2, Tag 15)
const DATASET_CATEGORY: u8 = 15;

/// Dataset number for SupplementalCategories (IPTC Record 2, Tag 20) - repeatable
const DATASET_SUPPLEMENTAL_CATEGORIES: u8 = 20;

/// Dataset number for FixtureIdentifier (IPTC Record 2, Tag 22)
const DATASET_FIXTURE_IDENTIFIER: u8 = 22;

/// Dataset number for Keywords (IPTC Record 2, Tag 25) - repeatable
const DATASET_KEYWORDS: u8 = 25;

/// Dataset number for LocationCode (IPTC Record 2, Tag 26) - repeatable
const DATASET_LOCATION_CODE: u8 = 26;

/// Dataset number for LocationName (IPTC Record 2, Tag 27) - repeatable
const DATASET_LOCATION_NAME: u8 = 27;

/// Dataset number for ReleaseDate (IPTC Record 2, Tag 30)
const DATASET_RELEASE_DATE: u8 = 30;

/// Dataset number for ReleaseTime (IPTC Record 2, Tag 35)
const DATASET_RELEASE_TIME: u8 = 35;

/// Dataset number for ExpirationDate (IPTC Record 2, Tag 37)
const DATASET_EXPIRATION_DATE: u8 = 37;

/// Dataset number for ExpirationTime (IPTC Record 2, Tag 38)
const DATASET_EXPIRATION_TIME: u8 = 38;

/// Dataset number for SpecialInstructions (IPTC Record 2, Tag 40)
const DATASET_SPECIAL_INSTRUCTIONS: u8 = 40;

/// Dataset number for ActionAdvised (IPTC Record 2, Tag 42)
const DATASET_ACTION_ADVISED: u8 = 42;

/// Dataset number for ReferenceService (IPTC Record 2, Tag 45) - repeatable
const DATASET_REFERENCE_SERVICE: u8 = 45;

/// Dataset number for ReferenceDate (IPTC Record 2, Tag 47) - repeatable
const DATASET_REFERENCE_DATE: u8 = 47;

/// Dataset number for ReferenceNumber (IPTC Record 2, Tag 50) - repeatable
const DATASET_REFERENCE_NUMBER: u8 = 50;

/// Dataset number for DateCreated (IPTC Record 2, Tag 55)
const DATASET_DATE_CREATED: u8 = 55;

/// Dataset number for TimeCreated (IPTC Record 2, Tag 60)
const DATASET_TIME_CREATED: u8 = 60;

/// Dataset number for DigitalCreationDate (IPTC Record 2, Tag 62)
const DATASET_DIGITAL_CREATION_DATE: u8 = 62;

/// Dataset number for DigitalCreationTime (IPTC Record 2, Tag 63)
const DATASET_DIGITAL_CREATION_TIME: u8 = 63;

/// Dataset number for OriginatingProgram (IPTC Record 2, Tag 65)
const DATASET_ORIGINATING_PROGRAM: u8 = 65;

/// Dataset number for ProgramVersion (IPTC Record 2, Tag 70)
const DATASET_PROGRAM_VERSION: u8 = 70;

/// Dataset number for ObjectCycle (IPTC Record 2, Tag 75)
const DATASET_OBJECT_CYCLE: u8 = 75;

/// Dataset number for ByLine (IPTC Record 2, Tag 80) - repeatable
const DATASET_BY_LINE: u8 = 80;

/// Dataset number for ByLineTitle (IPTC Record 2, Tag 85) - repeatable
const DATASET_BY_LINE_TITLE: u8 = 85;

/// Dataset number for City (IPTC Record 2, Tag 90)
const DATASET_CITY: u8 = 90;

/// Dataset number for SubLocation (IPTC Record 2, Tag 92)
const DATASET_SUB_LOCATION: u8 = 92;

/// Dataset number for Province-State (IPTC Record 2, Tag 95)
const DATASET_PROVINCE_STATE: u8 = 95;

/// Dataset number for Country-PrimaryLocationCode (IPTC Record 2, Tag 100)
const DATASET_COUNTRY_PRIMARY_LOCATION_CODE: u8 = 100;

/// Dataset number for Country-PrimaryLocationName (IPTC Record 2, Tag 101)
const DATASET_COUNTRY_PRIMARY_LOCATION_NAME: u8 = 101;

/// Dataset number for OriginalTransmissionReference (IPTC Record 2, Tag 103)
const DATASET_ORIGINAL_TRANSMISSION_REFERENCE: u8 = 103;

/// Dataset number for Headline (IPTC Record 2, Tag 105)
const DATASET_HEADLINE: u8 = 105;

/// Dataset number for Credit (IPTC Record 2, Tag 110)
const DATASET_CREDIT: u8 = 110;

/// Dataset number for Source (IPTC Record 2, Tag 115) - repeatable
const DATASET_SOURCE: u8 = 115;

/// Dataset number for CopyrightNotice (IPTC Record 2, Tag 116)
const DATASET_COPYRIGHT_NOTICE: u8 = 116;

/// Dataset number for Contact (IPTC Record 2, Tag 118) - repeatable
const DATASET_CONTACT: u8 = 118;

/// Dataset number for Caption-Abstract (IPTC Record 2, Tag 120)
const DATASET_CAPTION_ABSTRACT: u8 = 120;

/// Dataset number for Writer-Editor (IPTC Record 2, Tag 122) - repeatable
const DATASET_WRITER_EDITOR: u8 = 122;

/// Dataset number for ImageType (IPTC Record 2, Tag 130)
const DATASET_IMAGE_TYPE: u8 = 130;

/// Dataset number for ImageOrientation (IPTC Record 2, Tag 131)
const DATASET_IMAGE_ORIENTATION: u8 = 131;

/// Dataset number for LanguageIdentifier (IPTC Record 2, Tag 135) - repeatable
const DATASET_LANGUAGE_IDENTIFIER: u8 = 135;

/// Dataset number for AudioType (IPTC Record 2, Tag 150)
const DATASET_AUDIO_TYPE: u8 = 150;

/// Dataset number for AudioDuration (IPTC Record 2, Tag 151)
const DATASET_AUDIO_DURATION: u8 = 151;

/// Dataset number for AudioOutcue (IPTC Record 2, Tag 152)
const DATASET_AUDIO_OUTCUE: u8 = 152;

/// Dataset number for VideoType (IPTC Record 2, Tag 160)
const DATASET_VIDEO_TYPE: u8 = 160;

// =============================================================================
// PUBLIC API
// =============================================================================

/// Parses IPTC Record 2 (Application Record) datasets from a data block.
///
/// This function extracts editorial metadata from IPTC Record 2 datasets.
/// Unlike Record 1, Record 2 datasets are repeatable (can appear multiple times).
/// All occurrences are collected into the metadata map.
///
/// # Arguments
///
/// * `data` - Raw IPTC data block containing Record 2 datasets
///
/// # Returns
///
/// A MetadataMap containing all parsed Record 2 tags with their values.
pub fn parse_iptc_record2(data: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let mut offset = 0;

    // Iterate through all IPTC datasets in the data block
    while offset + MIN_DATASET_SIZE <= data.len() {
        // Verify tag marker byte
        if data[offset] != IPTC_TAG_MARKER {
            // No more valid IPTC data; stop parsing
            break;
        }

        let record_number = data[offset + 1];
        let dataset_number = data[offset + 2];

        // Parse the data length (big-endian 16-bit value)
        let length_high = data[offset + 3] as usize;
        let length_low = data[offset + 4] as usize;
        let data_length = (length_high << 8) | length_low;

        // Check for extended length format (if bit 15 is set)
        if length_high & 0x80 != 0 {
            // Extended format: skip this dataset
            offset += MIN_DATASET_SIZE;
            continue;
        }

        // Verify we have enough data for the payload
        let payload_start = offset + MIN_DATASET_SIZE;
        let payload_end = payload_start + data_length;

        if payload_end > data.len() {
            // Truncated data; stop parsing
            break;
        }

        let payload = &data[payload_start..payload_end];

        // Only process Record 2 (Application) datasets
        if record_number == APPLICATION_RECORD_NUMBER {
            process_record2_dataset(dataset_number, payload, &mut metadata);
        }

        // Move to the next dataset
        offset = payload_end;
    }

    metadata
}

// =============================================================================
// INTERNAL HELPERS
// =============================================================================

/// Processes a single Record 2 dataset and adds it to the metadata map.
///
/// This function handles the type-specific parsing for each Record 2 tag,
/// converting raw bytes to appropriate string or integer values.
fn process_record2_dataset(dataset_number: u8, payload: &[u8], metadata: &mut MetadataMap) {
    match dataset_number {
        DATASET_RECORD_VERSION => {
            // RecordVersion is a 2-byte binary integer
            if let Some(version) = parse_binary_u16(payload) {
                metadata.insert(
                    "IPTC:ApplicationRecordVersion",
                    TagValue::new_integer(version as i64),
                );
            }
        }

        DATASET_OBJECT_NAME => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:ObjectName", TagValue::new_string(value));
            }
        }

        DATASET_EDIT_STATUS => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:EditStatus", TagValue::new_string(value));
            }
        }

        DATASET_EDITORIAL_UPDATE => {
            if !payload.is_empty() {
                let value = payload[0] as i64;
                metadata.insert("IPTC:EditorialUpdate", TagValue::new_integer(value));
            }
        }

        DATASET_URGENCY => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                let formatted = format_iptc_urgency(&value);
                metadata.insert("IPTC:Urgency", TagValue::new_string(formatted));
            }
        }

        DATASET_SUBJECT => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Subject", TagValue::new_string(value));
            }
        }

        DATASET_CATEGORY => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Category", TagValue::new_string(value));
            }
        }

        DATASET_SUPPLEMENTAL_CATEGORIES => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:SupplementalCategories", TagValue::new_string(value));
            }
        }

        DATASET_FIXTURE_IDENTIFIER => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:FixtureIdentifier", TagValue::new_string(value));
            }
        }

        DATASET_KEYWORDS => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Keywords", TagValue::new_string(value));
            }
        }

        DATASET_LOCATION_CODE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:LocationCode", TagValue::new_string(value));
            }
        }

        DATASET_LOCATION_NAME => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:LocationName", TagValue::new_string(value));
            }
        }

        DATASET_RELEASE_DATE => {
            let raw_date = decode_iptc_string(payload);
            if !raw_date.is_empty() {
                let formatted = format_iptc_date(&raw_date);
                metadata.insert("IPTC:ReleaseDate", TagValue::new_string(formatted));
            }
        }

        DATASET_RELEASE_TIME => {
            let raw_time = decode_iptc_string(payload);
            if !raw_time.is_empty() {
                let formatted = format_iptc_time(&raw_time);
                metadata.insert("IPTC:ReleaseTime", TagValue::new_string(formatted));
            }
        }

        DATASET_EXPIRATION_DATE => {
            let raw_date = decode_iptc_string(payload);
            if !raw_date.is_empty() {
                let formatted = format_iptc_date(&raw_date);
                metadata.insert("IPTC:ExpirationDate", TagValue::new_string(formatted));
            }
        }

        DATASET_EXPIRATION_TIME => {
            let raw_time = decode_iptc_string(payload);
            if !raw_time.is_empty() {
                let formatted = format_iptc_time(&raw_time);
                metadata.insert("IPTC:ExpirationTime", TagValue::new_string(formatted));
            }
        }

        DATASET_SPECIAL_INSTRUCTIONS => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:SpecialInstructions", TagValue::new_string(value));
            }
        }

        DATASET_ACTION_ADVISED => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:ActionAdvised", TagValue::new_string(value));
            }
        }

        DATASET_REFERENCE_SERVICE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:ReferenceService", TagValue::new_string(value));
            }
        }

        DATASET_REFERENCE_DATE => {
            let raw_date = decode_iptc_string(payload);
            if !raw_date.is_empty() {
                let formatted = format_iptc_date(&raw_date);
                metadata.insert("IPTC:ReferenceDate", TagValue::new_string(formatted));
            }
        }

        DATASET_REFERENCE_NUMBER => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:ReferenceNumber", TagValue::new_string(value));
            }
        }

        DATASET_DATE_CREATED => {
            let raw_date = decode_iptc_string(payload);
            if !raw_date.is_empty() {
                let formatted = format_iptc_date(&raw_date);
                metadata.insert("IPTC:DateCreated", TagValue::new_string(formatted));
            }
        }

        DATASET_TIME_CREATED => {
            let raw_time = decode_iptc_string(payload);
            if !raw_time.is_empty() {
                let formatted = format_iptc_time(&raw_time);
                metadata.insert("IPTC:TimeCreated", TagValue::new_string(formatted));
            }
        }

        DATASET_DIGITAL_CREATION_DATE => {
            let raw_date = decode_iptc_string(payload);
            if !raw_date.is_empty() {
                let formatted = format_iptc_date(&raw_date);
                metadata.insert("IPTC:DigitalCreationDate", TagValue::new_string(formatted));
            }
        }

        DATASET_DIGITAL_CREATION_TIME => {
            let raw_time = decode_iptc_string(payload);
            if !raw_time.is_empty() {
                let formatted = format_iptc_time(&raw_time);
                metadata.insert("IPTC:DigitalCreationTime", TagValue::new_string(formatted));
            }
        }

        DATASET_ORIGINATING_PROGRAM => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:OriginatingProgram", TagValue::new_string(value));
            }
        }

        DATASET_PROGRAM_VERSION => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:ProgramVersion", TagValue::new_string(value));
            }
        }

        DATASET_OBJECT_CYCLE => {
            if !payload.is_empty() {
                let cycle_char = payload[0] as char;
                metadata.insert(
                    "IPTC:ObjectCycle",
                    TagValue::new_string(cycle_char.to_string()),
                );
            }
        }

        DATASET_BY_LINE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:By-line", TagValue::new_string(value));
            }
        }

        DATASET_BY_LINE_TITLE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:By-lineTitle", TagValue::new_string(value));
            }
        }

        DATASET_CITY => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:City", TagValue::new_string(value));
            }
        }

        DATASET_SUB_LOCATION => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:SubLocation", TagValue::new_string(value));
            }
        }

        DATASET_PROVINCE_STATE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Province-State", TagValue::new_string(value));
            }
        }

        DATASET_COUNTRY_PRIMARY_LOCATION_CODE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert(
                    "IPTC:Country-PrimaryLocationCode",
                    TagValue::new_string(value),
                );
            }
        }

        DATASET_COUNTRY_PRIMARY_LOCATION_NAME => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert(
                    "IPTC:Country-PrimaryLocationName",
                    TagValue::new_string(value),
                );
            }
        }

        DATASET_ORIGINAL_TRANSMISSION_REFERENCE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert(
                    "IPTC:OriginalTransmissionReference",
                    TagValue::new_string(value),
                );
            }
        }

        DATASET_HEADLINE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Headline", TagValue::new_string(value));
            }
        }

        DATASET_CREDIT => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Credit", TagValue::new_string(value));
            }
        }

        DATASET_SOURCE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Source", TagValue::new_string(value));
            }
        }

        DATASET_COPYRIGHT_NOTICE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:CopyrightNotice", TagValue::new_string(value));
            }
        }

        DATASET_CONTACT => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Contact", TagValue::new_string(value));
            }
        }

        DATASET_CAPTION_ABSTRACT => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Caption-Abstract", TagValue::new_string(value));
            }
        }

        DATASET_WRITER_EDITOR => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:Writer-Editor", TagValue::new_string(value));
            }
        }

        DATASET_IMAGE_TYPE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:ImageType", TagValue::new_string(value));
            }
        }

        DATASET_IMAGE_ORIENTATION => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:ImageOrientation", TagValue::new_string(value));
            }
        }

        DATASET_LANGUAGE_IDENTIFIER => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:LanguageIdentifier", TagValue::new_string(value));
            }
        }

        DATASET_AUDIO_TYPE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:AudioType", TagValue::new_string(value));
            }
        }

        DATASET_AUDIO_DURATION => {
            if let Some(duration) = parse_binary_u16(payload) {
                metadata.insert("IPTC:AudioDuration", TagValue::new_integer(duration as i64));
            }
        }

        DATASET_AUDIO_OUTCUE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:AudioOutcue", TagValue::new_string(value));
            }
        }

        DATASET_VIDEO_TYPE => {
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                metadata.insert("IPTC:VideoType", TagValue::new_string(value));
            }
        }

        _ => {
            // Unknown Record 2 dataset; store as generic tag with raw string value
            let value = decode_iptc_string(payload);
            if !value.is_empty() {
                let tag_name = format!("IPTC:Application-{}", dataset_number);
                metadata.insert(tag_name, TagValue::new_string(value));
            }
        }
    }
}

/// Parses a big-endian 16-bit unsigned integer from a byte slice.
fn parse_binary_u16(data: &[u8]) -> Option<u16> {
    if data.len() < 2 {
        return None;
    }
    Some(((data[0] as u16) << 8) | (data[1] as u16))
}

/// Decodes an IPTC string from bytes, handling UTF-8 and fallback to Latin-1.
fn decode_iptc_string(payload: &[u8]) -> String {
    // Try UTF-8 first
    if let Ok(s) = std::str::from_utf8(payload) {
        return s.trim().to_string();
    }

    // Fall back to Latin-1 (ISO-8859-1)
    payload
        .iter()
        .map(|&b| {
            if b < 128 {
                b as char
            } else {
                // For bytes >= 128, decode as Latin-1
                char::from_u32(b as u32).unwrap_or('?')
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_iptc_dataset(record: u8, dataset: u8, data: &[u8]) -> Vec<u8> {
        let mut result = vec![IPTC_TAG_MARKER, record, dataset];
        result.push((data.len() >> 8) as u8);
        result.push((data.len() & 0xFF) as u8);
        result.extend_from_slice(data);
        result
    }

    #[test]
    fn test_parse_object_name() {
        let data = make_iptc_dataset(
            APPLICATION_RECORD_NUMBER,
            DATASET_OBJECT_NAME,
            b"Test Caption",
        );
        let metadata = parse_iptc_record2(&data);
        assert_eq!(metadata.get_string("IPTC:ObjectName"), Some("Test Caption"));
    }

    #[test]
    fn test_parse_urgency() {
        let data = make_iptc_dataset(APPLICATION_RECORD_NUMBER, DATASET_URGENCY, &[5]);
        let metadata = parse_iptc_record2(&data);
        assert!(metadata.get_string("IPTC:Urgency").is_some());
    }

    #[test]
    fn test_parse_date_created() {
        let data = make_iptc_dataset(APPLICATION_RECORD_NUMBER, DATASET_DATE_CREATED, b"20041225");
        let metadata = parse_iptc_record2(&data);
        assert!(metadata.get_string("IPTC:DateCreated").is_some());
    }

    #[test]
    fn test_parse_by_line() {
        let data = make_iptc_dataset(APPLICATION_RECORD_NUMBER, DATASET_BY_LINE, b"John Smith");
        let metadata = parse_iptc_record2(&data);
        assert_eq!(metadata.get_string("IPTC:By-line"), Some("John Smith"));
    }

    #[test]
    fn test_parse_city() {
        let data = make_iptc_dataset(APPLICATION_RECORD_NUMBER, DATASET_CITY, b"New York");
        let metadata = parse_iptc_record2(&data);
        assert_eq!(metadata.get_string("IPTC:City"), Some("New York"));
    }

    #[test]
    fn test_parse_headline() {
        let data = make_iptc_dataset(
            APPLICATION_RECORD_NUMBER,
            DATASET_HEADLINE,
            b"Breaking News",
        );
        let metadata = parse_iptc_record2(&data);
        assert_eq!(metadata.get_string("IPTC:Headline"), Some("Breaking News"));
    }

    #[test]
    fn test_parse_caption() {
        let data = make_iptc_dataset(
            APPLICATION_RECORD_NUMBER,
            DATASET_CAPTION_ABSTRACT,
            b"A detailed caption",
        );
        let metadata = parse_iptc_record2(&data);
        assert_eq!(
            metadata.get_string("IPTC:Caption-Abstract"),
            Some("A detailed caption")
        );
    }

    #[test]
    fn test_parse_copyright() {
        let data = make_iptc_dataset(
            APPLICATION_RECORD_NUMBER,
            DATASET_COPYRIGHT_NOTICE,
            b"Copyright 2024",
        );
        let metadata = parse_iptc_record2(&data);
        assert_eq!(
            metadata.get_string("IPTC:CopyrightNotice"),
            Some("Copyright 2024")
        );
    }

    #[test]
    fn test_record_version() {
        let data = make_iptc_dataset(
            APPLICATION_RECORD_NUMBER,
            DATASET_RECORD_VERSION,
            &[0x00, 0x02],
        );
        let metadata = parse_iptc_record2(&data);
        assert_eq!(
            metadata.get_integer("IPTC:ApplicationRecordVersion"),
            Some(2)
        );
    }

    #[test]
    fn test_empty_payload() {
        let data = make_iptc_dataset(APPLICATION_RECORD_NUMBER, DATASET_OBJECT_NAME, b"");
        let metadata = parse_iptc_record2(&data);
        assert!(metadata.get("IPTC:ObjectName").is_none());
    }

    #[test]
    fn test_record1_datasets_ignored() {
        // Record 1 datasets should be ignored by parse_iptc_record2
        let mut data = Vec::new();
        data.extend(make_iptc_dataset(1, 0, &[0x00, 0x04])); // Record 1
        data.extend(make_iptc_dataset(2, 5, b"Test")); // Record 2

        let metadata = parse_iptc_record2(&data);
        assert!(metadata.get("IPTC:ModelVersion").is_none());
        assert_eq!(metadata.get_string("IPTC:ObjectName"), Some("Test"));
    }
}
