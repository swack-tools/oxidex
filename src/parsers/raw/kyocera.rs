//! Kyocera Contax N Digital RAW metadata parser.
//!
//! ExifTool routes a `.raw` file through `Image::ExifTool::KyoceraRaw::ProcessRAW`
//! (`KyoceraRaw.pm:112-134`) when its first 156 bytes carry the reversed
//! `"KYOCERA"` signature (`ARECOYK`) at byte offset 0x19: `substr($buff, 0x19,
//! 7) eq 'ARECOYK'`. Once validated it runs `ProcessBinaryData` over
//! `KyoceraRaw::Main` (`KyoceraRaw.pm:25-106`) with big-endian byte order,
//! which this module reproduces from the generated table
//! (`find_table("KyoceraRaw", "Main")`) rather than restating offsets here.
//!
//! ExifTool's own top-level `RAW` magic number is `(.{25}ARECOYK|II|MM)`
//! (`ExifTool.pm`'s `%magicNumber`): a `.raw` file is either this Kyocera
//! format or a TIFF-based one (Panasonic RAW). `detect_format` matches the
//! same `ARECOYK` signature and hands this parser `FileFormat::CameraRaw(
//! RawFormat::GenericRAW)`; the TIFF-magic alternative still falls to
//! `parse_tiff_based_raw` in `raw/metadata.rs`, unaffected by this parser.
//!
//! # Why the table alone is not enough
//!
//! Four of the eleven `KyoceraRaw::Main` fields carry `ValueConv => \&ReverseString`
//! (`KyoceraRaw.pm:22`, a Perl code reference the generator cannot inline):
//! `FirmwareVersion` (:29-33), `Model` (:34-38), `Make` (:39-43) and
//! `DateTimeOriginal` (:44-51). All four decode fine as raw fixed-length
//! strings -- the generated table's `Fmt::Str` decode already truncates at the
//! first embedded NUL the same way ExifTool's own `ReadValue` does
//! (`Image::ExifTool.pm:6311`, `s/\0.*//s if $format eq 'string'`) -- but the
//! byte-order reversal itself is hand-applied below, each behind a
//! [`RawAccess`] citation, per AGENTS.md's "never approximate a conversion"
//! rule (a field the generator declines to convert must not reach the tag map
//! unconverted). `DateTimeOriginal`'s `PrintConv => '$self->ConvertDateTime($val)'`
//! is likewise not run by the generated table (it never gets past the
//! `ValueConv` omission to try), but `ConvertDateTime` only rewrites its input
//! when a `-d` DateFormat option or a global time shift is active
//! (`Image::ExifTool.pm:6653`, `if ($fmt) { ... } return $date;`) -- with
//! neither set, which is OxiDex's default, it returns the reversed date
//! string unchanged, so no further conversion is needed here.
//!
//! Two more fields, `FNumber` (:84-90) and `MaxAperture` (:91-96), carry a
//! `ValueConv` (`2**($val/16)`) and a `PrintConv` (`sprintf("%.2g",$val)`)
//! the generator now compiles in full, so `DecodedField::emit` would render
//! ExifTool's two-significant-figure `"11"` / `"5.2"`. This parser withholds
//! that rendering on purpose and reports the full-precision `ValueConv`
//! number (`11.313708...`) instead: `Composite:Aperture`/`LightValue` read
//! `KyoceraRaw:FNumber` back out and recompute from it, and
//! `composite/compute.rs`'s `("Exif", "Aperture")` arm parses the tag's
//! *display* string rather than its ValueConv form -- ExifTool's composites
//! run on the ValueConv value (`Image::ExifTool.pm`'s `BuildCompositeTags`
//! fetches `$val` with the print conversion off), so the display `"11"`
//! would feed the composite `11` where ExifTool computes from `11.3137`, and
//! land a wrong `LightValue` under a real tag name -- exactly what
//! AGENTS.md's "never approximate a conversion" rule rules out. A
//! full-precision `FNumber` display is a citeable PrintConv gap; a wrong
//! `LightValue` is not. The lasting fix is for the composite engine to read
//! a tag's ValueConv form (`MetadataMap::value_form`) instead of its display
//! string; when it does, these two arms go away and `emit()` takes over.
//!
//! The remaining five fields (`ISO`, `ExposureTime`, `WB_RGGBLevels`,
//! `FocalLength`, `Lens`) carry ValueConvs and PrintConvs the generator does
//! model in full, and decode through the ordinary `DecodedField::emit` path
//! unmodified.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/KyoceraRaw.pm`

