//! Writing an EXIF tag into IFD0 moves it out of ExifIFD, and the other way
//! round, as pinned ExifTool 13.59 does (WriteExif.pl 13.59:20-23
//! `%crossDelete = (ExifIFD => 'IFD0', IFD0 => 'ExifIFD')`, applied at
//! :1156-1171 and :990-996).
//!
//! `-IFD0:CreateDate=...` on `t/images/Canon.jpg`, whose ExifIFD holds
//! CreateDate, leaves ExifTool's file with `[IFD0] CreateDate` only; oxidex
//! kept `[ExifIFD] CreateDate 2003:12:04 06:46:52` beside the new one.
//!
//! Every case runs the real CLI (`CARGO_BIN_EXE_oxidex`) and the graded
//! oracle (`exiftool_oracle::graded`) with the same arguments on two copies
//! of one source, then reads both back with the oracle (`-j -a -G1 -n
//! -EXIF:All`) and requires the same rows for every tag the case writes, in
//! every EXIF directory. Sources that need a tag in both directories are made
//! by the oracle itself from a `t/images` file. The cases are the
//! `cross-dir-move` matrix named in the PR; the ones oxidex refuses for other
//! reasons (an `ExifIFD:` qualifier the generated writer does not admit, a
//! TIFF with no ExifIFD to create one in, a PrintConv value) are left out.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle::{self, Oracle};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const DATE: &str = "2020:01:02 03:04:05";
const EXIF_GROUPS: &[&str] = &["IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD"];

/// A `t/images` file, optionally rewritten by the oracle first.
#[derive(Clone, Copy)]
struct Source {
    image: &'static str,
    setup: &'static [&'static str],
}

const CANON: Source = Source {
    image: "Canon.jpg",
    setup: &[],
};
const NIKON: Source = Source {
    image: "Nikon.jpg",
    setup: &[],
};
const GPS: Source = Source {
    image: "GPS.jpg",
    setup: &[],
};
/// Canon.jpg with CreateDate and ModifyDate in both IFD0 and ExifIFD.
const CANON_BOTH: Source = Source {
    image: "Canon.jpg",
    setup: &[
        "-IFD0:CreateDate=2001:01:01 01:01:01",
        "-ExifIFD:CreateDate=2002:02:02 02:02:02",
        "-IFD0:ModifyDate=2003:03:03 03:03:03",
        "-ExifIFD:ModifyDate=2004:04:04 04:04:04",
    ],
};
/// Canon.jpg with Artist (a generated-writer tag) in IFD0 and ExifIFD.
const CANON_BOTH_ARTIST: Source = Source {
    image: "Canon.jpg",
    setup: &["-IFD0:Artist=Ifd0 Artist", "-ExifIFD:Artist=Exif Artist"],
};
/// ExifTool.tif with an ExifIFD, and ModifyDate and Artist in both.
const TIFF_BOTH: Source = Source {
    image: "ExifTool.tif",
    setup: &[
        "-ExifIFD:ModifyDate=2004:04:04 04:04:04",
        "-IFD0:ModifyDate=2003:03:03 03:03:03",
        "-ExifIFD:Artist=Exif Artist",
        "-IFD0:Artist=Ifd0 Artist",
    ],
};
/// PNG.png with an `eXIf` chunk holding IFD0 and ExifIFD tags.
const PNG_EXIF: Source = Source {
    image: "PNG.png",
    setup: &[
        "-IFD0:ModifyDate=2003:03:03 03:03:03",
        "-IFD0:XResolution=72",
        "-ExifIFD:CreateDate=2002:02:02 02:02:02",
        "-ExifIFD:ISO=100",
    ],
};

fn oxidex() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
}

/// A copy of `source` in `dir` named `name`, or `None` when the pinned
/// `t/images` tree is absent.
fn materialize(oracle: &Oracle, source: Source, dir: &Path, name: &str) -> Option<PathBuf> {
    let image = fixtures::pinned_t_images_fixture_path(source.image)?;
    let extension = Path::new(source.image).extension().unwrap();
    let path = dir.join(name).with_extension(extension);
    std::fs::copy(&image, &path).unwrap();
    if !source.setup.is_empty() {
        let status = oracle
            .command()
            .args(["-q", "-q", "-overwrite_original"])
            .args(source.setup)
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success(), "oracle setup {:?} failed", source.setup);
    }
    Some(path)
}

