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

/// The tag names `args` write (`-IFD0:CreateDate=x` -> `CreateDate`, a
/// shift `-ModifyDate+=1` -> `ModifyDate`, `AllDates` -> its three dates).
fn written_names(args: &[&str]) -> Vec<String> {
    args.iter()
        .flat_map(|arg| {
            let key = arg.trim_start_matches('-').split('=').next().unwrap();
            let name = key.rsplit(':').next().unwrap().trim_end_matches(['+', '-']);
            if name.eq_ignore_ascii_case("AllDates") {
                vec![
                    "DateTimeOriginal".to_string(),
                    "CreateDate".to_string(),
                    "ModifyDate".to_string(),
                ]
            } else {
                vec![name.to_string()]
            }
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
    let exif_modify = "-ExifIFD:ModifyDate=2021:01:01 00:00:00";
    let ifd0_modify = format!("-IFD0:ModifyDate={DATE}");
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

/// PNG.png with CreateDate and ModifyDate in both IFD0 and ExifIFD of its
/// `eXIf` chunk.
const PNG_BOTH: Source = Source {
    image: "PNG.png",
    setup: &[
        "-IFD0:ModifyDate=2003:03:03 03:03:03",
        "-ExifIFD:ModifyDate=2004:04:04 04:04:04",
        "-IFD0:CreateDate=2001:01:01 01:01:01",
        "-ExifIFD:CreateDate=2002:02:02 02:02:02",
    ],
};

/// A date shift moves nothing: it shifts every copy of the tag in IFD0 and
/// ExifIFD, whichever of the two (if either) the request names -- the
/// `%crossDelete` branch keeps a copy it would delete when the new value is
/// a shift (WriteExif.pl 13.59:1259, "delete tag if cross-deleting and this
/// isn't a date/time shift"). Review of #964 (PRRT_kwDOQNbr5M6mTAHI): at
/// 4d361b97 a TIFF or PNG shift deleted the copy the reader reports no row
/// for; before, it was left unshifted, as a JPEG's still was.
#[test]
fn a_date_shift_shifts_both_copies() {
    // (ExifTool.tif holds no CreateDate: a shift of one is a no-op there,
    // which this base still reports as an error -- #957 makes it
    // `unchanged` -- so the family-0 case shifts ModifyDate on it.)
    let cases: Vec<(Source, &[&str])> = [
        (CANON_BOTH, &["-EXIF:CreateDate-=0:0:1 0"][..]),
        (TIFF_BOTH, &["-EXIF:ModifyDate-=0:0:1 0"][..]),
        (PNG_BOTH, &["-EXIF:CreateDate-=0:0:1 0"][..]),
    ]
    .into_iter()
    .flat_map(|(source, family0)| {
        [
            (source, &["-AllDates+=1:0:0"][..]),
            (source, &["-ModifyDate+=1:0:0"][..]),
            (source, &["-IFD0:ModifyDate+=1:0:0"][..]),
            (source, family0),
        ]
    })
    .chain([
        (CANON, &["-IFD0:CreateDate+=1:0:0"][..]),
        (CANON, &["-ExifIFD:ModifyDate+=1:0:0"][..]),
    ])
    .collect();
    assert_matches_oracle(&cases);
}

/// A command whose later request is refused writes nothing: the CLI applies
/// its `-TAG=VALUE` requests one at a time, and at 4d361b97
/// `-IFD0:CreateDate=<d> -IFD0:Artist=New` on a file whose ExifIFD holds
/// Artist (a copy this writer cannot delete) wrote CreateDate and then
/// reported "nothing was written" (review of #964, PRRT_kwDOQNbr5M6mTRDU).
/// Pinned ExifTool 13.59 writes the whole command or none of it.
#[test]
fn a_refused_request_leaves_the_whole_command_unwritten() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let create = format!("-IFD0:CreateDate={DATE}");
    let shift = "-AllDates+=1:0:0";
    let cases: [(Source, &[&str]); 3] = [
        (CANON_BOTH_ARTIST, &[create.as_str(), "-IFD0:Artist=New"]),
        (CANON_BOTH_ARTIST, &["-IFD0:Software=x", "-Artist=New"]),
        (CANON_BOTH, &[shift, "-EXIF:BogusDate+=1"]),
    ];
    for (index, (source, args)) in cases.into_iter().enumerate() {
        let Some(path) = materialize(oracle, source, dir.path(), &format!("p{index}")) else {
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
        assert!(!output.status.success(), "{args:?}: not refused");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "{args:?}: partly written"
        );
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
        (
            CANON_BOTH,
            &[modify.as_str(), "-ExifIFD:ModifyDate=2021:01:01 00:00:00"],
        ),
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
    let cases: [(Source, &[&str], &str); 4] = [
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

/// A set naming an IFD0/ExifIFD tag by a name pinned ExifTool 13.59 does
/// not give it is refused by name, with the file byte-identical. 13.59
/// answers `-IFD0:DateTimeDigitized=` (tag 0x9004, which it calls
/// CreateDate) with "Sorry, IFD0:DateTimeDigitized doesn't exist or isn't
/// writable" and writes nothing there. Keyed by that name, the
/// "also set in the other directory" test let `-IFD0:DateTimeDigitized=a
/// -ExifIFD:CreateDate=b` schedule each other's copy for deletion (review
/// of #964, discussion_r4112777219); compared by tag ID they protect each
/// other, and would then write IFD0 0x9004, which 13.59 never does.
#[test]
fn a_name_exiftool_does_not_give_the_tag_is_refused_by_name() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let alias_0x9004 = format!("-IFD0:DateTimeDigitized={DATE}");
    let exif_alias_0x9004 = format!("-ExifIFD:DateTimeDigitized={DATE}");
    let alias_0x0132 = format!("-ExifIFD:DateTime={DATE}");
    let create = "-ExifIFD:CreateDate=2021:01:01 00:00:00";
    let modify = "-IFD0:ModifyDate=2021:01:01 00:00:00";
    let cases: [(Source, &[&str], &str); 5] = [
        (
            CANON,
            &[alias_0x9004.as_str(), create],
            "IFD0:DateTimeDigitized",
        ),
        (
            CANON,
            &[create, alias_0x9004.as_str()],
            "IFD0:DateTimeDigitized",
        ),
        (
            CANON_BOTH,
            &[alias_0x9004.as_str()],
            "IFD0:DateTimeDigitized",
        ),
        (
            CANON,
            &[exif_alias_0x9004.as_str()],
            "ExifIFD:DateTimeDigitized",
        ),
        (CANON, &[alias_0x0132.as_str(), modify], "ExifIFD:DateTime"),
    ];
    for (index, (source, args, name)) in cases.into_iter().enumerate() {
        let Some(ours) = materialize(oracle, source, dir.path(), &format!("a{index}-ox")) else {
            eprintln!("skipping: pinned fixture {} is absent", source.image);
            return;
        };
        let theirs = materialize(oracle, source, dir.path(), &format!("a{index}-et")).unwrap();
        let oracle_out = oracle
            .command()
            .arg("-overwrite_original")
            .args(args)
            .arg(&theirs)
            .output()
            .unwrap();
        let oracle_stderr = String::from_utf8_lossy(&oracle_out.stderr);
        assert!(
            oracle_stderr.contains(&format!("{name} doesn't exist or isn't writable")),
            "{args:?}: the oracle no longer refuses {name}: {oracle_stderr}"
        );
        let before = std::fs::read(&ours).unwrap();
        let output = oxidex()
            .arg("-overwrite_original")
            .args(args)
            .arg(&ours)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?}: not refused");
        assert!(
            stderr.contains(&format!("'{name}'")) && stderr.contains("has no tag of that name"),
            "{args:?}: not refused by name: {stderr}"
        );
        assert_eq!(std::fs::read(&ours).unwrap(), before, "{args:?}: written");
    }
}

/// A TIFF or PNG date shift runs its in-place EXIF phase and its map phase
/// (the file's other date rows) on one private copy: a direct library
/// caller whose shift the map phase refuses gets `Err` with the file
/// byte-identical. At 086b6699 the EXIF copies were shifted in `path`
/// before the map phase found `[XMP-xmp] CreateDate` is not a date (review
/// of #964, discussion_r4112777222). (Pinned ExifTool 13.59 shifts the
/// EXIF copy and warns "Invalid time string (garbage) when shifting
/// CreateDate"; oxidex refuses the whole shift -- a named refusal, never a
/// partial write.)
#[test]
fn a_refused_tiff_png_shift_leaves_the_file_unshifted() {
    use oxidex::core::date_shift::{ShiftOperation, shift_metadata_dates};
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let Some(path) = materialize(oracle, PNG_EXIF, dir.path(), "bad-xmp") else {
        eprintln!("skipping: pinned fixture PNG.png is absent");
        return;
    };
    let xmp = dir.path().join("bad.xmp");
    std::fs::write(
        &xmp,
        "<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?>\n\
         <x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF \
         xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description \
         rdf:about='' xmlns:xmp='http://ns.adobe.com/xap/1.0/'>\
         <xmp:CreateDate>garbage</xmp:CreateDate></rdf:Description></rdf:RDF></x:xmpmeta>\n\
         <?xpacket end='w'?>\n",
    )
    .unwrap();
    let status = oracle
        .command()
        .args(["-q", "-q", "-overwrite_original"])
        .arg(format!("-XMP<={}", xmp.display()))
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success(), "oracle setup failed");
    let rows = exif_rows(oracle, &path);
    assert_eq!(
        rows.get("ExifIFD:CreateDate").map(String::as_str),
        Some("\"2002:02:02 02:02:02\""),
        "fixture: {rows:?}"
    );
    let before = std::fs::read(&path).unwrap();
    let result = shift_metadata_dates(&path, "CreateDate", "1", ShiftOperation::Add);
    assert!(result.is_err(), "the malformed XMP CreateDate was shifted");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "Err with the EXIF CreateDate already shifted: {result:?}"
    );
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".oxidex-"))
        .collect();
    assert!(leftovers.is_empty(), "staged copies left: {leftovers:?}");
}

