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

/// Output lines in order: `-a` prints every copy in ExifTool's encounter
/// order (`FILE_ORDER`), so the order is part of what is graded.
fn rows(text: &str) -> Vec<String> {
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
            "{label} -a -G1:\n  oracle {theirs:#?}\n  oxidex {ours:#?}"
        ));
    }

    // Where the copies fall among the file's other rows: the full `-a -G1
    // -s` listing (grouped) and the full `-a -s` listing (file order),
    // each cut down to the lines of `tags`, order kept.
    for listing in [&["-a", "-G1", "-s"][..], &["-a", "-s"][..]] {
        let args: Vec<String> = listing.iter().map(|arg| arg.to_string()).collect();
        let named = |text: &str| -> Vec<String> {
            text.lines()
                .filter(|line| {
                    let row = line.split_once(']').map_or(*line, |(_, row)| row);
                    row.split_whitespace()
                        .next()
                        .is_some_and(|name| tags.contains(&name))
                })
                .map(str::to_string)
                .collect()
        };
        let theirs = named(&run(oracle.command(), &args, &file));
        let ours = named(&run(oxidex(), &args, &file));
        if ours != theirs {
            failures.push(format!(
                "{label} full {}:\n  oracle {theirs:#?}\n  oxidex {ours:#?}",
                listing.join(" ")
            ));
        }
    }

    // The default copy: plain `-s`, `-j` and `-s3 -<Name>`.
    let plain = tag_args(&["-s"], tags);
    let theirs = rows(&run(oracle.command(), &plain, &file));
    let ours = rows(&run(oxidex(), &plain, &file));
    if ours != theirs {
        failures.push(format!("{label} -s: oracle {theirs:?}, oxidex {ours:?}"));
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
    /// A second 0x8769 `ExifOffset`, to the extra directory.
    ExtraExifPointer,
    /// `(tag, type, count, value-or-offset)` written verbatim: a malformed
    /// entry whose value lies past the end of the block.
    Raw(u16, u16, u32, u32),
    /// `(tag, type, count)` whose out-of-line value points into its own
    /// directory's entry array: in bounds, so the IFD parser keeps it, but
    /// ExifTool refuses it ("Suspicious ... offset").
    IntoOwnDirectory(u16, u16, u32),
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

/// A little-endian TIFF block: IFD0, the ExifIFD, the InteropIFD and an
/// extra directory (each written even when empty), then every out-of-line
/// value, entries kept in the order given.
fn tiff(dirs: [&[Entry]; 4]) -> Vec<u8> {
    let size = |entries: &[Entry]| 2 + 12 * entries.len() + 4;
    let mut at = [8usize; 4];
    for i in 1..4 {
        at[i] = at[i - 1] + size(dirs[i - 1]);
    }
    let data_start = at[3] + size(dirs[3]);
    let mut out = b"II*\0".to_vec();
    out.extend_from_slice(&8u32.to_le_bytes());
    let mut blobs = Vec::new();
    for (entries, here) in dirs.iter().zip(at) {
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for entry in entries.iter() {
            let (tag, kind, count, field) = match entry {
                Entry::Raw(tag, kind, count, value) => (*tag, *kind, *count, value.to_le_bytes()),
                Entry::IntoOwnDirectory(tag, kind, count) => {
                    (*tag, *kind, *count, ((here + 2) as u32).to_le_bytes())
                }
                Entry::ExifPointer => (0x8769, LONG, 1, (at[1] as u32).to_le_bytes()),
                Entry::InteropPointer => (0xa005, LONG, 1, (at[2] as u32).to_le_bytes()),
                Entry::ExtraExifPointer => (0x8769, LONG, 1, (at[3] as u32).to_le_bytes()),
                Entry::Value(tag, kind, bytes) => {
                    let unit = match *kind {
                        SHORT => 2,
                        LONG => 4,
                        RATIONAL => 8,
                        _ => 1,
                    };
                    let field = if bytes.len() <= 4 {
                        let mut inline = bytes.clone();
                        inline.resize(4, 0);
                        [inline[0], inline[1], inline[2], inline[3]]
                    } else {
                        let offset = (data_start + blobs.len()) as u32;
                        blobs.extend_from_slice(bytes);
                        if bytes.len() % 2 == 1 {
                            blobs.push(0);
                        }
                        offset.to_le_bytes()
                    };
                    (*tag, *kind, (bytes.len() / unit) as u32, field)
                }
            };
            out.extend_from_slice(&tag.to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&count.to_le_bytes());
            out.extend_from_slice(&field);
        }
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out.extend_from_slice(&blobs);
    out
}

/// A JFIF APP0 segment with 72 dpi density, which ExifTool reads at
/// `Priority => -1` (ExifTool.pm 13.59:2218-2233).
const JFIF_APP0: [u8; 18] = [
    0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0, 1, 2, 1, 0, 72, 0, 72, 0, 0,
];

/// `tiff` as the APP1 Exif segment of [`BASE_JPEG_HEX`], after a JFIF
/// APP0 segment when `jfif`.
fn jpeg(tiff: &[u8], jfif: bool) -> Vec<u8> {
    let base: Vec<u8> = (0..BASE_JPEG_HEX.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&BASE_JPEG_HEX[i..i + 2], 16).unwrap())
        .collect();
    let mut out = base[..2].to_vec();
    if jfif {
        out.extend_from_slice(&JFIF_APP0);
    }
    out.extend_from_slice(&[0xff, 0xe1]);
    out.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
    out.extend_from_slice(b"Exif\0\0");
    out.extend_from_slice(tiff);
    out.extend_from_slice(&base[2..]);
    out
}

#[derive(Default)]
struct Crafted {
    label: &'static str,
    ifd0: Vec<Entry>,
    exif: Vec<Entry>,
    interop: Vec<Entry>,
    /// The directory a second `ExifOffset` ([`Entry::ExtraExifPointer`])
    /// points at.
    extra: Vec<Entry>,
    /// Precede the Exif segment with a JFIF APP0 segment.
    jfif: bool,
    /// Write the TIFF block as a standalone `.tif` instead of a JPEG.
    tiff_file: bool,
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        },
        // Review round 3, finding 1: 0x920e has no `tag_db` name, so the
        // IFD0 twin must be keyed by the name the generated table reports
        // (FocalPlaneXResolution); after the pointer, at priority 1, it is
        // found last and wins.
        Crafted {
            label: "engine-named 0x920e twin after the pointer",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                rational(0x920e, 100),
            ],
            exif: vec![rational(0x920e, 200)],
            interop: vec![],
            tags: &["FocalPlaneXResolution"],
            must_have: &[
                ("IFD0", "FocalPlaneXResolution"),
                ("ExifIFD", "FocalPlaneXResolution"),
            ],
            ..Default::default()
        },
        // Finding 2: a malformed IFD0 copy after the pointer (its value
        // lies past the block) is skipped, as ExifTool skips it, so it
        // cannot displace the ExifIFD copy; the valid IFD0 copy before the
        // pointer is displaced by that equal-priority ExifIFD copy.
        Crafted {
            label: "malformed IFD0 duplicate after the pointer",
            ifd0: vec![
                ascii(0x010f, "Test"),
                ascii(0xa430, "IFD0Owner"),
                Entry::ExifPointer,
                Entry::Raw(0xa430, ASCII, 20, 0x00ff_ff00),
            ],
            exif: vec![ascii(0xa430, "ExifOwner")],
            interop: vec![],
            tags: &["OwnerName"],
            must_have: &[("IFD0", "OwnerName"), ("ExifIFD", "OwnerName")],
            ..Default::default()
        },
        // Finding 3: two priority-0 ExifIFD copies and a priority-0 IFD0
        // twin after the pointer: the first ExifIFD copy, found first, is
        // never displaced.
        Crafted {
            label: "two priority 0 ExifIFD copies and a later IFD0 twin",
            ifd0: vec![ascii(0x010f, "Test"), Entry::ExifPointer, short(0xa403, 0)],
            exif: vec![short(0xa403, 1), short(0xa403, 2)],
            interop: vec![],
            tags: &["WhiteBalance"],
            must_have: &[("IFD0", "WhiteBalance"), ("ExifIFD", "WhiteBalance")],
            ..Default::default()
        },
        // Local review of the round-3 fix: an earlier copy that is never
        // found (its value overlaps its own directory) does not count as
        // the first copy.
        Crafted {
            label: "refused first ExifIFD copy and a later IFD0 twin",
            ifd0: vec![ascii(0x010f, "Test"), Entry::ExifPointer, short(0xa403, 0)],
            exif: vec![Entry::IntoOwnDirectory(0xa403, SHORT, 3), short(0xa403, 1)],
            interop: vec![],
            tags: &["WhiteBalance"],
            must_have: &[("IFD0", "WhiteBalance"), ("ExifIFD", "WhiteBalance")],
            ..Default::default()
        },
        // Review round 4: an IFD0 entry after the pointer whose RawConv
        // returns undef (an all-NUL PanasonicTitle) never reaches FoundTag,
        // so the ExifIFD copy displaces the valid IFD0 copy before the
        // pointer and nothing displaces it.
        Crafted {
            label: "RawConv-dropped IFD0 duplicate after the pointer",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::Value(0xc6d2, 7, b"IFD0Title\0".to_vec()),
                Entry::ExifPointer,
                Entry::Value(0xc6d2, 7, vec![0; 64]),
            ],
            exif: vec![Entry::Value(0xc6d2, 7, b"ExifTitle\0".to_vec())],
            interop: vec![],
            tags: &["PanasonicTitle"],
            must_have: &[("IFD0", "PanasonicTitle"), ("ExifIFD", "PanasonicTitle")],
            ..Default::default()
        },
        // Local review of the round-4 fix: a SubfileType 0 stored after the
        // pointer makes IFD0 the priority directory for the entries after
        // it, so their priority-0 copies arrive at 1 and displace.
        Crafted {
            label: "priority directory marked after the pointer",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                long(0x00fe, 0),
                short(0xa403, 0),
                ascii(0x010e, "IFD0Desc"),
            ],
            exif: vec![short(0xa403, 1), ascii(0x010e, "ExifDesc")],
            interop: vec![],
            tags: &["WhiteBalance", "ImageDescription"],
            must_have: &[
                ("IFD0", "WhiteBalance"),
                ("ExifIFD", "WhiteBalance"),
                ("IFD0", "ImageDescription"),
                ("ExifIFD", "ImageDescription"),
            ],
            ..Default::default()
        },
        // Local review, second pair of runs: an IFD0 entry before the
        // pointer that only the generated reader accepts (format 13, the
        // `ifd` int32u) is still found before the ExifIFD.
        Crafted {
            label: "engine-only IFD0 entry before the pointer",
            ifd0: vec![Entry::Raw(0x010f, 13, 1, 0x1234), Entry::ExifPointer],
            exif: vec![ascii(0x010f, "ExifMake")],
            interop: vec![],
            tags: &["Make"],
            must_have: &[("ExifIFD", "Make")],
            ..Default::default()
        },
        // Local review, round 4 (three xhigh runs on 0d257243):
        // GeoTIFF keys are derived after every IFD0 entry, including those
        // after the pointer (a standalone TIFF).
        Crafted {
            label: "GeoTIFF keys after IFD0 entries past the pointer",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                ascii(0x010e, "IFD0Desc"),
                Entry::Value(
                    0x87af,
                    SHORT,
                    [1u16, 1, 0, 1, 1024, 0, 1, 2]
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect(),
                ),
            ],
            exif: vec![ascii(0x010e, "ExifDesc")],
            tiff_file: true,
            tags: &["ImageDescription", "GeoTiffVersion", "GTModelType"],
            must_have: &[
                ("IFD0", "ImageDescription"),
                ("ExifIFD", "ImageDescription"),
            ],
            ..Default::default()
        },
        // JFIF's density tags rank below every EXIF copy: a priority-0 IFD0
        // copy after the pointer still displaces them ...
        Crafted {
            label: "JFIF density and a priority 0 IFD0 copy after the pointer",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                rational(0x011a, 180),
            ],
            exif: vec![short(0xa001, 1)],
            jfif: true,
            tags: &["XResolution"],
            must_have: &[("JFIF", "XResolution"), ("IFD0", "XResolution")],
            ..Default::default()
        },
        // ... but not an ExifIFD copy found before it.
        Crafted {
            label: "JFIF density, an ExifIFD copy and a later IFD0 copy",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                rational(0x011a, 180),
            ],
            exif: vec![rational(0x011a, 300)],
            jfif: true,
            tags: &["XResolution"],
            must_have: &[
                ("JFIF", "XResolution"),
                ("ExifIFD", "XResolution"),
                ("IFD0", "XResolution"),
            ],
            ..Default::default()
        },
        // Two `ExifOffset` entries: each sub-directory is read at its own
        // entry.
        Crafted {
            label: "two ExifOffset entries",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                rational(0x011a, 180),
                Entry::ExtraExifPointer,
            ],
            exif: vec![short(0xa001, 1)],
            extra: vec![rational(0x011a, 300)],
            tags: &["XResolution", "ColorSpace"],
            must_have: &[("IFD0", "XResolution"), ("ExifIFD", "XResolution")],
            ..Default::default()
        },
        // An entry only the generated reader accepts (format 13), after
        // the pointer, is found at its own position: the later ASCII Make
        // still arrives after it.
        Crafted {
            label: "engine-only IFD0 entry after the pointer",
            ifd0: vec![
                Entry::ExifPointer,
                Entry::Raw(0x010f, 13, 1, 0x1234),
                ascii(0x010f, "IFD0Make"),
            ],
            exif: vec![ascii(0x010f, "ExifMake")],
            tags: &["Make"],
            must_have: &[("IFD0", "Make"), ("ExifIFD", "Make")],
            ..Default::default()
        },
        // ... and ranked by the priority-directory state at its own entry,
        // before a later SubfileType 0.
        Crafted {
            label: "engine-only priority 0 entry before a later SubfileType",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                Entry::Raw(0x0100, 13, 1, 100),
                long(0x00fe, 0),
            ],
            exif: vec![long(0x0100, 300)],
            tiff_file: true,
            tags: &["ImageWidth"],
            must_have: &[("IFD0", "ImageWidth"), ("ExifIFD", "ImageWidth")],
            ..Default::default()
        },
        // Local review of c7323dfb: a priority-0 copy in a second ExifIFD
        // cannot displace the first ExifIFD's.
        Crafted {
            label: "priority 0 copies in two ExifIFDs",
            ifd0: vec![
                ascii(0x010f, "Test"),
                Entry::ExifPointer,
                Entry::ExtraExifPointer,
            ],
            exif: vec![short(0xa403, 1)],
            extra: vec![short(0xa403, 2)],
            tiff_file: true,
            tags: &["WhiteBalance"],
            must_have: &[("ExifIFD", "WhiteBalance")],
            ..Default::default()
        },
        // ... and a MakerNote stored in IFD0 before the pointer is read at
        // its own entry, before the ExifIFD.
        Crafted {
            label: "IFD0 MakerNote before the pointer",
            ifd0: vec![
                ascii(0x010f, "Canon"),
                Entry::Value(
                    0x927c,
                    7,
                    [
                        &1u16.to_le_bytes()[..],
                        &0x0009u16.to_le_bytes(),
                        &2u16.to_le_bytes(),
                        &4u32.to_le_bytes(),
                        b"MOW\0",
                        &0u32.to_le_bytes(),
                    ]
                    .concat(),
                ),
                Entry::ExifPointer,
            ],
            exif: vec![ascii(0xa430, "EEX")],
            tiff_file: true,
            tags: &["OwnerName"],
            must_have: &[("Canon", "OwnerName"), ("ExifIFD", "OwnerName")],
            ..Default::default()
        },
        // Local review of 90d9633e: the InteropIFD is read at its pointer
        // entry, before the ExifIFD entries after it ...
        Crafted {
            label: "InteropIFD pointer before an ExifIFD twin",
            ifd0: vec![ascii(0x010f, "Test"), Entry::ExifPointer],
            exif: vec![Entry::InteropPointer, rational(0x011a, 300)],
            interop: vec![ascii(0x0001, "R98"), rational(0x011a, 200)],
            tiff_file: true,
            tags: &["XResolution"],
            must_have: &[("ExifIFD", "XResolution"), ("InteropIFD", "XResolution")],
            ..Default::default()
        },
        // ... and an IFD0 `Avoid` copy after an IFD0 MakerNote copy cannot
        // displace it, with no ExifIFD at all.
        Crafted {
            label: "IFD0 Avoid copy after an IFD0 MakerNote",
            ifd0: vec![
                ascii(0x010f, "Canon"),
                Entry::Value(
                    0x927c,
                    7,
                    [
                        &1u16.to_le_bytes()[..],
                        &0x0009u16.to_le_bytes(),
                        &2u16.to_le_bytes(),
                        &4u32.to_le_bytes(),
                        b"MOW\0",
                        &0u32.to_le_bytes(),
                    ]
                    .concat(),
                ),
                ascii(0xfde8, "Owner's Name: IFD"),
            ],
            tiff_file: true,
            tags: &["OwnerName"],
            must_have: &[("Canon", "OwnerName"), ("IFD0", "OwnerName")],
            ..Default::default()
        },
        // ... and without any IFD0 twin.
        Crafted {
            label: "two priority 0 ExifIFD copies",
            ifd0: vec![ascii(0x010f, "Test"), Entry::ExifPointer],
            exif: vec![short(0xa403, 1), short(0xa403, 2)],
            interop: vec![],
            tags: &["WhiteBalance"],
            must_have: &[("ExifIFD", "WhiteBalance")],
            ..Default::default()
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
            "{}.{}",
            case.label
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>(),
            if case.tiff_file { "tif" } else { "jpg" }
        ));
        let block = tiff([&case.ifd0, &case.exif, &case.interop, &case.extra]);
        let bytes = if case.tiff_file {
            block
        } else {
            jpeg(&block, case.jfif)
        };
        std::fs::write(&path, bytes).unwrap();
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
