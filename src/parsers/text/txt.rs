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
//! - Line ending style (CRLF, LF, or CR)
//! - Text statistics (line count, word count, character count)
//!
//! # Supported Metadata
//!
//! - FileType: Always "TXT"
//! - MIMEType: "text/plain"
//! - MIMEEncoding: Detected encoding (utf-8, utf-16le, utf-16be, us-ascii, etc.)
//! - ByteOrderMark: "Yes" or "No"
//! - Newlines: Line ending style (Unix LF, Windows CRLF, Macintosh CR, or (none))
//! - LineCount: Number of lines in the file (single-byte encodings only)
//! - WordCount: Number of words in the file (single-byte encodings only)

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
    /// Single-byte 8-bit text with no C1 control bytes (ISO-8859-1)
    Latin1,
    /// Single-byte 8-bit text using the C1 range (MacRoman, CP1252, ...)
    Unknown8Bit,
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
            Encoding::Latin1 => "iso-8859-1",
            Encoding::Unknown8Bit => "unknown-8bit",
            Encoding::Unknown => "unknown",
        }
    }

    /// Whether ExifTool reports `ByteOrderMark` for this encoding.
    ///
    /// It leaves `$isBOM` undefined -- and so omits the tag entirely -- for
    /// the encodings that have no byte order to mark.
    fn can_carry_bom(&self) -> bool {
        !matches!(
            self,
            Encoding::ASCII | Encoding::Latin1 | Encoding::Unknown8Bit | Encoding::Unknown
        )
    }

    /// The byte pattern surrounding a newline in this multi-byte encoding.
    ///
    /// Returns (leading padding, trailing padding, the CR-LF sequence), the
    /// three parts of `Text.pm`'s newline regexes. `None` for encodings that
    /// are not multi-byte.
    fn multibyte_newline_pattern(&self) -> Option<(&'static [u8], &'static [u8], &'static [u8])> {
        match self {
            Encoding::UTF16LE => Some((b"", b"\0", b"\r\0\n")),
            Encoding::UTF16BE => Some((b"\0", b"", b"\r\0\n")),
            Encoding::UTF32LE => Some((b"", b"\0\0\0", b"\r\0\0\0\n")),
            Encoding::UTF32BE => Some((b"\0\0\0", b"", b"\r\0\0\0\n")),
            _ => None,
        }
    }
}

/// Line ending style
///
/// These are exactly the four values in ExifTool's `Newlines` PrintConv.
/// There is deliberately no "mixed" state: ExifTool names a file after the
/// first newline sequence it contains, however the rest of it is terminated.
#[derive(Debug, Clone, PartialEq)]
pub enum LineEnding {
    /// Unix/Linux/macOS (LF, \n)
    LF,
    /// Windows (CRLF, \r\n)
    CRLF,
    /// Old Mac (CR, \r)
    CR,
    /// No line endings found
    None,
}

