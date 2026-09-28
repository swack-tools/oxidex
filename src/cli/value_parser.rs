//! Parses a command-line `-TAG=VALUE` string into the tag's *declared* value type.
//!
//! # Why this module exists
//!
//! The CLI used to wrap every `-TAG=VALUE` as [`TagValue::String`]
//! (`main.rs`), or to guess a type from the value's own shape
//! (`batch_processor::parse_tag_value`). Both are wrong for the same reason:
//! the write path validates the value against the type the tag *declares* in
//! the registry, so a `String` handed to an `Integer` tag was rejected with
//! `Type mismatch: expected Integer but got String` and no Integer, Rational
//! or DateTime tag could be set from the command line on any format.
//!
//! The declared type is knowable — [`get_tag_descriptor`] carries it — so the
//! value is parsed *into* that type here, before it reaches the writer.
//!
//! # Where the accepted input forms come from
//!
//! Every form accepted below is taken from ExifTool 13.55, not invented. The
//! path a `-TAG=VALUE` string takes in ExifTool is
//! `SetNewValue` -> `ConvInv` (`Writer.pl:2963`) -> the table's `CHECK_PROC`,
//! which for EXIF/TIFF reaches `CheckValue` (`Writer.pl:6842-6907`); the
//! accepted *shapes* are the `IsInt` / `IsHex` / `IsFloat` / `IsRational`
//! predicates in `ExifTool.pm:5924-5933`, and the float-to-rational and
//! string-to-date conversions are `Rationalize` (`Writer.pl:5200-5228`) and
//! `InverseDateTime` (`Writer.pl:5012-5151`).
//!
//! Per declared type:
//!
//! | Declared type | Accepted input | ExifTool source |
//! |---|---|---|
//! | `Integer` | `123`, `+123` (`IsInt`); `0x7b`, `7b` (`IsHex`); `122.6` rounded half-away-from-zero (`IsFloat`) | `Writer.pl:6873-6883`, `ExifTool.pm:5931-5932` |
//! | `Rational` | `1/250` (`IsRational`); `0.004`, `5.6`, `+5.6`, `1e1`, `5,6` (`IsFloat`); `inf`; `undef` | `Writer.pl:6888-6900`, `Writer.pl:5203-5205` |
//! | `Float` | `5.6`, `+5.6`, `1e1`, `5,6` (`IsFloat`) — no fraction form | `Writer.pl:6888-6900` |
//! | `DateTime` | `2024:01:15 10:30:00`, `2024-01-15T10:30:00`, `20240115103000`, `2024:01:15 10:30`, trailing `Z` / `+05:00` / `.25`, `now` | `Writer.pl:5012-5151` |
//! | `String` | anything (`CheckValue`'s `string`/`undef` branch imposes no shape) | `Writer.pl:6847-6858` |
//! | `Binary` | the literal bytes of the argument (ExifTool's `undef` format) | `Writer.pl:6847-6858` |
//!
//! # On failure
//!
//! A value that cannot be parsed for the declared type is an error. It is
//! **never** silently downgraded to `TagValue::String`: that is precisely the
//! bug this module replaces, only with a friendlier face — the write would be
//! rejected later by the type check anyway, or worse, would succeed and store
//! the wrong bytes. ExifTool behaves the same way, refusing the tag and
//! leaving the file untouched ("Warning: Not an integer for ...",
//! "Nothing to do.").

use crate::core::FormatFamily;
use crate::core::ValueType;
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use crate::tag_db::tag_registry::{get_tag_descriptor, has_reliable_value_type};
use chrono::{NaiveDate, TimeZone, Timelike, Utc};
use std::borrow::Cow;

/// The largest numerator/denominator [`Rationalize`] may produce.
///
/// ExifTool's `Rationalize` takes the cap as an argument: `0xffffffff` for
/// `rational64u` and `0x7fffffff` for `rational64s`
/// (`Writer.pl:5262-5273`), defaulting to `0x7fffffff` (`Writer.pl:5211`).
/// [`TagValue::Rational`] stores `i32` components, so the unsigned variant's
/// wider cap is not representable here and the default is used for both.
const RATIONAL_MAX: i64 = 0x7fff_ffff;

/// Port of ExifTool's `InverseOffsetTime` (WriteExif.pl:60-67).
fn inverse_offset_time(value: &str) -> Option<String> {
    if value.eq_ignore_ascii_case("now") {
        return Some(chrono::Local::now().format("%:z").to_string());
    }
    if value.ends_with('Z') {
        return Some("+00:00".to_string());
    }

    let bytes = value.as_bytes();
    for start in 0..bytes.len() {
        let sign = bytes[start];
        if sign != b'+' && sign != b'-' {
            continue;
        }
        let hour_start = start + 1;
        if hour_start >= bytes.len() || !bytes[hour_start].is_ascii_digit() {
            continue;
        }
        let mut hour_end = hour_start + 1;
        if hour_end < bytes.len() && bytes[hour_end].is_ascii_digit() {
            hour_end += 1;
        }
        let minute_start = if bytes.get(hour_end) == Some(&b':') {
            hour_end + 1
        } else {
            hour_end
        };
        let minute_end = minute_start.checked_add(2)?;
        let minute = bytes.get(minute_start..minute_end)?;
        if !minute.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let hour = std::str::from_utf8(&bytes[hour_start..hour_end]).ok()?;
        let minute = std::str::from_utf8(minute).ok()?;
        return Some(format!("{}{:0>2}:{minute}", sign as char, hour));
    }
    None
}

/// [`parse_cli_tag_value`] for a value as the command line delivered it,
/// which need not be UTF-8. Text goes through [`parse_cli_tag_value`]; any
/// other bytes through `cli::non_utf8::tag_value`, which encodes them as
/// ExifTool does for an XP string and refuses them for every other tag.
///
/// This is the library's string-typed setter: `raw` is always matched
/// against a tag's PrintConv label first (see [`parse_cli_tag_value`]'s
/// doc). A caller that already has the tag's raw/machine value should use a
/// typed setter (`TagValue::Integer`, `set_tag_integer`) instead of routing
/// a stringified number through here -- those are unaffected by this
/// distinction and keep working exactly as before.
pub fn parse_cli_tag_value_os(tag_name: &str, raw: &std::ffi::OsStr) -> Result<TagValue> {
    parse_cli_tag_value_os_with_mode(tag_name, raw, false)
}

/// [`parse_cli_tag_value_os`], additionally selecting ExifTool's raw
/// (PrintConv-bypassing) mode: the CLI's own `#` suffix and `--no-print-conv`
/// (its spelling of ExifTool's `-n`) reach here as `raw_mode: true`; the
/// public [`parse_cli_tag_value_os`] always passes `false`. Not exported --
/// the public API's numeric-vs-label policy is decided by
/// [`parse_cli_tag_value`]'s doc comment, not by a parameter a library
/// caller could flip.
pub(crate) fn parse_cli_tag_value_os_with_mode(
    tag_name: &str,
    raw: &std::ffi::OsStr,
    raw_mode: bool,
) -> Result<TagValue> {
    match raw.to_str() {
        Some(text) => parse_cli_tag_value_with_mode(tag_name, text, raw_mode),
        None => crate::cli::non_utf8::tag_value(tag_name, crate::cli::non_utf8::os_bytes(raw)),
    }
}

/// Parses `raw` into the value type `tag_name` declares in the tag registry.
///
/// Tags with no registry entry — or whose registry type metadata is flagged
/// unreliable — have no declared type to honour, so their value is passed
/// through as a string; the write path validates those with intrinsic checks
/// only (`core::validation::validate_tag_value_intrinsics`).
///
/// # Numeric input on a tag with an enum PrintConv
///
/// `raw` is matched against the tag's PrintConv label first (exact, then
/// case-insensitive -- [`invert_enum_printconv`]), the same way ExifTool's
/// own `ConvInv` does by default. A number that is not itself a label is
/// **refused**, not silently accepted as the tag's raw code: pinned ExifTool
/// 13.59 refuses `-Orientation=6` the same way (`Can't convert
/// IFD0:Orientation (not in PrintConv)`) unless the caller asks for the raw
/// form (`-Orientation#=6` or `-n`). This function is the CLI's own
/// string-value parser and the library's string-typed setter at once, so
/// this policy applies to both: a library caller that wants the raw code
/// should use a typed setter (`TagValue::Integer`, `set_tag_integer`) rather
/// than this string path, exactly as the CLI's `#`/`-n` distinguishes "a
/// label" from "the raw value" for the same argument text.
pub fn parse_cli_tag_value(tag_name: &str, raw: &str) -> Result<TagValue> {
    parse_cli_tag_value_with_mode(tag_name, raw, false)
}

/// The registry key a bare (ungrouped) tag name is typed by when the
/// registry cannot resolve the bare name itself, or `None` for a name this
/// table does not list. A bare name it does not list is typed by the address
/// the write resolves it to instead (`cli::write_transaction::apply_sets`),
/// so `-ColorSpace#=1` is an integer, not a string.
pub(crate) fn declared_alias(tag_name: &str) -> Option<&'static str> {
    match tag_name {
        "GPSDestBearing" => Some("GPS:GPSDestBearing"),
        "DateTimeOriginal" => Some("EXIF:DateTimeOriginal"),
        "ModifyDate" => Some("EXIF:ModifyDate"),
        "CreateDate" => Some("ExifIFD:CreateDate"),
        "ExposureTime" => Some("EXIF:ExposureTime"),
        "BrightnessValue" => Some("EXIF:BrightnessValue"),
        "LightSource" => Some("EXIF:LightSource"),
        "DigitalZoomRatio" => Some("EXIF:DigitalZoomRatio"),
        "Sharpness" => Some("EXIF:Sharpness"),
        "Contrast" => Some("EXIF:Contrast"),
        "CustomRendered" => Some("EXIF:CustomRendered"),
        "GainControl" => Some("EXIF:GainControl"),
        "FileSource" => Some("EXIF:FileSource"),
        "ExposureProgram" => Some("EXIF:ExposureProgram"),
        "WhiteBalance" => Some("EXIF:WhiteBalance"),
        "SceneCaptureType" => Some("EXIF:SceneCaptureType"),
        "Saturation" => Some("EXIF:Saturation"),
        "FlashpixVersion" => Some("EXIF:FlashpixVersion"),
        "CompressedBitsPerPixel" => Some("EXIF:CompressedBitsPerPixel"),
        "SubjectDistance" => Some("EXIF:SubjectDistance"),
        "FocalLength" => Some("EXIF:FocalLength"),
        "FocalLengthIn35mmFormat" => Some("EXIF:FocalLengthIn35mmFormat"),
        "AmbientTemperature" => Some("EXIF:AmbientTemperature"),
        "RelatedSoundFile" => Some("EXIF:RelatedSoundFile"),
        "SubjectDistanceRange" => Some("EXIF:SubjectDistanceRange"),
        "ComponentsConfiguration" => Some("EXIF:ComponentsConfiguration"),
        "SecurityClassification" => Some("EXIF:SecurityClassification"),
        "MeteringMode" => Some("EXIF:MeteringMode"),
        "ShutterSpeedValue" => Some("ExifIFD:ShutterSpeedValue"),
        "ApertureValue" => Some("EXIF:ApertureValue"),
        "Flash" => Some("ExifIFD:Flash"),
        "MakerNoteSafety" => Some("EXIF:MakerNoteSafety"),
        "ProfileEmbedPolicy" => Some("EXIF:ProfileEmbedPolicy"),
        // Exif.pm 13.59 0x0112/0x0128/0xa402/0x0103/0x0213: plain int16u
        // tags with a flat enum `PrintConv`. Without a group prefix these
        // fell through to `_ => tag_name`, a bare name `get_tag_descriptor`
        // cannot resolve, so the declared type was silently lost and the
        // value stored as a `String` -- `-Orientation=6` then failed the
        // writer's own type check ("expected Integer but got String") even
        // though the value was a plain, valid integer.
        "Orientation" => Some("EXIF:Orientation"),
        "ResolutionUnit" => Some("EXIF:ResolutionUnit"),
        "ExposureMode" => Some("EXIF:ExposureMode"),
        "Compression" => Some("EXIF:Compression"),
        "YCbCrPositioning" => Some("EXIF:YCbCrPositioning"),
        // Same bare-name gap as above, for three more plain enum tags:
        // `GrayResponseUnit` (0x0122) is `TAG_REGISTRY`-only (no colon-free
        // lookup); `SceneType` (0xa301) is looked up by leaf name directly
        // (see the early-return block above) so this alias only matters for
        // its type when reached bare; `CalibrationIlluminant1/2/3`
        // (0xc65a/0xc65b/0xcd31) resolve only through `YAML_TAG_ENTRIES`,
        // which is keyed `"EXIF:<name>"` and never matched by a bare lookup.
        "GrayResponseUnit" => Some("EXIF:GrayResponseUnit"),
        "SceneType" => Some("EXIF:SceneType"),
        "CalibrationIlluminant1" => Some("EXIF:CalibrationIlluminant1"),
        "CalibrationIlluminant2" => Some("EXIF:CalibrationIlluminant2"),
        "CalibrationIlluminant3" => Some("EXIF:CalibrationIlluminant3"),
        _ => None,
    }
}

