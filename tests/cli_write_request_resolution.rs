//! A `-TAG=VALUE` write either changes the file or says it did not.
//!
//! The defect: `oxidex -XPTitle=v photo.jpg` (no group) printed
//! `1 image files updated`, exited 0, and wrote nothing, while
//! `-IFD0:XPTitle=v` and `-EXIF:XPTitle=v` worked. The ungrouped name reached
//! the EXIF writers unresolved; they only visit `IFD0:`/`ExifIFD:`/`GPS:`/
//! `EXIF:` keys, skipped it, and `main` printed success unconditionally. The
//! same silence covered undefined names (`-NoSuchTag=v`), grouped keys the
//! format's writer does not address (`-XMP:Title=v` in a JPEG), and
//! no-op deletions (`-XPTitle=` where there is none), which rewrote the file
//! and claimed an update.
//!
//! Every expectation below was taken from the pinned oracle (ExifTool 13.59,
//! `perl5.38.2 -I<pinned>/lib <pinned>/exiftool -config ""`, probes `-ver` =
//! 13.59 and `OOXML.docx` FileType = DOCX) on the same fixtures:
//!
//! | command (fixture)                          | ExifTool 13.59                                      |
//! |--------------------------------------------|-----------------------------------------------------|
//! | `-XPTitle=v` (synthetic_001.jpg)           | `[IFD0] XPTitle: v`; `1 image files updated`; rc 0  |
//! | `-Make=v` (synthetic_001.jpg)              | `[IFD0] Make: v`; updated; rc 0                     |
//! | `-XPTitle=` (synthetic_001.jpg, none)      | bytes same; `0 image files updated` / `1 image files unchanged`; rc 0 |
//! | `-NoSuchTag=v`                             | `Warning: Tag 'NoSuchTag' is not defined` / `Nothing to do.`; rc 1 |
//! | `-NoSuchTag=v -XPTitle=v`                  | the warning, then `1 image files updated`; rc 0     |
//! | `-XPTitle=v` (sample.tif)                  | `[IFD0] XPTitle: v`; updated; rc 0                  |
//! | `-Title=v` (sample_with_exif_xmp.jpg)      | writes `[XMP-dc] Title` (oxidex cannot: refused)    |
//! | `-XResolution=300` (tag_matrix_base.jpg)   | writes `[IFD0]` *and* `[JFIF] XResolution` (refused)|
//!
//! Where ExifTool writes a group oxidex cannot, oxidex must refuse with an
//! error and leave the file byte-identical -- never report an update.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const JPEG_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";
const JPEG_JFIF: &str = "tests/fixtures/jpeg/tag_matrix_base.jpg";
const TIFF: &str = "tests/fixtures/tiff/sample.tif";
const PDF: &str = "tests/fixtures/pdf/sample.pdf";
const PNG: &str = "tests/fixtures/png/sample.png";

fn oxidex(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

fn copy_into(dir: &TempDir, fixture: &str, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn sha(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).expect("read file")).to_vec()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The value oxidex reads back for `key` (`-s3` prints the bare value).
fn read_back(path: &Path, key: &str) -> String {
    let out = oxidex(&["-s3", &format!("-{key}"), path.to_str().unwrap()]);
    stdout(&out).lines().next().unwrap_or_default().to_string()
}

fn write(path: &Path, args: &[&str]) -> Output {
    let mut all: Vec<&str> = args.to_vec();
    all.push(path.to_str().unwrap());
    oxidex(&all)
}

#[test]
fn ungrouped_xptitle_writes_ifd0_like_exiftool() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let out = write(&file, &["-XPTitle=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:XPTitle"), "v");
}

#[test]
fn ungrouped_existing_ifd0_tag_is_rewritten() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let out = write(&file, &["-Make=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:Make"), "v");
}

#[test]
fn ungrouped_xptitle_writes_ifd0_in_a_tiff() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, TIFF, "a.tif");
    let out = write(&file, &["-XPTitle=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:XPTitle"), "v");
}

