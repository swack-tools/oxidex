//! XMP Edit History parser for forensic tamper detection
//!
//! This module extracts edit history metadata from XMP data, which is critical
//! for detecting image manipulation in forensic analysis.
//!
//! # XMP History Structure
//!
//! Edit history is stored in the xmpMM (Media Management) namespace:
//! ```xml
//! <xmpMM:History>
//!   <rdf:Seq>
//!     <rdf:li rdf:parseType="Resource">
//!       <stEvt:action>created</stEvt:action>
//!       <stEvt:when>2023-01-15T10:30:00+05:30</stEvt:when>
//!       <stEvt:softwareAgent>Adobe Photoshop 24.0</stEvt:softwareAgent>
//!     </rdf:li>
//!   </rdf:Seq>
//! </xmpMM:History>
//! ```
//!
//! # Extracted Tags
//!
//! - HistoryAction - Edit action (saved, created, converted, derived)
//! - HistoryWhen - ISO 8601 timestamp of edit
//! - HistorySoftwareAgent - Software that made the edit
//! - HistoryChanged - What was changed (/metadata, /content)
//! - HistoryInstanceID - Version instance ID
//! - HistoryParameters - Action parameters
//! - DerivedFrom* tags - Source document references
//! - Document/Instance IDs - Version tracking

