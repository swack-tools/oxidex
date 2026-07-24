//! PE VERSION_INFO Resource Parser

use crate::io::EndianReader;
use crate::parsers::pe::structures::VsFixedFileInfo;
use nom::{
    IResult,
    number::complete::{le_u16, le_u32},
};
use std::collections::HashMap;

/// Parse VS_FIXEDFILEINFO structure (52 bytes)
pub fn parse_vs_fixed_file_info(input: &[u8]) -> IResult<&[u8], VsFixedFileInfo> {
    let (input, signature) = le_u32(input)?;
    let (input, struct_version) = le_u32(input)?;
    let (input, file_version_ms) = le_u32(input)?;
    let (input, file_version_ls) = le_u32(input)?;
    let (input, product_version_ms) = le_u32(input)?;
    let (input, product_version_ls) = le_u32(input)?;
    let (input, file_flags_mask) = le_u32(input)?;
    let (input, file_flags) = le_u32(input)?;
    let (input, file_os) = le_u32(input)?;
    let (input, file_type) = le_u32(input)?;
    let (input, file_subtype) = le_u32(input)?;
    let (input, file_date_ms) = le_u32(input)?;
    let (input, file_date_ls) = le_u32(input)?;

    Ok((
        input,
        VsFixedFileInfo {
            signature,
            struct_version,
            file_version_ms,
            file_version_ls,
            product_version_ms,
            product_version_ls,
            file_flags_mask,
            file_flags,
            file_os,
            file_type,
            file_subtype,
            file_date_ms,
            file_date_ls,
        },
    ))
}

/// Parse VERSION_INFO structure and extract string table
pub fn parse_version_info(data: &[u8]) -> Option<(VsFixedFileInfo, HashMap<String, String>)> {
    // VERSION_INFO structure starts with:
    // WORD  wLength
    // WORD  wValueLength
    // WORD  wType
    // WCHAR szKey[] = "VS_VERSION_INFO"
    // WORD  Padding
    // VS_FIXEDFILEINFO Value
    // WORD  Padding2
    // WORD  Children (StringFileInfo and/or VarFileInfo)

    if data.len() < 6 {
        return None;
    }

    let (_input, w_length) = le_u16::<_, nom::error::Error<_>>(data).ok()?;
    let (_input, w_value_length) = le_u16::<_, nom::error::Error<_>>(&data[2..]).ok()?;
    let (_input, _w_type) = le_u16::<_, nom::error::Error<_>>(&data[4..]).ok()?;

    // Skip to after "VS_VERSION_INFO" null-terminated wide string
    // VS_VERSION_INFO = 15 chars + null = 16 * 2 = 32 bytes
    if data.len() < 6 + 32 {
        return None;
    }

    // Find VS_FIXEDFILEINFO (aligned to 4-byte boundary after header)
    let mut offset = 6 + 32;
    // Align to 4 bytes
    offset = (offset + 3) & !3;

    if offset + 52 > data.len() || w_value_length != 52 {
        return None;
    }

    let fixed_info = parse_vs_fixed_file_info(&data[offset..]).ok()?.1;

    // Find StringFileInfo child
    offset += 52;
    // Align to 4 bytes
    offset = (offset + 3) & !3;

    // VERSION_INFO can have multiple children (StringFileInfo, VarFileInfo)
    // We need to search for StringFileInfo specifically
    let children_length = (w_length as usize).checked_sub(offset)?;
    let strings = find_string_file_info(&data[offset..], children_length).unwrap_or_default();

    Some((fixed_info, strings))
}

/// Find and parse StringFileInfo among VERSION_INFO children
fn find_string_file_info(data: &[u8], max_length: usize) -> Option<HashMap<String, String>> {
    let mut offset = 0;
    while offset + 6 < max_length && offset + 6 < data.len() {
        // Read child structure header
        let (_, child_length) = le_u16::<_, nom::error::Error<_>>(&data[offset..]).ok()?;

        if child_length < 6 || child_length as usize > max_length - offset {
            break;
        }

        // Read the key name
        let key = read_wide_string(&data[offset + 6..])?;

        if key == "StringFileInfo" {
            return parse_string_file_info(&data[offset..], child_length as usize);
        }

        // Move to next child (align to 4 bytes)
        offset += child_length as usize;
        offset = (offset + 3) & !3;
    }

    None
}

