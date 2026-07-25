//! PDF Root/Catalog dictionary parser
//!
//! This module handles parsing of PDF Root (Catalog) dictionaries to extract
//! document-level settings and preferences.
//!
//! # PDF Root/Catalog Structure
//!
//! The Root dictionary is referenced from the trailer and contains the document catalog:
//! ```text
//! trailer
//! << /Size 6 /Root 1 0 R /Info 4 0 R >>
//! ```
//!
//! The Root/Catalog object contains document-level metadata:
//! ```text
//! 1 0 obj
//! <<
//!   /Type /Catalog
//!   /Pages 2 0 R
//!   /Lang (en-US)
//!   /PageLayout /SinglePage
//!   /PageMode /UseOutlines
//! >>
//! endobj
//! ```

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use nom::{
    IResult,
    bytes::complete::{tag, take_until, take_while1},
    character::complete::multispace0,
    combinator::map_res,
    sequence::preceded,
};
use std::collections::HashMap;
use std::str;

//
// ═══════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts PDF Root/Catalog dictionary metadata from a PDF file.
///
/// This function:
/// 1. Locates the trailer dictionary at the end of the file
/// 2. Extracts the /Root reference
/// 3. Finds the cross-reference table to locate the Root object
/// 4. Parses the Root dictionary and extracts metadata fields
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
/// # Supported Root Dictionary Fields
///
/// - Language (from /Lang key)
/// - PageLayout (SinglePage, OneColumn, TwoColumnLeft, TwoColumnRight, TwoPageLeft, TwoPageRight)
/// - PageMode (UseNone, UseOutlines, UseThumbs, FullScreen, UseOC, UseAttachments)
/// - JavaScript (Yes/No - indicates if document contains JavaScript)
/// - Outlines (Yes/No - indicates if document has bookmarks)
/// - Names (Yes/No - indicates if document has named destinations)
pub fn parse_root_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    // Load PDF navigation context (xref table and trailer)
    let context = PdfContext::load(reader)?;

    // Find and parse the Root dictionary
    let root_ref = find_dict_reference(&context.xref_data, "/Root")?;
    let root_offset = context.get_object_offset(root_ref.object_num, "Root")?;
    let root_data = reader.read(
        root_offset,
        std::cmp::min(8192, reader.size().saturating_sub(root_offset) as usize),
    )?;
    let root_dict = parse_root_object(root_data)?;

    // Convert dictionary to metadata map with basic fields
    let mut metadata = convert_root_dict_to_metadata(root_dict);

    // Detect JavaScript presence (security indicator)
    let has_javascript = detect_javascript(root_data, reader, &context);
    metadata.insert(
        "PDF:JavaScript".to_string(),
        TagValue::new_string(if has_javascript { "Yes" } else { "No" }),
    );

    // Detect Outlines/Bookmarks
    let has_outlines = detect_outlines(root_data);
    metadata.insert(
        "PDF:Outlines".to_string(),
        TagValue::new_string(if has_outlines { "Yes" } else { "No" }),
    );

    // Detect Named Destinations
    let has_names = detect_names(root_data);
    metadata.insert(
        "PDF:Names".to_string(),
        TagValue::new_string(if has_names { "Yes" } else { "No" }),
    );

    Ok(metadata)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Navigation Context
// ═══════════════════════════════════════════════════════════════════════════
//

/// Encapsulates the PDF cross-reference table and trailer data needed for
/// navigating the PDF object structure.
struct PdfContext {
    xref_data: Vec<u8>,
    xref_map: HashMap<u32, u64>,
}

impl PdfContext {
    /// Loads the PDF context by reading the trailer and xref table from the file.
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

