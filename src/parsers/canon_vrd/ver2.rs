//! `%Image::ExifTool::CanonVRD::Ver2` -- the DPP 2.0 picture-style block.
//!
//! CanonVRD.pm:485-974. Unlike [`super::ver1_table`], nothing here is
//! transcribed: the layout comes from `exiftool_tables::find_table("CanonVRD",
//! "Ver2")`, which the generator dumped out of ExifTool's own in-memory hash.
//! That table already carries what this record needs -- `FORMAT => 'int16s'`
//! (so a tag ID is an index, and the byte offset is twice it), `FIRST_ENTRY =>
//! 0`, and the `PrintConv` enums -- so re-deriving it by hand would only add a
//! second, weaker copy. See `docs/TRANSCRIPTION.md`.
//!
//! # Why this stops at index 0x54
//!
//! The generated schema describes byte layout and the conversions the
//! transcription pipeline could reproduce *exactly*; a `ValueConv` it could not
//! reproduce is dropped, leaving `PrintConv::None` and the raw value. That is
//! the right default for the generator but it is not safe to emit blindly,
//! because the entries after this record's DPP 2.0 section lean on `ValueConv`
//! heavily and the dropped conversion is invisible at the call site:
//!
//! * 0x66-0x68 `ChromaticAberration`, `DistortionCorrection`,
//!   `PeripheralIllumination` -- `$val / 0x400` then `sprintf("%.0f%%")`, so
//!   ExifTool prints "100%" where the raw value is 1024 (CanonVRD.pm:726-748).
//! * 0x75-0x85 the `*RawHighlight` / `*RawShadow` pairs -- `$val / 10`
//!   (CanonVRD.pm:786-880).
//! * 0x8b `AngleAdj` -- `$val / 100` (CanonVRD.pm:881-886).
//! * 0x69 `AberrationCorrectionDistance` and 0xde `DLOShootingDistance` -- a
//!   `RawConv` that suppresses 0x7fff entirely, then `1 - $val / 0x400`
//!   (CanonVRD.pm:752-759, 941-949).
//! * 0xe0 `DLOInfo` -- a `SubDirectory` gated on the `DLOOn` DataMember with a
//!   `Hook` that advances `$varSize`, which the generated schema records as a
//!   plain int16s field (CanonVRD.pm:957-962).
//! * 0x5e-0x60 the noise-reduction tags -- `Condition`al lists keyed on
//!   `VRDVersion`, which the generator dropped rather than pick a branch.
//!
//! Emitting any of those from the raw value would put a confident wrong number
//! under a real ExifTool tag name, which is the one failure mode AGENTS.md
//! rules out. They are left unread instead. Index 0x54 is the last entry before
//! that starts: CanonVRD.pm:606 ends the DPP 2.0 record at index 0x59, and its
//! remaining entries are `Unknown => 1` (0x45-0x4b) or the `var_int16u`
//! `CustomPictureStyleData` at 0x58, none of which ExifTool reports by default.

use crate::core::{MetadataMap, TagValue};
use crate::exiftool_tables::{PrintConv, decode_binary_table, find_table};
use crate::io::ByteOrder;

/// Last `%Ver2` index whose value the generated table describes losslessly.
///
/// `CustomOutputShadowPoint` (CanonVRD.pm:604). See the module comment for why
/// the later entries are not read.
const LAST_LOSSLESS_INDEX: i64 = 0x54;

/// Reads `%CanonVRD::Ver2` as `ProcessBinaryData` would.
///
/// The record is big-endian like the rest of the trailer
/// (`SetByteOrder('MM')`, CanonVRD.pm:2148). `decode_binary_table` already
/// refuses a field whose bytes fall outside `record`, which is how a short
/// section -- the 178-byte DPP 2.0 form this file carries, against a table that
/// runs to index 0xe9 -- drops its later tags exactly as ExifTool does.
pub(super) fn parse_ver2(record: &[u8], metadata: &mut MetadataMap) {
    let Some(table) = find_table("CanonVRD", "Ver2") else {
        return;
    };
    // Every entry at or below `LAST_LOSSLESS_INDEX` is `Omitted::NONE` in the
    // generated table (verified against `src/exiftool_tables`), so `emit`
    // never refuses here.
    for decoded in decode_binary_table(table, record, ByteOrder::Big).fields() {
        let field = decoded.field;
        if field.index > LAST_LOSSLESS_INDEX {
            continue;
        }
        // Every in-scope entry takes the record's int16s FORMAT; anything else
        // would mean the table moved under us.
        let Some(value) = decoded.emit() else {
            continue;
        };
        let value = match (field.print_conv, &value) {
            // No conversion: ExifTool prints the int16s as it stands.
            (PrintConv::None, TagValue::Integer(_)) => value,
            // The enum matched: `emit` already rendered its string.
            (PrintConv::IntEnum(_), TagValue::String(_)) => value,
            // `emit` fell back to the raw value because the enum lookup
            // missed -- ExifTool's fallback for a value the hash does not
            // list.
            (PrintConv::IntEnum(_), TagValue::Integer(raw)) => {
                TagValue::String(format!("Unknown ({raw})"))
            }
            // Unreachable below LAST_LOSSLESS_INDEX, and guessing at a
            // conversion this module has not accounted for is exactly what the
            // index bound exists to prevent.
            (PrintConv::StrEnum(_) | PrintConv::Expr(_), _) => continue,
            (PrintConv::None | PrintConv::IntEnum(_), _) => continue,
        };
        metadata.insert(format!("CanonVRD:{}", field.name), value);
    }
}

