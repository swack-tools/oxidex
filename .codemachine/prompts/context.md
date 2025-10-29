# Task Briefing Package

This package contains all necessary information and strategic guidance for the Coder Agent.

---

## 1. Current Task Details

This is the full specification of the task you must complete.

```json
{
  "task_id": "I2.T10",
  "iteration_id": "I2",
  "iteration_goal": "Implement tag registry with subset of ExifTool tags, core metadata read/write operations, basic CLI with argument parsing, and extend format support to include XMP parsing and PNG format.",
  "description": "Implement validation in src/core/validation.rs. Create fn validate_tag_value(descriptor: &TagDescriptor, value: &TagValue) -> Result<(), ExifToolError> that checks: (1) value type matches descriptor type (e.g., String tag can't have Integer value), (2) range validation for numeric types if descriptor specifies constraints, (3) format validation for DateTime strings. Integrate into write operations (I3 will use this). Add unit tests with valid and invalid tag values.",
  "agent_type_hint": "BackendAgent",
  "inputs": "I1.T6 TagDescriptor and TagValue, I2.T2 tag registry",
  "target_files": ["src/core/validation.rs", "src/core/mod.rs"],
  "input_files": ["src/core/tag_descriptor.rs", "src/core/tag_value.rs", "src/tag_db/tag_registry.rs"],
  "deliverables": "Tag value validation function, unit tests for validation",
  "acceptance_criteria": "Validation succeeds for correct type matches, returns InvalidTagValue error for type mismatches, validates DateTime format (e.g., EXIF DateTime: YYYY:MM:DD HH:MM:SS), unit tests cover at least 5 validation scenarios (valid, wrong type, invalid date, etc.), cargo test validation passes",
  "dependencies": ["I1.T6", "I2.T2"],
  "parallelizable": true,
  "done": false
}
```

---

## 2. Architectural & Planning Context

The following are the relevant sections from the architecture and plan documents, which I found by analyzing the task description.

### Context: Validation Engine (from 03_System_Structure_and_Data.md)

```markdown
Component(validation, "Validation Engine", "Rust", "Tag value type checking, range validation")

Rel(operations, validation, "Validates values via")
```

The Validation Engine is a core component in the Domain Layer of the hexagonal architecture. It is called by the Metadata Operations component to validate tag values before write operations.

### Context: Key Entities - TagDescriptor (from 03_System_Structure_and_Data.md)

```markdown
#### Key Entities

1. **File**: Represents a media file being processed (JPEG, PNG, etc.)
2. **MetadataMap**: Collection of all metadata tags extracted from a file
3. **TagValue**: A single metadata tag with its name, value, and type information
4. **TagDescriptor**: Definition of a tag (from tag database) including ID, name, type constraints, format family
5. **FormatFamily**: Grouping of related metadata standards (EXIF, XMP, IPTC, MakerNotes)
6. **IFD (Image File Directory)**: TIFF-specific structural element containing tags
```

TagDescriptor contains the schema definition that validation must check against. TagValue contains the actual value that needs validation.

### Context: TagValue Variant Types (from 03_System_Structure_and_Data.md)

```markdown
- **Variant Value Type**: `TagValue.value` uses a Rust `enum` to represent heterogeneous tag types:
  ```rust
  enum TagValueData {
      String(String),
      Number(f64),
      Integer(i64),
      Binary(Vec<u8>),
      Rational { numerator: i32, denominator: i32 },
      Struct(HashMap<String, TagValueData>), // For complex XMP structures
  }
  ```
```

This shows the complete set of value types that validation must handle.

### Context: Security - Input Validation (from 05_Operational_Architecture.md)

```markdown
**Input Validation**:

All parsers follow defensive pattern:
```rust
fn read_u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data.get(offset..offset+4)
        .ok_or(ParseError::UnexpectedEof)?;  // Bounds check
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}
```
```

This defensive programming pattern should be applied to validation logic as well - always validate before processing.

### Context: Error Handling Strategy (from 05_Operational_Architecture.md)

