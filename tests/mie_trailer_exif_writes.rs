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

/// `(requests, the tag whose rows are compared)`.
type Case = (&'static [&'static str], &'static str);

const CASES: &[Case] = &[
    (&["-IFD0:Artist=mie case"], "Artist"),
    (&["-EXIF:Artist=mie case"], "Artist"),
    (&["-Artist=mie case"], "Artist"),
    (
        &["-IFD0:CalibrationIlluminant1#=20"],
        "CalibrationIlluminant1",
    ),
    (&["-ExifIFD:UserComment=mie case"], "UserComment"),
    (&["-IFD0:Artist="], "Artist"),
    (&["-Artist="], "Artist"),
    (&["-IFD0:Software="], "Software"),
    (&["-EXIF:All="], "EXIF:All"),
    (&["-GPS:All="], "GPS:All"),
];

/// Requests MIE plays no part in that oxidex refuses for another reason,
/// file untouched: the TIFF writer edits entries in place and cannot
/// shrink an IFD table ("not yet supported"), independent of MIE.
const NON_MIE_REFUSALS: &[(&str, &[&str])] = &[("ExifTool.tif+MIE", &["-IFD0:Software="])];

/// Every MIE trailer of `file`, in file order: each big-endian `zmie`
/// footer's length back from its end (MIE.pm:1705-1730).
fn mie_trailers(file: &[u8]) -> Vec<&[u8]> {
    let footer = b"~\0\x04\0zmie~\0\0\x06";
    file.windows(footer.len())
        .enumerate()
        .filter(|(_, w)| w == footer)
        .map(|(at, _)| {
            let length_at = at + footer.len();
            assert_eq!(file[length_at + 4..length_at + 6], [0x10, 4], "a MM footer");
            let length =
                u32::from_be_bytes(file[length_at..length_at + 4].try_into().unwrap()) as usize;
            let end = length_at + 6;
            &file[end - length..end]
        })
        .collect()
}

/// The last MIE trailer of `file`.
fn mie_trailer(file: &[u8]) -> Option<&[u8]> {
    mie_trailers(file).pop()
}

/// The first SOS segment in the pinned Writer.jpg is complete and length 12.
/// `ff 11` here is a component/table pair inside its header, not a marker.
fn ff11_first_sos(mut jpeg: Vec<u8>) -> Vec<u8> {
    let sos = jpeg
        .windows(2)
        .position(|w| w == [0xff, 0xda])
        .expect("SOS");
    assert_eq!(&jpeg[sos..sos + 7], b"\xff\xda\0\x0c\x03\x01\0");
    jpeg[sos + 5..sos + 7].copy_from_slice(b"\xff\x11");
    jpeg
}

