//! JPEG metadata parsing helpers
//!
//! This module contains helper functions for parsing JPEG segment structures
//! and extracting metadata from different segment types (JFIF, EXIF, XMP, IPTC, ICC).

use super::{FileReader, MetadataMap, TagValue};
use crate::core::operations_helpers::read_u32;
use crate::core::read_report::{Diagnostic, DiagnosticSink};
use crate::core::tag_conversion::raw_bytes_to_tag_value;
use crate::core::tiff_helpers::{parse_exif_subifd, parse_gps_subifd};
use crate::exiftool_tables::{decode_binary_table, find_table};
use crate::io::EndianReader;
use crate::parsers::common::print_im::{PRINT_IM_VERSION_TAG, decode_print_im_version};
use crate::parsers::jpeg::app_segments::app8_isothermal::INFIRAY_ISOTHERMAL_MIN_LENGTH;
use crate::parsers::jpeg::app_segments::infiray::{binary_data_placeholder, read_record};
use crate::parsers::jpeg::app_segments::infiray_tables as infiray;
use crate::parsers::jpeg::app_segments::{
    parse_app6_ijpeg, parse_app10_hdr, parse_app11_jpeg_hdr, parse_app12_olympus,
    parse_app12_picture_info, parse_app14_adobe, parse_infiray_isothermal, parse_jumbf,
    parse_meta_app3, parse_photoshop_irb,
};
use crate::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
use crate::parsers::jpeg::quality_estimate::estimate_quality_from_dqt_tables;
use crate::parsers::jpeg::segment_parser::Segment;
use crate::parsers::jpeg::xmp_parser::extract_xmp_from_segments;
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::parsers::tiff::tiff_subreader::TiffSubReader;
use crate::tag_db::lookup_tag_name;

/// Processes JFIF APP0 segments and extracts version and resolution metadata.
///
/// JFIF segments contain basic image information including version, resolution unit,
/// and X/Y resolution values.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with JFIF tags
/// * `diagnostics` - Sink for problems that don't stop the read (a
///   malformed JFXX extension segment is skipped, not fatal)
pub fn process_jfif_segments(
    segments: &[Segment],
    metadata: &mut MetadataMap,
    diagnostics: &mut DiagnosticSink,
) {
    for segment in segments.iter().filter(|s| s.marker == 0xFFE0) {
        // Also try extended APP0 parser for JFXX segments
        if let Err(e) =
            crate::parsers::jpeg::app_parsers::parse_app0_extended(segment.data, metadata)
        {
            diagnostics.push(Diagnostic::warning(format!(
                "Failed to parse JFXX extension in APP0: {e}"
            )));
        }

        // Check if this is a JFIF segment (starts with "JFIF\0")
        if segment.data.len() >= 14 && &segment.data[0..5] == b"JFIF\0" {
            // JFIF structure after identifier:
            // Bytes 5-6: Version (major.minor)
            // Byte 7: Units (0=none, 1=inches, 2=cm)
            // Bytes 8-9: X density (big-endian u16)
            // Bytes 10-11: Y density (big-endian u16)
            let version_major = segment.data[5];
            let version_minor = segment.data[6];
            let units = segment.data[7];

            // JFIF uses big-endian byte order for density values
            let reader = EndianReader::big_endian(segment.data);
            let x_density = reader.u16_at(8).unwrap_or(0);
            let y_density = reader.u16_at(10).unwrap_or(0);

            // JFIFVersion is a printed string, not a quantity.
            // `%Image::ExifTool::JFIF::Main` (ExifTool.pm) declares it
            //
            //     Name => 'JFIFVersion',
            //     Format => 'int8u[2]',
            //     PrintConv => 'sprintf("%d.%.2d", split(" ",$val))',
            //
            // so `exiftool -G1 -s` prints `1.00` for a 1.00 file. A f64 built
            // as `major + minor/100` cannot carry that second digit: `1 + 0/100`
            // is 1.0, and every renderer in this crate turns that back into "1"
            // -- the CLI uses `f64::to_string` (shortest round-tripping form)
            // and the comparison harness `{:.5}` with trailing zeros trimmed.
            // `1.01` and `1.02` survived the round trip by luck, which is why
            // only the 14 corpus files that are version 1.00 ever showed it.
            //
            // Store the same string that the `JPEG:` alias below already
            // builds -- and that the two (dead) JFIF parsers in
            // `parsers/jpeg/app_segments/app0.rs` and
            // `parsers/jpeg/jfif_parser.rs` already assert in their tests.
            let jfif_version = format!("{}.{:02}", version_major, version_minor);
            metadata.insert(
                "JFIF:JFIFVersion".to_string(),
                TagValue::String(jfif_version.clone()),
            );
            // Also add JPEG: prefixed version for format-specific tagging
            metadata.insert(
                "JPEG:JFIFVersion".to_string(),
                TagValue::String(jfif_version),
            );

            let unit_string = match units {
                0 => "None",
                1 => "inches",
                2 => "cm",
                _ => "Unknown",
            };
            metadata.insert(
                "JFIF:ResolutionUnit".to_string(),
                TagValue::String(unit_string.to_string()),
            );
            // Also add JPEG: prefixed version for format-specific tagging
            metadata.insert(
                "JPEG:ResolutionUnit".to_string(),
                TagValue::String(unit_string.to_string()),
            );

            metadata.insert(
                "JFIF:XResolution".to_string(),
                TagValue::Integer(x_density as i64),
            );
            // Also add JPEG: prefixed version for format-specific tagging
            metadata.insert(
                "JPEG:XResolution".to_string(),
                TagValue::Integer(x_density as i64),
            );

            metadata.insert(
                "JFIF:YResolution".to_string(),
                TagValue::Integer(y_density as i64),
            );
            // Also add JPEG: prefixed version for format-specific tagging
            metadata.insert(
                "JPEG:YResolution".to_string(),
                TagValue::Integer(y_density as i64),
            );
        }
    }
}

/// Processes EXIF APP1 segments and extracts TIFF-based EXIF metadata.
///
/// EXIF data is stored in APP1 segments with a TIFF structure containing
/// IFD0, EXIF sub-IFD, and GPS sub-IFD.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `reader` - File reader for accessing full file (needed for offset calculations)
/// * `metadata` - MetadataMap to populate with EXIF tags
/// * `diagnostics` - Sink for problems that don't stop the read (a
///   malformed FLIR/EXIF sub-block is skipped, not fatal to the JPEG read)
pub fn process_exif_segments(
    segments: &[Segment],
    reader: &dyn FileReader,
    metadata: &mut MetadataMap,
    diagnostics: &mut DiagnosticSink,
) {
    // Find all APP1 segments (EXIF/XMP/FLIR)
    let app1_segments: Vec<_> = segments.iter().filter(|s| s.is_app1()).collect();

    // Process each APP1 segment
    for segment in app1_segments {
        // Check if this is a FLIR segment (starts with "FLIR\0")
        if segment.data.len() >= 5 && &segment.data[0..5] == b"FLIR\0" {
            if let Err(e) =
                crate::parsers::jpeg::flir_parser::parse_flir_segment(segment.data, metadata)
            {
                diagnostics.push(Diagnostic::warning(format!("Incomplete FLIR record: {e}")));
            }
            continue;
        }

        // Check if this is a Casio QVCI segment (starts with "QVCI\0")
        //
        // JPEG.pm:58-61: `Condition => '$$valPt =~ /^QVCI\0/'`, `SubDirectory
        // => { TagTable => 'Image::ExifTool::Casio::QVCI' }` -- no `Start`
        // override, so `Casio::QVCI`'s `FIRST_ENTRY => 0` counts from the
        // segment's own first byte (the "QVCI\0" signature itself), not from
        // after it. Verified against `combined-samples/CasioQVCI.jpg`:
        // `ModelType` (table index 0x62=98) sits at file offset 0x18+98=0x7a,
        // which is exactly where `exiftool -v3` shows "KX-778\0".
        if segment.data.len() >= 5 && &segment.data[0..5] == b"QVCI\0" {
            crate::parsers::jpeg::app_parsers::parse_casio_qvci_segment(segment.data, metadata);
            continue;
        }

        // Check if this is an EXIF segment (starts with "Exif\0\0")
        if segment.data.len() >= 6 && &segment.data[0..6] == b"Exif\0\0" {
            // Extract EXIF data starting after the 6-byte header
            let tiff_data = &segment.data[6..];

            if tiff_data.len() < 8 {
                // EXIF data too small for valid TIFF header
                continue;
            }

            // Detect byte order from TIFF header (bytes 0-1)
            let byte_order = if &tiff_data[0..2] == b"II" {
                ByteOrder::LittleEndian
            } else if &tiff_data[0..2] == b"MM" {
                ByteOrder::BigEndian
            } else {
                // Invalid byte order marker
                continue;
            };

            // ExifTool reports the endianness of the EXIF block itself. It is
            // known here and nowhere later, since everything downstream works
            // through an already-configured reader.
            metadata.insert(
                "File:ExifByteOrder",
                TagValue::new_string(byte_order.exif_byte_order_tag()),
            );

            // Read IFD offset from bytes 4-7 (relative to TIFF data start)
            // Create EndianReader with appropriate byte order for the TIFF data
            let tiff_header_reader = match byte_order {
                ByteOrder::LittleEndian => EndianReader::little_endian(tiff_data),
                ByteOrder::BigEndian => EndianReader::big_endian(tiff_data),
            };
            let ifd_offset = tiff_header_reader.u32_at(4).unwrap_or(0) as u64;

            // Create a sub-reader for TIFF data
            // We need to create a wrapper that adjusts offsets to be relative to TIFF start
            let tiff_offset = segment.offset + 10; // Segment offset + marker(2) + length(2) + "Exif\0\0"(6)
            let tiff_reader = TiffSubReader::new(reader, tiff_offset);

            // Parse IFD structure
            match parse_ifd(&tiff_reader, ifd_offset, byte_order) {
                Err(e) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "Failed to parse EXIF IFD0: {e}"
                    )));
                }
                Ok(tags) => {
                    // Process IFD0 tags and get sub-IFD offsets
                    let (exif_ifd_offset, gps_ifd_offset) =
                        process_ifd0_tags(&tags, byte_order, metadata, diagnostics);

                    // Parse EXIF Sub-IFD if present. `tiff_offset` is the absolute
                    // file position of the TIFF header, which ExifTool adds to
                    // stored offsets (e.g. the Interop IFD's OtherImageStart).
                    // `tiff_data.len()` is the APP1 segment's EXIF payload -- what
                    // ExifTool calls `$dataLen` -- and bounds how far a MakerNote
                    // decoder may resolve its own value offsets. `tiff_reader`
                    // itself runs to the end of the file, so this is the tighter
                    // of the two limits and keeps a MakerNote out of the JPEG's
                    // compressed scan data.
                    if let Some(offset) = exif_ifd_offset {
                        parse_exif_subifd(
                            &tiff_reader,
                            offset,
                            byte_order,
                            tiff_offset,
                            tiff_data.len() as u64,
                            metadata,
                        );
                    }

                    // Parse GPS Sub-IFD if present
                    if let Some(offset) = gps_ifd_offset {
                        parse_gps_subifd(&tiff_reader, offset, byte_order, metadata);
                    }

                    // IFD0's on-disk entry count, needed to locate its own
                    // next-IFD pointer (immediately after the last entry).
                    // `tags.len()` is NOT this: parse_ifd silently drops
                    // malformed entries, so it can undercount, which walks the
                    // pointer lookup too few entries into the file and misreads
                    // whatever bytes happen to sit there -- typically the tail of
                    // a skipped entry -- as the next-IFD offset. `parse_ifd`
                    // above already succeeded reading this exact IFD, so this
                    // 2-byte re-read cannot fail in practice; `tags.len()` is
                    // kept only as a defensive fallback, never the primary path.
                    let ifd0_entry_count = crate::parsers::tiff::ifd_parser::ifd_entry_count(
                        &tiff_reader,
                        ifd_offset,
                        byte_order,
                    )
                    .map(|count| count as usize)
                    .unwrap_or(tags.len());

                    // Walk IFD0's next-IFD pointer to IFD1 (the thumbnail IFD), which
                    // carries Compression/ThumbnailOffset/ThumbnailLength/ThumbnailImage.
                    // `tiff_offset` is the absolute file position of the TIFF header,
                    // which ExifTool adds to the stored ThumbnailOffset.
                    crate::core::tiff_helpers::parse_ifd1_thumbnail(
                        &tiff_reader,
                        ifd_offset,
                        ifd0_entry_count,
                        byte_order,
                        tiff_offset,
                        metadata,
                    );

                    // Walk IFD1's next-IFD pointer to IFD2. Leica JPEGs carry a
                    // second, larger preview there under tag 0x0111/0x0117,
                    // named PreviewImageStart/PreviewImageLength (not
                    // StripOffsets/StripByteCounts - see Exif.pm:707-768).
                    // Offsets are TIFF-relative: the helper reads via
                    // `tiff_reader` and uses `tiff_offset` only for the absolute
                    // JpgFromRawStart value ExifTool displays.
                    crate::core::tiff_helpers::parse_ifd2_preview_image(
                        &tiff_reader,
                        ifd_offset,
                        ifd0_entry_count,
                        byte_order,
                        tiff_offset,
                        metadata,
                    );
                }
            }
        }
    }
}

