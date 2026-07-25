use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub writable: bool,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TagTable {
    pub name: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TagDatabase {
    pub tables: Vec<TagTable>,
}

#[derive(Debug, Error)]
pub enum LookupError {
    #[error("tag table '{0}' not found")]
    NotFound(String),
}

pub fn find_table<'a>(db: &'a TagDatabase, name: &str) -> Result<&'a TagTable, LookupError> {
    db.tables
        .iter()
        .find(|table| table.name == name)
        .ok_or_else(|| LookupError::NotFound(name.to_string()))
}
