//! `oxidex -Artist=x DIR` (and a plain read of a directory) used to print
//! only the `image files updated`/`read` line: pinned ExifTool 13.59 always
//! leads a directory-touching summary with a `%5d directories scanned\n`
//! line first (`exiftool:2067`, from `$countDir` -- `exiftool:4421`
//! increments it once per `ScanDir` call, whether or not that directory held
//! any files). Alongside it, two dispatch gaps in `main.rs` meant a mix of
//! file and directory arguments on one command line either silently dropped
//! every argument but the last (when the last happened to be a directory) or
//! tried to read a directory as if it were a plain file (when it was not
//! last) -- see `handle_multi_file_processing`'s doc comment.
//!
//! Every expected line below is re-measured against the pinned oracle
//! through `exiftool_oracle::graded()` (13.59, `-ver` + `OOXML.docx` ->
//! `DOCX` verified), not hard-coded from a prior run, using temporary
//! directories built from ExifTool's own `t/images` fixtures. Only the
//! lines in ExifTool's own summary vocabulary are compared -- oxidex's own
//! `N files skipped (extension not recognized)` diagnostic
//! (`BatchStats::unidentified`) has no ExifTool counterpart and is
//! deliberately left out of the comparison.

use crate::fixtures::pinned_t_images_fixture_path;
use oxidex::exiftool_oracle::{self, Oracle};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn oxidex_bin() -> &'static str {
    env!("CARGO_BIN_EXE_oxidex")
}

fn run_oxidex(args: &[&str]) -> Output {
    Command::new(oxidex_bin())
        .args(args)
        .output()
        .expect("run oxidex binary")
}

fn run_oracle(oracle: &Oracle, args: &[&str]) -> Output {
    oracle.command().args(args).output().expect("run oracle")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// A JPEG fixture, real enough that ExifTool considers it writable -- unlike
/// e.g. BMP, which a directory *write* scan filters out via `CanWrite`
/// (`scanWritable`, `exiftool:4374-4382`) silently, with no error, no
/// warning and no count anywhere: a divergence real but out of scope here.
/// Panics (skipping the test, matching every other pinned-fixture helper in
/// this suite) when the pinned corpus is not configured.
fn copy_jpeg(name: &str, dest_dir: &Path, dest_name: &str) -> Option<PathBuf> {
    let src = pinned_t_images_fixture_path(name)?;
    let dest = dest_dir.join(dest_name);
    std::fs::copy(&src, &dest).unwrap_or_else(|e| panic!("copy {name} into {dest_dir:?}: {e}"));
    Some(dest)
}

/// Every line in ExifTool's own directory/file summary vocabulary
/// (`exiftool`:2067-2081, 13.59), in the order printed. Anything else --
/// including oxidex's own `unidentified` diagnostic line -- is left out, so
/// the comparison is exactly the lines this feature is about.
fn summary_lines(output: &str) -> Vec<String> {
    const SUFFIXES: &[&str] = &[
        "directories scanned",
        "image files read",
        "image files updated",
        "image files unchanged",
        "files weren't updated due to errors",
        "files could not be read",
    ];
    output
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with(|c: char| c.is_ascii_digit())
                && SUFFIXES.iter().any(|suffix| trimmed.ends_with(suffix))
        })
        .map(str::to_string)
        .collect()
}

macro_rules! require_oracle {
    () => {
        match exiftool_oracle::graded() {
            Some(oracle) => oracle,
            None => {
                eprintln!(
                    "skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)"
                );
                return;
            }
        }
    };
}

// --- read of a directory ---------------------------------------------------

#[test]
fn read_of_a_directory_matches_oracle() {
    let oracle = require_oracle!();
    let dir = TempDir::new().expect("temp dir");
    let Some(_a) = copy_jpeg("Canon.jpg", dir.path(), "Canon.jpg") else {
        return;
    };
    let Some(_b) = copy_jpeg("Casio.jpg", dir.path(), "Casio.jpg") else {
        return;
    };

    let path = dir.path().to_str().unwrap();
    let theirs = run_oracle(oracle, &[path]);
    let ours = run_oxidex(&[path]);

    assert_eq!(
        summary_lines(&stdout(&ours)),
        summary_lines(&stdout(&theirs)),
        "ours: {}\ntheirs: {}",
        stdout(&ours),
        stdout(&theirs)
    );
    assert_eq!(
        summary_lines(&stdout(&ours)),
        vec![
            "    1 directories scanned".to_string(),
            "    2 image files read".to_string()
        ]
    );
}