/// The `VRD2` section of combined-samples/CanonVRD.vrd, byte for byte: file
/// offset 0x29e, the 178 bytes ExifTool reports as `[BinaryData directory, 178
/// bytes, Big-endian]` under `VRD2 (SubDirectory)` in `exiftool -v3`.
///
/// [`super`] composes it into a whole edit record to check the section walk.
#[cfg(test)]
pub(super) const CANONVRD_VRD_VER2: [u8; 178] = [
    0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x0f, 0xff, 0x00, 0x00, 0x0f, 0xff, 0x00, 0x00, 0xff, 0xfc, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x0f, 0xff, 0x00, 0x00, 0x0f, 0xff, 0x00, 0x00, 0x00, 0x04, 0x00, 0x02,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x0f, 0xff, 0x00, 0x06, 0x0f, 0xff, 0x00, 0x00, 0x00, 0x04,
    0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x0f, 0xff, 0x00, 0x06, 0x0f, 0xff, 0x00, 0x00,
    0x00, 0x04, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x0f, 0xff, 0x00, 0x06, 0x0f, 0xff,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x0f, 0xff, 0x00, 0x06,
    0x0f, 0xff, 0x00, 0x00, 0xff, 0xff, 0x00, 0x01, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x0f, 0xff,
    0x00, 0x00, 0x0f, 0xff, 0x00, 0x00, 0x00, 0x04, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07,
    0x0f, 0xff, 0x00, 0x06, 0x0f, 0xff, 0x00, 0x00, 0x00, 0x04, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x07, 0x0f, 0xff, 0x00, 0x06, 0x0f, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool_tables::Fmt;

    /// Every assertion is `exiftool -a -G1 -s combined-samples/CanonVRD.vrd`
    /// (ExifTool 13.55), value for value. These 65 tags are the whole of what
    /// the DPP 2.0 section of that file reports.
    #[test]
    fn canonvrd_vrd_ver2_matches_exiftool() {
        let mut m = MetadataMap::new();
        parse_ver2(&CANONVRD_VRD_VER2, &mut m);

        // The two tags that name the record's subject.
        assert_eq!(m.get_string("CanonVRD:PictureStyle"), Some("Standard"));
        assert_eq!(m.get_string("CanonVRD:IsCustomPictureStyle"), Some("No"));

        // Five styles share a nine-tag shape and a "Raw" infix. Standard's
        // ColorTone is negative, which is the whole reason the record's FORMAT
        // of int16s matters: read unsigned it would print 65532.
        for (style, color_tone, saturation, sharpness, raw_shadow) in [
            ("Standard", -4, 0, 1, 0),
            ("Portrait", 4, 2, 7, 6),
            ("Landscape", 4, 2, 7, 6),
            ("Neutral", 4, 2, 7, 6),
            ("Faithful", 4, 2, 7, 6),
        ] {
            let get = |suffix: &str| m.get_integer(&format!("CanonVRD:{style}{suffix}"));
            assert_eq!(get("RawColorTone"), Some(color_tone), "{style}");
            assert_eq!(get("RawSaturation"), Some(saturation), "{style}");
            assert_eq!(get("RawContrast"), Some(0), "{style}");
            assert_eq!(
                m.get_string(&format!("CanonVRD:{style}RawLinear")),
                Some("No"),
                "{style}"
            );
            assert_eq!(get("RawSharpness"), Some(sharpness), "{style}");
            assert_eq!(get("RawHighlightPoint"), Some(4095), "{style}");
            assert_eq!(get("RawShadowPoint"), Some(raw_shadow), "{style}");
            assert_eq!(get("OutputHighlightPoint"), Some(4095), "{style}");
            assert_eq!(get("OutputShadowPoint"), Some(0), "{style}");
        }

        // Monochrome swaps ColorTone and Saturation for the two filter enums,
        // both of which are keyed by negative values.
        assert_eq!(
            m.get_string("CanonVRD:MonochromeFilterEffect"),
            Some("Yellow")
        );
        assert_eq!(
            m.get_string("CanonVRD:MonochromeToningEffect"),
            Some("Purple")
        );
        assert_eq!(m.get_integer("CanonVRD:MonochromeContrast"), Some(3));
        assert_eq!(m.get_string("CanonVRD:MonochromeLinear"), Some("No"));
        assert_eq!(m.get_integer("CanonVRD:MonochromeSharpness"), Some(4));
        assert_eq!(
            m.get_integer("CanonVRD:MonochromeRawHighlightPoint"),
            Some(4095)
        );
        assert_eq!(m.get_integer("CanonVRD:MonochromeRawShadowPoint"), Some(0));
        assert_eq!(
            m.get_integer("CanonVRD:MonochromeOutputHighlightPoint"),
            Some(4095)
        );
        assert_eq!(
            m.get_integer("CanonVRD:MonochromeOutputShadowPoint"),
            Some(0)
        );

        // Custom drops the "Raw" from its first three names.
        assert_eq!(m.get_integer("CanonVRD:CustomColorTone"), Some(4));
        assert_eq!(m.get_integer("CanonVRD:CustomSaturation"), Some(2));
        assert_eq!(m.get_integer("CanonVRD:CustomContrast"), Some(0));
        assert_eq!(m.get_string("CanonVRD:CustomLinear"), Some("No"));
        assert_eq!(m.get_integer("CanonVRD:CustomSharpness"), Some(7));
        assert_eq!(
            m.get_integer("CanonVRD:CustomRawHighlightPoint"),
            Some(4095)
        );
        assert_eq!(m.get_integer("CanonVRD:CustomRawShadowPoint"), Some(6));
        assert_eq!(
            m.get_integer("CanonVRD:CustomOutputHighlightPoint"),
            Some(4095)
        );
        assert_eq!(m.get_integer("CanonVRD:CustomOutputShadowPoint"), Some(0));

        // ExifTool reports exactly these 65 tags from this section: nothing
        // past index 0x54 fits in 178 bytes, and the entries it skips inside
        // the section are its own `Unknown => 1` ones.
        assert_eq!(m.len(), 65);
    }

    /// The bound this module draws is only sound if every entry below it is a
    /// bare int16s or an integer enum. An ExifTool release that added a
    /// `ValueConv` under the bound would otherwise start emitting raw values
    /// under real tag names without any test noticing.
    #[test]
    fn no_entry_below_the_bound_needs_a_conversion_we_drop() {
        let table = find_table("CanonVRD", "Ver2").expect("generated CanonVRD::Ver2");
        assert_eq!(table.default_format, Fmt::Int16s);
        assert_eq!(table.first_entry, 0);

        let mut in_scope = 0;
        for field in table.fields {
            if field.index > LAST_LOSSLESS_INDEX {
                continue;
            }
            in_scope += 1;
            assert!(field.sub.is_none(), "{} is a bit field", field.name);
            assert_eq!(field.count, 1, "{} is an array", field.name);
            assert!(
                field.format.is_none(),
                "{} overrides the record format",
                field.name
            );
            assert!(
                matches!(field.print_conv, PrintConv::None | PrintConv::IntEnum(_)),
                "{} carries a conversion this module does not apply",
                field.name
            );
        }
        assert_eq!(in_scope, 65, "the DPP 2.0 picture-style block is 65 tags");
    }

    /// A section shorter than the full DPP 2.0 record must lose its later tags
    /// rather than read past the end, which is what ExifTool's own bounds check
    /// does for the truncated sections real files carry.
    #[test]
    fn a_short_section_drops_its_later_tags() {
        let mut m = MetadataMap::new();
        parse_ver2(&CANONVRD_VRD_VER2[..0x20], &mut m);
        assert_eq!(m.get_string("CanonVRD:PictureStyle"), Some("Standard"));
        assert_eq!(m.get_integer("CanonVRD:StandardRawColorTone"), Some(-4));
        // StandardRawLinear sits at index 0x10, i.e. byte 0x20.
        assert!(m.get("CanonVRD:StandardRawLinear").is_none());

        let mut empty = MetadataMap::new();
        parse_ver2(&[], &mut empty);
        assert!(empty.is_empty());
    }

    /// A value absent from a PrintConv hash prints ExifTool's fallback rather
    /// than being dropped or silently rendered as the raw number.
    #[test]
    fn a_value_outside_a_printconv_hash_reports_unknown() {
        let mut record = CANONVRD_VRD_VER2;
        // PictureStyle is index 0x02, so byte 4. The hash runs 0 through 7.
        record[4..6].copy_from_slice(&42i16.to_be_bytes());
        let mut m = MetadataMap::new();
        parse_ver2(&record, &mut m);
        assert_eq!(m.get_string("CanonVRD:PictureStyle"), Some("Unknown (42)"));
    }
}
