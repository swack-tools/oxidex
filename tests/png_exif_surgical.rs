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

use oxidex::core::operations::{modify_tag, read_metadata, remove_tag};
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
fn dump(tiff: &[u8]) -> BTreeMap<String, (u16, u32, Vec<u8>)> {
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
    before: &BTreeMap<String, (u16, u32, Vec<u8>)>,
    after: &BTreeMap<String, (u16, u32, Vec<u8>)>,
    changed: &[(&str, Option<(u16, u32, &[u8])>)],
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
