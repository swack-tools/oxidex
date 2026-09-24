//! The Windows XP strings (0x9c9b-0x9c9f XPTitle, XPComment, XPAuthor,
//! XPKeywords, XPSubject) as ExifTool writes them.
//!
//! Exif.pm 13.59 (`0x9c9b => {` at :2629, the other four after it) declares
//! each one
//!
//! ```text
//! Format       => 'undef',
//! Writable     => 'int8u',
//! ValueConv    => '$self->Decode($val,"UCS2","II")',
//! ValueConvInv => '$self->Encode($val,"UCS2","II") . "\0\0"',
//! ```
//!
//! The map holds the decoded text, so every writer that serializes one of
//! these from the map must apply `ValueConvInv`: the text's UCS-2 code units,
//! little-endian whatever the file's byte order (the `"II"` argument), then a
//! NUL pair. Serializing the text as an ASCII entry -- what every writer did
//! before -- puts its UTF-8 bytes on disk, and ExifTool reads `Title` back as
//! `楔汴e`.
//!
//! Verified against the pinned oracle (13.59, `-ver` and the `OOXML.docx`
//! probe asserted): `-XPTitle=Title` writes `54 00 69 00 74 00 6c 00 65 00
//! 00 00` as `int8u` in an II and in an MM file alike, and `-TagsFromFile`
//! writes `ValueConvInv(ValueConv(raw))` -- never the source's raw bytes: a
//! BOM, text after an embedded NUL, an odd trailing byte and extra NULs are
//! all gone from the copy, and an `undef` or `int16u` source entry lands as
//! `int8u`. `tests/xp_string_write.rs` pins each case.

use crate::core::tag_conversion::xp_ucs2_units;
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};

/// The EXIF ids of the five tags (`Exif::Main`, so any IFD that uses that
/// table: IFD0, where `WriteGroup` puts them, and ExifIFD alike).
pub(crate) fn is_xp_tag_id(tag_id: u16) -> bool {
    (0x9c9b..=0x9c9f).contains(&tag_id)
}

/// Whether a map key (`IFD0:XPTitle`, `EXIF:XPTitle`, `XPTitle`) names one of
/// the five tags.
pub(crate) fn is_xp_tag_key(key: &str) -> bool {
    matches!(
        key.rsplit(':').next(),
        Some("XPTitle" | "XPComment" | "XPAuthor" | "XPKeywords" | "XPSubject")
    )
}

/// The IFD field type ExifTool writes: `int8u` (1), the tag's `Writable`,
/// except when it rewrites an existing entry already stored as `undef` (7),
/// its `Format`, which it keeps. WriteExif.pl 13.59:1225-1242 only swaps in
/// `Writable` as the IFD format when the old format differs from `Format`;
/// a new entry takes `Writable` (:1202-1207). Oracle: `-XPTitle=Hi` on an
/// `undef` entry stays type 7; on an `int16u`, `string` or `int8u` entry it
/// becomes type 1.
pub(crate) fn xp_field_type(existing: Option<u16>) -> u16 {
    match existing {
        Some(7) => 7,
        _ => 1,
    }
}

/// The bytes ExifTool stores for `value`: `Encode($val,"UCS2","II") . "\0\0"`.
///
/// * Text is encoded as its UTF-16 code units, little-endian, then `00 00`;
///   the empty text is the NUL pair alone (what a copy of an empty XPTitle
///   writes, Nikon/NikonCoolpixS9900.jpg).
/// * An integer is the decimal text it was typed as: the CLI keeps a value
///   as `Integer` only when `i64::to_string` reproduces it byte for byte
///   (`parse_string_to_tag_value`). Oracle: `-XPTitle=123` writes
///   `31 00 32 00 33 00 00 00`.
/// * Bytes are the value as a file stores it, so they take ExifTool's whole
///   round trip, `ValueConvInv(ValueConv(bytes))`: [`xp_ucs2_units`] (a
///   leading BOM honoured and removed, an odd byte dropped, the value ended
///   at the first zero unit), packed back little-endian. That is exactly
///   the oracle's `-TagsFromFile` output for every shape tried, lone
///   surrogates included.
/// * Anything else (a rational, a float, a date, a list, a structure) has no
///   text ExifTool would have been given, and is refused rather than
///   guessed at.
///
/// One input is deliberately not ExifTool's bytes. A code point above U+FFFF
/// is written as its surrogate pair. 13.59 packs every code point with
/// `pack('v*')` (Charset.pm:387-390), keeping its low 16 bits, so it writes
/// a *typed* `-XPTitle=A🎌` as `41 00 8c f3 00 00` (U+F38C) -- but it copies
/// a stored pair `3c d8 8c df` back unchanged, because its UCS2 decode keeps
/// the two halves as two code points. oxidex's reader folds that stored pair
/// into the one `char` a typed value holds
/// (`tag_conversion::decode_xp_ucs2_string`), so the two cases reach this
/// function identical; the pair is ExifTool's bytes for the copy, and
/// truncating would corrupt it.
pub(crate) fn encode_xp_value(value: &TagValue) -> Result<Vec<u8>> {
    let units: Vec<u16> = match value {
        TagValue::String(text) => text.encode_utf16().collect(),
        TagValue::Integer(number) => number.to_string().encode_utf16().collect(),
        TagValue::Binary(bytes) => xp_ucs2_units(bytes),
        other => {
            return Err(ExifToolError::parse_error(format!(
                "XP* tags hold UCS-2 text; cannot write {other:?}"
            )));
        }
    };
    let mut out: Vec<u8> = units.into_iter().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    Ok(out)
}

