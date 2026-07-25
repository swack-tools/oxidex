//! PE Export Directory Parser

use crate::core::FileReader;
use crate::error::Result;
use crate::io::EndianReader;
use crate::parsers::pe::structures::{ExportInfo, ImageExportDirectory, SectionHeader};
use nom::{IResult, number::complete::le_u32};

/// Parse IMAGE_EXPORT_DIRECTORY structure (40 bytes)
pub fn parse_export_directory(input: &[u8]) -> IResult<&[u8], ImageExportDirectory> {
    let (input, characteristics) = le_u32(input)?;
    let (input, time_date_stamp) = le_u32(input)?;
    let (input, major_version) = nom::number::complete::le_u16(input)?;
    let (input, minor_version) = nom::number::complete::le_u16(input)?;
    let (input, name) = le_u32(input)?;
    let (input, base) = le_u32(input)?;
    let (input, number_of_functions) = le_u32(input)?;
    let (input, number_of_names) = le_u32(input)?;
    let (input, address_of_functions) = le_u32(input)?;
    let (input, address_of_names) = le_u32(input)?;
    let (input, address_of_name_ordinals) = le_u32(input)?;

    Ok((
        input,
        ImageExportDirectory {
            characteristics,
            time_date_stamp,
            major_version,
            minor_version,
            name,
            base,
            number_of_functions,
            number_of_names,
            address_of_functions,
            address_of_names,
            address_of_name_ordinals,
        },
    ))
}

/// Convert RVA to file offset using section headers
fn rva_to_file_offset(rva: u32, sections: &[SectionHeader]) -> Option<u64> {
    for section in sections {
        if rva >= section.virtual_address && rva < section.virtual_address + section.virtual_size {
            let offset_in_section = rva - section.virtual_address;
            return Some(section.pointer_to_raw_data as u64 + offset_in_section as u64);
        }
    }
    None
}

/// Read null-terminated ASCII string from file at given offset
fn read_string_at_offset(reader: &dyn FileReader, offset: u64) -> Result<String> {
    // Read up to 256 bytes for string (typical max for export names)
    let data = reader.read(offset, 256)?;

    // Find null terminator
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());

    Ok(String::from_utf8_lossy(&data[..end]).to_string())
}

