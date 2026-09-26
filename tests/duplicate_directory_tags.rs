//! One `Exif::Main` tag stored in two EXIF directories keeps both rows.
//!
//! ExifTool's `FoundTag` (ExifTool.pm 13.59:9536-9591) never drops a second
//! occurrence of a tag: it files it under a new key, and `-a -G1` prints
//! every copy under its own family-1 group. Priority and the order the
//! occurrences were found only decide which copy a bare `-TAG`, plain `-s`
//! or `-j` without `-G` shows. The reader used to discard an ExifIFD (and an
//! image-carrying InteropIFD) row whenever IFD0 held the same name, so a
//! file with `IFD0:CreateDate` beside `ExifIFD:CreateDate` read as though
//! the ExifIFD copy did not exist: `-a -G1` missed it, and
//! `-ExifIFD:CreateDate` printed nothing.
//!
//! Every file below is written by the pinned oracle itself from ExifTool's
//! own `t/images` (both copies in one command: a group-qualified write of a
//! tag that exists elsewhere otherwise *moves* it), and every expectation is
//! the oracle's own reading of that file. Graded only by
//! [`exiftool_oracle::graded`] (pinned `-ver` and the `OOXML.docx` probe).
//!
//! The cases cover the three ways ExifTool arbitrates the default copy:
//!
//! * file order at equal priority: `ExifIFD` is processed inline at IFD0's
//!   0x8769 entry, so an IFD0 twin stored *after* that entry (0x9003
//!   DateTimeOriginal, 0x9004 CreateDate) is found later and wins, while one
//!   stored before it (0x010f Make, 0x0132 ModifyDate) loses to the ExifIFD
//!   copy;
//! * `Priority => 0` (0x010e ImageDescription, 0x011a XResolution, ...):
//!   the first copy found, IFD0's, is kept against ExifIFD and InteropIFD;
//! * `LOW_PRIORITY_DIR` IFD1 in a JPEG (ExifTool.pm:7317): IFD0 wins.

use oxidex::exiftool_oracle;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Case {
    label: &'static str,
    /// A file in the pinned tree's `t/images`.
    source: &'static str,
    /// The oracle's write arguments that give the file its duplicate rows.
    writes: &'static [&'static str],
    /// The names compared, with and without `-a`.
    tags: &'static [&'static str],
    /// `(group1, name)` rows the fixture must carry, per the oracle, so a
    /// write that silently did nothing cannot pass as agreement.
    must_have: &'static [(&'static str, &'static str)],
}

const DATES: &[&str] = &["Make", "ModifyDate", "DateTimeOriginal", "CreateDate"];

