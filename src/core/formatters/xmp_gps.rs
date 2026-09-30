//! XMP-exif GPS coordinate ValueConv and PrintConv from ExifTool's XMP.pm.
//!
//! XMP.pm:224-235 applies `%latConv`/`%longConv` to the four tags below:
//! `GPS::ToDegrees($val, 1)` is the value used by `-n` and composites, and
//! `GPS::ToDMS($self, $val, 1, "N"/"E")` is the printed value. Keep the two
//! forms together so a rounded DMS string never becomes a composite input.

use std::sync::LazyLock;

use regex::Regex;

use super::numeric_precision::perl_number;

static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    // GPS.pm:594: the exponent belongs to a number only when explicitly
    // signed, so `1e2` extracts 1 and 2 while `1e+2` extracts 100.
    Regex::new(r"[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[Ee][+-]\d+)?").expect("GPS number regex is valid")
});
static INVALID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:inf|undef)\b").expect("GPS invalid-value regex is valid"));
static SOUTH_WEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[^a-z](?:s(?:outh)?|w(?:est)?)\s*$").expect("GPS hemisphere regex is valid")
});
static SOUTH_WEST_1178: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[^a-z](?:s|w)$").expect("11.78 GPS hemisphere regex is valid")
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct XmpGpsForms {
    pub value: String,
    pub print: String,
}

/// Whether pinned XMP.pm attaches the GPS coordinate conversions to this row.
/// A same-key copy of such a row uses its stored packet scalar, not DMS text.
pub(crate) fn is_xmp_exif_coordinate(tag: &str) -> bool {
    matches!(
        tag,
        "XMP-exif:GPSLatitude"
            | "XMP-exif:GPSLongitude"
            | "XMP-exif:GPSDestLatitude"
            | "XMP-exif:GPSDestLongitude"
    )
}

/// Returns the two ExifTool forms for the four properties whose XMP-exif
/// table rows actually carry `%latConv` or `%longConv` in pinned 13.59.
pub(crate) fn convert(tag: &str, raw: &str) -> Option<XmpGpsForms> {
    let positive = match tag {
        "XMP-exif:GPSLatitude" | "XMP-exif:GPSDestLatitude" => 'N',
        "XMP-exif:GPSLongitude" | "XMP-exif:GPSDestLongitude" => 'E',
        _ => return None,
    };
    let Some(degrees) = to_degrees(raw, crate::exiftool_tables::EXIFTOOL_VERSION) else {
        // GPS::ToDegrees returns '' when no first number exists (or the
        // source contains a lowercase `inf`/`undef` word). ToDMS keeps it.
        return Some(XmpGpsForms {
            value: String::new(),
            print: String::new(),
        });
    };
    Some(XmpGpsForms {
        value: perl_number(degrees),
        print: to_dms(degrees, positive),
    })
}

