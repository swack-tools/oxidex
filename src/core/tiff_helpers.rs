//! TIFF metadata parsing helpers
//!
//! This module contains helper functions for parsing TIFF IFD structures,
//! processing tags, and handling sub-IFDs (EXIF, GPS), MakerNotes, and GeoTiff.

use super::{FileReader, MetadataMap, TagValue};
use crate::core::operations_helpers::read_u32;
use crate::core::tag_conversion::{gps_coordinate_degrees, raw_bytes_to_tag_value};
use crate::parsers::common::print_im::{PRINT_IM_VERSION_TAG, decode_print_im_version};
use crate::parsers::tiff::geotiff_parser;
use crate::parsers::tiff::ifd_parser::{ByteOrder, find_entry_position, parse_ifd};
use crate::parsers::tiff::makernote_dispatcher::dispatch_makernote_with_context_and_values;
use crate::parsers::tiff::makernotes::makernote_context::{
    MakerNoteContext, value_overlaps_directory,
};
use crate::tag_db::lookup_tag_name;
use std::collections::HashMap;

// =============================================================================
// Interoperability IFD Tag Constants
// =============================================================================
//
// The Interoperability IFD is a sub-IFD within the EXIF IFD that stores
// compatibility information for DCF (Design Rule for Camera File System) files.
// These tags specify the color space and format conformance of the image.

/// InteropIndex (0x0001): Identifies the conformance standard.
/// Common values: "R98" (DCF basic), "R03" (DCF option/Adobe RGB), "THM" (thumbnail)
const INTEROP_INDEX: u16 = 0x0001;

/// InteropVersion (0x0002): Version of the interoperability standard.
/// Typically "0100" encoded as four ASCII digits.
const INTEROP_VERSION: u16 = 0x0002;

/// RelatedImageWidth (0x1001): Width of the related full-resolution image.
/// Stored in the Interoperability IFD to indicate dimensions.
const RELATED_IMAGE_WIDTH: u16 = 0x1001;

/// RelatedImageHeight (0x1002): Height of the related full-resolution image.
/// Stored in the Interoperability IFD to indicate dimensions.
const RELATED_IMAGE_HEIGHT: u16 = 0x1002;

/// InteroperabilityIFDPointer (0xA005): Offset to the Interoperability IFD.
/// Found in the EXIF IFD, points to a sub-IFD containing interop tags.
const INTEROPERABILITY_IFD_POINTER: u16 = 0xA005;

/// MakerNote (0x927C): the manufacturer's private block in the EXIF IFD.
const MAKERNOTE: u16 = 0x927C;

// Image-carrying tags some cameras (Samsung SPH-A800/A940, Canon XL H1) write
// into the Interoperability IFD alongside - or instead of - the DCF tags.
// 0x0201/0x0202 are the JPEGInterchangeFormat pair, but ExifTool names that
// pair contextually: ThumbnailOffset/ThumbnailLength ONLY inside IFD1; in any
// other directory (including this one) it reports OtherImageStart /
// OtherImageLength, plus the derived OtherImage blob.

/// XResolution (0x011A) - resolution of the embedded image.
const TAG_X_RESOLUTION: u16 = 0x011A;

/// YResolution (0x011B) - resolution of the embedded image.
const TAG_Y_RESOLUTION: u16 = 0x011B;

/// ResolutionUnit (0x0128) - unit for the resolution pair.
const TAG_RESOLUTION_UNIT: u16 = 0x0128;

/// JPEGInterchangeFormat (0x0201) - `OtherImageStart` outside IFD1.
const TAG_OTHER_IMAGE_START: u16 = 0x0201;

/// JPEGInterchangeFormatLength (0x0202) - `OtherImageLength` outside IFD1.
const TAG_OTHER_IMAGE_LENGTH: u16 = 0x0202;

// =============================================================================
// Interoperability IFD Helper Functions
// =============================================================================

/// Maps an Interoperability IFD tag ID to its canonical name.
///
/// The Interoperability IFD contains only a few defined tags. This function
/// returns the ExifTool-compatible tag name for known tags, or "Unknown" for
/// unrecognized tag IDs.
///
/// # Arguments
///
/// * `tag_id` - The numeric tag identifier from the Interoperability IFD
///
/// # Returns
///
/// A static string with the tag name (e.g., "InteropIndex", "InteropVersion")
pub(crate) fn interop_tag_to_name(tag_id: u16) -> &'static str {
    match tag_id {
        INTEROP_INDEX => "InteropIndex",
        INTEROP_VERSION => "InteropVersion",
        RELATED_IMAGE_WIDTH => "RelatedImageWidth",
        RELATED_IMAGE_HEIGHT => "RelatedImageHeight",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod print_im_dispatch_tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn standalone_tiff_ifd0_dispatches_tag_c4a5_to_print_im() {
        let mut value = b"PrintIM\0".to_vec();
        value.extend_from_slice(b"0250");
        value.extend_from_slice(&[0, 0, 0, 0]);

        let mut tiff = b"II\x2a\0\x08\0\0\0\x01\0".to_vec();
        tiff.extend_from_slice(&0xC4A5u16.to_le_bytes());
        tiff.extend_from_slice(&7u16.to_le_bytes());
        tiff.extend_from_slice(&(value.len() as u32).to_le_bytes());
        tiff.extend_from_slice(&26u32.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(&value);

        let reader = TestReader::new(tiff);
        let mut metadata = MetadataMap::new();
        parse_ifd_chain(&reader, 8, ByteOrder::LittleEndian, &mut metadata).unwrap();

        assert_eq!(metadata.get_string("PrintIM:PrintIMVersion"), Some("0250"));
        assert!(metadata.get("IFD0:PrintIM").is_none());
    }
}

/// Formats the InteropIndex value with a human-readable description.
///
/// The InteropIndex tag (0x0001) contains a short identifier indicating which
/// DCF (Design rule for Camera File system) specification the image conforms to.
/// This function expands the identifier to include the full description as
/// ExifTool does.
///
/// # Arguments
///
/// * `raw_index` - The raw InteropIndex string (e.g., "R98", "R03", "THM")
///
/// # Returns
///
/// A formatted string with the index and its description:
/// - "R98" -> "R98 - DCF basic file (sRGB)"
/// - "R03" -> "R03 - DCF option file (Adobe RGB)"
/// - "THM" -> "THM - DCF thumbnail file"
/// - Other values are returned as-is
///
/// # Examples
///
/// ```ignore
/// assert_eq!(format_interop_index("R98"), "R98 - DCF basic file (sRGB)");
/// assert_eq!(format_interop_index("R03"), "R03 - DCF option file (Adobe RGB)");
/// assert_eq!(format_interop_index("THM"), "THM - DCF thumbnail file");
/// assert_eq!(format_interop_index("UNKNOWN"), "UNKNOWN");
/// ```
fn format_interop_index(raw_index: &str) -> String {
    match raw_index {
        "R98" => "R98 - DCF basic file (sRGB)".to_string(),
        "R03" => "R03 - DCF option file (Adobe RGB)".to_string(),
        "THM" => "THM - DCF thumbnail file".to_string(),
        other => other.to_string(),
    }
}

/// Parses a chain of IFDs in a TIFF file.
///
/// TIFF files can contain multiple IFDs linked together. This function
/// traverses the chain and processes each IFD.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the TIFF file
/// * `first_offset` - Offset to the first IFD
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `metadata` - MetadataMap to populate
pub fn parse_ifd_chain(
    reader: &dyn FileReader,
    first_offset: u64,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) -> crate::error::Result<()> {
    let mut ifd_offset = first_offset;
    let mut ifd_index = 0;

    while ifd_offset != 0 {
        // Determine IFD name based on index
        let ifd_name = get_ifd_name(ifd_index);

        // Parse this IFD
        let tags = parse_ifd(reader, ifd_offset, byte_order)?;

        // Process IFD tags and get sub-IFD information
        let (exif_offset, gps_offset, makernote_data) =
            process_tiff_ifd_tags(&tags, ifd_name, byte_order, metadata);

        // Parse EXIF Sub-IFD if present. A standalone TIFF's structure starts
        // at file offset 0, so the TIFF base ExifTool adds to stored offsets
        // is 0 here, and the whole file is the enclosing TIFF block.
        if let Some(offset) = exif_offset {
            parse_exif_subifd(reader, offset, byte_order, 0, reader.size(), metadata);
        }

        // Parse GPS Sub-IFD if present
        if let Some(offset) = gps_offset {
            parse_gps_subifd(reader, offset, byte_order, metadata);
        }

        // Parse a MakerNote sitting directly in this IFD rather than in the
        // EXIF sub-IFD. Its value offsets are measured from the file's own TIFF
        // header, which is this reader's offset 0.
        if let Some(makernote_bytes) = makernote_data {
            let ctx = makernote_context(
                reader,
                ifd_offset,
                byte_order,
                0,
                reader.size(),
                makernote_bytes,
            );
            parse_makernote(&ctx, byte_order, metadata);
        }

        // Read next IFD offset
        let entry_count = tags.len();
        let next_offset_location = ifd_offset + 2 + (entry_count as u64 * 12);

        if next_offset_location + 4 > reader.size() {
            // Can't read next offset, end of chain
            break;
        }

        let next_offset_bytes = reader.read(next_offset_location, 4)?;
        ifd_offset = read_u32(next_offset_bytes, byte_order) as u64;
        ifd_index += 1;

        // Safety check: prevent infinite loops
        if ifd_index > 10 {
            eprintln!("Warning: More than 10 IFDs found, stopping to prevent infinite loop");
            break;
        }
    }

    Ok(())
}

/// Gets the canonical IFD name for a given index.
///
/// # Arguments
///
/// * `index` - Zero-based IFD index
///
/// # Returns
///
/// The IFD name (e.g., "IFD0", "IFD1", "IFD2", "IFD3")
fn get_ifd_name(index: usize) -> &'static str {
    match index {
        0 => "IFD0",
        1 => "IFD1",
        2 => "IFD2",
        3 => "IFD3",
        n => {
            // For IFD4 and beyond, just use IFDX format
            eprintln!("Warning: Found IFD{} which is unusual", n);
            "IFD0" // Fallback to IFD0 for unusual cases
        }
    }
}