/// [`parse_cli_tag_value`], additionally selecting raw mode (see
/// [`parse_cli_tag_value_os_with_mode`]). When `raw_mode` is `true`, every
/// PrintConv label lookup in this function is skipped -- a tag with an enum
/// PrintConv is parsed purely by its declared type, exactly as a tag with no
/// PrintConv at all already is. Not exported for the same reason as
/// [`parse_cli_tag_value_os_with_mode`].
pub(crate) fn parse_cli_tag_value_with_mode(
    tag_name: &str,
    raw: &str,
    raw_mode: bool,
) -> Result<TagValue> {
    let (tag_name, raw_mode) = match tag_name.strip_suffix('#') {
        Some(stripped)
            if stripped
                .rsplit(':')
                .next()
                .and_then(unit_suffix_tag)
                .is_some() =>
        {
            (stripped, true)
        }
        _ => (tag_name, raw_mode),
    };
    let declared_tag_name = declared_alias(tag_name).unwrap_or(tag_name);
    // Codex review finding on PR #963 (comment 4112327521, P2): ExifTool tag
    // and group names are case-insensitive, but `get_tag_descriptor` (and
    // this file's own leaf matches below) key on exact case. Without this,
    // `-ExifIFD:focallength=50 mm` or `-exififd:FocalLength=50 mm` failed
    // `get_tag_descriptor`'s lookup entirely, fell back to `TagValue::String`
    // here, and were only refused later -- with a misleading "Type mismatch"
    // -- once the writer's own (separately case-insensitive) tag resolution
    // found the real Rational descriptor. Canonicalizing both the group and
    // the leaf here, for exactly the four tags this fix covers, makes the
    // rest of this function (and `get_tag_descriptor`) see the same spelling
    // it would for the canonical form. Confirmed against the oracle: both
    // spellings above behave exactly like `-ExifIFD:FocalLength=50 mm`.
    let declared_tag_name_owned;
    let declared_tag_name = match canonicalize_unit_suffix_tag_name(declared_tag_name) {
        Some(canonical) => {
            declared_tag_name_owned = canonical;
            declared_tag_name_owned.as_str()
        }
        None => declared_tag_name,
    };

    let leaf = declared_tag_name.rsplit(':').next();

    // Writer.pl `ReverseLookup` (13.59:3614-3620): for a PrintConv HASH, a
    // printed `Unknown (X)` is its raw value X (`0x..` read as hex), taken
    // without any label lookup -- exactly what `raw_mode` does, so the value
    // is re-parsed in raw mode. ExifTool prints a code it has no label for
    // that way (t/images/Ricoh2.jpg: `GPSDestDistanceRef: Unknown ()`) and a
    // copy hands it back. Pinned 13.59: `-GPS:GPSStatus=Unknown (X)` writes
    // `X`, `-GPS:GPSDestDistanceRef=Unknown ()` the empty string,
    // `-ExifIFD:ColorSpace=Unknown (3)` 3, `-IFD0:Orientation=unknown(0x10)`
    // 16, while `-GPS:GPSStatus=Unknown (Xy)` is `String too long`. A tag
    // whose PrintConvInv is a routine rather than a hash lookup
    // (`Contrast`'s ConvertParameter: `Error converting value ...
    // (PrintConvInv)`) is not a hash here and keeps its own handling.
    //
    // This used to be a GPS-reference-only strip that ran after this PR's
    // catch-all arms were in place, so `Unknown ()` reached the
    // GPSDestDistanceRef catch-all as `""` and was refused; it also ran
    // under `raw_mode`, where ExifTool never calls `ReverseLookup`.
    if !raw_mode
        && let Some(inner) = unknown_print_conv_value(raw)
        && print_conv_is_lookup_hash(declared_tag_name)
    {
        return parse_cli_tag_value_with_mode(tag_name, &inner, true);
    }

    // Raw mode (`#`, `--no-print-conv`) skips the PrintConvInv of
    // FlashpixVersion and ExifVersion (both writable `undef`, no Count):
    // pinned 13.59 stores `-ExifIFD:ExifVersion#=2.31` as the four bytes
    // `2.31` and `#=02310` as five, where the dot-stripping inverse below
    // stored `0231` (Codex pre-review of PR #959, round 5).
    if raw_mode && matches!(leaf, Some("FlashpixVersion" | "ExifVersion")) {
        return Ok(TagValue::Binary(raw.as_bytes().to_vec()));
    }

    if leaf == Some("FlashpixVersion") {
        let encoded: String = raw.chars().filter(|character| *character != '.').collect();
        if encoded.len() != 4 || !encoded.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid(tag_name, "Error converting value (PrintConvInv)"));
        }
        return Ok(TagValue::Binary(encoded.into_bytes()));
    }

    // `raw_mode` (`#`, `--no-print-conv`) was ignored here (Codex PR #959
    // round 4): `-ExifIFD:ComponentsConfiguration#="1 2 3 0"` and the
    // `--no-print-conv` equivalent still ran the label-only parser
    // (`parse_components_configuration`, which only recognizes `Y`/`Cb`/
    // `Cr`/`R`/`G`/`B`/`-`), so the canonical four raw byte codes the oracle
    // accepts under `#`/`-n` were refused as "not in PrintConv". Confirmed
    // against the oracle, which stores the raw bytes verbatim under
    // `#`/`-n`.
    if leaf == Some("ComponentsConfiguration") {
        return if raw_mode {
            parse_components_configuration_raw(tag_name, raw)
        } else {
            parse_components_configuration(tag_name, raw)
        };
    }

    // `raw_mode` (`#`, `--no-print-conv`) bypasses the word lookup for both
    // of these the same way it bypasses every other PrintConv inversion in
    // this function: the value is already the tag's raw stored form
    // (an int16u code for SubjectDistanceRange, a single ASCII letter for
    // SecurityClassification), not a label to look up.
    if leaf == Some("SubjectDistanceRange") && !raw_mode {
        let value = match raw.trim().to_ascii_lowercase().as_str() {
            "unknown" => 0,
            "macro" => 1,
            "close" => 2,
            "distant" => 3,
            _ => {
                return Err(invalid(
                    tag_name,
                    "Can't convert SubjectDistanceRange value (not in PrintConv)",
                ));
            }
        };
        return Ok(TagValue::Integer(value));
    }

    if leaf == Some("SecurityClassification") && !raw_mode {
        let value = match raw.trim().to_ascii_lowercase().as_str() {
            "top secret" => "T",
            "secret" => "S",
            "confidential" => "C",
            "restricted" => "R",
            "unclassified" => "U",
            _ => {
                return Err(invalid(
                    tag_name,
                    "Can't convert SecurityClassification value (not in PrintConv)",
                ));
            }
        };
        return Ok(TagValue::String(value.to_string()));
    }

    // Exif.pm 13.59 0x8827 declares ISO as a variable-count int16u list
    // (`Writable => 'int16u'`, `Count => -1`), reached through
    // `PrintConvInv => '$val=~tr/,//d'`.
    if matches!(leaf, Some("ISO")) {
        // Raw mode skips `tr/,//d`, and pinned 13.59's raw path then reads a
        // comma in ways this port does not model (`#=1,000` stores 1,
        // `#=1,5` stores 2, `#=100, 200` is `Not an integer`), so a raw
        // value with a comma is refused rather than guessed at (Codex
        // pre-review of PR #959, round 5). Whitespace-separated raw values
        // take the same `CheckValue` rules as below.
        if raw_mode && raw.contains(',') {
            return Err(invalid(
                tag_name,
                "oxidex does not convert a raw ISO value containing a comma",
            ));
        }
        // `tr/,//d` *deletes* commas, it does not turn them into separators.
        // "100, 200" is two values because of the space the comma left behind,
        // but "100,200" collapses into the single value 100200, which then
        // fails the int16u range check below.
        let normalized: String = raw.chars().filter(|character| *character != ',').collect();
        let parts: Vec<_> = normalized.split_whitespace().collect();
        if parts.is_empty() {
            return Err(invalid(
                tag_name,
                "Expected at least one unsigned 16-bit ISO value",
            ));
        }
        // `CheckValue` rounds a fractional value only when it is the whole
        // value: `return 'Not an integer' unless IsFloat($val) and $count == 1`
        // (Writer.pl:6900), and `Count => -1` sets `$count` to the number of
        // values supplied (Writer.pl:6882). So "800.5" is stored as 801 while
        // "800.5 900" is rejected outright. `IsInt`/`IsHex` carry no such
        // restriction.
        let rounding_allowed = parts.len() == 1;
        let values = parts
            .into_iter()
            .map(|part| {
                if !rounding_allowed && !is_int(part) && !is_hex(part) {
                    return Err(invalid(tag_name, "Not an integer"));
                }
                let value = parse_integer(tag_name, part)?;
                // `%intRange` int16u is [0, 0xffff] (Writer.pl:241); a value
                // outside it is refused and nothing is written. The oracle's
                // wording differs by end: 65536 gives "Value above int16u
                // maximum", but -1 reports "Value below int32u minimum",
                // because `-ExifIFD:ISO=` matches several candidate tags and
                // the surfaced warning comes from the last one tried. Either
                // way the write is refused.
                if !(0..=u16::MAX as i64).contains(&value) {
                    return Err(invalid(tag_name, "ISO value does not fit unsigned 16-bit"));
                }
                Ok(TagValue::new_integer(value))
            })
            .collect::<Result<Vec<_>>>()?;
        return if values.len() == 1 {
            Ok(values.into_iter().next().expect("one ISO value"))
        } else {
            Ok(TagValue::new_array(values))
        };
    }

    // UserComment (Exif.pm 0x9286) and GPSProcessingMethod /
    // GPSAreaInformation (GPS.pm 0x001b / 0x001c) apply `EncodeExifText` as
    // their RawConvInv, which needs the byte order of the EXIF block being
    // written (UTF-16 for non-ASCII text). The value stays the caller's text
    // here; the EXIF serializers encode it (`writers::exif_text`).
    if crate::tag_db::tag_registry::is_encode_exif_text_tag(tag_name) {
        return Ok(TagValue::String(raw.to_string()));
    }

    // Exif.pm 13.59 0x9000 writes ExifVersion as four UNDEFINED ASCII bytes.
    // PrintConvInv accepts dotted versions, removes their dots, and left-pads
    // a three-digit input (for example, `2.31` becomes `0231`).
    if declared_tag_name.rsplit(':').next() == Some("ExifVersion") {
        let digits = raw.replace('.', "");
        let digits = match digits.len() {
            4 if digits.bytes().all(|byte| byte.is_ascii_digit()) => digits,
            3 if digits.bytes().all(|byte| byte.is_ascii_digit()) => format!("0{digits}"),
            _ => {
                return Err(invalid(
                    tag_name,
                    "ExifVersion requires three or four digits, optionally separated by dots",
                ));
            }
        };
        return Ok(TagValue::Binary(digits.into_bytes()));
    }

    // Exif.pm 0x9010 delegates PrintConvInv to InverseOffsetTime. The stored
    // value is always a canonical EXIF offset, even when the caller supplies
    // a full date/time, `Z`, or an offset without a leading zero.
    //
    // `raw_mode` (`#`, `--no-print-conv`) was ignored by all three of these
    // (Codex PR #959 round 4): `-ExifIFD:OffsetTime#=Z` and `--no-print-conv
    // -ExifIFD:OffsetTime=Z` still ran `inverse_offset_time` and silently
    // stored `+00:00` instead of the caller's raw string `Z` -- confirmed
    // against the oracle, which stores `Z` verbatim under `#`/`-n` (this tag
    // is a plain ASCII string with no further `ValueConvInv` restriction, so
    // raw mode's job here is simply "skip `PrintConvInv`, take the string").
    if declared_tag_name.rsplit(':').next() == Some("OffsetTime") {
        if raw_mode {
            return Ok(TagValue::String(raw.to_string()));
        }
        let offset = inverse_offset_time(raw).ok_or_else(|| {
            invalid(
                tag_name,
                "Can't convert OffsetTime value to a time zone offset",
            )
        })?;
        return Ok(TagValue::String(offset));
    }

    if declared_tag_name.rsplit(':').next() == Some("OffsetTimeOriginal") {
        if raw_mode {
            return Ok(TagValue::String(raw.to_string()));
        }
        let offset = inverse_offset_time(raw).ok_or_else(|| {
            invalid(
                tag_name,
                "Can't convert OffsetTimeOriginal value to a time zone offset",
            )
        })?;
        return Ok(TagValue::String(offset));
    }

    if declared_tag_name.rsplit(':').next() == Some("OffsetTimeDigitized") {
        if raw_mode {
            return Ok(TagValue::String(raw.to_string()));
        }
        let offset = inverse_offset_time(raw).ok_or_else(|| {
            invalid(
                tag_name,
                "Can't convert OffsetTimeDigitized value to a time zone offset",
            )
        })?;
        return Ok(TagValue::String(offset));
    }

    // Exif.pm 0xa300 is writable undef. PrintConvInv accepts the three labels,
    // then ValueConvInv converts a decimal byte to its one-byte TIFF payload.
    // Under raw mode (`-FileSource#=`, `--no-print-conv`) ExifTool's `#`
    // skips PrintConvInv and takes the byte directly; the label lookup is
    // skipped the same way every other enum tag's is (see the leaf dispatch
    // below).
    if declared_tag_name.rsplit(':').next() == Some("FileSource") {
        if raw_mode {
            return Ok(file_source_raw(raw));
        }
        let raw = match raw {
            "Film Scanner" => "1",
            "Reflection Print Scanner" => "2",
            "Digital Camera" => "3",
            _ => {
                return Err(invalid(
                    tag_name,
                    format!(
                        "Can't convert {} (not in PrintConv)",
                        display_tag_for_message(tag_name, "FileSource")
                    ),
                ));
            }
        };
        return Ok(TagValue::Binary(vec![
            raw.parse::<u8>().expect("known byte"),
        ]));
    }

    // GPS.pm 0x001d GPSDateStamp PrintConvInv: the date part of any date or
    // date/time -- `$val =~ /(\d{4}).*?(\d{2}).*?(\d{2})/ ? "$1:$2:$3" :
    // undef` -- after adjusting a zoned date/time to UTC. No date is no
    // value (13.59 does not copy t/images/InfiRay.jpg's empty stamp); the
    // UTC adjustment and `now` are refused rather than approximated. The
    // adjustment runs only when the value holds a `-`/`+` *and* is a full
    // date/time `GetUnixTime` parses (`gps_date_stamp_adjusts_to_utc`), so a
    // hyphen used as a date separator (`2024-01-02`) is just a separator.
    if declared_tag_name.rsplit(':').next() == Some("GPSDateStamp")
        && declared_tag_name.rsplit_once(':').is_none_or(|(group, _)| {
            group.eq_ignore_ascii_case("GPS") || group.eq_ignore_ascii_case("EXIF")
        })
    {
        // Raw mode skips this PrintConvInv, leaving only GPS.pm's
        // ValueConvInv and the string[11] Count: pinned 13.59 stores
        // `#=2024:01:02` as is, refuses `#=2024:01:02 10:11:12` (`Data too
        // long`) and reformats `#=20240102`. Only the already-canonical
        // form is taken; the rest is refused rather than approximated.
        if raw_mode {
            let canonical = raw.len() == 10
                && raw.bytes().enumerate().all(|(at, byte)| match at {
                    4 | 7 => byte == b':',
                    _ => byte.is_ascii_digit(),
                });
            return if canonical {
                Ok(TagValue::String(raw.to_string()))
            } else {
                Err(invalid(
                    tag_name,
                    "oxidex takes a raw GPSDateStamp only as YYYY:mm:dd",
                ))
            };
        }
        if raw.eq_ignore_ascii_case("now") || gps_date_stamp_adjusts_to_utc(raw) {
            return Err(invalid(
                tag_name,
                "oxidex does not adjust a zoned GPSDateStamp to UTC; give the date",
            ));
        }
        let digits: Vec<(usize, char)> = raw.char_indices().collect();
        let run = |from: usize, len: usize| -> Option<(usize, String)> {
            (from..digits.len()).find_map(|start| {
                let text: String = digits
                    .get(start..start + len)?
                    .iter()
                    .map(|(_, c)| *c)
                    .collect();
                text.chars()
                    .all(|c| c.is_ascii_digit())
                    .then_some((start + len, text))
            })
        };
        let date = run(0, 4).and_then(|(after, year)| {
            let (after, month) = run(after, 2)?;
            let (_, day) = run(after, 2)?;
            Some(format!("{year}:{month}:{day}"))
        });
        return date
            .map(TagValue::String)
            .ok_or_else(|| invalid(tag_name, "GPSDateStamp needs a date (YYYY:mm:dd)"));
    }

    // Exif.pm 0xa302 CFAPattern: PrintConvInv `GetCFAPattern` turns the
    // printed `[Blue,Green][Green,Red]` into the dimensions and colours, and
    // RawConvInv packs them in the file's byte order. oxidex does neither;
    // refuse the text rather than store it as the tag's bytes.
    if declared_tag_name.rsplit(':').next() == Some("CFAPattern") {
        return Err(invalid(
            tag_name,
            "oxidex does not convert a CFAPattern value (GetCFAPattern)",
        ));
    }

    // Exif.pm 0xa301 is writable undef, one byte, with a single-entry
    // PrintConv hash (`{1 => 'Directly photographed'}`). Unregistered in
    // the tag registry, so with no conversion here the label reached the
    // generic `ValueType::Binary` arm below and was stored as its own UTF-8
    // bytes verbatim (`"Directly photographed"`, 21 bytes) instead of the
    // single byte `01` ExifTool writes. The table lookup is the same
    // mechanism as every plain enum tag; only the wrapping differs, because
    // `undef` stores the code as raw bytes, not a TIFF SHORT.
    //
    // This dispatched on `leaf` name alone and hardcoded `Exif::Main` for the
    // non-raw path, same mistake as `Sony:ExposureMode` (Codex PR #959 round
    // 2/4): the repository's own DICOM dictionary declares a real
    // `DICOM:SceneType` field (`src/parsers/specialized/dicom_dict.rs`,
    // `0016,003B`, `US` = unsigned short, nothing like Exif.pm's one-byte
    // `undef`), so without a table check this arm would take EXIF's
    // `"Directly photographed"` label -- or even a bare raw integer through
    // `scene_type_raw`, which assumes the same one-byte `undef` shape
    // -- and apply it to a DICOM field with a completely different meaning
    // and width. `exif_or_gps_module_for` gates both branches on the tag
    // actually resolving to `Exif::Main`.
    if declared_tag_name.rsplit(':').next() == Some("SceneType")
        && exif_or_gps_module_for(declared_tag_name) == Some("Exif")
    {
        if raw_mode {
            return scene_type_raw(tag_name, raw);
        }
        return match invert_enum_printconv("Exif", "SceneType", declared_tag_name, raw) {
            Some(Ok(code @ 0..=255)) => Ok(TagValue::Binary(vec![code as u8])),
            Some(Ok(_)) => Err(invalid(tag_name, "SceneType code does not fit a byte")),
            Some(Err(EnumInverseError::Ambiguous)) => Err(invalid(
                tag_name,
                format!(
                    "Can't convert {} (matches more than one PrintConv)",
                    display_tag_for_message(tag_name, "SceneType")
                ),
            )),
            Some(Err(EnumInverseError::NoMatch)) | None => Err(invalid(
                tag_name,
                format!(
                    "Can't convert {} (not in PrintConv)",
                    display_tag_for_message(tag_name, "SceneType")
                ),
            )),
        };
    }

    // GPS.pm 13.59 converts GPSDestLatitude's decimal input into a three-part
    // DMS value before rationalizing the components. Preserve finite decimal
    // text exactly here so that later conversion does not start from the
    // generic rational parser's deliberately approximate continued fraction.
    if tag_name == "GPS:GPSDestLatitude"
        && let Some((numerator, denominator)) = exact_decimal_fraction(raw)
    {
        return Ok(TagValue::Rational {
            numerator,
            denominator,
        });
    }

    // GPS.pm 0x0000 applies `tr/./ /` before checking four int8u values.
    // Preserve those numeric components as bytes instead of the ASCII text.
    if tag_name.rsplit(':').next() == Some("GPSVersionID") {
        // `tr/./ /` is the PrintConvInv, which raw mode skips: pinned 13.59
        // refuses `-GPS:GPSVersionID#=2.3.0.0` (`Not enough values
        // specified (4 required)`).
        let normalized = if raw_mode {
            raw.to_string()
        } else {
            raw.replace('.', " ")
        };
        let bytes = normalized
            .split_ascii_whitespace()
            .map(|part| part.parse::<u8>())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| invalid(tag_name, "Expected four unsigned bytes"))?;
        if bytes.len() != 4 {
            return Err(invalid(tag_name, "Expected four unsigned bytes"));
        }
        return Ok(TagValue::Binary(bytes));
    }

    if declared_tag_name.rsplit(':').next() == Some("SubjectArea") {
        let components = raw
            .split_whitespace()
            .map(|component| component.parse::<u16>())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| {
                invalid(
                    tag_name,
                    "SubjectArea requires 2 to 4 unsigned short values",
                )
            })?;
        if !(2..=4).contains(&components.len()) {
            return Err(invalid(
                tag_name,
                "SubjectArea requires 2 to 4 unsigned short values",
            ));
        }
        return Ok(TagValue::Array(
            components
                .into_iter()
                .map(|component| TagValue::Integer(i64::from(component)))
                .collect(),
        ));
    }

    // Exif.pm 13.59 0xa214 stores exactly two int16u components (X and Y).
    if declared_tag_name.rsplit(':').next() == Some("SubjectLocation") {
        return parse_subject_location(tag_name, raw);
    }

    // Exif.pm 13.59 0x0129 declares PageNumber as `int16u[2]`. Unlike a
    // scalar integer, the CLI spelling contains both the zero-based page
    // index and the document page count.
    if matches!(
        tag_name,
        "PageNumber" | "EXIF:PageNumber" | "IFD0:PageNumber"
    ) {
        let parts: Vec<_> = raw.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(invalid(
                tag_name,
                "Expected two unsigned 16-bit page numbers",
            ));
        }
        let values = parts
            .into_iter()
            .map(|part| {
                let value = parse_integer(tag_name, part)?;
                if !(0..=u16::MAX as i64).contains(&value) {
                    return Err(invalid(
                        tag_name,
                        "Page number does not fit unsigned 16-bit",
                    ));
                }
                Ok(TagValue::new_integer(value))
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(TagValue::new_array(values));
    }

    // Exif.pm 13.59 0xa461 declares CompositeImageCount as `int16u[2]`.
    if matches!(
        tag_name,
        "CompositeImageCount" | "EXIF:CompositeImageCount" | "ExifIFD:CompositeImageCount"
    ) {
        let parts: Vec<_> = raw.split_whitespace().collect();
        if parts.len() != 2 {
            return Err(invalid(
                tag_name,
                "Expected two unsigned 16-bit composite image counts",
            ));
        }
        let values = parts
            .into_iter()
            .map(|part| {
                let value = parse_integer(tag_name, part)?;
                if !(0..=u16::MAX as i64).contains(&value) {
                    return Err(invalid(
                        tag_name,
                        "Composite image count does not fit unsigned 16-bit",
                    ));
                }
                Ok(TagValue::new_integer(value))
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(TagValue::new_array(values));
    }

    // `Unknown (X)` (Writer.pl `ReverseLookup`) is handled at the top of
    // this function, before any PrintConv arm: see
    // `unknown_print_conv_value`.
    //
    // `GPSLatitudeRef`/`GPSDestLatitudeRef`'s inversion is the PrintConv
    // hash's `OTHER` routine (GPS.pm 0x0001/0x0013), so `raw_mode` (`#`,
    // `--no-print-conv`) skips it like every other PrintConv inverse (PR
    // #959, `4112816741`): pinned 13.59 stores `-GPS:GPSLatitudeRef#=X` as
    // `X` and refuses `#=South` as `String too long` (Count 2), where this
    // used to turn `South` into `S` and refuse `X`. The raw string then
    // takes the GPS reference Count check in the declared-type match below.
    let raw = if !raw_mode
        && matches!(
            tag_name,
            "GPSLatitudeRef"
                | "GPS:GPSLatitudeRef"
                | "GPSDestLatitudeRef"
                | "GPS:GPSDestLatitudeRef"
        ) {
        invert_gps_latitude_ref(tag_name, raw)?
    } else {
        raw
    };

    // Exif.pm 0x9291/0x9292 (SubSecTimeOriginal/SubSecTime) is a ValueConv
    // (fraction extraction), not a PrintConv -- ExifTool's `#`/`-n` bypass
    // PrintConvInv only, never ValueConvInv, so this stays active under
    // `raw_mode` and is kept in its own match, ahead of the PrintConv-only
    // one below that `raw_mode` does skip entirely.
    let raw = match (tag_name, raw) {
        ("EXIF:SubSecTimeOriginal" | "ExifIFD:SubSecTimeOriginal", value) => {
            invert_subsec_time(value)
                .ok_or_else(|| invalid(tag_name, "SubSecTimeOriginal needs fractional seconds"))?
        }
        ("EXIF:SubSecTime" | "ExifIFD:SubSecTime", value) => invert_subsec_time(value)
            .ok_or_else(|| invalid(tag_name, "SubSecTime needs fractional seconds"))?,
        _ => raw,
    };

    // GPS.pm 0x000a: the TIFF value is ASCII "2" or "3", while ExifTool's
    // PrintConv exposes the corresponding measurement label.  Writer.pl
    // applies that PrintConvInv before its generic string check.
    //
    // Every arm below is a PrintConv label lookup, so `raw_mode` (`#`,
    // `--no-print-conv`) skips this whole match rather than gating each of
    // its ~20 arms individually: `-GPS:GPSDifferential#=0` and
    // `-ExifIFD:Contrast#=1` must take the raw code directly, not run
    // through `ReverseLookup`/`ConvertParameter` at all (confirmed against
    // the oracle: `-ExifIFD:Contrast#=1` writes `1`, not `2`).
    let raw = if raw_mode {
        raw
    } else {
        match (tag_name, raw) {
            // Exif.pm 0xa001 is a writable int16u with this PrintConv table.
            ("EXIF:ColorSpace" | "ExifIFD:ColorSpace", "sRGB") => "1",
            ("EXIF:ColorSpace" | "ExifIFD:ColorSpace", "Adobe RGB") => "2",
            ("EXIF:ColorSpace" | "ExifIFD:ColorSpace", "Uncalibrated") => "65535",
            ("EXIF:ColorSpace" | "ExifIFD:ColorSpace", "ICC Profile") => "65534",
            ("EXIF:ColorSpace" | "ExifIFD:ColorSpace", "Wide Gamut RGB") => "65533",
            // A catch-all, same reasoning as `GPSDifferential`'s below: without
            // one, a value matching none of the five labels above (a bare
            // numeric code included) fell through this match unchanged and
            // reached the generic integer parser, which stored it --
            // confirmed on `-ExifIFD:ColorSpace=garbage`: pinned ExifTool
            // 13.59 refuses it (`Can't convert ExifIFD:ColorSpace (not in
            // PrintConv)`), but this file wrote the raw text with nothing to
            // catch it. `ColorSpace` is excluded from the generic
            // table-driven dispatch below (it is hand-written here instead,
            // this being its own catch-all), so this is the only place that
            // can refuse it.
            ("EXIF:ColorSpace" | "ExifIFD:ColorSpace", _) => {
                return Err(invalid(
                    tag_name,
                    format!(
                        "Can't convert {} (not in PrintConv)",
                        display_tag_for_message(tag_name, "ColorSpace")
                    ),
                ));
            }
            // Exif.pm 0xa408 uses ConvertParameter as its PrintConvInv rather
            // than a direct label map. It accepts the documented display labels
            // and any signed float, collapsing them to the three stored codes.
            ("Contrast" | "EXIF:Contrast" | "ExifIFD:Contrast", value) => {
                // ExifTool's wording for a failed ConvertParameter, not the
                // hash lookup's `(not in PrintConv)`: pinned 13.59 answers
                // `-ExifIFD:Contrast=bogus` with `Error converting value for
                // ExifIFD:Contrast (PrintConvInv)` and the file unchanged, so
                // the CLI must not frame it as `Nothing to do.` (Codex
                // pre-review of PR #959, round 5).
                invert_exif_contrast_parameter(value).ok_or_else(|| {
                    invalid(
                        tag_name,
                        format!(
                            "Error converting value for {} (PrintConvInv)",
                            display_tag_for_message(tag_name, "Contrast")
                        ),
                    )
                })?
            }
            // Exif.pm 0xa401 stores int16u values and defines Apple extension
            // labels in addition to the two standard EXIF values.
            ("CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered", "Normal") => "0",
            ("CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered", "Custom") => "1",
            (
                "CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered",
                "HDR (no original saved)",
            ) => "2",
            (
                "CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered",
                "HDR (original saved)",
            ) => "3",
            (
                "CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered",
                "Original (for HDR)",
            ) => "4",
            ("CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered", "Panorama") => {
                "6"
            }
            (
                "CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered",
                "Portrait HDR",
            ) => "7",
            ("CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered", "Portrait") => {
                "8"
            }
            ("CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered", _) => {
                return Err(invalid(
                    tag_name,
                    "Can't convert CustomRendered value (not in PrintConv)",
                ));
            }
            // GainControl (Exif.pm 0xa407) is a plain int16u enum `PrintConv`,
            // inverted generically below (`declared_tag_name` leaf dispatch)
            // against the transcribed table rather than this hand-written list.
            ("GPS:GPSStatus", "Measurement Active") => "A",
            ("GPS:GPSStatus", "Measurement Void") => "V",
            // A catch-all: without one, a value matching neither label above
            // (including outright garbage) fell through this match unchanged
            // and reached the plain string parser, which stored it verbatim
            // -- confirmed on `-GPS:GPSStatus=garbage`: pinned ExifTool 13.59
            // refuses it (`Can't convert GPS:GPSStatus (not in PrintConv)`),
            // but this file wrote it with nothing to catch it.
            //
            // Before refusing outright, this also tries the one extra
            // `ReverseLookup` tier (`Writer.pl:3609`) this port otherwise
            // does not implement: a case-insensitive PREFIX match, tried
            // before the case-insensitive SUBSTRING tier that follows it.
            // `GPSMeasureMode`/`GPSDestDistanceRef`'s own raw codes happen to
            // be unique prefixes of their own labels ("2" only prefixes
            // "2-Dimensional Measurement", not "3-Dimensional Measurement";
            // "K" only prefixes "Kilometers"), so this tier resolves them
            // exactly the way the oracle does -- not a guess, because a
            // unique prefix match cannot select a different code than the
            // oracle's own algorithm would. `GPSStatus`'s codes ("A"/"V") are
            // NOT unique prefixes of either label (both start with "M"), so
            // this tier correctly finds nothing for them and they still
            // refuse -- matching the oracle's own refusal too, just by a
            // different route (the oracle reaches its SUBSTRING tier, finds
            // `A`/`V` inside BOTH labels, and refuses as ambiguous; this port
            // stops one tier earlier having found no match at all). Neither
            // outcome is ever a wrong WRITTEN value.
            ("GPS:GPSStatus", other) => {
                match unique_case_insensitive_prefix_match(GPS_STATUS_ENTRIES, other) {
                    Some(code) => code,
                    None => {
                        return Err(invalid(
                            tag_name,
                            "Can't convert GPS:GPSStatus (not in PrintConv)",
                        ));
                    }
                }
            }
            ("GPS:GPSMeasureMode", "2-Dimensional Measurement") => "2",
            ("GPS:GPSMeasureMode", "3-Dimensional Measurement") => "3",
            // Catch-all with the same prefix-tier fallback as `GPSStatus`
            // above; see that arm's comment for the full reasoning.
            ("GPS:GPSMeasureMode", other) => {
                match unique_case_insensitive_prefix_match(GPS_MEASURE_MODE_ENTRIES, other) {
                    Some(code) => code,
                    None => {
                        return Err(invalid(
                            tag_name,
                            "Can't convert GPS:GPSMeasureMode (not in PrintConv)",
                        ));
                    }
                }
            }
            ("GPS:GPSDestDistanceRef", "Kilometers") => "K",
            ("GPS:GPSDestDistanceRef", "Miles") => "M",
            ("GPS:GPSDestDistanceRef", "Nautical Miles") => "N",
            // Catch-all with the same prefix-tier fallback as `GPSStatus`
            // above; see that arm's comment for the full reasoning. "M"
            // uniquely PREFIX-matches "Miles" (not "Nautical Miles", which
            // starts with "N"), so it resolves here even though "M" is also
            // a substring of "Nautical Miles" -- the oracle's own algorithm
            // stops at the prefix tier too, before it would ever see that
            // ambiguity, confirmed directly against it.
            ("GPS:GPSDestDistanceRef", other) => {
                match unique_case_insensitive_prefix_match(GPS_DEST_DISTANCE_REF_ENTRIES, other) {
                    Some(code) => code,
                    None => {
                        return Err(invalid(
                            tag_name,
                            "Can't convert GPS:GPSDestDistanceRef (not in PrintConv)",
                        ));
                    }
                }
            }
            ("GPS:GPSDifferential", "No Correction") => "0",
            ("GPS:GPSDifferential", "Differential Corrected") => "1",
            // A catch-all for each of these three: without one, a value that
            // matches neither label above (a bare numeric code included) fell
            // through this match unchanged and reached the generic integer
            // parser, which happily stored it -- confirmed on
            // `-GPS:GPSDifferential=0`: pinned ExifTool 13.59 refuses it
            // (`Can't convert GPS:GPSDifferential (not in PrintConv)`, the same
            // hash-`PrintConv`-with-no-`OTHER` refusal as `Orientation`'s), but
            // this file wrote it as the raw code with nothing to catch it.
            ("GPS:GPSDifferential", _) => {
                return Err(invalid(
                    tag_name,
                    "Can't convert GPS:GPSDifferential (not in PrintConv)",
                ));
            }
            ("EXIF:PlanarConfiguration" | "IFD0:PlanarConfiguration", "Chunky") => "1",
            ("EXIF:PlanarConfiguration" | "IFD0:PlanarConfiguration", "Planar") => "2",
            ("EXIF:PlanarConfiguration" | "IFD0:PlanarConfiguration", _) => {
                return Err(invalid(
                    tag_name,
                    format!(
                        "Can't convert {} (not in PrintConv)",
                        display_tag_for_message(tag_name, "PlanarConfiguration")
                    ),
                ));
            }
            // YCbCrPositioning (Exif.pm 13.59 0x0213) is a plain int16u enum
            // `PrintConv` now inverted generically against the transcribed table
            // -- see the `declared_tag_name` leaf dispatch below. SubSecTime*
            // moved to its own always-active match above `raw_mode`'s branch.
            // Exif.pm 13.59 0xc635 converts the writable int16u code to these
            // labels. Apply the inverse before the generic integer parser.
            ("MakerNoteSafety" | "EXIF:MakerNoteSafety" | "IFD0:MakerNoteSafety", "Unsafe") => "0",
            ("MakerNoteSafety" | "EXIF:MakerNoteSafety" | "IFD0:MakerNoteSafety", "Safe") => "1",
            ("MakerNoteSafety" | "EXIF:MakerNoteSafety" | "IFD0:MakerNoteSafety", _) => {
                return Err(invalid(
                    tag_name,
                    format!(
                        "Can't convert {} (not in PrintConv)",
                        display_tag_for_message(tag_name, "MakerNoteSafety")
                    ),
                ));
            }
            (
                "ProfileEmbedPolicy" | "EXIF:ProfileEmbedPolicy" | "IFD0:ProfileEmbedPolicy",
                "Allow Copying",
            ) => "0",
            (
                "ProfileEmbedPolicy" | "EXIF:ProfileEmbedPolicy" | "IFD0:ProfileEmbedPolicy",
                "Embed if Used",
            ) => "1",
            (
                "ProfileEmbedPolicy" | "EXIF:ProfileEmbedPolicy" | "IFD0:ProfileEmbedPolicy",
                "Never Embed",
            ) => "2",
            (
                "ProfileEmbedPolicy" | "EXIF:ProfileEmbedPolicy" | "IFD0:ProfileEmbedPolicy",
                "No Restrictions",
            ) => "3",
            ("ProfileEmbedPolicy" | "EXIF:ProfileEmbedPolicy" | "IFD0:ProfileEmbedPolicy", _) => {
                return Err(invalid(
                    tag_name,
                    format!(
                        "Can't convert {} (not in PrintConv)",
                        display_tag_for_message(tag_name, "ProfileEmbedPolicy")
                    ),
                ));
            }
            _ => raw,
        }
    };
    // Exif.pm 13.59 0x920a/0xa405/0x9400 append a literal unit to a plain
    // numeric PrintConv (` mm`, ` mm`, an optional-space `C`) that
    // PrintConvInv strips again before the value reaches CheckValue -- see
    // `strip_printconv_unit_suffix` for the exact citations. Raw mode (a
    // trailing `#` on one of these four tag names, recognised above) skips
    // this exactly as it skips every other PrintConvInv: the caller already
    // supplied the bare stored number.
    //
    // The leaf lookup goes through `unit_suffix_tag` rather than a direct
    // string match: ExifTool tag and group names are case-insensitive, so
    // `-ExifIFD:focallength=50 mm` and `-exififd:FocalLength=50 mm` must
    // take this same path (confirmed against the oracle) even though
    // `declared_tag_name`'s own casing here is whatever the caller typed.
    let stripped_unit_owned;
    let raw = if raw_mode {
        raw
    } else {
        match declared_tag_name
            .rsplit(':')
            .next()
            .and_then(unit_suffix_tag)
        {
            Some(leaf @ ("FocalLength" | "FocalLengthIn35mmFormat" | "AmbientTemperature")) => {
                stripped_unit_owned = strip_printconv_unit_suffix(leaf, raw);
                stripped_unit_owned.as_ref()
            }
            _ => raw,
        }
    };
    let raw = match declared_tag_name.rsplit(':').next() {
        // Saturation (Exif.pm 0xa409) is ConvertParameter too, same as
        // Contrast above -- `raw_mode` skips it, taking the raw code
        // directly (confirmed against the oracle the same way as Contrast).
        Some("Saturation") if raw_mode => raw,
        Some("Saturation") => invert_exif_contrast_parameter(raw).ok_or_else(|| {
            invalid(
                tag_name,
                format!(
                    "Error converting value for {} (PrintConvInv)",
                    display_tag_for_message(tag_name, "Saturation")
                ),
            )
        })?,
        // Orientation (0x0112), ResolutionUnit (0x0128), Compression
        // (0x0103), YCbCrPositioning (0x0213), ExposureMode (0xa402),
        // MeteringMode (0x9207), ExposureProgram (0x8822), WhiteBalance
        // (0xa403), SceneCaptureType (0xa406), GainControl (0xa407) and
        // GrayResponseUnit (0x0122) are plain int16u tags whose `PrintConv`
        // is a flat enum hash with no duplicate label, no OTHER and no
        // PrintHex. Several gained no hand-written inverse at all and so
        // rejected every label ExifTool itself writes ("Rotate 90 CW",
        // "inches", ...); the rest had one but matched case-sensitively
        // only, or (`GrayResponseUnit`) never noticed its own labels are
        // digit strings. Rather than transcribing more per-tag match arms,
        // invert against the table `tools/exiftool-tables` already
        // transcribed (`exiftool_tables::find_ifd_table("Exif", "Main")`):
        // exact match first, then case-insensitive, only when exactly one
        // entry matches either way -- [`invert_enum_printconv`] ports
        // ExifTool's `ReverseLookup` (`Writer.pl:3609`).
        //
        // No numeric bypass, for any of them: pinned ExifTool 13.59 refuses
        // a raw numeric code here too (`-Orientation=6` without `-n` is
        // `Warning: Can't convert IFD0:Orientation (not in PrintConv)`,
        // confirmed against the oracle) -- `ReverseLookup` never falls back
        // to a numeric code for a hash `PrintConv` with no `OTHER`, so
        // neither does this port. `raw_mode` (the CLI's `#` suffix or
        // `--no-print-conv`, ExifTool's `-n`) is the only way to write a raw
        // code, and it skips this whole arm rather than special-casing
        // numeric shape, matching ExifTool exactly: `-Orientation#=Bogus`
        // fails as "not an integer", not as "not in PrintConv".
        Some(
            leaf @ ("Orientation" | "ResolutionUnit" | "Compression" | "YCbCrPositioning"
            | "ExposureMode" | "MeteringMode" | "ExposureProgram" | "WhiteBalance"
            | "SceneCaptureType" | "GainControl" | "GrayResponseUnit"),
        ) if !raw_mode && exif_or_gps_module_for(declared_tag_name) == Some("Exif") => {
            return invert_table_printconv_label(tag_name, declared_tag_name, leaf, raw);
        }
        // LightSource (0x9208) repeats the "Daylight" label at codes 1 and
        // 25; ExifTool's own tie-break (first match in `sort keys %$conv`
        // order) picks code 1, which this generic path's stricter
        // "exactly one match" rule would instead refuse as ambiguous. Kept
        // hand-written rather than mis-migrated.
        //
        // DNG's CalibrationIlluminant1/2/3 (0xc65a/0xc65b/0xcd31) declare
        // this exact same `%lightSource` hash as their `PrintConv` --
        // verbatim, duplicate "Daylight" included -- so they share this
        // table rather than either the ambiguity-refusing generic path or a
        // second hand-transcribed copy. Before this arm covered them, their
        // digit-and-letter labels ("D55", "D65", "D75", "D50") fell through
        // to the generic integer parser, whose `IsHex` branch (`Writer.pl`'s
        // `IsHex`, tried before `IsFloat`) accepted "D55" as the hex value
        // 0x0D55 = 3413 -- a confident, wrong, silently-written code under a
        // real tag name (confirmed identical on be202db3, so unrelated to
        // this fix's Orientation/ResolutionUnit/etc. gap; fixed the same
        // way: the label reaches this table before it ever reaches a
        // numeric parser). `raw_mode` skips this arm too, same as the group
        // above.
        // Matched case-insensitively too (`invert_int_enum`, the same
        // exact-then-case-insensitive algorithm the generic table lookup
        // above uses): `-CalibrationIlluminant1=d65` resolves to 21 just
        // like `D65`. Safe to reuse directly (no ambiguity risk) because
        // `LIGHT_SOURCE_LABELS` already omits the code-25 "Daylight"
        // duplicate, so case-insensitive matching never sees two
        // candidates for it.
        Some(
            leaf @ ("LightSource"
            | "CalibrationIlluminant1"
            | "CalibrationIlluminant2"
            | "CalibrationIlluminant3"),
        ) if !raw_mode && exif_or_gps_module_for(declared_tag_name) == Some("Exif") => {
            return match invert_int_enum(LIGHT_SOURCE_LABELS, raw) {
                Ok(code) => Ok(TagValue::Integer(code)),
                Err(EnumInverseError::Ambiguous) => Err(invalid(
                    tag_name,
                    format!(
                        "Can't convert {} (matches more than one PrintConv)",
                        display_tag_for_message(tag_name, leaf)
                    ),
                )),
                Err(EnumInverseError::NoMatch) => Err(invalid(
                    tag_name,
                    format!(
                        "Can't convert {} (not in PrintConv)",
                        display_tag_for_message(tag_name, leaf)
                    ),
                )),
            };
        }
        _ => raw,
    };
    let declared = get_tag_descriptor(declared_tag_name)
        .filter(|_| has_reliable_value_type(declared_tag_name))
        .map(|descriptor| descriptor.value_type())
        .or_else(|| declared_value_type_from_transcribed_table(declared_tag_name));

    match declared {
        // Writer.pl `CheckValue` (13.59:6867-6871): a `string` with a Count
        // holds at most Count-1 bytes (the NUL). Every GPS letter-code
        // reference is `string[2]`, so a raw (`#`, `--no-print-conv`,
        // `Unknown (X)`) value longer than one byte is refused, as the oracle
        // refuses `-GPS:GPSLatitudeRef#=South` and `-GPS:GPSStatus#=Xy`.
        None | Some(ValueType::String)
            if is_gps_reference_string(declared_tag_name) && raw.len() >= 2 =>
        {
            Err(invalid(
                tag_name,
                format!(
                    "String too long for GPS:{}",
                    declared_tag_name
                        .rsplit(':')
                        .next()
                        .unwrap_or(declared_tag_name)
                ),
            ))
        }
        None | Some(ValueType::String) => Ok(TagValue::String(raw.to_string())),
        // Exif.pm 0xa40a (Sharpness) is ConvertParameter too, same family as
        // Contrast/Saturation above -- `raw_mode` (`#`, `--no-print-conv`)
        // must take the raw code directly rather than running it through
        // `parse_sharpness`'s word-initial/sign mapping (confirmed against
        // the oracle: `-ExifIFD:Sharpness#=1` writes `1`, not `2`). This arm
        // is gated the same way the generic enum-dispatch arm below is
        // gated for its own list of hand-written conversions; when the guard
        // fails, `raw` falls through unchanged to the plain integer parser.
        Some(ValueType::Integer)
            if !raw_mode && declared_tag_name.rsplit(':').next() == Some("Sharpness") =>
        {
            parse_sharpness(tag_name, raw)
        }
        Some(ValueType::Integer)
            if !raw_mode && declared_tag_name.rsplit(':').next() == Some("Flash") =>
        {
            crate::core::formatters::exif_enums::parse_flash_label(raw)
                .map(TagValue::Integer)
                .ok_or_else(|| {
                    invalid(
                        tag_name,
                        format!(
                            "Can't convert {} (not in PrintConv)",
                            display_tag_for_message(tag_name, "Flash")
                        ),
                    )
                })
        }
        // Generic fallback for every OTHER Integer-typed tag: the leaf
        // dispatch above only names the tags this fix specifically
        // examined, but the transcribed `Exif::Main`/`GPS::Main` tables
        // carry a flat enum `PrintConv` for roughly fifty tags in all, and
        // a breadth measurement against the pinned oracle over every one of
        // them (not just the ten this fix set out to cover) found the same
        // defect on ~28 more -- `ChromaticAberrationCorrection`,
        // `PhotometricInterpretation`, `Thresholding`, `Sharpness`'s own
        // sibling tags, and so on -- with no PrintConv gate at all, so a
        // bare numeric code that matched no label was silently accepted as
        // the tag's raw value: the same `-Orientation=6` defect this fix
        // exists to close, just not yet wired up for these. Rather than
        // writing thirty more per-tag match arms, consult the table
        // generically: if `leaf` has a plain `IntEnum` PrintConv, `raw` must
        // match one of its labels (unless `raw_mode`); if the table has no
        // entry for `leaf`, or its PrintConv is not a plain enum, this falls
        // through unchanged to the plain integer parser, exactly as before.
        //
        // Excluded: leaves a hand-written arm above ALREADY fully resolves
        // by the time control reaches here (`ColorSpace`, `Contrast`,
        // `CustomRendered`, `GPSDifferential`, `GPSStatus`,
        // `GPSMeasureMode`, `GPSDestDistanceRef`, `PlanarConfiguration`,
        // `MakerNoteSafety`, `ProfileEmbedPolicy`, `Saturation`,
        // `LightSource`, `CalibrationIlluminant1/2/3`). Each of those
        // reassigns `raw` to the tag's own numeric code string on success
        // (e.g. `"Chunky"` -> `"1"`) without an early `return`, so by the
        // time this arm would run, `raw` is already the STORED code, not a
        // label -- looking `"1"` up in `PlanarConfiguration`'s own table
        // (labels `"Chunky"`/`"Planar"`) would find no match and wrongly
        // refuse an already-correct conversion.
        // `exif_or_gps_module_for` (PR #959 finding `4111785371`) additionally
        // requires the descriptor's OWN table to be Exif::Main/GPS::Main
        // before this generic dispatch runs at all: a leaf-name match alone
        // is not enough, since a MakerNotes tag can declare the same leaf
        // name as an Exif::Main/GPS::Main tag while being a different tag at
        // a different id with (if any) its own unrelated PrintConv. When the
        // guard fails, `raw` falls through unchanged to the plain declared-
        // type parser below, which refuses a label it cannot parse as an
        // integer -- exactly the outcome the pinned oracle gives for such a
        // tag today.
        Some(ValueType::Integer)
            if !raw_mode
                && !matches!(
                    declared_tag_name.rsplit(':').next(),
                    Some(
                        "ColorSpace"
                            | "Contrast"
                            | "CustomRendered"
                            | "GPSDifferential"
                            | "GPSStatus"
                            | "GPSMeasureMode"
                            | "GPSDestDistanceRef"
                            | "PlanarConfiguration"
                            | "MakerNoteSafety"
                            | "ProfileEmbedPolicy"
                            | "Saturation"
                            | "LightSource"
                            | "CalibrationIlluminant1"
                            | "CalibrationIlluminant2"
                            | "CalibrationIlluminant3"
                            | "FocalLengthIn35mmFormat"
                    )
                )
                && exif_or_gps_module_for(declared_tag_name).is_some() =>
        {
            let leaf = declared_tag_name
                .rsplit(':')
                .next()
                .unwrap_or(declared_tag_name);
            let module = exif_or_gps_module_for(declared_tag_name)
                .expect("guarded above: exif_or_gps_module_for(...).is_some()");
            match invert_enum_printconv(module, leaf, declared_tag_name, raw) {
                Some(Ok(code)) => Ok(TagValue::Integer(code)),
                Some(Err(EnumInverseError::Ambiguous)) => Err(invalid(
                    tag_name,
                    format!(
                        "Can't convert {} (matches more than one PrintConv)",
                        display_tag_for_message(tag_name, leaf)
                    ),
                )),
                Some(Err(EnumInverseError::NoMatch)) => Err(invalid(
                    tag_name,
                    format!(
                        "Can't convert {} (not in PrintConv)",
                        display_tag_for_message(tag_name, leaf)
                    ),
                )),
                None => Ok(TagValue::Integer(parse_integer(tag_name, raw)?)),
            }
        }
        // Codex review finding on PR #963 (comment 4112327507): Exif.pm
        // 13.59 0xa405 declares FocalLengthIn35mmFormat `int16u`. Stripping
        // its ` mm` suffix let a value outside 0..=65535 reach the generic
        // integer parser below, which has no notion of the tag's own width
        // -- confirmed against the oracle: `-ExifIFD:FocalLengthIn35mmFormat=70000
        // mm` and `=-1 mm` both refuse (`Value above/below int16u
        // maximum/minimum`) before the value is ever stored.
        Some(ValueType::Integer)
            if declared_tag_name
                .rsplit(':')
                .next()
                .is_some_and(|leaf| leaf.eq_ignore_ascii_case("FocalLengthIn35mmFormat")) =>
        {
            let value = parse_integer(tag_name, raw)?;
            if !(0..=u16::MAX as i64).contains(&value) {
                return Err(invalid(
                    tag_name,
                    "FocalLengthIn35mmFormat does not fit unsigned 16-bit (int16u)",
                ));
            }
            Ok(TagValue::new_integer(value))
        }
        Some(ValueType::Integer) => Ok(TagValue::Integer(parse_integer(tag_name, raw)?)),
        Some(ValueType::Float) => Ok(TagValue::Float(parse_float(tag_name, raw)?)),
        // Raw mode skips these PrintConvInvs (Codex pre-review of PR #959,
        // round 5), each confirmed against pinned 13.59: SubjectDistance's
        // `s/\s*m$//` and FocalLength's `s/\s*mm$//` (`#=5 m`, `#=50 mm`:
        // `Not a floating point number`), and ShutterSpeedValue's
        // fraction-accepting inverse, whose ValueConvInv then needs a
        // number (`#=1/250`: `Argument "1/250" isn't numeric ...`).
        Some(ValueType::Rational)
            if raw_mode
                && matches!(
                    declared_tag_name.rsplit(':').next(),
                    Some("SubjectDistance" | "FocalLength")
                )
                && raw.trim_end().ends_with('m') =>
        {
            Err(invalid(tag_name, "Not a floating point number"))
        }
        Some(ValueType::Rational)
            if raw_mode
                && declared_tag_name.rsplit(':').next() == Some("ShutterSpeedValue")
                && as_fraction(raw).is_some() =>
        {
            Err(invalid(tag_name, "Not a floating point number"))
        }
        Some(ValueType::Rational)
            if declared_tag_name.rsplit(':').next() == Some("ShutterSpeedValue") =>
        {
            parse_shutter_speed_value(tag_name, raw)
        }
        Some(ValueType::Rational) => parse_rational(declared_tag_name, raw, raw_mode),
        // A raw PDF date skips the display-date inverse too (Codex
        // pre-review of PR #959, round 5): pinned 13.59 turns
        // `-PDF:CreateDate#=2020-01-02T03:04:05` into
        // `D:2020-01-02T030405`, while `parse_pdf_date` normalises it to
        // `D:20200102030405`. Only the canonical `YYYY:mm:dd HH:MM:SS`,
        // optionally with a `+HH:MM`/`-HH:MM` zone, is stored identically by
        // both (confirmed on t/images/PDF.pdf); any other raw spelling is
        // refused.
        Some(ValueType::DateTime)
            if raw_mode
                && declared_tag_name.starts_with("PDF:")
                && !is_canonical_exif_datetime(raw)
                && !raw
                    .get(..19)
                    .zip(raw.get(19..))
                    .is_some_and(|(date, zone)| {
                        is_canonical_exif_datetime(date)
                            && zone.len() == 6
                            && zone.bytes().enumerate().all(|(at, byte)| match at {
                                0 => byte == b'+' || byte == b'-',
                                3 => byte == b':',
                                _ => byte.is_ascii_digit(),
                            })
                    }) =>
        {
            Err(invalid(
                tag_name,
                "oxidex takes a raw PDF date only as YYYY:mm:dd HH:MM:SS[+-HH:MM]",
            ))
        }
        Some(ValueType::DateTime) if declared_tag_name.starts_with("PDF:") => {
            parse_pdf_date(tag_name, raw)
        }
        // `parse_datetime` is ExifTool's `InverseDateTime`, the PrintConvInv
        // raw mode skips: pinned 13.59 stores `-ExifIFD:DateTimeOriginal#=
        // 2020-01-02 03:04:05` and `#=2020:01:02 03:04:05+02:00` verbatim.
        // A `DateTime` value cannot carry either spelling, so raw mode takes
        // only the canonical `YYYY:mm:dd HH:MM:SS` (stored identically) and
        // refuses the rest (Codex pre-review of PR #959, round 5).
        Some(ValueType::DateTime) if raw_mode && !is_canonical_exif_datetime(raw) => Err(invalid(
            tag_name,
            "oxidex takes a raw date/time only as YYYY:mm:dd HH:MM:SS",
        )),
        Some(ValueType::DateTime) => parse_datetime(tag_name, raw),
        // ExifTool's `undef` format imposes no shape on the value and stores
        // the argument's bytes verbatim (`Writer.pl:6847-6858`).
        Some(ValueType::Binary) => Ok(TagValue::Binary(raw.as_bytes().to_vec())),
        // There is no command-line syntax for a nested structure, and the EXIF
        // serializer rejects `TagValue::Struct` outright
        // (`writers::exif_surgical::tag_value_to_field`).
        Some(ValueType::Struct) => Err(invalid(
            tag_name,
            "Structured values cannot be set from the command line",
        )),
    }
}

/// Whether GPS.pm 13.59's GPSDateStamp `PrintConvInv` would adjust `raw` to
/// UTC before taking its date: `$val =~ /[-+]/ and ($secs =
/// GetUnixTime($val, 1))`. `GetUnixTime` (ExifTool.pm 13.59) parses only
/// `^(\d+)[-:](\d+)[-:](\d+)\s+(\d+):(\d+):(\d+)` -- a full date/time --
/// and treats a time without a zone as local, so the result depends on the
/// zone either way; any other value (`2024-01-02`, `2024-01-02T10:11:12`)
/// keeps its hyphens as separators and goes straight to the date scan.
fn gps_date_stamp_adjusts_to_utc(raw: &str) -> bool {
    if !raw.contains(['-', '+']) {
        return false;
    }
    let bytes = raw.as_bytes();
    let mut at = 0;
    let digits = |at: &mut usize| {
        let start = *at;
        while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
        *at > start
    };
    let sep = |at: &mut usize, allowed: &[u8]| {
        let ok = bytes.get(*at).is_some_and(|b| allowed.contains(b));
        *at += usize::from(ok);
        ok
    };
    let date = digits(&mut at)
        && sep(&mut at, b"-:")
        && digits(&mut at)
        && sep(&mut at, b"-:")
        && digits(&mut at);
    if !date {
        return false;
    }
    let space_start = at;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    at > space_start
        && digits(&mut at)
        && sep(&mut at, b":")
        && digits(&mut at)
        && sep(&mut at, b":")
        && digits(&mut at)
}

/// Case-insensitively matches `leaf` against one of the four Exif.pm tags
/// this fix's `PrintConvInv` covers, returning the canonically-cased name
/// used elsewhere in this file (`strip_printconv_unit_suffix`'s match arms,
/// `parse_rational`'s `SubjectDistance` branch) -- ExifTool tag and group
/// names are case-insensitive (confirmed against the oracle:
/// `-ExifIFD:focallength=`, `-exififd:FocalLength=`, `-SUBJECTDISTANCE=`
/// all behave exactly like the canonical spelling), so an exact-case `match`
/// on the leaf alone silently missed every non-canonical spelling.
fn unit_suffix_tag(leaf: &str) -> Option<&'static str> {
    const TAGS: [&str; 4] = [
        "FocalLength",
        "FocalLengthIn35mmFormat",
        "SubjectDistance",
        "AmbientTemperature",
    ];
    TAGS.into_iter().find(|tag| leaf.eq_ignore_ascii_case(tag))
}

/// If `tag_name`'s leaf (after the last `:`) case-insensitively names one of
/// the four tags [`unit_suffix_tag`] covers, returns the same tag with the
/// leaf canonically cased and, when the group prefix itself case-
/// insensitively names a group this file already keys on, that canonicalized
/// too -- so a lookup keyed on exact case (`get_tag_descriptor`, this file's
/// own `declared_tag_name.rsplit(':').next()` matches) sees the same
/// spelling it would for the canonical form. A bare name becomes
/// `EXIF:<Leaf>`, the key the registry holds. `None` when the leaf does not
/// match, or the tag has a group prefix this list does not recognise (kept
/// unchanged rather than guessed -- a group this function does not know is
/// left for the caller's existing case-sensitive handling, unaffected by
/// this fix, exactly as before).
fn canonicalize_unit_suffix_tag_name(tag_name: &str) -> Option<String> {
    const KNOWN_GROUPS: [&str; 6] = ["EXIF", "ExifIFD", "GPS", "IFD0", "IFD1", "InteropIFD"];
    let (group, leaf) = match tag_name.rsplit_once(':') {
        Some((group, leaf)) => (Some(group), leaf),
        None => (None, tag_name),
    };
    let canonical_leaf = unit_suffix_tag(leaf)?;
    match group {
        // A bare name resolves to the `EXIF:` descriptor, exactly as the
        // exact-case alias table above maps `FocalLength` to
        // `EXIF:FocalLength`: the registry holds no bare key, so returning
        // the bare canonical leaf left `-focallength=50 mm` untyped and the
        // writer refused it as a String/Rational mismatch, where the pinned
        // oracle writes it (local Codex pre-review of PR #963).
        None => Some(format!("EXIF:{canonical_leaf}")),
        Some(group) => {
            let canonical_group = KNOWN_GROUPS
                .into_iter()
                .find(|candidate| group.eq_ignore_ascii_case(candidate))?;
            Some(format!("{canonical_group}:{canonical_leaf}"))
        }
    }
}

/// Reproduces the PrintConvInv these three EXIF tags declare for stripping
/// the literal unit their PrintConv appends, so a print-converted CLI value
/// (as `-j` or the default text output would print it) round-trips back
/// into a write. Byte-for-byte from the pinned Exif.pm (13.59):
///
/// | Tag | PrintConv | PrintConvInv | Exif.pm |
/// |---|---|---|---|
/// | FocalLength (0x920a) | `sprintf("%.1f mm",$val)` | `$val=~s/\s*mm$//;$val` | :2425-2426 |
/// | FocalLengthIn35mmFormat (0xa405) | `"$val mm"` | `$val=~s/\s*mm$//;$val` | :2896-2897 |
/// | AmbientTemperature (0x9400) | `"$val C"` | `$val=~s/ ?C//; $val` | :2590-2591 |
///
/// `leaf` must be one of the three names above; the caller (`parse_cli_tag_value`)
/// only reaches this for those. `SubjectDistance` (0x9206, ` m`) has the same
/// shape but is handled inline in `parse_rational`, where its stripped value
/// already needs to reach the ApertureValue/fraction/`inf`/`undef` cases that
/// follow it.
fn strip_printconv_unit_suffix<'a>(leaf: &str, raw: &'a str) -> Cow<'a, str> {
    match leaf {
        "FocalLength" | "FocalLengthIn35mmFormat" => {
            Cow::Borrowed(raw.strip_suffix("mm").map(str::trim_end).unwrap_or(raw))
        }
        "AmbientTemperature" => match raw.find('C') {
            // `s/ ?C//` carries neither a `$` anchor nor `/g`: only the
            // first "C" is removed, together with a single preceding space
            // if there is one. PrintConv always places the unit last
            // ("20 C"), so this only diverges from an end-anchored strip
            // when the caller's own value already contains an unrelated
            // "C" earlier -- in which case pinned ExifTool strips that
            // occurrence too, not the trailing unit, and this matches it.
            Some(index) => {
                let before = raw[..index].strip_suffix(' ').unwrap_or(&raw[..index]);
                let mut owned = String::with_capacity(raw.len().saturating_sub(1));
                owned.push_str(before);
                owned.push_str(&raw[index + 1..]);
                Cow::Owned(owned)
            }
            None => Cow::Borrowed(raw),
        },
        _ => Cow::Borrowed(raw),
    }
}

