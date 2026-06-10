//! Format detection via magic byte analysis
//!
//! This module provides format detection capabilities for determining file types
//! by examining magic bytes (file signatures) at the beginning of files.
//!
//! # Architectural Role
//!
//! The format detector is part of the **infrastructure layer** and serves as the
//! entry point for the parsing pipeline. It uses the `FileReader` port to read
//! magic bytes and returns a `FileFormat` enum variant that routes to the
//! appropriate format parser.
//!
//! # Supported Formats
//!
//! The detector currently identifies:
//! - JPEG: 0xFF 0xD8 0xFF
//! - TIFF (Little-Endian): 0x49 0x49 0x2A 0x00
//! - TIFF (Big-Endian): 0x4D 0x4D 0x00 0x2A
//! - PNG: 0x89 0x50 0x4E 0x47
//! - FLAC: 0x66 0x4C 0x61 0x43 ("fLaC")
//! - PDF: 0x25 0x50 0x44 0x46
//! - QuickTime/MP4: "ftyp" at bytes 4-7
//!
//! Unknown formats return `FileFormat::Unknown`.
//!
//! # Examples
//!
//! ```no_run
//! use oxidex::parsers::detection::detect_format;
//! use oxidex::io::MMapReader;
//! use std::path::Path;
//!
//! # fn example() -> std::io::Result<()> {
//! let reader = MMapReader::new(Path::new("image.jpg"))?;
//! let format = detect_format(&reader)?;
//! println!("Detected format: {}", format);
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

mod archive;
mod audio;
mod binary;
mod bmff;
mod camera;
mod helpers;
mod riff;
mod signatures;
mod text;
mod tiff;
mod video;
mod x509_der;

use crate::core::{FileFormat, FileReader};
use std::io;

// Re-export detection functions for internal use
use archive::detect_zip_variant;
use audio::{detect_ogg_variant, is_aac_adts, is_mp3_sync};
use binary::{detect_pe_format, is_dwg, is_macho};
use bmff::detect_bmff_variants;
use camera::detect_casio_cam;
use helpers::matches_at_offset;
use riff::detect_riff_formats;
use signatures::SIMPLE_SIGNATURES;
use text::detect_text_formats;
use tiff::detect_tiff_variants;
use video::is_mts_stream;
use x509_der::{looks_like_der_x509, top_level_der_object_len};

const DER_X509_MAX_PROBE_SIZE: usize = 1024 * 1024;
pub(crate) const TEXT_FORMAT_PROBE_SIZE: usize = 64 * 1024;

