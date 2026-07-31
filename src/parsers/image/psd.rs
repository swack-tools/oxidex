//! Adobe Photoshop (PSD) format parser
//!
//! PSD file structure:
//! - Header (26 bytes): signature, version, reserved, channels, height, width, depth, color mode
//! - Color Mode Data section
//! - Image Resources section (contains EXIF, IPTC, XMP, etc.)
//! - Layer and Mask Information section
//! - Image Data section

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::buffered_reader::BufferedReader;
use crate::io::{ByteOrder as EndianByteOrder, EndianReader};
use crate::parsers::icc::parse_icc_profile_data;
use crate::parsers::jpeg::iptc_parser::{
    dataset_to_tag_name, decode_iptc_string, parse_all_iptc_records,
};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::parsers::xmp::rdf_parser::parse_xmp;
use crate::tag_db::lookup_tag_name;

const PSD_SIGNATURE: &[u8] = b"8BPS";

/// Image resource IDs
const IPTC_NAA_RECORD: u16 = 0x0404; // IPTC-NAA record
const EXIF_DATA_1: u16 = 0x0422; // EXIF data 1
const EXIF_DATA_3: u16 = 0x0423; // EXIF data 3
const XMP_DATA: u16 = 0x0424; // XMP metadata
const ICC_PROFILE: u16 = 0x040F; // ICC profile
const RESOLUTION_INFO: u16 = 0x03ED; // Resolution info
const PRINT_FLAGS: u16 = 0x03F1; // Print flags
const COPYRIGHT_FLAG: u16 = 0x040A; // Copyright flag

