//! Metadata operations (Read/Write/Copy/Transform)
//!
//! This module defines core operations for metadata manipulation.
//! It orchestrates format detection, parser selection, and metadata extraction
//! following the hexagonal architecture pattern.

use super::{FileFormat, FileReader, MetadataMap, TagValue};
use crate::core::validation::validate_tag_value_with_name;
use crate::error::{ExifToolError, Result};
use crate::io::MMapReader;
use crate::parsers::DetectorMode;
use crate::parsers::archive::gz::parse_gz_metadata;
use crate::parsers::archive::iso::parse_iso_metadata;
use crate::parsers::archive::rar::parse_rar_metadata;
use crate::parsers::archive::sevenz::parse_7z_metadata;
use crate::parsers::archive::tar::parse_tar_metadata;
use crate::parsers::archive::zip::parse_zip_metadata;
use crate::parsers::audio::aac::parse_aac_metadata;
use crate::parsers::audio::ape::parse_ape_metadata;
use crate::parsers::audio::flac::parse_flac_metadata;
use crate::parsers::audio::mp3::parse_mp3_metadata;
use crate::parsers::audio::ogg::parse_ogg_metadata;
use crate::parsers::audio::opus::parse_opus_metadata;
use crate::parsers::audio::wav::parse_wav_metadata;
use crate::parsers::format_detector::detect_format;
use crate::parsers::jpeg::segment_parser::{parse_segments, Segment};
use crate::parsers::jpeg::xmp_parser::extract_xmp_from_segments;
use crate::parsers::pdf::parse_pdf_metadata;
use crate::parsers::pe::parse_pe_metadata;
use crate::parsers::png::parse_png_metadata;
use crate::parsers::quicktime::parse_quicktime_metadata;
use crate::parsers::video::avi::parse_avi_metadata;
use crate::parsers::video::flv::parse_flv_metadata;
use crate::parsers::video::mkv::parse_mkv_metadata;
use crate::parsers::video::mts::parse_mts_metadata;
use crate::parsers::video::webm::parse_webm_metadata;
// Font parsers
use crate::parsers::document::epub::parse_epub_metadata;
use crate::parsers::document::ooxml::parse_docx_metadata;
use crate::parsers::document::ooxml::parse_pptx_metadata;
use crate::parsers::document::ooxml::parse_xlsx_metadata;
use crate::parsers::font::otf::parse_otf_metadata;
use crate::parsers::font::ttf::parse_ttf_metadata;
use crate::parsers::font::woff::parse_woff_metadata;
use crate::parsers::font::woff2::parse_woff2_metadata;
// Advanced image parsers
use crate::parsers::image::avif::parse_avif_metadata;
use crate::parsers::image::bpg::parse_bpg_metadata;
use crate::parsers::image::exr::parse_exr_metadata;
use crate::parsers::image::flif::parse_flif_metadata;
use crate::parsers::image::ico::parse_ico_metadata;
use crate::parsers::image::jxl::parse_jxl_metadata;
use crate::parsers::image::psd::parse_psd_metadata;
use crate::parsers::image::svg::parse_svg_metadata;
// Specialized parsers
use crate::parsers::specialized::dwg::parse_dwg_metadata;
use crate::parsers::specialized::dxf::parse_dxf_metadata;
use crate::parsers::specialized::elf::parse_elf_metadata;
use crate::parsers::specialized::fits::parse_fits_metadata;
use crate::parsers::specialized::gltf::parse_gltf_metadata;
use crate::parsers::specialized::hdf5::parse_hdf5_metadata;
use crate::parsers::specialized::lnk::parse_lnk_metadata;
use crate::parsers::specialized::macho::parse_macho_metadata;
use crate::parsers::specialized::obj::parse_obj_metadata;
use crate::parsers::specialized::stl::parse_stl_metadata;
// Text parsers
use crate::parsers::text::vcf::parse_vcf_metadata;
// Image parsers
use crate::parsers::image::bmp::parse_bmp_metadata;
use crate::parsers::image::gif::parse_gif_metadata;
use crate::parsers::image::heif::parse_heif_metadata;
use crate::parsers::image::webp::parse_webp_metadata;
use crate::parsers::tiff::ifd_parser::{parse_ifd, ByteOrder};
use crate::parsers::tiff::makernotes::canon;
use crate::tag_db::lookup_tag_name;
use crate::tag_db::tag_registry::get_tag_descriptor;
use crate::writers::atomic_writer::write_atomic;
use crate::writers::jpeg_writer::write_exif_to_jpeg;
use chrono;
use std::collections::HashMap;
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
///
/// # See Also
///
/// * `read_metadata_with_detector` - Version that supports AI-powered detection
pub fn read_metadata(path: &Path) -> Result<MetadataMap> {
    read_metadata_with_detector(path, DetectorMode::Signature)
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
///
/// # Examples
///
/// ```no_run
/// use oxidex::core::operations::read_metadata_with_detector;
/// use oxidex::parsers::DetectorMode;
/// use std::path::Path;
///
/// // Default signature-based detection
/// let metadata = read_metadata_with_detector(
///     Path::new("photo.jpg"),
///     DetectorMode::Signature
/// )?;
///
/// // AI-powered detection (requires --features magika)
/// # #[cfg(feature = "magika")]
/// let metadata = read_metadata_with_detector(
///     Path::new("photo.jpg"),
///     DetectorMode::Magika
/// )?;
/// # Ok::<(), oxidex::error::ExifToolError>(())
/// ```
pub fn read_metadata_with_detector(path: &Path, detector_mode: DetectorMode) -> Result<MetadataMap> {
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
    let mut format = detect_format_with_mode(&reader, detector_mode)?;

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
            // Pass the original tag_name (e.g., "IFD0:Make") for error messages
            validate_tag_value_with_name(tag_name, descriptor, tag_value)?;
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
// SECTION 2: FORMAT PARSER DISPATCH
// ============================================================================

/// Dispatches to the appropriate format parser based on file format.
///
/// This function encapsulates the large match statement for format-specific parsing,
/// applying a consistent error conversion pattern across all parsers.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the file data
/// * `format` - Detected file format
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully parsed metadata
/// * `Err(ExifToolError)` - Parse error or unsupported format
fn dispatch_format_parser(reader: &dyn FileReader, format: FileFormat) -> Result<MetadataMap> {
    match format {
        FileFormat::JPEG => parse_jpeg_metadata(reader),
        FileFormat::TIFF => parse_tiff_metadata(reader),
        FileFormat::PNG => parse_png_metadata(reader),
        FileFormat::PDF => parse_pdf_metadata(reader),
        FileFormat::PE => parse_pe_metadata(reader),
        FileFormat::QuickTime => convert_string_error(parse_quicktime_metadata(reader), "QuickTime"),
        FileFormat::CasioCAM => parse_casio_cam_metadata(reader),
        FileFormat::CameraRaw(raw_format) => {
            // Parse camera raw format using raw metadata parser
            // Read entire file for raw parsing (raw formats need full file access)
            let size = reader.size() as usize;
            let data = reader.read(0, size)?;
            crate::parsers::raw::parse_raw_metadata(data, raw_format)
        }
        FileFormat::MKV => convert_string_error(parse_mkv_metadata(reader), "MKV"),
        FileFormat::WEBM => convert_string_error(parse_webm_metadata(reader), "WebM"),
        FileFormat::FLV => convert_string_error(parse_flv_metadata(reader), "FLV"),
        FileFormat::AVI => convert_string_error(parse_avi_metadata(reader), "AVI"),
        FileFormat::MTS => convert_string_error(parse_mts_metadata(reader), "MTS"),
        FileFormat::MP3 => convert_string_error(parse_mp3_metadata(reader), "MP3"),
        FileFormat::FLAC => convert_string_error(parse_flac_metadata(reader), "FLAC"),
        FileFormat::AAC => convert_string_error(parse_aac_metadata(reader), "AAC"),
        FileFormat::WAV => convert_string_error(parse_wav_metadata(reader), "WAV"),
        FileFormat::OGG => convert_string_error(parse_ogg_metadata(reader), "OGG"),
        FileFormat::OPUS => convert_string_error(parse_opus_metadata(reader), "Opus"),
        FileFormat::APE => convert_string_error(parse_ape_metadata(reader), "APE"),
        FileFormat::ZIP => convert_string_error(parse_zip_metadata(reader), "ZIP"),
        FileFormat::DOCX => convert_string_error(parse_docx_metadata(reader), "DOCX"),
        FileFormat::XLSX => convert_string_error(parse_xlsx_metadata(reader), "XLSX"),
        FileFormat::PPTX => convert_string_error(parse_pptx_metadata(reader), "PPTX"),
        FileFormat::Pages => convert_string_error(parse_docx_metadata(reader), "Pages"),
        FileFormat::Numbers => convert_string_error(parse_xlsx_metadata(reader), "Numbers"),
        FileFormat::Keynote => convert_string_error(parse_pptx_metadata(reader), "Keynote"),
        FileFormat::EPUB => convert_string_error(parse_epub_metadata(reader), "EPUB"),
        FileFormat::RAR => convert_string_error(parse_rar_metadata(reader), "RAR"),
        FileFormat::SevenZ => convert_string_error(parse_7z_metadata(reader), "7z"),
        FileFormat::ISO => convert_string_error(parse_iso_metadata(reader), "ISO"),
        FileFormat::TAR => convert_string_error(parse_tar_metadata(reader), "TAR"),
        FileFormat::GZ => convert_string_error(parse_gz_metadata(reader), "GZ"),
        // Font formats
        FileFormat::TTF => convert_string_error(parse_ttf_metadata(reader), "TTF"),
        FileFormat::OTF => convert_string_error(parse_otf_metadata(reader), "OTF"),
        FileFormat::WOFF => convert_string_error(parse_woff_metadata(reader), "WOFF"),
        FileFormat::WOFF2 => convert_string_error(parse_woff2_metadata(reader), "WOFF2"),
        // Advanced image formats
        FileFormat::AVIF => convert_string_error(parse_avif_metadata(reader), "AVIF"),
        FileFormat::HEIF => convert_string_error(parse_heif_metadata(reader), "HEIF"),
        FileFormat::JXL => convert_string_error(parse_jxl_metadata(reader), "JXL"),
        FileFormat::BPG => convert_string_error(parse_bpg_metadata(reader), "BPG"),
        FileFormat::EXR => convert_string_error(parse_exr_metadata(reader), "EXR"),
        FileFormat::FLIF => convert_string_error(parse_flif_metadata(reader), "FLIF"),
        FileFormat::SVG => convert_string_error(parse_svg_metadata(reader), "SVG"),
        FileFormat::ICO => convert_string_error(parse_ico_metadata(reader), "ICO"),
        FileFormat::PSD => convert_string_error(parse_psd_metadata(reader), "PSD"),
        // Specialized formats
        FileFormat::ELF => convert_string_error(parse_elf_metadata(reader), "ELF"),
        FileFormat::MachO => convert_string_error(parse_macho_metadata(reader), "Mach-O"),
        FileFormat::DWG => convert_string_error(parse_dwg_metadata(reader), "DWG"),
        FileFormat::DXF => convert_string_error(parse_dxf_metadata(reader), "DXF"),
        FileFormat::STL => convert_string_error(parse_stl_metadata(reader), "STL"),
        FileFormat::OBJ => convert_string_error(parse_obj_metadata(reader), "OBJ"),
        FileFormat::GLTF => convert_string_error(parse_gltf_metadata(reader), "glTF"),
        FileFormat::FITS => convert_string_error(parse_fits_metadata(reader), "FITS"),
        FileFormat::HDF5 => convert_string_error(parse_hdf5_metadata(reader), "HDF5"),
        FileFormat::VCF => convert_string_error(parse_vcf_metadata(reader), "VCF"),
        FileFormat::LNK => convert_string_error(parse_lnk_metadata(reader), "LNK"),
        FileFormat::GIF => convert_string_error(parse_gif_metadata(reader), "GIF"),
        FileFormat::BMP => convert_string_error(parse_bmp_metadata(reader), "BMP"),
        FileFormat::WebP => convert_string_error(parse_webp_metadata(reader), "WebP"),
        _ => Err(ExifToolError::unsupported_format(format!(
            "Format {:?} not yet supported in this iteration",
            format
        ))),
    }
}

/// Converts a Result<T, String> to Result<T, ExifToolError> with a formatted parse error.
///
/// This helper function provides a consistent error conversion pattern for parsers
/// that return String errors.
///
/// # Arguments
///
/// * `result` - The result to convert
/// * `format_name` - The name of the format for error messages (e.g., "PNG", "QuickTime")
///
/// # Returns
///
/// * `Ok(T)` - The successful value
/// * `Err(ExifToolError)` - A parse error with the format name included
fn convert_string_error<T>(result: std::result::Result<T, String>, format_name: &str) -> Result<T> {
    result.map_err(|e| ExifToolError::parse_error(format!("{} parse error: {}", format_name, e)))
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
fn parse_jpeg_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    // Parse JPEG segment structure
    let segments = parse_segments(reader)?;

    let mut metadata = MetadataMap::new();

    // Process different segment types
    process_jfif_segments(&segments, &mut metadata);
    process_exif_segments(&segments, reader, &mut metadata);
    process_xmp_segments(&segments, &mut metadata);
    process_iptc_segments(&segments, &mut metadata);
    process_icc_segments(&segments, &mut metadata);

    Ok(metadata)
}

/// Processes JFIF APP0 segments and extracts version and resolution metadata.
///
/// JFIF segments contain basic image information including version, resolution unit,
/// and X/Y resolution values.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with JFIF tags
fn process_jfif_segments(segments: &[Segment], metadata: &mut MetadataMap) {
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
fn process_exif_segments(segments: &[Segment], reader: &dyn FileReader, metadata: &mut MetadataMap) {
    // Find all APP1 segments (EXIF/XMP)
    let app1_segments: Vec<_> = segments.iter().filter(|s| s.is_app1()).collect();

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
            if let Ok(tags) = parse_ifd(&tiff_reader, ifd_offset, byte_order) {
                // Process IFD0 tags and get sub-IFD offsets
                let (exif_ifd_offset, gps_ifd_offset) =
                    process_ifd0_tags(&tags, byte_order, metadata);

                // Parse EXIF Sub-IFD if present
                if let Some(offset) = exif_ifd_offset {
                    parse_exif_subifd(&tiff_reader, offset, byte_order, metadata);
                }

                // Parse GPS Sub-IFD if present
                if let Some(offset) = gps_ifd_offset {
                    parse_gps_subifd(&tiff_reader, offset, byte_order, metadata);
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
///
/// # Returns
///
/// A tuple of (exif_ifd_offset, gps_ifd_offset) for sub-IFD parsing
fn process_ifd0_tags(
    tags: &[(u16, u16, u32, std::borrow::Cow<[u8]>)],
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
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

        // Convert tag ID to tag name (IFD0 for main JPEG EXIF)
        let tag_name = lookup_tag_name(*tag_id, "IFD0");

        // Convert raw bytes to TagValue
        let tag_value = raw_bytes_to_tag_value(
            bytes,
            *field_type,
            *value_count,
            *tag_id,
            byte_order,
        );

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
fn process_xmp_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    match extract_xmp_from_segments(segments) {
        Ok(xmp_tags) => {
            // Add all XMP tags to metadata
            for (tag_name, value) in xmp_tags {
                // Try to parse as integer first, then as float, otherwise keep as string
                let tag_value = parse_string_to_tag_value(&value);
                metadata.insert(tag_name, tag_value);
            }
        }
        Err(e) => {
            // Log error but continue processing (don't fail entire read)
            eprintln!("Warning: Failed to parse XMP: {}", e);
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
fn process_iptc_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    match crate::parsers::jpeg::iptc_parser::extract_iptc_from_segments(segments) {
        Ok(iptc_tags) => {
            // Add all IPTC tags to metadata
            for (tag_name, value) in iptc_tags {
                // Try to parse as integer first, then as float, otherwise keep as string
                let tag_value = parse_string_to_tag_value(&value);
                metadata.insert(tag_name, tag_value);
            }
        }
        Err(e) => {
            // Log error but continue processing
            eprintln!("Warning: Failed to extract IPTC metadata: {}", e);
        }
    }
}

/// Processes ICC profile APP2 segments and extracts color profile metadata.
///
/// ICC (International Color Consortium) profiles describe the color
/// characteristics of an image and are stored in APP2 segments.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments
/// * `metadata` - MetadataMap to populate with ICC profile tags
fn process_icc_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    for segment in segments.iter().filter(|s| s.marker == 0xFFE2) {
        // Check if this is an ICC profile segment (starts with "ICC_PROFILE\0")
        if segment.data.len() >= 14 && &segment.data[0..12] == b"ICC_PROFILE\0" {
            // ICC profile structure in JPEG APP2:
            // Bytes 0-11: "ICC_PROFILE\0" identifier
            // Byte 12: Chunk number (1-based)
            // Byte 13: Total chunks
            // Bytes 14+: ICC profile data

            // For now, only handle single-chunk ICC profiles (most common)
            let chunk_num = segment.data[12];
            let total_chunks = segment.data[13];

            if chunk_num == 1 && total_chunks == 1 {
                // Single chunk - parse ICC profile directly
                let icc_data = &segment.data[14..];
                match crate::parsers::icc_parser::parse_icc_profile_data(icc_data) {
                    Ok(icc_tags) => {
                        // Add all ICC tags to metadata with "Profile:" prefix
                        for (tag_name, value) in icc_tags {
                            metadata.insert(format!("Profile:{}", tag_name), value);
                        }
                    }
                    Err(e) => {
                        // Log error but continue processing
                        eprintln!("Warning: Failed to parse ICC profile: {}", e);
                    }
                }
            } else {
                // Multi-chunk ICC profile - would need to reassemble chunks
                // This is less common, so we'll skip for now
                eprintln!(
                    "Warning: Multi-chunk ICC profile detected ({}/{}), not yet supported",
                    chunk_num, total_chunks
                );
            }
        }
    }
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

    Ok(metadata)
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
/// * `byte_order` - Byte order for the file
/// * `metadata` - MetadataMap to populate
///
/// # Returns
///
/// * `Ok(())` - Successfully parsed all IFDs
/// * `Err(ExifToolError)` - Parse error
fn parse_ifd_chain(
    reader: &dyn FileReader,
    first_offset: u64,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) -> Result<()> {
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

        // Parse EXIF Sub-IFD if present
        if let Some(offset) = exif_offset {
            parse_exif_subifd(reader, offset, byte_order, metadata);
        }

        // Parse GPS Sub-IFD if present
        if let Some(offset) = gps_offset {
            parse_gps_subifd(reader, offset, byte_order, metadata);
        }

        // Parse Canon MakerNote if present
        if let Some(makernote_bytes) = makernote_data {
            parse_makernote_if_canon(makernote_bytes, byte_order, metadata);
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
            match crate::parsers::icc_parser::parse_icc_profile_data(bytes) {
                Ok(icc_tags) => {
                    // Add all ICC tags to metadata with "Profile:" prefix
                    for (tag_name, value) in icc_tags {
                        metadata.insert(format!("Profile:{}", tag_name), value);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse ICC profile in TIFF: {}", e);
                }
            }
            // Don't continue - still add the raw ICC_Profile tag
        }

        // Convert tag to metadata
        let tag_name = lookup_tag_name(*tag_id, ifd_name);
        let tag_value =
            raw_bytes_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order);
        metadata.insert(tag_name, tag_value);
    }

    (exif_ifd_offset, gps_ifd_offset, makernote_data)
}

/// Parses an EXIF sub-IFD and extracts tags.
///
/// The EXIF sub-IFD contains detailed camera settings and shooting parameters.
/// It may also contain MakerNote data specific to camera manufacturers.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the file
/// * `offset` - Offset to the EXIF sub-IFD
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `metadata` - MetadataMap to populate
fn parse_exif_subifd(
    reader: &dyn FileReader,
    offset: u64,
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    if let Ok(exif_tags) = parse_ifd(reader, offset, byte_order) {
        // Track MakerNote in EXIF IFD
        let mut exif_makernote_data: Option<&[u8]> = None;

        // First pass: convert tags and capture MakerNote
        for (tag_id, field_type, value_count, raw_bytes) in &exif_tags {
            // Convert Cow<[u8]> to &[u8] for processing
            let bytes = raw_bytes.as_ref();

            // Check for MakerNote in EXIF IFD
            if *tag_id == 0x927C {
                exif_makernote_data = Some(bytes);
            }

            let tag_name = lookup_tag_name(*tag_id, "ExifIFD");
            let tag_value = raw_bytes_to_tag_value(
                bytes,
                *field_type,
                *value_count,
                *tag_id,
                byte_order,
            );
            metadata.insert(tag_name, tag_value);
        }

        // Second pass: Parse Canon MakerNote if found in EXIF IFD
        if let Some(makernote_bytes) = exif_makernote_data {
            parse_makernote_if_canon(makernote_bytes, byte_order, metadata);
        }
    }
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
fn parse_gps_subifd(
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
            metadata.insert(tag_name, tag_value);
        }
    }
}

/// Parses Canon MakerNote data if present.
///
/// Canon cameras store proprietary metadata in MakerNote tags.
/// This function detects and parses Canon-specific formats.
///
/// # Arguments
///
/// * `makernote_data` - Raw MakerNote bytes
/// * `byte_order` - Byte order for interpreting multi-byte values
/// * `metadata` - MetadataMap to populate with Canon tags
fn parse_makernote_if_canon(
    makernote_data: &[u8],
    byte_order: ByteOrder,
    metadata: &mut MetadataMap,
) {
    // Check if this is a Canon MakerNote
    if canon::is_canon_makernote(makernote_data) {
        // Parse Canon MakerNote tags
        let mut canon_tags = HashMap::new();
        canon::parse_canon_makernotes(makernote_data, byte_order, &mut canon_tags);
        // Add Canon tags to metadata
        // Note: tag names already include "Canon:" prefix from canon_tag_to_name()
        for (tag_name, tag_value_str) in canon_tags {
            // Convert string value to TagValue
            let tag_value = TagValue::String(tag_value_str);
            metadata.insert(tag_name, tag_value);
        }
    }
    // If not Canon, silently ignore - other vendors' MakerNotes
    // can be added in future phases (Nikon, Sony, etc.)
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
fn parse_casio_cam_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
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
// SECTION 5: TAG VALUE CONVERSION HELPERS
// ============================================================================

/// Converts raw bytes from IFD to a TagValue.
///
/// This function interprets raw bytes according to the EXIF field type,
/// converting them to the appropriate TagValue variant. It delegates to
/// specialized helper functions for complex types.
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

    // Try special tag handlers first (GPS_VERSION_ID, EXIF_VERSION, etc.)
    if let Some(value) = handle_special_byte_tags(tag_id, bytes) {
        return value;
    }

    // Try to convert field_type to ExifType
    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            // RATIONAL (type 5): two 32-bit unsigned integers (numerator/denominator)
            ExifType::Rational if bytes.len() >= 8 => {
                return handle_rational_type(bytes, value_count, tag_id, byte_order);
            }

            // SRATIONAL (type 10): two 32-bit signed integers (numerator/denominator)
            ExifType::SRational if bytes.len() >= 8 => {
                return handle_srational_type(bytes, value_count, byte_order);
            }

            // SHORT (type 3): unsigned 16-bit integers
            ExifType::Short if bytes.len() >= 2 => {
                return handle_short_type(bytes, value_count, byte_order);
            }

            // LONG (type 4): unsigned 32-bit integers
            ExifType::Long if bytes.len() >= 4 => {
                return handle_long_type(bytes, value_count, byte_order);
            }

            // ASCII (type 2): null-terminated string
            ExifType::Ascii => {
                return handle_ascii_type(bytes);
            }

            // BYTE (type 1) and UNDEFINED (type 7): binary or heuristic conversion
            ExifType::Byte | ExifType::Undefined => {
                // For UNDEFINED type, if no specific handler matched, return binary
                if field_type == 7 {
                    return TagValue::new_binary(bytes.to_vec());
                }
                // Fall through to heuristic conversion for BYTE type
            }

            _ => {
                // Fall through to heuristic conversion below
            }
        }
    }

    // Fallback heuristic conversion for unknown types or when type-specific logic doesn't apply
    heuristic_bytes_to_tag_value(bytes, byte_order)
}

/// Handles special byte-encoded tags that need custom formatting.
///
/// This includes GPS_VERSION_ID, EXIF_VERSION, and COMPONENTS_CONFIGURATION
/// which have specific byte-level encoding requirements.
///
/// # Arguments
///
/// * `tag_id` - The tag ID to check
/// * `bytes` - The raw bytes
///
/// # Returns
///
/// Some(TagValue) if this is a special tag, None otherwise
fn handle_special_byte_tags(tag_id: u16, bytes: &[u8]) -> Option<TagValue> {
    // Tag ID constants
    const GPS_VERSION_ID: u16 = 0x0000;
    const EXIF_VERSION: u16 = 0x9000;
    const COMPONENTS_CONFIGURATION: u16 = 0x9101;

    match tag_id {
        // GPS Version ID (4 bytes: major.minor.rev.0)
        GPS_VERSION_ID if bytes.len() >= 4 => {
            Some(TagValue::new_string(format!(
                "{}.{}.{}.{}",
                bytes[0], bytes[1], bytes[2], bytes[3]
            )))
        }

        // Exif Version (4 bytes: ASCII "0232")
        EXIF_VERSION if bytes.len() >= 4 => {
            // ExifVersion is stored as ASCII bytes
            let version = String::from_utf8_lossy(&bytes[0..4]);
            Some(TagValue::new_string(version.to_string()))
        }

        // ComponentsConfiguration (4 bytes with component IDs)
        COMPONENTS_CONFIGURATION if bytes.len() >= 4 => {
            let component_names = bytes
                .iter()
                .take(4)
                .map(|&b| match b {
                    0 => "-",
                    1 => "Y",
                    2 => "Cb",
                    3 => "Cr",
                    4 => "R",
                    5 => "G",
                    6 => "B",
                    _ => "?",
                })
                .collect::<Vec<_>>();
            Some(TagValue::new_string(component_names.join(", ")))
        }

        _ => None,
    }
}

/// Handles RATIONAL type fields (type 5).
///
/// RATIONAL values are pairs of unsigned 32-bit integers (numerator/denominator).
/// This function handles both single rationals and arrays of rationals, with
/// special formatting for GPS coordinates and exposure settings.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing rational data
/// * `value_count` - Number of rational values
/// * `tag_id` - Tag ID for special handling
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// A TagValue representing the rational data
fn handle_rational_type(
    bytes: &[u8],
    value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    // GPS coordinate tags (3 rationals: degrees, minutes, seconds)
    const GPS_LATITUDE: u16 = 0x0002;
    const GPS_LONGITUDE: u16 = 0x0004;
    const GPS_DEST_LATITUDE: u16 = 0x0014;
    const GPS_DEST_LONGITUDE: u16 = 0x0016;
    const GPS_ALTITUDE: u16 = 0x0006;
    const EXPOSURE_TIME: u16 = 0x829A;

    // Check if this is an array of rationals (count > 1)
    if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
        // Special handling for GPS coordinates (3 rationals: degrees, minutes, seconds)
        if matches!(
            tag_id,
            GPS_LATITUDE | GPS_LONGITUDE | GPS_DEST_LATITUDE | GPS_DEST_LONGITUDE
        ) && value_count == 3
        {
            return format_gps_coordinate(bytes, byte_order);
        }

        // Parse array of rationals and format as space-separated decimals
        return parse_rational_array(bytes, value_count, byte_order);
    }

    // Single rational value - parse numerator and denominator
    let numerator = read_u32(&bytes[0..4], byte_order);
    let denominator = read_u32(&bytes[4..8], byte_order);

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
        let gcd_value = gcd(numerator, denominator);
        let simplified_num = numerator / gcd_value;
        let simplified_den = denominator / gcd_value;
        // Only format as fraction if denominator > 1
        if simplified_den > 1 {
            return TagValue::new_string(format!("{}/{}", simplified_num, simplified_den));
        }
    }

    TagValue::new_rational(numerator as i32, denominator as i32)
}

/// Formats a GPS coordinate from 3 rational values (degrees, minutes, seconds).
///
/// GPS coordinates are stored as 3 rationals representing degrees, minutes, and seconds.
/// This function converts them to a human-readable DMS (Degrees, Minutes, Seconds) format.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing 3 rationals (24 bytes total)
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// A TagValue with formatted GPS coordinate (e.g., "37 deg 46' 33.24\"")
fn format_gps_coordinate(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    let mut dms = Vec::new();
    for i in 0..3 {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
        if denominator != 0 {
            dms.push(numerator as f64 / denominator as f64);
        } else {
            dms.push(numerator as f64);
        }
    }
    // Format as DMS: "37 deg 46' 33.24""
    let formatted = format!("{} deg {}' {:.2}\"", dms[0] as i32, dms[1] as i32, dms[2]);
    TagValue::new_string(formatted)
}

/// Parses an array of rational values into a space-separated string.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing multiple rationals
/// * `value_count` - Number of rational values
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// A TagValue with space-separated decimal values
fn parse_rational_array(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    let mut values = Vec::new();
    for i in 0..value_count as usize {
        let offset = i * 8;
        let numerator = read_u32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_u32(&bytes[offset + 4..offset + 8], byte_order);
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
    TagValue::new_string(formatted)
}

/// Handles SRATIONAL type fields (type 10).
///
/// SRATIONAL values are pairs of signed 32-bit integers (numerator/denominator).
/// This function handles both single signed rationals and arrays.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing signed rational data
/// * `value_count` - Number of signed rational values
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// A TagValue representing the signed rational data
fn handle_srational_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    // Check if this is an array
    if value_count > 1 && bytes.len() >= (value_count as usize * 8) {
        return parse_srational_array(bytes, value_count, byte_order);
    }

    // Single signed rational
    let numerator = read_i32(&bytes[0..4], byte_order);
    let denominator = read_i32(&bytes[4..8], byte_order);
    TagValue::new_rational(numerator, denominator)
}

/// Parses an array of signed rational values into a space-separated string.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing multiple signed rationals
/// * `value_count` - Number of signed rational values
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// A TagValue with space-separated decimal values
fn parse_srational_array(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    let mut values = Vec::new();
    for i in 0..value_count as usize {
        let offset = i * 8;
        let numerator = read_i32(&bytes[offset..offset + 4], byte_order);
        let denominator = read_i32(&bytes[offset + 4..offset + 8], byte_order);
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
    TagValue::new_string(formatted)
}

/// Handles SHORT type fields (type 3).
///
/// SHORT values are unsigned 16-bit integers. This function handles both
/// single shorts and arrays of shorts.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing short data
/// * `value_count` - Number of short values
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// A TagValue representing the short data
fn handle_short_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    // Handle array of shorts
    if value_count > 1 && bytes.len() >= (value_count as usize * 2) {
        let mut values = Vec::new();
        for i in 0..value_count as usize {
            let offset = i * 2;
            let value = read_u16(&bytes[offset..offset + 2], byte_order);
            values.push(value.to_string());
        }
        return TagValue::new_string(values.join(" "));
    }

    // Handle single SHORT value
    let value = read_u16(&bytes[0..2], byte_order) as i64;

    // Keep raw numeric value; friendly names are applied at presentation time.
    TagValue::new_integer(value)
}

/// Handles LONG type fields (type 4).
///
/// LONG values are unsigned 32-bit integers. This function handles both
/// single longs and arrays of longs.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing long data
/// * `value_count` - Number of long values
/// * `byte_order` - Byte order for interpreting values
///
/// # Returns
///
/// A TagValue representing the long data
fn handle_long_type(bytes: &[u8], value_count: u32, byte_order: ByteOrder) -> TagValue {
    // Handle array of longs
    if value_count > 1 && bytes.len() >= (value_count as usize * 4) {
        let mut values = Vec::new();
        for i in 0..value_count as usize {
            let offset = i * 4;
            let value = read_u32(&bytes[offset..offset + 4], byte_order);
            values.push(value.to_string());
        }
        return TagValue::new_string(values.join(" "));
    }

    // Handle single LONG value
    let value = read_u32(&bytes[0..4], byte_order) as i64;

    // Keep raw numeric value; friendly names are applied at presentation time.
    TagValue::new_integer(value)
}

/// Handles ASCII type fields (type 2).
///
/// ASCII values are null-terminated strings. This function also detects
/// and parses EXIF DateTime strings.
///
/// # Arguments
///
/// * `bytes` - Raw bytes containing ASCII string
///
/// # Returns
///
/// A TagValue representing the string (or DateTime if applicable)
fn handle_ascii_type(bytes: &[u8]) -> TagValue {
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
    TagValue::new_string(String::new())
}

/// Applies heuristic conversion for unknown or ambiguous byte sequences.
///
/// This function attempts to intelligently convert raw bytes to a TagValue
/// when the field type is unknown or doesn't provide enough information.
///
/// # Arguments
///
/// * `bytes` - Raw bytes to convert
/// * `byte_order` - Byte order for interpreting multi-byte values
///
/// # Returns
///
/// A TagValue based on heuristic analysis of the bytes
fn heuristic_bytes_to_tag_value(bytes: &[u8], byte_order: ByteOrder) -> TagValue {
    // Try to interpret as integer first (if 2 or 4 bytes)
    if bytes.len() == 2 {
        let value = read_u16(bytes, byte_order) as i64;
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
        let value = read_u32(bytes, byte_order) as i64;
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

// ============================================================================
// SECTION 6: UTILITY FUNCTIONS
// ============================================================================

/// Checks if a string matches the EXIF DateTime format (YYYY:MM:DD HH:MM:SS).
///
/// EXIF DateTime format: "2025:01:15 10:30:00" (19 characters)
/// - 4 digits for year
/// - 2 colons separating year:month:day
/// - 1 space between date and time
/// - 2 colons separating hour:minute:second
///
/// # Arguments
///
/// * `s` - String to check
///
/// # Returns
///
/// true if the string matches EXIF DateTime format, false otherwise
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
///
/// # Arguments
///
/// * `s` - EXIF DateTime string
///
/// # Returns
///
/// * `Ok(DateTime<Utc>)` - Successfully parsed datetime
/// * `Err(ExifToolError)` - Invalid datetime format
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

/// Parses a string value to an appropriate TagValue.
///
/// Attempts to parse as integer first, then float, otherwise returns as string.
/// Used for XMP and IPTC metadata parsing.
///
/// # Arguments
///
/// * `value` - String value to parse
///
/// # Returns
///
/// A TagValue with the appropriate type
fn parse_string_to_tag_value(value: &str) -> TagValue {
    if let Ok(int_val) = value.parse::<i64>() {
        TagValue::Integer(int_val)
    } else if let Ok(float_val) = value.parse::<f64>() {
        TagValue::Float(float_val)
    } else {
        TagValue::String(value.to_string())
    }
}

/// Reads an unsigned 16-bit integer from bytes with the specified byte order.
///
/// # Arguments
///
/// * `bytes` - Byte slice (must be at least 2 bytes)
/// * `byte_order` - Byte order for interpretation
///
/// # Returns
///
/// The u16 value
fn read_u16(bytes: &[u8], byte_order: ByteOrder) -> u16 {
    match byte_order {
        ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
    }
}

/// Reads an unsigned 32-bit integer from bytes with the specified byte order.
///
/// # Arguments
///
/// * `bytes` - Byte slice (must be at least 4 bytes)
/// * `byte_order` - Byte order for interpretation
///
/// # Returns
///
/// The u32 value
fn read_u32(bytes: &[u8], byte_order: ByteOrder) -> u32 {
    match byte_order {
        ByteOrder::LittleEndian => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        ByteOrder::BigEndian => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    }
}

/// Reads a signed 32-bit integer from bytes with the specified byte order.
///
/// # Arguments
///
/// * `bytes` - Byte slice (must be at least 4 bytes)
/// * `byte_order` - Byte order for interpretation
///
/// # Returns
///
/// The i32 value
fn read_i32(bytes: &[u8], byte_order: ByteOrder) -> i32 {
    match byte_order {
        ByteOrder::LittleEndian => i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        ByteOrder::BigEndian => i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    }
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

// ============================================================================
// SECTION 7: TESTS
// ============================================================================

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
