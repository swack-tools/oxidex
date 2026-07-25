//! Adobe Lightroom (lr) namespace handler
//!
//! Extracts Lightroom-specific metadata including hierarchical keywords.
//!
//! **Namespace URI:** http://ns.adobe.com/lightroom/1.0/
//!
//! **Key properties:**
//! - hierarchicalSubject: Hierarchical keywords (Bag of pipe-delimited paths)
//! - privateRTKInfo: Lightroom's private metadata
//! - weightedFlatSubject: Weighted keyword suggestions

use crate::error::Result;
use crate::parsers::xmp::namespaces::{extract_bag_values, extract_text_content};
use quick_xml::events::Event;
use quick_xml::Reader;

/// Extract Lightroom namespace values from XMP data
///
/// # Parameters
///
/// - `xml_bytes`: Raw XMP XML data
///
/// # Returns
///
/// Vector of (tag_name, value) pairs with XMP-lr: prefix
pub fn extract_lightroom_values(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut results = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                // Handle lr properties
                if tag_name.contains("hierarchicalSubject") {
                    // hierarchicalSubject is a Bag of hierarchical keyword paths
                    if let Ok(Event::Start(inner)) = reader.read_event_into(&mut buf) {
                        let inner_tag = std::str::from_utf8(inner.name().as_ref()).unwrap_or("");
                        if inner_tag.ends_with("Bag") {
                            if let Ok(subjects) = extract_bag_values(&mut reader, &mut buf) {
                                for subject in subjects {
                                    results
                                        .push(("XMP-lr:HierarchicalSubject".to_string(), subject));
                                }
                            }
                        }
                    }
                } else if tag_name.contains("privateRTKInfo") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-lr:PrivateRTKInfo".to_string(), value));
                        }
                    }
                } else if tag_name.contains("weightedFlatSubject") {
                    // weightedFlatSubject is a Bag of weighted keywords
                    if let Ok(Event::Start(inner)) = reader.read_event_into(&mut buf) {
                        let inner_tag = std::str::from_utf8(inner.name().as_ref()).unwrap_or("");
                        if inner_tag.ends_with("Bag") {
                            if let Ok(subjects) = extract_bag_values(&mut reader, &mut buf) {
                                for subject in subjects {
                                    results
                                        .push(("XMP-lr:WeightedFlatSubject".to_string(), subject));
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
    fn test_extract_hierarchical_subject() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:lr="http://ns.adobe.com/lightroom/1.0/">
              <rdf:Description>
                <lr:hierarchicalSubject>
                  <rdf:Bag>
                    <rdf:li>Nature|Landscape|Mountains</rdf:li>
                    <rdf:li>Nature|Wildlife|Birds</rdf:li>
                    <rdf:li>Places|USA|Colorado</rdf:li>
                  </rdf:Bag>
                </lr:hierarchicalSubject>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_lightroom_values(xml).unwrap();
        let subjects: Vec<_> = result
            .iter()
            .filter(|(k, _)| k == "XMP-lr:HierarchicalSubject")
            .map(|(_, v)| v.as_str())
            .collect();

        assert_eq!(subjects.len(), 3);
        assert!(subjects.contains(&"Nature|Landscape|Mountains"));
        assert!(subjects.contains(&"Nature|Wildlife|Birds"));
        assert!(subjects.contains(&"Places|USA|Colorado"));
    }

    #[test]
    fn test_extract_weighted_flat_subject() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:lr="http://ns.adobe.com/lightroom/1.0/">
              <rdf:Description>
                <lr:weightedFlatSubject>
                  <rdf:Bag>
                    <rdf:li>mountains</rdf:li>
                    <rdf:li>landscape</rdf:li>
                  </rdf:Bag>
                </lr:weightedFlatSubject>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_lightroom_values(xml).unwrap();
        let subjects: Vec<_> = result
            .iter()
            .filter(|(k, _)| k == "XMP-lr:WeightedFlatSubject")
            .map(|(_, v)| v.as_str())
            .collect();

        assert!(subjects.contains(&"mountains"));
        assert!(subjects.contains(&"landscape"));
    }

    #[test]
    fn test_extract_private_rtk_info() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:lr="http://ns.adobe.com/lightroom/1.0/">
              <rdf:Description>
                <lr:privateRTKInfo>some_private_data</lr:privateRTKInfo>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_lightroom_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-lr:PrivateRTKInfo" && v == "some_private_data"));
    }
}
