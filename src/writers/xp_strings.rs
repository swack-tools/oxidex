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
//! Every writer that serializes one of these from the map must apply
//! `ValueConvInv`: the value's UCS-2 code units, little-endian whatever the
//! file's byte order (the `"II"` argument), then a NUL pair. Serializing the
//! text as an ASCII entry -- what every writer did before -- puts its UTF-8
//! bytes on disk, and ExifTool reads `Title` back as `楔汴e`.
//!
//! What the code units are depends on where the value came from, exactly as
//! in ExifTool, and the map value's variant carries that provenance:
//!
//! * [`TagValue::String`] (or an integer) is a value the caller supplied
//!   (`-IFD0:XPTitle=V`, `modify_tag`, any API set). ExifTool encodes its
//!   code points with `pack('v*')`, which keeps the low 16 bits of one above
//!   U+FFFF: `-XPTitle=A🎌` writes `41 00 8c f3 00 00`.
//! * [`TagValue::Binary`] is the entry's bytes as a file stored them -- the
//!   readers keep them as `TagOccurrence::stored`, which `copy_metadata`
//!   and the PNG `eXIf` rebuild serialize. ExifTool's UCS2 decode keeps
//!   every unit as its own code point, so its copy packs the stored units
//!   back unchanged: a surrogate pair `3c d8 8c df`, and even a lone
//!   surrogate, survive `-TagsFromFile`.
//!
//! Whether a value is the caller's is recorded, never inferred from its
//! value: an occurrence recorded after the read is an assignment
//! (`MetadataMap::assigned_after_read`). An assigned XP string is always a
//! direct write, even when its text is exactly what the file decodes to --
//! a stored pair, lone surrogate or BOM decodes to text that does not
//! encode back to the same bytes, and ExifTool re-encodes the assigned text
//! ([`is_explicit_xp_set`]); an entry nobody assigned keeps its stored
//! bytes. A path that holds the decoded text of a *read* value without its
//! bytes cannot tell which of the two ExifTool would write for a code point
//! above U+FFFF, and refuses that value ([`refuse_unknown_provenance`])
//! rather than pick one.
//!
//! Verified against the pinned oracle (13.59, `-ver` and the `OOXML.docx`
//! probe asserted): `-XPTitle=Title` writes `54 00 69 00 74 00 6c 00 65 00
//! 00 00` as `int8u` in an II and in an MM file alike, and `-TagsFromFile`
//! writes `ValueConvInv(ValueConv(raw))` -- never the source's raw bytes: a
//! BOM, text after an embedded NUL, an odd trailing byte and extra NULs are
//! all gone from the copy, and an `undef` or `int16u` source entry lands as
//! `int8u`. `tests/xp_string_write.rs` pins each case.
//!
//! One case is not exact. When an unrelated edit rebuilds a PNG's `eXIf`
//! chunk, ExifTool leaves an untouched XP entry byte for byte (type, BOM,
//! text after a NUL and all), while the rebuild -- which re-serializes every
//! tag from the map, always II -- writes the copy form above. The two agree
//! whenever the stored value is canonical (UCS-2LE text, one NUL pair,
//! `int8u`), surrogates included, and ExifTool reads the same text from
//! both in every case; the JPEG and TIFF writers carry untouched entries
//! verbatim and are exact.

