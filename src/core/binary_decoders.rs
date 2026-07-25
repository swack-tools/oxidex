//! Binary data decoders for specific EXIF tags
//!
//! This module provides decoding functions for EXIF tags that store binary data
//! which ExifTool displays as human-readable strings. Many EXIF tags store
//! information in compact binary formats that need to be interpreted.
//!
//! # Supported Tags
//!
//! | Tag ID | Tag Name              | Binary Format          | Decoded Example           |
//! |--------|-----------------------|------------------------|---------------------------|
//! | 0x9000 | ExifVersion           | 4 ASCII bytes          | "0232"                    |
//! | 0x9101 | ComponentsConfiguration | 4 component bytes    | "Y, Cb, Cr"               |
//! | 0x9286 | UserComment           | 8-byte encoding + text | "Hello World"             |
//! | 0xA000 | FlashpixVersion       | 4 ASCII bytes          | "0100"                    |
//! | 0xA300 | FileSource            | 1 byte enum            | "Digital Camera"          |
//! | 0xA301 | SceneType             | 1 byte enum            | "Directly photographed"   |
//! | 0xA302 | CFAPattern            | H*V color array        | "[Green,Blue][Red,Green]" |

/// Decode EXIF version bytes to a human-readable string.
///
/// The EXIF version is stored as 4 ASCII bytes representing the version number.
/// For example, bytes `[0x30, 0x32, 0x33, 0x32]` (ASCII "0232") represents
/// EXIF version 2.32.
///
/// # Arguments
///
/// * `data` - Raw binary data containing the version bytes
///
/// # Returns
///
/// * `Some(String)` - The version string if data has at least 4 bytes
/// * `None` - If data is too short
///
/// # Examples
///
/// ```
/// use oxidex::core::binary_decoders::decode_exif_version;
///
/// assert_eq!(decode_exif_version(b"0232"), Some("0232".to_string()));
/// assert_eq!(decode_exif_version(b"0230"), Some("0230".to_string()));
/// assert_eq!(decode_exif_version(b"02"), None); // Too short
/// ```
pub fn decode_exif_version(data: &[u8]) -> Option<String> {
    if data.len() >= 4 {
        Some(String::from_utf8_lossy(&data[0..4]).to_string())
    } else {
        None
    }
}

/// Decode FlashPix version bytes to a human-readable string.
///
/// Uses the same format as EXIF version - 4 ASCII bytes representing the version.
///
/// # Arguments
///
/// * `data` - Raw binary data containing the version bytes
///
/// # Returns
///
/// * `Some(String)` - The version string if data has at least 4 bytes
/// * `None` - If data is too short
///
/// # Examples
///
/// ```
/// use oxidex::core::binary_decoders::decode_flashpix_version;
///
/// assert_eq!(decode_flashpix_version(b"0100"), Some("0100".to_string()));
/// assert_eq!(decode_flashpix_version(b"01"), None); // Too short
/// ```
pub fn decode_flashpix_version(data: &[u8]) -> Option<String> {
    decode_exif_version(data)
}

/// Decode FileSource tag (0xA300) to a human-readable string.
///
/// The FileSource tag indicates how the image was captured. It's stored as
/// a single byte with the following meanings:
///
/// - 1: Film Scanner
/// - 2: Reflection Print Scanner
/// - 3: Digital Camera
///
/// # Arguments
///
/// * `data` - Raw binary data containing the file source byte
///
/// # Returns
///
/// * `Some(String)` - The human-readable source description
/// * `None` - If data is empty
///
/// # Examples
///
/// ```
/// use oxidex::core::binary_decoders::decode_file_source;
///
/// assert_eq!(decode_file_source(&[3]), Some("Digital Camera".to_string()));
/// assert_eq!(decode_file_source(&[1]), Some("Film Scanner".to_string()));
/// assert_eq!(decode_file_source(&[2]), Some("Reflection Print Scanner".to_string()));
/// assert_eq!(decode_file_source(&[5]), Some("Unknown (5)".to_string()));
/// assert_eq!(decode_file_source(&[]), None);
/// ```
pub fn decode_file_source(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        1 => Some("Film Scanner".to_string()),
        2 => Some("Reflection Print Scanner".to_string()),
        3 => Some("Digital Camera".to_string()),
        other => Some(format!("Unknown ({})", other)),
    }
}

