//! PDF Info dictionary parser
//!
//! This module handles parsing of PDF Info dictionaries, which contain
//! document metadata such as Title, Author, Subject, Keywords, etc.
//!
//! # PDF Structure Overview
//!
//! PDF files have a trailer dictionary at the end that references an Info object:
//! ```text
//! trailer
//! << /Size 6 /Root 1 0 R /Info 4 0 R >>
//! startxref
//! 1234
//! %%EOF
//! ```
//!
//! The Info dictionary contains standard metadata fields:
//! ```text
//! 4 0 obj
//! <<
//!   /Title (My Document)
//!   /Author (John Doe)
//!   /Subject (Example)
//!   /Keywords (test, pdf)
//!   /Creator (Application Name)
//!   /Producer (PDF Library)
//!   /CreationDate (D:20240115143000+00'00')
//!   /ModDate (D:20240115150000+00'00')
//! >>
//! endobj
//! ```

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use nom::{
    bytes::complete::{tag, take_until, take_while, take_while1},
    character::complete::{digit1, multispace0, space0},
    combinator::map_res,
    multi::many0,
    sequence::{delimited, preceded, tuple},
    IResult,
};
use std::collections::HashMap;
use std::str;

/// Extracts PDF Info dictionary metadata from a PDF file.
///
/// This function:
/// 1. Locates the trailer dictionary at the end of the file
/// 2. Extracts the /Info reference (e.g., "4 0 R")
/// 3. Finds the cross-reference table to locate the Info object
/// 4. Parses the Info dictionary and extracts metadata fields
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PDF file
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted metadata with "PDF:" prefix
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Supported Info Dictionary Fields
///
/// - Title
/// - Author
/// - Subject
/// - Keywords
/// - Creator
/// - Producer
/// - CreationDate
/// - ModDate
pub fn parse_info_dict(reader: &dyn FileReader) -> Result<MetadataMap> {
    let file_size = reader.size();

    // Read the last 1024 bytes to find trailer
    let tail_size = std::cmp::min(1024, file_size as usize);
    let tail_offset = file_size.saturating_sub(tail_size as u64);
    let tail_data = reader.read(tail_offset, tail_size)?;

    // Find startxref and get xref offset
    let xref_offset = find_xref_offset(tail_data)?;

    // Read xref table and trailer region (up to 8KB should be enough)
    let xref_size = std::cmp::min(8192, file_size.saturating_sub(xref_offset) as usize);
    let xref_data = reader.read(xref_offset, xref_size)?;

    // Parse trailer to find Info reference
    let info_ref = find_info_reference(xref_data)?;

    // Parse xref table to build object offset map
    let xref_map = parse_xref_table(xref_data)?;

    // Get Info object offset from xref table
    let info_offset = xref_map.get(&info_ref.object_num).ok_or_else(|| {
        ExifToolError::parse_error(format!(
            "Info object {} not found in xref table",
            info_ref.object_num
        ))
    })?;

    // Read Info object (up to 4KB should be enough for metadata)
    let info_size = std::cmp::min(4096, file_size.saturating_sub(*info_offset) as usize);
    let info_data = reader.read(*info_offset, info_size)?;

    // Parse Info dictionary
    let info_dict = parse_info_object(info_data)?;

    // Convert to MetadataMap with PDF: prefix
    let mut metadata = MetadataMap::with_capacity(info_dict.len());
    for (key, value) in info_dict {
        let tag_name = format!("PDF:{}", key);
        metadata.insert(tag_name, TagValue::new_string(value));
    }

    Ok(metadata)
}

/// Object reference structure (e.g., "4 0 R" means object 4, generation 0)
#[derive(Debug, Clone, Copy)]
struct ObjectRef {
    object_num: u32,
    #[allow(dead_code)]
    generation: u16,
}

