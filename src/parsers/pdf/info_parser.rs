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
    character::complete::{digit1, multispace0},
    combinator::map_res,
    multi::many0,
    sequence::{delimited, preceded},
    IResult,
};
use std::collections::HashMap;
use std::str;

//
// ═══════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════
//

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
    // Load PDF navigation context (xref table and trailer)
    let context = PdfContext::load(reader)?;

    // Find and parse the Info dictionary
    let info_ref = find_dict_reference(&context.xref_data, "/Info")?;
    let info_offset = context.get_object_offset(info_ref.object_num, "Info")?;
    let info_data = reader.read(
        info_offset,
        std::cmp::min(4096, reader.size().saturating_sub(info_offset) as usize),
    )?;
    let info_dict = parse_info_object(info_data)?;

    // Convert dictionary to metadata map with proper formatting
    let mut metadata = convert_info_dict_to_metadata(info_dict);

    // Add additional document properties
    if let Ok(page_count) = extract_page_count_from_context(reader, &context) {
        metadata.insert(
            "PDF:PageCount".to_string(),
            TagValue::new_integer(page_count as i64),
        );
    }

    if let Ok(media_box) = extract_media_box_from_context(reader, &context) {
        metadata.insert("PDF:MediaBox".to_string(), TagValue::new_string(media_box));
    }

    Ok(metadata)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Navigation Context
// ═══════════════════════════════════════════════════════════════════════════
//

/// Encapsulates the PDF cross-reference table and trailer data needed for
/// navigating the PDF object structure. This avoids repeatedly reading and
/// parsing the same data.
struct PdfContext {
    xref_data: Vec<u8>,
    xref_map: HashMap<u32, u64>,
}

impl PdfContext {
    /// Loads the PDF context by reading the trailer and xref table from the file.
    /// This centralizes the common logic of finding and parsing the xref table.
    fn load(reader: &dyn FileReader) -> Result<Self> {
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

        // Parse xref table to build object offset map
        let xref_map = parse_xref_table(xref_data)?;

        Ok(PdfContext {
            xref_data: xref_data.to_vec(),
            xref_map,
        })
    }

    /// Gets the file offset for a given object number, with descriptive error messages.
    fn get_object_offset(&self, object_num: u32, object_type: &str) -> Result<u64> {
        self.xref_map.get(&object_num).copied().ok_or_else(|| {
            ExifToolError::parse_error(format!(
                "{} object {} not found in xref table",
                object_type, object_num
            ))
        })
    }
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Metadata Conversion
// ═══════════════════════════════════════════════════════════════════════════
//

/// Converts a raw Info dictionary to a MetadataMap with proper formatting.
/// Handles special cases like date formatting and keyword parsing.
fn convert_info_dict_to_metadata(info_dict: HashMap<String, String>) -> MetadataMap {
    let mut metadata = MetadataMap::with_capacity(info_dict.len() + 1);

    for (key, value) in info_dict {
        match key.as_str() {
            "CreationDate" => {
                insert_date_metadata(&mut metadata, "CreationDate", "CreateDate", &value)
            }
            "ModDate" => insert_date_metadata(&mut metadata, "ModDate", "ModifyDate", &value),
            "Keywords" => insert_keywords_metadata(&mut metadata, &value),
            _ => {
                metadata.insert(format!("PDF:{}", key), TagValue::new_string(value));
            }
        }
    }

    metadata
}

/// Inserts date metadata with proper formatting. PDF dates are converted to EXIF format
/// and stored under both the standard and alternate key names.
fn insert_date_metadata(
    metadata: &mut MetadataMap,
    primary_key: &str,
    alternate_key: &str,
    value: &str,
) {
    if let Some(formatted_date) = format_pdf_date(value) {
        metadata.insert(
            format!("PDF:{}", primary_key),
            TagValue::new_string(formatted_date.clone()),
        );
        metadata.insert(
            format!("PDF:{}", alternate_key),
            TagValue::new_string(formatted_date),
        );
    }
}

/// Inserts keywords metadata, parsing comma-separated values into an array if needed.
fn insert_keywords_metadata(metadata: &mut MetadataMap, value: &str) {
    let keyword_values: Vec<String> = value
        .split(',')
        .map(|keyword| keyword.trim())
        .filter(|keyword| !keyword.is_empty())
        .map(|keyword| keyword.to_string())
        .collect();

    let tag_value = match keyword_values.len() {
        0 => TagValue::new_string(value.to_string()),
        1 => TagValue::new_string(keyword_values[0].clone()),
        _ => TagValue::new_array(
            keyword_values
                .into_iter()
                .map(TagValue::new_string)
                .collect(),
        ),
    };

    metadata.insert("PDF:Keywords".to_string(), tag_value);
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Date Formatting
// ═══════════════════════════════════════════════════════════════════════════
//

/// Formats a PDF date string to EXIF format.
///
/// PDF date format: D:YYYYMMDDHHmmSSOHH'mm'
/// EXIF format: YYYY:MM:DD HH:MM:SS+HH:mm
///
/// Examples:
/// - D:20240115143000+00'00' → 2024:01:15 14:30:00+00:00
/// - D:20240115143000Z → 2024:01:15 14:30:00+00:00
/// - D:20240115 → 2024:01:15 00:00:00
fn format_pdf_date(pdf_date: &str) -> Option<String> {
    // Remove "D:" prefix if present
    let date_str = pdf_date.strip_prefix("D:").unwrap_or(pdf_date);

    // Minimum: YYYYMMDD (8 chars)
    if date_str.len() < 8 {
        return None;
    }

    // Extract date components with defaults for missing parts
    let year = &date_str[0..4];
    let month = &date_str[4..6];
    let day = &date_str[6..8];
    let hour = extract_date_component(date_str, 8, 10, "00");
    let minute = extract_date_component(date_str, 10, 12, "00");
    let second = extract_date_component(date_str, 12, 14, "00");
    let timezone = extract_timezone(date_str);

    Some(format!(
        "{}:{}:{} {}:{}:{}{}",
        year, month, day, hour, minute, second, timezone
    ))
}

/// Extracts a date component from the date string, returning a default if not present.
fn extract_date_component<'a>(
    date_str: &'a str,
    start: usize,
    end: usize,
    default: &'a str,
) -> &'a str {
    if date_str.len() >= end {
        &date_str[start..end]
    } else {
        default
    }
}