/// Decode SceneType tag (0xA301) to a human-readable string.
///
/// The SceneType tag indicates how the scene was captured. Currently only
/// one value is defined in the EXIF specification:
///
/// - 1: Directly photographed
///
/// # Arguments
///
/// * `data` - Raw binary data containing the scene type byte
///
/// # Returns
///
/// * `Some(String)` - The human-readable scene type description
/// * `None` - If data is empty
///
/// # Examples
///
/// ```
/// use oxidex::core::binary_decoders::decode_scene_type;
///
/// assert_eq!(decode_scene_type(&[1]), Some("Directly photographed".to_string()));
/// assert_eq!(decode_scene_type(&[2]), Some("Unknown (2)".to_string()));
/// assert_eq!(decode_scene_type(&[]), None);
/// ```
pub fn decode_scene_type(data: &[u8]) -> Option<String> {
    if data.is_empty() {
        return None;
    }
    match data[0] {
        1 => Some("Directly photographed".to_string()),
        other => Some(format!("Unknown ({})", other)),
    }
}

/// Decode CFA Pattern tag (0xA302) to a human-readable string.
///
/// The Color Filter Array (CFA) pattern describes the layout of color filters
/// on the image sensor. The format is:
///
/// - 2 bytes: Horizontal repeat count (big-endian)
/// - 2 bytes: Vertical repeat count (big-endian)
/// - H*V bytes: Color values for each position in the pattern
///
/// Color values are:
/// - 0: Red
/// - 1: Green
/// - 2: Blue
/// - 3: Cyan
/// - 4: Magenta
/// - 5: Yellow
/// - 6: White
///
/// # Arguments
///
/// * `data` - Raw binary data containing the CFA pattern
///
/// # Returns
///
/// * `Some(String)` - Pattern like "[Green,Blue][Red,Green]" for a 2x2 GBRG pattern
/// * `None` - If data is too short or malformed
///
/// # Examples
///
/// ```
/// use oxidex::core::binary_decoders::decode_cfa_pattern;
///
/// // 2x2 RGGB pattern (common Bayer pattern)
/// let data = [0, 2, 0, 2, 0, 1, 1, 2]; // Red, Green / Green, Blue
/// assert_eq!(decode_cfa_pattern(&data), Some("[Red,Green][Green,Blue]".to_string()));
///
/// // 2x2 GBRG pattern
/// let data2 = [0, 2, 0, 2, 1, 2, 0, 1]; // Green, Blue / Red, Green
/// assert_eq!(decode_cfa_pattern(&data2), Some("[Green,Blue][Red,Green]".to_string()));
/// ```
pub fn decode_cfa_pattern(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }

    // Parse horizontal and vertical repeat counts (big-endian)
    let h_repeat = u16::from_be_bytes([data[0], data[1]]) as usize;
    let v_repeat = u16::from_be_bytes([data[2], data[3]]) as usize;

    // Validate we have enough data for the pattern
    if data.len() < 4 + h_repeat * v_repeat {
        return None;
    }

    // Color names as defined in EXIF specification
    let colors = ["Red", "Green", "Blue", "Cyan", "Magenta", "Yellow", "White"];
    let mut result = String::new();

    // Build the pattern string row by row
    for row in 0..v_repeat {
        result.push('[');
        for col in 0..h_repeat {
            let idx = data[4 + row * h_repeat + col] as usize;
            if col > 0 {
                result.push(',');
            }
            result.push_str(colors.get(idx).unwrap_or(&"Unknown"));
        }
        result.push(']');
    }

    Some(result)
}

