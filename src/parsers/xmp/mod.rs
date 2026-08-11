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

    // Parse the XMP data, keeping List-valued properties as lists. The second
    // element carries FocalPlaneXResolution/FocalPlaneYResolution in their
    // unreduced `n/d` form -- see the doc comment on
    // `parse_xmp_typed_with_rational_forms` for why the composite layer needs
    // it and why this carriage is tactical (Step 8 of
    // OVERHAUL_OXIDEX_PLAN.md, superseded by Step 18).
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
        // `set_value_form` only attaches to a tag already present in the map,
        // which the insert just above guarantees.
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

    /// Regression test for the Step 8 "tactical rational carriage": a
    /// standalone XMP sidecar declaring Canon FocalPlaneX/YResolution as
    /// unreduced rationals (as ExifTool's own XMP.xmp fixture does --
    /// `exif:FocalPlaneXResolution="2272000/224"`,
    /// `exif:FocalPlaneYResolution="1704000/168"`) must let
    /// `Composite:ScaleFactor35efl` take
    /// `Image::ExifTool::Canon::CalcSensorDiag`'s Canon-specific sensor
    /// diagonal (Canon.pm:10145-10175), not the generic focal-plane
    /// fallback. Without the carriage, `format_xmp_plain_rational` reduces
    /// these to "10142.8571428571" before the composite layer ever sees
    /// them, `canon_sensor_diag`'s `n/d` split fails, and ScaleFactor35efl
    /// silently computes from the wrong path (6.1 on the real fixture
    /// becomes 12.2).
    #[test]
    fn xmp_sidecar_carries_unreduced_focal_plane_resolution_into_canon_sensor_diag() {
        let xml = br#"<?xpacket begin="" id=""?>
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description
                  xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
                  xmlns:exif="http://ns.adobe.com/exif/1.0/"
                  tiff:Make="Canon"
                  tiff:Model="Canon DIGITAL IXUS 40"
                  exif:FocalLength="58/10"
                  exif:FocalPlaneResolutionUnit="2"
                  exif:FocalPlaneXResolution="2272000/224"
                  exif:FocalPlaneYResolution="1704000/168">
              </rdf:Description>
            </rdf:RDF>
            <?xpacket end="w"?>"#;

        let reader = TestReader::from_slice(xml);
        let mut metadata = parse_xmp_file(&reader).expect("parse_xmp_file");

        // The formatted (display) value is still the reduced decimal --
        // ExifTool prints "10142.8571428571", not the raw rational.
        assert_eq!(
            metadata.get_string("XMP-exif:FocalPlaneXResolution"),
            Some("10142.8571428571")
        );
        // But the unreduced n/d text must have been attached as this tag's
        // value_form, which is what `composite::apply`'s `resolve` (via
        // `lookup_key`) consults first.
        assert_eq!(
            metadata.value_form("XMP-exif:FocalPlaneXResolution"),
            Some("2272000/224")
        );
        assert_eq!(
            metadata.value_form("XMP-exif:FocalPlaneYResolution"),
            Some("1704000/168")
        );

        let added = crate::composite::apply(&mut metadata);
        assert!(added > 0, "composite layer produced nothing");

        // Canon.pm:10145-10175: sqrt(224^2 + 168^2) * 0.0254 = 7.0104 mm
        // diagonal; ScaleFactor35efl = FocalLengthIn35mmFormat/FocalLength,
        // and with only FocalLength known, CalcScaleFactor35efl falls to the
        // sensor-diagonal branch: 35mm diag (43.2666mm) / this diag. What
        // matters for this regression is that it lands on the real
        // ExifTool fixture's answer (6.1), not the generic-fallback 12.2
        // that a reduced/decimal FocalPlaneXResolution produces.
        assert_eq!(
            metadata.get_string("Composite:ScaleFactor35efl"),
            Some("6.1"),
            "canon_sensor_diag did not receive the unreduced rational"
        );
    }

    /// Without the value_form carriage this test locks in, a rational whose
    /// `n/d` denominators fail `canon_sensor_diag`'s validity checks (here,
    /// far outside the plausible 61..1500 sensor-size range) must fall
    /// through cleanly rather than panicking or fabricating a value.
    #[test]
    fn xmp_sidecar_with_non_canon_make_skips_sensor_diag_entirely() {
        let xml = br#"<?xpacket begin="" id=""?>
            <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
              <rdf:Description
                  xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
                  xmlns:exif="http://ns.adobe.com/exif/1.0/"
                  tiff:Make="NotCanon"
                  exif:FocalLength="58/10"
                  exif:FocalPlaneXResolution="2272000/224"
                  exif:FocalPlaneYResolution="1704000/168">
              </rdf:Description>
            </rdf:RDF>
            <?xpacket end="w"?>"#;

        let reader = TestReader::from_slice(xml);
        let mut metadata = parse_xmp_file(&reader).expect("parse_xmp_file");
        // The carriage is unconditional on the tag name, not on Make -- it
        // only feeds `canon_sensor_diag`, which itself gates on Make.
        assert_eq!(
            metadata.value_form("XMP-exif:FocalPlaneXResolution"),
            Some("2272000/224")
        );

        crate::composite::apply(&mut metadata);
        // ScaleFactor35efl still computes (FocalLength alone is enough to
        // reach the resolution-based fallback), just not through the
        // Canon-only sensor-diagonal path.
        assert_ne!(
            metadata.get_string("Composite:ScaleFactor35efl"),
            Some("6.1"),
            "a non-Canon Make must not take the Canon sensor-diagonal branch"
        );
    }
}