#[test]
fn first_sos_header_ff11_does_not_hide_mie_from_write_census() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let plain = ff11_first_sos(
        std::fs::read(fixtures::required_t_images_fixture_path("Writer.jpg")).unwrap(),
    );
    let mie_source =
        std::fs::read(fixtures::required_t_images_fixture_path("ExifTool.jpg")).unwrap();
    let mut carrying = plain.clone();
    carrying.extend_from_slice(mie_trailer(&mie_source).expect("pinned MIE trailer"));
    assert_eq!(mie_trailer(&carrying).unwrap().len(), 90);

    let dir = tempfile::tempdir().unwrap();
    // grade independently proves that native ExifTool copies Artist into MIE
    // while OxiDex refuses with the file's bytes and inode unchanged.
    assert_eq!(
        grade(
            oracle,
            "first SOS ff11 + MIE",
            "jpg",
            &carrying,
            &[(&["-IFD0:Artist=x"], "Artist")]
        ),
        Ok(1)
    );

    // The same header without MIE is still an ordinary writable JPEG.
    let et_path = dir.path().join("native-plain.jpg");
    let ox_path = dir.path().join("oxidex-plain.jpg");
    std::fs::write(&et_path, &plain).unwrap();
    std::fs::write(&ox_path, &plain).unwrap();
    let et = oracle
        .command()
        .args(["-m", "-overwrite_original", "-IFD0:Artist=x"])
        .arg(&et_path)
        .output()
        .unwrap();
    let ox = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-IFD0:Artist=x")
        .arg(&ox_path)
        .output()
        .unwrap();
    assert!(
        et.status.success(),
        "native: {}",
        String::from_utf8_lossy(&et.stderr)
    );
    assert!(
        ox.status.success(),
        "oxidex: {}",
        String::from_utf8_lossy(&ox.stderr)
    );
    assert_ne!(std::fs::read(&ox_path).unwrap(), plain);
    assert_eq!(
        oracle_rows(oracle, &et_path, "Artist"),
        oracle_rows(oracle, &ox_path, "Artist")
    );
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
fn grade(
    oracle: &Oracle,
    label: &str,
    ext: &str,
    original: &[u8],
    cases: &[Case],
) -> Result<usize, Vec<String>> {
    let before: Vec<Vec<u8>> = mie_trailers(original)
        .into_iter()
        .map(<[u8]>::to_vec)
        .collect();
    assert!(!before.is_empty(), "{label}: fixture carries MIE");
    let mut failures = Vec::new();
    let mut mie_cases = 0;
    for &(requests, tag) in cases {
        let dir = tempfile::tempdir().unwrap();
        let et_path = dir.path().join(format!("et.{ext}"));
        let ox_path = dir.path().join(format!("ox.{ext}"));
        std::fs::write(&et_path, original).unwrap();
        std::fs::write(&ox_path, original).unwrap();
        #[cfg(unix)]
        let ox_inode_before = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(&ox_path).unwrap().ino()
        };
        let et = oracle
            .command()
            .args(["-m", "-overwrite_original"])
            .args(requests)
            .arg(&et_path)
            .output()
            .expect("run oracle write");
        assert!(et.status.success(), "{label} {requests:?}: oracle failed");
        let ox = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(requests)
            .arg(&ox_path)
            .output()
            .expect("run oxidex");
        let et_bytes = std::fs::read(&et_path).unwrap();
        let ox_bytes = std::fs::read(&ox_path).unwrap();
        let stderr = String::from_utf8_lossy(&ox.stderr).into_owned();
        let case = format!("{label} {requests:?}");
        let last = requests.last().unwrap();
        let named = last.trim_start_matches('-').split('=').next().unwrap();
        let named = named.trim_end_matches('#');
        if mie_trailers(&et_bytes) != before {
            mie_cases += 1;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                assert_eq!(
                    std::fs::metadata(&ox_path).unwrap().ino(),
                    ox_inode_before,
                    "{case}: refusal preserves inode"
                );
            }
            if ox.status.success()
                || ox_bytes != original
                // oxidex names the tag in its canonical spelling
                // (`-gps:all=` is refused as 'GPS:All').
                || !stderr
                    .to_ascii_lowercase()
                    .contains(&format!("cannot write tag '{}'", named.to_ascii_lowercase()))
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
        if NON_MIE_REFUSALS.contains(&(label, requests)) {
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
        match grade(oracle, label, ext, bytes, CASES) {
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

/// The MIE trailer an EXIF `-IFD0:Artist=x` of the oracle's own leaves on
/// ExifTool.jpg: `0MIE` / `Meta` / `Document` / Copyright, then `EXIF`.
fn oracle_mie_with_exif(oracle: &Oracle, jpeg: &[u8]) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mie-exif.jpg");
    std::fs::write(&path, jpeg).unwrap();
    let status = oracle
        .command()
        .args(["-q", "-overwrite_original", "-IFD0:Artist=x"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    mie_trailer(&std::fs::read(&path).unwrap())
        .unwrap()
        .to_vec()
}

/// A big-endian MIE trailer holding `tiff` as MIE-Meta's `EXIF` element.
fn mie_holding_exif(tiff: &[u8]) -> Vec<u8> {
    let mut out = b"~\x10\x04\xfe0MIE\0\0\0\0~\x10\x04\0Meta~\0\x04\xfeEXIF".to_vec();
    out.extend_from_slice(&(tiff.len() as u32).to_be_bytes());
    out.extend_from_slice(tiff);
    out.extend_from_slice(b"~\0\0\0~\0\x04\0zmie~\0\0\x06");
    let length = out.len() as u32 + 6;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&[0x10, 4]);
    out
}

/// A truncated Adobe successor leaves maker-note presence uncertain even
/// though the census has counted no complete MakN record. A named note
/// request or group clear must stop the whole transaction, including an ordinary sibling.
#[test]
fn malformed_adobe_mie_note_refuses_write_atomically() {
    let canon = std::fs::read(fixtures::required_t_images_fixture_path("Canon.jpg")).unwrap();
    let mut private = b"Adobe\0XxxN".to_vec();
    private.extend_from_slice(&17_u32.to_be_bytes());
    private.extend_from_slice(b"payload MakN text");
    private.push(0); // even-sized Adobe record
    private.extend_from_slice(b"MakN\0\0\0\x06II"); // truncated successor
    let mut tiff = b"MM\0*\0\0\0\x08\0\x01\xc6\x34\0\x07".to_vec();
    tiff.extend_from_slice(&(private.len() as u32).to_be_bytes());
    tiff.extend_from_slice(&26_u32.to_be_bytes());
    tiff.extend_from_slice(&[0; 4]); // no next IFD
    tiff.extend_from_slice(&private);
    let original = [canon.as_slice(), &mie_holding_exif(&tiff)].concat();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("malformed-adobe-mie.jpg");
    std::fs::write(&file, &original).unwrap();
    let sibling = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg("-IFD0:Make=")
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        sibling.status.success(),
        "sibling alone: {}",
        String::from_utf8_lossy(&sibling.stderr)
    );
    assert_ne!(std::fs::read(&file).unwrap(), original);
    for requests in [
        &["-MakerNotes:OwnerName=", "-IFD0:Make="][..],
        &["-IFD0:Make=", "-MakerNotes:OwnerName="][..],
        &["-MakerNotes:All=", "-IFD0:Make="][..],
        &["-IFD0:Make=", "-MakerNotes:All="][..],
    ] {
        std::fs::write(&file, &original).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(requests)
            .arg(&file)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{requests:?}");
        assert!(output.stdout.is_empty(), "{requests:?}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("MIE") && error.contains("MakerNotes"),
            "{error}"
        );
        assert_eq!(std::fs::read(&file).unwrap(), original, "{requests:?}");
    }
}

/// The TIFF payload of the first `Exif\0\0` APP1 of `jpeg`.
fn jpeg_exif_tiff(jpeg: &[u8]) -> Vec<u8> {
    let at = jpeg.windows(2).position(|w| w == [0xFF, 0xE1]).unwrap();
    let length = u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]) as usize;
    let segment = &jpeg[at + 4..at + 2 + length];
    assert_eq!(&segment[..6], b"Exif\0\0");
    segment[6..].to_vec()
}