    /// Gets the file offset for a given object number.
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

/// Converts a raw Root dictionary to a MetadataMap.
fn convert_root_dict_to_metadata(root_dict: HashMap<String, String>) -> MetadataMap {
    let mut metadata = MetadataMap::with_capacity(3);

    for (key, value) in root_dict {
        match key.as_str() {
            "Lang" => {
                metadata.insert("PDF:Language".to_string(), TagValue::new_string(value));
            }
            "PageLayout" => {
                metadata.insert("PDF:PageLayout".to_string(), TagValue::new_string(value));
            }
            "PageMode" => {
                metadata.insert("PDF:PageMode".to_string(), TagValue::new_string(value));
            }
            _ => {}
        }
    }

    metadata
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Reference Finding
// ═══════════════════════════════════════════════════════════════════════════
//

/// Object reference structure (e.g., "4 0 R" means object 4, generation 0)
#[derive(Debug, Clone, Copy)]
struct ObjectRef {
    object_num: u32,
    #[allow(dead_code)]
    generation: u16,
}

/// Finds a dictionary reference by key name.
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
            Err(_) => break,
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

/// Parses all entries in a single xref subsection.
fn parse_xref_subsection_entries<'a>(
    mut input: &'a [u8],
    start_num: u64,
    count: u64,
    xref_map: &mut HashMap<u32, u64>,
) -> &'a [u8] {
    for i in 0..count {
        let obj_num = match (start_num as u32).checked_add(i as u32) {
            Some(num) => num,
            None => {
                input = skip_to_next_line(input);
                continue;
            }
        };

        match parse_xref_entry(input) {
            Ok((new_input, Some(offset))) => {
                xref_map.insert(obj_num, offset);
                input = new_input;
            }
            Ok((new_input, None)) => {
                input = new_input;
            }
            Err(_) => {
                input = skip_to_next_line(input);
            }
        }
    }
    input
}

/// Parses a single xref entry: "offset generation n/f"
fn parse_xref_entry(input: &[u8]) -> IResult<&[u8], Option<u64>> {
    use nom::bytes::complete::take_while1;

    let (input, offset) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _generation) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, in_use) = take_while1(|c| c == b'n' || c == b'f')(input)?;
    let (input, _) = multispace0(input)?;

    let result = if in_use[0] == b'n' {
        Some(offset)
    } else {
        None
    };

    Ok((input, result))
}

