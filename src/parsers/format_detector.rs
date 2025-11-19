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
//! use oxidex::parsers::format_detector::detect_format;
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

use crate::core::{FileFormat, FileReader};
use crate::parsers::raw;
use std::io;

/// A signature definition for format detection
///
/// This structure describes a file format signature including:
/// - The byte pattern to match
/// - The offset where the pattern should be found
/// - The format to return if matched
#[derive(Debug)]
struct Signature {
    /// Magic bytes to match against
    bytes: &'static [u8],
    /// Offset in the file where signature should be found (0 = file start)
    offset: u64,
    /// Format to return when this signature matches
    format: FileFormat,
}

/// Macro to simplify signature table creation
///
/// Usage: signature!(bytes, offset, format)
/// Example: signature!(b"PNG", 0, FileFormat::PNG)
macro_rules! signature {
    ($bytes:expr, $offset:expr, $format:expr) => {
        Signature {
            bytes: $bytes,
            offset: $offset,
            format: $format,
        }
    };
}

/// Check if bytes at a specific offset match a pattern
///
/// # Arguments
///
/// * `data` - The data buffer to search within
/// * `pattern` - The byte pattern to match
/// * `offset` - The offset within data where pattern should start
///
/// # Returns
///
/// `true` if the pattern matches at the specified offset, `false` otherwise
#[inline]
fn matches_at_offset(data: &[u8], pattern: &[u8], offset: usize) -> bool {
    if offset + pattern.len() > data.len() {
        return false;
    }
    &data[offset..offset + pattern.len()] == pattern
}

/// Check if data starts with any of the provided patterns
///
/// # Arguments
///
/// * `data` - The data buffer to check
/// * `patterns` - Slice of byte patterns to test against
///
/// # Returns
///
/// `true` if data starts with any of the patterns, `false` otherwise
#[inline]
fn starts_with_any(data: &[u8], patterns: &[&[u8]]) -> bool {
    patterns.iter().any(|pattern| data.starts_with(pattern))
}

/// Check if data contains a text pattern within the first N bytes
///
/// # Arguments
///
/// * `data` - The data buffer to search
/// * `pattern` - The text pattern to find
/// * `limit` - Maximum bytes to search from start
///
/// # Returns
///
/// `true` if pattern is found within the first `limit` bytes as valid UTF-8
#[inline]
fn contains_text(data: &[u8], pattern: &str, limit: usize) -> bool {
    if data.len() < limit {
        return false;
    }
    if let Ok(text) = std::str::from_utf8(&data[0..limit]) {
        text.contains(pattern)
    } else {
        false
    }
}

/// Static signature table for simple format detection
///
/// This table contains signatures that can be checked with simple byte matching
/// at fixed offsets. More complex formats requiring additional logic are handled
/// separately in the detect_format function.
///
/// Signatures are ordered from most specific to least specific to ensure
/// correct detection when multiple formats share similar patterns.
static SIMPLE_SIGNATURES: &[Signature] = &[
    // Camera Raw formats with unique signatures
    signature!(
        b"FUJIFILMCCD-RAW ",
        0,
        FileFormat::CameraRaw(raw::RawFormat::FujifilmRAF)
    ),
    signature!(b"FOVb", 0, FileFormat::CameraRaw(raw::RawFormat::SigmaX3F)),
    signature!(
        b"\x00MRM",
        0,
        FileFormat::CameraRaw(raw::RawFormat::MinoltaMRW)
    ),
    // Image formats
    signature!(b"\x89PNG", 0, FileFormat::PNG),
    signature!(b"GIF87a", 0, FileFormat::GIF),
    signature!(b"GIF89a", 0, FileFormat::GIF),
    signature!(b"BM", 0, FileFormat::BMP),
    signature!(b"8BPS", 0, FileFormat::PSD),
    signature!(b"\x00\x00\x01\x00", 0, FileFormat::ICO),
    signature!(b"FLIF", 0, FileFormat::FLIF),
    signature!(b"\x76\x2F\x31\x01", 0, FileFormat::EXR),
    signature!(b"\x42\x50\x47\xFB", 0, FileFormat::BPG),
    signature!(b"\xFF\x0A", 0, FileFormat::JXL),
    // Audio formats
    signature!(b"fLaC", 0, FileFormat::FLAC),
    signature!(b"ID3", 0, FileFormat::MP3),
    signature!(b"FLV", 0, FileFormat::FLV),
    signature!(b"MAC ", 0, FileFormat::APE),
    signature!(b"\x1A\x45\xDF\xA3", 0, FileFormat::MKV),
    signature!(b"OggS", 0, FileFormat::OGG),
    // Document formats
    signature!(b"%PDF", 0, FileFormat::PDF),
    // Archive formats
    signature!(b"PK", 0, FileFormat::ZIP),
    signature!(b"Rar!", 0, FileFormat::RAR),
    signature!(b"\x37\x7A\xBC\xAF\x27\x1C", 0, FileFormat::SevenZ),
    signature!(b"\x1F\x8B", 0, FileFormat::GZ),
    // Font formats
    signature!(b"OTTO", 0, FileFormat::OTF),
    signature!(b"wOFF", 0, FileFormat::WOFF),
    signature!(b"wOF2", 0, FileFormat::WOFF2),
    signature!(b"\x00\x01\x00\x00", 0, FileFormat::TTF),
    signature!(b"true", 0, FileFormat::TTF),
    // Binary formats
    signature!(b"\x7FELF", 0, FileFormat::ELF),
    signature!(b"\x89HDF\x0D\x0A\x1A\x0A", 0, FileFormat::HDF5),
    signature!(b"SIMPLE", 0, FileFormat::FITS),
    signature!(b"BEGIN:VCARD", 0, FileFormat::VCF),
    signature!(b"\x4C\x00\x00\x00", 0, FileFormat::LNK),
    // Archive formats with offset signatures
    signature!(b"ustar", 257, FileFormat::TAR),
    signature!(b"CD001", 32769, FileFormat::ISO),
];

