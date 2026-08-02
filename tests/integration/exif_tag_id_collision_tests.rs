//! Two metadata-map keys can name one EXIF tag id. Neither may vanish silently.
//!
//! `plan_exif_write` may emit only one IFD record per tag id, so when two keys
//! resolve to the same id the second one cannot be written. The guard that
//! enforced this used to `continue` — dropping the caller's edit while the CLI
//! still printed "1 image files updated" and exited 0:
//!
//! ```text
//! $ oxidex -ExifIFD:ExposureBiasValue=-0.5 photo.jpg   # 0x9204
//!     1 image files updated                            # exit 0
//! $ exiftool -a -G1 -s -ExposureCompensation photo.jpg
//! [ExifIFD]  ExposureCompensation : -3/2                # unchanged
//! ```
//!
//! ExifTool refuses the same command outright — `ExposureBiasValue` is not a
//! writable tag name in any of its tables, only a `Notes` remark on 0x9204
//! ("called ExposureBiasValue by the EXIF spec", Exif.pm:2369) and an XMP
//! property whose tag name is again `ExposureCompensation` (XMP.pm:2096-2097):
//!
//! ```text
//! $ exiftool -ExifIFD:ExposureBiasValue=-0.5 photo.jpg
//! Warning: Tag 'ExifIFD:ExposureBiasValue' is not defined     # Writer.pl:581-584
//! Nothing to do.                                              # exit 1
//! ```
//!
//! So a collision that would lose an edit is now refused loudly, which matches
//! ExifTool's outcome. A collision that would lose *nothing* — the same value
//! already planned under another spelling, which is what the documented
//! `-EXIF:Tag=value` syntax produces — is still skipped silently.
//!
//! Field types are asserted from the serialized bytes, not the reader's
//! rendering: an ASCII "-0.5" and a rational -1/2 both print as a number, so a
//! type-blind assertion would pass on corrupt bytes.

use oxidex::writers::exif_surgical::{IfdKind, RawEntry, scan_exif_entries};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::NamedTempFile;

const JPEG_FIXTURE: &str = "tests/fixtures/jpeg/simple/synthetic_001.jpg";

// TIFF field types (TIFF 6.0 §2)
const ASCII: u16 = 2;
const SRATIONAL: u16 = 10;

// Tag ids
const EXPOSURE_TIME: u16 = 0x829A;
const EXPOSURE_COMPENSATION: u16 = 0x9204;

fn oxidex(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .expect("run oxidex binary")
}

fn copy_fixture(suffix: &str) -> NamedTempFile {
    let temp = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create temp fixture copy");
    fs::copy(JPEG_FIXTURE, temp.path()).expect("copy fixture");
    let mut perms = fs::metadata(temp.path()).expect("stat copy").permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(temp.path(), perms).expect("make copy writable");
    temp
}

fn path_of(temp: &NamedTempFile) -> String {
    temp.path().to_str().expect("utf-8 temp path").to_string()
}

/// The raw EXIF entries of a JPEG, scanned out of its APP1 `Exif\0\0` segment.
fn jpeg_entries(path: &Path) -> Vec<RawEntry> {
    let bytes = fs::read(path).expect("read written jpeg");
    let marker = b"Exif\0\0";
    let start = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .expect("jpeg carries an EXIF APP1 segment")
        + marker.len();
    scan_exif_entries(&bytes[start..])
        .expect("scan EXIF structure")
        .entries
}

fn entry(path: &Path, ifd: IfdKind, tag_id: u16) -> RawEntry {
    jpeg_entries(path)
        .into_iter()
        .find(|e| e.ifd == ifd && e.tag_id == tag_id)
        .unwrap_or_else(|| panic!("no {ifd:?} entry for tag 0x{tag_id:04x}"))
}

fn count_entries(path: &Path, tag_id: u16) -> usize {
    jpeg_entries(path)
        .iter()
        .filter(|e| e.tag_id == tag_id)
        .count()
}

/// Seeds the fixture copy with a known 0x9204, using oxidex's own native-name
/// write (proven correct by `native_name_writes_a_true_srational`).
fn seed_exposure_compensation(temp: &NamedTempFile, value: &str) {
    let path = path_of(temp);
    let spec = format!("-ExifIFD:ExposureCompensation={value}");
    let out = oxidex(&[&spec, &path]);
    assert!(out.status.success(), "seeding 0x9204 failed");
}

// ---------------------------------------------------------------------------
// The dropped write must be reported, not swallowed
// ---------------------------------------------------------------------------

#[test]
fn alias_colliding_with_a_present_tag_fails_loudly() {
    let temp = copy_fixture(".jpg");
    seed_exposure_compensation(&temp, "-1.5");
    let before = fs::read(temp.path()).expect("read seeded copy");

    let path = path_of(&temp);
    let out = oxidex(&["-ExifIFD:ExposureBiasValue=-0.5", &path]);

    assert!(
        !out.status.success(),
        "a write that cannot be applied must not report success; stdout={}",
        String::from_utf8_lossy(&out.stdout),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ExposureBiasValue") && stderr.contains("0x9204"),
        "error must name the tag and the colliding id, got: {stderr}",
    );

    let after = fs::read(temp.path()).expect("read copy after refusal");
    assert_eq!(before, after, "a refused write must not touch the file");
}

