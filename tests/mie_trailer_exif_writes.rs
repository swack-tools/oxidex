//! CLI parity: EXIF writes into a file that carries a MIE trailer, graded
//! against pinned ExifTool 13.59 (`exiftool_oracle::graded()`).
//!
//! What the oracle does (evidence `20260921-beta1-direct/mie-grouped`):
//!
//! - Every EXIF-family set -- `-IFD0:Artist=v`, `-EXIF:Artist=v`,
//!   `-Artist=v`, `-IFD0:CalibrationIlluminant1#=20` -- is written into the
//!   main EXIF and into MIE-Meta's own `EXIF` element, which ExifTool
//!   creates when the trailer has none (t/images/ExifTool.jpg, `-v2`:
//!   `Creating EXIF` under `MIE1-Meta1`; two `[IFD0] Artist` rows after).
//! - A deletion -- `-IFD0:Artist=`, `-Artist=`, `-EXIF:All=` -- is applied
//!   to MIE's EXIF too where that holds the tag, and leaves a trailer without
//!   it byte-for-byte as it was (ExifTool.jpg's own MIE has no EXIF).
//! - The same holds after a TIFF (ExifTool.tif with ExifTool.jpg's MIE
//!   trailer appended); a PNG's trailing bytes are no MIE to it.
//!
//! oxidex writes no MIE, so every request whose oracle result changes the
//! MIE trailer must be refused by name (`Cannot write tag '<tag>': ... MIE
//! ...`), file byte-identical; every other one must give the oracle's rows.

use oxidex::exiftool_oracle::{self, Oracle};
use std::path::Path;
use std::process::Command;

#[path = "common/fixtures.rs"]
mod fixtures;

/// `(request, the tag whose rows are compared)`.
const CASES: &[(&str, &str)] = &[
    ("-IFD0:Artist=mie case", "Artist"),
    ("-EXIF:Artist=mie case", "Artist"),
    ("-Artist=mie case", "Artist"),
    ("-IFD0:CalibrationIlluminant1#=20", "CalibrationIlluminant1"),
    ("-ExifIFD:UserComment=mie case", "UserComment"),
    ("-IFD0:Artist=", "Artist"),
    ("-Artist=", "Artist"),
    ("-IFD0:Software=", "Software"),
    ("-EXIF:All=", "EXIF:All"),
    ("-GPS:All=", "GPS:All"),
];

/// Requests MIE plays no part in that oxidex refuses for another reason,
/// file untouched: the TIFF writer edits entries in place and cannot
/// shrink an IFD table ("not yet supported"), independent of MIE.
const NON_MIE_REFUSALS: &[(&str, &str)] = &[("ExifTool.tif+MIE", "-IFD0:Software=")];

/// The MIE trailer of `file`: the big-endian `zmie` footer's length back
/// from its end (MIE.pm:1705-1730), or `None`.
fn mie_trailer(file: &[u8]) -> Option<&[u8]> {
    let footer = b"~\0\x04\0zmie~\0\0\x06";
    let at = file.windows(footer.len()).rposition(|w| w == footer)?;
    let length_at = at + footer.len();
    assert_eq!(file[length_at + 4..length_at + 6], [0x10, 4], "a MM footer");
    let length = u32::from_be_bytes(file[length_at..length_at + 4].try_into().unwrap()) as usize;
    let end = length_at + 6;
    Some(&file[end - length..end])
}

