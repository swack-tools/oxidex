//! PDF Info dictionary writer
//!
//! This module handles writing PDF Info dictionaries with modified metadata.
//! It reads an existing PDF, modifies the Info dictionary, recalculates the
//! cross-reference (xref) table with correct byte offsets, and writes a valid PDF.
//!
//! # PDF Writing Strategy
//!
//! 1. **Read Original PDF**: Parse structure to identify objects and Info dictionary
//! 2. **Build Modified PDF**: Sequentially rebuild PDF in memory buffer
//! 3. **Track Byte Offsets**: Record exact byte position of each object
//! 4. **Serialize Info Dictionary**: Convert MetadataMap to PDF dictionary format
//! 5. **Recalculate xref Table**: Write xref entries with correct byte offsets
//! 6. **Atomic Write**: Use temp-file-and-rename pattern for safety
//!
//! # Example
//!
//! ```no_run
//! use oxidex::io::buffered_reader::BufferedReader;
//! use oxidex::core::metadata_map::MetadataMap;
//! use oxidex::core::tag_value::TagValue;
//! use oxidex::writers::pdf_writer::write_pdf_file;
//! use std::path::Path;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let input_path = Path::new("input.pdf");
//! let output_path = Path::new("output.pdf");
//! let reader = BufferedReader::new(input_path)?;
//!
//! let mut metadata = MetadataMap::new();
//! metadata.insert("PDF:Title", TagValue::new_string("Modified Title"));
//! metadata.insert("PDF:Author", TagValue::new_string("New Author"));
//!
//! write_pdf_file(output_path, &reader, &metadata)?;
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::writers::atomic_writer::write_atomic;
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, Timelike, Utc};
use std::collections::{BTreeMap, HashMap, btree_map::Entry};
use std::path::Path;
use std::str;

/// Writes a complete PDF file with modified Info dictionary metadata.
///
/// This is the main entry point for PDF file writing. It reads the original file,
/// parses its structure, modifies the Info dictionary, recalculates the xref table,
/// and writes a new PDF file atomically.
///
/// # Parameters
///
/// - `path`: Output file path where the PDF file will be written
/// - `original_reader`: FileReader for the original PDF file
/// - `modified_metadata`: MetadataMap containing the PDF: tags to write
///
/// # Returns
///
/// - `Ok(())`: File written successfully with valid xref table
/// - `Err(ExifToolError)`: Write error, I/O error, or invalid metadata
///
/// # Supported PDF Metadata Fields
///
/// - PDF:Title
/// - PDF:Author
/// - PDF:Subject
/// - PDF:Keywords
/// - PDF:Creator
/// - PDF:Producer
/// - PDF:CreationDate
/// - PDF:ModDate
///
/// # Example
///
/// ```no_run
/// use oxidex::io::buffered_reader::BufferedReader;
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
/// use oxidex::writers::pdf_writer::write_pdf_file;
/// use std::path::Path;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let input_path = Path::new("input.pdf");
/// let output_path = Path::new("output.pdf");
/// let reader = BufferedReader::new(input_path)?;
///
/// let mut metadata = MetadataMap::new();
/// metadata.insert("PDF:Title", TagValue::new_string("My Document"));
///
/// write_pdf_file(output_path, &reader, &metadata)?;
/// # Ok(())
/// # }
/// ```
pub fn write_pdf_file(
    path: &Path,
    original_reader: &dyn FileReader,
    modified_metadata: &MetadataMap,
) -> Result<()> {
    // Parse original PDF structure
    let pdf_structure = parse_pdf_structure(original_reader)?;

    // Build modified PDF with updated Info dictionary
    let pdf_data = build_modified_pdf(original_reader, &pdf_structure, modified_metadata)?;

    // Write atomically to prevent corruption
    write_atomic(path, &pdf_data)?;

    Ok(())
}

/// PDF structure information extracted from original file
#[derive(Debug)]
struct PdfStructure {
    /// Byte offset of xref table start
    xref_offset: u64,
    /// Map of object number to byte offset in original file
    object_offsets: HashMap<u32, u64>,
    /// Object number of the Info dictionary
    info_object_num: u32,
    /// Generation number of Info object
    info_generation: u16,
    /// Total number of objects (for /Size in trailer)
    size: u32,
    /// Root object reference
    root_ref: ObjectRef,
}