impl LineEnding {
    /// Returns the display name for this line ending style
    pub fn display_name(&self) -> &'static str {
        match self {
            LineEnding::LF => "Unix LF",
            LineEnding::CRLF => "Windows CRLF",
            LineEnding::CR => "Macintosh CR",
            LineEnding::None => "(none)",
        }
    }

    /// The byte sequence that separates records for this line ending.
    ///
    /// ExifTool assigns the detected newline to Perl's `$/` before counting
    /// lines, and leaves `$/` at its `"\n"` default when it found none.
    fn separator(&self) -> &'static str {
        match self {
            LineEnding::CRLF => "\r\n",
            LineEnding::CR => "\r",
            LineEnding::LF | LineEnding::None => "\n",
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

        // Not UTF-8, so it is single-byte 8-bit text. ExifTool distinguishes
        // the two cases by the C1 range: a file using only 0xA0-0xFF is
        // reported as iso-8859-1, one that touches 0x80-0x9F (MacRoman,
        // CP1252, ...) as unknown-8bit.
        if data.iter().any(|&byte| (0x80..=0x9F).contains(&byte)) {
            return (Encoding::Unknown8Bit, false);
        }
        (Encoding::Latin1, false)
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
        // ExifTool takes the *first* newline sequence in the file
        // (`$nl = $1 if $$dataPt =~ /(\r\n|\r|\n)/` in Text.pm) and has no
        // notion of a mixed file. Reporting "Mixed" for one -- as this did --
        // emitted a value no ExifTool PrintConv can ever produce.
        let bytes = text.as_bytes();
        for (index, &byte) in bytes.iter().enumerate() {
            match byte {
                b'\r' if bytes.get(index + 1) == Some(&b'\n') => return LineEnding::CRLF,
                b'\r' => return LineEnding::CR,
                b'\n' => return LineEnding::LF,
                _ => {}
            }
        }

        LineEnding::None
    }

    /// Finds the newline sequence in a buffer holding a multi-byte encoding.
    ///
    /// ExifTool matches these against the raw bytes rather than against
    /// decoded text, so the encoding's NUL padding is part of the pattern
    /// (`Text.pm` strips the NULs back out of whatever it captured).
    fn detect_multibyte_line_endings(data: &[u8], encoding: &Encoding) -> LineEnding {
        let Some((leading, trailing, crlf)) = encoding.multibyte_newline_pattern() else {
            return LineEnding::None;
        };

        for start in 0..data.len() {
            let Some(rest) = data[start..].strip_prefix(leading) else {
                continue;
            };
            // Alternation order matters, exactly as in the Perl regex: the
            // CR-LF pair has to be tried before the bare CR it starts with.
            for (candidate, line_ending) in [
                (crlf, LineEnding::CRLF),
                (b"\r".as_slice(), LineEnding::CR),
                (b"\n".as_slice(), LineEnding::LF),
            ] {
                if let Some(tail) = rest.strip_prefix(candidate)
                    && tail.starts_with(trailing)
                {
                    return line_ending;
                }
            }
        }

        LineEnding::None
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
        // Lines are the records left by the file's own separator. Counting
        // '\n' regardless -- as this did -- reported every CR-terminated
        // (classic Macintosh) file as a single line no matter how long.
        let separator = Self::detect_line_endings(text).separator();
        let line_count = if text.is_empty() {
            0
        } else {
            let terminated = text.matches(separator).count();
            terminated + usize::from(!text.ends_with(separator))
        };

        // ExifTool counts runs of `\S` in bytes, so only ASCII whitespace
        // separates words; `split_whitespace` would also break on Unicode
        // spaces such as the non-breaking space Latin-1 puts at 0xA0.
        let word_count = text
            .split(|character: char| {
                matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}')
            })
            .filter(|word| !word.is_empty())
            .count();

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

        // ExifTool only answers the BOM question for encodings that can
        // carry one (`HandleTag(ByteOrderMark => $isBOM) if defined $isBOM`).
        // Reporting "No" for us-ascii or iso-8859-1 states something ExifTool
        // deliberately declines to state.
        if encoding.can_carry_bom() {
            metadata.insert(
                "ByteOrderMark".to_string(),
                TagValue::String(if has_bom { "Yes" } else { "No" }.to_string()),
            );
        }

        // Decode for further analysis. Only a single-byte encoding gets line
        // and word statistics -- `Text.pm` returns before counting once
        // `$isUTF8` is undefined, which is exactly the multi-byte cases.
        let decoded;
        let text = match encoding {
            Encoding::UTF8 => {
                let start = if has_bom { 3 } else { 0 };
                std::str::from_utf8(&data[start..])
                    .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8: {}", e)))?
            }
            Encoding::ASCII => std::str::from_utf8(data)
                .map_err(|e| ExifToolError::parse_error(format!("Invalid ASCII: {}", e)))?,
            // 8-bit text is transcoded from Latin-1 so that its newlines and
            // statistics are reported like any other single-byte file's.
            Encoding::Latin1 | Encoding::Unknown8Bit => {
                decoded = data.iter().map(|&byte| byte as char).collect::<String>();
                &decoded
            }
            Encoding::UTF16LE | Encoding::UTF16BE | Encoding::UTF32LE | Encoding::UTF32BE => {
                // Newlines is *not* gated on a single-byte encoding, and
                // ExifTool finds it in the raw bytes without decoding.
                let line_ending = Self::detect_multibyte_line_endings(data, &encoding);
                metadata.insert(
                    "Newlines".to_string(),
                    TagValue::String(line_ending.display_name().to_string()),
                );
                return Ok(metadata);
            }
            Encoding::Unknown => return Ok(metadata),
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
        // No `FileSize` here. `extract_file_metadata` already records the file's
        // length as `File:FileSize`, formatted the way ExifTool prints it
        // ("785 bytes"); a raw byte count under a second, ungrouped key added a
        // third spelling of one fact and disagreed with the oracle.
        // `operations::drop_redundant_file_size` is the backstop for the other
        // parsers that still do this.

        // Parse text content and merge with basic metadata
        let text_metadata = Self::parse_text_content(reader)?;
        for (key, value) in text_metadata {
            metadata.insert(key, value);
        }

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

// There is deliberately no `add_text_tag_aliases` here any more.
//
// It mirrored seven already-extracted facts into a `TEXT:` group -- TEXT:Encoding,
// TEXT:LineCount, TEXT:CharacterCount, TEXT:WordCount, TEXT:FileSize,
// TEXT:HasBOM, TEXT:LineEnding -- so every one of them was emitted twice, once
// bare and once prefixed. ExifTool 13.59 has no `TEXT` group at all: the family-0
// group for `Image::ExifTool::Text::Main` is `File`, so the oracle reports
// `File:MIMEEncoding`, `File:Newlines`, `File:LineCount`, `File:WordCount` and
// `File:ByteOrderMark`. Across the 20 text-family files in ExifTool's `t/images`
// the aliases produced 101 keys that the oracle never emits under any group, and
// the names it *does* use (`Newlines`, `ByteOrderMark`, `MIMEEncoding`) are not
// even the names they aliased to (`LineEnding`, `HasBOM`, `Encoding`).
//
// Filing these facts under `File:` instead is a separate change, and it now has
// what it was missing. The oracle emits them only for its TXT and CSV types, and
// format dispatch still routes XML, XMP, JSON, RTF, AFM, URL and INX files here,
// so an unconditional move would invent `File:LineCount` on the nine corpus files
// ExifTool gives no such tag. What changed is that the gate is available: since
// #648 and #650 resolved identity from the generated tables, `File:FileType` is
// the oracle's answer on 12 of those 13 files (XMP.xml still reads TXT), so a
// relocation can condition on it. ExifTool's own rule, from `Text.pm`
// `ProcessTXT`: `MIMEEncoding`, `Newlines` and `ByteOrderMark` for TXT *and* CSV;
// `LineCount`/`WordCount` only for TXT, skipped for UTF-16 (`Text5.txt` has none)
// and above 20 MB. Removing the duplicate is correct on its own.

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

    /// ExifTool separates the two single-byte cases by the C1 range: only
    /// 0xA0-0xFF is iso-8859-1, anything touching 0x80-0x9F is unknown-8bit.
    #[test]
    fn test_encoding_detection_latin1() {
        let (encoding, has_bom) = TXTParser::detect_encoding(b"this \xe9 is Latin1\r\n");
        assert_eq!(encoding, Encoding::Latin1);
        assert_eq!(encoding.mime_name(), "iso-8859-1");
        assert!(!has_bom);
    }

    #[test]
    fn test_encoding_detection_unknown_8bit() {
        let (encoding, has_bom) = TXTParser::detect_encoding(b"this \x8e is MacRoman\r");
        assert_eq!(encoding, Encoding::Unknown8Bit);
        assert_eq!(encoding.mime_name(), "unknown-8bit");
        assert!(!has_bom);
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
        // ExifTool's Newlines PrintConv spells this "Macintosh CR".
        assert_eq!(LineEnding::CR.display_name(), "Macintosh CR");
    }

    /// Statistics must still be reported for 8-bit text: the parser used to
    /// return early for anything that was not ASCII or UTF-8.
    #[test]
    fn test_stats_reported_for_eight_bit_text() {
        let reader = crate::test_support::TestReader::new(b"this \xe9 is Latin1\r\n".to_vec());
        let metadata = TXTParser::parse_text_content(&reader).unwrap();
        assert_eq!(metadata.get_string("MIMEEncoding"), Some("iso-8859-1"));
        assert_eq!(metadata.get_string("Newlines"), Some("Windows CRLF"));
        assert_eq!(metadata.get_string("LineCount"), Some("1"));
        assert_eq!(metadata.get_string("WordCount"), Some("4"));
        // iso-8859-1 has no byte order to mark, so ExifTool says nothing.
        assert_eq!(metadata.get_string("ByteOrderMark"), None);
    }

    #[test]
    fn test_line_ending_detection_none() {
        let text = "Single line no ending";
        assert_eq!(TXTParser::detect_line_endings(text), LineEnding::None);
    }

    /// ExifTool names a file after its *first* newline sequence; it has no
    /// "Mixed" value, so a file that mixes styles must not invent one.
    #[test]
    fn test_line_ending_detection_takes_the_first_sequence() {
        assert_eq!(
            TXTParser::detect_line_endings("first\nsecond\r\nthird"),
            LineEnding::LF
        );
        assert_eq!(
            TXTParser::detect_line_endings("first\r\nsecond\nthird"),
            LineEnding::CRLF
        );
        assert_eq!(
            TXTParser::detect_line_endings("first\rsecond\nthird"),
            LineEnding::CR
        );
    }

    /// Records are separated by the file's own newline, so a classic
    /// Macintosh file has as many lines as it has carriage returns -- not the
    /// single line a bare '\n' count reports.
    #[test]
    fn test_line_count_follows_the_detected_separator() {
        assert_eq!(TXTParser::compute_stats("a\rb\rc").line_count, 3);
        assert_eq!(TXTParser::compute_stats("a\rb\rc\r").line_count, 3);
        assert_eq!(TXTParser::compute_stats("a\r\nb\r\n").line_count, 2);
        assert_eq!(TXTParser::compute_stats("a\nb\nc").line_count, 3);
    }

    /// ExifTool counts runs of `\S` over bytes, so Latin-1's non-breaking
    /// space at 0xA0 is part of a word rather than a break between two.
    #[test]
    fn test_word_count_breaks_only_on_ascii_whitespace() {
        assert_eq!(TXTParser::compute_stats("one\u{a0}two three").word_count, 2);
    }

    /// Newlines is reported for the multi-byte encodings as well; only the
    /// line and word statistics are restricted to single-byte files.
    #[test]
    fn test_newlines_reported_for_utf16() {
        let mut data = b"\xfe\xff".to_vec(); // UTF-16BE BOM
        data.extend_from_slice(b"\x00t\x00e\x00x\x00t\x00\n");

        let reader = crate::test_support::TestReader::new(data);
        let metadata = TXTParser::parse_text_content(&reader).unwrap();
        assert_eq!(metadata.get_string("MIMEEncoding"), Some("utf-16be"));
        assert_eq!(metadata.get_string("Newlines"), Some("Unix LF"));
        // Statistics stay absent, as they do in ExifTool.
        assert_eq!(metadata.get_string("LineCount"), None);
        assert_eq!(metadata.get_string("WordCount"), None);
    }

    #[test]
    fn test_newlines_reported_for_utf16le_crlf() {
        let mut data = b"\xff\xfe".to_vec(); // UTF-16LE BOM
        data.extend_from_slice(b"t\x00e\x00\r\x00\n\x00");

        let reader = crate::test_support::TestReader::new(data);
        let metadata = TXTParser::parse_text_content(&reader).unwrap();
        assert_eq!(metadata.get_string("MIMEEncoding"), Some("utf-16le"));
        assert_eq!(metadata.get_string("Newlines"), Some("Windows CRLF"));
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
