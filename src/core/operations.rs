//! Metadata operations (Read/Write/Copy/Transform)
//!
//! This module defines core operations for metadata manipulation.
//! It orchestrates format detection, parser selection, and metadata extraction
//! following the hexagonal architecture pattern.

use super::{FileFormat, FileReader, MetadataMap, TagValue};
use crate::core::validation::validate_tag_value;
use crate::error::{ExifToolError, Result};
use crate::io::MMapReader;
use crate::parsers::format_detector::detect_format;
use crate::parsers::jpeg::segment_parser::parse_segments;
use crate::parsers::jpeg::xmp_parser::extract_xmp_from_segments;
use crate::parsers::pdf::parse_pdf_metadata;
use crate::parsers::png::parse_png_metadata;
use crate::parsers::quicktime::parse_quicktime_metadata;
use crate::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
use crate::tag_db::lookup_tag_name;
use crate::tag_db::tag_registry::get_tag_descriptor;
use crate::writers::atomic_writer::write_atomic;
use crate::writers::jpeg_writer::write_exif_to_jpeg;
use chrono;
use std::path::Path;

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
/// use exiftool_rs::core::operations::read_metadata;
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
    // Step 1: Open file with MMapReader for zero-copy access
    let reader = MMapReader::new(path)?;

    // Step 2: Detect format via magic bytes
    let format = detect_format(&reader)?;

    // Step 3: Route to appropriate parser based on detected format
    match format {
        FileFormat::JPEG => parse_jpeg_metadata(&reader),
        FileFormat::TIFF => parse_tiff_metadata(&reader),
        FileFormat::PNG => {
            // PNG parser returns Result<MetadataMap, String>, need to convert
            parse_png_metadata(&reader)
                .map_err(|e| ExifToolError::parse_error(format!("PNG parse error: {}", e)))
        }
        FileFormat::PDF => parse_pdf_metadata(&reader),
        FileFormat::QuickTime => {
            // QuickTime parser returns Result<MetadataMap, String>, need to convert
            parse_quicktime_metadata(&reader)
                .map_err(|e| ExifToolError::parse_error(format!("QuickTime parse error: {}", e)))
        }
        _ => Err(ExifToolError::unsupported_format(format!(
            "Format {:?} not yet supported in this iteration",
            format
        ))),
    }
}