/// Parse export information from PE file
pub fn parse_exports(
    reader: &dyn FileReader,
    export_rva: u32,
    export_size: u32,
    sections: &[SectionHeader],
) -> Result<ExportInfo> {
    // Convert export directory RVA to file offset
    let export_offset = rva_to_file_offset(export_rva, sections)
        .ok_or_else(|| crate::error::ExifToolError::parse_error("Invalid export directory RVA"))?;

    // Read and parse export directory (40 bytes)
    let export_data = reader.read(export_offset, 40)?;
    let (_, directory) = parse_export_directory(export_data).map_err(|e| {
        crate::error::ExifToolError::parse_error(format!(
            "Failed to parse export directory: {:?}",
            e
        ))
    })?;

    // Read DLL name
    let name_offset = rva_to_file_offset(directory.name, sections)
        .ok_or_else(|| crate::error::ExifToolError::parse_error("Invalid export name RVA"))?;
    let dll_name = read_string_at_offset(reader, name_offset)?;

    // Calculate export section bounds for forwarding detection
    let export_section_start = export_rva;
    let export_section_end = export_rva + export_size;

    // Read Export Address Table (EAT)
    let mut forwarded_count = 0u32;
    if directory.number_of_functions > 0 && directory.address_of_functions > 0 {
        let eat_offset = rva_to_file_offset(directory.address_of_functions, sections)
            .ok_or_else(|| crate::error::ExifToolError::parse_error("Invalid EAT RVA"))?;
        let eat_size = (directory.number_of_functions * 4) as usize;
        let eat_data = reader.read(eat_offset, eat_size)?;
        let eat_reader = EndianReader::little_endian(eat_data);

        // Count forwarded exports (RVA points within export section)
        for i in 0..directory.number_of_functions {
            let offset = (i * 4) as usize;
            if let Some(function_rva) = eat_reader.u32_at(offset)
                && function_rva > 0
                && function_rva >= export_section_start
                && function_rva < export_section_end
            {
                forwarded_count += 1;
            }
        }
    }

    // Read Export Name Table (limit to first 30 for metadata)
    let mut function_names = Vec::new();
    if directory.number_of_names > 0 && directory.address_of_names > 0 {
        let name_table_offset = rva_to_file_offset(directory.address_of_names, sections)
            .ok_or_else(|| crate::error::ExifToolError::parse_error("Invalid name table RVA"))?;
        let names_to_read = std::cmp::min(directory.number_of_names, 30);
        let name_table_size = (names_to_read * 4) as usize;
        let name_table_data = reader.read(name_table_offset, name_table_size)?;
        let name_table_reader = EndianReader::little_endian(name_table_data);

        // Read name RVAs and resolve to strings
        for i in 0..names_to_read {
            let offset = (i * 4) as usize;
            if let Some(name_rva) = name_table_reader.u32_at(offset)
                && let Some(name_file_offset) = rva_to_file_offset(name_rva, sections)
                && let Ok(name) = read_string_at_offset(reader, name_file_offset)
            {
                function_names.push(name);
            }
        }
    }

    Ok(ExportInfo {
        directory,
        dll_name,
        function_names,
        forwarded_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_export_directory() {
        // Create test export directory structure
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes()); // characteristics
        data.extend_from_slice(&1609459200u32.to_le_bytes()); // timestamp
        data.extend_from_slice(&0u16.to_le_bytes()); // major_version
        data.extend_from_slice(&0u16.to_le_bytes()); // minor_version
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // name RVA
        data.extend_from_slice(&1u32.to_le_bytes()); // base
        data.extend_from_slice(&5u32.to_le_bytes()); // number_of_functions
        data.extend_from_slice(&3u32.to_le_bytes()); // number_of_names
        data.extend_from_slice(&0x2000u32.to_le_bytes()); // address_of_functions
        data.extend_from_slice(&0x3000u32.to_le_bytes()); // address_of_names
        data.extend_from_slice(&0x4000u32.to_le_bytes()); // address_of_name_ordinals

        let (_, directory) = parse_export_directory(&data).unwrap();

        assert_eq!(directory.characteristics, 0);
        assert_eq!(directory.time_date_stamp, 1609459200);
        assert_eq!(directory.name, 0x1000);
        assert_eq!(directory.base, 1);
        assert_eq!(directory.number_of_functions, 5);
        assert_eq!(directory.number_of_names, 3);
        assert_eq!(directory.address_of_functions, 0x2000);
        assert_eq!(directory.address_of_names, 0x3000);
        assert_eq!(directory.address_of_name_ordinals, 0x4000);
    }

    #[test]
    fn test_rva_to_file_offset() {
        // Create test section
        let section = SectionHeader {
            name: *b".edata\0\0",
            virtual_size: 0x1000,
            virtual_address: 0x2000,
            size_of_raw_data: 0x1000,
            pointer_to_raw_data: 0x400,
            pointer_to_relocations: 0,
            pointer_to_line_numbers: 0,
            number_of_relocations: 0,
            number_of_line_numbers: 0,
            characteristics: 0,
        };

        let sections = vec![section];

        // RVA within section
        assert_eq!(
            rva_to_file_offset(0x2000, &sections),
            Some(0x400) // section start
        );
        assert_eq!(
            rva_to_file_offset(0x2100, &sections),
            Some(0x500) // offset 0x100 into section
        );

        // RVA outside section
        assert_eq!(rva_to_file_offset(0x1000, &sections), None);
        assert_eq!(rva_to_file_offset(0x4000, &sections), None);
    }
}