/// A MIE EXIF holding a Nikon maker note cannot own Canon:OwnerName.
/// The requested vendor, not mere presence of any note, decides whether
/// ExifTool touches MIE on a named maker-note deletion.
#[test]
fn mie_makernote_deletion_respects_requested_vendor() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let writer = std::fs::read(fixtures::required_t_images_fixture_path("Writer.jpg")).unwrap();
    let nikon = std::fs::read(fixtures::required_t_images_fixture_path("Nikon.jpg")).unwrap();
    let canon = std::fs::read(fixtures::required_t_images_fixture_path("Canon.jpg")).unwrap();
    let cases: &[Case] = &[(&["-Canon:OwnerName="], "OwnerName")];
    let nikon_mie = [
        writer.as_slice(),
        &mie_holding_exif(&jpeg_exif_tiff(&nikon)),
    ]
    .concat();
    let canon_mie = [
        writer.as_slice(),
        &mie_holding_exif(&jpeg_exif_tiff(&canon)),
    ]
    .concat();
    assert_eq!(
        grade(oracle, "Writer+MIE Nikon", "jpg", &nikon_mie, cases),
        Ok(0),
        "Nikon note: the native deletion is unchanged"
    );
    assert_eq!(
        grade(oracle, "Writer+MIE Canon", "jpg", &canon_mie, cases),
        Ok(1),
        "Canon note: native edits MIE, so this writer must refuse"
    );
}

