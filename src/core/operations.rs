//! Metadata operations (Read/Write/Copy/Transform)
//!
//! This module defines core operations for metadata manipulation.
//! It orchestrates format detection, parser selection, and metadata extraction
//! following the hexagonal architecture pattern.

use super::{FileFormat, FileReader, Instance, MetadataMap, TagValue};
use crate::core::file_metadata::UNKNOWN_MIME_TYPE;
use crate::core::format_dispatch::dispatch_format_parser;
use crate::core::jpeg_helpers::{
    extract_direct_preview_image, process_app3_segments, process_app6_segments,
    process_app10_segments, process_app11_segments, process_app12_segments, process_app14_segments,
    process_app15_segments, process_com_segments, process_dji_dbg_segments,
    process_dji_thermal_segments, process_dqt_segments_with_options,
    process_exif_segments_with_options, process_icc_segments, process_infiray_segments,
    process_iptc_segments, process_jfif_segments, process_media_jukebox_segments,
    process_mpf_segments, process_photoshop_segments, process_qualcomm_segments,
    process_ricoh_rmeta_segments, process_samsung_unique_id_segments,
    process_sof_segments_with_options, process_spiff_segments,
    process_uniform_resource_name_segments, process_xmp_segments,
};
use crate::core::operations_helpers::{read_u16, read_u32};
use crate::core::read_options::ReadOptions;
use crate::core::read_report::{
    Diagnostic, DiagnosticKind, DiagnosticSink, ParseStatus, ReadReport,
};
#[cfg(test)]
use crate::core::tag_conversion::raw_bytes_to_tag_value;
use crate::core::tag_occurrence::ValueChannel;
use crate::core::tiff_helpers::parse_ifd_chain_with_options;
use crate::core::validation::{validate_tag_value_intrinsics, validate_tag_value_with_name};
use crate::core::write_transaction::WriteOutcome;
use crate::error::{ExifToolError, Result};
use crate::io::MMapReader;
use crate::parsers::DetectorMode;
use crate::parsers::detection::detect_format;
use crate::parsers::jpeg::segment_parser::parse_segments;
use crate::parsers::tiff::ifd_parser::ByteOrder;
#[cfg(test)]
use crate::parsers::tiff::tiff_subreader::TiffSubReader;
use crate::tag_db::tag_registry::{get_tag_descriptor, has_reliable_value_type};
use crate::writers::atomic_writer::write_atomic;
use crate::writers::pdf_writer::write_pdf_file;
use crate::writers::png_writer::write_png_metadata_with_removals;
use std::path::Path;

// ============================================================================
// SECTION 1: PUBLIC API FUNCTIONS
// ============================================================================

/// Reads metadata from a file at the specified path.
///
/// This function orchestrates the complete metadata extraction workflow:
/// 1. Opens file with MMapReader (zero-copy memory-mapped access)
/// 2. Detects file format via magic bytes
/// 3. Selects and invokes appropriate format parser
/// 4. Parses raw metadata to MetadataMap
/// 5. Enriches metadata with tag descriptors from registry
///
/// # Arguments
///
/// * `path` - Path to the file to read metadata from
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(ExifToolError)` - I/O error, unsupported format, or parse error
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::operations::read_metadata;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let metadata = read_metadata(Path::new("photo.jpg"))?;
///
/// // Access typed metadata
/// if let Some(make) = metadata.get_string("EXIF:Make") {
///     println!("Camera: {}", make);
/// }
/// if let Some(iso) = metadata.get_integer("EXIF:ISO") {
///     println!("ISO: {}", iso);
/// }
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - File cannot be opened or read (IoError)
/// - File format is unsupported (UnsupportedFormat)
/// - File contains invalid or truncated metadata (ParseError)
pub fn read_metadata(path: &Path) -> Result<MetadataMap> {
    read_metadata_with_detector(path, DetectorMode::Signature)
}

/// Whether a failed read should fall back to bare identification.
///
/// Only `UnsupportedFormat` qualifies: that means "no parser for this", and the
/// file is still worth identifying. A `ParseError` means the file *is* a format
/// we handle and is malformed, and must stay an error -- downgrading it to a
/// successful read with three identity tags would report a corrupt document as
/// fine, which is worse than failing.
pub(crate) fn is_unsupported(e: &ExifToolError) -> bool {
    matches!(e, ExifToolError::UnsupportedFormat { .. })
}

/// Whether a `File:` identity value is `extract_file_metadata`'s optimistic
/// placeholder rather than a real answer.
///
/// `extract_file_metadata` resolves these before the format is known, so a
/// file whose extension answers for neither arrives here carrying "Unknown"
/// and [`UNKNOWN_MIME_TYPE`].
///
/// `application/octet-stream` stays on the list even though it is now no
/// longer the fallback. It remains a *weak* answer: `extract_file_metadata`
/// can reach it from the root type of a sub-type `%mimeType` does not carry,
/// and `add_identity_tags` should still be free to improve on that from the
/// header. Where it is the real answer -- DR4, VRD, LNK, MOI, the EXE family
/// -- the header-derived value agrees, so replacing it changes nothing.
fn is_placeholder(v: Option<&str>) -> bool {
    matches!(
        v,
        None | Some("")
            | Some("Unknown")
            | Some("unknown")
            | Some("application/octet-stream")
            | Some(UNKNOWN_MIME_TYPE)
    )
}

/// Drop a parser's ungrouped copy of the file size.
///
/// `extract_file_metadata` owns `File:FileSize` and renders it the way ExifTool
/// prints it -- `"785 bytes"`. 41 parsers *also* record `reader.size()` under a
/// bare `FileSize`, and the text parser mirrored that into `TEXT:FileSize`, so
/// one fact reached the output under three keys carrying two different
/// spellings: `"File:FileSize": "785 bytes"` beside `"FileSize": "785"`.
/// ExifTool emits one. On ExifTool's own `t/images` corpus 51 of 194 files
/// showed the duplicate, 20 of them all three keys at once.
///
/// Only the exactly-ungrouped key is removed, and only once the authoritative
/// `File:FileSize` is present -- absent that, dropping the parser's value would
/// trade a badly-formatted answer for no answer.
///
/// Grouping is not by itself proof of a distinct fact, so this does not try to
/// judge grouped keys. The ones in the corpus divide both ways and each was
/// checked against the oracle rather than inferred from its name: `XML:FileSize`
/// is a size recorded *inside* an XML document, `File:DPXFileSize` is the length
/// the DPX header declares (12812288 against a 2.1 kB file), `Prefetch:FileSize`
/// comes from the prefetch header and `LNK:TargetFileSize` describes a
/// shortcut's target -- all real, all left alone. `EXE:FileSize` was the
/// opposite: `reader.size()` again, under a group prefix that hid it from the
/// `insert("FileSize"` search, and a tag ExifTool emits for no Mach-O. It was
/// removed at its source in the Mach-O parser instead of here.
///
/// [`normalize_identity_tags`] is the sibling of this for `FileType`,
/// `FileTypeExtension` and `MIMEType`, and stays separate because the two cases
/// differ in kind. There, a parser's ungrouped string is a rival *answer* to
/// "what is this file?", so that function has to arbitrate, and a parser can
/// still name a type the tables left `Unknown`. Here there is nothing to
/// arbitrate: both keys report the same byte count, and the parser's spelling of
/// it is simply the unformatted one, so it can never fill a gap in
/// `File:FileSize` the way a parser's `FileType` can.
fn drop_redundant_file_size(metadata: &mut MetadataMap) {
    if metadata.contains_key("File:FileSize") {
        metadata.remove("FileSize");
    }
}

/// Fill in `FileType`, `FileTypeExtension` and `MIMEType` from ExifTool's
/// identification tables. Returns whether the file was recognised.
///
/// Identifying a file is independent of being able to read its contents, so
/// this runs on **every** read, not only when no parser matched. It used to be
/// reachable only from the `UnsupportedFormat` arms below, which meant a format
/// the dispatcher *does* recognise never got here -- and the optimistic values
/// `extract_file_metadata` leaves behind come from a ~50-extension hand-written
/// table. The result was 67 corpus files parsing their contents perfectly while
/// reporting `FileType: Unknown` and `MIMEType: application/octet-stream`:
/// ICC_Profile.icc (35 tags), Photoshop.psd (111 tags), Font.ttf (50 tags),
/// every .xmp, .json, .csv and .plist in the corpus.
///
/// A real answer from a parser is never replaced. Only placeholders are
/// filled, and the canonical extension is corrected only when the file type we
/// resolved is the one being reported -- otherwise a parser that knows better
/// than the extension (a `.m4a` that is really MOV) would get a contradictory
/// pair.
fn add_identity_tags(metadata: &mut MetadataMap, reader: &dyn FileReader, path: &Path) -> bool {
    // 1 KiB is what the magic-number patterns are written against.
    let want = reader.size().min(1024) as usize;
    let header = reader.read(0, want).unwrap_or_default();
    let ext = path.extension().and_then(|e| e.to_str());

    let Some(id) = crate::filetype::identify(&header, ext) else {
        return false;
    };

    let reported = metadata.get_string("File:FileType");
    let ours_is_authoritative = is_placeholder(reported) || reported == Some(id.file_type.as_ref());
    let suppress_producer = crate::exiftool_tables::attribution::silenced(
        crate::exiftool_tables::attribution::Token::Producers,
    );
    if is_placeholder(reported) && !suppress_producer {
        metadata.insert("File:FileType", TagValue::new_string(id.file_type.as_ref()));
    }

    // The on-disk extension is a placeholder whenever it disagrees with
    // ExifTool's canonical one (`aif` where ExifTool says `aiff`), which is
    // exactly when it should be corrected.
    if ours_is_authoritative && !suppress_producer {
        let current = metadata.get_string("File:FileTypeExtension");
        if is_placeholder(current) || current != Some(id.extension.as_ref()) {
            metadata.insert(
                "File:FileTypeExtension",
                TagValue::new_string(id.extension.as_ref()),
            );
        }
    }

    if let Some(mime) = id.mime_type
        && ours_is_authoritative
        && !suppress_producer
        && is_placeholder(metadata.get_string("File:MIMEType"))
    {
        metadata.insert("File:MIMEType", TagValue::new_string(mime));
    }

    // A MIE file's own MIME type is generic; ExifTool sharpens it to name the
    // subfile the container wraps (`application/x-mie-jpeg`, not just
    // `application/x-mie`), which `%mimeType` cannot express as a table row --
    // it is assembled at read time from the file's own top-level tags. See
    // `mie::document_mime_type` for the derivation.
    //
    // Bounded to 4 MiB: real `.mie` files carry their identifying tags in the
    // first few dozen bytes, and this exists to sharpen a MIME type, not to
    // extract the file -- a multi-gigabyte MIE should not be read whole for
    // that.
    if id.file_type == "MIE"
        && ours_is_authoritative
        && let Ok(whole) = reader.read(0, (reader.size() as usize).min(4 << 20))
        && let Some(mime) = crate::parsers::mie::document_mime_type(whole)
    {
        metadata.insert("File:MIMEType", TagValue::new_string(mime));
    }

    // RealMedia's own MIME type is generic too, and is overridden the same
    // way when the file wraps exactly one stream -- ExifTool reports that
    // stream's own MIME type instead. See `real::single_stream_mime_type`.
    // RM, RV and RMVB share the format and the one Perl function that reads
    // it, so all three are covered even though only RM is in the corpus.
    if matches!(id.file_type.as_ref(), "RM" | "RV" | "RMVB")
        && ours_is_authoritative
        && let Ok(whole) = reader.read(0, (reader.size() as usize).min(4 << 20))
        && let Some(mime) = crate::parsers::real::single_stream_mime_type(whole)
    {
        metadata.insert("File:MIMEType", TagValue::new_string(mime));
    }
    true
}

/// The three tags that answer "what is this file?".
const IDENTITY_TAGS: [&str; 3] = ["FileType", "FileTypeExtension", "MIMEType"];

/// Leave exactly one answer per identity tag, in the group ExifTool uses.
///
/// ExifTool emits `FileType`, `FileTypeExtension` and `MIMEType` once each,
/// under `File`. Roughly forty parsers insert them ungrouped as well, so any
/// file that reached a parser carried two answers to the same question -- and
/// on 21 of the 194 files in ExifTool's own `t/images` the two disagreed.
/// `Geotag.log` reported `File:FileType "TXT"` beside a bare `FileType "TXT"`;
/// `Font.dfont` reported `File:FileType "DFONT"` beside a bare `FileType
/// "ICO"`, left there by the ICO parser, which the file reaches because
/// ExifTool's `Font` magic number matches anything starting `\0\x01`. Nothing
/// downstream could say which of the two was meant.
///
/// The `File:` value is the one kept. [`add_identity_tags`] and
/// `extract_file_metadata` both resolve it from `crate::filetype`, which is
/// generated from ExifTool's `%fileTypeLookup`, `%fileTypeExt` and `%mimeType`;
/// a parser's own string is its private spelling of the same fact at best
/// (`WebP` where ExifTool says `WEBP`, `Plist` where it says `PLIST`) and a
/// loose magic match at worst. Across those 21 disagreements the `File:` value
/// is ExifTool's answer 20 times.
///
/// The one thing a parser can still contribute is a name where the tables
/// produced none, so a bare `FileType` fills an absent or `Unknown` one before
/// being dropped. `MIMEType` is deliberately not treated the same way:
/// `application/octet-stream` reads like a placeholder but is ExifTool's real
/// answer for DR4, VRD, LNK, MOI and the Mach-O family, and overwriting it with
/// a parser's guess would replace a correct value rather than fill a gap.
fn normalize_identity_tags(metadata: &mut MetadataMap) {
    let parser_type = metadata.remove("FileType");
    metadata.remove("FileTypeExtension");
    metadata.remove("MIMEType");

    let unnamed = matches!(
        metadata.get_string("File:FileType"),
        None | Some("") | Some("Unknown") | Some("unknown")
    );
    if unnamed && let Some(name) = parser_type.as_ref().and_then(TagValue::as_string) {
        metadata.insert("File:FileType", TagValue::new_string(name));
    }

    debug_assert!(
        IDENTITY_TAGS.iter().all(|t| !metadata.contains_key(*t)),
        "identity tags must be emitted only under the File group"
    );
}

/// Detect file format using the specified detection mode.
///
/// This helper function wraps format detection to support both signature-based
/// and AI-powered (Magika) detection methods.
fn detect_format_with_mode(reader: &dyn FileReader, mode: DetectorMode) -> Result<FileFormat> {
    match mode {
        DetectorMode::Signature => {
            // Convert io::Error to ExifToolError
            detect_format(reader).map_err(ExifToolError::from)
        }
        #[cfg(feature = "magika")]
        DetectorMode::Magika => {
            use crate::parsers::magika_detector::detect_with_magika;
            let size = reader.size();
            let data = reader.read(0, size as usize)?;
            // Convert io::Error to ExifToolError
            detect_with_magika(&data).map_err(ExifToolError::from)
        }
        #[cfg(not(feature = "magika"))]
        DetectorMode::Magika => Err(ExifToolError::unsupported_format(
            "Magika AI detection not available (build with --features magika)",
        )),
    }
}

/// Reads metadata from a file with specified detection mode.
///
/// This function extends `read_metadata` to support both signature-based and
/// AI-powered file format detection. Use this when you want to enable Magika
/// AI detection via the `--detector=magika` CLI flag.
///
/// # Arguments
///
/// * `path` - Path to the file to analyze
/// * `detector_mode` - Detection mode (Signature or Magika)
///
/// # Returns
///
/// A MetadataMap containing all extracted metadata
pub fn read_metadata_with_detector(
    path: &Path,
    detector_mode: DetectorMode,
) -> Result<MetadataMap> {
    read_metadata_with_detector_and_options(
        path,
        detector_mode,
        &ReadOptions::default_full_listing(),
    )
}

/// [`read_metadata_with_detector`], with Step 21's request-awareness
/// (`ReadOptions`) exposed. This is the entry point the CLI uses so that a
/// specifically-requested `-JPEGQualityEstimate` or `--extended-output` read
/// actually reaches the JPEG parser; every other caller (library
/// consumers, the surgical writer's original/desired diff, every
/// pre-existing test) keeps using [`read_metadata_with_detector`] or
/// [`read_metadata`], which default to
/// [`ReadOptions::default_full_listing`] and so observe no behavior change
/// from this step.
pub fn read_metadata_with_detector_and_options(
    path: &Path,
    detector_mode: DetectorMode,
    options: &ReadOptions,
) -> Result<MetadataMap> {
    // Everything recorded by the read is the file's own; what a caller
    // inserts or mutates afterwards is an assignment
    // (`MetadataMap::is_assigned`).
    read_metadata_unmarked(path, detector_mode, options).map(|mut metadata| {
        metadata.mark_read_complete();
        metadata.set_read_source(path);
        metadata
    })
}

fn read_metadata_unmarked(
    path: &Path,
    detector_mode: DetectorMode,
    options: &ReadOptions,
) -> Result<MetadataMap> {
    // Step 1: Extract file system metadata (File:FileName, File:FileSize, etc.)
    // This is done first and independently of the file format
    let mut metadata = match crate::core::file_metadata::extract_file_metadata(path) {
        Ok(file_meta) => file_meta,
        Err(e) => {
            // If we can't get file metadata, log a warning but continue
            eprintln!("Warning: Failed to extract file metadata: {}", e);
            MetadataMap::new()
        }
    };

    // Step 2: Open file with MMapReader for zero-copy access
    let reader = MMapReader::new(path)?;

    // Step 3: Detect format using specified detector mode
    //
    // A format we cannot parse is not the same as a file we cannot recognise.
    // ExifTool still reports FileType/FileTypeExtension/MIMEType for AIFF, DPX,
    // SWF and ~40 other formats OxiDex has no parser for; returning Err here
    // meant emitting nothing at all for those files, including the file-system
    // metadata already gathered above. Identify what we can and return that.
    let mut format = match detect_format_with_mode(&reader, detector_mode) {
        Ok(f) => f,
        Err(e) => {
            if is_unsupported(&e) && add_identity_tags(&mut metadata, &reader, path) {
                crate::composite::apply(&mut metadata);
                return Ok(metadata);
            }
            return Err(e);
        }
    };

    // Step 3b: Check for camera raw formats using filename + magic bytes
    // Many raw formats are TIFF-based and need filename context for proper detection
    // (e.g., DNG, NEF, ARW all have TIFF magic bytes but different file extensions)
    if format == FileFormat::TIFF {
        // Get filename for raw format detection
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Read first 32 bytes for raw format detection
        if let Ok(magic_bytes) = reader.read(0, 32) {
            // Check if this is a camera raw format
            if let Some(raw_format) = crate::parsers::raw::detect_raw_format(magic_bytes, filename)
            {
                // Override TIFF detection with specific raw format
                format = FileFormat::CameraRaw(raw_format);
            }
        }
    }

    // Step 3c: An MPC file carrying a leading ID3v2 tag opens with the same
    // three bytes ("ID3") as a plain MP3, so `detect_format` -- which never
    // sees a filename -- resolves it to `FileFormat::MP3`. Real ExifTool
    // does not have this ambiguity: it assigns FileType from the `.mpc`
    // extension directly (a single-candidate `%fileTypeExt` lookup) and
    // dispatches to `MPC::ProcessMPC`, which is what finds the real `MP+`
    // signature 263 bytes in, behind the ID3v2 tag (`MPC.pm:79-116`,
    // `ID3.pm:1691-1698`). A file with no leading ID3 tag needs no override:
    // it opens directly with `MP+` and the `signature!` table in
    // `parsers/detection/signatures.rs` already resolves it to
    // `FileFormat::MPC` unaided.
    if format == FileFormat::MP3
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("mpc"))
    {
        format = FileFormat::MPC;
    }

    // Step 3d: CSV is an extension-assigned type, exactly as in ExifTool:
    // `ProcessTXT` computes Delimiter/Quoting/ColumnCount/RowCount only when
    // `$$et{FileType} eq 'CSV'`, and that FileType comes from the `.csv`
    // extension's `%fileTypeLookup` entry (`CSV => ['TXT', 'Comma-Separated
    // Values']`, ExifTool.pm) -- the content alone is indistinguishable from
    // plain text, which is why `detect_format` (which never sees a filename)
    // resolves it to TXT. A `.txt` file with identical bytes stays TXT and
    // keeps LineCount/WordCount, matching the oracle on both.
    if format == FileFormat::TXT
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
    {
        format = FileFormat::CSV;
    }

    // Step 4: Route to appropriate parser based on detected format and extract format-specific metadata
    //
    // Detection returning Unknown, or a parser refusing the file, still leaves
    // it identifiable: ExifTool reports FileType/FileTypeExtension/MIMEType for
    // ~40 formats OxiDex has no parser for. Failing the whole read there threw
    // away the file-system metadata too, so those files produced no output at
    // all rather than partial output.
    let format_metadata = match dispatch_format_parser(&reader, format, options) {
        Ok(m) => m,
        Err(e) => {
            if is_unsupported(&e) && add_identity_tags(&mut metadata, &reader, path) {
                crate::composite::apply(&mut metadata);
                return Ok(metadata);
            }
            return Err(e);
        }
    };

    // Step 5: Merge format-specific metadata into file metadata
    // Format-specific metadata takes precedence over file metadata in case of conflicts
    // Use into_iter() to consume format_metadata and avoid cloning keys and values
    metadata.merge(format_metadata);

    // Step 5a0: One key per fact for file size. The merge above is the only
    // place a parser's tags enter, so this is the one point that sees both the
    // authoritative `File:FileSize` and a parser's ungrouped duplicate.
    drop_redundant_file_size(&mut metadata);

    // Step 5a: Identity tags, from ExifTool's own tables.
    //
    // This used to run only when parsing failed, which made a working parser
    // and a correct `File:FileType` mutually exclusive: LNK, EXR and ICC files
    // parsed fine and still reported `FileType: Unknown`. Only placeholders are
    // filled, so a parser that names the type itself still wins.
    add_identity_tags(&mut metadata, &reader, path);

    // Step 5a': One answer per identity tag, under the group ExifTool uses.
    //
    // Runs after Step 5a, not before: the tables get first refusal on naming
    // the file, and only what they decline is filled from the parser's own
    // ungrouped copy before that copy is dropped.
    normalize_identity_tags(&mut metadata);

    // Step 5b: Backstop for ExifByteOrder on TIFF-based files.
    //
    // The JPEG path records this when it parses the APP1 TIFF header, but the
    // raw formats (CR2, DNG, NEF, ...) reach their IFDs through a dozen
    // different entry points in the raw parsers. Every TIFF-based file starts
    // with the marker, so reading it here covers all of them at once instead
    // of threading the same insert through each parser.
    //
    // DR4 is excluded: its magic number *is* `IIII`, a Canon byte-order marker
    // for the recipe directory rather than a TIFF header, and ExifTool reports
    // no ExifByteOrder for it.
    //
    // BigTIFF is excluded for a different reason: it *does* have a TIFF byte
    // order marker, but ExifTool branches to `BigTIFF::ProcessBTF` at
    // ExifTool.pm:8661-8665 and returns at :8667, never reaching the
    // `FoundTag('ExifByteOrder', ...)` at :8702 that every other TIFF-based
    // file goes through. The pinned 13.59 oracle agrees: `-ExifByteOrder` on
    // `BigTIFF.btf` prints nothing where `ExifTool.tif` prints
    // `Big-endian (Motorola, MM)`.
    if !metadata.contains_key("File:ExifByteOrder")
        && format != FileFormat::DR4
        && format != FileFormat::BigTIFF
    {
        if let Ok(head) = reader.read(0, 2) {
            let order = match &head[..] {
                b"II" => Some(ByteOrder::LittleEndian),
                b"MM" => Some(ByteOrder::BigEndian),
                _ => None,
            };
            if let Some(order) = order {
                metadata.insert(
                    "File:ExifByteOrder",
                    TagValue::new_string(order.exif_byte_order_tag()),
                );
            }
        }
    }

    // Step 6: Derive ExifTool's Composite tags (ImageSize, Megapixels,
    // Aperture, ShutterSpeed, ...). These are computed from tags already
    // extracted above, so this runs last and never overwrites a parsed value.
    crate::composite::apply(&mut metadata);

    Ok(metadata)
}

