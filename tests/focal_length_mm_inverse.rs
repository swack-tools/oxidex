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

/// Codex review finding on PR #963 (comment 4112327516, P1): the bare-name
/// aliases added to `parse_cli_tag_value`'s type-resolution table let the
/// *value* parse correctly, but `core::operations::canonical_write_tag_name`
/// had no entry for these tags, so the surgical writer could not find a
/// write target for the bare (groupless) key -- the CLI reported "1 image
/// files updated" while silently writing nothing. Confirmed against the
/// oracle: `-FocalLength=50.0 mm` (no group) lands under `[ExifIFD]` there,
/// exactly like the explicit `-ExifIFD:FocalLength=` form.
#[test]
fn bare_tag_names_actually_write_under_exififd() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    let cases: &[(&str, &str, &str)] = &[
        ("FocalLength", "FocalLength", "50.0 mm"),
        (
            "FocalLengthIn35mmFormat",
            "FocalLengthIn35mmFormat",
            "75 mm",
        ),
        ("SubjectDistance", "SubjectDistance", "3.5 m"),
        ("AmbientTemperature", "AmbientTemperature", "20 C"),
    ];
    for (write_tag, leaf, value) in cases {
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

        let ours_read = oxidex_read(&ours, leaf, false);
        let theirs_read = oracle_read(oracle, &theirs, leaf, false);
        assert!(
            ours_read.is_some(),
            "{arg}: bare form did not actually write the tag (silent no-op)"
        );
        assert_eq!(
            ours_read, theirs_read,
            "{arg}: oxidex and the oracle disagree"
        );
    }
}

/// Codex review finding (comment 4112327521, P2): ExifTool tag and group
/// names are case-insensitive, but this file's own leaf-name dispatch used
/// exact-case comparisons, so `-ExifIFD:focallength=` or
/// `-exififd:FocalLength=` fell through to the generic string/rational path
/// with the unit still attached and was refused. Confirmed against the
/// oracle: both spellings below succeed there exactly like the canonical
/// one.
#[test]
fn case_insensitive_spellings_match_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    let cases: &[(&str, &str)] = &[
        ("ExifIFD:focallength", "FocalLength"),
        ("exififd:FocalLength", "FocalLength"),
        ("ExifIFD:SUBJECTDISTANCE", "SubjectDistance"),
        ("ExifIFD:ambienttemperature", "AmbientTemperature"),
    ];
    for (write_tag, leaf) in cases {
        let value = match *leaf {
            "SubjectDistance" => "3.5 m",
            "AmbientTemperature" => "20 C",
            _ => "50 mm",
        };
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

        assert_eq!(
            oxidex_read(&ours, leaf, false),
            oracle_read(oracle, &theirs, leaf, false),
            "{arg}: oxidex and the oracle disagree"
        );
    }
}

/// Codex review finding (comment 4112327507, P2): stripping the unit must
/// not bypass the tag's own native-range check. `FocalLength` is
/// `rational64u` (unsigned) and `FocalLengthIn35mmFormat` is `int16u`
/// (0..=65535); confirmed against the oracle that each refuses a value
/// outside its own range with the unit still attached.
#[test]
fn out_of_range_values_are_refused_like_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    let cases: &[(&str, &str)] = &[
        ("ExifIFD:FocalLength", "-1 mm"),
        ("ExifIFD:FocalLengthIn35mmFormat", "70000 mm"),
        ("ExifIFD:FocalLengthIn35mmFormat", "-1 mm"),
    ];
    for (write_tag, value) in cases {
        let dir = tempfile::tempdir().unwrap();
        let ours = write_fixture(dir.path(), "ours.jpg");
        let theirs = write_fixture(dir.path(), "theirs.jpg");
        let arg = format!("-{write_tag}={value}");

        let out = run_oxidex(&[os(&arg), ours.as_os_str().to_owned()]);
        assert!(
            !out.status.success(),
            "{arg}: oxidex must refuse a value outside the tag's native range"
        );
        let out = oracle_write(oracle, &arg, &theirs);
        assert!(
            !out.status.success(),
            "{arg}: the oracle unexpectedly accepted this value (test assumption wrong)"
        );
    }
}

