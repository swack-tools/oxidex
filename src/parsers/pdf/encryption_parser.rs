//! PDF Encryption dictionary parser
//!
//! This module handles parsing of PDF Encrypt dictionaries, which contain
//! encryption and security information including encryption method, algorithm
//! version, key length, and user permissions.
//!
//! # PDF Encryption Structure
//!
//! Encrypted PDFs have an /Encrypt entry in the trailer that references the
//! encryption dictionary:
//! ```text
//! trailer
//! << /Size 6 /Root 1 0 R /Encrypt 5 0 R >>
//! ```
//!
//! The Encrypt dictionary contains encryption parameters:
//! ```text
//! 5 0 obj
//! <<
//!   /Filter /Standard
//!   /V 4
//!   /R 4
//!   /Length 128
//!   /P -1340
//!   /CF << /StdCF << /CFM /AESV2 >> >>
//! >>
//! endobj
//! ```
//!
//! # Extracted Tags
//!
//! - **PDF:Encryption**: Formatted as "Standard V{version}.{revision} {length}-bit {method}"
//!   - Example: "Standard V4.4 128-bit AES"
//! - **PDF:UserAccess**: Comma-separated list of allowed permissions
//!   - Example: "Print, Copy, Annotate"
//! - **PDF:Encrypted**: "Yes" or "No" to indicate encryption status

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use nom::{
    IResult,
    bytes::complete::{tag, take_until, take_while},
    character::complete::{digit1, multispace0},
    combinator::{map_res, opt},
    sequence::preceded,
};
use std::collections::HashMap;
use std::str;

//
// ═══════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════
//

/// Extracts PDF Encrypt dictionary metadata from a PDF file.
///
/// This function:
/// 1. Locates the trailer dictionary at the end of the file
/// 2. Checks for /Encrypt reference
/// 3. If encrypted, reads the Encrypt dictionary
/// 4. Parses encryption parameters (/V, /R, /Length, /P, /CF)
/// 5. Formats encryption string and decodes permissions
///
/// # Parameters
///
/// - `reader`: FileReader implementation for accessing the PDF file
///
/// # Returns
///
/// - `Ok(MetadataMap)`: Extracted encryption metadata with "PDF:" prefix
/// - `Err(ExifToolError)`: Parse error or I/O error
///
/// # Extracted Tags
///
/// - **PDF:Encrypted**: "Yes" if encrypted, "No" otherwise
/// - **PDF:Encryption**: Algorithm description (e.g., "Standard V4.4 128-bit AES")
/// - **PDF:UserAccess**: Comma-separated list of permissions
pub fn parse_encryption_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let file_size = reader.size();

    // Read the last 1024 bytes to find trailer
    let tail_size = std::cmp::min(1024, file_size as usize);
    let tail_offset = file_size.saturating_sub(tail_size as u64);
    let tail_data = reader.read(tail_offset, tail_size)?;

    // Check if /Encrypt exists in trailer
    let has_encrypt = tail_data.windows(8).any(|window| window == b"/Encrypt");

    let mut metadata = MetadataMap::new();

    if !has_encrypt {
        // Not encrypted - return "No"
        metadata.insert(
            "PDF:Encrypted".to_string(),
            TagValue::new_string("No".to_string()),
        );
        return Ok(metadata);
    }

    // PDF is encrypted
    metadata.insert(
        "PDF:Encrypted".to_string(),
        TagValue::new_string("Yes".to_string()),
    );

    // Find startxref and get xref offset
    let xref_offset = find_xref_offset(tail_data)?;

    // Read xref table and trailer region
    let xref_size = std::cmp::min(8192, file_size.saturating_sub(xref_offset) as usize);
    let xref_data = reader.read(xref_offset, xref_size)?;

    // Parse xref table to build object offset map
    let xref_map = parse_xref_table(xref_data)?;

    // Find /Encrypt reference in trailer
    let encrypt_ref = match find_encrypt_reference(xref_data) {
        Ok(obj_ref) => obj_ref,
        Err(_) => {
            // Has /Encrypt marker but can't parse - return minimal metadata
            return Ok(metadata);
        }
    };

    // Get offset from xref table
    let encrypt_offset = match xref_map.get(&encrypt_ref.object_num) {
        Some(&offset) => offset,
        None => return Ok(metadata), // Can't find object, return what we have
    };

    // Read Encrypt object
    let encrypt_size = std::cmp::min(4096, file_size.saturating_sub(encrypt_offset) as usize);
    let encrypt_data = reader.read(encrypt_offset, encrypt_size)?;

    // Parse Encrypt dictionary
    let encrypt_dict = match parse_encrypt_object(encrypt_data) {
        Ok(dict) => dict,
        Err(_) => return Ok(metadata), // Can't parse, return what we have
    };

    // Extract encryption details
    if let Some(encryption_string) = format_encryption_string(&encrypt_dict, encrypt_data) {
        metadata.insert(
            "PDF:Encryption".to_string(),
            TagValue::new_string(encryption_string),
        );
    }

    // Extract and decode permissions
    if let Some(permissions) = decode_permissions_from_dict(&encrypt_dict) {
        metadata.insert(
            "PDF:UserAccess".to_string(),
            TagValue::new_string(permissions),
        );
    }

    Ok(metadata)
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Encryption Dictionary Parsing
// ═══════════════════════════════════════════════════════════════════════════
//

