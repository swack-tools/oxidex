//! The PNG `eXIf` writer edits the EXIF block in place: entries the caller
//! did not change keep their type, count and value bytes, exactly as pinned
//! ExifTool 13.59 leaves them (`exiftool -IFD0:Artist=you file.png`).
//!
//! Before, any write rebuilt the whole chunk from the parsed map: an
//! `int16u` entry came back `int8u`, a rational was cut at its first NUL
//! byte, odd XP entries were re-encoded, the byte order became `II`, and the
//! MakerNote, IFD1 and unknown tags were lost. The expectations below are
//! what the oracle does with the same shapes of input, as measured by the
//! `png-exif-surgical` matrix harness named in the PR (its fixtures are a
//! superset of these), transcribed as data.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{
    clear_all_metadata, modify_tag, read_metadata, remove_tag, write_metadata,
};
use oxidex::core::tag_value::TagValue;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Order {
    Ii,
    Mm,
}

impl Order {
    fn u16(self, v: u16) -> [u8; 2] {
        match self {
            Order::Ii => v.to_le_bytes(),
            Order::Mm => v.to_be_bytes(),
        }
    }
    fn u32(self, v: u32) -> [u8; 4] {
        match self {
            Order::Ii => v.to_le_bytes(),
            Order::Mm => v.to_be_bytes(),
        }
    }
    fn read_u16(self, b: &[u8]) -> u16 {
        let a = [b[0], b[1]];
        match self {
            Order::Ii => u16::from_le_bytes(a),
            Order::Mm => u16::from_be_bytes(a),
        }
    }
    fn read_u32(self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        match self {
            Order::Ii => u32::from_le_bytes(a),
            Order::Mm => u32::from_be_bytes(a),
        }
    }
}

/// One IFD entry: tag, TIFF type, count, value bytes (already in order).
type Entry = (u16, u16, u32, Vec<u8>);

/// A dumped entry's TIFF type, count and value bytes.
type Field = (u16, u32, Vec<u8>);

/// An expected change: the entry key and its new field (`None`: deleted).
type Change<'a> = (&'a str, Option<(u16, u32, &'a [u8])>);

fn type_size(field_type: u16) -> usize {
    match field_type {
        3 | 8 => 2,
        4 | 9 | 11 | 13 => 4,
        5 | 10 | 12 => 8,
        _ => 1,
    }
}

/// A TIFF block: IFD0 (+ ExifIFD with Interop, GPS, IFD1 with a JPEG
/// thumbnail when given). Pointers are synthesized; the layout is plain
/// sequential, like a camera's.
struct Tiff {
    ifd0: Vec<Entry>,
    exif: Option<Vec<Entry>>,
    interop: Option<Vec<Entry>>,
    gps: Option<Vec<Entry>>,
    ifd1: Option<(Vec<Entry>, Vec<u8>)>,
}

impl Tiff {
    fn build(&self, order: Order) -> Vec<u8> {
        // (name, entries); pointer entries get placeholder values patched below
        let mut dirs: Vec<(&str, Vec<Entry>)> = vec![("IFD0", self.ifd0.clone())];
        if let Some(exif) = &self.exif {
            let mut exif = exif.clone();
            if self.interop.is_some() {
                exif.push((0xA005, 4, 1, vec![0; 4]));
            }
            dirs.push(("ExifIFD", exif));
            dirs[0].1.push((0x8769, 4, 1, vec![0; 4]));
        }
        if let Some(interop) = &self.interop {
            dirs.push(("InteropIFD", interop.clone()));
        }
        if let Some(gps) = &self.gps {
            dirs.push(("GPS", gps.clone()));
            dirs[0].1.push((0x8825, 4, 1, vec![0; 4]));
        }
        if let Some((ifd1, thumb)) = &self.ifd1 {
            let mut ifd1 = ifd1.clone();
            ifd1.push((0x0201, 4, 1, vec![0; 4]));
            ifd1.push((0x0202, 4, 1, order.u32(thumb.len() as u32).to_vec()));
            dirs.push(("IFD1", ifd1));
        }
        for (_, entries) in &mut dirs {
            entries.sort_by_key(|e| e.0);
        }
        let blob = |entries: &[Entry]| -> usize {
            entries
                .iter()
                .filter(|e| e.3.len() > 4)
                .map(|e| e.3.len() + (e.3.len() & 1))
                .sum()
        };
        let mut offsets = BTreeMap::new();
        let mut at = 8usize;
        for (name, entries) in &dirs {
            offsets.insert(*name, at);
            at += 2 + 12 * entries.len() + 4 + blob(entries);
        }
        let thumb_at = at;
        let mut out = match order {
            Order::Ii => b"II".to_vec(),
            Order::Mm => b"MM".to_vec(),
        };
        out.extend(order.u16(42));
        out.extend(order.u32(8));
        for (name, entries) in &dirs {
            let dir_at = offsets[name];
            assert_eq!(out.len(), dir_at);
            let mut data_at = dir_at + 2 + 12 * entries.len() + 4;
            let mut data = Vec::new();
            out.extend(order.u16(entries.len() as u16));
            for (tag, field_type, count, value) in entries {
                let value = match (*name, *tag) {
                    ("IFD0", 0x8769) => order.u32(offsets["ExifIFD"] as u32).to_vec(),
                    ("IFD0", 0x8825) => order.u32(offsets["GPS"] as u32).to_vec(),
                    ("ExifIFD", 0xA005) => order.u32(offsets["InteropIFD"] as u32).to_vec(),
                    ("IFD1", 0x0201) => order.u32(thumb_at as u32).to_vec(),
                    _ => value.clone(),
                };
                out.extend(order.u16(*tag));
                out.extend(order.u16(*field_type));
                out.extend(order.u32(*count));
                if value.len() <= 4 {
                    let mut inline = value.clone();
                    inline.resize(4, 0);
                    out.extend(inline);
                } else {
                    out.extend(order.u32(data_at as u32));
                    data.extend(&value);
                    if value.len() % 2 == 1 {
                        data.push(0);
                    }
                    data_at += value.len() + (value.len() & 1);
                }
            }
            let next = if *name == "IFD0" {
                offsets.get("IFD1").copied().unwrap_or(0)
            } else {
                0
            };
            out.extend(order.u32(next as u32));
            out.extend(data);
        }
        if let Some((_, thumb)) = &self.ifd1 {
            out.extend(thumb);
        }
        out
    }
}

/// Every entry of a TIFF block, keyed `IFD:0xTAG`, with pointers resolved
/// (their offsets are layout, not content) and the thumbnail's bytes in
/// place of its offset.
fn dump(tiff: &[u8]) -> BTreeMap<String, Field> {
    let order = match &tiff[..2] {
        b"II" => Order::Ii,
        b"MM" => Order::Mm,
        other => panic!("not a TIFF header: {other:?}"),
    };
    let mut out = BTreeMap::new();
    let mut todo = vec![("IFD0", order.read_u32(&tiff[4..8]) as usize)];
    while let Some((name, at)) = todo.pop() {
        let count = order.read_u16(&tiff[at..]) as usize;
        let (mut thumb_at, mut thumb_len) = (None, None);
        for i in 0..count {
            let p = at + 2 + 12 * i;
            let tag = order.read_u16(&tiff[p..]);
            let field_type = order.read_u16(&tiff[p + 2..]);
            let n = order.read_u32(&tiff[p + 4..]);
            let size = type_size(field_type) * n as usize;
            let value = if size <= 4 {
                tiff[p + 8..p + 8 + size].to_vec()
            } else {
                let off = order.read_u32(&tiff[p + 8..]) as usize;
                tiff[off..off + size].to_vec()
            };
            let target = order.read_u32(&tiff[p + 8..]) as usize;
            match (name, tag) {
                ("IFD0", 0x8769) => todo.push(("ExifIFD", target)),
                ("IFD0", 0x8825) => todo.push(("GPS", target)),
                ("ExifIFD", 0xA005) => todo.push(("InteropIFD", target)),
                ("IFD1", 0x0201) => thumb_at = Some(target),
                ("IFD1", 0x0202) => thumb_len = Some(target),
                _ => {
                    out.insert(format!("{name}:0x{tag:04x}"), (field_type, n, value));
                }
            }
        }
        if let (Some(t), Some(l)) = (thumb_at, thumb_len) {
            out.insert("IFD1:thumbnail".into(), (0, 0, tiff[t..t + l].to_vec()));
        }
        if name == "IFD0" {
            let next = order.read_u32(&tiff[at + 2 + 12 * count..]) as usize;
            if next != 0 {
                todo.push(("IFD1", next));
            }
        }
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(data);
    let mut crc_input = kind.to_vec();
    crc_input.extend(data);
    out.extend(crc32(&crc_input).to_be_bytes());
    out
}

const IHDR: [u8; 13] = [0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0];
const IDAT: [u8; 10] = [0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01];

/// A 1x1 greyscale PNG: `pre` chunks between IHDR and IDAT, `post` between
/// IDAT and IEND.
fn png(pre: &[(&[u8; 4], Vec<u8>)], post: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &IHDR));
    for (kind, data) in pre {
        out.extend(chunk(kind, data));
    }
    out.extend(chunk(b"IDAT", &IDAT));
    for (kind, data) in post {
        out.extend(chunk(kind, data));
    }
    out.extend(chunk(b"IEND", &[]));
    out
}

/// The chunks of a PNG, in order, each with its CRC checked.
fn chunks(png: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut at = 8;
    while at + 12 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
        let kind = &png[at + 4..at + 8];
        let data = &png[at + 8..at + 8 + len];
        let crc = u32::from_be_bytes(png[at + 8 + len..at + 12 + len].try_into().unwrap());
        let mut crc_input = kind.to_vec();
        crc_input.extend(data);
        assert_eq!(crc, crc32(&crc_input), "CRC of {:?}", kind);
        out.push((String::from_utf8_lossy(kind).into_owned(), data.to_vec()));
        at += 12 + len;
        if kind == b"IEND" {
            break;
        }
    }
    out
}

fn kinds(png: &[u8]) -> Vec<String> {
    chunks(png).into_iter().map(|(kind, _)| kind).collect()
}

