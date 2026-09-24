//! The library's write APIs either apply every requested change or refuse the
//! whole write, naming each key they would not write.
//!
//! #945 made the CLI resolve every `-TAG=VALUE` request and refuse any key a
//! writer would drop. The public library entry points did not go through that
//! resolution: `write_metadata` handed the caller's map straight to the
//! format writer, which only visits the groups it writes and skips the rest,
//! and returned `Ok(())`. So an ungrouped `XPTitle`, an `XMP:Title` in a
//! JPEG, TIFF or PNG, an `IFD1:` or a `File:` key was dropped while success
//! was reported -- through `write_metadata`, `Metadata::save`/`write_to`,
//! `CopyBuilder::execute`, `copy_metadata` and `shift_metadata_dates` alike.
//!
//! What pinned ExifTool 13.59 does with the same requests on the same
//! fixtures (`perl5.38.2 -I<pinned>/lib <pinned>/exiftool -config ""`,
//! probes `-ver` = 13.59 and `OOXML.docx` FileType = DOCX; evidence
//! `library-ffi-writes/oracle-writes.txt`):
//!
//! | request (fixture)                        | ExifTool 13.59                     | oxidex             |
//! |------------------------------------------|------------------------------------|--------------------|
//! | `XPTitle=v` (synthetic_001.jpg)          | `[IFD0] XPTitle: v`                | writes IFD0        |
//! | `Title=v` (synthetic_001.jpg)            | `[XMP-dc] Title: v`                | refused (no XMP writer) |
//! | `XMP:Title=v` (JPEG, TIFF, PNG)          | `[XMP-dc] Title: v`                | refused            |
//! | `File:Comment=c` (synthetic_001.jpg)     | `[File] Comment: c` (COM segment)  | refused            |
//! | `IFD1:ImageDescription=x` (synthetic_001)| `[IFD1] ImageDescription: x`       | refused            |
//! | `IFD1:XResolution=300` (sample.png)      | `[IFD1] XResolution: 300`          | writes IFD1 (#943) |
//!
//! Every refusal leaves the file byte-identical, and a refusal of one key
//! refuses the whole request (ExifTool writes all of a file's tags or none).

use oxidex::core::date_shift::{ShiftOperation, shift_metadata_dates};
use oxidex::core::operations::{
    copy_metadata, modify_tag, read_metadata, remove_tag, write_metadata,
};
use oxidex::core::{Metadata, MetadataMap, TagValue};
use oxidex::error::ExifToolError;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const JPEG_XMP: &str = "tests/fixtures/jpeg/sample_with_exif_xmp.jpg";
const TIFF: &str = "tests/fixtures/tiff/sample.tif";
const PNG: &str = "tests/fixtures/png/sample.png";
const PDF: &str = "tests/fixtures/pdf/sample.pdf";

fn copy_into(dir: &TempDir, fixture: &str) -> PathBuf {
    let name = Path::new(fixture).file_name().unwrap();
    let path = dir.path().join(name);
    fs::copy(fixture, &path).expect("copy fixture");
    path
}

fn sha(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).expect("read file")).to_vec()
}

/// Reads `path`, applies `edit` to its map and writes it back with
/// `write_metadata`: the read-modify-write the API documents.
fn rmw(path: &Path, edit: impl FnOnce(&mut MetadataMap)) -> Result<(), ExifToolError> {
    let mut metadata = read_metadata(path).expect("read fixture");
    edit(&mut metadata);
    write_metadata(path, &metadata)
}

fn set(map: &mut MetadataMap, key: &str, value: &str) {
    map.insert(key, TagValue::new_string(value));
}

/// Asserts `result` is a refusal naming every key in `keys` (and no key in
/// `not_named`), and that `path` still has the bytes `before`.
fn assert_refused(
    what: &str,
    result: Result<(), ExifToolError>,
    keys: &[&str],
    not_named: &[&str],
    path: &Path,
    before: &[u8],
) {
    let err = match result {
        Ok(()) => panic!("{what}: reported success for a write it did not make"),
        Err(err) => err,
    };
    let text = err.to_string();
    for key in keys {
        assert!(
            text.contains(key),
            "{what}: error does not name {key}: {text}"
        );
    }
    for key in not_named {
        assert!(
            !text.contains(&format!("'{key}'")),
            "{what}: error names the writable {key}: {text}"
        );
    }
    // Typed: every refused key, spelled as the request spelled it.
    assert!(
        matches!(err, ExifToolError::TagsNotWritten { .. }),
        "{what}: expected TagsNotWritten, got {err:?}"
    );
    let named: Vec<&str> = err
        .tags_not_written()
        .iter()
        .map(|tag| tag.tag.as_str())
        .collect();
    for key in keys {
        assert!(named.contains(key), "{what}: {key} not in {named:?}");
    }
    for key in not_named {
        assert!(!named.contains(key), "{what}: writable {key} in {named:?}");
    }
    assert_eq!(
        sha(path),
        before,
        "{what}: a refused write changed the file"
    );
}