/// Parses metadata from a JPEG file.
///
/// JPEG files contain metadata in APP1 segments with EXIF data.
/// This function:
/// 1. Parses JPEG segment structure
/// 2. Locates APP1 segments (marker 0xFFE1)
/// 3. Extracts EXIF data from APP1 segments
/// 4. Parses TIFF IFD structure within EXIF data
///
/// # EXIF Structure in JPEG APP1
///
/// ```text
/// Bytes 0-5:   "Exif\0\0"  (6-byte EXIF header)
/// Bytes 6-7:   Byte order  (0x4949="II" little-endian, 0x4D4D="MM" big-endian)
/// Bytes 8-9:   Magic 42    (0x002A in detected byte order)
/// Bytes 10-13: IFD offset  (4 bytes, relative to byte 6, TIFF data start)
/// At offset:   IFD data
/// ```
fn parse_jpeg_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    // Parse JPEG segment structure
    let segments = parse_segments(reader)?;

    let mut metadata = MetadataMap::new();

    // Process APP0 segments for JFIF metadata
    for segment in segments.iter().filter(|s| s.marker == 0xFFE0) {
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
            let x_density = u16::from_be_bytes([segment.data[8], segment.data[9]]);
            let y_density = u16::from_be_bytes([segment.data[10], segment.data[11]]);

            // Add JFIF tags to metadata
            metadata.insert(
                "JFIF:JFIFVersion".to_string(),
                TagValue::Float(version_major as f64 + version_minor as f64 / 100.0),
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

            metadata.insert(
                "JFIF:XResolution".to_string(),
                TagValue::Integer(x_density as i64),
            );

            metadata.insert(
                "JFIF:YResolution".to_string(),
                TagValue::Integer(y_density as i64),
            );
        }
    }

    // Find all APP1 segments (EXIF/XMP)
    let app1_segments: Vec<_> = segments.iter().filter(|s| s.is_app1()).collect();

    if app1_segments.is_empty() && metadata.is_empty() {
        // No APP1 or JFIF segments found - return empty metadata
        return Ok(MetadataMap::new());
    }

    // Process each APP1 segment
    for segment in app1_segments {
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

            // Read IFD offset from bytes 4-7 (relative to TIFF data start)
            let ifd_offset = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([tiff_data[4], tiff_data[5], tiff_data[6], tiff_data[7]])
                }
            } as u64;

            // Create a sub-reader for TIFF data
            // We need to create a wrapper that adjusts offsets to be relative to TIFF start
            let tiff_offset = segment.offset + 10; // Segment offset + marker(2) + length(2) + "Exif\0\0"(6)
            let tiff_reader = TiffSubReader::new(reader, tiff_offset);

            // Parse IFD structure
            match parse_ifd(&tiff_reader, ifd_offset, byte_order) {
                Ok(tags) => {
                    // Track sub-IFD offsets
                    let mut exif_ifd_offset = None;
                    let mut gps_ifd_offset = None;

                    // Convert raw tag data to MetadataMap entries
                    for (tag_id, field_type, value_count, raw_bytes) in &tags {
                        // Check for EXIF Sub-IFD pointer (tag 0x8769)
                        if *tag_id == 0x8769 && raw_bytes.len() >= 4 {
                            let offset = match byte_order {
                                ByteOrder::LittleEndian => u32::from_le_bytes([
                                    raw_bytes[0],
                                    raw_bytes[1],
                                    raw_bytes[2],
                                    raw_bytes[3],
                                ]),
                                ByteOrder::BigEndian => u32::from_be_bytes([
                                    raw_bytes[0],
                                    raw_bytes[1],
                                    raw_bytes[2],
                                    raw_bytes[3],
                                ]),
                            };
                            exif_ifd_offset = Some(offset as u64);
                            continue; // Don't add the pointer tag to metadata
                        }

                        // Check for GPS Sub-IFD pointer (tag 0x8825)
                        if *tag_id == 0x8825 && raw_bytes.len() >= 4 {
                            let offset = match byte_order {
                                ByteOrder::LittleEndian => u32::from_le_bytes([
                                    raw_bytes[0],
                                    raw_bytes[1],
                                    raw_bytes[2],
                                    raw_bytes[3],
                                ]),
                                ByteOrder::BigEndian => u32::from_be_bytes([
                                    raw_bytes[0],
                                    raw_bytes[1],
                                    raw_bytes[2],
                                    raw_bytes[3],
                                ]),
                            };
                            gps_ifd_offset = Some(offset as u64);
                            continue; // Don't add the pointer tag to metadata
                        }

                        // Convert tag ID to tag name (IFD0 for main JPEG EXIF)
                        let tag_name = lookup_tag_name(*tag_id, "IFD0");

                        // Convert raw bytes to TagValue
                        let tag_value = raw_bytes_to_tag_value(
                            raw_bytes,
                            *field_type,
                            *value_count,
                            *tag_id,
                            byte_order,
                        );

                        metadata.insert(tag_name, tag_value);
                    }

                    // Parse EXIF Sub-IFD if present
                    if let Some(offset) = exif_ifd_offset {
                        if let Ok(exif_tags) = parse_ifd(&tiff_reader, offset, byte_order) {
                            for (tag_id, field_type, value_count, raw_bytes) in exif_tags {
                                let tag_name = lookup_tag_name(tag_id, "ExifIFD");
                                let tag_value = raw_bytes_to_tag_value(
                                    &raw_bytes,
                                    field_type,
                                    value_count,
                                    tag_id,
                                    byte_order,
                                );
                                metadata.insert(tag_name, tag_value);
                            }
                        }
                    }

                    // Parse GPS Sub-IFD if present
                    if let Some(offset) = gps_ifd_offset {
                        if let Ok(gps_tags) = parse_ifd(&tiff_reader, offset, byte_order) {
                            for (tag_id, field_type, value_count, raw_bytes) in gps_tags {
                                let tag_name = lookup_tag_name(tag_id, "GPS");
                                let tag_value = raw_bytes_to_tag_value(
                                    &raw_bytes,
                                    field_type,
                                    value_count,
                                    tag_id,
                                    byte_order,
                                );
                                metadata.insert(tag_name, tag_value);
                            }
                        }
                    }
                }
                Err(e) => {
                    // Log error but continue processing (don't fail entire read)
                    eprintln!("Warning: Failed to parse EXIF IFD: {}", e);
                }
            }
        }
    }

    // Extract XMP metadata from APP1 segments
    match extract_xmp_from_segments(&segments) {
        Ok(xmp_tags) => {
            // Add all XMP tags to metadata
            for (tag_name, value) in xmp_tags {
                // Try to parse as integer first, then as float, otherwise keep as string
                let tag_value = if let Ok(int_val) = value.parse::<i64>() {
                    TagValue::Integer(int_val)
                } else if let Ok(float_val) = value.parse::<f64>() {
                    TagValue::Float(float_val)
                } else {
                    TagValue::String(value)
                };
                metadata.insert(tag_name, tag_value);
            }
        }
        Err(e) => {
            // Log error but continue processing (don't fail entire read)
            eprintln!("Warning: Failed to parse XMP: {}", e);
        }
    }

    Ok(metadata)
}

