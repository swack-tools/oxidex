//! High-level metadata API with ergonomic builder pattern
//!
//! This module provides a user-friendly interface for reading, modifying,
//! and writing file metadata. It wraps the lower-level MetadataMap and
//! operations functions with a more intuitive API.
//!
//! # Examples
//!
//! ```no_run
//! use oxidex::Metadata;
//!
//! // Read and display metadata
//! let metadata = Metadata::from_path("photo.jpg").unwrap();
//! if let Some(make) = metadata.get_string("EXIF:Make") {
//!     println!("Camera: {}", make);
//! }
//!
//! // Modify and save metadata
//! Metadata::from_path("photo.jpg").unwrap()
//!     .set_tag("EXIF:Artist", "John Doe")
//!     .set_tag("EXIF:Copyright", "2025 John Doe")
//!     .save()
//!     .unwrap();
//! ```

use crate::core::operations::{read_metadata, write_metadata};
use crate::core::{MetadataMap, TagValue};
use crate::error::Result;
use std::path::{Path, PathBuf};

/// High-level metadata container with ergonomic API
///
/// `Metadata` provides a builder-pattern interface for working with
/// file metadata. It wraps the underlying `MetadataMap` with convenient
/// methods for common operations.
///
/// # Examples
///
/// ```no_run
/// use oxidex::Metadata;
///
/// // Read metadata from a file
/// let meta = Metadata::from_path("photo.jpg")?;
///
/// // Access typed values
/// if let Some(iso) = meta.get_integer("EXIF:ISO") {
///     println!("ISO: {}", iso);
/// }
/// # Ok::<(), oxidex::error::ExifToolError>(())
/// ```
pub struct Metadata {
    map: MetadataMap,
    source_path: Option<PathBuf>,
}

