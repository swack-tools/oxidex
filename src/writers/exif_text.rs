//! The EXIF text tags whose write conversion is `EncodeExifText`, as
//! ExifTool 13.59 writes them.
//!
//! Three tags carry it, each `Writable => 'undef'` with
//! `RawConvInv => 'Image::ExifTool::Exif::EncodeExifText($self,$val)'`:
//!
//! * ExifIFD `0x9286` UserComment (Exif.pm 13.59:2497-2506, also
//!   `Format => 'undef'`);
//! * GPS `0x001b` GPSProcessingMethod and `0x001c` GPSAreaInformation
//!   (GPS.pm 13.59:294-307).
//!
//! (`ConvertExifText`, the read side, has other callers -- Kodak and RIFF
//! tables -- but no other `EncodeExifText` user exists in the pinned tree,
//! and no writer here reaches those tables.)
//!
//! `EncodeExifText` (WriteExif.pl 13.59:131-141):
//!
//! ```perl
//! if ($val =~ /[\x80-\xff]/) {
//!     my $order = $et->GetNewValue('ExifUnicodeByteOrder');
//!     return "UNICODE\0" . $et->Encode($val,'UTF16',$order);
//! } else {
//!     return "ASCII\0\0\0$val";
//! }
//! ```
//!
//! `$val` is the caller's text in the default `UTF8` charset, so the test is
//! "any non-ASCII character". `ExifUnicodeByteOrder` is unset unless a caller
//! writes it, and `Charset::Recompose` then packs with `GetByteOrder()` --
//! the byte order of the EXIF block being written (Charset.pm 13.59:386-389),
//! which is why this conversion runs at raw-conversion time ("MUST be called
//! Raw conversion time so the EXIF byte order is known!"). Code points in
//! U+10000..U+10FFFE become surrogate pairs; U+10FFFF fails that loop's
//! `< 0x10ffff` bound and is packed by `pack('v*'/'n*')` as its low 16 bits,
//! `ff ff` (Charset.pm 13.59:374-384). The IFD format is always `undef`: the
//! tag's `Writable`/`Format`, whatever format an existing entry had
//! (WriteExif.pl 13.59:1224-1241).
//!
//! Provenance follows the map value's variant, as in `writers::xp_strings`:
//! a [`TagValue::String`] (or an integer) is a value the caller supplied and
//! is encoded; a [`TagValue::Binary`] is an entry's stored bytes (the reader's
//! stored form, header included) and is written back unchanged.

use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// The TIFF `undef` field type every one of these tags is written as.
const UNDEF: u16 = 7;

/// Whether a map key names one of the three `EncodeExifText` tags: the
/// tag-metadata classification
/// ([`crate::tag_db::tag_registry::is_encode_exif_text_tag`]).
pub(crate) fn is_exif_text_key(key: &str) -> bool {
    crate::tag_db::tag_registry::is_encode_exif_text_tag(key)
}

/// `EncodeExifText($val)` for `text`, packing UTF-16 in `order`.
pub(crate) fn encode_exif_text(text: &str, order: ByteOrder) -> Vec<u8> {
    if text.is_ascii() {
        let mut out = b"ASCII\0\0\0".to_vec();
        out.extend_from_slice(text.as_bytes());
        return out;
    }
    let mut out = b"UNICODE\0".to_vec();
    for ch in text.chars() {
        let cp = u32::from(ch);
        let units: Vec<u16> = if (0x10000..0x10FFFF).contains(&cp) {
            let t = cp - 0x10000;
            vec![
                0xD800 + ((t >> 10) & 0x3FF) as u16,
                0xDC00 + (t & 0x3FF) as u16,
            ]
        } else {
            // BMP code points; U+10FFFF as its low 16 bits (`pack` of an
            // integer wider than the template keeps the low bits).
            vec![(cp & 0xFFFF) as u16]
        };
        for unit in units {
            out.extend_from_slice(&match order {
                ByteOrder::LittleEndian => unit.to_le_bytes(),
                ByteOrder::BigEndian => unit.to_be_bytes(),
            });
        }
    }
    out
}

/// The `(field type, count, bytes)` ExifTool writes for `value` in an EXIF
/// block of byte order `order`. The bytes are final: `undef` is endian-
/// neutral, so the writers' native-to-file re-encoding leaves them alone.
pub(crate) fn exif_text_field(value: &TagValue, order: ByteOrder) -> Result<(u16, u32, Vec<u8>)> {
    let bytes = match value {
        TagValue::String(text) => encode_exif_text(text, order),
        TagValue::Integer(number) => encode_exif_text(&number.to_string(), order),
        // Stored bytes (a copy, or an unchanged entry): header included.
        TagValue::Binary(stored) => stored.clone(),
        other => {
            return Err(ExifToolError::parse_error(format!(
                "EXIF text (UserComment, GPSProcessingMethod, GPSAreaInformation) \
                 must be text, not {other:?}"
            )));
        }
    };
    let count = u32::try_from(bytes.len())
        .map_err(|_| ExifToolError::parse_error("EXIF text value is too long"))?;
    Ok((UNDEF, count, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Bytes the pinned oracle writes for `-ExifIFD:UserComment=V` (the GPS
    /// tags write the same), in an II and an MM block.
    #[test]
    fn encodes_like_the_oracle() {
        for (value, ii, mm) in [
            (
                "Hello world",
                "415343494900000048656c6c6f20776f726c64",
                "415343494900000048656c6c6f20776f726c64",
            ),
            ("", "4153434949000000", "4153434949000000"),
            (
                "café",
                "554e49434f444500630061006600e900",
                "554e49434f44450000630061006600e9",
            ),
            (
                "中文",
                "554e49434f4445002d4e8765",
                "554e49434f4445004e2d6587",
            ),
            (
                "A😀B",
                "554e49434f44450041003dd800de4200",
                "554e49434f4445000041d83dde000042",
            ),
            (
                "\u{10000}",
                "554e49434f44450000d800dc",
                "554e49434f444500d800dc00",
            ),
            (
                "\u{10fffe}",
                "554e49434f444500ffdbfedf",
                "554e49434f444500dbffdffe",
            ),
            ("\u{10ffff}", "554e49434f444500ffff", "554e49434f444500ffff"),
        ] {
            assert_eq!(
                hex(&encode_exif_text(value, ByteOrder::LittleEndian)),
                ii,
                "II {value:?}"
            );
            assert_eq!(
                hex(&encode_exif_text(value, ByteOrder::BigEndian)),
                mm,
                "MM {value:?}"
            );
        }
    }

    #[test]
    fn stored_bytes_are_written_back_unchanged() {
        let stored = b"UNICODE\0\x00\x41".to_vec();
        assert_eq!(
            exif_text_field(&TagValue::Binary(stored.clone()), ByteOrder::LittleEndian).unwrap(),
            (7, 10, stored)
        );
    }

    #[test]
    fn keys() {
        for key in [
            "ExifIFD:UserComment",
            "EXIF:UserComment",
            "UserComment",
            "GPS:GPSProcessingMethod",
            "EXIF:GPSAreaInformation",
            "GPSAreaInformation",
        ] {
            assert!(is_exif_text_key(key), "{key}");
        }
        for key in [
            "XMP-exif:UserComment",
            "IFD1:UserComment",
            "IFD0:UserComment",
            "ExifIFD:GPSProcessingMethod",
            "XMP-exif:GPSProcessingMethod",
            "ExifIFD:Comment",
        ] {
            assert!(!is_exif_text_key(key), "{key}");
        }
    }
}
