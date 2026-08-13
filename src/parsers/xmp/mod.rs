//! XMP (Extensible Metadata Platform) parser
//!
//! Handles RDF/XML parsing and XMP namespace resolution.
//!
//! This module provides functionality to parse XMP metadata from RDF/XML
//! format. It supports:
//! - Namespace resolution for standard XMP namespaces (xmp, dc, exif, etc.)
//! - Extraction of simple string properties
//! - Edit history extraction for forensic tamper detection
//! - Document ID and version tracking metadata
//! - Graceful handling of malformed XML
//!
//! Complex XMP structures (bags, sequences, structs) are currently skipped,
//! except for xmpMM:History which is fully parsed for forensic analysis.
//!
//! # Example
//!
//! ```no_run
//! use oxidex::parsers::xmp::parse_xmp;
//!
//! let xml = br#"
//!     <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
//!              xmlns:xmp="http://ns.adobe.com/xap/1.0/">
//!       <rdf:Description>
//!         <xmp:Creator>John Doe</xmp:Creator>
//!       </rdf:Description>
//!     </rdf:RDF>
//! "#;
//!
//! let result = parse_xmp(xml).unwrap();
//! assert_eq!(result.len(), 1);
//! ```

pub mod google_hdrp;
pub mod history_parser;
pub mod namespace_mapping;
pub mod namespace_resolver;
pub mod plus_vocab;
pub mod rdf_parser;
pub mod struct_flatten;

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::Result;

// Re-export main parsing function for convenience
pub use history_parser::{XmpHistoryEntry, parse_xmp_history};
pub use namespace_mapping::namespace_to_family;
pub use namespace_resolver::NamespaceResolver;
pub use rdf_parser::parse_xmp;

/// Parses a standalone XMP sidecar file.
///
/// This function reads an XMP sidecar file (.xmp) and extracts all metadata.
pub fn parse_xmp_file(reader: &dyn FileReader) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    // A sidecar is XMP whatever it is called. `%fileTypeLookup` answers for
    // the `.xmp` extension, but an RDF-rooted sidecar named `.xml` reaches
    // this parser by content, and the identification layer has already called
    // it TXT or XML by then -- `filetype::identify_text` deliberately declines
    // to claim XMP, leaving the naming to this parser.
    //
    // The values are the ones `SetFileType` produces for the RDF branch, where
    // both its arguments are undefined: the file type falls back to XMP.pm's
    // own, and the MIME type to `$mimeType{XMP}` (XMP.pm:4430).
    metadata.insert("File:FileType", TagValue::new_string("XMP"));
    metadata.insert("File:FileTypeExtension", TagValue::new_string("xmp"));
    metadata.insert("File:MIMEType", TagValue::new_string("application/rdf+xml"));

    // Read the entire XMP file
    let size = reader.size() as usize;
    let xmp_data = reader.read(0, size)?;

    // Preserve the unreduced focal-plane rationals as value forms so the
    // Canon sensor-diagonal composite conversion receives ExifTool's exact
    // numerator/denominator pair rather than its display decimal.
    let (xmp_tags, rational_forms) = rdf_parser::parse_xmp_typed_with_rational_forms(xmp_data)?;

    // Add all XMP tags to metadata and enrich with core XMP tags for Worker 30
    for (key, typed) in xmp_tags {
        let value = typed.clone().into_joined();
        let stored = match typed {
            rdf_parser::XmpValue::List(values) => {
                TagValue::Array(values.into_iter().map(TagValue::new_string).collect())
            }
            rdf_parser::XmpValue::Scalar(value) => TagValue::new_string(value),
        };
        metadata.insert(key.clone(), stored);
        if let Some((_, raw)) = rational_forms.iter().find(|(tag, _)| *tag == key) {
            metadata.set_value_form(key.clone(), raw.clone());
        }

        // Add Worker 30 core XMP tags based on extracted values
        // These tags ensure consistent naming for core XMP properties
        match key.as_str() {
            // XMP:CreatorTool mapping
            "XMP:XMPToolkit" | "XMP:CreatorTool" => {
                if !metadata.contains_key("XMP:CreatorTool") {
                    metadata.insert(
                        "XMP:CreatorTool".to_string(),
                        TagValue::new_string(value.clone()),
                    );
                }
            }
            // XMP:CreationDate mapping
            "XMP:CreateDate" | "XMP:CreationDate" => {
                if !metadata.contains_key("XMP:CreationDate") {
                    metadata.insert(
                        "XMP:CreationDate".to_string(),
                        TagValue::new_string(value.clone()),
                    );
                }
            }
            // XMP:ModificationDate mapping
            "XMP:ModifyDate" | "XMP:ModificationDate" => {
                if !metadata.contains_key("XMP:ModificationDate") {
                    metadata.insert(
                        "XMP:ModificationDate".to_string(),
                        TagValue::new_string(value.clone()),
                    );
                }
            }
            // XMP:Creator mapping
            "XMP:Creator" => {
                // Ensure XMP:Creator is present
                if !metadata.contains_key("XMP:Creator") {
                    metadata.insert(
                        "XMP:Creator".to_string(),
                        TagValue::new_string(value.clone()),
                    );
                }
            }
            // XMP:Subject mapping
            "XMP:Subject" => {
                if !metadata.contains_key("XMP:Subject") {
                    metadata.insert(
                        "XMP:Subject".to_string(),
                        TagValue::new_string(value.clone()),
                    );
                }
            }
            // XMP:Keywords mapping
            "XMP:Keywords" => {
                if !metadata.contains_key("XMP:Keywords") {
                    metadata.insert(
                        "XMP:Keywords".to_string(),
                        TagValue::new_string(value.clone()),
                    );
                }
            }
            // XMP:Description mapping
            "XMP:Description" => {
                if !metadata.contains_key("XMP:Description") {
                    metadata.insert(
                        "XMP:Description".to_string(),
                        TagValue::new_string(value.clone()),
                    );
                }
            }
            // XMP:Rights mapping
            "XMP:Rights" => {
                if !metadata.contains_key("XMP:Rights") {
                    metadata.insert(
                        "XMP:Rights".to_string(),
                        TagValue::new_string(value.clone()),
                    );
                }
            }
            _ => {}
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn xmp_sidecar_carries_unreduced_focal_plane_resolution_into_canon_sensor_diag() {
        let xml = br#"<?xpacket begin="" id=""?>
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
                  xmlns:exif="http://ns.adobe.com/exif/1.0/"
                  tiff:Make="Canon" exif:FocalLength="58/10"
                  exif:FocalPlaneResolutionUnit="2"
                  exif:FocalPlaneXResolution="2272000/224"
                  exif:FocalPlaneYResolution="1704000/168" />
            </rdf:RDF>"#;
        let reader = TestReader::from_slice(xml);
        let mut metadata = parse_xmp_file(&reader).expect("parse XMP");

        assert_eq!(
            metadata.value_form("XMP-exif:FocalPlaneXResolution"),
            Some("2272000/224")
        );
        crate::composite::apply(&mut metadata);
        assert_eq!(
            metadata.get_string("Composite:ScaleFactor35efl"),
            Some("6.1")
        );
    }
}