```markdown
**Threat Model**:

ExifTool-RS processes potentially malicious files from untrusted sources (e.g., user uploads, scraped images). Primary threats:

1. **Memory Corruption**: Buffer overflows, use-after-free in parsers
2. **Resource Exhaustion**: Zip bombs, billion laughs (XML), decompression bombs
3. **Path Traversal**: Malicious filenames in archive processing
4. **Code Injection**: Via scripting features (if added)

**Mitigations**:

| **Threat** | **Mitigation** | **Implementation** |
|------------|---------------|-------------------|
| Buffer overflows | Rust ownership system | Compile-time prevention via borrow checker |
| Integer overflows | Checked arithmetic | `#![deny(overflowing_literals)]`, `checked_add()` in parsers |
| Resource exhaustion | Size limits | Max allocation: 1GB per file, max parse depth: 64 levels (nested IFDs) |
```

Validation is part of the security strategy - reject invalid inputs early.

### Context: Reliability - Testing Strategy (from 05_Operational_Architecture.md)

```markdown
**Reliability Strategy**:

1. **Fault Tolerance**:
   - **Graceful Degradation**: On parser error, return partial metadata rather than failing entirely
   - **Error Recovery**: Malformed EXIF segment logs warning but continues parsing other segments (IPTC, XMP)
   - **Atomic Writes**: Temporary file + rename prevents corruption on crash mid-write

2. **Testing Pyramid**:
   ```
          /\
         /E2E\        <- Integration tests (10%): Full workflows
        /------\
       /  Unit  \      <- Unit tests (70%): Parser functions, tag validation
      /----------\
     / Property   \    <- Property-based (20%): Round-trip serialization, invariants
    /--------------\
   ```

   - **Unit Tests**: Every parser function has success/failure test cases
```

Tag validation requires comprehensive unit tests covering both success and failure cases.

---

## 3. Codebase Analysis & Strategic Guidance

The following analysis is based on my direct review of the current codebase. Use these notes and tips to guide your implementation.

### Relevant Existing Code

*   **File:** `src/core/tag_descriptor.rs`
    *   **Summary:** This file defines the complete TagDescriptor struct with all fields needed for validation: `tag_id`, `tag_name`, `format_family`, `writable`, `value_type` (enum with String, Integer, Float, Rational, Binary, DateTime, Struct variants), `description`, and `example_values`. It also defines the ValueType enum that validation must check against.
    *   **Recommendation:** You MUST import `TagDescriptor` and `ValueType` from this module. The `value_type` field on TagDescriptor is the schema that validation checks against. Use `descriptor.value_type()` accessor method to get the expected type.

*   **File:** `src/core/tag_value.rs`
    *   **Summary:** This file defines the TagValue enum with 7 variants: String(String), Integer(i64), Float(f64), Rational{numerator: i32, denominator: i32}, Binary(Vec<u8>), DateTime(DateTime<Utc>), and Struct(Box<HashMap<String, TagValue>>). It provides type-checking methods like `is_string()`, `is_integer()`, etc., and accessor methods like `as_string()`, `as_integer()`, etc.
    *   **Recommendation:** You MUST import `TagValue` from this module. Use the `is_*()` methods to check the variant type. The validation function must match TagValue variants against TagDescriptor's value_type field.

*   **File:** `src/error/mod.rs`
    *   **Summary:** This file defines the ExifToolError enum with 5 variants: IoError, ParseError, TagNotFound, InvalidTagValue, and UnsupportedFormat. The InvalidTagValue variant has fields `tag_name: String` and `reason: String`. There are convenience constructors like `ExifToolError::invalid_tag_value(tag_name, reason)`.
    *   **Recommendation:** You MUST use `ExifToolError::InvalidTagValue` for validation failures. Use the constructor `ExifToolError::invalid_tag_value(tag_name, reason)` to create validation errors. The `reason` field should clearly explain why validation failed (e.g., "Expected String but got Integer", "Invalid DateTime format: expected YYYY:MM:DD HH:MM:SS").

*   **File:** `src/tag_db/tag_registry.rs`
    *   **Summary:** This file contains the static TAG_REGISTRY with 100 pre-defined tags (60 EXIF, 20 GPS, 20 XMP). Each tag has a complete TagDescriptor with value_type specified. The registry uses `once_cell::sync::Lazy` for lazy initialization. It exports `get_tag_descriptor(name: &str) -> Option<&TagDescriptor>` for tag lookup.
    *   **Recommendation:** You SHOULD reference this file for understanding how TagDescriptor objects are structured in practice. The validate_tag_value function receives a TagDescriptor parameter, so you don't need to look up tags yourself - the caller will pass in the descriptor. However, reviewing the registry helps understand typical tag schemas.

*   **File:** `src/core/validation.rs`
    *   **Summary:** This file currently exists but contains only module documentation and `#![allow(dead_code)]`. It's a stub waiting for implementation.
    *   **Recommendation:** This is your PRIMARY target file. You MUST implement the `validate_tag_value` function here and add comprehensive unit tests.