fn to_degrees(raw: &str, source_version: &str) -> Option<f64> {
    // GPS.pm:585,593-599. Only the first three numeric captures matter.
    if INVALID.is_match(raw) {
        return None;
    }
    let numbers = NUMBER
        .find_iter(raw)
        .take(3)
        .map(|capture| capture.as_str().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let degrees = *numbers.first()?;
    let minutes = numbers.get(1).copied().unwrap_or(0.0);
    let seconds = numbers.get(2).copied().unwrap_or(0.0);
    let mut result = degrees + (minutes + seconds / 60.0) / 60.0;
    let south_or_west = if source_version == "11.78" {
        SOUTH_WEST_1178.is_match(raw)
    } else {
        SOUTH_WEST.is_match(raw)
    };
    if south_or_west {
        // Perl negates the result; it does not force a negative magnitude.
        // Thus `-43,30S` becomes +42.5 degrees.
        result = -result;
    }
    result.is_finite().then_some(result)
}

fn to_dms(mut degrees: f64, positive: char) -> String {
    // GPS.pm:495-578 with default CoordFormat `%d deg %d' %.2f" N/E`.
    let reference = if degrees < 0.0 {
        degrees = -degrees;
        if positive == 'N' { 'S' } else { 'W' }
    } else {
        positive
    };
    let mut whole_degrees = degrees.trunc();
    // Perl's integer conversion prints zero without a negative sign.
    if whole_degrees == 0.0 {
        whole_degrees = 0.0;
    }
    let mut whole_minutes = ((degrees - whole_degrees) * 60.0).trunc();
    if whole_minutes == 0.0 {
        whole_minutes = 0.0;
    }
    let mut seconds = (degrees - whole_degrees - whole_minutes / 60.0) * 3600.0;
    if seconds == 0.0 {
        seconds = 0.0;
    }
    // The native code sprintfs the final component before its carry check.
    let mut printed_seconds = format!("{seconds:.2}");
    if printed_seconds.parse::<f64>().unwrap_or(0.0) >= 60.0 {
        let carried = printed_seconds.parse::<f64>().unwrap_or(0.0) - 60.0;
        printed_seconds = format!("{carried:.2}");
        whole_minutes += 1.0;
        if whole_minutes >= 60.0 {
            whole_minutes -= 60.0;
            whole_degrees += 1.0;
        }
    }
    format!("{whole_degrees:.0} deg {whole_minutes:.0}' {printed_seconds}\" {reference}")
}

#[cfg(test)]
mod tests {
    use super::convert;

    #[test]
    fn four_xmp_exif_coordinates_have_distinct_value_and_print_forms() {
        let cases = [
            (
                "GPSLatitude",
                "43,30.4233408N",
                "43.50705568",
                "43 deg 30' 25.40\" N",
            ),
            (
                "GPSLongitude",
                "16,26.3012136E",
                "16.43835356",
                "16 deg 26' 18.07\" E",
            ),
            (
                "GPSDestLatitude",
                "43,30.4233408S",
                "-43.50705568",
                "43 deg 30' 25.40\" S",
            ),
            (
                "GPSDestLongitude",
                "16,26.3012136W",
                "-16.43835356",
                "16 deg 26' 18.07\" W",
            ),
        ];
        for (name, raw, value, print) in cases {
            let forms = convert(&format!("XMP-exif:{name}"), raw).unwrap();
            assert_eq!(forms.value, value, "{name}");
            assert_eq!(forms.print, print, "{name}");
        }
        assert!(convert("XMP-drone-dji:GPSLatitude", "43,30N").is_none());
        assert!(convert("XMP:GPSLatitude", "43,30N").is_none());
    }

    #[test]
    fn historical_1178_hemisphere_suffix_does_not_accept_full_word() {
        assert_eq!(super::to_degrees("43,30S", "11.78"), Some(-43.5));
        assert_eq!(super::to_degrees("43,30 South", "11.78"), Some(43.5));
        assert_eq!(super::to_degrees("43,30S ", "11.78"), Some(43.5));
        assert_eq!(super::to_degrees("43,30 South", "12.64"), Some(-43.5));
        assert_eq!(super::to_degrees("43,30 South", "13.59"), Some(-43.5));
    }

    #[test]
    fn source_numeric_extraction_sign_and_dms_carry() {
        let cases = [
            ("-43,30S", "42.5", "42 deg 30' 0.00\" N"),
            ("43,30s", "-43.5", "43 deg 30' 0.00\" S"),
            // ExifTool 11.78 accepts only a single-letter suffix. Its
            // full-word South input remains positive; later pins accept it.
            (
                "43,30 South",
                if crate::exiftool_tables::EXIFTOOL_VERSION == "11.78" {
                    "43.5"
                } else {
                    "-43.5"
                },
                if crate::exiftool_tables::EXIFTOOL_VERSION == "11.78" {
                    "43 deg 30' 0.00\" N"
                } else {
                    "43 deg 30' 0.00\" S"
                },
            ),
            ("-43,30", "-42.5", "42 deg 30' 0.00\" S"),
            ("0S", "0", "0 deg 0' 0.00\" N"),
            ("1e+2N", "100", "100 deg 0' 0.00\" N"),
            ("1e2N", "1.03333333333333", "1 deg 2' 0.00\" N"),
            ("72,59,60N", "73", "73 deg 0' 0.00\" N"),
            ("garbage", "", ""),
            ("inf", "", ""),
        ];
        for (raw, value, print) in cases {
            let forms = convert("XMP-exif:GPSLatitude", raw).unwrap();
            assert_eq!(forms.value, value, "{raw}");
            assert_eq!(forms.print, print, "{raw}");
        }
    }
}
