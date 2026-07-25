//! ICC Profile parser for embedded color profiles
//!
//! This module handles parsing of ICC (International Color Consortium)
//! profiles embedded in various file formats (PDF, JPEG, PNG, TIFF, etc.).
//! ICC profiles describe color characteristics for accurate color reproduction
//! across different devices.
//!
//! # Architecture
//!
//! The parser uses a **registry-based approach** for maintainability and extensibility:
//! - **TagRegistry**: Table-driven tag definitions with signatures, names, and decoders
//! - **HeaderField**: Registry for header field locations and extractors
//! - **LookupTables**: Static lookup tables for enumerations (profile class, platform, etc.)
//!
//! # Module Structure
//!
//! - [`binary`]: Low-level binary data readers
//! - [`header`]: ICC profile header parsing (128 bytes)
//! - [`pdf`]: PDF ICC profile extraction and decompression
//! - [`registries`]: Static registries and lookup tables
//! - [`tags`]: ICC tag decoding (text, XYZ, curves, etc.)
//!
//! # ICC Profile Structure
//!
//! An ICC profile consists of:
//! 1. **Profile Header** (128 bytes): Contains profile metadata
//! 2. **Tag Table**: List of tags with their signatures, offsets, and sizes
//! 3. **Tagged Element Data**: Actual tag data (descriptions, calibration data, etc.)

mod binary;
mod header;
mod pdf;
mod registries;
mod tags;

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use std::collections::HashMap;

// Re-export main types for external use
pub use registries::{TagDef, TagType};

// ============================================================================
// PUBLIC API
// ============================================================================

/// Extracts ICC profile metadata from a PDF file.
///
/// This function searches for ICC profiles in the PDF's OutputIntents,
/// extracts the profile stream, decompresses if necessary, and parses
/// the ICC profile header and tags.
pub fn extract_icc_profile(reader: &dyn FileReader) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Extract ICC profile from PDF
    let icc_data = pdf::extract_icc_from_pdf(reader)?;

    // Parse the ICC profile
    let icc_metadata = parse_icc_profile(&icc_data)?;

    // Add Profile: prefix to all tags
    for (key, value) in icc_metadata {
        metadata.insert(format!("Profile:{}", key), value);
    }

    if metadata.is_empty() {
        return Err(ExifToolError::parse_error("No ICC profile found in PDF"));
    }

    Ok(metadata)
}

/// Parses ICC profile binary data and extracts metadata.
///
/// This is the main entry point for parsing ICC profile data from any source
/// (JPEG APP2 segments, PDF streams, PNG chunks, etc.).
pub fn parse_icc_profile_data(data: &[u8]) -> Result<HashMap<String, TagValue>> {
    parse_icc_profile(data)
}

/// Parses a standalone ICC profile file.
///
/// This function reads an ICC profile file directly (not embedded in PDF/JPEG/etc.)
/// and extracts all metadata with ICC_Profile: prefix.
pub fn parse_icc_file(reader: &dyn FileReader) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Read the entire ICC profile file
    let size = reader.size() as usize;
    let icc_data = reader.read(0, size)?;

    // Parse the ICC profile
    let icc_metadata = parse_icc_profile(icc_data)?;

    // Add ICC_Profile: prefix to all tags (to match ExifTool's family naming)
    for (key, value) in icc_metadata {
        metadata.insert(format!("ICC_Profile:{}", key), value);
    }

    if metadata.is_empty() {
        return Err(ExifToolError::parse_error(
            "No valid ICC profile data found",
        ));
    }

    Ok(metadata)
}

// ============================================================================
// CORE PARSING LOGIC
// ============================================================================

/// Main ICC profile parser - uses registry-based approach
fn parse_icc_profile(data: &[u8]) -> Result<HashMap<String, TagValue>> {
    if data.len() < 128 {
        return Err(ExifToolError::parse_error(
            "ICC profile too small (< 128 bytes)",
        ));
    }

    let mut metadata = HashMap::new();

    // Parse header using registry
    header::parse_header_registry(data, &mut metadata)?;

    // Parse tags using registry
    if data.len() > 128 {
        tags::parse_tags_registry(data, &mut metadata)?;
    }

    Ok(metadata)
}
