//! `-GROUP:all`, `-all`, `-GROUP:*`, `-*`, wildcard names and `--TAG`
//! exclusions, run through the real `oxidex` binary.
//!
//! Until this suite, every one of these selectors printed nothing and exited
//! 0: `-ExifIFD:all` was read as a request for a tag literally named `all`,
//! and `--ISO` as a request *for* ISO. The rules come from `SetFoundTags`
//! (`ExifTool.pm` 13.59); see `cli::tag_resolution::TagSelection`.
//!
//! Two kinds of check:
//!
//! * the repository fixture, always present, against the pinned ExifTool
//!   13.59 oracle's stdout recorded verbatim (`exiftool` asserted with
//!   `-ver` = 13.59 and the `OOXML.docx` capability probe = `DOCX`);
//! * `t/images` samples graded live against the oracle, byte for byte, in
//!   order. These run wherever an oracle may grade output (CI sets
//!   `OXIDEX_REQUIRE_EXIFTOOL_ORACLE=1`, which turns a missing oracle into a
//!   failure rather than a skip). The cases are ones whose rows OxiDex reads
//!   exactly; samples where the reader itself still differs (occurrence
//!   order, a missing maker-note tag) measure the reader, not selection.

use std::path::Path;
use std::process::{Command, Output};

const REPO_FIXTURE: &str = "tests/fixtures/jpeg/sample_with_exif.jpg";

fn oxidex(args: &[&str], file: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(file)
        .output()
        .unwrap_or_else(|e| panic!("failed to run oxidex {args:?}: {e}"))
}

