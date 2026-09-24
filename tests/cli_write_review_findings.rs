//! Review findings on #945 ("never report a CLI write that did not happen").
//! Every test here failed at b8491b77 and pins the ExifTool 13.59 outcome
//! measured with the pinned oracle (perl 5.38.2, `-config ""`; probes
//! `-ver` = 13.59 and `OOXML.docx` FileType = DOCX) on the same fixtures.
//! Where ExifTool writes something oxidex cannot, oxidex must refuse with
//! exit 1 and leave the file byte-identical.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const JPEG_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";
const PNG_TEXT: &str = "tests/fixtures/png/simple/synthetic_text_001.png";
const PNG_EXIF: &str = "tests/fixtures/png/sample.png";
const PDF: &str = "tests/fixtures/pdf/sample.pdf";

fn oxidex(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

fn copy_into(dir: &TempDir, fixture: &str, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn sha(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).expect("read file")).to_vec()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// oxidex's grouped read-back of `key` (`-s3 -GROUP:TAG`).
fn read_back(path: &Path, key: &str) -> String {
    let o = oxidex(&["-s3", &format!("-{key}"), s(path)]);
    out(&o).lines().next().unwrap_or_default().to_string()
}

fn run(path: &Path, args: &[&str]) -> Output {
    let mut all = args.to_vec();
    all.push(s(path));
    oxidex(&all)
}

fn assert_refused_untouched(o: &Output, path: &Path, before: &[u8], label: &str) {
    assert_eq!(o.status.code(), Some(1), "{label}: {}", out(o));
    assert!(
        !out(o).contains("image files updated"),
        "{label}: {}",
        out(o)
    );
    assert!(err(o).contains("Error"), "{label}: {}", err(o));
    assert_eq!(sha(path), before, "{label}: file changed");
}

/// A source with `[IFD0] XPTitle = hello`, written by oxidex (bytes pinned
/// equal to the oracle's by cli_write_request_resolution).
fn xp_source(dir: &TempDir) -> PathBuf {
    let src = copy_into(dir, JPEG, "src.jpg");
    assert_eq!(run(&src, &["-XPTitle=hello"]).status.code(), Some(0));
    src
}

// --- P1-1: -TagsFromFile goes through the write resolution gate ---------

/// ExifTool 13.59, `-TagsFromFile sample_with_exif_xmp.jpg -XMP:Title
/// -IFD0:Make synthetic_001.jpg`: `[XMP-dc] Title : Sample Photo`, `[IFD0]
/// Make : TestCamera`, `1 image files updated`. oxidex cannot write XMP;
/// it printed `1 image files updated (2 tags copied)` with the title
/// dropped. Now the copy is refused whole.
#[test]
fn tags_from_file_refuses_a_tag_it_cannot_write() {
    let dir = TempDir::new().unwrap();
    let dst = copy_into(&dir, JPEG, "dst.jpg");
    let before = sha(&dst);
    let o = run(
        &dst,
        &["-TagsFromFile", JPEG_XMP, "-XMP:Title", "-IFD0:Make"],
    );
    assert_refused_untouched(&o, &dst, &before, "-XMP:Title copy");
    assert!(err(&o).contains("XMP:Title"), "{}", err(&o));
}

/// ExifTool 13.59 copies a bare `-XPTitle`, `-EXIF:XPTitle`, and a
/// redirected `-XPTitle>XPComment` (`[IFD0] XPTitle : hello` / `[IFD0]
/// XPComment : hello`, `1 image files updated`); oxidex copied nothing.
#[test]
fn tags_from_file_copies_bare_family_and_redirected_names() {
    let dir = TempDir::new().unwrap();
    let src = xp_source(&dir);
    for (args, key) in [
        (vec!["-XPTitle"], "IFD0:XPTitle"),
        (vec!["-EXIF:XPTitle"], "IFD0:XPTitle"),
        (vec!["-XPTitle>XPComment"], "IFD0:XPComment"),
        (vec!["-XPComment<XPTitle"], "IFD0:XPComment"),
    ] {
        let dst = copy_into(&dir, JPEG, "dst.jpg");
        let mut all = vec!["-TagsFromFile", s(&src)];
        all.extend(args.iter().copied());
        let o = run(&dst, &all);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", err(&o));
        assert_eq!(out(&o), "    1 image files updated\n", "{args:?}");
        assert_eq!(read_back(&dst, key), "hello", "{args:?}");
    }
}

/// ExifTool 13.59, a tag the source lacks: `Warning: No writable tags set
/// from src.jpg`, `0 image files updated` / `1 image files unchanged`.
#[test]
fn tags_from_file_with_nothing_to_copy_warns_and_is_unchanged() {
    let dir = TempDir::new().unwrap();
    let src = xp_source(&dir);
    let dst = copy_into(&dir, JPEG, "dst.jpg");
    let before = sha(&dst);
    let o = run(&dst, &["-TagsFromFile", s(&src), "-XPSubject"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o),
        "    0 image files updated\n    1 image files unchanged\n"
    );
    assert!(err(&o).contains("No writable tags set from"), "{}", err(&o));
    assert_eq!(sha(&dst), before);
}

// --- P1-2: combined modes are applied together or refused --------------

/// ExifTool 13.59: `-all= -XPTitle=x` clears and then sets (`[IFD0]
/// XPTitle : x`, Make gone); `-DateTimeOriginal+=1:0:0 -XPTitle=x` shifts
/// (12:00:00 -> 13:00:00) and sets. oxidex applied only the first mode.
#[test]
fn combined_clear_shift_and_set_apply_every_request() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let o = run(&file, &["-all=", "-XPTitle=x"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "    1 image files updated\n");
    assert_eq!(read_back(&file, "IFD0:XPTitle"), "x");
    assert_eq!(read_back(&file, "IFD0:Make"), "");

    let file = copy_into(&dir, JPEG, "b.jpg");
    let o = run(&file, &["-DateTimeOriginal+=1:0:0", "-XPTitle=x"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "    1 image files updated\n");
    assert_eq!(
        read_back(&file, "ExifIFD:DateTimeOriginal"),
        "2024:01:01 13:00:00"
    );
    assert_eq!(read_back(&file, "IFD0:XPTitle"), "x");
}

/// Combinations oxidex cannot apply faithfully are refused before any
/// file is touched, never half-applied.
#[test]
fn unsupported_mode_combinations_are_refused() {
    let dir = TempDir::new().unwrap();
    let src = xp_source(&dir);
    for args in [
        vec!["-all=", "-DateTimeOriginal+=1:0:0"],
        vec!["-TagsFromFile", s(&src), "-XPTitle", "-Artist=x"],
        vec![
            "-DateTimeOriginal+=1:0:0",
            "-DateTimeOriginal=2020:01:01 00:00:00",
        ],
    ] {
        let file = copy_into(&dir, JPEG, "a.jpg");
        let before = sha(&file);
        let o = run(&file, &args);
        assert_refused_untouched(&o, &file, &before, &format!("{args:?}"));
    }
}

// --- P2-4: hand-kept bare names under the same checks ------------------

/// A JPEG carrying `[XMP-tiff] Software : OLD`: ExifTool 13.59's
/// `-Software=NEW` writes `[IFD0]` *and* `[XMP-tiff]` Software. oxidex wrote
/// IFD0 only and reported an update; it must refuse, naming the typed key.
#[test]
fn bare_software_is_refused_when_exiftool_would_also_update_xmp() {
    let xmp = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Software="OLD"/></rdf:RDF></x:xmpmeta>"#;
    let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    payload.extend_from_slice(xmp);
    let base = fs::read(JPEG).unwrap();
    let mut bytes = base[..2].to_vec();
    bytes.extend([0xff, 0xe1]);
    bytes.extend(((payload.len() + 2) as u16).to_be_bytes());
    bytes.extend(&payload);
    bytes.extend(&base[2..]);
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("xsw.jpg");
    fs::write(&file, &bytes).unwrap();
    assert_eq!(read_back(&file, "XMP-tiff:Software"), "OLD");
    let before = sha(&file);
    let o = run(&file, &["-Software=NEW"]);
    assert_refused_untouched(&o, &file, &before, "-Software=NEW");
    assert!(err(&o).contains("'Software'"), "{}", err(&o));
    assert!(err(&o).contains("XMP-tiff:Software"), "{}", err(&o));
}

/// ExifTool 13.59 on a PNG: bare `-Software=NEW` writes `[PNG] Software`;
/// oxidex wrote `[IFD0] Software`. Bare names are refused in PNG.
#[test]
fn bare_names_are_refused_in_png() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, PNG_TEXT, "a.png");
    let before = sha(&file);
    let o = run(&file, &["-Software=NEW"]);
    assert_refused_untouched(&o, &file, &before, "PNG -Software");
    assert!(err(&o).contains("'Software'"), "{}", err(&o));
}

