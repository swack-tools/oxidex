//! Mach-O (Mach Object) executable format parser
//!
//! This module provides comprehensive parsing for Mach-O files, the native
//! executable format for macOS, iOS, watchOS, and tvOS. It supports both
//! 32-bit and 64-bit formats, as well as FAT/Universal binaries.
//!
//! # Supported Features
//!
//! - **Headers**: CPU type, file type, flags, byte order
//! - **Load Commands**: Segments, dylibs, UUID, version info
//! - **Segments**: __TEXT, __DATA, __LINKEDIT analysis
//! - **Dynamic Libraries**: Dependencies, versions, rpaths
//! - **Version Info**: Min OS version, SDK version, build tools
//! - **Code Signing**: Signature presence, team ID, identifier
//! - **Symbols**: Symbol counts, exports, imports
//!
//! # Architecture
//!
//! The parser follows a modular design:
//!
//! ```text
//! mod.rs                  - Public API, FormatParser impl
//! structures.rs           - Data structures and constants
//! header_parser.rs        - Mach-O and FAT header parsing
//! load_command_parser.rs  - Load command dispatcher
//! segment_parser.rs       - Segment and section analysis
//! dylib_parser.rs         - Dynamic library analysis
//! version_parser.rs       - Version information handling
//! signature_parser.rs     - Code signature parsing
//! symbol_parser.rs        - Symbol table analysis
//! metadata_extractor.rs   - Orchestrates metadata extraction
//! ```

#![allow(dead_code)]

pub mod dylib_parser;
pub mod header_parser;
pub mod load_command_parser;
pub mod metadata_extractor;
pub mod segment_parser;
pub mod signature_parser;
pub mod structures;
pub mod symbol_parser;
pub mod version_parser;

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

use header_parser::{
    is_fat_magic, is_macho_magic, parse_fat_archs, parse_fat_header, parse_mach_header,
};
use load_command_parser::parse_all_load_commands;
use metadata_extractor::{extract_macho_metadata, populate_macho_info};
use signature_parser::parse_code_signature_info;
use structures::{MachOInfo, cpu_type};

// =============================================================================
// MachOParser
// =============================================================================

/// Parser for Mach-O (Mach Object) executable files
///
/// Extracts comprehensive metadata from macOS/iOS executable files including
/// architecture, load commands, dependencies, version info, and code signing.
///
/// # Example
///
/// ```no_run
/// use oxidex::core::{FormatParser, FileReader};
/// use oxidex::parsers::macho::MachOParser;
///
/// fn parse_binary(reader: &dyn FileReader) -> Result<(), Box<dyn std::error::Error>> {
///     let parser = MachOParser;
///     let metadata = parser.parse(reader)?;
///
///     if let Some(cpu_type) = metadata.get_string("EXE:CPUType") {
///         println!("CPU Type: {}", cpu_type);
///     }
///     if let Some(uuid) = metadata.get_string("EXE:UUID") {
///         println!("UUID: {}", uuid);
///     }
///     Ok(())
/// }
/// ```
pub struct MachOParser;

impl MachOParser {
    /// Creates a new MachOParser instance
    pub fn new() -> Self {
        MachOParser
    }

