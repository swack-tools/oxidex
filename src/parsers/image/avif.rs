//! AVIF image format parser
//!
//! Implements basic metadata extraction from AVIF (AV1 Image File Format) files.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// AVIF uses ISO BMFF with "ftyp" at offset 4 and "avif" brand
const FTYP_SIGNATURE: &[u8] = b"ftyp";

/// AVIF parser (ISO BMFF-based format)
pub struct AVIFParser;

impl AVIFParser {
    /// Verifies the AVIF file signature by checking for the ISO BMFF structure
    /// with "ftyp" box and "avif" brand identifier
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        let header = reader.read(4, 8)?;
        Ok(&header[0..4] == FTYP_SIGNATURE && &header[4..8] == b"avif")
    }
}

impl FormatParser for AVIFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid AVIF signature"));
        }
        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("AVIF".to_string()));
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::AVIF)
    }
}

/// Parses metadata from AVIF files.
///
/// This is a convenience wrapper around AVIFParser that provides a functional API.
pub fn parse_avif_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = AVIFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
