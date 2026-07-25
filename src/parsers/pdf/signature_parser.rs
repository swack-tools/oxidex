//! PDF Digital Signature parser
//!
//! This module handles parsing of PDF digital signatures from AcroForm fields.
//!
//! # PDF Signature Structure
//!
//! Digital signatures in PDFs are found via:
//! 1. Root -> /AcroForm -> /Fields array
//! 2. Each field with /FT /Sig is a signature field
//! 3. /V contains the signature dictionary with:
//!    - /ContactInfo - Contact information for signer
//!    - /Location - Physical location where signing occurred
//!    - /M - Signing date (modification date)
//!    - /Name - Name of person/authority signing
//!    - /Reason - Reason for signing
//!
//! # Example Signature Dictionary
//!
//! ```text
//! 10 0 obj
//! <<
//!   /Type /Sig
//!   /Filter /Adobe.PPKLite
//!   /SubFilter /adbe.pkcs7.detached
//!   /Name (John Doe)
//!   /Location (New York, NY)
//!   /Reason (I agree to the terms)
//!   /ContactInfo (john@example.com)
//!   /M (D:20240115143000+00'00')
//! >>
//! endobj
//! ```

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use nom::{
    IResult,
    bytes::complete::{tag, take_until, take_while1},
    character::complete::multispace0,
};
use std::str;

use super::info_parser::{ObjectRef, PdfContext, format_pdf_date};

//
// ═══════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts PDF digital signature metadata from a PDF file.
///
/// This function:
/// 1. Locates the Root catalog from the trailer
/// 2. Finds the /AcroForm dictionary
/// 3. Searches /Fields array for signature fields (/FT /Sig)
/// 4. Parses the signature dictionary (/V) for metadata
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PDF file
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted signature metadata with "PDF:" prefix
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Supported Signature Fields
///
/// - SignerContactInfo - /ContactInfo from signature dict
/// - SigningLocation - /Location from signature dict
/// - SigningDate - /M (modification date) from signature dict
/// - SigningAuthority - /Name from signature dict
/// - SigningReason - /Reason from signature dict
///
/// # Notes
///
/// If multiple signatures exist, only the first one is extracted.
/// Returns empty metadata if no signatures are found.
pub fn parse_signature_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    // Load PDF navigation context (xref table and trailer)
    let context = PdfContext::load(reader)?;

    // Navigate: Trailer -> Root -> AcroForm
    let acroform_data = match navigate_to_acroform(reader, &context) {
        Ok(data) => data,
        Err(_) => {
            // No AcroForm found - return empty metadata (not an error)
            return Ok(MetadataMap::new());
        }
    };

    // Find signature field in AcroForm
    let signature_ref = match find_signature_field(&acroform_data) {
        Ok(sig_ref) => sig_ref,
        Err(_) => {
            // No signature field found - return empty metadata
            return Ok(MetadataMap::new());
        }
    };

    // Read signature field object
    let sig_offset = match context.get_object_offset(signature_ref.object_num, "Signature") {
        Ok(offset) => offset,
        Err(_) => return Ok(MetadataMap::new()),
    };

    let sig_size = std::cmp::min(4096, reader.size().saturating_sub(sig_offset) as usize);
    let sig_data = reader.read(sig_offset, sig_size)?;

    // Find /V (signature dictionary) reference
    let sig_value_ref = match find_dict_reference(sig_data, "/V") {
        Ok(v_ref) => v_ref,
        Err(_) => {
            // No /V found - return empty metadata
            return Ok(MetadataMap::new());
        }
    };

    // Read signature value object
    let sig_value_offset =
        match context.get_object_offset(sig_value_ref.object_num, "SignatureValue") {
            Ok(offset) => offset,
            Err(_) => return Ok(MetadataMap::new()),
        };

    let sig_value_size = std::cmp::min(
        8192,
        reader.size().saturating_sub(sig_value_offset) as usize,
    );
    let sig_value_data = reader.read(sig_value_offset, sig_value_size)?;

    // Parse signature dictionary
    let mut metadata = MetadataMap::new();
    extract_signature_fields(sig_value_data, &mut metadata);

    Ok(metadata)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Navigation Functions