/// Processes IFD0 tags from JPEG EXIF data.
///
/// Extracts tags from the main IFD (IFD0) and identifies pointers to
/// EXIF and GPS sub-IFDs for further processing.
///
/// # Arguments
///
/// * `tags` - Parsed IFD tags
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `metadata` - MetadataMap to populate
/// * `diagnostics` - Sink for problems that don't stop the read (an
///   unparseable embedded ICC profile is skipped, not fatal)
///
/// # Returns
///
/// A tuple of (exif_ifd_offset, gps_ifd_offset) for sub-IFD parsing
fn process_ifd0_tags(
    tags: &[(u16, u16, u32, std::borrow::Cow<[u8]>)],
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
    diagnostics: &mut DiagnosticSink,
) -> (Option<u64>, Option<u64>) {
    let mut exif_ifd_offset = None;
    let mut gps_ifd_offset = None;

    // Convert raw tag data to MetadataMap entries
    for (tag_id, field_type, value_count, raw_bytes) in tags {
        // Convert Cow<[u8]> to &[u8] for processing
        let bytes = raw_bytes.as_ref();

        // Check for EXIF Sub-IFD pointer (tag 0x8769)
        if *tag_id == 0x8769 && bytes.len() >= 4 {
            let offset = read_u32(bytes, byte_order);
            exif_ifd_offset = Some(offset as u64);
            continue; // Don't add the pointer tag to metadata
        }

        // Check for GPS Sub-IFD pointer (tag 0x8825)
        if *tag_id == 0x8825 && bytes.len() >= 4 {
            let offset = read_u32(bytes, byte_order);
            gps_ifd_offset = Some(offset as u64);
            continue; // Don't add the pointer tag to metadata
        }

        // Exif.pm 0xc4a5 is a SubDirectory into PrintIM.pm, not a printable
        // binary tag. ProcessPrintIM validates the directory and exposes only
        // PrintIMVersion by default.
        if *tag_id == 0xC4A5 {
            if let Some(version) = decode_print_im_version(bytes, byte_order) {
                metadata.insert(PRINT_IM_VERSION_TAG, TagValue::new_string(version));
            }
            continue;
        }

        // Check for ICC_Profile (tag 0x8773 = 34675).
        //
        // A JPEG may carry its ICC profile inside IFD0 rather than in an APP2
        // "ICC_PROFILE\0" segment. Exif.pm 0x8773 is a SubDirectory into
        // ICC_Profile::Main, the same table `process_icc_segments` feeds from
        // APP2, so the payload goes to the same parser and lands under the
        // same family-0 "ICC_Profile" group. Panasonic/PanasonicDMC-ZS19.jpg
        // has no APP2 segment at all, yet ExifTool reports a full 39-tag
        // ICC_Profile group from IFD0 alone.
        //
        // 128 is the fixed ICC header size, below which there is no profile
        // to read; `parse_icc_profile_data` rejects anything shorter anyway.
        //
        // This runs before `process_icc_segments`, so an APP2 profile still
        // wins when a file carries both.
        if *tag_id == 0x8773 && bytes.len() >= 128 {
            match crate::parsers::icc::parse_icc_profile_data(bytes) {
                Ok(icc_tags) => {
                    for (tag_name, value) in icc_tags {
                        metadata.insert(format!("ICC_Profile:{}", tag_name), value);
                    }
                }
                Err(e) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "Failed to parse ICC profile in EXIF IFD0: {e}"
                    )));
                }
            }
            // Don't continue - the raw blob is still added below, matching
            // `process_tiff_ifd_tags`'s handling of the same tag in TIFF.
        }

        // Check for IPTC-NAA (tag 0x83BB = 33723).
        //
        // A JPEG may carry its IPTC IIM block inside IFD0 rather than in an
        // APP13 Photoshop resource. ExifTool routes both to ProcessIPTC and
        // prints the datasets under the same family-0 "IPTC" group, so a file
        // like Canon/CanonEOS-1D.jpg -- which has no APP13 at all -- still
        // reports a full Envelope and Application record.
        //
        // The raw block itself is not printed: ExifTool treats IPTC-NAA as a
        // SubDirectory and omits it from a default dump.
        //
        // This runs before `process_iptc_segments`, so an APP13 resource (the
        // MWG-standard location) still wins when a file carries both.
        if *tag_id == 0x83BB && !bytes.is_empty() {
            for (tag_name, value) in
                crate::parsers::jpeg::iptc_parser::extract_iptc_from_block(bytes)
            {
                metadata.insert(tag_name, TagValue::new_string(value));
            }
            continue;
        }

        // Convert tag ID to tag name (IFD0 for main JPEG EXIF)
        let tag_name = lookup_tag_name(*tag_id, "IFD0");

        // Convert raw bytes to TagValue
        let tag_value =
            raw_bytes_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order);

        metadata.insert(tag_name, tag_value);
    }

    (exif_ifd_offset, gps_ifd_offset)
}

/// Processes XMP APP1 segments and extracts XMP metadata.
///
/// XMP (Extensible Metadata Platform) is an XML-based metadata format
/// stored in APP1 segments with "http://ns.adobe.com/xap/1.0/" marker.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with XMP tags
/// * `diagnostics` - Sink for problems that don't stop the read (malformed
///   XMP is skipped, not fatal to the JPEG read)
pub fn process_xmp_segments(
    segments: &[Segment],
    metadata: &mut MetadataMap,
    diagnostics: &mut DiagnosticSink,
) {
    match extract_xmp_from_segments(segments) {
        Ok(xmp_tags) => {
            // Add all XMP tags to metadata
            for (tag_name, value) in xmp_tags {
                // A List keeps its entries apart -- ExifTool reports
                // dc:subject as a list, not one joined string.
                let tag_value = match value {
                    crate::parsers::xmp::rdf_parser::XmpValue::List(values) => {
                        TagValue::Array(values.into_iter().map(TagValue::new_string).collect())
                    }
                    // XMP is a text format and ExifTool prints the property's
                    // characters back verbatim: crs:ProcessVersion "11.0" is
                    // "11.0", not 11, and Device:Camera's "0.321765" keeps all
                    // six digits. Parsing to a number and re-formatting loses
                    // exactly those.
                    crate::parsers::xmp::rdf_parser::XmpValue::Scalar(value) => {
                        TagValue::new_string(value)
                    }
                };
                metadata.insert(tag_name, tag_value);
            }
        }
        Err(e) => {
            // Continue processing (don't fail entire read); the problem is
            // recorded rather than dropped.
            diagnostics.push(Diagnostic::warning(format!("Failed to parse XMP: {e}")));
        }
    }
}

/// Processes IPTC APP13 segments and extracts IPTC metadata.
///
/// IPTC (International Press Telecommunications Council) metadata is
/// stored in APP13 segments and contains fields like keywords, caption, etc.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with IPTC tags
/// * `diagnostics` - Sink for problems that don't stop the read (malformed
///   IPTC is skipped, not fatal to the JPEG read)
pub fn process_iptc_segments(
    segments: &[Segment],
    metadata: &mut MetadataMap,
    diagnostics: &mut DiagnosticSink,
) {
    if let Some(digest) =
        crate::parsers::jpeg::iptc_parser::current_iptc_digest_from_segments(segments)
    {
        metadata.insert("File:CurrentIPTCDigest", TagValue::new_string(digest));
    }

    match crate::parsers::jpeg::iptc_parser::extract_iptc_values_from_segments(segments) {
        Ok(iptc_tags) => {
            // Add all IPTC tags to metadata. Keywords and
            // SupplementalCategories arrive as a TagValue::Array -- they are
            // written as one IIM record per entry, and inserting them one at a
            // time kept only the last.
            //
            // `extract_iptc_values_from_segments` already applied IPTC.pm's
            // Format + PrintConv pipeline (dataset_value_to_string), so the
            // string it returns is ExifTool's final printed value. Re-parsing
            // it as int/float here would strip exactly what that pipeline
            // produced -- e.g. a `digits[N]`-format field's leading zeros.
            for (tag_name, value) in iptc_tags {
                metadata.insert(tag_name, value);
            }
        }
        Err(e) => {
            // Continue processing; the problem is recorded rather than
            // dropped.
            diagnostics.push(Diagnostic::warning(format!(
                "Failed to extract IPTC metadata: {e}"
            )));
        }
    }
}

/// Processes Photoshop APP13 segments and extracts Image Resource Block tags.
///
/// ExifTool routes an APP13 payload beginning with "Photoshop 3.0\0" to
/// `%Photoshop::Main` (ExifTool.pm:8348) and concatenates CONSECUTIVE APP13
/// Photoshop segments before parsing, because a resource may straddle the
/// 64 kB segment limit. The IPTC resource (0x0404) is handled separately by
/// `process_iptc_segments`.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with Photoshop tags
/// * `diagnostics` - Sink for problems that don't stop the read (a
///   malformed APP13 Photoshop resource is skipped, not fatal)
pub fn process_photoshop_segments(
    segments: &[Segment],
    metadata: &mut MetadataMap,
    diagnostics: &mut DiagnosticSink,
) {
    const APP13_MARKER: u16 = 0xFFED;
    const PHOTOSHOP_HEADER: &[u8] = b"Photoshop 3.0\0";
    const ADOBE_CM_HEADER: &[u8] = b"Adobe_CM";

    // Join runs of consecutive Photoshop APP13 segments, dropping the
    // repeated header on every continuation segment.
    let mut combined: Vec<u8> = Vec::new();
    let flush =
        |combined: &mut Vec<u8>, metadata: &mut MetadataMap, diagnostics: &mut DiagnosticSink| {
            if combined.is_empty() {
                return;
            }
            match parse_photoshop_irb(combined) {
                Ok(photoshop_metadata) => {
                    for (key, value) in photoshop_metadata.iter() {
                        metadata.insert(key.clone(), value.clone());
                    }
                }
                Err(e) => {
                    diagnostics.push(Diagnostic::warning(format!(
                        "Failed to parse APP13 Photoshop segment: {e}"
                    )));
                }
            }
            combined.clear();
        };

    for segment in segments.iter() {
        if segment.marker == APP13_MARKER
            && let Some(body) = segment.data.strip_prefix(ADOBE_CM_HEADER)
            && let Some(value) = body.get(..2)
        {
            metadata.insert(
                "APP13:AdobeCMType".to_string(),
                TagValue::Integer(u16::from_be_bytes([value[0], value[1]]) as i64),
            );
        }

        let is_photoshop =
            segment.marker == APP13_MARKER && segment.data.starts_with(PHOTOSHOP_HEADER);
        if !is_photoshop {
            flush(&mut combined, metadata, diagnostics);
            continue;
        }
        if combined.is_empty() {
            combined.extend_from_slice(segment.data);
        } else {
            combined.extend_from_slice(&segment.data[PHOTOSHOP_HEADER.len()..]);
        }
    }
    flush(&mut combined, metadata, diagnostics);
}

/// Processes APP3 "Meta" segments and extracts Kodak Meta IFD metadata.
///
/// ExifTool routes an APP3 payload matching `/^(Meta|META|Exif)\0\0/` to
/// `%Kodak::Meta` (ExifTool.pm:7990), a TIFF directory with its own tag ids.
/// Tags land in the `Meta:` family, ExifTool's family-0 group for that table.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with Meta tags
pub fn process_app3_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const APP3_MARKER: u16 = 0xFFE3;

    for segment in segments.iter().filter(|s| s.marker == APP3_MARKER) {
        // APP3 also carries Stim and other payloads; a non-Meta identifier
        // is not an error, just not this parser's directory.
        let Ok(meta) = parse_meta_app3(segment.data) else {
            continue;
        };
        for (key, value) in meta.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    if metadata.get_string("IFD0:Make") != Some("DJI") {
        return;
    }

    let mut thermal_len = 0usize;
    for (index, segment) in segments.iter().enumerate() {
        if segment.marker != APP3_MARKER {
            continue;
        }
        thermal_len += segment.data.len();
        if !segments
            .get(index + 1)
            .is_some_and(|next| next.marker == APP3_MARKER)
        {
            metadata.insert(
                "APP3:ThermalData".to_string(),
                binary_data_placeholder(thermal_len),
            );
            thermal_len = 0;
        }
    }
}

