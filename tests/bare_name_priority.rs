//! A bare `-TAG` answers with the copy ExifTool 13.59 answers with.
//!
//! When a file carries one tag name in several groups, `exiftool -TAG`
//! prints the copy `FoundTag` left under the bare key (`SetFoundTags`,
//! ExifTool.pm 13.59:5389-5395), and `FoundTag` (ExifTool.pm:9518-9591)
//! decides that by priority -- the tag's own `Priority`, the table's
//! `PRIORITY`, 0 for `Avoid`, 0 in a `LOW_PRIORITY_DIR`, promoted in the
//! `PRIORITY_DIR` -- then by the order the copies were found, the later
//! winning a tie, and never across sub-documents (`DOC_NUM`). Since #957
//! `-TagsFromFile`/`copy_metadata` copies each tag by that same name, so a
//! wrong winner is also a wrong value written into the destination.
//!
//! Every case is one `t/images` file of the pinned tree, graded only by
//! [`exiftool_oracle::graded`] (pinned `-ver` and the `OOXML.docx` probe):
//! the oracle must itself report the name under at least two family-1
//! groups (`-a -G1`), so a case cannot pass by the file having lost its
//! duplicate, and then oxidex's `-s3 -TAG` and `-j -TAG` must equal the
//! oracle's. `tools/exiftool-tables/bare_name_breadth.py` measures the same
//! question over every multi-group name of every `t/images` file.

use oxidex::exiftool_oracle;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

struct Case {
    file: &'static str,
    tag: &'static str,
    /// The rule that picks ExifTool's copy, for the failure message.
    rule: &'static str,
}

/// The seven reported divergences.
const REPORTED: &[Case] = &[
    Case {
        file: "Kodak.jpg",
        tag: "MeteringMode",
        rule: "Kodak::Main is read (all 25 fields) and found after ExifIFD 0x9207",
    },
    Case {
        file: "Kodak.jpg",
        tag: "ExposureTime",
        rule: "Kodak::Main is read and found after ExifIFD 0x829a",
    },
    Case {
        file: "Kodak.jpg",
        tag: "FNumber",
        rule: "Kodak::Main is read and found after ExifIFD 0x829d",
    },
    Case {
        file: "Google.jpg",
        tag: "CreateDate",
        rule: "Google HDRP CreateDate is Priority => 0 (Google.pm:539-546)",
    },
    Case {
        file: "Casio2.jpg",
        tag: "WhiteBalance",
        rule: "Casio::Type2 0x2012 is found at 0x927C, before ExifIFD 0xa403 (Priority => 0)",
    },
    Case {
        file: "ExifTool.jpg",
        tag: "Copyright",
        rule: "MIE trailer is found last; Ducky is Priority => 0; JUMBF is a sub-document",
    },
];

/// A sample of the breadth set, one or two per rule.
const BREADTH: &[Case] = &[
    Case {
        file: "Pentax.jpg",
        tag: "Contrast",
        rule: "ExifIFD 0xa408 is found after the MakerNote at 0x927C",
    },
    Case {
        file: "NikonD70.jpg",
        tag: "Sharpness",
        rule: "ExifIFD 0xa40a is found after the MakerNote at 0x927C",
    },
    Case {
        file: "Olympus2.jpg",
        tag: "ExposureMode",
        rule: "ExifIFD 0xa402 is found after the MakerNote at 0x927C",
    },
    Case {
        file: "FujiFilm.raf",
        tag: "Saturation",
        rule: "RAF: ExifIFD 0xa409 is found after the MakerNote at 0x927C",
    },
    Case {
        file: "SigmaDP2.x3f",
        tag: "ExposureMode",
        rule: "X3F: ExifIFD 0xa402 is found after the MakerNote at 0x927C",
    },
    Case {
        file: "SigmaDP2.x3f",
        tag: "Contrast",
        rule: "Sigma::Main numeric Contrast is Priority => 0",
    },
    Case {
        file: "SigmaDP2.x3f",
        tag: "Software",
        rule: "Sigma::Main Software is Priority => 0",
    },
    Case {
        file: "SigmaDP2.x3f",
        tag: "DriveMode",
        rule: "SigmaRaw::Properties is PRIORITY => 0",
    },
    Case {
        file: "Real.rm",
        tag: "StreamMimeType",
        rule: "Real::MediaProps is PRIORITY => 0: the first stream wins",
    },
    Case {
        file: "Panasonic.rw2",
        tag: "WBRedLevel",
        rule: "the RW2 JpgFromRaw is sub-document Doc1",
    },
    Case {
        file: "PhaseOne.iiq",
        tag: "ImageWidth",
        rule: "IFD1 is not PRIORITY_DIR: its Priority => 0 ImageWidth stays 0",
    },
    Case {
        file: "DNG.dng",
        tag: "ImageWidth",
        rule: "the full-resolution SubIFD is PRIORITY_DIR",
    },
    Case {
        file: "DNG.dng",
        tag: "RowsPerStrip",
        rule: "reduced SubIFDs keep Priority => 0",
    },
    Case {
        file: "MWG.jpg",
        tag: "City",
        rule: "APP13 precedes the XMP APP1 in this file",
    },
    Case {
        file: "ExifTool.jpg",
        tag: "CropLeft",
        rule: "trailers are read from the end inwards: FotoStation last",
    },
    Case {
        file: "ExifTool.jpg",
        tag: "ProfileID",
        rule: "SPIFF APP8 is found before the ICC APP2",
    },
    Case {
        file: "XMP.svg",
        tag: "Title",
        rule: "the C2PA manifest's JSON is a JUMBF sub-document",
    },
    Case {
        file: "XMP.xml",
        tag: "FileType",
        rule: "an XMP tag no table defines is Priority => 0 (XMP.pm:3589-3596)",
    },
    Case {
        file: "CanonRaw.cr3",
        tag: "CreateDate",
        rule: "the Canon uuid box precedes mvhd in moov",
    },
];

