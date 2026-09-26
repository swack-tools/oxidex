//! The ninth round of Codex review threads on the beta.1 roll-up (#957) at
//! 12d92b11: deleting a chunk-backed PNG tag. Oracle rows are pinned
//! ExifTool 13.59 (`perl5.38.2 -I<pinned>/lib <pinned>/exiftool`; probes
//! `-ver` = 13.59, `OOXML.docx` FileType = DOCX), re-measured through
//! `exiftool_oracle::graded()`.

use oxidex::core::WriteOutcome;
use oxidex::core::operations::{read_metadata, remove_tag, write_metadata};
use oxidex::exiftool_oracle;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn oxidex(args: &[&str], paths: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .args(paths)
        .output()
        .expect("run oxidex")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

// --- a PNG built chunk by chunk ---------------------------------------------

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
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

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

/// A 1x1 RGB PNG carrying `ancillary` between IHDR and IDAT. The IDAT is
/// the zlib stream of one filtered scanline (`00 00 00 00`), stored.
fn png(ancillary: &[&[u8; 4]]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
    for kind in ancillary {
        let data: Vec<u8> = match *kind {
            b"tIME" => vec![0x07, 0xE4, 1, 2, 3, 4, 5], // 2020:01:02 03:04:05
            b"gAMA" => 45455u32.to_be_bytes().to_vec(),
            b"sRGB" => vec![0],
            b"pHYs" => [&2834u32.to_be_bytes()[..], &2834u32.to_be_bytes(), &[1]].concat(),
            b"bKGD" => vec![0, 255, 0, 255, 0, 255],
            b"tEXt" => b"Author\0Someone".to_vec(),
            other => panic!("no sample for {}", String::from_utf8_lossy(other)),
        };
        out.extend(chunk(kind, &data));
    }
    out.extend(chunk(
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x04, 0x00, 0xFB, 0xFF, 0, 0, 0, 0, 0x00, 0x04, 0x00, 0x01,
        ],
    ));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// The chunk types of a PNG file, in order.
fn chunk_types(path: &Path) -> Vec<String> {
    let bytes = fs::read(path).unwrap();
    let mut at = 8;
    let mut kinds = Vec::new();
    while at + 8 <= bytes.len() {
        let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        kinds.push(String::from_utf8_lossy(&bytes[at + 4..at + 8]).into_owned());
        at += 12 + length;
    }
    kinds
}

const ALL: &[&[u8; 4]] = &[b"tIME", b"gAMA", b"sRGB", b"pHYs", b"bKGD", b"tEXt"];

fn write_png(dir: &TempDir, name: &str, ancillary: &[&[u8; 4]]) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, png(ancillary)).unwrap();
    path
}

fn without(kinds: &[String], gone: &str) -> Vec<String> {
    kinds.iter().filter(|k| *k != gone).cloned().collect()
}

// --- PRRT_kwDOQNbr5M6mTtBM: deleting a chunk-backed PNG tag ------------------

/// 13.59 on a PNG carrying tIME, gAMA, sRGB, pHYs, bKGD and tEXt:
/// `-PNG:ModifyDate=`, `-PNG:Gamma=` and `-PNG:SRGBRendering=` each print
/// `1 image files updated` and drop exactly that tag's chunk. oxidex carried
/// the chunk back (tIME, gAMA: `after writing, PNG:<Name> is still present`,
/// exit 1) or called the deletion a no-op because its reader surfaces no
/// SRGBRendering (`1 image files unchanged`, sRGB kept).
#[test]
fn deleting_a_chunk_backed_png_tag_drops_its_chunk() {
    let dir = TempDir::new().unwrap();
    let original = write_png(&dir, "original.png", ALL);
    let before = chunk_types(&original);
    assert_eq!(
        before,
        [
            "IHDR", "tIME", "gAMA", "sRGB", "pHYs", "bKGD", "tEXt", "IDAT", "IEND"
        ]
    );
    let oracle = exiftool_oracle::graded();
    for (arg, kind) in [
        ("-PNG:ModifyDate=", "tIME"),
        ("-PNG:Gamma=", "gAMA"),
        ("-PNG:SRGBRendering=", "sRGB"),
    ] {
        let ours = write_png(&dir, "ours.png", ALL);
        let o = oxidex(&[arg], &[&ours]);
        assert_eq!(
            (o.status.code(), out(&o).as_str()),
            (Some(0), "    1 image files updated\n"),
            "{arg}: {}",
            err(&o)
        );
        assert_eq!(chunk_types(&ours), without(&before, kind), "{arg}");

        if let Some(oracle) = oracle {
            let theirs = write_png(&dir, "theirs.png", ALL);
            let t = oracle
                .command()
                .args(["-overwrite_original", arg])
                .arg(&theirs)
                .output()
                .unwrap();
            assert_eq!(out(&t), "    1 image files updated\n", "13.59 {arg}");
            assert_eq!(chunk_types(&theirs), without(&before, kind), "13.59 {arg}");
            // Nothing else moved: the two files are the same bytes.
            assert_eq!(
                fs::read(&ours).unwrap(),
                fs::read(&theirs).unwrap(),
                "{arg}"
            );
        }
    }
    // What stays behind still reads: the ModifyDate deletion left Gamma.
    let ours = write_png(&dir, "readback.png", ALL);
    oxidex(&["-PNG:ModifyDate="], &[&ours]);
    let map = read_metadata(&ours).unwrap();
    assert!(!map.contains_key("PNG:ModifyDate"));
    assert!(map.contains_key("PNG:Gamma"));
}