// --- P2-5: EXIF: resolves to the tag's own directory -------------------

/// ExifTool 13.59 on synthetic_001.jpg: `-EXIF:ISO=100` writes `[ExifIFD]
/// ISO` (so do UserComment, LensModel, SerialNumber, OwnerName). oxidex
/// created `[IFD0] ISO`.
#[test]
fn exif_family_writes_land_in_the_tags_own_directory() {
    for tag in [
        "ISO",
        "UserComment",
        "LensModel",
        "SerialNumber",
        "OwnerName",
    ] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, JPEG, "a.jpg");
        let o = run(&file, &[&format!("-EXIF:{tag}=100")]);
        assert_eq!(o.status.code(), Some(0), "{tag}: {}", err(&o));
        assert_eq!(read_back(&file, &format!("ExifIFD:{tag}")), "100", "{tag}");
        assert_eq!(read_back(&file, &format!("IFD0:{tag}")), "", "{tag}");
    }
}

// --- P2-6: absent deletions in PNG and PDF -----------------------------

/// ExifTool 13.59: deleting a tag the file lacks is `0 image files
/// updated` / `1 image files unchanged`, bytes untouched -- `-PDF:Author=`
/// on a PDF whose Author is already gone, `-PNG:Copyright=` and
/// `-IFD0:Artist=` on synthetic_text_001.png. oxidex appended an empty PDF
/// revision / re-laid-out the PNG and reported an update.
#[test]
fn absent_deletions_in_png_and_pdf_are_unchanged() {
    let dir = TempDir::new().unwrap();
    let pdf = copy_into(&dir, PDF, "a.pdf");
    let o = run(&pdf, &["-PDF:Author="]);
    assert_eq!(out(&o), "    1 image files updated\n", "{}", err(&o));
    for (file, arg) in [
        (pdf.clone(), "-PDF:Author="),
        (copy_into(&dir, PNG_TEXT, "a.png"), "-PNG:Copyright="),
        (copy_into(&dir, PNG_TEXT, "b.png"), "-IFD0:Artist="),
    ] {
        let before = sha(&file);
        let o = run(&file, &[arg]);
        assert_eq!(o.status.code(), Some(0), "{arg}: {}", err(&o));
        assert_eq!(
            out(&o),
            "    0 image files updated\n    1 image files unchanged\n",
            "{arg}"
        );
        assert_eq!(sha(&file), before, "{arg}");
    }
}

