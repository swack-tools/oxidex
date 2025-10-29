//! XMP (Extensible Metadata Platform) parser
//!
//! Handles RDF/XML parsing and XMP namespace resolution.
//!
//! This module provides functionality to parse XMP metadata from RDF/XML
//! format. It supports:
//! - Namespace resolution for standard XMP namespaces (xmp, dc, exif, etc.)
//! - Extraction of simple string properties
//! - Graceful handling of malformed XML
//!
//! Complex XMP structures (bags, sequences, structs) are currently skipped.
//!
//! # Example
//!
//! ```no_run
//! use exiftool_rs::parsers::xmp::parse_xmp;
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

pub mod namespace_resolver;
pub mod rdf_parser;

// Re-export main parsing function for convenience
pub use rdf_parser::parse_xmp;
pub use namespace_resolver::NamespaceResolver;
