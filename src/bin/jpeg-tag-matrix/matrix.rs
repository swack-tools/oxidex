#![allow(dead_code)]

use clap::Args;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::types::{ManifestTag, ResultEntry};

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

// ------------------------------------------------------------- key mapping
//
// Translate between ExifTool's `group:name` tag identifiers and the various
// key spellings oxidex's CLI/JSON output actually uses. Port of
// `scripts/jpeg_tag_matrix.py:270-351`.

const EXIF_GROUPS: &[&str] = &["IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD", "SubIFD"];

/// Candidate keys under which oxidex -j may expose this exiftool tag.
pub fn oxidex_read_keys(tag: &ManifestTag) -> Vec<String> {
    let (g, n) = (tag.group.as_str(), tag.name.as_str());
    let mut keys = Vec::new();
    if g == "InteropIFD" {
        keys.push(format!("EXIF:{n}"));
        keys.push(format!("InteropIFD:{n}"));
    } else if EXIF_GROUPS.contains(&g) {
        keys.push(format!("{g}:{n}"));
    } else if g.starts_with("XMP") {
        keys.push(format!("XMP:{n}"));
        keys.push(format!("{g}:{n}"));
    } else if g == "IPTC" {
        keys.push(format!("IPTC:{n}"));
    } else if g == "Photoshop" {
        keys.push(format!("Photoshop:{n}"));
        keys.push(format!("IPTC:{n}"));
    } else if g == "JFIF" {
        keys.push(format!("JFIF:{n}"));
    } else {
        keys.push(format!("{g}:{n}"));
    }
    keys.push(n.to_string());
    keys
}

/// Write routing (validator.rs separate_by_ifd) only honors IFD0:/IFD1:/
/// ExifIFD:/GPS:/EXIF: prefixes; EXIF: lands in IFD0 (wrong IFD for ExifIFD
/// tags) so we use the exact family-1 prefix only. Other families are
/// dropped silently -- one spelling suffices to prove NOT_WRITTEN.
pub fn oxidex_write_keys(tag: &ManifestTag) -> Vec<String> {
    let (g, n) = (tag.group.as_str(), tag.name.as_str());
    if EXIF_GROUPS.contains(&g) {
        vec![format!("{g}:{n}")]
    } else if g.starts_with("XMP") {
        vec![format!("XMP:{n}")]
    } else {
        vec![format!("{g}:{n}")]
    }
}

pub fn find_in_json<'a>(data: &'a Value, keys: &[String]) -> (Option<String>, Option<&'a Value>) {
    for k in keys {
        if let Some(v) = data.get(k) {
            return (Some(k.clone()), Some(v));
        }
    }
    (None, None)
}

/// Find tag in exiftool -j -G1 output (exact group:name, then name-only).
///
/// strict_group: require the exact family-1 group, with no bare-name
/// fallback to a different group at all. Used for write-test read-back:
/// without this, a tag we never actually wrote can spuriously "match" an
/// unrelated pre-existing tag of the same bare name in a different group.
pub fn find_in_exiftool_json<'a>(
    data: &'a Value,
    tag: &ManifestTag,
    strict_group: bool,
) -> Option<&'a Value> {
    let k = format!("{}:{}", tag.group, tag.name);
    if let Some(v) = data.get(&k) {
        return Some(v);
    }
    if strict_group {
        return None;
    }
    data.as_object()?
        .iter()
        .find(|(key, _)| key.split(':').next_back() == Some(tag.name.as_str()))
        .map(|(_, v)| v)
}

/// Scan for `sample` under any key sharing this tag's group prefix. Catches
/// write/read registry asymmetries without hardcoding specific tag names.
pub fn find_same_group_fallback<'a>(
    data: &'a Value,
    tag: &ManifestTag,
    sample: &str,
) -> (Option<String>, Option<&'a Value>) {
    let prefix = format!("{}:", tag.group);
    if let Some(obj) = data.as_object() {
        for (key, v) in obj {
            if key.starts_with(&prefix)
                && v.as_str().map(|s| values_match(sample, s)).unwrap_or(false)
            {
                return (Some(key.clone()), Some(v));
            }
        }
    }
    (None, None)
}

#[cfg(test)]
mod key_mapping_tests {
    use super::*;
    use serde_json::json;

    fn tag(group: &str, name: &str) -> ManifestTag {
        ManifestTag {
            group: group.into(), name: name.into(), family0: "EXIF".into(),
            writable: true, vtype: "string".into(), protected: false,
            flags: None, count: None, sample: Some("x".into()),
            sample_is_file: None, noop: None,
        }
    }

    #[test]
    fn interop_ifd_gets_exif_prefixed_first() {
        let keys = oxidex_read_keys(&tag("InteropIFD", "InteropIndex"));
        assert_eq!(keys, vec!["EXIF:InteropIndex", "InteropIFD:InteropIndex", "InteropIndex"]);
    }

    #[test]
    fn xmp_group_gets_flattened_and_full_variants() {
        let keys = oxidex_read_keys(&tag("XMP-dc", "Creator"));
        assert_eq!(keys, vec!["XMP:Creator", "XMP-dc:Creator", "Creator"]);
    }

    #[test]
    fn photoshop_falls_back_to_iptc() {
        let keys = oxidex_read_keys(&tag("Photoshop", "IPTCDigest"));
        assert_eq!(keys, vec!["Photoshop:IPTCDigest", "IPTC:IPTCDigest", "IPTCDigest"]);
    }

    #[test]
    fn exif_group_write_key_uses_exact_family1_prefix() {
        let keys = oxidex_write_keys(&tag("ExifIFD", "ISO"));
        assert_eq!(keys, vec!["ExifIFD:ISO"]);
    }

    #[test]
    fn find_in_json_returns_first_present_key() {
        let data = json!({"InteropIFD:InteropIndex": "R98"});
        let (k, v) = find_in_json(&data, &["EXIF:InteropIndex".into(), "InteropIFD:InteropIndex".into()]);
        assert_eq!(k.as_deref(), Some("InteropIFD:InteropIndex"));
        assert_eq!(v, Some(&json!("R98")));
    }

    #[test]
    fn find_in_exiftool_json_strict_group_has_no_bare_name_fallback() {
        let data = json!({"ExifIFD:ColorSpace": "1"});
        let t = tag("XMP-exif", "ColorSpace");
        assert_eq!(find_in_exiftool_json(&data, &t, true), None);
        assert_eq!(find_in_exiftool_json(&data, &t, false), Some(&json!("1")));
    }
}
