//! Generated tag database
//!
//! Re-exports tag definitions from the exiftool-tags crate

#![allow(dead_code)]

pub mod generated_tags;
pub mod tag_registry;

// Re-export everything from exiftool-tags crate
pub use oxidex_tags::*;

use std::collections::HashMap;
use std::sync::LazyLock;

// Re-export commonly used registry functions
pub use tag_registry::{get_tag_descriptor, tag_count};

/// Reverse lookup index: (numeric tag ID, format family) -> tag name
/// Built lazily on first access from the YAML-based tag databases
static TAG_ID_TO_NAME_INDEX: LazyLock<HashMap<(u16, FormatFamily), String>> = LazyLock::new(|| {
    let mut index = HashMap::with_capacity(10000);

    // Helper function to determine FormatFamily and prefix from table name
    fn get_format_info(table_name: &str) -> Option<(FormatFamily, &'static str)> {
        if table_name.starts_with("Exif::") {
            Some((FormatFamily::EXIF, "EXIF"))
        } else if table_name.starts_with("GPS::") {
            Some((FormatFamily::GPS, "GPS"))
        } else if table_name.starts_with("XMP::") {
            Some((FormatFamily::XMP, "XMP"))
        } else if table_name.starts_with("IPTC::") {
            Some((FormatFamily::IPTC, "IPTC"))
        } else if table_name.starts_with("ICC_Profile::") {
            Some((FormatFamily::ICCProfile, "ICC_Profile"))
        } else if table_name.starts_with("Photoshop::") {
            Some((FormatFamily::Photoshop, "Photoshop"))
        } else {
            // Default to EXIF for other tables that might contain numeric tags
            None
        }
    }

    // Helper function to parse hex tag ID from string
    fn parse_tag_id(id_str: &str) -> Option<u16> {
        if let Some(hex_str) = id_str.strip_prefix("0x") {
            u16::from_str_radix(hex_str, 16).ok()
        } else {
            id_str.parse::<u16>().ok()
        }
    }

    // This used to be ~50 substring and exact-match hacks -- "-bit ",
    // " Channels", "Profile M", and a literal list containing "Manual",
    // "Portrait", "Auto", "Uncompressed" -- introduced because the YAML
    // registry carried 16,014 PrintConv display values as tag entries, and
    // something had to keep them out of this index.
    //
    // The data is fixed at the source now (see
    // scripts/prune_printconv_tag_entries.py and
    // tests/tag_registry_invariants.rs), which inverted this filter: it
    // rejected zero fabrications and one real tag. `Uncompressed` is a
    // genuine Exif::Main tag at 0xBC03 -- HD Photo's compression value --
    // and the enum list dropped it, so it could never be identified.
    //
    // What survives is the one rule still doing work: XMP::Main is keyed by
    // namespace prefix (`x`, `mwg-rs`, `acdsee-rs`, `drone-dji`), and those
    // are SubDirectory routes rather than tags, so they must not claim a
    // numeric id here. ExifTool has no lowercase-initial tag name.
    fn is_valid_tag_name(name: &str) -> bool {
        let Some(first) = name.chars().next() else {
            return false;
        };
        !first.is_ascii_lowercase() || name.starts_with("undef") || name.starts_with("n/a")
    }

    // Scan all domain tag databases and build reverse index
    // We iterate through: core, camera, media, image, document, specialty
    // Using entry().or_insert() so FIRST occurrence wins (standard tags take priority over value names)

    // Core domain (contains standard EXIF/TIFF tags - process first for priority)
    // Skip Composite tables as they contain derived/calculated values, not primary tags
    for table in &core::CORE_TAGS.tables {
        // Skip Composite tables - they're derived values, not primary tag definitions
        if table.name.contains("::Composite") {
            continue;
        }

        if let Some((format_family, prefix)) = get_format_info(&table.name) {
            for tag in &table.tags {
                if let Some(tag_id) = parse_tag_id(&tag.id) {
                    // Skip invalid tag names (enum values mixed in with real tags)
                    if !is_valid_tag_name(&tag.name) {
                        continue;
                    }
                    let full_name = format!("{}:{}", prefix, tag.name);
                    index.entry((tag_id, format_family)).or_insert(full_name);
                }
            }
        }
    }

    // Camera domain
    for table in &camera::CAMERA_TAGS.tables {
        if table.name.contains("::Composite") {
            continue;
        }
        if let Some((format_family, prefix)) = get_format_info(&table.name) {
            for tag in &table.tags {
                if let Some(tag_id) = parse_tag_id(&tag.id) {
                    if !is_valid_tag_name(&tag.name) {
                        continue;
                    }
                    let full_name = format!("{}:{}", prefix, tag.name);
                    index.entry((tag_id, format_family)).or_insert(full_name);
                }
            }
        }
    }

    // Media domain
    for table in &media::MEDIA_TAGS.tables {
        if table.name.contains("::Composite") {
            continue;
        }
        if let Some((format_family, prefix)) = get_format_info(&table.name) {
            for tag in &table.tags {
                if let Some(tag_id) = parse_tag_id(&tag.id) {
                    if !is_valid_tag_name(&tag.name) {
                        continue;
                    }
                    let full_name = format!("{}:{}", prefix, tag.name);
                    index.entry((tag_id, format_family)).or_insert(full_name);
                }
            }
        }
    }

    // Image domain
    for table in &image::IMAGE_TAGS.tables {
        if table.name.contains("::Composite") {
            continue;
        }
        if let Some((format_family, prefix)) = get_format_info(&table.name) {
            for tag in &table.tags {
                if let Some(tag_id) = parse_tag_id(&tag.id) {
                    if !is_valid_tag_name(&tag.name) {
                        continue;
                    }
                    let full_name = format!("{}:{}", prefix, tag.name);
                    index.entry((tag_id, format_family)).or_insert(full_name);
                }
            }
        }
    }

    // Document domain
    for table in &document::DOCUMENT_TAGS.tables {
        if table.name.contains("::Composite") {
            continue;
        }
        if let Some((format_family, prefix)) = get_format_info(&table.name) {
            for tag in &table.tags {
                if let Some(tag_id) = parse_tag_id(&tag.id) {
                    if !is_valid_tag_name(&tag.name) {
                        continue;
                    }
                    let full_name = format!("{}:{}", prefix, tag.name);
                    index.entry((tag_id, format_family)).or_insert(full_name);
                }
            }
        }
    }

    // Specialty domain
    for table in &specialty::SPECIALTY_TAGS.tables {
        if table.name.contains("::Composite") {
            continue;
        }
        if let Some((format_family, prefix)) = get_format_info(&table.name) {
            for tag in &table.tags {
                if let Some(tag_id) = parse_tag_id(&tag.id) {
                    if !is_valid_tag_name(&tag.name) {
                        continue;
                    }
                    let full_name = format!("{}:{}", prefix, tag.name);
                    index.entry((tag_id, format_family)).or_insert(full_name);
                }
            }
        }
    }

    index
});

