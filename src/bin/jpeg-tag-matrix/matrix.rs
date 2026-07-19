#![allow(dead_code)]

use clap::Args;
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};

use crate::types::ResultEntry;

#[derive(Args)]
pub struct RunArgs {
    #[arg(long)]
    pub only_group: Option<String>,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub skip_write: bool,
    #[arg(long)]
    pub reread: bool,
    #[arg(long, default_value_t = 8)]
    pub workers: usize,
}

pub fn run(_args: RunArgs) -> anyhow::Result<()> {
    Ok(())
}

// ---------------------------------------------------------------- value compare

static RATIONAL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(-?\d+)/(-?\d+)$").unwrap());
static UNIT_SUFFIX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(-?[\d.]+(?:/\d+)?)\s*\D*$").unwrap());
static DATE_LIKE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\d{4}[:-]\d{2}[:-]\d{2}").unwrap());

fn as_float(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Some(caps) = RATIONAL_RE.captures(s) {
        let num: i64 = caps[1].parse().ok()?;
        let den: i64 = caps[2].parse().ok()?;
        if den != 0 {
            return Some(num as f64 / den as f64);
        }
    }
    s.parse::<f64>().ok()
}

fn norm_str(s: &str) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.to_lowercase()
}

fn dnorm(s: &str) -> String {
    let replaced: String = s
        .chars()
        .map(|c| match c {
            '-' | ':' | 't' | 'T' | ' ' => ':',
            other => other,
        })
        .collect();
    let no_tz = replaced.split('+').next().unwrap_or(&replaced);
    no_tz.split('.').next().unwrap_or(no_tz).trim().to_string()
}

/// Lenient comparison: exact, numeric (incl. rationals), date, unit-suffix.
/// Port of `scripts/jpeg_tag_matrix.py:91-148`'s `values_match`. Callers
/// with two real strings in hand call this directly; call sites where
/// either side may be absent should go through `values_match_opt` instead.
pub fn values_match(expected: &str, actual: &str) -> bool {
    let (es, as_) = (expected.trim(), actual.trim());
    if es == as_ {
        return true;
    }
    if norm_str(es) == norm_str(as_) {
        return true;
    }
    let (ef, af) = (as_float(es), as_float(as_));
    if let (Some(ef), Some(af)) = (ef, af) {
        if ef == af {
            return true;
        }
        let denom = ef.abs().max(af.abs()).max(1e-9);
        if (ef - af).abs() / denom < 1e-3 {
            return true;
        }
    }
    // numeric with unit suffix, e.g. "10.5 m" vs "10.5"
    if let Some(ef) = ef
        && let Some(caps) = UNIT_SUFFIX_RE.captures(as_)
        && let Some(af2) = as_float(&caps[1])
        && (ef - af2).abs() / ef.abs().max(1e-9) < 1e-3
    {
        return true;
    }
    if let Some(af) = af
        && let Some(caps) = UNIT_SUFFIX_RE.captures(es)
        && let Some(ef2) = as_float(&caps[1])
        && (af - ef2).abs() / af.abs().max(1e-9) < 1e-3
    {
        return true;
    }
    // single-letter enum abbreviation vs PrintConv expansion ("N" <-> "North")
    if es.chars().count() == 1
        && !as_.is_empty()
        && as_
            .chars()
            .next()
            .unwrap()
            .eq_ignore_ascii_case(&es.chars().next().unwrap())
    {
        return true;
    }
    if as_.chars().count() == 1
        && !es.is_empty()
        && es
            .chars()
            .next()
            .unwrap()
            .eq_ignore_ascii_case(&as_.chars().next().unwrap())
    {
        return true;
    }
    // dates: normalize separators (incl. T vs space), drop subseconds/timezone
    if DATE_LIKE_RE.is_match(es) && dnorm(es) == dnorm(as_) {
        return true;
    }
    false
}

/// Thin wrapper for call sites where either side may be missing (the
/// Python original's `values_match(expected: Optional[str], actual:
/// Optional[str])` returns `False` if either is `None`).
pub fn values_match_opt(expected: Option<&str>, actual: Option<&str>) -> bool {
    match (expected, actual) {
        (Some(e), Some(a)) => values_match(e, a),
        _ => false,
    }
}