fn invert_subsec_time(value: &str) -> Option<&str> {
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(value);
    }
    let (_, fraction) = value.split_once('.')?;
    let end = fraction.bytes().take_while(u8::is_ascii_digit).count();
    (end > 0).then_some(&fraction[..end])
}

/// `GPS:GPSStatus`'s own (code, label) pairs (GPS.pm 0x0009), for
/// [`unique_case_insensitive_prefix_match`].
const GPS_STATUS_ENTRIES: &[(&str, &str)] =
    &[("A", "Measurement Active"), ("V", "Measurement Void")];

/// `GPS:GPSMeasureMode`'s own (code, label) pairs (GPS.pm 0x000a), for
/// [`unique_case_insensitive_prefix_match`].
const GPS_MEASURE_MODE_ENTRIES: &[(&str, &str)] = &[
    ("2", "2-Dimensional Measurement"),
    ("3", "3-Dimensional Measurement"),
];

/// `GPS:GPSDestDistanceRef`'s own (code, label) pairs (GPS.pm 0x0019), for
/// [`unique_case_insensitive_prefix_match`].
const GPS_DEST_DISTANCE_REF_ENTRIES: &[(&str, &str)] =
    &[("K", "Kilometers"), ("M", "Miles"), ("N", "Nautical Miles")];

