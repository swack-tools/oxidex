//! GPS Processing Method decoder for EXIF GPSProcessingMethod tag
//!
//! The GPSProcessingMethod tag (0x001B) in the GPS IFD stores information about
//! the method used to determine GPS location. The data format consists of:
//!
//! - **First 8 bytes**: Character code identifier specifying the text encoding
//! - **Remaining bytes**: The actual processing method string, typically null-padded
//!
//! # Character Code Identifiers
//!
//! | Identifier        | Encoding    | Description                      |
//! |-------------------|-------------|----------------------------------|
//! | `ASCII\0\0\0`     | ASCII       | Standard ASCII text              |
//! | `JIS\0\0\0\0\0`   | JIS X 0208  | Japanese Industrial Standard     |
//! | `UNICODE\0`       | UTF-16      | Unicode (typically little-endian)|
//! | `\0\0\0\0\0\0\0\0`| Undefined   | Encoding not specified           |
//!
//! # Common Processing Method Values
//!
//! - `GPS` - GPS satellite positioning
//! - `CELLID` - Cell tower triangulation
//! - `WLAN` - WiFi-based positioning
//! - `MANUAL` - Manually entered coordinates
//!
//! # References
//!
//! - EXIF 2.32 Specification, Section 4.6.6 (GPS Attribute Information)
//! - ExifTool GPSProcessingMethod documentation

/// Decode GPSProcessingMethod binary data to a human-readable string.
///
/// This function extracts the processing method string from the raw binary data
/// stored in the GPSProcessingMethod EXIF tag. The data format follows the EXIF
/// specification with an 8-byte character code prefix.
///
/// # Arguments
///
/// * `data` - Raw binary data from the GPSProcessingMethod tag. Expected format:
///   - Bytes 0-7: Character code identifier (ASCII, JIS, UNICODE, or Undefined)
///   - Bytes 8+: Processing method string (null-padded)
///
/// # Returns
///
/// A `String` containing the decoded processing method. Returns an empty string
/// if the data is too short (less than 8 bytes) or if the method string is empty.
///
/// # Encoding Handling
///
/// - **ASCII**: Decoded as UTF-8 (ASCII is a subset of UTF-8)
/// - **UNICODE**: Decoded as UTF-16 Little Endian (most common on Windows/cameras)
/// - **JIS**: Decoded as lossy UTF-8 (proper JIS would require external crate)
/// - **Undefined/Unknown**: Decoded as lossy UTF-8
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::gps_processing_method::decode_gps_processing_method;
///
/// // ASCII-encoded "GPS" method
/// let data = b"ASCII\0\0\0GPS\0\0\0\0\0";
/// assert_eq!(decode_gps_processing_method(data), "GPS");
///
/// // ASCII-encoded "CELLID" method
/// let data = b"ASCII\0\0\0CELLID\0\0";
/// assert_eq!(decode_gps_processing_method(data), "CELLID");
///
/// // Empty or too-short data
/// assert_eq!(decode_gps_processing_method(b"SHORT"), "");
/// ```
pub fn decode_gps_processing_method(data: &[u8]) -> String {
    // The minimum valid data is 8 bytes for the character code identifier.
    // If data is shorter, we cannot determine the encoding, so return empty.
    if data.len() < 8 {
        return String::new();
    }

    // Extract the 8-byte character code identifier and the remaining text data.
    let encoding = &data[0..8];
    let text_data = &data[8..];

    // If there's no text data after the encoding prefix, return empty string.
    if text_data.is_empty() {
        return String::new();
    }

    // Decode based on the character code identifier.
    // The EXIF spec defines these standard encoding prefixes.
    match encoding {
        b"ASCII\0\0\0" => {
            // ASCII encoding: Convert to UTF-8 string and strip null padding.
            // ASCII is a proper subset of UTF-8, so this conversion is safe.
            String::from_utf8_lossy(text_data)
                .trim_end_matches('\0')
                .trim()
                .to_string()
        }
        b"UNICODE\0" => {
            // Unicode (UTF-16) encoding: Most cameras use little-endian.
            // Convert pairs of bytes to UTF-16 code units, then decode to UTF-8.
            decode_utf16_le(text_data)
        }
        b"JIS\0\0\0\0\0" => {
            // JIS X 0208 encoding: Japanese character set.
            // For proper decoding, we would need the encoding_rs crate.
            // As a fallback, try UTF-8 lossy conversion (will work for ASCII subset).
            String::from_utf8_lossy(text_data)
                .trim_end_matches('\0')
                .trim()
                .to_string()
        }
        // Undefined encoding (all zeros) or unknown encoding prefix.
        // Try UTF-8 lossy conversion as a best-effort fallback.
        _ => String::from_utf8_lossy(text_data)
            .trim_end_matches('\0')
            .trim()
            .to_string(),
    }
}