    /// Verifies the Mach-O file signature (supports both 32-bit and 64-bit, big and little endian)
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        Ok(is_macho_magic(header))
    }

    /// Parse a single Mach-O binary (not FAT)
    fn parse_single_macho(
        &self,
        reader: &dyn FileReader,
        offset: u64,
        size: u64,
    ) -> Result<MachOInfo> {
        let mut info = MachOInfo::new();

        // Read header data
        let header_size = 32.min(size as usize);
        let header_data = reader.read(offset, header_size)?;

        // Parse Mach-O header
        let (_, header) = parse_mach_header(header_data).map_err(|e| {
            ExifToolError::parse_error(format!("Failed to parse Mach-O header: {:?}", e))
        })?;

        let is_64bit = header.is_64bit;
        let actual_header_size = header.header_size();

        // Read load commands
        let load_commands_offset = offset + actual_header_size as u64;
        let load_commands_size = header.sizeofcmds as usize;

        if load_commands_offset + load_commands_size as u64 > offset + size {
            return Err(ExifToolError::parse_error(
                "Load commands extend beyond file",
            ));
        }

        let load_commands_data = reader.read(load_commands_offset, load_commands_size)?;

        // Parse all load commands
        let (_, commands) = parse_all_load_commands(load_commands_data, header.ncmds, is_64bit)
            .map_err(|e| {
                ExifToolError::parse_error(format!("Failed to parse load commands: {:?}", e))
            })?;

        info.header = Some(header);
        populate_macho_info(&mut info, &commands);

        // Parse code signature if present
        if let Some(ref cs_cmd) = info.code_signature {
            let cs_offset = offset + cs_cmd.dataoff as u64;
            let cs_size = cs_cmd.datasize as usize;

            if cs_offset + cs_size as u64 <= offset + size
                && let Ok(cs_data) = reader.read(cs_offset, cs_size)
            {
                info.code_signature_info = parse_code_signature_info(cs_data);
            }
        }

        Ok(info)
    }

    /// Parse a FAT/Universal binary and extract metadata from the first architecture
    fn parse_fat_binary(&self, reader: &dyn FileReader) -> Result<MachOInfo> {
        let header_data = reader.read(0, 8)?;

        let (_, fat_header) = parse_fat_header(header_data).map_err(|e| {
            ExifToolError::parse_error(format!("Failed to parse FAT header: {:?}", e))
        })?;

        // Read FAT arch entries
        let arch_entry_size = if fat_header.is_64bit { 32 } else { 20 };
        let archs_size = fat_header.nfat_arch as usize * arch_entry_size;
        let archs_data = reader.read(8, archs_size)?;

        let (_, fat_archs) = parse_fat_archs(
            archs_data,
            fat_header.nfat_arch,
            fat_header.is_64bit,
            fat_header.is_swapped,
        )
        .map_err(|e| ExifToolError::parse_error(format!("Failed to parse FAT archs: {:?}", e)))?;

        if fat_archs.is_empty() {
            return Err(ExifToolError::parse_error(
                "FAT binary contains no architectures",
            ));
        }

        // Find preferred architecture (ARM64 > x86_64 > others)
        let preferred_index = find_preferred_architecture(&fat_archs);
        let arch = &fat_archs[preferred_index];

        // Parse the selected architecture
        let mut info = self.parse_single_macho(reader, arch.offset, arch.size)?;

        // Add FAT binary metadata
        info.is_from_fat = true;
        info.fat_header = Some(fat_header);
        info.fat_archs = fat_archs;
        info.fat_arch_index = preferred_index;

        Ok(info)
    }
}

impl Default for MachOParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatParser for MachOParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if reader.size() < 4 {
            return Err(ExifToolError::parse_error("File too small to be a Mach-O"));
        }

        // Check magic number
        let magic_bytes = reader.read(0, 4)?;
        if !is_macho_magic(magic_bytes) {
            return Err(ExifToolError::parse_error("Invalid Mach-O signature"));
        }

        // Parse based on whether this is a FAT binary
        let info = if is_fat_magic(magic_bytes) {
            self.parse_fat_binary(reader)?
        } else {
            self.parse_single_macho(reader, 0, reader.size())?
        };

        // Extract metadata
        let mut metadata = extract_macho_metadata(&info);

        // Add file size
        metadata.insert(
            "EXE:FileSize".to_string(),
            TagValue::Integer(reader.size() as i64),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::MachO)
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Find the preferred architecture index in a FAT binary
///
/// Preference order: ARM64 > ARM64e > x86_64 > others (first in list)
fn find_preferred_architecture(archs: &[structures::FatArch]) -> usize {
    // First, look for ARM64e
    if let Some(idx) = archs.iter().position(|a| {
        a.cputype == cpu_type::CPU_TYPE_ARM64
            && (a.cpusubtype & 0xFF) == structures::cpu_subtype_arm64::CPU_SUBTYPE_ARM64E
    }) {
        return idx;
    }

    // Then ARM64 (any subtype)
    if let Some(idx) = archs
        .iter()
        .position(|a| a.cputype == cpu_type::CPU_TYPE_ARM64)
    {
        return idx;
    }

    // Then x86_64
    if let Some(idx) = archs
        .iter()
        .position(|a| a.cputype == cpu_type::CPU_TYPE_X86_64)
    {
        return idx;
    }

    // Fall back to first architecture
    0
}

