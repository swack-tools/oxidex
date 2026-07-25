//! PLUS (Picture Licensing Universal System) namespace handler
//!
//! Extracts image licensing and rights management metadata from the PLUS namespace.
//!
//! **Namespace URI:** http://ns.useplus.org/ldf/xmp/1.0/
//!
//! **Key properties:**
//! - ImageSupplier: Information about the image supplier
//! - Licensor: Licensing organization details (Seq of structs)
//! - LicensorName: Name of the licensor
//! - ImageCreator: Creator/photographer details
//! - CopyrightOwner: Copyright holder details

use crate::error::Result;
use crate::parsers::xmp::namespaces::{extract_seq_values, extract_text_content};
use quick_xml::events::Event;
use quick_xml::Reader;

/// Extract PLUS namespace values from XMP data
///
/// # Parameters
///
/// - `xml_bytes`: Raw XMP XML data
///
/// # Returns
///
/// Vector of (tag_name, value) pairs with XMP-plus: prefix
pub fn extract_plus_values(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut results = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                // Handle plus properties
                if tag_name.contains("ImageSupplierName") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-plus:ImageSupplierName".to_string(), value));
                        }
                    }
                } else if tag_name.contains("ImageSupplierID") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-plus:ImageSupplierID".to_string(), value));
                        }
                    }
                } else if tag_name.contains("ImageSupplierImageID") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-plus:ImageSupplierImageID".to_string(), value));
                        }
                    }
                } else if tag_name.contains("Licensor")
                    && !tag_name.contains("Name")
                    && !tag_name.contains("URL")
                {
                    // Licensor is a Seq of structs
                    if let Ok(licensor_values) = extract_licensor(&mut reader, &mut buf) {
                        results.extend(licensor_values);
                    }
                } else if tag_name.contains("ImageCreatorName") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-plus:ImageCreatorName".to_string(), value));
                        }
                    }
                } else if tag_name.contains("ImageCreatorID") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-plus:ImageCreatorID".to_string(), value));
                        }
                    }
                } else if tag_name.contains("CopyrightOwnerName") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-plus:CopyrightOwnerName".to_string(), value));
                        }
                    }
                } else if tag_name.contains("CopyrightOwnerID") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-plus:CopyrightOwnerID".to_string(), value));
                        }
                    }
                } else if tag_name.contains("LicensorName") && !tag_name.contains("Licensor:") {
                    // Simple LicensorName (not in Licensor struct)
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-plus:LicensorName".to_string(), value));
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

/// Extract Licensor struct values from Seq
fn extract_licensor(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    let mut depth = 1;
    let mut licensor_num = 0;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                if tag_name.ends_with("li") {
                    licensor_num += 1;
                } else if tag_name.contains("LicensorName") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            if licensor_num > 0 {
                                results.push((
                                    format!("XMP-plus:Licensor{}Name", licensor_num),
                                    value,
                                ));
                            } else {
                                results.push(("XMP-plus:LicensorName".to_string(), value));
                            }
                        }
                    }
                } else if tag_name.contains("LicensorURL") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            if licensor_num > 0 {
                                results
                                    .push((format!("XMP-plus:Licensor{}URL", licensor_num), value));
                            } else {
                                results.push(("XMP-plus:LicensorURL".to_string(), value));
                            }
                        }
                    }
                } else if tag_name.contains("LicensorID") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            if licensor_num > 0 {
                                results
                                    .push((format!("XMP-plus:Licensor{}ID", licensor_num), value));
                            } else {
                                results.push(("XMP-plus:LicensorID".to_string(), value));
                            }
                        }
                    }
                } else if tag_name.contains("LicensorEmail") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            if licensor_num > 0 {
                                results.push((
                                    format!("XMP-plus:Licensor{}Email", licensor_num),
                                    value,
                                ));
                            } else {
                                results.push(("XMP-plus:LicensorEmail".to_string(), value));
                            }
                        }
                    }
                } else if tag_name.contains("LicensorTelephone") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            if licensor_num > 0 {
                                results.push((
                                    format!("XMP-plus:Licensor{}Telephone", licensor_num),
                                    value,
                                ));
                            } else {
                                results.push(("XMP-plus:LicensorTelephone".to_string(), value));
                            }
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
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
    fn test_extract_image_supplier() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:plus="http://ns.useplus.org/ldf/xmp/1.0/">
              <rdf:Description>
                <plus:ImageSupplierName>Getty Images</plus:ImageSupplierName>
                <plus:ImageSupplierID>12345</plus:ImageSupplierID>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_plus_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-plus:ImageSupplierName" && v == "Getty Images"));
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-plus:ImageSupplierID" && v == "12345"));
    }

    #[test]
    fn test_extract_image_creator() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:plus="http://ns.useplus.org/ldf/xmp/1.0/">
              <rdf:Description>
                <plus:ImageCreatorName>John Photographer</plus:ImageCreatorName>
                <plus:ImageCreatorID>photographer123</plus:ImageCreatorID>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_plus_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-plus:ImageCreatorName" && v == "John Photographer"));
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-plus:ImageCreatorID" && v == "photographer123"));
    }

    #[test]
    fn test_extract_copyright_owner() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:plus="http://ns.useplus.org/ldf/xmp/1.0/">
              <rdf:Description>
                <plus:CopyrightOwnerName>Acme Corp</plus:CopyrightOwnerName>
                <plus:CopyrightOwnerID>acme-001</plus:CopyrightOwnerID>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_plus_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-plus:CopyrightOwnerName" && v == "Acme Corp"));
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-plus:CopyrightOwnerID" && v == "acme-001"));
    }
}