/// Parser for Adobe Photoshop (PSD) document files
///
/// Extracts metadata from PSD files including dimensions, color mode, channels,
/// bit depth, and embedded EXIF/IPTC/XMP data.
pub struct PSDParser;
/// Formats a PSD resolution the way ExifTool prints it.
///
/// The header stores a fixed-point value that is almost always whole, and
/// ExifTool prints it without a fractional part -- "72", not "72.00". A
/// hard-coded two decimals turned a correct number into a value mismatch.
fn format_psd_resolution(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

impl PSDParser {
    /// Verifies the PSD file signature ("8BPS")
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        Ok(header == PSD_SIGNATURE)
    }

    /// Reads the PSD file version number (1 for PSD, 2 for PSB)
    pub fn read_version(reader: &dyn FileReader) -> Result<u16> {
        if reader.size() < 6 {
            return Ok(0);
        }
        let version_bytes = reader.read(4, 2)?;
        // PSD uses big-endian byte order
        let version_reader = EndianReader::big_endian(version_bytes);
        Ok(version_reader.u16_at(0).unwrap_or(0))
    }

    /// Parse the PSD header (26 bytes)
    fn parse_header(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        if reader.size() < 26 {
            return Ok(());
        }

        let header = reader.read(0, 26)?;
        // PSD uses big-endian byte order
        let header_reader = EndianReader::big_endian(header);

        // Version (offset 4, 2 bytes)
        let version = header_reader.u16_at(4).unwrap_or(1);
        let format_name = if version == 1 { "PSD" } else { "PSB" };
        metadata.insert(
            "FileType".to_string(),
            TagValue::String(format_name.to_string()),
        );
        metadata.insert("PSDVersion".to_string(), TagValue::Integer(version as i64));

        // Channels (offset 12, 2 bytes)
        let channels = header_reader.u16_at(12).unwrap_or(0);
        metadata.insert(
            "Photoshop:NumChannels".to_string(),
            TagValue::Integer(channels as i64),
        );

        // Height (offset 14, 4 bytes)
        let height = header_reader.u32_at(14).unwrap_or(0);
        metadata.insert(
            "Photoshop:ImageHeight".to_string(),
            TagValue::Integer(height as i64),
        );

        // Width (offset 18, 4 bytes)
        let width = header_reader.u32_at(18).unwrap_or(0);
        metadata.insert(
            "Photoshop:ImageWidth".to_string(),
            TagValue::Integer(width as i64),
        );

        // Bit Depth (offset 22, 2 bytes)
        let depth = header_reader.u16_at(22).unwrap_or(0);
        metadata.insert(
            "Photoshop:BitDepth".to_string(),
            TagValue::Integer(depth as i64),
        );

        // Color Mode (offset 24, 2 bytes)
        let color_mode = header_reader.u16_at(24).unwrap_or(0);
        let color_mode_name = match color_mode {
            0 => "Bitmap",
            1 => "Grayscale",
            2 => "Indexed",
            3 => "RGB",
            4 => "CMYK",
            7 => "Multichannel",
            8 => "Duotone",
            9 => "Lab",
            _ => "Unknown",
        };
        metadata.insert(
            "Photoshop:ColorMode".to_string(),
            TagValue::String(color_mode_name.to_string()),
        );

        Ok(())
    }

    /// Parse Image Resources section
    fn parse_image_resources(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        if reader.size() < 34 {
            return Ok(());
        }

        // Color mode data length at offset 26
        let cmd_len_bytes = reader.read(26, 4)?;
        // PSD uses big-endian byte order
        let cmd_len_reader = EndianReader::big_endian(cmd_len_bytes);
        let color_mode_data_length = cmd_len_reader.u32_at(0).unwrap_or(0);

        // Image resources section starts after color mode data
        let resources_offset = 30 + color_mode_data_length as usize;

        if reader.size() < (resources_offset + 4) as u64 {
            return Ok(());
        }

        // Image resources length
        let irl_bytes = reader.read(resources_offset as u64, 4)?;
        let irl_reader = EndianReader::big_endian(irl_bytes);
        let resources_length = irl_reader.u32_at(0).unwrap_or(0) as usize;

        if resources_length == 0 || reader.size() < (resources_offset + 4 + resources_length) as u64
        {
            return Ok(());
        }

        // Read entire resources section
        let resources_data = reader.read((resources_offset + 4) as u64, resources_length)?;

        // Parse individual resources
        let mut pos = 0;
        while pos + 12 <= resources_data.len() {
            // Resource signature "8BIM"
            if &resources_data[pos..pos + 4] != b"8BIM" {
                break;
            }
            pos += 4;

            // Resource ID (2 bytes)
            let res_reader = EndianReader::big_endian(&resources_data[pos..]);
            let resource_id = res_reader.u16_at(0).unwrap_or(0);
            pos += 2;

            // Pascal string name (padded to even)
            let name_len = resources_data[pos] as usize;
            let padded_name_len = if (name_len + 1).is_multiple_of(2) {
                name_len + 1
            } else {
                name_len + 2
            };
            pos += padded_name_len;

            if pos + 4 > resources_data.len() {
                break;
            }

            // Resource data size (4 bytes)
            let size_reader = EndianReader::big_endian(&resources_data[pos..]);
            let data_size = size_reader.u32_at(0).unwrap_or(0) as usize;
            pos += 4;

            if pos + data_size > resources_data.len() {
                break;
            }

            let resource_data = &resources_data[pos..pos + data_size];

            // Process specific resources
            match resource_id {
                RESOLUTION_INFO => {
                    Self::parse_resolution_info(resource_data, metadata);
                }
                EXIF_DATA_1 | EXIF_DATA_3 => {
                    Self::parse_exif_data(resource_data, metadata);
                }
                COPYRIGHT_FLAG => {
                    if !resource_data.is_empty() && resource_data[0] != 0 {
                        metadata.insert(
                            "Copyrighted".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                }
                XMP_DATA => {
                    if let Ok(xmp_str) = std::str::from_utf8(resource_data) {
                        Self::parse_xmp_data(xmp_str, metadata);
                    }
                }
                ICC_PROFILE => {
                    metadata.insert(
                        "HasICCProfile".to_string(),
                        TagValue::String("Yes".to_string()),
                    );
                    // Parse ICC profile data
                    if let Ok(icc_tags) = parse_icc_profile_data(resource_data) {
                        for (key, value) in icc_tags {
                            metadata.insert(format!("ICC_Profile:{}", key), value);
                        }
                    }
                }
                IPTC_NAA_RECORD => {
                    Self::parse_iptc_data(resource_data, metadata);
                }
                _ => {}
            }

            // Pad to even boundary
            let padded_size = if data_size.is_multiple_of(2) {
                data_size
            } else {
                data_size + 1
            };
            pos += padded_size;
        }

        Ok(())
    }

    /// Parse resolution info resource
    fn parse_resolution_info(data: &[u8], metadata: &mut MetadataMap) {
        if data.len() < 16 {
            return;
        }

        // PSD uses big-endian byte order
        let res_reader = EndianReader::big_endian(data);

        // Horizontal resolution (fixed point 16.16)
        let h_res_fixed = res_reader.u32_at(0).unwrap_or(0);
        let h_res = h_res_fixed as f64 / 65536.0;

        // Resolution unit (offset 4, 2 bytes): 1=pixels/inch, 2=pixels/cm
        let res_unit = res_reader.u16_at(4).unwrap_or(1);
        let unit_name = if res_unit == 1 { "inch" } else { "cm" };

        // Vertical resolution (offset 8, fixed point 16.16)
        let v_res_fixed = res_reader.u32_at(8).unwrap_or(0);
        let v_res = v_res_fixed as f64 / 65536.0;

        metadata.insert(
            "Photoshop:XResolution".to_string(),
            TagValue::String(format_psd_resolution(h_res)),
        );
        metadata.insert(
            "Photoshop:YResolution".to_string(),
            TagValue::String(format_psd_resolution(v_res)),
        );
        metadata.insert(
            "ResolutionUnit".to_string(),
            TagValue::String(unit_name.to_string()),
        );
    }

    /// Parse embedded EXIF data
    fn parse_exif_data(data: &[u8], metadata: &mut MetadataMap) {
        if data.len() < 8 {
            return;
        }

        // Detect byte order
        let byte_order = match &data[0..2] {
            b"II" => ByteOrder::LittleEndian,
            b"MM" => ByteOrder::BigEndian,
            _ => return,
        };

        // Create EndianReader with appropriate byte order
        let endian_order = match byte_order {
            ByteOrder::LittleEndian => EndianByteOrder::Little,
            ByteOrder::BigEndian => EndianByteOrder::Big,
        };
        let tiff_reader = EndianReader::new(data, endian_order);

        // Verify TIFF magic
        let magic = tiff_reader.u16_at(2).unwrap_or(0);
        if magic != 0x002A {
            return;
        }

        // Get IFD0 offset
        let ifd0_offset = tiff_reader.u32_at(4).unwrap_or(0);

        // Create a BufferedReader from the TIFF data
        let reader = BufferedReader::from_bytes(data);

        // Parse IFD0
        if let Ok(entries) = parse_ifd(&reader, ifd0_offset as u64, byte_order) {
            for (tag_id, field_type, value_count, raw_bytes) in &entries {
                let tag_name = lookup_tag_name(*tag_id, "IFD0");
                let value = raw_bytes_to_tag_value(
                    raw_bytes.as_ref(),
                    *field_type,
                    *value_count,
                    *tag_id,
                    byte_order,
                );
                metadata.insert(tag_name, value);

                // Check for ExifIFD pointer
                if *tag_id == 0x8769 && raw_bytes.len() >= 4 {
                    let tag_reader = EndianReader::new(raw_bytes, endian_order);
                    let exif_offset = tag_reader.u32_at(0).unwrap_or(0);
                    if let Ok(exif_entries) = parse_ifd(&reader, exif_offset as u64, byte_order) {
                        for (exif_tag_id, exif_field_type, exif_value_count, exif_raw_bytes) in
                            &exif_entries
                        {
                            let exif_tag_name = lookup_tag_name(*exif_tag_id, "ExifIFD");
                            let value = raw_bytes_to_tag_value(
                                exif_raw_bytes.as_ref(),
                                *exif_field_type,
                                *exif_value_count,
                                *exif_tag_id,
                                byte_order,
                            );
                            metadata.insert(exif_tag_name, value);
                        }
                    }
                }
            }
        }

        // parse_ifd() returns only the entries in the requested directory.
        // Follow IFD0's next-directory pointer because embedded EXIF commonly
        // stores thumbnail tags, including Compression, in IFD1.
        if let Some(ifd1_offset) = next_ifd_offset(data, ifd0_offset, byte_order) {
            if ifd1_offset != 0 {
                if let Ok(entries) = parse_ifd(&reader, ifd1_offset as u64, byte_order) {
                    for (tag_id, field_type, value_count, raw_bytes) in &entries {
                        let tag_name = lookup_ifd1_tag_name(*tag_id);
                        let value = raw_bytes_to_tag_value(
                            raw_bytes.as_ref(),
                            *field_type,
                            *value_count,
                            *tag_id,
                            byte_order,
                        );
                        metadata.insert(tag_name, value);
                    }
                }
            }
        }
    }

    /// Extract metadata from XMP using the proper RDF parser.
    ///
    /// Camera Bits stores Photo Mechanic properties in its XMP namespace.  The
    /// generic RDF parser intentionally normalizes simple properties to the
    /// `XMP:` family, but ExifTool reports these four properties in the
    /// `PhotoMechanic` family. Move the existing values instead of inserting a
    /// parallel copy.
    fn parse_xmp_data(xmp: &str, metadata: &mut MetadataMap) {
        if let Ok(xmp_tags) = parse_xmp(xmp.as_bytes()) {
            for (tag_name, value) in xmp_tags {
                metadata.insert(tag_name, TagValue::String(value));
            }

            // Namespace URI from PhotoMechanic.pm's XMP namespace definition.
            // Scope the rewrite to this exact document namespace so unrelated
            // XMP properties with names such as Rotation aren't reclassified.
            if xmp.contains("http://ns.camerabits.com/photomechanic/1.0/") {
                for property in ["CropRight", "CropTop", "Rotation", "Tagged"] {
                    let xmp_name = format!("XMP:{property}");
                    if let Some(value) = metadata.remove(&xmp_name) {
                        let value = if property == "Tagged" {
                            match value {
                                TagValue::String(raw) if raw == "0" => {
                                    TagValue::String("No".to_string())
                                }
                                TagValue::String(raw) if raw == "1" => {
                                    TagValue::String("Yes".to_string())
                                }
                                other => other,
                            }
                        } else {
                            value
                        };
                        metadata.insert(format!("PhotoMechanic:{property}"), value);
                    }
                }
            }
        }
    }

    /// Parse IPTC data from image resource block
    fn parse_iptc_data(data: &[u8], metadata: &mut MetadataMap) {
        if let Ok(records) = parse_all_iptc_records(data) {
            for record in records {
                // Only process Application Record (record 2)
                if record.record_number == 2 {
                    let tag_name = dataset_to_tag_name(record.record_number, record.dataset_number);
                    let value = decode_iptc_string(&record.data);

                    // Use IPTC: prefix for tag names
                    let full_name = if tag_name.starts_with("IPTC:") {
                        tag_name
                    } else {
                        format!("IPTC:{}", tag_name)
                    };
                    metadata.insert(full_name, TagValue::String(value));
                } else if record.record_number == 1 {
                    // Record 1 is the envelope record - parse version
                    if record.dataset_number == 0 && record.data.len() >= 2 {
                        let version = u16::from_be_bytes([record.data[0], record.data[1]]);
                        metadata.insert(
                            "IPTC:ApplicationRecordVersion".to_string(),
                            TagValue::Integer(version as i64),
                        );
                    }
                }
            }
        }
    }
}

impl FormatParser for PSDParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid PSD signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert(
            "FileSize".to_string(),
            TagValue::Integer(reader.size() as i64),
        );

        // Parse header
        Self::parse_header(reader, &mut metadata)?;

        // Parse image resources (EXIF, XMP, etc.)
        Self::parse_image_resources(reader, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::PSD)
    }
}