/// ExifTool.pm:7997-8127 — a preview JPEG embedded directly in APP2/APP3
/// (optionally continued into APP4/APP5), found by byte-pattern rather than
/// an offset/length pair. No IFD context means ExifTool's FoundTag defaults
/// the displayed group to File.
///
/// - APP2: payload optionally prefixed by the literal bytes `QVGA\0` or
///   `BGTH`, followed by `\xFF\xD8\xFF` and one of `\xDB`/`\xE0`/`\xE1`. The
///   prefix (if any) is stripped before storing. Emitted immediately unless
///   the next segment is also APP2, in which case consecutive APP2 payloads
///   accumulate (Samsung/HP/BenQ APP2 preview).
/// - APP3: payload starting `\xFF\xD8\xFF\xDB` (no prefix) is the preview in
///   full. Continues into APP4 if the immediately following segment is APP4
///   (Samsung S1060-style split preview).
/// - APP4: only relevant as a continuation of an APP3 preview in progress;
///   its payload is appended and the tag emitted unless the next segment is
///   APP5, in which case the preview continues accumulating there.
/// - APP5: only relevant as a further continuation of an APP3/APP4 preview
///   in progress (BenQ DC E1050); its payload is appended and the tag is
///   always emitted immediately -- there is no further continuation marker
///   past APP5.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with `File:PreviewImage`
pub fn extract_direct_preview_image(segments: &[Segment], metadata: &mut MetadataMap) {
    let mut preview: Option<Vec<u8>> = None;

    for (index, segment) in segments.iter().enumerate() {
        let next_marker = segments.get(index + 1).map(|s| s.marker);

        match segment.marker {
            APP2_MARKER => {
                let mut matched_pattern = false;
                for prefix in [&b""[..], b"QVGA\0", b"BGTH"] {
                    if let Some(rest) = segment.data.strip_prefix(prefix) {
                        if rest.starts_with(b"\xff\xd8\xff")
                            && matches!(rest.get(3), Some(0xdb | 0xe0 | 0xe1))
                        {
                            preview = Some(rest.to_vec());
                            matched_pattern = true;
                            break;
                        }
                    }
                }
                // ExifTool.pm:7929-7997: APP2 also carries ICC_PROFILE, FPXR,
                // MPF, InfiRay's IJPEG version header and an "urn:" (Apple
                // HDR) payload, each handled by its own `elsif` branch ahead
                // of the `elsif ($preview)` continuation fallback. None of
                // those identifiers is preview continuation data, so a
                // segment carrying one must not be appended to an
                // in-progress preview even though it also fails the preview
                // byte pattern above.
                let is_other_app2_payload = !matched_pattern
                    && (segment.data.starts_with(b"ICC_PROFILE\0")
                        || segment.data.starts_with(b"FPXR\0")
                        || segment.data.starts_with(b"MPF\0")
                        || segment.data.get(4..10) == Some(b"IJPEG\0".as_slice())
                        || segment.data.starts_with(b"urn:"));
                if !matched_pattern && !is_other_app2_payload {
                    if let Some(existing) = preview.as_mut() {
                        existing.extend_from_slice(segment.data);
                    }
                }
                if preview.is_some() && next_marker != Some(APP2_MARKER) {
                    metadata.insert(
                        "File:PreviewImage",
                        TagValue::new_binary(preview.take().unwrap()),
                    );
                }
            }
            APP3_MARKER => {
                if segment.data.starts_with(b"\xff\xd8\xff\xdb") {
                    preview = Some(segment.data.to_vec());
                }
                if preview.is_some() && next_marker != Some(APP4_MARKER) {
                    metadata.insert(
                        "File:PreviewImage",
                        TagValue::new_binary(preview.take().unwrap()),
                    );
                }
            }
            APP4_MARKER => {
                // ExifTool.pm:8116-8127: "continued Samsung S1060 preview
                // from APP3" -- appended unconditionally, but only emitted
                // if the *next* segment isn't APP5 ("BenQ DC E1050 continues
                // preview in APP5").
                if let Some(existing) = preview.as_mut() {
                    existing.extend_from_slice(segment.data);
                }
                if preview.is_some() && next_marker != Some(APP5_MARKER) {
                    metadata.insert(
                        "File:PreviewImage",
                        TagValue::new_binary(preview.take().unwrap()),
                    );
                }
            }
            APP5_MARKER => {
                // ExifTool.pm:8146-8151 (BenQ DC E1050 continuing a preview
                // from APP4 into APP5): appends and emits unconditionally in
                // the same statement -- unlike APP2/APP3/APP4, there is no
                // further "does the run continue" check; APP5 is always the
                // last segment of this preview mechanism. Only relevant as a
                // continuation of an already-in-progress preview.
                if let Some(mut existing) = preview.take() {
                    existing.extend_from_slice(segment.data);
                    metadata.insert("File:PreviewImage", TagValue::new_binary(existing));
                }
            }
            _ => {}
        }
    }
}

/// Processes MPF (Multi-Picture Format) APP2 segments.
///
/// MPF is used in dual-camera phones and 3D cameras to store multiple images
/// in a single JPEG file. MPF segments are identified by the "MPF\x00" marker.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with MPF tags
pub fn process_mpf_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    for segment in segments.iter().filter(|s| s.marker == 0xFFE2) {
        // Check if this is an MPF segment (starts with "MPF\0")
        if segment.data.len() >= 4 && &segment.data[0..4] == b"MPF\0" {
            // MPImageStart is stored relative to the MPF TIFF header and
            // ExifTool rebases it there (MPF.pm:148 `IsOffset => '$val'`,
            // resolved against `$$dirInfo{Base}`, which ExifTool.pm:7958 sets
            // to the APP2 segment's data position + 4). `Segment::offset` is
            // the position of the 0xFFE2 marker, so the header sits 2 marker
            // bytes + 2 length bytes + 4 identifier bytes further on.
            let tiff_base = segment.offset + 8;
            match crate::parsers::jpeg::mpf_parser::parse_mpf_segment(
                segment.data,
                tiff_base,
                metadata,
            ) {
                Ok(()) => {
                    // Successfully parsed MPF data
                }
                Err(e) => {
                    // Log error but continue processing
                    eprintln!("Warning: Failed to parse MPF segment: {}", e);
                }
            }
        }
    }
}

/// Extracts the Apple HDR Uniform Resource Name stored directly in JPEG APP2.
///
/// ExifTool 13.59's `JPEG::Main` table matches an APP2 payload only when it
/// begins with `urn:`.  The matching payload is exposed as a string, whose
/// NUL padding is not part of the displayed value.
pub fn process_uniform_resource_name_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    for segment in segments.iter().filter(|s| s.marker == APP2_MARKER) {
        if !segment.data.starts_with(b"urn:") {
            continue;
        }

        let value = segment
            .data
            .split(|&byte| byte == 0)
            .next()
            .expect("split always yields the first field");
        let Ok(value) = std::str::from_utf8(value) else {
            continue;
        };

        metadata.insert(
            "JPEG:UniformResourceName",
            TagValue::String(value.to_owned()),
        );
    }
}

/// Processes ICC profile APP2 segments and extracts color profile metadata.
///
/// ICC (International Color Consortium) profiles describe the color
/// characteristics of an image. Profiles larger than one APP2 segment
/// (~64KB) are split into chunks carrying a 1-based sequence number and a
/// total count; chunks are reassembled with IccChunkAssembler before parsing.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with ICC profile tags
pub fn process_icc_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    let icc_segments: Vec<&Segment> = segments
        .iter()
        .filter(|s| s.marker == 0xFFE2 && s.data.len() >= 14 && &s.data[0..12] == b"ICC_PROFILE\0")
        .collect();
    if icc_segments.is_empty() {
        return;
    }

    // Fast path: single-chunk profile parses in place, no reassembly copy.
    if icc_segments.len() == 1 && icc_segments[0].data[12] == 1 && icc_segments[0].data[13] == 1 {
        insert_icc_tags(&icc_segments[0].data[14..], metadata);
        return;
    }

    let mut assembler = IccChunkAssembler::new();
    for segment in &icc_segments {
        if let Err(e) = assembler.add_chunk(segment.data) {
            eprintln!("Warning: Invalid ICC profile chunk: {}", e);
            // ExifTool warns and keeps the FIRST profile when duplicate
            // "chunk 1 of 1" segments collide (the previous oxidex release
            // kept the last). Approximate that by falling back to the
            // first segment whose header marks it chunk 1 of 1, instead of
            // dropping every ICC tag.
            if let Some(seg) = icc_segments
                .iter()
                .find(|s| s.data[12] == 1 && s.data[13] == 1)
            {
                insert_icc_tags(&seg.data[14..], metadata);
            }
            return;
        }
    }
    if !assembler.is_complete() {
        eprintln!(
            "Warning: Incomplete multi-chunk ICC profile ({} of {:?} chunks), skipping",
            assembler.chunk_count(),
            assembler.expected_total()
        );
        return;
    }
    match assembler.assemble() {
        Ok(profile) => insert_icc_tags(&profile, metadata),
        Err(e) => eprintln!("Warning: Failed to assemble ICC profile: {}", e),
    }
}

/// Parses raw ICC profile bytes and inserts ICC_Profile-prefixed tags.
fn insert_icc_tags(icc_data: &[u8], metadata: &mut MetadataMap) {
    match crate::parsers::icc::parse_icc_profile_data(icc_data) {
        Ok(icc_tags) => {
            for (tag_name, value) in icc_tags {
                metadata.insert(format!("ICC_Profile:{}", tag_name), value);
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to parse ICC profile: {}", e);
        }
    }
}

/// Processes SOF (Start of Frame) segments and extracts File-level dimension metadata.
///
/// SOF segments contain image dimensions, color information, and encoding details
/// extracted from the JPEG frame header.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with File-level tags
pub fn process_sof_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    // SOF markers range from 0xFFC0 to 0xFFCF (excluding 0xFFC4, 0xFFC8, 0xFFCC)
    const SOF_MARKERS: [u16; 13] = [
        0xFFC0, 0xFFC1, 0xFFC2, 0xFFC3, 0xFFC5, 0xFFC6, 0xFFC7, 0xFFC9, 0xFFCA, 0xFFCB, 0xFFCD,
        0xFFCE, 0xFFCF,
    ];

    for segment in segments.iter() {
        if SOF_MARKERS.contains(&segment.marker) {
            // Parse SOF segment using the app_parsers module
            let _ = crate::parsers::jpeg::app_parsers::parse_sof_segment(
                segment.marker,
                segment.data,
                metadata,
            );
            // Only process the first SOF segment found
            break;
        }
    }
}

/// Processes APP6 segments and extracts EPPIM, GoPro GPMF, TDHD, or NITF metadata.
///
/// APP6 segments (marker 0xFFE6) are dispatched on the same identifier
/// conditions ExifTool uses: Toshiba PrintIM ("EPPIM\0"), GoPro GPMF
/// ("GoPro\0"), HP/Toshiba TDHD ("TDHD\x01\0\0\0"), and NITF ("NITF\0").
/// Unrecognized APP6 payloads extract nothing.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with APP6 tags
pub fn process_app6_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const APP6_MARKER: u16 = 0xFFE6;
    let is_ijpeg = has_ijpeg_header(segments);
    for segment in segments.iter().filter(|s| s.marker == APP6_MARKER) {
        match parse_app6_ijpeg(segment.data, is_ijpeg) {
            Ok(app6_metadata) => {
                for (key, value) in app6_metadata.iter() {
                    metadata.insert(key.clone(), value.clone());
                }
            }
            Err(e) => {
                // APP6 data is optional; parse failures are not fatal
                eprintln!("Warning: Failed to parse APP6 segment: {}", e);
            }
        }
    }
}

/// Process APP10 segments to extract HDR gain curve data
pub fn process_app10_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    // APP10 marker is 0xFFEA
    const APP10_MARKER: u16 = 0xFFEA;

    for segment in segments.iter().filter(|s| s.marker == APP10_MARKER) {
        // Attempt to parse as HDR gain curve data
        match parse_app10_hdr(segment.data) {
            Ok(hdr_metadata) => {
                // Merge HDR metadata into the main metadata map
                for (key, value) in hdr_metadata.iter() {
                    metadata.insert(key.clone(), value.clone());
                }
            }
            Err(e) => {
                // Log warning but continue processing other segments
                // HDR data is optional, so parse failures are not fatal
                eprintln!("Warning: Failed to parse APP10 HDR segment: {}", e);
            }
        }
    }
}

/// Processes APP11 segments and extracts JPEG-HDR and JUMBF metadata.
///
/// APP11 segments (marker 0xFFEB) carry two unrelated payloads, and ExifTool
/// (ExifTool.pm, APP11 branch) splits them exactly two ways:
///
/// - a payload starting with "HDR_RI" is JPEG-HDR (High Dynamic Range) tone
///   mapping data;
/// - a payload matching `JP..` and at least 16 bytes long is a JUMBF chunk -
///   the container C2PA / CAI provenance metadata and JPEG XT box metadata ride
///   in.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate
///
/// # Extracted Tags
///
/// - JPEG-HDR:Version - Format version
/// - JPEG-HDR:Alpha/Beta - Tone mapping coefficients
/// - JPEG-HDR:Ln0/Ln1 - Luminance bounds
/// - JPEG-HDR:CorrectionMethod - HDR correction method
/// - JPEG-HDR:RatioImageSize - Size of embedded ratio image
/// - JUMBF:* - box descriptions plus the flattened JSON/CBOR payloads
///
/// # JUMBF chunking
///
/// A single JUMBF box is routinely split across several APP11 segments, so the
/// chunks are collected for the whole file and handed to [`parse_jumbf`] in one
/// go rather than parsed segment by segment.
pub fn process_app11_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    // APP11 marker is 0xFFEB
    const APP11_MARKER: u16 = 0xFFEB;

    // JPEG-HDR identifier, including the trailing space.
    //
    // ExifTool, JPEG.pm splits APP11 two ways and has no third case:
    //
    //     APP11 => [{
    //         Name => 'JPEG-HDR',
    //         Condition => '$$valPt =~ /^HDR_RI /',
    //         ...
    //       }, {
    //         Name => 'JUMBF',
    //         Condition => '$$valPt =~ /^JP/',
    //
    // so anything that is not `HDR_RI ` belongs to the JUMBF collector.
    const HDR_RI_PREFIX: &[u8] = b"HDR_RI ";

    let mut jumbf_payloads: Vec<&[u8]> = Vec::new();

    for segment in segments.iter().filter(|s| s.marker == APP11_MARKER) {
        if segment.data.starts_with(HDR_RI_PREFIX) {
            match parse_app11_jpeg_hdr(segment.data) {
                Ok(hdr_metadata) => {
                    // Merge JPEG-HDR metadata into the main metadata map
                    for (key, value) in hdr_metadata.iter() {
                        metadata.insert(key.clone(), value.clone());
                    }
                }
                Err(e) => {
                    // Log warning but continue processing other segments
                    // JPEG-HDR data is optional, so parse failures are not fatal
                    eprintln!("Warning: Failed to parse APP11 JPEG-HDR segment: {}", e);
                }
            }
        } else {
            jumbf_payloads.push(segment.data);
        }
    }

    if jumbf_payloads.is_empty() {
        return;
    }

    match parse_jumbf(&jumbf_payloads) {
        Ok(jumbf_metadata) => {
            for (key, value) in jumbf_metadata.iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
        Err(e) => {
            // JUMBF data is optional; a malformed box must not fail the file.
            eprintln!("Warning: Failed to parse APP11 JUMBF segments: {}", e);
        }
    }
}

