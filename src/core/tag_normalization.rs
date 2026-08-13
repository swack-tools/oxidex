//! Tag family normalization to match ExifTool conventions
//!
//! This module provides functions to normalize tag family prefixes from OxiDex's
//! internal representation to ExifTool's conventions. This improves compatibility
//! with ExifTool's output format.
//!
//! # Mapping Rules
//!
//! - `ExifIFD:`, `IFD0:`, `IFD1:`, `GPS:` remain unchanged (to match Perl ExifTool output)
//! - Manufacturer names (`Canon:`, `Nikon:`, `Sony:`, etc.) remain unchanged
//! - `Fujifilm:` -> `FujiFilm:` (ExifTool capitalises both halves)
//!
//! Step 22 retired this module's former `Profile:` -> `ICC_Profile:` entry
//! (plus its `BlueToneReproductionCurve` -> `BlueTRC`-style name shortening):
//! every ICC extraction site now inserts directly under `ICC_Profile:` with
//! its real family-1 group (`ICC-header`, `ICC-cicp`, `ICC-view`, `ICC-meas`,
//! or none) attached at extraction time (`src/parsers/icc/mod.rs`), so there
//! is no more `Profile:`-prefixed key for this module to rewrite. Before this
//! step, that rewrite only ran for JPEG (via [`normalize_metadata_map`],
//! JPEG's own last pipeline stage) -- every other ICC-bearing format (PNG's
//! `iCCP` chunk, GIF, FLIF, PSD, XCF, standalone `.icc`, embedded TIFF/RAW)
//! never called it at all, so their ICC tags stayed under the internal
//! `Profile:` prefix forever. That was the PNG "leak" this step's plan
//! names: not a bug in this module, but every non-JPEG caller never reaching
//! it.

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
    // ExifTool capitalises both halves: MakerNotes.pm:121 spells the tag
    // `MakerNoteFujiFilm`, and family 1 is named after it, so `-G1` prints
    // `[FujiFilm]`. Fold the old single-capital spelling onto it so any
    // straggler path still lands on the ExifTool key.
    m.insert("Fujifilm", "FujiFilm");
    m.insert("FujiFilm", "FujiFilm");
    m.insert("Panasonic", "Panasonic");
    m.insert("Olympus", "Olympus");
    m.insert("Pentax", "Pentax");
    m.insert("Samsung", "Samsung");
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
/// assert_eq!(normalize_tag_name("EXIF", "Make"), "Make");
/// ```
pub fn normalize_tag_name(_family: &str, name: &str) -> String {
    // No family currently needs a name-level rewrite. ICC's
    // `BlueToneReproductionCurve` -> `BlueTRC`-style shortening used to live
    // here as a backstop for `Profile:`-prefixed keys; Step 22 removed the
    // `Profile:` family mapping entirely (see the module doc comment), and
    // the ICC tag registry (`src/parsers/icc/registries.rs`) already emits
    // the short names directly at the source, so the backstop had nothing
    // left to catch.
    name.to_string()
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
/// assert_eq!(normalize_tag_family("Fujifilm:Quality"), "FujiFilm:Quality");
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
///
/// Step 19: rebuilt from [`MetadataMap::all_occurrences`] rather than
/// [`MetadataMap::iter`]'s winner-only projection, so every retained
/// occurrence -- not just each key's current winner -- survives family
/// renaming with its own priority, family-1 group and instance intact. This
/// is the one call site in the JPEG pipeline where a segment producing more
/// than one occurrence for the same key (`File:Comment`'s two `Priority =>
/// 0` sources, in particular) is still live *before* this function runs --
/// iterating winners only would have silently thrown the loser away a
/// second time, immediately after `MetadataMap::merge` was fixed to stop
/// doing exactly that at the pipeline's other flattening point.
///
/// Step 22: copies each occurrence via
/// [`MetadataMap::insert_renamed_occurrence`] instead of
/// [`MetadataMap::insert_occurrence`] plus a separate `value_form`
/// reattachment. The old two-call shape silently dropped
/// `TagOccurrence.value` for any occurrence that had one (`insert_occurrence`
/// has no `value` parameter), which the `value_forms` sidecar papered over
/// only for tags that also happened to go through `set_value_form` under the
/// exact same key string -- `insert_occurrence_with_raw`'s no-print-conv
/// form (Step 20) was never covered by that sidecar and was quietly lost by
/// this function specifically. `insert_renamed_occurrence` clones the whole
/// occurrence, so both forms now survive the rename intact.
pub fn normalize_metadata_map(map: &crate::core::MetadataMap) -> crate::core::MetadataMap {
    let mut normalized = crate::core::MetadataMap::with_capacity(map.len());
    for (key, occurrence) in map.all_occurrences() {
        let normalized_key = normalize_tag_family(&key);
        normalized.insert_renamed_occurrence(normalized_key, occurrence);
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
    fn test_normalize_tag_name_is_identity_now() {
        // Step 22 retired ICC's TRC-shortening special case (the ICC
        // registry emits short names directly at extraction time instead --
        // see `src/parsers/icc/registries.rs`), so this function has nothing
        // left to rewrite.
        assert_eq!(normalize_tag_name("EXIF", "Make"), "Make");
        assert_eq!(normalize_tag_name("ICC_Profile", "BlueTRC"), "BlueTRC");
    }

    #[test]
    fn test_normalize_metadata_map_preserves_value_form() {
        // Regression pin for the Step 22 finding documented on
        // `normalize_metadata_map`: renaming a family used to drop
        // `TagOccurrence.value` (the ValueConv form) because the old
        // `insert_occurrence` call this function made has no `value`
        // parameter. `insert_renamed_occurrence` carries the whole
        // occurrence across the rename instead.
        let mut map = MetadataMap::new();
        map.insert_occurrence_with_raw(
            "ExifIFD:FileSize",
            TagValue::new_string("26 kB"),
            TagValue::new_integer(26106),
            crate::core::SHIM_DEFAULT_PRIORITY,
            "ExifIFD",
            crate::core::Instance::default(),
        );

        let normalized = normalize_metadata_map(&map);

        assert_eq!(normalized.get_string("ExifIFD:FileSize"), Some("26 kB"));
        assert_eq!(
            normalized
                .without_print_conv()
                .get_integer("ExifIFD:FileSize"),
            Some(26106),
            "the no-print-conv form must survive the rename"
        );
    }
}
