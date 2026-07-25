//! PE Section Table Parser

use crate::parsers::pe::structures::SectionHeader;
use nom::{
    IResult,
    bytes::complete::take,
    number::complete::{le_u16, le_u32},
};

/// Parse a single PE section header (40 bytes)
pub fn parse_section_header(input: &[u8]) -> IResult<&[u8], SectionHeader> {
    let (input, name) = take(8usize)(input)?;
    let (input, virtual_size) = le_u32(input)?;
    let (input, virtual_address) = le_u32(input)?;
    let (input, size_of_raw_data) = le_u32(input)?;
    let (input, pointer_to_raw_data) = le_u32(input)?;
    let (input, pointer_to_relocations) = le_u32(input)?;
    let (input, pointer_to_line_numbers) = le_u32(input)?;
    let (input, number_of_relocations) = le_u16(input)?;
    let (input, number_of_line_numbers) = le_u16(input)?;
    let (input, characteristics) = le_u32(input)?;

    let mut name_array = [0u8; 8];
    name_array.copy_from_slice(name);

    Ok((
        input,
        SectionHeader {
            name: name_array,
            virtual_size,
            virtual_address,
            size_of_raw_data,
            pointer_to_raw_data,
            pointer_to_relocations,
            pointer_to_line_numbers,
            number_of_relocations,
            number_of_line_numbers,
            characteristics,
        },
    ))
}

/// Parse PE section table
pub fn parse_section_table(
    input: &[u8],
    number_of_sections: u16,
) -> IResult<&[u8], Vec<SectionHeader>> {
    let mut sections = Vec::new();
    let mut remaining = input;

    for _ in 0..number_of_sections {
        let (rest, section) = parse_section_header(remaining)?;
        sections.push(section);
        remaining = rest;
    }

    Ok((remaining, sections))
}