/// Reads metadata from a file, returning a [`ReadReport`] rather than a bare
/// `Result<MetadataMap>`.
///
/// This is the machine-readable counterpart to [`read_metadata`]. Where
/// `read_metadata` either hands back a full `MetadataMap` or fails the whole
/// read, `read_metadata_report` always hands back whatever could be
/// extracted -- filesystem tags at a minimum -- tagged with a [`ParseStatus`]
/// that says how far the read got, plus the [`Diagnostic`]s a caller can
/// inspect programmatically instead of grepping stderr.
///
/// See [`read_metadata_report_with_detector`] for the full behavior; this is
/// that function fixed to [`DetectorMode::Signature`].
pub fn read_metadata_report(path: &Path) -> Result<ReadReport> {
    read_metadata_report_with_detector(path, DetectorMode::Signature)
}

/// [`read_metadata_report`], with the detector mode exposed.
///
/// This mirrors [`read_metadata_with_detector`] step for step through
/// filesystem-metadata extraction, format detection, and dispatch, but
/// diverges at two points where the older function had no choice but to
/// fail the whole read:
///
/// * **A recognised format whose parser cannot complete** (a truncated
///   JPEG, a damaged sub-block) used to return `Err`, discarding the
///   filesystem metadata already gathered. ExifTool does not do this:
///   `ProcessJPEG` clears its `$success` flag on a bad segment but keeps
///   walking the file, and only afterwards does `$success or
///   $self->Warn('JPEG format error')` (`ExifTool.pm:8483`) turn that into
///   a `Warning` tag rather than an exception -- `Warn` itself
///   (`ExifTool.pm:5616-5643`) is just `FoundTag('Warning', $str)`, a
///   warning is another extracted tag, not a distinct failure channel. This
///   function does the same: on such a failure it still returns filesystem
///   + identity tags, records the problem as a `Diagnostic`, mirrors it
///   into a `File:Warning` tag, and reports [`ParseStatus::Partial`].
/// * **A format neither a parser nor `crate::filetype::identify` can name**
///   used to return `Err` too. This function instead reports
///   [`ParseStatus::Unsupported`] with the filesystem tags it already had
///   and a diagnostic explaining why.
///
/// The genuinely successful paths are unchanged in substance: a full parse
/// with nothing pushed to the diagnostic sink is [`ParseStatus::Parsed`]; a
/// full parse that pushed at least one diagnostic (a malformed embedded XMP
/// packet, say, in an otherwise-healthy JPEG) is [`ParseStatus::Partial`];
/// and the pre-existing "detected but not parsed" fallback --
/// [`add_identity_tags`] reached because the format has no parser at all --
/// is [`ParseStatus::IdentifiedOnly`]. AGENTS.md calls that state "detected
/// is not parsed": a file can report a perfectly correct `FileType` while
/// 100% of its real tags are missing, and `IdentifiedOnly` is what makes
/// that machine-distinguishable from an actual parse.
///
/// Diagnostic collection only runs through JPEG and PNG today (the two
/// parsers this step threaded a sink into); every other format's parser
/// still resolves internally the way it always did; a hard failure from one
/// of them lands on the "recognised format whose parser cannot complete"
/// branch above with a single diagnostic built from the propagated error,
/// same as before this step, just no longer thrown away as an `Err`.
pub fn read_metadata_report_with_detector(
    path: &Path,
    detector_mode: DetectorMode,
) -> Result<ReadReport> {
    read_metadata_report_with_detector_and_options(
        path,
        detector_mode,
        &ReadOptions::default_full_listing(),
    )
}

/// [`read_metadata_report_with_detector`], with Step 21's request-awareness
/// (`ReadOptions`) exposed. See
/// [`read_metadata_with_detector_and_options`]'s doc comment -- the same
/// reasoning applies here.
pub fn read_metadata_report_with_detector_and_options(
    path: &Path,
    detector_mode: DetectorMode,
    options: &ReadOptions,
) -> Result<ReadReport> {
    // As `read_metadata_with_detector_and_options`: the read's occurrences
    // are the file's, later insertions are assignments.
    read_metadata_report_unmarked(path, detector_mode, options).map(|mut report| {
        report.metadata.mark_read_complete();
        report.metadata.set_read_source(path);
        report
    })
}

fn read_metadata_report_unmarked(
    path: &Path,
    detector_mode: DetectorMode,
    options: &ReadOptions,
) -> Result<ReadReport> {
    // Step 1: Extract file system metadata (File:FileName, File:FileSize, etc.)
    let mut metadata = match crate::core::file_metadata::extract_file_metadata(path) {
        Ok(file_meta) => file_meta,
        Err(e) => {
            eprintln!("Warning: Failed to extract file metadata: {}", e);
            MetadataMap::new()
        }
    };

    // Step 2: Open file with MMapReader for zero-copy access
    let reader = MMapReader::new(path)?;

    // Step 3: Detect format
    let mut format = match detect_format_with_mode(&reader, detector_mode) {
        Ok(f) => f,
        Err(e) => return Ok(identify_or_report_unsupported(metadata, &reader, path, e)),
    };

    // Step 3b: Camera raw formats hiding behind a TIFF magic number
    if format == FileFormat::TIFF {
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if let Ok(magic_bytes) = reader.read(0, 32)
            && let Some(raw_format) = crate::parsers::raw::detect_raw_format(magic_bytes, filename)
        {
            format = FileFormat::CameraRaw(raw_format);
        }
    }

    // Step 3c: see the identical override's comment in
    // `read_metadata_with_detector_and_options`, above -- both entry points
    // detect format independently, so both need it.
    if format == FileFormat::MP3
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("mpc"))
    {
        format = FileFormat::MPC;
    }

    // Step 3d: CSV is an extension-assigned type, exactly as in ExifTool:
    // `ProcessTXT` computes Delimiter/Quoting/ColumnCount/RowCount only when
    // `$$et{FileType} eq 'CSV'`, and that FileType comes from the `.csv`
    // extension's `%fileTypeLookup` entry (`CSV => ['TXT', 'Comma-Separated
    // Values']`, ExifTool.pm) -- the content alone is indistinguishable from
    // plain text, which is why `detect_format` (which never sees a filename)
    // resolves it to TXT. A `.txt` file with identical bytes stays TXT and
    // keeps LineCount/WordCount, matching the oracle on both.
    if format == FileFormat::TXT
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
    {
        format = FileFormat::CSV;
    }

    // Step 4: Dispatch. JPEG and PNG go through their diagnostics-carrying
    // entry points so a recoverable problem inside an otherwise-successful
    // parse surfaces as `Partial` rather than vanishing into stderr.
    let mut diagnostics: DiagnosticSink = Vec::new();
    let dispatch_result = match format {
        FileFormat::JPEG => {
            parse_jpeg_metadata_with_diagnostics(&reader, &mut diagnostics, options)
        }
        FileFormat::PNG => {
            crate::parsers::png::parse_png_metadata_with_diagnostics(&reader, &mut diagnostics)
        }
        _ => dispatch_format_parser(&reader, format, options),
    };

    let format_metadata = match dispatch_result {
        Ok(m) => m,
        Err(e) => {
            if is_unsupported(&e) {
                return Ok(identify_or_report_unsupported(metadata, &reader, path, e));
            }
            // A format we do have a parser for, but that parser could not
            // finish. Keep what identification gives us instead of failing
            // outright -- see the ExifTool.pm citations on this function's
            // doc comment.
            return Ok(if add_identity_tags(&mut metadata, &reader, path) {
                // JPEG gets ExifTool's own wording verbatim
                // (`$self->Warn('JPEG format error')`, ExifTool.pm:8483) so
                // the two tools agree byte-for-byte on a truncated JPEG.
                // Every other format keeps the real error text -- there is
                // no equivalent fixed phrase to match, and the actual
                // message is more useful than inventing one.
                let message = if format == FileFormat::JPEG {
                    "JPEG format error".to_string()
                } else {
                    e.to_string()
                };
                let diagnostics = vec![Diagnostic::warning(message)];
                record_diagnostics(&mut metadata, &diagnostics);
                normalize_identity_tags(&mut metadata);
                crate::composite::apply(&mut metadata);
                ReadReport {
                    metadata,
                    status: ParseStatus::Partial,
                    diagnostics,
                }
            } else {
                let diagnostics = vec![Diagnostic::warning(e.to_string())];
                record_diagnostics(&mut metadata, &diagnostics);
                ReadReport {
                    metadata,
                    status: ParseStatus::Unsupported,
                    diagnostics,
                }
            });
        }
    };

    // Step 5: Merge, same as `read_metadata_with_detector`.
    metadata.merge(format_metadata);
    drop_redundant_file_size(&mut metadata);
    add_identity_tags(&mut metadata, &reader, path);
    normalize_identity_tags(&mut metadata);

    if !metadata.contains_key("File:ExifByteOrder")
        && format != FileFormat::DR4
        // See the citation on the equivalent guard in `read_metadata`.
        && format != FileFormat::BigTIFF
        && let Ok(head) = reader.read(0, 2)
    {
        let order = match &head[..] {
            b"II" => Some(ByteOrder::LittleEndian),
            b"MM" => Some(ByteOrder::BigEndian),
            _ => None,
        };
        if let Some(order) = order {
            metadata.insert(
                "File:ExifByteOrder",
                TagValue::new_string(order.exif_byte_order_tag()),
            );
        }
    }

    crate::composite::apply(&mut metadata);

    record_diagnostics(&mut metadata, &diagnostics);
    let status = if diagnostics.is_empty() {
        ParseStatus::Parsed
    } else {
        ParseStatus::Partial
    };

    Ok(ReadReport {
        metadata,
        status,
        diagnostics,
    })
}

/// Shared tail of `read_metadata_report_with_detector`'s two "format
/// detection/dispatch declined this file" branches: try to at least name
/// the file from `crate::filetype`'s identification tables
/// ([`add_identity_tags`]), and either way record why the real parse never
/// happened.
fn identify_or_report_unsupported(
    mut metadata: MetadataMap,
    reader: &dyn FileReader,
    path: &Path,
    e: ExifToolError,
) -> ReadReport {
    if add_identity_tags(&mut metadata, reader, path) {
        crate::composite::apply(&mut metadata);
        return ReadReport {
            metadata,
            status: ParseStatus::IdentifiedOnly,
            diagnostics: Vec::new(),
        };
    }
    let diagnostics = vec![Diagnostic::warning(e.to_string())];
    record_diagnostics(&mut metadata, &diagnostics);
    ReadReport {
        metadata,
        status: ParseStatus::Unsupported,
        diagnostics,
    }
}

/// Surfaces diagnostics as ExifTool-style tags instead of leaving them only
/// in `ReadReport::diagnostics`.
///
/// `Warn`/`Error` (`ExifTool.pm:5616`, `:5654`) both resolve to
/// `$self->FoundTag('Warning'|'Error', $str)` -- in ExifTool a diagnostic
/// *is* a tag, not a side channel that can go unreported. OxiDex has no
/// per-read family-1 group to file it under, so both land under `File:`,
/// the same group the pre-existing Casio CAM `File:Warning`
/// (`parse_casio_cam_metadata`, below) already uses.
///
/// Warnings: each *distinct* message becomes its own occurrence at priority
/// `0`, one call to [`MetadataMap::insert_occurrence`] per message --
/// `Warn` itself dedupes identical text before ever calling `FoundTag`
/// (`WAS_WARNED`, `ExifTool.pm:5629-5636`), so a repeated message is
/// recorded once here too. The default view is then whichever warning was
/// recorded *first*: verified against the pinned oracle on `GE.jpg`, which
/// carries two distinct MakerNotes-offset warnings and reports only the
/// first (`"...offset for tag 0x0200"`) as the bare `Warning` tag under
/// `-j -Warning`, `-j -a -Warning` and `-Warning` alike -- i.e. `-a` makes no
/// difference to the default winner, matching two `Priority => 0`-shaped
/// arrivals tying in the first's favor. An existing higher-priority
/// `File:Warning` (e.g. Casio's own, below, inserted through the ordinary
/// `insert()` shim) is *never* clobbered by a diagnostic either, for the
/// same reason: `TagSink::record`'s priority-0 promotion means a real
/// priority-1 tag always beats a priority-0 arrival, in either order. So
/// nothing routed through here is silently lost the way the `eprintln!`s it
/// replaces were, and every distinct message stays reachable through the
/// occurrence store even when it does not win the default view.
fn record_diagnostics(metadata: &mut MetadataMap, diagnostics: &[Diagnostic]) {
    const WARNING_PRIORITY: u8 = 0;
    let mut seen_warnings = std::collections::HashSet::new();
    for d in diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::Warning)
    {
        if seen_warnings.insert(d.message.as_str()) {
            metadata.insert_occurrence(
                "File:Warning",
                TagValue::new_string(d.message.clone()),
                WARNING_PRIORITY,
                "",
                Instance::default(),
            );
        }
    }

    let errors: Vec<&str> = diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::Error)
        .map(|d| d.message.as_str())
        .collect();
    if !errors.is_empty() && !metadata.contains_key("File:Error") {
        metadata.insert("File:Error", TagValue::new_string(errors.join("; ")));
    }
    // DiagnosticKind::Refusal is deliberately not surfaced as a tag here --
    // it is the seam for Step 10's runtime refusals, which are a maintainer
    // policy decision rather than something ExifTool would ever call a
    // `Warning`/`Error`. Nothing constructs one yet.
}

/// Writes modified metadata to a file at the specified path.
///
/// This function orchestrates the complete metadata write workflow:
/// 1. Validates all tag values against their type definitions
/// 2. Opens the original file with MMapReader
/// 3. Detects file format via magic bytes
/// 4. Serializes metadata using appropriate format writer
/// 5. Writes result atomically using atomic_writer
///
/// # Arguments
///
/// * `path` - Path to the file to write metadata to
/// * `metadata` - MetadataMap containing tags to write
///
/// # Returns
///
/// * `Ok(())` - Successfully validated and wrote metadata
/// * `Err(ExifToolError)` - A key that would not be written (`TagsNotWritten`),
///   validation failure, I/O error, or unsupported format
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::operations::{read_metadata, write_metadata};
/// use oxidex::core::tag_value::TagValue;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let path = Path::new("photo.jpg");
///
/// // Read existing metadata
/// let mut metadata = read_metadata(path)?;
///
/// // Modify a tag
/// metadata.insert("EXIF:Artist", TagValue::new_string("John Doe"));
///
/// // Write back to file
/// write_metadata(path, &metadata)?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - Any tag value fails validation (InvalidTagValue)
/// - File cannot be opened or read (IoError)
/// - File format is unsupported (UnsupportedFormat)
/// - Serialization fails (ParseError)
/// - Atomic write fails (IoError)
///
/// # Validation
///
/// All tags are validated before any file operations. Validation checks:
/// - Type matching for tags with reliable registry type metadata
/// - Intrinsic value constraints (e.g., Rational denominator != 0)
///
/// YAML-backed tags with absent or conflicting type metadata are still validated for intrinsic
/// constraints, but they do not force strict fallback `String` matching.
///
/// Tags not in the registry are skipped during validation (allows custom tags).
///
/// # What is requested, and the guarantee
///
/// Every row of `metadata` that is new or differs from the file's current
/// map is a request to set it. A row of the file's map that `metadata` lacks
/// is a request to delete it only when `metadata` is a read of this same
/// file (`read_metadata(path)`, `Metadata::from_path`) from which the caller
/// removed that row; a map built from scratch, or read from another file,
/// only sets -- ExifTool's `SetNewValue` model, where a tag nobody named is
/// never deleted (ExifTool.pod; maintainer decision on #951). Explicit
/// deletions are [`remove_tag`]'s. The rows that describe the file rather
/// than being stored in it (`File:`, `System:`, `Composite:`, `ExifTool:`)
/// are never deletions, and the file-system facts among them (`FileName`,
/// `Directory`, `FileSize`, the file dates and permissions) are never
/// requests at all. A read-modify-write that changes nothing writes nothing.
///
/// The requests go through the same resolution as [`modify_tag`] and the
/// CLI (`core::write_transaction`): an ungrouped name is written where
/// pinned ExifTool writes it (`XPTitle` -> `IFD0:XPTitle`) or refused, and a
/// key the format's writer cannot write -- `XMP:Title` in a JPEG, TIFF or
/// PNG, a `File:` key, most `IFD1:` keys -- is refused. `Ok(())` means every
/// request is in the file, proven by a read-back, and the [`WriteOutcome`]
/// says whether the bytes changed (ExifTool `WriteInfo`'s 1 / 2); otherwise
/// the error is
/// [`ExifToolError::TagsNotWritten`] naming every key that would not be
/// written, and the file is byte-identical to before the call.
pub fn write_metadata(path: &Path, metadata: &MetadataMap) -> Result<WriteOutcome> {
    write_metadata_and_delete_groups(path, metadata, &[])
}

/// [`write_metadata`] plus `-GROUP:All=` group deletions (`"EXIF:All"`,
/// `"GPS:All"`), all in the one transaction. The C ABI's
/// `exiftool_write_file` uses it for the groups `exiftool_remove_tag`
/// recorded: a group deletion names no row of the map, so the map alone
/// cannot carry it.
pub(crate) fn write_metadata_and_delete_groups(
    path: &Path,
    metadata: &MetadataMap,
    groups: &[String],
) -> Result<WriteOutcome> {
    let baseline = read_metadata(path)?;
    let mut changes = crate::core::write_transaction::changes_between(
        &baseline,
        metadata,
        metadata.read_from(path),
    );
    changes.extend(
        groups
            .iter()
            .map(|group| crate::core::write_transaction::TagChange::delete(group.clone())),
    );
    if changes.is_empty() {
        // the file already holds this map: nothing to write
        return Ok(WriteOutcome::Unchanged);
    }
    crate::core::write_transaction::apply_tag_changes(path, &changes)
}