// --- P1-3 (pre-#943 PNG writer): no misplaced PNG EXIF ------------------

/// ExifTool 13.59 on tests/fixtures/png/sample.png: `-IFD1:XResolution=10`
/// writes `[IFD1]`, `-ExifIFD:ISO=200` writes `[ExifIFD]`. The PNG writer
/// flattens everything into IFD0 (a second IFD0 XResolution; ISO in IFD0),
/// and even `-PNG:Title=t` moved sample.png's ExifIFD entries into IFD0. All
/// are refused until the in-place PNG writer lands.
#[test]
fn png_writes_that_would_misplace_exif_are_refused() {
    for args in [
        vec!["-IFD1:XResolution=10"],
        vec!["-ExifIFD:ISO=200"],
        vec!["-PNG:Title=t"],
    ] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, PNG_EXIF, "a.png");
        let before = sha(&file);
        let o = run(&file, &args);
        assert_refused_untouched(&o, &file, &before, &format!("{args:?}"));
    }
}

// --- P2-7 / P2-8: every file, and --readonly ---------------------------

/// ExifTool 13.59, `-all= c1.jpg c2.jpg`: `2 image files updated`. oxidex
/// cleared c2 only and printed `1 image files updated`; the same for date
/// shifts and -TagsFromFile.
#[test]
fn multi_file_clear_shift_and_copy_process_every_file() {
    let dir = TempDir::new().unwrap();
    let src = xp_source(&dir);
    for args in [
        vec!["-all="],
        vec!["-DateTimeOriginal+=1:0:0"],
        vec!["-TagsFromFile", s(&src), "-XPTitle"],
    ] {
        let c1 = copy_into(&dir, JPEG, "c1.jpg");
        let c2 = copy_into(&dir, JPEG, "c2.jpg");
        let (h1, h2) = (sha(&c1), sha(&c2));
        let mut all = args.clone();
        all.extend([s(&c1), s(&c2)]);
        let o = oxidex(&all);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", err(&o));
        assert_eq!(out(&o), "    2 image files updated\n", "{args:?}");
        assert_ne!(sha(&c1), h1, "{args:?}: c1");
        assert_ne!(sha(&c2), h2, "{args:?}: c2");
    }
}