/// PR #966 review threads, each graded against the oracle as above:
///
/// - 4112546168: a set a later deletion of the same field overrides is not
///   ExifTool's to write (13.59: `-IFD0:XPTitle=x -IFD0:XPTitle=` on
///   ExifTool.jpg is `unchanged`; `-IFD0:Artist=x -IFD0:Artist=` deletes
///   the main Artist, MIE's 90 bytes untouched), so it cannot refuse.
/// - 4112546172: a MIE `EXIF` element whose TIFF magic is not 42 is read
///   anyway, and `-IFD0:Artist=` deletes its Artist (188 -> 90 bytes).
/// - 4112546173: of two MIE trailers, the inner one's EXIF is edited too
///   (Writer.jpg + [EXIF MIE] + [MIE]: `-IFD0:Artist=` 278 -> 266 bytes).
/// - 4112546175: a maker note in MIE's EXIF is edited by
///   `-MakerNotes:OwnerName=` (Writer.jpg + MIE holding Canon.jpg's EXIF:
///   2490 -> 2496 bytes, `[Canon] OwnerName` emptied) where the main EXIF
///   has no maker note at all.
#[test]
fn review_966_mie_cases_match_the_oracle_or_are_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let jpeg = std::fs::read(fixtures::required_t_images_fixture_path("ExifTool.jpg")).unwrap();
    let writer = std::fs::read(fixtures::required_t_images_fixture_path("Writer.jpg")).unwrap();
    let canon = std::fs::read(fixtures::required_t_images_fixture_path("Canon.jpg")).unwrap();
    let plain_mie = mie_trailer(&jpeg).unwrap().to_vec();
    let exif_mie = oracle_mie_with_exif(oracle, &jpeg);

    // 4112546172: the EXIF element's `MM\0*` made `MM\0+`.
    let mut bad_magic = exif_mie.clone();
    let at = bad_magic
        .windows(8)
        .position(|w| w == b"EXIFMM\0*")
        .unwrap();
    bad_magic[at + 7] = 0x2b;
    let bad_magic = [&writer[..], &bad_magic].concat();
    // 4112546173: an inner MIE holding EXIF, an outer one without.
    let two_mie = [&writer[..], &exif_mie, &plain_mie].concat();
    // 4112546175: a MIE holding Canon.jpg's EXIF (a Canon maker note).
    let makernote_mie = [&writer[..], &mie_holding_exif(&jpeg_exif_tiff(&canon))].concat();

    const OVERRIDES: &[Case] = &[
        (&["-IFD0:XPTitle=x", "-IFD0:XPTitle="], "XPTitle"),
        (&["-IFD0:Artist=x", "-IFD0:Artist="], "Artist"),
    ];
    const DELETIONS: &[Case] = &[
        (&["-IFD0:Artist="], "Artist"),
        (&["-EXIF:All="], "EXIF:All"),
    ];
    const MAKERNOTE: &[Case] = &[
        (&["-MakerNotes:OwnerName="], "OwnerName"),
        (&["-IFD0:Software="], "Software"),
    ];
    let mut failures = Vec::new();
    let mut mie_cases = Vec::new();
    for (label, bytes, cases) in [
        ("ExifTool.jpg overrides", &jpeg, OVERRIDES),
        ("Writer.jpg+MIE-EXIF(magic 43)", &bad_magic, DELETIONS),
        ("Writer.jpg+MIE-EXIF+MIE", &two_mie, DELETIONS),
        ("Writer.jpg+MIE-Canon-EXIF", &makernote_mie, MAKERNOTE),
    ] {
        match grade(oracle, label, "jpg", bytes, cases) {
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
    assert_eq!(
        mie_cases,
        [
            ("ExifTool.jpg overrides", 0),
            ("Writer.jpg+MIE-EXIF(magic 43)", 2),
            ("Writer.jpg+MIE-EXIF+MIE", 2),
            ("Writer.jpg+MIE-Canon-EXIF", 1),
        ]
    );
}

/// Writer.jpg with `object` inside an APP15 segment ahead of its first
/// marker after SOI: a MIE object no trailer walk reaches.
fn in_app15(jpeg: &[u8], object: &[u8]) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xEF]);
    out.extend_from_slice(&((object.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(object);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// A big-endian MIE trailer holding `tiff` as an `EXIF` leaf of its
/// `0MIE` / `Meta` / `Document` group -- no EXIF directory to ExifTool.
fn mie_holding_document_exif(tiff: &[u8]) -> Vec<u8> {
    let mut out =
        b"~\x10\x04\xfe0MIE\0\0\0\0~\x10\x04\0Meta~\x10\x08\0Document~\0\x04\xfeEXIF".to_vec();
    out.extend_from_slice(&(tiff.len() as u32).to_be_bytes());
    out.extend_from_slice(tiff);
    out.extend_from_slice(b"~\0\0\0~\0\0\0~\0\x04\0zmie~\0\0\x06");
    let length = out.len() as u32 + 6;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&[0x10, 4]);
    out
}

/// A big-endian TIFF: IFD0 `Make` "FooCam", and an ExifIFD whose 0x927c
/// MakerNote is plain text no maker parser claims -- pinned 13.59 reads it
/// as `[ExifIFD] MakerNoteUnknownText`.
fn tiff_with_unknown_text_note() -> Vec<u8> {
    let make: &[u8] = b"FooCam\0";
    let note: &[u8] = b"Plain text maker note\0";
    let make_at = 8 + 2 + 2 * 12 + 4;
    let exif_at = make_at + make.len() + (make.len() & 1);
    let note_at = exif_at + 2 + 12 + 4;
    let mut out = b"MM\0*\0\0\0\x08\0\x02".to_vec();
    for (tag, kind, count, value) in [
        (0x010f_u16, 2_u16, make.len(), make_at),
        (0x8769, 4, 1, exif_at),
    ] {
        out.extend_from_slice(&tag.to_be_bytes());
        out.extend_from_slice(&kind.to_be_bytes());
        out.extend_from_slice(&(count as u32).to_be_bytes());
        out.extend_from_slice(&(value as u32).to_be_bytes());
    }
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(make);
    if make.len() & 1 == 1 {
        out.push(0);
    }
    out.extend_from_slice(b"\0\x01\x92\x7c\0\x07");
    out.extend_from_slice(&(note.len() as u32).to_be_bytes());
    out.extend_from_slice(&(note_at as u32).to_be_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(note);
    out
}

/// PR #966 review threads on f1bec0f6 / 4df75fa4, graded against the
/// oracle as above (evidence `20260921-beta1-direct/mie-grouped`):
///
/// - 4112736390: `-gps:all=` is `-GPS:All=` in any spelling; 13.59 deletes
///   the GPS a MIE copy holds (ExifTool.jpg after the oracle's own
///   `-GPS:GPSLatitude=1`): refused.
/// - 4112736393: a MIE object inside an APP15 segment is no trailer; 13.59
///   leaves it (`-IFD0:Make=`: unchanged) and writes `-IFD0:Artist=x` into
///   the main EXIF only.
/// - 4112736395: an `EXIF` leaf under MIE `Document` is no EXIF directory;
///   `-IFD0:Make=` and `-IFD0:Model=` are unchanged.
/// - 4112736397: `-MakerNotes:All=` deletes a DNGPrivateData maker note in
///   MIE's EXIF (Writer.jpg + MIE holding DNG.dng: 13967 -> 5593 bytes).
/// - 4113020034: a 0x927c note 13.59 files under ExifIFD
///   (`MakerNoteUnknownText`) is left by `-MakerNotes:All=` (unchanged).
/// - 4113020039: a bare deletion also edits MIE's maker note:
///   `-WhiteBalance=` resolves to ExifIFD, which Nikon.jpg's EXIF lacks, but
///   13.59 deletes `[Nikon] WhiteBalance` in a MIE holding that EXIF
///   (1747 -> 1731 bytes; 4df75fa4 reported `unchanged`). A maker-note
///   group's own name, `-Canon:OwnerName=`, edits it too (2741 -> 2747).
/// - Codex pre-review of 1fdcc215: `-GPS:All=` deletes an empty GPS IFD
///   (and its IFD0 pointer) from MIE's EXIF (357 -> 335 bytes); with no
///   IFD1 there, `-IFD1:All=` is unchanged.
#[test]
fn review_966_round_two_mie_cases_match_the_oracle_or_are_refused() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let jpeg = std::fs::read(fixtures::required_t_images_fixture_path("ExifTool.jpg")).unwrap();
    let writer = std::fs::read(fixtures::required_t_images_fixture_path("Writer.jpg")).unwrap();
    let canon = std::fs::read(fixtures::required_t_images_fixture_path("Canon.jpg")).unwrap();
    let dng = std::fs::read(fixtures::required_t_images_fixture_path("DNG.dng")).unwrap();
    let nikon = std::fs::read(fixtures::required_t_images_fixture_path("Nikon.jpg")).unwrap();
    let canon_tiff = jpeg_exif_tiff(&canon);

    // 4112736390: ExifTool.jpg whose main EXIF and MIE copy both hold GPS.
    let dir = tempfile::tempdir().unwrap();
    let gps_path = dir.path().join("mie-gps.jpg");
    std::fs::write(&gps_path, &jpeg).unwrap();
    let status = oracle
        .command()
        .args(["-q", "-overwrite_original", "-GPS:GPSLatitude=1"])
        .arg(&gps_path)
        .status()
        .unwrap();
    assert!(status.success());
    let gps = std::fs::read(&gps_path).unwrap();
    assert!(
        mie_trailer(&gps).unwrap().len() > mie_trailer(&jpeg).unwrap().len(),
        "the oracle's set created MIE's EXIF"
    );
    // 4112736393: a MIE holding Canon.jpg's EXIF inside an APP15 segment.
    let app15 = in_app15(&writer, &mie_holding_exif(&canon_tiff));
    // 4112736395: a MIE whose Document group holds Canon.jpg's EXIF.
    let document = [&writer[..], &mie_holding_document_exif(&canon_tiff)].concat();
    // 4112736397 / 4113020034 / 4113020039.
    let dng_mie = [&writer[..], &mie_holding_exif(&dng)].concat();
    let unknown_note = [
        &writer[..],
        &mie_holding_exif(&tiff_with_unknown_text_note()),
    ]
    .concat();
    let canon_mie = [&writer[..], &mie_holding_exif(&canon_tiff)].concat();
    // Codex pre-review of 1fdcc215: an IFD0 pointing at an empty GPS IFD.
    let mut empty_gps = b"MM\0*\0\0\0\x08\0\x02\x01\x0f\0\x02\0\0\0\x07\0\0\0\x26".to_vec();
    empty_gps
        .extend_from_slice(b"\x88\x25\0\x04\0\0\0\x01\0\0\0\x2e\0\0\0\0FooCam\0\0\0\0\0\0\0\0");
    let empty_gps_mie = [&writer[..], &mie_holding_exif(&empty_gps)].concat();
    let nikon_mie = [&writer[..], &mie_holding_exif(&jpeg_exif_tiff(&nikon))].concat();

    const GPS: &[Case] = &[(&["-gps:all="], "GPS:All"), (&["-GPS:All="], "GPS:All")];
    const EMBEDDED: &[Case] = &[
        (&["-IFD0:Make="], "Make"),
        (&["-IFD0:Artist=main only"], "Artist"),
    ];
    const DOCUMENT: &[Case] = &[(&["-IFD0:Make="], "Make"), (&["-IFD0:Model="], "Model")];
    const MAKERNOTES_ALL: &[Case] = &[
        (&["-MakerNotes:All="], "MakerNotes:All"),
        (&["-makernotes:all="], "MakerNotes:All"),
    ];
    const BARE: &[Case] = &[
        (&["-WhiteBalance="], "WhiteBalance"),
        (&["-MakerNotes:OwnerName="], "OwnerName"),
        (&["-MakerNotes:WhiteBalance="], "WhiteBalance"),
    ];
    const MAKERNOTE_GROUP: &[Case] = &[(&["-Canon:OwnerName="], "OwnerName")];
    const EMPTY_GPS: &[Case] = &[(&["-GPS:All="], "GPS:All"), (&["-IFD1:All="], "IFD1:All")];
    let mut failures = Vec::new();
    let mut mie_cases = Vec::new();
    for (label, bytes, cases) in [
        ("ExifTool.jpg+MIE-GPS", &gps, GPS),
        ("Writer.jpg+APP15-MIE", &app15, EMBEDDED),
        ("Writer.jpg+MIE-Document-EXIF", &document, DOCUMENT),
        ("Writer.jpg+MIE-DNG", &dng_mie, MAKERNOTES_ALL),
        ("Writer.jpg+MIE-unknown-note", &unknown_note, MAKERNOTES_ALL),
        ("Writer.jpg+MIE-Nikon-EXIF", &nikon_mie, BARE),
        ("Writer.jpg+MIE-Canon-EXIF", &canon_mie, MAKERNOTE_GROUP),
        ("Writer.jpg+MIE-empty-GPS", &empty_gps_mie, EMPTY_GPS),
    ] {
        match grade(oracle, label, "jpg", bytes, cases) {
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
    assert_eq!(
        mie_cases,
        [
            ("ExifTool.jpg+MIE-GPS", 2),
            ("Writer.jpg+APP15-MIE", 0),
            ("Writer.jpg+MIE-Document-EXIF", 0),
            ("Writer.jpg+MIE-DNG", 2),
            ("Writer.jpg+MIE-unknown-note", 0),
            ("Writer.jpg+MIE-Nikon-EXIF", 2),
            ("Writer.jpg+MIE-Canon-EXIF", 1),
            ("Writer.jpg+MIE-empty-GPS", 1),
        ]
    );
}

/// Native MIE.pm strips a trailing units suffix before directory/tag lookup.
#[test]
fn review_repair_966_unit_bearing_meta_and_exif_refuse_partial_deletions() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let images = fixtures::pinned_t_images_dir().unwrap();
    let writer = std::fs::read(images.join("Writer.jpg")).unwrap();
    let canon = std::fs::read(images.join("Canon.jpg")).unwrap();
    let tiff = jpeg_exif_tiff(&canon);
    for (label, meta, exif) in [
        ("ordinary", "Meta", "EXIF"),
        ("meta-units", "Meta(x)", "EXIF"),
        ("exif-units", "Meta", "EXIF(x)"),
        ("both-units", "Meta(x)", "EXIF(x)"),
        ("nested-units", "Meta(x(y))", "EXIF(x(y))"),
    ] {
        let mut mie = b"~\x10\x04\xfe0MIE\0\0\0\0".to_vec();
        mie.extend([b'~', 0x10, meta.len() as u8, 0]);
        mie.extend(meta.as_bytes());
        mie.extend([b'~', 0, exif.len() as u8, 0xfe]);
        mie.extend(exif.as_bytes());
        mie.extend((tiff.len() as u32).to_be_bytes());
        mie.extend(&tiff);
        mie.extend(b"~\0\0\0~\0\x04\0zmie~\0\0\x06");
        let length = mie.len() as u32 + 6;
        mie.extend(length.to_be_bytes());
        mie.extend([0x10, 4]);
        let jpeg = [writer.as_slice(), mie.as_slice()].concat();
        let result = grade(oracle, label, "jpg", &jpeg, &[(&["-IFD0:Make="], "Make")]);
        assert_eq!(result, Ok(1), "{label}: native removes MIE's Make");
    }
}

/// Legal JPEG FF fill cannot hide an SOS the native trailer walker reaches.
#[test]
fn review_repair_966_jpeg_marker_fill_refuses_partial_mie_deletions() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let images = fixtures::pinned_t_images_dir().unwrap();
    let writer = std::fs::read(images.join("Writer.jpg")).unwrap();
    let canon = std::fs::read(images.join("Canon.jpg")).unwrap();
    let mie = mie_holding_exif(&jpeg_exif_tiff(&canon));
    let sos = writer.windows(2).position(|w| w == [0xff, 0xda]).unwrap();
    for (label, at, fill) in [
        ("ordinary", sos, 0),
        ("one-fill-before-sos", sos, 1),
        ("many-fill-before-sos", sos, 8),
    ] {
        let jpeg = [&writer[..at], &vec![0xff; fill], &writer[at..], &mie].concat();
        let result = grade(oracle, label, "jpg", &jpeg, &[(&["-IFD0:Make="], "Make")]);
        assert_eq!(result, Ok(1), "{label}: native removes MIE's Make");
    }
    // A fill run before the first APP segment is also legal JPEG, but the
    // existing generic JPEG metadata reader rejects it before write planning.
    // This separate limit must fail closed, preserving bytes and inode.
    let early_fill = [&writer[..2], &[0xff; 3], &writer[2..], &mie].concat();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fill-before-app.jpg");
    std::fs::write(&path, &early_fill).unwrap();
    #[cfg(unix)]
    let inode_before = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(&path).unwrap().ino()
    };
    let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-IFD0:Make="])
        .arg(&path)
        .output()
        .unwrap();
    assert!(!out.status.success(), "early fill must fail closed");
    assert_eq!(std::fs::read(&path).unwrap(), early_fill);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode_before);
    }
}