/// Finds the startxref offset from the PDF tail
fn find_xref_offset(tail_data: &[u8]) -> Result<u64> {
    // Convert to string for easier searching
    let tail_str = str::from_utf8(tail_data)
        .map_err(|_| ExifToolError::parse_error("PDF tail contains invalid UTF-8"))?;

    // Find "startxref" keyword
    let startxref_pos = tail_str
        .rfind("startxref")
        .ok_or_else(|| ExifToolError::parse_error("startxref not found in PDF"))?;

    // Extract the number after startxref
    let after_start = startxref_pos
        .checked_add(9)
        .ok_or_else(|| ExifToolError::parse_error("Offset overflow after startxref"))?;
    if after_start > tail_str.len() {
        return Err(ExifToolError::parse_error("Invalid startxref position"));
    }
    let after_keyword = &tail_str[after_start..];

    // Parse the offset number
    let (_, offset) = parse_number(after_keyword.as_bytes())
        .map_err(|_| ExifToolError::parse_error("Invalid xref offset after startxref"))?;

    Ok(offset)
}

/// Finds the /Info reference from the trailer dictionary
fn find_info_reference(xref_data: &[u8]) -> Result<ObjectRef> {
    // Convert to string
    let xref_str = str::from_utf8(xref_data)
        .map_err(|_| ExifToolError::parse_error("xref data contains invalid UTF-8"))?;

    // Find "trailer" keyword
    let trailer_pos = xref_str
        .find("trailer")
        .ok_or_else(|| ExifToolError::parse_error("trailer not found in PDF"))?;

    let after_trailer = &xref_str[trailer_pos..];

    // Find /Info reference in trailer dictionary
    let (_, obj_ref) = parse_trailer_info_ref(after_trailer.as_bytes())
        .map_err(|_| ExifToolError::parse_error("Could not parse /Info reference from trailer"))?;

    Ok(obj_ref)
}

/// Parses the /Info reference from trailer dictionary
fn parse_trailer_info_ref(input: &[u8]) -> IResult<&[u8], ObjectRef> {
    // Find /Info key followed by object reference
    let (input, _) = take_until("/Info")(input)?;
    let (input, _) = tag(b"/Info")(input)?;
    let (input, _) = multispace0(input)?;

    // Parse object reference (e.g., "4 0 R")
    parse_object_reference(input)
}

/// Parses an object reference like "4 0 R"
fn parse_object_reference(input: &[u8]) -> IResult<&[u8], ObjectRef> {
    let (input, object_num) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, generation) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(b"R")(input)?;

    Ok((
        input,
        ObjectRef {
            object_num: object_num as u32,
            generation: generation as u16,
        },
    ))
}

/// Parses a decimal number from bytes
fn parse_number(input: &[u8]) -> IResult<&[u8], u64> {
    preceded(
        multispace0,
        map_res(map_res(digit1, str::from_utf8), |s: &str| s.parse::<u64>()),
    )(input)
}

/// Parses the xref table and builds a map of object numbers to file offsets
fn parse_xref_table(xref_data: &[u8]) -> Result<HashMap<u32, u64>> {
    let xref_str = str::from_utf8(xref_data)
        .map_err(|_| ExifToolError::parse_error("xref table contains invalid UTF-8"))?;

    // Find "xref" keyword
    let xref_pos = xref_str
        .find("xref")
        .ok_or_else(|| ExifToolError::parse_error("xref table not found"))?;

    let after_xref = &xref_str[xref_pos + 4..]; // "xref".len() = 4

    // Parse xref entries
    let (_, xref_map) = parse_xref_entries(after_xref.as_bytes())
        .map_err(|_| ExifToolError::parse_error("Failed to parse xref entries"))?;

    Ok(xref_map)
}

