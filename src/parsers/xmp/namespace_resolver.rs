//! XMP namespace handling
//!
//! This module handles XMP namespace resolution and mapping between
//! namespace prefixes (e.g., "xmp", "dc") and their full URIs.
//!
//! # Standard XMP Namespaces
//!
//! The XMP specification defines several standard namespaces:
//! - `xmp:` → http://ns.adobe.com/xap/1.0/ (Core XMP properties)
//! - `dc:` → http://purl.org/dc/elements/1.1/ (Dublin Core)
//! - `exif:` → http://ns.adobe.com/exif/1.0/ (EXIF properties)
//! - `tiff:` → http://ns.adobe.com/tiff/1.0/ (TIFF properties)
//! - `photoshop:` → http://ns.adobe.com/photoshop/1.0/ (Photoshop metadata)
//! - `xmpRights:` → http://ns.adobe.com/xap/1.0/rights/ (Rights management)
//! - `Iptc4xmpCore:` → http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/ (IPTC Core metadata)
//! - `Iptc4xmpExt:` → http://iptc.org/std/Iptc4xmpExt/2008-02-29/ (IPTC Extension metadata)
//! - `plus:` → http://ns.useplus.org/ldf/xmp/1.0/ (PLUS image licensing)
//!
//! # Example
//!
//! ```no_run
//! use oxidex::parsers::xmp::namespace_resolver::NamespaceResolver;
//!
//! let mut resolver = NamespaceResolver::new();
//! resolver.register_namespace("xmp", "http://ns.adobe.com/xap/1.0/");
//!
//! assert_eq!(resolver.resolve_prefix("xmp"), Some("http://ns.adobe.com/xap/1.0/"));
//! assert_eq!(resolver.resolve_uri("http://ns.adobe.com/xap/1.0/"), Some("xmp"));
//! ```

use std::collections::HashMap;

/// Manages XMP namespace prefix-to-URI mappings.
///
/// This resolver maintains bidirectional mappings between namespace prefixes
/// (e.g., "xmp", "dc") and their full URIs. It allows looking up URIs from
/// prefixes and vice versa during XML parsing.
#[derive(Debug, Clone)]
pub struct NamespaceResolver {
    /// Maps prefix to URI (e.g., "xmp" → "http://ns.adobe.com/xap/1.0/")
    prefix_to_uri: HashMap<String, String>,
    /// Maps URI to prefix (e.g., "http://ns.adobe.com/xap/1.0/" → "xmp")
    uri_to_prefix: HashMap<String, String>,
}

impl NamespaceResolver {
    /// Creates a new namespace resolver with standard XMP namespaces pre-registered.
    ///
    /// The following namespaces are registered by default:
    /// - `xmp:` (Core XMP properties)
    /// - `dc:` (Dublin Core)
    /// - `exif:` (EXIF properties)
    /// - `tiff:` (TIFF properties)
    /// - `photoshop:` (Photoshop metadata)
    /// - `xmpRights:` (Rights management)
    /// - `Iptc4xmpCore:` (IPTC Core metadata)
    /// - `Iptc4xmpExt:` (IPTC Extension metadata)
    /// - `plus:` (PLUS image licensing)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oxidex::parsers::xmp::namespace_resolver::NamespaceResolver;
    ///
    /// let resolver = NamespaceResolver::new();
    /// assert_eq!(resolver.resolve_prefix("xmp"), Some("http://ns.adobe.com/xap/1.0/"));
    /// ```
    pub fn new() -> Self {
        let mut resolver = Self {
            prefix_to_uri: HashMap::new(),
            uri_to_prefix: HashMap::new(),
        };

        // Register standard XMP namespaces
        resolver.register_namespace("xmp", "http://ns.adobe.com/xap/1.0/");
        resolver.register_namespace("dc", "http://purl.org/dc/elements/1.1/");
        resolver.register_namespace("exif", "http://ns.adobe.com/exif/1.0/");
        resolver.register_namespace("tiff", "http://ns.adobe.com/tiff/1.0/");
        resolver.register_namespace("photoshop", "http://ns.adobe.com/photoshop/1.0/");
        resolver.register_namespace("xmpRights", "http://ns.adobe.com/xap/1.0/rights/");
        resolver.register_namespace("rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#");

        // Register IPTC namespaces for professional workflow metadata
        resolver.register_namespace(
            "Iptc4xmpCore",
            "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/",
        );
        resolver.register_namespace("Iptc4xmpExt", "http://iptc.org/std/Iptc4xmpExt/2008-02-29/");

        // Register PLUS namespace for image licensing
        resolver.register_namespace("plus", "http://ns.useplus.org/ldf/xmp/1.0/");

        resolver
    }

