//! Shared utilities for PDF parsing
//!
//! This module provides common helper functions used across multiple PDF parsers,
//! including dictionary value extraction, object reference resolution, and
//! navigation context management.

use crate::core::FileReader;
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use nom::{
    IResult, Parser,
    bytes::complete::{tag, take_while1},
    character::complete::{digit1, multispace0},
    combinator::map_res,
    sequence::preceded,
};
use std::str;

//
// ═══════════════════════════════════════════════════════════════════════════
// Public Re-exports
// ═══════════════════════════════════════════════════════════════════════════
//

pub use crate::parsers::pdf::info_parser::PdfContext;

//
// ═══════════════════════════════════════════════════════════════════════════
// Object Reference Type
// ═══════════════════════════════════════════════════════════════════════════
//

/// Object reference structure (e.g., "4 0 R" means object 4, generation 0)
#[derive(Debug, Clone, Copy)]
pub struct ObjectRef {
    /// The object number in the PDF file
    pub object_num: u32,
    /// The generation number (usually 0)
    #[allow(dead_code)]
    pub generation: u16,
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Dictionary Value Extraction Helpers
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts a string value for a given key from a PDF dictionary.
///
/// Supports various PDF value types: string literals, hex strings, names, and numbers.
///
/// # Parameters
///
/// - `data`: The PDF dictionary data to search
/// - `key`: The dictionary key to find (e.g., "SourceModified")
///
/// # Returns
///
/// - `Some(String)`: The extracted value
/// - `None`: Key not found or value could not be parsed
///
/// # Examples
///
/// ```ignore
/// let data = b"<< /Title (My Document) /Author (John Doe) >>";
/// assert_eq!(find_dict_value(data, "Title"), Some("My Document".to_string()));
/// ```
pub fn find_dict_value(data: &[u8], key: &str) -> Option<String> {
    // Search for the key pattern: /Key
    let search_pattern = format!("/{}", key);
    let key_bytes = search_pattern.as_bytes();

    // Find the key in the data
    let key_pos = data
        .windows(key_bytes.len())
        .position(|window| window == key_bytes)?;

    // Skip past the key
    let after_key = &data[key_pos + key_bytes.len()..];

    // Parse the value after the key
    parse_dict_value_generic(after_key).ok().map(|(_, v)| v)
}

/// Extracts a boolean value for a given key from a PDF dictionary.
///
/// PDF boolean values can be:
/// - `/true` or `/True` for true
/// - `/false` or `/False` for false
///
/// # Parameters
///
/// - `data`: The PDF dictionary data to search
/// - `key`: The dictionary key to find (e.g., "Linearized")
///
/// # Returns
///
/// - `Some(true)`: If value is /true or /True
/// - `Some(false)`: If value is /false or /False
/// - `None`: Key not found or value is not a boolean
///
/// # Examples
///
/// ```ignore
/// let data = b"<< /Linearized /true /Encrypted /false >>";
/// assert_eq!(find_dict_bool(data, "Linearized"), Some(true));
/// assert_eq!(find_dict_bool(data, "Encrypted"), Some(false));
/// ```
pub fn find_dict_bool(data: &[u8], key: &str) -> Option<bool> {
    let value_str = find_dict_value(data, key)?;
    match value_str.to_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Extracts a PDF name value for a given key from a PDF dictionary.
///
/// PDF names start with a forward slash (e.g., `/SinglePage`, `/Unknown`).
/// This function returns the name without the leading slash.
///
/// # Parameters
///
/// - `data`: The PDF dictionary data to search
/// - `key`: The dictionary key to find (e.g., "PageLayout")
///
/// # Returns
///
/// - `Some(String)`: The extracted name value (without leading slash)
/// - `None`: Key not found or value is not a name
///
/// # Examples
///
/// ```ignore
/// let data = b"<< /PageLayout /SinglePage /Trapped /Unknown >>";
/// assert_eq!(find_dict_name(data, "PageLayout"), Some("SinglePage".to_string()));
/// assert_eq!(find_dict_name(data, "Trapped"), Some("Unknown".to_string()));
/// ```
pub fn find_dict_name(data: &[u8], key: &str) -> Option<String> {
    // For name values, we expect the result to start with '/'
    // But find_dict_value already strips it, so we just validate it's alphanumeric
    let value = find_dict_value(data, key)?;

    // PDF names should be alphanumeric (after the slash is stripped)
    if value
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        Some(value)
    } else {
        None
    }
}

/// Reads a PDF object by its reference from the file.
///
/// This function uses the cross-reference table to locate the object,
/// then reads it from the file.
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PDF file
/// - `context`: PDF navigation context containing the xref table
/// - `obj_ref`: Object reference to read (e.g., object 4, generation 0)
///
/// # Returns
///
/// - `Ok(Vec<u8>)`: The raw object data
/// - `Err(ExifToolError)`: Object not found or I/O error
///
/// # Examples
///
/// ```ignore
/// let context = PdfContext::load(reader)?;
/// let obj_ref = ObjectRef { object_num: 4, generation: 0 };
/// let obj_data = find_object_by_ref(reader, &context, obj_ref)?;
/// ```
pub fn find_object_by_ref(
    reader: &dyn FileReader,
    context: &PdfContext,
    obj_ref: ObjectRef,
) -> Result<Vec<u8>> {
    // Get object offset from xref table
    let offset = context
        .xref_map
        .get(&obj_ref.object_num)
        .copied()
        .ok_or_else(|| {
            ExifToolError::parse_error(format!(
                "Object {} not found in xref table",
                obj_ref.object_num
            ))
        })?;

    // Read object data (read up to 4KB)
    let obj_size = std::cmp::min(4096, reader.size().saturating_sub(offset) as usize);
    let obj_data = reader.read(offset, obj_size)?;

    Ok(obj_data.to_vec())
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Generic PDF Value Parsers
// ═══════════════════════════════════════════════════════════════════════════
//

/// Parses a generic PDF dictionary value.
/// Handles string literals, hex strings, names, numbers, and booleans.
fn parse_dict_value_generic(input: &[u8]) -> IResult<&[u8], String> {
    // Skip whitespace before value
    let (input, _) = multispace0(input)?;

    // Try each value type parser in order of likelihood
    parse_string_literal(input)
        .or_else(|_| parse_hex_string(input))
        .or_else(|_| parse_name_value(input))
        .or_else(|_| parse_boolean_value(input))
        .or_else(|_| parse_number(input).map(|(i, n)| (i, n.to_string())))
}

/// Parses a PDF string literal: (text)
/// Handles escaped characters and nested parentheses
fn parse_string_literal(input: &[u8]) -> IResult<&[u8], String> {
    let (input, _) = tag(&b"("[..])(input)?;

    let (content, close_pos) = extract_parenthesized_content(input)?;
    let text = String::from_utf8_lossy(&content).to_string();

    Ok((&input[close_pos + 1..], text))
}

/// Extracts content from a parenthesized string, handling escapes and nesting.
#[allow(clippy::type_complexity)]
fn extract_parenthesized_content(
    input: &[u8],
) -> std::result::Result<(Vec<u8>, usize), nom::Err<nom::error::Error<&[u8]>>> {
    let mut content = Vec::new();
    let mut i = 0;
    let mut depth = 1;

    while i < input.len() {
        if input[i] == b'\\' && i + 1 < input.len() {
            content.push(input[i]);
            content.push(input[i + 1]);
            i += 2;
        } else if input[i] == b'(' {
            depth += 1;
            content.push(input[i]);
            i += 1;
        } else if input[i] == b')' {
            depth -= 1;
            if depth == 0 {
                return Ok((content, i));
            }
            content.push(input[i]);
            i += 1;
        } else {
            content.push(input[i]);
            i += 1;
        }
    }

    Err(nom::Err::Error(nom::error::Error::new(
        input,
        nom::error::ErrorKind::Char,
    )))
}

/// Parses a PDF hex string: <hexdigits>
fn parse_hex_string(input: &[u8]) -> IResult<&[u8], String> {
    use nom::bytes::complete::take_while;
    let (input, _) = tag(&b"<"[..])(input)?;
    let (input, content) =
        take_while(|c: u8| c.is_ascii_hexdigit() || c.is_ascii_whitespace())(input)?;
    let (input, _) = tag(&b">"[..])(input)?;

    let hex_str = str::from_utf8(content)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    let hex_clean: String = hex_str.chars().filter(|c| !c.is_whitespace()).collect();
    let decoded = decode_hex_string(&hex_clean);

    Ok((input, decoded))
}

/// Decodes a hex string to UTF-8
fn decode_hex_string(hex_clean: &str) -> String {
    // Try UTF-16BE with BOM first
    if hex_clean.len() >= 4
        && hex_clean.starts_with("FEFF")
        && let Some(decoded) = decode_utf16be_hex(&hex_clean[4..])
    {
        return decoded;
    }

    // Fall back to Latin-1/ASCII
    decode_latin1_hex(hex_clean)
}

/// Decodes UTF-16BE hex string
fn decode_utf16be_hex(hex_str: &str) -> Option<String> {
    let bytes: std::result::Result<Vec<u8>, _> = (0..hex_str.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16))
        .collect();

    let bytes = bytes.ok()?;
    let reader = EndianReader::big_endian(&bytes);
    let u16_vec: Vec<u16> = (0..bytes.len())
        .step_by(2)
        .filter_map(|offset| reader.u16_at(offset))
        .collect();

    String::from_utf16(&u16_vec).ok()
}

/// Decodes Latin-1/ASCII hex string
fn decode_latin1_hex(hex_str: &str) -> String {
    let bytes: std::result::Result<Vec<u8>, _> = (0..hex_str.len())
        .step_by(2)
        .map(|i| {
            if i + 1 < hex_str.len() {
                u8::from_str_radix(&hex_str[i..i + 2], 16)
            } else {
                u8::from_str_radix(&hex_str[i..i + 1], 16)
            }
        })
        .collect();

    bytes
        .map(|b| String::from_utf8_lossy(&b).to_string())
        .unwrap_or_default()
}

/// Parses a PDF name value: /Name
fn parse_name_value(input: &[u8]) -> IResult<&[u8], String> {
    let (input, _) = tag(&b"/"[..])(input)?;
    let (input, name) =
        take_while1(|c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')(input)?;

    let text = str::from_utf8(name)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    Ok((input, text.to_string()))
}

/// Parses a PDF boolean value: true or false (case-insensitive)
fn parse_boolean_value(input: &[u8]) -> IResult<&[u8], String> {
    // Try to match "true" or "false" (case-insensitive)
    let input_str = str::from_utf8(input)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    let lower = input_str.to_lowercase();
    if lower.starts_with("true") {
        Ok((&input[4..], "true".to_string()))
    } else if lower.starts_with("false") {
        Ok((&input[5..], "false".to_string()))
    } else {
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )))
    }
}