/// Codex review finding (comment 4112327528, P2): `#` normalization in
/// `main.rs` only ran in the non-empty-value (modify) branch, so
/// `-ExifIFD:FocalLength#=` (ExifTool's delete syntax with the raw suffix)
/// called `remove_tag` with the literal, nonexistent name
/// `"ExifIFD:FocalLength#"` and left the real tag in place while still
/// reporting success. Confirmed against the oracle: it deletes the tag.
#[test]
fn hash_suffixed_deletion_actually_removes_the_tag() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let ours = write_fixture(dir.path(), "ours.jpg");
    let theirs = write_fixture(dir.path(), "theirs.jpg");

    for path in [&ours, &theirs] {
        let out = run_oxidex(&[
            os("-ExifIFD:FocalLength=50 mm"),
            path.as_os_str().to_owned(),
        ]);
        assert!(out.status.success());
    }
    assert!(oxidex_read(&ours, "FocalLength", false).is_some());

    let out = run_oxidex(&[os("-ExifIFD:FocalLength#="), ours.as_os_str().to_owned()]);
    assert!(
        out.status.success(),
        "delete: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = oracle_write(oracle, "-ExifIFD:FocalLength#=", &theirs);
    assert!(
        out.status.success(),
        "oracle delete failed (test assumption wrong): {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert_eq!(
        oxidex_read(&ours, "FocalLength", false),
        None,
        "-ExifIFD:FocalLength#= must actually delete the tag, not silently no-op"
    );
    assert_eq!(
        oracle_read(oracle, &theirs, "FocalLength", false),
        None,
        "test assumption wrong: the oracle did not delete it either"
    );
}

/// Codex review finding (comment 4112779672, P2): Exif.pm 13.59 declares
/// FocalLength (0x920a) and SubjectDistance (0x9206) `rational64u`, which
/// ExifTool rationalizes with a `0xffffffff` cap (`SetRational64u`,
/// Writer.pl:5282-5287). `TagValue::Rational` holds `i32` components, so the
/// signed-cap path silently stored `3000000000` as `2147483647/1`. The oracle
/// stores `3000000000/1`; oxidex cannot, so it must refuse the value rather
/// than write a different one -- and every value it *can* carry must match
/// the oracle's stored rational exactly.
#[test]
fn rational64u_values_beyond_i32_are_refused_not_clamped() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    // The oracle accepts each of these and stores a component above
    // i32::MAX (confirmed below through `-v3`'s raw rational).
    let unrepresentable: &[(&str, &str, &str)] = &[
        ("ExifIFD:FocalLength", "FocalLength", "3000000000 mm"),
        ("ExifIFD:FocalLength", "FocalLength", "2147483648"),
        ("ExifIFD:FocalLength", "FocalLength", "4294967295"),
        ("ExifIFD:FocalLength", "FocalLength", "0.0000000003"),
        ("ExifIFD:FocalLength", "FocalLength", "3000000000/1"),
        ("ExifIFD:FocalLength#", "FocalLength", "3000000000"),
        ("ExifIFD:SubjectDistance", "SubjectDistance", "3000000000 m"),
    ];
    for (write_tag, leaf, value) in unrepresentable {
        let dir = tempfile::tempdir().unwrap();
        let ours = write_fixture(dir.path(), "ours.jpg");
        let theirs = write_fixture(dir.path(), "theirs.jpg");
        let arg = format!("-{write_tag}={value}");

        let out = oracle_write(oracle, &arg, &theirs);
        assert!(
            out.status.success(),
            "{arg}: oracle write failed (test assumption wrong): {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stored = oracle_stored_rational(oracle, &theirs, leaf)
            .unwrap_or_else(|| panic!("{arg}: oracle stored no {leaf} (test assumption wrong)"));
        assert!(
            stored.0 > i64::from(i32::MAX) || stored.1 > i32::MAX as u64,
            "{arg}: oracle stored {stored:?}, which fits i32 (test assumption wrong)"
        );

        let out = run_oxidex(&[os(&arg), ours.as_os_str().to_owned()]);
        assert!(
            !out.status.success(),
            "{arg}: oxidex must refuse a rational64u value it cannot store exactly \
             (the oracle stores {stored:?})"
        );
        assert_eq!(
            oxidex_read(&ours, leaf, true),
            None,
            "{arg}: a refused write must leave the file without the tag"
        );
    }

    // Values that do fit i32 must store exactly the oracle's rational, and
    // negative values are refused by both tools (CheckValue's `u` branch).
    let representable: &[(&str, &str, &str)] = &[
        ("ExifIFD:FocalLength", "FocalLength", "2147483647"),
        ("ExifIFD:FocalLength", "FocalLength", "1.23456789012345"),
        ("ExifIFD:FocalLength", "FocalLength", "0.3333333333 mm"),
        ("ExifIFD:SubjectDistance", "SubjectDistance", "12.345 m"),
    ];
    for (write_tag, leaf, value) in representable {
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
        assert_eq!(
            oracle_stored_rational(oracle, &ours, leaf),
            oracle_stored_rational(oracle, &theirs, leaf),
            "{arg}: oxidex stored a different rational than the oracle"
        );
    }

    let negative: &[(&str, &str)] = &[
        ("ExifIFD:FocalLength", "-1/2"),
        ("ExifIFD:SubjectDistance", "-1 m"),
        ("ExifIFD:SubjectDistance", "-1/2"),
    ];
    for (write_tag, value) in negative {
        let dir = tempfile::tempdir().unwrap();
        let ours = write_fixture(dir.path(), "ours.jpg");
        let theirs = write_fixture(dir.path(), "theirs.jpg");
        let arg = format!("-{write_tag}={value}");
        let out = run_oxidex(&[os(&arg), ours.as_os_str().to_owned()]);
        assert!(
            !out.status.success(),
            "{arg}: oxidex must refuse a negative rational64u value"
        );
        let out = oracle_write(oracle, &arg, &theirs);
        assert!(
            !out.status.success(),
            "{arg}: the oracle unexpectedly accepted this value (test assumption wrong)"
        );
    }
}

