//! HEIF/HEIC image format parser
//!
//! HEIF (High Efficiency Image Format) uses ISO BMFF container with various brands:
//! - heic, heix: HEVC (H.265) image sequence
//! - hevc, hevx: HEVC image
//! - heim, heis: HEVC image sequence with auxiliary images
//! - hevm, hevs: HEVC multiview image
//! - mif1: Multi-image format

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

const FTYP_SIGNATURE: &[u8] = b"ftyp";

/// HEIF/HEIC parser
pub struct HEIFParser;

impl HEIFParser {
    /// Verifies HEIF signature by checking ISO BMFF "ftyp" box with HEIF brands
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        let header = reader.read(4, 8)?;

        // Check for "ftyp" at offset 4
        if &header[0..4] != FTYP_SIGNATURE {
            return Ok(false);
        }

        // Check for HEIF-compatible brands: heic, heix, hevc, hevx, heim, heis, hevm, hevs, mif1
        let brand = &header[4..8];
        Ok(brand == b"heic"
            || brand == b"heix"
            || brand == b"hevc"
            || brand == b"hevx"
            || brand == b"heim"
            || brand == b"heis"
            || brand == b"hevm"
            || brand == b"hevs"
            || brand == b"mif1")
    }
}

impl FormatParser for HEIFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid HEIF signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("HEIF".to_string()));

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::HEIF)
    }
}

/// Parses metadata from HEIF/HEIC files.
pub fn parse_heif_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = HEIFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