/// Processes tags from a TIFF IFD.
///
/// Extracts tags and identifies special pointers (EXIF sub-IFD, GPS sub-IFD,
/// MakerNote, ICC profile).
///
/// # Arguments
///
/// * `tags` - Parsed IFD tags
/// * `ifd_name` - Name of the IFD (e.g., "IFD0")
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `metadata` - MetadataMap to populate
///
/// # Returns
///
/// A tuple of (exif_offset, gps_offset, makernote_data)
fn process_tiff_ifd_tags<'a>(
    tags: &'a [(u16, u16, u32, std::borrow::Cow<[u8]>)],
    ifd_name: &str,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) -> (Option<u64>, Option<u64>, Option<&'a [u8]>) {
    let mut exif_ifd_offset = None;
    let mut gps_ifd_offset = None;
    let mut makernote_data: Option<&[u8]> = None;

    // GeoTiff tag data collectors
    let mut geotiff_directory: Option<&[u8]> = None;
    let mut geotiff_double_params: Option<&[u8]> = None;
    let mut geotiff_ascii_params: Option<&str> = None;
    let mut model_transformation: Option<&[u8]> = None;

    // Convert tags to metadata
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

        // Exif.pm 0xc4a5 declares a PrintIM SubDirectory. Keep the raw
        // directory out of the map even when PrintIM.pm rejects its contents.
        if *tag_id == 0xC4A5 {
            if let Some(version) = decode_print_im_version(bytes, byte_order) {
                metadata.insert(PRINT_IM_VERSION_TAG, TagValue::new_string(version));
            }
            continue;
        }

        // Check for GeoTiff tags
        // Tag 34735 (0x87AF): GeoKeyDirectoryTag - the main GeoTiff key directory
        if *tag_id == geotiff_parser::GEOTIFF_DIRECTORY_TAG {
            geotiff_directory = Some(bytes);
            continue; // Don't add raw directory tag - we'll parse it into named keys
        }
        // Tag 34736 (0x87B0): GeoDoubleParamsTag - double precision values
        if *tag_id == geotiff_parser::GEOTIFF_DOUBLE_PARAMS_TAG {
            geotiff_double_params = Some(bytes);
            continue; // Don't add raw params tag - used by directory parser
        }
        // Tag 34737 (0x87B1): GeoAsciiParamsTag - ASCII string values
        if *tag_id == geotiff_parser::GEOTIFF_ASCII_PARAMS_TAG {
            // Convert bytes to string for ASCII params
            if let Ok(s) = std::str::from_utf8(bytes) {
                geotiff_ascii_params = Some(s);
            }
            continue; // Don't add raw params tag - used by directory parser
        }
        // Tag 34264 (0x85D8): ModelTransformation - 4x4 transformation matrix
        if *tag_id == geotiff_parser::MODEL_TRANSFORMATION_TAG {
            model_transformation = Some(bytes);
            continue; // Don't add raw tag - we'll output parsed EXIF:ModelTransform
        }

        // Check for MakerNote tag (0x927C)
        // Store the data for later processing after we've added other tags
        if *tag_id == 0x927C {
            makernote_data = Some(bytes);
            // Note: We don't continue here - we still add the raw MakerNote tag
            // to metadata so tools can see it, but we'll also parse it below
        }

        // Check for ICC Profile tag (0x8773)
        if *tag_id == 0x8773 && bytes.len() >= 128 {
            // Parse ICC profile data
            match crate::parsers::icc::parse_icc_profile_data(bytes) {
                Ok(icc_tags) => {
                    // Add all ICC tags to metadata with "ICC_Profile:" prefix
                    // to match ExifTool's family naming
                    for (tag_name, value) in icc_tags {
                        metadata.insert(format!("ICC_Profile:{}", tag_name), value);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse ICC profile in TIFF: {}", e);
                }
            }
            // Don't continue - still add the raw ICC_Profile tag
        }

        // Check for IPTC-NAA tag (0x83BB = 33723)
        // Contains IPTC IIM (Information Interchange Model) metadata
        if *tag_id == 0x83BB && !bytes.is_empty() {
            use crate::core::value_formatter::{format_iptc_date, format_iptc_time};
            use crate::parsers::jpeg::iptc_parser::{
                dataset_to_tag_name, decode_iptc_string, parse_all_iptc_records,
            };

            match parse_all_iptc_records(bytes) {
                Ok(records) => {
                    // Track keywords for aggregation (ExifTool combines them)
                    let mut keywords: Vec<String> = Vec::new();

                    for record in records {
                        // Only handle Record 2 (Application Record)
                        if record.record_number != 2 {
                            continue;
                        }

                        let tag_name =
                            dataset_to_tag_name(record.record_number, record.dataset_number);
                        let mut value = decode_iptc_string(&record.data);

                        // Apply formatting for specific dataset types
                        match record.dataset_number {
                            0 => {
                                // ApplicationRecordVersion (dataset 0) is a numeric value
                                // It's stored as 2 bytes big-endian
                                if record.data.len() >= 2 {
                                    let version =
                                        u16::from_be_bytes([record.data[0], record.data[1]]);
                                    metadata.insert(
                                        "IPTC:ApplicationRecordVersion".to_string(),
                                        TagValue::Integer(version as i64),
                                    );
                                }
                                continue;
                            }
                            25 => {
                                // Keywords (dataset 25) - collect for aggregation
                                keywords.push(value);
                                continue;
                            }
                            55 => {
                                // DateCreated: YYYYMMDD -> YYYY:MM:DD
                                value = format_iptc_date(&value);
                            }
                            60 => {
                                // TimeCreated: HHMMSS±HHMM -> HH:MM:SS±HH:MM
                                value = format_iptc_time(&value);
                            }
                            _ => {}
                        }

                        metadata.insert(tag_name, TagValue::String(value));
                    }

                    // Add aggregated keywords if any
                    if !keywords.is_empty() {
                        metadata.insert(
                            "IPTC:Keywords".to_string(),
                            TagValue::Array(keywords.into_iter().map(TagValue::String).collect()),
                        );
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse IPTC metadata in TIFF: {}", e);
                }
            }
            // Skip adding the raw IPTC tag since we've parsed it
            continue;
        }

        // Convert tag to metadata
        let tag_name = lookup_tag_name(*tag_id, ifd_name);
        let tag_value =
            raw_bytes_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order);
        metadata.insert(tag_name, tag_value);
    }

    // Parse GeoTiff keys if directory tag is present
    let is_little_endian = byte_order == ByteOrder::LittleEndian;
    if let Some(directory) = geotiff_directory {
        let geotiff_tags = geotiff_parser::parse_geotiff_keys(
            directory,
            geotiff_double_params,
            geotiff_ascii_params,
            is_little_endian,
        );
        for (tag_name, value) in geotiff_tags {
            metadata.insert(tag_name, TagValue::String(value));
        }
    }

    // Parse ModelTransformation if present (outputs as EXIF:ModelTransform)
    if let Some(transform_data) = model_transformation {
        if let Some(formatted) =
            geotiff_parser::parse_model_transformation(transform_data, is_little_endian)
        {
            metadata.insert(
                "EXIF:ModelTransform".to_string(),
                TagValue::String(formatted),
            );
        }
    }

    (exif_ifd_offset, gps_ifd_offset, makernote_data)
}

