//! IFD0-group edits of a Panasonic RAW/RW2/RWL file, routed as pinned
//! ExifTool 13.59 routes them.
//!
//! Such a file's outer IFD0 is read and written with the
//! `PanasonicRaw::Main` table (ExifTool.pm 13.59:8646-8659, TIFF magic
//! 0x55), not `Exif::Main`, and its 0x002e JpgFromRaw is "processed as an
//! embedded document because it contains full EXIF" (PanasonicRaw.pm
//! 13.59:198-216), rewritten by `WriteJpgFromRaw` (:831-866) with its own
//! IFD0 -- `Doc1:IFD0` in a `-G3` read-back. So an `IFD0:` tag lands:
//!
//! - in the outer IFD0 only, for a `PanasonicRaw::Main` tag `Exif::Main`
//!   does not name (`ISO` 0x0017, `LinearityLimitRed`, `WBRedLevel`, ...);
//! - in `Doc1:IFD0` only, for an `Exif::Main` tag the outer table lacks
//!   (`Software`, `ImageDescription`, `XResolution`), or holds only as
//!   `Permanent` and the camera did not write (`Artist`, `Copyright`:
//!   PanasonicRaw.pm:307-313, 337-346);
//! - in both, for `Make`, `Model` and `Orientation`.
//!
//! This writer edits the outer TIFF in place and never the JpgFromRaw, whose
//! length an edit changes (and with it the 0x002e offset/length and the raw
//! data after it, which ExifTool re-lays out through RawDataOffset). So an
//! edit ExifTool makes in `Doc1:IFD0` is refused by name, never made in the
//! outer IFD0 alone; one it makes in the outer IFD0 that this writer cannot
//! make (a deletion -- it cannot shrink an IFD -- or a `PanasonicRaw` tag it
//! does not write) is refused by name, never reported done unchanged.
//!
//! Every expectation below was measured with the pinned oracle (`-a -G3:1
//! -s` read-backs; evidence `rw2-embedded-ifd0/`), and the oracle-graded
//! test re-measures it when an oracle is resolved.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{modify_tag, read_metadata, remove_tag};
use oxidex::core::tag_value::TagValue;
use oxidex::exiftool_oracle;
use std::path::{Path, PathBuf};

fn sample() -> Option<Vec<u8>> {
    let path = fixtures::pinned_t_images_fixture_path("Panasonic.rw2")?;
    Some(std::fs::read(path).unwrap())
}

fn u16_le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The file offset of each IFD0 record of a little-endian TIFF starting at
/// `tiff`, with its tag id.
fn ifd0_records(b: &[u8], tiff: usize) -> Vec<(usize, u16)> {
    let ifd0 = tiff + u32_le(b, tiff + 4) as usize;
    (0..usize::from(u16_le(b, ifd0)))
        .map(|i| ifd0 + 2 + 12 * i)
        .map(|at| (at, u16_le(b, at)))
        .collect()
}

/// (offset, length) of the outer 0x002e JpgFromRaw.
fn jpg_from_raw(b: &[u8]) -> (usize, usize) {
    let (at, _) = ifd0_records(b, 0)
        .into_iter()
        .find(|(_, tag)| *tag == 0x002e)
        .expect("no JpgFromRaw");
    (u32_le(b, at + 8) as usize, u32_le(b, at + 4) as usize)
}

/// The TIFF header offset of the JpgFromRaw's APP1 EXIF block.
fn embedded_tiff(b: &[u8]) -> usize {
    let (start, _) = jpg_from_raw(b);
    let mut p = start + 2;
    loop {
        let len = usize::from(u16::from_be_bytes([b[p + 2], b[p + 3]]));
        if b[p + 1] == 0xE1 && &b[p + 4..p + 10] == b"Exif\0\0" {
            return p + 10;
        }
        p += 2 + len;
    }
}