/// Processes APP12 segments and extracts manufacturer-specific metadata.
///
/// APP12 segments (marker 0xFFEC) contain various proprietary metadata formats:
/// - Olympus Picture Info (cameras store camera settings and serial numbers)
/// - Ducky (Adobe Photoshop "Save for Web" quality settings)
/// - "Picture Info" text (Agfa, Polaroid and others)
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with manufacturer-specific tags
///
/// # Identifier Dispatch
///
/// ExifTool (ExifTool.pm:8338) splits APP12 exactly two ways: a payload
/// starting with "Ducky" goes to `%APP12::Ducky`, and EVERYTHING else goes to
/// `%APP12::PictureInfo`, whose scan simply finds nothing in a segment that
/// holds no `tag=value` text. OxiDex keeps one extra branch ahead of that for
/// the binary Olympus APP12 layout, which has its own dedicated parser:
/// - "OLYM"/"OLYMP"/"OLYMPUS" prefix -> Olympus parser
/// - "Ducky" prefix -> `parse_ducky_segment`
/// - anything else -> Picture Info scan
///
/// # Error Handling
///
/// Parse errors for individual segments are logged as warnings but do not
/// prevent processing of remaining segments. This ensures robust handling
/// of files with partially corrupt or unsupported APP12 data.
pub fn process_app12_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    // APP12 marker is 0xFFEC
    const APP12_MARKER: u16 = 0xFFEC;

    for segment in segments.iter().filter(|s| s.marker == APP12_MARKER) {
        // Dispatch to appropriate parser based on identifier prefix
        // We need at least 4-5 bytes to identify the format

        if segment.data.len() < 4 {
            // Segment too short to identify, skip it
            continue;
        }

        // Check for Olympus identifier ("OLYM" or "OLYMP" prefix).
        // "[picture info]" is deliberately NOT routed here: that is the
        // ordinary textual Picture Info layout ExifTool scans with
        // ProcessAPP12, and the Olympus parser expects binary data.
        let is_olympus = segment.data.starts_with(b"OLYM");

        // Check for Ducky identifier (handled by existing parser in app_parsers.rs)
        let is_ducky = segment.data.starts_with(b"Ducky");

        if is_olympus {
            // Parse Olympus Picture Info segment
            match parse_app12_olympus(segment.data) {
                Ok(olympus_metadata) => {
                    // Merge Olympus metadata into the main metadata map
                    for (key, value) in olympus_metadata.iter() {
                        metadata.insert(key.clone(), value.clone());
                    }
                }
                Err(e) => {
                    // Log warning but continue processing
                    // Olympus data may have variations that our parser doesn't handle
                    eprintln!("Warning: Failed to parse APP12 Olympus segment: {}", e);
                }
            }
        } else if is_ducky {
            // Ducky segments are already handled by the existing parse_ducky_segment
            // function in app_parsers.rs. We call it here for consistency.
            let _ = crate::parsers::jpeg::app_parsers::parse_ducky_segment(segment.data, metadata);
        } else {
            // Every remaining APP12 payload is scanned as "Picture Info";
            // one with no tag=value text yields nothing, matching ExifTool.
            match parse_app12_picture_info(segment.data) {
                Ok(picture_info) => {
                    for (key, value) in picture_info.iter() {
                        metadata.insert(key.clone(), value.clone());
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse APP12 Picture Info segment: {}", e);
                }
            }
        }
    }
}

/// Processes APP14 segments and extracts Adobe DCT encoding metadata.
///
/// APP14 segments (marker 0xFFEE) contain Adobe-specific metadata when they
/// start with the "Adobe" identifier. This includes information about the
/// DCT encoding version and color transformation used.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with APP14 Adobe tags
///
/// # Extracted Tags
///
/// - APP14:DCTEncodeVersion - Version of the DCT encoder
/// - APP14:APP14Flags0 - First set of encoding flags
/// - APP14:APP14Flags1 - Second set of encoding flags
/// - APP14:ColorTransform - Color transformation type (Unknown, YCbCr, or YCCK)
///
/// # Color Transform Values
///
/// The ColorTransform field is critical for proper JPEG decoding:
/// - 0 = Unknown (RGB or CMYK, context-dependent)
/// - 1 = YCbCr (standard JPEG color space for RGB images)
/// - 2 = YCCK (CMYK encoded as YCCK)
///
/// # Error Handling
///
/// Parse errors for individual segments are logged as warnings but do not
/// prevent processing of remaining segments. Segments without the "Adobe"
/// identifier are silently skipped.
pub fn process_app14_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    // APP14 marker is 0xFFEE
    const APP14_MARKER: u16 = 0xFFEE;

    // Adobe identifier that marks an APP14 segment as Adobe-format
    const ADOBE_IDENTIFIER: &[u8] = b"Adobe";

    for segment in segments.iter().filter(|s| s.marker == APP14_MARKER) {
        // Check if this is an Adobe APP14 segment (starts with "Adobe")
        if segment.data.len() >= ADOBE_IDENTIFIER.len()
            && &segment.data[..ADOBE_IDENTIFIER.len()] == ADOBE_IDENTIFIER
        {
            match parse_app14_adobe(segment.data) {
                Ok(adobe_metadata) => {
                    // Merge APP14 Adobe metadata into the main metadata map
                    for (key, value) in adobe_metadata.iter() {
                        metadata.insert(key.clone(), value.clone());
                    }
                }
                Err(e) => {
                    // Log warning but continue processing other segments
                    // APP14 data is optional, so parse failures are not fatal
                    eprintln!("Warning: Failed to parse APP14 Adobe segment: {}", e);
                }
            }
        }
        // Non-Adobe APP14 segments are silently ignored - they may contain
        // other proprietary data that we don't support yet.
    }
}

/// Processes GraphicConverter APP15 segments.
///
/// GraphicConverter records the JPEG quality it saved with in APP15 as the
/// four bytes `Q<space><digits>`. ExifTool reads it in the JPEG marker loop
/// (ExifTool.pm:8423-8427):
///
/// ```text
/// } elsif ($marker == 0xef) {         # APP15 (GraphicConverter)
///     if ($$segDataPt =~ /^Q\s*(\d+)/ and $length == 4) {
///         $dumpType = 'GraphicConverter';
///         my $tagTablePtr = GetTagTable('Image::ExifTool::JPEG::GraphConv');
///         $self->HandleTag($tagTablePtr, 'Q', $1);
///     }
/// }
/// ```
///
/// `$length` is `length $$segDataPt` (ExifTool.pm:7697), the payload after the
/// two length bytes -- the same bytes `Segment::data` holds -- so the exact
/// four-byte test carries over unchanged. It is what keeps this off the other
/// APP15 payload in the wild, the `TEXT\0` block Casio and FujiFilm write
/// (JPEG.pm:310), which ExifTool deliberately leaves unread.
///
/// `%Image::ExifTool::JPEG::GraphConv` (JPEG.pm:665-670) is
/// `GROUPS => { 0 => 'APP15', 1 => 'GraphConv', 2 => 'Image' }` with the
/// single entry `'Q' => 'Quality'`, and declares no conversion, so the digits
/// are reported verbatim.
pub fn process_app15_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    // APP15 marker is 0xFFEF
    const APP15_MARKER: u16 = 0xFFEF;
    /// ExifTool's `$length == 4` guard on the payload.
    const GRAPHIC_CONVERTER_LENGTH: usize = 4;

    for segment in segments.iter().filter(|s| s.marker == APP15_MARKER) {
        if segment.data.len() != GRAPHIC_CONVERTER_LENGTH {
            continue;
        }
        // /^Q\s*(\d+)/ -- the leading Q, optional whitespace, then at least
        // one digit. Perl's \s is [ \t\n\f\r\x0b]; Rust's
        // `u8::is_ascii_whitespace` leaves out the vertical tab, so spell the
        // class out rather than approximate it.
        let is_perl_space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0c | 0x0b);
        let Some(rest) = segment.data.strip_prefix(b"Q") else {
            continue;
        };
        let digits: &[u8] = rest
            .iter()
            .position(|b| !is_perl_space(*b))
            .map_or(&[], |start| &rest[start..]);
        let end = digits
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(digits.len());
        if end == 0 {
            continue;
        }
        // At most three digits survive the four-byte length test, so this
        // cannot overflow.
        let quality: i64 = digits[..end]
            .iter()
            .fold(0, |acc, b| acc * 10 + i64::from(b - b'0'));
        metadata.insert("APP15:Quality".to_string(), TagValue::Integer(quality));
    }
}

/// Processes JPEG COM (comment) segments, and the APP10 "UNICODE" comment
/// variant that JPEG.pm's `%Main` table declares as the very same `Comment`
/// tag (`APP10 => [{ Name => 'Comment', Condition => '$$valPt =~
/// /^UNICODE\0/' }, ...]`, JPEG.pm:260-268) -- see
/// [`crate::parsers::jpeg::app_parsers::parse_app10_unicode_comment_segment`].
///
/// COM segments (marker 0xFFFE) carry free-form comment text. ExifTool exposes
/// them as File:Comment with trailing NULs stripped; when several segments --
/// of either kind -- are present the *first one in the file* wins
/// (`Comment`'s Extra-table `Priority => 0`, `ExifTool.pm:1311-1315`, "to
/// preserve order of JPEG COM segments" -- see
/// [`crate::parsers::jpeg::app_parsers::parse_comment_segment`]). Every
/// segment is still recorded, so later ones remain reachable as retained,
/// non-winning occurrences rather than being overwritten.
///
/// Both kinds are gathered and sorted by [`Segment::offset`] before either is
/// parsed, rather than each being handled by its own single-marker pass in
/// whatever fixed order this module happens to call them in: the two share
/// one `Priority => 0` tag, so which one is genuinely first *in the file* is
/// exactly what decides the winner, and only true byte position answers
/// that. `ExifTool.jpg`'s own layout is why this matters -- its APP10
/// Unicode segment (offset ~18228) precedes its COM segment (offset
/// ~21931), so the Unicode comment must win despite COM being the more
/// commonly-checked case.
pub fn process_com_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const COM_MARKER: u16 = 0xFFFE;
    const APP10_MARKER: u16 = 0xFFEA;
    const UNICODE_PREFIX: &[u8] = b"UNICODE\0";

    let mut comment_segments: Vec<&Segment> = segments
        .iter()
        .filter(|s| {
            s.marker == COM_MARKER
                || (s.marker == APP10_MARKER && s.data.starts_with(UNICODE_PREFIX))
        })
        .collect();
    comment_segments.sort_by_key(|s| s.offset);

    for segment in comment_segments {
        if segment.marker == COM_MARKER {
            let _ =
                crate::parsers::jpeg::app_parsers::parse_comment_segment(segment.data, metadata);
        } else {
            let _ = crate::parsers::jpeg::app_parsers::parse_app10_unicode_comment_segment(
                segment.data,
                metadata,
            );
        }
    }
}

/// Processes DQT (Define Quantization Table) segments into a quality estimate.
///
/// Collects DQT payloads indexed by table id (first byte & 0x0F, ids 0-3,
/// later segments overwrite earlier ones — ExifTool.pm DQT handler) and emits
/// File:JPEGQualityEstimate. ExifTool computes this tag only when explicitly
/// requested; oxidex has no tag-request mechanism and always emits it (see
/// tests/integration/KNOWN_DISCREPANCIES.md).
pub fn process_dqt_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const DQT_MARKER: u16 = 0xFFDB;
    let mut dqt_list: [Option<&[u8]>; 4] = [None, None, None, None];
    for segment in segments.iter().filter(|s| s.marker == DQT_MARKER) {
        if segment.data.is_empty() {
            continue;
        }
        let table_id = (segment.data[0] & 0x0F) as usize;
        if table_id < 4 {
            dqt_list[table_id] = Some(segment.data);
        }
    }
    if let Some(quality) = estimate_quality_from_dqt_tables(&dqt_list) {
        metadata.insert(
            "File:JPEGQualityEstimate".to_string(),
            TagValue::Integer(quality),
        );
    }
}

