//! Metadata operations (Read/Write/Copy/Transform)
//!
//! This module defines core operations for metadata manipulation.
//! It orchestrates format detection, parser selection, and metadata extraction
//! following the hexagonal architecture pattern.

use super::{FileFormat, FileReader, MetadataMap, TagValue};
use crate::core::format_dispatch::dispatch_format_parser;
use crate::core::jpeg_helpers::{
    process_app10_segments, process_app11_segments, process_app12_segments, process_app14_segments,
    process_exif_segments, process_icc_segments, process_iptc_segments, process_jfif_segments,
    process_mpf_segments, process_sof_segments, process_xmp_segments,
};
use crate::core::operations_helpers::{read_u16, read_u32};
#[cfg(test)]
use crate::core::tag_conversion::raw_bytes_to_tag_value;
use crate::core::tiff_helpers::parse_ifd_chain;
use crate::core::validation::{validate_tag_value_intrinsics, validate_tag_value_with_name};
use crate::error::{ExifToolError, Result};
use crate::io::MMapReader;
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
use crate::writers::tiff_writer::write_tiff_file;
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

    // Step 3: Detect format via magic bytes
    let mut format = detect_format(&reader)?;

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
    let format_metadata = dispatch_format_parser(&reader, format)?;

    // Step 5: Merge format-specific metadata into file metadata
    // Format-specific metadata takes precedence over file metadata in case of conflicts
    // Use into_iter() to consume format_metadata and avoid cloning keys and values
    for (key, value) in format_metadata {
        metadata.insert(key, value);
    }

    Ok(metadata)
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
    // PHASE 1: VALIDATION (fail fast before any file operations)
    // Iterate through all tags and validate each one against its descriptor
    for (tag_name, tag_value) in metadata.iter() {
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

    // PHASE 2: READ ORIGINAL FILE
    // Open file with MMapReader for zero-copy access
    let reader = MMapReader::new(path)?;

    // PHASE 3: DETECT FORMAT
    let format = detect_format(&reader)?;

    // PHASE 4: ROUTE TO APPROPRIATE WRITER
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
        FileFormat::TIFF => {
            write_tiff_file(path, &reader, metadata)?;
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
    metadata.insert(tag_name, new_value);

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
    metadata.remove(tag_name);

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
    // Parse JPEG segment structure
    let segments = parse_segments(reader)?;

    let mut metadata = MetadataMap::new();

    // Process different segment types
    process_jfif_segments(&segments, &mut metadata);
    process_exif_segments(&segments, reader, &mut metadata);
    process_xmp_segments(&segments, &mut metadata);
    process_iptc_segments(&segments, &mut metadata);
    process_icc_segments(&segments, &mut metadata);
    process_mpf_segments(&segments, &mut metadata);
    process_sof_segments(&segments, &mut metadata);

    // Process HDR and manufacturer-specific APP segments
    // TODO: re-enable when parse_app6 is implemented
    // process_app6_segments(&segments, &mut metadata);
    process_app10_segments(&segments, &mut metadata);
    process_app11_segments(&segments, &mut metadata);
    process_app12_segments(&segments, &mut metadata);
    process_app14_segments(&segments, &mut metadata);

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