fn images(oracle: &exiftool_oracle::Oracle) -> std::path::PathBuf {
    exiftool_oracle::capability_sample(oracle)
        .and_then(|docx| docx.parent().map(Path::to_path_buf))
        .expect("the graded oracle's t/images")
}

fn run(mut command: Command, args: &[&str], file: &Path) -> String {
    let out = command.args(args).arg(file).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn oxidex() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
}

/// The family-1 groups the oracle's `-a -G1 -s -TAG` reports the name under.
fn oracle_groups(oracle: &exiftool_oracle::Oracle, tag: &str, file: &Path) -> BTreeSet<String> {
    run(
        oracle.command(),
        &["-a", "-G1", "-s", &format!("-{tag}")],
        file,
    )
    .lines()
    .filter_map(|line| {
        let group = line.strip_prefix('[')?.split_once(']')?.0;
        (line.split_whitespace().nth(1) == Some(tag)).then(|| group.to_string())
    })
    .collect()
}

/// The single value `-j -TAG` reports, whatever the key's group prefix:
/// what is graded is which copy was chosen, not the key's spelling.
fn json_value(text: &str, tag: &str) -> Option<Value> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    parsed[0].as_object()?.iter().find_map(|(key, value)| {
        let name = key.rsplit_once(':').map_or(key.as_str(), |(_, name)| name);
        (name == tag).then(|| value.clone())
    })
}

fn grade(cases: &[Case]) {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping bare-name priority parity: no ExifTool oracle may grade output");
        return;
    };
    let images = images(oracle);
    let mut failures = Vec::new();
    for case in cases {
        let file = images.join(case.file);
        let groups = oracle_groups(oracle, case.tag, &file);
        assert!(
            groups.len() >= 2,
            "{} -{}: the oracle reports it in {groups:?}, not in two groups",
            case.file,
            case.tag
        );
        let bare = format!("-{}", case.tag);
        let theirs = run(oracle.command(), &["-s3", &bare], &file);
        let ours = run(oxidex(), &["-s3", &bare], &file);
        if ours != theirs {
            failures.push(format!(
                "{} -s3 -{} ({}): oracle {theirs:?}, oxidex {ours:?}",
                case.file, case.tag, case.rule
            ));
        }
        let theirs = json_value(&run(oracle.command(), &["-j", &bare], &file), case.tag);
        let ours = json_value(&run(oxidex(), &["-j", &bare], &file), case.tag);
        if ours != theirs {
            failures.push(format!(
                "{} -j -{} ({}): oracle {theirs:?}, oxidex {ours:?}",
                case.file, case.tag, case.rule
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "graded against {}:\n{}",
        oracle.provenance(),
        failures.join("\n")
    );
}

#[test]
fn reported_bare_names_answer_as_exiftool_does() {
    grade(REPORTED);
}

#[test]
fn breadth_sample_bare_names_answer_as_exiftool_does() {
    grade(BREADTH);
}

/// ExifTool reads a rational with a zero denominator as `inf` (non-zero
/// numerator) or `undef` (zero numerator) -- `GetRational64u` and its
/// siblings, ExifTool.pm 13.59:6090-6119 -- and a tag with no conversion,
/// like `DigitalZoomRatio` (Exif.pm:2886-2890), prints that string in every
/// output mode. `Casio2.jpg` stores 0/0.
#[test]
fn a_zero_over_zero_rational_is_undef_in_every_output_mode() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping zero-denominator rational parity: no oracle may grade output");
        return;
    };
    let file = images(oracle).join("Casio2.jpg");
    let tag = "-DigitalZoomRatio";
    let mut failures = Vec::new();
    for mode in [&["-s3"][..], &["-s3", "-n"][..]] {
        let args: Vec<&str> = mode.iter().copied().chain([tag]).collect();
        let theirs = run(oracle.command(), &args, &file);
        let ours = run(oxidex(), &args, &file);
        assert_eq!(theirs, "undef\n", "the oracle's own {mode:?} reading");
        if ours != theirs {
            failures.push(format!("{mode:?}: oracle {theirs:?}, oxidex {ours:?}"));
        }
    }
    for mode in [&["-j"][..], &["-j", "-n"][..]] {
        let args: Vec<&str> = mode.iter().copied().chain([tag]).collect();
        let theirs = json_value(&run(oracle.command(), &args, &file), "DigitalZoomRatio");
        let ours = json_value(&run(oxidex(), &args, &file), "DigitalZoomRatio");
        assert_eq!(theirs, Some(Value::String("undef".into())), "{mode:?}");
        if ours != theirs {
            failures.push(format!("{mode:?}: oracle {theirs:?}, oxidex {ours:?}"));
        }
    }
    // The grouped listing renders the same value.
    let theirs = run(oracle.command(), &["-G1", "-s", tag], &file);
    let ours = run(oxidex(), &["-G1", "-s", tag], &file);
    if ours != theirs {
        failures.push(format!("-G1 -s: oracle {theirs:?}, oxidex {ours:?}"));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