/// Parse StringFileInfo structure
fn parse_string_file_info(data: &[u8], _max_length: usize) -> Option<HashMap<String, String>> {
    if data.len() < 6 {
        return None;
    }

    // StringFileInfo structure:
    // WORD  wLength
    // WORD  wValueLength (always 0)
    // WORD  wType (1 = text)
    // WCHAR szKey[] = "StringFileInfo"

    let (_input, _length) = le_u16::<_, nom::error::Error<_>>(data).ok()?;
    let (_input, value_len) = le_u16::<_, nom::error::Error<_>>(&data[2..]).ok()?;

    if value_len != 0 {
        return None; // StringFileInfo should have wValueLength = 0
    }

    // Read the key name to find actual offset
    let key = read_wide_string(&data[6..])?;

    // Skip header (6 bytes) + key string (including null terminator)
    let mut offset = 6 + (key.len() + 1) * 2; // +1 for null terminator
    offset = (offset + 3) & !3;

    // Now we should have StringTable structure
    parse_string_table(&data[offset..])
}

/// Parse StringTable structure (contains the actual key-value pairs)
fn parse_string_table(data: &[u8]) -> Option<HashMap<String, String>> {
    if data.len() < 6 {
        return None;
    }

    let (_, length) = le_u16::<_, nom::error::Error<_>>(data).ok()?;

    // Read the language ID string to find actual offset
    let lang_id = read_wide_string(&data[6..])?;

    let language_code = language_code_from_string_table_key(&lang_id);
    // Skip header (6 bytes) + language ID string (including null terminator)
    let mut offset = 6 + (lang_id.len() + 1) * 2; // +1 for null terminator
    offset = (offset + 3) & !3;

    let end_offset = length as usize;
    let mut strings = HashMap::new();

    // Parse all String structures
    while offset + 6 < end_offset && offset < data.len() {
        if let Some((key, value, next_offset)) = parse_string_entry(&data[offset..]) {
            strings.insert(key, value);
            offset += next_offset;
            offset = (offset + 3) & !3; // Align
        } else {
            break;
        }
    }

    // A StringTable key consists of a four-digit hexadecimal Windows
    // language ID followed by a four-digit code-page ID. Version strings are
    // already emitted through the EXE metadata path, so expose the decoded
    // language alongside them without hard-coding a group prefix here.
    if let Some(language_code) = language_code {
        strings.insert("LanguageCode".to_string(), language_code.to_string());
    }

    Some(strings)
}

fn language_code_from_string_table_key(key: &str) -> Option<&'static str> {
    let language_id = u16::from_str_radix(key.get(..4)?, 16).ok()?;
    match language_id {
        0x0409 => Some("English (U.S.)"),
        _ => None,
    }
}

/// Parse a single String entry (key-value pair)
fn parse_string_entry(data: &[u8]) -> Option<(String, String, usize)> {
    if data.len() < 6 {
        return None;
    }

    let (_, length) = le_u16::<_, nom::error::Error<_>>(data).ok()?;
    let (_, value_length) = le_u16::<_, nom::error::Error<_>>(&data[2..]).ok()?;

    if length < 6 {
        return None;
    }

    // Skip header
    let mut offset = 6;

    // Read key (null-terminated wide string)
    let key = read_wide_string(&data[offset..])?;
    offset += (key.len() + 1) * 2; // +1 for null terminator

    // Align to 4 bytes
    offset = (offset + 3) & !3;

    // Read value if present
    // value_length is in WORDs (16-bit values), so convert to bytes
    let value = if value_length > 0 && offset < data.len() {
        read_wide_string_length(&data[offset..], value_length as usize * 2)?
    } else {
        String::new()
    };

    Some((key, value, length as usize))
}

/// Read null-terminated wide (UTF-16LE) string
fn read_wide_string(data: &[u8]) -> Option<String> {
    let reader = EndianReader::little_endian(data);
    let mut chars = Vec::new();
    let mut i = 0;

    while i + 1 < data.len() {
        let ch = reader.u16_at(i)?;
        if ch == 0 {
            break;
        }
        chars.push(ch);
        i += 2;
    }

    String::from_utf16(&chars).ok()
}

/// Read wide string with specific length (in bytes)
fn read_wide_string_length(data: &[u8], byte_length: usize) -> Option<String> {
    if byte_length == 0 || data.len() < byte_length {
        return Some(String::new());
    }

    let reader = EndianReader::little_endian(data);
    let mut chars = Vec::new();
    let mut i = 0;
    let max = byte_length.min(data.len());

    while i + 1 < max {
        let ch = reader.u16_at(i)?;
        if ch == 0 {
            break;
        }
        chars.push(ch);
        i += 2;
    }

    String::from_utf16(&chars).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_us_english_string_table_language() {
        assert_eq!(
            language_code_from_string_table_key("040904B0"),
            Some("English (U.S.)")
        );
        assert_eq!(language_code_from_string_table_key("invalid"), None);
        assert_eq!(language_code_from_string_table_key("040"), None);
    }
}
