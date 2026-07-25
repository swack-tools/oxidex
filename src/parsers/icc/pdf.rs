//! PDF ICC Profile extraction
//!
//! This module handles extracting ICC profiles embedded in PDF files,
//! including decompression of FlateDecode streams.

use super::binary::find_bytes;
use crate::core::FileReader;
use crate::error::{ExifToolError, Result};
use flate2::read::ZlibDecoder;
use std::collections::HashMap;
use std::io::Read;

/// Extracts the raw ICC profile data from a PDF file
pub fn extract_icc_from_pdf(reader: &dyn FileReader) -> Result<Vec<u8>> {
    let file_size = reader.size();

    // Read the last 1024 bytes to find trailer
    let tail_size = std::cmp::min(1024, file_size as usize);
    let tail_offset = file_size.saturating_sub(tail_size as u64);
    let tail_data = reader.read(tail_offset, tail_size)?;

    let xref_offset = find_xref_offset(tail_data)?;

    // Read xref table and trailer region
    let xref_size = std::cmp::min(8192, file_size.saturating_sub(xref_offset) as usize);
    let xref_data = reader.read(xref_offset, xref_size)?;

    let xref_map = parse_xref_table(xref_data)?;
    let root_ref = find_root_reference(xref_data)?;

    let root_offset = *xref_map.get(&root_ref).ok_or_else(|| {
        ExifToolError::parse_error(format!("Root object {} not found in xref table", root_ref))
    })?;

    // Read Root/Catalog object
    let root_size = std::cmp::min(8192, file_size.saturating_sub(root_offset) as usize);
    let root_data = reader.read(root_offset, root_size)?;

    // Find ICC profile reference
    let output_profile_ref = find_output_profile_reference(root_data)
        .or_else(|_| find_icc_based_reference(reader, &xref_map))?;

    let profile_offset = *xref_map.get(&output_profile_ref).ok_or_else(|| {
        ExifToolError::parse_error(format!(
            "ICC profile object {} not found in xref table",
            output_profile_ref
        ))
    })?;

    // Read ICC profile stream object
    let profile_size = std::cmp::min(131072, file_size.saturating_sub(profile_offset) as usize);
    let profile_data = reader.read(profile_offset, profile_size)?;

    extract_and_decompress_stream(profile_data)
}

/// Finds the /OutputIntents -> /DestOutputProfile reference in the Catalog
fn find_output_profile_reference(root_data: &[u8]) -> Result<u32> {
    let output_intents_pos = find_bytes(root_data, b"/OutputIntents").ok_or_else(|| {
        ExifToolError::parse_error("No /OutputIntents found in Catalog (no ICC profile)")
    })?;

    let after_output = &root_data[output_intents_pos..];
    let dest_profile_pos = find_bytes(after_output, b"/DestOutputProfile").ok_or_else(|| {
        ExifToolError::parse_error("No /DestOutputProfile found in OutputIntents")
    })?;

    let after_dest = &after_output[dest_profile_pos + 18..];
    let after_dest_str =
        std::str::from_utf8(&after_dest[..std::cmp::min(100, after_dest.len())]).unwrap_or("");

    parse_object_ref(after_dest_str)
}

/// Finds ICC profile reference via /ICCBased in the PDF
fn find_icc_based_reference(reader: &dyn FileReader, _xref_map: &HashMap<u32, u64>) -> Result<u32> {
    let file_size = reader.size();
    let search_size = std::cmp::min(65536, file_size as usize);
    let search_data = reader.read(0, search_size)?;

    let icc_based_pos = find_bytes(search_data, b"/ICCBased").ok_or_else(|| {
        ExifToolError::parse_error("No /ICCBased reference found (no ICC profile)")
    })?;

    let after_icc = &search_data[icc_based_pos + 9..];
    let after_icc_str = String::from_utf8_lossy(&after_icc[..std::cmp::min(100, after_icc.len())]);

    parse_object_ref(&after_icc_str)
}

/// Parses an object reference like "5 0 R" and returns the object number
fn parse_object_ref(s: &str) -> Result<u32> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.is_empty() {
        return Err(ExifToolError::parse_error("No object reference found"));
    }

    if let Some(idx) = parts.iter().position(|&p| p == "R" || p.starts_with('R')) {
        if idx >= 2 {
            return parts[idx - 2]
                .parse::<u32>()
                .map_err(|_| ExifToolError::parse_error("Invalid object number in reference"));
        } else if idx >= 1 {
            return parts[0]
                .parse::<u32>()
                .map_err(|_| ExifToolError::parse_error("Invalid object number in reference"));
        }
    }

    for part in &parts {
        if let Ok(num) = part.parse::<u32>() {
            return Ok(num);
        }
    }

    Err(ExifToolError::parse_error(
        "Invalid object reference format",
    ))
}

