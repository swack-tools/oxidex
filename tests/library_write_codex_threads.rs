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

/// Thread 3 (write_transaction.rs:549). A PNG text chunk whose literal
/// keyword is `XMP` surfaces as `PNG:XMP`, and the PNG writer edits that
/// chunk; the proof must read that chunk back, not look for an
/// `XML:com.adobe.xmp` packet. (13.59 answers `-PNG:XMP=world` on such a file
/// `Sorry, PNG:XMP doesn't exist or isn't writable`; oxidex edits the chunk
/// its reader reports -- the point here is that success is not refused.)
#[test]
fn a_literal_xmp_text_chunk_is_verified_as_the_chunk_written() {
    let dir = TempDir::new().unwrap();
    let mut bytes = fs::read(PNG_TEXT).unwrap();
    let iend = bytes.windows(4).rposition(|w| w == b"IEND").unwrap() - 4;
    bytes.splice(iend..iend, png_chunk(b"tEXt", b"XMP\0hello"));
    let file = dir.path().join("xmp.png");
    fs::write(&file, &bytes).unwrap();
    assert_eq!(
        read_metadata(&file).unwrap().get_string("PNG:XMP"),
        Some("hello")
    );
    let outcome = modify_tag(&file, "PNG:XMP", TagValue::new_string("world"))
        .expect("the text chunk is edited and read back");
    assert_eq!(outcome, WriteOutcome::Updated);
    assert_eq!(
        read_metadata(&file).unwrap().get_string("PNG:XMP"),
        Some("world")
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
/// the caller's set, not a carried row: here the stored XPTitle is typed
/// `undef` (7), which 13.59 rewrites as `int8u` for `-XPTitle=v`. It must be
/// applied or refused by name -- never dropped as `Unchanged`.
#[test]
fn an_explicit_same_text_xp_set_is_not_discarded() {
    let dir = TempDir::new().unwrap();
    let file = copy_into(&dir, JPEG, "a.jpg");
    modify_tag(&file, "IFD0:XPTitle", TagValue::new_string("v")).unwrap();
    let mut bytes = fs::read(&file).unwrap();
    // synthetic_001.jpg is big-endian: tag 0x9c9b, type 0x0001 -> 0x0007.
    let at = bytes
        .windows(4)
        .position(|w| w == [0x9c, 0x9b, 0x00, 0x01])
        .expect("the XPTitle entry");
    bytes[at + 3] = 0x07;
    fs::write(&file, &bytes).unwrap();
    assert_eq!(
        read_metadata(&file).unwrap().get_string("IFD0:XPTitle"),
        Some("v")
    );
    let before = fs::read(&file).unwrap();
    let mut map = read_metadata(&file).unwrap();
    map.insert("IFD0:XPTitle", TagValue::new_string("v"));
    match write_metadata(&file, &map) {
        Ok(WriteOutcome::Unchanged) => {
            panic!("an explicit XP set was dropped as a carried row")
        }
        Ok(_) => assert_ne!(fs::read(&file).unwrap(), before),
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