/// Parses metadata from a TIFF file.
///
/// TIFF files begin with a TIFF header followed by IFD structures.
/// This function:
/// 1. Reads TIFF header (byte order, magic number, IFD offset)
/// 2. Parses all IFD structures (IFD0, IFD1, IFD2, ...)
/// 3. Parses sub-IFDs (EXIF, GPS, Interoperability)
/// 4. Converts raw tag data to MetadataMap
fn parse_tiff_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
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
    let magic = match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([header[2], header[3]]),
        ByteOrder::BigEndian => u16::from_be_bytes([header[2], header[3]]),
    };

    if magic != 42 {
        return Err(ExifToolError::parse_error(format!(
            "Invalid TIFF magic number: expected 42, got {}",
            magic
        )));
    }

    // Read first IFD offset from bytes 4-7
    let first_ifd_offset = match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes([header[4], header[5], header[6], header[7]]),
        ByteOrder::BigEndian => u32::from_be_bytes([header[4], header[5], header[6], header[7]]),
    } as u64;

    // Parse all IFDs in the chain (IFD0, IFD1, IFD2, ...)
    let mut metadata = MetadataMap::new();
    let mut ifd_offset = first_ifd_offset;
    let mut ifd_index = 0;

    while ifd_offset != 0 {
        // Determine IFD name based on index
        let ifd_name = match ifd_index {
            0 => "IFD0",
            1 => "IFD1",
            2 => "IFD2",
            3 => "IFD3",
            n => {
                // For IFD4 and beyond, just use IFDX format
                eprintln!("Warning: Found IFD{} which is unusual", n);
                "IFD0" // Fallback to IFD0 for unusual cases
            }
        };

        // Parse this IFD
        let tags = parse_ifd(reader, ifd_offset, byte_order)?;

        // Track sub-IFD offsets
        let mut exif_ifd_offset = None;
        let mut gps_ifd_offset = None;

        // Convert tags to metadata
        for (tag_id, field_type, value_count, raw_bytes) in &tags {
            // Check for EXIF Sub-IFD pointer (tag 0x8769)
            if *tag_id == 0x8769 && raw_bytes.len() >= 4 {
                let offset = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                    }
                };
                exif_ifd_offset = Some(offset as u64);
                continue; // Don't add the pointer tag to metadata
            }

            // Check for GPS Sub-IFD pointer (tag 0x8825)
            if *tag_id == 0x8825 && raw_bytes.len() >= 4 {
                let offset = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([raw_bytes[0], raw_bytes[1], raw_bytes[2], raw_bytes[3]])
                    }
                };
                gps_ifd_offset = Some(offset as u64);
                continue; // Don't add the pointer tag to metadata
            }

            // Convert tag to metadata
            let tag_name = lookup_tag_name(*tag_id, ifd_name);
            let tag_value =
                raw_bytes_to_tag_value(raw_bytes, *field_type, *value_count, *tag_id, byte_order);
            metadata.insert(tag_name, tag_value);
        }

        // Parse EXIF Sub-IFD if present
        if let Some(offset) = exif_ifd_offset {
            if let Ok(exif_tags) = parse_ifd(reader, offset, byte_order) {
                for (tag_id, field_type, value_count, raw_bytes) in exif_tags {
                    let tag_name = lookup_tag_name(tag_id, "ExifIFD");
                    let tag_value = raw_bytes_to_tag_value(
                        &raw_bytes,
                        field_type,
                        value_count,
                        tag_id,
                        byte_order,
                    );
                    metadata.insert(tag_name, tag_value);
                }
            }
        }

        // Parse GPS Sub-IFD if present
        if let Some(offset) = gps_ifd_offset {
            if let Ok(gps_tags) = parse_ifd(reader, offset, byte_order) {
                for (tag_id, field_type, value_count, raw_bytes) in gps_tags {
                    let tag_name = lookup_tag_name(tag_id, "GPS");
                    let tag_value = raw_bytes_to_tag_value(
                        &raw_bytes,
                        field_type,
                        value_count,
                        tag_id,
                        byte_order,
                    );
                    metadata.insert(tag_name, tag_value);
                }
            }
        }

        // Read next IFD offset (located after all tag entries)
        // Each IFD has: 2 bytes (entry count) + 12 * entry_count + 4 bytes (next offset)
        let entry_count = tags.len();
        let next_offset_location = ifd_offset + 2 + (entry_count as u64 * 12);

        if next_offset_location + 4 > reader.size() {
            // Can't read next offset, end of chain
            break;
        }

        let next_offset_bytes = reader.read(next_offset_location, 4)?;
        let next_offset = match byte_order {
            ByteOrder::LittleEndian => u32::from_le_bytes([
                next_offset_bytes[0],
                next_offset_bytes[1],
                next_offset_bytes[2],
                next_offset_bytes[3],
            ]),
            ByteOrder::BigEndian => u32::from_be_bytes([
                next_offset_bytes[0],
                next_offset_bytes[1],
                next_offset_bytes[2],
                next_offset_bytes[3],
            ]),
        } as u64;

        ifd_offset = next_offset;
        ifd_index += 1;

        // Safety check: prevent infinite loops
        if ifd_index > 10 {
            eprintln!("Warning: More than 10 IFDs found, stopping to prevent infinite loop");
            break;
        }
    }

    Ok(metadata)
}

