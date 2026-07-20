#![allow(dead_code)]

use clap::Args;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use crate::types::{ManifestFile, ManifestTag, ResultEntry};

#[derive(Args)]
pub struct RunArgs {
    #[arg(long)]
    pub only_group: Option<String>,
    #[arg(long)]
    pub limit: Option<usize>,
    #[arg(long)]
    pub skip_write: bool,
    #[arg(long, help = "redo READ phase only; merge into existing results.json")]
    pub reread: bool,
    #[arg(long, default_value_t = 8)]
    pub workers: usize,
}

/// Subcommand entry point: load the manifest, run the read + write phases,
/// classify bugs, and write `results.json`. Port of
/// `scripts/jpeg_tag_matrix.py:552-648`'s `main`.
pub fn run(args: RunArgs) -> anyhow::Result<()> {
    let exiftool = std::env::var("EXIFTOOL").unwrap_or_else(|_| "exiftool".into());
    let repo = std::env::current_dir()?;
    let oxidex = std::env::var("OXIDEX")
        .unwrap_or_else(|_| repo.join("target/release/oxidex").display().to_string());
    let work = std::env::var("TAGMATRIX_WORK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("oxidex-tagmap"));
    let base = std::env::var("TAGMATRIX_BASE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| repo.join("tests/fixtures/jpeg/tag_matrix_base.jpg"));
    let results_path = work.join("results.json");
    let tools = Tools {
        exiftool: &exiftool,
        oxidex: &oxidex,
    };

    // Bounds the previously-unbounded rayon parallelism used by both the
    // read phase (Task 7's run_read_phase) and the write phase below.
    rayon::ThreadPoolBuilder::new()
        .num_threads(args.workers)
        .build_global()
        .ok();

    let manifest: ManifestFile = serde_json::from_str(&std::fs::read_to_string(
        work.join("exiftool_jpeg_tags.json"),
    )?)?;
    let mut tags: Vec<ManifestTag> = manifest.tags.into_iter().filter(|t| t.writable).collect();
    if let Some(g) = &args.only_group {
        tags.retain(|t| &t.group == g);
    }
    if let Some(limit) = args.limit {
        tags.truncate(limit);
    }

    let mut results: HashMap<String, ResultEntry> = HashMap::new();
    let mut skip_write = args.skip_write;
    if args.reread && results_path.exists() {
        results = serde_json::from_str(&std::fs::read_to_string(&results_path)?)?;
        skip_write = true;
    }

    let read_res = run_read_phase(&tools, &base, &tags);
    merge_read_phase_results(&mut results, &tags, &read_res);

    if !skip_write {
        let base_ox = oxidex_json(&tools, &base)
            .0
            .unwrap_or_else(|| Value::Object(Default::default()));
        let base_et = exiftool_json(&tools, &base);
        let base_validate_warnings = exiftool_validate_warnings(&tools, &base);
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_validate_warnings,
        };
        let write_results: Vec<(String, ResultEntry)> = tags
            .par_iter()
            .map(|t| (key_of(t), write_test_tag(&tools, &base, t, &ctx)))
            .collect();
        merge_write_phase_results(&mut results, write_results);
    }

    for t in &tags {
        let r = results.entry(key_of(t)).or_default();
        r.group = t.group.clone();
        r.name = t.name.clone();
        r.sample = t.sample.clone().unwrap_or_default();
        r.vtype = Some(t.vtype.clone());
        r.protected = t.protected;
    }

    apply_bug_classification(&mut results);

    std::fs::write(&results_path, serde_json::to_string_pretty(&results)?)?;
    let mut counts: HashMap<(Option<String>, Option<String>), u32> = HashMap::new();
    for r in results.values() {
        *counts.entry((r.read.clone(), r.write.clone())).or_insert(0) += 1;
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for ((rd, wr), n) in sorted {
        println!(
            "  read={:<18} write={:<18} {n}",
            rd.unwrap_or_default(),
            wr.unwrap_or_default()
        );
    }
    println!("Results: {}", results_path.display());
    Ok(())
}

/// Merge the read phase's per-tag results into the accumulated `results`
/// map, clearing stale read-owned fields first. Port of
/// `scripts/jpeg_tag_matrix.py:604-612`.
///
/// `bug_cluster` is dual-owned: the read phase sets it only on the
/// registry-asymmetry fallback branch (`resolve_read`), otherwise the
/// per-tag read result simply omits it. Python's `dict.update()` only
/// overwrites keys that are *present* in the source dict, so an absent
/// `bug_cluster` there leaves whatever was already in `results[key]`
/// untouched (e.g. a value the write phase or a prior --reread pass set).
/// The unconditional-field-assignment translation of that in Rust has to
/// mirror "only overwrite when present" explicitly, hence the `is_some()`
/// guard below -- an unconditional `entry.bug_cluster = r.bug_cluster.clone()`
/// would silently null out a previously-preserved value on every tag that
/// isn't hitting the fallback branch on this particular pass.
fn merge_read_phase_results(
    results: &mut HashMap<String, ResultEntry>,
    tags: &[ManifestTag],
    read_res: &HashMap<String, ResultEntry>,
) {
    for t in tags {
        let entry = results.entry(key_of(t)).or_default();
        // drop stale read fields before merging fresh read results
        *entry = ResultEntry {
            write: entry.write.clone(),
            wkey: entry.wkey.clone(),
            detail: entry.detail.clone(),
            write_ox_val: entry.write_ox_val.clone(),
            write_et_val: entry.write_et_val.clone(),
            write_ox_key: entry.write_ox_key.clone(),
            bug_cluster: entry.bug_cluster.clone(),
            write_quality: entry.write_quality.clone(),
            write_warnings: entry.write_warnings.clone(),
            ..Default::default()
        };
        if let Some(r) = read_res.get(&key_of(t)) {
            entry.read = r.read.clone();
            entry.read_batch = r.read_batch.clone();
            entry.read_detail = r.read_detail.clone();
            entry.read_bug = r.read_bug.clone();
            entry.read_note = r.read_note.clone();
            entry.ox_key = r.ox_key.clone();
            entry.ox_val = r.ox_val.clone();
            entry.et_val = r.et_val.clone();
            if r.bug_cluster.is_some() {
                entry.bug_cluster = r.bug_cluster.clone();
            }
        }
    }
}

/// Merge the write phase's per-tag results into the accumulated `results`
/// map. Port of `scripts/jpeg_tag_matrix.py:619-624`
/// (`results.setdefault(key, {}).update(fut.result())`).
///
/// Every field here except `bug_cluster` is write-phase-owned in both
/// Python and Rust, so an unconditional overwrite is correct for them.
/// `bug_cluster` is dual-owned (see `merge_read_phase_results`): the write
/// phase's per-tag result only carries a `bug_cluster` value on its own
/// registry-asymmetry branch (`write_test_tag`), so like Python's
/// `dict.update()`, we must only overwrite when the write phase actually
/// produced one -- otherwise this would clobber a value the read-phase
/// merge (run moments earlier on the very same `entry`) had just set.
fn merge_write_phase_results(
    results: &mut HashMap<String, ResultEntry>,
    write_results: Vec<(String, ResultEntry)>,
) {
    for (k, wr) in write_results {
        let entry = results.entry(k).or_default();
        entry.write = wr.write;
        entry.wkey = wr.wkey;
        entry.detail = wr.detail;
        entry.write_ox_val = wr.write_ox_val;
        entry.write_et_val = wr.write_et_val;
        entry.write_ox_key = wr.write_ox_key;
        if wr.bug_cluster.is_some() {
            entry.bug_cluster = wr.bug_cluster;
        }
        entry.write_quality = wr.write_quality;
        entry.write_warnings = wr.write_warnings;
    }
}

#[cfg(test)]
mod merge_phase_tests {
    use super::*;
    use crate::types::ManifestTag;

    fn tag(group: &str, name: &str) -> ManifestTag {
        ManifestTag {
            group: group.into(),
            name: name.into(),
            family0: "IPTC".into(),
            writable: true,
            vtype: "int16u".into(),
            protected: false,
            flags: None,
            count: None,
            sample: Some("8".into()),
            sample_is_file: None,
            noop: None,
        }
    }

    /// Regression test for the merge bug found during Task 10's dry run: a
    /// tag whose read phase resolves via the registry-asymmetry fallback
    /// (`resolve_read`'s `bug_cluster: Some("R4-registry-asymmetry")`
    /// branch) must have that `bug_cluster` survive both merge steps, even
    /// though the write phase's own result for that same tag carries no
    /// `bug_cluster` (e.g. `write: NOT_WRITTEN`). Before the fix: the
    /// read-phase merge hardcoded a copy of only 8 named fields and
    /// silently dropped `bug_cluster`, and the write-phase merge
    /// unconditionally assigned `entry.bug_cluster = wr.bug_cluster`
    /// (`None` here), clobbering whatever the read phase had just set.
    #[test]
    fn bug_cluster_from_read_phase_survives_write_phase_merge() {
        let t = tag("IPTC", "BitsPerComponent");
        let key = key_of(&t);

        let mut read_res = HashMap::new();
        read_res.insert(
            key.clone(),
            ResultEntry {
                read: Some("OK".into()),
                ox_key: Some("IPTC:0x0016".into()),
                ox_val: Some("8".into()),
                et_val: Some("8".into()),
                bug_cluster: Some("R4-registry-asymmetry".into()),
                ..Default::default()
            },
        );

        let mut results: HashMap<String, ResultEntry> = HashMap::new();
        merge_read_phase_results(&mut results, std::slice::from_ref(&t), &read_res);
        assert_eq!(
            results[&key].bug_cluster.as_deref(),
            Some("R4-registry-asymmetry"),
            "read-phase merge must copy bug_cluster from the read result"
        );

        let write_results = vec![(
            key.clone(),
            ResultEntry {
                write: Some("NOT_WRITTEN".into()),
                bug_cluster: None,
                ..Default::default()
            },
        )];
        merge_write_phase_results(&mut results, write_results);

        assert_eq!(
            results[&key].bug_cluster.as_deref(),
            Some("R4-registry-asymmetry"),
            "write-phase merge must not clobber a bug_cluster the write phase itself didn't set"
        );
        assert_eq!(results[&key].write.as_deref(), Some("NOT_WRITTEN"));
    }

    /// A tag whose write phase *does* determine its own bug_cluster (its
    /// own registry-asymmetry-on-write branch in `write_test_tag`) must
    /// still have that value applied -- the write-phase guard only skips
    /// the assignment when `wr.bug_cluster` is `None`, not always.
    #[test]
    fn write_phase_bug_cluster_still_applies_when_present() {
        let t = tag("ExifIFD", "ISO");
        let key = key_of(&t);
        let mut results: HashMap<String, ResultEntry> = HashMap::new();
        results.insert(key.clone(), ResultEntry::default());

        let write_results = vec![(
            key.clone(),
            ResultEntry {
                write: Some("OK".into()),
                bug_cluster: Some("R4-registry-asymmetry".into()),
                ..Default::default()
            },
        )];
        merge_write_phase_results(&mut results, write_results);

        assert_eq!(
            results[&key].bug_cluster.as_deref(),
            Some("R4-registry-asymmetry")
        );
    }

    /// On a --reread pass (skip_write forced true, write-phase merge never
    /// runs), a tag that previously had a bug_cluster set (e.g. by an
    /// earlier full run's write phase) but doesn't hit the fallback branch
    /// on this particular read must keep its previously-persisted value --
    /// mirroring Python's `dict.update()`, which never touches a key absent
    /// from the source dict.
    #[test]
    fn read_phase_merge_preserves_prior_bug_cluster_when_read_result_has_none() {
        let t = tag("EXIF", "SomeTag");
        let key = key_of(&t);

        let mut results: HashMap<String, ResultEntry> = HashMap::new();
        results.insert(
            key.clone(),
            ResultEntry {
                bug_cluster: Some("R4-registry-asymmetry".into()),
                ..Default::default()
            },
        );

        let mut read_res = HashMap::new();
        read_res.insert(
            key.clone(),
            ResultEntry {
                read: Some("OK".into()),
                bug_cluster: None,
                ..Default::default()
            },
        );
        merge_read_phase_results(&mut results, std::slice::from_ref(&t), &read_res);

        assert_eq!(
            results[&key].bug_cluster.as_deref(),
            Some("R4-registry-asymmetry"),
            "read-phase merge must not null out a preserved bug_cluster when the fresh read result has none"
        );
    }
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
    [
        "ApertureValue",
        "MaxApertureValue",
        "ShutterSpeedValue",
        "FlashEnergy",
    ]
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
    [
        "Contrast",
        "Saturation",
        "Sharpness",
        "SensingMethod",
        "CustomRendered",
    ]
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

/// Post-process raw harness results into refined read/write categories.
///
/// read=MISMATCH splits into: a tagged real bug (read_bug set, stays
/// MISMATCH), or MISMATCH_FORMAT (value is equivalent; only formatting
/// differs). write=INTEROP_BROKEN gets a bug_cluster label when the specific
/// tag is a previously root-caused case. Port of
/// `scripts/jpeg_tag_matrix.py:240-267`.
pub fn apply_bug_classification(results: &mut HashMap<String, ResultEntry>) {
    for r in results.values_mut() {
        // Independent axes -- a tag can be both read=MISMATCH and
        // write=INTEROP_BROKEN at once, so these must not be if/else'd.
        if r.read.as_deref() == Some("MISMATCH") {
            if let Some(bug) = classify_read_mismatch(r) {
                r.read_bug = Some(bug.to_string());
            } else {
                r.read = Some("MISMATCH_FORMAT".into());
                r.read_note = Some(
                    "value equivalent; oxidex shows stored/raw form, exiftool applies PrintConv"
                        .into(),
                );
            }
        }
        if r.write.as_deref() == Some("INTEROP_BROKEN")
            && r.bug_cluster.is_none()
            && let Some(cluster) = write_bug_cluster_for(&r.name)
        {
            r.bug_cluster = Some(cluster.to_string());
        }
    }
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
        // splitn(2, ..) matches Python's key.split(":", 1)[-1] exactly (split
        // on the first colon only), not split(':').next_back() (last colon) --
        // equivalent for today's single-colon "Group:Name" keys, but this is
        // the byte-faithful form.
        .find(|(key, _)| key.splitn(2, ':').last() == Some(tag.name.as_str()))
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
            if key.starts_with(&prefix) && values_match(sample, &value_to_str(v)) {
                return (Some(key.clone()), Some(v));
            }
        }
    }
    (None, None)
}

// ------------------------------------------------------------- read phase
//
// Subprocess-driving read testing: exiftool writes a sample value into a
// fresh JPEG, then oxidex -j reads it back and the value is compared
// (normalized). Port of `scripts/jpeg_tag_matrix.py:50-88` (subprocess
// wrappers), `:357-420` (read functions), and `:580-613` (two-phase
// batch+individual-retest orchestration).

pub struct Tools<'a> {
    pub exiftool: &'a str,
    pub oxidex: &'a str,
}

