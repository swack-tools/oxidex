//! TagDescriptor structure for tag definitions
//!
//! This module defines the TagDescriptor struct for metadata tag descriptions.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Unique identifier for a metadata tag.
///
/// Tags can be identified either by a numeric ID (common in EXIF/TIFF)
/// or by a string ID (common in XMP).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TagId {
    /// Numeric tag ID (e.g., 0x010F for EXIF Make tag)
    Numeric(u16),
    /// String-based tag ID (e.g., "XMP-dc:Creator")
    Named(String),
}

impl TagId {
    /// Creates a new numeric TagId
    pub fn new_numeric(id: u16) -> Self {
        TagId::Numeric(id)
    }

    /// Creates a new named TagId
    pub fn new_named<S: Into<String>>(id: S) -> Self {
        TagId::Named(id.into())
    }

    /// Returns true if this is a numeric ID
    pub fn is_numeric(&self) -> bool {
        matches!(self, TagId::Numeric(_))
    }

    /// Returns true if this is a named ID
    pub fn is_named(&self) -> bool {
        matches!(self, TagId::Named(_))
    }
}

/// Metadata format family classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormatFamily {
    /// EXIF (Exchangeable Image File Format) metadata
    EXIF,
    /// XMP (Extensible Metadata Platform) metadata
    XMP,
    /// IPTC (International Press Telecommunications Council) metadata
    IPTC,
    /// GPS (Global Positioning System) metadata
    GPS,
    /// ICC Profile (International Color Consortium) metadata
    #[serde(rename = "ICC_Profile")]
    ICCProfile,
    /// Adobe Photoshop metadata
    Photoshop,
    /// Camera-specific maker notes
    MakerNotes,
    /// JFIF (JPEG File Interchange Format) metadata
    JFIF,
    /// JPEG (Joint Photographic Experts Group) metadata
    JPEG,
    /// PNG (Portable Network Graphics) metadata
    PNG,
    /// PDF (Portable Document Format) metadata
    PDF,
    /// QuickTime/MOV metadata
    QuickTime,
    /// TIFF (Tagged Image File Format) metadata
    TIFF,
    /// RIFF (Resource Interchange File Format) metadata
    RIFF,
    /// PostScript metadata
    PostScript,
}

/// Value type enumeration corresponding to TagValue variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueType {
    /// String value type
    String,
    /// Integer value type
    Integer,
    /// Float value type
    Float,
    /// Rational number value type
    Rational,
    /// Binary data value type
    Binary,
    /// Date/time value type
    DateTime,
    /// Structured/nested value type
    Struct,
}

/// Complete descriptor for a metadata tag definition.
///
/// This structure contains all information needed to understand, validate,
/// and process a metadata tag. Tag descriptors are typically loaded from
/// the tag database at compile time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TagDescriptor {
    /// Unique identifier for this tag (numeric or string-based)
    pub tag_id: TagId,

    /// Full canonical name of the tag (e.g., "EXIF:Make", "XMP-dc:Creator")
    pub tag_name: String,

    /// Metadata format family this tag belongs to
    pub format_family: FormatFamily,

    /// Whether this tag can be written back to files
    pub writable: bool,

    /// Expected data type for this tag's value
    pub value_type: ValueType,

    /// Human-readable description of the tag's purpose
    pub description: String,

    /// Example values demonstrating typical content
    pub example_values: Vec<String>,
}