/// [`write_metadata`], plus the keys the caller asked by name to delete. The
/// surgical EXIF writers need them for an entry the reader surfaced no row
/// for, whose key a `-TAG=` could not take out of the map (see
/// `exif_surgical::removal_names_rowless_entry`); every other writer judges
/// removals by the map alone.
pub(crate) fn write_metadata_with_removals(
    path: &Path,
    metadata: &MetadataMap,
    removed: &[String],
) -> Result<()> {
    write_metadata_transaction(path, metadata, removed)
}

/// One write transaction: `removed` (named tags and `<group>:All`) applied
/// first, then the map -- whose assigned keys are the caller's explicit sets
/// (`modify_tag`'s tag, a batch's `-TAG=value`s), whatever their value.
///
/// Provenance, not equality, decides what a set is, and the map records it
/// per occurrence: a value a public mutation (`insert`, `get_mut`) put there
/// is an assignment, a row a reader produced is not
/// (`MetadataMap::is_assigned`, the single notion the writers --
/// this transaction, the XP strings' direct write -- all ask). A value that
/// differs from the file's is a set whatever its provenance (a carried row
/// never differs); an assigned key whose value equals the file's is a set
/// too, which the values alone cannot show. When such a same-value set falls under
/// one of the removals (`removed = ["IFD0:Make"]` and `IFD0:Make=Acme` with
/// Make already Acme, or `EXIF:All` and a set in it) the transaction runs in
/// ExifTool's order -- the removals, then the sets -- as two passes on a
/// private copy of the file that replaces it only when both succeed; pinned
/// ExifTool 13.59's `-IFD0:Make= -IFD0:Make=Acme` keeps Make. Otherwise it
/// is one pass.
pub(crate) fn write_metadata_transaction(
    path: &Path,
    metadata: &MetadataMap,
    removed: &[String],
) -> Result<()> {
    let baseline = read_metadata(path).unwrap_or_default();
    let assigned = metadata.assigned_keys();
    crate::writers::rw2_ifd0::refuse_rw2_same_value_sets(path, &baseline, metadata, &assigned)?;
    let canonical = |key: &str| crate::writers::exif_surgical::canonical_write_key(key, &baseline);
    let removals: Vec<String> = removed.iter().map(|key| canonical(key)).collect();
    // Same-value sets a removal covers: the only ones the map cannot tell
    // from carried rows, and the only ones that need the two passes.
    let resets: Vec<(String, TagValue)> = assigned
        .iter()
        .filter_map(|key| {
            let value = metadata.get(key)?.clone();
            let key = canonical(key);
            (baseline.get(key.as_str()) == Some(&value)
                && removals.iter().any(|removal| {
                    crate::writers::exif_surgical::removal_covers(removal, &key, &baseline)
                }))
            .then_some((key, value))
        })
        .collect();
    if resets.is_empty() {
        return write_single_pass(path, metadata, removed);
    }
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // Same directory (the final rename stays on one filesystem), same
    // extension (a format some readers tell by it reads the same).
    let suffix = path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    let staged = tempfile::Builder::new()
        .prefix(".oxidex-transaction-")
        .suffix(&suffix)
        .tempfile_in(dir)
        .map_err(ExifToolError::from)?;
    std::fs::copy(path, staged.path()).map_err(ExifToolError::from)?;
    // Pass 1: the removals, with the same-value sets they cover left out.
    let mut first = metadata.clone();
    for (key, _) in &resets {
        first.remove(key);
        if let Some(spelled) = assigned.iter().find(|k| canonical(k) == *key) {
            first.remove(spelled);
        }
    }
    write_single_pass(staged.path(), &first, removed)?;
    // Pass 2: the sets, over what the removals left.
    let mut second = read_metadata(staged.path())?;
    for (key, value) in &resets {
        second.insert(key.clone(), value.clone());
    }
    write_single_pass(staged.path(), &second, &[])?;
    staged
        .persist(path)
        .map_err(|error| ExifToolError::from(error.error))?;
    Ok(())
}

/// One pass of [`write_metadata_transaction`].
fn write_single_pass(path: &Path, metadata: &MetadataMap, removed: &[String]) -> Result<()> {
    let reader = MMapReader::new(path)?;
    let format = detect_format(&reader)?;

    // The caller-changed set, recovered the same way the JPEG surgical writer
    // recovers it: by re-reading the file being written. See
    // `validate_caller_changes` for why that is both sufficient and necessary.
    let baseline = read_metadata(path).ok();

    // One spelling for every key, once, before any gate reads one: group
    // and tag names are case-insensitive, as they are to ExifTool
    // (`-exif:All=`, `-ExifIFD:iso=`, `-ifd0:artist=you`). Every prefix test
    // below -- the no-op checks, the carrier and group removals, the PNG
    // EXIF gate, the planners, the verifiers -- then sees the canonical form;
    // before, a lowercase group was carried as unchanged and reported done.
    let (normalized_metadata, normalized_removed) =
        crate::writers::exif_surgical::normalize_write_request(
            baseline.as_ref().unwrap_or(&MetadataMap::new()),
            metadata,
            removed,
        );
    let metadata = &normalized_metadata;
    let removed = normalized_removed.as_slice();

    // PHASE 1: VALIDATION
    // JPEG and the TIFF-structured formats validate inside their surgical
    // writers, which already have the original bytes to diff against.
    if !matches!(format, FileFormat::JPEG) && !is_surgical_tiff_target(format, &reader) {
        validate_caller_changes(metadata, baseline.as_ref())?;
    }

    // PHASE 2: ROUTE TO APPROPRIATE WRITER
    // A whole-carrier clear (an empty replacement map, no named removals:
    // `clear_all_metadata`, `-all=`) never parses or verifies what it
    // discards; its post-condition is only that the carrier is gone. Any
    // other request is first asked whether it is a no-op for the file's EXIF
    // (`exif_surgical::exif_request_is_no_op`) -- before every writer guard
    // and the post-write check -- and a no-op writes nothing.
    let whole_clear = metadata.is_empty() && removed.is_empty();
    if is_surgical_tiff_target(format, &reader) {
        let file_bytes = reader.read(0, reader.size() as usize)?;
        let original = baseline.unwrap_or_default();
        // Group-wide `<group>:All` removals pinned ExifTool 13.59 makes no
        // change for here (IFD0 of any TIFF; ExifIFD and MakerNotes of a raw
        // type) are no-ops; the rest are refused
        // (`exif_surgical::resolve_tiff_group_removals`).
        let removed = &crate::writers::exif_surgical::resolve_tiff_group_removals(
            file_bytes, &original, removed,
        )?;
        // An IFD0-group edit of a Panasonic RAW/RW2/RWL that pinned
        // ExifTool 13.59 makes in the embedded JpgFromRaw's IFD0, or in an
        // outer `PanasonicRaw::Main` entry this writer cannot edit, is
        // refused by name, before the no-op check can take it for one
        // (`rw2_ifd0::refuse_rw2_ifd0_edits`).
        if !whole_clear {
            crate::writers::rw2_ifd0::refuse_rw2_ifd0_edits(
                file_bytes, &original, metadata, removed,
            )?;
        }
        // A single-tag edit pinned ExifTool 13.59 makes in a Panasonic
        // JpgFromRaw's own EXIF is refused by name, before the no-op check
        // (which reads only the outer directories) can take a tag held only
        // there for an absent one (`exif_surgical::refuse_embedded_jpeg_edits`).
        if !whole_clear {
            crate::writers::exif_surgical::refuse_embedded_jpeg_edits(
                file_bytes, &original, metadata, removed,
            )?;
        }
        // A maker-note row left out of the map is a deletion this writer
        // cannot make (`exif_surgical::dropped_makernote_rows`); nor is such
        // a request a no-op.
        let drops_makernote_rows = !whole_clear
            && (!crate::writers::exif_surgical::dropped_makernote_rows(&original, metadata, &[])
                .is_empty()
                || !crate::writers::exif_surgical::changed_makernote_rows(&original, metadata)
                    .is_empty());
        if !whole_clear {
            crate::writers::exif_surgical::refuse_dropped_makernote_rows(
                &crate::writers::exif_surgical::dropped_makernote_rows(
                    &original, metadata, removed,
                ),
            )?;
            crate::writers::exif_surgical::refuse_changed_makernote_rows(
                &crate::writers::exif_surgical::changed_makernote_rows(&original, metadata),
            )?;
        }
        if !whole_clear
            && !drops_makernote_rows
            && crate::writers::exif_surgical::exif_request_is_no_op(
                &[file_bytes],
                &[file_bytes],
                crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS,
                false,
                &original,
                metadata,
                removed,
            )
        {
            return Ok(());
        }
        let plan = crate::writers::generated_public_write::plan_public_write(
            &original, metadata, removed,
        )?;
        let out = crate::writers::generated_public_write::rewrite_tiff_transaction(
            file_bytes, &original, plan,
        )?;
        // Every removal gone, every set present, before anything is written.
        if !whole_clear {
            crate::writers::exif_surgical::verify_exif_write(
                Some(file_bytes),
                &out,
                &original,
                metadata,
                removed,
                crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS,
            )?;
            crate::writers::exif_surgical::verify_dropped_rows_gone(
                &original,
                metadata,
                &out,
                crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS,
            )?;
            crate::writers::exif_surgical::verify_makernote_rows_set(
                &original,
                metadata,
                Some(file_bytes),
                &out,
                crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS,
            )?;
            // Every maker-note value still reads back, the data it locates
            // outside the MakerNote included (`writers::makernote_guard`).
            // Never on a whole clear or a carrier removal (a deleted carrier
            // is not read), and a no-op returned above.
            if !crate::writers::exif_surgical::removes_carrier(removed) {
                crate::writers::makernote_guard::verify_makernote_preserved(
                    crate::writers::makernote_guard::Carrier::block(file_bytes),
                    crate::writers::makernote_guard::Carrier::block(&out),
                    crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS,
                )?;
            }
        }
        crate::writers::rw2_ifd0::verify_jpg_from_raw_kept(file_bytes, &out)?;
        write_atomic(path, &out)?;
        return Ok(());
    }

    match format {
        FileFormat::JPEG => {
            let original = baseline.unwrap_or_default();
            let file_bytes = reader.read(0, reader.size() as usize)?;
            // `MakerNotes:All` also drops a Canon CIFF APP0 segment, as pinned
            // ExifTool 13.59 does (`exif_surgical::jpeg_without_ciff`); the
            // EXIF half of the write then runs on the file without it.
            let without_ciff = removed
                .iter()
                .any(|key| {
                    crate::writers::exif_surgical::group_removal(key)
                        == Some(crate::writers::exif_surgical::GroupRemoval::MakerNotes)
                })
                .then(|| crate::writers::exif_surgical::jpeg_without_ciff(file_bytes))
                .transpose()?
                .flatten();
            let stripped = without_ciff
                .as_deref()
                .map(crate::writers::exif_surgical::SliceReader);
            let (reader, file_bytes): (&dyn FileReader, &[u8]) = match &stripped {
                Some(stripped) => (stripped, stripped.0),
                None => (&reader, file_bytes),
            };
            let drops_makernote_rows = !whole_clear
                && (!crate::writers::exif_surgical::dropped_makernote_rows(
                    &original,
                    metadata,
                    &[],
                )
                .is_empty()
                    || !crate::writers::exif_surgical::changed_makernote_rows(&original, metadata)
                        .is_empty());
            if !whole_clear {
                crate::writers::exif_surgical::refuse_dropped_makernote_rows(
                    &crate::writers::exif_surgical::dropped_makernote_rows(
                        &original, metadata, removed,
                    ),
                )?;
                crate::writers::exif_surgical::refuse_changed_makernote_rows(
                    &crate::writers::exif_surgical::changed_makernote_rows(&original, metadata),
                )?;
            }
            if !whole_clear
                && !drops_makernote_rows
                && let Ok(payloads) = crate::writers::exif_surgical::jpeg_exif_payloads(file_bytes)
            {
                let blocks: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
                if crate::writers::exif_surgical::exif_request_is_no_op(
                    &blocks,
                    &blocks,
                    crate::writers::exif_surgical::EXIF_BLOCK_MAGICS,
                    true,
                    &original,
                    metadata,
                    removed,
                ) {
                    if without_ciff.is_some() {
                        write_atomic(path, file_bytes)?;
                    }
                    return Ok(());
                }
            }
            let plan = crate::writers::generated_public_write::plan_public_write(
                &original, metadata, removed,
            )?;
            let serialized_bytes = crate::writers::jpeg_writer::write_public_exif_transaction(
                reader, &original, plan,
            )?;
            let after = crate::writers::exif_surgical::jpeg_exif_payloads(&serialized_bytes)?;
            if whole_clear
                || (crate::writers::exif_surgical::removes_carrier(removed) && after.len() > 1)
            {
                // The clear's only post-condition, and one of `IFD0:All` /
                // `EXIF:All` (every EXIF APP1 goes): no second EXIF APP1 is
                // left, nor any after a clear.
                if !after.is_empty() {
                    return Err(ExifToolError::unsupported_format(
                        "EXIF write verification failed: the EXIF block was to be \
                         cleared but is still present; nothing was written",
                    ));
                }
            } else {
                // Every removal gone, every set present, before anything is
                // written (`exif_surgical::verify_exif_write`).
                let before = crate::writers::exif_surgical::jpeg_exif_payload(file_bytes)?;
                crate::writers::exif_surgical::verify_exif_write(
                    before.as_deref(),
                    after.first().map(Vec::as_slice).unwrap_or_default(),
                    &original,
                    metadata,
                    removed,
                    crate::writers::exif_surgical::EXIF_BLOCK_MAGICS,
                )?;
                crate::writers::exif_surgical::verify_dropped_rows_gone(
                    &original,
                    metadata,
                    after.first().map(Vec::as_slice).unwrap_or_default(),
                    crate::writers::exif_surgical::EXIF_BLOCK_MAGICS,
                )?;
                crate::writers::exif_surgical::verify_makernote_rows_set(
                    &original,
                    metadata,
                    before.as_deref(),
                    after.first().map(Vec::as_slice).unwrap_or_default(),
                    crate::writers::exif_surgical::EXIF_BLOCK_MAGICS,
                )?;
                // The chain past IFD1 on the whole files: its IFD2 preview
                // after the image re-pointed as ExifTool re-points it, and
                // no other data outside the block kept (`verify_jpeg_chain`).
                // A deleted carrier is not read (its chain went with it).
                if !crate::writers::exif_surgical::removes_carrier(removed) {
                    crate::writers::exif_surgical::verify_jpeg_chain(
                        file_bytes,
                        &serialized_bytes,
                    )?;
                }
                // Every maker-note value still reads back, the data it
                // locates outside the MakerNote included -- here in the whole
                // file, so a preview in the JPEG trailer that a longer EXIF
                // segment would shift away is caught too
                // (`writers::makernote_guard`). Never on a whole clear or a
                // carrier removal (a deleted carrier is not read), and a no-op
                // returned above.
                if !crate::writers::exif_surgical::removes_carrier(removed) {
                    crate::writers::makernote_guard::verify_jpeg_makernotes(
                        file_bytes,
                        &serialized_bytes,
                    )?;
                }
            }
            if without_ciff.is_some()
                && !matches!(
                    crate::writers::exif_surgical::jpeg_without_ciff(&serialized_bytes),
                    Ok(None)
                )
            {
                return Err(ExifToolError::unsupported_format(
                    "EXIF write verification failed: the CIFF segment was to be \
                     deleted but is still present; nothing was written",
                ));
            }
            write_atomic(path, &serialized_bytes)?;
        }
        FileFormat::PNG => {
            // The caller's map was derived from `read_metadata`, so judge
            // which keys changed against that same map (the eXIf chunk goes
            // through the JPEG writer's surgical EXIF transaction, which also
            // needs the named removals).
            let baseline = match baseline {
                Some(baseline) => baseline,
                None => crate::parsers::png::parse_png_metadata(&reader).unwrap_or_default(),
            };
            if !whole_clear {
                crate::writers::exif_surgical::refuse_dropped_makernote_rows(
                    &crate::writers::exif_surgical::dropped_makernote_rows(
                        &baseline, metadata, removed,
                    ),
                )?;
                crate::writers::exif_surgical::refuse_changed_makernote_rows(
                    &crate::writers::exif_surgical::changed_makernote_rows(&baseline, metadata),
                )?;
            }
            write_png_metadata_with_removals(path, &reader, metadata, &baseline, removed)?
        }
        FileFormat::PDF => {
            write_pdf_file(path, &reader, metadata)?;
        }
        FileFormat::TIFF | FileFormat::CameraRaw(_) => {
            // Reached only when the container is not actually walkable as a
            // TIFF (BigTIFF, or a RAW wrapper like RAF/MRW/X3F/CR3 whose
            // outer bytes are not a TIFF header).
            return Err(ExifToolError::unsupported_format(format!(
                "Write operations for format {:?} are not supported: its container \
                 is not a walkable TIFF structure",
                format
            )));
        }
        _ => {
            return Err(ExifToolError::unsupported_format(format!(
                "Write operations for format {:?} are not supported",
                format
            )));
        }
    }

    Ok(())
}

/// Whether this file should go through the surgical whole-file TIFF writer.
///
/// Gated on the *bytes*, not the format label: `FileFormat::CameraRaw` covers
/// both TIFF-structured RAWs (NEF, CR2, IIQ, RW2, ARW, ...) and proprietary
/// wrappers that merely embed a TIFF somewhere inside (RAF, MRW, X3F, CR3).
/// Only the former can be edited in place.
pub(crate) fn is_surgical_tiff_target(format: FileFormat, reader: &dyn FileReader) -> bool {
    if !matches!(format, FileFormat::TIFF | FileFormat::CameraRaw(_)) {
        return false;
    }
    let header = reader.read(0, reader.size().min(8) as usize).unwrap_or(&[]);
    crate::writers::tiff_surgical::is_walkable_tiff(header)
}

/// Validates the values the caller actually changed, and only those.
///
/// `baseline` is the map the reader produces from the file on disk right now.
/// A value equal to its baseline is *carried over*, not authored: writing it
/// back cannot introduce a value the file did not already contain, so
/// re-validating it protects nothing — while rejecting it breaks writes
/// outright, which is issue #20. The reader legitimately emits display forms
/// that do not match a tag's declared `TagValue` type (`IFD0:BitsPerSample`
/// as the string "8 8 8", `ExifIFD:DateTimeOriginal` as an unparsed string),
/// so whole-map validation failed on tags the caller never touched and every
/// non-JPEG write died on a tag it was not writing.
///
/// This is the same rule the JPEG surgical writer applies per entry
/// (`exif_surgical::plan_exif_write`), hoisted to be format-agnostic. It needs
/// no API change: the "was this changed?" question is answerable from the file
/// itself, exactly as `rewrite_jpeg_exif` answers it.
///
/// With no readable baseline (`None`) every value is treated as changed, so
/// the check degrades to the original whole-map validation rather than to
/// no validation at all.
fn validate_caller_changes(metadata: &MetadataMap, baseline: Option<&MetadataMap>) -> Result<()> {
    for (tag_name, tag_value) in metadata.iter() {
        if let Some(baseline) = baseline
            && baseline.get(tag_name) == Some(tag_value)
        {
            continue; // carried over unchanged
        }
        // Look up tag descriptor in registry
        if let Some(descriptor) = get_tag_descriptor(tag_name) {
            // A PDF Info date travels as text so its zone (or its absence)
            // survives to the writer (`cli::value_parser::parse_pdf_date`;
            // `TagValue::DateTime` cannot say "no zone").
            let pdf_date_text = tag_name.starts_with("PDF:")
                && matches!(tag_value, TagValue::String(_))
                && matches!(descriptor.value_type, crate::core::ValueType::DateTime);
            if has_reliable_value_type(tag_name) && !pdf_date_text {
                // Pass the original tag_name (e.g., "IFD0:Make") for error messages.
                validate_tag_value_with_name(tag_name, descriptor, tag_value)?;
            } else {
                validate_tag_value_intrinsics(tag_name, tag_value)?;
            }
        }
        // If tag is not in registry, skip validation (allows custom/rare tags)
    }
    Ok(())
}

