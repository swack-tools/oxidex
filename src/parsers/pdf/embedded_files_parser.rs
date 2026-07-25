//! PDF Embedded Files metadata parser
//!
//! This module handles parsing of PDF embedded files (attachments) to extract
//! file names, sizes, modification dates, and other metadata.
//!
//! # PDF Embedded Files Structure
//!
//! Embedded files are referenced in the Names dictionary from the Root/Catalog:
//! ```text
//! 1 0 obj
//! << /Type /Catalog
//!    /Names << /EmbeddedFiles 7 0 R >>
//! >>
//! endobj
//! ```
//!
//! The EmbeddedFiles name tree contains file specifications:
//! ```text
//! 7 0 obj
//! << /Names [
//!      (file1.txt) 8 0 R
//!      (file2.pdf) 9 0 R
//!    ]
//! >>
//! endobj
//! ```
//!
//! Each file specification object contains the embedded file stream:
//! ```text
//! 8 0 obj
//! << /Type /Filespec
//!    /F (file1.txt)
//!    /EF << /F 10 0 R >>
//! >>
//! endobj
//!
//! 10 0 obj
//! << /Type /EmbeddedFile
//!    /Length 1234
//!    /Params << /Size 1234 /ModDate (D:20240115120000Z) >>
//! >>
//! stream
//! ... file data ...
//! endstream
//! endobj
//! ```

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::pdf::shared::PdfContext;
use nom::{
    IResult, Parser,
    bytes::complete::{tag, take_until},
    character::complete::{digit1, multispace0},
    combinator::map_res,
    sequence::preceded,
};
use std::str;

//
// ═══════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts embedded file metadata from a PDF file.
///
/// This function:
/// 1. Locates the Root/Catalog dictionary
/// 2. Finds the /Names -> /EmbeddedFiles reference
/// 3. Parses the EmbeddedFiles name tree
/// 4. Extracts file names and counts
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
/// # Extracted Tags
///
/// - `PDF:EmbeddedFileCount`: Total number of embedded files
/// - `PDF:EmbeddedFileNames`: Array of embedded file names
pub fn parse_embedded_files_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    // Load PDF context (xref table and trailer)
    let context = PdfContext::load(reader)?;

    // Navigate to Root/Catalog object
    let root_offset = find_root_offset(reader, &context)?;

    // Read Root object data
    let root_data = reader.read(
        root_offset,
        std::cmp::min(8192, reader.size().saturating_sub(root_offset) as usize),
    )?;

    // Check if /Names dictionary exists
    let names_ref = match find_object_reference(root_data, "/Names") {
        Ok(r) => r,
        Err(_) => {
            return Err(ExifToolError::parse_error(
                "No /Names dictionary found in Root",
            ));
        }
    };

    // Read Names object
    let names_offset = context
        .xref_map
        .get(&names_ref.object_num)
        .copied()
        .ok_or_else(|| ExifToolError::parse_error("Names object not found in xref table"))?;

    let names_data = reader.read(
        names_offset,
        std::cmp::min(4096, reader.size().saturating_sub(names_offset) as usize),
    )?;

    // Check if /EmbeddedFiles exists in Names dictionary
    let embedded_files_ref = match find_object_reference(names_data, "/EmbeddedFiles") {
        Ok(r) => r,
        Err(_) => {
            return Err(ExifToolError::parse_error(
                "No /EmbeddedFiles found in Names dictionary",
            ));
        }
    };

    // Read EmbeddedFiles object
    let ef_offset = context
        .xref_map
        .get(&embedded_files_ref.object_num)
        .copied()
        .ok_or_else(|| {
            ExifToolError::parse_error("EmbeddedFiles object not found in xref table")
        })?;

    let ef_data = reader.read(
        ef_offset,
        std::cmp::min(4096, reader.size().saturating_sub(ef_offset) as usize),
    )?;

    // Extract file names from /Names array
    let file_names = extract_file_names(ef_data)?;

    // Build metadata map
    let mut metadata = MetadataMap::with_capacity(2);

    metadata.insert(
        "PDF:EmbeddedFileCount".to_string(),
        TagValue::new_integer(file_names.len() as i64),
    );

    if !file_names.is_empty() {
        metadata.insert(
            "PDF:EmbeddedFileNames".to_string(),
            TagValue::new_array(file_names.into_iter().map(TagValue::new_string).collect()),
        );
    }

    Ok(metadata)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Navigation Helpers
// ═══════════════════════════════════════════════════════════════════════════
//