fn exif_of(png: &[u8]) -> Option<Vec<u8>> {
    chunks(png)
        .into_iter()
        .find(|(kind, _)| kind == "eXIf")
        .map(|(_, data)| data)
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The matrix harness's "full" fixture: entries of every shape the old
/// rebuild damaged.
fn full(order: Order) -> Tiff {
    let h = |v: &[u16]| v.iter().flat_map(|x| order.u16(*x)).collect::<Vec<u8>>();
    let l = |v: &[u32]| v.iter().flat_map(|x| order.u32(*x)).collect::<Vec<u8>>();
    let r = |v: &[(u32, u32)]| {
        v.iter()
            .flat_map(|(a, b)| [order.u32(*a), order.u32(*b)].concat())
            .collect::<Vec<u8>>()
    };
    let sr = |a: i32, b: i32| [order.u32(a as u32), order.u32(b as u32)].concat();
    let maker = [
        &b"LSI1\0\x01\x02\x03"[..],
        &(0u8..40).collect::<Vec<_>>(),
        b"\0\0tail",
    ]
    .concat();
    let xp = |s: &str| {
        let mut v: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        v.extend([0, 0]);
        v
    };
    Tiff {
        ifd0: vec![
            (0x010E, 2, 12, b"desc\0hidden\0".to_vec()),
            (0x010F, 2, 5, b"Acme\0".to_vec()),
            (0x0110, 2, 4, b"M1\0\0".to_vec()),
            (0x0112, 3, 1, h(&[1])),
            (0x011A, 5, 1, r(&[(72, 1)])),
            (0x011B, 5, 1, r(&[(72, 1)])),
            (0x0128, 3, 1, h(&[2])),
            (0x0131, 2, 6, b"sw1.0\0".to_vec()),
            (0x013B, 2, 3, b"me\0".to_vec()),
            (0x0213, 3, 1, h(&[1])),
            (0x8298, 2, 10, b"copy\0edit\0".to_vec()),
            (0x9C9B, 1, 7, b"T\0i\0t\0l".to_vec()),
            (0x9C9C, 1, 8, b"\xff\xfeC\0m\0\0\0".to_vec()),
            (0x9C9E, 1, 8, vec![0x41, 0, 0, 0xd8, 0x42, 0, 0, 0]),
            (0x9C9F, 1, 10, xp("subj")),
            (0x4746, 3, 1, h(&[3])),
            (0xC000, 3, 3, h(&[1, 0, 2])),
            (0xC001, 4, 2, l(&[1, 0])),
            (0xC002, 1, 3, vec![1, 0, 2]),
        ],
        exif: Some(vec![
            (0x829A, 5, 1, r(&[(1, 250)])),
            (0x829D, 5, 1, r(&[(28, 10)])),
            (0x8827, 3, 1, h(&[100])),
            (0x9000, 7, 4, b"0232".to_vec()),
            (0x9003, 2, 20, b"2020:01:02 03:04:05\0".to_vec()),
            (0x9204, 10, 1, sr(-1, 3)),
            (0x927C, 7, maker.len() as u32, maker),
            (0x9286, 7, 16, b"ASCII\0\0\0orig\0\0\0\0".to_vec()),
            (0xA002, 4, 1, l(&[640])),
        ]),
        interop: Some(vec![(0x0001, 2, 4, b"R98\0".to_vec())]),
        gps: Some(vec![
            (0x0000, 1, 4, vec![2, 3, 0, 0]),
            (0x0001, 2, 2, b"N\0".to_vec()),
            (0x0002, 5, 3, r(&[(35, 1), (40, 1), (1234, 100)])),
            (0x0006, 5, 1, r(&[(100, 1)])),
        ]),
        ifd1: Some((
            vec![(0x0103, 3, 1, h(&[6])), (0x011A, 5, 1, r(&[(72, 1)]))],
            vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x04, 0x01, 0x02, 0xFF, 0xD9],
        )),
    }
}

/// Asserts `after` holds exactly `before`'s entries, except those `changed`
/// maps (to `Some(new)` or, for a deletion, `None`).
fn assert_only_changed(
    before: &BTreeMap<String, Field>,
    after: &BTreeMap<String, Field>,
    changed: &[Change<'_>],
) {
    let mut expected = before.clone();
    for (key, value) in changed {
        match value {
            Some((field_type, count, bytes)) => {
                expected.insert(key.to_string(), (*field_type, *count, bytes.to_vec()));
            }
            None => {
                expected.remove(*key);
            }
        }
    }
    for (key, value) in &expected {
        assert_eq!(after.get(key), Some(value), "entry {key}");
    }
    assert_eq!(
        after.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
}

/// `-IFD0:Artist=you`, `-ExifIFD:ISO=200` and `-IFD0:Artist=` each change
/// one entry; every other entry -- int16u, rationals with zero bytes, ASCII
/// with an embedded NUL, odd / BOM / lone-surrogate XP bytes, unknown tags,
/// the MakerNote blob, GPS, InteropIFD, IFD1 and its thumbnail -- keeps its
/// type, count and bytes, in the original byte order. Oracle: pinned
/// ExifTool 13.59 leaves the same entries (matrix cases
/// `{II,MM}_full_{artist,iso,del_artist}`).
#[test]
fn edits_change_only_the_named_entry() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = full(order).build(order);
        let before = dump(&tiff);
        let original = png(&[(b"eXIf", tiff.clone())], &[]);

        // generated scalar path
        let path = write(dir.path(), "artist.png", &original);
        modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap();
        let out = std::fs::read(&path).unwrap();
        assert_eq!(&exif_of(&out).unwrap()[..2], &tiff[..2], "{order:?}");
        assert_only_changed(
            &before,
            &dump(&exif_of(&out).unwrap()),
            &[("IFD0:0x013b", Some((2, 4, b"you\0")))],
        );
        assert_eq!(kinds(&out), ["IHDR", "eXIf", "IDAT", "IEND"]);

        // legacy surgical path
        let path = write(dir.path(), "iso.png", &original);
        modify_tag(&path, "ExifIFD:ISO", TagValue::new_integer(200)).unwrap();
        let out = std::fs::read(&path).unwrap();
        assert_only_changed(
            &before,
            &dump(&exif_of(&out).unwrap()),
            &[("ExifIFD:0x8827", Some((3, 1, &order.u16(200))))],
        );

        // deletion
        let path = write(dir.path(), "delete.png", &original);
        remove_tag(&path, "IFD0:Artist").unwrap();
        let out = std::fs::read(&path).unwrap();
        assert_only_changed(
            &before,
            &dump(&exif_of(&out).unwrap()),
            &[("IFD0:0x013b", None)],
        );
        assert!(
            read_metadata(&path)
                .unwrap()
                .get_string("IFD0:Artist")
                .is_none()
        );
    }
}

/// A write that names no EXIF tag carries the `eXIf` chunk byte-for-byte:
/// ExifTool rewrites that directory only when a tag in it is edited
/// (PNG.pm 13.59:1395-1399). Oracle: `-PNG:Author=x` leaves the chunk
/// identical (matrix cases `*_png_author`).
#[test]
fn a_non_exif_edit_carries_the_exif_chunk_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = full(order).build(order);
        let path = write(
            dir.path(),
            "author.png",
            &png(&[(b"eXIf", tiff.clone())], &[]),
        );
        modify_tag(&path, "PNG:Author", TagValue::new_string("x")).unwrap();
        let out = std::fs::read(&path).unwrap();
        assert_eq!(exif_of(&out).unwrap(), tiff, "{order:?}");
        assert_eq!(
            read_metadata(&path).unwrap().get_string("PNG:Author"),
            Some("x")
        );
    }
}

/// Oracle for a PNG with no EXIF: `-IFD0:Artist=you` creates a big-endian
/// block holding Artist and the mandatory YCbCrPositioning, in a new eXIf
/// chunk placed after the existing text chunk, immediately before IDAT
/// (`AddChunks(..., 'IFD0')`, PNG.pm 13.59:1538-1539).
#[test]
fn a_new_exif_chunk_goes_immediately_before_idat() {
    let dir = tempfile::tempdir().unwrap();
    let text = b"Comment\0hello".to_vec();
    let path = write(dir.path(), "new.png", &png(&[(b"tEXt", text.clone())], &[]));
    modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap();
    let out = std::fs::read(&path).unwrap();
    assert_eq!(kinds(&out), ["IHDR", "tEXt", "eXIf", "IDAT", "IEND"]);
    let tiff = exif_of(&out).unwrap();
    assert_eq!(&tiff[..4], b"MM\0*");
    let entries = dump(&tiff);
    assert_eq!(
        entries.into_iter().collect::<Vec<_>>(),
        vec![
            ("IFD0:0x013b".to_string(), (2, 4, b"you\0".to_vec())),
            ("IFD0:0x0213".to_string(), (3, 1, vec![0, 1])),
        ]
    );
    assert_eq!(chunks(&out)[1].1, text);
}

/// An eXIf chunk after IDAT moves to just before it, as ExifTool moves text
/// and EXIF chunks there when it rewrites a PNG (PNG.pm 13.59:1451-1465);
/// one between other ancillary chunks stays where it is.
#[test]
fn exif_chunk_placement_follows_exiftool() {
    let dir = tempfile::tempdir().unwrap();
    let order = Order::Mm;
    let tiff = Tiff {
        ifd0: vec![
            (0x010F, 2, 5, b"Acme\0".to_vec()),
            (0x013B, 2, 3, b"me\0".to_vec()),
        ],
        exif: None,
        interop: None,
        gps: None,
        ifd1: None,
    }
    .build(order);

    let path = write(
        dir.path(),
        "post.png",
        &png(&[], &[(b"eXIf", tiff.clone())]),
    );
    modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap();
    let out = std::fs::read(&path).unwrap();
    assert_eq!(kinds(&out), ["IHDR", "eXIf", "IDAT", "IEND"]);
    assert_eq!(
        dump(&exif_of(&out).unwrap()).get("IFD0:0x010f"),
        Some(&(2, 5, b"Acme\0".to_vec()))
    );

    let gama = 45455u32.to_be_bytes().to_vec();
    let path = write(
        dir.path(),
        "between.png",
        &png(
            &[
                (b"gAMA", gama.clone()),
                (b"eXIf", tiff.clone()),
                (b"tEXt", b"Comment\0hello".to_vec()),
            ],
            &[],
        ),
    );
    modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap();
    let out = std::fs::read(&path).unwrap();
    assert_eq!(
        kinds(&out),
        ["IHDR", "gAMA", "eXIf", "tEXt", "IDAT", "IEND"]
    );
    assert_eq!(chunks(&out)[1].1, gama);
}

