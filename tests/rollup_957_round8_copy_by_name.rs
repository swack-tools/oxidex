//! #957 round 8: `-TagsFromFile` copies by name, as pinned ExifTool 13.59's
//! `SetNewValuesFromFile` does -- to the name's preferred group and to every
//! other group the destination already carries it in -- and names, by
//! family-1 group and tag, every destination oxidex cannot write. The four
//! measured cases, pinned; the class is graded in
//! `tests/request_order_matrix.rs::dates_and_copies_match_pinned_exiftool`.
//! Oracle values are pinned ExifTool 13.59 (`perl5.38.2 -I<pinned>/lib
//! <pinned>/exiftool -overwrite_original -TagsFromFile SRC -all DST`;
//! probes `-ver` = 13.59, `OOXML.docx` FileType = DOCX), re-measured through
//! `exiftool_oracle::graded()` where it is available.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{CopyReport, copy_metadata_report};
use oxidex::exiftool_oracle;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const JPEG_EXIF: &str = "tests/fixtures/jpeg/sample_with_exif.jpg";
const PNG: &str = "tests/fixtures/png/sample.png";
const PDF: &str = "tests/fixtures/pdf/sample.pdf";

fn copy_into(dir: &TempDir, fixture: &Path, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn oxidex(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .expect("run oxidex")
}

/// `-a -G1 -s` value of `[group] name` (the pinned oracle when graded,
/// else oxidex's own read).
fn value(path: &Path, group: &str, name: &str) -> Option<String> {
    let arg = format!("-{group}:{name}");
    let o = match exiftool_oracle::graded() {
        Some(oracle) => oracle
            .command()
            .args(["-s3", &arg])
            .arg(path)
            .output()
            .unwrap(),
        None => oxidex(&["-s3", &arg], path),
    };
    let text = String::from_utf8_lossy(&o.stdout).trim_end().to_string();
    (!text.is_empty()).then_some(text)
}

fn named(report: &CopyReport, tag: &str) -> bool {
    report.uncopied_tags.iter().any(|t| t.tag == tag)
        && report
            .uncopied_groups
            .iter()
            .any(|group| tag.starts_with(&format!("{group}:")))
}

/// Case 1: 13.59 writes the source's File ImageWidth/ImageHeight/
/// BitsPerSample/YCbCr* rows into XMP-tiff (IFD0's are Protected) and its
/// ComponentsConfiguration into XMP-exif. oxidex cannot write XMP into a
/// JPEG: it names each, and writes the EXIF.
#[test]
fn a_jpeg_copy_names_the_xmp_13_59_writes_from_file_rows() {
    let dir = TempDir::new().unwrap();
    let dst = copy_into(&dir, Path::new(JPEG_EXIF), "dst.jpg");
    let report = copy_metadata_report(Path::new(JPEG), &dst, None).unwrap();
    for tag in [
        "XMP-tiff:ImageWidth",
        "XMP-tiff:ImageHeight",
        "XMP-tiff:BitsPerSample",
        "XMP-tiff:YCbCrPositioning",
        "XMP-tiff:YCbCrSubSampling",
        "XMP-exif:ComponentsConfiguration",
    ] {
        assert!(named(&report, tag), "{tag}: {report:?}");
    }
    assert_eq!(
        value(&dst, "IFD0", "Make").as_deref(),
        Some("Synthetic Camera Co")
    );
    assert_eq!(
        value(&dst, "ExifIFD", "DateTimeOriginal").as_deref(),
        Some("2024:01:01 12:00:00")
    );

    // The CLI warns with the groups.
    let dst = copy_into(&dir, Path::new(JPEG_EXIF), "cli.jpg");
    let o = oxidex(&["-TagsFromFile", JPEG, "-all"], &dst);
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(
        stderr.contains("oxidex cannot write the") && stderr.contains("XMP-tiff"),
        "{stderr}"
    );
}

/// Case 2: into a PNG, 13.59 writes Make/Model/Artist as PNG text chunks --
/// and to the eXIf IFD0 rows the file already carries. `-TagsFromFile SRC
/// -Make` was refused ("ExifTool writes this name to a PNG as a PNG text
/// tag"); both are written now.
#[test]
fn a_png_copy_writes_text_chunks_and_existing_exif() {
    let dir = TempDir::new().unwrap();
    let dst = copy_into(&dir, Path::new(PNG), "named.png");
    let o = oxidex(&["-TagsFromFile", JPEG, "-Make"], &dst);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(
        value(&dst, "PNG", "Make").as_deref(),
        Some("Synthetic Camera Co")
    );
    assert_eq!(
        value(&dst, "IFD0", "Make").as_deref(),
        Some("Synthetic Camera Co")
    );

    let dst = copy_into(&dir, Path::new(PNG), "all.png");
    let report = copy_metadata_report(Path::new(JPEG), &dst, None).unwrap();
    for (group, name, expected) in [
        ("PNG", "Artist", "Synthetic Artist 1"),
        ("PNG", "Make", "Synthetic Camera Co"),
        ("PNG", "Model", "TestCam 1"),
        ("IFD0", "Artist", "Synthetic Artist 1"),
    ] {
        assert_eq!(
            value(&dst, group, name).as_deref(),
            Some(expected),
            "{group}:{name}"
        );
    }
    assert!(named(&report, "XMP-tiff:ImageWidth"), "{report:?}");
}

/// Case 3: a PDF onto itself: 13.59 also writes XMP-dc/XMP-pdf/XMP-xmp/
/// XMP-prism from the Info fields. oxidex writes the Info dictionary and
/// names the XMP.
#[test]
fn a_pdf_copy_names_the_xmp_13_59_writes_from_its_info() {
    let dir = TempDir::new().unwrap();
    let dst = copy_into(&dir, Path::new(PDF), "dst.pdf");
    let report = copy_metadata_report(Path::new(PDF), &dst, None).unwrap();
    for tag in [
        "XMP-dc:Title",
        "XMP-dc:Creator",
        "XMP-dc:Subject",
        "XMP-pdf:Author",
        "XMP-pdf:Keywords",
        "XMP-pdf:Producer",
        "XMP-xmp:CreateDate",
        "XMP-xmp:ModifyDate",
        "XMP-prism:PageCount",
    ] {
        assert!(named(&report, tag), "{tag}: {report:?}");
    }
    assert_eq!(report.copied, 8, "{report:?}");
    assert_eq!(
        value(&dst, "PDF", "Title").as_deref(),
        Some("Sample PDF for Testing")
    );
}

/// Case 4: t/images/Canon.jpg: 13.59 copies the Canon maker note block
/// whole (oxidex names every Canon tag), writes maker-note values into
/// ExifIFD by name (Canon's MeteringMode is the one `-MeteringMode`
/// reports), and converts each copied value from its printed form, as
/// `-TAG=VALUE` does: FocalPlaneXResolution 3072000/892 prints 3443.946188
/// and is written back as 13.59 rationalises that, 3443.946154.
#[test]
fn a_camera_copy_writes_by_name_and_names_the_maker_notes() {
    let Some(canon) = fixtures::pinned_t_images_fixture_path("Canon.jpg") else {
        eprintln!("skipping: pinned t/images/Canon.jpg not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    let dst = copy_into(&dir, Path::new(JPEG), "dst.jpg");
    let report = copy_metadata_report(&canon, &dst, None).unwrap();
    assert!(
        report.uncopied_groups.iter().any(|g| g == "Canon"),
        "{report:?}"
    );
    assert!(named(&report, "Canon:MeteringMode"), "{report:?}");
    assert_eq!(
        value(&dst, "ExifIFD", "MeteringMode").as_deref(),
        Some("Center-weighted average")
    );
    assert_eq!(
        value(&dst, "ExifIFD", "FocalPlaneXResolution").as_deref(),
        Some("3443.946154")
    );
    assert_eq!(
        value(&dst, "ExifIFD", "MaxApertureValue").as_deref(),
        Some("4.5")
    );
    // Excluding Make leaves the destination's own Make, whose MakerNotes
    // condition the Canon block does not meet: 13.59 writes no block, and
    // oxidex names none.
    let dst = copy_into(&dir, Path::new(JPEG), "nomake.jpg");
    let filters = ["all".to_string(), "-Make".to_string()];
    let report = copy_metadata_report(&canon, &dst, Some(&filters)).unwrap();
    assert!(
        !report.uncopied_groups.iter().any(|g| g == "Canon"),
        "{report:?}"
    );
}

/// Setting IFD0's XResolution makes 13.59 add IFD1's mandatory XResolution
/// (72) to a TIFF whose IFD1 lacks it (WriteExif.pl 13.59:1150-1200;
/// pinned `-IFD0:XResolution=1` on tests/fixtures/tiff/sample.tif:
/// `+ IFD1:XResolution = '72' (mandatory)`). A copy into a TIFF sets it.
#[test]
fn setting_ifd0_resolution_adds_the_ifd1_mandatory_entry() {
    let dir = TempDir::new().unwrap();
    let tif = copy_into(&dir, Path::new("tests/fixtures/tiff/sample.tif"), "a.tif");
    assert_eq!(value(&tif, "IFD1", "XResolution"), None);
    let o = oxidex(&["-IFD0:XResolution=1"], &tif);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(value(&tif, "IFD1", "XResolution").as_deref(), Some("72"));
    assert_eq!(value(&tif, "IFD1", "YResolution"), None);
    assert_eq!(value(&tif, "IFD0", "XResolution").as_deref(), Some("1"));
}
