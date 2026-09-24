//! JPEG EXIF/XMP segment writing
//!
//! This module handles writing metadata to JPEG files, specifically replacing or
//! inserting EXIF APP1 segments with modified metadata.
//!
//! # JPEG Structure with EXIF
//!
//! JPEG files consist of a sequence of segments:
//! - **SOI marker**: 0xFFD8 (Start of Image) - 2 bytes, no length field
//! - **Segments**: Each segment has:
//!   - **Marker**: 2 bytes (0xFFXX)
//!   - **Length**: 2 bytes (big-endian), includes length field but NOT marker
//!   - **Data**: Variable-length payload (length - 2 bytes)
//! - **EOI marker**: 0xFFD9 (End of Image) - 2 bytes, no length field
//!
//! # EXIF APP1 Segment Structure
//!
//! EXIF metadata is stored in an APP1 segment (marker 0xFFE1):
//! 1. Marker: 0xFFE1 (2 bytes)
//! 2. Length: 2 bytes (big-endian, includes itself + header + TIFF data, but NOT marker)
//! 3. EXIF identifier: "Exif\0\0" (6 bytes)
//! 4. TIFF IFD data: Complete TIFF structure with header and IFD
//!
//! # Example
//!
//! ```no_run
//! use oxidex::writers::jpeg_writer::write_exif_to_jpeg;
//! use oxidex::core::metadata_map::MetadataMap;
//! use oxidex::core::tag_value::TagValue;
//! use oxidex::io::buffered_reader::BufferedReader;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let reader = BufferedReader::new(Path::new("image.jpg"))?;
//! let mut metadata = MetadataMap::new();
//! metadata.insert("EXIF:Artist", TagValue::new_string("John Doe"));
//!
//! let modified_jpeg = write_exif_to_jpeg(&reader, &metadata)?;
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

use crate::core::FileReader;
use crate::core::metadata_map::MetadataMap;
use crate::error::{ExifToolError, Result};
use crate::parsers::jpeg::{Segment, parse_segments};

/// EXIF identifier that appears at the start of EXIF APP1 segment data
const EXIF_IDENTIFIER: &[u8] = b"Exif\0\0";

/// APP1 marker (0xFFE1) - used for EXIF and XMP
const APP1_MARKER: u16 = 0xFFE1;

/// Start of Image marker (0xFFD8)
const SOI_MARKER: u16 = 0xFFD8;

/// End of Image marker (0xFFD9)
const EOI_MARKER: u16 = 0xFFD9;

/// Start of Scan marker (0xFFDA) - entropy-coded image data follows its header
const SOS_MARKER: u16 = 0xFFDA;

/// Restart markers (RST0-RST7) have no length field
const RST0_MARKER: u16 = 0xFFD0;
const RST7_MARKER: u16 = 0xFFD7;

/// Writes modified EXIF metadata to a JPEG file structure.
///
/// This function:
/// 1. Parses the original JPEG using segment_parser
/// 2. Serializes modified EXIF tags using tiff_writer
/// 3. Replaces the EXIF APP1 segment (or inserts if not present)
/// 4. Returns the complete modified JPEG as Vec<u8>
///
/// # EXIF Segment Construction
///
/// The new EXIF APP1 segment is constructed as follows:
/// - **TIFF Header**: 8 bytes (byte order marker + magic + IFD offset)
/// - **TIFF IFD**: Serialized using `serialize_ifd()`
/// - **EXIF Identifier**: "Exif\0\0" prefix (6 bytes)
/// - **Segment Length**: Calculated as 2 + 6 + 8 + IFD size
///
/// # Segment Preservation
///
/// - All non-EXIF segments are preserved in their original positions
/// - If EXIF segment exists, it's replaced with the new one
/// - If EXIF segment doesn't exist, new one is inserted after APP0 (or after SOI)
/// - Multiple APP1 segments (EXIF + XMP) are handled correctly
///
/// # Parameters
///
/// - `reader`: FileReader for reading the original JPEG file
/// - `metadata`: MetadataMap containing EXIF tags to write (only "EXIF:" tags are processed)
///
/// # Returns
///
/// - `Ok(Vec<u8>)`: Complete modified JPEG file as bytes
/// - `Err(ExifToolError)`: If parsing fails or JPEG structure is invalid
///
/// # Errors
///
/// Returns an error if:
/// - The file is not a valid JPEG (missing SOI marker)
/// - Segment parsing fails (truncated or malformed segments)
/// - TIFF IFD serialization fails (invalid tag values)
/// - Resulting JPEG would be invalid (e.g., segments too large)
///
/// # Example
///
/// ```no_run
/// use oxidex::writers::jpeg_writer::write_exif_to_jpeg;
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
/// use oxidex::io::buffered_reader::BufferedReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = BufferedReader::new(Path::new("image.jpg"))?;
/// let mut metadata = MetadataMap::new();
/// metadata.insert("EXIF:Artist", TagValue::new_string("John Doe"));
/// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
///
/// let modified_jpeg = write_exif_to_jpeg(&reader, &metadata)?;
/// // Write modified_jpeg to file...
/// # Ok(())
/// # }
/// ```
pub fn write_exif_to_jpeg(reader: &dyn FileReader, metadata: &MetadataMap) -> Result<Vec<u8>> {
    write_exif_to_jpeg_with_removals(reader, metadata, &[])
}

/// [`write_exif_to_jpeg`], plus the keys the caller asked by name to delete
/// (`exif_surgical::plan_exif_write_with_removals`).
pub(crate) fn write_exif_to_jpeg_with_removals(
    reader: &dyn FileReader,
    metadata: &MetadataMap,
    removed: &[String],
) -> Result<Vec<u8>> {
    // Step 1: Parse original JPEG segments
    let segments = parse_segments(reader)?;

    // Step 2: Build new EXIF APP1 segment surgically (raw carry-over,
    // original byte order, MakerNotes preserved) — issue #20
    let file_size = reader.size() as usize;
    let file_bytes = reader.read(0, file_size)?;
    let new_exif_segment = crate::writers::exif_surgical::rewrite_jpeg_exif_with_removals(
        file_bytes, metadata, removed,
    )?;

    // Step 3: Entropy-coded scan data follows the SOS header and is not
    // segment-structured; the parser cannot represent it (it either stops or
    // misreads scan bytes as segments). Reconstruct only the segments up to
    // and including the SOS header, and copy everything after it verbatim.
    let sos_index = segments.iter().position(|seg| seg.marker == SOS_MARKER);
    let (head_segments, raw_tail) = match sos_index {
        Some(index) => {
            let sos = &segments[index];
            // marker (2) + length field (2) + scan header payload
            let tail_start = sos.offset as usize + 4 + sos.data.len();
            let file_size = reader.size() as usize;
            let tail = if tail_start < file_size {
                &reader.read(0, file_size)?[tail_start..]
            } else {
                &[][..]
            };
            (&segments[..=index], tail)
        }
        None => (&segments[..], &[][..]),
    };

    // Step 4: Find existing EXIF segment position (metadata segments always
    // precede the scan, so search only the head)
    let exif_position = head_segments.iter().position(|seg| is_exif_segment(seg));

    // Step 5: Reconstruct JPEG with modified EXIF
    let output = reconstruct_jpeg(head_segments, new_exif_segment, exif_position, raw_tail)?;

    // Step 6: the verbatim tail moved by the length change; re-base the
    // absolute offsets of any AFCP trailer in it (AFCP.pm 13.59:205-217).
    let tail_start = file_size - raw_tail.len();
    crate::writers::jpeg_trailer::rebase_trailer_offsets(file_bytes, tail_start, tail_start, output)
}

/// Checks if a segment is an EXIF APP1 segment.
///
/// An EXIF APP1 segment is identified by:
/// - Marker 0xFFE1 (APP1)
/// - Data starts with "Exif\0\0" identifier
///
/// This distinguishes EXIF from XMP (which also uses APP1 but has different identifier).
fn is_exif_segment(segment: &Segment) -> bool {
    segment.is_app1() && segment.data.starts_with(EXIF_IDENTIFIER)
}

/// Writer.pl calls a later APP1 block ExtendedEXIF only after an IFD0 or
/// ExtendedEXIF directory, with no leading bytes before `Exif\0\0`, and only
/// when the bytes following that identifier are not a fresh TIFF header.
fn is_extended_exif_continuation(previous_was_exif_directory: bool, segment: &Segment) -> bool {
    previous_was_exif_directory
        && is_exif_segment(segment)
        && !segment.data[EXIF_IDENTIFIER.len()..].starts_with(b"MM\0*")
        && !segment.data[EXIF_IDENTIFIER.len()..].starts_with(b"II*\0")
}

