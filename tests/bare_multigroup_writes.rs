//! CLI parity: a write of a tag name that exists in more than one writable
//! group, graded against pinned ExifTool 13.59 (`exiftool_oracle::graded()`).
//!
//! What the oracle does (evidence `multigroup-write/exp/`, pinned 13.59 on
//! t/images):
//!
//! - An ungrouped `-TAG=VALUE` creates the tag in its highest-priority group
//!   (EXIF: `[ExifIFD] WhiteBalance`, `[IFD0] CalibrationIlluminant1`) and
//!   also edits every same-named tag the file already carries in another
//!   writable group -- `-WhiteBalance#=1` on Canon.jpg writes `[ExifIFD]`
//!   and `[Canon] WhiteBalance`, on Nikon.jpg `[ExifIFD]` and `[Nikon]
//!   WhiteBalance`. A tag only a maker note defines is edited where the note
//!   holds it and never created (`-MacroMode#=2` on Nikon.jpg: `1 image files
//!   unchanged`).
//! - `-Flash=` names the writable `Composite:Flash`, whose `WriteAlso` also
//!   creates an XMP-exif Flash structure.
//! - `-EXIF:TAG=` writes only EXIF; `-MakerNotes:TAG=` only the maker note.
//! - ExifTool.jpg carries an MIE trailer: every EXIF write -- grouped
//!   (`-EXIF:`, `-IFD0:`) or not -- is also written into MIE-Meta's EXIF
//!   directory, which ExifTool creates; a deletion is applied there too
//!   where that directory holds the tag.
//!
//! oxidex writes only EXIF, so it writes exactly the requests whose ExifTool
//! result is EXIF alone and refuses the rest by name (`Cannot write tag
//! ...`), file untouched -- never a partial write, never a silent no-op.
//! Every case is written by both tools onto fresh copies and both results
//! are read back by the oracle (oxidex's reader does not surface every
//! maker-note row ExifTool edits). A case expected to match must hold every
//! group's value the oracle's write holds; a case expected to be refused
//! must be one the oracle changed the file for.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle::{self, Oracle};
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    /// `-Name=<print-converted label>`
    Printed,
    /// `-Name#=<raw value>`
    Hash,
    /// `-EXIF:Name#=<raw value>`
    Grouped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    /// oxidex writes, and every group's value equals the oracle's write.
    Match,
    /// oxidex refuses by name, file untouched; the oracle writes more than
    /// EXIF (or into MIE), which oxidex cannot.
    Refused,
}

/// (name, printed value, raw value) -- each name is also defined by a maker
/// note table (except CalibrationIlluminant1, which only EXIF defines).
const NAMES: &[(&str, &str, &str)] = &[
    ("WhiteBalance", "Manual", "1"),
    ("ColorSpace", "Uncalibrated", "2"),
    ("MeteringMode", "Spot", "3"),
    ("CalibrationIlluminant1", "D55", "20"),
    ("Contrast", "High", "2"),
    ("Saturation", "High", "2"),
    ("Sharpness", "Hard", "2"),
    ("FocalLength", "50", "50"),
    ("ISO", "200", "200"),
    ("Flash", "Off, Did not fire", "16"),
];

/// Canon.jpg (a Canon maker note), Nikon.jpg (a maker note oxidex's reader
/// decodes no row of), ExifTool.jpg (MIE) and Writer.jpg (no maker note, no
/// MIE: EXIF is the whole of ExifTool's write).
const FILES: &[&str] = &["Canon.jpg", "Nikon.jpg", "ExifTool.jpg", "Writer.jpg"];

/// The pinned outcome of every case.
fn expected(file: &str, name: &str, form: Form) -> Expect {
    match (file, form) {
        // MIE: every EXIF write, grouped or not, is also an MIE write.
        ("ExifTool.jpg", _) => Expect::Refused,
        // `-EXIF:` names one group: exactly what oxidex writes.
        (_, Form::Grouped) => Expect::Match,
        // Composite:Flash is written (with its XMP WriteAlso) everywhere.
        (_, _) if name == "Flash" => Expect::Refused,
        // Only EXIF defines it: no other group ExifTool would also write.
        (_, _) if name == "CalibrationIlluminant1" => Expect::Match,
        // No maker note, no MIE: the bare name is EXIF alone, typed by its
        // EXIF address (`-ColorSpace#=2` was a string, refused, before).
        ("Writer.jpg", _) => Expect::Match,
        // Canon.jpg: Canon's maker note may hold every other name;
        // Nikon.jpg: a maker note oxidex's reader decodes no row of, which
        // may hold any of them.
        _ => Expect::Refused,
    }
}

fn arg(name: &str, printed: &str, raw: &str, form: Form) -> String {
    match form {
        Form::Printed => format!("-{name}={printed}"),
        Form::Hash => format!("-{name}#={raw}"),
        Form::Grouped => format!("-EXIF:{name}#={raw}"),
    }
}

/// Every group's `name` row of `path` as the oracle reads it (`-a -G1 -n`).
fn oracle_rows(oracle: &Oracle, path: &Path, name: &str) -> Vec<String> {
    let out = oracle
        .command()
        .args(["-a", "-G1", "-n", "-s", "-m", &format!("-{name}")])
        .arg(path)
        .output()
        .expect("run oracle read");
    let mut rows: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.starts_with('['))
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    rows.sort();
    rows
}

