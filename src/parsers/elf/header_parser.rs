//! ELF header parsing
//!
//! This module provides parsing for the ELF file header (Elf32_Ehdr/Elf64_Ehdr),
//! which is always located at the beginning of an ELF file.

use crate::parsers::elf::structures::{ElfHeader, ei_index, elf_class, elf_data};
use nom::{
    IResult,
    bytes::complete::take,
    number::complete::{be_u16, be_u32, be_u64, le_u16, le_u32, le_u64},
};

/// Parses the ELF identification bytes (e_ident)
///
/// The e_ident array is always the first 16 bytes of an ELF file and determines:
/// - Magic bytes (0x7F "ELF")
/// - Class (32-bit or 64-bit)
/// - Data encoding (little-endian or big-endian)
/// - Version
/// - OS/ABI
fn parse_e_ident(input: &[u8]) -> IResult<&[u8], [u8; 16]> {
    let (remaining, bytes) = take(16usize)(input)?;
    let mut ident = [0u8; 16];
    ident.copy_from_slice(bytes);
    Ok((remaining, ident))
}

/// Parses the complete ELF header from the file
///
/// This function handles both ELF32 and ELF64 formats, as well as both
/// little-endian and big-endian byte orders. The format is determined
/// by examining e_ident[EI_CLASS] and e_ident[EI_DATA].
///
/// # Arguments
/// * `input` - Byte slice starting at the beginning of the ELF file
///
/// # Returns
/// * `Ok((remaining, ElfHeader))` - The parsed header and remaining bytes
/// * `Err` - If parsing fails
///
/// # Errors
/// Returns a nom error if:
/// - Input is too short for the header
/// - Invalid ELF class (not 32-bit or 64-bit)
/// - Invalid data encoding (not little-endian or big-endian)
pub fn parse_elf_header(input: &[u8]) -> IResult<&[u8], ElfHeader> {
    // First, parse the identification bytes to determine format
    let (remaining, e_ident) = parse_e_ident(input)?;

    // Determine if this is 32-bit or 64-bit
    let is_64bit = match e_ident[ei_index::EI_CLASS] {
        elf_class::ELFCLASS32 => false,
        elf_class::ELFCLASS64 => true,
        _ => {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Verify,
            )));
        }
    };

    // Determine endianness
    let is_little_endian = match e_ident[ei_index::EI_DATA] {
        elf_data::ELFDATA2LSB => true,
        elf_data::ELFDATA2MSB => false,
        _ => {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Verify,
            )));
        }
    };

    // Parse the rest of the header based on format
    if is_64bit {
        parse_elf64_header_body(remaining, e_ident, is_little_endian)
    } else {
        parse_elf32_header_body(remaining, e_ident, is_little_endian)
    }
}

/// Parses the body of an ELF64 header (after e_ident)
fn parse_elf64_header_body(
    input: &[u8],
    e_ident: [u8; 16],
    is_little_endian: bool,
) -> IResult<&[u8], ElfHeader> {
    if is_little_endian {
        let (input, e_type) = le_u16(input)?;
        let (input, e_machine) = le_u16(input)?;
        let (input, e_version) = le_u32(input)?;
        let (input, e_entry) = le_u64(input)?;
        let (input, e_phoff) = le_u64(input)?;
        let (input, e_shoff) = le_u64(input)?;
        let (input, e_flags) = le_u32(input)?;
        let (input, e_ehsize) = le_u16(input)?;
        let (input, e_phentsize) = le_u16(input)?;
        let (input, e_phnum) = le_u16(input)?;
        let (input, e_shentsize) = le_u16(input)?;
        let (input, e_shnum) = le_u16(input)?;
        let (input, e_shstrndx) = le_u16(input)?;

        Ok((
            input,
            ElfHeader {
                e_ident,
                e_type,
                e_machine,
                e_version,
                e_entry,
                e_phoff,
                e_shoff,
                e_flags,
                e_ehsize,
                e_phentsize,
                e_phnum,
                e_shentsize,
                e_shnum,
                e_shstrndx,
                is_64bit: true,
                is_little_endian: true,
            },
        ))
    } else {
        let (input, e_type) = be_u16(input)?;
        let (input, e_machine) = be_u16(input)?;
        let (input, e_version) = be_u32(input)?;
        let (input, e_entry) = be_u64(input)?;
        let (input, e_phoff) = be_u64(input)?;
        let (input, e_shoff) = be_u64(input)?;
        let (input, e_flags) = be_u32(input)?;
        let (input, e_ehsize) = be_u16(input)?;
        let (input, e_phentsize) = be_u16(input)?;
        let (input, e_phnum) = be_u16(input)?;
        let (input, e_shentsize) = be_u16(input)?;
        let (input, e_shnum) = be_u16(input)?;
        let (input, e_shstrndx) = be_u16(input)?;

        Ok((
            input,
            ElfHeader {
                e_ident,
                e_type,
                e_machine,
                e_version,
                e_entry,
                e_phoff,
                e_shoff,
                e_flags,
                e_ehsize,
                e_phentsize,
                e_phnum,
                e_shentsize,
                e_shnum,
                e_shstrndx,
                is_64bit: true,
                is_little_endian: false,
            },
        ))
    }
}

