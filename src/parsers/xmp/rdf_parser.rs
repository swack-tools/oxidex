//! RDF/XML parsing for XMP
//!
//! This module handles parsing of RDF/XML data using quick-xml.
//! It extracts simple string properties from XMP metadata while
//! gracefully skipping complex structures (bags, sequences, structs).
//!
//! # XMP Structure
//!
//! Standard XMP has this structure:
//! ```xml
//! <x:xmpmeta xmlns:x="adobe:ns:meta/">
//!   <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
//!     <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/">
//!       <xmp:Creator>John Doe</xmp:Creator>
//!       <xmp:ModifyDate>2023-05-15</xmp:ModifyDate>
//!     </rdf:Description>
//!   </rdf:RDF>
//! </x:xmpmeta>
//! ```
//!
//! # Example
//!
//! ```no_run
//! use exiftool_rs::parsers::xmp::rdf_parser::parse_xmp;
//!
//! let xml = br#"
//!     <x:xmpmeta xmlns:x="adobe:ns:meta/">
//!       <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
//!         <rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/">
//!           <xmp:Creator>John Doe</xmp:Creator>
//!           <xmp:Rating>5</xmp:Rating>
//!         </rdf:Description>
//!       </rdf:RDF>
//!     </x:xmpmeta>
//! "#;
//!
//! let result = parse_xmp(xml).unwrap();
//! assert!(result.len() >= 2);
//! ```

use crate::error::{ExifToolError, Result};
use crate::parsers::xmp::namespace_resolver::NamespaceResolver;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

/// Parses XMP metadata from RDF/XML format.
///
/// This function extracts simple string properties from XMP metadata.
/// Complex structures (rdf:Bag, rdf:Seq, rdf:Alt, nested structs) are
/// currently skipped and not parsed.
///
/// # Parameters
///
/// - `xml_bytes`: Raw XML data containing XMP metadata
///
/// # Returns
///
/// Vector of (tag_name, value) pairs where tag_name includes namespace
/// prefix in the format "XMP:PropertyName" (e.g., "XMP:Creator", "XMP:Rights").
///
/// # Errors
///
/// Returns `ParseError` if XML is malformed or cannot be parsed.
///
/// # Example
///
/// ```no_run
/// use exiftool_rs::parsers::xmp::rdf_parser::parse_xmp;
///
/// let xml = br#"
///     <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
///              xmlns:xmp="http://ns.adobe.com/xap/1.0/">
///       <rdf:Description>
///         <xmp:Creator>John Doe</xmp:Creator>
///       </rdf:Description>
///     </rdf:RDF>
/// "#;
///
/// let result = parse_xmp(xml).unwrap();
/// assert_eq!(result.len(), 1);
/// assert_eq!(result[0], ("XMP:Creator".to_string(), "John Doe".to_string()));
/// ```
pub fn parse_xmp(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.trim_text(true); // Trim whitespace from text nodes

    let mut resolver = NamespaceResolver::new();
    let mut results = Vec::new();
    let mut buf = Vec::new();

    // State tracking
    let mut inside_description = false;
    let mut current_property: Option<String> = None;
    let mut current_value = String::new();
    let mut depth = 0;
    let mut property_depth = 0;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;

                let tag_name = extract_tag_name(&e)?;

                // Check if this is an rdf:Description element
                if is_rdf_description(&tag_name, &resolver) {
                    inside_description = true;
                    // Register any new namespaces from attributes
                    register_namespaces_from_element(&e, &mut resolver)?;
                } else if inside_description && current_property.is_none() {
                    // This is a property element inside rdf:Description
                    // Check if it's a complex structure we should skip
                    if is_simple_property(&tag_name, &resolver) {
                        current_property = Some(tag_name.to_string());
                        current_value.clear();
                        property_depth = depth;
                    }
                    // Register any new namespaces
                    register_namespaces_from_element(&e, &mut resolver)?;
                }
            }

            Ok(Event::End(e)) => {
                let tag_name = extract_tag_name_from_bytes(e.name().as_ref())?;

                if is_rdf_description(&tag_name, &resolver) {
                    inside_description = false;
                } else if let Some(ref prop) = current_property {
                    if depth == property_depth {
                        // End of current property - extract tag name and value
                        if !current_value.trim().is_empty() {
                            let prefixed_name = format_tag_name(prop, &resolver);
                            results.push((prefixed_name, current_value.trim().to_string()));
                        }
                        current_property = None;
                        current_value.clear();
                    }
                }
                depth -= 1;
            }

            Ok(Event::Text(e)) => {
                // Collect text content if we're inside a property
                if current_property.is_some() {
                    if let Ok(text) = e.unescape() {
                        current_value.push_str(&text);
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                // Handle self-closing tags like <xmp:Rating>5</xmp:Rating>
                // that might have text as an attribute
                let tag_name = extract_tag_name(&e)?;

                if inside_description && is_simple_property(&tag_name, &resolver) {
                    // For empty tags, check if there's an rdf:value attribute
                    // or other value-bearing attributes
                    // For now, we skip empty tags as they typically don't have simple string values
                    register_namespaces_from_element(&e, &mut resolver)?;
                }
            }

            Ok(Event::Eof) => break,

            Ok(_) => {} // Ignore other events (comments, PI, etc.)

            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid XMP XML structure: {}",
                    e
                )));
            }
        }

        buf.clear();
    }

    Ok(results)
}

