//! IPTC Extension (Iptc4xmpExt) namespace handler
//!
//! Extracts professional news and media workflow metadata from the IPTC Extension namespace.
//!
//! **Namespace URI:** http://iptc.org/std/Iptc4xmpExt/2008-02-29/
//!
//! **Key properties:**
//! - PersonInImage: People shown in the image (Bag)
//! - LocationShown: Geographic locations depicted (Struct with city, state, country)
//! - ArtworkOrObject: Artwork or cultural object details (Struct)
//! - OrganisationInImageName: Organizations shown
//! - Event: Event being covered

use crate::error::Result;
use crate::parsers::xmp::namespaces::{extract_bag_values, extract_text_content, skip_element};
use quick_xml::events::Event;
use quick_xml::Reader;

/// Extract Iptc4xmpExt namespace values from XMP data
///
/// # Parameters
///
/// - `xml_bytes`: Raw XMP XML data
///
/// # Returns
///
/// Vector of (tag_name, value) pairs with XMP-iptcExt: prefix
pub fn extract_iptc_ext_values(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut results = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                // Handle Iptc4xmpExt properties
                if tag_name.contains("PersonInImage") {
                    // PersonInImage is a Bag of names
                    if let Ok(Event::Start(inner)) = reader.read_event_into(&mut buf) {
                        let inner_tag = std::str::from_utf8(inner.name().as_ref()).unwrap_or("");
                        if inner_tag.ends_with("Bag") {
                            if let Ok(persons) = extract_bag_values(&mut reader, &mut buf) {
                                for person in persons {
                                    results.push(("XMP-iptcExt:PersonInImage".to_string(), person));
                                }
                            }
                        }
                    }
                } else if tag_name.contains("OrganisationInImageName") {
                    // OrganisationInImageName is a Bag
                    if let Ok(Event::Start(inner)) = reader.read_event_into(&mut buf) {
                        let inner_tag = std::str::from_utf8(inner.name().as_ref()).unwrap_or("");
                        if inner_tag.ends_with("Bag") {
                            if let Ok(orgs) = extract_bag_values(&mut reader, &mut buf) {
                                for org in orgs {
                                    results.push((
                                        "XMP-iptcExt:OrganisationInImageName".to_string(),
                                        org,
                                    ));
                                }
                            }
                        }
                    }
                } else if tag_name.contains("Event") && !tag_name.contains("action") {
                    if let Ok(value) = extract_text_content(&mut reader, &mut buf) {
                        if !value.is_empty() {
                            results.push(("XMP-iptcExt:Event".to_string(), value));
                        }
                    }
                } else if tag_name.contains("LocationShown") {
                    // LocationShown is a Bag of LocationDetails structs
                    if let Ok(location_values) = extract_location_shown(&mut reader, &mut buf) {
                        results.extend(location_values);
                    }
                } else if tag_name.contains("ArtworkOrObject") {
                    // ArtworkOrObject is a Bag of artwork detail structs
                    if let Ok(artwork_values) = extract_artwork_or_object(&mut reader, &mut buf) {
                        results.extend(artwork_values);
                    }
                } else if tag_name.contains("LocationCreated") {
                    // LocationCreated is similar to LocationShown
                    if let Ok(location_values) = extract_location_created(&mut reader, &mut buf) {
                        results.extend(location_values);
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

/// Extract LocationShown struct values
fn extract_location_shown(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    let mut depth = 1;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                if tag_name.contains("City") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push(("XMP-iptcExt:LocationShownCity".to_string(), value));
                        }
                    }
                } else if tag_name.contains("ProvinceState") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push((
                                "XMP-iptcExt:LocationShownProvinceState".to_string(),
                                value,
                            ));
                        }
                    }
                } else if tag_name.contains("CountryName") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results
                                .push(("XMP-iptcExt:LocationShownCountryName".to_string(), value));
                        }
                    }
                } else if tag_name.contains("CountryCode") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results
                                .push(("XMP-iptcExt:LocationShownCountryCode".to_string(), value));
                        }
                    }
                } else if tag_name.contains("WorldRegion") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results
                                .push(("XMP-iptcExt:LocationShownWorldRegion".to_string(), value));
                        }
                    }
                } else if tag_name.contains("Sublocation") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results
                                .push(("XMP-iptcExt:LocationShownSublocation".to_string(), value));
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

/// Extract LocationCreated struct values
fn extract_location_created(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    let mut depth = 1;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                if tag_name.contains("City") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push(("XMP-iptcExt:LocationCreatedCity".to_string(), value));
                        }
                    }
                } else if tag_name.contains("ProvinceState") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push((
                                "XMP-iptcExt:LocationCreatedProvinceState".to_string(),
                                value,
                            ));
                        }
                    }
                } else if tag_name.contains("CountryName") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push((
                                "XMP-iptcExt:LocationCreatedCountryName".to_string(),
                                value,
                            ));
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

/// Extract ArtworkOrObject struct values
fn extract_artwork_or_object(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Vec<(String, String)>> {
    let mut results = Vec::new();
    let mut depth = 1;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");

                if tag_name.contains("AOTitle") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push(("XMP-iptcExt:ArtworkTitle".to_string(), value));
                        }
                    }
                } else if tag_name.contains("AOCreator") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push(("XMP-iptcExt:ArtworkCreator".to_string(), value));
                        }
                    }
                } else if tag_name.contains("AODateCreated") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push(("XMP-iptcExt:ArtworkDateCreated".to_string(), value));
                        }
                    }
                } else if tag_name.contains("AOSource") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push(("XMP-iptcExt:ArtworkSource".to_string(), value));
                        }
                    }
                } else if tag_name.contains("AOCopyrightNotice") {
                    if let Ok(value) = extract_text_content(reader, buf) {
                        if !value.is_empty() {
                            results.push(("XMP-iptcExt:ArtworkCopyrightNotice".to_string(), value));
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
    fn test_extract_person_in_image() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:Iptc4xmpExt="http://iptc.org/std/Iptc4xmpExt/2008-02-29/">
              <rdf:Description>
                <Iptc4xmpExt:PersonInImage>
                  <rdf:Bag>
                    <rdf:li>John Doe</rdf:li>
                    <rdf:li>Jane Smith</rdf:li>
                  </rdf:Bag>
                </Iptc4xmpExt:PersonInImage>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_iptc_ext_values(xml).unwrap();
        let persons: Vec<_> = result
            .iter()
            .filter(|(k, _)| k == "XMP-iptcExt:PersonInImage")
            .map(|(_, v)| v.as_str())
            .collect();

        assert!(persons.contains(&"John Doe"));
        assert!(persons.contains(&"Jane Smith"));
    }

    #[test]
    fn test_extract_event() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:Iptc4xmpExt="http://iptc.org/std/Iptc4xmpExt/2008-02-29/">
              <rdf:Description>
                <Iptc4xmpExt:Event>Concert 2024</Iptc4xmpExt:Event>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = extract_iptc_ext_values(xml).unwrap();
        assert!(result
            .iter()
            .any(|(k, v)| k == "XMP-iptcExt:Event" && v == "Concert 2024"));
    }
}
