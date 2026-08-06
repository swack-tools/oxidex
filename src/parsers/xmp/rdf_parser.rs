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
//! <x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 5.1.0">
//!   <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
//!     <rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/">
//!       <xmp:Creator>John Doe</xmp:Creator>
//!       <xmp:ModifyDate>2023-05-15</xmp:ModifyDate>
//!     </rdf:Description>
//!   </rdf:RDF>
//! </x:xmpmeta>
//! ```
//!
//! # Extracted Tags
//!
//! This parser extracts:
//! - **XMP:XMPToolkit** - from `x:xmptk` attribute on `x:xmpmeta` element
//! - **XMP:About** - from `rdf:about` attribute on `rdf:Description` element
//! - **Property elements** - like `<xmp:Creator>value</xmp:Creator>`
//! - **Property attributes** - XMP shorthand form on `rdf:Description`
//!
//! # Example
//!
//! ```no_run
//! use oxidex::parsers::xmp::rdf_parser::parse_xmp;
//!
//! let xml = br#"
//!     <x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Image::ExifTool 12.46">
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
//! assert!(result.len() >= 3); // XMPToolkit + Creator + Rating
//! ```

use crate::core::formatters::print_fraction;
use crate::core::value_formatter::format_iptc_urgency;
use crate::error::{ExifToolError, Result};
use crate::parsers::xmp::namespace_resolver::NamespaceResolver;
use quick_xml::Reader;
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};

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
/// use oxidex::parsers::xmp::rdf_parser::parse_xmp;
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
    Ok(parse_xmp_typed(xml_bytes)?
        .into_iter()
        .map(|(tag, value)| (tag, value.into_joined()))
        .collect())
}

/// One XMP property value: a scalar, or a List whose elements ExifTool reports
/// individually rather than as one joined string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmpValue {
    /// A single value.
    Scalar(String),
    /// An `rdf:Bag`/`Seq` with more than one entry, or a struct field repeated
    /// across the entries of one.
    List(Vec<String>),
}

impl XmpValue {
    /// The value as one string, joining a List the way ExifTool's plain text
    /// output joins one.
    pub fn into_joined(self) -> String {
        match self {
            XmpValue::Scalar(value) => value,
            XmpValue::List(values) => values.join(", "),
        }
    }
}

impl std::fmt::Display for XmpValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            XmpValue::Scalar(value) => f.write_str(value),
            XmpValue::List(values) => f.write_str(&values.join(", ")),
        }
    }
}

impl PartialEq<str> for XmpValue {
    fn eq(&self, other: &str) -> bool {
        match self {
            XmpValue::Scalar(value) => value == other,
            XmpValue::List(values) => values.join(", ") == other,
        }
    }
}

impl PartialEq<&str> for XmpValue {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

/// Parses an XMP packet, keeping List-valued properties as lists.
///
/// [`parse_xmp`] joins them, which is right for a text dump but wrong for
/// structured output: `exiftool -json` reports `dc:subject` as
/// `["ExifTool","Test","XMP"]`, and a caller that stores one joined string can
/// never reproduce that. Callers that build a `TagValue` should use this.
pub fn parse_xmp_typed(xml_bytes: &[u8]) -> Result<Vec<(String, XmpValue)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true); // Trim whitespace from text nodes

    let mut resolver = NamespaceResolver::new();
    let mut results: Vec<(String, String)> = Vec::new();
    // Tags whose value came from a multi-entry Bag/Seq, with their elements
    // kept apart. Recorded beside `results` rather than replacing it so that
    // every focused pass below keeps working on plain strings.
    let mut list_elements: Vec<(String, Vec<String>)> = Vec::new();
    let mut buf = Vec::new();

    // State tracking. `description_depth` is a COUNT, not a flag: RDF allows a
    // struct to be written as a nested `rdf:Description` (XMP3.xmp writes
    // `ph:supervisor` that way), and with a bool the inner element's `</...>`
    // cleared the flag for the rest of the OUTER Description -- silently
    // dropping every property written after it. On XMP3.xmp that was
    // CountryCode, Scene and both pdfx custom properties.
    let mut description_depth = 0usize;
    let mut current_property: Option<String> = None;
    let mut current_value = String::new();
    let mut depth = 0;
    let mut property_depth = 0;
    let mut inside_collection = false; // Are we in a Bag/Seq/Alt?
    let mut collection_values: Vec<String> = Vec::new(); // Collect rdf:li values
    // `xml:lang` of each collected `rdf:li`, parallel to `collection_values`.
    // A lang-alt is not one comma-joined value: ExifTool reports the
    // x-default entry under the plain tag name and every other language under
    // `Tag-<lang>` (XMP.pm, GetLangInfo), so joining them lost both.
    let mut collection_langs: Vec<Option<String>> = Vec::new();
    let mut pending_lang: Option<String> = None;
    // Whether the current property is a STRUCTURE rather than a value: it has
    // sub-property elements, a nested rdf:Description, or RDF shorthand
    // attributes. ExifTool reports only the flattened fields of one, never the
    // container -- reporting it produced tags like
    // `XMP:Regions = "32642448pixel-1179414036..."`, every field's text run
    // together, which no ExifTool output ever contains.
    let mut property_is_struct = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;

                let tag_name = extract_tag_name(&e)?;

                // Register any new namespaces from this element first
                register_namespaces_from_element(&e, &mut resolver)?;

                // Check for x:xmpmeta element and extract XMPToolkit
                if is_xmpmeta(&tag_name) {
                    extract_xmpmeta_attributes(&e, &mut results)?;
                }
                // Check if this is an rdf:Description element
                else if is_rdf_description(&tag_name, &resolver) {
                    description_depth += 1;
                    if current_property.is_some() {
                        property_is_struct = true;
                    }
                    // Extract rdf:about and property attributes from Description
                    extract_description_attributes(&e, &resolver, &mut results)?;
                } else if description_depth > 0 && current_property.is_none() {
                    // This is a property element inside rdf:Description
                    // Check if it's a complex structure we should skip
                    if is_simple_property(&tag_name, &resolver) {
                        current_property = Some(tag_name.to_string());
                        current_value.clear();
                        collection_values.clear();
                        collection_langs.clear();
                        inside_collection = false;
                        property_is_struct = has_shorthand_fields(&e);
                        property_depth = depth;
                    }
                } else if current_property.is_some() {
                    // Check if this is a Bag/Seq/Alt container
                    if is_collection_container(&tag_name, &resolver) {
                        inside_collection = true;
                        collection_values.clear();
                        collection_langs.clear();
                    } else if inside_collection && is_rdf_li(&tag_name, &resolver) {
                        pending_lang = xml_lang_attribute(&e);
                    } else if !is_rdf_namespace(&tag_name, &resolver) {
                        property_is_struct = true;
                    }
                }
            }

            Ok(Event::End(e)) => {
                let tag_name = extract_tag_name_from_bytes(e.name().as_ref())?;

                if is_rdf_description(&tag_name, &resolver) {
                    description_depth = description_depth.saturating_sub(1);
                } else if is_rdf_li(&tag_name, &resolver) && inside_collection {
                    // End of rdf:li - save the collected value
                    if !current_value.trim().is_empty() {
                        collection_values.push(current_value.trim().to_string());
                        collection_langs.push(pending_lang.take());
                    }
                    pending_lang = None;
                    current_value.clear();
                } else if is_collection_container(&tag_name, &resolver) {
                    inside_collection = false;
                } else if let Some(ref prop) = current_property
                    && depth == property_depth
                {
                    // End of current property - extract tag name and value
                    let prefixed_name = format_tag_name(prop, &resolver);

                    if property_is_struct {
                        // Reported only through its flattened fields.
                    } else if !collection_values.is_empty() {
                        if collection_langs.iter().any(Option::is_some) {
                            // lang-alt: one tag per language, x-default (or the
                            // first entry) keeping the plain name.
                            let default_index = collection_langs
                                .iter()
                                .position(|l| l.as_deref() == Some("x-default"))
                                .unwrap_or(0);
                            for (index, value) in collection_values.iter().enumerate() {
                                let tag = match collection_langs.get(index).and_then(Clone::clone) {
                                    Some(lang) if index != default_index => {
                                        format!("{prefixed_name}-{lang}")
                                    }
                                    _ if index == default_index => prefixed_name.clone(),
                                    None => continue,
                                    _ => continue,
                                };
                                if !results.iter().any(|(t, _)| *t == tag) {
                                    results.push((tag, value.clone()));
                                }
                            }
                        } else {
                            // Output collection as comma-separated list
                            if collection_values.len() > 1 {
                                list_elements
                                    .push((prefixed_name.clone(), collection_values.clone()));
                            }
                            results.push((prefixed_name, collection_values.join(", ")));
                        }
                    } else if !current_value.trim().is_empty() {
                        results.push((prefixed_name, current_value.trim().to_string()));
                    } else {
                        // An empty property -- `<x:Tag></x:Tag>`, or one whose
                        // only content is an empty Bag/Seq/Alt. ExifTool
                        // reports it with an empty value (XMP.pm's ParseXMPElement
                        // calls FoundXMP whenever `length $val or not $shorthand`),
                        // so dropping it loses the tag outright.
                        results.push((prefixed_name, String::new()));
                    }
                    current_property = None;
                    current_value.clear();
                    collection_values.clear();
                    collection_langs.clear();
                    pending_lang = None;
                    inside_collection = false;
                    property_is_struct = false;
                }
                depth -= 1;
            }

            Ok(Event::Text(e)) => {
                // Collect text content if we're inside a property
                // First decode the bytes, then unescape XML entities like &apos; &quot; &amp; etc.
                if current_property.is_some()
                    && let Ok(decoded) = e.xml10_content()
                {
                    // Unescape XML entities (e.g., &apos; -> ', &quot; -> ", &amp; -> &)
                    let unescaped =
                        quick_xml::escape::unescape(&decoded).unwrap_or_else(|_| decoded.clone());
                    current_value.push_str(&unescaped);
                }
            }

            Ok(Event::Empty(e)) => {
                let tag_name = extract_tag_name(&e)?;

                // Register namespaces from empty elements
                register_namespaces_from_element(&e, &mut resolver)?;

                // Handle self-closing x:xmpmeta
                if is_xmpmeta(&tag_name) {
                    extract_xmpmeta_attributes(&e, &mut results)?;
                }
                // Handle self-closing rdf:Description (shorthand form)
                else if is_rdf_description(&tag_name, &resolver) {
                    extract_description_attributes(&e, &resolver, &mut results)?;
                }
            }

            Ok(Event::Eof) => break,

            Ok(Event::GeneralRef(e)) => {
                // Handle XML entity references like &apos; &quot; &amp; &lt; &gt;
                if current_property.is_some() {
                    if let Ok(entity_name) = e.xml10_content() {
                        // First try to resolve as character reference (&#123; or &#x7B;)
                        if let Ok(Some(ch)) = e.resolve_char_ref() {
                            current_value.push(ch);
                        }
                        // Then try predefined XML entities (apos, quot, amp, lt, gt)
                        else if let Some(resolved) = resolve_predefined_entity(&entity_name) {
                            current_value.push_str(resolved);
                        }
                        // Unknown entity - keep the original reference
                        else {
                            current_value.push('&');
                            current_value.push_str(&entity_name);
                            current_value.push(';');
                        }
                    }
                }
            }

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

    // AboutCvTerm is an IPTC Extension bag of structures. ExifTool flattens
    // fields from every structure into list-valued AboutCvTerm tags.
    let (about_cv_term_cv_ids, about_cv_term_names) = extract_about_cv_term_values(xml_bytes)?;
    if !about_cv_term_cv_ids.is_empty() {
        const TAG: &str = "XMP:AboutCvTermCvId";

        // Avoid duplicate output if generic structured-property support is
        // added later.
        results.retain(|(tag, _)| tag != TAG);
        list_elements.retain(|(tag, _)| tag != TAG);
        list_elements.push((TAG.to_string(), about_cv_term_cv_ids.clone()));
        results.push((TAG.to_string(), about_cv_term_cv_ids.join(", ")));
    }

    if !about_cv_term_names.is_empty() {
        const TAG: &str = "XMP:AboutCvTermName";

        // Avoid duplicate output if generic structured-property support is
        // added later.
        results.retain(|(tag, _)| tag != TAG);
        list_elements.retain(|(tag, _)| tag != TAG);
        list_elements.push((TAG.to_string(), about_cv_term_names.clone()));
        results.push((TAG.to_string(), about_cv_term_names.join(", ")));
    }

    // ArtworkTitle is an IPTC Extension bag of ArtworkOrObject structures.
    // ExifTool flattens AOTitle fields from top-level ArtworkOrObject
    // (not those nested inside mwg-rs:Regions) into language-qualified tags.
    let artwork_titles = extract_artwork_title_values(xml_bytes)?;
    for (tag, value) in &artwork_titles {
        if !results.iter().any(|(t, _)| t == tag) {
            results.push((tag.clone(), value.clone()));
        }
    }

    // PLUS Custom1 is a Bag whose entries are language alternatives. Preserve
    // the list independently for each language, including explicitly empty
    // entries (XMP9.xmp).
    let custom1_language_values = extract_custom1_language_values(xml_bytes)?;
    if !custom1_language_values.is_empty() {
        // Remove the generic container once, before the x-default value is
        // installed under this same family/name. Doing this inside the loop
        // made the following language delete the freshly written default.
        results.retain(|(existing, _)| existing != "XMP-plus:Custom1");
    }
    for (tag, values) in custom1_language_values {
        results.retain(|(existing, _)| existing != &tag);
        list_elements.retain(|(existing, _)| existing != &tag);
        list_elements.push((tag.clone(), values.clone()));
        results.push((tag, values.join(", ")));
    }

    // ResourceRef fields may use element or RDF attribute shorthand. Handle
    // both forms, including a nested rdf:Description (XMP.xmp).
    for (tag, value) in extract_derived_from_ids(xml_bytes)? {
        results.retain(|(existing, _)| existing != &tag);
        results.push((tag, value));
    }

    // Flatten top-level `rdf:parseType="Resource"` structures into
    // ParentField tags, the way ExifTool's XMP::GetXMPTagID builds tag IDs
    // ("$tag .= ucfirst($nm)", XMP.pm). This is what produces e.g.
    // exif:Flash/exif:Mode -> FlashMode and test:BareStruct/test:Item1 ->
    // BareStructItem1.
    let struct_fields = extract_top_level_struct_values(xml_bytes)?;
    for (tag, value) in &struct_fields {
        if !results.iter().any(|(t, _)| t == tag) {
            results.push((tag.clone(), value.clone()));
        }
    }

    // PLUS CopyrightOwner is a Seq of structures rather than a bare struct,
    // so the generic pass above does not reach it.
    let copyright_owner = extract_plus_copyright_owner_name(xml_bytes)?;
    for (tag, value) in &copyright_owner {
        if !results.iter().any(|(t, _)| t == tag) {
            results.push((tag.clone(), value.clone()));
        }
    }

    // XMP-xmpBJ JobRef is a Bag of structures, likewise out of the generic
    // pass's reach. ExifTool reports only the flattened fields, so drop the
    // container tag once a field has replaced it rather than reporting a
    // JobRef tag ExifTool never emits.
    let job_ref_fields = extract_job_ref_fields(xml_bytes)?;
    if !job_ref_fields.is_empty() {
        results.retain(|(tag, _)| tag != "XMP:JobRef");
        for (tag, value) in &job_ref_fields {
            if !results.iter().any(|(t, _)| t == tag) {
                results.push((tag.clone(), value.clone()));
            }
        }
    }

    // Structures reached through a Bag/Seq of rdf:li -- LocationShown,
    // LocationCreated, Manifest, MWG keyword hierarchies. Appended last so the
    // focused passes above, which know their schemas' FlatName overrides, keep
    // precedence over this one's plain path concatenation.
    let list_structs = extract_list_struct_values(xml_bytes)?;
    for (tag, values) in &list_structs {
        if !results.iter().any(|(t, _)| t == tag) {
            if values.len() > 1 {
                list_elements.retain(|(existing, _)| existing != tag);
                list_elements.push((tag.clone(), values.clone()));
            }
            results.push((tag.clone(), values.join(", ")));
        }
    }

    // Everything else: the generic walk of the RDF property path that mirrors
    // XMP.pm's GetXMPTagID. Appended last so every focused pass above, each of
    // which knows its own schema's FlatName overrides and value formatting,
    // keeps precedence over this one's plain concatenation.
    let flattened = super::struct_flatten::extract_flattened_struct_fields(xml_bytes)?;
    for (tag, values) in flattened {
        if results.iter().any(|(t, _)| *t == tag) {
            continue;
        }
        if values.len() > 1 {
            list_elements.push((tag.clone(), values.clone()));
        }
        results.push((tag, values.join(", ")));
    }

    // RDF blank nodes (`rdf:nodeID`), which no tree walk can resolve on its own
    // because the fields of one node are spread across several places in the
    // document.
    let blank_nodes = super::struct_flatten::extract_blank_node_fields(xml_bytes)?;
    for (tag, value) in blank_nodes {
        results.retain(|(existing, _)| *existing != tag);
        results.push((tag, value));
    }

    // XMP's reported tag name is derived from the namespace URI, not the XML
    // prefix.  Two different custom namespaces can therefore collapse onto
    // the same generic `XMP:<name>` tag when a prefix is rebound in a later
    // rdf:Description.  ExifTool registers the first such mapping (XMP6.xmp:
    // fish/Test -> trout, then feline/Test -> tabby) and retains `trout`.
    // Keep the first emitted value too, rather than letting a downstream
    // MetadataMap insertion silently overwrite it.
    let mut emitted_tags = std::collections::HashSet::new();
    results.retain(|(tag, _)| emitted_tags.insert(tag.clone()));

    // Google's `GCamera:HdrPlusMakernote` property carries a base64,
    // encrypted, gzipped Protobuf blob (Google.pm's `ProcessHDRP`). ExifTool
    // re-files the fields it extracts from that blob under the `MakerNotes`
    // group rather than `XMP` (`Google::HDRPlusMakerNote`'s `GROUPS => { 0
    // => 'MakerNotes' }`), so decode it here, before `format_xmp_value` gets
    // a chance to see -- and rewrite -- the raw base64 text.
    let raw_hdrp_makernote = results
        .iter()
        .find(|(tag, _)| tag == "XMP:HDRPlusMakerNote")
        .map(|(_, value)| value.clone());

    // Post-process results to apply formatting for specific tags
    let mut formatted: Vec<(String, XmpValue)> = results
        .into_iter()
        .map(
            |(tag, value)| match list_elements.iter().find(|(t, _)| *t == tag) {
                Some((_, elements)) => {
                    let formatted = elements
                        .iter()
                        .map(|element| format_xmp_value(&tag, element))
                        .collect();
                    (tag, XmpValue::List(formatted))
                }
                None => {
                    let formatted = format_xmp_value(&tag, &value);
                    (tag, XmpValue::Scalar(formatted))
                }
            },
        )
        .collect();

    if let Some(raw) = raw_hdrp_makernote {
        for (tag, value) in super::google_hdrp::decode_hdrp_plus_makernote(&raw) {
            if !formatted.iter().any(|(t, _)| *t == tag) {
                formatted.push((tag, XmpValue::Scalar(value)));
            }
        }
    }

    Ok(formatted)
}

/// Extracts flattened fields from the IPTC Extension AboutCvTerm structured bag.
///
/// A second, focused pass is used because the general RDF parser treats an
/// entire collection as one property and cannot distinguish fields in
/// resource-valued `rdf:li` entries.
fn extract_about_cv_term_values(xml_bytes: &[u8]) -> Result<(Vec<String>, Vec<String>)> {
    const IPTC_EXT_NAMESPACE: &str = "http://iptc.org/std/Iptc4xmpExt/2008-02-29/";

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut cv_ids = Vec::new();
    let mut cv_term_names = Vec::new();
    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut about_cv_term_depth: Option<usize> = None;
    let mut cv_id_depth: Option<usize> = None;
    let mut cv_term_name_depth: Option<usize> = None;
    let mut current_cv_id = String::new();
    let mut current_cv_term_name = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                register_namespaces_from_element(&e, &mut resolver)?;

                let tag_name = extract_tag_name(&e)?;
                if about_cv_term_depth.is_none()
                    && is_property_in_namespace(
                        &tag_name,
                        "AboutCvTerm",
                        IPTC_EXT_NAMESPACE,
                        &resolver,
                    )
                {
                    about_cv_term_depth = Some(depth);
                } else if about_cv_term_depth.is_some()
                    && cv_id_depth.is_none()
                    && is_property_in_namespace(&tag_name, "CvId", IPTC_EXT_NAMESPACE, &resolver)
                {
                    cv_id_depth = Some(depth);
                    current_cv_id.clear();
                } else if about_cv_term_depth.is_some()
                    && cv_term_name_depth.is_none()
                    && is_property_in_namespace(
                        &tag_name,
                        "CvTermName",
                        IPTC_EXT_NAMESPACE,
                        &resolver,
                    )
                {
                    cv_term_name_depth = Some(depth);
                    current_cv_term_name.clear();
                }
            }

            Ok(Event::End(e)) => {
                let tag_name = extract_tag_name_from_bytes(e.name().as_ref())?;

                if cv_id_depth == Some(depth)
                    && is_property_in_namespace(&tag_name, "CvId", IPTC_EXT_NAMESPACE, &resolver)
                {
                    let value = current_cv_id.trim();
                    if !value.is_empty() {
                        cv_ids.push(value.to_string());
                    }
                    current_cv_id.clear();
                    cv_id_depth = None;
                }

                if cv_term_name_depth == Some(depth)
                    && is_property_in_namespace(
                        &tag_name,
                        "CvTermName",
                        IPTC_EXT_NAMESPACE,
                        &resolver,
                    )
                {
                    // CvTermName is normally an rdf:Alt. Text collected from
                    // its nested rdf:li is the flattened value ExifTool emits.
                    let value = current_cv_term_name.trim();
                    if !value.is_empty() {
                        cv_term_names.push(value.to_string());
                    }
                    current_cv_term_name.clear();
                    cv_term_name_depth = None;
                }

                if about_cv_term_depth == Some(depth)
                    && is_property_in_namespace(
                        &tag_name,
                        "AboutCvTerm",
                        IPTC_EXT_NAMESPACE,
                        &resolver,
                    )
                {
                    about_cv_term_depth = None;
                }

                depth = depth.saturating_sub(1);
            }

            Ok(Event::Text(e)) => {
                if (cv_id_depth.is_some() || cv_term_name_depth.is_some())
                    && let Ok(decoded) = e.xml10_content()
                {
                    let unescaped =
                        quick_xml::escape::unescape(&decoded).unwrap_or_else(|_| decoded.clone());
                    if cv_id_depth.is_some() {
                        current_cv_id.push_str(&unescaped);
                    }
                    if cv_term_name_depth.is_some() {
                        current_cv_term_name.push_str(&unescaped);
                    }
                }
            }

            Ok(Event::GeneralRef(e)) => {
                if (cv_id_depth.is_some() || cv_term_name_depth.is_some())
                    && let Ok(entity_name) = e.xml10_content()
                {
                    if let Ok(Some(ch)) = e.resolve_char_ref() {
                        if cv_id_depth.is_some() {
                            current_cv_id.push(ch);
                        }
                        if cv_term_name_depth.is_some() {
                            current_cv_term_name.push(ch);
                        }
                    } else if let Some(resolved) = resolve_predefined_entity(&entity_name) {
                        if cv_id_depth.is_some() {
                            current_cv_id.push_str(resolved);
                        }
                        if cv_term_name_depth.is_some() {
                            current_cv_term_name.push_str(resolved);
                        }
                    } else {
                        let unresolved = format!("&{};", entity_name);
                        if cv_id_depth.is_some() {
                            current_cv_id.push_str(&unresolved);
                        }
                        if cv_term_name_depth.is_some() {
                            current_cv_term_name.push_str(&unresolved);
                        }
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                register_namespaces_from_element(&e, &mut resolver)?;
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid XMP AboutCvTerm structure: {}",
                    e
                )));
            }
        }

        buf.clear();
    }

    Ok((cv_ids, cv_term_names))
}

/// Extracts language-qualified ArtworkTitle values from IPTC Extension
/// ArtworkOrObject structures, only at the top level (not nested inside
/// mwg-rs:Regions).
fn extract_artwork_title_values(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    const IPTC_EXT_NS: &str = "http://iptc.org/std/Iptc4xmpExt/2008-02-29/";
    const MWG_RS_NS: &str = "http://www.metadataworkinggroup.com/schemas/regions/";

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut results = Vec::new();
    let mut buf = Vec::new();
    let mut depth = 0usize;

    let mut mwg_rs_depth: Option<usize> = None;
    let mut artwork_depth: Option<usize> = None;
    let mut ao_title_depth: Option<usize> = None;
    let mut alt_depth: Option<usize> = None;
    let mut li_depth: Option<usize> = None;
    let mut current_lang: Option<String> = None;
    let mut current_value = String::new();
    let mut x_default_value: Option<String> = None;
    let mut lang_values: Vec<(String, String)> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                register_namespaces_from_element(&e, &mut resolver)?;
                let tag_name = extract_tag_name(&e)?;

                if mwg_rs_depth.is_none()
                    && is_property_in_namespace(&tag_name, "Regions", MWG_RS_NS, &resolver)
                {
                    mwg_rs_depth = Some(depth);
                }

                if artwork_depth.is_none()
                    && mwg_rs_depth.is_none()
                    && is_property_in_namespace(
                        &tag_name,
                        "ArtworkOrObject",
                        IPTC_EXT_NS,
                        &resolver,
                    )
                {
                    artwork_depth = Some(depth);
                    x_default_value = None;
                    lang_values.clear();
                }

                if artwork_depth.is_some()
                    && ao_title_depth.is_none()
                    && is_property_in_namespace(&tag_name, "AOTitle", IPTC_EXT_NS, &resolver)
                {
                    ao_title_depth = Some(depth);
                }

                if ao_title_depth.is_some()
                    && alt_depth.is_none()
                    && is_collection_container(&tag_name, &resolver)
                {
                    alt_depth = Some(depth);
                }

                if alt_depth.is_some() && li_depth.is_none() && is_rdf_li(&tag_name, &resolver) {
                    li_depth = Some(depth);
                    current_lang = None;
                    current_value.clear();
                    for attr in e.attributes().flatten() {
                        let key = std::str::from_utf8(attr.key.as_ref()).unwrap_or("");
                        if key == "xml:lang" {
                            if let Ok(val) = std::str::from_utf8(&attr.value) {
                                current_lang = Some(val.to_string());
                            }
                        }
                    }
                }
            }

            Ok(Event::End(e)) => {
                let tag_name = extract_tag_name_from_bytes(e.name().as_ref())?;

                if li_depth == Some(depth) && is_rdf_li(&tag_name, &resolver) {
                    let value = current_value.trim().to_string();
                    if !value.is_empty() {
                        if let Some(ref lang) = current_lang {
                            if lang == "x-default" {
                                x_default_value = Some(value.clone());
                            }
                            lang_values.push((lang.clone(), value));
                        }
                    }
                    current_value.clear();
                    li_depth = None;
                    current_lang = None;
                }

                if alt_depth == Some(depth) && is_collection_container(&tag_name, &resolver) {
                    alt_depth = None;
                }

                if ao_title_depth == Some(depth)
                    && is_property_in_namespace(&tag_name, "AOTitle", IPTC_EXT_NS, &resolver)
                {
                    // Emit base tag with x-default value
                    if let Some(ref default_val) = x_default_value {
                        results.push(("XMP:ArtworkTitle".to_string(), default_val.clone()));
                    }
                    // Emit language-qualified tags
                    for (lang, val) in &lang_values {
                        if lang != "x-default" {
                            results.push((format!("XMP:ArtworkTitle-{}", lang), val.clone()));
                        }
                    }
                    ao_title_depth = None;
                    x_default_value = None;
                    lang_values.clear();
                }

                if artwork_depth == Some(depth)
                    && is_property_in_namespace(
                        &tag_name,
                        "ArtworkOrObject",
                        IPTC_EXT_NS,
                        &resolver,
                    )
                {
                    artwork_depth = None;
                }

                if mwg_rs_depth == Some(depth)
                    && is_property_in_namespace(&tag_name, "Regions", MWG_RS_NS, &resolver)
                {
                    mwg_rs_depth = None;
                }

                depth = depth.saturating_sub(1);
            }

            Ok(Event::Text(e)) => {
                if li_depth.is_some()
                    && let Ok(decoded) = e.xml10_content()
                {
                    let unescaped =
                        quick_xml::escape::unescape(&decoded).unwrap_or_else(|_| decoded.clone());
                    current_value.push_str(&unescaped);
                }
            }

            Ok(Event::Empty(e)) => {
                register_namespaces_from_element(&e, &mut resolver)?;
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_e) => break,
        }
        buf.clear();
    }

    Ok(results)
}