fn canonical_write_tag_name(tag_name: &str) -> &str {
    match tag_name {
        "GPSVersionID" => "GPS:GPSVersionID",
        "GPSDateStamp" => "GPS:GPSDateStamp",
        "GPSLatitudeRef" => "GPS:GPSLatitudeRef",
        "GPSDestLatitudeRef" => "GPS:GPSDestLatitudeRef",
        "GPSDestBearing" => "GPS:GPSDestBearing",
        "CreateDate" => "ExifIFD:CreateDate",
        "ExposureTime" => "ExifIFD:ExposureTime",
        "BrightnessValue" => "ExifIFD:BrightnessValue",
        "LightSource" => "ExifIFD:LightSource",
        "Contrast" => "ExifIFD:Contrast",
        "DigitalZoomRatio" => "ExifIFD:DigitalZoomRatio",
        "Sharpness" => "ExifIFD:Sharpness",
        "CustomRendered" => "ExifIFD:CustomRendered",
        "GainControl" => "ExifIFD:GainControl",
        "FileSource" => "ExifIFD:FileSource",
        "ExposureProgram" => "ExifIFD:ExposureProgram",
        "WhiteBalance" => "ExifIFD:WhiteBalance",
        "SceneCaptureType" => "ExifIFD:SceneCaptureType",
        "Saturation" => "ExifIFD:Saturation",
        "FlashpixVersion" => "ExifIFD:FlashpixVersion",
        "CompressedBitsPerPixel" => "ExifIFD:CompressedBitsPerPixel",
        "RelatedSoundFile" => "ExifIFD:RelatedSoundFile",
        "SubjectDistanceRange" => "ExifIFD:SubjectDistanceRange",
        "ComponentsConfiguration" => "ExifIFD:ComponentsConfiguration",
        "SecurityClassification" => "ExifIFD:SecurityClassification",
        "MeteringMode" => "ExifIFD:MeteringMode",
        "ShutterSpeedValue" => "ExifIFD:ShutterSpeedValue",
        "Flash" => "ExifIFD:Flash",
        "Software" => "IFD0:Software",
        "DocumentName" => "IFD0:DocumentName",
        "PageNumber" => "IFD0:PageNumber",
        "MakerNoteSafety" => "IFD0:MakerNoteSafety",
        "ProfileEmbedPolicy" => "IFD0:ProfileEmbedPolicy",
        "ModifyDate" => "IFD0:ModifyDate",
        "DateTimeOriginal" => "ExifIFD:DateTimeOriginal",
        "ApertureValue" => "ExifIFD:ApertureValue",
        _ => tag_name,
    }
}

/// The key `modify_tag`/`remove_tag` hand to the writers for `tag_name`,
/// or an error when no writer of this file's format would write it.
///
/// Without this, an ungrouped name (`XPTitle`) or a group the format's writer
/// never visits (`XMP:Title` in a JPEG) was inserted into the map, skipped by
/// the writer, and reported as a successful write. The hand-kept
/// [`canonical_write_tag_name`] spellings keep their existing addresses;
/// every other ungrouped name is resolved the way pinned ExifTool resolves it
/// or refused (`writers::write_request::resolve_write_key`), and the result
/// must be a key the format's writer addresses
/// (`writers::write_request::ensure_writer_addresses`).
pub(crate) fn resolve_write_address(
    path: &Path,
    tag_name: &str,
    baseline: &MetadataMap,
) -> Result<String> {
    let (key, addressed) = resolve_write_key_for(path, tag_name, baseline)?;
    addressed?;
    Ok(key)
}

/// [`resolve_write_address`] in two parts: the resolved key (or the error
/// resolving it), and whether the format's writer addresses that key. A
/// removal asks whether it is a no-op between the two (`remove_tag`): a
/// deletion that names nothing succeeds untouched even where the writer
/// could not have written the key.
pub(crate) fn resolve_write_key_for(
    path: &Path,
    tag_name: &str,
    baseline: &MetadataMap,
) -> Result<(String, Result<()>)> {
    use crate::writers::write_request::{
        ensure_not_also_updated, ensure_writer_addresses, generated_route_resolves,
        png_prefers_text, resolve_exif_family_key, resolve_write_key,
    };
    // Group and tag names are case-insensitive, as they are to ExifTool
    // (`-exififd:ISO=200`, `-ExifIFD:iso=`): a grouped name is resolved in
    // #943's canonical spelling (`exif_surgical::canonical_write_key`, the
    // spelling its transaction entry uses too), or the address checks below
    // refused `exififd` and a deletion of `ExifIFD:iso` looked absent.
    // An ungrouped name takes the registry's spelling
    // (`write_request::canonical_request_tag`): the hand-kept spellings below
    // are matched exactly, so `-exposuretime=1/30` missed `ExposureTime` and
    // was refused as a write ExifTool also applies elsewhere.
    let respelled = crate::writers::write_request::canonical_request_tag(tag_name);
    let respelled = if respelled.contains(':') {
        crate::writers::exif_surgical::canonical_write_key(&respelled, baseline)
    } else {
        respelled
    };
    let tag_name = respelled.as_str();
    let reader = MMapReader::new(path)?;
    let format = detect_format(&reader)?;
    let surgical = is_surgical_tiff_target(format, &reader);
    let png = matches!(format, FileFormat::PNG);
    // ExifTool reads IFD0 with Exif::Main unless the TIFF header's identifier
    // is Panasonic's 0x55, which selects PanasonicRaw::Main (ExifTool.pm:
    // 8646-8659 vs 8718). A JPEG's APP1 TIFF and a PNG's eXIf are always
    // Exif::Main.
    let exif_ifd0_target = matches!(format, FileFormat::JPEG)
        || png
        || (surgical && {
            let header = reader.read(0, reader.size().min(4) as usize).unwrap_or(&[]);
            matches!(header, [b'I', b'I', 42, 0] | [b'M', b'M', 0, 42])
        });
    // A Panasonic RAW/RW2/RWL (IFD0 read with PanasonicRaw::Main): a bare
    // name and `EXIF:<name>` act as `IFD0:<name>` there, and a
    // PanasonicRaw tag no writable table names is ExifTool's "Sorry, ...
    // doesn't exist or isn't writable" (#956's `rw2_ifd0::route_rw2_name`,
    // measured against pinned 13.59: roll-up evidence
    // `rw2-bare-names-oracle.txt`). Any other file, or a name neither table
    // declares, keeps the resolution below (#945's refusal of an ungrouped
    // name where IFD0 is not Exif::Main included).
    let rw2_key;
    let tag_name = if surgical && !exif_ifd0_target {
        let file_bytes = reader.read(0, reader.size() as usize)?;
        match crate::writers::rw2_ifd0::route_rw2_name(file_bytes, tag_name) {
            crate::writers::rw2_ifd0::Rw2Name::Ifd0(key) => {
                rw2_key = key;
                rw2_key.as_str()
            }
            crate::writers::rw2_ifd0::Rw2Name::NotWritable => {
                return Err(ExifToolError::tag_not_written(
                    tag_name,
                    crate::writers::write_request::sorry_not_writable(tag_name),
                ));
            }
            crate::writers::rw2_ifd0::Rw2Name::NoOp | crate::writers::rw2_ifd0::Rw2Name::Other => {
                tag_name
            }
        }
    } else {
        tag_name
    };
    let canonical = canonical_write_tag_name(tag_name);
    let key = if canonical != tag_name {
        // The hand-kept spellings keep their addresses, under the same checks
        // as every other ungrouped name: only where IFD0 is EXIF, not where
        // ExifTool writes a PNG text tag instead (a PNG's bare `Software` is
        // ExifTool's `PNG:Software`), and never half of a write ExifTool also
        // applies to another group (`XMP-tiff:Software`).
        if (!exif_ifd0_target && !surgical) || (png && png_prefers_text(tag_name)) {
            return Err(resolve_write_key(tag_name, exif_ifd0_target, png, baseline)
                .err()
                .unwrap_or_else(|| {
                    ExifToolError::tag_not_written(tag_name, "name its group explicitly")
                }));
        }
        ensure_not_also_updated(tag_name, canonical, baseline)?;
        canonical.to_string()
    } else if !tag_name.contains(':')
        && surgical
        && !exif_ifd0_target
        && generated_route_resolves(tag_name)
    {
        // A Panasonic RAW's IFD0 is read with PanasonicRaw::Main, so the
        // Exif::Main resolution does not apply there; the generated route's
        // own ungrouped resolution, which predates this function, keeps
        // answering the names it owns.
        tag_name.to_string()
    } else if let Some(resolved) = resolve_exif_family_key(tag_name) {
        // `EXIF:<name>` is the family, not a directory: the tag's own IFD.
        resolved
    } else {
        resolve_write_key(tag_name, exif_ifd0_target, png, baseline)?
    };
    // `PNG:XMP` is oxidex's own key for the raw-packet route
    // (`png_writer::XMP_PACKET_KEY`), which pinned 13.59 also refuses by name
    // -- except when the file's `PNG:XMP` names an ordinary text chunk whose
    // literal keyword is `XMP` (the reader reports it under this key exactly
    // then, per `write_transaction::png_xmp_packets`'s doc): 13.59 answers
    // `-PNG:XMP=world` and `-PNG:XMP=` alike on such a file with `Sorry,
    // PNG:XMP doesn't exist or isn't writable` / `Nothing to do.`, file
    // untouched (maintainer decision on PR #951 review comment 4098201945,
    // superseding the literal-chunk edit this route used to perform). A
    // packet write where no literal chunk exists is unaffected.
    let addressed = if png && key == "PNG:XMP" && baseline.contains_key("PNG:XMP") {
        Err(ExifToolError::tag_not_written(
            tag_name,
            crate::writers::write_request::sorry_not_writable(tag_name),
        ))
    } else {
        ensure_writer_addresses(tag_name, &key, format, surgical)
    };
    Ok((key, addressed))
}

/// Whether deleting `key` from the file at `path` changes nothing: the map
/// does not hold it (under any spelling) and -- for an EXIF carrier -- no
/// entry of any EXIF block is named by it (`exif_surgical::
/// exif_request_is_no_op`, #943's up-front no-op decision, which also sees
/// entries the reader surfaces no row for). A PDF Info field or a PNG text
/// tag the map lacks names nothing. Pinned ExifTool 13.59 answers such a
/// deletion `0 image files updated` / `1 image files unchanged`, bytes
/// untouched.
pub(crate) fn removal_is_no_op(path: &Path, key: &str, metadata: &MetadataMap) -> Result<bool> {
    use crate::writers::exif_surgical::{
        EXIF_BLOCK_MAGICS, exif_request_is_no_op, jpeg_exif_payloads,
    };
    if metadata_holds(metadata, key) {
        return Ok(false);
    }
    // Only where absence is provable: an EXIF directory the block scan below
    // sees entry by entry, a PDF Info field, a PNG text tag. A group the map
    // spells differently (the reader keys `XMP-dc:Title` as `XMP:Title`) is
    // never judged absent from the map alone.
    let group = key.split_once(':').map_or("", |(group, _)| group);
    // The directory chain past IFD1 (`IFD2:`, ... -- #954's
    // `exif_surgical::chain_key_dir`) is scanned entry by entry too: a
    // removal naming nothing it holds is a no-op beside a real edit.
    let exif_group = matches!(
        group,
        "IFD0" | "IFD1" | "ExifIFD" | "GPS" | "InteropIFD" | "EXIF" | "MakerNotes"
    ) || crate::writers::exif_surgical::chain_key_dir(key).is_some();
    if !exif_group && group != "PDF" && group != "PNG" {
        return Ok(false);
    }
    let removed = [key.to_string()];
    let reader = MMapReader::new(path)?;
    let format = detect_format(&reader)?;
    let file_bytes = reader.read(0, reader.size() as usize)?;
    // A PNG with a bad chunk CRC is refused before any no-op decision, as the
    // PNG writer refuses it (#947, `png_writer::check_chunk_crcs`): pinned
    // 13.59 checks every CRC before it learns nothing changed.
    if matches!(format, FileFormat::PNG) {
        crate::writers::png_writer::refuse_bad_chunk_crcs(&reader)?;
    }
    // A single-tag removal acts on every block alike (only a group-wide
    // `<group>:All` distinguishes #943's `group_blocks`); `embedded` marks a
    // JPEG's APP1s, whose empty carrier a rewrite drops.
    let no_op = |blocks: &[&[u8]], magics: &[u16], embedded: bool| {
        exif_request_is_no_op(
            blocks, blocks, magics, embedded, metadata, metadata, &removed,
        )
    };
    Ok(if is_surgical_tiff_target(format, &reader) {
        // The block scan below reads only the outer directories; a tag held
        // only in a Panasonic RW2's embedded JpgFromRaw EXIF is not absent
        // (ExifTool deletes it there), and #943 refuses that edit by name
        // (`exif_surgical::refuse_embedded_jpeg_edits`) before any no-op
        // decision -- so must this shortcut.
        crate::writers::exif_surgical::refuse_embedded_jpeg_edits(
            file_bytes, metadata, metadata, &removed,
        )?;
        // Likewise an IFD0-group removal pinned 13.59 makes in a Panasonic
        // RAW's embedded JpgFromRaw IFD0 (#956, `rw2_ifd0::refuse_rw2_ifd0_edits`,
        // which the writer runs before its own no-op check).
        crate::writers::rw2_ifd0::refuse_rw2_ifd0_edits(file_bytes, metadata, metadata, &removed)?;
        no_op(
            &[file_bytes],
            crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS,
            false,
        )
    } else {
        match format {
            FileFormat::PDF => group == "PDF",
            FileFormat::PNG if group == "PNG" => true,
            _ if !exif_group => false,
            FileFormat::JPEG => {
                let payloads = jpeg_exif_payloads(file_bytes)?;
                let blocks: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
                no_op(&blocks, EXIF_BLOCK_MAGICS, true)
            }
            FileFormat::PNG => match crate::writers::png_writer::png_exif_payloads(&reader)? {
                Some(payloads) => {
                    let blocks: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
                    no_op(&blocks, EXIF_BLOCK_MAGICS, false)
                }
                None => false,
            },
            _ => false,
        }
    })
}

/// The key [`modify_tag`]/[`remove_tag`] would write for `tag_name` in the
/// file at `path`, or the error they would refuse it with. Lets a caller read
/// back exactly the address a write request names.
pub fn resolve_write_tag(path: &Path, tag_name: &str) -> Result<String> {
    let metadata = read_metadata(path)?;
    resolve_write_address(path, tag_name, &metadata)
}

/// Whether `tag_name` names an EXIF-family group in a PDF: ExifTool keeps no
/// EXIF block in a PDF, and pinned 13.59 answers every such write -- a set or
/// a deletion, `-IFD0:Artist=you`, `-EXIF:XPTitle=v`, `-ExifIFD:ISO=200`,
/// `-GPS:GPSAltitude=50`, `-IFD1:XResolution=300`, `-MakerNotes:OwnerName=x`,
/// `-InteropIFD:InteropIndex=R03` on t/images/PDF.pdf and
/// tests/fixtures/pdf/sample.pdf -- with `0 image files updated` / `1 image
/// files unchanged`, bytes untouched. oxidex does the same instead of
/// refusing -- for a name `SetNewValue` accepts in that group
/// (`write_request::exif_group_answer`); any other is refused.
pub(crate) fn exif_group_in_pdf(path: &Path, tag_name: &str) -> Result<bool> {
    use crate::writers::write_request::{ExifGroupAnswer, exif_group_answer};
    let Some((group, name)) = tag_name.rsplit_once(':') else {
        return Ok(false);
    };
    let Some(answer) = exif_group_answer(group, name) else {
        return Ok(false);
    };
    let reader = MMapReader::new(path)?;
    if !matches!(detect_format(&reader)?, FileFormat::PDF) {
        return Ok(false);
    }
    // Only a name `SetNewValue` accepts in that group is ExifTool's
    // `unchanged` (it keeps no EXIF in a PDF). A name it does not define, or
    // one with no address in the group (`GPS:Title`, pinned 13.59: `Sorry,
    // GPS:Title doesn't exist or isn't writable`), is refused, not reported
    // done; so is one oxidex cannot place either way.
    let name = name.strip_suffix('#').unwrap_or(name);
    if !crate::writers::write_request::exiftool_tag_exists(name) {
        return Err(ExifToolError::tag_not_written(
            tag_name,
            format!("Tag '{tag_name}' is not defined"),
        ));
    }
    match answer {
        ExifGroupAnswer::Accepted => Ok(true),
        ExifGroupAnswer::Rejected => Err(ExifToolError::tag_not_written(
            tag_name,
            format!("Sorry, {tag_name} doesn't exist or isn't writable"),
        )),
        ExifGroupAnswer::Unknown => Err(ExifToolError::tag_not_written(
            tag_name,
            format!(
                "Cannot write '{tag_name}' to a PDF: oxidex cannot tell whether ExifTool \
                 gives {name} an address in {group}"
            ),
        )),
    }
}

/// `-GROUP:All=` (`remove_tag(path, "GPS:All")`): a group-wide deletion,
/// decided for the library's write transaction (`core::write_transaction`).
///
/// An EXIF-family group (`exif_surgical::group_removal`) in a JPEG, PNG or
/// TIFF-structured file goes to the writers' group-wide expansion (#943):
/// `Some(key)` is the `<group>:All` removal to hand to
/// [`write_metadata_with_removals`], whose up-front no-op check leaves a file
/// holding nothing in the group byte-identical -- ExifTool's `0 image files
/// updated` / `1 image files unchanged` (pinned 13.59 on t/images/PNG.png:
/// `-GPS:All=`, `-ExifIFD:All=`, `-IFD1:All=`, `-InteropIFD:All=`,
/// `-MakerNotes:All=`, `-IFD0:All=`, `-EXIF:All=`), and whose verifier
/// checks the group is gone. A PDF keeps no EXIF (13.59: unchanged): `None`.
/// Any other group is a no-op (`None`) only where [`group_is_empty`] proves
/// the file holds none of it; otherwise the deletion is refused by name
/// ([`ExifToolError::TagsNotWritten`]) -- never reported as an update that
/// stripped nothing.
pub(crate) fn plan_group_deletion(
    path: &Path,
    tag_name: &str,
    group: &str,
) -> Result<Option<String>> {
    let key = format!("{group}:All");
    let reader = MMapReader::new(path)?;
    let format = detect_format(&reader)?;
    if crate::writers::exif_surgical::group_removal(&key).is_some() {
        let expanded = matches!(format, FileFormat::JPEG | FileFormat::PNG)
            || is_surgical_tiff_target(format, &reader);
        if expanded {
            return Ok(Some(key));
        }
        if matches!(format, FileFormat::PDF) {
            return Ok(None);
        }
    } else {
        drop(reader);
        if group_is_empty(group, &read_metadata(path)?) {
            return Ok(None);
        }
    }
    Err(ExifToolError::tag_not_written(
        tag_name,
        format!(
            "oxidex does not delete a whole {group} group from this file yet, and \
             cannot prove it holds no {group} tags; delete the tags by name"
        ),
    ))
}

/// Whether the reader's map proves the file holds nothing in `group`.
///
/// Only for groups whose rows the map keys under the group's own name: XMP
/// and XML (with every `XMP-*`/`XML-*` group), IPTC and PNG. A family-2
/// group (`Time`, `Camera`) is no map key at all, and some family-0 groups
/// are keyed under another spelling (`Adobe` rows as `APP14`, `MPF` as
/// `MPF0`), so for those an absent key proves nothing: pinned 13.59 rewrites
/// synthetic_text_001.png for `-Time:All=` although the map holds no `Time:`
/// key.
fn group_is_empty(group: &str, metadata: &MetadataMap) -> bool {
    let lower = group.to_ascii_lowercase();
    let key_group_matches = |pred: &dyn Fn(&str) -> bool| {
        metadata.keys().any(|key| {
            key.split_once(':')
                .is_some_and(|(key_group, _)| pred(&key_group.to_ascii_lowercase()))
        })
    };
    let family = lower.split('-').next().unwrap_or_default();
    match family {
        // Every XMP-* (XML-*) row is keyed under an `XMP` (`XML`) spelling:
        // any such row makes every XMP-* group suspect.
        "xmp" | "xml" => !key_group_matches(&|g| g.starts_with(family)),
        // IPTC rides in a Photoshop IRB or a PNG raw profile: a row for
        // either proves nothing about its absence.
        "iptc" if lower == "iptc" => {
            !key_group_matches(&|g| g == "iptc" || g == "photoshop")
                && !metadata
                    .keys()
                    .any(|key| key.to_ascii_lowercase().contains("iptc"))
        }
        "png" if lower == "png" => !key_group_matches(&|g| g == "png"),
        _ => false,
    }
}

/// Every spelling under which the reader surfaces the PDF Info field `key`
/// names: `PDF:CreateDate` and `PDF:CreationDate` (and `ModifyDate` /
/// `ModDate`) are one field, both emitted for PDF.pdf. A single spelling
/// otherwise.
pub(crate) fn field_spellings(key: &str) -> &[&str] {
    match key {
        "PDF:CreateDate" | "PDF:CreationDate" => &["PDF:CreateDate", "PDF:CreationDate"],
        "PDF:ModifyDate" | "PDF:ModDate" => &["PDF:ModifyDate", "PDF:ModDate"],
        _ => &[],
    }
}

