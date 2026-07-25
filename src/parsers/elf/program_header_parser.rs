//! ELF program header parsing
//!
//! This module provides parsing for the program header table (Elf32_Phdr/Elf64_Phdr).
//! Program headers describe segments used for runtime execution.

use crate::parsers::elf::structures::ProgramHeader;
use nom::{
    IResult,
    number::complete::{be_u32, be_u64, le_u32, le_u64},
};

/// Parses a single ELF64 program header in little-endian format
fn parse_elf64_phdr_le(input: &[u8]) -> IResult<&[u8], ProgramHeader> {
    let (input, p_type) = le_u32(input)?;
    let (input, p_flags) = le_u32(input)?;
    let (input, p_offset) = le_u64(input)?;
    let (input, p_vaddr) = le_u64(input)?;
    let (input, p_paddr) = le_u64(input)?;
    let (input, p_filesz) = le_u64(input)?;
    let (input, p_memsz) = le_u64(input)?;
    let (input, p_align) = le_u64(input)?;

    Ok((
        input,
        ProgramHeader {
            p_type,
            p_flags,
            p_offset,
            p_vaddr,
            p_paddr,
            p_filesz,
            p_memsz,
            p_align,
        },
    ))
}

/// Parses a single ELF64 program header in big-endian format
fn parse_elf64_phdr_be(input: &[u8]) -> IResult<&[u8], ProgramHeader> {
    let (input, p_type) = be_u32(input)?;
    let (input, p_flags) = be_u32(input)?;
    let (input, p_offset) = be_u64(input)?;
    let (input, p_vaddr) = be_u64(input)?;
    let (input, p_paddr) = be_u64(input)?;
    let (input, p_filesz) = be_u64(input)?;
    let (input, p_memsz) = be_u64(input)?;
    let (input, p_align) = be_u64(input)?;

    Ok((
        input,
        ProgramHeader {
            p_type,
            p_flags,
            p_offset,
            p_vaddr,
            p_paddr,
            p_filesz,
            p_memsz,
            p_align,
        },
    ))
}

/// Parses a single ELF32 program header in little-endian format
///
/// Note: ELF32 has a different field order than ELF64 (flags comes after align)
fn parse_elf32_phdr_le(input: &[u8]) -> IResult<&[u8], ProgramHeader> {
    let (input, p_type) = le_u32(input)?;
    let (input, p_offset) = le_u32(input)?;
    let (input, p_vaddr) = le_u32(input)?;
    let (input, p_paddr) = le_u32(input)?;
    let (input, p_filesz) = le_u32(input)?;
    let (input, p_memsz) = le_u32(input)?;
    let (input, p_flags) = le_u32(input)?;
    let (input, p_align) = le_u32(input)?;

    Ok((
        input,
        ProgramHeader {
            p_type,
            p_flags,
            p_offset: p_offset as u64,
            p_vaddr: p_vaddr as u64,
            p_paddr: p_paddr as u64,
            p_filesz: p_filesz as u64,
            p_memsz: p_memsz as u64,
            p_align: p_align as u64,
        },
    ))
}

/// Parses a single ELF32 program header in big-endian format
fn parse_elf32_phdr_be(input: &[u8]) -> IResult<&[u8], ProgramHeader> {
    let (input, p_type) = be_u32(input)?;
    let (input, p_offset) = be_u32(input)?;
    let (input, p_vaddr) = be_u32(input)?;
    let (input, p_paddr) = be_u32(input)?;
    let (input, p_filesz) = be_u32(input)?;
    let (input, p_memsz) = be_u32(input)?;
    let (input, p_flags) = be_u32(input)?;
    let (input, p_align) = be_u32(input)?;

    Ok((
        input,
        ProgramHeader {
            p_type,
            p_flags,
            p_offset: p_offset as u64,
            p_vaddr: p_vaddr as u64,
            p_paddr: p_paddr as u64,
            p_filesz: p_filesz as u64,
            p_memsz: p_memsz as u64,
            p_align: p_align as u64,
        },
    ))
}

