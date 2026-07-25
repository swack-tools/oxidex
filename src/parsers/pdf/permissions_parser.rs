//! PDF Permissions parser
//!
//! This module handles parsing of PDF document permissions from the /Perms dictionary.
//!
//! # PDF Permissions Structure
//!
//! Document permissions are found via:
//! 1. Root -> /Perms -> /DocMDP (Document Modification Detection & Prevention)
//! 2. Root -> /Perms -> /FieldMDP (Field Modification Detection & Prevention)
//! 3. Root -> /Perms -> /UR3 (Usage Rights version 3)
//!
//! # Permission Levels
//!
//! DocMDP and FieldMDP use numeric levels:
//! - 1 = No changes allowed (locked)
//! - 2 = Form filling, signing, and annotations allowed
//! - 3 = Form filling, signing, annotations, and page operations allowed
//!
//! # Example Perms Dictionary
//!
//! ```text
//! 1 0 obj
//! <<
//!   /Type /Catalog
//!   /Perms <<
//!     /DocMDP 15 0 R
//!     /UR3 16 0 R
//!   >>
//! >>
//! endobj
//!
//! 15 0 obj
//! <<
//!   /Type /TransformParams
//!   /P 2  % Permission level
//! >>
//! endobj
//! ```

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use nom::{
    IResult,
    bytes::complete::{tag, take_until},
    character::complete::multispace0,
};
use std::str;

use super::info_parser::{ObjectRef, PdfContext};

//
// ═══════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts PDF permissions metadata from a PDF file.
///
/// This function:
/// 1. Locates the Root catalog from the trailer
/// 2. Finds the /Perms dictionary
/// 3. Parses /DocMDP, /FieldMDP, and /UR3 entries
/// 4. Extracts permission levels
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PDF file
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted permissions metadata with "PDF:" prefix
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Supported Permission Fields
///
/// - DocMDP - Document modification detection level (1, 2, or 3)
/// - FieldMDP - Field modification detection level (1, 2, or 3)
/// - UR3 - Usage rights version 3 presence (Yes/No)
///
/// # Permission Levels
///
/// - 1 = No changes allowed
/// - 2 = Form filling, signing, annotations allowed
/// - 3 = Form filling, signing, annotations, page operations allowed
///
/// # Notes
///
/// Returns empty metadata if no /Perms dictionary is found.
pub fn parse_permissions_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    // Load PDF navigation context (xref table and trailer)
    let context = PdfContext::load(reader)?;

    // Navigate: Trailer -> Root -> Perms
    let perms_data = match navigate_to_perms(reader, &context) {
        Ok(data) => data,
        Err(_) => {
            // No /Perms found - return empty metadata (not an error)
            return Ok(MetadataMap::new());
        }
    };

    let mut metadata = MetadataMap::new();

    // Extract DocMDP
    if let Ok(docmdp_level) = extract_permission_level(reader, &context, &perms_data, "/DocMDP") {
        metadata.insert(
            "PDF:DocMDP".to_string(),
            TagValue::new_integer(docmdp_level),
        );
    }

    // Extract FieldMDP
    if let Ok(fieldmdp_level) = extract_permission_level(reader, &context, &perms_data, "/FieldMDP")
    {
        metadata.insert(
            "PDF:FieldMDP".to_string(),
            TagValue::new_integer(fieldmdp_level),
        );
    }

    // Extract UR3 (just check presence)
    if check_ur3_presence(&perms_data) {
        metadata.insert(
            "PDF:UR3".to_string(),
            TagValue::new_string("Yes".to_string()),
        );
    }

    Ok(metadata)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Navigation Functions
// ═══════════════════════════════════════════════════════════════════════════
//