/// Whether `metadata` holds `key` under any of its [`field_spellings`].
pub(crate) fn metadata_holds(metadata: &MetadataMap, key: &str) -> bool {
    metadata.contains_key(key)
        || field_spellings(key)
            .iter()
            .any(|alias| metadata.contains_key(alias))
}

/// Removes `key` and every other spelling of the same field. Removing only
/// the requested spelling left the reader's alias in the map, which the PDF
/// writer serialized straight back: `-PDF:CreateDate=` on PDF.pdf appended a
/// revision that still carried the date and was reported as an update, and
/// `-PDF:CreateDate=<new>` lost to the stale `CreationDate` spelling.
pub(crate) fn remove_field(metadata: &mut MetadataMap, key: &str) {
    metadata.remove(key);
    for alias in field_spellings(key) {
        metadata.remove(alias);
    }
}

/// Modifies a single tag in a file's metadata.
///
/// This is a convenience function that:
/// 1. Reads existing metadata from the file
/// 2. Modifies the specified tag with the new value
/// 3. Writes all metadata back to the file
///
/// This ensures all other tags are preserved unchanged.
///
/// # Arguments
///
/// * `path` - Path to the file to modify
/// * `tag_name` - Canonical tag name (e.g., "EXIF:Artist")
/// * `new_value` - New value for the tag
///
/// # Returns
///
/// * `Ok(())` - Successfully modified tag and wrote file
/// * `Err(ExifToolError)` - Read error, validation error, or write error
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::operations::modify_tag;
/// use oxidex::core::tag_value::TagValue;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let path = Path::new("photo.jpg");
///
/// // Modify a single tag
/// modify_tag(
///     path,
///     "EXIF:Artist",
///     TagValue::new_string("John Doe")
/// )?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - File cannot be read (IoError)
/// - The tag would not be written (TagsNotWritten): an ungrouped name that
///   does not resolve to one address, a group the file's writer cannot
///   write, or a value the read-back after writing does not find
/// - New value fails validation (InvalidTagValue)
/// - File cannot be written (IoError)
///
/// The file is unchanged whenever an error is returned.
pub fn modify_tag(path: &Path, tag_name: &str, new_value: TagValue) -> Result<WriteOutcome> {
    // One request through the library's write transaction: resolved to an
    // address the file's writer is proven to write (`resolve_write_address`),
    // applied with every other tag preserved, and read back.
    crate::core::write_transaction::apply_tag_changes(
        path,
        &[crate::core::write_transaction::TagChange::set(
            tag_name, new_value,
        )],
    )
}

/// Removes a metadata tag from a file.
///
/// This function reads the file's metadata, removes the specified tag,
/// and writes the modified metadata back to the file.
///
/// # Arguments
///
/// * `path` - Path to the file
/// * `tag_name` - Name of the tag to remove (e.g., "EXIF:Artist")
///
/// # Returns
///
/// * `Ok(())` - Tag was removed (or didn't exist), proven by a read-back
/// * `Err` - The tag would not be removed (`TagsNotWritten`: a group the
///   file's writer cannot write, or an entry still present after writing),
///   I/O error or unsupported format; the file is unchanged then
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::operations::remove_tag;
/// use std::path::Path;
///
/// // Remove the Artist tag from a JPEG file
/// remove_tag(Path::new("photo.jpg"), "EXIF:Artist").unwrap();
/// ```
pub fn remove_tag(path: &Path, tag_name: &str) -> Result<WriteOutcome> {
    // One deletion through the library's write transaction (see
    // `modify_tag`); deleting a tag the file does not carry is a no-op.
    crate::core::write_transaction::apply_tag_changes(
        path,
        &[crate::core::write_transaction::TagChange::delete(tag_name)],
    )
}

/// Clears all metadata from a file.
///
/// This function removes all metadata tags from a file, leaving only
/// the essential file structure intact. Useful for privacy purposes
/// before sharing files.
///
/// # Arguments
///
/// * `path` - Path to the file
///
/// # Returns
///
/// * `Ok(())` - All metadata was cleared
/// * `Err` - I/O error or unsupported format
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::operations::clear_all_metadata;
/// use std::path::Path;
///
/// // Remove all metadata from a file (privacy)
/// clear_all_metadata(Path::new("photo.jpg")).unwrap();
/// ```
pub fn clear_all_metadata(path: &Path) -> Result<WriteOutcome> {
    // An empty map with no named removals is the writers' whole-carrier
    // clear (`generated_public_write::plan_public_write`), not a series of
    // per-tag deletions -- which `write_metadata` would now make of it. Run
    // on a private copy so the outcome is decided by the bytes.
    crate::core::write_transaction::transact(path, |scratch| {
        write_metadata_with_removals(scratch, &MetadataMap::new(), &[])
    })
}

/// Copies metadata from a source file to a destination file.
///
/// This function orchestrates the metadata copy workflow:
/// 1. Reads metadata from the source file
/// 2. Optionally filters to specified tags
/// 3. Reads existing metadata from destination file
/// 4. Merges source tags into destination metadata (preserving unspecified tags)
/// 5. Writes merged metadata back to destination file
///
/// # Arguments
///
/// * `src` - Path to the source file to copy metadata from
/// * `dest` - Path to the destination file to copy metadata to
/// * `tags` - Optional slice of tag names to copy. If `None`, all tags are copied.
///
/// # Returns
///
/// * `Ok(())` - Successfully copied metadata
/// * `Err(ExifToolError)` - Read error, validation error, or write error
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::operations::copy_metadata;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Copy all metadata from source to destination
/// copy_metadata(
///     Path::new("source.jpg"),
///     Path::new("dest.jpg"),
///     None
/// )?;
///
/// // Copy only specific tags
/// copy_metadata(
///     Path::new("source.jpg"),
///     Path::new("dest.jpg"),
///     Some(&["EXIF:Artist".to_string(), "EXIF:Copyright".to_string()])
/// )?;
/// # Ok(())
/// # }
/// ```
///
/// # Behavior
///
/// - Source tags are merged into destination metadata
/// - Existing destination tags not in the source are preserved
/// - If a tag exists in both source and destination, the source value overwrites it
/// - If `tags` filter is specified, only those tags are copied from source
///
/// # Errors
///
/// Returns an error if:
/// - Source file cannot be read (IoError)
/// - Destination file cannot be read (IoError)
/// - A tag the caller named would not be written (TagsNotWritten, naming
///   it). Without a filter the copy is best-effort, like ExifTool's
///   `SetNewValuesFromFile` with no tag list ("All writable tags are set if
///   none are specified"): every source tag the destination's writer cannot
///   write is skipped, and [`copy_metadata_report`] returns them in
///   [`CopyReport::uncopied_groups`] / [`CopyReport::uncopied_tags`]
///   (maintainer decision on #951). The derived `File:`, `System:`,
///   `Composite:` and `ExifTool:` rows are never copied.
/// - Any tag value fails validation (InvalidTagValue)
/// - Destination file cannot be written (IoError)
///
/// The destination is unchanged whenever an error is returned.
pub fn copy_metadata(src: &Path, dest: &Path, tags: Option<&[String]>) -> Result<WriteOutcome> {
    copy_metadata_report(src, dest, tags).map(|report| report.outcome)
}

/// What [`copy_metadata_report`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CopyReport {
    /// Tags actually written to the destination.
    pub copied: usize,
    /// For a copy-all: groups of source tags this destination's writer cannot
    /// write, which were therefore not copied (never silently).
    pub uncopied_groups: Vec<String>,
    /// For a copy-all: every source tag not copied, as the source spells it,
    /// with the reason (the tags behind [`Self::uncopied_groups`], plus any
    /// tag of a writable group the destination's writer refused).
    pub uncopied_tags: Vec<crate::error::TagNotWritten>,
    /// What the copy did to the destination's bytes.
    pub outcome: WriteOutcome,
}

/// Groups no ExifTool writer writes either (file-system, derived and
/// ExifTool-generated rows); a copy-all does not report them as uncopied.
const READ_ONLY_GROUPS: &[&str] = &["File", "System", "Composite", "ExifTool"];