/// Detect TIFF-based formats
///
/// TIFF and TIFF-based raw camera formats share similar byte-order markers
/// but differ in magic numbers. This function consolidates the detection
/// logic for all TIFF variants.
///
/// # TIFF Variants
///
/// - Standard TIFF: II/MM + 0x002A (42)
/// - Panasonic RW2: II + 0x0055 (85)
/// - Olympus ORF: II + "RO" or "RS", MM + "OR"
/// - Canon CR2: II + 0x002A + "CR\x02\x00" at offset 8
/// - Canon CRW: II + 0x001A + "HEAPCCDR" at offset 6
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 16 bytes recommended)
///
/// # Returns
///
/// `Some(FileFormat)` if TIFF variant detected, `None` otherwise
fn detect_tiff_variants(data: &[u8]) -> Option<FileFormat> {
    if data.len() < 4 {
        return None;
    }

    // Canon CR2: Little-endian TIFF with "CR\x02\x00" at offset 8
    if data.len() >= 12
        && data.starts_with(&[0x49, 0x49, 0x2A, 0x00])
        && matches_at_offset(data, b"CR\x02\x00", 8)
    {
        return Some(FileFormat::CameraRaw(raw::RawFormat::CanonCR2));
    }

    // Canon CRW: CIFF format with "II\x1a\x00" + "HEAPCCDR"
    if data.len() >= 14
        && matches_at_offset(data, &[0x49, 0x49, 0x1A, 0x00], 0)
        && matches_at_offset(data, b"HEAPCCDR", 6)
    {
        return Some(FileFormat::CameraRaw(raw::RawFormat::CanonCRW));
    }

    // All TIFF variants (group by byte order for efficiency)
    let tiff_signatures = [
        ([0x49, 0x49, 0x2A, 0x00], "standard LE"),
        ([0x49, 0x49, 0x55, 0x00], "Panasonic RW2"),
        ([0x49, 0x49, 0x52, 0x4F], "Olympus ORF (RO)"),
        ([0x49, 0x49, 0x52, 0x53], "Olympus ORF (RS)"),
        ([0x4D, 0x4D, 0x00, 0x2A], "standard BE"),
        ([0x4D, 0x4D, 0x4F, 0x52], "Olympus ORF (OR)"),
    ];

    for (sig, _desc) in &tiff_signatures {
        if data.starts_with(sig) {
            return Some(FileFormat::TIFF);
        }
    }

    None
}

