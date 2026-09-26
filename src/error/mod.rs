//! Error types
//!
//! This module defines the ExifToolError enum and related error types.

#![allow(dead_code)]

use std::fmt;
use std::io;

/// The main error type for OxiDex operations.
///
/// This enum represents all possible errors that can occur during
/// metadata extraction, parsing, validation, and file I/O operations.
/// `#[non_exhaustive]`: new failure kinds may be added in a minor release;
/// match with a wildcard arm.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExifToolError {
    /// I/O error occurred during file operations
    IoError(io::Error),

    /// Error parsing metadata from file
    ParseError {
        /// Description of what failed to parse
        message: String,
        /// Optional byte offset where the error occurred
        offset: Option<usize>,
    },

    /// Requested tag was not found in the metadata
    TagNotFound {
        /// Name of the tag that wasn't found
        tag_name: String,
    },

    /// Tag value is invalid or doesn't match expected type
    InvalidTagValue {
        /// Name of the tag with invalid value
        tag_name: String,
        /// Description of why the value is invalid
        reason: String,
    },

    /// File format is not supported or recognized
    UnsupportedFormat {
        /// Description of the unsupported format or reason
        message: String,
    },

    /// A write request named tags that would not be written, so nothing was
    /// written: the file is byte-identical to before the call.
    ///
    /// Every write API (`write_metadata`, `modify_tag`, `remove_tag`,
    /// `copy_metadata`, the `Metadata` builder, the C ABI's
    /// `exiftool_write_file`) returns this rather than `Ok` when any
    /// requested change would be dropped -- a name no writer of the file's
    /// format addresses (`XMP:Title` in a JPEG, `File:Comment`, most `IFD1:`
    /// keys), an ungrouped name that cannot be resolved to one address, or a
    /// change the read-back after writing could not find. Each entry names
    /// the key as the caller spelled it.
    TagsNotWritten {
        /// One entry per key that would not be written, in request order.
        tags: Vec<TagNotWritten>,
    },
}

/// One key a write request named that would not be written, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TagNotWritten {
    /// The key as the caller spelled it (`XPTitle`, `XMP:Title`).
    pub tag: String,
    /// Why it would not be written.
    pub reason: String,
}