impl Metadata {
    /// Creates a new empty Metadata container
    ///
    /// # Examples
    ///
    /// ```
    /// use oxidex::Metadata;
    ///
    /// let metadata = Metadata::new();
    /// assert!(metadata.is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            map: MetadataMap::new(),
            source_path: None,
        }
    }

    /// Reads metadata from a file path
    ///
    /// This is the primary way to load metadata from an existing file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to read
    ///
    /// # Returns
    ///
    /// * `Ok(Metadata)` - Successfully loaded metadata
    /// * `Err` - I/O error or unsupported format
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use oxidex::Metadata;
    ///
    /// let metadata = Metadata::from_path("photo.jpg")?;
    /// println!("Found {} tags", metadata.len());
    /// # Ok::<(), oxidex::error::ExifToolError>(())
    /// ```
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let map = read_metadata(path)?;
        Ok(Self {
            map,
            source_path: Some(path.to_path_buf()),
        })
    }

    /// Gets a string value by tag name
    ///
    /// Returns `None` if the tag doesn't exist or isn't a string type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use oxidex::Metadata;
    ///
    /// let meta = Metadata::from_path("photo.jpg")?;
    /// if let Some(make) = meta.get_string("EXIF:Make") {
    ///     println!("Camera: {}", make);
    /// }
    /// # Ok::<(), oxidex::error::ExifToolError>(())
    /// ```
    pub fn get_string(&self, tag: &str) -> Option<&str> {
        self.map.get(tag).and_then(|v| v.as_string())
    }

    /// Gets an integer value by tag name
    ///
    /// Returns `None` if the tag doesn't exist or isn't an integer type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use oxidex::Metadata;
    ///
    /// let meta = Metadata::from_path("photo.jpg")?;
    /// if let Some(iso) = meta.get_integer("EXIF:ISO") {
    ///     println!("ISO: {}", iso);
    /// }
    /// # Ok::<(), oxidex::error::ExifToolError>(())
    /// ```
    pub fn get_integer(&self, tag: &str) -> Option<i64> {
        self.map.get(tag).and_then(|v| v.as_integer())
    }

    /// Gets a float value by tag name
    ///
    /// Returns `None` if the tag doesn't exist or isn't a float type.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use oxidex::Metadata;
    ///
    /// let meta = Metadata::from_path("photo.jpg")?;
    /// if let Some(fnumber) = meta.get_float("EXIF:FNumber") {
    ///     println!("F-Number: f/{:.1}", fnumber);
    /// }
    /// # Ok::<(), oxidex::error::ExifToolError>(())
    /// ```
    pub fn get_float(&self, tag: &str) -> Option<f64> {
        self.map.get(tag).and_then(|v| v.as_float())
    }

    /// Gets a raw TagValue by tag name
    ///
    /// Use this for accessing tags with complex types (Rational, Binary, etc.)
    pub fn get(&self, tag: &str) -> Option<&TagValue> {
        self.map.get(tag)
    }

    /// Checks if a tag exists
    pub fn has_tag(&self, tag: &str) -> bool {
        self.map.contains_key(tag)
    }

    /// Sets a tag value (builder pattern)
    ///
    /// This method consumes self and returns it, enabling chained calls.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use oxidex::Metadata;
    ///
    /// Metadata::from_path("photo.jpg")?
    ///     .set_tag("EXIF:Artist", "John Doe")
    ///     .set_tag("EXIF:Copyright", "2025")
    ///     .save()?;
    /// # Ok::<(), oxidex::error::ExifToolError>(())
    /// ```
    pub fn set_tag<V: Into<TagValue>>(mut self, tag: &str, value: V) -> Self {
        self.map.insert(tag, value.into());
        self
    }

    /// Sets a tag value without consuming self
    ///
    /// Use this for imperative-style modifications.
    pub fn insert<V: Into<TagValue>>(&mut self, tag: &str, value: V) {
        self.map.insert(tag, value.into());
    }

    /// Removes a tag
    pub fn remove(&mut self, tag: &str) -> Option<TagValue> {
        self.map.remove(tag)
    }

    /// Writes metadata to a specific file
    ///
    /// # Arguments
    ///
    /// * `path` - Destination file path
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use oxidex::Metadata;
    ///
    /// let meta = Metadata::from_path("source.jpg")?;
    /// meta.write_to("output.jpg")?;
    /// # Ok::<(), oxidex::error::ExifToolError>(())
    /// ```
    pub fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        write_metadata(path.as_ref(), &self.map)
    }

    /// Saves metadata back to the source file
    ///
    /// This method only works if the Metadata was created with `from_path()`.
    /// Use `write_to()` to write to a different file.
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Successfully saved
    /// * `Err` - No source path or I/O error
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use oxidex::Metadata;
    ///
    /// Metadata::from_path("photo.jpg")?
    ///     .set_tag("EXIF:Artist", "John Doe")
    ///     .save()?;
    /// # Ok::<(), oxidex::error::ExifToolError>(())
    /// ```
    pub fn save(&self) -> Result<()> {
        match &self.source_path {
            Some(path) => write_metadata(path, &self.map),
            None => Err(crate::error::ExifToolError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No source path - use write_to() instead",
            ))),
        }
    }

    /// Creates a copy builder for copying metadata to another file
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use oxidex::Metadata;
    ///
    /// // Copy all metadata
    /// Metadata::from_path("source.jpg")?
    ///     .copy_to("dest.jpg")?
    ///     .execute()?;
    ///
    /// // Copy specific tags
    /// Metadata::from_path("source.jpg")?
    ///     .copy_to("dest.jpg")?
    ///     .with_tags(&["EXIF:Make", "EXIF:Model"])?
    ///     .execute()?;
    /// # Ok::<(), oxidex::error::ExifToolError>(())
    /// ```
    pub fn copy_to<P: AsRef<Path>>(&self, dest: P) -> Result<CopyBuilder<'_>> {
        Ok(CopyBuilder {
            source: self,
            dest: dest.as_ref().to_path_buf(),
            tags: None,
        })
    }

    /// Returns the number of tags
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns true if empty
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Iterates over all tags
    pub fn iter(&self) -> impl Iterator<Item = (&String, &TagValue)> {
        self.map.iter()
    }

    /// Returns the source file path, if any
    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    /// Returns a reference to the underlying MetadataMap
    pub fn as_map(&self) -> &MetadataMap {
        &self.map
    }

    /// Consumes self and returns the underlying MetadataMap
    pub fn into_map(self) -> MetadataMap {
        self.map
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for copy operations
pub struct CopyBuilder<'a> {
    source: &'a Metadata,
    dest: PathBuf,
    tags: Option<Vec<String>>,
}

