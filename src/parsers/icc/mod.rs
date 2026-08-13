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
//!
//! # Family-1 groups (Step 22)
//!
//! Every ICC tag lands under family-0 `ICC_Profile:`, but ExifTool splits
//! family 1 by which of the profile's internal tables actually decoded it
//! (`lib/Image/ExifTool/ICC_Profile.pm`, pinned 13.59):
//!
//! ```text
//! ICC_Profile.pm:654   %Header       GROUPS => { 1 => 'ICC-header' }
//! ICC_Profile.pm:762   %ColorRep     GROUPS => { 1 => 'ICC-cicp'   }  (the `cicp` tag)
//! ICC_Profile.pm:833   %ViewingConditions  GROUPS => { 1 => 'ICC-view' }  (`view`)
//! ICC_Profile.pm:852   %Measurement  GROUPS => { 1 => 'ICC-meas'   }  (`meas`)
//! ICC_Profile.pm:345   %Main         no `1 =>` override -- family 1 == family 0
//! ```
//!
//! [`insert_icc_tags`] is where that split happens now, at extraction time,
//! from each decoded tag's own table provenance ([`header::parse_header_registry`]
//! is unconditionally `ICC-header`; [`tags::icc_output_group1`] covers the
//! three tag-table sub-structures oxidex decodes). Before this step every
//! ICC-bearing format inserted a flat `Profile:`/`ICC_Profile:`-prefixed key
//! with no family-1 information at all, and only JPEG's own
//! `normalize_metadata_map` post-pass even attempted the `Profile:` ->
//! `ICC_Profile:` rename -- every other format (PNG's `iCCP` chunk, GIF,
//! FLIF, PSD, XCF, standalone `.icc`, embedded TIFF/RAW) left its ICC tags
//! under the internal `Profile:` prefix forever, since none of them ever
//! called that JPEG-only function.

mod binary;
mod header;
mod pdf;
mod registries;
mod tags;

use crate::core::tag_occurrence::{Instance, SHIM_DEFAULT_PRIORITY};
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use std::collections::HashMap;

// Re-export main types for external use
pub use registries::{TagDef, TagType};

// ============================================================================
// PUBLIC API
// ============================================================================

/// One decoded ICC tag, ready for insertion into a [`MetadataMap`]: its bare
/// name, value, and the ExifTool family-1 group that owns it -- see the
/// module doc comment's table. `""` means "no family-1 override", which
/// [`insert_icc_tags`] passes straight through to
/// [`MetadataMap::insert_occurrence`] as `group1` (empty there means exactly
/// the same thing: fall back to family 0).
pub struct IccTag {
    pub name: String,
    pub value: TagValue,
    pub group1: &'static str,
}

/// Extracts ICC profile metadata from a PDF file.
///
/// This function searches for ICC profiles in the PDF's OutputIntents,
/// extracts the profile stream, decompresses if necessary, and parses
/// the ICC profile header and tags.
pub fn extract_icc_profile(reader: &dyn FileReader) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // Extract ICC profile from PDF
    let icc_data = pdf::extract_icc_from_pdf(reader)?;

    // Parse the ICC profile and insert it, grouped by table provenance.
    let icc_tags = parse_icc_profile(&icc_data)?;
    insert_icc_tags(&mut metadata, icc_tags);

    if metadata.is_empty() {
        return Err(ExifToolError::parse_error("No ICC profile found in PDF"));
    }

    Ok(metadata)
}

/// Parses ICC profile binary data and extracts metadata.
///
/// This is the main entry point for parsing ICC profile data from any source
/// (JPEG APP2 segments, PDF streams, PNG chunks, etc.). Returns each tag's
/// bare name, value and family-1 group; callers insert via
/// [`insert_icc_tags`] under whichever `ICC_Profile:` (or, rarely, a
/// caller-specific) key convention they use.
pub fn parse_icc_profile_data(data: &[u8]) -> Result<Vec<IccTag>> {
    parse_icc_profile(data)
}

/// Inserts every decoded ICC tag into `metadata` under `ICC_Profile:{name}`,
/// carrying each occurrence's family-1 group from table provenance --
/// [`IccTag::group1`], set at decode time. This is the shared insertion path
/// every ICC-bearing format now goes through (see the module doc comment).
pub fn insert_icc_tags(metadata: &mut MetadataMap, tags: Vec<IccTag>) {
    for tag in tags {
        metadata.insert_occurrence(
            format!("ICC_Profile:{}", tag.name),
            tag.value,
            SHIM_DEFAULT_PRIORITY,
            tag.group1,
            Instance::default(),
        );
    }
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

    // Parse the ICC profile and insert it, grouped by table provenance.
    let icc_tags = parse_icc_profile(icc_data)?;
    insert_icc_tags(&mut metadata, icc_tags);

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

/// Main ICC profile parser - uses registry-based approach.
///
/// Parses the 128-byte header and the tag table as two separate passes
/// (matching `ProcessICC_Profile`'s own two `ProcessDirectory` calls,
/// ICC_Profile.pm:1283-1295) so each can be tagged with its own family-1
/// group: every header field is unconditionally `ICC-header`
/// (ICC_Profile.pm:654); every tag-table field is `""` (no override) unless
/// [`tags::icc_output_group1`] names one of the three sub-structure-decoded
/// exceptions.
fn parse_icc_profile(data: &[u8]) -> Result<Vec<IccTag>> {
    if data.len() < 128 {
        return Err(ExifToolError::parse_error(
            "ICC profile too small (< 128 bytes)",
        ));
    }

    let mut header_map = HashMap::new();
    header::parse_header_registry(data, &mut header_map)?;
    let mut out: Vec<IccTag> = header_map
        .into_iter()
        .map(|(name, value)| IccTag {
            name,
            value,
            group1: "ICC-header",
        })
        .collect();

    if data.len() > 128 {
        let mut tag_map = HashMap::new();
        tags::parse_tags_registry(data, &mut tag_map)?;
        out.extend(tag_map.into_iter().map(|(name, value)| {
            let group1 = tags::icc_output_group1(&name);
            IccTag {
                name,
                value,
                group1,
            }
        }));
    }

    Ok(out)
}