/// Every group's `tag` rows of `path` as the oracle reads them.
fn oracle_rows(oracle: &Oracle, path: &Path, tag: &str) -> Vec<String> {
    let out = oracle
        .command()
        .args(["-a", "-G1", "-n", "-s", "-m", &format!("-{tag}")])
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

/// Which of the cases the oracle wrote MIE for; `Err` lists the departures.
fn grade(oracle: &Oracle, label: &str, ext: &str, original: &[u8]) -> Result<usize, Vec<String>> {
    let before = mie_trailer(original).expect("fixture carries MIE").to_vec();
    let mut failures = Vec::new();
    let mut mie_cases = 0;
    for &(request, tag) in CASES {
        let dir = tempfile::tempdir().unwrap();
        let et_path = dir.path().join(format!("et.{ext}"));
        let ox_path = dir.path().join(format!("ox.{ext}"));
        std::fs::write(&et_path, original).unwrap();
        std::fs::write(&ox_path, original).unwrap();
        let et = oracle
            .command()
            .args(["-m", "-overwrite_original", request])
            .arg(&et_path)
            .output()
            .expect("run oracle write");
        assert!(et.status.success(), "{label} {request}: oracle failed");
        let ox = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .arg(request)
            .arg(&ox_path)
            .output()
            .expect("run oxidex");
        let et_bytes = std::fs::read(&et_path).unwrap();
        let ox_bytes = std::fs::read(&ox_path).unwrap();
        let stderr = String::from_utf8_lossy(&ox.stderr).into_owned();
        let case = format!("{label} {request}");
        let named = request.trim_start_matches('-').split('=').next().unwrap();
        let named = named.trim_end_matches('#');
        if mie_trailer(&et_bytes) != Some(before.as_slice()) {
            mie_cases += 1;
            if ox.status.success()
                || ox_bytes != original
                || !stderr.contains(&format!("Cannot write tag '{named}'"))
                || !stderr.contains("MIE")
            {
                failures.push(format!(
                    "{case}: the oracle wrote MIE; expected a named MIE refusal, file \
                     untouched -- oxidex exit {:?}, changed {}, said {stderr:?}",
                    ox.status.code(),
                    ox_bytes != original
                ));
            }
            continue;
        }
        if NON_MIE_REFUSALS.contains(&(label, request)) {
            if ox.status.success() || ox_bytes != original || stderr.contains("MIE") {
                failures.push(format!(
                    "{case}: expected the TIFF writer's own refusal, file untouched; \
                     oxidex exit {:?}, said {stderr:?}",
                    ox.status.code()
                ));
            }
            continue;
        }
        let (et_rows, ox_rows) = (
            oracle_rows(oracle, &et_path, tag),
            oracle_rows(oracle, &ox_path, tag),
        );
        if !ox.status.success()
            || et_rows != ox_rows
            || (et_bytes == original) != (ox_bytes == original)
        {
            failures.push(format!(
                "{case}: the oracle left MIE as it was; expected its rows {et_rows:?} \
                 (changed {}), oxidex {ox_rows:?} (changed {}, exit {:?}, {stderr:?})",
                et_bytes != original,
                ox_bytes != original,
                ox.status.code()
            ));
        }
    }
    if failures.is_empty() {
        Ok(mie_cases)
    } else {
        Err(failures)
    }
}

/// ExifTool.jpg (a MIE trailer with no EXIF), the same after the oracle's
/// own `-IFD0:Artist=x` (a MIE trailer holding EXIF), and ExifTool.tif with
/// ExifTool.jpg's MIE trailer appended: every set is refused (the oracle
/// writes MIE for each), every deletion MIE holds nothing of matches.
#[test]
fn exif_writes_into_a_mie_carrying_file_match_the_oracle_or_are_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let jpeg = std::fs::read(fixtures::required_t_images_fixture_path("ExifTool.jpg")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let with_exif = dir.path().join("mie-exif.jpg");
    std::fs::write(&with_exif, &jpeg).unwrap();
    let status = oracle
        .command()
        .args(["-q", "-overwrite_original", "-IFD0:Artist=x"])
        .arg(&with_exif)
        .status()
        .unwrap();
    assert!(status.success());
    let with_exif = std::fs::read(&with_exif).unwrap();
    assert!(
        mie_trailer(&with_exif).unwrap().len() > mie_trailer(&jpeg).unwrap().len(),
        "the oracle's set created MIE's EXIF"
    );
    let tiff = [
        std::fs::read(fixtures::required_t_images_fixture_path("ExifTool.tif")).unwrap(),
        mie_trailer(&jpeg).unwrap().to_vec(),
    ]
    .concat();

    let mut failures = Vec::new();
    let mut mie_cases = Vec::new();
    for (label, ext, bytes) in [
        ("ExifTool.jpg", "jpg", &jpeg),
        ("ExifTool.jpg+MIE-EXIF", "jpg", &with_exif),
        ("ExifTool.tif+MIE", "tif", &tiff),
    ] {
        match grade(oracle, label, ext, bytes) {
            Ok(count) => mie_cases.push((label, count)),
            Err(departures) => failures.extend(departures),
        }
    }
    assert!(
        failures.is_empty(),
        "{} departures:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // The five sets write MIE everywhere; of the deletions, MIE's EXIF
    // holds only Artist (x) and the carrier for `-EXIF:All=`.
    assert_eq!(
        mie_cases,
        [
            ("ExifTool.jpg", 5),
            ("ExifTool.jpg+MIE-EXIF", 8),
            ("ExifTool.tif+MIE", 5)
        ]
    );
}