/// Processes APP8 SPIFF segments.
///
/// Matching ExifTool, only 32-byte payloads starting with "SPIFF\0" are
/// treated as SPIFF headers; other APP8 payloads (InfiRay, SEAL, ...) are
/// left alone.
pub fn process_spiff_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const APP8_MARKER: u16 = 0xFFE8;
    let is_ijpeg = has_ijpeg_header(segments);
    for segment in segments.iter().filter(|s| s.marker == APP8_MARKER) {
        // The 32-byte/"SPIFF\0" gate is intentionally duplicated in
        // parse_spiff_segment as defense-in-depth; its own length/identifier
        // error paths are therefore unreachable from production callers by
        // design, not dead code.
        if segment.data.len() == 32 && segment.data.starts_with(b"SPIFF\0") {
            let _ = crate::parsers::jpeg::app_parsers::parse_spiff_segment(segment.data, metadata);
            continue;
        }
        // ExifTool falls through to InfiRay's isothermal record for any APP8
        // of at least 32 bytes in an IJPEG file (ExifTool.pm:8215).
        if is_ijpeg && segment.data.len() >= INFIRAY_ISOTHERMAL_MIN_LENGTH {
            for (key, value) in parse_infiray_isothermal(segment.data).iter() {
                metadata.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Extracts Ricoh's standard APP5 `RMETA` menu fields.
///
/// `Ricoh.pm::ProcessRicohRMETA` stores parallel NUL-delimited tag names and
/// big/little-endian `int16u` menu values in section types 1 and 3.  Each
/// emitted value below is a complete PrintConv map from the pinned
/// `Ricoh::RMETA` table.
pub fn process_ricoh_rmeta_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const DIRECTIONS: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    const SIGN_TYPES: [&str; 3] = ["Directional", "Warning", "Information"];
    const LOCATIONS: [&str; 4] = ["Verge", "Gantry", "Central reservation", "Roundabout"];
    const LIT_VALUES: [&str; 2] = ["Yes", "No"];
    const CONDITIONS: [&str; 4] = ["Good", "Fair", "Poor", "Damaged"];

    for data in segments
        .iter()
        .filter(|segment| segment.marker == APP5_MARKER)
        .map(|segment| segment.data)
        .filter(|data| data.starts_with(b"RMETA\0") && data.len() >= 20)
    {
        let little_endian = match data.get(6..8) {
            Some(b"II") => true,
            Some(b"MM") => false,
            _ => continue,
        };
        let read_u16 = |bytes: &[u8]| {
            let pair = [bytes[0], bytes[1]];
            if little_endian {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        };
        if read_u16(&data[10..12]) != 0 {
            continue;
        }
        let directory_offset = read_u16(&data[14..16]) as usize;
        let Some(count_at) = 6usize.checked_add(directory_offset) else {
            continue;
        };
        let Some(count_bytes) = data.get(count_at..count_at + 2) else {
            continue;
        };
        let count = read_u16(count_bytes) as usize;
        if count > 100 {
            continue;
        }
        let Some(mut pos) = count_at.checked_add(10) else {
            continue;
        };
        let mut names: Vec<&[u8]> = Vec::new();
        let mut numbers = Vec::new();
        while let Some(header) = data.get(pos..pos + 4) {
            let section_type = read_u16(&header[..2]);
            let stored_size = read_u16(&header[2..]) as usize;
            if stored_size < 2 {
                break;
            }
            let payload_size = stored_size - 2;
            pos += 4;
            let Some(section) = data.get(pos..pos + payload_size) else {
                break;
            };
            match section_type {
                1 => names = section.split(|byte| *byte == 0).take(count).collect(),
                3 if section.len() >= count.saturating_mul(2) => {
                    numbers = section[..count * 2].chunks_exact(2).map(read_u16).collect();
                }
                _ => {}
            }
            pos += payload_size;
        }

        for (index, name) in names.iter().enumerate() {
            let Some(value) = numbers.get(index).copied() else {
                continue;
            };
            let mapped = if *name == b"Sign type" {
                value
                    .checked_sub(1)
                    .and_then(|index| SIGN_TYPES.get(index as usize))
                    .map(|value| ("APP5:SignType", *value))
            } else if *name == b"Location" {
                value
                    .checked_sub(1)
                    .and_then(|index| LOCATIONS.get(index as usize))
                    .map(|value| ("APP5:Location", *value))
            } else if *name == b"Lit" {
                value
                    .checked_sub(1)
                    .and_then(|index| LIT_VALUES.get(index as usize))
                    .map(|value| ("APP5:Lit", *value))
            } else if *name == b"Condition" {
                value
                    .checked_sub(1)
                    .and_then(|index| CONDITIONS.get(index as usize))
                    .map(|value| ("APP5:Condition", *value))
            } else if *name == b"Azimuth" {
                value
                    .checked_sub(1)
                    .and_then(|value| DIRECTIONS.get(value as usize))
                    .map(|value| ("APP5:Azimuth", *value))
            } else {
                None
            };
            if let Some((key, value)) = mapped {
                metadata.insert(key, TagValue::new_string(value));
            }
        }
    }
}

/// Whether this file is an InfiRay IJPEG, i.e. carries an APP2 segment
/// matching ExifTool's `/^....IJPEG\0/s` version header.
///
/// ExifTool records this as `$$self{HasIJPEG}` while walking the segments
/// (ExifTool.pm:7968) and later uses it as the ONLY gate for the InfiRay
/// APP6/APP7/APP8/APP9 records, which carry no identifier of their own.
fn has_ijpeg_header(segments: &[Segment]) -> bool {
    segments
        .iter()
        .filter(|s| s.marker == APP2_MARKER)
        .any(|s| is_ijpeg_version_header(s.data))
}

const APP2_MARKER: u16 = 0xFFE2;
const APP3_MARKER: u16 = 0xFFE3;
const APP4_MARKER: u16 = 0xFFE4;
const APP5_MARKER: u16 = 0xFFE5;
const APP7_MARKER: u16 = 0xFFE7;
const APP9_MARKER: u16 = 0xFFE9;

/// Extracts Samsung's `ssuniqueid\0` APP5 record.
///
/// ExifTool 13.59 handles this before the other APP5 layouts: the eleven-byte
/// identifier is followed by the 32-byte unique ID, whose `ValueConv` is
/// `unpack("H*", $val)`.  Hex encoding produces the same lowercase rendering.
pub fn process_samsung_unique_id_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    const SAMSUNG_UNIQUE_ID_PREFIX: &[u8] = b"ssuniqueid\0";

    for segment in segments.iter().filter(|s| s.marker == APP5_MARKER) {
        let Some(unique_id) = segment.data.strip_prefix(SAMSUNG_UNIQUE_ID_PREFIX) else {
            continue;
        };
        metadata.insert(
            "Samsung:UniqueID".to_string(),
            TagValue::new_string(hex::encode(unique_id)),
        );
    }
}

/// ExifTool's `/^....IJPEG\0/s`: four bytes of version, then the signature.
///
/// This is checked after ICC_PROFILE, FPXR and MPF in ExifTool's APP2 chain,
/// but none of those three can also carry `IJPEG\0` at offset 4 -- their own
/// identifiers occupy those bytes -- so the signature alone is exact.
fn is_ijpeg_version_header(data: &[u8]) -> bool {
    data.len() >= infiray::VERSION_MIN_LENGTH && &data[4..10] == b"IJPEG\0"
}

/// APP4 selectors before DJI ThermalParams2 in ExifTool 13.59's single
/// `if`/`elsif` chain (ExifTool.pm:8060-8098).
fn app4_precedes_dji_thermal_params(data: &[u8]) -> bool {
    (data.starts_with(b"SCALADO\0") && data.len() >= 16)
        || data.starts_with(b"Qualcomm Dual Camera Attributes")
        || data.starts_with(b"FPXR\0")
}

fn app4_precedes_dji_thermal_params2(data: &[u8]) -> bool {
    app4_precedes_dji_thermal_params(data) || data.starts_with(&[0xaa, 0x55, 0x12, 0x06])
}

/// The byte offset selected by ExifTool's DJI ThermalParams2 APP4 branch.
fn dji_thermal_params2_offset(data: &[u8]) -> Option<usize> {
    if app4_precedes_dji_thermal_params2(data) {
        return None;
    }
    if data.get(64..68) == Some(&[0x2c, 0x01, 0x20, 0x00][..]) {
        Some(32)
    } else if data.get(32..36) == Some(&[0x2c, 0x01, 0x20, 0x00][..]) {
        Some(0)
    } else {
        None
    }
}

/// Whether an APP4 payload is consumed before ExifTool reaches its later
/// InfiRay `HasIJPEG` branch (ExifTool.pm:8060-8113).
fn app4_precedes_infiray(data: &[u8], make_is_dji: bool) -> bool {
    app4_precedes_dji_thermal_params(data)
        || (make_is_dji
            && (data.starts_with(&[0xaa, 0x55, 0x12, 0x06])
                || dji_thermal_params2_offset(data).is_some()
                || (data.len() >= 36 && data[32..36] == [0xaa, 0x55, 0x38, 0x00])))
}

/// Processes the InfiRay IJPEG records carried in APP2, APP3, APP4, APP5,
/// APP7 and APP9 (`Image::ExifTool::InfiRay`).
///
/// The sibling APP6 MixMode and APP8 Isothermal records are read by
/// [`process_app6_segments`] and [`process_spiff_segments`], which already own
/// those markers for other formats.
///
/// None of these records carries an identifier: ExifTool reads them only once
/// an APP2 segment has set `$$self{HasIJPEG}`, and then only when the segment
/// clears a per-marker minimum length. Both gates are generated into
/// [`infiray`] from ExifTool's own source.
pub fn process_infiray_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    if !has_ijpeg_header(segments) {
        return;
    }

    // The APP2 version header is the one record with a signature of its own.
    for segment in segments.iter().filter(|s| s.marker == APP2_MARKER) {
        if !is_ijpeg_version_header(segment.data) {
            continue;
        }
        merge(
            read_record("APP2", segment.data, infiray::VERSION),
            metadata,
        );
    }

    // The identifier-less binary-data records, each gated on its own minimum
    // segment length.
    const RECORDS: [(u16, &str, &[infiray::Field], usize); 4] = [
        (
            APP4_MARKER,
            "APP4",
            infiray::FACTORY,
            infiray::FACTORY_MIN_LENGTH,
        ),
        (
            APP5_MARKER,
            "APP5",
            infiray::PICTURE,
            infiray::PICTURE_MIN_LENGTH,
        ),
        (
            APP7_MARKER,
            "APP7",
            infiray::OP_MODE,
            infiray::OP_MODE_MIN_LENGTH,
        ),
        (
            APP9_MARKER,
            "APP9",
            infiray::SENSOR,
            infiray::SENSOR_MIN_LENGTH,
        ),
    ];
    let make_is_dji = metadata.get_string("IFD0:Make") == Some("DJI");
    for (marker, group, table, min_length) in RECORDS {
        for segment in segments.iter().filter(|s| s.marker == marker) {
            if segment.data.len() < min_length {
                continue;
            }
            if marker == APP4_MARKER && app4_precedes_infiray(segment.data, make_is_dji) {
                continue;
            }
            // ExifTool dispatches `ssuniqueid\0` before its HasIJPEG APP5
            // branch, so this Samsung record must never be decoded as an
            // unrelated InfiRay Picture table.
            if marker == APP5_MARKER && segment.data.starts_with(b"ssuniqueid\0") {
                continue;
            }
            // ExifTool tests the Qualcomm signature before it reaches the
            // `HasIJPEG` branch that selects InfiRay's APP7 (ExifTool.pm:8230
            // vs 8238), so a Qualcomm segment is never read as an OpMode
            // record even in a file that also carries an IJPEG header.
            if marker == APP7_MARKER
                && crate::parsers::jpeg::app_segments::qualcomm::is_qualcomm_app7(segment.data)
            {
                continue;
            }
            merge(read_record(group, segment.data, table), metadata);
        }
    }

    process_infiray_imaging_data(segments, metadata);
}

/// Processes the Qualcomm "Camera Attributes" record carried in JPEG APP7
/// (`Image::ExifTool::Qualcomm::Main`).
///
/// ExifTool selects this reader on the payload signature alone
/// (ExifTool.pm:8230), so unlike the InfiRay records sharing this marker it
/// needs no whole-file gate. Segments belonging to the other APP7 readers --
/// Pentax, Ricoh, Huawei, DJI-DBG, InfiRay -- do not carry the signature and
/// are left alone.
pub fn process_qualcomm_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    for segment in segments.iter().filter(|s| s.marker == APP7_MARKER) {
        merge(
            crate::parsers::jpeg::app_segments::parse_qualcomm_app7(segment.data),
            metadata,
        );
    }
}

/// Processes DJI's bracketed debug records carried in APP7.
pub fn process_dji_dbg_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    for segment in segments.iter().filter(|s| s.marker == APP7_MARKER) {
        merge(
            crate::parsers::jpeg::app_segments::parse_dji_dbg_app7(segment.data),
            metadata,
        );
    }
}

