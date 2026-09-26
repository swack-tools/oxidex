//! CLI value parser: `PrintConvInv` for the EXIF tags whose `PrintConv`
//! appends a literal unit -- FocalLength (Exif.pm 13.59 0x920a),
//! FocalLengthIn35mmFormat (0xa405), SubjectDistance (0x9206) and
//! AmbientTemperature (0x9400). See `src/cli/value_parser.rs`
//! (`strip_printconv_unit_suffix`, and the inline `SubjectDistance` case in
//! `parse_rational`) for the exact `PrintConvInv` each one reproduces, cited
//! by Exif.pm line number.
//!
//! Before this fix, `oxidex "-ExifIFD:FocalLength=50.0 mm" file.jpg` failed
//! `Invalid value for tag 'ExifIFD:FocalLength': Not a floating point
//! number` -- the printed form pinned ExifTool 13.59 itself writes back
//! (`[ExifIFD] FocalLength : 50`) could not be written through oxidex,
//! because `$val=~s/\s*mm$//;$val` (Exif.pm:2426) was never applied. The bug
//! also reproduced at commit bf168506.
//!
//! Graded live against the pinned 13.59 oracle: every test here skips (or,
//! with `OXIDEX_REQUIRE_EXIFTOOL_ORACLE=1`, fails) when
//! [`exiftool_oracle::graded`] finds none available. Each accepted form is
//! written by both oxidex and the oracle to a fresh copy of the same
//! metadata-free fixture (so the write also exercises creating a new
//! ExifIFD from scratch), then both copies are read back through oxidex's
//! own `-j -G1` (the printed form) and `--no-print-conv -j -G1` (ExifTool's
//! `-n`, the raw numeric form) and compared against the oracle's identical
//! read. Refused values and the `#` (raw-mode) suffix are checked the same
//! way: both tools must agree on what is refused.

use oxidex::exiftool_oracle;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// An 8x8 baseline JPEG with no metadata segment at all (the same fixture
/// `tests/xp_string_write.rs` and `tests/cli_non_utf8_args.rs` use):
/// decodable, so the oracle agrees to write it, and with no existing EXIF
/// IFD, so every write below also exercises creating one from scratch --
/// exactly the `-ExifIFD:FocalLength=50.0 mm` reproduction in the bug
/// report, which used an ExifIFD-free image (`t/images/Writer.jpg`).
const BASE_JPEG_HEX: &str = "ffd8ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn write_fixture(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, hex(BASE_JPEG_HEX)).unwrap();
    path
}

fn os(s: &str) -> OsString {
    OsString::from(s)
}

fn run_oxidex(args: &[OsString]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run oxidex {args:?}: {e}"))
}