/// Parses xref entries and returns a map of object numbers to offsets
fn parse_xref_entries(input: &[u8]) -> IResult<&[u8], HashMap<u32, u64>> {
    let mut xref_map = HashMap::new();
    let mut input = input;

    // Parse subsections until we hit "trailer"
    loop {
        // Skip whitespace
        let (inp, _) = multispace0(input)?;

        // Check if we've reached trailer
        if inp.starts_with(b"trailer") {
            break;
        }

        // Parse subsection header: "start_obj_num count"
        let parse_result = tuple((parse_number, multispace0, parse_number, multispace0))(inp);

        if let Ok((inp, (start_num, _, count, _))) = parse_result {
            input = inp;

            // Parse each entry in this subsection
            for i in 0..count {
                // Use checked_add to prevent overflow - skip invalid entries
                let obj_num = match (start_num as u32).checked_add(i as u32) {
                    Some(num) => num,
                    None => {
                        // Skip this entry if object number would overflow
                        if let Ok((inp, _)) =
                            take_until::<_, _, nom::error::Error<&[u8]>>("\n")(input)
                        {
                            input = &inp[1..];
                        }
                        continue;
                    }
                };

                // Parse xref entry: "offset generation n/f"
                let parse_entry = tuple((
                    parse_number,
                    multispace0,
                    parse_number,
                    multispace0,
                    take_while1(|c| c == b'n' || c == b'f'),
                    multispace0,
                ))(input);

                if let Ok((inp, (offset, _, _generation, _, in_use, _))) = parse_entry {
                    input = inp;

                    // Only store entries marked as 'in use' (n)
                    if in_use[0] == b'n' {
                        xref_map.insert(obj_num, offset);
                    }
                } else {
                    // If we can't parse an entry, skip to next line
                    if let Ok((inp, _)) = take_until::<_, _, nom::error::Error<&[u8]>>("\n")(input)
                    {
                        input = &inp[1..];
                    } else {
                        break;
                    }
                }
            }
        } else {
            // Can't parse subsection header, stop
            break;
        }
    }

    Ok((input, xref_map))
}

/// Parses the Info object and extracts the dictionary
fn parse_info_object(input: &[u8]) -> Result<HashMap<String, String>> {
    // Find the dictionary start "<<"
    let input_str = str::from_utf8(input)
        .map_err(|_| ExifToolError::parse_error("Info object contains invalid UTF-8"))?;

    let dict_start = input_str
        .find("<<")
        .ok_or_else(|| ExifToolError::parse_error("Info dictionary start << not found"))?;

    let dict_end = input_str[dict_start..]
        .find(">>")
        .ok_or_else(|| ExifToolError::parse_error("Info dictionary end >> not found"))?;

    // Use checked_add to prevent overflow when calculating dictionary bounds
    let content_start = dict_start
        .checked_add(2)
        .ok_or_else(|| ExifToolError::parse_error("Dictionary offset overflow"))?;
    let content_end = dict_start
        .checked_add(dict_end)
        .ok_or_else(|| ExifToolError::parse_error("Dictionary end offset overflow"))?;
    if content_end > input_str.len() {
        return Err(ExifToolError::parse_error(
            "Dictionary extends beyond input",
        ));
    }
    let dict_content = &input_str[content_start..content_end];

    // Parse dictionary entries
    let (_, entries) = parse_dict_entries(dict_content.as_bytes())
        .map_err(|_| ExifToolError::parse_error("Failed to parse Info dictionary entries"))?;

    Ok(entries)
}

/// Parses dictionary entries (key-value pairs)
fn parse_dict_entries(input: &[u8]) -> IResult<&[u8], HashMap<String, String>> {
    let (input, pairs) = many0(parse_dict_entry)(input)?;
    let dict = pairs.into_iter().collect();
    Ok((input, dict))
}

/// Parses a single dictionary entry: /Key (Value) or /Key <hex>
fn parse_dict_entry(input: &[u8]) -> IResult<&[u8], (String, String)> {
    // Skip whitespace
    let (input, _) = multispace0(input)?;

    // Parse key: /KeyName
    let (input, _) = tag(b"/")(input)?;
    let (input, key) = take_while1(|c: u8| c.is_ascii_alphanumeric())(input)?;
    let key = str::from_utf8(key)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    // Skip whitespace
    let (input, _) = space0(input)?;

    // Parse value (string literal, hex string, or other)
    let (input, value) = parse_dict_value(input)?;

    Ok((input, (key.to_string(), value)))
}