/// t/images Panasonic.rw2 with its JpgFromRaw record renumbered 0x0040 (no
/// `PanasonicRaw::Main` tag; the order of the records is kept): a Panasonic
/// RW2 with no embedded document, `-validate` OK.
fn without_jpg_from_raw(original: &[u8]) -> Vec<u8> {
    let mut b = original.to_vec();
    let (at, _) = ifd0_records(&b, 0)
        .into_iter()
        .find(|(_, tag)| *tag == 0x002e)
        .unwrap();
    b[at..at + 2].copy_from_slice(&0x0040u16.to_le_bytes());
    b
}

/// t/images Panasonic.rw2 with the JpgFromRaw's own IFD0 Make "Panasonic"
/// replaced by `make` (same length): the outer and embedded Make disagree.
fn with_embedded_make(original: &[u8], make: &[u8; 10]) -> Vec<u8> {
    let mut b = original.to_vec();
    let tiff = embedded_tiff(&b);
    let (at, _) = ifd0_records(&b, tiff)
        .into_iter()
        .find(|(_, tag)| *tag == 0x010f)
        .unwrap();
    assert_eq!(u32_le(&b, at + 4), 10);
    let value = tiff + u32_le(&b, at + 8) as usize;
    assert_eq!(&b[value..value + 10], b"Panasonic\0");
    b[value..value + 10].copy_from_slice(make);
    b
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Runs the CLI with one argument on a fresh copy of `original`: (exit
/// code, stdout+stderr, the file afterwards).
fn cli(dir: &Path, original: &[u8], arg: &str) -> (Option<i32>, String, Vec<u8>) {
    let path = write(dir, "cli.rw2", original);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg(arg)
        .arg(&path)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), text, std::fs::read(&path).unwrap())
}

/// `arg` is refused (exit 1), the file untouched, and the message names the
/// tag and every word of `why`.
fn assert_cli_refused(dir: &Path, original: &[u8], label: &str, arg: &str, why: &[&str]) {
    let (code, text, after) = cli(dir, original, arg);
    assert_eq!(code, Some(1), "{label} {arg}: not refused: {text}");
    assert!(after == original, "{label} {arg}: file touched");
    let tag = arg
        .trim_start_matches('-')
        .split('=')
        .next()
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap();
    assert!(
        text.to_lowercase().contains(&tag.to_lowercase()),
        "{label} {arg}: message does not name {tag}: {text}"
    );
    for word in why {
        assert!(text.contains(word), "{label} {arg}: no '{word}' in: {text}");
    }
}

/// Every `IFD0:` set pinned ExifTool 13.59 makes in t/images
/// Panasonic.rw2's `Doc1:IFD0` -- alone (`Artist`, `Copyright`,
/// `ImageDescription`, `XResolution`) or beside the outer IFD0 (`Make`,
/// `Model`; `ISO`, which it also adds as `Doc1:IFD0` 0x8827 and outer
/// 0x0037) -- is refused by name, file untouched, library and CLI. At
/// 8991992e (and tip 8825f101) `Artist`, `Copyright`, `Make` and `Model`
/// were written to the outer IFD0 only, and `ImageDescription` and
/// `XResolution` to an outer entry the `PanasonicRaw::Main` table does not
/// read: "1 image files updated", a read-back unlike the oracle's.
#[test]
fn rw2_ifd0_sets_exiftool_makes_in_the_jpgfromraw_are_refused_by_name() {
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for arg in [
        "-IFD0:Artist=x",
        "-IFD0:Copyright=c",
        "-IFD0:ImageDescription=d",
        "-IFD0:XResolution=300",
        "-IFD0:Make=Acme",
        "-IFD0:Model=M",
        "-IFD0:ISO=100",
        "-ifd0:artist=x",
    ] {
        assert_cli_refused(dir.path(), &original, "Panasonic.rw2", arg, &["JpgFromRaw"]);
    }
    for (key, value) in [
        ("IFD0:Artist", TagValue::new_string("x")),
        ("IFD0:Make", TagValue::new_string("Acme")),
    ] {
        let path = write(dir.path(), "lib.rw2", &original);
        let err = modify_tag(&path, key, value).expect_err(key);
        assert!(err.to_string().contains("JpgFromRaw"), "{key}: {err}");
        assert!(std::fs::read(&path).unwrap() == original, "{key}: touched");
    }
    // Make where the two disagree: `IFD0:Make=Panasonic` is the outer
    // Make's own value, but ExifTool rewrites the embedded one (Acmesonic
    // -> Panasonic). Reported done unchanged at 8991992e.
    let split = with_embedded_make(&original, b"Acmesonic\0");
    assert_cli_refused(
        dir.path(),
        &split,
        "split Make",
        "-IFD0:Make=Panasonic",
        &["JpgFromRaw"],
    );
    let path = write(dir.path(), "lib.rw2", &split);
    let err = modify_tag(&path, "IFD0:Make", TagValue::new_string("Panasonic"))
        .expect_err("split Make, library");
    assert!(err.to_string().contains("JpgFromRaw"), "{err}");
    assert!(std::fs::read(&path).unwrap() == split);
}