/// `(field type, count, bytes)` for an XP* entry: [`xp_field_type`] of the
/// entry being replaced, and [`encode_xp_value`]. The bytes are per-byte
/// (`int8u` or `undef`), so the file's byte order never touches them.
pub(crate) fn xp_field(value: &TagValue, existing: Option<u16>) -> Result<(u16, u32, Vec<u8>)> {
    let bytes = encode_xp_value(value)?;
    Ok((xp_field_type(existing), bytes.len() as u32, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Typed values, against the oracle's `-XPTitle=V` bytes (the empty
    /// text: what a copy of an empty XPTitle writes).
    #[test]
    fn text_encodes_as_ucs2le_with_a_nul_pair() {
        for (value, oracle) in [
            (TagValue::new_string("Title"), "5400690074006c0065000000"),
            (
                TagValue::new_string("é中文 café"),
                "e9002d4e87652000630061006600e9000000",
            ),
            (TagValue::Integer(123), "3100320033000000"),
            (TagValue::new_string(""), "0000"),
        ] {
            assert_eq!(encode_xp_value(&value).unwrap(), hex(oracle), "{value:?}");
        }
    }

    /// Stored bytes, against the oracle's `-TagsFromFile` output for each.
    #[test]
    fn stored_bytes_take_exiftools_decode_encode_round_trip() {
        for (raw, oracle) in [
            ("480065006c006c006f000000", "480065006c006c006f000000"),
            ("feff0042004f004d006200650000", "42004f004d00620065000000"),
            ("fffe42004f004d006c0065000000", "42004f004d006c0065000000"),
            (
                "4800650079000000680069006400640065006e000000",
                "4800650079000000",
            ),
            (
                "0000200020002000200020002000200020002000200020002000200020002000",
                "0000",
            ),
            ("0000", "0000"),
            ("", "0000"),
            ("41", "0000"),
            ("4800690058", "480069000000"),
            ("41003cd88cdf0000", "41003cd88cdf0000"),
            ("410000d842000000", "410000d842000000"),
            ("410000dc42000000", "410000dc42000000"),
            ("0057006f007200640000", "0057006f007200640000"),
            ("4173636969207465787400", "417363696920746578740000"),
        ] {
            assert_eq!(
                encode_xp_value(&TagValue::Binary(hex(raw))).unwrap(),
                hex(oracle),
                "{raw}"
            );
        }
    }

    #[test]
    fn a_code_point_above_the_bmp_is_its_surrogate_pair() {
        assert_eq!(
            encode_xp_value(&TagValue::new_string("A🎌")).unwrap(),
            hex("41003cd88cdf0000")
        );
    }

    #[test]
    fn values_with_no_text_are_refused() {
        assert!(encode_xp_value(&TagValue::Float(1.5)).is_err());
        assert!(encode_xp_value(&TagValue::new_rational(1, 2)).is_err());
        assert!(encode_xp_value(&TagValue::Array(vec![])).is_err());
    }

    #[test]
    fn field_type_keeps_undef_and_otherwise_writes_int8u() {
        assert_eq!(xp_field_type(None), 1);
        assert_eq!(xp_field_type(Some(7)), 7);
        for old in [1, 2, 3, 4] {
            assert_eq!(xp_field_type(Some(old)), 1, "{old}");
        }
    }

    #[test]
    fn keys_and_ids_name_the_five_tags() {
        for key in [
            "IFD0:XPTitle",
            "EXIF:XPComment",
            "XPAuthor",
            "IFD0:XPKeywords",
        ] {
            assert!(is_xp_tag_key(key), "{key}");
        }
        assert!(is_xp_tag_key("ExifIFD:XPSubject"));
        assert!(!is_xp_tag_key("IFD0:Artist"));
        assert!(!is_xp_tag_key("IFD0:XPTitleX"));
        assert!((0x9c9b..=0x9c9f).all(is_xp_tag_id));
        assert!(!is_xp_tag_id(0x9c9a) && !is_xp_tag_id(0x9ca0));
    }
}