/// One additional tier of ExifTool's `ReverseLookup` (`Writer.pl:3609`):
/// case-insensitive PREFIX matching, tried after exact/case-insensitive and
/// before case-insensitive SUBSTRING (which this file does not implement
/// anywhere, per the module doc comment's disclosed simplification). Scoped
/// to the three small, hand-enumerated GPS string arms that need it -- see
/// their call sites for why implementing this one extra tier there is exact
/// rather than a guess. Returns the matched entry's own first element (the
/// stored code) only when exactly one entry's label starts with `raw`
/// case-insensitively; `raw` empty or with more than one or zero matches
/// returns `None`.
fn unique_case_insensitive_prefix_match<'a>(
    entries: &[(&'a str, &'a str)],
    raw: &str,
) -> Option<&'a str> {
    if raw.is_empty() {
        return None;
    }
    let mut matches = entries.iter().filter(|(_, label)| {
        label
            .get(..raw.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(raw))
    });
    match (matches.next(), matches.next()) {
        (Some(&(code, _)), None) => Some(code),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Generic inverse PrintConv, sourced from the transcribed IFD tag tables
// ---------------------------------------------------------------------------

/// Exif.pm 13.59 `%lightSource` (Exif.pm:175 ff.), shared verbatim by
/// `LightSource` (0x9208) and DNG's `CalibrationIlluminant1/2/3`
/// (0xc65a/0xc65b/0xcd31, `SeparateTable => 'LightSource'`). Deliberately
/// hand-transcribed rather than read from the generated table: the real
/// hash also maps code 25 to `"Daylight"` (a duplicate of code 1), which
/// `invert_int_enum`'s "exactly one match" rule would refuse as ambiguous.
/// ExifTool's own tie-break (`ReverseLookup`'s `sort keys %$conv`, ascending
/// as strings) picks the lowest key, code 1 -- reproduced here simply by
/// never encoding the code-25 entry, so this table has no duplicate label at
/// all and every lookup (case-insensitive included) is unambiguous by
/// construction.
const LIGHT_SOURCE_LABELS: &[(i64, &str)] = &[
    (0, "Unknown"),
    (1, "Daylight"),
    (2, "Fluorescent"),
    (3, "Tungsten (Incandescent)"),
    (4, "Flash"),
    (9, "Fine Weather"),
    (10, "Cloudy"),
    (11, "Shade"),
    (12, "Daylight Fluorescent"),
    (13, "Day White Fluorescent"),
    (14, "Cool White Fluorescent"),
    (15, "White Fluorescent"),
    (16, "Warm White Fluorescent"),
    (17, "Standard Light A"),
    (18, "Standard Light B"),
    (19, "Standard Light C"),
    (20, "D55"),
    (21, "D65"),
    (22, "D75"),
    (23, "D50"),
    (24, "ISO Studio Tungsten"),
    (26, "Day White"),
    (27, "Cool White"),
    (28, "White"),
    (29, "Warm White"),
    (30, "Daylight LED"),
    (31, "Day White LED"),
    (32, "Cool White LED"),
    (33, "White LED"),
    (34, "Warm White LED"),
    (255, "Other"),
];

/// Why [`invert_enum_printconv`] refused to resolve a label.
enum EnumInverseError {
    /// No table entry's label matched, exactly or case-insensitively.
    NoMatch,
    /// More than one table entry's label matched at the same tier (exact,
    /// or case-insensitive when no exact match existed). ExifTool's own
    /// `ReverseLookup` (`Writer.pl:3609-3665`) breaks such a tie by picking
    /// the entry whose numeric key sorts first as a string; this port
    /// refuses instead, per `AGENTS.md`'s "never approximate a conversion"
    /// -- a duplicate label such as `Compression`'s `7`/`99` both printing
    /// `"JPEG"` is a real ExifTool ambiguity, not a transcription gap, and
    /// picking a winner would be a guess wearing a citation.
    Ambiguous,
}

/// Ports ExifTool's `ReverseLookup` (`Writer.pl:3609-3665`) for a plain enum
/// `PrintConv` hash: `raw` matched against the table's labels exactly, then
/// case-insensitively, and only when exactly one entry matches either tier.
fn invert_int_enum(
    entries: &[(i64, &str)],
    raw: &str,
) -> std::result::Result<i64, EnumInverseError> {
    let mut exact = entries.iter().filter(|(_, label)| *label == raw);
    if let Some(&(value, _)) = exact.next() {
        return if exact.next().is_none() {
            Ok(value)
        } else {
            Err(EnumInverseError::Ambiguous)
        };
    }
    let mut insensitive = entries
        .iter()
        .filter(|(_, label)| label.eq_ignore_ascii_case(raw));
    match (insensitive.next(), insensitive.next()) {
        (Some(&(value, _)), None) => Ok(value),
        (Some(_), Some(_)) => Err(EnumInverseError::Ambiguous),
        (None, _) => Err(EnumInverseError::NoMatch),
    }
}

/// Whether `declared_tag_name` may be inverted against `Exif::Main`
/// (`FormatFamily::EXIF`) or `GPS::Main` (`FormatFamily::GPS`) -- the only two
/// tables this file's enum PrintConv inversion draws from (the leaf-name
/// dispatch above `invert_table_printconv_label`, the
/// `LightSource`/`CalibrationIlluminant1/2/3` arm, and the generic fallback
/// dispatch). Every one of those matches by LEAF name only
/// (`declared_tag_name.rsplit(':').next()`), which is not enough on its own:
/// MakerNotes tables reuse EXIF's own leaf names for entirely different tags
/// at different ids with (if any) their own PrintConv -- `Sony:ExposureMode`
/// (Sony.pm 0x0119, `FormatFamily::MakerNotes`) shares its leaf with Exif.pm's
/// `ExposureMode` (0xa402), so a bare leaf match sent `-Sony:ExposureMode=Auto`
/// through `Exif::Main`'s inversion and would have silently stored the wrong
/// code under the wrong tag's meaning (confirmed against the oracle: pinned
/// ExifTool 13.59 refuses that value entirely, since Sony's tag has no such
/// label).
///
/// The candidate module is derived from `declared_tag_name`'s own group
/// prefix -- `"GPS"` for a `GPS:` tag, `"Exif"` for a bare name or one of the
/// EXIF-family IFD spellings this file and its callers use
/// (`EXIF:`/`ExifIFD:`/`IFD0:`/`IFD1:`/`InteropIFD:`), and NO candidate at
/// all for any other explicit group. A first draft of this function trusted
/// `"Exif"` for literally anything that did not start with `"GPS:"`, which
/// correctly covered `Sony:ExposureMode` (caught by the registry veto below,
/// since Sony.pm's tag has a `FormatFamily::MakerNotes` descriptor) but not
/// `DICOM:SceneType` (Codex PR #959 round 4): DICOM tags carry no
/// `FormatFamily` descriptor at all (`yaml_format_info` does not recognize
/// the `DICOM` prefix), so with no group check at all, the ABSENT-entry
/// branch below would have trusted the bare `"Exif"` guess and inverted
/// `DICOM:SceneType`'s value against `Exif::Main`'s own `SceneType`
/// (0xa301) -- a real DICOM field with an EXIF label silently misread as an
/// EXIF tag. Requiring a recognizable EXIF-family group before granting
/// `"Exif"` by default closes that hole without needing a registry entry for
/// every DICOM/XMP/IPTC/etc. tag this file has never heard of.
///
/// Once a candidate exists, this function's remaining job is only to VETO it
/// when the tag registry has positive evidence it is wrong. `get_tag_descriptor`
/// is a hand + generated registry that does not cover every transcribed
/// `Exif::Main` row (`EXIF:ShadingCorrection`/`EXIF:NoiseReduction`, confirmed
/// absent by a registry audit, are genuine Exif::Main tags with nobody's
/// descriptor at all); refusing whenever the registry has NO entry would
/// silently regress those already-fixed tags back to accepting an unmatched
/// value. So `None` (no registry entry) trusts the candidate, while `Some`
/// VERIFIES it -- vetoing only a descriptor that names a genuinely different
/// table (`FormatFamily::MakerNotes` etc.), which is exactly the evidence
/// `Sony:ExposureMode` supplies and `ShadingCorrection` does not. Every
/// caller treats a veto (`None` from this function, whether from the group
/// check or the registry check) the same way: skip this file's Exif/GPS-table
/// inversion and let `raw` fall through to the plain declared-type parser,
/// which naturally refuses a label it cannot parse -- "refuse" rather than
/// "guess", per this codebase's rule against approximating a conversion.
fn exif_or_gps_module_for(declared_tag_name: &str) -> Option<&'static str> {
    let candidate = match declared_tag_name.split_once(':') {
        None => Some("Exif"),
        Some(("GPS", _)) => Some("GPS"),
        Some(("EXIF" | "ExifIFD" | "IFD0" | "IFD1" | "InteropIFD", _)) => Some("Exif"),
        Some(_) => None,
    }?;
    match get_tag_descriptor(declared_tag_name).map(|descriptor| descriptor.format()) {
        None => Some(candidate),
        Some(FormatFamily::EXIF) if candidate == "Exif" => Some("Exif"),
        Some(FormatFamily::GPS) if candidate == "GPS" => Some("GPS"),
        Some(_) => None,
    }
}

/// The declared type this file should treat `declared_tag_name` as, sourced
/// directly from the transcribed `Exif::Main`/`GPS::Main` row rather than the
/// tag registry, for a tag [`exif_or_gps_module_for`] confirms belongs to one
/// of those tables but the registry has no descriptor for at all (Codex PR
/// #959 round 4: `EXIF:ShadingCorrection`/`EXIF:NoiseReduction`, confirmed
/// absent from `get_tag_descriptor` by the round-2 audit, are writable
/// `int16u` enum rows with no registry entry whatsoever). Without this,
/// `parse_cli_tag_value("EXIF:ShadingCorrection", "Yes")` took the
/// `declared == None` branch at the top of the type match below and returned
/// `TagValue::String("Yes")` -- the generic enum-inversion arm further down
/// is gated on `Some(ValueType::Integer)` and simply never ran, no matter how
/// correct its own logic was, because nothing ever produced that `Some`.
///
/// Deliberately narrow: only a plain writable [`crate::exiftool_tables::PrintConv::IntEnum`]
/// row reports `Some(ValueType::Integer)` here, because that is the only
/// shape the rest of this function knows how to invert generically; any
/// other transcribed row (a different `PrintConv`, or none) returns `None`
/// and the caller's `None | Some(ValueType::String)` arm still applies,
/// exactly as before this fix -- this only ADDS coverage for the specific
/// gap Codex found, never changes behavior for a tag the registry already
/// describes.
fn declared_value_type_from_transcribed_table(declared_tag_name: &str) -> Option<ValueType> {
    let tag = transcribed_row(declared_tag_name).filter(|tag| tag.writable.is_some())?;
    match tag.print_conv {
        crate::exiftool_tables::PrintConv::IntEnum(_) => Some(ValueType::Integer),
        _ => None,
    }
}

/// Looks up `leaf`'s `PrintConv` in the transcribed `(module, "Main")` IFD
/// table (`docs/TRANSCRIPTION.md`) and inverts it with [`invert_int_enum`].
///
/// Returns `None` when the table has no tag named `leaf`, or that tag's
/// `PrintConv` is not a plain [`crate::exiftool_tables::PrintConv::IntEnum`]
/// -- the caller then falls back to whatever handling it already has (a
/// `PrintHex` hash like `Flash`'s, or a computed inverse like `Saturation`'s
/// `ConvertParameter`, are not reachable through this generic path, and are
/// not meant to be: they are not "look the label up in a table").
/// `declared_tag_name` resolves the ROW: `Exif::Main` carries duplicate
/// names at different ids (`SensingMethod`'s first row by id maps `1` to
/// `"Monochrome area"`; the writable `0xa217` row this tag actually is maps
/// `1` to `"Not defined"` -- picking by name alone silently picked the wrong
/// row's table, accepting `"Monochrome area"` as code 1 and rejecting
/// `"Not defined"`, the label the writable tag's own PrintConv actually
/// uses). The registry's own numeric id (`get_tag_descriptor`, e.g.
/// `TagId::Numeric(0xa217)` for `"EXIF:SensingMethod"`) is resolved first and
/// looked up with `IfdTable::tag`'s id-keyed binary search, which cannot
/// return the wrong same-named row because ids in `tags` are unique; a
/// name-only `.find` is the fallback only when the registry has no numeric
/// id for `declared_tag_name` at all (an id lookup that hits a
/// DIFFERENTLY-named row, which should not happen, also falls back rather
/// than trusting a mismatch).
fn invert_enum_printconv(
    module: &str,
    leaf: &str,
    declared_tag_name: &str,
    raw: &str,
) -> Option<std::result::Result<i64, EnumInverseError>> {
    // The destination-selected row first (PR #959, `4112816750`): the
    // registry id names `ChromaticAberrationCorrection`'s Sony SubIFD row
    // `0x7034` (`Off`/`Auto`), so preferring it refused
    // `-ExifIFD:ChromaticAberrationCorrection=Yes`, which pinned 13.59 writes
    // to the ExifIFD row `0xa410` (`No`/`Yes`), and accepted `Auto`, which it
    // refuses.
    let destination_row = (module == "Exif")
        .then(|| crate::writers::write_request::exif_main_row_for_destination(declared_tag_name))
        .flatten()
        .filter(|tag| tag.name == leaf);
    let table = crate::exiftool_tables::find_ifd_table(module, "Main")?;
    let by_id = get_tag_descriptor(declared_tag_name).and_then(|descriptor| {
        let crate::core::TagId::Numeric(id) = descriptor.id() else {
            return None;
        };
        let tag = table.tag(*id)?;
        (tag.name == leaf).then_some(tag)
    });
    let print_conv = destination_row
        .or(by_id)
        .or_else(|| table.tags.iter().find(|tag| tag.name == leaf))
        .map(|tag| tag.print_conv)?;
    match print_conv {
        crate::exiftool_tables::PrintConv::IntEnum(entries) => Some(invert_int_enum(entries, raw)),
        _ => None,
    }
}

/// Resolves `raw` as `leaf`'s PrintConv label using the transcribed table
/// ([`invert_enum_printconv`]) and turns the result into this function's
/// `Result<TagValue>`, for use directly as a `return` from inside the
/// `declared_tag_name` leaf-dispatch match in [`parse_cli_tag_value`].
fn invert_table_printconv_label(
    tag_name: &str,
    declared_tag_name: &str,
    leaf: &str,
    raw: &str,
) -> Result<TagValue> {
    let module = if declared_tag_name.starts_with("GPS:") {
        "GPS"
    } else {
        "Exif"
    };
    match invert_enum_printconv(module, leaf, declared_tag_name, raw) {
        Some(Ok(code)) => Ok(TagValue::Integer(code)),
        Some(Err(EnumInverseError::Ambiguous)) => Err(invalid(
            tag_name,
            format!(
                "Can't convert {} (matches more than one PrintConv)",
                display_tag_for_message(tag_name, leaf)
            ),
        )),
        Some(Err(EnumInverseError::NoMatch)) | None => Err(invalid(
            tag_name,
            format!(
                "Can't convert {} (not in PrintConv)",
                display_tag_for_message(tag_name, leaf)
            ),
        )),
    }
}

/// The `Group:Tag` spelling ExifTool's own `Can't convert Group:Tag (...)`
/// diagnostic uses. `Exif::Main` is one shared table for IFD0 and ExifIFD
/// (`ifd_schema.rs`: which directory a tag lands in is only known once a
/// real file is read, never stored statically), so when the caller's own
/// `tag_name` already carries an explicit group (`-IFD0:Orientation=...`),
/// that group is trusted outright. For a bare name (`-Orientation=...`) this
/// falls back to the group each of these tags conventionally occupies in a
/// JPEG/TIFF -- the same convention `docs/.../breadth_measure.py`'s
/// `fallback_group` uses to grade this fix's own writes against the oracle.
fn display_tag_for_message(tag_name: &str, leaf: &str) -> String {
    match tag_name.rsplit_once(':') {
        Some((group, _)) if !group.is_empty() => format!("{group}:{leaf}"),
        _ => format!("{}:{leaf}", conventional_group_for_leaf(leaf)),
    }
}

/// See [`display_tag_for_message`]. Only the tags this fix's PrintConv
/// inversion covers need an entry; everything else defaults to `ExifIFD`,
/// which is at worst a cosmetically wrong group in a diagnostic string, not
/// a wrong value written -- `AGENTS.md`'s "never approximate a conversion"
/// is about the stored bytes, which this never touches.
fn conventional_group_for_leaf(leaf: &str) -> &'static str {
    match leaf {
        "Orientation"
        | "ResolutionUnit"
        | "Compression"
        | "YCbCrPositioning"
        | "GrayResponseUnit"
        | "CalibrationIlluminant1"
        | "CalibrationIlluminant2"
        | "CalibrationIlluminant3"
        | "PlanarConfiguration" => "IFD0",
        "GPSStatus" | "GPSMeasureMode" | "GPSDestDistanceRef" | "GPSDifferential"
        | "GPSLatitudeRef" | "GPSDestLatitudeRef" => "GPS",
        _ => "ExifIFD",
    }
}

/// ExifTool 13.59 `Image::ExifTool::Exif::ConvertParameter`, as used by
/// `ExifIFD:Contrast` (Exif.pm 0xa408).
fn invert_exif_contrast_parameter(raw: &str) -> Option<&'static str> {
    let is_word = |initial: u8| {
        let bytes = raw.as_bytes();
        bytes.iter().enumerate().any(|(index, byte)| {
            (index == 0 || !is_ascii_word(bytes[index - 1])) && byte.eq_ignore_ascii_case(&initial)
        })
    };
    let numeric = as_float_text(raw).and_then(|value| value.parse::<f64>().ok());

    if is_word(b'n') || numeric == Some(0.0) {
        Some("0")
    } else if is_word(b's') || is_word(b'l') || numeric.is_some_and(|value| value < 0.0) {
        Some("1")
    } else if is_word(b'h') || numeric.is_some() {
        Some("2")
    } else {
        None
    }
}

fn is_ascii_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Inverts Exif.pm 0x9101's four-component PrintConv into stored bytes.
fn parse_components_configuration(tag_name: &str, raw: &str) -> Result<TagValue> {
    use regex::Regex;
    use std::sync::OnceLock;

    let mut tokens = raw
        .split_whitespace()
        .map(|token| token.trim_matches(','))
        .collect::<Vec<_>>();
    let compact;
    if tokens.len() == 1 {
        static COMPONENT: OnceLock<Regex> = OnceLock::new();
        let component =
            COMPONENT.get_or_init(|| Regex::new(r"Y|Cb|Cr|R|G|B").expect("valid regex"));
        compact = component
            .find_iter(tokens[0])
            .map(|value| value.as_str())
            .collect();
        tokens = compact;
    }
    if tokens.len() > 4 {
        return Err(invalid(tag_name, "Too many values specified (4 required)"));
    }
    let mut bytes = Vec::with_capacity(4);
    for token in tokens {
        bytes.push(match token.to_ascii_lowercase().as_str() {
            "-" => 0,
            "y" => 1,
            "cb" => 2,
            "cr" => 3,
            "r" => 4,
            "g" => 5,
            "b" => 6,
            _ => {
                return Err(invalid(
                    tag_name,
                    "Can't convert ComponentsConfiguration value (not in PrintConv)",
                ));
            }
        });
    }
    bytes.resize(4, 0);
    Ok(TagValue::Binary(bytes))
}

/// `raw_mode` counterpart of [`parse_components_configuration`]: the caller
/// already supplied the four raw byte codes (whitespace/comma separated,
/// like `"1 2 3 0"`), not labels to look up -- `PrintConvInv` is skipped
/// entirely, matching the oracle's own `#`/`-n` behavior for this tag.
///
/// Pinned 13.59's raw `int8u[4]` write (`WriteValue`/`CheckValue`) takes
/// exactly four whitespace-separated decimal integers in 0..=255 and
/// refuses anything else (Codex pre-review of PR #959, round 5): `#=1 2` is
/// `Not enough values specified (4 required)`, `#=1.5 2 3 0` is `Not an
/// integer`, `#=1,2,3,0` is one value, `#=0x1 2 3 0` is an invalid value and
/// `#=256 0 0 0` is `Value above int8u maximum`. This used to pad a short
/// list with zeros and round a fraction.
fn parse_components_configuration_raw(tag_name: &str, raw: &str) -> Result<TagValue> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    match tokens.len() {
        4 => {}
        n if n < 4 => {
            return Err(invalid(
                tag_name,
                "Not enough values specified (4 required)",
            ));
        }
        _ => return Err(invalid(tag_name, "Too many values specified (4 required)")),
    }
    tokens
        .into_iter()
        .map(|token| {
            if !is_int(token) {
                return Err(invalid(tag_name, "Not an integer"));
            }
            match token.parse::<i64>() {
                Ok(value @ 0..=255) => Ok(value as u8),
                Ok(value) if value < 0 => Err(invalid(tag_name, "Value below int8u minimum")),
                _ => Err(invalid(tag_name, "Value above int8u maximum")),
            }
        })
        .collect::<Result<Vec<u8>>>()
        .map(TagValue::Binary)
}

/// FileSource's raw write (Exif.pm 0xa300 `ValueConvInv => '($val=~/^\d+$/
/// and $val < 256) ? chr($val) : $val'`): a decimal integer below 256 is its
/// one byte, anything else is stored verbatim as the `undef` bytes. Pinned
/// 13.59 stores `#=1.5`, `#=abc`, `#=300`, `#=-1` and `#=0x3` as their own
/// text; this used to round `1.5` to 2 and refuse the rest (Codex pre-review
/// of PR #959, round 5).
fn file_source_raw(raw: &str) -> TagValue {
    let small = (!raw.is_empty() && raw.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| raw.parse::<u64>().ok())
        .flatten()
        .filter(|value| *value < 256);
    TagValue::Binary(match small {
        Some(value) => vec![value as u8],
        None => raw.as_bytes().to_vec(),
    })
}

/// SceneType's raw write (Exif.pm 0xa301 `ValueConvInv => 'chr($val &
/// 0xff)'`) uses Perl's integer coercion before masking. Non-exponent
/// decimal strings retain their integer part exactly, including fixed
/// fractions, across Perl's signed IV / unsigned UV 64-bit ranges. Sending
/// these through f64 first loses low bits above 2^53 (PR959 local review).
/// Exponent strings use Perl's floating-point coercion; only finite values
/// below 2^53 are supported here. Larger exponent values and decimal integer
/// parts outside the IV/UV ranges refuse rather than guess at coercion.
fn scene_type_raw(tag_name: &str, raw: &str) -> Result<TagValue> {
    if !matches_float_shape(raw, b'.') {
        return Err(invalid(
            tag_name,
            format!("Argument \"{raw}\" isn't numeric (SceneType ValueConvInv)"),
        ));
    }
    let byte = if raw.contains(['e', 'E']) {
        let numeric = raw
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && value.abs() < 9_007_199_254_740_992.0)
            .ok_or_else(|| {
                invalid(
                    tag_name,
                    "SceneType exponent value exceeds the supported precision range (abs < 2^53)",
                )
            })?;
        (numeric.trunc() as i64 as u64 & 0xff) as u8
    } else {
        let negative = raw.starts_with('-');
        let unsigned = raw.strip_prefix(['+', '-']).unwrap_or(raw);
        let integer = unsigned
            .split_once('.')
            .map_or(unsigned, |(integer, _)| integer);
        let magnitude = if integer.is_empty() {
            Some(0)
        } else {
            integer.parse::<u64>().ok()
        }
        .filter(|value| !negative || *value <= (1u64 << 63))
        .ok_or_else(|| {
            invalid(
                tag_name,
                "SceneType decimal integer part exceeds the supported signed/unsigned 64-bit range",
            )
        })?;
        let byte = (magnitude & 0xff) as u8;
        if negative { byte.wrapping_neg() } else { byte }
    };
    Ok(TagValue::Binary(vec![byte]))
}

/// Exif.pm's `ConvertParameter` inverse conversion used by Sharpness (0xa40a).
///
/// `ConvertParameter` recognizes the first letter at a word boundary rather
/// than only accepting its three rendered labels: any normal/zero-like value
/// maps to 0, soft/low/negative to 1, and hard/high/positive to 2.
fn parse_sharpness(tag_name: &str, raw: &str) -> Result<TagValue> {
    let has_word_initial = |initials: &[u8]| {
        raw.as_bytes().iter().enumerate().any(|(index, byte)| {
            let at_word_boundary = index == 0
                || !raw.as_bytes()[index - 1].is_ascii_alphanumeric()
                    && raw.as_bytes()[index - 1] != b'_';
            at_word_boundary
                && byte.is_ascii_alphabetic()
                && initials.contains(&byte.to_ascii_lowercase())
        })
    };

    if has_word_initial(b"n") {
        return Ok(TagValue::Integer(0));
    }
    if has_word_initial(b"sl") {
        return Ok(TagValue::Integer(1));
    }
    if has_word_initial(b"h") {
        return Ok(TagValue::Integer(2));
    }
    if let Some(number) = as_float_text(raw).and_then(|value| value.parse::<f64>().ok()) {
        return Ok(TagValue::Integer(if number == 0.0 {
            0
        } else if number < 0.0 {
            1
        } else {
            2
        }));
    }

    Err(invalid(
        tag_name,
        "Can't convert Sharpness value (not a parameter)",
    ))
}