/// Decode UserComment tag (0x9286) to a human-readable string.
///
/// The UserComment tag has an 8-byte encoding identifier followed by the
/// actual text data. Supported encodings are:
///
/// - `ASCII\0\0\0` - ASCII text
/// - `UNICODE\0` - UTF-16 text (little-endian attempted first)
/// - `JIS\0\0\0\0\0` - JIS encoding (treated as lossy UTF-8)
/// - Unknown/missing - Treated as UTF-8
///
/// # Arguments
///
/// * `data` - Raw binary data containing encoding prefix and text
///
/// # Returns
///
/// * `Some(String)` - The decoded text with null terminators removed
/// * `None` - If data is too short (less than 8 bytes)
///
/// # Examples
///
/// ```
/// use oxidex::core::binary_decoders::decode_user_comment;
///
/// // ASCII encoded comment
/// let mut data = b"ASCII\0\0\0Hello World\0".to_vec();
/// assert_eq!(decode_user_comment(&data), Some("Hello World".to_string()));
///
/// // Empty comment with encoding prefix only
/// let empty = b"ASCII\0\0\0";
/// assert_eq!(decode_user_comment(empty), Some("".to_string()));
/// ```
pub fn decode_user_comment(data: &[u8]) -> Option<String> {
    if data.len() < 8 {
        return None;
    }

    let encoding = &data[0..8];
    let text_data = &data[8..];

    match encoding {
        b"ASCII\0\0\0" => {
            // ASCII encoding - simple UTF-8 conversion with null trimming
            Some(
                String::from_utf8_lossy(text_data)
                    .trim_end_matches('\0')
                    .to_string(),
            )
        }
        b"UNICODE\0" => {
            // UTF-16 encoding - try little-endian first (most common on Windows)
            let u16_data: Vec<u16> = text_data
                .chunks(2)
                .filter_map(|c| {
                    if c.len() == 2 {
                        Some(u16::from_le_bytes([c[0], c[1]]))
                    } else {
                        None
                    }
                })
                .collect();
            Some(
                String::from_utf16_lossy(&u16_data)
                    .trim_end_matches('\0')
                    .to_string(),
            )
        }
        b"JIS\0\0\0\0\0" => {
            // JIS encoding - use lossy UTF-8 conversion as a fallback
            // Note: Proper JIS decoding would require an external crate
            Some(
                String::from_utf8_lossy(text_data)
                    .trim_end_matches('\0')
                    .to_string(),
            )
        }
        _ => {
            // Unknown encoding or no encoding prefix - try UTF-8
            Some(
                String::from_utf8_lossy(text_data)
                    .trim_end_matches('\0')
                    .to_string(),
            )
        }
    }
}

/// Decode ComponentsConfiguration tag (0x9101) to a human-readable string.
///
/// The ComponentsConfiguration tag describes the order of pixel components.
/// It's stored as 4 bytes, each representing a component type:
///
/// - 0: Does not exist
/// - 1: Y (luminance)
/// - 2: Cb (blue chrominance)
/// - 3: Cr (red chrominance)
/// - 4: R (red)
/// - 5: G (green)
/// - 6: B (blue)
///
/// # Arguments
///
/// * `data` - Raw binary data containing 4 component bytes
///
/// # Returns
///
/// * `Some(String)` - Components like "Y, Cb, Cr" for YCbCr format
/// * `None` - If data has fewer than 4 bytes
///
/// # Examples
///
/// ```
/// use oxidex::core::binary_decoders::decode_components_configuration;
///
/// // Standard YCbCr configuration
/// assert_eq!(decode_components_configuration(&[1, 2, 3, 0]), Some("Y, Cb, Cr".to_string()));
///
/// // RGB configuration
/// assert_eq!(decode_components_configuration(&[4, 5, 6, 0]), Some("R, G, B".to_string()));
///
/// // Full 4-component configuration
/// assert_eq!(decode_components_configuration(&[1, 2, 3, 1]), Some("Y, Cb, Cr, Y".to_string()));
/// ```
pub fn decode_components_configuration(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }

    let components: Vec<&str> = data[0..4]
        .iter()
        .filter_map(|&b| match b {
            0 => None, // Does not exist - skip
            1 => Some("Y"),
            2 => Some("Cb"),
            3 => Some("Cr"),
            4 => Some("R"),
            5 => Some("G"),
            6 => Some("B"),
            _ => Some("?"),
        })
        .collect();

    Some(components.join(", "))
}