const CASES: &[Case] = &[
    Case {
        label: "IFD0 CreateDate after the ExifIFD pointer (the reported file)",
        source: "Canon.jpg",
        writes: &[
            "-IFD0:CreateDate=2020:01:02 03:04:05",
            "-ExifIFD:CreateDate=2003:12:04 06:46:52",
        ],
        tags: DATES,
        must_have: &[("IFD0", "CreateDate"), ("ExifIFD", "CreateDate")],
    },
    Case {
        label: "IFD0 twins before the ExifIFD pointer: the ExifIFD copy wins",
        source: "Canon.jpg",
        writes: &[
            "-IFD0:ModifyDate=2003:12:04 06:46:52",
            "-ExifIFD:ModifyDate=2021:05:06 07:08:09",
            "-IFD0:Make=Canon",
            "-ExifIFD:Make=ExifMake",
        ],
        tags: DATES,
        must_have: &[
            ("IFD0", "ModifyDate"),
            ("ExifIFD", "ModifyDate"),
            ("IFD0", "Make"),
            ("ExifIFD", "Make"),
        ],
    },
    Case {
        label: "IFD0 DateTimeOriginal after the ExifIFD pointer",
        source: "Canon.jpg",
        writes: &[
            "-IFD0:DateTimeOriginal=2019:01:01 00:00:00",
            "-ExifIFD:DateTimeOriginal=2003:12:04 06:46:52",
        ],
        tags: DATES,
        must_have: &[
            ("IFD0", "DateTimeOriginal"),
            ("ExifIFD", "DateTimeOriginal"),
        ],
    },
    Case {
        label: "Priority 0 tags in IFD0, ExifIFD and InteropIFD",
        source: "Canon.jpg",
        writes: &[
            "-IFD0:ImageDescription=IFD0Desc",
            "-ExifIFD:ImageDescription=ExifDesc",
            "-IFD0:XResolution=180",
            "-ExifIFD:XResolution=300",
            "-InteropIFD:XResolution=72",
            "-IFD0:YResolution=180",
            "-InteropIFD:YResolution=400",
            "-IFD0:ResolutionUnit=inches",
            "-InteropIFD:ResolutionUnit=cm",
        ],
        tags: &[
            "ImageDescription",
            "XResolution",
            "YResolution",
            "ResolutionUnit",
        ],
        must_have: &[
            ("ExifIFD", "ImageDescription"),
            ("ExifIFD", "XResolution"),
            ("InteropIFD", "XResolution"),
            ("InteropIFD", "YResolution"),
            ("InteropIFD", "ResolutionUnit"),
        ],
    },
    Case {
        label: "JPEG IFD1 twins (a LOW_PRIORITY_DIR)",
        source: "Canon.jpg",
        writes: &[
            "-IFD1:Make=IFD1Make",
            "-IFD1:ImageDescription=IFD1Desc",
            "-IFD0:ImageDescription=IFD0Desc",
            "-IFD1:ModifyDate=2022:02:02 02:02:02",
        ],
        tags: &["Make", "ImageDescription", "ModifyDate"],
        must_have: &[
            ("IFD1", "Make"),
            ("IFD1", "ImageDescription"),
            ("IFD1", "ModifyDate"),
        ],
    },
    Case {
        label: "TIFF IFD0 and ExifIFD twins",
        source: "ExifTool.tif",
        writes: &[
            "-IFD0:ModifyDate=2004:02:20 08:07:49",
            "-ExifIFD:ModifyDate=2021:05:06 07:08:09",
            "-IFD0:ImageDescription=The picture caption",
            "-ExifIFD:ImageDescription=ExifDesc",
            "-IFD0:CreateDate=2020:01:02 03:04:05",
            "-ExifIFD:CreateDate=2003:12:04 06:46:52",
        ],
        tags: &["ImageDescription", "ModifyDate", "CreateDate"],
        must_have: &[
            ("ExifIFD", "ModifyDate"),
            ("ExifIFD", "ImageDescription"),
            ("ExifIFD", "CreateDate"),
            ("IFD0", "CreateDate"),
        ],
    },
];

fn run(mut command: Command, args: &[String], file: &Path) -> String {
    let out = command.args(args).arg(file).output().unwrap();
    assert!(
        out.status.success(),
        "{args:?} {}: {}",
        file.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn oxidex() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
}

fn tag_args(prefix: &[&str], tags: &[&str]) -> Vec<String> {
    prefix
        .iter()
        .map(|arg| arg.to_string())
        .chain(tags.iter().map(|tag| format!("-{tag}")))
        .collect()
}

/// `-a -G1 -s` rows as a set: every copy, compared without ordering.
fn rows(text: &str) -> BTreeSet<String> {
    text.lines().map(str::to_string).collect()
}

/// The `-j` rows for `tags` as `(name, value)`, `SourceFile` aside. oxidex
/// keys a row `Group:Name` even without `-G`, so the group is dropped: what
/// is graded is which copy `-j` chose, not the key's spelling.
fn json_rows(text: &str, tags: &[&str]) -> Vec<(String, Value)> {
    let parsed: Value = serde_json::from_str(text).unwrap();
    let mut rows: Vec<(String, Value)> = parsed[0]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| {
            let name = key.split_once(':').map_or(key.as_str(), |(_, name)| name);
            (name.to_string(), value.clone())
        })
        .filter(|(name, _)| tags.contains(&name.as_str()))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

fn fixture(oracle: &exiftool_oracle::Oracle, images: &Path, dir: &Path, case: &Case) -> PathBuf {
    let path = dir.join(format!(
        "{}.{}",
        case.label
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>(),
        Path::new(case.source)
            .extension()
            .unwrap()
            .to_string_lossy()
    ));
    std::fs::copy(images.join(case.source), &path).unwrap();
    let status = oracle
        .command()
        .args(["-q", "-q", "-overwrite_original"])
        .args(case.writes)
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success(), "{}: oracle write failed", case.label);
    path
}

