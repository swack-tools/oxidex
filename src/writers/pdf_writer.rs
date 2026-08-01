//! PDF Info dictionary writer
//!
//! This module writes PDF Info dictionary metadata as an **incremental
//! update**: the original file is kept byte-for-byte and a new revision is
//! appended after it. That is the mechanism PDF was designed with, and the
//! same principle as the JPEG and TIFF writers -- never rebuild, only append
//! and repoint.
//!
//! # PDF Writing Strategy
//!
//! 1. **Locate the last revision**: `startxref` -> classic xref table -> trailer
//! 2. **Copy the original verbatim**: every object, stream and comment survives
//! 3. **Append a new Info object**: serialized from the MetadataMap
//! 4. **Append a one-entry xref section**: listing only that object
//! 5. **Append a trailer with /Prev**: chaining to the revision it updates
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
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::Path;
use std::str;

/// Writes a complete PDF file with modified Info dictionary metadata.
///
/// This is the main entry point for PDF file writing. It copies the original
/// file verbatim and appends a new revision holding the updated Info
/// dictionary, its own xref section and a trailer chaining the previous
/// revision via /Prev, then writes the result atomically.
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
    /// Byte offset of the last xref table, which the appended revision chains
    /// to via the trailer's /Prev key
    xref_offset: u64,
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

    // Find startxref and get the declared xref offset
    let declared = find_xref_offset(tail_data)?;

    // The appended revision is a classic xref section whose /Prev chains to
    // this one, so /Prev must be the offset of a real `xref` keyword. A stale
    // startxref would produce a chain no reader could follow, so verify it and
    // fall back to the last classic table actually present.
    let whole = reader.read(0, file_size as usize)?;
    let declared_lands_on_xref = usize::try_from(declared)
        .ok()
        .and_then(|at| whole.get(at..))
        .is_some_and(|rest| rest.starts_with(b"xref"));
    let xref_offset = if declared_lands_on_xref {
        declared
    } else {
        // A PDF 1.5+ cross-reference *stream* has no classic table at all and
        // cannot be chained this way; refuse rather than append a revision no
        // conforming reader would follow.
        let found = rfind(whole, b"\nxref").ok_or_else(|| {
            ExifToolError::unsupported_format(
                "PDF write operations are not yet supported for cross-reference \
                 stream PDFs (PDF 1.5+ /Type /XRef)",
            )
        })?;
        (found + 1) as u64
    };

    // Read xref table and trailer region (up to 8KB should be enough)
    let xref_size = std::cmp::min(8192, (file_size - xref_offset) as usize);
    let xref_data = reader.read(xref_offset, xref_size)?;

    // Parse trailer to find Info reference and Root reference
    let (info_ref, root_ref, size) = parse_trailer_refs(xref_data)?;

    // A PDF with no /Info gets one: the next free object number, which the
    // appended trailer then declares.
    let (info_object_num, info_generation) = match info_ref {
        Some(r) => (r.object_num, r.generation),
        None => (size, 0),
    };

    Ok(PdfStructure {
        xref_offset,
        info_object_num,
        info_generation,
        size,
        root_ref,
    })
}

/// Offset of the last occurrence of `needle` in `haystack`.
fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

/// Finds the startxref offset from the PDF tail
fn find_xref_offset(tail_data: &[u8]) -> Result<u64> {
    crate::parsers::pdf::find_startxref_offset(tail_data)
}

/// Parses trailer to extract Info reference (absent in PDFs with no Info
/// dictionary), Root reference, and Size
fn parse_trailer_refs(xref_data: &[u8]) -> Result<(Option<ObjectRef>, ObjectRef, u32)> {
    // The xref region is ASCII up to the trailer; binary comments after %%EOF
    // must not make the whole parse fail.
    let xref_str = match str::from_utf8(xref_data) {
        Ok(s) => s,
        Err(e) => str::from_utf8(&xref_data[..e.valid_up_to()])
            .map_err(|_| ExifToolError::parse_error("xref data contains invalid UTF-8"))?,
    };

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

    // Parse /Info reference (optional: a PDF may have no Info dictionary)
    let info_ref = parse_dict_object_ref(dict_content, "/Info");

    // Parse /Root reference
    let root_ref = parse_dict_object_ref(dict_content, "/Root")
        .ok_or_else(|| ExifToolError::parse_error("/Root reference not found in trailer"))?;

    // Parse /Size
    let size = parse_dict_integer(dict_content, "/Size")
        .ok_or_else(|| ExifToolError::parse_error("/Size not found in trailer"))?
        as u32;

    Ok((info_ref, root_ref, size))
}

/// Splits a dictionary fragment into PDF tokens.
///
/// Whitespace alone is not enough: PDF permits compact dictionaries where a
/// value abuts the next key, as in `<</Size 5/Root 1 0 R>>`. Splitting that on
/// whitespace yields "5/Root", which does not parse as a number — so /Size
/// silently resolved to the *next* integer in the dictionary and the appended
/// trailer declared a wrong object count. Delimiters are token boundaries.
fn pdf_tokens(fragment: &str) -> impl Iterator<Item = &str> {
    fragment
        .split(|c: char| c.is_whitespace() || "/<>[]()".contains(c))
        .filter(|t| !t.is_empty())
}