use crate::core::{MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{
    Acknowledged, DecodedField, DecodedValue, PerlCitation, RawAccess, apply_value_conv,
    decode_binary_table, find_table, to_tag_value,
};
use crate::io::ByteOrder;

/// `KyoceraRaw.pm:116`, `my $size = 156; # size of header`.
const HEADER_LEN: usize = 156;

/// `KyoceraRaw.pm:121`, `substr($buff, 0x19, 7) eq 'ARECOYK'` -- the ASCII
/// literal `"KYOCERA"` stored byte-reversed at offset 0x19 (25 decimal).
const SIGNATURE_OFFSET: usize = 0x19;
const SIGNATURE: &[u8] = b"ARECOYK";

/// Whether `data` opens with the Kyocera Contax N Digital RAW signature,
/// reproducing ExifTool's own acceptance test (`KyoceraRaw.pm:119-121`)
/// rather than merely the shared `RAW`-extension magic number.
#[must_use]
pub fn looks_like_kyocera_raw(data: &[u8]) -> bool {
    data.len() >= HEADER_LEN
        && data.get(SIGNATURE_OFFSET..SIGNATURE_OFFSET + SIGNATURE.len()) == Some(SIGNATURE)
}

const fn citation(tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "KyoceraRaw",
        table: "Main",
        tag,
        lines,
    }
}

const FIRMWARE_VERSION: PerlCitation = citation("FirmwareVersion", "KyoceraRaw.pm:22,29-33");
const MODEL: PerlCitation = citation("Model", "KyoceraRaw.pm:22,34-38");
const MAKE: PerlCitation = citation("Make", "KyoceraRaw.pm:22,39-43");
const DATE_TIME_ORIGINAL: PerlCitation = citation("DateTimeOriginal", "KyoceraRaw.pm:22,44-51");
const F_NUMBER: PerlCitation = citation("FNumber", "KyoceraRaw.pm:84-90");
const MAX_APERTURE: PerlCitation = citation("MaxAperture", "KyoceraRaw.pm:91-96");

/// `ReverseString`, `KyoceraRaw.pm:22`:
/// `pack('C*',reverse unpack('C*',shift))` -- a byte-order reversal, applied
/// here after the generated table's `Fmt::Str` decode has already truncated
/// the fixed-length field at its first embedded NUL (matching ExifTool's own
/// `ReadValue` truncation, which runs before any `ValueConv`).
fn reverse_string(value: &str) -> Option<String> {
    let reversed: Vec<u8> = value.bytes().rev().collect();
    String::from_utf8(reversed).ok()
}

/// The field's generated `ValueConv` result with its generated `PrintConv`
/// deliberately withheld -- `FNumber`/`MaxAperture`, whose `sprintf("%.2g",
/// $val)` display the composite engine would parse back into a rounded
/// number (see the module doc). Nothing is omitted on these fields, so the
/// [`RawAccess`] here is not an escape past a refusal; `Acknowledged::
/// PRINT_CONV` names the step this call site takes responsibility for, and
/// the citation names the Perl whose rendering it declines.
fn value_conv_only(decoded: &DecodedField, cite: &'static PerlCitation) -> Option<TagValue> {
    let access = RawAccess::new(decoded, Acknowledged::PRINT_CONV, cite)?;
    let converted = apply_value_conv(decoded.field.value_conv, access.raw())?;
    Some(to_tag_value(&converted))
}

