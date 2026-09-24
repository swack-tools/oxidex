//! Writer round trips for the five Windows XP strings, 0x9c9b-0x9c9f
//! XPTitle, XPComment, XPAuthor, XPKeywords, XPSubject.
//!
//! Exif.pm 13.59 (`0x9c9b => {` at :2629, the other four follow it) declares
//! each one
//!
//! ```text
//! Format => 'undef', Writable => 'int8u', WriteGroup => 'IFD0',
//! ValueConv    => '$self->Decode($val,"UCS2","II")',
//! ValueConvInv => '$self->Encode($val,"UCS2","II") . "\0\0"',
//! ```
//!
//! so what ExifTool puts on disk for a value is its UCS-2 code units,
//! little-endian whatever the file's byte order, then a NUL pair, as an
//! `int8u` (BYTE, type 1) entry -- or `undef` (type 7) when it rewrites an
//! existing `undef` entry (WriteExif.pl 13.59:1225-1242: the IFD format
//! becomes `Writable` only when the old format differs from `Format`). A copy
//! (`-TagsFromFile`) goes through the same pair, so it writes
//! `ValueConvInv(ValueConv(raw))`, never the source's raw bytes.
//!
//! Every expected byte string below was produced by the pinned oracle
//! (`perl5.38.2 -I.../13.59/exiftool/lib .../13.59/exiftool/exiftool`,
//! `-ver` 13.59, `OOXML.docx` probe `DOCX`) from the same input: `-XPTitle=V`
//! for a value, `-TagsFromFile SRC DST` for a copy. Before the fix, every
//! oxidex writer serialized the decoded text's UTF-8 bytes as an ASCII
//! (type 2) entry, which ExifTool then decodes as UCS-2 into mojibake
//! (`Title` came back `楔汴e`).
//!
//! A value's provenance decides its bytes, as it does in ExifTool. A value
//! the caller supplies (`-IFD0:XPTitle=V`, `modify_tag`) is encoded from its
//! code points with `pack('v*')`, which keeps only the low 16 bits of one
//! above U+FFFF (`A🎌` -> 41 00 8c f3 00 00). A value copied from a file
//! (`-TagsFromFile`, `copy_metadata`, the PNG `eXIf` rebuild) is the
//! source's stored code units re-packed, so a surrogate pair -- or a lone
//! surrogate -- survives the copy exactly as ExifTool's copy keeps it.
//!
//! The last test re-derives the expectations from the oracle itself when one
//! is available.

use oxidex::core::operations::{copy_metadata, modify_tag, read_metadata};
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::tag_value::TagValue;
use oxidex::exiftool_oracle;
use std::collections::BTreeMap;
use std::path::Path;

const XP_TITLE: u16 = 0x9c9b;

/// The five tags in id order, with the name oxidex and ExifTool report.
const XP_TAGS: [(u16, &str); 5] = [
    (0x9c9b, "XPTitle"),
    (0x9c9c, "XPComment"),
    (0x9c9d, "XPAuthor"),
    (0x9c9e, "XPKeywords"),
    (0x9c9f, "XPSubject"),
];

/// An 8x8 baseline JPEG with no metadata segment (APP0-free, DQT, SOF0, DHT,
/// SOS): decodable, so the oracle agrees to write it.
const BASE_JPEG_HEX: &str = "ffd8ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    Ii,
    Mm,
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// UCS-2LE code units of `text` plus the NUL pair: ExifTool's
/// `ValueConvInv` for every value below that has no code point above U+FFFF.
fn ucs2(text: &str) -> Vec<u8> {
    let mut out: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    out.extend_from_slice(&[0, 0]);
    out
}

