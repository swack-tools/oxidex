//! Writer parity for the EXIF text tags whose write conversion is
//! `EncodeExifText`: ExifIFD UserComment (Exif.pm 13.59:2497-2506,
//! `0x9286`) and GPS GPSProcessingMethod / GPSAreaInformation (GPS.pm
//! 13.59:294-307, `0x001b` / `0x001c`). All three are `Writable => 'undef'`
//! with
//!
//! ```text
//! RawConvInv => 'Image::ExifTool::Exif::EncodeExifText($self,$val)',
//! ```
//!
//! and `EncodeExifText` (WriteExif.pl 13.59:131-141) stores
//!
//! - `"ASCII\0\0\0" . $val` when the value has no byte in 0x80-0xff, and
//! - `"UNICODE\0" . Encode($val,'UTF16',$order)` otherwise, where `$order`
//!   is the `ExifUnicodeByteOrder` new value -- unset by default, so
//!   `Charset::Recompose` falls back to `GetByteOrder()`: the byte order of
//!   the EXIF block being written (Charset.pm 13.59:386-389). Code points
//!   U+10000..U+10FFFE become surrogate pairs; U+10FFFF is outside that
//!   `< 0x10ffff` test and packs as its low 16 bits, `ff ff`
//!   (Charset.pm 13.59:374-384).
//!
//! The entry is always type 7 (`undef`) with count = 8 + payload length.
//! Before this change oxidex stored UserComment as a type 2 string with no
//! header (`48 65 6c 6c 6f 00`), GPSProcessingMethod as bare UTF-8 bytes,
//! and GPSAreaInformation with an `ASCII` header even over non-ASCII text.
//!
//! Every expected byte string below was produced by the pinned oracle
//! (`perl5.38.2 -I.../13.59/exiftool/lib .../13.59/exiftool/exiftool`,
//! `-ver` 13.59, `OOXML.docx` probe `DOCX`) writing the same fixture:
//! `-ExifIFD:UserComment=V`, `-GPS:GPSProcessingMethod=V`,
//! `-GPS:GPSAreaInformation=V` (all three give identical bytes), and
//! `-ExifIFD:UserComment^=` for the empty value (`-TAG=` deletes). The
//! instrument is `uc_parity.py` in the PR's evidence directory; the last
//! test re-derives the expectations from the oracle itself when one is
//! available.

use oxidex::core::operations::{modify_tag, read_metadata};
use oxidex::core::tag_value::TagValue;
use oxidex::exiftool_oracle;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    Ii,
    Mm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Container {
    Jpeg,
    Png,
}

const ORDERS: [Order; 2] = [Order::Ii, Order::Mm];
const CONTAINERS: [Container; 2] = [Container::Jpeg, Container::Png];

/// (key, IFD pointer tag, tag id) of the three `EncodeExifText` tags.
const TAGS: [(&str, u16, u16); 3] = [
    ("ExifIFD:UserComment", 0x8769, 0x9286),
    ("GPS:GPSProcessingMethod", 0x8825, 0x001b),
    ("GPS:GPSAreaInformation", 0x8825, 0x001c),
];

/// (label, value, oracle bytes in an II block, oracle bytes in an MM block).
/// The bytes are the whole `undef` value, header included; the count is
/// their length.
const CASES: [(&str, &str, &str, &str); 7] = [
    (
        "ascii",
        "Hello world",
        "415343494900000048656c6c6f20776f726c64",
        "415343494900000048656c6c6f20776f726c64",
    ),
    (
        "e_acute",
        "café",
        "554e49434f444500630061006600e900",
        "554e49434f44450000630061006600e9",
    ),
    (
        "cjk",
        "中文",
        "554e49434f4445002d4e8765",
        "554e49434f4445004e2d6587",
    ),
    (
        "astral",
        "A😀B",
        "554e49434f44450041003dd800de4200",
        "554e49434f4445000041d83dde000042",
    ),
    (
        "u10ffff",
        "A\u{10ffff}B",
        "554e49434f4445004100ffff4200",
        "554e49434f4445000041ffff0042",
    ),
    (
        "ascii_trailing_blanks",
        "trail  ",
        "4153434949000000747261696c2020",
        "4153434949000000747261696c2020",
    ),
    // `-TAG^=`: an empty value is the bare ASCII header.
    ("empty", "", "4153434949000000", "4153434949000000"),
];

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// An 8x8 baseline JPEG with no metadata segment: decodable, so the oracle
/// agrees to write it (the same image as `xp_string_write.rs`).
const BASE_JPEG_HEX: &str = "ffd8ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";

