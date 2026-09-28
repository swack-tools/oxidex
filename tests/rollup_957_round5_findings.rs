//! The fifth round of Codex review threads on the beta.1 roll-up (#957) at
//! 8275bb23, and the defects the request-order matrix
//! (`tests/request_order_matrix.rs`) found beside them, each pinned by a test
//! that fails there. Oracle rows are pinned ExifTool 13.59
//! (`perl5.38.2 -I<pinned>/lib <pinned>/exiftool`; probes `-ver` = 13.59,
//! `OOXML.docx` FileType = DOCX), re-measured through
//! `exiftool_oracle::graded()`.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{modify_tag, read_metadata, remove_tag};
use oxidex::core::tag_value::TagValue;
use oxidex::error::ExifToolError;
use oxidex::exiftool_oracle::{self, Oracle};
use oxidex::ffi::{
    EXIFTOOL_ERR_TAG_NOT_WRITTEN, EXIFTOOL_OK, exiftool_create, exiftool_destroy,
    exiftool_get_last_error_tag, exiftool_get_last_error_tag_count, exiftool_read_file,
    exiftool_remove_tag, exiftool_set_tag_string, exiftool_write_file,
};
use std::ffi::{CStr, CString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const JPEG_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";
const PNG: &str = "tests/fixtures/png/sample.png";
const PDF: &str = "tests/fixtures/pdf/sample.pdf";
const TIFF: &str = "tests/fixtures/tiff/sample.tif";

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

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// oxidex's `-s3 -KEY` read-back (`None` when absent).
fn get(path: &Path, key: &str) -> Option<String> {
    let o = oxidex(&["-s3", &format!("-{key}")], path);
    let value = out(&o).lines().next().unwrap_or_default().to_string();
    (!value.is_empty()).then_some(value)
}

fn exif_groups(path: &Path) -> Vec<String> {
    let mut groups: Vec<String> = read_metadata(path)
        .unwrap()
        .keys()
        .filter_map(|key| key.split_once(':').map(|(group, _)| group.to_string()))
        .filter(|group| ["IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD"].contains(&group.as_str()))
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

fn oracle_run(oracle: &Oracle, args: &[&str], path: &Path) -> Output {
    oracle.command().args(args).arg(path).output().unwrap()
}

/// The oracle's `-a -G1 -s` rows of `path`, without the rows describing the
/// file rather than its metadata, sorted (writers order PDF Info keys and
/// PNG chunks differently).
fn oracle_rows(oracle: &Oracle, path: &Path) -> Vec<String> {
    let o = oracle_run(oracle, &["-a", "-G1", "-s"], path);
    out(&o)
        .lines()
        .filter(|line| {
            !["[System]", "[File]", "[ExifTool]", "[Composite]"]
                .iter()
                .any(|group| line.starts_with(group))
        })
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

enum Call<'a> {
    Set(&'a str, &'a str),
    Remove(&'a str),
}

/// Reads `path` into a handle, makes `calls` in order, writes it back:
/// the write's code and the tags its refusal names.
fn ffi(path: &Path, calls: &[Call<'_>]) -> (i32, Vec<String>) {
    let c_path = CString::new(path.to_str().unwrap()).unwrap();
    let handle = exiftool_create();
    assert_eq!(exiftool_read_file(handle, c_path.as_ptr()), EXIFTOOL_OK);
    for call in calls {
        match call {
            Call::Set(tag, value) => {
                let (tag, value) = (CString::new(*tag).unwrap(), CString::new(*value).unwrap());
                assert_eq!(
                    exiftool_set_tag_string(handle, tag.as_ptr(), value.as_ptr()),
                    EXIFTOOL_OK
                );
            }
            Call::Remove(tag) => {
                let tag = CString::new(*tag).unwrap();
                assert_eq!(exiftool_remove_tag(handle, tag.as_ptr()), EXIFTOOL_OK);
            }
        }
    }
    let code = exiftool_write_file(handle, c_path.as_ptr());
    let named = (0..exiftool_get_last_error_tag_count())
        .map(|index| {
            // SAFETY: a NUL-terminated string the C ABI owns until the next
            // call on this thread.
            unsafe { CStr::from_ptr(exiftool_get_last_error_tag(index)) }
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    exiftool_destroy(handle);
    (code, named)
}

// --- PRRT_kwDOQNbr5M6mRRXx: a PDF Info date follows the call order -------

/// `set(PDF:CreateDate)` then `remove(PDF:CreationDate)` on one handle is
/// 13.59's `-PDF:CreateDate=<new> -PDF:CreateDate=`: the date is deleted.
/// The map still held the assigned `CreateDate`, and that set won.
#[test]
fn a_later_alias_removal_deletes_the_pdf_date() {
    let dir = TempDir::new().unwrap();
    for (alias, field) in [
        ("PDF:CreationDate", "PDF:CreateDate"),
        ("PDF:ModDate", "PDF:ModifyDate"),
    ] {
        let pdf = copy_into(&dir, Path::new(PDF), "later-removal.pdf");
        let (code, _) = ffi(
            &pdf,
            &[Call::Set(field, "2020:01:02 03:04:05"), Call::Remove(alias)],
        );
        assert_eq!(code, EXIFTOOL_OK, "{alias}");
        assert_eq!(get(&pdf, field), None, "{alias}: the set won");

        // The other order: the later set wins.
        let pdf = copy_into(&dir, Path::new(PDF), "later-set.pdf");
        let (code, _) = ffi(
            &pdf,
            &[Call::Remove(alias), Call::Set(field, "2020:01:02 03:04:05")],
        );
        assert_eq!(code, EXIFTOOL_OK, "{alias}");
        assert_eq!(
            get(&pdf, field).as_deref(),
            Some("2020:01:02 03:04:05"),
            "{alias}"
        );
    }
}

#[test]
fn oracle_orders_a_pdf_date_set_and_removal() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let dir = TempDir::new().unwrap();
    for (args, calls) in [
        (
            ["-PDF:CreateDate=2020:01:02 03:04:05", "-PDF:CreateDate="],
            [
                Call::Set("PDF:CreateDate", "2020:01:02 03:04:05"),
                Call::Remove("PDF:CreationDate"),
            ],
        ),
        (
            ["-PDF:CreateDate=", "-PDF:CreateDate=2020:01:02 03:04:05"],
            [
                Call::Remove("PDF:CreationDate"),
                Call::Set("PDF:CreateDate", "2020:01:02 03:04:05"),
            ],
        ),
    ] {
        let theirs = copy_into(&dir, Path::new(PDF), "theirs.pdf");
        assert!(oracle_run(oracle, &args, &theirs).status.success());
        let ours = copy_into(&dir, Path::new(PDF), "ours.pdf");
        assert_eq!(ffi(&ours, &calls).0, EXIFTOOL_OK);
        assert_eq!(
            oracle_rows(oracle, &ours),
            oracle_rows(oracle, &theirs),
            "{args:?}"
        );
    }
}

// --- PRRT_kwDOQNbr5M6mRRX8: a group deletion cancels what it covers -------

/// On a JPEG with EXIF, `-IFD1:Make=x -EXIF:All=` is 13.59's cleared EXIF;
/// oxidex kept the IFD1 set (which it cannot make) and refused the write.
/// The same for `-IFD0:All=`, which also removes every EXIF value set
/// before it, and on the C ABI.
#[test]
fn a_group_deletion_cancels_the_earlier_sets_it_covers() {
    let dir = TempDir::new().unwrap();
    for group in ["-EXIF:All=", "-IFD0:All=", "-IFD1:All="] {
        let file = copy_into(&dir, Path::new(JPEG), "cli.jpg");
        let o = oxidex(&["-IFD1:Make=x", group], &file);
        assert_eq!(o.status.code(), Some(0), "{group}: {}", err(&o));
        assert!(out(&o).contains("1 image files updated") || group == "-IFD1:All=");
    }
    let file = copy_into(&dir, Path::new(JPEG), "exif.jpg");
    assert_eq!(
        oxidex(&["-IFD1:Make=x", "-EXIF:All="], &file).status.code(),
        Some(0)
    );
    assert_eq!(exif_groups(&file), Vec::<String>::new());

    let file = copy_into(&dir, Path::new(JPEG), "ffi.jpg");
    let (code, named) = ffi(
        &file,
        &[Call::Set("IFD1:Make", "x"), Call::Remove("EXIF:All")],
    );
    assert_eq!((code, named), (EXIFTOOL_OK, vec![]));
    assert_eq!(exif_groups(&file), Vec::<String>::new());

    // Where oxidex cannot delete the group, only the deletion is refused.
    let file = copy_into(&dir, Path::new(JPEG_XMP), "xmp.jpg");
    let (code, named) = ffi(
        &file,
        &[Call::Set("XMP-dc:Title", "x"), Call::Remove("XMP:All")],
    );
    assert_eq!(
        (code, named),
        (EXIFTOOL_ERR_TAG_NOT_WRITTEN, vec!["XMP:All".to_string()])
    );
}

// --- PRRT_kwDOQNbr5M6mRRYE: an uncovered earlier set is still proven ------

/// 13.59: `-IFD0:Artist=<its value> -GPS:All=` on a JPEG without GPS is
/// `1 image files updated` (a same-value set); oxidex left the set before the
/// group deletion out of its proof and count and said `unchanged`.
#[test]
fn a_set_before_an_unrelated_group_deletion_is_counted() {
    let dir = TempDir::new().unwrap();
    for (fixture, set) in [
        (JPEG, "-IFD0:Artist=Synthetic Artist 1"),
        (PNG, "-IFD0:Artist=PNG Artist 1"),
        (TIFF, "-IFD0:Make=TestCamera"),
    ] {
        let ext = Path::new(fixture).extension().unwrap().to_str().unwrap();
        let file = copy_into(&dir, Path::new(fixture), &format!("a.{ext}"));
        let o = oxidex(&[set, "-GPS:All="], &file);
        assert_eq!(o.status.code(), Some(0), "{fixture}: {}", err(&o));
        assert_eq!(out(&o), "    1 image files updated\n", "{fixture}");
    }
}

#[test]
fn oracle_counts_a_same_value_set_before_a_no_op_deletion() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, Path::new(JPEG), "a.jpg");
    let o = oracle_run(
        oracle,
        &["-IFD0:Artist=Synthetic Artist 1", "-GPS:All="],
        &file,
    );
    assert_eq!(out(&o), "    1 image files updated\n");
}

// --- PRRT_kwDOQNbr5M6mRRYB: every per-tag writer refusal is typed ----------

/// A writer's refusal of one tag is `TagsNotWritten` naming it -- which the
/// C ABI reports as `EXIFTOOL_ERR_TAG_NOT_WRITTEN` with the tag list -- not
/// `UnsupportedFormat`: the RW2 guards (`IFD0:Artist` on a Panasonic RAW
/// whose JpgFromRaw would need the edit too), and the in-place TIFF writer's
/// entry and directory removals.
#[test]
fn writer_refusals_of_one_tag_are_typed() {
    let dir = TempDir::new().unwrap();
    let typed = |result: Result<_, ExifToolError>, tag: &str| match result {
        Err(ExifToolError::TagsNotWritten { tags }) => {
            assert_eq!(tags.len(), 1, "{tags:?}");
            assert_eq!(tags[0].tag, tag);
        }
        other => panic!("{tag}: {other:?}"),
    };
    let tiff = copy_into(&dir, Path::new(TIFF), "a.tif");
    typed(remove_tag(&tiff, "IFD0:Make"), "IFD0:Make");
    typed(remove_tag(&tiff, "EXIF:All"), "EXIF:All");
    let (code, named) = ffi(&tiff, &[Call::Remove("IFD0:Make")]);
    assert_eq!(
        (code, named),
        (EXIFTOOL_ERR_TAG_NOT_WRITTEN, vec!["IFD0:Make".to_string()])
    );

    if let Some(rw2) = fixtures::pinned_t_images_fixture_path("Panasonic.rw2") {
        let rw2 = copy_into(&dir, &rw2, "a.rw2");
        typed(
            modify_tag(&rw2, "IFD0:Artist", TagValue::new_string("x")),
            "IFD0:Artist",
        );
        let (code, named) = ffi(&rw2, &[Call::Set("IFD0:Artist", "x")]);
        assert_eq!(
            (code, named),
            (
                EXIFTOOL_ERR_TAG_NOT_WRITTEN,
                vec!["IFD0:Artist".to_string()]
            )
        );
    }
}

// --- the matrix: a removal the map cannot carry is still asked for --------

/// `remove(XMP-dc:Title)` on a handle read from a JPEG whose reader keys the
/// title `XMP:Title` changed nothing and returned success; 13.59's
/// `-XMP-dc:Title=` deletes it. It is now a deletion by name, which oxidex
/// (no XMP writer) refuses by name.
#[test]
fn a_removal_the_map_cannot_carry_is_a_named_deletion() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, Path::new(JPEG_XMP), "a.jpg");
    let before = fs::read(&file).unwrap();
    let (code, named) = ffi(&file, &[Call::Remove("XMP-dc:Title")]);
    assert_eq!(code, EXIFTOOL_ERR_TAG_NOT_WRITTEN);
    assert_eq!(named, ["XMP-dc:Title"]);
    assert_eq!(fs::read(&file).unwrap(), before);

    // A tag the file does not hold is a no-op, as 13.59's `-IFD0:Artist=`.
    let file = copy_into(&dir, Path::new(JPEG_XMP), "b.jpg");
    assert_eq!(ffi(&file, &[Call::Remove("IFD0:Artist")]).0, EXIFTOOL_OK);
    assert_eq!(fs::read(&file).unwrap(), before);
}

// --- the matrix: -all= deletes what 13.59's -all= deletes -----------------

/// JPEG segment names (APPn with its signature, COM) and whether anything
/// follows EOI.
fn jpeg_layout(path: &Path) -> (Vec<String>, bool) {
    let bytes = fs::read(path).unwrap();
    let mut names = Vec::new();
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let marker = bytes[at + 1];
        if marker == 0xDA {
            break;
        }
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        let payload = &bytes[at + 4..at + 2 + length];
        match marker {
            0xE0..=0xEF => names.push(format!(
                "APP{}:{}",
                marker - 0xE0,
                String::from_utf8_lossy(&payload[..payload.len().min(5)])
            )),
            0xFE => names.push("COM".into()),
            _ => {}
        }
        at += 2 + length;
    }
    let eoi = bytes
        .windows(2)
        .position(|pair| pair == [0xFF, 0xD9])
        .unwrap();
    (names, eoi + 2 < bytes.len())
}