/// A TIFF block with one IFD0 holding `entries` (tag, type, count, bytes).
fn tiff(order: Order, entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
    let u16b = |v: u16| match order {
        Order::Ii => v.to_le_bytes(),
        Order::Mm => v.to_be_bytes(),
    };
    let u32b = |v: u32| match order {
        Order::Ii => v.to_le_bytes(),
        Order::Mm => v.to_be_bytes(),
    };
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|e| e.0);
    let mut out = match order {
        Order::Ii => b"II".to_vec(),
        Order::Mm => b"MM".to_vec(),
    };
    out.extend_from_slice(&u16b(42));
    out.extend_from_slice(&u32b(8));
    out.extend_from_slice(&u16b(sorted.len() as u16));
    let mut blob_at = 8 + 2 + 12 * sorted.len() + 4;
    let mut blobs = Vec::new();
    for (tag, typ, count, bytes) in &sorted {
        out.extend_from_slice(&u16b(*tag));
        out.extend_from_slice(&u16b(*typ));
        out.extend_from_slice(&u32b(*count));
        if bytes.len() <= 4 {
            let mut inline = bytes.clone();
            inline.resize(4, 0);
            out.extend_from_slice(&inline);
        } else {
            out.extend_from_slice(&u32b(blob_at as u32));
            blobs.extend_from_slice(bytes);
            if blobs.len() % 2 == 1 {
                blobs.push(0);
            }
            blob_at = 8 + 2 + 12 * sorted.len() + 4 + blobs.len();
        }
    }
    out.extend_from_slice(&u32b(0));
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

/// A 1x1 greyscale PNG, with an `eXIf` chunk when `tiff` is given.
fn png_with(tiff: Option<&[u8]>) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]));
    if let Some(tiff) = tiff {
        out.extend(png_chunk(b"eXIf", tiff));
    }
    // zlib stream of the one filtered scanline `00 00`
    out.extend(png_chunk(
        b"IDAT",
        &[0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01],
    ));
    out.extend(png_chunk(b"IEND", &[]));
    out
}

/// The TIFF block of a JPEG (APP1 `Exif\0\0`), a PNG (`eXIf`) or a TIFF.
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
    } else if file.starts_with(b"II") || file.starts_with(b"MM") {
        Some(file)
    } else {
        None
    }
}