/// An edit pinned ExifTool 13.59 makes in the outer `PanasonicRaw::Main`
/// IFD0 that this writer cannot make is refused by name, never reported
/// done with the file byte-identical: deleting `ISO` (0x0017) and the other
/// `PanasonicRaw` tags (the writer cannot shrink an IFD), whatever the
/// spelling (`IFD0:`, the family-0 `EXIF:` of `PanasonicRaw::Main`, a bare
/// name, lowercase), and setting one (`LinearityLimitRed=4000`), which this
/// writer does not write. `IFD0:SensorWidth` is not writable at all
/// (ExifTool: "doesn't exist or isn't writable", exit 1). All of these
/// exited 0 with the file unchanged at 8991992e and tip 8825f101.
#[test]
fn rw2_outer_panasonicraw_ifd0_edits_are_never_reported_done_unchanged() {
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let no_jpeg = without_jpg_from_raw(&original);
    for (label, file) in [("Panasonic.rw2", &original), ("no JpgFromRaw", &no_jpeg)] {
        for arg in [
            "-IFD0:ISO=",
            "-ifd0:iso=",
            "-IFD0:LinearityLimitRed=",
            "-IFD0:WBRedLevel=",
            "-IFD0:RawFormat=",
            "-IFD0:PanasonicRawVersion=",
            "-LinearityLimitRed=",
            "-EXIF:LinearityLimitRed=",
            "-LinearityLimitRed=4000",
            "-IFD0:LinearityLimitRed=4000",
        ] {
            assert_cli_refused(dir.path(), file, label, arg, &["PanasonicRaw"]);
        }
        assert_cli_refused(
            dir.path(),
            file,
            label,
            "-IFD0:SensorWidth=",
            &["not writable"],
        );
        let path = write(dir.path(), "lib.rw2", file);
        let err = remove_tag(&path, "IFD0:ISO").expect_err("remove IFD0:ISO");
        assert!(err.to_string().contains("PanasonicRaw"), "{label}: {err}");
        assert!(std::fs::read(&path).unwrap() == *file, "{label}: touched");
    }
    // With no embedded document the outer IFD0 is all ExifTool edits:
    // deleting `Make` or `ISO` by any spelling deletes the outer entry.
    for arg in ["-Make=", "-EXIF:Make=", "-ISO="] {
        assert_cli_refused(
            dir.path(),
            &no_jpeg,
            "no JpgFromRaw",
            arg,
            &["PanasonicRaw"],
        );
    }
}

/// With no JpgFromRaw, pinned ExifTool 13.59 has nowhere to put an
/// `Exif::Main` tag the outer IFD0 does not hold (`Artist` and `Copyright`
/// are `Permanent` there, `Software` is not in the table): "0 image files
/// updated". 8991992e wrote each to the outer IFD0 and reported it done.
/// Refused by name instead, file untouched.
#[test]
fn rw2_ifd0_sets_exiftool_has_nowhere_to_make_are_refused() {
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let no_jpeg = without_jpg_from_raw(&original);
    for arg in ["-IFD0:Artist=x", "-IFD0:Copyright=c", "-IFD0:Software=x"] {
        assert_cli_refused(
            dir.path(),
            &no_jpeg,
            "no JpgFromRaw",
            arg,
            &["PanasonicRaw"],
        );
    }
}