/// Grades `file` against the oracle: every `-a -G1` row of `tags`, the
/// default copy (`-s`, `-j`, `-s3 -<Name>`) and each `must_have` copy by its
/// own group (`-s3 -<Group>:<Name>`), which the oracle must itself report.
fn grade(
    oracle: &exiftool_oracle::Oracle,
    file: &Path,
    label: &str,
    tags: &[&str],
    must_have: &[(&str, &str)],
    failures: &mut Vec<String>,
) {
    let file = file.to_path_buf();
    // Every copy: `-a -G1 -s`.
    let all = tag_args(&["-a", "-G1", "-s"], tags);
    let theirs = rows(&run(oracle.command(), &all, &file));
    for (group, name) in must_have {
        assert!(
            theirs
                .iter()
                .any(|row| row.starts_with(&format!("[{group}]"))
                    && row.split_whitespace().nth(1) == Some(name)),
            "{}: the oracle's fixture lacks [{group}] {name}: {theirs:?}",
            label
        );
    }
    let ours = rows(&run(oxidex(), &all, &file));
    if ours != theirs {
        failures.push(format!(
            "{} -a -G1: missing {:?}, extra {:?}",
            label,
            theirs.difference(&ours).collect::<Vec<_>>(),
            ours.difference(&theirs).collect::<Vec<_>>()
        ));
    }

    // The default copy: plain `-s`, `-j` and `-s3 -<Name>`.
    let plain = tag_args(&["-s"], tags);
    let theirs = rows(&run(oracle.command(), &plain, &file));
    let ours = rows(&run(oxidex(), &plain, &file));
    if ours != theirs {
        failures.push(format!(
            "{} -s: oracle {:?}, oxidex {:?}",
            label,
            theirs.difference(&ours).collect::<Vec<_>>(),
            ours.difference(&theirs).collect::<Vec<_>>()
        ));
    }
    // `-j` without `-G`: one row per name, the default copy.
    let json = tag_args(&["-j"], tags);
    let theirs = json_rows(&run(oracle.command(), &json, &file), tags);
    let ours = json_rows(&run(oxidex(), &json, &file), tags);
    if ours != theirs {
        failures.push(format!("{} -j: oracle {theirs:?}, oxidex {ours:?}", label));
    }

    // The default copy of each name alone: `-s3 -<Name>`.
    for name in tags {
        let bare = vec!["-s3".to_string(), format!("-{name}")];
        let theirs = run(oracle.command(), &bare, &file);
        let ours = run(oxidex(), &bare, &file);
        if ours != theirs {
            failures.push(format!(
                "{} -s3 -{name}: oracle {theirs:?}, oxidex {ours:?}",
                label
            ));
        }
    }

    // Each copy by its own group: `-s3 -<Group>:<Name>`.
    for (group, name) in must_have {
        let qualified = vec!["-s3".to_string(), format!("-{group}:{name}")];
        let theirs = run(oracle.command(), &qualified, &file);
        let ours = run(oxidex(), &qualified, &file);
        if ours != theirs {
            failures.push(format!(
                "{} -s3 -{group}:{name}: oracle {theirs:?}, oxidex {ours:?}",
                label
            ));
        }
    }
}

