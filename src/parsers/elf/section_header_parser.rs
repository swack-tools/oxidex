//! ELF section header parsing
//!
//! This module provides parsing for the section header table (Elf32_Shdr/Elf64_Shdr).
//! Section headers describe the file's sections (.text, .data, .bss, etc.).

use crate::parsers::elf::structures::SectionHeader;
use nom::{
    IResult,
    number::complete::{be_u32, be_u64, le_u32, le_u64},
};

/// Parses a single ELF64 section header in little-endian format
fn parse_elf64_shdr_le(input: &[u8]) -> IResult<&[u8], SectionHeader> {
    let (input, sh_name) = le_u32(input)?;
    let (input, sh_type) = le_u32(input)?;
    let (input, sh_flags) = le_u64(input)?;
    let (input, sh_addr) = le_u64(input)?;
    let (input, sh_offset) = le_u64(input)?;
    let (input, sh_size) = le_u64(input)?;
    let (input, sh_link) = le_u32(input)?;
    let (input, sh_info) = le_u32(input)?;
    let (input, sh_addralign) = le_u64(input)?;
    let (input, sh_entsize) = le_u64(input)?;

    Ok((
        input,
        SectionHeader {
            sh_name,
            name: None, // Resolved later from string table
            sh_type,
            sh_flags,
            sh_addr,
            sh_offset,
            sh_size,
            sh_link,
            sh_info,
            sh_addralign,
            sh_entsize,
        },
    ))
}

/// Parses a single ELF64 section header in big-endian format
fn parse_elf64_shdr_be(input: &[u8]) -> IResult<&[u8], SectionHeader> {
    let (input, sh_name) = be_u32(input)?;
    let (input, sh_type) = be_u32(input)?;
    let (input, sh_flags) = be_u64(input)?;
    let (input, sh_addr) = be_u64(input)?;
    let (input, sh_offset) = be_u64(input)?;
    let (input, sh_size) = be_u64(input)?;
    let (input, sh_link) = be_u32(input)?;
    let (input, sh_info) = be_u32(input)?;
    let (input, sh_addralign) = be_u64(input)?;
    let (input, sh_entsize) = be_u64(input)?;

    Ok((
        input,
        SectionHeader {
            sh_name,
            name: None,
            sh_type,
            sh_flags,
            sh_addr,
            sh_offset,
            sh_size,
            sh_link,
            sh_info,
            sh_addralign,
            sh_entsize,
        },
    ))
}

/// Parses a single ELF32 section header in little-endian format
fn parse_elf32_shdr_le(input: &[u8]) -> IResult<&[u8], SectionHeader> {
    let (input, sh_name) = le_u32(input)?;
    let (input, sh_type) = le_u32(input)?;
    let (input, sh_flags) = le_u32(input)?;
    let (input, sh_addr) = le_u32(input)?;
    let (input, sh_offset) = le_u32(input)?;
    let (input, sh_size) = le_u32(input)?;
    let (input, sh_link) = le_u32(input)?;
    let (input, sh_info) = le_u32(input)?;
    let (input, sh_addralign) = le_u32(input)?;
    let (input, sh_entsize) = le_u32(input)?;

    Ok((
        input,
        SectionHeader {
            sh_name,
            name: None,
            sh_type,
            sh_flags: sh_flags as u64,
            sh_addr: sh_addr as u64,
            sh_offset: sh_offset as u64,
            sh_size: sh_size as u64,
            sh_link,
            sh_info,
            sh_addralign: sh_addralign as u64,
            sh_entsize: sh_entsize as u64,
        },
    ))
}

/// Parses a single ELF32 section header in big-endian format
fn parse_elf32_shdr_be(input: &[u8]) -> IResult<&[u8], SectionHeader> {
    let (input, sh_name) = be_u32(input)?;
    let (input, sh_type) = be_u32(input)?;
    let (input, sh_flags) = be_u32(input)?;
    let (input, sh_addr) = be_u32(input)?;
    let (input, sh_offset) = be_u32(input)?;
    let (input, sh_size) = be_u32(input)?;
    let (input, sh_link) = be_u32(input)?;
    let (input, sh_info) = be_u32(input)?;
    let (input, sh_addralign) = be_u32(input)?;
    let (input, sh_entsize) = be_u32(input)?;

    Ok((
        input,
        SectionHeader {
            sh_name,
            name: None,
            sh_type,
            sh_flags: sh_flags as u64,
            sh_addr: sh_addr as u64,
            sh_offset: sh_offset as u64,
            sh_size: sh_size as u64,
            sh_link,
            sh_info,
            sh_addralign: sh_addralign as u64,
            sh_entsize: sh_entsize as u64,
        },
    ))
}

