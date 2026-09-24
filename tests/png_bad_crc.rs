//! A PNG whose chunk CRC is wrong is not rewritten, as pinned ExifTool 13.59
//! refuses to rewrite it.
//!
//! `ProcessPNG` (PNG.pm 13.59:1612-1619) checks every chunk's CRC while it
//! writes, unless `FastScan` is set:
//!
//! ```perl
//! if ($verbose or $validate or ($outfile and not $fastScan)) {
//!     my $crc = CalculateCRC(\$hbuf, undef, 4);
//!     $crc = CalculateCRC(\$dbuf, $crc);
//!     unless ($crc == unpack('N',$cbuf)) {
//!         my $msg = "Bad CRC for $chunk chunk";
//!         $outfile ? $et->Error($msg, 1) : $et->Warn($msg);
//!     }
//! ```
//!
//! `Error($msg, 1)` is a minor error (ExifTool.pm 13.59:5649-5661): without
//! `-m` the file is not written ("Error: [minor] Bad CRC for IDAT chunk",
//! exit 1). That covers critical and ancillary chunks alike -- IHDR, IDAT,
//! eXIf, tEXt, zTXt, iTXt, tIME, an unknown private chunk, and a text chunk
//! after IDAT that the writer would move. Two chunks are never checked, and
//! their CRC bytes are copied as they are:
//!
//! - IEND: its branch reads the CRC and writes it straight back
//!   (PNG.pm 13.59:1546-1556);
//! - an IDAT of more than 10,000,000 bytes, which is copied with
//!   `CopyBlock` without being read (PNG.pm 13.59:1577-1584).
//!
//! With `-m` ExifTool writes and keeps the bad CRC of every chunk it copies
//! (it recomputes only a chunk it rebuilds). oxidex has no `-m` / ignore-
//! minor-errors option, so it always refuses.
//!
//! Before this change the writer recomputed every CRC, so each of these
//! files was "repaired" and the edit reported success. The oracle columns
//! here were measured by `crc_matrix.py` (PR evidence); the last test
//! re-checks the refusals against the oracle itself when one is available.

use oxidex::core::operations::{clear_all_metadata, modify_tag};
use oxidex::core::tag_value::TagValue;
use oxidex::exiftool_oracle;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn good_crc(kind: &[u8; 4], data: &[u8]) -> u32 {
    let mut input = kind.to_vec();
    input.extend_from_slice(data);
    crc32(&input)
}

/// One chunk; `bad` inverts its CRC.
fn chunk(kind: &[u8; 4], data: &[u8], bad: bool) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = good_crc(kind, data) ^ if bad { 0xFFFF_FFFF } else { 0 };
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

/// A minimal II TIFF: IFD0 { Artist "me" }.
fn tiff() -> Vec<u8> {
    let mut t = b"II\x2a\0\x08\0\0\0".to_vec();
    t.extend_from_slice(&1u16.to_le_bytes());
    t.extend_from_slice(&0x013bu16.to_le_bytes());
    t.extend_from_slice(&2u16.to_le_bytes());
    t.extend_from_slice(&3u32.to_le_bytes());
    t.extend_from_slice(b"me\0\0");
    t.extend_from_slice(&0u32.to_le_bytes());
    t
}

const IDAT: [u8; 10] = [0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01];

/// The chunk list of the test PNG, as (label, type, data). The two IDATs
/// split one zlib stream; `after_idat_text` is a tEXt after the image data,
/// which a write moves in front of it.
fn chunks() -> Vec<(&'static str, [u8; 4], Vec<u8>)> {
    vec![
        (
            "IHDR",
            *b"IHDR",
            vec![0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0],
        ),
        ("tIME", *b"tIME", vec![0x07, 0xE8, 1, 2, 3, 4, 5]),
        ("tEXt", *b"tEXt", b"Comment\0plain text".to_vec()),
        ("zTXt", *b"zTXt", {
            // keyword, NUL, method 0, zlib("someone")
            let mut d = b"Author\0\0".to_vec();
            d.extend_from_slice(&[
                0x78, 0x9c, 0x2b, 0xce, 0xcf, 0x4d, 0xcd, 0xcf, 0x4b, 0x05, 0x00, 0x0c, 0x09, 0x02,
                0xf7,
            ]);
            d
        }),
        ("iTXt", *b"iTXt", b"Copyright\0\0\0\0\0caf\xc3\xa9".to_vec()),
        ("prVt", *b"prVt", b"private ancillary".to_vec()),
        ("eXIf", *b"eXIf", tiff()),
        ("IDAT", *b"IDAT", IDAT[..6].to_vec()),
        ("IDAT2", *b"IDAT", IDAT[6..].to_vec()),
        ("after_idat_text", *b"tEXt", b"Title\0late".to_vec()),
        ("IEND", *b"IEND", Vec::new()),
    ]
}

