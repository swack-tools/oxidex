//! Plain text file parser
//!
//! This parser extracts metadata from plain text files (.txt) including
//! encoding detection, line ending styles, BOM detection, and text statistics.
//!
//! # Format Structure
//!
//! Plain text files are unstructured data but contain useful metadata:
//! - Character encoding (UTF-8, UTF-16LE, UTF-16BE, ASCII, etc.)
//! - Byte Order Mark (BOM) presence
//! - Line ending style (CRLF, LF, CR, or mixed)
//! - Text statistics (line count, word count, character count)
//!
//! # Supported Metadata
//!
//! - FileType: Always "TXT"
//! - FileSize: Size of the file in bytes
//! - MIMEType: "text/plain"
//! - MIMEEncoding: Detected encoding (utf-8, utf-16le, utf-16be, us-ascii, etc.)
//! - ByteOrderMark: "Yes" or "No"
//! - Newlines: Line ending style (Unix LF, Windows CRLF, Mac CR, Mixed, or (none))
//! - LineCount: Number of lines in the file
//! - WordCount: Number of words in the file

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// Maximum bytes to read for analysis (to avoid loading huge files entirely)
const MAX_ANALYSIS_BYTES: usize = 1024 * 1024; // 1MB

/// UTF-8 Byte Order Mark
const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// UTF-16 Little Endian BOM
const UTF16LE_BOM: &[u8] = &[0xFF, 0xFE];

/// UTF-16 Big Endian BOM
const UTF16BE_BOM: &[u8] = &[0xFE, 0xFF];

/// UTF-32 Little Endian BOM
const UTF32LE_BOM: &[u8] = &[0xFF, 0xFE, 0x00, 0x00];

/// UTF-32 Big Endian BOM
const UTF32BE_BOM: &[u8] = &[0x00, 0x00, 0xFE, 0xFF];

/// Detected character encoding
#[derive(Debug, Clone, PartialEq)]
pub enum Encoding {
    /// ASCII (7-bit)
    ASCII,
    /// UTF-8
    UTF8,
    /// UTF-16 Little Endian
    UTF16LE,
    /// UTF-16 Big Endian
    UTF16BE,
    /// UTF-32 Little Endian
    UTF32LE,
    /// UTF-32 Big Endian
    UTF32BE,
    /// Unknown or binary
    Unknown,
}

impl Encoding {
    /// Returns the MIME encoding name for this encoding
    pub fn mime_name(&self) -> &'static str {
        match self {
            Encoding::ASCII => "us-ascii",
            Encoding::UTF8 => "utf-8",
            Encoding::UTF16LE => "utf-16le",
            Encoding::UTF16BE => "utf-16be",
            Encoding::UTF32LE => "utf-32le",
            Encoding::UTF32BE => "utf-32be",
            Encoding::Unknown => "unknown",
        }
    }
}

/// Line ending style
#[derive(Debug, Clone, PartialEq)]
pub enum LineEnding {
    /// Unix/Linux/macOS (LF, \n)
    LF,
    /// Windows (CRLF, \r\n)
    CRLF,
    /// Old Mac (CR, \r)
    CR,
    /// Mixed line endings
    Mixed,
    /// No line endings found
    None,
}

impl LineEnding {
    /// Returns the display name for this line ending style
    pub fn display_name(&self) -> &'static str {
        match self {
            LineEnding::LF => "Unix LF",
            LineEnding::CRLF => "Windows CRLF",
            LineEnding::CR => "Mac CR",
            LineEnding::Mixed => "Mixed",
            LineEnding::None => "(none)",
        }
    }
}

/// Text statistics
#[derive(Debug, Clone, Default)]
pub struct TextStats {
    /// Number of lines
    pub line_count: usize,
    /// Number of words
    pub word_count: usize,
    /// Number of characters
    pub char_count: usize,
}

/// Plain text file parser
pub struct TXTParser;

impl TXTParser {
    /// Detects the character encoding and BOM presence
    ///
    /// # Arguments
    ///
    /// * `data` - File data to analyze
    ///
    /// # Returns
    ///
    /// Tuple of (Encoding, has_bom)
    pub fn detect_encoding(data: &[u8]) -> (Encoding, bool) {
        // Check for UTF-32 BOMs first (4 bytes)
        if data.len() >= 4 {
            if &data[0..4] == UTF32LE_BOM {
                return (Encoding::UTF32LE, true);
            }
            if &data[0..4] == UTF32BE_BOM {
                return (Encoding::UTF32BE, true);
            }
        }

        // Check for UTF-16 BOMs (2 bytes)
        if data.len() >= 2 {
            if &data[0..2] == UTF16LE_BOM {
                // Need to distinguish from UTF-32LE
                if data.len() >= 4 && &data[0..4] != UTF32LE_BOM {
                    return (Encoding::UTF16LE, true);
                }
            }
            if &data[0..2] == UTF16BE_BOM {
                return (Encoding::UTF16BE, true);
            }
        }

        // Check for UTF-8 BOM (3 bytes)
        if data.len() >= 3 && &data[0..3] == UTF8_BOM {
            return (Encoding::UTF8, true);
        }

        // No BOM found, try to detect encoding by content
        if Self::is_ascii(data) {
            return (Encoding::ASCII, false);
        }

        // Try UTF-8 validation
        if std::str::from_utf8(data).is_ok() {
            return (Encoding::UTF8, false);
        }

        // Could add more heuristics for UTF-16 without BOM
        (Encoding::Unknown, false)
    }