/// Extracts PLUS Custom1's Bag-of-Alt values, retaining list positions that
/// are represented by explicitly empty language alternatives.
fn extract_custom1_language_values(xml_bytes: &[u8]) -> Result<Vec<(String, Vec<String>)>> {
    const PLUS_NS: &str = "http://ns.useplus.org/ldf/xmp/1.0/";

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut property_depth: Option<usize> = None;
    let mut language_li_depth: Option<usize> = None;
    let mut current_language: Option<String> = None;
    let mut current_value = String::new();
    let mut values: Vec<(String, String)> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                register_namespaces_from_element(&e, &mut resolver)?;
                let tag_name = extract_tag_name(&e)?;

                if property_depth.is_none()
                    && is_property_in_namespace(&tag_name, "Custom1", PLUS_NS, &resolver)
                {
                    property_depth = Some(depth);
                    values.clear();
                } else if property_depth.is_some()
                    && language_li_depth.is_none()
                    && is_rdf_li(&tag_name, &resolver)
                    && let Some(language) = xml_lang_attribute(&e)
                {
                    language_li_depth = Some(depth);
                    current_language = Some(language);
                    current_value.clear();
                }
            }

            Ok(Event::End(_)) => {
                if language_li_depth == Some(depth) {
                    if let Some(language) = current_language.take() {
                        values.push((language, current_value.trim().to_string()));
                    }
                    language_li_depth = None;
                    current_value.clear();
                }

                if property_depth == Some(depth) {
                    property_depth = None;
                }
                depth = depth.saturating_sub(1);
            }

            Ok(Event::Text(e)) => {
                if language_li_depth.is_some()
                    && let Ok(decoded) = e.xml10_content()
                {
                    let unescaped =
                        quick_xml::escape::unescape(&decoded).unwrap_or_else(|_| decoded.clone());
                    current_value.push_str(&unescaped);
                }
            }

            Ok(Event::Empty(e)) => {
                register_namespaces_from_element(&e, &mut resolver)?;
                let tag_name = extract_tag_name(&e)?;
                if property_depth.is_some()
                    && is_rdf_li(&tag_name, &resolver)
                    && let Some(language) = xml_lang_attribute(&e)
                {
                    values.push((language, String::new()));
                }
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid PLUS Custom1 XML: {e}"
                )));
            }
        }
        buf.clear();
    }

    let mut languages: Vec<(String, Vec<String>)> = Vec::new();
    for (language, value) in values {
        if let Some((_, language_values)) =
            languages.iter_mut().find(|(known, _)| known == &language)
        {
            language_values.push(value);
        } else {
            languages.push((language, vec![value]));
        }
    }

    Ok(languages
        .into_iter()
        .map(|(language, language_values)| {
            let tag = if language == "x-default" {
                "XMP-plus:Custom1".to_string()
            } else {
                format!("XMP-plus:Custom1-{language}")
            };
            (tag, language_values)
        })
        .collect())
}

/// Extracts xmpMM:DerivedFrom ResourceRef identifiers from either child
/// elements or RDF attribute shorthand.
fn extract_derived_from_ids(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    const XMP_MM_NS: &str = "http://ns.adobe.com/xap/1.0/mm/";
    const ST_REF_NS: &str = "http://ns.adobe.com/xap/1.0/sType/ResourceRef#";

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut derived_depth: Option<usize> = None;
    let mut field_depth: Option<usize> = None;
    let mut field_tag: Option<&'static str> = None;
    let mut field_value = String::new();
    let mut results = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                register_namespaces_from_element(&e, &mut resolver)?;
                let tag_name = extract_tag_name(&e)?;

                if derived_depth.is_none()
                    && is_property_in_namespace(&tag_name, "DerivedFrom", XMP_MM_NS, &resolver)
                {
                    derived_depth = Some(depth);
                    append_derived_from_attributes(&e, &resolver, ST_REF_NS, &mut results)?;
                } else if derived_depth.is_some() {
                    append_derived_from_attributes(&e, &resolver, ST_REF_NS, &mut results)?;

                    if field_depth.is_none() {
                        field_tag = derived_from_field_tag(&tag_name, &resolver, ST_REF_NS);
                        if field_tag.is_some() {
                            field_depth = Some(depth);
                            field_value.clear();
                        }
                    }
                }
            }

            Ok(Event::End(_)) => {
                if field_depth == Some(depth) {
                    if let Some(tag) = field_tag.take() {
                        let value = field_value.trim();
                        if !value.is_empty() {
                            upsert_string_result(&mut results, tag, value.to_string());
                        }
                    }
                    field_depth = None;
                    field_value.clear();
                }

                if derived_depth == Some(depth) {
                    derived_depth = None;
                }
                depth = depth.saturating_sub(1);
            }

            Ok(Event::Text(e)) => {
                if field_depth.is_some()
                    && let Ok(decoded) = e.xml10_content()
                {
                    let unescaped =
                        quick_xml::escape::unescape(&decoded).unwrap_or_else(|_| decoded.clone());
                    field_value.push_str(&unescaped);
                }
            }

            Ok(Event::Empty(e)) => {
                register_namespaces_from_element(&e, &mut resolver)?;
                let tag_name = extract_tag_name(&e)?;

                if derived_depth.is_none()
                    && is_property_in_namespace(&tag_name, "DerivedFrom", XMP_MM_NS, &resolver)
                {
                    append_derived_from_attributes(&e, &resolver, ST_REF_NS, &mut results)?;
                } else if derived_depth.is_some() {
                    append_derived_from_attributes(&e, &resolver, ST_REF_NS, &mut results)?;
                    if let Some(tag) = derived_from_field_tag(&tag_name, &resolver, ST_REF_NS)
                        && let Some(value) = resource_attribute(&e)?
                    {
                        upsert_string_result(&mut results, tag, value);
                    }
                }
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => {
                return Err(ExifToolError::parse_error(format!(
                    "Invalid XMP DerivedFrom XML: {e}"
                )));
            }
        }
        buf.clear();
    }

    Ok(results)
}

fn derived_from_field_tag(
    qname: &str,
    resolver: &NamespaceResolver,
    st_ref_namespace: &str,
) -> Option<&'static str> {
    if is_property_in_namespace(qname, "documentID", st_ref_namespace, resolver) {
        Some("XMP:DerivedFromDocumentID")
    } else if is_property_in_namespace(qname, "instanceID", st_ref_namespace, resolver) {
        Some("XMP:DerivedFromInstanceID")
    } else {
        None
    }
}

fn append_derived_from_attributes(
    element: &BytesStart,
    resolver: &NamespaceResolver,
    st_ref_namespace: &str,
    results: &mut Vec<(String, String)>,
) -> Result<()> {
    for attr in element.attributes().flatten() {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in DerivedFrom attribute: {e}"))
        })?;
        let Some(tag) = derived_from_field_tag(key, resolver, st_ref_namespace) else {
            continue;
        };
        let value = std::str::from_utf8(&attr.value).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in DerivedFrom value: {e}"))
        })?;
        if !value.trim().is_empty() {
            upsert_string_result(results, tag, value.trim().to_string());
        }
    }
    Ok(())
}

fn resource_attribute(element: &BytesStart) -> Result<Option<String>> {
    for attr in element.attributes().flatten() {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in RDF attribute: {e}"))
        })?;
        if key == "rdf:resource" {
            let value = std::str::from_utf8(&attr.value).map_err(|e| {
                ExifToolError::parse_error(format!("Invalid UTF-8 in RDF resource: {e}"))
            })?;
            return Ok(Some(value.to_string()));
        }
    }
    Ok(None)
}

fn upsert_string_result(results: &mut Vec<(String, String)>, tag: &str, value: String) {
    if let Some((_, existing)) = results.iter_mut().find(|(known, _)| known == tag) {
        *existing = value;
    } else {
        results.push((tag.to_string(), value));
    }
}

/// Flattens top-level `rdf:parseType="Resource"` structures into ExifTool's
/// `ParentField` tag names.
///
/// ExifTool builds an XMP tag ID by walking the RDF property path and
/// concatenating the capitalised local names, skipping the RDF structural
/// properties (`XMP.pm`, `GetXMPTagID`):
///
/// ```text
/// $tag .= ucfirst($nm);       # add to tag name
/// ```
///
/// So `test:BareStruct` / `test:Item1` becomes `BareStructItem1`, and
/// `exif:Flash` / `exif:Mode` becomes `FlashMode`. The general RDF pass in
/// [`parse_xmp`] treats a struct property as one opaque value and drops the
/// fields, which is why this focused second pass exists (same pattern as
/// `extract_about_cv_term_values`).
///
/// Deliberately limited to structures that are a *direct* child of an
/// `rdf:Description`:
/// - Nested structures reached through a `rdf:Bag`/`rdf:Seq` of `rdf:li`
///   (e.g. `Iptc4xmpExt:LocationCreated`) are List-valued in ExifTool and need
///   list semantics this pass does not implement.
/// - Fields inside `mwg-rs:Regions` flatten to a different name entirely
///   (`RegionExtensionsFlashMode`, not `FlashMode`), and `Regions` reaches
///   them through a `rdf:Bag`, so the direct-child rule already excludes them.
///
/// A field whose content is a language alternative (`rdf:Alt` of `rdf:li` with
/// `xml:lang`, optionally wrapped in a `rdf:Bag`) is emitted once per language
/// as `ParentField-<lang>`, with `x-default` emitted under the bare name --
/// matching ExifTool's `GetLangInfo` naming.
fn extract_top_level_struct_values(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut results: Vec<(String, String)> = Vec::new();
    let mut buf = Vec::new();
    let mut depth = 0usize;

    let mut description_depth: Option<usize> = None;
    let mut struct_depth: Option<usize> = None;
    let mut struct_name = String::new();
    let mut field_depth: Option<usize> = None;
    let mut field_name = String::new();
    let mut field_text = String::new();
    let mut li_depth: Option<usize> = None;
    let mut li_lang: Option<String> = None;
    let mut li_text = String::new();
    let mut lang_values: Vec<(String, String)> = Vec::new();
    let mut nested_depth: Option<usize> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                register_namespaces_from_element(&e, &mut resolver)?;
                let tag_name = extract_tag_name(&e)?;

                if is_rdf_description(&tag_name, &resolver) {
                    description_depth = Some(depth);
                } else if struct_depth.is_none()
                    && description_depth.is_some_and(|d| depth == d + 1)
                    && !is_rdf_namespace(&tag_name, &resolver)
                    && has_parse_type_resource(&e)
                {
                    struct_depth = Some(depth);
                    struct_name = ucfirst(NamespaceResolver::extract_local_name(&tag_name));
                } else if let Some(sd) = struct_depth {
                    if field_depth.is_none()
                        && depth == sd + 1
                        && !is_rdf_namespace(&tag_name, &resolver)
                    {
                        field_depth = Some(depth);
                        field_name = ucfirst(NamespaceResolver::extract_local_name(&tag_name));
                        field_text.clear();
                        lang_values.clear();
                    } else if field_depth.is_some() && nested_depth.is_none() {
                        if !is_rdf_namespace(&tag_name, &resolver) {
                            // A non-RDF element inside the field means a
                            // sub-structure, which flattens to a longer name
                            // than this pass builds -- mwg-rs:Regions/RegionList
                            // reaches lang-alt entries whose real ExifTool names
                            // are RegionExtensionsArtworkTitle-de and friends,
                            // not RegionsRegionList-de. Measured on XMP5.xmp
                            // 2026-07-27: without this guard the pass emitted
                            // exactly those three bogus tags.
                            nested_depth = Some(depth);
                        } else if li_depth.is_none()
                            && is_rdf_li(&tag_name, &resolver)
                            && let Some(lang) = xml_lang_attribute(&e)
                        {
                            li_depth = Some(depth);
                            li_lang = Some(lang);
                            li_text.clear();
                        }
                    }
                }
            }

            Ok(Event::End(_)) => {
                if nested_depth == Some(depth) {
                    nested_depth = None;
                }

                if li_depth == Some(depth) {
                    let value = li_text.trim().to_string();
                    if let (Some(lang), false) = (li_lang.take(), value.is_empty()) {
                        lang_values.push((lang, value));
                    }
                    li_depth = None;
                    li_text.clear();
                }

                if field_depth == Some(depth) {
                    let flat_id = format!("{struct_name}{field_name}");
                    let reported = exiftool_flat_tag_name(&flat_id);
                    if lang_values.is_empty() {
                        let value = field_text.trim().to_string();
                        if !value.is_empty() {
                            results.push((format!("XMP:{reported}"), value));
                        }
                    } else {
                        for (lang, value) in &lang_values {
                            let tag = if lang == "x-default" {
                                format!("XMP:{reported}")
                            } else {
                                format!("XMP:{reported}-{lang}")
                            };
                            // ExifTool keeps every entry of a List-valued
                            // lang-alt (XMP5.xmp has two en-US entries for
                            // BTestTagField1); emitting the tag twice would
                            // just be a duplicate emission here, so first
                            // wins.
                            if !results.iter().any(|(t, _)| *t == tag) {
                                results.push((tag, value.clone()));
                            }
                        }
                    }
                    field_depth = None;
                    field_text.clear();
                    lang_values.clear();
                    nested_depth = None;
                }

                if struct_depth == Some(depth) {
                    struct_depth = None;
                    struct_name.clear();
                }

                if description_depth == Some(depth) {
                    description_depth = None;
                }

                depth = depth.saturating_sub(1);
            }

            Ok(Event::Text(e)) => {
                if let Ok(decoded) = e.xml10_content() {
                    let unescaped =
                        quick_xml::escape::unescape(&decoded).unwrap_or_else(|_| decoded.clone());
                    if li_depth.is_some() {
                        li_text.push_str(&unescaped);
                    } else if field_depth == Some(depth) {
                        field_text.push_str(&unescaped);
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                register_namespaces_from_element(&e, &mut resolver)?;
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }

    Ok(results)
}

/// Flattens structures reached through an `rdf:Bag`/`rdf:Seq` of
/// `rdf:li rdf:parseType="Resource"`.
///
/// [`extract_top_level_struct_values`] deliberately stops at structures that
/// are a direct child of an `rdf:Description`. The other shape a struct takes
/// is a *list* of them, and it is by far the more common one in real files:
/// `Iptc4xmpExt:LocationShown`, `Iptc4xmpExt:LocationCreated`,
/// `xmpMM:Manifest` and MWG's keyword hierarchy are all Bags or Seqs of
/// structures. ExifTool names their fields the same way either way -- walk the
/// property path, concatenate `ucfirst` of each non-RDF name, skip the RDF
/// structural elements (`XMP.pm`, `GetXMPTagID`) -- so `Iptc4xmpExt:LocationShown`
/// / `rdf:Bag` / `rdf:li` / `Iptc4xmpExt:City` becomes `LocationShownCity`, and
/// the nested `xmpMM:Manifest` / `rdf:Seq` / `rdf:li` / `stMfs:reference` /
/// `stRef:filePath` becomes `ManifestReferenceFilePath`.
///
/// ExifTool reports these tags as Lists: every `rdf:li` contributes one value,
/// entries that omit a field simply do not contribute (XMP4.xmp's fourth
/// LocationCreated has no City, and ExifTool's LocationCreatedCity is still
/// three items). Values are joined the way ExifTool's own text output joins a
/// List.
///
/// `mwg-rs:Regions` is excluded for the same reason the top-level pass excludes
/// it: MWG.pm gives its fields `FlatName` overrides, so its flattened names are
/// `RegionName`/`RegionAreaH`/`RegionExtensions...` rather than the
/// `RegionsRegionList...` this concatenation builds. Emitting those would trade
/// missing tags for wrong ones.
fn extract_list_struct_values(xml_bytes: &[u8]) -> Result<Vec<(String, Vec<String>)>> {
    const MWG_RS_NS: &str = "http://www.metadataworkinggroup.com/schemas/regions/";

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut buf = Vec::new();
    let mut depth = 0usize;

    let mut description_depth: Option<usize> = None;
    let mut container_depth: Option<usize> = None;
    let mut container_name = String::new();
    // ExifTool has `List` declarations for registered XMP schemas, but an
    // unknown schema is handled generically: repeated flattened fields keep
    // the first value (XMP4.xmp's test:StructList2Item1/Item2).  Track this
    // at the container level so an ordinary list inside one struct remains a
    // list while repeated fields across unknown struct records do not merge.
    let mut container_allows_repeated_fields = false;
    // The resource-valued rdf:li currently contributing fields. This lets an
    // unknown schema keep the first repeated *field* across structs while
    // retaining every value in a list that belongs to that one struct.
    let mut resource_entry_depth: Option<usize> = None;
    let mut resource_entry_index = 0usize;
    // Field names below the container, with the RDF structural elements left
    // out -- the rest of ExifTool's tag ID, in pieces.
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    // (flattened id, values, resource entry of the last value) in first-seen
    // order.
    let mut collected: Vec<(String, Vec<String>, Option<usize>)> = Vec::new();

    let mut push_value = |flat_id: String,
                          value: String,
                          allow_repeat: bool,
                          resource_entry: Option<usize>| {
        if let Some((_, values, previous_entry)) =
            collected.iter_mut().find(|(id, _, _)| *id == flat_id)
        {
            if allow_repeat || resource_entry.is_some_and(|entry| Some(entry) == *previous_entry) {
                values.push(value);
                *previous_entry = resource_entry;
            }
        } else {
            collected.push((flat_id, vec![value], resource_entry));
        }
    };

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                register_namespaces_from_element(&e, &mut resolver)?;
                let tag_name = extract_tag_name(&e)?;

                if is_rdf_description(&tag_name, &resolver) {
                    description_depth = Some(depth);
                } else if container_depth.is_none() {
                    let is_container_candidate = description_depth
                        .is_some_and(|parent| depth == parent + 1)
                        && !is_rdf_namespace(&tag_name, &resolver)
                        && !has_parse_type_resource(&e)
                        && !is_property_in_namespace(&tag_name, "Regions", MWG_RS_NS, &resolver);
                    if is_container_candidate {
                        let local = ucfirst(NamespaceResolver::extract_local_name(&tag_name));
                        container_depth = Some(depth);
                        container_allows_repeated_fields =
                            NamespaceResolver::extract_prefix(&tag_name)
                                .and_then(|prefix| resolver.resolve_prefix(prefix))
                                .is_some_and(|uri| {
                                    super::namespace_mapping::namespace_to_family(uri).is_some()
                                        || uri == "http://ns.adobe.com/xap/1.0/mm/"
                                });
                        container_name = if is_flat_name_suppressed(&local) {
                            String::new()
                        } else {
                            local
                        };
                        path.clear();
                        text.clear();
                    }
                } else if is_rdf_namespace(&tag_name, &resolver) {
                    // rdf:Bag / rdf:Seq / rdf:Alt / rdf:li carry no name.
                    if is_rdf_li(&tag_name, &resolver) {
                        if resource_entry_depth.is_none() && has_parse_type_resource(&e) {
                            resource_entry_depth = Some(depth);
                            resource_entry_index += 1;
                        }
                        text.clear();
                    }
                } else {
                    path.push(ucfirst(NamespaceResolver::extract_local_name(&tag_name)));
                    text.clear();
                }
            }

            Ok(Event::Text(e)) => {
                if container_depth.is_some()
                    && let Ok(decoded) = e.xml10_content()
                {
                    let unescaped =
                        quick_xml::escape::unescape(&decoded).unwrap_or_else(|_| decoded.clone());
                    text.push_str(&unescaped);
                }
            }

            Ok(Event::End(e)) => {
                let tag_name = extract_tag_name_from_bytes(e.name().as_ref())?;

                if container_depth == Some(depth) {
                    container_depth = None;
                    container_name.clear();
                    container_allows_repeated_fields = false;
                    path.clear();
                    text.clear();
                } else if container_depth.is_some() {
                    let value = text.trim().to_string();
                    // Only struct *fields* are flattened; text directly under
                    // the container is the whole property, which the ordinary
                    // RDF pass already reports.
                    if !value.is_empty() && !path.is_empty() {
                        let flat_id = format!("{}{}", container_name, path.join(""));
                        push_value(
                            flat_id,
                            value,
                            container_allows_repeated_fields,
                            resource_entry_depth.map(|_| resource_entry_index),
                        );
                    }
                    text.clear();
                    if !is_rdf_namespace(&tag_name, &resolver) {
                        path.pop();
                    }
                }

                if description_depth == Some(depth) {
                    description_depth = None;
                }
                if resource_entry_depth == Some(depth) {
                    resource_entry_depth = None;
                }
                depth = depth.saturating_sub(1);
            }

            Ok(Event::Empty(e)) => {
                register_namespaces_from_element(&e, &mut resolver)?;
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }

    Ok(collected
        .into_iter()
        .map(|(flat_id, values, _)| (format!("XMP:{}", exiftool_flat_tag_name(&flat_id)), values))
        .collect())
}

/// Extracts `plus:CopyrightOwnerName` from the PLUS `CopyrightOwner` sequence.
///
/// `PLUS.pm` defines the structure and its flattened name:
///
/// ```text
/// my %plusCopyrightOwner = (
///     STRUCT_NAME => 'CopyrightOwner',
///     NAMESPACE   => 'plus',
///     CopyrightOwnerID    => { },
///     CopyrightOwnerName  => { },
/// );
/// ...
///     CopyrightOwner => {
///         FlatName => '',
///         Struct => \%plusCopyrightOwner,
///         List => 'Seq',
///     },
/// ```
///
/// `FlatName => ''` means the container contributes nothing to the flattened
/// name, so the field is reported as plain `CopyrightOwnerName` rather than
/// `CopyrightOwnerCopyrightOwnerName`. The `plus` prefix URI is
/// `http://ns.useplus.org/ldf/xmp/1.0/` (`XMP.pm` `%nsURI`, line 166).
///
/// `List => 'Seq'` allows several owners; multiple values are joined the same
/// way `extract_about_cv_term_values` joins its list.
fn extract_plus_copyright_owner_name(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    // Every PLUS sequence-of-structs in the schema has the identical shape --
    // a container holding rdf:Seq/rdf:li[parseType=Resource] wrapping one
    // named field -- and ExifTool reports only the inner field. This walker
    // was written for CopyrightOwner alone; the other three were left to the
    // generic path, which emits the CONTAINER under its bare name instead
    // (XMP-plus:ImageCreator rather than XMP-plus:ImageCreatorName) and so
    // matched nothing.
    const PLUS_SEQUENCES: &[(&str, &str)] = &[
        ("CopyrightOwner", "CopyrightOwnerName"),
        ("ImageCreator", "ImageCreatorName"),
        ("ImageSupplier", "ImageSupplierName"),
        ("Licensor", "LicensorName"),
    ];
    let mut out = Vec::new();
    for (container, field) in PLUS_SEQUENCES {
        out.extend(extract_plus_sequence_field(xml_bytes, container, field)?);
    }
    Ok(out)
}

/// Collects one PLUS container's inner field; see the table above.
fn extract_plus_sequence_field(
    xml_bytes: &[u8],
    container: &str,
    field: &str,
) -> Result<Vec<(String, String)>> {
    const PLUS_NS: &str = "http://ns.useplus.org/ldf/xmp/1.0/";

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut names: Vec<String> = Vec::new();
    let mut buf = Vec::new();
    let mut depth = 0usize;

    let mut owner_depth: Option<usize> = None;
    let mut name_depth: Option<usize> = None;
    let mut current_value = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                register_namespaces_from_element(&e, &mut resolver)?;
                let tag_name = extract_tag_name(&e)?;

                if owner_depth.is_none()
                    && is_property_in_namespace(&tag_name, container, PLUS_NS, &resolver)
                {
                    owner_depth = Some(depth);
                } else if owner_depth.is_some()
                    && name_depth.is_none()
                    && is_property_in_namespace(&tag_name, field, PLUS_NS, &resolver)
                {
                    name_depth = Some(depth);
                    current_value.clear();
                }
            }

            Ok(Event::End(_)) => {
                if name_depth == Some(depth) {
                    let value = current_value.trim().to_string();
                    if !value.is_empty() {
                        names.push(value);
                    }
                    name_depth = None;
                    current_value.clear();
                }
                if owner_depth == Some(depth) {
                    owner_depth = None;
                }
                depth = depth.saturating_sub(1);
            }

            Ok(Event::Text(e)) => {
                if name_depth.is_some()
                    && let Ok(decoded) = e.xml10_content()
                {
                    let unescaped =
                        quick_xml::escape::unescape(&decoded).unwrap_or_else(|_| decoded.clone());
                    current_value.push_str(&unescaped);
                }
            }

            Ok(Event::Empty(e)) => {
                register_namespaces_from_element(&e, &mut resolver)?;
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }

    if names.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![(format!("XMP:{field}"), names.join(", "))])
    }
}

