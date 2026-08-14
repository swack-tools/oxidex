//! Regression coverage for the recursive-mode silent-drop defect.
//!
//! `oxidex -r <dir>` used to gate every file it walked on a hand-maintained
//! `SUPPORTED_EXTENSIONS` allow-list in `cli::batch_processor`. That list
//! fell behind the ~40+ extensions OxiDex had already grown real parsers
//! for (MP3, ZIP, TXT, FLAC, WAV among them -- see
//! `crate::core::format_dispatch::dispatch_format_parser`), so a directory
//! containing nothing but such files was walked, filtered down to nothing,
//! and reported success having read zero of them: no error, no warning, no
//! count anywhere in the output.
//!
//! `is_supported_file` is now backed by `crate::filetype`'s extension
//! table (generated from ExifTool's own `%fileTypeLookup`) instead, and
//! every file it still declines is counted into
//! `BatchStats::unidentified` and printed in the run's summary rather than
//! dropped. These tests pin both halves of that fix against real files
//! from ExifTool's own `t/images` corpus, not synthetic input.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Root of the pinned ExifTool source tree these tests copy fixtures from.
/// See `.exiftool-version` at the repo root and AGENTS.md's "Never grade
/// against an unpinned ExifTool" -- these tests never invoke `exiftool`
/// itself, only read files out of its checked-out `t/images` corpus, so the
/// pinning rule does not bind here, but the corpus still has to exist on
/// disk for the test to mean anything.
const EXIFTOOL_IMAGES: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images";

fn oxidex(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

fn copy_fixture(fixture_name: &str, dest_dir: &Path) {
    let src = Path::new(EXIFTOOL_IMAGES).join(fixture_name);
    let dest = dest_dir.join(fixture_name);
    fs::copy(&src, &dest)
        .unwrap_or_else(|e| panic!("copy pinned fixture {}: {}", src.display(), e));
}

/// Extensions that are absent from the old hand-maintained
/// `SUPPORTED_EXTENSIONS` list (see the commit that removed it) but that
/// OxiDex has real, dispatched parsers for. `oxidex -r` used to walk right
/// past every one of these with no indication anything was skipped.
const PREVIOUSLY_DROPPED_FIXTURES: &[&str] =
    &["MP3.mp3", "ZIP.zip", "Text1.txt", "FLAC.flac", "RIFF.wav"];

/// Extensions ExifTool's own `%fileTypeLookup` does not define at all
/// (confirmed by grep against the pinned `lib/Image/ExifTool.pm`: neither
/// `'ELF'` nor `'GPX'` appears anywhere in it as a lookup key). These are
/// the genuine "cannot identify" case -- they should still be skipped, but
/// counted, never silently dropped.
const GENUINELY_UNIDENTIFIABLE_FIXTURES: &[&str] = &["EXE.elf", "Geotag.gpx"];

#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn recursive_mode_reads_previously_dropped_extensions() {
    let dir = tempfile::tempdir().expect("create temp dir");
    for fixture in PREVIOUSLY_DROPPED_FIXTURES {
        copy_fixture(fixture, dir.path());
    }

    // JSON mode: every copied file must produce a tagged result. The old
    // extension allow-list would have filtered this list to empty before a
    // single file was ever opened.
    let json_output = oxidex(&["-j", "-r", dir.path().to_str().unwrap()]);
    assert!(
        json_output.status.success(),
        "expected recursive read to succeed: stderr={}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&json_output.stdout);
    let results: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("parse JSON: {e}\n{stdout}"));
    assert_eq!(
        results.len(),
        PREVIOUSLY_DROPPED_FIXTURES.len(),
        "expected one JSON object per copied file, got {}: {stdout}",
        results.len()
    );
    let source_files: Vec<String> = results
        .iter()
        .filter_map(|v| v.get("SourceFile")?.as_str().map(str::to_string))
        .collect();
    for fixture in PREVIOUSLY_DROPPED_FIXTURES {
        assert!(
            source_files.iter().any(|s| s.ends_with(fixture)),
            "expected {fixture} among the read files, got {source_files:?}"
        );
    }

    // Human-readable mode: the run's own summary line has to say so too.
    let human_output = oxidex(&["-r", dir.path().to_str().unwrap()]);
    assert!(human_output.status.success());
    let human_stdout = String::from_utf8_lossy(&human_output.stdout);
    assert!(
        human_stdout.contains(&format!(
            "{} image files read",
            PREVIOUSLY_DROPPED_FIXTURES.len()
        )),
        "expected the summary to report all {} files read, got:\n{human_stdout}",
        PREVIOUSLY_DROPPED_FIXTURES.len()
    );
    assert!(
        !human_stdout.contains("could not be read"),
        "none of these files should have failed to read:\n{human_stdout}"
    );
}

#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn recursive_mode_counts_unidentifiable_files_instead_of_dropping_them() {
    let dir = tempfile::tempdir().expect("create temp dir");
    for fixture in PREVIOUSLY_DROPPED_FIXTURES
        .iter()
        .take(2)
        .chain(GENUINELY_UNIDENTIFIABLE_FIXTURES.iter())
    {
        copy_fixture(fixture, dir.path());
    }

    let output = oxidex(&["-r", dir.path().to_str().unwrap()]);
    assert!(
        output.status.success(),
        "a directory with some unidentifiable files should not itself fail: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Two readable files, two the extension table has never heard of --
    // both halves have to show up as numbers, not as a gap between "4
    // files in the directory" and whatever total the summary prints.
    assert!(
        stdout.contains("2 image files read"),
        "expected exactly 2 files read, got:\n{stdout}"
    );
    assert!(
        stdout.contains("2 files skipped (extension not recognized)"),
        "expected the 2 unidentifiable files to be counted, not silently \
         dropped, got:\n{stdout}"
    );
}
