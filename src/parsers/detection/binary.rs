//! Binary format detection
//!
//! Handles detection of binary executable formats including PE, Mach-O, and DWG.

use crate::core::{FileFormat, FileReader};
use crate::io::EndianReader;

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
pub fn detect_pe_format(data: &[u8], reader: &dyn FileReader) -> Option<FileFormat> {
    // Check for MZ signature
    if data.len() < 64 || !data.starts_with(&[0x4D, 0x5A]) {
        return None;
    }

    // Read e_lfanew field at offset 0x3C
    if data.len() < 0x40 {
        return None;
    }

    // PE format uses little-endian byte order
    let header = EndianReader::little_endian(data);
    let e_lfanew = header.u32_at(0x3C).unwrap_or(0) as u64;

    // Verify PE signature at e_lfanew offset
    if e_lfanew < reader.size()
        && e_lfanew + 4 <= reader.size()
        && let Ok(pe_sig) = reader.read(e_lfanew, 4)
        && pe_sig == [0x50, 0x45, 0x00, 0x00]
    {
        return Some(FileFormat::PE);
    }

    None
}

/// Detect Mach-O binary format
///
/// Mach-O has several magic numbers for different architectures and endianness.
/// Also detects FAT/Universal binaries which contain multiple architectures.
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 4 bytes)
///
/// # Returns
///
/// `true` if Mach-O or FAT magic number detected
pub fn is_macho(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    let macho_signatures = [
        // Mach-O 32-bit
        [0xFE, 0xED, 0xFA, 0xCE], // MH_MAGIC (BE)
        [0xCE, 0xFA, 0xED, 0xFE], // MH_CIGAM (LE)
        // Mach-O 64-bit
        [0xFE, 0xED, 0xFA, 0xCF], // MH_MAGIC_64 (BE)
        [0xCF, 0xFA, 0xED, 0xFE], // MH_CIGAM_64 (LE)
        // FAT/Universal binary 32-bit
        [0xCA, 0xFE, 0xBA, 0xBE], // FAT_MAGIC
        [0xBE, 0xBA, 0xFE, 0xCA], // FAT_CIGAM
        // FAT/Universal binary 64-bit
        [0xCA, 0xFE, 0xBA, 0xBF], // FAT_MAGIC_64
        [0xBF, 0xBA, 0xFE, 0xCA], // FAT_CIGAM_64
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
pub fn is_dwg(data: &[u8]) -> bool {
    // ExifTool's magic is `^AC10\d{2}\x00` (nine bytes: "AC10" + two digits +
    // a NUL). The hand-written version this replaced tested only
    // `data[2] >= b'1' && data[3] >= b'0'`, which accepts any byte at or past
    // those values -- a plain-text file opening "ACTION", "ACCESS" or
    // "ACQUIRE" satisfied it and was dispatched to `DWGParser` ahead of every
    // text rule, while the magic table (and so `File:FileType`) correctly
    // declined it. `matches_magic` is also what `DWGParser::verify_signature`
    // asks now, so the two cannot drift apart again.
    crate::filetype::matches_magic("DWG", data)
}
