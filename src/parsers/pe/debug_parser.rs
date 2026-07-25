//! PE Debug Directory Parser

use crate::parsers::pe::structures::{CodeViewNB10, CodeViewRSDS, DebugDirectoryEntry};
use nom::{
    IResult,
    bytes::complete::take,
    number::complete::{le_u16, le_u32},
};

/// Parse Debug Directory Entry (28 bytes)
pub fn parse_debug_directory_entry(input: &[u8]) -> IResult<&[u8], DebugDirectoryEntry> {
    let (input, characteristics) = le_u32(input)?;
    let (input, time_date_stamp) = le_u32(input)?;
    let (input, major_version) = le_u16(input)?;
    let (input, minor_version) = le_u16(input)?;
    let (input, debug_type) = le_u32(input)?;
    let (input, size_of_data) = le_u32(input)?;
    let (input, address_of_raw_data) = le_u32(input)?;
    let (input, pointer_to_raw_data) = le_u32(input)?;

    Ok((
        input,
        DebugDirectoryEntry {
            characteristics,
            time_date_stamp,
            major_version,
            minor_version,
            debug_type,
            size_of_data,
            address_of_raw_data,
            pointer_to_raw_data,
        },
    ))
}

/// Parse CodeView RSDS debug info
pub fn parse_codeview_rsds(data: &[u8]) -> Option<CodeViewRSDS> {
    if data.len() < 24 {
        return None;
    }

    let (input, signature) = take::<usize, &[u8], nom::error::Error<&[u8]>>(4)(data).ok()?;
    if signature != b"RSDS" {
        return None;
    }

    let (input, guid_bytes) = take::<usize, &[u8], nom::error::Error<&[u8]>>(16)(input).ok()?;
    let (input, age) = le_u32::<_, nom::error::Error<_>>(input).ok()?;

    let mut guid = [0u8; 16];
    guid.copy_from_slice(guid_bytes);

    // Read PDB file name (null-terminated ASCII string)
    let pdb_file_name = String::from_utf8_lossy(input)
        .split('\0')
        .next()
        .unwrap_or("")
        .to_string();

    Some(CodeViewRSDS {
        signature: *b"RSDS",
        guid,
        age,
        pdb_file_name,
    })
}

/// Parse CodeView NB10 debug info
pub fn parse_codeview_nb10(data: &[u8]) -> Option<CodeViewNB10> {
    if data.len() < 16 {
        return None;
    }

    let (input, signature) = take::<usize, &[u8], nom::error::Error<&[u8]>>(4)(data).ok()?;
    if signature != b"NB10" {
        return None;
    }

    let (input, offset) = le_u32::<_, nom::error::Error<_>>(input).ok()?;
    let (input, timestamp) = le_u32::<_, nom::error::Error<_>>(input).ok()?;
    let (input, age) = le_u32::<_, nom::error::Error<_>>(input).ok()?;

    // Read PDB file name (null-terminated ASCII string)
    let pdb_file_name = String::from_utf8_lossy(input)
        .split('\0')
        .next()
        .unwrap_or("")
        .to_string();

    Some(CodeViewNB10 {
        signature: *b"NB10",
        offset,
        timestamp,
        age,
        pdb_file_name,
    })
}