/// Run `prog` with `args`, waiting up to `timeout_secs` for it to exit.
/// Returns `(exit_code, stdout, stderr)`. Uses spawn + poll (no
/// `wait_timeout` crate dependency, since this binary otherwise doesn't
/// need one): poll `try_wait` every 20ms, killing the child and returning
/// a synthetic `-1`/`"TIMEOUT"` result if the deadline passes.
///
/// stdout/stderr are drained continuously by two background threads (each
/// blocked on `Read::read_to_string` until the pipe closes at child exit),
/// handed back to this function over a channel once the child exits. This
/// mirrors Python's `Popen.communicate()`, which reads concurrently with
/// waiting: if the child writes more than the OS pipe buffer holds (~64KB)
/// before exiting, and nothing is draining the pipe meanwhile, the child
/// blocks on the full pipe write while this loop just keeps polling
/// `try_wait` (which can never see the child exit, since it's stuck
/// writing) -- a deadlock-adjacent false "TIMEOUT" on what would otherwise
/// be a fast, successful call. Reading only after `try_wait` reports exit
/// (the prior approach) doesn't drain anything until it's too late.
fn run_cmd(prog: &str, args: &[String], timeout_secs: u64) -> (i32, String, String) {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;

    let mut child = match Command::new(prog)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (-2, String::new(), e.to_string()),
    };

    let mut stdout_pipe = child.stdout.take().unwrap();
    let mut stderr_pipe = child.stderr.take().unwrap();
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout_pipe.read_to_string(&mut buf);
        let _ = stdout_tx.send(buf);
    });
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        let _ = stderr_tx.send(buf);
    });

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = stdout_rx.recv().unwrap_or_default();
                let err = stderr_rx.recv().unwrap_or_default();
                return (status.code().unwrap_or(-1), out, err);
            }
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(timeout_secs) {
                    // Python's subprocess.run(timeout=X) kills *and* waits on
                    // TimeoutExpired internally (Popen.communicate() reaps
                    // before re-raising), so the child never lingers as a
                    // zombie. `Child::kill()` alone only sends SIGKILL; the
                    // kernel won't release the process table entry until
                    // something calls wait() on it. Since the child is being
                    // forcibly killed, wait() here cannot block on the
                    // pipes -- it only waits for exit status, matching
                    // Python's behavior and avoiding a defunct process for
                    // however long this run_cmd caller's process keeps running.
                    let _ = child.kill();
                    let _ = child.wait();
                    return (-1, String::new(), "TIMEOUT".into());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return (-2, String::new(), e.to_string()),
        }
    }
}

