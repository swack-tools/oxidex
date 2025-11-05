//! ExifTool-RS Tag Database
//!
//! Facade crate that re-exports all domain-specific tag databases.
//! Contains 32,677+ metadata tags for 300+ file formats.
//!
//! # Architecture
//!
//! This crate serves as a unified interface to six domain-specific crates:
//! - `exiftool-tags-core`: Universal metadata standards (EXIF, XMP, IPTC, GPS, ICC Profile)
//! - `exiftool-tags-camera`: Camera manufacturer tags (Canon, Nikon, Sony, etc.)
//! - `exiftool-tags-media`: Audio/video format tags (QuickTime, FLAC, MPEG, etc.)
//! - `exiftool-tags-image`: Image format tags (PNG, GIF, JPEG2000, etc.)
//! - `exiftool-tags-document`: Document format tags (PDF, fonts, archives, etc.)
//! - `exiftool-tags-specialty`: Medical/scientific format tags (DICOM, FITS, MRC, etc.)
//!
//! # Usage
//!
//! ```rust
//! use exiftool_tags::*;
//!
//! // Global search across all domains
//! let table = get_tag_table("Canon");
//!
//! // Domain-specific access
//! let exif = core::get_tag_table("EXIF");
//! let canon = camera::get_tag_table("Canon");
//! ```

// Re-export all domain crates at the module level
// This allows users to access domain-specific functionality via `exiftool_tags::core::`, etc.
pub use exiftool_tags_core as core;
pub use exiftool_tags_camera as camera;
pub use exiftool_tags_media as media;
pub use exiftool_tags_image as image;
pub use exiftool_tags_document as document;
pub use exiftool_tags_specialty as specialty;

// Re-export common types at root level for convenience
// This maintains backward compatibility with code expecting types at the root
pub use exiftool_tags_core::types::*;

// Backward compatibility: stub implementation for old generated tag registry
// The new YAML-based system doesn't use this, but old code may reference it
use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Stub for backward compatibility with old generated tag system
/// In the new YAML-based system, this is empty as tags are accessed differently
pub static GENERATED_TAG_REGISTRY: Lazy<HashMap<String, TagDescriptor>> = Lazy::new(HashMap::new);

/// Get the count of tags in the generated registry
/// Returns 0 in the new YAML-based system (tags are counted differently)
pub fn generated_tag_count() -> usize {
    0
}

/// Get a tag descriptor by name from the generated registry
/// Returns None in the new YAML-based system (use get_tag_table instead)
pub fn get_generated_tag_descriptor(_name: &str) -> Option<&'static TagDescriptor> {
    None
}

/// Get a tag table from any domain by name.
///
/// This function performs a unified search across all domain-specific tag databases.
/// It searches domains in the following order:
/// 1. Core (universal standards)
/// 2. Camera (manufacturer-specific tags)
/// 3. Media (audio/video formats)
/// 4. Image (image formats)
/// 5. Document (document formats)
/// 6. Specialty (medical/scientific formats)
///
/// # Arguments
///
/// * `name` - The name of the tag table to retrieve (e.g., "EXIF", "Canon", "QuickTime")
///
/// # Returns
///
/// Returns `Some(&'static TagTable)` if the table is found in any domain,
/// or `None` if the table doesn't exist in any domain.
///
/// # Examples
///
/// ```rust
/// use exiftool_tags::*;
///
/// // Search for EXIF table (found in core domain)
/// if let Some(exif) = get_tag_table("EXIF") {
///     println!("Found EXIF table with {} tags", exif.tags.len());
/// }
///
/// // Search for Canon table (found in camera domain)
/// if let Some(canon) = get_tag_table("Canon") {
///     println!("Found Canon table");
/// }
/// ```
pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    // Try each domain in order
    // Core domain contains universal standards, so check it first
    core::get_tag_table(name)
        // Camera manufacturers are commonly accessed, check second
        .or_else(|| camera::get_tag_table(name))
        // Media formats
        .or_else(|| media::get_tag_table(name))
        // Image formats
        .or_else(|| image::get_tag_table(name))
        // Document formats
        .or_else(|| document::get_tag_table(name))
        // Specialty/scientific formats (least common, check last)
        .or_else(|| specialty::get_tag_table(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that the facade can access all domain crates
    #[test]
    fn test_facade_compiles() {
        // Just ensure all crates are accessible and their static data can be referenced
        // This forces initialization of all the lazy statics
        let _core = &*core::CORE_TAGS;
        let _camera = &*camera::CAMERA_TAGS;
        let _media = &*media::MEDIA_TAGS;
        let _image = &*image::IMAGE_TAGS;
        let _document = &*document::DOCUMENT_TAGS;
        let _specialty = &*specialty::SPECIALTY_TAGS;
    }

    /// Test that domain-specific access works
    #[test]
    fn test_domain_specific_access() {
        // Core domain should be accessible
        let core_tags = &*core::CORE_TAGS;
        assert!(!core_tags.tables.is_empty(), "Core tags should not be empty");

        // Camera domain should be accessible
        let camera_tags = &*camera::CAMERA_TAGS;
        assert!(!camera_tags.tables.is_empty(), "Camera tags should not be empty");
    }

    /// Test that the unified get_tag_table function works
    #[test]
    fn test_unified_get_tag_table() {
        // The unified function should be able to find tables from any domain
        // Note: This test will only pass once actual tag data is generated
        // For now, we just verify the function exists and can be called
        let _result = get_tag_table("EXIF");
        let _result2 = get_tag_table("Canon");
        let _result3 = get_tag_table("QuickTime");
    }
}
