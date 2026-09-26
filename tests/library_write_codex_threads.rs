//! The seven Codex review threads on #951 at 1e9bccae, each pinned by a test
//! that fails there. Oracle rows are pinned ExifTool 13.59 (`perl5.38.2
//! -I<pinned>/lib <pinned>/exiftool -config ""`; probes 13.59 / DOCX),
//! evidence `library-ffi-writes/codex-threads/oracle-order.txt`.

use oxidex::core::operations::{
    modify_tag, read_metadata, read_metadata_with_detector_and_options, remove_tag, write_metadata,
};
use oxidex::core::{ReadOptions, TagChange, TagValue, WriteOutcome, apply_tag_changes};
use oxidex::error::ExifToolError;
use oxidex::parsers::DetectorMode;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const JPEG: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";
const PNG_TEXT: &str = "tests/fixtures/png/simple/synthetic_text_001.png";

fn copy_into(dir: &TempDir, fixture: &str, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::copy(fixture, &path).unwrap();
    path
}

fn oxidex(args: &[&str], file: &Path) -> (Option<i32>, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(file)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

/// Thread 1 (write_transaction.rs:296). A later deletion of the same field
/// wins over an earlier set, even when the file never held the tag. 13.59 on
/// synthetic_001.jpg (no XPTitle): `-IFD0:XPTitle=x -IFD0:XPTitle=` is `0
/// image files updated` / `1 image files unchanged`; with Artist present,
/// `-IFD0:Artist=x -IFD0:Artist=` deletes Artist.
#[test]
fn a_trailing_delete_after_a_set_of_the_same_field_wins() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let before = fs::read(&file).unwrap();
    let outcome = apply_tag_changes(
        &file,
        &[
            TagChange::set("IFD0:XPTitle", TagValue::new_string("x")),
            TagChange::delete("IFD0:XPTitle"),
        ],
    )
    .unwrap();
    assert_eq!(outcome, WriteOutcome::Unchanged);
    assert_eq!(fs::read(&file).unwrap(), before, "the delete must win");

    let (code, out) = oxidex(&["-IFD0:XPTitle=x", "-IFD0:XPTitle="], &file);
    assert_eq!(code, Some(0));
    assert_eq!(
        out,
        "    0 image files updated\n    1 image files unchanged\n"
    );
    assert_eq!(fs::read(&file).unwrap(), before);

    let (code, _) = oxidex(&["-IFD0:Artist=x", "-IFD0:Artist="], &file);
    assert_eq!(code, Some(0));
    assert!(read_metadata(&file).unwrap().get("IFD0:Artist").is_none());
}

/// Thread 2 (write_transaction.rs:146). A request that is all no-ops needs
/// no scratch file, so it succeeds in a directory the caller cannot write.
#[cfg(unix)]
#[test]
fn no_op_requests_need_no_scratch_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let sealed = |mode| fs::set_permissions(dir.path(), fs::Permissions::from_mode(mode)).unwrap();
    sealed(0o555);
    // Root can create files anyway; the property is then unobservable.
    let probe = tempfile::tempfile_in(dir.path());
    if probe.is_ok() {
        sealed(0o755);
        eprintln!("skipped: the directory is writable despite 0555 (running as root?)");
        return;
    }
    let absent = remove_tag(&file, "IFD0:XPTitle");
    let empty = apply_tag_changes(&file, &[]);
    let same = write_metadata(&file, &read_metadata(&file).unwrap());
    sealed(0o755);
    assert_eq!(absent.unwrap(), WriteOutcome::Unchanged);
    assert_eq!(empty.unwrap(), WriteOutcome::Unchanged);
    assert_eq!(same.unwrap(), WriteOutcome::Unchanged);
}

/// PNG chunk with a correct CRC.
fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut crc = 0xffff_ffffu32;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(kind);
    chunk.extend_from_slice(data);
    chunk.extend_from_slice(&(!crc).to_be_bytes());
    chunk
}

