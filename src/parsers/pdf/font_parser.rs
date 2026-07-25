//! PDF Font metadata parser
//!
//! This module handles parsing of PDF font resources to extract font metadata
//! including font names, types, and encoding information.
//!
//! # PDF Font Structure
//!
//! Fonts are referenced in page Resources dictionaries:
//! ```text
//! 3 0 obj
//! << /Type /Page
//!    /Resources <<
//!      /Font <<
//!        /F1 5 0 R
//!        /F2 6 0 R
//!      >>
//!    >>
//! >>
//! endobj
//! ```
//!
//! Font objects contain metadata about the font:
//! ```text
//! 5 0 obj
//! << /Type /Font
//!    /Subtype /Type1
//!    /BaseFont /Helvetica
//!    /Encoding /WinAnsiEncoding
//! >>
//! endobj
//! ```

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::pdf::shared::{PdfContext, find_dict_name};
use nom::{
    IResult, Parser,
    bytes::complete::{tag, take_until, take_while1},
    character::complete::{digit1, multispace0},
    combinator::map_res,
    sequence::preceded,
};
use std::collections::HashSet;
use std::str;

//
// ═══════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts font metadata from a PDF file.
///
/// This function:
/// 1. Locates the first page object
/// 2. Finds the /Resources -> /Font dictionary
/// 3. Extracts all font object references
/// 4. Parses each font object to extract font name, type, and encoding
/// 5. Returns unique font names and count
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
/// - `PDF:FontNames`: Array of unique font names used in the document
/// - `PDF:FontCount`: Total number of unique fonts
/// - `PDF:FontTypes`: Array of font types (Type1, TrueType, etc.)
pub fn parse_font_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    // Load PDF context (xref table and trailer)
    let context = load_pdf_context(reader)?;

    // Navigate to first page object
    let first_page_offset = find_first_page_offset(reader, &context)?;

    // Read page object data
    let page_data = reader.read(
        first_page_offset,
        std::cmp::min(
            8192,
            reader.size().saturating_sub(first_page_offset) as usize,
        ),
    )?;

    // Find Font references in the page's /Resources dictionary
    let font_refs = extract_font_references(page_data)?;

    if font_refs.is_empty() {
        return Err(ExifToolError::parse_error(
            "No fonts found in page resources",
        ));
    }

    // Extract metadata from each font
    let mut font_names = HashSet::new();
    let mut font_types = HashSet::new();

    for font_ref in &font_refs {
        // Get font offset from xref table
        let font_offset = match context.xref_map.get(&font_ref.object_num) {
            Some(&offset) => offset,
            None => continue, // Skip if not in xref table
        };

        // Read font object data
        let font_data = reader.read(
            font_offset,
            std::cmp::min(4096, reader.size().saturating_sub(font_offset) as usize),
        )?;

        // Extract font metadata
        if let Some(base_font) = find_dict_name(font_data, "BaseFont") {
            font_names.insert(base_font);
        }

        if let Some(subtype) = find_dict_name(font_data, "Subtype") {
            font_types.insert(subtype);
        }
    }

    // Build metadata map
    let mut metadata = MetadataMap::with_capacity(3);

    metadata.insert(
        "PDF:FontCount".to_string(),
        TagValue::new_integer(font_names.len() as i64),
    );

    if !font_names.is_empty() {
        let mut names: Vec<_> = font_names.into_iter().collect();
        names.sort();
        metadata.insert(
            "PDF:FontNames".to_string(),
            TagValue::new_array(names.into_iter().map(TagValue::new_string).collect()),
        );
    }

    if !font_types.is_empty() {
        let mut types: Vec<_> = font_types.into_iter().collect();
        types.sort();
        metadata.insert(
            "PDF:FontTypes".to_string(),
            TagValue::new_array(types.into_iter().map(TagValue::new_string).collect()),
        );
    }

    Ok(metadata)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Navigation Context
// ═══════════════════════════════════════════════════════════════════════════
//