/// Object reference structure (e.g., "4 0 R" means object 4, generation 0)
#[derive(Debug, Clone, Copy)]
struct ObjectRef {
    object_num: u32,
    generation: u16,
}

/// Allowed fields in the PDF Info dictionary
const PDF_INFO_FIELDS: &[&str] = &[
    "Title",
    "Author",
    "Subject",
    "Keywords",
    "Creator",
    "Producer",
    "CreationDate",
    "ModDate",
];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum FieldSource {
    Canonical,
    Alias,
}

fn canonicalize_pdf_field(field: &str) -> Option<(String, FieldSource)> {
    match field {
        "CreateDate" => Some(("CreationDate".to_string(), FieldSource::Alias)),
        "CreationDate" => Some(("CreationDate".to_string(), FieldSource::Canonical)),
        "ModifyDate" => Some(("ModDate".to_string(), FieldSource::Alias)),
        "ModDate" => Some(("ModDate".to_string(), FieldSource::Canonical)),
        other => {
            if PDF_INFO_FIELDS.contains(&other) {
                Some((other.to_string(), FieldSource::Canonical))
            } else {
                None
            }
        }
    }
}

/// Parses PDF structure to extract xref table and Info object location
fn parse_pdf_structure(reader: &dyn FileReader) -> Result<PdfStructure> {
    let file_size = reader.size();

    // Read the last 1024 bytes to find trailer
    let tail_size = std::cmp::min(1024, file_size as usize);
    let tail_offset = file_size - tail_size as u64;
    let tail_data = reader.read(tail_offset, tail_size)?;

    // Find startxref and get xref offset
    let xref_offset = find_xref_offset(tail_data)?;

    // Read xref table and trailer region (up to 8KB should be enough)
    let xref_size = std::cmp::min(8192, (file_size - xref_offset) as usize);
    let xref_data = reader.read(xref_offset, xref_size)?;

    // The writer only rebuilds a single classic xref section. An incremental
    // update chains earlier revisions via the trailer's /Prev key, and the
    // final section lists only the objects that revision changed; rewriting
    // from it alone would drop the Catalog/Pages/Page objects and corrupt the
    // document. Reject rather than silently destroy it.
    if trailer_has_prev(xref_data) {
        return Err(ExifToolError::unsupported_format(
            "PDF write operations are not yet supported for incrementally-updated PDFs (trailer /Prev)",
        ));
    }

    // Parse trailer to find Info reference and Root reference
    let (info_ref, root_ref, size) = parse_trailer_refs(xref_data)?;

    // Parse xref table to build object offset map
    let object_offsets = parse_xref_table(xref_data)?;

    Ok(PdfStructure {
        xref_offset,
        object_offsets,
        info_object_num: info_ref.object_num,
        info_generation: info_ref.generation,
        size,
        root_ref,
    })
}

/// Finds the startxref offset from the PDF tail
fn find_xref_offset(tail_data: &[u8]) -> Result<u64> {
    let tail_str = str::from_utf8(tail_data)
        .map_err(|_| ExifToolError::parse_error("PDF tail contains invalid UTF-8"))?;

    let startxref_pos = tail_str
        .rfind("startxref")
        .ok_or_else(|| ExifToolError::parse_error("startxref not found in PDF"))?;

    let after_keyword = &tail_str[startxref_pos + 9..]; // "startxref".len() = 9

    // Parse the offset number
    let offset = after_keyword
        .lines()
        .nth(1)
        .and_then(|line| line.trim().parse::<u64>().ok())
        .ok_or_else(|| ExifToolError::parse_error("Invalid xref offset after startxref"))?;

    Ok(offset)
}

/// Reports whether the final trailer references an earlier revision via /Prev.
///
/// Such incrementally-updated PDFs cannot be safely rewritten from the final
/// xref section alone (it lists only the last revision's objects).
fn trailer_has_prev(xref_data: &[u8]) -> bool {
    let Ok(xref_str) = str::from_utf8(xref_data) else {
        // Non-UTF-8 xref region implies a cross-reference stream, which this
        // writer also cannot rebuild; treat it as unsupported too.
        return true;
    };
    let Some(trailer_pos) = xref_str.find("trailer") else {
        // No classic trailer keyword: likely a cross-reference stream.
        return true;
    };
    xref_str[trailer_pos..].contains("/Prev")
}