#[test]
fn readonly_refuses_writes_to_a_file_list() {
    let dir = TempDir::new().unwrap();
    let r1 = copy_into(&dir, JPEG, "r1.jpg");
    let r2 = copy_into(&dir, JPEG, "r2.jpg");
    let (h1, h2) = (sha(&r1), sha(&r2));
    let o = oxidex(&["--readonly", "-Artist=x", s(&r1), s(&r2)]);
    assert_eq!(o.status.code(), Some(1), "{}", out(&o));
    assert!(err(&o).contains("read-only"), "{}", err(&o));
    assert_eq!((sha(&r1), sha(&r2)), (h1, h2));
}

/// One refused file in a list: ExifTool's summary shape (`2 image files
/// updated` / `1 files weren't updated due to errors`), exit 1, and the
/// refused file untouched.
#[test]
fn a_refused_file_in_a_list_is_counted_as_an_error() {
    let dir = TempDir::new().unwrap();
    let c1 = copy_into(&dir, JPEG, "c1.jpg");
    let pdf = copy_into(&dir, PDF, "p.pdf");
    let c2 = copy_into(&dir, JPEG, "c2.jpg");
    let hp = sha(&pdf);
    let o = oxidex(&["-XPTitle=v", s(&c1), s(&pdf), s(&c2)]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        out(&o),
        "    2 image files updated\n    1 files weren't updated due to errors\n"
    );
    assert_eq!(sha(&pdf), hp);
    assert_eq!(read_back(&c1, "IFD0:XPTitle"), "v");
    assert_eq!(read_back(&c2, "IFD0:XPTitle"), "v");
}

/// Batch `--backup` names the copy `<file>.bak` like the single-file path;
/// an extension-less file got `a..bak`.
#[test]
fn batch_backup_appends_bak() {
    let dir = TempDir::new().unwrap();
    let a = copy_into(&dir, JPEG, "a");
    let b = copy_into(&dir, JPEG, "b.jpg");
    let (ha, hb) = (sha(&a), sha(&b));
    let o = oxidex(&["--backup", "-XPTitle=v", s(&a), s(&b)]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(sha(&dir.path().join("a.bak")), ha);
    assert_eq!(sha(&dir.path().join("b.jpg.bak")), hb);
    assert!(!dir.path().join("a..bak").exists());
}

// --- P3 -----------------------------------------------------------------

/// A stored `"Canon   "` reads as `Canon` (ExifTool's RawConv trims it), but
/// ExifTool 13.59's `-Make=Canon` rewrites it to `Canon` (6 bytes). oxidex
/// left the padding and reported an update from the normalized read-back.
/// It must not claim an update it did not make -- and, since the write
/// transaction proves every set by reading its field bytes back, it must not
/// claim success for a set it left undone either: it either rewrites the
/// entry or refuses by name with the file untouched.
#[test]
fn a_normalized_read_back_is_no_proof() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    assert_eq!(run(&file, &["-Make=Canon   "]).status.code(), Some(0));
    let before = sha(&file);
    let o = run(&file, &["-Make=Canon"]);
    if o.status.code() == Some(0) {
        assert_ne!(sha(&file), before, "success without the rewrite");
        assert_eq!(out(&o), "    1 image files updated\n");
    } else {
        assert_eq!(o.status.code(), Some(1), "{}", err(&o));
        assert!(err(&o).contains("'Make'"), "{}", err(&o));
        assert!(!out(&o).contains("1 image files updated"), "{}", out(&o));
        assert_eq!(sha(&file), before, "a refused write changed the file");
    }
}

/// `-PDF:Trapped=True` is refused for the field, not the group.
#[test]
fn pdf_refusal_names_the_field() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, PDF, "a.pdf");
    let before = sha(&file);
    let o = run(&file, &["-PDF:Trapped=True"]);
    assert_refused_untouched(&o, &file, &before, "Trapped");
    assert!(err(&o).contains("Info dictionary"), "{}", err(&o));
    assert!(
        !err(&o).contains("cannot write the PDF group"),
        "{}",
        err(&o)
    );
}