#[test]
fn same_tag_in_two_directories_keeps_both_rows_and_exiftools_default() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping duplicate-directory parity: no ExifTool oracle may grade output");
        return;
    };
    let images = exiftool_oracle::capability_sample(oracle)
        .and_then(|docx| docx.parent().map(Path::to_path_buf))
        .expect("the graded oracle's t/images");
    let dir = tempfile::tempdir().unwrap();

    let mut failures = Vec::new();
    for case in CASES {
        let file = fixture(oracle, &images, dir.path(), case);
        grade(
            oracle,
            &file,
            case.label,
            case.tags,
            case.must_have,
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "graded against {}:\n{}",
        oracle.provenance(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------
// Crafted directory orders. ExifTool's `FoundTag` (ExifTool.pm 13.59:
// 9468-9472, 9540-9591) keeps the first copy of a name unless a later one
// arrives with `priority >= old` (an old 0 counting as 1). A tag's priority
// is its own `Priority`, else 0 for `Avoid`, else 1 -- and a defined 0 is
// raised to 1 in the priority directory (IFD0 once its SubfileType is 0,
// Exif.pm 13.59:450-455). The ExifIFD (and the InteropIFD inside it) is
// walked inline at IFD0's 0x8769 entry, so an IFD0 entry stored after that
// entry arrives after every sub-IFD copy. No oracle write produces an
// unsorted IFD0 or a Photoshop 0xfde8 beside a 0xa430, so these files are
// built here, wrapped in a bare JPEG, and read by the oracle as they are.
// ---------------------------------------------------------------------

/// An 8x8 baseline JPEG with no metadata segment (from `xp_string_write.rs`).
const BASE_JPEG_HEX: &str = "ffd8ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";

const ASCII: u16 = 2;
const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;

/// One entry, in physical order: a value, or a sub-IFD pointer the builder
/// fills in.
enum Entry {
    Value(u16, u16, Vec<u8>),
    ExifPointer,
    InteropPointer,
}

fn ascii(tag: u16, text: &str) -> Entry {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    Entry::Value(tag, ASCII, bytes)
}

fn short(tag: u16, value: u16) -> Entry {
    Entry::Value(tag, SHORT, value.to_le_bytes().to_vec())
}

fn long(tag: u16, value: u32) -> Entry {
    Entry::Value(tag, LONG, value.to_le_bytes().to_vec())
}

fn rational(tag: u16, numerator: u32) -> Entry {
    let mut bytes = numerator.to_le_bytes().to_vec();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    Entry::Value(tag, RATIONAL, bytes)
}

/// A little-endian TIFF block: IFD0, the ExifIFD, an optional InteropIFD,
/// then every out-of-line value, entries kept in the order given.
fn tiff(ifd0: &[Entry], exif: &[Entry], interop: &[Entry]) -> Vec<u8> {
    let size = |entries: &[Entry]| 2 + 12 * entries.len() + 4;
    let exif_at = 8 + size(ifd0);
    let interop_at = exif_at + size(exif);
    let mut data_at = interop_at + if interop.is_empty() { 0 } else { size(interop) };
    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&8u32.to_le_bytes());
    let mut blobs = Vec::new();
    for entries in [ifd0, exif, interop] {
        if entries.is_empty() {
            continue;
        }
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for entry in entries {
            let (tag, kind, bytes) = match entry {
                Entry::Value(tag, kind, bytes) => (*tag, *kind, bytes.clone()),
                Entry::ExifPointer => (0x8769, LONG, (exif_at as u32).to_le_bytes().to_vec()),
                Entry::InteropPointer => (0xa005, LONG, (interop_at as u32).to_le_bytes().to_vec()),
            };
            let unit = match kind {
                SHORT => 2,
                LONG => 4,
                RATIONAL => 8,
                _ => 1,
            };
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&((bytes.len() / unit) as u32).to_le_bytes());
            if bytes.len() <= 4 {
                let mut inline = bytes.clone();
                inline.resize(4, 0);
                out.extend_from_slice(&inline);
            } else {
                out.extend_from_slice(&(data_at as u32).to_le_bytes());
                blobs.extend_from_slice(&bytes);
                if bytes.len() % 2 == 1 {
                    blobs.push(0);
                }
                data_at =
                    interop_at + if interop.is_empty() { 0 } else { size(interop) } + blobs.len();
            }
        }
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out.extend_from_slice(&blobs);
    out
}

/// `tiff` as the APP1 Exif segment of [`BASE_JPEG_HEX`].
fn jpeg(tiff: &[u8]) -> Vec<u8> {
    let base: Vec<u8> = (0..BASE_JPEG_HEX.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&BASE_JPEG_HEX[i..i + 2], 16).unwrap())
        .collect();
    let mut out = base[..2].to_vec();
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(tiff);
    out.extend_from_slice(&base[2..]);
    out
}

struct Crafted {
    label: &'static str,
    ifd0: Vec<Entry>,
    exif: Vec<Entry>,
    interop: Vec<Entry>,
    tags: &'static [&'static str],
    must_have: &'static [(&'static str, &'static str)],
}