// ------------------------------------------------------- bug classification
//
// A raw read=MISMATCH or write=INTEROP_BROKEN result only says "the values
// differ" / "oxidex and exiftool disagree" -- it doesn't say why. The
// patterns and tag-name sets below were derived empirically (diagnosis
// agents reproduced each case against the release binary + exiftool 13.55
// and traced it to specific source locations; see docs/reference/
// jpeg-tag-matrix.md's Known Bugs section) and separate "this is a real,
// specific decoding/encoding bug" from "the value is equivalent, just
// formatted differently than ExifTool" (the latter still counts as
// supported for coverage purposes).

static APEX_TAG_NAMES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["ApertureValue", "MaxApertureValue", "ShutterSpeedValue", "FlashEnergy"]
        .into_iter()
        .collect()
});
static IPTC_BINARY_TAG_NAMES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "ARMIdentifier",
        "ARMVersion",
        "FileFormat",
        "FileVersion",
        "ObjectPreviewFileFormat",
    ]
    .into_iter()
    .collect()
});
static NAMESPACE_BLIND_ENUM_NAMES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["Contrast", "Saturation", "Sharpness", "SensingMethod", "CustomRendered"]
        .into_iter()
        .collect()
});
static XP_INT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d{6,}$").unwrap());
static FLOAT_RAW_BITS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^-?\d{7,}$").unwrap());

/// Root-cause a read=MISMATCH result.
///
/// Returns a `read_bug` id (real, specific bug) or `None` (value is
/// equivalent to ExifTool's; only the presentation format differs). Port of
/// `scripts/jpeg_tag_matrix.py:163-201`'s `classify_read_mismatch`.
pub fn classify_read_mismatch(r: &ResultEntry) -> Option<&'static str> {
    let name = r.name.as_str();
    let group = r.group.as_str();
    let oxs = r.ox_val.clone().unwrap_or_default();
    let sample = r.sample.as_str();
    let vtype = r.vtype.clone().unwrap_or_default();

    if oxs.contains('\u{0}') {
        return Some(if group == "IPTC" {
            "R-iptc-binary-garbage"
        } else {
            "R-binary-garbage"
        });
    }
    if IPTC_BINARY_TAG_NAMES.contains(name) && group == "IPTC" {
        return Some("R-iptc-binary-garbage");
    }
    if APEX_TAG_NAMES.contains(name) {
        return Some("R-apex-missing");
    }
    if group.starts_with("XMP")
        && (oxs.starts_with("Unknown (") || NAMESPACE_BLIND_ENUM_NAMES.contains(name))
    {
        return Some("R-namespace-blind-printconv");
    }
    if oxs.starts_with(&format!("{name}: ")) {
        return Some("R-acr-prefix");
    }
    if oxs.starts_with("(Binary,") {
        return Some("R-undef-not-decoded");
    }
    if name.starts_with("XP") && XP_INT_RE.is_match(&oxs) {
        return Some("R-utf16-not-decoded");
    }
    if (vtype.starts_with("float") || vtype.starts_with("double"))
        && FLOAT_RAW_BITS_RE.is_match(&oxs)
    {
        return Some("R-float-raw-bits");
    }
    if !sample.is_empty() && oxs.matches(sample).count() >= 2 {
        return Some("R-xmp-struct-concat");
    }
    None
}

static WRITE_BUG_CLUSTER_TAG_NAMES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let clusters: &[(&str, &[&str])] = &[
        (
            "I1-no-printconvinv",
            &[
                "GPSSpeedRef",
                "GPSStatus",
                "GPSMeasureMode",
                "GPSDestBearingRef",
                "GPSDestDistanceRef",
                "GPSImgDirectionRef",
                "GPSTrackRef",
                "SecurityClassification",
            ],
        ),
        (
            "I2-wrong-type-enum",
            &[
                "CalibrationIlluminant1",
                "CalibrationIlluminant2",
                "CalibrationIlluminant3",
                "ColorimetricReference",
                "DefaultBlackRender",
                "DepthFormat",
                "DepthMeasureType",
                "DepthUnits",
                "MakerNoteSafety",
                "OldSubfileType",
                "PreviewColorSpace",
                "ProfileEmbedPolicy",
                "ProfileHueSatMapEncoding",
                "ProfileLookTableEncoding",
                "Thresholding",
            ],
        ),
        (
            "I3-wrong-type-numeric",
            &[
                "DNGVersion",
                "DNGBackwardVersion",
                "RawImageDigest",
                "NewRawImageDigest",
                "OriginalRawFileDigest",
                "RawDataUniqueID",
                "TimeCodes",
                "ExposureCompensation",
                "DNGLensInfo",
                "GeoTiffDoubleParams",
            ],
        ),
        (
            "I4-wrong-type-undef",
            &[
                "Padding",
                "GooglePlusUploadCode",
                "CompositeImageExposureTimes",
                "RGBTables",
                "ImageStats",
                "ProfileGainTableMap2",
                "GeoTiffAsciiParams",
            ],
        ),
        (
            "I5-subdir-poison",
            &[
                "CurrentICCProfile",
                "AsShotICCProfile",
                "XiaomiSettings",
                "ImageSequenceInfo",
                "OriginalRawFileData",
                "ProfileDynamicRange",
                "SEAL",
            ],
        ),
    ];
    let mut map = HashMap::new();
    for (cluster, names) in clusters {
        for name in *names {
            map.insert(*name, *cluster);
        }
    }
    map
});