#[test]
fn undefined_tag_is_exiftools_warning_and_nothing_to_do() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let before = sha(&file);
    let out = write(&file, &["-NoSuchTag=v"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout(&out), "");
    assert_eq!(
        stderr(&out),
        "Warning: Tag 'NoSuchTag' is not defined\nNothing to do.\n"
    );
    assert_eq!(sha(&file), before, "file must be untouched");
}

#[test]
fn undefined_tag_beside_a_real_one_warns_and_writes_the_rest() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let out = write(&file, &["-NoSuchTag=v", "-XPTitle=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stderr(&out), "Warning: Tag 'NoSuchTag' is not defined\n");
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:XPTitle"), "v");
}

#[test]
fn deleting_an_absent_tag_reports_unchanged_and_keeps_bytes() {
    for tag in ["-XPTitle=", "-IFD0:XPTitle=", "-EXIF:XPTitle="] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, JPEG, "a.jpg");
        let before = sha(&file);
        let out = write(&file, &[tag]);
        assert_eq!(out.status.code(), Some(0), "{tag}: {}", stderr(&out));
        assert_eq!(
            stdout(&out),
            "    0 image files updated\n    1 image files unchanged\n",
            "{tag}"
        );
        assert_eq!(sha(&file), before, "{tag}: bytes must be untouched");
    }
}

/// ExifTool writes these where oxidex cannot; refusing is the only outcome
/// that is not a lie. The file must stay byte-identical.
#[test]
fn writes_oxidex_cannot_perform_are_refused_not_reported() {
    let cases: &[(&str, &str, &[&str])] = &[
        (JPEG_XMP, "a.jpg", &["-Title=v"]),
        (JPEG_XMP, "a.jpg", &["-XMP:Title=v"]),
        (JPEG, "a.jpg", &["-IPTC:Keywords=k"]),
        (JPEG, "a.jpg", &["-File:Comment=c"]),
        (JPEG_JFIF, "a.jpg", &["-XResolution=300"]),
        (PDF, "a.pdf", &["-Title=v"]),
        (PDF, "a.pdf", &["-XMP:Title=v"]),
        (PNG, "a.png", &["-XMP:Title=v"]),
        // Family-1 XMP spellings (pinned 13.59 writes [XMP-dc] Title in all
        // three formats, creating the XMP packet when there is none); oxidex
        // has no XMP writer, so each must fail loudly.
        (JPEG, "a.jpg", &["-XMP-dc:Title=v"]),
        (JPEG_XMP, "a.jpg", &["-XMP-dc:Title=v"]),
        (JPEG_XMP, "a.jpg", &["-XMP-dc:Title="]),
        (PNG, "a.png", &["-XMP-dc:Title=v"]),
        (TIFF, "a.tif", &["-XMP-dc:Title=v"]),
        // A refused request aborts the whole file: the XPTitle beside it is
        // not half-applied (ExifTool writes all of a file's tags or none).
        (JPEG, "a.jpg", &["-XPTitle=v", "-XMP:Title=v"]),
    ];
    for (fixture, name, args) in cases {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture, name);
        let before = sha(&file);
        let out = write(&file, args);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{fixture} {args:?}: {}",
            stdout(&out)
        );
        assert!(
            !stdout(&out).contains("image files updated")
                || stdout(&out).contains("0 image files updated"),
            "{fixture} {args:?} claimed an update: {}",
            stdout(&out)
        );
        assert!(
            stderr(&out).contains("Error"),
            "{fixture} {args:?}: {}",
            stderr(&out)
        );
        assert_eq!(sha(&file), before, "{fixture} {args:?}: file changed");
    }
}