/// Parses an EXIF sub-IFD and extracts tags.
///
/// The EXIF sub-IFD contains detailed camera settings and shooting parameters.
/// It may also contain:
/// - MakerNote data specific to camera manufacturers
/// - InteroperabilityIFDPointer (0xA005) pointing to the Interoperability sub-IFD
///
/// # Arguments
///
/// * `reader` - File reader providing access to the file
/// * `offset` - Offset to the EXIF sub-IFD
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `tiff_base` - Absolute file offset of the TIFF header (0 for a standalone
///   TIFF), added to stored offsets such as `OtherImageStart` the way ExifTool
///   absolutises them
/// * `tiff_len` - Length of the enclosing TIFF block as measured from `reader`
///   offset 0. This is ExifTool's `$dataLen`: the EXIF APP1 payload for a
///   JPEG, the file for a standalone TIFF. It bounds how far a MakerNote
///   decoder may resolve its own value offsets -- see [`makernote_context`].
/// * `metadata` - MetadataMap to populate
pub fn parse_exif_subifd(
    reader: &dyn FileReader,
    offset: u64,
    byte_order: ByteOrder,
    tiff_base: u64,
    tiff_len: u64,
    metadata: &mut MetadataMap,
) {
    if let Ok(exif_tags) = parse_ifd(reader, offset, byte_order) {
        // Track MakerNote and InteroperabilityIFD pointer in EXIF IFD
        let mut exif_makernote_data: Option<&[u8]> = None;
        let mut interop_ifd_offset: Option<u64> = None;

        // First pass: convert tags and capture special pointers
        for (tag_id, field_type, value_count, raw_bytes) in &exif_tags {
            // Convert Cow<[u8]> to &[u8] for processing
            let bytes = raw_bytes.as_ref();

            // Check for MakerNote in EXIF IFD (tag 0x927C)
            if *tag_id == MAKERNOTE {
                exif_makernote_data = Some(bytes);
            }

            // Check for InteroperabilityIFDPointer (tag 0xA005)
            // This pointer leads to the Interoperability sub-IFD containing
            // DCF conformance information (InteropIndex, InteropVersion, etc.)
            if *tag_id == INTEROPERABILITY_IFD_POINTER && bytes.len() >= 4 {
                let iop_offset = read_u32(bytes, byte_order);
                interop_ifd_offset = Some(iop_offset as u64);
                // Don't add the pointer tag to metadata - we'll parse the sub-IFD instead
                continue;
            }

            let tag_name = lookup_tag_name(*tag_id, "ExifIFD");
            let tag_value =
                raw_bytes_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order);
            metadata.insert(tag_name, tag_value);
        }

        // Second pass: parse the MakerNote found in the EXIF IFD. The decoder
        // is given the enclosing TIFF block as well as the payload, because a
        // MakerNote's value offsets are measured from the TIFF header and
        // routinely address bytes past the payload's declared end.
        if let Some(makernote_bytes) = exif_makernote_data {
            let ctx = makernote_context(
                reader,
                offset,
                byte_order,
                tiff_base,
                tiff_len,
                makernote_bytes,
            );
            parse_makernote(&ctx, byte_order, metadata);
        }

        // Third pass: Parse Interoperability IFD if pointer was found
        // The Interop IFD contains DCF conformance tags like InteropIndex and InteropVersion
        if let Some(iop_offset) = interop_ifd_offset {
            parse_interop_subifd(reader, iop_offset, byte_order, tiff_base, metadata);
        }
    }
}

/// Parses an Interoperability sub-IFD and extracts Interop tags.
///
/// The Interoperability IFD is a sub-IFD referenced from the EXIF IFD via tag 0xA005.
/// It contains DCF (Design Rule for Camera File System) conformance information:
///
/// - **InteropIndex (0x0001)**: Conformance standard identifier
///   - "R98": DCF basic file (sRGB color space)
///   - "R03": DCF option file (Adobe RGB color space)
///   - "THM": DCF thumbnail file
/// - **InteropVersion (0x0002)**: Version of the interoperability standard (usually "0100")
/// - **RelatedImageWidth (0x1001)**: Width of the related full-resolution image
/// - **RelatedImageHeight (0x1002)**: Height of the related full-resolution image
///
/// Those DCF tags are output with the "EXIF:" prefix to match ExifTool's output
/// format (and the surgical writer's key-anticipation in
/// `carried_class_reader_keys`, `src/writers/exif_surgical.rs`).
///
/// # Image-carrying Interop directories
///
/// Some cameras (SamsungSPH-A800.jpg, SamsungSPH-A940.jpg and CanonXL_H1.jpg
/// in the corpus) write an embedded image into the Interoperability IFD
/// instead: `Compression`/`XResolution`/`YResolution`/`ResolutionUnit` plus
/// the JPEGInterchangeFormat pair 0x0201/0x0202. ExifTool names that pair
/// contextually - `ThumbnailOffset`/`ThumbnailLength` ONLY inside IFD1; here
/// it reports `OtherImageStart`/`OtherImageLength` and the derived
/// `OtherImage` blob. These are emitted under the "InteropIFD:" prefix, the
/// family-1 group ExifTool assigns them (`exiftool -G1`).
///
/// # Arguments
///
/// * `reader` - File reader providing access to the file
/// * `offset` - Offset to the Interoperability sub-IFD
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `tiff_base` - Absolute file offset of the TIFF header, added to
///   `OtherImageStart` exactly as `parse_ifd1_thumbnail` absolutises
///   `ThumbnailOffset`
/// * `metadata` - MetadataMap to populate with Interop tags
fn parse_interop_subifd(
    reader: &dyn FileReader,
    offset: u64,
    byte_order: ByteOrder,
    tiff_base: u64,
    metadata: &mut MetadataMap,
) {
    // Attempt to parse the Interoperability IFD structure
    let Ok(interop_tags) = parse_ifd(reader, offset, byte_order) else {
        return;
    };

    let mut other_image_start: Option<u64> = None;
    let mut other_image_length: Option<u64> = None;

    for (tag_id, field_type, value_count, raw_bytes) in &interop_tags {
        let bytes = raw_bytes.as_ref();

        match *tag_id {
            // Image-carrying tags: emitted under the "InteropIFD:" group.
            // ExifTool's default (duplicate-suppressed) output only reports
            // these when IFD0 does not already own the same tag name - the
            // IFD0 copy has priority, the Interop copy priority 0 - so yield
            // to an existing IFD0 twin the same way `parse_ifd1_thumbnail`
            // yields IFD1:Compression to IFD0:Compression.
            TAG_COMPRESSION | TAG_X_RESOLUTION | TAG_Y_RESOLUTION | TAG_RESOLUTION_UNIT => {
                let tag_name = lookup_tag_name(*tag_id, "InteropIFD");
                let base_name = tag_name
                    .split_once(':')
                    .map_or(tag_name.as_str(), |(_, n)| n);
                if metadata.get(&format!("IFD0:{}", base_name)).is_some() {
                    continue;
                }
                let tag_value =
                    raw_bytes_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order);
                metadata.insert(tag_name, tag_value);
            }
            // The offset/length pair is emitted after the loop, once both
            // halves are known.
            TAG_OTHER_IMAGE_START => {
                other_image_start =
                    read_unsigned_field(bytes, *field_type, *value_count, *tag_id, byte_order);
            }
            TAG_OTHER_IMAGE_LENGTH => {
                other_image_length =
                    read_unsigned_field(bytes, *field_type, *value_count, *tag_id, byte_order);
            }
            _ => {
                // DCF conformance tags - use our local mapping for known tags
                let tag_base_name = interop_tag_to_name(*tag_id);

                // Skip unknown tags (they would return "Unknown" from interop_tag_to_name)
                if tag_base_name == "Unknown" {
                    continue;
                }

                // Build the full tag name with "EXIF:" prefix to match ExifTool output
                let tag_name = format!("EXIF:{}", tag_base_name);

                // Convert the raw bytes to a TagValue
                let mut tag_value =
                    raw_bytes_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order);

                // Apply special formatting for InteropIndex
                // ExifTool formats this as "R98 - DCF basic file (sRGB)" etc.
                if *tag_id == INTEROP_INDEX
                    && let Some(raw_index) = tag_value.as_string()
                {
                    let formatted = format_interop_index(raw_index);
                    tag_value = TagValue::String(formatted);
                }

                metadata.insert(tag_name, tag_value);
            }
        }
    }

    // ExifTool emits the offset/length pair only when both are present.
    let (Some(image_offset), Some(length)) = (other_image_start, other_image_length) else {
        return;
    };

    // The stored value is TIFF-relative; ExifTool reports the absolute file
    // offset (stored value + TIFF base), same as ThumbnailOffset in IFD1.
    let Some(absolute_offset) = image_offset.checked_add(tiff_base) else {
        return;
    };

    // Named explicitly rather than via `lookup_tag_name`: 0x0201/0x0202 are
    // registered under IFD1's contextual names, but outside IFD1 ExifTool
    // reports them as OtherImageStart/OtherImageLength.
    metadata.insert(
        "InteropIFD:OtherImageStart",
        TagValue::new_integer(absolute_offset as i64),
    );
    metadata.insert(
        "InteropIFD:OtherImageLength",
        TagValue::new_integer(length as i64),
    );

    // OtherImage is the bytes the pair points at, emitted only when the range
    // is actually readable - the same byte-range validation
    // `parse_ifd1_thumbnail` applies to ThumbnailImage.
    if length == 0 || length > MAX_THUMBNAIL_BYTES {
        return;
    }
    let Some(end) = image_offset.checked_add(length) else {
        return;
    };
    if end > reader.size() {
        return;
    }
    let Ok(bytes) = reader.read(image_offset, length as usize) else {
        return;
    };
    // A reader that clamps a short read instead of failing would otherwise make
    // us report a byte count the file does not actually contain.
    if bytes.len() as u64 != length {
        return;
    }
    metadata.insert(
        "InteropIFD:OtherImage",
        TagValue::new_binary(bytes.to_vec()),
    );
}