/// What pinned ExifTool 13.59 edits in the outer IFD0 alone, or does not
/// edit at all, this writer still does exactly: no refusal where the
/// oracle succeeds.
///
/// - `IFD0:Make=Acme` with no JpgFromRaw: the outer Make is written.
/// - `IFD0:Make=Acmesonic` when the embedded Make already is Acmesonic:
///   the outer Make is written, the embedded document is untouched.
/// - `IFD0:Artist=` / `IFD0:Gamma=` (in neither directory) and
///   `IFD0:Make=Panasonic` (both already Panasonic): no-ops, exit 0, the
///   file unchanged ("1 image files unchanged" / an identical read-back).
#[test]
fn rw2_ifd0_edits_exiftool_makes_outside_the_jpgfromraw_stay_exact() {
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for arg in ["-IFD0:Artist=", "-IFD0:Gamma=", "-IFD0:Make=Panasonic"] {
        let (code, text, after) = cli(dir.path(), &original, arg);
        assert_eq!(code, Some(0), "{arg}: {text}");
        assert!(after == original, "{arg}: file changed");
    }
    let no_jpeg = without_jpg_from_raw(&original);
    let split = with_embedded_make(&original, b"Acmesonic\0");
    for (label, file, make) in [
        ("no JpgFromRaw", &no_jpeg, "Acme"),
        ("embedded Make already set", &split, "Acmesonic"),
    ] {
        let arg = format!("-IFD0:Make={make}");
        let (code, text, after) = cli(dir.path(), file, &arg);
        assert_eq!(code, Some(0), "{label}: {text}");
        let path = write(dir.path(), "out.rw2", &after);
        assert_eq!(
            read_metadata(&path).unwrap().get_string("IFD0:Make"),
            Some(make),
            "{label}"
        );
        if label == "embedded Make already set" {
            let (start, len) = jpg_from_raw(file);
            let (start2, len2) = jpg_from_raw(&after);
            assert_eq!(
                &after[start2..start2 + len2],
                &file[start..start + len],
                "{label}: JpgFromRaw changed"
            );
        }
        assert_oracle_parity(file, &after, &arg, label);
    }
}

/// Read-back rows of pinned ExifTool 13.59 for `path` (`-a -G3:1 -s`), less
/// the file-system rows and the ones that are positions in the file, which
/// ExifTool's full re-layout moves and an in-place edit does not.
fn oracle_rows(path: &Path) -> Vec<String> {
    let oracle = exiftool_oracle::shared().unwrap();
    let out = oracle
        .command()
        .args(["-a", "-G3:1", "-s", "-all", "-Warning"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| {
            ![
                "[System]",
                "[File]",
                "[ExifTool]",
                "[Composite]",
                "[Doc1:File]",
            ]
            .iter()
            .any(|group| line.starts_with(group))
                && !["RawDataOffset", "ThumbnailOffset", " JpgFromRaw "]
                    .iter()
                    .any(|name| line.contains(name))
        })
        .map(str::to_string)
        .collect()
}

/// `ours` (oxidex's edit of `original` by `arg`) reads back, under pinned
/// ExifTool 13.59, as the oracle's own edit of `original` does. Skipped
/// when no usable oracle is resolved.
fn assert_oracle_parity(original: &[u8], ours: &[u8], arg: &str, label: &str) {
    if !exiftool_oracle::available() || exiftool_oracle::shared().is_err() {
        eprintln!("skipping oracle parity ({label}): no usable ExifTool oracle");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let reference = write(dir.path(), "oracle.rw2", original);
    let status = exiftool_oracle::shared()
        .unwrap()
        .command()
        .args(["-q", "-q", "-overwrite_original", arg])
        .arg(&reference)
        .status()
        .unwrap();
    assert!(status.success(), "{label}: oracle {arg} failed");
    let ours = write(dir.path(), "ours.rw2", ours);
    assert_eq!(
        oracle_rows(&ours),
        oracle_rows(&reference),
        "{label}: {arg} reads back unlike the oracle's edit"
    );
}