/// Thread 3 (write_transaction.rs:549), superseded by the maintainer's
/// decision on PR #951 review comment 4098201945: a PNG text chunk whose
/// literal keyword is `XMP` surfaces as `PNG:XMP`, and pinned ExifTool 13.59
/// refuses to write that key at all -- `Sorry, PNG:XMP doesn't exist or
/// isn't writable` / `Nothing to do.`, on both a set and a deletion, file
/// untouched (measured directly against the pinned oracle: `-PNG:XMP=world`
/// and `-PNG:XMP=` on a copy of this same chunk both print exactly that
/// warning, exit 1, and leave the file byte-identical). oxidex used to edit
/// the literal chunk instead (3010ab4b); it now matches ExifTool and refuses
/// through the same typed [`ExifToolError::TagsNotWritten`] every other
/// unwritable-tag request uses, for `modify_tag`, `remove_tag` and
/// `apply_tag_changes` alike. A real XMP packet (no literal `XMP`-keyword
/// chunk) is unaffected -- see
/// `production_wiring_tests::png_write_carries_xmp_chunk_but_still_removes_dropped_text_chunks`.
#[test]
fn a_literal_xmp_text_chunk_is_not_writable() {
    let dir = TempDir::new().unwrap();
    let mut bytes = fs::read(PNG_TEXT).unwrap();
    let iend = bytes.windows(4).rposition(|w| w == b"IEND").unwrap() - 4;
    bytes.splice(iend..iend, png_chunk(b"tEXt", b"XMP\0hello"));
    let file = dir.path().join("xmp.png");
    fs::write(&file, &bytes).unwrap();
    let before = fs::read(&file).unwrap();
    assert_eq!(
        read_metadata(&file).unwrap().get_string("PNG:XMP"),
        Some("hello")
    );

    let set_err = modify_tag(&file, "PNG:XMP", TagValue::new_string("world"))
        .expect_err("13.59 refuses a set of a literal XMP-keyword chunk");
    assert!(
        matches!(&set_err, ExifToolError::TagsNotWritten { .. }),
        "{set_err:?}"
    );
    let set_refused = set_err.tags_not_written();
    assert_eq!(set_refused.len(), 1);
    assert_eq!(set_refused[0].tag, "PNG:XMP");
    assert_eq!(
        set_refused[0].reason,
        "Sorry, PNG:XMP doesn't exist or isn't writable"
    );
    assert_eq!(fs::read(&file).unwrap(), before, "set must not touch bytes");

    let delete_err =
        remove_tag(&file, "PNG:XMP").expect_err("13.59 refuses a deletion of the same chunk");
    let delete_refused = delete_err.tags_not_written();
    assert_eq!(delete_refused.len(), 1);
    assert_eq!(delete_refused[0].tag, "PNG:XMP");
    assert_eq!(
        delete_refused[0].reason,
        "Sorry, PNG:XMP doesn't exist or isn't writable"
    );
    assert_eq!(
        fs::read(&file).unwrap(),
        before,
        "deletion must not touch bytes"
    );
    assert_eq!(
        read_metadata(&file).unwrap().get_string("PNG:XMP"),
        Some("hello"),
        "the chunk is exactly as it was"
    );
}

fn stored_groups(path: &Path) -> Vec<String> {
    let mut groups: Vec<String> = read_metadata(path)
        .unwrap()
        .keys()
        .filter_map(|key| key.split_once(':').map(|(g, _)| g.to_string()))
        .filter(|g| {
            matches!(
                g.as_str(),
                "IFD0" | "ExifIFD" | "GPS" | "IFD1" | "InteropIFD"
            )
        })
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

/// Thread 4 (write_transaction.rs:351). Group deletions keep their place in
/// the request order. 13.59 on synthetic_001.jpg: `-EXIF:All=
/// -IFD0:Artist=x` leaves exactly `[IFD0] Artist: x`; `-IFD0:Artist=x
/// -EXIF:All=` leaves no EXIF at all.
#[test]
fn group_deletions_keep_their_place_in_the_request_order() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let outcome = apply_tag_changes(
        &file,
        &[
            TagChange::delete("EXIF:All"),
            TagChange::set("IFD0:Artist", TagValue::new_string("x")),
        ],
    )
    .expect("delete EXIF, then set Artist");
    assert_eq!(outcome, WriteOutcome::Updated);
    let after = read_metadata(&file).unwrap();
    assert_eq!(after.get_string("IFD0:Artist"), Some("x"));
    assert!(after.get("IFD0:Make").is_none(), "EXIF:All deleted Make");
    assert!(after.get("ExifIFD:ExifVersion").is_none());

    let file = copy_into(&dir, JPEG, "b.jpg");
    apply_tag_changes(
        &file,
        &[
            TagChange::set("IFD0:Artist", TagValue::new_string("x")),
            TagChange::delete("EXIF:All"),
        ],
    )
    .expect("set Artist, then delete EXIF");
    assert_eq!(stored_groups(&file), Vec::<String>::new(), "EXIF left");

    let file = copy_into(&dir, JPEG, "c.jpg");
    let (code, out) = oxidex(&["-EXIF:All=", "-IFD0:Artist=x"], &file);
    assert_eq!(
        (code, out.as_str()),
        (Some(0), "    1 image files updated\n")
    );
    assert_eq!(stored_groups(&file), ["IFD0"]);
    assert_eq!(
        read_metadata(&file).unwrap().get_string("IFD0:Artist"),
        Some("x")
    );
}

