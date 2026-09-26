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

use crate::core::ValueType;
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use crate::tag_db::tag_registry::{get_tag_descriptor, has_reliable_value_type};
use chrono::{NaiveDate, TimeZone, Timelike, Utc};

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
    let declared_tag_name = declared_alias(tag_name).unwrap_or(tag_name);

    let leaf = declared_tag_name.rsplit(':').next();

    if leaf == Some("FlashpixVersion") {
        let encoded: String = raw.chars().filter(|character| *character != '.').collect();
        if encoded.len() != 4 || !encoded.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid(tag_name, "Error converting value (PrintConvInv)"));
        }
        return Ok(TagValue::Binary(encoded.into_bytes()));
    }

    if leaf == Some("ComponentsConfiguration") {
        return parse_components_configuration(tag_name, raw);
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
    if declared_tag_name.rsplit(':').next() == Some("OffsetTime") {
        let offset = inverse_offset_time(raw).ok_or_else(|| {
            invalid(
                tag_name,
                "Can't convert OffsetTime value to a time zone offset",
            )
        })?;
        return Ok(TagValue::String(offset));
    }

    if declared_tag_name.rsplit(':').next() == Some("OffsetTimeOriginal") {
        let offset = inverse_offset_time(raw).ok_or_else(|| {
            invalid(
                tag_name,
                "Can't convert OffsetTimeOriginal value to a time zone offset",
            )
        })?;
        return Ok(TagValue::String(offset));
    }

    if declared_tag_name.rsplit(':').next() == Some("OffsetTimeDigitized") {
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
            return byte_from_raw_integer(tag_name, raw, "FileSource");
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

    // Exif.pm 0xa301 is writable undef, one byte, with a single-entry
    // PrintConv hash (`{1 => 'Directly photographed'}`). Unregistered in
    // the tag registry, so with no conversion here the label reached the
    // generic `ValueType::Binary` arm below and was stored as its own UTF-8
    // bytes verbatim (`"Directly photographed"`, 21 bytes) instead of the
    // single byte `01` ExifTool writes. The table lookup is the same
    // mechanism as every plain enum tag; only the wrapping differs, because
    // `undef` stores the code as raw bytes, not a TIFF SHORT.
    if declared_tag_name.rsplit(':').next() == Some("SceneType") {
        if raw_mode {
            return byte_from_raw_integer(tag_name, raw, "SceneType");
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
        let normalized = raw.replace('.', " ");
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

    let raw = if matches!(
        tag_name,
        "GPSLatitudeRef" | "GPS:GPSLatitudeRef" | "GPSDestLatitudeRef" | "GPS:GPSDestLatitudeRef"
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
            // Exif.pm 0xa408 uses ConvertParameter as its PrintConvInv rather
            // than a direct label map. It accepts the documented display labels
            // and any signed float, collapsing them to the three stored codes.
            ("Contrast" | "EXIF:Contrast" | "ExifIFD:Contrast", value) => {
                invert_exif_contrast_parameter(value).ok_or_else(|| {
                    invalid(tag_name, "Can't convert Contrast value (not in PrintConv)")
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
            ("GPS:GPSMeasureMode", "2-Dimensional Measurement") => "2",
            ("GPS:GPSMeasureMode", "3-Dimensional Measurement") => "3",
            ("GPS:GPSDestDistanceRef", "Kilometers") => "K",
            ("GPS:GPSDestDistanceRef", "Miles") => "M",
            ("GPS:GPSDestDistanceRef", "Nautical Miles") => "N",
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
    let raw = match declared_tag_name.rsplit(':').next() {
        // Saturation (Exif.pm 0xa409) is ConvertParameter too, same as
        // Contrast above -- `raw_mode` skips it, taking the raw code
        // directly (confirmed against the oracle the same way as Contrast).
        Some("Saturation") if raw_mode => raw,
        Some("Saturation") => invert_exif_contrast_parameter(raw).ok_or_else(|| {
            invalid(
                tag_name,
                "Can't convert Saturation value (not in PrintConv)",
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
        ) if !raw_mode => {
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
        ) if !raw_mode => {
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
        .map(|descriptor| descriptor.value_type());

    match declared {
        None | Some(ValueType::String) => Ok(TagValue::String(raw.to_string())),
        Some(ValueType::Integer) if declared_tag_name.rsplit(':').next() == Some("Sharpness") => {
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
                    )
                ) =>
        {
            let leaf = declared_tag_name
                .rsplit(':')
                .next()
                .unwrap_or(declared_tag_name);
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
        Some(ValueType::Integer) => Ok(TagValue::Integer(parse_integer(tag_name, raw)?)),
        Some(ValueType::Float) => Ok(TagValue::Float(parse_float(tag_name, raw)?)),
        Some(ValueType::Rational)
            if declared_tag_name.rsplit(':').next() == Some("ShutterSpeedValue") =>
        {
            parse_shutter_speed_value(tag_name, raw)
        }
        Some(ValueType::Rational) => parse_rational(declared_tag_name, raw),
        Some(ValueType::DateTime) if declared_tag_name.starts_with("PDF:") => {
            parse_pdf_date(tag_name, raw)
        }
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

fn invert_subsec_time(value: &str) -> Option<&str> {
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(value);
    }
    let (_, fraction) = value.split_once('.')?;
    let end = fraction.bytes().take_while(u8::is_ascii_digit).count();
    (end > 0).then_some(&fraction[..end])
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
    let table = crate::exiftool_tables::find_ifd_table(module, "Main")?;
    let by_id = get_tag_descriptor(declared_tag_name).and_then(|descriptor| {
        let crate::core::TagId::Numeric(id) = descriptor.id() else {
            return None;
        };
        let tag = table.tag(*id)?;
        (tag.name == leaf).then_some(tag)
    });
    let print_conv = by_id
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

/// `raw` as a plain integer (`IsInt`/`IsHex`, not a label), packed as one
/// byte -- the raw-mode counterpart of a PrintConv label lookup for an
/// `undef`-typed one-byte enum tag (`FileSource`, `SceneType`): ExifTool's
/// `#`/`-n` skip `PrintConvInv` but still apply the tag's own `ValueConvInv`
/// (the byte-packing), which is what this reproduces.
fn byte_from_raw_integer(tag_name: &str, raw: &str, leaf: &str) -> Result<TagValue> {
    let value = parse_integer(tag_name, raw)?;
    if !(0..=255).contains(&value) {
        return Err(invalid(
            tag_name,
            format!("{leaf} code does not fit a byte"),
        ));
    }
    Ok(TagValue::Binary(vec![value as u8]))
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
fn parse_rational(tag_name: &str, raw: &str) -> Result<TagValue> {
    if tag_name.rsplit(':').next() == Some("CompressedBitsPerPixel") {
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
    let raw = if tag_name.rsplit_once(':').map_or(tag_name, |(_, name)| name) == "SubjectDistance" {
        raw.strip_suffix('m').map(str::trim_end).unwrap_or(raw)
    } else {
        raw
    };
    // Exif.pm 13.59 0x9202 stores APEX but accepts the displayed F-number:
    // ValueConvInv => '$val>0 ? 2*log($val)/log(2) : 0'. Apply this before
    // the generic rational cases because ExifTool rejects fractions, `inf`
    // and `undef` here rather than storing them directly.
    if tag_name.rsplit_once(':').map_or(tag_name, |(_, name)| name) == "ApertureValue" {
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
    Ok(TagValue::Rational {
        numerator: numerator as i32,
        denominator: denominator as i32,
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