/// The raw IFD0 XP entries of a file: id -> (type, value bytes).
fn xp_entries(path: &Path) -> BTreeMap<u16, (u16, Vec<u8>)> {
    let file = std::fs::read(path).unwrap();
    let Some(t) = tiff_block(&file) else {
        return BTreeMap::new();
    };
    let le = t.starts_with(b"II");
    let r16 = |o: usize| {
        let b = [t[o], t[o + 1]];
        if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    };
    let r32 = |o: usize| {
        let b = [t[o], t[o + 1], t[o + 2], t[o + 3]];
        if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    };
    let ifd = r32(4) as usize;
    let mut out = BTreeMap::new();
    for k in 0..r16(ifd) as usize {
        let p = ifd + 2 + 12 * k;
        let (tag, typ, count) = (r16(p), r16(p + 2), r32(p + 4) as usize);
        if !(0x9c9b..=0x9c9f).contains(&tag) {
            continue;
        }
        let size = count
            * match typ {
                3 | 8 => 2,
                4 | 9 | 11 => 4,
                5 | 10 | 12 => 8,
                _ => 1,
            };
        let at = if size <= 4 {
            p + 8
        } else {
            r32(p + 8) as usize
        };
        out.insert(tag, (typ, t[at..at + size].to_vec()));
    }
    out
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The destinations a copy lands in: a JPEG with no metadata, a TIFF and a
/// PNG. The TIFF and the PNG each hold an unrelated IFD0 tag so the writer
/// has a directory to write into.
fn destinations(dir: &Path, order: Order) -> Vec<std::path::PathBuf> {
    let artist = (0x013b, 2, 3, b"me\0".to_vec());
    let tiff_dest = tiff(order, std::slice::from_ref(&artist));
    vec![
        write(dir, "dest.jpg", &hex(BASE_JPEG_HEX)),
        write(dir, "dest.tif", &tiff_dest),
        write(dir, "dest.png", &png_with(Some(&tiff(order, &[artist])))),
    ]
}

fn xp_text(path: &Path, name: &str) -> Option<String> {
    read_metadata(path)
        .unwrap()
        .get_string(&format!("IFD0:{name}"))
        .map(str::to_owned)
}

/// -TagsFromFile of all five tags, from an II and an MM source, into a JPEG,
/// a TIFF and a PNG destination (the PNG through the `eXIf` rebuild): each
/// lands as a BYTE entry holding UCS-2LE text and a NUL pair -- little-endian
/// in an MM file too -- and reads back as the source text. Oracle: the
/// `-TagsFromFile all_five{,_MM}.jpg` runs write exactly these bytes.
#[test]
fn copy_writes_all_five_xp_strings_as_ucs2le_bytes_in_both_byte_orders() {
    let texts = ["Title", "Comment", "Author", "Key;Words", "Subject"];
    for order in [Order::Ii, Order::Mm] {
        let dir = tempfile::tempdir().unwrap();
        let entries: Vec<_> = XP_TAGS
            .iter()
            .zip(texts)
            .map(|(&(id, _), text)| {
                let raw = ucs2(text);
                (id, 1, raw.len() as u32, raw)
            })
            .collect();
        let src = write(dir.path(), "src.jpg", &jpeg_with(&tiff(order, &entries)));
        for dest in destinations(dir.path(), order) {
            copy_metadata(&src, &dest, None).unwrap();
            let got = xp_entries(&dest);
            for (&(id, name), text) in XP_TAGS.iter().zip(texts) {
                assert_eq!(
                    got.get(&id),
                    Some(&(1, ucs2(text))),
                    "{order:?} -> {} {name}",
                    dest.display()
                );
                assert_eq!(xp_text(&dest, name).as_deref(), Some(text), "{name}");
            }
        }
    }
}

/// (label, source byte order, source type, source count, source bytes,
/// expected bytes) of one copy case.
type CopyCase = (&'static str, Order, u16, u32, Vec<u8>, Vec<u8>);

/// What a copy writes for a source value that is not already canonical:
/// ExifTool's `ValueConvInv(ValueConv(raw))`, so the stored form is
/// re-derived from the decoded text. Each expectation is the oracle's
/// `-TagsFromFile` output for that source.
#[test]
fn copy_writes_exiftool_copy_bytes_for_every_source_shape() {
    let cases: [CopyCase; 13] = [
        // BMP, non-ASCII: e9 00 2d 4e 87 65 ...
        (
            "non_ascii",
            Order::Ii,
            1,
            18,
            ucs2("é中文 café"),
            hex("e9002d4e87652000630061006600e9000000"),
        ),
        // A surrogate pair round-trips as the pair.
        (
            "surrogate_pair",
            Order::Ii,
            1,
            8,
            hex("41003cd88cdf0000"),
            hex("41003cd88cdf0000"),
        ),
        // Lone surrogates survive too: ExifTool's UCS2 decode keeps each
        // unit as a code point, and its copy packs the same units back.
        (
            "lone_high_surrogate",
            Order::Ii,
            1,
            8,
            hex("410000d842000000"),
            hex("410000d842000000"),
        ),
        (
            "lone_low_surrogate",
            Order::Mm,
            1,
            8,
            hex("410000dc42000000"),
            hex("410000dc42000000"),
        ),
        // Two NULs: the empty value, written back as the NUL pair.
        ("empty", Order::Ii, 1, 2, hex("0000"), hex("0000")),
        // A leading U+0000 ends the value (FujiFilm Z100fd's XPTitle).
        (
            "leading_nul_fuji",
            Order::Ii,
            1,
            32,
            {
                let mut v = vec![0, 0];
                v.extend(" ".repeat(15).encode_utf16().flat_map(u16::to_le_bytes));
                v
            },
            hex("0000"),
        ),
        // Text after an embedded NUL is not part of the value.
        (
            "embedded_nul",
            Order::Ii,
            1,
            22,
            hex("4800650079000000680069006400640065006e000000"),
            hex("4800650079000000"),
        ),
        // A big-endian BOM switches the source's decode; the copy is LE.
        (
            "bom_be",
            Order::Ii,
            1,
            14,
            hex("feff0042004f004d006200650000"),
            hex("42004f004d00620065000000"),
        ),
        // An odd trailing byte is dropped.
        (
            "odd_count",
            Order::Ii,
            1,
            5,
            hex("4800690058"),
            hex("480069000000"),
        ),
        (
            "many_trailing_nuls",
            Order::Ii,
            1,
            26,
            {
                let mut v = ucs2("Hey");
                v.resize(26, 0);
                v
            },
            hex("4800650079000000"),
        ),
        // `Format => 'undef'` reads the int16u entry's bytes as they lie:
        // in an MM file that is 00 57 00 6f ..., U+5700 U+6F00 ...
        (
            "int16u_mm",
            Order::Mm,
            3,
            5,
            hex("0057006f007200640000"),
            hex("0057006f007200640000"),
        ),
        // An undef-typed source still copies as BYTE.
        ("undef_type", Order::Ii, 7, 12, ucs2("Hello"), ucs2("Hello")),
        // One byte holds no code unit: the empty value.
        ("one_byte", Order::Ii, 1, 1, b"A".to_vec(), hex("0000")),
    ];
    for (label, order, typ, count, raw, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        let src = write(
            dir.path(),
            "src.jpg",
            &jpeg_with(&tiff(order, &[(XP_TITLE, typ, count, raw)])),
        );
        let text = xp_text(&src, "XPTitle").expect("source value is read");
        for dest in destinations(dir.path(), order) {
            copy_metadata(&src, &dest, None).unwrap();
            assert_eq!(
                xp_entries(&dest).get(&XP_TITLE),
                Some(&(1, expected.clone())),
                "{label} -> {}",
                dest.display()
            );
            assert_eq!(xp_text(&dest, "XPTitle"), Some(text.clone()), "{label}");
        }
    }
}

/// Writing another tag into a PNG rebuilds its `eXIf` chunk from the map:
/// the XP strings survive as the bytes they were (canonical input), not as
/// the UTF-8 of their text. Oracle: `-EXIF:Artist=me` leaves them untouched.
#[test]
fn png_exif_rebuild_keeps_xp_strings_ucs2() {
    for order in [Order::Ii, Order::Mm] {
        let dir = tempfile::tempdir().unwrap();
        let entries: Vec<_> = XP_TAGS
            .iter()
            .map(|&(id, name)| {
                let raw = ucs2(name);
                (id, 1, raw.len() as u32, raw)
            })
            .collect();
        let png = write(
            dir.path(),
            "rebuild.png",
            &png_with(Some(&tiff(order, &entries))),
        );
        modify_tag(&png, "IFD0:Artist", TagValue::new_string("me")).unwrap();
        let got = xp_entries(&png);
        for &(id, name) in &XP_TAGS {
            assert_eq!(got.get(&id), Some(&(1, ucs2(name))), "{order:?} {name}");
            assert_eq!(xp_text(&png, name).as_deref(), Some(name));
        }
        assert_eq!(
            read_metadata(&png).unwrap().get_string("IFD0:Artist"),
            Some("me")
        );
    }
}

/// The PNG rebuild re-serializes an existing XP entry from the bytes the
/// file stored, not from its text: a surrogate pair and a lone surrogate
/// come back unit for unit, exactly the bytes the oracle's `-EXIF:Artist=me`
/// leaves in place (41 00 3c d8 8c df 00 00, 41 00 00 d8 42 00 00 00).
#[test]
fn png_exif_rebuild_keeps_stored_surrogates() {
    for raw in [hex("41003cd88cdf0000"), hex("410000d842000000")] {
        let dir = tempfile::tempdir().unwrap();
        let png = write(
            dir.path(),
            "rebuild.png",
            &png_with(Some(&tiff(
                Order::Ii,
                &[(XP_TITLE, 1, raw.len() as u32, raw.clone())],
            ))),
        );
        modify_tag(&png, "IFD0:Artist", TagValue::new_string("me")).unwrap();
        assert_eq!(xp_entries(&png).get(&XP_TITLE), Some(&(1, raw)));
    }
}

/// Every reader of the five tags keeps the entry's bytes as the stored
/// form (`TagOccurrence::stored`, "an `undef` run as its bytes") beside the
/// decoded text on the print and ValueConv channels: that is the provenance
/// a copy serializes from. A JPEG APP1, a standalone TIFF and a PNG `eXIf`
/// (the embedded-EXIF walk) each read one.
#[test]
fn readers_keep_the_stored_bytes_beside_the_text() {
    let raw = hex("41003cd88cdf0000");
    let block = tiff(Order::Mm, &[(XP_TITLE, 1, raw.len() as u32, raw.clone())]);
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("src.jpg", jpeg_with(&block)),
        ("src.tif", block.clone()),
        ("src.png", png_with(Some(&block))),
    ] {
        let path = write(dir.path(), name, &bytes);
        let metadata = read_metadata(&path).unwrap();
        let project = |channel| {
            metadata
                .project_occurrences(channel)
                .filter(|(key, _, _)| *key == "IFD0:XPTitle")
                .map(|(_, _, value)| value.into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            project(ValueChannel::Stored),
            vec![TagValue::Binary(raw.clone())],
            "{name}"
        );
        for channel in [ValueChannel::ValueConv, ValueChannel::PrintConv] {
            assert_eq!(
                project(channel),
                vec![TagValue::new_string("A🎌")],
                "{name} {channel:?}"
            );
        }
    }
}

/// `-IFD0:XPTitle=V` on a JPEG (no EXIF yet), a TIFF and a PNG. Oracle:
/// `Title` -> 54 00 69 00 74 00 6c 00 65 00 00 00, `é中文 café` -> e9 00 2d
/// 4e ..., `123` (an integer-looking value) -> 31 00 32 00 33 00 00 00, all
/// BYTE, all little-endian in an MM file too.
#[test]
fn direct_write_encodes_ucs2le_with_a_nul_pair() {
    let values = [
        (TagValue::new_string("Title"), ucs2("Title")),
        (TagValue::new_string("é中文 café"), ucs2("é中文 café")),
        (TagValue::Integer(123), ucs2("123")),
    ];
    for order in [Order::Ii, Order::Mm] {
        for (value, expected) in &values {
            let dir = tempfile::tempdir().unwrap();
            for dest in destinations(dir.path(), order) {
                modify_tag(&dest, "IFD0:XPTitle", value.clone()).unwrap();
                assert_eq!(
                    xp_entries(&dest).get(&XP_TITLE),
                    Some(&(1, expected.clone())),
                    "{order:?} {value:?} -> {}",
                    dest.display()
                );
            }
        }
    }
}

/// Rewriting an existing entry keeps `undef` (type 7) when it was `undef`
/// and otherwise writes `int8u` (WriteExif.pl 13.59:1225-1242). Oracle:
/// `-XPTitle=Hi` on an undef entry -> type 7, on an int16u, a string or an
/// MM int8u entry -> type 1; the bytes are 48 00 69 00 00 00 each time.
#[test]
fn rewriting_an_existing_xp_entry_keeps_undef_and_otherwise_writes_byte() {
    let cases = [
        (Order::Ii, 7u16, 12u32, ucs2("Hello"), 7u16),
        (Order::Ii, 3, 5, ucs2("Word"), 1),
        (Order::Ii, 2, 11, b"Ascii text\0".to_vec(), 1),
        (Order::Mm, 1, 12, ucs2("Hello"), 1),
    ];
    for (order, typ, count, raw, want_type) in cases {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "existing.jpg",
            &jpeg_with(&tiff(order, &[(XP_TITLE, typ, count, raw)])),
        );
        modify_tag(&path, "IFD0:XPTitle", TagValue::new_string("Hi")).unwrap();
        assert_eq!(
            xp_entries(&path).get(&XP_TITLE),
            Some(&(want_type, ucs2("Hi"))),
            "{order:?} type {typ}"
        );
    }
}

/// A typed code point above U+FFFF keeps only its low 16 bits, as ExifTool
/// 13.59 writes it: `Encode($val,"UCS2","II")` packs each code point with
/// `pack('v*')` (Charset.pm:387-390). Oracle `-XPTitle=V` bytes: `A🎌` ->
/// 41 00 8c f3 00 00; `x😀y中𝄞z` -> 78 00 00 f6 79 00 2d 4e 1e d1 7a 00 00 00;
/// U+10000 -> 00 00 00 00 (its low unit is U+0000); U+10FFFF -> ff ff 00 00;
/// U+FFFD (a replacement character typed as such) -> 41 00 fd ff 42 00 00 00.
/// Each reads back, in ExifTool and in oxidex, as the truncated text.
#[test]
fn direct_write_of_a_code_point_above_the_bmp_keeps_its_low_16_bits() {
    let cases = [
        ("A🎌", "41008cf30000", "A\u{f38c}"),
        (
            "x😀y中𝄞z",
            "780000f679002d4e1ed17a000000",
            "x\u{f600}y中\u{d11e}z",
        ),
        ("\u{10000}", "00000000", ""),
        ("\u{10ffff}", "ffff0000", "\u{ffff}"),
        ("A\u{fffd}B", "4100fdff42000000", "A\u{fffd}B"),
    ];
    for order in [Order::Ii, Order::Mm] {
        for (value, oracle, text) in cases {
            let dir = tempfile::tempdir().unwrap();
            for dest in destinations(dir.path(), order) {
                modify_tag(&dest, "IFD0:XPTitle", TagValue::new_string(value)).unwrap();
                assert_eq!(
                    xp_entries(&dest).get(&XP_TITLE),
                    Some(&(1, hex(oracle))),
                    "{order:?} {value:?} -> {}",
                    dest.display()
                );
                assert_eq!(
                    xp_text(&dest, "XPTitle").as_deref(),
                    Some(text),
                    "{value:?}"
                );
            }
        }
    }
}

/// The same round trips graded live against the pinned oracle: for each
/// value, the oracle writes a reference file (`-XPTitle=V` into the base
/// JPEG; `-TagsFromFile SRC` for a copy) and oxidex writes its own through
/// the same operation. The raw XP entries must be identical and the oracle
/// must read both files back as the same text.
#[test]
fn oracle_writes_the_same_xp_bytes() {
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
    let et_text = |path: &Path| et(&["-b".as_ref(), "-IFD0:XPTitle".as_ref(), path.as_os_str()]);

    // Direct writes.
    for value in [
        "Title",
        "é中文 café",
        "123",
        "Key;Words; more",
        "A🎌",
        "x😀y中𝄞z",
        "\u{10ffff}",
        "A\u{fffd}B",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let reference = write(dir.path(), "ref.jpg", &hex(BASE_JPEG_HEX));
        let arg = format!("-XPTitle={value}");
        et(&[
            "-q".as_ref(),
            "-overwrite_original".as_ref(),
            arg.as_ref(),
            reference.as_os_str(),
        ]);
        let ours = write(dir.path(), "ours.jpg", &hex(BASE_JPEG_HEX));
        modify_tag(&ours, "IFD0:XPTitle", TagValue::new_string(value)).unwrap();
        assert_eq!(
            xp_entries(&ours).get(&XP_TITLE),
            xp_entries(&reference).get(&XP_TITLE),
            "-XPTitle={value}"
        );
        let oracle_text = et_text(&reference);
        assert_eq!(et_text(&ours), oracle_text, "-XPTitle={value}");
        assert_eq!(
            xp_text(&ours, "XPTitle").map(String::into_bytes),
            Some(oracle_text),
            "-XPTitle={value}"
        );
    }

    // Copies, from both byte orders and every source shape above.
    let sources: [(Order, u16, Vec<u8>); 11] = [
        (Order::Ii, 1, ucs2("Hello")),
        (Order::Mm, 1, ucs2("Hello")),
        (Order::Ii, 1, ucs2("é中文 café")),
        (Order::Ii, 1, hex("41003cd88cdf0000")),
        (Order::Ii, 1, hex("410000d842000000")),
        (Order::Mm, 1, hex("410000dc42000000")),
        (Order::Ii, 1, hex("0000")),
        (Order::Ii, 1, hex("feff0042004f004d006200650000")),
        (
            Order::Ii,
            1,
            hex("4800650079000000680069006400640065006e000000"),
        ),
        (Order::Mm, 3, hex("0057006f007200640000")),
        (Order::Ii, 7, ucs2("Undef")),
    ];
    for (order, typ, raw) in sources {
        let dir = tempfile::tempdir().unwrap();
        let count = if typ == 3 { raw.len() / 2 } else { raw.len() } as u32;
        let src = write(
            dir.path(),
            "src.jpg",
            &jpeg_with(&tiff(order, &[(XP_TITLE, typ, count, raw.clone())])),
        );
        let reference = write(dir.path(), "ref.jpg", &hex(BASE_JPEG_HEX));
        et(&[
            "-q".as_ref(),
            "-overwrite_original".as_ref(),
            "-TagsFromFile".as_ref(),
            src.as_os_str(),
            reference.as_os_str(),
        ]);
        let ours = write(dir.path(), "ours.jpg", &hex(BASE_JPEG_HEX));
        copy_metadata(&src, &ours, None).unwrap();
        let raw_hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let label = format!("{order:?} type {typ} {raw_hex}");
        assert_eq!(
            xp_entries(&ours).get(&XP_TITLE),
            xp_entries(&reference).get(&XP_TITLE),
            "copy of {label}"
        );
        assert_eq!(et_text(&ours), et_text(&reference), "copy of {label}");
    }
}