fn read_string(path: &Path, key: &str) -> Option<String> {
    read_metadata(path)
        .unwrap()
        .get(key)
        .and_then(|value| value.as_string().map(str::to_string))
}

/// An ungrouped name goes through the same resolution as the CLI's
/// `-XPTitle=v`: pinned 13.59 writes `[IFD0] XPTitle`.
#[test]
fn write_metadata_resolves_an_ungrouped_name_like_the_cli() {
    for fixture in [JPEG, TIFF] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture);
        rmw(&file, |map| set(map, "XPTitle", "v")).unwrap();
        assert_eq!(
            read_string(&file, "IFD0:XPTitle").as_deref(),
            Some("v"),
            "{fixture}: an ungrouped XPTitle must land in IFD0"
        );
    }
}

/// An ungrouped name ExifTool writes to a group oxidex cannot (`Title` is
/// XMP-dc in a JPEG, 13.59) is refused by name.
#[test]
fn write_metadata_refuses_an_ungrouped_name_it_cannot_resolve() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG);
    let before = sha(&file);
    let result = rmw(&file, |map| set(map, "Title", "v"));
    assert_refused("JPEG Title", result, &["Title"], &[], &file, &before);
}

/// XMP in a JPEG, TIFF or PNG: ExifTool 13.59 writes `[XMP-dc] Title`;
/// oxidex has no XMP writer there and must say so.
#[test]
fn write_metadata_refuses_xmp_where_the_writer_cannot_write_it() {
    for (fixture, key) in [
        (JPEG, "XMP:Title"),
        (JPEG_XMP, "XMP-dc:Title"),
        (TIFF, "XMP:Title"),
        (PNG, "XMP:Title"),
        (PDF, "XMP:Title"),
    ] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture);
        let before = sha(&file);
        let result = rmw(&file, |map| set(map, key, "v"));
        assert_refused(
            &format!("{fixture} {key}"),
            result,
            &[key],
            &[],
            &file,
            &before,
        );
    }
}

/// Deleting a row by dropping it from the map is a request too: an XMP row
/// the JPEG writer cannot remove must not be reported removed.
#[test]
fn write_metadata_refuses_a_deletion_the_writer_cannot_make() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG_XMP);
    let before = sha(&file);
    // The reader keys XMP rows by family 0 (`-G1` prints `[XMP-dc] Title`).
    let key = "XMP:Title";
    assert!(
        read_metadata(&file).unwrap().contains_key(key),
        "fixture carries {key}"
    );
    let result = rmw(&file, |map| {
        map.remove(key);
    });
    assert_refused(
        "JPEG_XMP delete XMP:Title",
        result,
        &[key],
        &[],
        &file,
        &before,
    );
}

/// `File:Comment` is a JPEG COM segment to ExifTool 13.59; oxidex's JPEG
/// writer does not write it.
#[test]
fn write_metadata_refuses_file_group_keys() {
    for fixture in [JPEG, TIFF, PNG] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture);
        let before = sha(&file);
        let result = rmw(&file, |map| set(map, "File:Comment", "c"));
        assert_refused(
            &format!("{fixture} File:Comment"),
            result,
            &["File:Comment"],
            &[],
            &file,
            &before,
        );
    }
}

/// IFD1 keys the format's writer does not address. ExifTool 13.59 writes
/// `[IFD1] ImageDescription` in synthetic_001.jpg; oxidex's JPEG writer does
/// not, so it must refuse by name.
#[test]
fn write_metadata_refuses_ifd1_keys_the_writer_would_drop() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG);
    let before = sha(&file);
    let key = "IFD1:ImageDescription";
    let result = rmw(&file, |map| set(map, key, "x"));
    assert_refused(
        &format!("{JPEG} {key}"),
        result,
        &[key],
        &[],
        &file,
        &before,
    );
}

