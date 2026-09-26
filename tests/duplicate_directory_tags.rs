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

        // Every copy: `-a -G1 -s`.
        let all = tag_args(&["-a", "-G1", "-s"], case.tags);
        let theirs = rows(&run(oracle.command(), &all, &file));
        for (group, name) in case.must_have {
            assert!(
                theirs
                    .iter()
                    .any(|row| row.starts_with(&format!("[{group}]"))
                        && row.split_whitespace().nth(1) == Some(name)),
                "{}: the oracle's fixture lacks [{group}] {name}: {theirs:?}",
                case.label
            );
        }
        let ours = rows(&run(oxidex(), &all, &file));
        if ours != theirs {
            failures.push(format!(
                "{} -a -G1: missing {:?}, extra {:?}",
                case.label,
                theirs.difference(&ours).collect::<Vec<_>>(),
                ours.difference(&theirs).collect::<Vec<_>>()
            ));
        }

        // The default copy: plain `-s`, `-j` and `-s3 -<Name>`.
        let plain = tag_args(&["-s"], case.tags);
        let theirs = rows(&run(oracle.command(), &plain, &file));
        let ours = rows(&run(oxidex(), &plain, &file));
        if ours != theirs {
            failures.push(format!(
                "{} -s: oracle {:?}, oxidex {:?}",
                case.label,
                theirs.difference(&ours).collect::<Vec<_>>(),
                ours.difference(&theirs).collect::<Vec<_>>()
            ));
        }
        // `-j` without `-G`: one row per name, the default copy.
        let json = tag_args(&["-j"], case.tags);
        let theirs = json_rows(&run(oracle.command(), &json, &file), case.tags);
        let ours = json_rows(&run(oxidex(), &json, &file), case.tags);
        if ours != theirs {
            failures.push(format!(
                "{} -j: oracle {theirs:?}, oxidex {ours:?}",
                case.label
            ));
        }

        // The default copy of each name alone: `-s3 -<Name>`.
        for name in case.tags {
            let bare = vec!["-s3".to_string(), format!("-{name}")];
            let theirs = run(oracle.command(), &bare, &file);
            let ours = run(oxidex(), &bare, &file);
            if ours != theirs {
                failures.push(format!(
                    "{} -s3 -{name}: oracle {theirs:?}, oxidex {ours:?}",
                    case.label
                ));
            }
        }

        // Each copy by its own group: `-s3 -<Group>:<Name>`.
        for (group, name) in case.must_have {
            let qualified = vec!["-s3".to_string(), format!("-{group}:{name}")];
            let theirs = run(oracle.command(), &qualified, &file);
            let ours = run(oxidex(), &qualified, &file);
            if ours != theirs {
                failures.push(format!(
                    "{} -s3 -{group}:{name}: oracle {theirs:?}, oxidex {ours:?}",
                    case.label
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "graded against {}:\n{}",
        oracle.provenance(),
        failures.join("\n")
    );
}