/// Extracts the tag name from a BytesStart event.
fn extract_tag_name(element: &BytesStart) -> Result<String> {
    let name = element.name();
    let name_str = std::str::from_utf8(name.as_ref())
        .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8 in tag name: {}", e)))?;
    Ok(name_str.to_string())
}

/// Extracts the tag name from any element (helper for End events).
fn extract_tag_name_from_bytes(name_bytes: &[u8]) -> Result<String> {
    let name_str = std::str::from_utf8(name_bytes)
        .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8 in tag name: {}", e)))?;
    Ok(name_str.to_string())
}

/// Checks if a tag name represents an rdf:Description element.
fn is_rdf_description(tag_name: &str, resolver: &NamespaceResolver) -> bool {
    if let Some(prefix) = NamespaceResolver::extract_prefix(tag_name) {
        let local_name = NamespaceResolver::extract_local_name(tag_name);
        if local_name == "Description" {
            if let Some(uri) = resolver.resolve_prefix(prefix) {
                return uri == "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
            }
        }
    }
    false
}

/// Checks if a property is a simple property (not a complex structure).
///
/// We skip complex RDF structures like:
/// - rdf:Bag, rdf:Seq, rdf:Alt (collections)
/// - Nested rdf:Description (structs)
fn is_simple_property(tag_name: &str, resolver: &NamespaceResolver) -> bool {
    if let Some(prefix) = NamespaceResolver::extract_prefix(tag_name) {
        let local_name = NamespaceResolver::extract_local_name(tag_name);

        // Check if it's an RDF namespace element
        if let Some(uri) = resolver.resolve_prefix(prefix) {
            if uri == "http://www.w3.org/1999/02/22-rdf-syntax-ns#" {
                // Skip RDF structural elements
                return !matches!(
                    local_name,
                    "Bag" | "Seq" | "Alt" | "Description" | "RDF" | "li"
                );
            }
        }

        // It's a property in a non-RDF namespace (xmp, dc, exif, etc.)
        return true;
    }

    // No namespace prefix - treat as simple property
    true
}

/// Registers namespace declarations from an element's attributes.
fn register_namespaces_from_element(
    element: &BytesStart,
    resolver: &mut NamespaceResolver,
) -> Result<()> {
    for attr in element.attributes().flatten() {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in attribute key: {}", e))
        })?;

        // Check for xmlns:prefix="uri" declarations
        if let Some(prefix) = key.strip_prefix("xmlns:") {
            let uri = std::str::from_utf8(&attr.value).map_err(|e| {
                ExifToolError::parse_error(format!("Invalid UTF-8 in namespace URI: {}", e))
            })?;

            resolver.register_namespace(prefix, uri);
        } else if key == "xmlns" {
            // Default namespace
            let uri = std::str::from_utf8(&attr.value).map_err(|e| {
                ExifToolError::parse_error(format!("Invalid UTF-8 in default namespace URI: {}", e))
            })?;
            resolver.register_namespace("", uri);
        }
    }
    Ok(())
}