/// EXIF the writer cannot edit surgically is refused and the file left
/// untouched. ExifTool 13.59 refuses both cases too (`[minor] IFD0 pointer
/// references previous IFD0 directory`): a second eXIf chunk, and a tag set
/// into EXIF held in a `Raw profile type exif` text chunk.
#[test]
fn uneditable_exif_carriers_are_refused_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let order = Order::Ii;
    let tiff = Tiff {
        ifd0: vec![(0x013B, 2, 3, b"me\0".to_vec())],
        exif: None,
        interop: None,
        gps: None,
        ifd1: None,
    }
    .build(order);

    let two = png(&[(b"eXIf", tiff.clone()), (b"eXIf", tiff.clone())], &[]);
    let path = write(dir.path(), "two.png", &two);
    let error = modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap_err();
    assert!(error.to_string().contains("more than one eXIf"), "{error}");
    assert_eq!(std::fs::read(&path).unwrap(), two);

    let mut hex = b"\nexif\n      20\n".to_vec();
    hex.extend(
        [b"Exif\0\0".as_slice(), &tiff[..14]]
            .concat()
            .iter()
            .flat_map(|b| format!("{b:02x}").into_bytes()),
    );
    let profile = png(
        &[(
            b"tEXt",
            [b"Raw profile type exif\0".as_slice(), &hex].concat(),
        )],
        &[],
    );
    let path = write(dir.path(), "profile.png", &profile);
    let error = modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap_err();
    assert!(
        error.to_string().contains("Raw profile type exif"),
        "{error}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), profile);

    // The keyword is resolved as ExifTool resolves it, through the `ucfirst`
    // fallback (PNG.pm 13.59:919-921): the oracle reads `IFD0:Artist` from
    // `raw profile type exif` and `raw profile type APP1` chunks (tEXt, zTXt
    // and iTXt alike) and refuses to set a tag with either present. A
    // spelling beyond `ucfirst` is not a profile: the oracle then creates an
    // eXIf chunk, and so does this writer.
    for (kind, data) in [
        (
            b"tEXt",
            [b"raw profile type exif\0".as_slice(), &hex].concat(),
        ),
        (
            b"iTXt",
            [b"raw profile type APP1\0\0\0\0\0".as_slice(), &hex].concat(),
        ),
    ] {
        let profile = png(&[(kind, data)], &[]);
        let path = write(dir.path(), "lower.png", &profile);
        let error = modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap_err();
        assert!(error.to_string().contains("raw profile type"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), profile);
    }
    let path = write(
        dir.path(),
        "not-a-profile.png",
        &png(
            &[(
                b"tEXt",
                [b"Raw Profile Type exif\0".as_slice(), &hex].concat(),
            )],
            &[],
        ),
    );
    modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap();
    assert_eq!(
        kinds(&std::fs::read(&path).unwrap()),
        ["IHDR", "tEXt", "eXIf", "IDAT", "IEND"]
    );
}

/// A minimal baseline JPEG with no metadata segment.
fn jpeg_without_exif() -> Vec<u8> {
    const BODY: &str = "ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";
    let mut out = vec![0xFF, 0xD8];
    out.extend(
        (0..BODY.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&BODY[i..i + 2], 16).unwrap()),
    );
    out
}

/// [`jpeg_without_exif`] with `tiff` as its EXIF APP1 block.
fn jpeg_with(tiff: &[u8]) -> Vec<u8> {
    let bare = jpeg_without_exif();
    let mut out = bare[..2].to_vec();
    out.extend([0xFF, 0xE1]);
    out.extend(((tiff.len() + 8) as u16).to_be_bytes());
    out.extend(b"Exif\0\0");
    out.extend(tiff);
    out.extend(&bare[2..]);
    out
}

/// A TIFF block whose IFD0 has no entries and whose IFD1 holds the only
/// ones (Compression 6, XResolution 72/1): the reader surfaces no EXIF row
/// for it in a PNG, while the oracle reads both IFD1 tags.
fn ifd1_only(order: Order) -> Vec<u8> {
    let r = [order.u32(72), order.u32(1)].concat();
    Tiff {
        ifd0: vec![],
        exif: None,
        interop: None,
        gps: None,
        ifd1: Some((
            vec![(0x0103, 3, 1, order.u16(6).to_vec()), (0x011A, 5, 1, r)],
            vec![0xFF, 0xD8, 0xFF, 0xD9],
        )),
    }
    .build(order)
}

/// Pinned ExifTool 13.59 adds a new IFD1 tag (`-IFD1:PanasonicTitle=x`,
/// `-IFD1:ImageDescription=x`, creating IFD1 when there is none), edits an
/// existing one (`-IFD1:Compression=1`) and adds an InteropIFD tag
/// (`-InteropIFD:RelatedImageWidth=5`). The surgical writers carry IFD1 and
/// InteropIFD raw and cannot, so the write is refused and the file left
/// untouched -- before, both the PNG and the JPEG writer reported success
/// and changed nothing. `-IFD1:XResolution=300`, which the generated path
/// owns, still writes.
#[test]
fn ifd1_and_interop_edits_are_refused_not_dropped() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = full(order).build(order);
        let small = Tiff {
            ifd0: vec![(0x013B, 2, 3, b"me\0".to_vec())],
            exif: None,
            interop: None,
            gps: None,
            ifd1: None,
        }
        .build(order);
        for (name, original) in [
            ("full.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("small.png", png(&[(b"eXIf", small.clone())], &[])),
            ("full.jpg", jpeg_with(&tiff)),
            ("small.jpg", jpeg_with(&small)),
            // An EXIF-family edit whose only rows are IFD1's: the planner's
            // "no EXIF key left" shortcut ran before the refusal, emptied
            // the plan and deleted the whole carrier (or created nothing).
            ("ifd1only.png", png(&[(b"eXIf", ifd1_only(order))], &[])),
            ("ifd1only.jpg", jpeg_with(&ifd1_only(order))),
            ("none.png", png(&[], &[])),
            ("none.jpg", jpeg_without_exif()),
        ] {
            for (key, value) in [
                ("IFD1:PanasonicTitle", TagValue::new_string("x")),
                ("IFD1:ImageDescription", TagValue::new_string("x")),
                ("IFD1:Compression", TagValue::new_integer(1)),
                ("InteropIFD:RelatedImageWidth", TagValue::new_integer(5)),
            ] {
                let path = write(dir.path(), name, &original);
                let result = modify_tag(&path, key, value);
                assert!(result.is_err(), "{order:?} {name} {key}: silent success");
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    original,
                    "{order:?} {name} {key}"
                );
            }
        }

        let path = write(
            dir.path(),
            "xres.png",
            &png(&[(b"eXIf", tiff.clone())], &[]),
        );
        modify_tag(
            &path,
            "IFD1:XResolution",
            TagValue::Rational {
                numerator: 300,
                denominator: 1,
            },
        )
        .unwrap();
        let out = std::fs::read(&path).unwrap();
        let expected = [order.u32(300), order.u32(1)].concat();
        let after = dump(&exif_of(&out).unwrap());
        assert_eq!(after.get("IFD1:0x011a").map(|f| &f.2), Some(&expected));
    }
}

/// ExifTool strips an improper `Exif\0\0` header from an eXIf chunk and
/// writes the edited block without it (PNG.pm 13.59:1367-1372), and copies
/// bytes after IEND (a trailer) through a rewrite.
#[test]
fn exif_header_is_dropped_on_edit_and_a_trailer_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let order = Order::Ii;
    let tiff = Tiff {
        ifd0: vec![
            (0x010F, 2, 5, b"Acme\0".to_vec()),
            (0x013B, 2, 3, b"me\0".to_vec()),
        ],
        exif: None,
        interop: None,
        gps: None,
        ifd1: None,
    }
    .build(order);
    let mut original = png(&[(b"eXIf", [b"Exif\0\0".as_slice(), &tiff].concat())], &[]);
    original.extend(b"TRAILER");
    let path = write(dir.path(), "header.png", &original);
    modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap();
    let out = std::fs::read(&path).unwrap();
    let exif = exif_of(&out).unwrap();
    assert_eq!(&exif[..2], b"II");
    assert_eq!(
        dump(&exif).get("IFD0:0x010f"),
        Some(&(2, 5, b"Acme\0".to_vec()))
    );
    assert!(out.ends_with(b"TRAILER"));
}

/// `-all=` (an empty replacement map, no named removals) drops the eXIf
/// chunk even when the reader surfaces no EXIF row from it: an IFD1-only
/// block, or one behind an improper `Exif\0\0` header. Oracle: pinned
/// ExifTool 13.59 `-all=` leaves `IHDR IDAT IEND` for both, as did the base
/// rebuild; the in-place writer carried the chunk and reported success.
#[test]
fn clear_all_drops_an_exif_chunk_the_reader_surfaces_nothing_from() {
    let dir = tempfile::tempdir().unwrap();
    let small = Tiff {
        ifd0: vec![(0x013B, 2, 3, b"me\0".to_vec())],
        exif: None,
        interop: None,
        gps: None,
        ifd1: None,
    }
    .build(Order::Ii);
    for (name, exif) in [
        ("ifd1only.png", ifd1_only(Order::Ii)),
        ("ifd1only-mm.png", ifd1_only(Order::Mm)),
        ("header.png", [b"Exif\0\0".as_slice(), &small].concat()),
        ("plain.png", small.clone()),
    ] {
        let path = write(dir.path(), name, &png(&[(b"eXIf", exif)], &[]));
        clear_all_metadata(&path).unwrap();
        assert_eq!(
            kinds(&std::fs::read(&path).unwrap()),
            ["IHDR", "IDAT", "IEND"],
            "{name}"
        );
    }
}