/// Master function to decode binary EXIF data by tag ID.
///
/// This function serves as a dispatcher that routes binary data to the
/// appropriate decoder based on the EXIF tag ID. Use this when processing
/// tags during metadata extraction.
///
/// # Arguments
///
/// * `tag_id` - The EXIF tag ID (e.g., 0x9000 for ExifVersion)
/// * `data` - The raw binary data for the tag
///
/// # Returns
///
/// * `Some(String)` - The decoded human-readable value if the tag is supported
/// * `None` - If the tag is not supported or decoding fails
///
/// # Supported Tag IDs
///
/// | Tag ID | Tag Name                |
/// |--------|-------------------------|
/// | 0x9000 | ExifVersion             |
/// | 0x9101 | ComponentsConfiguration |
/// | 0x9286 | UserComment             |
/// | 0xA000 | FlashpixVersion         |
/// | 0xA300 | FileSource              |
/// | 0xA301 | SceneType               |
/// | 0xA302 | CFAPattern              |
///
/// # Examples
///
/// ```
/// use oxidex::core::binary_decoders::decode_binary_exif;
///
/// // Decode ExifVersion
/// assert_eq!(decode_binary_exif(0x9000, b"0232"), Some("0232".to_string()));
///
/// // Decode FileSource
/// assert_eq!(decode_binary_exif(0xA300, &[3]), Some("Digital Camera".to_string()));
///
/// // Unknown tag returns None
/// assert_eq!(decode_binary_exif(0x0000, &[1, 2, 3]), None);
/// ```
pub fn decode_binary_exif(tag_id: u16, data: &[u8]) -> Option<String> {
    match tag_id {
        0x9000 => decode_exif_version(data),             // ExifVersion
        0x9101 => decode_components_configuration(data), // ComponentsConfiguration
        0x9286 => decode_user_comment(data),             // UserComment
        0xA000 => decode_flashpix_version(data),         // FlashpixVersion
        0xA300 => decode_file_source(data),              // FileSource
        0xA301 => decode_scene_type(data),               // SceneType
        0xA302 => decode_cfa_pattern(data),              // CFAPattern
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== ExifVersion Tests ====================

    #[test]
    fn test_exif_version_valid() {
        assert_eq!(decode_exif_version(b"0232"), Some("0232".to_string()));
        assert_eq!(decode_exif_version(b"0230"), Some("0230".to_string()));
        assert_eq!(decode_exif_version(b"0220"), Some("0220".to_string()));
        assert_eq!(decode_exif_version(b"0210"), Some("0210".to_string()));
    }

    #[test]
    fn test_exif_version_extra_data() {
        // Extra bytes should be ignored
        assert_eq!(decode_exif_version(b"0232extra"), Some("0232".to_string()));
    }

    #[test]
    fn test_exif_version_too_short() {
        assert_eq!(decode_exif_version(b"023"), None);
        assert_eq!(decode_exif_version(b"02"), None);
        assert_eq!(decode_exif_version(b""), None);
    }

    // ==================== FlashpixVersion Tests ====================

    #[test]
    fn test_flashpix_version() {
        assert_eq!(decode_flashpix_version(b"0100"), Some("0100".to_string()));
        assert_eq!(decode_flashpix_version(b"0101"), Some("0101".to_string()));
        assert_eq!(decode_flashpix_version(b"01"), None);
    }

    // ==================== FileSource Tests ====================

    #[test]
    fn test_file_source_known_values() {
        assert_eq!(decode_file_source(&[1]), Some("Film Scanner".to_string()));
        assert_eq!(
            decode_file_source(&[2]),
            Some("Reflection Print Scanner".to_string())
        );
        assert_eq!(decode_file_source(&[3]), Some("Digital Camera".to_string()));
    }

    #[test]
    fn test_file_source_unknown_values() {
        assert_eq!(decode_file_source(&[0]), Some("Unknown (0)".to_string()));
        assert_eq!(decode_file_source(&[4]), Some("Unknown (4)".to_string()));
        assert_eq!(
            decode_file_source(&[255]),
            Some("Unknown (255)".to_string())
        );
    }

    #[test]
    fn test_file_source_empty() {
        assert_eq!(decode_file_source(&[]), None);
    }

    #[test]
    fn test_file_source_extra_data() {
        // Extra bytes should be ignored
        assert_eq!(
            decode_file_source(&[3, 0, 0, 0]),
            Some("Digital Camera".to_string())
        );
    }

    // ==================== SceneType Tests ====================

    #[test]
    fn test_scene_type_known_values() {
        assert_eq!(
            decode_scene_type(&[1]),
            Some("Directly photographed".to_string())
        );
    }

    #[test]
    fn test_scene_type_unknown_values() {
        assert_eq!(decode_scene_type(&[0]), Some("Unknown (0)".to_string()));
        assert_eq!(decode_scene_type(&[2]), Some("Unknown (2)".to_string()));
    }

    #[test]
    fn test_scene_type_empty() {
        assert_eq!(decode_scene_type(&[]), None);
    }

    // ==================== CFAPattern Tests ====================

    #[test]
    fn test_cfa_pattern_rggb() {
        // Common RGGB Bayer pattern: Red, Green / Green, Blue
        let data = [0, 2, 0, 2, 0, 1, 1, 2];
        assert_eq!(
            decode_cfa_pattern(&data),
            Some("[Red,Green][Green,Blue]".to_string())
        );
    }

    #[test]
    fn test_cfa_pattern_gbrg() {
        // GBRG pattern: Green, Blue / Red, Green
        let data = [0, 2, 0, 2, 1, 2, 0, 1];
        assert_eq!(
            decode_cfa_pattern(&data),
            Some("[Green,Blue][Red,Green]".to_string())
        );
    }

    #[test]
    fn test_cfa_pattern_bggr() {
        // BGGR pattern: Blue, Green / Green, Red
        let data = [0, 2, 0, 2, 2, 1, 1, 0];
        assert_eq!(
            decode_cfa_pattern(&data),
            Some("[Blue,Green][Green,Red]".to_string())
        );
    }

    #[test]
    fn test_cfa_pattern_grbg() {
        // GRBG pattern: Green, Red / Blue, Green
        let data = [0, 2, 0, 2, 1, 0, 2, 1];
        assert_eq!(
            decode_cfa_pattern(&data),
            Some("[Green,Red][Blue,Green]".to_string())
        );
    }

    #[test]
    fn test_cfa_pattern_with_other_colors() {
        // Pattern with cyan, magenta, yellow
        let data = [0, 2, 0, 1, 3, 4]; // 2x1: Cyan, Magenta
        assert_eq!(
            decode_cfa_pattern(&data),
            Some("[Cyan,Magenta]".to_string())
        );
    }

    #[test]
    fn test_cfa_pattern_unknown_color() {
        // Pattern with unknown color index
        let data = [0, 2, 0, 1, 7, 8]; // 2x1: Unknown colors
        assert_eq!(
            decode_cfa_pattern(&data),
            Some("[Unknown,Unknown]".to_string())
        );
    }

    #[test]
    fn test_cfa_pattern_too_short() {
        assert_eq!(decode_cfa_pattern(&[0, 2, 0]), None);
        assert_eq!(decode_cfa_pattern(&[0, 2]), None);
        assert_eq!(decode_cfa_pattern(&[]), None);
    }

    #[test]
    fn test_cfa_pattern_insufficient_color_data() {
        // Header says 2x2 but only 3 color bytes provided
        let data = [0, 2, 0, 2, 0, 1, 1];
        assert_eq!(decode_cfa_pattern(&data), None);
    }

    // ==================== UserComment Tests ====================

    #[test]
    fn test_user_comment_ascii() {
        let data = b"ASCII\0\0\0Hello World\0";
        assert_eq!(decode_user_comment(data), Some("Hello World".to_string()));
    }

    #[test]
    fn test_user_comment_ascii_no_null() {
        let data = b"ASCII\0\0\0No trailing null";
        assert_eq!(
            decode_user_comment(data),
            Some("No trailing null".to_string())
        );
    }

    #[test]
    fn test_user_comment_ascii_multiple_nulls() {
        let data = b"ASCII\0\0\0Test\0\0\0";
        assert_eq!(decode_user_comment(data), Some("Test".to_string()));
    }

    #[test]
    fn test_user_comment_ascii_empty() {
        let data = b"ASCII\0\0\0";
        assert_eq!(decode_user_comment(data), Some("".to_string()));
    }

    #[test]
    fn test_user_comment_unicode() {
        // "Hi" in UTF-16LE: H=0x48, i=0x69
        let mut data = b"UNICODE\0".to_vec();
        data.extend_from_slice(&[0x48, 0x00, 0x69, 0x00, 0x00, 0x00]);
        assert_eq!(decode_user_comment(&data), Some("Hi".to_string()));
    }

    #[test]
    fn test_user_comment_unknown_encoding() {
        // Unknown encoding prefix - treat text as UTF-8
        let data = b"UNKNOWN\0Test text";
        assert_eq!(decode_user_comment(data), Some("Test text".to_string()));
    }

    #[test]
    fn test_user_comment_too_short() {
        assert_eq!(decode_user_comment(b"ASCII\0\0"), None);
        assert_eq!(decode_user_comment(b""), None);
    }

    // ==================== ComponentsConfiguration Tests ====================

    #[test]
    fn test_components_configuration_ycbcr() {
        // Standard YCbCr configuration
        assert_eq!(
            decode_components_configuration(&[1, 2, 3, 0]),
            Some("Y, Cb, Cr".to_string())
        );
    }

    #[test]
    fn test_components_configuration_ycbcry() {
        // 4-component YCbCrY
        assert_eq!(
            decode_components_configuration(&[1, 2, 3, 1]),
            Some("Y, Cb, Cr, Y".to_string())
        );
    }

    #[test]
    fn test_components_configuration_rgb() {
        assert_eq!(
            decode_components_configuration(&[4, 5, 6, 0]),
            Some("R, G, B".to_string())
        );
    }

    #[test]
    fn test_components_configuration_all_zeros() {
        // All zeros means no components
        assert_eq!(
            decode_components_configuration(&[0, 0, 0, 0]),
            Some("".to_string())
        );
    }

    #[test]
    fn test_components_configuration_unknown() {
        // Unknown component values
        assert_eq!(
            decode_components_configuration(&[7, 8, 9, 0]),
            Some("?, ?, ?".to_string())
        );
    }

    #[test]
    fn test_components_configuration_too_short() {
        assert_eq!(decode_components_configuration(&[1, 2, 3]), None);
        assert_eq!(decode_components_configuration(&[1, 2]), None);
        assert_eq!(decode_components_configuration(&[]), None);
    }

    // ==================== decode_binary_exif Master Function Tests ====================

    #[test]
    fn test_decode_binary_exif_exif_version() {
        assert_eq!(
            decode_binary_exif(0x9000, b"0232"),
            Some("0232".to_string())
        );
    }

    #[test]
    fn test_decode_binary_exif_components_configuration() {
        assert_eq!(
            decode_binary_exif(0x9101, &[1, 2, 3, 0]),
            Some("Y, Cb, Cr".to_string())
        );
    }

    #[test]
    fn test_decode_binary_exif_user_comment() {
        let data = b"ASCII\0\0\0GCM_TAG";
        assert_eq!(
            decode_binary_exif(0x9286, data),
            Some("GCM_TAG".to_string())
        );
    }

    #[test]
    fn test_decode_binary_exif_flashpix_version() {
        assert_eq!(
            decode_binary_exif(0xA000, b"0100"),
            Some("0100".to_string())
        );
    }

    #[test]
    fn test_decode_binary_exif_file_source() {
        assert_eq!(
            decode_binary_exif(0xA300, &[3]),
            Some("Digital Camera".to_string())
        );
    }

    #[test]
    fn test_decode_binary_exif_scene_type() {
        assert_eq!(
            decode_binary_exif(0xA301, &[1]),
            Some("Directly photographed".to_string())
        );
    }

    #[test]
    fn test_decode_binary_exif_cfa_pattern() {
        let data = [0, 2, 0, 2, 1, 2, 0, 1];
        assert_eq!(
            decode_binary_exif(0xA302, &data),
            Some("[Green,Blue][Red,Green]".to_string())
        );
    }

    #[test]
    fn test_decode_binary_exif_unsupported_tag() {
        assert_eq!(decode_binary_exif(0x0000, &[1, 2, 3]), None);
        assert_eq!(decode_binary_exif(0xFFFF, &[1, 2, 3]), None);
    }
}
