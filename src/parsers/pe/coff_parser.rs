//! COFF header parser for PE files

use crate::parsers::pe::structures::CoffHeader;
use nom::{
    IResult,
    bytes::complete::tag,
    number::complete::{le_u16, le_u32},
};

/// Parse COFF header from PE file (after PE signature)
pub fn parse_coff_header(input: &[u8]) -> IResult<&[u8], CoffHeader> {
    // First verify PE signature "PE\0\0"
    let (input, _) = tag(&b"PE\0\0"[..])(input)?;

    // Parse COFF header fields
    let (input, machine) = le_u16(input)?;
    let (input, number_of_sections) = le_u16(input)?;
    let (input, time_date_stamp) = le_u32(input)?;
    let (input, pointer_to_symbol_table) = le_u32(input)?;
    let (input, number_of_symbols) = le_u32(input)?;
    let (input, size_of_optional_header) = le_u16(input)?;
    let (input, characteristics) = le_u16(input)?;

    Ok((
        input,
        CoffHeader {
            machine,
            number_of_sections,
            time_date_stamp,
            pointer_to_symbol_table,
            number_of_symbols,
            size_of_optional_header,
            characteristics,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_coff_header_valid() {
        let mut data = Vec::new();
        data.extend_from_slice(b"PE\0\0"); // PE signature
        data.extend_from_slice(&0x014Cu16.to_le_bytes()); // machine (i386)
        data.extend_from_slice(&0x0003u16.to_le_bytes()); // 3 sections
        data.extend_from_slice(&0x12345678u32.to_le_bytes()); // timestamp
        data.extend_from_slice(&0x00000000u32.to_le_bytes()); // symbol table
        data.extend_from_slice(&0x00000000u32.to_le_bytes()); // num symbols
        data.extend_from_slice(&0x00E0u16.to_le_bytes()); // optional header size
        data.extend_from_slice(&0x0102u16.to_le_bytes()); // characteristics

        let result = parse_coff_header(&data);
        assert!(result.is_ok());

        let (_remaining, header) = result.unwrap();
        assert_eq!(header.machine, 0x014C);
        assert_eq!(header.number_of_sections, 3);
        assert_eq!(header.time_date_stamp, 0x12345678);
        assert_eq!(header.size_of_optional_header, 0xE0);
    }

    #[test]
    fn test_parse_coff_header_invalid_signature() {
        let mut data = Vec::new();
        data.extend_from_slice(b"XX\0\0"); // Wrong signature
        data.extend_from_slice(&[0; 20]);

        let result = parse_coff_header(&data);
        assert!(result.is_err()); // Should fail on signature mismatch
    }
}