/// Parses a GPS sub-IFD and extracts GPS tags.
///
/// The GPS sub-IFD contains GPS positioning information including
/// latitude, longitude, altitude, and timestamp.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the file
/// * `offset` - Offset to the GPS sub-IFD
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `metadata` - MetadataMap to populate
pub fn parse_gps_subifd(
    reader: &dyn FileReader,
    offset: u64,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    if let Ok(gps_tags) = parse_ifd(reader, offset, byte_order) {
        for (tag_id, field_type, value_count, raw_bytes) in gps_tags {
            let tag_name = lookup_tag_name(tag_id, "GPS");
            let tag_value = raw_bytes_to_tag_value(
                raw_bytes.as_ref(),
                field_type,
                value_count,
                tag_id,
                byte_order,
            );
            metadata.insert(&tag_name, tag_value);
            if matches!(tag_id, 0x0002 | 0x0004 | 0x0014 | 0x0016)
                && field_type == 5
                && value_count == 3
                && let Some(degrees) = gps_coordinate_degrees(raw_bytes.as_ref(), byte_order)
            {
                metadata.set_value_form(tag_name, degrees.to_string());
            }
        }
    }
}

// =============================================================================
// IFD1 (Thumbnail IFD)
// =============================================================================

/// Compression (0x0103) - in IFD1 this describes the thumbnail encoding.
const TAG_COMPRESSION: u16 = 0x0103;

/// JPEGInterchangeFormat (0x0201) - ExifTool names this `ThumbnailOffset` in IFD1.
const TAG_THUMBNAIL_OFFSET: u16 = 0x0201;

/// JPEGInterchangeFormatLength (0x0202) - ExifTool names this `ThumbnailLength` in IFD1.
const TAG_THUMBNAIL_LENGTH: u16 = 0x0202;

/// Upper bound on a thumbnail we are willing to materialise, guarding against
/// a corrupt length field causing a huge allocation. Real camera thumbnails
/// are 5-30 KB; ExifTool's own EXIF segment ceiling is 64 KB.
const MAX_THUMBNAIL_BYTES: u64 = 1 << 20;

/// Reads the next-IFD pointer that follows an IFD's entry array.
///
/// Returns `None` when the pointer is unreadable, zero (end of the chain), or
/// points back at the IFD it follows (a loop some malformed files contain).
fn next_ifd_offset(
    reader: &dyn FileReader,
    ifd_offset: u64,
    entry_count: usize,
    byte_order: ByteOrder,
) -> Option<u64> {
    let pointer_offset = ifd_offset
        .checked_add(2)?
        .checked_add((entry_count as u64).checked_mul(12)?)?;

    if pointer_offset.checked_add(4)? > reader.size() {
        return None;
    }

    let next = read_u32(reader.read(pointer_offset, 4).ok()?, byte_order) as u64;

    if next == 0 || next == ifd_offset {
        None
    } else {
        Some(next)
    }
}

/// Collects the offsets of the EXIF directories that have already been walked
/// before IFD1 is reached: IFD0 itself, its EXIF and GPS sub-IFDs, and the
/// Interoperability IFD that hangs off the EXIF sub-IFD.
///
/// ExifTool keeps the same set in `$$self{PROCESSED}` and refuses to descend
/// into a directory address it has seen before ("IFD1 pointer references
/// previous InteropIFD directory", `ProcessDirectory` in ExifTool.pm), which
/// makes it report no thumbnail at all for such a file. Four samples in the
/// corpus are built that way - SamsungSPH-A800.jpg, SamsungSPH-A940.jpg and
/// CanonXL_H1.jpg aim IFD1 at the InteropIFD, SamsungGT-S5620.jpg aims it at
/// the GPS IFD - and following the pointer anyway turns an unrelated
/// directory's entries into invented thumbnail tags.
fn visited_directory_offsets(
    reader: &dyn FileReader,
    ifd0_offset: u64,
    byte_order: ByteOrder,
) -> Vec<u64> {
    const EXIF_IFD_POINTER: u16 = 0x8769;
    const GPS_IFD_POINTER: u16 = 0x8825;

    let mut visited = vec![ifd0_offset];
    let Ok(ifd0) = parse_ifd(reader, ifd0_offset, byte_order) else {
        return visited;
    };

    for (tag_id, _, _, raw_bytes) in &ifd0 {
        if !matches!(*tag_id, EXIF_IFD_POINTER | GPS_IFD_POINTER) || raw_bytes.len() < 4 {
            continue;
        }
        let sub_offset = read_u32(raw_bytes, byte_order) as u64;
        visited.push(sub_offset);

        if *tag_id != EXIF_IFD_POINTER {
            continue;
        }
        let Ok(exif_ifd) = parse_ifd(reader, sub_offset, byte_order) else {
            continue;
        };
        for (sub_tag, _, _, sub_bytes) in &exif_ifd {
            if *sub_tag == INTEROPERABILITY_IFD_POINTER && sub_bytes.len() >= 4 {
                visited.push(read_u32(sub_bytes, byte_order) as u64);
            }
        }
    }

    visited
}