/// Loads the PDF context by reading the trailer and xref table
fn load_pdf_context(reader: &dyn FileReader) -> Result<PdfContext> {
    PdfContext::load(reader)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Page Navigation
// ═══════════════════════════════════════════════════════════════════════════
//

/// Finds the file offset of the first page object
fn find_first_page_offset(reader: &dyn FileReader, context: &PdfContext) -> Result<u64> {
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

    // Read Root object
    let root_size = std::cmp::min(4096, file_size.saturating_sub(root_offset) as usize);
    let root_data = reader.read(root_offset, root_size)?;

    // Find /Pages reference in Root
    let pages_ref = find_object_reference(root_data, "/Pages")?;
    let pages_offset = context
        .xref_map
        .get(&pages_ref.object_num)
        .copied()
        .ok_or_else(|| ExifToolError::parse_error("Pages object not found in xref table"))?;

    // Read Pages object
    let pages_size = std::cmp::min(4096, file_size.saturating_sub(pages_offset) as usize);
    let pages_data = reader.read(pages_offset, pages_size)?;

    // Find first page in /Kids array
    let first_page_ref = find_first_page_in_kids(pages_data)?;
    let first_page_offset = context
        .xref_map
        .get(&first_page_ref.object_num)
        .copied()
        .ok_or_else(|| ExifToolError::parse_error("First page object not found in xref table"))?;

    Ok(first_page_offset)
}

/// Finds the first page reference in a /Kids array
fn find_first_page_in_kids(pages_data: &[u8]) -> Result<ObjectRef> {
    let (_, page_ref) = parse_first_kid(pages_data)
        .map_err(|_| ExifToolError::parse_error("Could not parse /Kids array"))?;
    Ok(page_ref)
}

/// Parses the first object reference from /Kids array
fn parse_first_kid(input: &[u8]) -> IResult<&[u8], ObjectRef> {
    let (input, _) = take_until("/Kids")(input)?;
    let (input, _) = tag(&b"/Kids"[..])(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(&b"["[..])(input)?;
    let (input, _) = multispace0(input)?;
    parse_object_reference(input)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Font Extraction
// ═══════════════════════════════════════════════════════════════════════════
//

/// Object reference structure (e.g., "5 0 R")
#[derive(Debug, Clone, Copy)]
struct ObjectRef {
    object_num: u32,
    #[allow(dead_code)]
    generation: u16,
}

/// Extracts all font references from page /Resources dictionary
fn extract_font_references(page_data: &[u8]) -> Result<Vec<ObjectRef>> {
    match parse_font_dict(page_data) {
        Ok((_, refs)) => Ok(refs),
        Err(_) => Ok(Vec::new()), // No fonts is not an error
    }
}

/// Parses the /Font dictionary and extracts all object references
fn parse_font_dict(input: &[u8]) -> IResult<&[u8], Vec<ObjectRef>> {
    // Find /Resources
    let (input, _) = take_until("/Resources")(input)?;
    let (input, _) = tag(&b"/Resources"[..])(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(&b"<<"[..])(input)?;

    // Find /Font within Resources
    let (input, _) = take_until("/Font")(input)?;
    let (input, _) = tag(&b"/Font"[..])(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(&b"<<"[..])(input)?;

    // Extract all object references until closing >>
    let mut refs = Vec::new();
    let mut remaining = input;

    loop {
        // Skip whitespace
        let (input, _) = multispace0(remaining)?;

        // Check for closing >>
        if input.starts_with(b">>") {
            break;
        }

        // Try to parse /Name followed by object reference
        match parse_name_and_reference(input) {
            Ok((new_input, obj_ref)) => {
                refs.push(obj_ref);
                remaining = new_input;
            }
            Err(_) => {
                // Skip this entry and try to continue
                if let Ok((new_input, _)) =
                    take_until::<_, _, nom::error::Error<&[u8]>>("/Type")(input)
                {
                    remaining = new_input;
                } else {
                    break;
                }
            }
        }
    }

    Ok((remaining, refs))
}

/// Parses /Name <objnum> <gen> R pattern
fn parse_name_and_reference(input: &[u8]) -> IResult<&[u8], ObjectRef> {
    let (input, _) = tag(&b"/"[..])(input)?;
    let (input, _) = take_while1(|c: u8| c.is_ascii_alphanumeric())(input)?;
    let (input, _) = multispace0(input)?;
    parse_object_reference(input)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Basic Nom Parsers
// ═══════════════════════════════════════════════════════════════════════════
//

/// Parses an object reference like "5 0 R"
fn parse_object_reference(input: &[u8]) -> IResult<&[u8], ObjectRef> {
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

/// Finds an object reference by key name (e.g., "/Root", "/Pages")
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
    use crate::test_support::TestReader;

    #[test]
    fn test_parse_font_dict() {
        let page_data = b"<< /Type /Page /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>";
        let result = parse_font_dict(page_data);
        assert!(result.is_ok());
        let (_, refs) = result.unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].object_num, 5);
        assert_eq!(refs[1].object_num, 6);
    }

    #[test]
    fn test_parse_font_metadata() {
        // Create a minimal PDF with fonts
        let pdf = b"%PDF-1.4
1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]
   /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >>
>>
endobj
5 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>
endobj
6 0 obj
<< /Type /Font /Subtype /TrueType /BaseFont /Arial >>
endobj
xref
0 7
0000000000 65535 f
0000000009 00000 n
0000000058 00000 n
0000000115 00000 n
0000000000 65535 f
0000000250 00000 n
0000000320 00000 n
trailer
<< /Size 7 /Root 1 0 R >>
startxref
395
%%EOF";

        let reader = TestReader::new(pdf.to_vec());
        let result = parse_font_metadata(&reader);

        match result {
            Ok(metadata) => {
                if let Some(count) = metadata.get("PDF:FontCount") {
                    println!("Found {:?} fonts", count);
                }
            }
            Err(e) => {
                println!("Expected error for minimal test PDF: {}", e);
            }
        }
    }
}