/// Parses the section header table
///
/// # Arguments
/// * `input` - Byte slice containing the section header table
/// * `num_sections` - Number of section headers to parse
/// * `is_64bit` - True for ELF64, false for ELF32
/// * `is_little_endian` - True for little-endian, false for big-endian
///
/// # Returns
/// * `Ok((remaining, Vec<SectionHeader>))` - Parsed section headers
/// * `Err` - If parsing fails
pub fn parse_section_headers(
    input: &[u8],
    num_sections: u16,
    is_64bit: bool,
    is_little_endian: bool,
) -> IResult<&[u8], Vec<SectionHeader>> {
    let parser: fn(&[u8]) -> IResult<&[u8], SectionHeader> = match (is_64bit, is_little_endian) {
        (true, true) => parse_elf64_shdr_le,
        (true, false) => parse_elf64_shdr_be,
        (false, true) => parse_elf32_shdr_le,
        (false, false) => parse_elf32_shdr_be,
    };

    // Parse sections one by one
    let mut sections = Vec::with_capacity(num_sections as usize);
    let mut remaining = input;

    for _ in 0..num_sections {
        let (rest, section) = parser(remaining)?;
        sections.push(section);
        remaining = rest;
    }

    Ok((remaining, sections))
}

/// Extracts a null-terminated string from a string table
///
/// # Arguments
/// * `strtab` - The string table data
/// * `offset` - Offset into the string table
///
/// # Returns
/// * `Some(String)` - The extracted string
/// * `None` - If offset is out of bounds or string is malformed
pub fn get_string_from_strtab(strtab: &[u8], offset: u32) -> Option<String> {
    if offset as usize >= strtab.len() {
        return None;
    }

    let start = offset as usize;
    let end = strtab[start..]
        .iter()
        .position(|&b| b == 0)
        .map(|pos| start + pos)
        .unwrap_or(strtab.len());

    String::from_utf8(strtab[start..end].to_vec()).ok()
}