/// The raw `(numerator, denominator)` the oracle's `-v3` dump reports for
/// `leaf` -- e.g. `FocalLength = 3000000000 (3000000000/1)` -- read through
/// the pinned ExifTool so both files are decoded by the same reader.
fn oracle_stored_rational(
    oracle: &exiftool_oracle::Oracle,
    path: &Path,
    leaf: &str,
) -> Option<(i64, u64)> {
    let out = oracle
        .command()
        .arg("-v3")
        .arg(path)
        .output()
        .expect("run oracle -v3");
    assert!(out.status.success(), "oracle -v3 failed");
    let text = String::from_utf8_lossy(&out.stdout);
    let needle = format!(" {leaf} = ");
    let line = text
        .lines()
        .find(|line| line.contains(&needle) && line.ends_with(')'))?;
    let pair = line.rsplit_once('(')?.1.strip_suffix(')')?;
    let (numerator, denominator) = pair.split_once('/')?;
    Some((numerator.parse().ok()?, denominator.parse().ok()?))
}

/// The TIFF format label (`rational64s[1]`, ...) the oracle's `-v3` dump
/// reports for tag `id` in `path`.
fn oracle_format_label(oracle: &exiftool_oracle::Oracle, path: &Path, id: &str) -> Option<String> {
    let out = oracle
        .command()
        .arg("-v3")
        .arg(path)
        .output()
        .expect("run oracle -v3");
    assert!(out.status.success(), "oracle -v3 failed");
    let text = String::from_utf8_lossy(&out.stdout);
    let needle = format!("- Tag {id} (");
    let line = text.lines().find(|line| line.contains(&needle))?;
    let inner = line.split_once(&needle)?.1.strip_suffix("):")?;
    Some(inner.rsplit_once(", ")?.1.to_string())
}

