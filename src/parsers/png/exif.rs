//! PNG eXIf chunk parsing
//!
//! This module handles parsing EXIF data embedded in PNG eXIf chunks,
//! including IFD0, ExifIFD, and GPS sub-IFDs.

use crate::core::{MetadataMap, TagValue};
use crate::error::Result;
use crate::io::{ByteOrder as IoByteOrder, EndianReader};
use crate::parsers::png::chunk_parser::{ExifDataReader, parse_exif_chunk};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::tag_db::lookup_tag_name;

use super::value_conversion::{raw_bytes_to_tag_value, raw_bytes_to_tag_value_no_enum};

/// Parses EXIF data from PNG eXIf chunk and inserts tags into metadata map.
///
/// This function handles the complete EXIF parsing including:
/// - IFD0 (main image tags)
/// - ExifIFD sub-IFD (extended EXIF tags)
/// - GPS sub-IFD (GPS tags)
///
/// The implementation follows the same logic as JPEG EXIF parsing to ensure
/// consistent tag naming and value conversion.
///
/// # Arguments
///
/// * `exif_data` - Raw TIFF-format EXIF data from the eXIf chunk
/// * `metadata` - Metadata map to insert parsed tags into
///
/// # Returns
///
/// - `Ok(())` if parsing succeeded
/// - `Err(ExifToolError)` if parsing failed
pub fn parse_and_insert_exif_tags(exif_data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    // Parse the eXIf chunk to get IFD0 tags
    let tags = parse_exif_chunk(exif_data)?;

    // Detect byte order from TIFF header (first 2 bytes)
    let byte_order = match &exif_data[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => {
            return Err(crate::error::ExifToolError::parse_error(
                "Invalid byte order marker in eXIf chunk",
            ));
        }
    };

    // Create a reader for the EXIF data to parse sub-IFDs
    let exif_reader = ExifDataReader::new(exif_data.to_vec());

    // Track sub-IFD offsets
    let mut exif_ifd_offset = None;
    let mut gps_ifd_offset = None;

    // Convert TIFF ByteOrder to IO ByteOrder for EndianReader
    let io_order = match byte_order {
        ByteOrder::LittleEndian => IoByteOrder::Little,
        ByteOrder::BigEndian => IoByteOrder::Big,
    };

    // Convert raw tag data to MetadataMap entries
    for (tag_id, field_type, value_count, raw_bytes) in &tags {
        // Check for EXIF Sub-IFD pointer (tag 0x8769)
        if *tag_id == 0x8769 && raw_bytes.len() >= 4 {
            let reader = EndianReader::new(raw_bytes, io_order);
            let offset = reader.u32_at(0).unwrap_or(0);
            exif_ifd_offset = Some(offset as u64);

            // Perl ExifTool outputs ExifOffset in PNG:Exif namespace
            metadata.insert(
                "PNG:ExifExifOffset".to_string(),
                TagValue::new_integer(offset as i64),
            );
            continue; // Don't add to IFD0: namespace
        }

        // Check for GPS Sub-IFD pointer (tag 0x8825)
        if *tag_id == 0x8825 && raw_bytes.len() >= 4 {
            let reader = EndianReader::new(raw_bytes, io_order);
            let offset = reader.u32_at(0).unwrap_or(0);
            gps_ifd_offset = Some(offset as u64);
            continue; // Don't add the pointer tag to metadata
        }

        // Convert tag ID to tag name
        let base_tag_name = lookup_tag_name(*tag_id, "IFD0");

        // Convert raw bytes to TagValue using the same logic as JPEG
        let tag_value =
            raw_bytes_to_tag_value(raw_bytes, *field_type, *value_count, *tag_id, byte_order);

        // Perl ExifTool outputs PNG eXIf tags in BOTH "IFD0:" AND "PNG:Exif" namespaces
        // Add the IFD0: version (with enum interpretation)
        metadata.insert(base_tag_name.clone(), tag_value);

        // Also add the PNG:Exif version (WITHOUT enum interpretation, raw values only)
        if let Some(stripped) = base_tag_name.strip_prefix("IFD0:") {
            let raw_value = raw_bytes_to_tag_value_no_enum(
                raw_bytes,
                *field_type,
                *value_count,
                *tag_id,
                byte_order,
            );
            metadata.insert(format!("PNG:Exif{}", stripped), raw_value);
        }
    }

    // Parse EXIF Sub-IFD if present
    if let Some(offset) = exif_ifd_offset
        && let Ok(exif_tags) = parse_ifd(&exif_reader, offset, byte_order)
    {
        for (tag_id, field_type, value_count, raw_bytes) in exif_tags {
            let base_tag_name = lookup_tag_name(tag_id, "ExifIFD");
            let tag_value =
                raw_bytes_to_tag_value(&raw_bytes, field_type, value_count, tag_id, byte_order);

            // Perl ExifTool outputs PNG eXIf tags in BOTH "ExifIFD:" AND "PNG:Exif" namespaces
            // Add the ExifIFD: version (with enum interpretation)
            metadata.insert(base_tag_name.clone(), tag_value);

            // Also add the PNG:Exif version (WITHOUT enum interpretation, raw values only)
            if let Some(stripped) = base_tag_name.strip_prefix("ExifIFD:") {
                let raw_value = raw_bytes_to_tag_value_no_enum(
                    &raw_bytes,
                    field_type,
                    value_count,
                    tag_id,
                    byte_order,
                );
                metadata.insert(format!("PNG:Exif{}", stripped), raw_value);
            }
        }
    }

    // Parse GPS Sub-IFD if present
    if let Some(offset) = gps_ifd_offset
        && let Ok(gps_tags) = parse_ifd(&exif_reader, offset, byte_order)
    {
        for (tag_id, field_type, value_count, raw_bytes) in gps_tags {
            // GPS tags keep their "GPS:" prefix even in PNG eXIf chunks
            let tag_name = lookup_tag_name(tag_id, "GPS");
            let tag_value =
                raw_bytes_to_tag_value(&raw_bytes, field_type, value_count, tag_id, byte_order);
            metadata.insert(tag_name, tag_value);
        }
    }

    Ok(())
}