#[test]
fn refused_collision_leaves_the_original_value_intact() {
    let temp = copy_fixture(".jpg");
    seed_exposure_compensation(&temp, "-1.5");
    let path = path_of(&temp);

    let _ = oxidex(&["-ExifIFD:ExposureBiasValue=-0.5", &path]);

    let e = entry(temp.path(), IfdKind::ExifIfd, EXPOSURE_COMPENSATION);
    assert_eq!(e.field_type, SRATIONAL);
    // -3/2 as SRATIONAL, big- or little-endian agnostic via the scanned bytes
    assert_eq!(e.count, 1, "0x9204 must remain a single-valued entry");
    assert_eq!(
        count_entries(temp.path(), EXPOSURE_COMPENSATION),
        1,
        "a refused write must not append a second record",
    );
}

// ---------------------------------------------------------------------------
// The registry type fix: 0x9204 is rational64s, not a string
// ---------------------------------------------------------------------------

#[test]
fn native_name_writes_a_true_srational() {
    let temp = copy_fixture(".jpg");
    let path = path_of(&temp);
    let out = oxidex(&["-ExifIFD:ExposureCompensation=-0.5", &path]);
    assert!(out.status.success(), "native-name write must succeed");

    let e = entry(temp.path(), IfdKind::ExifIfd, EXPOSURE_COMPENSATION);
    assert_eq!(
        e.field_type, SRATIONAL,
        "ExifTool declares 0x9204 Writable => 'rational64s'; a string \"-0.5\" \
         serialized as ASCII would round-trip as a number but is the wrong type",
    );
    assert_ne!(e.field_type, ASCII);
    assert_eq!(e.count, 1);
}

#[test]
fn date_tags_are_normalised_not_stored_verbatim() {
    for tag in ["ModifyDate", "CreateDate"] {
        let temp = copy_fixture(".jpg");
        let path = path_of(&temp);
        let spec = format!("-ExifIFD:{tag}=2024-01-15T10:30:00");
        let out = oxidex(&[&spec, &path]);
        assert!(out.status.success(), "{tag} write must succeed");

        let read = oxidex(&[&path]);
        let stdout = String::from_utf8_lossy(&read.stdout).into_owned();
        let line = stdout
            .lines()
            .find(|l| l.starts_with(&format!("ExifIFD:{tag}: ")))
            .unwrap_or_else(|| panic!("{tag} not read back"));
        assert!(
            line.ends_with("2024:01:15 10:30:00"),
            "{tag} must be normalised to ExifTool's stored form, got: {line}",
        );
    }
}

// ---------------------------------------------------------------------------
// A collision that loses nothing stays silent
// ---------------------------------------------------------------------------

#[test]
fn exif_family_alias_still_writes_and_makes_no_duplicate() {
    let temp = copy_fixture(".jpg");
    let path = path_of(&temp);

    // The fixture carries no ExposureTime, so seed one into ExifIFD first --
    // the duplicate only arises when the tag already exists there.
    let seed = oxidex(&["-ExifIFD:ExposureTime=1/60", &path]);
    assert!(seed.status.success(), "seeding 0x829A failed");
    assert_eq!(count_entries(temp.path(), EXPOSURE_TIME), 1);

    // "EXIF:" names the tag family, not a physical IFD. The alias fold routes
    // the edit to the native ExifIFD key; the leftover "EXIF:" key must then be
    // recognised as the same value and skipped, not appended to IFD0.
    let out = oxidex(&["-EXIF:ExposureTime=1/250", &path]);
    assert!(
        out.status.success(),
        "documented -EXIF:Tag= syntax must work"
    );

    assert_eq!(
        count_entries(temp.path(), EXPOSURE_TIME),
        1,
        "0x829A must exist exactly once; main wrote it to both IFD0 and ExifIFD",
    );
    let e = entry(temp.path(), IfdKind::ExifIfd, EXPOSURE_TIME);
    assert_eq!(
        e.count, 1,
        "the edit must land on the existing ExifIFD entry"
    );
}

#[test]
fn ordinary_single_tag_write_is_unaffected() {
    let temp = copy_fixture(".jpg");
    let path = path_of(&temp);
    let out = oxidex(&["-IFD0:Artist=Ada Lovelace", &path]);
    assert!(out.status.success(), "plain write must still succeed");

    let read = oxidex(&[&path]);
    let stdout = String::from_utf8_lossy(&read.stdout).into_owned();
    assert!(
        stdout.contains("IFD0:Artist: Ada Lovelace"),
        "Artist must round-trip",
    );
}
