//! The ninth round of Codex review threads on the beta.1 roll-up (#957) at
//! 12d92b11: deleting a chunk-backed PNG tag, and a batch read of a
//! malformed file. Oracle rows are pinned ExifTool 13.59 (`perl5.38.2
//! -I<pinned>/lib <pinned>/exiftool`; probes `-ver` = 13.59, `OOXML.docx`
//! FileType = DOCX), re-measured through `exiftool_oracle::graded()`.

use oxidex::core::WriteOutcome;
use oxidex::core::operations::{read_metadata, remove_tag, write_metadata};
use oxidex::exiftool_oracle;
use std::collections::BTreeSet;
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

// --- PRRT_kwDOQNbr5M6mTtBR: a batch read of a malformed file ----------------

/// A JPEG whose APP1 claims 4096 bytes and the file ends after 20: read
/// alone it is `Partial` -- filesystem and identity tags plus 13.59's
/// `Warning: JPEG format error`.
fn truncated_jpeg() -> Vec<u8> {
    b"\xff\xd8\xff\xe1\x10\x00Exif\x00\x00II*\x00\x08\x00\x00\x00".to_vec()
}

/// A PNG whose tEXt chunk claims 500 bytes past the end of the file.
fn truncated_png() -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
    out.extend(500u32.to_be_bytes());
    out.extend(b"tEXtAuthor\0x");
    out
}

/// The `-s` lines ExifTool prints for `file` (`Tag : value`), from a run
/// over one or several files: the block after `======== <file>` when there
/// is one. The access date is left out (reading a file moves it), and so
/// is ExifTool's own `ExifToolVersion`.
fn block_for(stdout: &str, file: &Path) -> BTreeSet<String> {
    let header = format!("======== {}", file.display());
    let lines: Vec<&str> = stdout.lines().collect();
    let body: Vec<&str> = match lines.iter().position(|line| *line == header) {
        Some(start) => lines[start + 1..]
            .iter()
            .take_while(|line| !line.starts_with("======== ") && !line.starts_with("    "))
            .copied()
            .collect(),
        None => lines
            .iter()
            .take_while(|line| !line.starts_with("    "))
            .copied()
            .collect(),
    };
    body.into_iter()
        .filter(|line| !line.starts_with("FileAccessDate") && !line.starts_with("ExifToolVersion"))
        .map(str::to_string)
        .collect()
}

/// Reading a malformed JPEG (and PNG) alone, beside another file, and in a
/// directory walk prints the same per-file lines and no error, as 13.59
/// does on each path; oxidex's file-list and directory reads used the
/// fail-fast read, turned the `Partial` read into `Error reading ...`,
/// dropped the file's metadata and counted it `could not be read`.
#[test]
fn a_malformed_file_reads_the_same_alone_in_a_list_and_in_a_directory() {
    let dir = TempDir::new().unwrap();
    let walk = dir.path().join("walk");
    fs::create_dir(&walk).unwrap();
    let bad = walk.join("bad.jpg");
    let bad_png = walk.join("bad.png");
    let good = walk.join("good.png");
    fs::write(&bad, truncated_jpeg()).unwrap();
    fs::write(&bad_png, truncated_png()).unwrap();
    fs::write(&good, png(&[b"tEXt"])).unwrap();
    let oracle = exiftool_oracle::graded();

    for malformed in [&bad, &bad_png] {
        let alone = oxidex(&["-s"], &[malformed]);
        assert_eq!(alone.status.code(), Some(0), "{}", err(&alone));
        assert_eq!(err(&alone), "");
        let single = block_for(&out(&alone), malformed);
        assert!(
            single.iter().any(|line| line.starts_with("Warning ")),
            "{single:?}"
        );

        let list = oxidex(&["-s"], &[malformed, &good]);
        assert_eq!(err(&list), "", "file list");
        assert_eq!(list.status.code(), Some(0));
        assert_eq!(block_for(&out(&list), malformed), single, "file list");
        assert!(
            out(&list).ends_with("    2 image files read\n"),
            "{}",
            out(&list)
        );

        // `-j` carries the same Status marker single-file `-j` does.
        let json = |o: &Output| -> serde_json::Value {
            let all: Vec<serde_json::Value> = serde_json::from_slice(&o.stdout).unwrap();
            all.into_iter()
                .find(|object| {
                    object
                        .get("SourceFile")
                        .is_none_or(|source| source.as_str() == Some(malformed.to_str().unwrap()))
                })
                .unwrap()
        };
        let single_json = json(&oxidex(&["-j"], &[malformed]));
        let list_json = json(&oxidex(&["-j"], &[malformed, &good]));
        assert_eq!(single_json["Status"], "Partial");
        assert_eq!(list_json["Status"], "Partial");
        assert_eq!(list_json["File:Warning"], single_json["File:Warning"]);
    }

    let walked = oxidex(&["-s"], &[&walk]);
    assert_eq!(err(&walked), "", "directory");
    assert_eq!(walked.status.code(), Some(0));
    for malformed in [&bad, &bad_png] {
        let single = block_for(&out(&oxidex(&["-s"], &[malformed])), malformed);
        assert_eq!(block_for(&out(&walked), malformed), single, "directory");
    }
    assert!(
        out(&walked).contains("    3 image files read\n"),
        "{}",
        out(&walked)
    );
    assert!(!out(&walked).contains("could not be read"));

    // `--strict` refuses the partial read on every path alike.
    let strict = oxidex(&["-s", "--strict"], &[&bad, &good]);
    assert_eq!(strict.status.code(), Some(1));
    assert!(
        err(&strict).contains("JPEG format error"),
        "{}",
        err(&strict)
    );
    assert!(out(&strict).contains("    1 files could not be read\n"));

    // 13.59: the JPEG's lines (ExifTool's own tag order aside) are oxidex's
    // on every path, it is counted read, and nothing goes to stderr; the
    // truncated PNG is read (with 13.59's own warning text) on every path.
    if let Some(oracle) = oracle {
        let run = |paths: &[&Path]| oracle.command().arg("-s").args(paths).output().unwrap();
        let ours = block_for(&out(&oxidex(&["-s"], &[&bad])), &bad);
        for (label, theirs) in [
            ("alone", run(&[&bad])),
            ("file list", run(&[&bad, &good])),
            ("directory", run(&[&walk])),
        ] {
            assert_eq!(err(&theirs), "", "13.59 {label}");
            assert_eq!(theirs.status.code(), Some(0), "13.59 {label}");
            assert_eq!(block_for(&out(&theirs), &bad), ours, "13.59 {label}");
            let png_lines = block_for(&out(&theirs), &bad_png);
            if label != "alone" {
                assert!(
                    png_lines.iter().any(|line| line.starts_with("Warning ")),
                    "13.59 {label}: {png_lines:?}"
                );
            }
        }
        assert!(out(&run(&[&bad, &good])).ends_with("    2 image files read\n"));
        assert!(out(&run(&[&walk])).ends_with("    3 image files read\n"));
    }
}
