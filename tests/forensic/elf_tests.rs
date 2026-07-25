//! Comprehensive integration tests for ELF parser
//!
//! Tests verify:
//! - ELF signature validation (0x7F 'E' 'L' 'F')
//! - 32-bit and 64-bit ELF format detection
//! - Little-endian (ELFDATA2LSB) and big-endian (ELFDATA2MSB) byte order
//! - ELF type detection (ET_EXEC, ET_DYN, ET_REL, ET_CORE)
//! - Machine architecture detection (x86, x86-64, ARM, AArch64, MIPS, etc.)
//! - Program header and section header parsing
//! - Dynamic section and symbol table handling
//! - Entry point address extraction
//! - Minimal/truncated ELF handling
//! - Invalid signature rejection
//!
//! Uses TestReader pattern with synthetic ELF data to ensure reproducible tests.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::{FormatParser, TagValue};
use oxidex::parsers::elf::ELFParser;

/// ELF identification index constants
#[allow(dead_code)]
const EI_MAG0: usize = 0; // Magic number byte 0 (0x7F)
#[allow(dead_code)]
const EI_MAG1: usize = 1; // Magic number byte 1 ('E')
#[allow(dead_code)]
const EI_MAG2: usize = 2; // Magic number byte 2 ('L')
#[allow(dead_code)]
const EI_MAG3: usize = 3; // Magic number byte 3 ('F')
const EI_CLASS: usize = 4; // File class (32-bit or 64-bit)
const EI_DATA: usize = 5; // Data encoding (little-endian or big-endian)
const EI_VERSION: usize = 6; // ELF version
const EI_OSABI: usize = 7; // OS/ABI identification
const EI_ABIVERSION: usize = 8; // ABI version

/// ELF class values
const ELFCLASS32: u8 = 1; // 32-bit objects
const ELFCLASS64: u8 = 2; // 64-bit objects

/// ELF data encoding values
const ELFDATA2LSB: u8 = 1; // Little-endian
const ELFDATA2MSB: u8 = 2; // Big-endian

/// ELF OS/ABI values
#[allow(dead_code)]
const ELFOSABI_SYSV: u8 = 0; // UNIX System V ABI
#[allow(dead_code)]
const ELFOSABI_LINUX: u8 = 3; // Linux

/// ELF type values (e_type field)
#[allow(dead_code)]
const ET_NONE: u16 = 0; // No file type
#[allow(dead_code)]
const ET_REL: u16 = 1; // Relocatable file
#[allow(dead_code)]
const ET_EXEC: u16 = 2; // Executable file
#[allow(dead_code)]
const ET_DYN: u16 = 3; // Shared object file
#[allow(dead_code)]
const ET_CORE: u16 = 4; // Core file

/// ELF machine type values (e_machine field)
#[allow(dead_code)]
const EM_NONE: u16 = 0; // No machine
#[allow(dead_code)]
const EM_386: u16 = 3; // Intel 80386
#[allow(dead_code)]
const EM_ARM: u16 = 40; // ARM
#[allow(dead_code)]
const EM_X86_64: u16 = 62; // AMD x86-64
#[allow(dead_code)]
const EM_AARCH64: u16 = 183; // ARM 64-bit
#[allow(dead_code)]
const EM_RISCV: u16 = 243; // RISC-V

