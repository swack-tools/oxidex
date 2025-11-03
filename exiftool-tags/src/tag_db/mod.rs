//! Tag database types and generated tags module

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

pub mod generated_tags;

// Re-export the generated tag API
pub use generated_tags::{
    get_generated_tag_descriptor, generated_tag_count, GENERATED_TAG_REGISTRY,
};

/// Tag identifier (numeric or string-based)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TagId {
    Numeric(u16),
    Named(String),
}

impl TagId {
    pub fn new_numeric(id: u16) -> Self {
        TagId::Numeric(id)
    }

    pub fn new_named<S: Into<String>>(id: S) -> Self {
        TagId::Named(id.into())
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, TagId::Numeric(_))
    }

    pub fn is_named(&self) -> bool {
        matches!(self, TagId::Named(_))
    }
}

/// Metadata format family
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormatFamily {
    EXIF,
    XMP,
    IPTC,
    GPS,
    #[serde(rename = "ICC_Profile")]
    ICCProfile,
    Photoshop,
    MakerNotes,
    JFIF,
    JPEG,
    PNG,
    PDF,
    QuickTime,
    TIFF,
    RIFF,
    PostScript,
}

/// Value type for tag values
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueType {
    String,
    Integer,
    Float,
    Rational,
    Binary,
    DateTime,
    Struct,
}

/// Complete tag descriptor
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TagDescriptor {
    pub tag_id: TagId,
    pub tag_name: String,
    pub format_family: FormatFamily,
    pub writable: bool,
    pub value_type: ValueType,
    pub description: String,
    pub example_values: Vec<String>,
}

impl TagDescriptor {
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

    pub fn id(&self) -> &TagId {
        &self.tag_id
    }

    pub fn name(&self) -> &str {
        &self.tag_name
    }

    pub fn format(&self) -> FormatFamily {
        self.format_family
    }

    pub fn is_writable(&self) -> bool {
        self.writable
    }

    pub fn value_type(&self) -> ValueType {
        self.value_type
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn examples(&self) -> &[String] {
        &self.example_values
    }
}