fn png(bad: Option<&str>) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (label, kind, data) in chunks() {
        out.extend(chunk(&kind, &data, Some(label) == bad));
    }
    out
}

/// Every chunk of a PNG as (type, data, stored CRC).
fn parse(png: &[u8]) -> Vec<([u8; 4], Vec<u8>, u32)> {
    let mut out = Vec::new();
    let mut i = 8;
    while i + 12 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = png[i + 4..i + 8].try_into().unwrap();
        let data = png[i + 8..i + 8 + len].to_vec();
        let crc = u32::from_be_bytes(png[i + 8 + len..i + 12 + len].try_into().unwrap());
        out.push((kind, data, crc));
        i += 12 + len;
        if &kind == b"IEND" {
            break;
        }
    }
    out
}

fn write_png(dir: &Path, bytes: &[u8]) -> PathBuf {
    let path = dir.join("f.png");
    std::fs::write(&path, bytes).unwrap();
    path
}

fn run(args: &[&std::ffi::OsStr]) -> Output {
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run oxidex {args:?}: {e}"));
    assert!(
        output.status.code().is_some_and(|code| code != 101),
        "oxidex {args:?} died: {:?}",
        output.status
    );
    output
}

/// The edits tried on each corrupt file: an EXIF tag (the `eXIf` rewrite),
/// a PNG text tag (a new text chunk), the EXIF text tag this PR also fixes,
/// and a whole-metadata delete.
const EDITS: [&str; 4] = [
    "-IFD0:Artist=you",
    "-PNG:Title=hello",
    "-ExifIFD:UserComment=hi",
    "-all=",
];