fn parse_subject_location(tag_name: &str, raw: &str) -> Result<TagValue> {
    let components = raw
        .split_whitespace()
        .map(|component| component.parse::<u16>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| {
            invalid(
                tag_name,
                "SubjectLocation requires two unsigned short values",
            )
        })?;
    if components.len() != 2 {
        return Err(invalid(
            tag_name,
            "SubjectLocation requires exactly two unsigned short values",
        ));
    }
    Ok(TagValue::Array(
        components
            .into_iter()
            .map(|component| TagValue::Integer(i64::from(component)))
            .collect(),
    ))
}

fn invert_gps_latitude_ref<'a>(tag_name: &str, raw: &'a str) -> Result<&'a str> {
    use regex::Regex;
    use std::sync::OnceLock;

    static CARDINAL: OnceLock<Regex> = OnceLock::new();
    static NUMBER: OnceLock<Regex> = OnceLock::new();
    let cardinal = CARDINAL.get_or_init(|| {
        Regex::new(r"(?i)(^|[^A-Z])([NS])(orth|outh)?\b").expect("valid GPS latitude regex")
    });
    if let Some(captures) = cardinal.captures(raw) {
        return Ok(if captures[2].eq_ignore_ascii_case("S") {
            "S"
        } else {
            "N"
        });
    }

    let number =
        NUMBER.get_or_init(|| Regex::new(r"([-+]?)\d+").expect("valid signed-number regex"));
    if let Some(captures) = number.captures(raw) {
        return Ok(if &captures[1] == "-" { "S" } else { "N" });
    }

    Err(invalid(
        tag_name,
        "GPSLatitudeRef must contain N/North, S/South, or a signed number",
    ))
}

fn invalid(tag_name: &str, reason: impl Into<String>) -> ExifToolError {
    ExifToolError::invalid_tag_value(tag_name, reason.into())
}

// ---------------------------------------------------------------------------
// Shape predicates — ports of ExifTool.pm:5924-5933
// ---------------------------------------------------------------------------

/// `IsInt` — `ExifTool.pm:5931`: `/^[+-]?\d+$/`
fn is_int(s: &str) -> bool {
    let bytes = s.as_bytes();
    let digits = match bytes.first() {
        Some(b'+') | Some(b'-') => &bytes[1..],
        _ => bytes,
    };
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

/// `IsHex` — `ExifTool.pm:5932`: `/^(0x)?[0-9a-f]{1,8}$/i`
///
/// Note this matches bare hex digits with no `0x` prefix, so ExifTool really
/// does store `-ISO=abc` as 0xabc = 2748. `IsInt` is tried first
/// (`Writer.pl:6875-6876`), so plain decimal never reaches this branch.
fn is_hex(s: &str) -> bool {
    let body = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    (1..=8).contains(&body.len()) && body.bytes().all(|b| b.is_ascii_hexdigit())
}

/// One half of `IsFloat` (`ExifTool.pm:5925`/`5927`), parameterised on the
/// decimal separator so the same routine serves both the `.` form and the
/// comma-locale form: `/^[+-]?(?=\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?$/`
fn matches_float_shape(s: &str, decimal: u8) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+') | Some(b'-')) {
        i = 1;
    }
    // (?=\d|<decimal>\d) — a bare sign, a bare separator or an empty string
    // must not be accepted.
    let starts_with_digit = b.get(i).is_some_and(u8::is_ascii_digit);
    let starts_with_decimal_digit =
        b.get(i) == Some(&decimal) && b.get(i + 1).is_some_and(u8::is_ascii_digit);
    if !starts_with_digit && !starts_with_decimal_digit {
        return false;
    }
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    if b.get(i) == Some(&decimal) {
        i += 1;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    if matches!(b.get(i), Some(b'E') | Some(b'e')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+') | Some(b'-')) {
            j += 1;
        }
        let digits_start = j;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
        // The exponent group is optional: if it does not match, the regex
        // backtracks and `$` then fails on the leftover 'e'.
        if j > digits_start {
            i = j;
        }
    }
    i == b.len()
}

/// `IsFloat` — `ExifTool.pm:5924-5930`. Returns the value with the comma
/// locale form translated to a `.` ("but translate ',' to '.'", `:5928`), or
/// `None` when the string is not a floating point number.
fn as_float_text(s: &str) -> Option<String> {
    if matches_float_shape(s, b'.') {
        return Some(s.to_string());
    }
    if matches_float_shape(s, b',') {
        return Some(s.replace(',', "."));
    }
    None
}

/// `IsRational` — `ExifTool.pm:5933`: `m{^[-+]?\d+/\d+$}`, split into parts.
fn as_fraction(s: &str) -> Option<(i64, i64)> {
    let (num, den) = s.split_once('/')?;
    // `[-+]?\d+` for the numerator; the denominator half carries no sign.
    if !is_int(num) || den.is_empty() || !den.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((num.parse().ok()?, den.parse().ok()?))
}

// ---------------------------------------------------------------------------
// Per-type parsers
// ---------------------------------------------------------------------------

/// `CheckValue`'s `int` branch — `Writer.pl:6873-6883`.
///
/// Range checking against a specific TIFF integer width (`%intRange`,
/// `Writer.pl:238-248`) is not done here: [`ValueType::Integer`] does not
/// record a width, and the EXIF serializer picks the narrowest TIFF type that
/// fits (`writers::exif_surgical::tag_value_to_field`).
fn parse_integer(tag_name: &str, raw: &str) -> Result<i64> {
    if is_int(raw) {
        return raw
            .parse::<i64>()
            .map_err(|_| invalid(tag_name, format!("Integer value '{}' is out of range", raw)));
    }
    if is_hex(raw) {
        let body = raw
            .strip_prefix("0x")
            .or_else(|| raw.strip_prefix("0X"))
            .unwrap_or(raw);
        return i64::from_str_radix(body, 16)
            .map_err(|_| invalid(tag_name, format!("Integer value '{}' is out of range", raw)));
    }
    // "round single floating point values to the nearest integer"
    // (`Writer.pl:6879-6881`): int($val + ($val < 0 ? -0.5 : 0.5)).
    if let Some(text) = as_float_text(raw) {
        let value: f64 = text
            .parse()
            .map_err(|_| invalid(tag_name, "Not an integer"))?;
        let rounded = (value + if value < 0.0 { -0.5 } else { 0.5 }).trunc();
        if !rounded.is_finite() || rounded < i64::MIN as f64 || rounded > i64::MAX as f64 {
            return Err(invalid(
                tag_name,
                format!("Integer value '{}' is out of range", raw),
            ));
        }
        return Ok(rounded as i64);
    }
    Err(invalid(tag_name, "Not an integer"))
}

/// `CheckValue`'s `float`/`double` branch — `Writer.pl:6888-6900`. Unlike
/// `rational`, these formats do not accept the `N/D` fraction form.
fn parse_float(tag_name: &str, raw: &str) -> Result<f64> {
    as_float_text(raw)
        .and_then(|text| text.parse::<f64>().ok())
        .ok_or_else(|| invalid(tag_name, "Not a floating point number"))
}

/// `CheckValue`'s `rational` branch (`Writer.pl:6888-6903`) followed by
/// `Rationalize` (`Writer.pl:5200-5228`).
fn parse_rational(tag_name: &str, raw: &str, raw_mode: bool) -> Result<TagValue> {
    let leaf = tag_name.rsplit_once(':').map_or(tag_name, |(_, name)| name);
    if leaf == "CompressedBitsPerPixel" {
        let negative_fraction = as_fraction(raw).is_some_and(|(numerator, _)| numerator < 0);
        let negative_float = as_float_text(raw)
            .and_then(|text| text.parse::<f64>().ok())
            .is_some_and(|value| value < 0.0);
        if negative_fraction || negative_float {
            return Err(invalid(tag_name, "Must be an unsigned rational"));
        }
    }
    // Exif.pm 13.59 0x9206 PrintConvInv removes the optional whitespace and
    // trailing metres suffix from SubjectDistance before rationalizing it.
    // Raw mode (`-SubjectDistance#=`) bypasses this, exactly like the other
    // three unit-appending tags handled in `parse_cli_tag_value`. The
    // comparison is case-insensitive for the same reason `unit_suffix_tag`
    // is: ExifTool's own tag/group names are (confirmed against the oracle,
    // PR #963 comment 4112327521).
    let raw = if !raw_mode && leaf.eq_ignore_ascii_case("SubjectDistance") {
        raw.strip_suffix('m').map(str::trim_end).unwrap_or(raw)
    } else {
        raw
    };
    // Exif.pm 13.59 declares FocalLength (0x920a) and SubjectDistance
    // (0x9206) `rational64u`, so they take CheckValue's unsigned branch and
    // `SetRational64u`'s wider `Rationalize` cap (see `parse_rational64u`).
    if leaf.eq_ignore_ascii_case("FocalLength") || leaf.eq_ignore_ascii_case("SubjectDistance") {
        return parse_rational64u(tag_name, raw);
    }
    // Exif.pm 13.59 0x9202 ApertureValue and 0x9205 MaxApertureValue store
    // APEX but accept the displayed F-number: ValueConvInv => '$val>0 ?
    // 2*log($val)/log(2) : 0' on both. Apply this before the generic
    // rational cases because ExifTool rejects fractions, `inf` and `undef`
    // here rather than storing them directly.
    if matches!(
        tag_name.rsplit_once(':').map_or(tag_name, |(_, name)| name),
        "ApertureValue" | "MaxApertureValue"
    ) {
        let text =
            as_float_text(raw).ok_or_else(|| invalid(tag_name, "Not a floating point number"))?;
        let f_number: f64 = text
            .parse()
            .map_err(|_| invalid(tag_name, "Not a floating point number"))?;
        let apex = if f_number > 0.0 {
            2.0 * f_number.ln() / 2.0_f64.ln()
        } else {
            0.0
        };
        let (numerator, denominator) = rationalize(apex, RATIONAL_MAX);
        return Ok(TagValue::Rational {
            numerator: numerator as i32,
            denominator: denominator as i32,
        });
    }

    // Writer.pl:5203-5204 — 'inf' is 1/0 and 'undef' is 0/0.
    if raw == "inf" {
        return Ok(TagValue::Rational {
            numerator: 1,
            denominator: 0,
        });
    }
    if raw == "undef" {
        return Ok(TagValue::Rational {
            numerator: 0,
            denominator: 0,
        });
    }
    // Writer.pl:5205 — "accept fractional values", returned unchanged.
    if let Some((numerator, denominator)) = as_fraction(raw) {
        let numerator = i32::try_from(numerator).map_err(|_| {
            invalid(
                tag_name,
                format!("Rational numerator '{}' does not fit a 32-bit value", raw),
            )
        })?;
        let denominator = i32::try_from(denominator).map_err(|_| {
            invalid(
                tag_name,
                format!("Rational denominator '{}' does not fit a 32-bit value", raw),
            )
        })?;
        return Ok(TagValue::Rational {
            numerator,
            denominator,
        });
    }
    // GPS.pm 13.59 converts GPSLongitude's decimal input to D/M/S before
    // Rationalize runs on the three components. Retain a plain decimal's
    // exact value here so the writer can perform that conversion without the
    // precision loss caused by rationalizing the combined degree value first.
    if tag_name == "GPS:GPSLongitude"
        && let Some((numerator, denominator)) = exact_decimal_fraction(raw)
    {
        return Ok(TagValue::Rational {
            numerator,
            denominator,
        });
    }
    let text =
        as_float_text(raw).ok_or_else(|| invalid(tag_name, "Not a floating point number"))?;
    let value: f64 = text
        .parse()
        .map_err(|_| invalid(tag_name, "Not a floating point number"))?;
    let (numerator, denominator) = rationalize(value, RATIONAL_MAX);
    // An unsigned rational64u is rationalized up to 0xffffffff (Writer.pl
    // `SetRational64u`: `Rationalize($_[0],0xffffffff)`); a value that
    // needs more than 0x7fffffff (3.614421976e-10 is 1/2766695617) comes
    // out differently there, and a `TagValue::Rational` cannot hold it.
    // Refuse it rather than store the signed-range approximation (0/1). A
    // signed rational64s is rationalized up to 0x7fffffff there too
    // (`SetRational64s`), so its result is exactly this one: 13.59 stores
    // `-BrightnessValue=3.614421976e-10` as 0/1 (PR #957 review, Codex).
    if !rational_is_signed(tag_name) && rationalize(value, 0xffff_ffff) != (numerator, denominator)
    {
        return Err(invalid(
            tag_name,
            "the value needs a rational beyond the signed 32-bit range, which oxidex \
             cannot represent",
        ));
    }
    Ok(TagValue::Rational {
        numerator: numerator as i32,
        denominator: denominator as i32,
    })
}

/// Whether `tag_name`'s leaf is a `rational64s` tag of `Exif::Main` or
/// `GPS::Main` (its transcribed `Writable`). A name neither table declares
/// is not known to be signed.
fn rational_is_signed(tag_name: &str) -> bool {
    let leaf = tag_name.rsplit(':').next().unwrap_or(tag_name);
    [("Exif", "Main"), ("GPS", "Main")]
        .iter()
        .any(|(module, table)| {
            crate::exiftool_tables::find_ifd_table(module, table).is_some_and(|table| {
                table.tags.iter().any(|tag| {
                    tag.name.eq_ignore_ascii_case(leaf) && tag.writable == Some("rational64s")
                })
            })
        })
}

fn parse_rational64u(tag_name: &str, raw: &str) -> Result<TagValue> {
    const RATIONAL64U_MAX: i64 = 0xffff_ffff;
    let unrepresentable = || {
        invalid(
            tag_name,
            format!(
                "'{raw}' needs an unsigned 32-bit rational component (rational64u) \
                 above {}, which cannot be written exactly",
                i32::MAX
            ),
        )
    };
    // Writer.pl:5223-5224 -- 'inf' is 1/0 and 'undef' is 0/0.
    if raw == "inf" {
        return Ok(TagValue::Rational {
            numerator: 1,
            denominator: 0,
        });
    }
    if raw == "undef" {
        return Ok(TagValue::Rational {
            numerator: 0,
            denominator: 0,
        });
    }
    // Writer.pl:6914-6916 -- a negative `N/D` is refused for a `u` format;
    // otherwise `Rationalize` returns the pair unchanged (:5225).
    if let Some((numerator, denominator)) = as_fraction(raw) {
        if numerator < 0 {
            return Err(invalid(tag_name, "Must be an unsigned rational"));
        }
        let numerator = i32::try_from(numerator).map_err(|_| unrepresentable())?;
        let denominator = i32::try_from(denominator).map_err(|_| unrepresentable())?;
        return Ok(TagValue::Rational {
            numerator,
            denominator,
        });
    }
    let text =
        as_float_text(raw).ok_or_else(|| invalid(tag_name, "Not a floating point number"))?;
    let value: f64 = text
        .parse()
        .map_err(|_| invalid(tag_name, "Not a floating point number"))?;
    // Writer.pl:6921-6923.
    if value < 0.0 {
        return Err(invalid(tag_name, "Must be a positive number"));
    }
    let (numerator, denominator) = rationalize(value, RATIONAL64U_MAX);
    let numerator = i32::try_from(numerator).map_err(|_| unrepresentable())?;
    let denominator = i32::try_from(denominator).map_err(|_| unrepresentable())?;
    Ok(TagValue::Rational {
        numerator,
        denominator,
    })
}

fn exact_decimal_fraction(raw: &str) -> Option<(i32, i32)> {
    let raw = raw.strip_prefix('+').unwrap_or(raw);
    let (negative, raw) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let (whole, fraction) = raw.split_once('.')?;
    if whole.is_empty() && fraction.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let scale = 10_i64.checked_pow(u32::try_from(fraction.len()).ok()?)?;
    let whole = if whole.is_empty() {
        0
    } else {
        whole.parse::<i64>().ok()?
    };
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<i64>().ok()?
    };
    let mut numerator = whole.checked_mul(scale)?.checked_add(fraction)?;
    if negative {
        numerator = -numerator;
    }
    let divisor = gcd_i64(numerator, scale);
    Some((
        i32::try_from(numerator / divisor).ok()?,
        i32::try_from(scale / divisor).ok()?,
    ))
}

fn gcd_i64(mut left: i64, mut right: i64) -> i64 {
    left = left.abs();
    right = right.abs();
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left.max(1)
}

/// Inverts Exif.pm 0x9201's display conversions before storing the signed
/// APEX rational. `1/125` is an exposure time in seconds, not the literal
/// value written to TIFF: PrintConvInv converts the fraction to 0.008, then
/// ValueConvInv computes `-log2(0.008)`.
fn parse_shutter_speed_value(tag_name: &str, raw: &str) -> Result<TagValue> {
    let seconds = if let Some((numerator, denominator)) = as_fraction(raw) {
        if denominator == 0 {
            return Err(invalid(tag_name, "Not a floating point number"));
        }
        numerator as f64 / denominator as f64
    } else {
        as_float_text(raw)
            .and_then(|text| text.parse::<f64>().ok())
            .ok_or_else(|| invalid(tag_name, "Not a floating point number"))?
    };
    let apex = if seconds > 0.0 {
        -seconds.log2()
    } else {
        -100.0
    };
    let (numerator, denominator) = rationalize(apex, RATIONAL_MAX);
    Ok(TagValue::Rational {
        numerator: numerator as i32,
        denominator: denominator as i32,
    })
}

/// `AssembleRational` — `Writer.pl:5182-5187`.
fn assemble_rational(num: f64, denom: f64, fracs: &[f64]) -> (f64, f64) {
    match fracs.split_first() {
        None => (num, denom),
        Some((frac, rest)) => assemble_rational(frac * num + denom, num, rest),
    }
}

/// `YYYY:mm:dd HH:MM:SS`, exactly: the one raw date/time spelling a
/// `TagValue::DateTime` stores byte-for-byte as given.
fn is_canonical_exif_datetime(raw: &str) -> bool {
    raw.len() == 19
        && raw.bytes().enumerate().all(|(at, byte)| match at {
            4 | 7 | 13 | 16 => byte == b':',
            10 => byte == b' ',
            _ => byte.is_ascii_digit(),
        })
}

/// `ReverseLookup`'s `Unknown (X)` form (Writer.pl 13.59:3614-3620):
/// `/^Unknown\s*\((.*)\)$/i`, with an `0x`-prefixed X read as hex (`hex()`,
/// so `Unknown (0x10)` is 16). `None` for any other value.
fn unknown_print_conv_value(raw: &str) -> Option<String> {
    let head = raw.get(..7)?;
    if !head.eq_ignore_ascii_case("unknown") {
        return None;
    }
    let inner = raw[7..]
        .trim_start_matches(|c: char| c.is_ascii_whitespace())
        .strip_prefix('(')?
        .strip_suffix(')')?;
    let hex = inner
        .strip_prefix("0x")
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit()));
    Some(
        match hex.and_then(|digits| u64::from_str_radix(digits, 16).ok()) {
            Some(value) => value.to_string(),
            None => inner.to_string(),
        },
    )
}

/// Whether `declared_tag_name` is an `Exif::Main`/`GPS::Main` tag whose
/// PrintConv is a hash that ExifTool inverts with `ReverseLookup` (so its
/// `Unknown (X)` form is the raw value): the GPS letter-code references,
/// the hand-written hash arms of [`parse_cli_tag_value_with_mode`], and any
/// transcribed plain [`crate::exiftool_tables::PrintConv::IntEnum`] row.
/// `Contrast`/`Saturation`/`Sharpness` declare a hash too, but their
/// `PrintConvInv` is `ConvertParameter`, which ExifTool calls instead of
/// `ReverseLookup` (13.59: `-ExifIFD:Contrast=Unknown (1)` is `Error
/// converting value ... (PrintConvInv)`), and `ComponentsConfiguration`'s
/// value is a list, so neither is a lookup hash here.
fn print_conv_is_lookup_hash(declared_tag_name: &str) -> bool {
    if is_gps_reference_string(declared_tag_name) {
        return true;
    }
    if exif_or_gps_module_for(declared_tag_name).is_none() {
        return false;
    }
    let leaf = declared_tag_name
        .rsplit(':')
        .next()
        .unwrap_or(declared_tag_name);
    if matches!(
        leaf,
        "Contrast" | "Saturation" | "Sharpness" | "ComponentsConfiguration"
    ) {
        return false;
    }
    matches!(
        leaf,
        "ColorSpace"
            | "CustomRendered"
            | "GPSDifferential"
            | "PlanarConfiguration"
            | "MakerNoteSafety"
            | "ProfileEmbedPolicy"
            | "SubjectDistanceRange"
            | "SecurityClassification"
            | "FileSource"
            | "SceneType"
            | "LightSource"
            | "CalibrationIlluminant1"
            | "CalibrationIlluminant2"
            | "CalibrationIlluminant3"
            | "Flash"
    ) || transcribed_row(declared_tag_name).is_some_and(|row| {
        matches!(
            row.print_conv,
            crate::exiftool_tables::PrintConv::IntEnum(_)
        )
    })
}

/// One of the GPS.pm letter-code reference tags ([`is_gps_enum_reference`]),
/// addressed bare or under `GPS:`. Every one is `Writable => 'string'`,
/// `Count => 2` in the transcribed `GPS::Main` table.
fn is_gps_reference_string(declared_tag_name: &str) -> bool {
    is_gps_enum_reference(declared_tag_name)
        && matches!(declared_tag_name.split_once(':'), None | Some(("GPS", _)))
}

/// The transcribed `Exif::Main`/`GPS::Main` row `declared_tag_name`
/// addresses: for an `Exif::Main` name with several rows, the one its
/// destination directory selects
/// ([`crate::writers::write_request::exif_main_row_for_destination`]);
/// otherwise the row at the registry's numeric id when that row carries the
/// name; otherwise the first row of that name. `None` when
/// [`exif_or_gps_module_for`] rejects the tag or the table has no such row.
fn transcribed_row(declared_tag_name: &str) -> Option<&'static crate::exiftool_tables::IfdTag> {
    let module = exif_or_gps_module_for(declared_tag_name)?;
    let leaf = declared_tag_name.rsplit(':').next()?;
    if module == "Exif"
        && let Some(row) =
            crate::writers::write_request::exif_main_row_for_destination(declared_tag_name)
    {
        return Some(row);
    }
    let table = crate::exiftool_tables::find_ifd_table(module, "Main")?;
    get_tag_descriptor(declared_tag_name)
        .and_then(|descriptor| {
            let crate::core::TagId::Numeric(id) = descriptor.id() else {
                return None;
            };
            table.tag(*id).filter(|tag| tag.name == leaf)
        })
        .or_else(|| table.tags.iter().find(|tag| tag.name == leaf))
}

/// The GPS.pm string tags whose PrintConv is a hash of letter codes.
fn is_gps_enum_reference(tag_name: &str) -> bool {
    matches!(
        tag_name.rsplit(':').next(),
        Some(
            "GPSLatitudeRef"
                | "GPSLongitudeRef"
                | "GPSStatus"
                | "GPSMeasureMode"
                | "GPSSpeedRef"
                | "GPSTrackRef"
                | "GPSImgDirectionRef"
                | "GPSDestLatitudeRef"
                | "GPSDestLongitudeRef"
                | "GPSDestBearingRef"
                | "GPSDestDistanceRef"
        )
    )
}

/// `Rationalize` — `Writer.pl:5200-5228`. The `inf` / `undef` / `N/D` cases
/// (`:5203-5205`) are handled by [`parse_rational`] before this is reached.
fn rationalize(value: f64, max_int: i64) -> (i64, i64) {
    if value == 0.0 {
        return (0, 1); // :5207
    }
    let (sign, value) = if value < 0.0 {
        (-1i64, -value)
    } else {
        (1i64, value)
    }; // :5208
    let max = max_int as f64;
    let mut best: Option<(f64, f64)> = None;
    let mut fracs: Vec<f64> = Vec::new();
    let mut frac = value;
    loop {
        let (n, d) = assemble_rational((frac + 0.5).trunc(), 1.0, &fracs); // :5213
        if n > max || d > max {
            // :5214
            if best.is_some() {
                break; // :5215
            }
            if value < 1.0 {
                return (sign, max_int); // :5216
            }
            return (sign * max_int, 1); // :5217
        }
        best = Some((n, d)); // :5219
        let err = (n / d - value) / value; // :5220
        if err.abs() < 1e-8 {
            break; // :5221
        }
        let int = frac.trunc(); // :5222
        fracs.insert(0, int); // :5223
        frac -= int;
        if frac == 0.0 {
            break; // :5224
        }
        frac = 1.0 / frac; // :5225
    }
    let (num, denom) = best.unwrap_or((0.0, 1.0));
    (num as i64 * sign, denom as i64)
}