    /// Registers a new namespace mapping.
    ///
    /// This adds a bidirectional mapping between a prefix and its full URI.
    /// If the prefix or URI already exists, the mapping will be overwritten.
    ///
    /// # Parameters
    ///
    /// - `prefix`: The short prefix (e.g., "xmp")
    /// - `uri`: The full namespace URI (e.g., "http://ns.adobe.com/xap/1.0/")
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oxidex::parsers::xmp::namespace_resolver::NamespaceResolver;
    ///
    /// let mut resolver = NamespaceResolver::new();
    /// resolver.register_namespace("custom", "http://example.com/ns/custom/");
    /// ```
    pub fn register_namespace(&mut self, prefix: &str, uri: &str) {
        self.prefix_to_uri
            .insert(prefix.to_string(), uri.to_string());
        self.uri_to_prefix
            .insert(uri.to_string(), prefix.to_string());
    }

    /// Resolves a namespace prefix to its full URI.
    ///
    /// # Parameters
    ///
    /// - `prefix`: The prefix to resolve (e.g., "xmp")
    ///
    /// # Returns
    ///
    /// The full URI if the prefix is registered, or `None` if unknown.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oxidex::parsers::xmp::namespace_resolver::NamespaceResolver;
    ///
    /// let resolver = NamespaceResolver::new();
    /// assert_eq!(resolver.resolve_prefix("xmp"), Some("http://ns.adobe.com/xap/1.0/"));
    /// assert_eq!(resolver.resolve_prefix("unknown"), None);
    /// ```
    pub fn resolve_prefix(&self, prefix: &str) -> Option<&str> {
        self.prefix_to_uri.get(prefix).map(|s| s.as_str())
    }

    /// Resolves a namespace URI to its prefix.
    ///
    /// # Parameters
    ///
    /// - `uri`: The URI to resolve (e.g., "http://ns.adobe.com/xap/1.0/")
    ///
    /// # Returns
    ///
    /// The prefix if the URI is registered, or `None` if unknown.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oxidex::parsers::xmp::namespace_resolver::NamespaceResolver;
    ///
    /// let resolver = NamespaceResolver::new();
    /// assert_eq!(resolver.resolve_uri("http://ns.adobe.com/xap/1.0/"), Some("xmp"));
    /// ```
    pub fn resolve_uri(&self, uri: &str) -> Option<&str> {
        self.uri_to_prefix.get(uri).map(|s| s.as_str())
    }

    /// Extracts the namespace prefix from a qualified name (QName).
    ///
    /// A QName has the format "prefix:localname" (e.g., "xmp:Creator").
    /// This method returns the prefix part before the colon.
    ///
    /// # Parameters
    ///
    /// - `qname`: A qualified name (e.g., "xmp:Creator")
    ///
    /// # Returns
    ///
    /// The prefix if a colon is present, or `None` if no namespace prefix.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oxidex::parsers::xmp::namespace_resolver::NamespaceResolver;
    ///
    /// assert_eq!(NamespaceResolver::extract_prefix("xmp:Creator"), Some("xmp"));
    /// assert_eq!(NamespaceResolver::extract_prefix("Creator"), None);
    /// ```
    pub fn extract_prefix(qname: &str) -> Option<&str> {
        qname.split(':').next().filter(|&_p| qname.contains(':'))
    }

    /// Extracts the local name from a qualified name (QName).
    ///
    /// A QName has the format "prefix:localname" (e.g., "xmp:Creator").
    /// This method returns the local name part after the colon.
    ///
    /// # Parameters
    ///
    /// - `qname`: A qualified name (e.g., "xmp:Creator")
    ///
    /// # Returns
    ///
    /// The local name, or the entire string if no colon is present.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oxidex::parsers::xmp::namespace_resolver::NamespaceResolver;
    ///
    /// assert_eq!(NamespaceResolver::extract_local_name("xmp:Creator"), "Creator");
    /// assert_eq!(NamespaceResolver::extract_local_name("Creator"), "Creator");
    /// ```
    pub fn extract_local_name(qname: &str) -> &str {
        qname.split(':').next_back().unwrap_or(qname)
    }
}

impl Default for NamespaceResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_registers_standard_namespaces() {
        let resolver = NamespaceResolver::new();