/// `-j -G1 -TAG` (or `--no-print-conv -j -G1 -TAG`, ExifTool's `-n`) through
/// oxidex: the tag's own JSON value, or `None` if the read produced no
/// matching key.
fn oxidex_read(path: &Path, leaf: &str, raw_mode: bool) -> Option<serde_json::Value> {
    let mut args = Vec::new();
    if raw_mode {
        args.push(os("--no-print-conv"));
    }
    args.push(os("-j"));
    args.push(os("-G1"));
    args.push(os(&format!("-{leaf}")));
    args.push(path.as_os_str().to_owned());
    let out = run_oxidex(&args);
    assert!(
        out.status.success(),
        "oxidex read -{leaf} (raw_mode={raw_mode}) on {path:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    extract(&out.stdout, leaf, "oxidex")
}

/// The same read through the pinned oracle.
fn oracle_read(
    oracle: &exiftool_oracle::Oracle,
    path: &Path,
    leaf: &str,
    raw_mode: bool,
) -> Option<serde_json::Value> {
    let mut cmd = oracle.command();
    if raw_mode {
        cmd.arg("-n");
    }
    cmd.arg("-j").arg("-G1").arg(format!("-{leaf}")).arg(path);
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("failed to run oracle read -{leaf}: {e}"));
    assert!(
        out.status.success(),
        "oracle read -{leaf} (raw_mode={raw_mode}) on {path:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    extract(&out.stdout, leaf, "oracle")
}

fn extract(stdout: &[u8], leaf: &str, who: &str) -> Option<serde_json::Value> {
    let json: serde_json::Value = serde_json::from_slice(stdout)
        .unwrap_or_else(|e| panic!("{who} -j produced invalid JSON: {e}: {stdout:?}"));
    let object = json
        .get(0)
        .unwrap_or_else(|| panic!("{who} -j produced an empty array"))
        .as_object()
        .unwrap_or_else(|| panic!("{who} -j element is not an object"));
    object
        .iter()
        .find(|(key, _)| key.rsplit(':').next() == Some(leaf))
        .map(|(_, value)| value.clone())
}

fn oracle_write(oracle: &exiftool_oracle::Oracle, arg: &str, path: &Path) -> Output {
    oracle
        .command()
        .arg("-overwrite_original")
        .arg(arg)
        .arg(path)
        .output()
        .unwrap_or_else(|e| panic!("failed to run oracle write {arg}: {e}"))
}

/// The four tags this fix covers, each with the CLI tag to write, the leaf
/// name to read back by, and every accepted printed/numeric form Exif.pm
/// 13.59 declares (see the module doc for the exact `PrintConvInv` each
/// reproduces).
const ACCEPTED: &[(&str, &str, &[&str])] = &[
    (
        "ExifIFD:FocalLength",
        "FocalLength",
        &["50.0 mm", "50.0mm", "50"],
    ),
    (
        "ExifIFD:FocalLengthIn35mmFormat",
        "FocalLengthIn35mmFormat",
        &["75 mm", "75"],
    ),
    (
        "ExifIFD:SubjectDistance",
        "SubjectDistance",
        &["3.5 m", "3.5m", "3.5"],
    ),
    (
        "ExifIFD:AmbientTemperature",
        "AmbientTemperature",
        &["20 C", "20C", "20.5 C", "20"],
    ),
];

/// Values that must be refused -- by both tools, so the assumption itself is
/// checked, not just oxidex's own behaviour.
const REFUSED: &[(&str, &[&str])] = &[
    ("ExifIFD:FocalLength", &["50 cm", "fifty mm", "mm"]),
    ("ExifIFD:FocalLengthIn35mmFormat", &["fifty mm", "75 cm"]),
    ("ExifIFD:SubjectDistance", &["3.5 ft", "far m"]),
    ("ExifIFD:AmbientTemperature", &["twenty C", "20 F"]),
];

#[test]
fn printed_and_numeric_forms_round_trip_and_match_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    for (write_tag, leaf, values) in ACCEPTED {
        for value in *values {
            let dir = tempfile::tempdir().unwrap();
            let ours = write_fixture(dir.path(), "ours.jpg");
            let theirs = write_fixture(dir.path(), "theirs.jpg");
            let arg = format!("-{write_tag}={value}");

            let out = run_oxidex(&[os(&arg), ours.as_os_str().to_owned()]);
            assert!(
                out.status.success(),
                "{arg}: oxidex write failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );

            let out = oracle_write(oracle, &arg, &theirs);
            assert!(
                out.status.success(),
                "{arg}: oracle write failed (test assumption wrong): {}",
                String::from_utf8_lossy(&out.stderr)
            );

            for raw_mode in [false, true] {
                let ours_read = oxidex_read(&ours, leaf, raw_mode);
                let theirs_read = oracle_read(oracle, &theirs, leaf, raw_mode);
                assert!(
                    ours_read.is_some(),
                    "{arg} raw_mode={raw_mode}: tag missing from oxidex's own read-back"
                );
                assert_eq!(
                    ours_read, theirs_read,
                    "{arg} raw_mode={raw_mode}: oxidex and the oracle disagree"
                );
            }
        }
    }
}

#[test]
fn invalid_values_are_refused_the_same_way_as_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    for (write_tag, values) in REFUSED {
        for value in *values {
            let dir = tempfile::tempdir().unwrap();
            let ours = write_fixture(dir.path(), "ours.jpg");
            let theirs = write_fixture(dir.path(), "theirs.jpg");
            let arg = format!("-{write_tag}={value}");

            let out = run_oxidex(&[os(&arg), ours.as_os_str().to_owned()]);
            assert!(
                !out.status.success(),
                "{arg}: oxidex must refuse this value (never approximate a PrintConvInv it \
                 cannot reproduce exactly)"
            );

            let out = oracle_write(oracle, &arg, &theirs);
            assert!(
                !out.status.success(),
                "{arg}: the oracle unexpectedly accepted this value (test assumption wrong)"
            );
        }
    }
}