use crate::core::metadata_map::MetadataMap;
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
/// * Text (a caller-supplied value) is encoded one unit per code point,
///   keeping each code point's low 16 bits -- Charset.pm 13.59:387-390
///   `pack('v*', @uni)` -- little-endian, then `00 00`. Oracle `-XPTitle=V`:
///   `A🎌` -> `41 00 8c f3 00 00`, `x😀y中𝄞z` -> `78 00 00 f6 79 00 2d 4e 1e d1
///   7a 00 00 00`, U+10000 -> `00 00 00 00`, U+10FFFF -> `ff ff 00 00`. The
///   empty text is the NUL pair alone (what a copy of an empty XPTitle
///   writes, Nikon/NikonCoolpixS9900.jpg). A Rust `str` cannot hold the one
///   input that differs further -- a lone surrogate, which ExifTool accepts
///   as CESU-8 -- so no case is left unmatched.
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
pub(crate) fn encode_xp_value(value: &TagValue) -> Result<Vec<u8>> {
    let units: Vec<u16> = match value {
        TagValue::String(text) => pack_v(text),
        TagValue::Integer(number) => pack_v(&number.to_string()),
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

/// Perl's `pack('v*', @uni)` over a string's code points: each one's low 16
/// bits (Charset.pm 13.59:387-390; `UCS2`, unlike `UTF16`, adds no
/// surrogate pairs).
fn pack_v(text: &str) -> Vec<u16> {
    text.chars()
        .map(|c| (u32::from(c) & 0xFFFF) as u16)
        .collect()
}

/// Whether `key` in `desired` is an XP string the caller assigned
/// ([`MetadataMap::assigned_after_read`]) -- a direct write, which ExifTool
/// re-encodes from the assigned text even when that text equals what the
/// file decodes to: a stored surrogate pair, lone surrogate or BOM reads as
/// text that does not encode back to the same bytes. The writers rewrite
/// such an entry instead of carrying it over by value equality.
pub(crate) fn is_explicit_xp_set(desired: &MetadataMap, key: &str) -> bool {
    is_xp_tag_key(key) && desired.assigned_after_read(key)
}

/// Whether `desired` holds any XP string the caller assigned
/// ([`is_explicit_xp_set`]).
pub(crate) fn has_explicit_xp_set(desired: &MetadataMap) -> bool {
    desired
        .iter()
        .any(|(key, _)| is_explicit_xp_set(desired, key))
}

/// Refuses the decoded text of a *stored* XP value that arrived without the
/// bytes it was decoded from, when it holds a code point above U+FFFF: its
/// bytes are then either the stored surrogate pair (what ExifTool's copy
/// writes) or the low 16 bits of the code point (what its direct write
/// does), and nothing here says which. Every other text encodes the same
/// either way. `key` names the tag in the error.
pub(crate) fn refuse_unknown_provenance(key: &str, value: &TagValue) -> Result<()> {
    match value {
        TagValue::String(text) if text.chars().any(|c| u32::from(c) > 0xFFFF) => {
            Err(ExifToolError::unsupported_format(format!(
                "Cannot write {key}: its value was read from a file whose stored bytes \
                 were not kept, and it holds a code point above U+FFFF, which ExifTool \
                 writes differently for a copied value (the stored surrogate pair) and a \
                 typed one (the low 16 bits)"
            )))
        }
        _ => Ok(()),
    }
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

    /// A typed code point above U+FFFF keeps its low 16 bits, as the
    /// oracle's `-XPTitle=V` writes it; the same text copied from stored
    /// bytes keeps the stored surrogate pair.
    #[test]
    fn typed_code_points_above_the_bmp_keep_their_low_16_bits() {
        for (typed, oracle) in [
            ("A🎌", "41008cf30000"),
            ("x😀y中𝄞z", "780000f679002d4e1ed17a000000"),
            ("\u{10000}", "00000000"),
            ("\u{10ffff}", "ffff0000"),
        ] {
            assert_eq!(
                encode_xp_value(&TagValue::new_string(typed)).unwrap(),
                hex(oracle),
                "{typed:?}"
            );
        }
        assert_eq!(
            encode_xp_value(&TagValue::Binary(hex("41003cd88cdf0000"))).unwrap(),
            hex("41003cd88cdf0000")
        );
    }

    #[test]
    fn only_stored_text_above_the_bmp_is_of_unknown_provenance() {
        assert!(refuse_unknown_provenance("IFD0:XPTitle", &TagValue::new_string("A🎌")).is_err());
        for fine in [
            TagValue::new_string("é中文 café"),
            TagValue::new_string("A\u{fffd}B"),
            TagValue::Binary(hex("41003cd88cdf0000")),
        ] {
            assert!(
                refuse_unknown_provenance("IFD0:XPTitle", &fine).is_ok(),
                "{fine:?}"
            );
        }
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
