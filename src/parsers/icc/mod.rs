//! ICC Profile parser for embedded color profiles
//!
//! This module handles parsing of ICC (International Color Consortium)
//! profiles embedded in various file formats (PDF, JPEG, PNG, TIFF, etc.).
//! ICC profiles describe color characteristics for accurate color reproduction
//! across different devices.
//!
//! # Architecture
//!
//! The 128-byte **header** is not parsed here at all: it is ExifTool's
//! `%Image::ExifTool::ICC_Profile::Header` table (ICC_Profile.pm:652-757), a
//! `ProcessBinaryData` table the generator transcribes into
//! `src/exiftool_tables` (`find_table("ICC_Profile", "Header")`), and
//! [`parse_icc_profile`] routes the header bytes through the shared engine
//! walk exactly as `ProcessICC_Profile` hands them to `ProcessDirectory`
//! (ICC_Profile.pm:1283-1295). Every conversion the header carries -- the
//! manufacturer/CMM signature hashes, `ProfileVersion`'s nibble arithmetic,
//! the `int16u[6]` date, the `CMMFlags`/`DeviceAttributes` bit text,
//! `fixed32s[3]` illuminant, `HexID` -- is the generated, oracle-verified
//! one; a hand transcription of the same table lived in `header.rs` until
//! 4b-ii and disagreed with the pinned oracle on 55 corpus values (blank
//! or unlisted signatures, which ExifTool prints as `""` / `Unknown (...)`
//! and the hand code dropped).
//!
//! The **tag table** (everything after the header) still uses the
//! registry-based approach:
//! - **TagRegistry**: Table-driven tag definitions with signatures, names, and decoders
//! - **LookupTables**: Static lookup tables for enumerations (technology, illuminant, etc.)
//!
//! # Module Structure
//!
//! - [`binary`]: Low-level binary data readers
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
//! from each decoded tag's own table provenance (the generated header table
//! declares `group1: "ICC-header"`; [`tags::icc_output_group1`] covers the
//! three tag-table sub-structures oxidex decodes). Before this step every
//! ICC-bearing format inserted a flat `Profile:`/`ICC_Profile:`-prefixed key
//! with no family-1 information at all, and only JPEG's own
//! `normalize_metadata_map` post-pass even attempted the `Profile:` ->
//! `ICC_Profile:` rename -- every other format (PNG's `iCCP` chunk, GIF,
//! FLIF, PSD, XCF, standalone `.icc`, embedded TIFF/RAW) left its ICC tags
//! under the internal `Profile:` prefix forever, since none of them ever
//! called that JPEG-only function.

mod binary;
mod pdf;
mod registries;
mod tags;

use crate::core::tag_occurrence::{Instance, SHIM_DEFAULT_PRIORITY};
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{Ctx, Dir, find_table, process_binary_data};
use crate::io::ByteOrder;
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

/// Main ICC profile parser.
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

    let mut out = header_tags(&data[..128]);

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