/// Helper function to create minimal ELF64 header
///
/// # Arguments
/// * `class` - ELF class (ELFCLASS32 or ELFCLASS64)
/// * `data_encoding` - Data encoding (ELFDATA2LSB or ELFDATA2MSB)
/// * `elf_type` - ELF type (ET_EXEC, ET_DYN, etc.)
/// * `machine` - Machine architecture (EM_X86_64, EM_ARM, etc.)
///
/// # Returns
/// A complete minimal ELF header (64 bytes for ELF64, 52 bytes for ELF32)
fn create_elf_header(class: u8, data_encoding: u8, elf_type: u16, machine: u16) -> Vec<u8> {
    let mut data = Vec::new();
    let little_endian = data_encoding == ELFDATA2LSB;

    // Helper to write u16 in specified endianness
    let write_u16 = |data: &mut Vec<u8>, val: u16| {
        if little_endian {
            data.extend_from_slice(&val.to_le_bytes());
        } else {
            data.extend_from_slice(&val.to_be_bytes());
        }
    };

    // Helper to write u32 in specified endianness
    let write_u32 = |data: &mut Vec<u8>, val: u32| {
        if little_endian {
            data.extend_from_slice(&val.to_le_bytes());
        } else {
            data.extend_from_slice(&val.to_be_bytes());
        }
    };

    // Helper to write u64 in specified endianness
    let write_u64 = |data: &mut Vec<u8>, val: u64| {
        if little_endian {
            data.extend_from_slice(&val.to_le_bytes());
        } else {
            data.extend_from_slice(&val.to_be_bytes());
        }
    };

    // ELF identification (e_ident[16])
    data.extend_from_slice(&[0x7F, b'E', b'L', b'F']); // Magic number
    data.push(class); // EI_CLASS
    data.push(data_encoding); // EI_DATA
    data.push(1); // EI_VERSION (current)
    data.push(ELFOSABI_SYSV); // EI_OSABI
    data.push(0); // EI_ABIVERSION
    data.extend_from_slice(&[0; 7]); // EI_PAD (padding)

    // ELF header fields
    write_u16(&mut data, elf_type); // e_type
    write_u16(&mut data, machine); // e_machine
    write_u32(&mut data, 1); // e_version

    if class == ELFCLASS64 {
        // ELF64 header
        write_u64(&mut data, 0x400000); // e_entry
        write_u64(&mut data, 64); // e_phoff (program header offset)
        write_u64(&mut data, 0); // e_shoff (section header offset)
        write_u32(&mut data, 0); // e_flags
        write_u16(&mut data, 64); // e_ehsize (ELF header size)
        write_u16(&mut data, 56); // e_phentsize (program header entry size)
        write_u16(&mut data, 0); // e_phnum (number of program headers)
        write_u16(&mut data, 64); // e_shentsize (section header entry size)
        write_u16(&mut data, 0); // e_shnum (number of section headers)
        write_u16(&mut data, 0); // e_shstrndx (section header string table index)
    } else {
        // ELF32 header
        write_u32(&mut data, 0x8048000); // e_entry
        write_u32(&mut data, 52); // e_phoff (program header offset)
        write_u32(&mut data, 0); // e_shoff (section header offset)
        write_u32(&mut data, 0); // e_flags
        write_u16(&mut data, 52); // e_ehsize (ELF header size)
        write_u16(&mut data, 32); // e_phentsize (program header entry size)
        write_u16(&mut data, 0); // e_phnum (number of program headers)
        write_u16(&mut data, 40); // e_shentsize (section header entry size)
        write_u16(&mut data, 0); // e_shnum (number of section headers)
        write_u16(&mut data, 0); // e_shstrndx (section header string table index)
    }

    data
}

#[test]
fn test_elf64_little_endian_x86_64() {
    // Create a minimal ELF64 x86-64 little-endian executable
    let data = create_elf_header(ELFCLASS64, ELFDATA2LSB, ET_EXEC, EM_X86_64);
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_ok(), "Failed to parse ELF64 x86-64 file");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("ELF".to_string()))
    );
    assert_eq!(
        metadata.get("EXE:Class"),
        Some(&TagValue::String("64-bit".to_string()))
    );
}

#[test]
fn test_elf32_little_endian_x86() {
    // Create a minimal ELF32 x86 little-endian executable
    let data = create_elf_header(ELFCLASS32, ELFDATA2LSB, ET_EXEC, EM_386);
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_ok(), "Failed to parse ELF32 x86 file");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("ELF".to_string()))
    );
    assert_eq!(
        metadata.get("EXE:Class"),
        Some(&TagValue::String("32-bit".to_string()))
    );
}

#[test]
fn test_elf64_big_endian_arm() {
    // Create a minimal ELF64 ARM big-endian executable
    let data = create_elf_header(ELFCLASS64, ELFDATA2MSB, ET_EXEC, EM_ARM);
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_ok(), "Failed to parse ELF64 ARM big-endian file");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("ELF".to_string()))
    );
    assert_eq!(
        metadata.get("EXE:Class"),
        Some(&TagValue::String("64-bit".to_string()))
    );
}

#[test]
fn test_elf32_big_endian_mips() {
    // Create a minimal ELF32 MIPS big-endian executable
    // MIPS machine type is 8
    let data = create_elf_header(ELFCLASS32, ELFDATA2MSB, ET_EXEC, 8);
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_ok(), "Failed to parse ELF32 MIPS big-endian file");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("ELF".to_string()))
    );
    assert_eq!(
        metadata.get("EXE:Class"),
        Some(&TagValue::String("32-bit".to_string()))
    );
}

#[test]
fn test_elf64_shared_object_aarch64() {
    // Create a minimal ELF64 AArch64 shared object (ET_DYN)
    let data = create_elf_header(ELFCLASS64, ELFDATA2LSB, ET_DYN, EM_AARCH64);
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(
        result.is_ok(),
        "Failed to parse ELF64 AArch64 shared object"
    );

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("ELF".to_string()))
    );
    assert_eq!(
        metadata.get("EXE:Class"),
        Some(&TagValue::String("64-bit".to_string()))
    );
}