/// Detects the file format by examining magic bytes.
///
/// This function reads the first 1024 bytes of the file (or fewer if the file is smaller)
/// and matches them against known format signatures using a combination of:
/// 1. Simple signature table lookup
/// 2. Specialized detection functions for complex formats
/// 3. Text-based format detection
///
/// Format detection is performed by checking byte sequences in order from most
/// specific to least specific to avoid false positives.
///
/// # Arguments
///
/// * `reader` - A file reader providing access to file contents via the FileReader port
///
/// # Returns
///
/// * `Ok(FileFormat)` - The detected format, or `FileFormat::Unknown` if unrecognized
/// * `Err(io::Error)` - An I/O error occurred while reading the file
///
/// # Error Handling
///
/// This function gracefully handles files smaller than 1024 bytes by reading only the
/// available bytes and attempting format detection with the partial data. Empty files
/// return `Ok(FileFormat::Unknown)`.
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::detection::detect_format;
/// use oxidex::io::MMapReader;
/// use oxidex::core::FileFormat;
/// use std::path::Path;
///
/// # fn example() -> std::io::Result<()> {
/// let reader = MMapReader::new(Path::new("photo.jpg"))?;
/// let format = detect_format(&reader)?;
///
/// match format {
///     FileFormat::JPEG => println!("JPEG image detected"),
///     FileFormat::PNG => println!("PNG image detected"),
///     FileFormat::TIFF => println!("TIFF image detected"),
///     FileFormat::PDF => println!("PDF document detected"),
///     FileFormat::Unknown => println!("Unknown or unsupported format"),
///     _ => println!("Other format detected"),
/// }
/// # Ok(())
/// # }
/// ```
pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat> {
    // Read 1 KiB to align with EML validation while covering MTS's 3-packet probe.
    let magic_bytes = match reader.read(0, 1024) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            // File is smaller than 1024 bytes, read what's available
            let size = reader.size() as usize;
            if size == 0 {
                return Ok(FileFormat::Unknown);
            }
            reader.read(0, size)?
        }
        Err(e) => return Err(e),
    };

    // Empty file check
    if magic_bytes.is_empty() {
        return Ok(FileFormat::Unknown);
    }

    // Phase 1: Check complex formats that need special handling
    // These must be checked before simple signatures to ensure correct priority

    // TIFF and raw camera formats (many share similar signatures)
    if let Some(format) = detect_tiff_variants(magic_bytes) {
        return Ok(format);
    }

    // ISO Base Media File Format variants (ftyp-based)
    if let Some(format) = detect_bmff_variants(magic_bytes) {
        return Ok(format);
    }

    // RIFF-based formats (WAV, AVI, WebP)
    if let Some(format) = detect_riff_formats(magic_bytes) {
        return Ok(format);
    }

    // ZIP variants require archive inspection before offset-based signatures can claim
    // bytes that happen to appear inside ZIP headers, names, or payloads.
    if magic_bytes.starts_with(&[0x50, 0x4B]) {
        return Ok(detect_zip_variant(reader));
    }

    // Phase 2: Check simple signatures from lookup table
    for sig in SIMPLE_SIGNATURES {
        if sig.offset == 0 {
            // Optimization: most signatures are at offset 0
            if magic_bytes.starts_with(sig.bytes) {
                return Ok(sig.format);
            }
        } else if matches_at_offset(magic_bytes, sig.bytes, sig.offset as usize) {
            return Ok(sig.format);
        }
    }

    // DER X.509 certificates share ASN.1 SEQUENCE prefixes with many formats, so inspect the
    // declared top-level object only. Cap the object size to avoid unbounded reads while allowing
    // large but ordinary certificates.
    if magic_bytes.first() == Some(&0x30) {
        if let Some(der_object_len) = top_level_der_object_len(magic_bytes)
            && der_object_len <= DER_X509_MAX_PROBE_SIZE
            && der_object_len as u64 == reader.size()
        {
            let der_probe = if der_object_len > magic_bytes.len() {
                reader.read(0, der_object_len)?
            } else {
                &magic_bytes[..der_object_len]
            };

            if looks_like_der_x509(der_probe) {
                return Ok(FileFormat::X509);
            }
        }
    }

    // Phase 3: Check formats with special detection logic

    // OGG/Opus (already checked in table, but need variant detection)
    if magic_bytes.starts_with(b"OggS")
        && let Some(format) = detect_ogg_variant(magic_bytes)
    {
        return Ok(format);
    }

    // MP3 (MPEG sync pattern, not in simple table due to bit masking)
    if is_mp3_sync(magic_bytes) {
        return Ok(FileFormat::MP3);
    }

    // AAC (ADTS sync pattern)
    if is_aac_adts(magic_bytes) {
        return Ok(FileFormat::AAC);
    }

    // MTS/M2TS (transport stream sync pattern)
    if is_mts_stream(magic_bytes) {
        return Ok(FileFormat::MTS);
    }

    // PE format (requires DOS stub validation)
    if let Some(format) = detect_pe_format(magic_bytes, reader) {
        return Ok(format);
    }

    // Mach-O (multiple magic numbers)
    if is_macho(magic_bytes) {
        return Ok(FileFormat::MachO);
    }

    // DWG (version-based signature)
    if is_dwg(magic_bytes) {
        return Ok(FileFormat::DWG);
    }

    // SVG must outrank ICS/EML text heuristics, but only when SVG is the XML
    // root element. Email bodies may legitimately embed SVG markup.
    if looks_like_svg_root(magic_bytes) {
        return Ok(FileFormat::SVG);
    }

    if looks_like_xml_plist_root(magic_bytes) {
        return Ok(FileFormat::Plist);
    }

    // Text-based formats need a wider bounded probe for long ICS bodies and EML header blocks.
    let text_probe_len = reader
        .size()
        .min(TEXT_FORMAT_PROBE_SIZE as u64)
        .try_into()
        .unwrap_or(TEXT_FORMAT_PROBE_SIZE);
    let text_probe = if text_probe_len > magic_bytes.len() {
        reader.read(0, text_probe_len)?
    } else {
        &magic_bytes[..text_probe_len]
    };
    if let Some(format) = detect_text_formats(text_probe) {
        return Ok(format);
    }

    // Casio CAM (JPEG at offset 70)
    if let Some(format) = detect_casio_cam(magic_bytes, reader) {
        return Ok(format);
    }

    // JPEG (checked late due to Casio CAM sharing similar pattern)
    if magic_bytes.len() >= 3 && magic_bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok(FileFormat::JPEG);
    }

    // JXL (second variant with longer signature)
    if magic_bytes.len() >= 12
        && matches_at_offset(
            magic_bytes,
            &[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20],
            0,
        )
    {
        return Ok(FileFormat::JXL);
    }

    // Plain text detection (fallback for files that look like text)
    // Check if most bytes are printable ASCII or valid UTF-8
    if is_likely_text(magic_bytes) {
        return Ok(FileFormat::TXT);
    }

    // No known format matched
    Ok(FileFormat::Unknown)
}