/// Parses the program header table
///
/// # Arguments
/// * `input` - Byte slice containing the program header table
/// * `count` - Number of program headers to parse
/// * `is_64bit` - True for ELF64, false for ELF32
/// * `is_little_endian` - True for little-endian, false for big-endian
///
/// # Returns
/// * `Ok((remaining, Vec<ProgramHeader>))` - Parsed program headers
/// * `Err` - If parsing fails
pub fn parse_program_headers(
    input: &[u8],
    num_headers: u16,
    is_64bit: bool,
    is_little_endian: bool,
) -> IResult<&[u8], Vec<ProgramHeader>> {
    let parser: fn(&[u8]) -> IResult<&[u8], ProgramHeader> = match (is_64bit, is_little_endian) {
        (true, true) => parse_elf64_phdr_le,
        (true, false) => parse_elf64_phdr_be,
        (false, true) => parse_elf32_phdr_le,
        (false, false) => parse_elf32_phdr_be,
    };

    // Parse headers one by one
    let mut headers = Vec::with_capacity(num_headers as usize);
    let mut remaining = input;

    for _ in 0..num_headers {
        let (rest, header) = parser(remaining)?;
        headers.push(header);
        remaining = rest;
    }

    Ok((remaining, headers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::elf::structures::{pf_flags, pt_type};

    /// Creates a test ELF64 program header (little-endian)
    fn create_elf64_phdr_le(p_type: u32, p_flags: u32, p_offset: u64, p_filesz: u64) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&p_type.to_le_bytes());
        data.extend_from_slice(&p_flags.to_le_bytes());
        data.extend_from_slice(&p_offset.to_le_bytes());
        data.extend_from_slice(&0x400000u64.to_le_bytes()); // p_vaddr
        data.extend_from_slice(&0x400000u64.to_le_bytes()); // p_paddr
        data.extend_from_slice(&p_filesz.to_le_bytes());
        data.extend_from_slice(&p_filesz.to_le_bytes()); // p_memsz
        data.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align
        data
    }

    /// Creates a test ELF32 program header (little-endian)
    fn create_elf32_phdr_le(p_type: u32, p_flags: u32, p_offset: u32, p_filesz: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&p_type.to_le_bytes());
        data.extend_from_slice(&p_offset.to_le_bytes());
        data.extend_from_slice(&0x08048000u32.to_le_bytes()); // p_vaddr
        data.extend_from_slice(&0x08048000u32.to_le_bytes()); // p_paddr
        data.extend_from_slice(&p_filesz.to_le_bytes());
        data.extend_from_slice(&p_filesz.to_le_bytes()); // p_memsz
        data.extend_from_slice(&p_flags.to_le_bytes());
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // p_align
        data
    }

    #[test]
    fn test_parse_single_elf64_phdr_le() {
        let data =
            create_elf64_phdr_le(pt_type::PT_LOAD, pf_flags::PF_R | pf_flags::PF_X, 0, 0x1000);

        let result = parse_program_headers(&data, 1, true, true);
        assert!(result.is_ok());

        let (remaining, headers) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(headers.len(), 1);

        let ph = &headers[0];
        assert_eq!(ph.p_type, pt_type::PT_LOAD);
        assert_eq!(ph.p_flags, pf_flags::PF_R | pf_flags::PF_X);
        assert_eq!(ph.p_offset, 0);
        assert_eq!(ph.p_filesz, 0x1000);
        assert!(ph.is_load());
        assert!(ph.is_executable());
        assert_eq!(ph.flags_str(), "R-X");
        assert_eq!(ph.type_str(), "LOAD");
    }

    #[test]
    fn test_parse_single_elf32_phdr_le() {
        let data = create_elf32_phdr_le(
            pt_type::PT_LOAD,
            pf_flags::PF_R | pf_flags::PF_W,
            0x1000,
            0x2000,
        );

        let result = parse_program_headers(&data, 1, false, true);
        assert!(result.is_ok());

        let (remaining, headers) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(headers.len(), 1);

        let ph = &headers[0];
        assert_eq!(ph.p_type, pt_type::PT_LOAD);
        assert_eq!(ph.p_flags, pf_flags::PF_R | pf_flags::PF_W);
        assert_eq!(ph.p_offset, 0x1000);
        assert_eq!(ph.p_filesz, 0x2000);
        assert!(ph.is_load());
        assert!(!ph.is_executable());
        assert_eq!(ph.flags_str(), "RW-");
    }

    #[test]
    fn test_parse_multiple_phdrs() {
        let mut data = Vec::new();
        data.extend(create_elf64_phdr_le(
            pt_type::PT_PHDR,
            pf_flags::PF_R,
            64,
            168,
        ));
        data.extend(create_elf64_phdr_le(
            pt_type::PT_INTERP,
            pf_flags::PF_R,
            0x200,
            0x1C,
        ));
        data.extend(create_elf64_phdr_le(
            pt_type::PT_LOAD,
            pf_flags::PF_R | pf_flags::PF_X,
            0,
            0x5000,
        ));

        let result = parse_program_headers(&data, 3, true, true);
        assert!(result.is_ok());

        let (remaining, headers) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(headers.len(), 3);

        assert_eq!(headers[0].p_type, pt_type::PT_PHDR);
        assert_eq!(headers[0].type_str(), "PHDR");

        assert_eq!(headers[1].p_type, pt_type::PT_INTERP);
        assert_eq!(headers[1].type_str(), "INTERP");

        assert_eq!(headers[2].p_type, pt_type::PT_LOAD);
        assert_eq!(headers[2].type_str(), "LOAD");
    }

    #[test]
    fn test_parse_special_segment_types() {
        let mut data = Vec::new();
        data.extend(create_elf64_phdr_le(pt_type::PT_GNU_STACK, 0, 0, 0));
        data.extend(create_elf64_phdr_le(
            pt_type::PT_GNU_RELRO,
            pf_flags::PF_R,
            0x1000,
            0x200,
        ));

        let result = parse_program_headers(&data, 2, true, true);
        assert!(result.is_ok());

        let (_, headers) = result.unwrap();
        assert_eq!(headers[0].type_str(), "GNU_STACK");
        assert_eq!(headers[1].type_str(), "GNU_RELRO");
    }

    #[test]
    fn test_parse_truncated_data() {
        let data = vec![0u8; 32]; // Too short for a full ELF64 phdr (56 bytes)
        let result = parse_program_headers(&data, 1, true, true);
        assert!(result.is_err());
    }
}
