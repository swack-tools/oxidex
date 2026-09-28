//! CLI parity for enum labels and raw numeric values, checked against the
//! selected native ExifTool release and its Canon.jpg fixture.
//!
//! The inverse uses the transcribed EXIF and GPS IFD tables. It follows
//! ReverseLookup: exact, case-insensitive exact, prefix, then substring.
//! Exact duplicates select the first key in Perl string order; ambiguous
//! partial matches are refused. Every accepted label is checked through
//! both writers and both raw readbacks.
//!
//! A numeric string is a label unless the caller supplies `#` or disables
//! PrintConv. For example, `-Orientation=6` is refused, while
//! `-Orientation=1` uniquely matches `Rotate 180`. The rejected numeric
//! cases below also check that their explicit raw forms preserve the code.
//! Command-level status differences for grouped WhiteBalance and Compression
//! are recorded beside those assertions; unchanged bytes alone do not prove
//! equivalent command behavior.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn oxidex(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

fn copy_into(dir: &TempDir, src: &Path, name: &str) -> PathBuf {
    let dest = dir.path().join(name);
    std::fs::copy(src, &dest).unwrap_or_else(|e| panic!("copy {src:?} -> {dest:?}: {e}"));
    dest
}

/// The tag's raw (unconverted) value, from oxidex's own `--no-print-conv
/// -s3` output. oxidex's own `-n` is unrelated (`Dry-run mode`, per
/// `oxidex --help`) -- ExifTool's no-PrintConv flag is spelled out here.
fn oxidex_read_n(path: &Path, tag: &str) -> String {
    let out = oxidex(&[
        "--no-print-conv",
        "-s3",
        &format!("-{tag}"),
        path.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "oxidex --no-print-conv -s3 -{tag} {path:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The tag's raw (unconverted) value, from the pinned oracle's `-n -s3`.
fn oracle_read_n(oracle: &exiftool_oracle::Oracle, path: &Path, tag: &str) -> String {
    let out = oracle
        .command()
        .args(["-n", "-s3", &format!("-{tag}"), path.to_str().unwrap()])
        .output()
        .expect("run oracle");
    assert!(
        out.status.success(),
        "{} -n -s3 -{tag} {path:?}: {}",
        oracle.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Writes `-{tag}={label}` with oxidex on a fresh copy of `base`, then
/// asserts: oxidex accepted the write, oxidex's own read-back reports
/// `expected_code`, and the pinned oracle -- writing the same `-{tag}=
/// {label}` onto its own fresh copy -- agrees on `expected_code` too. Both
/// tools are given the identical, explicitly grouped `tag` spelling, so a
/// pre-existing, unrelated ambiguity in oxidex's bare-name write routing
/// (see `metering_mode_label_write_matches_oracle`) cannot mask a result
/// here.
fn assert_label_write_matches_oracle(
    oracle: &exiftool_oracle::Oracle,
    base: &Path,
    tag: &str,
    label: &str,
    expected_code: i64,
) {
    let dir = tempfile::tempdir().unwrap();
    let ox_path = copy_into(&dir, base, "ox.jpg");
    let arg = format!("-{tag}={label}");
    let out = oxidex(&[&arg, ox_path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "oxidex {arg} should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        oxidex_read_n(&ox_path, tag),
        expected_code.to_string(),
        "oxidex read-back for {tag}={label}"
    );

    let et_path = copy_into(&dir, base, "et.jpg");
    let et_out = oracle
        .command()
        .args(["-overwrite_original", &arg, et_path.to_str().unwrap()])
        .output()
        .expect("run oracle write");
    assert!(
        et_out.status.success(),
        "oracle {arg} should succeed: {}",
        String::from_utf8_lossy(&et_out.stderr)
    );
    assert_eq!(
        oracle_read_n(oracle, &et_path, tag),
        expected_code.to_string(),
        "oracle read-back for {tag}={label}"
    );
}

/// `Canon.jpg` from the pinned tree's `t/images`, or a loud skip -- never a
/// silent pass -- when the fixture cannot be resolved.
fn canon_jpg() -> Option<PathBuf> {
    fixtures::pinned_t_images_fixture_path("Canon.jpg")
}

/// A bare numeric code on an enum-PrintConv tag must be refused, exactly
/// like an unrecognized label -- the coordinator's correction to this fix's
/// original brief: pinned ExifTool 13.59 refuses `-Orientation=6` with
/// `Warning: Can't convert IFD0:Orientation (not in PrintConv)` /
/// `Nothing to do.`, leaving the file untouched, and only accepts a raw
/// code through `#` (`-Orientation#=6`) or `--no-print-conv` (oxidex's
/// spelling of ExifTool's `-n`; oxidex's own `-n` is unrelated dry-run).
/// This asserts all three: the bare form is refused with the oracle's own
/// wording and leaves the file byte-identical, and both raw forms are
/// accepted and agree with the oracle's own `#`/`-n` write.
fn assert_numeric_input_policy_matches_oracle(
    oracle: &exiftool_oracle::Oracle,
    base: &Path,
    tag: &str,
    code: i64,
) {
    let dir = tempfile::tempdir().unwrap();

    // The selected EXIF row refuses this numeric label. Partial matches
    // have a different reason from labels missing from the table.
    let bare_path = copy_into(&dir, base, "bare.jpg");
    let before = std::fs::read(&bare_path).unwrap();
    let arg = format!("-{tag}={code}");
    let out = oxidex(&[&arg, bare_path.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "oxidex {arg} (bare numeric) should be refused, not accepted as a raw code"
    );
    assert_eq!(
        std::fs::read(&bare_path).unwrap(),
        before,
        "a refused bare-numeric write must leave the file untouched"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let reason = match tag {
        "IFD0:Compression" | "IFD0:GrayResponseUnit" => "matches more than one PrintConv",
        _ => "not in PrintConv",
    };
    assert!(
        stderr.contains(&format!("Can't convert {tag} ({reason})")),
        "expected the selected EXIF row's conversion reason, got: {stderr}"
    );

    let et_bare_path = copy_into(&dir, base, "bare_et.jpg");
    let et_before = std::fs::read(&et_bare_path).unwrap();
    let et_out = oracle
        .command()
        .args(["-overwrite_original", &arg, et_bare_path.to_str().unwrap()])
        .output()
        .expect("run oracle");
    // The oracle's own exit code/wording for this varies by tag (confirmed:
    // `Orientation` is `Nothing to do.`, exit 1; a `Priority => 0` tag like
    // `WhiteBalance` sharing a MakerNote duplicate is `0 image files
    // updated` / `1 image files unchanged`, exit 0; `Compression=2` is
    // also unchanged with no warning). Those two command-level status
    // differences predate the candidate conversion repair. Every other
    // case must agree on the conversion reason as well as preserved bytes.
    if !matches!(tag, "ExifIFD:WhiteBalance" | "IFD0:Compression") {
        assert!(!et_out.status.success());
        assert!(
            String::from_utf8_lossy(&et_out.stderr)
                .contains(&format!("Can't convert {tag} ({reason})")),
            "native conversion reason for {arg}: {}",
            String::from_utf8_lossy(&et_out.stderr)
        );
    }
    assert_eq!(
        std::fs::read(&et_bare_path).unwrap(),
        et_before,
        "the oracle must also leave the file untouched for a bare numeric code"
    );

    // `#`: accepted as the raw code, matching the oracle's own `#` write.
    let hash_path = copy_into(&dir, base, "hash.jpg");
    let hash_arg = format!("-{tag}#={code}");
    let out = oxidex(&[&hash_arg, hash_path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "oxidex {hash_arg} should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(oxidex_read_n(&hash_path, tag), code.to_string());

    let et_hash_path = copy_into(&dir, base, "hash_et.jpg");
    let et_out = oracle
        .command()
        .args([
            "-overwrite_original",
            &hash_arg,
            et_hash_path.to_str().unwrap(),
        ])
        .output()
        .expect("run oracle");
    assert!(et_out.status.success());
    assert_eq!(oracle_read_n(oracle, &et_hash_path, tag), code.to_string());

    // `--no-print-conv` (oxidex) / `-n` (oracle): same raw-code acceptance,
    // applied globally instead of per-tag.
    let np_path = copy_into(&dir, base, "np.jpg");
    let out = oxidex(&["--no-print-conv", &arg, np_path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "oxidex --no-print-conv {arg} should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(oxidex_read_n(&np_path, tag), code.to_string());

    let et_np_path = copy_into(&dir, base, "np_et.jpg");
    let et_out = oracle
        .command()
        .args([
            "-overwrite_original",
            "-n",
            &arg,
            et_np_path.to_str().unwrap(),
        ])
        .output()
        .expect("run oracle");
    assert!(et_out.status.success());
    assert_eq!(oracle_read_n(oracle, &et_np_path, tag), code.to_string());
}

#[test]
fn orientation_all_eight_values_match_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [
        ("Horizontal (normal)", 1),
        ("Mirror horizontal", 2),
        ("Rotate 180", 3),
        ("Mirror vertical", 4),
        ("Mirror horizontal and rotate 270 CW", 5),
        ("Rotate 90 CW", 6),
        ("Mirror horizontal and rotate 90 CW", 7),
        ("Rotate 270 CW", 8),
    ] {
        assert_label_write_matches_oracle(oracle, &base, "IFD0:Orientation", label, code);
    }
}

#[test]
fn orientation_case_insensitive_spelling_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    // ExifTool's `ReverseLookup` tries an exact match, then a
    // case-insensitive one (`Writer.pl:3609-3665`); this is the second tier.
    assert_label_write_matches_oracle(oracle, &base, "IFD0:Orientation", "rotate 90 cw", 6);
}

#[test]
fn resolution_unit_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [("None", 1), ("inches", 2), ("cm", 3)] {
        assert_label_write_matches_oracle(oracle, &base, "IFD0:ResolutionUnit", label, code);
    }
}

/// A bare `-MeteringMode=` write fails in oxidex for a reason this fix does
/// not touch: `write_transaction.rs`'s bare-tag routing refuses to write
/// `EXIF:MeteringMode` alone because ExifTool would also update the
/// same-named Canon MakerNote tag, which oxidex's writer cannot do
/// (`use -ExifIFD:MeteringMode= to change only the EXIF field`). That
/// refusal fires identically for a plain numeric value
/// (`-MeteringMode=3`), so it is a pre-existing, unrelated multi-group
/// write-routing gap, not a PrintConv defect -- out of this fix's bounded
/// scope. Every case here therefore names the EXIF group explicitly, which
/// already avoids the collision.
#[test]
fn metering_mode_label_write_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [
        ("Unknown", 0),
        ("Average", 1),
        ("Center-weighted average", 2),
        ("Spot", 3),
        ("Multi-spot", 4),
        ("Multi-segment", 5),
        ("Partial", 6),
        ("Other", 255),
    ] {
        assert_label_write_matches_oracle(oracle, &base, "ExifIFD:MeteringMode", label, code);
    }
}

#[test]
fn exposure_program_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [
        ("Not Defined", 0),
        ("Manual", 1),
        ("Program AE", 2),
        ("Bulb", 9),
    ] {
        assert_label_write_matches_oracle(oracle, &base, "ExifIFD:ExposureProgram", label, code);
    }
}

#[test]
fn flash_still_works_through_its_own_printhex_path() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    // Flash (Exif.pm 0x9209) is `PrintConv => \%flash` plus `Flags =>
    // 'PrintHex'` -- not a plain enum hash -- so it is not reachable through
    // `invert_enum_printconv` and keeps using
    // `core::formatters::exif_enums::parse_flash_label`. This test pins that
    // this fix did not disturb it.
    for (label, code) in [("Off, Did not fire", 0x10), ("Fired", 0x01)] {
        assert_label_write_matches_oracle(oracle, &base, "ExifIFD:Flash", label, code);
    }
}

#[test]
fn white_balance_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [("Auto", 0), ("Manual", 1)] {
        assert_label_write_matches_oracle(oracle, &base, "ExifIFD:WhiteBalance", label, code);
    }
}

#[test]
fn exposure_mode_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [("Auto", 0), ("Manual", 1), ("Auto bracket", 2)] {
        assert_label_write_matches_oracle(oracle, &base, "ExifIFD:ExposureMode", label, code);
    }
}

#[test]
fn scene_capture_type_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [
        ("Standard", 0),
        ("Landscape", 1),
        ("Portrait", 2),
        ("Night", 3),
    ] {
        assert_label_write_matches_oracle(oracle, &base, "ExifIFD:SceneCaptureType", label, code);
    }
}

#[test]
fn ycbcr_positioning_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [("Centered", 1), ("Co-sited", 2)] {
        assert_label_write_matches_oracle(oracle, &base, "IFD0:YCbCrPositioning", label, code);
    }
}

/// Found by the breadth measurement, not the original repro: `-GrayResponseUnit=<label>`
/// silently stored the wrong code, rather than failing loudly. Exif.pm
/// 13.59 0x0122 declares this int16u tag's `PrintConv` as a plain hash
/// whose labels (`0.1`, `0.001`, `0.0001`, `1e-05`, `1e-06`) are digit
/// strings. Before this fix, `GrayResponseUnit` had no entry in the
/// `declared_tag_name` leaf dispatch, so its label reached the generic
/// integer parser directly; `IsFloat` accepted "0.1"/"0.001"/"0.0001" and
/// rounded each to the nearest integer, storing `0` -- a confident, wrong
/// code under a real tag name, worse than the refusal `1e-05`/`1e-06` got
/// (`IsHex`/`IsFloat` both reject those, so they already errored "Not an
/// integer"). Routed through the same table lookup as every other tag in
/// this file now.
#[test]
fn gray_response_unit_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [
        ("0.1", 1),
        ("0.001", 2),
        ("0.0001", 3),
        ("1e-05", 4),
        ("1e-06", 5),
    ] {
        assert_label_write_matches_oracle(oracle, &base, "IFD0:GrayResponseUnit", label, code);
    }
}

/// Found by the breadth measurement: Exif.pm 13.59 0xa301 declares
/// `SceneType` `Writable => 'undef'` (a one-byte field) with the
/// single-entry `PrintConv` `{1 => 'Directly photographed'}`. `SceneType`
/// is unregistered in the tag registry, so before this fix its label
/// reached the generic `ValueType::Binary` arm unconverted and was stored
/// as its own 21 UTF-8 bytes -- reading back as
/// `(Binary data 21 bytes, use -b option to extract)` instead of the
/// single byte `01`. Fixed the same way `FileSource` (also `undef`) is
/// handled: look the label up, then wrap the resulting code as raw bytes
/// instead of falling through to the generic dispatch at all.
#[test]
fn scene_type_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    assert_label_write_matches_oracle(
        oracle,
        &base,
        "ExifIFD:SceneType",
        "Directly photographed",
        1,
    );
}