impl TagNotWritten {
    /// A key that would not be written, and why.
    pub fn new<T: Into<String>, R: Into<String>>(tag: T, reason: R) -> Self {
        TagNotWritten {
            tag: tag.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for TagNotWritten {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cannot write tag '{}': {}", self.tag, self.reason)
    }
}

impl ExifToolError {
    /// Creates a new ParseError with a message
    pub fn parse_error<S: Into<String>>(message: S) -> Self {
        ExifToolError::ParseError {
            message: message.into(),
            offset: None,
        }
    }

    /// Creates a new ParseError with a message and offset
    pub fn parse_error_at<S: Into<String>>(message: S, offset: usize) -> Self {
        ExifToolError::ParseError {
            message: message.into(),
            offset: Some(offset),
        }
    }

    /// Creates a new TagNotFound error
    pub fn tag_not_found<S: Into<String>>(tag_name: S) -> Self {
        ExifToolError::TagNotFound {
            tag_name: tag_name.into(),
        }
    }

    /// Creates a new InvalidTagValue error
    pub fn invalid_tag_value<S: Into<String>, R: Into<String>>(tag_name: S, reason: R) -> Self {
        ExifToolError::InvalidTagValue {
            tag_name: tag_name.into(),
            reason: reason.into(),
        }
    }

    /// The bare `reason` of an [`ExifToolError::InvalidTagValue`], without
    /// the `Invalid value for tag '...': ` wrapping `Display` adds --
    /// `None` for any other variant. Lets a caller recognize a specific
    /// reason string (e.g. `value_parser`'s `(not in PrintConv)` /
    /// `(matches more than one PrintConv)` phrasing, which is ExifTool's own
    /// wording and needs its own `Warning: ... / Nothing to do.` framing
    /// rather than oxidex's `Invalid value for ...` diagnosis) without
    /// re-parsing the `Display` string.
    pub fn invalid_tag_value_reason(&self) -> Option<&str> {
        match self {
            ExifToolError::InvalidTagValue { reason, .. } => Some(reason),
            _ => None,
        }
    }

    /// Creates a new UnsupportedFormat error
    pub fn unsupported_format<S: Into<String>>(message: S) -> Self {
        ExifToolError::UnsupportedFormat {
            message: message.into(),
        }
    }

    /// A refusal of the single key `tag`, for `reason`.
    pub fn tag_not_written<T: Into<String>, R: Into<String>>(tag: T, reason: R) -> Self {
        ExifToolError::TagsNotWritten {
            tags: vec![TagNotWritten::new(tag, reason)],
        }
    }

    /// The keys a [`ExifToolError::TagsNotWritten`] names; empty for any
    /// other error.
    pub fn tags_not_written(&self) -> &[TagNotWritten] {
        match self {
            ExifToolError::TagsNotWritten { tags } => tags,
            _ => &[],
        }
    }
}

impl fmt::Display for ExifToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExifToolError::IoError(e) => write!(f, "I/O error: {}", e),
            ExifToolError::ParseError { message, offset } => {
                if let Some(off) = offset {
                    write!(f, "Parse error at offset {}: {}", off, message)
                } else {
                    write!(f, "Parse error: {}", message)
                }
            }
            ExifToolError::TagNotFound { tag_name } => {
                write!(f, "Tag not found: {}", tag_name)
            }
            ExifToolError::InvalidTagValue { tag_name, reason } => {
                write!(f, "Invalid value for tag '{}': {}", tag_name, reason)
            }
            ExifToolError::UnsupportedFormat { message } => {
                write!(f, "Unsupported format: {}", message)
            }
            ExifToolError::TagsNotWritten { tags } => {
                for (index, tag) in tags.iter().enumerate() {
                    if index > 0 {
                        f.write_str("; ")?;
                    }
                    write!(f, "{tag}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for ExifToolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ExifToolError::IoError(e) => Some(e),
            _ => None,
        }
    }
}

// Conversion from std::io::Error to ExifToolError
impl From<io::Error> for ExifToolError {
    fn from(error: io::Error) -> Self {
        ExifToolError::IoError(error)
    }
}

/// Type alias for Results that use ExifToolError
pub type Result<T> = std::result::Result<T, ExifToolError>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;
    use std::io::{Error, ErrorKind};

    #[test]
    fn test_io_error_variant() {
        let io_err = Error::new(ErrorKind::NotFound, "file not found");
        let err = ExifToolError::from(io_err);

        match err {
            ExifToolError::IoError(_) => {}
            _ => panic!("Expected IoError variant"),
        }

        let display = format!("{}", err);
        assert!(display.contains("I/O error"));
    }

    #[test]
    fn test_parse_error_variant() {
        let err = ExifToolError::parse_error("Invalid JPEG marker");

        match &err {
            ExifToolError::ParseError { message, offset } => {
                assert_eq!(message, "Invalid JPEG marker");
                assert_eq!(*offset, None);
            }
            _ => panic!("Expected ParseError variant"),
        }

        let display = format!("{}", err);
        assert!(display.contains("Parse error"));
        assert!(display.contains("Invalid JPEG marker"));
    }

    #[test]
    fn test_parse_error_with_offset() {
        let err = ExifToolError::parse_error_at("Unexpected byte", 1024);

        match &err {
            ExifToolError::ParseError { message, offset } => {
                assert_eq!(message, "Unexpected byte");
                assert_eq!(*offset, Some(1024));
            }
            _ => panic!("Expected ParseError variant"),
        }

        let display = format!("{}", err);
        assert!(display.contains("offset 1024"));
    }

    #[test]
    fn test_tag_not_found_variant() {
        let err = ExifToolError::tag_not_found("EXIF:Make");

        match &err {
            ExifToolError::TagNotFound { tag_name } => {
                assert_eq!(tag_name, "EXIF:Make");
            }
            _ => panic!("Expected TagNotFound variant"),
        }

        let display = format!("{}", err);
        assert!(display.contains("Tag not found"));
        assert!(display.contains("EXIF:Make"));
    }

    #[test]
    fn test_invalid_tag_value_variant() {
        let err = ExifToolError::invalid_tag_value("EXIF:ISO", "value out of range");

        match &err {
            ExifToolError::InvalidTagValue { tag_name, reason } => {
                assert_eq!(tag_name, "EXIF:ISO");
                assert_eq!(reason, "value out of range");
            }
            _ => panic!("Expected InvalidTagValue variant"),
        }

        let display = format!("{}", err);
        assert!(display.contains("Invalid value"));
        assert!(display.contains("EXIF:ISO"));
        assert!(display.contains("value out of range"));
    }

    #[test]
    fn test_unsupported_format_variant() {
        let err = ExifToolError::unsupported_format("BMP files are not supported");

        match &err {
            ExifToolError::UnsupportedFormat { message } => {
                assert_eq!(message, "BMP files are not supported");
            }
            _ => panic!("Expected UnsupportedFormat variant"),
        }

        let display = format!("{}", err);
        assert!(display.contains("Unsupported format"));
        assert!(display.contains("BMP files"));
    }

    #[test]
    fn test_tags_not_written_variant_names_every_key() {
        let err = ExifToolError::TagsNotWritten {
            tags: vec![
                TagNotWritten::new("XMP:Title", "no XMP writer"),
                TagNotWritten::new("File:Comment", "no File writer"),
            ],
        };
        assert_eq!(
            err.to_string(),
            "Cannot write tag 'XMP:Title': no XMP writer; \
             Cannot write tag 'File:Comment': no File writer"
        );
        let names: Vec<&str> = err
            .tags_not_written()
            .iter()
            .map(|tag| tag.tag.as_str())
            .collect();
        assert_eq!(names, ["XMP:Title", "File:Comment"]);
        assert!(
            ExifToolError::parse_error("x")
                .tags_not_written()
                .is_empty()
        );
        assert_eq!(
            ExifToolError::tag_not_written("XPTitle", "why").to_string(),
            "Cannot write tag 'XPTitle': why"
        );
    }

    #[test]
    fn test_error_trait_implementation() {
        let io_err = Error::new(ErrorKind::PermissionDenied, "access denied");
        let err = ExifToolError::from(io_err);

        // Test that it implements std::error::Error
        let _: &dyn std::error::Error = &err;

        // Test source() method
        assert!(err.source().is_some());
    }

    #[test]
    fn test_debug_derive() {
        let err = ExifToolError::tag_not_found("EXIF:Model");
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("TagNotFound"));
        assert!(debug_str.contains("EXIF:Model"));
    }

    #[test]
    fn test_result_type_alias() {
        fn example_function() -> Result<i32> {
            Err(ExifToolError::parse_error("test error"))
        }

        let result = example_function();
        assert!(result.is_err());
    }
}
