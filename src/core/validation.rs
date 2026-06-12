//! Tag value validation engine
//!
//! This module provides validation logic for metadata tag values before write
//! operations are performed. Tags with reliable registry type metadata are
//! checked against their descriptor type; registry-owned YAML descriptors with
//! absent or conflicting type metadata are limited to intrinsic value checks.

#![allow(dead_code)]

use crate::core::tag_value::TagValue;
use crate::core::{TagDescriptor, ValueType};
use crate::error::ExifToolError;
use crate::tag_db::tag_registry::descriptor_has_reliable_value_type;

fn descriptor_allows_datetime(descriptor: &TagDescriptor) -> bool {
    let name = descriptor.name();
    name.contains("Date") || name.contains("Time")
}

/// Validates that a TagValue matches the expected type defined in its TagDescriptor.
///
/// This function performs comprehensive type checking to ensure tag values conform to their
/// schema definitions. It validates:
/// - Type matching between TagValue variants and reliable TagDescriptor value_type metadata
/// - Intrinsic value constraints for descriptors whose YAML type metadata is ambiguous
/// - Special constraints like non-zero denominators for Rational values
/// - DateTime structural validity (already guaranteed by chrono::DateTime type)
///
/// This validation is intended to be called before write operations to prevent
/// invalid metadata from being written to files.
///
/// # Arguments
///
/// * `descriptor` - The tag descriptor containing the expected value type and constraints
/// * `value` - The tag value to validate against the descriptor
///
/// # Returns
///
/// * `Ok(())` if validation succeeds
/// * `Err(ExifToolError::InvalidTagValue)` if validation fails with detailed reason
///
/// # Examples
///
/// ```
/// use oxidex::core::{TagDescriptor, TagId, FormatFamily, ValueType};
/// use oxidex::core::tag_value::TagValue;
/// use oxidex::core::validation::validate_tag_value;
///
/// // Example: Validate a String tag value
/// let descriptor = TagDescriptor::new(
///     TagId::new_numeric(0x010F),
///     "EXIF:Make".to_string(),
///     FormatFamily::EXIF,
///     true,
///     ValueType::String,
///     "Camera manufacturer".to_string(),
///     vec!["Canon".to_string()],
/// );
///
/// let value = TagValue::new_string("Nikon");
/// assert!(validate_tag_value(&descriptor, &value).is_ok());
///
/// // Example: Type mismatch fails validation
/// let wrong_value = TagValue::new_integer(42);
/// assert!(validate_tag_value(&descriptor, &wrong_value).is_err());
/// ```
///
/// # Validation Rules
///
/// ## Type Matching
/// The function checks that the TagValue variant matches the expected ValueType when the
/// descriptor has reliable type metadata:
/// - `TagValue::String` must match `ValueType::String`
/// - `TagValue::Integer` must match `ValueType::Integer`
/// - `TagValue::Float` must match `ValueType::Float`
/// - `TagValue::Rational` must match `ValueType::Rational`
/// - `TagValue::Binary` must match `ValueType::Binary`
/// - `TagValue::DateTime` must match `ValueType::DateTime`
/// - `TagValue::Struct` must match `ValueType::Struct`
///
/// Exact descriptors returned from the active registry may skip strict type matching when the
/// descriptor came from YAML rows with absent, unrecognized, or conflicting type metadata.
/// ## Rational Number Constraints
/// For Rational values, the denominator must not be zero, as this would represent
/// an undefined mathematical value.
///
/// ## DateTime Validation
/// DateTime values are already validated by the `chrono::DateTime<Utc>` type system.
/// If a TagValue::DateTime variant exists, it is structurally valid. EXIF DateTime
/// format (YYYY:MM:DD HH:MM:SS) validation would occur during string parsing, not here.
pub fn validate_tag_value(
    descriptor: &TagDescriptor,
    value: &TagValue,
) -> Result<(), ExifToolError> {
    validate_tag_value_with_name(descriptor.name(), descriptor, value)
}

