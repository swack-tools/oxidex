//! Validation logic for TIFF metadata
//!
//! This module provides validation functions for ensuring metadata can be
//! correctly written to TIFF format. It checks tag families, tag IDs, and
//! value compatibility.

use crate::core::TagId;
use crate::core::metadata_map::MetadataMap;
use crate::error::{ExifToolError, Result};
use crate::tag_db::tag_registry;

/// Checks if a tag family is writable to TIFF format.
///
/// TIFF files support these families:
/// - IFD0, IFD1 (main image and thumbnail metadata)
/// - ExifIFD (EXIF-specific tags)
/// - GPS (GPS location data)
/// - EXIF (generic EXIF tags)
/// - InteropIFD (interoperability data)
/// - MakerNotes (manufacturer-specific data)
///
/// Other families like XMP, IPTC, QuickTime, etc. are not writable to TIFF.
pub fn is_tiff_writable_family(tag_name: &str) -> bool {
    tag_name.starts_with("IFD0:")
        || tag_name.starts_with("IFD1:")
        || tag_name.starts_with("ExifIFD:")
        || tag_name.starts_with("GPS:")
        || tag_name.starts_with("EXIF:")
        || tag_name.starts_with("InteropIFD:")
        || tag_name.starts_with("MakerNotes:")
}

/// Validates that a tag can be written to TIFF format.
///
/// Checks:
/// 1. Tag family is TIFF-writable
/// 2. Tag exists in the registry
/// 3. Tag has a numeric ID (required for TIFF)
///
/// # Parameters
///
/// - `tag_name`: Full tag name (e.g., "EXIF:Make")
///
/// # Returns
///
/// - `Ok(tag_id)`: Tag is valid and can be written, returns the numeric tag ID
/// - `Err`: Tag cannot be written (invalid family, unknown tag, or non-numeric ID)
pub fn validate_tag_for_tiff(tag_name: &str) -> Result<u16> {
    // Check if family is writable
    if !is_tiff_writable_family(tag_name) {
        return Err(ExifToolError::unsupported_format(format!(
            "Tag {} is not writable to TIFF format (unsupported family)",
            tag_name
        )));
    }

    // Look up tag in registry
    let tag_descriptor = tag_registry::get_tag_descriptor(tag_name)
        .ok_or_else(|| ExifToolError::unsupported_format(format!("Unknown tag: {}", tag_name)))?;

    // Extract numeric tag ID
    match &tag_descriptor.tag_id {
        TagId::Numeric(id) => Ok(*id),
        TagId::Named(_) => Err(ExifToolError::unsupported_format(format!(
            "Tag {} has non-numeric ID (not supported for TIFF serialization)",
            tag_name
        ))),
    }
}

/// Filters a MetadataMap to include only TIFF-writable tags.
///
/// Returns a new MetadataMap containing only tags that can be written to TIFF format.
/// Non-writable tags (XMP, IPTC, etc.) are silently filtered out.
///
/// # Parameters
///
/// - `metadata`: Source metadata to filter
///
/// # Returns
///
/// A new MetadataMap containing only TIFF-writable tags
pub fn filter_tiff_writable_tags(metadata: &MetadataMap) -> MetadataMap {
    let mut filtered = MetadataMap::new();

    for (tag_name, tag_value) in metadata.iter() {
        if is_tiff_writable_family(tag_name) {
            filtered.insert(tag_name.clone(), tag_value.clone());
        }
    }

    filtered
}

/// Separates metadata into IFD-specific collections.
///
/// TIFF files organize metadata into different IFDs (Image File Directories).
/// This function separates tags based on their family prefix:
/// - IFD0 tags: Main image metadata (IFD0:, EXIF:, IFD1:)
/// - ExifIFD tags: EXIF-specific tags (ExifIFD:)
/// - GPS tags: GPS location data (GPS:)
///
/// # Parameters
///
/// - `metadata`: Source metadata to separate
///
/// # Returns
///
/// Tuple of (ifd0_metadata, exif_ifd_metadata, gps_ifd_metadata)
pub fn separate_by_ifd(metadata: &MetadataMap) -> (MetadataMap, MetadataMap, MetadataMap) {
    let mut ifd0_metadata = MetadataMap::new();
    let mut exif_ifd_metadata = MetadataMap::new();
    let mut gps_ifd_metadata = MetadataMap::new();

    for (tag_name, tag_value) in metadata.iter() {
        if tag_name.starts_with("ExifIFD:") {
            exif_ifd_metadata.insert(tag_name.clone(), tag_value.clone());
        } else if tag_name.starts_with("GPS:") {
            gps_ifd_metadata.insert(tag_name.clone(), tag_value.clone());
        } else if tag_name.starts_with("IFD0:")
            || tag_name.starts_with("EXIF:")
            || tag_name.starts_with("IFD1:")
        {
            ifd0_metadata.insert(tag_name.clone(), tag_value.clone());
        }
    }

    (ifd0_metadata, exif_ifd_metadata, gps_ifd_metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;

    #[test]
    fn test_is_tiff_writable_family() {
        assert!(is_tiff_writable_family("EXIF:Make"));
        assert!(is_tiff_writable_family("IFD0:Model"));
        assert!(is_tiff_writable_family("ExifIFD:ISO"));
        assert!(is_tiff_writable_family("GPS:Latitude"));

        assert!(!is_tiff_writable_family("XMP:Creator"));
        assert!(!is_tiff_writable_family("IPTC:Keywords"));
        assert!(!is_tiff_writable_family("QuickTime:Duration"));
    }

    #[test]
    fn test_filter_tiff_writable_tags() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("XMP:Creator", TagValue::new_string("John"));
        metadata.insert("GPS:Latitude", TagValue::new_string("37.7749"));

        let filtered = filter_tiff_writable_tags(&metadata);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.get("EXIF:Make").is_some());
        assert!(filtered.get("GPS:Latitude").is_some());
        assert!(filtered.get("XMP:Creator").is_none());
    }

    #[test]
    fn test_separate_by_ifd() {
        let mut metadata = MetadataMap::new();
        metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
        metadata.insert("ExifIFD:ISO", TagValue::new_integer(400));
        metadata.insert("GPS:Latitude", TagValue::new_string("37.7749"));

        let (ifd0, exif_ifd, gps_ifd) = separate_by_ifd(&metadata);

        assert_eq!(ifd0.len(), 1);
        assert!(ifd0.get("EXIF:Make").is_some());

        assert_eq!(exif_ifd.len(), 1);
        assert!(exif_ifd.get("ExifIFD:ISO").is_some());

        assert_eq!(gps_ifd.len(), 1);
        assert!(gps_ifd.get("GPS:Latitude").is_some());
    }
}