/// One `write_metadata` call that sets a generated-path tag (IFD0:Artist)
/// and deletes a legacy one (ExifIFD:ISO). The generated transaction applied
/// the legacy delta with the in-place TIFF payload writer, which cannot
/// shrink an IFD and refused the deletion; the deletion now goes through
/// the reconstructing surgical writer first. Oracle: `-IFD0:Artist=you
/// -ExifIFD:ISO=` sets Artist, deletes ISO and leaves every other entry.
#[test]
fn a_generated_set_and_a_legacy_deletion_in_one_write() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = full(order).build(order);
        let before = dump(&tiff);
        for (name, original) in [
            ("mixed.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("mixed.jpg", jpeg_with(&tiff)),
        ] {
            let path = write(dir.path(), name, &original);
            let mut map = read_metadata(&path).unwrap();
            map.insert("IFD0:Artist", TagValue::new_string("you"));
            assert!(map.remove("ExifIFD:ISO").is_some(), "{name}");
            write_metadata(&path, &map).unwrap_or_else(|e| panic!("{order:?} {name}: {e}"));
            let out = std::fs::read(&path).unwrap();
            let tiff_out = if name.ends_with(".png") {
                exif_of(&out).unwrap()
            } else {
                let at = out
                    .windows(6)
                    .position(|w| w == b"Exif\0\0")
                    .expect("EXIF APP1")
                    + 6;
                let len = u16::from_be_bytes([out[at - 8], out[at - 7]]) as usize - 8;
                out[at..at + len].to_vec()
            };
            assert_only_changed(
                &before,
                &dump(&tiff_out),
                &[
                    ("IFD0:0x013b", Some((2, 4, b"you\0"))),
                    ("ExifIFD:0x8827", None),
                ],
            );
        }
    }
}

/// One `write_metadata` call that deletes the file's last legacy-owned
/// EXIF value (ExifIFD:ISO) and sets a generated one (IFD0:Artist). The
/// reconstructing writer's "nothing left" result used to be refused; with an
/// IFD1 thumbnail present its drop-all shortcut would also have discarded
/// the directory. Oracle (pinned ExifTool 13.59, `-ExifIFD:ISO=
/// -IFD0:Artist=you`, PNG and JPEG alike): the original byte order is kept,
/// IFD0 holds Artist and nothing else (no mandatory YCbCrPositioning, as
/// IFD0 already existed), the emptied ExifIFD goes, and an IFD1 thumbnail or
/// a MakerNote beside ISO survives verbatim.
#[test]
fn deleting_the_last_legacy_tag_while_setting_a_generated_one() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let iso = (0x8827, 3, 1, order.u16(100).to_vec());
        let note = (0x927C, 7, 12, b"LSI1\0abcdefg".to_vec());
        let r = [order.u32(72), order.u32(1)].concat();
        let base = |exif: Vec<Entry>, ifd1: Option<(Vec<Entry>, Vec<u8>)>| {
            Tiff {
                ifd0: vec![],
                exif: Some(exif),
                interop: None,
                gps: None,
                ifd1,
            }
            .build(order)
        };
        for (label, tiff) in [
            ("iso", base(vec![iso.clone()], None)),
            (
                "iso+thumb",
                base(
                    vec![iso.clone()],
                    Some((
                        vec![
                            (0x0103, 3, 1, order.u16(6).to_vec()),
                            (0x011A, 5, 1, r.clone()),
                        ],
                        vec![0xFF, 0xD8, 0xFF, 0xD9],
                    )),
                ),
            ),
            ("iso+makernote", base(vec![iso.clone(), note.clone()], None)),
            // IFD1 holding only the JPEGInterchangeFormat/Length pair: the
            // scanner moves that pair into the plan's thumbnail, and the
            // serializer judged emptiness by the entry lists alone, so the
            // staged block came back empty and the thumbnail was dropped
            // (the "iso+thumb" fixture above has IFD1 entries besides it).
            (
                "iso+thumb-pointers-only",
                base(
                    vec![iso.clone()],
                    Some((vec![], vec![0xFF, 0xD8, 0xFF, 0xD9])),
                ),
            ),
        ] {
            let mut expected = dump(&tiff);
            expected.remove("ExifIFD:0x8827");
            expected.insert("IFD0:0x013b".into(), (2, 4, b"you\0".to_vec()));
            for (name, original) in [
                ("last.png", png(&[(b"eXIf", tiff.clone())], &[])),
                ("last.jpg", jpeg_with(&tiff)),
            ] {
                let path = write(dir.path(), name, &original);
                let mut map = read_metadata(&path).unwrap();
                map.insert("IFD0:Artist", TagValue::new_string("you"));
                assert!(map.remove("ExifIFD:ISO").is_some(), "{label} {name}");
                write_metadata(&path, &map)
                    .unwrap_or_else(|e| panic!("{order:?} {label} {name}: {e}"));
                let out = std::fs::read(&path).unwrap();
                let tiff_out = if name.ends_with(".png") {
                    exif_of(&out).unwrap()
                } else {
                    let at = out
                        .windows(6)
                        .position(|w| w == b"Exif\0\0")
                        .expect("EXIF APP1")
                        + 6;
                    let len = u16::from_be_bytes([out[at - 8], out[at - 7]]) as usize - 8;
                    out[at..at + len].to_vec()
                };
                assert_eq!(&tiff_out[..2], &tiff[..2], "{order:?} {label} {name}");
                assert_eq!(dump(&tiff_out), expected, "{order:?} {label} {name}");
            }
        }
    }
}

/// A named deletion of a raw-carried entry -- `-IFD1:Compression=`,
/// `-InteropIFD:InteropIndex=`, a `MakerNotes:` tag -- which the surgical
/// writers cannot perform. Pinned ExifTool 13.59 deletes the IFD1 and
/// Interop entries (and prunes an Interop directory left with nothing); the
/// PNG reader surfaces no IFD1 row, so the key arrived only as a named
/// removal and the PNG write reported success with the entry still there.
/// It is refused, exit 1 from the CLI, file untouched, for PNG and JPEG.
#[test]
fn named_removals_of_raw_carried_entries_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cli = env!("CARGO_BIN_EXE_oxidex");
    for order in [Order::Ii, Order::Mm] {
        let tiff = full(order).build(order);
        for (name, original) in [
            ("carried.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("carried.jpg", jpeg_with(&tiff)),
        ] {
            for key in [
                "IFD1:Compression",
                "InteropIFD:InteropIndex",
                "MakerNotes:MakerNoteUnknownBinary",
            ] {
                let path = write(dir.path(), name, &original);
                let result = remove_tag(&path, key);
                assert!(result.is_err(), "{order:?} {name} {key}: silent success");
                assert_eq!(std::fs::read(&path).unwrap(), original, "{name} {key}");

                let status = std::process::Command::new(cli)
                    .arg(format!("-{key}="))
                    .arg(&path)
                    .output()
                    .unwrap();
                assert_eq!(status.status.code(), Some(1), "{order:?} {name} -{key}=");
                assert_eq!(std::fs::read(&path).unwrap(), original, "{name} -{key}=");
            }
        }
    }
}

/// A block whose IFD0 has a `SubIFDs` pointer (0x014a): IFD0 {Make, Artist,
/// SubIFDs, ExifOffset}, ExifIFD {ISO}, and a SubIFD {NewSubfileType 1,
/// ImageWidth 160, ImageDescription "sub-ifd-desc"} after them.
fn subifd_block(order: Order) -> Vec<u8> {
    let entry = |tag: u16, ty: u16, count: u32, value: [u8; 4]| {
        [
            order.u16(tag).as_slice(),
            &order.u16(ty),
            &order.u32(count),
            &value,
        ]
        .concat()
    };
    let pad2 = |v: u16| {
        let b = order.u16(v);
        [b[0], b[1], 0, 0]
    };
    let (ifd0, make_at, exif_at, sub_at, desc_at) = (8u32, 62u32, 68u32, 86u32, 128u32);
    let mut t = match order {
        Order::Ii => b"II".to_vec(),
        Order::Mm => b"MM".to_vec(),
    };
    t.extend(order.u16(42));
    t.extend(order.u32(ifd0));
    t.extend(order.u16(4));
    t.extend(entry(0x010F, 2, 6, order.u32(make_at)));
    t.extend(entry(0x013B, 2, 3, *b"me\0\0"));
    t.extend(entry(0x014A, 4, 1, order.u32(sub_at)));
    t.extend(entry(0x8769, 4, 1, order.u32(exif_at)));
    t.extend(order.u32(0));
    t.extend(b"Acme\0\0");
    t.extend(order.u16(1));
    t.extend(entry(0x8827, 3, 1, pad2(100)));
    t.extend(order.u32(0));
    t.extend(order.u16(3));
    t.extend(entry(0x00FE, 4, 1, order.u32(1)));
    t.extend(entry(0x0100, 3, 1, pad2(160)));
    t.extend(entry(0x010E, 2, 14, order.u32(desc_at)));
    t.extend(order.u32(0));
    assert_eq!(t.len(), desc_at as usize);
    t.extend(b"sub-ifd-desc\0\0");
    t
}