/// The library's deletions take the same route: `remove_tag` names the tag,
/// and `write_metadata` with the row taken out of a read map deletes it
/// (the API's approved semantics: a row removed from a read map is
/// deleted). Both used to fail the read-back with `TagsNotWritten`.
#[test]
fn library_deletions_of_png_modify_date_drop_the_time_chunk() {
    let dir = TempDir::new().unwrap();
    let path = write_png(&dir, "remove.png", ALL);
    assert_eq!(
        remove_tag(&path, "PNG:ModifyDate").unwrap(),
        WriteOutcome::Updated
    );
    assert!(!chunk_types(&path).contains(&"tIME".to_string()));

    let path = write_png(&dir, "write.png", ALL);
    let mut map = read_metadata(&path).unwrap();
    assert!(map.remove("PNG:ModifyDate").is_some());
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    let kinds = chunk_types(&path);
    assert!(!kinds.contains(&"tIME".to_string()), "{kinds:?}");
    assert!(kinds.contains(&"gAMA".to_string()), "{kinds:?}");
}

/// An unchanged value is still carried, byte for byte: an unrelated edit
/// keeps tIME/gAMA/sRGB, a same-value ModifyDate set rewrites nothing
/// else, and a deletion that names no chunk the file holds is 13.59's
/// `1 image files unchanged` (bytes untouched).
#[test]
fn unchanged_or_absent_chunk_tags_are_carried_or_no_ops() {
    let dir = TempDir::new().unwrap();
    let path = write_png(&dir, "edit.png", ALL);
    let o = oxidex(&["-PNG:Author=Other"], &[&path]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let original = png(ALL);
    let edited = fs::read(&path).unwrap();
    // tIME, gAMA, sRGB, pHYs and bKGD precede the (rewritten) tEXt chunk
    // unchanged.
    let prefix = 8 + 25 + 19 + 16 + 13 + 21 + 18;
    assert_eq!(edited[..prefix], original[..prefix]);

    let oracle = exiftool_oracle::graded();
    for arg in ["-PNG:ModifyDate=", "-PNG:Gamma=", "-PNG:SRGBRendering="] {
        let path = write_png(&dir, "bare.png", &[b"pHYs", b"tEXt"]);
        let before = fs::read(&path).unwrap();
        let o = oxidex(&[arg], &[&path]);
        assert_eq!(
            out(&o),
            "    0 image files updated\n    1 image files unchanged\n",
            "{arg}: {}",
            err(&o)
        );
        assert_eq!(fs::read(&path).unwrap(), before, "{arg}");
        if let Some(oracle) = oracle {
            let theirs = write_png(&dir, "bare-theirs.png", &[b"pHYs", b"tEXt"]);
            let t = oracle
                .command()
                .args(["-overwrite_original", arg])
                .arg(&theirs)
                .output()
                .unwrap();
            assert_eq!(
                out(&t),
                "    0 image files updated\n    1 image files unchanged\n",
                "13.59 {arg}"
            );
        }
    }

    // A later set of the same tag wins over its deletion (13.59:
    // `-PNG:ModifyDate= -PNG:ModifyDate=<d>` writes <d>).
    let path = write_png(&dir, "reset.png", ALL);
    let o = oxidex(
        &["-PNG:ModifyDate=", "-PNG:ModifyDate=2021:05:06 07:08:09"],
        &[&path],
    );
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&oxidex(&["-s3", "-PNG:ModifyDate"], &[&path])),
        "2021:05:06 07:08:09\n"
    );
}