fn u16b(order: Order, v: u16) -> [u8; 2] {
    match order {
        Order::Ii => v.to_le_bytes(),
        Order::Mm => v.to_be_bytes(),
    }
}

fn u32b(order: Order, v: u32) -> [u8; 4] {
    match order {
        Order::Ii => v.to_le_bytes(),
        Order::Mm => v.to_be_bytes(),
    }
}

/// IFD0 {Artist "me", ExifIFD pointer} and an ExifIFD holding ExifVersion
/// `0232`, plus `extra` (tag, type, count, bytes) entries in the ExifIFD
/// placed after its directory.
fn tiff(order: Order, extra: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
    let mut exif_entries = vec![(0x9000u16, 7u16, 4u32, b"0232".to_vec())];
    exif_entries.extend_from_slice(extra);
    exif_entries.sort_by_key(|entry| entry.0);
    let mut out = match order {
        Order::Ii => b"II".to_vec(),
        Order::Mm => b"MM".to_vec(),
    };
    out.extend_from_slice(&u16b(order, 42));
    out.extend_from_slice(&u32b(order, 8));
    // IFD0 at 8: two entries, 8 + 2 + 24 + 4 = 38.
    out.extend_from_slice(&u16b(order, 2));
    out.extend_from_slice(&u16b(order, 0x013b));
    out.extend_from_slice(&u16b(order, 2));
    out.extend_from_slice(&u32b(order, 3));
    out.extend_from_slice(b"me\0\0");
    out.extend_from_slice(&u16b(order, 0x8769));
    out.extend_from_slice(&u16b(order, 4));
    out.extend_from_slice(&u32b(order, 1));
    out.extend_from_slice(&u32b(order, 38));
    out.extend_from_slice(&u32b(order, 0));
    // ExifIFD at 38.
    let n = exif_entries.len();
    let mut blob_at = 38 + 2 + 12 * n + 4;
    let mut blobs = Vec::new();
    out.extend_from_slice(&u16b(order, n as u16));
    for (tag, typ, count, bytes) in &exif_entries {
        out.extend_from_slice(&u16b(order, *tag));
        out.extend_from_slice(&u16b(order, *typ));
        out.extend_from_slice(&u32b(order, *count));
        if bytes.len() <= 4 {
            let mut inline = bytes.clone();
            inline.resize(4, 0);
            out.extend_from_slice(&inline);
        } else {
            out.extend_from_slice(&u32b(order, blob_at as u32));
            blobs.extend_from_slice(bytes);
            if blobs.len() % 2 == 1 {
                blobs.push(0);
            }
            blob_at = 38 + 2 + 12 * n + 4 + blobs.len();
        }
    }
    out.extend_from_slice(&u32b(order, 0));
    out.extend_from_slice(&blobs);
    out
}

fn jpeg_with(tiff: &[u8]) -> Vec<u8> {
    let base = hex(BASE_JPEG_HEX);
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(tiff);
    let mut out = base[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&app1);
    out.extend_from_slice(&base[2..]);
    out
}

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

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
    out
}

/// A 1x1 greyscale PNG with an `eXIf` chunk holding `tiff`.
fn png_with(tiff: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]));
    out.extend(png_chunk(b"eXIf", tiff));
    out.extend(png_chunk(
        b"IDAT",
        &[0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01],
    ));
    out.extend(png_chunk(b"IEND", &[]));
    out
}

fn fixture(dir: &Path, container: Container, order: Order) -> PathBuf {
    fixture_with(dir, container, order, &[])
}