/// Parses metadata from PSD files.
pub fn parse_psd_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = PSDParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Returns the offset of the IFD linked after `ifd_offset`.
///
/// A TIFF IFD consists of a two-byte entry count, twelve bytes per entry, and
/// then a four-byte offset to the next IFD.
fn next_ifd_offset(data: &[u8], ifd_offset: u32, byte_order: ByteOrder) -> Option<u32> {
    let endian_order = match byte_order {
        ByteOrder::LittleEndian => EndianByteOrder::Little,
        ByteOrder::BigEndian => EndianByteOrder::Big,
    };
    let reader = EndianReader::new(data, endian_order);
    let ifd_offset = ifd_offset as usize;
    let entry_count = reader.u16_at(ifd_offset)? as usize;
    let entries_size = entry_count.checked_mul(12)?;
    let next_offset_position = ifd_offset.checked_add(2)?.checked_add(entries_size)?;

    reader.u32_at(next_offset_position)
}

/// Looks up an IFD1 tag name, accounting for context-dependent EXIF aliases.
fn lookup_ifd1_tag_name(tag_id: u16) -> String {
    let database_name = lookup_tag_name(tag_id, "IFD1");

    // ExifTool names 0x0201 ThumbnailOffset when it occurs in IFD1. Keep the
    // group selected by the tag database rather than hard-coding an EXIF
    // prefix, since the same numeric ID has other names in other directories.
    if tag_id == 0x0201 {
        if let Some((group, _)) = database_name.rsplit_once(':') {
            return format!("{group}:ThumbnailOffset");
        }
    }

    // ExifTool names 0x0202 ThumbnailLength when it occurs in IFD1.
    if tag_id == 0x0202 {
        if let Some((group, _)) = database_name.rsplit_once(':') {
            return format!("{group}:ThumbnailLength");
        }
    }

    database_name
}