impl TagDescriptor {
    /// Creates a new TagDescriptor with all required fields
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tag_id: TagId,
        tag_name: String,
        format_family: FormatFamily,
        writable: bool,
        value_type: ValueType,
        description: String,
        example_values: Vec<String>,
    ) -> Self {
        Self {
            tag_id,
            tag_name,
            format_family,
            writable,
            value_type,
            description,
            example_values,
        }
    }

    /// Returns the tag's unique identifier
    pub fn id(&self) -> &TagId {
        &self.tag_id
    }

    /// Returns the tag's canonical name
    pub fn name(&self) -> &str {
        &self.tag_name
    }

    /// Returns the format family this tag belongs to
    pub fn format(&self) -> FormatFamily {
        self.format_family
    }

    /// Returns whether this tag is writable
    pub fn is_writable(&self) -> bool {
        self.writable
    }

    /// Returns the expected value type for this tag
    pub fn value_type(&self) -> ValueType {
        self.value_type
    }

    /// Returns the tag's description
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns example values for this tag
    pub fn examples(&self) -> &[String] {
        &self.example_values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_id_numeric_creation() {
        let id = TagId::new_numeric(271);
        assert!(id.is_numeric());
        assert!(!id.is_named());
        assert_eq!(id, TagId::Numeric(271));
    }

    #[test]
    fn test_tag_id_named_creation() {
        let id = TagId::new_named("XMP-dc:Creator");
        assert!(id.is_named());
        assert!(!id.is_numeric());
        match id {
            TagId::Named(s) => assert_eq!(s, "XMP-dc:Creator"),
            _ => panic!("Expected Named variant"),
        }
    }

    #[test]
    fn test_tag_descriptor_exif_make() {
        let descriptor = TagDescriptor::new(
            TagId::new_numeric(271),
            "EXIF:Make".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Manufacturer of the recording equipment".to_string(),
            vec!["Canon".to_string(), "Nikon".to_string(), "Sony".to_string()],
        );

        assert_eq!(descriptor.name(), "EXIF:Make");
        assert_eq!(descriptor.format(), FormatFamily::EXIF);
        assert!(descriptor.is_writable());
        assert_eq!(descriptor.value_type(), ValueType::String);
        assert_eq!(descriptor.examples().len(), 3);
    }

    #[test]
    fn test_tag_descriptor_xmp_creator() {
        let descriptor = TagDescriptor::new(
            TagId::new_named("XMP-dc:Creator"),
            "XMP-dc:Creator".to_string(),
            FormatFamily::XMP,
            true,
            ValueType::String,
            "Creator or author of the document".to_string(),
            vec!["John Doe".to_string(), "Jane Smith".to_string()],
        );

        assert_eq!(descriptor.name(), "XMP-dc:Creator");
        assert_eq!(descriptor.format(), FormatFamily::XMP);
        assert!(descriptor.is_writable());
    }

    #[test]
    fn test_format_family_variants() {
        let families = [
            FormatFamily::EXIF,
            FormatFamily::XMP,
            FormatFamily::IPTC,
            FormatFamily::GPS,
            FormatFamily::ICCProfile,
            FormatFamily::Photoshop,
            FormatFamily::MakerNotes,
            FormatFamily::JFIF,
            FormatFamily::JPEG,
            FormatFamily::PNG,
            FormatFamily::PDF,
            FormatFamily::QuickTime,
            FormatFamily::TIFF,
            FormatFamily::RIFF,
            FormatFamily::PostScript,
        ];
        assert_eq!(families.len(), 15);
    }

    #[test]
    fn test_value_type_variants() {
        let types = [
            ValueType::String,
            ValueType::Integer,
            ValueType::Float,
            ValueType::Rational,
            ValueType::Binary,
            ValueType::DateTime,
            ValueType::Struct,
        ];
        assert_eq!(types.len(), 7);
    }

    #[test]
    fn test_tag_descriptor_clone() {
        let desc1 = TagDescriptor::new(
            TagId::new_numeric(1),
            "GPS:GPSLatitudeRef".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::String,
            "Latitude reference".to_string(),
            vec!["N".to_string(), "S".to_string()],
        );
        let desc2 = desc1.clone();
        assert_eq!(desc1, desc2);
    }

    #[test]
    fn test_tag_descriptor_debug() {
        let descriptor = TagDescriptor::new(
            TagId::new_numeric(271),
            "EXIF:Make".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Camera manufacturer".to_string(),
            vec!["Canon".to_string()],
        );
        let debug_str = format!("{:?}", descriptor);
        assert!(debug_str.contains("EXIF:Make"));
        assert!(debug_str.contains("Canon"));
    }

    #[test]
    fn test_tag_descriptor_serde() {
        let descriptor = TagDescriptor::new(
            TagId::new_numeric(271),
            "EXIF:Make".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Manufacturer of the recording equipment".to_string(),
            vec!["Canon".to_string()],
        );

        let json = serde_json::to_string(&descriptor).unwrap();
        let deserialized: TagDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(descriptor, deserialized);
    }
}
