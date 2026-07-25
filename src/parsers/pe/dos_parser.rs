//! DOS header parser for PE files

use crate::parsers::pe::structures::DosHeader;
use nom::{
    IResult,
    number::complete::{le_u16, le_u32},
};

/// Parse DOS header from PE file
pub fn parse_dos_header(input: &[u8]) -> IResult<&[u8], DosHeader> {
    let (input, e_magic) = le_u16(input)?;
    let (input, e_cblp) = le_u16(input)?;
    let (input, e_cp) = le_u16(input)?;
    let (input, e_crlc) = le_u16(input)?;
    let (input, e_cparhdr) = le_u16(input)?;
    let (input, e_minalloc) = le_u16(input)?;
    let (input, e_maxalloc) = le_u16(input)?;
    let (input, e_ss) = le_u16(input)?;
    let (input, e_sp) = le_u16(input)?;
    let (input, e_csum) = le_u16(input)?;
    let (input, e_ip) = le_u16(input)?;
    let (input, e_cs) = le_u16(input)?;
    let (input, e_lfarlc) = le_u16(input)?;
    let (input, e_ovno) = le_u16(input)?;

    // Parse e_res[4]
    let (input, res0) = le_u16(input)?;
    let (input, res1) = le_u16(input)?;
    let (input, res2) = le_u16(input)?;
    let (input, res3) = le_u16(input)?;
    let e_res = [res0, res1, res2, res3];

    let (input, e_oemid) = le_u16(input)?;
    let (input, e_oeminfo) = le_u16(input)?;

    // Parse e_res2[10]
    let (input, res2_0) = le_u16(input)?;
    let (input, res2_1) = le_u16(input)?;
    let (input, res2_2) = le_u16(input)?;
    let (input, res2_3) = le_u16(input)?;
    let (input, res2_4) = le_u16(input)?;
    let (input, res2_5) = le_u16(input)?;
    let (input, res2_6) = le_u16(input)?;
    let (input, res2_7) = le_u16(input)?;
    let (input, res2_8) = le_u16(input)?;
    let (input, res2_9) = le_u16(input)?;
    let e_res2 = [
        res2_0, res2_1, res2_2, res2_3, res2_4, res2_5, res2_6, res2_7, res2_8, res2_9,
    ];

    let (input, e_lfanew) = le_u32(input)?;

    Ok((
        input,
        DosHeader {
            e_magic,
            e_cblp,
            e_cp,
            e_crlc,
            e_cparhdr,
            e_minalloc,
            e_maxalloc,
            e_ss,
            e_sp,
            e_csum,
            e_ip,
            e_cs,
            e_lfarlc,
            e_ovno,
            e_res,
            e_oemid,
            e_oeminfo,
            e_res2,
            e_lfanew,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dos_header_valid() {
        // Create minimal valid DOS header (64 bytes)
        let mut data = Vec::new();
        data.extend_from_slice(&0x5A4Du16.to_le_bytes()); // e_magic "MZ"
        data.extend_from_slice(&0x0090u16.to_le_bytes()); // e_cblp
        data.extend_from_slice(&0x0003u16.to_le_bytes()); // e_cp
        data.extend_from_slice(&[0; 2]); // e_crlc
        data.extend_from_slice(&[0; 2]); // e_cparhdr
        data.extend_from_slice(&[0; 2]); // e_minalloc
        data.extend_from_slice(&[0; 2]); // e_maxalloc
        data.extend_from_slice(&[0; 2]); // e_ss
        data.extend_from_slice(&[0; 2]); // e_sp
        data.extend_from_slice(&[0; 2]); // e_csum
        data.extend_from_slice(&[0; 2]); // e_ip
        data.extend_from_slice(&[0; 2]); // e_cs
        data.extend_from_slice(&[0; 2]); // e_lfarlc
        data.extend_from_slice(&[0; 2]); // e_ovno
        data.extend_from_slice(&[0; 8]); // e_res[4]
        data.extend_from_slice(&[0; 2]); // e_oemid
        data.extend_from_slice(&[0; 2]); // e_oeminfo
        data.extend_from_slice(&[0; 20]); // e_res2[10]
        data.extend_from_slice(&0x000000F0u32.to_le_bytes()); // e_lfanew

        let result = parse_dos_header(&data);
        assert!(result.is_ok());

        let (remaining, header) = result.unwrap();
        assert_eq!(remaining.len(), 0);
        assert_eq!(header.e_magic, 0x5A4D);
        assert_eq!(header.e_cblp, 0x0090);
        assert_eq!(header.e_lfanew, 0xF0);
    }

    #[test]
    fn test_parse_dos_header_invalid_magic() {
        let mut data = vec![0; 64];
        data[0..2].copy_from_slice(&0x4142u16.to_le_bytes()); // Wrong magic "AB"

        let result = parse_dos_header(&data);
        assert!(result.is_ok());
        let (_, header) = result.unwrap();
        // Parser doesn't validate magic, just parses bytes
        assert_eq!(header.e_magic, 0x4142);
    }

    #[test]
    fn test_parse_dos_header_truncated() {
        let data = vec![0; 32]; // Only 32 bytes, need 64

        let result = parse_dos_header(&data);
        assert!(result.is_err()); // Should fail due to insufficient data
    }
}
