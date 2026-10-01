//! SVG (Scalable Vector Graphics) parser

#![allow(dead_code)]

use base64::{Engine as _, engine::general_purpose};

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::xmp::parse_xmp;

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
    /// Also tolerates whitespace around the `=`, e.g. `xmlns = 'http://...'`
    fn extract_attribute(text: &str, attr_name: &str) -> Option<String> {
        let bytes = text.as_bytes();
        let mut search_start = 0usize;

        while let Some(rel_pos) = text[search_start..].find(attr_name) {
            let pos = search_start + rel_pos;

            // Basic word-boundary check so e.g. "width" doesn't match inside
            // "strokewidth" or "xmlns" doesn't match the start of "xmlns:foo"
            let before_ok = pos == 0 || {
                let c = bytes[pos - 1];
                !(c.is_ascii_alphanumeric() || c == b'-' || c == b':' || c == b'_')
            };

            if before_ok {
                let mut idx = pos + attr_name.len();
                while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
                    idx += 1;
                }
                if idx < bytes.len() && bytes[idx] == b'=' {
                    idx += 1;
                    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
                        idx += 1;
                    }
                    if idx < bytes.len() && (bytes[idx] == b'"' || bytes[idx] == b'\'') {
                        let quote = bytes[idx];
                        let value_start = idx + 1;
                        if let Some(end_rel) = text[value_start..].find(quote as char) {
                            return Some(text[value_start..value_start + end_rel].to_string());
                        }
                    }
                }
            }

            search_start = pos + attr_name.len();
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

        // rdf:about (or bare "about" within an rdf:Description tag) -> XMP:About
        if let Some(desc_start) = text.find("<rdf:Description")
            && let Some(tag_end_rel) = text[desc_start..].find('>')
        {
            let desc_tag = &text[desc_start..desc_start + tag_end_rel + 1];
            if let Some(about) = Self::extract_attribute(desc_tag, "rdf:about")
                .or_else(|| Self::extract_attribute(desc_tag, "about"))
            {
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
                let tag_end_abs = desc_abs_start + tag_end;
                let tag_content = &text[desc_abs_start..tag_end_abs + 1];

                // Look for closing tag
                if let Some(close) = text[tag_end_abs..].find("</desc>") {
                    let inner_start = tag_end_abs + 1;
                    let inner_end = tag_end_abs + close;
                    let content = &text[inner_start..inner_end];

                    // Extract role attribute (e.g. <desc role="xxxTitle">content</desc>)
                    if let Some(role) = Self::extract_attribute(tag_content, "role") {
                        let tag_name = format!("SVG:Desc{}", capitalize_first(&role));
                        metadata.insert(tag_name, TagValue::new_string(content.trim().to_string()));
                    } else {
                        // Otherwise, recursively walk any namespaced child elements
                        // (e.g. <myfoo:title>...</myfoo:title>) and build tag names by
                        // concatenating the capitalized path of leaf element names,
                        // matching ExifTool's generic SVG "desc" processing.
                        Self::extract_desc_children(content, "", metadata, 0);
                    }

                    pos = inner_end + "</desc>".len();
                } else {
                    pos = desc_abs_start + 1;
                }
            } else {
                break;
            }
        }
    }

    /// Recursively walks child elements of a `<desc>` element. Leaf elements (those with
    /// no nested child elements) produce a tag named `SVG:Desc<Path>` where `<Path>` is the
    /// capitalized, concatenated names of all ancestor elements (excluding `<desc>` itself)
    /// down to and including the leaf. Non-leaf elements only contribute to the path and do
    /// not themselves produce a tag.
    fn extract_desc_children(content: &str, path: &str, metadata: &mut MetadataMap, depth: usize) {
        if depth > 20 {
            return;
        }

        let mut pos = 0usize;
        while let Some(lt_rel) = content[pos..].find('<') {
            let lt_abs = pos + lt_rel;

            // Skip XML comments
            if content[lt_abs..].starts_with("<!--") {
                if let Some(end_rel) = content[lt_abs..].find("-->") {
                    pos = lt_abs + end_rel + 3;
                    continue;
                } else {
                    break;
                }
            }

            // A stray closing tag means our bounds are off; stop to avoid misparsing
            if content[lt_abs..].starts_with("</") {
                break;
            }

            let Some(gt_rel) = content[lt_abs..].find('>') else {
                break;
            };
            let gt_abs = lt_abs + gt_rel;
            let open_tag = content[lt_abs + 1..gt_abs].trim_end();

            let self_closing = open_tag.ends_with('/');
            let tag_token = open_tag.trim_end_matches('/').trim_end();
            let tag_name_full = tag_token.split_whitespace().next().unwrap_or("");

            if tag_name_full.is_empty() {
                break;
            }

            if self_closing {
                pos = gt_abs + 1;
                continue;
            }

            let local_name = tag_name_full.rsplit(':').next().unwrap_or(tag_name_full);
            let close_tag = format!("</{}>", tag_name_full);

            if let Some(close_rel) = content[gt_abs + 1..].find(&close_tag) {
                let inner_start = gt_abs + 1;
                let inner_end = gt_abs + 1 + close_rel;
                let inner = &content[inner_start..inner_end];
                let new_path = format!("{}{}", path, capitalize_first(local_name));

                if inner.contains('<') {
                    // Non-leaf: recurse into children, don't emit a tag for this element
                    Self::extract_desc_children(inner, &new_path, metadata, depth + 1);
                } else {
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        let tag_key = format!("SVG:Desc{}", new_path);
                        metadata.insert(tag_key, TagValue::new_string(trimmed.to_string()));
                    }
                }

                pos = inner_end + close_tag.len();
            } else {
                // No matching close tag found; bail out rather than looping forever
                break;
            }
        }
    }

    /// Extract JUMBF (JPEG Universal Metadata Box Format) metadata embedded as base64 inside
    /// a `<c2pa:manifest>` element within SVG `<metadata>`. This is used to carry C2PA content
    /// provenance data (ISO/IEC 19566-5).
    fn extract_c2pa_manifest(text: &str, metadata: &mut MetadataMap) {
        let Some(start) = text.find("<c2pa:manifest") else {
            return;
        };
        let Some(tag_end_rel) = text[start..].find('>') else {
            return;
        };
        let content_start = start + tag_end_rel + 1;
        let Some(close_rel) = text[content_start..].find("</c2pa:manifest>") else {
            return;
        };

        let raw = &text[content_start..content_start + close_rel];
        let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
        let Ok(decoded) = general_purpose::STANDARD.decode(cleaned.as_bytes()) else {
            return;
        };

        metadata.insert(
            "JUMBF:JUMBF".to_string(),
            TagValue::new_string(format!(
                "(Binary data {} bytes, use -b option to extract)",
                decoded.len()
            )),
        );

        let mut first_jumd_seen = false;
        Self::parse_jumbf_boxes(&decoded, metadata, &mut first_jumd_seen, 0);
    }

    /// Recursively walks JUMBF boxes (ISO/IEC 19566-5), extracting the JUMDType/JUMDLabel
    /// of the outermost description box (matching ExifTool, which keeps only the first
    /// value for duplicate tag names), plus any string values found in "json" content boxes.
    fn parse_jumbf_boxes(
        data: &[u8],
        metadata: &mut MetadataMap,
        first_jumd_seen: &mut bool,
        depth: usize,
    ) {
        if depth > 20 {
            return;
        }

        let mut offset = 0usize;
        while offset + 8 <= data.len() {
            let length = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            if length < 8 || offset + length > data.len() {
                break;
            }
            let box_type = &data[offset + 4..offset + 8];
            let content = &data[offset + 8..offset + length];

            if box_type == b"jumb" {
                Self::parse_jumbf_boxes(content, metadata, first_jumd_seen, depth + 1);
            } else if box_type == b"jumd" {
                if !*first_jumd_seen {
                    *first_jumd_seen = true;
                    if content.len() >= 17 {
                        let uuid = &content[0..16];
                        metadata.insert(
                            "JUMBF:JUMDType".to_string(),
                            TagValue::new_string(Self::format_jumd_type(uuid)),
                        );

                        let rest = &content[17..];
                        if let Some(nul_rel) = rest.iter().position(|&b| b == 0)
                            && let Ok(label) = std::str::from_utf8(&rest[..nul_rel])
                            && !label.is_empty()
                        {
                            metadata.insert(
                                "JUMBF:JUMDLabel".to_string(),
                                TagValue::new_string(label.to_string()),
                            );
                        }
                    }
                }
            } else if box_type == b"json" {
                Self::extract_jumbf_json_strings(content, metadata);
            }

            offset += length;
        }
    }

    /// Formats a 16-byte JUMBF content-type UUID the way ExifTool does: the first 4 bytes
    /// are shown as ASCII (in parens) when printable, and the remaining 12 bytes as
    /// hyphen-separated hex groups of 2/2/8 bytes.
    fn format_jumd_type(uuid: &[u8]) -> String {
        let first4 = &uuid[0..4];
        let first_part = if first4.iter().all(|&b| b.is_ascii_graphic()) {
            format!("({})", std::str::from_utf8(first4).unwrap_or_default())
        } else {
            format!(
                "{:02x}{:02x}{:02x}{:02x}",
                first4[0], first4[1], first4[2], first4[3]
            )
        };

        format!(
            "{}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            first_part,
            uuid[4],
            uuid[5],
            uuid[6],
            uuid[7],
            uuid[8],
            uuid[9],
            uuid[10],
            uuid[11],
            uuid[12],
            uuid[13],
            uuid[14],
            uuid[15]
        )
    }

    /// Extracts string values from a JUMBF "json" content box, inserting tags as
    /// `JUMBF:<CapitalizedKey>`. Only the first value seen for a given key is kept
    /// (matching ExifTool's JSON-based output, which cannot represent duplicate keys).
    fn extract_jumbf_json_strings(content: &[u8], metadata: &mut MetadataMap) {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(content) else {
            return;
        };
        let Some(obj) = value.as_object() else {
            return;
        };
        for (key, val) in obj {
            if let Some(s) = val.as_str() {
                let tag_key = format!("JUMBF:{}", capitalize_first(key));
                if !metadata.contains_key(&tag_key) {
                    metadata.insert(tag_key, TagValue::new_string(s.to_string()));
                }
            }
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

        // Extract embedded C2PA/JUMBF manifest data, if present
        Self::extract_c2pa_manifest(text, &mut metadata);

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
