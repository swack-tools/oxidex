//! Raw value extraction and PrintConv helpers shared by the Nikon MakerNote parsers.
//!
//! The Nikon MakerNote embeds a complete little/big-endian TIFF structure after
//! a 10-byte `"Nikon\0" + version` header. Every entry value offset inside that
//! IFD is relative to the start of the embedded TIFF header, NOT to the start of
//! the MakerNote block and NOT to the file. `value_bytes` is the single place
//! that resolves an entry to its bytes, so the rest of the Nikon code never has
//! to repeat that arithmetic (or guess at inline-vs-offset storage).
//!
//! The PrintConv helpers below reproduce ExifTool's `Image::ExifTool::Nikon`
//! conversions exactly -- see the doc comment on each for the Perl it mirrors.

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use once_cell::sync::Lazy;
use regex::Regex;
use std::borrow::Cow;

/// Size in bytes of one element of a TIFF field type.
///
/// Returns 0 for unknown/unsupported types so callers can reject them.
pub fn field_type_size(field_type: u16) -> usize {
    match field_type {
        1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,         // SHORT, SSHORT
        4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
        16 | 17 | 18 => 8,  // LONG8, SLONG8, IFD8 (BigTIFF)
        _ => 0,
    }
}

/// Total byte length of an IFD entry's value, or `None` for an unknown type.
pub fn value_len(entry: &IfdEntry) -> Option<usize> {
    let size = field_type_size(entry.field_type);
    if size == 0 {
        return None;
    }
    size.checked_mul(entry.value_count as usize)
}

/// Resolve an IFD entry to its raw value bytes.
///
/// Values of four bytes or fewer live inline in the entry's value-offset field.
/// `IfdEntry::value_offset` was decoded as a `u32` using `order`, so re-encoding
/// it with the same order reproduces the original on-disk byte sequence -- this
/// is what makes short ASCII tags and packed `undef[4]` tags readable.
///
/// Longer values live at `tiff_start + value_offset` within `data`, where
/// `data` is the whole MakerNote block and `tiff_start` is the offset of the
/// embedded TIFF header inside it (10 for every Nikon type 2/3 MakerNote).
pub fn value_bytes<'a>(
    entry: &IfdEntry,
    data: &'a [u8],
    tiff_start: usize,
    order: ByteOrder,
) -> Option<Cow<'a, [u8]>> {
    let len = value_len(entry)?;
    if len == 0 {
        return None;
    }
    if len <= 4 {
        let raw = match order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };
        Some(Cow::Owned(raw[..len].to_vec()))
    } else {
        let start = tiff_start.checked_add(entry.value_offset as usize)?;
        let end = start.checked_add(len)?;
        data.get(start..end).map(Cow::Borrowed)
    }
}

/// Read a `u16` at byte offset `at`.
pub fn read_u16(bytes: &[u8], at: usize, order: ByteOrder) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(at..at + 2)?.try_into().ok()?;
    Some(match order {
        ByteOrder::LittleEndian => u16::from_le_bytes(raw),
        ByteOrder::BigEndian => u16::from_be_bytes(raw),
    })
}

/// Read a `u32` at byte offset `at`.
pub fn read_u32(bytes: &[u8], at: usize, order: ByteOrder) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
    Some(match order {
        ByteOrder::LittleEndian => u32::from_le_bytes(raw),
        ByteOrder::BigEndian => u32::from_be_bytes(raw),
    })
}

/// Read the `index`-th rational (numerator/denominator pair) from `bytes`.
///
/// `signed` selects RATIONAL (`int32u`) versus SRATIONAL (`int32s`) components.
pub fn read_rational(
    bytes: &[u8],
    index: usize,
    order: ByteOrder,
    signed: bool,
) -> Option<(i64, i64)> {
    let at = index.checked_mul(8)?;
    let num = read_u32(bytes, at, order)?;
    let den = read_u32(bytes, at + 4, order)?;
    if signed {
        Some((num as i32 as i64, den as i32 as i64))
    } else {
        Some((num as i64, den as i64))
    }
}

/// Decode a rational to a float, treating a zero denominator as "no value".
pub fn rational_value(bytes: &[u8], index: usize, order: ByteOrder, signed: bool) -> Option<f64> {
    let (num, den) = read_rational(bytes, index, order, signed)?;
    if den == 0 {
        return None;
    }
    Some(num as f64 / den as f64)
}

