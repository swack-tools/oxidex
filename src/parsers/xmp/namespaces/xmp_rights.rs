//! XMP Rights Management (xmpRights) namespace handler
//!
//! Extracts copyright and usage rights metadata from the xmpRights namespace.
//!
//! **Namespace URI:** http://ns.adobe.com/xap/1.0/rights/
//!
//! **Key properties:**
//! - WebStatement: URL for rights information
//! - Owner: Copyright owner(s)
//! - Marked: Copyright status (True/False)
//! - UsageTerms: License terms and conditions
//! - Certificate: Digital certificate URL

use crate::error::Result;
use crate::parsers::xmp::namespaces::{
    extract_alt_value, extract_bag_values, extract_text_content,
};
use quick_xml::events::Event;
use quick_xml::Reader;

/// Extract xmpRights namespace values from XMP data
///
/// # Parameters
///
/// - `xml_bytes`: Raw XMP XML data
///
/// # Returns
///
/// Vector of (tag_name, value) pairs with XMP-xmpRights: prefix
pub fn extract_xmp_rights_values(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut results = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                // Handle xmpRights properties
                if tag_name.contains("WebStatement") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpRights:WebStatement".to_string(), value));
                        }
                    }
                } else if tag_name.contains("Marked") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpRights:Marked".to_string(), value));
                        }
                    }
                } else if tag_name.contains("Certificate") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-xmpRights:Certificate".to_string(), value));
                        }
                    }
                } else if tag_name.contains("Owner") {
                    // Owner can be a Bag (multiple owners)
                    if let Ok(Event::Start(inner)) = reader.read_event_into(&mut buf) {
                        let inner_tag = std::str::from_utf8(inner.name().as_ref()).unwrap_or("");
                        if inner_tag.ends_with("Bag") {
                            if let Ok(owners) = extract_bag_values(&mut reader, &mut buf) {
                                for owner in owners {
                                    results.push(("XMP-xmpRights:Owner".to_string(), owner));
                                }
                            }
                        }
                    }
                } else if tag_name.contains("UsageTerms") {
                    // UsageTerms can be Alt (language alternatives)
                    if let Ok(Event::Start(inner)) = reader.read_event_into(&mut buf) {
                        let inner_tag = std::str::from_utf8(inner.name().as_ref()).unwrap_or("");
                        if inner_tag.ends_with("Alt") {
                            if let Ok(value) = extract_alt_value(&mut reader, &mut buf) {
                                if !value.is_empty() {
                                    results.push(("XMP-xmpRights:UsageTerms".to_string(), value));
                                }
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_rights() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/">
              <rdf:Description>
                <xmpRights:WebStatement>https://example.com/rights</xmpRights:WebStatement>
                <xmpRights:Marked>True</xmpRights:Marked>
                <xmpRights:Certificate>https://example.com/cert</xmpRights:Certificate>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_xmp_rights_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpRights:WebStatement" && v == "https://example.com/rights"));
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpRights:Marked" && v == "True"));
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpRights:Certificate" && v == "https://example.com/cert"));
    }

    #[test]
    fn test_extract_owner_bag() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/">
              <rdf:Description>
                <xmpRights:Owner>
                  <rdf:Bag>
                    <rdf:li>John Doe</rdf:li>
                    <rdf:li>Jane Smith</rdf:li>
                  </rdf:Bag>
                </xmpRights:Owner>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_xmp_rights_values(xml).unwrap();
        let owners: Vec<_> = result
            .iter()
            .filter(|(k, _)| k == "XMP-xmpRights:Owner")
            .map(|(_, v)| v.as_str())
            .collect();

        assert!(owners.contains(&"John Doe"));
        assert!(owners.contains(&"Jane Smith"));
    }

    #[test]
    fn test_extract_usage_terms_alt() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/">
              <rdf:Description>
                <xmpRights:UsageTerms>
                  <rdf:Alt>
                    <rdf:li xml:lang="x-default">Free for personal use</rdf:li>
                    <rdf:li xml:lang="en">Free for personal use</rdf:li>
                  </rdf:Alt>
                </xmpRights:UsageTerms>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_xmp_rights_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-xmpRights:UsageTerms" && v == "Free for personal use"));
    }
}