/// The header through the generated `ICC_Profile::Header` table
/// (ICC_Profile.pm:652-757; `DirLen => 128` at :1291, so exactly the first
/// 128 bytes are the directory).
///
/// Keep the `find_table` call literal: Step 28's reachability census counts
/// literal roots, and the table is walked only while its Gate-B allowlist
/// line in `exiftool_tables/enabled.rs` holds -- `enabled()` re-checks Gate A
/// at runtime, so a future regeneration that stops being sound for this
/// table silences the header rather than emitting a guess (the corpus gate's
/// non-regressing TOTAL rule is what would catch that).
fn header_tags(header: &[u8]) -> Vec<IccTag> {
    let Some(table) = find_table("ICC_Profile", "Header") else {
        return Vec::new();
    };
    if !table.enabled() {
        return Vec::new();
    }
    let mut members = HashMap::new();
    let mut ctx = Ctx::new(&mut members);
    let mut emitted = Vec::new();
    process_binary_data(
        table,
        Dir::whole(header, ByteOrder::Big),
        &mut ctx,
        &mut emitted,
    );
    emitted
        .into_iter()
        .map(|tag| IccTag {
            name: tag.name.to_string(),
            value: tag.value,
            group1: "ICC-header",
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 157-byte profile: the 128-byte header, a one-entry tag table and a
    /// `cprt` textType payload (ExifTool refuses a table with 0 entries,
    /// ICC_Profile.pm:1275). Identical to scratchpad `icc-fixture.icc`, whose
    /// expected values below were pinned with the pinned 13.59 oracle
    /// (`exiftool-pinned.sh -j -G1 -ICC_Profile:all`, 2026-09-06).
    fn fixture(
        platform: &[u8; 4],
        manufacturer: &[u8; 4],
        model: &[u8; 4],
        flags: u32,
        attributes_low: u32,
        id: [u8; 16],
    ) -> Vec<u8> {
        let data = b"text\0\0\0\0test\0";
        let mut h = vec![0u8; 128];
        let tag_table = [
            &1u32.to_be_bytes()[..],
            b"cprt",
            &144u32.to_be_bytes(),
            &(data.len() as u32).to_be_bytes(),
        ]
        .concat();
        let size = 128 + tag_table.len() + data.len();
        h[0..4].copy_from_slice(&(size as u32).to_be_bytes());
        h[4..8].copy_from_slice(b"lcms");
        h[8..12].copy_from_slice(&[2, 0x40, 0, 0]);
        h[12..16].copy_from_slice(b"mntr");
        h[16..20].copy_from_slice(b"RGB ");
        h[20..24].copy_from_slice(b"XYZ ");
        for (i, v) in [2020u16, 1, 2, 3, 4, 5].iter().enumerate() {
            h[24 + 2 * i..26 + 2 * i].copy_from_slice(&v.to_be_bytes());
        }
        h[36..40].copy_from_slice(b"acsp");
        h[40..44].copy_from_slice(platform);
        h[44..48].copy_from_slice(&flags.to_be_bytes());
        h[48..52].copy_from_slice(manufacturer);
        h[52..56].copy_from_slice(model);
        h[56..60].copy_from_slice(&0u32.to_be_bytes());
        h[60..64].copy_from_slice(&attributes_low.to_be_bytes());
        h[64..68].copy_from_slice(&1u32.to_be_bytes());
        for (i, v) in [63190i32, 65536, 54061].iter().enumerate() {
            h[68 + 4 * i..72 + 4 * i].copy_from_slice(&v.to_be_bytes());
        }
        h[80..84].copy_from_slice(b"lcms");
        h[84..100].copy_from_slice(&id);
        [h, tag_table, data.to_vec()].concat()
    }

    fn shown(tags: &[IccTag], name: &str) -> String {
        let tag = tags
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} missing"));
        assert_eq!(
            tag.group1,
            if name == "ProfileCopyright" {
                ""
            } else {
                "ICC-header"
            },
            "{name}"
        );
        match &tag.value {
            TagValue::String(s) => s.clone(),
            TagValue::Integer(i) => i.to_string(),
            other => panic!("{name}: unexpected value shape {other:?}"),
        }
    }

    /// Blank / unlisted signatures: the cases the hand transcription got
    /// wrong (it dropped the tag; ExifTool prints `""` or `Unknown (...)`).
    #[test]
    fn header_matches_the_pinned_oracle_for_blank_and_unlisted_signatures() {
        let tags =
            parse_icc_profile(&fixture(b"\0\0\0\0", b"SEC\0", b"\0\0\0\0", 0, 0, [0; 16])).unwrap();
        let expected = [
            ("ProfileCMMType", "Little CMS"),
            ("ProfileVersion", "2.4.0"),
            ("ProfileClass", "Display Device Profile"),
            ("ColorSpaceData", "RGB "),
            ("ProfileConnectionSpace", "XYZ "),
            ("ProfileDateTime", "2020:01:02 03:04:05"),
            ("ProfileFileSignature", "acsp"),
            ("PrimaryPlatform", "Unknown ()"),
            ("CMMFlags", "Not Embedded, Independent"),
            ("DeviceManufacturer", "Unknown (SEC)"),
            ("DeviceModel", ""),
            ("DeviceAttributes", "Reflective, Glossy, Positive, Color"),
            ("RenderingIntent", "Media-Relative Colorimetric"),
            ("ConnectionSpaceIlluminant", "0.9642 1 0.82491"),
            ("ProfileCreator", "Little CMS"),
            ("ProfileID", "0"),
            ("ProfileCopyright", "test"),
        ];
        for (name, want) in expected {
            assert_eq!(shown(&tags, name), want, "{name}");
        }
        assert_eq!(tags.len(), expected.len(), "exactly the oracle's tag set");
    }

    /// Listed signatures, set flag bits and a real profile ID: the hit paths
    /// (scratchpad `icc-fixture2.icc`, same oracle run).
    #[test]
    fn header_matches_the_pinned_oracle_for_listed_signatures() {
        let id: [u8; 16] = core::array::from_fn(|i| 0x10 + i as u8);
        let tags = parse_icc_profile(&fixture(b"APPL", b"APPL", b"sRGB", 3, 0x0F, id)).unwrap();
        for (name, want) in [
            ("PrimaryPlatform", "Apple Computer Inc."),
            ("CMMFlags", "Embedded, Not Independent"),
            ("DeviceManufacturer", "Apple Computer Inc."),
            ("DeviceModel", "sRGB"),
            ("DeviceAttributes", "Transparency, Matte, Negative, B&W"),
            ("ProfileID", "101112131415161718191a1b1c1d1e1f"),
        ] {
            assert_eq!(shown(&tags, name), want, "{name}");
        }
    }

    #[test]
    fn a_short_buffer_is_refused_before_the_engine_runs() {
        assert!(parse_icc_profile(&[0u8; 127]).is_err());
    }
}