    /// Checks if data is valid ASCII (all bytes < 128)
    fn is_ascii(data: &[u8]) -> bool {
        data.iter().all(|&b| b < 128)
    }

    /// Detects line ending style
    ///
    /// # Arguments
    ///
    /// * `text` - UTF-8 text to analyze
    ///
    /// # Returns
    ///
    /// Detected line ending style
    pub fn detect_line_endings(text: &str) -> LineEnding {
        let has_crlf = text.contains("\r\n");
        let has_lf = text.contains('\n') && !has_crlf;
        let has_cr = text.contains('\r') && !has_crlf;

        match (has_crlf, has_lf, has_cr) {
            (true, false, false) => LineEnding::CRLF,
            (false, true, false) => LineEnding::LF,
            (false, false, true) => LineEnding::CR,
            (false, false, false) => LineEnding::None,
            _ => LineEnding::Mixed,
        }
    }

    /// Computes text statistics
    ///
    /// # Arguments
    ///
    /// * `text` - UTF-8 text to analyze
    ///
    /// # Returns
    ///
    /// Text statistics (line count, word count, character count)
    pub fn compute_stats(text: &str) -> TextStats {
        let line_count = if text.is_empty() {
            0
        } else {
            // Count newlines, but ensure at least 1 line if text is not empty
            let newline_count = text.matches('\n').count();
            if newline_count == 0 && !text.is_empty() {
                1
            } else {
                newline_count + if text.ends_with('\n') { 0 } else { 1 }
            }
        };

        let word_count = text.split_whitespace().filter(|s| !s.is_empty()).count();

        let char_count = text.chars().count();

        TextStats {
            line_count,
            word_count,
            char_count,
        }
    }

    /// Parses text file content and extracts metadata
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted text metadata
    /// * `Err(ExifToolError)` - Parse error
    pub fn parse_text_content(reader: &dyn FileReader) -> Result<MetadataMap> {
        let size = reader.size() as usize;
        let read_size = size.min(MAX_ANALYSIS_BYTES);
        let data = reader.read(0, read_size)?;

        let mut metadata = MetadataMap::new();

        // Detect encoding and BOM
        let (encoding, has_bom) = Self::detect_encoding(data);

        metadata.insert(
            "MIMEEncoding".to_string(),
            TagValue::String(encoding.mime_name().to_string()),
        );

        metadata.insert(
            "ByteOrderMark".to_string(),
            TagValue::String(if has_bom { "Yes" } else { "No" }.to_string()),
        );

        // Try to decode as UTF-8 for further analysis
        let text = match encoding {
            Encoding::UTF8 => {
                let start = if has_bom { 3 } else { 0 };
                std::str::from_utf8(&data[start..])
                    .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8: {}", e)))?
            }
            Encoding::ASCII => std::str::from_utf8(data)
                .map_err(|e| ExifToolError::parse_error(format!("Invalid ASCII: {}", e)))?,
            _ => {
                // For other encodings, we can't easily analyze without additional dependencies
                // Just return what we have
                return Ok(metadata);
            }
        };

        // Detect line endings
        let line_ending = Self::detect_line_endings(text);
        metadata.insert(
            "Newlines".to_string(),
            TagValue::String(line_ending.display_name().to_string()),
        );

        // Compute statistics
        let stats = Self::compute_stats(text);
        metadata.insert(
            "LineCount".to_string(),
            TagValue::String(stats.line_count.to_string()),
        );
        metadata.insert(
            "WordCount".to_string(),
            TagValue::String(stats.word_count.to_string()),
        );

        Ok(metadata)
    }
}

impl FormatParser for TXTParser {
    /// Parses a TXT file and extracts metadata
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Successfully extracted metadata
    /// * `Err(ExifToolError)` - Parse error
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("TXT".to_string()));
        metadata.insert(
            "MIMEType".to_string(),
            TagValue::String("text/plain".to_string()),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Parse text content and merge with basic metadata
        let text_metadata = Self::parse_text_content(reader)?;
        for (key, value) in text_metadata {
            metadata.insert(key, value);
        }

        // Add TEXT-specific tag aliases for Worker 23 requirements
        add_text_tag_aliases(&mut metadata);

        Ok(metadata)
    }

    /// Indicates whether this parser supports the given file format
    ///
    /// # Arguments
    ///
    /// * `format` - FileFormat to check
    ///
    /// # Returns
    ///
    /// * `true` if format is TXT
    /// * `false` otherwise
    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::TXT)
    }
}