/// Codex review finding (comment 4112779679, P2): Exif.pm 13.59 0x9400
/// declares AmbientTemperature `rational64s`, and the oracle writes type 10
/// (SRATIONAL) even for a positive value. Comparing decoded JSON alone missed
/// that oxidex created the entry as type 5 (RATIONAL) whenever the value was
/// non-negative.
#[test]
fn ambient_temperature_is_created_as_srational_like_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    for arg in [
        "-ExifIFD:AmbientTemperature=20 C",
        "-ExifIFD:AmbientTemperature=0",
        "-AmbientTemperature=20.5C",
        "-exififd:ambienttemperature=20 C",
        "-ExifIFD:AmbientTemperature#=20",
        "-ExifIFD:AmbientTemperature=-5 C",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let ours = write_fixture(dir.path(), "ours.jpg");
        let theirs = write_fixture(dir.path(), "theirs.jpg");
        let out = run_oxidex(&[os(arg), ours.as_os_str().to_owned()]);
        assert!(
            out.status.success(),
            "{arg}: oxidex write failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = oracle_write(oracle, arg, &theirs);
        assert!(
            out.status.success(),
            "{arg}: oracle write failed (test assumption wrong): {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let theirs_label = oracle_format_label(oracle, &theirs, "0x9400");
        assert_eq!(
            theirs_label.as_deref(),
            Some("rational64s[1]"),
            "{arg}: test assumption wrong"
        );
        assert_eq!(
            oracle_format_label(oracle, &ours, "0x9400"),
            theirs_label,
            "{arg}: oxidex wrote a different TIFF format than the oracle"
        );
        assert_eq!(
            oracle_stored_rational(oracle, &ours, "AmbientTemperature"),
            oracle_stored_rational(oracle, &theirs, "AmbientTemperature"),
            "{arg}: oxidex stored a different rational than the oracle"
        );
    }
}

/// Codex review finding (comment 4112779675, P2): with a directory or two or
/// more files, writes go through `batch_processor::apply_modifications`,
/// which parsed an empty value as a write, so `-ExifIFD:FocalLength#=`
/// (and `-ExifIFD:FocalLength=`) failed rational parsing instead of deleting
/// the tag. The oracle deletes it from every file.
#[test]
fn batch_empty_assignment_deletes_from_every_file() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    for delete_arg in ["-ExifIFD:FocalLength#=", "-ExifIFD:FocalLength="] {
        for use_directory in [false, true] {
            let ours_dir = tempfile::tempdir().unwrap();
            let theirs_dir = tempfile::tempdir().unwrap();
            let ours = [
                write_fixture(ours_dir.path(), "a.jpg"),
                write_fixture(ours_dir.path(), "b.jpg"),
            ];
            let theirs = [
                write_fixture(theirs_dir.path(), "a.jpg"),
                write_fixture(theirs_dir.path(), "b.jpg"),
            ];
            for path in ours.iter().chain(theirs.iter()) {
                let out = run_oxidex(&[
                    os("-ExifIFD:FocalLength=50 mm"),
                    path.as_os_str().to_owned(),
                ]);
                assert!(out.status.success());
                assert!(oxidex_read(path, "FocalLength", true).is_some());
            }

            let mut args = vec![os(delete_arg)];
            if use_directory {
                args.push(ours_dir.path().as_os_str().to_owned());
            } else {
                args.extend(ours.iter().map(|path| path.as_os_str().to_owned()));
            }
            let out = run_oxidex(&args);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                out.status.success() && !stderr.contains("Error"),
                "{delete_arg} (directory={use_directory}): oxidex batch delete failed: {stderr}"
            );

            let mut cmd = oracle.command();
            cmd.arg("-overwrite_original").arg(delete_arg);
            if use_directory {
                cmd.arg(theirs_dir.path());
            } else {
                cmd.args(&theirs);
            }
            let out = cmd.output().expect("run oracle batch delete");
            assert!(
                out.status.success(),
                "oracle batch delete failed (test assumption wrong): {}",
                String::from_utf8_lossy(&out.stderr)
            );

            for (our_path, their_path) in ours.iter().zip(theirs.iter()) {
                assert_eq!(
                    oracle_read(oracle, their_path, "FocalLength", true),
                    None,
                    "test assumption wrong: the oracle did not delete it"
                );
                assert_eq!(
                    oxidex_read(our_path, "FocalLength", true),
                    None,
                    "{delete_arg} (directory={use_directory}): {our_path:?} still has FocalLength"
                );
            }
        }
    }
}

