//! Import directory parser for PE files
//!
//! This module handles parsing of the Import Directory Table (Data Directory index 1)
//! and extraction of imported DLLs and functions.

use crate::parsers::pe::structures::{
    ImageImportDescriptor, ImportFunction, ImportInfo, SectionHeader,
};
use nom::{
    IResult,
    number::complete::{le_u16, le_u32},
};

/// Parse an IMAGE_IMPORT_DESCRIPTOR (20 bytes)
pub fn parse_import_descriptor(input: &[u8]) -> IResult<&[u8], ImageImportDescriptor> {
    let (input, original_first_thunk) = le_u32(input)?;
    let (input, time_date_stamp) = le_u32(input)?;
    let (input, forwarder_chain) = le_u32(input)?;
    let (input, name) = le_u32(input)?;
    let (input, first_thunk) = le_u32(input)?;

    Ok((
        input,
        ImageImportDescriptor {
            original_first_thunk,
            time_date_stamp,
            forwarder_chain,
            name,
            first_thunk,
        },
    ))
}

/// Convert RVA to file offset using section headers
pub fn rva_to_file_offset(rva: u32, sections: &[SectionHeader]) -> Option<u64> {
    for section in sections {
        let section_start = section.virtual_address;
        let section_end = section_start + section.virtual_size;

        if rva >= section_start && rva < section_end {
            let offset_in_section = rva - section_start;
            return Some(section.pointer_to_raw_data as u64 + offset_in_section as u64);
        }
    }
    None
}

/// Read a null-terminated string from a byte slice
pub fn read_null_terminated_string(data: &[u8]) -> String {
    let null_pos = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..null_pos]).to_string()
}

/// Parse import name table entry (2-byte hint + null-terminated name)
pub fn parse_import_by_name(data: &[u8]) -> IResult<&[u8], (u16, String)> {
    let (input, hint) = le_u16(data)?;
    let name = read_null_terminated_string(input);
    Ok((input, (hint, name)))
}

/// Parse a thunk value (32-bit or 64-bit)
/// Returns (is_ordinal, ordinal_or_rva)
pub fn parse_thunk_32(data: &[u8]) -> IResult<&[u8], (bool, u32)> {
    let (input, value) = le_u32(data)?;
    let is_ordinal = (value & 0x80000000) != 0;
    let ordinal_or_rva = if is_ordinal {
        value & 0xFFFF // Lower 16 bits for ordinal
    } else {
        value // RVA for import by name
    };
    Ok((input, (is_ordinal, ordinal_or_rva)))
}

/// Parse a thunk value (64-bit)
pub fn parse_thunk_64(data: &[u8]) -> IResult<&[u8], (bool, u64)> {
    let (input, value_low) = le_u32(data)?;
    let (input, value_high) = le_u32(input)?;
    let value = (value_high as u64) << 32 | value_low as u64;
    let is_ordinal = (value & 0x8000000000000000) != 0;
    let ordinal_or_rva = if is_ordinal {
        value & 0xFFFF // Lower 16 bits for ordinal
    } else {
        value // RVA for import by name
    };
    Ok((input, (is_ordinal, ordinal_or_rva)))
}