/// Parses trailer to extract Info reference, Root reference, and Size
fn parse_trailer_refs(xref_data: &[u8]) -> Result<(ObjectRef, ObjectRef, u32)> {
    let xref_str = str::from_utf8(xref_data)
        .map_err(|_| ExifToolError::parse_error("xref data contains invalid UTF-8"))?;

    // Find trailer dictionary
    let trailer_pos = xref_str
        .find("trailer")
        .ok_or_else(|| ExifToolError::parse_error("trailer not found in PDF"))?;

    let trailer_section = &xref_str[trailer_pos..];

    // Find dictionary bounds
    let dict_start = trailer_section
        .find("<<")
        .ok_or_else(|| ExifToolError::parse_error("trailer dictionary start not found"))?;
    let dict_end = trailer_section[dict_start..]
        .find(">>")
        .ok_or_else(|| ExifToolError::parse_error("trailer dictionary end not found"))?;

    let dict_content = &trailer_section[dict_start..dict_start + dict_end + 2];

    // Parse /Info reference
    let info_ref = parse_dict_object_ref(dict_content, "/Info")
        .ok_or_else(|| ExifToolError::parse_error("/Info reference not found in trailer"))?;

    // Parse /Root reference
    let root_ref = parse_dict_object_ref(dict_content, "/Root")
        .ok_or_else(|| ExifToolError::parse_error("/Root reference not found in trailer"))?;

    // Parse /Size
    let size = parse_dict_integer(dict_content, "/Size")
        .ok_or_else(|| ExifToolError::parse_error("/Size not found in trailer"))?
        as u32;

    Ok((info_ref, root_ref, size))
}

/// Parses an object reference from a dictionary (e.g., "/Info 4 0 R")
fn parse_dict_object_ref(dict_str: &str, key: &str) -> Option<ObjectRef> {
    let key_pos = dict_str.find(key)?;
    let after_key = &dict_str[key_pos + key.len()..];

    // Extract numbers before 'R'
    let mut nums = Vec::new();
    for token in after_key.split_whitespace() {
        if token == "R" {
            break;
        }
        if let Ok(num) = token.parse::<u32>() {
            nums.push(num);
        }
    }

    if nums.len() >= 2 {
        Some(ObjectRef {
            object_num: nums[0],
            generation: nums[1] as u16,
        })
    } else {
        None
    }
}

/// Parses an integer value from a dictionary (e.g., "/Size 3")
fn parse_dict_integer(dict_str: &str, key: &str) -> Option<u64> {
    let key_pos = dict_str.find(key)?;
    let after_key = &dict_str[key_pos + key.len()..];

    // Find first number
    for token in after_key.split_whitespace() {
        if let Ok(num) = token.parse::<u64>() {
            return Some(num);
        }
    }

    None
}

/// Parses the xref table and builds a map of object numbers to file offsets
fn parse_xref_table(xref_data: &[u8]) -> Result<HashMap<u32, u64>> {
    let xref_str = str::from_utf8(xref_data)
        .map_err(|_| ExifToolError::parse_error("xref table contains invalid UTF-8"))?;

    let xref_pos = xref_str
        .find("xref")
        .ok_or_else(|| ExifToolError::parse_error("xref table not found"))?;

    let after_xref = &xref_str[xref_pos + 4..]; // "xref".len() = 4

    let mut object_offsets = HashMap::new();
    let mut lines = after_xref.lines();

    // Parse xref subsections
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("trailer") {
            break;
        }

        // Parse subsection header: "start_obj_num count"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2
            && let (Ok(start_num), Ok(count)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>())
        {
            // Parse each entry in this subsection
            for i in 0..count {
                if let Some(entry_line) = lines.next() {
                    let entry_parts: Vec<&str> = entry_line.split_whitespace().collect();
                    if entry_parts.len() >= 3
                        && let Ok(offset) = entry_parts[0].parse::<u64>()
                    {
                        let in_use = entry_parts[2];
                        if in_use == "n" {
                            object_offsets.insert(start_num + i, offset);
                        }
                    }
                }
            }
        }
    }

    Ok(object_offsets)
}

