//! TIFF metadata parsing helpers
//!
//! This module contains helper functions for parsing TIFF IFD structures,
//! processing tags, and handling sub-IFDs (EXIF, GPS), MakerNotes, and GeoTiff.

use super::{FileReader, MetadataMap, TagValue};
use crate::core::formatters::composite_image_exposure_times::format_composite_image_exposure_times;
use crate::core::operations_helpers::read_u32;
use crate::core::tag_conversion::{
    apply_tile_offsets_value_conv, gps_coordinate_degrees, raw_bytes_to_tag_value,
};
use crate::parsers::common::print_im::{PRINT_IM_VERSION_TAG, decode_print_im_version};
use crate::parsers::tiff::geotiff_parser;
use crate::parsers::tiff::ifd_parser::{
    ByteOrder, find_entry_position, ifd_entry_count, parse_ifd,
};
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
const TAG_SUBFILE_TYPE: u16 = 0x00FE;

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

    #[test]
    fn linked_tiff_ifds_emit_file_page_count() {
        // Three empty top-level IFDs linked at offsets 8, 14, and 20.
        let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
        tiff.extend_from_slice(&0_u16.to_le_bytes());
        tiff.extend_from_slice(&14_u32.to_le_bytes());
        tiff.extend_from_slice(&0_u16.to_le_bytes());
        tiff.extend_from_slice(&20_u32.to_le_bytes());
        tiff.extend_from_slice(&0_u16.to_le_bytes());
        tiff.extend_from_slice(&0_u32.to_le_bytes());

        let reader = TestReader::new(tiff);
        let mut metadata = MetadataMap::new();
        parse_ifd_chain(&reader, 8, ByteOrder::LittleEndian, &mut metadata).unwrap();

        assert_eq!(metadata.get_integer("File:PageCount"), Some(3));
    }
}

#[cfg(test)]
mod software_tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn software_trims_trailing_whitespace_like_exiftool() {
        // ExifTool 13.59 Exif.pm 0x0131 uses
        // `$val =~ s/\s+$//; $$self{Software} = $val`.  The whitespace here
        // is part of the on-disk ASCII value, before its NUL terminator.
        let software = b"Capture One \t\n\0";
        let mut tiff = b"II\x2a\0\x08\0\0\0\x01\0".to_vec();
        tiff.extend_from_slice(&0x0131u16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes()); // ASCII
        tiff.extend_from_slice(&(software.len() as u32).to_le_bytes());
        tiff.extend_from_slice(&26u32.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        tiff.extend_from_slice(software);

        let reader = TestReader::new(tiff);
        let mut metadata = MetadataMap::new();
        parse_ifd_chain(&reader, 8, ByteOrder::LittleEndian, &mut metadata).unwrap();

        assert_eq!(metadata.get_string("IFD0:Software"), Some("Capture One"));
    }
}

#[cfg(test)]
mod tile_offsets_tests {
    use super::*;
    use crate::test_support::TestReader;

    fn push_entry(out: &mut Vec<u8>, tag: u16, field_type: u16, count: u32, value: u32) {
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn long_tile_offsets_use_exiftools_valueconv_binary_reference() {
        // ExifTool 13.59 Exif.pm 0x144:
        // ValueConv => 'length($val) > 32 ? \\$val : $val'.  Ten four-digit
        // offsets produce a 49-byte, space-separated value string.
        let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
        tiff.extend_from_slice(&4_u16.to_le_bytes());
        push_entry(&mut tiff, 0x0100, 4, 1, 10);
        push_entry(&mut tiff, 0x0101, 4, 1, 10);
        push_entry(&mut tiff, 0x0144, 4, 10, 62);
        push_entry(&mut tiff, 0x0145, 4, 10, 102);
        tiff.extend_from_slice(&0_u32.to_le_bytes());
        for offset in 1000_u32..1010 {
            tiff.extend_from_slice(&offset.to_le_bytes());
        }
        for _ in 0..10 {
            tiff.extend_from_slice(&1_u32.to_le_bytes());
        }

        let reader = TestReader::new(tiff);
        let mut metadata = MetadataMap::new();
        parse_ifd_chain(&reader, 8, ByteOrder::LittleEndian, &mut metadata).unwrap();

        assert_eq!(
            metadata.get("IFD0:TileOffsets"),
            Some(&TagValue::Binary(
                b"1000 1001 1002 1003 1004 1005 1006 1007 1008 1009".to_vec()
            ))
        );
        assert_eq!(
            metadata
                .get("IFD0:TileByteCounts")
                .and_then(TagValue::as_string),
            Some("1 1 1 1 1 1 1 1 1 1")
        );
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

        // Read next IFD offset. This MUST be the on-disk entry count, not
        // `tags.len()`: parse_ifd silently drops malformed entries, so the
        // returned vector can be shorter than what is actually on disk. Using
        // its length here walks too few entries into the file and reads
        // whatever bytes happen to sit at that wrong location -- typically
        // the tail of a skipped entry -- as the next-IFD pointer, corrupting
        // the rest of the chain instead of correctly reaching offset 0 (or
        // whatever ExifTool itself would read there).
        let Some(entry_count) = ifd_entry_count(reader, ifd_offset, byte_order) else {
            break;
        };
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

    // ExifTool reports the number of linked top-level image directories as
    // File:PageCount. Sub-IFDs (Exif/GPS/SubIFDs) are not pages and do not
    // participate in this count.
    if ifd_index > 1 {
        metadata.insert("File:PageCount", TagValue::new_integer(ifd_index as i64));
    }

    Ok(())
}

#[cfg(test)]
mod parse_ifd_chain_malformed_entry_tests {
    use super::*;
    use crate::test_support::TestReader;

    /// A minimal little-endian TIFF: `II`, magic 42, IFD0 at offset 8 with 2
    /// entries -- `Make` (ASCII, inline, well-formed) and `ExposureTime`
    /// (RATIONAL, so its 8-byte value is stored via an offset field, which
    /// this test deliberately sets far past EOF) -- followed by a
    /// terminating next-IFD offset of 0.
    ///
    /// `exiftool -a -G1 -s` on the equivalent real file reports `Make: A`
    /// and warns about the bad ExposureTime entry, but still terminates the
    /// chain correctly; it does not abandon IFD0.
    fn malformed_second_entry_tiff() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at offset 8

        data.extend_from_slice(&2u16.to_le_bytes()); // 2 entries

        // Entry 1: Make (0x010F), ASCII, count 1, inline "A\0\0\0"
        data.extend_from_slice(&0x010Fu16.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(b"A\0\0\0");

        // Entry 2: ExposureTime (0x829A), RATIONAL (8 bytes -> offset-based),
        // count 1, offset pointing far past the 38-byte file.
        data.extend_from_slice(&0x829Au16.to_le_bytes());
        data.extend_from_slice(&5u16.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&999_999_999u32.to_le_bytes());

        data.extend_from_slice(&0u32.to_le_bytes()); // next IFD = 0
        data
    }

    /// Regression test for the bug this module's `parse_ifd_chain` used to
    /// have: it located the next-IFD pointer at
    /// `ifd_offset + 2 + tags.len() * 12`, where `tags.len()` is the
    /// *returned, post-skip* entry count. `parse_ifd` drops individual
    /// malformed entries (Exif.pm:6660's "Bad offset" -> `$bad=1` ->
    /// skipped), so once any entry is dropped, `tags.len()` undercounts the
    /// true on-disk entry count and the pointer lookup lands inside the
    /// *previous* entry's own bytes instead of the real next-IFD field.
    ///
    /// Here that means the lookup used to happen at offset 22 -- the start
    /// of the (skipped) ExposureTime entry -- misreading its own
    /// `tag_id`+`field_type` bytes (`0x829A`, `0x0005`) as the next-IFD
    /// offset (0x0005829A), which is nowhere near the 38-byte file. The old
    /// code surfaced this as a top-level parse failure -- "IFD offset beyond
    /// file size" at a garbage offset -- discarding the `Make` tag it had
    /// already extracted, where real ExifTool (and the fixed code) reports
    /// `Make: A` and a bad-entry warning, then correctly terminates the
    /// chain at the real (zero) next-IFD pointer.
    #[test]
    fn a_skipped_entry_does_not_desync_the_next_ifd_pointer_lookup() {
        let reader = TestReader::new(malformed_second_entry_tiff());
        let mut metadata = MetadataMap::new();

        parse_ifd_chain(&reader, 8, ByteOrder::LittleEndian, &mut metadata)
            .expect("a single bad entry must not fail the whole chain");

        assert_eq!(metadata.get_string("IFD0:Make"), Some("A"));
        // The chain must terminate normally (next-IFD == 0), not report a
        // second page from a misread pointer.
        assert_eq!(metadata.get_integer("File:PageCount"), None);
    }
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
        // Exif.pm 0x140 declares `Format => 'binary', Binary => 1`, so the
        // original byte sequence is the value even when the TIFF field type
        // says SHORT.  Gate on the resolved name because 0x0140 is reused by
        // MakerNote tables whose values are not ColorMap payloads.
        let tag_value = if tag_name.rsplit(':').next() == Some("ColorMap") {
            TagValue::Binary(bytes.to_vec())
        } else if tag_name.rsplit(':').next() == Some("CompositeImageExposureTimes") {
            // Exif.pm 0xa462 has no static byte layout for `find_table` to
            // carry: its RawConv (Exif.pm:3079-3095) is a Perl closure that
            // switches field type mid-buffer -- seven rational64u fields,
            // then two int16u counts, then more rational64u fields -- and
            // its PrintConv (Exif.pm:3106-3116) runs each exposure-time
            // field through PrintExposureTime. Both steps need this IFD's
            // byte order, which `format_tag_value` (exiftool_compat.rs)
            // never receives, so they run fused, here, producing the final
            // display string directly instead of leaving an opaque
            // `TagValue::Binary` for a later stage that cannot decode it.
            TagValue::String(format_composite_image_exposure_times(bytes, byte_order))
        } else {
            raw_bytes_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order)
        };
        // Exif.pm 0x117's generic StripByteCounts branch deliberately turns a
        // long printable list into a scalar reference:
        //
        //     ValueConv => 'length($val) > 32 ? \\$val : $val'
        //
        // ExifTool renders that referenced string through its binary-data
        // path.  Gate on the resolved name rather than tag id alone: 0x0117
        // is reused by MakerNote tables and those tags do not inherit this
        // TIFF-table conversion.
        let tag_value = match (tag_name.rsplit(':').next(), tag_value) {
            (Some("StripByteCounts"), TagValue::String(value)) if value.len() > 32 => {
                TagValue::Binary(value.into_bytes())
            }
            (_, value) => value,
        };
        let tag_value = apply_tile_offsets_value_conv(*tag_id, tag_value);
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

#[cfg(test)]
mod strip_byte_counts_tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn long_strip_byte_counts_use_exiftools_binary_placeholder_value() {
        // Exif.pm 0x117 applies
        // `ValueConv => 'length($val) > 32 ? \\$val : $val'`.  Fifty four-digit
        // counts become a 249-byte space-separated value, so ExifTool exposes
        // the referenced string as binary data instead of printing the list.
        let raw = (0..50)
            .flat_map(|_| 1961u32.to_le_bytes())
            .collect::<Vec<_>>();
        let tags = vec![(0x0117, 4, 50, Cow::Owned(raw))];
        let mut metadata = MetadataMap::new();

        process_tiff_ifd_tags(&tags, "IFD0", ByteOrder::LittleEndian, &mut metadata);

        let expected = std::iter::repeat_n("1961", 50)
            .collect::<Vec<_>>()
            .join(" ")
            .into_bytes();
        assert_eq!(expected.len(), 249);
        assert_eq!(
            metadata.get("IFD0:StripByteCounts"),
            Some(&TagValue::Binary(expected))
        );
    }
}

#[cfg(test)]
mod color_map_tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn color_map_preserves_the_original_short_bytes_as_binary() {
        // Exif.pm 0x140 declares `Format => 'binary', Binary => 1`.  Pinned
        // ExifTool 13.59 therefore reports this six-SHORT payload as 12 bytes
        // of binary data and `-b` returns these bytes verbatim, rather than a
        // decoded space-separated list of the six integers.
        let raw = vec![1, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0];
        let tags = vec![(0x0140, 3, 6, Cow::Borrowed(raw.as_slice()))];
        let mut metadata = MetadataMap::new();

        process_tiff_ifd_tags(&tags, "IFD0", ByteOrder::LittleEndian, &mut metadata);

        assert_eq!(metadata.get("IFD0:ColorMap"), Some(&TagValue::Binary(raw)));
    }
}

#[cfg(test)]
mod document_name_tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn document_name_preserves_trailing_whitespace() {
        // ExifTool 13.59 Exif.pm 0x10d has no RawConv or PrintConv.  In
        // particular, the trailing ASCII space is data: the pinned oracle
        // prints "Plan Scan " (the NUL terminator itself is not exposed).
        let tags = vec![(0x010d, 2, 11, Cow::Borrowed(b"Plan Scan \0".as_slice()))];
        let mut metadata = MetadataMap::new();