### Implementation Tips & Notes

*   **Tip - Type Matching Logic:** The core validation logic is straightforward type matching. Create a match expression on the TagValue to extract its variant, then compare against descriptor.value_type(). For example:
    ```rust
    match value {
        TagValue::String(_) => {
            if descriptor.value_type() != ValueType::String {
                return Err(ExifToolError::invalid_tag_value(
                    descriptor.name(),
                    format!("Expected {:?} but got String", descriptor.value_type())
                ));
            }
        }
        // ... handle other variants
    }
    ```

*   **Tip - DateTime Validation:** For DateTime values, you need to validate EXIF DateTime format which is `YYYY:MM:DD HH:MM:SS` (colons not hyphens for date, 24-hour time). The task acceptance criteria specifically mentions this format. TagValue::DateTime stores a `chrono::DateTime<Utc>`, so if the TagValue is already a DateTime variant, it's structurally valid. However, you may want to add format validation if constructing from strings in the future. For now, accept any valid DateTime<Utc> value.

*   **Note - Range Validation:** The task mentions "range validation for numeric types if descriptor specifies constraints". However, reviewing the current TagDescriptor structure (line 96-118 in tag_descriptor.rs), there are no constraint fields defined. The acceptance criteria doesn't test range validation. You SHOULD implement basic type checking first, and you MAY skip range validation for now since the schema doesn't support it yet. If you choose to implement range validation, document that it's a future enhancement placeholder.

*   **Note - Rational Type:** The Rational variant has special structure: `Rational { numerator: i32, denominator: i32 }`. You MUST check that the denominator is not zero as part of validation. A rational number with zero denominator is mathematically invalid and should return an InvalidTagValue error.

*   **Note - Struct Type:** The Struct variant is for complex XMP structures. For this iteration, basic type matching is sufficient - if descriptor expects Struct and value is Struct, validation passes. Recursive validation of nested structure contents is out of scope for this task.

*   **Warning - Test Coverage:** The acceptance criteria requires "at least 5 validation scenarios (valid, wrong type, invalid date, etc.)". Based on the 7 TagValue variants, you MUST write tests covering: (1) Valid type match for each variant (7 tests), (2) Type mismatch scenarios (at least 3 tests), (3) Rational with zero denominator (1 test), (4) DateTime format validation if implemented. Aim for 15-20 unit tests total to meet the "comprehensive" requirement.

*   **Tip - Module Integration:** After implementing validation.rs, you MUST add `pub mod validation;` to `src/core/mod.rs` to expose the validation module. Also add `pub use validation::validate_tag_value;` if you want to re-export the function at the core module level for easier importing.

*   **Tip - Function Signature:** The task specifies the exact signature: `fn validate_tag_value(descriptor: &TagDescriptor, value: &TagValue) -> Result<(), ExifToolError>`. This returns `Result<(), ExifToolError>` where `Ok(())` means validation passed (no data returned, just success), and `Err(ExifToolError)` means validation failed. This signature is idiomatic for validation functions in Rust.

*   **Tip - Documentation:** Add comprehensive doc comments to the validate_tag_value function explaining: (1) What it validates, (2) When it returns Ok vs Err, (3) Example usage. Follow the documentation style seen in other core modules (see tag_descriptor.rs lines 92-117 for good examples).