/// Skips to the next line in the input
fn skip_to_next_line(input: &[u8]) -> &[u8] {
    use nom::bytes::complete::take_until;
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
// Root Dictionary Parsing
// ═══════════════════════════════════════════════════════════════════════════
//

/// Parses the Root object and extracts the dictionary
fn parse_root_object(input: &[u8]) -> Result<HashMap<String, String>> {
    let input_str = str::from_utf8(input)
        .map_err(|_| ExifToolError::parse_error("Root object contains invalid UTF-8"))?;

    let dict_start = input_str
        .find("<<")
        .ok_or_else(|| ExifToolError::parse_error("Root dictionary start << not found"))?;

    let dict_end = input_str[dict_start..]
        .find(">>")
        .ok_or_else(|| ExifToolError::parse_error("Root dictionary end >> not found"))?;

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
        .map_err(|_| ExifToolError::parse_error("Failed to parse Root dictionary entries"))?;

    Ok(entries)
}

/// Parses dictionary entries (key-value pairs)
fn parse_dict_entries(input: &[u8]) -> IResult<&[u8], HashMap<String, String>> {
    use nom::Parser;
    use nom::multi::many0;

    let (input, pairs) = many0(parse_dict_entry).parse(input)?;
    let dict = pairs.into_iter().collect();
    Ok((input, dict))
}

/// Parses a single dictionary entry: /Key value
fn parse_dict_entry(input: &[u8]) -> IResult<&[u8], (String, String)> {
    let (input, _) = multispace0(input)?;

    // Parse key: /KeyName
    let (input, _) = tag(&b"/"[..])(input)?;
    let (input, key) = take_while1(|c: u8| c.is_ascii_alphanumeric())(input)?;
    let key = str::from_utf8(key)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    let (input, _) = multispace0(input)?;

    // Parse value
    let (input, value) = parse_dict_value(input)?;

    Ok((input, (key.to_string(), value)))
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Value Parsing
// ═══════════════════════════════════════════════════════════════════════════
//

/// Parses a dictionary value (handles string literals, names)
fn parse_dict_value(input: &[u8]) -> IResult<&[u8], String> {
    // Try each value type parser
    parse_string_literal(input)
        .or_else(|_| parse_name_value(input))
        .or_else(|_| {
            use nom::bytes::complete::take_while;
            // Skip to next key or end
            let (input, _) = take_while(|c| c != b'/')(input)?;
            Ok((input, String::new()))
        })
}

/// Parses a PDF string literal: (text)
fn parse_string_literal(input: &[u8]) -> IResult<&[u8], String> {
    let (input, _) = tag(&b"("[..])(input)?;

    let (content, close_pos) = extract_parenthesized_content(input)?;
    let text = String::from_utf8_lossy(&content).to_string();

    Ok((&input[close_pos + 1..], text))
}

/// Extracts content from a parenthesized string.
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
// Feature Detection Functions
// ═══════════════════════════════════════════════════════════════════════════
//

/// Detects if the PDF document contains JavaScript.
///
/// JavaScript in PDFs can be found in several locations:
/// 1. /Names -> /JavaScript dictionary in the Root
/// 2. /AA (Additional Actions) with /JS entries
/// 3. /OpenAction with JavaScript actions
fn detect_javascript(root_data: &[u8], reader: &dyn FileReader, context: &PdfContext) -> bool {
    let root_str = String::from_utf8_lossy(root_data);

    // Check for /Names dictionary with /JavaScript
    if root_str.contains("/Names") && root_str.contains("/JavaScript") {
        return true;
    }

    // Check for /AA (Additional Actions) with /JS
    if root_str.contains("/AA") {
        // Look for /JS key which indicates JavaScript action
        if root_str.contains("/JS") {
            return true;
        }
    }

    // Check for /OpenAction with JavaScript
    if root_str.contains("/OpenAction") {
        // Try to follow the OpenAction reference if it exists
        if let Ok(action_ref) = find_dict_reference(root_data, "/OpenAction")
            && let Ok(offset) = context.get_object_offset(action_ref.object_num, "OpenAction")
            && let Ok(action_data) = reader.read(
                offset,
                std::cmp::min(1024, reader.size().saturating_sub(offset) as usize),
            )
        {
            let action_str = String::from_utf8_lossy(action_data);
            if action_str.contains("/JS") || action_str.contains("/JavaScript") {
                return true;
            }
        }
    }

    // Search for /JavaScript keyword anywhere in root data
    root_str.contains("/JavaScript")
}

/// Detects if the PDF document has Outlines (bookmarks).
///
/// Outlines are referenced from the Root/Catalog dictionary via /Outlines key.
fn detect_outlines(root_data: &[u8]) -> bool {
    // Look for /Outlines key in Root dictionary
    root_data
        .windows(b"/Outlines".len())
        .any(|window| window == b"/Outlines")
}

/// Detects if the PDF document has Named Destinations.
///
/// Named destinations are stored in the /Names dictionary, specifically
/// in the /Dests subdictionary.
fn detect_names(root_data: &[u8]) -> bool {
    let root_str = String::from_utf8_lossy(root_data);

    // Check for /Names dictionary
    if root_str.contains("/Names") {
        // More specifically, check for /Dests which contains named destinations
        if root_str.contains("/Dests") {
            return true;
        }
        // Names dictionary exists even without Dests
        return true;
    }

    false
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
    use nom::character::complete::digit1;

    preceded(
        multispace0,
        map_res(map_res(digit1, str::from_utf8), |s: &str| s.parse::<u64>()),
    )
    .parse(input)
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

    /// Creates a minimal valid PDF with Root dictionary
    fn create_test_pdf_with_root() -> Vec<u8> {
        // Use the same structure as info_parser tests which are known to work
        let pdf = b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /Lang (en-US) /PageLayout /SinglePage /PageMode /UseOutlines >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>
endobj
xref
0 4
0000000000 65535 f
0000000009 00000 n
0000000113 00000 n
0000000172 00000 n
trailer
<< /Size 4 /Root 1 0 R >>
startxref
243
%%EOF";

        pdf.to_vec()
    }

    #[test]
    fn test_parse_root_metadata() {
        let pdf_data = create_test_pdf_with_root();
        let reader = TestReader::new(pdf_data);

        let result = parse_root_metadata(&reader);
        assert!(result.is_ok(), "Failed to parse Root: {:?}", result.err());

        let metadata = result.unwrap();

        assert_eq!(metadata.get_string("PDF:Language"), Some("en-US"));
        assert_eq!(metadata.get_string("PDF:PageLayout"), Some("SinglePage"));
        assert_eq!(metadata.get_string("PDF:PageMode"), Some("UseOutlines"));
    }

    #[test]
    fn test_parse_name_value() {
        let input = b"/SinglePage";
        let result = parse_name_value(input);
        assert!(result.is_ok());
        let (_, value) = result.unwrap();
        assert_eq!(value, "SinglePage");
    }

    #[test]
    fn test_parse_string_literal() {
        let input = b"(en-US)";
        let result = parse_string_literal(input);
        assert!(result.is_ok());
        let (_, value) = result.unwrap();
        assert_eq!(value, "en-US");
    }

    #[test]
    fn test_parse_dict_entry() {
        let input = b"/Lang (en-US)";
        let result = parse_dict_entry(input);
        assert!(result.is_ok());
        let (_, (key, value)) = result.unwrap();
        assert_eq!(key, "Lang");
        assert_eq!(value, "en-US");
    }

    #[test]
    fn test_detect_javascript_with_names() {
        let pdf = create_pdf_with_javascript();
        let reader = TestReader::new(pdf);
        let result = parse_root_metadata(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:JavaScript"), Some("Yes"));
    }

    #[test]
    fn test_detect_javascript_none() {
        let pdf = create_test_pdf_with_root();
        let reader = TestReader::new(pdf);
        let result = parse_root_metadata(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:JavaScript"), Some("No"));
    }

    #[test]
    fn test_detect_outlines() {
        let pdf = create_pdf_with_outlines();
        let reader = TestReader::new(pdf);
        let result = parse_root_metadata(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:Outlines"), Some("Yes"));
    }

    #[test]
    fn test_detect_outlines_none() {
        let pdf = create_test_pdf_with_root();
        let reader = TestReader::new(pdf);
        let result = parse_root_metadata(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:Outlines"), Some("No"));
    }

    #[test]
    fn test_detect_names() {
        let pdf = create_pdf_with_names();
        let reader = TestReader::new(pdf);
        let result = parse_root_metadata(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:Names"), Some("Yes"));
    }

    #[test]
    fn test_detect_names_none() {
        let pdf = create_test_pdf_with_root();
        let reader = TestReader::new(pdf);
        let result = parse_root_metadata(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:Names"), Some("No"));
    }

    /// Creates a PDF with JavaScript
    fn create_pdf_with_javascript() -> Vec<u8> {
        let pdf = b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /Names << /JavaScript 5 0 R >> >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>
endobj
5 0 obj
<< /Names [(MyScript) 6 0 R] >>
endobj
6 0 obj
<< /S /JavaScript /JS (app.alert('Hello');) >>
endobj
xref
0 7
0000000000 65535 f
0000000009 00000 n
0000000086 00000 n
0000000145 00000 n
0000000000 65535 f
0000000216 00000 n
0000000258 00000 n
trailer
<< /Size 7 /Root 1 0 R >>
startxref
320
%%EOF";
        pdf.to_vec()
    }

    /// Creates a PDF with Outlines (bookmarks)
    fn create_pdf_with_outlines() -> Vec<u8> {
        let pdf = b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>
endobj
5 0 obj
<< /Type /Outlines /Count 0 >>
endobj
xref
0 6
0000000000 65535 f
0000000009 00000 n
0000000074 00000 n
0000000133 00000 n
0000000000 65535 f
0000000204 00000 n
trailer
<< /Size 6 /Root 1 0 R >>
startxref
246
%%EOF";
        pdf.to_vec()
    }

    /// Creates a PDF with Names dictionary
    fn create_pdf_with_names() -> Vec<u8> {
        let pdf = b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /Names << /Dests 5 0 R >> >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>
endobj
5 0 obj
<< /Names [(Page1) [3 0 R /XYZ 0 792 0]] >>
endobj
xref
0 6
0000000000 65535 f
0000000009 00000 n
0000000080 00000 n
0000000139 00000 n
0000000000 65535 f
0000000210 00000 n
trailer
<< /Size 6 /Root 1 0 R >>
startxref
270
%%EOF";
        pdf.to_vec()
    }
}