/// sample.png has an `eXIf` but no IFD1. ExifTool 13.59 creates `[IFD1]
/// XResolution: 300` there (with Compression, YResolution, ResolutionUnit),
/// and #943's in-place PNG writer does the same. oxidex's PNG *reader* does
/// not surface an eXIf IFD1, so the proof reads the chunk's entry bytes, as
/// the transaction's own read-back does.
#[test]
fn write_metadata_png_ifd1_is_written_or_refused_by_name() {
    use oxidex::writers::exif_surgical::{IfdKind, scan_exif_entries};
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, PNG);
    let before = sha(&file);
    let key = "IFD1:XResolution";
    let result = rmw(&file, |map| {
        map.insert(key, TagValue::new_rational(300, 1));
    });
    match result {
        Ok(()) => {
            let bytes = fs::read(&file).unwrap();
            // the eXIf chunk: length, type, data
            let at = bytes
                .windows(4)
                .position(|w| w == b"eXIf")
                .expect("eXIf chunk");
            let len = u32::from_be_bytes(bytes[at - 4..at].try_into().unwrap()) as usize;
            let tiff = &bytes[at + 4..at + 4 + len];
            let entry = scan_exif_entries(tiff)
                .unwrap()
                .entries
                .into_iter()
                .find(|e| e.ifd == IfdKind::Ifd1 && e.tag_id == 0x011a)
                .expect("reported written: IFD1 0x011a must exist");
            let (num, den) = if tiff.starts_with(b"II") {
                (
                    u32::from_le_bytes(entry.value[0..4].try_into().unwrap()),
                    u32::from_le_bytes(entry.value[4..8].try_into().unwrap()),
                )
            } else {
                (
                    u32::from_be_bytes(entry.value[0..4].try_into().unwrap()),
                    u32::from_be_bytes(entry.value[4..8].try_into().unwrap()),
                )
            };
            assert_eq!((entry.field_type, num, den), (5, 300, 1));
        }
        Err(err) => assert_refused(
            &format!("{PNG} {key}"),
            Err(err),
            &[key],
            &[],
            &file,
            &before,
        ),
    }
}

/// One unwritable key refuses the whole request: the writable XPTitle beside
/// it is not half-applied, and every unwritable key is named -- not only the
/// first one met.
#[test]
fn write_metadata_refuses_a_multi_key_request_as_a_whole() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG);
    let before = sha(&file);
    let result = rmw(&file, |map| {
        set(map, "IFD0:XPTitle", "v");
        set(map, "XMP:Title", "v");
        set(map, "File:Comment", "c");
        set(map, "IFD1:ImageDescription", "x");
    });
    assert_refused(
        "JPEG multi-key",
        result,
        &["XMP:Title", "File:Comment", "IFD1:ImageDescription"],
        &["IFD0:XPTitle"],
        &file,
        &before,
    );
}

/// The high-level `Metadata` API writes through `write_metadata` and must
/// refuse the same way.
#[test]
fn metadata_save_and_write_to_refuse_unwritable_keys() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG);
    let before = sha(&file);
    let result = Metadata::from_path(&file)
        .unwrap()
        .set_tag("XMP:Title", "v")
        .save();
    assert_refused(
        "Metadata::save",
        result,
        &["XMP:Title"],
        &[],
        &file,
        &before,
    );

    let result = Metadata::from_path(&file)
        .unwrap()
        .set_tag("File:Comment", "c")
        .write_to(&file);
    assert_refused(
        "Metadata::write_to",
        result,
        &["File:Comment"],
        &[],
        &file,
        &before,
    );
}

/// `CopyBuilder::execute` merges source rows into the destination's map and
/// writes it with `write_metadata`.
#[test]
fn copy_builder_refuses_rows_the_destination_cannot_hold() {
    let dir = TempDir::new().unwrap();
    let source = copy_into(&dir, JPEG_XMP);
    let dest_dir = TempDir::new().unwrap();
    let dest = copy_into(&dest_dir, JPEG);
    let before = sha(&dest);
    let result = Metadata::from_path(&source)
        .unwrap()
        .copy_to(&dest)
        .unwrap()
        .with_tags(&["XMP:Title"])
        .unwrap()
        .execute();
    assert_refused("CopyBuilder", result, &["XMP:Title"], &[], &dest, &before);
}

