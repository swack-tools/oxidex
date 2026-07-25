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
//! # Supported Metadata
//!
//! - FileType: Always "vCard"
//! - FileSize: Size of the file in bytes
//! - VCardVersion: Version of the vCard format (e.g., "2.1", "3.0", "4.0")
//! - FullName: Full name from FN field
//! - Email: Email address from EMAIL field
//! - Telephone: Phone number from TEL field

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// VCF signature: "BEGIN:VCARD"
const VCF_SIGNATURE: &[u8] = b"BEGIN:VCARD";

/// VCF/vCard parser for extracting metadata from contact files
pub struct VCFParser;

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

    /// Parse vCard content to extract basic metadata
    ///
    /// Reads up to 8KB of the file and parses line-by-line to extract
    /// common vCard fields like VERSION, FN (full name), EMAIL, and TEL (telephone).
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
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();

                // Extract standard vCard fields
                match key {
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
                    "FN" => {
                        metadata
                            .insert("FullName".to_string(), TagValue::String(value.to_string()));
                    }
                    "EMAIL" => {
                        metadata.insert("Email".to_string(), TagValue::String(value.to_string()));
                        has_email = true;
                    }
                    "TEL" => {
                        metadata
                            .insert("Telephone".to_string(), TagValue::String(value.to_string()));
                        has_phone = true;
                    }
                    // Worker 28 additional fields
                    "PHOTO" => {
                        has_photo = true;
                    }
                    "ORG" => {
                        has_organization = true;
                    }
                    "ADR" => {
                        has_address = true;
                    }
                    "URL" => {
                        has_url = true;
                    }
                    _ => {}
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
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
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
