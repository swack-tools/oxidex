//! `-GROUP:All=` group deletions, and PNG writes that change nothing.
//!
//! Every test here failed at e4edc55c. Each pins a 13.59 outcome from the
//! pinned oracle (perl 5.38.2, `-config ""`; probes: `-ver` = 13.59, and
//! `OOXML.docx` FileType = DOCX) on the same fixture:
//!
//! * If the file holds nothing in the group, ExifTool reports `0 image files
//!   updated` / `1 image files unchanged` and leaves the bytes identical.
//! * If the group holds entries (or oxidex cannot prove it empty), ExifTool
//!   deletes them. oxidex does not expand a group deletion yet, so it must
//!   refuse with exit 1 and leave the file untouched, not report an update.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{read_metadata, remove_tag, write_metadata};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const PNG_TEXT: &str = "tests/fixtures/png/simple/synthetic_text_001.png";
const PNG_EXIF: &str = "tests/fixtures/png/sample.png";
const JPEG_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";

/// The eight edits the #945 review ran on t/images/PNG.png. In 13.59 each one
/// is `0 image files updated` / `1 image files unchanged` with the bytes
/// identical, on PNG.png and on synthetic_text_001.png alike.
const NO_OP_EDITS: [&str; 8] = [
    "-GPS:All=",
    "-IFD0:Artist=",
    "-ExifIFD:All=",
    "-IFD1:All=",
    "-InteropIFD:All=",
    "-MakerNotes:All=",
    "-IFD0:All=",
    "-EXIF:All=",
];

fn oxidex(args: &[&str], path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .expect("run oxidex binary")
}

fn copy_into(dir: &TempDir, fixture: &Path, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn assert_each_edit_is_unchanged(fixture: &Path) {
    let original = fs::read(fixture).expect("read fixture");
    for edit in NO_OP_EDITS {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture, "a.png");
        let o = oxidex(&[edit], &file);
        assert_eq!(o.status.code(), Some(0), "{edit}: {}", err(&o));
        assert_eq!(
            out(&o),
            "    0 image files updated\n    1 image files unchanged\n",
            "{edit}: {}",
            err(&o)
        );
        assert!(
            fs::read(&file).unwrap() == original,
            "{edit}: the file was rewritten"
        );
    }
}

/// On t/images/PNG.png (text chunks after IDAT), e4edc55c answered the seven
/// group deletions with `Tag 'GPS:All' is not defined`.
#[test]
fn group_deletions_on_pinned_png_are_unchanged() {
    let Some(fixture) = fixtures::pinned_t_images_fixture_path("PNG.png") else {
        eprintln!("skipping: pinned fixture PNG.png is absent");
        return;
    };
    assert_each_edit_is_unchanged(&fixture);
}

/// The same eight edits on an in-repo PNG. This test always runs.
#[test]
fn group_deletions_on_a_text_png_are_unchanged() {
    assert_each_edit_is_unchanged(Path::new(PNG_TEXT));
}

/// 13.59: `-Foo:All=` prints `Warning: Not a deletable group: Foo`, then
/// `Nothing to do.`, and exits 1.
#[test]
fn an_unknown_group_is_not_deletable() {
    for fixture in [PNG_TEXT, JPEG_XMP] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, Path::new(fixture), "a");
        let before = fs::read(&file).unwrap();
        let o = oxidex(&["-Foo:All="], &file);
        assert_eq!(o.status.code(), Some(1), "{fixture}");
        assert_eq!(
            err(&o),
            "Warning: Not a deletable group: Foo\nNothing to do.\n",
            "{fixture}"
        );
        assert!(fs::read(&file).unwrap() == before, "{fixture}");
    }
}

/// 13.59 updates every one of these files (`1 image files updated`):
/// * sample_with_exif_xmp.jpg holds IFD0, EXIF and XMP entries.
/// * `-Time:All=` strips a text PNG's creation time, although no reader row
///   is keyed `Time:`.
/// * `-GPS:All=x` sets every GPS tag.
///
/// oxidex cannot do any of these yet, so each must be refused.
#[test]
fn a_group_deletion_oxidex_cannot_perform_is_refused() {
    for (fixture, edit) in [
        (JPEG_XMP, "-EXIF:All="),
        (JPEG_XMP, "-IFD0:All="),
        (JPEG_XMP, "-XMP:All="),
        (PNG_EXIF, "-ExifIFD:All="),
        (PNG_TEXT, "-Time:All="),
        (PNG_TEXT, "-GPS:All=x"),
    ] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, Path::new(fixture), "a");
        let before = fs::read(&file).unwrap();
        let o = oxidex(&[edit], &file);
        let label = format!("{fixture} {edit}");
        assert_eq!(o.status.code(), Some(1), "{label}: {}", out(&o));
        assert!(!out(&o).contains("image files updated"), "{label}");
        assert!(err(&o).contains("Error"), "{label}: {}", err(&o));
        assert!(fs::read(&file).unwrap() == before, "{label}: file changed");
    }
}

/// Library no-ops on a PNG leave every byte in place, chunk order included.
/// This covers writing back the map just read, and removing a tag or group
/// the file does not hold. At e4edc55c these went through the PNG rebuild,
/// which moves text chunks that follow IDAT ahead of it.
#[test]
fn library_png_no_ops_leave_the_bytes_identical() {
    let mut fixtures_to_check = vec![PathBuf::from(PNG_TEXT), PathBuf::from(PNG_EXIF)];
    fixtures_to_check.extend(fixtures::pinned_t_images_fixture_path("PNG.png"));
    for fixture in fixtures_to_check {
        let original = fs::read(&fixture).expect("read fixture");
        let dir = TempDir::new().unwrap();

        let file = copy_into(&dir, &fixture, "write.png");
        let metadata = read_metadata(&file).expect("read metadata");
        write_metadata(&file, &metadata).expect("write the same metadata back");
        assert!(
            fs::read(&file).unwrap() == original,
            "{}: write_metadata(read_metadata) rewrote the file",
            fixture.display()
        );

        for tag in ["PNG:Copyright", "IFD1:ImageDescription", "GPS:All"] {
            let file = copy_into(&dir, &fixture, "remove.png");
            remove_tag(&file, tag).unwrap_or_else(|e| panic!("{tag}: {e}"));
            assert!(
                fs::read(&file).unwrap() == original,
                "{}: remove_tag({tag}) rewrote the file",
                fixture.display()
            );
        }
    }
}
