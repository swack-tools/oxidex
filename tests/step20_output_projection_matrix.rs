//! Step 20 (`OVERHAUL_STEP18_DESIGN.md` §2.3 Phase C, "output projection")
//! acceptance matrix: `-a`, `-G*`, group-qualified requests, and
//! `--no-print-conv`, run through the real `oxidex` CLI binary against the
//! pinned ExifTool corpus.
//!
//! Every expectation below is the pinned 13.59 oracle's own answer, run and
//! recorded by the maintainer before this step began (not re-derived here):
//!
//! ```text
//! t/images/ExifTool.jpg:
//!   -Make              -> Canon      (priority winner -- CIFF's, not IFD0's FUJIFILM)
//!   -EXIF:Make         -> FUJIFILM
//!   -a -G1 -s -Make    -> [IFD0] FUJIFILM  AND  [CIFF] Canon, in that order
//!   -n -s -FileSize    -> 26106
//!   -G0:1 -s -FileSize -> [File:System]  FileSize : 26 kB
//! t/images/Canon.jpg:
//!   -n -s -FocalLength -> 34   (default -s -FocalLength -> 34.0 mm)
//! ```
//!
//! oxidex's own `-n` is dry-run (`CliArgs::dry_run`), not ExifTool's
//! `--no-print-conv` -- see `CliArgs::exiftool_compat`'s doc comment -- so
//! every row above that the oracle invoked with `-n` is run against the
//! binary with `--no-print-conv` instead. This is a CLI-spelling
//! substitution only; the semantic being tested (raw/ValueConv form instead
//! of PrintConv) is the same one the oracle exercised.
//!
//! Two fixture roots are checked because the two places this repo's own
//! tooling stages a pinned ExifTool checkout differ: the developer sandbox
//! convention used throughout this repo's other pinned-fixture tests
//! (`/tmp/oxidex-exiftool-cache/exiftool`, see e.g. `tests/mie_trailer_signature.rs`)
//! and CI's own `.github/workflows/ci.yml` download target
//! (`/tmp/exiftool-src`, populated by the "Download pinned ExifTool" step
//! that already runs ahead of `cargo nextest run --all-features` -- the same
//! job this file's tests execute in). Checking both is what makes this a
//! real CI job rather than a local-only one: no new workflow file is
//! needed, because `t/images/ExifTool.jpg` and `t/images/Canon.jpg` are
//! already present in the tarball CI downloads for the tag-table verifier.
//! A run with neither root present (a stripped-down environment) skips with
//! a message instead of failing, matching this repo's existing convention
//! for pinned-fixture tests.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture_root() -> Option<PathBuf> {
    for candidate in [
        "/tmp/oxidex-exiftool-cache/exiftool/t/images",
        "/tmp/exiftool-src/t/images",
    ] {
        let path = Path::new(candidate);
        if path.is_dir() {
            return Some(path.to_path_buf());
        }
    }
    None
}

fn oxidex_bin() -> &'static str {
    env!("CARGO_BIN_EXE_oxidex")
}

/// Runs the built `oxidex` binary and returns its stdout, panicking on a
/// non-UTF8 result (every case here is plain text) but not on a non-zero
/// exit -- some of these invocations intentionally probe edge cases where
/// stdout is still the thing under test.
fn run(args: &[&str]) -> String {
    let output = Command::new(oxidex_bin())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run oxidex {args:?}: {e}"));
    String::from_utf8(output.stdout)
        .unwrap_or_else(|e| panic!("oxidex {args:?} produced non-UTF8 stdout: {e}"))
}

macro_rules! skip_without_fixtures {
    ($root:expr) => {
        match $root {
            Some(root) => root,
            None => {
                eprintln!(
                    "skip: neither /tmp/oxidex-exiftool-cache/exiftool/t/images nor \
                     /tmp/exiftool-src/t/images is present"
                );
                return;
            }
        }
    };
}

/// `-Make` resolves to the priority winner across every group sharing the
/// short name `Make` -- CIFF's `Canon`, not `IFD0`'s `FUJIFILM` -- proving
/// the cross-group arbitration works at all (the naive "first/only group
/// found" answer is wrong here). See `cli::tag_resolution::
/// resolve_requested_tags` and `parsers::jpeg::ciff_app0` (the CIFF-in-APP0
/// occurrence this row depends on existing).
#[test]
fn bare_make_request_resolves_to_the_priority_winner() {
    let root = skip_without_fixtures!(fixture_root());
    let file = root.join("ExifTool.jpg");
    let output = run(&["-s", "-Make", file.to_str().unwrap()]);
    assert!(
        output.contains("Canon"),
        "-Make should resolve to CIFF's Canon, got: {output:?}"
    );
    assert!(
        !output.contains("FUJIFILM"),
        "-Make must not show IFD0's FUJIFILM as the (only) answer: {output:?}"
    );
}