/// Looks up a tag name from a numeric tag ID and IFD context.
///
/// This function performs a reverse lookup in the generated tag database to find
/// the canonical tag name for a given numeric ID. It handles the ExifTool naming
/// convention where the main IFD tags use "IFD0:" prefix, EXIF sub-IFD tags use
/// "ExifIFD:" prefix, and GPS sub-IFD tags use "GPS:" prefix.
///
/// # Arguments
///
/// * `tag_id` - The numeric tag identifier (e.g., 0x010F for Make)
/// * `ifd_name` - The IFD context ("IFD0", "ExifIFD", "GPS", etc.)
///
/// # Returns
///
/// A tag name string in the format "Family:TagName" (e.g., "IFD0:Make").
/// If the tag is not in the database, returns a hex fallback (e.g., "IFD0:0x010F").
///
/// # Examples
///
/// ```
/// use oxidex::tag_db::lookup_tag_name;
///
/// assert_eq!(lookup_tag_name(0x010F, "IFD0"), "IFD0:Make");
/// assert_eq!(lookup_tag_name(0x0110, "IFD0"), "IFD0:Model");
/// assert_eq!(lookup_tag_name(0x829A, "ExifIFD"), "ExifIFD:ExposureTime");
/// // Unknown tags return hex format
/// assert_eq!(lookup_tag_name(0xF999, "IFD0"), "IFD0:0xF999");
/// ```
pub fn lookup_tag_name(tag_id: u16, ifd_name: &str) -> String {
    // Determine which format family to look in based on IFD name
    // GPS IFD uses GPS format family, all others use EXIF format family
    let format_family = if ifd_name == "GPS" {
        FormatFamily::GPS
    } else {
        FormatFamily::EXIF
    };

    // Exif.pm:2427-2438 declares these legacy 0x92xx fields as exact
    // duplicates of the standard 0xA2xx entries.  The generated database
    // contains the latter, so use its canonical names when real JPEGs carry
    // the legacy spellings.
    let tag_id = match (format_family, tag_id) {
        (FormatFamily::EXIF, 0x920C) => 0xA20C,
        (FormatFamily::EXIF, 0x920D) => 0xA20D,
        (FormatFamily::EXIF, 0x9215) => 0xA215,
        _ => tag_id,
    };

    // Look up the tag in the appropriate format family
    if let Some(tag_name) = TAG_ID_TO_NAME_INDEX.get(&(tag_id, format_family)) {
        // Found the tag, now we need to replace the prefix with the correct IFD name
        // The generated tags use format family prefixes (EXIF:, GPS:, etc.)
        // but we want to use IFD-specific prefixes for output:
        // - Main IFD (IFD0): Use "IFD0:" prefix for compatibility with Perl ExifTool -G1 output
        // - EXIF Sub-IFD (ExifIFD): Use "ExifIFD:" prefix
        // - GPS Sub-IFD (GPS): Use "GPS:" prefix
        // - Thumbnail IFD (IFD1): Use "IFD1:" prefix
        // - IFD2, IFD3: Use "IFD2:", "IFD3:" prefixes for multi-page TIFF

        if let Some(colon_pos) = tag_name.find(':') {
            let tag_base_name = &tag_name[colon_pos + 1..];
            return format!("{}:{}", ifd_name, tag_base_name);
        }
    }

    // The YAML index missed. Before giving up on a name, consult the manual
    // registry's numeric view: the write path resolves through it, so a tag
    // present only there would otherwise write correctly and read back as hex
    // (the W8 write/read asymmetry). Consulted only after the generated index,
    // so it can never override a generated name.
    if let Some(name) = tag_registry::manual_only_name_for_id(tag_id, format_family, ifd_name) {
        return format!("{ifd_name}:{name}");
    }

    // Fallback: return hex format if tag not found in either database
    format!("{}:0x{:04X}", ifd_name, tag_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Uncompressed` is a genuine `Exif::Main` tag at 0xBC03 -- HD Photo's
    /// compression value -- but it was unreachable, because the enum-value
    /// blocklist that kept PrintConv display strings out of this index also
    /// listed "Uncompressed" verbatim. With the registry corrected at the
    /// source, the blocklist is gone and the tag identifies.
    #[test]
    fn hd_photo_uncompressed_tag_resolves_by_id() {
        assert_eq!(lookup_tag_name(0xBC03, "IFD0"), "IFD0:Uncompressed");
    }

    #[test]
    fn exif_timezone_offset_resolves_by_id() {
        assert_eq!(lookup_tag_name(0x882A, "ExifIFD"), "ExifIFD:TimeZoneOffset");
    }

    #[test]
    fn tiff_ep_standard_id_uses_the_pinned_exiftool_name() {
        assert_eq!(
            lookup_tag_name(0x9216, "ExifIFD"),
            "ExifIFD:TIFF-EPStandardID"
        );
    }

    #[test]
    fn legacy_exif_frequency_and_noise_ids_resolve_to_standard_names() {
        assert_eq!(
            lookup_tag_name(0x920C, "ExifIFD"),
            "ExifIFD:SpatialFrequencyResponse"
        );
        assert_eq!(lookup_tag_name(0x920D, "ExifIFD"), "ExifIFD:Noise");
    }

    /// `Exif.pm:1050-1054` declares 0x151 as TargetPrinter (`Writable =>
    /// 'string'`, `WriteGroup => 'IFD0'`), and oxidex's manual write registry
    /// carries it -- but the YAML-built read index does not, so it used to
    /// write correctly and read back as `IFD0:0x0151`. That hex spelling was
    /// invisible until Step 21 began stripping hex-fallback names from default
    /// output, at which point the tag vanished from read-back entirely and the
    /// jpeg-tag-matrix ratchet caught it (full 136 -> 135).
    #[test]
    fn manual_registry_ids_absent_from_the_yaml_index_still_resolve_by_name() {
        assert_eq!(lookup_tag_name(0x0151, "IFD0"), "IFD0:TargetPrinter");
        // A genuinely unknown ID must still fall through to hex.
        assert_eq!(lookup_tag_name(0xF999, "IFD0"), "IFD0:0xF999");
    }

    #[test]
    fn legacy_exif_exposure_index_id_resolves_to_standard_name() {
        assert_eq!(lookup_tag_name(0x9215, "ExifIFD"), "ExifIFD:ExposureIndex");
    }

    #[test]
    fn fujifilm_sp_2500_legacy_exif_aliases_match_pinned_exiftool() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new(
            "/tmp/oxidex-exiftool-cache/combined-samples/FujiFilm/FujiSP-2500.jpg",
        );
        let metadata = crate::core::operations::read_metadata(path).expect("Fuji SP-2500 parses");

        assert_eq!(
            metadata.get_integer("ExifIFD:SpatialFrequencyResponse"),
            Some(311)
        );
        assert_eq!(metadata.get_integer("ExifIFD:Noise"), Some(6));
    }

    /// The rule that still earns its place. `XMP::Main` is keyed by namespace
    /// prefix, and those route to sub-tables rather than naming a tag, so they
    /// must not claim a numeric id. ExifTool has no lowercase-initial tag name.
    #[test]
    fn xmp_namespace_prefixes_do_not_claim_tag_ids() {
        for prefix in ["x", "mwg-rs", "acdsee-rs", "drone-dji"] {
            assert!(
                !is_valid_tag_name_for_test(prefix),
                "{prefix} should not be indexed as a tag name"
            );
        }
        assert!(is_valid_tag_name_for_test("Uncompressed"));
        assert!(is_valid_tag_name_for_test("ExposureMode"));
    }
}

/// Test-only re-export of the index-build name filter; the real one is a
/// closure-local `fn` inside the `LazyLock`, which tests cannot reach.
#[cfg(test)]
fn is_valid_tag_name_for_test(name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return false;
    };
    !first.is_ascii_lowercase() || name.starts_with("undef") || name.starts_with("n/a")
}
