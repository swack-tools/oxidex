//! Optional header parser for PE files

use crate::parsers::pe::structures::{OptionalHeaderNT, OptionalHeaderStandard};
use nom::{
    IResult,
    number::complete::{le_u8, le_u16, le_u32, le_u64},
};

/// Parse Optional Header Standard Fields
pub fn parse_optional_header_standard(input: &[u8]) -> IResult<&[u8], OptionalHeaderStandard> {
    let (input, magic) = le_u16(input)?;
    let (input, major_linker_version) = le_u8(input)?;
    let (input, minor_linker_version) = le_u8(input)?;
    let (input, size_of_code) = le_u32(input)?;
    let (input, size_of_initialized_data) = le_u32(input)?;
    let (input, size_of_uninitialized_data) = le_u32(input)?;
    let (input, address_of_entry_point) = le_u32(input)?;
    let (input, base_of_code) = le_u32(input)?;

    Ok((
        input,
        OptionalHeaderStandard {
            magic,
            major_linker_version,
            minor_linker_version,
            size_of_code,
            size_of_initialized_data,
            size_of_uninitialized_data,
            address_of_entry_point,
            base_of_code,
        },
    ))
}

/// Parse Optional Header NT-Specific Fields (PE32+/PE32)
pub fn parse_optional_header_nt(
    input: &[u8],
    is_pe32_plus: bool,
) -> IResult<&[u8], OptionalHeaderNT> {
    // For PE32+, skip BaseOfData field (doesn't exist)
    // For PE32, read BaseOfData then ImageBase is 32-bit
    let (input, image_base) = if is_pe32_plus {
        le_u64(input)?
    } else {
        // PE32: skip BaseOfData (u32), then read ImageBase (u32) and extend to u64
        let (input, _base_of_data) = le_u32(input)?;
        let (input, image_base_32) = le_u32(input)?;
        (input, image_base_32 as u64)
    };

    let (input, section_alignment) = le_u32(input)?;
    let (input, file_alignment) = le_u32(input)?;
    let (input, major_operating_system_version) = le_u16(input)?;
    let (input, minor_operating_system_version) = le_u16(input)?;
    let (input, major_image_version) = le_u16(input)?;
    let (input, minor_image_version) = le_u16(input)?;
    let (input, major_subsystem_version) = le_u16(input)?;
    let (input, minor_subsystem_version) = le_u16(input)?;
    let (input, win32_version_value) = le_u32(input)?;
    let (input, size_of_image) = le_u32(input)?;
    let (input, size_of_headers) = le_u32(input)?;
    let (input, checksum) = le_u32(input)?;
    let (input, subsystem) = le_u16(input)?;
    let (input, dll_characteristics) = le_u16(input)?;

    let (
        input,
        size_of_stack_reserve,
        size_of_stack_commit,
        size_of_heap_reserve,
        size_of_heap_commit,
    ) = if is_pe32_plus {
        let (input, ssr) = le_u64(input)?;
        let (input, ssc) = le_u64(input)?;
        let (input, shr) = le_u64(input)?;
        let (input, shc) = le_u64(input)?;
        (input, ssr, ssc, shr, shc)
    } else {
        let (input, ssr) = le_u32(input)?;
        let (input, ssc) = le_u32(input)?;
        let (input, shr) = le_u32(input)?;
        let (input, shc) = le_u32(input)?;
        (input, ssr as u64, ssc as u64, shr as u64, shc as u64)
    };

    let (input, loader_flags) = le_u32(input)?;
    let (input, number_of_rva_and_sizes) = le_u32(input)?;

    // Parse data directories
    let mut data_directories = Vec::new();
    let mut remaining = input;
    for _ in 0..number_of_rva_and_sizes {
        if remaining.len() < 8 {
            break;
        }
        let (rest, rva) = le_u32(remaining)?;
        let (rest, size) = le_u32(rest)?;
        data_directories.push((rva, size));
        remaining = rest;
    }

    Ok((
        remaining,
        OptionalHeaderNT {
            image_base,
            section_alignment,
            file_alignment,
            major_operating_system_version,
            minor_operating_system_version,
            major_image_version,
            minor_image_version,
            major_subsystem_version,
            minor_subsystem_version,
            win32_version_value,
            size_of_image,
            size_of_headers,
            checksum,
            subsystem,
            dll_characteristics,
            size_of_stack_reserve,
            size_of_stack_commit,
            size_of_heap_reserve,
            size_of_heap_commit,
            loader_flags,
            number_of_rva_and_sizes,
            data_directories,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_optional_header_standard() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x010Bu16.to_le_bytes()); // PE32 magic
        data.push(14); // major linker version
        data.push(0); // minor linker version
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // size of code
        data.extend_from_slice(&0x2000u32.to_le_bytes()); // initialized data
        data.extend_from_slice(&0x0000u32.to_le_bytes()); // uninitialized data
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // entry point
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // base of code

        let result = parse_optional_header_standard(&data);
        assert!(result.is_ok());

        let (_, header) = result.unwrap();
        assert_eq!(header.magic, 0x010B);
        assert_eq!(header.major_linker_version, 14);
        assert_eq!(header.size_of_code, 0x1000);
    }

    #[test]
    fn test_parse_optional_header_nt_pe32() {
        let mut data = Vec::new();
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // base of data
        data.extend_from_slice(&0x00400000u32.to_le_bytes()); // image base
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // section alignment
        data.extend_from_slice(&0x0200u32.to_le_bytes()); // file alignment
        data.extend_from_slice(&[6, 0]); // major OS version
        data.extend_from_slice(&[0, 0]); // minor OS version
        data.extend_from_slice(&[0, 0]); // major image version
        data.extend_from_slice(&[0, 0]); // minor image version
        data.extend_from_slice(&[6, 0]); // major subsystem version
        data.extend_from_slice(&[0, 0]); // minor subsystem version
        data.extend_from_slice(&[0; 4]); // win32 version
        data.extend_from_slice(&0x10000u32.to_le_bytes()); // size of image
        data.extend_from_slice(&0x0400u32.to_le_bytes()); // size of headers
        data.extend_from_slice(&[0; 4]); // checksum
        data.extend_from_slice(&3u16.to_le_bytes()); // subsystem (CUI)
        data.extend_from_slice(&[0; 2]); // dll characteristics
        data.extend_from_slice(&0x100000u32.to_le_bytes()); // stack reserve
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // stack commit
        data.extend_from_slice(&0x100000u32.to_le_bytes()); // heap reserve
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // heap commit
        data.extend_from_slice(&[0; 4]); // loader flags
        data.extend_from_slice(&16u32.to_le_bytes()); // number of rva and sizes

        let result = parse_optional_header_nt(&data, false);
        assert!(result.is_ok());

        let (_, header) = result.unwrap();
        assert_eq!(header.image_base, 0x00400000);
        assert_eq!(header.subsystem, 3);
    }
}