/// Detect ISO Base Media File Format (BMFF) variants
///
/// Many modern formats use the BMFF container with "ftyp" at offset 4.
/// This function checks the brand identifier to distinguish between:
/// - Canon CR3 (ftypcrx)
/// - AVIF (ftypavif)
/// - HEIF/HEIC (ftyp + HEIF brands)
/// - Generic QuickTime/MP4
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 12 bytes recommended)
///
/// # Returns
///
/// `Some(FileFormat)` if BMFF variant detected, `None` otherwise
fn detect_bmff_variants(data: &[u8]) -> Option<FileFormat> {
    if data.len() < 8 || !matches_at_offset(data, b"ftyp", 4) {
        return None;
    }

    // Check brand identifier at offset 8
    if data.len() < 12 {
        // Has ftyp but not enough bytes for brand
        return Some(FileFormat::QuickTime);
    }

    let brand = &data[8..12];

    // Canon CR3
    if brand == b"crx " {
        return Some(FileFormat::CameraRaw(raw::RawFormat::CanonCR3));
    }

    // AVIF
    if brand == b"avif" {
        return Some(FileFormat::AVIF);
    }

    // HEIF brands
    let heif_brands = [
        b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs", b"mif1",
    ];

    if heif_brands.iter().any(|b| brand == *b) {
        return Some(FileFormat::HEIF);
    }

    // Default to QuickTime/MP4
    Some(FileFormat::QuickTime)
}

/// Detect RIFF-based formats
///
/// RIFF (Resource Interchange File Format) is used by multiple formats:
/// - WAV audio (RIFF...WAVE)
/// - AVI video (RIFF...AVI )
/// - WebP image (RIFF...WEBP)
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 12 bytes recommended)
///
/// # Returns
///
/// `Some(FileFormat)` if RIFF format detected, `None` otherwise
fn detect_riff_formats(data: &[u8]) -> Option<FileFormat> {
    if data.len() < 12 || !data.starts_with(b"RIFF") {
        return None;
    }

    let format_type = &data[8..12];
    match format_type {
        b"WAVE" => Some(FileFormat::WAV),
        b"AVI " => Some(FileFormat::AVI),
        b"WEBP" => Some(FileFormat::WebP),
        _ => None,
    }
}

/// Detect MP3 format via MPEG sync pattern
///
/// MP3 files without ID3 tags start with MPEG frame sync bytes.
/// Valid sync: 0xFF followed by 0xEx where x is not E or F (UTF-16 BOM)
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 2 bytes)
///
/// # Returns
///
/// `true` if MPEG sync pattern detected
fn is_mp3_sync(data: &[u8]) -> bool {
    data.len() >= 2
        && data[0] == 0xFF
        && (data[1] & 0xE0) == 0xE0
        && data[1] != 0xFE
        && data[1] != 0xFF
}

/// Detect AAC format via ADTS sync word
///
/// AAC files use ADTS framing with sync word 0xFFF in first 12 bits.
/// Common patterns: 0xFF 0xF1 or 0xFF 0xF9
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 2 bytes)
///
/// # Returns
///
/// `true` if ADTS sync pattern detected
fn is_aac_adts(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0xFF && (data[1] == 0xF1 || data[1] == 0xF9)
}

/// Detect MPEG Transport Stream (MTS/M2TS) format
///
/// MTS uses sync byte 0x47 repeating every 188 bytes (standard TS)
/// or every 192 bytes (M2TS with 4-byte timestamp header).
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 576 bytes for reliable detection)
///
/// # Returns
///
/// `true` if MTS sync pattern detected
fn is_mts_stream(data: &[u8]) -> bool {
    // Standard TS: 188-byte packets
    if data.len() >= 564 && data[0] == 0x47 && data[188] == 0x47 && data[376] == 0x47 {
        return true;
    }

    // M2TS: 192-byte packets with timestamp
    if data.len() >= 576 && data[4] == 0x47 && data[196] == 0x47 && data[388] == 0x47 {
        return true;
    }

    false
}

/// Detect Opus audio within OGG container
///
/// Opus uses OGG container with "OpusHead" signature at offset 28.
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 36 bytes)
///
/// # Returns
///
/// `Some(FileFormat::OPUS)` if Opus detected, `Some(FileFormat::OGG)` for generic OGG
fn detect_ogg_variant(data: &[u8]) -> Option<FileFormat> {
    if data.len() >= 36 && matches_at_offset(data, b"OpusHead", 28) {
        Some(FileFormat::OPUS)
    } else {
        Some(FileFormat::OGG)
    }
}