/// Collects the flattened fields of the XMP-xmpBJ JobRef structure.
///
/// `JobRef` is `List => 'Bag'` over `%sJobRef` (XMP.pm:345-351, 1175), so its
/// shape is `xmpBJ:JobRef / rdf:Bag / rdf:li[parseType=Resource] /
/// stJob:{id,name,url}`. That is one level deeper than the bare
/// `parseType="Resource"` structs the generic pass walks, so the generic pass
/// reports only the container -- oxidex emitted `XMP:JobRef` where ExifTool
/// reports `XMP:JobRefName` (measured on combined-samples/Photoshop.psd).
///
/// Deliberately table-driven rather than generic: the flattened name is
/// `STRUCT_NAME . ucfirst(field)`, and only structures whose ExifTool
/// definition has actually been read can be named with confidence.
fn extract_job_ref_fields(xml_bytes: &[u8]) -> Result<Vec<(String, String)>> {
    const XMP_BJ_NS: &str = "http://ns.adobe.com/xap/1.0/bj/";
    const ST_JOB_NS: &str = "http://ns.adobe.com/xap/1.0/sType/Job#";
    // %sJobRef fields, in XMP.pm declaration order.
    const FIELDS: &[(&str, &str)] = &[
        ("id", "JobRefId"),
        ("name", "JobRefName"),
        ("url", "JobRefUrl"),
    ];

    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true);

    let mut resolver = NamespaceResolver::new();
    let mut collected: Vec<(&'static str, Vec<String>)> =
        FIELDS.iter().map(|(_, tag)| (*tag, Vec::new())).collect();
    let mut buf = Vec::new();
    let mut depth = 0usize;

    let mut job_ref_depth: Option<usize> = None;
    let mut field_depth: Option<usize> = None;
    let mut field_index: Option<usize> = None;
    let mut current_value = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                depth += 1;
                register_namespaces_from_element(&e, &mut resolver)?;
                let tag_name = extract_tag_name(&e)?;

                if job_ref_depth.is_none()
                    && is_property_in_namespace(&tag_name, "JobRef", XMP_BJ_NS, &resolver)
                {
                    job_ref_depth = Some(depth);
                } else if job_ref_depth.is_some() && field_depth.is_none() {
                    if let Some(index) = FIELDS.iter().position(|(field, _)| {
                        is_property_in_namespace(&tag_name, field, ST_JOB_NS, &resolver)
                    }) {
                        field_depth = Some(depth);
                        field_index = Some(index);
                        current_value.clear();
                    }
                }
            }

            Ok(Event::End(_)) => {
                if field_depth == Some(depth) {
                    let value = current_value.trim().to_string();
                    if let (Some(index), false) = (field_index, value.is_empty()) {
                        collected[index].1.push(value);
                    }
                    field_depth = None;
                    field_index = None;
                    current_value.clear();
                }
                if job_ref_depth == Some(depth) {
                    job_ref_depth = None;
                }
                depth = depth.saturating_sub(1);
            }

            Ok(Event::Text(e)) => {
                if field_depth.is_some()
                    && let Ok(decoded) = e.xml10_content()
                {
                    let unescaped =
                        quick_xml::escape::unescape(&decoded).unwrap_or_else(|_| decoded.clone());
                    current_value.push_str(&unescaped);
                }
            }

            Ok(Event::GeneralRef(e)) => {
                // quick-xml reports `&apos;` and friends as their own event
                // rather than as text. Without this arm the field value silently
                // lost the character: "This isn't a job" arrived as
                // "This isnt a job".
                if field_depth.is_some()
                    && let Ok(entity_name) = e.xml10_content()
                {
                    if let Ok(Some(ch)) = e.resolve_char_ref() {
                        current_value.push(ch);
                    } else if let Some(resolved) = resolve_predefined_entity(&entity_name) {
                        current_value.push_str(resolved);
                    } else {
                        current_value.push('&');
                        current_value.push_str(&entity_name);
                        current_value.push(';');
                    }
                }
            }

            Ok(Event::Empty(e)) => {
                register_namespaces_from_element(&e, &mut resolver)?;
            }

            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        buf.clear();
    }

    Ok(collected
        .into_iter()
        .filter(|(_, values)| !values.is_empty())
        .map(|(tag, values)| (format!("XMP:{tag}"), values.join(", ")))
        .collect())
}

/// ExifTool tag names for flattened structure fields whose reported name is
/// not the concatenated tag ID.
///
/// `AddFlattenedTags` builds the tag *ID* by concatenation, but a table may
/// pre-define that ID with its own `Name`, which is what gets reported --
/// e.g. `Iptc4xmpCore:CreatorContactInfo`/`CiAdrCity` has ID
/// `CreatorContactInfoCiAdrCity` but prints as `CreatorCity`. Without this
/// mapping the flattening pass emitted four such tags under their raw IDs on
/// SonyDSC-P2.jpg (measured 2026-07-27), which is four wrong tag names rather
/// than four closed gaps.
///
/// Transcribed mechanically from every `Flat => 1` entry that overrides
/// `Name` in ExifTool 13.55; entries the top-level pass cannot reach are
/// inert but kept so the table stays a faithful copy of its source.
const FLAT_TAG_RENAMES: &[(&str, &str)] = &[
    // XMP.pm:2688
    ("CreatorContactInfoCiAdrCity", "CreatorCity"),
    // XMP.pm:2689
    ("CreatorContactInfoCiAdrCtry", "CreatorCountry"),
    // XMP.pm:2690
    ("CreatorContactInfoCiAdrExtadr", "CreatorAddress"),
    // XMP.pm:2691
    ("CreatorContactInfoCiAdrPcode", "CreatorPostalCode"),
    // XMP.pm:2692
    ("CreatorContactInfoCiAdrRegion", "CreatorRegion"),
    // XMP.pm:2693
    ("CreatorContactInfoCiEmailWork", "CreatorWorkEmail"),
    // XMP.pm:2694
    ("CreatorContactInfoCiTelWork", "CreatorWorkTelephone"),
    // XMP.pm:2695
    ("CreatorContactInfoCiUrlWork", "CreatorWorkURL"),
    // DarwinCore.pm:95
    ("EventEventDate", "EventDate"),
    // DarwinCore.pm:96
    ("EventEventID", "EventID"),
    // DarwinCore.pm:97
    ("EventEventRemarks", "EventRemarks"),
    // DarwinCore.pm:98
    ("EventEventTime", "EventTime"),
    // XMP.pm:1193
    ("FontsComposite", "FontComposite"),
    // XMP.pm:1192
    ("FontsVersionString", "FontVersion"),
    // MWG.pm:506
    ("KeywordsHierarchy", "HierarchicalKeywords"),
    // MWG.pm:508
    ("KeywordsHierarchyApplied", "HierarchicalKeywords1Applied"),
    // MWG.pm:509
    ("KeywordsHierarchyChildren", "HierarchicalKeywords1Children"),
    // MWG.pm:511
    (
        "KeywordsHierarchyChildrenApplied",
        "HierarchicalKeywords2Applied",
    ),
    // MWG.pm:512
    (
        "KeywordsHierarchyChildrenChildren",
        "HierarchicalKeywords2Children",
    ),
    // MWG.pm:514
    (
        "KeywordsHierarchyChildrenChildrenApplied",
        "HierarchicalKeywords3Applied",
    ),
    // MWG.pm:515
    (
        "KeywordsHierarchyChildrenChildrenChildren",
        "HierarchicalKeywords3Children",
    ),
    // MWG.pm:517
    (
        "KeywordsHierarchyChildrenChildrenChildrenApplied",
        "HierarchicalKeywords4Applied",
    ),
    // MWG.pm:518
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildren",
        "HierarchicalKeywords4Children",
    ),
    // MWG.pm:520
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildrenApplied",
        "HierarchicalKeywords5Applied",
    ),
    // MWG.pm:521
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildrenChildren",
        "HierarchicalKeywords5Children",
    ),
    // MWG.pm:523
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildrenChildrenApplied",
        "HierarchicalKeywords6Applied",
    ),
    // MWG.pm:522
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildrenChildrenKeyword",
        "HierarchicalKeywords6",
    ),
    // MWG.pm:519
    (
        "KeywordsHierarchyChildrenChildrenChildrenChildrenKeyword",
        "HierarchicalKeywords5",
    ),
    // MWG.pm:516
    (
        "KeywordsHierarchyChildrenChildrenChildrenKeyword",
        "HierarchicalKeywords4",
    ),
    // MWG.pm:513
    (
        "KeywordsHierarchyChildrenChildrenKeyword",
        "HierarchicalKeywords3",
    ),
    // MWG.pm:510
    ("KeywordsHierarchyChildrenKeyword", "HierarchicalKeywords2"),
    // MWG.pm:507
    ("KeywordsHierarchyKeyword", "HierarchicalKeywords1"),
    // DarwinCore.pm:151
    ("MaterialSampleMaterialSampleID", "MaterialSampleID"),
    // DarwinCore.pm:207
    ("OccurrenceOccurrenceDetails", "OccurrenceDetails"),
    // DarwinCore.pm:208
    ("OccurrenceOccurrenceID", "OccurrenceID"),
    // DarwinCore.pm:209
    ("OccurrenceOccurrenceRemarks", "OccurrenceRemarks"),
    // DarwinCore.pm:210
    ("OccurrenceOccurrenceStatus", "OccurrenceStatus"),
    // DarwinCore.pm:224
    ("OrganismOrganismID", "OrganismID"),
    // DarwinCore.pm:225
    ("OrganismOrganismName", "OrganismName"),
    // DarwinCore.pm:226
    ("OrganismOrganismRemarks", "OrganismRemarks"),
    // DarwinCore.pm:227
    ("OrganismOrganismScope", "OrganismScope"),
    // XMP.pm:1073
    ("PageInfoImage", "PageImage"),
    // Microsoft.pm:347
    (
        "RegionInfoRegionsPersonDisplayName",
        "RegionPersonDisplayName",
    ),
    // Microsoft.pm:348
    (
        "RegionInfoRegionsPersonEmailDigest",
        "RegionPersonEmailDigest",
    ),
    // Microsoft.pm:349
    ("RegionInfoRegionsPersonLiveIdCID", "RegionPersonLiveIdCID"),
    // Microsoft.pm:350
    ("RegionInfoRegionsPersonSourceID", "RegionPersonSourceID"),
    // Microsoft.pm:346
    ("RegionInfoRegionsRectangle", "RegionRectangle"),
    // MWG.pm:481
    ("RegionsRegionList", "RegionList"),
    // XMP.pm:1208
    ("SwatchGroupsGroupName", "SwatchGroupName"),
    // XMP.pm:1209
    ("SwatchGroupsGroupType", "SwatchGroupType"),
    // DarwinCore.pm:307
    ("TaxonTaxonConceptID", "TaxonConceptID"),
    // DarwinCore.pm:308
    ("TaxonTaxonID", "TaxonID"),
    // DarwinCore.pm:309
    ("TaxonTaxonRank", "TaxonRank"),
    // DarwinCore.pm:310
    ("TaxonTaxonRemarks", "TaxonRemarks"),
    // XMP2.pl:514
    ("AboutCvTermCvTermId", "AboutCvTermId"),
    // XMP2.pl:515
    ("AboutCvTermCvTermName", "AboutCvTermName"),
    // XMP2.pl:516
    ("AboutCvTermCvTermRefinedAbout", "AboutCvTermRefinedAbout"),
    // XMP2.pl:553
    (
        "ArtworkOrObjectAOCircaDateCreated",
        "ArtworkCircaDateCreated",
    ),
    // XMP2.pl:556
    (
        "ArtworkOrObjectAOContentDescription",
        "ArtworkContentDescription",
    ),
    // XMP2.pl:557
    (
        "ArtworkOrObjectAOContributionDescription",
        "ArtworkContributionDescription",
    ),
    // XMP2.pl:542
    ("ArtworkOrObjectAOCopyrightNotice", "ArtworkCopyrightNotice"),
    // XMP2.pl:543
    ("ArtworkOrObjectAOCreator", "ArtworkCreator"),
    // XMP2.pl:552
    ("ArtworkOrObjectAOCreatorId", "ArtworkCreatorID"),
    // XMP2.pl:549
    (
        "ArtworkOrObjectAOCurrentCopyrightOwnerId",
        "ArtworkCopyrightOwnerID",
    ),
    // XMP2.pl:548
    (
        "ArtworkOrObjectAOCurrentCopyrightOwnerName",
        "ArtworkCopyrightOwnerName",
    ),
    // XMP2.pl:551
    ("ArtworkOrObjectAOCurrentLicensorId", "ArtworkLicensorID"),
    // XMP2.pl:550
    (
        "ArtworkOrObjectAOCurrentLicensorName",
        "ArtworkLicensorName",
    ),
    // XMP2.pl:544
    ("ArtworkOrObjectAODateCreated", "ArtworkDateCreated"),
    // XMP2.pl:558
    (
        "ArtworkOrObjectAOPhysicalDescription",
        "ArtworkPhysicalDescription",
    ),
    // XMP2.pl:545
    ("ArtworkOrObjectAOSource", "ArtworkSource"),
    // XMP2.pl:546
    ("ArtworkOrObjectAOSourceInvNo", "ArtworkSourceInventoryNo"),
    // XMP2.pl:555
    ("ArtworkOrObjectAOSourceInvURL", "ArtworkSourceInvURL"),
    // XMP2.pl:554
    ("ArtworkOrObjectAOStylePeriod", "ArtworkStylePeriod"),
    // XMP2.pl:547
    ("ArtworkOrObjectAOTitle", "ArtworkTitle"),
    // XMP2.pl:581
    (
        "EmbdEncRightsExprEncRightsExpr",
        "EmbeddedEncodedRightsExpr",
    ),
    // XMP2.pl:582
    (
        "EmbdEncRightsExprRightsExprEncType",
        "EmbeddedEncodedRightsExprType",
    ),
    // XMP2.pl:583
    (
        "EmbdEncRightsExprRightsExprLangId",
        "EmbeddedEncodedRightsExprLangID",
    ),
    // XMP2.pl:600
    (
        "LinkedEncRightsExprLinkedRightsExpr",
        "LinkedEncodedRightsExpr",
    ),
    // XMP2.pl:601
    (
        "LinkedEncRightsExprRightsExprEncType",
        "LinkedEncodedRightsExprType",
    ),
    // XMP2.pl:602
    (
        "LinkedEncRightsExprRightsExprLangId",
        "LinkedEncodedRightsExprLangID",
    ),
    // XMP2.pl:635
    (
        "PersonInImageWDetailsPersonCharacteristic",
        "PersonInImageCharacteristic",
    ),
    // XMP2.pl:636
    (
        "PersonInImageWDetailsPersonCharacteristicCvId",
        "PersonInImageCvTermCvId",
    ),
    // XMP2.pl:637
    (
        "PersonInImageWDetailsPersonCharacteristicCvTermId",
        "PersonInImageCvTermId",
    ),
    // XMP2.pl:638
    (
        "PersonInImageWDetailsPersonCharacteristicCvTermName",
        "PersonInImageCvTermName",
    ),
    // XMP2.pl:639
    (
        "PersonInImageWDetailsPersonCharacteristicCvTermRefinedAbout",
        "PersonInImageCvTermRefinedAbout",
    ),
    // XMP2.pl:640
    (
        "PersonInImageWDetailsPersonDescription",
        "PersonInImageDescription",
    ),
    // XMP2.pl:633
    ("PersonInImageWDetailsPersonId", "PersonInImageId"),
    // XMP2.pl:634
    ("PersonInImageWDetailsPersonName", "PersonInImageName"),
    // XMP2.pl:654
    (
        "ProductInImageProductDescription",
        "ProductInImageDescription",
    ),
    // XMP2.pl:653
    ("ProductInImageProductGTIN", "ProductInImageGTIN"),
    // XMP2.pl:652
    ("ProductInImageProductName", "ProductInImageName"),
    // XMP2.pl:668
    ("RegistryIdRegEntryRole", "RegistryEntryRole"),
    // XMP2.pl:666
    ("RegistryIdRegItemId", "RegistryItemID"),
    // XMP2.pl:667
    ("RegistryIdRegOrgId", "RegistryOrganisationID"),
    // XMP2.pl:1291
    ("SubVersionsFileName", "SubVersionFileName"),
    // XMP2.pl:1290
    ("SubVersionsVersRef", "SubVersionReference"),
    // XMP2.pl:1266
    ("TagStructureLabelName", "LabelName1"),
    // XMP2.pl:1269
    ("TagStructureParentReference", "ParentReference1"),
    // XMP2.pl:1267
    ("TagStructureReference", "Reference1"),
    // XMP2.pl:1268
    ("TagStructureSubLabels", "SubLabels1"),
    // XMP2.pl:1270
    ("TagStructureSubLabelsLabelName", "LabelName2"),
    // XMP2.pl:1273
    ("TagStructureSubLabelsParentReference", "ParentReference2"),
    // XMP2.pl:1271
    ("TagStructureSubLabelsReference", "Reference2"),
    // XMP2.pl:1272
    ("TagStructureSubLabelsSubLabels", "SubLabels2"),
    // XMP2.pl:1274
    ("TagStructureSubLabelsSubLabelsLabelName", "LabelName3"),
    // XMP2.pl:1277
    (
        "TagStructureSubLabelsSubLabelsParentReference",
        "ParentReference3",
    ),
    // XMP2.pl:1275
    ("TagStructureSubLabelsSubLabelsReference", "Reference3"),
    // XMP2.pl:1276
    ("TagStructureSubLabelsSubLabelsSubLabels", "SubLabels3"),
    // XMP2.pl:1278
    (
        "TagStructureSubLabelsSubLabelsSubLabelsLabelName",
        "LabelName4",
    ),
    // XMP2.pl:1281
    (
        "TagStructureSubLabelsSubLabelsSubLabelsParentReference",
        "ParentReference4",
    ),
    // XMP2.pl:1279
    (
        "TagStructureSubLabelsSubLabelsSubLabelsReference",
        "Reference4",
    ),
    // XMP2.pl:1280
    (
        "TagStructureSubLabelsSubLabelsSubLabelsSubLabels",
        "SubLabels4",
    ),
    // XMP2.pl:1282
    (
        "TagStructureSubLabelsSubLabelsSubLabelsSubLabelsLabelName",
        "LabelName5",
    ),
    // XMP2.pl:1285
    (
        "TagStructureSubLabelsSubLabelsSubLabelsSubLabelsParentReference",
        "ParentReference5",
    ),
    // XMP2.pl:1283
    (
        "TagStructureSubLabelsSubLabelsSubLabelsSubLabelsReference",
        "Reference5",
    ),
    // XMP2.pl:1284
    (
        "TagStructureSubLabelsSubLabelsSubLabelsSubLabelsSubLabels",
        "SubLabels5",
    ),
    // XMP2.pl:1286
    (
        "TagStructureSubLabelsSubLabelsSubLabelsSubLabelsSubLabelsLabelName",
        "LabelName6",
    ),
    // XMP2.pl:1288
    (
        "TagStructureSubLabelsSubLabelsSubLabelsSubLabelsSubLabelsParentReference",
        "ParentReference6",
    ),
    // XMP2.pl:1287
    (
        "TagStructureSubLabelsSubLabelsSubLabelsSubLabelsSubLabelsReference",
        "Reference6",
    ),
];

/// Structure properties whose own name contributes nothing to the flattened
/// tag ID.
///
/// `AddFlattenedTags` skips a property's name segment when the table declares
/// `FlatName => ''`, so `plus:Licensee`/`plus:LicenseeName` flattens to
/// `LicenseeName`, not `LicenseeLicenseeName`. Transcribed from every
/// `FlatName => ''` in ExifTool 13.59.
const FLAT_NAME_SUPPRESSED: &[&str] = &[
    // DarwinCore.pm:101, 131, 153, 247
    "GeologicalContext",
    "Identification",
    "MeasurementOrFact",
    "ResourceRelationship",
    // PLUS.pm:2324, 2329, 2334, 2488, 2494, 2500
    "Licensee",
    "EndUser",
    "Licensor",
    "CopyrightOwner",
    "ImageCreator",
    "ImageSupplier",
    // XMP.pm:1188
    "Fonts",
    // MWG.pm:537
    "Collections",
];

/// Reports whether a structure property's name is dropped from flattened IDs.
/// Whether `element` carries RDF shorthand attributes -- namespaced attributes
/// that are structure fields rather than XML bookkeeping.
fn has_shorthand_fields(element: &BytesStart) -> bool {
    element.attributes().flatten().any(|attr| {
        std::str::from_utf8(attr.key.as_ref())
            .ok()
            .and_then(|key| key.split_once(':'))
            .is_some_and(|(prefix, _)| !matches!(prefix, "rdf" | "xml" | "xmlns" | "x"))
    })
}

fn is_flat_name_suppressed(local_name: &str) -> bool {
    FLAT_NAME_SUPPRESSED.contains(&local_name)
}

/// Maps a concatenated flattened tag ID to the name ExifTool reports for it.
fn exiftool_flat_tag_name(flat_id: &str) -> &str {
    FLAT_TAG_RENAMES
        .iter()
        .find(|(id, _)| *id == flat_id)
        .map_or(flat_id, |(_, name)| *name)
}

/// Checks whether a qualified name resolves to the RDF namespace.
fn is_rdf_namespace(tag_name: &str, resolver: &NamespaceResolver) -> bool {
    NamespaceResolver::extract_prefix(tag_name)
        .and_then(|prefix| resolver.resolve_prefix(prefix))
        .is_some_and(|uri| uri == "http://www.w3.org/1999/02/22-rdf-syntax-ns#")
}

/// Reports whether an element carries `rdf:parseType="Resource"`.
fn has_parse_type_resource(element: &BytesStart) -> bool {
    element.attributes().flatten().any(|attr| {
        std::str::from_utf8(attr.key.as_ref()).is_ok_and(|key| key.ends_with(":parseType"))
            && attr.value.as_ref() == b"Resource"
    })
}

/// Returns the `xml:lang` attribute value of an element, if present.
fn xml_lang_attribute(element: &BytesStart) -> Option<String> {
    element.attributes().flatten().find_map(|attr| {
        if std::str::from_utf8(attr.key.as_ref()).ok()? == "xml:lang" {
            Some(std::str::from_utf8(&attr.value).ok()?.to_string())
        } else {
            None
        }
    })
}

/// Capitalises the first character, as ExifTool's `ucfirst` does when building
/// flattened XMP tag names.
fn ucfirst(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Checks a property's local name and resolved namespace URI.
fn is_property_in_namespace(
    tag_name: &str,
    expected_local_name: &str,
    expected_namespace: &str,
    resolver: &NamespaceResolver,
) -> bool {
    let Some(prefix) = NamespaceResolver::extract_prefix(tag_name) else {
        return false;
    };

    NamespaceResolver::extract_local_name(tag_name) == expected_local_name
        && resolver.resolve_prefix(prefix) == Some(expected_namespace)
}

#[cfg(test)]
mod legacy_adobe_xmp_tests {
    use super::*;

    /// The XMP packet Photoshop 7.0 writes, as found in
    /// /tmp/oxidex-exiftool-cache/combined-samples/Photoshop.psd: the wrapper
    /// is `x:xapmeta` with an `x:xaptk` toolkit attribute, and rdf:Description
    /// carries an unprefixed `about`.
    const LEGACY_PACKET: &[u8] = br#"
        <x:xapmeta xmlns:x='adobe:ns:meta/' x:xaptk='XMP toolkit 2.8.2-33, framework 1.5'>
        <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>
         <rdf:Description about='uuid:c197d41e-f8f7-11d9-b03e-c023c3939af5'
          xmlns:stJob='http://ns.adobe.com/xap/1.0/sType/Job#'
          xmlns:xapBJ='http://ns.adobe.com/xap/1.0/bj/'>
          <xapBJ:JobRef>
           <rdf:Bag>
            <rdf:li rdf:parseType='Resource'>
             <stJob:name>This isn&apos;t a job</stJob:name>
            </rdf:li>
           </rdf:Bag>
          </xapBJ:JobRef>
         </rdf:Description>
        </rdf:RDF>
        </x:xapmeta>"#;

    fn value_of(results: &[(String, String)], tag: &str) -> Option<String> {
        results
            .iter()
            .find(|(name, _)| name == tag)
            .map(|(_, value)| value.clone())
    }

    #[test]
    fn xaptk_on_xapmeta_yields_the_toolkit_string() {
        // XMP.pm %recognizedAttrs maps both 'x:xmptk' and 'x:xaptk' onto
        // XMPToolkit. Recognising only the modern spelling dropped the tag on
        // every file an Adobe toolkit older than 2004 wrote.
        let results = parse_xmp(LEGACY_PACKET).unwrap();
        assert_eq!(
            value_of(&results, "XMP:XMPToolkit").as_deref(),
            Some("XMP toolkit 2.8.2-33, framework 1.5")
        );
    }

    #[test]
    fn an_unprefixed_about_attribute_is_still_rdf_about() {
        // XMP.pm:4081-4086 -- an attribute with no prefix takes the namespace
        // of the element it sits on, and this attribute only ever sits on
        // rdf:Description, so bare `about` IS `rdf:about`.
        let results = parse_xmp(LEGACY_PACKET).unwrap();
        assert_eq!(
            value_of(&results, "XMP:About").as_deref(),
            Some("uuid:c197d41e-f8f7-11d9-b03e-c023c3939af5")
        );
    }

    #[test]
    fn job_ref_is_reported_by_its_flattened_field_name() {
        // %sJobRef (XMP.pm:345-351) has STRUCT_NAME 'JobRef' and a 'name'
        // field, so ExifTool reports JobRefName. The container tag JobRef is
        // not something ExifTool ever emits, so it must not survive alongside.
        let results = parse_xmp(LEGACY_PACKET).unwrap();
        assert_eq!(
            value_of(&results, "XMP:JobRefName").as_deref(),
            Some("This isn't a job"),
            "the &apos; entity must survive as an apostrophe"
        );
        assert!(
            value_of(&results, "XMP:JobRef").is_none(),
            "the struct container must not be reported next to its flattened field"
        );
    }

    #[test]
    fn the_modern_spelling_still_works() {
        // Guard against fixing the legacy spelling by replacing the modern one.
        let modern = br#"
            <x:xmpmeta xmlns:x='adobe:ns:meta/' x:xmptk='Image::ExifTool 12.46'>
            <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>
             <rdf:Description rdf:about='uuid:modern'
              xmlns:dc='http://purl.org/dc/elements/1.1/'>
              <dc:format>image/jpeg</dc:format>
             </rdf:Description>
            </rdf:RDF>
            </x:xmpmeta>"#;
        let results = parse_xmp(modern).unwrap();
        assert_eq!(
            value_of(&results, "XMP:XMPToolkit").as_deref(),
            Some("Image::ExifTool 12.46")
        );
        assert_eq!(
            value_of(&results, "XMP:About").as_deref(),
            Some("uuid:modern")
        );
    }

    #[test]
    fn a_packet_without_a_job_ref_gains_no_job_tags() {
        let plain = br#"
            <x:xmpmeta xmlns:x='adobe:ns:meta/'>
            <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>
             <rdf:Description rdf:about=''
              xmlns:dc='http://purl.org/dc/elements/1.1/'>
              <dc:format>image/jpeg</dc:format>
             </rdf:Description>
            </rdf:RDF>
            </x:xmpmeta>"#;
        let results = parse_xmp(plain).unwrap();
        assert!(
            results
                .iter()
                .all(|(tag, _)| !tag.starts_with("XMP:JobRef")),
            "found unexpected JobRef tags: {results:?}"
        );
    }
}

#[cfg(test)]
mod about_cv_term_tests {
    use super::*;

    #[test]
    fn test_extract_about_cv_term_values() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description
                  xmlns:ext="http://iptc.org/std/Iptc4xmpExt/2008-02-29/">
                <ext:AboutCvTerm>
                  <rdf:Bag>
                    <rdf:li rdf:parseType="Resource">
                      <ext:CvId>1</ext:CvId>
                      <ext:CvTermName><rdf:Alt><rdf:li xml:lang="x-default">one</rdf:li></rdf:Alt></ext:CvTermName>
                    </rdf:li>
                    <rdf:li rdf:parseType="Resource">
                      <ext:CvId>2</ext:CvId>
                      <ext:CvTermName><rdf:Alt><rdf:li xml:lang="x-default">two</rdf:li></rdf:Alt></ext:CvTermName>
                    </rdf:li>
                    <rdf:li rdf:parseType="Resource">
                      <ext:CvId>3</ext:CvId>
                      <ext:CvTermName><rdf:Alt><rdf:li xml:lang="x-default">three</rdf:li></rdf:Alt></ext:CvTermName>
                    </rdf:li>
                  </rdf:Bag>
                </ext:AboutCvTerm>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        let cv_ids: Vec<_> = result
            .iter()
            .filter(|(tag, _)| tag == "XMP:AboutCvTermCvId")
            .map(|(_, value)| value.as_str())
            .collect();
        let names: Vec<_> = result
            .iter()
            .filter(|(tag, _)| tag == "XMP:AboutCvTermName")
            .map(|(_, value)| value.as_str())
            .collect();

        assert_eq!(cv_ids, vec!["1, 2, 3"]);
        assert_eq!(names, vec!["one, two, three"]);

        // `exiftool -json XMP8.xmp` serializes both flattened IPTC lists as
        // JSON arrays. Keep that structure for API callers while parse_xmp's
        // plain-text view above remains comma-separated.
        let typed = parse_xmp_typed(xml).unwrap();
        assert_eq!(
            typed
                .iter()
                .find(|(tag, _)| tag == "XMP:AboutCvTermCvId")
                .map(|(_, value)| value),
            Some(&XmpValue::List(vec![
                "1".to_string(),
                "2".to_string(),
                "3".to_string()
            ]))
        );
        assert_eq!(
            typed
                .iter()
                .find(|(tag, _)| tag == "XMP:AboutCvTermName")
                .map(|(_, value)| value),
            Some(&XmpValue::List(vec![
                "one".to_string(),
                "two".to_string(),
                "three".to_string(),
            ]))
        );
    }
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

/// Checks if a tag name represents an x:xmpmeta element.
///
/// The xmpmeta element wraps XMP data and may contain the XMPToolkit attribute.
///
/// Pre-2004 Adobe toolkits spell the same element `x:xapmeta` -- that is what
/// Photoshop 7.0 writes, and it is the wrapper in
/// /tmp/oxidex-exiftool-cache/combined-samples/Photoshop.psd. ExifTool reads
/// both spellings (XMP.pm %recognizedAttrs lists `x:xmptk` and `x:xaptk` side
/// by side), so the toolkit string must not be lost just because the file is old.
fn is_xmpmeta(tag_name: &str) -> bool {
    // Check for x:xmpmeta or xmpmeta (with or without prefix)
    matches!(tag_name, "x:xmpmeta" | "xmpmeta" | "x:xapmeta" | "xapmeta")
}

/// Extracts XMPToolkit from x:xmpmeta element attributes.
///
/// The XMPToolkit value comes from the x:xmptk attribute on the x:xmpmeta element:
/// `<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Image::ExifTool 12.46">`
fn extract_xmpmeta_attributes(
    element: &BytesStart,
    results: &mut Vec<(String, String)>,
) -> Result<()> {
    for attr in element.attributes().flatten() {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in attribute key: {}", e))
        })?;

        // Check for x:xmptk or xmptk attribute (XMP Toolkit version).
        // `x:xaptk` is the older Adobe spelling of the same attribute; XMP.pm
        // maps both onto the XMPToolkit tag.
        if matches!(key, "x:xmptk" | "xmptk" | "x:xaptk" | "xaptk") {
            let value = std::str::from_utf8(&attr.value).map_err(|e| {
                ExifToolError::parse_error(format!("Invalid UTF-8 in XMPToolkit value: {}", e))
            })?;

            // Only add non-empty XMPToolkit values
            if !value.trim().is_empty() {
                results.push(("XMP:XMPToolkit".to_string(), value.trim().to_string()));
            }
        }
    }
    Ok(())
}