/// Checks if a string matches the EXIF DateTime format (YYYY:MM:DD HH:MM:SS).
///
/// EXIF DateTime format: "2025:01:15 10:30:00" (19 characters)
/// - 4 digits for year
/// - 2 colons separating year:month:day
/// - 1 space between date and time
/// - 2 colons separating hour:minute:second
fn is_datetime_string(s: &str) -> bool {
    // EXIF DateTime format: YYYY:MM:DD HH:MM:SS (19 characters)
    s.len() == 19
        && s.chars().filter(|&c| c == ':').count() == 4
        && s.chars().filter(|&c| c == ' ').count() == 1
        && s.chars().nth(4) == Some(':')
        && s.chars().nth(7) == Some(':')
        && s.chars().nth(10) == Some(' ')
        && s.chars().nth(13) == Some(':')
        && s.chars().nth(16) == Some(':')
}

/// Parses an EXIF DateTime string into a chrono::DateTime<Utc>.
///
/// EXIF format: "2025:01:15 10:30:00" (YYYY:MM:DD HH:MM:SS)
fn parse_exif_datetime(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    use chrono::NaiveDateTime;

    // EXIF format: "2025:01:15 10:30:00"
    let naive = NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S")
        .map_err(|e| ExifToolError::parse_error(format!("Invalid DateTime: {}", e)))?;

    Ok(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        naive,
        chrono::Utc,
    ))
}