/// Parses the body of an ELF32 header (after e_ident)
fn parse_elf32_header_body(
    input: &[u8],
    e_ident: [u8; 16],
    is_little_endian: bool,
) -> IResult<&[u8], ElfHeader> {
    if is_little_endian {
        let (input, e_type) = le_u16(input)?;
        let (input, e_machine) = le_u16(input)?;
        let (input, e_version) = le_u32(input)?;
        let (input, e_entry) = le_u32(input)?;
        let (input, e_phoff) = le_u32(input)?;
        let (input, e_shoff) = le_u32(input)?;
        let (input, e_flags) = le_u32(input)?;
        let (input, e_ehsize) = le_u16(input)?;
        let (input, e_phentsize) = le_u16(input)?;
        let (input, e_phnum) = le_u16(input)?;
        let (input, e_shentsize) = le_u16(input)?;
        let (input, e_shnum) = le_u16(input)?;
        let (input, e_shstrndx) = le_u16(input)?;

        Ok((
            input,
            ElfHeader {
                e_ident,
                e_type,
                e_machine,
                e_version,
                e_entry: e_entry as u64,
                e_phoff: e_phoff as u64,
                e_shoff: e_shoff as u64,
                e_flags,
                e_ehsize,
                e_phentsize,
                e_phnum,
                e_shentsize,
                e_shnum,
                e_shstrndx,
                is_64bit: false,
                is_little_endian: true,
            },
        ))
    } else {
        let (input, e_type) = be_u16(input)?;
        let (input, e_machine) = be_u16(input)?;
        let (input, e_version) = be_u32(input)?;
        let (input, e_entry) = be_u32(input)?;
        let (input, e_phoff) = be_u32(input)?;
        let (input, e_shoff) = be_u32(input)?;
        let (input, e_flags) = be_u32(input)?;
        let (input, e_ehsize) = be_u16(input)?;
        let (input, e_phentsize) = be_u16(input)?;
        let (input, e_phnum) = be_u16(input)?;
        let (input, e_shentsize) = be_u16(input)?;
        let (input, e_shnum) = be_u16(input)?;
        let (input, e_shstrndx) = be_u16(input)?;

        Ok((
            input,
            ElfHeader {
                e_ident,
                e_type,
                e_machine,
                e_version,
                e_entry: e_entry as u64,
                e_phoff: e_phoff as u64,
                e_shoff: e_shoff as u64,
                e_flags,
                e_ehsize,
                e_phentsize,
                e_phnum,
                e_shentsize,
                e_shnum,
                e_shstrndx,
                is_64bit: false,
                is_little_endian: false,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::elf::structures::{elf_type, machine_types};

    /// Creates a minimal valid ELF64 little-endian header
    fn create_elf64_le_header() -> Vec<u8> {
        let mut data = Vec::new();

        // e_ident (16 bytes)
        data.extend_from_slice(&[0x7F, b'E', b'L', b'F']); // Magic
        data.push(elf_class::ELFCLASS64); // 64-bit
        data.push(elf_data::ELFDATA2LSB); // Little-endian
        data.push(1); // EV_CURRENT
        data.push(0); // ELFOSABI_SYSV
        data.extend_from_slice(&[0; 8]); // Padding

        // Rest of header (48 bytes for ELF64)
        data.extend_from_slice(&(elf_type::ET_EXEC as u16).to_le_bytes()); // e_type
        data.extend_from_slice(&(machine_types::EM_X86_64 as u16).to_le_bytes()); // e_machine
        data.extend_from_slice(&1u32.to_le_bytes()); // e_version
        data.extend_from_slice(&0x400000u64.to_le_bytes()); // e_entry
        data.extend_from_slice(&64u64.to_le_bytes()); // e_phoff
        data.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
        data.extend_from_slice(&0u32.to_le_bytes()); // e_flags
        data.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
        data.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
        data.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
        data.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize
        data.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
        data.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx

        data
    }

    /// Creates a minimal valid ELF32 little-endian header
    fn create_elf32_le_header() -> Vec<u8> {
        let mut data = Vec::new();

        // e_ident (16 bytes)
        data.extend_from_slice(&[0x7F, b'E', b'L', b'F']); // Magic
        data.push(elf_class::ELFCLASS32); // 32-bit
        data.push(elf_data::ELFDATA2LSB); // Little-endian
        data.push(1); // EV_CURRENT
        data.push(0); // ELFOSABI_SYSV
        data.extend_from_slice(&[0; 8]); // Padding

        // Rest of header (36 bytes for ELF32)
        data.extend_from_slice(&(elf_type::ET_EXEC as u16).to_le_bytes()); // e_type
        data.extend_from_slice(&(machine_types::EM_386 as u16).to_le_bytes()); // e_machine
        data.extend_from_slice(&1u32.to_le_bytes()); // e_version
        data.extend_from_slice(&0x08048000u32.to_le_bytes()); // e_entry
        data.extend_from_slice(&52u32.to_le_bytes()); // e_phoff
        data.extend_from_slice(&0u32.to_le_bytes()); // e_shoff
        data.extend_from_slice(&0u32.to_le_bytes()); // e_flags
        data.extend_from_slice(&52u16.to_le_bytes()); // e_ehsize
        data.extend_from_slice(&32u16.to_le_bytes()); // e_phentsize
        data.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
        data.extend_from_slice(&40u16.to_le_bytes()); // e_shentsize
        data.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
        data.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx

        data
    }

    #[test]
    fn test_parse_elf64_le_header() {
        let data = create_elf64_le_header();
        let result = parse_elf_header(&data);
        assert!(result.is_ok());

        let (remaining, header) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        assert!(header.is_64bit);
        assert!(header.is_little_endian);
        assert_eq!(header.e_type, elf_type::ET_EXEC);
        assert_eq!(header.e_machine, machine_types::EM_X86_64);
        assert_eq!(header.e_entry, 0x400000);
        assert_eq!(header.e_phoff, 64);
        assert_eq!(header.e_ehsize, 64);
        assert_eq!(header.e_phentsize, 56);
        assert_eq!(header.e_phnum, 1);
    }

    #[test]
    fn test_parse_elf32_le_header() {
        let data = create_elf32_le_header();
        let result = parse_elf_header(&data);
        assert!(result.is_ok());

        let (remaining, header) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        assert!(!header.is_64bit);
        assert!(header.is_little_endian);
        assert_eq!(header.e_type, elf_type::ET_EXEC);
        assert_eq!(header.e_machine, machine_types::EM_386);
        assert_eq!(header.e_entry, 0x08048000);
        assert_eq!(header.e_phoff, 52);
        assert_eq!(header.e_ehsize, 52);
        assert_eq!(header.e_phentsize, 32);
        assert_eq!(header.e_phnum, 1);
    }

    #[test]
    fn test_parse_invalid_class() {
        let mut data = create_elf64_le_header();
        data[4] = 0xFF; // Invalid class

        let result = parse_elf_header(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_endian() {
        let mut data = create_elf64_le_header();
        data[5] = 0xFF; // Invalid endianness

        let result = parse_elf_header(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_truncated_header() {
        let data = vec![0x7F, b'E', b'L', b'F', 0x02, 0x01]; // Only 6 bytes
        let result = parse_elf_header(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_header_helper_methods() {
        let data = create_elf64_le_header();
        let (_, header) = parse_elf_header(&data).unwrap();

        assert_eq!(header.class_str(), "64-bit");
        assert_eq!(header.endian_str(), "Little-endian");
        assert_eq!(header.type_str(), "Executable");
        assert_eq!(header.machine_str(), "AMD x86-64");
        assert_eq!(header.osabi_str(), "UNIX System V");
    }
}
