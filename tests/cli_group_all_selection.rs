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

#[test]
fn qvci_app1_rows_keep_their_makernote_selection_family() {
    let source = std::fs::read(REPO_FIXTURE).expect("repository JPEG");
    assert_eq!(&source[..2], &[0xff, 0xd8]);
    let mut qvci = vec![0u8; 133];
    qvci[..5].copy_from_slice(b"QVCI\0");
    qvci[98..105].copy_from_slice(b"KX-778\0");
    qvci[114..123].copy_from_slice(b"98082901\0");
    qvci[124..133].copy_from_slice(b"98000829\0");
    let mut jpeg = source[..2].to_vec();
    jpeg.extend_from_slice(&[0xff, 0xe1]);
    jpeg.extend_from_slice(&u16::try_from(qvci.len() + 2).unwrap().to_be_bytes());
    jpeg.extend_from_slice(&qvci);
    jpeg.extend_from_slice(&source[2..]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("qvci.jpg");
    std::fs::write(&path, jpeg).unwrap();

    for args in [
        &["-j", "-G0:1", "-MakerNotes:all"][..],
        &["-j", "-G0:1", "-Casio:all"][..],
    ] {
        let output: serde_json::Value = serde_json::from_str(&stdout(args, &path)).unwrap();
        let row = &output[0];
        assert_eq!(row["MakerNotes:Casio:ModelType"], "KX-778", "{args:?}");
        assert_eq!(
            row["MakerNotes:Casio:ManufactureIndex"], 98082901,
            "{args:?}"
        );
        assert_eq!(
            row["MakerNotes:Casio:ManufactureCode"], 98000829,
            "{args:?}"
        );
        assert!(row.get("Casio:ModelType").is_none(), "{args:?}");
    }
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

// Selector forms whose public CLI rows are fully covered by the pinned
// ExifTool t/images samples. This exercises group membership, exclusions,
// duplicate selection, wildcard names and the -s2 spelling together.
const GRADED_SELECTOR_CASES: &[(&[&str], &[&str])] = &[
    (
        &["-a", "-G1", "-s", "-ExifIFD:all"],
        &[
            "Canon.jpg",
            "Nikon.jpg",
            "ExifTool.jpg",
            "GPS.jpg",
            "Pentax.jpg",
            "Sony.jpg",
        ],
    ),
    (
        &["-G1", "-s", "-EXIF:all"],
        &["ExifTool.jpg", "GPS.jpg", "Apple.jpg", "XMP.jpg"],
    ),
    (
        &["-G1", "-s", "-EXIF:all", "--ISO"],
        &["Sigma.jpg", "ExifTool.jpg", "GPS.jpg"],
    ),
    (&["-s", "-EXIF:all"], &["GPS.jpg"]),
    (&["-G1", "-s", "-XMP-dc:all"], &["XMP.jpg"]),
    (
        &["-G1", "-s", "-all:ISO", "-ExifIFD:*"],
        &["Olympus.jpg", "Sony.jpg", "Panasonic.jpg"],
    ),
    (
        &["-a", "-G1", "-s", "-GPS:all", "-IFD0:all"],
        &["GPS.jpg", "Nikon.jpg"],
    ),
    (&["-G1", "-s2", "-GPS:GPS*", "--GPSVersionID"], &["GPS.jpg"]),
    (&["-s", "-*:Comment"], &["ExifTool.jpg"]),
];

#[test]
fn group_all_selection_matches_pinned_oracle_row_for_row() {
    let Some(oracle) = oxidex::exiftool_oracle::graded() else {
        eprintln!("skipping: pinned ExifTool 13.59 oracle unavailable");
        return;
    };
    let mut calls = 0;
    for &(flags, samples) in GRADED_SELECTOR_CASES {
        for &sample in samples {
            let file = fixtures::required_t_images_fixture_path(sample);
            let expected = oracle
                .command()
                .args(["-config", ""])
                .args(flags)
                .arg(&file)
                .output()
                .unwrap_or_else(|error| panic!("pinned oracle {flags:?} {sample}: {error}"));
            assert!(
                expected.status.success(),
                "pinned oracle {flags:?} {sample}: {}",
                String::from_utf8_lossy(&expected.stderr)
            );
            assert!(
                !expected.stdout.is_empty(),
                "pinned oracle printed no rows for {flags:?} {sample}"
            );
            let actual = oxidex(flags, &file);
            assert!(
                actual.status.success(),
                "oxidex {flags:?} {sample}: {}",
                String::from_utf8_lossy(&actual.stderr)
            );
            assert_eq!(actual.stdout, expected.stdout, "{flags:?} {sample}");
            calls += 1;
        }
    }
    assert_eq!(GRADED_SELECTOR_CASES.len(), 9);
    assert_eq!(calls, 22);
}

#[test]
#[ignore = "explicit CI replay against pinned ExifTool 13.59 t/images RAW fixtures"]
fn canon_and_kyocera_raw_rows_keep_makernotes_family_selection() {
    use oxidex::cli::tag_resolution::{family0_label, family1_label, resolve_requested_tags};
    use oxidex::core::operations::read_metadata;

    let oracle = oxidex::exiftool_oracle::graded().expect("pinned capable 13.59 oracle");
    for (file_name, group1, tag, expected) in [
        ("CanonRaw.crw", "CanonRaw", "FileFormat", "CRW"),
        (
            "KyoceraRaw.raw",
            "KyoceraRaw",
            "FirmwareVersion",
            "Ver. 1.07",
        ),
    ] {
        let file = fixtures::required_t_images_fixture_path(file_name);
        let key = format!("MakerNotes:{group1}:{tag}");
        let native = oracle
            .command()
            .args(["-j", "-G0:1", &format!("-MakerNotes:{tag}")])
            .arg(&file)
            .output()
            .expect("pinned native read");
        assert!(
            native.status.success(),
            "{file_name}: {}",
            String::from_utf8_lossy(&native.stderr)
        );
        let native: serde_json::Value =
            serde_json::from_slice(&native.stdout).expect("native JSON");
        assert_eq!(native[0][&key], expected, "pinned native {file_name}");
        let metadata = read_metadata(&file).expect("public library read");
        let selected = resolve_requested_tags(&metadata, &[format!("MakerNotes:{tag}")], false);
        let row = selected
            .iter()
            .find(|row| row.occurrence.name.as_ref() == tag)
            .expect("public library family-0 selection");
        assert_eq!(family0_label(row.occurrence), "MakerNotes", "{file_name}");
        assert_eq!(family1_label(row.occurrence), group1, "{file_name}");

        let json: serde_json::Value =
            serde_json::from_str(&stdout(&["-j", "-G0:1", "-MakerNotes:all"], &file))
                .expect("selected JSON");
        assert_eq!(json[0][&key], expected, "{file_name}");
        assert!(
            json[0].get(format!("{group1}:{tag}")).is_none(),
            "{file_name}"
        );
        let family1: serde_json::Value =
            serde_json::from_str(&stdout(&["-j", "-G1", &format!("-{group1}:all")], &file))
                .expect("family-1 JSON");
        assert_eq!(
            family1[0][format!("{group1}:{tag}")],
            expected,
            "{file_name}"
        );
        let full = stdout(&["-G0:1", "-s"], &file);
        assert!(
            full.contains(&format!("[MakerNotes:{group1}]")),
            "{file_name}"
        );
        let csv = stdout(&["-csv", "-G0:1", "-MakerNotes:all"], &file);
        assert!(
            csv.contains(&format!("[MakerNotes:{group1}] {tag}")),
            "{file_name}: {csv}"
        );
        let negative: serde_json::Value =
            serde_json::from_str(&stdout(&["-j", "-G0:1", "-XMP:all"], &file))
                .expect("negative JSON");
        assert!(negative[0].get(&key).is_none(), "{file_name}");

        // Family-0 selection is a read identity. A standalone RAW directory
        // is not an ExifIFD:MakerNote<vendor> block for TagsFromFile.
        for selector in [
            "-all",
            "-MakerNotes:all",
            &format!("-MakerNote{group1}"),
            &format!("-MakerNote{}", group1.trim_end_matches("Raw")),
            &format!("-ExifIFD:MakerNote{}", group1.trim_end_matches("Raw")),
        ] {
            let dir = tempfile::tempdir().expect("isolated copy destination");
            let destination = dir.path().join("destination.jpg");
            std::fs::copy(REPO_FIXTURE, &destination).expect("copy destination");
            let before = std::fs::read(&destination).expect("read destination");
            let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
                .args([
                    "-TagsFromFile",
                    file.to_str().expect("UTF-8 fixture"),
                    selector,
                ])
                .arg(&destination)
                .output()
                .expect("copy command");
            assert!(
                output.status.success(),
                "{file_name} {selector}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stderr = String::from_utf8_lossy(&output.stderr);
            let uncopied = stderr
                .split("oxidex cannot write the ")
                .nth(1)
                .unwrap_or("")
                .split(" group(s)")
                .next()
                .unwrap_or("");
            assert!(
                !uncopied.split(", ").any(|group| group == group1),
                "{file_name} {selector}: invented physical RAW block: {stderr}"
            );
            if selector != "-all" {
                assert_eq!(
                    std::fs::read(&destination).unwrap(),
                    before,
                    "{file_name} {selector}"
                );
            }
        }
    }
}

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