/// `copy_metadata(src, dest, None)` copies "all"; the XMP it could not write
/// used to vanish behind `Ok(())` (the report that names it is only returned
/// by `copy_metadata_report`).
#[test]
fn copy_metadata_all_refuses_when_it_cannot_copy_everything() {
    let dir = TempDir::new().unwrap();
    let source = copy_into(&dir, JPEG_XMP);
    let dest_dir = TempDir::new().unwrap();
    let dest = copy_into(&dest_dir, JPEG);
    let before = sha(&dest);
    let result = copy_metadata(&source, &dest, None);
    assert_refused(
        "copy_metadata all",
        result,
        &["XMP:Title"],
        &[],
        &dest,
        &before,
    );
}

/// `shift_metadata_dates` on a PNG shifts through the map and
/// `write_metadata`. sample.png's only date the shift parses is the eXIf
/// `ExifIFD:DateTimeOriginal`, which the PNG writer cannot put back where it
/// was: success must mean it now holds the shifted value, at that address.
#[test]
fn shift_metadata_dates_refuses_dates_it_cannot_write() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, PNG);
    let key = "ExifIFD:DateTimeOriginal";
    let before_map = read_metadata(&file).unwrap();
    let original = *before_map
        .get(key)
        .and_then(|value| value.as_datetime())
        .expect("fixture carries a parsed ExifIFD:DateTimeOriginal");
    let before = sha(&file);
    match shift_metadata_dates(&file, "AllDates", "1:0:0 0:0:0", ShiftOperation::Add) {
        Ok(()) => {
            let after = read_metadata(&file).unwrap();
            let shifted = after
                .get(key)
                .and_then(|value| value.as_datetime())
                .copied();
            assert_eq!(
                shifted.map(|date| date.format("%Y").to_string()),
                Some((original.format("%Y").to_string().parse::<i32>().unwrap() + 1).to_string()),
                "{key} reported shifted, but reads back as {:?}",
                after.get(key)
            );
        }
        Err(err) => {
            assert!(
                err.tags_not_written().iter().any(|tag| tag.tag == key),
                "{err:?}"
            );
            assert_eq!(sha(&file), before, "a refused shift changed the file");
        }
    }
}

/// The single-tag APIs already resolved (#945) and keep doing so.
#[test]
fn modify_and_remove_tag_keep_refusing_by_name() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG);
    let before = sha(&file);
    let result = modify_tag(&file, "XMP:Title", TagValue::new_string("v"));
    assert_refused("modify_tag", result, &["XMP:Title"], &[], &file, &before);

    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG_XMP);
    let before = sha(&file);
    let result = remove_tag(&file, "XMP-dc:Title");
    assert_refused("remove_tag", result, &["XMP-dc:Title"], &[], &file, &before);

    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG);
    modify_tag(&file, "XPTitle", TagValue::new_string("v")).unwrap();
    assert_eq!(read_string(&file, "IFD0:XPTitle").as_deref(), Some("v"));
}

/// `write_metadata` and `modify_tag` are one implementation: the same
/// request writes the same bytes through both, or both refuse it with the
/// same message and the file untouched.
#[test]
fn write_metadata_writes_what_modify_tag_writes() {
    for (fixture, key, value) in [
        (JPEG, "IFD0:XPTitle", "v"),
        (JPEG, "IFD0:Artist", "someone"),
        (TIFF, "IFD0:Artist", "someone"),
        (PNG, "PNG:Title", "v"),
        (PDF, "PDF:Title", "v"),
    ] {
        let dir = TempDir::new().unwrap();
        let by_map = copy_into(&dir, fixture);
        let other = TempDir::new().unwrap();
        let by_tag = copy_into(&other, fixture);
        let before = sha(&by_map);
        let map_result = rmw(&by_map, |map| set(map, key, value));
        let tag_result = modify_tag(&by_tag, key, TagValue::new_string(value));
        match (map_result, tag_result) {
            (Ok(()), Ok(())) => {
                assert_eq!(
                    read_string(&by_map, key).as_deref(),
                    Some(value),
                    "{fixture} {key}"
                );
                assert_eq!(
                    fs::read(&by_map).unwrap(),
                    fs::read(&by_tag).unwrap(),
                    "{fixture} {key}: write_metadata and modify_tag disagree"
                );
            }
            (Err(by_map_err), Err(by_tag_err)) => {
                assert_eq!(
                    by_map_err.to_string(),
                    by_tag_err.to_string(),
                    "{fixture} {key}"
                );
                assert_eq!(sha(&by_map), before, "{fixture} {key}");
                assert_eq!(sha(&by_tag), before, "{fixture} {key}");
            }
            (by_map_result, by_tag_result) => panic!(
                "{fixture} {key}: write_metadata {by_map_result:?} but modify_tag {by_tag_result:?}"
            ),
        }
    }
}