/// Found by the breadth measurement: DNG's `CalibrationIlluminant1/2/3`
/// (Exif.pm 13.59 0xc65a/0xc65b/0xcd31) declare the exact same
/// `%lightSource` hash as `LightSource`'s own `PrintConv` -- duplicate
/// "Daylight" included -- so before this fix, with no entry for these three
/// names, a label like "D55" reached the generic integer parser, whose
/// `IsHex` branch (tried before `IsFloat`) accepted "D55" as the hex value
/// `0x0D55` = 3413 and stored that: a confident, wrong code under a real
/// tag name. Routed through `LightSource`'s own hand-written table (kept
/// hand-written rather than generic specifically because of the duplicate
/// label -- see that match arm's comment), which all three share verbatim
/// in ExifTool itself.
#[test]
fn calibration_illuminant_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for tag in [
        "IFD0:CalibrationIlluminant1",
        "IFD0:CalibrationIlluminant2",
        "IFD0:CalibrationIlluminant3",
    ] {
        for (label, code) in [
            ("Daylight", 1),
            ("D55", 20),
            ("D65", 21),
            ("D75", 22),
            ("D50", 23),
        ] {
            assert_label_write_matches_oracle(oracle, &base, tag, label, code);
        }
    }
}

#[test]
fn compression_unambiguous_labels_match_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [("Uncompressed", 1), ("LZW", 5), ("Adobe Deflate", 8)] {
        assert_label_write_matches_oracle(oracle, &base, "IFD0:Compression", label, code);
    }
}