fn fixture_with(
    dir: &Path,
    container: Container,
    order: Order,
    extra: &[(u16, u16, u32, Vec<u8>)],
) -> PathBuf {
    let t = tiff(order, extra);
    let (name, bytes) = match container {
        Container::Jpeg => ("f.jpg", jpeg_with(&t)),
        Container::Png => ("f.png", png_with(&t)),
    };
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The TIFF block of a JPEG (APP1 `Exif\0\0`) or a PNG (`eXIf`).
fn tiff_block(file: &[u8]) -> Option<&[u8]> {
    if file.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 4 <= file.len() && file[i] == 0xFF {
            let marker = file[i + 1];
            if marker == 0xDA || marker == 0xD9 {
                return None;
            }
            let len = u16::from_be_bytes([file[i + 2], file[i + 3]]) as usize;
            let seg = &file[i + 4..i + 2 + len];
            if marker == 0xE1 && seg.starts_with(b"Exif\0\0") {
                return Some(&seg[6..]);
            }
            i += 2 + len;
        }
        None
    } else if file.starts_with(b"\x89PNG") {
        let mut i = 8;
        while i + 8 <= file.len() {
            let len = u32::from_be_bytes(file[i..i + 4].try_into().unwrap()) as usize;
            if &file[i + 4..i + 8] == b"eXIf" {
                return Some(&file[i + 8..i + 8 + len]);
            }
            i += 12 + len;
        }
        None
    } else {
        None
    }
}

/// A raw walk to one sub-IFD entry: `(byte order, type, count, value bytes)`
/// of `tag` in the IFD that IFD0's `pointer` entry points at.
fn raw_entry(path: &Path, pointer: u16, tag: u16) -> Option<(Order, u16, u32, Vec<u8>)> {
    let file = std::fs::read(path).unwrap();
    let t = tiff_block(&file)?;
    let order = if t.starts_with(b"II") {
        Order::Ii
    } else {
        Order::Mm
    };
    let r16 = |o: usize| {
        let b = [t[o], t[o + 1]];
        match order {
            Order::Ii => u16::from_le_bytes(b),
            Order::Mm => u16::from_be_bytes(b),
        }
    };
    let r32 = |o: usize| {
        let b = [t[o], t[o + 1], t[o + 2], t[o + 3]];
        match order {
            Order::Ii => u32::from_le_bytes(b),
            Order::Mm => u32::from_be_bytes(b),
        }
    };
    let find = |ifd: usize, want: u16| -> Option<(u16, u32, Vec<u8>)> {
        for k in 0..r16(ifd) as usize {
            let p = ifd + 2 + 12 * k;
            let (id, typ, count) = (r16(p), r16(p + 2), r32(p + 4));
            if id != want {
                continue;
            }
            let size = count as usize
                * match typ {
                    3 | 8 => 2,
                    4 | 9 | 11 | 13 => 4,
                    5 | 10 | 12 => 8,
                    _ => 1,
                };
            let at = if size <= 4 {
                p + 8
            } else {
                r32(p + 8) as usize
            };
            return Some((typ, count, t[at..at + size].to_vec()));
        }
        None
    };
    let (_, _, ptr) = find(r32(4) as usize, pointer)?;
    let sub = match order {
        Order::Ii => u32::from_le_bytes(ptr[..4].try_into().unwrap()),
        Order::Mm => u32::from_be_bytes(ptr[..4].try_into().unwrap()),
    } as usize;
    let (typ, count, bytes) = find(sub, tag)?;
    Some((order, typ, count, bytes))
}