/// Resolves section names from the section name string table
///
/// # Arguments
/// * `sections` - Mutable slice of section headers
/// * `strtab` - The section name string table (.shstrtab)
pub fn resolve_section_names(sections: &mut [SectionHeader], strtab: &[u8]) {
    for section in sections.iter_mut() {
        section.name = get_string_from_strtab(strtab, section.sh_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::elf::structures::{sh_flags, sh_type};

    /// Creates a test ELF64 section header (little-endian)
    fn create_elf64_shdr_le(
        sh_name: u32,
        sh_type: u32,
        sh_flags: u64,
        sh_offset: u64,
        sh_size: u64,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&sh_name.to_le_bytes());
        data.extend_from_slice(&sh_type.to_le_bytes());
        data.extend_from_slice(&sh_flags.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes()); // sh_addr
        data.extend_from_slice(&sh_offset.to_le_bytes());
        data.extend_from_slice(&sh_size.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // sh_link
        data.extend_from_slice(&0u32.to_le_bytes()); // sh_info
        data.extend_from_slice(&1u64.to_le_bytes()); // sh_addralign
        data.extend_from_slice(&0u64.to_le_bytes()); // sh_entsize
        data
    }

    /// Creates a test ELF32 section header (little-endian)
    fn create_elf32_shdr_le(
        sh_name: u32,
        sh_type: u32,
        sh_flags: u32,
        sh_offset: u32,
        sh_size: u32,
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&sh_name.to_le_bytes());
        data.extend_from_slice(&sh_type.to_le_bytes());
        data.extend_from_slice(&sh_flags.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // sh_addr
        data.extend_from_slice(&sh_offset.to_le_bytes());
        data.extend_from_slice(&sh_size.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes()); // sh_link
        data.extend_from_slice(&0u32.to_le_bytes()); // sh_info
        data.extend_from_slice(&1u32.to_le_bytes()); // sh_addralign
        data.extend_from_slice(&0u32.to_le_bytes()); // sh_entsize
        data
    }

    #[test]
    fn test_parse_single_elf64_shdr_le() {
        let data = create_elf64_shdr_le(
            1,                                             // name offset
            sh_type::SHT_PROGBITS,                         // type
            sh_flags::SHF_ALLOC | sh_flags::SHF_EXECINSTR, // flags
            0x1000,                                        // offset
            0x500,                                         // size
        );

        let result = parse_section_headers(&data, 1, true, true);
        assert!(result.is_ok());

        let (remaining, sections) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(sections.len(), 1);

        let sh = &sections[0];
        assert_eq!(sh.sh_name, 1);
        assert_eq!(sh.sh_type, sh_type::SHT_PROGBITS);
        assert_eq!(sh.sh_flags, sh_flags::SHF_ALLOC | sh_flags::SHF_EXECINSTR);
        assert_eq!(sh.sh_offset, 0x1000);
        assert_eq!(sh.sh_size, 0x500);
        assert_eq!(sh.type_str(), "PROGBITS");
        assert!(sh.flags_str().contains('A'));
        assert!(sh.flags_str().contains('X'));
    }

    #[test]
    fn test_parse_single_elf32_shdr_le() {
        let data = create_elf32_shdr_le(10, sh_type::SHT_SYMTAB, 0, 0x2000, 0x1000);

        let result = parse_section_headers(&data, 1, false, true);
        assert!(result.is_ok());

        let (_, sections) = result.unwrap();
        assert_eq!(sections.len(), 1);

        let sh = &sections[0];
        assert_eq!(sh.sh_type, sh_type::SHT_SYMTAB);
        assert_eq!(sh.sh_offset, 0x2000);
        assert_eq!(sh.sh_size, 0x1000);
        assert_eq!(sh.type_str(), "SYMTAB");
    }

    #[test]
    fn test_parse_multiple_shdrs() {
        let mut data = Vec::new();
        // NULL section (always first)
        data.extend(create_elf64_shdr_le(0, sh_type::SHT_NULL, 0, 0, 0));
        // .text section
        data.extend(create_elf64_shdr_le(
            1,
            sh_type::SHT_PROGBITS,
            sh_flags::SHF_ALLOC | sh_flags::SHF_EXECINSTR,
            0x1000,
            0x500,
        ));
        // .data section
        data.extend(create_elf64_shdr_le(
            7,
            sh_type::SHT_PROGBITS,
            sh_flags::SHF_ALLOC | sh_flags::SHF_WRITE,
            0x2000,
            0x100,
        ));

        let result = parse_section_headers(&data, 3, true, true);
        assert!(result.is_ok());

        let (_, sections) = result.unwrap();
        assert_eq!(sections.len(), 3);

        assert_eq!(sections[0].sh_type, sh_type::SHT_NULL);
        assert_eq!(sections[0].type_str(), "NULL");

        assert_eq!(sections[1].sh_type, sh_type::SHT_PROGBITS);
        assert_eq!(sections[1].sh_offset, 0x1000);

        assert_eq!(sections[2].sh_type, sh_type::SHT_PROGBITS);
        assert_eq!(sections[2].sh_offset, 0x2000);
    }

    #[test]
    fn test_get_string_from_strtab() {
        // Typical string table: null byte, then ".text\0.data\0.bss\0"
        let strtab = b"\0.text\0.data\0.bss\0.symtab\0";

        assert_eq!(get_string_from_strtab(strtab, 0), Some("".to_string()));
        assert_eq!(get_string_from_strtab(strtab, 1), Some(".text".to_string()));
        assert_eq!(get_string_from_strtab(strtab, 7), Some(".data".to_string()));
        assert_eq!(get_string_from_strtab(strtab, 13), Some(".bss".to_string()));
        assert_eq!(
            get_string_from_strtab(strtab, 18),
            Some(".symtab".to_string())
        );

        // Out of bounds
        assert_eq!(get_string_from_strtab(strtab, 100), None);
    }

    #[test]
    fn test_resolve_section_names() {
        let strtab = b"\0.text\0.data\0";

        let mut sections = vec![
            SectionHeader {
                sh_name: 0,
                name: None,
                sh_type: sh_type::SHT_NULL,
                sh_flags: 0,
                sh_addr: 0,
                sh_offset: 0,
                sh_size: 0,
                sh_link: 0,
                sh_info: 0,
                sh_addralign: 0,
                sh_entsize: 0,
            },
            SectionHeader {
                sh_name: 1,
                name: None,
                sh_type: sh_type::SHT_PROGBITS,
                sh_flags: 0,
                sh_addr: 0,
                sh_offset: 0x1000,
                sh_size: 0x500,
                sh_link: 0,
                sh_info: 0,
                sh_addralign: 0,
                sh_entsize: 0,
            },
            SectionHeader {
                sh_name: 7,
                name: None,
                sh_type: sh_type::SHT_PROGBITS,
                sh_flags: 0,
                sh_addr: 0,
                sh_offset: 0x2000,
                sh_size: 0x100,
                sh_link: 0,
                sh_info: 0,
                sh_addralign: 0,
                sh_entsize: 0,
            },
        ];

        resolve_section_names(&mut sections, strtab);

        assert_eq!(sections[0].name, Some("".to_string()));
        assert_eq!(sections[1].name, Some(".text".to_string()));
        assert_eq!(sections[2].name, Some(".data".to_string()));

        assert_eq!(sections[0].name_str(), "");
        assert_eq!(sections[1].name_str(), ".text");
        assert_eq!(sections[2].name_str(), ".data");
    }

    #[test]
    fn test_section_type_strings() {
        let test_cases = vec![
            (sh_type::SHT_NULL, "NULL"),
            (sh_type::SHT_PROGBITS, "PROGBITS"),
            (sh_type::SHT_SYMTAB, "SYMTAB"),
            (sh_type::SHT_STRTAB, "STRTAB"),
            (sh_type::SHT_RELA, "RELA"),
            (sh_type::SHT_HASH, "HASH"),
            (sh_type::SHT_DYNAMIC, "DYNAMIC"),
            (sh_type::SHT_NOTE, "NOTE"),
            (sh_type::SHT_NOBITS, "NOBITS"),
            (sh_type::SHT_DYNSYM, "DYNSYM"),
            (sh_type::SHT_GNU_HASH, "GNU_HASH"),
        ];

        for (sh_type_val, expected) in test_cases {
            let sh = SectionHeader {
                sh_name: 0,
                name: None,
                sh_type: sh_type_val,
                sh_flags: 0,
                sh_addr: 0,
                sh_offset: 0,
                sh_size: 0,
                sh_link: 0,
                sh_info: 0,
                sh_addralign: 0,
                sh_entsize: 0,
            };
            assert_eq!(sh.type_str(), expected);
        }
    }
}