fn looks_like_svg_root(data: &[u8]) -> bool {
    let Ok(mut text) = std::str::from_utf8(data) else {
        return false;
    };
    text = text.strip_prefix('\u{feff}').unwrap_or(text);

    loop {
        text = text.trim_start();

        if let Some(rest) = text.strip_prefix("<?xml") {
            let Some(end) = rest.find("?>") else {
                return false;
            };
            text = &rest[end + 2..];
            continue;
        }

        if let Some(rest) = text.strip_prefix("<!--") {
            let Some(end) = rest.find("-->") else {
                return false;
            };
            text = &rest[end + 3..];
            continue;
        }

        if text.starts_with("<!DOCTYPE") {
            let Some(end) = xml_doctype_end(text) else {
                return false;
            };
            text = &text[end + 1..];
            continue;
        }

        break;
    }

    let Some(after_svg) = text.strip_prefix("<svg") else {
        return false;
    };
    after_svg
        .chars()
        .next()
        .is_none_or(|character| character.is_whitespace() || matches!(character, '>' | '/'))
}

fn looks_like_xml_plist_root(data: &[u8]) -> bool {
    let data = &data[..data.len().min(512)];
    let Ok(text) = std::str::from_utf8(data) else {
        return false;
    };

    let Some(rest) = text.strip_prefix("<?xml") else {
        return false;
    };
    let Some(end) = rest.find("?>") else {
        return false;
    };

    let mut text = &rest[end + 2..];
    loop {
        text = text.trim_start();

        if let Some(rest) = text.strip_prefix("<!--") {
            let Some(end) = rest.find("-->") else {
                return false;
            };
            text = &rest[end + 3..];
            continue;
        }

        if text.starts_with("<!DOCTYPE") {
            let Some(end) = xml_doctype_end(text) else {
                return false;
            };
            text = &text[end + 1..];
            continue;
        }

        break;
    }

    let Some(after_plist) = text.strip_prefix("<plist") else {
        return false;
    };
    after_plist
        .chars()
        .next()
        .is_none_or(|character| character.is_whitespace() || matches!(character, '>' | '/'))
}