#[test]
fn hash_suffix_bypasses_printconvinv_like_the_oracles_raw_mode() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    // A bare number in raw mode has nothing to strip, so it must still
    // succeed and match the oracle exactly.
    for (write_tag, leaf, _) in ACCEPTED {
        let dir = tempfile::tempdir().unwrap();
        let ours = write_fixture(dir.path(), "ours.jpg");
        let theirs = write_fixture(dir.path(), "theirs.jpg");
        let arg = format!("-{write_tag}#=42");

        let out = run_oxidex(&[os(&arg), ours.as_os_str().to_owned()]);
        assert!(
            out.status.success(),
            "{arg}: oxidex write failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = oracle_write(oracle, &arg, &theirs);
        assert!(
            out.status.success(),
            "{arg}: oracle write failed (test assumption wrong): {}",
            String::from_utf8_lossy(&out.stderr)
        );

        for raw_mode in [false, true] {
            assert_eq!(
                oxidex_read(&ours, leaf, raw_mode),
                oracle_read(oracle, &theirs, leaf, raw_mode),
                "{arg} raw_mode={raw_mode}: oxidex and the oracle disagree"
            );
        }
    }

    // A printed value in raw mode must be refused: raw mode does not run
    // PrintConvInv, so the unit is just part of an invalid number.
    let printed_by_leaf = [
        ("FocalLength", "42 mm"),
        ("FocalLengthIn35mmFormat", "42 mm"),
        ("SubjectDistance", "3.5 m"),
        ("AmbientTemperature", "20 C"),
    ];
    for ((write_tag, leaf, _), (expected_leaf, printed)) in ACCEPTED.iter().zip(printed_by_leaf) {
        assert_eq!(*leaf, expected_leaf);
        let dir = tempfile::tempdir().unwrap();
        let ours = write_fixture(dir.path(), "ours.jpg");
        let theirs = write_fixture(dir.path(), "theirs.jpg");
        let arg = format!("-{write_tag}#={printed}");

        let out = run_oxidex(&[os(&arg), ours.as_os_str().to_owned()]);
        assert!(
            !out.status.success(),
            "{arg}: oxidex accepted a printed value in raw mode"
        );
        let out = oracle_write(oracle, &arg, &theirs);
        assert!(
            !out.status.success(),
            "{arg}: the oracle unexpectedly accepted it too (test assumption wrong)"
        );
    }
}

/// Not oracle-gated: the parse itself, exercised through the real `oxidex`
/// binary rather than `exiftool_oracle`, so this still runs (and would still
/// catch a regression) on a machine with no pinned ExifTool at all.
#[test]
fn covered_tags_reproduce_the_bug_reports_exact_commands() {
    let dir = tempfile::tempdir().unwrap();

    let file = write_fixture(dir.path(), "grouped.jpg");
    let out = run_oxidex(&[
        os("-ExifIFD:FocalLength=50.0 mm"),
        file.as_os_str().to_owned(),
    ]);
    assert!(
        out.status.success(),
        "grouped form: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value = oxidex_read(&file, "FocalLength", false);
    assert_eq!(value, Some(serde_json::json!("50.0 mm")));

    let file = write_fixture(dir.path(), "bare.jpg");
    let out = run_oxidex(&[os("-FocalLength=50.0 mm"), file.as_os_str().to_owned()]);
    // The bare (groupless) form's own write-target resolution is a separate,
    // pre-existing gap unrelated to PrintConvInv (see the PR description);
    // this only pins that the *value* is no longer refused for the reason
    // the bug report named.
    assert!(
        out.status.success(),
        "bare form: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("Not a floating point number"),
        "bare form must not reproduce the reported PrintConvInv failure"
    );
}