/// Extracts and formats timezone information from a PDF date string.
/// Returns empty string if no timezone, "Z" for UTC, or "+HH:mm"/"-HH:mm" for offset.
fn extract_timezone(date_str: &str) -> String {
    if date_str.len() <= 14 {
        return String::new();
    }

    let tz_part = &date_str[14..];

    if tz_part.starts_with('Z') {
        // Z indicates UTC - format might be just "Z" or "Z00'00'"
        "Z".to_string()
    } else if let Some(sign) = tz_part.chars().next().filter(|&c| c == '+' || c == '-') {
        // Format: +HH'mm' or -HH'mm'
        if tz_part.len() >= 3 {
            let tz_hour = &tz_part[1..3];
            let tz_min = if tz_part.len() >= 6 {
                &tz_part[4..6] // Skip apostrophe at position 3
            } else {
                "00"
            };
            format!("{}{}:{}", sign, tz_hour, tz_min)
        } else {
            "+00:00".to_string()
        }
    } else {
        String::new()
    }
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Page Count and Media Box Extraction
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts page count from PDF document catalog using the provided context.
/// The page count is stored in the Pages object tree, referenced from the Root.
fn extract_page_count_from_context(reader: &dyn FileReader, context: &PdfContext) -> Result<u32> {
    // Navigate: Trailer -> Root -> Pages -> Count
    let pages_data = navigate_to_pages_object(reader, context)?;
    extract_count_from_pages(&pages_data)
}

/// Extracts media box from PDF document pages tree using the provided context.
/// The media box defines the boundaries of the page (format: [x1, y1, x2, y2]).
fn extract_media_box_from_context(reader: &dyn FileReader, context: &PdfContext) -> Result<String> {
    // Navigate: Trailer -> Root -> Pages -> MediaBox
    let pages_data = navigate_to_pages_object(reader, context)?;
    extract_media_box_from_pages(&pages_data)
}

/// Navigates from the trailer to the Pages object, centralizing the common
/// navigation logic used by both page count and media box extraction.
fn navigate_to_pages_object(reader: &dyn FileReader, context: &PdfContext) -> Result<Vec<u8>> {
    // Find /Root reference in trailer
    let root_ref = find_dict_reference(&context.xref_data, "/Root")?;
    let root_offset = context.get_object_offset(root_ref.object_num, "Root")?;

    // Read Root object
    let root_size = std::cmp::min(4096, reader.size().saturating_sub(root_offset) as usize);
    let root_data = reader.read(root_offset, root_size)?;

    // Find /Pages reference in Root object
    let pages_ref = find_dict_reference(root_data, "/Pages")?;
    let pages_offset = context.get_object_offset(pages_ref.object_num, "Pages")?;

    // Read Pages object
    let pages_size = std::cmp::min(4096, reader.size().saturating_sub(pages_offset) as usize);
    Ok(reader.read(pages_offset, pages_size)?.to_vec())
}

/// Extracts /Count value from Pages object
fn extract_count_from_pages(pages_data: &[u8]) -> Result<u32> {
    let (_, count) = parse_count(pages_data)
        .map_err(|_| ExifToolError::parse_error("Could not parse /Count from Pages object"))?;
    Ok(count as u32)
}

/// Extracts /MediaBox value from Pages object
fn extract_media_box_from_pages(pages_data: &[u8]) -> Result<String> {
    let (_, media_box) = parse_media_box(pages_data)
        .map_err(|_| ExifToolError::parse_error("Could not parse /MediaBox from Pages object"))?;
    Ok(media_box)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Structure Types
// ═══════════════════════════════════════════════════════════════════════════
//

/// Object reference structure (e.g., "4 0 R" means object 4, generation 0)
#[derive(Debug, Clone, Copy)]
struct ObjectRef {
    object_num: u32,
    #[allow(dead_code)]
    generation: u16,
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Reference Finding (Generic)
// ═══════════════════════════════════════════════════════════════════════════
//

/// Finds a dictionary reference by key name. This generic function replaces the
/// separate find_info_reference, find_root_reference, and find_pages_reference functions.
///
/// # Parameters
/// - `data`: The PDF data to search (trailer or object data)
/// - `key`: The dictionary key to find (e.g., "/Info", "/Root", "/Pages")
fn find_dict_reference(data: &[u8], key: &str) -> Result<ObjectRef> {
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
// XRef Table Parsing
// ═══════════════════════════════════════════════════════════════════════════
//

/// Finds the startxref offset from the PDF tail
fn find_xref_offset(tail_data: &[u8]) -> Result<u64> {
    let tail_str = str::from_utf8(tail_data)
        .map_err(|_| ExifToolError::parse_error("PDF tail contains invalid UTF-8"))?;

    let startxref_pos = tail_str
        .rfind("startxref")
        .ok_or_else(|| ExifToolError::parse_error("startxref not found in PDF"))?;

    let after_start = startxref_pos
        .checked_add(9) // "startxref".len()
        .ok_or_else(|| ExifToolError::parse_error("Offset overflow after startxref"))?;

    if after_start > tail_str.len() {
        return Err(ExifToolError::parse_error("Invalid startxref position"));
    }

    let after_keyword = &tail_str[after_start..];
    let (_, offset) = parse_number(after_keyword.as_bytes())
        .map_err(|_| ExifToolError::parse_error("Invalid xref offset after startxref"))?;

    Ok(offset)
}

/// Parses the xref table and builds a map of object numbers to file offsets
fn parse_xref_table(xref_data: &[u8]) -> Result<HashMap<u32, u64>> {
    let xref_str = str::from_utf8(xref_data)
        .map_err(|_| ExifToolError::parse_error("xref table contains invalid UTF-8"))?;

    let xref_pos = xref_str
        .find("xref")
        .ok_or_else(|| ExifToolError::parse_error("xref table not found"))?;

    let after_xref = &xref_str[xref_pos + 4..]; // "xref".len() = 4

    let (_, xref_map) = parse_xref_entries(after_xref.as_bytes())
        .map_err(|_| ExifToolError::parse_error("Failed to parse xref entries"))?;

    Ok(xref_map)
}

/// Parses xref entries and returns a map of object numbers to offsets.
/// This has been refactored to extract helper functions and reduce nesting.
fn parse_xref_entries(input: &[u8]) -> IResult<&[u8], HashMap<u32, u64>> {
    let mut xref_map = HashMap::new();
    let mut input = input;

    // Parse subsections until we hit "trailer"
    loop {
        let (inp, _) = multispace0(input)?;

        // Check if we've reached trailer
        if inp.starts_with(b"trailer") {
            break;
        }

        // Try to parse subsection header
        match parse_xref_subsection_header(inp) {
            Ok((new_input, (start_num, count))) => {
                input = new_input;
                input = parse_xref_subsection_entries(input, start_num, count, &mut xref_map);
            }
            Err(_) => break, // Can't parse subsection header, stop
        }
    }

    Ok((input, xref_map))
}

/// Parses a single xref subsection header: "start_obj_num count"
fn parse_xref_subsection_header(input: &[u8]) -> IResult<&[u8], (u64, u64)> {
    let (input, start_num) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, count) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    Ok((input, (start_num, count)))
}

/// Parses all entries in a single xref subsection, updating the xref_map.
/// Returns the remaining input after processing all entries.
fn parse_xref_subsection_entries<'a>(
    mut input: &'a [u8],
    start_num: u64,
    count: u64,
    xref_map: &mut HashMap<u32, u64>,
) -> &'a [u8] {
    for i in 0..count {
        // Calculate object number with overflow protection
        let obj_num = match (start_num as u32).checked_add(i as u32) {
            Some(num) => num,
            None => {
                input = skip_to_next_line(input);
                continue;
            }
        };

        // Parse single xref entry
        match parse_xref_entry(input) {
            Ok((new_input, Some(offset))) => {
                xref_map.insert(obj_num, offset);
                input = new_input;
            }
            Ok((new_input, None)) => {
                // Entry was 'free' (not in use), skip it
                input = new_input;
            }
            Err(_) => {
                // Failed to parse entry, skip to next line
                input = skip_to_next_line(input);
            }
        }
    }
    input
}

/// Parses a single xref entry: "offset generation n/f"
/// Returns Some(offset) if entry is in use ('n'), None if free ('f')
fn parse_xref_entry(input: &[u8]) -> IResult<&[u8], Option<u64>> {
    let (input, offset) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _generation) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, in_use) = take_while1(|c| c == b'n' || c == b'f')(input)?;
    let (input, _) = multispace0(input)?;

    // Only return offset if entry is 'in use' (n)
    let result = if in_use[0] == b'n' {
        Some(offset)
    } else {
        None
    };

    Ok((input, result))
}