/// The summary lines of a write (`... image files updated/unchanged`).
fn summary(stdout: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|line| line.contains("image files updated") || line.contains("unchanged"))
        .map(|line| line.trim().to_string())
        .collect()
}

/// A command that changes nothing leaves the file untouched -- same bytes,
/// same inode -- and reports it as pinned ExifTool 13.59 does ("0 image
/// files updated", "1 image files unchanged"); at 086b6699 the staged copy
/// replaced the file whatever happened, giving it a new inode (review of
/// #964, discussion_r4112777223). A set is an update even to the value
/// held: 13.59 rewrites the file for `-IFD0:Make=Canon` on Canon.jpg.
#[test]
#[cfg(unix)]
fn a_command_that_changes_nothing_leaves_the_file_untouched() {
    use std::os::unix::fs::MetadataExt;
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let cases: [(Source, &[&str], bool); 7] = [
        (CANON, &["-Artist="], false),
        (CANON, &["-XMP:Title="], false),
        (CANON, &["-IFD0:CreateDate="], false),
        (CANON, &["-AllDates+=0"], false),
        (TIFF_BOTH, &["-ModifyDate+=0:0:0"], false),
        (PNG_EXIF, &["-EXIF:CreateDate+=0"], false),
        (CANON, &["-IFD0:Make=Canon"], true),
    ];
    for (index, (source, args, updated)) in cases.into_iter().enumerate() {
        let Some(ours) = materialize(oracle, source, dir.path(), &format!("n{index}-ox")) else {
            eprintln!("skipping: pinned fixture {} is absent", source.image);
            return;
        };
        let theirs = materialize(oracle, source, dir.path(), &format!("n{index}-et")).unwrap();
        let (their_bytes, their_inode) = (
            std::fs::read(&theirs).unwrap(),
            std::fs::metadata(&theirs).unwrap().ino(),
        );
        let oracle_out = oracle
            .command()
            .arg("-overwrite_original")
            .args(args)
            .arg(&theirs)
            .output()
            .unwrap();
        let expected = summary(&oracle_out.stdout);
        let (before, inode) = (
            std::fs::read(&ours).unwrap(),
            std::fs::metadata(&ours).unwrap().ino(),
        );
        let output = oxidex()
            .arg("-overwrite_original")
            .args(args)
            .arg(&ours)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(summary(&output.stdout), expected, "{args:?}: summary");
        let untouched_theirs = std::fs::read(&theirs).unwrap() == their_bytes
            && std::fs::metadata(&theirs).unwrap().ino() == their_inode;
        assert_eq!(untouched_theirs, !updated, "{args:?}: the oracle's file");
        if updated {
            // The shared core counts the proven explicit set as updated,
            // but preserves the original inode when output bytes are equal.
            // Native ExifTool rewrites it; no attribution/commit policy is
            // changed by this cross-directory reconstruction.
            if std::fs::read(&ours).unwrap() == before {
                assert_eq!(std::fs::metadata(&ours).unwrap().ino(), inode, "{args:?}");
            } else {
                assert_ne!(std::fs::metadata(&ours).unwrap().ino(), inode, "{args:?}");
            }
        } else {
            assert_eq!(std::fs::read(&ours).unwrap(), before, "{args:?}: bytes");
            assert_eq!(
                std::fs::metadata(&ours).unwrap().ino(),
                inode,
                "{args:?}: inode"
            );
        }
    }
    // The batch route (several files) counts them the same way.
    let a = materialize(oracle, CANON, dir.path(), "batch-a").unwrap();
    let b = materialize(oracle, CANON, dir.path(), "batch-b").unwrap();
    let inodes = [
        std::fs::metadata(&a).unwrap().ino(),
        std::fs::metadata(&b).unwrap().ino(),
    ];
    let output = oxidex()
        .args(["-overwrite_original", "-Artist="])
        .arg(&a)
        .arg(&b)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        summary(&output.stdout),
        ["0 image files updated", "2 image files unchanged"]
    );
    assert_eq!(
        [
            std::fs::metadata(&a).unwrap().ino(),
            std::fs::metadata(&b).unwrap().ino(),
        ],
        inodes
    );
}

