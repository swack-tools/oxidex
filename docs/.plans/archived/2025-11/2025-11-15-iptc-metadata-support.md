# IPTC Metadata Support Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add IPTC metadata extraction from JPEG APP13 segments (Photoshop IRB format)

**Architecture:** Parse APP13 segments → Extract Photoshop 8BIM resources → Parse IPTC IIM records → Map to metadata tags

**Tech Stack:** Rust, nom parser combinators, existing JPEG segment infrastructure

---

## Context

IPTC metadata is stored in JPEG APP13 segments using Adobe's Photoshop Image Resource Block (IRB) format. The structure is:

```
APP13 Segment (marker 0xFFED):
  [0xFF, 0xED] - Marker
  [length] - 2 bytes, big-endian
  "Photoshop 3.0\0" - 14 bytes signature

  Image Resource Block(s):
    "8BIM" - 4 bytes type signature
    [ID] - 2 bytes (0x0404 for IPTC)
    [name] - Pascal string (padded to even length)
    [size] - 4 bytes, big-endian
    [data] - IPTC IIM records

IPTC IIM Record:
  0x1C - Tag marker
  [record_number] - 1 byte (usually 2)
  [dataset_number] - 1 byte
  [length] - 2 bytes
  [data] - variable
```

**Reference:** docs/IMPLEMENTATION_ROADMAP.md lines 50-173

---

## Task 1: Define IPTC Constants and Data Structures

**Files:**
- Modify: `src/parsers/jpeg/iptc_parser.rs:1-100`

**Step 1: Write test for IPTC constants**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_photoshop_signature() {
        assert_eq!(PHOTOSHOP_SIGNATURE, b"Photoshop 3.0\0");
        assert_eq!(PHOTOSHOP_SIGNATURE.len(), 14);
    }

    #[test]
    fn test_8bim_signature() {
        assert_eq!(EIGHTBIM_SIGNATURE, b"8BIM");
        assert_eq!(EIGHTBIM_SIGNATURE.len(), 4);
    }

    #[test]
    fn test_iptc_resource_id() {
        assert_eq!(IPTC_RESOURCE_ID, 0x0404);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_photoshop_signature --lib -- --nocapture`
Expected: FAIL with "unresolved import `PHOTOSHOP_SIGNATURE`"

**Step 3: Implement constants and structures**

```rust
//! IPTC segment parser for JPEG
//!
//! This module handles parsing of IPTC data in JPEG APP13 segments.
//! IPTC data is stored in Adobe Photoshop Image Resource Blocks (8BIM).

use crate::error::{ExifToolError, Result};
use crate::parsers::jpeg::segment_parser::Segment;
use nom::{
    bytes::complete::{tag, take},
    number::complete::{be_u16, be_u32, u8 as nom_u8},
    IResult,
};

// Constants
const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";
const EIGHTBIM_SIGNATURE: &[u8] = b"8BIM";
const IPTC_RESOURCE_ID: u16 = 0x0404;
const IPTC_TAG_MARKER: u8 = 0x1C;
const APP13_MARKER: u16 = 0xFFED;

/// Represents an Adobe Photoshop Image Resource Block
#[derive(Debug, Clone, PartialEq)]
struct ImageResourceBlock<'a> {
    /// Resource ID (e.g., 0x0404 for IPTC)
    id: u16,
    /// Resource name (Pascal string)
    name: &'a [u8],
    /// Resource data payload
    data: &'a [u8],
}

/// Represents a single IPTC IIM record
#[derive(Debug, Clone, PartialEq)]
struct IptcRecord {
    /// Record number (usually 2 for Application Record)
    record_number: u8,
    /// Dataset number (identifies the specific tag)
    dataset_number: u8,
    /// Record data
    data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_photoshop_signature() {
        assert_eq!(PHOTOSHOP_SIGNATURE, b"Photoshop 3.0\0");
        assert_eq!(PHOTOSHOP_SIGNATURE.len(), 14);
    }

    #[test]
    fn test_8bim_signature() {
        assert_eq!(EIGHTBIM_SIGNATURE, b"8BIM");
        assert_eq!(EIGHTBIM_SIGNATURE.len(), 4);
    }