/// `-EXIF:Make` resolves against family 0 (`EXIF`), which is `IFD0`'s real
/// family-0 group despite `IFD0` being what oxidex's own key convention
/// stores as `group0` -- `cli::tag_resolution::resolve_family0`.
#[test]
fn group_qualified_request_resolves_against_family_zero() {
    let root = skip_without_fixtures!(fixture_root());
    let file = root.join("ExifTool.jpg");
    let output = run(&["-s", "-EXIF:Make", file.to_str().unwrap()]);
    assert!(
        output.contains("FUJIFILM"),
        "-EXIF:Make should resolve to IFD0's FUJIFILM, got: {output:?}"
    );
}

/// `-a -G1 -s -Make` lists every retained occurrence, group-1-labeled, in
/// file order: `IFD0` before `CIFF`.
#[test]
fn all_occurrences_with_group_display_lists_both_in_file_order() {
    let root = skip_without_fixtures!(fixture_root());
    let file = root.join("ExifTool.jpg");
    let output = run(&["-a", "-G1", "-s", "-Make", file.to_str().unwrap()]);

    let ifd0_line = output.lines().position(|line| line.contains("[IFD0]"));
    let ciff_line = output.lines().position(|line| line.contains("[CIFF]"));
    let (ifd0_line, ciff_line) = (
        ifd0_line.unwrap_or_else(|| panic!("missing [IFD0] line in: {output:?}")),
        ciff_line.unwrap_or_else(|| panic!("missing [CIFF] line in: {output:?}")),
    );
    assert!(
        output.lines().nth(ifd0_line).unwrap().contains("FUJIFILM"),
        "the [IFD0] line should carry FUJIFILM: {output:?}"
    );
    assert!(
        output.lines().nth(ciff_line).unwrap().contains("Canon"),
        "the [CIFF] line should carry Canon: {output:?}"
    );
    assert!(
        ifd0_line < ciff_line,
        "[IFD0] must print before [CIFF] (file order): {output:?}"
    );
}

/// `--no-print-conv -s -FileSize` selects the raw byte count instead of
/// the fused `"26 kB"` string `extract_file_metadata` stores for default
/// display -- `MetadataMap::insert_occurrence_with_raw`'s whole reason to
/// exist (AGENTS.md's tagmodel/1.5 finding).
#[test]
fn no_print_conv_selects_the_raw_file_size() {
    let root = skip_without_fixtures!(fixture_root());
    let file = root.join("ExifTool.jpg");
    let output = run(&["--no-print-conv", "-s", "-FileSize", file.to_str().unwrap()]);
    assert!(
        output.contains("26106"),
        "--no-print-conv -FileSize should show the raw byte count, got: {output:?}"
    );
    assert!(
        !output.contains("kB"),
        "--no-print-conv must not show the PrintConv-formatted size: {output:?}"
    );
}

/// `-G0:1 -s -FileSize` (no `-a`, no `--no-print-conv`) is unaffected by
/// this step's `File:FileSize` raw-form migration: the display value is
/// exactly what it was before (`"26 kB"`), and the two-family label reads
/// `[File:System]` -- `File`'s real family 0, `System`'s real family 1
/// (`ExifTool.pm:1388-1389`'s `%Extra` override, confirmed against the
/// pinned oracle).
#[test]
fn multi_family_group_display_labels_file_size_correctly() {
    let root = skip_without_fixtures!(fixture_root());
    let file = root.join("ExifTool.jpg");
    let output = run(&["-G0:1", "-s", "-FileSize", file.to_str().unwrap()]);
    assert!(
        output.contains("[File:System]"),
        "expected the [File:System] label, got: {output:?}"
    );
    assert!(
        output.contains("FileSize"),
        "expected the FileSize tag name, got: {output:?}"
    );
    assert!(
        output.contains("26 kB"),
        "default display must still be the PrintConv-formatted size: {output:?}"
    );
}

/// A bare `-FocalLength` request resolves to `ExifIFD:FocalLength`
/// (`34.0 mm`), not `Canon:FocalLength` (`34 mm`) -- Canon.pm:2723-2724's
/// real `Priority => 0` on the MakerNote copy, reproduced by
/// `tiff_helpers::parse_makernote`'s special case, must make the standard
/// EXIF tag win the cross-group arbitration despite both occurrences
/// otherwise tying on priority.
#[test]
fn focal_length_request_resolves_to_the_standard_exif_tag() {
    let root = skip_without_fixtures!(fixture_root());
    let file = root.join("Canon.jpg");
    let output = run(&["-s", "-FocalLength", file.to_str().unwrap()]);
    assert!(
        output.contains("34.0 mm"),
        "-FocalLength should resolve to ExifIFD's 34.0 mm, got: {output:?}"
    );
}

/// The raw form of that same resolved occurrence is the bare quotient
/// (`34`), not the fused `"34 mm"`/`"34.0 mm"` PrintConv strings either
/// source could otherwise show.
#[test]
fn no_print_conv_focal_length_selects_the_raw_quotient() {
    let root = skip_without_fixtures!(fixture_root());
    let file = root.join("Canon.jpg");
    let output = run(&[
        "--no-print-conv",
        "-s",
        "-FocalLength",
        file.to_str().unwrap(),
    ]);
    assert!(
        output.contains("FocalLength: 34\n") || output.trim_end() == "FocalLength: 34",
        "--no-print-conv -FocalLength should show the bare 34, got: {output:?}"
    );
}
