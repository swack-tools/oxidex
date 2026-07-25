//! XMP namespace to ExifTool family mapping
//!
//! This module provides mapping from XMP namespace URIs to ExifTool family prefixes.
//! ExifTool uses a simplified `XMP:` prefix for most XMP properties, regardless of
//! their namespace (dc, xmp, exif, etc.).
//!
//! # Standard Mappings
//!
//! Common XMP namespaces map to ExifTool families as follows:
//! - http://purl.org/dc/elements/1.1/ → XMP (simplified, not XMP-dc)
//! - http://ns.adobe.com/xap/1.0/ → XMP (simplified, not XMP-xmp)
//! - http://ns.adobe.com/xap/1.0/mm/ → XMP-xmpMM
//! - http://ns.adobe.com/xap/1.0/rights/ → XMP-xmpRights
//! - http://ns.adobe.com/exif/1.0/ → XMP-exif
//! - http://ns.adobe.com/tiff/1.0/ → XMP-tiff
//! - http://ns.adobe.com/photoshop/1.0/ → XMP-photoshop
//! - http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/ → XMP-iptcCore
//!
//! Note: ExifTool uses simplified `XMP:` prefix for common properties like Creator, Title,
//! Rights, etc., instead of namespace-specific prefixes like `XMP-dc:` or `XMP-xmp:`.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Map XMP namespace URIs to ExifTool family prefixes
static NAMESPACE_TO_FAMILY: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    // ExifTool uses simplified "XMP:" prefix for common namespaces
    // instead of namespace-specific prefixes
    m.insert("http://purl.org/dc/elements/1.1/", "XMP");
    m.insert("http://ns.adobe.com/xap/1.0/", "XMP");

    // Specialized namespaces retain their full prefix
    m.insert("http://ns.adobe.com/xap/1.0/mm/", "XMP-xmpMM");
    m.insert("http://ns.adobe.com/xap/1.0/rights/", "XMP-xmpRights");
    m.insert("http://ns.adobe.com/exif/1.0/", "XMP-exif");
    m.insert("http://ns.adobe.com/tiff/1.0/", "XMP-tiff");
    m.insert("http://ns.adobe.com/photoshop/1.0/", "XMP-photoshop");
    m.insert(
        "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/",
        "XMP-iptcCore",
    );
    m.insert("http://iptc.org/std/Iptc4xmpExt/2008-02-29/", "XMP-iptcExt");
    m.insert("http://ns.useplus.org/ldf/xmp/1.0/", "XMP-plus");

    m
});

/// Get ExifTool family prefix for an XMP namespace URI
///
/// # Arguments
///
/// * `namespace_uri` - Full XMP namespace URI (e.g., "http://purl.org/dc/elements/1.1/")
///
/// # Returns
///
/// ExifTool family prefix (e.g., "XMP" for dc and xmp namespaces, "XMP-exif" for EXIF namespace)
///
/// # Example
///
/// ```no_run
/// use oxidex::parsers::xmp::namespace_mapping::namespace_to_family;
///
/// assert_eq!(namespace_to_family("http://purl.org/dc/elements/1.1/"), Some("XMP"));
/// assert_eq!(namespace_to_family("http://ns.adobe.com/xap/1.0/"), Some("XMP"));
/// assert_eq!(namespace_to_family("http://ns.adobe.com/exif/1.0/"), Some("XMP-exif"));
/// ```
pub fn namespace_to_family(namespace_uri: &str) -> Option<&'static str> {
    NAMESPACE_TO_FAMILY.get(namespace_uri).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dublin_core_maps_to_xmp() {
        assert_eq!(
            namespace_to_family("http://purl.org/dc/elements/1.1/"),
            Some("XMP")
        );
    }

    #[test]
    fn test_xmp_core_maps_to_xmp() {
        assert_eq!(
            namespace_to_family("http://ns.adobe.com/xap/1.0/"),
            Some("XMP")
        );
    }

    #[test]
    fn test_xmp_mm_maps_to_xmp_xmpmm() {
        assert_eq!(
            namespace_to_family("http://ns.adobe.com/xap/1.0/mm/"),
            Some("XMP-xmpMM")
        );
    }

    #[test]
    fn test_xmp_rights_maps_to_xmp_xmprights() {
        assert_eq!(
            namespace_to_family("http://ns.adobe.com/xap/1.0/rights/"),
            Some("XMP-xmpRights")
        );
    }

    #[test]
    fn test_exif_namespace_maps_to_xmp_exif() {
        assert_eq!(
            namespace_to_family("http://ns.adobe.com/exif/1.0/"),
            Some("XMP-exif")
        );
    }

    #[test]
    fn test_tiff_namespace_maps_to_xmp_tiff() {
        assert_eq!(
            namespace_to_family("http://ns.adobe.com/tiff/1.0/"),
            Some("XMP-tiff")
        );
    }

    #[test]
    fn test_photoshop_namespace_maps_to_xmp_photoshop() {
        assert_eq!(
            namespace_to_family("http://ns.adobe.com/photoshop/1.0/"),
            Some("XMP-photoshop")
        );
    }

    #[test]
    fn test_iptc_core_maps_to_xmp_iptccore() {
        assert_eq!(
            namespace_to_family("http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/"),
            Some("XMP-iptcCore")
        );
    }

    #[test]
    fn test_unknown_namespace_returns_none() {
        assert_eq!(namespace_to_family("http://example.com/unknown/"), None);
    }
}