/// Convenience function to parse Mach-O metadata
pub fn parse_macho_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = MachOParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn create_minimal_macho_64() -> Vec<u8> {
        let mut data = Vec::new();

        // Mach-O 64-bit header
        data.extend_from_slice(&structures::magic::MH_MAGIC_64.to_le_bytes()); // magic
        data.extend_from_slice(&(cpu_type::CPU_TYPE_ARM64 as u32).to_le_bytes()); // cputype
        data.extend_from_slice(&0u32.to_le_bytes()); // cpusubtype
        data.extend_from_slice(&structures::file_type::MH_EXECUTE.to_le_bytes()); // filetype
        data.extend_from_slice(&1u32.to_le_bytes()); // ncmds (1 command)
        data.extend_from_slice(&24u32.to_le_bytes()); // sizeofcmds
        data.extend_from_slice(
            &(structures::flags::MH_PIE | structures::flags::MH_TWOLEVEL).to_le_bytes(),
        ); // flags
        data.extend_from_slice(&0u32.to_le_bytes()); // reserved

        // LC_UUID command (24 bytes)
        data.extend_from_slice(&structures::load_command::LC_UUID.to_le_bytes());
        data.extend_from_slice(&24u32.to_le_bytes()); // cmdsize
        // UUID bytes
        data.extend_from_slice(&[
            0x55, 0x0E, 0x84, 0x00, 0xE2, 0x9B, 0x41, 0xD4, 0xA7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ]);

        data
    }

    #[test]
    fn test_verify_signature() {
        let data = create_minimal_macho_64();
        let reader = TestReader::new(data);
        assert!(MachOParser::verify_signature(&reader).unwrap());

        // Test invalid signature
        let invalid = TestReader::new(vec![0x00, 0x00, 0x00, 0x00]);
        assert!(!MachOParser::verify_signature(&invalid).unwrap());
    }

    #[test]
    fn test_parse_minimal_macho() {
        let data = create_minimal_macho_64();
        let reader = TestReader::new(data);
        let parser = MachOParser;

        let result = parser.parse(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();

        // Check basic fields
        assert_eq!(metadata.get_string("EXE:CPUType").unwrap(), "ARM64");
        assert_eq!(metadata.get_string("EXE:FileType").unwrap(), "Executable");
        assert_eq!(metadata.get_integer("EXE:Is64Bit").unwrap(), 1);
        assert_eq!(metadata.get_integer("EXE:IsPIE").unwrap(), 1);
        assert_eq!(
            metadata.get_string("EXE:UUID").unwrap(),
            "550E8400-E29B-41D4-A716-446655440000"
        );
    }

    #[test]
    fn test_supports_format() {
        let parser = MachOParser;
        assert!(parser.supports_format(FileFormat::MachO));
        assert!(!parser.supports_format(FileFormat::JPEG));
        assert!(!parser.supports_format(FileFormat::PE));
    }

    #[test]
    fn test_find_preferred_architecture() {
        use structures::FatArch;

        // ARM64 should be preferred over x86_64
        let archs = vec![
            FatArch {
                cputype: cpu_type::CPU_TYPE_X86_64,
                cpusubtype: 0,
                offset: 0x1000,
                size: 0x10000,
                align: 14,
            },
            FatArch {
                cputype: cpu_type::CPU_TYPE_ARM64,
                cpusubtype: 0,
                offset: 0x20000,
                size: 0x10000,
                align: 14,
            },
        ];
        assert_eq!(find_preferred_architecture(&archs), 1);

        // x86_64 should be preferred if no ARM64
        let archs = vec![
            FatArch {
                cputype: cpu_type::CPU_TYPE_I386,
                cpusubtype: 0,
                offset: 0x1000,
                size: 0x10000,
                align: 14,
            },
            FatArch {
                cputype: cpu_type::CPU_TYPE_X86_64,
                cpusubtype: 0,
                offset: 0x20000,
                size: 0x10000,
                align: 14,
            },
        ];
        assert_eq!(find_preferred_architecture(&archs), 1);
    }

    #[test]
    fn test_parse_macho_metadata_function() {
        let data = create_minimal_macho_64();
        let reader = TestReader::new(data);

        let result = parse_macho_metadata(&reader);
        assert!(result.is_ok());
    }
}