use crate::error::{ExifToolError, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// Represents a single XMP history event
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XmpHistoryEntry {
    /// Edit action (saved, created, converted, derived)
    pub action: Option<String>,
    /// ISO 8601 timestamp of edit
    pub when: Option<String>,
    /// Software that made the edit
    pub software_agent: Option<String>,
    /// What was changed (/metadata, /content)
    pub changed: Option<String>,
    /// Version instance ID
    pub instance_id: Option<String>,
    /// Action parameters
    pub parameters: Option<String>,
}

/// Parse XMP edit history from XMP data string
///
/// Extracts edit history from xmpMM:History arrays and related metadata.
///
/// # Parameters
///
/// - `xmp_data`: Raw XMP XML data as string
///
/// # Returns
///
/// Vector of (tag_name, value) pairs with history metadata in numbered format:
/// - XMP-xmpMM:HistoryCount
/// - XMP-xmpMM:History1Action, XMP-xmpMM:History1When, etc.
/// - XMP-xmpMM:DocumentID, XMP-xmpMM:InstanceID, etc.
///
/// # Example
///
/// ```no_run
/// use oxidex::parsers::xmp::history_parser::parse_xmp_history;
///
/// let xmp = r#"<xmpMM:History>
///   <rdf:Seq>
///     <rdf:li rdf:parseType="Resource">
///       <stEvt:action>created</stEvt:action>
///       <stEvt:when>2023-01-15T10:30:00</stEvt:when>
///     </rdf:li>
///   </rdf:Seq>
/// </xmpMM:History>"#;
///
/// let history = parse_xmp_history(xmp).unwrap();
/// ```
pub fn parse_xmp_history(xmp_data: &str) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_str(xmp_data);
    reader.config_mut().trim_text(true);

    let mut results = Vec::new();
    let mut history_entries: Vec<XmpHistoryEntry> = Vec::new();
    let mut buf = Vec::new();

    // State tracking
    let mut in_history = false;
    let mut in_seq = false;
    let mut in_li = false;
    let mut current_entry = XmpHistoryEntry::default();
    let mut current_property: Option<String> = None;
    let mut current_value = String::new();

    // Simple document ID tracking
    let mut in_description = false;
    let mut in_derived_from = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag_name = extract_tag_name(e)?;

                // Track xmpMM:History
                if is_tag(&tag_name, "xmpMM", "History") {
                    in_history = true;
                }
                // Track rdf:Seq inside History
                else if in_history && is_tag(&tag_name, "rdf", "Seq") {
                    in_seq = true;
                }
                // Track rdf:li (list items - history entries)
                else if in_seq && is_tag(&tag_name, "rdf", "li") {
                    in_li = true;
                    current_entry = XmpHistoryEntry::default();
                }
                // Track stEvt: properties inside rdf:li
                else if in_li && is_namespace(&tag_name, "stEvt") {
                    current_property = Some(get_local_name(&tag_name));
                    current_value.clear();
                }
                // Track rdf:Description for top-level properties
                else if is_tag(&tag_name, "rdf", "Description") {
                    in_description = true;
                }
                // Track xmpMM:DerivedFrom
                else if is_tag(&tag_name, "xmpMM", "DerivedFrom") {
                    in_derived_from = true;
                }
                // Track stRef: properties inside DerivedFrom
                else if in_derived_from && is_namespace(&tag_name, "stRef") {
                    current_property = Some(format!(
                        "DerivedFrom{}",
                        capitalize(&get_local_name(&tag_name))
                    ));
                    current_value.clear();
                }
                // Track top-level xmpMM properties
                else if in_description && is_namespace(&tag_name, "xmpMM") {
                    let local = get_local_name(&tag_name);
                    if local == "OriginalDocumentID"
                        || local == "DocumentID"
                        || local == "InstanceID"
                    {
                        current_property = Some(local);
                        current_value.clear();
                    }
                }
            }

            Ok(Event::End(ref e)) => {
                let tag_name = extract_tag_name_from_end(e.name().as_ref())?;

                if is_tag(&tag_name, "xmpMM", "History") {
                    in_history = false;
                } else if is_tag(&tag_name, "rdf", "Seq") {
                    in_seq = false;
                } else if is_tag(&tag_name, "rdf", "li") {
                    in_li = false;
                    // Save completed history entry
                    history_entries.push(current_entry.clone());
                } else if is_tag(&tag_name, "rdf", "Description") {
                    in_description = false;
                } else if is_tag(&tag_name, "xmpMM", "DerivedFrom") {
                    in_derived_from = false;
                } else if let Some(ref prop) = current_property {
                    // Save property value
                    let value = current_value.trim().to_string();
                    if !value.is_empty() {
                        if in_li {
                            // Save to current history entry
                            match prop.as_str() {
                                "action" => current_entry.action = Some(value.clone()),
                                "when" => current_entry.when = Some(value.clone()),
                                "softwareAgent" => {
                                    current_entry.software_agent = Some(value.clone())
                                }
                                "changed" => current_entry.changed = Some(value.clone()),
                                "instanceID" => current_entry.instance_id = Some(value.clone()),
                                "parameters" => current_entry.parameters = Some(value.clone()),
                                _ => {}
                            }
                        } else {
                            // Top-level property - add directly to results
                            results.push((format!("XMP-xmpMM:{}", prop), value));
                        }
                    }
                    current_property = None;
                    current_value.clear();
                }
            }

            Ok(Event::Text(e)) => {
                if current_property.is_some() {
                    current_value.push_str(&String::from_utf8_lossy(&e));
                }
            }

            Ok(Event::Empty(ref e)) => {
                let tag_name = extract_tag_name(e)?;

                // Handle self-closing tags
                if is_tag(&tag_name, "xmpMM", "History") {
                    // Empty history tag - no entries
                } else if in_description && is_namespace(&tag_name, "xmpMM") {
                    // Check for rdf:resource attribute for top-level properties
                    if let Some(value) = get_attribute(e, "rdf:resource") {
                        let local = get_local_name(&tag_name);
                        if local == "OriginalDocumentID"
                            || local == "DocumentID"
                            || local == "InstanceID"
                        {
                            results.push((format!("XMP-xmpMM:{}", local), value));
                        }
                    }
                } else if in_derived_from && is_namespace(&tag_name, "stRef") {
                    // Check for rdf:resource attribute in DerivedFrom
                    if let Some(value) = get_attribute(e, "rdf:resource") {
                        let prop = format!("DerivedFrom{}", capitalize(&get_local_name(&tag_name)));
                        results.push((format!("XMP-xmpMM:{}", prop), value));
                    }
                }
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid XMP history XML: {}",
                    e
                )));
            }
        }

        buf.clear();
    }

    // Add history count
    if !history_entries.is_empty() {
        results.push((
            "XMP-xmpMM:HistoryCount".to_string(),
            history_entries.len().to_string(),
        ));

        // Add numbered history entries
        for (idx, entry) in history_entries.iter().enumerate() {
            let num = idx + 1;

            if let Some(ref action) = entry.action {
                results.push((format!("XMP-xmpMM:History{}Action", num), action.clone()));
            }
            if let Some(ref when) = entry.when {
                results.push((format!("XMP-xmpMM:History{}When", num), when.clone()));
            }
            if let Some(ref agent) = entry.software_agent {
                results.push((
                    format!("XMP-xmpMM:History{}SoftwareAgent", num),
                    agent.clone(),
                ));
            }
            if let Some(ref changed) = entry.changed {
                results.push((format!("XMP-xmpMM:History{}Changed", num), changed.clone()));
            }
            if let Some(ref instance_id) = entry.instance_id {
                results.push((
                    format!("XMP-xmpMM:History{}InstanceID", num),
                    instance_id.clone(),
                ));
            }
            if let Some(ref parameters) = entry.parameters {
                results.push((
                    format!("XMP-xmpMM:History{}Parameters", num),
                    parameters.clone(),
                ));
            }
        }
    }

    Ok(results)
}