/// [`copy_metadata`], reporting how many tags were written.
///
/// With a filter, each entry is one ExifTool `-TagsFromFile` argument:
/// `TAG` or `GROUP:TAG`, optionally redirected (`SRC>DST` or `DST<SRC`).
/// The source value is found by name (and group, when given: a family-1
/// group, or `EXIF`/`XMP` for any of their directories) and written with
/// [`modify_tag`] to the destination spelling -- the same resolution and
/// refusal gate as `-TAG=VALUE`, so a bare `XPTitle` lands in IFD0 and a tag
/// the destination's writer cannot write is refused, never dropped. A filter
/// entry the source does not carry is skipped, as ExifTool skips it (13.59:
/// `-TagsFromFile src -XPSubject -XPTitle` copies XPTitle and says nothing
/// about XPSubject). Wildcards, `all` inside a group, and `--TAG`
/// exclusions are refused rather than approximated.
///
/// Without a filter every source tag the destination's writer can address
/// is copied, and the groups it cannot write are returned in
/// [`CopyReport::uncopied_groups`] for the caller to report.
pub fn copy_metadata_report(
    src: &Path,
    dest: &Path,
    tags: Option<&[String]>,
) -> Result<CopyReport> {
    let source_metadata = read_metadata(src)?;
    let filters = tags.filter(|filters| {
        !filters
            .iter()
            .all(|f| f.eq_ignore_ascii_case("all") || f == "*")
    });
    let Some(filters) = filters else {
        return copy_all(&source_metadata, dest);
    };

    // Resolve every filter against the source first; the destination is then
    // written once, by one write transaction, and only if every request is
    // written -- a later filter's refusal must not leave earlier ones
    // committed.
    let mut pending: Vec<crate::core::write_transaction::TagChange> = Vec::new();
    for filter in filters {
        let (source_spec, dest_spec) = if let Some((from, to)) = filter.split_once('>') {
            (from.trim(), to.trim())
        } else if let Some((to, from)) = filter.split_once('<') {
            (from.trim(), to.trim())
        } else {
            (filter.as_str(), filter.as_str())
        };
        let (source_group, source_name) = match source_spec.rsplit_once(':') {
            Some((group, name)) => (Some(group), name),
            None => (None, source_spec),
        };
        let plain = |text: &str| {
            !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        };
        if !plain(source_name)
            || source_name.eq_ignore_ascii_case("all")
            || source_group.is_some_and(|group| !plain(group))
            || dest_spec.is_empty()
        {
            return Err(ExifToolError::unsupported_format(format!(
                "Cannot copy '{filter}': oxidex copies plain TAG, GROUP:TAG and \
                 SRC>DST names only (no wildcards, group-wide 'all' or exclusions)"
            )));
        }
        let group_matches = |key_group: &str| match source_group {
            None => true,
            Some(group) if group.eq_ignore_ascii_case("EXIF") => matches!(
                key_group,
                "IFD0" | "IFD1" | "ExifIFD" | "GPS" | "InteropIFD" | "EXIF"
            ),
            Some(group) if group.eq_ignore_ascii_case("XMP") => {
                key_group == "XMP" || key_group.starts_with("XMP-")
            }
            Some(group) => key_group.eq_ignore_ascii_case(group),
        };
        let candidates: Vec<(&String, TagValue)> = source_metadata
            .winner_occurrences()
            .filter(|(key, _)| {
                key.split_once(':').is_some_and(|(group, name)| {
                    name.eq_ignore_ascii_case(source_name) && group_matches(group)
                })
            })
            .map(|(key, occurrence)| (key, occurrence.project(ValueChannel::Stored).into_owned()))
            .collect();
        let Some((source_key, value)) = candidates.first().cloned() else {
            continue; // the source does not carry it
        };
        if candidates.iter().any(|(_, other)| *other != value) {
            return Err(ExifToolError::unsupported_format(format!(
                "Cannot copy '{filter}': the source holds {} with different values; \
                 name the group to copy from",
                candidates
                    .iter()
                    .map(|(key, _)| key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        // An XP string copies as its stored bytes re-packed, which keeps a
        // stored surrogate pair (ExifTool's `-TagsFromFile`). Text standing
        // in for those bytes cannot say whether a code point above U+FFFF was
        // a pair; refuse it rather than guess.
        if crate::writers::xp_strings::is_xp_tag_key(source_key) {
            crate::writers::xp_strings::refuse_unknown_provenance(source_key, &value)?;
        }
        pending.push(crate::core::write_transaction::TagChange::set(
            dest_spec, value,
        ));
    }
    let mut report = CopyReport {
        copied: pending.len(),
        ..CopyReport::default()
    };
    if pending.is_empty() {
        return Ok(report); // nothing to copy: the destination is not touched
    }
    report.outcome = crate::core::write_transaction::apply_tag_changes(dest, &pending)?;
    Ok(report)
}

/// The copy-all half of [`copy_metadata_report`]: best-effort, like
/// ExifTool's `SetNewValuesFromFile` with no tag list ("All writable tags
/// are set if none are specified", ExifTool.pod). Every source tag the
/// destination's writer addresses is merged into the destination's own map
/// and written by [`write_metadata`] (sets only: the map is not a read of the
/// source); a tag it cannot write is skipped and reported in
/// [`CopyReport::uncopied_groups`] / [`CopyReport::uncopied_tags`]. A tag of
/// a writable group the write itself refuses by name is dropped from the
/// request and reported the same way (as is a value the destination's
/// validation rejects), and the rest is written again.
fn copy_all(source_metadata: &MetadataMap, dest: &Path) -> Result<CopyReport> {
    let reader = MMapReader::new(dest)?;
    let format = detect_format(&reader)?;
    let surgical = is_surgical_tiff_target(format, &reader);
    drop(reader);
    let dest_baseline = read_metadata(dest)?;
    let mut report = CopyReport::default();
    let mut copied: Vec<(String, TagValue)> = Vec::new();
    for (tag_name, occurrence) in source_metadata.winner_occurrences() {
        let group = tag_name.split_once(':').map_or("", |(group, _)| group);
        if READ_ONLY_GROUPS.contains(&group) {
            continue; // derived rows: never copied
        }
        if let Err(err) = crate::writers::write_request::ensure_writer_addresses(
            tag_name, tag_name, format, surgical,
        ) {
            report
                .uncopied_tags
                .extend(err.tags_not_written().iter().cloned());
            continue;
        }
        // The value as the source file stores it where the producer keeps
        // one beside its printed value (the ExifIFD engine rows: the SHORT
        // behind `ColorSpace` `sRGB`, the bytes behind `Padding`'s
        // placeholder, `TagOccurrence::stored`); the writer serializes stored
        // forms, never printed ones.
        let value = occurrence.project(ValueChannel::Stored).into_owned();
        if crate::writers::xp_strings::is_xp_tag_key(tag_name) {
            crate::writers::xp_strings::refuse_unknown_provenance(tag_name, &value)?;
        }
        copied.push((tag_name.clone(), value));
    }
    // Nothing the destination can hold: do not write at all. Serializing the
    // unchanged map still appended a PDF revision (and may re-lay-out a PNG),
    // which was then reported as an update.
    while !copied.is_empty() {
        let mut dest_metadata = dest_baseline.clone();
        for (key, value) in &copied {
            dest_metadata.insert(key.clone(), value.clone());
        }
        match write_metadata(dest, &dest_metadata) {
            Ok(outcome) => {
                report.outcome = outcome;
                break;
            }
            Err(ExifToolError::TagsNotWritten { tags })
                if tags
                    .iter()
                    .all(|refused| copied.iter().any(|(key, _)| *key == refused.tag)) =>
            {
                // Skip what cannot be written, keep the rest: each round
                // removes at least one tag, so this ends.
                copied.retain(|(key, _)| !tags.iter().any(|refused| refused.tag == *key));
                report.uncopied_tags.extend(tags);
            }
            // A copied value the destination's validation rejects (a source
            // stored form the registry types differently) is skipped the same
            // way.
            Err(ExifToolError::InvalidTagValue { tag_name, reason })
                if copied.iter().any(|(key, _)| *key == tag_name) =>
            {
                copied.retain(|(key, _)| *key != tag_name);
                report
                    .uncopied_tags
                    .push(crate::error::TagNotWritten::new(tag_name, reason));
            }
            Err(other) => return Err(other),
        }
    }
    report.copied = copied.len();
    for tag in &report.uncopied_tags {
        let group = tag.tag.split_once(':').map_or("", |(group, _)| group);
        if !report.uncopied_groups.iter().any(|known| known == group) {
            report.uncopied_groups.push(group.to_string());
        }
    }
    report.uncopied_groups.sort();
    Ok(report)
}

// ============================================================================
// SECTION 3: JPEG METADATA PARSING
// ============================================================================

/// Parses metadata from a JPEG file.
///
/// JPEG files contain metadata in APP segments with EXIF, JFIF, XMP, IPTC, and ICC data.
/// This function coordinates parsing of all segment types.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the JPEG file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully parsed metadata from all segments
/// * `Err(ExifToolError)` - Parse error or invalid JPEG structure
pub(crate) fn parse_jpeg_metadata(
    reader: &dyn FileReader,
    options: &ReadOptions,
) -> Result<MetadataMap> {
    let mut diagnostics = Vec::new();
    parse_jpeg_metadata_with_diagnostics(reader, &mut diagnostics, options)
}

/// Same as [`parse_jpeg_metadata`], but pushes recoverable problems (a
/// malformed XMP packet, an unparseable APP13 Photoshop resource, ...) into
/// `diagnostics` instead of dropping them. [`read_metadata_report_with_detector`]
/// is the only caller that reads `diagnostics` back out; `parse_jpeg_metadata`
/// itself discards them, matching its previous (silent) behavior exactly.
pub(crate) fn parse_jpeg_metadata_with_diagnostics(
    reader: &dyn FileReader,
    diagnostics: &mut DiagnosticSink,
    options: &ReadOptions,
) -> Result<MetadataMap> {
    // Parse JPEG segment structure
    let segments = parse_segments(reader)?;

    let mut metadata = MetadataMap::new();

    // Process different segment types
    process_jfif_segments(&segments, &mut metadata, diagnostics);
    process_exif_segments_with_options(&segments, reader, options, &mut metadata, diagnostics);
    // `Composite:OriginalDecisionData` reads the file at the Canon maker
    // note's `OriginalDecisionDataOffset`, so it needs both.
    crate::parsers::tiff::makernotes::canon::original_decision_data::process_original_decision_data(
        reader,
        &mut metadata,
    );
    // Must run after `process_exif_segments`: a CIFF directory embedded in an
    // APP0 segment can carry its own `Make`/`Model`, and Step 18/19's
    // equal-priority tie rule (`TagSink::record`) gives the win to whichever
    // occurrence is recorded *later* -- which is what the pinned oracle's
    // `t/images/ExifTool.jpg` requires (`-Make` resolves to CIFF's `Canon`,
    // not `IFD0`'s `FUJIFILM`). See `jpeg::ciff_app0` for the citation.
    crate::parsers::jpeg::ciff_app0::process_ciff_app0_segments(&segments, &mut metadata);
    process_xmp_segments(&segments, &mut metadata, diagnostics);

    // AFCP and FotoStation write their records after the JPEG's EOI, so they
    // need the whole file rather than the parsed segment list. Both can carry
    // an IPTC block, and ExifTool ranks the three possible sources like this:
    //
    // * `IPTC::ProcessIPTC` (IPTC.pm:1064-1102) checks each IPTC directory's
    //   metadata path against `%isStandardIPTC` (IPTC.pm:38-54). Only
    //   `JPEG-APP13-Photoshop-IPTC` is standard in a JPEG; a trailer's path is
    //   not, so ExifTool sets `LOW_PRIORITY_DIR{IPTC}` for it and files it
    //   under a numbered family-1 group (`IPTC2`, `IPTC3`, ...).
    // * `FoundTag` (ExifTool.pm:9535-9543) turns that into priority 0 and
    //   keeps the existing value unless `$priority >= $oldPriority`, so a
    //   trailer never displaces the APP13 value -- it is only reachable with
    //   `exiftool -a`.
    // * Between two low-priority directories the *first* one processed wins:
    //   FoundTag promotes an existing 0-priority tag to 1 before comparing
    //   ("promote existing 0-priority tag so it takes precedence over a new
    //   0-tag", ExifTool.pm:9518-9527). `ProcessTrailers` works inwards from
    //   the end of the file, so the outermost trailer is the one processed
    //   first. In `combined-samples/ExifTool.jpg` that is FotoStation, with
    //   AFCP innermost.
    //
    // Inserting into a map keeps the *last* write, so the order below is the
    // reverse of ExifTool's processing order: innermost trailer, outermost
    // trailer, then the standard APP13 resource.
    //
    // The numbered family-1 groups follow ExifTool's processing order
    // instead: every trailer IPTC directory is non-standard, so each takes
    // the next of `IPTC2`, `IPTC3`, ... (see `NonStandardIptcGroups`), handed
    // out outermost trailer first. A second APP13 IPTC resource would itself
    // be non-standard and take `IPTC2` ahead of them; this reader merges all
    // APP13 IPTC under the one standard group, so for such a file the
    // trailers keep the unnumbered legacy group rather than a number that
    // would be off by the directories it cannot see.
    if let Ok(file) = reader.read(0, reader.size() as usize) {
        use crate::parsers::jpeg::{afcp, fotostation, iptc_parser};
        // An IFD0 IPTC-NAA block is non-standard too and would be numbered
        // before the trailers; keep the trailers unnumbered rather than
        // assign them the number that belongs to it.
        let mut iptc_groups = if iptc_parser::app13_iptc_resource_count(&segments) <= 1
            && !crate::core::jpeg_helpers::exif_ifd0_has_iptc_naa(&segments, reader)
        {
            iptc_parser::NonStandardIptcGroups::first()
        } else {
            iptc_parser::NonStandardIptcGroups::unnumbered()
        };
        let (afcp_trailer, fotostation_trailer);
        if fotostation::fotostation_trailer_position(file) > afcp::afcp_trailer_position(file) {
            fotostation_trailer =
                fotostation::parse_fotostation_trailer_grouped(file, &mut iptc_groups);
            afcp_trailer = afcp::parse_afcp_trailer_grouped(file, &mut iptc_groups);
        } else {
            afcp_trailer = afcp::parse_afcp_trailer_grouped(file, &mut iptc_groups);
            fotostation_trailer =
                fotostation::parse_fotostation_trailer_grouped(file, &mut iptc_groups);
        }
        metadata.merge_winners_keeping_group1(&afcp_trailer);
        metadata.merge_winners_keeping_group1(&fotostation_trailer);
    }

    process_iptc_segments(&segments, &mut metadata, diagnostics);
    process_photoshop_segments(&segments, &mut metadata, diagnostics);
    process_uniform_resource_name_segments(&segments, &mut metadata);
    process_icc_segments(&segments, &mut metadata);
    process_mpf_segments(&segments, &mut metadata);
    // APP2/APP4 FPXR: FlashPix streams split across application segments.
    crate::parsers::jpeg::flashpix::process_fpxr_segments(&segments, &mut metadata);
    // SPIFF (APP8) runs before SOF: in a real JPEG byte stream every APPn
    // marker precedes the SOF marker, so ExifTool's own file-order-driven
    // FoundTag arbitration always records SPIFF's ImageWidth/ImageHeight
    // before SOF's own File:ImageWidth/ImageHeight -- meaning, on the rare
    // file that carries both (ExifTool.jpg, a deliberately multi-format
    // test fixture), File:ImageWidth wins the bare `ImageWidth` composite
    // dependency tie by `order` alone (both are ordinary, undeclared
    // priority -- neither table sets `Priority`/`PRIORITY`, confirmed
    // against JPEG.pm's `%SPIFF` GROUPS declaration). Processing them in
    // this file's original SOF-then-SPIFF order inverted that tie and fed
    // Composite:ImageSize/Megapixels the SPIFF dimensions instead
    // (`Composite:ImageSize` "3000x4500"/"13.5" MP where the pinned oracle
    // reports "8x8"/"6.4e-05" MP), caught only once Step 22 replaced the
    // old hard-coded group-rank table with real priority+order arbitration.
    process_spiff_segments(&segments, &mut metadata);
    process_sof_segments_with_options(&segments, &mut metadata, options);
    process_com_segments(&segments, &mut metadata);
    process_dqt_segments_with_options(&segments, &mut metadata, options);
    process_ricoh_rmeta_segments(&segments, &mut metadata);
    process_media_jukebox_segments(&segments, &mut metadata);

    // Canon VRD sits after the JPEG's EOI, so it needs the whole file rather
    // than the parsed segment list, which stops at the EOI marker. It carries
    // no IPTC, so unlike the AFCP and FotoStation trailers read further up it
    // does not have to run before `process_iptc_segments`.
    if let Ok(file) = reader.read(0, reader.size() as usize) {
        for (key, value) in crate::parsers::canon_vrd::parse_canon_vrd_trailer(file).iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // Photo Mechanic's trailer is format-agnostic (ExifTool reads it from
    // ProcessTrailers, not a JPEG-specific proc), so like Canon VRD it needs
    // the whole file rather than the parsed segment list. It carries no
    // IPTC either, so it runs here rather than before `process_iptc_segments`.
    if let Ok(file) = reader.read(0, reader.size() as usize) {
        for (key, value) in
            crate::parsers::photo_mechanic::parse_photo_mechanic_trailer(file).iter()
        {
            metadata.insert(key.clone(), value.clone());
        }
        for (key, value) in crate::parsers::mie::parse_mie_trailer(file).iter() {
            metadata.insert(key.clone(), value.clone());
        }
        // ProcessJPEG identifies and walks trailers only once it reaches SOS
        // (ExifTool.pm:7627-7634); a JPEG whose marker walk ends first (EOI
        // before SOS, a format error) has none read. TrailerStart, which
        // ProcessVivo scans from, is the byte after the EOI that the walk
        // reaches from that SOS (ExifTool.pm:7464-7468,7547-7552).
        if let Some(sos) = segments.iter().find(|segment| segment.marker == 0xFFDA) {
            let trailer_start = usize::try_from(sos.offset)
                .ok()
                .and_then(|offset| offset.checked_add(2))
                .and_then(|after_marker| {
                    crate::parsers::vivo::jpeg_trailer_start(file, after_marker)
                });
            metadata.merge_winners_keeping_group1(
                &crate::parsers::samsung_trailer::parse_trailer_chain(file, trailer_start),
            );
        }
    }

    // Process HDR and manufacturer-specific APP segments
    process_app3_segments(&segments, &mut metadata);
    // Scalado's directory may span consecutive APP4 segments. Each starts
    // with the same 16-byte SCALADO header, which is excluded before the
    // remaining 12-byte records are reassembled.
    let mut scalado_directory = Vec::new();
    for segment in segments.iter().filter(|segment| segment.marker == 0xFFE4) {
        if segment.data.starts_with(b"SCALADO\0") && segment.data.len() >= 16 {
            scalado_directory.extend_from_slice(&segment.data[16..]);
        }
    }
    for (key, value) in
        crate::parsers::jpeg::app_parsers::parse_scalado_directory(&scalado_directory)
    {
        metadata.insert(key, value);
    }
    // Samsung/HP/BenQ/GoPro/Rollei preview JPEG found by byte pattern
    // directly in APP2/APP3(/APP4) payload (ExifTool.pm:7997-8127).
    extract_direct_preview_image(&segments, &mut metadata);
    // InfiRay IJPEG spreads its records over APP2-APP9; APP6 and APP8 are read
    // by the two calls that already own those markers.
    process_samsung_unique_id_segments(&segments, &mut metadata);
    process_infiray_segments(&segments, &mut metadata);
    process_qualcomm_segments(&segments, &mut metadata);
    process_dji_dbg_segments(&segments, &mut metadata);
    process_dji_thermal_segments(&segments, &mut metadata, diagnostics);
    process_app6_segments(&segments, &mut metadata);
    process_app10_segments(&segments, &mut metadata);
    process_app11_segments(&segments, &mut metadata);
    process_app12_segments(&segments, &mut metadata);
    process_app14_segments(&segments, &mut metadata);
    process_app15_segments(&segments, &mut metadata);

    // Normalize tag families to match ExifTool conventions (ExifIFD: -> EXIF:)
    use crate::core::tag_normalization::normalize_metadata_map;
    let normalized = normalize_metadata_map(&metadata);

    Ok(normalized)
}

// ============================================================================
// SECTION 4: TIFF METADATA PARSING
// ============================================================================

/// Parses metadata from a TIFF file.
///
/// TIFF files begin with a TIFF header followed by IFD structures.
/// This function coordinates parsing of all IFDs and sub-IFDs.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the TIFF file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully parsed metadata from all IFDs
/// * `Err(ExifToolError)` - Parse error or invalid TIFF structure
pub(crate) fn parse_tiff_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    parse_tiff_metadata_with_options(reader, &ReadOptions::default_full_listing())
}

pub(crate) fn parse_tiff_metadata_with_options(
    reader: &dyn FileReader,
    options: &ReadOptions,
) -> Result<MetadataMap> {
    // Read TIFF header (first 8 bytes)
    let header = reader.read(0, 8)?;

    // Detect byte order from bytes 0-1
    let byte_order = if &header[0..2] == b"II" {
        ByteOrder::LittleEndian
    } else if &header[0..2] == b"MM" {
        ByteOrder::BigEndian
    } else {
        return Err(ExifToolError::parse_error("Invalid TIFF byte order marker"));
    };

    // Verify magic number 42 (bytes 2-3)
    let magic = read_u16(&header[2..4], byte_order);

    if magic != 42 {
        return Err(ExifToolError::parse_error(format!(
            "Invalid TIFF magic number: expected 42, got {}",
            magic
        )));
    }

    // Read first IFD offset from bytes 4-7
    let first_ifd_offset = read_u32(&header[4..8], byte_order) as u64;

    // Parse all IFDs in the chain (IFD0, IFD1, IFD2, ...)
    let mut metadata = MetadataMap::new();

    // Endianness of the TIFF header, which is what ExifTool reports as
    // ExifByteOrder for TIFF-based files (TIFF, DNG, CR2, NEF, ...).
    metadata.insert(
        "File:ExifByteOrder",
        TagValue::new_string(byte_order.exif_byte_order_tag()),
    );
    parse_ifd_chain_with_options(reader, first_ifd_offset, byte_order, options, &mut metadata)?;

    // Add TIFF: prefixed format-specific tags from standard EXIF tags
    // These map standard EXIF tag names to TIFF-specific format tags
    // Collect the tags to add to avoid borrow checker issues
    let tiff_tags = collect_tiff_format_tags(&metadata);
    for (key, value) in tiff_tags {
        metadata.insert(key, value);
    }

    Ok(metadata)
}

/// Collects TIFF: prefixed format-specific tags from standard EXIF tags.
///
/// This function reads standard EXIF tag names from the metadata and creates
/// corresponding TIFF: prefixed versions for format-specific identification.
///
/// Mapped tags:
/// - ImageWidth -> TIFF:Width
/// - ImageLength -> TIFF:Height
/// - BitsPerSample -> TIFF:BitsPerSample
/// - Compression -> TIFF:Compression
/// - PhotometricInterpretation -> TIFF:PhotometricInterpretation
/// - Orientation -> TIFF:Orientation
/// - XResolution -> TIFF:XResolution
/// - YResolution -> TIFF:YResolution
fn collect_tiff_format_tags(source: &MetadataMap) -> Vec<(String, TagValue)> {
    // Map standard EXIF tag names to TIFF: prefixed versions
    let tag_mappings = [
        ("ImageWidth", "TIFF:Width"),
        ("ImageLength", "TIFF:Height"),
        ("BitsPerSample", "TIFF:BitsPerSample"),
        ("Compression", "TIFF:Compression"),
        (
            "PhotometricInterpretation",
            "TIFF:PhotometricInterpretation",
        ),
        ("Orientation", "TIFF:Orientation"),
        ("XResolution", "TIFF:XResolution"),
        ("YResolution", "TIFF:YResolution"),
    ];

    let mut result = Vec::new();

    for (source_tag, dest_tag) in &tag_mappings {
        // Look for the source tag in IFD0 (main image)
        let ifd0_key = format!("IFD0:{}", source_tag);
        if let Some(value) = source.get(&ifd0_key) {
            result.push((dest_tag.to_string(), value.clone()));
            continue;
        }

        // Fall back to unprefixed version if IFD0 version not found
        if let Some(value) = source.get(source_tag) {
            result.push((dest_tag.to_string(), value.clone()));
        }
    }

    result
}

/// Parses metadata from a Casio CAM file.
///
/// Casio CAM files are proprietary JPEG containers with a 70-byte header.
/// This function skips the header and parses the embedded JPEG data.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the Casio CAM file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully parsed metadata from embedded JPEG
/// * `Err(ExifToolError)` - Parse error or invalid file structure
pub(crate) fn parse_casio_cam_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    // Casio CAM format: 70-byte proprietary header + JPEG data
    const HEADER_SIZE: u64 = 70;

    if reader.size() <= HEADER_SIZE {
        return Err(ExifToolError::parse_error(
            "File too small to be a valid Casio CAM file",
        ));
    }

    // Read the JPEG data starting at offset 70
    let jpeg_size = (reader.size() - HEADER_SIZE) as usize;
    let jpeg_data = reader.read(HEADER_SIZE, jpeg_size)?;

    // Create an in-memory reader for the JPEG data
    struct CasioCamJpegReader {
        data: Vec<u8>,
    }

    impl FileReader for CasioCamJpegReader {
        fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = start + length;

            if end > self.data.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "read beyond end of JPEG data",
                ));
            }

            Ok(&self.data[start..end])
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    let jpeg_reader = CasioCamJpegReader {
        data: jpeg_data.to_vec(),
    };

    // Parse the JPEG metadata
    let mut metadata = parse_jpeg_metadata(&jpeg_reader, &ReadOptions::default_full_listing())?;

    // Add warning tag to match ExifTool's behavior
    metadata.insert(
        "File:Warning".to_string(),
        TagValue::String("Processing JPEG-like data after unknown 70-byte header".to_string()),
    );

    Ok(metadata)
}

// ============================================================================
// SECTION 7: TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// A map as it reaches `normalize_identity_tags`: the `File:` group as the
    /// generated tables left it, plus whatever the parser inserted ungrouped.
    fn identity_map(grouped: &[(&str, &str)], bare: &[(&str, &str)]) -> MetadataMap {
        let mut map = MetadataMap::new();
        for (k, v) in grouped {
            map.insert(format!("File:{k}"), TagValue::new_string(*v));
        }
        for (k, v) in bare {
            map.insert((*k).to_string(), TagValue::new_string(*v));
        }
        map
    }

    #[test]
    fn identity_tags_are_emitted_once_under_the_file_group() {
        // Geotag.log: the tables and the TXT parser agree, and the output
        // still carried the answer twice.
        let mut map = identity_map(
            &[
                ("FileType", "TXT"),
                ("FileTypeExtension", "txt"),
                ("MIMEType", "text/plain"),
            ],
            &[("FileType", "TXT"), ("MIMEType", "text/plain")],
        );
        normalize_identity_tags(&mut map);

        for tag in IDENTITY_TAGS {
            assert!(!map.contains_key(tag), "{tag} must not survive ungrouped");
        }
        assert_eq!(map.get_string("File:FileType"), Some("TXT"));
        assert_eq!(map.get_string("File:MIMEType"), Some("text/plain"));
    }

    #[test]
    fn the_tables_outrank_a_contradicting_parser() {
        // Font.dfont reaches the ICO parser because ExifTool's `Font` magic
        // number matches any file starting `\0\x01`. DFONT is ExifTool's
        // answer, and it is the one already in the `File:` group.
        let mut map = identity_map(&[("FileType", "DFONT")], &[("FileType", "ICO")]);
        normalize_identity_tags(&mut map);

        assert_eq!(map.get_string("File:FileType"), Some("DFONT"));
        assert!(!map.contains_key("FileType"));
    }

    #[test]
    fn a_parser_name_fills_an_unnamed_file_type() {
        // EXE.elf: the tables decline, so the parser's name is all there is.
        let mut map = identity_map(&[("FileType", "Unknown")], &[("FileType", "ELF")]);
        normalize_identity_tags(&mut map);

        assert_eq!(map.get_string("File:FileType"), Some("ELF"));
    }

    #[test]
    fn octet_stream_is_an_answer_not_a_gap_to_fill() {
        // ExifTool really does report `application/octet-stream` for LNK, DR4,
        // VRD, MOI and the Mach-O family. A parser's `text/plain` must not
        // overwrite it.
        let mut map = identity_map(
            &[
                ("FileType", "URL"),
                ("MIMEType", "application/octet-stream"),
            ],
            &[("FileType", "TXT"), ("MIMEType", "text/plain")],
        );
        normalize_identity_tags(&mut map);

        assert_eq!(map.get_string("File:FileType"), Some("URL"));
        assert_eq!(
            map.get_string("File:MIMEType"),
            Some("application/octet-stream")
        );
    }

    #[test]
    fn ungrouped_file_size_is_dropped_when_the_grouped_one_exists() {
        let mut m = MetadataMap::new();
        m.insert("File:FileSize", TagValue::new_string("785 bytes"));
        m.insert("FileSize", TagValue::new_string("785"));
        drop_redundant_file_size(&mut m);
        assert_eq!(m.get_string("File:FileSize"), Some("785 bytes"));
        assert!(
            !m.contains_key("FileSize"),
            "the ungrouped duplicate should be gone"
        );
    }

    #[test]
    fn ungrouped_file_size_survives_when_there_is_no_grouped_one() {
        // `extract_file_metadata` failing is the only way here; a badly
        // formatted answer still beats no answer at all.
        let mut m = MetadataMap::new();
        m.insert("FileSize", TagValue::new_string("785"));
        drop_redundant_file_size(&mut m);
        assert_eq!(m.get_string("FileSize"), Some("785"));
    }

    #[test]
    fn grouped_tags_merely_ending_in_file_size_are_left_alone() {
        // Each of these is a different fact from the file's length on disk.
        let mut m = MetadataMap::new();
        m.insert("File:FileSize", TagValue::new_string("785 bytes"));
        m.insert("XML:FileSize", TagValue::new_string("1234"));
        m.insert("File:DPXFileSize", TagValue::new_string("2048"));
        m.insert("LNK:TargetFileSize", TagValue::new_string("4096"));
        drop_redundant_file_size(&mut m);
        assert_eq!(m.get_string("XML:FileSize"), Some("1234"));
        assert_eq!(m.get_string("File:DPXFileSize"), Some("2048"));
        assert_eq!(m.get_string("LNK:TargetFileSize"), Some("4096"));
    }

    /// One fact, one key -- end to end through `read_metadata`.
    ///
    /// A plain-text file used to come back with `File:FileSize` ("785 bytes"),
    /// a bare `FileSize` ("785") and a `TEXT:FileSize` ("785"): three keys for
    /// one fact, two of which contradict ExifTool, which reports only
    /// `File:FileSize`.
    #[test]
    fn a_text_file_reports_its_size_exactly_once() {
        use std::io::Write;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("sample.txt");
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(b"this is plain ASCII text\n").expect("write");
        f.sync_all().expect("sync");

        let metadata = read_metadata(&path).expect("read");

        let size_keys: Vec<&String> = metadata
            .keys()
            .filter(|k| k.rsplit(':').next() == Some("FileSize"))
            .collect();
        assert_eq!(
            size_keys,
            vec!["File:FileSize"],
            "exactly one key should describe the file size"
        );
        assert_eq!(metadata.get_string("File:FileSize"), Some("25 bytes"));

        let text_group: Vec<&String> = metadata.keys().filter(|k| k.starts_with("TEXT:")).collect();
        assert!(
            text_group.is_empty(),
            "ExifTool has no TEXT group; found {text_group:?}"
        );
    }

    #[test]
    fn bare_gps_date_stamp_write_targets_the_gps_ifd() {
        assert_eq!(canonical_write_tag_name("GPSDateStamp"), "GPS:GPSDateStamp");
        assert_eq!(
            canonical_write_tag_name("GPS:GPSDateStamp"),
            "GPS:GPSDateStamp"
        );
    }

    #[test]
    fn bare_writable_exif_parity_tags_target_exif_ifd() {
        for tag in [
            "LightSource",
            "Contrast",
            "DigitalZoomRatio",
            "Sharpness",
            "CustomRendered",
            "GainControl",
            "FileSource",
            "ExposureProgram",
            "WhiteBalance",
            "SceneCaptureType",
            "Saturation",
        ] {
            assert_eq!(
                canonical_write_tag_name(tag),
                format!("ExifIFD:{tag}"),
                "{tag}"
            );
        }
    }

    #[test]
    fn bare_gps_version_id_write_targets_the_gps_ifd() {
        assert_eq!(canonical_write_tag_name("GPSVersionID"), "GPS:GPSVersionID");
        assert_eq!(
            canonical_write_tag_name("GPS:GPSVersionID"),
            "GPS:GPSVersionID"
        );
    }

    #[test]
    fn bare_gps_latitude_ref_write_targets_the_gps_ifd() {
        assert_eq!(
            canonical_write_tag_name("GPSLatitudeRef"),
            "GPS:GPSLatitudeRef"
        );
        assert_eq!(
            canonical_write_tag_name("GPS:GPSLatitudeRef"),
            "GPS:GPSLatitudeRef"
        );
    }

    #[test]
    fn bare_gps_dest_latitude_ref_write_targets_the_gps_ifd() {
        assert_eq!(
            canonical_write_tag_name("GPSDestLatitudeRef"),
            "GPS:GPSDestLatitudeRef"
        );
        assert_eq!(
            canonical_write_tag_name("GPS:GPSDestLatitudeRef"),
            "GPS:GPSDestLatitudeRef"
        );
    }

    #[test]
    fn bare_gps_dest_bearing_write_targets_the_gps_ifd() {
        assert_eq!(
            canonical_write_tag_name("GPSDestBearing"),
            "GPS:GPSDestBearing"
        );
        assert_eq!(
            canonical_write_tag_name("GPS:GPSDestBearing"),
            "GPS:GPSDestBearing"
        );
    }

    #[test]
    fn bare_software_write_targets_ifd0() {
        assert_eq!(canonical_write_tag_name("Software"), "IFD0:Software");
        assert_eq!(canonical_write_tag_name("IFD0:Software"), "IFD0:Software");
    }

    #[test]
    fn bare_create_date_write_targets_exif_ifd() {
        assert_eq!(canonical_write_tag_name("CreateDate"), "ExifIFD:CreateDate");
        assert_eq!(
            canonical_write_tag_name("ExifIFD:CreateDate"),
            "ExifIFD:CreateDate"
        );
    }

    #[test]
    fn bare_exposure_time_write_targets_exif_ifd() {
        assert_eq!(
            canonical_write_tag_name("ExposureTime"),
            "ExifIFD:ExposureTime"
        );
        assert_eq!(
            canonical_write_tag_name("ExifIFD:ExposureTime"),
            "ExifIFD:ExposureTime"
        );
    }

    #[test]
    fn bare_brightness_value_write_targets_exif_ifd() {
        assert_eq!(
            canonical_write_tag_name("BrightnessValue"),
            "ExifIFD:BrightnessValue"
        );
        assert_eq!(
            canonical_write_tag_name("ExifIFD:BrightnessValue"),
            "ExifIFD:BrightnessValue"
        );
    }

    #[test]
    fn bare_metering_mode_write_targets_exif_ifd() {
        assert_eq!(
            canonical_write_tag_name("MeteringMode"),
            "ExifIFD:MeteringMode"
        );
        assert_eq!(
            canonical_write_tag_name("ExifIFD:MeteringMode"),
            "ExifIFD:MeteringMode"
        );
    }

    #[test]
    fn bare_shutter_speed_value_write_targets_exif_ifd() {
        assert_eq!(
            canonical_write_tag_name("ShutterSpeedValue"),
            "ExifIFD:ShutterSpeedValue"
        );
        assert_eq!(
            canonical_write_tag_name("ExifIFD:ShutterSpeedValue"),
            "ExifIFD:ShutterSpeedValue"
        );
    }

    #[test]
    fn bare_flash_write_targets_exif_ifd() {
        assert_eq!(canonical_write_tag_name("Flash"), "ExifIFD:Flash");
        assert_eq!(canonical_write_tag_name("ExifIFD:Flash"), "ExifIFD:Flash");
    }

    #[test]
    fn write_parity_addendum_bare_names_target_exif_ifd() {
        for name in [
            "FlashpixVersion",
            "CompressedBitsPerPixel",
            "RelatedSoundFile",
            "SubjectDistanceRange",
            "ComponentsConfiguration",
            "SecurityClassification",
        ] {
            assert_eq!(
                canonical_write_tag_name(name),
                format!("ExifIFD:{name}"),
                "{name}"
            );
        }
    }

    #[test]
    fn bare_document_name_write_targets_ifd0() {
        assert_eq!(
            canonical_write_tag_name("DocumentName"),
            "IFD0:DocumentName"
        );
        assert_eq!(
            canonical_write_tag_name("IFD0:DocumentName"),
            "IFD0:DocumentName"
        );
    }

    #[test]
    fn bare_page_number_write_targets_ifd0() {
        assert_eq!(canonical_write_tag_name("PageNumber"), "IFD0:PageNumber");
        assert_eq!(
            canonical_write_tag_name("IFD0:PageNumber"),
            "IFD0:PageNumber"
        );
    }

    #[test]
    fn bare_maker_note_safety_write_targets_ifd0() {
        assert_eq!(
            canonical_write_tag_name("MakerNoteSafety"),
            "IFD0:MakerNoteSafety"
        );
        assert_eq!(
            canonical_write_tag_name("IFD0:MakerNoteSafety"),
            "IFD0:MakerNoteSafety"
        );
    }

    #[test]
    fn bare_profile_embed_policy_write_targets_ifd0() {
        assert_eq!(
            canonical_write_tag_name("ProfileEmbedPolicy"),
            "IFD0:ProfileEmbedPolicy"
        );
    }

    #[test]
    fn bare_modify_date_write_targets_ifd0() {
        assert_eq!(canonical_write_tag_name("ModifyDate"), "IFD0:ModifyDate");
        assert_eq!(
            canonical_write_tag_name("IFD0:ModifyDate"),
            "IFD0:ModifyDate"
        );
    }

    // ------------------------------------------------------------------
    // ReadReport / diagnostic sink (Step 13)
    // ------------------------------------------------------------------

    /// A minimal, hand-built JPEG: SOI, one APP1 segment starting `FLIR\0`
    /// but far short of `MIN_FLIR_SEGMENT_LENGTH` (11 bytes), then EOI.
    /// `parse_flir_segment` rejects it with "FLIR segment too short",
    /// exercising the swallow site at `jpeg_helpers.rs`'s
    /// `process_exif_segments` (formerly `let _ = parse_flir_segment(...)`).
    fn jpeg_with_undersized_flir_segment() -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8]; // SOI
        let flir_data = b"FLIR\0"; // 5 bytes, well under MIN_FLIR_SEGMENT_LENGTH
        bytes.push(0xFF);
        bytes.push(0xE1); // APP1
        let len = (flir_data.len() + 2) as u16; // length field includes itself
        bytes.extend_from_slice(&len.to_be_bytes());
        bytes.extend_from_slice(flir_data);
        bytes.push(0xFF);
        bytes.push(0xD9); // EOI
        bytes
    }

    #[test]
    fn malformed_flir_segment_is_recorded_not_swallowed() {
        let reader = TestReader::new(jpeg_with_undersized_flir_segment());
        let mut diagnostics = Vec::new();

        // The read itself still succeeds -- one bad sub-block does not fail
        // an otherwise-parseable JPEG.
        let _metadata = parse_jpeg_metadata_with_diagnostics(
            &reader,
            &mut diagnostics,
            &ReadOptions::default_full_listing(),
        )
        .expect("a bad FLIR segment does not fail the whole JPEG parse");

        assert_eq!(
            diagnostics.len(),
            1,
            "the FLIR failure must be recorded exactly once"
        );
        assert_eq!(diagnostics[0].kind, DiagnosticKind::Warning);
        assert!(
            diagnostics[0].message.contains("FLIR"),
            "diagnostic should name what failed: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn record_diagnostics_first_of_several_warnings_wins_and_all_are_retained() {
        // Matches the pinned oracle on GE.jpg: two distinct warnings, and
        // the bare `Warning` tag is always the first regardless of `-a`
        // (ExifTool.pm:9541-9551's Priority=>0-shaped tie).
        let mut metadata = MetadataMap::new();
        let diagnostics = vec![
            Diagnostic::warning("first problem"),
            Diagnostic::warning("second problem"),
        ];
        record_diagnostics(&mut metadata, &diagnostics);
        assert_eq!(metadata.get_string("File:Warning"), Some("first problem"));

        let occurrences = metadata.occurrences_for("File:Warning");
        assert_eq!(occurrences.len(), 2, "both warnings must be retained");
        assert_eq!(occurrences[0].raw, TagValue::new_string("first problem"));
        assert_eq!(occurrences[1].raw, TagValue::new_string("second problem"));
    }

    #[test]
    fn record_diagnostics_deduplicates_identical_warning_text() {
        // `Warn`'s `WAS_WARNED` dedupes identical text before ever calling
        // `FoundTag` (ExifTool.pm:5629-5636) -- a repeated message is one
        // occurrence, not two.
        let mut metadata = MetadataMap::new();
        record_diagnostics(
            &mut metadata,
            &[
                Diagnostic::warning("same problem"),
                Diagnostic::warning("same problem"),
            ],
        );
        assert_eq!(metadata.occurrences_for("File:Warning").len(), 1);
    }

    #[test]
    fn record_diagnostics_never_overwrites_an_existing_warning_tag() {
        let mut metadata = MetadataMap::new();
        metadata.insert("File:Warning", TagValue::new_string("parser's own warning"));
        record_diagnostics(&mut metadata, &[Diagnostic::warning("sink warning")]);
        assert_eq!(
            metadata.get_string("File:Warning"),
            Some("parser's own warning"),
            "an existing File:Warning must win over the sink's"
        );
    }

    #[test]
    fn record_diagnostics_never_overwrites_an_existing_warning_tag_inserted_after() {
        // The priority-0 promotion rule makes this true in either
        // insertion order, unlike a `contains_key` guard would be.
        let mut metadata = MetadataMap::new();
        record_diagnostics(&mut metadata, &[Diagnostic::warning("sink warning")]);
        metadata.insert("File:Warning", TagValue::new_string("parser's own warning"));
        assert_eq!(
            metadata.get_string("File:Warning"),
            Some("parser's own warning"),
        );
    }

    #[test]
    fn record_diagnostics_files_errors_separately_from_warnings() {
        let mut metadata = MetadataMap::new();
        record_diagnostics(
            &mut metadata,
            &[Diagnostic::warning("w"), Diagnostic::error("e")],
        );
        assert_eq!(metadata.get_string("File:Warning"), Some("w"));
        assert_eq!(metadata.get_string("File:Error"), Some("e"));
    }

    #[test]
    fn record_diagnostics_does_not_surface_refusals_as_tags() {
        // Refusal is reserved for Step 10 and must not masquerade as a
        // Warning/Error tag today.
        let mut metadata = MetadataMap::new();
        record_diagnostics(&mut metadata, &[Diagnostic::refusal("not implemented yet")]);
        assert!(metadata.get_string("File:Warning").is_none());
        assert!(metadata.get_string("File:Error").is_none());
    }

    /// The truncated-JPEG motivating defect, end to end through
    /// `read_metadata_report`. Bytes are embedded, not read from disk: the
    /// first 20 bytes of a real JPEG (SOI, an APP1/Exif segment header
    /// declaring a 0x098c-byte payload, and the start of a TIFF header)
    /// with everything after byte 20 missing.
    ///
    /// The pinned oracle (`/usr/bin/perl5.34 ... exiftool`, ExifTool
    /// 13.59) reports this file as `FileType: JPEG`, `MIMEType:
    /// image/jpeg`, and `Warning: JPEG format error`, exiting 0.
    /// `ExifTool.pm:8483`: `$success or $self->Warn('JPEG format
    /// error');` -- `ProcessJPEG` degrades instead of raising an
    /// exception, which is the model this test holds oxidex to.
    const TRUNCATED_JPEG: &[u8] = &[
        0xff, 0xd8, 0xff, 0xe1, 0x09, 0x8c, 0x45, 0x78, 0x69, 0x66, 0x00, 0x00, 0x49, 0x49, 0x2a,
        0x00, 0x08, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn truncated_jpeg_reports_partial_instead_of_failing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("truncated.jpg");
        std::fs::write(&path, TRUNCATED_JPEG).expect("write truncated JPEG fixture");

        let report = read_metadata_report(&path)
            .expect("a damaged-but-identifiable JPEG must still read Ok");

        assert_eq!(report.status, ParseStatus::Partial);

        // Filesystem tags survived.
        assert!(report.metadata.get_string("File:FileName").is_some());
        assert!(report.metadata.get_string("File:FileSize").is_some());

        // Identity tags survived: the file is still nameable even though
        // its content could not be parsed.
        assert_eq!(report.metadata.get_string("File:FileType"), Some("JPEG"));
        assert_eq!(
            report.metadata.get_string("File:FileTypeExtension"),
            Some("jpg")
        );
        assert_eq!(
            report.metadata.get_string("File:MIMEType"),
            Some("image/jpeg")
        );

        // ExifTool's own wording for this exact failure mode.
        assert_eq!(
            report.metadata.get_string("File:Warning"),
            Some("JPEG format error")
        );

        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].message, "JPEG format error");
        assert_eq!(report.diagnostics[0].kind, DiagnosticKind::Warning);
    }

    #[test]
    fn mie_reports_parsed() {
        let Some(path) = crate::test_support::pinned_t_images_fixture_path("MIE.mie") else {
            eprintln!("skipping: pinned fixture MIE.mie is absent");
            return;
        };

        let report = read_metadata_report(&path).expect("MIE now has a real parser");

        // Step 32 routed `FileFormat::MIE` to `mie.rs`'s standalone-document
        // parser -- this file used to bottom out in `add_identity_tags`
        // (`IdentifiedOnly`) because nothing dispatched it at all.
        assert_eq!(report.status, ParseStatus::Parsed);
        assert_eq!(report.metadata.get_string("File:FileType"), Some("MIE"));
        assert_eq!(
            report.metadata.get_string("MIE-Camera:Make"),
            Some("FUJIFILM")
        );
        assert!(
            report.diagnostics.is_empty(),
            "a clean MIE.mie read should not produce any diagnostics"
        );
    }

    #[test]
    fn samsung_i8910_scalado_app4_matches_pinned_exiftool() {
        let Some(path) =
            crate::test_support::pinned_combined_fixture_path("Samsung/SamsungGT-i8910.jpg")
        else {
            return;
        };
        let reader = crate::io::buffered_reader::BufferedReader::new(&path)
            .expect("read pinned Samsung GT-i8910 fixture");
        let metadata = parse_jpeg_metadata(&reader, &ReadOptions::default_full_listing())
            .expect("parse pinned Samsung fixture");

        assert_eq!(metadata.get_integer("APP4:PreviewImageWidth"), Some(816));
        assert_eq!(metadata.get_integer("APP4:PreviewImageHeight"), Some(459));
        assert_eq!(metadata.get_integer("APP4:PreviewQuality"), Some(85));
    }

    #[test]
    fn test_lookup_tag_name_known_tags() {
        use crate::tag_db::lookup_tag_name;
        assert_eq!(lookup_tag_name(0x010F, "IFD0"), "IFD0:Make");
        assert_eq!(lookup_tag_name(0x0110, "IFD0"), "IFD0:Model");
        assert_eq!(lookup_tag_name(0x0112, "IFD0"), "IFD0:Orientation");
    }

    #[test]
    fn test_lookup_tag_name_unknown_tags() {
        use crate::tag_db::lookup_tag_name;
        // Use tag IDs from unused ranges in the database
        assert_eq!(lookup_tag_name(0xF999, "IFD0"), "IFD0:0xF999");
        assert_eq!(lookup_tag_name(0xF998, "GPS"), "GPS:0xF998");
    }

    #[test]
    fn test_raw_bytes_to_tag_value_string() {
        use crate::parsers::tiff::ifd_parser::ByteOrder;
        let bytes = b"Canon\0";
        // Use tag_id=0x010F (Make tag) instead of 0 to avoid GPS_VERSION_ID special handler
        let value = raw_bytes_to_tag_value(bytes, 2, 1, 0x010F, ByteOrder::LittleEndian); // Type 2 = ASCII
        assert_eq!(value.as_string(), Some("Canon"));
    }

    #[test]
    fn test_raw_bytes_to_tag_value_integer_u16() {
        use crate::parsers::tiff::ifd_parser::ByteOrder;
        let bytes = [0x05, 0x00]; // 5 in little-endian
        // Use tag_id=0x0112 (Orientation) instead of 0
        let value = raw_bytes_to_tag_value(&bytes, 3, 1, 0x0112, ByteOrder::LittleEndian); // Type 3 = SHORT
        assert_eq!(value.as_integer(), Some(5));
    }

    #[test]
    fn test_raw_bytes_to_tag_value_integer_u32() {
        use crate::parsers::tiff::ifd_parser::ByteOrder;
        let bytes = [0x64, 0x00, 0x00, 0x00]; // 100 in little-endian
        // Use tag_id=0x0100 (ImageWidth) instead of 0
        let value = raw_bytes_to_tag_value(&bytes, 4, 1, 0x0100, ByteOrder::LittleEndian); // Type 4 = LONG
        assert_eq!(value.as_integer(), Some(100));
    }

    #[test]
    fn test_raw_bytes_to_tag_value_binary() {
        use crate::parsers::tiff::ifd_parser::ByteOrder;
        let bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x10, 0x20]; // Non-ASCII bytes
        // Use tag_id=0xFFFF which doesn't match any special handlers
        let value = raw_bytes_to_tag_value(&bytes, 7, 1, 0xFFFF, ByteOrder::LittleEndian); // Type 7 = UNDEFINED
        assert!(value.is_binary());
    }

    #[test]
    fn test_tiff_sub_reader_offset_adjustment() {
        let data = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let reader = TestReader::new(data);
        let sub_reader = TiffSubReader::new(&reader, 5);

        // Reading offset 0 from sub_reader should read offset 5 from base
        let result = sub_reader.read(0, 3).unwrap();
        assert_eq!(result, &[5, 6, 7]);

        // Reading offset 2 from sub_reader should read offset 7 from base
        let result = sub_reader.read(2, 2).unwrap();
        assert_eq!(result, &[7, 8]);
    }

    #[test]
    fn test_tiff_sub_reader_size() {
        let data = vec![0; 100];
        let reader = TestReader::new(data);
        let sub_reader = TiffSubReader::new(&reader, 20);

        // Size should be (100 - 20) = 80
        assert_eq!(sub_reader.size(), 80);
    }
}

