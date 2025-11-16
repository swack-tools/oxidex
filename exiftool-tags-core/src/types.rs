use serde::{Deserialize, Serialize};

/// Represents a single metadata tag definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tag {
    /// Tag ID (numeric or string)
    pub id: String,
    /// Tag name
    pub name: String,
    /// Whether the tag is writable
    pub writable: bool,
    /// Data type (e.g., "int16u", "string")
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    /// Human-readable description
    pub description: Option<String>,
}

/// Represents a table of related tags
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TagTable {
    /// Table name (e.g., "EXIF", "Canon", "QuickTime")
    pub name: String,
    /// Tags in this table
    pub tags: Vec<Tag>,
}

/// Database containing multiple tag tables
#[derive(Debug, Deserialize, Serialize)]
pub struct TagDatabase {
    /// All tag tables in this domain
    pub tables: Vec<TagTable>,
}

// ============================================================================
// Backward Compatibility Types
// These types maintain API compatibility with the old generated tag system
// ============================================================================

/// Tag identifier (numeric or string-based)
/// Maintained for backward compatibility with existing code
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
/// Maintained for backward compatibility with existing code
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
/// Maintained for backward compatibility with existing code
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
/// Maintained for backward compatibility with existing code
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
