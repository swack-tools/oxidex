//! FotoStation/FotoWare MakerNote parser
//!
//! Parses FotoStation/FotoWare asset management metadata.
//! FotoStation is a professional digital asset management (DAM)
//! system used by agencies, corporations, and media organizations.
//!
//! ## Supported Versions
//! - FotoStation 9.x (current)
//! - FotoStation 8.x
//! - FotoWare Cloud
//! - Index Manager integration
//!
//! ## Key Features
//! - Asset workflow status
//! - Archive categories and collections
//! - Approval status and routing
//! - Publication state
//! - Archive location metadata
//! - Asset expiration dates
//! - Rights management status
//! - Taxonomies and controlled vocabularies
//! - Custom field metadata
//! - Version tracking
//! - Check-in/check-out status
//! - Batch processing metadata
//!
//! ## Architecture
//! FotoStation stores DAM workflow metadata in proprietary
//! formats for enterprise asset management and distribution.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::array_extractors::extract_i16_array;
use super::shared::MakerNoteParser;

// FotoStation MakerNote Tag IDs
const FS_VERSION: u16 = 0x0001; // FotoStation version
const FS_WORKFLOW_STATUS: u16 = 0x0010; // Workflow status
const FS_APPROVAL_STATUS: u16 = 0x0011; // Approval status
const FS_PUBLICATION_STATUS: u16 = 0x0012; // Publication status
const FS_ARCHIVE_LOCATION: u16 = 0x0020; // Archive location path
const FS_CATEGORY: u16 = 0x0021; // Primary category
const FS_SUBCATEGORY: u16 = 0x0022; // Subcategory
const FS_COLLECTION_NAME: u16 = 0x0023; // Collection name
const FS_ARCHIVE_ID: u16 = 0x0024; // Archive identifier
const FS_RIGHTS_STATUS: u16 = 0x0030; // Rights management status
const FS_USAGE_RIGHTS: u16 = 0x0031; // Usage rights level
const FS_EXPIRATION_DATE: u16 = 0x0032; // Asset expiration date
const FS_RELEASE_STATUS: u16 = 0x0033; // Model/property release status
const FS_TAXONOMY_1: u16 = 0x0040; // Taxonomy level 1
const FS_TAXONOMY_2: u16 = 0x0041; // Taxonomy level 2
const FS_TAXONOMY_3: u16 = 0x0042; // Taxonomy level 3
const FS_CONTROLLED_VOCAB: u16 = 0x0043; // Controlled vocabulary terms
const FS_CUSTOM_FIELD_1: u16 = 0x0050; // Custom field 1
const FS_CUSTOM_FIELD_2: u16 = 0x0051; // Custom field 2
const FS_CUSTOM_FIELD_3: u16 = 0x0052; // Custom field 3
const FS_VERSION_NUMBER: u16 = 0x0060; // Asset version number
const FS_VERSION_COMMENT: u16 = 0x0061; // Version comment
const FS_CHECKED_OUT_BY: u16 = 0x0062; // Checked out by user
const FS_CHECKED_OUT_DATE: u16 = 0x0063; // Check-out timestamp
const FS_BATCH_ID: u16 = 0x0070; // Batch processing ID
const FS_OPERATOR: u16 = 0x0071; // Operator/user name
const FS_STATION_NAME: u16 = 0x0072; // FotoStation name

// FotoStation signature
const FOTOSTATION_SIGNATURE: &[u8] = b"FotoWare";