/// Builds a complete modified PDF file
fn build_modified_pdf(
    original_reader: &dyn FileReader,
    structure: &PdfStructure,
    modified_metadata: &MetadataMap,
) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut new_offsets: HashMap<u32, u64> = HashMap::new();

    // Write PDF header
    buffer.extend_from_slice(b"%PDF-1.4\n");

    // Read and copy all objects, modifying Info dictionary
    for (&obj_num, &offset) in &structure.object_offsets {
        // Record this object's offset in new file
        new_offsets.insert(obj_num, buffer.len() as u64);

        if obj_num == structure.info_object_num {
            // Write modified Info dictionary
            write_info_object(
                &mut buffer,
                obj_num,
                structure.info_generation,
                modified_metadata,
            )?;
        } else {
            // Copy object from original file
            copy_object(&mut buffer, original_reader, offset)?;
        }
    }

    // Record xref table offset
    let xref_start = buffer.len() as u64;

    // Write xref table
    write_xref_table(&mut buffer, &new_offsets, structure.size)?;

    // Write trailer
    write_trailer(
        &mut buffer,
        structure.size,
        structure.root_ref,
        ObjectRef {
            object_num: structure.info_object_num,
            generation: structure.info_generation,
        },
    )?;

    // Write startxref
    buffer.extend_from_slice(b"startxref\n");
    buffer.extend_from_slice(xref_start.to_string().as_bytes());
    buffer.extend_from_slice(b"\n%%EOF\n");

    Ok(buffer)
}

/// Writes a modified Info object to the buffer
fn write_info_object(
    buffer: &mut Vec<u8>,
    obj_num: u32,
    generation: u16,
    metadata: &MetadataMap,
) -> Result<()> {
    // Write object header
    buffer.extend_from_slice(obj_num.to_string().as_bytes());
    buffer.extend_from_slice(b" ");
    buffer.extend_from_slice(generation.to_string().as_bytes());
    buffer.extend_from_slice(b" obj\n");

    // Write dictionary start
    buffer.extend_from_slice(b"<<\n");

    let mut entries: BTreeMap<String, (&TagValue, FieldSource)> = BTreeMap::new();

    for (key, value) in metadata.iter() {
        if let Some(field) = key.strip_prefix("PDF:")
            && let Some((canonical, source)) = canonicalize_pdf_field(field)
        {
            match entries.entry(canonical) {
                Entry::Vacant(entry) => {
                    entry.insert((value, source));
                }
                Entry::Occupied(mut entry) => {
                    if matches!(source, FieldSource::Canonical) {
                        entry.insert((value, source));
                    }
                }
            }
        }
    }

    for (field_name, (value, _)) in entries {
        serialize_pdf_field(buffer, &field_name, value)?;
    }

    // Write dictionary end and object trailer
    buffer.extend_from_slice(b">>\nendobj\n");

    Ok(())
}

/// Serializes a single PDF Info dictionary field
fn serialize_pdf_field(buffer: &mut Vec<u8>, field_name: &str, value: &TagValue) -> Result<()> {
    // Write field name
    buffer.extend_from_slice(b"/");
    buffer.extend_from_slice(field_name.as_bytes());
    buffer.extend_from_slice(b" ");

    // Write field value based on type
    match value {
        TagValue::String(s) => {
            if matches!(field_name, "CreationDate" | "ModDate")
                && let Some(pdf_date) = convert_exif_string_to_pdf_date(s)
            {
                buffer.extend_from_slice(b"(D:");
                buffer.extend_from_slice(pdf_date.as_bytes());
                buffer.extend_from_slice(b")\n");
                return Ok(());
            }
            serialize_pdf_text_string(buffer, s);
        }
        TagValue::Integer(i) => {
            buffer.extend_from_slice(i.to_string().as_bytes());
        }
        TagValue::DateTime(dt) => {
            // Format as PDF date string: (D:YYYYMMDDHHmmSS+HH'mm')
            let datetime_str = format_pdf_datetime(dt);
            buffer.extend_from_slice(b"(D:");
            buffer.extend_from_slice(datetime_str.as_bytes());
            buffer.extend_from_slice(b")");
        }
        TagValue::Float(f) => {
            buffer.extend_from_slice(f.to_string().as_bytes());
        }
        TagValue::Rational {
            numerator,
            denominator,
        } => {
            // Write as fraction string
            let rational_str = format!("{}/{}", numerator, denominator);
            buffer.extend_from_slice(b"(");
            buffer.extend_from_slice(rational_str.as_bytes());
            buffer.extend_from_slice(b")");
        }
        TagValue::Binary(data) => {
            // Write as hex string
            buffer.extend_from_slice(b"<");
            for byte in data {
                buffer.extend_from_slice(format!("{:02X}", byte).as_bytes());
            }
            buffer.extend_from_slice(b">");
        }
        TagValue::Array(values) => {
            let mut keyword_strings: Vec<String> = Vec::new();
            for value in values {
                if let TagValue::String(s) = value {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        keyword_strings.push(trimmed.to_string());
                    }
                }
            }

            let joined = keyword_strings.join(", ");
            serialize_pdf_text_string(buffer, &joined);
        }
        TagValue::Struct(_) => {
            // Structured data not supported in PDF Info dictionary
            // Write empty string
            buffer.extend_from_slice(b"()");
        }
    }

    buffer.extend_from_slice(b"\n");
    Ok(())
}