/// Reads DJI's APP3/APP5 opaque thermal payloads and APP4 ThermalParams2.
///
/// ExifTool selects `DJI::ThermalParams2` only for a DJI image when the APP4
/// payload has its `2c 01 20 00` signature after either 32 or 64 bytes.  The
/// optional first 32 bytes are a prefix, so the table begins at byte 32 when
/// the signature is at byte 64.  Its temperature is a little-endian float at
/// table offset 0, rendered by DJI.pm as `sprintf("%.1f C", $val)`. Relative
/// humidity is a float at table offset 12, rendered as
/// `sprintf("%g %%", $val * 100)`.
pub fn process_dji_thermal_segments(
    segments: &[Segment],
    metadata: &mut MetadataMap,
    diagnostics: &mut DiagnosticSink,
) {
    if metadata.get_string("IFD0:Make") != Some("DJI") {
        return;
    }

    // JPEG.pm routes APP3 to Meta, Stim, and JPS before falling through to
    // DJI's opaque ThermalData payload. The Mavic 2 fixture splits that one
    // payload over eleven consecutive APP3 segments, exactly as ProcessJPEG
    // reassembles multi-segment JPEG application data.
    let is_other_app3_payload = |data: &[u8]| {
        data.starts_with(b"Meta\0\0")
            || data.starts_with(b"META\0\0")
            || data.starts_with(b"Exif\0\0")
            || data.starts_with(b"Stim\0")
            || data.starts_with(b"_JPSJPS_")
    };
    let mut thermal_data_len = 0usize;
    for (index, segment) in segments.iter().enumerate() {
        if segment.marker != APP3_MARKER || is_other_app3_payload(segment.data) {
            continue;
        }
        thermal_data_len += segment.data.len();
        let run_continues = segments
            .get(index + 1)
            .is_some_and(|next| next.marker == APP3_MARKER && !is_other_app3_payload(next.data));
        if !run_continues {
            metadata.insert(
                "APP3:ThermalData".to_string(),
                binary_data_placeholder(thermal_data_len),
            );
            thermal_data_len = 0;
        }
    }

    // APP5's earlier JPEG.pm routes (RMETA, SamsungUniqueID, and IJPEG) take
    // precedence over DJI's fallback ThermalCalibration entry.
    if !has_ijpeg_header(segments) {
        for segment in segments.iter().filter(|segment| {
            segment.marker == APP5_MARKER
                && !segment.data.starts_with(b"RMETA\0")
                && !segment
                    .data
                    .windows(b"ssuniqueid\0".len())
                    .any(|w| w == b"ssuniqueid\0")
        }) {
            metadata.insert(
                "APP5:ThermalCalibration".to_string(),
                binary_data_placeholder(segment.data.len()),
            );
        }
    }

    let Some(table) = find_table("DJI", "ThermalParams2") else {
        return;
    };

    for segment in segments.iter().filter(|s| s.marker == APP4_MARKER) {
        let data = segment.data;
        let Some(table_offset) = dji_thermal_params2_offset(data) else {
            continue;
        };

        // Every `DJI::ThermalParams2` field is `Omitted::NONE` in the
        // generated table, so `emit` never refuses here -- `refusals` below
        // is expected to stay empty for this table, but the wiring is the
        // same for any table that does withhold fields (Step 10/13 seam).
        let decode =
            decode_binary_table(table, &data[table_offset..], crate::io::ByteOrder::Little);
        if let Some(diagnostic) = Diagnostic::refusals("DJI", "ThermalParams2", decode.refusals()) {
            diagnostics.push(diagnostic);
        }
        for decoded in decode.fields() {
            // Step 15's compiler translates the temperature and distance
            // PrintConvs (`sprintf("%.1f C",$val)` / `sprintf("%.1f m",$val)`),
            // so `emit()` hands back the rendered string. These arms used to
            // re-format the raw float themselves because the generated table
            // carried `PrintConv::None`; matching on Float now silently drops
            // the tag. Take the generated rendering, as Emissivity already did.
            let value = match (decoded.field.name, decoded.emit()) {
                (
                    "AmbientTemperature"
                    | "ReflectedTemperature"
                    | "ObjectDistance"
                    | "Emissivity"
                    | "IDString",
                    Some(TagValue::String(rendered)),
                ) => Some(rendered),
                _ => None,
            };
            if let Some(value) = value {
                metadata.insert(
                    format!("APP4:{}", decoded.field.name),
                    TagValue::String(value),
                );
            }
        }

        let humidity = decode
            .fields()
            .iter()
            .find(|field| field.field.name == "RelativeHumidity")
            .and_then(|field| match field.emit() {
                Some(TagValue::Float(value)) => Some(value),
                _ => None,
            });
        if let Some(humidity) = humidity {
            metadata.insert(
                "APP4:RelativeHumidity".to_string(),
                TagValue::String(format!("{} %", perl_sprintf_g(humidity * 100.0))),
            );
        }
    }
}

/// Formats a number with Perl/C `sprintf("%g")` semantics.
///
/// ExifTool's conversion uses the platform C formatter, so doing the same is
/// the exact port: six significant digits by default, C exponent spelling,
/// and the same rounding behavior. Perl normalizes negative zero when it is
/// multiplied by positive 100, which is reproduced before calling `snprintf`.
fn perl_sprintf_g(value: f64) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    // `libc::c_char`, not a literal `i8`: it is signed on x86_64 and UNSIGNED
    // on aarch64, so hardcoding either side compiles on one architecture and
    // fails on the other. This was `[0_i8; 32]`, which broke every arm64
    // build -- caught by the Docker dry run, since arm64 is only built there
    // and in release.yml.
    let mut output = [0 as libc::c_char; 32];
    // SAFETY: `output` is writable for its full reported size, the format is a
    // static NUL-terminated C string with one `%g`, and `value` has the
    // required promoted `double` type. A default-precision rendering of any
    // finite f64, infinity or NaN fits comfortably in 32 bytes.
    let length =
        unsafe { libc::snprintf(output.as_mut_ptr(), output.len(), c"%g".as_ptr(), value) };
    debug_assert!(length >= 0 && (length as usize) < output.len());
    let length = usize::try_from(length).unwrap_or(0).min(output.len() - 1);
    let bytes = &output[..length];
    // `%g` emits ASCII digits, punctuation, exponent markers, or the
    // implementation's ASCII inf/nan spelling. `as u8` is a no-op where
    // c_char is already unsigned and a reinterpret where it is signed; both
    // are correct for ASCII.
    String::from_utf8(bytes.iter().map(|byte| *byte as u8).collect()).expect("C %g output is ASCII")
}
/// Emits `APP3:ImagingData`, the InfiRay IR + thermal + visible payload.
///
/// `JPEG::Main` declares it `Binary => 1` (JPEG.pm:119-123), so ExifTool
/// prints a byte count rather than the bytes.
///
/// A record larger than one segment is split across consecutive APP3
/// segments, which ExifTool concatenates in file order and processes once the
/// run ends (`$nextMarker == $marker`, ExifTool.pm:8035). There is no chunk
/// header and no index or checksum byte to misread -- unlike FLIR's APP1,
/// whose reassembly was the subject of #321 -- the payloads simply abut.
fn process_infiray_imaging_data(segments: &[Segment], metadata: &mut MetadataMap) {
    // ExifTool tests the earlier APP3 identifiers first, and a payload that
    // matches one of them takes that branch instead.
    fn is_imaging_data(data: &[u8]) -> bool {
        !(data.starts_with(b"Meta\0\0")
            || data.starts_with(b"META\0\0")
            || data.starts_with(b"Exif\0\0")
            || data.starts_with(b"Stim\0")
            || data.starts_with(b"_JPSJPS_"))
    }

    let mut run_len = 0usize;
    let mut previous_index: Option<usize> = None;
    for (index, segment) in segments.iter().enumerate() {
        if segment.marker != APP3_MARKER || !is_imaging_data(segment.data) {
            continue;
        }
        // A gap in segment indices ends the run and starts a new record.
        if previous_index != Some(index.wrapping_sub(1)) {
            run_len = 0;
        }
        run_len += segment.data.len();
        previous_index = Some(index);

        // Emit at the end of each run. Later runs overwrite earlier ones, as
        // ExifTool's own tag store keeps one value per key.
        let run_continues = segments
            .get(index + 1)
            .is_some_and(|next| next.marker == APP3_MARKER);
        if !run_continues {
            metadata.insert(
                "APP3:ImagingData".to_string(),
                binary_data_placeholder(run_len),
            );
        }
    }
}