/// The entries of the IFD that IFD0's SubIFDs pointer leads to, raw, or
/// `None` when the pointer is gone or leads outside the block.
fn subifd_entries(tiff: &[u8]) -> Option<Vec<Entry>> {
    let order = if &tiff[..2] == b"II" {
        Order::Ii
    } else {
        Order::Mm
    };
    let ifd0 = order.read_u32(&tiff[4..8]) as usize;
    let n = order.read_u16(&tiff[ifd0..]) as usize;
    let at = (0..n)
        .map(|i| ifd0 + 2 + 12 * i)
        .find(|&p| order.read_u16(&tiff[p..]) == 0x014A)?;
    let sub = order.read_u32(&tiff[at + 8..]) as usize;
    if sub + 2 > tiff.len() {
        return None;
    }
    let m = order.read_u16(&tiff[sub..]) as usize;
    let mut out = Vec::new();
    for i in 0..m {
        let p = sub + 2 + 12 * i;
        if p + 12 > tiff.len() {
            return None;
        }
        let (tag, ty, count) = (
            order.read_u16(&tiff[p..]),
            order.read_u16(&tiff[p + 2..]),
            order.read_u32(&tiff[p + 4..]),
        );
        let size = type_size(ty) * count as usize;
        let value = if size <= 4 {
            tiff[p + 8..p + 8 + size].to_vec()
        } else {
            let off = order.read_u32(&tiff[p + 8..]) as usize;
            tiff.get(off..off + size)?.to_vec()
        };
        out.push((tag, ty, count, value));
    }
    Some(out)
}

fn tiff_of(name: &str, file: &[u8]) -> Vec<u8> {
    if name.ends_with(".png") {
        exif_of(file).unwrap()
    } else {
        let at = file
            .windows(6)
            .position(|w| w == b"Exif\0\0")
            .expect("EXIF APP1")
            + 6;
        let len = u16::from_be_bytes([file[at - 8], file[at - 7]]) as usize - 8;
        file[at..at + len].to_vec()
    }
}

/// The reconstructing (re-laid-out) EXIF writer models IFD0, ExifIFD, GPS,
/// InteropIFD, IFD1 and the MakerNote; a pointer it does not model --
/// IFD0's SubIFDs here -- was written back holding its old offset while the
/// IFD it pointed to was not copied: a dangling pointer, reported as
/// success (JPEG at base cb1f5def and tip too; PNG from this branch). Pinned
/// ExifTool 13.59 keeps the SubIFD intact for each of these writes. The
/// reconstructing paths -- a legacy set, a legacy deletion, and a mixed
/// generated set + legacy deletion -- now refuse such a block, file
/// untouched; the in-place generated path, which never moves existing
/// bytes, still writes and leaves the SubIFD reachable and unchanged.
#[test]
fn a_block_with_an_unmodelled_pointer_is_never_re_laid_out() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = subifd_block(order);
        let sub = subifd_entries(&tiff).unwrap();
        assert_eq!(sub.len(), 3);
        for (name, original) in [
            ("sub.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("sub.jpg", jpeg_with(&tiff)),
        ] {
            let path = write(dir.path(), name, &original);
            let error = modify_tag(&path, "ExifIFD:ISO", TagValue::new_integer(200)).unwrap_err();
            assert!(
                error.to_string().contains("0x014A"),
                "{order:?} {name}: {error}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), original, "{name} set");

            assert!(
                remove_tag(&path, "ExifIFD:ISO").is_err(),
                "{order:?} {name} delete"
            );
            assert_eq!(std::fs::read(&path).unwrap(), original, "{name} delete");

            let mut map = read_metadata(&path).unwrap();
            map.insert("IFD0:Artist", TagValue::new_string("you"));
            map.remove("ExifIFD:ISO");
            assert!(
                write_metadata(&path, &map).is_err(),
                "{order:?} {name} mixed"
            );
            assert_eq!(std::fs::read(&path).unwrap(), original, "{name} mixed");

            modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap();
            let out = tiff_of(name, &std::fs::read(&path).unwrap());
            assert_eq!(
                subifd_entries(&out).as_ref(),
                Some(&sub),
                "{order:?} {name} generated"
            );
            assert_eq!(
                dump(&out).get("IFD0:0x013b"),
                Some(&(2, 4, b"you\0".to_vec()))
            );
        }
    }
}

/// A named removal of an IFD1/InteropIFD/MakerNotes tag the block does not
/// hold -- an unregistered name, or a registered one that is absent -- is a
/// no-op success, as the `remove_tag` API promises and as oxidex treats an
/// undefined name in any group. (Pinned ExifTool 13.59 prints "Tag ... is
/// not defined" / "Nothing to do.", or "1 image files unchanged" for the
/// absent registered name, and changes nothing.) A missing tag id is no
/// wildcard: only a removal naming an entry actually present is refused.
#[test]
fn named_removals_of_absent_or_unmapped_carried_tags_are_no_ops() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = full(order).build(order);
        let before = dump(&tiff);
        for (name, original) in [
            ("absent.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("absent.jpg", jpeg_with(&tiff)),
        ] {
            for key in [
                "IFD1:BogusTag",
                "IFD1:ImageDescription",
                "InteropIFD:BogusTag",
                "InteropIFD:RelatedImageWidth",
                "MakerNotes:BogusTag",
            ] {
                let path = write(dir.path(), name, &original);
                remove_tag(&path, key).unwrap_or_else(|e| panic!("{order:?} {name} {key}: {e}"));
                let out = std::fs::read(&path).unwrap();
                let tiff_out = if name.ends_with(".png") {
                    exif_of(&out).unwrap()
                } else {
                    let at = out.windows(6).position(|w| w == b"Exif\0\0").unwrap() + 6;
                    let len = u16::from_be_bytes([out[at - 8], out[at - 7]]) as usize - 8;
                    out[at..at + len].to_vec()
                };
                assert_eq!(dump(&tiff_out), before, "{order:?} {name} {key}");
            }
        }
    }
}

/// One `write_metadata` call that sets a generated tag (IFD0:Artist) and
/// drops a surfaced row of a raw-carried directory from the map
/// (InteropIFD:InteropIndex; for JPEG, whose reader surfaces IFD1, also
/// IFD1:Compression). The mixed transaction's staging predicate knew only
/// IFD0/ExifIFD/GPS/EXIF rows, so the legacy delta went to the in-place
/// payload writer, which does not walk those directories: Artist was set,
/// the deletion silently kept, success reported. Pinned ExifTool 13.59
/// performs both; this writer cannot delete from those directories, so the
/// write is refused with the file untouched.
#[test]
fn a_mixed_write_dropping_a_carried_row_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = full(order).build(order);
        for (name, original, keys) in [
            (
                "mixed-carried.png",
                png(&[(b"eXIf", tiff.clone())], &[]),
                &["InteropIFD:InteropIndex"][..],
            ),
            (
                "mixed-carried.jpg",
                jpeg_with(&tiff),
                &["InteropIFD:InteropIndex", "IFD1:Compression"][..],
            ),
        ] {
            for key in keys {
                let path = write(dir.path(), name, &original);
                let mut map = read_metadata(&path).unwrap();
                assert!(map.remove(key).is_some(), "{name}: reader surfaces {key}");
                map.insert("IFD0:Artist", TagValue::new_string("you"));
                assert!(
                    write_metadata(&path, &map).is_err(),
                    "{order:?} {name} {key}: silent success"
                );
                assert_eq!(std::fs::read(&path).unwrap(), original, "{name} {key}");
            }
        }
    }
}

/// `remove_tag("EXIF:InteropIndex")` names the InteropIFD entry by its
/// family-0 alias (the planner already maps `-EXIF:InteropIndex=R03` to
/// InteropIFD when setting). The raw-carried removal check accepted only the
/// `InteropIFD:` spelling, so with IFD0 rows present the entry was carried
/// and success reported. Pinned ExifTool 13.59 deletes it; this writer
/// cannot, so it refuses, file untouched, for PNG and JPEG and the CLI.
#[test]
fn a_family_alias_removal_of_a_carried_entry_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cli = env!("CARGO_BIN_EXE_oxidex");
    for order in [Order::Ii, Order::Mm] {
        let tiff = full(order).build(order);
        for (name, original) in [
            ("alias.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("alias.jpg", jpeg_with(&tiff)),
        ] {
            let path = write(dir.path(), name, &original);
            assert!(
                remove_tag(&path, "EXIF:InteropIndex").is_err(),
                "{order:?} {name}: silent success"
            );
            assert_eq!(std::fs::read(&path).unwrap(), original, "{name}");
            let out = std::process::Command::new(cli)
                .arg("-EXIF:InteropIndex=")
                .arg(&path)
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(1), "{order:?} {name} CLI");
            assert_eq!(std::fs::read(&path).unwrap(), original, "{name} CLI");
        }
    }
}

/// A TIFF-structured file is verified with the header set its writer walks:
/// magic 42, and 85 for Panasonic's RW2/RWL (`tiff_surgical::
/// is_walkable_tiff`). The post-write check scanned with the EXIF-block
/// scanner, which accepts 42 only, so every RW2 edit failed "Invalid TIFF
/// magic number in EXIF data" while tip 707c7565 wrote it. Pinned ExifTool
/// 13.59 writes `-IFD0:Artist=x` to t/images/Panasonic.rw2 and reads it
/// back; so does oxidex.
#[test]
fn an_rw2_edit_passes_the_post_write_check() {
    let dir = tempfile::tempdir().unwrap();
    // Synthetic RW2 header (`IIU\0`, magic 85, little-endian as Panasonic
    // writes it): IFD0 {Make "Panasonic", Artist "me"}.
    for order in [Order::Ii] {
        let mut t = match order {
            Order::Ii => b"II".to_vec(),
            Order::Mm => b"MM".to_vec(),
        };
        t.extend(order.u16(85));
        t.extend(order.u32(8));
        t.extend(order.u16(2));
        for (tag, count, value) in [(0x010Fu16, 10u32, order.u32(38)), (0x013B, 3, *b"me\0\0")] {
            t.extend(order.u16(tag));
            t.extend(order.u16(2));
            t.extend(order.u32(count));
            t.extend(value);
        }
        t.extend(order.u32(0));
        t.extend(b"Panasonic\0");
        let path = write(dir.path(), "synthetic.rw2", &t);
        modify_tag(&path, "IFD0:Artist", TagValue::new_string("x"))
            .unwrap_or_else(|e| panic!("{order:?} synthetic RW2: {e}"));
        assert_eq!(
            read_metadata(&path).unwrap().get_string("IFD0:Artist"),
            Some("x"),
            "{order:?}"
        );
    }
    let Some(sample) = fixtures::pinned_t_images_fixture_path("Panasonic.rw2") else {
        return;
    };
    let path = dir.path().join("Panasonic.rw2");
    std::fs::copy(&sample, &path).unwrap();
    modify_tag(&path, "IFD0:Artist", TagValue::new_string("x"))
        .unwrap_or_else(|e| panic!("t/images/Panasonic.rw2: {e}"));
    assert_eq!(
        read_metadata(&path).unwrap().get_string("IFD0:Artist"),
        Some("x")
    );
}