// --- Codex threads on a59758a0 -------------------------------------------

/// Thread 1. The reader surfaces a PDF's creation date as both
/// `PDF:CreateDate` and `PDF:CreationDate`; removing one spelling left the
/// other in the map and the writer put the date back. Pinned ExifTool 13.59
/// on sample.pdf: `-PDF:CreateDate=` removes it (`[PDF] ModifyDate` alone
/// remains), `-PDF:CreateDate='2020:01:02 03:04:05'` reads back as written,
/// and the Info-key spellings are not tags: `-PDF:CreationDate=` / `-PDF:ModDate=`
/// warn `Sorry, PDF:ModDate doesn't exist or isn't writable` / `Nothing to
/// do.` and exit 1.
#[test]
fn pdf_date_writes_act_on_every_spelling_of_the_field() {
    let dir = TempDir::new().unwrap();
    let pdf = copy_into(&dir, PDF, "a.pdf");
    let o = run(&pdf, &["-PDF:CreateDate="]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "    1 image files updated\n");
    assert_eq!(read_back(&pdf, "PDF:CreateDate"), "");
    assert_eq!(
        read_back(&pdf, "PDF:ModifyDate"),
        "2024:01:15 15:00:00+00:00"
    );

    let pdf = copy_into(&dir, PDF, "b.pdf");
    let o = run(&pdf, &["-PDF:ModifyDate="]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(read_back(&pdf, "PDF:ModifyDate"), "");
    assert_eq!(
        read_back(&pdf, "PDF:CreateDate"),
        "2024:01:15 14:30:00+00:00"
    );

    let pdf = copy_into(&dir, PDF, "c.pdf");
    let o = run(&pdf, &["-PDF:CreateDate=2020:01:02 03:04:05"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(read_back(&pdf, "PDF:CreateDate"), "2020:01:02 03:04:05");
    // With a zone, and through ExifTool's other spelling: pinned 13.59 writes
    // `(D:20200102030405+02'00')` and reads back `...+02:00`.
    let o = run(&pdf, &["-PDF:ModifyDate=2020:01:02 03:04:05+02:00"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        read_back(&pdf, "PDF:ModifyDate"),
        "2020:01:02 03:04:05+02:00"
    );

    for spelling in ["PDF:CreationDate", "PDF:ModDate"] {
        let pdf = copy_into(&dir, PDF, "d.pdf");
        let before = sha(&pdf);
        let o = run(&pdf, &[&format!("-{spelling}=")]);
        assert_eq!(o.status.code(), Some(1), "{spelling}");
        assert_eq!(out(&o), "", "{spelling}");
        assert_eq!(
            err(&o),
            format!("Warning: Sorry, {spelling} doesn't exist or isn't writable\nNothing to do.\n")
        );
        assert_eq!(sha(&pdf), before, "{spelling}");
    }
}

/// Thread 2. The public `copy_metadata` wrote each filtered tag as it went,
/// so a later refusal left the earlier ones committed.
#[test]
fn a_refused_filter_leaves_the_copy_destination_untouched() {
    let dir = TempDir::new().unwrap();
    let src = copy_into(&dir, JPEG_XMP, "src.jpg");
    assert_eq!(run(&src, &["-XPTitle=hello"]).status.code(), Some(0));
    let dst = copy_into(&dir, JPEG, "dst.jpg");
    let before = sha(&dst);
    let filters = ["XPTitle".to_string(), "XMP:Title".to_string()];
    let result = oxidex::core::operations::copy_metadata(&src, &dst, Some(&filters));
    assert!(result.is_err(), "XMP:Title cannot be written");
    assert_eq!(sha(&dst), before, "the XPTitle before it was committed");
}

/// A JPEG whose GPS IFD holds GPSAltitude (100/1) and GPSSpeed (5/1) and no
/// GPSVersionID, built on tag_matrix_base.jpg with its EXIF segment removed.
fn gps_jpeg(dir: &TempDir) -> PathBuf {
    let entry = |tag: u16, typ: u16, count: u32, value: u32| {
        let mut e = tag.to_le_bytes().to_vec();
        e.extend(typ.to_le_bytes());
        e.extend(count.to_le_bytes());
        e.extend(value.to_le_bytes());
        e
    };
    let mut tiff = b"II*\0".to_vec();
    tiff.extend(8u32.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(entry(0x8825, 4, 1, 26));
    tiff.extend(0u32.to_le_bytes());
    let data = 26 + 2 + 2 * 12 + 4;
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(entry(0x0006, 5, 1, data));
    tiff.extend(entry(0x000d, 5, 1, data + 8));
    tiff.extend(0u32.to_le_bytes());
    for (n, d) in [(100u32, 1u32), (5, 1)] {
        tiff.extend(n.to_le_bytes());
        tiff.extend(d.to_le_bytes());
    }
    let base = fs::read("tests/fixtures/jpeg/tag_matrix_base.jpg").unwrap();
    let mut kept = base[..2].to_vec();
    let mut at = 2;
    while at + 4 <= base.len() && base[at] == 0xff && !matches!(base[at + 1], 0xda | 0xd9) {
        let len = u16::from_be_bytes([base[at + 2], base[at + 3]]) as usize;
        let segment = &base[at..at + 2 + len];
        if !(base[at + 1] == 0xe1 && segment[4..].starts_with(b"Exif\0\0")) {
            kept.extend_from_slice(segment);
        }
        at += 2 + len;
    }
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend(&tiff);
    let mut out = kept[..2].to_vec();
    out.extend([0xff, 0xe1]);
    out.extend(((app1.len() + 2) as u16).to_be_bytes());
    out.extend(&app1);
    out.extend(&kept[2..]);
    out.extend(&base[at..]);
    let path = dir.path().join("gps.jpg");
    fs::write(&path, out).unwrap();
    path
}

/// Thread 3. Pinned ExifTool 13.59 on that file: `-GPS:GPSAltitude=` leaves
/// `[GPS] GPSSpeed : 5` and reports an update. The planner dropped
/// GPSAltitude and added the GPSVersionID default, the entry count matched
/// the original, and the identity test handed back the original bytes:
/// `0 image files updated` / `1 image files unchanged`, altitude still there.
#[test]
fn a_deletion_offset_by_a_generated_default_is_not_an_identity() {
    let dir = TempDir::new().unwrap();
    let file = gps_jpeg(&dir);
    assert_eq!(read_back(&file, "GPS:GPSAltitude"), "100 m");
    let o = run(&file, &["-GPS:GPSAltitude="]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "    1 image files updated\n");
    assert_eq!(read_back(&file, "GPS:GPSAltitude"), "");
    assert_eq!(read_back(&file, "GPS:GPSSpeed"), "5");
}

/// Thread 4. A copy-all with nothing the destination can hold (every tag of
/// synthetic_001.jpg is EXIF/JFIF/File; the PDF writer takes only Info
/// fields) serialized the PDF anyway, appending a revision reported as an
/// update. Nothing copied is nothing written.
#[test]
fn a_copy_all_with_nothing_copied_leaves_the_file_alone() {
    let dir = TempDir::new().unwrap();
    let pdf = copy_into(&dir, PDF, "a.pdf");
    let before = sha(&pdf);
    let o = run(&pdf, &["-TagsFromFile", JPEG]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o),
        "    0 image files updated\n    1 image files unchanged\n"
    );
    assert!(err(&o).contains("Not copied"), "{}", err(&o));
    assert_eq!(sha(&pdf), before);
}

/// Thread 5. Pinned ExifTool 13.59: `-TagsFromFile src -XPTitle -NoSuchTag=x
/// dst` prints `Warning: Tag 'NoSuchTag' is not defined` and copies
/// (`1 image files updated`); the undefined name was counted as a set and
/// the copy refused.
#[test]
fn an_undefined_set_beside_a_copy_is_only_a_warning() {
    let dir = TempDir::new().unwrap();
    let src = xp_source(&dir);
    for filters in [vec!["-XPTitle"], vec![]] {
        let dst = copy_into(&dir, JPEG, "dst.jpg");
        let mut args = vec!["-TagsFromFile", s(&src)];
        args.extend(filters.iter().copied());
        args.push("-NoSuchTag=x");
        let o = run(&dst, &args);
        assert_eq!(o.status.code(), Some(0), "{filters:?}: {}", err(&o));
        assert!(
            err(&o).starts_with("Warning: Tag 'NoSuchTag' is not defined\n"),
            "{filters:?}: {}",
            err(&o)
        );
        assert_eq!(out(&o), "    1 image files updated\n", "{filters:?}");
        assert_eq!(read_back(&dst, "IFD0:XPTitle"), "hello", "{filters:?}");
    }
}