/// Object reference structure (e.g., "5 0 R" means object 5, generation 0)
#[derive(Debug, Clone, Copy)]
struct ObjectRef {
    object_num: u32,
    #[allow(dead_code)]
    generation: u16,
}

/// Finds the /Encrypt reference in the trailer
fn find_encrypt_reference(data: &[u8]) -> Result<ObjectRef> {
    let (_, obj_ref) = parse_dict_reference(data, "/Encrypt")
        .map_err(|_| ExifToolError::parse_error("Could not parse /Encrypt reference"))?;
    Ok(obj_ref)
}

/// Parses a dictionary reference for the given key
fn parse_dict_reference<'a>(input: &'a [u8], key: &str) -> IResult<&'a [u8], ObjectRef> {
    let (input, _) = take_until(key.as_bytes())(input)?;
    let (input, _) = tag(key.as_bytes())(input)?;
    let (input, _) = multispace0(input)?;
    parse_object_reference(input)
}

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

/// Parses the Encrypt object and extracts key-value pairs
fn parse_encrypt_object(input: &[u8]) -> Result<HashMap<String, String>> {
    let input_str = str::from_utf8(input)
        .map_err(|_| ExifToolError::parse_error("Encrypt object contains invalid UTF-8"))?;

    let dict_start = input_str
        .find("<<")
        .ok_or_else(|| ExifToolError::parse_error("Encrypt dictionary start << not found"))?;

    let dict_end = input_str[dict_start..]
        .find(">>")
        .ok_or_else(|| ExifToolError::parse_error("Encrypt dictionary end >> not found"))?;

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

    // Parse simple key-value pairs (name -> number/name)
    let mut dict = HashMap::new();

    // Extract /V (version)
    if let Some(v_val) = extract_numeric_value(dict_content, "/V") {
        dict.insert("V".to_string(), v_val.to_string());
    }

    // Extract /R (revision)
    if let Some(r_val) = extract_numeric_value(dict_content, "/R") {
        dict.insert("R".to_string(), r_val.to_string());
    }

    // Extract /Length (key length in bits)
    if let Some(length_val) = extract_numeric_value(dict_content, "/Length") {
        dict.insert("Length".to_string(), length_val.to_string());
    }

    // Extract /P (permissions - can be negative)
    if let Some(p_val) = extract_signed_numeric_value(dict_content, "/P") {
        dict.insert("P".to_string(), p_val.to_string());
    }

    // Extract /Filter (usually /Standard)
    if let Some(filter_val) = extract_name_value(dict_content, "/Filter") {
        dict.insert("Filter".to_string(), filter_val);
    }

    Ok(dict)
}