/// Extracts XMP properties from rdf:Description element attributes.
///
/// This handles two types of attributes:
/// 1. rdf:about - the subject URI, extracted as XMP:About
/// 2. Property shorthand - XMP properties written as attributes (e.g., xmp:Rating="5")
///
/// Example:
/// ```xml
/// <rdf:Description rdf:about="uuid:faf5bdd5-ba3d-11da-ad31-d33d75182f1b"
///                  xmp:CreateDate="2023-01-15T10:30:00"
///                  xmp:ModifyDate="2023-01-20T14:00:00">
/// ```
fn extract_description_attributes(
    element: &BytesStart,
    resolver: &NamespaceResolver,
    results: &mut Vec<(String, String)>,
) -> Result<()> {
    for attr in element.attributes().flatten() {
        let key = std::str::from_utf8(attr.key.as_ref()).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in attribute key: {}", e))
        })?;

        let value = std::str::from_utf8(&attr.value).map_err(|e| {
            ExifToolError::parse_error(format!("Invalid UTF-8 in attribute value: {}", e))
        })?;

        // Skip namespace declarations (xmlns:xxx)
        if key.starts_with("xmlns") {
            continue;
        }

        // Handle rdf:about attribute (the subject URI).
        //
        // An attribute with no prefix inherits the namespace of the element it
        // sits on (XMP.pm:4081-4086: "assume same namespace as parent"), and
        // this function only ever runs on rdf:Description. So a bare
        // `about='uuid:...'` -- what Photoshop 7.0 writes, and what
        // combined-samples/Photoshop.psd contains -- is the same property as
        // `rdf:about` and must produce the same About tag.
        if key == "rdf:about" || key == "about" {
            // An empty rdf:about is the "no subject URI" default every writer
            // emits; ExifTool reports no About tag for it.
            if !value.trim().is_empty() {
                results.push(("XMP:About".to_string(), value.trim().to_string()));
            }
            continue;
        }

        // Skip other rdf: attributes (rdf:parseType, rdf:resource, etc.)
        if key.starts_with("rdf:") {
            continue;
        }

        // Handle XMP property shorthand (properties as attributes)
        // These are namespace-prefixed attributes like xmp:Rating="5".
        // An EMPTY one is still a property ExifTool reports (with an empty
        // value) -- GCamera:GFileMetadata="" on GooglePixel10.jpg.
        if key.contains(':') {
            let prefixed_name = format_tag_name(key, resolver);
            results.push((prefixed_name, value.trim().to_string()));
        }
    }
    Ok(())
}

/// Checks if a tag name represents an rdf:Description element.
fn is_rdf_description(tag_name: &str, resolver: &NamespaceResolver) -> bool {
    if let Some(prefix) = NamespaceResolver::extract_prefix(tag_name) {
        let local_name = NamespaceResolver::extract_local_name(tag_name);
        if local_name == "Description"
            && let Some(uri) = resolver.resolve_prefix(prefix)
        {
            return uri == "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
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
        if let Some(uri) = resolver.resolve_prefix(prefix)
            && uri == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
        {
            // Skip RDF structural elements
            return !matches!(
                local_name,
                "Bag" | "Seq" | "Alt" | "Description" | "RDF" | "li"
            );
        }

        // It's a property in a non-RDF namespace (xmp, dc, exif, etc.)
        return true;
    }

    // No namespace prefix - treat as simple property
    true
}

/// Checks if a tag is an rdf:Bag, rdf:Seq, or rdf:Alt container.
fn is_collection_container(tag_name: &str, resolver: &NamespaceResolver) -> bool {
    if let Some(prefix) = NamespaceResolver::extract_prefix(tag_name) {
        let local_name = NamespaceResolver::extract_local_name(tag_name);
        if let Some(uri) = resolver.resolve_prefix(prefix)
            && uri == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
        {
            return matches!(local_name, "Bag" | "Seq" | "Alt");
        }
    }
    false
}

/// Checks if a tag is an rdf:li element.
fn is_rdf_li(tag_name: &str, resolver: &NamespaceResolver) -> bool {
    if let Some(prefix) = NamespaceResolver::extract_prefix(tag_name) {
        let local_name = NamespaceResolver::extract_local_name(tag_name);
        if let Some(uri) = resolver.resolve_prefix(prefix)
            && uri == "http://www.w3.org/1999/02/22-rdf-syntax-ns#"
        {
            return local_name == "li";
        }
    }
    false
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

/// Properties ExifTool reports under a different name than the XMP schema's
/// own local name.
///
/// Keyed on (family prefix, local name) because the rename is namespace
/// specific: `tiff:ImageLength` is ExifTool's `ImageHeight`, but a bare
/// `ImageLength` in some other schema is not.
const PROPERTY_RENAMES: &[(&str, &str, &str)] = &[
    // XMP.pm: the photoshop namespace's ICCProfile is reported with the
    // "Name" suffix, matching the PLUS sequences above it.
    ("XMP-photoshop", "ICCProfile", "ICCProfileName"),
    // plus:Version is the PLUS schema version, not a generic Version.
    ("XMP-plus", "Version", "PLUSVersion"),
    // TIFF calls it ImageLength; every ExifTool group calls it ImageHeight.
    ("XMP-tiff", "ImageLength", "ImageHeight"),
    // XMP.pm:2143-2144 -- the EXIF schema's pixel-dimension properties are
    // reported under the same names the EXIF group uses.
    ("XMP-exif", "PixelXDimension", "ExifImageWidth"),
    ("XMP-exif", "PixelYDimension", "ExifImageHeight"),
    // XMP.pm:2096 -- `ExposureBiasValue => { Name => 'ExposureCompensation' }`.
    ("XMP-exif", "ExposureBiasValue", "ExposureCompensation"),
    // XMP.pm:2068 -- `ISOSpeedRatings => { Name => 'ISO' }` (the property is
    // deprecated in the XMP spec, but every camera that writes exif: XMP at
    // all still writes this spelling).
    ("XMP-exif", "ISOSpeedRatings", "ISO"),
    // XMP.pm:2247 -- `FocalLengthIn35mmFilm => { Name => 'FocalLengthIn35mmFormat' }`.
    (
        "XMP-exif",
        "FocalLengthIn35mmFilm",
        "FocalLengthIn35mmFormat",
    ),
    // XMP.pm:2350 -- exif:GPSTimeStamp is a full date/time in XMP (unlike the
    // time-only EXIF tag of the same name), so ExifTool renames it to keep the
    // two from being copied into each other.
    ("XMP-exif", "GPSTimeStamp", "GPSDateTime"),
];

/// Properties renamed by ExifTool in schemas that oxidex files under the plain
/// `XMP` family, so [`PROPERTY_RENAMES`]'s family key cannot tell them apart.
/// Keyed on the namespace URI instead.
const NAMESPACE_PROPERTY_RENAMES: &[(&str, &str, &str)] = &[
    // XMP.pm:1460 -- `Temperature => { Name => 'ColorTemperature' }`.
    (
        "http://ns.adobe.com/camera-raw-settings/1.0/",
        "Temperature",
        "ColorTemperature",
    ),
    // Microsoft.pm:248 -- MicrosoftPhoto's own Rating is a percentage, not the
    // 0-5 star xmp:Rating, so ExifTool reports it under a distinct name.
    (
        "http://ns.microsoft.com/photo/1.0",
        "Rating",
        "RatingPercent",
    ),
    // Google.pm:117-122
    (
        "http://ns.google.com/photos/1.0/image/",
        "Data",
        "ImageData",
    ),
    (
        "http://ns.google.com/photos/1.0/image/",
        "Mime",
        "ImageMimeType",
    ),
    // Google.pm:229
    (
        "http://ns.google.com/photos/1.0/depthmap/",
        "Data",
        "DepthImage",
    ),
    // Google.pm, %Image::ExifTool::Google::GCamera
    (
        "http://ns.google.com/photos/1.0/camera/",
        "hdrp_makernote",
        "HDRPMakerNote",
    ),
    (
        "http://ns.google.com/photos/1.0/camera/",
        "HdrPlusMakernote",
        "HDRPlusMakerNote",
    ),
    (
        "http://ns.google.com/photos/1.0/camera/",
        "shot_log_data",
        "ShotLogData",
    ),
];

/// Applies [`PROPERTY_RENAMES`], leaving anything unlisted untouched.
fn exiftool_property_name<'a>(family: &str, local: &'a str) -> &'a str {
    PROPERTY_RENAMES
        .iter()
        .find(|(f, l, _)| *f == family && *l == local)
        // The replacement is a 'static str, which outlives 'a.
        .map_or(local, |(_, _, renamed)| *renamed)
}

/// Applies [`NAMESPACE_PROPERTY_RENAMES`], keyed on the property's namespace
/// URI. `local` is the raw local name, before the leading letter is capitalized
/// -- Google writes `hdrp_makernote` and `shot_log_data` in lower case.
fn exiftool_property_name_for_uri<'a>(uri: &str, local: &'a str) -> &'a str {
    NAMESPACE_PROPERTY_RENAMES
        .iter()
        .find(|(u, l, _)| *u == uri && *l == local)
        .map_or(local, |(_, _, renamed)| *renamed)
}

/// XMP properties ExifTool decodes from base64 before printing. Both are
/// flagged `Binary => 1` on top of a `ValueConv => DecodeBase64`, so the
/// reported length is the DECODED byte count (Google.pm, GImage/GDepth).
const BASE64_DECODED_BINARY_TAGS: [&str; 2] = ["XMP:ImageData", "XMP:DepthImage"];

/// XMP properties flagged `Binary => 1` with NO ValueConv: the value stays the
/// base64 text and ExifTool prints the length of that text, not of the bytes it
/// encodes. GooglePixel6a.jpg's `GCamera:hdrp_makernote` is 79648 characters
/// long and ExifTool reports 79648 bytes, where the decoded payload is 59734.
const RAW_BINARY_TAGS: [&str; 3] = [
    "XMP:HDRPMakerNote",
    "XMP:HDRPlusMakerNote",
    "XMP:ShotLogData",
];

/// Renders a base64 XMP property the way ExifTool renders a binary tag.
///
/// Returns `None` when the payload is not valid base64, in which case the raw
/// value is kept rather than a fabricated byte count reported.
fn base64_binary_placeholder(value: &str) -> Option<String> {
    use base64::Engine;
    let compact: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&compact))
        .ok()?;
    Some(format!(
        "(Binary data {} bytes, use -b option to extract)",
        decoded.len()
    ))
}

