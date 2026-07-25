//! ELF (Executable and Linkable Format) parser
//!
//! This module provides comprehensive parsing for ELF files (executables, shared libraries,
//! object files) commonly found on Linux, BSD, and other Unix-like systems.
//!
//! # Supported Features
//!
//! - ELF32 and ELF64 formats
//! - Both little-endian and big-endian byte orders
//! - Program headers (PT_LOAD, PT_DYNAMIC, PT_INTERP, PT_NOTE)
//! - Section headers (symbol tables, string tables, dynamic section)
//! - Dynamic linking information (shared objects, rpaths)
//! - Build information (GNU build ID, ABI tags)
//! - Symbol tables (exports and imports)
//!
//! # Architecture
//!
//! The parser is organized into sub-modules following the PE parser pattern:
//! - `structures` - Data structure definitions matching ELF specification
//! - `header_parser` - ELF header parsing (Elf32_Ehdr/Elf64_Ehdr)
//! - `program_header_parser` - Program header table parsing
//! - `section_header_parser` - Section header table parsing
//! - `dynamic_parser` - .dynamic section parsing
//! - `symbol_parser` - Symbol table parsing
//! - `note_parser` - Note section parsing (build ID, ABI info)
//! - `metadata_extractor` - Orchestrates extraction and produces metadata

#![allow(dead_code)]

pub mod dynamic_parser;
pub mod header_parser;
pub mod metadata_extractor;
pub mod note_parser;
pub mod program_header_parser;
pub mod section_header_parser;
pub mod structures;
pub mod symbol_parser;

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap};
use crate::error::{ExifToolError, Result};

/// ELF magic bytes: 0x7F followed by "ELF"
const ELF_MAGIC: &[u8] = &[0x7F, 0x45, 0x4C, 0x46];

/// Parser for ELF (Executable and Linkable Format) files
///
/// Extracts comprehensive metadata from Unix/Linux executable and object files
/// including architecture, sections, symbols, and dynamic linking information.
pub struct ELFParser;

impl ELFParser {
    /// Verifies the ELF file signature (0x7F "ELF")
    ///
    /// # Arguments
    /// * `reader` - File reader providing access to file bytes
    ///
    /// # Returns
    /// * `Ok(true)` if the file has a valid ELF signature
    /// * `Ok(false)` if the file is too small or has an invalid signature
    /// * `Err` if reading fails
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        Ok(header == ELF_MAGIC)
    }
}

impl FormatParser for ELFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid ELF signature"));
        }

        // Delegate to the metadata extractor which orchestrates all parsing
        metadata_extractor::extract_elf_metadata(reader)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::ELF)
    }
}

/// Parses metadata from ELF files.
///
/// This is a convenience wrapper around ELFParser that provides a functional API.
///
/// # Arguments
/// * `reader` - File reader providing access to the ELF file
///
/// # Returns
/// * `Ok(MetadataMap)` containing extracted tags
/// * `Err(String)` with error description on failure
pub fn parse_elf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = ELFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_verify_signature_valid() {
        // Create a mock reader with valid ELF signature
        struct MockReader;
        impl FileReader for MockReader {
            fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
                static DATA: [u8; 16] = [
                    0x7F, 0x45, 0x4C, 0x46, // ELF magic
                    0x02, // 64-bit
                    0x01, // Little endian
                    0x01, // Version
                    0x00, // SYSV ABI
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Padding
                ];
                if offset as usize + length <= DATA.len() {
                    Ok(&DATA[offset as usize..offset as usize + length])
                } else {
                    Err(io::Error::new(io::ErrorKind::InvalidInput, "Out of bounds"))
                }
            }
            fn size(&self) -> u64 {
                16
            }
        }

        let reader = MockReader;
        assert!(ELFParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_invalid() {
        struct MockReader;
        impl FileReader for MockReader {
            fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
                static DATA: [u8; 4] = [0x4D, 0x5A, 0x00, 0x00]; // PE signature
                if offset as usize + length <= DATA.len() {
                    Ok(&DATA[offset as usize..offset as usize + length])
                } else {
                    Err(io::Error::new(io::ErrorKind::InvalidInput, "Out of bounds"))
                }
            }
            fn size(&self) -> u64 {
                4
            }
        }

        let reader = MockReader;
        assert!(!ELFParser::verify_signature(&reader).unwrap());
    }
}