/// Extracts an unsigned numeric value for a given key
fn extract_numeric_value(content: &str, key: &str) -> Option<u64> {
    let key_pos = content.find(key)?;
    let after_key = &content[key_pos + key.len()..];

    // Skip whitespace
    let trimmed = after_key.trim_start();

    // Parse number
    let (_, num) = parse_number(trimmed.as_bytes()).ok()?;
    Some(num)
}

/// Extracts a signed numeric value for a given key (for /P permissions)
fn extract_signed_numeric_value(content: &str, key: &str) -> Option<i32> {
    let key_pos = content.find(key)?;
    let after_key = &content[key_pos + key.len()..];

    // Skip whitespace
    let trimmed = after_key.trim_start();

    // Parse signed number
    let (_, num) = parse_signed_number(trimmed.as_bytes()).ok()?;
    Some(num)
}

/// Extracts a name value (e.g., /Standard) for a given key
fn extract_name_value(content: &str, key: &str) -> Option<String> {
    let key_pos = content.find(key)?;
    let after_key = &content[key_pos + key.len()..];

    // Skip whitespace
    let trimmed = after_key.trim_start();

    // Parse name (starts with /)
    if let Some(stripped) = trimmed.strip_prefix('/') {
        // Take alphanumeric characters
        let name: String = stripped
            .chars()
            .take_while(|c| c.is_alphanumeric())
            .collect();
        if !name.is_empty() {
            return Some(name);
        }
    }

    None
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Encryption String Formatting
// ═══════════════════════════════════════════════════════════════════════════
//

/// Formats the encryption string from the Encrypt dictionary
/// Format: "Standard V{version}.{revision} {length}-bit {method}"
fn format_encryption_string(dict: &HashMap<String, String>, raw_data: &[u8]) -> Option<String> {
    let filter = dict.get("Filter")?;
    let version = dict.get("V")?.parse::<u32>().ok()?;
    let revision = dict.get("R")?.parse::<u32>().ok()?;

    // Determine key length
    let length = if let Some(len_str) = dict.get("Length") {
        len_str.parse::<u32>().unwrap_or(40)
    } else {
        // Default lengths based on version
        match version {
            1 => 40,
            2 | 3 => 128,
            4 => 128,
            5 => 256,
            _ => 128,
        }
    };

    // Determine encryption method
    let method = determine_encryption_method(version, raw_data);

    Some(format!(
        "{} V{}.{} {}-bit {}",
        filter, version, revision, length, method
    ))
}

/// Determines the encryption method based on version and /CF dictionary
fn determine_encryption_method(version: u32, raw_data: &[u8]) -> &'static str {
    if version <= 1 {
        return "RC4";
    }

    if version == 2 || version == 3 {
        return "RC4";
    }

    // For V4+, check /CF -> /StdCF -> /CFM
    if version >= 4 {
        let data_str = String::from_utf8_lossy(raw_data);

        // Look for /CFM value in /StdCF
        if let Some(cfm_pos) = data_str.find("/CFM") {
            let after_cfm = &data_str[cfm_pos + 4..];
            let trimmed = after_cfm.trim_start();

            if trimmed.starts_with("/AESV3") || trimmed.starts_with("/AESV2") {
                return "AES";
            } else if trimmed.starts_with("/V2") {
                return "RC4";
            }
        }

        // Default for V4 is AES if length >= 128
        return "AES";
    }

    // V5 is always AES-256
    if version == 5 {
        return "AES";
    }

    "RC4"
}

//
// ═══════════════════════════════════════════════════════════════════════════
// Permission Decoding
// ═══════════════════════════════════════════════════════════════════════════
//

/// Decodes permissions from the Encrypt dictionary
fn decode_permissions_from_dict(dict: &HashMap<String, String>) -> Option<String> {
    let p_str = dict.get("P")?;
    let p_value = p_str.parse::<i32>().ok()?;
    Some(decode_permissions(p_value))
}

