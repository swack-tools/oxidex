//! ExifTool's short output levels (`-s`, `-s2`/`-S`, `-s3`) and their `-G`,
//! `-a`, `--no-print-conv` and multi-file interactions, run through the real
//! `oxidex` binary.
//!
//! Every expected string below is the pinned ExifTool 13.59 oracle's own
//! stdout (`exiftool` asserted with `-ver` = 13.59 and the `OOXML.docx`
//! capability probe = `DOCX`), recorded verbatim -- not re-derived. The rules
//! they pin come from the `exiftool` script's text writer (13.59,
//! `exiftool`:1243-1244 option parsing, :3034-3061 line layout):
//!
//! * `-s` adds 1 to the output level, `-S`/`-veryShort` add 2, and
//!   `-sN`/`-shortN` set it to N -- so `-s -s -s`, `-s -S` and `-s3` are all
//!   level 3;
//! * level 1 prints the tag name padded to 32 columns (`Name<pad>: value`),
//!   preceded under `-G` by `sprintf("%-15s ", "[group]")`, with the name
//!   column shortened by however far a long group label overflowed 15;
//! * level 2 prints `[group] Name: value` / `Name: value` with no padding;
//! * level 3 prints values only, preceded under `-G` by the group label
//!   *without* brackets (`File JPEG`);
//! * a multi-family group label (`-G0:1`) drops empty and adjacent-duplicate
//!   family names (`ExifTool.pm` `GetGroup`, the `$simplify` branch), so
//!   `FileType` is `[File]`, not `[File:File]`;
//! * requested tags print in request order; a tag that is not present prints
//!   nothing;
//! * several files print a `======== FILE` header before each and a
//!   `%5d image files read` summary at the end.
//!
//! oxidex's `-n` is dry-run, so the oracle's `-n` rows run as
//! `--no-print-conv` here (the same substitution
//! `tests/step20_output_projection_matrix.rs` documents).

#[path = "common/fixtures.rs"]
mod fixtures;

use fixtures::pinned_fixture_path;
use std::path::Path;
use std::process::Command;

