//! MetadataMap structure for storing extracted metadata
//!
//! This module defines the core MetadataMap data structure.

#![allow(dead_code)]

use super::tag_value::TagValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A collection of metadata tags extracted from a file.
///
/// MetadataMap stores key-value pairs where keys are tag names (e.g., "EXIF:Make")
/// and values are TagValue enums that can represent different data types.
///
/// This structure is the primary in-memory representation of file metadata
/// and can be serialized to JSON for output or deserialized from existing data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetadataMap {
    /// Internal storage mapping tag names to their values
    #[serde(flatten)]
    tags: HashMap<String, TagValue>,
}

impl MetadataMap {
    /// Creates a new empty MetadataMap
    ///
    /// # Examples
    ///
    /// ```
    /// use exiftool_rs::core::metadata_map::MetadataMap;
    ///
    /// let metadata = MetadataMap::new();
    /// assert_eq!(metadata.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            tags: HashMap::new(),
        }
    }

    /// Creates a new MetadataMap with the specified capacity
    ///
    /// This pre-allocates space for at least `capacity` tags, which can
    /// improve performance when the approximate number of tags is known.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            tags: HashMap::with_capacity(capacity),
        }
    }

    /// Inserts a tag into the metadata map
    ///
    /// If the tag already exists, its value is replaced and the old value is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use exiftool_rs::core::metadata_map::MetadataMap;
    /// use exiftool_rs::core::tag_value::TagValue;
    ///
    /// let mut metadata = MetadataMap::new();
    /// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
    /// ```
    pub fn insert<K: Into<String>>(&mut self, key: K, value: TagValue) -> Option<TagValue> {
        self.tags.insert(key.into(), value)
    }

    /// Retrieves a tag value by name
    ///
    /// Returns `None` if the tag doesn't exist.
    pub fn get(&self, key: &str) -> Option<&TagValue> {
        self.tags.get(key)
    }

    /// Retrieves a mutable reference to a tag value by name
    ///
    /// Returns `None` if the tag doesn't exist.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut TagValue> {
        self.tags.get_mut(key)
    }

    /// Removes a tag from the map
    ///
    /// Returns the value if the tag existed, `None` otherwise.
    pub fn remove(&mut self, key: &str) -> Option<TagValue> {
        self.tags.remove(key)
    }

    /// Checks if a tag exists in the map
    pub fn contains_key(&self, key: &str) -> bool {
        self.tags.contains_key(key)
    }

    /// Returns the number of tags in the map
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    /// Returns true if the map contains no tags
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// Clears all tags from the map
    pub fn clear(&mut self) {
        self.tags.clear();
    }

    /// Returns an iterator over tag names and values
    pub fn iter(&self) -> impl Iterator<Item = (&String, &TagValue)> {
        self.tags.iter()
    }

    /// Returns an iterator over tag names
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.tags.keys()
    }

    /// Returns an iterator over tag values
    pub fn values(&self) -> impl Iterator<Item = &TagValue> {
        self.tags.values()
    }

    /// Typed getter for string values
    ///
    /// Returns `None` if the tag doesn't exist or isn't a String variant.
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.as_string())
    }

    /// Typed getter for integer values
    ///
    /// Returns `None` if the tag doesn't exist or isn't an Integer variant.
    pub fn get_integer(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.as_integer())
    }

    /// Typed getter for float values
    ///
    /// Returns `None` if the tag doesn't exist or isn't a Float variant.
    pub fn get_float(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|v| v.as_float())
    }
}

impl Default for MetadataMap {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIterator<(String, TagValue)> for MetadataMap {
    fn from_iter<T: IntoIterator<Item = (String, TagValue)>>(iter: T) -> Self {
        Self {
            tags: HashMap::from_iter(iter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metadata_map() {
        let map = MetadataMap::new();
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn test_insert_and_get() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));

        assert_eq!(map.len(), 1);
        assert!(!map.is_empty());
        assert_eq!(map.get_string("EXIF:Make"), Some("Canon"));
    }

    #[test]
    fn test_insert_multiple_tags() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Nikon"));
        map.insert("EXIF:Model", TagValue::new_string("D850"));
        map.insert("EXIF:ISO", TagValue::new_integer(400));

        assert_eq!(map.len(), 3);
        assert_eq!(map.get_string("EXIF:Make"), Some("Nikon"));
        assert_eq!(map.get_string("EXIF:Model"), Some("D850"));
        assert_eq!(map.get_integer("EXIF:ISO"), Some(400));
    }

    #[test]
    fn test_replace_existing_tag() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        let old = map.insert("EXIF:Make", TagValue::new_string("Sony"));

        assert_eq!(
            old.and_then(|v| v.as_string().map(String::from)),
            Some("Canon".to_string())
        );
        assert_eq!(map.get_string("EXIF:Make"), Some("Sony"));
    }

    #[test]
    fn test_remove_tag() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        assert_eq!(map.len(), 1);

        let removed = map.remove("EXIF:Make");
        assert!(removed.is_some());
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn test_contains_key() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));

        assert!(map.contains_key("EXIF:Make"));
        assert!(!map.contains_key("EXIF:Model"));
    }

    #[test]
    fn test_clear() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        map.insert("EXIF:Model", TagValue::new_string("EOS R5"));

        assert_eq!(map.len(), 2);
        map.clear();
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn test_typed_getters() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        map.insert("EXIF:ISO", TagValue::new_integer(800));
        map.insert("EXIF:FNumber", TagValue::new_float(2.8));

        assert_eq!(map.get_string("EXIF:Make"), Some("Canon"));
        assert_eq!(map.get_integer("EXIF:ISO"), Some(800));
        assert_eq!(map.get_float("EXIF:FNumber"), Some(2.8));

        // Wrong type should return None
        assert_eq!(map.get_integer("EXIF:Make"), None);
        assert_eq!(map.get_string("EXIF:ISO"), None);
    }

    #[test]
    fn test_clone() {
        let mut map1 = MetadataMap::new();
        map1.insert("EXIF:Make", TagValue::new_string("Canon"));

        let map2 = map1.clone();
        assert_eq!(map1, map2);
        assert_eq!(map2.get_string("EXIF:Make"), Some("Canon"));
    }

    #[test]
    fn test_debug() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));

        let debug_str = format!("{:?}", map);
        assert!(debug_str.contains("MetadataMap"));
    }

    #[test]
    fn test_serde_serialization() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        map.insert("EXIF:ISO", TagValue::new_integer(400));

        let json = serde_json::to_string(&map).unwrap();
        assert!(json.contains("EXIF:Make"));
        assert!(json.contains("Canon"));
        assert!(json.contains("EXIF:ISO"));
    }

    #[test]
    fn test_serde_deserialization() {
        let json = r#"{"EXIF:Make":{"type":"String","value":"Nikon"},"EXIF:ISO":{"type":"Integer","value":800}}"#;
        let map: MetadataMap = serde_json::from_str(json).unwrap();

        assert_eq!(map.len(), 2);
        assert_eq!(map.get_string("EXIF:Make"), Some("Nikon"));
        assert_eq!(map.get_integer("EXIF:ISO"), Some(800));
    }

    #[test]
    fn test_from_iterator() {
        let tags = vec![
            ("EXIF:Make".to_string(), TagValue::new_string("Canon")),
            ("EXIF:Model".to_string(), TagValue::new_string("EOS R5")),
        ];

        let map: MetadataMap = tags.into_iter().collect();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get_string("EXIF:Make"), Some("Canon"));
    }
}