/// Formats a tag name with namespace-specific "XMP-prefix:" output to match Perl ExifTool.
///
/// XMP properties are returned with namespace-specific prefixes:
/// - xmp:Creator → XMP-xmp:Creator
/// - dc:title → XMP-dc:Title (Dublin Core uses Title-case)
/// - exif:Make → XMP-exif:Make
fn format_tag_name(qname: &str, _resolver: &NamespaceResolver) -> String {
    let mut local_name = NamespaceResolver::extract_local_name(qname).to_string();

    // Extract namespace prefix from the qualified name
    if let Some(prefix) = NamespaceResolver::extract_prefix(qname) {
        // Dublin Core (dc) namespace uses Title-case for property names
        // Convert first letter to uppercase for dc: elements to match Perl ExifTool
        if prefix == "dc" && !local_name.is_empty() {
            // Capitalize first letter
            local_name = capitalize_first_letter(&local_name);
        }

        // Use "XMP-{prefix}:{localName}" format to match Perl ExifTool exactly
        format!("XMP-{}:{}", prefix, local_name)
    } else {
        // No namespace prefix - use generic "XMP:" prefix
        format!("XMP:{}", local_name)
    }
}

/// Capitalizes the first letter of a string
fn capitalize_first_letter(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_xmp() {
        let xml = br#"
            <x:xmpmeta xmlns:x="adobe:ns:meta/">
              <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
                <rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/">
                  <xmp:Creator>John Doe</xmp:Creator>
                  <xmp:Rating>5</xmp:Rating>
                </rdf:Description>
              </rdf:RDF>
            </x:xmpmeta>
        "#;

        let result = parse_xmp(xml).unwrap();
        assert!(
            result.len() >= 2,
            "Expected at least 2 properties, got {}",
            result.len()
        );

        // Check that Creator and Rating are present with namespace-specific prefixes
        let creators: Vec<_> = result
            .iter()
            .filter(|(name, _)| name == "XMP-xmp:Creator")
            .collect();
        assert_eq!(creators.len(), 1);
        assert_eq!(creators[0].1, "John Doe");

        let ratings: Vec<_> = result
            .iter()
            .filter(|(name, _)| name == "XMP-xmp:Rating")
            .collect();
        assert_eq!(ratings.len(), 1);
        assert_eq!(ratings[0].1, "5");
    }

    #[test]
    fn test_parse_multiple_namespaces() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmp="http://ns.adobe.com/xap/1.0/"
                     xmlns:dc="http://purl.org/dc/elements/1.1/"
                     xmlns:exif="http://ns.adobe.com/exif/1.0/">
              <rdf:Description>
                <xmp:Creator>Jane Smith</xmp:Creator>
                <dc:title>My Photo</dc:title>
                <dc:rights>Copyright 2024</dc:rights>
                <exif:Make>Canon</exif:Make>
                <exif:Model>EOS R5</exif:Model>
                <xmp:ModifyDate>2024-01-15</xmp:ModifyDate>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        assert!(
            result.len() >= 5,
            "Expected at least 5 properties, got {}",
            result.len()
        );

        // Verify properties from all 3 namespaces (xmp, dc, exif)
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();

        // Check for xmp properties with namespace-specific prefixes
        assert!(
            prop_names.iter().any(|n| n == "XMP-xmp:Creator"),
            "Missing XMP-xmp:Creator"
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP-xmp:ModifyDate"),
            "Missing XMP-xmp:ModifyDate"
        );

        // Check for dc properties (Dublin Core uses Title-case)
        assert!(
            prop_names.iter().any(|n| n == "XMP-dc:Title"),
            "Missing XMP-dc:Title (dc:title)"
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP-dc:Rights"),
            "Missing XMP-dc:Rights (dc:rights)"
        );

        // Check for exif properties
        assert!(
            prop_names.iter().any(|n| n == "XMP-exif:Make"),
            "Missing XMP-exif:Make (exif:Make)"
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP-exif:Model"),
            "Missing XMP-exif:Model (exif:Model)"
        );
    }

    #[test]
    fn test_malformed_xml_returns_error() {
        // quick-xml is lenient with structure, but will fail on invalid UTF-8 in tag names
        // Create XML with invalid UTF-8 sequence in a tag name
        let mut xml = Vec::new();
        xml.extend_from_slice(b"<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description><");
        xml.push(0xFF); // Invalid UTF-8 start byte
        xml.push(0xFE); // Invalid UTF-8 continuation
        xml.extend_from_slice(b":test>value</test></rdf:Description></rdf:RDF>");

        let result = parse_xmp(&xml);

        // Should error due to invalid UTF-8 in tag name
        assert!(
            result.is_err(),
            "Expected error for malformed XML with invalid UTF-8"
        );

        // Verify we got a ParseError
        match result {
            Err(ExifToolError::ParseError { .. }) => {
                // Success - got the expected error type
            }
            Ok(_) => panic!("Expected error for malformed XML, got Ok"),
            Err(e) => panic!("Expected ParseError, got {:?}", e),
        }
    }

    #[test]
    fn test_empty_xml_returns_empty_vector() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description />
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_skip_complex_structures() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:dc="http://purl.org/dc/elements/1.1/">
              <rdf:Description>
                <dc:creator>Simple Creator</dc:creator>
                <dc:subject>
                  <rdf:Bag>
                    <rdf:li>keyword1</rdf:li>
                    <rdf:li>keyword2</rdf:li>
                  </rdf:Bag>
                </dc:subject>
                <dc:title>Simple Title</dc:title>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();

        // Should have simple properties but not the complex Bag structure
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();
        assert!(prop_names.iter().any(|n| n == "XMP-dc:Creator"));
        assert!(prop_names.iter().any(|n| n == "XMP-dc:Title"));

        // The Bag contents should not be present as individual items
        assert!(!prop_names.iter().any(|n| n.contains("keyword")));
    }

    #[test]
    fn test_whitespace_trimming() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmp="http://ns.adobe.com/xap/1.0/">
              <rdf:Description>
                <xmp:Creator>
                  John Doe
                </xmp:Creator>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            ("XMP-xmp:Creator".to_string(), "John Doe".to_string())
        );
    }

    #[test]
    fn test_utf8_content() {
        // Use a regular string literal and convert to bytes to support UTF-8
        let xml = r#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:dc="http://purl.org/dc/elements/1.1/">
              <rdf:Description>
                <dc:creator>José García</dc:creator>
                <dc:title>Ñandú en la Patagonia</dc:title>
                <dc:rights>版权所有 2024</dc:rights>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml.as_bytes()).unwrap();
        assert_eq!(result.len(), 3);

        // Verify UTF-8 is preserved
        assert!(result.iter().any(|(_, v)| v.contains("José García")));
        assert!(result.iter().any(|(_, v)| v.contains("Ñandú")));
        assert!(result.iter().any(|(_, v)| v.contains("版权所有")));
    }

    #[test]
    fn test_multiple_descriptions() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmp="http://ns.adobe.com/xap/1.0/"
                     xmlns:dc="http://purl.org/dc/elements/1.1/">
              <rdf:Description>
                <xmp:Creator>First Creator</xmp:Creator>
              </rdf:Description>
              <rdf:Description>
                <dc:title>First Title</dc:title>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        assert_eq!(result.len(), 2);

        // Should handle properties from both Description blocks
        let creators: Vec<_> = result
            .iter()
            .filter(|(name, _)| name == "XMP-xmp:Creator")
            .collect();
        assert_eq!(creators.len(), 1);

        let titles: Vec<_> = result
            .iter()
            .filter(|(name, _)| name == "XMP-dc:Title")
            .collect();
        assert_eq!(titles.len(), 1);
    }
}
