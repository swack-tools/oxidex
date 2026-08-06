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

/// Parses `raw` into the value type `tag_name` declares in the tag registry.
///
/// Tags with no registry entry — or whose registry type metadata is flagged
/// unreliable — have no declared type to honour, so their value is passed
/// through as a string; the write path validates those with intrinsic checks
/// only (`core::validation::validate_tag_value_intrinsics`).
pub fn parse_cli_tag_value(tag_name: &str, raw: &str) -> Result<TagValue> {
    let declared_tag_name = match tag_name {
        "GPSDestBearing" => "GPS:GPSDestBearing",
        "DateTimeOriginal" => "EXIF:DateTimeOriginal",
        "CreateDate" => "ExifIFD:CreateDate",
        "ExposureTime" => "EXIF:ExposureTime",
        "BrightnessValue" => "EXIF:BrightnessValue",
        "LightSource" => "EXIF:LightSource",
        "DigitalZoomRatio" => "EXIF:DigitalZoomRatio",
        "Sharpness" => "EXIF:Sharpness",
        "Contrast" => "EXIF:Contrast",
        "CustomRendered" => "EXIF:CustomRendered",
        "GainControl" => "EXIF:GainControl",
        "FileSource" => "EXIF:FileSource",
        "ExposureProgram" => "EXIF:ExposureProgram",
        "WhiteBalance" => "EXIF:WhiteBalance",
        "SceneCaptureType" => "EXIF:SceneCaptureType",
        "Saturation" => "EXIF:Saturation",
        "MeteringMode" => "EXIF:MeteringMode",
        "ShutterSpeedValue" => "ExifIFD:ShutterSpeedValue",
        "ApertureValue" => "EXIF:ApertureValue",
        "Flash" => "ExifIFD:Flash",
        "MakerNoteSafety" => "EXIF:MakerNoteSafety",
        _ => tag_name,
    };

    // GPS.pm 0x001c applies EncodeExifText as its RawConvInv. With the default
    // CharsetEXIF this is the eight-byte ASCII identifier followed by the
    // caller's text; the TIFF field itself remains UNDEFINED.
    if tag_name == "GPS:GPSAreaInformation" {
        let mut encoded = b"ASCII\0\0\0".to_vec();
        encoded.extend_from_slice(raw.as_bytes());
        return Ok(TagValue::Binary(encoded));
    }

    // Exif.pm 0xa300 is writable undef. PrintConvInv accepts the three labels,
    // then ValueConvInv converts a decimal byte to its one-byte TIFF payload.
    if declared_tag_name.rsplit(':').next() == Some("FileSource") {
        let raw = match raw {
            "Film Scanner" => "1",
            "Reflection Print Scanner" => "2",
            "Digital Camera" => "3",
            _ => {
                return Err(invalid(
                    tag_name,
                    "Can't convert FileSource value (not in PrintConv)",
                ));
            }
        };
        return Ok(TagValue::Binary(vec![
            raw.parse::<u8>().expect("known byte"),
        ]));
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

    let raw = if matches!(
        tag_name,
        "GPSLatitudeRef" | "GPS:GPSLatitudeRef" | "GPSDestLatitudeRef" | "GPS:GPSDestLatitudeRef"
    ) {
        invert_gps_latitude_ref(tag_name, raw)?
    } else {
        raw
    };

    // GPS.pm 0x000a: the TIFF value is ASCII "2" or "3", while ExifTool's
    // PrintConv exposes the corresponding measurement label.  Writer.pl
    // applies that PrintConvInv before its generic string check.
    let raw = match (tag_name, raw) {
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
        ("CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered", "Panorama") => "6",
        ("CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered", "Portrait HDR") => {
            "7"
        }
        ("CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered", "Portrait") => "8",
        ("CustomRendered" | "EXIF:CustomRendered" | "ExifIFD:CustomRendered", _) => {
            return Err(invalid(
                tag_name,
                "Can't convert CustomRendered value (not in PrintConv)",
            ));
        }
        // Exif.pm 0xa407 stores int16u values and exposes these PrintConv
        // labels. Invert them before generic integer parsing.
        ("GainControl" | "EXIF:GainControl" | "ExifIFD:GainControl", "None") => "0",
        ("GainControl" | "EXIF:GainControl" | "ExifIFD:GainControl", "Low gain up") => "1",
        ("GainControl" | "EXIF:GainControl" | "ExifIFD:GainControl", "High gain up") => "2",
        ("GainControl" | "EXIF:GainControl" | "ExifIFD:GainControl", "Low gain down") => "3",
        ("GainControl" | "EXIF:GainControl" | "ExifIFD:GainControl", "High gain down") => "4",
        ("GainControl" | "EXIF:GainControl" | "ExifIFD:GainControl", _) => {
            return Err(invalid(
                tag_name,
                "Can't convert GainControl value (not in PrintConv)",
            ));
        }
        ("GPS:GPSStatus", "Measurement Active") => "A",
        ("GPS:GPSStatus", "Measurement Void") => "V",
        ("GPS:GPSMeasureMode", "2-Dimensional Measurement") => "2",
        ("GPS:GPSMeasureMode", "3-Dimensional Measurement") => "3",
        ("GPS:GPSDestDistanceRef", "Kilometers") => "K",
        ("GPS:GPSDestDistanceRef", "Miles") => "M",
        ("GPS:GPSDestDistanceRef", "Nautical Miles") => "N",
        ("GPS:GPSDifferential", "No Correction") => "0",
        ("GPS:GPSDifferential", "Differential Corrected") => "1",
        ("EXIF:PlanarConfiguration" | "IFD0:PlanarConfiguration", "Chunky") => "1",
        ("EXIF:PlanarConfiguration" | "IFD0:PlanarConfiguration", "Planar") => "2",
        // Exif.pm 0x0213 declares this writable int16u tag with these two
        // PrintConv labels. Writer.pl applies the inverse conversion before
        // validating the integer stored in the TIFF entry.
        ("EXIF:YCbCrPositioning", "Centered") => "1",
        ("EXIF:YCbCrPositioning", "Co-sited") => "2",
        // Exif.pm 13.59 0xc635 converts the writable int16u code to these
        // labels. Apply the inverse before the generic integer parser.
        ("MakerNoteSafety" | "EXIF:MakerNoteSafety" | "IFD0:MakerNoteSafety", "Unsafe") => "0",
        ("MakerNoteSafety" | "EXIF:MakerNoteSafety" | "IFD0:MakerNoteSafety", "Safe") => "1",
        _ => raw,
    };
    let raw = match declared_tag_name.rsplit(':').next() {
        Some("ExposureProgram") => match raw {
            "Not Defined" => "0",
            "Manual" => "1",
            "Program AE" => "2",
            "Aperture-priority AE" => "3",
            "Shutter speed priority AE" => "4",
            "Creative (Slow speed)" => "5",
            "Action (High speed)" => "6",
            "Portrait" => "7",
            "Landscape" => "8",
            "Bulb" => "9",
            _ => {
                return Err(invalid(
                    tag_name,
                    "Can't convert ExposureProgram value (not in PrintConv)",
                ));
            }
        },
        Some("WhiteBalance") => match raw {
            "Auto" => "0",
            "Manual" => "1",
            _ => {
                return Err(invalid(
                    tag_name,
                    "Can't convert WhiteBalance value (not in PrintConv)",
                ));
            }
        },
        Some("SceneCaptureType") => match raw {
            "Standard" => "0",
            "Landscape" => "1",
            "Portrait" => "2",
            "Night" => "3",
            "Other" => "4",
            _ => {
                return Err(invalid(
                    tag_name,
                    "Can't convert SceneCaptureType value (not in PrintConv)",
                ));
            }
        },
        Some("Saturation") => invert_exif_contrast_parameter(raw).ok_or_else(|| {
            invalid(
                tag_name,
                "Can't convert Saturation value (not in PrintConv)",
            )
        })?,
        Some("LightSource") => match raw {
            // Exif.pm 13.59 %lightSource PrintConv. Code 25 repeats the
            // "Daylight" label, so its inverse is the first matching code 1.
            "Unknown" => "0",
            "Daylight" => "1",
            "Fluorescent" => "2",
            "Tungsten (Incandescent)" => "3",
            "Flash" => "4",
            "Fine Weather" => "9",
            "Cloudy" => "10",
            "Shade" => "11",
            "Daylight Fluorescent" => "12",
            "Day White Fluorescent" => "13",
            "Cool White Fluorescent" => "14",
            "White Fluorescent" => "15",
            "Warm White Fluorescent" => "16",
            "Standard Light A" => "17",
            "Standard Light B" => "18",
            "Standard Light C" => "19",
            "D55" => "20",
            "D65" => "21",
            "D75" => "22",
            "D50" => "23",
            "ISO Studio Tungsten" => "24",
            "Day White" => "26",
            "Cool White" => "27",
            "White" => "28",
            "Warm White" => "29",
            "Daylight LED" => "30",
            "Day White LED" => "31",
            "Cool White LED" => "32",
            "White LED" => "33",
            "Warm White LED" => "34",
            "Other" => "255",
            _ => {
                return Err(invalid(
                    tag_name,
                    "Can't convert LightSource value (not in PrintConv)",
                ));
            }
        },
        Some("MeteringMode") => match raw {
            "Unknown" => "0",
            "Average" => "1",
            "Center-weighted average" => "2",
            "Spot" => "3",
            "Multi-spot" => "4",
            "Multi-segment" => "5",
            "Partial" => "6",
            "Other" => "255",
            _ => raw,
        },
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
        Some(ValueType::Integer) if declared_tag_name.rsplit(':').next() == Some("Flash") => {
            crate::core::formatters::exif_enums::parse_flash_label(raw)
                .map(TagValue::Integer)
                .ok_or_else(|| invalid(tag_name, "Can't convert Flash value (not in PrintConv)"))
        }
        Some(ValueType::Integer) => Ok(TagValue::Integer(parse_integer(tag_name, raw)?)),
        Some(ValueType::Float) => Ok(TagValue::Float(parse_float(tag_name, raw)?)),
        Some(ValueType::Rational)
            if declared_tag_name.rsplit(':').next() == Some("ShutterSpeedValue") =>
        {
            parse_shutter_speed_value(tag_name, raw)
        }
        Some(ValueType::Rational) => parse_rational(declared_tag_name, raw),
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
    if !strip_timezone_suffix(&mut val) {
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

/// Strips a trailing timezone, reporting whether one was found:
/// `s/([-+])(\d{1,2}):?(\d{2})\s*(DST)?$//i` — `Writer.pl:5018`.
fn strip_timezone_suffix(s: &mut String) -> bool {
    let b = s.as_bytes();
    let mut end = b.len();
    if end >= 3 && s[end - 3..].eq_ignore_ascii_case("DST") {
        end -= 3;
    }
    while end > 0 && b[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    // (\d{2})
    if end < 2 || !b[end - 1].is_ascii_digit() || !b[end - 2].is_ascii_digit() {
        return false;
    }
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
        return false;
    }
    // ([-+])
    if end == 0 || (b[end - 1] != b'-' && b[end - 1] != b'+') {
        return false;
    }
    s.truncate(end - 1);
    true
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

    #[test]
    fn integer_accepts_every_form_checkvalue_accepts() {
        // Verified against `exiftool -ExifIFD:ISO=<v>` + `-n` read-back.
        let cases = [
            ("800", 800),
            ("+800", 800),
            ("800.4", 800),
            ("800.5", 801),
            ("-800.5", -801),
            ("0x320", 800),
            ("320a", 0x320a), // IsHex matches bare hex digits
            ("abc", 0xabc),
            ("1e1", 0x1e1), // IsHex is tried before IsFloat
        ];
        for (raw, want) in cases {
            assert_eq!(
                parse("EXIF:ISO", raw).unwrap(),
                TagValue::Integer(want),
                "input {raw}"
            );
        }
    }

    #[test]
    fn integer_rejects_what_checkvalue_rejects() {
        for raw in ["", "zz", "12:30", "1/2", "hello world"] {
            let err = parse("EXIF:ISO", raw).unwrap_err().to_string();
            assert!(err.contains("Not an integer"), "input {raw}: {err}");
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
            parse("EXIF:MeteringMode", "255").unwrap(),
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

    #[test]
    fn gps_area_information_is_encoded_as_exif_text() {
        assert_eq!(
            parse("GPS:GPSAreaInformation", "San Francisco").unwrap(),
            TagValue::Binary(b"ASCII\0\0\0San Francisco".to_vec())
        );
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
}
