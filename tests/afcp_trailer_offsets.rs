//! A JPEG write that changes the file's length must re-base the absolute
//! offsets of an AFCP trailer copied after the EOI.
//!
//! AFCP (AFCP.pm 13.59:72-222) stores each entry's data position, and the
//! position of its own header in its end-of-file record, as absolute file
//! offsets. oxidex's JPEG writers copy everything after the edited EXIF block
//! verbatim, so before the fix a growing write left those offsets pointing
//! short of the data and a shrinking one past it; pinned ExifTool 13.59 then
//! read the output with `Warning: [minor] Adjusted AFCP offsets by N`
//! (AFCP.pm:154) -- 246 for the long `IFD0:Artist` below on
//! `t/images/ExifTool.jpg`, -850 for `-all=`, -32 for `-IFD0:Software=`.
//! ExifTool's own writer re-bases them (AFCP.pm:205-217, Writer.pl
//! 13.59:5522-5543).
//!
//! Every case runs the real CLI (`CARGO_BIN_EXE_oxidex`) on a copy of the
//! fixture and checks the trailer structurally -- no oracle needed: the
//! end-of-file record points at the moved header, and each entry offset
//! points at the same bytes it did before, moved by the length delta. With the
//! pinned oracle available, the output must also read back without the
//! AFCP warning and with every AFCP and IPTC tag (values and binary payloads)
//! unchanged under `-a -G1 -s -AFCP:all -IPTC:all`.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle;
use std::path::{Path, PathBuf};
use std::process::Command;

/// An 8x8 baseline JPEG with no metadata segment (DQT, SOF0, DHT, SOS):
/// decodable, so the oracle agrees to read and write it.
const BASE_JPEG_HEX: &str = "ffd8ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";

const LONG_ARTIST: &str = "-IFD0:Artist=a very long artist value that surely does not fit in the old slot at all 1234567890";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// A big-endian EXIF APP1 holding IFD0 { Make "Oxi", Artist `artist` }.
fn exif_app1(artist: &str) -> Vec<u8> {
    let mut artist = artist.as_bytes().to_vec();
    artist.push(0);
    let mut tiff = b"MM\0\x2a\0\0\0\x08".to_vec();
    tiff.extend_from_slice(&2u16.to_be_bytes());
    let blob_at = 8 + 2 + 2 * 12 + 4;
    // 0x010f Make, ASCII, 4 bytes inline.
    tiff.extend_from_slice(&[0x01, 0x0f, 0, 2, 0, 0, 0, 4, b'O', b'x', b'i', 0]);
    // 0x013b Artist, ASCII, out of line.
    tiff.extend_from_slice(&[0x01, 0x3b, 0, 2]);
    tiff.extend_from_slice(&(artist.len() as u32).to_be_bytes());
    tiff.extend_from_slice(&(blob_at as u32).to_be_bytes());
    tiff.extend_from_slice(&0u32.to_be_bytes());
    tiff.extend_from_slice(&artist);
    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);
    app1
}