/// `Compression`'s `%compression` hash (Exif.pm 13.59) names two codes
/// `"JPEG"`: `7` (the modern code) and `99` (a legacy alias, `#16`). Exact
/// match here finds both. `ReverseLookup` permits exact duplicates and
/// selects the first string-sorted key: `"7"` precedes `"99"`. Check both
/// writers and both raw readbacks so a successful no-op cannot pass.
#[test]
fn compression_jpeg_exact_duplicates_match_native_key_order() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    assert_label_write_matches_oracle(oracle, &base, "IFD0:Compression", "JPEG", 7);
}

#[test]
fn orientation_invalid_label_is_refused_like_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    let dir = tempfile::tempdir().unwrap();

    let ox_path = copy_into(&dir, &base, "ox_invalid.jpg");
    let before = std::fs::read(&ox_path).unwrap();
    let out = oxidex(&["-IFD0:Orientation=Sideways", ox_path.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "oxidex should refuse an unknown label"
    );
    assert_eq!(
        std::fs::read(&ox_path).unwrap(),
        before,
        "a refused write must leave the file untouched"
    );

    let et_path = copy_into(&dir, &base, "et_invalid.jpg");
    let et_before = std::fs::read(&et_path).unwrap();
    let et_out = oracle
        .command()
        .args([
            "-overwrite_original",
            "-IFD0:Orientation=Sideways",
            et_path.to_str().unwrap(),
        ])
        .output()
        .expect("run oracle");
    assert!(
        !et_out.status.success() || String::from_utf8_lossy(&et_out.stdout).contains("0 image"),
        "the oracle should also refuse an unknown label"
    );
    assert_eq!(
        std::fs::read(&et_path).unwrap(),
        et_before,
        "the oracle's refused write must also leave the file untouched"
    );
}

/// The coordinator's correction: "numeric input stays accepted" in this
/// fix's original brief was wrong. Pinned ExifTool 13.59 refuses a bare
/// numeric code on every one of these tags (confirmed against the oracle
/// directly for each), and only accepts one raw via `#` or `-n`. One
/// representative code per tag (its `PrintConv`'s first entry) is enough to
/// pin the bare-vs-raw dispatch; the label tests above already cover every
/// entry's exact value. `GainControl` and `LightSource` are exercised via
/// the exhaustive `breadth_measure.py` sweep instead of here (`LightSource`
/// specifically triggers a disclosed, out-of-scope divergence -- see that
/// script's module doc on ExifTool's substring-fallback tier of
/// `ReverseLookup`, which this port deliberately does not implement).
#[test]
fn numeric_input_is_refused_bare_and_accepted_raw_for_every_required_tag() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (tag, code) in [
        // 1 is excluded for Orientation: "1" as a search string happens to
        // substring-match "Rotate 180" (code 3) -- see the module doc on
        // ExifTool's substring-fallback tier.
        ("IFD0:Orientation", 3),
        ("IFD0:ResolutionUnit", 1),
        ("ExifIFD:MeteringMode", 0),
        ("ExifIFD:ExposureProgram", 0),
        ("ExifIFD:WhiteBalance", 0),
        ("ExifIFD:ExposureMode", 0),
        ("ExifIFD:SceneCaptureType", 0),
        // "2" has multiple partial matches; "1" would uniquely select
        // "CCITT 1D" and therefore belongs to the label acceptance cases.
        ("IFD0:Compression", 2),
        ("IFD0:YCbCrPositioning", 1),
        ("IFD0:GrayResponseUnit", 1),
        ("ExifIFD:SceneType", 1),
        ("ExifIFD:Flash", 1),
    ] {
        assert_numeric_input_policy_matches_oracle(oracle, &base, tag, code);
    }
}

/// PR #959 review (Codex, P1): `Exif::Main` carries a SECOND, non-writable
/// `SensingMethod` row (id 0x9217, `1 => "Monochrome area"`) alongside the
/// writable one this tag actually is (id 0xa217, `1 => "Not defined"`).
/// `invert_enum_printconv`'s old name-only `.find()` picked whichever row
/// sorts first by id -- 0x9217, the WRONG one -- so `-ExifIFD:SensingMethod
/// ='Not defined'` was rejected while `='Monochrome area'` was silently
/// accepted as code 1 and read back as `Not defined` (both a wrong
/// acceptance and a wrong rejection from a single mis-resolved row). Fixed
/// by resolving through the registry's own numeric id
/// (`get_tag_descriptor("EXIF:SensingMethod").id()` is `0xa217`) via
/// `IfdTable::tag`'s id-keyed lookup, which cannot return the wrong
/// same-named row because ids in `tags` are unique.
#[test]
fn sensing_method_resolves_the_writable_row_not_the_first_same_named_one() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    // The writable row's own labels succeed, matching the oracle exactly.
    for (label, code) in [
        ("Not defined", 1),
        ("Two-chip color area", 3),
        ("Trilinear", 7),
    ] {
        assert_label_write_matches_oracle(oracle, &base, "ExifIFD:SensingMethod", label, code);
    }
    // "Monochrome area" belongs only to the OTHER (non-writable) row; the
    // oracle refuses it for the writable tag, and so must oxidex.
    let dir = tempfile::tempdir().unwrap();
    let ox_path = copy_into(&dir, &base, "sm_wrong_row.jpg");
    let before = std::fs::read(&ox_path).unwrap();
    let out = oxidex(&[
        "-ExifIFD:SensingMethod=Monochrome area",
        ox_path.to_str().unwrap(),
    ]);
    assert!(
        !out.status.success(),
        "\"Monochrome area\" belongs to SensingMethod's non-writable row and must be refused"
    );
    assert_eq!(std::fs::read(&ox_path).unwrap(), before);

    let et_path = copy_into(&dir, &base, "sm_wrong_row_et.jpg");
    let et_before = std::fs::read(&et_path).unwrap();
    oracle
        .command()
        .args([
            "-overwrite_original",
            "-ExifIFD:SensingMethod=Monochrome area",
            et_path.to_str().unwrap(),
        ])
        .output()
        .expect("run oracle");
    assert_eq!(
        std::fs::read(&et_path).unwrap(),
        et_before,
        "the oracle must also refuse it for the writable tag"
    );
}