/// Parses metadata from plain text files.
///
/// This is a convenience function that creates a TXTParser and invokes it.
///
/// # Arguments
///
/// * `reader` - FileReader implementation for accessing file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_txt_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = TXTParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Adds TEXT-specific tag aliases to metadata (Worker 23 requirements)
///
/// Maps generic text metadata to TEXT-specific tags for ExifTool compatibility
/// Worker 23 requires: TEXT:Encoding, TEXT:LineCount, TEXT:CharacterCount,
/// TEXT:WordCount, TEXT:FileSize, TEXT:LastModified, TEXT:HasBOM, TEXT:LineEnding
fn add_text_tag_aliases(metadata: &mut MetadataMap) {
    // Create aliases with TEXT prefix
    let mappings = [
        ("MIMEEncoding", "TEXT:Encoding"),
        ("LineCount", "TEXT:LineCount"),
        ("CharacterCount", "TEXT:CharacterCount"),
        ("WordCount", "TEXT:WordCount"),
        ("FileSize", "TEXT:FileSize"),
        ("ByteOrderMark", "TEXT:HasBOM"),
        ("Newlines", "TEXT:LineEnding"),
    ];

    let mut text_tags = Vec::new();
    for (source, text_tag) in &mappings {
        if let Some(value) = metadata.get(source) {
            text_tags.push((text_tag.to_string(), value.clone()));
        }
    }

    for (key, value) in text_tags {
        metadata.insert(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoding_detection_ascii() {
        let data = b"Hello World";
        let (encoding, has_bom) = TXTParser::detect_encoding(data);
        assert_eq!(encoding, Encoding::ASCII);
        assert!(!has_bom);
    }

    #[test]
    fn test_encoding_detection_utf8_no_bom() {
        let data = "Hello UTF-8 ™ ® ©".as_bytes();
        let (encoding, has_bom) = TXTParser::detect_encoding(data);
        assert_eq!(encoding, Encoding::UTF8);
        assert!(!has_bom);
    }

    #[test]
    fn test_encoding_detection_utf8_with_bom() {
        let data = b"\xEF\xBB\xBFHello UTF-8";
        let (encoding, has_bom) = TXTParser::detect_encoding(data);
        assert_eq!(encoding, Encoding::UTF8);
        assert!(has_bom);
    }

    #[test]
    fn test_encoding_detection_utf16le() {
        let data = b"\xFF\xFEH\x00e\x00l\x00l\x00o\x00";
        let (encoding, has_bom) = TXTParser::detect_encoding(data);
        assert_eq!(encoding, Encoding::UTF16LE);
        assert!(has_bom);
    }

    #[test]
    fn test_encoding_detection_utf16be() {
        let data = b"\xFE\xFF\x00H\x00e\x00l\x00l\x00o";
        let (encoding, has_bom) = TXTParser::detect_encoding(data);
        assert_eq!(encoding, Encoding::UTF16BE);
        assert!(has_bom);
    }

    #[test]
    fn test_line_ending_detection_lf() {
        let text = "Line 1\nLine 2\nLine 3";
        assert_eq!(TXTParser::detect_line_endings(text), LineEnding::LF);
    }

    #[test]
    fn test_line_ending_detection_crlf() {
        let text = "Line 1\r\nLine 2\r\nLine 3";
        assert_eq!(TXTParser::detect_line_endings(text), LineEnding::CRLF);
    }

    #[test]
    fn test_line_ending_detection_cr() {
        let text = "Line 1\rLine 2\rLine 3";
        assert_eq!(TXTParser::detect_line_endings(text), LineEnding::CR);
    }

    #[test]
    fn test_line_ending_detection_none() {
        let text = "Single line no ending";
        assert_eq!(TXTParser::detect_line_endings(text), LineEnding::None);
    }

    #[test]
    fn test_stats_simple() {
        let text = "Hello World\nThis is a test";
        let stats = TXTParser::compute_stats(text);
        assert_eq!(stats.line_count, 2);
        assert_eq!(stats.word_count, 6);
        assert!(stats.char_count > 0);
    }

    #[test]
    fn test_stats_empty() {
        let text = "";
        let stats = TXTParser::compute_stats(text);
        assert_eq!(stats.line_count, 0);
        assert_eq!(stats.word_count, 0);
        assert_eq!(stats.char_count, 0);
    }

    #[test]
    fn test_stats_single_line() {
        let text = "Single line";
        let stats = TXTParser::compute_stats(text);
        assert_eq!(stats.line_count, 1);
        assert_eq!(stats.word_count, 2);
    }
}