/// Skips to the next line in the input, used for error recovery
fn skip_to_next_line(input: &[u8]) -> &[u8] {
    if let Ok((new_input, _)) = take_until::<_, _, nom::error::Error<&[u8]>>("\n")(input) {
        if new_input.is_empty() {
            new_input
        } else {
            &new_input[1..] // Skip the newline itself
        }
    } else {
        &[] // No newline found, return empty
    }
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Info Dictionary Parsing
// ═══════════════════════════════════════════════════════════════════════════
//

/// Parses the Info object and extracts the dictionary
fn parse_info_object(input: &[u8]) -> Result<HashMap<String, String>> {
    let input_str = str::from_utf8(input)
        .map_err(|_| ExifToolError::parse_error("Info object contains invalid UTF-8"))?;

    let dict_start = input_str
        .find("<<")
        .ok_or_else(|| ExifToolError::parse_error("Info dictionary start << not found"))?;

    let dict_end = input_str[dict_start..]
        .find(">>")
        .ok_or_else(|| ExifToolError::parse_error("Info dictionary end >> not found"))?;

    // Calculate dictionary bounds with overflow protection
    let content_start = dict_start
        .checked_add(2) // Skip "<<"
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
    use nom::Parser;
    let (input, pairs) = many0(parse_dict_entry).parse(input)?;
    let dict = pairs.into_iter().collect();
    Ok((input, dict))
}

/// Parses a single dictionary entry: /Key (Value) or /Key <hex>
fn parse_dict_entry(input: &[u8]) -> IResult<&[u8], (String, String)> {
    let (input, _) = multispace0(input)?;

    // Parse key: /KeyName
    let (input, _) = tag(&b"/"[..])(input)?;
    let (input, key) = take_while1(|c: u8| c.is_ascii_alphanumeric())(input)?;
    let key = str::from_utf8(key)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    let (input, _) = multispace0(input)?;

    // Parse value (tries multiple value types in order)
    let (input, value) = parse_dict_value(input)?;

    Ok((input, (key.to_string(), value)))
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Value Parsing
// ═══════════════════════════════════════════════════════════════════════════
//

/// Parses a dictionary value (handles string literals, hex strings, names, and numbers)
fn parse_dict_value(input: &[u8]) -> IResult<&[u8], String> {
    // Try each value type parser in order of likelihood
    parse_string_literal(input)
        .or_else(|_| parse_hex_string(input))
        .or_else(|_| parse_name_value(input))
        .or_else(|_| parse_number(input).map(|(i, n)| (i, n.to_string())))
        .or_else(|_| {
            // Default: skip to next key or end
            let (input, _) = take_while(|c| c != b'/')(input)?;
            Ok((input, String::new()))
        })
}

/// Parses a PDF string literal: (text)
/// Handles escaped parentheses \( and \) and nested parentheses
fn parse_string_literal(input: &[u8]) -> IResult<&[u8], String> {
    let (input, _) = tag(&b"("[..])(input)?;

    // Parse content with depth tracking for nested parentheses
    let (content, close_pos) = extract_parenthesized_content(input)?;
    let text = String::from_utf8_lossy(&content).to_string();

    // Return remaining input (skip the closing paren)
    Ok((&input[close_pos + 1..], text))
}

/// Extracts content from a parenthesized string, handling escapes and nesting.
/// Returns (content_bytes, closing_paren_position)
#[allow(clippy::type_complexity)]
fn extract_parenthesized_content(
    input: &[u8],
) -> std::result::Result<(Vec<u8>, usize), nom::Err<nom::error::Error<&[u8]>>> {
    let mut content = Vec::new();
    let mut i = 0;
    let mut depth = 1; // Track nested parentheses

    while i < input.len() {
        if input[i] == b'\\' && i + 1 < input.len() {
            // Escaped character - include both backslash and next char
            content.push(input[i]);
            content.push(input[i + 1]);
            i += 2;
        } else if input[i] == b'(' {
            // Unescaped opening paren - increase depth
            depth += 1;
            content.push(input[i]);
            i += 1;
        } else if input[i] == b')' {
            // Unescaped closing paren
            depth -= 1;
            if depth == 0 {
                return Ok((content, i)); // Found matching closing paren
            }
            content.push(input[i]);
            i += 1;
        } else {
            content.push(input[i]);
            i += 1;
        }
    }

    // Unclosed parentheses
    Err(nom::Err::Error(nom::error::Error::new(
        input,
        nom::error::ErrorKind::Char,
    )))
}

/// Parses a PDF hex string: <hexdigits>
fn parse_hex_string(input: &[u8]) -> IResult<&[u8], String> {
    use nom::Parser;
    let (input, content) = delimited(
        tag(&b"<"[..]),
        take_while(|c: u8| c.is_ascii_hexdigit() || c.is_ascii_whitespace()),
        tag(&b">"[..]),
    )
    .parse(input)?;

    let hex_str = str::from_utf8(content)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    // Remove whitespace
    let hex_clean: String = hex_str.chars().filter(|c| !c.is_whitespace()).collect();

    // Try UTF-16BE decoding first, then fall back to Latin-1
    let decoded = decode_hex_string(&hex_clean);
    Ok((input, decoded))
}

/// Decodes a hex string, trying UTF-16BE with BOM first, then Latin-1
fn decode_hex_string(hex_clean: &str) -> String {
    // Try UTF-16BE with BOM (FEFF prefix)
    if hex_clean.len() >= 4 && hex_clean.starts_with("FEFF") {
        if let Some(decoded) = decode_utf16be_hex(&hex_clean[4..]) {
            return decoded;
        }
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

//
// ═══════════════════════════════════════════════════════════════════════════
// Basic Nom Parsers
// ═══════════════════════════════════════════════════════════════════════════
//

/// Parses an object reference like "4 0 R"
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
    use nom::Parser;
    preceded(
        multispace0,
        map_res(map_res(digit1, str::from_utf8), |s: &str| s.parse::<u64>()),
    )
    .parse(input)
}

/// Parses the /Count value from Pages dictionary
fn parse_count(input: &[u8]) -> IResult<&[u8], u64> {
    let (input, _) = take_until("/Count")(input)?;
    let (input, _) = tag(&b"/Count"[..])(input)?;
    let (input, _) = multispace0(input)?;
    parse_number(input)
}

/// Parses the /MediaBox array from Pages dictionary
/// Format: /MediaBox [x1 y1 x2 y2]
/// Returns formatted as "x1, y1, x2, y2"
fn parse_media_box(input: &[u8]) -> IResult<&[u8], String> {
    let (input, _) = take_until("/MediaBox")(input)?;
    let (input, _) = tag(&b"/MediaBox"[..])(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(&b"["[..])(input)?;
    let (input, _) = multispace0(input)?;

    // Parse four numbers
    let (input, x1) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, y1) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, x2) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, y2) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(&b"]"[..])(input)?;

    // Format as comma-separated string (matching ExifTool output)
    Ok((input, format!("{}, {}, {}, {}", x1, y1, x2, y2)))
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