/// PR #959 review (Codex, P1): raw mode must skip every PrintConv inversion
/// this file has, not only the generic enum-table dispatch added for
/// Orientation and friends. Before this fix, `-GPS:GPSDifferential#=0` and
/// `--no-print-conv -GPS:GPSDifferential=0` were refused (the hand-written
/// tuple match that (re)implements `GPSDifferential`'s catch-all had no
/// `raw_mode` guard at all), and `-ExifIFD:Contrast#=1` ran through
/// `invert_exif_contrast_parameter` and silently turned the requested raw
/// code `1` into `2` (`ConvertParameter` treats a positive number as "High"
/// -- correct for a PrintConv label, wrong for a caller who already supplied
/// the raw code and asked to skip PrintConv entirely).
#[test]
fn raw_mode_bypasses_every_hand_written_inverse_conversion() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };

    // `#`
    for (tag, code) in [("GPS:GPSDifferential", 0), ("ExifIFD:Contrast", 1)] {
        let dir = tempfile::tempdir().unwrap();
        let arg = format!("-{tag}#={code}");

        let ox_path = copy_into(&dir, &base, "hash.jpg");
        let out = oxidex(&[&arg, ox_path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "oxidex {arg} should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            oxidex_read_n(&ox_path, tag),
            code.to_string(),
            "oxidex {arg} must store the raw code, not run it through PrintConvInv"
        );

        let et_path = copy_into(&dir, &base, "hash_et.jpg");
        let et_out = oracle
            .command()
            .args(["-overwrite_original", &arg, et_path.to_str().unwrap()])
            .output()
            .expect("run oracle");
        assert!(et_out.status.success());
        assert_eq!(oracle_read_n(oracle, &et_path, tag), code.to_string());
    }

    // `--no-print-conv` / `-n`, applied globally instead of per-tag.
    for (tag, code) in [("GPS:GPSDifferential", 0), ("ExifIFD:Contrast", 1)] {
        let dir = tempfile::tempdir().unwrap();
        let arg = format!("-{tag}={code}");

        let ox_path = copy_into(&dir, &base, "np.jpg");
        let out = oxidex(&["--no-print-conv", &arg, ox_path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "oxidex --no-print-conv {arg} should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(oxidex_read_n(&ox_path, tag), code.to_string());

        let et_path = copy_into(&dir, &base, "np_et.jpg");
        let et_out = oracle
            .command()
            .args(["-overwrite_original", "-n", &arg, et_path.to_str().unwrap()])
            .output()
            .expect("run oracle");
        assert!(et_out.status.success());
        assert_eq!(oracle_read_n(oracle, &et_path, tag), code.to_string());
    }
}

/// PR #959 review (Codex, P2): `CalibrationIlluminant1/2/3` share
/// `LightSource`'s hand-written table (kept hand-written rather than
/// generic specifically because of the duplicated "Daylight" label -- see
/// that match arm's own comment), but the table was matched with a plain
/// case-sensitive `match`, unlike every other label lookup in this fix.
/// `-IFD0:CalibrationIlluminant1=d65` now resolves to 21 exactly like `D65`,
/// via the same `invert_int_enum` (exact, then case-insensitive) the
/// generic path uses -- safe here because `LIGHT_SOURCE_LABELS` omits the
/// code-25 "Daylight" duplicate, so no case-insensitive match is ever
/// ambiguous.
#[test]
fn calibration_illuminant_labels_match_case_insensitively() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (label, code) in [("d65", 21), ("DAYLIGHT", 1), ("fine weather", 9)] {
        assert_label_write_matches_oracle(
            oracle,
            &base,
            "IFD0:CalibrationIlluminant1",
            label,
            code,
        );
    }
}

/// PR #959 review (Codex, round 2, P1): the `Sharpness` (0xa40a) arm is
/// `ConvertParameter`-based, the same family as `Contrast`/`Saturation`, but
/// was missed by the earlier raw-mode audit -- it ran unconditionally, so
/// `-ExifIFD:Sharpness#=1` and `--no-print-conv -ExifIFD:Sharpness=1` turned
/// the caller's raw code `1` into `2` (`ConvertParameter` treats any positive
/// number as "High"). Confirmed against the oracle: a plain, non-raw
/// `-ExifIFD:Sharpness=1` (a `ConvertParameter` *parameter*, not a raw code)
/// really does store `2`, so raw mode's job is specifically to skip that
/// conversion and store the caller's `1` unchanged.
#[test]
fn sharpness_raw_mode_bypasses_convert_parameter() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    let tag = "ExifIFD:Sharpness";

    // `#`
    {
        let dir = tempfile::tempdir().unwrap();
        let arg = format!("-{tag}#=1");

        let ox_path = copy_into(&dir, &base, "sharp_hash.jpg");
        let out = oxidex(&[&arg, ox_path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "oxidex {arg} should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            oxidex_read_n(&ox_path, tag),
            "1",
            "oxidex {arg} must store the raw code, not run it through ConvertParameter"
        );

        let et_path = copy_into(&dir, &base, "sharp_hash_et.jpg");
        let et_out = oracle
            .command()
            .args(["-overwrite_original", &arg, et_path.to_str().unwrap()])
            .output()
            .expect("run oracle");
        assert!(et_out.status.success());
        assert_eq!(oracle_read_n(oracle, &et_path, tag), "1");
    }

    // `--no-print-conv` / `-n`, applied globally instead of per-tag.
    {
        let dir = tempfile::tempdir().unwrap();
        let arg = format!("-{tag}=1");

        let ox_path = copy_into(&dir, &base, "sharp_np.jpg");
        let out = oxidex(&["--no-print-conv", &arg, ox_path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "oxidex --no-print-conv {arg} should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(oxidex_read_n(&ox_path, tag), "1");

        let et_path = copy_into(&dir, &base, "sharp_np_et.jpg");
        let et_out = oracle
            .command()
            .args(["-overwrite_original", "-n", &arg, et_path.to_str().unwrap()])
            .output()
            .expect("run oracle");
        assert!(et_out.status.success());
        assert_eq!(oracle_read_n(oracle, &et_path, tag), "1");
    }

    // Sanity: without raw mode, the plain digit "1" is a `ConvertParameter`
    // PARAMETER, not a raw code, and the oracle really does store `2` for it
    // -- proving the two forms are genuinely different, not coincidentally
    // equal.
    let dir = tempfile::tempdir().unwrap();
    let ox_path = copy_into(&dir, &base, "sharp_label.jpg");
    let out = oxidex(&[&format!("-{tag}=1"), ox_path.to_str().unwrap()]);
    assert!(out.status.success());
    assert_eq!(oxidex_read_n(&ox_path, tag), "2");
}

/// PR #959 review (Codex, round 2, P2): the leaf-only dispatch that inverts a
/// label against the transcribed `Exif::Main` table matched by tag NAME
/// alone, so it also caught a MakerNotes tag sharing that name --
/// `Sony:ExposureMode` (Sony.pm 0x0119) shares its leaf with Exif.pm's own
/// `ExposureMode` (0xa402) but is a different tag with a different meaning.
/// `Canon.jpg` has no Sony MakerNote group, so there is no tag to create
/// either way; what this proves is that oxidex's parser-level fix does not
/// change that outcome -- both tools leave the file byte-identical, matching
/// the more precise unit-level proof in
/// `value_parser::tests::sony_exposure_mode_is_not_inverted_through_exif_main`
/// (which calls the parser directly and confirms it refuses "Auto" rather
/// than silently resolving it against `Exif::Main`).
#[test]
fn sony_exposure_mode_write_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let ox_path = copy_into(&dir, &base, "sony_ox.jpg");
    let ox_before = std::fs::read(&ox_path).unwrap();
    oxidex(&["-Sony:ExposureMode=Auto", ox_path.to_str().unwrap()]);
    assert_eq!(
        std::fs::read(&ox_path).unwrap(),
        ox_before,
        "oxidex must leave a Sony-less file untouched for -Sony:ExposureMode=Auto"
    );

    let et_path = copy_into(&dir, &base, "sony_et.jpg");
    let et_before = std::fs::read(&et_path).unwrap();
    oracle
        .command()
        .args([
            "-overwrite_original",
            "-Sony:ExposureMode=Auto",
            et_path.to_str().unwrap(),
        ])
        .output()
        .expect("run oracle");
    assert_eq!(
        std::fs::read(&et_path).unwrap(),
        et_before,
        "the oracle also leaves it untouched"
    );
}

/// PR #959 review (Codex, round 4, P2): `ComponentsConfiguration`'s raw mode
/// was ignored, so `-ExifIFD:ComponentsConfiguration#="1 2 3 0"` and the
/// `--no-print-conv` equivalent still ran the label-only parser (which only
/// recognizes `Y`/`Cb`/`Cr`/`R`/`G`/`B`/`-`) instead of taking the four raw
/// byte codes directly.
#[test]
fn components_configuration_raw_mode_takes_the_raw_bytes() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    let tag = "ExifIFD:ComponentsConfiguration";

    // `#`
    {
        let dir = tempfile::tempdir().unwrap();
        let arg = format!("-{tag}#=1 2 3 0");

        let ox_path = copy_into(&dir, &base, "cc_hash.jpg");
        let out = oxidex(&[&arg, ox_path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "oxidex {arg} should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(oxidex_read_n(&ox_path, tag), "1 2 3 0");

        let et_path = copy_into(&dir, &base, "cc_hash_et.jpg");
        let et_out = oracle
            .command()
            .args(["-overwrite_original", &arg, et_path.to_str().unwrap()])
            .output()
            .expect("run oracle");
        assert!(et_out.status.success());
        assert_eq!(oracle_read_n(oracle, &et_path, tag), "1 2 3 0");
    }

    // `--no-print-conv` / `-n`
    {
        let dir = tempfile::tempdir().unwrap();
        let arg = format!("-{tag}=1 2 3 0");

        let ox_path = copy_into(&dir, &base, "cc_np.jpg");
        let out = oxidex(&["--no-print-conv", &arg, ox_path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "oxidex --no-print-conv {arg} should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(oxidex_read_n(&ox_path, tag), "1 2 3 0");

        let et_path = copy_into(&dir, &base, "cc_np_et.jpg");
        let et_out = oracle
            .command()
            .args(["-overwrite_original", "-n", &arg, et_path.to_str().unwrap()])
            .output()
            .expect("run oracle");
        assert!(et_out.status.success());
        assert_eq!(oracle_read_n(oracle, &et_path, tag), "1 2 3 0");
    }
}

/// PR #959 review (Codex, round 4, P2): `ColorSpace`, `GPS:GPSStatus`,
/// `GPS:GPSMeasureMode` and `GPS:GPSDestDistanceRef` had no catch-all for an
/// unmatched (non-raw) value, so garbage input reached the plain
/// string/integer parser and was written verbatim with zero validation.
/// Confirmed against the oracle, which refuses all four.
#[test]
fn hand_written_enum_arms_reject_unmatched_garbage_like_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };

    for (tag, bad_value) in [
        // Codex's exact repro: `-ExifIFD:ColorSpace=1` (a bare numeric code,
        // coincidentally `sRGB`'s own stored value) was silently ACCEPTED at
        // 43c8dda8 -- not merely a non-numeric garbage string -- because
        // `ColorSpace` is excluded from the generic enum-inversion dispatch
        // and, with no catch-all of its own, fell through to the plain
        // integer parser, which parses `1`. The oracle refuses it
        // outright: `PrintConv` has no `OTHER`, so a raw code without `#`/
        // `-n` is never accepted, matching this tag's whole `Orientation`-
        // family. `garbage` is included too, to cover the non-numeric case.
        ("ExifIFD:ColorSpace", "1"),
        ("ExifIFD:ColorSpace", "garbage"),
        ("GPS:GPSStatus", "garbage"),
        ("GPS:GPSMeasureMode", "garbage"),
        ("GPS:GPSDestDistanceRef", "garbage"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let arg = format!("-{tag}={bad_value}");

        let ox_path = copy_into(&dir, &base, "garbage_ox.jpg");
        let ox_before = std::fs::read(&ox_path).unwrap();
        let out = oxidex(&[&arg, ox_path.to_str().unwrap()]);
        assert!(
            !out.status.success(),
            "oxidex {arg} must be refused, matching the oracle"
        );
        assert_eq!(
            std::fs::read(&ox_path).unwrap(),
            ox_before,
            "oxidex {arg} must leave the file untouched"
        );

        let et_path = copy_into(&dir, &base, "garbage_et.jpg");
        let et_before = std::fs::read(&et_path).unwrap();
        oracle
            .command()
            .args(["-overwrite_original", &arg, et_path.to_str().unwrap()])
            .output()
            .expect("run oracle");
        assert_eq!(
            std::fs::read(&et_path).unwrap(),
            et_before,
            "the oracle also refuses {arg}"
        );
    }
}

/// The catch-all added above (previous test) must not regress
/// `GPSMeasureMode`/`GPSDestDistanceRef`'s own raw codes, which the oracle
/// accepts via its case-insensitive PREFIX tier (`Writer.pl:3609`) because
/// each code happens to be a unique prefix of its own label -- confirmed by
/// a breadth-measurement regression caught before this landed
/// (`GPSMeasureMode`'s numeric-bare "2" newly refused, pushing mismatched
/// from 6 to 7). `unique_case_insensitive_prefix_match` reproduces exactly
/// this one extra tier for these three small, hand-enumerated GPS arms.
/// These tags are String-typed (the stored code IS the read-back text), so
/// this asserts directly rather than through
/// `assert_label_write_matches_oracle` (which compares against an `i64`).
#[test]
fn gps_measure_mode_and_dest_distance_ref_accept_their_own_codes_via_prefix_tier() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for (tag, code) in [
        ("GPS:GPSMeasureMode", "2"),
        ("GPS:GPSMeasureMode", "3"),
        ("GPS:GPSDestDistanceRef", "K"),
        ("GPS:GPSDestDistanceRef", "M"),
        ("GPS:GPSDestDistanceRef", "N"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let arg = format!("-{tag}={code}");

        let ox_path = copy_into(&dir, &base, "prefix_ox.jpg");
        let out = oxidex(&[&arg, ox_path.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "oxidex {arg} should succeed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            oxidex_read_n(&ox_path, tag),
            code,
            "oxidex read-back for {arg}"
        );

        let et_path = copy_into(&dir, &base, "prefix_et.jpg");
        let et_out = oracle
            .command()
            .args(["-overwrite_original", &arg, et_path.to_str().unwrap()])
            .output()
            .expect("run oracle write");
        assert!(et_out.status.success());
        assert_eq!(
            oracle_read_n(oracle, &et_path, tag),
            code,
            "oracle read-back for {arg}"
        );
    }
}

/// PR #959 review (Codex, round 4, P2): `OffsetTime`/`OffsetTimeOriginal`/
/// `OffsetTimeDigitized` ignored `raw_mode` entirely, so
/// `-ExifIFD:OffsetTime#=Z` / `--no-print-conv -ExifIFD:OffsetTime=Z` still
/// ran `inverse_offset_time` and silently stored `+00:00` instead of the
/// caller's raw string `Z`.
#[test]
fn offset_time_raw_mode_bypasses_inverse_offset_time() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };

    for tag in [
        "ExifIFD:OffsetTime",
        "ExifIFD:OffsetTimeOriginal",
        "ExifIFD:OffsetTimeDigitized",
    ] {
        // `#`
        {
            let dir = tempfile::tempdir().unwrap();
            let arg = format!("-{tag}#=Z");

            let ox_path = copy_into(&dir, &base, "offset_hash.jpg");
            let out = oxidex(&[&arg, ox_path.to_str().unwrap()]);
            assert!(
                out.status.success(),
                "oxidex {arg} should succeed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                oxidex_read_n(&ox_path, tag),
                "Z",
                "oxidex {arg} must store the raw string, not run it through InverseOffsetTime"
            );

            let et_path = copy_into(&dir, &base, "offset_hash_et.jpg");
            let et_out = oracle
                .command()
                .args(["-overwrite_original", &arg, et_path.to_str().unwrap()])
                .output()
                .expect("run oracle");
            assert!(et_out.status.success());
            assert_eq!(oracle_read_n(oracle, &et_path, tag), "Z");
        }

        // `--no-print-conv` / `-n`
        {
            let dir = tempfile::tempdir().unwrap();
            let arg = format!("-{tag}=Z");

            let ox_path = copy_into(&dir, &base, "offset_np.jpg");
            let out = oxidex(&["--no-print-conv", &arg, ox_path.to_str().unwrap()]);
            assert!(
                out.status.success(),
                "oxidex --no-print-conv {arg} should succeed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(oxidex_read_n(&ox_path, tag), "Z");

            let et_path = copy_into(&dir, &base, "offset_np_et.jpg");
            let et_out = oracle
                .command()
                .args(["-overwrite_original", "-n", &arg, et_path.to_str().unwrap()])
                .output()
                .expect("run oracle");
            assert!(et_out.status.success());
            assert_eq!(oracle_read_n(oracle, &et_path, tag), "Z");
        }

        // Sanity: without raw mode, `Z` really is converted to `+00:00`.
        let dir = tempfile::tempdir().unwrap();
        let ox_path = copy_into(&dir, &base, "offset_label.jpg");
        let out = oxidex(&[&format!("-{tag}=Z"), ox_path.to_str().unwrap()]);
        assert!(out.status.success());
        assert_eq!(oxidex_read_n(&ox_path, tag), "+00:00");
    }
}

/// Runs `ox_args` (oxidex) and `et_args` (the pinned oracle) on two fresh
/// copies of `base`, and asserts both tools agree on whether the file was
/// updated (each prints ExifTool's `1 image files updated` summary line, a
/// same-value set included) and, when it was, on the oracle's own raw (`-n`)
/// read-back of every tag in `tags` from both files -- one reader for both,
/// so a display difference in oxidex's reader cannot pass for (or mask) a
/// write difference. When neither updated, oxidex's file must be untouched
/// (the oracle may refuse with a warning or report the file unchanged; both
/// leave the bytes alone). Returns oxidex's output.
fn assert_write_matches_oracle(
    oracle: &exiftool_oracle::Oracle,
    base: &Path,
    ox_args: &[&str],
    et_args: &[&str],
    tags: &[&str],
) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let ox_path = copy_into(&dir, base, "ox.jpg");
    let et_path = copy_into(&dir, base, "et.jpg");
    let before = std::fs::read(&ox_path).unwrap();

    let mut args: Vec<&str> = ox_args.to_vec();
    args.push(ox_path.to_str().unwrap());
    let out = oxidex(&args);
    let et_out = oracle
        .command()
        .arg("-overwrite_original")
        .args(et_args)
        .arg(et_path.to_str().unwrap())
        .output()
        .expect("run oracle write");

    let updated =
        |output: &Output| String::from_utf8_lossy(&output.stdout).contains("1 image files updated");
    let (ox_updated, et_updated) = (updated(&out), updated(&et_out));
    assert_eq!(
        ox_updated,
        et_updated,
        "oxidex {ox_args:?} updated={ox_updated} (stderr {}), oracle {et_args:?} \
         updated={et_updated} (stdout {} stderr {})",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&et_out.stdout),
        String::from_utf8_lossy(&et_out.stderr),
    );
    if !ox_updated {
        assert_eq!(
            std::fs::read(&ox_path).unwrap(),
            before,
            "oxidex {ox_args:?} reported no update, so the file must be untouched"
        );
        return out;
    }
    for tag in tags {
        assert_eq!(
            oracle_read_n(oracle, &ox_path, tag),
            oracle_read_n(oracle, &et_path, tag),
            "oracle read-back of {tag} after oxidex {ox_args:?} / oracle {et_args:?}"
        );
    }
    out
}

/// PR #959 review `4112816741`: raw mode skips GPSLatitudeRef's `OTHER`
/// inversion. Pinned 13.59 stores `#=X` as `X` and refuses `#=South` as
/// `String too long` (string[2]); oxidex used to store `S` and refuse `X`.
#[test]
fn gps_latitude_ref_raw_mode_matches_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for tag in ["GPS:GPSLatitudeRef", "GPS:GPSDestLatitudeRef"] {
        for value in ["X", "S", "South"] {
            let hashed = format!("-{tag}#={value}");
            assert_write_matches_oracle(oracle, &base, &[&hashed], &[&hashed], &[tag]);
            let plain = format!("-{tag}={value}");
            assert_write_matches_oracle(
                oracle,
                &base,
                &["--no-print-conv", &plain],
                &["-n", &plain],
                &[tag],
            );
            // Without raw mode the label is inverted (South -> S, X refused).
            assert_write_matches_oracle(oracle, &base, &[&plain], &[&plain], &[tag]);
        }
    }
}

/// Writer.pl `ReverseLookup`: `Unknown (X)` is the raw value X of a
/// PrintConv hash (hex for `0x..`). The integrator's roll-up accepts the GPS
/// form (a copy of t/images/Ricoh2.jpg's `GPSDestDistanceRef: Unknown ()`);
/// this PR's GPS catch-alls refused it.
#[test]
fn unknown_form_writes_the_raw_value_like_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for arg in [
        "-GPS:GPSStatus=Unknown (X)",
        "-GPS:GPSStatus=Unknown (Xy)",
        "-GPS:GPSDestDistanceRef=Unknown ()",
        "-GPS:GPSMeasureMode=Unknown (4)",
        "-GPS:GPSLatitudeRef=Unknown (Q)",
        "-GPS:GPSLatitudeRef=Unknown (South)",
        "-GPS:GPSDifferential=Unknown (0x10)",
        "-ExifIFD:ColorSpace=Unknown (3)",
        "-IFD0:Orientation=Unknown (9)",
        "-IFD0:Orientation=unknown(0x10)",
        "-ExifIFD:Contrast=Unknown (1)",
    ] {
        let tag = arg[1..].split('=').next().unwrap();
        assert_write_matches_oracle(oracle, &base, &[arg], &[arg], &[tag]);
    }
}