pub fn exiftool_json(tools: &Tools, path: &Path) -> Value {
    let (code, out, _) = run_cmd(
        tools.exiftool,
        &[
            "-j".into(),
            "-G1".into(),
            "-charset".into(),
            "utf8".into(),
            path.display().to_string(),
        ],
        30,
    );
    if code != 0 || out.trim().is_empty() {
        return Value::Object(Default::default());
    }
    serde_json::from_str::<Vec<Value>>(&out)
        .ok()
        .and_then(|mut v| v.pop())
        .unwrap_or_else(|| Value::Object(Default::default()))
}

pub fn oxidex_json(tools: &Tools, path: &Path) -> (Option<Value>, Option<String>) {
    // -e (exiftool-compat) gives PrintConv-style values closest to exiftool -j -G1
    let (code, out, err) = run_cmd(
        tools.oxidex,
        &["-j".into(), "-e".into(), path.display().to_string()],
        30,
    );
    if code != 0 || out.trim().is_empty() {
        return (None, Some(err));
    }
    match serde_json::from_str::<Vec<Value>>(&out) {
        Ok(mut v) => (v.pop(), None),
        Err(_) => (None, Some("unparseable JSON".into())),
    }
}

fn value_to_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn resolve_read(ox: &Value, tag: &ManifestTag, et_val: &Value) -> ResultEntry {
    let keys = oxidex_read_keys(tag);
    let (k, v) = find_in_json(ox, &keys);
    let et_str = value_to_str(et_val);
    match (k, v) {
        (None, _) => {
            let sample = tag.sample.clone().unwrap_or_default();
            let (fk, fv) = find_same_group_fallback(ox, tag, &sample);
            if let (Some(fk), Some(fv)) = (fk, fv) {
                return ResultEntry {
                    read: Some("OK".into()),
                    ox_key: Some(fk),
                    ox_val: Some(value_to_str(fv)),
                    et_val: Some(et_str),
                    bug_cluster: Some("R4-registry-asymmetry".into()),
                    ..Default::default()
                };
            }
            ResultEntry {
                read: Some("MISSING".into()),
                et_val: Some(et_str),
                ..Default::default()
            }
        }
        (Some(k), Some(v)) => {
            let vs = value_to_str(v);
            let sample = tag.sample.clone().unwrap_or_default();
            if values_match(&et_str, &vs) || values_match(&sample, &vs) {
                ResultEntry {
                    read: Some("OK".into()),
                    ox_key: Some(k),
                    ox_val: Some(vs),
                    et_val: Some(et_str),
                    ..Default::default()
                }
            } else {
                ResultEntry {
                    read: Some("MISMATCH".into()),
                    ox_key: Some(k),
                    ox_val: Some(vs),
                    et_val: Some(et_str),
                    ..Default::default()
                }
            }
        }
        _ => unreachable!("find_in_json returns matching Some/Some or None/None"),
    }
}