// --- write to a directory --------------------------------------------------

#[test]
fn write_to_a_directory_matches_oracle() {
    let oracle = require_oracle!();
    let their_dir = TempDir::new().expect("temp dir");
    let our_dir = TempDir::new().expect("temp dir");
    for dest in [their_dir.path(), our_dir.path()] {
        if copy_jpeg("Canon.jpg", dest, "Canon.jpg").is_none() {
            return;
        }
        if copy_jpeg("Casio.jpg", dest, "Casio.jpg").is_none() {
            return;
        }
    }

    let theirs = run_oracle(
        oracle,
        &[
            "-overwrite_original",
            "-Artist=Test",
            their_dir.path().to_str().unwrap(),
        ],
    );
    let ours = run_oxidex(&[
        "-overwrite_original",
        "-Artist=Test",
        our_dir.path().to_str().unwrap(),
    ]);

    assert_eq!(
        summary_lines(&stdout(&ours)),
        summary_lines(&stdout(&theirs)),
        "ours: {}\ntheirs: {}",
        stdout(&ours),
        stdout(&theirs)
    );
    assert_eq!(
        summary_lines(&stdout(&ours)),
        vec![
            "    1 directories scanned".to_string(),
            "    2 image files updated".to_string()
        ]
    );
    // No `*_original` backup litter from either side.
    assert!(std::fs::read_dir(their_dir.path()).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .contains("_original")
    }));
}

// --- -r recursive read and write, counting every directory walked ---------

/// Builds `root/Canon.jpg`, `root/sub1/Casio.jpg`,
/// `root/sub1/sub2/Apple.jpg`, `root/sub3/Nikon.jpg`: four directories
/// (the root plus three nested), four writable JPEGs. Returns `None`
/// (skipping the test) when the pinned corpus is not configured.
fn build_recursive_tree(root: &Path) -> Option<()> {
    std::fs::create_dir_all(root.join("sub1/sub2")).unwrap();
    std::fs::create_dir_all(root.join("sub3")).unwrap();
    copy_jpeg("Canon.jpg", root, "Canon.jpg")?;
    copy_jpeg("Casio.jpg", &root.join("sub1"), "Casio.jpg")?;
    copy_jpeg("Apple.jpg", &root.join("sub1/sub2"), "Apple.jpg")?;
    copy_jpeg("Nikon.jpg", &root.join("sub3"), "Nikon.jpg")?;
    Some(())
}

#[test]
fn recursive_read_counts_every_directory_walked() {
    let oracle = require_oracle!();
    let dir = TempDir::new().expect("temp dir");
    if build_recursive_tree(dir.path()).is_none() {
        return;
    }

    let path = dir.path().to_str().unwrap();
    let theirs = run_oracle(oracle, &["-r", path]);
    let ours = run_oxidex(&["-r", path]);

    assert_eq!(
        summary_lines(&stdout(&ours)),
        summary_lines(&stdout(&theirs)),
        "ours: {}\ntheirs: {}",
        stdout(&ours),
        stdout(&theirs)
    );
    assert_eq!(
        summary_lines(&stdout(&ours)),
        vec![
            "    4 directories scanned".to_string(),
            "    4 image files read".to_string()
        ]
    );

    // Without `-r`, only the root is scanned: the nested files are neither
    // descended into nor counted, exactly as ExifTool's `next unless
    // $recurse` (`exiftool:4358`) never calls `ScanDir` on them.
    let non_recursive = run_oxidex(&[path]);
    assert_eq!(
        summary_lines(&stdout(&non_recursive)),
        vec![
            "    1 directories scanned".to_string(),
            "    1 image files read".to_string()
        ]
    );
}

#[test]
fn recursive_write_counts_every_directory_walked() {
    let oracle = require_oracle!();
    let their_root = TempDir::new().expect("temp dir");
    let our_root = TempDir::new().expect("temp dir");
    for root in [their_root.path(), our_root.path()] {
        if build_recursive_tree(root).is_none() {
            return;
        }
    }

    let theirs = run_oracle(
        oracle,
        &[
            "-overwrite_original",
            "-r",
            "-Artist=Test",
            their_root.path().to_str().unwrap(),
        ],
    );
    let ours = run_oxidex(&[
        "-overwrite_original",
        "-r",
        "-Artist=Test",
        our_root.path().to_str().unwrap(),
    ]);

    assert_eq!(
        summary_lines(&stdout(&ours)),
        summary_lines(&stdout(&theirs)),
        "ours: {}\ntheirs: {}",
        stdout(&ours),
        stdout(&theirs)
    );
    assert_eq!(
        summary_lines(&stdout(&ours)),
        vec![
            "    4 directories scanned".to_string(),
            "    4 image files updated".to_string()
        ]
    );
}