fn run(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run oxidex {args:?}: {e}"));
    assert!(
        output.status.success(),
        "oxidex {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|e| panic!("oxidex {args:?} produced non-UTF8 stdout: {e}"))
}

fn run_with(options: &[&str], tags: &[&str], files: &[&Path]) -> String {
    let mut args: Vec<&str> = options.to_vec();
    args.extend_from_slice(tags);
    args.extend(files.iter().map(|f| f.to_str().unwrap()));
    run(&args)
}

// ---------------------------------------------------------------------------
// Repository fixture: always present, so these run in every environment.
// ---------------------------------------------------------------------------

const REPO_FIXTURE: &str = "tests/fixtures/jpeg/sample_with_exif.jpg";
const REPO_TAGS: &[&str] = &[
    "-Make",
    "-Model",
    "-FileType",
    "-XResolution",
    "-Nonexistent",
];

fn repo(options: &[&str]) -> String {
    run_with(options, REPO_TAGS, &[Path::new(REPO_FIXTURE)])
}

#[test]
fn repo_fixture_level_one_pads_names_to_32_columns_in_request_order() {
    assert_eq!(
        repo(&["-s"]),
        "Make                            : TestCamera\n\
         Model                           : TM\n\
         FileType                        : JPEG\n"
    );
}

#[test]
fn repo_fixture_level_two_is_unpadded_name_value() {
    let expected = "Make: TestCamera\nModel: TM\nFileType: JPEG\n";
    assert_eq!(repo(&["-s2"]), expected);
    assert_eq!(repo(&["-S"]), expected);
    assert_eq!(repo(&["-s", "-s"]), expected);
}

#[test]
fn repo_fixture_level_three_prints_values_only() {
    let expected = "TestCamera\nTM\nJPEG\n";
    assert_eq!(repo(&["-s3"]), expected);
    assert_eq!(repo(&["-s", "-s", "-s"]), expected);
}

#[test]
fn repo_fixture_group_interactions() {
    assert_eq!(
        repo(&["-G1", "-s"]),
        "[IFD0]          Make                            : TestCamera\n\
         [IFD0]          Model                           : TM\n\
         [File]          FileType                        : JPEG\n"
    );
    assert_eq!(
        repo(&["-G1", "-s3"]),
        "IFD0 TestCamera\nIFD0 TM\nFile JPEG\n"
    );
    assert_eq!(
        repo(&["-G0:1", "-s2"]),
        "[EXIF:IFD0] Make: TestCamera\n[EXIF:IFD0] Model: TM\n[File] FileType: JPEG\n"
    );
}

// ---------------------------------------------------------------------------
// Pinned `t/images/ExifTool.jpg`: two `Make`s (IFD0 FUJIFILM, CIFF Canon) and
// two JPEG `COM` comments, so `-a` and the group labels are exercised.
// ---------------------------------------------------------------------------

const TAGS: &[&str] = &[
    "-Make",
    "-FileType",
    "-Comment",
    "-FileSize",
    "-Nonexistent",
];
const COMMENT: &str = "\u{a9} PhotoStudio Unicode comment..";

fn exiftool_jpg(options: &[&str]) -> Option<String> {
    let file = pinned_fixture_path("ExifTool.jpg")?;
    Some(run_with(options, TAGS, &[file.as_path()]))
}

#[test]
fn level_one_spellings() {
    let expected = format!(
        "Make                            : Canon\n\
         FileType                        : JPEG\n\
         Comment                         : {COMMENT}\n\
         FileSize                        : 26 kB\n"
    );
    for options in [&["-s"][..], &["-short"][..], &["-s1"][..]] {
        let Some(output) = exiftool_jpg(options) else {
            return;
        };
        assert_eq!(output, expected, "{options:?}");
    }
}

#[test]
fn level_two_spellings() {
    let expected = format!("Make: Canon\nFileType: JPEG\nComment: {COMMENT}\nFileSize: 26 kB\n");
    for options in [
        &["-s2"][..],
        &["-S"][..],
        &["-s", "-s"][..],
        &["-veryShort"][..],
        &["-short2"][..],
    ] {
        let Some(output) = exiftool_jpg(options) else {
            return;
        };
        assert_eq!(output, expected, "{options:?}");
    }
}

#[test]
fn level_three_spellings() {
    let expected = format!("Canon\nJPEG\n{COMMENT}\n26 kB\n");
    for options in [
        &["-s3"][..],
        &["-s", "-s", "-s"][..],
        &["-short3"][..],
        &["-s", "-S"][..],
        &["-S", "-s"][..],
    ] {
        let Some(output) = exiftool_jpg(options) else {
            return;
        };
        assert_eq!(output, expected, "{options:?}");
    }
}

#[test]
fn family_zero_group_at_each_level() {
    let Some(level1) = exiftool_jpg(&["-G", "-s"]) else {
        return;
    };
    assert_eq!(
        level1,
        format!(
            "[MakerNotes]    Make                            : Canon\n\
             [File]          FileType                        : JPEG\n\
             [File]          Comment                         : {COMMENT}\n\
             [File]          FileSize                        : 26 kB\n"
        )
    );
}

#[test]
fn family_one_group_at_each_level() {
    let Some(level1) = exiftool_jpg(&["-G1", "-s"]) else {
        return;
    };
    assert_eq!(
        level1,
        format!(
            "[CIFF]          Make                            : Canon\n\
             [File]          FileType                        : JPEG\n\
             [File]          Comment                         : {COMMENT}\n\
             [System]        FileSize                        : 26 kB\n"
        )
    );
    assert_eq!(
        exiftool_jpg(&["-G1", "-s2"]).unwrap(),
        format!(
            "[CIFF] Make: Canon\n[File] FileType: JPEG\n[File] Comment: {COMMENT}\n\
             [System] FileSize: 26 kB\n"
        )
    );
    assert_eq!(
        exiftool_jpg(&["-G1", "-s3"]).unwrap(),
        format!("CIFF Canon\nFile JPEG\nFile {COMMENT}\nSystem 26 kB\n")
    );
}

#[test]
fn multi_family_group_labels_simplify_and_shorten_the_name_column() {
    let Some(level1) = exiftool_jpg(&["-G0:1", "-s"]) else {
        return;
    };
    // `[MakerNotes:CIFF]` is 17 wide, 2 past the 15-column group field, so
    // the name column shrinks from 32 to 30 to keep `:` aligned.
    assert_eq!(
        level1,
        format!(
            "[MakerNotes:CIFF] Make                          : Canon\n\
             [File]          FileType                        : JPEG\n\
             [File]          Comment                         : {COMMENT}\n\
             [File:System]   FileSize                        : 26 kB\n"
        )
    );
    assert_eq!(
        exiftool_jpg(&["-G0:1", "-s2"]).unwrap(),
        format!(
            "[MakerNotes:CIFF] Make: Canon\n[File] FileType: JPEG\n[File] Comment: {COMMENT}\n\
             [File:System] FileSize: 26 kB\n"
        )
    );
    assert_eq!(
        exiftool_jpg(&["-G0:1", "-s3"]).unwrap(),
        format!("MakerNotes:CIFF Canon\nFile JPEG\nFile {COMMENT}\nFile:System 26 kB\n")
    );
}

#[test]
fn all_occurrences_keep_request_order_and_plain_names() {
    let Some(level1) = exiftool_jpg(&["-a", "-s"]) else {
        return;
    };
    assert_eq!(
        level1,
        format!(
            "Make                            : FUJIFILM\n\
             Make                            : Canon\n\
             FileType                        : JPEG\n\
             Comment                         : {COMMENT}\n\
             Comment                         : a comment\n\
             FileSize                        : 26 kB\n"
        )
    );
    assert_eq!(
        exiftool_jpg(&["-a", "-s3"]).unwrap(),
        format!("FUJIFILM\nCanon\nJPEG\n{COMMENT}\na comment\n26 kB\n")
    );
    assert_eq!(
        exiftool_jpg(&["-a", "-G1", "-s"]).unwrap(),
        format!(
            "[IFD0]          Make                            : FUJIFILM\n\
             [CIFF]          Make                            : Canon\n\
             [File]          FileType                        : JPEG\n\
             [File]          Comment                         : {COMMENT}\n\
             [File]          Comment                         : a comment\n\
             [System]        FileSize                        : 26 kB\n"
        )
    );
}

#[test]
fn no_print_conv_level_two() {
    let Some(output) = exiftool_jpg(&["--no-print-conv", "-s2"]) else {
        return;
    };
    assert_eq!(
        output,
        format!("Make: Canon\nFileType: JPEG\nComment: {COMMENT}\nFileSize: 26106\n")
    );
}

#[test]
fn multiple_files_print_headers_and_summary() {
    let (Some(canon), Some(nikon), Some(exiftool)) = (
        pinned_fixture_path("Canon.jpg"),
        pinned_fixture_path("Nikon.jpg"),
        pinned_fixture_path("ExifTool.jpg"),
    ) else {
        return;
    };
    let files = [canon.as_path(), nikon.as_path(), exiftool.as_path()];
    let output = run_with(&["-s3"], &["-Make", "-FileType"], &files);
    assert_eq!(
        output,
        format!(
            "======== {}\nCanon\nJPEG\n======== {}\nNIKON\nJPEG\n======== {}\nCanon\nJPEG\n    \
             3 image files read\n",
            canon.display(),
            nikon.display(),
            exiftool.display()
        )
    );
    // A file with none of the requested tags still gets its header.
    let output = run_with(&["-s3"], &["-Nonexistent"], &files[..2]);
    assert_eq!(
        output,
        format!(
            "======== {}\n======== {}\n    2 image files read\n",
            canon.display(),
            nikon.display()
        )
    );
}

// ---------------------------------------------------------------------------
// Option-order edge cases. ExifTool has no single-letter option clustering:
// `-sa`, `-ss`, `-sS` are unknown tag names to it, so they never change the
// short level, and the level is whatever the standalone spellings produce in
// argument order. Pinned 13.59 on this fixture, `-Make -FileType`:
//
//   -sa -s1  / -s1 -sa  -> level 1
//   -ss -s2  / -s2 -ss  -> level 2
//   -sS -s3             -> level 3
//   -s2 -Make -Model -- -s      (a file named `-s`) -> level 2, reads `-s`
// ---------------------------------------------------------------------------

fn repo_make_file_type(options: &[&str]) -> String {
    run_with(options, &["-Make", "-FileType"], &[Path::new(REPO_FIXTURE)])
}

#[test]
fn a_clustered_s_never_changes_the_short_level() {
    let level1 = "Make                            : TestCamera\n\
                  FileType                        : JPEG\n";
    assert_eq!(repo_make_file_type(&["-sa", "-s1"]), level1);
    assert_eq!(repo_make_file_type(&["-s1", "-sa"]), level1);
    let level2 = "Make: TestCamera\nFileType: JPEG\n";
    assert_eq!(repo_make_file_type(&["-ss", "-s2"]), level2);
    assert_eq!(repo_make_file_type(&["-s2", "-ss"]), level2);
    assert_eq!(repo_make_file_type(&["-sS", "-s3"]), "TestCamera\nJPEG\n");
}

#[test]
fn double_dash_ends_option_recognition() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::copy(REPO_FIXTURE, dir.path().join("-s")).expect("copy fixture to `-s`");
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .current_dir(dir.path())
        .args(["-s2", "-Make", "-Model", "--", "-s"])
        .output()
        .expect("run oxidex");
    assert!(
        output.status.success(),
        "`-- -s` must read the file named `-s`: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Make: TestCamera\nModel: TM\n"
    );
}
