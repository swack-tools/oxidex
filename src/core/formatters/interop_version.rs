//! Version byte decoding for EXIF interoperability and version tags
//!
//! This module provides decoding functions for version-related EXIF tags that store
//! their values as 4 ASCII bytes. These tags include:
//!
//! - **InteropVersion** (0x0002): Version of the DCF interoperability standard
//! - **ExifVersion** (0x9000): Version of the EXIF standard used
//! - **FlashpixVersion** (0xA000): Version of the FlashPix format supported
//!
//! # Binary Format
//!
//! All these version tags use the same format: 4 ASCII bytes representing a
//! 4-digit version string. For example:
//!
//! | Bytes (hex)           | ASCII       | Meaning              |
//! |-----------------------|-------------|----------------------|
//! | `[0x30, 0x31, 0x30, 0x30]` | "0100"  | Version 1.00         |
//! | `[0x30, 0x32, 0x33, 0x32]` | "0232"  | Version 2.32 (EXIF)  |
//! | `[0x30, 0x32, 0x32, 0x31]` | "0221"  | Version 2.21 (EXIF)  |
//!
//! # Problem Addressed
//!
//! OxiDex was outputting "[Binary data]" for these tags while ExifTool correctly
//! outputs the decoded version string (e.g., "0100"). This module provides the
//! decoding logic to match ExifTool's behavior.
//!
//! # Usage
//!
//! ```rust
//! use oxidex::core::formatters::interop_version::decode_version_bytes;
//!
//! // Decode 4 ASCII bytes to version string
//! let result = decode_version_bytes(b"0100");
//! assert_eq!(result, "0100");
//!
//! // Already decoded strings pass through unchanged
//! let result = decode_version_bytes("0232".as_bytes());
//! assert_eq!(result, "0232");
//! ```

/// Tags that use the 4-byte ASCII version format.
///
/// These tag names (with or without group prefix) should have their binary
/// data decoded using [`decode_version_bytes`].
pub const VERSION_TAGS: &[&str] = &["InteropVersion", "ExifVersion", "FlashpixVersion"];