/// Formats a tag name to match ExifTool's XMP output conventions.
///
/// ExifTool uses a simplified "XMP:" prefix for most common XMP properties,
/// regardless of their namespace. This function uses namespace URI resolution
/// to determine the correct family prefix.
///
/// XMP properties are returned with these prefixes:
/// - dc:title -> XMP:Title (Dublin Core uses simplified XMP: prefix and Title-case)
/// - dc:rights -> XMP:Rights (Dublin Core uses simplified XMP: prefix and Title-case)
/// - xmp:Creator -> XMP:Creator (Core XMP uses simplified XMP: prefix)
/// - exif:Make -> XMP-exif:Make (EXIF namespace uses XMP-exif: prefix)
/// - tiff:Model -> XMP-tiff:Model (TIFF namespace uses XMP-tiff: prefix)
fn format_tag_name(qname: &str, resolver: &NamespaceResolver) -> String {
    use super::namespace_mapping::namespace_to_family;

    let mut local_name = NamespaceResolver::extract_local_name(qname).to_string();
    // XMP encodes otherwise-invalid property-name characters as U+2182 plus
    // four hexadecimal digits. ExifTool's generated tag ID drops the marker
    // but retains those digits (XMP3.xmp).
    local_name.retain(|ch| ch != '\u{2182}');

    // Extract namespace prefix from the qualified name
    if let Some(prefix) = NamespaceResolver::extract_prefix(qname) {
        // Resolve the namespace URI from the prefix
        let namespace_uri = resolver.resolve_prefix(prefix);
        let family_prefix = if let Some(namespace_uri) = namespace_uri {
            // Use namespace mapping to get ExifTool family prefix
            namespace_to_family(namespace_uri).unwrap_or("XMP")
        } else {
            // Unknown namespace - use generic XMP prefix
            "XMP"
        };

        // Schemas oxidex files under the plain XMP family need the rename
        // applied on the raw local name, before capitalization: Google writes
        // `hdrp_makernote`, not `Hdrp_makernote`.
        if let Some(uri) = namespace_uri {
            let renamed = exiftool_property_name_for_uri(uri, &local_name);
            if renamed != local_name {
                return format!("{}:{}", family_prefix, renamed);
            }
        }

        // XMP.pm reports exif:PixelYDimension as ExifImageHeight in the
        // generic XMP family rather than under XMP-exif (XMP.xmp).
        if family_prefix == "XMP-exif" && local_name == "PixelYDimension" {
            return "XMP:ExifImageHeight".to_string();
        }

        // ExifTool capitalizes the first letter of all XMP property names
        // to create consistent PascalCase tag names (e.g., album → Album)
        if !local_name.is_empty() {
            local_name = capitalize_first_letter(&local_name);
        }

        // Some properties are reported by ExifTool under a different name.
        let reported = exiftool_property_name(family_prefix, &local_name);

        // Format with the appropriate family prefix
        format!("{}:{}", family_prefix, reported)
    } else {
        // No namespace prefix - use generic "XMP:" prefix
        // Still capitalize to match ExifTool's PascalCase convention
        if !local_name.is_empty() {
            local_name = capitalize_first_letter(&local_name);
        }
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

/// Formats XMP values to match ExifTool output conventions.
///
/// Applies special formatting for specific XMP tags:
/// - Urgency: Adds human-readable description (e.g., "8" -> "8 (least urgent)")
/// - EXIF enum tags: Decodes numeric values to human-readable strings
/// - TIFF numeric tags: Formats numeric values appropriately
/// - EXIF exposure tags: Decodes exposure mode, metering mode, etc.
/// - Photoshop color tags: Decodes color mode values
///
/// # Namespace-specific formatting:
///
/// - **Dublin Core (dc:)**: Title, Creator, Subject, Description, Language, Rights
/// - **Photoshop**: AuthorsPosition, Caption, CreditLine, Source, CopyrightNotice, Instructions
/// - **Camera Raw Settings (crs:)**: CameraRawInfo, ProcessingParameters
/// - **TIFF (tiff:)**: Make, Model, XResolution, YResolution, Software, DateTime
/// - **EXIF (exif:)**: ISO, ShutterSpeed, Aperture, ExposureCompensation, FocalLength
/// - **Basic Job Ticket (xmpBJ:)**: JobName, CreationDate, Status
fn format_xmp_value(tag: &str, value: &str) -> String {
    // Extract local tag name (after colon)
    let local_name = tag.split(':').last().unwrap_or(tag);

    // Binary properties print as a byte count, not the payload.
    if RAW_BINARY_TAGS.contains(&tag) {
        return format!(
            "(Binary data {} bytes, use -b option to extract)",
            value.len()
        );
    }
    if BASE64_DECODED_BINARY_TAGS.contains(&tag)
        && let Some(placeholder) = base64_binary_placeholder(value)
    {
        return placeholder;
    }

    // PLUS controlled-vocabulary properties are URIs until both of `%plusVocab`'s
    // conversions have run; see `plus_vocab`.
    if let Some(converted) = super::plus_vocab::convert(local_name, value) {
        return converted;
    }

    // PLUS.pm:2339-2342 assigns the MediaMatrix PrintConv to this one
    // property. Its fallback preserves unknown IDs, so decoding the known
    // table entries never fabricates a value for an unrecognized code.
    if tag == "XMP-plus:MediaSummaryCode" {
        return format_plus_media_summary_code(value);
    }

    // XMP.pdf::Trapped removes one leading PDF name slash before its
    // True/False/Unknown PrintConv (ExifTool XMP.pm:1232-1237).
    if tag == "XMP:Trapped" {
        return value.strip_prefix('/').unwrap_or(value).to_string();
    }

    match local_name {
        // IPTC Urgency (0-8 scale)
        "Urgency" => format_iptc_urgency(value),

        // EXIF enum tags that appear in XMP
        "ColorSpace" => decode_xmp_color_space(value),
        "Contrast" | "Saturation" if tag.starts_with("XMP-exif:") => {
            decode_xmp_contrast_or_saturation(value)
        }
        // XMP.pm:2011-2024 gives this exif: Seq its own PrintConv. Do not
        // apply it to an unrelated property with the same local name.
        "ComponentsConfiguration" if tag.starts_with("XMP-exif:") => {
            decode_xmp_components_configuration(value)
        }
        "CustomRendered" => decode_xmp_custom_rendered(value),
        "ExposureMode" => decode_xmp_exposure_mode(value),
        "FileSource" => decode_xmp_file_source(value),
        "FocalPlaneResolutionUnit" | "ResolutionUnit" => decode_xmp_resolution_unit(value),
        "MeteringMode" => decode_xmp_metering_mode(value),
        "Orientation" => decode_xmp_orientation(value),
        "SceneCaptureType" => decode_xmp_scene_capture_type(value),
        // Enum properties the XMP schema stores as bare integers, which share
        // the EXIF tag of the same name. Decoding them here rather than
        // leaving the number is what ExifTool does (the XMP-exif table inherits
        // the EXIF PrintConv), and is required now that XMP scalars keep their
        // text form instead of being re-parsed as numbers downstream.
        "ExposureProgram" => decode_via_tiff_enum(0x8822, value),
        "GainControl" => decode_via_tiff_enum(0xA407, value),
        "LightSource" => decode_via_tiff_enum(0x9208, value),
        "SceneType" => decode_via_tiff_enum(0xA301, value),
        "ComponentsConfiguration" if tag == "XMP-exif:ComponentsConfiguration" => match value {
            "0" => "-".to_string(),
            "1" => "Y".to_string(),
            "2" => "Cb".to_string(),
            "3" => "Cr".to_string(),
            "4" => "R".to_string(),
            "5" => "G".to_string(),
            "6" => "B".to_string(),
            _ => value.to_string(),
        },
        // XMP-exif inherits the EXIF Sharpness PrintConv. Camera Raw also
        // defines a numeric Sharpness property, so this must stay scoped to
        // the EXIF schema rather than converting every XMP tag with the same
        // local name (crs:Sharpness=0 is an adjustment amount, not Normal).
        "Sharpness" if tag.starts_with("XMP-exif:") => decode_via_tiff_enum(0xA40A, value),
        "SubjectDistanceRange" => decode_via_tiff_enum(0xA40C, value),
        // XMP-exif is the only XMP schema in ExifTool 13.59 that declares a
        // SensingMethod, and its PrintConv is *not* the EXIF 0xa217 one --
        // see `decode_xmp_sensing_method`. Scoping matters twice over here:
        // the labels differ, and the unnamed-code fallback wraps whatever it
        // is handed in `Unknown (...)`, which must not be inflicted on an
        // unrelated property that merely shares the local name.
        "SensingMethod" if tag.starts_with("XMP-exif:") => decode_xmp_sensing_method(value),
        "WhiteBalance" => decode_xmp_white_balance(value),
        "YCbCrPositioning" => decode_xmp_ycbcr_positioning(value),
        "ColorMode" => decode_xmp_color_mode(value),
        "PhotometricInterpretation" => decode_xmp_photometric_interpretation(value),
        // The exif:Flash struct keeps its PrintConvs wherever it is nested:
        // inside mwg-rs Extensions the flattened name is
        // RegionExtensionsFlashMode, not FlashMode (XMP5.xmp).
        name if name.ends_with("FlashMode") => decode_xmp_flash_mode(value),
        name if name.ends_with("FlashReturn") => decode_xmp_flash_return(value),
        // The same struct's three `boolean` fields (XMP.pm:2139/2157/2158
        // declare Fired/Function/RedEyeMode with `%boolConv`). Suffix-matched
        // for the same nesting reason as Mode/Return above.
        name if name.ends_with("FlashFired")
            || name.ends_with("FlashFunction")
            || name.ends_with("FlashRedEyeMode") =>
        {
            decode_xmp_boolean(value)
        }

        // Camera Raw Settings - numeric parameters
        "ProcessingParameters" => format_camera_raw_parameters(value),

        // TIFF numeric tags - resolution and dimensions
        "XResolution" | "YResolution" => format_tiff_resolution(value),

        // EXIF exposure tags - numeric or enum
        "ISO" => format_exif_iso(value),
        "ShutterSpeed" => format_exif_shutter_speed(value),
        "Aperture" => format_exif_aperture(value),
        // XMP.pm:2096-2099 gives PrintFraction to exif:ExposureBiasValue alone
        // (renamed ExposureCompensation by PROPERTY_RENAMES). Microsoft's
        // Windows Live Photo Gallery writes an unrelated
        // `MSImagingV1:ExposureCompensation`, which ExifTool reports verbatim
        // as "0.080000"; without this guard it came back "+0.08" -- wrong in
        // kind, not just precision. Falling through leaves it to the generic
        // rational/verbatim handling every sibling property in that vendor
        // block already gets.
        "ExposureCompensation" if tag.starts_with("XMP-exif:") => {
            format_exif_exposure_compensation(value)
        }
        "FocalLength" => format_exif_focal_length(value),

        // XMP.pm:2088/2102 -- ApertureValue and MaxApertureValue are APEX,
        // ValueConv 'sqrt(2) ** $val', PrintConv sprintf("%.1f").
        "ApertureValue" | "MaxApertureValue" => format_xmp_apex_aperture(value),

        // XMP.pm:2081 -- ShutterSpeedValue is APEX,
        // ValueConv 'abs($val)<100 ? 1/(2**$val) : 0', PrintConv
        // PrintExposureTime.
        "ShutterSpeedValue" => format_xmp_apex_shutter_speed(value),

        // XMP.pm:2173-2174 -- plain rationals with no PrintConv, so ExifTool
        // prints the quotient with Perl's default 15-significant-digit format.
        "FocalPlaneXResolution" | "FocalPlaneYResolution" => format_xmp_plain_rational(value),

        // XMP.pm:2047-2051 -- FNumber is a rational whose PrintConv is
        // Exif::PrintFNumber.
        "FNumber" => format_xmp_fnumber(value),

        // XMP.pm:2042-2046 -- ExposureTime is a rational whose PrintConv is
        // Exif::PrintExposureTime.
        "ExposureTime" => match parse_xmp_number(value) {
            Some(seconds) => print_xmp_exposure_time(seconds),
            None => value.to_string(),
        },

        // Photoshop numeric tags
        "Quality" => format_photoshop_quality(value),

        // PhotoMechanic.pm:113-116 -- ColorClass is an integer printed through
        // %colorClasses; PhotoMechanic.pm:133 -- Tagged is an XMP boolean.
        "ColorClass" => decode_photomechanic_color_class(value),
        "Tagged" => decode_photomechanic_tagged(value),
        // PhotoMechanic.pm:120-127 -- "0:6:5:003344" becomes
        // "Tagged:0, ColorClass:6, Rating:5, FrameNum:003344".
        "Prefs" => format_photomechanic_prefs(value),
        // PhotoMechanic.pm:134-140 applies Exif::ExifTime to this XMP
        // property, accepting the compact IPTC-style time it writes.
        "TimeCreated" => format_photomechanic_time_created(value),

        // XMP.pm:1666-1677 -- crs:PerspectiveUpright names its own numbers.
        "PerspectiveUpright" => decode_perspective_upright(value),

        // XMP.pm:2110-2115 and XMP.pm:2342-2349 -- both are rationals whose
        // PrintConv appends " m". oxidex already carries the unit, so only the
        // rational in front of it still has to be evaluated.
        "GPSAltitude" | "SubjectDistance" => format_xmp_rational_with_unit(value),

        // XMP.pm:3678 -- ExifTool evaluates `numerator/denominator` for every
        // property declared `Writable => 'rational'`, and (via XMPAutoConv) for
        // every property it does not know at all. Applied after the arms above
        // so a tag with its own PrintConv keeps it.
        _ if parse_xmp_rational(value).is_some() => format_xmp_plain_rational(value),

        // XMP date/time properties are ISO 8601 in the file; ExifTool's
        // %dateTimeInfo PrintConv (ConvertDateTime) reports them EXIF-style and
        // keeps the UTC offset.
        _ if is_xmp_date_tag(local_name) => format_xmp_date_time(value),

        // Default: return original value unchanged
        _ => value.to_string(),
    }
}

/// XMP.pm:2275-2290 gives exif:Contrast and exif:Saturation the same
/// PrintConv. Keep it namespace-scoped because Camera Raw reuses both names
/// with unrelated integer ranges.
fn decode_xmp_contrast_or_saturation(value: &str) -> String {
    match value {
        "0" => "Normal",
        "1" => "Low",
        "2" => "High",
        _ => return value.to_string(),
    }
    .to_string()
}

/// Decodes the PLUS Media Matrix IDs used by the pinned `PLUS.xmp` fixture.
///
/// This follows `PLUS.pm`'s `%mediaMatrix` `OTHER` PrintConv: normalize the
/// wire value, describe the version/usage headers, then render each 4-byte
/// Media Matrix ID with its table description when one is known.
fn format_plus_media_summary_code(value: &str) -> String {
    let compact: String = value
        .chars()
        .filter(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || *ch == '|')
        .collect();
    let mut fields = compact.split('|');
    if fields.next() != Some("") || fields.next() != Some("PLUS") {
        return value.to_string();
    }
    let (Some(version), Some(usage_count)) = (fields.next(), fields.next()) else {
        return value.to_string();
    };
    let Some(version_digits) = version.strip_prefix('V') else {
        return value.to_string();
    };
    if version_digits.len() < 3 || !usage_count.starts_with('U') {
        return value.to_string();
    }

    let (major, minor) = version_digits.split_at(version_digits.len() - 2);
    let major = major.trim_start_matches('0');
    let major = if major.is_empty() { "0" } else { major };
    let usage_total = usage_count
        .strip_prefix('U')
        .and_then(|count| count.parse::<u32>().ok());

    let mut formatted = format!("PLUS {version} (LDF Version {major}.{minor}) {usage_count}");
    if let Some(total) = usage_total {
        formatted.push_str(&format!(" ({total} Media Usages:)"));
    }

    let codes: String = fields
        .flat_map(str::chars)
        .filter(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        .collect();
    for bytes in codes.as_bytes().chunks_exact(4) {
        let code = std::str::from_utf8(bytes).expect("ASCII Media Matrix ID");
        if let Some(count) = plus_usage_item_count(code) {
            formatted.push_str(&format!("; {code} ({count} Usage Items:)"));
        } else if let Some(letter) = code.strip_prefix("1UN") {
            formatted.push_str(&format!(" {code} (Usage Number {letter})"));
        } else if let Some(description) = plus_media_matrix_description(code) {
            formatted.push_str(&format!(" {code} ({description})"));
        } else {
            formatted.push_str(&format!(" {code}"));
        }
    }
    formatted
}

fn plus_usage_item_count(code: &str) -> Option<u32> {
    let bytes = code.as_bytes();
    if bytes.len() == 4
        && bytes.starts_with(b"1I")
        && bytes[2].is_ascii_uppercase()
        && bytes[3].is_ascii_uppercase()
    {
        Some(u32::from(bytes[2] - b'A') * 26 + u32::from(bytes[3] - b'A') + 1)
    } else {
        None
    }
}

/// Pinned `PLUS.pm` `%mediaMatrix` entries used by `PLUS.xmp`.
fn plus_media_matrix_description(code: &str) -> Option<&'static str> {
    Some(match code {
        "2BFT" => "Personal Use|Website|Web Page, All Types|All Electronic Distribution Formats",
        "2BOS" => "Advertising|Art|Art Display, All Art Types|Electronic Display",
        "2EMA" => "Advertising|Email|All Email Types|Internet Email",
        "2FET" => "Advertising|Marketing Materials|Promotional E-card|Internet Email",
        "3PRV" => "Multiple Placements on Both Sides",
        "3PSD" => "Multiple Placements on Screen",
        "3PTZ" => "Multiple Placements on Any Pages",
        "4SBG" => "Any Size Image|Up To Full Screen Ad",
        "4SDL" => "Up To Full Screen Image|Any Size Screen",
        "4SKG" => "Any Size Image|Any Size Screen",
        "4SLA" => "Any Size Image|Any Size Pages",
        "5VUP" => "Single Version",
        "6QCH" => "One|Copy",
        "6QCX" => "One|Display",
        "6QUL" => "Any Quantity",
        "7DWM" => "In Perpetuity",
        "8IAD" => "Advertising and Marketing",
        "8IAE" => "Arts and Entertainment",
        "8IAG" => "Agriculture, Farming and Horticulture",
        "8IAR" => "Architecture and Engineering",
        "8IBR" => "Broadcast Media",
        "8IEC" => "Ecology, Environmental and Conservation",
        "8IEN" => "Energy, Utilities and Fuel",
        "8IEV" => "Events and Conventions",
        "8IFO" => "Forestry and Wood Products",
        "8IGL" => "Gardening and Landscaping",
        "8IGR" => "Graphic Design",
        "8IHH" => "Hotels and Hospitality",
        "8IIM" => "Industry and Manufacturing",
        "8INP" => "Not For Profit, Social, Charitable",
        "8IPO" => "Personal Use Only",
        "8IPM" => "Publishing Media",
        "8IPR" => "Public Relations",
        "8ISM" => "Retail Sales and Marketing",
        "8ITR" => "Travel and Tourism",
        "8LEN" => "English",
        "8RAU" => "Oceania|Australia",
        "9EXC" => "All Exclusive",
        _ => return None,
    })
}

/// XMP.pm:2011-2024 `%Image::ExifTool::XMP::exif` PrintConv.
fn decode_xmp_components_configuration(value: &str) -> String {
    match value {
        "0" => "-".to_string(),
        "1" => "Y".to_string(),
        "2" => "Cb".to_string(),
        "3" => "Cr".to_string(),
        "4" => "R".to_string(),
        "5" => "G".to_string(),
        "6" => "B".to_string(),
        _ => value.to_string(),
    }
}

/// Evaluates the rational in a value that already carries a unit, e.g.
/// `"40/1 m"` -> `"40 m"`.
///
/// Deliberately NOT a general "rational followed by anything" rule: XMP also
/// stores space-separated rational LISTS -- `aux:LensInfo` is
/// `"18/1 55/1 0/0 0/0"` -- and evaluating just the first element of one
/// produces a value ExifTool never prints.
fn format_xmp_rational_with_unit(value: &str) -> String {
    let value = value.trim();
    let Some((number, unit)) = value.split_once(' ') else {
        return format_xmp_plain_rational(value);
    };
    if unit.is_empty() || !unit.chars().all(|c| c.is_ascii_alphabetic()) {
        return value.to_string();
    }
    match parse_xmp_rational(number) {
        Some(_) => format!("{} {}", format_xmp_plain_rational(number), unit),
        None => value.to_string(),
    }
}

/// `XMP.pm`'s `ConvertRational` (XMP.pm:3400-3412), whose regex is
/// `^(-?\d+)/(-?\d+)$`: this is the test for "ExifTool would evaluate this",
/// so unlike [`parse_xmp_number`] a bare decimal is *not* a rational.
///
/// Returns `None` for a zero denominator, which ExifTool reports as `inf` or
/// `undef` rather than a quotient.
fn parse_xmp_rational(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.trim().split_once('/')?;
    let numerator: i64 = numerator.parse().ok()?;
    let denominator: i64 = denominator.parse().ok()?;
    if denominator == 0 {
        return None;
    }
    Some(numerator as f64 / denominator as f64)
}

/// `Image::ExifTool::Exif::PrintFNumber` (`Exif.pm:5715-5723`), verbatim:
///
/// ```text
/// if (Image::ExifTool::IsFloat($val) and $val > 0) {
///     # round to 1 decimal place, or 2 for values < 1.0
///     $val = sprintf(($val<1 ? "%.2f" : "%.1f"), $val);
/// }
/// ```
///
/// Note there is no trailing-zero trimming: ExifTool prints f/8 as `8.0`.
fn format_xmp_fnumber(value: &str) -> String {
    let Some(number) = parse_xmp_number(value) else {
        return value.to_string();
    };
    if number <= 0.0 {
        return value.to_string();
    }
    if number < 1.0 {
        format!("{number:.2}")
    } else {
        format!("{number:.1}")
    }
}

/// `PhotoMechanic.pm:23-33` (`%colorClasses`), verbatim:
///
/// ```text
/// 0 => '0 (None)',        1 => '1 (Winner)',      2 => '2 (Winner alt)',
/// 3 => '3 (Superior)',    4 => '4 (Superior alt)',5 => '5 (Typical)',
/// 6 => '6 (Typical alt)', 7 => '7 (Extras)',      8 => '8 (Trash)',
/// ```
fn decode_photomechanic_color_class(value: &str) -> String {
    match value.trim() {
        "0" => "0 (None)",
        "1" => "1 (Winner)",
        "2" => "2 (Winner alt)",
        "3" => "3 (Superior)",
        "4" => "4 (Superior alt)",
        "5" => "5 (Typical)",
        "6" => "6 (Typical alt)",
        "7" => "7 (Extras)",
        "8" => "8 (Trash)",
        _ => return value.to_string(),
    }
    .to_string()
}

/// `PhotoMechanic.pm:133` -- `Tagged => { Writable => 'boolean',
/// PrintConv => { False => 'No', True => 'Yes' } }`.
///
/// The comparison is case-insensitive only because oxidex lower-cases XMP
/// booleans upstream; the file itself spells them `True`/`False`, which is what
/// ExifTool matches. No other spelling is converted -- ExifTool has no entry
/// for `0`/`1` and would report `Unknown (0)`.
fn decode_photomechanic_tagged(value: &str) -> String {
    let value = value.trim();
    if value.eq_ignore_ascii_case("false") {
        "No".to_string()
    } else if value.eq_ignore_ascii_case("true") {
        "Yes".to_string()
    } else {
        value.to_string()
    }
}

/// `PhotoMechanic.pm:120-127` -- the Prefs PrintConv, whose substitution is
/// `s[\s*(\d+):\s*(\d+):\s*(\d+):\s*(\S*)][Tagged:$1, ColorClass:$2, Rating:$3, FrameNum:$4]`.
fn format_photomechanic_prefs(value: &str) -> String {
    let fields: Vec<&str> = value.trim().split(':').map(str::trim).collect();
    let [tagged, color_class, rating, frame_num] = fields.as_slice() else {
        return value.to_string();
    };
    // The first three captures are `\d+`, so each must be at least one digit.
    let is_number = |field: &str| !field.is_empty() && field.bytes().all(|b| b.is_ascii_digit());
    if !is_number(tagged) || !is_number(color_class) || !is_number(rating) {
        return value.to_string();
    }
    format!("Tagged:{tagged}, ColorClass:{color_class}, Rating:{rating}, FrameNum:{frame_num}")
}

/// PhotoMechanic.pm:138 uses `Exif::ExifTime` for `TimeCreated`.
///
/// Exif.pm:6085-6094 inserts separators into compact `HHMMSS` values and
/// `+HHMM`/`-HHMM` offsets, while leaving every other spelling unchanged.
fn format_photomechanic_time_created(value: &str) -> String {
    // Exif.pm performs this replacement before testing for the compact form.
    let value = value.trim_end_matches('\0').replace(' ', ":");
    let bytes = value.as_bytes();
    let mut formatted = if bytes.len() >= 6 && bytes[..6].iter().all(u8::is_ascii_digit) {
        format!(
            "{}:{}:{}{}",
            &value[..2],
            &value[2..4],
            &value[4..6],
            &value[6..]
        )
    } else {
        value
    };

    // This runs after the time has been separated, so it applies equally to
    // the compact form and `10:30:55+0500` after ExifTime replaces spaces.
    if formatted.len() >= 5 {
        let timezone_start = formatted.len() - 5;
        let timezone = &formatted[timezone_start..];
        if matches!(timezone.as_bytes()[0], b'+' | b'-')
            && timezone.as_bytes()[1..].iter().all(u8::is_ascii_digit)
        {
            formatted = format!(
                "{}{}:{}",
                &formatted[..timezone_start],
                &timezone[..3],
                &timezone[3..]
            );
        }
    }
    formatted
}

/// `XMP.pm:1666-1677` -- the crs:PerspectiveUpright PrintConv, verbatim:
///
/// ```text
/// 0 => 'Off', 1 => 'Auto', 2 => 'Full',
/// 3 => 'Level', 4 => 'Vertical', 5 => 'Guided',
/// ```
fn decode_perspective_upright(value: &str) -> String {
    match value.trim() {
        "0" => "Off",
        "1" => "Auto",
        "2" => "Full",
        "3" => "Level",
        "4" => "Vertical",
        "5" => "Guided",
        _ => return value.to_string(),
    }
    .to_string()
}

/// XMP properties ExifTool declares with `%dateTimeInfo` (XMP.pm:236-243),
/// whose PrintConv is `ConvertDateTime`.
///
/// Each name is a real ExifTool XMP tag: CreateDate (XMP.pm:1053),
/// CreationDate (XMP.pm:1226), DateAcquired (Microsoft.pm:241), DateCreated
/// (XMP.pm:1296), DateTime (XMP.pm:1966), DateTimeDigitized (XMP.pm:2037),
/// DateTimeOriginal (XMP.pm:2032), GPSDateTime (XMP.pm:2351), MetadataDate
/// (XMP.pm:1057), ModDate (XMP.pm:1225), ModificationDate (XMP2.pl:899),
/// ModifyDate (XMP.pm:1058) and the flattened HistoryWhen (XMP.pm:341).
fn is_xmp_date_tag(local_name: &str) -> bool {
    matches!(
        local_name,
        "CreateDate"
            | "CreationDate"
            | "DateAcquired"
            | "DateCreated"
            | "DateTime"
            | "DateTimeDigitized"
            | "DateTimeOriginal"
            | "GPSDateTime"
            | "MetadataDate"
            | "ModDate"
            | "ModificationDate"
            | "ModifyDate"
            // XMP-drone-dji:UTCAtExposure has no 13.59 tag definition at all;
            // ExifTool still prints it as a datetime because XMPAutoConv
            // (XMP.pm:3676-3682) runs ConvertXMPDate over every UNKNOWN
            // property whose value matches the full-timestamp shape --
            // DJI_M3T.jpg prints "2022:10:27 05:08:32.100476". oxidex has no
            // is-known-to-ExifTool index to reproduce that trigger exactly
            // (a known non-date tag like dc:Description must NOT convert), so
            // the oracle-verified carrier is named here instead. A future
            // unknown timestamp-shaped property will surface as an honest
            // value difference rather than being converted on a guess.
            | "UTCAtExposure"
    ) || local_name.starts_with("HistoryWhen")
}

/// Rewrites an ISO 8601 XMP timestamp the way ExifTool prints one:
/// `2021-10-01T14:18:11.534+01:00` -> `2021:10:01 14:18:11.534+01:00`.
///
/// Only the date's own separators change. The UTC offset (or trailing `Z`) is
/// part of the value ExifTool reports and must survive.
fn format_xmp_date_time(value: &str) -> String {
    let value = value.trim();
    let (date, rest) = match value.split_once('T') {
        Some((date, rest)) => (date, Some(rest)),
        None => (value, None),
    };
    // A date is YYYY-MM-DD, YYYY-MM or YYYY; anything else is not a timestamp.
    let year = date.split('-').next().unwrap_or("");
    if year.len() != 4 || !year.bytes().all(|b| b.is_ascii_digit()) {
        return value.to_string();
    }
    let converted_date = date.replace('-', ":");
    match rest {
        Some(rest) => format!("{converted_date} {rest}"),
        None => converted_date,
    }
}

/// Decode the `exif:Mode` field of the XMP `exif:Flash` structure.
///
/// Verbatim from the `Flash` structure in `XMP.pm` (`Mode` field PrintConv):
///
/// ```text
/// 0 => 'Unknown',
/// 1 => 'On',
/// 2 => 'Off',
/// 3 => 'Auto',
/// ```
/// Decodes an XMP property that shares an EXIF tag's PrintConv, keeping the
/// original text when the value is not one of the numbers that table names.
fn decode_via_tiff_enum(tag_id: u16, value: &str) -> String {
    value
        .trim()
        .parse::<i64>()
        .ok()
        .and_then(|n| crate::parsers::tiff::tiff_enums::tiff_enum_to_string(tag_id, n))
        .unwrap_or_else(|| value.to_string())
}

fn decode_xmp_flash_mode(value: &str) -> String {
    match value.trim() {
        "0" => "Unknown".to_string(),
        "1" => "On".to_string(),
        "2" => "Off".to_string(),
        "3" => "Auto".to_string(),
        _ => value.to_string(),
    }
}

/// Decode the `exif:Return` field of the XMP `exif:Flash` structure.
///
/// Verbatim from the `Flash` structure in `XMP.pm` (`Return` field PrintConv);
/// note that 1 is deliberately absent there, so it falls through unchanged:
///
/// ```text
/// 0 => 'No return detection',
/// 2 => 'Return not detected',
/// 3 => 'Return detected',
/// ```
fn decode_xmp_flash_return(value: &str) -> String {
    match value.trim() {
        "0" => "No return detection".to_string(),
        "2" => "Return not detected".to_string(),
        "3" => "Return detected".to_string(),
        _ => value.to_string(),
    }
}

/// Decode a `Writable => 'boolean'` XMP field through ExifTool's `%boolConv`
/// (`XMP.pm:246-257`). The hash names `True`/`False` exactly and its `OTHER`
/// fallback lower-cases anything else before comparing, so the file's spelling
/// never reaches the output:
///
/// ```text
/// PrintConv => {
///     OTHER => sub { # (inverse conversion is the same)
///         my $val = shift;
///         return 'False' if lc $val eq 'false';
///         return 'True' if lc $val eq 'true';
///         return $val;
///     },
///     True => 'True',
///     False => 'False',
/// },
/// ```
///
/// Anything that is not a boolean spelling passes through untouched -- ExifTool
/// has no entry for `0`/`1` and returns `$val` unchanged for them.
fn decode_xmp_boolean(value: &str) -> String {
    if value.eq_ignore_ascii_case("true") {
        "True".to_string()
    } else if value.eq_ignore_ascii_case("false") {
        "False".to_string()
    } else {
        value.to_string()
    }
}

/// Decode XMP ColorSpace (1 = sRGB, 65535 = Uncalibrated)
fn decode_xmp_color_space(value: &str) -> String {
    match value.trim() {
        "1" => "sRGB".to_string(),
        "2" => "Adobe RGB".to_string(),
        "65535" => "Uncalibrated".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP CustomRendered (0 = Normal, 1 = Custom, etc.)
fn decode_xmp_custom_rendered(value: &str) -> String {
    match value.trim() {
        "0" => "Normal".to_string(),
        "1" => "Custom".to_string(),
        "2" => "HDR (no original saved)".to_string(),
        "3" => "HDR (original saved)".to_string(),
        "4" => "Original (for HDR)".to_string(),
        "6" => "Panorama".to_string(),
        "7" => "Portrait HDR".to_string(),
        "8" => "Portrait".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP ExposureMode (0 = Auto, 1 = Manual, 2 = Auto bracket)
fn decode_xmp_exposure_mode(value: &str) -> String {
    match value.trim() {
        "0" => "Auto".to_string(),
        "1" => "Manual".to_string(),
        "2" => "Auto bracket".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP FileSource (3 = Digital Camera)
fn decode_xmp_file_source(value: &str) -> String {
    match value.trim() {
        "1" => "Film Scanner".to_string(),
        "2" => "Reflection Print Scanner".to_string(),
        "3" => "Digital Camera".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP ResolutionUnit (2 = inches, 3 = centimeters)
fn decode_xmp_resolution_unit(value: &str) -> String {
    match value.trim() {
        "2" => "inches".to_string(),
        "3" => "cm".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP MeteringMode
fn decode_xmp_metering_mode(value: &str) -> String {
    match value.trim() {
        "0" => "Unknown".to_string(),
        "1" => "Average".to_string(),
        "2" => "Center-weighted average".to_string(),
        "3" => "Spot".to_string(),
        "4" => "Multi-spot".to_string(),
        "5" => "Multi-segment".to_string(),
        "6" => "Partial".to_string(),
        "255" => "Other".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP Orientation
fn decode_xmp_orientation(value: &str) -> String {
    match value.trim() {
        "1" => "Horizontal (normal)".to_string(),
        "2" => "Mirror horizontal".to_string(),
        "3" => "Rotate 180".to_string(),
        "4" => "Mirror vertical".to_string(),
        "5" => "Mirror horizontal and rotate 270 CW".to_string(),
        "6" => "Rotate 90 CW".to_string(),
        "7" => "Mirror horizontal and rotate 90 CW".to_string(),
        "8" => "Rotate 270 CW".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP SceneCaptureType
fn decode_xmp_scene_capture_type(value: &str) -> String {
    match value.trim() {
        "0" => "Standard".to_string(),
        "1" => "Landscape".to_string(),
        "2" => "Portrait".to_string(),
        "3" => "Night".to_string(),
        "4" => "Other".to_string(),
        _ => value.to_string(),
    }
}

/// Decode `XMP-exif:SensingMethod` (`exif:SensingMethod`).
///
/// This is **not** the EXIF 0xa217 table. `%Image::ExifTool::Exif::Main`
/// (Exif.pm:2797) names 1 `Not defined` and has no 6 at all; the XMP schema
/// (XMP.pm:2189, inside the `GROUPS => { 1 => 'XMP-exif' }` table that opens at
/// XMP.pm:1990) carries its own hash, quoted verbatim:
///
/// ```text
///     Notes => 'values 1 and 6 are not standard EXIF',
///     PrintConv => {
///         1 => 'Monochrome area', # (not standard EXIF)
///         2 => 'One-chip color area',
///         3 => 'Two-chip color area',
///         4 => 'Three-chip color area',
///         5 => 'Color sequential area',
///         6 => 'Monochrome linear', # (not standard EXIF)
///         7 => 'Trilinear',
///         8 => 'Color sequential linear',
///     },
/// ```
///
/// Reusing the EXIF labels here printed `Not defined` for the one value the two
/// tables actively disagree about, and left 6 as a bare `6`.
///
/// A code the hash does not name falls through ExifTool's generic PrintConv
/// miss (ExifTool.pm:3633, `$value = "Unknown ($val)"`); there is no `PrintHex`
/// on this tag, so the number stays decimal. Confirmed against pinned 13.59:
/// `0`, `9`, `15` and the non-numeric `foo` all print `Unknown (<val>)`.
fn decode_xmp_sensing_method(value: &str) -> String {
    let code = value.trim();
    match code {
        "1" => "Monochrome area".to_string(),
        "2" => "One-chip color area".to_string(),
        "3" => "Two-chip color area".to_string(),
        "4" => "Three-chip color area".to_string(),
        "5" => "Color sequential area".to_string(),
        "6" => "Monochrome linear".to_string(),
        "7" => "Trilinear".to_string(),
        "8" => "Color sequential linear".to_string(),
        _ => format!("Unknown ({code})"),
    }
}

/// Decode XMP WhiteBalance (0 = Auto, 1 = Manual)
fn decode_xmp_white_balance(value: &str) -> String {
    match value.trim() {
        "0" => "Auto".to_string(),
        "1" => "Manual".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP YCbCrPositioning (1 = Centered, 2 = Co-sited)
fn decode_xmp_ycbcr_positioning(value: &str) -> String {
    match value.trim() {
        "1" => "Centered".to_string(),
        "2" => "Co-sited".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP ColorMode (Photoshop color mode)
fn decode_xmp_color_mode(value: &str) -> String {
    match value.trim() {
        "0" => "Bitmap".to_string(),
        "1" => "Grayscale".to_string(),
        "2" => "Indexed".to_string(),
        "3" => "RGB".to_string(),
        "4" => "CMYK".to_string(),
        "7" => "Multichannel".to_string(),
        "8" => "Duotone".to_string(),
        "9" => "Lab".to_string(),
        _ => value.to_string(),
    }
}

/// Decode XMP PhotometricInterpretation
fn decode_xmp_photometric_interpretation(value: &str) -> String {
    match value.trim() {
        "0" => "WhiteIsZero".to_string(),
        "1" => "BlackIsZero".to_string(),
        "2" => "RGB".to_string(),
        "3" => "RGB Palette".to_string(),
        "4" => "Transparency Mask".to_string(),
        "5" => "CMYK".to_string(),
        "6" => "YCbCr".to_string(),
        "8" => "CIE Lab".to_string(),
        "9" => "ICC Lab".to_string(),
        "10" => "ITU Lab".to_string(),
        "32803" => "Color Filter Array".to_string(),
        "32844" => "Pixar Log L".to_string(),
        "32845" => "Pixar Log Luv".to_string(),
        "34892" => "Linear Raw".to_string(),
        _ => value.to_string(),
    }
}

// =============================================================================
// NAMESPACE-SPECIFIC FORMATTERS (47 new tags across 6+ namespaces)
// =============================================================================

/// Formats Camera Raw Settings processing parameters.
///
/// Camera Raw Settings namespace (crs:) stores numeric processing parameters.
/// These values represent exposure, contrast, highlights, shadows, etc.
///
/// # Supported tags:
/// - CameraRawInfo: Camera model and version information
/// - ProcessingParameters: Numeric exposure/contrast/saturation values
fn format_camera_raw_parameters(value: &str) -> String {
    // Camera Raw parameters are typically numeric values
    // Try to parse and validate as decimal number
    if let Ok(_) = value.trim().parse::<f64>() {
        // Keep numeric values as-is, they're already formatted
        value.to_string()
    } else {
        // Non-numeric values pass through
        value.to_string()
    }
}

/// Formats TIFF resolution values.
///
/// TIFF namespace (tiff:) stores resolution as numeric values.
/// These represent pixels per unit (typically inches or cm).
///
/// # Supported tags:
/// - XResolution: Horizontal resolution
/// - YResolution: Vertical resolution
/// - ResolutionUnit: Unit (2 = inches, 3 = centimeters)
fn format_tiff_resolution(value: &str) -> String {
    // XMP.pm:1950-1951 -- tiff:XResolution and tiff:YResolution are declared
    // `Writable => 'rational'`, so the stored "1800000/10000" has to be
    // evaluated before it is printed, not just decimals.
    if let Some(num) = parse_xmp_number(value) {
        // Format with up to 6 decimal places, removing trailing zeros
        let formatted = format!("{:.6}", num);
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    } else {
        // Non-numeric values pass through unchanged
        value.to_string()
    }
}

/// Formats EXIF ISO value.
///
/// EXIF ISO sensitivity is typically a numeric value representing
/// light sensitivity (e.g., 100, 400, 3200).
///
/// # Supported tags:
/// - ISO: Light sensitivity value
/// - PhotographicSensitivity: Alternative ISO tag name
fn format_exif_iso(value: &str) -> String {
    // ISO values are plain numeric, just validate and pass through
    let trimmed = value.trim();
    if trimmed.parse::<u32>().is_ok() || trimmed.parse::<f64>().is_ok() {
        trimmed.to_string()
    } else {
        value.to_string()
    }
}

/// Formats EXIF shutter speed value.
///
/// Shutter speed in EXIF is stored as a fraction or APEX value
/// (e.g., "1/250", "125", "0.004" seconds).
///
/// # Supported tags:
/// - ShutterSpeed: Exposure time
/// - ExposureTime: Alternative name
fn format_exif_shutter_speed(value: &str) -> String {
    let trimmed = value.trim();

    // Check for fraction format (e.g., "1/250")
    if trimmed.contains('/') {
        // Keep fraction format as-is
        trimmed.to_string()
    } else if let Ok(num) = trimmed.parse::<f64>() {
        // Format as decimal with 3 decimal places
        format!("{:.3}", num)
    } else {
        trimmed.to_string()
    }
}

/// Formats EXIF aperture value.
///
/// Aperture (f-number) in EXIF is typically stored as decimal (e.g., 2.8, 5.6).
///
/// # Supported tags:
/// - Aperture: f-number value
/// - ApertureValue: APEX encoded value
/// - FNumber: Alternative aperture tag
fn format_exif_aperture(value: &str) -> String {
    let trimmed = value.trim();

    if let Ok(num) = trimmed.parse::<f64>() {
        // Format f-number with appropriate precision
        if (num - num.round()).abs() < 0.01 {
            // Whole number f-stops
            format!("f/{:.0}", num)
        } else {
            // Fractional f-stops (2.8, 5.6, etc.)
            format!("f/{:.1}", num)
        }
    } else {
        trimmed.to_string()
    }
}

/// Formats EXIF exposure compensation value.
///
/// Exposure compensation is stored as a signed fraction or decimal
/// representing EV offset (e.g., "+1.0", "-0.5").
///
/// # Supported tags:
/// - ExposureCompensation: EV offset value
/// - BrightnessValue: Alternative brightness tag
fn format_exif_exposure_compensation(value: &str) -> String {
    match parse_xmp_number(value) {
        // XMP.pm:2099 -- `PrintConv => Image::ExifTool::Exif::PrintFraction`.
        // The old `{:.2}` here printed XMP.xmp's exif:ExposureBiasValue of
        // "-3/3" as "-1.00" (and, before that, could not parse the rational at
        // all); ExifTool prints "-1". A later local reimplementation of
        // PrintFraction fixed that but kept a `{:+.3}` (three *decimal
        // places*) fallback for values that aren't a whole/half/third stop,
        // where ExifTool's `sprintf("%+.3g", $val)` is three *significant
        // digits* with insignificant trailing zeros stripped -- so
        // NikonCoolpixP520.jpg's -3/10 EV printed "-0.300" instead of "-0.3".
        // `print_fraction` is the shared, ExifTool-Perl-swept port (see its
        // doc comment for the 1,538-input comparison); every other call site
        // in this crate already uses it.
        Some(number) => print_fraction(number),
        None => value.trim().to_string(),
    }
}

/// Parses an XMP numeric value, which the schema may store as a `rational`.
///
/// `XMP.pm` marks most EXIF-derived properties `Writable => 'rational'`, and
/// real files write them that way: XMP.xmp carries `exif:FocalLength` as
/// "5800/1000" and `exif:FocalPlaneXResolution` as "2272000/224". The numeric
/// formatters used to call `str::parse::<f64>` directly, which fails on those,
/// so every one of them passed the raw fraction through untouched.
fn parse_xmp_number(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if let Some((numerator, denominator)) = trimmed.split_once('/') {
        let numerator: f64 = numerator.trim().parse().ok()?;
        let denominator: f64 = denominator.trim().parse().ok()?;
        if denominator == 0.0 {
            return None;
        }
        return Some(numerator / denominator);
    }
    trimmed.parse().ok()
}

/// APEX aperture: `sqrt(2) ** $val`, printed with one decimal.
fn format_xmp_apex_aperture(value: &str) -> String {
    match parse_xmp_number(value) {
        Some(apex) => format!("{:.1}", std::f64::consts::SQRT_2.powf(apex)),
        None => value.trim().to_string(),
    }
}

/// APEX shutter speed: `1/(2**$val)`, printed by `PrintExposureTime`.
fn format_xmp_apex_shutter_speed(value: &str) -> String {
    let Some(apex) = parse_xmp_number(value) else {
        return value.trim().to_string();
    };
    // XMP.pm:2083 -- values of 100 or more collapse to zero rather than
    // underflowing.
    let seconds = if apex.abs() < 100.0 {
        1.0 / 2f64.powf(apex)
    } else {
        0.0
    };
    print_xmp_exposure_time(seconds)
}

/// `Image::ExifTool::Exif::PrintExposureTime` (`Exif.pm:5701`).
///
/// ```text
/// sub PrintExposureTime($)
/// {
///     my $secs = shift;
///     return $secs unless Image::ExifTool::IsFloat($secs);
///     if ($secs < 0.25001 and $secs > 0) {
///         return sprintf("1/%d",int(0.5 + 1/$secs));
///     }
///     $_ = sprintf("%.1f",$secs);
///     s/\.0$//;
///     return $_;
/// }
/// ```
///
/// Delegates to the shared port instead of keeping a second copy. The copy
/// that used to live here tested `seconds == seconds.trunc()` in place of
/// Perl's `s/\.0$//`, which is a strictly narrower condition: Perl drops the
/// decimal whenever `sprintf("%.1f")` *rounds* to a whole number, not only
/// when the value already is one. A 2.00694 s exposure therefore printed
/// "2.0" here against ExifTool's "2". The `as i64` cast in that branch also
/// saturated for exposures past `i64::MAX`, turning 1e20 s into
/// "9223372036854775807" where ExifTool prints "100000000000000000000".
fn print_xmp_exposure_time(seconds: f64) -> String {
    crate::core::formatters::print_exposure_time(seconds)
}

/// A rational with no PrintConv, printed the way Perl prints a number.
///
/// Perl's default stringification is 15 significant digits with trailing zeros
/// removed, which is why ExifTool reports 2272000/224 as "10142.8571428571".
fn format_xmp_plain_rational(value: &str) -> String {
    let Some(number) = parse_xmp_number(value) else {
        return value.trim().to_string();
    };
    let formatted = format!("{:.*e}", 14, number);
    // Round-trip through the 15-significant-digit form, then render plainly.
    let rounded: f64 = formatted.parse().unwrap_or(number);
    let mut text = format!("{}", rounded);
    if let Some(dot) = text.find('.') {
        // Perl keeps at most 15 significant digits.
        let significant = text[..dot].trim_start_matches('-').len();
        let keep = 15usize.saturating_sub(significant);
        if text.len() - dot - 1 > keep {
            text = format!("{:.*}", keep, rounded);
            while text.ends_with('0') {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
        }
    }
    text
}

/// Formats EXIF focal length value.
///
/// Focal length in EXIF is stored as a decimal number in millimeters
/// (e.g., 50.0, 24.0).
///
/// # Supported tags:
/// - FocalLength: Lens focal length in mm
/// - FocalLengthIn35mmFilm: Equivalent focal length
fn format_exif_focal_length(value: &str) -> String {
    // XMP.pm:2161-2165 -- `Writable => 'rational'`, `PrintConv =>
    // 'sprintf("%.1f mm",$val)'`. One decimal always, including whole
    // millimetres, and the stored form is a fraction: XMP.xmp writes
    // "5800/1000", which the old plain-float parse could not read at all, so
    // the tag was reported as "5800/1000 mm".
    match parse_xmp_number(value) {
        Some(number) => format!("{:.1} mm", number),
        None => value.trim().to_string(),
    }
}

/// Formats Photoshop quality/compression value.
///
/// Photoshop namespace stores quality as a percentage (0-100).
///
/// # Supported tags:
/// - Quality: JPEG quality percentage
/// - CompressionLevel: Compression level
fn format_photoshop_quality(value: &str) -> String {
    let trimmed = value.trim();

    if let Ok(num) = trimmed.parse::<u32>() {
        if num <= 100 {
            format!("{}%", num)
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebound_custom_prefix_keeps_the_first_generic_xmp_tag() {
        // ExifTool's XMP6.xmp fixture binds `xxxx` first to a fish schema,
        // then to a feline schema. `exiftool -G1 -s XMP6.xmp` emits only
        // `XMP-xxxx:Test : trout`.
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description xmlns:xxxx="http://testtag.com/fish/1.0/">
                <xxxx:Test>trout</xxxx:Test>
              </rdf:Description>
              <rdf:Description xmlns:xxxx="http://testtag.com/feline/1.0/">
                <xxxx:Test>tabby</xxxx:Test>
              </rdf:Description>
            </rdf:RDF>
        "#;

        assert_eq!(
            parse_xmp(xml).unwrap(),
            vec![("XMP:Test".to_string(), "trout".to_string())]
        );
    }

    #[test]
    fn plus_custom1_language_lists_keep_the_plus_family_without_aliases() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description xmlns:plus="http://ns.useplus.org/ldf/xmp/1.0/">
                <plus:Custom1>
                  <rdf:Bag>
                    <rdf:li><rdf:Alt>
                      <rdf:li xml:lang="x-default">cu1</rdf:li>
                      <rdf:li xml:lang="de">de1</rdf:li>
                      <rdf:li xml:lang="fr">fr1</rdf:li>
                    </rdf:Alt></rdf:li>
                    <rdf:li><rdf:Alt>
                      <rdf:li xml:lang="x-default">cu2</rdf:li>
                      <rdf:li xml:lang="fr"/>
                    </rdf:Alt></rdf:li>
                    <rdf:li><rdf:Alt>
                      <rdf:li xml:lang="x-default">cu3</rdf:li>
                      <rdf:li xml:lang="de">de3</rdf:li>
                      <rdf:li xml:lang="fr">fr3</rdf:li>
                    </rdf:Alt></rdf:li>
                  </rdf:Bag>
                </plus:Custom1>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = parse_xmp(xml).unwrap();
        let value = |name: &str| {
            tags.iter()
                .find(|(tag, _)| tag == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(value("XMP-plus:Custom1"), Some("cu1, cu2, cu3"), "{tags:?}");
        assert_eq!(value("XMP-plus:Custom1-de"), Some("de1, de3"));
        assert_eq!(value("XMP-plus:Custom1-fr"), Some("fr1, , fr3"));
        assert!(!tags.iter().any(|(name, _)| name.starts_with("XMP:Custom1")));

        // PLUS.pm defines Custom1 as a Bag. `exiftool -json XMP9.xmp`
        // therefore preserves each language's entries as an array, including
        // the intentionally empty French middle entry.
        let typed = parse_xmp_typed(xml).unwrap();
        let typed_value = |name: &str| {
            typed
                .iter()
                .find(|(tag, _)| tag == name)
                .map(|(_, value)| value)
        };
        assert_eq!(
            typed_value("XMP-plus:Custom1"),
            Some(&XmpValue::List(vec![
                "cu1".to_string(),
                "cu2".to_string(),
                "cu3".to_string(),
            ]))
        );
        assert_eq!(
            typed_value("XMP-plus:Custom1-de"),
            Some(&XmpValue::List(vec!["de1".to_string(), "de3".to_string()]))
        );
        assert_eq!(
            typed_value("XMP-plus:Custom1-fr"),
            Some(&XmpValue::List(vec![
                "fr1".to_string(),
                String::new(),
                "fr3".to_string(),
            ]))
        );
    }

    #[test]
    fn plus_media_summary_code_uses_the_media_matrix_print_conversion() {
        // Pinned PLUS.pm `%mediaMatrix` PrintConv, exercised by PLUS.xmp:
        // version/count headers, usage metadata, table descriptions and an
        // unknown ID that must remain visible all follow its OTHER callback.
        assert_eq!(
            format_xmp_value(
                "XMP-plus:MediaSummaryCode",
                "|PLUS|V0121|U001|1IAA1UNA2EMA3PTZ4SBG5VUP6QUL7DWM8RAU8IAD8LEN9EXC|",
            ),
            "PLUS V0121 (LDF Version 1.21) U001 (1 Media Usages:); 1IAA (1 Usage Items:) 1UNA (Usage Number A) 2EMA (Advertising|Email|All Email Types|Internet Email) 3PTZ (Multiple Placements on Any Pages) 4SBG (Any Size Image|Up To Full Screen Ad) 5VUP (Single Version) 6QUL (Any Quantity) 7DWM (In Perpetuity) 8RAU (Oceania|Australia) 8IAD (Advertising and Marketing) 8LEN (English) 9EXC (All Exclusive)"
        );
        assert_eq!(
            format_xmp_value("XMP-plus:MediaSummaryCode", "|PLUS|V0100|U001|1IAAZZZZ|"),
            "PLUS V0100 (LDF Version 1.00) U001 (1 Media Usages:); 1IAA (1 Usage Items:) ZZZZ"
        );
    }

    #[test]
    fn sharpness_print_conversion_is_scoped_to_xmp_exif() {
        assert_eq!(format_xmp_value("XMP-exif:Sharpness", "0"), "Normal");
        assert_eq!(format_xmp_value("XMP-exif:Sharpness", "1"), "Soft");
        assert_eq!(format_xmp_value("XMP-exif:Sharpness", "2"), "Hard");
        assert_eq!(format_xmp_value("XMP:Sharpness", "0"), "0");
        assert_eq!(format_xmp_value("XMP:Sharpness", "25"), "25");
    }

    /// `XMP-exif:SensingMethod` used the EXIF 0xa217 PrintConv, which disagrees
    /// with the XMP schema's own hash on exactly the two codes XMP.pm:2192
    /// flags as "not standard EXIF". Ground truth, from the pinned 13.59 oracle
    /// over a JPEG written with `exiftool "-XMP-exif:SensingMethod#=<n>"`:
    ///
    /// ```text
    /// $ exiftool -G1 -s sm.jpg
    /// [XMP-exif]      SensingMethod                   : Monochrome area      # 1
    /// [XMP-exif]      SensingMethod                   : Monochrome linear    # 6
    /// [XMP-exif]      SensingMethod                   : Unknown (0)          # 0
    /// [XMP-exif]      SensingMethod                   : Unknown (9)          # 9
    /// ```
    ///
    /// oxidex previously printed `Not defined` for 1 and a bare `6` for 6.
    #[test]
    fn xmp_exif_sensing_method_uses_the_xmp_schema_print_conversion() {
        // XMP.pm:2194 -- the EXIF table (Exif.pm:2801) says "Not defined" here.
        assert_eq!(
            format_xmp_value("XMP-exif:SensingMethod", "1"),
            "Monochrome area"
        );
        // XMP.pm:2199 -- absent entirely from the EXIF table.
        assert_eq!(
            format_xmp_value("XMP-exif:SensingMethod", "6"),
            "Monochrome linear"
        );
        // The six codes both tables agree on.
        for (code, label) in [
            ("2", "One-chip color area"),
            ("3", "Two-chip color area"),
            ("4", "Three-chip color area"),
            ("5", "Color sequential area"),
            ("7", "Trilinear"),
            ("8", "Color sequential linear"),
        ] {
            assert_eq!(format_xmp_value("XMP-exif:SensingMethod", code), label);
        }
        // Unnamed codes take ExifTool.pm:3633's generic PrintConv miss, in
        // decimal (no PrintHex on this tag).
        assert_eq!(
            format_xmp_value("XMP-exif:SensingMethod", "0"),
            "Unknown (0)"
        );
        assert_eq!(
            format_xmp_value("XMP-exif:SensingMethod", "9"),
            "Unknown (9)"
        );
        // XMP-exif is the only XMP schema declaring a SensingMethod in 13.59,
        // so neither the labels nor the Unknown() wrapper may leak elsewhere.
        assert_eq!(format_xmp_value("XMP:SensingMethod", "1"), "1");
        assert_eq!(format_xmp_value("XMP-crs:SensingMethod", "0"), "0");
    }

    /// End-to-end through the RDF parser, matching the packet ExifTool writes
    /// for `-XMP-exif:SensingMethod#=1`.
    #[test]
    fn xmp_exif_sensing_method_decodes_through_the_rdf_parser() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description xmlns:exif="http://ns.adobe.com/exif/1.0/"
                exif:SensingMethod="1"/>
            </rdf:RDF>
        "#;

        assert_eq!(
            parse_xmp(xml).unwrap(),
            vec![(
                "XMP-exif:SensingMethod".to_string(),
                "Monochrome area".to_string(),
            )]
        );
    }

    #[test]
    fn xmp_exif_components_configuration_names_each_sequence_component() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description xmlns:exif="http://ns.adobe.com/exif/1.0/">
                <exif:ComponentsConfiguration>
                  <rdf:Seq>
                    <rdf:li>1</rdf:li>
                    <rdf:li>2</rdf:li>
                    <rdf:li>3</rdf:li>
                    <rdf:li>0</rdf:li>
                  </rdf:Seq>
                </exif:ComponentsConfiguration>
              </rdf:Description>
            </rdf:RDF>
        "#;

        assert_eq!(
            parse_xmp(xml).unwrap(),
            vec![(
                "XMP-exif:ComponentsConfiguration".to_string(),
                "Y, Cb, Cr, -".to_string(),
            )]
        );
    }

    #[test]
    fn exif_contrast_and_saturation_use_xmp_pm_print_conversion() {
        // XMP.pm:2275-2290; NikonCoolpixP520.jpg stores both as 0.
        for tag in ["XMP-exif:Contrast", "XMP-exif:Saturation"] {
            assert_eq!(format_xmp_value(tag, "0"), "Normal");
            assert_eq!(format_xmp_value(tag, "1"), "Low");
            assert_eq!(format_xmp_value(tag, "2"), "High");
        }
        assert_eq!(format_xmp_value("XMP-crs:Contrast", "0"), "0");
        assert_eq!(format_xmp_value("XMP-crs:Saturation", "0"), "0");
    }

    #[test]
    fn components_configuration_uses_xmp_exif_print_conversion() {
        // XMP.pm:2011-2024: the integer Seq in XMP.xmp is displayed as
        // Y, Cb, Cr, - rather than its stored 1, 2, 3, 0 values.
        assert_eq!(
            format_xmp_value("XMP-exif:ComponentsConfiguration", "1"),
            "Y"
        );
        assert_eq!(
            format_xmp_value("XMP-exif:ComponentsConfiguration", "0"),
            "-"
        );
        // No conversion leaks into an unrelated namespace with the same name.
        assert_eq!(format_xmp_value("XMP:ComponentsConfiguration", "1"), "1");
    }

    // The expectations below are `exiftool -G1 -s` output for an XMP packet
    // holding each raw value; see the ExifTool source lines quoted on each
    // helper.

    #[test]
    fn fnumber_keeps_print_fnumbers_trailing_zero() {
        // Exif.pm:5715 -- sprintf("%.1f"), which prints f/8 as "8.0".
        assert_eq!(format_xmp_value("XMP-exif:FNumber", "80/10"), "8.0");
        assert_eq!(format_xmp_value("XMP-exif:FNumber", "28/10"), "2.8");
        assert_eq!(format_xmp_value("XMP-exif:FNumber", "59/10"), "5.9");
        // Below f/1.0 the format is "%.2f".
        assert_eq!(format_xmp_value("XMP-exif:FNumber", "95/100"), "0.95");
        // Not a positive float -- PrintFNumber returns the value untouched.
        assert_eq!(format_xmp_value("XMP-exif:FNumber", "0/1"), "0/1");
        assert_eq!(format_xmp_value("XMP-exif:FNumber", "unknown"), "unknown");
    }

    #[test]
    fn exposure_time_uses_print_exposure_time() {
        assert_eq!(format_xmp_value("XMP-exif:ExposureTime", "1/125"), "1/125");
        assert_eq!(format_xmp_value("XMP-exif:ExposureTime", "4/10"), "0.4");
        assert_eq!(format_xmp_value("XMP-exif:ExposureTime", "30/1"), "30");
    }

    #[test]
    fn photomechanic_color_class_names_its_numbers() {
        // PhotoMechanic.pm:23-33 (%colorClasses).
        assert_eq!(format_xmp_value("XMP:ColorClass", "0"), "0 (None)");
        assert_eq!(format_xmp_value("XMP:ColorClass", "6"), "6 (Typical alt)");
        assert_eq!(format_xmp_value("XMP:ColorClass", "8"), "8 (Trash)");
        // Nothing is invented past the end of ExifTool's table.
        assert_eq!(format_xmp_value("XMP:ColorClass", "9"), "9");
    }

    #[test]
    fn photomechanic_tagged_is_the_xmp_boolean_only() {
        // PhotoMechanic.pm:133 -- PrintConv => { False => 'No', True => 'Yes' }.
        assert_eq!(format_xmp_value("XMP:Tagged", "True"), "Yes");
        assert_eq!(format_xmp_value("XMP:Tagged", "False"), "No");
        // ExifTool has no entry for 0/1, so neither do we.
        assert_eq!(format_xmp_value("XMP:Tagged", "0"), "0");
        assert_eq!(format_xmp_value("XMP:Tagged", "1"), "1");
    }

    #[test]
    fn photomechanic_prefs_names_its_four_fields() {
        // PhotoMechanic.pm:120-127.
        assert_eq!(
            format_xmp_value("XMP:Prefs", "0:6:5:003344"),
            "Tagged:0, ColorClass:6, Rating:5, FrameNum:003344"
        );
        // The first three captures are `\d+`; anything else is left alone.
        assert_eq!(format_xmp_value("XMP:Prefs", "0:6:5"), "0:6:5");
        assert_eq!(format_xmp_value("XMP:Prefs", "a:b:c:d"), "a:b:c:d");
    }

    #[test]
    fn photomechanic_time_created_uses_exif_time_conversion() {
        // PhotoMechanic.pm:134-140 -> Exif.pm:6085-6094 (ExifTime).
        assert_eq!(
            format_xmp_value("XMP:TimeCreated", "062751-0500"),
            "06:27:51-05:00"
        );
        assert_eq!(
            format_xmp_value("XMP:TimeCreated", "10 30 55+0500"),
            "10:30:55+05:00"
        );
        assert_eq!(
            format_xmp_value("XMP:TimeCreated", "not-a-time"),
            "not-a-time"
        );
    }

    #[test]
    fn photomechanic_jpeg_time_created_matches_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path =
            std::path::Path::new("/tmp/oxidex-exiftool-cache/combined-samples/PhotoMechanic.jpg");
        let metadata =
            crate::core::operations::read_metadata(path).expect("PhotoMechanic JPEG parses");

        assert_eq!(
            metadata.get_string("XMP:TimeCreated"),
            Some("06:27:51-05:00")
        );
    }

    #[test]
    fn perspective_upright_names_its_numbers() {
        // XMP.pm:1666-1677.
        assert_eq!(format_xmp_value("XMP-crs:PerspectiveUpright", "0"), "Off");
        assert_eq!(format_xmp_value("XMP-crs:PerspectiveUpright", "2"), "Full");
        assert_eq!(
            format_xmp_value("XMP-crs:PerspectiveUpright", "5"),
            "Guided"
        );
        assert_eq!(format_xmp_value("XMP-crs:PerspectiveUpright", "6"), "6");
    }

    #[test]
    fn rationals_carrying_a_unit_keep_it() {
        // XMP.pm:2110-2115 / XMP.pm:2342-2349 -- PrintConv appends " m".
        assert_eq!(format_xmp_value("XMP-exif:GPSAltitude", "40/1 m"), "40 m");
        assert_eq!(
            format_xmp_value("XMP-exif:SubjectDistance", "501/100 m"),
            "5.01 m"
        );
        // A rational LIST must never be read as "first element plus a unit".
        assert_eq!(
            format_xmp_rational_with_unit("18/1 55/1 0/0 0/0"),
            "18/1 55/1 0/0 0/0"
        );
    }

    #[test]
    fn bare_rationals_are_evaluated_like_convert_rational() {
        // XMP.pm:3400 -- `^(-?\d+)/(-?\d+)$`, applied to every rational-typed
        // and every unknown XMP property.
        assert_eq!(
            format_xmp_value("XMP-exif:CompressedBitsPerPixel", "3/1"),
            "3"
        );
        assert_eq!(
            format_xmp_value("XMP-exif:DigitalZoomRatio", "2272/2272"),
            "1"
        );
        assert_eq!(
            format_xmp_value("XMP-tiff:XResolution", "1800000/10000"),
            "180"
        );
        // A zero denominator is `inf`/`undef` to ExifTool, never a quotient.
        assert_eq!(format_xmp_value("XMP-exif:FlashEnergy", "1/0"), "1/0");
        // Not the rational shape at all.
        assert_eq!(
            format_xmp_value("XMP-dc:Format", "image/jpeg"),
            "image/jpeg"
        );
        assert_eq!(format_xmp_value("XMP-crs:Version", "1.5/2"), "1.5/2");
    }

    #[test]
    fn date_tags_print_exif_style_and_keep_the_offset() {
        // XMP.pm:236-243 (%dateTimeInfo) -> ConvertDateTime.
        assert_eq!(
            format_xmp_value("XMP-xmp:CreateDate", "2021-10-01T14:18:11.534+01:00"),
            "2021:10:01 14:18:11.534+01:00"
        );
        assert_eq!(
            format_xmp_value("XMP-xmp:ModifyDate", "2021-10-01T14:18:11Z"),
            "2021:10:01 14:18:11Z"
        );
        assert_eq!(
            format_xmp_value("XMP-photoshop:DateCreated", "2004-02-26"),
            "2004:02:26"
        );
        assert_eq!(
            format_xmp_value("XMP-tiff:DateTime", "2005-08-03T18:59:18-04:00"),
            "2005:08:03 18:59:18-04:00"
        );
        // Already EXIF-style, and non-dates, pass through untouched.
        assert_eq!(
            format_xmp_value("XMP-xmp:ModifyDate", "2005:06:09 20:09:27+02:00"),
            "2005:06:09 20:09:27+02:00"
        );
        assert_eq!(format_xmp_value("XMP-xmp:CreateDate", ""), "");
        // A tag that is not one of ExifTool's date properties is left alone.
        assert_eq!(
            format_xmp_value("XMP-dc:Description", "2021-10-01T14:18:11Z"),
            "2021-10-01T14:18:11Z"
        );
        // XMPAutoConv territory: 13.59 has no definition for UTCAtExposure,
        // yet `exiftool -json -G DJI_M3T.jpg` prints
        // "2022:10:27 05:08:32.100476" -- subseconds survive, no zone.
        assert_eq!(
            format_xmp_value("XMP-drone-dji:UTCAtExposure", "2022-10-27T05:08:32.100476"),
            "2022:10:27 05:08:32.100476"
        );
    }

    #[test]
    fn pdf_trapped_removes_one_leading_slash() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description xmlns:pdf="http://ns.adobe.com/pdf/1.3/">
                <pdf:Trapped>/Unknown</pdf:Trapped>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = parse_xmp(xml).expect("parses PDF Trapped property");
        assert_eq!(
            tags.iter()
                .find(|(name, _)| name == "XMP:Trapped")
                .map(|(_, value)| value.as_str()),
            Some("Unknown")
        );
        assert_eq!(format_xmp_value("XMP:Trapped", "//Unknown"), "/Unknown");
    }

    #[test]
    fn pdf_trapped_removes_one_leading_slash() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description xmlns:pdf="http://ns.adobe.com/pdf/1.3/">
                <pdf:Trapped>/Unknown</pdf:Trapped>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = parse_xmp(xml).expect("parses PDF Trapped property");
        assert_eq!(
            tags.iter()
                .find(|(name, _)| name == "XMP:Trapped")
                .map(|(_, value)| value.as_str()),
            Some("Unknown")
        );
        assert_eq!(format_xmp_value("XMP:Trapped", "//Unknown"), "/Unknown");
    }

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

        // Check that Creator and Rating are present with simplified XMP: prefix
        let creators: Vec<_> = result
            .iter()
            .filter(|(name, _)| name == "XMP:Creator")
            .collect();
        assert_eq!(creators.len(), 1);
        assert_eq!(creators[0].1, "John Doe");

        let ratings: Vec<_> = result
            .iter()
            .filter(|(name, _)| name == "XMP:Rating")
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

        // Check for xmp properties with simplified XMP: prefix
        assert!(
            prop_names.iter().any(|n| n == "XMP:Creator"),
            "Missing XMP:Creator"
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:ModifyDate"),
            "Missing XMP:ModifyDate"
        );

        // Check for dc properties (Dublin Core uses simplified XMP: prefix and Title-case)
        assert!(
            prop_names.iter().any(|n| n == "XMP:Title"),
            "Missing XMP:Title (dc:title)"
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:Rights"),
            "Missing XMP:Rights (dc:rights)"
        );

        // Check for exif properties (EXIF namespace uses XMP-exif: prefix)
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
        assert!(prop_names.iter().any(|n| n == "XMP:Creator"));
        assert!(prop_names.iter().any(|n| n == "XMP:Title"));

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
            ("XMP:Creator".to_string(), "John Doe".to_string())
        );
    }

    #[test]
    fn test_utf8_content() {
        // Use a regular string literal and convert to bytes to support UTF-8
        let xml = r#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:dc="http://purl.org/dc/elements/1.1/">
              <rdf:Description>
                <dc:creator>Jose Garcia</dc:creator>
                <dc:title>Nandu en la Patagonia</dc:title>
                <dc:rights>Copyright 2024</dc:rights>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml.as_bytes()).unwrap();
        assert_eq!(result.len(), 3);

        // Verify content is preserved
        assert!(result.iter().any(|(_, v)| v.contains("Jose Garcia")));
        assert!(result.iter().any(|(_, v)| v.contains("Nandu")));
        assert!(result.iter().any(|(_, v)| v.contains("Copyright")));
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
            .filter(|(name, _)| name == "XMP:Creator")
            .collect();
        assert_eq!(creators.len(), 1);

        let titles: Vec<_> = result
            .iter()
            .filter(|(name, _)| name == "XMP:Title")
            .collect();
        assert_eq!(titles.len(), 1);
    }

    #[test]
    fn test_xmp_toolkit_extraction() {
        // Test extraction of XMPToolkit from x:xmpmeta element
        let xml = br#"
            <x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Image::ExifTool 12.46">
              <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
                <rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/">
                  <xmp:Creator>John Doe</xmp:Creator>
                </rdf:Description>
              </rdf:RDF>
            </x:xmpmeta>
        "#;

        let result = parse_xmp(xml).unwrap();

        // Should have XMPToolkit and Creator
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();
        assert!(
            prop_names.iter().any(|n| n == "XMP:XMPToolkit"),
            "Missing XMP:XMPToolkit. Found: {:?}",
            prop_names
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:Creator"),
            "Missing XMP:Creator"
        );

        // Verify XMPToolkit value
        let toolkit = result
            .iter()
            .find(|(name, _)| name == "XMP:XMPToolkit")
            .map(|(_, v)| v.as_str());
        assert_eq!(toolkit, Some("Image::ExifTool 12.46"));
    }

    #[test]
    fn test_rdf_about_extraction() {
        // Test extraction of rdf:about attribute from rdf:Description
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about="uuid:faf5bdd5-ba3d-11da-ad31-d33d75182f1b"
                               xmlns:xmp="http://ns.adobe.com/xap/1.0/">
                <xmp:Creator>John Doe</xmp:Creator>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();

        // Should have About and Creator
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();
        assert!(
            prop_names.iter().any(|n| n == "XMP:About"),
            "Missing XMP:About. Found: {:?}",
            prop_names
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:Creator"),
            "Missing XMP:Creator"
        );

        // Verify About value
        let about = result
            .iter()
            .find(|(name, _)| name == "XMP:About")
            .map(|(_, v)| v.as_str());
        assert_eq!(about, Some("uuid:faf5bdd5-ba3d-11da-ad31-d33d75182f1b"));
    }

    #[test]
    fn test_shorthand_attributes() {
        // Test extraction of XMP properties from rdf:Description attributes (shorthand form)
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmp="http://ns.adobe.com/xap/1.0/"
                     xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/">
              <rdf:Description rdf:about=""
                               xmp:CreateDate="2023-01-15T10:30:00"
                               xmp:ModifyDate="2023-01-20T14:00:00"
                               photoshop:DateCreated="2023-01-15">
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();

        // Should have shorthand properties extracted
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();
        assert!(
            prop_names.iter().any(|n| n == "XMP:CreateDate"),
            "Missing XMP:CreateDate. Found: {:?}",
            prop_names
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:ModifyDate"),
            "Missing XMP:ModifyDate"
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP-photoshop:DateCreated"),
            "Missing XMP-photoshop:DateCreated. Found: {:?}",
            prop_names
        );

        // Verify values
        let create_date = result
            .iter()
            .find(|(name, _)| name == "XMP:CreateDate")
            .map(|(_, v)| v.as_str());
        // `exiftool -G1 -s` on this packet:
        //   [XMP-xmp] CreateDate : 2023:01:15 10:30:00
        // The ISO 8601 form the attribute is written in is the raw value, not
        // the one ExifTool reports (XMP.pm:236-243, %dateTimeInfo).
        assert_eq!(create_date, Some("2023:01:15 10:30:00"));
    }

    #[test]
    fn test_self_closing_description_with_attributes() {
        // Test self-closing rdf:Description with shorthand properties
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:xmp="http://ns.adobe.com/xap/1.0/">
              <rdf:Description rdf:about="test.jpg"
                               xmp:Rating="5"
                               xmp:Label="Yellow" />
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();

        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();
        assert!(
            prop_names.iter().any(|n| n == "XMP:About"),
            "Missing XMP:About. Found: {:?}",
            prop_names
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:Rating"),
            "Missing XMP:Rating"
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:Label"),
            "Missing XMP:Label"
        );

        let rating = result
            .iter()
            .find(|(name, _)| name == "XMP:Rating")
            .map(|(_, v)| v.as_str());
        assert_eq!(rating, Some("5"));
    }

    #[test]
    fn test_full_xmp_packet_structure() {
        // Test a complete XMP packet with all features
        let xml = br#"
            <x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 5.6-c140">
              <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
                <rdf:Description rdf:about=""
                                 xmlns:xmp="http://ns.adobe.com/xap/1.0/"
                                 xmlns:dc="http://purl.org/dc/elements/1.1/"
                                 xmp:CreateDate="2023-01-15T10:30:00+05:30"
                                 xmp:ModifyDate="2023-01-20T14:00:00Z">
                  <dc:creator>John Doe</dc:creator>
                  <dc:title>My Photo</dc:title>
                </rdf:Description>
              </rdf:RDF>
            </x:xmpmeta>
        "#;

        let result = parse_xmp(xml).unwrap();

        // Verify all expected tags are present
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();

        // XMPToolkit from x:xmpmeta
        assert!(
            prop_names.iter().any(|n| n == "XMP:XMPToolkit"),
            "Missing XMP:XMPToolkit. Found: {:?}",
            prop_names
        );

        // Shorthand attributes from rdf:Description
        assert!(
            prop_names.iter().any(|n| n == "XMP:CreateDate"),
            "Missing XMP:CreateDate"
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:ModifyDate"),
            "Missing XMP:ModifyDate"
        );

        // Child element properties
        assert!(
            prop_names.iter().any(|n| n == "XMP:Creator"),
            "Missing XMP:Creator (dc:creator)"
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:Title"),
            "Missing XMP:Title (dc:title)"
        );

        // Verify XMPToolkit value
        let toolkit = result
            .iter()
            .find(|(name, _)| name == "XMP:XMPToolkit")
            .map(|(_, v)| v.as_str());
        assert_eq!(toolkit, Some("Adobe XMP Core 5.6-c140"));
    }

    #[test]
    fn test_empty_rdf_about_is_skipped() {
        // Test that empty rdf:about values are not included
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about=""
                               xmlns:xmp="http://ns.adobe.com/xap/1.0/">
                <xmp:Creator>John Doe</xmp:Creator>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();

        // Should only have Creator, not an empty About
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();
        assert!(
            !prop_names.iter().any(|n| n == "XMP:About"),
            "Should not include empty XMP:About. Found: {:?}",
            prop_names
        );
        assert!(
            prop_names.iter().any(|n| n == "XMP:Creator"),
            "Missing XMP:Creator"
        );
    }

    #[test]
    fn test_xml_entity_unescaping() {
        // Test that XML entities like &apos; are properly decoded
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/">
              <rdf:Description>
                <photoshop:Source>I&apos;m the source</photoshop:Source>
                <photoshop:Credit>&quot;Famous&quot;Photographer</photoshop:Credit>
                <photoshop:Instructions>Use&amp;enjoy</photoshop:Instructions>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();

        // Find the Source tag
        let source = result
            .iter()
            .find(|(name, _)| name.ends_with("Source"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            source,
            Some("I'm the source"),
            "Expected &apos; to be decoded to apostrophe"
        );

        // Find the Credit tag - no spaces around entities
        let credit = result
            .iter()
            .find(|(name, _)| name.ends_with("Credit"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            credit,
            Some("\"Famous\"Photographer"),
            "Expected &quot; to be decoded to double quote"
        );

        // Find the Instructions tag - no spaces around entity
        let instructions = result
            .iter()
            .find(|(name, _)| name.ends_with("Instructions"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            instructions,
            Some("Use&enjoy"),
            "Expected &amp; to be decoded to ampersand"
        );
    }

    #[test]
    fn test_rdf_seq_collection() {
        // Test the structure causing PSD issues - dc:creator with rdf:Seq inside
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:dc="http://purl.org/dc/elements/1.1/">
              <rdf:Description>
                <dc:creator>
                  <rdf:Seq>
                    <rdf:li>Phil Harvey</rdf:li>
                  </rdf:Seq>
                </dc:creator>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        eprintln!("Result: {:?}", result);

        // Should extract "Phil Harvey" from the rdf:Seq/rdf:li structure
        let creator = result
            .iter()
            .find(|(name, _)| name.ends_with("Creator") || name.ends_with("creator"))
            .map(|(n, v)| (n.as_str(), v.as_str()));

        assert!(
            creator.is_some(),
            "Expected to find Creator tag. Results: {:?}",
            result
        );
        let (name, value) = creator.unwrap();
        assert!(
            !value.contains("rdf:"),
            "Value should not contain raw RDF XML. Got: {}: {}",
            name,
            value
        );
        assert_eq!(value, "Phil Harvey", "Expected extracted value");
    }

    #[test]
    fn test_rdf_alt_collection() {
        // Test rdf:Alt for dc:title with xml:lang attribute
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:dc="http://purl.org/dc/elements/1.1/">
              <rdf:Description>
                <dc:title>
                  <rdf:Alt>
                    <rdf:li xml:lang="x-default">Test Picture</rdf:li>
                  </rdf:Alt>
                </dc:title>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        eprintln!("Result: {:?}", result);

        let title = result
            .iter()
            .find(|(name, _)| name.ends_with("Title") || name.ends_with("title"))
            .map(|(n, v)| (n.as_str(), v.as_str()));

        assert!(
            title.is_some(),
            "Expected to find Title tag. Results: {:?}",
            result
        );
        let (name, value) = title.unwrap();
        assert!(
            !value.contains("rdf:"),
            "Value should not contain raw RDF XML. Got: {}: {}",
            name,
            value
        );
        assert_eq!(value, "Test Picture", "Expected extracted value");
    }

    // =============================================================================
    // TESTS FOR 47 NEW TAGS ACROSS 6+ NAMESPACES
    // =============================================================================

    #[test]
    fn test_dublin_core_namespace_tags() {
        // Test Dublin Core (dc:) namespace tags: Title, Creator, Subject, Description, Language, Rights
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:dc="http://purl.org/dc/elements/1.1/">
              <rdf:Description>
                <dc:title>My Photo Collection</dc:title>
                <dc:creator>Jane Smith</dc:creator>
                <dc:subject>landscape, nature</dc:subject>
                <dc:description>Beautiful mountain scenery</dc:description>
                <dc:language>en</dc:language>
                <dc:rights>Copyright 2024 Jane Smith</dc:rights>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();

        // Verify all 6 Dublin Core tags are extracted
        assert!(prop_names.iter().any(|n| n == "XMP:Title"));
        assert!(prop_names.iter().any(|n| n == "XMP:Creator"));
        assert!(prop_names.iter().any(|n| n == "XMP:Subject"));
        assert!(prop_names.iter().any(|n| n == "XMP:Description"));
        assert!(prop_names.iter().any(|n| n == "XMP:Language"));
        assert!(prop_names.iter().any(|n| n == "XMP:Rights"));

        // Verify values
        let title = result
            .iter()
            .find(|(n, _)| n == "XMP:Title")
            .map(|(_, v)| v);
        assert_eq!(title, Some(&"My Photo Collection".to_string()));
    }

    #[test]
    fn test_photoshop_namespace_tags() {
        // Test Photoshop namespace tags: AuthorsPosition, Caption, CreditLine, Source, CopyrightNotice, Instructions
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/">
              <rdf:Description>
                <photoshop:AuthorsPosition>Chief Photographer</photoshop:AuthorsPosition>
                <photoshop:Caption>Beautiful sunset over the ocean</photoshop:Caption>
                <photoshop:CreditLine>Photo by Jane Smith</photoshop:CreditLine>
                <photoshop:Source>Stock Photo Database</photoshop:Source>
                <photoshop:CopyrightNotice>Copyright 2024 Jane Smith</photoshop:CopyrightNotice>
                <photoshop:Instructions>Do not modify without permission</photoshop:Instructions>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();

        // Verify all 6 Photoshop tags are extracted with XMP-photoshop: prefix
        assert!(
            prop_names
                .iter()
                .any(|n| n == "XMP-photoshop:AuthorsPosition")
        );
        assert!(prop_names.iter().any(|n| n == "XMP-photoshop:Caption"));
        assert!(prop_names.iter().any(|n| n == "XMP-photoshop:CreditLine"));
        assert!(prop_names.iter().any(|n| n == "XMP-photoshop:Source"));
        assert!(
            prop_names
                .iter()
                .any(|n| n == "XMP-photoshop:CopyrightNotice")
        );
        assert!(prop_names.iter().any(|n| n == "XMP-photoshop:Instructions"));
    }

    #[test]
    fn test_tiff_namespace_tags() {
        // Test TIFF namespace tags: Make, Model, XResolution, YResolution, Software, DateTime
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:tiff="http://ns.adobe.com/tiff/1.0/">
              <rdf:Description>
                <tiff:Make>Canon</tiff:Make>
                <tiff:Model>Canon EOS R5</tiff:Model>
                <tiff:XResolution>300</tiff:XResolution>
                <tiff:YResolution>300</tiff:YResolution>
                <tiff:Software>Adobe Lightroom 6.0</tiff:Software>
                <tiff:DateTime>2024-01-15T14:30:00</tiff:DateTime>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();

        // Verify all 6 TIFF tags are extracted with XMP-tiff: prefix
        assert!(prop_names.iter().any(|n| n == "XMP-tiff:Make"));
        assert!(prop_names.iter().any(|n| n == "XMP-tiff:Model"));
        assert!(prop_names.iter().any(|n| n == "XMP-tiff:XResolution"));
        assert!(prop_names.iter().any(|n| n == "XMP-tiff:YResolution"));
        assert!(prop_names.iter().any(|n| n == "XMP-tiff:Software"));
        assert!(prop_names.iter().any(|n| n == "XMP-tiff:DateTime"));

        // Verify values
        let make = result
            .iter()
            .find(|(n, _)| n == "XMP-tiff:Make")
            .map(|(_, v)| v);
        assert_eq!(make, Some(&"Canon".to_string()));
    }

    #[test]
    fn test_exif_namespace_tags() {
        // Test EXIF namespace tags: ISO, ShutterSpeed, Aperture, ExposureCompensation, FocalLength
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:exif="http://ns.adobe.com/exif/1.0/">
              <rdf:Description>
                <exif:ISO>3200</exif:ISO>
                <exif:ShutterSpeed>0.004</exif:ShutterSpeed>
                <exif:Aperture>2.8</exif:Aperture>
                <exif:ExposureCompensation>0.5</exif:ExposureCompensation>
                <exif:FocalLength>50</exif:FocalLength>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();

        // Verify all 5 EXIF tags are extracted with XMP-exif: prefix
        assert!(prop_names.iter().any(|n| n == "XMP-exif:ISO"));
        assert!(prop_names.iter().any(|n| n == "XMP-exif:ShutterSpeed"));
        assert!(prop_names.iter().any(|n| n == "XMP-exif:Aperture"));
        assert!(
            prop_names
                .iter()
                .any(|n| n == "XMP-exif:ExposureCompensation")
        );
        assert!(prop_names.iter().any(|n| n == "XMP-exif:FocalLength"));
    }

    #[test]
    fn test_exif_exposure_formatting() {
        // Test that EXIF exposure tags are properly formatted
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:exif="http://ns.adobe.com/exif/1.0/">
              <rdf:Description>
                <exif:ISO>1600</exif:ISO>
                <exif:Aperture>5.6</exif:Aperture>
                <exif:FocalLength>85</exif:FocalLength>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();

        // Verify formatting
        let aperture = result
            .iter()
            .find(|(n, _)| n == "XMP-exif:Aperture")
            .map(|(_, v)| v.as_str());
        assert_eq!(aperture, Some("f/5.6"));

        let focal = result
            .iter()
            .find(|(n, _)| n == "XMP-exif:FocalLength")
            .map(|(_, v)| v.as_str());
        // XMP.pm:2164 -- `PrintConv => 'sprintf("%.1f mm",$val)'`, one decimal
        // even for a whole number of millimetres.
        assert_eq!(focal, Some("85.0 mm"));
    }

    #[test]
    fn test_multiple_namespace_extraction() {
        // Test extracting tags from multiple namespaces in one document
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                     xmlns:dc="http://purl.org/dc/elements/1.1/"
                     xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
                     xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
                     xmlns:exif="http://ns.adobe.com/exif/1.0/">
              <rdf:Description>
                <dc:title>Landscape Photo</dc:title>
                <photoshop:Caption>Mountain view at sunrise</photoshop:Caption>
                <tiff:Make>Sony</tiff:Make>
                <exif:ISO>400</exif:ISO>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let result = parse_xmp(xml).unwrap();
        let prop_names: Vec<String> = result.iter().map(|(name, _)| name.clone()).collect();

        // Verify tags from all 4 namespaces are present
        assert!(prop_names.iter().any(|n| n == "XMP:Title"));
        assert!(prop_names.iter().any(|n| n == "XMP-photoshop:Caption"));
        assert!(prop_names.iter().any(|n| n == "XMP-tiff:Make"));
        assert!(prop_names.iter().any(|n| n == "XMP-exif:ISO"));

        assert_eq!(
            result.len(),
            4,
            "Expected 4 properties from multiple namespaces"
        );
    }

    #[test]
    fn test_namespace_resolver_with_new_namespaces() {
        // Test that namespace resolver correctly handles all namespace URIs
        use crate::parsers::xmp::namespace_resolver::NamespaceResolver;

        let resolver = NamespaceResolver::new();

        // Verify all standard namespaces are pre-registered
        assert_eq!(
            resolver.resolve_prefix("dc"),
            Some("http://purl.org/dc/elements/1.1/")
        );
        assert_eq!(
            resolver.resolve_prefix("photoshop"),
            Some("http://ns.adobe.com/photoshop/1.0/")
        );
        assert_eq!(
            resolver.resolve_prefix("tiff"),
            Some("http://ns.adobe.com/tiff/1.0/")
        );
        assert_eq!(
            resolver.resolve_prefix("exif"),
            Some("http://ns.adobe.com/exif/1.0/")
        );
    }

    #[test]
    fn test_formatter_functions() {
        // Test individual formatter functions for new namespace tags

        // EXIF ISO formatting
        assert_eq!(format_exif_iso("100"), "100");
        assert_eq!(format_exif_iso("6400"), "6400");

        // EXIF aperture formatting
        assert_eq!(format_exif_aperture("2.8"), "f/2.8");
        assert_eq!(format_exif_aperture("5.6"), "f/5.6");
        assert_eq!(format_exif_aperture("8"), "f/8");

        // EXIF focal length formatting. XMP.pm:2164 prints one decimal
        // unconditionally, and the stored form may be a rational.
        assert_eq!(format_exif_focal_length("50"), "50.0 mm");
        assert_eq!(format_exif_focal_length("85.0"), "85.0 mm");
        assert_eq!(format_exif_focal_length("24.5"), "24.5 mm");
        assert_eq!(format_exif_focal_length("5800/1000"), "5.8 mm");

        // TIFF resolution formatting
        assert_eq!(format_tiff_resolution("300"), "300");
        assert_eq!(format_tiff_resolution("72.5"), "72.5");

        // Photoshop quality formatting
        assert_eq!(format_photoshop_quality("85"), "85%");
        assert_eq!(format_photoshop_quality("100"), "100%");
    }

    #[test]
    fn test_exposure_compensation_formatting() {
        // XMP.pm:2099 -- `PrintConv => Image::ExifTool::Exif::PrintFraction`,
        // not a fixed two decimals: whole stops print signed, halves and
        // thirds print as fractions, and the stored form is a rational.
        assert_eq!(format_exif_exposure_compensation("1.0"), "+1");
        assert_eq!(format_exif_exposure_compensation("-0.5"), "-1/2");
        assert_eq!(format_exif_exposure_compensation("0"), "0");
        assert_eq!(format_exif_exposure_compensation("1/3"), "+1/3");
        assert_eq!(format_exif_exposure_compensation("-3/3"), "-1");
    }

    #[test]
    fn test_shutter_speed_formatting() {
        // Test shutter speed formatting with 3 decimal places for decimal values
        assert_eq!(format_exif_shutter_speed("0.004"), "0.004");
        assert_eq!(format_exif_shutter_speed("1/250"), "1/250");
        assert_eq!(format_exif_shutter_speed("0.5"), "0.500");
    }
}

#[cfg(test)]
mod top_level_struct_tests {
    use super::*;

    fn tag<'a>(tags: &'a [(String, String)], name: &str) -> Option<&'a str> {
        tags.iter()
            .find(|(t, _)| t == name)
            .map(|(_, v)| v.as_str())
    }

    /// `test:BareStruct` / `test:Item1` flattens to `BareStructItem1`, per
    /// ExifTool's `GetXMPTagID` (`$tag .= ucfirst($nm)`). Values are the
    /// literals `exiftool -G1 -a -s XMP4.xmp` reports for XMP-test.
    #[test]
    fn flattens_bare_struct_fields() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about="" xmlns:test="http://ns.test.com/">
                <test:BareStruct rdf:parseType="Resource">
                  <test:Item1>a1</test:Item1>
                  <test:Item2>a2</test:Item2>
                </test:BareStruct>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = extract_top_level_struct_values(xml).unwrap();
        assert_eq!(tag(&tags, "XMP:BareStructItem1"), Some("a1"));
        assert_eq!(tag(&tags, "XMP:BareStructItem2"), Some("a2"));
    }

    /// `myXMPns:BTestTag` / `myXMPns:Field1` holds a Bag of lang-alts, which
    /// ExifTool reports as `BTestTagField1-<lang>`. Values are the literals
    /// `exiftool -G1 -a -s XMP5.xmp` reports for XMP-myXMPns.
    #[test]
    fn flattens_lang_alternatives_in_struct_field() {
        let xml = "
            <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>
              <rdf:Description rdf:about='' xmlns:myXMPns='http://ns.exiftool.org/t/XMP.t'>
                <myXMPns:BTestTag rdf:parseType='Resource'>
                  <myXMPns:Field1>
                    <rdf:Bag>
                      <rdf:li>
                        <rdf:Alt>
                          <rdf:li xml:lang='en-CA'>eh?</rdf:li>
                          <rdf:li xml:lang='en-US'>huh?</rdf:li>
                        </rdf:Alt>
                      </rdf:li>
                      <rdf:li>
                        <rdf:Alt>
                          <rdf:li xml:lang='en-US'>groovy</rdf:li>
                          <rdf:li xml:lang='fr'>ing\u{e9}nieux</rdf:li>
                        </rdf:Alt>
                      </rdf:li>
                    </rdf:Bag>
                  </myXMPns:Field1>
                </myXMPns:BTestTag>
              </rdf:Description>
            </rdf:RDF>
        "
        .as_bytes();

        let tags = extract_top_level_struct_values(xml).unwrap();
        assert_eq!(tag(&tags, "XMP:BTestTagField1-en-CA"), Some("eh?"));
        assert_eq!(tag(&tags, "XMP:BTestTagField1-fr"), Some("ingénieux"));
        // ExifTool prints en-US twice ("huh?" then "groovy") because the
        // field is List-valued; we keep the first and never emit the tag
        // twice.
        assert_eq!(tag(&tags, "XMP:BTestTagField1-en-US"), Some("huh?"));
        assert_eq!(
            tags.iter()
                .filter(|(t, _)| t == "XMP:BTestTagField1-en-US")
                .count(),
            1
        );
    }

    /// A lang-alt buried under a sub-structure belongs to a longer flattened
    /// name (ExifTool calls this one `RegionExtensionsArtworkTitle-de`), so
    /// this pass must not claim it as `RegionsRegionList-de`.
    #[test]
    fn does_not_flatten_through_substructures() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about=""
                  xmlns:mwg-rs="http://www.metadataworkinggroup.com/schemas/regions/"
                  xmlns:Iptc4xmpExt="http://iptc.org/std/Iptc4xmpExt/2008-02-29/">
                <mwg-rs:Regions rdf:parseType="Resource">
                  <mwg-rs:RegionList>
                    <rdf:Bag>
                      <rdf:li rdf:parseType="Resource">
                        <mwg-rs:Extensions rdf:parseType="Resource">
                          <Iptc4xmpExt:AOTitle>
                            <rdf:Alt><rdf:li xml:lang="de">verfaenglich</rdf:li></rdf:Alt>
                          </Iptc4xmpExt:AOTitle>
                        </mwg-rs:Extensions>
                      </rdf:li>
                    </rdf:Bag>
                  </mwg-rs:RegionList>
                </mwg-rs:Regions>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = extract_top_level_struct_values(xml).unwrap();
        assert!(
            !tags
                .iter()
                .any(|(t, _)| t.starts_with("XMP:RegionsRegionList")),
            "sub-structure leaked into the parent field name: {tags:?}"
        );
    }

    /// ExifTool capitalises each path component (`ucfirst`), so the lowercase
    /// `stRef:instanceID` field of `xapMM:DerivedFrom` becomes
    /// `DerivedFromInstanceID` -- exactly what `exiftool -G1 -a -s XMP.inx`
    /// reports for XMP-xmpMM.
    #[test]
    fn capitalises_lowercase_field_names() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about=""
                  xmlns:xapMM="http://ns.adobe.com/xap/1.0/mm/"
                  xmlns:stRef="http://ns.adobe.com/xap/1.0/sType/ResourceRef#">
                <xapMM:DerivedFrom rdf:parseType="Resource">
                  <stRef:instanceID>f0d208df-dc56-11df-95ac-e273561c7691</stRef:instanceID>
                  <stRef:documentID>adobe:docid:indd:0419e6d7</stRef:documentID>
                </xapMM:DerivedFrom>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = extract_top_level_struct_values(xml).unwrap();
        assert_eq!(
            tag(&tags, "XMP:DerivedFromInstanceID"),
            Some("f0d208df-dc56-11df-95ac-e273561c7691")
        );
        assert_eq!(
            tag(&tags, "XMP:DerivedFromDocumentID"),
            Some("adobe:docid:indd:0419e6d7")
        );
    }

    /// The `exif:Flash` struct flattens to FlashMode/FlashReturn, and those
    /// carry PrintConv tables in XMP.pm: Mode 2 => 'Off', Return 0 => 'No
    /// return detection'.
    #[test]
    fn decodes_flash_struct_enums() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about="" xmlns:exif="http://ns.adobe.com/exif/1.0/">
                <exif:Flash rdf:parseType="Resource">
                  <exif:Mode>2</exif:Mode>
                  <exif:Return>0</exif:Return>
                </exif:Flash>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = parse_xmp(xml).unwrap();
        assert_eq!(tag(&tags, "XMP:FlashMode"), Some("Off"));
        assert_eq!(tag(&tags, "XMP:FlashReturn"), Some("No return detection"));
    }

    /// XMP.pm Flash struct PrintConv tables, asserted as literals.
    #[test]
    fn flash_printconv_tables_match_exiftool() {
        assert_eq!(decode_xmp_flash_mode("0"), "Unknown");
        assert_eq!(decode_xmp_flash_mode("1"), "On");
        assert_eq!(decode_xmp_flash_mode("2"), "Off");
        assert_eq!(decode_xmp_flash_mode("3"), "Auto");
        assert_eq!(decode_xmp_flash_return("0"), "No return detection");
        assert_eq!(decode_xmp_flash_return("2"), "Return not detected");
        assert_eq!(decode_xmp_flash_return("3"), "Return detected");
        // 1 is absent from ExifTool's Return table, so it passes through.
        assert_eq!(decode_xmp_flash_return("1"), "1");
    }

    /// The `exif:Flash` struct's three `boolean` fields carry `%boolConv`
    /// (XMP.pm:2139/2157/2158), whose `OTHER` sub lower-cases the value before
    /// comparing (XMP.pm:246-257). A file that spells them the XML Schema way
    /// -- `<exif:Fired>true</exif:Fired>` -- therefore still reports `True`,
    /// and oxidex used to echo the file's own lowercase spelling instead.
    ///
    /// Ground truth, exiftool 13.59 (the release `.exiftool-version` pins) on a
    /// JPEG whose XMP packet is the one below:
    ///
    /// ```text
    /// $ exiftool -G1 -s -XMP-exif:all flash_lc.jpg
    /// [XMP-exif]      FlashFired                      : True
    /// [XMP-exif]      FlashFunction                   : False
    /// [XMP-exif]      FlashRedEyeMode                 : True
    /// ```
    #[test]
    fn flash_struct_booleans_normalise_case_like_exiftool() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about="" xmlns:exif="http://ns.adobe.com/exif/1.0/">
                <exif:Flash rdf:parseType="Resource">
                  <exif:Fired>true</exif:Fired>
                  <exif:Function>false</exif:Function>
                  <exif:RedEyeMode>true</exif:RedEyeMode>
                </exif:Flash>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = parse_xmp(xml).unwrap();
        assert_eq!(tag(&tags, "XMP:FlashFired"), Some("True"));
        assert_eq!(tag(&tags, "XMP:FlashFunction"), Some("False"));
        assert_eq!(tag(&tags, "XMP:FlashRedEyeMode"), Some("True"));
    }

    /// `%boolConv` asserted as literals, including the spelling ExifTool's own
    /// files use (`True`/`False`, which the hash matches directly) and a
    /// non-boolean, which `OTHER` returns unchanged rather than coercing.
    #[test]
    fn xmp_boolconv_matches_exiftool() {
        assert_eq!(decode_xmp_boolean("True"), "True");
        assert_eq!(decode_xmp_boolean("False"), "False");
        assert_eq!(decode_xmp_boolean("true"), "True");
        assert_eq!(decode_xmp_boolean("false"), "False");
        assert_eq!(decode_xmp_boolean("TRUE"), "True");
        assert_eq!(decode_xmp_boolean("FALSE"), "False");
        // ExifTool's table has no 0/1 entry and `OTHER` returns $val, so these
        // must not become True/False.
        assert_eq!(decode_xmp_boolean("1"), "1");
        assert_eq!(decode_xmp_boolean("0"), "0");
        assert_eq!(decode_xmp_boolean(""), "");
    }

    /// `Iptc4xmpCore:CreatorContactInfo` fields have concatenated tag IDs but
    /// are reported under the shorter names XMP.pm pre-defines for them.
    /// Values are the literals `exiftool -G1 -a -s Sony/SonyDSC-P2.jpg`
    /// reports for XMP-iptcCore.
    #[test]
    fn renames_flattened_creator_contact_info() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about=""
                  xmlns:Iptc4xmpCore="http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/">
                <Iptc4xmpCore:CreatorContactInfo rdf:parseType="Resource">
                  <Iptc4xmpCore:CiAdrCity>Amsterdam</Iptc4xmpCore:CiAdrCity>
                  <Iptc4xmpCore:CiAdrCtry>Netherlands</Iptc4xmpCore:CiAdrCtry>
                  <Iptc4xmpCore:CiAdrExtadr>Govert Flinkstraat 302hs</Iptc4xmpCore:CiAdrExtadr>
                  <Iptc4xmpCore:CiAdrPcode>1073 CH</Iptc4xmpCore:CiAdrPcode>
                </Iptc4xmpCore:CreatorContactInfo>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = extract_top_level_struct_values(xml).unwrap();
        assert_eq!(tag(&tags, "XMP:CreatorCity"), Some("Amsterdam"));
        assert_eq!(tag(&tags, "XMP:CreatorCountry"), Some("Netherlands"));
        assert_eq!(
            tag(&tags, "XMP:CreatorAddress"),
            Some("Govert Flinkstraat 302hs")
        );
        assert_eq!(tag(&tags, "XMP:CreatorPostalCode"), Some("1073 CH"));
        assert!(
            !tags
                .iter()
                .any(|(t, _)| t.starts_with("XMP:CreatorContactInfoCi")),
            "raw flattened IDs leaked instead of ExifTool names: {tags:?}"
        );
    }

    /// Spot-check of the rename table against the literal names in ExifTool
    /// 13.55, and of the pass-through for IDs it does not cover.
    #[test]
    fn flat_tag_rename_table_matches_exiftool() {
        assert_eq!(
            exiftool_flat_tag_name("CreatorContactInfoCiUrlWork"),
            "CreatorWorkURL"
        );
        assert_eq!(exiftool_flat_tag_name("RegionsRegionList"), "RegionList");
        assert_eq!(
            exiftool_flat_tag_name("KeywordsHierarchy"),
            "HierarchicalKeywords"
        );
        assert_eq!(exiftool_flat_tag_name("EventEventDate"), "EventDate");
        assert_eq!(exiftool_flat_tag_name("PageInfoImage"), "PageImage");
        // Not renamed by any table: the concatenated ID is the reported name.
        assert_eq!(exiftool_flat_tag_name("BareStructItem1"), "BareStructItem1");
        assert_eq!(exiftool_flat_tag_name("FlashMode"), "FlashMode");
    }

    /// PLUS.pm gives `CopyrightOwner` `FlatName => ''`, so the field keeps its
    /// own name. Value is what `exiftool -G1 -a -s PLUS.xmp` reports.
    #[test]
    fn extracts_plus_copyright_owner_name() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about="" xmlns:plus="http://ns.useplus.org/ldf/xmp/1.0/">
                <plus:CopyrightOwner>
                  <rdf:Seq>
                    <rdf:li rdf:parseType="Resource">
                      <plus:CopyrightOwnerName>Phil Harvey</plus:CopyrightOwnerName>
                    </rdf:li>
                  </rdf:Seq>
                </plus:CopyrightOwner>
              </rdf:Description>
            </rdf:RDF>
        "#;

        let tags = extract_plus_copyright_owner_name(xml).unwrap();
        assert_eq!(tag(&tags, "XMP:CopyrightOwnerName"), Some("Phil Harvey"));
    }

    /// The same local name in a foreign namespace must not be mistaken for the
    /// PLUS field.
    #[test]
    fn ignores_copyright_owner_name_outside_plus_namespace() {
        let xml = br#"
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description rdf:about="" xmlns:plus="http://example.invalid/not-plus/">
                <plus:CopyrightOwner>
                  <rdf:Seq>
                    <rdf:li rdf:parseType="Resource">
                      <plus:CopyrightOwnerName>Nobody</plus:CopyrightOwnerName>
                    </rdf:li>
                  </rdf:Seq>
                </plus:CopyrightOwner>
              </rdf:Description>
            </rdf:RDF>
        "#;

        assert!(extract_plus_copyright_owner_name(xml).unwrap().is_empty());
    }

    /// XMP.xmp stores every EXIF-derived number as a rational; the formatters
    /// used to call `parse::<f64>` straight on "5800/1000" and pass the raw
    /// fraction through.
    #[test]
    fn xmp_rationals_parse() {
        assert_eq!(parse_xmp_number("5800/1000"), Some(5.8));
        assert_eq!(parse_xmp_number("-3/3"), Some(-1.0));
        assert_eq!(parse_xmp_number("2.5"), Some(2.5));
        assert_eq!(parse_xmp_number("1/0"), None);
        assert_eq!(parse_xmp_number("not a number"), None);
    }

    /// Values quoted from `exiftool -G1 -s XMP.xmp` (ExifTool 13.59).
    #[test]
    fn xmp_apex_and_rational_values_match_exiftool() {
        // [XMP] ApertureValue : 2.8   (exif:ApertureValue = 95/32)
        assert_eq!(format_xmp_apex_aperture("95/32"), "2.8");
        // [XMP] MaxApertureValue : 2.8
        assert_eq!(format_xmp_apex_aperture("95/32"), "2.8");
        // [XMP] ShutterSpeedValue : 0.4   (exif:ShutterSpeedValue = 42/32)
        assert_eq!(format_xmp_apex_shutter_speed("42/32"), "0.4");
        // [XMP] FocalLength : 5.8 mm   (exif:FocalLength = 5800/1000)
        assert_eq!(format_exif_focal_length("5800/1000"), "5.8 mm");
        // [XMP] FocalPlaneXResolution : 10142.8571428571
        assert_eq!(format_xmp_plain_rational("2272000/224"), "10142.8571428571");
        // [XMP] ExposureCompensation : -1   (exif:ExposureBiasValue = -3/3)
        assert_eq!(format_exif_exposure_compensation("-3/3"), "-1");
    }

    /// `XMP-exif:ShutterSpeedValue` is APEX (`XMP.pm:2081`,
    /// `ValueConv => 'abs($val)<100 ? 1/(2**$val) : 0'`) printed through
    /// `Image::ExifTool::Exif::PrintExposureTime` (`Exif.pm:5701`), which ends
    /// `$_ = sprintf("%.1f",$secs); s/\.0$//;` -- the trailing ".0" is dropped
    /// whenever the value *rounds* to a whole number of seconds, not only when
    /// it is exactly one. This regressed as "2.0" while the XMP path carried
    /// its own `seconds == seconds.trunc()` copy of the function.
    ///
    /// Ground truth, ExifTool 13.59, on a sidecar carrying
    /// `exif:ShutterSpeedValue='-201/200'` (APEX -1.005 -> 2.00694 s):
    ///
    /// ```text
    /// $ exiftool -G1 -s apex_edge.xmp
    /// [XMP-exif]      ShutterSpeedValue               : 2
    /// ```
    #[test]
    fn xmp_apex_shutter_speed_drops_a_rounded_trailing_zero() {
        // APEX -1.005 -> 2.00694 s. Perl: sprintf("%.1f") = "2.0", s/\.0$// = "2".
        assert_eq!(format_xmp_apex_shutter_speed("-201/200"), "2");
        // Exactly 2 s (APEX -1) has always printed "2"; keep it that way.
        assert_eq!(format_xmp_apex_shutter_speed("-1/1"), "2");
        // A value that rounds up to a whole second from below.
        // PrintExposureTime(0.959995800571048) == "1" under ExifTool 13.59.
        assert_eq!(print_xmp_exposure_time(0.959_995_800_571_048), "1");
        // 30.0438959140945 s -> "30", not "30.0".
        assert_eq!(print_xmp_exposure_time(30.043_895_914_094_5), "30");
        // The sub-quarter-second branch is untouched: XMP.xmp's 42/32 APEX
        // still prints "0.4", and a genuine fraction keeps one decimal.
        assert_eq!(format_xmp_apex_shutter_speed("42/32"), "0.4");
        assert_eq!(print_xmp_exposure_time(0.5), "0.5");
    }

    /// PrintFraction is what EXIF-style exposure compensation goes through;
    /// whole stops print signed, thirds and halves print as fractions, and
    /// anything else falls back to `%+.3g` -- three *significant digits*,
    /// trailing zeros stripped, not three decimal places. Exercises the
    /// public `format_exif_exposure_compensation` entry point (rationals as
    /// XMP actually stores them) rather than the shared `print_fraction`
    /// helper directly, since that helper already has its own doctest/unit
    /// coverage in `core::formatters::exif_print_conv`.
    #[test]
    fn print_fraction_follows_exif_pm() {
        assert_eq!(format_exif_exposure_compensation("0/10"), "0");
        assert_eq!(format_exif_exposure_compensation("-10/10"), "-1");
        assert_eq!(format_exif_exposure_compensation("20/10"), "+2");
        assert_eq!(format_exif_exposure_compensation("1/3"), "+1/3");
        assert_eq!(format_exif_exposure_compensation("5/10"), "+1/2");
        // Regression: NikonCoolpixP520.jpg's exif:ExposureBiasValue="-3/10".
        // ExifTool (`exiftool -G1 -s`) prints "-0.3"; a `{:+.3}` fallback
        // (three decimal places, not three significant digits) printed
        // "-0.300" here because -0.3 is not a whole/half/third stop.
        assert_eq!(format_exif_exposure_compensation("-3/10"), "-0.3");
        // Regression: LeicaV-LUX4.jpg's exif:ExposureBiasValue="-33/100".
        // ExifTool prints "-0.33"; the same fixed-decimals fallback printed
        // "-0.330".
        assert_eq!(format_exif_exposure_compensation("-33/100"), "-0.33");
    }

    /// XMP.pm gives PrintFraction to exif:ExposureBiasValue alone. A
    /// vendor property that merely shares the local name is reported
    /// verbatim -- ExifTool never applies another schema's PrintConv.
    ///
    /// Quoted from `exiftool -a -G1 -s FujiFilm/FujiFilmFinePixHS30EXR.jpg`:
    ///   [XMP-prefix0] ExposureCompensation : 0.080000
    #[test]
    fn exposure_compensation_print_conv_is_namespace_scoped() {
        assert_eq!(
            format_xmp_value("XMP-exif:ExposureCompensation", "-3/3"),
            "-1"
        );
        // MSImagingV1 (Microsoft Windows Live Photo Gallery) -- unknown
        // namespace, so oxidex files it under the bare XMP family.
        assert_eq!(
            format_xmp_value("XMP:ExposureCompensation", "0.080000"),
            "0.080000"
        );
        assert_eq!(
            format_xmp_value("XMP:ExposureCompensation", "0.000000"),
            "0.000000"
        );
    }

    /// XMP.pm renames three exif-namespace properties; without these the tags
    /// landed under names ExifTool never prints.
    #[test]
    fn exif_namespace_property_renames() {
        assert_eq!(
            exiftool_property_name("XMP-exif", "PixelXDimension"),
            "ExifImageWidth"
        );
        assert_eq!(
            exiftool_property_name("XMP-exif", "PixelYDimension"),
            "ExifImageHeight"
        );
        assert_eq!(
            exiftool_property_name("XMP-exif", "ExposureBiasValue"),
            "ExposureCompensation"
        );
        // The rename is namespace scoped.
        assert_eq!(
            exiftool_property_name("XMP-tiff", "PixelXDimension"),
            "PixelXDimension"
        );
    }

    /// `FlatName => ''` drops the structure's own name segment, so PLUS
    /// Licensee/LicenseeName is LicenseeName rather than LicenseeLicenseeName.
    #[test]
    fn flat_name_suppression_covers_the_plus_structures() {
        for name in ["Licensee", "Licensor", "CopyrightOwner", "ImageCreator"] {
            assert!(is_flat_name_suppressed(name), "{name}");
        }
        assert!(!is_flat_name_suppressed("LocationShown"));
    }

    /// A Bag of structures flattens to Container+Field, one value per rdf:li.
    /// Quoted from `exiftool -G1 -s XMP7.xmp`:
    ///   [XMP] LocationShownCity        : London, Paris, Berlin
    ///   [XMP] LocationShownCountryCode : GB, FR, DE
    #[test]
    fn list_struct_flattening_matches_exiftool() {
        let xml = br#"<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>
 <rdf:Description rdf:about=''
  xmlns:Iptc4xmpExt='http://iptc.org/std/Iptc4xmpExt/2008-02-29/'>
  <Iptc4xmpExt:LocationShown>
   <rdf:Bag>
    <rdf:li rdf:parseType='Resource'>
     <Iptc4xmpExt:City>London</Iptc4xmpExt:City>
     <Iptc4xmpExt:CountryCode>GB</Iptc4xmpExt:CountryCode>
    </rdf:li>
    <rdf:li rdf:parseType='Resource'>
     <Iptc4xmpExt:City>Paris</Iptc4xmpExt:City>
     <Iptc4xmpExt:CountryCode>FR</Iptc4xmpExt:CountryCode>
    </rdf:li>
   </rdf:Bag>
  </Iptc4xmpExt:LocationShown>
 </rdf:Description>
</rdf:RDF>"#;
        let tags = extract_list_struct_values(xml).unwrap();
        assert_eq!(
            tags.iter()
                .find(|(t, _)| t == "XMP:LocationShownCity")
                .map(|(_, values)| values.as_slice()),
            Some(["London".to_string(), "Paris".to_string()].as_slice())
        );
        assert_eq!(
            tags.iter()
                .find(|(t, _)| t == "XMP:LocationShownCountryCode")
                .map(|(_, values)| values.as_slice()),
            Some(["GB".to_string(), "FR".to_string()].as_slice())
        );
        // The container itself is not a flattened tag.
        assert!(!tags.iter().any(|(t, _)| t == "XMP:LocationShown"));

        // The Iptc4xmpExt table declares LocationShown as a Bag of
        // structures, so `exiftool -json XMP7.xmp` emits arrays rather than
        // one comma-joined scalar for its repeated fields.
        let typed = parse_xmp_typed(xml).unwrap();
        assert_eq!(
            typed
                .iter()
                .find(|(tag, _)| tag == "XMP:LocationShownCity")
                .map(|(_, value)| value),
            Some(&XmpValue::List(vec![
                "London".to_string(),
                "Paris".to_string()
            ]))
        );
    }

    #[test]
    fn unknown_schema_struct_lists_keep_the_first_repeated_field() {
        // `exiftool -G1 -s XMP4.xmp` reports XMP-test:StructList2Item1 as
        // c1-1 and StructList2Item2 as c2-1, despite the second rdf:li
        // carrying c1-2/c2-2.  The test namespace has no registered XMP
        // table, so ExifTool's generic duplicate handling keeps the first.
        let xml = br#"<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>
 <rdf:Description xmlns:test='http://ns.test.com/'>
  <test:StructList2><rdf:Bag>
   <rdf:li rdf:parseType='Resource'><test:Item1>c1-1</test:Item1><test:Item2>c2-1</test:Item2></rdf:li>
   <rdf:li rdf:parseType='Resource'>
    <test:Item1>c1-2</test:Item1><test:Item2>c2-2</test:Item2>
    <test:TestList2><rdf:Bag><rdf:li>y1</rdf:li><rdf:li>y2</rdf:li></rdf:Bag></test:TestList2>
   </rdf:li>
  </rdf:Bag></test:StructList2>
 </rdf:Description>
</rdf:RDF>"#;
        let tags = extract_list_struct_values(xml).unwrap();
        assert_eq!(
            tags.iter()
                .find(|(tag, _)| tag == "XMP:StructList2Item1")
                .map(|(_, values)| values.as_slice()),
            Some(["c1-1".to_string()].as_slice())
        );
        assert_eq!(
            tags.iter()
                .find(|(tag, _)| tag == "XMP:StructList2Item2")
                .map(|(_, values)| values.as_slice()),
            Some(["c2-1".to_string()].as_slice())
        );
        // XMP4.xmp's second structure owns both values of TestList2; the
        // unknown-schema duplicate rule must not discard its second list item.
        assert_eq!(
            tags.iter()
                .find(|(tag, _)| tag == "XMP:StructList2TestList2")
                .map(|(_, values)| values.as_slice()),
            Some(["y1".to_string(), "y2".to_string()].as_slice())
        );
        assert_eq!(
            parse_xmp_typed(xml)
                .unwrap()
                .iter()
                .find(|(tag, _)| tag == "XMP:StructList2TestList2")
                .map(|(_, value)| value.clone()),
            Some(XmpValue::List(vec!["y1".to_string(), "y2".to_string()]))
        );
    }

    #[test]
    fn unknown_schema_struct_lists_keep_the_first_repeated_field() {
        // `exiftool -G1 -s XMP4.xmp` reports XMP-test:StructList2Item1 as
        // c1-1 and StructList2Item2 as c2-1, despite the second rdf:li
        // carrying c1-2/c2-2.  The test namespace has no registered XMP
        // table, so ExifTool's generic duplicate handling keeps the first.
        let xml = br#"<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>
 <rdf:Description xmlns:test='http://ns.test.com/'>
  <test:StructList2><rdf:Bag>
   <rdf:li rdf:parseType='Resource'><test:Item1>c1-1</test:Item1><test:Item2>c2-1</test:Item2></rdf:li>
   <rdf:li rdf:parseType='Resource'><test:Item1>c1-2</test:Item1><test:Item2>c2-2</test:Item2></rdf:li>
  </rdf:Bag></test:StructList2>
 </rdf:Description>
</rdf:RDF>"#;
        let tags = extract_list_struct_values(xml).unwrap();
        assert_eq!(
            tags.iter()
                .find(|(tag, _)| tag == "XMP:StructList2Item1")
                .map(|(_, value)| value.as_str()),
            Some("c1-1")
        );
        assert_eq!(
            tags.iter()
                .find(|(tag, _)| tag == "XMP:StructList2Item2")
                .map(|(_, value)| value.as_str()),
            Some("c2-1")
        );
    }

    /// A field that is itself a structure keeps concatenating:
    /// `xmpMM:Manifest`/`stMfs:reference`/`stRef:filePath` is
    /// ManifestReferenceFilePath, quoted from `exiftool -G1 -s XMP3.xmp`.
    #[test]
    fn nested_list_struct_fields_concatenate() {
        let xml = br#"<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'
         xmlns:stMfs='http://ns.adobe.com/xap/1.0/sType/ManifestItem#'
         xmlns:stRef='http://ns.adobe.com/xap/1.0/sType/ResourceRef#'
         xmlns:xmpMM='http://ns.adobe.com/xap/1.0/mm/'>
 <rdf:Description>
  <xmpMM:Manifest>
   <rdf:Seq>
    <rdf:li rdf:parseType='Resource'>
     <stMfs:linkForm>EmbedByReference</stMfs:linkForm>
     <stMfs:reference rdf:parseType='Resource'>
      <stRef:filePath>C:\some path\file.ext</stRef:filePath>
     </stMfs:reference>
    </rdf:li>
   </rdf:Seq>
  </xmpMM:Manifest>
 </rdf:Description>
</rdf:RDF>"#;
        let tags = extract_list_struct_values(xml).unwrap();
        assert_eq!(
            tags.iter()
                .find(|(t, _)| t == "XMP:ManifestLinkForm")
                .map(|(_, values)| values.as_slice()),
            Some(["EmbedByReference".to_string()].as_slice())
        );
        assert_eq!(
            tags.iter()
                .find(|(t, _)| t == "XMP:ManifestReferenceFilePath")
                .map(|(_, values)| values.as_slice()),
            Some([r"C:\some path\file.ext".to_string()].as_slice())
        );
    }
}
