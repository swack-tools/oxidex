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

use super::generated_namespaces::{URI_PREFIXES, translated_prefix};

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
    /// Lowest `N` whose `tmpN` is not yet a key of `cur_uri`. Keys are only
    /// ever added, so this never moves back, and allocating a `tmpN` costs
    /// amortized O(1) instead of rescanning from `tmp0` every time.
    next_tmp: usize,
    /// ExifTool's `$$et{xlatNS}`: document prefix -> the prefix it is
    /// translated to. Scoped the way `ParseXMPElement` scopes it, see
    /// [`Self::push_element_scope`].
    xlat: HashMap<String, String>,
    /// One entry per open element (see [`Self::push_element_scope`]).
    scopes: Vec<ElementScope>,
    /// Bindings replaced by declarations not yet attached to a scope: the
    /// declarations of the element about to be pushed.
    pending_bindings: Vec<(String, Option<String>)>,
}

/// Namespace state to restore when an element closes.
#[derive(Debug, Clone, Default)]
struct ElementScope {
    /// The prefix -> URI bindings this element's own `xmlns` declarations
    /// replaced (XML scoping: restored when the element closes).
    replaced_bindings: Vec<(String, Option<String>)>,
    /// Undo log for translation changes made by this element's *children*:
    /// `(prefix, translation before the change)`, replayed in reverse when the
    /// element closes. This is `ParseXMPElement`'s `$saveNS` (restored when
    /// that level returns, XMP.pm:4247) without copying the whole map per
    /// level, so a deeply nested packet costs O(depth + changes), not
    /// O(depth x translations).
    xlat_undo: Vec<(String, Option<String>)>,
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
            next_tmp: 0,
            xlat: HashMap::new(),
            scopes: Vec::new(),
            pending_bindings: Vec::new(),
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