        // Verify standard namespaces are registered
        assert_eq!(
            resolver.resolve_prefix("xmp"),
            Some("http://ns.adobe.com/xap/1.0/")
        );
        assert_eq!(
            resolver.resolve_prefix("dc"),
            Some("http://purl.org/dc/elements/1.1/")
        );
        assert_eq!(
            resolver.resolve_prefix("exif"),
            Some("http://ns.adobe.com/exif/1.0/")
        );
        assert_eq!(
            resolver.resolve_prefix("tiff"),
            Some("http://ns.adobe.com/tiff/1.0/")
        );
        assert_eq!(
            resolver.resolve_prefix("photoshop"),
            Some("http://ns.adobe.com/photoshop/1.0/")
        );
        assert_eq!(
            resolver.resolve_prefix("xmpRights"),
            Some("http://ns.adobe.com/xap/1.0/rights/")
        );
    }

    #[test]
    fn test_iptc_namespaces_registered() {
        let resolver = NamespaceResolver::new();

        // Verify IPTC namespaces are registered
        assert_eq!(
            resolver.resolve_prefix("Iptc4xmpCore"),
            Some("http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/")
        );
        assert_eq!(
            resolver.resolve_prefix("Iptc4xmpExt"),
            Some("http://iptc.org/std/Iptc4xmpExt/2008-02-29/")
        );
    }

    #[test]
    fn test_plus_namespace_registered() {
        let resolver = NamespaceResolver::new();

        // Verify PLUS namespace is registered
        assert_eq!(
            resolver.resolve_prefix("plus"),
            Some("http://ns.useplus.org/ldf/xmp/1.0/")
        );
    }

    #[test]
    fn test_register_namespace() {
        let mut resolver = NamespaceResolver::new();

        resolver.register_namespace("custom", "http://example.com/custom/");

        assert_eq!(
            resolver.resolve_prefix("custom"),
            Some("http://example.com/custom/")
        );
        assert_eq!(
            resolver.resolve_uri("http://example.com/custom/"),
            Some("custom")
        );
    }

    #[test]
    fn test_resolve_prefix() {
        let resolver = NamespaceResolver::new();

        assert_eq!(
            resolver.resolve_prefix("xmp"),
            Some("http://ns.adobe.com/xap/1.0/")
        );
        assert_eq!(resolver.resolve_prefix("unknown"), None);
    }

    #[test]
    fn test_resolve_uri() {
        let resolver = NamespaceResolver::new();

        assert_eq!(
            resolver.resolve_uri("http://ns.adobe.com/xap/1.0/"),
            Some("xmp")
        );
        assert_eq!(resolver.resolve_uri("http://unknown.com/"), None);
    }

    #[test]
    fn test_extract_prefix() {
        assert_eq!(
            NamespaceResolver::extract_prefix("xmp:Creator"),
            Some("xmp")
        );
        assert_eq!(NamespaceResolver::extract_prefix("dc:title"), Some("dc"));
        assert_eq!(NamespaceResolver::extract_prefix("Creator"), None);
        assert_eq!(NamespaceResolver::extract_prefix(""), None);
    }

    #[test]
    fn test_extract_local_name() {
        assert_eq!(
            NamespaceResolver::extract_local_name("xmp:Creator"),
            "Creator"
        );
        assert_eq!(NamespaceResolver::extract_local_name("dc:title"), "title");
        assert_eq!(NamespaceResolver::extract_local_name("Creator"), "Creator");
        assert_eq!(NamespaceResolver::extract_local_name(""), "");
    }

    #[test]
    fn test_bidirectional_mapping() {
        let mut resolver = NamespaceResolver::new();

        resolver.register_namespace("test", "http://test.com/");

        // Forward lookup
        let uri = resolver.resolve_prefix("test").unwrap();
        assert_eq!(uri, "http://test.com/");

        // Reverse lookup
        let prefix = resolver.resolve_uri(uri).unwrap();
        assert_eq!(prefix, "test");
    }

    #[test]
    fn test_overwrite_mapping() {
        let mut resolver = NamespaceResolver::new();

        resolver.register_namespace("test", "http://test1.com/");
        assert_eq!(resolver.resolve_prefix("test"), Some("http://test1.com/"));

        // Overwrite with new URI
        resolver.register_namespace("test", "http://test2.com/");
        assert_eq!(resolver.resolve_prefix("test"), Some("http://test2.com/"));
    }
}