/// PR #959 review `4112816745`: `-AllDates#=VALUE` expands to the three
/// dates, each carrying the raw suffix, as pinned 13.59 does; the suffixed
/// `AllDates#` used to skip the shortcut and become a write to `AllDates`.
#[test]
fn all_dates_raw_suffix_expands_like_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for arg in [
        "-AllDates#=2020:01:02 03:04:05",
        "-EXIF:AllDates#=2020:01:02 03:04:05",
    ] {
        let out = assert_write_matches_oracle(
            oracle,
            &base,
            &[arg],
            &[arg],
            &[
                "ExifIFD:DateTimeOriginal",
                "ExifIFD:CreateDate",
                "IFD0:ModifyDate",
            ],
        );
        assert!(
            out.status.success(),
            "oxidex {arg}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// PR #959 review `4112816750`: `Exif::Main` repeats
/// ChromaticAberrationCorrection/DistortionCorrection. Pinned 13.59 writes
/// `-ExifIFD:ChromaticAberrationCorrection=Yes` to the ExifIFD row 0xa410 and
/// refuses the Sony SubIFD row's `Auto`. oxidex's EXIF writer takes the id
/// from the tag registry (0x7034), so it refuses the write by name rather
/// than store the value under the wrong tag -- which `=Auto` and `#=1` used
/// to do (0x7034 in ExifIFD).
#[test]
fn duplicate_enum_rows_never_write_the_wrong_row() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    // Refused by both tools.
    for arg in [
        "-ExifIFD:ChromaticAberrationCorrection=Auto",
        "-ExifIFD:DistortionCorrection=Auto fixed by lens",
        "-IFD0:DistortionCorrection=Auto",
    ] {
        let tag = arg[1..].split('=').next().unwrap();
        assert_write_matches_oracle(oracle, &base, &[arg], &[arg], &[tag]);
    }
    // The oracle writes 0xa410/0xa40f; oxidex refuses by name, never the
    // registry's SubIFD id.
    for arg in [
        "-ExifIFD:ChromaticAberrationCorrection=Yes",
        "-ExifIFD:ChromaticAberrationCorrection#=1",
        "-IFD0:DistortionCorrection=Yes",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = copy_into(&dir, &base, "dup.jpg");
        let before = std::fs::read(&path).unwrap();
        let out = oxidex(&[arg, path.to_str().unwrap()]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "oxidex {arg} must be refused");
        assert!(
            stderr.contains("oxidex's tag registry addresses it as 0x70"),
            "oxidex {arg}: {stderr}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "{arg}");
    }
}

/// PR #959 review `4112816753`: ExifTool's `Warning: ... / Nothing to do.`
/// framing belongs to a plan whose only request is the unconvertible set.
/// Beside a copy, pinned 13.59 warns and still updates the file, so oxidex
/// must not claim `Nothing to do.` there.
#[test]
fn nothing_to_do_framing_needs_a_single_request_plan() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let dst = copy_into(&dir, &base, "dst.jpg");
    let out = oxidex(&["-IFD0:Orientation=6", dst.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr
            .contains("Warning: Can't convert IFD0:Orientation (not in PrintConv)\nNothing to do."),
        "single request: {stderr}"
    );

    let et_dst = copy_into(&dir, &base, "et_dst.jpg");
    let et_out = oracle
        .command()
        .args([
            "-overwrite_original",
            "-IFD0:Orientation=6",
            "-TagsFromFile",
        ])
        .arg(&base)
        .arg(&et_dst)
        .output()
        .expect("run oracle");
    assert!(
        !String::from_utf8_lossy(&et_out.stderr).contains("Nothing to do."),
        "the oracle proceeds with the copy"
    );
    let src = copy_into(&dir, &base, "src.jpg");
    for args in [
        vec![
            "-IFD0:Orientation=6",
            "-TagsFromFile",
            src.to_str().unwrap(),
        ],
        vec!["-IFD0:Orientation=6", "-all="],
        vec!["-IFD0:Orientation=6", "-IFD0:Artist=x"],
    ] {
        let dst = copy_into(&dir, &base, "dst2.jpg");
        let mut full = args.clone();
        full.push(dst.to_str().unwrap());
        let out = oxidex(&full);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("Nothing to do."),
            "oxidex {args:?} must not claim Nothing to do: {stderr}"
        );
    }
}

/// Codex pre-review of PR #959 (round 5): raw mode (`#`) skips every
/// hand-written PrintConvInv. Where pinned 13.59's raw write is modelled the
/// two tools must agree; where it stores something this port does not model
/// (`#=1,000` stores ISO 1, a non-canonical raw date is kept verbatim),
/// oxidex refuses and leaves the file untouched.
#[test]
fn raw_mode_skips_hand_written_inverses_like_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for arg in [
        "-ExifIFD:ISO#=100 200",
        "-ExifIFD:ISO#=100, 200",
        "-GPS:GPSDateStamp#=2024:01:02",
        "-GPS:GPSDateStamp#=2024:01:02 10:11:12",
        "-GPS:GPSVersionID#=2.3.0.0",
        "-GPS:GPSVersionID#=2 3 0 0",
        "-ExifIFD:SubjectDistance#=5 m",
        "-ExifIFD:FocalLength#=50 mm",
        "-ExifIFD:ShutterSpeedValue#=1/250",
        "-ExifIFD:DateTimeOriginal#=2020:01:02 03:04:05",
    ] {
        let tag = arg[1..].split('#').next().unwrap();
        assert_write_matches_oracle(oracle, &base, &[arg], &[arg], &[tag]);
    }
    for arg in [
        "-ExifIFD:ISO#=1,000",
        "-ExifIFD:DateTimeOriginal#=2020-01-02 03:04:05",
        "-ExifIFD:DateTimeOriginal#=2020:01:02 03:04:05+02:00",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = copy_into(&dir, &base, "raw_refused.jpg");
        let before = std::fs::read(&path).unwrap();
        let out = oxidex(&[arg, path.to_str().unwrap()]);
        assert!(!out.status.success(), "oxidex {arg} must be refused");
        assert_eq!(std::fs::read(&path).unwrap(), before, "{arg}");
    }
}

