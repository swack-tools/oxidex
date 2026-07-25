//! EPS (Encapsulated PostScript) parser
//!
//! Parses EPS files to extract metadata from:
//! - PostScript DSC (Document Structuring Convention) comments
//! - Embedded XMP data
//! - Embedded IPTC data (via Photoshop 8BIM blocks)

#![allow(dead_code)]

use crate::core::tag_conversion::parse_string_to_tag_value;
use crate::core::value_formatter::{
    format_iptc_coded_charset, format_iptc_date, format_iptc_record_version, format_iptc_time,
    format_iptc_urgency,
};
use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::jpeg::iptc_parser::{
    dataset_to_tag_name, decode_iptc_string, parse_all_iptc_records,
};
use crate::parsers::xmp::{parse_xmp, parse_xmp_history};

/// Maximum bytes to read from EPS file for parsing
const MAX_READ_SIZE: usize = 1024 * 1024; // 1MB

/// Parser for EPS (Encapsulated PostScript) files
///
/// Extracts metadata from EPS files including:
/// - PostScript DSC comments (BoundingBox, Creator, Title, etc.)
/// - Embedded XMP metadata
/// - Embedded IPTC metadata (via Photoshop 8BIM resource blocks)
pub struct EPSParser;

impl EPSParser {
    /// Verifies the EPS file by checking for the PostScript signature
    pub fn verify_signature(data: &[u8]) -> bool {
        // Check for ASCII EPS: %!PS-Adobe
        if data.starts_with(b"%!PS-Adobe") {
            return true;
        }

        // Check for binary EPS (DOS EPS): 0xC5D0D3C6 magic
        if data.len() >= 4
            && data[0] == 0xC5
            && data[1] == 0xD0
            && data[2] == 0xD3
            && data[3] == 0xC6
        {
            return true;
        }

        false
    }