/// Writer.pl also recognizes case variants and up to four leading junk bytes.
/// These are outside the canonical carrier adapter: refuse them before treating
/// a JPEG as fresh, so an unrecognized EXIF directory cannot bypass its barrier.
fn has_unmodelled_exif_signature(segment: &Segment) -> bool {
    segment.is_app1()
        && !is_exif_segment(segment)
        && (0..=4).any(|offset| {
            segment.data.get(offset..).is_some_and(|data| {
                data.len() >= 7 && data[..4].eq_ignore_ascii_case(b"Exif") && data[4] == 0
            })
        })
}

/// Inactive carrier adapter for already resolved raw IFD edits. Requires one
/// existing EXIF block; choosing defaults for a new block belongs to the
/// source-derived writer contract. Copy all bytes outside that block verbatim.
pub(crate) fn apply_raw_exif_edits(
    reader: &dyn FileReader,
    edits: &[crate::writers::tiff_surgical::entry_edits::ScopedEntryEdit],
) -> Result<Vec<u8>> {
    replace_existing_exif(reader, |tiff| {
        crate::writers::tiff_surgical::entry_edits::apply_entry_edits(tiff, edits)
            .map(|bytes| (bytes, ()))
    })
    .map(|(bytes, ())| bytes)
}

/// Apply generated scalar semantics to an existing JPEG EXIF block. The
/// replacement mechanism knows only JPEG storage, not names or tag formats.
/// Source-derived defaults for creating a new EXIF block remain unsupported.
pub(crate) fn rewrite_generated_exif_scalars(
    reader: &dyn FileReader,
    requests: Vec<crate::writers::tiff_surgical::generated_scalar::ScalarWriteRequest<'_>>,
    rules: &crate::writers::tiff_surgical::generated_scalar::ScalarWriteRules<'_>,
) -> Result<crate::writers::tiff_surgical::generated_scalar::ScalarWriteOutput> {
    use crate::writers::tiff_surgical::generated_scalar::{
        ScalarWriteOutput, rewrite_generated_scalars,
    };
    let (bytes, warnings) = replace_existing_exif(reader, |tiff| {
        rewrite_generated_scalars(tiff, requests, rules)
            .map(|output| (output.bytes, output.warnings))
    })?;
    Ok(ScalarWriteOutput { bytes, warnings })
}

/// Apply an address-resolved generated batch without reducing its identity to a name.
pub(crate) fn rewrite_resolved_generated_exif_scalars(
    reader: &dyn FileReader,
    requests: Vec<crate::writers::tiff_surgical::generated_scalar::ResolvedScalarWriteRequest<'_>>,
    rules: &crate::writers::tiff_surgical::generated_scalar::ScalarWriteRules<'_>,
) -> Result<crate::writers::tiff_surgical::generated_scalar::ScalarWriteOutput> {
    use crate::writers::tiff_surgical::generated_scalar::{
        ScalarWriteOutput, rewrite_resolved_generated_scalars,
    };
    let (bytes, warnings) = replace_existing_exif(reader, |tiff| {
        rewrite_resolved_generated_scalars(tiff, requests, rules)
            .map(|output| (output.bytes, output.warnings))
    })?;
    Ok(ScalarWriteOutput { bytes, warnings })
}

/// Execute a mixed public transaction without exposing intermediate file writes.
pub(crate) fn write_public_exif_transaction(
    reader: &dyn FileReader,
    baseline: &MetadataMap,
    plan: crate::writers::generated_public_write::PublicWritePlan,
) -> Result<Vec<u8>> {
    if plan.whole_exif_clear {
        return transform_exif(reader, |_, _| Ok((Vec::new(), ()))).map(|(bytes, ())| bytes);
    }
    if plan.generated.is_empty() {
        return write_exif_to_jpeg_with_removals(
            reader,
            &plan.legacy_metadata,
            &plan.legacy_removed,
        );
    }
    transform_exif(reader, |original, head| {
        rewrite_generated_exif_payload(original, &|| source_raw_properties(head), baseline, plan)
            .map(|bytes| (bytes, ()))
    })
    .map(|(bytes, ())| bytes)
}

/// The EXIF TIFF payload (header onward) a public transaction with a
/// non-empty generated part leaves in a carrier whose original payload is
/// `original` (`None`: the carrier has no EXIF yet, which is then created
/// with the source-derived fresh byte order and mandatory entries). An empty
/// result means the carrier should drop its EXIF block.
///
/// Carrier-neutral: the JPEG APP1 writer and the PNG `eXIf` writer both call
/// it. `raw_properties` supplies the carrier's raw JFIF properties that
/// `WriteExif` seeds mandatory resolution defaults from (`JFIFXResolution`
/// etc., WriteExif.pl 13.59:705-711); a carrier with no JFIF segment
/// supplies none, as `$$et{JFIFYResolution}` is then undefined.
pub(crate) fn rewrite_generated_exif_payload(
    original: Option<&[u8]>,
    raw_properties: &dyn Fn() -> Result<std::collections::BTreeMap<String, i64>>,
    baseline: &MetadataMap,
    plan: crate::writers::generated_public_write::PublicWritePlan,
) -> Result<Vec<u8>> {
    use crate::writers::mandatory_defaults_runtime as mandatory;
    use crate::writers::tiff_surgical::{self, entry_edits, generated_scalar};
    // The legacy delta is normally applied by the in-place TIFF payload
    // writer, which cannot shrink an IFD and refuses a deletion. A plan that
    // deletes a legacy tag (a key gone from the map, or named) therefore
    // applies its legacy delta first through the reconstructing surgical
    // writer -- the one a legacy-only plan uses -- and the generated edits
    // on top of that. Before, one write setting IFD0:Artist and deleting
    // ExifIFD:ISO failed "cannot shrink an IFD table".
    //
    // That legacy half is not a clear even when it leaves no IFD0/ExifIFD/
    // GPS row, so the writer's drop-all shortcut is off: an IFD1 thumbnail
    // or a MakerNote beside the last deleted tag is carried. When no entry
    // at all is left, the generated edits start from an empty IFD0 in the
    // original byte order; whether `WriteExif` seeds mandatory entries is
    // still decided by the original IFD0 (`$numEntries`, WriteExif.pl
    // 13.59:714-719), which existed. Oracle, `-ExifIFD:ISO= -IFD0:Artist=you`
    // on a block holding only ISO: byte order kept, IFD0 = {Artist}.
    let creation_count = match original {
        Some(tiff) => Some(tiff_surgical::ifd0_state(tiff)?.0),
        None => None,
    };
    let mut plan = plan;
    let staged;
    let restaged =
        matches!(original, Some(_) if plan.has_legacy_changes && legacy_deletes(baseline, &plan));
    let original = match original {
        Some(tiff) if restaged => {
            let kept = crate::writers::exif_surgical::rewrite_tiff_exif_keeping_carrier(
                tiff,
                baseline,
                &plan.legacy_metadata,
                &plan.legacy_removed,
            )?;
            staged = if kept.is_empty() {
                let order = match crate::writers::exif_surgical::scan_exif_entries(tiff)?.byte_order
                {
                    crate::parsers::tiff::ifd_parser::ByteOrder::LittleEndian => {
                        mandatory::TiffByteOrder::Little
                    }
                    crate::parsers::tiff::ifd_parser::ByteOrder::BigEndian => {
                        mandatory::TiffByteOrder::Big
                    }
                };
                mandatory::serialize_ifd0_defaults(Vec::new(), order)
                    .map_err(ExifToolError::unsupported_format)?
            } else {
                kept
            };
            plan.has_legacy_changes = false;
            Some(staged.as_slice())
        }
        other => other,
    };
    let empty;
    let tiff = match original {
        Some(tiff) => tiff,
        None => {
            let order = source_fresh_byte_order()?;
            empty = mandatory::serialize_ifd0_defaults(Vec::new(), order)
                .map_err(ExifToolError::unsupported_format)?;
            &empty
        }
    };
    let scan = crate::writers::exif_surgical::scan_exif_entries(tiff)?;
    let original_count = match creation_count {
        Some(count) => count,
        None => tiff_surgical::ifd0_state(tiff)?.0,
    };
    let order = match scan.byte_order {
        crate::parsers::tiff::ifd_parser::ByteOrder::LittleEndian => {
            mandatory::TiffByteOrder::Little
        }
        crate::parsers::tiff::ifd_parser::ByteOrder::BigEndian => mandatory::TiffByteOrder::Big,
    };
    let prepare_legacy = |bytes: &[u8]| {
        if plan.has_legacy_changes {
            tiff_surgical::rewrite_tiff_payload_with_removals(
                bytes,
                baseline,
                &plan.legacy_metadata,
                &plan.legacy_removed,
                true,
            )
        } else {
            Ok(bytes.to_vec())
        }
    };
    let legacy = prepare_legacy(tiff)?;
    // Resolve conversions first. A defined input can still be rejected or
    // become NoEdit; only an actual set can trigger directory creation.
    let generated = generated_scalar::plan_resolved_generated_scalars(
        &legacy,
        plan.generated,
        &generated_scalar::generated_rules(),
    )?;
    let ifd1_entries_before = entry_edits::ifd1_entry_count(&legacy)?;
    let needs_ifd1_defaults = ifd1_entries_before.unwrap_or(0) == 0
        && generated.edits.iter().any(|edit| {
            edit.ifd == crate::writers::exif_surgical::IfdKind::Ifd1
                && matches!(edit.mutation, entry_edits::EntryMutation::Set { .. })
        });
    let has_ifd1_delete = generated.edits.iter().any(|edit| {
        edit.ifd == crate::writers::exif_surgical::IfdKind::Ifd1
            && matches!(edit.mutation, entry_edits::EntryMutation::Delete)
    });
    let ifd1_mandatory = if needs_ifd1_defaults || has_ifd1_delete {
        validate_creation_sources()?;
        let properties = raw_properties()?;
        let recipe = &crate::writers::generated_mandatory_defaults::MANDATORY_DEFAULTS;
        mandatory::require_ifd1_mandatory_cleanup(recipe)
            .map_err(ExifToolError::unsupported_format)?;
        Some(mandatory_default_edits(recipe, "IFD1", order, &properties)?)
    } else {
        None
    };
    let legacy = if original_count == 0
        && (generated.has_set() || tiff_surgical::ifd0_state(&legacy)?.0 != 0)
    {
        validate_creation_sources()?;
        let properties = raw_properties()?;
        let recipe = &crate::writers::generated_mandatory_defaults::MANDATORY_DEFAULTS;
        let mut edits = mandatory_default_edits(recipe, "IFD0", order, &properties)?;
        edits.extend(ifd1_defaults_for_creation(
            needs_ifd1_defaults,
            ifd1_mandatory.as_deref(),
        )?);
        let seeded = entry_edits::apply_entry_edits(tiff, &edits)?;
        // Reapply the authored legacy delta against the original baseline:
        // explicit user overrides/removals take precedence over defaults.
        prepare_legacy(&seeded)?
    } else if needs_ifd1_defaults {
        let edits = ifd1_defaults_for_creation(needs_ifd1_defaults, ifd1_mandatory.as_deref())?;
        prepare_legacy(&entry_edits::apply_entry_edits(&legacy, &edits)?)?
    } else {
        legacy
    };
    let output = generated.apply(&legacy)?.bytes;
    let ifd1_entries_after = entry_edits::ifd1_entry_count(&output)?;
    let ifd1_shrank =
        has_ifd1_delete && ifd1_entries_after.unwrap_or(0) < ifd1_entries_before.unwrap_or(0);
    let output = if ifd1_shrank {
        // Existing IFD1 cleanup must compare the survivor's physical
        // type/count/value exactly as WriteExif does. Reuse the guarded
        // generated predicate used for TIFF rather than the creation
        // operands, which describe only new-directory encodings.
        let properties = raw_properties()?;
        let recipe = &crate::writers::generated_mandatory_defaults::MANDATORY_DEFAULTS;
        let defaults = mandatory::defaults_with_properties(recipe, "IFD1", false, 0, &properties)
            .map_err(ExifToolError::unsupported_format)?;
        super::generated_public_write::cleanup_source_mandatory_ifd1_with_defaults(
            &output, recipe, &defaults,
        )?
    } else if needs_ifd1_defaults && ifd1_mandatory.is_some() {
        entry_edits::remove_ifd1_if_only_mandatory(&output, ifd1_mandatory.as_ref().unwrap(), true)?
    } else {
        output
    };
    let (count, next) = tiff_surgical::ifd0_state(&output)?;
    Ok(if count == 0 && !next {
        Vec::new()
    } else if restaged {
        // The staged block was laid out by the serializer and the generated
        // edits then grew it in place, which moves a grown directory to the
        // end and leaves its old table behind: `-ExifIFD:ISO=
        // -IFD0:Artist=you` in one pass put IFD0 after IFD1 and left the
        // staged empty IFD0 at offset 8, and pinned ExifTool 13.59
        // `-validate` warned "Short directory size for IFD1 (missing 8
        // bytes)", as its own edit does not. Laid out once more, as a whole.
        crate::writers::exif_surgical::relayout_exif(&output)?
    } else {
        // Grown in place: a relocated IFD0 lands after the chain it links
        // (`entry_edits::chain_tables_after_ifd0`).
        entry_edits::chain_tables_after_ifd0(&output)?
    })
}

