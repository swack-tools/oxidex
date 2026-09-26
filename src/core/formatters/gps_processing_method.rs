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
//! | `UNICODE\0`       | UTF-16      | Unicode; a leading BOM selects byte order, else the EXIF block's order, checked as ExifTool checks it|
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

use crate::exiftool_tables::session::{ByteOrder, MemberVal, Session};

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
///   otherwise starting little-endian ([`decode_gps_text`] takes the block's
///   order; see [`decode_unicode_gps_text`])
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
    decode_gps_text(data, ByteOrder::LittleEndian)
}

/// [`decode_gps_processing_method`] for a value read from an EXIF block of
/// byte order `order` (`GetByteOrder()` while ExifTool processes the GPS
/// IFD). That order is where a `UNICODE` value with no byte-order mark
/// starts; see [`decode_unicode_gps_text`]. Readers that know the block's
/// order must use this: a big-endian block's UTF-16 read as little-endian
/// comes back byte-swapped (`café` as `挀愀昀`).
pub fn decode_gps_text(data: &[u8], order: ByteOrder) -> String {
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
            decode_unicode_gps_text(text_data, order)
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
/// (Exif.pm:5586) calls `$et->Decode($str, 'UTF16', 'Unknown')`, and this
/// runs that call through the ported `Charset::Decompose`
/// ([`crate::exiftool_tables::charset::decompose`]) with `order` as
/// `GetByteOrder()`:
///
/// - a leading byte-order mark selects the order and is removed
///   (`Charset.pm:203`);
/// - with no BOM, decoding starts in the EXIF block's byte order, and the
///   `'Unknown'` check then swaps it when the other order fits the units
///   better (`Charset.pm:212-232`: the byte with more distinct values is the
///   low byte, else the byte that is zero more often is the high byte). That
///   is how ExifTool reads MicrosoftPhoto's little-endian text in a
///   big-endian block, and a big-endian block's own big-endian text;
/// - `UTF16` combines surrogate pairs (`Charset.pm:235`); a lone surrogate
///   has no Rust `char` and becomes U+FFFD.
///
/// Before, every no-BOM value was read little-endian whatever the block's
/// order, so the `UNICODE` text ExifTool (and now oxidex) writes into a
/// big-endian block came back byte-swapped.
fn decode_unicode_gps_text(data: &[u8], order: ByteOrder) -> String {
    let mut session = Session::new();
    session.byte_order = Some(order);
    let Ok(units) = crate::exiftool_tables::charset::decompose(
        &mut session,
        data,
        "UTF16",
        &MemberVal::Str("Unknown".into()),
    ) else {
        return String::new();
    };
    let text: String = units
        .iter()
        .map(|&unit| {
            u32::try_from(unit)
                .ok()
                .and_then(char::from_u32)
                .unwrap_or('\u{FFFD}')
        })
        .collect();
    text.trim_end_matches('\0').trim().to_string()
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
        assert_eq!(
            decode_unicode_gps_text(&data, ByteOrder::LittleEndian),
            "Hi"
        );
    }

    #[test]
    fn test_decode_utf16_le_odd_length() {
        // Odd number of bytes (last byte should be ignored)
        let data = [0x48, 0x00, 0x69, 0x00, 0xFF];
        assert_eq!(
            decode_unicode_gps_text(&data, ByteOrder::LittleEndian),
            "Hi"
        );
    }

    #[test]
    fn test_decode_utf16_le_empty() {
        assert_eq!(decode_unicode_gps_text(&[], ByteOrder::LittleEndian), "");
    }

    #[test]
    fn test_decode_utf16_le_only_null() {
        let data = [0x00, 0x00];
        assert_eq!(decode_unicode_gps_text(&data, ByteOrder::LittleEndian), "");
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

        assert_eq!(
            decode_unicode_gps_text(&data, ByteOrder::LittleEndian),
            "巌根駅"
        );

        let mut full = b"UNICODE\0".to_vec();
        full.extend_from_slice(&data);
        assert_eq!(decode_gps_processing_method(&full), "巌根駅");
    }

    #[test]
    fn decode_unicode_gps_text_strips_be_bom_and_flips_order() {
        // `\xfe\xff` (big-endian BOM) both removes the mark and switches the
        // rest of the value to big-endian (Charset.pm:203).
        let data = [0xFE, 0xFF, 0x00, 0x48, 0x00, 0x69, 0x00, 0x00];
        assert_eq!(
            decode_unicode_gps_text(&data, ByteOrder::LittleEndian),
            "Hi"
        );
    }

    #[test]
    fn decode_unicode_gps_text_no_bom_starts_in_the_block_order() {
        // No BOM: `Decode($str,'UTF16','Unknown')` starts in GetByteOrder(),
        // the EXIF block's order. `café` / `中文` as pinned ExifTool 13.59
        // writes them into an II and an MM block (`EncodeExifText`).
        for (order, bytes, text) in [
            (
                ByteOrder::LittleEndian,
                &[0x63, 0x00, 0x61, 0x00, 0x66, 0x00, 0xe9, 0x00][..],
                "café",
            ),
            (
                ByteOrder::BigEndian,
                &[0x00, 0x63, 0x00, 0x61, 0x00, 0x66, 0x00, 0xe9][..],
                "café",
            ),
            (
                ByteOrder::LittleEndian,
                &[0x2d, 0x4e, 0x87, 0x65][..],
                "中文",
            ),
            (ByteOrder::BigEndian, &[0x4e, 0x2d, 0x65, 0x87][..], "中文"),
            (
                ByteOrder::BigEndian,
                &[0x00, 0x41, 0xd8, 0x3d, 0xde, 0x00, 0x00, 0x42][..],
                "A😀B",
            ),
        ] {
            assert_eq!(
                decode_unicode_gps_text(bytes, order),
                text,
                "{order:?} {bytes:02x?}"
            );
            let mut full = b"UNICODE\0".to_vec();
            full.extend_from_slice(bytes);
            assert_eq!(decode_gps_text(&full, order), text, "{order:?}");
        }
    }

    #[test]
    fn decode_unicode_gps_text_swaps_when_the_other_order_fits() {
        // Charset.pm:212-232 ('Unknown'): the byte with more distinct values
        // is the low byte. Little-endian `Hi` in a big-endian block reads
        // `Hi` (MicrosoftPhoto writes little-endian even in MM EXIF), and
        // big-endian `Hi` in a little-endian block reads `Hi` too.
        let le = [0x48, 0x00, 0x69, 0x00, 0x00, 0x00];
        let be = [0x00, 0x48, 0x00, 0x69, 0x00, 0x00];
        assert_eq!(decode_unicode_gps_text(&le, ByteOrder::BigEndian), "Hi");
        assert_eq!(decode_unicode_gps_text(&be, ByteOrder::LittleEndian), "Hi");
        assert_eq!(decode_unicode_gps_text(&le, ByteOrder::LittleEndian), "Hi");
        assert_eq!(decode_unicode_gps_text(&be, ByteOrder::BigEndian), "Hi");
    }

    #[test]
    fn decode_unicode_gps_text_bom_only_honoured_at_start() {
        // A BOM sequence appearing mid-string is ordinary text, not a marker.
        let data = [0x48, 0x00, 0xFF, 0xFE, 0x69, 0x00, 0x00, 0x00];
        assert_eq!(
            decode_unicode_gps_text(&data, ByteOrder::LittleEndian),
            "H\u{FEFF}i"
        );
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