/// Extracts and decompresses a stream from a PDF object
fn extract_and_decompress_stream(obj_data: &[u8]) -> Result<Vec<u8>> {
    let stream_start_pos = find_bytes(obj_data, b"stream")
        .ok_or_else(|| ExifToolError::parse_error("No stream marker found in object"))?;

    let mut stream_offset = stream_start_pos + 6;

    // Skip newline after "stream"
    if obj_data.len() > stream_offset {
        if obj_data[stream_offset] == b'\r' {
            stream_offset += 1;
        }
        if obj_data.len() > stream_offset && obj_data[stream_offset] == b'\n' {
            stream_offset += 1;
        }
    }

    let endstream_pos = find_bytes(obj_data, b"endstream").ok_or_else(|| {
        ExifToolError::parse_error("No endstream marker found - stream may be truncated")
    })?;

    let stream_data = &obj_data[stream_offset..endstream_pos];

    // Check if stream is compressed
    let header_data = &obj_data[..stream_start_pos];
    let is_compressed = find_bytes(header_data, b"/FlateDecode").is_some()
        || find_bytes(header_data, b"/Fl").is_some();

    if is_compressed {
        let mut decoder = ZlibDecoder::new(stream_data);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).map_err(|e| {
            ExifToolError::parse_error(format!("Failed to decompress FlateDecode stream: {}", e))
        })?;
        Ok(decompressed)
    } else {
        Ok(stream_data.to_vec())
    }
}

/// Finds the /Root reference from trailer
fn find_root_reference(xref_data: &[u8]) -> Result<u32> {
    let xref_str = std::str::from_utf8(xref_data)
        .map_err(|_| ExifToolError::parse_error("xref data contains invalid UTF-8"))?;

    let trailer_pos = xref_str
        .find("trailer")
        .ok_or_else(|| ExifToolError::parse_error("trailer not found in PDF"))?;

    let after_trailer = &xref_str[trailer_pos..];
    let root_pos = after_trailer
        .find("/Root")
        .ok_or_else(|| ExifToolError::parse_error("No /Root reference in trailer"))?;

    let after_root = &after_trailer[root_pos + 5..];
    parse_object_ref(after_root)
}

/// Finds startxref offset from PDF tail
fn find_xref_offset(tail_data: &[u8]) -> Result<u64> {
    let tail_str = std::str::from_utf8(tail_data)
        .map_err(|_| ExifToolError::parse_error("PDF tail contains invalid UTF-8"))?;

    let startxref_pos = tail_str
        .rfind("startxref")
        .ok_or_else(|| ExifToolError::parse_error("startxref not found in PDF"))?;

    let after_keyword = &tail_str[startxref_pos + 9..];
    let num_str: String = after_keyword
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();

    num_str
        .parse::<u64>()
        .map_err(|_| ExifToolError::parse_error("Invalid xref offset after startxref"))
}

/// Parses xref table and builds object offset map
fn parse_xref_table(xref_data: &[u8]) -> Result<HashMap<u32, u64>> {
    let xref_str = std::str::from_utf8(xref_data)
        .map_err(|_| ExifToolError::parse_error("xref table contains invalid UTF-8"))?;

    let xref_pos = xref_str
        .find("xref")
        .ok_or_else(|| ExifToolError::parse_error("xref table not found"))?;

    let after_xref = &xref_str[xref_pos + 4..];
    let mut xref_map = HashMap::new();
    let lines: Vec<&str> = after_xref.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        if line.starts_with("trailer") {
            break;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2
            && let (Ok(start_obj), Ok(count)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>())
        {
            for j in 0..count {
                i += 1;
                if i >= lines.len() {
                    break;
                }

                let entry_line = lines[i].trim();
                let entry_parts: Vec<&str> = entry_line.split_whitespace().collect();

                if entry_parts.len() >= 3
                    && let Ok(offset) = entry_parts[0].parse::<u64>()
                    && entry_parts[2] == "n"
                {
                    let obj_num = start_obj + j;
                    xref_map.insert(obj_num, offset);
                }
            }
        }

        i += 1;
    }

    if xref_map.is_empty() {
        return Err(ExifToolError::parse_error("No valid xref entries found"));
    }

    Ok(xref_map)
}