/// Every `-G1` key (with its value) for `leaf` in `path`, read through the
/// pinned oracle, so the *group* a tool wrote the tag under is compared too
/// -- `extract` keeps only the value, which cannot tell `IFD0:FocalLength`
/// from `ExifIFD:FocalLength`.
fn oracle_read_grouped(
    oracle: &exiftool_oracle::Oracle,
    path: &Path,
    leaf: &str,
) -> Vec<(String, serde_json::Value)> {
    let out = oracle
        .command()
        .arg("-j")
        .arg("-G1")
        .arg(format!("-{leaf}"))
        .arg(path)
        .output()
        .expect("run oracle -j -G1");
    assert!(out.status.success(), "oracle -j -G1 -{leaf} failed");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("oracle JSON");
    json[0]
        .as_object()
        .expect("oracle -j element")
        .iter()
        .filter(|(key, _)| key.rsplit(':').next() == Some(leaf))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Local Codex pre-review of PR #963 (two findings): a bare name in another
/// letter case (`-focallength=50 mm`) failed type resolution and was refused,
/// and the family-0 group (`-EXIF:FocalLength=50 mm`) was stored as
/// `IFD0:FocalLength`. The pinned oracle writes every spelling below to
/// `ExifIFD`; compare the full `-G1` key, not just the value.
#[test]
fn every_spelling_writes_the_same_exififd_key_as_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };

    let cases: &[(&str, &str)] = &[
        ("-FocalLength=50.0 mm", "FocalLength"),
        ("-focallength=50 mm", "FocalLength"),
        ("-FOCALLENGTH=50 mm", "FocalLength"),
        ("-focallength#=50", "FocalLength"),
        ("-subjectdistance=3.5 m", "SubjectDistance"),
        ("-ambienttemperature=20 C", "AmbientTemperature"),
        ("-focallengthin35mmformat=75 mm", "FocalLengthIn35mmFormat"),
        ("-EXIF:FocalLength=50 mm", "FocalLength"),
        ("-exif:focallength=50 mm", "FocalLength"),
        ("-EXIF:SubjectDistance=3.5 m", "SubjectDistance"),
        ("-EXIF:AmbientTemperature=20 C", "AmbientTemperature"),
        (
            "-EXIF:FocalLengthIn35mmFormat=75 mm",
            "FocalLengthIn35mmFormat",
        ),
        ("-ExifIFD:FocalLength=50 mm", "FocalLength"),
    ];
    for (arg, leaf) in cases {
        let dir = tempfile::tempdir().unwrap();
        let ours = write_fixture(dir.path(), "ours.jpg");
        let theirs = write_fixture(dir.path(), "theirs.jpg");
        let out = run_oxidex(&[os(arg), ours.as_os_str().to_owned()]);
        assert!(
            out.status.success(),
            "{arg}: oxidex write failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = oracle_write(oracle, arg, &theirs);
        assert!(
            out.status.success(),
            "{arg}: oracle write failed (test assumption wrong): {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let theirs_read = oracle_read_grouped(oracle, &theirs, leaf);
        assert_eq!(
            theirs_read
                .iter()
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>(),
            [format!("ExifIFD:{leaf}")],
            "{arg}: test assumption wrong"
        );
        assert_eq!(
            oracle_read_grouped(oracle, &ours, leaf),
            theirs_read,
            "{arg}: oxidex wrote a different group or value than the oracle"
        );
    }
}

#[test]
fn unsupported_raw_writes_do_not_apply_print_conversion() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for file_count in [1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let ours: Vec<_> = (0..file_count)
            .map(|i| write_fixture(dir.path(), &format!("ours{i}.jpg")))
            .collect();
        let theirs = write_fixture(dir.path(), "theirs.jpg");
        let arg = "-GPS:GPSLongitudeRef#=West";
        let expected = oracle_write(oracle, arg, &theirs);
        assert!(!expected.status.success(), "oracle must refuse raw West");
        let before: Vec<_> = ours.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let mut args = vec![os(arg)];
        args.extend(ours.iter().map(|p| p.as_os_str().to_owned()));
        let actual = run_oxidex(&args);
        assert!(
            !actual.status.success(),
            "raw West unexpectedly accepted: {:?}",
            actual
        );
        for (path, bytes) in ours.iter().zip(before) {
            assert_eq!(
                std::fs::read(path).unwrap(),
                bytes,
                "refused raw write changed file"
            );
        }
    }
}