/// Whether a public plan's legacy delta deletes a tag: an EXIF-family key of
/// `baseline` that `legacy_metadata` no longer holds, or a named removal.
/// The raw-carried directories count too: a surfaced InteropIFD, IFD1 or
/// MakerNotes row dropped from the map must reach the reconstructing
/// writer, which refuses it, not the in-place payload writer, which never
/// walks those directories and would report success with the row kept.
fn legacy_deletes(
    baseline: &MetadataMap,
    plan: &crate::writers::generated_public_write::PublicWritePlan,
) -> bool {
    !plan.legacy_removed.is_empty()
        || baseline.iter().any(|(key, _)| {
            [
                "IFD0:",
                "ExifIFD:",
                "GPS:",
                "EXIF:",
                "IFD1:",
                "InteropIFD:",
                "MakerNotes:",
            ]
            .iter()
            .any(|prefix| key.starts_with(prefix))
                && !plan.legacy_metadata.contains_key(key)
        })
}

/// `WriteExif` adds mandatory entries only when creating an empty directory.
/// Delete cleanup receives the same immutable entries for comparison, but it
/// must never serialize them into an existing IFD1.
fn ifd1_defaults_for_creation(
    needs_ifd1_defaults: bool,
    mandatory: Option<&[crate::writers::tiff_surgical::entry_edits::ScopedEntryEdit]>,
) -> Result<Vec<crate::writers::tiff_surgical::entry_edits::ScopedEntryEdit>> {
    if !needs_ifd1_defaults {
        return Ok(Vec::new());
    }
    mandatory
        .map(|entries| entries.to_vec())
        .ok_or_else(|| ExifToolError::unsupported_format("missing IFD1 mandatory recipe"))
}

fn mandatory_default_edits(
    recipe: &crate::writers::mandatory_defaults_runtime::MandatoryRecipe,
    directory: &str,
    order: crate::writers::mandatory_defaults_runtime::TiffByteOrder,
    properties: &std::collections::BTreeMap<String, i64>,
) -> Result<Vec<crate::writers::tiff_surgical::entry_edits::ScopedEntryEdit>> {
    use crate::writers::mandatory_defaults_runtime as mandatory;
    use crate::writers::tiff_surgical::entry_edits::{EntryMutation, ScopedEntryEdit};
    let ifd = match directory {
        "IFD0" => crate::writers::exif_surgical::IfdKind::Ifd0,
        "IFD1" => crate::writers::exif_surgical::IfdKind::Ifd1,
        _ => {
            return Err(ExifToolError::unsupported_format(
                "generated mandatory directory is outside the public TIFF carrier",
            ));
        }
    };
    let defaults = mandatory::defaults_with_properties(recipe, directory, false, 0, properties)
        .map_err(ExifToolError::unsupported_format)?;
    mandatory::encode_ifd0_defaults(recipe, &defaults, order)
        .map_err(ExifToolError::unsupported_format)
        .map(|encoded| {
            encoded
                .into_iter()
                .map(|field| ScopedEntryEdit {
                    ifd,
                    tag_id: field.tag_id,
                    mutation: EntryMutation::Set {
                        field_type: field.tiff_type,
                        count: field.count,
                        bytes: field.bytes,
                    },
                })
                .collect()
        })
}

fn validate_creation_sources() -> Result<()> {
    let address =
        crate::writers::generated_setnewvalue_address_rules::SET_NEW_VALUE_ADDRESS_CAPTURE
            .ok_or_else(|| {
                ExifToolError::unsupported_format("generated address source is absent")
            })?;
    let order = &crate::writers::generated_fresh_jpeg_byte_order::FRESH_JPEG_BYTE_ORDER;
    let mandatory = &crate::writers::generated_mandatory_defaults::MANDATORY_DEFAULTS;
    let raw = &crate::writers::generated_raw_jfif::RAW_JFIF;
    if order.exiftool_version != address.exiftool_version
        || order.caller_source_sha256 != address.main_source_sha256
        || order.set_preferred_source_sha256 != address.writer_source_sha256
        || order.set_byte_order_source_sha256 != address.main_source_sha256
        || order.get_byte_order_source_sha256 != address.main_source_sha256
        || mandatory.writer_source_sha256 != address.write_exif_source_sha256
        || mandatory.write_value_source_sha256 != address.writer_source_sha256
        || raw.source_core_sha256 != address.main_source_sha256
        || raw.source_writer_sha256 != address.writer_source_sha256
    {
        return Err(ExifToolError::unsupported_format(
            "generated EXIF creation sources differ",
        ));
    }
    Ok(())
}

