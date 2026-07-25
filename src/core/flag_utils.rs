//! Utilities for decoding bit flags into human-readable strings

/// Decodes bit flags into a vector of string labels.
///
/// # Arguments
/// * `value` - The bit field value to decode
/// * `flags` - Slice of (mask, label) tuples
///
/// # Example
/// ```
/// use oxidex::core::decode_flags;
/// let flags = decode_flags(
///     0x2003,
///     &[
///         (0x0001, "Flag A"),
///         (0x0002, "Flag B"),
///         (0x2000, "Flag C"),
///     ],
/// );
/// assert_eq!(flags, vec!["Flag A", "Flag B", "Flag C"]);
/// ```
pub fn decode_flags<'a>(value: u32, flags: &'a [(u32, &'a str)]) -> Vec<&'a str> {
    flags
        .iter()
        .filter(|(mask, _)| (value & mask) != 0)
        .map(|(_, label)| *label)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_flags_multiple_set() {
        let result = decode_flags(
            0x2003,
            &[
                (0x0001, "No relocs"),
                (0x0002, "Executable"),
                (0x2000, "DLL"),
            ],
        );
        assert_eq!(result, vec!["No relocs", "Executable", "DLL"]);
    }

    #[test]
    fn test_decode_flags_none_set() {
        let result = decode_flags(0x0000, &[(0x0001, "Flag A"), (0x0002, "Flag B")]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_decode_flags_partial_match() {
        let result = decode_flags(
            0x0004,
            &[(0x0001, "Flag A"), (0x0004, "Flag C"), (0x0008, "Flag D")],
        );
        assert_eq!(result, vec!["Flag C"]);
    }
}