/// Decodes the /P permission integer into human-readable list
///
/// Permission bits (when SET, permission is GRANTED):
/// - Bit 3 (0x4): Print
/// - Bit 4 (0x8): Modify
/// - Bit 5 (0x10): Copy
/// - Bit 6 (0x20): Annotate
/// - Bit 9 (0x100): Fill Forms
/// - Bit 10 (0x200): Extract
/// - Bit 11 (0x400): Assemble
/// - Bit 12 (0x800): Print high-res
fn decode_permissions(p_value: i32) -> String {
    let mut permissions = Vec::new();

    // Check each permission bit
    if p_value & 0x4 != 0 {
        permissions.push("Print");
    }
    if p_value & 0x8 != 0 {
        permissions.push("Modify");
    }
    if p_value & 0x10 != 0 {
        permissions.push("Copy");
    }
    if p_value & 0x20 != 0 {
        permissions.push("Annotate");
    }
    if p_value & 0x100 != 0 {
        permissions.push("Fill Forms");
    }
    if p_value & 0x200 != 0 {
        permissions.push("Extract");
    }
    if p_value & 0x400 != 0 {
        permissions.push("Assemble");
    }
    if p_value & 0x800 != 0 {
        permissions.push("Print high-res");
    }

    if permissions.is_empty() {
        "None".to_string()
    } else {
        permissions.join(", ")
    }
}

//
// ═══════════════════════════════════════════════════════════════════════════
// XRef Table Parsing (Shared with info_parser)
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

/// Parses xref entries and returns a map of object numbers to offsets
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

/// Parses a single xref subsection header: "start_obj_num count"
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

