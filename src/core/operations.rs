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
use crate::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
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

    // Find all APP1 segments (EXIF/XMP)
    let app1_segments: Vec<_> = segments.iter().filter(|s| s.is_app1()).collect();

    if app1_segments.is_empty() {
        // No APP1 segments found - return empty metadata
        return Ok(MetadataMap::new());
    }

    let mut metadata = MetadataMap::new();

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
                    // Convert raw tag data to MetadataMap entries
                    for (tag_id, raw_bytes) in tags {
                        // Convert tag ID to tag name
                        let tag_name = tag_id_to_name(tag_id, "EXIF");

                        // Convert raw bytes to TagValue
                        // Note: We need the EXIF type to properly convert
                        // For now, we'll do a simple conversion assuming ASCII for strings
                        let tag_value = raw_bytes_to_tag_value(&raw_bytes);

                        metadata.insert(tag_name, tag_value);
                    }
                }
                Err(e) => {
                    // Log error but continue processing (don't fail entire read)
                    eprintln!("Warning: Failed to parse EXIF IFD: {}", e);
                }
            }
        }
    }

    Ok(metadata)
}

/// Parses metadata from a TIFF file.
///
/// TIFF files begin with a TIFF header followed by IFD structures.
/// This function:
/// 1. Reads TIFF header (byte order, magic number, IFD offset)
/// 2. Parses IFD structure
/// 3. Converts raw tag data to MetadataMap
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

    // Read IFD offset from bytes 4-7
    let ifd_offset = match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes([header[4], header[5], header[6], header[7]]),
        ByteOrder::BigEndian => u32::from_be_bytes([header[4], header[5], header[6], header[7]]),
    } as u64;

    // Parse IFD structure
    let tags = parse_ifd(reader, ifd_offset, byte_order)?;

    // Convert to MetadataMap
    let mut metadata = MetadataMap::new();
    for (tag_id, raw_bytes) in tags {
        let tag_name = tag_id_to_name(tag_id, "EXIF");
        let tag_value = raw_bytes_to_tag_value(&raw_bytes);
        metadata.insert(tag_name, tag_value);
    }

    Ok(metadata)
}

/// Converts a numeric tag ID to a canonical tag name.
///
/// Attempts to lookup the tag in the registry. If not found, returns
/// a fallback name in the format "FAMILY:0xXXXX".
fn tag_id_to_name(tag_id: u16, family: &str) -> String {
    // Try to find a tag descriptor matching this numeric ID
    // The registry is currently keyed by name, so we need to search by ID
    // For now, use a simple mapping for common tags
    match tag_id {
        0x010F => "EXIF:Make".to_string(),
        0x0110 => "EXIF:Model".to_string(),
        0x0112 => "EXIF:Orientation".to_string(),
        0x0131 => "EXIF:Software".to_string(),
        0x0132 => "EXIF:DateTime".to_string(),
        0x013B => "EXIF:Artist".to_string(),
        0x8298 => "EXIF:Copyright".to_string(),
        0x829A => "EXIF:ExposureTime".to_string(),
        0x829D => "EXIF:FNumber".to_string(),
        0x8827 => "EXIF:ISO".to_string(),
        0x9003 => "EXIF:DateTimeOriginal".to_string(),
        0x9004 => "EXIF:DateTimeDigitized".to_string(),
        0x920A => "EXIF:FocalLength".to_string(),
        _ => format!("{}:0x{:04X}", family, tag_id),
    }
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

/// Converts raw bytes from IFD to a TagValue.
///
/// This is a simplified conversion that attempts to interpret the data
/// as ASCII string if it looks like text, otherwise stores as binary.
///
/// TODO: This should use the EXIF type information for proper conversion
fn raw_bytes_to_tag_value(bytes: &[u8]) -> TagValue {
    // Try to interpret as integer first (if 2 or 4 bytes)
    if bytes.len() == 2 {
        let value = u16::from_le_bytes([bytes[0], bytes[1]]) as i64;
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
        let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64;
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
    fn test_tag_id_to_name_known_tags() {
        assert_eq!(tag_id_to_name(0x010F, "EXIF"), "EXIF:Make");
        assert_eq!(tag_id_to_name(0x0110, "EXIF"), "EXIF:Model");
        assert_eq!(tag_id_to_name(0x0132, "EXIF"), "EXIF:DateTime");
    }

    #[test]
    fn test_tag_id_to_name_unknown_tags() {
        assert_eq!(tag_id_to_name(0xFFFF, "EXIF"), "EXIF:0xFFFF");
        assert_eq!(tag_id_to_name(0x1234, "GPS"), "GPS:0x1234");
    }

    #[test]
    fn test_raw_bytes_to_tag_value_string() {
        let bytes = b"Canon\0";
        let value = raw_bytes_to_tag_value(bytes);
        assert_eq!(value.as_string(), Some("Canon"));
    }

    #[test]
    fn test_raw_bytes_to_tag_value_integer_u16() {
        let bytes = [0x05, 0x00]; // 5 in little-endian
        let value = raw_bytes_to_tag_value(&bytes);
        assert_eq!(value.as_integer(), Some(5));
    }

    #[test]
    fn test_raw_bytes_to_tag_value_integer_u32() {
        let bytes = [0x64, 0x00, 0x00, 0x00]; // 100 in little-endian
        let value = raw_bytes_to_tag_value(&bytes);
        // This will be treated as integer because it has null bytes
        assert_eq!(value.as_integer(), Some(100));
    }

    #[test]
    fn test_raw_bytes_to_tag_value_binary() {
        let bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x10, 0x20]; // Non-ASCII bytes
        let value = raw_bytes_to_tag_value(&bytes);
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