/// Computes the Greatest Common Divisor (GCD) of two unsigned integers using Euclid's algorithm.
///
/// Used for simplifying fractions when displaying RATIONAL values.
///
/// # Arguments
///
/// * `a` - First number
/// * `b` - Second number
///
/// # Returns
///
/// The GCD of a and b
fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Converts raw bytes from IFD to a TagValue.
///
/// This function interprets raw bytes according to the EXIF field type,
/// converting them to the appropriate TagValue variant.
///
/// # Arguments
///
/// * `bytes` - The raw bytes to convert
/// * `field_type` - The EXIF field type (from IFD entry)
/// * `value_count` - The number of values (from IFD entry)
/// * `tag_id` - The tag ID (for enum mapping and special handling)
/// * `byte_order` - The byte order for interpreting multi-byte values
///
/// # Returns
///
/// A TagValue representing the data
fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    use crate::parsers::common::exif_types::ExifType;
    use crate::parsers::tiff::tiff_enums::tiff_enum_to_string;

    // GPS-specific tag IDs
    const GPS_LATITUDE: u16 = 0x0002;
    const GPS_LONGITUDE: u16 = 0x0004;
    const GPS_ALTITUDE: u16 = 0x0006;
    const GPS_VERSION_ID: u16 = 0x0000;
    const GPS_TIME_STAMP: u16 = 0x0007;
    const GPS_DEST_LATITUDE: u16 = 0x0014;
    const GPS_DEST_LONGITUDE: u16 = 0x0016;

    // ExifVersion tag ID
    const EXIF_VERSION: u16 = 0x9000;

    // ComponentsConfiguration tag ID
    const COMPONENTS_CONFIGURATION: u16 = 0x9101;

    // Rational tags that should be formatted as fractions
    const EXPOSURE_TIME: u16 = 0x829A;
    const F_NUMBER: u16 = 0x829D;
    const APERTURE_VALUE: u16 = 0x9202;
    const SHUTTER_SPEED_VALUE: u16 = 0x9201;
    const MAX_APERTURE_VALUE: u16 = 0x9205;
    const FOCAL_LENGTH: u16 = 0x920A;

    // Try to convert field_type to ExifType
    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            // RATIONAL (type 5): two 32-bit unsigned integers (numerator/denominator)
            ExifType::Rational if bytes.len() >= 8 => {
                // Check if this is an array of rationals (count > 1)
                if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
                    // Special handling for GPS coordinates (3 rationals: degrees, minutes, seconds)
                    if matches!(tag_id, GPS_LATITUDE | GPS_LONGITUDE | GPS_DEST_LATITUDE | GPS_DEST_LONGITUDE) && value_count == 3 {
                        let mut dms = Vec::new();
                        for i in 0..3 {
                            let offset = i * 8;
                            let numerator = match byte_order {
                                ByteOrder::LittleEndian => u32::from_le_bytes([
                                    bytes[offset],
                                    bytes[offset + 1],
                                    bytes[offset + 2],
                                    bytes[offset + 3],
                                ]),
                                ByteOrder::BigEndian => u32::from_be_bytes([
                                    bytes[offset],
                                    bytes[offset + 1],
                                    bytes[offset + 2],
                                    bytes[offset + 3],
                                ]),
                            };
                            let denominator = match byte_order {
                                ByteOrder::LittleEndian => u32::from_le_bytes([
                                    bytes[offset + 4],
                                    bytes[offset + 5],
                                    bytes[offset + 6],
                                    bytes[offset + 7],
                                ]),
                                ByteOrder::BigEndian => u32::from_be_bytes([
                                    bytes[offset + 4],
                                    bytes[offset + 5],
                                    bytes[offset + 6],
                                    bytes[offset + 7],
                                ]),
                            };
                            if denominator != 0 {
                                dms.push(numerator as f64 / denominator as f64);
                            } else {
                                dms.push(numerator as f64);
                            }
                        }
                        // Format as DMS: "37 deg 46' 33.24""
                        let formatted = format!("{} deg {}' {:.2}\"",
                            dms[0] as i32,
                            dms[1] as i32,
                            dms[2]
                        );
                        return TagValue::new_string(formatted);
                    }

                    // Parse array of rationals and format as space-separated decimals
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 8;
                        let numerator = match byte_order {
                            ByteOrder::LittleEndian => u32::from_le_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                            ByteOrder::BigEndian => u32::from_be_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                        };
                        let denominator = match byte_order {
                            ByteOrder::LittleEndian => u32::from_le_bytes([
                                bytes[offset + 4],
                                bytes[offset + 5],
                                bytes[offset + 6],
                                bytes[offset + 7],
                            ]),
                            ByteOrder::BigEndian => u32::from_be_bytes([
                                bytes[offset + 4],
                                bytes[offset + 5],
                                bytes[offset + 6],
                                bytes[offset + 7],
                            ]),
                        };
                        if denominator != 0 {
                            values.push(numerator as f64 / denominator as f64);
                        } else {
                            values.push(numerator as f64);
                        }
                    }
                    // Format as space-separated string to match Perl ExifTool
                    let formatted = values
                        .iter()
                        .map(|v| format!("{:.10}", v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    return TagValue::new_string(formatted);
                }

                // Single rational value
                let numerator = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                };
                let denominator = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
                    }
                };

                // Special handling for GPS Altitude
                if tag_id == GPS_ALTITUDE && denominator != 0 {
                    let value = numerator as f64 / denominator as f64;
                    // Format without decimal point if it's a whole number
                    if value.fract() == 0.0 {
                        return TagValue::new_string(format!("{} m", value as i32));
                    } else {
                        return TagValue::new_string(format!("{:.1} m", value));
                    }
                }

                // Special handling for ExposureTime - format as fraction string
                if tag_id == EXPOSURE_TIME && denominator != 0 {
                    // Simplify fraction using GCD
                    let gcd = gcd(numerator, denominator);
                    let simplified_num = numerator / gcd;
                    let simplified_den = denominator / gcd;
                    // Only format as fraction if denominator > 1
                    if simplified_den > 1 {
                        return TagValue::new_string(format!("{}/{}", simplified_num, simplified_den));
                    }
                }

                return TagValue::new_rational(numerator as i32, denominator as i32);
            }

            // SRATIONAL (type 10): two 32-bit signed integers (numerator/denominator)
            ExifType::SRational if bytes.len() >= 8 => {
                // Check if this is an array
                if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 8;
                        let numerator = match byte_order {
                            ByteOrder::LittleEndian => i32::from_le_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                            ByteOrder::BigEndian => i32::from_be_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                        };
                        let denominator = match byte_order {
                            ByteOrder::LittleEndian => i32::from_le_bytes([
                                bytes[offset + 4],
                                bytes[offset + 5],
                                bytes[offset + 6],
                                bytes[offset + 7],
                            ]),
                            ByteOrder::BigEndian => i32::from_be_bytes([
                                bytes[offset + 4],
                                bytes[offset + 5],
                                bytes[offset + 6],
                                bytes[offset + 7],
                            ]),
                        };
                        if denominator != 0 {
                            values.push(numerator as f64 / denominator as f64);
                        } else {
                            values.push(numerator as f64);
                        }
                    }
                    let formatted = values
                        .iter()
                        .map(|v| format!("{:.10}", v))
                        .collect::<Vec<_>>()
                        .join(" ");
                    return TagValue::new_string(formatted);
                }

                let numerator = match byte_order {
                    ByteOrder::LittleEndian => {
                        i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                    ByteOrder::BigEndian => {
                        i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                };
                let denominator = match byte_order {
                    ByteOrder::LittleEndian => {
                        i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
                    }
                    ByteOrder::BigEndian => {
                        i32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
                    }
                };
                return TagValue::new_rational(numerator, denominator);
            }

            // SHORT (type 3): unsigned 16-bit integers
            ExifType::Short if bytes.len() >= 2 => {
                // Handle array of shorts
                if value_count > 1 && bytes.len() >= (value_count as usize * 2) {
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 2;
                        let value = match byte_order {
                            ByteOrder::LittleEndian => {
                                u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
                            }
                            ByteOrder::BigEndian => {
                                u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
                            }
                        };
                        values.push(value.to_string());
                    }
                    return TagValue::new_string(values.join(" "));
                }

                // Handle single SHORT value
                let value = match byte_order {
                    ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
                    ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
                } as i64;

                // Try to convert to enum string if applicable
                if let Some(enum_str) = tiff_enum_to_string(tag_id, value) {
                    return TagValue::new_string(enum_str);
                }

                return TagValue::new_integer(value);
            }

            // LONG (type 4): unsigned 32-bit integers
            ExifType::Long if bytes.len() >= 4 => {
                // Handle array of longs
                if value_count > 1 && bytes.len() >= (value_count as usize * 4) {
                    let mut values = Vec::new();
                    for i in 0..value_count as usize {
                        let offset = i * 4;
                        let value = match byte_order {
                            ByteOrder::LittleEndian => u32::from_le_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                            ByteOrder::BigEndian => u32::from_be_bytes([
                                bytes[offset],
                                bytes[offset + 1],
                                bytes[offset + 2],
                                bytes[offset + 3],
                            ]),
                        };
                        values.push(value.to_string());
                    }
                    return TagValue::new_string(values.join(" "));
                }

                // Handle single LONG value
                let value = match byte_order {
                    ByteOrder::LittleEndian => {
                        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                    ByteOrder::BigEndian => {
                        u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                    }
                } as i64;

                // Try to convert to enum string if applicable
                if let Some(enum_str) = tiff_enum_to_string(tag_id, value) {
                    return TagValue::new_string(enum_str);
                }

                return TagValue::new_integer(value);
            }

            // ASCII (type 2): null-terminated string
            ExifType::Ascii => {
                let s = String::from_utf8_lossy(bytes);
                let s = s.trim_end_matches('\0');
                if !s.is_empty() {
                    // Check if this is a DateTime string
                    if is_datetime_string(s) {
                        if let Ok(dt) = parse_exif_datetime(s) {
                            return TagValue::DateTime(dt);
                        }
                    }
                    return TagValue::new_string(s.to_string());
                }
                return TagValue::new_string(String::new());
            }

            // BYTE (type 1) and UNDEFINED (type 7): special handling for specific tags
            ExifType::Byte | ExifType::Undefined => {
                // GPS Version ID (4 bytes: major.minor.rev.0)
                if tag_id == GPS_VERSION_ID && bytes.len() >= 4 {
                    return TagValue::new_string(format!("{}.{}.{}.{}",
                        bytes[0], bytes[1], bytes[2], bytes[3]
                    ));
                }

                // Exif Version (4 bytes: ASCII "0232")
                if tag_id == EXIF_VERSION && bytes.len() >= 4 {
                    // ExifVersion is stored as ASCII bytes
                    let version = String::from_utf8_lossy(&bytes[0..4]);
                    return TagValue::new_string(version.to_string());
                }

                // ComponentsConfiguration (4 bytes with component IDs)
                if tag_id == COMPONENTS_CONFIGURATION && bytes.len() >= 4 {
                    let component_names = bytes.iter().take(4).map(|&b| match b {
                        0 => "-",
                        1 => "Y",
                        2 => "Cb",
                        3 => "Cr",
                        4 => "R",
                        5 => "G",
                        6 => "B",
                        _ => "?",
                    }).collect::<Vec<_>>();
                    return TagValue::new_string(component_names.join(", "));
                }

                // Fall through to heuristic conversion
            }

            _ => {
                // Fall through to heuristic conversion below
            }
        }
    }

    // Fallback heuristic conversion for unknown types or when type-specific logic doesn't apply
    // Try to interpret as integer first (if 2 or 4 bytes)
    if bytes.len() == 2 {
        let value = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
        } as i64;
        // Try to map to enum string
        if let Some(enum_str) = tiff_enum_to_string(tag_id, value) {
            return TagValue::new_string(enum_str);
        }
        return TagValue::new_integer(value);
    } else if bytes.len() == 4 {
        // Check if it looks like a string (4-character ASCII string like "EOS\0")
        // A string should have no null bytes except possibly at the end
        let null_count = bytes.iter().filter(|&&b| b == 0).count();
        let has_printable = bytes.iter().any(|&b| (32..=126).contains(&b));

        // If it has multiple nulls or no printable chars, treat as integer
        if null_count > 1 || !has_printable {
            let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64;
            return TagValue::new_integer(value);
        }

        // If all bytes are printable ASCII (and maybe one trailing null), treat as string
        if bytes.iter().all(|&b| (32..=126).contains(&b) || b == 0) {
            let s = String::from_utf8_lossy(bytes);
            let s = s.trim_end_matches('\0');
            if !s.is_empty() && s.len() >= 3 {
                // Likely a short string like "EOS\0"
                return TagValue::new_string(s.to_string());
            }
        }

        // Otherwise treat as integer
        let value = match byte_order {
            ByteOrder::LittleEndian => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            ByteOrder::BigEndian => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        } as i64;
        // Try to map to enum string
        if let Some(enum_str) = tiff_enum_to_string(tag_id, value) {
            return TagValue::new_string(enum_str);
        }
        return TagValue::new_integer(value);
    }

    // Try to interpret as ASCII string (null-terminated)
    if bytes
        .iter()
        .all(|&b| (32..=126).contains(&b) || b == 0 || b == b'\n' || b == b'\r' || b == b'\t')
    {
        // Convert to string, removing null terminator
        let s = String::from_utf8_lossy(bytes);
        let s = s.trim_end_matches('\0');
        if !s.is_empty() {
            // Check if this is a DateTime string (YYYY:MM:DD HH:MM:SS format)
            if is_datetime_string(s) {
                // Parse and return as DateTime type
                if let Ok(dt) = parse_exif_datetime(s) {
                    return TagValue::DateTime(dt);
                }
            }
            return TagValue::new_string(s.to_string());
        }
    }

    // Fallback: store as binary
    TagValue::new_binary(bytes.to_vec())
}