/// Parses an object reference from a dictionary (e.g., "/Info 4 0 R")
fn parse_dict_object_ref(dict_str: &str, key: &str) -> Option<ObjectRef> {
    let key_pos = dict_str.find(key)?;
    let after_key = &dict_str[key_pos + key.len()..];

    // Extract numbers before 'R'
    let mut nums = Vec::new();
    for token in pdf_tokens(after_key) {
        if token == "R" {
            break;
        }
        match token.parse::<u32>() {
            Ok(num) => nums.push(num),
            // A non-numeric, non-"R" token means this key's value is not an
            // indirect reference; stop rather than scavenge later keys.
            Err(_) => break,
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

    // The value is the very next token, not the next token that happens to
    // parse as a number — see `pdf_tokens`.
    pdf_tokens(after_key).next()?.parse::<u64>().ok()
}

/// Builds the updated PDF as an **incremental update**: the original bytes
/// verbatim, followed by a new revision containing just the Info dictionary.
///
/// This is the mechanism PDF was designed with, and the same principle the
/// TIFF and JPEG writers use — never rebuild, only append and repoint. It
/// replaced a whole-document rebuild that walked the final xref section and
/// re-emitted the objects it found there: content streams it could not copy,
/// objects absent from that section, and the original `%PDF-` version were
/// all lost, which turned an 8.9 kB corpus PDF into a 384-byte stub with
/// none of its 122 tags. Appending cannot lose what it never touches.
fn build_modified_pdf(
    original_reader: &dyn FileReader,
    structure: &PdfStructure,
    modified_metadata: &MetadataMap,
) -> Result<Vec<u8>> {
    let size = original_reader.size() as usize;
    let mut buffer = original_reader.read(0, size)?.to_vec();

    // A revision must start on its own line
    if !buffer.ends_with(b"\n") {
        buffer.push(b'\n');
    }

    let info_offset = buffer.len() as u64;
    write_info_object(
        &mut buffer,
        structure.info_object_num,
        structure.info_generation,
        modified_metadata,
    )?;

    // New xref section: one subsection listing only the object this revision
    // changed. Entries are exactly 20 bytes, as the spec requires.
    let xref_start = buffer.len() as u64;
    buffer.extend_from_slice(b"xref\n");
    buffer.extend_from_slice(format!("{} 1\n", structure.info_object_num).as_bytes());
    buffer.extend_from_slice(
        format!("{:010} {:05} n \n", info_offset, structure.info_generation).as_bytes(),
    );

    // /Size must exceed the highest object number in use; /Prev chains to the
    // revision this one updates, so every earlier object stays resolvable.
    let new_size = structure.size.max(structure.info_object_num + 1);
    buffer.extend_from_slice(b"trailer\n<<\n");
    buffer.extend_from_slice(format!("/Size {}\n", new_size).as_bytes());
    buffer.extend_from_slice(
        format!(
            "/Root {} {} R\n",
            structure.root_ref.object_num, structure.root_ref.generation
        )
        .as_bytes(),
    );
    buffer.extend_from_slice(
        format!(
            "/Info {} {} R\n",
            structure.info_object_num, structure.info_generation
        )
        .as_bytes(),
    );
    buffer.extend_from_slice(format!("/Prev {}\n", structure.xref_offset).as_bytes());
    buffer.extend_from_slice(b">>\nstartxref\n");
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

    /// An already-incrementally-updated PDF is now writable: appending yet
    /// another revision on top is exactly what /Prev chaining is for. The
    /// trailer's own /Prev must still parse, and /Info stays optional.
    #[test]
    fn test_parse_trailer_refs_accepts_prev_and_optional_info() {
        let (info, root, size) =
            parse_trailer_refs(b"xref\n0 1\ntrailer<</Size 5/Root 1 0 R/Prev 116>>\nstartxref\n")
                .expect("an incremental-update trailer must parse");
        assert!(info.is_none(), "no /Info key means no Info object yet");
        assert_eq!(root.object_num, 1);
        assert_eq!(size, 5);

        let (info, _, _) =
            parse_trailer_refs(b"xref\n0 5\ntrailer<</Size 5/Root 1 0 R/Info 4 0 R>>\nstartxref\n")
                .expect("a classic trailer must parse");
        assert_eq!(info.expect("has /Info").object_num, 4);
    }

    /// A compact dictionary (no space before the next key) must not make a
    /// value scavenge the following key's number.
    #[test]
    fn test_parse_dict_handles_compact_dictionaries() {
        let compact = "<</Size 5/Root 1 0 R/Prev 116>>";
        assert_eq!(parse_dict_integer(compact, "/Size"), Some(5));
        assert_eq!(parse_dict_integer(compact, "/Prev"), Some(116));
        let root = parse_dict_object_ref(compact, "/Root").expect("has /Root");
        assert_eq!((root.object_num, root.generation), (1, 0));
        // /Type is a name, not a reference or an integer
        assert_eq!(
            parse_dict_integer("<</Type/Catalog/Size 9>>", "/Type"),
            None
        );
        assert!(parse_dict_object_ref("<</Type/Catalog/Root 1 0 R>>", "/Type").is_none());
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
