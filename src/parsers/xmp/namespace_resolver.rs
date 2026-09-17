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

use super::generated_namespaces::{URI_PREFIXES, standard_prefix, translated_prefix};

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
    /// ExifTool's per-packet `$$et{curNS}`: non-standard namespace URI ->
    /// the prefix ExifTool settled on for it (XMP.pm:3939-3955).
    cur_ns: HashMap<String, String>,
    /// ExifTool's per-packet `$$et{curURI}`: that prefix -> its URI.
    cur_uri: HashMap<String, String>,
    /// Declared URI -> effective ExifTool namespace prefix (before
    /// `%stdXlatNS`), fixed the first time the URI is declared.
    effective_prefix: HashMap<String, String>,
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
            cur_ns: HashMap::new(),
            cur_uri: HashMap::new(),
            effective_prefix: HashMap::new(),
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
        // `xmlns="..."` has no colon, and XMP.pm only handles `xmlns:PREFIX`
        // declarations (`if ($attr =~ /(.*?):/)`), so a default namespace
        // never takes part in prefix taming.
        if !prefix.is_empty() {
            self.tame_prefix(prefix, uri);
        }
    }

    /// ExifTool's namespace-prefix taming for one `xmlns:PREFIX="URI"`
    /// declaration (XMP.pm:3901-3955), recorded per URI.
    ///
    /// A resolver lives for one XMP packet, like ExifTool's `curNS`/`curURI`
    /// (reset per packet in `ProcessXMP`, XMP.pm:4277-4278), and every parse
    /// pass registers declarations in document order, so every pass arrives
    /// at the same prefix for the same URI.
    fn tame_prefix(&mut self, prefix: &str, uri: &str) {
        if let Some(std_ns) = standard_prefix_for_declared_uri(uri) {
            // "use standard namespace prefix if pre-defined"
            self.effective_prefix
                .insert(uri.to_string(), std_ns.to_string());
            return;
        }
        if let Some(used) = self.cur_ns.get(uri) {
            // "use a consistent prefix over the entire XMP for a given
            // namespace URI"
            let used = used.clone();
            self.effective_prefix.insert(uri.to_string(), used);
            return;
        }
        // "use unique prefixes for all namespaces across the entire XMP":
        // a prefix already bound to another non-standard URI in this packet,
        // or one of ExifTool's own standard prefixes (`$nsURI{$ns}`), is
        // replaced by the lowest free `tmpN`.
        let mut used = prefix.to_string();
        if self.cur_uri.contains_key(prefix) || is_standard_prefix(prefix) {
            let mut index = 0usize;
            while self.cur_uri.contains_key(&format!("tmp{index}")) {
                index += 1;
            }
            used = format!("tmp{index}");
        }
        self.cur_ns.insert(uri.to_string(), used.clone());
        self.cur_uri.insert(used.clone(), uri.to_string());
        self.effective_prefix.insert(uri.to_string(), used);
    }

    /// The ExifTool family-1 group for a property written with `prefix`:
    /// `XMP-<prefix>`, where the prefix is the one ExifTool tamed the bound
    /// URI to and then passed through `%stdXlatNS` (XMP.pm FoundXMP:
    /// `$ns = $stdXlatNS{$ns} if $stdXlatNS{$ns}` ... `SetGroup($key,
    /// "$$tagTablePtr{GROUPS}{0}-$ns")`).
    ///
    /// An empty prefix has no namespace, and ExifTool calls no `SetGroup` for
    /// it, so the group stays plain `XMP`. A prefix with no declaration in
    /// scope keeps its own spelling, as ExifTool's does.
    ///
    /// Not modelled here (the key is left as plain `XMP`):
    /// - URIs under `http://ns.exiftool.org/<g0>/<g1>/` (ExifTool's own `-X`
    ///   output), whose tags ExifTool files under the family-0/1 groups named
    ///   in the URI (`StaticGroup1`, XMP.pm:3599-3615) -- groups outside the
    ///   `XMP` family entirely.
    /// - ExifTool's clean-up of prefixes containing characters outside
    ///   `[-.0-9A-Z_a-z\x80-\xff]` (XMP.pm:3480-3486); a well-formed XML
    ///   prefix cannot contain them.
    /// - `TABLE_NAMESPACES` (Google `Device`, PhotoMechanic): ExifTool only
    ///   learns these URIs once their tag table loads, so at declaration time
    ///   they are not standard and follow the document-prefix rules above.
    ///   PhotoMechanic.jpg bears this out (`photomechanic` -> `XMP-photomech`
    ///   through `%stdXlatNS`, not through a URI lookup).
    pub fn group_for_prefix(&self, prefix: &str) -> String {
        if prefix.is_empty() {
            return "XMP".to_string();
        }
        let effective = match self.resolve_prefix(prefix) {
            Some(uri) if is_exiftool_static_group_uri(uri) => return "XMP".to_string(),
            Some(uri) => self
                .effective_prefix
                .get(uri)
                .map(String::as_str)
                .or_else(|| standard_prefix_for_declared_uri(uri))
                .unwrap_or(prefix),
            None => prefix,
        };
        format!("XMP-{}", translated_prefix(effective))
    }

    /// [`Self::group_for_prefix`] for a qualified name (`dc:title`).
    pub fn group_for_qname(&self, qname: &str) -> String {
        self.group_for_prefix(Self::extract_prefix(qname).unwrap_or(""))
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

/// Whether `prefix` is one of ExifTool's standard prefixes (a key of
/// `%nsURI`).
fn is_standard_prefix(prefix: &str) -> bool {
    URI_PREFIXES.iter().any(|(_, known)| *known == prefix)
}

/// ExifTool's own `-X` namespaces, which carry their groups in the URI
/// (XMP.pm:3599: `m{^http://ns.exiftool.(?:ca|org)/(.*?)/(.*?)/}`).
fn is_exiftool_static_group_uri(uri: &str) -> bool {
    let rest = uri
        .strip_prefix("http://ns.exiftool.org/")
        .or_else(|| uri.strip_prefix("http://ns.exiftool.ca/"));
    rest.is_some_and(|rest| {
        let mut parts = rest.splitn(3, '/');
        matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(_), Some(_), Some(_))
        )
    })
}