/// Codex pre-review of PR #959 (round 5, second run): raw FileSource,
/// SceneType and ComponentsConfiguration writes follow each tag's own
/// ValueConvInv / `CheckValue` exactly as pinned 13.59 does (FileSource
/// keeps a non-small-integer verbatim, SceneType masks `chr($val & 0xff)`,
/// ComponentsConfiguration needs exactly four integer bytes).
#[test]
fn raw_undef_enum_writes_match_the_oracle() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for arg in [
        "-ExifIFD:FileSource#=2",
        "-ExifIFD:FileSource#=1.5",
        "-ExifIFD:FileSource#=abc",
        "-ExifIFD:SceneType#=1.5",
        "-ExifIFD:SceneType#=257",
        "-ExifIFD:SceneType#=abc",
        "-ExifIFD:ComponentsConfiguration#=1 2",
        "-ExifIFD:ComponentsConfiguration#=1.5 2 3 0",
        "-ExifIFD:ComponentsConfiguration#=256 0 0 0",
    ] {
        let tag = arg[1..].split('#').next().unwrap();
        assert_write_matches_oracle(oracle, &base, &[arg], &[arg], &[tag]);
    }
}

/// Codex pre-review of PR #959 (round 5, third run): an unconvertible value
/// is ExifTool's warning and only that request is dropped -- pinned 13.59
/// writes Artist for `-IFD0:Orientation=6 -IFD0:Artist=x` and says `Nothing
/// to do.` only when every request is refused. A failed ConvertParameter
/// (`Contrast`) keeps ExifTool's `Error converting value ... (PrintConvInv)`
/// wording, never the hash lookup's `Nothing to do.` framing.
#[test]
fn an_unconvertible_set_is_dropped_and_the_rest_applied() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    let args = ["-IFD0:Orientation=6", "-IFD0:Artist=x"];
    let out = assert_write_matches_oracle(
        oracle,
        &base,
        &args,
        &args,
        &["IFD0:Artist", "IFD0:Orientation"],
    );
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("Warning: Can't convert IFD0:Orientation (not in PrintConv)"),
        "the dropped request is still warned about"
    );
    let args = ["-IFD0:Orientation=6", "-ExifIFD:ColorSpace=9"];
    let out = assert_write_matches_oracle(oracle, &base, &args, &args, &[]);
    assert!(String::from_utf8_lossy(&out.stderr).ends_with("Nothing to do.\n"));

    let dir = tempfile::tempdir().unwrap();
    let path = copy_into(&dir, &base, "contrast.jpg");
    let out = oxidex(&["-ExifIFD:Contrast=bogus", path.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Error converting value for ExifIFD:Contrast (PrintConvInv)")
            && !stderr.contains("Nothing to do."),
        "{stderr}"
    );
}