/// Runs one case; `Err` describes how it departed from `expect`.
fn run_case(
    oracle: &Oracle,
    source: &Path,
    file: &str,
    (name, printed, raw): (&str, &str, &str),
    form: Form,
    expect: Expect,
) -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    let original = std::fs::read(source).unwrap();
    let et_path = dir.path().join("et.jpg");
    let ox_path = dir.path().join("ox.jpg");
    std::fs::write(&et_path, &original).unwrap();
    std::fs::write(&ox_path, &original).unwrap();
    let arg = arg(name, printed, raw, form);

    let et = oracle
        .command()
        .args(["-m", "-overwrite_original", &arg])
        .arg(&et_path)
        .output()
        .expect("run oracle write");
    let ox = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg(&arg)
        .arg(&ox_path)
        .output()
        .expect("run oxidex");
    let et_changed = std::fs::read(&et_path).unwrap() != original;
    let ox_changed = std::fs::read(&ox_path).unwrap() != original;
    let ox_stderr = String::from_utf8_lossy(&ox.stderr).into_owned();
    let et_rows = oracle_rows(oracle, &et_path, name);
    let ox_rows = oracle_rows(oracle, &ox_path, name);
    let case = format!("{file} {arg}");
    match expect {
        Expect::Match => {
            if !ox.status.success() {
                return Err(format!(
                    "{case}: expected a write, oxidex failed: {ox_stderr}"
                ));
            }
            if ox_changed != et_changed || ox_rows != et_rows {
                return Err(format!(
                    "{case}: oxidex wrote {ox_rows:?} (changed {ox_changed}), the oracle \
                     {et_rows:?} (changed {et_changed}); oracle said {}",
                    String::from_utf8_lossy(&et.stdout).trim()
                ));
            }
        }
        Expect::Refused => {
            if ox.status.success() || ox_changed || !ox_stderr.contains("Cannot write tag") {
                return Err(format!(
                    "{case}: expected a named refusal with the file untouched; oxidex exit \
                     {:?}, changed {ox_changed}, said {ox_stderr:?}{}; the oracle wrote \
                     {et_rows:?}",
                    ox.status.code(),
                    String::from_utf8_lossy(&ox.stdout).trim()
                ));
            }
            if !et_changed {
                return Err(format!(
                    "{case}: oxidex refused a request the oracle left unchanged ({})",
                    String::from_utf8_lossy(&et.stdout).trim()
                ));
            }
        }
    }
    Ok(())
}

/// Canon.jpg, Nikon.jpg, ExifTool.jpg and Writer.jpg x ten multi-group names x printed,
/// `#` and `-EXIF:` forms: each matches the oracle's write or is refused by
/// name, as [`expected`] pins.
#[test]
fn bare_multigroup_writes_match_the_oracle_or_are_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let mut failures = Vec::new();
    let mut counted = 0;
    for file in FILES {
        let source = fixtures::required_t_images_fixture_path(file);
        for &case in NAMES {
            for form in [Form::Printed, Form::Hash, Form::Grouped] {
                counted += 1;
                let expect = expected(file, case.0, form);
                if let Err(failure) = run_case(oracle, &source, file, case, form, expect) {
                    failures.push(failure);
                }
            }
        }
    }
    assert_eq!(counted, 120);
    assert!(
        failures.is_empty(),
        "{} of {counted} cases departed from the pinned outcome:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The maker-note half on its own: `-MakerNotes:WhiteBalance=` deletes
/// `[Nikon] WhiteBalance` from Nikon.jpg in pinned 13.59, a row oxidex's
/// reader does not surface. It must be refused, not reported unchanged.
#[test]
fn a_makernote_deletion_oxidex_cannot_rule_out_is_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let source = fixtures::required_t_images_fixture_path("Nikon.jpg");
    let dir = tempfile::tempdir().unwrap();
    let original = std::fs::read(&source).unwrap();
    let et_path = dir.path().join("et.jpg");
    let ox_path = dir.path().join("ox.jpg");
    std::fs::write(&et_path, &original).unwrap();
    std::fs::write(&ox_path, &original).unwrap();
    for (tool_path, is_oracle) in [(&et_path, true), (&ox_path, false)] {
        let out = if is_oracle {
            oracle
                .command()
                .args(["-m", "-overwrite_original", "-MakerNotes:WhiteBalance="])
                .arg(tool_path)
                .output()
        } else {
            Command::new(env!("CARGO_BIN_EXE_oxidex"))
                .arg("-MakerNotes:WhiteBalance=")
                .arg(tool_path)
                .output()
        }
        .unwrap();
        if is_oracle {
            assert!(out.status.success());
        } else {
            assert!(
                !out.status.success(),
                "oxidex must refuse, not report unchanged"
            );
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("Cannot write tag"),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
    assert_ne!(
        oracle_rows(oracle, &et_path, "WhiteBalance"),
        oracle_rows(oracle, &source, "WhiteBalance"),
        "the oracle edits [Nikon] WhiteBalance"
    );
    assert_eq!(std::fs::read(&ox_path).unwrap(), original);
}