fn expected(order: Order, ii: &str, mm: &str) -> (Order, u16, u32, Vec<u8>) {
    let bytes = hex(match order {
        Order::Ii => ii,
        Order::Mm => mm,
    });
    (order, 7, bytes.len() as u32, bytes)
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

/// A value supplied through the API (`modify_tag`, the path every CLI set
/// takes) is stored as `EncodeExifText` stores it, for each of the three
/// tags, in an II and an MM block, in a JPEG APP1 and a PNG `eXIf`.
#[test]
fn typed_exif_text_is_stored_as_encode_exif_text_does() {
    let mut failures = Vec::new();
    for (key, pointer, tag) in TAGS {
        for container in CONTAINERS {
            for order in ORDERS {
                for (label, value, ii, mm) in CASES {
                    let dir = tempfile::tempdir().unwrap();
                    let path = fixture(dir.path(), container, order);
                    if let Err(error) = modify_tag(&path, key, TagValue::new_string(value)) {
                        failures.push(format!("{key} {container:?} {order:?} {label}: {error}"));
                        continue;
                    }
                    let got = raw_entry(&path, pointer, tag);
                    let want = expected(order, ii, mm);
                    if got.as_ref() != Some(&want) {
                        failures.push(format!(
                            "{key} {container:?} {order:?} {label}: got {:?}, oracle {:?}",
                            got.map(|(o, t, n, b)| (o, t, n, to_hex(&b))),
                            (want.0, want.1, want.2, to_hex(&want.3)),
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The same through the command line, for every spelling the CLI routes to
/// the three tags today. (`-TAG=` deletes, as it does in ExifTool.)
#[test]
fn cli_exif_text_is_stored_as_encode_exif_text_does() {
    let mut failures = Vec::new();
    for (key, pointer, tag) in TAGS {
        for container in CONTAINERS {
            for order in ORDERS {
                for (label, value, ii, mm) in CASES.iter().filter(|case| !case.1.is_empty()) {
                    let dir = tempfile::tempdir().unwrap();
                    let path = fixture(dir.path(), container, order);
                    let arg = format!("-{key}={value}");
                    let out = run(&[arg.as_ref(), path.as_os_str()]);
                    let got = raw_entry(&path, pointer, tag);
                    let want = expected(order, ii, mm);
                    if !out.status.success() || got.as_ref() != Some(&want) {
                        failures.push(format!(
                            "{arg} {container:?} {order:?} {label}: exit {:?} {}, got {:?}, oracle {:?}",
                            out.status.code(),
                            String::from_utf8_lossy(&out.stderr).trim(),
                            got.map(|(o, t, n, b)| (o, t, n, to_hex(&b))),
                            (want.0, want.1, want.2, to_hex(&want.3)),
                        ));
                    }
                }
                // `-TAG=` removes a present entry, as the oracle's does.
                let dir = tempfile::tempdir().unwrap();
                let path = fixture(dir.path(), container, order);
                let arg = format!("-{key}=café");
                assert!(run(&[arg.as_ref(), path.as_os_str()]).status.success());
                let arg = format!("-{key}=");
                assert!(run(&[arg.as_ref(), path.as_os_str()]).status.success());
                if raw_entry(&path, pointer, tag).is_some() {
                    failures.push(format!("{arg} {container:?} {order:?}: entry not deleted"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A write that also sets a generated-route tag (`IFD0:Artist` is a
/// migrated `Exif::Main` spelling in
/// `generated_setnewvalue_public_migration_rules.rs`) runs the generated
/// scalar path, which applies the legacy delta -- here the EXIF text tag --
/// through `tiff_surgical`. The text must come out the same there.
#[test]
fn exif_text_beside_a_generated_route_write_is_stored_the_same() {
    let mut failures = Vec::new();
    for (key, pointer, tag) in TAGS {
        for container in CONTAINERS {
            for order in ORDERS {
                for (label, value, ii, mm) in CASES.iter().filter(|case| !case.1.is_empty()) {
                    let dir = tempfile::tempdir().unwrap();
                    let path = fixture(dir.path(), container, order);
                    let arg = format!("-{key}={value}");
                    let out = run(&["-IFD0:Artist=you".as_ref(), arg.as_ref(), path.as_os_str()]);
                    let got = raw_entry(&path, pointer, tag);
                    let want = expected(order, ii, mm);
                    let artist = read_metadata(&path)
                        .ok()
                        .and_then(|m| m.get_string("IFD0:Artist").map(str::to_owned));
                    if !out.status.success()
                        || got.as_ref() != Some(&want)
                        || artist.as_deref() != Some("you")
                    {
                        failures.push(format!(
                            "-IFD0:Artist=you {arg} {container:?} {order:?} {label}: exit {:?} {}, artist {artist:?}, got {:?}, oracle {:?}",
                            out.status.code(),
                            String::from_utf8_lossy(&out.stderr).trim(),
                            got.map(|(o, t, n, b)| (o, t, n, to_hex(&b))),
                            (want.0, want.1, want.2, to_hex(&want.3)),
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An existing UserComment someone stored as a type 2 string (what oxidex
/// itself wrote before this change) is rewritten as `undef`: the tag's
/// `Writable => 'undef'` decides the format, not the old entry
/// (WriteExif.pl 13.59:1224-1241). Oracle, `-ExifIFD:UserComment=café` over
/// a `string[4] "old\0"` entry: `undef[16] UNICODE\0 ...`.
#[test]
fn a_string_typed_user_comment_is_rewritten_as_undef() {
    for container in CONTAINERS {
        for order in ORDERS {
            let dir = tempfile::tempdir().unwrap();
            let old = (0x9286, 2, 4, b"old\0".to_vec());
            let path = fixture_with(dir.path(), container, order, &[old]);
            modify_tag(&path, "ExifIFD:UserComment", TagValue::new_string("café")).unwrap();
            assert_eq!(
                raw_entry(&path, 0x8769, 0x9286),
                Some(expected(
                    order,
                    "554e49434f444500630061006600e900",
                    "554e49434f44450000630061006600e9"
                )),
                "{container:?} {order:?}"
            );
        }
    }
}

/// oxidex reads its own UserComment back as the value it was given: the
/// ExifIFD walk's generated `ConvertExifText` decodes the header, guessing
/// the UTF-16 order as ExifTool does.
#[test]
fn user_comment_reads_back_as_written() {
    for container in CONTAINERS {
        for order in ORDERS {
            for (label, value, _, _) in CASES {
                if label == "u10ffff" {
                    continue; // stored as U+FFFF: not the typed text
                }
                let dir = tempfile::tempdir().unwrap();
                let path = fixture(dir.path(), container, order);
                modify_tag(&path, "ExifIFD:UserComment", TagValue::new_string(value)).unwrap();
                let metadata = read_metadata(&path).unwrap();
                // `ConvertExifText` trims trailing blanks.
                assert_eq!(
                    metadata.get_string("ExifIFD:UserComment"),
                    Some(value.trim_end_matches(' ')),
                    "{container:?} {order:?} {label}"
                );
            }
        }
    }
}

/// The same writes graded live against the pinned oracle: for every tag,
/// container, byte order and value, the oracle writes a reference copy of
/// the fixture and oxidex writes its own; the raw entries must be
/// identical, and the oracle must read oxidex's file back as it reads its
/// own, with no warning.
#[test]
fn oracle_writes_the_same_exif_text_bytes() {
    if !exiftool_oracle::available() {
        eprintln!("skipping: no usable ExifTool oracle");
        return;
    }
    let oracle = exiftool_oracle::shared().expect("available() resolved it");
    let et = |args: &[&std::ffi::OsStr]| {
        let out = oracle.command().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{} {args:?}: {}",
            oracle.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    };
    let mut failures = Vec::new();
    for (key, pointer, tag) in TAGS {
        for container in CONTAINERS {
            for order in ORDERS {
                for (label, value, _, _) in CASES {
                    let dir = tempfile::tempdir().unwrap();
                    let reference_dir = dir.path().join("ref");
                    std::fs::create_dir(&reference_dir).unwrap();
                    let reference = fixture(&reference_dir, container, order);
                    // `^=` writes the empty value; `=` would delete.
                    let arg = if value.is_empty() {
                        format!("-{key}^=")
                    } else {
                        format!("-{key}={value}")
                    };
                    et(&[
                        "-q".as_ref(),
                        "-overwrite_original".as_ref(),
                        arg.as_ref(),
                        reference.as_os_str(),
                    ]);
                    let ours = fixture(dir.path(), container, order);
                    modify_tag(&ours, key, TagValue::new_string(value)).unwrap();
                    let want = raw_entry(&reference, pointer, tag);
                    let got = raw_entry(&ours, pointer, tag);
                    if got != want {
                        failures.push(format!(
                            "{arg} {container:?} {order:?} {label}: oxidex {:?} oracle {:?}",
                            got.map(|(o, t, n, b)| (o, t, n, to_hex(&b))),
                            want.map(|(o, t, n, b)| (o, t, n, to_hex(&b))),
                        ));
                        continue;
                    }
                    let read = |path: &Path| {
                        String::from_utf8_lossy(&et(&[
                            "-j".as_ref(),
                            "-G1".as_ref(),
                            format!("-{key}").as_ref(),
                            "-Warning".as_ref(),
                            path.as_os_str(),
                        ]))
                        .replace(&*path.to_string_lossy(), "FILE")
                    };
                    let (theirs, mine) = (read(&reference), read(&ours));
                    if theirs != mine || mine.contains("Warning") {
                        failures.push(format!(
                            "{arg} {container:?} {order:?} {label}: oracle reads oxidex's file as {mine}, its own as {theirs}"
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