/// Reads an IFD1 offset/length field as an unsigned integer.
///
/// 0x0201/0x0202 are LONG in the EXIF spec, but real files also store them as
/// SHORT - `LeicaM8.jpg` and `LeicaM8.2.jpg` both write ThumbnailOffset as
/// SHORT. A fixed 4-byte read of those entries sees a 2-byte inline slice and
/// silently yields 0, so go through the type-aware converter instead.
fn read_unsigned_field(
    raw_bytes: &[u8],
    field_type: u16,
    value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> Option<u64> {
    let value = raw_bytes_to_tag_value(raw_bytes, field_type, value_count, tag_id, byte_order)
        .as_integer()?;
    u64::try_from(value).ok()
}

/// Parses the thumbnail IFD (IFD1) that follows IFD0 and emits the thumbnail tags.
///
/// A JPEG's APP1 EXIF payload is a TIFF structure whose IFD0 carries a
/// next-IFD pointer to IFD1, which holds the embedded thumbnail. Callers that
/// only walk IFD0 never surface `Compression`, `ThumbnailOffset`,
/// `ThumbnailLength` or `ThumbnailImage`.
///
/// # Offset semantics
///
/// `ThumbnailOffset` is stored in the file relative to the TIFF header, but
/// ExifTool reports it as an absolute file offset - it adds the base at which
/// the TIFF header sits. For a JPEG that base is the byte after the
/// `Exif\0\0` APP1 header (12 for a bare APP1, 30 when a JFIF APP0 precedes
/// it); for a standalone TIFF it is 0. `tiff_base` supplies that value, while
/// `reader` must address the TIFF structure itself (offset 0 == TIFF header).
///
/// # Arguments
///
/// * `reader` - Reader addressing the TIFF structure (TIFF-relative offsets)
/// * `ifd0_offset` - Offset of IFD0 within the TIFF structure
/// * `ifd0_entry_count` - Number of entries in IFD0, used to find its next-IFD pointer
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `tiff_base` - Absolute file offset of the TIFF header, added to `ThumbnailOffset`
/// * `metadata` - MetadataMap to populate
pub fn parse_ifd1_thumbnail(
    reader: &dyn FileReader,
    ifd0_offset: u64,
    ifd0_entry_count: usize,
    byte_order: ByteOrder,
    tiff_base: u64,
    metadata: &mut MetadataMap,
) {
    let Some(ifd1_offset) = next_ifd_offset(reader, ifd0_offset, ifd0_entry_count, byte_order)
    else {
        // No IFD1: emit nothing. A wrong thumbnail tag is worse than a missing one.
        return;
    };

    // An IFD1 pointer aimed at a directory the EXIF walk already visited is a
    // malformed file, not a thumbnail. ExifTool skips the directory outright.
    if visited_directory_offsets(reader, ifd0_offset, byte_order).contains(&ifd1_offset) {
        return;
    }

    let Ok(entries) = parse_ifd(reader, ifd1_offset, byte_order) else {
        return;
    };

    let mut compression: Option<i64> = None;
    let mut thumb_offset: Option<u64> = None;
    let mut thumb_length: Option<u64> = None;

    for (tag_id, field_type, value_count, raw_bytes) in &entries {
        match *tag_id {
            TAG_COMPRESSION => {
                compression = raw_bytes_to_tag_value(
                    raw_bytes,
                    *field_type,
                    *value_count,
                    *tag_id,
                    byte_order,
                )
                .as_integer();
            }
            TAG_THUMBNAIL_OFFSET => {
                thumb_offset =
                    read_unsigned_field(raw_bytes, *field_type, *value_count, *tag_id, byte_order);
            }
            TAG_THUMBNAIL_LENGTH => {
                thumb_length =
                    read_unsigned_field(raw_bytes, *field_type, *value_count, *tag_id, byte_order);
            }
            _ => {}
        }
    }

    // Compression carries the standard PrintConv ("JPEG (old-style)" for a
    // thumbnail); the exiftool_compat layer applies it to the integer.
    //
    // Precedence: when a file carries Compression in BOTH IFD0 and IFD1, the
    // two collapse onto a single `EXIF:Compression` in the family-normalised
    // view, and ExifTool's default (duplicate-suppressed) output reports the
    // IFD0 one - e.g. OlympusAIR-A01.jpg is `Uncompressed` (IFD0), not
    // `JPEG (old-style)` (IFD1). Yield to IFD0 so the thumbnail IFD never
    // rewrites the main image's value. An image-carrying Interoperability IFD
    // (see `parse_interop_subifd`) also wins: it is walked before IFD1, and
    // with both copies at ExifTool priority 0 the first-extracted one is the
    // one ExifTool displays.
    if let Some(value) = compression
        && metadata.get("IFD0:Compression").is_none()
        && metadata.get("InteropIFD:Compression").is_none()
    {
        metadata.insert(
            lookup_tag_name(TAG_COMPRESSION, "IFD1"),
            TagValue::new_integer(value),
        );
    }

    // ExifTool emits the offset/length pair only when both are present.
    let (Some(offset), Some(length)) = (thumb_offset, thumb_length) else {
        return;
    };

    let Some(absolute_offset) = offset.checked_add(tiff_base) else {
        return;
    };

    // Named explicitly rather than via `lookup_tag_name`: 0x0201/0x0202 are
    // registered under their spec names (JPEGInterchangeFormat/Length) and the
    // reverse index resolves to those, but ExifTool names them contextually -
    // inside IFD1 they print as ThumbnailOffset/ThumbnailLength.
    metadata.insert(
        "IFD1:ThumbnailOffset",
        TagValue::new_integer(absolute_offset as i64),
    );
    metadata.insert("IFD1:ThumbnailLength", TagValue::new_integer(length as i64));

    // ThumbnailImage is the bytes the offset/length pair points at. The bytes
    // are NOT required to be a JPEG - ExifTool's ValidateImage only rejects a
    // non-JPEG payload when the tag was named on the command line, so a plain
    // extraction reports whatever is there.
    //
    // We emit it only when the range is actually readable. ExifTool is looser:
    // with the Binary option off, ExtractBinary returns the literal string
    // "Binary data $length bytes" *without seeking*, so it prints a thumbnail
    // for a range that runs past EOF (PanasonicDMC-LS60.jpg and
    // SonyCLIE_PEG-NZ90.jpg in the corpus are both truncated this way). Since
    // we report a real byte count from real bytes, an unreadable range gets no
    // tag rather than an invented length.
    if length == 0 || length > MAX_THUMBNAIL_BYTES {
        return;
    }
    let Some(end) = offset.checked_add(length) else {
        return;
    };
    if end > reader.size() {
        return;
    }
    let Ok(bytes) = reader.read(offset, length as usize) else {
        return;
    };
    // A reader that clamps a short read instead of failing would otherwise make
    // us report a byte count the file does not actually contain.
    if bytes.len() as u64 != length {
        return;
    }
    metadata.insert("IFD1:ThumbnailImage", TagValue::new_binary(bytes.to_vec()));
}

/// Locates a MakerNote inside its enclosing TIFF block.
///
/// A MakerNote is one EXIF entry with a declared byte count, but the offsets
/// inside it are measured from the enclosing TIFF header and routinely address
/// bytes past that count -- `NikonCoolpixS8200.jpg` declares 2219 bytes and
/// puts the last four bytes of `NEFBitDepth` outside them, and a Sigma value
/// offset addresses the TIFF header outright. `parse_ifd` hands back a copy of
/// the declared block, which loses both the position those offsets count from
/// and the bytes past the end, so the entry is re-read here for its position.
///
/// The context is bounded three ways over. `tiff_len` (ExifTool's `$dataLen`,
/// and never more than the reader holds) caps how far it can reach. The
/// MakerNote's own value must not overlap the IFD that declared it, which is
/// ExifTool's "Suspicious offset" test ([`value_overlaps_directory`]) and means
/// the entry is not describing a real block. And the resulting `payload()` must
/// be byte-identical to the block `parse_ifd` already resolved -- if it is not,
/// the entry does not describe the bytes we hold and the widened window would
/// be addressing something else entirely.
///
/// Falls back to [`MakerNoteContext::detached`] -- the payload alone, exactly
/// the reach decoders had before contexts existed -- whenever the enclosing
/// block cannot be established this way.
fn makernote_context<'a>(
    reader: &'a dyn FileReader,
    ifd_offset: u64,
    byte_order: ByteOrder,
    tiff_base: u64,
    tiff_len: u64,
    payload: &'a [u8],
) -> MakerNoteContext<'a> {
    let detached = MakerNoteContext::detached(payload);

    let Some(entry) = find_entry_position(reader, ifd_offset, byte_order, MAKERNOTE) else {
        return detached;
    };
    // A value of four bytes or fewer is stored in the entry's offset field
    // itself, so there is no position in the block to widen from.
    if entry.value_len <= 4 {
        return detached;
    }

    let block_len = tiff_len.min(reader.size());
    let (Ok(block_len), Ok(value_offset), Ok(value_len), Ok(dir_start), Ok(dir_end)) = (
        usize::try_from(block_len),
        usize::try_from(entry.value_offset),
        usize::try_from(entry.value_len),
        usize::try_from(ifd_offset),
        usize::try_from(entry.dir_end),
    ) else {
        return detached;
    };
    // A MakerNote whose value runs back over the entry list that declared it is
    // what ExifTool warns about and drops, not a block to widen into.
    if value_overlaps_directory(value_offset, value_len, dir_start, dir_end) {
        return detached;
    }
    let Ok(tiff) = reader.read(0, block_len) else {
        return detached;
    };

    let ctx = MakerNoteContext::in_tiff(tiff, value_offset, value_len, tiff_base);
    if ctx.payload() == payload {
        ctx
    } else {
        detached
    }
}