        // The built-in bindings are the base scope: no element declared them,
        // so no element's close may undo them.
        resolver.pending_bindings.clear();

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
        let previous = self
            .prefix_to_uri
            .insert(prefix.to_string(), uri.to_string());
        self.pending_bindings.push((prefix.to_string(), previous));
        self.uri_to_prefix
            .insert(uri.to_string(), prefix.to_string());
        // `xmlns="..."` has no colon, and XMP.pm only handles `xmlns:PREFIX`
        // declarations (`if ($attr =~ /(.*?):/)`), so a default namespace
        // never takes part in prefix taming.
        if !prefix.is_empty() {
            self.tame_prefix(prefix, uri);
        }
    }

    /// Opens the scope of the element whose `xmlns` declarations were just
    /// registered. Call once per start tag, after registering its
    /// declarations and before resolving anything inside it, and pair it with
    /// [`Self::pop_element_scope`] at the matching end tag (an empty element
    /// pushes and pops at once).
    ///
    /// Two scopes are tracked, because ExifTool's differs from XML's:
    /// - prefix -> URI bindings follow XML: a declaration holds for its
    ///   element and descendants and is undone when the element closes;
    /// - prefix translations follow `ParseXMPElement` (XMP.pm:3768-4248): it
    ///   processes one level of sibling elements, a translation a sibling's
    ///   declaration introduces stays in force for every *later sibling* at
    ///   that level and their descendants (`$saveNS or $saveNS = $xlatNS,
    ///   $xlatNS = $$et{xlatNS} = { %$xlatNS }`, XMP.pm:3962), and the level
    ///   restores the translation map when it returns (XMP.pm:4247). So a
    ///   translation made on a child of element P is undone when P closes.
    pub fn push_element_scope(&mut self) {
        let replaced_bindings = std::mem::take(&mut self.pending_bindings);
        self.scopes.push(ElementScope {
            replaced_bindings,
            xlat_undo: Vec::new(),
        });
    }

    /// Closes the innermost element scope; see [`Self::push_element_scope`].
    pub fn pop_element_scope(&mut self) {
        let Some(scope) = self.scopes.pop() else {
            return;
        };
        for (prefix, previous) in scope.xlat_undo.into_iter().rev() {
            match previous {
                Some(translation) => {
                    self.xlat.insert(prefix, translation);
                }
                None => {
                    self.xlat.remove(&prefix);
                }
            }
        }
        for (prefix, previous) in scope.replaced_bindings.into_iter().rev() {
            match previous {
                Some(uri) => {
                    self.prefix_to_uri.insert(prefix, uri);
                }
                None => {
                    self.prefix_to_uri.remove(&prefix);
                }
            }
        }
    }

    /// Sets (`Some`) or removes (`None`) the translation for `prefix`,
    /// logging the previous value against the current level (the innermost
    /// open element's children) so the level's close can undo it.
    fn set_xlat(&mut self, prefix: &str, translation: Option<String>) {
        let previous = match translation {
            Some(translation) => self.xlat.insert(prefix.to_string(), translation),
            None => self.xlat.remove(prefix),
        };
        // With no element open there is no level to restore on return.
        if let Some(scope) = self.scopes.last_mut() {
            scope.xlat_undo.push((prefix.to_string(), previous));
        }
    }

    /// ExifTool's namespace-prefix taming for one `xmlns:PREFIX="URI"`
    /// declaration (XMP.pm:3901-3970).
    ///
    /// `curNS`/`curURI` are per packet (reset in `ProcessXMP`,
    /// XMP.pm:4277-4278), and a resolver lives for one packet. The resulting
    /// translation is textual -- it renames the prefix, not the URI -- and
    /// scoped as [`Self::push_element_scope`] describes, which is why a
    /// translation can outlive the declaration that caused it (XMP.pm leaks
    /// it to later siblings, and a later plain re-declaration of the prefix
    /// does not clear it).
    fn tame_prefix(&mut self, prefix: &str, uri: &str) {
        let new_prefix: Option<String> = if let Some(std_ns) = standard_prefix_for_declared_uri(uri)
        {
            // "use standard namespace prefix if pre-defined"
            if std_ns != prefix {
                Some(std_ns.to_string())
            } else if self.xlat.contains_key(prefix) {
                // "this prefix is re-defined to the standard prefix in this
                // scope"
                Some(String::new())
            } else {
                None
            }
        } else if let Some(used) = self.cur_ns.get(uri) {
            // "use a consistent prefix over the entire XMP for a given
            // namespace URI"
            (used != prefix).then(|| used.clone())
        } else {
            // "use unique prefixes for all namespaces across the entire
            // XMP": a prefix already bound to another non-standard URI in
            // this packet, or one of ExifTool's own standard prefixes
            // (`$nsURI{$ns}`), is replaced by the lowest free `tmpN`.
            let mut used = prefix.to_string();
            let mut new_prefix = None;
            if self.cur_uri.contains_key(prefix) || is_standard_prefix(prefix) {
                while self.cur_uri.contains_key(&format!("tmp{}", self.next_tmp)) {
                    self.next_tmp += 1;
                }
                used = format!("tmp{}", self.next_tmp);
                new_prefix = Some(used.clone());
            }
            self.cur_ns.insert(uri.to_string(), used.clone());
            self.cur_uri.insert(used, uri.to_string());
            new_prefix
        };
        if let Some(new_prefix) = new_prefix {
            let translation = (!new_prefix.is_empty()).then_some(new_prefix);
            self.set_xlat(prefix, translation);
        }
    }

    /// The ExifTool family-1 group for a property written with `prefix`, as
    /// of now: `XMP-<prefix>`, where the prefix is the document prefix after
    /// ExifTool's translation ([`Self::tame_prefix`]) and then `%stdXlatNS`
    /// (XMP.pm FoundXMP: `$ns = $stdXlatNS{$ns} if $stdXlatNS{$ns}` ...
    /// `SetGroup($key, "$$tagTablePtr{GROUPS}{0}-$ns")`).
    ///
    /// ExifTool translates a property's name when it reaches the element,
    /// before parsing the element's children, so callers resolve the group
    /// right after [`Self::push_element_scope`] for that element -- not at its
    /// end tag, when a child's declaration may have changed the answer.
    ///
    /// An empty prefix has no namespace, and ExifTool calls no `SetGroup` for
    /// it, so the group stays plain `XMP`. A prefix with no translation keeps
    /// its own spelling, declared or not, as ExifTool's does.
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
    /// - ExifTool processes a start tag's attributes in order, so an
    ///   attribute *before* an `xmlns` on the same element is renamed
    ///   retroactively while a property element is not; every declaration on
    ///   an element is registered before any of its names are resolved here.
    pub fn group_for_prefix(&self, prefix: &str) -> String {
        if prefix.is_empty() {
            return "XMP".to_string();
        }
        if self
            .resolve_prefix(prefix)
            .is_some_and(is_exiftool_static_group_uri)
        {
            return "XMP".to_string();
        }
        let effective = self.xlat.get(prefix).map_or(prefix, String::as_str);
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
    standard_namespace_for_declared_uri(uri).map(|(_, prefix)| prefix)
}

/// The `%nsURI` URI ExifTool treats a declared URI as, by the same matching
/// as [`standard_prefix_for_declared_uri`]; `None` for a non-standard URI.
/// Lets URI-keyed rename tables follow ExifTool's slash/version tolerance
/// (`http://ns.adobe.com/camera-raw-settings/12.3/` is the `crs` table).
pub fn canonical_standard_uri(uri: &str) -> Option<&'static str> {
    standard_namespace_for_declared_uri(uri).map(|(known, _)| known)
}