/// A read-modify-write that changes nothing requests nothing: the file is
/// left alone (no re-layout, no empty PDF revision).
#[test]
fn write_metadata_with_the_files_own_map_changes_nothing() {
    for fixture in [JPEG, JPEG_XMP, TIFF, PNG, PDF] {
        let dir = TempDir::new().unwrap();
        let file = copy_into(&dir, fixture);
        let before = sha(&file);
        rmw(&file, |_| {}).unwrap();
        assert_eq!(sha(&file), before, "{fixture}: nothing was requested");
    }
}

#[path = "common/fixtures.rs"]
mod fixtures;

/// The EXIF directories a whole-EXIF deletion must leave no row in.
const EXIF_DIRECTORY_GROUPS: &[&str] = &["IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD", "SubIFD"];

fn exif_rows(path: &Path) -> Vec<String> {
    read_metadata(path)
        .unwrap()
        .keys()
        .filter(|key| {
            key.split_once(':')
                .is_some_and(|(group, _)| EXIF_DIRECTORY_GROUPS.contains(&group))
        })
        .cloned()
        .collect()
}

/// `remove_tag("EXIF:All")` is ExifTool's `-EXIF:All=`: routed through the
/// transaction to #943's group-wide expansion, it strips the EXIF block and
/// returns Ok. Pinned 13.59 on t/images/Canon.jpg: `1 image files updated`,
/// no IFD0/ExifIFD/InteropIFD/MakerNotes left.
#[test]
fn remove_tag_exif_all_strips_exif_from_canon_jpg() {
    let Some(canon) = fixtures::pinned_t_images_fixture_path("Canon.jpg") else {
        eprintln!("skipped: pinned t/images/Canon.jpg is not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("Canon.jpg");
    fs::copy(&canon, &file).unwrap();
    assert!(!exif_rows(&file).is_empty(), "Canon.jpg carries EXIF");
    let before = sha(&file);
    remove_tag(&file, "EXIF:All").expect("EXIF:All strips EXIF");
    assert_ne!(
        sha(&file),
        before,
        "EXIF:All reported done without a change"
    );
    assert_eq!(exif_rows(&file), Vec::<String>::new(), "EXIF rows left");
}

/// `remove_tag("GPS:All")` on t/images/PNG.png, which holds no GPS: Ok and
/// the file byte-identical (pinned 13.59: `0 image files updated` / `1 image
/// files unchanged`).
#[test]
fn remove_tag_gps_all_on_png_without_gps_is_a_byte_identical_ok() {
    let Some(png) = fixtures::pinned_t_images_fixture_path("PNG.png") else {
        eprintln!("skipped: pinned t/images/PNG.png is not available");
        return;
    };
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("PNG.png");
    fs::copy(&png, &file).unwrap();
    let before = sha(&file);
    remove_tag(&file, "GPS:All").expect("GPS:All on a PNG without GPS is a no-op");
    assert_eq!(
        sha(&file),
        before,
        "a no-op group deletion changed the file"
    );
}

/// `remove_tag("XMP:All")` where the file holds XMP: pinned 13.59 deletes
/// it; oxidex has no XMP writer, so it refuses by name, file untouched. A
/// `<group>:All` set is refused too (it only deletes).
#[test]
fn remove_tag_xmp_all_is_refused_by_name() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG_XMP);
    let before = sha(&file);
    let result = remove_tag(&file, "XMP:All");
    assert_refused(
        "remove_tag XMP:All",
        result,
        &["XMP:All"],
        &[],
        &file,
        &before,
    );
    let result = modify_tag(&file, "GPS:All", TagValue::new_string("x"));
    assert_refused(
        "modify_tag GPS:All",
        result,
        &["GPS:All"],
        &[],
        &file,
        &before,
    );
}