    /// Extracts DSC (Document Structuring Convention) comments
    fn extract_dsc_comments(text: &str, metadata: &mut MetadataMap) {
        let mut version: Option<String> = None;
        let mut pages: Option<String> = None;

        // EPS files may use bare CR (old Mac style) line endings, which
        // `str::lines()` does not split on. Normalize all line ending
        // styles to `\n` first so DSC comments are found regardless of
        // the source application's newline convention.
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");

        // ExifTool only reads DSC comments from the top-level document by
        // default; content nested inside %%BeginDocument/%%EndDocument
        // (embedded files) is skipped unless the -ee (ExtractEmbedded)
        // option is used. Track nesting depth so embedded document comments
        // don't clobber the outer document's metadata.
        let mut doc_depth: u32 = 0;

        // Helper: only the first occurrence of each DSC-comment tag counts
        // (ExifTool marks these tags Priority 0, "first found wins").
        macro_rules! insert_first {
            ($map:expr, $key:expr, $val:expr) => {
                if !$map.contains_key($key) {
                    $map.insert($key.to_string(), $val);
                }
            };
        }

        for line in normalized.lines() {
            let line = line.trim();

            if line.starts_with("%%BeginDocument") {
                doc_depth += 1;
                continue;
            }
            if line.starts_with("%%EndDocument") {
                doc_depth = doc_depth.saturating_sub(1);
                continue;
            }
            if doc_depth > 0 {
                // Inside an embedded document; skip its DSC comments.
                continue;
            }

            // DSC comments start with %%, except for a small set of tags
            // (notably ImageData) that some applications write with only a
            // single leading '%'. Match ExifTool's PostScript.pm behavior by
            // special-casing "%ImageData:" here before requiring "%%".
            if !line.starts_with("%%") {
                if let Some(value) = line.strip_prefix("%ImageData:") {
                    insert_first!(
                        metadata,
                        "PostScript:ImageData",
                        TagValue::String(value.trim().to_string())
                    );
                }
                continue;
            }

            // Parse specific DSC comments
            if let Some(value) = line.strip_prefix("%%BoundingBox:") {
                let value = value.trim();
                if value != "(atend)" {
                    insert_first!(
                        metadata,
                        "PostScript:BoundingBox",
                        TagValue::String(value.to_string())
                    );
                    // Also add EPS:BoundingBox for consistency with Worker 24 requirements
                    insert_first!(
                        metadata,
                        "EPS:BoundingBox",
                        TagValue::new_string(value.to_string())
                    );
                }
            } else if let Some(value) = line.strip_prefix("%%HiResBoundingBox:") {
                let value = value.trim();
                if value != "(atend)" {
                    insert_first!(
                        metadata,
                        "PostScript:HiResBoundingBox",
                        TagValue::String(value.to_string())
                    );
                }
            } else if let Some(value) = line.strip_prefix("%%Creator:") {
                let trimmed_value = value.trim().to_string();
                insert_first!(
                    metadata,
                    "PostScript:Creator",
                    TagValue::String(trimmed_value.clone())
                );
                // Add EPS:Creator as per Worker 24 requirements
                insert_first!(metadata, "EPS:Creator", TagValue::new_string(trimmed_value));
            } else if let Some(value) = line.strip_prefix("%%CreationDate:") {
                let trimmed_value = value.trim().to_string();
                insert_first!(
                    metadata,
                    "PostScript:CreateDate",
                    TagValue::String(trimmed_value.clone())
                );
                // Add EPS:CreationDate as per Worker 24 requirements
                insert_first!(
                    metadata,
                    "EPS:CreationDate",
                    TagValue::new_string(trimmed_value)
                );
            } else if let Some(value) = line.strip_prefix("%%Title:") {
                // Remove surrounding parentheses if present
                let value = value.trim();
                let value = if value.starts_with('(') && value.ends_with(')') {
                    &value[1..value.len() - 1]
                } else {
                    value
                };
                let value_str = value.to_string();
                insert_first!(
                    metadata,
                    "PostScript:Title",
                    TagValue::String(value_str.clone())
                );
                // Add EPS:Title as per Worker 24 requirements
                insert_first!(metadata, "EPS:Title", TagValue::new_string(value_str));
            } else if let Some(value) = line.strip_prefix("%%For:") {
                let trimmed_value = value.trim().to_string();
                insert_first!(
                    metadata,
                    "PostScript:For",
                    TagValue::String(trimmed_value.clone())
                );
                // Add EPS:For as per Worker 24 requirements
                insert_first!(metadata, "EPS:For", TagValue::new_string(trimmed_value));
            } else if let Some(value) = line.strip_prefix("%%DocumentData:") {
                insert_first!(
                    metadata,
                    "PostScript:DocumentData",
                    TagValue::String(value.trim().to_string())
                );
            } else if let Some(value) = line.strip_prefix("%%LanguageLevel:") {
                insert_first!(
                    metadata,
                    "PostScript:LanguageLevel",
                    TagValue::String(value.trim().to_string())
                );
            } else if let Some(value) = line.strip_prefix("%%Pages:") {
                let value = value.trim();
                if value != "(atend)" {
                    pages = Some(value.to_string());
                    insert_first!(
                        metadata,
                        "PostScript:Pages",
                        TagValue::String(value.to_string())
                    );
                }
            } else if let Some(value) = line.strip_prefix("%%ImageData:") {
                insert_first!(
                    metadata,
                    "PostScript:ImageData",
                    TagValue::String(value.trim().to_string())
                );
            }

            // Extract version from first line %!PS-Adobe-X.X EPSF-X.X
            if line.starts_with("%!PS-Adobe") && version.is_none() {
                if let Some(version_str) = extract_eps_version_from_header(line) {
                    version = Some(version_str);
                }
            }
        }

        // Add EPS:Version if extracted from header
        if let Some(v) = version {
            metadata.insert("EPS:Version".to_string(), TagValue::new_string(v));
        }

        // Add EPS:Pages as integer if available
        if let Some(pages_str) = pages {
            if let Ok(pages_int) = pages_str.parse::<i64>() {
                metadata.insert("EPS:Pages".to_string(), TagValue::new_integer(pages_int));
            }
        }

        // EPS:Orientation is typically not in DSC comments, but we can try to infer from BoundingBox
        // For now, we'll leave this for future enhancement
    }