/// Detect Portable Executable (PE) format
///
/// PE files start with MZ (DOS stub) followed by PE signature.
/// The e_lfanew field at offset 0x3C points to the PE header.
///
/// # Arguments
///
/// * `data` - Magic bytes buffer
/// * `reader` - File reader for additional validation
///
/// # Returns
///
/// `Some(FileFormat::PE)` if valid PE detected, `None` otherwise
fn detect_pe_format(data: &[u8], reader: &dyn FileReader) -> Option<FileFormat> {
    // Check for MZ signature
    if data.len() < 64 || !data.starts_with(&[0x4D, 0x5A]) {
        return None;
    }

    // Read e_lfanew field at offset 0x3C
    if data.len() < 0x40 {
        return None;
    }

    let e_lfanew = u32::from_le_bytes([data[0x3C], data[0x3D], data[0x3E], data[0x3F]]) as u64;

    // Verify PE signature at e_lfanew offset
    if e_lfanew < reader.size() && e_lfanew + 4 <= reader.size() {
        if let Ok(pe_sig) = reader.read(e_lfanew, 4) {
            if pe_sig == [0x50, 0x45, 0x00, 0x00] {
                return Some(FileFormat::PE);
            }
        }
    }

    None
}

/// Detect ZIP-based document formats
///
/// Many document formats use ZIP containers. This function examines
/// internal structure to distinguish between:
/// - EPUB, DOCX, XLSX, PPTX (Office Open XML)
/// - Pages, Numbers, Keynote (iWork)
/// - Generic ZIP
///
/// # Arguments
///
/// * `reader` - File reader for reading ZIP contents
///
/// # Returns
///
/// `FileFormat` variant for detected format
fn detect_zip_variant(reader: &dyn FileReader) -> FileFormat {
    use std::io::Cursor;
    use zip::ZipArchive;

    let size = reader.size() as usize;
    if let Ok(all_data) = reader.read(0, size) {
        if let Ok(mut archive) = ZipArchive::new(Cursor::new(all_data)) {
            // Check for specific marker files in priority order

            if archive.by_name("mimetype").is_ok() {
                return FileFormat::EPUB;
            }

            if archive.by_name("word/document.xml").is_ok() {
                return FileFormat::DOCX;
            }

            if archive.by_name("xl/workbook.xml").is_ok() {
                return FileFormat::XLSX;
            }

            if archive.by_name("ppt/presentation.xml").is_ok() {
                return FileFormat::PPTX;
            }

            if archive.by_name("Index/Presentation.iwa").is_ok() {
                return FileFormat::Keynote;
            }

            // Numbers and Pages both have Document.iwa, check for Tables
            if archive.by_name("Index/Document.iwa").is_ok() {
                if archive.by_name("Index/Tables/").is_ok() {
                    return FileFormat::Numbers;
                }
                return FileFormat::Pages;
            }
        }
    }

    FileFormat::ZIP
}

/// Detect Mach-O binary format
///
/// Mach-O has several magic numbers for different architectures and endianness.
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 4 bytes)
///
/// # Returns
///
/// `true` if Mach-O magic number detected
fn is_macho(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    let macho_signatures = [
        [0xFE, 0xED, 0xFA, 0xCE],
        [0xFE, 0xED, 0xFA, 0xCF],
        [0xCE, 0xFA, 0xED, 0xFE],
        [0xCF, 0xFA, 0xED, 0xFE],
    ];

    macho_signatures.iter().any(|sig| data.starts_with(sig))
}

/// Detect DWG (AutoCAD Drawing) format
///
/// DWG files have version-specific signatures like "AC1015", "AC1018", etc.
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 6 bytes)
///
/// # Returns
///
/// `true` if DWG signature detected
fn is_dwg(data: &[u8]) -> bool {
    data.len() >= 6 && matches_at_offset(data, b"AC", 0) && data[2] >= b'1' && data[3] >= b'0'
}

