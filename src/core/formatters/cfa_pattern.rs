//! CFAPattern decoder -- the single implementation for EXIF tag 0xA302.
//!
//! # Binary format
//!
//! Two 16-bit repeat counts followed by one colour byte per position, in
//! row-major order. The counts are *usually* big-endian, but Sony, Nikon and
//! Kodak bodies write them in the file's own little-endian order, and ExifTool
//! accepts both -- see [`decode_cfa_pattern`] for how it tells them apart.
//!
//! # Colour values
//!
//! | Value | Colour  |
//! |-------|---------|
//! | 0     | Red     |
//! | 1     | Green   |
//! | 2     | Blue    |
//! | 3     | Cyan    |
//! | 4     | Magenta |
//! | 5     | Yellow  |
//! | 6     | White   |
//!
//! Anything above 6 prints as `Unknown`.
//!
//! # Common Bayer patterns
//!
//! - RGGB: `[Red,Green][Green,Blue]` -- most common (Canon, Sony)
//! - BGGR: `[Blue,Green][Green,Red]` -- Nikon, Fuji
//! - GRBG: `[Green,Red][Blue,Green]` -- some Kodak sensors
//! - GBRG: `[Green,Blue][Red,Green]`

/// Colour names for CFA pattern values, indexed by the stored byte.
const CFA_COLOR_NAMES: [&str; 7] = ["Red", "Green", "Blue", "Cyan", "Magenta", "Yellow", "White"];

/// Emitted when the value is shorter than the two repeat counts.
const TRUNCATED: &str = "<truncated data>";
/// Emitted when a repeat count is zero.
const ZERO_SIZE: &str = "<zero pattern size>";
/// Emitted when the repeat counts ask for more colour bytes than are present.
const INVALID_SIZE: &str = "<invalid pattern size>";

/// Decodes CFAPattern binary data the way ExifTool prints it.
///
/// # Byte order
///
/// The counts are read big-endian first. Only when that reading asks for more
/// colour bytes than the value actually carries are they re-read
/// little-endian, and only if *that* reading fits; otherwise the big-endian
/// reading stands and the value is reported as malformed. So a Nikon
/// `02 00 02 00 ...` (little-endian 2x2) decodes exactly like a Canon
/// `00 02 00 02 ...`, while a genuinely corrupt header is not silently
/// reinterpreted into a plausible-looking pattern.
///
/// # Count order
///
/// The **first** count is the number of bracketed groups and the **second** is
/// the number of colours inside each -- `00 05 00 03` is five groups of three,
/// not three groups of five. The two are only interchangeable for the square
/// 2x2 Bayer patterns that dominate real files, which is what lets a
/// transposed decoder pass a test suite built from them.
///
/// # Malformed values
///
/// ExifTool prints a sentinel rather than dropping the tag: `<truncated data>`
/// when fewer than four bytes are present, `<zero pattern size>` when a count
/// is zero, and `<invalid pattern size>` when the counts overrun the value.
/// Trailing bytes beyond `rows * cols` are ignored.
///
/// Every behaviour above was established by executing
/// `Image::ExifTool::Exif::DecodeCFAPattern` and `::PrintCFAPattern` from the
/// installed ExifTool 13.55 over a sweep of synthetic values; the unit tests
/// below carry that sweep's answers.
///
/// # Examples
///
/// ```
/// use oxidex::core::formatters::cfa_pattern::decode_cfa_pattern;
///
/// // 2x2 RGGB Bayer pattern with big-endian counts (Canon)
/// let rggb = [0, 2, 0, 2, 0, 1, 1, 2];
/// assert_eq!(decode_cfa_pattern(&rggb), "[Red,Green][Green,Blue]");
///
/// // The same pattern with little-endian counts (Nikon)
/// let rggb_le = [2, 0, 2, 0, 0, 1, 1, 2];
/// assert_eq!(decode_cfa_pattern(&rggb_le), "[Red,Green][Green,Blue]");
///
/// // Non-square patterns are grouped by the first count
/// let five_by_three = [0, 5, 0, 3, 0, 1, 2, 3, 4, 5, 6, 0, 1, 2, 3, 4, 5, 6, 0];
/// assert_eq!(
///     decode_cfa_pattern(&five_by_three),
///     "[Red,Green,Blue][Cyan,Magenta,Yellow][White,Red,Green][Blue,Cyan,Magenta][Yellow,White,Red]"
/// );
///
/// // Panasonic's ASCII spelling of the same 2x2 GRBG pattern
/// assert_eq!(decode_cfa_pattern(b"02021021"), "[Green,Red][Blue,Green]");
///
/// // Too short to hold the counts at all
/// assert_eq!(decode_cfa_pattern(&[0, 2]), "<truncated data>");
/// ```
pub fn decode_cfa_pattern(data: &[u8]) -> String {
    let decoded_ascii = decode_ascii_digits(data);
    let data = decoded_ascii.as_deref().unwrap_or(data);

    if data.len() < 4 {
        return TRUNCATED.to_string();
    }

    let values = &data[4..];
    let big = (
        u16::from_be_bytes([data[0], data[1]]) as usize,
        u16::from_be_bytes([data[2], data[3]]) as usize,
    );
    let little = (
        u16::from_le_bytes([data[0], data[1]]) as usize,
        u16::from_le_bytes([data[2], data[3]]) as usize,
    );

    let (rows, cols) = if big.0 * big.1 <= values.len() {
        big
    } else if little.0 * little.1 <= values.len() {
        little
    } else {
        big
    };

    if rows == 0 || cols == 0 {
        return ZERO_SIZE.to_string();
    }
    if rows * cols > values.len() {
        return INVALID_SIZE.to_string();
    }

    let mut result = String::with_capacity(rows * (cols * 8 + 2));
    for row in 0..rows {
        result.push('[');
        for col in 0..cols {
            if col > 0 {
                result.push(',');
            }
            let color = values[row * cols + col] as usize;
            result.push_str(CFA_COLOR_NAMES.get(color).copied().unwrap_or("Unknown"));
        }
        result.push(']');
    }
    result
}