/// Parses a dictionary value (handles string literals and hex strings)
fn parse_dict_value(input: &[u8]) -> IResult<&[u8], String> {
    // Try parsing as string literal: (...)
    if let Ok((input, value)) = parse_string_literal(input) {
        return Ok((input, value));
    }

    // Try parsing as hex string: <...>
    if let Ok((input, value)) = parse_hex_string(input) {
        return Ok((input, value));
    }

    // Try parsing as name: /Name
    if let Ok((input, value)) = parse_name_value(input) {
        return Ok((input, value));
    }

    // Try parsing as number
    if let Ok((input, num)) = parse_number(input) {
        return Ok((input, num.to_string()));
    }

    // Default: take until next key or end
    let (input, _) = take_while(|c| c != b'/')(input)?;
    Ok((input, String::new()))
}

/// Parses a PDF string literal: (text)
fn parse_string_literal(input: &[u8]) -> IResult<&[u8], String> {
    let (input, content) = delimited(tag(b"("), take_while(|c| c != b')'), tag(b")"))(input)?;

    let text = str::from_utf8(content)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    Ok((input, text.to_string()))
}

/// Parses a PDF hex string: <hexdigits>
fn parse_hex_string(input: &[u8]) -> IResult<&[u8], String> {
    let (input, content) = delimited(
        tag(b"<"),
        take_while(|c: u8| c.is_ascii_hexdigit() || c.is_ascii_whitespace()),
        tag(b">"),
    )(input)?;

    // Convert hex to string
    let hex_str = str::from_utf8(content)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    // Remove whitespace and convert hex pairs to bytes
    let hex_clean: String = hex_str.chars().filter(|c| !c.is_whitespace()).collect();

    // Try to decode as UTF-16BE (common for PDF hex strings with non-ASCII)
    if hex_clean.len() >= 4 && hex_clean.starts_with("FEFF") {
        // UTF-16BE with BOM
        let bytes: std::result::Result<Vec<u8>, _> = (4..hex_clean.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex_clean[i..i + 2], 16))
            .collect();

        if let Ok(bytes) = bytes {
            // Convert UTF-16BE bytes to string
            let u16_vec: Vec<u16> = bytes
                .chunks(2)
                .filter_map(|chunk| {
                    if chunk.len() == 2 {
                        Some(u16::from_be_bytes([chunk[0], chunk[1]]))
                    } else {
                        None
                    }
                })
                .collect();

            if let Ok(decoded) = String::from_utf16(&u16_vec) {
                return Ok((input, decoded));
            }
        }
    }

    // Otherwise treat as Latin-1/ASCII
    let bytes: std::result::Result<Vec<u8>, _> = (0..hex_clean.len())
        .step_by(2)
        .map(|i| {
            if i + 1 < hex_clean.len() {
                u8::from_str_radix(&hex_clean[i..i + 2], 16)
            } else {
                u8::from_str_radix(&hex_clean[i..i + 1], 16)
            }
        })
        .collect();

    let text = bytes
        .map(|b| String::from_utf8_lossy(&b).to_string())
        .unwrap_or_default();

    Ok((input, text))
}

/// Parses a PDF name value: /Name
fn parse_name_value(input: &[u8]) -> IResult<&[u8], String> {
    let (input, _) = tag(b"/")(input)?;
    let (input, name) =
        take_while1(|c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')(input)?;

    let text = str::from_utf8(name)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    Ok((input, text.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_object_reference() {
        let input = b"4 0 R";
        let result = parse_object_reference(input);
        assert!(result.is_ok());
        let (_, obj_ref) = result.unwrap();
        assert_eq!(obj_ref.object_num, 4);
        assert_eq!(obj_ref.generation, 0);
    }

    #[test]
    fn test_parse_string_literal() {
        let input = b"(Hello World)";
        let result = parse_string_literal(input);
        assert!(result.is_ok());
        let (_, text) = result.unwrap();
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn test_parse_dict_entry() {
        let input = b"/Title (My Document)";
        let result = parse_dict_entry(input);
        assert!(result.is_ok());
        let (_, (key, value)) = result.unwrap();
        assert_eq!(key, "Title");
        assert_eq!(value, "My Document");
    }

    #[test]
    fn test_find_xref_offset() {
        let tail = b"startxref\n1234\n%%EOF";
        let result = find_xref_offset(tail);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1234);
    }
}
