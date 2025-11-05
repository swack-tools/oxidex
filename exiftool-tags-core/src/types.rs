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
#[derive(Debug, Deserialize)]
pub struct TagDatabase {
    /// All tag tables in this domain
    pub tables: Vec<TagTable>,
}