/// The standard prefix XMP.pm's `xmlns` handling finds for a declared URI
/// (XMP.pm:3905-3926): the URI itself, then the URI with its trailing `/`
/// toggled, then the same URI with a different `N.N` version in its first
/// `/N.N/` (or trailing `/N.N`) segment.
fn standard_prefix_for_declared_uri(uri: &str) -> Option<&'static str> {
    if let Some(prefix) = standard_prefix(uri) {
        return Some(prefix);
    }
    let toggled = match uri.strip_suffix('/') {
        Some(stripped) => stripped.to_string(),
        None => format!("{uri}/"),
    };
    if let Some(prefix) = standard_prefix(&toggled) {
        return Some(prefix);
    }
    let (head, tail) = split_version_segment(uri)?;
    // `grep /^$try$/, keys %uri2ns` takes whichever match hash order yields
    // first; the table is sorted, so take the first sorted match.
    URI_PREFIXES.iter().find_map(|(known, prefix)| {
        let (known_head, known_tail) = split_version_segment(known)?;
        (known_head == head && known_tail == tail).then_some(*prefix)
    })
}

/// Splits `uri` around its first `/<digits>.<digits>` segment that is
/// followed by `/` or the end of the string, returning the text before the
/// version (including the `/`) and the text after it.
fn split_version_segment(uri: &str) -> Option<(&str, &str)> {
    let bytes = uri.as_bytes();
    let mut start = 0usize;
    while let Some(offset) = uri[start..].find('/') {
        let slash = start + offset;
        let mut i = slash + 1;
        let major = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i > major && i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            let minor = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i > minor && (i == bytes.len() || bytes[i] == b'/') {
                return Some((&uri[..=slash], &uri[i..]));
            }
        }
        start = slash + 1;
    }
    None
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

    /// Registers `(prefix, uri)` declarations in document order and returns
    /// the group for each `prefix` right after its own declaration.
    fn groups_after(declarations: &[(&str, &str)]) -> Vec<String> {
        let mut resolver = NamespaceResolver::new();
        declarations
            .iter()
            .map(|(prefix, uri)| {
                resolver.register_namespace(prefix, uri);
                resolver.group_for_prefix(prefix)
            })
            .collect()
    }

    /// Pinned ExifTool 13.59 (`exiftool -a -G1 -s`) on a packet declaring
    /// these namespaces in this order reports exactly these groups:
    /// XMP-photoshop, XMP-dc, XMP-tmp0, XMP-aaa, XMP-iptcCore,
    /// XMP-microsoft, then (second rdf:Description) XMP-aaa, XMP-tmp1.
    #[test]
    fn group_prefixes_follow_exiftool_namespace_taming() {
        let groups = groups_after(&[
            // A different N.N version of a standard URI.
            ("ps", "http://ns.adobe.com/photoshop/2.5/"),
            // A standard URI missing its trailing slash.
            ("dcx", "http://purl.org/dc/elements/1.1"),
            // A standard prefix bound to a non-standard URI.
            ("dc", "http://example.com/notdc/"),
            ("aaa", "http://example.com/aaa/"),
            // %stdXlatNS applies after the URI lookup.
            ("core", "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/"),
            // Only `MicrosoftPhoto` translates; this is an ordinary prefix.
            ("microsoft", "http://example.com/ms/"),
            // A URI already seen keeps its first prefix.
            ("bbb", "http://example.com/aaa/"),
            // A prefix already used for another URI gets the next tmpN.
            ("aaa", "http://example.com/other/"),
        ]);
        assert_eq!(
            groups,
            [
                "XMP-photoshop",
                "XMP-dc",
                "XMP-tmp0",
                "XMP-aaa",
                "XMP-iptcCore",
                "XMP-microsoft",
                "XMP-aaa",
                "XMP-tmp1",
            ]
        );
    }

    #[test]
    fn unprefixed_undeclared_and_static_group_namespaces() {
        let mut resolver = NamespaceResolver::new();
        // No namespace: ExifTool calls no SetGroup, so the group stays XMP.
        assert_eq!(resolver.group_for_prefix(""), "XMP");
        // An undeclared prefix keeps its own spelling.
        assert_eq!(resolver.group_for_prefix("undeclared"), "XMP-undeclared");
        // ExifTool's own -X namespaces carry non-XMP groups; not modelled.
        resolver.register_namespace("IFD0", "http://ns.exiftool.org/EXIF/IFD0/1.0/");
        assert_eq!(resolver.group_for_prefix("IFD0"), "XMP");
        // ...but ExifTool's plain `et` namespace is an ordinary standard one.
        resolver.register_namespace("zz", "http://ns.exiftool.org/1.0/");
        assert_eq!(resolver.group_for_prefix("zz"), "XMP-et");
    }

    #[test]
    fn version_segment_split_matches_xmp_pm_regex() {
        assert_eq!(
            split_version_segment("http://ns.adobe.com/photoshop/2.5/"),
            Some(("http://ns.adobe.com/photoshop/", "/"))
        );
        assert_eq!(
            split_version_segment("http://ns.microsoft.com/photo/1.0"),
            Some(("http://ns.microsoft.com/photo/", ""))
        );
        // `/1.0x/` is not a version segment.
        assert_eq!(split_version_segment("http://example.com/1.0x/"), None);
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