/// Parses a single xref entry: "offset generation n/f"
fn parse_xref_entry(input: &[u8]) -> IResult<&[u8], Option<u64>> {
    let (input, offset) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _generation) = parse_number(input)?;
    let (input, _) = multispace0(input)?;
    let (input, in_use) = take_while(|c| c == b'n' || c == b'f')(input)?;
    let (input, _) = multispace0(input)?;

    let result = if !in_use.is_empty() && in_use[0] == b'n' {
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

/// Parses an unsigned decimal number from bytes
fn parse_number(input: &[u8]) -> IResult<&[u8], u64> {
    use nom::Parser;
    preceded(
        multispace0,
        map_res(map_res(digit1, str::from_utf8), |s: &str| s.parse::<u64>()),
    )
    .parse(input)
}

/// Parses a signed decimal number from bytes (for /P permissions)
fn parse_signed_number(input: &[u8]) -> IResult<&[u8], i32> {
    use nom::{Parser, combinator::recognize, sequence::pair};
    preceded(
        multispace0,
        map_res(
            map_res(recognize(pair(opt(tag(&b"-"[..])), digit1)), str::from_utf8),
            |s: &str| s.parse::<i32>(),
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
    use crate::test_support::TestReader;

    #[test]
    fn test_parse_unencrypted_pdf() {
        let pdf = b"%PDF-1.4
1 0 obj
<< /Type /Catalog >>
endobj
xref
0 2
0000000000 65535 f
0000000009 00000 n
trailer
<< /Size 2 /Root 1 0 R >>
startxref
100
%%EOF";

        let reader = TestReader::new(pdf.to_vec());
        let result = parse_encryption_metadata(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:Encrypted"), Some("No"));
        assert!(metadata.get("PDF:Encryption").is_none());
    }

    #[test]
    fn test_parse_encrypted_pdf_v2_rc4() {
        let pdf = b"%PDF-1.4
1 0 obj
<< /Type /Catalog >>
endobj
5 0 obj
<<
/Filter /Standard
/V 2
/R 3
/Length 128
/P -1340
>>
endobj
xref
0 6
0000000000 65535 f
0000000009 00000 n
0000000000 65535 f
0000000000 65535 f
0000000000 65535 f
0000000045 00000 n
trailer
<< /Size 6 /Root 1 0 R /Encrypt 5 0 R >>
startxref
115
%%EOF";

        let reader = TestReader::new(pdf.to_vec());
        let result = parse_encryption_metadata(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:Encrypted"), Some("Yes"));
        assert_eq!(
            metadata.get_string("PDF:Encryption"),
            Some("Standard V2.3 128-bit RC4")
        );
    }

    #[test]
    fn test_parse_encrypted_pdf_v4_aes() {
        let pdf = b"%PDF-1.6
1 0 obj
<< /Type /Catalog >>
endobj
5 0 obj
<<
/Filter /Standard
/V 4
/R 4
/Length 128
/P -1340
/CF << /StdCF << /CFM /AESV2 >> >>
>>
endobj
xref
0 6
0000000000 65535 f
0000000009 00000 n
0000000000 65535 f
0000000000 65535 f
0000000000 65535 f
0000000045 00000 n
trailer
<< /Size 6 /Root 1 0 R /Encrypt 5 0 R >>
startxref
150
%%EOF";

        let reader = TestReader::new(pdf.to_vec());
        let result = parse_encryption_metadata(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();
        assert_eq!(metadata.get_string("PDF:Encrypted"), Some("Yes"));
        assert_eq!(
            metadata.get_string("PDF:Encryption"),
            Some("Standard V4.4 128-bit AES")
        );
    }

    #[test]
    fn test_decode_permissions_all_granted() {
        // All permission bits set
        let p_value: i32 = 0x4 | 0x8 | 0x10 | 0x20 | 0x100 | 0x200 | 0x400 | 0x800;
        let result = decode_permissions(p_value);
        assert_eq!(
            result,
            "Print, Modify, Copy, Annotate, Fill Forms, Extract, Assemble, Print high-res"
        );
    }

    #[test]
    fn test_decode_permissions_partial() {
        // Only print and copy
        let p_value: i32 = 0x4 | 0x10;
        let result = decode_permissions(p_value);
        assert_eq!(result, "Print, Copy");
    }

    #[test]
    fn test_decode_permissions_none() {
        let p_value: i32 = 0;
        let result = decode_permissions(p_value);
        assert_eq!(result, "None");
    }

    #[test]
    fn test_decode_permissions_negative_value() {
        // -1340 in binary has many bits set
        let p_value: i32 = -1340;
        let result = decode_permissions(p_value);
        // Should still parse permission bits correctly
        assert!(result.contains("Print") || result.contains("Modify"));
    }

    #[test]
    fn test_parse_object_reference() {
        let input = b"5 0 R";
        let result = parse_object_reference(input);
        assert!(result.is_ok());
        let (_, obj_ref) = result.unwrap();
        assert_eq!(obj_ref.object_num, 5);
        assert_eq!(obj_ref.generation, 0);
    }

    #[test]
    fn test_extract_numeric_value() {
        let content = "/V 4 /R 3 /Length 128";
        assert_eq!(extract_numeric_value(content, "/V"), Some(4));
        assert_eq!(extract_numeric_value(content, "/R"), Some(3));
        assert_eq!(extract_numeric_value(content, "/Length"), Some(128));
    }

    #[test]
    fn test_extract_signed_numeric_value() {
        let content = "/P -1340 /Other 42";
        assert_eq!(extract_signed_numeric_value(content, "/P"), Some(-1340));
        assert_eq!(extract_signed_numeric_value(content, "/Other"), Some(42));
    }

    #[test]
    fn test_extract_name_value() {
        let content = "/Filter /Standard /Type /Encrypt";
        assert_eq!(
            extract_name_value(content, "/Filter"),
            Some("Standard".to_string())
        );
        assert_eq!(
            extract_name_value(content, "/Type"),
            Some("Encrypt".to_string())
        );
    }

    #[test]
    fn test_determine_encryption_method() {
        // V2 should be RC4
        assert_eq!(determine_encryption_method(2, b""), "RC4");

        // V4 with AESV2
        let v4_aes = b"/CF << /StdCF << /CFM /AESV2 >> >>";
        assert_eq!(determine_encryption_method(4, v4_aes), "AES");

        // V4 with V2 (RC4)
        let v4_rc4 = b"/CF << /StdCF << /CFM /V2 >> >>";
        assert_eq!(determine_encryption_method(4, v4_rc4), "RC4");

        // V5 should always be AES
        assert_eq!(determine_encryption_method(5, b""), "AES");
    }
}