/// Parses a decimal number from bytes
fn parse_number(input: &[u8]) -> IResult<&[u8], u64> {
    preceded(
        multispace0,
        map_res(map_res(digit1, str::from_utf8), |s: &str| s.parse::<u64>()),
    )
    .parse(input)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Context (Re-exported from info_parser)
// ═══════════════════════════════════════════════════════════════════════════
//

// Note: PdfContext is defined in info_parser.rs and re-exported at the top
// This allows shared access without circular dependencies

//
// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════
//

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_dict_value_string() {
        let data = b"<< /Title (My Document) /Author (John Doe) >>";
        assert_eq!(
            find_dict_value(data, "Title"),
            Some("My Document".to_string())
        );
        assert_eq!(
            find_dict_value(data, "Author"),
            Some("John Doe".to_string())
        );
    }

    #[test]
    fn test_find_dict_value_name() {
        let data = b"<< /PageLayout /SinglePage /Trapped /Unknown >>";
        assert_eq!(
            find_dict_value(data, "PageLayout"),
            Some("SinglePage".to_string())
        );
        assert_eq!(
            find_dict_value(data, "Trapped"),
            Some("Unknown".to_string())
        );
    }

    #[test]
    fn test_find_dict_value_not_found() {
        let data = b"<< /Title (My Document) >>";
        assert_eq!(find_dict_value(data, "Author"), None);
    }

    #[test]
    fn test_find_dict_bool_true() {
        let data = b"<< /Linearized /true >>";
        assert_eq!(find_dict_bool(data, "Linearized"), Some(true));
    }

    #[test]
    fn test_find_dict_bool_false() {
        let data = b"<< /Encrypted /false >>";
        assert_eq!(find_dict_bool(data, "Encrypted"), Some(false));
    }

    #[test]
    fn test_find_dict_bool_case_insensitive() {
        let data = b"<< /Flag1 /True /Flag2 /False >>";
        assert_eq!(find_dict_bool(data, "Flag1"), Some(true));
        assert_eq!(find_dict_bool(data, "Flag2"), Some(false));
    }

    #[test]
    fn test_find_dict_bool_not_boolean() {
        let data = b"<< /Value /Unknown >>";
        assert_eq!(find_dict_bool(data, "Value"), None);
    }

    #[test]
    fn test_find_dict_name() {
        let data = b"<< /PageLayout /SinglePage /Trapped /Unknown >>";
        assert_eq!(
            find_dict_name(data, "PageLayout"),
            Some("SinglePage".to_string())
        );
        assert_eq!(find_dict_name(data, "Trapped"), Some("Unknown".to_string()));
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
    fn test_parse_name_value() {
        let input = b"/SinglePage";
        let result = parse_name_value(input);
        assert!(result.is_ok());
        let (_, name) = result.unwrap();
        assert_eq!(name, "SinglePage");
    }

    #[test]
    fn test_parse_boolean_value() {
        let input = b"true";
        let result = parse_boolean_value(input);
        assert!(result.is_ok());
        let (_, val) = result.unwrap();
        assert_eq!(val, "true");

        let input = b"false";
        let result = parse_boolean_value(input);
        assert!(result.is_ok());
        let (_, val) = result.unwrap();
        assert_eq!(val, "false");
    }
}
