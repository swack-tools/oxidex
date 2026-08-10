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
    /// Verifies the DWG file signature against ExifTool's magic number
    ///
    /// Asks the magic table rather than restating the pattern: the
    /// hand-written version here required only `header[2] >= b'1' &&
    /// header[3] >= b'0'`, which accepts any byte at or past those values --
    /// a plain-text file opening "ACTION" or "ACCESS" passed. Detection's
    /// `is_dwg` carried the identical loose rule, so a file like that was
    /// dispatched to this parser and this gate agreed, even though
    /// `File:FileType` (read from the same magic table this now asks) never
    /// called it DWG.
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 6 {
            return Ok(false);
        }
        let probe_len = reader.size().min(1024) as usize;
        Ok(crate::filetype::matches_magic(
            "DWG",
            reader.read(0, probe_len)?,
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn dwg_header(version: &[u8; 6]) -> Vec<u8> {
        let mut data = version.to_vec();
        data.push(0); // the magic's trailing NUL
        data.resize(64, 0);
        data
    }

    #[test]
    fn accepts_every_documented_version_string() {
        for version in [
            b"AC1012", b"AC1014", b"AC1015", b"AC1018", b"AC1021", b"AC1024", b"AC1027", b"AC1032",
        ] {
            let reader = TestReader::new(dwg_header(version));
            assert!(
                DWGParser::verify_signature(&reader).unwrap(),
                "{:?} should be accepted",
                std::str::from_utf8(version)
            );
        }
    }

    /// `header[2] >= b'1' && header[3] >= b'0'` accepted any byte at or past
    /// those values, so a plain-text file whose first six bytes happened to
    /// be "ACTION" or "ACCESS" passed as DWG. ExifTool's magic requires two
    /// ASCII *digits* followed by a NUL, which none of these have.
    #[test]
    fn rejects_prose_that_the_old_range_check_accepted() {
        for opening in [
            b"ACTION" as &[u8],
            b"ACCESS",
            b"ACQUIRE",
            b"ACME12",
            b"AC1015x", // right prefix, but no trailing NUL
        ] {
            let mut data = opening.to_vec();
            data.resize(64, 0);
            let reader = TestReader::new(data);
            assert!(
                !DWGParser::verify_signature(&reader).unwrap(),
                "{:?} should be rejected",
                std::str::from_utf8(opening)
            );
        }
    }

    /// The detector's `is_dwg` and this gate now both ask the magic table, so
    /// they cannot answer a given header two different ways.
    #[test]
    fn signature_check_agrees_with_the_magic_table() {
        for header in [
            dwg_header(b"AC1015"),
            {
                let mut data = b"ACTION".to_vec();
                data.resize(64, 0);
                data
            },
            {
                let mut data = b"AC9999".to_vec();
                data.push(0);
                data.resize(64, 0);
                data
            },
        ] {
            let reader = TestReader::new(header.clone());
            assert_eq!(
                DWGParser::verify_signature(&reader).unwrap(),
                crate::filetype::matches_magic("DWG", &header),
                "gate and magic table disagree about {header:?}"
            );
        }
    }
}
