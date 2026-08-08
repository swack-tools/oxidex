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
//! | `UNICODE\0`       | UTF-16      | Unicode; a leading BOM selects byte order, else little-endian|
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
/// - **UNICODE**: Decoded as UTF-16, honoring a leading byte-order mark and
///   otherwise defaulting to little-endian (see [`decode_unicode_gps_text`])
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
            // Unicode (UTF-16) encoding.
            decode_unicode_gps_text(text_data)
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

/// Decode the `UNICODE\0`-prefixed text carried by GPSProcessingMethod
/// (GPS.pm 0x001b) and GPSAreaInformation (GPS.pm 0x001c). Both declare
/// `RawConv => 'Image::ExifTool::Exif::ConvertExifText($self,$val,1,$tag)'`
/// (GPS.pm 13.59:299, :305). For the `UNICODE` id, `ConvertExifText`
/// (Exif.pm:5586) calls `$et->Decode($str, 'UTF16', 'Unknown')`.
///
/// `Decode` -> `Decompose` (Charset.pm) runs the 2-byte fixed-width branch:
/// a leading byte-order mark overrides the declared/guessed order
/// (`Charset.pm:203`, `$val =~ s/^(\xfe\xff|\xff\xfe)//`; `\xfe\xff` selects
/// big-endian, `\xff\xfe` selects little-endian). `Charset.pm:147` states the
/// rule outright -- "byte order mark observed and then removed with UCS2 and
/// UCS4".
///
/// With no BOM, the order is genuinely unknown and ExifTool *guesses* it
/// (`Charset.pm:213-228`: count distinct high vs. low bytes across the code
/// units, then prefer whichever byte is zero more often). That heuristic is
/// deliberately not reproduced here -- guessing wrong would silently swap
/// every byte pair and produce plausible-looking mojibake under the real tag
/// name, which is worse than the honest default below. Bytes with no BOM
/// fall back to little-endian, which is what Windows/EXIF cameras write in
/// practice; every no-BOM `UNICODE` value in the sample corpus already
/// matches this default (e.g. OlympusSH-25MR.jpg, PanasonicDMC-TZ20.jpg,
/// PanasonicDMC-ZS10.jpg GPSAreaInformation).
///
/// Unlike the XP* tags (`UCS2`, no surrogate combination), this path uses
/// charset `UTF16`, which Charset.pm:80 documents as "UCS2 with surrogate
/// pairs added" and combines at Charset.pm:235 -- exactly what
/// [`String::from_utf16_lossy`] already does.
fn decode_unicode_gps_text(data: &[u8]) -> String {
    // Honour a leading BOM over the little-endian default, as Charset.pm does.
    let (data, big_endian) = match data {
        [0xFE, 0xFF, rest @ ..] => (rest, true),
        [0xFF, 0xFE, rest @ ..] => (rest, false),
        _ => (data, false),
    };

    // Convert pairs of bytes to UTF-16 code units.
    // Filter out incomplete pairs at the end (odd byte count).
    let u16_data: Vec<u16> = data
        .chunks(2)
        .filter_map(|chunk| {
            if chunk.len() == 2 {
                let unit = if big_endian {
                    u16::from_be_bytes([chunk[0], chunk[1]])
                } else {
                    u16::from_le_bytes([chunk[0], chunk[1]])
                };
                Some(unit)
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
        assert_eq!(decode_unicode_gps_text(&data), "Hi");
    }

    #[test]
    fn test_decode_utf16_le_odd_length() {
        // Odd number of bytes (last byte should be ignored)
        let data = [0x48, 0x00, 0x69, 0x00, 0xFF];
        assert_eq!(decode_unicode_gps_text(&data), "Hi");
    }

    #[test]
    fn test_decode_utf16_le_empty() {
        assert_eq!(decode_unicode_gps_text(&[]), "");
    }

    #[test]
    fn test_decode_utf16_le_only_null() {
        let data = [0x00, 0x00];
        assert_eq!(decode_unicode_gps_text(&data), "");
    }

    // ==================== Byte-Order-Mark Tests ====================
    //
    // Ground truth from the pinned 13.59 oracle (`.exiftool-version`):
    //
    //     $ perl exiftool -G1 -s -GPSAreaInformation \
    //         combined-samples/Olympus/OlympusTG-1.jpg
    //     [GPS]           GPSAreaInformation              : 巌根駅
    //
    // Raw tag bytes (exiftool -v3): `55 4e 49 43 4f 44 45 00` ("UNICODE\0")
    // followed by `ff fe cc 5d 39 68 c5 99` then NUL padding -- a
    // little-endian BOM (`\xff\xfe`) in front of the UTF-16LE text.

    #[test]
    fn olympus_tg1_gps_area_information_strips_le_bom() {
        // "UNICODE\0" + ff fe cc 5d 39 68 c5 99 + NUL padding, i.e. the
        // GPSAreaInformation bytes from Olympus/OlympusTG-1.jpg with the
        // 8-byte encoding id already stripped by decode_gps_processing_method.
        let mut data = vec![0xFF, 0xFE, 0xCC, 0x5D, 0x39, 0x68, 0xC5, 0x99];
        data.extend(std::iter::repeat_n(0u8, 256 - data.len()));

        assert_eq!(decode_unicode_gps_text(&data), "巌根駅");

        let mut full = b"UNICODE\0".to_vec();
        full.extend_from_slice(&data);
        assert_eq!(decode_gps_processing_method(&full), "巌根駅");
    }

    #[test]
    fn decode_unicode_gps_text_strips_be_bom_and_flips_order() {
        // `\xfe\xff` (big-endian BOM) both removes the mark and switches the
        // rest of the value to big-endian (Charset.pm:203).
        let data = [0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69, 0x00, 0x00];
        assert_eq!(decode_unicode_gps_text(&data), "Hi");
    }

    #[test]
    fn decode_unicode_gps_text_no_bom_defaults_to_little_endian() {
        // No BOM: every no-BOM UNICODE value in the sample corpus
        // (OlympusSH-25MR.jpg, PanasonicDMC-TZ20.jpg, PanasonicDMC-ZS10.jpg
        // GPSAreaInformation) is little-endian, so that is the fallback.
        // ExifTool's real behavior guesses the order per-value
        // (Charset.pm:213-228); that heuristic is intentionally not
        // reproduced -- see decode_unicode_gps_text's doc comment.
        let data = [0x48, 0x00, 0x69, 0x00, 0x00, 0x00];
        assert_eq!(decode_unicode_gps_text(&data), "Hi");
    }

    #[test]
    fn decode_unicode_gps_text_bom_only_honoured_at_start() {
        // A BOM sequence appearing mid-string is ordinary text, not a marker.
        let data = [0x48, 0x00, 0xFF, 0xFE, 0x69, 0x00, 0x00, 0x00];
        assert_eq!(decode_unicode_gps_text(&data), "H\u{FEFF}i");
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