        process_tiff_ifd_tags(&tags, "IFD0", ByteOrder::LittleEndian, &mut metadata);

        assert_eq!(
            metadata
                .get("IFD0:DocumentName")
                .and_then(TagValue::as_string),
            Some("Plan Scan ")
        );
    }
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
        // An EXIF IFD may declare 0x927C more than once -- an editor that
        // appends its own private block leaves the camera's in place, and
        // ExifTool processes each entry in turn. Keeping only the last one
        // meant `Apple_iPhone6.jpg`, whose second 0x927C is a UTF-16 JSON blob
        // written by an editing app, reached the Apple parser with the wrong
        // 142 bytes and reported nothing at all.
        let mut exif_makernote_data: Vec<&[u8]> = Vec::new();
        let mut interop_ifd_offset: Option<u64> = None;

        // ExifTool's MakerNote Condition list reads `$$self{Make}` and
        // `$$self{Model}`, DataMembers set while walking IFD0 (before this
        // sub-IFD) with trailing whitespace stripped (Exif.pm:585,595).
        let make = trimmed_data_member(metadata, "IFD0:Make");
        let model = trimmed_data_member(metadata, "IFD0:Model");

        // First pass: convert tags and capture special pointers
        for (tag_id, field_type, value_count, raw_bytes) in &exif_tags {
            // Convert Cow<[u8]> to &[u8] for processing
            let bytes = raw_bytes.as_ref();

            // Check for MakerNote in EXIF IFD (tag 0x927C)
            if *tag_id == MAKERNOTE {
                exif_makernote_data.push(bytes);
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

            let resolved_name = lookup_tag_name(*tag_id, "ExifIFD");
            let (tag_name, special_value) = if *tag_id == MAKERNOTE {
                special_makernote_value(&resolved_name, bytes, &make, &model)
                    .map_or((resolved_name, None), |(name, value)| (name, Some(value)))
            } else {
                (resolved_name, None)
            };
            let base_name = tag_name
                .split_once(':')
                .map_or(tag_name.as_str(), |(_, name)| name);

            // ExifTool's default duplicate-suppressed view gives IFD0
            // priority when the same tag name also appears in the EXIF
            // sub-IFD. Real files do this with Compression, Padding, and
            // OffsetSchema; keeping both raw keys makes the family-normalized
            // output nondeterministically clobber one value with the other.
            // Match the precedence already applied to IFD1 and InteropIFD.
            if metadata.get(&format!("IFD0:{base_name}")).is_some() {
                continue;
            }

            // Exif.pm 0xa462 (CompositeImageExposureTimes) has no static byte
            // layout for `find_table` to carry: its RawConv (Exif.pm:3079-3095)
            // is a Perl closure that switches field type mid-buffer -- seven
            // rational64u fields, then two int16u counts, then more
            // rational64u fields -- and its PrintConv (Exif.pm:3106-3116) runs
            // each exposure-time field through PrintExposureTime. Both steps
            // need this IFD's byte order, which `format_tag_value`
            // (exiftool_compat.rs) never receives, so they run fused, here,
            // producing the final display string directly instead of leaving
            // an opaque `TagValue::Binary` for a later stage that cannot
            // decode it.
            let tag_value = if let Some(value) = special_value {
                value
            } else if base_name == "CompositeImageExposureTimes" {
                TagValue::String(format_composite_image_exposure_times(bytes, byte_order))
            } else {
                raw_bytes_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order)
            };
            metadata.insert(tag_name, tag_value);
        }

        // Second pass: parse the MakerNote found in the EXIF IFD. The decoder
        // is given the enclosing TIFF block as well as the payload, because a
        // MakerNote's value offsets are measured from the TIFF header and
        // routinely address bytes past the payload's declared end.
        for makernote_bytes in exif_makernote_data {
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

    // OtherImage is the bytes the pair points at - the same byte-range handling
    // `parse_ifd1_thumbnail` applies to ThumbnailImage, including the
    // placeholder fallback ExifTool prints for an unreadable range (see
    // `read_or_placeholder`). No corpus file currently exercises the fallback
    // here; it is shared so the two paths cannot diverge.
    if length == 0 || length > MAX_THUMBNAIL_BYTES {
        return;
    }
    metadata.insert(
        "InteropIFD:OtherImage",
        read_or_placeholder(reader, image_offset, length),
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
            let tag_value = if matches!(tag_id, 0x001B | 0x001C)
                && let TagValue::Binary(bytes) = &tag_value
            {
                let decoded = crate::core::formatters::decode_gps_processing_method(bytes);
                if decoded.is_empty() {
                    tag_value
                } else {
                    TagValue::new_string(decoded)
                }
            } else {
                tag_value
            };
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

fn contextual_tag_name(resolved: &str, base_name: &str) -> String {
    resolved.split_once(':').map_or_else(
        || base_name.to_string(),
        |(group, _)| format!("{group}:{base_name}"),
    )
}

/// Reads a string tag the way ExifTool keeps its `Make`/`Model` DataMembers:
/// trailing whitespace stripped (`RawConv => '$val =~ s/\s+$//; ...'`,
/// Exif.pm:585,595). Trailing NULs are also dropped defensively; an absent
/// tag reads as the empty string, which fails every `eq`/prefix test below
/// exactly as Perl's `undef` fails them.
fn trimmed_data_member(metadata: &MetadataMap, key: &str) -> String {
    metadata.get_string(key).map_or_else(String::new, |value| {
        value
            .trim_end_matches(['\0', ' ', '\t', '\n', '\r', '\x0b', '\x0c'])
            .to_string()
    })
}

/// Does `val` start with any of `prefixes`?
fn any_prefix(val: &[u8], prefixes: &[&[u8]]) -> bool {
    prefixes.iter().any(|prefix| val.starts_with(prefix))
}

/// ASCII-case-insensitive `starts_with` over a string (Perl `=~ /^.../i`).
fn ci_starts_with(text: &str, prefix: &str) -> bool {
    let text = text.as_bytes();
    let prefix = prefix.as_bytes();
    text.len() >= prefix.len() && text[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// ASCII-case-insensitive `starts_with` over value bytes.
fn ci_val_prefix(val: &[u8], prefix: &[u8]) -> bool {
    val.len() >= prefix.len() && val[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// `MakerNoteKodak7`'s serial-number shape,
/// `/^[CK][A-Z\d]{3} ?[A-Z\d]{1,2}\d{2}[A-Z\d]\d{4}[ \0]/` (MakerNotes.pm).
/// The optional space and the one-or-two alphanumerics are tried in every
/// combination, as the regex engine's backtracking would.
fn kodak7_serial(val: &[u8]) -> bool {
    fn alnum(byte: u8) -> bool {
        byte.is_ascii_uppercase() || byte.is_ascii_digit()
    }
    if val.len() < 12
        || !(val[0] == b'C' || val[0] == b'K')
        || !val[1..4].iter().copied().all(alnum)
    {
        return false;
    }
    for with_space in [true, false] {
        let start = if with_space {
            if val.get(4) != Some(&b' ') {
                continue;
            }
            5
        } else {
            4
        };
        for id_len in [2usize, 1] {
            let Some(id) = val.get(start..start + id_len) else {
                continue;
            };
            if !id.iter().copied().all(alnum) {
                continue;
            }
            let Some(rest) = val.get(start + id_len..start + id_len + 8) else {
                continue;
            };
            if rest[0].is_ascii_digit()
                && rest[1].is_ascii_digit()
                && alnum(rest[2])
                && rest[3..7].iter().all(u8::is_ascii_digit)
                && (rest[7] == b' ' || rest[7] == 0)
            {
                return true;
            }
        }
    }
    false
}

/// True when ExifTool's `@MakerNotes::Main` (pinned 13.59) resolves the note
/// to an entry *before* `MakerNoteSamsung1a`: everything from `MakerNoteApple`
/// (MakerNotes.pm:38) through `MakerNoteRicohText` (:942). `GetTagInfo` walks
/// the list in order and takes the first entry whose `Condition` holds, so
/// the value-typed entries this module emits are reachable only when every
/// one of these fails. Conditions are transcribed verbatim; entries whose
/// union is order-independent (a bare Make catch-all following narrower
/// tests of the same Make) are collapsed, which cannot change the OR.
///
/// `val` must be the condition prefix - the first `min(size, 128)` bytes of
/// the value (Exif.pm:6717) - and `make`/`model` the trimmed DataMembers.
#[allow(clippy::too_many_lines)]
fn claimed_before_samsung1a(make: &str, model: &str, val: &[u8]) -> bool {
    // MakerNoteApple: $$valPt =~ /^Apple iOS\0/
    if val.starts_with(b"Apple iOS\0") {
        return true;
    }
    // MakerNoteNikon: $$valPt=~/^Nikon\x00\x02/; MakerNoteNikon2: /^Nikon\x00\x01/
    if val.starts_with(b"Nikon\x00\x02") || val.starts_with(b"Nikon\x00\x01") {
        return true;
    }
    // MakerNoteCanon: $$self{Make} =~ /^Canon/
    if make.starts_with("Canon") {
        return true;
    }
    // MakerNoteCasio ($$self{Make}=~/^CASIO/ and $$valPt!~/^(QVC|DCI)\0/) and
    // MakerNoteCasio2 ($$valPt =~ /^(QVC|DCI)\0/) jointly claim either way.
    if make.starts_with("CASIO") || val.starts_with(b"QVC\0") || val.starts_with(b"DCI\0") {
        return true;
    }
    // MakerNoteDJIInfo: $$valPt =~ /^\[ae_dbg_info:/
    if val.starts_with(b"[ae_dbg_info:") {
        return true;
    }
    // MakerNoteDJI: $$self{Make} eq "DJI" and $$valPt !~ /^(...\@AMBA|DJI)/s
    if make == "DJI" && !(val.len() >= 8 && &val[3..8] == b"@AMBA") && !val.starts_with(b"DJI") {
        return true;
    }
    // MakerNoteFLIR: $$self{Make} =~ /^(FLIR Systems|Teledyne FLIR)/
    if make.starts_with("FLIR Systems") || make.starts_with("Teledyne FLIR") {
        return true;
    }
    // MakerNoteFujiFilm: $$valPt =~ /^(FUJIFILM|GENERALE)/
    if val.starts_with(b"FUJIFILM") || val.starts_with(b"GENERALE") {
        return true;
    }
    // MakerNoteGE: $$valPt =~ /^GE(\0\0|NIC\0)/; MakerNoteGE2: /^GE\x0c\0\0\0\x16\0\0\0/
    if any_prefix(val, &[b"GE\0\0", b"GENIC\0", b"GE\x0c\0\0\0\x16\0\0\0"]) {
        return true;
    }
    // MakerNoteGoogle: $$valPt =~ /^HDRP[\x02\x03]/
    if val.starts_with(b"HDRP\x02") || val.starts_with(b"HDRP\x03") {
        return true;
    }
    // MakerNoteHasselblad: $$self{Make} eq "Hasselblad"
    if make == "Hasselblad" {
        return true;
    }
    // MakerNoteHP: $$valPt =~ /^(Hewlett-Packard|Vivitar)/
    if val.starts_with(b"Hewlett-Packard") || val.starts_with(b"Vivitar") {
        return true;
    }
    // MakerNoteHP2: $$valPt =~ /^610[\0-\4]/
    if val.len() >= 4 && val.starts_with(b"610") && val[3] <= 0x04 {
        return true;
    }
    // MakerNoteHP4: $$valPt =~ /^IIII[\x04|\x05]\0/ (the class holds a literal '|')
    if val.len() >= 6
        && val.starts_with(b"IIII")
        && matches!(val[4], 0x04 | 0x05 | b'|')
        && val[5] == 0
    {
        return true;
    }
    // MakerNoteHP6: $$valPt =~ /^IIII\x06\0/
    if val.starts_with(b"IIII\x06\0") {
        return true;
    }
    // MakerNoteISL: $$valPt =~ /^ISLMAKERNOTE000\0/
    if val.starts_with(b"ISLMAKERNOTE000\0") {
        return true;
    }
    // MakerNoteJVC: $$valPt=~/^JVC /
    if val.starts_with(b"JVC ") {
        return true;
    }
    // MakerNoteJVCText: $$self{Make}=~/^(JVC|Victor)/ and $$valPt=~/^VER:/
    if (make.starts_with("JVC") || make.starts_with("Victor")) && val.starts_with(b"VER:") {
        return true;
    }
    // MakerNoteKodak1a (/^KDK INFO/) and MakerNoteKodak1b (/^KDK/), both
    // gated on $$self{Make}=~/^EASTMAN KODAK/: the 1b prefix covers 1a.
    if make.starts_with("EASTMAN KODAK") && val.starts_with(b"KDK") {
        return true;
    }
    // MakerNoteKodak2: $$valPt =~ /^.{8}Eastman Kodak/s or
    //                  $$valPt =~ /^\x01\0[\0\x01]\0\0\0\x04\0[a-zA-Z]{4}/
    if val.len() >= 21 && &val[8..21] == b"Eastman Kodak" {
        return true;
    }
    if val.len() >= 12
        && val[0] == 0x01
        && val[1] == 0
        && (val[2] == 0 || val[2] == 0x01)
        && val[3] == 0
        && val[4] == 0
        && val[5] == 0
        && val[6] == 0x04
        && val[7] == 0
        && val[8..12].iter().all(u8::is_ascii_alphabetic)
    {
        return true;
    }
    let mm_ii_aoc = val.starts_with(b"MM") || val.starts_with(b"II") || val.starts_with(b"AOC");
    // MakerNoteKodak3: /^EASTMAN KODAK/, $$valPt =~ /^(?!MM|II).{12}\x07/s
    // and !~ /^(MM|II|AOC)/ (the lookahead is subsumed by the negative)
    if make.starts_with("EASTMAN KODAK") && val.len() >= 13 && val[12] == 0x07 && !mm_ii_aoc {
        return true;
    }
    // MakerNoteKodak4: /^Eastman Kodak/, $$valPt =~ /^.{41}JPG/s, !^(MM|II|AOC)
    if make.starts_with("Eastman Kodak") && val.len() >= 44 && &val[41..44] == b"JPG" && !mm_ii_aoc
    {
        return true;
    }
    // MakerNoteKodak5: /^EASTMAN KODAK/ and (Model CX-list or the byte probe)
    if make.starts_with("EASTMAN KODAK")
        && (["CX4200", "CX4230", "CX4300", "CX4310", "CX6200", "CX6230"]
            .iter()
            .any(|cx| model.contains(cx))
            || (val.len() >= 4
                && val[0] == 0
                && matches!(
                    (val[1], val[2]),
                    (0x1a, 0x18) | (0x3a, 0x08) | (0x59, 0xf8) | (0x14, 0x80)
                )
                && val[3] == 0))
    {
        return true;
    }
    // MakerNoteKodak6a (Model DX3215) / MakerNoteKodak6b (Model DX3700)
    if make.starts_with("EASTMAN KODAK") && (model.contains("DX3215") || model.contains("DX3700")) {
        return true;
    }
    let kodak_make = make.to_ascii_lowercase().contains("kodak");
    // MakerNoteKodak7: /Kodak/i and the serial-number probe
    if kodak_make && kodak7_serial(val) {
        return true;
    }
    // MakerNoteKodak8a: /Kodak/i and either IFD-entry probe
    if kodak_make
        && ((val.len() >= 8
            && val[0] == 0
            && (0x02..=0x7f).contains(&val[1])
            && val[4] == 0
            && (0x01..=0x0c).contains(&val[5])
            && val[6] == 0
            && val[7] == 0)
            || (val.len() >= 10
                && (0x02..=0x7f).contains(&val[0])
                && val[1] == 0
                && (0x01..=0x0c).contains(&val[4])
                && val[5] == 0
                && val[8] == 0
                && val[9] == 0))
    {
        return true;
    }
    // MakerNoteKodak8b: /Kodak/i and /^MM\0\x2a\0\0\0\x08\0.\0\0/ (no /s:
    // the wildcard byte may not be "\n")
    if kodak_make
        && val.len() >= 12
        && val.starts_with(b"MM\0\x2a\0\0\0\x08\0")
        && val[9] != b'\n'
        && val[10] == 0
        && val[11] == 0
    {
        return true;
    }
    // MakerNoteKodak8c: /Kodak/i and /^(MM\0\x2a\0\0\0\x08|II\x2a\0\x08\0\0\0)/
    if kodak_make
        && (val.starts_with(b"MM\0\x2a\0\0\0\x08") || val.starts_with(b"II\x2a\0\x08\0\0\0"))
    {
        return true;
    }
    // MakerNoteKodak9: m{^IIII[\x02\x03]\0.{14}\d{4}/\d{2}/\d{2} }s
    if val.len() >= 31
        && val.starts_with(b"IIII")
        && matches!(val[4], 0x02 | 0x03)
        && val[5] == 0
        && val[20..24].iter().all(u8::is_ascii_digit)
        && val[24] == b'/'
        && val[25..27].iter().all(u8::is_ascii_digit)
        && val[27] == b'/'
        && val[28..30].iter().all(u8::is_ascii_digit)
        && val[30] == b' '
    {
        return true;
    }
    // MakerNoteKodak10: /Kodak/i and /^(MM\0[\x02-\x7f]|II[\x02-\x7f]\0)/
    if kodak_make
        && val.len() >= 4
        && ((val.starts_with(b"MM\0") && (0x02..=0x7f).contains(&val[3]))
            || (val.starts_with(b"II") && (0x02..=0x7f).contains(&val[2]) && val[3] == 0))
    {
        return true;
    }
    // MakerNoteKodak11 and MakerNoteKodak12 key on Model =~ /(Kodak|PixPro)/i
    let kodak_model = {
        let lower = model.to_ascii_lowercase();
        lower.contains("kodak") || lower.contains("pixpro")
    };
    if kodak_model
        && val.len() >= 12
        && ((val.starts_with(b"II\x2a\0\x08\0\0\0") && val[9] == 0 && val[10] == 0 && val[11] == 0)
            || (val.starts_with(b"MM\0\x2a\0\0\0\x08")
                && val[8] == 0
                && val[9] == 0
                && val[10] == 0))
    {
        return true;
    }
    // MakerNoteKodakUnknown: $$self{Make}=~/Kodak/i and $$valPt!~/^AOC\0/
    if kodak_make && !val.starts_with(b"AOC\0") {
        return true;
    }
    // MakerNoteKyocera: $$valPt =~ /^KYOCERA/
    if val.starts_with(b"KYOCERA") {
        return true;
    }
    // MakerNoteMinolta (Make and !^(MINOL|CAMER|MLY0|KC|\+M\+M|\xd7)) plus the
    // MakerNoteMinolta3 catch-all on the same /^(Konica Minolta|Minolta)/i.
    if ci_starts_with(make, "Konica Minolta") || ci_starts_with(make, "Minolta") {
        return true;
    }
    // MakerNoteMinolta2: $$valPt =~ /^(MINOL|CAMER)\0/
    if val.starts_with(b"MINOL\0") || val.starts_with(b"CAMER\0") {
        return true;
    }
    // MakerNoteMotorola: $$valPt=~/^MOT\0/
    if val.starts_with(b"MOT\0") {
        return true;
    }
    // MakerNoteNikon3: $$self{Make}=~/^NIKON/i
    if ci_starts_with(make, "NIKON") {
        return true;
    }
    // MakerNoteNintendo: $$self{Make} eq "Nintendo"
    if make == "Nintendo" {
        return true;
    }
    // MakerNoteOlympus (/^(OLYMP|EPSON)\0/), MakerNoteOlympus2 (/^OLYMPUS\0/),
    // MakerNoteOlympus3 (/^OM SYSTEM\0/)
    if any_prefix(val, &[b"OLYMP\0", b"EPSON\0", b"OLYMPUS\0", b"OM SYSTEM\0"]) {
        return true;
    }
    // MakerNoteLeica: $$self{Make} eq "LEICA"
    if make == "LEICA" {
        return true;
    }
    let leica_ag = make.starts_with("Leica Camera AG");
    // MakerNoteLeica2: Make and $$valPt =~ /^LEICA\0\0\0/
    if leica_ag && val.starts_with(b"LEICA\0\0\0") {
        return true;
    }
    // MakerNoteLeica3: Make, $$valPt !~ /^LEICA/, Model ne S2 / M (Typ 240)
    if leica_ag && !val.starts_with(b"LEICA") && model != "S2" && model != "LEICA M (Typ 240)" {
        return true;
    }
    // MakerNoteLeica4: Make and $$valPt =~ /^LEICA0/ (a literal '0': the
    // M9/M-Monochrom header is "LEICA0\x03\0")
    if leica_ag && val.starts_with(b"LEICA0") {
        return true;
    }
    // MakerNoteLeica5: $$valPt =~ /^LEICA\0[\x01\x04\x05\x06\x07\x10\x1a]\0/
    if val.len() >= 8
        && val.starts_with(b"LEICA\0")
        && matches!(val[6], 0x01 | 0x04..=0x07 | 0x10 | 0x1a)
        && val[7] == 0
    {
        return true;
    }
    // MakerNoteLeica6: Make eq 'Leica Camera AG' and the three Model names
    if make == "Leica Camera AG"
        && matches!(model, "S2" | "LEICA M (Typ 240)" | "LEICA S (Typ 006)")
    {
        return true;
    }
    // MakerNoteLeica7: $$valPt =~ /^LEICA\0\x02\xff/
    if val.starts_with(b"LEICA\0\x02\xff") {
        return true;
    }
    // MakerNoteLeica8: $$valPt =~ /^LEICA\0[\x08\x09\x0a]\0/
    if val.len() >= 8 && val.starts_with(b"LEICA\0") && matches!(val[6], 0x08..=0x0a) && val[7] == 0
    {
        return true;
    }
    // MakerNoteLeica9: Make and $$valPt =~ /^LEICA\0\x02\0/
    if leica_ag && val.starts_with(b"LEICA\0\x02\0") {
        return true;
    }
    // MakerNoteLeica10: $$valPt =~ /^LEICA CAMERA AG\0/
    if val.starts_with(b"LEICA CAMERA AG\0") {
        return true;
    }
    // MakerNotePanasonic (Model ne "DC-FT7") and MakerNotePanasonic3 (no
    // Model test) claim every /^Panasonic/ value between them.
    if val.starts_with(b"Panasonic") {
        return true;
    }
    // MakerNotePanasonic2: $$self{Make}=~/^Panasonic/ and $$valPt=~/^MKE/
    if make.starts_with("Panasonic") && val.starts_with(b"MKE") {
        return true;
    }
    // MakerNotePentax: /^AOC\0/ and Model !~ /^PENTAX Optio ?[34]30RS\s*$/
    // (trailing whitespace is already stripped from the member)
    if val.starts_with(b"AOC\0")
        && !matches!(
            model,
            "PENTAX Optio 330RS" | "PENTAX Optio330RS" | "PENTAX Optio 430RS" | "PENTAX Optio430RS"
        )
    {
        return true;
    }
    // MakerNotePentax2 (/^Asahi/ and !^AOC\0) plus the MakerNotePentax3
    // catch-all on the same /^Asahi/.
    if make.starts_with("Asahi") {
        return true;
    }
    // MakerNotePentax4: $$self{Make}=~/^PENTAX/ and $$valPt=~/^\d{3}/
    if make.starts_with("PENTAX") && val.len() >= 3 && val[..3].iter().all(u8::is_ascii_digit) {
        return true;
    }
    // MakerNotePentax5: $$valPt=~/^PENTAX \0/
    if val.starts_with(b"PENTAX \0") {
        return true;
    }
    // MakerNotePentax6: $$valPt=~/^S1\0{6}\x0c\0{3}/
    if val.starts_with(b"S1\0\0\0\0\0\0\x0c\0\0\0") {
        return true;
    }
    // MakerNotePhaseOne: $$valPt =~ /^(IIII.waR|MMMMRaw.)/s
    if val.len() >= 8
        && ((val.starts_with(b"IIII") && &val[5..8] == b"waR") || val.starts_with(b"MMMMRaw"))
    {
        return true;
    }
    // MakerNoteReconyxHyperFire: $$valPt =~ /^\x01\xf1([\x02\x03]\x00)?/ and
    // ($1 or $$self{Make} eq "RECONYX")
    if val.starts_with(b"\x01\xf1\x02\x00")
        || val.starts_with(b"\x01\xf1\x03\x00")
        || (val.starts_with(b"\x01\xf1") && make == "RECONYX")
    {
        return true;
    }
    // MakerNoteReconyxUltraFire / HyperFire2 / MicroFire / HyperFire4K
    if any_prefix(
        val,
        &[
            b"RECONYXUF\0",
            b"RECONYXH2\0",
            b"RECONYXMF\0",
            b"RECONYXHF4K\0",
        ],
    ) {
        return true;
    }
    // MakerNoteRicohPentax: $$valPt=~/^RICOH\0(II|MM)/
    if val.starts_with(b"RICOH\0II") || val.starts_with(b"RICOH\0MM") {
        return true;
    }
    // MakerNoteRicohText's bare /^RICOH/ catch-all claims every remaining
    // RICOH-made note; a "PENTAX RICOH" Make reaches only MakerNoteRicoh and
    // MakerNoteRicoh2, whose conditions are tested verbatim below.
    if make.starts_with("RICOH") {
        return true;
    }
    if make.starts_with("PENTAX RICOH") {
        // The /s-mode probe shared by MakerNoteRicoh (negated) and
        // MakerNoteRicoh2: /^(MM\0\x2a\0\0\0\x08\0.\0\0|II\x2a\0\x08\0\0\0.\0\0\0)/s
        let ricoh2_probe = (val.len() >= 12
            && val.starts_with(b"MM\0\x2a\0\0\0\x08\0")
            && val[10] == 0
            && val[11] == 0)
            || (val.len() >= 12
                && val.starts_with(b"II\x2a\0\x08\0\0\0")
                && val[9] == 0
                && val[10] == 0
                && val[11] == 0);
        // MakerNoteRicoh: /^(Ricoh|      |MM\0\x2a|II\x2a\0)/i, not the
        // probe, Model ne 'RICOH WG-M1'
        if (ci_val_prefix(val, b"Ricoh")
            || val.starts_with(b"      ")
            || ci_val_prefix(val, b"MM\0\x2a")
            || ci_val_prefix(val, b"II\x2a\0"))
            && !ricoh2_probe
            && model != "RICOH WG-M1"
        {
            return true;
        }
        // MakerNoteRicoh2: Model eq 'RICOH WG-M1' or the probe
        if model == "RICOH WG-M1" || ricoh2_probe {
            return true;
        }
    }
    false
}

/// True when an entry *between* `MakerNoteSamsung1b` and the
/// `MakerNoteUnknown*` fallbacks claims the note (MakerNotes.pm:966-1101:
/// Samsung2, the Sanyo and Sony families, Sigma). `MakerNoteSamsung1b`
/// itself is handled by the caller's STMN branch.
fn claimed_between_samsung1b_and_unknown(make: &str, model: &str, val: &[u8]) -> bool {
    // MakerNoteSamsung2: uc $$self{Make} eq 'SAMSUNG' and ($$self{TIFF_TYPE}
    // eq 'SRW' or $$valPt=~/^(\0.\0\x01\0\x07\0{3}\x04|.\0\x01\0\x07\0\x04\0{3})0100/s).
    // The TIFF_TYPE arm is unavailable here (no container context reaches
    // this helper), but an SRW's EXIF-format maker note is a binary IFD that
    // can never satisfy the text/LSI1 fallbacks below, so the byte probe
    // alone is exact for every value those fallbacks could otherwise take.
    if make.eq_ignore_ascii_case("SAMSUNG")
        && val.len() >= 14
        && &val[10..14] == b"0100"
        && ((val[0] == 0
            && val[2] == 0
            && val[3] == 0x01
            && val[4] == 0
            && val[5] == 0x07
            && val[6] == 0
            && val[7] == 0
            && val[8] == 0
            && val[9] == 0x04)
            || (val[1] == 0
                && val[2] == 0x01
                && val[3] == 0
                && val[4] == 0x07
                && val[5] == 0
                && val[6] == 0x04
                && val[7] == 0
                && val[8] == 0
                && val[9] == 0))
    {
        return true;
    }
    // MakerNoteSanyo / SanyoC4 / SanyoPatch: SanyoPatch is a bare
    // $$self{Make}=~/^SANYO/ catch-all, so the Make alone decides.
    if make.starts_with("SANYO") {
        return true;
    }
    // MakerNoteSigma: $$self{Make}=~/^(SIGMA|FOVEON)/i
    if ci_starts_with(make, "SIGMA") || ci_starts_with(make, "FOVEON") {
        return true;
    }
    // MakerNoteSony: /^(SONY (DSC|CAM|MOBILE)|\0\0SONY PIC\0|VHAB     \0)/,
    // MakerNoteSony2 (/^SONY PI\0/), MakerNoteSony3 (/^(PREMI)\0/),
    // MakerNoteSony4 (/^SONY PIC\0/)
    if any_prefix(
        val,
        &[
            b"SONY DSC",
            b"SONY CAM",
            b"SONY MOBILE",
            b"\0\0SONY PIC\0",
            b"VHAB     \0",
            b"SONY PI\0",
            b"PREMI\0",
            b"SONY PIC\0",
        ],
    ) {
        return true;
    }
    // MakerNoteSony5 plus the MakerNoteSonySRF catch-all: /^SONY/ claims
    // regardless, and Sony5's Hasselblad-rebadge arm adds
    // (Make ^HASSELBLAD, Model ^(HV|Stellar|Lusso|Lunar), val !^\x01\x00).
    if make.starts_with("SONY") {
        return true;
    }
    if make.starts_with("HASSELBLAD")
        && (model.starts_with("HV")
            || model.starts_with("Stellar")
            || model.starts_with("Lusso")
            || model.starts_with("Lunar"))
        && !val.starts_with(b"\x01\x00")
    {
        return true;
    }
    // MakerNoteSonyEricsson: $$valPt =~ /^SEMC MS\0/
    if val.starts_with(b"SEMC MS\0") {
        return true;
    }
    false
}

/// `MakerNoteUnknownText`'s Condition,
/// `$$valPt =~ /^[\x09\x0d\x0a\x20-\x7e]+\0*$/` (MakerNotes.pm:1102-1108),
/// applied as Perl applies it: with no `/m`, `$` also matches just before a
/// string-final `"\n"`, so `text NULs "\n"` passes too. Nothing is
/// NUL-trimmed before the test.
fn unknown_text_condition(prefix: &[u8]) -> bool {
    fn text_then_nuls(bytes: &[u8]) -> bool {
        let text_len = bytes
            .iter()
            .take_while(|byte| matches!(**byte, b'\t' | b'\n' | b'\r' | 0x20..=0x7e))
            .count();
        text_len > 0 && bytes[text_len..].iter().all(|byte| *byte == 0)
    }
    text_then_nuls(prefix)
        || matches!(prefix.split_last(), Some((b'\n', body)) if text_then_nuls(body))
}

/// Applies ExifTool's condition-specific names to the MakerNote (0x927C)
/// values it stores as plain values rather than parsed subdirectories:
/// `MakerNoteSamsung1a`, `MakerNoteUnknownText` and `MakerNoteUnknownBinary`
/// (MakerNotes.pm, pinned 13.59).
///
/// ExifTool resolves the name by walking `@MakerNotes::Main` in order and
/// taking the first entry whose `Condition` holds; a Condition sees
/// `$$self{Make}`/`$$self{Model}` and only the first `min(size, 128)` bytes
/// of the value (Exif.pm:6717). The three names above are reachable only
/// when every maker-specific entry before them fails, so when a preceding
/// entry claims the note this returns `None` - oxidex may lack that maker's
/// parser, and a `MakerNoteUnknown*` name for a claimed note would be a
/// wrong name, not a fallback.
fn special_makernote_value(
    resolved_name: &str,
    data: &[u8],
    make: &str,
    model: &str,
) -> Option<(String, TagValue)> {
    use crate::parsers::tiff::makernotes::samsung::stmn;

    let condition_prefix = &data[..data.len().min(128)];

    if claimed_before_samsung1a(make, model, condition_prefix) {
        return None;
    }

    // MakerNoteSamsung1a (`/^STMN\d{3}.\0{4}/s`) stores the note as a bare
    // binary value; MakerNoteSamsung1b (`/^STMN\d{3}/`) is a subdirectory
    // the second pass parses, and it must not fall through to the fallbacks.
    if stmn::is_stmn(condition_prefix) {
        if stmn::is_binary_only(condition_prefix) {
            return Some((
                contextual_tag_name(resolved_name, "MakerNoteSamsung1a"),
                TagValue::new_binary(data.to_vec()),
            ));
        }
        return None;
    }

    if claimed_between_samsung1b_and_unknown(make, model, condition_prefix) {
        return None;
    }

    // MakerNoteUnknownText. The Condition sees only the 128-byte prefix and
    // nothing is NUL-trimmed anywhere: its ValueConv
    // `length($val) > 64 ? \$val : $val` measures the FULL untrimmed value,
    // so a short text note NUL-padded past 64 bytes is reported as binary
    // with the padded length.
    if unknown_text_condition(condition_prefix) {
        let name = contextual_tag_name(resolved_name, "MakerNoteUnknownText");
        if data.len() > 64 {
            return Some((name, TagValue::new_binary(data.to_vec())));
        }
        // The stored value is the untrimmed text, which the exiftool
        // application prints through its output filter (exiftool:3007-3009):
        // \x01-\x1f and \x7f become '.', NULs are deleted, trailing spaces
        // are trimmed. oxidex stores display strings, so the same filter is
        // fused here; a value this short (<= 64 bytes) was covered by the
        // condition in full, so every byte is ASCII and the mapping is total.
        let rendered: String = data
            .iter()
            .filter_map(|byte| match *byte {
                0 => None,
                0x01..=0x1f | 0x7f => Some('.'),
                printable => Some(printable as char),
            })
            .collect();
        return Some((
            name,
            TagValue::new_string(rendered.trim_end_matches(' ').to_string()),
        ));
    }

    // MakerNoteUnknownBinary: $$valPt =~ /^LSI1\0/ (SilverFast).
    if condition_prefix.starts_with(b"LSI1\0") {
        return Some((
            contextual_tag_name(resolved_name, "MakerNoteUnknownBinary"),
            TagValue::new_binary(data.to_vec()),
        ));
    }
    None
}

#[cfg(test)]
mod makernote_fallback_tests {
    use super::*;

    const RESOLVED: &str = "ExifIFD:MakerNote";

    fn fallback(data: &[u8], make: &str, model: &str) -> Option<(String, TagValue)> {
        special_makernote_value(RESOLVED, data, make, model)
    }

    /// Exif.pm:6717 hands the Condition only the first min(size, 128) bytes,
    /// so binary garbage past byte 128 cannot defeat the text match.
    #[test]
    fn text_condition_examines_only_the_first_128_bytes() {
        let mut data = vec![b'A'; 128];
        data.extend_from_slice(&[0xFF, 0x00, 0x13]);
        let (name, value) = fallback(&data, "", "").expect("prefix is pure text");
        assert_eq!(name, "ExifIFD:MakerNoteUnknownText");
        assert!(
            matches!(value, TagValue::Binary(ref bytes) if bytes.len() == 131),
            "the >64 branch must carry the FULL value"
        );
    }

    /// The `length($val) > 64` split measures the untrimmed value: 14 text
    /// bytes NUL-padded to 70 report as 70 bytes of binary, never as the
    /// trimmed string (SamsungDigimaxA4.jpg does this with 460 bytes).
    #[test]
    fn text_binary_split_measures_the_untrimmed_value() {
        let mut data = b"Unknown Format".to_vec();
        data.resize(70, 0);
        let (name, value) = fallback(&data, "SAMSUNG TECHWIN CO.", "").expect("text plus NULs");
        assert_eq!(name, "ExifIFD:MakerNoteUnknownText");
        assert_eq!(value, TagValue::new_binary(data));
    }

    /// A value at or under 64 bytes stays a string, rendered as the exiftool
    /// application renders it (NULs deleted, trailing spaces trimmed).
    #[test]
    fn short_text_value_renders_like_the_exiftool_app() {
        let mut data = b"FINE".to_vec();
        data.resize(10, 0);
        let (_, value) = fallback(&data, "Samsung", "SPH-A940").expect("short text");
        assert_eq!(value.as_string(), Some("FINE"));
    }

    /// `/^[\x09\x0d\x0a\x20-\x7e]+\0*$/` rejects text resuming after a NUL.
    #[test]
    fn nul_interrupted_text_is_not_text() {
        assert_eq!(fallback(b"AB\0CD", "", ""), None);
    }

    /// Perl's `$` (no /m) also matches just before a string-final "\n".
    #[test]
    fn trailing_newline_after_nuls_still_matches() {
        let (name, _) = fallback(b"AB\0\0\n", "", "").expect("Perl-$ newline form");
        assert_eq!(name, "ExifIFD:MakerNoteUnknownText");
    }

    /// @MakerNotes::Main entries preceding the fallbacks claim the note
    /// first: MakerNoteRicohText takes any RICOH-made note,
    /// MakerNoteJVCText takes a JVC "VER:" note, MakerNoteCanon takes
    /// everything Canon-made. No MakerNoteUnknown* may fire for them, and
    /// oxidex emits nothing in their place.
    #[test]
    fn preceding_maker_conditions_claim_the_note() {
        assert_eq!(
            fallback(b"Text note\0\0", "RICOH IMAGING COMPANY, LTD.", ""),
            None
        );
        assert_eq!(fallback(b"VER:1.0\0", "JVC", "GR-D230"), None);
        assert_eq!(fallback(b"LSI1\0abc", "Canon", "EOS"), None);
    }

    /// Makes that defeat their maker's Condition fall through to the
    /// fallbacks: "FS-Nikon" fails /^NIKON/i (NikonLS-50.jpg), and a note
    /// starting "DJI" fails MakerNoteDJI's `$$valPt !~ /^(...\@AMBA|DJI)/s`
    /// (DJI_M3T.jpg).
    #[test]
    fn non_claimed_makes_reach_the_fallbacks() {
        let (name, _) = fallback(b"LSI1\0data", "FS-Nikon", "LS-50").expect("LSI1 note");
        assert_eq!(name, "ExifIFD:MakerNoteUnknownBinary");

        let (name, value) = fallback(b"DJI MakerNotes\0\0", "DJI", "M3T").expect("DJI text note");
        assert_eq!(name, "ExifIFD:MakerNoteUnknownText");
        assert_eq!(value.as_string(), Some("DJI MakerNotes"));
    }

    /// STMN with a zeroed PreviewImageStart is MakerNoteSamsung1a; with a
    /// nonzero one it is the MakerNoteSamsung1b subdirectory, which the
    /// second pass parses - not a fallback value.
    #[test]
    fn stmn_splits_between_samsung1a_and_samsung1b() {
        let mut binary_only = b"STMN010\0".to_vec();
        binary_only.extend_from_slice(&[0, 0, 0, 0, 0xAA]);
        let (name, _) = fallback(&binary_only, "SAMSUNG", "").expect("1a note");
        assert_eq!(name, "ExifIFD:MakerNoteSamsung1a");

        let mut with_preview = b"STMN010\0".to_vec();
        with_preview.extend_from_slice(&[1, 2, 3, 4, 0xAA]);
        assert_eq!(fallback(&with_preview, "SAMSUNG", ""), None);
    }
}

fn push_u16(out: &mut Vec<u8>, value: u16, byte_order: ByteOrder) {
    let bytes = match byte_order {
        ByteOrder::LittleEndian => value.to_le_bytes(),
        ByteOrder::BigEndian => value.to_be_bytes(),
    };
    out.extend_from_slice(&bytes);
}

fn push_u32(out: &mut Vec<u8>, value: u32, byte_order: ByteOrder) {
    let bytes = match byte_order {
        ByteOrder::LittleEndian => value.to_le_bytes(),
        ByteOrder::BigEndian => value.to_be_bytes(),
    };
    out.extend_from_slice(&bytes);
}

/// Compression (0x0103) - in IFD1 this describes the thumbnail encoding.
const TAG_COMPRESSION: u16 = 0x0103;

/// JPEGInterchangeFormat (0x0201) - ExifTool names this `ThumbnailOffset` in IFD1.
const TAG_THUMBNAIL_OFFSET: u16 = 0x0201;

/// JPEGInterchangeFormatLength (0x0202) - ExifTool names this `ThumbnailLength` in IFD1.
const TAG_THUMBNAIL_LENGTH: u16 = 0x0202;

/// TIFF strip offset (0x0111).
const TAG_STRIP_OFFSETS: u16 = 0x0111;

/// TIFF rows per strip (0x0116).
const TAG_ROWS_PER_STRIP: u16 = 0x0116;

/// TIFF strip byte counts (0x0117).
const TAG_STRIP_BYTE_COUNTS: u16 = 0x0117;

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

fn read_unsigned_fields(
    raw: &[u8],
    field_type: u16,
    count: u32,
    byte_order: ByteOrder,
) -> Option<Vec<u64>> {
    let width = match field_type {
        3 => 2usize,
        4 => 4usize,
        _ => return None,
    };
    let count = usize::try_from(count).ok()?;
    let bytes = raw.get(..count.checked_mul(width)?)?;
    let values = match (field_type, byte_order) {
        (3, ByteOrder::LittleEndian) => bytes
            .chunks_exact(2)
            .map(|value| u16::from_le_bytes([value[0], value[1]]) as u64)
            .collect(),
        (3, ByteOrder::BigEndian) => bytes
            .chunks_exact(2)
            .map(|value| u16::from_be_bytes([value[0], value[1]]) as u64)
            .collect(),
        (4, ByteOrder::LittleEndian) => bytes
            .chunks_exact(4)
            .map(|value| u32::from_le_bytes([value[0], value[1], value[2], value[3]]) as u64)
            .collect(),
        (4, ByteOrder::BigEndian) => bytes
            .chunks_exact(4)
            .map(|value| u32::from_be_bytes([value[0], value[1], value[2], value[3]]) as u64)
            .collect(),
        _ => return None,
    };
    Some(values)
}

/// ImageWidth (0x0100), ImageHeight (0x0101), BitsPerSample (0x0102),
/// PhotometricInterpretation (0x0106), Orientation (0x0112),
/// SamplesPerPixel (0x0115) and PlanarConfiguration (0x011C), the IFD1
/// fields `RebuildTIFF` consumes.
const TAG_IMAGE_WIDTH: u16 = 0x0100;
const TAG_IMAGE_HEIGHT: u16 = 0x0101;
const TAG_BITS_PER_SAMPLE: u16 = 0x0102;
const TAG_PHOTOMETRIC_INTERPRETATION: u16 = 0x0106;
const TAG_ORIENTATION: u16 = 0x0112;
const TAG_SAMPLES_PER_PIXEL: u16 = 0x0115;
const TAG_PLANAR_CONFIGURATION: u16 = 0x011C;

/// A value slot in the rebuilt directory, carrying its TIFF type.
enum RebuiltValue {
    /// int16u (TIFF SHORT, type 3); may hold several values (BitsPerSample).
    Short(Vec<u16>),
    /// int32u (TIFF LONG, type 4).
    Long(u32),
    /// rational64u (TIFF RATIONAL, type 5) as numerator/denominator.
    Rational(u32, u32),
}

/// Rebuilds an uncompressed strip-based IFD1 image as ExifTool's
/// self-contained `ThumbnailTIFF`/`PreviewTIFF`, replicating `RebuildTIFF`
/// and `GenerateTIFF` (Exif.pm:6139-6208 and 6101-6130, pinned 13.59) byte
/// for byte:
///
/// * gates: `SubfileType == 1` (reduced-resolution image) and
///   `Compression == 1` (uncompressed); ImageWidth, ImageHeight,
///   BitsPerSample, PhotometricInterpretation, StripOffsets,
///   SamplesPerPixel, RowsPerStrip and StripByteCounts must all be present
///   in this IFD, while PlanarConfiguration and Orientation default to 1;
/// * validation: every strip's byte count must equal
///   `rowBytes * RowsPerStrip`, where `rowBytes` is
///   `sum(ImageWidth * int((bits + 7) / 8))` over the BitsPerSample values,
///   and every strip must read back in full;
/// * layout: a fixed 15-entry directory (0x00FE..0x0128) in this IFD's byte
///   order and ascending tag order, SubfileType rewritten to 0, a single
///   strip (`RowsPerStrip = ImageHeight`,
///   `StripByteCounts = ImageHeight * rowBytes`), XResolution/YResolution 72
///   and ResolutionUnit inches, each tag in its `Exif::Main` Writable format
///   (the conditional-list tags 0x0111/0x0117 fall back to int32u),
///   out-of-line values appended after the directory in entry order, and the
///   concatenated strip data last;
/// * naming: `PreviewTIFF` when ImageWidth > 256, else `ThumbnailTIFF`
///   (Exif.pm:6199-6203).
fn rebuild_thumbnail_tiff(
    reader: &dyn FileReader,
    entries: &[(u16, u16, u32, std::borrow::Cow<'_, [u8]>)],
    byte_order: ByteOrder,
) -> Option<(&'static str, Vec<u8>)> {
    let field = |tag: u16| entries.iter().find(|entry| entry.0 == tag);
    let scalar = |tag: u16| {
        let entry = field(tag)?;
        read_unsigned_field(entry.3.as_ref(), entry.1, entry.2, entry.0, byte_order)
    };
    let vector = |tag: u16| {
        let entry = field(tag)?;
        read_unsigned_fields(entry.3.as_ref(), entry.1, entry.2, byte_order)
    };

    // RebuildTIFF only processes a SubfileType == 1 (reduced-resolution)
    // directory whose Compression is 1 (uncompressed).
    if scalar(TAG_SUBFILE_TYPE)? != 1 || scalar(TAG_COMPRESSION)? != 1 {
        return None;
    }
    let width = scalar(TAG_IMAGE_WIDTH)?;
    let height = scalar(TAG_IMAGE_HEIGHT)?;
    let photometric = scalar(TAG_PHOTOMETRIC_INTERPRETATION)?;
    let samples_per_pixel = scalar(TAG_SAMPLES_PER_PIXEL)?;
    let rows_per_strip = scalar(TAG_ROWS_PER_STRIP)?;
    let bits = vector(TAG_BITS_PER_SAMPLE)?;
    let offsets = vector(TAG_STRIP_OFFSETS)?;
    let counts = vector(TAG_STRIP_BYTE_COUNTS)?;
    let planar_configuration = scalar(TAG_PLANAR_CONFIGURATION).unwrap_or(1);
    let orientation = scalar(TAG_ORIENTATION).unwrap_or(1);
    if bits.is_empty() {
        return None;
    }

    // $rowBytes += $w * int(($_+7)/8) foreach @bits;
    let mut row_bytes: u64 = 0;
    for bit in &bits {
        row_bytes = row_bytes.checked_add(width.checked_mul(bit.checked_add(7)? / 8)?)?;
    }
    let expected_strip_len = row_bytes.checked_mul(rows_per_strip)?;

    // Read and concatenate the strips; any short or failed read aborts, as
    // ExtractBinary's failure does.
    let mut data = Vec::new();
    for (index, &offset) in offsets.iter().enumerate() {
        if *counts.get(index)? != expected_strip_len {
            return None;
        }
        let len = usize::try_from(expected_strip_len).ok()?;
        let strip = reader.read(offset, len).ok()?;
        if strip.len() != len {
            return None;
        }
        data.extend_from_slice(strip);
    }

    // GenerateTIFF's fixed entry set, ascending tag order.
    let directory: [(u16, RebuiltValue); 15] = [
        (TAG_SUBFILE_TYPE, RebuiltValue::Long(0)),
        (TAG_IMAGE_WIDTH, RebuiltValue::Long(width as u32)),
        (TAG_IMAGE_HEIGHT, RebuiltValue::Long(height as u32)),
        (
            TAG_BITS_PER_SAMPLE,
            RebuiltValue::Short(bits.iter().map(|bit| *bit as u16).collect()),
        ),
        (TAG_COMPRESSION, RebuiltValue::Short(vec![1])),
        (
            TAG_PHOTOMETRIC_INTERPRETATION,
            RebuiltValue::Short(vec![photometric as u16]),
        ),
        (TAG_STRIP_OFFSETS, RebuiltValue::Long(0)), // fixed up below
        (
            TAG_ORIENTATION,
            RebuiltValue::Short(vec![orientation as u16]),
        ),
        (
            TAG_SAMPLES_PER_PIXEL,
            RebuiltValue::Short(vec![samples_per_pixel as u16]),
        ),
        (TAG_ROWS_PER_STRIP, RebuiltValue::Long(height as u32)),
        (
            TAG_STRIP_BYTE_COUNTS,
            RebuiltValue::Long(height.checked_mul(row_bytes)? as u32),
        ),
        (0x011A, RebuiltValue::Rational(72, 1)), // XResolution
        (0x011B, RebuiltValue::Rational(72, 1)), // YResolution
        (
            TAG_PLANAR_CONFIGURATION,
            RebuiltValue::Short(vec![planar_configuration as u16]),
        ),
        (0x0128, RebuiltValue::Short(vec![2])), // ResolutionUnit = inches
    ];

    // Header (10 bytes) + entries + the next-IFD terminator.
    let directory_end = 10 + 12 * directory.len() + 4;
    let mut out = Vec::with_capacity(directory_end);
    out.extend_from_slice(match byte_order {
        ByteOrder::LittleEndian => b"II",
        ByteOrder::BigEndian => b"MM",
    });
    push_u16(&mut out, 42, byte_order);
    push_u32(&mut out, 8, byte_order);
    push_u16(&mut out, directory.len() as u16, byte_order);

    let mut out_of_line = Vec::new();
    let mut strip_offset_position = None;
    for (tag, value) in &directory {
        push_u16(&mut out, *tag, byte_order);
        let (tiff_type, size, bytes) = match value {
            RebuiltValue::Short(items) => {
                let mut encoded = Vec::with_capacity(items.len() * 2);
                for item in items {
                    push_u16(&mut encoded, *item, byte_order);
                }
                (3u16, 2usize, encoded)
            }
            RebuiltValue::Long(item) => {
                let mut encoded = Vec::with_capacity(4);
                push_u32(&mut encoded, *item, byte_order);
                (4, 4, encoded)
            }
            RebuiltValue::Rational(numerator, denominator) => {
                let mut encoded = Vec::with_capacity(8);
                push_u32(&mut encoded, *numerator, byte_order);
                push_u32(&mut encoded, *denominator, byte_order);
                (5, 8, encoded)
            }
        };
        push_u16(&mut out, tiff_type, byte_order);
        push_u32(&mut out, (bytes.len() / size) as u32, byte_order);
        if *tag == TAG_STRIP_OFFSETS {
            strip_offset_position = Some(out.len());
        }
        if bytes.len() > 4 {
            push_u32(
                &mut out,
                (directory_end + out_of_line.len()) as u32,
                byte_order,
            );
            out_of_line.extend_from_slice(&bytes);
        } else {
            // Inline values are right-padded with NULs regardless of byte
            // order, as Set-then-pad does in GenerateTIFF.
            out.extend_from_slice(&bytes);
            out.resize(out.len() + 4 - bytes.len(), 0);
        }
    }
    push_u32(&mut out, 0, byte_order); // no IFD1 in the rebuilt file

    // StripOffsets points at the strip data, which follows the out-of-line
    // values.
    let data_offset = (directory_end + out_of_line.len()) as u32;
    let position = strip_offset_position?;
    let patched = match byte_order {
        ByteOrder::LittleEndian => data_offset.to_le_bytes(),
        ByteOrder::BigEndian => data_offset.to_be_bytes(),
    };
    out[position..position + 4].copy_from_slice(&patched);

    out.extend_from_slice(&out_of_line);
    out.extend_from_slice(&data);
    Some((
        if width > 256 {
            "PreviewTIFF"
        } else {
            "ThumbnailTIFF"
        },
        out,
    ))
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
            TAG_SUBFILE_TYPE => {
                // Family 1 is IFD1 here, like every sibling in this
                // directory: `exiftool -G1` prints `[IFD1] SubfileType`.
                metadata.insert(
                    lookup_tag_name(*tag_id, "IFD1"),
                    raw_bytes_to_tag_value(
                        raw_bytes,
                        *field_type,
                        *value_count,
                        *tag_id,
                        byte_order,
                    ),
                );
            }
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
            TAG_STRIP_OFFSETS | TAG_ROWS_PER_STRIP | TAG_STRIP_BYTE_COUNTS => {
                let tag_name = lookup_tag_name(*tag_id, "IFD1");
                let base_name = tag_name
                    .split_once(':')
                    .map_or(tag_name.as_str(), |(_, name)| name);

                // At family 0 the IFD0 copy has precedence over IFD1, as it
                // does for Compression below. AppleQT-200.jpg has these only
                // in IFD1, where ExifTool reports all three.
                if metadata.get(&format!("IFD0:{base_name}")).is_none() {
                    let tag_value = if *tag_id == TAG_STRIP_OFFSETS && *value_count == 1 {
                        // TIFF stores IFD1 strip locations relative to the
                        // APP1 TIFF header, while ExifTool reports the file
                        // offset (AppleQT-200: 784 + 12 = 796).
                        read_unsigned_field(
                            raw_bytes,
                            *field_type,
                            *value_count,
                            *tag_id,
                            byte_order,
                        )
                        .and_then(|offset| offset.checked_add(tiff_base))
                        .and_then(|offset| i64::try_from(offset).ok())
                        .map(TagValue::new_integer)
                        .unwrap_or_else(|| {
                            raw_bytes_to_tag_value(
                                raw_bytes,
                                *field_type,
                                *value_count,
                                *tag_id,
                                byte_order,
                            )
                        })
                    } else {
                        raw_bytes_to_tag_value(
                            raw_bytes,
                            *field_type,
                            *value_count,
                            *tag_id,
                            byte_order,
                        )
                    };
                    metadata.insert(tag_name, tag_value);
                }
            }
            _ => {}
        }
    }

    if let Some((name, tiff)) = rebuild_thumbnail_tiff(reader, &entries, byte_order) {
        // RebuildTIFF names the rebuilt image after the SubfileType tag's
        // groups (family 1 = IFD1), calling it PreviewTIFF above 256 pixels
        // wide (Exif.pm:6199-6203).
        metadata.insert(format!("IFD1:{name}"), TagValue::new_binary(tiff));
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
    // A zero length is not a thumbnail and ExifTool reports none either (four
    // corpus files - ExifTool.jpg, XMP.jpg, PDF.pdf, SamsungSGH_G810.jpg -
    // carry ThumbnailOffset/Length with Length 0 and no ThumbnailImage).
    if length == 0 || length > MAX_THUMBNAIL_BYTES {
        return;
    }
    metadata.insert(
        "IFD1:ThumbnailImage",
        read_or_placeholder(reader, offset, length),
    );
}

/// Parses the Leica-preview IFD (IFD2) that follows IFD1 and emits
/// `PreviewImage`.
///
/// A JPEG's APP1 EXIF payload can chain IFD0 -> IFD1 -> IFD2; Leica JPEGs use
/// IFD2 to carry a second, larger preview. `Exif.pm:707-768` excludes APP1's
/// IFD2 from the generic `StripOffsets`/`StripByteCounts` naming for tags
/// `0x0111`/`0x0117` (comment: "APP1 IFD2 is for Leica JPEG preview"),
/// naming the pair `PreviewImageStart`/`PreviewImageLength` with
/// `DataTag => 'PreviewImage'` instead.
///
/// `reader` must address the TIFF structure itself (offset 0 == TIFF
/// header) - the same convention `parse_ifd1_thumbnail` uses - since the
/// offsets stored in IFD2 are TIFF-relative and `read_or_placeholder` reads
/// directly against `reader`.
pub fn parse_ifd2_preview_image(
    reader: &dyn FileReader,
    ifd0_offset: u64,
    ifd0_entry_count: usize,
    byte_order: ByteOrder,
    tiff_base: u64,
    metadata: &mut MetadataMap,
) {
    let Some(ifd1_offset) = next_ifd_offset(reader, ifd0_offset, ifd0_entry_count, byte_order)
    else {
        return;
    };

    let mut visited = visited_directory_offsets(reader, ifd0_offset, byte_order);
    if visited.contains(&ifd1_offset) {
        return;
    }

    // Bail if IFD1 itself is too malformed to parse at all (this call's
    // result is otherwise unused here -- IFD2's own tags are what get read
    // below -- but it's the same directory-level validation every other
    // caller in this chain performs before trusting an offset into it).
    if parse_ifd(reader, ifd1_offset, byte_order).is_err() {
        return;
    }

    // On-disk count, not a parsed-entries `.len()` -- see `ifd_entry_count`'s
    // doc comment. parse_ifd can silently drop malformed IFD1 entries, and
    // this offset locates IFD1's *own* next-IFD pointer.
    let Some(ifd1_entry_count) = ifd_entry_count(reader, ifd1_offset, byte_order) else {
        return;
    };
    let Some(ifd2_offset) =
        next_ifd_offset(reader, ifd1_offset, ifd1_entry_count as usize, byte_order)
    else {
        return;
    };

    visited.push(ifd1_offset);
    if visited.contains(&ifd2_offset) {
        return;
    }

    let Ok(ifd2_entries) = parse_ifd(reader, ifd2_offset, byte_order) else {
        return;
    };

    let mut preview_start: Option<u64> = None;
    let mut preview_length: Option<u64> = None;
    let mut jpg_from_raw_start: Option<u64> = None;
    let mut jpg_from_raw_length: Option<u64> = None;
    for (tag_id, field_type, value_count, raw_bytes) in &ifd2_entries {
        match *tag_id {
            0x0111 => {
                preview_start =
                    read_unsigned_field(raw_bytes, *field_type, *value_count, *tag_id, byte_order);
            }
            0x0117 => {
                preview_length =
                    read_unsigned_field(raw_bytes, *field_type, *value_count, *tag_id, byte_order);
            }
            // Leica TL2's IFD2 carries the larger embedded JPEG via the
            // JPEGInterchangeFormat pair. ExifTool names this JpgFromRaw
            // outside IFD1 (Exif.pm's contextual 0x0201/0x0202 entries).
            0x0201 => {
                jpg_from_raw_start =
                    read_unsigned_field(raw_bytes, *field_type, *value_count, *tag_id, byte_order);
            }
            0x0202 => {
                jpg_from_raw_length =
                    read_unsigned_field(raw_bytes, *field_type, *value_count, *tag_id, byte_order);
            }
            _ => {}
        }
    }

    if let (Some(start), Some(length)) = (preview_start, preview_length)
        && let Some(absolute_start) = start.checked_add(tiff_base)
    {
        metadata.insert(
            "IFD2:PreviewImageStart",
            TagValue::new_integer(absolute_start as i64),
        );
        metadata.insert(
            "IFD2:PreviewImageLength",
            TagValue::new_integer(length as i64),
        );
        if length > 0 {
            metadata.insert(
                "IFD2:PreviewImage",
                read_or_placeholder(reader, start, length),
            );
        }
    }

    if let (Some(start), Some(length)) = (jpg_from_raw_start, jpg_from_raw_length)
        && let Some(absolute_start) = start.checked_add(tiff_base)
    {
        metadata.insert(
            "IFD2:JpgFromRawStart",
            TagValue::new_integer(absolute_start as i64),
        );
        metadata.insert(
            "IFD2:JpgFromRawLength",
            TagValue::new_integer(length as i64),
        );
        if length > 0 {
            metadata.insert(
                "IFD2:JpgFromRaw",
                read_or_placeholder(reader, start, length),
            );
        }
    }
}

/// The value ExifTool reports for a binary tag: the bytes when the range is
/// readable, and the length-only placeholder when it is not.
///
/// With the Binary option off, `ExtractBinary` returns its placeholder *before*
/// it ever touches the file:
///
/// ```text
/// ExifTool.pm:9828          if ((not $$options{Binary} or $$self{EXCL_TAG_LOOKUP}{$lcTag}) and
/// ExifTool.pm:9829               not $$options{Verbose} and not $$options{Validate} and
/// ExifTool.pm:9830               not $$self{REQ_TAG_LOOKUP}{$lcTag})
/// ExifTool.pm:9831          {
/// ExifTool.pm:9832              return "Binary data $length bytes";
/// ExifTool.pm:9833          }
/// ExifTool.pm:9835      unless ($$self{RAF}->Seek($offset,0)
/// ExifTool.pm:9836          and $$self{RAF}->Read($buff, $length) == $length)
/// ```
///
/// The seek that would fail is on line 9835, below the `return` on 9832, so a
/// truncated file still reports the tag at its *declared* length. Two corpus
/// files are truncated this way and oxidex used to drop the tag on both:
///
/// * `Panasonic/PanasonicDMC-LS60.jpg` - 8,210 bytes, thumbnail declared at
///   offset 7,966 for 6,974 bytes, so the range ends at 14,940;
/// * `Sony/SonyCLIE_PEG-NZ90.jpg` - 7,789 bytes, declared at 1,276 for 6,658,
///   ending at 7,934.
///
/// Emitting the placeholder is not "inventing a length": the declared length is
/// exactly what ExifTool prints, and it is reported as a pre-rendered string
/// rather than a `TagValue::Binary` precisely because there are no bytes behind
/// it -- nothing downstream can mistake it for readable data.
fn read_or_placeholder(reader: &dyn FileReader, offset: u64, length: u64) -> TagValue {
    let readable = offset
        .checked_add(length)
        .is_some_and(|end| end <= reader.size())
        .then(|| reader.read(offset, length as usize).ok())
        .flatten()
        // A reader that clamps a short read instead of failing would otherwise
        // make us report a byte count the file does not actually contain.
        .filter(|bytes| bytes.len() as u64 == length);

    match readable {
        Some(bytes) => TagValue::new_binary(bytes.to_vec()),
        None => TagValue::new_string(format!(
            "(Binary data {} bytes, use -b option to extract)",
            length
        )),
    }
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

    // Sony's own `PreviewImage` (0x2001) is deliberately absent from Sony's
    // string-map `MAIN_TABLE`: its value routinely lives outside the
    // MakerNote payload the dispatcher hands that table, so it needs the
    // whole TIFF block the way Sigma's preview does. Unlike Sigma this is an
    // addition alongside the normal dispatch below, not a replacement for
    // it - every other Sony tag reaches metadata through the ordinary path.
    parse_sony_preview_image_if_sony(&make, ctx, metadata);

    // Casio Type2's `PreviewImage` (0x2000) and Olympus's (via
    // `CameraSettings` 0x0100/0x0101/0x0102) and Minolta's (via 0x0081 or
    // 0x0088/0x0089) are the same shape: a value the string-map dispatcher's
    // `HashMap<String, String>` can't carry, needing `MetadataMap`/
    // `TagValue::Binary` and (for Olympus/Minolta) the whole TIFF block the
    // way Sigma's/Sony's do. All three are additions alongside the normal
    // dispatch below, not a replacement for it - every other tag from these
    // makes still reaches metadata through the ordinary path.
    parse_casio_preview_image_if_casio(&make, ctx, byte_order, metadata);
    parse_casio_type2_extra_tags_if_casio(&make, ctx, byte_order, metadata);
    parse_ricoh_extra_tags_if_ricoh(&make, ctx, byte_order, model.as_deref(), metadata);
    parse_ge_extra_tags_if_ge(&make, ctx, byte_order, metadata);
    parse_olympus_preview_image_if_olympus(&make, ctx, byte_order, metadata);
    parse_minolta_preview_image_if_minolta(&make, ctx, byte_order, metadata);

    // Parse MakerNote using the dispatcher
    let mut makernote_tags = HashMap::new();
    let mut value_forms = HashMap::new();
    if let Err(e) = dispatch_makernote_with_context_and_values(
        &make,
        model.as_deref(),
        ctx,
        byte_order,
        &mut makernote_tags,
        &mut value_forms,
    ) {
        // A MakerNote that fails to parse must not fail the file -- ExifTool
        // warns and goes on reading the rest of it -- but it must not be
        // silent either. This is the JPEG/EXIF dispatch site, and it was the
        // only one of the four that dropped the error: `file_parser.rs` and
        // both `raw/metadata.rs` sites already print this exact line. Staying
        // quiet here meant a whole class of MakerNote failure produced no
        // output and no warning, so it looked identical to a file with no
        // MakerNote at all and never appeared in any coverage report.
        eprintln!("Warning: Failed to parse MakerNote for {}: {}", make, e);
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

/// Extracts Sony MakerNotes 0x2001 (`PreviewImage`) when `make` is Sony.
///
/// A no-op for any other make. See
/// [`crate::parsers::tiff::makernotes::sony::parse_sony_preview_image_tag`]
/// for why this needs the `MakerNoteContext`'s full TIFF block rather than
/// the payload the string-map dispatcher's trait receives.
fn parse_sony_preview_image_if_sony(
    make: &str,
    ctx: &MakerNoteContext<'_>,
    metadata: &mut MetadataMap,
) {
    if make.trim().to_ascii_lowercase() != "sony" {
        return;
    }
    crate::parsers::tiff::makernotes::sony::parse_sony_preview_image_tag(ctx, metadata);
}

/// Extracts Casio Type2's `PreviewImage` (0x2000) when `make` is Casio.
///
/// A no-op for any other make, and for a Casio Type1 ("Main") payload, which
/// has no 0x2000 tag. Matches by prefix rather than
/// `makernote_dispatcher.rs`'s exact-match `"casio computer co.,ltd."` entry,
/// which carries a trailing period the real `Make` string
/// (`"CASIO COMPUTER CO.,LTD"`, verified on `Casio2.jpg`) does not have and
/// so never matches -- a pre-existing dispatcher bug out of this task's
/// scope to fix, but not one worth reproducing here.
/// See [`crate::parsers::tiff::makernotes::casio::parse_casio_preview_image_tag`].
fn parse_casio_preview_image_if_casio(
    make: &str,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    if !make.trim().to_ascii_lowercase().starts_with("casio") {
        return;
    }
    crate::parsers::tiff::makernotes::casio::parse_casio_preview_image_tag(
        ctx, byte_order, metadata,
    );
}

/// Extracts Casio Type2's `PreviewImageSize`, `FlashDistance`,
/// `HometownCity` and `FirmwareDate` when `make` is Casio.
///
/// A no-op for any other make, and for a Casio Type1 ("Main") payload. See
/// [`crate::parsers::tiff::makernotes::casio::parse_casio_type2_extra_tags`].
fn parse_casio_type2_extra_tags_if_casio(
    make: &str,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    if !make.trim().to_ascii_lowercase().starts_with("casio") {
        return;
    }
    crate::parsers::tiff::makernotes::casio::parse_casio_type2_extra_tags(
        ctx, byte_order, metadata,
    );
}

/// Extracts Ricoh's `ImageInfo` (RicohImageWidth/RicohImageHeight/RicohDate)
/// and `Subdir` (ManufactureDate1/ManufactureDate2) sub-directory tags when
/// `make` is Ricoh.
///
/// A no-op for any other make. See
/// [`crate::parsers::tiff::makernotes::ricoh::parse_ricoh_extra_tags`].
fn parse_ricoh_extra_tags_if_ricoh(
    make: &str,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    model: Option<&str>,
    metadata: &mut MetadataMap,
) {
    if !make.trim().to_ascii_lowercase().starts_with("ricoh") {
        return;
    }
    crate::parsers::tiff::makernotes::ricoh::parse_ricoh_extra_tags(
        ctx, byte_order, model, metadata,
    );
}

/// Extracts GE's `GEModel`/`GEMake` when `make` is General Imaging (branded
/// "General Imaging Co." in EXIF, per `makernote_dispatcher.rs`'s
/// `parser_for_make_prefix`).
///
/// A no-op for any other make. See
/// [`crate::parsers::tiff::makernotes::ge::parse_ge_extra_tags`].
fn parse_ge_extra_tags_if_ge(
    make: &str,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    if !make
        .trim()
        .to_ascii_lowercase()
        .starts_with("general imaging")
    {
        return;
    }
    crate::parsers::tiff::makernotes::ge::parse_ge_extra_tags(ctx, byte_order, metadata);
}

/// Extracts Olympus's `PreviewImage` (`CameraSettings` 0x0100/0x0101/0x0102)
/// when `make` starts with "olympus" or "om digital solutions" - the same
/// prefix match `makernote_dispatcher.rs` uses to route to `OlympusParser`.
///
/// A no-op for any other make. See
/// [`crate::parsers::tiff::makernotes::olympus::parse_olympus_preview_image_tag`].
fn parse_olympus_preview_image_if_olympus(
    make: &str,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    let make = make.trim().to_ascii_lowercase();
    if !(make.starts_with("olympus") || make.starts_with("om digital solutions")) {
        return;
    }
    crate::parsers::tiff::makernotes::olympus::parse_olympus_preview_image_tag(
        ctx, byte_order, metadata,
    );
}

/// Extracts Minolta's `PreviewImage` (0x0081, or the 0x0088/0x0089 offset
/// pair) when `make` is Minolta or Konica Minolta - the same match
/// `makernote_dispatcher.rs` uses to route to `MinoltaParser`.
///
/// A no-op for any other make. See
/// [`crate::parsers::tiff::makernotes::minolta::parse_minolta_preview_image_tag`].
fn parse_minolta_preview_image_if_minolta(
    make: &str,
    ctx: &MakerNoteContext<'_>,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    if !matches!(
        make.trim().to_ascii_lowercase().as_str(),
        "minolta" | "konica minolta" | "minolta co., ltd."
    ) {
        return;
    }
    crate::parsers::tiff::makernotes::minolta::parse_minolta_preview_image_tag(
        ctx, byte_order, metadata,
    );
}

#[cfg(test)]
mod exif_subifd_tests {
    use super::*;
    use crate::test_support::TestReader;

    const SHORT: u16 = 3;

    fn one_entry_ifd(tag_id: u16, field_type: u16, value: u32) -> TestReader {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&tag_id.to_le_bytes());
        data.extend_from_slice(&field_type.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&value.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        TestReader::new(data)
    }

    #[test]
    fn exif_subifd_duplicate_yields_to_ifd0() {
        let reader = one_entry_ifd(TAG_COMPRESSION, SHORT, 0);
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Compression", TagValue::new_integer(4));

        parse_exif_subifd(
            &reader,
            8,
            ByteOrder::LittleEndian,
            0,
            reader.size(),
            &mut metadata,
        );

        assert_eq!(
            metadata
                .get("IFD0:Compression")
                .and_then(TagValue::as_integer),
            Some(4)
        );
        assert!(metadata.get("ExifIFD:Compression").is_none());
    }

    #[test]
    fn exif_subifd_tag_emits_without_ifd0_twin() {
        let reader = one_entry_ifd(TAG_COMPRESSION, SHORT, 0);
        let mut metadata = MetadataMap::new();

        parse_exif_subifd(
            &reader,
            8,
            ByteOrder::LittleEndian,
            0,
            reader.size(),
            &mut metadata,
        );

        assert_eq!(
            metadata
                .get("ExifIFD:Compression")
                .and_then(TagValue::as_integer),
            Some(0)
        );
    }
}

#[cfg(test)]
mod ifd1_tests {
    use super::*;
    use crate::test_support::TestReader;

    const SHORT: u16 = 3;
    const LONG: u16 = 4;

    #[test]
    fn apple_qt_200_ifd1_strip_metadata_matches_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new(
            "/tmp/oxidex-exiftool-cache/combined-samples/Apple/AppleQT-200.jpg",
        );
        let metadata = crate::core::operations::read_metadata(path).expect("AppleQT-200 parses");

        assert_eq!(metadata.get_integer("IFD1:StripOffsets"), Some(796));
        assert_eq!(metadata.get_integer("IFD1:RowsPerStrip"), Some(60));
        assert_eq!(metadata.get_integer("IFD1:StripByteCounts"), Some(9600));
    }

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

    /// A minimal uncompressed reduced-resolution IFD1 (2x1 grayscale, one
    /// strip of "AB") must rebuild into the exact byte stream ExifTool's
    /// GenerateTIFF produces: 10-byte header, the fixed 15 entries in
    /// ascending tag order, no next IFD, XResolution/YResolution 72/1
    /// out-of-line, then the strip data at offset 210.
    #[test]
    fn rebuilt_thumbnail_tiff_matches_generate_tiff_byte_layout() {
        let thumb = *b"AB";
        // IFD1 sits at 14; ten 12-byte entries + count + next pointer put the
        // strip data at 14 + 2 + 120 + 4 = 140.
        let strip_offset = 140u32;
        let metadata = run(
            &[
                (TAG_SUBFILE_TYPE, LONG, 1),
                (TAG_IMAGE_WIDTH, LONG, 2),
                (TAG_IMAGE_HEIGHT, LONG, 1),
                (TAG_BITS_PER_SAMPLE, SHORT, 8),
                (TAG_COMPRESSION, SHORT, 1),
                (TAG_PHOTOMETRIC_INTERPRETATION, SHORT, 1),
                (TAG_STRIP_OFFSETS, LONG, strip_offset),
                (TAG_SAMPLES_PER_PIXEL, SHORT, 1),
                (TAG_ROWS_PER_STRIP, LONG, 1),
                (TAG_STRIP_BYTE_COUNTS, LONG, 2),
            ],
            &thumb,
            0,
        );

        let mut expected: Vec<u8> = Vec::new();
        expected.extend_from_slice(b"II");
        expected.extend_from_slice(&42u16.to_le_bytes());
        expected.extend_from_slice(&8u32.to_le_bytes());
        expected.extend_from_slice(&15u16.to_le_bytes());
        // (tag, type, count, value-field) per GenerateTIFF; SHORT values are
        // right-padded to four bytes.
        let entries: [(u16, u16, u32, [u8; 4]); 15] = [
            (0x00FE, 4, 1, 0u32.to_le_bytes()),   // SubfileType = 0
            (0x0100, 4, 1, 2u32.to_le_bytes()),   // ImageWidth
            (0x0101, 4, 1, 1u32.to_le_bytes()),   // ImageHeight
            (0x0102, 3, 1, [8, 0, 0, 0]),         // BitsPerSample
            (0x0103, 3, 1, [1, 0, 0, 0]),         // Compression
            (0x0106, 3, 1, [1, 0, 0, 0]),         // PhotometricInterpretation
            (0x0111, 4, 1, 210u32.to_le_bytes()), // StripOffsets -> data
            (0x0112, 3, 1, [1, 0, 0, 0]),         // Orientation (default)
            (0x0115, 3, 1, [1, 0, 0, 0]),         // SamplesPerPixel
            (0x0116, 4, 1, 1u32.to_le_bytes()),   // RowsPerStrip = height
            (0x0117, 4, 1, 2u32.to_le_bytes()),   // StripByteCounts
            (0x011A, 5, 1, 194u32.to_le_bytes()), // XResolution, out-of-line
            (0x011B, 5, 1, 202u32.to_le_bytes()), // YResolution, out-of-line
            (0x011C, 3, 1, [1, 0, 0, 0]),         // PlanarConfiguration (default)
            (0x0128, 3, 1, [2, 0, 0, 0]),         // ResolutionUnit = inches
        ];
        for (tag, tiff_type, count, value) in entries {
            expected.extend_from_slice(&tag.to_le_bytes());
            expected.extend_from_slice(&tiff_type.to_le_bytes());
            expected.extend_from_slice(&count.to_le_bytes());
            expected.extend_from_slice(&value);
        }
        expected.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
        expected.extend_from_slice(&72u32.to_le_bytes()); // XResolution 72/1
        expected.extend_from_slice(&1u32.to_le_bytes());
        expected.extend_from_slice(&72u32.to_le_bytes()); // YResolution 72/1
        expected.extend_from_slice(&1u32.to_le_bytes());
        expected.extend_from_slice(&thumb);

        assert_eq!(
            metadata.get("IFD1:ThumbnailTIFF"),
            Some(&TagValue::new_binary(expected))
        );
    }

    /// RebuildTIFF proceeds only for SubfileType == 1 with Compression == 1
    /// and strips whose byte count equals rowBytes * RowsPerStrip; anything
    /// else is omitted, never approximated.
    #[test]
    fn rebuilt_thumbnail_tiff_honours_exiftool_gates() {
        let thumb = *b"AB";
        let base: [(u16, u16, u32); 10] = [
            (TAG_SUBFILE_TYPE, LONG, 1),
            (TAG_IMAGE_WIDTH, LONG, 2),
            (TAG_IMAGE_HEIGHT, LONG, 1),
            (TAG_BITS_PER_SAMPLE, SHORT, 8),
            (TAG_COMPRESSION, SHORT, 1),
            (TAG_PHOTOMETRIC_INTERPRETATION, SHORT, 1),
            (TAG_STRIP_OFFSETS, LONG, 140),
            (TAG_SAMPLES_PER_PIXEL, SHORT, 1),
            (TAG_ROWS_PER_STRIP, LONG, 1),
            (TAG_STRIP_BYTE_COUNTS, LONG, 2),
        ];

        // SubfileType 0 (full-resolution image): not a thumbnail.
        let mut wrong_subfile = base;
        wrong_subfile[0].2 = 0;
        assert_eq!(
            run(&wrong_subfile, &thumb, 0).get("IFD1:ThumbnailTIFF"),
            None
        );

        // Compression 6 (JPEG): RebuildTIFF requires uncompressed data.
        let mut compressed = base;
        compressed[4].2 = 6;
        assert_eq!(run(&compressed, &thumb, 0).get("IFD1:ThumbnailTIFF"), None);

        // StripByteCounts != rowBytes * RowsPerStrip: invalid strip, omitted.
        let mut bad_strip = base;
        bad_strip[9].2 = 3;
        assert_eq!(run(&bad_strip, &thumb, 0).get("IFD1:ThumbnailTIFF"), None);

        // A missing required field (no PhotometricInterpretation) also omits.
        let missing: Vec<_> = base
            .iter()
            .copied()
            .filter(|entry| entry.0 != TAG_PHOTOMETRIC_INTERPRETATION)
            .collect();
        assert_eq!(run(&missing, &thumb, 0).get("IFD1:ThumbnailTIFF"), None);
    }

    /// Above 256 pixels wide, RebuildTIFF files the image as PreviewTIFF
    /// (Exif.pm:6199-6203), still under IFD1.
    #[test]
    fn wide_rebuilt_image_is_named_preview_tiff() {
        let thumb = [0x55u8; 300];
        let metadata = run(
            &[
                (TAG_SUBFILE_TYPE, LONG, 1),
                (TAG_IMAGE_WIDTH, LONG, 300),
                (TAG_IMAGE_HEIGHT, LONG, 1),
                (TAG_BITS_PER_SAMPLE, SHORT, 8),
                (TAG_COMPRESSION, SHORT, 1),
                (TAG_PHOTOMETRIC_INTERPRETATION, SHORT, 1),
                (TAG_STRIP_OFFSETS, LONG, 140),
                (TAG_SAMPLES_PER_PIXEL, SHORT, 1),
                (TAG_ROWS_PER_STRIP, LONG, 1),
                (TAG_STRIP_BYTE_COUNTS, LONG, 300),
            ],
            &thumb,
            0,
        );
        assert_eq!(metadata.get("IFD1:ThumbnailTIFF"), None);
        assert!(matches!(
            metadata.get("IFD1:PreviewTIFF"),
            Some(TagValue::Binary(bytes)) if bytes.len() == 210 + 300
        ));
    }

    /// The pinned Leica sample carries the one JPEG-path ThumbnailTIFF in the
    /// corpus; the rebuilt stream was verified byte-identical to
    /// `exiftool -b -ThumbnailTIFF` (47952 bytes), so pin its shape.
    #[test]
    fn leica_r9_dmr_thumbnail_tiff_matches_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new(
            "/tmp/oxidex-exiftool-cache/combined-samples/Leica/LeicaR9-DigitalBackDMR.jpg",
        );
        let metadata = crate::core::operations::read_metadata(path).expect("Leica R9 DMR parses");
        let Some(TagValue::Binary(tiff)) = metadata.get("IFD1:ThumbnailTIFF") else {
            panic!("IFD1:ThumbnailTIFF must be emitted");
        };
        assert_eq!(tiff.len(), 47952);
        // Header, entry count, and the strip data offset (216 = 194 + the
        // 22 out-of-line bytes: BitsPerSample "8 8 8" plus two rationals).
        assert_eq!(&tiff[..10], b"II\x2a\0\x08\0\0\0\x0f\0");
        assert_eq!(&tiff[90..94], &216u32.to_le_bytes());
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

    /// A thumbnail range that runs past EOF is still reported, at its declared
    /// length. `ExtractBinary` returns the placeholder on ExifTool.pm:9832,
    /// above the seek on :9835, so truncation never suppresses the tag --
    /// PanasonicDMC-LS60.jpg (8,210 bytes, range ending at 14,940) and
    /// SonyCLIE_PEG-NZ90.jpg (7,789 bytes, ending at 7,934) both print it.
    #[test]
    fn thumbnail_past_eof_reports_the_placeholder_not_nothing() {
        let metadata = run(
            &[
                (TAG_THUMBNAIL_OFFSET, LONG, 100),
                (TAG_THUMBNAIL_LENGTH, LONG, 6974),
            ],
            &[],
            0,
        );
        assert_eq!(
            metadata
                .get("IFD1:ThumbnailLength")
                .and_then(|v| v.as_integer()),
            Some(6974)
        );
        assert_eq!(
            metadata
                .get("IFD1:ThumbnailImage")
                .and_then(|v| v.as_string()),
            Some("(Binary data 6974 bytes, use -b option to extract)")
        );
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

    /// Mirror of the thumbnail rule, and for the same reason: `OtherImage` is
    /// built by `ExtractImage`, which falls through to `ExtractBinary` for a
    /// range outside the loaded EXIF block --
    ///
    /// ```text
    /// Exif.pm:6136          $image = $et->ExtractBinary($offset, $len, $tag);
    /// ```
    ///
    /// -- and that returns the length-only placeholder before seeking
    /// (ExifTool.pm:9832). An unreadable range is reported, not dropped.
    #[test]
    fn unreadable_other_image_range_reports_the_placeholder() {
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
        assert_eq!(
            metadata
                .get("InteropIFD:OtherImage")
                .and_then(|v| v.as_string()),
            Some("(Binary data 100 bytes, use -b option to extract)")
        );
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

#[cfg(test)]
mod ifd2_preview_image_tests {
    use super::*;

    const LONG: u16 = 4;
    /// Offset of IFD0 within the synthetic TIFF built below (right after the
    /// 8-byte TIFF header).
    const TIFF_HEADER_SIZE: u64 = 8;

    /// Builds a little-endian TIFF: IFD0 (no entries) -> IFD1 (no entries) ->
    /// IFD2 carrying `0x0111`=`preview_start` (LONG) and `0x0117`=`preview_length`
    /// (LONG), followed by `trailing` bytes.
    ///
    /// Layout: header(8) | IFD0(6) @8 | IFD1(6) @14 | IFD2(30) @20 | trailing @50
    /// so IFD2's declared entries always point at offset 50 regardless of the
    /// `preview_length` value under test.
    fn build_tiff_with_ifd2(preview_start: u32, preview_length: u32, trailing: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at 8

        // IFD0: no entries, next -> IFD1 at 14
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&14u32.to_le_bytes());

        // IFD1: no entries, next -> IFD2 at 20
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&20u32.to_le_bytes());

        // IFD2: PreviewImageStart (0x0111) and PreviewImageLength (0x0117), no next IFD
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&0x0111u16.to_le_bytes());
        data.extend_from_slice(&LONG.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&preview_start.to_le_bytes());
        data.extend_from_slice(&0x0117u16.to_le_bytes());
        data.extend_from_slice(&LONG.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&preview_length.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // no IFD3

        assert_eq!(
            data.len(),
            50,
            "IFD2 entries assume trailing data starts at 50"
        );
        data.extend_from_slice(trailing);
        data
    }

    fn build_tiff_with_ifd2_preview(preview_bytes: &[u8]) -> Vec<u8> {
        build_tiff_with_ifd2(50, preview_bytes.len() as u32, preview_bytes)
    }

    fn build_tiff_with_ifd2_preview_oob(declared_length: u32) -> Vec<u8> {
        // PreviewImageLength points well past the end of the buffer.
        build_tiff_with_ifd2(50, declared_length, &[])
    }

    fn build_tiff_with_ifd2_jpg_from_raw(start: u32, length: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&14u32.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&20u32.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        for (tag, value) in [(0x0201u16, start), (0x0202, length)] {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&LONG.to_le_bytes());
            data.extend_from_slice(&1u32.to_le_bytes());
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.extend_from_slice(&0u32.to_le_bytes());
        data
    }

    // These tests call `parse_ifd2_preview_image` directly rather than
    // `parse_ifd_chain`: the rename/extraction only belongs to the
    // APP1/JPEG-embedded case (Exif.pm:707-768's "APP1 IFD2 is for Leica
    // JPEG preview"), and `parse_ifd2_preview_image` -- called only from
    // `jpeg_helpers.rs`'s JPEG-specific handler -- is the sole code path
    // that implicitly gates on that. `parse_ifd_chain`/`process_tiff_ifd_tags`
    // serve the standalone-TIFF/DNG/CR2/NEF path (`operations.rs`) and must
    // keep naming 0x111/0x117 as StripOffsets/StripByteCounts unconditionally
    // there -- a chained IFD2 in a real standalone TIFF is not a Leica
    // preview and must not be mis-renamed.
    #[test]
    fn ifd2_preview_image_start_length_pair_becomes_preview_image() {
        // Build a minimal TIFF: IFD0 (no entries of interest, next-IFD -> IFD1),
        // IFD1 (no entries of interest, next-IFD -> IFD2),
        // IFD2 with tag 0x0111 (PreviewImageStart) = some in-bounds offset and
        // tag 0x0117 (PreviewImageLength) = a small length, followed by that many
        // real bytes at that offset.
        let preview_bytes = b"\xff\xd8\xff\xdbFAKEPREVIEWDATA";
        let buffer = build_tiff_with_ifd2_preview(preview_bytes);
        let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&buffer);
        let mut metadata = MetadataMap::new();

        parse_ifd2_preview_image(
            &reader,
            TIFF_HEADER_SIZE,
            0,
            ByteOrder::LittleEndian,
            0,
            &mut metadata,
        );

        assert_eq!(
            metadata.get("IFD2:PreviewImage"),
            Some(&TagValue::new_binary(preview_bytes.to_vec()))
        );
        assert!(metadata.get("IFD2:StripOffsets").is_none());
        assert!(metadata.get("IFD2:StripByteCounts").is_none());
    }

    #[test]
    fn ifd2_preview_image_shows_placeholder_when_out_of_bounds() {
        // Same shape, but PreviewImageLength points past the end of the buffer.
        // Real ExifTool still reports the tag here (LeicaCL.jpg ground truth:
        // IFD2:PreviewImageStart=7064224 in a 50,939-byte file, yet
        // IFD2:PreviewImage is the placeholder, not omitted) because
        // ExtractBinary's shortcut returns the declared-length placeholder
        // before ever seeking. Mirror ThumbnailImage's `read_or_placeholder`.
        let declared_length = 895146u64;
        let buffer = build_tiff_with_ifd2_preview_oob(declared_length as u32);
        let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&buffer);
        let mut metadata = MetadataMap::new();

        parse_ifd2_preview_image(
            &reader,
            TIFF_HEADER_SIZE,
            0,
            ByteOrder::LittleEndian,
            0,
            &mut metadata,
        );

        assert_eq!(
            metadata.get("IFD2:PreviewImage"),
            Some(&TagValue::new_string(format!(
                "(Binary data {} bytes, use -b option to extract)",
                declared_length
            )))
        );
    }

    #[test]
    fn ifd2_jpeg_interchange_pair_becomes_jpg_from_raw() {
        let stored_start = 50u32;
        let length = 26u32;
        let buffer = build_tiff_with_ifd2_jpg_from_raw(stored_start, length);
        let reader = crate::io::buffered_reader::BufferedReader::from_bytes(&buffer);
        let mut metadata = MetadataMap::new();

        parse_ifd2_preview_image(
            &reader,
            TIFF_HEADER_SIZE,
            0,
            ByteOrder::LittleEndian,
            12,
            &mut metadata,
        );

        assert_eq!(
            metadata.get("IFD2:JpgFromRawStart"),
            Some(&TagValue::new_integer(62))
        );
        assert_eq!(
            metadata.get("IFD2:JpgFromRawLength"),
            Some(&TagValue::new_integer(i64::from(length)))
        );
        assert_eq!(
            metadata.get("IFD2:JpgFromRaw"),
            Some(&TagValue::new_string(
                "(Binary data 26 bytes, use -b option to extract)".to_string()
            ))
        );
    }
}