/// Navigates from trailer to Perms object
fn navigate_to_perms(reader: &dyn FileReader, context: &PdfContext) -> Result<Vec<u8>> {
    // Find /Root reference in trailer
    let root_ref = find_dict_reference(&context.xref_data, "/Root")?;
    let root_offset = context.get_object_offset(root_ref.object_num, "Root")?;

    // Read Root object
    let root_size = std::cmp::min(8192, reader.size().saturating_sub(root_offset) as usize);
    let root_data = reader.read(root_offset, root_size)?;

    // Find /Perms reference in Root object
    let perms_ref = find_dict_reference(root_data, "/Perms")?;
    let perms_offset = context.get_object_offset(perms_ref.object_num, "Perms")?;

    // Read Perms object
    let perms_size = std::cmp::min(4096, reader.size().saturating_sub(perms_offset) as usize);
    Ok(reader.read(perms_offset, perms_size)?.to_vec())
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Permission Extraction
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts permission level for a given permission type (DocMDP or FieldMDP)
fn extract_permission_level(
    reader: &dyn FileReader,
    context: &PdfContext,
    perms_data: &[u8],
    perm_key: &str,
) -> Result<i64> {
    // Find permission reference in Perms dictionary
    let perm_ref = find_dict_reference(perms_data, perm_key)?;
    let perm_offset = context.get_object_offset(perm_ref.object_num, perm_key)?;

    // Read permission object (signature dictionary or transform params)
    let perm_size = std::cmp::min(4096, reader.size().saturating_sub(perm_offset) as usize);
    let perm_data = reader.read(perm_offset, perm_size)?;

    // Look for /P value (permission level) or /Reference array with TransformParams
    // Try direct /P first
    if let Ok(level) = extract_p_value(perm_data) {
        return Ok(level);
    }

    // Try /Reference array with TransformParams
    extract_p_from_reference(reader, context, perm_data)
}

/// Extracts /P value directly from the object
fn extract_p_value(data: &[u8]) -> Result<i64> {
    // Find /P key
    let p_pos = data
        .windows(2)
        .position(|w| w == b"/P")
        .ok_or_else(|| ExifToolError::parse_error("/P not found"))?;

    // Make sure this is actually /P and not part of another key like /Parent
    // Check that the character before is whitespace or '<' or '/'
    if p_pos > 0 {
        let prev_char = data[p_pos - 1];
        if prev_char != b' '
            && prev_char != b'\n'
            && prev_char != b'\r'
            && prev_char != b'\t'
            && prev_char != b'<'
            && prev_char != b'/'
        {
            return Err(ExifToolError::parse_error("/P is part of another key"));
        }
    }

    // Check that the character after /P is whitespace or delimiter
    if p_pos + 2 < data.len() {
        let next_char = data[p_pos + 2];
        if next_char != b' '
            && next_char != b'\n'
            && next_char != b'\r'
            && next_char != b'\t'
            && next_char != b'/'
            && next_char != b'>'
        {
            return Err(ExifToolError::parse_error("/P is part of another key"));
        }
    }

    let after_p = &data[p_pos + 2..];

    // Parse number
    let (_, level) = parse_number(after_p)
        .map_err(|_| ExifToolError::parse_error("Failed to parse /P value"))?;

    Ok(level)
}

/// Extracts /P from /Reference array TransformParams
fn extract_p_from_reference(
    reader: &dyn FileReader,
    context: &PdfContext,
    data: &[u8],
) -> Result<i64> {
    // Find /Reference array
    let ref_pos = data
        .windows(10)
        .position(|w| w == b"/Reference")
        .ok_or_else(|| ExifToolError::parse_error("/Reference not found"))?;

    let after_ref = &data[ref_pos..];

    // Find TransformParams reference
    let params_ref = find_transform_params_ref(after_ref)?;
    let params_offset = context.get_object_offset(params_ref.object_num, "TransformParams")?;

    // Read TransformParams object
    let params_size = std::cmp::min(2048, reader.size().saturating_sub(params_offset) as usize);
    let params_data = reader.read(params_offset, params_size)?;

    // Extract /P from TransformParams
    extract_p_value(params_data)
}

/// Finds TransformParams reference in /Reference array
fn find_transform_params_ref(data: &[u8]) -> Result<ObjectRef> {
    // Find /TransformParams key
    let params_pos = data
        .windows(16)
        .position(|w| w == b"/TransformParams")
        .ok_or_else(|| ExifToolError::parse_error("/TransformParams not found"))?;

    let after_params = &data[params_pos + 16..];

    // Parse object reference
    let (_, obj_ref) = parse_object_reference(after_params)
        .map_err(|_| ExifToolError::parse_error("Failed to parse TransformParams reference"))?;

    Ok(obj_ref)
}

/// Checks if /UR3 is present in Perms dictionary
fn check_ur3_presence(perms_data: &[u8]) -> bool {
    // Simply check if /UR3 key exists
    perms_data.windows(4).any(|w| w == b"/UR3")
}

//
// ═══════════════════════════════════════════════════════════════════════════
// PDF Reference Parsing (from info_parser pattern)
// ═══════════════════════════════════════════════════════════════════════════
//

/// Finds a dictionary reference by key name
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

/// Parses a decimal number (signed or unsigned) from bytes
fn parse_number(input: &[u8]) -> IResult<&[u8], i64> {
    use nom::combinator::map_res;
    use nom::{
        Parser,
        character::complete::{digit1, one_of},
        combinator::{opt, recognize},
        sequence::{pair, preceded},
    };

    preceded(
        multispace0,
        map_res(
            recognize(pair(opt(one_of("+-")), digit1)),
            |s: &[u8]| -> std::result::Result<i64, nom::error::Error<&[u8]>> {
                let s_str = str::from_utf8(s)
                    .map_err(|_| nom::error::Error::new(s, nom::error::ErrorKind::Char))?;
                s_str
                    .parse::<i64>()
                    .map_err(|_| nom::error::Error::new(s, nom::error::ErrorKind::Digit))
            },
        ),
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

    #[test]
    fn test_parse_object_reference() {
        let input = b"15 0 R";
        let result = parse_object_reference(input);
        assert!(result.is_ok());
        let (_, obj_ref) = result.unwrap();
        assert_eq!(obj_ref.object_num, 15);
        assert_eq!(obj_ref.generation, 0);
    }

    #[test]
    fn test_extract_p_value() {
        let data = b"<< /Type /TransformParams /P 2 /V (1.0) >>";
        let result = extract_p_value(data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);
    }

    #[test]
    fn test_extract_p_value_negative() {
        let data = b"<< /P -1 >>";
        let result = extract_p_value(data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), -1);
    }

    #[test]
    fn test_extract_p_value_not_parent() {
        // Should not match /Parent key
        let data = b"<< /Parent 5 0 R >>";
        let result = extract_p_value(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_ur3_presence() {
        let data = b"<< /DocMDP 10 0 R /UR3 11 0 R >>";
        assert!(check_ur3_presence(data));

        let data_no_ur3 = b"<< /DocMDP 10 0 R >>";
        assert!(!check_ur3_presence(data_no_ur3));
    }

    #[test]
    fn test_parse_number_positive() {
        let input = b"  123";
        let result = parse_number(input);
        assert!(result.is_ok());
        let (_, num) = result.unwrap();
        assert_eq!(num, 123);
    }

    #[test]
    fn test_parse_number_negative() {
        let input = b"  -456";
        let result = parse_number(input);
        assert!(result.is_ok());
        let (_, num) = result.unwrap();
        assert_eq!(num, -456);
    }
}