*   **Tip - Error Messages:** Provide clear, actionable error messages. Good examples:
    - "Type mismatch for tag 'EXIF:Make': expected String but got Integer"
    - "Invalid Rational value for tag 'EXIF:ExposureTime': denominator cannot be zero"
    - "Type mismatch for tag 'GPS:GPSLatitude': expected Rational but got String"

*   **Code Pattern from Existing Tests:** The existing test modules follow this pattern (see tag_descriptor.rs lines 179-317):
    ```rust
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_validate_string_type_correct() {
            let descriptor = TagDescriptor::new(/* ... */);
            let value = TagValue::new_string("Canon");
            assert!(validate_tag_value(&descriptor, &value).is_ok());
        }

        #[test]
        fn test_validate_type_mismatch() {
            // String descriptor, Integer value
            let descriptor = TagDescriptor::new(/* String type */);
            let value = TagValue::new_integer(42);
            let result = validate_tag_value(&descriptor, &value);
            assert!(result.is_err());
            // Check error message contains expected text
        }
    }
    ```

*   **Critical Dependency Check:** You MUST verify that `src/core/mod.rs` currently exports the required types. Check that it has:
    - `pub mod tag_descriptor;` (exports TagDescriptor, ValueType)
    - `pub mod tag_value;` (exports TagValue)
    - After your implementation: `pub mod validation;`

### Summary of Required Changes

1. **src/core/validation.rs**: Implement `validate_tag_value()` function with comprehensive logic (~50-80 lines)
2. **src/core/validation.rs**: Add unit test module with 15-20 tests (~150-200 lines)
3. **src/core/mod.rs**: Add `pub mod validation;` (1 line change)

### Example Implementation Structure

```rust
//! Tag value validation engine
//!
//! This module provides validation logic for metadata tag values.

use crate::core::tag_descriptor::{TagDescriptor, ValueType};
use crate::core::tag_value::TagValue;
use crate::error::ExifToolError;

/// Validates that a TagValue matches the expected type defined in its TagDescriptor.
///
/// This function performs type checking to ensure tag values conform to their
/// schema definitions before write operations.
///
/// # Arguments
///
/// * `descriptor` - The tag descriptor containing the expected value type
/// * `value` - The tag value to validate
///
/// # Returns
///
/// * `Ok(())` if validation succeeds
/// * `Err(ExifToolError::InvalidTagValue)` if validation fails
///
/// # Examples
///
/// ```
/// use exiftool_rs::core::tag_descriptor::{TagDescriptor, TagId, FormatFamily, ValueType};
/// use exiftool_rs::core::tag_value::TagValue;
/// use exiftool_rs::core::validation::validate_tag_value;
///
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
/// ```
pub fn validate_tag_value(
    descriptor: &TagDescriptor,
    value: &TagValue,
) -> Result<(), ExifToolError> {
    // Implementation here
    // Match on value variant, compare against descriptor.value_type()
    // Return Ok(()) for matches, Err for mismatches
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    // Import test utilities

    #[test]
    fn test_validate_string_matches() { /* ... */ }

    #[test]
    fn test_validate_integer_matches() { /* ... */ }

    // ... 15+ more tests
}
```

### Acceptance Criteria Checklist

After implementation, verify:

- [ ] `validate_tag_value()` function implemented with correct signature
- [ ] Function checks String/Integer/Float/Rational/Binary/DateTime/Struct types
- [ ] Rational validation checks denominator != 0
- [ ] Function returns `Ok(())` for correct type matches
- [ ] Function returns `Err(InvalidTagValue)` for type mismatches
- [ ] Error messages are clear and actionable
- [ ] Unit tests cover all 7 TagValue variants (7 success tests)
- [ ] Unit tests cover type mismatches (3+ failure tests)
- [ ] Unit tests cover Rational zero denominator (1 test)
- [ ] Unit tests cover edge cases (empty string, max values, etc.)
- [ ] Total of 15+ unit tests implemented
- [ ] `cargo test validation` passes all tests
- [ ] `cargo clippy` shows no warnings
- [ ] Code has comprehensive documentation comments
- [ ] `src/core/mod.rs` exports validation module