#[test]
fn exif_family_deletions_keep_their_directory_scope() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for (leaf, value) in [
        ("FocalLength", "50"),
        ("FocalLengthIn35mmFormat", "75"),
        ("SubjectDistance", "3.5"),
        ("AmbientTemperature", "20"),
    ] {
        for group in ["EXIF:", "exif:", ""] {
            let dir = tempfile::tempdir().unwrap();
            let ours = write_fixture(dir.path(), "ours.jpg");
            let theirs = write_fixture(dir.path(), "theirs.jpg");
            let setup = format!("-IFD0:{leaf}={value}");
            for path in [&ours, &theirs] {
                assert!(oracle_write(oracle, &setup, path).status.success());
                assert_eq!(
                    oracle_read_grouped(oracle, path, leaf)[0].0,
                    format!("IFD0:{leaf}")
                );
            }
            let arg = format!("-{group}{leaf}=");
            assert!(oracle_write(oracle, &arg, &theirs).status.success());
            assert!(oracle_read_grouped(oracle, &theirs, leaf).is_empty());
            let actual = run_oxidex(&[os(&arg), ours.as_os_str().to_owned()]);
            assert!(
                actual.status.success(),
                "family deletion failed: {:?}",
                actual
            );
            assert_eq!(
                oracle_read_grouped(oracle, &ours, leaf),
                oracle_read_grouped(oracle, &theirs, leaf),
                "{arg}"
            );
        }
    }
}

/// Cross-directory replacement is owned by PR #964. Until its sibling-aware
/// transaction lands, these newly routed family assignments must match the
/// oracle or refuse by name without leaving a conflicting IFD0 copy.
#[test]
fn family_updates_move_or_refuse_existing_ifd0_values() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    for leaf in [
        "FocalLength",
        "FocalLengthIn35mmFormat",
        "SubjectDistance",
        "AmbientTemperature",
    ] {
        for both_directories in [false, true] {
            for group in ["EXIF:", "exif:", ""] {
                let dir = tempfile::tempdir().unwrap();
                let ours = write_fixture(dir.path(), "ours.jpg");
                let theirs = write_fixture(dir.path(), "theirs.jpg");
                for path in [&ours, &theirs] {
                    let mut cmd = oracle.command();
                    cmd.arg("-overwrite_original")
                        .arg(format!("-IFD0:{leaf}=35"));
                    if both_directories {
                        cmd.arg(format!("-ExifIFD:{leaf}=40"));
                    }
                    assert!(cmd.arg(path).output().unwrap().status.success());
                }
                let before = std::fs::read(&ours).unwrap();
                let arg = format!("-{group}{leaf}=50");
                assert!(oracle_write(oracle, &arg, &theirs).status.success());
                let expected = oracle_read_grouped(oracle, &theirs, leaf);
                assert_eq!(expected.len(), 1);
                assert_eq!(expected[0].0, format!("ExifIFD:{leaf}"));
                let actual = run_oxidex(&[os(&arg), ours.as_os_str().to_owned()]);
                if actual.status.success() {
                    assert_eq!(
                        oracle_read_grouped(oracle, &ours, leaf),
                        expected,
                        "{arg}, both={both_directories}"
                    );
                } else {
                    assert_eq!(std::fs::read(&ours).unwrap(), before);
                    let error = String::from_utf8_lossy(&actual.stderr);
                    assert!(
                        error.contains(&format!("IFD0:{leaf}"))
                            && error.contains("cross-directory"),
                        "{error}"
                    );
                }
            }
        }
    }
}