/// Parses MakerNote data for any supported manufacturer.
///
/// Camera manufacturers store proprietary metadata in MakerNote tags.
/// This function dispatches to the appropriate manufacturer parser based on
/// the camera make detected from the TIFF metadata.
///
/// # Arguments
///
/// * `ctx` - Where the MakerNote sits, and how far its decoder may read
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `metadata` - MetadataMap to populate with manufacturer-specific tags
fn parse_makernote(ctx: &MakerNoteContext<'_>, byte_order: ByteOrder, metadata: &mut MetadataMap) {
    // Extract camera make from metadata to determine which parser to use
    let make = metadata.get_string("IFD0:Make").unwrap_or("").to_string();
    // A few MakerNote sub-structures are laid out per camera model rather than
    // self-describing (Nikon AFInfo's byte order, for one).
    let model = metadata.get_string("IFD0:Model").map(str::to_string);

    if make.is_empty() {
        return;
    }

    // Sigma reads the same context, but writes `MetadataMap` rather than the
    // `HashMap<String, String>` the dispatcher's trait returns: its
    // `PreviewImage` is binary and its `PreviewImageStart` an integer, neither
    // of which survives a string map. See `makernotes::sigma`.
    if parse_sigma_makernote_if_sigma(&make, ctx, metadata) {
        return;
    }

    // Parse MakerNote using the dispatcher
    let mut makernote_tags = HashMap::new();
    let mut value_forms = HashMap::new();
    if let Err(_e) = dispatch_makernote_with_context_and_values(
        &make,
        model.as_deref(),
        ctx,
        byte_order,
        &mut makernote_tags,
        &mut value_forms,
    ) {
        // Silently skip failed MakerNote parsing
        return;
    }

    // Add manufacturer tags to metadata
    // Note: tag names already include manufacturer prefix (e.g., "Canon:", "Nikon:")
    for (tag_name, tag_value_str) in makernote_tags {
        // ExifTool gives the standard IFD0 PrintIM directory higher priority
        // than a MakerNote copy. Minolta.jpg carries 0250 in IFD0 and 0100 in
        // its MakerNote; the default visible value is 0250.
        if tag_name == PRINT_IM_VERSION_TAG && metadata.contains_key(PRINT_IM_VERSION_TAG) {
            continue;
        }
        // Convert string value to TagValue
        let tag_value = TagValue::String(tag_value_str);
        metadata.insert(tag_name, tag_value);
    }
    for (tag_name, value) in value_forms {
        metadata.set_value_form(tag_name, value);
    }
}

/// Decodes a Sigma/Foveon MakerNote, and reports whether it did.
///
/// Sigma consumes the same [`MakerNoteContext`] every other make now gets; what
/// keeps it out of the dispatcher is its *output*, not its input. `MakerNotes:
/// PreviewImage` is binary and `PreviewImageStart` an integer, and the
/// `MakerNoteParser` trait returns `HashMap<String, String>`.
///
/// Returns `false` for any other make, leaving the caller to dispatch normally.
fn parse_sigma_makernote_if_sigma(
    make: &str,
    ctx: &MakerNoteContext<'_>,
    metadata: &mut MetadataMap,
) -> bool {
    if !matches!(
        make.trim().to_ascii_lowercase().as_str(),
        "sigma" | "sigma corporation" | "foveon"
    ) {
        return false;
    }

    crate::parsers::tiff::makernotes::sigma::parse_sigma_makernote(
        ctx.tiff(),
        ctx.payload_offset(),
        ctx.tiff_base(),
        metadata,
    );
    true
}

#[cfg(test)]
mod ifd1_tests {
    use super::*;
    use crate::test_support::TestReader;

    const SHORT: u16 = 3;
    const LONG: u16 = 4;

    /// Builds a little-endian TIFF whose IFD0 is empty and whose IFD1 holds the
    /// supplied `(tag, type, value)` entries, followed by `thumb` bytes.
    ///
    /// Layout: header(8) | IFD0 count+next(6) | IFD1 | thumbnail
    /// so IFD0 sits at 8, its next-IFD pointer at 10, and IFD1 at 14.
    fn build_tiff(entries: &[(u16, u16, u32)], thumb: &[u8]) -> (Vec<u8>, u64) {
        let ifd1_offset = 14u32;
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at 8
        data.extend_from_slice(&0u16.to_le_bytes()); // IFD0 entry count
        data.extend_from_slice(&ifd1_offset.to_le_bytes()); // IFD0 -> IFD1

        data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, field_type, value) in entries {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&field_type.to_le_bytes());
            data.extend_from_slice(&1u32.to_le_bytes()); // value count
            // Values are left-justified in the 4-byte field, so a SHORT lands in
            // the low two bytes exactly as a little-endian u32 write puts it.
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.extend_from_slice(&0u32.to_le_bytes()); // no IFD2

        let thumb_offset = data.len() as u32;
        data.extend_from_slice(thumb);
        (data, thumb_offset as u64)
    }

    fn run(entries: &[(u16, u16, u32)], thumb: &[u8], tiff_base: u64) -> MetadataMap {
        let (data, _) = build_tiff(entries, thumb);
        let reader = TestReader::new(data);
        let mut metadata = MetadataMap::new();
        parse_ifd1_thumbnail(
            &reader,
            8,
            0,
            ByteOrder::LittleEndian,
            tiff_base,
            &mut metadata,
        );
        metadata
    }

    #[test]
    fn short_typed_thumbnail_offset_is_not_read_as_zero() {
        // LeicaM8.jpg / LeicaM8.2.jpg store ThumbnailOffset as SHORT, not LONG.
        // A fixed 4-byte read sees a 2-byte inline slice and yields 0.
        let thumb = [0xFFu8, 0xD8, 0xFF, 0xDB];
        let (_, thumb_offset) = build_tiff(
            &[
                (TAG_COMPRESSION, SHORT, 6),
                (TAG_THUMBNAIL_OFFSET, SHORT, 0),
                (TAG_THUMBNAIL_LENGTH, LONG, thumb.len() as u32),
            ],
            &thumb,
        );
        let metadata = run(
            &[
                (TAG_COMPRESSION, SHORT, 6),
                (TAG_THUMBNAIL_OFFSET, SHORT, thumb_offset as u32),
                (TAG_THUMBNAIL_LENGTH, LONG, thumb.len() as u32),
            ],
            &thumb,
            12,
        );

        assert_eq!(
            metadata
                .get("IFD1:ThumbnailOffset")
                .and_then(|v| v.as_integer()),
            Some(thumb_offset as i64 + 12),
            "SHORT-typed ThumbnailOffset must survive, with the TIFF base added"
        );
        assert_eq!(
            metadata
                .get("IFD1:ThumbnailLength")
                .and_then(|v| v.as_integer()),
            Some(thumb.len() as i64)
        );
        assert!(matches!(
            metadata.get("IFD1:ThumbnailImage"),
            Some(TagValue::Binary(bytes)) if bytes == &thumb
        ));
    }

    #[test]
    fn long_typed_offset_reports_absolute_position() {
        let thumb = [0xFFu8, 0xD8, 0xFF, 0xDB, 0x00, 0x01];
        let (_, thumb_offset) = build_tiff(
            &[
                (TAG_THUMBNAIL_OFFSET, LONG, 0),
                (TAG_THUMBNAIL_LENGTH, LONG, thumb.len() as u32),
            ],
            &thumb,
        );
        let metadata = run(
            &[
                (TAG_THUMBNAIL_OFFSET, LONG, thumb_offset as u32),
                (TAG_THUMBNAIL_LENGTH, LONG, thumb.len() as u32),
            ],
            &thumb,
            30,
        );
        assert_eq!(
            metadata
                .get("IFD1:ThumbnailOffset")
                .and_then(|v| v.as_integer()),
            Some(thumb_offset as i64 + 30)
        );
    }

    #[test]
    fn ifd1_pointer_aimed_at_an_already_visited_directory_is_refused() {
        // SamsungGT-S5620.jpg aims IFD1 at the GPS IFD; ExifTool warns "IFD1
        // pointer references previous GPS directory" and reports no thumbnail.
        // Reading that directory as IFD1 would invent one.
        let gps_offset = 26u32;
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at 8
        data.extend_from_slice(&1u16.to_le_bytes()); // one entry
        data.extend_from_slice(&0x8825u16.to_le_bytes()); // GPS sub-IFD pointer
        data.extend_from_slice(&LONG.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&gps_offset.to_le_bytes());
        data.extend_from_slice(&gps_offset.to_le_bytes()); // IFD0 -> GPS, not IFD1
        assert_eq!(data.len(), gps_offset as usize);

        // A GPS directory whose entries would read as thumbnail tags.
        data.extend_from_slice(&2u16.to_le_bytes());
        for (tag, value) in [(TAG_THUMBNAIL_OFFSET, 60u32), (TAG_THUMBNAIL_LENGTH, 4u32)] {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&LONG.to_le_bytes());
            data.extend_from_slice(&1u32.to_le_bytes());
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xDB]);

        let reader = TestReader::new(data);
        let mut metadata = MetadataMap::new();
        parse_ifd1_thumbnail(&reader, 8, 1, ByteOrder::LittleEndian, 0, &mut metadata);
        assert!(metadata.get("IFD1:ThumbnailOffset").is_none());
        assert!(metadata.get("IFD1:ThumbnailLength").is_none());
        assert!(metadata.get("IFD1:ThumbnailImage").is_none());
    }

    #[test]
    fn zero_length_thumbnail_keeps_the_pair_but_emits_no_image() {
        // ExifTool.jpg and XMP.jpg both report ThumbnailOffset/Length with
        // Length 0 and no ThumbnailImage.
        let metadata = run(
            &[
                (TAG_THUMBNAIL_OFFSET, LONG, 100),
                (TAG_THUMBNAIL_LENGTH, LONG, 0),
            ],
            &[],
            0,
        );
        assert_eq!(
            metadata
                .get("IFD1:ThumbnailLength")
                .and_then(|v| v.as_integer()),
            Some(0)
        );
        assert!(metadata.get("IFD1:ThumbnailImage").is_none());
    }

    #[test]
    fn ifd1_compression_yields_to_ifd0() {
        // OlympusAIR-A01.jpg carries Compression in both IFDs; ExifTool's
        // duplicate-suppressed output reports IFD0's "Uncompressed".
        let (data, _) = build_tiff(&[(TAG_COMPRESSION, SHORT, 6)], &[]);
        let reader = TestReader::new(data);
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Compression", TagValue::new_integer(1));
        parse_ifd1_thumbnail(&reader, 8, 0, ByteOrder::LittleEndian, 0, &mut metadata);
        assert_eq!(
            metadata
                .get("IFD0:Compression")
                .and_then(|v| v.as_integer()),
            Some(1)
        );
        assert!(metadata.get("IFD1:Compression").is_none());
    }

    #[test]
    fn ifd1_compression_yields_to_interop() {
        // An image-carrying Interop IFD is walked before IFD1; with both
        // copies at ExifTool priority 0 the first-extracted (Interop) one is
        // the one ExifTool displays.
        let (data, _) = build_tiff(&[(TAG_COMPRESSION, SHORT, 6)], &[]);
        let reader = TestReader::new(data);
        let mut metadata = MetadataMap::new();
        metadata.insert("InteropIFD:Compression", TagValue::new_integer(6));
        parse_ifd1_thumbnail(&reader, 8, 0, ByteOrder::LittleEndian, 0, &mut metadata);
        assert!(metadata.get("IFD1:Compression").is_none());
    }

    #[test]
    fn absent_ifd1_emits_nothing() {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // no IFD1
        let reader = TestReader::new(data);
        let mut metadata = MetadataMap::new();
        parse_ifd1_thumbnail(&reader, 8, 0, ByteOrder::LittleEndian, 12, &mut metadata);
        assert!(metadata.get("IFD1:ThumbnailOffset").is_none());
        assert!(metadata.get("IFD1:Compression").is_none());
    }
}