/// One public-batch transaction that removes a whole group and sets tags in
/// it (`write_metadata_with_removals`: the map with the sets, the group in
/// `removed`). Pinned ExifTool 13.59 deletes first, then sets:
/// `-EXIF:All= -Make=x` leaves a block holding Make, `-ExifIFD:All=
/// -ExifIFD:ISO=200` an ExifIFD holding ISO, `-GPS:All= -GPS:GPSAltitude=50`
/// a GPS IFD holding GPSAltitude. The carrier-wide removal returned an empty
/// plan before looking at the sets, the JPEG and PNG paths dropped the whole
/// EXIF carrier, and `verify_exif_write` accepted the empty output before
/// checking the sets: success, and the set lost (b92d0c44). Group removals
/// cleared the directory the set had just been planned into.
///
/// A block or directory created so carries WriteExif's mandatory entries,
/// as the oracle's does: the fresh block's IFD0 YCbCrPositioning, a
/// re-created ExifIFD's ExifVersion/ComponentsConfiguration/ColorSpace, a
/// re-created GPS IFD's GPSVersionID (WriteExif.pl 13.59:25-50, 714-719).
/// InteropIFD sets are refused (a raw-carried class) where ExifTool
/// creates the directory.
#[cfg(test)]
mod removal_then_set_tests {
    use super::*;
    use crate::parsers::tiff::ifd_parser::ByteOrder;