impl<'a> CopyBuilder<'a> {
    /// Filter to copy only specific tags
    ///
    /// If not called, all tags are copied.
    pub fn with_tags(mut self, tags: &[&str]) -> Result<Self> {
        self.tags = Some(tags.iter().map(|s| s.to_string()).collect());
        Ok(self)
    }

    /// Execute the copy operation
    pub fn execute(self) -> Result<()> {
        // Read destination metadata
        let mut dest_map = read_metadata(&self.dest)?;

        // Copy tags from source
        for (tag_name, tag_value) in self.source.map.iter() {
            let should_copy = self
                .tags
                .as_ref()
                .map(|t| t.contains(tag_name))
                .unwrap_or(true);

            if should_copy {
                dest_map.insert(tag_name, tag_value.clone());
            }
        }

        // Write to destination
        write_metadata(&self.dest, &dest_map)
    }
}

// Implement From traits for easy value conversion
impl From<&str> for TagValue {
    fn from(s: &str) -> Self {
        TagValue::new_string(s.to_string())
    }
}

impl From<String> for TagValue {
    fn from(s: String) -> Self {
        TagValue::new_string(s)
    }
}

impl From<i64> for TagValue {
    fn from(i: i64) -> Self {
        TagValue::new_integer(i)
    }
}

impl From<i32> for TagValue {
    fn from(i: i32) -> Self {
        TagValue::new_integer(i as i64)
    }
}

impl From<f64> for TagValue {
    fn from(f: f64) -> Self {
        TagValue::new_float(f)
    }
}

impl From<f32> for TagValue {
    fn from(f: f32) -> Self {
        TagValue::new_float(f as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metadata_is_empty() {
        let meta = Metadata::new();
        assert!(meta.is_empty());
        assert_eq!(meta.len(), 0);
        assert!(meta.source_path().is_none());
    }

    #[test]
    fn test_set_tag_builder_pattern() {
        let meta = Metadata::new()
            .set_tag("EXIF:Make", "Canon")
            .set_tag("EXIF:ISO", 400i64);

        assert_eq!(meta.len(), 2);
        assert_eq!(meta.get_string("EXIF:Make"), Some("Canon"));
        assert_eq!(meta.get_integer("EXIF:ISO"), Some(400));
    }

    #[test]
    fn test_insert_and_remove() {
        let mut meta = Metadata::new();
        meta.insert("EXIF:Artist", "Test");
        assert!(meta.has_tag("EXIF:Artist"));

        meta.remove("EXIF:Artist");
        assert!(!meta.has_tag("EXIF:Artist"));
    }

    #[test]
    fn test_typed_getters_return_none_for_wrong_type() {
        let meta = Metadata::new().set_tag("EXIF:Make", "Canon");

        // String tag should not return as integer
        assert!(meta.get_integer("EXIF:Make").is_none());
        // Non-existent tag should return None
        assert!(meta.get_string("EXIF:NonExistent").is_none());
    }

    #[test]
    fn test_from_conversions() {
        let _ = TagValue::from("test");
        let _ = TagValue::from(String::from("test"));
        let _ = TagValue::from(42i64);
        let _ = TagValue::from(42i32);
        let _ = TagValue::from(3.14f64);
        let _ = TagValue::from(3.14f32);
    }
}