fn standard_namespace_for_declared_uri(uri: &str) -> Option<(&'static str, &'static str)> {
    let find = |candidate: &str| {
        URI_PREFIXES
            .binary_search_by(|(known, _)| (*known).cmp(candidate))
            .ok()
            .map(|index| URI_PREFIXES[index])
    };
    if let Some(found) = find(uri) {
        return Some(found);
    }
    let toggled = match uri.strip_suffix('/') {
        Some(stripped) => stripped.to_string(),
        None => format!("{uri}/"),
    };
    if let Some(found) = find(&toggled) {
        return Some(found);
    }
    let (head, tail) = split_version_segment(uri)?;
    VERSIONED_URIS.get(&(head, tail)).copied()
}

/// The standard URIs that carry an `N.N` version segment, keyed by the text
/// around it. `grep /^$try$/, keys %uri2ns` takes whichever match hash order
/// yields first; the table is sorted, so the first sorted URI is kept.
static VERSIONED_URIS: std::sync::LazyLock<
    HashMap<(&'static str, &'static str), (&'static str, &'static str)>,
> = std::sync::LazyLock::new(|| {
    let mut index = HashMap::new();
    for &(known, prefix) in URI_PREFIXES {
        if let Some(key) = split_version_segment(known) {
            index.entry(key).or_insert((known, prefix));
        }
    }
    index
});

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

    /// Runs `parse_xmp` and returns `(key, value)` for every property.
    fn keys(packet: &str) -> Vec<(String, String)> {
        crate::parsers::xmp::parse_xmp(packet.as_bytes()).unwrap()
    }

    const RDF_OPEN: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">"#;
    const RDF_CLOSE: &str = "</rdf:RDF></x:xmpmeta>";

    /// Review packet t2.xmp: a declaration on a property's own child does not
    /// change the property's group (pinned 13.59: `[XMP-a]`), because ExifTool
    /// translates the property name before parsing its value.
    #[test]
    fn a_childs_declaration_does_not_regroup_its_parent_property() {
        let got = keys(&format!(
            r#"{RDF_OPEN}<rdf:Description rdf:about="" xmlns:a="http://e.com/A/"><a:x><rdf:Bag xmlns:a="http://e.com/B/"><rdf:li>v1</rdf:li><rdf:li>v2</rdf:li></rdf:Bag></a:x></rdf:Description>{RDF_CLOSE}"#
        ));
        assert!(got.iter().all(|(k, _)| k.starts_with("XMP-a:")), "{got:?}");
    }

    /// Review packet t3.xmp: a translation made by a sibling's declaration
    /// leaks to later siblings, and re-declaring the prefix for its first URI
    /// does not clear it (pinned 13.59: P1 `XMP-a`, P2-P4 `XMP-tmp0`).
    #[test]
    fn a_sibling_translation_leaks_to_later_siblings() {
        let got = keys(&format!(
            r#"{RDF_OPEN}<rdf:Description rdf:about="" xmlns:a="http://e.com/A/"><a:p1>1</a:p1></rdf:Description><rdf:Description rdf:about="" xmlns:a="http://e.com/B/"><a:p2>2</a:p2></rdf:Description><rdf:Description rdf:about="" xmlns:a="http://e.com/A/"><a:p3>3</a:p3></rdf:Description><rdf:Description rdf:about="" xmlns:b="http://e.com/B/"><b:p4>4</b:p4></rdf:Description>{RDF_CLOSE}"#
        ));
        let expected = [
            ("XMP-a:P1", "1"),
            ("XMP-tmp0:P2", "2"),
            ("XMP-tmp0:P3", "3"),
            ("XMP-tmp0:P4", "4"),
        ];
        for (key, value) in expected {
            assert!(
                got.contains(&(key.to_string(), value.to_string())),
                "{key}: {got:?}"
            );
        }
    }

    /// Review packet t11.xmp: a translation made inside a struct is undone
    /// when the struct's level returns (pinned 13.59: SIn and After both
    /// `XMP-a`).
    #[test]
    fn a_translation_inside_a_struct_ends_with_the_struct() {
        let got = keys(&format!(
            r#"{RDF_OPEN}<rdf:Description rdf:about="" xmlns:a="http://e.com/A/"><a:s rdf:parseType="Resource"><a:in xmlns:a="http://e.com/B/">inner</a:in></a:s><a:after>outer</a:after></rdf:Description>{RDF_CLOSE}"#
        ));
        assert!(
            got.contains(&("XMP-a:SIn".to_string(), "inner".to_string())),
            "{got:?}"
        );
        assert!(
            got.contains(&("XMP-a:After".to_string(), "outer".to_string())),
            "{got:?}"
        );
    }

    /// Review packet t5.xmp: URI-keyed renames follow ExifTool's slash and
    /// version matching (pinned 13.59: `XMP-microsoft:RatingPercent`,
    /// `XMP-crs:ColorTemperature`).
    #[test]
    fn uri_keyed_renames_match_tolerant_uris() {
        let got = keys(&format!(
            r#"{RDF_OPEN}<rdf:Description rdf:about="" xmlns:ms="http://ns.microsoft.com/photo/1.0/" xmlns:cr="http://ns.adobe.com/camera-raw-settings/12.3/"><ms:Rating>50</ms:Rating><cr:Temperature>5000</cr:Temperature></rdf:Description>{RDF_CLOSE}"#
        ));
        assert!(
            got.contains(&("XMP-microsoft:RatingPercent".to_string(), "50".to_string())),
            "{got:?}"
        );
        assert!(
            got.contains(&("XMP-crs:ColorTemperature".to_string(), "5000".to_string())),
            "{got:?}"
        );
    }

    /// Review packet r1.xmp: a self-closing `x:xmpmeta` followed by an
    /// `rdf:RDF` that relies on the built-in `rdf` binding. Closing the first
    /// element must not undo the built-in bindings (pinned 13.59 and
    /// 8f90b337: `[XMP-xmp] Rating 3`, `[XMP-dc] Format image/jpeg`).
    #[test]
    fn built_in_bindings_survive_the_first_element_closing() {
        let got = keys(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="t"/><rdf:RDF><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmp="http://ns.adobe.com/xap/1.0/"><xmp:Rating>3</xmp:Rating><dc:format>image/jpeg</dc:format></rdf:Description></rdf:RDF>"#,
        );
        assert!(
            got.contains(&("XMP-xmp:Rating".to_string(), "3".to_string())),
            "{got:?}"
        );
        assert!(
            got.contains(&("XMP-dc:Format".to_string(), "image/jpeg".to_string())),
            "{got:?}"
        );

        let mut resolver = NamespaceResolver::new();
        resolver.push_element_scope();
        resolver.pop_element_scope();
        assert_eq!(
            resolver.resolve_prefix("rdf"),
            Some("http://www.w3.org/1999/02/22-rdf-syntax-ns#")
        );
    }

    /// Each translation change is logged once against its level and undone
    /// on close, so nesting N levels that each rebind a prefix keeps N undo
    /// entries in total -- not a copy of the whole translation map per level
    /// (a 5000-level packet took 4.48 GB that way).
    #[test]
    fn translation_undo_log_is_linear_in_changes() {
        const DEPTH: usize = 5000;
        let mut resolver = NamespaceResolver::new();
        for level in 0..DEPTH {
            resolver.register_namespace(&format!("p{level}"), "http://e.com/shared/");
            resolver.push_element_scope();
        }
        let logged: usize = resolver.scopes.iter().map(|s| s.xlat_undo.len()).sum();
        // p0 claims the URI; every later prefix is translated to p0.
        assert_eq!(logged, DEPTH - 1);
        assert_eq!(
            resolver.group_for_prefix(&format!("p{}", DEPTH - 1)),
            "XMP-p0"
        );
        for _ in 0..DEPTH {
            resolver.pop_element_scope();
        }
        assert!(resolver.xlat.is_empty());

        // And a deeply nested packet parses. `a:s0`'s own declaration
        // translates `a` to `tmp0` at the Description's level, so both the
        // flattened leaf and the later sibling land in XMP-tmp0 -- pinned
        // 13.59 on the same shape three levels deep prints
        // `[XMP-tmp0] S0S1S2Leaf : v` and `[XMP-tmp0] After : w`.
        let mut packet = String::from(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:a="http://e.com/A/">"#,
        );
        for level in 0..500 {
            packet.push_str(&format!(
                r#"<a:s{level} rdf:parseType="Resource" xmlns:a="http://e.com/N{level}/">"#
            ));
        }
        packet.push_str("<a:leaf>v</a:leaf>");
        for level in (0..500).rev() {
            packet.push_str(&format!("</a:s{level}>"));
        }
        packet.push_str("<a:after>w</a:after></rdf:Description></rdf:RDF>");
        let got = keys(&packet);
        assert!(
            got.contains(&("XMP-tmp0:After".to_string(), "w".to_string())),
            "{got:?}"
        );
        let leaf: String = (0..500).map(|level| format!("S{level}")).collect();
        assert!(
            got.contains(&(format!("XMP-tmp0:{leaf}Leaf"), "v".to_string())),
            "{got:?}"
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
