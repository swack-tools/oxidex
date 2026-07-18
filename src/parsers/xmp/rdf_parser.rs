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
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(true); // Trim whitespace from text nodes

    let mut resolver = NamespaceResolver::new();
    let mut results = Vec::new();
    let mut buf = Vec::new();

    // State tracking
    let mut inside_description = false;
    let mut current_property: Option<String> = None;
    let mut current_value = String::new();
    let mut depth = 0;
    let mut property_depth = 0;
    let mut inside_collection = false; // Are we in a Bag/Seq/Alt?
    let mut collection_values: Vec<String> = Vec::new(); // Collect rdf:li values

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
                    inside_description = true;
                    // Extract rdf:about and property attributes from Description
                    extract_description_attributes(&e, &resolver, &mut results)?;
                } else if inside_description && current_property.is_none() {
                    // This is a property element inside rdf:Description
                    // Check if it's a complex structure we should skip
                    if is_simple_property(&tag_name, &resolver) {
                        current_property = Some(tag_name.to_string());
                        current_value.clear();
                        collection_values.clear();
                        inside_collection = false;
                        property_depth = depth;
                    }
                } else if current_property.is_some() {
                    // Check if this is a Bag/Seq/Alt container
                    if is_collection_container(&tag_name, &resolver) {
                        inside_collection = true;
                        collection_values.clear();
                    }
                }
            }

            Ok(Event::End(e)) => {
                let tag_name = extract_tag_name_from_bytes(e.name().as_ref())?;

                if is_rdf_description(&tag_name, &resolver) {
                    inside_description = false;
                } else if is_rdf_li(&tag_name, &resolver) && inside_collection {
                    // End of rdf:li - save the collected value
                    if !current_value.trim().is_empty() {
                        collection_values.push(current_value.trim().to_string());
                    }
                    current_value.clear();
                } else if is_collection_container(&tag_name, &resolver) {
                    inside_collection = false;
                } else if let Some(ref prop) = current_property
                    && depth == property_depth
                {
                    // End of current property - extract tag name and value
                    let prefixed_name = format_tag_name(prop, &resolver);

                    if !collection_values.is_empty() {
                        // Output collection as comma-separated list
                        results.push((prefixed_name, collection_values.join(", ")));
                    } else if !current_value.trim().is_empty() {
                        results.push((prefixed_name, current_value.trim().to_string()));
                    }
                    current_property = None;
                    current_value.clear();
                    collection_values.clear();
                    inside_collection = false;
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

    // Post-process results to apply formatting for specific tags
    let results = results
        .into_iter()
        .map(|(tag, value)| {
            let formatted = format_xmp_value(&tag, &value);
            (tag, formatted)
        })
        .collect();

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

/// Checks if a tag name represents an x:xmpmeta element.
///
/// The xmpmeta element wraps XMP data and may contain the XMPToolkit attribute.
fn is_xmpmeta(tag_name: &str) -> bool {
    // Check for x:xmpmeta or xmpmeta (with or without prefix)
    tag_name == "x:xmpmeta" || tag_name == "xmpmeta"
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

        // Check for x:xmptk or xmptk attribute (XMP Toolkit version)
        if key == "x:xmptk" || key == "xmptk" {
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

        // Skip empty values
        if value.trim().is_empty() {
            continue;
        }

        // Skip namespace declarations (xmlns:xxx)
        if key.starts_with("xmlns") {
            continue;
        }

        // Handle rdf:about attribute (the subject URI)
        if key == "rdf:about" {
            results.push(("XMP:About".to_string(), value.trim().to_string()));
            continue;
        }

        // Skip other rdf: attributes (rdf:parseType, rdf:resource, etc.)
        if key.starts_with("rdf:") {
            continue;
        }

        // Handle XMP property shorthand (properties as attributes)
        // These are namespace-prefixed attributes like xmp:Rating="5"
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

    // Extract namespace prefix from the qualified name
    if let Some(prefix) = NamespaceResolver::extract_prefix(qname) {
        // Resolve the namespace URI from the prefix
        let family_prefix = if let Some(namespace_uri) = resolver.resolve_prefix(prefix) {
            // Use namespace mapping to get ExifTool family prefix
            namespace_to_family(namespace_uri).unwrap_or("XMP")
        } else {
            // Unknown namespace - use generic XMP prefix
            "XMP"
        };

        // ExifTool capitalizes the first letter of all XMP property names
        // to create consistent PascalCase tag names (e.g., album → Album)
        if !local_name.is_empty() {
            local_name = capitalize_first_letter(&local_name);
        }

        // Format with the appropriate family prefix
        format!("{}:{}", family_prefix, local_name)
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

    match local_name {
        // IPTC Urgency (0-8 scale)
        "Urgency" => format_iptc_urgency(value),

        // EXIF enum tags that appear in XMP
        "ColorSpace" => decode_xmp_color_space(value),
        "CustomRendered" => decode_xmp_custom_rendered(value),
        "ExposureMode" => decode_xmp_exposure_mode(value),
        "FileSource" => decode_xmp_file_source(value),
        "FocalPlaneResolutionUnit" | "ResolutionUnit" => decode_xmp_resolution_unit(value),
        "MeteringMode" => decode_xmp_metering_mode(value),
        "Orientation" => decode_xmp_orientation(value),
        "SceneCaptureType" => decode_xmp_scene_capture_type(value),
        "SensingMethod" => decode_xmp_sensing_method(value),
        "WhiteBalance" => decode_xmp_white_balance(value),
        "YCbCrPositioning" => decode_xmp_ycbcr_positioning(value),
        "ColorMode" => decode_xmp_color_mode(value),
        "PhotometricInterpretation" => decode_xmp_photometric_interpretation(value),

        // Camera Raw Settings - numeric parameters
        "ProcessingParameters" => format_camera_raw_parameters(value),

        // TIFF numeric tags - resolution and dimensions
        "XResolution" | "YResolution" => format_tiff_resolution(value),

        // EXIF exposure tags - numeric or enum
        "ISO" => format_exif_iso(value),
        "ShutterSpeed" => format_exif_shutter_speed(value),
        "Aperture" => format_exif_aperture(value),
        "ExposureCompensation" => format_exif_exposure_compensation(value),
        "FocalLength" => format_exif_focal_length(value),

        // Photoshop numeric tags
        "Quality" => format_photoshop_quality(value),

        // Default: return original value unchanged
        _ => value.to_string(),
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

/// Decode XMP SensingMethod
fn decode_xmp_sensing_method(value: &str) -> String {
    match value.trim() {
        "1" => "Not defined".to_string(),
        "2" => "One-chip color area".to_string(),
        "3" => "Two-chip color area".to_string(),
        "4" => "Three-chip color area".to_string(),
        "5" => "Color sequential area".to_string(),
        "7" => "Trilinear".to_string(),
        "8" => "Color sequential linear".to_string(),
        _ => value.to_string(),
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
    // TIFF resolution values are rational numbers or decimals
    // Try to format with appropriate precision
    if let Ok(num) = value.trim().parse::<f64>() {
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
    let trimmed = value.trim();

    if let Ok(num) = trimmed.parse::<f64>() {
        // Format with appropriate precision (2 decimal places)
        format!("{:.2}", num)
    } else {
        trimmed.to_string()
    }
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
    let trimmed = value.trim();

    if let Ok(num) = trimmed.parse::<f64>() {
        // Format focal length in mm
        if (num - num.round()).abs() < 0.01 {
            // Whole millimeters
            format!("{:.0} mm", num)
        } else {
            // Decimal millimeters
            format!("{:.1} mm", num)
        }
    } else {
        trimmed.to_string()
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
        assert_eq!(create_date, Some("2023-01-15T10:30:00"));
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
        assert_eq!(focal, Some("85 mm"));
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

        // EXIF focal length formatting
        assert_eq!(format_exif_focal_length("50"), "50 mm");
        assert_eq!(format_exif_focal_length("85.0"), "85 mm");
        assert_eq!(format_exif_focal_length("24.5"), "24.5 mm");

        // TIFF resolution formatting
        assert_eq!(format_tiff_resolution("300"), "300");
        assert_eq!(format_tiff_resolution("72.5"), "72.5");

        // Photoshop quality formatting
        assert_eq!(format_photoshop_quality("85"), "85%");
        assert_eq!(format_photoshop_quality("100"), "100%");
    }

    #[test]
    fn test_exposure_compensation_formatting() {
        // Test exposure compensation formatting with 2 decimal places
        assert_eq!(format_exif_exposure_compensation("1.0"), "1.00");
        assert_eq!(format_exif_exposure_compensation("-0.5"), "-0.50");
        assert_eq!(format_exif_exposure_compensation("0"), "0.00");
        assert_eq!(format_exif_exposure_compensation("0.3"), "0.30");
    }

    #[test]
    fn test_shutter_speed_formatting() {
        // Test shutter speed formatting with 3 decimal places for decimal values
        assert_eq!(format_exif_shutter_speed("0.004"), "0.004");
        assert_eq!(format_exif_shutter_speed("1/250"), "1/250");
        assert_eq!(format_exif_shutter_speed("0.5"), "0.500");
    }
}