/// Finds the Root/Catalog object offset
fn find_root_offset(reader: &dyn FileReader, context: &PdfContext) -> Result<u64> {
    let file_size = reader.size();

    // Read tail to find trailer
    let tail_size = std::cmp::min(1024, file_size as usize);
    let tail_offset = file_size.saturating_sub(tail_size as u64);
    let tail_data = reader.read(tail_offset, tail_size)?;

    // Find /Root reference in trailer
    let root_ref = find_object_reference(tail_data, "/Root")?;
    let root_offset = context
        .xref_map
        .get(&root_ref.object_num)
        .copied()
        .ok_or_else(|| ExifToolError::parse_error("Root object not found in xref table"))?;

    Ok(root_offset)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// File Name Extraction
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts file names from /Names array in EmbeddedFiles name tree
fn extract_file_names(data: &[u8]) -> Result<Vec<String>> {
    match parse_names_array(data) {
        Ok((_, names)) => Ok(names),
        Err(_) => Ok(Vec::new()), // No files is acceptable
    }
}

/// Parses the /Names array to extract file names
fn parse_names_array(input: &[u8]) -> IResult<&[u8], Vec<String>> {
    // Find /Names
    let (input, _) = take_until("/Names")(input)?;
    let (input, _) = tag(&b"/Names"[..])(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(&b"["[..])(input)?;

    // Extract all string literals until closing ]
    let mut names = Vec::new();
    let mut remaining = input;

    loop {
        // Skip whitespace
        let (input, _) = multispace0(remaining)?;

        // Check for closing ]
        if input.starts_with(b"]") {
            break;
        }

        // Try to parse string literal (file name)
        match parse_string_literal(input) {
            Ok((new_input, name)) => {
                names.push(name);
                remaining = new_input;

                // Skip the object reference that follows
                if let Ok((skip_input, _)) = parse_object_reference(new_input) {
                    remaining = skip_input;
                }
            }
            Err(_) => {
                // Try to skip object reference
                if let Ok((skip_input, _)) = parse_object_reference(input) {
                    remaining = skip_input;
                } else {
                    break;
                }
            }
        }
    }

    Ok((remaining, names))
}

/// Parses a PDF string literal: (text)
fn parse_string_literal(input: &[u8]) -> IResult<&[u8], String> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(&b"("[..])(input)?;

    let (content, close_pos) = extract_parenthesized_content(input)?;
    let text = String::from_utf8_lossy(&content).to_string();

    Ok((&input[close_pos + 1..], text))
}

/// Extracts content from a parenthesized string
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

//
// ═══════════════════════════════════════════════════════════════════════════
// Basic Nom Parsers
// ═══════════════════════════════════════════════════════════════════════════
//

/// Object reference structure (e.g., "5 0 R")
#[derive(Debug, Clone, Copy)]
struct ObjectRef {
    object_num: u32,
    #[allow(dead_code)]
    generation: u16,
}

/// Parses an object reference like "5 0 R"
fn parse_object_reference(input: &[u8]) -> IResult<&[u8], ObjectRef> {
    let (input, _) = multispace0(input)?;
    let (input, object_num) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, generation) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(&b"R"[..])(input)?;

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
    )
    .parse(input)
}

/// Finds an object reference by key name (e.g., "/Root", "/Names")
fn find_object_reference(data: &[u8], key: &str) -> Result<ObjectRef> {
    let (_, obj_ref) = parse_dict_reference(data, key)
        .map_err(|_| ExifToolError::parse_error(format!("Could not parse {} reference", key)))?;
    Ok(obj_ref)
}

/// Parses a dictionary reference for the given key
fn parse_dict_reference<'a>(input: &'a [u8], key: &str) -> IResult<&'a [u8], ObjectRef> {
    let (input, _) = take_until(key.as_bytes())(input)?;
    let (input, _) = tag(key.as_bytes())(input)?;
    let (input, _) = multispace0(input)?;
    parse_object_reference(input)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════
//

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_names_array() {
        let data = b"/Names [(file1.txt) 8 0 R (file2.pdf) 9 0 R]";
        let result = parse_names_array(data);
        assert!(result.is_ok());
        let (_, names) = result.unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(names[0], "file1.txt");
        assert_eq!(names[1], "file2.pdf");
    }

    #[test]
    fn test_parse_string_literal() {
        let input = b"(test.txt)";
        let result = parse_string_literal(input);
        assert!(result.is_ok());
        let (_, text) = result.unwrap();
        assert_eq!(text, "test.txt");
    }
}