/// Thread 5 (operations.rs:988). `docs/reference/api-reference.md` states
/// the write signatures `tests/api_reference_signatures.rs` compile-checks.
#[test]
fn the_api_reference_states_the_write_outcome_signatures() {
    let doc = fs::read_to_string("docs/reference/api-reference.md").unwrap();
    for line in [
        "pub fn modify_tag(path: &Path, tag_name: &str, new_value: TagValue) -> Result<WriteOutcome>;",
        "pub fn remove_tag(path: &Path, tag_name: &str) -> Result<WriteOutcome>;",
        "pub fn write_metadata(path: &Path, metadata: &MetadataMap) -> Result<WriteOutcome>;",
        "pub fn clear_all_metadata(path: &Path) -> Result<WriteOutcome>;",
        "`fn save(&self) -> Result<WriteOutcome>`",
    ] {
        assert!(doc.contains(line), "api-reference.md lacks: {line}");
    }
    assert!(
        !doc.contains("-> Result<()>"),
        "a stale Result<()> signature"
    );
}

/// Thread 6a (write_transaction.rs:220). A row only an optioned read
/// produces (`File:JPEGQualityEstimate`, requested) is the read's, not the
/// caller's: writing the untouched map back is a no-op, not a refused set.
#[test]
fn rows_of_an_optioned_read_are_not_sets() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    let options = ReadOptions::new(&["JPEGQualityEstimate".to_string()], false);
    let map =
        read_metadata_with_detector_and_options(&file, DetectorMode::Signature, &options).unwrap();
    assert!(
        map.keys().any(|key| key.ends_with(":JPEGQualityEstimate")),
        "the optioned read produces the estimate"
    );
    assert_eq!(
        write_metadata(&file, &map).unwrap(),
        WriteOutcome::Unchanged
    );
}

/// Thread 6b. An explicit re-set of the text an XP tag already reads as is
/// the caller's set, not a carried row. Here the stored XPTitle is `A` plus
/// the surrogate pair of U+1F38C, which reads as `A🎌`; 13.59's
/// `-XPTitle=A🎌` rewrites it as `41 00 8c f3 00 00` (the code point's low 16
/// bits, `Encode($val,"UCS2","II")`). The set must be applied or refused by
/// name -- never dropped as `Unchanged` with the surrogate pair still there.
#[test]
fn an_explicit_same_text_xp_set_is_not_discarded() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    modify_tag(&file, "IFD0:XPTitle", TagValue::new_string("ABC")).unwrap();
    let mut bytes = fs::read(&file).unwrap();
    let at = bytes
        .windows(8)
        .position(|w| w == [0x41, 0, 0x42, 0, 0x43, 0, 0, 0])
        .expect("the XPTitle bytes");
    bytes[at + 2..at + 6].copy_from_slice(&[0x3c, 0xd8, 0x8c, 0xdf]);
    fs::write(&file, &bytes).unwrap();
    let text = read_metadata(&file)
        .unwrap()
        .get_string("IFD0:XPTitle")
        .map(str::to_string)
        .expect("the stored XPTitle reads");
    assert_eq!(text, "A\u{1f38c}");
    let before = fs::read(&file).unwrap();
    let mut map = read_metadata(&file).unwrap();
    map.insert("IFD0:XPTitle", TagValue::new_string(text));
    match write_metadata(&file, &map) {
        Ok(WriteOutcome::Unchanged) => {
            panic!("an explicit XP set was dropped as a carried row")
        }
        Ok(_) => {
            let after = fs::read(&file).unwrap();
            assert!(
                after.windows(6).any(|w| w == [0x41, 0, 0x8c, 0xf3, 0, 0]),
                "written, but not as 13.59 writes it"
            );
        }
        Err(err) => {
            assert!(
                matches!(&err, ExifToolError::TagsNotWritten { .. })
                    && err.tags_not_written()[0].tag == "IFD0:XPTitle",
                "{err:?}"
            );
            assert_eq!(fs::read(&file).unwrap(), before);
        }
    }
}