/// PNG chunk types, and whether anything follows IEND.
fn png_layout(path: &Path) -> (Vec<String>, bool) {
    let bytes = fs::read(path).unwrap();
    let mut names = Vec::new();
    let mut at = 8;
    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = String::from_utf8_lossy(&bytes[at + 4..at + 8]).into_owned();
        at += 12 + length;
        let end = kind == "IEND";
        names.push(kind);
        if end {
            break;
        }
    }
    (names, at < bytes.len())
}

/// A PNG with the chunks whose fate under `-all=` the fixtures do not show:
/// gAMA and sRGB (writable PNG tags, deleted), cICP, sBIT, oFFs and vpAg
/// (kept), and trailing bytes (deleted).
fn crafted_png(dir: &TempDir) -> PathBuf {
    let source = fs::read("tests/fixtures/png/simple/synthetic_text_001.png").unwrap();
    let chunk = |kind: &[u8], data: &[u8]| {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc_input = kind.to_vec();
        crc_input.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        out
    };
    let ihdr_end = 8 + 12 + 13;
    let mut bytes = source[..ihdr_end].to_vec();
    for (kind, data) in [
        (&b"gAMA"[..], &45455u32.to_be_bytes()[..]),
        (b"sRGB", &[0u8][..]),
        (b"cICP", &[1u8, 13, 0, 1][..]),
        (b"sBIT", &[8u8, 8, 8][..]),
    ] {
        bytes.extend(chunk(kind, data));
    }
    bytes.extend_from_slice(&source[ihdr_end..]);
    bytes.extend_from_slice(b"TRAILER");
    let path = dir.path().join("crafted.png");
    fs::write(&path, bytes).unwrap();
    path
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// 13.59's `-all=` leaves a JPEG only an `Adobe` APP14 and nothing after
/// EOI, and a PNG only its image chunks (no text, eXIf, tIME, pHYs, iCCP,
/// gAMA or sRGB, no trailer). oxidex's `-all=` cleared the EXIF alone and
/// reported the file updated with its XMP, JFIF, ICC, comment, pHYs and tIME
/// still in it.
#[test]
fn oracle_all_deletes_what_exiftool_deletes() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no ExifTool oracle may grade output (pinned -ver + DOCX probe)");
        return;
    };
    let dir = TempDir::new().unwrap();
    let mut fixtures_to_clear: Vec<PathBuf> =
        [JPEG, JPEG_XMP, PNG].iter().map(PathBuf::from).collect();
    for name in [
        "ExifTool.jpg",
        "AFCP.jpg",
        "PhotoMechanic.jpg",
        "XMP.jpg",
        "IPTC.jpg",
        "ExtendedXMP.jpg",
        "PNG.png",
    ] {
        if let Some(path) = fixtures::pinned_t_images_fixture_path(name) {
            fixtures_to_clear.push(path);
        }
    }
    fixtures_to_clear.push(crafted_png(&dir));
    for fixture in fixtures_to_clear {
        let name = fixture.file_name().unwrap().to_string_lossy().into_owned();
        let theirs = copy_into(&dir, &fixture, &format!("theirs-{name}"));
        let o = oracle_run(oracle, &["-all="], &theirs);
        assert!(o.status.success(), "{name}: {}", err(&o));
        let ours = copy_into(&dir, &fixture, &format!("ours-{name}"));
        let o = oxidex(&["-all="], &ours);
        assert_eq!(o.status.code(), Some(0), "{name}: {}", err(&o));
        let layout = if name.ends_with(".png") {
            png_layout
        } else {
            jpeg_layout
        };
        assert_eq!(layout(&ours), layout(&theirs), "{name}: layout");
        assert_eq!(
            oracle_rows(oracle, &ours),
            oracle_rows(oracle, &theirs),
            "{name}: tags"
        );
    }
}