/// `remove_tag` of a tag the block does not hold is a no-op success, and it
/// must stay one on a block the reconstructing writer refuses to re-lay out
/// (an unmodelled SubIFDs pointer): the structural refusal applied before
/// the writer noticed there was nothing to do. The payload is now returned
/// unchanged, so the file's EXIF -- SubIFD included -- is untouched.
#[test]
fn a_no_op_removal_on_a_block_with_an_unmodelled_pointer_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = subifd_block(order);
        for (name, original) in [
            ("noop.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("noop.jpg", jpeg_with(&tiff)),
        ] {
            for key in ["ExifIFD:ExposureTime", "GPS:GPSAltitude", "IFD0:Software"] {
                let path = write(dir.path(), name, &original);
                remove_tag(&path, key).unwrap_or_else(|e| panic!("{order:?} {name} {key}: {e}"));
                let out = tiff_of(name, &std::fs::read(&path).unwrap());
                assert_eq!(out, tiff, "{order:?} {name} {key}: EXIF unchanged");
            }
        }
    }
}

/// Malformed EXIF payloads: shorter than a TIFF header, a bad byte-order
/// mark, a bad magic number.
fn malformed_exif_payloads() -> [(&'static str, Vec<u8>); 3] {
    [
        ("short", b"II*".to_vec()),
        ("byte-order", b"XX*\0\x08\0\0\0\0\0\0\0\0\0".to_vec()),
        ("magic", b"II\x2b\0\x08\0\0\0\0\0\0\0\0\0".to_vec()),
    ]
}

/// The clear-all contract never parses what it discards: `-all=` on a JPEG
/// whose `Exif\0\0` APP1 is malformed drops the segment, as tip 707c7565 and
/// pinned ExifTool 13.59 do. The post-write check scanned the discarded
/// payload and refused the clear. A PNG eXIf clear likewise.
#[test]
fn clear_all_drops_a_malformed_exif_carrier() {
    let dir = tempfile::tempdir().unwrap();
    for (label, payload) in malformed_exif_payloads() {
        let path = write(dir.path(), "bad.jpg", &jpeg_with(&payload));
        clear_all_metadata(&path).unwrap_or_else(|e| panic!("{label} jpg: {e}"));
        let out = std::fs::read(&path).unwrap();
        assert!(
            !out.windows(6).any(|w| w == b"Exif\0\0"),
            "{label}: EXIF APP1 left"
        );

        let path = write(
            dir.path(),
            "bad.png",
            &png(&[(b"eXIf", payload.clone())], &[]),
        );
        clear_all_metadata(&path).unwrap_or_else(|e| panic!("{label} png: {e}"));
        assert_eq!(
            kinds(&std::fs::read(&path).unwrap()),
            ["IHDR", "IDAT", "IEND"]
        );
    }
}

/// A PNG `Raw profile type exif` text chunk, hex-encoding `Exif\0\0` + `tiff`.
fn raw_exif_profile(kind: &[u8; 4], tiff: &[u8]) -> ([u8; 4], Vec<u8>) {
    let body = [b"Exif\0\0".as_slice(), tiff].concat();
    let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
    let text = format!("\nexif\n{:8}\n{hex}\n", body.len());
    let data = if kind == b"zTXt" {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(text.as_bytes()).unwrap();
        [
            b"Raw profile type exif\0\0".as_slice(),
            &z.finish().unwrap(),
        ]
        .concat()
    } else {
        [b"Raw profile type exif\0".as_slice(), text.as_bytes()].concat()
    };
    (*kind, data)
}

/// Whether a request is a no-op is decided once, before any refusal: a
/// removal that names nothing in any EXIF carrier -- an unmapped name, a
/// registered tag the carrier does not hold, or anything in a carrier no
/// reader can parse -- succeeds with the file untouched. Before, such a
/// removal on a PNG with a raw EXIF profile hit the raw-profile refusal, and
/// on a malformed eXIf/APP1 the scan error. A removal of a tag the raw
/// profile does hold is still refused (this writer cannot edit a profile;
/// pinned ExifTool 13.59 deletes it).
#[test]
fn absent_removals_are_no_ops_before_any_carrier_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let tiff = Tiff {
        ifd0: vec![
            (0x010F, 2, 5, b"Acme\0".to_vec()),
            (0x013B, 2, 3, b"me\0".to_vec()),
        ],
        exif: None,
        interop: None,
        gps: None,
        ifd1: None,
    }
    .build(Order::Ii);
    for kind in [b"zTXt", b"tEXt"] {
        let (k, data) = raw_exif_profile(kind, &tiff);
        let original = png(&[(&k, data)], &[]);
        for key in ["EXIF:BogusTag", "IFD0:Software"] {
            let path = write(dir.path(), "profile.png", &original);
            remove_tag(&path, key).unwrap_or_else(|e| panic!("{kind:?} {key}: {e}"));
            assert_eq!(std::fs::read(&path).unwrap(), original, "{kind:?} {key}");
        }
        let path = write(dir.path(), "profile.png", &original);
        assert!(remove_tag(&path, "IFD0:Artist").is_err(), "{kind:?} Artist");
        assert_eq!(std::fs::read(&path).unwrap(), original, "{kind:?} Artist");
    }
    for (label, payload) in malformed_exif_payloads() {
        for (name, original) in [
            ("bad.png", png(&[(b"eXIf", payload.clone())], &[])),
            ("bad.jpg", jpeg_with(&payload)),
        ] {
            for key in ["EXIF:BogusTag", "IFD0:Artist"] {
                let path = write(dir.path(), name, &original);
                // The one exception: an empty JPEG block whose only fault is
                // its magic number, which pinned ExifTool 13.59 reads and, on
                // a removal of a real tag, drops; this writer cannot read it,
                // so it refuses (see
                // `an_empty_exif_app1_is_dropped_by_an_exif_removal`).
                if (label, name, key) == ("magic", "bad.jpg", "IFD0:Artist") {
                    assert!(remove_tag(&path, key).is_err(), "{label} {name} {key}");
                } else {
                    remove_tag(&path, key).unwrap_or_else(|e| panic!("{label} {name} {key}: {e}"));
                }
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    original,
                    "{label} {name} {key}"
                );
            }
        }
    }
}

/// A legacy-only deletion of the block's last ordinary value
/// (`-ExifIFD:ISO=`) beside an IFD1 holding only a thumbnail. Pinned
/// ExifTool 13.59 keeps the thumbnail: IFD0 empty, IFD1 = the pointer pair,
/// thumbnail bytes unchanged. The reconstructing writer's "no EXIF row left:
/// drop everything" shortcut took a named removal for a clear and dropped
/// the block, thumbnail included (tip 707c7565 too); it now applies only to
/// a map with no EXIF row and no named removal.
#[test]
fn deleting_the_last_row_keeps_a_thumbnail_only_ifd1() {
    let dir = tempfile::tempdir().unwrap();
    let thumb = vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x03, 0x01, 0xFF, 0xD9];
    for order in [Order::Ii, Order::Mm] {
        let tiff = Tiff {
            ifd0: vec![],
            exif: Some(vec![(0x8827, 3, 1, order.u16(100).to_vec())]),
            interop: None,
            gps: None,
            ifd1: Some((vec![], thumb.clone())),
        }
        .build(order);
        let mut expected = dump(&tiff);
        expected.remove("ExifIFD:0x8827");
        for (name, original) in [
            ("thumb.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("thumb.jpg", jpeg_with(&tiff)),
        ] {
            let path = write(dir.path(), name, &original);
            remove_tag(&path, "ExifIFD:ISO").unwrap_or_else(|e| panic!("{order:?} {name}: {e}"));
            let out = tiff_of(name, &std::fs::read(&path).unwrap());
            assert_eq!(&out[..2], &tiff[..2], "{order:?} {name}");
            assert_eq!(dump(&out), expected, "{order:?} {name}");
            assert_eq!(
                dump(&out).get("IFD1:thumbnail").map(|t| t.2.clone()),
                Some(thumb.clone())
            );
        }
    }
}

