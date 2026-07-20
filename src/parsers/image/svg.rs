//! SVG (Scalable Vector Graphics) parser

#![allow(dead_code)]

use base64::{Engine as _, engine::general_purpose};
use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::xmp::parse_xmp;
use quick_xml::Reader;
use quick_xml::events::Event;

/// Maximum bytes to read from SVG file for parsing (SVG headers are at the start)
const MAX_READ_SIZE: usize = 65536; // 64KB

/// Parser for SVG (Scalable Vector Graphics) files
///
/// Extracts metadata from SVG XML-based vector graphics files including dimensions,
/// title, description, and other attributes.
pub struct SVGParser;

impl SVGParser {
    /// Verifies the SVG file by checking for the presence of "<svg" tag in the header
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        let read_size = (reader.size() as usize).min(1000);
        if read_size < 4 {
            return Ok(false);
        }
        let header = reader.read(0, read_size)?;
        let text = std::str::from_utf8(header).unwrap_or("");
        Ok(text.contains("<svg"))
    }

    /// Extracts an attribute value from an XML tag
    /// Handles both single and double quotes: width="100" or width='100'
    /// Also tolerates whitespace around the `=`, e.g. `width = "100"`, which
    /// is valid XML and appears in real-world SVG files.
    fn extract_attribute(text: &str, attr_name: &str) -> Option<String> {
        let bytes = text.as_bytes();
        let name_bytes = attr_name.as_bytes();
        let mut search_start = 0usize;

        while let Some(rel_pos) = text[search_start..].find(attr_name) {
            let name_start = search_start + rel_pos;
            let name_end = name_start + name_bytes.len();

            // Ensure the match is a whole attribute name: preceded by whitespace
            // (or start of tag/text) and not immediately followed by an
            // identifier character (so "width" doesn't match "strokewidth").
            let preceded_ok = name_start == 0
                || !(bytes[name_start - 1].is_ascii_alphanumeric() || bytes[name_start - 1] == b'-' || bytes[name_start - 1] == b':');

            if !preceded_ok {
                search_start = name_end;
                continue;
            }

            // Skip whitespace before '='
            let mut pos = name_end;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }

            if pos >= bytes.len() || bytes[pos] != b'=' {
                search_start = name_end;
                continue;
            }
            pos += 1;

            // Skip whitespace after '='
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }

            if pos >= bytes.len() || (bytes[pos] != b'"' && bytes[pos] != b'\'') {
                search_start = name_end;
                continue;
            }
            let quote = bytes[pos];
            let value_start = pos + 1;

            if let Some(end) = text[value_start..].find(quote as char) {
                return Some(text[value_start..value_start + end].to_string());
            }

            search_start = name_end;
        }

        None
    }

    /// Extracts text content from an XML element
    /// Example: <title>My SVG</title> returns "My SVG"
    fn extract_element_content(text: &str, element: &str) -> Option<String> {
        let open_tag = format!("<{}>", element);
        let close_tag = format!("</{}>", element);

        if let Some(start) = text.find(&open_tag) {
            let content_start = start + open_tag.len();
            if let Some(end) = text[content_start..].find(&close_tag) {
                let content = text[content_start..content_start + end].trim();
                return if !content.is_empty() {
                    Some(content.to_string())
                } else {
                    None
                };
            }
        }
        None
    }

    /// Parses dimension value, preserving units like "px", "em", "in", "%"
    /// ExifTool keeps units intact, so we should too
    fn parse_dimension(value: &str) -> Option<String> {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            Some(trimmed.to_string())
        } else {
            None
        }
    }

    /// Parses viewBox attribute: "minX minY width height"
    fn parse_viewbox(viewbox: &str) -> Option<(String, String)> {
        let parts: Vec<&str> = viewbox.split_whitespace().collect();
        if parts.len() == 4 {
            Some((parts[2].to_string(), parts[3].to_string()))
        } else {
            None
        }
    }

    /// Checks if SVG contains animation elements
    fn is_animated(text: &str) -> bool {
        text.contains("<animate") || text.contains("<animateTransform")
    }

    /// Counts SVG elements (shape elements, text, etc.)
    /// Counts common SVG elements: rect, circle, ellipse, line, polyline, polygon, path, text, image, use, g
    fn count_svg_elements(text: &str) -> i64 {
        let mut count = 0i64;
        let elements = [
            "<rect",
            "<circle",
            "<ellipse",
            "<line",
            "<polyline",
            "<polygon",
            "<path",
            "<text",
            "<image",
            "<use",
            "<g ",
        ];

        for element in &elements {
            // Count occurrences of each element tag
            let mut start = 0;
            while let Some(pos) = text[start..].find(element) {
                count += 1;
                start = start + pos + element.len();
            }
        }

        count
    }

    /// Extracts dc:creator content, handling RDF bags/sequences
    /// Handles formats like:
    /// - Simple: <dc:creator>Name</dc:creator>
    /// - RDF Bag: <dc:creator><rdf:Bag><rdf:li>Name1</rdf:li><rdf:li>Name2</rdf:li></rdf:Bag></dc:creator>
    /// - RDF Seq: <dc:creator><rdf:Seq><rdf:li>Name</rdf:li></rdf:Seq></dc:creator>
    fn extract_dc_creator(text: &str) -> Option<String> {
        // First try to find dc:creator element
        let start_tag = "<dc:creator>";
        let end_tag = "</dc:creator>";

        let start = text.find(start_tag)?;
        let content_start = start + start_tag.len();
        let end = text[content_start..].find(end_tag)? + content_start;
        let content = &text[content_start..end];

        // Try to extract rdf:li elements (handles both Bag and Seq)
        let li_values: Vec<String> = Self::extract_all_rdf_li(content);

        if !li_values.is_empty() {
            // ExifTool formats multiple creators as ["name1","name2"]
            if li_values.len() == 1 {
                Some(li_values[0].clone())
            } else {
                Some(format!(
                    "[{}]",
                    li_values
                        .iter()
                        .map(|s| format!("\"{}\"", s))
                        .collect::<Vec<_>>()
                        .join(",")
                ))
            }
        } else {
            // Simple content without RDF structure
            let trimmed = content.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('<') {
                Some(trimmed.to_string())
            } else {
                None
            }
        }
    }

    /// Extract all rdf:li values from content
    fn extract_all_rdf_li(content: &str) -> Vec<String> {
        let mut values = Vec::new();
        let mut pos = 0;

        while let Some(start) = content[pos..].find("<rdf:li>") {
            let value_start = pos + start + 8; // len of "<rdf:li>"
            if let Some(end) = content[value_start..].find("</rdf:li>") {
                let value = content[value_start..value_start + end].trim();
                if !value.is_empty() {
                    values.push(value.to_string());
                }
                pos = value_start + end + 10; // len of "</rdf:li>"
            } else {
                break;
            }
        }

        values
    }

    /// Extract embedded XMP metadata from SVG
    /// SVG can contain XMP in <x:xmpmeta> or <rdf:RDF> elements
    fn extract_xmp(text: &str, metadata: &mut MetadataMap) {
        // Look for x:xmpmeta element
        if let Some(start) = text.find("<x:xmpmeta") {
            if let Some(end) = text[start..].find("</x:xmpmeta>") {
                let xmp_data = &text[start..start + end + 12];
                if let Ok(xmp_tuples) = parse_xmp(xmp_data.as_bytes()) {
                    for (key, value) in xmp_tuples {
                        metadata.insert(key, TagValue::new_string(value));
                    }
                }
            }
        }
        // Also look for standalone rdf:RDF inside metadata element
        else if let Some(meta_start) = text.find("<metadata") {
            if let Some(meta_end) = text[meta_start..].find("</metadata>") {
                let meta_content = &text[meta_start..meta_start + meta_end + 11];
                if let Some(rdf_start) = meta_content.find("<rdf:RDF") {
                    if let Some(rdf_end) = meta_content[rdf_start..].find("</rdf:RDF>") {
                        let rdf_data = &meta_content[rdf_start..rdf_start + rdf_end + 10];
                        // Wrap in xmpmeta for parser
                        let wrapped = format!("<x:xmpmeta>{}</x:xmpmeta>", rdf_data);
                        if let Ok(xmp_tuples) = parse_xmp(wrapped.as_bytes()) {
                            for (key, value) in xmp_tuples {
                                metadata.insert(key, TagValue::new_string(value));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Extract Dublin Core elements that map to XMP tags
    fn extract_dublin_core(text: &str, metadata: &mut MetadataMap) {
        // dc:date -> XMP:Date
        if let Some(dc_date) = Self::extract_element_content(text, "dc:date") {
            metadata.insert("XMP:Date".to_string(), TagValue::new_string(dc_date));
        }

        // dc:format -> XMP:Format
        if let Some(dc_format) = Self::extract_element_content(text, "dc:format") {
            metadata.insert("XMP:Format".to_string(), TagValue::new_string(dc_format));
        }

        // dc:language -> XMP:Language
        if let Some(dc_lang) = Self::extract_element_content(text, "dc:language") {
            metadata.insert("XMP:Language".to_string(), TagValue::new_string(dc_lang));
        }

        // dc:publisher -> XMP:Publisher
        if let Some(dc_pub) = Self::extract_element_content(text, "dc:publisher") {
            metadata.insert("XMP:Publisher".to_string(), TagValue::new_string(dc_pub));
        }

        // rdf:about -> XMP:About
        // The "about" attribute lives on the <rdf:Description> element and, per
        // RDF/XML, may be written with or without the "rdf:" prefix (some real
        // world SVG producers omit it), so scope the search to that element's
        // opening tag and accept either spelling.
        if let Some(desc_start) = text.find("<rdf:Description")
            && let Some(tag_end) = text[desc_start..].find('>')
        {
            let tag_content = &text[desc_start..desc_start + tag_end + 1];
            let about = Self::extract_attribute(tag_content, "rdf:about")
                .or_else(|| Self::extract_attribute(tag_content, "about"));
            if let Some(about) = about {
                metadata.insert("XMP:About".to_string(), TagValue::new_string(about));
            }
        }
    }

    /// Extract SVG-specific description metadata
    fn extract_svg_desc_metadata(text: &str, metadata: &mut MetadataMap) {
        // Look for desc elements with specific structure
        // <desc role="xxxTitle">content</desc>
        let mut pos = 0;
        while let Some(desc_start) = text[pos..].find("<desc") {
            let desc_abs_start = pos + desc_start;

            // Find end of opening tag
            if let Some(tag_end) = text[desc_abs_start..].find('>') {
                let tag_content = &text[desc_abs_start..desc_abs_start + tag_end + 1];

                // Look for closing tag
                if let Some(close) = text[desc_abs_start + tag_end..].find("</desc>") {
                    let content =
                        &text[desc_abs_start + tag_end + 1..desc_abs_start + tag_end + close];

                    // Extract role attribute
                    if let Some(role) = Self::extract_attribute(tag_content, "role") {
                        let tag_name = format!("SVG:Desc{}", capitalize_first(&role));
                        metadata.insert(tag_name, TagValue::new_string(content.trim().to_string()));
                    }

                    pos = desc_abs_start + tag_end + close + 7;
                } else {
                    pos = desc_abs_start + 1;
                }
            } else {
                break;
            }
        }
    }

    /// Extracts nested, arbitrarily-namespaced elements inside `<desc>` blocks.
    ///
    /// SVG producers sometimes embed custom, namespaced markup inside `<desc>`
    /// (e.g. `<myfoo:title>...</myfoo:title>`, or several levels deep such as
    /// `<myfoo:scene><myfoo:what>...</myfoo:what></myfoo:scene>`). ExifTool
    /// surfaces each leaf element (an element with text but no child elements)
    /// as a tag named by concatenating the capitalized, namespace-stripped
    /// local names from `desc` down to the leaf, e.g. `DescSceneWhat`.
    fn extract_desc_nested_tags(text: &str, metadata: &mut MetadataMap) {
        let mut pos = 0;
        while let Some(desc_start) = text[pos..].find("<desc") {
            let desc_abs_start = pos + desc_start;

            let Some(tag_end) = text[desc_abs_start..].find('>') else {
                break;
            };
            let content_start = desc_abs_start + tag_end + 1;

            let Some(close) = text[content_start..].find("</desc>") else {
                pos = desc_abs_start + 1;
                continue;
            };
            let content_end = content_start + close;

            // Re-parse the whole block (including the opening tag) so quick-xml
            // sees a single well-formed root element.
            let block = &text[desc_abs_start..content_end + "</desc>".len()];
            walk_desc_xml(block, metadata);

            pos = content_end + "</desc>".len();
        }
    }
}

/// Capitalize first letter of string
fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Strips a namespace prefix (e.g. "myfoo:title" -> "title") from a qualified name.
fn local_name(qname: &str) -> &str {
    qname.rsplit(':').next().unwrap_or(qname)
}

/// Recursively walks a `<desc>...</desc>` XML fragment, emitting `SVG:Desc...`
/// tags for leaf elements (elements with text content but no child elements),
/// named by joining the capitalized, namespace-stripped local names of every
/// ancestor starting from `desc` itself.
fn walk_desc_xml(xml: &str, metadata: &mut MetadataMap) {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut path: Vec<String> = Vec::new();
    let mut has_child_stack: Vec<bool> = Vec::new();
    let mut text_stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let raw_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if let Some(parent_has_child) = has_child_stack.last_mut() {
                    *parent_has_child = true;
                }
                path.push(capitalize_first(local_name(&raw_name)));
                has_child_stack.push(false);
                text_stack.push(String::new());
            }
            Ok(Event::Text(t)) => {
                if let Some(buf_text) = text_stack.last_mut() {
                    buf_text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(buf_text) = text_stack.last_mut() {
                    buf_text.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
            }
            Ok(Event::End(_)) => {
                let has_child = has_child_stack.pop().unwrap_or(false);
                let text = text_stack.pop().unwrap_or_default();
                if !has_child {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        let tag_name = format!("SVG:{}", path.join(""));
                        if !metadata.contains_key(&tag_name) {
                            metadata.insert(tag_name, TagValue::new_string(trimmed.to_string()));
                        }
                    }
                }
                path.pop();
            }
            Ok(Event::Empty(e)) => {
                if let Some(parent_has_child) = has_child_stack.last_mut() {
                    *parent_has_child = true;
                }
                let _ = e; // self-closing leaf carries no text; nothing to emit
            }
            Err(_) => break,
            _ => {}
        }
    }
}

/// Extracts embedded C2PA content authenticity data from a `<c2pa:manifest>`
/// element and decodes its base64 payload as a JUMBF (ISO/IEC 19566-5) box
/// structure, surfacing tags the same way ExifTool's JUMBF module does.
fn extract_c2pa_manifest(text: &str, metadata: &mut MetadataMap) {
    let Some(start) = text.find("<c2pa:manifest") else {
        return;
    };
    let Some(tag_end) = text[start..].find('>') else {
        return;
    };
    let content_start = start + tag_end + 1;

    let Some(close) = text[content_start..].find("</c2pa:manifest>") else {
        return;
    };
    let content_end = content_start + close;

    let cleaned: String = text[content_start..content_end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    let Ok(decoded) = general_purpose::STANDARD.decode(cleaned.as_bytes()) else {
        return;
    };

    walk_jumbf_boxes(&decoded, metadata, true);
}

/// Recursively walks a JUMBF box stream (ISO/IEC 19566-5), populating
/// `JUMBF:*` tags. Only the first value seen for a given tag is kept,
/// matching ExifTool's default (non `-a`) behavior for duplicate tags.
fn walk_jumbf_boxes(data: &[u8], metadata: &mut MetadataMap, is_top: bool) {
    const BOX_HEADER_SIZE: usize = 8;
    let mut offset = 0usize;

    while offset + BOX_HEADER_SIZE <= data.len() {
        let length = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        let box_type = &data[offset + 4..offset + BOX_HEADER_SIZE];

        let box_end = if length == 0 {
            data.len()
        } else {
            (offset + length).min(data.len())
        };
        if box_end <= offset + BOX_HEADER_SIZE {
            break;
        }
        let payload = &data[offset + BOX_HEADER_SIZE..box_end];

        match box_type {
            b"jumb" => {
                if is_top && !metadata.contains_key("JUMBF:JUMBF") {
                    metadata.insert(
                        "JUMBF:JUMBF".to_string(),
                        TagValue::new_string(format!(
                            "(Binary data {} bytes, use -b option to extract)",
                            box_end - offset
                        )),
                    );
                }
                walk_jumbf_boxes(payload, metadata, false);
            }
            b"jumd" => parse_jumd_box(payload, metadata),
            b"json" => parse_json_box(payload, metadata),
            _ => {}
        }

        if length == 0 {
            break;
        }
        offset += length;
    }
}

/// Parses a JUMBF description ("jumd") box: a 16-byte content-type UUID
/// (whose first 4 bytes are conventionally an ASCII mnemonic), a 1-byte
/// toggles field, and an optional null-terminated UTF-8 label.
fn parse_jumd_box(payload: &[u8], metadata: &mut MetadataMap) {
    if payload.len() < 17 {
        return;
    }

    let type_bytes = &payload[0..4];
    let rest = &payload[4..16];

    let type_str = if type_bytes.iter().all(|b| b.is_ascii_graphic()) {
        String::from_utf8_lossy(type_bytes).to_string()
    } else {
        type_bytes.iter().map(|b| format!("{:02x}", b)).collect()
    };

    let jumd_type = format!(
        "({})-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        type_str,
        rest[0],
        rest[1],
        rest[2],
        rest[3],
        rest[4],
        rest[5],
        rest[6],
        rest[7],
        rest[8],
        rest[9],
        rest[10],
        rest[11]
    );

    if !metadata.contains_key("JUMBF:JUMDType") {
        metadata.insert("JUMBF:JUMDType".to_string(), TagValue::new_string(jumd_type));
    }

    // Label follows the 1-byte toggles field at offset 16.
    if payload.len() > 17
        && let Some(null_pos) = payload[17..].iter().position(|&b| b == 0)
        && let Ok(label) = std::str::from_utf8(&payload[17..17 + null_pos])
        && !label.is_empty()
        && !metadata.contains_key("JUMBF:JUMDLabel")
    {
        metadata.insert(
            "JUMBF:JUMDLabel".to_string(),
            TagValue::new_string(label.to_string()),
        );
    }
}

/// Parses a JSON content box, surfacing each top-level string field as a
/// `JUMBF:<CapitalizedKey>` tag (e.g. `{"location": "Salem, Oregon"}` becomes
/// `JUMBF:Location`), matching ExifTool's handling of C2PA JSON assertions.
fn parse_json_box(payload: &[u8], metadata: &mut MetadataMap) {
    let Ok(text) = std::str::from_utf8(payload) else {
        return;
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text)
    else {
        return;
    };

    for (key, val) in map {
        if let serde_json::Value::String(s) = val {
            let tag_name = format!("JUMBF:{}", capitalize_first(&key));
            if !metadata.contains_key(&tag_name) {
                metadata.insert(tag_name, TagValue::new_string(s));
            }
        }
    }
}

impl FormatParser for SVGParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid SVG signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("SVG".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Read up to 64KB for parsing (SVG metadata is in the header)
        let read_size = std::cmp::min(reader.size() as usize, MAX_READ_SIZE);
        let content = reader.read(0, read_size)?;
        let text = std::str::from_utf8(content).unwrap_or("");

        // Extract <svg> tag (find first occurrence)
        if let Some(svg_start) = text.find("<svg") {
            let svg_end = text[svg_start..]
                .find('>')
                .map(|pos| svg_start + pos)
                .unwrap_or(text.len());
            let svg_tag = &text[svg_start..svg_end];

            // Extract width and height
            if let Some(width) = Self::extract_attribute(svg_tag, "width")
                && let Some(parsed) = Self::parse_dimension(&width)
            {
                metadata.insert("ImageWidth".to_string(), TagValue::String(parsed.clone()));
                // Also add SVG:Width for Worker 26 compatibility
                metadata.insert("SVG:Width".to_string(), TagValue::new_string(parsed));
            }

            if let Some(height) = Self::extract_attribute(svg_tag, "height")
                && let Some(parsed) = Self::parse_dimension(&height)
            {
                metadata.insert("ImageHeight".to_string(), TagValue::String(parsed.clone()));
                // Also add SVG:Height for Worker 26 compatibility
                metadata.insert("SVG:Height".to_string(), TagValue::new_string(parsed));
            }

            // Extract viewBox for dimensions if width/height not present
            if let Some(viewbox) = Self::extract_attribute(svg_tag, "viewBox") {
                metadata.insert(
                    "SVG:ViewBox".to_string(),
                    TagValue::new_string(viewbox.clone()),
                );

                // If no width/height, try to extract from viewBox
                if !metadata.contains_key("ImageWidth")
                    && let Some((vb_width, vb_height)) = Self::parse_viewbox(&viewbox)
                {
                    metadata.insert("ImageWidth".to_string(), TagValue::String(vb_width));
                    metadata.insert("ImageHeight".to_string(), TagValue::String(vb_height));
                }
            }

            // Extract xmlns (namespace) - ExifTool calls this "Xmlns"
            if let Some(xmlns) = Self::extract_attribute(svg_tag, "xmlns") {
                metadata.insert("SVG:Xmlns".to_string(), TagValue::String(xmlns));
            }

            // Extract version - ExifTool calls this "SVGVersion" or "Version"
            if let Some(version) = Self::extract_attribute(svg_tag, "version") {
                metadata.insert(
                    "SVG:SVGVersion".to_string(),
                    TagValue::String(version.clone()),
                );
                // Also add SVG:Version for Worker 26 compatibility
                metadata.insert("SVG:Version".to_string(), TagValue::new_string(version));
            }

            // Extract preserveAspectRatio
            if let Some(preserve) = Self::extract_attribute(svg_tag, "preserveAspectRatio") {
                metadata.insert(
                    "SVG:PreserveAspectRatio".to_string(),
                    TagValue::new_string(preserve),
                );
            }
        }

        // Extract title
        if let Some(title) = Self::extract_element_content(text, "title") {
            metadata.insert("Title".to_string(), TagValue::String(title));
        }

        // Extract description
        if let Some(desc) = Self::extract_element_content(text, "desc") {
            metadata.insert("Description".to_string(), TagValue::String(desc));
        }

        // Extract embedded XMP metadata first
        Self::extract_xmp(text, &mut metadata);

        // Extract Dublin Core metadata if present
        if text.contains("dc:") {
            if let Some(dc_title) = Self::extract_element_content(text, "dc:title") {
                metadata.insert("XMP:Title".to_string(), TagValue::String(dc_title));
            }
            if let Some(dc_creator) = Self::extract_dc_creator(text) {
                metadata.insert("XMP:Creator".to_string(), TagValue::String(dc_creator));
            }
            if let Some(dc_desc) = Self::extract_element_content(text, "dc:description") {
                metadata.insert("XMP:Description".to_string(), TagValue::String(dc_desc));
            }

            // Extract additional Dublin Core elements
            Self::extract_dublin_core(text, &mut metadata);
        }

        // Extract SVG-specific desc metadata with roles
        Self::extract_svg_desc_metadata(text, &mut metadata);

        // Extract arbitrarily-namespaced nested elements inside <desc> (e.g.
        // <myfoo:title>, <myfoo:scene><myfoo:what>...)
        Self::extract_desc_nested_tags(text, &mut metadata);

        // Extract embedded C2PA/JUMBF metadata from a <c2pa:manifest> element
        extract_c2pa_manifest(text, &mut metadata);

        // Check if animated
        if Self::is_animated(text) {
            metadata.insert(
                "SVG:Animated".to_string(),
                TagValue::String("true".to_string()),
            );
        }

        // Count SVG elements (shapes, text, etc.) for Worker 26
        let element_count = Self::count_svg_elements(text);
        if element_count > 0 {
            metadata.insert(
                "SVG:ElementCount".to_string(),
                TagValue::new_integer(element_count),
            );
        }

        // Check for <defs> definitions
        let has_definitions = text.contains("<defs");
        metadata.insert(
            "SVG:HasDefinitions".to_string(),
            TagValue::new_string(if has_definitions { "true" } else { "false" }),
        );

        // Check for <metadata> element
        let has_metadata = text.contains("<metadata");
        metadata.insert(
            "SVG:HasMetadata".to_string(),
            TagValue::new_string(if has_metadata { "true" } else { "false" }),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::SVG)
    }
}

/// Parses metadata from SVG files.
///
/// This is a convenience wrapper around SVGParser that provides a functional API.
pub fn parse_svg_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = SVGParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::BufferedReader;

    #[test]
    fn test_svg_basic_parsing() {
        let svg_data = r#"<?xml version="1.0"?>
<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="200" height="150">
  <title>Test SVG</title>
  <desc>A test description</desc>
  <rect x="10" y="10" width="100" height="50"/>
</svg>"#;

        let reader = BufferedReader::from_bytes(svg_data.as_bytes());
        let parser = SVGParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(metadata.get("FileType").unwrap().as_string(), Some("SVG"));
        assert_eq!(metadata.get("ImageWidth").unwrap().as_string(), Some("200"));
        assert_eq!(
            metadata.get("ImageHeight").unwrap().as_string(),
            Some("150")
        );
        assert_eq!(metadata.get("Title").unwrap().as_string(), Some("Test SVG"));
        assert_eq!(
            metadata.get("Description").unwrap().as_string(),
            Some("A test description")
        );
        assert_eq!(
            metadata.get("SVG:Xmlns").unwrap().as_string(),
            Some("http://www.w3.org/2000/svg")
        );
        assert_eq!(
            metadata.get("SVG:SVGVersion").unwrap().as_string(),
            Some("1.1")
        );
    }

    #[test]
    fn test_svg_viewbox() {
        let svg_data = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 200"></svg>"#;

        let reader = BufferedReader::from_bytes(svg_data.as_bytes());
        let parser = SVGParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(metadata.get("ImageWidth").unwrap().as_string(), Some("100"));
        assert_eq!(
            metadata.get("ImageHeight").unwrap().as_string(),
            Some("200")
        );
        assert_eq!(
            metadata.get("SVG:ViewBox").unwrap().as_string(),
            Some("0 0 100 200")
        );
    }

    #[test]
    fn test_svg_dimension_units() {
        let svg_data = r#"<svg width="300px" height="200em"></svg>"#;

        let reader = BufferedReader::from_bytes(svg_data.as_bytes());
        let parser = SVGParser;
        let metadata = parser.parse(&reader).unwrap();

        // Units should be preserved to match ExifTool behavior
        assert_eq!(
            metadata.get("ImageWidth").unwrap().as_string(),
            Some("300px")
        );
        assert_eq!(
            metadata.get("ImageHeight").unwrap().as_string(),
            Some("200em")
        );
    }

    #[test]
    fn test_svg_animated() {
        let svg_data = r#"<svg>
  <rect x="10" y="10" width="50" height="50">
    <animate attributeName="x" from="10" to="100" dur="1s"/>
  </rect>
</svg>"#;

        let reader = BufferedReader::from_bytes(svg_data.as_bytes());
        let parser = SVGParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("SVG:Animated").unwrap().as_string(),
            Some("true")
        );
    }

    #[test]
    fn test_svg_dublin_core() {
        let svg_data = r#"<svg xmlns:dc="http://purl.org/dc/elements/1.1/">
  <metadata>
    <dc:title>DC Title</dc:title>
    <dc:creator>DC Creator</dc:creator>
    <dc:description>DC Description</dc:description>
  </metadata>
</svg>"#;

        let reader = BufferedReader::from_bytes(svg_data.as_bytes());
        let parser = SVGParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("XMP:Title").unwrap().as_string(),
            Some("DC Title")
        );
        assert_eq!(
            metadata.get("XMP:Creator").unwrap().as_string(),
            Some("DC Creator")
        );
        assert_eq!(
            metadata.get("XMP:Description").unwrap().as_string(),
            Some("DC Description")
        );
    }

    #[test]
    fn test_svg_dublin_core_rdf_bag() {
        // Test RDF Bag structure for multiple creators
        let svg_data = r#"<svg xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <metadata>
    <dc:creator>
      <rdf:Bag>
        <rdf:li>Irving Bird</rdf:li>
        <rdf:li>Mary Lambert</rdf:li>
      </rdf:Bag>
    </dc:creator>
  </metadata>
</svg>"#;

        let reader = BufferedReader::from_bytes(svg_data.as_bytes());
        let parser = SVGParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("XMP:Creator").unwrap().as_string(),
            Some("[\"Irving Bird\",\"Mary Lambert\"]")
        );
    }

    #[test]
    fn test_svg_invalid() {
        let invalid_data = b"Not an SVG file";
        let reader = BufferedReader::from_bytes(invalid_data);
        let parser = SVGParser;

        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_svg_single_quotes() {
        let svg_data = r#"<svg width='100' height='200'></svg>"#;

        let reader = BufferedReader::from_bytes(svg_data.as_bytes());
        let parser = SVGParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(metadata.get("ImageWidth").unwrap().as_string(), Some("100"));
        assert_eq!(
            metadata.get("ImageHeight").unwrap().as_string(),
            Some("200")
        );
    }
}