/// Rewrites Panasonic's ASCII spelling of the value into the binary one.
///
/// The SV-AS3 and SV-AS30 store CFAPattern as the characters `"02021021"`
/// rather than the eight bytes those digits stand for, and ExifTool converts
/// them -- digit character to digit value -- before doing anything else. So
/// `"02021021"` becomes `00 02 00 02 01 00 02 01`: a 2x2 GRBG pattern.
///
/// The trigger is ExifTool's `/^[0-6]+$/`, which is why a `7`, a letter or a
/// trailing NUL leaves the value alone -- and why a real binary pattern is
/// never mistaken for this form, since a binary one starts with a `00` byte
/// that is not an ASCII digit.
///
/// Returns `None` when the value is not in this form, so the caller keeps the
/// original slice rather than a copy of it.
fn decode_ascii_digits(data: &[u8]) -> Option<Vec<u8>> {
    if data.is_empty() || !data.iter().all(|b| (b'0'..=b'6').contains(b)) {
        return None;
    }
    Some(data.iter().map(|b| b - b'0').collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every expected string in this module was produced by running
    // `Image::ExifTool::Exif::DecodeCFAPattern` followed by `::PrintCFAPattern`
    // from ExifTool 13.55 on the same bytes.

    // ==================== Standard Bayer patterns ====================

    #[test]
    fn test_bayer_patterns_big_endian_counts() {
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 0, 1, 1, 2]),
            "[Red,Green][Green,Blue]"
        );
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 2, 1, 1, 0]),
            "[Blue,Green][Green,Red]"
        );
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 1, 0, 2, 1]),
            "[Green,Red][Blue,Green]"
        );
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 1, 2, 0, 1]),
            "[Green,Blue][Red,Green]"
        );
    }

    /// Sony, Nikon and Kodak write the repeat counts in the file's own
    /// little-endian order. A decoder that only reads them big-endian sees
    /// 512x512, cannot fit it, and reports the tag as malformed -- which is
    /// what oxidex did on every Nikon 1 and Nikon D-series sample in the
    /// corpus. These four are the little-endian spellings of the four above.
    #[test]
    fn test_bayer_patterns_little_endian_counts() {
        assert_eq!(
            decode_cfa_pattern(&[2, 0, 2, 0, 0, 1, 1, 2]),
            "[Red,Green][Green,Blue]"
        );
        assert_eq!(
            decode_cfa_pattern(&[2, 0, 2, 0, 2, 1, 1, 0]),
            "[Blue,Green][Green,Red]"
        );
        assert_eq!(
            decode_cfa_pattern(&[2, 0, 2, 0, 1, 0, 2, 1]),
            "[Green,Red][Blue,Green]"
        );
        assert_eq!(
            decode_cfa_pattern(&[2, 0, 2, 0, 1, 2, 0, 1]),
            "[Green,Blue][Red,Green]"
        );
    }

    // ==================== Count order ====================

    /// The first count is the number of groups. A transposed decoder agrees on
    /// every square pattern, so only a non-square one can tell the two apart.
    #[test]
    fn test_first_count_is_the_group_count() {
        let values = [0u8, 1, 2, 3, 4, 5, 6, 0, 1, 2, 3, 4, 5, 6, 0];

        let three_groups_of_five: Vec<u8> = [0, 3, 0, 5].iter().chain(&values).copied().collect();
        assert_eq!(
            decode_cfa_pattern(&three_groups_of_five),
            "[Red,Green,Blue,Cyan,Magenta][Yellow,White,Red,Green,Blue][Cyan,Magenta,Yellow,White,Red]"
        );

        let five_groups_of_three: Vec<u8> = [0, 5, 0, 3].iter().chain(&values).copied().collect();
        assert_eq!(
            decode_cfa_pattern(&five_groups_of_three),
            "[Red,Green,Blue][Cyan,Magenta,Yellow][White,Red,Green][Blue,Cyan,Magenta][Yellow,White,Red]"
        );

        // ...and the little-endian spellings agree with their big-endian twins.
        let le_three_of_five: Vec<u8> = [3, 0, 5, 0].iter().chain(&values).copied().collect();
        assert_eq!(
            decode_cfa_pattern(&le_three_of_five),
            decode_cfa_pattern(&three_groups_of_five)
        );
        let le_five_of_three: Vec<u8> = [5, 0, 3, 0].iter().chain(&values).copied().collect();
        assert_eq!(
            decode_cfa_pattern(&le_five_of_three),
            decode_cfa_pattern(&five_groups_of_three)
        );
    }

    // ==================== Colours ====================

    #[test]
    fn test_extended_colors_and_unknown() {
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 4, 5, 3, 1]),
            "[Magenta,Yellow][Cyan,Green]"
        );
        // Above 6 there is no name.
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 7, 8, 9, 255]),
            "[Unknown,Unknown][Unknown,Unknown]"
        );
    }

    #[test]
    fn test_all_same_color() {
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 1, 1, 1, 1]),
            "[Green,Green][Green,Green]"
        );
    }

    // ==================== Dimensions ====================

    /// There is no 16x16 ceiling. `00 11 00 01` is seventeen groups of one.
    #[test]
    fn test_dimensions_above_sixteen() {
        let mut data = vec![0u8, 17, 0, 1];
        data.extend((0..17u8).map(|i| i % 7));
        assert_eq!(
            decode_cfa_pattern(&data),
            "[Red][Green][Blue][Cyan][Magenta][Yellow][White][Red][Green][Blue][Cyan][Magenta][Yellow][White][Red][Green][Blue]"
        );
    }

    // ==================== Malformed values ====================

    #[test]
    fn test_shorter_than_the_counts_is_truncated() {
        assert_eq!(decode_cfa_pattern(&[]), "<truncated data>");
        assert_eq!(decode_cfa_pattern(&[0, 2]), "<truncated data>");
        assert_eq!(decode_cfa_pattern(&[0, 2, 0]), "<truncated data>");
    }

    #[test]
    fn test_zero_count_is_zero_pattern_size() {
        assert_eq!(
            decode_cfa_pattern(&[0, 0, 0, 2, 0, 1]),
            "<zero pattern size>"
        );
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 0, 0, 1]),
            "<zero pattern size>"
        );
    }

    #[test]
    fn test_counts_overrunning_the_value_are_invalid() {
        // Header alone, no colour bytes.
        assert_eq!(decode_cfa_pattern(&[0, 2, 0, 2]), "<invalid pattern size>");
        // 2x2 declared, three bytes supplied.
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 0, 1, 1]),
            "<invalid pattern size>"
        );
        // Neither byte order fits, so the big-endian reading stands and the
        // value is reported malformed rather than reinterpreted.
        assert_eq!(
            decode_cfa_pattern(&[2, 0, 0, 2, 0, 1, 1, 2]),
            "<invalid pattern size>"
        );
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 2, 0, 0, 1, 1, 2]),
            "<invalid pattern size>"
        );
        // Four bare colour bytes are not a headerless 2x2 pattern: read as
        // counts they are 1 by 258, which does not fit.
        assert_eq!(decode_cfa_pattern(&[0, 1, 1, 2]), "<invalid pattern size>");
    }

    /// A big-endian reading that fits is never second-guessed, even when the
    /// little-endian reading would also fit.
    #[test]
    fn test_fitting_big_endian_reading_wins() {
        assert_eq!(decode_cfa_pattern(&[0, 1, 0, 2, 0, 1]), "[Red,Green]");
        // The mirror image: big-endian is 256 by 512, little-endian is 1 by 2.
        assert_eq!(decode_cfa_pattern(&[1, 0, 2, 0, 0, 1]), "[Red,Green]");
    }

    // ==================== Panasonic's ASCII form ====================

    /// The SV-AS3 and SV-AS30 write the pattern as digit characters. ExifTool
    /// converts them to digit values and then parses normally, which is how
    /// `"02021021"` -- 2, 2, then 1,0,2,1 -- comes out GRBG. Both bodies are
    /// in the sample corpus and both used to print `<invalid pattern size>`.
    #[test]
    fn test_panasonic_ascii_digits() {
        assert_eq!(decode_cfa_pattern(b"02021021"), "[Green,Red][Blue,Green]");
        assert_eq!(decode_cfa_pattern(b"020103"), "[Red][Cyan]");
        assert_eq!(decode_cfa_pattern(b"010103"), "[Red]");
        assert_eq!(decode_cfa_pattern(b"03010123"), "[Red][Green][Blue]");
        // The little-endian fallback still applies after the conversion.
        assert_eq!(decode_cfa_pattern(b"20201201"), "[Green,Blue][Red,Green]");
    }

    /// ExifTool's trigger is `/^[0-6]+$/`, so anything outside that class --
    /// a `7`, a `9`, a letter, a trailing NUL -- leaves the bytes alone.
    #[test]
    fn test_non_digit_bytes_are_not_ascii_form() {
        assert_eq!(decode_cfa_pattern(b"02027021"), "<invalid pattern size>");
        assert_eq!(decode_cfa_pattern(b"02029021"), "<invalid pattern size>");
        assert_eq!(decode_cfa_pattern(b"0202102a"), "<invalid pattern size>");
        assert_eq!(decode_cfa_pattern(b"02021021\0"), "<invalid pattern size>");
        // A binary pattern always starts with a byte that is not an ASCII
        // digit, so it can never be mistaken for the ASCII form.
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 0, 1, 1, 2]),
            "[Red,Green][Green,Blue]"
        );
    }

    #[test]
    fn test_extra_trailing_data_is_ignored() {
        assert_eq!(
            decode_cfa_pattern(&[0, 2, 0, 2, 0, 1, 1, 2, 255, 255, 255]),
            "[Red,Green][Green,Blue]"
        );
        assert_eq!(
            decode_cfa_pattern(&[2, 0, 2, 0, 0, 1, 1, 2, 9, 9]),
            "[Red,Green][Green,Blue]"
        );
    }
}