pub fn read_test_single(tools: &Tools, base: &Path, tag: &ManifestTag) -> ResultEntry {
    let td = tempfile::tempdir().unwrap();
    let img = td.path().join("t.jpg");
    std::fs::copy(base, &img).unwrap();
    let spec = format!(
        "-{}:{}={}",
        tag.group,
        tag.name,
        tag.sample.as_deref().unwrap_or("")
    );
    run_cmd(
        tools.exiftool,
        &[
            "-m".into(),
            "-q".into(),
            "-overwrite_original".into(),
            spec,
            img.display().to_string(),
        ],
        60,
    );
    let et = exiftool_json(tools, &img);
    let et_val = find_in_exiftool_json(&et, tag, false);
    let Some(et_val) = et_val else {
        return ResultEntry {
            read: Some("NO_SAMPLE".into()),
            ..Default::default()
        };
    };
    let (ox, oxerr) = oxidex_json(tools, &img);
    let Some(ox) = ox else {
        return ResultEntry {
            read: Some("OXIDEX_PARSE_FAIL".into()),
            et_val: Some(value_to_str(et_val)),
            read_detail: oxerr.map(|e| e.chars().take(200).collect()),
            ..Default::default()
        };
    };
    resolve_read(&ox, tag, et_val)
}

// exiftool 13.55 itself serializes this tag with a malformed value offset
// (ASCII "1.5\0" in the offset field), which poisons the whole file for
// oxidex (drops the entire EXIF block) and aborts subsequent exiftool write
// chunks. Excluded from batch writes; tested individually only.
const BATCH_POISON: &[(&str, &str)] = &[("IFD0", "GeoTiffDoubleParams")];

fn key_of(t: &ManifestTag) -> String {
    format!("{}:{}", t.group, t.name)
}

pub fn read_test_group(
    tools: &Tools,
    base: &Path,
    tags: &[ManifestTag],
) -> HashMap<String, ResultEntry> {
    let mut results = HashMap::new();
    let td = tempfile::tempdir().unwrap();
    let img = td.path().join("t.jpg");
    std::fs::copy(base, &img).unwrap();

    let chunk = 80;
    let batch_tags: Vec<&ManifestTag> = tags
        .iter()
        .filter(|t| !BATCH_POISON.contains(&(t.group.as_str(), t.name.as_str())))
        .collect();
    for group in batch_tags.chunks(chunk) {
        let mut args = vec![
            "-m".to_string(),
            "-q".to_string(),
            "-overwrite_original".to_string(),
        ];
        for t in group {
            args.push(format!(
                "-{}:{}={}",
                t.group,
                t.name,
                t.sample.as_deref().unwrap_or("")
            ));
        }
        args.push(img.display().to_string());
        run_cmd(tools.exiftool, &args, 120);
    }

    let et = exiftool_json(tools, &img);
    let (ox, oxerr) = oxidex_json(tools, &img);

    for t in tags {
        let et_val = find_in_exiftool_json(&et, t, false);
        let Some(et_val) = et_val else {
            results.insert(
                key_of(t),
                ResultEntry {
                    read: Some("NO_SAMPLE".into()),
                    ..Default::default()
                },
            );
            continue;
        };
        match &ox {
            None => {
                results.insert(
                    key_of(t),
                    ResultEntry {
                        read: Some("OXIDEX_PARSE_FAIL".into()),
                        et_val: Some(value_to_str(et_val)),
                        read_detail: oxerr.clone().map(|e| e.chars().take(200).collect()),
                        ..Default::default()
                    },
                );
            }
            Some(ox) => {
                results.insert(key_of(t), resolve_read(ox, t, et_val));
            }
        }
    }
    results
}