fn stdout(args: &[&str], file: &Path) -> String {
    let output = oxidex(args, file);
    assert!(
        output.status.success(),
        "oxidex {args:?} {} failed: {}",
        file.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 stdout")
}

fn repo(args: &[&str]) -> String {
    stdout(args, Path::new(REPO_FIXTURE))
}

// ---------------------------------------------------------------------------
// Repository fixture, oracle output recorded verbatim.
// ---------------------------------------------------------------------------

#[test]
fn group_all_lists_the_groups_tags_in_file_order() {
    let expected = "[IFD0]          Make                            : TestCamera\n\
                    [IFD0]          Model                           : TM\n\
                    [IFD0]          ModifyDate                      : 2025:01:15 10:30:00\n";
    assert_eq!(repo(&["-G1", "-s", "-EXIF:all"]), expected);
    // Group names are case-insensitive, and `*` is `all`.
    assert_eq!(repo(&["-G1", "-s", "-exif:*"]), expected);
    assert_eq!(repo(&["-G1", "-s", "-IFD0:all"]), expected);
    assert_eq!(
        repo(&["-s3", "-exif:all"]),
        "TestCamera\nTM\n2025:01:15 10:30:00\n"
    );
}

#[test]
fn exclusions_remove_tags_from_a_group_all_request() {
    assert_eq!(
        repo(&["-G1", "-s", "-EXIF:all", "--Model"]),
        "[IFD0]          Make                            : TestCamera\n\
         [IFD0]          ModifyDate                      : 2025:01:15 10:30:00\n"
    );
    assert_eq!(
        repo(&["-s2", "-IFD0:*", "--Make"]),
        "Model: TM\nModifyDate: 2025:01:15 10:30:00\n"
    );
    // `-x TAG` is the same exclusion.
    assert_eq!(
        repo(&["-s2", "-IFD0:*", "-x", "Make"]),
        "Model: TM\nModifyDate: 2025:01:15 10:30:00\n"
    );
}

#[test]
fn all_group_with_a_tag_name_selects_that_tag() {
    assert_eq!(
        repo(&["-s", "-all:Make"]),
        "Make                            : TestCamera\n"
    );
}

#[test]
fn all_and_name_glob_select_existing_rows() {
    let all = repo(&["-s", "-all"]);
    assert!(all.contains("Make"), "{all}");
    assert!(all.contains("Model"), "{all}");
    let names = repo(&["-s", "-M*"]);
    assert!(names.contains("Make"), "{names}");
    assert!(names.contains("Model"), "{names}");
    assert!(!names.contains("Orientation"), "{names}");
}

#[test]
fn group_all_is_never_silently_empty_in_structured_output() {
    let json: serde_json::Value =
        serde_json::from_str(&repo(&["-j", "-G1", "-EXIF:all"])).expect("JSON");
    let keys: Vec<&str> = json[0]
        .as_object()
        .expect("one object")
        .keys()
        .map(String::as_str)
        .collect();
    for key in ["IFD0:Make", "IFD0:Model", "IFD0:ModifyDate"] {
        assert!(keys.contains(&key), "{key} missing from {keys:?}");
    }
    let csv = repo(&["-csv", "-EXIF:all"]);
    assert!(csv.contains("TestCamera"), "{csv}");
}

#[test]
fn unmodeled_selectors_are_refused_by_name_with_a_failing_exit() {
    for args in [
        &["-s", "-Camera:all"][..],
        &["-s", "-Time:all"][..],
        &["-s", "-Copy1:Make"][..],
        &["-s", "-AllDates"][..],
        &["-s", "-ls-l"][..],
        &["-s", "-Make#"][..],
        &["-s", "-EXIF:*:Make"][..],
        &["-s", "-EXIF:all", "--EXIF:*:Make"][..],
        &["-s", "-EXIF:all", "--Main:all"][..],
        &["-s", "--a"][..],
    ] {
        let output = oxidex(args, Path::new(REPO_FIXTURE));
        let stderr = String::from_utf8_lossy(&output.stderr);
        let selector = args.last().unwrap();
        assert!(!output.status.success(), "{args:?} must fail");
        assert!(output.stdout.is_empty(), "{args:?} printed rows");
        assert!(stderr.contains(selector), "{args:?}: {stderr}");
    }
}

#[test]
fn family5_metadata_paths_fail_closed_for_requests_and_exclusions() {
    for args in [
        &["-G5", "-s", "-JPEG-APP1-IFD0:Make"][..],
        &["-G5", "-s", "-Make", "--JPEG-APP1-IFD0:Make"][..],
        &["-G5", "-s", "-Make", "-x", "JPEG-APP1-IFD0:Make"][..],
        &["-G5", "-s", "-Make", "--JPEG-APP1-IFD0-ExifIFD:Make"][..],
    ] {
        let output = oxidex(args, Path::new(REPO_FIXTURE));
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("family 5 metadata path"),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // Pinned family 0/1 selectors continue to resolve despite a hyphen or
    // an explicit family number.
    assert_eq!(
        repo(&["-G1", "-s", "-0EXIF:Make"]),
        repo(&["-G1", "-s", "-Make"])
    );
    assert_eq!(
        repo(&["-G1", "-s", "-1IFD0:Make"]),
        repo(&["-G1", "-s", "-Make"])
    );
    let xmp_file = Path::new("tests/fixtures/jpeg/sample_with_exif_xmp.jpg");
    assert_eq!(
        stdout(&["-G1", "-s", "-XMP-dc:Title"], xmp_file),
        "[XMP-dc]        Title                           : Sample Photo\n"
    );
}

#[test]
fn multipart_star_group_is_invalid_in_pinned_set_found_tags() {
    // ExifTool 13.59 SetFoundTags validates the entire group with
    // /^[-\w:]*$/ before calling GroupMatches. The latter skips `*`, but
    // public read requests and exclusions never reach it for `EXIF:*`.
    for flags in [
        &["-G1", "-s", "-EXIF:*:Make"][..],
        &["-G1", "-s", "-EXIF:all", "--EXIF:*:Make"][..],
    ] {
        let output = oxidex(flags, Path::new(REPO_FIXTURE));
        assert!(!output.status.success(), "{flags:?}");
        assert!(output.stdout.is_empty(), "{flags:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("invalid group name"),
            "{flags:?}"
        );
    }
}

#[test]
fn copy_exclusion_fails_closed_before_touching_a_destination() {
    let dir = tempfile::tempdir().expect("isolated copy destination");
    let destination = dir.path().join("destination.jpg");
    std::fs::copy(REPO_FIXTURE, &destination).expect("copy fixture");
    let before = std::fs::read(&destination).expect("read destination");
    for exclusion in [&["-x", "IFD0:Make"][..], &["-exclude", "IFD0:Make"][..]] {
        let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-TagsFromFile", REPO_FIXTURE])
            .args(exclusion)
            .arg(&destination)
            .output()
            .expect("run copy-exclusion control");
        assert!(!output.status.success(), "{exclusion:?}");
        assert!(output.stdout.is_empty(), "{exclusion:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("-x/-exclude in copy or write mode is not supported"),
            "{exclusion:?}"
        );
        assert_eq!(
            std::fs::read(&destination).expect("read destination"),
            before,
            "{exclusion:?}"
        );
    }
}

#[test]
fn rename_exclusion_fails_closed_before_moving_a_file() {
    let dir = tempfile::tempdir().expect("isolated rename destination");
    let source = dir.path().join("original.jpg");
    let renamed = dir.path().join("renamed.jpg");
    std::fs::copy(REPO_FIXTURE, &source).expect("copy fixture");
    let before = std::fs::read(&source).expect("read original");
    for exclusion in [
        &["--Make"][..],
        &["-x", "Make"][..],
        &["-exclude", "Make"][..],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(exclusion)
            .arg("-FileName<renamed.jpg")
            .arg(&source)
            .output()
            .expect("run rename-exclusion control");
        assert!(!output.status.success(), "{exclusion:?}");
        assert!(output.stdout.is_empty(), "{exclusion:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("tag exclusions in rename mode are not supported"),
            "{exclusion:?}"
        );
        assert_eq!(std::fs::read(&source).expect("read original"), before);
        assert!(!renamed.exists(), "{exclusion:?} renamed the file");
    }
}

#[path = "common/fixtures.rs"]
mod fixtures;

#[test]
#[ignore = "requires pinned ExifTool 13.59 and combined-samples/Leica/LeicaX1.jpg"]
fn leica_x1_public_read_selectors_match_pinned_oracle() {
    let oracle = oxidex::exiftool_oracle::graded().expect("pinned capable 13.59 oracle");
    let file = fixtures::required_combined_fixture_path("Leica/LeicaX1.jpg");
    for flags in [
        &["-G1", "-s", "-EXIF:all"][..],
        &["-s", "-M*"][..],
        &["-G1", "-s", "-exif:ALL"][..],
        &["-G1", "-s", "-0EXIF:all"][..],
        &["-G1", "-s", "-EXIF:all", "--ISO"][..],
        &["-G1", "-s", "-EXIF:all", "-x", "ISO"][..],
    ] {
        let expected = oracle
            .command()
            .args(flags)
            .arg(&file)
            .output()
            .expect("oracle read");
        assert!(expected.status.success(), "oracle failed for {flags:?}");
        assert!(
            !expected.stdout.is_empty(),
            "oracle selection empty for {flags:?}"
        );
        let actual = oxidex(flags, &file);
        assert!(
            actual.status.success(),
            "oxidex failed for {flags:?}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "selector {flags:?}");
    }
}

#[test]
#[ignore = "explicit CI replay against pinned ExifTool 13.59 t/images/Apple.jpg"]
fn apple_pinned_t_images_read_selectors_match_pinned_oracle() {
    let oracle = oxidex::exiftool_oracle::graded().expect("pinned capable 13.59 oracle");
    let file = fixtures::required_t_images_fixture_path("Apple.jpg");
    for flags in [
        &["-G1", "-s", "-EXIF:all"][..],
        &["-s", "-M*"][..],
        &["-G1", "-s", "-exif:ALL"][..],
        &["-G1", "-s", "-0EXIF:all"][..],
        &["-G1", "-s", "-EXIF:all", "--ISO"][..],
        &["-G1", "-s", "-EXIF:all", "-x", "ISO"][..],
        &["-G1", "-s", "-EXIF:*"][..],
        &["-G1", "-s", "-EXIF:all:Make"][..],
        &["-G1", "-s", "-EXIF:all", "--EXIF:all:Make"][..],
    ] {
        let expected = oracle
            .command()
            .args(flags)
            .arg(&file)
            .output()
            .expect("oracle read");
        assert!(expected.status.success(), "oracle failed for {flags:?}");
        assert!(
            !expected.stdout.is_empty(),
            "oracle selection empty for {flags:?}"
        );
        let actual = oxidex(flags, &file);
        assert!(
            actual.status.success(),
            "oxidex failed for {flags:?}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stdout, expected.stdout, "selector {flags:?}");
    }
}
