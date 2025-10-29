//! FormatParser trait definition (Port)
//!
//! This module defines the `FormatParser` trait, which serves as the primary
//! secondary port in the hexagonal architecture for pluggable format support.
//!
//! # Architectural Role
//!
//! `FormatParser` is a **secondary port** (infrastructure interface) that enables
//! the domain layer to extract metadata from files without knowing the specifics
//! of each format. This design supports:
//!
//! - Adding new format support without modifying core logic
//! - Testing metadata operations with mock parsers
//! - Parallel development of format adapters
//! - Graceful degradation for unsupported formats
//!
//! # Format Adapters (Infrastructure Layer)
//!
//! Concrete implementations of this trait include:
//!
//! - `JpegParser`: EXIF/JFIF/XMP extraction from JPEG files
//! - `TiffParser`: IFD structure parsing for TIFF files
//! - `PngParser`: PNG chunk-based metadata extraction
//! - `XmpParser`: RDF/XML parsing for sidecar XMP files
//! - Additional parsers for 300+ file formats (roadmap)
//!
//! # Design Pattern: Strategy Pattern
//!
//! The core library uses this trait to implement the Strategy pattern:
//! format detection routes files to the appropriate parser implementation
//! at runtime, allowing the extraction algorithm to vary independently.

#![allow(dead_code)]

use super::file_format::FileFormat;
use super::file_reader_trait::FileReader;
use super::metadata_map::MetadataMap;
use crate::error::Result;

/// Primary interface for format-specific metadata parsers.
///
/// This trait defines the contract that all format parser adapters must implement
/// to integrate with the core metadata extraction engine. Parsers are responsible
/// for reading file structures, locating metadata sections, and converting
/// format-specific data into the unified `MetadataMap` representation.
///
/// # Object Safety
///
/// This trait is object-safe and designed for use with `dyn FormatParser`.
/// The core library stores a registry of `Box<dyn FormatParser>` implementations
/// and dispatches to them based on file format detection.
///
/// # Examples
///
/// ```no_run
/// use exiftool_rs::core::{FormatParser, FileReader, FileFormat, MetadataMap};
/// use exiftool_rs::error::{ExifToolError, Result};
///
/// struct MockJpegParser;
///
/// impl FormatParser for MockJpegParser {
///     fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
///         // Read JPEG SOI marker
///         let header = reader.read(0, 2)
///             .map_err(|e| ExifToolError::IoError(e))?;
///
///         if header != [0xFF, 0xD8] {
///             return Err(ExifToolError::parse_error("Invalid JPEG SOI marker"));
///         }
///
///         // Parse segments and extract metadata
///         let mut metadata = MetadataMap::new();
///         // ... parsing logic ...
///
///         Ok(metadata)
///     }
///
///     fn supports_format(&self, format: FileFormat) -> bool {
///         matches!(format, FileFormat::JPEG)
///     }
/// }
/// ```
///
/// # Implementer Contracts
///
/// Implementations MUST:
/// - Return a complete `MetadataMap` with all extractable tags on success
/// - Return `ExifToolError::ParseError` for malformed file structures
/// - Return `ExifToolError::UnsupportedFormat` for unsupported variants
/// - Not modify the file (read-only operations via `FileReader`)
/// - Be thread-safe if intended for concurrent access
/// - Handle missing or corrupt metadata sections gracefully
///
/// Implementations SHOULD:
/// - Validate file structure (magic bytes, checksums) before parsing
/// - Use `supports_format()` to limit scope to specific formats
/// - Provide detailed error messages in `ParseError::details`
/// - Skip over unrecognized or malformed tags rather than failing
/// - Use lazy evaluation for large embedded resources (thumbnails, etc.)
pub trait FormatParser {
    /// Parses metadata from a file and returns a unified metadata map.
    ///
    /// This method performs the core work of metadata extraction:
    /// reading the file structure via the `FileReader`, locating metadata
    /// sections, parsing format-specific encodings, and converting tags
    /// to the universal `TagValue` representation.
    ///
    /// # Parameters
    ///
    /// - `reader`: Abstraction for reading file data (may be memory-mapped)
    ///
    /// # Returns
    ///
    /// - `Ok(MetadataMap)`: Successfully extracted metadata (may be empty)
    /// - `Err(ExifToolError)`: Parse error, unsupported format, or I/O error
    ///
    /// # Errors
    ///
    /// This method returns an error if:
    /// - File structure is invalid (wrong magic bytes, corrupt headers)
    /// - Required sections are missing or malformed
    /// - I/O errors occur during reading
    /// - Format is recognized but not supported by this parser
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use exiftool_rs::core::{FormatParser, FileReader};
    /// # use exiftool_rs::error::ExifToolError;
    /// # fn example(parser: &dyn FormatParser, reader: &dyn FileReader) -> Result<(), ExifToolError> {
    /// // Parse metadata from file
    /// let metadata = parser.parse(reader)?;
    ///
    /// // Access extracted tags
    /// if let Some(make) = metadata.get_string("Make") {
    ///     println!("Camera make: {}", make);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap>;

    /// Checks if this parser supports a given file format.
    ///
    /// This method enables format-based routing in the core library.
    /// The metadata engine uses file detection to determine the format,
    /// then queries each registered parser to find a compatible implementation.
    ///
    /// # Parameters
    ///
    /// - `format`: The detected or suspected file format
    ///
    /// # Returns
    ///
    /// - `true`: This parser can handle the format
    /// - `false`: This parser does not support the format
    ///
    /// # Design Notes
    ///
    /// Parsers should return `false` for `FileFormat::Unknown` unless they
    /// implement fallback or heuristic-based parsing. Most parsers should
    /// only return `true` for their specific format(s).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use exiftool_rs::core::{FormatParser, FileFormat};
    /// # fn example(parser: &dyn FormatParser) {
    /// if parser.supports_format(FileFormat::JPEG) {
    ///     println!("Parser supports JPEG files");
    /// } else {
    ///     println!("Parser does not support JPEG files");
    /// }
    /// # }
    /// ```
    fn supports_format(&self, format: FileFormat) -> bool;
}
