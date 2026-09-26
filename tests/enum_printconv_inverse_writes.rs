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
