//! Step 21: single-file vs. batch/directory output equivalence.
//!
//! Step 20 threaded `-a`/`-G*`/`--no-print-conv` through
//! `cli::tag_resolution::resolve_requested_tags` for single-file reads only;
//! `cli::batch_processor` still fed `args.specific_tags()` straight into
//! each `OutputFormatter`'s own exact/suffix `filter_tags` matching,
//! bypassing that resolution entirely. Step 21 routes batch through the same
//! `cli::tag_resolution::resolve_file_output` single-file mode uses (see its
//! doc comment), so a directory read of exactly one file must produce the
//! same *tags* as a single-file read of that file, for every flag
//! combination this module resolves specially.
//!
//! JSON is used for the comparison (rather than diffing raw stdout) because
//! `SourceFile` legitimately differs in shape between the two modes (batch
//! always prints a per-file `SourceFile`/`File:` header line so multiple
//! files in one run stay distinguishable; single-file mode does not need
//! one) -- that is a real, intentional difference in decoration, not in the
//! tags read. Stripping the `SourceFile` *key* from each parsed JSON object
//! isolates exactly the thing this test is about: does batch mode resolve
//! the same tags, under the same keys, with the same values, as single-file
//! mode, for the same file and the same flags.

use serde_json::Value;
use std::process::Command;

/// Filesystem-timestamp tags that are expected to differ between two
/// separate invocations of the same file even when nothing about metadata
/// *resolution* differs -- `File:FileAccessDate` updates on every read (this
/// helper reads the file twice, once per CLI invocation) and
/// `File:FileInodeChangeDate` can tick between them too. Same exclusion set
/// `tests/integration/KNOWN_DISCREPANCIES.md` documents for the ExifTool
/// comparison harness, for the same reason.
const FILESYSTEM_TIMESTAMP_KEYS: [&str; 2] = ["File:FileAccessDate", "File:FileInodeChangeDate"];

fn oxidex(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

/// Runs oxidex with `-j` plus `extra_args` against `path`, parses the JSON
/// array, and returns the first (only) object with `SourceFile` removed.
fn json_tags(path: &str, extra_args: &[&str]) -> Value {
    let mut args: Vec<&str> = vec!["-j"];
    args.extend_from_slice(extra_args);
    args.push(path);
    let output = oxidex(&args);
    assert!(
        output.status.success(),
        "expected oxidex {:?} to succeed: stderr={}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut array: Vec<Value> = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("parse JSON from {args:?}: {e}\nstdout={stdout}"));
    assert_eq!(
        array.len(),
        1,
        "expected exactly one JSON object from {args:?}, got {}: {stdout}",
        array.len()
    );
    let mut obj = array.pop().unwrap();
    if let Some(map) = obj.as_object_mut() {
        map.remove("SourceFile");
        for key in FILESYSTEM_TIMESTAMP_KEYS {
            map.remove(key);
        }
    }
    obj
}

/// Builds a temp directory containing a copy of `fixture`, and returns
/// `(tempdir, single_file_path_string, dir_path_string)`.
///
/// Both paths point through the *same* copied file, rather than the single-
/// file path being the original fixture: `File:Directory` and the
/// filesystem timestamp tags are legitimately path-dependent (see
/// `tests/integration/KNOWN_DISCREPANCIES.md`'s exclusion list), so
/// comparing a read of the original fixture against a read of a directory
/// holding a *copy* would fail on those tags alone, for reasons that have
/// nothing to do with what this test checks (whether batch and single-file
/// resolve the same tags for the same flags).
fn single_file_and_matching_dir(fixture: &str) -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let file_name = std::path::Path::new(fixture)
        .file_name()
        .expect("fixture has a file name");
    let dest = dir.path().join(file_name);
    std::fs::copy(fixture, &dest).expect("copy fixture into temp dir");
    (
        dir,
        dest.to_string_lossy().to_string(),
        dest.parent().unwrap().to_string_lossy().to_string(),
    )
}

/// Runs the same `-j` + `extra_args` invocation once directly against the
/// single file and once against a directory containing only a copy of it,
/// and asserts the resolved tags (everything but `SourceFile`) match.
fn assert_single_file_and_batch_agree(fixture: &str, extra_args: &[&str]) {
    let (_dir, single_path, dir_path) = single_file_and_matching_dir(fixture);

    let single = json_tags(&single_path, extra_args);
    let batch = json_tags(&dir_path, extra_args);

    assert_eq!(
        single, batch,
        "single-file and batch output diverged for {extra_args:?} on {fixture}"
    );
}

#[test]
fn default_listing_matches_between_single_file_and_batch() {
    assert_single_file_and_batch_agree("tests/fixtures/jpeg/sample_with_exif.jpg", &[]);
}

#[test]
fn no_print_conv_matches_between_single_file_and_batch() {
    assert_single_file_and_batch_agree(
        "tests/fixtures/jpeg/sample_with_exif.jpg",
        &["--no-print-conv"],
    );
}

/// The Step 20 gap this step closes: before Step 21, batch mode ignored
/// `-a`/`-G*` outright (never even read `args.all_tags`/`args.group_display`),
/// so a duplicate-tag file resolved differently between the two modes.
/// `canon_sample.jpg` carries a real Canon MakerNote, which is exactly the
/// shape (`IFD0:Make` vs `CIFF`/`Canon`-family duplicates) Step 20's
/// priority/order arbitration exists for.
#[test]
fn all_tags_and_group_display_match_between_single_file_and_batch() {
    assert_single_file_and_batch_agree(
        "tests/fixtures/jpeg/makernotes/canon_sample.jpg",
        &["-a", "-G1", "-Make"],
    );
}

/// Step 21's own extended-output namespace must resolve identically in
/// batch mode too -- not just the Step 20 flags.
#[test]
fn extended_output_matches_between_single_file_and_batch() {
    assert_single_file_and_batch_agree(
        "tests/fixtures/jpeg/makernotes/canon_sample.jpg",
        &["--extended-output"],
    );
}

/// And the default (non-extended) listing on the same MakerNote-bearing
/// fixture must agree too, confirming Step 21's `ReadOptions`/hex-fallback
/// filter is applied identically by both entry points.
#[test]
fn default_listing_on_makernote_fixture_matches_between_single_file_and_batch() {
    assert_single_file_and_batch_agree("tests/fixtures/jpeg/makernotes/canon_sample.jpg", &[]);
}