/// Detect text-based 3D and interchange formats
///
/// Several formats use text-based representations with distinctive patterns:
/// - DXF: AutoCAD exchange format
/// - OBJ: Wavefront 3D object
/// - GLTF: GL Transmission Format (JSON)
/// - STL: Stereolithography (ASCII variant)
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 100 bytes recommended)
///
/// # Returns
///
/// `Some(FileFormat)` if text format detected, `None` otherwise
fn detect_text_formats(data: &[u8]) -> Option<FileFormat> {
    if data.len() < 100 {
        return None;
    }

    let text = std::str::from_utf8(&data[0..100]).ok()?;

    // DXF: starts with "0\n" and contains "SECTION"
    if text.starts_with("0\n") && text.contains("SECTION") {
        return Some(FileFormat::DXF);
    }

    // OBJ: contains vertex definitions
    if text.contains("v ") || text.contains("vn ") || text.contains("vt ") {
        return Some(FileFormat::OBJ);
    }

    // GLTF: JSON with "asset" field
    if text.contains("\"asset\"") && text.contains("{") {
        return Some(FileFormat::GLTF);
    }

    // STL ASCII: starts with "solid"
    if text.starts_with("solid") {
        return Some(FileFormat::STL);
    }

    None
}

/// Detect Casio CAM proprietary format
///
/// Casio CAM files have a 70-byte proprietary header followed by JPEG data.
/// The header contains "MM" marker at offset 2.
///
/// # Arguments
///
/// * `data` - Initial magic bytes buffer
/// * `reader` - File reader for additional validation
///
/// # Returns
///
/// `Some(FileFormat::CasioCAM)` if detected, `None` otherwise
fn detect_casio_cam(data: &[u8], reader: &dyn FileReader) -> Option<FileFormat> {
    if reader.size() <= 73 {
        return None;
    }

    // Check for JPEG at offset 70
    if let Ok(header_check) = reader.read(70, 3) {
        if header_check.starts_with(&[0xFF, 0xD8, 0xFF]) {
            // Verify "MM" marker at offset 2
            if data.len() >= 4 && data[2] == 0x4D && data[3] == 0x4D {
                return Some(FileFormat::CasioCAM);
            }
        }
    }

    None
}

/// Detects the file format by examining magic bytes.
///
/// This function reads the first 600 bytes of the file (or fewer if the file is smaller)
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
/// This function gracefully handles files smaller than 600 bytes by reading only the
/// available bytes and attempting format detection with the partial data. Empty files
/// return `Ok(FileFormat::Unknown)`.
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::format_detector::detect_format;
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
    // Read magic bytes for detection (600 bytes needed for MTS which requires 3 packets)
    let magic_bytes = match reader.read(0, 600) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            // File is smaller than 600 bytes, read what's available
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

    // Phase 3: Check formats with special detection logic

    // OGG/Opus (already checked in table, but need variant detection)
    if magic_bytes.starts_with(b"OggS") {
        if let Some(format) = detect_ogg_variant(magic_bytes) {
            return Ok(format);
        }
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

    // ZIP-based formats (requires archive inspection)
    if magic_bytes.starts_with(&[0x50, 0x4B]) {
        return Ok(detect_zip_variant(reader));
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

    // Text-based formats (DXF, OBJ, GLTF, STL)
    if let Some(format) = detect_text_formats(magic_bytes) {
        return Ok(format);
    }

    // SVG (XML-based, separate check)
    if contains_text(magic_bytes, "<svg", 100) {
        return Ok(FileFormat::SVG);
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

    // No known format matched
    Ok(FileFormat::Unknown)
}

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
            let end = start.saturating_add(length).min(self.data.len());

            if start > self.data.len() {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "offset beyond end of data",
                ));
            }

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

    // Tests for helper functions
    #[test]
    fn test_matches_at_offset() {
        let data = b"Hello World";
        assert!(matches_at_offset(data, b"Hello", 0));
        assert!(matches_at_offset(data, b"World", 6));
        assert!(!matches_at_offset(data, b"World", 0));
        assert!(!matches_at_offset(data, b"TooLong", 10));
    }

    #[test]
    fn test_starts_with_any() {
        let data = b"Test Data";
        assert!(starts_with_any(data, &[b"Test", b"Data"]));
        assert!(starts_with_any(data, &[b"Wrong", b"Test"]));
        assert!(!starts_with_any(data, &[b"Wrong", b"Data"]));
    }

    #[test]
    fn test_contains_text() {
        let data = b"This is a test string with some content";
        assert!(contains_text(data, "test", 39));
        assert!(contains_text(data, "content", 39));
        assert!(!contains_text(data, "missing", 39));
        assert!(!contains_text(data, "test", 10)); // Not enough bytes
    }
}
