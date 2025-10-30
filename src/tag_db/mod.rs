//! Generated tag database
//!
//! Contains the tag registry and generated tag definitions from ExifTool specifications.

#![allow(dead_code)]

pub mod generated_tags;
pub mod tag_registry;

use crate::core::tag_descriptor::{FormatFamily, TagId};
use generated_tags::GENERATED_TAG_REGISTRY;
use once_cell::sync::Lazy;
use std::collections::HashMap;

// Re-export commonly used registry functions
pub use tag_registry::{get_tag_descriptor, tag_count};

/// Reverse lookup index: (numeric tag ID, format family) -> tag name
/// Built lazily on first access from the generated tag registry
static TAG_ID_TO_NAME_INDEX: Lazy<HashMap<(u16, FormatFamily), &'static str>> = Lazy::new(|| {
    let mut index = HashMap::with_capacity(733);

    // Scan the generated tag registry and build reverse index
    // Store tags by (tag_id, format_family) to handle same IDs across different formats
    for (name, descriptor) in GENERATED_TAG_REGISTRY.iter() {
        if let TagId::Numeric(id) = descriptor.id() {
            index.insert((*id, descriptor.format()), *name);
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
/// assert_eq!(lookup_tag_name(0x829A, "ExifIFD"), "ExifIFD:ExposureTime");
/// assert_eq!(lookup_tag_name(0x0002, "GPS"), "GPS:Latitude");
/// assert_eq!(lookup_tag_name(0xFFFF, "IFD0"), "IFD0:0xFFFF");
/// ```
pub fn lookup_tag_name(tag_id: u16, ifd_name: &str) -> String {
    // Try to find the tag in the EXIF format family (most common for TIFF/JPEG)
    if let Some(tag_name) = TAG_ID_TO_NAME_INDEX.get(&(tag_id, FormatFamily::EXIF)) {
        // Found the tag, now we need to replace the prefix with the correct IFD name
        // The generated tags use "EXIF:" prefix, but we want to use IFD-specific prefixes

        // Handle the naming convention:
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

    // If not found in EXIF family, try GPS family for GPS IFD
    if ifd_name == "GPS" {
        if let Some(tag_name) = TAG_ID_TO_NAME_INDEX.get(&(tag_id, FormatFamily::GPS)) {
            if let Some(colon_pos) = tag_name.find(':') {
                let tag_base_name = &tag_name[colon_pos + 1..];
                return format!("GPS:{}", tag_base_name);
            }
        }
    }

    // Fallback: return hex format if tag not found in database
    format!("{}:0x{:04X}", ifd_name, tag_id)
}
