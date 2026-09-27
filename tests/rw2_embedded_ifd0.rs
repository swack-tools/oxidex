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

/// t/images Panasonic.rw2 with an ISO of 80 in the JpgFromRaw's own IFD0
/// too (Exif 0x8827, SHORT): the ExifIFD pointer record moves into the
/// YCbCrPositioning slot and ISO takes the pointer's, so the records stay in
/// order and the IFD keeps its size (YCbCrPositioning is dropped). Its
/// ExifIFD still holds ISO 80.
fn with_embedded_ifd0_iso(original: &[u8]) -> Vec<u8> {
    let mut b = original.to_vec();
    let tiff = embedded_tiff(&b);
    let records = ifd0_records(&b, tiff);
    let slot = |tag: u16| records.iter().find(|(_, t)| *t == tag).unwrap().0;
    let (ycbcr, exif) = (slot(0x0213), slot(0x8769));
    assert_eq!(exif, ycbcr + 12);
    let pointer = b[exif..exif + 12].to_vec();
    b[ycbcr..ycbcr + 12].copy_from_slice(&pointer);
    let mut iso = Vec::new();
    iso.extend(0x8827u16.to_le_bytes());
    iso.extend(3u16.to_le_bytes());
    iso.extend(1u32.to_le_bytes());
    iso.extend(80u32.to_le_bytes());
    b[exif..exif + 12].copy_from_slice(&iso);
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
            &["doesn't exist or isn't writable"],
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
/// updated" / "1 image files unchanged", exit 0. 8991992e wrote each to the
/// outer IFD0 and reported it done; #956 refused it by name, since no write
/// path could then report "unchanged" truthfully. The roll-up's one write
/// transaction (#951) can, so it now answers as ExifTool does
/// (`rw2_ifd0::rw2_set_is_no_op`), under every spelling (bare, `EXIF:`,
/// `IFD0:`; roll-up evidence `rw2-bare-names-oracle.txt`).
#[test]
fn rw2_ifd0_sets_exiftool_has_nowhere_to_make_are_unchanged() {
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let no_jpeg = without_jpg_from_raw(&original);
    for name in ["Artist=x", "Copyright=c", "Software=x"] {
        for arg in [
            format!("-IFD0:{name}"),
            format!("-EXIF:{name}"),
            format!("-{name}"),
        ] {
            let (code, text, after) = cli(dir.path(), &no_jpeg, &arg);
            assert_eq!(code, Some(0), "no JpgFromRaw {arg}: {text}");
            assert_eq!(
                text, "    0 image files updated\n    1 image files unchanged\n",
                "no JpgFromRaw {arg}"
            );
            assert!(after == no_jpeg, "no JpgFromRaw {arg}: file changed");
        }
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
fn oracle_rows(oracle: &exiftool_oracle::Oracle, path: &Path) -> Vec<String> {
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
/// (loudly, and a failure under `OXIDEX_REQUIRE_EXIFTOOL`) when no oracle may
/// grade (`exiftool_oracle::graded`).
fn assert_oracle_parity(original: &[u8], ours: &[u8], arg: &str, label: &str) {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping oracle parity ({label}): no grading ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let reference = write(dir.path(), "oracle.rw2", original);
    let status = oracle
        .command()
        .args(["-q", "-q", "-overwrite_original", arg])
        .arg(&reference)
        .status()
        .unwrap();
    assert!(status.success(), "{label}: oracle {arg} failed");
    let ours = write(dir.path(), "ours.rw2", ours);
    assert_eq!(
        oracle_rows(oracle, &ours),
        oracle_rows(oracle, &reference),
        "{label}: {arg} reads back unlike the oracle's edit"
    );
}

/// A removal or set of a `PanasonicRaw::Main` tag no writable table of the
/// group declares (`SensorWidth`) is refused under `EXIF:` as under `IFD0:`:
/// pinned ExifTool 13.59 answers "Sorry, EXIF:SensorWidth doesn't exist or
/// isn't writable" / "Nothing to do.", exit 1, file unchanged -- and so,
/// word for word, does the roll-up's resolver (`rw2_ifd0::route_rw2_name`). de92d7c4 checked only the
/// `IFD0:` spelling and exited 0 for `-EXIF:SensorWidth=` (review of #956).
/// A bare `-SensorWidth=` reaches every module's tables, and there the
/// oracle exits 0 with the file unchanged ("0 image files updated"), so
/// it stays a no-op success.
#[test]
fn rw2_non_writable_panasonicraw_tags_are_refused_under_every_group_spelling() {
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let no_jpeg = without_jpg_from_raw(&original);
    for (label, file) in [("Panasonic.rw2", &original), ("no JpgFromRaw", &no_jpeg)] {
        for (arg, key) in [
            ("-EXIF:SensorWidth=", "EXIF:SensorWidth"),
            ("-exif:sensorwidth=", "EXIF:SensorWidth"),
            ("-IFD0:SensorWidth=", "IFD0:SensorWidth"),
            ("-EXIF:SensorWidth=5", "EXIF:SensorWidth"),
            ("-IFD0:SensorWidth=5", "IFD0:SensorWidth"),
        ] {
            // ExifTool's own warning (it echoes the group as typed --
            // `exif:SensorWidth` -- where oxidex's CLI has canonicalized it).
            let (code, text, after) = cli(dir.path(), file, arg);
            assert_eq!(code, Some(1), "{label} {arg}: {text}");
            assert_eq!(
                text,
                format!("Warning: Sorry, {key} doesn't exist or isn't writable\nNothing to do.\n"),
                "{label} {arg}"
            );
            assert!(after == *file, "{label} {arg}: file touched");
        }
        for arg in ["-SensorWidth=", "-sensorwidth="] {
            let (code, text, after) = cli(dir.path(), file, arg);
            assert_eq!(code, Some(0), "{label} {arg}: {text}");
            assert!(after == *file, "{label} {arg}: file changed");
        }
    }
}

/// An explicit `IFD0:` set to the value the reader already reports is
/// checked against every destination pinned ExifTool 13.59 writes, not only
/// the embedded IFD0 (review of #956). `-IFD0:ISO=80` over an outer 0x0017
/// of 80 still adds outer 0x0037 = 80 (`PanasonicRaw::Main` declares both
/// writable); where the JpgFromRaw's IFD0 already holds ISO 80 ExifTool also
/// removes its ExifIFD copy. de92d7c4 reported both done with the file
/// unchanged; refused by name now. `-IFD0:Make=Panasonic`, which every
/// directory already holds, stays a no-op that reads back as the oracle's.
#[test]
fn rw2_same_value_ifd0_sets_check_every_outer_destination() {
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let no_jpeg = without_jpg_from_raw(&original);
    let embedded_iso = with_embedded_ifd0_iso(&original);
    for (label, file) in [
        ("no JpgFromRaw", &no_jpeg),
        ("embedded IFD0 ISO 80", &embedded_iso),
        ("Panasonic.rw2", &original),
    ] {
        for arg in ["-IFD0:ISO=80", "-ifd0:iso=80"] {
            assert_cli_refused(dir.path(), file, label, arg, &["PanasonicRaw"]);
        }
        let path = write(dir.path(), "lib.rw2", file);
        let err = modify_tag(&path, "IFD0:ISO", TagValue::new_integer(80))
            .expect_err("IFD0:ISO=80 reported done");
        assert!(err.to_string().contains("PanasonicRaw"), "{label}: {err}");
        assert!(std::fs::read(&path).unwrap() == *file, "{label}: touched");
    }
    for (label, file) in [("no JpgFromRaw", &no_jpeg), ("Panasonic.rw2", &original)] {
        let arg = "-IFD0:Make=Panasonic";
        let (code, text, after) = cli(dir.path(), file, arg);
        assert_eq!(code, Some(0), "{label} {arg}: {text}");
        assert!(after == *file, "{label} {arg}: file changed");
        assert_oracle_parity(file, &after, arg, label);
    }
}

/// Bare (ungrouped) names on a Panasonic RAW, and their `EXIF:` and `IFD0:`
/// spellings, graded against pinned ExifTool 13.59 case by case (#945's
/// resolver refused every ungrouped name where IFD0 is not `Exif::Main`
/// before #956's checks could run; the roll-up routes them through those
/// checks, `rw2_ifd0::route_rw2_name`). Per request:
///
/// - the oracle refuses (exit 1): so does oxidex, file untouched, with the
///   same warning lines;
/// - the oracle leaves the file unchanged (exit 0): oxidex reports "1 image
///   files unchanged", exit 0, file untouched;
/// - the oracle edits the file: oxidex refuses by name (an edit it cannot
///   make: a deletion from the outer IFD0, a JpgFromRaw edit, a PanasonicRaw
///   tag it does not write -- the message names PanasonicRaw), file
///   untouched, or it reports "updated" and reads back as the oracle's edit.
///
/// The same outcomes are recorded, with the exact oracle output, in the
/// roll-up evidence `rw2-bare-names-oracle.txt`.
#[test]
fn rw2_bare_names_answer_as_the_oracle_does() {
    let Some(original) = sample() else {
        return;
    };
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no grading ExifTool oracle (pinned -ver + DOCX probe)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let no_jpeg = without_jpg_from_raw(&original);
    let names = [
        "SensorWidth=",
        "sensorwidth=",
        "SensorWidth=5",
        "LinearityLimitRed=",
        "LinearityLimitRed=4000",
        "ISO=",
        "ISO=100",
        "iso=",
        "Make=",
        "Make=Acme",
        "Make=Panasonic",
        "Model=M",
        "Artist=x",
        "Artist=",
        "Copyright=c",
        "Software=x",
        "ImageDescription=d",
        "XResolution=300",
        "WBRedLevel=",
        "Gamma=",
        "RawFormat=",
        "PanasonicRawVersion=",
        "Orientation=",
        "NoSuchTag=",
    ];
    let warnings = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with("Warning"))
            .map(str::to_string)
            .collect()
    };
    let mut failures = Vec::new();
    for (label, file) in [("Panasonic.rw2", &original), ("no JpgFromRaw", &no_jpeg)] {
        for name in names {
            for arg in [
                format!("-{name}"),
                format!("-EXIF:{name}"),
                format!("-IFD0:{name}"),
            ] {
                let reference = write(dir.path(), "oracle.rw2", file);
                let theirs = oracle
                    .command()
                    .args(["-overwrite_original", &arg])
                    .arg(&reference)
                    .output()
                    .unwrap();
                let their_text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&theirs.stdout),
                    String::from_utf8_lossy(&theirs.stderr)
                );
                let oracle_changed = std::fs::read(&reference).unwrap() != *file;
                let path = write(dir.path(), "ours.rw2", file);
                let ours = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
                    .arg(&arg)
                    .arg(&path)
                    .output()
                    .unwrap();
                let our_text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&ours.stdout),
                    String::from_utf8_lossy(&ours.stderr)
                );
                let untouched = std::fs::read(&path).unwrap() == *file;
                let ok = if theirs.status.code() != Some(0) {
                    ours.status.code() == theirs.status.code()
                        && untouched
                        && warnings(&our_text) == warnings(&their_text)
                } else if !oracle_changed {
                    ours.status.code() == Some(0)
                        && untouched
                        && our_text.contains("1 image files unchanged")
                } else {
                    (ours.status.code() == Some(1)
                        && untouched
                        && our_text.contains("PanasonicRaw"))
                        || (ours.status.code() == Some(0)
                            && our_text.contains("1 image files updated")
                            && oracle_rows(oracle, &path) == oracle_rows(oracle, &reference))
                };
                if !ok {
                    failures.push(format!(
                        "{label} {arg}: oracle exit {:?} changed {oracle_changed} {:?}; \
                         oxidex exit {:?} untouched {untouched} {:?}",
                        theirs.status.code(),
                        their_text.trim(),
                        ours.status.code(),
                        our_text.trim()
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The records of the IFD1 chained after the IFD0 of a little-endian TIFF
/// starting at `tiff`, with their tag ids.
fn ifd1_records(b: &[u8], tiff: usize) -> Vec<(usize, u16)> {
    let ifd0 = tiff + u32_le(b, tiff + 4) as usize;
    let next = ifd0 + 2 + 12 * usize::from(u16_le(b, ifd0));
    let ifd1 = tiff + u32_le(b, next) as usize;
    (0..usize::from(u16_le(b, ifd1)))
        .map(|i| ifd1 + 2 + 12 * i)
        .map(|at| (at, u16_le(b, at)))
        .collect()
}

/// [`with_embedded_make`] "Acmesonic", plus a Make in the JpgFromRaw's IFD1:
/// its Compression record (0x0103, the IFD's first) renumbered 0x010f and
/// made the inline ASCII "Acm", so the records stay in order.
fn with_embedded_ifd1_make(original: &[u8]) -> Vec<u8> {
    let mut b = with_embedded_make(original, b"Acmesonic\0");
    let tiff = embedded_tiff(&b);
    let (at, _) = ifd1_records(&b, tiff)
        .into_iter()
        .find(|(_, tag)| *tag == 0x0103)
        .unwrap();
    b[at..at + 2].copy_from_slice(&0x010fu16.to_le_bytes());
    b[at + 2..at + 4].copy_from_slice(&2u16.to_le_bytes());
    b[at + 4..at + 8].copy_from_slice(&4u32.to_le_bytes());
    b[at + 8..at + 12].copy_from_slice(b"Acm\0");
    b
}

/// Review of #956 (rw2_ifd0.rs:198): the only copy of a tag pinned ExifTool
/// 13.59 moves when it writes `IFD0:<tag>` is an ExifIFD one
/// (WriteExif.pl 13.59:20-23 `%crossDelete = (ExifIFD => 'IFD0', IFD0 =>
/// 'ExifIFD')`, applied at :1156-1171). A same-ID entry in the JpgFromRaw's
/// IFD1 is left alone, so it is no reason to refuse:
///
/// - `IFD0:Make=Acmesonic` where the embedded IFD0 already holds Acmesonic
///   and the embedded IFD1 holds a Make too: ExifTool rewrites the outer
///   Make and leaves `Doc1:IFD1` Make as it was (`-v2`: no IFD1 change);
/// - `IFD0:XResolution=180` on t/images Panasonic.rw2, whose JpgFromRaw
///   holds XResolution 180 in IFD0 and in IFD1, and whose outer IFD0 has
///   none: ExifTool changes no value.
///
/// Both were refused at e4d2d79a ("writes it into the IFD0 of the embedded
/// JpgFromRaw"). The ExifIFD case stays refused
/// (`rw2_same_value_ifd0_sets_check_every_outer_destination`).
#[test]
fn rw2_ifd0_sets_ignore_embedded_copies_exiftool_does_not_move() {
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let ifd1_make = with_embedded_ifd1_make(&original);
    let arg = "-IFD0:Make=Acmesonic";
    let (code, text, after) = cli(dir.path(), &ifd1_make, arg);
    assert_eq!(code, Some(0), "IFD1 Make {arg}: {text}");
    assert!(text.contains("1 image files updated"), "{text}");
    let (start, len) = jpg_from_raw(&ifd1_make);
    let (start2, len2) = jpg_from_raw(&after);
    assert_eq!(
        &after[start2..start2 + len2],
        &ifd1_make[start..start + len],
        "IFD1 Make: JpgFromRaw changed"
    );
    assert_oracle_parity(&ifd1_make, &after, arg, "IFD1 Make");
    let path = write(dir.path(), "lib.rw2", &ifd1_make);
    modify_tag(&path, "IFD0:Make", TagValue::new_string("Acmesonic")).expect("IFD1 Make, library");
    assert_eq!(
        read_metadata(&path).unwrap().get_string("IFD0:Make"),
        Some("Acmesonic")
    );

    let arg = "-IFD0:XResolution=180";
    let (code, text, after) = cli(dir.path(), &original, arg);
    assert_eq!(code, Some(0), "Panasonic.rw2 {arg}: {text}");
    assert!(after == original, "Panasonic.rw2 {arg}: file changed");
    assert_oracle_parity(&original, &after, arg, "Panasonic.rw2");
}

/// Review of #956 (rw2_ifd0.rs:265): on an RW2 with no JpgFromRaw and no
/// outer Artist or Copyright (both `Permanent` in `PanasonicRaw::Main`,
/// PanasonicRaw.pm 13.59:307-313, :337-346), pinned ExifTool 13.59 creates
/// neither under any spelling: "0 image files updated", file byte-identical.
/// Every library entry point answers the same, under the bare, `EXIF:` and
/// `IFD0:` spellings -- the transaction resolves the first two to `IFD0:`
/// (`rw2_ifd0::route_rw2_name`) before any destination check, so the
/// family/bare branch of `check_set` never sees them. Did not reproduce at
/// e4d2d79a (roll-up evidence `rollup-fixes/`); pinned here, oracle-graded.
#[test]
fn rw2_permanent_tags_the_camera_omitted_are_never_created() {
    use oxidex::core::operations::write_metadata;
    use oxidex::core::write_transaction::WriteOutcome;
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let no_jpeg = without_jpg_from_raw(&original);
    let outer_ids =
        |b: &[u8]| -> Vec<u16> { ifd0_records(b, 0).into_iter().map(|(_, t)| t).collect() };
    assert!(!outer_ids(&no_jpeg).contains(&0x013b));
    assert!(!outer_ids(&no_jpeg).contains(&0x8298));
    let oracle = exiftool_oracle::graded();
    for (name, value) in [("Artist", "x"), ("Copyright", "c")] {
        for key in [
            name.to_string(),
            format!("EXIF:{name}"),
            format!("IFD0:{name}"),
        ] {
            if let Some(oracle) = oracle {
                let reference = write(dir.path(), "oracle.rw2", &no_jpeg);
                let status = oracle
                    .command()
                    .args([
                        "-q",
                        "-q",
                        "-overwrite_original",
                        &format!("-{key}={value}"),
                    ])
                    .arg(&reference)
                    .status()
                    .unwrap();
                assert!(status.success(), "oracle -{key}={value}");
                assert!(
                    std::fs::read(&reference).unwrap() == no_jpeg,
                    "the oracle changed the file for -{key}={value}"
                );
            }
            let path = write(dir.path(), "lib.rw2", &no_jpeg);
            let outcome = modify_tag(&path, &key, TagValue::new_string(value))
                .unwrap_or_else(|err| panic!("modify_tag {key}: {err}"));
            assert_eq!(outcome, WriteOutcome::Unchanged, "modify_tag {key}");
            assert!(
                std::fs::read(&path).unwrap() == no_jpeg,
                "modify_tag {key}: file changed"
            );

            let mut map = read_metadata(&path).unwrap();
            map.insert(key.clone(), TagValue::new_string(value));
            let outcome = write_metadata(&path, &map)
                .unwrap_or_else(|err| panic!("write_metadata {key}: {err}"));
            assert_eq!(outcome, WriteOutcome::Unchanged, "write_metadata {key}");
            assert!(
                std::fs::read(&path).unwrap() == no_jpeg,
                "write_metadata {key}: file changed"
            );
        }
    }
    if oracle.is_none() {
        eprintln!("skipping oracle grading: no grading ExifTool oracle");
    }
}

/// Review of #956 (rw2_ifd0.rs:539): an explicit library set of the value the
/// reader reports, print-converted (`IFD0:ResolutionUnit` "inches" for the
/// JpgFromRaw's SHORT 2; `IFD0:YCbCrPositioning` "Co-sited" for its SHORT
/// 2), is compared through the tag's inverse PrintConv -- ExifTool's
/// `ReverseLookup` (Writer.pl 13.59:3609-3650) of the `Exif::Main` hash --
/// not as ASCII text. Neither is in the outer `PanasonicRaw::Main` IFD0. Pinned
/// ExifTool 13.59 rewrites each entry with its own value ("- IFD0:
/// ResolutionUnit = '2' / + ... '2'"), a read-back identical to the file's;
/// the library reported "writes it into the IFD0 of the embedded
/// JpgFromRaw" and refused at e4d2d79a.
#[test]
fn rw2_same_value_print_converted_sets_compare_through_the_inverse() {
    use oxidex::core::operations::write_metadata;
    use oxidex::core::write_transaction::WriteOutcome;
    let Some(original) = sample() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let baseline = {
        let path = write(dir.path(), "read.rw2", &original);
        read_metadata(&path).unwrap()
    };
    for (key, printed) in [
        ("IFD0:ResolutionUnit", "inches"),
        ("IFD0:YCbCrPositioning", "Co-sited"),
    ] {
        let value = baseline.get(key).cloned().expect(key);
        assert_eq!(
            value,
            TagValue::new_string(printed),
            "{key} as the reader reports it"
        );
        let path = write(dir.path(), "lib.rw2", &original);
        let outcome = modify_tag(&path, key, value.clone())
            .unwrap_or_else(|err| panic!("modify_tag {key}={printed}: {err}"));
        assert_eq!(outcome, WriteOutcome::Unchanged, "modify_tag {key}");
        assert!(
            std::fs::read(&path).unwrap() == original,
            "{key}: file changed"
        );

        let mut map = read_metadata(&path).unwrap();
        map.insert(key, value);
        let outcome = write_metadata(&path, &map)
            .unwrap_or_else(|err| panic!("write_metadata {key}={printed}: {err}"));
        assert_eq!(outcome, WriteOutcome::Unchanged, "write_metadata {key}");
        assert!(
            std::fs::read(&path).unwrap() == original,
            "{key}: file changed"
        );
        assert_oracle_parity(&original, &original, &format!("-{key}={printed}"), key);
    }
    // A label the entry does not hold is still a change ExifTool makes in
    // the JpgFromRaw: refused by name, file untouched.
    let path = write(dir.path(), "lib.rw2", &original);
    let err = modify_tag(&path, "IFD0:ResolutionUnit", TagValue::new_string("cm"))
        .expect_err("ResolutionUnit=cm reported done");
    assert!(err.to_string().contains("JpgFromRaw"), "{err}");
    assert!(std::fs::read(&path).unwrap() == original);
}
