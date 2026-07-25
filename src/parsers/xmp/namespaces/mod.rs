//! XMP namespace-specific value extraction handlers
//!
//! This module provides handlers for extracting values from different XMP namespaces,
//! including support for complex XMP structures like Seq (ordered arrays), Bag (unordered
//! arrays), Alt (alternatives with language tags), and Struct (nested structures).

pub mod iptc_ext;
pub mod lightroom;
pub mod plus;
pub mod xmp_dm;
pub mod xmp_rights;

pub use iptc_ext::extract_iptc_ext_values;
pub use lightroom::extract_lightroom_values;
pub use plus::extract_plus_values;
pub use xmp_dm::extract_xmp_dm_values;
pub use xmp_rights::extract_xmp_rights_values;

use crate::error::Result;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

/// Extract text content from an element
pub(crate) fn extract_text_content(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<String> {
    let mut content = String::new();

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Text(e)) => {
                if let Ok(text) = e.unescape() {
                    content.push_str(&text);
                }
            }
            Ok(Event::End(_)) => break,
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(content.trim().to_string())
}

/// Extract values from rdf:Seq (ordered array)
pub(crate) fn extract_seq_values(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Vec<String>> {
    let mut values = Vec::new();
    let mut depth = 1;
    let mut in_li = false;
    let mut current_value = String::new();

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");
                if tag_name.ends_with("li") {
                    in_li = true;
                    current_value.clear();
                }
            }
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                if in_li {
                    if !current_value.trim().is_empty() {
                        values.push(current_value.trim().to_string());
                    }
                    in_li = false;
                }
            }
            Ok(Event::Text(e)) if in_li => {
                if let Ok(text) = e.unescape() {
                    current_value.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(values)
}

/// Extract values from rdf:Bag (unordered array)
pub(crate) fn extract_bag_values(
    reader: &mut Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Vec<String>> {
    // Bag has same structure as Seq, just different semantics
    extract_seq_values(reader, buf)
}

/// Extract value from rdf:Alt (alternatives with language tags)
/// Returns the default or first alternative
pub(crate) fn extract_alt_value(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Result<String> {
    let mut default_value = String::new();
    let mut first_value = String::new();
    let mut depth = 1;
    let mut in_li = false;
    let mut current_value = String::new();
    let mut is_default = false;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                let tag_name = std::str::from_utf8(e.name().as_ref()).unwrap_or("");
                if tag_name.ends_with("li") {
                    in_li = true;
                    current_value.clear();

                    // Check for xml:lang="x-default"
                    for attr in e.attributes().flatten() {
                        if let Ok(key) = std::str::from_utf8(attr.key.as_ref()) {
                            if key == "xml:lang" {
                                if let Ok(value) = std::str::from_utf8(&attr.value) {
                                    is_default = value == "x-default";
                                }
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
                if in_li {
                    let trimmed = current_value.trim().to_string();
                    if is_default {
                        default_value = trimmed;
                    } else if first_value.is_empty() {
                        first_value = trimmed;
                    }
                    in_li = false;
                    is_default = false;
                }
            }
            Ok(Event::Text(e)) if in_li => {
                if let Ok(text) = e.unescape() {
                    current_value.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    // Prefer default, fall back to first
    Ok(if !default_value.is_empty() {
        default_value
    } else {
        first_value
    })
}

/// Skip a complex structure and return to parent level
pub(crate) fn skip_element(reader: &mut Reader<&[u8]>, buf: &mut Vec<u8>) -> Result<()> {
    let mut depth = 1;

    loop {
        match reader.read_event_into(buf) {
            Ok(Event::Start(_)) => depth += 1,
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

    Ok(())
}