/// Two-phase read orchestration: one batch write+read per group (in
/// parallel across groups), then an individual retest of every non-OK tag
/// so a poison tag / aborted chunk / mandatory-tag interaction in one
/// group's batch can't misclassify other tags in that batch. Port of
/// `scripts/jpeg_tag_matrix.py:580-613`.
pub fn run_read_phase(
    tools: &Tools,
    base: &Path,
    tags: &[ManifestTag],
) -> HashMap<String, ResultEntry> {
    let mut by_group: HashMap<String, Vec<ManifestTag>> = HashMap::new();
    for t in tags {
        by_group.entry(t.group.clone()).or_default().push(t.clone());
    }
    println!(
        "Testing {} tags across {} groups",
        tags.len(),
        by_group.len()
    );

    // READ phase 1: one batch per group, groups in parallel
    let group_results: Vec<HashMap<String, ResultEntry>> = by_group
        .par_iter()
        .map(|(_, ts)| read_test_group(tools, base, ts))
        .collect();
    let mut read_res: HashMap<String, ResultEntry> = HashMap::new();
    for gr in group_results {
        read_res.extend(gr);
    }
    println!("READ batch phase done");

    // READ phase 2: individually retest every non-OK tag so one poison tag /
    // aborted chunk / mandatory-tag interaction can't contaminate a group.
    let retest: Vec<&ManifestTag> = tags
        .iter()
        .filter(|t| {
            matches!(
                read_res.get(&key_of(t)).and_then(|r| r.read.as_deref()),
                Some("MISSING") | Some("MISMATCH") | Some("NO_SAMPLE") | Some("OXIDEX_PARSE_FAIL")
            )
        })
        .collect();
    println!("READ retest phase: {} tags individually", retest.len());

    let retested: Vec<(String, ResultEntry, Option<String>)> = retest
        .par_iter()
        .map(|t| {
            let mut single = read_test_single(tools, base, t);
            let batch_status = read_res.get(&key_of(t)).and_then(|r| r.read.clone());
            if single.read != batch_status {
                single.read_batch = batch_status.clone();
            }
            (key_of(t), single, batch_status)
        })
        .collect();
    for (k, single, _) in retested {
        read_res.insert(k, single);
    }
    println!("READ phase done");
    read_res
}

// ------------------------------------------------------------ write phase
//
// oxidex writes the tag into a fresh JPEG, then both oxidex -j and
// exiftool -j -G1 read it back and each value is compared against the
// sample -- but only after checking each side against its pristine
// pre-write value (BASE_OX/BASE_ET) to detect a silent no-op (the write
// path exists and exits 0, but the tag family isn't actually wired into
// the writer, so the file comes back byte-identical to the base fixture).
// Port of `scripts/jpeg_tag_matrix.py:440-545`.

static NONSTANDARD_WARNING_MARKERS: &[&str] = &[
    "Non-standard format",
    "Non-standard count",
    "Missing required",
];

/// Return the set of exiftool `-validate` warning lines for a file.
fn exiftool_validate_warnings(tools: &Tools, path: &Path) -> HashSet<String> {
    let (_, out, _) = run_cmd(
        tools.exiftool,
        &[
            "-validate".into(),
            "-warning".into(),
            "-a".into(),
            path.display().to_string(),
        ],
        30,
    );
    out.lines()
        .filter_map(|ln| {
            let (prefix, rest) = ln.split_once(':')?;
            if prefix.contains("Validate") {
                return None;
            }
            Some(rest.trim().to_string())
        })
        .collect()
}

/// Read-only pristine base-fixture values, computed once before the write
/// phase's parallel workers start and shared across them.
pub struct WriteContext<'a> {
    pub base_ox: &'a Value,
    pub base_et: &'a Value,
    pub base_validate_warnings: &'a HashSet<String>,
}