/// Parse imports for a single DLL
///
/// # Arguments
/// * `reader` - File reader for accessing file data
/// * `descriptor` - Import descriptor for this DLL
/// * `sections` - Section headers for RVA to file offset conversion
/// * `is_pe32_plus` - Whether this is PE32+ (64-bit) format
/// * `max_functions` - Maximum number of functions to extract (to avoid huge lists)
pub fn parse_dll_imports(
    reader: &dyn crate::core::FileReader,
    descriptor: &ImageImportDescriptor,
    sections: &[SectionHeader],
    is_pe32_plus: bool,
    max_functions: usize,
) -> Option<ImportInfo> {
    // Read DLL name
    let name_offset = rva_to_file_offset(descriptor.name, sections)?;
    let name_data = reader.read(name_offset, 256).ok()?;
    let dll_name = read_null_terminated_string(name_data);

    // Use OriginalFirstThunk (ILT) if available, otherwise use FirstThunk (IAT)
    let thunk_rva = if descriptor.original_first_thunk != 0 {
        descriptor.original_first_thunk
    } else {
        descriptor.first_thunk
    };

    let thunk_offset = rva_to_file_offset(thunk_rva, sections)?;
    let thunk_size = if is_pe32_plus { 8 } else { 4 };

    // Read thunk table (limit to max_functions to avoid reading too much)
    let thunk_data = reader
        .read(thunk_offset, thunk_size * (max_functions + 1))
        .ok()?;

    let mut functions = Vec::new();
    let mut offset = 0;

    for _ in 0..max_functions {
        if offset + thunk_size > thunk_data.len() {
            break;
        }

        let (is_ordinal, value) = if is_pe32_plus {
            let (_remaining, result) = parse_thunk_64(&thunk_data[offset..]).ok()?;
            offset += 8;
            result
        } else {
            let (_remaining, result) = parse_thunk_32(&thunk_data[offset..]).ok()?;
            offset += 4;
            (result.0, result.1 as u64)
        };

        // Check for null terminator (end of import list)
        if value == 0 {
            break;
        }

        if is_ordinal {
            functions.push(ImportFunction::ByOrdinal {
                ordinal: value as u16,
            });
        } else {
            // Read import by name structure
            let name_rva = value as u32;
            if let Some(name_offset) = rva_to_file_offset(name_rva, sections)
                && let Ok(name_data) = reader.read(name_offset, 256)
                && let Ok((_, (hint, name))) = parse_import_by_name(name_data)
            {
                functions.push(ImportFunction::ByName { hint, name });
            }
        }
    }

    Some(ImportInfo {
        dll_name,
        functions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_import_descriptor() {
        let data = [
            0x10, 0x20, 0x00, 0x00, // OriginalFirstThunk
            0x00, 0x00, 0x00, 0x00, // TimeDateStamp
            0x00, 0x00, 0x00, 0x00, // ForwarderChain
            0x30, 0x40, 0x00, 0x00, // Name
            0x50, 0x60, 0x00, 0x00, // FirstThunk
        ];

        let (_, descriptor) = parse_import_descriptor(&data).unwrap();
        assert_eq!(descriptor.original_first_thunk, 0x2010);
        assert_eq!(descriptor.time_date_stamp, 0);
        assert_eq!(descriptor.forwarder_chain, 0);
        assert_eq!(descriptor.name, 0x4030);
        assert_eq!(descriptor.first_thunk, 0x6050);
        assert!(!descriptor.is_null());
    }

    #[test]
    fn test_null_descriptor() {
        let data = [0u8; 20];
        let (_, descriptor) = parse_import_descriptor(&data).unwrap();
        assert!(descriptor.is_null());
    }

    #[test]
    fn test_parse_thunk_32_by_name() {
        let data = [0x10, 0x20, 0x00, 0x00]; // RVA 0x2010, not ordinal
        let (_, (is_ordinal, value)) = parse_thunk_32(&data).unwrap();
        assert!(!is_ordinal);
        assert_eq!(value, 0x2010);
    }

    #[test]
    fn test_parse_thunk_32_by_ordinal() {
        let data = [0x01, 0x00, 0x00, 0x80]; // Ordinal 1 with high bit set
        let (_, (is_ordinal, value)) = parse_thunk_32(&data).unwrap();
        assert!(is_ordinal);
        assert_eq!(value, 1);
    }

    #[test]
    fn test_parse_thunk_64_by_name() {
        let data = [0x10, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]; // RVA 0x2010
        let (_, (is_ordinal, value)) = parse_thunk_64(&data).unwrap();
        assert!(!is_ordinal);
        assert_eq!(value, 0x2010);
    }

    #[test]
    fn test_parse_thunk_64_by_ordinal() {
        let data = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80]; // Ordinal 1
        let (_, (is_ordinal, value)) = parse_thunk_64(&data).unwrap();
        assert!(is_ordinal);
        assert_eq!(value, 1);
    }

    #[test]
    fn test_read_null_terminated_string() {
        let data = b"kernel32.dll\0extra data";
        let result = read_null_terminated_string(data);
        assert_eq!(result, "kernel32.dll");
    }

    #[test]
    fn test_parse_import_by_name() {
        let data = b"\x00\x01CreateFileW\0";
        let (_, (hint, name)) = parse_import_by_name(data).unwrap();
        assert_eq!(hint, 0x0100);
        assert_eq!(name, "CreateFileW");
    }

    #[test]
    fn test_rva_to_file_offset() {
        let sections = vec![
            SectionHeader {
                name: *b".text\0\0\0",
                virtual_size: 0x1000,
                virtual_address: 0x1000,
                size_of_raw_data: 0x1000,
                pointer_to_raw_data: 0x400,
                pointer_to_relocations: 0,
                pointer_to_line_numbers: 0,
                number_of_relocations: 0,
                number_of_line_numbers: 0,
                characteristics: 0,
            },
            SectionHeader {
                name: *b".rdata\0\0",
                virtual_size: 0x2000,
                virtual_address: 0x3000,
                size_of_raw_data: 0x2000,
                pointer_to_raw_data: 0x1400,
                pointer_to_relocations: 0,
                pointer_to_line_numbers: 0,
                number_of_relocations: 0,
                number_of_line_numbers: 0,
                characteristics: 0,
            },
        ];

        // RVA 0x1500 is in .text section
        let offset = rva_to_file_offset(0x1500, &sections);
        assert_eq!(offset, Some(0x900)); // 0x400 + (0x1500 - 0x1000)

        // RVA 0x3100 is in .rdata section
        let offset = rva_to_file_offset(0x3100, &sections);
        assert_eq!(offset, Some(0x1500)); // 0x1400 + (0x3100 - 0x3000)

        // RVA 0x100 is not in any section
        let offset = rva_to_file_offset(0x100, &sections);
        assert_eq!(offset, None);
    }
}
