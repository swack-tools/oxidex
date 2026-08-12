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
    process_dji_thermal_segments, process_dqt_segments, process_exif_segments,
    process_icc_segments, process_infiray_segments, process_iptc_segments, process_jfif_segments,
    process_mpf_segments, process_photoshop_segments, process_qualcomm_segments,
    process_ricoh_rmeta_segments, process_samsung_unique_id_segments, process_sof_segments,
    process_spiff_segments, process_uniform_resource_name_segments, process_xmp_segments,
};
use crate::core::operations_helpers::{read_u16, read_u32};
use crate::core::read_report::{
    Diagnostic, DiagnosticKind, DiagnosticSink, ParseStatus, ReadReport,
};
#[cfg(test)]
use crate::core::tag_conversion::raw_bytes_to_tag_value;
use crate::core::tiff_helpers::parse_ifd_chain;
use crate::core::validation::{validate_tag_value_intrinsics, validate_tag_value_with_name};
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
use crate::writers::jpeg_writer::write_exif_to_jpeg;
use crate::writers::pdf_writer::write_pdf_file;
use crate::writers::png_writer::write_png_metadata;
use crate::writers::tiff_surgical::rewrite_tiff_file;
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
    if is_placeholder(reported) {
        metadata.insert("File:FileType", TagValue::new_string(id.file_type.as_ref()));
    }

    // The on-disk extension is a placeholder whenever it disagrees with
    // ExifTool's canonical one (`aif` where ExifTool says `aiff`), which is
    // exactly when it should be corrected.
    if ours_is_authoritative {
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

    // Step 4: Route to appropriate parser based on detected format and extract format-specific metadata
    //
    // Detection returning Unknown, or a parser refusing the file, still leaves
    // it identifiable: ExifTool reports FileType/FileTypeExtension/MIMEType for
    // ~40 formats OxiDex has no parser for. Failing the whole read there threw
    // away the file-system metadata too, so those files produced no output at
    // all rather than partial output.
    let format_metadata = match dispatch_format_parser(&reader, format) {
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
    if !metadata.contains_key("File:ExifByteOrder") && format != FileFormat::DR4 {
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

    // Step 4: Dispatch. JPEG and PNG go through their diagnostics-carrying
    // entry points so a recoverable problem inside an otherwise-successful
    // parse surfaces as `Partial` rather than vanishing into stderr.
    let mut diagnostics: DiagnosticSink = Vec::new();
    let dispatch_result = match format {
        FileFormat::JPEG => parse_jpeg_metadata_with_diagnostics(&reader, &mut diagnostics),
        FileFormat::PNG => {
            crate::parsers::png::parse_png_metadata_with_diagnostics(&reader, &mut diagnostics)
        }
        _ => dispatch_format_parser(&reader, format),
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
/// * `Err(ExifToolError)` - Validation failure, I/O error, or unsupported format
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
pub fn write_metadata(path: &Path, metadata: &MetadataMap) -> Result<()> {
    let reader = MMapReader::new(path)?;
    let format = detect_format(&reader)?;

    // The caller-changed set, recovered the same way the JPEG surgical writer
    // recovers it: by re-reading the file being written. See
    // `validate_caller_changes` for why that is both sufficient and necessary.
    let baseline = read_metadata(path).ok();

    // PHASE 1: VALIDATION
    // JPEG and the TIFF-structured formats validate inside their surgical
    // writers, which already have the original bytes to diff against.
    if !matches!(format, FileFormat::JPEG) && !is_surgical_tiff_target(format, &reader) {
        validate_caller_changes(metadata, baseline.as_ref())?;
    }

    // PHASE 2: ROUTE TO APPROPRIATE WRITER
    if is_surgical_tiff_target(format, &reader) {
        let file_bytes = reader.read(0, reader.size() as usize)?;
        let original = baseline.unwrap_or_default();
        let out = rewrite_tiff_file(file_bytes, &original, metadata)?;
        write_atomic(path, &out)?;
        return Ok(());
    }

    match format {
        FileFormat::JPEG => {
            // Use JPEG writer to serialize metadata
            let serialized_bytes = write_exif_to_jpeg(&reader, metadata)?;
            write_atomic(path, &serialized_bytes)?;
        }
        FileFormat::PNG => {
            write_png_metadata(path, &reader, metadata)?;
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
fn is_surgical_tiff_target(format: FileFormat, reader: &dyn FileReader) -> bool {
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
            if has_reliable_value_type(tag_name) {
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
/// - New value fails validation (InvalidTagValue)
/// - File cannot be written (IoError)
pub fn modify_tag(path: &Path, tag_name: &str, new_value: TagValue) -> Result<()> {
    // Step 1: Read existing metadata (preserves all other tags)
    let mut metadata = read_metadata(path)?;

    // Step 2: Modify the single tag
    metadata.insert(canonical_write_tag_name(tag_name), new_value);

    // Step 3: Write all metadata back to file
    write_metadata(path, &metadata)?;

    Ok(())
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
/// * `Ok(())` - Tag was removed (or didn't exist)
/// * `Err` - I/O error or unsupported format
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
pub fn remove_tag(path: &Path, tag_name: &str) -> Result<()> {
    // Step 1: Read existing metadata
    let mut metadata = read_metadata(path)?;

    // Step 2: Remove the tag (if it exists)
    metadata.remove(canonical_write_tag_name(tag_name));

    // Step 3: Write metadata back to file
    write_metadata(path, &metadata)?;

    Ok(())
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
pub fn clear_all_metadata(path: &Path) -> Result<()> {
    // Create empty metadata map
    let metadata = MetadataMap::new();

    // Write empty metadata (format-specific writers handle cleanup)
    write_metadata(path, &metadata)?;

    Ok(())
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
/// - Any tag value fails validation (InvalidTagValue)
/// - Destination file cannot be written (IoError)
pub fn copy_metadata(src: &Path, dest: &Path, tags: Option<&[String]>) -> Result<()> {
    // Step 1: Read metadata from source file
    let source_metadata = read_metadata(src)?;

    // Step 2: Read existing metadata from destination file
    let mut dest_metadata = read_metadata(dest)?;

    // Step 3: Filter and merge source tags into destination metadata
    // Use into_iter() to consume source_metadata and avoid cloning when possible
    for (tag_name, tag_value) in source_metadata {
        // Check if this tag should be copied (if filter is specified)
        let should_copy = tags.is_none_or(|filter| filter.contains(&tag_name));

        if should_copy {
            // Insert tag into destination (merges with existing, preserving others)
            // No clone needed since we own the data from into_iter()
            dest_metadata.insert(tag_name, tag_value);
        }
    }

    // Step 4: Write merged metadata back to destination file
    write_metadata(dest, &dest_metadata)?;

    Ok(())
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
pub(crate) fn parse_jpeg_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let mut diagnostics = Vec::new();
    parse_jpeg_metadata_with_diagnostics(reader, &mut diagnostics)
}

/// Same as [`parse_jpeg_metadata`], but pushes recoverable problems (a
/// malformed XMP packet, an unparseable APP13 Photoshop resource, ...) into
/// `diagnostics` instead of dropping them. [`read_metadata_report_with_detector`]
/// is the only caller that reads `diagnostics` back out; `parse_jpeg_metadata`
/// itself discards them, matching its previous (silent) behavior exactly.
pub(crate) fn parse_jpeg_metadata_with_diagnostics(
    reader: &dyn FileReader,
    diagnostics: &mut DiagnosticSink,
) -> Result<MetadataMap> {
    // Parse JPEG segment structure
    let segments = parse_segments(reader)?;

    let mut metadata = MetadataMap::new();

    // Process different segment types
    process_jfif_segments(&segments, &mut metadata, diagnostics);
    process_exif_segments(&segments, reader, &mut metadata, diagnostics);
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
    if let Ok(file) = reader.read(0, reader.size() as usize) {
        for (key, value) in crate::parsers::jpeg::afcp::parse_afcp_trailer(file).iter() {
            metadata.insert(key.clone(), value.clone());
        }
        for (key, value) in
            crate::parsers::jpeg::fotostation::parse_fotostation_trailer(file).iter()
        {
            metadata.insert(key.clone(), value.clone());
        }
    }

    process_iptc_segments(&segments, &mut metadata, diagnostics);
    process_photoshop_segments(&segments, &mut metadata, diagnostics);
    process_uniform_resource_name_segments(&segments, &mut metadata);
    process_icc_segments(&segments, &mut metadata);
    process_mpf_segments(&segments, &mut metadata);
    // APP2/APP4 FPXR: FlashPix streams split across application segments.
    crate::parsers::jpeg::flashpix::process_fpxr_segments(&segments, &mut metadata);
    process_sof_segments(&segments, &mut metadata);
    process_com_segments(&segments, &mut metadata);
    process_dqt_segments(&segments, &mut metadata);
    process_spiff_segments(&segments, &mut metadata);
    process_ricoh_rmeta_segments(&segments, &mut metadata);

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
    parse_ifd_chain(reader, first_ifd_offset, byte_order, &mut metadata)?;

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
    let mut metadata = parse_jpeg_metadata(&jpeg_reader)?;

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
        let _metadata = parse_jpeg_metadata_with_diagnostics(&reader, &mut diagnostics)
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
    fn mie_reports_identified_only() {
        let path = std::path::Path::new("/tmp/oxidex-exiftool-cache/exiftool/t/images/MIE.mie");
        if !path.is_file() {
            eprintln!("skipping: pinned fixture not present at {}", path.display());
            return;
        }

        let report =
            read_metadata_report(path).expect("MIE identifies even though it has no parser");

        assert_eq!(report.status, ParseStatus::IdentifiedOnly);
        assert_eq!(report.metadata.get_string("File:FileType"), Some("MIE"));
        assert!(
            report.diagnostics.is_empty(),
            "the ~40-format no-parser fallback is not itself a diagnosable problem"
        );
    }

    #[test]
    fn samsung_i8910_scalado_app4_matches_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new(
            "/tmp/oxidex-exiftool-cache/combined-samples/Samsung/SamsungGT-i8910.jpg",
        );
        let reader = crate::io::buffered_reader::BufferedReader::new(path)
            .expect("read pinned Samsung GT-i8910 fixture");
        let metadata = parse_jpeg_metadata(&reader).expect("parse pinned Samsung fixture");

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