fn crafted_cases() -> Vec<Crafted> {
    vec![
        // Review finding 1: a `Priority => 0` twin stored after the pointer
        // arrives second with 0 and cannot displace the first copy.
        Crafted {
            label: "Priority 0 WhiteBalance: the ExifIFD copy is found first",
            ifd0: vec![ascii(0x010f, "Test"), Entry::ExifPointer, short(0xa403, 0)],
            exif: vec![short(0xa403, 1)],
            interop: vec![],
            tags: &["WhiteBalance"],
            must_have: &[("IFD0", "WhiteBalance"), ("ExifIFD", "WhiteBalance")],
        },
        // ... unless IFD0 is the priority directory (SubfileType 0), which
        // raises its defined 0 to 1.
        Crafted {
            label: "Priority 0 WhiteBalance in the priority directory",
            ifd0: vec![
                long(0x00fe, 0),
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                short(0xa403, 0),
            ],
            exif: vec![short(0xa403, 1)],
            interop: vec![],
            tags: &["WhiteBalance"],
            must_have: &[("IFD0", "WhiteBalance"), ("ExifIFD", "WhiteBalance")],
        },
        // Finding 1 for the InteropIFD and an unsorted IFD0: IFD0's
        // ImageDescription/XResolution after the pointer lose to the
        // ExifIFD/InteropIFD copies found first; its CreateDate (priority
        // 1) still wins.
        Crafted {
            label: "unsorted IFD0 with Priority 0 twins after the pointer",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                ascii(0x010e, "IFD0Desc"),
                rational(0x011a, 180),
                ascii(0x9004, "2020:01:02 03:04:05"),
            ],
            exif: vec![
                ascii(0x010e, "ExifDesc"),
                ascii(0x9004, "2003:12:04 06:46:52"),
                Entry::InteropPointer,
            ],
            interop: vec![ascii(0x0001, "R98"), rational(0x011a, 72)],
            tags: &["ImageDescription", "XResolution", "CreateDate"],
            must_have: &[
                ("IFD0", "ImageDescription"),
                ("ExifIFD", "ImageDescription"),
                ("IFD0", "XResolution"),
                ("InteropIFD", "XResolution"),
                ("IFD0", "CreateDate"),
                ("ExifIFD", "CreateDate"),
            ],
        },
        // Review finding 2: one name, two ids, two priorities. IFD0's
        // 0xfde8 OwnerName (`Avoid`) after the pointer cannot displace the
        // ExifIFD's 0xa430 (priority 1) ...
        Crafted {
            label: "Avoid 0xfde8 OwnerName in IFD0 after the pointer",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                ascii(0xfde8, "Owner's Name: IFD0Owner"),
            ],
            exif: vec![ascii(0xa430, "ExifOwner")],
            interop: vec![],
            tags: &["OwnerName"],
            must_have: &[("IFD0", "OwnerName"), ("ExifIFD", "OwnerName")],
        },
        // ... while IFD0's 0xa430 does displace an ExifIFD 0xfde8.
        Crafted {
            label: "Avoid 0xfde8 OwnerName in the ExifIFD",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                ascii(0xa430, "IFD0Owner"),
            ],
            exif: vec![ascii(0xfde8, "Owner's Name: ExifOwner")],
            interop: vec![],
            tags: &["OwnerName"],
            must_have: &[("IFD0", "OwnerName"), ("ExifIFD", "OwnerName")],
        },
        // Before the pointer, IFD0's copy is first: a `Priority => 0` or
        // `Avoid` sub-IFD copy cannot displace it, a priority-1 one does.
        Crafted {
            label: "IFD0 twins before the pointer",
            ifd0: vec![
                ascii(0x010e, "IFD0Desc"),
                ascii(0x010f, "IFD0Make"),
                Entry::ExifPointer,
            ],
            exif: vec![ascii(0x010e, "ExifDesc"), ascii(0x010f, "ExifMake")],
            interop: vec![],
            tags: &["ImageDescription", "Make"],
            must_have: &[
                ("IFD0", "ImageDescription"),
                ("ExifIFD", "ImageDescription"),
                ("IFD0", "Make"),
                ("ExifIFD", "Make"),
            ],
        },
    ]
}

#[test]
fn crafted_directory_orders_follow_exiftools_found_tag_arbitration() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!(
            "skipping crafted duplicate-directory parity: no ExifTool oracle may grade output"
        );
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut failures = Vec::new();
    for case in crafted_cases() {
        let path = dir.path().join(format!(
            "{}.jpg",
            case.label
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        ));
        std::fs::write(&path, jpeg(&tiff(&case.ifd0, &case.exif, &case.interop))).unwrap();
        grade(
            oracle,
            &path,
            case.label,
            case.tags,
            case.must_have,
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "graded against {}:\n{}",
        oracle.provenance(),
        failures.join("\n")
    );
}