// --- a directory holding no supported files --------------------------------

#[test]
fn directory_with_no_supported_files_matches_oracle() {
    let oracle = require_oracle!();
    let their_dir = TempDir::new().expect("temp dir");
    let our_dir = TempDir::new().expect("temp dir");
    for dir in [their_dir.path(), our_dir.path()] {
        std::fs::write(dir.join("notes.oxidextestunknown"), b"not an image").unwrap();
    }

    // Read.
    let path = their_dir.path().to_str().unwrap();
    let their_read = run_oracle(oracle, &[path]);
    let our_read = run_oxidex(&[our_dir.path().to_str().unwrap()]);
    let expected = vec![
        "    1 directories scanned".to_string(),
        "    0 image files read".to_string(),
    ];
    assert_eq!(summary_lines(&stdout(&their_read)), expected);
    assert_eq!(summary_lines(&stdout(&our_read)), expected);
    // No summary line about a write ExifTool never attempted.
    assert!(!stdout(&our_read).contains("image files updated"));

    // Write: ExifTool's own fallback for "nothing to write" is the read-style
    // `0 image files read`, never `image files updated`
    // (`exiftool`:2076, `$countDir and not $totWr`).
    let their_write = run_oracle(oracle, &["-overwrite_original", "-Artist=Test", path]);
    let our_write = run_oxidex(&[
        "-overwrite_original",
        "-Artist=Test",
        our_dir.path().to_str().unwrap(),
    ]);
    assert_eq!(summary_lines(&stdout(&their_write)), expected);
    assert_eq!(summary_lines(&stdout(&our_write)), expected);
    assert!(!stdout(&our_write).contains("image files updated"));
}

// --- mixed files plus directories on one command line ----------------------

#[test]
fn mixed_files_and_directories_read_matches_oracle() {
    let oracle = require_oracle!();
    let dir = TempDir::new().expect("temp dir");
    let top = TempDir::new().expect("temp dir");
    if copy_jpeg("Canon.jpg", dir.path(), "Canon.jpg").is_none() {
        return;
    }
    let Some(top_file) = copy_jpeg("Casio.jpg", top.path(), "top.jpg") else {
        return;
    };

    let dir_path = dir.path().to_str().unwrap();
    let top_path = top_file.to_str().unwrap();

    for args in [[dir_path, top_path], [top_path, dir_path]] {
        let theirs = run_oracle(oracle, &args);
        let ours = run_oxidex(&args);
        assert_eq!(
            summary_lines(&stdout(&ours)),
            summary_lines(&stdout(&theirs)),
            "{args:?}\nours: {}\ntheirs: {}",
            stdout(&ours),
            stdout(&theirs)
        );
        assert_eq!(
            summary_lines(&stdout(&ours)),
            vec![
                "    1 directories scanned".to_string(),
                "    2 image files read".to_string()
            ],
            "{args:?}"
        );
    }
}

#[test]
fn mixed_files_and_directories_write_matches_oracle() {
    let oracle = require_oracle!();
    let their_dir = TempDir::new().expect("temp dir");
    let their_top = TempDir::new().expect("temp dir");
    let our_dir = TempDir::new().expect("temp dir");
    let our_top = TempDir::new().expect("temp dir");
    if copy_jpeg("Canon.jpg", their_dir.path(), "Canon.jpg").is_none() {
        return;
    }
    if copy_jpeg("Canon.jpg", our_dir.path(), "Canon.jpg").is_none() {
        return;
    }
    let Some(their_top_file) = copy_jpeg("Casio.jpg", their_top.path(), "top.jpg") else {
        return;
    };
    let Some(our_top_file) = copy_jpeg("Casio.jpg", our_top.path(), "top.jpg") else {
        return;
    };

    let theirs = run_oracle(
        oracle,
        &[
            "-overwrite_original",
            "-Artist=Test",
            their_dir.path().to_str().unwrap(),
            their_top_file.to_str().unwrap(),
        ],
    );
    let ours = run_oxidex(&[
        "-overwrite_original",
        "-Artist=Test",
        our_dir.path().to_str().unwrap(),
        our_top_file.to_str().unwrap(),
    ]);

    assert_eq!(
        summary_lines(&stdout(&ours)),
        summary_lines(&stdout(&theirs)),
        "ours: {}\ntheirs: {}",
        stdout(&ours),
        stdout(&theirs)
    );
    assert_eq!(
        summary_lines(&stdout(&ours)),
        vec![
            "    1 directories scanned".to_string(),
            "    2 image files updated".to_string()
        ]
    );
}