/// A SubIFD can lead to GPS or ExifIFD absent from the root-only surgical
/// scan. Native 13.59 clears the MIE copy as well as the main carrier's.
#[test]
fn nested_subifd_groups_in_mie_refuse_partial_clears() {
    let Some(oracle) = exiftool_oracle::graded() else {
        return;
    };
    let entry = |tag: u16, field_type: u16, count: u32, value: u32| {
        [
            tag.to_le_bytes().as_slice(),
            field_type.to_le_bytes().as_slice(),
            count.to_le_bytes().as_slice(),
            value.to_le_bytes().as_slice(),
        ]
        .concat()
    };
    const GPS_CLEAR: &[Case] = &[(&["-GPS:All="], "GPS:GPSVersionID")];
    const EXIF_CLEAR: &[Case] = &[(&["-ExifIFD:All="], "ExifIFD:ExifVersion")];
    let dir = tempfile::tempdir().unwrap();
    for (label, root_pointer, child_pointer, leaf, main_fixture, make_main_gps, cases) in [
        (
            "SubIFD GPS",
            0x014a,
            0x8825,
            entry(0, 1, 4, u32::from_le_bytes([2, 3, 0, 0])),
            "Writer.jpg",
            true,
            GPS_CLEAR,
        ),
        (
            "ExifIFD GPS",
            0x8769,
            0x8825,
            entry(0, 1, 4, u32::from_le_bytes([2, 3, 0, 0])),
            "Writer.jpg",
            true,
            GPS_CLEAR,
        ),
        (
            "SubIFD ExifIFD",
            0x014a,
            0x8769,
            entry(0x9000, 7, 4, u32::from_le_bytes(*b"0231")),
            "Canon.jpg",
            false,
            EXIF_CLEAR,
        ),
    ] {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        tiff.extend(2_u16.to_le_bytes());
        tiff.extend(entry(0x010f, 2, 4, u32::from_le_bytes(*b"Foo\0")));
        tiff.extend(entry(root_pointer, 4, 1, 38));
        tiff.extend(0_u32.to_le_bytes());
        tiff.extend(1_u16.to_le_bytes());
        tiff.extend(entry(child_pointer, 4, 1, 56));
        tiff.extend(0_u32.to_le_bytes());
        tiff.extend(1_u16.to_le_bytes());
        tiff.extend(leaf);
        tiff.extend(0_u32.to_le_bytes());
        assert_eq!(tiff.len(), 74);
        let main = dir.path().join(format!("main-{label}.jpg"));
        std::fs::copy(
            fixtures::required_t_images_fixture_path(main_fixture),
            &main,
        )
        .unwrap();
        if make_main_gps {
            let set = oracle
                .command()
                .args(["-overwrite_original", "-GPS:GPSLatitude=1"])
                .arg(&main)
                .output()
                .unwrap();
            assert!(
                set.status.success(),
                "{}",
                String::from_utf8_lossy(&set.stderr)
            );
        }
        let original = [std::fs::read(&main).unwrap(), mie_holding_exif(&tiff)].concat();
        assert_eq!(
            grade(
                oracle,
                &format!("main and nested MIE SubIFD {label}"),
                "jpg",
                &original,
                cases,
            ),
            Ok(1),
            "{label}",
        );
    }
}