/// Extract tag name from BytesStart event
fn extract_tag_name(element: &BytesStart) -> Result<String> {
    let name = element.name();
    let name_str = std::str::from_utf8(name.as_ref())
        .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8 in tag name: {}", e)))?;
    Ok(name_str.to_string())
}

/// Extract tag name from Event::End name bytes
fn extract_tag_name_from_end(name_bytes: &[u8]) -> Result<String> {
    let name_str = std::str::from_utf8(name_bytes)
        .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8 in tag name: {}", e)))?;
    Ok(name_str.to_string())
}

/// Check if tag matches namespace:local pattern
fn is_tag(tag_name: &str, namespace: &str, local_name: &str) -> bool {
    tag_name == format!("{}:{}", namespace, local_name)
}

/// Check if tag belongs to a namespace
fn is_namespace(tag_name: &str, namespace: &str) -> bool {
    tag_name.starts_with(&format!("{}:", namespace))
}

/// Get local name from qualified name (xmpMM:History -> History)
fn get_local_name(qname: &str) -> String {
    qname.split(':').nth(1).unwrap_or(qname).to_string()
}

/// Capitalize first letter
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Get attribute value from element
fn get_attribute(element: &BytesStart, attr_name: &str) -> Option<String> {
    for attr in element.attributes().flatten() {
        if let Ok(key) = std::str::from_utf8(attr.key.as_ref())
            && key == attr_name
            && let Ok(value) = std::str::from_utf8(&attr.value)
        {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_history() {
        let xmp = r#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
                     xmlns:stEvt="http://ns.adobe.com/xap/1.0/sType/ResourceEvent#">
              <rdf:Description>
                <xmpMM:History>
                  <rdf:Seq>
                    <rdf:li rdf:parseType="Resource">
                      <stEvt:action>created</stEvt:action>
                      <stEvt:when>2023-01-15T10:30:00+05:30</stEvt:when>
                      <stEvt:softwareAgent>Adobe Photoshop 24.0</stEvt:softwareAgent>
                    </rdf:li>
                  </rdf:Seq>
                </xmpMM:History>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp_history(xmp).unwrap();

        // Should have HistoryCount + 3 properties (action, when, softwareAgent)
        assert!(
            result.len() >= 4,
            "Expected at least 4 tags, got {}",
            result.len()
        );

        // Check HistoryCount
        let count = result.iter().find(|(k, _)| k == "XMP-xmpMM:HistoryCount");
        assert_eq!(count.map(|(_, v)| v.as_str()), Some("1"));

        // Check history entry
        let action = result.iter().find(|(k, _)| k == "XMP-xmpMM:History1Action");
        assert_eq!(action.map(|(_, v)| v.as_str()), Some("created"));

        let when = result.iter().find(|(k, _)| k == "XMP-xmpMM:History1When");
        assert_eq!(
            when.map(|(_, v)| v.as_str()),
            Some("2023-01-15T10:30:00+05:30")
        );

        let agent = result
            .iter()
            .find(|(k, _)| k == "XMP-xmpMM:History1SoftwareAgent");
        assert_eq!(agent.map(|(_, v)| v.as_str()), Some("Adobe Photoshop 24.0"));
    }

    #[test]
    fn test_parse_multiple_history_entries() {
        let xmp = r#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
                     xmlns:stEvt="http://ns.adobe.com/xap/1.0/sType/ResourceEvent#">
              <rdf:Description>
                <xmpMM:History>
                  <rdf:Seq>
                    <rdf:li rdf:parseType="Resource">
                      <stEvt:action>created</stEvt:action>
                      <stEvt:when>2023-01-15T10:30:00</stEvt:when>
                    </rdf:li>
                    <rdf:li rdf:parseType="Resource">
                      <stEvt:action>saved</stEvt:action>
                      <stEvt:when>2023-01-15T11:45:00</stEvt:when>
                      <stEvt:changed>/content</stEvt:changed>
                    </rdf:li>
                  </rdf:Seq>
                </xmpMM:History>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp_history(xmp).unwrap();

        // Check HistoryCount
        let count = result.iter().find(|(k, _)| k == "XMP-xmpMM:HistoryCount");
        assert_eq!(count.map(|(_, v)| v.as_str()), Some("2"));

        // Check first entry
        let action1 = result.iter().find(|(k, _)| k == "XMP-xmpMM:History1Action");
        assert_eq!(action1.map(|(_, v)| v.as_str()), Some("created"));

        // Check second entry
        let action2 = result.iter().find(|(k, _)| k == "XMP-xmpMM:History2Action");
        assert_eq!(action2.map(|(_, v)| v.as_str()), Some("saved"));

        let changed2 = result
            .iter()
            .find(|(k, _)| k == "XMP-xmpMM:History2Changed");
        assert_eq!(changed2.map(|(_, v)| v.as_str()), Some("/content"));
    }

    #[test]
    fn test_parse_document_ids() {
        let xmp = r#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/">
              <rdf:Description>
                <xmpMM:DocumentID>xmp.did:12345</xmpMM:DocumentID>
                <xmpMM:InstanceID>xmp.iid:67890</xmpMM:InstanceID>
                <xmpMM:OriginalDocumentID>xmp.did:original</xmpMM:OriginalDocumentID>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp_history(xmp).unwrap();

        let doc_id = result.iter().find(|(k, _)| k == "XMP-xmpMM:DocumentID");
        assert_eq!(doc_id.map(|(_, v)| v.as_str()), Some("xmp.did:12345"));

        let inst_id = result.iter().find(|(k, _)| k == "XMP-xmpMM:InstanceID");
        assert_eq!(inst_id.map(|(_, v)| v.as_str()), Some("xmp.iid:67890"));

        let orig_id = result
            .iter()
            .find(|(k, _)| k == "XMP-xmpMM:OriginalDocumentID");
        assert_eq!(orig_id.map(|(_, v)| v.as_str()), Some("xmp.did:original"));
    }

    #[test]
    fn test_parse_derived_from() {
        let xmp = r#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
                     xmlns:stRef="http://ns.adobe.com/xap/1.0/sType/ResourceRef#">
              <rdf:Description>
                <xmpMM:DerivedFrom rdf:parseType="Resource">
                  <stRef:documentID>xmp.did:source123</stRef:documentID>
                  <stRef:instanceID>xmp.iid:source456</stRef:instanceID>
                  <stRef:originalDocumentID>xmp.did:source_original</stRef:originalDocumentID>
                </xmpMM:DerivedFrom>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp_history(xmp).unwrap();

        let derived_doc = result
            .iter()
            .find(|(k, _)| k == "XMP-xmpMM:DerivedFromDocumentID");
        assert_eq!(
            derived_doc.map(|(_, v)| v.as_str()),
            Some("xmp.did:source123")
        );

        let derived_inst = result
            .iter()
            .find(|(k, _)| k == "XMP-xmpMM:DerivedFromInstanceID");
        assert_eq!(
            derived_inst.map(|(_, v)| v.as_str()),
            Some("xmp.iid:source456")
        );

        let derived_orig = result
            .iter()
            .find(|(k, _)| k == "XMP-xmpMM:DerivedFromOriginalDocumentID");
        assert_eq!(
            derived_orig.map(|(_, v)| v.as_str()),
            Some("xmp.did:source_original")
        );
    }

    #[test]
    fn test_parse_all_history_fields() {
        let xmp = r#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
                     xmlns:stEvt="http://ns.adobe.com/xap/1.0/sType/ResourceEvent#">
              <rdf:Description>
                <xmpMM:History>
                  <rdf:Seq>
                    <rdf:li rdf:parseType="Resource">
                      <stEvt:action>converted</stEvt:action>
                      <stEvt:when>2023-01-15T10:30:00</stEvt:when>
                      <stEvt:softwareAgent>Adobe Photoshop</stEvt:softwareAgent>
                      <stEvt:changed>/metadata</stEvt:changed>
                      <stEvt:instanceID>xmp.iid:abc123</stEvt:instanceID>
                      <stEvt:parameters>from image/tiff to image/jpeg</stEvt:parameters>
                    </rdf:li>
                  </rdf:Seq>
                </xmpMM:History>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp_history(xmp).unwrap();

        assert!(
            result
                .iter()
                .any(|(k, v)| k == "XMP-xmpMM:History1Action" && v == "converted")
        );
        assert!(
            result
                .iter()
                .any(|(k, v)| k == "XMP-xmpMM:History1When" && v == "2023-01-15T10:30:00")
        );
        assert!(
            result
                .iter()
                .any(|(k, v)| k == "XMP-xmpMM:History1SoftwareAgent" && v == "Adobe Photoshop")
        );
        assert!(
            result
                .iter()
                .any(|(k, v)| k == "XMP-xmpMM:History1Changed" && v == "/metadata")
        );
        assert!(
            result
                .iter()
                .any(|(k, v)| k == "XMP-xmpMM:History1InstanceID" && v == "xmp.iid:abc123")
        );
        assert!(
            result
                .iter()
                .any(|(k, v)| k == "XMP-xmpMM:History1Parameters"
                    && v == "from image/tiff to image/jpeg")
        );
    }

    #[test]
    fn test_empty_history() {
        let xmp = r#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/">
              <rdf:Description>
                <xmpMM:History>
                  <rdf:Seq />
                </xmpMM:History>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp_history(xmp).unwrap();

        // Should not have HistoryCount if no entries
        let count = result.iter().find(|(k, _)| k == "XMP-xmpMM:HistoryCount");
        assert_eq!(count, None);
    }

    #[test]
    fn test_no_history() {
        let xmp = r#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/">
              <rdf:Description>
                <xmpMM:DocumentID>xmp.did:12345</xmpMM:DocumentID>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp_history(xmp).unwrap();

        // Should have DocumentID but no history
        let doc_id = result.iter().find(|(k, _)| k == "XMP-xmpMM:DocumentID");
        assert_eq!(doc_id.map(|(_, v)| v.as_str()), Some("xmp.did:12345"));

        let count = result.iter().find(|(k, _)| k == "XMP-xmpMM:HistoryCount");
        assert_eq!(count, None);
    }
}