/// A bad CRC in any checked chunk -- critical or ancillary, known or
/// private, before or after IDAT -- refuses every edit with exit 1 and
/// leaves the file byte-identical. Oracle (`crc_matrix.py`, 13.59): every
/// one of these is "Error: [minor] Bad CRC for <type> chunk", exit 1, file
/// untouched.
#[test]
fn a_bad_crc_refuses_the_write_and_leaves_the_file_untouched() {
    let mut failures = Vec::new();
    for (label, kind, _) in chunks() {
        if label == "IEND" {
            continue;
        }
        let bytes = png(Some(label));
        let chunk_type = String::from_utf8_lossy(&kind).into_owned();
        for edit in EDITS {
            let dir = tempfile::tempdir().unwrap();
            let path = write_png(dir.path(), &bytes);
            let out = run(&[edit.as_ref(), path.as_os_str()]);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let untouched = std::fs::read(&path).unwrap() == bytes;
            let named = stderr.contains(&format!("Bad CRC for {chunk_type} chunk"));
            if out.status.code() != Some(1) || !untouched || !named {
                failures.push(format!(
                    "bad {label} {edit}: exit {:?}, untouched {untouched}, stderr {}",
                    out.status.code(),
                    stderr.trim()
                ));
            }
        }
        // The library entry points refuse too.
        let dir = tempfile::tempdir().unwrap();
        let path = write_png(dir.path(), &bytes);
        let set = modify_tag(&path, "IFD0:Artist", TagValue::new_string("you"));
        let clear = clear_all_metadata(&path);
        if set.is_ok() || clear.is_ok() || std::fs::read(&path).unwrap() != bytes {
            failures.push(format!(
                "bad {label}: modify_tag {:?}, clear_all_metadata {:?}",
                set.map(|_| ()),
                clear.map(|_| ())
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// ExifTool never checks IEND's CRC: the write goes ahead and the bad CRC
/// is copied. Oracle (`-IFD0:Artist=you`, `-PNG:Title=hello`,
/// `-ExifIFD:UserComment=hi`): exit 0, "1 image files updated", IEND CRC
/// unchanged, every other chunk's CRC valid.
#[test]
fn a_bad_iend_crc_is_not_checked_and_is_copied() {
    let bytes = png(Some("IEND"));
    let bad_iend = parse(&bytes).last().unwrap().2;
    for edit in &EDITS[..3] {
        let dir = tempfile::tempdir().unwrap();
        let path = write_png(dir.path(), &bytes);
        let out = run(&[edit.as_ref(), path.as_os_str()]);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{edit}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let written = std::fs::read(&path).unwrap();
        assert_ne!(written, bytes, "{edit}: nothing was written");
        let chunks = parse(&written);
        let (kind, _, crc) = chunks.last().unwrap();
        assert_eq!(kind, b"IEND");
        assert_eq!(*crc, bad_iend, "{edit}: IEND CRC must be copied");
        for (kind, data, crc) in &chunks[..chunks.len() - 1] {
            assert_eq!(*crc, good_crc(kind, data), "{edit}: {kind:?}");
        }
    }
}

/// A chunk this writer carries keeps its CRC bytes (ExifTool writes back
/// the `$cbuf` it read), while a chunk it rebuilds gets a fresh one. On a
/// file with valid CRCs the two agree, so every CRC is valid afterwards.
#[test]
fn every_crc_is_valid_after_a_write_of_a_sound_file() {
    let bytes = png(None);
    for edit in &EDITS[..3] {
        let dir = tempfile::tempdir().unwrap();
        let path = write_png(dir.path(), &bytes);
        let out = run(&[edit.as_ref(), path.as_os_str()]);
        assert_eq!(out.status.code(), Some(0), "{edit}");
        for (kind, data, crc) in parse(&std::fs::read(&path).unwrap()) {
            assert_eq!(crc, good_crc(&kind, &data), "{edit}: {kind:?}");
        }
    }
}

fn big_idat_png(len: usize) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(
        b"IHDR",
        &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0],
        false,
    ));
    out.extend(chunk(b"IDAT", &vec![0u8; len], true));
    out.extend(chunk(b"IEND", &[], false));
    out
}

/// An IDAT of more than 10,000,000 bytes is copied unread
/// (`$chunkSizeLimit`), so its CRC is never checked: the oracle writes the
/// file and keeps the bad CRC. At exactly 10,000,000 bytes the chunk is
/// read and checked, and the write is refused.
#[test]
fn an_oversized_idat_is_not_checked_and_keeps_its_crc() {
    let dir = tempfile::tempdir().unwrap();

    let bytes = big_idat_png(10_000_001);
    let bad = parse(&bytes)[1].2;
    let path = write_png(dir.path(), &bytes);
    let out = run(&["-IFD0:Artist=you".as_ref(), path.as_os_str()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let written = parse(&std::fs::read(&path).unwrap());
    let idat = written.iter().find(|c| &c.0 == b"IDAT").unwrap();
    assert_eq!(idat.2, bad, "the oversized IDAT's CRC must be copied");

    let bytes = big_idat_png(10_000_000);
    let path = write_png(dir.path(), &bytes);
    let out = run(&["-IFD0:Artist=you".as_ref(), path.as_os_str()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Bad CRC for IDAT chunk"));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

/// The refusals above graded live against the pinned oracle: it too exits 1
/// on every checked chunk and leaves the file untouched, and writes over a
/// bad IEND CRC, keeping it.
#[test]
fn the_oracle_refuses_the_same_files() {
    if !exiftool_oracle::available() {
        eprintln!("skipping: no usable ExifTool oracle");
        return;
    }
    let oracle = exiftool_oracle::shared().expect("available() resolved it");
    for (label, _, _) in chunks() {
        let bytes = png(Some(label));
        let dir = tempfile::tempdir().unwrap();
        let path = write_png(dir.path(), &bytes);
        let out = oracle
            .command()
            .args(["-overwrite_original", "-IFD0:Artist=you"])
            .arg(&path)
            .output()
            .unwrap();
        let written = std::fs::read(&path).unwrap();
        if label == "IEND" {
            assert!(out.status.success(), "oracle, bad IEND");
            assert_eq!(
                parse(&written).last().unwrap().2,
                parse(&bytes).last().unwrap().2
            );
        } else {
            assert_eq!(out.status.code(), Some(1), "oracle, bad {label}");
            assert_eq!(written, bytes, "oracle, bad {label}");
        }
    }
}