pub fn write_bug_cluster_for(name: &str) -> Option<&'static str> {
    WRITE_BUG_CLUSTER_TAG_NAMES.get(name).copied()
}

#[cfg(test)]
mod value_match_tests {
    use super::*;

    #[test]
    fn exact_match() {
        assert!(values_match("37.7749", "37.7749"));
    }

    #[test]
    fn rational_vs_decimal() {
        assert!(values_match("3/2", "1.5"));
    }

    #[test]
    fn numeric_tolerance() {
        assert!(values_match("1.500001", "1.5"));
    }

    #[test]
    fn unit_suffix() {
        assert!(values_match("10.5", "10.5 m"));
    }

    #[test]
    fn single_letter_enum_abbreviation() {
        assert!(values_match("N", "North"));
        assert!(values_match("North", "N"));
    }

    #[test]
    fn date_separator_normalization() {
        assert!(values_match("2024:01:15 10:30:00", "2024-01-15T10:30:00"));
    }

    #[test]
    fn date_drops_subseconds_and_timezone() {
        assert!(values_match(
            "2024:01:15 10:30:00",
            "2024:01:15 10:30:00.500+05:00"
        ));
    }

    #[test]
    fn whitespace_and_case_normalized() {
        assert!(values_match("Foo  Bar", "foo bar"));
    }

    #[test]
    fn genuinely_different_values_do_not_match() {
        assert!(!values_match("North", "South"));
        assert!(!values_match("3", "4"));
    }

    #[test]
    fn none_never_matches() {
        assert!(!values_match_opt(None, Some("x")));
        assert!(!values_match_opt(Some("x"), None));
    }
}

#[cfg(test)]
mod bug_classification_tests {
    use super::*;
    use crate::types::ResultEntry;

    fn result(name: &str, group: &str, ox_val: &str, sample: &str, vtype: &str) -> ResultEntry {
        ResultEntry {
            name: name.into(),
            group: group.into(),
            ox_val: Some(ox_val.into()),
            sample: sample.into(),
            vtype: Some(vtype.into()),
            read: Some("MISMATCH".into()),
            ..Default::default()
        }
    }

    #[test]
    fn apex_tag_names_flagged() {
        let r = result("ApertureValue", "ExifIFD", "4.0", "4.0", "rational64u");
        assert_eq!(classify_read_mismatch(&r), Some("R-apex-missing"));
    }

    #[test]
    fn nul_byte_in_iptc_flags_binary_garbage() {
        let r = result("SomeIptcTag", "IPTC", "foo\u{0}bar", "x", "string");
        assert_eq!(classify_read_mismatch(&r), Some("R-iptc-binary-garbage"));
    }

    #[test]
    fn xp_utf16_not_decoded() {
        let r = result("XPComment", "ExifIFD", "1234567", "x", "string");
        assert_eq!(classify_read_mismatch(&r), Some("R-utf16-not-decoded"));
    }

    #[test]
    fn unrecognized_mismatch_returns_none() {
        let r = result("SomeNewTag", "EXIF", "totally different", "x", "string");
        assert_eq!(classify_read_mismatch(&r), None);
    }

    #[test]
    fn write_bug_cluster_lookup() {
        assert_eq!(
            write_bug_cluster_for("GPSSpeedRef"),
            Some("I1-no-printconvinv")
        );
        assert_eq!(
            write_bug_cluster_for("DNGVersion"),
            Some("I3-wrong-type-numeric")
        );
        assert_eq!(write_bug_cluster_for("SomeUnclusteredTag"), None);
    }
}
