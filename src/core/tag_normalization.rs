//! Tag family normalization to match ExifTool conventions
//!
//! This module provides functions to normalize tag family prefixes from OxiDex's
//! internal representation to ExifTool's conventions. This improves compatibility
//! with ExifTool's output format.
//!
//! # Mapping Rules
//!
//! - `Profile:` -> `ICC_Profile:` (with name normalization for TRC tags)
//! - `ExifIFD:`, `IFD0:`, `IFD1:`, `GPS:` remain unchanged (to match Perl ExifTool output)
//! - Manufacturer names (`Canon:`, `Nikon:`, `Sony:`, etc.) remain unchanged

use std::collections::HashMap;
use std::sync::LazyLock;

/// Family prefix mappings from OxiDex to ExifTool conventions
static FAMILY_MAPPINGS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // ExifIFD remains unchanged - Perl ExifTool outputs ExifIFD:xxx
    m.insert("ExifIFD", "ExifIFD");
    // GPS IFD remains unchanged - Perl ExifTool outputs GPS tags as GPS:xxx
    m.insert("GPS", "GPS");
    // InteropIFD mapping (unchanged)
    m.insert("InteropIFD", "InteropIFD");
    // IFD0 and IFD1 remain unchanged but are included for documentation
    m.insert("IFD0", "IFD0");
    m.insert("IFD1", "IFD1");
    // Maker note families (unchanged)
    m.insert("Canon", "Canon");
    m.insert("Nikon", "Nikon");
    m.insert("Sony", "Sony");
    m.insert("Fujifilm", "Fujifilm");
    m.insert("Panasonic", "Panasonic");
    m.insert("Olympus", "Olympus");
    m.insert("Pentax", "Pentax");
    m.insert("Samsung", "Samsung");
    // ICC Profile mapping - OxiDex uses "Profile" but ExifTool uses "ICC_Profile"
    m.insert("Profile", "ICC_Profile");
    m
});

/// Normalize tag names within specific families to match ExifTool conventions
///
/// Some tag names differ between OxiDex's internal representation and ExifTool's
/// output. This function handles those specific name mappings.
///
/// # Arguments
/// * `family` - The normalized family name (e.g., "ICC_Profile")
/// * `name` - The tag name to potentially normalize
///
/// # Returns
/// The normalized tag name, or the original name if no normalization is needed
///
/// # Examples
/// ```
/// use oxidex::core::tag_normalization::normalize_tag_name;
///
/// // TRC (Tone Reproduction Curve) tags are shortened in ExifTool
/// assert_eq!(normalize_tag_name("ICC_Profile", "BlueToneReproductionCurve"), "BlueTRC");
/// assert_eq!(normalize_tag_name("ICC_Profile", "BlueMatrixColumn"), "BlueMatrixColumn");
/// ```
pub fn normalize_tag_name(family: &str, name: &str) -> String {
    match (family, name) {
        // ICC Profile naming differences - ExifTool uses abbreviated "TRC" suffix
        // instead of "ToneReproductionCurve" for tone reproduction curve tags
        ("ICC_Profile", "BlueToneReproductionCurve") => "BlueTRC".to_string(),
        ("ICC_Profile", "GreenToneReproductionCurve") => "GreenTRC".to_string(),
        ("ICC_Profile", "RedToneReproductionCurve") => "RedTRC".to_string(),
        ("ICC_Profile", "GrayToneReproductionCurve") => "GrayTRC".to_string(),
        // All other tag names remain unchanged
        _ => name.to_string(),
    }
}

/// Normalize a tag key to match ExifTool family conventions
///
/// This function normalizes both the family prefix and the tag name. For example,
/// "Profile:BlueToneReproductionCurve" becomes "ICC_Profile:BlueTRC".
///
/// # Arguments
/// * `tag_key` - Full tag key like "ExifIFD:Make"
///
/// # Returns
/// Normalized key like "ExifIFD:Make" or "ICC_Profile:BlueTRC"
///
/// # Examples
///
/// ```
/// use oxidex::core::tag_normalization::normalize_tag_family;
///
/// assert_eq!(normalize_tag_family("ExifIFD:Make"), "ExifIFD:Make");
/// assert_eq!(normalize_tag_family("IFD0:Make"), "IFD0:Make");
/// assert_eq!(normalize_tag_family("Canon:LensModel"), "Canon:LensModel");
/// assert_eq!(normalize_tag_family("Profile:BlueToneReproductionCurve"), "ICC_Profile:BlueTRC");
/// ```
pub fn normalize_tag_family(tag_key: &str) -> String {
    if let Some((family, name)) = tag_key.split_once(':')
        && let Some(normalized_family) = FAMILY_MAPPINGS.get(family)
    {
        // Apply both family and name normalization
        let normalized_name = normalize_tag_name(normalized_family, name);
        return format!("{}:{}", normalized_family, normalized_name);
    }
    tag_key.to_string()
}