/// A PDF Info date: ExifTool's `InverseDateTime` for PDF.pm's
/// `PrintConvInv => '$self->InverseDateTime($val)'` (Writer.pl:5012-5151
/// with `$tzFlag` undefined and no `DateFormat`), which keeps the zone and
/// the sub-seconds as given (the EXIF path, [`parse_datetime`], drops both),
/// returned as the text it produces. The PDF writer then encodes that text
/// as WritePDF.pl's `WritePDFValue` does (`pdf_writer`: sub-seconds dropped,
/// `+HH:MM` as `+HH'MM'`, `Z` kept).
///
/// Pinned 13.59 on tests/fixtures/pdf/sample.pdf, `-PDF:CreateDate=V`
/// writes `/CreationDate (D:...)`:
///
/// | V | written |
/// |---|---|
/// | `2020:01:02 03:04:05` | `D:20200102030405` |
/// | `2020:01:02 03:04:05Z` (or `z`) | `D:20200102030405Z` |
/// | `2020:01:02 03:04:05.25+02:00` | `D:20200102030405+02'00'` |
/// | `2020:01:02 03:04:05-0530` | `D:20200102030405-05'30'` |
/// | `2020-01-02T03:04:05`, `20200102030405` | `D:20200102030405` |
/// | `2020:01:02 03:04` | `D:20200102030400` |
/// | `2020:01:02 03:04:0é` | `D:20200102030400` |
/// | `2020:01:02`, `junk` | refused: `Invalid date/time (use ...)` |
///
/// Char-safe throughout: a multibyte character anywhere is only a
/// non-digit, never a place to split the text.
fn parse_pdf_date(tag_name: &str, raw: &str) -> Result<TagValue> {
    const USAGE: &str = "Invalid date/time (use YYYY:mm:dd HH:MM:SS[.ss][+/-HH:MM|Z])";
    let mut val = raw.to_string();
    // :5018-5026 -- a trailing zone, else a trailing `Z`, else `now`.
    let tz = match strip_timezone_suffix(&mut val) {
        Some(tz) => tz,
        None => match val.strip_suffix(['Z', 'z']) {
            Some(rest) => {
                val = rest.to_string();
                "Z".to_string()
            }
            // :5025 -- TimeNow with the local zone.
            None if val.eq_ignore_ascii_case("now") => {
                let now = chrono::Local::now();
                return Ok(TagValue::String(
                    now.format("%Y:%m:%d %H:%M:%S%:z").to_string(),
                ));
            }
            None => String::new(),
        },
    };
    // :5100-5103 -- the year, then every one-or-two digit run, padded.
    let (year, mut parts) = scan_date_numbers(&val).ok_or_else(|| invalid(tag_name, USAGE))?;
    for part in &mut parts {
        if part.len() < 2 {
            part.insert(0, '0');
        }
    }
    // :5104 -- fewer than mm/dd/HH is no date/time.
    if parts.len() < 3 {
        return Err(invalid(tag_name, USAGE));
    }
    let given = parts.len();
    let seconds = parts.get(4).cloned(); // :5105
    while parts.len() < 5 {
        parts.push("00".to_string()); // :5106
    }
    // :5108-5110 -- sub-seconds only after SS, with a leading `.`.
    let fraction = if given > 5 {
        trailing_fraction(&val).unwrap_or_default()
    } else {
        String::new()
    };
    let number =
        |s: &str| -> Result<u32> { s.parse::<u32>().map_err(|_| invalid(tag_name, USAGE)) };
    // :5134-5143 -- ExifTool's own range checks, with its own wording.
    if !(1..=12).contains(&number(&parts[0])?) {
        return Err(invalid(
            tag_name,
            format!("Month '{}' out of range 1..12", parts[0]),
        ));
    }
    if !(1..=31).contains(&number(&parts[1])?) {
        return Err(invalid(
            tag_name,
            format!("Day '{}' out of range 1..31", parts[1]),
        ));
    }
    if number(&parts[2])? > 24 {
        return Err(invalid(
            tag_name,
            format!("Hour '{}' out of range 0..24", parts[2]),
        ));
    }
    if number(&parts[3])? > 59 {
        return Err(invalid(
            tag_name,
            format!("Minutes '{}' out of range 0..59", parts[3]),
        ));
    }
    // :5126-5132 -- seconds only when given and below 60.
    let seconds = match seconds {
        Some(ss) if number(&ss)? < 60 => ss,
        _ => "00".to_string(),
    };
    Ok(TagValue::String(format!(
        "{year:04}:{}:{} {}:{}:{seconds}{fraction}{tz}",
        parts[0], parts[1], parts[2], parts[3]
    )))
}

/// `$val =~ /(\.\d+)\s*$/` -- `Writer.pl:5109`.
fn trailing_fraction(val: &str) -> Option<String> {
    let trimmed = val.trim_end_matches(|c: char| c.is_ascii_whitespace());
    let digits = trimmed.len() - trimmed.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let head = &trimmed[..trimmed.len() - digits];
    (digits > 0 && head.ends_with('.')).then(|| format!(".{}", &trimmed[trimmed.len() - digits..]))
}

/// `InverseDateTime` — `Writer.pl:5012-5151`, for the case this CLI is in:
/// no `DateFormat` option set, and `$tzFlag = 0` as EXIF's date tags pass it
/// — which discards both the timezone and the sub-seconds (`:5123-5124`).
/// That discard is why mapping the parsed wall clock onto `Utc` is faithful
/// rather than lossy: ExifTool stores the same wall clock ExifTool was given.
fn parse_datetime(tag_name: &str, raw: &str) -> Result<TagValue> {
    const USAGE: &str = "Invalid date/time (use YYYY:mm:dd HH:MM:SS[.ss][+/-HH:MM|Z])";

    let mut val = raw.to_string();
    // :5018-5026 — strip a trailing timezone, else a trailing 'Z', else allow
    // the special value 'now'.
    if strip_timezone_suffix(&mut val).is_none() {
        let stripped = val.strip_suffix(['Z', 'z']).map(str::to_string);
        match stripped {
            Some(rest) => val = rest,
            None if val.eq_ignore_ascii_case("now") => {
                // :5025 — ExifTool's TimeNow uses local time; with the
                // timezone dropped, that is the local wall clock.
                let now = chrono::Local::now().naive_local().with_nanosecond(0);
                let now = now.ok_or_else(|| invalid(tag_name, USAGE))?;
                return Ok(TagValue::DateTime(Utc.from_utc_datetime(&now)));
            }
            None => {}
        }
    }

    // :5100-5102 — first run of four digits is the year, then every
    // subsequent one-or-two digit run is mm, dd, HH, and maybe MM, SS.
    let (year, mut parts) = scan_date_numbers(&val).ok_or_else(|| invalid(tag_name, USAGE))?;
    // :5103 — pad each to two digits.
    for part in &mut parts {
        if part.len() < 2 {
            part.insert(0, '0');
        }
    }
    // :5104 — fewer than mm/dd/HH is not a date/time (we never set $dateOnly).
    if parts.len() < 3 {
        return Err(invalid(tag_name, USAGE));
    }
    let seconds_given = parts.get(4).cloned(); // :5105, taken before padding below
    while parts.len() < 5 {
        parts.push("00".to_string()); // :5106
    }

    let number =
        |s: &str| -> Result<u32> { s.parse::<u32>().map_err(|_| invalid(tag_name, USAGE)) };
    let (month, day, hour, minute) = (
        number(&parts[0])?,
        number(&parts[1])?,
        number(&parts[2])?,
        number(&parts[3])?,
    );
    // :5134-5143 — ExifTool's own range checks, with its own wording.
    if !(1..=12).contains(&month) {
        return Err(invalid(
            tag_name,
            format!("Month '{}' out of range 1..12", parts[0]),
        ));
    }
    if !(1..=31).contains(&day) {
        return Err(invalid(
            tag_name,
            format!("Day '{}' out of range 1..31", parts[1]),
        ));
    }
    if hour > 24 {
        return Err(invalid(
            tag_name,
            format!("Hour '{}' out of range 0..24", parts[2]),
        ));
    }
    if minute > 59 {
        return Err(invalid(
            tag_name,
            format!("Minutes '{}' out of range 0..59", parts[3]),
        ));
    }
    // :5126-5132 — seconds are only used when they were given and are < 60.
    let second = match seconds_given.as_deref().map(number).transpose()? {
        Some(s) if s < 60 => s,
        _ => 0,
    };

    // ExifTool's checks are looser than a real calendar: it permits day 31 in
    // February and hour 24, which `chrono::NaiveDate`/`NaiveTime` cannot
    // represent. Refuse rather than silently store a different instant.
    let naive = NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_opt(hour, minute, second))
        .ok_or_else(|| {
            invalid(
                tag_name,
                format!(
                    "'{}' is not a representable date/time (parsed as \
                     {:04}:{:02}:{:02} {:02}:{:02}:{:02})",
                    raw, year, month, day, hour, minute, second
                ),
            )
        })?;
    Ok(TagValue::DateTime(Utc.from_utc_datetime(&naive)))
}