/// Mandatory IFD0 seeding follows `WriteExif`'s `$numEntries` of the IFD0
/// being written (WriteExif.pl 13.59:714-719), per write. On the pointer-only
/// isothumb block (IFD0 = {ExifOffset}, ExifIFD = {ISO}, IFD1 = thumbnail
/// pair), pinned ExifTool 13.59:
///
/// - one pass, `-ExifIFD:ISO= -IFD0:Artist=you`: IFD0 = {Artist} (IFD0 had
///   an entry, so nothing is seeded) -- one `write_metadata` here;
/// - two passes, `-ExifIFD:ISO=` then `-IFD0:Artist=you`: IFD0 = {Artist,
///   YCbCrPositioning} (the second pass finds IFD0 with no entries) -- the
///   same two writes here, which is also what the oxidex CLI does with both
///   flags, as it applies each `-TAG=` as its own write;
/// - no EXIF at all, `-IFD0:Artist=you`: MM, IFD0 = {Artist,
///   YCbCrPositioning} (see `a_new_exif_chunk_goes_immediately_before_idat`).
#[test]
fn mandatory_ifd0_seeding_follows_the_ifd0_each_write_sees() {
    let dir = tempfile::tempdir().unwrap();
    let thumb = vec![0xFF, 0xD8, 0xFF, 0xD9];
    for order in [Order::Ii, Order::Mm] {
        let tiff = Tiff {
            ifd0: vec![],
            exif: Some(vec![(0x8827, 3, 1, order.u16(100).to_vec())]),
            interop: None,
            gps: None,
            ifd1: Some((vec![], thumb.clone())),
        }
        .build(order);
        let ifd0 = |tiff: &[u8]| -> Vec<(String, Field)> {
            dump(tiff)
                .into_iter()
                .filter(|(key, _)| key.starts_with("IFD0:"))
                .collect()
        };
        let artist = ("IFD0:0x013b".to_string(), (2, 4, b"you\0".to_vec()));
        let ycbcr = ("IFD0:0x0213".to_string(), (3, 1, order.u16(1).to_vec()));
        for (name, original) in [
            ("seed.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("seed.jpg", jpeg_with(&tiff)),
        ] {
            // one pass
            let path = write(dir.path(), name, &original);
            let mut map = read_metadata(&path).unwrap();
            map.remove("ExifIFD:ISO");
            map.insert("IFD0:Artist", TagValue::new_string("you"));
            write_metadata(&path, &map).unwrap();
            let out = tiff_of(name, &std::fs::read(&path).unwrap());
            assert_eq!(
                ifd0(&out),
                vec![artist.clone()],
                "{order:?} {name} one pass"
            );

            // two passes
            let path = write(dir.path(), name, &original);
            remove_tag(&path, "ExifIFD:ISO").unwrap();
            modify_tag(&path, "IFD0:Artist", TagValue::new_string("you")).unwrap();
            let out = tiff_of(name, &std::fs::read(&path).unwrap());
            assert_eq!(
                ifd0(&out),
                vec![artist.clone(), ycbcr.clone()],
                "{order:?} {name} two passes"
            );
            assert_eq!(
                dump(&out).get("IFD1:thumbnail").map(|t| t.2.clone()),
                Some(thumb.clone())
            );
        }
    }
}

/// What pinned ExifTool 13.59 leaves of a block after `-<group>:All=`
/// (measured on the "full" fixture, both byte orders, PNG eXIf and JPEG
/// APP1 alike): `IFD0:All` and `EXIF:All` remove the whole EXIF block;
/// `ExifIFD:All` removes ExifIFD with its InteropIFD and MakerNote;
/// `GPS:All`, `IFD1:All` (with the thumbnail) and `InteropIFD:All` remove
/// that directory; `MakerNotes:All` leaves a SilverFast/unknown-text/
/// Samsung1a note (ExifTool files those under EXIF, not MakerNotes) and
/// deletes any other MakerNote. `None` = no EXIF left.
fn after_group_removal(
    group: &str,
    before: &BTreeMap<String, Field>,
    note_is_fallback: bool,
) -> Option<BTreeMap<String, Field>> {
    let keep = |prefixes: &[&str]| {
        before
            .iter()
            .filter(|(k, _)| !prefixes.iter().any(|p| k.starts_with(p)))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    match group {
        "IFD0:All" | "EXIF:All" => None,
        "ExifIFD:All" => Some(keep(&["ExifIFD:", "InteropIFD:"])),
        "GPS:All" => Some(keep(&["GPS:"])),
        "IFD1:All" => Some(keep(&["IFD1:"])),
        "InteropIFD:All" => Some(keep(&["InteropIFD:"])),
        "MakerNotes:All" if note_is_fallback => Some(before.clone()),
        "MakerNotes:All" => Some(keep(&["ExifIFD:0x927c"])),
        _ => unreachable!(),
    }
}

const GROUP_REMOVALS: [&str; 7] = [
    "IFD0:All",
    "ExifIFD:All",
    "GPS:All",
    "IFD1:All",
    "InteropIFD:All",
    "MakerNotes:All",
    "EXIF:All",
];

fn exif_payload(name: &str, file: &[u8]) -> Option<Vec<u8>> {
    if name.ends_with(".png") {
        exif_of(file)
    } else {
        file.windows(6)
            .position(|w| w == b"Exif\0\0")
            .map(|_| tiff_of(name, file))
    }
}

/// `-<group>:All=` expands to that group's entries, exactly as the oracle
/// (see [`after_group_removal`]). Before, no group removal was expanded: on
/// a JPEG every one reported success and left the block as it was
/// (Canon.jpg's IFD0 kept all 8 rows under `-IFD0:All=`/`-EXIF:All=`).
#[test]
fn group_wide_removals_match_the_oracle() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        // A MakerNote ExifTool recognizes as a directory (an unknown IFD):
        // `MakerNotes:All` deletes it.
        let mut known = full(order);
        let note = [
            order.u16(1).as_slice(),
            &order.u16(0x0001),
            &order.u16(3),
            &order.u32(1),
            &order.u16(5),
            &[0, 0],
            &order.u32(0),
        ]
        .concat();
        for entry in known.exif.as_mut().unwrap().iter_mut() {
            if entry.0 == 0x927C {
                *entry = (0x927C, 7, note.len() as u32, note.clone());
            }
        }
        for (label, tiff, fallback) in [
            ("full", full(order).build(order), true),
            ("known-note", known.build(order), false),
        ] {
            let before = dump(&tiff);
            for (name, original) in [
                ("group.png", png(&[(b"eXIf", tiff.clone())], &[])),
                ("group.jpg", jpeg_with(&tiff)),
            ] {
                for group in GROUP_REMOVALS {
                    let path = write(dir.path(), name, &original);
                    remove_tag(&path, group)
                        .unwrap_or_else(|e| panic!("{order:?} {label} {name} {group}: {e}"));
                    let out = std::fs::read(&path).unwrap();
                    let got = exif_payload(name, &out).map(|t| dump(&t));
                    assert_eq!(
                        got,
                        after_group_removal(group, &before, fallback),
                        "{order:?} {label} {name} {group}"
                    );
                }
            }
        }
    }
}

/// A group removal of a group the block does not hold is a no-op; one of a
/// group it does hold is not (a pointer-only IFD0 still holds the block:
/// the oracle removes it all under `-IFD0:All=`).
#[test]
fn group_wide_removals_on_a_pointer_only_ifd0() {
    let dir = tempfile::tempdir().unwrap();
    let thumb = vec![0xFF, 0xD8, 0xFF, 0xD9];
    for order in [Order::Ii, Order::Mm] {
        let tiff = Tiff {
            ifd0: vec![],
            exif: Some(vec![(0x8827, 3, 1, order.u16(100).to_vec())]),
            interop: None,
            gps: None,
            ifd1: Some((vec![], thumb.clone())),
        }
        .build(order);
        let before = dump(&tiff);
        for (name, original) in [
            ("ptr.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("ptr.jpg", jpeg_with(&tiff)),
        ] {
            for group in GROUP_REMOVALS {
                let path = write(dir.path(), name, &original);
                remove_tag(&path, group)
                    .unwrap_or_else(|e| panic!("{order:?} {name} {group}: {e}"));
                let out = std::fs::read(&path).unwrap();
                let got = exif_payload(name, &out).map(|t| dump(&t));
                let want = match group {
                    "IFD0:All" | "EXIF:All" => None,
                    "ExifIFD:All" => Some(
                        before
                            .iter()
                            .filter(|(k, _)| !k.starts_with("ExifIFD:"))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    ),
                    "IFD1:All" => Some(
                        before
                            .iter()
                            .filter(|(k, _)| !k.starts_with("IFD1:"))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    ),
                    _ => Some(before.clone()),
                };
                assert_eq!(got, want, "{order:?} {name} {group}");
                if want.as_ref() == Some(&before) {
                    assert_eq!(
                        out, original,
                        "{order:?} {name} {group}: a no-op writes nothing"
                    );
                }
            }
        }
    }
}

/// `remove_tag("EXIF:Make")` names IFD0:Make by its family-0 alias; pinned
/// ExifTool 13.59 deletes it. The removal was matched only literally, so
/// IFD0:Make stayed in the map and the write was refused (tip 707c7565
/// reported success with Make still there).
#[test]
fn a_family_alias_removal_of_a_surfaced_entry_deletes_it() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = full(order).build(order);
        let mut want = dump(&tiff);
        want.remove("IFD0:0x010f");
        for (name, original) in [
            ("alias.png", png(&[(b"eXIf", tiff.clone())], &[])),
            ("alias.jpg", jpeg_with(&tiff)),
        ] {
            let path = write(dir.path(), name, &original);
            remove_tag(&path, "EXIF:Make").unwrap_or_else(|e| panic!("{order:?} {name}: {e}"));
            let out = tiff_of(name, &std::fs::read(&path).unwrap());
            assert_eq!(dump(&out), want, "{order:?} {name}");
        }
    }
}

/// On a TIFF-structured file pinned ExifTool 13.59 never deletes IFD0
/// ("Can't delete IFD0 from TIFF", file unchanged, exit 0), so `IFD0:All` is
/// a no-op, not the refusal c175e36b made it; `EXIF:All` deletes only
/// ExifIFD there, and a group with content is a directory deletion this
/// in-place writer refuses. A group the file lacks is a no-op.
#[test]
fn group_wide_removals_on_a_tiff_file() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = Tiff {
            ifd0: vec![
                (0x010F, 2, 5, b"Acme\0".to_vec()),
                (0x013B, 2, 3, b"me\0".to_vec()),
            ],
            exif: Some(vec![(0x8827, 3, 1, order.u16(100).to_vec())]),
            interop: None,
            gps: None,
            ifd1: None,
        }
        .build(order);
        for group in [
            "IFD0:All",
            "GPS:All",
            "IFD1:All",
            "InteropIFD:All",
            "MakerNotes:All",
        ] {
            let path = write(dir.path(), "g.tif", &tiff);
            remove_tag(&path, group).unwrap_or_else(|e| panic!("{order:?} {group}: {e}"));
            assert_eq!(std::fs::read(&path).unwrap(), tiff, "{order:?} {group}");
        }
        for group in ["EXIF:All", "ExifIFD:All"] {
            let path = write(dir.path(), "g.tif", &tiff);
            assert!(remove_tag(&path, group).is_err(), "{order:?} {group}");
            assert_eq!(std::fs::read(&path).unwrap(), tiff, "{order:?} {group}");
        }

        let gps = Tiff {
            ifd0: vec![(0x010F, 2, 5, b"Acme\0".to_vec())],
            exif: None,
            interop: None,
            gps: Some(vec![(0x0000, 1, 4, vec![2, 3, 0, 0])]),
            ifd1: None,
        }
        .build(order);
        for (group, deletes) in [("GPS:All", true), ("EXIF:All", false), ("IFD0:All", false)] {
            let path = write(dir.path(), "gps.tif", &gps);
            assert_eq!(
                remove_tag(&path, group).is_err(),
                deletes,
                "{order:?} {group}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), gps, "{order:?} {group}");
        }
    }
}

