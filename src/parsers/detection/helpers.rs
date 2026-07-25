//! Format detection helper functions
//!
//! This module provides utility functions for pattern matching operations
//! used by format detection logic.

/// Check if bytes at a specific offset match a pattern
///
/// # Arguments
///
/// * `data` - The data buffer to search within
/// * `pattern` - The byte pattern to match
/// * `offset` - The offset within data where pattern should start
///
/// # Returns
///
/// `true` if the pattern matches at the specified offset, `false` otherwise
#[inline]
pub fn matches_at_offset(data: &[u8], pattern: &[u8], offset: usize) -> bool {
    if offset + pattern.len() > data.len() {
        return false;
    }
    &data[offset..offset + pattern.len()] == pattern
}

/// Check if data starts with any of the provided patterns
///
/// # Arguments
///
/// * `data` - The data buffer to check
/// * `patterns` - Slice of byte patterns to test against
///
/// # Returns
///
/// `true` if data starts with any of the patterns, `false` otherwise
#[inline]
pub fn starts_with_any(data: &[u8], patterns: &[&[u8]]) -> bool {
    patterns.iter().any(|pattern| data.starts_with(pattern))
}

/// Check if data contains a text pattern within the first N bytes
///
/// # Arguments
///
/// * `data` - The data buffer to search
/// * `pattern` - The text pattern to find
/// * `limit` - Maximum bytes to search from start
///
/// # Returns
///
/// `true` if pattern is found within the first `limit` bytes as valid UTF-8
#[inline]
pub fn contains_text(data: &[u8], pattern: &str, limit: usize) -> bool {
    if data.len() < limit {
        return false;
    }
    if let Ok(text) = std::str::from_utf8(&data[0..limit]) {
        text.contains(pattern)
    } else {
        false
    }
}

/// Returns the longest prefix of `data` that is valid UTF-8
///
/// Detection probes cut files at fixed byte offsets, which can split a
/// multibyte character at the end of the buffer. The split character must not
/// disqualify otherwise valid text, so callers should judge this prefix
/// instead of requiring the whole buffer to parse.
///
/// # Arguments
///
/// * `data` - The data buffer to interpret as UTF-8
///
/// # Returns
///
/// The longest valid UTF-8 prefix of `data` (empty if the first byte is invalid)
#[inline]
pub fn utf8_prefix(data: &[u8]) -> &str {
    match std::str::from_utf8(data) {
        Ok(text) => text,
        // valid_up_to() is always a character boundary, so this cannot fail.
        Err(error) => std::str::from_utf8(&data[..error.valid_up_to()]).unwrap_or(""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_at_offset() {
        let data = b"Hello World";
        assert!(matches_at_offset(data, b"Hello", 0));
        assert!(matches_at_offset(data, b"World", 6));
        assert!(!matches_at_offset(data, b"World", 0));
        assert!(!matches_at_offset(data, b"TooLong", 10));
    }

    #[test]
    fn test_starts_with_any() {
        let data = b"Test Data";
        assert!(starts_with_any(data, &[b"Test", b"Data"]));
        assert!(starts_with_any(data, &[b"Wrong", b"Test"]));
        assert!(!starts_with_any(data, &[b"Wrong", b"Data"]));
    }

    #[test]
    fn test_contains_text() {
        let data = b"This is a test string with some content";
        assert!(contains_text(data, "test", 39));
        assert!(contains_text(data, "content", 39));
        assert!(!contains_text(data, "missing", 39));
        assert!(!contains_text(data, "test", 10)); // Not enough bytes
    }
}