/// Codex pre-review of PR #959 (round 5, fourth run): a raw date set beside
/// a shift of the same field is the same set/shift conflict as without the
/// `#`. Pinned 13.59 keeps the shifted date there; oxidex used to miss the
/// conflict (`datetimeoriginal#` vs `datetimeoriginal`) and overwrite the
/// shift with the set, so it now refuses the plan like the unsuffixed form.
#[test]
fn a_raw_date_set_beside_its_shift_is_refused() {
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    for set in [
        "-ExifIFD:DateTimeOriginal#=2020:01:02 03:04:05",
        "-AllDates#=2020:01:02 03:04:05",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = copy_into(&dir, &base, "shift.jpg");
        let before = std::fs::read(&path).unwrap();
        let out = oxidex(&[
            set,
            "-DateTimeOriginal+=1:0:0 0:0:0",
            path.to_str().unwrap(),
        ]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{set}: {stderr}");
        assert!(
            stderr.contains("is both set and shifted"),
            "{set}: {stderr}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "{set}");
    }
}

/// `--no-print-conv` (ExifTool's `-n`) applies to a multi-file write too:
/// the batch path built its plan without it, so the raw code the one-file
/// form writes was refused as not in PrintConv. Pinned 13.59 writes both.
#[test]
fn no_print_conv_reaches_the_multi_file_write() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let a = copy_into(&dir, &base, "a.jpg");
    let b = copy_into(&dir, &base, "b.jpg");
    let out = oxidex(&[
        "--no-print-conv",
        "-IFD0:Orientation=6",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for path in [&a, &b] {
        assert_eq!(oracle_read_n(oracle, path, "IFD0:Orientation"), "6");
    }
}

/// Codex pre-review of PR #959 (round 5, fifth run): ExifTool judges each
/// value once, before any file is opened. Pinned 13.59 prints the
/// `Can't convert` warning once for a two-file write, and also for a set a
/// later `-all=` supersedes; oxidex used to repeat it per file and to drop
/// the superseded set without a word.
#[test]
fn an_unconvertible_value_is_warned_about_once_per_command() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    const WARNING: &str = "Warning: Can't convert IFD0:Orientation (not in PrintConv)";
    let dir = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> = ["a.jpg", "b.jpg", "et_a.jpg", "et_b.jpg"]
        .iter()
        .map(|name| copy_into(&dir, &base, name))
        .collect();
    let out = oxidex(&[
        "-IFD0:Orientation=6",
        "-IFD0:Artist=x",
        files[0].to_str().unwrap(),
        files[1].to_str().unwrap(),
    ]);
    let et = oracle
        .command()
        .args([
            "-overwrite_original",
            "-IFD0:Orientation=6",
            "-IFD0:Artist=x",
        ])
        .arg(&files[2])
        .arg(&files[3])
        .output()
        .expect("run oracle");
    let count = |output: &Output| {
        String::from_utf8_lossy(&output.stderr)
            .matches(WARNING)
            .count()
    };
    assert_eq!(count(&et), 1, "the oracle warns once");
    assert_eq!(count(&out), 1, "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success());
    for path in &files {
        assert_eq!(oracle_read_n(oracle, path, "IFD0:Artist"), "x", "{path:?}");
    }

    let path = copy_into(&dir, &base, "clear.jpg");
    let out = oxidex(&["-IFD0:Orientation=6", "-all=", path.to_str().unwrap()]);
    assert_eq!(count(&out), 1, "{}", String::from_utf8_lossy(&out.stderr));
}

/// Codex pre-review of PR #959 (round 5, sixth run): a raw PDF date skips
/// the display-date inverse. Pinned 13.59 stores `#=2020-01-02T03:04:05` as
/// `D:2020-01-02T030405`, which oxidex does not model, so it refuses that
/// spelling; the canonical form (with or without a zone) is stored
/// byte-identically by both tools.
#[test]
fn raw_pdf_dates_match_the_oracle_or_are_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let Some(base) = fixtures::pinned_t_images_fixture_path("PDF.pdf") else {
        eprintln!("skipping: PDF.pdf not resolved from the pinned t/images corpus");
        return;
    };
    let creation_date = |path: &Path| {
        let bytes = std::fs::read(path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let at = text.rfind("/CreationDate").expect("a CreationDate entry");
        text[at..].split(')').next().unwrap().to_string()
    };
    for value in ["2020:01:02 03:04:05", "2020:01:02 03:04:05+02:00"] {
        let dir = tempfile::tempdir().unwrap();
        let ox = copy_into(&dir, &base, "ox.pdf");
        let et = copy_into(&dir, &base, "et.pdf");
        let arg = format!("-PDF:CreateDate#={value}");
        let out = oxidex(&[&arg, ox.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let et_out = oracle
            .command()
            .args(["-overwrite_original", &arg])
            .arg(&et)
            .output()
            .expect("run oracle");
        assert!(et_out.status.success());
        assert_eq!(creation_date(&ox), creation_date(&et), "{arg}");
    }
    let dir = tempfile::tempdir().unwrap();
    let ox = copy_into(&dir, &base, "refused.pdf");
    let before = std::fs::read(&ox).unwrap();
    let out = oxidex(&["-PDF:CreateDate#=2020-01-02T03:04:05", ox.to_str().unwrap()]);
    assert!(!out.status.success());
    assert_eq!(std::fs::read(&ox).unwrap(), before);
}

/// Refusing an inverse that oxidex cannot reproduce must roll back other
/// valid sets in the same transaction and must not create a backup.
#[test]
fn unsupported_inverse_refusals_are_atomic_with_a_valid_companion() {
    let Some(base) = canon_jpg() else {
        panic!("Canon.jpg not resolved from the pinned t/images corpus");
    };
    for arg in [
        "-GPS:GPSDateStamp#=20240102",
        "-GPS:GPSDateStamp=2024:01:02 00:30:00+02:00",
        "-ExifIFD:DateTimeOriginal#=2020-01-02 03:04:05",
        "-ExifIFD:ChromaticAberrationCorrection=Yes",
    ] {
        for companion_first in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let path = copy_into(&dir, &base, "atomic.jpg");
            let before = std::fs::read(&path).unwrap();
            let companion = "-IFD0:Artist=atomic companion";
            let requests = if companion_first {
                [companion, arg]
            } else {
                [arg, companion]
            };
            let out = oxidex(&["--backup", requests[0], requests[1], path.to_str().unwrap()]);
            assert!(
                !out.status.success(),
                "unsupported inverse {arg} must refuse: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(std::fs::read(&path).unwrap(), before, "{arg}");
            assert_eq!(
                std::fs::read_dir(dir.path()).unwrap().count(),
                1,
                "a refused transaction must not create a backup: {arg}"
            );
        }
    }
}

/// Raw SceneType uses Perl's integer coercion before masking, preserving
/// low bits even when an integer or fixed decimal exceeds f64 precision.
#[test]
fn raw_scene_type_preserves_integer_bits_at_numeric_boundaries() {
    let oracle = exiftool_oracle::graded().expect("pinned oracle required");
    let base = canon_jpg().expect("Canon.jpg required");
    for (raw, code) in [
        ("9007199254740993", "1"),
        ("+9007199254740993", "1"),
        ("-9007199254740993", "255"),
        ("9223372036854775807", "255"),
        ("9223372036854775808", "0"),
        ("18446744073709551615", "255"),
        ("-9223372036854775808", "0"),
        ("9007199254740993.5", "1"),
        ("18446744073709551615.0", "255"),
        ("-9007199254740993.5", "255"),
        ("0.999999999999999999999", "0"),
        ("1.5", "1"),
        ("-1.5", "255"),
        ("2.57e2", "1"),
        ("-2.57e2", "255"),
    ] {
        for global_raw in [false, true] {
            let arg = format!(
                "-ExifIFD:SceneType{}={raw}",
                if global_raw { "" } else { "#" }
            );
            let ox_args = if global_raw {
                vec!["--no-print-conv", arg.as_str()]
            } else {
                vec![arg.as_str()]
            };
            let et_args = if global_raw {
                vec!["-n", arg.as_str()]
            } else {
                vec![arg.as_str()]
            };
            let out = assert_write_matches_oracle(
                oracle,
                &base,
                &ox_args,
                &et_args,
                &["ExifIFD:SceneType"],
            );
            assert!(out.status.success(), "{raw}");
            // Pin the oracle result as well as equality between the tools.
            let dir = tempfile::tempdir().unwrap();
            let path = copy_into(&dir, &base, "expected.jpg");
            let mut args = ox_args;
            args.push(path.to_str().unwrap());
            assert!(oxidex(&args).status.success());
            assert_eq!(oracle_read_n(oracle, &path, "ExifIFD:SceneType"), code);
        }
    }
}

#[test]
fn raw_scene_type_unsupported_numeric_ranges_refuse_atomically() {
    let base = canon_jpg().expect("Canon.jpg required");
    for raw in [
        "18446744073709551616",
        "-9223372036854775809",
        "18446744073709551616.5",
        "9007199254740993e0",
        "1e300",
        "1e999",
    ] {
        for global_raw in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = copy_into(&dir, &base, "refused.jpg");
            let before = std::fs::read(&path).unwrap();
            let arg = format!(
                "-ExifIFD:SceneType{}={raw}",
                if global_raw { "" } else { "#" }
            );
            let mut args = vec!["--backup", "-IFD0:Artist=must not commit", arg.as_str()];
            if global_raw {
                args.push("--no-print-conv");
            }
            args.push(path.to_str().unwrap());
            let out = oxidex(&args);
            assert!(!out.status.success(), "{raw}");
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("supported"),
                "range refusal must be explicit: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(std::fs::read(&path).unwrap(), before, "{raw}");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }
}