/// Reader-hidden TIFF types must participate in the same physical sibling
/// replacement checks as decoded rows. Type 99 is not decoded by the TIFF value reader.
#[test]
fn reader_hidden_ifd0_copy_is_moved_or_refused_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend_from_slice(&1_u16.to_le_bytes());
    tiff.extend_from_slice(&0x920a_u16.to_le_bytes());
    tiff.extend_from_slice(&99_u16.to_le_bytes());
    tiff.extend_from_slice(&1_u32.to_le_bytes());
    tiff.extend_from_slice(&35_u32.to_le_bytes());
    tiff.extend_from_slice(&0_u32.to_le_bytes());
    for extension in ["tif", "jpg"] {
        let path = dir.path().join(format!("hidden.{extension}"));
        let bytes = if extension == "jpg" {
            let base = hex(BASE_JPEG_HEX);
            let mut jpeg = base[..2].to_vec();
            jpeg.extend_from_slice(&[0xff, 0xe1]);
            jpeg.extend_from_slice(&u16::try_from(tiff.len() + 8).unwrap().to_be_bytes());
            jpeg.extend_from_slice(b"Exif\0\0");
            jpeg.extend_from_slice(&tiff);
            jpeg.extend_from_slice(&base[2..]);
            jpeg
        } else {
            tiff.clone()
        };
        for tag in ["FocalLength", "focallength#", "EXIF:FocalLength"] {
            std::fs::write(&path, &bytes).unwrap();
            let map = oxidex::core::operations::read_metadata(&path).unwrap();
            assert!(!map.contains_key("IFD0:FocalLength"));
            let result = oxidex::core::operations::modify_tag(
                &path,
                "FocalLength",
                oxidex::core::TagValue::Rational {
                    numerator: 50,
                    denominator: 1,
                },
            );
            if let Err(error) = result {
                // An unknown TIFF type also prevents the maker-note census
                // from proving that a bare FocalLength has no other owner.
                // That earlier refusal must leave the physical row intact.
                assert!(
                    error
                        .to_string()
                        .contains("maker note oxidex cannot identify"),
                    "{error}"
                );
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
            } else {
                assert_hidden_sibling_moved(&path);
            }
            std::fs::write(&path, &bytes).unwrap();

            let out = run_oxidex(&[os(&format!("-{tag}=50")), path.as_os_str().to_owned()]);
            if out.status.success() {
                assert_hidden_sibling_moved(&path);
            } else {
                let error = String::from_utf8_lossy(&out.stderr);
                if tag.eq_ignore_ascii_case("FocalLength") || tag == "focallength#" {
                    assert!(
                        error.contains("maker note oxidex cannot identify"),
                        "{error}"
                    );
                } else {
                    assert!(error.contains("IFD0:FocalLength"), "{error}");
                }
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
            }
        }
    }
}

// A successful replacement must remove the undecodable physical IFD0
// entry, not merely add a decoded ExifIFD row beside it.
fn assert_hidden_sibling_moved(path: &Path) {
    let bytes = std::fs::read(path).unwrap();
    let tiff = if bytes.starts_with(b"II") {
        bytes.as_slice()
    } else {
        let at = bytes.windows(6).position(|w| w == b"Exif\0\0").unwrap();
        &bytes[at + 6..]
    };
    let start = u32::from_le_bytes(tiff[4..8].try_into().unwrap()) as usize;
    let count = u16::from_le_bytes(tiff[start..start + 2].try_into().unwrap()) as usize;
    for i in 0..count {
        let at = start + 2 + i * 12;
        assert_ne!(
            u16::from_le_bytes(tiff[at..at + 2].try_into().unwrap()),
            0x920a
        );
    }
    let map = oxidex::core::operations::read_metadata(path).unwrap();
    assert_eq!(map.get_string("ExifIFD:FocalLength"), Some("50.0 mm"));
}

/// A per-file batch parses every assignment before committing its deletion.
#[test]
fn batch_later_invalid_raw_assignment_preserves_each_file() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let paths = [
        write_fixture(dir.path(), "a.jpg"),
        write_fixture(dir.path(), "b.jpg"),
    ];
    for path in &paths {
        assert!(
            oracle_write(oracle, "-ExifIFD:FocalLength=35", path)
                .status
                .success()
        );
    }
    let before: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let out = run_oxidex(&[
        os("-ExifIFD:FocalLength#="),
        os("-ExifIFD:FocalLength#=50 mm"),
        paths[0].as_os_str().to_owned(),
        paths[1].as_os_str().to_owned(),
    ]);
    assert!(!out.status.success(), "{out:?}");
    for (path, bytes) in paths.iter().zip(before) {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