/// Decodes workflow status
///
/// # Arguments
/// * `value` - Workflow status code
///
/// # Returns
/// Human-readable workflow status
fn decode_workflow_status(value: i16) -> String {
    match value {
        0 => "New".to_string(),
        1 => "In Progress".to_string(),
        2 => "Pending Review".to_string(),
        3 => "Approved".to_string(),
        4 => "Rejected".to_string(),
        5 => "Published".to_string(),
        6 => "Archived".to_string(),
        7 => "Expired".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes approval status
///
/// # Arguments
/// * `value` - Approval status code
///
/// # Returns
/// Human-readable approval status
fn decode_approval_status(value: i16) -> String {
    match value {
        0 => "Pending".to_string(),
        1 => "Approved".to_string(),
        2 => "Rejected".to_string(),
        3 => "Needs Revision".to_string(),
        4 => "Final".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes publication status
///
/// # Arguments
/// * `value` - Publication status code
///
/// # Returns
/// Human-readable publication status
fn decode_publication_status(value: i16) -> String {
    match value {
        0 => "Unpublished".to_string(),
        1 => "Published".to_string(),
        2 => "Scheduled".to_string(),
        3 => "Retracted".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes rights status
///
/// # Arguments
/// * `value` - Rights status code
///
/// # Returns
/// Human-readable rights status
fn decode_rights_status(value: i16) -> String {
    match value {
        0 => "Unknown".to_string(),
        1 => "Rights Managed".to_string(),
        2 => "Royalty Free".to_string(),
        3 => "Rights Reserved".to_string(),
        4 => "Public Domain".to_string(),
        5 => "Creative Commons".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes usage rights level
///
/// # Arguments
/// * `value` - Usage rights code
///
/// # Returns
/// Human-readable usage level
fn decode_usage_rights(value: i16) -> String {
    match value {
        0 => "No Restrictions".to_string(),
        1 => "Internal Use Only".to_string(),
        2 => "Editorial Use".to_string(),
        3 => "Commercial Use".to_string(),
        4 => "Limited Use".to_string(),
        5 => "Restricted".to_string(),
        _ => format!("Unknown ({})", value),
    }
}

/// Decodes release status
///
/// # Arguments
/// * `value` - Release status code
///
/// # Returns
/// Human-readable release status
fn decode_release_status(value: i16) -> String {
    match value {
        0 => "Not Required".to_string(),
        1 => "Not Available".to_string(),
        2 => "On File".to_string(),
        3 => "Pending".to_string(),
        _ => format!("Unknown ({})", value),
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

/// FotoStation MakerNote parser implementing the MakerNoteParser trait
#[derive(Default)]
pub struct FotoStationParser;

impl FotoStationParser {
    /// Creates a new FotoStation parser instance
    pub fn new() -> Self {
        FotoStationParser
    }
}

impl MakerNoteParser for FotoStationParser {
    fn manufacturer_name(&self) -> &'static str {
        "FotoStation"
    }

    fn tag_prefix(&self) -> &'static str {
        "FotoStation:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        if data.len() < 8 {
            return false;
        }
        data.starts_with(FOTOSTATION_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        if data.len() < 8 {
            return Err("FotoStation MakerNote data too short".to_string());
        }

        // Skip FotoStation signature if present
        let start_offset = if data.starts_with(FOTOSTATION_SIGNATURE) {
            8
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

        if num_entries == 0 || num_entries > 150 {
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
                FS_VERSION | FS_ARCHIVE_LOCATION | FS_CATEGORY | FS_SUBCATEGORY
                | FS_COLLECTION_NAME | FS_ARCHIVE_ID | FS_EXPIRATION_DATE | FS_TAXONOMY_1
                | FS_TAXONOMY_2 | FS_TAXONOMY_3 | FS_CONTROLLED_VOCAB | FS_CUSTOM_FIELD_1
                | FS_CUSTOM_FIELD_2 | FS_CUSTOM_FIELD_3 | FS_VERSION_COMMENT
                | FS_CHECKED_OUT_BY | FS_BATCH_ID | FS_OPERATOR | FS_STATION_NAME => {
                    if let Some(s) = extract_string(&entry, parse_data) {
                        let tag_name = match tag {
                            FS_VERSION => "Version",
                            FS_ARCHIVE_LOCATION => "ArchiveLocation",
                            FS_CATEGORY => "Category",
                            FS_SUBCATEGORY => "Subcategory",
                            FS_COLLECTION_NAME => "CollectionName",
                            FS_ARCHIVE_ID => "ArchiveID",
                            FS_EXPIRATION_DATE => "ExpirationDate",
                            FS_TAXONOMY_1 => "TaxonomyLevel1",
                            FS_TAXONOMY_2 => "TaxonomyLevel2",
                            FS_TAXONOMY_3 => "TaxonomyLevel3",
                            FS_CONTROLLED_VOCAB => "ControlledVocabulary",
                            FS_CUSTOM_FIELD_1 => "CustomField1",
                            FS_CUSTOM_FIELD_2 => "CustomField2",
                            FS_CUSTOM_FIELD_3 => "CustomField3",
                            FS_VERSION_COMMENT => "VersionComment",
                            FS_CHECKED_OUT_BY => "CheckedOutBy",
                            FS_BATCH_ID => "BatchID",
                            FS_OPERATOR => "Operator",
                            FS_STATION_NAME => "StationName",
                            _ => continue,
                        };
                        tags.insert(format!("FotoStation:{}", tag_name), s);
                    }
                }

                _ => {
                    // Try to extract as i16 array
                    if let Some(array) = extract_i16_array(&entry, parse_data, byte_order) {
                        if let Some(&val) = array.first() {
                            let (tag_name, formatted_value) = match tag {
                                FS_WORKFLOW_STATUS => {
                                    ("WorkflowStatus", decode_workflow_status(val))
                                }
                                FS_APPROVAL_STATUS => {
                                    ("ApprovalStatus", decode_approval_status(val))
                                }
                                FS_PUBLICATION_STATUS => {
                                    ("PublicationStatus", decode_publication_status(val))
                                }
                                FS_RIGHTS_STATUS => ("RightsStatus", decode_rights_status(val)),
                                FS_USAGE_RIGHTS => ("UsageRights", decode_usage_rights(val)),
                                FS_RELEASE_STATUS => ("ReleaseStatus", decode_release_status(val)),
                                FS_VERSION_NUMBER => ("VersionNumber", val.to_string()),
                                _ => continue,
                            };
                            tags.insert(format!("FotoStation:{}", tag_name), formatted_value);
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
    fn test_fotostation_parser_creation() {
        let parser = FotoStationParser::new();
        assert_eq!(parser.manufacturer_name(), "FotoStation");
        assert_eq!(parser.tag_prefix(), "FotoStation:");
    }

    #[test]
    fn test_decode_workflow_status() {
        assert_eq!(decode_workflow_status(0), "New");
        assert_eq!(decode_workflow_status(3), "Approved");
        assert_eq!(decode_workflow_status(5), "Published");
    }

    #[test]
    fn test_decode_approval_status() {
        assert_eq!(decode_approval_status(0), "Pending");
        assert_eq!(decode_approval_status(1), "Approved");
        assert_eq!(decode_approval_status(4), "Final");
    }

    #[test]
    fn test_decode_publication_status() {
        assert_eq!(decode_publication_status(0), "Unpublished");
        assert_eq!(decode_publication_status(1), "Published");
    }

    #[test]
    fn test_decode_rights_status() {
        assert_eq!(decode_rights_status(1), "Rights Managed");
        assert_eq!(decode_rights_status(2), "Royalty Free");
        assert_eq!(decode_rights_status(4), "Public Domain");
    }

    #[test]
    fn test_decode_usage_rights() {
        assert_eq!(decode_usage_rights(2), "Editorial Use");
        assert_eq!(decode_usage_rights(3), "Commercial Use");
    }

    #[test]
    fn test_decode_release_status() {
        assert_eq!(decode_release_status(2), "On File");
        assert_eq!(decode_release_status(3), "Pending");
    }
}