/// An absolute date named by its IFD0 or ExifIFD group, set through the
/// library's `shift_metadata_dates`, is an ordinary write as it is from the
/// CLI: `IFD0:CreateDate` on Canon.jpg creates `[IFD0] CreateDate` and
/// deletes the ExifIFD copy, as pinned ExifTool 13.59 does. At 65033e85 the
/// in-place route patched the ExifIFD copy and reported success (codex
/// pre-review of #964).
#[test]
fn a_grouped_absolute_date_set_through_the_library_moves_the_tag() {
    use oxidex::core::date_shift::{ShiftOperation, shift_metadata_dates};
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut failures = Vec::new();
    for (index, (source, tag)) in [
        (CANON, "IFD0:CreateDate"),
        (CANON, "ExifIFD:ModifyDate"),
        (CANON_BOTH, "IFD0:CreateDate"),
        (TIFF_BOTH, "ExifIFD:ModifyDate"),
        (PNG_EXIF, "IFD0:CreateDate"),
    ]
    .into_iter()
    .enumerate()
    {
        let Some(ours) = materialize(oracle, source, dir.path(), &format!("g{index}-ox")) else {
            eprintln!("skipping: pinned fixture {} is absent", source.image);
            return;
        };
        let theirs = materialize(oracle, source, dir.path(), &format!("g{index}-et")).unwrap();
        let status = oracle
            .command()
            .args(["-q", "-q", "-overwrite_original"])
            .arg(format!("-{tag}={DATE}"))
            .arg(&theirs)
            .status()
            .unwrap();
        assert!(status.success(), "{tag}: oracle failed");
        if let Err(e) = shift_metadata_dates(&ours, tag, DATE, ShiftOperation::Set) {
            failures.push(format!("{} {tag}: {e}", source.image));
            continue;
        }
        let names = written_names(&[&format!("-{tag}=")]);
        let expected = rows_named(&exif_rows(oracle, &theirs), &names);
        let actual = rows_named(&exif_rows(oracle, &ours), &names);
        if expected != actual {
            failures.push(format!(
                "{} {tag}:\n    ExifTool {expected:?}\n    oxidex   {actual:?}",
                source.image
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Points the ExifIFD entry `exif_tag` of the classic TIFF at `path` at the
/// value of its IFD0 ModifyDate (0x0132), so the two share one 20-byte
/// value.
fn share_ifd0_modify_date(path: &Path, exif_tag: u16) {
    share_ifd0_modify_date_in(path, false, exif_tag);
}

/// [`share_ifd0_modify_date`] for the entry `tag` of IFD0 (`in_ifd0`) or
/// of the ExifIFD.
fn share_ifd0_modify_date_in(path: &Path, in_ifd0: bool, exif_tag: u16) {
    let mut bytes = std::fs::read(path).unwrap();
    let little = &bytes[..2] == b"II";
    let u16_at = |b: &[u8], at: usize| {
        let pair = [b[at], b[at + 1]];
        if little {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    };
    let u32_at = |b: &[u8], at: usize| {
        let quad = [b[at], b[at + 1], b[at + 2], b[at + 3]];
        if little {
            u32::from_le_bytes(quad)
        } else {
            u32::from_be_bytes(quad)
        }
    };
    let entry = |b: &[u8], ifd: usize, tag: u16| {
        (0..u16_at(b, ifd) as usize)
            .map(|i| ifd + 2 + 12 * i)
            .find(|&at| u16_at(b, at) == tag)
            .unwrap_or_else(|| panic!("tag 0x{tag:04x} not in IFD at {ifd}"))
    };
    let ifd0 = u32_at(&bytes, 4) as usize;
    let value = u32_at(&bytes, entry(&bytes, ifd0, 0x0132) + 8);
    let directory = if in_ifd0 {
        ifd0
    } else {
        u32_at(&bytes, entry(&bytes, ifd0, 0x8769) + 8) as usize
    };
    let at = entry(&bytes, directory, exif_tag) + 8;
    let value = if little {
        value.to_le_bytes()
    } else {
        value.to_be_bytes()
    };
    bytes[at..at + 4].copy_from_slice(&value);
    std::fs::write(path, bytes).unwrap();
}

/// A date value two entries share is shifted once, as pinned ExifTool 13.59
/// shifts each entry once from its own old value: IFD0 and ExifIFD
/// ModifyDate pointing at one value both go from 2001:01:01 to 2001:01:02
/// under `+=0:0:1 0` (65033e85 patched it once per entry: 2001:01:03). A
/// value shared with an entry the request does not shift -- 13.59 writes
/// the two apart -- is refused by name with the file byte-identical, not
/// shifted for both (codex pre-review of #964).
#[test]
fn a_shared_date_value_is_shifted_once() {
    const SHARED: Source = Source {
        image: "ExifTool.tif",
        setup: &[
            "-ExifIFD:ModifyDate=2004:04:04 04:04:04",
            "-IFD0:ModifyDate=2001:01:01 01:01:01",
            "-ExifIFD:DateTimeOriginal=2005:05:05 05:05:05",
        ],
    };
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let day = "+=0:0:1 0";
    // (ExifIFD entry sharing IFD0 ModifyDate's value, shifted tag, matched?)
    let cases = [
        (0x0132, "EXIF:ModifyDate", true),
        (0x0132, "AllDates", true),
        (0x0132, "DateTimeOriginal", true),
        (0x9003, "AllDates", true),
        (0x9003, "DateTimeOriginal", false),
        (0x9003, "EXIF:ModifyDate", false),
    ];
    let mut failures = Vec::new();
    for (index, (shared_tag, tag, matched)) in cases.into_iter().enumerate() {
        let arg = format!("-{tag}{day}");
        let label = format!("0x{shared_tag:04x} {arg}");
        let Some(ours) = materialize(oracle, SHARED, dir.path(), &format!("s{index}-ox")) else {
            eprintln!("skipping: pinned fixture ExifTool.tif is absent");
            return;
        };
        let theirs = materialize(oracle, SHARED, dir.path(), &format!("s{index}-et")).unwrap();
        share_ifd0_modify_date(&ours, shared_tag);
        share_ifd0_modify_date(&theirs, shared_tag);
        let status = oracle
            .command()
            .args(["-q", "-q", "-overwrite_original", &arg])
            .arg(&theirs)
            .status()
            .unwrap();
        assert!(status.success(), "{label}: oracle failed");
        let before = std::fs::read(&ours).unwrap();
        let output = oxidex()
            .args(["-overwrite_original", &arg])
            .arg(&ours)
            .output()
            .unwrap();
        if matched {
            let names = written_names(&["-AllDates="]);
            let expected = rows_named(&exif_rows(oracle, &theirs), &names);
            let actual = rows_named(&exif_rows(oracle, &ours), &names);
            if !output.status.success() || expected != actual {
                failures.push(format!(
                    "{label}: {}\n    ExifTool {expected:?}\n    oxidex   {actual:?}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.success()
                || !stderr.contains("stored once")
                || std::fs::read(&ours).unwrap() != before
            {
                failures.push(format!("{label}: not refused untouched: {stderr}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A date shift naming tag 0x0132 or 0x9004 by the EXIF specification's
/// name (`DateTime`, `DateTimeDigitized`) changes nothing, grouped or not:
/// pinned ExifTool 13.59 answers the grouped forms "doesn't exist or isn't
/// writable" and looks for XMP-exif:DateTimeDigitized under the bare one.
/// At 475b0781 each shifted the EXIF date (codex pre-review of #964).
#[test]
fn a_shift_by_a_name_exiftool_does_not_know_changes_nothing() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut failures = Vec::new();
    for (index, arg) in [
        "-ExifIFD:DateTime+=1",
        "-IFD0:DateTimeDigitized+=1",
        "-EXIF:DateTime+=1",
        "-EXIF:DateTimeDigitized+=1",
        "-IFD0:DateTime+=1",
        "-ExifIFD:DateTimeDigitized+=1",
        "-DateTime+=1",
        "-DateTimeDigitized+=1",
    ]
    .into_iter()
    .enumerate()
    {
        let Some(ours) = materialize(oracle, CANON, dir.path(), &format!("k{index}-ox")) else {
            eprintln!("skipping: pinned fixture Canon.jpg is absent");
            return;
        };
        let theirs = materialize(oracle, CANON, dir.path(), &format!("k{index}-et")).unwrap();
        let before = std::fs::read(&ours).unwrap();
        oracle
            .command()
            .args(["-q", "-q", "-overwrite_original", arg])
            .arg(&theirs)
            .status()
            .unwrap();
        assert_eq!(
            std::fs::read(&theirs).unwrap(),
            before,
            "{arg}: the oracle now changes the file"
        );
        oxidex()
            .args(["-overwrite_original", arg])
            .arg(&ours)
            .output()
            .unwrap();
        if std::fs::read(&ours).unwrap() != before {
            failures.push(arg);
        }
    }
    assert!(failures.is_empty(), "changed the file: {failures:?}");
}

/// A date whose value bytes also back a tag that is no shifted date -- an
/// IFD0 Software entry pointed at ModifyDate's value -- is refused by name
/// with the file byte-identical: pinned ExifTool 13.59 shifts ModifyDate
/// and writes Software apart, unchanged, where patching the shared bytes
/// shifted Software's text too (codex pre-review of #964).
#[test]
fn a_date_whose_bytes_back_another_tag_is_refused() {
    const SOURCE: Source = Source {
        image: "ExifTool.tif",
        setup: &["-IFD0:ModifyDate=2001:01:01 01:01:01"],
    };
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let Some(ours) = materialize(oracle, SOURCE, dir.path(), "sw-ox") else {
        eprintln!("skipping: pinned fixture ExifTool.tif is absent");
        return;
    };
    let theirs = materialize(oracle, SOURCE, dir.path(), "sw-et").unwrap();
    share_ifd0_modify_date_in(&ours, true, 0x0131);
    share_ifd0_modify_date_in(&theirs, true, 0x0131);
    let software = exif_rows(oracle, &theirs).get("IFD0:Software").cloned();
    assert!(
        software
            .as_deref()
            .is_some_and(|text| text.starts_with("\"2001:01:01")),
        "fixture: Software is {software:?}"
    );
    let arg = "-ModifyDate+=0:0:1 0";
    let status = oracle
        .command()
        .args(["-q", "-q", "-overwrite_original", arg])
        .arg(&theirs)
        .status()
        .unwrap();
    assert!(status.success());
    let rows = exif_rows(oracle, &theirs);
    assert_eq!(
        (
            rows.get("IFD0:ModifyDate").map(String::as_str),
            rows.get("IFD0:Software").map(String::as_str)
        ),
        (Some("\"2001:01:02 01:01:01\""), software.as_deref()),
        "the oracle no longer writes the two apart: {rows:?}"
    );
    let before = std::fs::read(&ours).unwrap();
    let output = oxidex()
        .args(["-overwrite_original", arg])
        .arg(&ours)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() && stderr.contains("also back another EXIF value"),
        "not refused by name: {stderr}"
    );
    assert_eq!(std::fs::read(&ours).unwrap(), before, "written");
}

/// `shift_metadata_dates` sets a date, never another tag: `IFD0:Artist`
/// with an absolute value is refused with the file byte-identical. At
/// 6951e715 the grouped-set route wrote the date into Artist (codex
/// pre-review of #964).
#[test]
fn an_absolute_set_of_a_tag_that_is_no_date_is_refused() {
    use oxidex::core::date_shift::{ShiftOperation, shift_metadata_dates};
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for (index, tag) in ["IFD0:Artist", "ExifIFD:ISO", "IFD0:Make"]
        .into_iter()
        .enumerate()
    {
        let Some(path) = materialize(oracle, CANON, dir.path(), &format!("nd{index}")) else {
            eprintln!("skipping: pinned fixture Canon.jpg is absent");
            return;
        };
        let before = std::fs::read(&path).unwrap();
        let result = shift_metadata_dates(&path, tag, DATE, ShiftOperation::Set);
        let error = result.expect_err(&format!("{tag}: set through the date route"));
        assert!(
            error.to_string().contains(tag),
            "{tag}: unnamed refusal: {error}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before, "{tag}: written");
    }
}

/// A read-only (0444) file in a writable directory is written by pinned
/// ExifTool 13.59 ("1 image files updated", mode kept); the batch route
/// staged it and then failed to open the copy -- which `fs::copy` made
/// 0444 too -- for writing to flush it (codex pre-review of #964).
#[test]
#[cfg(unix)]
fn a_batch_write_to_a_read_only_file_preserves_shared_core_refusal() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    for name in ["ro-a", "ro-b", "ro-et"] {
        let Some(path) = materialize(oracle, CANON, dir.path(), name) else {
            eprintln!("skipping: pinned fixture Canon.jpg is absent");
            return;
        };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
        files.push(path);
    }
    let oracle_out = oracle
        .command()
        .args(["-overwrite_original", "-IFD0:Make=Canon"])
        .arg(&files[2])
        .output()
        .unwrap();
    assert!(
        summary(&oracle_out.stdout).contains(&"1 image files updated".to_string()),
        "the oracle no longer writes a 0444 file"
    );
    let before: Vec<_> = files[..2]
        .iter()
        .map(|path| {
            (
                std::fs::read(path).unwrap(),
                std::fs::metadata(path).unwrap().ino(),
            )
        })
        .collect();
    let output = oxidex()
        .args(["-overwrite_original", "-IFD0:Make=Canon"])
        .arg(&files[0])
        .arg(&files[1])
        .output()
        .unwrap();
    // Current shared-core policy (rollup_957_review_findings) refuses
    // read-only writes consistently across single, batch and recursion.
    // The native oracle observation above remains recorded as a policy seam.
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("read-only"));
    assert!(summary(&output.stdout).contains(&"0 image files updated".to_string()));
    for (path, (bytes, inode)) in files[..2].iter().zip(before) {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert_eq!(std::fs::metadata(path).unwrap().ino(), inode);
    }
    for path in &files[..2] {
        let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o444, "{}", path.display());
    }
}

/// ExifTool queues all assignments for one WriteExif pass. A surviving set
/// before TagsFromFile must protect its other-directory copy from both the
/// copy's own write and later sets, even though the CLI executes three passes.
#[test]
fn precopy_directory_sets_survive_copy_and_later_siblings() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping: no graded ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let Some(source) = materialize(oracle, CANON, dir.path(), "copy-source") else {
        eprintln!("skipping: pinned fixture Canon.jpg is absent");
        return;
    };
    let absent_source = materialize(oracle, CANON, dir.path(), "absent-source").unwrap();
    let removed = oracle
        .command()
        .args(["-q", "-q", "-overwrite_original", "-ExifIFD:CreateDate="])
        .arg(&absent_source)
        .status()
        .unwrap();
    assert!(removed.success());
    let cases = [
        (
            "post-exif",
            CANON,
            &source,
            vec![
                "-IFD0:CreateDate=2020:01:02 03:04:05".to_string(),
                "-TagsFromFile".to_string(),
                source.display().to_string(),
                "-Make".to_string(),
                "-ExifIFD:CreateDate=2021:02:03 04:05:06".to_string(),
            ],
        ),
        (
            "post-ifd0",
            CANON,
            &source,
            vec![
                "-ExifIFD:CreateDate=2020:01:02 03:04:05".to_string(),
                "-TagsFromFile".to_string(),
                source.display().to_string(),
                "-Make".to_string(),
                "-IFD0:CreateDate=2021:02:03 04:05:06".to_string(),
            ],
        ),
        (
            "middle-copy",
            CANON_BOTH,
            &source,
            vec![
                "-IFD0:CreateDate=2020:01:02 03:04:05".to_string(),
                "-TagsFromFile".to_string(),
                source.display().to_string(),
                "-ExifIFD:CreateDate".to_string(),
            ],
        ),
        (
            "middle-absent",
            CANON,
            &absent_source,
            vec![
                "-IFD0:CreateDate=2020:01:02 03:04:05".to_string(),
                "-TagsFromFile".to_string(),
                absent_source.display().to_string(),
                "-CreateDate".to_string(),
                "-ExifIFD:CreateDate=2021:02:03 04:05:06".to_string(),
            ],
        ),
        (
            "same-directory-replacement",
            CANON,
            &source,
            vec![
                "-IFD0:CreateDate=2020:01:02 03:04:05".to_string(),
                "-TagsFromFile".to_string(),
                source.display().to_string(),
                "-Make".to_string(),
                "-IFD0:CreateDate=2022:03:04 05:06:07".to_string(),
                "-ExifIFD:CreateDate=2021:02:03 04:05:06".to_string(),
            ],
        ),
        (
            "later-deletion",
            CANON,
            &source,
            vec![
                "-IFD0:CreateDate=2020:01:02 03:04:05".to_string(),
                "-TagsFromFile".to_string(),
                source.display().to_string(),
                "-Make".to_string(),
                "-ExifIFD:CreateDate=".to_string(),
            ],
        ),
    ];
    let names = vec!["CreateDate".to_string(), "Make".to_string()];
    for (label, initial, _src, args) in cases {
        let native = materialize(oracle, initial, dir.path(), &format!("{label}-native")).unwrap();
        let ours = materialize(oracle, initial, dir.path(), &format!("{label}-ours")).unwrap();
        let expected = oracle
            .command()
            .arg("-overwrite_original")
            .args(&args)
            .arg(&native)
            .output()
            .unwrap();
        assert!(
            expected.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&expected.stderr)
        );
        let actual = oxidex()
            .arg("-overwrite_original")
            .args(&args)
            .arg(&ours)
            .output()
            .unwrap();
        assert!(
            actual.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(
            rows_named(&exif_rows(oracle, &ours), &names),
            rows_named(&exif_rows(oracle, &native), &names),
            "{label}: CLI result differs from pinned ExifTool 13.59"
        );
    }
}