/// oxidex writes the tag -> oxidex reads back -> exiftool reads back.
///
/// Every apparent match/mismatch is checked against the tag's pristine
/// pre-write value in `ctx.base_ox`/`ctx.base_et`: if the post-write value
/// is simply unchanged from what the base fixture already had, this write
/// path is a silent no-op (the family isn't wired into the writer), not a
/// genuine value mismatch -- regardless of whether that stale value
/// happens to coincidentally match or differ from the sample we tried to
/// write. Port of `scripts/jpeg_tag_matrix.py:447-545`'s `write_test_tag`;
/// kept as a single function rather than decomposed further, matching why
/// the Python keeps it as one function (its internal state doesn't factor
/// cleanly into smaller pieces).
pub fn write_test_tag(
    tools: &Tools,
    base: &Path,
    tag: &ManifestTag,
    ctx: &WriteContext,
) -> ResultEntry {
    let base_ox_val = find_in_json(ctx.base_ox, &oxidex_read_keys(tag))
        .1
        .map(value_to_str);
    let base_et_val = find_in_exiftool_json(ctx.base_et, tag, true).map(value_to_str);
    let sample = tag.sample.clone().unwrap_or_default();

    let mut res = ResultEntry {
        write: Some("ERROR".into()),
        detail: Some(String::new()),
        ..Default::default()
    };

    for wkey in oxidex_write_keys(tag) {
        let td = tempfile::tempdir().unwrap();
        let img = td.path().join("t.jpg");
        std::fs::copy(base, &img).unwrap();
        let spec = format!("-{wkey}={sample}");
        let (code, out, err) = run_cmd(tools.oxidex, &[spec, img.display().to_string()], 30);
        let errtext = format!("{err}{out}").trim().to_string();
        if code != 0 || errtext.contains("Error:") {
            res = ResultEntry {
                write: Some("ERROR".into()),
                wkey: Some(wkey),
                detail: Some(errtext.chars().take(200).collect()),
                ..Default::default()
            };
            continue;
        }
        let ox = oxidex_json(tools, &img).0;
        let et = exiftool_json(tools, &img);
        if et.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            res = ResultEntry {
                write: Some("CORRUPTS_FILE".into()),
                wkey: Some(wkey),
                detail: Some("exiftool cannot parse output file".into()),
                ..Default::default()
            };
            continue;
        }
        let et_val = find_in_exiftool_json(&et, tag, true).map(value_to_str);
        let ox_val = ox
            .as_ref()
            .and_then(|ox| find_in_json(ox, &oxidex_read_keys(tag)).1)
            .map(value_to_str);
        let mut ox_key_used: Option<String> = None;

        let sample_eq_base_ox = base_ox_val
            .as_deref()
            .map(|b| b.trim() == sample.trim())
            .unwrap_or(false);
        let sample_eq_base_et = base_et_val
            .as_deref()
            .map(|b| b.trim() == sample.trim())
            .unwrap_or(false);
        let ox_unchanged = ox_val.is_some()
            && base_ox_val.is_some()
            && ox_val.as_deref().unwrap().trim() == base_ox_val.as_deref().unwrap().trim()
            && !sample_eq_base_ox;
        let et_unchanged = et_val.is_some()
            && base_et_val.is_some()
            && et_val.as_deref().unwrap().trim() == base_et_val.as_deref().unwrap().trim()
            && !sample_eq_base_et;

        let mut ox_ok = ox_val.is_some()
            && !ox_unchanged
            && values_match(&sample, ox_val.as_deref().unwrap_or(""));
        let et_ok = et_val.is_some()
            && !et_unchanged
            && values_match(&sample, et_val.as_deref().unwrap_or(""));

        // Registry asymmetry: oxidex has no display name for this tag, but the
        // value landed correctly under a raw/hex key in the same group.
        let mut ox_val = ox_val;
        if !ox_ok
            && et_ok
            && let Some(ox_ref) = &ox
        {
            let (fk, fv) = find_same_group_fallback(ox_ref, tag, &sample);
            if let (Some(fk), Some(fv)) = (fk, fv) {
                ox_key_used = Some(fk);
                ox_val = Some(value_to_str(fv));
                ox_ok = true;
            }
        }

        if ox_ok && et_ok {
            let mut result = ResultEntry {
                write: Some("OK".into()),
                wkey: Some(wkey),
                write_ox_val: ox_val.clone(),
                write_et_val: et_val.clone(),
                ..Default::default()
            };
            if let Some(k) = ox_key_used {
                result.write_ox_key = Some(k);
                result.bug_cluster = Some("R4-registry-asymmetry".into());
            }
            let new_warnings: HashSet<String> = exiftool_validate_warnings(tools, &img)
                .difference(ctx.base_validate_warnings)
                .cloned()
                .collect();
            let real_warnings: Vec<&String> = new_warnings
                .iter()
                .filter(|w| NONSTANDARD_WARNING_MARKERS.iter().any(|m| w.contains(m)))
                .collect();
            if !real_warnings.is_empty() {
                result.write_quality = Some("nonstandard".into());
                let mut sorted: Vec<&str> = real_warnings.iter().map(|s| s.as_str()).collect();
                sorted.sort();
                result.write_warnings = Some(sorted.join("; ").chars().take(200).collect());
            }
            return result;
        }

        if ox_unchanged && et_unchanged {
            res = ResultEntry {
                write: Some("NOT_WRITTEN".into()),
                wkey: Some(wkey),
                detail: Some("silent no-op: value unchanged from pristine base fixture".into()),
                ..Default::default()
            };
            continue;
        }
        if et_ok && !ox_ok {
            res = ResultEntry {
                write: Some("READBACK_BROKEN".into()),
                wkey: Some(wkey),
                detail: Some(format!("exiftool sees {et_val:?}, oxidex sees {ox_val:?}")),
                ..Default::default()
            };
        } else if ox_ok && !et_ok {
            res = ResultEntry {
                write: Some("INTEROP_BROKEN".into()),
                wkey: Some(wkey),
                detail: Some(format!(
                    "oxidex reads back {ox_val:?} but exiftool sees {et_val:?}"
                )),
                ..Default::default()
            };
        } else if ox_val.is_some() || et_val.is_some() {
            res = ResultEntry {
                write: Some("VALUE_MISMATCH".into()),
                wkey: Some(wkey),
                detail: Some(format!(
                    "wrote {sample:?}; oxidex={ox_val:?} exiftool={et_val:?}"
                )),
                ..Default::default()
            };
        } else {
            res = ResultEntry {
                write: Some("NOT_WRITTEN".into()),
                wkey: Some(wkey),
                detail: Some(format!(
                    "exit 0 but tag absent on read-back; stderr: {}",
                    errtext.chars().take(150).collect::<String>()
                )),
                ..Default::default()
            };
        }
    }
    res
}

#[cfg(test)]
mod key_mapping_tests {
    use super::*;
    use serde_json::json;

    fn tag(group: &str, name: &str) -> ManifestTag {
        ManifestTag {
            group: group.into(),
            name: name.into(),
            family0: "EXIF".into(),
            writable: true,
            vtype: "string".into(),
            protected: false,
            flags: None,
            count: None,
            sample: Some("x".into()),
            sample_is_file: None,
            noop: None,
        }
    }

    #[test]
    fn interop_ifd_gets_exif_prefixed_first() {
        let keys = oxidex_read_keys(&tag("InteropIFD", "InteropIndex"));
        assert_eq!(
            keys,
            vec![
                "EXIF:InteropIndex",
                "InteropIFD:InteropIndex",
                "InteropIndex"
            ]
        );
    }

    #[test]
    fn xmp_group_gets_flattened_and_full_variants() {
        let keys = oxidex_read_keys(&tag("XMP-dc", "Creator"));
        assert_eq!(keys, vec!["XMP:Creator", "XMP-dc:Creator", "Creator"]);
    }

    #[test]
    fn photoshop_falls_back_to_iptc() {
        let keys = oxidex_read_keys(&tag("Photoshop", "IPTCDigest"));
        assert_eq!(
            keys,
            vec!["Photoshop:IPTCDigest", "IPTC:IPTCDigest", "IPTCDigest"]
        );
    }

    #[test]
    fn exif_group_write_key_uses_exact_family1_prefix() {
        let keys = oxidex_write_keys(&tag("ExifIFD", "ISO"));
        assert_eq!(keys, vec!["ExifIFD:ISO"]);
    }