fn serialize_pdf_text_string(buffer: &mut Vec<u8>, s: &str) {
    if s.chars()
        .all(|c| c.is_ascii() && c != '(' && c != ')' && c != '\\')
    {
        buffer.extend_from_slice(b"(");
        buffer.extend_from_slice(s.as_bytes());
        buffer.extend_from_slice(b")");
    } else {
        serialize_hex_string(buffer, s);
    }
}

/// Serializes a string as a PDF hex string with UTF-16BE encoding
fn serialize_hex_string(buffer: &mut Vec<u8>, s: &str) {
    buffer.extend_from_slice(b"<");

    // Write UTF-16BE BOM
    buffer.extend_from_slice(b"FEFF");

    // Encode string as UTF-16BE
    for c in s.encode_utf16() {
        buffer.extend_from_slice(format!("{:04X}", c).as_bytes());
    }

    buffer.extend_from_slice(b">");
}

/// Formats a chrono DateTime<Utc> into PDF date string components
fn format_pdf_datetime(dt: &DateTime<Utc>) -> String {
    let fixed = dt.with_timezone(&FixedOffset::east_opt(0).unwrap());
    format_fixed_offset_pdf_date(fixed)
}

/// Converts an EXIF-style string (YYYY:MM:DD HH:MM:SS[+HH:MM]) to PDF date format
fn convert_exif_string_to_pdf_date(value: &str) -> Option<String> {
    if let Ok(dt) = DateTime::parse_from_str(value, "%Y:%m:%d %H:%M:%S%:z") {
        return Some(format_fixed_offset_pdf_date(dt));
    }

    if let Ok(dt) = DateTime::parse_from_str(value, "%Y:%m:%d %H:%M:%S%.f%:z") {
        return Some(format_fixed_offset_pdf_date(dt));
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(value, "%Y:%m:%d %H:%M:%S") {
        let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
        let fixed = utc_dt.with_timezone(&FixedOffset::east_opt(0).unwrap());
        return Some(format_fixed_offset_pdf_date(fixed));
    }

    if let Ok(date_only) = NaiveDate::parse_from_str(value, "%Y:%m:%d") {
        let naive = date_only.and_hms_opt(0, 0, 0)?;
        let utc_dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
        let fixed = utc_dt.with_timezone(&FixedOffset::east_opt(0).unwrap());
        return Some(format_fixed_offset_pdf_date(fixed));
    }

    None
}

/// Formats a fixed-offset DateTime into PDF Info date string body (without leading "D:")
fn format_fixed_offset_pdf_date(dt: DateTime<FixedOffset>) -> String {
    let offset_seconds = dt.offset().local_minus_utc();
    let sign = if offset_seconds >= 0 { '+' } else { '-' };
    let abs_offset = offset_seconds.abs();
    let hours = abs_offset / 3600;
    let minutes = (abs_offset % 3600) / 60;

    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}{}{:02}'{:02}'",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        sign,
        hours,
        minutes
    )
}

/// Copies an object from the original file to the buffer
fn copy_object(buffer: &mut Vec<u8>, reader: &dyn FileReader, offset: u64) -> Result<()> {
    // Read a chunk starting at offset (4KB should be enough for most objects)
    let chunk_size = 4096;
    let file_size = reader.size();
    let read_size = std::cmp::min(chunk_size, (file_size - offset) as usize);
    let data = reader.read(offset, read_size)?;

    // Find "endobj" to determine object end
    let data_str = str::from_utf8(data)
        .map_err(|_| ExifToolError::parse_error("Object contains invalid UTF-8"))?;

    let endobj_pos = data_str
        .find("endobj")
        .ok_or_else(|| ExifToolError::parse_error("endobj not found in object"))?;

    // Copy object including "endobj" and newline
    let object_end = endobj_pos + 6; // "endobj".len() = 6
    buffer.extend_from_slice(&data[..object_end]);
    buffer.extend_from_slice(b"\n");

    Ok(())
}