/// Every EXIF row of `path` as the oracle reads it (`-a -G1 -n`), keyed
/// `Group:Tag`.
fn exif_rows(oracle: &Oracle, path: &Path) -> BTreeMap<String, String> {
    let out = oracle
        .command()
        .args(["-j", "-a", "-G1", "-n", "-EXIF:All"])
        .arg(path)
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("oracle read of {}: {e}", path.display()));
    json[0]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(key, _)| {
            key.split_once(':')
                .is_some_and(|(group, _)| EXIF_GROUPS.contains(&group))
        })
        .map(|(key, value)| (key.clone(), value.to_string()))
        .collect()
}

/// The tag names `args` write (`-IFD0:CreateDate=x` -> `CreateDate`).
fn written_names(args: &[&str]) -> Vec<String> {
    args.iter()
        .map(|arg| {
            let key = arg.trim_start_matches('-').split('=').next().unwrap();
            key.rsplit(':').next().unwrap().to_string()
        })
        .collect()
}

/// `rows` restricted to the tags named `names`, in any EXIF directory.
fn rows_named(rows: &BTreeMap<String, String>, names: &[String]) -> BTreeMap<String, String> {
    rows.iter()
        .filter(|(key, _)| {
            key.split_once(':')
                .is_some_and(|(_, tag)| names.iter().any(|name| name == tag))
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Pinned ExifTool's `-validate` warnings for `path`, less the summary line.
fn validate_warnings(oracle: &Oracle, path: &Path) -> std::collections::BTreeSet<String> {
    let out = oracle
        .command()
        .args(["-a", "-s3", "-validate", "-Warning"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| {
            *line != "OK"
                && !line.split_once(' ').is_some_and(|(n, rest)| {
                    n.bytes().all(|b| b.is_ascii_digit()) && rest.starts_with("Warning")
                })
        })
        .map(str::to_string)
        .collect()
}

/// Runs each `(source, args)` through oxidex and the oracle and returns a
/// description of every case whose written tags read back differently, or
/// whose oxidex run failed. `None` when no graded oracle or `t/images`.
fn mismatches(cases: &[(Source, &[&str])]) -> Option<Vec<String>> {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return None;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut failures = Vec::new();
    for (index, (source, args)) in cases.iter().enumerate() {
        let label = format!("{} {:?}", source.image, args);
        let Some(theirs) = materialize(oracle, *source, dir.path(), &format!("{index}-et")) else {
            eprintln!("skipping: pinned fixture {} is absent", source.image);
            return None;
        };
        let ours = materialize(oracle, *source, dir.path(), &format!("{index}-ox")).unwrap();
        let status = oracle
            .command()
            .args(["-q", "-q", "-overwrite_original"])
            .args(*args)
            .arg(&theirs)
            .status()
            .unwrap();
        assert!(status.success(), "{label}: oracle failed");
        let output = oxidex()
            .arg("-overwrite_original")
            .args(*args)
            .arg(&ours)
            .output()
            .unwrap();
        if !output.status.success() {
            failures.push(format!(
                "{label}: oxidex failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            continue;
        }
        let names = written_names(args);
        let expected = rows_named(&exif_rows(oracle, &theirs), &names);
        let actual = rows_named(&exif_rows(oracle, &ours), &names);
        if expected != actual {
            failures.push(format!(
                "{label}:\n    ExifTool {expected:?}\n    oxidex   {actual:?}"
            ));
        }
        // No structural damage the oracle's own edit does not have.
        let theirs_warnings = validate_warnings(oracle, &theirs);
        let extra: Vec<String> = validate_warnings(oracle, &ours)
            .difference(&theirs_warnings)
            .cloned()
            .collect();
        if !extra.is_empty() {
            failures.push(format!(
                "{label}: -validate warnings ExifTool's edit does not give: {extra:?}"
            ));
        }
    }
    Some(failures)
}

fn assert_matches_oracle(cases: &[(Source, &[&str])]) {
    if let Some(failures) = mismatches(cases) {
        assert!(
            failures.is_empty(),
            "{} of {} cases differ from pinned ExifTool 13.59:\n{}",
            failures.len(),
            cases.len(),
            failures.join("\n")
        );
    }
}

/// Created in one directory, removed from the other: IFD0 <- ExifIFD
/// (CreateDate, DateTimeOriginal, ISO, ExposureTime) and ExifIFD <- IFD0
/// (ModifyDate, Make), on two cameras' JPEGs.
#[test]
fn a_created_tag_moves_out_of_the_other_directory_jpeg() {
    let ifd0_create = format!("-IFD0:CreateDate={DATE}");
    let ifd0_original = format!("-IFD0:DateTimeOriginal={DATE}");
    let exif_modify = format!("-ExifIFD:ModifyDate={DATE}");
    assert_matches_oracle(&[
        (CANON, &[ifd0_create.as_str()]),
        (CANON, &[ifd0_original.as_str()]),
        (CANON, &["-IFD0:ISO=200"]),
        (CANON, &["-IFD0:ExposureTime=1/50"]),
        (CANON, &[exif_modify.as_str()]),
        (CANON, &["-ExifIFD:Make=Acme"]),
        (NIKON, &[ifd0_create.as_str()]),
        // Two tags in one command, each moved.
        (CANON_BOTH, &[ifd0_create.as_str(), exif_modify.as_str()]),
    ]);
}

/// Edited where both directories hold the tag (the reader reports only one
/// copy of such a pair; the other is deleted all the same): grouped, the
/// ungrouped name oxidex resolves to ExifIFD, the family-0 `EXIF:` alias
/// (written to the tag's write group, ExifIFD), and a set to the value IFD0
/// already holds, which still deletes the ExifIFD copy.
#[test]
fn an_edited_tag_moves_out_of_the_other_directory_jpeg() {
    let args: Vec<String> = [
        "-IFD0:CreateDate",
        "-ExifIFD:CreateDate",
        "-IFD0:ModifyDate",
        "-ExifIFD:ModifyDate",
        "-CreateDate",
        "-EXIF:CreateDate",
    ]
    .iter()
    .map(|tag| format!("{tag}={DATE}"))
    .collect();
    let mut cases: Vec<(Source, &[&str])> = Vec::new();
    let single: Vec<[&str; 1]> = args.iter().map(|arg| [arg.as_str()]).collect();
    for arg in &single {
        cases.push((CANON_BOTH, arg));
    }
    cases.push((CANON_BOTH, &["-IFD0:CreateDate=2001:01:01 01:01:01"]));
    assert_matches_oracle(&cases);
}

/// A command that sets the tag in both directories keeps both, whether the
/// file held one copy or two -- and a set with a deletion of the other copy
/// leaves the set one.
#[test]
fn setting_both_directories_in_one_command_keeps_both() {
    let ifd0 = format!("-IFD0:CreateDate={DATE}");
    let exif = "-ExifIFD:CreateDate=2021:01:01 00:00:00";
    let ifd0_modify = format!("-IFD0:ModifyDate={DATE}");
    let exif_modify = "-ExifIFD:ModifyDate=2021:01:01 00:00:00";
    assert_matches_oracle(&[
        (CANON, &[ifd0.as_str(), exif]),
        (CANON_BOTH, &[ifd0.as_str(), exif]),
        (CANON_BOTH, &[exif, ifd0.as_str()]),
        (CANON, &[ifd0.as_str(), "-ExifIFD:CreateDate="]),
        (TIFF_BOTH, &[ifd0_modify.as_str(), exif_modify]),
        (PNG_EXIF, &[ifd0.as_str(), exif]),
    ]);
}

/// Deleting a tag from one directory never deletes the other copy
/// (WriteExif.pl 13.59:1165-1169), nor does deleting an absent one.
#[test]
fn a_deletion_does_not_cross() {
    assert_matches_oracle(&[
        (CANON_BOTH, &["-IFD0:CreateDate="]),
        (CANON_BOTH, &["-ExifIFD:CreateDate="]),
        (CANON, &["-IFD0:CreateDate="]),
        (TIFF_BOTH, &["-ExifIFD:ModifyDate="]),
    ]);
}

/// `%crossDelete` pairs IFD0 and ExifIFD only: an IFD1, GPS or InteropIFD
/// copy is never touched, and a tag held nowhere else is simply created.
/// Nor is a mandatory entry of the other directory (ExifIFD's ExifVersion,
/// WriteExif.pl 13.59:26-54, checked at :1156).
#[test]
fn other_directories_and_mandatory_entries_are_untouched() {
    assert_matches_oracle(&[
        (GPS, &["-IFD1:XResolution=300"]),
        (GPS, &["-IFD0:XResolution=300"]),
        (GPS, &["-GPS:GPSAltitude=100"]),
        (NIKON, &["-IFD0:InteropIndex=R03"]),
        (NIKON, &["-ExifIFD:InteropIndex=R03"]),
        (CANON, &["-IFD0:Artist=Me"]),
        (CANON, &["-IFD0:ExifVersion=0230"]),
    ]);
}

/// TIFF: the in-place writer deletes the other copy too.
#[test]
fn a_tag_moves_between_directories_tiff() {
    let ifd0 = format!("-IFD0:ModifyDate={DATE}");
    let exif = format!("-ExifIFD:ModifyDate={DATE}");
    assert_matches_oracle(&[(TIFF_BOTH, &[ifd0.as_str()]), (TIFF_BOTH, &[exif.as_str()])]);
}

/// Setting an ExifIFD tag in a file with no ExifIFD creates the directory,
/// with WriteExif's mandatory ExifVersion, ComponentsConfiguration and
/// ColorSpace (WriteExif.pl 13.59:714-719), and moves the IFD0 copy: the
/// maintainer's `-ExifIFD:ModifyDate=` on t/images ExifTool.tif, its
/// mirrors, and the same on a JPEG and a PNG `eXIf` without an ExifIFD.
#[test]
fn a_tag_moves_into_an_exif_ifd_the_write_creates() {
    const TIFF: Source = Source {
        image: "ExifTool.tif",
        setup: &[],
    };
    const JPEG_NO_EXIF_IFD: Source = Source {
        image: "Canon.jpg",
        setup: &["-ExifIFD:All="],
    };
    const PNG_IFD0_ONLY: Source = Source {
        image: "PNG.png",
        setup: &["-IFD0:ModifyDate=2003:03:03 03:03:03"],
    };
    let exif_modify = format!("-ExifIFD:ModifyDate={DATE}");
    let ifd0_modify = "-IFD0:ModifyDate=2021:01:01 00:00:00";
    assert_matches_oracle(&[
        (TIFF, &[exif_modify.as_str()]),
        (TIFF, &["-ExifIFD:Software=Me"]),
        (TIFF, &["-ExifIFD:ISO=200"]),
        (TIFF, &[exif_modify.as_str(), ifd0_modify]),
        (JPEG_NO_EXIF_IFD, &[exif_modify.as_str()]),
        (JPEG_NO_EXIF_IFD, &["-ExifIFD:ISO=200"]),
        (PNG_IFD0_ONLY, &[exif_modify.as_str()]),
    ]);
}

/// `-ExifIFD:XResolution=300` (the maintainer's other example: 13.59 writes
/// `[ExifIFD] XResolution 300` and removes IFD0's) is refused by name, the
/// file untouched: XResolution belongs to the generated writer, which
/// compiles only SetNewValue's IFD<n> qualifier branch, not the %exifDirs
/// branch that selects ExifIFD. JPEG, TIFF and PNG `eXIf`.
#[test]
fn an_exififd_qualifier_the_generated_writer_lacks_is_refused_by_name() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sources = [
        CANON,
        Source {
            image: "ExifTool.tif",
            setup: &[],
        },
        PNG_EXIF,
    ];
    for (index, source) in sources.into_iter().enumerate() {
        let Some(path) = materialize(oracle, source, dir.path(), &format!("x{index}")) else {
            eprintln!("skipping: pinned fixture {} is absent", source.image);
            return;
        };
        let before = std::fs::read(&path).unwrap();
        let output = oxidex()
            .args(["-overwrite_original", "-ExifIFD:XResolution=300"])
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{}: not refused", source.image);
        assert!(
            stderr.contains("'ExifIFD:XResolution'") && stderr.contains("%exifDirs"),
            "{}: {stderr}",
            source.image
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "{}", source.image);
    }
}

/// PNG `eXIf`: the same moves inside the chunk.
#[test]
fn a_tag_moves_between_directories_png_exif() {
    let create = format!("-IFD0:CreateDate={DATE}");
    let modify = format!("-ExifIFD:ModifyDate={DATE}");
    assert_matches_oracle(&[
        (PNG_EXIF, &[create.as_str()]),
        (PNG_EXIF, &[modify.as_str()]),
        (PNG_EXIF, &["-IFD0:ISO=200"]),
    ]);
}

/// Ungrouped and `EXIF:` date forms (the CLI's date-shift route before):
/// `-ModifyDate=`, `-EXIF:ModifyDate=` and `-AllDates=` write each date to
/// its EXIF directory and move the other copy; `-EXIF:ModifyDate=` in a PNG
/// with no EXIF creates the `eXIf` chunk, as ExifTool does.
#[test]
fn date_forms_move_the_other_copy() {
    let modify = format!("-ModifyDate={DATE}");
    let exif_modify = format!("-EXIF:ModifyDate={DATE}");
    let exif_create = format!("-EXIF:CreateDate={DATE}");
    let all = format!("-AllDates={DATE}");
    const PNG: Source = Source {
        image: "PNG.png",
        setup: &[],
    };
    assert_matches_oracle(&[
        (CANON_BOTH, &[modify.as_str()]),
        (CANON_BOTH, &[exif_modify.as_str()]),
        (CANON_BOTH, &[all.as_str()]),
        (CANON_BOTH, &[exif_modify.as_str(), exif_create.as_str()]),
        (CANON, &[all.as_str()]),
        (TIFF_BOTH, &[modify.as_str()]),
        (TIFF_BOTH, &[exif_modify.as_str()]),
        (TIFF_BOTH, &[all.as_str()]),
        (PNG_EXIF, &[exif_modify.as_str()]),
        (PNG, &[exif_modify.as_str()]),
    ]);
}

/// What this writer cannot match is refused by name, the file left
/// byte-identical. An ungrouped date ExifTool also writes outside EXIF: in
/// a PNG its own chunks (13.59 writes `[PNG] ModifyDate`), in t/images
/// ExifTool.jpg the CIFF and MIE copies. A date route mixed with other
/// writes in one command, which the CLI used to cut short.
#[test]
fn date_forms_it_cannot_match_are_refused_by_name() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let modify = format!("-ModifyDate={DATE}");
    let original = format!("-DateTimeOriginal={DATE}");
    let all = format!("-AllDates={DATE}");
    let exif_modify = "-ExifIFD:ModifyDate=2021:01:01 00:00:00";
    let cases: [(Source, &[&str], &str); 5] = [
        (PNG_EXIF, &[modify.as_str()], "PNG:ModifyDate"),
        (PNG_EXIF, &[all.as_str()], "PNG:CreateDate"),
        (
            Source {
                image: "ExifTool.jpg",
                setup: &[],
            },
            &[original.as_str()],
            "CIFF:DateTimeOriginal",
        ),
        (
            Source {
                image: "ExifTool.jpg",
                setup: &[],
            },
            &[modify.as_str()],
            "MIE",
        ),
        (
            CANON_BOTH,
            &[modify.as_str(), exif_modify],
            "Cannot combine",
        ),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (index, (source, args, named)) in cases.into_iter().enumerate() {
        let Some(path) = materialize(oracle, source, dir.path(), &index.to_string()) else {
            eprintln!("skipping: pinned fixture {} is absent", source.image);
            return;
        };
        let before = std::fs::read(&path).unwrap();
        let output = oxidex()
            .arg("-overwrite_original")
            .args(args)
            .arg(&path)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?}: not refused");
        assert!(stderr.contains(named), "{args:?}: {stderr}");
        assert_eq!(std::fs::read(&path).unwrap(), before, "{args:?}");
    }
}

/// A move this writer cannot make is refused by name, the file left
/// byte-identical: IFD0 Artist, set grouped or ungrouped, is written by the
/// generated writer, which does not admit the `ExifIFD:` qualifier the
/// deletion of the ExifIFD copy needs. Before, the set succeeded and the
/// ExifIFD copy stayed.
#[test]
fn a_move_the_writer_cannot_make_is_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut index = 0;
    for source in [CANON_BOTH_ARTIST, TIFF_BOTH] {
        for arg in ["-IFD0:Artist=New", "-Artist=New"] {
            index += 1;
            let Some(path) = materialize(oracle, source, dir.path(), &index.to_string()) else {
                eprintln!("skipping: pinned fixture {} is absent", source.image);
                return;
            };
            let before = std::fs::read(&path).unwrap();
            let output = oxidex()
                .args(["-overwrite_original", arg])
                .arg(&path)
                .output()
                .unwrap();
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !output.status.success(),
                "{} {arg}: not refused",
                source.image
            );
            assert!(
                stderr.contains("ExifIFD:Artist"),
                "{} {arg}: {stderr}",
                source.image
            );
            assert_eq!(
                std::fs::read(&path).unwrap(),
                before,
                "{} {arg}",
                source.image
            );
        }
    }
}