    #[test]
    fn find_in_json_returns_first_present_key() {
        let data = json!({"InteropIFD:InteropIndex": "R98"});
        let (k, v) = find_in_json(
            &data,
            &["EXIF:InteropIndex".into(), "InteropIFD:InteropIndex".into()],
        );
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

    /// Regression test: `find_same_group_fallback` used to call
    /// `v.as_str()` directly, which returns `None` for any JSON value that
    /// isn't already a JSON string -- so a candidate emitted as a JSON
    /// number (e.g. real `oxidex -j -e` output for `IPTC:BitsPerComponent`)
    /// was silently skipped even when its stringified form matched the
    /// sample. This mirrors the confirmed real-world case where
    /// `IPTC:ApplicationRecordVersion`'s sample `"3"` should coincidentally
    /// match a same-group `BitsPerComponent` candidate serialized as the
    /// JSON integer `3`.
    #[test]
    fn find_same_group_fallback_matches_json_numeric_value() {
        let data = json!({"IPTC:BitsPerComponent": 3});
        let t = tag("IPTC", "ApplicationRecordVersion");
        let (k, v) = find_same_group_fallback(&data, &t, "3");
        assert_eq!(k.as_deref(), Some("IPTC:BitsPerComponent"));
        assert_eq!(v, Some(&json!(3)));
    }
}

#[cfg(test)]
mod read_phase_tests {
    use super::*;
    use crate::types::ManifestTag;

    #[test]
    fn read_test_group_marks_readable_tag_ok() {
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("base.jpg");
        std::fs::write(&base, b"fake").unwrap();
        // fake-exiftool-read.sh: `-j -G1 ...` prints a fixed JSON with the tag present.
        // fake-oxidex-read.sh: `-j -e ...` prints matching JSON.
        let tools = Tools {
            exiftool: "tests/fixtures/jpeg-tag-matrix/fake-exiftool-read.sh",
            oxidex: "tests/fixtures/jpeg-tag-matrix/fake-oxidex-read.sh",
        };
        let tag = ManifestTag {
            group: "ExifIFD".into(),
            name: "ISO".into(),
            family0: "EXIF".into(),
            writable: true,
            vtype: "int16u".into(),
            protected: false,
            flags: None,
            count: None,
            sample: Some("200".into()),
            sample_is_file: None,
            noop: None,
        };
        let res = read_test_group(&tools, &base, std::slice::from_ref(&tag));
        assert_eq!(res["ExifIFD:ISO"].read.as_deref(), Some("OK"));
    }
}

#[cfg(test)]
mod run_cmd_tests {
    use super::*;

    /// Regression test for a deadlock-adjacent bug: `run_cmd` used to only
    /// read stdout/stderr *after* `try_wait()` reported the child had
    /// exited. If a child writes more than the OS pipe buffer holds
    /// (~64KB on macOS/Linux) before exiting, and nothing is draining the
    /// pipe while it runs, the child blocks on the full pipe write and
    /// `try_wait()` never observes an exit -- producing a spurious TIMEOUT
    /// on what should be a fast, successful call. This spawns a child that
    /// writes ~200KB to stdout (well over any realistic pipe buffer size)
    /// before exiting, with a timeout far longer than the child needs, and
    /// asserts the full output comes back with no timeout.
    #[test]
    fn large_stdout_does_not_deadlock_or_timeout() {
        let n: usize = 200_000;
        let (code, out, err) = run_cmd("sh", &["-c".into(), format!("yes | head -c {n}")], 10);
        assert_eq!(code, 0, "expected clean exit, got stderr={err:?}");
        assert_eq!(
            out.len(),
            n,
            "expected full {n}-byte output to be captured, got {} bytes",
            out.len()
        );
        assert_ne!(
            err, "TIMEOUT",
            "run_cmd incorrectly reported a timeout on large stdout output"
        );
    }
}

#[cfg(test)]
mod write_phase_tests {
    use super::*;
    use crate::types::ManifestTag;
    use serde_json::json;

    /// All scenarios below share group=ExifIFD name=ISO sample="400" and a
    /// pristine base fixture value of "100", so each fake-tool script only
    /// has to encode one canned post-write JSON response.
    fn iso_tag() -> ManifestTag {
        ManifestTag {
            group: "ExifIFD".into(),
            name: "ISO".into(),
            family0: "EXIF".into(),
            writable: true,
            vtype: "int16u".into(),
            protected: false,
            flags: None,
            count: None,
            sample: Some("400".into()),
            sample_is_file: None,
            noop: None,
        }
    }

    fn base_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("base.jpg");
        std::fs::write(&base, b"fake").unwrap();
        (td, base)
    }

    const FIXTURE_DIR: &str = "tests/fixtures/jpeg-tag-matrix";