/// Extract Kyocera Contax N Digital RAW metadata using the generated
/// `KyoceraRaw::Main` binary layout, hand-porting the four `ReverseString`
/// fields the generator declines to convert.
pub fn parse_kyocera_raw_metadata(data: &[u8]) -> Result<MetadataMap> {
    if !looks_like_kyocera_raw(data) {
        return Err(ExifToolError::parse_error(
            "missing Kyocera Contax N Digital RAW signature",
        ));
    }
    let header = &data[..HEADER_LEN];

    let table = find_table("KyoceraRaw", "Main")
        .ok_or_else(|| ExifToolError::parse_error("missing KyoceraRaw::Main table"))?;
    let decode = decode_binary_table(table, header, ByteOrder::Big);

    // `File:FileType`/`File:MIMEType`/`File:FileTypeExtension` are filled in
    // by `add_identity_tags` from ExifTool's own tables after this parser
    // returns (`core::operations`'s Step 5a); this parser only needs to
    // supply the `KyoceraRaw:*` tags.
    let mut metadata = MetadataMap::new();

    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("KyoceraRaw:{name}");
        match name {
            "FirmwareVersion" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::VALUE_CONV, &FIRMWARE_VERSION)
                    && let DecodedValue::String(raw) = access.raw()
                    && let Some(rendered) = reverse_string(raw)
                {
                    metadata.insert(key, TagValue::new_string(rendered));
                }
            }
            "Model" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::VALUE_CONV, &MODEL)
                    && let DecodedValue::String(raw) = access.raw()
                    && let Some(rendered) = reverse_string(raw)
                {
                    metadata.insert(key, TagValue::new_string(rendered));
                }
            }
            "Make" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::VALUE_CONV, &MAKE)
                    && let DecodedValue::String(raw) = access.raw()
                    && let Some(rendered) = reverse_string(raw)
                {
                    metadata.insert(key, TagValue::new_string(rendered));
                }
            }
            "DateTimeOriginal" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::VALUE_CONV, &DATE_TIME_ORIGINAL)
                    && let DecodedValue::String(raw) = access.raw()
                    && let Some(rendered) = reverse_string(raw)
                {
                    metadata.insert(key, TagValue::new_string(rendered));
                }
            }
            // `WB_RGGBLevels` (`Format => 'int32u[4]'`, no `List`, no
            // `PrintConv`) prints as a plain space-joined string in
            // ExifTool ("84 64 64 86"), not a comma list -- `TagValue::Array`
            // is List-shaped, and `join_list` in `cli::output_formatter`
            // joins every array with `", "`, the right convention for an
            // actual List tag (Keywords) but not for this fixed-count scalar
            // array. Composite `BlueBalance`/`RedBalance` also parse this
            // tag's *string* value with `split_whitespace()`
            // (`composite/compute.rs`'s `red_blue_balance`), so the comma
            // form silently dropped both composites too. Rendered as a
            // string here rather than changing the shared array formatter,
            // which backs every other List-shaped tag in the codebase.
            "WB_RGGBLevels" => {
                if let Some(TagValue::Array(values)) = decoded.emit() {
                    let joined = values
                        .iter()
                        .filter_map(TagValue::as_integer)
                        .map(|value| value.to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    metadata.insert(key, TagValue::new_string(joined));
                }
            }
            // Full-precision ValueConv, generated `%.2g` PrintConv withheld:
            // the composite engine parses this tag's display string (module
            // doc, "Why the table alone is not enough").
            "FNumber" => {
                if let Some(value) = value_conv_only(decoded, &F_NUMBER) {
                    metadata.insert(key, value);
                }
            }
            "MaxAperture" => {
                if let Some(value) = value_conv_only(decoded, &MAX_APERTURE) {
                    metadata.insert(key, value);
                }
            }
            _ => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn fixture() -> Vec<u8> {
        // Real ExifTool test-suite fixture, not hand-authored bytes: see
        // AGENTS.md's rule that regression fixtures must be real files.
        let candidates = [
            "/tmp/oxidex-exiftool-cache/combined-samples/KyoceraRaw.raw",
            "/tmp/oxidex-exiftool-cache/exiftool/t/images/KyoceraRaw.raw",
        ];
        for candidate in candidates {
            if let Ok(bytes) = fs::read(Path::new(candidate)) {
                return bytes;
            }
        }
        panic!("KyoceraRaw.raw fixture not found in the oxidex-exiftool-cache");
    }

    #[test]
    fn recognizes_the_signature() {
        let data = fixture();
        assert!(looks_like_kyocera_raw(&data));
    }

    #[test]
    fn rejects_short_or_unsigned_data() {
        assert!(!looks_like_kyocera_raw(b"too short"));
        assert!(!looks_like_kyocera_raw(&[0u8; HEADER_LEN]));
    }

    #[test]
    fn matches_exiftool_13_59_on_the_real_fixture() {
        let data = fixture();
        let metadata = parse_kyocera_raw_metadata(&data).expect("parses");

        // Cross-checked against `exiftool -a -G1 -s` (pinned 13.59) on the
        // same fixture.
        assert_eq!(
            metadata.get("KyoceraRaw:FirmwareVersion"),
            Some(&TagValue::new_string("Ver. 1.07"))
        );
        assert_eq!(
            metadata.get("KyoceraRaw:Model"),
            Some(&TagValue::new_string("N DIGITAL"))
        );
        assert_eq!(
            metadata.get("KyoceraRaw:Make"),
            Some(&TagValue::new_string("KYOCERA"))
        );
        assert_eq!(
            metadata.get("KyoceraRaw:DateTimeOriginal"),
            Some(&TagValue::new_string("2005:07:16 18:14:30"))
        );
        assert_eq!(
            metadata.get("KyoceraRaw:ISO"),
            Some(&TagValue::new_string("100"))
        );
        assert_eq!(
            metadata.get("KyoceraRaw:Lens"),
            Some(&TagValue::new_string("VS28-80/3.5"))
        );
        assert_eq!(
            metadata.get("KyoceraRaw:WB_RGGBLevels"),
            Some(&TagValue::new_string("84 64 64 86"))
        );
        // FNumber/MaxAperture report the generated table's own `ValueConv`
        // at full precision -- the fixture's raw bytes give `2**(56/16) =
        // 11.3137...` and `2**(38/16) = 5.1874...` -- with the generated
        // `sprintf("%.2g",$val)` PrintConv ("11"/"5.2") deliberately
        // withheld: the composite engine parses this tag's display string,
        // so the rounded display would feed a less precise value into
        // `Composite:Aperture`/`LightValue` (see the module doc). Pinned as
        // numbers, not strings, so a future `emit()` rendering trips here.
        let f_number = metadata
            .get("KyoceraRaw:FNumber")
            .and_then(TagValue::as_float)
            .expect("FNumber present");
        assert!(
            (f_number - 11.313_708_498_984_76).abs() < 1e-6,
            "{f_number}"
        );
        let max_aperture = metadata
            .get("KyoceraRaw:MaxAperture")
            .and_then(TagValue::as_float)
            .expect("MaxAperture present");
        assert!(
            (max_aperture - 5.187_358_218_604_04).abs() < 1e-6,
            "{max_aperture}"
        );
    }
}