/// Converts raw bytes to TagValue
fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    _value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    use crate::parsers::common::exif_types::ExifType;

    // Create EndianReader with appropriate byte order
    let endian_order = match byte_order {
        ByteOrder::LittleEndian => EndianByteOrder::Little,
        ByteOrder::BigEndian => EndianByteOrder::Big,
    };
    let reader = EndianReader::new(bytes, endian_order);

    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            ExifType::Ascii => {
                let text = String::from_utf8_lossy(bytes);
                return TagValue::String(text.trim_end_matches('\0').to_string());
            }
            ExifType::Short if bytes.len() >= 2 => {
                let value = reader.u16_at(0).unwrap_or(0);

                // Image::ExifTool::Exif::Main Compression PrintConv.
                if tag_id == 0x0103 && value == 6 {
                    return TagValue::String("JPEG (old-style)".to_string());
                }

                return TagValue::Integer(value as i64);
            }
            ExifType::Long if bytes.len() >= 4 => {
                let value = reader.u32_at(0).unwrap_or(0);
                return TagValue::Integer(value as i64);
            }
            ExifType::Rational if bytes.len() >= 8 => {
                if let Some((num, den)) = reader.rational_at(0) {
                    if den == 1 {
                        return TagValue::Integer(num as i64);
                    }
                    return TagValue::Rational {
                        numerator: num as i32,
                        denominator: den as i32,
                    };
                }
            }
            ExifType::Undefined => {
                if tag_id == 0x9000 && bytes.len() >= 4 {
                    let version = String::from_utf8_lossy(&bytes[0..4]);
                    return TagValue::String(version.to_string());
                }
                return TagValue::Binary(bytes.to_vec());
            }
            _ => {}
        }
    }

    if bytes.iter().all(|&b| b.is_ascii() || b == 0) {
        let text = String::from_utf8_lossy(bytes);
        TagValue::String(text.trim_end_matches('\0').to_string())
    } else {
        TagValue::Binary(bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compression_from_embedded_exif_ifd1() {
        // Minimal little-endian TIFF:
        // - TIFF header points to an empty IFD0 at offset 8
        // - IFD0 points to IFD1 at offset 14
        // - IFD1 contains Compression (SHORT) = 6
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // Empty IFD0 followed by its next-IFD pointer.
        data[8..10].copy_from_slice(&0u16.to_le_bytes());
        data[10..14].copy_from_slice(&14u32.to_le_bytes());

        // IFD1 with one entry.
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16..18].copy_from_slice(&0x0103u16.to_le_bytes());
        data[18..20].copy_from_slice(&3u16.to_le_bytes()); // SHORT
        data[20..24].copy_from_slice(&1u32.to_le_bytes());
        data[24..26].copy_from_slice(&6u16.to_le_bytes());
        // Bytes 26..28 are inline-value padding; 28..32 is next IFD = 0.

        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&data, &mut metadata);

        let tag_name = lookup_tag_name(0x0103, "IFD1");
        assert_eq!(
            metadata.get(&tag_name),
            Some(&TagValue::String("JPEG (old-style)".to_string()))
        );
    }

    #[test]
    fn parses_thumbnail_offset_from_embedded_exif_ifd1() {
        // Minimal little-endian TIFF:
        // - TIFF header points to an empty IFD0 at offset 8
        // - IFD0 points to IFD1 at offset 14
        // - IFD1 contains JPEGInterchangeFormat (LONG) = 390, which ExifTool
        //   names ThumbnailOffset in this directory
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // Empty IFD0 followed by its next-IFD pointer.
        data[8..10].copy_from_slice(&0u16.to_le_bytes());
        data[10..14].copy_from_slice(&14u32.to_le_bytes());

        // IFD1 with one inline LONG entry.
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16..18].copy_from_slice(&0x0201u16.to_le_bytes());
        data[18..20].copy_from_slice(&4u16.to_le_bytes()); // LONG
        data[20..24].copy_from_slice(&1u32.to_le_bytes());
        data[24..28].copy_from_slice(&390u32.to_le_bytes());
        // Bytes 28..32 are the zero next-IFD pointer.

        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&data, &mut metadata);

        let tag_name = lookup_ifd1_tag_name(0x0201);
        assert!(tag_name.ends_with(":ThumbnailOffset"));
        assert_eq!(metadata.get(&tag_name), Some(&TagValue::Integer(390)));
    }

    #[test]
    fn parses_thumbnail_length_from_embedded_exif_ifd1() {
        // Ground truth, Exif.pm line 1295-1297: the FIRST variant of the 0x202
        // conditional list is `Name => 'ThumbnailLength'`, gated on
        // `$$self{DIR_NAME} eq 'IFD1'`. PSD-embedded EXIF reaches this code with
        // DIR_NAME == IFD1, so that variant is the one that matches.
        //
        // Measured 2026-07-26 on
        // /tmp/oxidex-exiftool-cache/combined-samples/Photoshop.psd:
        //   exiftool -G1 -a -s  ->  [IFD1] ThumbnailLength : 0
        //   oxidex (post-fix)   ->  IFD1:ThumbnailLength: 0
        //
        // The key is asserted as a LITERAL rather than via lookup_ifd1_tag_name()
        // so the test cannot pass by agreeing with whatever the function returns.
        // Deleting the 0x0202 branch makes the key fall back to "IFD1:0x0202"
        // and this test goes red.
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // Empty IFD0 followed by its next-IFD pointer.
        data[8..10].copy_from_slice(&0u16.to_le_bytes());
        data[10..14].copy_from_slice(&14u32.to_le_bytes());

        // IFD1 with one inline LONG entry: 0x0202 = 0, exactly as the sample
        // stores it (IFD1 entry #5, `- Tag 0x0202 (4 bytes, int32u[1])`).
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16..18].copy_from_slice(&0x0202u16.to_le_bytes());
        data[18..20].copy_from_slice(&4u16.to_le_bytes()); // LONG
        data[20..24].copy_from_slice(&1u32.to_le_bytes());
        data[24..28].copy_from_slice(&0u32.to_le_bytes());
        // Bytes 28..32 are the zero next-IFD pointer.

        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&data, &mut metadata);

        assert_eq!(
            metadata.get("IFD1:ThumbnailLength"),
            Some(&TagValue::Integer(0)),
            "0x0202 in IFD1 must be named ThumbnailLength (Exif.pm:1297)"
        );
        assert!(
            metadata.get("IFD1:0x0202").is_none(),
            "0x0202 must not also survive under its unnamed hex fallback"
        );
    }

    #[test]
    fn thumbnail_length_rename_is_confined_to_ifd1() {
        // BLIND-SPOT REGRESSION TEST. Photoshop.psd carries 0x0202 only in IFD1,
        // so no sample in the corpus exercises 0x0202 in any other directory. A
        // green "recheck-pass gaps=1->0" therefore says nothing about whether the
        // rename leaked into IFD0/ExifIFD -- which is exactly the hole that let
        // the TTF (%ttLang Spanish=12) and RAR (RAR5 host-OS 2/3/4) fabrications
        // ship on 2026-07-26 beside values the sample did happen to hit.
        //
        // ThumbnailLength is NOT 0x202's universal name. Exif.pm lists nine
        // conditional variants; outside IFD1 the same ID is PreviewImageLength
        // (DIR_NAME eq "MakerNotes", line ~1347), JpgFromRawLength (SubIFD /
        // IFD2, ~1368), or OtherImageLength (SubIFD1 / SubIFD2, ~1388). So
        // renaming 0x0202 unconditionally would replace real data with a wrong
        // tag name in every one of those directories.
        //
        // parse_exif_data routes IFD0 through lookup_tag_name(id, "IFD0") and
        // only IFD1 through lookup_ifd1_tag_name(), so this pins the scope of the
        // fix at the call site rather than trusting the branch to stay put.
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // IFD0 holding 0x0202 directly, with no IFD1 chained after it.
        data[8..10].copy_from_slice(&1u16.to_le_bytes());
        data[10..12].copy_from_slice(&0x0202u16.to_le_bytes());
        data[12..14].copy_from_slice(&4u16.to_le_bytes()); // LONG
        data[14..18].copy_from_slice(&1u32.to_le_bytes());
        data[18..22].copy_from_slice(&12345u32.to_le_bytes());
        // Bytes 22..26 are the zero next-IFD pointer.

        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&data, &mut metadata);

        assert!(
            metadata
                .iter()
                .all(|(name, _)| !name.ends_with(":ThumbnailLength")),
            "0x0202 outside IFD1 must not be renamed ThumbnailLength; got {:?}",
            metadata.iter().map(|(n, _)| n).collect::<Vec<_>>()
        );
        // Same guard for the sibling 0x0201 special case, which shares the
        // identical IFD1-only condition in Exif.pm (line 1149).
        assert!(
            metadata
                .iter()
                .all(|(name, _)| !name.ends_with(":ThumbnailOffset")),
            "0x0201 was never present; no ThumbnailOffset should appear"
        );
    }

    #[test]
    fn photo_mechanic_xmp_properties_use_photo_mechanic_group() {
        let xmp = br#"
            <x:xmpmeta xmlns:x="adobe:ns:meta/">
              <rdf:RDF
                xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                xmlns:photomechanic="http://ns.camerabits.com/photomechanic/1.0/">
                <rdf:Description rdf:about="">
                  <photomechanic:CropRight>890</photomechanic:CropRight>
                  <photomechanic:CropTop>618</photomechanic:CropTop>
                  <photomechanic:Rotation>180</photomechanic:Rotation>
                  <photomechanic:Tagged>1</photomechanic:Tagged>
                </rdf:Description>
              </rdf:RDF>
            </x:xmpmeta>
        "#;

        let mut metadata = MetadataMap::new();
        PSDParser::parse_xmp_data(
            std::str::from_utf8(xmp).unwrap_or_default(),
            &mut metadata,
        );

        assert_eq!(
            metadata.get("PhotoMechanic:CropRight"),
            Some(&TagValue::String("890".to_string()))
        );
        assert_eq!(
            metadata.get("PhotoMechanic:CropTop"),
            Some(&TagValue::String("618".to_string()))
        );
        assert_eq!(
            metadata.get("PhotoMechanic:Rotation"),
            Some(&TagValue::String("180".to_string()))
        );
        assert_eq!(
            metadata.get("PhotoMechanic:Tagged"),
            Some(&TagValue::String("Yes".to_string()))
        );
        assert!(metadata.get("XMP:Tagged").is_none());
    }
}