    #[test]
    fn ok_write_readback_matches_both_sides() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-ok.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-ok.sh"),
        };
        let base_ox = json!({"ExifIFD:ISO": "100"});
        let base_et = json!({"ExifIFD:ISO": "100"});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("OK"));
        assert_eq!(r.write_ox_val.as_deref(), Some("400"));
        assert_eq!(r.write_et_val.as_deref(), Some("400"));
        assert!(r.bug_cluster.is_none());
        assert!(r.write_quality.is_none());
    }

    #[test]
    fn ok_write_with_nonstandard_validate_warning_sets_write_quality() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-ok.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-ok-warn.sh"),
        };
        let base_ox = json!({"ExifIFD:ISO": "100"});
        let base_et = json!({"ExifIFD:ISO": "100"});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("OK"));
        assert_eq!(r.write_quality.as_deref(), Some("nonstandard"));
        assert!(
            r.write_warnings
                .as_deref()
                .unwrap()
                .contains("Non-standard count")
        );
    }

    #[test]
    fn silent_noop_detected_against_pristine_base_value() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-noop.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-noop.sh"),
        };
        let base_ox = json!({"ExifIFD:ISO": "100"});
        let base_et = json!({"ExifIFD:ISO": "100"});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("NOT_WRITTEN"));
        assert!(r.detail.unwrap().contains("silent no-op"));
    }

    #[test]
    fn oxidex_write_error_reported_verbatim() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-error.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-ok.sh"),
        };
        let base_ox = json!({"ExifIFD:ISO": "100"});
        let base_et = json!({"ExifIFD:ISO": "100"});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("ERROR"));
        assert!(r.detail.unwrap().contains("Error: cannot write tag"));
    }

    #[test]
    fn corrupted_output_file_reported_as_corrupts_file() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-ok.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-empty.sh"),
        };
        let base_ox = json!({"ExifIFD:ISO": "100"});
        let base_et = json!({"ExifIFD:ISO": "100"});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("CORRUPTS_FILE"));
    }

    #[test]
    fn registry_asymmetry_fallback_finds_value_under_raw_key() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-fallback.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-ok.sh"),
        };
        let base_ox = json!({"ExifIFD:ISO": "100"});
        let base_et = json!({"ExifIFD:ISO": "100"});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("OK"));
        assert_eq!(r.write_ox_key.as_deref(), Some("ExifIFD:0x8827"));
        assert_eq!(r.bug_cluster.as_deref(), Some("R4-registry-asymmetry"));
    }

    #[test]
    fn readback_broken_when_exiftool_ok_but_oxidex_missing_no_fallback() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-missing.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-ok.sh"),
        };
        let base_ox = json!({"ExifIFD:ISO": "100"});
        let base_et = json!({"ExifIFD:ISO": "100"});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("READBACK_BROKEN"));
    }

    #[test]
    fn interop_broken_when_oxidex_ok_but_exiftool_disagrees() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-ok.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-mismatch.sh"),
        };
        let base_ox = json!({"ExifIFD:ISO": "100"});
        let base_et = json!({"ExifIFD:ISO": "100"});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("INTEROP_BROKEN"));
    }

    #[test]
    fn value_mismatch_when_both_sides_disagree_with_sample() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-mismatch.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-mismatch.sh"),
        };
        let base_ox = json!({"ExifIFD:ISO": "100"});
        let base_et = json!({"ExifIFD:ISO": "100"});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("VALUE_MISMATCH"));
    }

    /// Distinct from `silent_noop_detected_against_pristine_base_value`: here
    /// there is no pristine base value at all (base_ox/base_et lack the tag
    /// entirely, so base_ox_val/base_et_val are both None -- ox_unchanged
    /// and et_unchanged can never be true). oxidex's write command exits 0
    /// with no "Error:" text, but on read-back both oxidex's and exiftool's
    /// JSON come back with the tag simply absent (not merely unchanged).
    /// This must fall through every other `write_test_tag` branch and land
    /// in the final `else` arm ("exit 0 but tag absent on read-back"), not
    /// the pristine-base no-op arm.
    #[test]
    fn exit_zero_but_tag_absent_on_both_sides_with_no_base_value() {
        let (_td, base) = base_fixture();
        let tools = Tools {
            oxidex: &format!("{FIXTURE_DIR}/fake-oxidex-write-missing.sh"),
            exiftool: &format!("{FIXTURE_DIR}/fake-exiftool-write-tag-absent.sh"),
        };
        let base_ox = json!({});
        let base_et = json!({});
        let base_warnings = HashSet::new();
        let ctx = WriteContext {
            base_ox: &base_ox,
            base_et: &base_et,
            base_validate_warnings: &base_warnings,
        };
        let r = write_test_tag(&tools, &base, &iso_tag(), &ctx);
        assert_eq!(r.write.as_deref(), Some("NOT_WRITTEN"));
        let detail = r.detail.unwrap();
        assert!(
            detail.contains("exit 0 but tag absent on read-back"),
            "detail was: {detail:?}"
        );
        assert!(!detail.contains("silent no-op"), "detail was: {detail:?}");
    }
}

#[cfg(test)]
mod bug_classification_post_process_tests {
    use super::*;
    use crate::types::ResultEntry;

    fn entry() -> ResultEntry {
        ResultEntry {
            group: "EXIF".into(),
            name: "SomeTag".into(),
            sample: "x".into(),
            ..Default::default()
        }
    }

    #[test]
    fn unclassifiable_read_mismatch_becomes_mismatch_format() {
        let mut e = entry();
        e.read = Some("MISMATCH".into());
        e.ox_val = Some("totally different".into());
        e.vtype = Some("string".into());
        let mut results = HashMap::new();
        results.insert("k".to_string(), e);
        apply_bug_classification(&mut results);
        let r = &results["k"];
        assert_eq!(r.read.as_deref(), Some("MISMATCH_FORMAT"));
        assert!(r.read_note.is_some());
        assert!(r.read_bug.is_none());
    }

    #[test]
    fn classifiable_read_mismatch_keeps_mismatch_and_sets_read_bug() {
        let mut e = entry();
        e.name = "ApertureValue".into();
        e.read = Some("MISMATCH".into());
        e.ox_val = Some("4.0".into());
        e.vtype = Some("rational64u".into());
        let mut results = HashMap::new();
        results.insert("k".to_string(), e);
        apply_bug_classification(&mut results);
        let r = &results["k"];
        assert_eq!(r.read.as_deref(), Some("MISMATCH"));
        assert_eq!(r.read_bug.as_deref(), Some("R-apex-missing"));
        assert!(r.read_note.is_none());
    }

    #[test]
    fn interop_broken_gets_bug_cluster_when_name_matches() {
        let mut e = entry();
        e.name = "GPSSpeedRef".into();
        e.write = Some("INTEROP_BROKEN".into());
        let mut results = HashMap::new();
        results.insert("k".to_string(), e);
        apply_bug_classification(&mut results);
        assert_eq!(
            results["k"].bug_cluster.as_deref(),
            Some("I1-no-printconvinv")
        );
    }

    #[test]
    fn interop_broken_does_not_overwrite_existing_bug_cluster() {
        let mut e = entry();
        e.name = "GPSSpeedRef".into();
        e.write = Some("INTEROP_BROKEN".into());
        e.bug_cluster = Some("R4-registry-asymmetry".into());
        let mut results = HashMap::new();
        results.insert("k".to_string(), e);
        apply_bug_classification(&mut results);
        assert_eq!(
            results["k"].bug_cluster.as_deref(),
            Some("R4-registry-asymmetry")
        );
    }

    #[test]
    fn read_and_write_axes_are_classified_independently() {
        let mut e = entry();
        e.name = "GPSSpeedRef".into();
        e.read = Some("MISMATCH".into());
        e.ox_val = Some("unrelated".into());
        e.vtype = Some("string".into());
        e.write = Some("INTEROP_BROKEN".into());
        let mut results = HashMap::new();
        results.insert("k".to_string(), e);
        apply_bug_classification(&mut results);
        let r = &results["k"];
        assert_eq!(r.read.as_deref(), Some("MISMATCH_FORMAT"));
        assert_eq!(r.bug_cluster.as_deref(), Some("I1-no-printconvinv"));
    }
}