/// A FileReader wrapper that adjusts offsets to be relative to a base offset.
///
/// This is used to create a "view" into the file where offset 0 corresponds
/// to a specific position in the original file. Needed for parsing TIFF data
/// embedded within JPEG segments.
struct TiffSubReader<'a> {
    reader: &'a dyn FileReader,
    base_offset: u64,
}

impl<'a> TiffSubReader<'a> {
    fn new(reader: &'a dyn FileReader, base_offset: u64) -> Self {
        Self {
            reader,
            base_offset,
        }
    }
}

impl<'a> FileReader for TiffSubReader<'a> {
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        // Adjust offset to be relative to base
        self.reader.read(self.base_offset + offset, length)
    }

    fn size(&self) -> u64 {
        // Return size relative to base (remaining size from base to end)
        let total_size = self.reader.size();
        total_size.saturating_sub(self.base_offset)
    }
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
/// use exiftool_rs::core::operations::{read_metadata, write_metadata};
/// use exiftool_rs::core::tag_value::TagValue;
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
/// - Type matching (String, Integer, Float, Rational, etc.)
/// - Value constraints (e.g., Rational denominator != 0)
///
/// Tags not in the registry are skipped during validation (allows custom tags).
pub fn write_metadata(path: &Path, metadata: &MetadataMap) -> Result<()> {
    // PHASE 1: VALIDATION (fail fast before any file operations)
    // Iterate through all tags and validate each one against its descriptor
    for (tag_name, tag_value) in metadata.iter() {
        // Look up tag descriptor in registry
        if let Some(descriptor) = get_tag_descriptor(tag_name) {
            // Validate that the tag value matches the expected type
            validate_tag_value(descriptor, tag_value)?;
        }
        // If tag is not in registry, skip validation (allows custom/rare tags)
    }

    // PHASE 2: READ ORIGINAL FILE
    // Open file with MMapReader for zero-copy access
    let reader = MMapReader::new(path)?;

    // PHASE 3: DETECT FORMAT
    let format = detect_format(&reader)?;

    // PHASE 4: SERIALIZE WITH APPROPRIATE WRITER
    let serialized_bytes = match format {
        FileFormat::JPEG => {
            // Use JPEG writer to serialize metadata
            write_exif_to_jpeg(&reader, metadata)?
        }
        FileFormat::TIFF => {
            // TIFF writer not yet implemented (will be in I3.T7)
            return Err(ExifToolError::unsupported_format(
                "TIFF write operations are not yet supported in this iteration",
            ));
        }
        _ => {
            return Err(ExifToolError::unsupported_format(format!(
                "Write operations for format {:?} are not supported",
                format
            )));
        }
    };

    // PHASE 5: ATOMIC WRITE
    // Write serialized bytes to file using atomic temp-file-and-rename pattern
    write_atomic(path, &serialized_bytes)?;

    Ok(())
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
/// use exiftool_rs::core::operations::modify_tag;
/// use exiftool_rs::core::tag_value::TagValue;
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
    metadata.insert(tag_name, new_value);

    // Step 3: Write all metadata back to file
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
/// use exiftool_rs::core::operations::copy_metadata;
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
    for (tag_name, tag_value) in source_metadata.iter() {
        // Check if this tag should be copied (if filter is specified)
        let should_copy = tags.is_none_or(|filter| filter.contains(tag_name));

        if should_copy {
            // Insert tag into destination (merges with existing, preserving others)
            dest_metadata.insert(tag_name.clone(), tag_value.clone());
        }
    }

    // Step 4: Write merged metadata back to destination file
    write_metadata(dest, &dest_metadata)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    /// Test implementation of FileReader for unit testing
    struct TestReader {
        data: Vec<u8>,
    }

    impl TestReader {
        fn new(data: Vec<u8>) -> Self {
            Self { data }
        }
    }

    impl FileReader for TestReader {
        fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
            let start = offset as usize;
            let end = start + length;

            if end > self.data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "read beyond end of data",
                ));
            }

            Ok(&self.data[start..end])
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
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
        assert_eq!(lookup_tag_name(0xFFFF, "IFD0"), "IFD0:0xFFFF");
        assert_eq!(lookup_tag_name(0x1234, "GPS"), "GPS:0x1234");
    }

    #[test]
    fn test_raw_bytes_to_tag_value_string() {
        use crate::parsers::tiff::ifd_parser::ByteOrder;
        let bytes = b"Canon\0";
        let value = raw_bytes_to_tag_value(bytes, 2, 1, 0, ByteOrder::LittleEndian); // Type 2 = ASCII
        assert_eq!(value.as_string(), Some("Canon"));
    }

    #[test]
    fn test_raw_bytes_to_tag_value_integer_u16() {
        use crate::parsers::tiff::ifd_parser::ByteOrder;
        let bytes = [0x05, 0x00]; // 5 in little-endian
        let value = raw_bytes_to_tag_value(&bytes, 3, 1, 0, ByteOrder::LittleEndian); // Type 3 = SHORT
        assert_eq!(value.as_integer(), Some(5));
    }

    #[test]
    fn test_raw_bytes_to_tag_value_integer_u32() {
        use crate::parsers::tiff::ifd_parser::ByteOrder;
        let bytes = [0x64, 0x00, 0x00, 0x00]; // 100 in little-endian
        let value = raw_bytes_to_tag_value(&bytes, 4, 1, 0, ByteOrder::LittleEndian); // Type 4 = LONG
                                                                                      // Fallback conversion treats 4 bytes as integer
        assert_eq!(value.as_integer(), Some(100));
    }

    #[test]
    fn test_raw_bytes_to_tag_value_binary() {
        use crate::parsers::tiff::ifd_parser::ByteOrder;
        let bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x10, 0x20]; // Non-ASCII bytes
        let value = raw_bytes_to_tag_value(&bytes, 7, 1, 0, ByteOrder::LittleEndian); // Type 7 = UNDEFINED
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