// --- an empty directory ------------------------------------------------

#[test]
fn empty_directory_matches_oracle() {
    let oracle = require_oracle!();
    let their_dir = TempDir::new().expect("temp dir");
    let our_dir = TempDir::new().expect("temp dir");

    let expected = vec![
        "    1 directories scanned".to_string(),
        "    0 image files read".to_string(),
    ];

    let their_read = run_oracle(oracle, &[their_dir.path().to_str().unwrap()]);
    let our_read = run_oxidex(&[our_dir.path().to_str().unwrap()]);
    assert_eq!(summary_lines(&stdout(&their_read)), expected);
    assert_eq!(summary_lines(&stdout(&our_read)), expected);

    let their_write = run_oracle(
        oracle,
        &[
            "-overwrite_original",
            "-Artist=Test",
            their_dir.path().to_str().unwrap(),
        ],
    );
    let our_write = run_oxidex(&[
        "-overwrite_original",
        "-Artist=Test",
        our_dir.path().to_str().unwrap(),
    ]);
    assert_eq!(summary_lines(&stdout(&their_write)), expected);
    assert_eq!(summary_lines(&stdout(&our_write)), expected);
}

// --- ExifTool omits the line entirely for plain file arguments -------------

#[test]
fn plain_file_arguments_have_no_directories_scanned_line() {
    let oracle = require_oracle!();
    let Some(a) = pinned_t_images_fixture_path("Canon.jpg") else {
        return;
    };
    let Some(b) = pinned_t_images_fixture_path("Casio.jpg") else {
        return;
    };
    let a = a.to_str().unwrap();
    let b = b.to_str().unwrap();

    for args in [vec![a], vec![a, b]] {
        let args: Vec<&str> = args;
        let theirs = run_oracle(oracle, &args);
        let ours = run_oxidex(&args);
        assert!(
            !stdout(&theirs).contains("directories scanned"),
            "oracle unexpectedly printed a directories-scanned line for {args:?}: {}",
            stdout(&theirs)
        );
        assert!(
            !stdout(&ours).contains("directories scanned"),
            "oxidex must not print a directories-scanned line for plain file arguments {args:?}: {}",
            stdout(&ours)
        );
        assert!(stderr(&ours).is_empty() || !stderr(&ours).contains("directories scanned"));
    }
}

// --- Codex review threads on PR #965 ---------------------------------------
//
// Three P2 findings against 206f336c, verified against the pinned oracle
// (see PR #965's review threads, `chatgpt-codex-connector`):
//
// 1. `PRRT_kwDOQNbr5M6mTOK-`: a mixed-argument expansion that finds no files
//    at all (every directory among the arguments empty or unsupported, no
//    plain file argument) printed the human-readable summary straight to
//    stdout even under `-j`/`-csv`, unlike every other read path in this CLI,
//    which keeps stdout clean for structured output
//    (`handle_batch_processing`'s single-directory path included).
// 2. `PRRT_kwDOQNbr5M6mTOLD`: `collect_paths` aborted the whole command the
//    moment any one top-level argument did not exist, so `oxidex realdir
//    missing.jpg` never read `realdir` at all. Ordinary multi-file
//    processing (no directory in the mix) instead counts a missing path as
//    one more per-file error and keeps going -- pinned 13.59 does the same
//    for a directory mixed with a missing path.
// 3. `PRRT_kwDOQNbr5M6mTOLF`: a recursive (`-r`) walk counted (and read
//    files from) a dot-prefixed subdirectory such as `.git`. ExifTool's
//    default `-r` (`exiftool:4358`, `$recurse == 1`) never descends into a
//    directory whose name starts with `.` -- only `-r.` does.