/// Strips a trailing timezone and returns it as ExifTool spells it
/// (`sprintf("$1%.2d:$3", $2)`), or `None` when there is none:
/// `s/([-+])(\d{1,2}):?(\d{2})\s*(DST)?$//i` — `Writer.pl:5018`.
fn strip_timezone_suffix(s: &mut String) -> Option<String> {
    let b = s.as_bytes();
    let mut end = b.len();
    // Bytes, not `str` slices: `end - 3` need not be a char boundary
    // (`-DateTimeOriginal=€b` panicked here).
    if end >= 3 && b[end - 3..end].eq_ignore_ascii_case(b"DST") {
        end -= 3;
    }
    while end > 0 && b[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    // (\d{2})
    if end < 2 || !b[end - 1].is_ascii_digit() || !b[end - 2].is_ascii_digit() {
        return None;
    }
    let minutes_at = end - 2;
    end -= 2;
    // :?
    if end > 0 && b[end - 1] == b':' {
        end -= 1;
    }
    // (\d{1,2})
    let mut seen = 0;
    while seen < 2 && end > 0 && b[end - 1].is_ascii_digit() {
        end -= 1;
        seen += 1;
    }
    if seen == 0 {
        return None;
    }
    // ([-+])
    if end == 0 || (b[end - 1] != b'-' && b[end - 1] != b'+') {
        return None;
    }
    let hours: u32 = std::str::from_utf8(&b[end..end + seen])
        .ok()?
        .parse()
        .ok()?;
    let minutes = std::str::from_utf8(&b[minutes_at..minutes_at + 2]).ok()?;
    // sprintf("$1%.2d:$3", $2)
    let tz = format!("{}{hours:02}:{minutes}", b[end - 1] as char);
    s.truncate(end - 1);
    Some(tz)
}

/// `($val =~ /(\d{4})/g)` then `($val =~ /\d{1,2}/g)` — `Writer.pl:5100-5102`.
/// The second match continues from where the first left off, so the year's own
/// digits are not re-read.
fn scan_date_numbers(s: &str) -> Option<(i32, Vec<String>)> {
    let b = s.as_bytes();
    let year_end = (0..b.len().saturating_sub(3))
        .find(|&i| b[i..i + 4].iter().all(u8::is_ascii_digit))
        .map(|i| i + 4)?;
    let year: i32 = s[year_end - 4..year_end].parse().ok()?;

    let mut parts = Vec::new();
    let mut i = year_end;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let mut j = i + 1;
            if j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            parts.push(s[i..j].to_string());
            i = j;
        } else {
            i += 1;
        }
    }
    Some((year, parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(tag: &str, raw: &str) -> Result<TagValue> {
        parse_cli_tag_value(tag, raw)
    }

    /// [`parse`] in raw mode (the CLI's `#` suffix / `--no-print-conv`):
    /// skips every PrintConv label lookup, parsing purely by declared type.
    fn parse_raw(tag: &str, raw: &str) -> Result<TagValue> {
        parse_cli_tag_value_with_mode(tag, raw, true)
    }

    /// PR #959 review finding (`4111785371`): leaf-only dispatch must not
    /// send a MakerNotes tag that happens to share a leaf name with an
    /// Exif::Main/GPS::Main tag (`Sony:ExposureMode` vs. Exif.pm's
    /// `ExposureMode`) through this file's Exif/GPS enum inversion.
    #[test]
    fn exif_or_gps_module_for_is_gated_by_the_tags_actual_table() {
        for tag in [
            "EXIF:Orientation",
            "IFD0:Orientation",
            "EXIF:ResolutionUnit",
            "EXIF:Compression",
            "EXIF:YCbCrPositioning",
            "EXIF:ExposureMode",
            "ExifIFD:ExposureMode",
            "EXIF:MeteringMode",
            "EXIF:ExposureProgram",
            "EXIF:WhiteBalance",
            "EXIF:SceneCaptureType",
            "EXIF:GainControl",
            "EXIF:GrayResponseUnit",
            "EXIF:LightSource",
            "EXIF:CalibrationIlluminant1",
            "EXIF:CalibrationIlluminant2",
            "EXIF:CalibrationIlluminant3",
        ] {
            assert_eq!(
                exif_or_gps_module_for(tag),
                Some("Exif"),
                "{tag} should resolve to Exif::Main"
            );
        }
        for tag in ["GPS:GPSStatus", "GPS:GPSDifferential"] {
            assert_eq!(
                exif_or_gps_module_for(tag),
                Some("GPS"),
                "{tag} should resolve to GPS::Main"
            );
        }
        // MakerNotes tags sharing a leaf name with an Exif::Main tag must
        // NOT resolve to either table.
        for tag in ["Sony:ExposureMode", "Canon:CanonExposureMode"] {
            assert_eq!(
                exif_or_gps_module_for(tag),
                None,
                "{tag} must not be treated as Exif::Main/GPS::Main"
            );
        }
    }

    /// Same finding, exercised end to end: `-Sony:ExposureMode=Auto` must not
    /// be silently accepted as if it were Exif.pm's `ExposureMode` (0xa402).
    /// Confirmed against pinned ExifTool 13.59 on Canon.jpg: it refuses this
    /// exact value (`Sony:ExposureMode` has no such PrintConv label).
    #[test]
    fn sony_exposure_mode_is_not_inverted_through_exif_main() {
        assert!(parse("Sony:ExposureMode", "Auto").is_err());
    }

    /// PR #959 review (Codex, round 4, P2): the `SceneType` early return
    /// dispatched by leaf name alone and hardcoded `Exif::Main`, so a real
    /// DICOM field sharing that leaf (`DICOM:SceneType`, `US` per
    /// `src/parsers/specialized/dicom_dict.rs`) would have been inverted
    /// against Exif.pm's one-byte `undef` `SceneType` (0xa301) instead of
    /// left alone. `exif_or_gps_module_for` now gates the whole block,
    /// including its `raw_mode` byte-packing branch (which assumed the same
    /// wrong one-byte shape).
    #[test]
    fn dicom_scene_type_is_not_inverted_through_exif_main() {
        assert_ne!(
            parse("DICOM:SceneType", "Directly photographed").ok(),
            Some(TagValue::Binary(vec![1])),
            "a DICOM field must never be silently read as Exif.pm's SceneType"
        );
        assert_eq!(
            parse("DICOM:SceneType", "Directly photographed").unwrap(),
            TagValue::String("Directly photographed".to_string()),
            "with no known conversion for a DICOM tag, this file passes the text through unchanged"
        );
        // The real EXIF tag, with and without an explicit group, still works.
        for tag in ["SceneType", "EXIF:SceneType", "ExifIFD:SceneType"] {
            assert_eq!(
                parse(tag, "Directly photographed").unwrap(),
                TagValue::Binary(vec![1])
            );
        }
    }

    #[test]
    fn exif_or_gps_module_for_requires_a_recognizable_exif_group_prefix() {
        // A bare name and every EXIF-family IFD spelling are trusted by
        // default (no registry entry needed).
        for tag in [
            "SceneType",
            "EXIF:SceneType",
            "ExifIFD:SceneType",
            "IFD0:SceneType",
            "IFD1:SceneType",
            "InteropIFD:SceneType",
        ] {
            assert_eq!(exif_or_gps_module_for(tag), Some("Exif"), "{tag}");
        }
        assert_eq!(exif_or_gps_module_for("GPS:GPSStatus"), Some("GPS"));
        // An explicit, non-EXIF-family group is never trusted by default,
        // registry entry or not.
        for tag in ["DICOM:SceneType", "Sony:ExposureMode", "XMP:SceneType"] {
            assert_eq!(exif_or_gps_module_for(tag), None, "{tag}");
        }
    }

    /// PR #959 review (Codex, round 4, P2): the generic enum-inversion arm is
    /// gated on `Some(ValueType::Integer)`, which only a registry descriptor
    /// (or, now, this fallback) can produce -- `EXIF:ShadingCorrection`
    /// (0xa411) and `EXIF:NoiseReduction` (0xa412) are writable `int16u` enum
    /// rows in the transcribed table with NO registry descriptor at all, so
    /// `declared` was unconditionally `None` and the function returned a
    /// `String` before the enum-inversion arm ever ran.
    /// `declared_value_type_from_transcribed_table` closes that gap by
    /// deriving the type from the transcribed row itself when the registry
    /// has nothing.
    #[test]
    fn registry_absent_transcribed_enum_rows_still_invert() {
        assert_eq!(
            parse("EXIF:ShadingCorrection", "Yes").unwrap(),
            TagValue::Integer(1)
        );
        assert_eq!(
            parse("EXIF:NoiseReduction", "No").unwrap(),
            TagValue::Integer(0)
        );
    }

    /// PR #959 review `4112816741`: raw mode (`#`, `--no-print-conv`) skips
    /// the GPSLatitudeRef/GPSDestLatitudeRef `OTHER` inversion. Pinned 13.59:
    /// `-GPS:GPSLatitudeRef#=X` stores `X`, `#=South` is `String too long
    /// for GPS:GPSLatitudeRef` (string[2]); without `#`, `South` is `S` and
    /// `X` is not in PrintConv.
    #[test]
    fn raw_mode_skips_the_gps_latitude_ref_inversion() {
        for tag in ["GPS:GPSLatitudeRef", "GPS:GPSDestLatitudeRef"] {
            let raw = |value: &str| parse_cli_tag_value_with_mode(tag, value, true);
            assert_eq!(raw("X").unwrap(), TagValue::String("X".into()), "{tag}");
            assert_eq!(raw("S").unwrap(), TagValue::String("S".into()), "{tag}");
            let err = raw("South").unwrap_err().to_string();
            assert!(err.contains("String too long for GPS:"), "{tag}: {err}");
            assert_eq!(parse(tag, "South").unwrap(), TagValue::String("S".into()));
            assert!(parse(tag, "X").is_err(), "{tag}");
        }
        let err = parse_cli_tag_value_with_mode("GPS:GPSStatus", "Xy", true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("String too long for GPS:GPSStatus"), "{err}");
    }

    /// Codex pre-review of PR #959 (round 5): raw mode skips every
    /// hand-written PrintConvInv, not only the enum lookups. Each expectation
    /// is pinned 13.59's raw (`#`) outcome, or a refusal where it stores a
    /// value this port does not model.
    #[test]
    fn raw_mode_skips_the_hand_written_print_conv_inverses() {
        let raw = |tag: &str, value: &str| parse_cli_tag_value_with_mode(tag, value, true);
        // Verbatim bytes, as 13.59 stores them.
        assert_eq!(
            raw("ExifIFD:ExifVersion", "2.31").unwrap(),
            TagValue::Binary(b"2.31".to_vec())
        );
        assert_eq!(
            raw("ExifIFD:FlashpixVersion", "1.0").unwrap(),
            TagValue::Binary(b"1.0".to_vec())
        );
        assert_eq!(
            raw("GPS:GPSDateStamp", "2024:01:02").unwrap(),
            TagValue::String("2024:01:02".into())
        );
        assert_eq!(
            raw("ExifIFD:ISO", "100 200").unwrap(),
            TagValue::new_array(vec![TagValue::new_integer(100), TagValue::new_integer(200)])
        );
        // Refused: 13.59 refuses these, or stores something unmodelled.
        for (tag, value) in [
            ("ExifIFD:ISO", "1,000"),
            ("ExifIFD:ISO", "100, 200"),
            ("GPS:GPSDateStamp", "2024:01:02 10:11:12"),
            ("GPS:GPSDateStamp", "20240102"),
            ("GPS:GPSVersionID", "2.3.0.0"),
            ("ExifIFD:SubjectDistance", "5 m"),
            ("ExifIFD:FocalLength", "50 mm"),
            ("ExifIFD:ShutterSpeedValue", "1/250"),
            ("ExifIFD:DateTimeOriginal", "2020-01-02 03:04:05"),
            ("ExifIFD:DateTimeOriginal", "2020:01:02 03:04:05+02:00"),
        ] {
            assert!(raw(tag, value).is_err(), "{tag}#={value}");
        }
        for value in ["2020-01-02T03:04:05", "2020:01:02 03:04:05Z"] {
            assert!(
                raw("PDF:CreateDate", value).is_err(),
                "PDF:CreateDate#={value}"
            );
        }
        // The canonical raw forms still write.
        assert!(raw("PDF:CreateDate", "2020:01:02 03:04:05").is_ok());
        assert!(raw("PDF:CreateDate", "2020:01:02 03:04:05+02:00").is_ok());
        assert!(raw("ExifIFD:DateTimeOriginal", "2020:01:02 03:04:05").is_ok());
        assert!(raw("GPS:GPSVersionID", "2 3 0 0").is_ok());
        assert!(raw("ExifIFD:SubjectDistance", "5").is_ok());
        assert!(raw("ExifIFD:ShutterSpeedValue", "0.004").is_ok());
        // Without raw mode the inverses still apply.
        assert_eq!(
            parse("ExifIFD:ExifVersion", "2.31").unwrap(),
            TagValue::Binary(b"0231".to_vec())
        );
        assert!(parse("ExifIFD:FocalLength", "50 mm").is_ok());
    }

    /// Codex pre-review of PR #959 (round 5, second run): the raw writes of
    /// the one-byte `undef` enums and ComponentsConfiguration follow each
    /// tag's own ValueConvInv / `CheckValue`, per pinned 13.59.
    #[test]
    fn raw_undef_enum_bytes_follow_their_value_conv_inverse() {
        let raw = |tag: &str, value: &str| parse_cli_tag_value_with_mode(tag, value, true);
        for (value, bytes) in [
            ("3", &b"\x03"[..]),
            ("1.5", b"1.5"),
            ("abc", b"abc"),
            ("300", b"300"),
            ("-1", b"-1"),
            ("0x3", b"0x3"),
        ] {
            assert_eq!(
                raw("ExifIFD:FileSource", value).unwrap(),
                TagValue::Binary(bytes.to_vec()),
                "FileSource#={value}"
            );
        }
        for (value, byte) in [("1", 1u8), ("1.5", 1), ("257", 1), ("-1", 255)] {
            assert_eq!(
                raw("ExifIFD:SceneType", value).unwrap(),
                TagValue::Binary(vec![byte]),
                "SceneType#={value}"
            );
        }
        for value in ["abc", "0x2"] {
            assert!(
                raw("ExifIFD:SceneType", value).is_err(),
                "SceneType#={value}"
            );
        }
        assert_eq!(
            raw("ExifIFD:ComponentsConfiguration", "1 2 3 0").unwrap(),
            TagValue::Binary(vec![1, 2, 3, 0])
        );
        for value in [
            "1 2",
            "1.5 2 3 0",
            "1,2,3,0",
            "0x1 2 3 0",
            "256 0 0 0",
            "1 2 3 0 0",
        ] {
            assert!(
                raw("ExifIFD:ComponentsConfiguration", value).is_err(),
                "ComponentsConfiguration#={value}"
            );
        }
    }

    /// `ReverseLookup`'s `Unknown (X)` is the raw X for every PrintConv hash
    /// this file inverts, `0x..` read as hex, and never in raw mode. Pinned
    /// 13.59 writes each of these values; the catch-all arms refused the GPS
    /// ones, and the old GPS-only strip never reached the int16u tags.
    #[test]
    fn unknown_form_is_the_raw_value_for_lookup_hashes() {
        for (tag, value, expected) in [
            ("GPS:GPSStatus", "Unknown (X)", TagValue::String("X".into())),
            (
                "GPS:GPSDestDistanceRef",
                "Unknown ()",
                TagValue::String(String::new()),
            ),
            (
                "GPS:GPSMeasureMode",
                "Unknown (4)",
                TagValue::String("4".into()),
            ),
            (
                "GPS:GPSLatitudeRef",
                "Unknown (Q)",
                TagValue::String("Q".into()),
            ),
            (
                "GPS:GPSSpeedRef",
                "Unknown (Z)",
                TagValue::String("Z".into()),
            ),
            ("GPS:GPSDifferential", "Unknown (5)", TagValue::Integer(5)),
            (
                "GPS:GPSDifferential",
                "Unknown (0x10)",
                TagValue::Integer(16),
            ),
            ("ExifIFD:ColorSpace", "Unknown (3)", TagValue::Integer(3)),
            ("IFD0:Orientation", "Unknown (9)", TagValue::Integer(9)),
            ("IFD0:Orientation", "unknown(0x10)", TagValue::Integer(16)),
        ] {
            assert_eq!(parse(tag, value).unwrap(), expected, "{tag}={value}");
        }
        for (tag, value) in [
            ("GPS:GPSStatus", "Unknown (Xy)"),
            ("GPS:GPSLatitudeRef", "Unknown (South)"),
            // ConvertParameter, not a hash lookup: `Error converting value`.
            ("ExifIFD:Contrast", "Unknown (1)"),
        ] {
            assert!(parse(tag, value).is_err(), "{tag}={value}");
        }
        // Raw mode never calls ReverseLookup: the text is the raw value.
        assert!(parse_cli_tag_value_with_mode("GPS:GPSStatus", "Unknown (A)", true).is_err());
    }

    /// PR #959 review `4112816750`: `Exif::Main` repeats
    /// ChromaticAberrationCorrection/DistortionCorrection (Sony SubIFD rows
    /// 0x7034/0x7036 `Off`/`Auto`; Exif 3.1 rows 0xa410/0xa40f `No`/`Yes`).
    /// Pinned 13.59 writes `-ExifIFD:ChromaticAberrationCorrection=Yes`
    /// (also `IFD0:`, `EXIF:` and bare) to 0xa410 and refuses `=Auto`.
    #[test]
    fn duplicate_enum_rows_resolve_by_destination_directory() {
        for tag in [
            "ChromaticAberrationCorrection",
            "EXIF:ChromaticAberrationCorrection",
            "ExifIFD:ChromaticAberrationCorrection",
            "IFD0:DistortionCorrection",
            "ExifIFD:DistortionCorrection",
        ] {
            assert_eq!(parse(tag, "Yes").unwrap(), TagValue::Integer(1), "{tag}");
            assert_eq!(parse(tag, "No").unwrap(), TagValue::Integer(0), "{tag}");
            let err = parse(tag, "Auto").unwrap_err().to_string();
            assert!(err.contains("not in PrintConv"), "{tag}: {err}");
        }
        let row = |key| {
            crate::writers::write_request::exif_main_row_for_destination(key).map(|row| row.id)
        };
        assert_eq!(row("ExifIFD:ChromaticAberrationCorrection"), Some(0xa410));
        assert_eq!(row("SubIFD:ChromaticAberrationCorrection"), Some(0x7034));
        assert_eq!(row("IFD0:DistortionCorrection"), Some(0xa40f));
        assert_eq!(row("SubIFD:DistortionCorrection"), Some(0x7036));
        assert_eq!(
            row("ExifIFD:Orientation"),
            None,
            "one row: nothing to choose"
        );
        assert_eq!(row("Sony:ChromaticAberrationCorrection"), None);
    }

    // -- shape predicates, against ExifTool.pm:5924-5933 --------------------

    #[test]
    fn is_int_matches_exiftool_regex() {
        for good in ["0", "123", "+123", "-123"] {
            assert!(is_int(good), "{good}");
        }
        for bad in ["", "+", "-", "1.0", "1e1", " 1", "1 ", "0x10", "abc"] {
            assert!(!is_int(bad), "{bad}");
        }
    }

    #[test]
    fn is_hex_matches_exiftool_regex() {
        for good in ["0x10", "0X10", "abc", "DEADBEEF", "1", "12345678"] {
            assert!(is_hex(good), "{good}");
        }
        for bad in ["", "0x", "123456789", "0xdeadbeef1", "g", "1.0", "-1"] {
            assert!(!is_hex(bad), "{bad}");
        }
    }

    #[test]
    fn is_float_matches_exiftool_regex() {
        for good in [
            "0", "5.6", "+5.6", "-5.6", ".5", "5.", "1e1", "1E-3", "+1.5e+2",
        ] {
            assert!(matches_float_shape(good, b'.'), "{good}");
        }
        for bad in ["", "+", ".", "+.", "5e", "abc", "1/2", "0x10", " 5", "5 "] {
            assert!(!matches_float_shape(bad, b'.'), "{bad}");
        }
        // ExifTool.pm:5927 — comma separators for other locales.
        assert_eq!(as_float_text("5,6").as_deref(), Some("5.6"));
        assert_eq!(as_float_text("5.6").as_deref(), Some("5.6"));
        assert_eq!(as_float_text("abc"), None);
    }

    // -- Integer, Writer.pl:6873-6883 --------------------------------------

    // These two pin the *generic* int branch, which deliberately does no range
    // checking (see [`parse_integer`]), so the vehicle has to be a tag that
    // adds none of its own. EXIF:ISO is not one: Exif.pm 0x8827 is int16u, so
    // it rejects the negative half of the rounding rule. Exif.pm 0x882a
    // TimeZoneOffset is int16s and carries no PrintConv, so every form below
    // survives to the stored value.

    #[test]
    fn integer_accepts_every_form_checkvalue_accepts() {
        // Verified against `exiftool -EXIF:TimeZoneOffset=<v>` + `-n`
        // read-back, ExifTool 13.59 (the pinned .exiftool-version).
        let cases = [
            ("800", 800),
            ("+800", 800),
            ("800.4", 800),
            ("800.5", 801),
            ("0x320", 800),
            ("320a", 0x320a), // IsHex matches bare hex digits
            ("abc", 0xabc),
            ("1e1", 0x1e1), // IsHex is tried before IsFloat
        ];
        for (raw, want) in cases {
            assert_eq!(
                parse("EXIF:TimeZoneOffset", raw).unwrap(),
                TagValue::Integer(want),
                "input {raw}"
            );
        }
    }

    #[test]
    fn integer_rejects_what_checkvalue_rejects() {
        for raw in ["zz", "12:30", "1/2", "hello world"] {
            let err = parse("EXIF:TimeZoneOffset", raw).unwrap_err().to_string();
            assert!(err.contains("Not an integer"), "input {raw}: {err}");
        }
        // Not a CheckValue case: `-TAG=` is a delete, intercepted in `main.rs`
        // before it reaches this module. Rejecting it is this module's own
        // guard against an empty value arriving by another route.
        let err = parse("EXIF:TimeZoneOffset", "").unwrap_err().to_string();
        assert!(err.contains("Not an integer"), "empty input: {err}");
    }

    /// ISO is unsigned, so a negative magnitude is out of range rather than
    /// "not an integer" -- `exiftool -ExifIFD:ISO=-800.5` (13.59) answers
    /// `Warning: Value below int32u minimum for ExifIFD:ISO`, not the
    /// `Not an integer` it gives for `zz`. The two rejections are distinct
    /// and must not be collapsed.
    #[test]
    fn integer_rejects_negative_iso_as_out_of_range() {
        let err = parse("EXIF:ISO", "-800.5").unwrap_err().to_string();
        assert!(!err.contains("Not an integer"), "{err}");
    }

    /// An empty value is a *delete* request upstream of CheckValue -- real
    /// ExifTool reports `0 image files updated` for `-ExifIFD:ISO=` rather
    /// than a validation warning. The CLI value parser still refuses it,
    /// but not with CheckValue's "Not an integer" wording.
    #[test]
    fn integer_rejects_empty_without_claiming_not_an_integer() {
        let err = parse("EXIF:ISO", "").unwrap_err().to_string();
        assert!(!err.contains("Not an integer"), "{err}");
    }

    #[test]
    fn integer_rejects_iso_out_of_16bit_range() {
        // ISO is `Writable => 'int16u'` (Exif.pm 0x8827), so CheckValue
        // range-checks against `%intRange{int16u}` = [0, 0xffff]
        // (Writer.pl:238-248). Verified: `exiftool -ExifIFD:ISO=-800.5`
        // against the pinned 13.59 build warns "Value below int16u minimum"
        // and leaves the file untouched, and `=65536` warns "Value above
        // int16u maximum" the same way. `-800.5` rounds to -801 first
        // (Writer.pl:6879-6881), which is still out of range.
        for raw in ["-800.5", "-1", "65536", "70000"] {
            let err = parse("EXIF:ISO", raw).unwrap_err().to_string();
            assert!(
                err.contains("does not fit unsigned 16-bit"),
                "input {raw}: {err}"
            );
        }
    }

    #[test]
    fn flash_uses_its_print_conversion_inverse() {
        assert_eq!(parse("Flash", "Fired").unwrap(), TagValue::Integer(1));
        assert_eq!(
            parse("ExifIFD:Flash", "Auto, Did not fire").unwrap(),
            TagValue::Integer(0x18)
        );
        assert_eq!(
            parse("EXIF:Flash", "Unknown (0x38)").unwrap(),
            TagValue::Integer(0x38)
        );
        assert!(parse("Flash", "25").is_err());
    }

    #[test]
    fn subsec_time_original_extracts_fraction_from_printed_datetime() {
        assert_eq!(
            parse("ExifIFD:SubSecTimeOriginal", "2024:01:02 03:04:05.6789").unwrap(),
            TagValue::String("6789".to_string())
        );
    }

    #[test]
    fn subsec_time_extracts_fraction_and_accepts_bare_digits() {
        assert_eq!(
            parse("EXIF:SubSecTime", "2024:01:02 03:04:05.42").unwrap(),
            TagValue::String("42".to_string())
        );
        assert_eq!(
            parse("ExifIFD:SubSecTime", "007").unwrap(),
            TagValue::String("007".to_string())
        );
    }

    // -- Rational, Writer.pl:6888-6903 + 5200-5228 --------------------------

    #[test]
    fn rational_accepts_fraction_and_float_forms() {
        assert_eq!(
            parse("EXIF:ExposureTime", "1/250").unwrap(),
            TagValue::Rational {
                numerator: 1,
                denominator: 250
            }
        );
        // Writer.pl:5205 — a fraction is returned unchanged, not reduced.
        assert_eq!(
            parse("EXIF:ExposureTime", "2/500").unwrap(),
            TagValue::Rational {
                numerator: 2,
                denominator: 500
            }
        );
        // Rationalize's continued fraction: 5.6 -> 28/5.
        assert_eq!(
            parse("EXIF:FNumber", "5.6").unwrap(),
            TagValue::Rational {
                numerator: 28,
                denominator: 5
            }
        );
        assert_eq!(
            parse("EXIF:FNumber", "5,6").unwrap(),
            TagValue::Rational {
                numerator: 28,
                denominator: 5
            }
        );
        assert_eq!(
            parse("EXIF:XResolution", "300").unwrap(),
            TagValue::Rational {
                numerator: 300,
                denominator: 1
            }
        );
        // Writer.pl:5203-5204
        assert_eq!(
            parse("EXIF:FNumber", "inf").unwrap(),
            TagValue::Rational {
                numerator: 1,
                denominator: 0
            }
        );
        assert_eq!(
            parse("EXIF:FNumber", "undef").unwrap(),
            TagValue::Rational {
                numerator: 0,
                denominator: 0
            }
        );
    }

    #[test]
    fn gps_longitude_preserves_decimal_for_dms_conversion() {
        assert_eq!(
            parse("GPS:GPSLongitude", "122.4194").unwrap(),
            TagValue::Rational {
                numerator: 612_097,
                denominator: 5_000,
            }
        );
    }

    #[test]
    fn rationalize_reproduces_exiftool_values() {
        assert_eq!(rationalize(0.0, RATIONAL_MAX), (0, 1));
        assert_eq!(rationalize(5.6, RATIONAL_MAX), (28, 5));
        assert_eq!(rationalize(0.004, RATIONAL_MAX), (1, 250));
        assert_eq!(rationalize(-0.5, RATIONAL_MAX), (-1, 2));
        assert_eq!(rationalize(72.0, RATIONAL_MAX), (72, 1));
    }

    #[test]
    fn aperture_value_inverts_the_displayed_f_number_to_apex() {
        for tag in [
            "EXIF:ApertureValue",
            "ExifIFD:ApertureValue",
            "ApertureValue",
        ] {
            assert_eq!(
                parse(tag, "1.5").unwrap(),
                TagValue::Rational {
                    numerator: 9515,
                    denominator: 8133,
                },
                "{tag}"
            );
        }
        assert_eq!(
            parse("EXIF:ApertureValue", "0").unwrap(),
            TagValue::Rational {
                numerator: 0,
                denominator: 1,
            }
        );
        assert!(parse("EXIF:ApertureValue", "3/2").is_err());
    }

    #[test]
    fn bare_exposure_time_uses_the_canonical_rational_declaration() {
        assert_eq!(
            parse("ExposureTime", "1/4").unwrap(),
            TagValue::Rational {
                numerator: 1,
                denominator: 4,
            }
        );
    }

    #[test]
    fn bare_brightness_value_uses_the_canonical_signed_rational_declaration() {
        assert_eq!(
            parse("BrightnessValue", "-2.5").unwrap(),
            TagValue::Rational {
                numerator: -5,
                denominator: 2,
            }
        );
    }

    /// Local Codex pre-review of PR #963: a bare name in any letter case must
    /// resolve to the tag's declared type, as the exact-case spelling does.
    #[test]
    fn bare_mixed_case_unit_suffix_tags_resolve_their_declared_types() {
        let rational = |numerator, denominator| TagValue::Rational {
            numerator,
            denominator,
        };
        assert_eq!(parse("focallength", "50 mm").unwrap(), rational(50, 1));
        assert_eq!(parse("FOCALLENGTH#", "50").unwrap(), rational(50, 1));
        assert_eq!(parse("subjectdistance", "3.5 m").unwrap(), rational(7, 2));
        assert_eq!(
            parse("ambienttemperature", "20 C").unwrap(),
            rational(20, 1)
        );
        assert_eq!(
            parse("focallengthin35mmformat", "75 mm").unwrap(),
            TagValue::Integer(75)
        );
    }

    /// PR #963 comment 4112779672: rational64u values are rationalized with
    /// SetRational64u's 0xffffffff cap, and a result `TagValue::Rational`
    /// cannot hold is refused instead of clamped to the signed cap.
    #[test]
    fn rational64u_tags_refuse_instead_of_clamping() {
        for (tag, raw) in [
            ("ExifIFD:FocalLength", "3000000000"),
            ("ExifIFD:FocalLength", "3000000000 mm"),
            ("ExifIFD:FocalLength", "2147483648"),
            ("ExifIFD:FocalLength", "0.0000000003"),
            ("ExifIFD:FocalLength", "3000000000/1"),
            ("ExifIFD:SubjectDistance", "3000000000 m"),
        ] {
            let error = parse(tag, raw).unwrap_err().to_string();
            assert!(error.contains("rational64u"), "{tag}={raw}: {error}");
        }
        assert_eq!(
            parse("ExifIFD:FocalLength", "2147483647").unwrap(),
            TagValue::Rational {
                numerator: i32::MAX,
                denominator: 1,
            }
        );
        assert_eq!(
            parse("ExifIFD:FocalLength", "1.23456789012345").unwrap(),
            TagValue::Rational {
                numerator: 100,
                denominator: 81,
            }
        );
        for (tag, raw, reason) in [
            (
                "ExifIFD:FocalLength",
                "-1/2",
                "Must be an unsigned rational",
            ),
            (
                "ExifIFD:SubjectDistance",
                "-1 m",
                "Must be a positive number",
            ),
        ] {
            let error = parse(tag, raw).unwrap_err().to_string();
            assert!(error.contains(reason), "{tag}={raw}: {error}");
        }
    }

    #[test]
    fn metering_mode_uses_the_declared_print_conversion_inverse() {
        assert_eq!(parse("MeteringMode", "Spot").unwrap(), TagValue::Integer(3));
        assert_eq!(
            parse("ExifIFD:MeteringMode", "Multi-segment").unwrap(),
            TagValue::Integer(5)
        );
        assert_eq!(
            parse("EXIF:MeteringMode", "Other").unwrap(),
            TagValue::Integer(255)
        );
        // Pinned ExifTool 13.59 refuses a bare numeric code for a hash
        // `PrintConv` with no `OTHER` (confirmed against the oracle:
        // `-MeteringMode=3` without `-n` is `Warning: Can't convert
        // ExifIFD:MeteringMode (not in PrintConv)`), so "255" is no longer
        // accepted as a shortcut for its own label ("Other") here either.
        assert!(parse("EXIF:MeteringMode", "255").is_err());
        // The raw-mode counterpart (`-MeteringMode#=255`, `-n`) still takes
        // the code directly.
        assert_eq!(
            parse_raw("EXIF:MeteringMode", "255").unwrap(),
            TagValue::Integer(255)
        );
    }

    #[test]
    fn shutter_speed_value_inverts_seconds_to_stored_apex() {
        // ExifTool 13.59 stores `-ShutterSpeedValue=1/125` as 49471/7102.
        assert_eq!(
            parse("ShutterSpeedValue", "1/125").unwrap(),
            TagValue::Rational {
                numerator: 49_471,
                denominator: 7_102,
            }
        );
        assert_eq!(
            parse("ExifIFD:ShutterSpeedValue", "0").unwrap(),
            TagValue::Rational {
                numerator: -100,
                denominator: 1,
            }
        );
    }

    #[test]
    fn rational_rejects_non_numeric() {
        let err = parse("EXIF:FNumber", "abc").unwrap_err().to_string();
        assert!(err.contains("Not a floating point number"), "{err}");
    }

    // -- DateTime, Writer.pl:5012-5151 -------------------------------------

    #[test]
    fn datetime_accepts_every_form_inversedatetime_accepts() {
        // Each verified against `exiftool -ExifIFD:DateTimeOriginal=<v>`.
        let expected = "2024:01:15 10:30:00";
        for raw in [
            "2024:01:15 10:30:00",
            "2024-01-15 10:30:00",
            "2024-01-15T10:30:00",
            "2024:01:15 10:30",
            "20240115103000",
            "2024:01:15 10:30:00.25",
            "2024:01:15 10:30:00+05:00",
            "2024:01:15 10:30:00-0500",
            "2024:01:15 10:30:00Z",
        ] {
            let TagValue::DateTime(dt) = parse("EXIF:DateTimeOriginal", raw).unwrap() else {
                panic!("{raw} did not parse as a DateTime");
            };
            assert_eq!(
                crate::core::date_shift::format_exif_datetime(&dt),
                expected,
                "input {raw}"
            );
        }
    }

    #[test]
    fn datetime_rejects_what_inversedatetime_rejects() {
        let cases = [
            ("not-a-date", "Invalid date/time"),
            ("2024:01:15", "Invalid date/time"), // date alone: only mm,dd -> < 3 parts
            ("2024:13:15 10:30:00", "Month '13' out of range 1..12"),
            ("2024:01:32 10:30:00", "Day '32' out of range 1..31"),
            ("2024:01:15 25:30:00", "Hour '25' out of range 0..24"),
            ("2024:01:15 10:75:00", "Minutes '75' out of range 0..59"),
        ];
        for (raw, want) in cases {
            let err = parse("EXIF:DateTimeOriginal", raw).unwrap_err().to_string();
            assert!(err.contains(want), "input {raw}: got {err}");
        }
    }

    #[test]
    fn datetime_refuses_dates_chrono_cannot_represent() {
        // ExifTool allows day 31 in February and hour 24; chrono cannot store
        // either, so oxidex refuses instead of storing a different instant.
        for raw in ["2024:02:31 10:30:00", "2024:01:15 24:00:00"] {
            let err = parse("EXIF:DateTimeOriginal", raw).unwrap_err().to_string();
            assert!(
                err.contains("not a representable date/time"),
                "{raw}: {err}"
            );
        }
    }

    #[test]
    fn datetime_accepts_now() {
        assert!(matches!(
            parse("EXIF:DateTimeOriginal", "now").unwrap(),
            TagValue::DateTime(_)
        ));
    }

    #[test]
    fn offset_time_uses_exiftool_inverse_offset_time() {
        for (raw, expected) in [
            ("Z", "+00:00"),
            ("+5:30", "+05:30"),
            ("-0830", "-08:30"),
            ("2024:01:15 10:30:00+12:45", "+12:45"),
        ] {
            assert_eq!(
                parse("ExifIFD:OffsetTime", raw).unwrap(),
                TagValue::String(expected.to_string()),
                "input {raw}"
            );
        }
        assert!(parse("ExifIFD:OffsetTime", "UTC").is_err());
    }

    #[test]
    fn derived_offset_times_use_exiftool_inverse_offset_time() {
        for tag in ["ExifIFD:OffsetTimeOriginal", "ExifIFD:OffsetTimeDigitized"] {
            assert_eq!(
                parse(tag, "2024:01:15 10:30:00+12:45").unwrap(),
                TagValue::String("+12:45".to_string()),
                "{tag}"
            );
            assert!(parse(tag, "UTC").is_err(), "{tag}");
        }
    }

    #[test]
    fn bare_create_date_uses_its_declared_datetime_type() {
        let TagValue::DateTime(dt) = parse("CreateDate", "2024:02:03 04:05:06").unwrap() else {
            panic!("bare CreateDate did not parse as DateTime");
        };
        assert_eq!(
            crate::core::date_shift::format_exif_datetime(&dt),
            "2024:02:03 04:05:06"
        );
    }

    // -- String / unknown tags ---------------------------------------------

    #[test]
    fn string_tags_pass_through_untouched() {
        assert_eq!(
            parse("EXIF:Artist", "800").unwrap(),
            TagValue::String("800".to_string())
        );
        assert_eq!(
            parse("IFD0:Software", "1/250").unwrap(),
            TagValue::String("1/250".to_string())
        );
    }

    #[test]
    fn gps_dest_distance_ref_display_value_is_inverted_to_its_exif_code() {
        // GPS.pm 0x0019 PrintConv maps raw ASCII "K" to "Kilometers".
        // The CLI must serialize the code, not the display label.
        assert_eq!(
            parse("GPS:GPSDestDistanceRef", "Kilometers").unwrap(),
            TagValue::String("K".to_string())
        );
    }

    #[test]
    fn gps_dest_latitude_preserves_decimal_input_for_exact_dms_conversion() {
        assert_eq!(
            parse("GPS:GPSDestLatitude", "37.7749").unwrap(),
            TagValue::Rational {
                numerator: 377_749,
                denominator: 10_000,
            }
        );
    }

    #[test]
    fn gps_latitude_ref_uses_the_declared_print_conversion_inverse() {
        assert_eq!(
            parse("GPS:GPSLatitudeRef", "South").unwrap(),
            TagValue::String("S".to_string())
        );
        assert_eq!(
            parse("GPSLatitudeRef", "12.5").unwrap(),
            TagValue::String("N".to_string())
        );
        assert_eq!(
            parse("GPSLatitudeRef", "-12.5").unwrap(),
            TagValue::String("S".to_string())
        );
    }

    #[test]
    fn gps_dest_latitude_ref_uses_the_declared_print_conversion_inverse() {
        assert_eq!(
            parse("GPS:GPSDestLatitudeRef", "South").unwrap(),
            TagValue::String("S".to_string())
        );
        assert_eq!(
            parse("GPSDestLatitudeRef", "+12.5").unwrap(),
            TagValue::String("N".to_string())
        );
        assert_eq!(
            parse("GPSDestLatitudeRef", "-12.5").unwrap(),
            TagValue::String("S".to_string())
        );
    }

    #[test]
    fn bare_gps_dest_bearing_uses_its_declared_rational_type() {
        assert_eq!(
            parse("GPSDestBearing", "90.5").unwrap(),
            TagValue::Rational {
                numerator: 181,
                denominator: 2,
            }
        );
    }

    #[test]
    fn gps_version_id_accepts_exiftool_component_separators() {
        for (tag, input) in [
            ("GPS:GPSVersionID", "2.3.0.0"),
            ("GPS:GPSVersionID", "2 3 0 0"),
            ("GPS:GPSVersionID", "2. 3.0 0"),
            ("GPSVersionID", "2.3.0.0"),
        ] {
            assert_eq!(
                parse(tag, input).unwrap(),
                TagValue::Binary(vec![2, 3, 0, 0]),
                "{tag}={input}"
            );
        }
    }

    /// The `EncodeExifText` tags keep the caller's text: its header and
    /// UTF-16 byte order depend on the EXIF block the writer lands it in
    /// (`writers::exif_text`), so a `Binary` built here -- the old
    /// `ASCII\0\0\0` + UTF-8 over any text -- was wrong for non-ASCII.
    #[test]
    fn exif_text_tags_are_parsed_as_the_callers_text() {
        for tag in [
            "GPS:GPSAreaInformation",
            "GPS:GPSProcessingMethod",
            "ExifIFD:UserComment",
            "UserComment",
        ] {
            for value in ["San Francisco", "café", ""] {
                assert_eq!(
                    parse(tag, value).unwrap(),
                    TagValue::new_string(value),
                    "{tag}={value}"
                );
            }
        }
    }

    #[test]
    fn file_source_printed_values_are_inverted_to_undef_bytes() {
        for (printed, byte) in [
            ("Film Scanner", 1),
            ("Reflection Print Scanner", 2),
            ("Digital Camera", 3),
        ] {
            for tag in ["EXIF:FileSource", "ExifIFD:FileSource"] {
                assert_eq!(parse(tag, printed).unwrap(), TagValue::Binary(vec![byte]));
            }
        }
        assert_eq!(
            parse("FileSource", "Digital Camera").unwrap(),
            TagValue::Binary(vec![3])
        );
        assert!(parse("FileSource", "3").is_err());
    }

    /// Exif.pm 0xa301: pinned 13.59 `-ExifIFD:SceneType="Directly
    /// photographed"` stores the byte 1.
    #[test]
    fn scene_type_printed_value_is_inverted_to_its_undef_byte() {
        for tag in ["SceneType", "EXIF:SceneType", "ExifIFD:SceneType"] {
            assert_eq!(
                parse(tag, "Directly photographed").unwrap(),
                TagValue::Binary(vec![1])
            );
        }
        assert!(parse("ExifIFD:SceneType", "Unknown").is_err());
    }

    /// Writer.pl `ReverseLookup`: `Unknown (X)` is X (pinned 13.59 copies
    /// Ricoh2.jpg's `GPSDestDistanceRef: Unknown ()` as the empty string).
    #[test]
    fn an_unknown_gps_reference_label_is_its_raw_value() {
        assert_eq!(
            parse("GPS:GPSDestDistanceRef", "Unknown ()").unwrap(),
            TagValue::String(String::new())
        );
        assert_eq!(
            parse("GPS:GPSLongitudeRef", "Unknown (Q)").unwrap(),
            TagValue::String("Q".into())
        );
    }

    /// 3.614421976e-10 needs the unsigned range (t/images/GoPro.jpg's
    /// ExposureIndex, which pinned 13.59 copies as 1/2766695617).
    /// GPS.pm 0x001d PrintConvInv keeps the date part; no date is no value.
    #[test]
    fn gps_date_stamp_keeps_the_date_part() {
        assert_eq!(
            parse("GPS:GPSDateStamp", "2024:01:02 10:11:12").unwrap(),
            TagValue::String("2024:01:02".into())
        );
        assert_eq!(
            parse("GPS:GPSDateStamp", "2024:01:02").unwrap(),
            TagValue::String("2024:01:02".into())
        );
        assert!(parse("GPS:GPSDateStamp", "").is_err());
        assert!(parse("GPS:GPSDateStamp", "2024:01:02 10:11:12+02:00").is_err());
    }

    /// PR #957 review (Codex, 4112862931): a hyphen separates the date's
    /// parts unless the value is a full date/time `GetUnixTime` would adjust
    /// to UTC (GPS.pm 13.59 0x001d PrintConvInv).
    #[test]
    fn gps_date_stamp_hyphens_are_separators_unless_a_date_time() {
        for raw in [
            "2024-01-02",
            "2024-01-02T10:11:12",
            "2024-01-02T10:11:12+02:00",
        ] {
            assert_eq!(
                parse("GPS:GPSDateStamp", raw).unwrap(),
                TagValue::String("2024:01:02".into()),
                "{raw}"
            );
        }
        for raw in [
            "2024-01-02 10:11:12",
            "2024:01:02 10:11:12-05:00",
            "2024:01:02 10:11:12+02:00",
            "now",
        ] {
            assert!(parse("GPS:GPSDateStamp", raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn a_cfa_pattern_text_is_refused() {
        assert!(parse("ExifIFD:CFAPattern", "[Blue,Green][Green,Red]").is_err());
    }

    #[test]
    fn a_rational_beyond_the_signed_range_is_refused() {
        assert!(parse("ExifIFD:ExposureIndex", "3.614421976e-10").is_err());
        assert_eq!(
            parse("ExifIFD:ExposureIndex", "100").unwrap(),
            TagValue::Rational {
                numerator: 100,
                denominator: 1
            }
        );
    }

    #[test]
    fn gps_status_printed_value_is_inverted_before_string_serialization() {
        // ExifTool 13.59 GPS.pm 0x0009 PrintConv.
        // Without the inverse conversion, the printable label is written as
        // the ASCII GPSStatus payload instead of the required status byte.
        for (printed, raw) in [("Measurement Active", "A"), ("Measurement Void", "V")] {
            assert_eq!(
                parse("GPS:GPSStatus", printed).unwrap(),
                TagValue::String(raw.to_string()),
                "{printed}"
            );
        }
    }

    #[test]
    fn planar_configuration_printed_values_are_inverted_to_tiff_codes() {
        // ExifTool 13.59 Exif.pm 0x011c declares int16u with this PrintConv.
        // The CLI must apply PrintConvInv before its integer shape check.
        for tag in ["EXIF:PlanarConfiguration", "IFD0:PlanarConfiguration"] {
            for (printed, raw) in [("Chunky", 1), ("Planar", 2)] {
                assert_eq!(
                    parse(tag, printed).unwrap(),
                    TagValue::Integer(raw),
                    "{tag}={printed}"
                );
            }
        }
    }

    #[test]
    fn contrast_uses_exiftools_parameter_inverse_conversion() {
        // ExifTool 13.59 Exif.pm 0xa408 uses ConvertParameter: labels and
        // signed numeric settings map to the three stored int16u codes.
        let cases = [
            ("Normal", 0),
            ("Low", 1),
            ("High", 2),
            ("Soft", 1),
            ("Hard", 2),
            ("-1", 1),
            ("0.00", 0),
            ("+1", 2),
            ("1", 2),
            ("nope", 0),
        ];
        for tag in ["EXIF:Contrast", "ExifIFD:Contrast"] {
            for (printed, raw) in cases {
                assert_eq!(parse(tag, printed).unwrap(), TagValue::Integer(raw));
            }
        }
        assert_eq!(parse("Contrast", "Normal").unwrap(), TagValue::Integer(0));
    }

    #[test]
    fn custom_rendered_printed_values_are_inverted_to_tiff_codes() {
        // ExifTool 13.59 Exif.pm 0xa401, including Apple's declared values.
        let cases = [
            ("Normal", 0),
            ("Custom", 1),
            ("HDR (no original saved)", 2),
            ("HDR (original saved)", 3),
            ("Original (for HDR)", 4),
            ("Panorama", 6),
            ("Portrait HDR", 7),
            ("Portrait", 8),
        ];
        for tag in ["EXIF:CustomRendered", "ExifIFD:CustomRendered"] {
            for (printed, raw) in cases {
                assert_eq!(parse(tag, printed).unwrap(), TagValue::Integer(raw));
            }
        }
        assert_eq!(
            parse("CustomRendered", "Normal").unwrap(),
            TagValue::Integer(0)
        );
        assert!(parse("CustomRendered", "1").is_err());
    }

    #[test]
    fn gain_control_printed_values_are_inverted_to_tiff_codes() {
        // ExifTool 13.59 Exif.pm 0xa407 PrintConv.
        let cases = [
            ("None", 0),
            ("Low gain up", 1),
            ("High gain up", 2),
            ("Low gain down", 3),
            ("High gain down", 4),
        ];
        for tag in ["EXIF:GainControl", "ExifIFD:GainControl"] {
            for (printed, raw) in cases {
                assert_eq!(parse(tag, printed).unwrap(), TagValue::Integer(raw));
            }
        }
        assert_eq!(parse("GainControl", "None").unwrap(), TagValue::Integer(0));
        assert!(parse("GainControl", "1").is_err());
    }

    #[test]
    fn color_space_printed_values_are_inverted_to_tiff_codes() {
        let cases = [
            ("sRGB", 1),
            ("Adobe RGB", 2),
            ("Uncalibrated", 65535),
            ("ICC Profile", 65534),
            ("Wide Gamut RGB", 65533),
        ];
        for tag in ["EXIF:ColorSpace", "ExifIFD:ColorSpace"] {
            for (printed, raw) in cases {
                assert_eq!(parse(tag, printed).unwrap(), TagValue::Integer(raw));
            }
        }
    }

    #[test]
    fn unknown_tags_pass_through_as_strings() {
        assert_eq!(
            parse("Nonexistent:MadeUpTag", "whatever").unwrap(),
            TagValue::String("whatever".to_string())
        );
    }

    // -- the regression this module exists for ------------------------------

    #[test]
    fn declared_type_is_honoured_not_the_value_shape() {
        // The old batch-mode heuristic typed by the *value*: "800" became an
        // Integer whatever the tag was, and "Ada" a String whatever the tag
        // was. Both directions must now follow the tag.
        assert!(matches!(
            parse("EXIF:ISO", "800").unwrap(),
            TagValue::Integer(800)
        ));
        assert!(matches!(
            parse("IFD0:Artist", "800").unwrap(),
            TagValue::String(_)
        ));
        assert!(matches!(
            parse("IFD0:XResolution", "300").unwrap(),
            TagValue::Rational { .. }
        ));
    }

    #[test]
    fn unparseable_values_never_fall_back_to_string() {
        for (tag, raw) in [
            ("EXIF:ISO", "hello world"),
            ("EXIF:FNumber", "wide open"),
            ("EXIF:DateTimeOriginal", "yesterday"),
        ] {
            let result = parse(tag, raw);
            assert!(
                result.is_err(),
                "{tag}={raw} produced {:?} instead of an error",
                result.ok()
            );
        }
    }

    #[test]
    fn ycbcr_positioning_accepts_exiftool_print_values() {
        assert_eq!(
            parse("EXIF:YCbCrPositioning", "Centered").unwrap(),
            TagValue::Integer(1)
        );
        assert_eq!(
            parse("EXIF:YCbCrPositioning", "Co-sited").unwrap(),
            TagValue::Integer(2)
        );
    }

    #[test]
    fn maker_note_safety_accepts_exiftool_print_values() {
        // ExifTool 13.59 Exif.pm 0xc635 declares writable int16u with this
        // PrintConv. The display labels must be inverted before integer
        // parsing and TIFF serialization.
        for (printed, raw) in [("Unsafe", 0), ("Safe", 1)] {
            assert_eq!(
                parse("EXIF:MakerNoteSafety", printed).unwrap(),
                TagValue::Integer(raw),
                "{printed}"
            );
        }
    }

    #[test]
    fn page_number_accepts_its_two_unsigned_short_components() {
        // ExifTool 13.59 Exif.pm 0x0129 declares PageNumber as
        // `Writable => 'int16u', Count => 2`. A space-separated CLI value is
        // therefore a pair, not one malformed integer.
        assert_eq!(
            parse("EXIF:PageNumber", "3 17").unwrap(),
            TagValue::new_array(vec![TagValue::new_integer(3), TagValue::new_integer(17)])
        );
    }

    #[test]
    fn composite_image_count_accepts_its_two_unsigned_short_components() {
        assert_eq!(
            parse("ExifIFD:CompositeImageCount", "3 2").unwrap(),
            TagValue::new_array(vec![TagValue::new_integer(3), TagValue::new_integer(2)])
        );
        assert!(parse("ExifIFD:CompositeImageCount", "3").is_err());
        assert!(parse("ExifIFD:CompositeImageCount", "3 2 1").is_err());
        assert!(parse("ExifIFD:CompositeImageCount", "-1 2").is_err());
    }

    #[test]
    fn light_source_uses_the_complete_exiftool_13_59_print_conversion_inverse() {
        // Exif.pm %lightSource: all labels accepted for the writable int16u
        // LightSource tag. The duplicate "Daylight" at code 25 must invert
        // to the first matching code, 1, as ExifTool's PrintConvInv does.
        for (printed, raw) in [
            ("Unknown", 0),
            ("Daylight", 1),
            ("Fluorescent", 2),
            ("Tungsten (Incandescent)", 3),
            ("Flash", 4),
            ("Fine Weather", 9),
            ("Cloudy", 10),
            ("Shade", 11),
            ("Daylight Fluorescent", 12),
            ("Day White Fluorescent", 13),
            ("Cool White Fluorescent", 14),
            ("White Fluorescent", 15),
            ("Warm White Fluorescent", 16),
            ("Standard Light A", 17),
            ("Standard Light B", 18),
            ("Standard Light C", 19),
            ("D55", 20),
            ("D65", 21),
            ("D75", 22),
            ("D50", 23),
            ("ISO Studio Tungsten", 24),
            ("Day White", 26),
            ("Cool White", 27),
            ("White", 28),
            ("Warm White", 29),
            ("Daylight LED", 30),
            ("Day White LED", 31),
            ("Cool White LED", 32),
            ("White LED", 33),
            ("Warm White LED", 34),
            ("Other", 255),
        ] {
            for tag in ["LightSource", "EXIF:LightSource", "ExifIFD:LightSource"] {
                assert_eq!(
                    parse(tag, printed).unwrap(),
                    TagValue::Integer(raw),
                    "{tag}: {printed}"
                );
            }
        }
        for tag in ["LightSource", "EXIF:LightSource", "ExifIFD:LightSource"] {
            assert!(parse(tag, "1").is_err(), "{tag} accepted a raw hash code");
        }
    }

    #[test]
    fn additional_exif_enum_writes_match_pinned_inverse_rules() {
        for (tag, printed, expected) in [
            ("ExposureProgram", "Program AE", 2),
            ("WhiteBalance", "Manual", 1),
            ("SceneCaptureType", "Portrait", 2),
            ("Saturation", "High", 2),
        ] {
            assert_eq!(parse(tag, printed).unwrap(), TagValue::Integer(expected));
        }
        for (tag, raw) in [
            ("ExposureProgram", "2"),
            ("WhiteBalance", "1"),
            ("SceneCaptureType", "2"),
        ] {
            assert!(parse(tag, raw).is_err(), "{tag} accepted raw hash code");
        }
        assert_eq!(parse("Saturation", "-1").unwrap(), TagValue::Integer(1));
    }

    #[test]
    fn sharpness_uses_exiftools_convert_parameter_inverse() {
        // Exif.pm 13.59 0xa40a delegates PrintConvInv to ConvertParameter:
        // Normal/zero -> 0, Soft/Low/negative -> 1, Hard/High/positive -> 2.
        for (printed, raw) in [
            ("Normal", 0),
            ("neutral", 0),
            ("0", 0),
            ("Soft", 1),
            ("Low", 1),
            ("-0.25", 1),
            ("Hard", 2),
            ("High", 2),
            ("0.25", 2),
        ] {
            for tag in ["Sharpness", "EXIF:Sharpness", "ExifIFD:Sharpness"] {
                assert_eq!(
                    parse(tag, printed).unwrap(),
                    TagValue::Integer(raw),
                    "{tag}: {printed}"
                );
            }
        }
        assert!(parse("ExifIFD:Sharpness", "ambiguous").is_err());
    }

    #[test]
    fn bare_digital_zoom_ratio_is_parsed_as_its_writable_exif_rational() {
        // ExifTool 13.59 Exif.pm 0xa404 declares DigitalZoomRatio as a
        // writable rational64u without a PrintConv.  The unqualified CLI
        // name must therefore be resolved to the EXIF descriptor before
        // parsing, rather than being passed to the writer as a String.
        assert_eq!(
            parse("DigitalZoomRatio", "1.5").unwrap(),
            TagValue::Rational {
                numerator: 3,
                denominator: 2,
            }
        );
    }

    #[test]
    fn subject_area_accepts_two_to_four_unsigned_short_components() {
        assert_eq!(
            parse("EXIF:SubjectArea", "3 17 42").unwrap(),
            TagValue::new_array(vec![
                TagValue::new_integer(3),
                TagValue::new_integer(17),
                TagValue::new_integer(42),
            ])
        );
        assert!(parse("EXIF:SubjectArea", "3").is_err());
        assert!(parse("EXIF:SubjectArea", "3 17 42 99 100").is_err());
        assert!(parse("EXIF:SubjectArea", "-1 17").is_err());
    }

    #[test]
    fn subject_location_parses_exactly_two_unsigned_short_components() {
        assert_eq!(
            parse("EXIF:SubjectLocation", "3 4").unwrap(),
            TagValue::Array(vec![TagValue::Integer(3), TagValue::Integer(4)])
        );
        assert!(parse("EXIF:SubjectLocation", "3").is_err());
        assert!(parse("EXIF:SubjectLocation", "3 4 5").is_err());
    }

    #[test]
    fn exif_write_parity_addendum_matches_pinned_inverse_rules() {
        for tag in [
            "FlashpixVersion",
            "EXIF:FlashpixVersion",
            "ExifIFD:FlashpixVersion",
        ] {
            assert_eq!(
                parse(tag, "01.00").unwrap(),
                TagValue::Binary(b"0100".to_vec())
            );
            assert!(parse(tag, "1.00").is_err());
        }
        for tag in [
            "CompressedBitsPerPixel",
            "EXIF:CompressedBitsPerPixel",
            "ExifIFD:CompressedBitsPerPixel",
        ] {
            assert!(parse(tag, "-1/2").is_err(), "{tag}");
            assert_eq!(
                parse(tag, "1.5").unwrap(),
                TagValue::Rational {
                    numerator: 3,
                    denominator: 2
                }
            );
        }
        for tag in [
            "SubjectDistanceRange",
            "EXIF:SubjectDistanceRange",
            "ExifIFD:SubjectDistanceRange",
        ] {
            assert_eq!(parse(tag, " close ").unwrap(), TagValue::Integer(2));
            assert!(parse(tag, "2").is_err());
        }
        for tag in [
            "SecurityClassification",
            "EXIF:SecurityClassification",
            "ExifIFD:SecurityClassification",
        ] {
            assert_eq!(parse(tag, "top secret").unwrap(), TagValue::new_string("T"));
            assert!(parse(tag, "T").is_err());
        }
        for tag in [
            "ComponentsConfiguration",
            "EXIF:ComponentsConfiguration",
            "ExifIFD:ComponentsConfiguration",
        ] {
            assert_eq!(
                parse(tag, "YCbCr").unwrap(),
                TagValue::Binary(vec![1, 2, 3, 0])
            );
            assert_eq!(parse(tag, "invalid").unwrap(), TagValue::Binary(vec![0; 4]));
        }
        assert_eq!(
            parse("RelatedSoundFile", "related.wav").unwrap(),
            TagValue::new_string("related.wav")
        );
    }

    #[test]
    fn profile_embed_policy_inverts_pinned_printconv_labels() {
        for (label, code) in [
            ("Allow Copying", 0),
            ("Embed if Used", 1),
            ("Never Embed", 2),
            ("No Restrictions", 3),
        ] {
            assert_eq!(
                parse("IFD0:ProfileEmbedPolicy", label).unwrap(),
                TagValue::Integer(code)
            );
        }
    }

    #[test]
    fn exif_version_writes_four_undefined_ascii_digits() {
        // ExifTool 13.59 Exif.pm 0x9000 accepts dotted versions, removes the
        // dots, and left-pads three digits before writing `undef` bytes.
        for tag in ["ExifVersion", "EXIF:ExifVersion", "ExifIFD:ExifVersion"] {
            assert_eq!(
                parse(tag, "2.31").unwrap(),
                TagValue::Binary(b"0231".to_vec())
            );
            assert_eq!(
                parse(tag, "0231").unwrap(),
                TagValue::Binary(b"0231".to_vec())
            );
            assert!(parse(tag, "23").is_err());
        }
    }

    #[test]
    fn iso_accepts_variable_count_unsigned_short_lists() {
        // Verified against `exiftool -ExifIFD:ISO=<v>` + `-n` read-back,
        // ExifTool 13.59.
        assert_eq!(
            parse("ExifIFD:ISO", "100, 200").unwrap(),
            TagValue::new_array(vec![TagValue::new_integer(100), TagValue::new_integer(200)])
        );
        assert_eq!(
            parse("ExifIFD:ISO", "100 200").unwrap(),
            TagValue::new_array(vec![TagValue::new_integer(100), TagValue::new_integer(200)])
        );
        assert_eq!(
            parse("ExifIFD:ISO", "400").unwrap(),
            TagValue::new_integer(400)
        );
        assert_eq!(
            parse("ExifIFD:ISO", "65535").unwrap(),
            TagValue::new_integer(65535)
        );
        // PrintConvInv is `tr/,//d`, so a comma with no space beside it joins
        // its neighbours rather than separating them: 100200 is one value, and
        // one that does not fit int16u.
        assert!(parse("ExifIFD:ISO", "100,200").is_err());
        // int16u range, both ends.
        assert!(parse("ExifIFD:ISO", "65536").is_err());
        assert!(parse("ExifIFD:ISO", "-1").is_err());
        assert!(parse("ExifIFD:ISO", "-800.5").is_err());
        // CheckValue rounds a fraction only when it is the sole value.
        assert_eq!(
            parse("ExifIFD:ISO", "800.5").unwrap(),
            TagValue::new_integer(801)
        );
        assert!(parse("ExifIFD:ISO", "800.5 900").is_err());
    }

    #[test]
    fn subject_distance_accepts_exiftools_printed_meter_suffix() {
        for tag in [
            "SubjectDistance",
            "EXIF:SubjectDistance",
            "ExifIFD:SubjectDistance",
        ] {
            assert_eq!(
                parse(tag, "1.5 m").unwrap(),
                TagValue::Rational {
                    numerator: 3,
                    denominator: 2,
                }
            );
            assert_eq!(
                parse(tag, "1.5m").unwrap(),
                TagValue::Rational {
                    numerator: 3,
                    denominator: 2,
                }
            );
        }
    }

    /// No date input panics, whatever multibyte character it carries or
    /// wherever: 1fdfbeab split a PDF date at byte 19 (`split_at(19)`) and
    /// the EXIF zone strip sliced `s[end - 3..]`, so `2020:01:02 03:04:0é`
    /// and `€b` aborted the CLI. Every insertion point of each character,
    /// into each base, for a PDF and an EXIF date tag.
    #[test]
    fn no_date_input_panics_on_multibyte_text() {
        let bases = [
            "2020:01:02 03:04:05",
            "2020:01:02 03:04:05+02:00",
            "2020:01:02 03:04:05.25Z",
            "2020:01:02 03:04:05 DST",
            "b",
            "",
        ];
        for base in bases {
            for extra in ["é", "€", "😀", "\u{301}", "ß€"] {
                let boundaries: Vec<usize> = (0..=base.len())
                    .filter(|&i| base.is_char_boundary(i))
                    .collect();
                for at in boundaries {
                    let text = format!("{}{extra}{}", &base[..at], &base[at..]);
                    for tag in ["PDF:CreateDate", "PDF:ModifyDate", "EXIF:DateTimeOriginal"] {
                        let _ = parse(tag, &text);
                    }
                }
            }
        }
        // A multibyte character is a non-digit, as it is to ExifTool's
        // `\d{1,2}` scan: pinned 13.59 writes `2020:01:02 03:04:0é` as
        // `(D:20200102030400)`.
        assert_eq!(
            parse("PDF:CreateDate", "2020:01:02 03:04:0é").unwrap(),
            TagValue::String("2020:01:02 03:04:00".to_string())
        );
    }

    /// `InverseDateTime` keeps a PDF date's zone and sub-seconds: the forms
    /// pinned 13.59 accepts for `-PDF:CreateDate=` on sample.pdf, and the
    /// text they become (the writer then drops the sub-seconds). 1fdfbeab
    /// refused `Z` and `.25+02:00` and every other form but two.
    #[test]
    fn pdf_dates_accept_exiftools_forms() {
        for (raw, text) in [
            ("2020:01:02 03:04:05", "2020:01:02 03:04:05"),
            ("2020:01:02 03:04:05Z", "2020:01:02 03:04:05Z"),
            ("2020:01:02 03:04:05z", "2020:01:02 03:04:05Z"),
            ("2020:01:02 03:04:05.25", "2020:01:02 03:04:05.25"),
            (
                "2020:01:02 03:04:05.25+02:00",
                "2020:01:02 03:04:05.25+02:00",
            ),
            ("2020:01:02 03:04:05.25Z", "2020:01:02 03:04:05.25Z"),
            ("2020:01:02 03:04:05-0530", "2020:01:02 03:04:05-05:30"),
            ("2020:01:02 03:04:05+2", "2020:01:02 03:04:05"),
            ("2020:01:02 03:04", "2020:01:02 03:04:00"),
            ("2020-01-02T03:04:05", "2020:01:02 03:04:05"),
            ("20200102030405", "2020:01:02 03:04:05"),
        ] {
            assert_eq!(
                parse("PDF:CreateDate", raw).unwrap(),
                TagValue::String(text.to_string()),
                "{raw}"
            );
        }
        for raw in ["2020:01:02", "junk", "2020:13:02 03:04:05"] {
            assert!(parse("PDF:CreateDate", raw).is_err(), "{raw}");
        }
    }
}

/// A mechanical audit, run over every writable plain-`IntEnum` tag in the
/// transcribed `Exif::Main`/`GPS::Main` tables (the same universe
/// `breadth_measure.py`'s duplicate-name audit and `parse_enum_tags` cover),
/// proving `exif_or_gps_module_for`'s "veto only on positive evidence" design
/// (see its doc comment) does not regress any of them: a first draft of the
/// gate rejected any tag absent from the hand/generated registry outright,
/// which silently un-fixed `EXIF:ShadingCorrection` (0xa411) and
/// `EXIF:NoiseReduction` (0xa412) -- genuine `Exif::Main` rows with no
/// registry descriptor at all -- caught by running exactly this sweep before
/// shipping the fix.
#[cfg(test)]
mod module_gate_regression_audit {
    use super::*;
    use crate::exiftool_tables::{PrintConv, find_ifd_table};

    #[test]
    fn every_writable_enum_tag_in_exif_or_gps_main_still_passes_the_module_gate() {
        let mut vetoed = Vec::new();
        for (module_str, prefix) in [("Exif", "EXIF"), ("GPS", "GPS")] {
            let table = find_ifd_table(module_str, "Main").expect("table exists");
            for tag in table.tags.iter() {
                if tag.writable.is_none() {
                    continue;
                }
                if !matches!(tag.print_conv, PrintConv::IntEnum(_)) {
                    continue;
                }
                let declared = format!("{prefix}:{}", tag.name);
                if exif_or_gps_module_for(&declared).is_none() {
                    vetoed.push(format!("{declared} (id={:#06x})", tag.id));
                }
            }
        }
        assert!(
            vetoed.is_empty(),
            "these Exif::Main/GPS::Main tags were wrongly vetoed: {vetoed:#?}"
        );
    }

    /// The two tags the sweep above caught during development: absent from
    /// the tag registry entirely, so the gate must trust the group prefix
    /// (`declared_tag_name` starts with `"EXIF:"`, not `"GPS:"`) rather than
    /// treating "no registry entry" as "not Exif::Main".
    #[test]
    fn tags_missing_from_the_registry_still_pass_the_module_gate() {
        assert_eq!(
            exif_or_gps_module_for("EXIF:ShadingCorrection"),
            Some("Exif")
        );
        assert_eq!(exif_or_gps_module_for("EXIF:NoiseReduction"), Some("Exif"));
    }
}