/// The pinned t/images TIFF-structured files under every `<group>:All`,
/// against pinned ExifTool 13.59 (`sweep2.py`, review-head-c175e36b): where
/// the oracle leaves the file unchanged -- IFD0 of any TIFF; ExifIFD,
/// MakerNotes and `EXIF:All` of a raw type ("Can't delete ExifIFD from
/// CR2"); a group the file lacks -- the write is a no-op success; where it
/// deletes a directory (InteropIFD of the CR2; ExifIFD and the
/// DNGPrivateData maker note of the DNG; IFD1, InteropIFD and the whole
/// EXIF of the RW2's embedded JpgFromRaw) or errors (IFD1 of the CR2 and
/// IIQ), it is refused. Either way the file is untouched. c175e36b refused
/// eighteen of the no-op cases.
#[test]
fn group_wide_removals_on_tiff_structured_files_follow_the_oracle() {
    const GROUPS: [&str; 7] = [
        "IFD0:All",
        "ExifIFD:All",
        "GPS:All",
        "IFD1:All",
        "InteropIFD:All",
        "MakerNotes:All",
        "EXIF:All",
    ];
    // Refused (true) or a no-op (false), in GROUPS order.
    let cases: [(&str, [bool; 7]); 6] = [
        (
            "CanonRaw.cr2",
            [false, false, false, true, true, false, false],
        ),
        ("DNG.dng", [false, true, false, false, false, true, true]),
        ("ExifTool.tif", [false; 7]),
        ("GeoTiff.tif", [false; 7]),
        (
            "Panasonic.rw2",
            [true, false, false, true, true, false, true],
        ),
        (
            "PhaseOne.iiq",
            [false, false, false, true, false, false, false],
        ),
    ];
    let dir = tempfile::tempdir().unwrap();
    for (name, refused) in cases {
        let Some(sample) = fixtures::pinned_t_images_fixture_path(name) else {
            continue;
        };
        let original = std::fs::read(&sample).unwrap();
        for (group, refused) in GROUPS.iter().zip(refused) {
            let path = write(dir.path(), name, &original);
            let result = remove_tag(&path, group);
            assert_eq!(result.is_err(), refused, "{name} {group}: {result:?}");
            assert_eq!(std::fs::read(&path).unwrap(), original, "{name} {group}");
        }
    }
}

/// `IFD0:All` and `EXIF:All` delete the EXIF carrier wholesale, as pinned
/// ExifTool 13.59 does even for one it cannot read ("1 image files
/// updated", the APP1 / eXIf gone). c175e36b judged an unreadable block
/// to hold nothing and reported success with it left in place.
#[test]
fn carrier_removals_drop_a_malformed_exif_carrier() {
    let dir = tempfile::tempdir().unwrap();
    for (label, payload) in malformed_exif_payloads() {
        for group in ["IFD0:All", "EXIF:All"] {
            let path = write(dir.path(), "bad.jpg", &jpeg_with(&payload));
            remove_tag(&path, group).unwrap_or_else(|e| panic!("{label} jpg {group}: {e}"));
            let out = std::fs::read(&path).unwrap();
            assert!(
                !out.windows(6).any(|w| w == b"Exif\0\0"),
                "{label} {group}: EXIF APP1 left"
            );

            let path = write(
                dir.path(),
                "bad.png",
                &png(&[(b"eXIf", payload.clone())], &[]),
            );
            remove_tag(&path, group).unwrap_or_else(|e| panic!("{label} png {group}: {e}"));
            assert_eq!(
                kinds(&std::fs::read(&path).unwrap()),
                ["IHDR", "IDAT", "IEND"],
                "{label} {group}"
            );
        }
    }
}

/// `MakerNotes:All` on a JPEG holding a Canon CIFF APP0 segment: pinned
/// ExifTool 13.59 drops that segment (its tags are MakerNotes; t/images
/// ExifTool.jpg, "1 image files updated", every other segment
/// byte-identical). c175e36b reported success with the segment kept; tip
/// e4edc55c also kept it and rewrote the EXIF APP1.
#[test]
fn makernotes_removal_drops_a_ciff_segment() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let plain = jpeg_with(&full(order).build(order));
        let ciff = [
            match order {
                Order::Ii => b"II".as_slice(),
                Order::Mm => b"MM".as_slice(),
            },
            &order.u32(26),
            b"HEAPJPGM",
            &[0; 16],
        ]
        .concat();
        let mut jpeg = plain[..2].to_vec();
        jpeg.extend([0xFF, 0xE0]);
        jpeg.extend(((ciff.len() + 2) as u16).to_be_bytes());
        jpeg.extend(&ciff);
        jpeg.extend(&plain[2..]);
        let path = write(dir.path(), "ciff.jpg", &jpeg);
        remove_tag(&path, "MakerNotes:All").unwrap_or_else(|e| panic!("{order:?}: {e}"));
        let out = std::fs::read(&path).unwrap();
        assert!(
            !out.windows(8).any(|w| w == b"HEAPJPGM"),
            "{order:?}: CIFF left"
        );
        // Only the CIFF segment went: the rest is the file without it, but
        // for the EXIF APP1 when the block held a maker note.
        let path = write(dir.path(), "plain.jpg", &plain);
        remove_tag(&path, "MakerNotes:All").unwrap();
        assert_eq!(out, std::fs::read(&path).unwrap(), "{order:?}");
    }
    let Some(sample) = fixtures::pinned_t_images_fixture_path("ExifTool.jpg") else {
        return;
    };
    let original = std::fs::read(&sample).unwrap();
    let path = write(dir.path(), "ExifTool.jpg", &original);
    remove_tag(&path, "MakerNotes:All").unwrap();
    let out = std::fs::read(&path).unwrap();
    // The output is the input without its CIFF APP0 segment (marker,
    // length, `II` + header length, then `HEAPJPGM`).
    let start = original
        .windows(8)
        .position(|w| w == b"HEAPJPGM")
        .expect("CIFF APP0")
        - 10;
    assert_eq!(original[start..start + 2], [0xFF, 0xE0]);
    let len = u16::from_be_bytes([original[start + 2], original[start + 3]]) as usize;
    let expected = [&original[..start], &original[start + 2 + len..]].concat();
    assert_eq!(out, expected);
}

/// An EXIF-family removal rewrites the EXIF block, and pinned ExifTool
/// 13.59 does not write back a JPEG APP1 left with no entry: on a bare
/// empty IFD0 `-IFD0:Software=` and `-GPS:All=` drop the APP1 ("1 image
/// files updated"; tip e4edc55c byte-identical to it). b83ec323's up-front
/// no-op check kept it. An empty PNG eXIf chunk the oracle keeps ("1 image
/// files unchanged") -- but for `IFD0:All` / `EXIF:All`, which delete it.
/// A JPEG block whose only fault is its magic number the oracle reads
/// anyway and drops when empty; the writer's scanner cannot
/// read it, so that write is refused, as at tip, not reported done. With
/// an entry in it, a removal naming nothing is a no-op (oracle unchanged).
#[test]
fn an_empty_exif_app1_is_dropped_by_an_exif_removal() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let head = |magic: u16| {
            [
                match order {
                    Order::Ii => b"II".as_slice(),
                    Order::Mm => b"MM".as_slice(),
                },
                &order.u16(magic),
                &order.u32(8),
            ]
            .concat()
        };
        let empty = [head(42), order.u16(0).to_vec(), order.u32(0).to_vec()].concat();
        let bad_magic = [head(43), order.u16(0).to_vec(), order.u32(0).to_vec()].concat();
        let bad_magic_make = [
            head(43),
            order.u16(1).to_vec(),
            order.u16(0x010F).to_vec(),
            order.u16(2).to_vec(),
            order.u32(4).to_vec(),
            b"Acme".to_vec(),
            order.u32(0).to_vec(),
        ]
        .concat();
        for key in ["IFD0:Software", "GPS:All"] {
            let path = write(dir.path(), "empty.jpg", &jpeg_with(&empty));
            remove_tag(&path, key).unwrap_or_else(|e| panic!("{order:?} {key}: {e}"));
            assert_eq!(
                std::fs::read(&path).unwrap(),
                jpeg_without_exif(),
                "{order:?} {key}"
            );

            let png_bytes = png(&[(b"eXIf", empty.clone())], &[]);
            let path = write(dir.path(), "empty.png", &png_bytes);
            remove_tag(&path, key).unwrap_or_else(|e| panic!("{order:?} {key} png: {e}"));
            assert_eq!(
                std::fs::read(&path).unwrap(),
                png_bytes,
                "{order:?} {key} png"
            );
            let carrier = if key == "GPS:All" {
                "EXIF:All"
            } else {
                "IFD0:All"
            };
            remove_tag(&path, carrier).unwrap_or_else(|e| panic!("{order:?} {carrier}: {e}"));
            assert_eq!(
                kinds(&std::fs::read(&path).unwrap()),
                ["IHDR", "IDAT", "IEND"],
                "{order:?} {carrier} png"
            );

            let jpeg = jpeg_with(&bad_magic);
            let path = write(dir.path(), "magic.jpg", &jpeg);
            assert!(remove_tag(&path, key).is_err(), "{order:?} {key} magic");
            assert_eq!(std::fs::read(&path).unwrap(), jpeg, "{order:?} {key} magic");

            let jpeg = jpeg_with(&bad_magic_make);
            let path = write(dir.path(), "magic-make.jpg", &jpeg);
            remove_tag(&path, key).unwrap_or_else(|e| panic!("{order:?} {key} magic+Make: {e}"));
            assert_eq!(
                std::fs::read(&path).unwrap(),
                jpeg,
                "{order:?} {key} magic+Make"
            );
        }
    }
}
