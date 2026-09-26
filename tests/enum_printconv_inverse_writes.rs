//! CLI parity: writing a tag's print-converted (human-readable) label for an
//! enum tag, the way pinned ExifTool 13.59's inverse PrintConv does.
//!
//! Before the fix, `oxidex "-Orientation=Rotate 90 CW"` failed with
//! `Invalid value for tag 'IFD0:Orientation': Type mismatch: expected
//! Integer but got String` -- the bare tag name had no entry in
//! `value_parser::parse_cli_tag_value`'s declared-type alias table, so its
//! registry type was never resolved and the label reached the writer as a
//! plain `String`. `-IFD0:ResolutionUnit=inches` failed too, with `Not an
//! integer`: the type resolved, but nothing inverted the label at all.
//! `-MeteringMode=Spot` fails for an unrelated, pre-existing reason (a bare
//! name collides with a Canon MakerNote tag of the same name -- see the note
//! on `metering_mode_label_write_matches_oracle` below); `-ExifIFD:
//! MeteringMode=Spot` already worked, because `MeteringMode`'s label set was
//! one of a handful hand-transcribed directly into `value_parser.rs`.
//!
//! `-ExposureProgram=Manual` and `-Flash="Off, Did not fire"` already
//! worked (hand-transcribed tables), but neither tolerated a
//! case-insensitively-spelled label, and neither degraded gracefully on
//! encountering a tag with no hand-written table at all (`Orientation`,
//! `ResolutionUnit`, `ExposureMode`, `Compression`, `YCbCrPositioning`).
//!
//! The fix (`value_parser::invert_enum_printconv`) inverts a label against
//! the transcribed `("Exif", "Main")` / `("GPS", "Main")` IFD tag tables
//! (`exiftool_tables::find_ifd_table`, `docs/TRANSCRIPTION.md`) instead of a
//! hand-maintained list: exact match first, then case-insensitive, and only
//! when exactly one table entry matches either way -- a straight port of
//! ExifTool's own `ReverseLookup` (`Writer.pl:3609-3665`), minus the tie
//! break it applies to an exact-but-duplicate match (see the `Compression`
//! `"JPEG"` case below). `Orientation`, `ResolutionUnit`, `ExposureMode`,
//! `Compression` and `YCbCrPositioning` gained write support for free; the
//! previously-hand-transcribed `ExposureProgram`, `WhiteBalance`,
//! `SceneCaptureType` and `MeteringMode` now route through the same
//! mechanism and additionally accept a case-insensitive spelling.
//!
//! Every reference value below was produced by the pinned oracle
//! (`perl5.38.2 -I.../13.59/exiftool/lib .../13.59/exiftool/exiftool`,
//! `-ver` 13.59 and the `OOXML.docx` capability probe `DOCX` both asserted)
//! against `t/images/Canon.jpg`.
//!
//! # Correction: numeric input does NOT stay accepted
//!
//! This fix's original brief said a plain numeric value should keep being
//! accepted for these tags. Checked directly against the oracle, that was
//! wrong: `-Orientation=6` (no `-n`, no `#`) is `Warning: Can't convert
//! IFD0:Orientation (not in PrintConv)` / `Nothing to do.`, file untouched --
//! ExifTool's `ReverseLookup` never falls back to a raw code for a hash
//! `PrintConv` with no `OTHER`. `value_parser::parse_cli_tag_value_with_mode`
//! now requires a label match for these tags unconditionally (`raw_mode`
//! aside); a bare numeric string only succeeds when it happens to also
//! satisfy the exact/case-insensitive label match (not the case for a plain
//! digit string against any tag in this file's scope). `#` and
//! `--no-print-conv` (oxidex's spelling of ExifTool's `-n`; oxidex's own
//! `-n` is dry-run) still take the raw value directly, unconditionally.
//!
//! ExifTool's real `ReverseLookup` has two more fallback tiers this port
//! does not implement (case-insensitive prefix, then case-insensitive
//! substring) -- ambiguity at either tier still refuses, but a *unique*
//! substring match does not. `-CalibrationIlluminant1=0` is refused here but
//! written as `23` (`D50`) by the oracle, because `"0"` is a substring of
//! `"D50"` and no other label; the same happens for `LightSource` (same
//! table) and for `-Orientation=1`/`-Compression=1` colliding with `"Rotate
//! 180"`/`"CCITT 1D"`. This is a disclosed, deliberate simplification
//! consistent with this fix's stated two-tier algorithm (exact, then
//! case-insensitive) and `AGENTS.md`'s "never approximate a conversion" --
//! refusing is safer than guessing which of several possible substring
//! matches a caller meant. `breadth_measure.py`'s numeric-bare sweep still
//! reports these as `mismatched` rather than silently dropping them.

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
/// tools are given the identical, fully-group-qualified `tag` spelling, so a
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

    // Bare: refused, file untouched, oracle's own wording.
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
    assert!(
        stderr.contains(&format!("Can't convert {tag} (not in PrintConv)")),
        "expected the oracle's own wording in stderr, got: {stderr}"
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
    // updated` / `1 image files unchanged`, exit 0) -- the file being
    // untouched is the invariant this asserts, not the exact wire format.
    let _ = et_out;
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
/// whose OWN labels ("0.1", "0.001", "0.0001", "1e-05", "1e-06") are digit
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
/// match here finds both, so per this fix's stated rule -- "only when
/// exactly one table entry matches" -- oxidex refuses rather than guess.
///
/// The pinned oracle does not refuse: `ReverseLookup`'s exact-match tier
/// does not re-check `$matches > 1` when the pattern already anchors both
/// ends (`Writer.pl:3634-3637`), so it falls through to `sort keys %$conv`
/// and takes the first key whose value matches -- `"7"` sorts before `"99"`
/// as a string, so ExifTool writes `7`. This is a deliberate, disclosed
/// divergence (`AGENTS.md`: never approximate a conversion; a duplicate
/// label is a genuine ambiguity, not a transcription gap), not a
/// transcription bug: oxidex reports it as a refusal (a loud "rejected"),
/// never as a silently wrong code (a "mismatched").
#[test]
fn compression_jpeg_is_ambiguous_and_refused() {
    let Some(base) = canon_jpg() else {
        eprintln!("skipping: Canon.jpg not resolved from the pinned t/images corpus");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let path = copy_into(&dir, &base, "compression_jpeg.jpg");
    let before = std::fs::read(&path).unwrap();
    let out = oxidex(&["-IFD0:Compression=JPEG", path.to_str().unwrap()]);
    assert!(
        !out.status.success(),
        "oxidex should refuse an ambiguous Compression label, not guess"
    );
    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        before, after,
        "a refused write must leave the file untouched"
    );
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
        ("IFD0:Compression", 2), // 1 is excluded: see the module doc on
        // ExifTool's substring-fallback tier -- "1" as a search string
        // happens to match "CCITT 1D" (code 2) as a substring, which this
        // port's exact/case-insensitive-only algorithm does not replicate.
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