#[test]
fn structured_output_stays_clean_when_mixed_expansion_finds_nothing() {
    let oracle = require_oracle!();
    for flag in ["-j", "-csv"] {
        let their_a = TempDir::new().expect("temp dir");
        let their_b = TempDir::new().expect("temp dir");
        let our_a = TempDir::new().expect("temp dir");
        let our_b = TempDir::new().expect("temp dir");

        let theirs = run_oracle(
            oracle,
            &[
                flag,
                their_a.path().to_str().unwrap(),
                their_b.path().to_str().unwrap(),
            ],
        );
        let ours = run_oxidex(&[
            flag,
            our_a.path().to_str().unwrap(),
            our_b.path().to_str().unwrap(),
        ]);

        // The oracle puts its summary on stderr and leaves stdout either
        // empty (`-j`) or header-only (`-csv`); either way, stdout never
        // carries the `directories scanned`/`image files` vocabulary. oxidex
        // already keeps stdout free of it for a *single* empty directory
        // (`handle_batch_processing`) -- this pins the same for a mixed,
        // multi-argument expansion that also finds nothing.
        assert!(
            !stdout(&theirs).contains("directories scanned")
                && !stdout(&theirs).contains("image files"),
            "{flag}: oracle stdout unexpectedly carries the summary: {}",
            stdout(&theirs)
        );
        assert!(
            !stdout(&ours).contains("directories scanned")
                && !stdout(&ours).contains("image files"),
            "{flag}: oxidex must not print the human-readable summary to stdout \
             when structured output was requested, got: {}",
            stdout(&ours)
        );
    }
}

#[test]
fn a_missing_path_in_a_mixed_batch_does_not_abort_the_real_directory() {
    let oracle = require_oracle!();
    let their_dir = TempDir::new().expect("temp dir");
    let our_dir = TempDir::new().expect("temp dir");
    if copy_jpeg("Canon.jpg", their_dir.path(), "Canon.jpg").is_none() {
        return;
    }
    if copy_jpeg("Canon.jpg", our_dir.path(), "Canon.jpg").is_none() {
        return;
    }
    let missing = "does-not-exist-oxidex-965.jpg";

    let theirs = run_oracle(oracle, &[their_dir.path().to_str().unwrap(), missing]);
    let ours = run_oxidex(&[our_dir.path().to_str().unwrap(), missing]);

    assert_eq!(theirs.status.code(), Some(1), "oracle: {}", stderr(&theirs));
    assert_eq!(
        ours.status.code(),
        Some(1),
        "oxidex must still exit 1 (the missing path is a real error): {}",
        stderr(&ours)
    );
    let expected = vec![
        "    1 directories scanned".to_string(),
        "    1 image files read".to_string(),
        "    1 files could not be read".to_string(),
    ];
    assert_eq!(summary_lines(&stdout(&theirs)), expected);
    assert_eq!(
        summary_lines(&stdout(&ours)),
        expected,
        "the real directory's file must still be read despite the missing path; got: {}",
        stdout(&ours)
    );
}

#[test]
fn recursive_walk_prunes_hidden_subdirectories_like_exiftool() {
    let oracle = require_oracle!();
    let their_root = TempDir::new().expect("temp dir");
    let our_root = TempDir::new().expect("temp dir");
    for root in [their_root.path(), our_root.path()] {
        std::fs::create_dir_all(root.join("visible")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        if copy_jpeg("Canon.jpg", &root.join("visible"), "Canon.jpg").is_none() {
            return;
        }
        if copy_jpeg("Casio.jpg", &root.join(".hidden"), "Casio.jpg").is_none() {
            return;
        }
    }

    let theirs = run_oracle(oracle, &["-r", their_root.path().to_str().unwrap()]);
    let ours = run_oxidex(&["-r", our_root.path().to_str().unwrap()]);

    let expected = vec![
        "    2 directories scanned".to_string(),
        "    1 image files read".to_string(),
    ];
    assert_eq!(
        summary_lines(&stdout(&theirs)),
        expected,
        "oracle: {}",
        stdout(&theirs)
    );
    assert_eq!(
        summary_lines(&stdout(&ours)),
        expected,
        "a `-r` walk must skip `.hidden` entirely (not descend, not count, not read its \
         files), matching ExifTool's default recursion; got: {}",
        stdout(&ours)
    );
    // The hidden file's own tag must never have been read.
    assert!(
        !stdout(&ours).contains("QV-3000EX"),
        "Casio.jpg must not have been read"
    );
}