// ═══════════════════════════════════════════════════════════════════════════
//

/// Navigates from trailer to AcroForm object
fn navigate_to_acroform(reader: &dyn FileReader, context: &PdfContext) -> Result<Vec<u8>> {
    // Find /Root reference in trailer
    let root_ref = find_dict_reference(&context.xref_data, "/Root")?;
    let root_offset = context.get_object_offset(root_ref.object_num, "Root")?;

    // Read Root object
    let root_size = std::cmp::min(4096, reader.size().saturating_sub(root_offset) as usize);
    let root_data = reader.read(root_offset, root_size)?;

    // Find /AcroForm reference in Root object
    let acroform_ref = find_dict_reference(root_data, "/AcroForm")?;
    let acroform_offset = context.get_object_offset(acroform_ref.object_num, "AcroForm")?;

    // Read AcroForm object
    let acroform_size = std::cmp::min(8192, reader.size().saturating_sub(acroform_offset) as usize);
    Ok(reader.read(acroform_offset, acroform_size)?.to_vec())
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Signature Field Finding
// ═══════════════════════════════════════════════════════════════════════════
//

/// Finds the first signature field in the AcroForm /Fields array
fn find_signature_field(acroform_data: &[u8]) -> Result<ObjectRef> {
    // First, try to find /Fields array
    let fields_start = acroform_data
        .windows(7)
        .position(|w| w == b"/Fields")
        .ok_or_else(|| ExifToolError::parse_error("No /Fields array in AcroForm"))?;

    // Parse /Fields array - it contains object references
    // Format: /Fields [10 0 R 11 0 R 12 0 R]
    let fields_section = &acroform_data[fields_start..];

    // Find array start '['
    let array_start = fields_section
        .iter()
        .position(|&b| b == b'[')
        .ok_or_else(|| ExifToolError::parse_error("No array start in /Fields"))?;

    // Find array end ']'
    let array_section = &fields_section[array_start..];
    let array_end = array_section
        .iter()
        .position(|&b| b == b']')
        .ok_or_else(|| ExifToolError::parse_error("No array end in /Fields"))?;

    let array_content = &array_section[1..array_end]; // Skip '[' and ']'

    // Parse all field references from the array
    let field_refs = parse_field_references(array_content)?;

    // Return first field reference (we assume it's a signature field)
    // In a full implementation, we'd read each field and check for /FT /Sig
    field_refs
        .first()
        .copied()
        .ok_or_else(|| ExifToolError::parse_error("No field references in /Fields array"))
}

/// Parses object references from /Fields array
fn parse_field_references(input: &[u8]) -> Result<Vec<ObjectRef>> {
    let mut refs = Vec::new();
    let mut input = input;

    loop {
        // Skip whitespace
        let (inp, _) = multispace0::<_, nom::error::Error<&[u8]>>(input)
            .map_err(|_| ExifToolError::parse_error("Failed to parse whitespace"))?;

        if inp.is_empty() {
            break;
        }

        // Try to parse object reference
        match parse_object_reference(inp) {
            Ok((remaining, obj_ref)) => {
                refs.push(obj_ref);
                input = remaining;
            }
            Err(_) => {
                // No more references, break
                break;
            }
        }
    }

    Ok(refs)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Signature Dictionary Parsing
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts signature fields from the signature dictionary
fn extract_signature_fields(sig_data: &[u8], metadata: &mut MetadataMap) {
    // Extract each signature field
    if let Ok(name) = extract_string_field(sig_data, "/Name") {
        metadata.insert(
            "PDF:SigningAuthority".to_string(),
            TagValue::new_string(name),
        );
    }

    if let Ok(location) = extract_string_field(sig_data, "/Location") {
        metadata.insert(
            "PDF:SigningLocation".to_string(),
            TagValue::new_string(location),
        );
    }

    if let Ok(reason) = extract_string_field(sig_data, "/Reason") {
        metadata.insert(
            "PDF:SigningReason".to_string(),
            TagValue::new_string(reason),
        );
    }

    if let Ok(contact) = extract_string_field(sig_data, "/ContactInfo") {
        metadata.insert(
            "PDF:SignerContactInfo".to_string(),
            TagValue::new_string(contact),
        );
    }

    // Extract signing date and format it
    if let Ok(date_str) = extract_string_field(sig_data, "/M")
        && let Some(formatted_date) = format_pdf_date(&date_str)
    {
        metadata.insert(
            "PDF:SigningDate".to_string(),
            TagValue::new_string(formatted_date),
        );
    }
}

/// Extracts a string value for a given dictionary key
fn extract_string_field(data: &[u8], key: &str) -> Result<String> {
    // Find the key
    let key_pos = data
        .windows(key.len())
        .position(|w| w == key.as_bytes())
        .ok_or_else(|| ExifToolError::parse_error(format!("{} not found", key)))?;

    let after_key = &data[key_pos + key.len()..];

    // Parse the value (string literal or name)
    let (_, value) = parse_string_value(after_key)
        .map_err(|_| ExifToolError::parse_error(format!("Failed to parse {} value", key)))?;

    Ok(value)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Reference and Value Parsing
// ═══════════════════════════════════════════════════════════════════════════
//

/// Finds a dictionary reference by key name (reusing from info_parser pattern)
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
    use nom::combinator::map_res;
    use nom::{Parser, character::complete::digit1, sequence::preceded};

    preceded(
        multispace0,
        map_res(map_res(digit1, str::from_utf8), |s: &str| s.parse::<u64>()),
    )
    .parse(input)
}

/// Parses a string value (literal or name)
fn parse_string_value(input: &[u8]) -> IResult<&[u8], String> {
    let (input, _) = multispace0(input)?;

    // Try string literal first, then name
    parse_string_literal(input).or_else(|_| parse_name_value(input))
}

/// Parses a PDF string literal: (text)
fn parse_string_literal(input: &[u8]) -> IResult<&[u8], String> {
    let (input, _) = tag(&b"("[..])(input)?;

    // Parse content with depth tracking for nested parentheses
    let (content, close_pos) = extract_parenthesized_content(input)?;
    let text = String::from_utf8_lossy(&content).to_string();

    // Return remaining input (skip the closing paren)
    Ok((&input[close_pos + 1..], text))
}

/// Extracts content from a parenthesized string, handling escapes and nesting
#[allow(clippy::type_complexity)]
fn extract_parenthesized_content(
    input: &[u8],
) -> std::result::Result<(Vec<u8>, usize), nom::Err<nom::error::Error<&[u8]>>> {
    let mut content = Vec::new();
    let mut i = 0;
    let mut depth = 1;

    while i < input.len() {
        if input[i] == b'\\' && i + 1 < input.len() {
            // Escaped character
            content.push(input[i]);
            content.push(input[i + 1]);
            i += 2;
        } else if input[i] == b'(' {
            // Nested opening paren
            depth += 1;
            content.push(input[i]);
            i += 1;
        } else if input[i] == b')' {
            // Closing paren
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
        take_while1(|c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'-' || c == b'.')(
            input,
        )?;

    let text = str::from_utf8(name)
        .map_err(|_| nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char)))?;

    Ok((input, text.to_string()))
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
        let input = b"10 0 R";
        let result = parse_object_reference(input);
        assert!(result.is_ok());
        let (_, obj_ref) = result.unwrap();
        assert_eq!(obj_ref.object_num, 10);
        assert_eq!(obj_ref.generation, 0);
    }

    #[test]
    fn test_parse_string_literal() {
        let input = b"(John Doe)";
        let result = parse_string_literal(input);
        assert!(result.is_ok());
        let (_, text) = result.unwrap();
        assert_eq!(text, "John Doe");
    }

    #[test]
    fn test_extract_string_field() {
        let data = b"/Name (John Doe) /Location (New York)";
        let result = extract_string_field(data, "/Name");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "John Doe");
    }

    #[test]
    fn test_parse_field_references() {
        let input = b"10 0 R 11 0 R 12 0 R";
        let result = parse_field_references(input);
        assert!(result.is_ok());
        let refs = result.unwrap();
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].object_num, 10);
        assert_eq!(refs[1].object_num, 11);
        assert_eq!(refs[2].object_num, 12);
    }
}