/// Validates a tag value with an explicit tag name for error messages.
///
/// This function is useful when the tag name used in the metadata differs from
/// the canonical name in the descriptor (e.g., "IFD0:Make" vs "EXIF:Make").
/// Validation errors will report the provided tag_name rather than descriptor.name().
///
/// # Arguments
///
/// * `tag_name` - The tag name to use in error messages (e.g., "IFD0:Make")
/// * `descriptor` - The tag descriptor containing type information
/// * `value` - The tag value to validate
pub fn validate_tag_value_with_name(
    tag_name: &str,
    descriptor: &TagDescriptor,
    value: &TagValue,
) -> Result<(), ExifToolError> {
    if !descriptor_has_reliable_value_type(descriptor) {
        return validate_tag_value_intrinsics(tag_name, value);
    }

    let expected_type = descriptor.value_type();

    match value {
        TagValue::String(_) => {
            if expected_type != ValueType::String {
                return Err(ExifToolError::invalid_tag_value(
                    tag_name,
                    format!("Type mismatch: expected {:?} but got String", expected_type),
                ));
            }
        }
        TagValue::Integer(_) => {
            if expected_type != ValueType::Integer {
                return Err(ExifToolError::invalid_tag_value(
                    tag_name,
                    format!(
                        "Type mismatch: expected {:?} but got Integer",
                        expected_type
                    ),
                ));
            }
        }
        TagValue::Float(_) => {
            if expected_type != ValueType::Float {
                return Err(ExifToolError::invalid_tag_value(
                    tag_name,
                    format!("Type mismatch: expected {:?} but got Float", expected_type),
                ));
            }
        }
        TagValue::Rational {
            numerator: _,
            denominator: _,
        } => {
            if expected_type != ValueType::Rational {
                return Err(ExifToolError::invalid_tag_value(
                    tag_name,
                    format!(
                        "Type mismatch: expected {:?} but got Rational",
                        expected_type
                    ),
                ));
            }
            validate_tag_value_intrinsics(tag_name, value)?;
        }
        TagValue::Binary(_) => {
            if expected_type != ValueType::Binary {
                return Err(ExifToolError::invalid_tag_value(
                    tag_name,
                    format!("Type mismatch: expected {:?} but got Binary", expected_type),
                ));
            }
        }
        TagValue::DateTime(_) => {
            if expected_type == ValueType::DateTime {
                // DateTime matches expected type
            } else if expected_type == ValueType::String && descriptor_allows_datetime(descriptor) {
                // Allow DateTime values for string-based descriptors that represent dates/times
            } else {
                return Err(ExifToolError::invalid_tag_value(
                    tag_name,
                    format!(
                        "Type mismatch: expected {:?} but got DateTime",
                        expected_type
                    ),
                ));
            }
        }
        TagValue::Struct(_) => {
            if expected_type != ValueType::Struct {
                return Err(ExifToolError::invalid_tag_value(
                    tag_name,
                    format!("Type mismatch: expected {:?} but got Struct", expected_type),
                ));
            }
        }
        TagValue::Array(_) => {
            // Arrays can contain any value type, skip type validation for now
            // TODO: Add ValueType::Array to support array type validation
            // For this iteration, basic type matching is sufficient
            // Recursive validation of nested structure contents is out of scope
        }
    }

    Ok(())
}