fn xml_doctype_end(text: &str) -> Option<usize> {
    let mut quote = None;
    let mut subset_depth = 0u32;

    for (index, character) in text.char_indices() {
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
            continue;
        }

        match character {
            '"' | '\'' => quote = Some(character),
            '[' => subset_depth += 1,
            ']' => subset_depth = subset_depth.saturating_sub(1),
            '>' if subset_depth == 0 => return Some(index),
            _ => {}
        }
    }

    None
}

/// Checks if data is likely to be plain text
///
/// Uses heuristics to determine if the data consists primarily of
/// printable characters and valid text encodings.
///
/// # Arguments
///
/// * `data` - Data to check
///
/// # Returns
///
/// `true` if data appears to be text, `false` otherwise
fn is_likely_text(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }

    // Check for UTF-8 BOM
    if data.len() >= 3 && &data[0..3] == b"\xEF\xBB\xBF" {
        return true;
    }

    // Check for UTF-16 BOM
    if data.len() >= 2 && (&data[0..2] == b"\xFF\xFE" || &data[0..2] == b"\xFE\xFF") {
        return true;
    }

    // Try to validate as UTF-8
    if std::str::from_utf8(data).is_ok() {
        // Check if it contains mostly printable characters
        let printable_count = data
            .iter()
            .filter(|&&b| {
                (0x20..0x7F).contains(&b) || // Printable ASCII
                b == b'\t' || b == b'\n' || b == b'\r' // Whitespace
            })
            .count();

        // If at least 95% of characters are printable, consider it text
        let ratio = printable_count as f64 / data.len() as f64;
        return ratio >= 0.95;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_detect_jpeg() {
        let data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::JPEG);
    }

    #[test]
    fn test_detect_tiff_little_endian() {
        let data = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_detect_tiff_big_endian() {
        let data = vec![0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_detect_png() {
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PNG);
    }

    #[test]
    fn test_detect_pdf() {
        let data = vec![0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x34];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PDF);
    }

    #[test]
    fn test_detect_unknown() {
        let data = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_empty_file() {
        let data = vec![];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_file_too_small_one_byte() {
        let data = vec![0xFF];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_file_too_small_two_bytes() {
        let data = vec![0xFF, 0xD8];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_short_file_matches_jpeg() {
        let data = vec![0xFF, 0xD8, 0xFF];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::JPEG);
    }

    #[test]
    fn test_short_file_matches_pdf() {
        let data = vec![0x25, 0x50, 0x44, 0x46];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PDF);
    }

    #[test]
    fn test_jpeg_with_padding() {
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE1];
        data.extend_from_slice(&[0x00; 20]);
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::JPEG);
    }

    #[test]
    fn test_tiff_little_endian_minimal() {
        let data = vec![0x49, 0x49, 0x2A, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_tiff_big_endian_minimal() {
        let data = vec![0x4D, 0x4D, 0x00, 0x2A];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_png_full_signature() {
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PNG);
    }

    #[test]
    fn test_partial_match_not_detected() {
        let data = vec![0xFF, 0xD8, 0x00, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_pdf_with_version() {
        let data = vec![0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x37, 0x0A];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PDF);
    }

    #[test]
    fn test_detect_pe_mz_signature() {
        let mut data = vec![0x4D, 0x5A];
        data.extend_from_slice(&[0x90, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.resize(0x3C, 0x00);
        data.extend_from_slice(&[0x80, 0x00, 0x00, 0x00]);
        data.resize(0x80, 0x00);
        data.extend_from_slice(&[0x50, 0x45, 0x00, 0x00]);

        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PE);
    }

    #[test]
    fn test_detect_pe_with_nt_signature() {
        let mut data = vec![0x4D, 0x5A];
        data.resize(0x3C, 0x00);
        data.extend_from_slice(&[0x40, 0x00, 0x00, 0x00]);
        data.resize(0x40, 0x00);
        data.extend_from_slice(&[0x50, 0x45, 0x00, 0x00]);

        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PE);
    }

    #[test]
    fn test_detect_non_pe_mz_file() {
        let mut data = vec![0x4D, 0x5A];
        data.resize(64, 0x00);

        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }
}