#[cfg(test)]
mod interop_tests {
    use super::*;
    use crate::test_support::TestReader;

    const ASCII: u16 = 2;
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    const RATIONAL: u16 = 5;

    /// Builds a little-endian buffer whose Interoperability IFD sits at offset
    /// 8 and holds the supplied raw `(tag, type, count, value_field)` entries,
    /// followed by `tail` bytes (offset-stored values and/or an embedded
    /// image). Returns the buffer and the offset at which `tail` begins.
    fn build_interop(entries: &[(u16, u16, u32, u32)], tail: &[u8]) -> (Vec<u8>, u64) {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD at 8
        data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, field_type, count, value) in entries {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&field_type.to_le_bytes());
            data.extend_from_slice(&count.to_le_bytes());
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        let tail_offset = data.len() as u64;
        data.extend_from_slice(tail);
        (data, tail_offset)
    }

    fn run(entries: &[(u16, u16, u32, u32)], tail: &[u8], tiff_base: u64) -> MetadataMap {
        let (data, _) = build_interop(entries, tail);
        let reader = TestReader::new(data);
        let mut metadata = MetadataMap::new();
        parse_interop_subifd(
            &reader,
            8,
            ByteOrder::LittleEndian,
            tiff_base,
            &mut metadata,
        );
        metadata
    }

    /// The tail offset for `n` entries: header(8) + count(2) + 12n + next(4).
    fn tail_offset_for(entry_count: u64) -> u64 {
        8 + 2 + 12 * entry_count + 4
    }

    #[test]
    fn image_carrying_interop_names_the_pair_other_image_not_thumbnail() {
        // SamsungSPH-A800.jpg / SamsungSPH-A940.jpg / CanonXL_H1.jpg embed an
        // image via 0x0201/0x0202 in the Interop IFD. ExifTool names that
        // pair ThumbnailOffset/ThumbnailLength ONLY in IFD1; here it must be
        // OtherImageStart/OtherImageLength (+ derived OtherImage).
        let blob = [0xFFu8, 0xD8, 0xFF, 0xDB];
        let blob_offset = tail_offset_for(3);
        let metadata = run(
            &[
                (TAG_COMPRESSION, SHORT, 1, 6),
                (TAG_OTHER_IMAGE_START, LONG, 1, blob_offset as u32),
                (TAG_OTHER_IMAGE_LENGTH, LONG, 1, blob.len() as u32),
            ],
            &blob,
            30,
        );

        // Offset is absolutised with the TIFF base, exactly like ThumbnailOffset.
        assert_eq!(
            metadata
                .get("InteropIFD:OtherImageStart")
                .and_then(|v| v.as_integer()),
            Some(blob_offset as i64 + 30)
        );
        assert_eq!(
            metadata
                .get("InteropIFD:OtherImageLength")
                .and_then(|v| v.as_integer()),
            Some(blob.len() as i64)
        );
        assert!(matches!(
            metadata.get("InteropIFD:OtherImage"),
            Some(TagValue::Binary(bytes)) if bytes == &blob
        ));
        assert_eq!(
            metadata
                .get("InteropIFD:Compression")
                .and_then(|v| v.as_integer()),
            Some(6)
        );

        // The naming rule: never the IFD1-only names, under any group.
        for key in [
            "InteropIFD:ThumbnailOffset",
            "InteropIFD:ThumbnailLength",
            "InteropIFD:ThumbnailImage",
            "IFD1:ThumbnailOffset",
            "IFD1:ThumbnailLength",
            "IFD1:ThumbnailImage",
        ] {
            assert!(metadata.get(key).is_none(), "{} must not be emitted", key);
        }
    }

    #[test]
    fn resolution_tags_yield_to_ifd0_twins() {
        // ExifTool's default (duplicate-suppressed) output does not repeat
        // XResolution/YResolution/ResolutionUnit out of the Interop IFD when
        // IFD0 already owns them - all three target corpus files are built
        // this way. Compression has no IFD0 twin here, so it IS emitted.
        let mut rational_tail = Vec::new();
        rational_tail.extend_from_slice(&72u32.to_le_bytes());
        rational_tail.extend_from_slice(&1u32.to_le_bytes());
        let x_res_offset = tail_offset_for(3) as u32;

        let (data, _) = build_interop(
            &[
                (TAG_COMPRESSION, SHORT, 1, 6),
                (TAG_X_RESOLUTION, RATIONAL, 1, x_res_offset),
                (TAG_RESOLUTION_UNIT, SHORT, 1, 2),
            ],
            &rational_tail,
        );
        let reader = TestReader::new(data);
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:XResolution", TagValue::new_rational(72, 1));
        metadata.insert("IFD0:ResolutionUnit", TagValue::new_integer(2));
        parse_interop_subifd(&reader, 8, ByteOrder::LittleEndian, 0, &mut metadata);

        assert!(metadata.get("InteropIFD:XResolution").is_none());
        assert!(metadata.get("InteropIFD:ResolutionUnit").is_none());
        assert_eq!(
            metadata
                .get("InteropIFD:Compression")
                .and_then(|v| v.as_integer()),
            Some(6)
        );
    }

    #[test]
    fn resolution_tags_emit_when_ifd0_lacks_them() {
        let mut rational_tail = Vec::new();
        rational_tail.extend_from_slice(&72u32.to_le_bytes());
        rational_tail.extend_from_slice(&1u32.to_le_bytes());
        let x_res_offset = tail_offset_for(2) as u32;

        let metadata = run(
            &[
                (TAG_X_RESOLUTION, RATIONAL, 1, x_res_offset),
                (TAG_RESOLUTION_UNIT, SHORT, 1, 2),
            ],
            &rational_tail,
            0,
        );

        assert!(matches!(
            metadata.get("InteropIFD:XResolution"),
            Some(TagValue::Rational {
                numerator: 72,
                denominator: 1
            })
        ));
        assert_eq!(
            metadata
                .get("InteropIFD:ResolutionUnit")
                .and_then(|v| v.as_integer()),
            Some(2)
        );
    }

    #[test]
    fn unreadable_other_image_range_keeps_pair_but_no_blob() {
        // Mirror of the thumbnail rule: the offset/length pair reports the
        // stored numbers, but a blob is only materialised from readable bytes.
        let metadata = run(
            &[
                (TAG_OTHER_IMAGE_START, LONG, 1, 4000),
                (TAG_OTHER_IMAGE_LENGTH, LONG, 1, 100),
            ],
            &[],
            12,
        );
        assert_eq!(
            metadata
                .get("InteropIFD:OtherImageStart")
                .and_then(|v| v.as_integer()),
            Some(4012)
        );
        assert_eq!(
            metadata
                .get("InteropIFD:OtherImageLength")
                .and_then(|v| v.as_integer()),
            Some(100)
        );
        assert!(metadata.get("InteropIFD:OtherImage").is_none());
    }

    #[test]
    fn lone_offset_without_length_emits_no_pair() {
        let metadata = run(&[(TAG_OTHER_IMAGE_START, LONG, 1, 60)], &[], 0);
        assert!(metadata.get("InteropIFD:OtherImageStart").is_none());
        assert!(metadata.get("InteropIFD:OtherImageLength").is_none());
        assert!(metadata.get("InteropIFD:OtherImage").is_none());
    }

    #[test]
    fn dcf_conformance_tags_keep_their_exif_keys() {
        // The pre-existing DCF path is untouched: InteropIndex still lands on
        // "EXIF:InteropIndex" (the key the surgical writer anticipates) with
        // ExifTool's expanded description.
        let metadata = run(
            &[(INTEROP_INDEX, ASCII, 4, u32::from_le_bytes(*b"R98\0"))],
            &[],
            0,
        );
        assert_eq!(
            metadata
                .get("EXIF:InteropIndex")
                .and_then(|v| v.as_string()),
            Some("R98 - DCF basic file (sRGB)")
        );
    }
}