/// An AFCP trailer written at absolute position `start`.
fn afcp(start: usize, entries: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
    let mut out = b"AXS!".to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    let mut data_at = start + 12 + 12 * entries.len();
    let mut data = Vec::new();
    for (tag, value) in entries {
        out.extend_from_slice(tag.as_slice());
        out.extend_from_slice(&(value.len() as u32).to_be_bytes());
        out.extend_from_slice(&(data_at as u32).to_be_bytes());
        data.extend_from_slice(value);
        data_at += value.len();
    }
    out.extend_from_slice(&data);
    out.extend_from_slice(b"AXS!");
    out.extend_from_slice(&(start as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out
}

/// IPTC record 2: version 2, By-line "afcp byline", Caption "afcp caption".
fn iptc() -> Vec<u8> {
    let mut out = Vec::new();
    for (dataset, value) in [
        (0u8, &[0u8, 2][..]),
        (80, b"afcp byline"),
        (120, b"afcp caption"),
    ] {
        out.extend_from_slice(&[0x1c, 2, dataset]);
        out.extend_from_slice(&(value.len() as u16).to_be_bytes());
        out.extend_from_slice(value);
    }
    out
}

/// The base JPEG with an EXIF block, then an AFCP trailer, then a FotoStation
/// record, so AFCP is not the last trailer (as in `t/images/ExifTool.jpg`).
fn synthetic_jpeg() -> Vec<u8> {
    let base = hex(BASE_JPEG_HEX);
    let mut file = base[..2].to_vec();
    file.extend_from_slice(&exif_app1("abcd"));
    file.extend_from_slice(&base[2..]);
    let start = file.len();
    file.extend_from_slice(&afcp(
        start,
        &[(b"IPTC", &iptc()), (b"TEXT", b"afcp text payload")],
    ));
    // FotoStation record: data, then int16u tag, int32u size (incl. the
    // 10-byte footer), 0xa1b2c3d4 (FotoStation.pm 13.59:134-138).
    let data = b"foto";
    file.extend_from_slice(data);
    file.extend_from_slice(&3u16.to_be_bytes());
    file.extend_from_slice(&((data.len() + 10) as u32).to_be_bytes());
    file.extend_from_slice(&[0xa1, 0xb2, 0xc3, 0xd4]);
    file
}

/// One AFCP trailer as found in a file: header, end-of-file record, entries.
#[derive(Debug)]
struct Afcp {
    start: usize,
    eof_record: usize,
    entries: Vec<([u8; 4], usize, usize)>,
}

fn be32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

/// The big-endian AFCP trailers of `bytes` whose end-of-file record points at
/// a matching header (the fixtures are all `AXS!`).
fn afcp_trailers(bytes: &[u8]) -> Vec<Afcp> {
    let mut found = Vec::new();
    for eof_record in 0..bytes.len().saturating_sub(11) {
        if &bytes[eof_record..eof_record + 4] != b"AXS!" {
            continue;
        }
        let start = be32(bytes, eof_record + 4);
        if start + 12 > eof_record || &bytes[start..start + 4] != b"AXS!" {
            continue;
        }
        let count = u16::from_be_bytes([bytes[start + 6], bytes[start + 7]]) as usize;
        let entries = (0..count)
            .map(|i| {
                let entry = start + 12 + 12 * i;
                (
                    bytes[entry..entry + 4].try_into().unwrap(),
                    be32(bytes, entry + 4),
                    be32(bytes, entry + 8),
                )
            })
            .collect();
        found.push(Afcp {
            start,
            eof_record,
            entries,
        });
    }
    found
}

/// Every AFCP trailer of `before` is in `after`, moved by the length delta,
/// with each entry offset re-based onto the same bytes.
fn assert_afcp_rebased(before: &[u8], after: &[u8], case: &str) {
    let delta = after.len() as isize - before.len() as isize;
    let old = afcp_trailers(before);
    assert!(!old.is_empty(), "{case}: fixture has no AFCP trailer");
    let new = afcp_trailers(after);
    assert_eq!(
        new.len(),
        old.len(),
        "{case}: AFCP end-of-file record no longer points at its header \
         (delta {delta}): before {old:?}, after {new:?}"
    );
    for (old, new) in old.iter().zip(&new) {
        assert_eq!(
            new.start as isize,
            old.start as isize + delta,
            "{case}: header moved"
        );
        assert_eq!(new.eof_record as isize, old.eof_record as isize + delta);
        assert_eq!(new.entries.len(), old.entries.len());
        for ((tag, size, offset), (new_tag, new_size, new_offset)) in
            old.entries.iter().zip(&new.entries)
        {
            assert_eq!((tag, size), (new_tag, new_size));
            assert_eq!(
                *new_offset as isize,
                *offset as isize + delta,
                "{case}: {} offset not re-based",
                String::from_utf8_lossy(tag)
            );
            assert_eq!(
                &after[*new_offset..*new_offset + *new_size],
                &before[*offset..*offset + *size],
                "{case}: {} data",
                String::from_utf8_lossy(tag)
            );
        }
        // Nothing else inside the trailer changed.
        let mut expected = before[old.start..old.eof_record + 12].to_vec();
        for (i, (_, _, offset)) in old.entries.iter().enumerate() {
            let field = 12 + 12 * i + 8;
            expected[field..field + 4]
                .copy_from_slice(&((*offset as isize + delta) as u32).to_be_bytes());
        }
        let eof = old.eof_record - old.start + 4;
        expected[eof..eof + 4].copy_from_slice(&(new.start as u32).to_be_bytes());
        assert_eq!(
            &after[new.start..new.eof_record + 12],
            expected.as_slice(),
            "{case}"
        );
    }
}

/// Run the oxidex CLI with `args` on a copy of `source`; return the output
/// bytes and whether it succeeded.
fn oxidex_write(source: &[u8], args: &[&str], dir: &Path, name: &str) -> (Vec<u8>, bool, String) {
    let path = dir.join(name);
    std::fs::write(&path, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(&path)
        .output()
        .unwrap();
    (
        std::fs::read(&path).unwrap(),
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The oracle's `-Warning` lines and `-AFCP:all -IPTC:all` read-back (JSON,
/// binary payloads included as base64) for `path`, or `None` without an
/// oracle.
fn oracle_view(path: &Path) -> Option<(String, serde_json::Value)> {
    if !exiftool_oracle::available() {
        return None;
    }
    let oracle = exiftool_oracle::shared().ok()?;
    let warnings = oracle
        .command()
        .args(["-a", "-s", "-Warning"])
        .arg(path)
        .output()
        .unwrap();
    let tags = oracle
        .command()
        .args(["-j", "-a", "-G1", "-s", "-b", "-AFCP:all", "-IPTC:all"])
        .arg(path)
        .output()
        .unwrap();
    assert!(tags.status.success(), "{}", oracle.display());
    let mut json: serde_json::Value = serde_json::from_slice(&tags.stdout).unwrap();
    json[0].as_object_mut().unwrap().remove("SourceFile");
    Some((String::from_utf8_lossy(&warnings.stdout).into_owned(), json))
}

/// How a write is expected to change the file's length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Length {
    Grows,
    Shrinks,
    Same,
    /// Not asserted: `-EXIF:All=` is a same-length rewrite or a no-op at
    /// e4edc55c but removes the block once the EXIF transaction drops it; the
    /// trailer must be right either way.
    Any,
}

/// Each `(args, length)` write on a copy of `source`, checked structurally
/// and (with an oracle) against the original's read-back.
fn check_fixture(source: &[u8], label: &str, edits: &[(&[&str], Length)]) {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join(format!("original-{label}"));
    std::fs::write(&original, source).unwrap();
    let before = oracle_view(&original);
    if let Some((warnings, _)) = &before {
        assert!(
            !warnings.contains("AFCP"),
            "{label}: fixture already warns: {warnings}"
        );
    }
    for (index, (args, length)) in edits.iter().enumerate() {
        let case = format!("{label} {args:?}");
        let name = format!("{index}-{label}");
        let (after, ok, stderr) = oxidex_write(source, args, dir.path(), &name);
        assert!(ok, "{case}: oxidex failed: {stderr}");
        match length {
            Length::Grows => assert!(after.len() > source.len(), "{case}: did not grow"),
            Length::Shrinks => assert!(after.len() < source.len(), "{case}: did not shrink"),
            Length::Same => {
                assert_eq!(after.len(), source.len(), "{case}: length changed");
                assert_ne!(after, source, "{case}: nothing was written");
            }
            Length::Any => {}
        }
        assert_afcp_rebased(source, &after, &case);
        if let Some((_, before_tags)) = &before {
            let (warnings, after_tags) = oracle_view(&dir.path().join(&name)).unwrap();
            assert!(
                !warnings.contains("Adjusted AFCP offsets"),
                "{case}: {warnings}"
            );
            assert_eq!(&after_tags, before_tags, "{case}: AFCP/IPTC read-back");
        }
    }
}

#[test]
fn afcp_offsets_follow_writes_to_a_synthetic_jpeg() {
    check_fixture(
        &synthetic_jpeg(),
        "synthetic.jpg",
        &[
            (&[LONG_ARTIST], Length::Grows),
            (&["-all="], Length::Shrinks),
            (&["-IFD0:Make="], Length::Shrinks),
            (&["-EXIF:All="], Length::Any),
            (&["-IFD0:Make=Oxy"], Length::Same),
        ],
    );
}

/// At e4edc55c pinned ExifTool reads these outputs with "Adjusted AFCP
/// offsets by" 246 (`LONG_ARTIST`), -850 (`-all=`) and -32
/// (`-IFD0:Software=`).
#[test]
fn afcp_offsets_follow_writes_to_pinned_exiftool_jpg() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("ExifTool.jpg") else {
        eprintln!("skipping: pinned fixture ExifTool.jpg is absent");
        return;
    };
    check_fixture(
        &std::fs::read(&path).unwrap(),
        "ExifTool.jpg",
        &[
            (&[LONG_ARTIST], Length::Grows),
            (&["-all="], Length::Shrinks),
            (&["-IFD0:Software="], Length::Shrinks),
            (&["-EXIF:All="], Length::Any),
            (&["-IFD0:Make=FUJIFILX"], Length::Same),
        ],
    );
}

/// `t/images/AFCP.jpg` has no EXIF, so every EXIF write creates the block:
/// both grow it (156 and 45 bytes at e4edc55c, each warned about).
#[test]
fn afcp_offsets_follow_writes_to_pinned_afcp_jpg() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("AFCP.jpg") else {
        eprintln!("skipping: pinned fixture AFCP.jpg is absent");
        return;
    };
    check_fixture(
        &std::fs::read(&path).unwrap(),
        "AFCP.jpg",
        &[
            (&[LONG_ARTIST], Length::Grows),
            (&["-IFD0:Make=FUJIFILX"], Length::Grows),
        ],
    );
}

/// An AFCP trailer whose end-of-file record is already stale cannot be
/// re-based exactly: a length-changing write is refused, exit 1, and the file
/// is left byte-identical.
#[test]
fn a_stale_afcp_trailer_refuses_a_length_changing_write() {
    let mut source = synthetic_jpeg();
    let start = afcp_trailers(&source)[0].start;
    let eof_record = afcp_trailers(&source)[0].eof_record;
    source[eof_record + 4..eof_record + 8].copy_from_slice(&((start - 5) as u32).to_be_bytes());
    let dir = tempfile::tempdir().unwrap();
    let path: PathBuf = dir.path().join("stale.jpg");
    std::fs::write(&path, &source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .arg(LONG_ARTIST)
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("AFCP"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&path).unwrap(), source);
}
