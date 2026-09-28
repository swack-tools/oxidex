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
//! - ExifTool.jpg carries an MIE trailer: every EXIF write is also written
//!   into MIE-Meta's EXIF directory, which ExifTool creates.
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
    /// A grouped EXIF write in a file carrying MIE: the oracle also writes
    /// MIE-Meta's EXIF copy; oxidex writes the main EXIF copy only. This
    /// predates the bare-name resolver and is pinned here as the known gap
    /// it is: oxidex's rows are the oracle's less the MIE copy.
    MieGap,
    /// oxidex's writer declines the whole request (an explicit error, not a
    /// resolver refusal), file untouched; the oracle changed the file.
    Declined,
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
        // MIE: every EXIF write is also an MIE write; a bare name is
        // refused, a grouped one keeps its pre-existing EXIF-only write.
        ("ExifTool.jpg", Form::Grouped) => Expect::MieGap,
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
    let original = std::fs::read(source).unwrap();
    let args = [arg(name, printed, raw, form)];
    run_args(oracle, &original, "jpg", file, name, &args, expect)
}

/// Writes `args` (one command line) with both tools onto fresh copies of
/// `original` (extension `ext`) and grades every group's `name` rows, read
/// back by the oracle, against `expect`.
fn run_args(
    oracle: &Oracle,
    original: &[u8],
    ext: &str,
    file: &str,
    name: &str,
    args: &[String],
    expect: Expect,
) -> Result<(), String> {
    let dir = tempfile::tempdir().unwrap();
    let et_path = dir.path().join(format!("et.{ext}"));
    let ox_path = dir.path().join(format!("ox.{ext}"));
    std::fs::write(&et_path, original).unwrap();
    std::fs::write(&ox_path, original).unwrap();

    let et = oracle
        .command()
        .args(["-m", "-overwrite_original"])
        .args(args)
        .arg(&et_path)
        .output()
        .expect("run oracle write");
    let ox = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(&ox_path)
        .output()
        .expect("run oxidex");
    let et_changed = std::fs::read(&et_path).unwrap() != original;
    let ox_changed = std::fs::read(&ox_path).unwrap() != original;
    let ox_stderr = String::from_utf8_lossy(&ox.stderr).into_owned();
    let et_rows = oracle_rows(oracle, &et_path, name);
    let ox_rows = oracle_rows(oracle, &ox_path, name);
    let case = format!("{file} {}", args.join(" "));
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
        Expect::MieGap => {
            let main_copy = ox_rows.iter().all(|row| et_rows.contains(row));
            if !ox.status.success() || !main_copy || et_rows.len() <= ox_rows.len() {
                return Err(format!(
                    "{case}: expected oxidex's EXIF-only write beside the oracle's extra MIE \
                     copy; oxidex {ox_rows:?} ({ox_stderr}), the oracle {et_rows:?}"
                ));
            }
        }
        Expect::Declined => {
            if ox.status.success() || ox_changed || !et_changed {
                return Err(format!(
                    "{case}: expected oxidex to decline with the file untouched beside the \
                     oracle's write; oxidex exit {:?}, changed {ox_changed}, said \
                     {ox_stderr:?}; the oracle changed {et_changed}, wrote {et_rows:?}",
                    ox.status.code()
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

/// One review-round case: file label, bytes, extension, graded name, the
/// command line, and its pinned outcome.
type Case<'a> = (&'a str, &'a [u8], &'a str, &'a str, Vec<String>, Expect);

/// `original` with the first EXIF APP1 of `donor` inserted after its own
/// first EXIF APP1: two EXIF blocks, as no corpus file has.
fn with_second_app1(original: &[u8], donor: &[u8]) -> Vec<u8> {
    fn first_app1(data: &[u8]) -> (usize, usize) {
        let mut at = 2;
        loop {
            let len = usize::from(u16::from_be_bytes([data[at + 2], data[at + 3]]));
            if data[at + 1] == 0xE1 && data[at + 4..].starts_with(b"Exif\0\0") {
                return (at, at + 2 + len);
            }
            at += 2 + len;
        }
    }
    let (_, end) = first_app1(original);
    let (donor_start, donor_end) = first_app1(donor);
    [
        &original[..end],
        &donor[donor_start..donor_end],
        &original[end..],
    ]
    .concat()
}

/// The shapes PR #960's review found, each graded against the pinned
/// oracle (evidence `multigroup-write/review/`):
///
/// - an IFD0 `DNGPrivateData` with no Adobe `MakN` record holds no maker
///   note: `-Contrast#=2` is EXIF alone (DNG.dng with the record renamed);
/// - a 0x927C that is a JPEG is `ProcessUnknownOrPreview`'s PreviewImage,
///   no maker note (SamsungDigimax370.jpg);
/// - a request that deletes the maker note leaves no copy to edit, in
///   either order (`-MakerNotes:All= -WhiteBalance#=1` on Canon.jpg);
/// - a value-typed note (NikonLS-50.jpg's `LSI1`) holds no tags, but two
///   EXIF APP1s -- one with Nikon.jpg's note -- are both written by ExifTool
///   and refused here;
/// - `-MakerNotes:FocusMode=` on Nikon.jpg deletes a row oxidex's reader
///   does not surface: refused, not reported unchanged.
#[test]
fn review_round_shapes_match_the_oracle_or_are_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let read = |path: std::path::PathBuf| std::fs::read(path).unwrap();
    let canon = read(fixtures::required_t_images_fixture_path("Canon.jpg"));
    let nikon = read(fixtures::required_t_images_fixture_path("Nikon.jpg"));
    let Some(dng) = fixtures::pinned_combined_fixture_path("DNG.dng").map(read) else {
        return;
    };
    let Some(digimax) =
        fixtures::pinned_combined_fixture_path("Samsung/SamsungDigimax370.jpg").map(read)
    else {
        return;
    };
    let Some(lsi) = fixtures::pinned_combined_fixture_path("Nikon/NikonLS-50.jpg").map(read) else {
        return;
    };
    let makn = b"Adobe\0MakN";
    let at = dng
        .windows(makn.len())
        .position(|window| window == makn)
        .expect("DNG.dng carries an Adobe MakN record");
    let mut foreign = dng.clone();
    foreign[at + 6..at + 10].copy_from_slice(b"XxxN");
    // The renamed record is unsupported. Its payload may still contain the
    // bytes MakN; only record-boundary identifiers name maker-note records.
    let mut foreign_payload = foreign.clone();
    foreign_payload[at + 14..at + 18].copy_from_slice(b"MakN");
    let two_app1 = with_second_app1(&nikon, &lsi);
    let s = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    let cases: Vec<Case<'_>> = vec![
        (
            "DNG.dng (MakN renamed)",
            &foreign,
            "dng",
            "Contrast",
            s(&["-Contrast#=2"]),
            Expect::Match,
        ),
        (
            "DNG.dng (MakN inside unsupported record payload)",
            &foreign_payload,
            "dng",
            "Contrast",
            s(&["-Contrast#=2"]),
            Expect::Match,
        ),
        (
            "DNG.dng",
            &dng,
            "dng",
            "MeteringMode",
            s(&["-MeteringMode#=2"]),
            Expect::Refused,
        ),
        (
            "SamsungDigimax370.jpg",
            &digimax,
            "jpg",
            "WhiteBalance",
            s(&["-WhiteBalance#=1"]),
            Expect::Match,
        ),
        (
            "SamsungDigimax370.jpg",
            &digimax,
            "jpg",
            "Contrast",
            s(&["-Contrast#=2"]),
            Expect::Match,
        ),
        (
            "Canon.jpg",
            &canon,
            "jpg",
            "WhiteBalance",
            s(&["-MakerNotes:All=", "-WhiteBalance#=1"]),
            Expect::Match,
        ),
        (
            "Canon.jpg",
            &canon,
            "jpg",
            "WhiteBalance",
            s(&["-WhiteBalance#=1", "-MakerNotes:All="]),
            Expect::Match,
        ),
        (
            "Canon.jpg",
            &canon,
            "jpg",
            "WhiteBalance",
            s(&["-ExifIFD:All=", "-WhiteBalance#=1"]),
            Expect::Match,
        ),
        (
            "Nikon.jpg",
            &nikon,
            "jpg",
            "WhiteBalance",
            s(&["-MakerNotes:All=", "-WhiteBalance#=1"]),
            Expect::Match,
        ),
        (
            "NikonLS-50.jpg",
            &lsi,
            "jpg",
            "WhiteBalance",
            s(&["-WhiteBalance#=1"]),
            Expect::Match,
        ),
        (
            "Nikon.jpg+NikonLS-50 APP1",
            &two_app1,
            "jpg",
            "WhiteBalance",
            s(&["-WhiteBalance#=1"]),
            Expect::Refused,
        ),
        (
            "Nikon.jpg+NikonLS-50 APP1",
            &two_app1,
            "jpg",
            "Sharpness",
            s(&["-Sharpness#=2"]),
            Expect::Refused,
        ),
        (
            "Nikon.jpg",
            &nikon,
            "jpg",
            "FocusMode",
            s(&["-MakerNotes:FocusMode="]),
            Expect::Refused,
        ),
        (
            "Nikon.jpg",
            &nikon,
            "jpg",
            "Quality",
            s(&["-MakerNotes:Quality="]),
            Expect::Refused,
        ),
    ];
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(file, bytes, ext, name, args, expect)| {
            run_args(oracle, bytes, ext, file, name, args, *expect).err()
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} cases departed from the pinned outcome:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// A JPEG's segments: (start, marker, payload range, end).
fn jpeg_segments(data: &[u8]) -> Vec<(usize, u8, std::ops::Range<usize>, usize)> {
    let mut out = Vec::new();
    let mut at = 2;
    while at + 4 <= data.len() && data[at] == 0xFF && !matches!(data[at + 1], 0xD9 | 0xDA) {
        let len = usize::from(u16::from_be_bytes([data[at + 2], data[at + 3]]));
        out.push((at, data[at + 1], at + 4..at + 2 + len, at + 2 + len));
        at += 2 + len;
    }
    out
}

/// The first EXIF APP1's TIFF (offset of its first byte in `data`, and
/// whether it is little-endian), and its ExifIFD entries as
/// (entry offset in the TIFF, tag, type, count, value/offset field).
/// One ExifIFD entry: (offset in the TIFF, tag, type, count, value field).
type IfdEntry = (usize, u16, u16, u32, [u8; 4]);

fn exif_ifd_entries(data: &[u8]) -> (usize, bool, Vec<IfdEntry>) {
    let (_, _, payload, _) = jpeg_segments(data)
        .into_iter()
        .find(|(_, marker, payload, _)| {
            *marker == 0xE1 && data[payload.clone()].starts_with(b"Exif\0\0")
        })
        .expect("an EXIF APP1");
    let base = payload.start + 6;
    let tiff = &data[base..payload.end];
    let le = tiff.starts_with(b"II");
    let u16_at = |at: usize| {
        let b = [tiff[at], tiff[at + 1]];
        if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    };
    let u32_at = |at: usize| {
        let b = [tiff[at], tiff[at + 1], tiff[at + 2], tiff[at + 3]];
        if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    let entries = |ifd: usize| {
        (0..usize::from(u16_at(ifd)))
            .map(|k| {
                let at = ifd + 2 + 12 * k;
                let field = [tiff[at + 8], tiff[at + 9], tiff[at + 10], tiff[at + 11]];
                (at, u16_at(at), u16_at(at + 2), u32_at(at + 4), field)
            })
            .collect::<Vec<_>>()
    };
    let ifd0 = u32_at(4) as usize;
    let exif = entries(ifd0)
        .into_iter()
        .find(|entry| entry.1 == 0x8769)
        .map(|entry| u32_at(entry.0 + 8) as usize)
        .expect("an ExifIFD");
    (base, le, entries(exif))
}

/// PR #960's second review round, graded against the pinned oracle
/// (evidence `multigroup-write/round4/`):
///
/// - a bare value is typed at its address after the request's deletions
///   (`-MakerNotes:All= -ColorSpace#=2` on Canon.jpg: `[ExifIFD]` 2);
/// - a Minolta note ExifTool reads as `MakerNoteMinolta3` holds no tags
///   (Minolta.jpg with its note's prefix set to `MLY0`);
/// - a maker-note entry named directly (`-ExifIFD:MakerNoteCanon=`) is
///   deleted by 13.59 and refused here -- it used to be reported unchanged;
/// - `MakerNotes:All` takes a JPEG's CIFF segment, `EXIF:All` does not
///   (Writer.jpg carrying ExifTool.jpg's CIFF APP0);
/// - a read-only-looking maker-note candidate is still written by 13.59
///   (`-Software=x` on Sigma.jpg edits `[Sigma] Software`): refused;
/// - two maker notes in one ExifIFD are two notes (Apple.jpg with a second
///   0x927C carrying NikonD70.jpg's self-contained note): refused.
#[test]
fn second_review_round_shapes_match_the_oracle_or_are_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let read = |path: std::path::PathBuf| std::fs::read(path).unwrap();
    let canon = read(fixtures::required_t_images_fixture_path("Canon.jpg"));
    let writer = read(fixtures::required_t_images_fixture_path("Writer.jpg"));
    let exiftool = read(fixtures::required_t_images_fixture_path("ExifTool.jpg"));
    let combined = |name: &str| fixtures::pinned_combined_fixture_path(name).map(read);
    let (Some(minolta), Some(sigma), Some(apple), Some(d70)) = (
        combined("Minolta.jpg"),
        combined("Sigma.jpg"),
        combined("Apple.jpg"),
        combined("NikonD70.jpg"),
    ) else {
        return;
    };

    // Writer.jpg with ExifTool.jpg's CIFF APP0 right after SOI.
    let ciff = jpeg_segments(&exiftool)
        .into_iter()
        .find(|(_, marker, payload, _)| {
            *marker == 0xE0 && exiftool[payload.start..].starts_with(b"II\x1a\0\0\0HEAP")
        })
        .map(|(start, _, _, end)| exiftool[start..end].to_vec())
        .expect("ExifTool.jpg carries CIFF");
    let writer_ciff = [&writer[..2], &ciff, &writer[2..]].concat();

    // Minolta.jpg whose note starts `MLY0`: MakerNoteMinolta3.
    let (base, le, entries) = exif_ifd_entries(&minolta);
    let note = entries
        .iter()
        .find(|entry| entry.1 == 0x927C)
        .expect("a note");
    let offset = if le {
        u32::from_le_bytes(note.4)
    } else {
        u32::from_be_bytes(note.4)
    } as usize;
    let mut minolta3 = minolta.clone();
    minolta3[base + offset..base + offset + 4].copy_from_slice(b"MLY0");

    // Apple.jpg with the entry after its MakerNote rewritten as a second
    // 0x927C pointing at NikonD70.jpg's `Nikon\0\2` note, appended to APP1.
    let (d70_base, d70_le, d70_entries) = exif_ifd_entries(&d70);
    let d70_note = d70_entries.iter().find(|entry| entry.1 == 0x927C).unwrap();
    let d70_offset = if d70_le {
        u32::from_le_bytes(d70_note.4)
    } else {
        u32::from_be_bytes(d70_note.4)
    } as usize;
    let nikon_note = &d70[d70_base + d70_offset..d70_base + d70_offset + d70_note.3 as usize];
    assert!(nikon_note.starts_with(b"Nikon\0\x02"));
    let (apple_base, apple_le, apple_entries) = exif_ifd_entries(&apple);
    let (start, _, payload, end) = jpeg_segments(&apple)
        .into_iter()
        .find(|(_, marker, payload, _)| {
            *marker == 0xE1 && apple[payload.clone()].starts_with(b"Exif\0\0")
        })
        .unwrap();
    let mut tiff = apple[apple_base..payload.end].to_vec();
    let at = apple_entries
        .iter()
        .position(|entry| entry.1 == 0x927C)
        .unwrap()
        + 1;
    let slot = apple_entries[at].0;
    let pad = tiff.len() % 2;
    let new_offset = (tiff.len() + pad) as u32;
    let (tag, kind, count, offset) = if apple_le {
        (
            0x927Cu16.to_le_bytes(),
            7u16.to_le_bytes(),
            (nikon_note.len() as u32).to_le_bytes(),
            new_offset.to_le_bytes(),
        )
    } else {
        (
            0x927Cu16.to_be_bytes(),
            7u16.to_be_bytes(),
            (nikon_note.len() as u32).to_be_bytes(),
            new_offset.to_be_bytes(),
        )
    };
    tiff[slot..slot + 12].copy_from_slice(&[&tag[..], &kind, &count, &offset].concat());
    tiff.extend(std::iter::repeat_n(0, pad));
    tiff.extend_from_slice(nikon_note);
    let app1 = [&b"Exif\0\0"[..], &tiff].concat();
    let length = u16::try_from(app1.len() + 2).unwrap().to_be_bytes();
    let two_notes = [
        &apple[..start],
        &[0xFF, 0xE1],
        &length,
        &app1,
        &apple[end..],
    ]
    .concat();

    let s = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    let cases: Vec<Case<'_>> = vec![
        (
            "Canon.jpg",
            &canon,
            "jpg",
            "ColorSpace",
            s(&["-MakerNotes:All=", "-ColorSpace#=2"]),
            Expect::Match,
        ),
        (
            "Canon.jpg",
            &canon,
            "jpg",
            "ColorSpace",
            s(&["-ColorSpace#=2", "-MakerNotes:All="]),
            Expect::Match,
        ),
        (
            "Minolta.jpg (MLY0)",
            &minolta3,
            "jpg",
            "WhiteBalance",
            s(&["-WhiteBalance#=1"]),
            Expect::Match,
        ),
        (
            "Minolta.jpg (MLY0)",
            &minolta3,
            "jpg",
            "Contrast",
            s(&["-Contrast#=2"]),
            Expect::Match,
        ),
        (
            "Canon.jpg",
            &canon,
            "jpg",
            "WhiteBalance",
            s(&["-ExifIFD:MakerNoteCanon="]),
            Expect::Refused,
        ),
        (
            "Canon.jpg",
            &canon,
            "jpg",
            "WhiteBalance",
            s(&["-MakerNotes:MakerNoteCanon="]),
            Expect::Refused,
        ),
        (
            "Canon.jpg",
            &canon,
            "jpg",
            "WhiteBalance",
            s(&["-ExifIFD:MakerNoteCanon=", "-WhiteBalance#=1"]),
            Expect::Refused,
        ),
        (
            "Writer.jpg+CIFF",
            &writer_ciff,
            "jpg",
            "FocalLength",
            s(&["-MakerNotes:All=", "-FocalLength#=50"]),
            Expect::Match,
        ),
        (
            "Writer.jpg+CIFF",
            &writer_ciff,
            "jpg",
            "FocalLength",
            s(&["-EXIF:All=", "-FocalLength#=50"]),
            Expect::Refused,
        ),
        (
            "Sigma.jpg",
            &sigma,
            "jpg",
            "Software",
            s(&["-Software=x"]),
            Expect::Refused,
        ),
        (
            "Apple.jpg+Nikon note",
            &two_notes,
            "jpg",
            "WhiteBalance",
            s(&["-WhiteBalance#=1"]),
            Expect::Refused,
        ),
        (
            "Apple.jpg+Nikon note",
            &two_notes,
            "jpg",
            "Sharpness",
            s(&["-Sharpness#=2"]),
            Expect::Refused,
        ),
    ];
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(file, bytes, ext, name, args, expect)| {
            run_args(oracle, bytes, ext, file, name, args, *expect).err()
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} cases departed from the pinned outcome:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// A Samsung Sound & Shot SEFT/QDIOBS trailer (the builder of
/// `trailer_tail_forward_port.rs`): pinned 13.59 reads it as
/// `[MakerNotes:Samsung] EmbeddedAudioFileName`.
fn samsung_soundshot_trailer() -> Vec<u8> {
    let (name, audio) = (&b"SoundShot_000"[..], &b"sound-shot-bytes"[..]);
    let mut trailer = 0u32.to_be_bytes().to_vec();
    trailer.extend_from_slice(&(name.len() as u32).to_le_bytes());
    trailer.extend_from_slice(name);
    trailer.extend_from_slice(audio);
    let directory_at = trailer.len() as u32;
    let mut directory = b"SEFH".to_vec();
    directory.extend_from_slice(&101u32.to_le_bytes());
    directory.extend_from_slice(&1u32.to_le_bytes());
    directory.extend_from_slice(&0u16.to_le_bytes());
    directory.extend_from_slice(&0x0100u16.to_le_bytes());
    directory.extend_from_slice(&directory_at.to_le_bytes());
    directory.extend_from_slice(&((8 + name.len() + audio.len()) as u32).to_le_bytes());
    trailer.extend_from_slice(&directory);
    trailer.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    trailer.extend_from_slice(b"SEFT");
    trailer.extend_from_slice(&[0; 20]);
    trailer.extend_from_slice(&20u32.to_le_bytes());
    trailer.extend_from_slice(b"QDIOBS");
    trailer
}

/// PR #960's third review round, graded against the pinned oracle
/// (evidence `multigroup-write/round5/`):
///
/// - a TIFF-structured file's group deletion counts only where its writer
///   makes it (4113017923): on a raw type ExifIFD/MakerNotes deletions,
///   and on every TIFF `IFD0:All`, are pinned 13.59's no-ops, so the maker
///   note survives and 13.59 edits it too (t/images/Nikon.nef and
///   CanonRaw.cr2: `[Nikon]` / `[Canon] WhiteBalance` 1 beside `[ExifIFD]`);
///   oxidex wrote EXIF only. A deletion *after* the set instead cancels
///   the set's copies in its groups, taking effect or not (13.59:
///   `-WhiteBalance#=1 -MakerNotes:All=` writes `[ExifIFD]` alone; with
///   `-ExifIFD:All=` the file is unchanged);
/// - `-MakerNotes:*=` is `-MakerNotes:All=`; a value its address cannot
///   type is left for a later deletion to cancel (`-ColorSpace#=junk
///   -EXIF:All=` on GPS.jpg deletes EXIF); and a maker-note tag or entry
///   named beside a deletion that takes the note away is a no-op;
/// - a Samsung SEFT trailer's `Samsung` rows do not identify the EXIF
///   note (4113017918): Nikon.jpg with a trailer appended, where 13.59
///   also edits `[Nikon] WhiteBalance`; oxidex wrote EXIF only;
/// - `-ExifIFD:MakerNote=` names no tag in 13.59 ("doesn't exist or isn't
///   writable"), so the Canon copy is still edited (4112376913);
/// - two EXIF APP1s deleted by `-EXIF:All=` are no longer two copies to
///   the resolver (4112376897); the multi-APP1 deletion itself is still
///   declined by the writer, file untouched.
#[test]
fn third_review_round_shapes_match_the_oracle_or_are_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let read = |path: std::path::PathBuf| std::fs::read(path).unwrap();
    let nef = read(fixtures::required_t_images_fixture_path("Nikon.nef"));
    let cr2 = read(fixtures::required_t_images_fixture_path("CanonRaw.cr2"));
    let nikon = read(fixtures::required_t_images_fixture_path("Nikon.jpg"));
    let canon = read(fixtures::required_t_images_fixture_path("Canon.jpg"));
    let gps = read(fixtures::required_t_images_fixture_path("GPS.jpg"));
    let Some(lsi) = fixtures::pinned_combined_fixture_path("Nikon/NikonLS-50.jpg").map(read) else {
        return;
    };
    let nikon_seft = [&nikon[..], &samsung_soundshot_trailer()].concat();
    let nikon_canon = with_second_app1(&nikon, &canon);
    let exiftool = read(fixtures::required_t_images_fixture_path("ExifTool.jpg"));
    let ciff = jpeg_segments(&exiftool)
        .into_iter()
        .find(|(_, marker, payload, _)| {
            *marker == 0xE0 && exiftool[payload.start..].starts_with(b"II\x1a\0\0\0HEAP")
        })
        .map(|(start, _, _, end)| exiftool[start..end].to_vec())
        .expect("ExifTool.jpg carries CIFF");
    let canon_ciff = [&canon[..2], &ciff, &canon[2..]].concat();
    let two_app1 = with_second_app1(&nikon, &lsi);
    let s = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    let mut cases: Vec<Case<'_>> = Vec::new();
    for deletion in [
        "-MakerNotes:All=",
        "-ExifIFD:All=",
        "-EXIF:All=",
        "-IFD0:All=",
    ] {
        for (file, bytes, ext) in [("Nikon.nef", &nef, "nef"), ("CanonRaw.cr2", &cr2, "cr2")] {
            cases.push((
                file,
                bytes,
                ext,
                "WhiteBalance",
                s(&[deletion, "-WhiteBalance#=1"]),
                Expect::Refused,
            ));
            // After the set, 13.59's deletion cancels the set's new values
            // in the groups it names whether or not it takes effect:
            // `MakerNotes:All` the maker-note copy (only `[ExifIFD]
            // WhiteBalance` is written, the note stays), the others every
            // copy (file unchanged).
            cases.push((
                file,
                bytes,
                ext,
                "WhiteBalance",
                s(&["-WhiteBalance#=1", deletion]),
                Expect::Match,
            ));
        }
    }
    cases.extend([
        (
            "Nikon.jpg+SEFT",
            &nikon_seft[..],
            "jpg",
            "WhiteBalance",
            s(&["-WhiteBalance#=1"]),
            Expect::Refused,
        ),
        (
            "Canon.jpg",
            &canon[..],
            "jpg",
            "WhiteBalance",
            s(&["-ExifIFD:MakerNote=", "-WhiteBalance#=1"]),
            Expect::Refused,
        ),
        (
            "Nikon.jpg+NikonLS-50 APP1",
            &two_app1[..],
            "jpg",
            "WhiteBalance",
            s(&["-EXIF:All=", "-WhiteBalance#=1"]),
            Expect::Declined,
        ),
        // `MakerNotes:*` is `MakerNotes:All`.
        (
            "Nikon.jpg",
            &nikon[..],
            "jpg",
            "WhiteBalance",
            s(&["-MakerNotes:*=", "-WhiteBalance#=1"]),
            Expect::Match,
        ),
        (
            "Nikon.jpg",
            &nikon[..],
            "jpg",
            "WhiteBalance",
            s(&["-WhiteBalance#=1", "-MakerNotes:*="]),
            Expect::Match,
        ),
        // A maker-note request beside a group deletion that takes the note
        // away is a no-op: 13.59 deletes the note and nothing else. Where
        // the deletion is itself a no-op (a raw type) the note is edited,
        // which oxidex cannot do.
        (
            "Canon.jpg",
            &canon[..],
            "jpg",
            "MakerNoteCanon",
            s(&["-MakerNotes:All=", "-ExifIFD:MakerNoteCanon="]),
            Expect::Match,
        ),
        (
            "Canon.jpg",
            &canon[..],
            "jpg",
            "MakerNoteCanon",
            s(&["-ExifIFD:MakerNoteCanon=", "-MakerNotes:All="]),
            Expect::Match,
        ),
        (
            "Canon.jpg",
            &canon[..],
            "jpg",
            "WhiteBalance",
            s(&["-MakerNotes:All=", "-MakerNotes:WhiteBalance#=1"]),
            Expect::Match,
        ),
        (
            "Nikon.jpg",
            &nikon[..],
            "jpg",
            "FocusMode",
            s(&["-MakerNotes:All=", "-MakerNotes:FocusMode="]),
            Expect::Match,
        ),
        (
            "Nikon.jpg",
            &nikon[..],
            "jpg",
            "FocusMode",
            s(&["-EXIF:All=", "-MakerNotes:FocusMode="]),
            Expect::Match,
        ),
        (
            "Nikon.nef",
            &nef[..],
            "nef",
            "FocusMode",
            s(&["-MakerNotes:All=", "-MakerNotes:FocusMode="]),
            Expect::Refused,
        ),
        // An EXIF maker-note entry never names the CIFF segment that
        // `EXIF:All` keeps: 13.59 deletes the EXIF note.
        (
            "Canon.jpg+CIFF",
            &canon_ciff[..],
            "jpg",
            "MakerNoteCanon",
            s(&["-EXIF:All=", "-ExifIFD:MakerNoteCanon="]),
            Expect::Match,
        ),
        // Two EXIF blocks: a bare deletion that provably removes nothing is
        // 13.59's `unchanged`; one a maker note may hold stays refused.
        (
            "Nikon.jpg+Canon.jpg APP1",
            &nikon_canon[..],
            "jpg",
            "CalibrationIlluminant1",
            s(&["-CalibrationIlluminant1="]),
            Expect::Match,
        ),
        (
            "Nikon.jpg+Canon.jpg APP1",
            &nikon_canon[..],
            "jpg",
            "WhiteBalance",
            s(&["-WhiteBalance="]),
            Expect::Refused,
        ),
        // A value its address cannot type, cancelled by a later deletion:
        // 13.59 warns and deletes EXIF.
        (
            "GPS.jpg",
            &gps[..],
            "jpg",
            "ColorSpace",
            s(&["-ColorSpace#=junk", "-EXIF:All="]),
            Expect::Match,
        ),
    ]);
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(file, bytes, ext, name, args, expect)| {
            run_args(oracle, bytes, ext, file, name, args, *expect).err()
        })
        .collect();
    assert_eq!(cases.len(), 31);
    assert!(
        failures.is_empty(),
        "{} of {} cases departed from the pinned outcome:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// A physically present CIFF APP0 with no surfaced field is still a possible
/// second destination for bare names in the CIFF root. ExifTool recognizes
/// this signature, but an empty payload has no row for our reader to census.
#[test]
fn hidden_ciff_app0_is_not_inferred_absent_from_rows() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let writer = std::fs::read(fixtures::required_t_images_fixture_path("Writer.jpg")).unwrap();
    let ciff = b"\xff\xe0\0\x10II\x1a\0\0\0HEAPJPGM";
    let jpeg = [&writer[..2], ciff, &writer[2..]].concat();
    run_args(
        oracle,
        &jpeg,
        "jpg",
        "Writer.jpg + undecoded CIFF APP0",
        "FocalLength",
        &["-FocalLength#=50".to_string()],
        Expect::Refused,
    )
    .unwrap();
}

/// Exif::Main's non-Adobe DNGPrivateData route can carry a writable Pentax
/// note even when our DNG reader surfaces no row from that note.
#[test]
fn pentax_dng_private_note_is_counted_without_reader_rows() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let pentax = fixtures::required_t_images_fixture_path("Pentax.jpg");
    let note = oracle
        .command()
        .args(["-b", "-MakerNotePentax"])
        .arg(pentax)
        .output()
        .unwrap();
    assert!(note.status.success());
    assert!(note.stdout.starts_with(b"AOC\0MM"));
    let mut private = b"PENTAX \0".to_vec();
    private.extend_from_slice(&note.stdout[4..]);

    let mut dng = std::fs::read(fixtures::required_t_images_fixture_path("DNG.dng")).unwrap();
    assert_eq!(&dng[..4], b"MM\0*");
    let u16_at =
        |bytes: &[u8], at: usize| u16::from_be_bytes(bytes[at..at + 2].try_into().unwrap());
    let u32_at =
        |bytes: &[u8], at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    let ifd = u32_at(&dng, 4) as usize;
    let entry = (0..u16_at(&dng, ifd) as usize)
        .map(|i| ifd + 2 + 12 * i)
        .find(|&at| u16_at(&dng, at) == 0xc634)
        .expect("DNGPrivateData entry");
    assert_eq!(u16_at(&dng, entry + 2), 1); // BYTE
    let offset = u32::try_from(dng.len()).unwrap();
    dng[entry + 4..entry + 8].copy_from_slice(&(private.len() as u32).to_be_bytes());
    dng[entry + 8..entry + 12].copy_from_slice(&offset.to_be_bytes());
    dng.extend_from_slice(&private);

    let dir = tempfile::tempdir().unwrap();
    let before = dir.path().join("before.dng");
    let after = dir.path().join("after.dng");
    std::fs::write(&before, &dng).unwrap();
    std::fs::write(&after, &dng).unwrap();
    let original_rows = oracle_rows(oracle, &before, "Contrast");
    assert!(
        original_rows
            .iter()
            .any(|row| row.contains("[Pentax] Contrast"))
    );
    let native = oracle
        .command()
        .args(["-m", "-overwrite_original", "-Contrast#=2"])
        .arg(&after)
        .output()
        .unwrap();
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    let changed_rows = oracle_rows(oracle, &after, "Contrast");
    assert!(
        changed_rows
            .iter()
            .any(|row| row == "[Pentax] Contrast : 2")
    );
    run_args(
        oracle,
        &dng,
        "dng",
        "DNG.dng + PENTAX private note",
        "Contrast",
        &["-Contrast#=2".to_string()],
        Expect::Refused,
    )
    .unwrap();
    for args in [
        ["-MakerNotes:All=", "-Contrast#=2"],
        ["-Contrast#=2", "-MakerNotes:All="],
    ] {
        run_args(
            oracle,
            &dng,
            "dng",
            "DNG.dng + PENTAX private note and deletion",
            "Contrast",
            &args.map(str::to_string),
            Expect::Declined,
        )
        .unwrap();
    }
}

/// 4112472786: the Sigma `Software` candidate is not read-only in
/// practice -- pinned 13.59 `-Software=x` on Sigma.jpg edits `[Sigma]
/// Software` beside `[IFD0] Software` (`-v2`: "Writing Sigma:Software if
/// tag exists"), so the bare name stays refused.
#[test]
fn the_oracle_writes_the_sigma_software_candidate() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let Some(source) = fixtures::pinned_combined_fixture_path("Sigma.jpg") else {
        return;
    };
    let original = std::fs::read(&source).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("et.jpg");
    std::fs::write(&path, &original).unwrap();
    let out = oracle
        .command()
        .args(["-m", "-overwrite_original", "-Software=x"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        oracle_rows(oracle, &path, "Software"),
        ["[IFD0] Software : x", "[Sigma] Software : x"]
    );
    run_args(
        oracle,
        &original,
        "jpg",
        "Sigma.jpg",
        "Software",
        &["-Software=x".to_string()],
        Expect::Refused,
    )
    .unwrap();
}
