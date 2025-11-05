//! Generated tag database
//!
//! Re-exports tag definitions from the exiftool-tags crate

#![allow(dead_code)]

pub mod tag_registry;
pub mod generated_tags;

// Re-export everything from exiftool-tags crate
pub use exiftool_tags::*;

use once_cell::sync::Lazy;
use std::collections::HashMap;

// Re-export commonly used registry functions
pub use tag_registry::{get_tag_descriptor, tag_count};

/// Reverse lookup index: (numeric tag ID, format family) -> tag name
/// Built lazily on first access from the YAML-based tag databases
static TAG_ID_TO_NAME_INDEX: Lazy<HashMap<(u16, FormatFamily), String>> = Lazy::new(|| {
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
/// use exiftool_rs::tag_db::lookup_tag_name;
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

    // Fallback: return hex format if tag not found in database
    format!("{}:0x{:04X}", ifd_name, tag_id)
}
