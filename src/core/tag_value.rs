//! TagValue enum for representing different metadata value types
//!
//! This module defines the TagValue enum for String/Number/Binary/etc.

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents different types of metadata values that can be stored in tags.
///
/// This enum supports all common metadata value types found across various
/// metadata formats (EXIF, XMP, IPTC, etc.).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum TagValue {
    /// UTF-8 string value (most common type)
    String(String),

    /// Signed 64-bit integer value
    Integer(i64),

    /// 64-bit floating point value
    Float(f64),

    /// Rational number represented as numerator/denominator (common in EXIF)
    Rational {
        /// Numerator of the rational number
        numerator: i32,
        /// Denominator of the rational number
        denominator: i32,
    },

    /// Binary data (arbitrary byte sequences)
    Binary(Vec<u8>),

    /// Date and time value with UTC timezone
    DateTime(DateTime<Utc>),

    /// Structured/nested data for complex XMP structures
    /// Boxed to prevent infinite size and allow recursion
    Struct(Box<HashMap<String, TagValue>>),
}

impl TagValue {
    /// Creates a new String variant
    pub fn new_string<S: Into<String>>(s: S) -> Self {
        TagValue::String(s.into())
    }

    /// Creates a new Integer variant
    pub fn new_integer(i: i64) -> Self {
        TagValue::Integer(i)
    }

    /// Creates a new Float variant
    pub fn new_float(f: f64) -> Self {
        TagValue::Float(f)
    }

    /// Creates a new Rational variant
    pub fn new_rational(numerator: i32, denominator: i32) -> Self {
        TagValue::Rational {
            numerator,
            denominator,
        }
    }

    /// Creates a new Binary variant
    pub fn new_binary(data: Vec<u8>) -> Self {
        TagValue::Binary(data)
    }

    /// Creates a new DateTime variant
    pub fn new_datetime(dt: DateTime<Utc>) -> Self {
        TagValue::DateTime(dt)
    }

    /// Creates a new Struct variant
    pub fn new_struct(data: HashMap<String, TagValue>) -> Self {
        TagValue::Struct(Box::new(data))
    }

    /// Returns true if this is a String variant
    pub fn is_string(&self) -> bool {
        matches!(self, TagValue::String(_))
    }

    /// Returns true if this is an Integer variant
    pub fn is_integer(&self) -> bool {
        matches!(self, TagValue::Integer(_))
    }

    /// Returns true if this is a Float variant
    pub fn is_float(&self) -> bool {
        matches!(self, TagValue::Float(_))
    }

    /// Returns true if this is a Rational variant
    pub fn is_rational(&self) -> bool {
        matches!(self, TagValue::Rational { .. })
    }

    /// Returns true if this is a Binary variant
    pub fn is_binary(&self) -> bool {
        matches!(self, TagValue::Binary(_))
    }

    /// Returns true if this is a DateTime variant
    pub fn is_datetime(&self) -> bool {
        matches!(self, TagValue::DateTime(_))
    }

    /// Returns true if this is a Struct variant
    pub fn is_struct(&self) -> bool {
        matches!(self, TagValue::Struct(_))
    }

    /// Attempts to get the value as a string reference
    pub fn as_string(&self) -> Option<&str> {
        match self {
            TagValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Attempts to get the value as an integer
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            TagValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Attempts to get the value as a float
    pub fn as_float(&self) -> Option<f64> {
        match self {
            TagValue::Float(f) => Some(*f),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_string_variant_creation_and_getter() {
        let value = TagValue::new_string("Canon EOS 5D");
        assert!(value.is_string());
        assert_eq!(value.as_string(), Some("Canon EOS 5D"));
        assert!(!value.is_integer());
    }

    #[test]
    fn test_integer_variant_creation_and_getter() {
        let value = TagValue::new_integer(42);
        assert!(value.is_integer());
        assert_eq!(value.as_integer(), Some(42));
        assert!(!value.is_string());
    }

    #[test]
    fn test_float_variant_creation_and_getter() {
        let value = TagValue::new_float(5.75);
        assert!(value.is_float());
        assert_eq!(value.as_float(), Some(5.75));
        assert!(!value.is_integer());
    }

    #[test]
    fn test_rational_variant_creation() {
        let value = TagValue::new_rational(1, 100);
        assert!(value.is_rational());
        match value {
            TagValue::Rational {
                numerator,
                denominator,
            } => {
                assert_eq!(numerator, 1);
                assert_eq!(denominator, 100);
            }
            _ => panic!("Expected Rational variant"),
        }
    }

    #[test]
    fn test_binary_variant_creation() {
        let data = vec![0xFF, 0xD8, 0xFF, 0xE0];
        let value = TagValue::new_binary(data.clone());
        assert!(value.is_binary());
        match value {
            TagValue::Binary(d) => assert_eq!(d, data),
            _ => panic!("Expected Binary variant"),
        }
    }

    #[test]
    fn test_datetime_variant_creation() {
        let dt = Utc.with_ymd_and_hms(2023, 6, 15, 12, 30, 0).unwrap();
        let value = TagValue::new_datetime(dt);
        assert!(value.is_datetime());
        match value {
            TagValue::DateTime(d) => assert_eq!(d, dt),
            _ => panic!("Expected DateTime variant"),
        }
    }

    #[test]
    fn test_struct_variant_creation() {
        let mut map = HashMap::new();
        map.insert("author".to_string(), TagValue::new_string("John Doe"));
        map.insert("version".to_string(), TagValue::new_integer(1));

        let value = TagValue::new_struct(map.clone());
        assert!(value.is_struct());
        match value {
            TagValue::Struct(s) => {
                assert_eq!(s.len(), 2);
                assert_eq!(
                    s.get("author").and_then(|v| v.as_string()),
                    Some("John Doe")
                );
            }
            _ => panic!("Expected Struct variant"),
        }
    }

    #[test]
    fn test_clone_derive() {
        let value1 = TagValue::new_string("test");
        let value2 = value1.clone();
        assert_eq!(value1, value2);
    }

    #[test]
    fn test_debug_derive() {
        let value = TagValue::new_integer(42);
        let debug_str = format!("{:?}", value);
        assert!(debug_str.contains("Integer"));
        assert!(debug_str.contains("42"));
    }

    #[test]
    fn test_serde_serialization() {
        let value = TagValue::new_string("Canon");
        let json = serde_json::to_string(&value).unwrap();
        assert!(json.contains("String"));
        assert!(json.contains("Canon"));
    }

    #[test]
    fn test_serde_deserialization() {
        let json = r#"{"type":"Integer","value":100}"#;
        let value: TagValue = serde_json::from_str(json).unwrap();
        assert_eq!(value.as_integer(), Some(100));
    }
}