#[cfg(test)]
mod makernote_window_tests {
    use super::*;
    use crate::test_support::TestReader;

    const UNDEFINED: u16 = 7;
    const LONG: u16 = 4;

    /// Builds a little-endian TIFF whose IFD at offset 8 holds a single entry
    /// `(tag, type, count, value_offset)`, followed by `tail` bytes laid down
    /// at `value_at`.
    ///
    /// The interesting shape is the real one: a MakerNote entry that declares
    /// fewer bytes than the values behind it actually occupy, so the last of
    /// them sits outside the declared block.
    fn tiff_with_entry(
        tag: u16,
        field_type: u16,
        count: u32,
        value_offset: u32,
        value_at: usize,
        value: &[u8],
        trailing: usize,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes()); // one entry
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&field_type.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        data.extend_from_slice(&value_offset.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        data.resize(value_at, 0);
        data.extend_from_slice(value);
        data.resize(data.len() + trailing, 0xAB);
        data
    }

    #[test]
    fn the_window_reaches_past_the_declared_makernote_block() {
        // A 16-byte MakerNote at offset 32, with 24 more bytes of file after
        // it -- the NEFBitDepth shape, where the value the last entry points
        // at runs off the end of the declared count.
        let payload: Vec<u8> = (0u8..16).collect();
        let data = tiff_with_entry(MAKERNOTE, UNDEFINED, 16, 32, 32, &payload, 24);
        let reader = TestReader::new(data.clone());

        let ctx = makernote_context(
            &reader,
            8,
            ByteOrder::LittleEndian,
            0,
            data.len() as u64,
            &payload,
        );

        assert_eq!(ctx.payload(), &payload[..], "declared block is unchanged");
        assert!(ctx.is_widened());
        assert_eq!(ctx.window().len(), data.len() - 32);
        assert_eq!(ctx.window()[..16], payload[..], "window starts at payload");
        assert_eq!(ctx.payload_offset(), 32);
    }

    #[test]
    fn the_window_stops_at_the_declared_tiff_block_not_the_reader() {
        // `tiff_len` is ExifTool's $dataLen: a JPEG's reader runs to the end of
        // the file, but the EXIF block does not, and the MakerNote must not
        // reach into the compressed scan data behind it.
        let payload: Vec<u8> = (0u8..16).collect();
        let data = tiff_with_entry(MAKERNOTE, UNDEFINED, 16, 32, 32, &payload, 4096);
        let reader = TestReader::new(data);

        let ctx = makernote_context(&reader, 8, ByteOrder::LittleEndian, 0, 64, &payload);

        assert_eq!(ctx.window().len(), 64 - 32);
    }

    #[test]
    fn a_payload_the_entry_does_not_describe_falls_back_to_detached() {
        // The guard that keeps a widened window from addressing something else
        // entirely: the context is only used when its own payload is byte-
        // identical to the block `parse_ifd` already resolved.
        let payload: Vec<u8> = (0u8..16).collect();
        let data = tiff_with_entry(MAKERNOTE, UNDEFINED, 16, 32, 32, &payload, 24);
        let reader = TestReader::new(data);

        let someone_elses = vec![0xFFu8; 16];
        let ctx = makernote_context(&reader, 8, ByteOrder::LittleEndian, 0, 64, &someone_elses);

        assert!(!ctx.is_widened());
        assert_eq!(ctx.payload(), &someone_elses[..]);
    }

    #[test]
    fn an_inline_value_has_no_position_to_widen_from() {
        // Four bytes or fewer live in the entry's offset field, so there is no
        // offset into the block to extend.
        let payload = vec![1u8, 2, 3, 4];
        let data = tiff_with_entry(MAKERNOTE, UNDEFINED, 4, 0x04030201, 32, &payload, 24);
        let reader = TestReader::new(data);

        let ctx = makernote_context(&reader, 8, ByteOrder::LittleEndian, 0, 64, &payload);

        assert!(!ctx.is_widened());
        assert_eq!(ctx.payload(), &payload[..]);
    }

    #[test]
    fn a_value_that_runs_back_over_the_entry_list_is_refused() {
        // ExifTool's "Suspicious MakerNotes offset": a value overlapping the
        // directory that declared it is not a block to widen into. The IFD here
        // spans 8..26 (count + one 12-byte entry + next pointer), and the entry
        // claims its value starts at 12 -- inside itself.
        let payload: Vec<u8> = (0u8..16).collect();
        let mut data = tiff_with_entry(MAKERNOTE, UNDEFINED, 16, 12, 32, &payload, 24);
        // Make the claimed payload match what sits at offset 12 so that only
        // the overlap test can reject it.
        let claimed = data[12..28].to_vec();
        data.resize(data.len(), 0);
        let reader = TestReader::new(data);

        let ctx = makernote_context(&reader, 8, ByteOrder::LittleEndian, 0, 64, &claimed);

        assert!(!ctx.is_widened());
        assert_eq!(ctx.payload(), &claimed[..]);
    }

    #[test]
    fn an_ifd_without_a_makernote_entry_falls_back_to_detached() {
        let payload: Vec<u8> = (0u8..16).collect();
        let data = tiff_with_entry(0x0100, LONG, 1, 32, 32, &payload, 24);
        let reader = TestReader::new(data);

        let ctx = makernote_context(&reader, 8, ByteOrder::LittleEndian, 0, 64, &payload);

        assert!(!ctx.is_widened());
    }

    #[test]
    fn the_tiff_base_travels_with_the_context() {
        // Sigma's PreviewImageStart is `IsOffset => 1`, so the block's own file
        // position has to survive the trip.
        let payload: Vec<u8> = (0u8..16).collect();
        let data = tiff_with_entry(MAKERNOTE, UNDEFINED, 16, 32, 32, &payload, 24);
        let reader = TestReader::new(data);

        let ctx = makernote_context(&reader, 8, ByteOrder::LittleEndian, 292, 64, &payload);

        assert_eq!(ctx.tiff_base(), 292);
        assert_eq!(ctx.tiff()[0..2], *b"II", "index 0 is the TIFF header");
    }
}