/// Normalize all tags in a MetadataMap
///
/// This function creates a new MetadataMap with all tag keys normalized
/// to match ExifTool conventions. The original map is not modified.
///
/// # Arguments
/// * `map` - The metadata map to normalize
///
/// # Returns
/// A new MetadataMap with normalized tag keys
///
/// # Examples
///
/// ```
/// use oxidex::core::{MetadataMap, TagValue};
/// use oxidex::core::tag_normalization::normalize_metadata_map;
///
/// let mut map = MetadataMap::new();
/// map.insert("ExifIFD:Make", TagValue::new_string("Canon"));
/// map.insert("IFD0:Model", TagValue::new_string("EOS R5"));
///
/// let normalized = normalize_metadata_map(&map);
/// assert_eq!(normalized.get_string("ExifIFD:Make"), Some("Canon"));
/// assert_eq!(normalized.get_string("IFD0:Model"), Some("EOS R5"));
/// ```
pub fn normalize_metadata_map(map: &crate::core::MetadataMap) -> crate::core::MetadataMap {
    let mut normalized = crate::core::MetadataMap::with_capacity(map.len());
    for (key, value) in map.iter() {
        let normalized_key = normalize_tag_family(key);
        normalized.insert(normalized_key, value.clone());
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{MetadataMap, TagValue};

    #[test]
    fn test_exififd_unchanged() {
        // ExifIFD remains unchanged to match Perl ExifTool output
        assert_eq!(normalize_tag_family("ExifIFD:Make"), "ExifIFD:Make");
        assert_eq!(normalize_tag_family("ExifIFD:Model"), "ExifIFD:Model");
        assert_eq!(
            normalize_tag_family("ExifIFD:DateTimeOriginal"),
            "ExifIFD:DateTimeOriginal"
        );
    }

    #[test]
    fn test_ifd0_unchanged() {
        assert_eq!(normalize_tag_family("IFD0:Make"), "IFD0:Make");
        assert_eq!(normalize_tag_family("IFD0:Model"), "IFD0:Model");
    }

    #[test]
    fn test_ifd1_unchanged() {
        assert_eq!(normalize_tag_family("IFD1:Compression"), "IFD1:Compression");
    }

    #[test]
    fn test_gps_unchanged() {
        // GPS tags remain unchanged - Perl ExifTool outputs GPS:xxx
        assert_eq!(normalize_tag_family("GPS:GPSLatitude"), "GPS:GPSLatitude");
        assert_eq!(normalize_tag_family("GPS:GPSLongitude"), "GPS:GPSLongitude");
        assert_eq!(normalize_tag_family("GPS:GPSAltitude"), "GPS:GPSAltitude");
    }

    #[test]
    fn test_makernotes_unchanged() {
        assert_eq!(normalize_tag_family("Canon:LensModel"), "Canon:LensModel");
        assert_eq!(
            normalize_tag_family("Nikon:ShutterCount"),
            "Nikon:ShutterCount"
        );
        assert_eq!(normalize_tag_family("Sony:SonyModelID"), "Sony:SonyModelID");
    }

    #[test]
    fn test_unknown_family_unchanged() {
        assert_eq!(normalize_tag_family("Custom:Tag"), "Custom:Tag");
        assert_eq!(normalize_tag_family("Unknown:Field"), "Unknown:Field");
    }

    #[test]
    fn test_no_colon_unchanged() {
        assert_eq!(normalize_tag_family("NoColonHere"), "NoColonHere");
        assert_eq!(normalize_tag_family("SimpleTag"), "SimpleTag");
    }

    #[test]
    fn test_normalize_metadata_map() {
        let mut map = MetadataMap::new();
        map.insert("ExifIFD:Make", TagValue::new_string("Canon"));
        map.insert("ExifIFD:Model", TagValue::new_string("EOS R5"));
        map.insert("IFD0:Software", TagValue::new_string("OxiDex"));
        map.insert("GPS:GPSLatitude", TagValue::new_string("37.7749"));
        map.insert("Canon:LensModel", TagValue::new_string("EF 24-70mm"));

        let normalized = normalize_metadata_map(&map);

        // ExifIFD remains unchanged
        assert_eq!(normalized.get_string("ExifIFD:Make"), Some("Canon"));
        assert_eq!(normalized.get_string("ExifIFD:Model"), Some("EOS R5"));

        // GPS, IFD0 and Canon should remain unchanged
        assert_eq!(normalized.get_string("GPS:GPSLatitude"), Some("37.7749"));
        assert_eq!(normalized.get_string("IFD0:Software"), Some("OxiDex"));
        assert_eq!(normalized.get_string("Canon:LensModel"), Some("EF 24-70mm"));

        // Verify we have the same number of tags
        assert_eq!(normalized.len(), map.len());
    }

    #[test]
    fn test_normalize_empty_map() {
        let map = MetadataMap::new();
        let normalized = normalize_metadata_map(&map);
        assert_eq!(normalized.len(), 0);
        assert!(normalized.is_empty());
    }

    #[test]
    fn test_normalize_preserves_values() {
        let mut map = MetadataMap::new();
        map.insert("ExifIFD:ISO", TagValue::new_integer(400));
        map.insert("ExifIFD:FNumber", TagValue::new_float(2.8));

        let normalized = normalize_metadata_map(&map);

        assert_eq!(normalized.get_integer("ExifIFD:ISO"), Some(400));
        assert_eq!(normalized.get_float("ExifIFD:FNumber"), Some(2.8));
    }

    #[test]
    fn test_profile_to_icc_profile() {
        // Basic Profile -> ICC_Profile family mapping
        assert_eq!(
            normalize_tag_family("Profile:BlueMatrixColumn"),
            "ICC_Profile:BlueMatrixColumn"
        );
        assert_eq!(
            normalize_tag_family("Profile:CMMFlags"),
            "ICC_Profile:CMMFlags"
        );
        assert_eq!(
            normalize_tag_family("Profile:ColorSpaceData"),
            "ICC_Profile:ColorSpaceData"
        );
        assert_eq!(
            normalize_tag_family("Profile:ProfileVersion"),
            "ICC_Profile:ProfileVersion"
        );
    }

    #[test]
    fn test_trc_name_normalization() {
        // ToneReproductionCurve tags should be shortened to TRC
        assert_eq!(
            normalize_tag_family("Profile:BlueToneReproductionCurve"),
            "ICC_Profile:BlueTRC"
        );
        assert_eq!(
            normalize_tag_family("Profile:GreenToneReproductionCurve"),
            "ICC_Profile:GreenTRC"
        );
        assert_eq!(
            normalize_tag_family("Profile:RedToneReproductionCurve"),
            "ICC_Profile:RedTRC"
        );
        assert_eq!(
            normalize_tag_family("Profile:GrayToneReproductionCurve"),
            "ICC_Profile:GrayTRC"
        );
    }

    #[test]
    fn test_normalize_tag_name_directly() {
        // Test the normalize_tag_name function directly
        assert_eq!(
            normalize_tag_name("ICC_Profile", "BlueToneReproductionCurve"),
            "BlueTRC"
        );
        assert_eq!(
            normalize_tag_name("ICC_Profile", "GreenToneReproductionCurve"),
            "GreenTRC"
        );
        assert_eq!(
            normalize_tag_name("ICC_Profile", "RedToneReproductionCurve"),
            "RedTRC"
        );
        assert_eq!(
            normalize_tag_name("ICC_Profile", "GrayToneReproductionCurve"),
            "GrayTRC"
        );

        // Non-TRC tags should remain unchanged
        assert_eq!(
            normalize_tag_name("ICC_Profile", "BlueMatrixColumn"),
            "BlueMatrixColumn"
        );
        assert_eq!(
            normalize_tag_name("ICC_Profile", "ProfileVersion"),
            "ProfileVersion"
        );

        // Other families should not have name changes
        assert_eq!(
            normalize_tag_name("EXIF", "BlueToneReproductionCurve"),
            "BlueToneReproductionCurve"
        );
    }

    #[test]
    fn test_normalize_metadata_map_with_icc_profile() {
        let mut map = MetadataMap::new();
        map.insert(
            "Profile:BlueMatrixColumn",
            TagValue::new_string("0.14307 0.06061 0.7141"),
        );
        map.insert(
            "Profile:BlueToneReproductionCurve",
            TagValue::new_string("(Binary data)"),
        );
        map.insert("Profile:CMMFlags", TagValue::new_string("Not Embedded"));

        let normalized = normalize_metadata_map(&map);

        // Profile should be normalized to ICC_Profile
        assert_eq!(
            normalized.get_string("ICC_Profile:BlueMatrixColumn"),
            Some("0.14307 0.06061 0.7141")
        );
        // ToneReproductionCurve should be shortened to TRC
        assert_eq!(
            normalized.get_string("ICC_Profile:BlueTRC"),
            Some("(Binary data)")
        );
        assert_eq!(
            normalized.get_string("ICC_Profile:CMMFlags"),
            Some("Not Embedded")
        );
    }
}