/// Decode a NUL-terminated ASCII value the way ExifTool does.
///
/// ExifTool keeps everything up to (but not including) the first NUL and does
/// NOT trim trailing spaces -- `NikonScan`'s `FilmType` really is reported as
/// `"POSITIVE       "`. Tags in the Nikon main table get their trailing space
/// removed later by [`format_string`], which is the table's `PRINT_CONV`.
pub fn ascii_value(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Render a `rational64u`/`rational64s` the way ExifTool does.
///
/// `Image::ExifTool::GetRational64u` ends in `RoundFloat($num/$den, 10)`, and
/// `RoundFloat` is `sprintf("%.${sig}g", $val)` -- so a 64-bit rational carries
/// TEN significant digits, not Perl's usual fifteen. That is the difference
/// between `4557/2048` printing as ExifTool's `2.225097656` and as the exact
/// `2.22509765625`. The shared `exiftool_rational_number` is exactly that
/// `RoundFloat`, sign of zero included: a signed rational `0/-N` divides to
/// `-0.0` in Perl (pp_divide only takes the integer path when |left| >= |right|)
/// and `%.10g` then prints `-0`. A private `%.<precision>g` used to live here
/// and printed `0` for that case; on every finite input away from negative
/// zero it agreed byte-for-byte with `perl_g(value, 10)`, and `rational_value`
/// already returns `None` for a zero denominator, so the two never differed on
/// a reachable value. One `%g` in the crate is enough.
pub fn format_number(value: f64) -> String {
    crate::core::formatters::numeric_precision::exiftool_rational_number(value)
}

/// Nikon aperture encoding: `2**($val/24)` (ExifTool `%nikonApertureConversions`).
pub fn nikon_aperture(raw: u8) -> f64 {
    2.0_f64.powf(raw as f64 / 24.0)
}

/// Nikon focal length encoding: `5 * 2**($val/24)` (ExifTool `%nikonFocalConversions`).
pub fn nikon_focal_length(raw: u8) -> f64 {
    5.0 * 2.0_f64.powf(raw as f64 / 24.0)
}

/// ExifTool `Image::ExifTool::Exif::PrintFraction`.
///
/// Renders a signed value as the nearest simple fraction, which is how Nikon's
/// `ProgramShift`, `ExposureBracketValue` and the flash compensations print.
pub fn print_fraction(value: f64) -> String {
    crate::core::formatters::exif_print_conv::print_fraction(value)
}

/// ExifTool `Image::ExifTool::Exif::PrintLensInfo`: four rationals
/// (short focal, long focal, aperture at short, aperture at long) rendered as
/// e.g. `18-70mm f/3.5-4.5`.
pub fn print_lens_info(values: &[f64]) -> Option<String> {
    if values.len() != 4 {
        return None;
    }
    let parts: Vec<String> = values.iter().map(|v| format_number(*v)).collect();
    let mut out = parts[0].clone();
    if values[1] != 0.0 && parts[1] != parts[0] {
        out.push('-');
        out.push_str(&parts[1]);
    }
    out.push_str("mm f/");
    out.push_str(&parts[2]);
    if values[3] != 0.0 && parts[3] != parts[2] {
        out.push('-');
        out.push_str(&parts[3]);
    }
    Some(out)
}

/// ExifTool `Image::ExifTool::DecodeBits` restricted to a fixed label table.
///
/// Bits without a label report themselves as `[n]` exactly as ExifTool does, so
/// an unknown flag is never silently folded into a neighbouring name.
pub fn decode_bits(value: u32, bits: u32, labels: &[(u32, &str)]) -> String {
    let mut out: Vec<String> = Vec::new();
    for i in 0..bits {
        if value & (1 << i) == 0 {
            continue;
        }
        match labels.iter().find(|(bit, _)| *bit == i) {
            Some((_, name)) => out.push((*name).to_string()),
            None => out.push(format!("[{}]", i)),
        }
    }
    if out.is_empty() {
        "(none)".to_string()
    } else {
        out.join(", ")
    }
}

// `\b([AEIOUY])([A-Z]+)` -- a word starting with a vowel.
static VOWEL_WORD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([AEIOUY])([A-Z]+)").expect("static regex"));
// `\b([A-Z])([A-Z]*[AEIOUY][A-Z]*)` -- a word whose tail contains a vowel.
static CAPS_WORD: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([A-Z])([A-Z]*[AEIOUY][A-Z]*)").expect("static regex"));
// `  +.$` -- the terminator patch below.
static BAD_TERMINATOR: Lazy<Regex> = Lazy::new(|| Regex::new(r"  +.$").expect("static regex"));

/// ExifTool `Image::ExifTool::Nikon::FormatString`, the `PRINT_CONV` applied to
/// every string tag in the Nikon main table.
///
/// Nikon writes these values in all caps (`"AUTO        "`, `"NONE  "`); this
/// restores the mixed case ExifTool reports (`Auto`, `None`) while preserving
/// the acronyms `AF` and `RAW` and leaving vowel-free strings such as `CS`
/// untouched.
pub fn format_string(value: &str) -> String {
    // s/\s+$//
    let mut s = value.trim_end().to_string();
    // Only words containing a vowel have their case changed.
    if !s
        .chars()
        .any(|c| matches!(c, 'A' | 'E' | 'I' | 'O' | 'U' | 'Y'))
    {
        return s;
    }
    let mut replaced = false;
    let first = VOWEL_WORD.replace_all(&s, |caps: &regex::Captures| {
        replaced = true;
        format!("{}{}", &caps[1], caps[2].to_lowercase())
    });
    s = first.into_owned();
    if replaced {
        s = replace_word(&s, "Af", "AF");
        s = BAD_TERMINATOR.replace(&s, "").into_owned();
    }
    let mut replaced2 = false;
    let second = CAPS_WORD.replace_all(&s, |caps: &regex::Captures| {
        replaced2 = true;
        format!("{}{}", &caps[1], caps[2].to_lowercase())
    });
    s = second.into_owned();
    if replaced2 {
        s = replace_word(&s, "Raw", "RAW");
    }
    s
}

/// Replace `needle` with `replacement` only where it stands as a whole word,
/// mirroring Perl's `s/\bWORD\b/.../`.
fn replace_word(haystack: &str, needle: &str, replacement: &str) -> String {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(idx) = rest.find(needle) {
        let before_ok = rest[..idx].chars().next_back().is_none_or(|c| !is_word(c));
        let after = &rest[idx + needle.len()..];
        let after_ok = after.chars().next().is_none_or(|c| !is_word(c));
        out.push_str(&rest[..idx]);
        if before_ok && after_ok {
            out.push_str(replacement);
        } else {
            out.push_str(needle);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// ExifTool's binary-data placeholder, used for tags flagged `Binary` that are
/// only emitted as a byte count unless `-b` is given.
pub fn binary_placeholder(len: usize) -> String {
    format!("(Binary data {} bytes, use -b option to extract)", len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_string_matches_exiftool_for_nikon_d70_strings() {
        // Values taken byte-for-byte from a D70 NEF MakerNote.
        assert_eq!(format_string("RAW    "), "RAW");
        assert_eq!(format_string("AUTO        "), "Auto");
        assert_eq!(format_string("NONE  "), "None");
        assert_eq!(format_string("AF-S  "), "AF-S");
        assert_eq!(format_string("CS      "), "CS");
        assert_eq!(format_string("MODE2   "), "Mode2");
        assert_eq!(format_string("NATURAL    "), "Natural");
        assert_eq!(format_string("OFF "), "Off");
        assert_eq!(format_string("CUSTOM"), "Custom");
        assert_eq!(format_string("NORMAL"), "Normal");
        assert_eq!(format_string(" "), "");
        // No vowel: left exactly as written.
        assert_eq!(format_string("12345678"), "12345678");
    }

    #[test]
    fn value_bytes_reads_inline_values_in_ifd_byte_order() {
        // undef[4] stored inline: "0210"
        let entry = IfdEntry {
            tag_id: 0x0001,
            field_type: 7,
            value_count: 4,
            value_offset: u32::from_le_bytes(*b"0210"),
        };
        let bytes = value_bytes(&entry, &[], 10, ByteOrder::LittleEndian).unwrap();
        assert_eq!(&*bytes, b"0210");
    }

    #[test]
    fn value_bytes_reads_offset_values_relative_to_the_embedded_tiff() {
        let mut data = vec![0u8; 32];
        data[10 + 4..10 + 12].copy_from_slice(b"POSITIVE");
        let entry = IfdEntry {
            tag_id: 0x0002,
            field_type: 2,
            value_count: 8,
            value_offset: 4,
        };
        let bytes = value_bytes(&entry, &data, 10, ByteOrder::LittleEndian).unwrap();
        assert_eq!(&*bytes, b"POSITIVE");
    }

    #[test]
    fn value_bytes_rejects_out_of_range_offsets() {
        let entry = IfdEntry {
            tag_id: 0x0002,
            field_type: 2,
            value_count: 64,
            value_offset: 4096,
        };
        assert!(value_bytes(&entry, &[0u8; 32], 10, ByteOrder::LittleEndian).is_none());
    }

    #[test]
    fn nikon_encodings_match_exiftool() {
        // D70 sample: raw 44 -> f/3.6, raw 45 -> 18.3mm, raw 92 -> 71.3mm.
        assert_eq!(format!("{:.1}", nikon_aperture(44)), "3.6");
        assert_eq!(format!("{:.1}", nikon_focal_length(45)), "18.3");
        assert_eq!(format!("{:.1}", nikon_focal_length(92)), "71.3");
        assert_eq!(format!("{:.1}", nikon_aperture(52)), "4.5");
    }

    #[test]
    fn print_lens_info_matches_exiftool() {
        assert_eq!(
            print_lens_info(&[18.0, 70.0, 3.5, 4.5]).unwrap(),
            "18-70mm f/3.5-4.5"
        );
        assert_eq!(
            print_lens_info(&[50.0, 50.0, 1.4, 1.4]).unwrap(),
            "50mm f/1.4"
        );
        assert_eq!(
            print_lens_info(&[24.0, 70.0, 2.8, 2.8]).unwrap(),
            "24-70mm f/2.8"
        );
        assert!(print_lens_info(&[18.0, 70.0, 3.5]).is_none());
    }

    #[test]
    fn print_fraction_matches_exiftool() {
        assert_eq!(print_fraction(0.0), "0");
        assert_eq!(print_fraction(1.0), "+1");
        assert_eq!(print_fraction(-1.0), "-1");
        assert_eq!(print_fraction(0.5), "+1/2");
        assert_eq!(print_fraction(1.0 / 3.0), "+1/3");
    }

    #[test]
    fn decode_bits_reports_unlabelled_bits_as_themselves() {
        let labels = [(0u32, "MF"), (1, "D"), (2, "G")];
        assert_eq!(decode_bits(6, 8, &labels), "D, G");
        assert_eq!(decode_bits(0, 8, &labels), "(none)");
        // Bit 5 has no label here: it must report itself, never a neighbour.
        assert_eq!(decode_bits(0x20, 8, &labels), "[5]");
    }

    #[test]
    fn ascii_value_keeps_trailing_spaces_but_stops_at_nul() {
        assert_eq!(ascii_value(b"POSITIVE       \x00"), "POSITIVE       ");
        assert_eq!(ascii_value(b"Normal\x00"), "Normal");
        assert_eq!(ascii_value(b"NoNul"), "NoNul");
    }

    #[test]
    fn format_number_rounds_to_ten_significant_digits_like_exiftool() {
        assert_eq!(format_number(18.0), "18");
        assert_eq!(format_number(3.5), "3.5");
        assert_eq!(format_number(7.8), "7.8");
        assert_eq!(format_number(102.4), "102.4");
        assert_eq!(format_number(1234.5), "1234.5");
        assert_eq!(format_number(0.001), "0.001");
        assert_eq!(format_number(-4.5), "-4.5");
        assert_eq!(format_number(0.0), "0");
        // A signed rational `0/-N` divides to -0.0 in Perl (pp_divide takes
        // the NV path when |numerator| < |denominator|) and `RoundFloat`'s
        // `%.10g` keeps the sign: pinned perl 5.38.2 `sprintf("%.10g", -0.0)`
        // is `-0`. The private `%g` this module used to carry printed `0`.
        assert_eq!(format_number(-0.0), "-0");
        // WB_RBLevels off a D5: 4557/2048 is exactly 2.22509765625, and
        // ExifTool reports 2.225097656 -- RoundFloat(..., 10).
        assert_eq!(format_number(4557.0 / 2048.0), "2.225097656");
        assert_eq!(format_number(3120.0 / 2048.0), "1.5234375");
        // NikonCoolpixP60's PreviewIFD XResolution.
        assert_eq!(format_number(0.00319652120842234), "0.003196521208");
        assert_eq!(format_number(1.0 / 3.0), "0.3333333333");
        // %g leaves fixed notation at exp -4 and switches at -5.
        assert_eq!(format_number(0.0001234), "0.0001234");
        assert_eq!(format_number(0.00001234), "1.234e-05");
        assert_eq!(format_number(1.0e11), "1e+11");
    }
}
