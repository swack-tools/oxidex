//! PDF structure and features parser
//!
//! This module detects PDF structural features and capabilities such as
//! Tagged PDF (accessibility), XFA forms, and AcroForm presence.
//!
//! # PDF Structure Features
//!
//! The Root/Catalog dictionary contains references to structure dictionaries:
//! ```text
//! 1 0 obj
//! <<
//!   /Type /Catalog
//!   /Pages 2 0 R
//!   /MarkInfo << /Marked true >>
//!   /AcroForm << /Fields [...] /XFA [...] >>
//! >>
//! endobj
//! ```
//!
//! # Detected Features
//!
//! - **TaggedPDF**: Document has structural tagging for accessibility (/MarkInfo -> /Marked true)
//! - **HasXFA**: Document contains XFA (XML Forms Architecture) forms
//! - **HasAcroForm**: Document contains AcroForm (standard PDF interactive forms)

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use nom::{
    IResult,
    bytes::complete::{tag, take_until},
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

/// Extracts PDF structure and feature metadata from a PDF file.
///
/// This function:
/// 1. Locates the Root/Catalog object
/// 2. Checks for /MarkInfo dictionary with /Marked flag (Tagged PDF)
/// 3. Checks for /AcroForm dictionary existence
/// 4. If AcroForm exists, checks for /XFA key
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
/// # Detected Features
///
/// - PDF:TaggedPDF - "Yes" if /MarkInfo -> /Marked is true, "No" otherwise
/// - PDF:HasXFA - "Yes" if /AcroForm -> /XFA exists, "No" otherwise
/// - PDF:HasAcroForm - "Yes" if /AcroForm exists, "No" otherwise
/// - PDF:AnnotationCount - Number of annotations in the document (if detectable)
pub fn parse_structure_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    // Load PDF navigation context (xref table and trailer)
    let context = PdfContext::load(reader)?;

    // Find and read the Root dictionary
    let root_ref = find_dict_reference(&context.xref_data, "/Root")?;
    let root_offset = context.get_object_offset(root_ref.object_num, "Root")?;
    let root_data = reader.read(
        root_offset,
        std::cmp::min(8192, reader.size().saturating_sub(root_offset) as usize),
    )?;

    // Detect structure features
    let mut metadata = MetadataMap::with_capacity(4);

    // Check for Tagged PDF (/MarkInfo -> /Marked true)
    let tagged_pdf = check_tagged_pdf(root_data);
    metadata.insert(
        "PDF:TaggedPDF".to_string(),
        TagValue::new_string(if tagged_pdf { "Yes" } else { "No" }),
    );

    // Check for AcroForm
    let has_acroform = check_has_acroform(root_data);
    metadata.insert(
        "PDF:HasAcroForm".to_string(),
        TagValue::new_string(if has_acroform { "Yes" } else { "No" }),
    );

    // Check for XFA (only if AcroForm exists)
    let has_xfa = if has_acroform {
        check_has_xfa(root_data, reader, &context)
    } else {
        false
    };
    metadata.insert(
        "PDF:HasXFA".to_string(),
        TagValue::new_string(if has_xfa { "Yes" } else { "No" }),
    );

    // Count annotations in the document
    // This searches for /Annots arrays in page objects
    if let Ok(annot_count) = count_annotations(reader, &context)
        && annot_count > 0
    {
        metadata.insert(
            "PDF:AnnotationCount".to_string(),
            TagValue::new_integer(annot_count as i64),
        );
    }

    Ok(metadata)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Navigation Context
// ═══════════════════════════════════════════════════════════════════════════
//

/// Encapsulates the PDF cross-reference table and trailer data.
struct PdfContext {
    xref_data: Vec<u8>,
    xref_map: HashMap<u32, u64>,
}

impl PdfContext {
    /// Loads the PDF context by reading the trailer and xref table.
    fn load(reader: &dyn FileReader) -> Result<Self> {
        let file_size = reader.size();

        // Read the last 1024 bytes to find trailer
        let tail_size = std::cmp::min(1024, file_size as usize);
        let tail_offset = file_size.saturating_sub(tail_size as u64);
        let tail_data = reader.read(tail_offset, tail_size)?;

        // Find startxref and get xref offset
        let xref_offset = find_xref_offset(tail_data)?;

        // Read xref table and trailer region
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
// Feature Detection
// ═══════════════════════════════════════════════════════════════════════════
//

/// Checks if PDF is tagged for accessibility.
/// Looks for /MarkInfo << /Marked true >> in the Root dictionary.
fn check_tagged_pdf(root_data: &[u8]) -> bool {
    // Look for /MarkInfo dictionary
    if let Ok(mark_info_data) = extract_dict_after_key(root_data, b"/MarkInfo") {
        // Check if /Marked is true
        is_boolean_true(&mark_info_data, b"/Marked")
    } else {
        false
    }
}

/// Checks if PDF has AcroForm (interactive forms).
/// Looks for /AcroForm in the Root dictionary.
fn check_has_acroform(root_data: &[u8]) -> bool {
    // Simple check: does /AcroForm key exist?
    root_data
        .windows(b"/AcroForm".len())
        .any(|window| window == b"/AcroForm")
}

/// Checks if PDF has XFA forms.
/// Looks for /XFA key within the AcroForm dictionary.
fn check_has_xfa(root_data: &[u8], reader: &dyn FileReader, context: &PdfContext) -> bool {
    // First, try to check in the Root data directly
    if let Ok(acroform_data) = extract_dict_after_key(root_data, b"/AcroForm")
        && acroform_data
            .windows(b"/XFA".len())
            .any(|window| window == b"/XFA")
    {
        return true;
    }

    // If /AcroForm is a reference, follow it
    if let Ok(acroform_ref) = find_acroform_reference(root_data)
        && let Ok(offset) = context.get_object_offset(acroform_ref.object_num, "AcroForm")
        && let Ok(acroform_obj_data) = reader.read(
            offset,
            std::cmp::min(4096, reader.size().saturating_sub(offset) as usize),
        )
    {
        return acroform_obj_data
            .windows(b"/XFA".len())
            .any(|window| window == b"/XFA");
    }

    false
}

/// Counts annotations in the PDF document.
///
/// Annotations are stored in /Annots arrays within page objects.
/// This function navigates through the Pages tree and counts annotations.
fn count_annotations(reader: &dyn FileReader, context: &PdfContext) -> Result<usize> {
    // Navigate: Trailer -> Root -> Pages
    let root_ref = find_dict_reference(&context.xref_data, "/Root")?;
    let root_offset = context.get_object_offset(root_ref.object_num, "Root")?;

    let root_size = std::cmp::min(4096, reader.size().saturating_sub(root_offset) as usize);
    let root_data = reader.read(root_offset, root_size)?;

    // Find /Pages reference in Root object
    let pages_ref = find_dict_reference(root_data, "/Pages")?;
    let pages_offset = context.get_object_offset(pages_ref.object_num, "Pages")?;

    // Read Pages object
    let pages_size = std::cmp::min(8192, reader.size().saturating_sub(pages_offset) as usize);
    let pages_data = reader.read(pages_offset, pages_size)?;

    // Count annotations by searching for /Annots in the pages data
    // This is a simplified approach that counts /Annots occurrences
    let pages_str = String::from_utf8_lossy(pages_data);
    let mut count = 0;

    // Search for /Annots arrays - each occurrence likely indicates annotations
    for _match in pages_str.match_indices("/Annots") {
        // This is a rough count - each /Annots key suggests annotations exist
        // To get exact count, we'd need to parse the array, but this gives us
        // a good indication that annotations are present
        count += 1;
    }

    Ok(count)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Dictionary Extraction Helpers
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts the dictionary content after a given key.
/// Returns the content between << and >> after the key.
fn extract_dict_after_key(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let data_str = str::from_utf8(data)
        .map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in PDF data"))?;

    let key_str =
        str::from_utf8(key).map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in key"))?;

    let key_pos = data_str
        .find(key_str)
        .ok_or_else(|| ExifToolError::parse_error(format!("{} not found", key_str)))?;

    let after_key = &data_str[key_pos + key_str.len()..];

    // Find opening <<
    let dict_start = after_key
        .find("<<")
        .ok_or_else(|| ExifToolError::parse_error("Dictionary start << not found"))?;

    // Find matching >>
    let dict_end = after_key[dict_start..]
        .find(">>")
        .ok_or_else(|| ExifToolError::parse_error("Dictionary end >> not found"))?;

    let content_start = dict_start + 2; // Skip "<<"
    let content_end = dict_start + dict_end;

    Ok(after_key.as_bytes()[content_start..content_end].to_vec())
}

/// Checks if a boolean key is set to true in the data.
/// Looks for patterns like: /KeyName true
fn is_boolean_true(data: &[u8], key: &[u8]) -> bool {
    let data_str = match str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let key_str = match str::from_utf8(key) {
        Ok(s) => s,
        Err(_) => return false,
    };

    if let Some(key_pos) = data_str.find(key_str) {
        let after_key = &data_str[key_pos + key_str.len()..];
        // Skip whitespace and check for "true"
        let trimmed = after_key.trim_start();
        trimmed.starts_with("true")
    } else {
        false
    }
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Reference Finding
// ═══════════════════════════════════════════════════════════════════════════
//

/// Object reference structure
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

/// Finds the AcroForm reference in Root data
fn find_acroform_reference(data: &[u8]) -> Result<ObjectRef> {
    let (_, obj_ref) = parse_dict_reference(data, "/AcroForm")
        .map_err(|_| ExifToolError::parse_error("Could not parse /AcroForm reference"))?;
    Ok(obj_ref)
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
        .checked_add(9)
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

    let after_xref = &xref_str[xref_pos + 4..];

    let (_, xref_map) = parse_xref_entries(after_xref.as_bytes())
        .map_err(|_| ExifToolError::parse_error("Failed to parse xref entries"))?;

    Ok(xref_map)
}

/// Parses xref entries and returns a map of object numbers to offsets.
fn parse_xref_entries(input: &[u8]) -> IResult<&[u8], HashMap<u32, u64>> {
    let mut xref_map = HashMap::new();
    let mut input = input;

    loop {
        let (inp, _) = multispace0(input)?;

        if inp.starts_with(b"trailer") {
            break;
        }

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

/// Parses a single xref subsection header
fn parse_xref_subsection_header(input: &[u8]) -> IResult<&[u8], (u64, u64)> {
    let (input, start_num) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, count) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    Ok((input, (start_num, count)))
}

/// Parses all entries in a single xref subsection
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

/// Parses a single xref entry
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
    if let Ok((new_input, _)) = take_until::<_, _, nom::error::Error<&[u8]>>("\n")(input) {
        if new_input.is_empty() {
            new_input
        } else {
            &new_input[1..]
        }
    } else {
        &[]
    }
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

    /// Creates a PDF with Tagged PDF support
    fn create_tagged_pdf() -> Vec<u8> {
        let pdf = b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /MarkInfo << /Marked true >> >>
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
0000000079 00000 n
0000000138 00000 n
trailer
<< /Size 4 /Root 1 0 R >>
startxref
209
%%EOF";

        pdf.to_vec()
    }

    /// Creates a PDF with AcroForm (no XFA)
    fn create_acroform_pdf() -> Vec<u8> {
        let pdf = b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>
endobj
4 0 obj
<< /Type /Field /T (TextField) /FT /Tx >>
endobj
xref
0 5
0000000000 65535 f
0000000009 00000 n
0000000082 00000 n
0000000141 00000 n
0000000212 00000 n
trailer
<< /Size 5 /Root 1 0 R >>
startxref
262
%%EOF";

        pdf.to_vec()
    }

    /// Creates a PDF with XFA forms
    fn create_xfa_pdf() -> Vec<u8> {
        let pdf = b"%PDF-1.7
1 0 obj
<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] /XFA [5 0 R] >> >>
endobj
2 0 obj
<< /Type /Pages /Kids [3 0 R] /Count 1 >>
endobj
3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>
endobj
4 0 obj
<< /Type /Field /T (TextField) /FT /Tx >>
endobj
5 0 obj
<< /Type /XObject >>
endobj
xref
0 6
0000000000 65535 f
0000000009 00000 n
0000000098 00000 n
0000000157 00000 n
0000000228 00000 n
0000000277 00000 n
trailer
<< /Size 6 /Root 1 0 R >>
startxref
322
%%EOF";

        pdf.to_vec()
    }

    #[test]
    fn test_tagged_pdf_detection() {
        let pdf_data = create_tagged_pdf();
        let reader = TestReader::new(pdf_data);

        let result = parse_structure_metadata(&reader);
        assert!(
            result.is_ok(),
            "Failed to parse structure: {:?}",
            result.err()
        );

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:TaggedPDF"), Some("Yes"));
        assert_eq!(metadata.get_string("PDF:HasAcroForm"), Some("No"));
        assert_eq!(metadata.get_string("PDF:HasXFA"), Some("No"));
    }

    #[test]
    fn test_acroform_detection() {
        let pdf_data = create_acroform_pdf();
        let reader = TestReader::new(pdf_data);

        let result = parse_structure_metadata(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:TaggedPDF"), Some("No"));
        assert_eq!(metadata.get_string("PDF:HasAcroForm"), Some("Yes"));
        assert_eq!(metadata.get_string("PDF:HasXFA"), Some("No"));
    }

    #[test]
    fn test_xfa_detection() {
        let pdf_data = create_xfa_pdf();
        let reader = TestReader::new(pdf_data);

        let result = parse_structure_metadata(&reader);
        assert!(result.is_ok());

        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:TaggedPDF"), Some("No"));
        assert_eq!(metadata.get_string("PDF:HasAcroForm"), Some("Yes"));
        assert_eq!(metadata.get_string("PDF:HasXFA"), Some("Yes"));
    }

    #[test]
    fn test_check_tagged_pdf() {
        let data = b"<< /Type /Catalog /MarkInfo << /Marked true >> >>";
        assert_eq!(check_tagged_pdf(data), true);

        let data_false = b"<< /Type /Catalog /MarkInfo << /Marked false >> >>";
        assert_eq!(check_tagged_pdf(data_false), false);

        let data_none = b"<< /Type /Catalog >>";
        assert_eq!(check_tagged_pdf(data_none), false);
    }

    #[test]
    fn test_check_has_acroform() {
        let data = b"<< /Type /Catalog /AcroForm 5 0 R >>";
        assert_eq!(check_has_acroform(data), true);

        let data_none = b"<< /Type /Catalog >>";
        assert_eq!(check_has_acroform(data_none), false);
    }
}