/// Writes the xref (cross-reference) table
fn write_xref_table(buffer: &mut Vec<u8>, offsets: &HashMap<u32, u64>, size: u32) -> Result<()> {
    buffer.extend_from_slice(b"xref\n");

    // Write subsection header
    buffer.extend_from_slice(b"0 ");
    buffer.extend_from_slice(size.to_string().as_bytes());
    buffer.extend_from_slice(b"\n");

    // Write entries for all objects
    for obj_num in 0..size {
        if obj_num == 0 {
            // First entry is always free object
            buffer.extend_from_slice(b"0000000000 65535 f \n");
        } else if let Some(&offset) = offsets.get(&obj_num) {
            // In-use object - write offset
            buffer.extend_from_slice(format!("{:010} 00000 n \n", offset).as_bytes());
        } else {
            // Object not in use
            buffer.extend_from_slice(b"0000000000 00000 f \n");
        }
    }

    Ok(())
}

/// Writes the trailer dictionary
fn write_trailer(
    buffer: &mut Vec<u8>,
    size: u32,
    root_ref: ObjectRef,
    info_ref: ObjectRef,
) -> Result<()> {
    buffer.extend_from_slice(b"trailer\n");
    buffer.extend_from_slice(b"<<\n");
    buffer.extend_from_slice(b"/Size ");
    buffer.extend_from_slice(size.to_string().as_bytes());
    buffer.extend_from_slice(b"\n");
    buffer.extend_from_slice(b"/Root ");
    buffer.extend_from_slice(root_ref.object_num.to_string().as_bytes());
    buffer.extend_from_slice(b" ");
    buffer.extend_from_slice(root_ref.generation.to_string().as_bytes());
    buffer.extend_from_slice(b" R\n");
    buffer.extend_from_slice(b"/Info ");
    buffer.extend_from_slice(info_ref.object_num.to_string().as_bytes());
    buffer.extend_from_slice(b" ");
    buffer.extend_from_slice(info_ref.generation.to_string().as_bytes());
    buffer.extend_from_slice(b" R\n");
    buffer.extend_from_slice(b">>\n");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_xref_offset() {
        let tail = b"startxref\n1234\n%%EOF";
        let result = find_xref_offset(tail);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1234);
    }

    #[test]
    fn test_trailer_has_prev() {
        // Incremental update: trailer references an earlier revision.
        assert!(trailer_has_prev(
            b"xref\n0 1\ntrailer<</Size 5/Root 1 0 R/Prev 116>>\nstartxref\n"
        ));
        // Classic single-revision trailer: safe to rebuild.
        assert!(!trailer_has_prev(
            b"xref\n0 5\ntrailer<</Size 5/Root 1 0 R/Info 4 0 R>>\nstartxref\n"
        ));
        // Cross-reference stream (no `trailer` keyword): also unsupported.
        assert!(trailer_has_prev(b"5 0 obj<</Type/XRef/Size 6>>stream\n"));
    }

    #[test]
    fn test_parse_dict_object_ref() {
        let dict = "<< /Info 4 0 R /Root 1 0 R >>";
        let info_ref = parse_dict_object_ref(dict, "/Info");
        assert!(info_ref.is_some());
        let info = info_ref.unwrap();
        assert_eq!(info.object_num, 4);
        assert_eq!(info.generation, 0);
    }

    #[test]
    fn test_parse_dict_integer() {
        let dict = "<< /Size 10 /Count 5 >>";
        let size = parse_dict_integer(dict, "/Size");
        assert_eq!(size, Some(10));
    }

    #[test]
    fn test_serialize_hex_string() {
        let mut buffer = Vec::new();
        serialize_hex_string(&mut buffer, "Test");

        // Should be <FEFF + UTF-16BE encoding>
        let result = String::from_utf8(buffer).unwrap();
        assert!(result.starts_with("<FEFF"));
        assert!(result.ends_with(">"));
    }
}