    fn w16(v: u16, bo: ByteOrder) -> [u8; 2] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }
    fn w32(v: u32, bo: ByteOrder) -> [u8; 4] {
        match bo {
            ByteOrder::LittleEndian => v.to_le_bytes(),
            ByteOrder::BigEndian => v.to_be_bytes(),
        }
    }

    /// IFD0 {Make "Acme", Model "M1", Artist "me", ExifIFD, GPS} -> IFD1
    /// {Compression 6, a 4-byte thumbnail}; ExifIFD {ExposureProgram 2,
    /// ISO 100}; GPS {GPSVersionID 2.3.0.0, GPSAltitudeRef 0}.
    fn block(bo: ByteOrder) -> Vec<u8> {
        let entry = |tag: u16, typ: u16, count: u32, value: [u8; 4]| {
            [
                w16(tag, bo).as_slice(),
                &w16(typ, bo),
                &w32(count, bo),
                &value,
            ]
            .concat()
        };
        let short = |v: u16| {
            let mut b = [0; 4];
            b[..2].copy_from_slice(&w16(v, bo));
            b
        };
        // IFD0 at 8: 5 entries -> 8+2+60+4 = 74; "Acme\0" at 74 (6 with pad)
        // ExifIFD at 80: 2 entries -> 80+2+24+4 = 110
        // GPS at 110: 2 entries -> 140; IFD1 at 140: 3 entries -> 182; thumb 182
        let mut t = match bo {
            ByteOrder::LittleEndian => b"II".to_vec(),
            ByteOrder::BigEndian => b"MM".to_vec(),
        };
        t.extend(w16(42, bo));
        t.extend(w32(8, bo));
        t.extend(w16(5, bo));
        t.extend(entry(0x010F, 2, 5, w32(74, bo)));
        t.extend(entry(0x0110, 2, 3, *b"M1\0\0"));
        t.extend(entry(0x013B, 2, 3, *b"me\0\0"));
        t.extend(entry(0x8769, 4, 1, w32(80, bo)));
        t.extend(entry(0x8825, 4, 1, w32(110, bo)));
        t.extend(w32(140, bo));
        t.extend(b"Acme\0\0");
        assert_eq!(t.len(), 80);
        t.extend(w16(2, bo));
        t.extend(entry(0x8822, 3, 1, short(2)));
        t.extend(entry(0x8827, 3, 1, short(100)));
        t.extend(w32(0, bo));
        assert_eq!(t.len(), 110);
        t.extend(w16(2, bo));
        t.extend(entry(0x0000, 1, 4, [2, 3, 0, 0]));
        t.extend(entry(0x0005, 1, 1, [0, 0, 0, 0]));
        t.extend(w32(0, bo));
        assert_eq!(t.len(), 140);
        t.extend(w16(3, bo));
        t.extend(entry(0x0103, 3, 1, short(6)));
        t.extend(entry(0x0201, 4, 1, w32(182, bo)));
        t.extend(entry(0x0202, 4, 1, w32(4, bo)));
        t.extend(w32(0, bo));
        assert_eq!(t.len(), 182);
        t.extend([0xFF, 0xD8, 0xFF, 0xD9]);
        t
    }

    fn jpeg(tiff: &[u8]) -> Vec<u8> {
        const BODY: &str = "ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
        out.extend(((tiff.len() + 8) as u16).to_be_bytes());
        out.extend(b"Exif\0\0");
        out.extend(tiff);
        out.extend(
            (0..BODY.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&BODY[i..i + 2], 16).unwrap()),
        );
        out
    }

    fn png(tiff: &[u8]) -> Vec<u8> {
        let crc = |data: &[u8]| {
            let mut crc = 0xFFFF_FFFFu32;
            for &byte in data {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 {
                        0xEDB8_8320 ^ (crc >> 1)
                    } else {
                        crc >> 1
                    };
                }
            }
            !crc
        };
        let chunk = |kind: &[u8; 4], data: &[u8]| {
            let body = [kind.as_slice(), data].concat();
            [
                (data.len() as u32).to_be_bytes().as_slice(),
                &body,
                &crc(&body).to_be_bytes(),
            ]
            .concat()
        };
        let ihdr = [0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0];
        let idat = [0x78, 0x9C, 0x63, 0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01];
        [
            b"\x89PNG\r\n\x1a\n".as_slice(),
            &chunk(b"IHDR", &ihdr),
            &chunk(b"eXIf", tiff),
            &chunk(b"IDAT", &idat),
            &chunk(b"IEND", &[]),
        ]
        .concat()
    }

    /// Runs one batch: `removed` plus the `sets` over the map read from the
    /// file. Returns the result, the file bytes before and the map after.
    fn batch(
        dir: &Path,
        name: &str,
        original: &[u8],
        removed: &[&str],
        sets: &[(&str, TagValue)],
    ) -> (Result<()>, MetadataMap) {
        let path = dir.join(name);
        std::fs::write(&path, original).unwrap();
        let mut map = read_metadata(&path).unwrap();
        for (key, value) in sets {
            map.insert(*key, value.clone());
        }
        let removed: Vec<String> = removed.iter().map(|k| k.to_string()).collect();
        let result = write_metadata_with_removals(&path, &map, &removed);
        if result.is_err() {
            assert_eq!(
                std::fs::read(&path).unwrap(),
                original,
                "{name}: refused but written"
            );
        }
        (result, read_metadata(&path).unwrap())
    }

    #[test]
    fn a_set_survives_a_carrier_wide_removal_in_one_batch() {
        let dir = tempfile::tempdir().unwrap();
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let tiff = block(bo);
            for (carrier, original) in [("b.jpg", jpeg(&tiff)), ("b.png", png(&tiff))] {
                for group in ["IFD0:All", "EXIF:All"] {
                    for sets in [
                        // legacy
                        vec![("IFD0:Make", TagValue::new_string("x"))],
                        // generated
                        vec![("IFD0:Artist", TagValue::new_string("you"))],
                        // mixed
                        vec![
                            ("IFD0:Make", TagValue::new_string("x")),
                            ("IFD0:Artist", TagValue::new_string("you")),
                        ],
                    ] {
                        let label = format!(
                            "{bo:?} {carrier} {group} {:?}",
                            sets.iter().map(|s| s.0).collect::<Vec<_>>()
                        );
                        let (result, after) =
                            batch(dir.path(), carrier, &original, &[group], &sets);
                        result.unwrap_or_else(|e| panic!("{label}: {e}"));
                        for (key, value) in &sets {
                            assert_eq!(after.get_string(key), value.as_string(), "{label}: {key}");
                        }
                        // Everything else went with the carrier.
                        for gone in [
                            "IFD0:Model",
                            "ExifIFD:ISO",
                            "ExifIFD:ExposureProgram",
                            "GPS:GPSAltitudeRef",
                        ] {
                            assert!(!after.contains_key(gone), "{label}: {gone} kept");
                        }
                        if !sets.iter().any(|s| s.0 == "IFD0:Make") {
                            assert!(!after.contains_key("IFD0:Make"), "{label}: Make kept");
                        }
                        if !sets.iter().any(|s| s.0 == "IFD0:Artist") {
                            assert!(!after.contains_key("IFD0:Artist"), "{label}: Artist kept");
                        }
                        // The new block's byte order, as the oracle's: MM,
                        // but for a PNG's IFD0:All, which keeps its header.
                        let written =
                            payload_of(&std::fs::read(dir.path().join(carrier)).unwrap()).unwrap();
                        let expected: &[u8] = if carrier == "b.png" && group == "IFD0:All" {
                            &tiff[..2]
                        } else {
                            b"MM"
                        };
                        assert_eq!(&written[..2], expected, "{label}: byte order");
                        // The fresh block's IFD0 is created: WriteExif's
                        // mandatory YCbCrPositioning (WriteExif.pl 13.59:28).
                        assert!(
                            after.contains_key("IFD0:YCbCrPositioning"),
                            "{label}: no mandatory YCbCrPositioning"
                        );
                    }
                }
            }
        }
    }

    /// A TIFF-structured file: pinned ExifTool 13.59 never deletes IFD0
    /// there ("Can't delete IFD0 from TIFF") and still sets Make, so
    /// `IFD0:All` is a no-op and the set lands in place; `EXIF:All`, which
    /// deletes ExifIFD there, is refused (a directory deletion the in-place
    /// TIFF writer cannot make), the file untouched.
    #[test]
    fn a_tiff_file_keeps_ifd0_and_takes_the_set() {
        let dir = tempfile::tempdir().unwrap();
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let tiff = block(bo);
            let (result, after) = batch(
                dir.path(),
                "t.tif",
                &tiff,
                &["IFD0:All"],
                &[("IFD0:Make", TagValue::new_string("xy"))],
            );
            result.unwrap_or_else(|e| panic!("{bo:?}: {e}"));
            assert_eq!(after.get_string("IFD0:Make"), Some("xy"), "{bo:?}");
            assert_eq!(after.get_string("IFD0:Model"), Some("M1"), "{bo:?}");
            let (result, _) = batch(
                dir.path(),
                "t.tif",
                &tiff,
                &["EXIF:All"],
                &[("IFD0:Make", TagValue::new_string("xy"))],
            );
            assert!(result.is_err(), "{bo:?} EXIF:All");
        }
    }

    /// The TIFF payload of a carrier's first EXIF block (JPEG APP1 or PNG
    /// eXIf), or `None`.
    fn payload_of(file: &[u8]) -> Option<Vec<u8>> {
        if file.starts_with(b"\x89PNG") {
            let mut at = 8;
            while at + 8 <= file.len() {
                let len = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
                if &file[at + 4..at + 8] == b"eXIf" {
                    return Some(file[at + 8..at + 8 + len].to_vec());
                }
                at += 12 + len;
            }
            None
        } else {
            crate::writers::exif_surgical::jpeg_exif_payload(file).unwrap()
        }
    }

    /// A carrier the transaction deletes (`IFD0:All` / `EXIF:All`) is never
    /// parsed: an unreadable APP1 / eXIf payload (too short, a bad byte-order
    /// mark, a bad magic number, an empty IFD0) no longer fails the write in
    /// `ifd0_state` or a scan before the fresh block is built (890fca8e
    /// refused every one with a generated set, "IFD count is outside the
    /// file" and the like). Pinned ExifTool 13.59, `-EXIF:All= -IFD0:Artist=x`
    /// on the same payloads (review-head evidence `carrier-malformed-oracle
    /// .txt`): Artist written into a new block, big-endian for a JPEG and
    /// for a PNG under `EXIF:All`; a PNG under `IFD0:All` keeps a readable
    /// `II` mark. One exception, the oracle's: `IFD0:All` keeps a PNG eXIf
    /// too short for a TIFF header, and ExifTool then writes nothing
    /// ("unchanged"); this writer refuses the set there instead of dropping
    /// it. The new block's IFD0 carries the mandatory YCbCrPositioning, as
    /// the oracle's does.
    #[test]
    fn a_deleted_unreadable_carrier_is_never_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let payloads: [(&str, Vec<u8>); 5] = [
            ("short", b"II*".to_vec()),
            ("byte-order", b"XX*\0\x08\0\0\0\0\0\0\0\0\0".to_vec()),
            ("magic", b"II\x2b\0\x08\0\0\0\0\0\0\0\0\0".to_vec()),
            ("empty-II", b"II*\0\x08\0\0\0\0\0\0\0\0\0".to_vec()),
            ("empty-MM", b"MM\0*\0\0\0\x08\0\0\0\0\0\0".to_vec()),
        ];
        for (label, payload) in payloads {
            for (carrier, original) in [("u.jpg", jpeg(&payload)), ("u.png", png(&payload))] {
                for group in ["EXIF:All", "IFD0:All"] {
                    for sets in [
                        vec![("IFD0:Artist", TagValue::new_string("x"))],
                        vec![("IFD0:Make", TagValue::new_string("x"))],
                        vec![
                            ("IFD0:Artist", TagValue::new_string("x")),
                            ("IFD0:Make", TagValue::new_string("x")),
                        ],
                    ] {
                        let case = format!(
                            "{label} {carrier} {group} {:?}",
                            sets.iter().map(|s| s.0).collect::<Vec<_>>()
                        );
                        let (result, after) =
                            batch(dir.path(), carrier, &original, &[group], &sets);
                        if carrier == "u.png" && group == "IFD0:All" && label == "short" {
                            assert!(result.is_err(), "{case}: the oracle writes nothing here");
                            continue;
                        }
                        result.unwrap_or_else(|e| panic!("{case}: {e}"));
                        for (key, value) in &sets {
                            assert_eq!(after.get_string(key), value.as_string(), "{case}: {key}");
                        }
                        let written = payload_of(&std::fs::read(dir.path().join(carrier)).unwrap())
                            .unwrap_or_else(|| panic!("{case}: no EXIF block"));
                        let keeps_ii =
                            carrier == "u.png" && group == "IFD0:All" && payload.starts_with(b"II");
                        let expected: &[u8] = if keeps_ii { b"II" } else { b"MM" };
                        assert_eq!(&written[..2], expected, "{case}: byte order");
                        assert!(
                            after.contains_key("IFD0:YCbCrPositioning"),
                            "{case}: no mandatory YCbCrPositioning"
                        );
                    }
                }
            }
        }
    }

    /// The boundary of the rule above on a PNG: a set in a malformed eXIf
    /// chunk that the same transaction does *not* delete -- no removal, or a
    /// removal short of the carrier (`IFD0:Software`, `ExifIFD:All`) -- is
    /// refused with the file untouched and a message naming the fault
    /// (`png_writer::MalformedExif`), since that write would have to edit
    /// the chunk. Pinned ExifTool 13.59 keeps a markless chunk and adds a
    /// second eXIf, edits a magic-43 chunk in place keeping the 43 (which
    /// this reader does not read), and leaves a too-short chunk unchanged;
    /// see `a_set_in_a_malformed_exif_chunk_is_refused_untouched`
    /// (tests/png_exif_surgical.rs).
    #[test]
    fn a_set_in_a_kept_malformed_exif_chunk_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let payloads: [(&str, Vec<u8>, &str); 3] = [
            ("short", b"II*".to_vec(), "too short for a TIFF header"),
            (
                "byte-order",
                b"XX*\0\x08\0\0\0\0\0\0\0\0\0".to_vec(),
                "does not start with a TIFF byte-order mark",
            ),
            (
                "magic",
                b"II\x2b\0\x08\0\0\0\0\0\0\0\0\0".to_vec(),
                "magic number 43, not 42",
            ),
        ];
        for (label, payload, fault) in payloads {
            let original = png(&payload);
            for removed in [&[][..], &["IFD0:Software"], &["ExifIFD:All"]] {
                let (result, _) = batch(
                    dir.path(),
                    "k.png",
                    &original,
                    removed,
                    &[("IFD0:Artist", TagValue::new_string("x"))],
                );
                let error = result.expect_err(label).to_string();
                assert!(error.contains(fault), "{label} {removed:?}: {error}");
            }
        }
    }

    /// A same-value set after a removal of the same tag, in one batch
    /// (`removed = ["IFD0:Make"]`, then `IFD0:Make=Acme` with Make already
    /// Acme) survives, as pinned ExifTool 13.59's `-IFD0:Make= -IFD0:Make=Acme`
    /// keeps Make. 00f0c398 inferred from the equal value that the row was
    /// carried and deleted it; the map now records what was assigned (a value
    /// the caller inserted, `MetadataMap::is_assigned`), and a
    /// carried row -- the same map without the assignment -- still goes. The same holds under a
    /// carrier removal (`EXIF:All` then `IFD0:Make=Acme`). JPEG and PNG, II
    /// and MM.
    #[test]
    fn a_same_value_set_after_a_removal_survives() {
        let dir = tempfile::tempdir().unwrap();
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let tiff = block(bo);
            for (carrier, original) in [("s.jpg", jpeg(&tiff)), ("s.png", png(&tiff))] {
                for removal in ["IFD0:Make", "EXIF:All", "IFD0:All"] {
                    let label = format!("{bo:?} {carrier} {removal}");
                    let path = dir.path().join(carrier);
                    std::fs::write(&path, &original).unwrap();
                    let map = read_metadata(&path).unwrap();
                    assert_eq!(map.get_string("IFD0:Make"), Some("Acme"), "{label}");
                    let mut assigned = map.clone();
                    assigned.insert("IFD0:Make", TagValue::new_string("Acme"));
                    write_metadata_transaction(&path, &assigned, &[removal.to_string()])
                        .unwrap_or_else(|e| panic!("{label}: {e}"));
                    let after = read_metadata(&path).unwrap();
                    assert_eq!(
                        after.get_string("IFD0:Make"),
                        Some("Acme"),
                        "{label}: Make lost"
                    );
                    if removal != "IFD0:Make" {
                        assert!(!after.contains_key("IFD0:Model"), "{label}: Model kept");
                    }

                    // Carried, not assigned: the removal wins.
                    std::fs::write(&path, &original).unwrap();
                    write_metadata_transaction(&path, &map, &[removal.to_string()])
                        .unwrap_or_else(|e| panic!("{label} carried: {e}"));
                    assert!(
                        !read_metadata(&path).unwrap().contains_key("IFD0:Make"),
                        "{label}: carried Make kept"
                    );
                }
            }
        }
    }

    /// Every entry of IFD1 in a carrier's EXIF block, `(tag, type, count,
    /// value bytes)`, the thumbnail pointer pair left out (its offset is
    /// layout); `None` when there is no IFD1.
    fn ifd1_entries(file: &[u8]) -> Option<Vec<(u16, u16, u32, Vec<u8>)>> {
        let tiff = payload_of(file)?;
        let scan = crate::writers::exif_surgical::scan_exif_entries(&tiff).unwrap();
        let entries: Vec<_> = scan
            .entries
            .iter()
            .filter(|entry| entry.ifd == crate::writers::exif_surgical::IfdKind::Ifd1)
            .map(|entry| {
                (
                    entry.tag_id,
                    entry.field_type,
                    entry.count,
                    entry.value.clone(),
                )
            })
            .collect();
        (!entries.is_empty()).then_some(entries)
    }

    /// One transaction deleting IFD1 (`IFD1:All`) or the whole carrier
    /// (`EXIF:All`) and setting a generated IFD1 tag creates IFD1 anew, so
    /// WriteExif gives it its other %mandatory entries (WriteExif.pl
    /// 13.59:25-50, 714-719). Pinned ExifTool 13.59, one invocation of
    /// `-IFD1:All= -IFD1:XResolution=300` (or `-EXIF:All= ...`), JPEG and
    /// PNG, II and MM: IFD1 = {Compression 6, XResolution 300, YResolution
    /// 72, ResolutionUnit 2}, the thumbnail gone. The transaction seeded
    /// those entries, and then the group-removal verification refused them
    /// as IFD1 content left behind (8b32abd8): a write ExifTool makes,
    /// refused.
    #[test]
    fn a_recreated_ifd1_keeps_its_mandatory_entries() {
        let Some(oracle) = crate::exiftool_oracle::graded() else {
            eprintln!("skipping: no usable ExifTool oracle");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let tiff = block(bo);
            for (carrier, original) in [("r.jpg", jpeg(&tiff)), ("r.png", png(&tiff))] {
                for group in ["IFD1:All", "EXIF:All"] {
                    let label = format!("{bo:?} {carrier} {group}");
                    let (result, _) = batch(
                        dir.path(),
                        carrier,
                        &original,
                        &[group],
                        &[("IFD1:XResolution", TagValue::new_rational(300, 1))],
                    );
                    result.unwrap_or_else(|e| panic!("{label}: {e}"));
                    // Compared in the block: the PNG reader surfaces no
                    // IFD1 row.
                    let ours = std::fs::read(dir.path().join(carrier)).unwrap();

                    let theirs = dir.path().join(format!("oracle-{carrier}"));
                    std::fs::write(&theirs, &original).unwrap();
                    let status = oracle
                        .command()
                        .args(["-q", "-q", "-overwrite_original"])
                        .arg(format!("-{group}="))
                        .arg("-IFD1:XResolution=300")
                        .arg(&theirs)
                        .status()
                        .unwrap();
                    assert!(status.success(), "{label}: oracle failed");
                    let theirs = std::fs::read(&theirs).unwrap();
                    assert_eq!(ifd1_entries(&ours), ifd1_entries(&theirs), "{label}: IFD1");
                    assert_eq!(
                        ifd1_entries(&theirs).map(|e| e.len()),
                        Some(4),
                        "{label}: oracle IFD1"
                    );
                }
            }
        }
    }

    #[test]
    fn a_set_survives_a_group_removal_of_its_own_directory_in_one_batch() {
        let dir = tempfile::tempdir().unwrap();
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let tiff = block(bo);
            for (carrier, original) in [("g.jpg", jpeg(&tiff)), ("g.png", png(&tiff))] {
                let label = format!("{bo:?} {carrier}");
                let (result, after) = batch(
                    dir.path(),
                    carrier,
                    &original,
                    &["ExifIFD:All"],
                    &[("ExifIFD:ISO", TagValue::new_integer(200))],
                );
                result.unwrap_or_else(|e| panic!("{label} ExifIFD: {e}"));
                assert_eq!(after.get_integer("ExifIFD:ISO"), Some(200), "{label}");
                assert!(!after.contains_key("ExifIFD:ExposureProgram"), "{label}");
                assert_eq!(after.get_string("IFD0:Make"), Some("Acme"), "{label}");
                // The ExifIFD the removal deleted is created anew by the set.
                for key in [
                    "ExifIFD:ExifVersion",
                    "ExifIFD:ComponentsConfiguration",
                    "ExifIFD:ColorSpace",
                ] {
                    assert!(after.contains_key(key), "{label}: no mandatory {key}");
                }

                let (result, after) = batch(
                    dir.path(),
                    carrier,
                    &original,
                    &["GPS:All"],
                    &[("GPS:GPSAltitude", TagValue::new_rational(50, 1))],
                );
                result.unwrap_or_else(|e| panic!("{label} GPS: {e}"));
                assert!(
                    after.contains_key("GPS:GPSAltitude"),
                    "{label}: GPSAltitude lost"
                );
                assert!(!after.contains_key("GPS:GPSAltitudeRef"), "{label}");
                assert!(
                    after.contains_key("GPS:GPSVersionID"),
                    "{label}: no mandatory GPSVersionID"
                );
                assert_eq!(after.get_string("IFD0:Make"), Some("Acme"), "{label}");

                // An IFD1 set: honored, never reported done and lost. Read
                // from the block itself: the PNG reader surfaces no IFD1 row.
                // (Its recreated IFD1 is pinned against the oracle in
                // `a_recreated_ifd1_keeps_its_mandatory_entries`.)
                let (result, _) = batch(
                    dir.path(),
                    carrier,
                    &original,
                    &["IFD1:All"],
                    &[("IFD1:XResolution", TagValue::new_rational(300, 1))],
                );
                result.unwrap_or_else(|e| panic!("{label} IFD1: {e}"));
                let written = std::fs::read(dir.path().join(carrier)).unwrap();
                assert!(
                    ifd1_entries(&written)
                        .unwrap_or_default()
                        .iter()
                        .any(|(tag, typ, count, _)| (*tag, *typ, *count) == (0x011a, 5, 1)),
                    "{label}: IFD1 set lost"
                );
            }
        }
    }
}