fn source_fresh_byte_order() -> Result<crate::writers::mandatory_defaults_runtime::TiffByteOrder> {
    use crate::writers::generated_fresh_jpeg_byte_order::*;
    use crate::writers::mandatory_defaults_runtime::TiffByteOrder;
    validate_creation_sources()?;
    let selected = fresh_jpeg_byte_order(
        &FRESH_JPEG_BYTE_ORDER,
        FreshJpegByteOrderInputs {
            byte_order_option: None,
            exif_byte_order: None,
            maker_note_byte_order: None,
        },
    )
    .map_err(ExifToolError::unsupported_format)?;
    Ok(match selected {
        FreshJpegExifByteOrder::LittleEndian => TiffByteOrder::Little,
        FreshJpegExifByteOrder::BigEndian => TiffByteOrder::Big,
    })
}

fn source_raw_properties(head: &[Segment<'_>]) -> Result<std::collections::BTreeMap<String, i64>> {
    let mut properties = std::collections::BTreeMap::new();
    for segment in head {
        if let Some(found) = crate::writers::raw_segment_properties::decode_raw_segment(
            &crate::writers::generated_raw_jfif::RAW_JFIF,
            segment.marker as u8,
            segment.data,
        )
        .map_err(ExifToolError::unsupported_format)?
        {
            properties.extend(found);
        }
    }
    Ok(properties)
}

/// Replace exactly one existing EXIF payload and preserve every other byte,
/// including non-EXIF APP1 blocks, scan data and the trailer.
fn replace_existing_exif<T>(
    reader: &dyn FileReader,
    transform: impl FnOnce(&[u8]) -> Result<(Vec<u8>, T)>,
) -> Result<(Vec<u8>, T)> {
    transform_exif(reader, |tiff, _head| {
        let tiff = tiff.ok_or_else(|| {
            ExifToolError::unsupported_format("Raw JPEG edits require an existing EXIF block")
        })?;
        transform(tiff)
    })
}

/// Replace or insert one EXIF payload while preserving original framing and
/// scan bytes. All format/tag semantics are supplied by the caller.
fn transform_exif<T>(
    reader: &dyn FileReader,
    transform: impl FnOnce(Option<&[u8]>, &[Segment<'_>]) -> Result<(Vec<u8>, T)>,
) -> Result<(Vec<u8>, T)> {
    let segments = parse_segments(reader)?;
    let end_index = segments
        .iter()
        .position(|seg| matches!(seg.marker, SOS_MARKER | EOI_MARKER))
        .ok_or_else(|| ExifToolError::parse_error("Incomplete JPEG header"))?;
    let head = &segments[..=end_index];
    if head.iter().enumerate().any(|(index, seg)| {
        if index == 0 {
            seg.marker != SOI_MARKER
        } else {
            // Length-bearing JPEG header markers occupy C0..FE. Restart
            // markers belong to scan data, and SOI may occur only once.
            !(0xffc0..=0xfffe).contains(&seg.marker)
                || (RST0_MARKER..=SOI_MARKER).contains(&seg.marker)
        }
    }) {
        return Err(ExifToolError::parse_error("Invalid JPEG header marker"));
    }
    if head.iter().any(has_unmodelled_exif_signature) {
        return Err(ExifToolError::unsupported_format(
            "Noncanonical EXIF directory requires a generated carrier protocol",
        ));
    }
    let mut ordinary_exif = Vec::new();
    let mut previous_was_exif_directory = false;
    let mut has_extended_exif = false;
    for (index, segment) in head.iter().enumerate() {
        if is_exif_segment(segment) {
            if is_extended_exif_continuation(previous_was_exif_directory, segment) {
                has_extended_exif = true;
            } else {
                ordinary_exif.push((index, segment));
            }
            previous_was_exif_directory = true;
        } else if (0xffe0..=0xffef).contains(&segment.marker) {
            // A later APP directory changes Writer.pl's dirOrder. Treat an
            // unmodelled APP directory conservatively: it cannot authorize a
            // continuation in this raw carrier adapter.
            previous_was_exif_directory = false;
        }
    }
    if ordinary_exif.len() > 1 {
        return Err(ExifToolError::parse_error(
            "Ambiguous multiple JPEG EXIF blocks",
        ));
    }
    let policy = &crate::writers::generated_raw_jfif::RAW_JFIF;
    if has_extended_exif
        && policy
            .creation_wait_for_directories
            .contains(&"ExtendedEXIF")
    {
        return Err(ExifToolError::unsupported_format(
            "ExtendedEXIF carrier requires a generated directory barrier",
        ));
    }
    let existing = ordinary_exif.into_iter().next();
    let target_index = if let Some((index, _)) = existing {
        if !policy.creation_wait_for_directories.contains(&"IFD0") {
            return Err(ExifToolError::unsupported_format(
                "source creation policy does not support existing IFD0 placement",
            ));
        }
        index
    } else {
        validate_creation_sources()?;
        head.iter()
            .enumerate()
            .skip(1) // SOI framing precedes all metadata.
            .find(|(_, segment)| {
                !policy
                    .creation_skip_markers
                    .contains(&(segment.marker as u8))
            })
            .map(|(index, _)| index)
            .ok_or_else(|| ExifToolError::parse_error("Missing JPEG creation boundary"))?
    };
    let property_prefix = match policy.creation_timing {
        crate::writers::raw_segment_properties::RawCreationTiming::BeforeCurrentSegment => {
            &head[..target_index]
        }
    };
    let (tiff, outcome) = transform(
        existing.map(|(_, block)| &block.data[EXIF_IDENTIFIER.len()..]),
        property_prefix,
    )?;
    let length = tiff
        .len()
        .checked_add(EXIF_IDENTIFIER.len() + 2)
        .and_then(|len| u16::try_from(len).ok())
        .ok_or_else(|| ExifToolError::parse_error("Edited EXIF exceeds JPEG APP1 size limit"))?;
    let bytes = reader.read(0, reader.size() as usize)?;
    let start = usize::try_from(head[target_index].offset)
        .map_err(|_| ExifToolError::parse_error("JPEG segment offset exceeds address space"))?;
    let end = if let Some((_, block)) = existing {
        start
            .checked_add(4 + block.data.len())
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| ExifToolError::parse_error("Truncated JPEG EXIF segment"))?
    } else {
        if start > bytes.len() {
            return Err(ExifToolError::parse_error("Truncated JPEG insertion point"));
        }
        start
    };
    let mut out = bytes[..start].to_vec();
    if !tiff.is_empty() {
        out.extend_from_slice(&APP1_MARKER.to_be_bytes());
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(EXIF_IDENTIFIER);
        out.extend_from_slice(&tiff);
    }
    out.extend_from_slice(&bytes[end..]);
    // Everything from `end` moved by the length change, including any AFCP
    // trailer and its absolute offsets (AFCP.pm 13.59:205-217). Its EOI is
    // searched from the end of the SOS header, or is the EOI marker itself.
    let boundary = &head[end_index];
    let scan_from = usize::try_from(boundary.offset)
        .map_err(|_| ExifToolError::parse_error("JPEG segment offset exceeds address space"))?
        + if boundary.marker == SOS_MARKER {
            4 + boundary.data.len()
        } else {
            0
        };
    let out = crate::writers::jpeg_trailer::rebase_trailer_offsets(bytes, end, scan_from, out)?;
    Ok((out, outcome))
}

/// Reconstructs a complete JPEG file with modified EXIF segment.
///
/// This function iterates through all original segments and:
/// - Copies SOI marker
/// - For each segment:
///   - If it's the EXIF segment, writes new version
///   - Otherwise, copies original
/// - If no EXIF segment existed, inserts new one after APP0 or SOI
/// - Copies EOI marker
///
/// # Parameters
///
/// - `segments`: Original JPEG segments from parser
/// - `new_exif_data`: New EXIF segment data (excluding marker and length)
/// - `exif_position`: Position of existing EXIF segment, or None if not present
///
/// # Returns
///
/// - `Ok(Vec<u8>)`: Complete modified JPEG file
/// - `Err(ExifToolError)`: If reconstruction fails or segment is too large
fn reconstruct_jpeg(
    segments: &[Segment],
    new_exif_data: Vec<u8>,
    exif_position: Option<usize>,
    raw_tail: &[u8],
) -> Result<Vec<u8>> {
    // Pre-allocate buffer (rough estimate)
    let mut output = Vec::with_capacity(
        segments.iter().map(|s| s.data.len() + 4).sum::<usize>()
            + new_exif_data.len()
            + raw_tail.len(),
    );

    // Determine insertion position if EXIF doesn't exist
    let insert_position = if exif_position.is_none() {
        // Find APP0 position (insert after it)
        // If no APP0, insert after SOI (position 1)
        segments
            .iter()
            .position(|seg| seg.marker == 0xFFE0)
            .map(|pos| pos + 1)
            .unwrap_or(1) // After SOI
    } else {
        0 // Not used if exif_position is Some
    };

    let mut exif_written = false;

    for (i, segment) in segments.iter().enumerate() {
        // Check if we need to insert new EXIF before this segment
        if exif_position.is_none() && i == insert_position && !exif_written {
            if !new_exif_data.is_empty() {
                write_segment(&mut output, APP1_MARKER, &new_exif_data)?;
            }
            exif_written = true;
        }

        // Write segment
        if Some(i) == exif_position {
            // Replace EXIF segment (or drop it entirely when new_exif_data is
            // empty — e.g. clear_all_metadata)
            if !new_exif_data.is_empty() {
                write_segment(&mut output, APP1_MARKER, &new_exif_data)?;
            }
            exif_written = true;
        } else {
            // Copy original segment
            write_segment(&mut output, segment.marker, segment.data)?;
        }
    }

    // If we still haven't written EXIF (shouldn't happen), add at end before EOI
    if !exif_written && !new_exif_data.is_empty() {
        // Remove EOI if present
        if output.len() >= 2 && output[output.len() - 2..] == [0xFF, 0xD9] {
            output.truncate(output.len() - 2);
        }
        write_segment(&mut output, APP1_MARKER, &new_exif_data)?;
        // Re-add EOI
        output.extend_from_slice(&EOI_MARKER.to_be_bytes());
    }

    // Entropy-coded scan data, EOI, and any trailer copied verbatim
    output.extend_from_slice(raw_tail);

    Ok(output)
}

/// Writes a single JPEG segment to output buffer.
///
/// For segments with data, writes:
/// - Marker (2 bytes, big-endian)
/// - Length (2 bytes, big-endian, includes length field but NOT marker)
/// - Data (variable length)
///
/// For standalone markers (SOI, EOI, RST0-RST7), writes only the marker.
///
/// # Parameters
///
/// - `output`: Output buffer to write to
/// - `marker`: 2-byte JPEG marker
/// - `data`: Segment data (empty for standalone markers)
///
/// # Returns
///
/// - `Ok(())`: Segment written successfully
/// - `Err(ExifToolError)`: If segment is too large (>65533 bytes)
fn write_segment(output: &mut Vec<u8>, marker: u16, data: &[u8]) -> Result<()> {
    // Write marker (2 bytes, big-endian)
    output.extend_from_slice(&marker.to_be_bytes());

    // Check if this is a standalone marker (no length or data)
    if is_standalone_marker(marker) {
        return Ok(());
    }

    // Calculate length: 2 (length field) + data.len()
    let length = 2 + data.len();

    // Validate length fits in u16
    if length > 0xFFFF {
        return Err(ExifToolError::invalid_tag_value(
            "segment_length",
            format!("Segment data too large: {} bytes (max 65533)", data.len()),
        ));
    }

    // Write length (2 bytes, big-endian)
    output.extend_from_slice(&(length as u16).to_be_bytes());

    // Write data
    output.extend_from_slice(data);

    Ok(())
}

/// Returns true if the marker is a standalone marker (no length field).
///
/// Standalone markers include:
/// - SOI (0xFFD8)
/// - EOI (0xFFD9)
/// - RST0-RST7 (0xFFD0-0xFFD7)
fn is_standalone_marker(marker: u16) -> bool {
    marker == SOI_MARKER || marker == EOI_MARKER || (RST0_MARKER..=RST7_MARKER).contains(&marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;
    use crate::io::EndianReader;
    use crate::test_support::TestReader;
    use crate::writers::exif_surgical::IfdKind;
    use crate::writers::tiff_surgical::entry_edits::{EntryMutation, ScopedEntryEdit};

    #[test]
    fn existing_ifd1_delete_uses_mandatory_operands_only_for_comparison() {
        // These are source-derived cleanup comparison operands. An existing
        // IFD1 may instead hold Artist alone, Compression=5, or XResolution
        // 300; none may be replaced by defaults while processing a delete.
        let cleanup_operands = vec![
            ScopedEntryEdit {
                ifd: IfdKind::Ifd1,
                tag_id: 0x0103,
                mutation: EntryMutation::Set {
                    field_type: 3,
                    count: 1,
                    bytes: vec![6, 0],
                },
            },
            ScopedEntryEdit {
                ifd: IfdKind::Ifd1,
                tag_id: 0x011a,
                mutation: EntryMutation::Set {
                    field_type: 5,
                    count: 1,
                    bytes: [72u32.to_le_bytes(), 1u32.to_le_bytes()].concat(),
                },
            },
        ];
        assert!(
            ifd1_defaults_for_creation(false, Some(&cleanup_operands))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            ifd1_defaults_for_creation(true, Some(&cleanup_operands))
                .unwrap()
                .len(),
            cleanup_operands.len()
        );
        assert!(ifd1_defaults_for_creation(true, None).is_err());
    }

    /// Creates a minimal valid JPEG with EXIF
    fn create_jpeg_with_exif() -> Vec<u8> {
        let mut data = Vec::new();

        // SOI marker
        data.extend_from_slice(&[0xFF, 0xD8]);

        // APP1 marker (EXIF)
        data.extend_from_slice(&[0xFF, 0xE1]);
        // Length: 2 + 6 (Exif\0\0) + 8 (TIFF header) = 16
        data.extend_from_slice(&[0x00, 0x10]);
        // EXIF identifier
        data.extend_from_slice(b"Exif\0\0");
        // TIFF header (little-endian, IFD at offset 8)
        data.extend_from_slice(&[0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00]);

        // EOI marker
        data.extend_from_slice(&[0xFF, 0xD9]);

        data
    }

    /// Creates a JPEG without EXIF (only SOI + EOI)
    fn create_jpeg_without_exif() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xD8]); // SOI
        data.extend_from_slice(&[0xFF, 0xD9]); // EOI
        data
    }

    /// Make an existing IFD1 whose entries exercise the public mixed
    /// generated/legacy delete route. The raw values deliberately differ
    /// from WriteExif's creation defaults: delete must retain them verbatim.
    fn create_jpeg_with_existing_ifd1(
        include_artist: bool,
        include_generated_document_name: bool,
        include_nondefault_mandatory: bool,
        include_native_fixed_width_mandatory: bool,
    ) -> Vec<u8> {
        let mut entries = Vec::new();
        if include_artist {
            entries.push((0x013b_u16, 2_u16, 7_u32, b"artist\0".to_vec()));
        }
        if include_generated_document_name {
            entries.push((0x010d, 2, 4, b"doc\0".to_vec()));
        }
        if include_nondefault_mandatory {
            entries.push((0x0103, 3, 1, vec![5, 0]));
            entries.push((
                0x011a,
                5,
                1,
                [300_u32.to_le_bytes(), 1_u32.to_le_bytes()].concat(),
            ));
        }
        if include_native_fixed_width_mandatory {
            // Compression uses its existing native byte carrier (BYTE), while
            // the remaining entries retain their source defaults. This is the
            // physical-survivor case that WriteExif prunes after Artist delete.
            entries.push((0x0103, 1, 1, vec![6]));
            entries.push((
                0x011a,
                5,
                1,
                [72_u32.to_le_bytes(), 1_u32.to_le_bytes()].concat(),
            ));
            entries.push((
                0x011b,
                5,
                1,
                [72_u32.to_le_bytes(), 1_u32.to_le_bytes()].concat(),
            ));
            entries.push((0x0128, 3, 1, vec![2, 0]));
        }
        entries.sort_by_key(|(tag_id, _, _, _)| *tag_id);

        // TIFF header plus an empty IFD0 whose next-IFD link points at 14.
        let ifd1_offset = 14_u32;
        let ifd1_table_end = ifd1_offset as usize + 2 + entries.len() * 12 + 4;
        let mut tiff = b"II\x2a\0\x08\0\0\0\0\0".to_vec();
        tiff.extend_from_slice(&ifd1_offset.to_le_bytes());
        tiff.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let mut external_values = Vec::new();
        for (tag_id, field_type, count, value) in entries {
            tiff.extend_from_slice(&tag_id.to_le_bytes());
            tiff.extend_from_slice(&field_type.to_le_bytes());
            tiff.extend_from_slice(&count.to_le_bytes());
            if value.len() <= 4 {
                tiff.extend_from_slice(&value);
                tiff.resize(tiff.len() + (4 - value.len()), 0);
            } else {
                let offset = ifd1_table_end + external_values.len();
                tiff.extend_from_slice(&(offset as u32).to_le_bytes());
                external_values.extend_from_slice(&value);
            }
        }
        tiff.extend_from_slice(&0_u32.to_le_bytes()); // no following IFD
        tiff.extend_from_slice(&external_values);

        let mut jpeg = vec![0xff, 0xd8];
        write_segment(&mut jpeg, 0xffe2, b"keep APP2 metadata").unwrap();
        let mut exif = EXIF_IDENTIFIER.to_vec();
        exif.extend_from_slice(&tiff);
        write_segment(&mut jpeg, APP1_MARKER, &exif).unwrap();
        write_segment(&mut jpeg, SOS_MARKER, b"scan header").unwrap();
        jpeg.extend_from_slice(b"\x12\xff\0\x34\xff\xd9trailer");
        jpeg
    }

    fn without_exif_segment(jpeg: &[u8]) -> Vec<u8> {
        let data_start = jpeg
            .windows(EXIF_IDENTIFIER.len())
            .position(|window| window == EXIF_IDENTIFIER)
            .expect("fixture contains one EXIF segment");
        let segment_start = data_start
            .checked_sub(4)
            .expect("EXIF has marker and length");
        assert_eq!(&jpeg[segment_start..segment_start + 2], &[0xff, 0xe1]);
        let segment_length = u16::from_be_bytes(
            jpeg[segment_start + 2..segment_start + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let segment_end = segment_start + 2 + segment_length;
        let mut without = jpeg[..segment_start].to_vec();
        without.extend_from_slice(&jpeg[segment_end..]);
        without
    }

    fn ifd1_entries(jpeg: &[u8]) -> Vec<crate::writers::exif_surgical::RawEntry> {
        crate::writers::exif_surgical::scan_exif_entries(
            crate::writers::exif_surgical_test_support::tiff_slice(jpeg),
        )
        .unwrap()
        .entries
        .into_iter()
        .filter(|entry| entry.ifd == IfdKind::Ifd1)
        .collect()
    }

    fn ifd1_entry(
        entries: &[crate::writers::exif_surgical::RawEntry],
        tag_id: u16,
    ) -> &crate::writers::exif_surgical::RawEntry {
        entries
            .iter()
            .find(|entry| entry.tag_id == tag_id)
            .unwrap_or_else(|| panic!("missing IFD1 tag {tag_id:#06x}"))
    }

    #[test]
    fn public_ifd1_delete_preserves_existing_nondefault_mandatory_entries() {
        let input = create_jpeg_with_existing_ifd1(true, true, true, false);
        let mut baseline = MetadataMap::new();
        baseline.insert("IFD1:Artist", TagValue::new_string("artist"));
        baseline.insert("IFD1:DocumentName", TagValue::new_string("doc"));
        let plan = crate::writers::generated_public_write::plan_public_write(
            &baseline,
            &MetadataMap::new(),
            &["IFD1:Artist".into(), "IFD1:DocumentName".into()],
        )
        .unwrap();
        // Both source-admitted entries share the generated IFD1 delete path.
        assert_eq!(plan.generated.len(), 2);
        assert!(!plan.has_legacy_changes);

        let output =
            write_public_exif_transaction(&TestReader::new(input), &baseline, plan).unwrap();
        let entries = ifd1_entries(&output);
        assert_eq!(entries.len(), 2);
        assert!(
            !entries
                .iter()
                .any(|entry| matches!(entry.tag_id, 0x010d | 0x013b))
        );
        assert_eq!(
            ifd1_entry(&entries, 0x0103),
            &crate::writers::exif_surgical::RawEntry {
                ifd: IfdKind::Ifd1,
                tag_id: 0x0103,
                field_type: 3,
                count: 1,
                value: vec![5, 0],
            }
        );
        assert_eq!(
            ifd1_entry(&entries, 0x011a),
            &crate::writers::exif_surgical::RawEntry {
                ifd: IfdKind::Ifd1,
                tag_id: 0x011a,
                field_type: 5,
                count: 1,
                value: [300_u32.to_le_bytes(), 1_u32.to_le_bytes()].concat(),
            }
        );
        // Existing non-default Compression/XResolution must not cause the
        // default YResolution or ResolutionUnit records to be injected.
        assert!(
            !entries
                .iter()
                .any(|entry| matches!(entry.tag_id, 0x011b | 0x0128))
        );
    }

    #[test]
    fn public_generated_only_ifd1_delete_omits_the_empty_directory() {
        let input = create_jpeg_with_existing_ifd1(false, true, false, false);
        let mut baseline = MetadataMap::new();
        baseline.insert("IFD1:DocumentName", TagValue::new_string("doc"));
        let plan = crate::writers::generated_public_write::plan_public_write(
            &baseline,
            &MetadataMap::new(),
            &["IFD1:DocumentName".into()],
        )
        .unwrap();
        assert_eq!(plan.generated.len(), 1);
        assert!(!plan.has_legacy_changes);
        let output =
            write_public_exif_transaction(&TestReader::new(input.clone()), &baseline, plan)
                .unwrap();
        assert!(
            !output
                .windows(EXIF_IDENTIFIER.len())
                .any(|window| window == EXIF_IDENTIFIER)
        );
        assert_eq!(output, without_exif_segment(&input));
    }

    #[test]
    fn public_artist_delete_prunes_native_fixed_width_mandatory_ifd1() {
        let input = create_jpeg_with_existing_ifd1(true, false, false, true);
        let mut baseline = MetadataMap::new();
        baseline.insert("IFD1:Artist", TagValue::new_string("artist"));
        let plan = crate::writers::generated_public_write::plan_public_write(
            &baseline,
            &MetadataMap::new(),
            &["IFD1:Artist".into()],
        )
        .unwrap();
        let output =
            write_public_exif_transaction(&TestReader::new(input.clone()), &baseline, plan)
                .unwrap();
        assert!(
            !output
                .windows(EXIF_IDENTIFIER.len())
                .any(|window| window == EXIF_IDENTIFIER)
        );
        assert_eq!(output, without_exif_segment(&input));
    }

    #[test]
    fn public_artist_only_ifd1_delete_omits_the_empty_directory() {
        let input = create_jpeg_with_existing_ifd1(true, false, false, false);
        let mut baseline = MetadataMap::new();
        baseline.insert("IFD1:Artist", TagValue::new_string("artist"));
        let plan = crate::writers::generated_public_write::plan_public_write(
            &baseline,
            &MetadataMap::new(),
            &["IFD1:Artist".into()],
        )
        .unwrap();
        assert_eq!(plan.generated.len(), 1);
        assert!(!plan.has_legacy_changes);
        let output =
            write_public_exif_transaction(&TestReader::new(input.clone()), &baseline, plan)
                .unwrap();
        assert!(
            !output
                .windows(EXIF_IDENTIFIER.len())
                .any(|window| window == EXIF_IDENTIFIER)
        );
        assert_eq!(output, without_exif_segment(&input));
    }

    /// Creates a JPEG with APP0 but no EXIF
    fn create_jpeg_with_app0() -> Vec<u8> {
        let mut data = Vec::new();

        // SOI
        data.extend_from_slice(&[0xFF, 0xD8]);

        // APP0 (JFIF)
        data.extend_from_slice(&[0xFF, 0xE0]);
        data.extend_from_slice(&[0x00, 0x06]); // Length: 6
        data.extend_from_slice(&[0x4A, 0x46, 0x49, 0x46]); // "JFIF"

        // EOI
        data.extend_from_slice(&[0xFF, 0xD9]);

        data
    }

    #[test]
    fn raw_scoped_jpeg_replacement_preserves_non_exif_bytes() {
        use crate::writers::exif_surgical::IfdKind;
        use crate::writers::tiff_surgical::entry_edits::{EntryMutation, ScopedEntryEdit};
        let mut file = vec![0xff, 0xd8];
        write_segment(&mut file, 0xffe0, b"JFIF\0prefix").unwrap();
        write_segment(
            &mut file,
            APP1_MARKER,
            b"http://ns.adobe.com/xap/1.0/\0keep",
        )
        .unwrap();
        let prefix_len = file.len();
        let mut exif = b"Exif\0\0II\x2a\0\x08\0\0\0".to_vec();
        exif.extend_from_slice(&[0; 6]); // Complete empty IFD0.
        write_segment(&mut file, APP1_MARKER, &exif).unwrap();
        let tail_start = file.len();
        write_segment(&mut file, SOS_MARKER, b"scan header").unwrap();
        file.extend_from_slice(b"\x12\xff\0\x34\xff\xd9trailer");
        let edit = ScopedEntryEdit {
            ifd: IfdKind::Ifd0,
            tag_id: 0x013c,
            mutation: EntryMutation::Set {
                field_type: 2,
                count: 6,
                bytes: b"Alpha\0".to_vec(),
            },
        };
        let out = apply_raw_exif_edits(&TestReader::new(file.clone()), std::slice::from_ref(&edit))
            .unwrap();
        assert_eq!(&out[..prefix_len], &file[..prefix_len]);
        assert!(out.ends_with(&file[tail_start..]));
        assert_eq!(
            apply_raw_exif_edits(&TestReader::new(out.clone()), &[edit]).unwrap(),
            out
        );
        let deleted = apply_raw_exif_edits(
            &TestReader::new(out),
            &[ScopedEntryEdit {
                ifd: IfdKind::Ifd0,
                tag_id: 0x013c,
                mutation: EntryMutation::Delete,
            }],
        )
        .unwrap();
        assert_eq!(&deleted[..prefix_len], &file[..prefix_len]);
        assert!(deleted.ends_with(&file[tail_start..]));
    }

    #[test]
    fn raw_scoped_jpeg_refuses_ambiguous_incomplete_and_oversized_exif() {
        use crate::writers::exif_surgical::IfdKind;
        use crate::writers::tiff_surgical::entry_edits::{EntryMutation, ScopedEntryEdit};
        let mut exif = b"Exif\0\0II\x2a\0\x08\0\0\0".to_vec();
        exif.extend_from_slice(&[0; 6]);
        for marker in [0xff00, 0xffff, SOI_MARKER, RST0_MARKER] {
            let mut malformed = vec![0xff, 0xd8];
            write_segment(&mut malformed, marker, b"bad prefix").unwrap();
            write_segment(&mut malformed, APP1_MARKER, &exif).unwrap();
            malformed.extend_from_slice(&[0xff, 0xd9]);
            assert!(
                apply_raw_exif_edits(&TestReader::new(malformed), &[]).is_err(),
                "must reject pre-scan marker {marker:#06x}"
            );
        }
        let mut file = vec![0xff, 0xd8];
        write_segment(&mut file, APP1_MARKER, &exif).unwrap();
        let mut incomplete = file.clone();
        incomplete.extend_from_slice(&[0xff, 0xe2, 0, 10, 0]);
        assert!(apply_raw_exif_edits(&TestReader::new(incomplete), &[]).is_err());
        let mut duplicate = file.clone();
        write_segment(&mut duplicate, APP1_MARKER, &exif).unwrap();
        duplicate.extend_from_slice(&[0xff, 0xd9]);
        assert!(apply_raw_exif_edits(&TestReader::new(duplicate), &[]).is_err());
        assert!(apply_raw_exif_edits(&TestReader::new(create_jpeg_without_exif()), &[]).is_err());
        file.extend_from_slice(&[0xff, 0xd9]);
        let err = apply_raw_exif_edits(
            &TestReader::new(file),
            &[ScopedEntryEdit {
                ifd: IfdKind::Ifd0,
                tag_id: 0x013c,
                mutation: EntryMutation::Set {
                    field_type: 2,
                    count: 65_530,
                    bytes: vec![0; 65_530],
                },
            }],
        )
        .unwrap_err();
        assert!(err.to_string().contains("APP1 size limit"));
    }

    #[test]
    fn generated_scalar_jpeg_preserves_other_segments_and_exact_scalar_states() {
        use crate::writers::generated_scalar::Scalar;
        use crate::writers::tiff_surgical::generated_scalar::{
            ScalarWriteRequest, generated_rules,
        };
        let mut original = vec![0xff, 0xd8];
        write_segment(&mut original, 0xffe2, b"unknown\0APP2 bytes").unwrap();
        write_segment(
            &mut original,
            APP1_MARKER,
            b"http://ns.adobe.com/xap/1.0/\0keep",
        )
        .unwrap();
        let prefix_len = original.len();
        let mut exif = b"Exif\0\0II\x2a\0\x08\0\0\0".to_vec();
        exif.extend_from_slice(&[0; 6]);
        write_segment(&mut original, APP1_MARKER, &exif).unwrap();
        let tail_start = original.len();
        write_segment(&mut original, SOS_MARKER, b"scan header").unwrap();
        original.extend_from_slice(b"\x12\xff\0\x34\xff\xd9trailer");
        let mut file = original.clone();
        for (value, expected) in [
            (Scalar::Utf8("é".to_owned()), Some(b"\xc3\xa9\0".as_slice())),
            (Scalar::Bytes(b"a\0b".to_vec()), Some(b"a\0b\0".as_slice())),
            (Scalar::Bytes(Vec::new()), Some(b"\0".as_slice())),
            (Scalar::Undefined, None),
        ] {
            let result = rewrite_generated_exif_scalars(
                &TestReader::new(file),
                vec![ScalarWriteRequest {
                    key: "EXIF:HostComputer",
                    value,
                }],
                &generated_rules(),
            )
            .unwrap();
            assert!(result.warnings.is_empty());
            assert_eq!(&result.bytes[..prefix_len], &original[..prefix_len]);
            assert!(result.bytes.ends_with(&original[tail_start..]));
            let tiff = crate::writers::exif_surgical_test_support::tiff_slice(&result.bytes);
            let scan = crate::writers::exif_surgical::scan_exif_entries(tiff).unwrap();
            let entry = scan.entries.iter().find(|entry| entry.tag_id == 0x013c);
            match expected {
                Some(value) => {
                    let entry = entry.unwrap();
                    assert_eq!(entry.field_type, 2);
                    assert_eq!(entry.count as usize, value.len());
                    assert_eq!(entry.value, value);
                }
                None => assert!(entry.is_none()),
            }
            file = result.bytes;
        }
    }

    #[test]
    fn generated_scalar_jpeg_refuses_missing_duplicate_and_oversized_blocks() {
        use crate::writers::generated_scalar::Scalar;
        use crate::writers::tiff_surgical::generated_scalar::{
            ScalarWriteRequest, generated_rules,
        };
        let mut file = vec![0xff, 0xd8];
        let mut exif = b"Exif\0\0II\x2a\0\x08\0\0\0".to_vec();
        exif.extend_from_slice(&[0; 6]);
        write_segment(&mut file, APP1_MARKER, &exif).unwrap();
        let mut duplicate = file.clone();
        write_segment(&mut duplicate, APP1_MARKER, &exif).unwrap();
        duplicate.extend_from_slice(&[0xff, 0xd9]);
        for rejected in [create_jpeg_without_exif(), duplicate] {
            assert!(
                rewrite_generated_exif_scalars(
                    &TestReader::new(rejected),
                    vec![ScalarWriteRequest {
                        key: "EXIF:HostComputer",
                        value: Scalar::Bytes(b"x".to_vec())
                    }],
                    &generated_rules()
                )
                .is_err()
            );
        }
        file.extend_from_slice(&[0xff, 0xd9]);
        let error = rewrite_generated_exif_scalars(
            &TestReader::new(file),
            vec![ScalarWriteRequest {
                key: "IFD0:HostComputer",
                value: Scalar::Bytes(vec![b'x'; 65_530]),
            }],
            &generated_rules(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("APP1 size limit"));
    }

    #[test]
    fn public_whole_clear_drops_exif_after_generated_partitioning() {
        let mut baseline = MetadataMap::new();
        baseline.insert("EXIF:HostComputer", TagValue::new_string("generated"));
        baseline.insert("IFD0:Artist", TagValue::new_string("legacy"));
        let plan = crate::writers::generated_public_write::plan_public_write(
            &baseline,
            &MetadataMap::new(),
            &[],
        )
        .unwrap();
        assert!(plan.whole_exif_clear);
        let bytes = write_public_exif_transaction(
            &TestReader::new(create_jpeg_with_exif()),
            &baseline,
            plan,
        )
        .unwrap();
        assert!(
            !parse_segments(&TestReader::new(bytes))
                .unwrap()
                .iter()
                .any(is_exif_segment)
        );
    }

    #[test]
    fn extended_exif_barrier_allows_no_second_tiff_to_be_misclassified() {
        let standard = Segment::new(APP1_MARKER, 0, b"Exif\0\0II*\0\x08\0\0\0");
        let continuation = Segment::new(APP1_MARKER, 0, b"Exif\0\0continuation");
        let repeated_tiff = Segment::new(APP1_MARKER, 0, b"Exif\0\0MM\0*\0\0\0\x08");
        assert!(is_extended_exif_continuation(true, &continuation));
        assert!(!is_extended_exif_continuation(true, &repeated_tiff));
        assert!(!is_extended_exif_continuation(false, &continuation));
        assert!(is_exif_segment(&standard));

        let mut extended_file = vec![0xff, 0xd8];
        write_segment(&mut extended_file, APP1_MARKER, standard.data).unwrap();
        write_segment(&mut extended_file, APP1_MARKER, continuation.data).unwrap();
        extended_file.extend_from_slice(&[0xff, 0xd9]);
        let extended_error = rewrite_generated_exif_scalars(
            &TestReader::new(extended_file),
            vec![
                crate::writers::tiff_surgical::generated_scalar::ScalarWriteRequest {
                    key: "EXIF:HostComputer",
                    value: crate::writers::generated_scalar::Scalar::Bytes(b"x".to_vec()),
                },
            ],
            &crate::writers::tiff_surgical::generated_scalar::generated_rules(),
        )
        .unwrap_err();
        assert!(extended_error.to_string().contains("ExtendedEXIF"));

        let mut repeated_file = vec![0xff, 0xd8];
        write_segment(&mut repeated_file, APP1_MARKER, standard.data).unwrap();
        write_segment(&mut repeated_file, APP1_MARKER, repeated_tiff.data).unwrap();
        repeated_file.extend_from_slice(&[0xff, 0xd9]);
        let repeated_error = rewrite_generated_exif_scalars(
            &TestReader::new(repeated_file),
            vec![
                crate::writers::tiff_surgical::generated_scalar::ScalarWriteRequest {
                    key: "EXIF:HostComputer",
                    value: crate::writers::generated_scalar::Scalar::Bytes(b"x".to_vec()),
                },
            ],
            &crate::writers::tiff_surgical::generated_scalar::generated_rules(),
        )
        .unwrap_err();
        assert!(repeated_error.to_string().contains("Ambiguous multiple"));
    }

    #[test]
    fn noncanonical_exif_cannot_bypass_the_fresh_directory_barrier() {
        for payload in [
            b"exif\0xII*\0data".as_slice(),
            b"junkExif\0\0data".as_slice(),
        ] {
            let segment = Segment::new(APP1_MARKER, 0, payload);
            assert!(has_unmodelled_exif_signature(&segment));
            let mut file = vec![0xff, 0xd8];
            write_segment(&mut file, APP1_MARKER, payload).unwrap();
            file.extend_from_slice(&[0xff, 0xd9]);
            let result: Result<(Vec<u8>, ())> = transform_exif(&TestReader::new(file), |_, _| {
                panic!("unmodelled directory must refuse before mutation")
            });
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("Noncanonical EXIF")
            );
        }
    }

    #[test]
    fn test_is_exif_segment() {
        // Create EXIF segment
        let exif_data = b"Exif\0\0test";
        let exif_seg = Segment::new(0xFFE1, 0, exif_data);
        assert!(is_exif_segment(&exif_seg));

        // Create XMP segment (also APP1, but different identifier)
        let xmp_data = b"http://ns.adobe.com/xap/1.0/\0test";
        let xmp_seg = Segment::new(0xFFE1, 0, xmp_data);
        assert!(!is_exif_segment(&xmp_seg));

        // Create non-APP1 segment
        let app0_seg = Segment::new(0xFFE0, 0, b"JFIF");
        assert!(!is_exif_segment(&app0_seg));
    }

    #[test]
    fn test_rewrite_jpeg_exif_builds_new_segment() {
        // No original EXIF segment: rewrite_jpeg_exif must still build a
        // fresh one from the desired map (the "insert new" path).
        let file_bytes = create_jpeg_without_exif();
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let result = crate::writers::exif_surgical::rewrite_jpeg_exif(&file_bytes, &metadata);
        assert!(result.is_ok());

        let segment_data = result.unwrap();

        // Should start with EXIF identifier
        assert_eq!(&segment_data[0..6], EXIF_IDENTIFIER);

        // Should have TIFF header
        assert_eq!(&segment_data[6..8], &[0x49, 0x49]); // Little-endian
        assert_eq!(&segment_data[8..10], &[0x2A, 0x00]); // Magic
        assert_eq!(&segment_data[10..14], &[0x08, 0x00, 0x00, 0x00]); // IFD offset

        // The segment must actually parse and contain the tag we asked for
        let tiff = &segment_data[EXIF_IDENTIFIER.len()..];
        let scan = crate::writers::exif_surgical::scan_exif_entries(tiff).unwrap();
        let make = scan
            .entries
            .iter()
            .find(|e| e.tag_id == 0x010F)
            .expect("Make tag must be present");
        assert_eq!(make.value, b"Canon\0");
    }

    #[test]
    fn test_write_segment() {
        let mut output = Vec::new();

        // Write APP1 segment
        let data = b"test data";
        write_segment(&mut output, 0xFFE1, data).unwrap();

        // Check marker
        assert_eq!(&output[0..2], &[0xFF, 0xE1]);

        // Check length (2 + 9 = 11)
        let reader = EndianReader::big_endian(&output);
        assert_eq!(reader.u16_at(2).unwrap_or(0), 11);

        // Check data
        assert_eq!(&output[4..13], data);
    }

    #[test]
    fn test_write_standalone_marker() {
        let mut output = Vec::new();

        // Write SOI (standalone, no length or data)
        write_segment(&mut output, SOI_MARKER, &[]).unwrap();

        // Should only have marker (2 bytes)
        assert_eq!(output.len(), 2);
        assert_eq!(&output, &[0xFF, 0xD8]);
    }

    #[test]
    fn test_write_exif_to_jpeg_replace_existing() {
        let jpeg = create_jpeg_with_exif();
        let reader = TestReader::new(jpeg);

        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Artist", TagValue::new_string("TestArtist"));

        let result = write_exif_to_jpeg(&reader, &metadata);
        assert!(result.is_ok());

        let modified_jpeg = result.unwrap();

        // Should still be valid JPEG
        assert_eq!(&modified_jpeg[0..2], &[0xFF, 0xD8]); // SOI
        assert_eq!(&modified_jpeg[modified_jpeg.len() - 2..], &[0xFF, 0xD9]); // EOI

        // Should have APP1 segment
        assert!(modified_jpeg.windows(2).any(|w| w == [0xFF, 0xE1]));

        // Parse and verify EXIF identifier is present
        let modified_reader = TestReader::new(modified_jpeg);
        let segments = parse_segments(&modified_reader).unwrap();
        let exif_seg = segments.iter().find(|s| is_exif_segment(s));
        assert!(exif_seg.is_some());
    }

    #[test]
    fn test_write_exif_to_jpeg_insert_new() {
        let jpeg = create_jpeg_without_exif();
        let reader = TestReader::new(jpeg);

        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));

        let result = write_exif_to_jpeg(&reader, &metadata);
        assert!(result.is_ok());

        let modified_jpeg = result.unwrap();

        // Should have SOI and EOI
        assert_eq!(&modified_jpeg[0..2], &[0xFF, 0xD8]);
        assert_eq!(&modified_jpeg[modified_jpeg.len() - 2..], &[0xFF, 0xD9]);

        // Should have new APP1 segment
        let modified_reader = TestReader::new(modified_jpeg);
        let segments = parse_segments(&modified_reader).unwrap();
        let exif_seg = segments.iter().find(|s| is_exif_segment(s));
        assert!(exif_seg.is_some());
    }

    #[test]
    fn test_write_exif_to_jpeg_insert_after_app0() {
        let jpeg = create_jpeg_with_app0();
        let reader = TestReader::new(jpeg);

        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Model", TagValue::new_string("EOS"));

        let result = write_exif_to_jpeg(&reader, &metadata);
        assert!(result.is_ok());

        let modified_jpeg = result.unwrap();

        // Parse segments
        let modified_reader = TestReader::new(modified_jpeg);
        let segments = parse_segments(&modified_reader).unwrap();

        // Should have: SOI, APP0, APP1 (EXIF), EOI
        assert_eq!(segments.len(), 4);
        assert_eq!(segments[0].marker, 0xFFD8); // SOI
        assert_eq!(segments[1].marker, 0xFFE0); // APP0
        assert_eq!(segments[2].marker, 0xFFE1); // APP1 (EXIF)
        assert!(is_exif_segment(&segments[2]));
        assert_eq!(segments[3].marker, 0xFFD9); // EOI
    }

    #[test]
    fn test_is_standalone_marker() {
        assert!(is_standalone_marker(SOI_MARKER));
        assert!(is_standalone_marker(EOI_MARKER));
        assert!(is_standalone_marker(0xFFD0)); // RST0
        assert!(is_standalone_marker(0xFFD7)); // RST7

        assert!(!is_standalone_marker(0xFFE1)); // APP1
        assert!(!is_standalone_marker(0xFFE0)); // APP0
    }
}