/// Validates constraints that are independent of registry type metadata.
pub(crate) fn validate_tag_value_intrinsics(
    tag_name: &str,
    value: &TagValue,
) -> Result<(), ExifToolError> {
    if let TagValue::Rational { denominator, .. } = value
        && *denominator == 0
    {
        return Err(ExifToolError::invalid_tag_value(
            tag_name,
            "Invalid Rational value: denominator cannot be zero".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{FormatFamily, TagId};
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;

    // Helper function to create a test descriptor
    fn create_descriptor(value_type: ValueType) -> TagDescriptor {
        TagDescriptor::new(
            TagId::new_numeric(0x010F),
            "TestTag".to_string(),
            FormatFamily::EXIF,
            true,
            value_type,
            "Test tag for validation".to_string(),
            vec![],
        )
    }

    // Test 1: Valid String type matches
    #[test]
    fn test_validate_string_type_matches() {
        let descriptor = create_descriptor(ValueType::String);
        let value = TagValue::new_string("Canon EOS 5D");

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 2: Valid Integer type matches
    #[test]
    fn test_validate_integer_type_matches() {
        let descriptor = create_descriptor(ValueType::Integer);
        let value = TagValue::new_integer(100);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 3: Valid Float type matches
    #[test]
    fn test_validate_float_type_matches() {
        let descriptor = create_descriptor(ValueType::Float);
        let value = TagValue::new_float(5.6);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 4: Valid Rational type matches
    #[test]
    fn test_validate_rational_type_matches() {
        let descriptor = create_descriptor(ValueType::Rational);
        let value = TagValue::new_rational(1, 100);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 5: Valid Binary type matches
    #[test]
    fn test_validate_binary_type_matches() {
        let descriptor = create_descriptor(ValueType::Binary);
        let value = TagValue::new_binary(vec![0xFF, 0xD8, 0xFF, 0xE0]);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 6: Valid DateTime type matches
    #[test]
    fn test_validate_datetime_type_matches() {
        let descriptor = create_descriptor(ValueType::DateTime);
        let dt = Utc.with_ymd_and_hms(2023, 6, 15, 14, 30, 0).unwrap();
        let value = TagValue::new_datetime(dt);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 7: Valid Struct type matches
    #[test]
    fn test_validate_struct_type_matches() {
        let descriptor = create_descriptor(ValueType::Struct);
        let mut map = HashMap::new();
        map.insert("author".to_string(), TagValue::new_string("John Doe"));
        let value = TagValue::new_struct(map);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 8: Type mismatch - String expected, Integer provided
    #[test]
    fn test_validate_type_mismatch_string_integer() {
        let descriptor = create_descriptor(ValueType::String);
        let value = TagValue::new_integer(42);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_err());

        if let Err(ExifToolError::InvalidTagValue { tag_name, reason }) = result {
            assert_eq!(tag_name, "TestTag");
            assert!(reason.contains("Type mismatch"));
            assert!(reason.contains("String"));
            assert!(reason.contains("Integer"));
        } else {
            panic!("Expected InvalidTagValue error");
        }
    }

    #[test]
    fn test_cloned_unreliable_yaml_descriptor_remains_strict() {
        let descriptor = crate::tag_db::tag_registry::get_tag_descriptor("PNG:ImageWidth")
            .expect("expected YAML-backed PNG descriptor")
            .clone();
        let value = TagValue::new_integer(640);

        let result = validate_tag_value(&descriptor, &value);

        assert!(result.is_err());
        if let Err(ExifToolError::InvalidTagValue { tag_name, reason }) = result {
            assert_eq!(tag_name, "PNG:ImageWidth");
            assert!(reason.contains("expected String"));
            assert!(reason.contains("got Integer"));
        } else {
            panic!("Expected InvalidTagValue error");
        }
    }

    // Test 9: Type mismatch - Integer expected, String provided
    #[test]
    fn test_validate_type_mismatch_integer_string() {
        let descriptor = create_descriptor(ValueType::Integer);
        let value = TagValue::new_string("not a number");

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_err());

        if let Err(ExifToolError::InvalidTagValue { tag_name, reason }) = result {
            assert_eq!(tag_name, "TestTag");
            assert!(reason.contains("Type mismatch"));
            assert!(reason.contains("Integer"));
            assert!(reason.contains("String"));
        } else {
            panic!("Expected InvalidTagValue error");
        }
    }

    // Test 10: Type mismatch - Float expected, Integer provided
    #[test]
    fn test_validate_type_mismatch_float_integer() {
        let descriptor = create_descriptor(ValueType::Float);
        let value = TagValue::new_integer(42);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_err());

        if let Err(ExifToolError::InvalidTagValue { reason, .. }) = result {
            assert!(reason.contains("Type mismatch"));
            assert!(reason.contains("Float"));
            assert!(reason.contains("Integer"));
        } else {
            panic!("Expected InvalidTagValue error");
        }
    }

    // Test 11: Type mismatch - Rational expected, String provided
    #[test]
    fn test_validate_type_mismatch_rational_string() {
        let descriptor = create_descriptor(ValueType::Rational);
        let value = TagValue::new_string("1/100");

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_err());

        if let Err(ExifToolError::InvalidTagValue { reason, .. }) = result {
            assert!(reason.contains("Type mismatch"));
            assert!(reason.contains("Rational"));
            assert!(reason.contains("String"));
        } else {
            panic!("Expected InvalidTagValue error");
        }
    }

    // Test 12: Rational with zero denominator fails validation
    #[test]
    fn test_validate_rational_zero_denominator() {
        let descriptor = create_descriptor(ValueType::Rational);
        let value = TagValue::new_rational(1, 0);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_err());

        if let Err(ExifToolError::InvalidTagValue { tag_name, reason }) = result {
            assert_eq!(tag_name, "TestTag");
            assert!(reason.contains("denominator"));
            assert!(reason.contains("zero"));
        } else {
            panic!("Expected InvalidTagValue error");
        }
    }

    // Test 13: Rational with negative denominator is valid
    #[test]
    fn test_validate_rational_negative_denominator() {
        let descriptor = create_descriptor(ValueType::Rational);
        let value = TagValue::new_rational(1, -100);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 14: Type mismatch - Binary expected, String provided
    #[test]
    fn test_validate_type_mismatch_binary_string() {
        let descriptor = create_descriptor(ValueType::Binary);
        let value = TagValue::new_string("binary data");

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_err());

        if let Err(ExifToolError::InvalidTagValue { reason, .. }) = result {
            assert!(reason.contains("Type mismatch"));
            assert!(reason.contains("Binary"));
        } else {
            panic!("Expected InvalidTagValue error");
        }
    }

    // Test 15: Type mismatch - DateTime expected, String provided
    #[test]
    fn test_validate_type_mismatch_datetime_string() {
        let descriptor = create_descriptor(ValueType::DateTime);
        let value = TagValue::new_string("2023:06:15 14:30:00");

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_err());

        if let Err(ExifToolError::InvalidTagValue { reason, .. }) = result {
            assert!(reason.contains("Type mismatch"));
            assert!(reason.contains("DateTime"));
        } else {
            panic!("Expected InvalidTagValue error");
        }
    }

    // Test 16: Empty string is valid for String type
    #[test]
    fn test_validate_empty_string() {
        let descriptor = create_descriptor(ValueType::String);
        let value = TagValue::new_string("");

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 17: Zero is valid for Integer type
    #[test]
    fn test_validate_zero_integer() {
        let descriptor = create_descriptor(ValueType::Integer);
        let value = TagValue::new_integer(0);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 18: Negative integer is valid
    #[test]
    fn test_validate_negative_integer() {
        let descriptor = create_descriptor(ValueType::Integer);
        let value = TagValue::new_integer(-42);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 19: Maximum i64 value is valid
    #[test]
    fn test_validate_max_integer() {
        let descriptor = create_descriptor(ValueType::Integer);
        let value = TagValue::new_integer(i64::MAX);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 20: Empty binary data is valid
    #[test]
    fn test_validate_empty_binary() {
        let descriptor = create_descriptor(ValueType::Binary);
        let value = TagValue::new_binary(vec![]);

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 21: Type mismatch - Struct expected, String provided
    #[test]
    fn test_validate_type_mismatch_struct_string() {
        let descriptor = create_descriptor(ValueType::Struct);
        let value = TagValue::new_string("not a struct");

        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_err());

        if let Err(ExifToolError::InvalidTagValue { reason, .. }) = result {
            assert!(reason.contains("Type mismatch"));
            assert!(reason.contains("Struct"));
        } else {
            panic!("Expected InvalidTagValue error");
        }
    }

    // Test 22: Real-world example - EXIF Make tag
    #[test]
    fn test_validate_real_world_exif_make() {
        let descriptor = TagDescriptor::new(
            TagId::new_numeric(0x010F),
            "EXIF:Make".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::String,
            "Camera manufacturer".to_string(),
            vec!["Canon".to_string(), "Nikon".to_string()],
        );

        let value = TagValue::new_string("Sony");
        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 23: Real-world example - EXIF ExposureTime (Rational)
    #[test]
    fn test_validate_real_world_exposure_time() {
        let descriptor = TagDescriptor::new(
            TagId::new_numeric(0x829A),
            "EXIF:ExposureTime".to_string(),
            FormatFamily::EXIF,
            true,
            ValueType::Rational,
            "Exposure time in seconds".to_string(),
            vec!["1/1000".to_string(), "1/250".to_string()],
        );

        let value = TagValue::new_rational(1, 1000);
        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_ok());
    }

    // Test 24: Real-world example - GPS Latitude (should fail with wrong type)
    #[test]
    fn test_validate_real_world_gps_latitude_wrong_type() {
        let descriptor = TagDescriptor::new(
            TagId::new_numeric(0x0002),
            "GPS:GPSLatitude".to_string(),
            FormatFamily::GPS,
            true,
            ValueType::Rational,
            "GPS latitude".to_string(),
            vec!["37.7749".to_string()],
        );

        let value = TagValue::new_string("37.7749");
        let result = validate_tag_value(&descriptor, &value);
        assert!(result.is_err());
    }
}