/// Decode UTF-16 Little Endian bytes to a UTF-8 string.
///
/// This helper function converts UTF-16LE encoded bytes to a Rust String.
/// It handles null-termination and padding commonly found in EXIF strings.
///
/// # Arguments
///
/// * `data` - Raw bytes containing UTF-16LE encoded text
///
/// # Returns
///
/// A `String` with the decoded text, trimmed of null characters.
fn decode_utf16_le(data: &[u8]) -> String {
    // Convert pairs of bytes to UTF-16 code units (little-endian).
    // Filter out incomplete pairs at the end (odd byte count).
    let u16_data: Vec<u16> = data
        .chunks(2)
        .filter_map(|chunk| {
            if chunk.len() == 2 {
                Some(u16::from_le_bytes([chunk[0], chunk[1]]))
            } else {
                // Skip incomplete byte pair (odd-length data).
                None
            }
        })
        .collect();

    // Decode UTF-16 to UTF-8, using replacement characters for invalid sequences.
    String::from_utf16_lossy(&u16_data)
        .trim_end_matches('\0')
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== ASCII Encoding Tests ====================

    #[test]
    fn test_ascii_gps() {
        // Standard GPS method with ASCII encoding and null padding
        let data = b"ASCII\0\0\0GPS\0\0\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "GPS");
    }

    #[test]
    fn test_ascii_cellid() {
        // CELLID method (cell tower positioning)
        let data = b"ASCII\0\0\0CELLID\0\0";
        assert_eq!(decode_gps_processing_method(data), "CELLID");
    }

    #[test]
    fn test_ascii_wlan() {
        // WLAN method (WiFi positioning)
        let data = b"ASCII\0\0\0WLAN\0\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "WLAN");
    }

    #[test]
    fn test_ascii_manual() {
        // MANUAL method (manually entered coordinates)
        let data = b"ASCII\0\0\0MANUAL\0\0";
        assert_eq!(decode_gps_processing_method(data), "MANUAL");
    }

    #[test]
    fn test_ascii_no_null_padding() {
        // ASCII without null padding at the end
        let data = b"ASCII\0\0\0GPS";
        assert_eq!(decode_gps_processing_method(data), "GPS");
    }

    #[test]
    fn test_ascii_excessive_null_padding() {
        // ASCII with many null bytes at the end
        let data = b"ASCII\0\0\0GPS\0\0\0\0\0\0\0\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "GPS");
    }

    #[test]
    fn test_ascii_with_spaces() {
        // ASCII with leading/trailing spaces (should be trimmed)
        let data = b"ASCII\0\0\0  GPS  \0\0";
        assert_eq!(decode_gps_processing_method(data), "GPS");
    }

    #[test]
    fn test_ascii_empty_text() {
        // ASCII encoding but empty text (only nulls)
        let data = b"ASCII\0\0\0\0\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "");
    }

    #[test]
    fn test_ascii_longer_method_name() {
        // Longer custom method name
        let data = b"ASCII\0\0\0ASSISTED-GPS\0";
        assert_eq!(decode_gps_processing_method(data), "ASSISTED-GPS");
    }

    // ==================== Unicode (UTF-16) Encoding Tests ====================

    #[test]
    fn test_unicode_gps() {
        // "GPS" in UTF-16LE: G=0x0047, P=0x0050, S=0x0053
        let mut data = b"UNICODE\0".to_vec();
        data.extend_from_slice(&[0x47, 0x00, 0x50, 0x00, 0x53, 0x00, 0x00, 0x00]);
        assert_eq!(decode_gps_processing_method(&data), "GPS");
    }

    #[test]
    fn test_unicode_cellid() {
        // "CELLID" in UTF-16LE
        let mut data = b"UNICODE\0".to_vec();
        data.extend_from_slice(&[
            0x43, 0x00, // C
            0x45, 0x00, // E
            0x4C, 0x00, // L
            0x4C, 0x00, // L
            0x49, 0x00, // I
            0x44, 0x00, // D
            0x00, 0x00, // null terminator
        ]);
        assert_eq!(decode_gps_processing_method(&data), "CELLID");
    }

    #[test]
    fn test_unicode_empty() {
        // Unicode encoding with only null terminator
        let mut data = b"UNICODE\0".to_vec();
        data.extend_from_slice(&[0x00, 0x00]);
        assert_eq!(decode_gps_processing_method(&data), "");
    }

    // ==================== JIS Encoding Tests ====================

    #[test]
    fn test_jis_gps() {
        // JIS encoding with ASCII-compatible "GPS" (ASCII subset works with UTF-8 lossy)
        let data = b"JIS\0\0\0\0\0GPS\0";
        assert_eq!(decode_gps_processing_method(data), "GPS");
    }

    #[test]
    fn test_jis_empty() {
        // JIS encoding with empty text
        let data = b"JIS\0\0\0\0\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "");
    }

    // ==================== Undefined/Unknown Encoding Tests ====================

    #[test]
    fn test_undefined_encoding_gps() {
        // All-zeros encoding (undefined) with GPS text
        let data = b"\0\0\0\0\0\0\0\0GPS\0";
        assert_eq!(decode_gps_processing_method(data), "GPS");
    }

    #[test]
    fn test_unknown_encoding() {
        // Unknown encoding prefix - should still attempt to decode text
        let data = b"CUSTOM\0\0GPS\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "GPS");
    }

    #[test]
    fn test_garbage_encoding() {
        // Random bytes as encoding - should still extract text
        let data = b"\xFF\xFE\xFD\xFC\xFB\xFA\xF9\xF8GPS\0";
        assert_eq!(decode_gps_processing_method(data), "GPS");
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_empty_data() {
        // Completely empty data
        assert_eq!(decode_gps_processing_method(&[]), "");
    }

    #[test]
    fn test_too_short_data() {
        // Data shorter than 8 bytes (encoding prefix)
        assert_eq!(decode_gps_processing_method(b"ASCII"), "");
        assert_eq!(decode_gps_processing_method(b"SHORT"), "");
        assert_eq!(decode_gps_processing_method(b"1234567"), "");
    }

    #[test]
    fn test_exactly_8_bytes() {
        // Exactly 8 bytes (just encoding, no text)
        let data = b"ASCII\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "");
    }

    #[test]
    fn test_single_char_text() {
        // Minimum text: single character after encoding
        let data = b"ASCII\0\0\0G";
        assert_eq!(decode_gps_processing_method(data), "G");
    }

    #[test]
    fn test_only_nulls_as_text() {
        // Encoding followed by only null bytes
        let data = b"ASCII\0\0\0\0\0\0\0\0\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "");
    }

    #[test]
    fn test_mixed_case() {
        // Mixed case method name (should be preserved)
        let data = b"ASCII\0\0\0GpS-Assisted\0";
        assert_eq!(decode_gps_processing_method(data), "GpS-Assisted");
    }

    // ==================== UTF-16 Helper Tests ====================

    #[test]
    fn test_decode_utf16_le_simple() {
        // "Hi" in UTF-16LE
        let data = [0x48, 0x00, 0x69, 0x00, 0x00, 0x00];
        assert_eq!(decode_utf16_le(&data), "Hi");
    }

    #[test]
    fn test_decode_utf16_le_odd_length() {
        // Odd number of bytes (last byte should be ignored)
        let data = [0x48, 0x00, 0x69, 0x00, 0xFF];
        assert_eq!(decode_utf16_le(&data), "Hi");
    }

    #[test]
    fn test_decode_utf16_le_empty() {
        assert_eq!(decode_utf16_le(&[]), "");
    }

    #[test]
    fn test_decode_utf16_le_only_null() {
        let data = [0x00, 0x00];
        assert_eq!(decode_utf16_le(&data), "");
    }

    // ==================== Real-World Data Simulation ====================

    #[test]
    fn test_real_world_gps_typical() {
        // Simulating typical camera output for GPS positioning
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(b"ASCII\0\0\0");
        data.extend_from_slice(b"GPS");
        // Pad to typical 24-byte field
        while data.len() < 24 {
            data.push(0);
        }
        assert_eq!(decode_gps_processing_method(&data), "GPS");
    }

    #[test]
    fn test_real_world_cellid_typical() {
        // Simulating typical smartphone output for cell tower positioning
        let mut data = Vec::with_capacity(24);
        data.extend_from_slice(b"ASCII\0\0\0");
        data.extend_from_slice(b"CELLID");
        while data.len() < 24 {
            data.push(0);
        }
        assert_eq!(decode_gps_processing_method(&data), "CELLID");
    }

    #[test]
    fn test_network_method() {
        // Some devices use "NETWORK" for network-based positioning
        let data = b"ASCII\0\0\0NETWORK\0\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "NETWORK");
    }

    #[test]
    fn test_fused_method() {
        // Some Android devices use "fused" for fused location provider
        let data = b"ASCII\0\0\0fused\0\0\0\0\0";
        assert_eq!(decode_gps_processing_method(data), "fused");
    }
}