/// The grouped spellings that already worked keep working.
#[test]
fn grouped_spellings_still_write() {
    for tag in ["-IFD0:XPTitle=v", "-EXIF:XPTitle=v"] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, JPEG, "a.jpg");
        let out = write(&file, &[tag]);
        assert_eq!(out.status.code(), Some(0), "{tag}: {}", stderr(&out));
        assert_eq!(stdout(&out), "    1 image files updated\n", "{tag}");
        assert_eq!(read_back(&file, "IFD0:XPTitle"), "v", "{tag}");
    }
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, PDF, "a.pdf");
    let out = write(&file, &["-PDF:Title=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(read_back(&file, "PDF:Title"), "v");
}

/// Multi-file writes summarize like ExifTool 13.59 (`exiftool -XPTitle= a b`):
/// `    0 image files updated` / `    2 image files unchanged`, no read count.
#[test]
fn multi_file_no_op_deletion_reports_unchanged() {
    let dir = TempDir::new().unwrap();
    let a = copy_into(&dir, JPEG, "a.jpg");
    let b = copy_into(&dir, JPEG, "b.jpg");
    let (ha, hb) = (sha(&a), sha(&b));
    let out = oxidex(&["-XPTitle=", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "    0 image files updated\n    2 image files unchanged\n"
    );
    assert_eq!((sha(&a), sha(&b)), (ha, hb));

    let out = oxidex(&["-XPTitle=v", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    2 image files updated\n");
    assert_eq!(read_back(&a, "IFD0:XPTitle"), "v");
    assert_eq!(read_back(&b, "IFD0:XPTitle"), "v");

    let out = oxidex(&["-NoSuchTag=v", a.to_str().unwrap(), b.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        stderr(&out),
        "Warning: Tag 'NoSuchTag' is not defined\nNothing to do.\n"
    );
}

/// The generic guard: across every spelling and fixture here, a run that
/// prints `1 image files updated` must have written the requested value at
/// the address pinned ExifTool 13.59 writes it to -- checked with a grouped
/// read-back -- and changed the bytes unless that read-back proves the value
/// was already stored; a failed run must leave the file untouched.
///
/// Each spelling carries the one address the oracle writes it to (on these
/// fixtures), or `None` where ExifTool writes a group oxidex cannot (XMP,
/// IPTC, JFIF, File, a PNG's PNG:Software) so any oxidex "update" is wrong.
#[test]
fn updated_count_never_increments_without_a_byte_change() {
    let fixtures = [
        (JPEG, "a.jpg"),
        (JPEG_XMP, "a.jpg"),
        (JPEG_JFIF, "a.jpg"),
        (TIFF, "a.tif"),
        (PDF, "a.pdf"),
        (PNG, "a.png"),
    ];
    let exif = |ext: &str, key: &'static str| (ext != "pdf").then_some(key);
    for (fixture, name) in fixtures {
        let ext = name.rsplit('.').next().unwrap();
        let in_exif_ifd0 = ext == "jpg" || ext == "tif" || ext == "png";
        let args: Vec<(&str, Option<&str>)> = vec![
            ("-XPTitle=v", in_exif_ifd0.then_some("IFD0:XPTitle")),
            ("-XPTitle=", in_exif_ifd0.then_some("IFD0:XPTitle")),
            ("-Make=v", in_exif_ifd0.then_some("IFD0:Make")),
            ("-Make=", in_exif_ifd0.then_some("IFD0:Make")),
            ("-Title=v", None),
            ("-Artist=v", in_exif_ifd0.then_some("IFD0:Artist")),
            ("-Software=v", in_exif_ifd0.then_some("IFD0:Software")),
            ("-NoSuchTag=v", None),
            ("-IFD0:XPTitle=v", exif(ext, "IFD0:XPTitle")),
            ("-EXIF:XPTitle=", exif(ext, "IFD0:XPTitle")),
            ("-EXIF:ISO=100", exif(ext, "ExifIFD:ISO")),
            ("-XMP:Title=v", None),
            ("-XMP:Title=", None),
            ("-IPTC:Keywords=k", None),
            (
                "-IFD1:XResolution=300",
                (ext != "pdf").then_some("IFD1:XResolution"),
            ),
            ("-InteropIFD:InteropIndex=R03", None),
            ("-JFIF:XResolution=300", None),
            ("-File:Comment=c", None),
            ("-PDF:Title=v", (ext == "pdf").then_some("PDF:Title")),
            ("-PDF:Trapped=True", None),
            ("-PNG:Title=v", (ext == "png").then_some("PNG:Title")),
        ];
        for (arg, address) in args {
            let dir = TempDir::new().unwrap();
            let file = copy_into(&dir, fixture, name);
            let before = sha(&file);
            let out = write(&file, &[arg]);
            let changed = sha(&file) != before;
            let text = stdout(&out);
            if out.status.code() != Some(0) {
                assert!(!changed, "{fixture} {arg}: failed but modified the file");
                continue;
            }
            if !text.contains("    1 image files updated") {
                assert!(!changed, "{fixture} {arg}: changed bytes without an update");
                continue;
            }
            let Some(address) = address else {
                panic!("{fixture} {arg}: reported an update ExifTool does not make here");
            };
            let value = arg.split_once('=').unwrap().1;
            if ext == "png" && address == "IFD1:XResolution" {
                assert_eq!(
                    png_ifd1_numerator(&file, 0x011a),
                    value.parse().ok(),
                    "{fixture} {arg}: not at {address}"
                );
            } else {
                assert_eq!(
                    read_back(&file, address),
                    value,
                    "{fixture} {arg}: not at {address}"
                );
            }
            if !changed {
                assert!(
                    !value.is_empty(),
                    "{fixture} {arg}: deletion without a change"
                );
            }
        }
    }
}

/// The same guard covers the other CLI writers. Pinned ExifTool 13.59:
/// `-all=` on a file with nothing left to strip prints `0 image files
/// updated` / `1 image files unchanged` and leaves the bytes alone;
/// `-TagsFromFile SRC -XPTitle` from a source holding `[IFD0] XPTitle :
/// hello` writes it (`1 image files updated`); from a source without one it
/// warns `No writable tags set from SRC` and is unchanged. oxidex rewrote the
/// file and printed `1 image files updated` (`(1 tags copied)`).
#[test]
fn clear_all_and_tags_from_file_report_unchanged_when_nothing_changes() {
    let dir = TempDir::new().unwrap();
    let stripped = copy_into(&dir, JPEG, "stripped.jpg");
    let out = write(&stripped, &["-all="]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");

    let before = sha(&stripped);
    let out = write(&stripped, &["-all="]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "    0 image files updated\n    1 image files unchanged\n"
    );
    assert_eq!(sha(&stripped), before);

    let source = copy_into(&dir, JPEG, "source.jpg");
    assert_eq!(write(&source, &["-XPTitle=hello"]).status.code(), Some(0));
    let dest = copy_into(&dir, JPEG, "dest.jpg");
    let out = write(
        &dest,
        &["-TagsFromFile", source.to_str().unwrap(), "-XPTitle"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&dest, "IFD0:XPTitle"), "hello");

    let dest = copy_into(&dir, JPEG, "dest2.jpg");
    let before = sha(&dest);
    let out = write(
        &dest,
        &["-TagsFromFile", stripped.to_str().unwrap(), "-XPTitle"],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "    0 image files updated\n    1 image files unchanged\n"
    );
    assert!(
        stderr(&out).contains("No writable tags set from"),
        "{}",
        stderr(&out)
    );
    assert_eq!(sha(&dest), before);
}

/// `--backup` copies the original only when the write changes it (ExifTool
/// makes no `_original` for an unchanged file).
#[test]
fn backup_is_made_only_for_an_update() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let backup = dir.path().join("a.jpg.bak");
    let out = write(&file, &["--backup", "-XPTitle="]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!backup.exists(), "no backup for an unchanged file");
    let original = sha(&file);
    let out = write(&file, &["--backup", "-XPTitle=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(sha(&backup), original, "backup holds the pre-write bytes");
}

/// ExifTool counts a write of the value already stored as an update even
/// when its rewrite leaves the bytes identical. Pinned 13.59 (fixtures as
/// named; `xp.jpg` is synthetic_001.jpg after `-XPTitle=v`):
///
/// | command                                        | ExifTool 13.59                      |
/// |------------------------------------------------|-------------------------------------|
/// | `-Make='Synthetic Camera Co'` (stored value)   | bytes same; `1 image files updated` |
/// | `-XPTitle=v` / `-IFD0:XPTitle=v` (xp.jpg)      | bytes same; `1 image files updated` |
/// | `-Artist='Synthetic Artist 1'` (stored value)  | bytes same; `1 image files updated` |
/// | `-Make=TestCamera` (sample.tif, stored value)  | `1 image files updated`             |
/// | `-Make=<stored> -XPTitle=` (XPTitle absent)    | bytes same; `1 image files updated` |
/// | `-Make=<stored> -XPTitle=v` (one new value)    | `1 image files updated`             |
/// | `-XPTitle=` (absent; deletion only)            | `0 image files updated` / `1 image files unchanged` |
///
/// oxidex leaves the bytes alone for these and reports `updated` only when a
/// read-back proves every requested value is stored and every requested
/// deletion absent.
#[test]
fn rewriting_a_stored_value_reports_updated_like_exiftool() {
    let dir = TempDir::new().unwrap();
    let xp = copy_into(&dir, JPEG, "xp.jpg");
    assert_eq!(write(&xp, &["-XPTitle=v"]).status.code(), Some(0));
    let cases: &[(&Path, &str, &[&str])] = &[
        (Path::new(JPEG), "a.jpg", &["-Make=Synthetic Camera Co"]),
        (&xp, "a.jpg", &["-XPTitle=v"]),
        (&xp, "a.jpg", &["-IFD0:XPTitle=v"]),
        (&xp, "a.jpg", &["-EXIF:XPTitle=v"]),
        (Path::new(JPEG), "a.jpg", &["-Artist=Synthetic Artist 1"]),
        (Path::new(TIFF), "a.tif", &["-Make=TestCamera"]),
        (
            Path::new(JPEG),
            "a.jpg",
            &["-Make=Synthetic Camera Co", "-XPTitle="],
        ),
    ];
    for (fixture, name, args) in cases {
        let work = TempDir::new().unwrap();
        let file = work.path().join(name);
        fs::copy(fixture, &file).unwrap();
        let before = sha(&file);
        let out = write(&file, args);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {}", stderr(&out));
        assert_eq!(stdout(&out), "    1 image files updated\n", "{args:?}");
        assert_eq!(sha(&file), before, "{args:?}: nothing needed rewriting");
    }

    // One stored value beside one new value: a real byte change.
    let file = copy_into(&dir, JPEG, "mixed.jpg");
    let out = write(&file, &["-Make=Synthetic Camera Co", "-XPTitle=v"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:XPTitle"), "v");
    assert_eq!(read_back(&file, "IFD0:Make"), "Synthetic Camera Co");

    // Multi-file: ExifTool counts each file.
    let a = copy_into(&dir, JPEG, "m1.jpg");
    let b = copy_into(&dir, JPEG, "m2.jpg");
    let out = oxidex(&[
        "-Make=Synthetic Camera Co",
        a.to_str().unwrap(),
        b.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    2 image files updated\n");
}

/// The read-back is a proof, not a courtesy: a stored value that differs
/// from the request (only in case here) is a real rewrite, and a deletion
/// beside it that names something present is not "already satisfied".
#[test]
fn a_value_the_file_does_not_hold_is_never_proven() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let before = sha(&file);
    let out = write(&file, &["-Make=synthetic camera co"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "    1 image files updated\n");
    assert_ne!(sha(&file), before, "the case change must be written");
    assert_eq!(read_back(&file, "IFD0:Make"), "synthetic camera co");
}

/// Bare Windows XP tags go through the same resolution to IFD0 and are
/// encoded by #942's `xp_strings` exactly as the pinned oracle stores them.
/// ExifTool 13.59, `-XPComment='héllo wörld'` (and XPTitle/XPAuthor/
/// XPKeywords/XPSubject) on synthetic_001.jpg and sample.tif, `-v3`:
/// `Tag 0x9c9c (24 bytes, int8u[24] ...)` = UCS-2LE text plus a NUL pair,
/// read back as `[IFD0] XPComment : héllo wörld`.
#[test]
fn bare_xp_tags_write_ifd0_bytes_identical_to_the_oracle() {
    use oxidex::writers::exif_surgical::{IfdKind, scan_exif_entries};
    const BYTE: u16 = 1;
    let text = "héllo wörld";
    let mut expected: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    expected.extend_from_slice(&[0, 0]);
    for (fixture, name) in [(JPEG, "a.jpg"), (TIFF, "a.tif")] {
        for (tag, id) in [
            ("XPTitle", 0x9c9b_u16),
            ("XPComment", 0x9c9c),
            ("XPAuthor", 0x9c9d),
            ("XPKeywords", 0x9c9e),
            ("XPSubject", 0x9c9f),
        ] {
            let dir = TempDir::new().unwrap();
            let file = copy_into(&dir, fixture, name);
            let out = write(&file, &[&format!("-{tag}={text}")]);
            assert_eq!(out.status.code(), Some(0), "{name} {tag}: {}", stderr(&out));
            assert_eq!(stdout(&out), "    1 image files updated\n", "{name} {tag}");
            assert_eq!(
                read_back(&file, &format!("IFD0:{tag}")),
                text,
                "{name} {tag}"
            );

            let bytes = fs::read(&file).unwrap();
            let tiff = match bytes.windows(6).position(|w| w == b"Exif\0\0") {
                Some(at) if name.ends_with(".jpg") => &bytes[at + 6..],
                _ => &bytes[..],
            };
            let entry = scan_exif_entries(tiff)
                .unwrap()
                .entries
                .into_iter()
                .find(|e| e.ifd == IfdKind::Ifd0 && e.tag_id == id)
                .unwrap_or_else(|| panic!("{name} {tag}: no IFD0 0x{id:04x}"));
            assert_eq!(entry.field_type, BYTE, "{name} {tag}: int8u");
            assert_eq!(entry.count as usize, expected.len(), "{name} {tag}");
            assert_eq!(entry.value, expected, "{name} {tag}: UCS-2LE + NUL pair");

            // The same value again is ExifTool's `updated` without a rewrite.
            let before = sha(&file);
            let out = write(&file, &[&format!("-{tag}={text}")]);
            assert_eq!(stdout(&out), "    1 image files updated\n", "{name} {tag}");
            assert_eq!(sha(&file), before, "{name} {tag}");
            // A bare deletion removes it from the JPEG. The in-place TIFF
            // writer cannot shrink an IFD (pre-existing, grouped spellings
            // too): it must refuse loudly and leave the file alone.
            let out = write(&file, &[&format!("-{tag}=")]);
            if name.ends_with(".jpg") {
                assert_eq!(out.status.code(), Some(0), "{name} {tag}: {}", stderr(&out));
                assert_eq!(stdout(&out), "    1 image files updated\n", "{name} {tag}");
                assert_eq!(read_back(&file, &format!("IFD0:{tag}")), "", "{name} {tag}");
            } else {
                assert_eq!(out.status.code(), Some(1), "{name} {tag}");
                assert!(
                    !stdout(&out).contains("image files updated"),
                    "{name} {tag}"
                );
                assert_eq!(sha(&file), before, "{name} {tag}");
            }
        }
    }
}

/// The first numerator of the IFD1 entry `tag` in a PNG's `eXIf` chunk,
/// scanned from the bytes: oxidex's PNG reader does not surface IFD1 from
/// `eXIf` (a read-side gap), so a grouped read-back cannot see it there.
fn png_ifd1_numerator(path: &Path, tag: u16) -> Option<u32> {
    use oxidex::writers::exif_surgical::{IfdKind, scan_exif_entries};
    let png = fs::read(path).ok()?;
    let mut at = 8;
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().ok()?) as usize;
        if &png[at + 4..at + 8] == b"eXIf" {
            let data = &png[at + 8..at + 8 + len];
            let tiff = data.strip_prefix(b"Exif\0\0".as_slice()).unwrap_or(data);
            let big = tiff.starts_with(b"MM");
            let entry = scan_exif_entries(tiff)
                .ok()?
                .entries
                .into_iter()
                .find(|e| e.ifd == IfdKind::Ifd1 && e.tag_id == tag)?;
            let raw: [u8; 4] = entry.value.get(..4)?.try_into().ok()?;
            return Some(if big {
                u32::from_be_bytes(raw)
            } else {
                u32::from_le_bytes(raw)
            });
        }
        at += 12 + len;
    }
    None
}

/// Group and tag names are case-insensitive, as they are to ExifTool: one
/// canonical spelling reaches value typing, resolution and the transaction.
/// Pinned 13.59 on synthetic_001.jpg, in sequence, every step `1 image
/// files updated` but the last (`0 image files updated` / `1 image files
/// unchanged`, ISO already gone), with the read-backs below:
/// - integer: `-exififd:iso=200`, `-EXIFIFD:ISO=300`, `-iso=400`;
/// - rational: `-exififd:exposuretime=1/250`, `-EXPOSURETIME=1/30`,
///   `-ifd0:xresolution=300`, `-IFD0:XRESOLUTION=150`;
/// - date: `-exififd:datetimeoriginal=...`, `-DATETIMEORIGINAL=...`;
/// - string: `-ifd0:artist=me`, `-ARTIST=you`;
/// - deletions: `-ExifIFD:iso=`, then `-exififd:iso=`.
///
/// At 98288f02 `-exififd:ISO=200` was refused (`cannot write the exififd
/// group`); at 1fdfbeab `-exififd:iso=200`, the rational and the date
/// spellings were refused as type mismatches (`expected Integer but got
/// String`: the value was typed by the name as typed) and `-EXPOSURETIME=`
/// as a write ExifTool would also apply elsewhere.
#[test]
fn mixed_case_names_resolve_like_exiftool() {
    let dir = tempfile::TempDir::new().unwrap();
    let file = dir.path().join("c.jpg");
    std::fs::copy("tests/fixtures/jpeg/simple/synthetic_001.jpg", &file).unwrap();
    let run = |arg: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .arg(arg)
            .arg(&file)
            .output()
            .unwrap()
    };
    let read = |key: &str| {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["-s3", &format!("-{key}")])
            .arg(&file)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    let updated = "    1 image files updated\n";
    for (arg, stdout, key, value) in [
        ("-exififd:iso=200", updated, "ExifIFD:ISO", "200"),
        ("-EXIFIFD:ISO=300", updated, "ExifIFD:ISO", "300"),
        ("-iso=400", updated, "ExifIFD:ISO", "400"),
        (
            "-exififd:exposuretime=1/250",
            updated,
            "ExifIFD:ExposureTime",
            "1/250",
        ),
        (
            "-EXPOSURETIME=1/30",
            updated,
            "ExifIFD:ExposureTime",
            "1/30",
        ),
        ("-ifd0:xresolution=300", updated, "IFD0:XResolution", "300"),
        ("-IFD0:XRESOLUTION=150", updated, "IFD0:XResolution", "150"),
        (
            "-exififd:datetimeoriginal=2024:01:15 10:30:00",
            updated,
            "ExifIFD:DateTimeOriginal",
            "2024:01:15 10:30:00",
        ),
        (
            "-DATETIMEORIGINAL=2023:02:03 04:05:06",
            updated,
            "ExifIFD:DateTimeOriginal",
            "2023:02:03 04:05:06",
        ),
        ("-ifd0:artist=me", updated, "IFD0:Artist", "me"),
        ("-ARTIST=you", updated, "IFD0:Artist", "you"),
        ("-ExifIFD:iso=", updated, "ExifIFD:ISO", ""),
        (
            "-exififd:iso=",
            "    0 image files updated\n    1 image files unchanged\n",
            "ExifIFD:ISO",
            "",
        ),
    ] {
        let o = run(arg);
        assert_eq!(
            o.status.code(),
            Some(0),
            "{arg}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&o.stdout), stdout, "{arg}");
        assert_eq!(read(key), value, "{arg}: {key}");
    }
}
