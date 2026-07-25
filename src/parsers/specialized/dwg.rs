//! AutoCAD DWG format parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// Parser for AutoCAD DWG (Drawing) files
///
/// Extracts metadata from DWG files including version information and file properties.
pub struct DWGParser;

impl DWGParser {
    /// Verifies the DWG file signature by checking for "AC" prefix followed by version number
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 6 {
            return Ok(false);
        }
        let header = reader.read(0, 6)?;
        // DWG versions: AC1012, AC1014, AC1015, AC1018, AC1021, AC1024, AC1027, AC1032
        Ok(&header[0..2] == b"AC" && header[2] >= b'1' && header[3] >= b'0')
    }

    /// Reads the AutoCAD version string from the file header
    pub fn read_version(reader: &dyn FileReader) -> Result<String> {
        if reader.size() < 6 {
            return Ok("Unknown".to_string());
        }
        let version = reader.read(0, 6)?;
        Ok(String::from_utf8_lossy(version).to_string())
    }

    /// Maps DWG version code to friendly AutoCAD release name
    pub fn map_version_to_release(version_code: &str) -> &'static str {
        match version_code {
            "AC1012" => "R13",
            "AC1014" => "R14",
            "AC1015" => "R2000",
            "AC1018" => "R2004",
            "AC1021" => "R2007",
            "AC1024" => "R2010",
            "AC1027" => "R2013",
            "AC1032" => "R2018",
            _ => "Unknown",
        }
    }

    /// Reads security flags from header to detect encryption
    pub fn is_encrypted(reader: &dyn FileReader) -> Result<bool> {
        // Security flags are at bytes 13-17
        if reader.size() < 18 {
            return Ok(false);
        }
        let security_flags = reader.read(13, 5)?;
        // Check if any encryption/password bits are set
        // Bit patterns vary by version, but non-zero typically indicates encryption
        Ok(security_flags.iter().any(|&b| b != 0))
    }

    /// Reads codepage information from header
    pub fn read_codepage(reader: &dyn FileReader) -> Result<Option<u16>> {
        // Codepage is typically at offset 19-20 for R2007+ (AC1021+)
        if reader.size() < 21 {
            return Ok(None);
        }

        let version = Self::read_version(reader)?;
        // Codepage only reliable in R2007+
        // DWG uses little-endian byte order
        if version.as_str() >= "AC1021" {
            let codepage_bytes = reader.read(19, 2)?;
            let codepage_reader = EndianReader::little_endian(codepage_bytes);
            let codepage = codepage_reader.u16_at(0).unwrap_or(0);
            if codepage > 0 {
                return Ok(Some(codepage));
            }
        }
        Ok(None)
    }

    /// Reads preview image information from header
    pub fn read_preview_info(reader: &dyn FileReader) -> Result<Option<(u64, u64)>> {
        // Preview address typically at bytes 13-20 (varies by version)
        if reader.size() < 21 {
            return Ok(None);
        }

        // For R2004+ (AC1018+), preview data location is in header
        // DWG uses little-endian byte order
        let version = Self::read_version(reader)?;
        if version.as_str() >= "AC1018" {
            // Read potential preview address at offset 13
            let preview_bytes = reader.read(13, 8)?;
            let preview_reader = EndianReader::little_endian(preview_bytes);
            let preview_offset = preview_reader.u64_at(0).unwrap_or(0);

            // Validate offset is within file bounds
            if preview_offset > 0 && preview_offset < reader.size() {
                // Try to read preview size (typically follows offset)
                if reader.size() >= 29 {
                    let size_bytes = reader.read(21, 8)?;
                    let size_reader = EndianReader::little_endian(size_bytes);
                    let preview_size = size_reader.u64_at(0).unwrap_or(0);
                    if preview_size > 0 && preview_offset + preview_size <= reader.size() {
                        return Ok(Some((preview_offset, preview_size)));
                    }
                }
            }
        }
        Ok(None)
    }
}

impl FormatParser for DWGParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid DWG signature"));
        }
        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("DWG".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Extract version information
        let version = Self::read_version(reader)?;
        metadata.insert("DWGVersion".to_string(), TagValue::String(version.clone()));

        // Map to friendly release name
        let release = Self::map_version_to_release(&version);
        metadata.insert(
            "AutoCADRelease".to_string(),
            TagValue::String(release.to_string()),
        );

        // Check for encryption
        if let Ok(encrypted) = Self::is_encrypted(reader)
            && encrypted
        {
            metadata.insert("Encrypted".to_string(), TagValue::String("Yes".to_string()));
        }

        // Extract codepage if available
        if let Ok(Some(codepage)) = Self::read_codepage(reader) {
            metadata.insert(
                "CodePage".to_string(),
                TagValue::String(codepage.to_string()),
            );
        }

        // Extract preview image information
        if let Ok(Some((offset, size))) = Self::read_preview_info(reader) {
            metadata.insert(
                "PreviewImageOffset".to_string(),
                TagValue::String(offset.to_string()),
            );
            metadata.insert(
                "PreviewImageSize".to_string(),
                TagValue::String(size.to_string()),
            );
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::DWG)
    }
}

/// Parses metadata from DWG files.
///
/// This is a convenience wrapper around DWGParser that provides a functional API.
pub fn parse_dwg_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = DWGParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