    /// Extracts XMP metadata from EPS data
    fn extract_xmp(data: &[u8], metadata: &mut MetadataMap) {
        // Search for XMP packet markers
        const XMP_BEGIN: &[u8] = b"<?xpacket begin=";
        const XMP_END: &[u8] = b"<?xpacket end=";

        if let Some(begin_pos) = find_subsequence(data, XMP_BEGIN) {
            // Find the end of the xpacket processing instruction
            let after_begin = &data[begin_pos..];
            if let Some(xml_start_offset) = find_subsequence(after_begin, b"?>") {
                let xml_start_pos = begin_pos + xml_start_offset + 2; // +2 to skip ?>

                // Find XMP end marker
                if let Some(end_offset) = find_subsequence(&data[xml_start_pos..], XMP_END) {
                    let xmp_data = &data[xml_start_pos..xml_start_pos + end_offset];

                    // Parse the XMP data
                    if let Ok(xmp_tags) = parse_xmp(xmp_data) {
                        for (key, value) in xmp_tags {
                            metadata.insert(key, TagValue::new_string(value));
                        }
                    }

                    // The shared XMP parser flattens list-type properties
                    // (rdf:Bag/Seq/Alt, e.g. dc:subject) into a single
                    // comma-joined string. ExifTool reports these as arrays,
                    // so re-expand the known list-type XMP tags here into
                    // TagValue::Array for correct multi-value representation.
                    for list_tag in [
                        "XMP:Subject",
                        "XMP:SupplementalCategories",
                        "XMP-photoshop:SupplementalCategories",
                    ] {
                        if let Some(TagValue::String(joined)) = metadata.get(list_tag) {
                            let items: Vec<TagValue> = joined
                                .split(", ")
                                .map(|s| TagValue::new_string(s.to_string()))
                                .collect();
                            if items.len() > 1 {
                                metadata.insert(list_tag.to_string(), TagValue::Array(items));
                            }
                        }
                    }

                    // Parse XMP history for forensic metadata
                    if let Ok(xml_str) = std::str::from_utf8(xmp_data) {
                        if let Ok(history_tags) = parse_xmp_history(xml_str) {
                            for (key, value) in history_tags {
                                metadata.insert(key, TagValue::new_string(value));
                            }
                        }

                        // A handful of properties from older (pre-RDF-namespace)
                        // XMP toolkit output aren't covered by the general-purpose
                        // shared RDF parser: the bare (non-"rdf:"-prefixed) `about`
                        // attribute, the toolkit version attribute on the root
                        // wrapper element, and the nested stJob:name struct field
                        // inside xmpBJ:JobRef. Extract them directly here.
                        if !metadata.contains_key("XMP:About") {
                            if let Some(about) = extract_xml_attribute(xml_str, "about") {
                                metadata
                                    .insert("XMP:About".to_string(), TagValue::new_string(about));
                            }
                        }
                        if !metadata.contains_key("XMP:XMPToolkit") {
                            if let Some(toolkit) = extract_xml_attribute(xml_str, "x:xaptk")
                                .or_else(|| extract_xml_attribute(xml_str, "xmptk"))
                            {
                                metadata.insert(
                                    "XMP:XMPToolkit".to_string(),
                                    TagValue::new_string(toolkit),
                                );
                            }
                        }
                        if !metadata.contains_key("XMP:JobRefName") {
                            if let Some(job_name) = extract_xml_element_text(xml_str, "stJob:name")
                            {
                                metadata.insert(
                                    "XMP:JobRefName".to_string(),
                                    TagValue::new_string(job_name),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Extracts and hex-decodes an embedded `%%BeginPhotoshop`/`%%EndPhotoshop`
    /// (or single-`%` variant) DSC block, returning the raw Photoshop resource
    /// bytes (which normally start with an "8BIM" signature).
    ///
    /// ASCII EPS files embed Photoshop's binary 8BIM resource data (which
    /// includes IPTC and the IPTC digest) as ASCII-hex text wrapped in a
    /// `%%BeginPhotoshop: <len>` / `%%EndPhotoshop` block, one line of hex
    /// digits per line (each line commented out with a leading `%`). This
    /// mirrors ExifTool's `PostScript.pm` handling of the same block.
    fn extract_photoshop_block(data: &[u8]) -> Option<Vec<u8>> {
        const BEGIN_MARKER: &[u8] = b"BeginPhotoshop";
        const END_MARKER: &[u8] = b"EndPhotoshop";

        let begin_pos = find_subsequence(data, BEGIN_MARKER)?;
        let after_begin = &data[begin_pos + BEGIN_MARKER.len()..];
        let end_pos = find_subsequence(after_begin, END_MARKER)?;
        let block = &after_begin[..end_pos];

        let block_str = String::from_utf8_lossy(block);
        let mut hex_chars = String::new();
        for (i, line) in block_str.split(['\r', '\n']).enumerate() {
            if i == 0 {
                // First "line" is the ": <length>" declaration, not data.
                continue;
            }
            for c in line.chars() {
                if c.is_ascii_hexdigit() {
                    hex_chars.push(c);
                }
            }
        }

        hex::decode(hex_chars).ok()
    }

    /// Extracts IPTC metadata from Photoshop 8BIM blocks in EPS data
    fn extract_iptc(data: &[u8], metadata: &mut MetadataMap) {
        // Search for Photoshop 8BIM signature
        const EIGHTBIM: &[u8] = b"8BIM";
        const IPTC_RESOURCE_ID: u16 = 0x0404;

        let mut pos = 0;
        while pos + 12 < data.len() {
            // Search for next 8BIM block
            if let Some(block_pos) = find_subsequence(&data[pos..], EIGHTBIM) {
                let abs_pos = pos + block_pos;

                // Verify we have enough data for the header
                if abs_pos + 12 > data.len() {
                    break;
                }

                // Parse resource ID (2 bytes after signature)
                let id = u16::from_be_bytes([data[abs_pos + 4], data[abs_pos + 5]]);

                // Parse Pascal string name length
                let name_len = data[abs_pos + 6] as usize;

                // Calculate padding for name (must be even total length)
                let total_name_len = 1 + name_len; // 1 for length byte
                let padding = if total_name_len % 2 == 1 { 1 } else { 0 };

                // Calculate data offset
                let data_offset = abs_pos + 7 + name_len + padding;

                // Verify we have enough data for size field
                if data_offset + 4 > data.len() {
                    pos = abs_pos + 1;
                    continue;
                }

                // Parse data size (4 bytes)
                let data_size = u32::from_be_bytes([
                    data[data_offset],
                    data[data_offset + 1],
                    data[data_offset + 2],
                    data[data_offset + 3],
                ]) as usize;

                let data_start = data_offset + 4;

                // Check if this is an IPTC block
                if id == IPTC_RESOURCE_ID && data_start + data_size <= data.len() && data_size > 0 {
                    let iptc_data = &data[data_start..data_start + data_size];

                    // Parse IPTC records
                    if let Ok(records) = parse_all_iptc_records(iptc_data) {
                        // List-type IPTC datasets (Keywords, SupplementalCategories)
                        // can legitimately repeat; accumulate instead of
                        // overwriting so all values are preserved.
                        let mut keywords: Vec<String> = Vec::new();
                        let mut supplemental_categories: Vec<String> = Vec::new();

                        for record in records {
                            let tag_name =
                                dataset_to_tag_name(record.record_number, record.dataset_number);
                            let value = format_iptc_record_value(
                                record.record_number,
                                record.dataset_number,
                                &record.data,
                            );

                            match (record.record_number, record.dataset_number) {
                                (2, 25) => keywords.push(value),
                                (2, 20) => supplemental_categories.push(value),
                                _ => {
                                    metadata.insert(tag_name, parse_string_to_tag_value(&value));
                                }
                            }
                        }

                        if !keywords.is_empty() {
                            insert_iptc_list(metadata, "IPTC:Keywords", keywords);
                        }
                        if !supplemental_categories.is_empty() {
                            insert_iptc_list(
                                metadata,
                                "IPTC:SupplementalCategories",
                                supplemental_categories,
                            );
                        }
                    }
                }

                // Move past this block
                pos = data_start + data_size;
            } else {
                // No more 8BIM blocks found
                break;
            }
        }

        // Also extract IPTCDigest from Photoshop resources
        // Resource ID 0x0425 contains IPTC digest
        const IPTC_DIGEST_RESOURCE_ID: u16 = 0x0425;
        pos = 0;
        while pos + 12 < data.len() {
            if let Some(block_pos) = find_subsequence(&data[pos..], EIGHTBIM) {
                let abs_pos = pos + block_pos;

                if abs_pos + 12 > data.len() {
                    break;
                }

                let id = u16::from_be_bytes([data[abs_pos + 4], data[abs_pos + 5]]);
                let name_len = data[abs_pos + 6] as usize;
                let total_name_len = 1 + name_len;
                let padding = if total_name_len % 2 == 1 { 1 } else { 0 };
                let data_offset = abs_pos + 7 + name_len + padding;

                if data_offset + 4 > data.len() {
                    pos = abs_pos + 1;
                    continue;
                }

                let data_size = u32::from_be_bytes([
                    data[data_offset],
                    data[data_offset + 1],
                    data[data_offset + 2],
                    data[data_offset + 3],
                ]) as usize;

                let data_start = data_offset + 4;

                if id == IPTC_DIGEST_RESOURCE_ID
                    && data_start + data_size <= data.len()
                    && data_size == 16
                {
                    // IPTC digest is a 16-byte MD5 hash
                    let digest_data = &data[data_start..data_start + data_size];
                    let digest_hex: String =
                        digest_data.iter().map(|b| format!("{:02x}", b)).collect();
                    metadata.insert(
                        "Photoshop:IPTCDigest".to_string(),
                        TagValue::String(digest_hex),
                    );
                }

                pos = data_start + data_size;
            } else {
                break;
            }
        }
    }
}

impl FormatParser for EPSParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Read the file data
        let file_size = reader.size() as usize;
        let read_size = file_size.min(MAX_READ_SIZE);
        let data = reader.read(0, read_size)?;

        // Verify EPS signature
        if !Self::verify_signature(data) {
            return Err(ExifToolError::parse_error("Invalid EPS signature"));
        }

        let mut metadata = MetadataMap::new();

        // Set basic file info
        metadata.insert("FileType".to_string(), TagValue::String("EPS".to_string()));
        metadata.insert("FileSize".to_string(), TagValue::Integer(file_size as i64));
        metadata.insert(
            "MIMEType".to_string(),
            TagValue::String("application/postscript".to_string()),
        );

        // Handle binary EPS (DOS EPS) header
        let ps_data = if data.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) && data.len() >= 30 {
            // Binary EPS header contains offsets to the PostScript section
            let ps_start = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
            let ps_length = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

            if ps_start < data.len() && ps_start + ps_length <= data.len() {
                &data[ps_start..ps_start + ps_length]
            } else {
                data
            }
        } else {
            data
        };

        // Convert to text for DSC comment parsing
        if let Ok(text) = std::str::from_utf8(ps_data) {
            Self::extract_dsc_comments(text, &mut metadata);
        } else {
            // Try to find ASCII portions for DSC parsing
            // Some EPS files have mixed binary/text content
            let text: String = ps_data.iter().map(|&b| b as char).collect();
            Self::extract_dsc_comments(&text, &mut metadata);
        }

        // Extract XMP metadata
        Self::extract_xmp(data, &mut metadata);

        // Extract IPTC metadata from raw binary 8BIM blocks, if present
        Self::extract_iptc(data, &mut metadata);

        // ASCII EPS files typically embed the Photoshop 8BIM resource data
        // (IPTC + IPTC digest) as a hex-encoded %%BeginPhotoshop block rather
        // than raw binary, since PostScript is a text format. Decode that
        // block, if present, and extract IPTC from it too.
        if let Some(photoshop_data) = Self::extract_photoshop_block(data) {
            Self::extract_iptc(&photoshop_data, &mut metadata);
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::EPS)
    }
}

/// Extracts EPS version from the header line
/// Example: "%!PS-Adobe-3.0 EPSF-3.0" returns "3.0"
fn extract_eps_version_from_header(header: &str) -> Option<String> {
    // Look for EPSF-X.X pattern
    if let Some(epsf_pos) = header.find("EPSF-") {
        let after_epsf = &header[epsf_pos + 5..];
        // Extract version number (typically X.X)
        let version: String = after_epsf
            .chars()
            .take_while(|c| c.is_numeric() || *c == '.')
            .collect();
        if !version.is_empty() {
            return Some(version);
        }
    }
    // Fallback: look for PS-Adobe-X.X pattern
    if let Some(ps_pos) = header.find("PS-Adobe-") {
        let after_ps = &header[ps_pos + 9..];
        let version: String = after_ps
            .chars()
            .take_while(|c| c.is_numeric() || *c == '.')
            .collect();
        if !version.is_empty() {
            return Some(version);
        }
    }
    None
}

/// Finds a subsequence in a byte slice
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Extracts the value of an XML attribute of the form `name='value'` or
/// `name="value"` from raw XML text. This is a lightweight, targeted lookup
/// (not a general XML attribute parser) used only for the handful of
/// legacy XMP attributes not handled by the shared RDF parser.
fn extract_xml_attribute(text: &str, attr_name: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        let needle = format!("{attr_name}={quote}");
        if let Some(pos) = text.find(&needle) {
            let after = &text[pos + needle.len()..];
            if let Some(end) = after.find(quote) {
                return Some(after[..end].to_string());
            }
        }
    }
    None
}

/// Extracts the text content of the first `<tag_name>...</tag_name>` element
/// found in raw XML text. This is a lightweight, targeted lookup (not a
/// general XML parser) used only for simple leaf elements nested inside
/// structures the shared RDF parser doesn't flatten.
fn extract_xml_element_text(text: &str, tag_name: &str) -> Option<String> {
    let open = format!("<{tag_name}>");
    let close = format!("</{tag_name}>");
    let start = text.find(&open)? + open.len();
    let relative_end = text[start..].find(&close)?;
    Some(text[start..start + relative_end].trim().to_string())
}

/// Converts a raw IPTC IIM record payload to its string representation,
/// applying the same record/dataset-specific formatting ExifTool uses
/// (binary version numbers, date/time reformatting, and Urgency's
/// human-readable suffix).
fn format_iptc_record_value(record_number: u8, dataset_number: u8, data: &[u8]) -> String {
    if record_number == 1 {
        return match dataset_number {
            0 => format_iptc_record_version(data), // EnvelopeRecordVersion
            70 => format_iptc_date(&decode_iptc_string(data)), // DateSent
            80 => format_iptc_time(&decode_iptc_string(data)), // TimeSent
            90 => format_iptc_coded_charset(data), // CodedCharacterSet
            _ => decode_iptc_string(data),
        };
    }

    if record_number == 2 {
        return match dataset_number {
            0 => format_iptc_record_version(data), // ApplicationRecordVersion
            10 => format_iptc_urgency(&decode_iptc_string(data)),
            30 | 37 | 47 | 55 | 62 => format_iptc_date(&decode_iptc_string(data)),
            35 | 38 | 60 | 63 => format_iptc_time(&decode_iptc_string(data)),
            _ => decode_iptc_string(data),
        };
    }

    decode_iptc_string(data)
}

/// Inserts a possibly multi-valued IPTC tag (e.g. Keywords,
/// SupplementalCategories) into the metadata map. If the tag already has a
/// value (from a prior 8BIM block, e.g. raw-binary vs. hex-decoded
/// Photoshop data), the new values are merged rather than overwriting.
/// Single-valued results are stored as a plain string; multi-valued results
/// are stored as a `TagValue::Array` so downstream formatting matches
/// ExifTool's List-type tag representation.
fn insert_iptc_list(metadata: &mut MetadataMap, key: &str, mut values: Vec<String>) {
    let mut all_values: Vec<String> = match metadata.get(key) {
        Some(TagValue::Array(existing)) => existing
            .iter()
            .filter_map(|v| v.as_string().map(|s| s.to_string()))
            .collect(),
        Some(TagValue::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };
    all_values.append(&mut values);

    if all_values.len() == 1 {
        metadata.insert(key.to_string(), TagValue::new_string(all_values.remove(0)));
    } else {
        metadata.insert(
            key.to_string(),
            TagValue::Array(all_values.into_iter().map(TagValue::new_string).collect()),
        );
    }
}

/// Parses metadata from EPS files.
///
/// This is a convenience wrapper around EPSParser that provides a functional API.
pub fn parse_eps_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = EPSParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::BufferedReader;

    #[test]
    fn test_eps_ascii_signature() {
        let eps_data = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 100\n";
        assert!(EPSParser::verify_signature(eps_data));
    }

    #[test]
    fn test_eps_binary_signature() {
        let mut eps_data = vec![0xC5, 0xD0, 0xD3, 0xC6]; // DOS EPS magic
        eps_data.extend_from_slice(&[0; 26]); // Padding
        assert!(EPSParser::verify_signature(&eps_data));
    }

    #[test]
    fn test_eps_dsc_parsing() {
        let eps_data = br#"%!PS-Adobe-3.0 EPSF-3.0
%%Creator: Test Creator
%%Title: (Test Title)
%%CreationDate: 2024/01/01
%%BoundingBox: 0 0 100 200
%%EndComments
"#;

        let reader = BufferedReader::from_bytes(eps_data);
        let parser = EPSParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(metadata.get("FileType").unwrap().as_string(), Some("EPS"));
        assert_eq!(
            metadata.get("PostScript:Creator").unwrap().as_string(),
            Some("Test Creator")
        );
        assert_eq!(
            metadata.get("PostScript:Title").unwrap().as_string(),
            Some("Test Title")
        );
        assert_eq!(
            metadata.get("PostScript:CreateDate").unwrap().as_string(),
            Some("2024/01/01")
        );
        assert_eq!(
            metadata.get("PostScript:BoundingBox").unwrap().as_string(),
            Some("0 0 100 200")
        );
    }

    #[test]
    fn test_eps_invalid() {
        let invalid_data = b"Not an EPS file";
        let reader = BufferedReader::from_bytes(invalid_data);
        let parser = EPSParser;

        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_subsequence() {
        let data = b"Hello World";
        assert_eq!(find_subsequence(data, b"World"), Some(6));
        assert_eq!(find_subsequence(data, b"Foo"), None);
    }
}