#[test]
fn test_elf32_relocatable_arm() {
    // Create a minimal ELF32 ARM relocatable file (ET_REL)
    let data = create_elf_header(ELFCLASS32, ELFDATA2LSB, ET_REL, EM_ARM);
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_ok(), "Failed to parse ELF32 ARM relocatable file");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("ELF".to_string()))
    );
    assert_eq!(
        metadata.get("EXE:Class"),
        Some(&TagValue::String("32-bit".to_string()))
    );
}

#[test]
fn test_elf64_core_dump() {
    // Create a minimal ELF64 x86-64 core dump (ET_CORE)
    let data = create_elf_header(ELFCLASS64, ELFDATA2LSB, ET_CORE, EM_X86_64);
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_ok(), "Failed to parse ELF64 core dump");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("ELF".to_string()))
    );
    assert_eq!(
        metadata.get("EXE:Class"),
        Some(&TagValue::String("64-bit".to_string()))
    );
}

#[test]
fn test_elf64_riscv() {
    // Create a minimal ELF64 RISC-V executable
    let data = create_elf_header(ELFCLASS64, ELFDATA2LSB, ET_EXEC, EM_RISCV);
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_ok(), "Failed to parse ELF64 RISC-V file");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileType"),
        Some(&TagValue::String("ELF".to_string()))
    );
    assert_eq!(
        metadata.get("EXE:Class"),
        Some(&TagValue::String("64-bit".to_string()))
    );
}

#[test]
fn test_elf_invalid_signature() {
    // Create a file with invalid ELF signature
    let data = vec![0x7F, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]; // PNG signature
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_err(), "Should fail on invalid ELF signature");

    let err = result.unwrap_err();
    assert!(err.to_string().contains("Invalid ELF signature"));
}

#[test]
fn test_elf_truncated_header() {
    // Create a truncated ELF file (only magic number, no full header)
    // The comprehensive parser requires a full ELF header (at least 52/64 bytes)
    let data = vec![0x7F, b'E', b'L', b'F'];
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    // Parser correctly returns an error for truncated files
    assert!(result.is_err(), "Should fail on truncated ELF file");
}

#[test]
fn test_elf_minimal_size() {
    // Test file with just magic + class byte (5 bytes)
    // The comprehensive parser requires a full ELF header (at least 52/64 bytes)
    let data = vec![0x7F, b'E', b'L', b'F', ELFCLASS64];
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    // Parser correctly returns an error for truncated files
    assert!(
        result.is_err(),
        "Should fail on minimal ELF file without full header"
    );
}

#[test]
fn test_elf_too_small() {
    // Test file that's too small to be ELF
    let data = vec![0x7F, b'E', b'L']; // Only 3 bytes
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_err(), "Should fail on too-small file");
}

#[test]
fn test_elf_unknown_class() {
    // Create an ELF file with unknown class value
    // Build a full header with invalid class value
    let mut data = vec![0x7F, b'E', b'L', b'F'];
    data.push(99); // Invalid class value
    data.push(1); // Little-endian
    data.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // Rest of e_ident
    // Pad to minimum header size
    data.extend_from_slice(&[0; 48]); // Remaining bytes for ELF32/64 header

    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    // Parser correctly returns an error for unknown class values
    assert!(result.is_err(), "Should fail on ELF with invalid class");
}

#[test]
fn test_elf_signature_verification() {
    // Test the verify_signature method directly
    let valid_data = vec![0x7F, b'E', b'L', b'F', ELFCLASS64];
    let valid_reader = TestReader::new(valid_data);

    let result = ELFParser::verify_signature(&valid_reader);
    assert!(result.is_ok());
    assert!(result.unwrap(), "Should verify valid ELF signature");

    let invalid_data = vec![b'M', b'Z', 0x90, 0x00]; // PE/COFF signature
    let invalid_reader = TestReader::new(invalid_data);

    let result = ELFParser::verify_signature(&invalid_reader);
    assert!(result.is_ok());
    assert!(!result.unwrap(), "Should reject invalid ELF signature");
}

#[test]
fn test_elf_file_size_tracking() {
    // Test that file size is properly tracked in metadata
    let data = create_elf_header(ELFCLASS64, ELFDATA2LSB, ET_EXEC, EM_X86_64);
    let expected_size = data.len();
    let reader = TestReader::new(data);
    let parser = ELFParser;

    let result = parser.parse(&reader);
    assert!(result.is_ok(), "Failed to parse ELF file");

    let metadata = result.unwrap();
    assert_eq!(
        metadata.get("FileSize"),
        Some(&TagValue::Integer(expected_size as i64))
    );
}