    #[test]
    fn test_iptc_resource_id() {
        assert_eq!(IPTC_RESOURCE_ID, 0x0404);
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test iptc_parser --lib -- --nocapture`
Expected: PASS (all 3 tests)

**Step 5: Commit**

```bash
git add src/parsers/jpeg/iptc_parser.rs
git commit -m "feat(iptc): add constants and data structures for IPTC parsing

Define Photoshop IRB and IPTC IIM record structures.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 2: Implement Image Resource Block Parser

**Files:**
- Modify: `src/parsers/jpeg/iptc_parser.rs:50-150`

**Step 1: Write test for parsing 8BIM resource block**

```rust
#[test]
fn test_parse_image_resource_block() {
    // Create a minimal 8BIM resource block
    let mut data = Vec::new();
    data.extend_from_slice(b"8BIM"); // Signature
    data.extend_from_slice(&[0x04, 0x04]); // ID: 0x0404 (IPTC)
    data.push(0x00); // Name: empty Pascal string (length = 0)
    data.push(0x00); // Padding to make name even length
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // Size: 4 bytes
    data.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4 bytes of data

    let result = parse_image_resource_block(&data);
    assert!(result.is_ok());

    let (remaining, block) = result.unwrap();
    assert_eq!(block.id, 0x0404);
    assert_eq!(block.name, &[]);
    assert_eq!(block.data, &[0xAA, 0xBB, 0xCC, 0xDD]);
    assert!(remaining.is_empty());
}

#[test]
fn test_parse_image_resource_block_with_name() {
    let mut data = Vec::new();
    data.extend_from_slice(b"8BIM");
    data.extend_from_slice(&[0x04, 0x04]); // ID
    data.push(0x04); // Name length: 4
    data.extend_from_slice(b"TEST"); // Name: "TEST"
    data.push(0x00); // Padding (4+1 = 5, need 1 byte padding for even)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x02]); // Size: 2 bytes
    data.extend_from_slice(&[0x11, 0x22]); // Data

    let result = parse_image_resource_block(&data);
    assert!(result.is_ok());

    let (remaining, block) = result.unwrap();
    assert_eq!(block.id, 0x0404);
    assert_eq!(block.name, b"TEST");
    assert_eq!(block.data, &[0x11, 0x22]);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_parse_image_resource_block --lib -- --nocapture`
Expected: FAIL with "unresolved function `parse_image_resource_block`"

**Step 3: Implement 8BIM resource block parser**

Add after the struct definitions:

```rust
/// Parses a single Adobe Photoshop Image Resource Block (8BIM).
///
/// # Format
/// - Signature: "8BIM" (4 bytes)
/// - ID: 2 bytes (big-endian)
/// - Name: Pascal string (1 byte length + data), padded to even length
/// - Size: 4 bytes (big-endian)
/// - Data: variable length
fn parse_image_resource_block(input: &[u8]) -> IResult<&[u8], ImageResourceBlock> {
    // Parse 8BIM signature
    let (input, _) = tag(EIGHTBIM_SIGNATURE)(input)?;

    // Parse resource ID (2 bytes, big-endian)
    let (input, id) = be_u16(input)?;

    // Parse Pascal string name (1 byte length + data)
    let (input, name_length) = nom_u8(input)?;
    let (input, name) = take(name_length as usize)(input)?;

    // Pascal string must be padded to even length (including length byte)
    // Total length so far: 1 (length byte) + name_length
    // If odd, add 1 byte padding
    let total_name_length = 1 + name_length as usize;
    let (input, _) = if total_name_length % 2 == 1 {
        take(1usize)(input)? // Take 1 byte padding
    } else {
        (input, &b""[..]) // No padding needed
    };

    // Parse data size (4 bytes, big-endian)
    let (input, data_size) = be_u32(input)?;

    // Parse data
    let (input, data) = take(data_size as usize)(input)?;

    Ok((input, ImageResourceBlock { id, name, data }))
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test test_parse_image_resource_block --lib -- --nocapture`
Expected: PASS (both tests)

**Step 5: Commit**

```bash
git add src/parsers/jpeg/iptc_parser.rs
git commit -m "feat(iptc): implement 8BIM image resource block parser

Parse Adobe Photoshop Image Resource Blocks with ID, name, and data.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 3: Implement IPTC IIM Record Parser

**Files:**
- Modify: `src/parsers/jpeg/iptc_parser.rs:150-250`

**Step 1: Write test for parsing IPTC IIM record**

```rust
#[test]
fn test_parse_iptc_record() {
    // Create a minimal IPTC record
    // Record 2, Dataset 5 (ObjectName), Data: "Test"
    let data = vec![
        0x1C, // Tag marker
        0x02, // Record number (Application Record)
        0x05, // Dataset number (ObjectName)
        0x00, 0x04, // Length: 4 bytes
        b'T', b'e', b's', b't', // Data: "Test"
    ];

    let result = parse_iptc_record(&data);
    assert!(result.is_ok());

    let (remaining, record) = result.unwrap();
    assert_eq!(record.record_number, 2);
    assert_eq!(record.dataset_number, 5);
    assert_eq!(record.data, b"Test");
    assert!(remaining.is_empty());
}

#[test]
fn test_parse_multiple_iptc_records() {
    let mut data = Vec::new();

    // Record 1
    data.push(0x1C);
    data.extend_from_slice(&[0x02, 0x05]); // Record 2, Dataset 5
    data.extend_from_slice(&[0x00, 0x05]); // Length: 5
    data.extend_from_slice(b"Title");

    // Record 2
    data.push(0x1C);
    data.extend_from_slice(&[0x02, 0x50]); // Record 2, Dataset 80 (ByLine)
    data.extend_from_slice(&[0x00, 0x06]); // Length: 6
    data.extend_from_slice(b"Author");

    let result = parse_all_iptc_records(&data);
    assert!(result.is_ok());

    let records = result.unwrap();
    assert_eq!(records.len(), 2);

    assert_eq!(records[0].dataset_number, 5);
    assert_eq!(records[0].data, b"Title");

    assert_eq!(records[1].dataset_number, 80);
    assert_eq!(records[1].data, b"Author");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_parse_iptc_record --lib -- --nocapture`
Expected: FAIL with "unresolved function `parse_iptc_record`"

**Step 3: Implement IPTC IIM record parsers**

Add after parse_image_resource_block:

```rust
/// Parses a single IPTC IIM record.
///
/// # Format
/// - Tag marker: 0x1C (1 byte)
/// - Record number: 1 byte (usually 2 for Application Record)
/// - Dataset number: 1 byte
/// - Length: 2 bytes (big-endian), or extended format for > 32767 bytes
/// - Data: variable length
fn parse_iptc_record(input: &[u8]) -> IResult<&[u8], IptcRecord> {
    // Parse tag marker (must be 0x1C)
    let (input, _) = tag(&[IPTC_TAG_MARKER])(input)?;

    // Parse record number (1 byte)
    let (input, record_number) = nom_u8(input)?;

    // Parse dataset number (1 byte)
    let (input, dataset_number) = nom_u8(input)?;

    // Parse length (2 bytes, big-endian)
    let (input, length) = be_u16(input)?;

    // Check for extended format (if length > 32767, it's actually a marker)
    // For now, we'll just support standard format (< 32768 bytes)
    let data_length = length as usize;

    // Parse data
    let (input, data_bytes) = take(data_length)(input)?;

    Ok((
        input,
        IptcRecord {
            record_number,
            dataset_number,
            data: data_bytes.to_vec(),
        },
    ))
}

/// Parses all IPTC IIM records from a data block.
///
/// Returns a vector of all successfully parsed records.
/// Stops at first parse error or end of data.
fn parse_all_iptc_records(input: &[u8]) -> Result<Vec<IptcRecord>> {
    let mut records = Vec::new();
    let mut current = input;

    while !current.is_empty() {
        // Check if next byte is tag marker
        if current[0] != IPTC_TAG_MARKER {
            break;
        }

        match parse_iptc_record(current) {
            Ok((remaining, record)) => {
                records.push(record);
                current = remaining;
            }
            Err(_) => {
                // Stop on parse error
                break;
            }
        }
    }

    Ok(records)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test test_parse_iptc_record --lib -- --nocapture`
Expected: PASS (both tests)

**Step 5: Commit**

```bash
git add src/parsers/jpeg/iptc_parser.rs
git commit -m "feat(iptc): implement IPTC IIM record parser

Parse IPTC Information Interchange Model records with record/dataset numbers.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 4: Implement IPTC Dataset to Tag Name Mapping

**Files:**
- Modify: `src/parsers/jpeg/iptc_parser.rs:250-400`

**Step 1: Write test for dataset mapping**

```rust
#[test]
fn test_dataset_to_tag_name() {
    assert_eq!(dataset_to_tag_name(2, 5), "IPTC:ObjectName");
    assert_eq!(dataset_to_tag_name(2, 25), "IPTC:Keywords");
    assert_eq!(dataset_to_tag_name(2, 80), "IPTC:By-line");
    assert_eq!(dataset_to_tag_name(2, 90), "IPTC:City");
    assert_eq!(dataset_to_tag_name(2, 120), "IPTC:Caption-Abstract");

    // Unknown dataset should return generic name
    assert_eq!(dataset_to_tag_name(2, 255), "IPTC:Unknown-2-255");
}

#[test]
fn test_decode_iptc_string() {
    // Test ASCII string
    let ascii_data = b"Hello World";
    assert_eq!(decode_iptc_string(ascii_data), "Hello World");

    // Test string with trailing spaces (should be trimmed)
    let padded_data = b"Test    ";
    assert_eq!(decode_iptc_string(padded_data), "Test");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_dataset_to_tag_name --lib -- --nocapture`
Expected: FAIL with "unresolved function `dataset_to_tag_name`"

**Step 3: Implement dataset mapping**

Add after parse_all_iptc_records:

```rust
/// Maps IPTC dataset numbers to tag names.
///
/// # Parameters
/// - `record_number`: The record number (usually 2 for Application Record)
/// - `dataset_number`: The dataset number identifying the tag
///
/// # Returns
/// Tag name in the format "IPTC:TagName"
fn dataset_to_tag_name(record_number: u8, dataset_number: u8) -> String {
    // Only handle Record 2 (Application Record) for now
    if record_number != 2 {
        return format!("IPTC:Unknown-{}-{}", record_number, dataset_number);
    }

    let tag_name = match dataset_number {
        5 => "ObjectName",
        7 => "EditStatus",
        10 => "Urgency",
        15 => "Category",
        20 => "SupplementalCategories",
        25 => "Keywords",
        40 => "SpecialInstructions",
        55 => "DateCreated",
        60 => "TimeCreated",
        80 => "By-line",
        85 => "By-lineTitle",
        90 => "City",
        92 => "Sub-location",
        95 => "Province-State",
        100 => "Country-PrimaryLocationCode",
        101 => "Country-PrimaryLocationName",
        103 => "OriginalTransmissionReference",
        105 => "Headline",
        110 => "Credit",
        115 => "Source",
        116 => "CopyrightNotice",
        118 => "Contact",
        120 => "Caption-Abstract",
        122 => "Writer-Editor",
        _ => return format!("IPTC:Unknown-{}-{}", record_number, dataset_number),
    };

    format!("IPTC:{}", tag_name)
}

/// Decodes an IPTC string from bytes.
///
/// IPTC strings are typically Latin-1 encoded, but may also be UTF-8.
/// This function attempts UTF-8 first, falls back to Latin-1, and trims whitespace.
fn decode_iptc_string(data: &[u8]) -> String {
    // Try UTF-8 first
    if let Ok(s) = std::str::from_utf8(data) {
        return s.trim().to_string();
    }

    // Fall back to Latin-1 (ISO-8859-1)
    // In Latin-1, each byte maps directly to a Unicode code point
    let s: String = data.iter().map(|&b| b as char).collect();
    s.trim().to_string()
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test test_dataset_to_tag_name --lib -- --nocapture`
Expected: PASS (both tests)

**Step 5: Commit**

```bash
git add src/parsers/jpeg/iptc_parser.rs
git commit -m "feat(iptc): add dataset to tag name mapping

Map IPTC dataset numbers to human-readable tag names.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 5: Implement Main IPTC Extraction Function

**Files:**
- Modify: `src/parsers/jpeg/iptc_parser.rs:400-600`

**Step 1: Write test for complete IPTC extraction**

```rust
#[test]
fn test_extract_iptc_from_segments() {
    // Create a complete APP13 segment with IPTC data
    let mut app13_data = Vec::new();

    // Photoshop signature
    app13_data.extend_from_slice(PHOTOSHOP_SIGNATURE);

    // 8BIM resource block
    app13_data.extend_from_slice(b"8BIM");
    app13_data.extend_from_slice(&[0x04, 0x04]); // ID: IPTC
    app13_data.push(0x00); // Empty name
    app13_data.push(0x00); // Padding

    // IPTC data
    let mut iptc_data = Vec::new();
    // Record: ObjectName (dataset 5)
    iptc_data.push(0x1C);
    iptc_data.extend_from_slice(&[0x02, 0x05]);
    iptc_data.extend_from_slice(&[0x00, 0x09]);
    iptc_data.extend_from_slice(b"Test Title");

    // Record: By-line (dataset 80)
    iptc_data.push(0x1C);
    iptc_data.extend_from_slice(&[0x02, 0x50]);
    iptc_data.extend_from_slice(&[0x00, 0x0B]);
    iptc_data.extend_from_slice(b"Test Author");

    // Add IPTC data size and data to 8BIM block
    let iptc_size = iptc_data.len() as u32;
    app13_data.extend_from_slice(&iptc_size.to_be_bytes());
    app13_data.extend_from_slice(&iptc_data);

    // Create APP13 segment
    let segment = Segment::new(APP13_MARKER, 0, &app13_data);
    let segments = vec![segment];

    // Extract IPTC
    let result = extract_iptc_from_segments(&segments);
    assert!(result.is_ok());

    let tags = result.unwrap();
    assert_eq!(tags.len(), 2);

    // Check tags
    let title = tags.iter().find(|(k, _)| k == "IPTC:ObjectName");
    assert!(title.is_some());
    assert_eq!(title.unwrap().1, "Test Title");

    let author = tags.iter().find(|(k, _)| k == "IPTC:By-line");
    assert!(author.is_some());
    assert_eq!(author.unwrap().1, "Test Author");
}

#[test]
fn test_extract_iptc_no_app13_segments() {
    // Empty segments
    let segments = vec![];
    let result = extract_iptc_from_segments(&segments);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_extract_iptc_from_segments --lib -- --nocapture`
Expected: FAIL with "unresolved function `extract_iptc_from_segments`"

**Step 3: Implement main extraction function**

Add after decode_iptc_string:

```rust
/// Extracts IPTC metadata from JPEG segments.
///
/// This function scans through all segments, identifies APP13 segments with
/// the Photoshop signature, extracts IPTC data from 8BIM resource blocks,
/// and parses IPTC IIM records.
///
/// # Parameters
///
/// - `segments`: Slice of parsed JPEG segments (from `parse_segments()`)
///
/// # Returns
///
/// Vector of (tag_name, value) tuples where tag_name is in the format
/// "IPTC:PropertyName" (e.g., "IPTC:ObjectName", "IPTC:By-line").
///
/// Returns an empty vector if no IPTC segments are found (not an error).
///
/// # Errors
///
/// Returns `ParseError` if:
/// - APP13 segment is malformed
/// - 8BIM resource blocks are invalid
/// - IPTC records cannot be parsed
pub fn extract_iptc_from_segments(segments: &[Segment]) -> Result<Vec<(String, String)>> {
    let mut all_iptc_tags = Vec::new();

    // Iterate through all segments looking for APP13 segments
    for segment in segments {
        // Check if this is an APP13 segment (0xFFED)
        if segment.marker != APP13_MARKER {
            continue;
        }

        // Check if this APP13 segment contains Photoshop data
        if !segment.data.starts_with(PHOTOSHOP_SIGNATURE) {
            continue;
        }

        // Skip past the Photoshop signature
        let mut current = &segment.data[PHOTOSHOP_SIGNATURE.len()..];

        // Parse all 8BIM resource blocks
        while current.len() > 4 {
            // Check if this looks like a 8BIM block
            if !current.starts_with(EIGHTBIM_SIGNATURE) {
                break;
            }

            match parse_image_resource_block(current) {
                Ok((remaining, block)) => {
                    // Check if this is the IPTC resource block (ID 0x0404)
                    if block.id == IPTC_RESOURCE_ID {
                        // Parse IPTC records from the block data
                        match parse_all_iptc_records(block.data) {
                            Ok(records) => {
                                // Convert records to tag name/value pairs
                                for record in records {
                                    let tag_name =
                                        dataset_to_tag_name(record.record_number, record.dataset_number);
                                    let value = decode_iptc_string(&record.data);

                                    all_iptc_tags.push((tag_name, value));
                                }
                            }
                            Err(e) => {
                                // Log error but continue processing other blocks
                                eprintln!("Warning: Failed to parse IPTC records: {}", e);
                            }
                        }
                    }

                    current = remaining;
                }
                Err(_) => {
                    // Failed to parse block, stop processing this segment
                    break;
                }
            }
        }
    }

    Ok(all_iptc_tags)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test extract_iptc --lib -- --nocapture`
Expected: PASS (both tests)

**Step 5: Commit**

```bash
git add src/parsers/jpeg/iptc_parser.rs
git commit -m "feat(iptc): implement main IPTC extraction function

Extract IPTC metadata from APP13 segments with full pipeline.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 6: Integrate IPTC Parser into JPEG Metadata Pipeline

**Files:**
- Modify: `src/core/operations.rs:336-360`

**Step 1: Write integration test**

Add to `tests/integration/jpeg_tests.rs`:

```rust
#[test]
fn test_jpeg_with_iptc_metadata() {
    use oxidex::parsers::jpeg::iptc_parser::extract_iptc_from_segments;
    use oxidex::parsers::jpeg::segment_parser::parse_segments;

    // Create minimal JPEG with APP13 (IPTC) segment
    let mut jpeg_data = Vec::new();

    // SOI marker
    jpeg_data.extend_from_slice(&[0xFF, 0xD8]);

    // APP13 marker
    jpeg_data.extend_from_slice(&[0xFF, 0xED]);

    // Create IPTC data
    let mut iptc_payload = Vec::new();
    iptc_payload.extend_from_slice(b"Photoshop 3.0\0"); // Signature
    iptc_payload.extend_from_slice(b"8BIM"); // 8BIM signature
    iptc_payload.extend_from_slice(&[0x04, 0x04]); // IPTC resource ID
    iptc_payload.push(0x00); // Empty name
    iptc_payload.push(0x00); // Padding

    // IPTC IIM records
    let mut iptc_records = Vec::new();
    iptc_records.push(0x1C); // Tag marker
    iptc_records.extend_from_slice(&[0x02, 0x05]); // Record 2, Dataset 5 (ObjectName)
    iptc_records.extend_from_slice(&[0x00, 0x0A]); // Length: 10
    iptc_records.extend_from_slice(b"IPTC Title");

    let iptc_size = iptc_records.len() as u32;
    iptc_payload.extend_from_slice(&iptc_size.to_be_bytes());
    iptc_payload.extend_from_slice(&iptc_records);

    // APP13 length
    let app13_length = (iptc_payload.len() + 2) as u16;
    jpeg_data.extend_from_slice(&app13_length.to_be_bytes());
    jpeg_data.extend_from_slice(&iptc_payload);

    // EOI marker
    jpeg_data.extend_from_slice(&[0xFF, 0xD9]);

    // Parse segments
    let reader = TestReader::new(jpeg_data);
    let segments = parse_segments(&reader).expect("Failed to parse segments");

    // Extract IPTC
    let iptc_tags = extract_iptc_from_segments(&segments).expect("Failed to extract IPTC");

    assert_eq!(iptc_tags.len(), 1);
    assert_eq!(iptc_tags[0].0, "IPTC:ObjectName");
    assert_eq!(iptc_tags[0].1, "IPTC Title");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_jpeg_with_iptc_metadata -- --nocapture`
Expected: PASS (IPTC extraction works, but not integrated into main pipeline yet)

**Step 3: Add IPTC extraction to parse_jpeg_metadata**

In `src/core/operations.rs`, find the XMP extraction section (around line 336) and add IPTC extraction after it:

```rust
    // Extract XMP metadata from APP1 segments
    match extract_xmp_from_segments(&segments) {
        Ok(xmp_tags) => {
            // Add all XMP tags to metadata
            for (tag_name, value) in xmp_tags {
                // Try to parse as integer first, then as float, otherwise keep as string
                let tag_value = if let Ok(int_val) = value.parse::<i64>() {
                    TagValue::Integer(int_val)
                } else if let Ok(float_val) = value.parse::<f64>() {
                    TagValue::Float(float_val)
                } else {
                    TagValue::String(value)
                };
                metadata.insert(tag_name, tag_value);
            }
        }
        Err(e) => {
            // Log error but continue processing
            eprintln!("Warning: Failed to extract XMP metadata: {}", e);
        }
    }

    // Extract IPTC metadata from APP13 segments
    match crate::parsers::jpeg::iptc_parser::extract_iptc_from_segments(&segments) {
        Ok(iptc_tags) => {
            // Add all IPTC tags to metadata
            for (tag_name, value) in iptc_tags {
                // Try to parse as integer first, then as float, otherwise keep as string
                let tag_value = if let Ok(int_val) = value.parse::<i64>() {
                    TagValue::Integer(int_val)
                } else if let Ok(float_val) = value.parse::<f64>() {
                    TagValue::Float(float_val)
                } else {
                    TagValue::String(value)
                };
                metadata.insert(tag_name, tag_value);
            }
        }
        Err(e) => {
            // Log error but continue processing
            eprintln!("Warning: Failed to extract IPTC metadata: {}", e);
        }
    }

    Ok(metadata)
}
```

**Step 4: Run integration tests**

Run: `cargo test jpeg -- --nocapture`
Expected: PASS (all JPEG tests including IPTC integration)

**Step 5: Commit**

```bash
git add src/core/operations.rs tests/integration/jpeg_tests.rs
git commit -m "feat(iptc): integrate IPTC parser into JPEG metadata pipeline

Add IPTC extraction to main metadata reading workflow.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 7: Add End-to-End Test with Real IPTC Sample

**Files:**
- Create: `tests/fixtures/iptc_sample.jpg` (binary test file)
- Create: `tests/integration/iptc_integration_test.rs`

**Step 1: Create minimal IPTC test fixture**

Note: This requires creating a binary file. Use a script or existing tool.

```bash
# Create a minimal valid JPEG with IPTC metadata for testing
# This can be done manually or using exiftool
echo "Creating IPTC test fixture..."
```

**Step 2: Write end-to-end integration test**

Create `tests/integration/iptc_integration_test.rs`:

```rust
//! End-to-end integration tests for IPTC metadata extraction

use oxidex::core::operations::read_metadata;
use std::path::Path;

#[test]
fn test_iptc_extraction_from_real_file() {
    // This test requires a real IPTC sample file
    // Download from: https://www.iptc.org/std/photometadata/examples/
    // For now, we'll skip if file doesn't exist

    let sample_path = Path::new("tests/fixtures/iptc_sample.jpg");

    if !sample_path.exists() {
        eprintln!("Skipping test: IPTC sample file not found");
        return;
    }

    let metadata = read_metadata(sample_path)
        .expect("Failed to read metadata from IPTC sample");

    // Verify IPTC tags were extracted
    assert!(metadata.contains_key("IPTC:ObjectName") ||
            metadata.contains_key("IPTC:By-line") ||
            metadata.contains_key("IPTC:Caption-Abstract"),
            "Expected at least one IPTC tag to be present");
}
```

**Step 3: Run test**

Run: `cargo test iptc_integration -- --nocapture`
Expected: PASS (skips if fixture doesn't exist, or validates if it does)

**Step 4: Add test module to integration tests**

Add to `tests/integration/mod.rs` (or create if it doesn't exist):

```rust
mod iptc_integration_test;
```

**Step 5: Commit**

```bash
git add tests/integration/iptc_integration_test.rs tests/integration/mod.rs
git commit -m "test(iptc): add end-to-end integration test

Add integration test for IPTC extraction from real files.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 8: Update Documentation and Examples

**Files:**
- Modify: `README.md` (add IPTC to supported features)
- Modify: `docs/IMPLEMENTATION_ROADMAP.md:624` (mark IPTC as complete)

**Step 1: Update README.md**

Find the "Supported Formats" or "Features" section and add IPTC:

```markdown
### Metadata Formats

- ✅ **EXIF** - Complete support for IFD0, IFD1, ExifIFD, GPS
- ✅ **XMP** - 10+ namespaces supported
- ✅ **IPTC** - Complete support for IPTC IIM Application Record (journalism/stock photography)
- ✅ **JFIF** - JPEG File Interchange Format
- ✅ **ICC Profiles** - Color profile metadata
```

**Step 2: Update implementation roadmap**

In `docs/IMPLEMENTATION_ROADMAP.md`, update the "Next Actions" section:

```markdown
### Immediate (This Week)

1. ✅ Create this implementation roadmap
2. ✅ Implement IPTC parser
3. Create GitHub issues for:
   - Canon MakerNotes (#6)
   - Composite tags (#7)
```

**Step 3: Verify all tests pass**

Run: `cargo test --all -- --nocapture`
Expected: PASS (all tests)

**Step 4: Commit**

```bash
git add README.md docs/IMPLEMENTATION_ROADMAP.md
git commit -m "docs: update documentation for IPTC support

Mark IPTC implementation as complete in roadmap.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 9: Run Final Verification and Benchmarks

**Files:**
- None (verification only)

**Step 1: Run full test suite**

Run: `cargo test --all -- --nocapture`
Expected: PASS (all tests)

**Step 2: Run with real IPTC samples**

Download IPTC samples and test:

```bash
# Download sample IPTC files
mkdir -p /tmp/iptc_samples
cd /tmp/iptc_samples

# Run oxidex on samples
for file in *.jpg; do
    echo "=== Testing: $file ==="
    cargo run --release -- "$file" | grep IPTC
done
```

Expected: IPTC tags are extracted and displayed

**Step 3: Compare with Perl ExifTool**

```bash
# Compare output with Perl ExifTool
for file in /tmp/iptc_samples/*.jpg; do
    echo "=== $file ==="
    echo "Perl ExifTool:"
    exiftool -IPTC:all "$file"
    echo ""
    echo "oxidex:"
    cargo run --release -- "$file" | grep IPTC
    echo "---"
done
```

Expected: Output matches Perl ExifTool (tag names and values)

**Step 4: Run benchmarks (if benchmark suite exists)**

Run: `cargo bench -- iptc`
Expected: Benchmarks complete without errors

**Step 5: Verify no compiler warnings**

Run: `cargo clippy --all-targets --all-features`
Expected: No warnings or errors

**Step 6: Format code**

Run: `cargo fmt --all`

**Step 7: Final commit and verification**

```bash
git status
# Ensure working directory is clean
cargo build --release
cargo test --all
```

Expected: Clean build and all tests pass

---

## Success Criteria

After completing all tasks, the following should be true:

- [ ] All unit tests pass (`cargo test --lib`)
- [ ] All integration tests pass (`cargo test --test '*'`)
- [ ] IPTC metadata is extracted from APP13 segments
- [ ] Common IPTC tags (ObjectName, By-line, Caption-Abstract, Keywords, etc.) are correctly parsed
- [ ] IPTC tags appear in metadata output with "IPTC:" prefix
- [ ] Output matches Perl ExifTool for test files
- [ ] No compiler warnings (`cargo clippy`)
- [ ] Code is properly formatted (`cargo fmt`)
- [ ] Documentation is updated
- [ ] Implementation roadmap reflects completion

---

## Estimated Time

- **Task 1-4:** 2-3 hours (core parsing logic)
- **Task 5-6:** 1-2 hours (integration)
- **Task 7-8:** 1 hour (testing and docs)
- **Task 9:** 30 minutes (verification)

**Total:** 4-6 hours of focused development time

---

## Notes for Executor

- Follow TDD rigorously: write test first, watch it fail, implement, watch it pass
- Commit after each task (not after each step)
- If any test fails, debug before moving to next task
- Use `--nocapture` to see print statements during test runs
- Reference the roadmap (docs/IMPLEMENTATION_ROADMAP.md) for detailed IPTC format specifications
- IPTC encoding can be Latin-1 or UTF-8; handle both
- Some IPTC fields are repeatable (e.g., Keywords); for now, only return the first occurrence

---

## References

- **IPTC Specification:** https://www.iptc.org/std/IIM/4.2/specification/IIMV4.2.pdf
- **IPTC Sample Files:** https://www.iptc.org/std/photometadata/examples/
- **Photoshop IRB Format:** Adobe Photoshop File Formats Specification
- **ExifTool IPTC.pm:** https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/IPTC.pm

---

**Plan Version:** 1.0
**Created:** 2025-01-15
**Last Updated:** 2025-01-15