/// Copies every entry of `source` into `metadata`.
fn merge(source: MetadataMap, metadata: &mut MetadataMap) {
    for (key, value) in source.iter() {
        metadata.insert(key.clone(), value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ExifTool.jpg`'s own shape: an APP10 Unicode comment segment earlier
    /// in the file than a COM segment. Both are the same `Priority => 0`
    /// `Comment` tag, so the *earlier byte offset* must win regardless of
    /// which marker type this module happens to build its result vector
    /// from first -- a fixed per-type processing order would get this
    /// backwards, which is exactly the bug this test pins.
    #[test]
    fn process_com_segments_orders_app10_unicode_and_com_by_true_file_offset() {
        let mut app10_payload = b"UNICODE\0".to_vec();
        for unit in "unicode wins".encode_utf16() {
            app10_payload.extend_from_slice(&unit.to_be_bytes());
        }
        let com_payload = b"com loses\0";

        let segments = vec![
            // COM appears earlier in this Vec, but later in the file --
            // `offset` must be what decides the winner, not Vec position.
            Segment::new(0xFFFE, 500, com_payload),
            Segment::new(0xFFEA, 100, &app10_payload),
        ];

        let mut metadata = MetadataMap::new();
        process_com_segments(&segments, &mut metadata);

        assert_eq!(
            metadata.get_string("File:Comment"),
            Some("unicode wins"),
            "the segment at the lower file offset must win, not the one first in the Vec"
        );
        assert_eq!(metadata.occurrences_for("File:Comment").len(), 2);
    }

    #[test]
    fn process_com_segments_ignores_non_unicode_app10_payloads() {
        // HDR gain-curve data (a disjoint APP10 payload shape this module
        // handles separately, see `process_app10_segments`) must never be
        // mistaken for a Comment.
        let hdr_payload = b"HDR\0some gain curve bytes";
        let segments = vec![Segment::new(0xFFEA, 0, hdr_payload)];
        let mut metadata = MetadataMap::new();
        process_com_segments(&segments, &mut metadata);
        assert!(!metadata.contains_key("File:Comment"));
    }

    fn dji_thermal_params2(relative_humidity: f32) -> Vec<u8> {
        let mut payload = vec![0; 68];
        payload[12..16].copy_from_slice(&relative_humidity.to_le_bytes());
        payload[32..36].copy_from_slice(&[0x2c, 0x01, 0x20, 0x00]);
        payload
    }

    fn dji_metadata() -> MetadataMap {
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Make", TagValue::new_string("DJI"));
        metadata
    }

    #[test]
    fn dji_relative_humidity_uses_perl_general_number_format() {
        for (raw, expected) in [
            (0.123_456_78_f32, "12.3457 %"),
            (1e-7_f32, "1e-05 %"),
            (-0.0_f32, "0 %"),
        ] {
            let payload = dji_thermal_params2(raw);
            let segment = Segment::new(APP4_MARKER, 0, &payload);
            let mut metadata = dji_metadata();

            process_dji_thermal_segments(&[segment], &mut metadata, &mut Vec::new());

            assert_eq!(
                metadata.get_string("APP4:RelativeHumidity"),
                Some(expected),
                "raw humidity {raw:?}"
            );
        }
    }

    #[test]
    fn dji_thermal_params2_does_not_claim_an_earlier_fpxr_app4_branch() {
        let mut payload = dji_thermal_params2(0.5);
        payload[..5].copy_from_slice(b"FPXR\0");
        let segment = Segment::new(APP4_MARKER, 0, &payload);
        let mut metadata = dji_metadata();

        process_dji_thermal_segments(&[segment], &mut metadata, &mut Vec::new());

        assert_eq!(metadata.get_string("APP4:RelativeHumidity"), None);
        assert_eq!(metadata.get_string("APP4:AmbientTemperature"), None);
    }

    #[test]
    fn infiray_does_not_claim_an_earlier_dji_thermal_params2_app4_branch() {
        let mut version = vec![0; infiray::VERSION_MIN_LENGTH];
        version[4..10].copy_from_slice(b"IJPEG\0");
        let mut thermal = dji_thermal_params2(0.5);
        thermal.resize(infiray::FACTORY_MIN_LENGTH, 0);
        let segments = [
            Segment::new(APP2_MARKER, 0, &version),
            Segment::new(APP4_MARKER, 0, &thermal),
        ];
        let mut metadata = dji_metadata();

        process_infiray_segments(&segments, &mut metadata);
        process_dji_thermal_segments(&segments, &mut metadata, &mut Vec::new());

        assert_eq!(metadata.get_string("APP4:RelativeHumidity"), Some("50 %"));
        assert_eq!(metadata.get_string("APP4:IJPEGTempVersion"), None);
        assert_eq!(metadata.get_string("APP4:FactDefEmissivity"), None);
    }

    #[test]
    fn process_app3_segments_combines_dji_thermal_data() {
        let segments = [
            Segment::new(0xFFE3, 0, b"thermal"),
            Segment::new(0xFFE3, 11, b"bytes"),
        ];
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Make", TagValue::String("DJI".to_string()));

        process_app3_segments(&segments, &mut metadata);

        assert_eq!(
            metadata.get_string("APP3:ThermalData"),
            Some("(Binary data 12 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn dji_mavic2_thermal_app_payloads_match_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new(
            "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg",
        );
        if !path.exists() {
            eprintln!("skipping: corpus fixture not present at {}", path.display());
            return;
        }
        let reader = crate::io::buffered_reader::BufferedReader::new(path)
            .expect("read pinned DJI Mavic 2 Enterprise Advanced fixture");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse pinned DJI fixture");
        let mut metadata = MetadataMap::new();

        process_exif_segments(&segments, &reader, &mut metadata, &mut Vec::new());
        process_dji_thermal_segments(&segments, &mut metadata, &mut Vec::new());

        assert_eq!(metadata.get_string("IFD0:Make"), Some("DJI"));
        assert_eq!(
            metadata.get_string("APP3:ThermalData"),
            Some("(Binary data 655360 bytes, use -b option to extract)")
        );
        assert_eq!(
            metadata.get_string("APP5:ThermalCalibration"),
            Some("(Binary data 23818 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn leica_tl2_ifd2_jpg_from_raw_pair_matches_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path =
            std::path::Path::new("/tmp/oxidex-exiftool-cache/combined-samples/Leica/LeicaTL2.jpg");
        if !path.exists() {
            eprintln!("skipping: corpus fixture not present at {}", path.display());
            return;
        }
        let reader = crate::io::buffered_reader::BufferedReader::new(path)
            .expect("read pinned Leica TL2 fixture");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse pinned Leica TL2 segments");
        let mut metadata = MetadataMap::new();

        process_exif_segments(&segments, &reader, &mut metadata, &mut Vec::new());

        assert_eq!(metadata.get_integer("IFD2:JpgFromRawStart"), Some(7063524));
        assert_eq!(metadata.get_integer("IFD2:JpgFromRawLength"), Some(1215652));
        assert_eq!(
            metadata.get_string("IFD2:JpgFromRaw"),
            Some("(Binary data 1215652 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn leica_cl_ifd2_preview_pair_matches_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path =
            std::path::Path::new("/tmp/oxidex-exiftool-cache/combined-samples/Leica/LeicaCL.jpg");
        if !path.exists() {
            eprintln!("skipping: corpus fixture not present at {}", path.display());
            return;
        }
        let reader = crate::io::buffered_reader::BufferedReader::new(path)
            .expect("read pinned Leica CL fixture");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse pinned Leica CL segments");
        let mut metadata = MetadataMap::new();

        process_exif_segments(&segments, &reader, &mut metadata, &mut Vec::new());

        assert_eq!(
            metadata.get_integer("IFD2:PreviewImageStart"),
            Some(7064224)
        );
        assert_eq!(
            metadata.get_integer("IFD2:PreviewImageLength"),
            Some(895146)
        );
        assert_eq!(
            metadata.get_string("IFD2:PreviewImage"),
            Some("(Binary data 895146 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn olympus_sh25mr_gps_area_information_decodes_exif_unicode() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new(
            "/tmp/oxidex-exiftool-cache/combined-samples/Olympus/OlympusSH-25MR.jpg",
        );
        if !path.exists() {
            eprintln!("skipping: corpus fixture not present at {}", path.display());
            return;
        }
        let reader = crate::io::buffered_reader::BufferedReader::new(path)
            .expect("read pinned Olympus SH-25MR fixture");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse pinned Olympus SH-25MR segments");
        let mut metadata = MetadataMap::new();

        process_exif_segments(&segments, &reader, &mut metadata, &mut Vec::new());

        assert_eq!(
            metadata.get_string("GPS:GPSAreaInformation"),
            Some("府中市郷土の森博物館")
        );
    }

    #[test]
    fn ricoh2_empty_gps_dest_distance_ref_matches_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new("/tmp/oxidex-exiftool-cache/combined-samples/Ricoh2.jpg");
        if !path.exists() {
            eprintln!("skipping: corpus fixture not present at {}", path.display());
            return;
        }
        let reader = crate::io::buffered_reader::BufferedReader::new(path)
            .expect("read pinned Ricoh2 fixture");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse pinned Ricoh2 segments");
        let mut metadata = MetadataMap::new();

        process_exif_segments(&segments, &reader, &mut metadata, &mut Vec::new());
        let formatted = crate::core::exiftool_compat::format_for_exiftool(&metadata);

        assert_eq!(
            formatted.get_string("GPS:GPSDestDistanceRef"),
            Some("Unknown ()")
        );
    }

    #[test]
    fn nikon_z7_2_lens_serial_number_stops_at_first_nul() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path =
            std::path::Path::new("/tmp/oxidex-exiftool-cache/combined-samples/Nikon/NikonZ7_2.jpg");
        if !path.exists() {
            eprintln!("skipping: corpus fixture not present at {}", path.display());
            return;
        }
        let reader = crate::io::buffered_reader::BufferedReader::new(path)
            .expect("read pinned Nikon Z7 II fixture");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse pinned Nikon Z7 II segments");
        let mut metadata = MetadataMap::new();

        process_exif_segments(&segments, &reader, &mut metadata, &mut Vec::new());

        assert_eq!(
            metadata.get_string("ExifIFD:LensSerialNumber"),
            Some("20147348")
        );
    }

    #[test]
    fn samsung_sdc130z_learning_opt_out_uses_exif_int16u_override() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new(
            "/tmp/oxidex-exiftool-cache/combined-samples/Samsung/SamsungSDC-130Z.jpg",
        );
        if !path.exists() {
            eprintln!("skipping: corpus fixture not present at {}", path.display());
            return;
        }
        let reader = crate::io::buffered_reader::BufferedReader::new(path)
            .expect("read pinned Samsung SDC-130Z fixture");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse pinned Samsung SDC-130Z segments");
        let mut metadata = MetadataMap::new();

        process_exif_segments(&segments, &reader, &mut metadata, &mut Vec::new());

        assert_eq!(
            metadata.get_string("ExifIFD:LearningOptOutIn"),
            Some("Unknown(65535)")
        );
    }

    #[test]
    fn panasonic_tz57_title2_uses_exif_string_format_override() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new(
            "/tmp/oxidex-exiftool-cache/combined-samples/Panasonic/PanasonicDMC-TZ57.jpg",
        );
        if !path.exists() {
            eprintln!("skipping: corpus fixture not present at {}", path.display());
            return;
        }
        let reader = crate::io::buffered_reader::BufferedReader::new(path)
            .expect("read pinned Panasonic TZ57 fixture");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse pinned Panasonic TZ57 segments");
        let mut metadata = MetadataMap::new();

        process_exif_segments(&segments, &reader, &mut metadata, &mut Vec::new());

        assert_eq!(
            metadata.get_string("IFD0:PanasonicTitle2"),
            Some("9999:99:99 00:00:00")
        );
    }

    #[test]
    fn process_ricoh_rmeta_extracts_azimuth() {
        // Minimal standard RMETA directory with one tag. Section sizes include
        // the two-byte size field, matching Ricoh.pm's ProcessRicohRMETA.
        let mut payload = b"RMETA\0MM\x01\0\0\0\0\0\0\x0a\0\x01\0\0\0\0\0\0\0\0".to_vec();
        payload.extend_from_slice(&[0, 1, 0, 10]);
        payload.extend_from_slice(b"Azimuth\0");
        payload.extend_from_slice(&[0, 2, 0, 3, 0]);
        payload.extend_from_slice(&[0, 3, 0, 4, 0, 5]);

        let segment = Segment::new(APP5_MARKER, 0, &payload);
        let mut metadata = MetadataMap::new();

        process_ricoh_rmeta_segments(&[segment], &mut metadata);

        assert_eq!(metadata.get_string("APP5:Azimuth"), Some("E"));
    }

    #[test]
    fn ricoh2_app5_azimuth_matches_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let paths = [
            "/tmp/oxidex-exiftool-cache/combined-samples/Ricoh2.jpg",
            "/tmp/oxidex-exiftool-cache/exiftool/t/images/Ricoh2.jpg",
        ];
        let Some(path) = paths
            .iter()
            .find(|path| std::path::Path::new(path).exists())
        else {
            return;
        };
        let reader = crate::io::buffered_reader::BufferedReader::new(std::path::Path::new(path))
            .expect("read Ricoh2.jpg");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse Ricoh2.jpg segments");
        let mut metadata = MetadataMap::new();

        process_ricoh_rmeta_segments(&segments, &mut metadata);

        assert_eq!(metadata.get_string("APP5:Azimuth"), Some("E"));
    }

    #[test]
    fn exiftool_jpeg_rmeta_menu_fields_match_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new("/tmp/oxidex-exiftool-cache/combined-samples/ExifTool.jpg");
        if !path.exists() {
            eprintln!("skipping: corpus fixture not present at {}", path.display());
            return;
        }
        let reader =
            crate::io::buffered_reader::BufferedReader::new(path).expect("read ExifTool.jpg");
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader)
            .expect("parse ExifTool.jpg segments");
        let mut metadata = MetadataMap::new();

        process_ricoh_rmeta_segments(&segments, &mut metadata);

        assert_eq!(metadata.get_string("APP5:Condition"), Some("Good"));
        assert_eq!(metadata.get_string("APP5:Lit"), Some("No"));
        assert_eq!(metadata.get_string("APP5:Location"), Some("Roundabout"));
        assert_eq!(metadata.get_string("APP5:SignType"), Some("Information"));
    }

    /// `IPTC:ReferenceNumber` (record 2, dataset 50) is IPTC.pm's
    /// `digits[8]` format: trailing-NUL-stripped text, kept as-is -- not
    /// reformatted as a number. A leading zero in the payload must survive,
    /// which only happens if the value is stored as `TagValue::String` and
    /// never re-parsed through `parse_string_to_tag_value`.
    #[test]
    fn process_iptc_segments_keeps_digits_format_leading_zero() {
        const APP13_MARKER: u16 = 0xFFED;
        const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";

        let mut app13_data = Vec::new();
        app13_data.extend_from_slice(PHOTOSHOP_SIGNATURE);
        app13_data.extend_from_slice(b"8BIM");
        app13_data.extend_from_slice(&[0x04, 0x04]); // resource ID: IPTC
        app13_data.push(0x00); // empty name
        app13_data.push(0x00); // padding

        let mut iptc_data = Vec::new();
        iptc_data.push(0x1C); // tag marker
        iptc_data.extend_from_slice(&[0x02, 0x32]); // record 2, dataset 50
        iptc_data.extend_from_slice(&[0x00, 0x04]); // length 4
        iptc_data.extend_from_slice(b"0042");

        let iptc_size = iptc_data.len() as u32;
        app13_data.extend_from_slice(&iptc_size.to_be_bytes());
        app13_data.extend_from_slice(&iptc_data);

        let segment = Segment::new(APP13_MARKER, 0, &app13_data);
        let mut metadata = MetadataMap::new();
        process_iptc_segments(&[segment], &mut metadata, &mut Vec::new());

        assert_eq!(
            metadata.get("IPTC:ReferenceNumber"),
            Some(&TagValue::new_string("0042"))
        );
    }

    /// ExifTool's IPTC.pm calculates `CurrentIPTCDigest` from the exact bytes
    /// in the standard APP13 Photoshop IPTC resource, not from decoded values.
    #[test]
    fn process_iptc_segments_reports_current_iptc_digest() {
        const APP13_MARKER: u16 = 0xFFED;
        const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";

        let mut app13_data = Vec::new();
        app13_data.extend_from_slice(PHOTOSHOP_SIGNATURE);
        app13_data.extend_from_slice(b"8BIM");
        app13_data.extend_from_slice(&[0x04, 0x04]); // resource ID: IPTC
        app13_data.extend_from_slice(&[0x00, 0x00]); // empty name and padding

        let iptc_data = b"\x1c\x02\x05\x00\x04Rust";
        app13_data.extend_from_slice(&(iptc_data.len() as u32).to_be_bytes());
        app13_data.extend_from_slice(iptc_data);
        app13_data.push(0); // required resource padding for the odd-length payload

        let segment = Segment::new(APP13_MARKER, 0, &app13_data);
        let mut metadata = MetadataMap::new();
        process_iptc_segments(&[segment], &mut metadata, &mut Vec::new());

        assert_eq!(
            metadata.get_string("File:CurrentIPTCDigest"),
            Some("b03d5c713884f15f88d6429c6c7bd683")
        );
    }

    #[test]
    fn process_iptc_segments_finds_digest_after_odd_length_resource() {
        const APP13_MARKER: u16 = 0xFFED;
        const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";

        let mut app13_data = Vec::new();
        app13_data.extend_from_slice(PHOTOSHOP_SIGNATURE);
        app13_data.extend_from_slice(b"8BIM");
        app13_data.extend_from_slice(&[0x04, 0x0d]);
        app13_data.extend_from_slice(&[0x00, 0x00]); // empty name and padding
        app13_data.extend_from_slice(&1u32.to_be_bytes());
        app13_data.push(30); // odd-length payload
        app13_data.push(0); // required resource padding
        app13_data.extend_from_slice(b"8BIM");
        app13_data.extend_from_slice(&[0x04, 0x04]); // resource ID: IPTC
        app13_data.extend_from_slice(&[0x00, 0x00]); // empty name and padding

        let iptc_data = b"\x1c\x02\x05\x00\x04Rust";
        app13_data.extend_from_slice(&(iptc_data.len() as u32).to_be_bytes());
        app13_data.extend_from_slice(iptc_data);
        app13_data.push(0); // required resource padding for the odd-length payload

        let segment = Segment::new(APP13_MARKER, 0, &app13_data);
        let mut metadata = MetadataMap::new();
        process_iptc_segments(&[segment], &mut metadata, &mut Vec::new());

        assert_eq!(
            metadata.get_string("File:CurrentIPTCDigest"),
            Some("b03d5c713884f15f88d6429c6c7bd683")
        );
    }

    #[test]
    fn process_photoshop_segments_extracts_adobe_cm_type() {
        let data = b"Adobe_CM\x00\x03";
        let segment = Segment::new(0xFFED, 0, data);
        let mut metadata = MetadataMap::new();

        process_photoshop_segments(&[segment], &mut metadata, &mut Vec::new());

        assert_eq!(metadata.get_integer("APP13:AdobeCMType"), Some(3));
    }

    #[test]
    fn process_photoshop_segments_ignores_truncated_adobe_cm() {
        let data = b"Adobe_CM\x00";
        let segment = Segment::new(0xFFED, 0, data);
        let mut metadata = MetadataMap::new();

        process_photoshop_segments(&[segment], &mut metadata, &mut Vec::new());

        assert!(!metadata.contains_key("APP13:AdobeCMType"));
    }

    #[test]
    fn process_app15_segments_extracts_graphic_converter_quality() {
        // The APP15 payload of combined-samples/ExifTool.jpg, byte for byte;
        // exiftool -G0 reports [APP15] Quality : 70 for it.
        let segment = Segment::new(0xFFEF, 0, b"Q 70");
        let mut metadata = MetadataMap::new();

        process_app15_segments(&[segment], &mut metadata);

        assert_eq!(metadata.get_integer("APP15:Quality"), Some(70));
    }

    #[test]
    fn process_app15_segments_reads_quality_without_a_space() {
        // /^Q\s*(\d+)/ makes the whitespace optional, so three digits fit.
        let segment = Segment::new(0xFFEF, 0, b"Q100");
        let mut metadata = MetadataMap::new();

        process_app15_segments(&[segment], &mut metadata);

        assert_eq!(metadata.get_integer("APP15:Quality"), Some(100));
    }

    #[test]
    fn process_app15_segments_ignores_payloads_that_are_not_four_bytes() {
        // ExifTool guards on `$length == 4`, so a longer payload that still
        // matches the pattern is not GraphicConverter's and is left alone.
        let segment = Segment::new(0xFFEF, 0, b"Q 70 extra");
        let mut metadata = MetadataMap::new();

        process_app15_segments(&[segment], &mut metadata);

        assert!(!metadata.contains_key("APP15:Quality"));
    }

    #[test]
    fn process_app15_segments_ignores_the_casio_text_payload() {
        // JPEG.pm:310 notes the other APP15 payload in the wild, the "TEXT\0"
        // block Casio and FujiFilm write, which ExifTool never reads.
        let segment = Segment::new(0xFFEF, 0, b"TEXT");
        let mut metadata = MetadataMap::new();

        process_app15_segments(&[segment], &mut metadata);

        assert!(!metadata.contains_key("APP15:Quality"));
    }

    #[test]
    fn process_app15_segments_ignores_a_q_with_no_digits() {
        let segment = Segment::new(0xFFEF, 0, b"Q  x");
        let mut metadata = MetadataMap::new();

        process_app15_segments(&[segment], &mut metadata);

        assert!(!metadata.contains_key("APP15:Quality"));
    }

    /// Mirrors `create_jpeg_with_multiple_segments` in
    /// `segment_parser.rs` (SOI, marker bytes, big-endian u16 length
    /// including the length field itself, payload, EOI).
    fn build_jpeg_with_app3_preview(payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xD8]); // SOI
        data.extend_from_slice(&[0xFF, 0xE3]); // APP3
        data.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        data.extend_from_slice(payload);
        data.extend_from_slice(&[0xFF, 0xD9]); // EOI
        data
    }

    fn build_jpeg_with_app2_segment(payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xD8]); // SOI
        data.extend_from_slice(&[0xFF, 0xE2]); // APP2
        data.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        data.extend_from_slice(payload);
        data.extend_from_slice(&[0xFF, 0xD9]); // EOI
        data
    }

    #[test]
    fn app3_preview_dump_is_extracted_as_file_preview_image() {
        let preview_payload = b"\xff\xd8\xff\xdb\x00\x43FAKEDATA";
        let jpeg = build_jpeg_with_app3_preview(preview_payload);
        let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&jpeg);
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader).unwrap();
        let mut metadata = MetadataMap::new();

        extract_direct_preview_image(&segments, &mut metadata);

        assert_eq!(
            metadata.get("File:PreviewImage"),
            Some(&TagValue::new_binary(preview_payload.to_vec()))
        );
    }

    #[test]
    fn app2_preview_dump_strips_qvga_prefix() {
        // Segment payload: "QVGA\0" + \xFF\xD8\xFF\xE0 + fake JPEG bytes.
        // The QVGA\0 prefix must NOT be part of the stored PreviewImage.
        let inner = b"\xff\xd8\xff\xe0FAKE2";
        let mut payload = b"QVGA\0".to_vec();
        payload.extend_from_slice(inner);
        let jpeg = build_jpeg_with_app2_segment(&payload);
        let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&jpeg);
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader).unwrap();
        let mut metadata = MetadataMap::new();

        extract_direct_preview_image(&segments, &mut metadata);

        assert_eq!(
            metadata.get("File:PreviewImage"),
            Some(&TagValue::new_binary(inner.to_vec()))
        );
    }

    /// General-purpose multi-segment JPEG builder: `(marker byte, payload)`
    /// pairs, SOI-prefixed and EOI-terminated, following the same
    /// length-field convention as `build_jpeg_with_app3_preview` above and
    /// `create_jpeg_with_multiple_segments` in `segment_parser.rs`.
    fn build_jpeg_with_segments(segments: &[(u8, &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&[0xFF, 0xD8]); // SOI
        for (marker, payload) in segments {
            data.extend_from_slice(&[0xFF, *marker]);
            data.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
            data.extend_from_slice(payload);
        }
        data.extend_from_slice(&[0xFF, 0xD9]); // EOI
        data
    }

    /// ExifTool.pm:7997-8010's `elsif ($preview) { $preview .= $segDataPt }`
    /// fallback: an APP2 preview run isn't limited to one segment. This is a
    /// regression test for a bug found during corpus verification, where an
    /// earlier version of this function only ever kept the first matching
    /// APP2 segment and dropped every continuation segment, truncating
    /// multi-segment previews (observed for real on GoProHERO.jpg, whose
    /// true 340142-byte preview was cut to the first segment's 65533 bytes).
    #[test]
    fn app2_preview_dump_accumulates_across_consecutive_segments() {
        let first = b"\xff\xd8\xff\xdb\x00\x43CHUNKONE";
        let second = b"CHUNKTWO-CONTINUATION";
        let jpeg = build_jpeg_with_segments(&[(0xE2, first), (0xE2, second)]);
        let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&jpeg);
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader).unwrap();
        let mut metadata = MetadataMap::new();

        extract_direct_preview_image(&segments, &mut metadata);

        let mut expected = first.to_vec();
        expected.extend_from_slice(second);
        assert_eq!(
            metadata.get("File:PreviewImage"),
            Some(&TagValue::new_binary(expected))
        );
    }

    /// A segment carrying one of APP2's other known identifiers
    /// (ICC_PROFILE\0/FPXR\0/MPF\0/`....IJPEG\0`/urn:) must not be folded
    /// into an in-progress preview even though it also fails the preview
    /// byte pattern -- ExifTool's `elsif` chain routes it to that
    /// identifier's own branch instead of the `elsif ($preview)`
    /// continuation fallback (ExifTool.pm:7929-7997).
    #[test]
    fn app2_preview_dump_does_not_absorb_mpf_segment() {
        let preview_start = b"\xff\xd8\xff\xdb\x00\x43ONLYCHUNK";
        let mpf_payload = b"MPF\0NOTPREVIEWDATA";
        let jpeg = build_jpeg_with_segments(&[(0xE2, preview_start), (0xE2, mpf_payload)]);
        let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&jpeg);
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader).unwrap();
        let mut metadata = MetadataMap::new();

        extract_direct_preview_image(&segments, &mut metadata);

        // The MPF segment ends the APP2 run without being appended: the
        // stored preview is exactly the first segment's bytes.
        assert_eq!(
            metadata.get("File:PreviewImage"),
            Some(&TagValue::new_binary(preview_start.to_vec()))
        );
    }

    /// ExifTool.pm:8116-8127 (Samsung S1060): a preview started in APP3
    /// continues into APP4, and is only emitted once the segment after APP4
    /// is not itself APP5.
    #[test]
    fn app3_preview_dump_continues_into_app4() {
        let app3_payload = b"\xff\xd8\xff\xdbAPP3PART";
        let app4_payload = b"APP4CONTINUATION";
        let jpeg = build_jpeg_with_segments(&[(0xE3, app3_payload), (0xE4, app4_payload)]);
        let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&jpeg);
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader).unwrap();
        let mut metadata = MetadataMap::new();

        extract_direct_preview_image(&segments, &mut metadata);

        let mut expected = app3_payload.to_vec();
        expected.extend_from_slice(app4_payload);
        assert_eq!(
            metadata.get("File:PreviewImage"),
            Some(&TagValue::new_binary(expected))
        );
    }

    /// ExifTool.pm:8116-8151 (BenQ DC E1050): APP3 -> APP4 -> APP5, where
    /// APP4 must NOT emit early because the next segment is APP5, and APP5
    /// appends its payload and emits unconditionally. This is the
    /// regression test for the APP4/APP5 gating bug: an earlier version of
    /// this function emitted (and truncated) the preview at APP4 because it
    /// never checked whether APP5 followed.
    #[test]
    fn app3_preview_dump_continues_through_app4_into_app5() {
        let app3_payload = b"\xff\xd8\xff\xdbAPP3PART";
        let app4_payload = b"APP4PART";
        let app5_payload = b"APP5PART-FINAL";
        let jpeg = build_jpeg_with_segments(&[
            (0xE3, app3_payload),
            (0xE4, app4_payload),
            (0xE5, app5_payload),
        ]);
        let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&jpeg);
        let segments = crate::parsers::jpeg::segment_parser::parse_segments(&reader).unwrap();
        let mut metadata = MetadataMap::new();

        extract_direct_preview_image(&segments, &mut metadata);

        let mut expected = app3_payload.to_vec();
        expected.extend_from_slice(app4_payload);
        expected.extend_from_slice(app5_payload);
        assert_eq!(
            metadata.get("File:PreviewImage"),
            Some(&TagValue::new_binary(expected))
        );
    }
}

#[cfg(test)]
mod print_im_tests {
    use super::*;
    use crate::parsers::jpeg::segment_parser::parse_segments;
    use crate::test_support::TestReader;

    fn print_im_block(version: &[u8; 4]) -> Vec<u8> {
        let mut block = b"PrintIM\0".to_vec();
        block.extend_from_slice(version);
        block.extend_from_slice(&[0, 0, 0, 0]); // reserved + zero entries
        block
    }

    fn jpeg_with_ifd0_print_im(version: &[u8; 4]) -> Vec<u8> {
        let value = print_im_block(version);
        let mut tiff = b"II\x2a\0\x08\0\0\0\x01\0".to_vec();
        tiff.extend_from_slice(&0xC4A5u16.to_le_bytes());
        tiff.extend_from_slice(&7u16.to_le_bytes());
        tiff.extend_from_slice(&(value.len() as u32).to_le_bytes());
        tiff.extend_from_slice(&26u32.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(&value);

        let mut payload = b"Exif\0\0".to_vec();
        payload.extend_from_slice(&tiff);
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }

    #[test]
    fn jpeg_ifd0_dispatches_tag_c4a5_to_print_im() {
        let reader = TestReader::new(jpeg_with_ifd0_print_im(b"0300"));
        let segments = parse_segments(&reader).unwrap();
        let mut metadata = MetadataMap::new();
        process_exif_segments(&segments, &reader, &mut metadata, &mut Vec::new());

        assert_eq!(metadata.get_string("PrintIM:PrintIMVersion"), Some("0300"));
        assert!(metadata.get("IFD0:PrintIM").is_none());
    }
}

#[cfg(test)]
mod transfer_function_tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn jpeg_ifd0_transfer_function_is_named_and_rendered_as_binary() {
        // Exif.pm 13.59 declares 0x012d as `int16u[768], Binary => 1`.
        // The Binary payload is ExifTool's space-separated int16 rendering,
        // not the raw TIFF byte span: "0 65535 12" is 10 bytes.
        let tags = vec![(
            0x012d,
            3,
            3,
            Cow::Owned(vec![0x00, 0x00, 0xff, 0xff, 0x0c, 0x00]),
        )];
        let mut metadata = MetadataMap::new();

        process_ifd0_tags(
            &tags,
            ByteOrder::LittleEndian,
            &mut metadata,
            &mut Vec::new(),
        );

        assert_eq!(
            crate::core::exiftool_compat::format_for_exiftool(&metadata)
                .get_string("IFD0:TransferFunction"),
            Some("(Binary data 10 bytes, use -b option to extract)")
        );
    }
}