/// Decode version bytes to a human-readable version string.
///
/// This function handles the common EXIF pattern where version information is
/// stored as 4 ASCII bytes. It is designed to be robust and handle multiple
/// input scenarios:
///
/// 1. **Raw 4-byte ASCII data**: The standard format where bytes like
///    `[0x30, 0x31, 0x30, 0x30]` decode to "0100"
///
/// 2. **Already-decoded strings**: If the input is already a valid ASCII
///    version string, it passes through unchanged
///
/// 3. **Non-printable bytes**: If bytes are not printable ASCII, returns
///    a hex representation for debugging purposes
///
/// 4. **Wrong length data**: Returns an appropriate fallback value
///
/// # Arguments
///
/// * `data` - Raw binary data or already-decoded string bytes
///
/// # Returns
///
/// A `String` containing the decoded version (e.g., "0100", "0232")
///
/// # Examples
///
/// ```rust
/// use oxidex::core::formatters::interop_version::decode_version_bytes;
///
/// // Standard 4-byte ASCII version
/// assert_eq!(decode_version_bytes(b"0100"), "0100");
/// assert_eq!(decode_version_bytes(b"0232"), "0232");
///
/// // Already decoded string (common when re-processing)
/// assert_eq!(decode_version_bytes("0100".as_bytes()), "0100");
///
/// // Edge cases
/// assert_eq!(decode_version_bytes(&[]), "");
/// assert_eq!(decode_version_bytes(b"01"), "01");
/// ```
pub fn decode_version_bytes(data: &[u8]) -> String {
    // Handle empty input
    if data.is_empty() {
        return String::new();
    }

    // Check if all bytes are printable ASCII (0x20-0x7E range)
    // Version strings should only contain digits '0'-'9' (0x30-0x39)
    let all_printable = data
        .iter()
        .all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace());

    if all_printable {
        // Data is already ASCII text, convert directly to string
        // Use from_utf8_lossy to handle any edge cases gracefully
        String::from_utf8_lossy(data).trim().to_string()
    } else {
        // Data contains non-printable bytes - this might be corrupted data
        // or a different encoding. Return hex representation for debugging.
        // In practice, valid version tags should always be ASCII.
        data.iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Check if a tag name is a version tag that needs byte decoding.
///
/// This function handles both simple tag names and fully-qualified names
/// with group prefixes (e.g., "EXIF:InteropVersion").
///
/// # Arguments
///
/// * `tag_name` - The tag name to check, optionally with group prefix
///
/// # Returns
///
/// `true` if the tag is a version tag, `false` otherwise
///
/// # Examples
///
/// ```rust
/// use oxidex::core::formatters::interop_version::is_version_tag;
///
/// assert!(is_version_tag("InteropVersion"));
/// assert!(is_version_tag("EXIF:ExifVersion"));
/// assert!(is_version_tag("FlashpixVersion"));
///
/// assert!(!is_version_tag("ImageWidth"));
/// assert!(!is_version_tag("Software"));
/// ```
pub fn is_version_tag(tag_name: &str) -> bool {
    // Extract the base tag name (after the colon if present)
    // This handles fully-qualified names like "EXIF:InteropVersion"
    let base_name = tag_name.rsplit(':').next().unwrap_or(tag_name);
    VERSION_TAGS.contains(&base_name)
}

/// Decode version data with tag-aware processing.
///
/// This is a convenience function that combines tag detection and decoding.
/// If the tag is not a version tag, the original data is returned as a string.
///
/// # Arguments
///
/// * `tag_name` - The tag name (e.g., "InteropVersion", "EXIF:ExifVersion")
/// * `data` - The raw byte data to decode
///
/// # Returns
///
/// The decoded version string if it's a version tag, or the data as a string otherwise.
///
/// # Examples
///
/// ```rust
/// use oxidex::core::formatters::interop_version::decode_version_for_tag;
///
/// // Version tags get decoded
/// assert_eq!(decode_version_for_tag("InteropVersion", b"0100"), "0100");
/// assert_eq!(decode_version_for_tag("EXIF:ExifVersion", b"0232"), "0232");
///
/// // Non-version tags pass through
/// assert_eq!(decode_version_for_tag("Software", b"Test"), "Test");
/// ```
pub fn decode_version_for_tag(tag_name: &str, data: &[u8]) -> String {
    if is_version_tag(tag_name) {
        decode_version_bytes(data)
    } else {
        // For non-version tags, just convert to string
        String::from_utf8_lossy(data).to_string()
    }
}

// =============================================================================
// UNIT TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // decode_version_bytes tests
    // =========================================================================

    #[test]
    fn test_decode_standard_version_bytes() {
        // Standard InteropVersion format
        assert_eq!(decode_version_bytes(b"0100"), "0100");

        // ExifVersion formats
        assert_eq!(decode_version_bytes(b"0232"), "0232");
        assert_eq!(decode_version_bytes(b"0230"), "0230");
        assert_eq!(decode_version_bytes(b"0221"), "0221");
        assert_eq!(decode_version_bytes(b"0220"), "0220");
        assert_eq!(decode_version_bytes(b"0210"), "0210");

        // FlashpixVersion format (typically "0100")
        assert_eq!(decode_version_bytes(b"0100"), "0100");
        assert_eq!(decode_version_bytes(b"0101"), "0101");
    }

    #[test]
    fn test_decode_already_decoded_strings() {
        // If the data is already a decoded string (e.g., from re-processing),
        // it should pass through unchanged
        assert_eq!(decode_version_bytes("0100".as_bytes()), "0100");
        assert_eq!(decode_version_bytes("0232".as_bytes()), "0232");
    }

    #[test]
    fn test_decode_empty_input() {
        assert_eq!(decode_version_bytes(&[]), "");
    }

    #[test]
    fn test_decode_short_input() {
        // Shorter than standard 4 bytes should still work
        assert_eq!(decode_version_bytes(b"01"), "01");
        assert_eq!(decode_version_bytes(b"010"), "010");
    }

    #[test]
    fn test_decode_longer_input() {
        // Longer than 4 bytes should return the full string
        assert_eq!(decode_version_bytes(b"01000"), "01000");
        assert_eq!(decode_version_bytes(b"0100 extra"), "0100 extra");
    }

    #[test]
    fn test_decode_non_printable_bytes() {
        // Non-printable bytes should be returned as hex
        assert_eq!(
            decode_version_bytes(&[0x00, 0x01, 0x02, 0x03]),
            "00 01 02 03"
        );
        assert_eq!(decode_version_bytes(&[0xFF, 0xFE]), "FF FE");
    }

    #[test]
    fn test_decode_mixed_printable_nonprintable() {
        // If any byte is non-printable, show as hex
        assert_eq!(
            decode_version_bytes(&[0x30, 0x00, 0x30, 0x30]),
            "30 00 30 30"
        );
    }

    #[test]
    fn test_decode_with_whitespace() {
        // Whitespace should be trimmed from ASCII strings
        assert_eq!(decode_version_bytes(b" 0100 "), "0100");
        assert_eq!(decode_version_bytes(b"0100\n"), "0100");
    }

    // =========================================================================
    // is_version_tag tests
    // =========================================================================

    #[test]
    fn test_is_version_tag_simple_names() {
        assert!(is_version_tag("InteropVersion"));
        assert!(is_version_tag("ExifVersion"));
        assert!(is_version_tag("FlashpixVersion"));
    }

    #[test]
    fn test_is_version_tag_with_group_prefix() {
        // Should handle group-prefixed names
        assert!(is_version_tag("EXIF:InteropVersion"));
        assert!(is_version_tag("EXIF:ExifVersion"));
        assert!(is_version_tag("EXIF:FlashpixVersion"));
        assert!(is_version_tag("Exif:InteropVersion"));
        assert!(is_version_tag("IFD0:InteropVersion"));
    }

    #[test]
    fn test_is_version_tag_non_version_tags() {
        assert!(!is_version_tag("ImageWidth"));
        assert!(!is_version_tag("Software"));
        assert!(!is_version_tag("Make"));
        assert!(!is_version_tag("Model"));
        assert!(!is_version_tag("EXIF:Make"));
        assert!(!is_version_tag(""));
    }

    #[test]
    fn test_is_version_tag_partial_matches() {
        // Should not match partial names
        assert!(!is_version_tag("Interop"));
        assert!(!is_version_tag("Version"));
        assert!(!is_version_tag("InteropVersionExtra"));
        assert!(!is_version_tag("XInteropVersion"));
    }

    // =========================================================================
    // decode_version_for_tag tests
    // =========================================================================

    #[test]
    fn test_decode_version_for_tag_interop() {
        assert_eq!(decode_version_for_tag("InteropVersion", b"0100"), "0100");
        assert_eq!(
            decode_version_for_tag("EXIF:InteropVersion", b"0100"),
            "0100"
        );
    }

    #[test]
    fn test_decode_version_for_tag_exif() {
        assert_eq!(decode_version_for_tag("ExifVersion", b"0232"), "0232");
        assert_eq!(decode_version_for_tag("EXIF:ExifVersion", b"0232"), "0232");
    }

    #[test]
    fn test_decode_version_for_tag_flashpix() {
        assert_eq!(decode_version_for_tag("FlashpixVersion", b"0100"), "0100");
        assert_eq!(
            decode_version_for_tag("EXIF:FlashpixVersion", b"0100"),
            "0100"
        );
    }

    #[test]
    fn test_decode_version_for_tag_non_version() {
        // Non-version tags should pass through the data as-is
        assert_eq!(decode_version_for_tag("Software", b"Test"), "Test");
        assert_eq!(decode_version_for_tag("Make", b"Canon"), "Canon");
    }

    // =========================================================================
    // VERSION_TAGS constant tests
    // =========================================================================

    #[test]
    fn test_version_tags_list() {
        assert!(VERSION_TAGS.contains(&"InteropVersion"));
        assert!(VERSION_TAGS.contains(&"ExifVersion"));
        assert!(VERSION_TAGS.contains(&"FlashpixVersion"));
        assert_eq!(VERSION_TAGS.len(), 3);
    }

    // =========================================================================
    // Edge case and real-world scenario tests
    // =========================================================================

    #[test]
    fn test_real_world_interop_version() {
        // Real InteropVersion data from JPEG files
        // The bytes [0x30, 0x31, 0x30, 0x30] represent "0100"
        let raw_bytes: [u8; 4] = [0x30, 0x31, 0x30, 0x30];
        assert_eq!(decode_version_bytes(&raw_bytes), "0100");
    }

    #[test]
    fn test_real_world_exif_version() {
        // Real ExifVersion data - EXIF 2.32
        // The bytes [0x30, 0x32, 0x33, 0x32] represent "0232"
        let raw_bytes: [u8; 4] = [0x30, 0x32, 0x33, 0x32];
        assert_eq!(decode_version_bytes(&raw_bytes), "0232");

        // EXIF 2.21
        let raw_bytes_221: [u8; 4] = [0x30, 0x32, 0x32, 0x31];
        assert_eq!(decode_version_bytes(&raw_bytes_221), "0221");
    }

    #[test]
    fn test_real_world_flashpix_version() {
        // Real FlashpixVersion data - version 1.00
        // The bytes [0x30, 0x31, 0x30, 0x30] represent "0100"
        let raw_bytes: [u8; 4] = [0x30, 0x31, 0x30, 0x30];
        assert_eq!(decode_version_bytes(&raw_bytes), "0100");
    }

    #[test]
    fn test_null_terminated_version_string() {
        // Some implementations might null-terminate the version string
        let with_null: [u8; 5] = [0x30, 0x31, 0x30, 0x30, 0x00];
        // Contains null byte, so returns hex
        assert_eq!(decode_version_bytes(&with_null), "30 31 30 30 00");

        // However, if we trim the null first (which callers might do):
        let trimmed = &with_null[..4];
        assert_eq!(decode_version_bytes(trimmed), "0100");
    }
}
