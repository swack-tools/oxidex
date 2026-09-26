//! Maker-note data that lies OUTSIDE the MakerNote entry's byte count must
//! survive an EXIF edit, or the edit must be refused with the file left
//! byte-identical. It must never be overwritten.
//!
//! A MakerNote is one ExifIFD entry (0x927C) with a declared byte count, but
//! vendors point from inside it to bytes outside it. Casio Type2 keeps its
//! PreviewImage (0x2000) after the note, where pinned ExifTool 13.59 reads it
//! from `t/images/Casio2.jpg` (`-v3`: note at TIFF 742..1252, preview at
//! 1412); Canon's PreviewImageInfo (0x00B6) locates a preview in the JPEG
//! trailer, past the EXIF block. ExifTool rebuilds the note and relocates the
//! data (WriteExif.pl 13.59: `RebuildMakerNotes`, the `PREVIEW_INFO`
//! fix-ups at 1992-2004 and 2657-2680, and Writer.pl 6177-6226 for a trailer
//! preview). The surgical writer pins the note at its original offset and
//! re-lays everything else out, so before this fix it wrote the new
//! ImageDescription over Casio's preview: `-IFD0:ImageDescription=DDD...`
//! read back `Casio:PreviewImage` as `DDDD...` in both JPEG and PNG
//! (`makernote_outofblob_matrix.py matrix`, base 6c8a7628).
//!
//! The synthetic blocks below reproduce the shapes the matrix measured on
//! real files: a preview in bytes no standard structure owns (a "hole"), a
//! maker-note value pointing into another tag's value ("claimed"), and a
//! preview in the JPEG trailer ("beyond-tiff"). Every case runs in both byte
//! orders and, where the carrier can hold it, in JPEG and in PNG.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::{modify_tag, read_metadata, remove_tag};
use oxidex::core::tag_value::TagValue;
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
        match self {
            Order::Ii => u16::from_le_bytes([b[0], b[1]]),
            Order::Mm => u16::from_be_bytes([b[0], b[1]]),
        }
    }
    fn read_u32(self, b: &[u8]) -> u32 {
        match self {
            Order::Ii => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            Order::Mm => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        }
    }
    fn mark(self) -> &'static [u8; 2] {
        match self {
            Order::Ii => b"II",
            Order::Mm => b"MM",
        }
    }
}

/// An IFD entry whose value is either inline (<= 4 bytes) or out of line.
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    value: Vec<u8>,
}

fn entry(tag: u16, kind: u16, count: u32, value: Vec<u8>) -> Entry {
    Entry {
        tag,
        kind,
        count,
        value,
    }
}

/// Emits one IFD table at `at` (values follow it), returning the bytes of
/// table + values. `fixed` pre-assigns a value offset (for an entry whose
/// value lives somewhere else, e.g. inside another tag's value).
fn ifd(order: Order, at: usize, entries: &[Entry], fixed: &[(u16, u32)]) -> Vec<u8> {
    let table = 2 + 12 * entries.len() + 4;
    let mut values = Vec::new();
    let mut out = order.u16(entries.len() as u16).to_vec();
    for e in entries {
        out.extend(order.u16(e.tag));
        out.extend(order.u16(e.kind));
        out.extend(order.u32(e.count));
        if let Some((_, off)) = fixed.iter().find(|(tag, _)| *tag == e.tag) {
            out.extend(order.u32(*off));
        } else if e.value.len() <= 4 {
            let mut v = e.value.clone();
            v.resize(4, 0);
            out.extend(v);
        } else {
            out.extend(order.u32((at + table + values.len()) as u32));
            values.extend(&e.value);
            if values.len() % 2 == 1 {
                values.push(0);
            }
        }
    }
    out.extend([0, 0, 0, 0]);
    out.extend(values);
    out
}

fn ascii(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

/// Where the preview data a maker note points at lives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    /// After every structure, in bytes nothing but the maker note owns.
    Hole,
    /// Inside IFD0's Software value (another tag's bytes).
    Software,
}

const PREVIEW: &[u8] = b"<Out-of-blob preview bytes, 44 of them.....>";
const SOFTWARE: &str = "Firmware Version 1.00 build 2003-02-28 0945 x";

/// A Casio Type2 EXIF block (`QVC\0` + 2 bytes, then an IFD whose offsets
/// count from the TIFF header, MakerNotes.pm 13.59 `MakerNoteCasio2`), whose
/// PreviewImage (0x2000) and PreviewImageStart/Length (0x0004/0x0003) point
/// at `PREVIEW` outside the note. Returns the block and the preview's TIFF
/// offset.
fn casio_block(order: Order, target: Target) -> (Vec<u8>, usize) {
    let make = ascii("CASIO COMPUTER CO.,LTD");
    let model = ascii("EX-Z55");
    let software = ascii(SOFTWARE);
    let ifd0_entries = |exif_at: u32| {
        vec![
            entry(0x010F, 2, make.len() as u32, make.clone()),
            entry(0x0110, 2, model.len() as u32, model.clone()),
            entry(0x0131, 2, software.len() as u32, software.clone()),
            entry(0x0132, 2, 20, ascii("2003:02:28 09:45:00")),
            entry(0x8769, 4, 1, order.u32(exif_at).to_vec()),
        ]
    };
    // IFD0 at 8; its size does not depend on the ExifIFD offset
    let ifd0_len = ifd(order, 8, &ifd0_entries(0), &[]).len();
    let exif_at = 8 + ifd0_len;
    // Software's value is the third out-of-line IFD0 value (each padded even)
    let padded = |n: usize| n + n % 2;
    let software_at = 8 + 2 + 12 * 5 + 4 + padded(make.len()) + padded(model.len());
    assert_eq!(
        &ifd(order, 8, &ifd0_entries(0), &[])[software_at - 8..software_at - 8 + software.len()],
        software.as_slice()
    );

    let exif_table = 2 + 12 * 2 + 4;
    let note_at = exif_at + exif_table + 8; // after ExposureTime's rational
    let note_ifd_at = note_at + 6;
    let note_entries = 5usize;
    let note_len = 6 + 2 + 12 * note_entries + 4 + 18;
    let (preview_at, preview_len) = match target {
        Target::Hole => ((note_at + note_len + 2 + 1) & !1, PREVIEW.len()),
        Target::Software => (software_at, software.len()),
    };
    let mut note = b"QVC\0\0\0".to_vec();
    note.extend(ifd(
        order,
        note_ifd_at,
        &[
            entry(0x0002, 3, 2, [order.u16(320), order.u16(240)].concat()),
            entry(0x0003, 4, 1, order.u32(preview_len as u32).to_vec()),
            entry(0x0004, 4, 1, order.u32(preview_at as u32).to_vec()),
            entry(0x2000, 7, preview_len as u32, vec![0; preview_len]),
            entry(0x2001, 7, 18, b"0302\0\x0028\0\x0009\0\x0045\0\0".to_vec()),
        ],
        &[(0x2000, preview_at as u32)],
    ));
    assert_eq!(note.len(), note_len);

    let mut tiff = order.mark().to_vec();
    tiff.extend(order.u16(42));
    tiff.extend(order.u32(8));
    tiff.extend(ifd(order, 8, &ifd0_entries(exif_at as u32), &[]));
    assert_eq!(tiff.len(), exif_at);
    tiff.extend(ifd(
        order,
        exif_at,
        &[
            entry(0x829A, 5, 1, [order.u32(1), order.u32(60)].concat()),
            entry(0x927C, 7, note.len() as u32, note),
        ],
        &[],
    ));
    assert_eq!(tiff.len(), note_at + note_len);
    if target == Target::Hole {
        tiff.resize(preview_at, 0);
        tiff.extend(PREVIEW);
    }
    (tiff, preview_at)
}

/// A Canon EXIF block whose PreviewImageInfo (0x00B6: int32u[12] as the
/// EOS 300D writes it, index 5
/// PreviewImageStart with `IsOffset`, Canon.pm 13.59) locates a preview
/// `trailer_at` bytes past the TIFF header -- in the JPEG trailer, outside
/// the EXIF segment altogether.
fn canon_block(order: Order, trailer_at: u32, len: u32) -> Vec<u8> {
    let make = ascii("Canon");
    let model = ascii("Canon EOS 300D DIGITAL");
    let software = ascii(SOFTWARE);
    let ifd0_entries = |exif_at: u32| {
        vec![
            entry(0x010F, 2, make.len() as u32, make.clone()),
            entry(0x0110, 2, model.len() as u32, model.clone()),
            entry(0x0131, 2, software.len() as u32, software.clone()),
            entry(0x0132, 2, 20, ascii("2003:02:28 09:45:00")),
            entry(0x8769, 4, 1, order.u32(exif_at).to_vec()),
        ]
    };
    let exif_at = 8 + ifd(order, 8, &ifd0_entries(0), &[]).len();
    let note_at = exif_at + 2 + 12 * 2 + 4 + 8;
    let info: Vec<u8> = [24, 2, len, 160, 120, trailer_at, 0, 0, 0, 0, 0, 0]
        .iter()
        .flat_map(|v| order.u32(*v))
        .collect();
    let note = ifd(
        order,
        note_at,
        &[
            entry(0x0006, 2, 13, ascii("IMG:EOS 300D")),
            entry(0x00B6, 4, 12, info),
        ],
        &[],
    );
    let mut tiff = order.mark().to_vec();
    tiff.extend(order.u16(42));
    tiff.extend(order.u32(8));
    tiff.extend(ifd(order, 8, &ifd0_entries(exif_at as u32), &[]));
    tiff.extend(ifd(
        order,
        exif_at,
        &[
            entry(0x829A, 5, 1, [order.u32(1), order.u32(60)].concat()),
            entry(0x927C, 7, note.len() as u32, note),
        ],
        &[],
    ));
    tiff
}

fn jpeg_body() -> Vec<u8> {
    const BODY: &str = "ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";
    (0..BODY.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&BODY[i..i + 2], 16).unwrap())
        .collect()
}

/// SOI, the EXIF APP1 holding `tiff`, then a small baseline JPEG body.
/// The TIFF header sits at file offset 12.
fn jpeg_with(tiff: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
    out.extend(((tiff.len() + 8) as u16).to_be_bytes());
    out.extend(b"Exif\0\0");
    out.extend(tiff);
    out.extend(jpeg_body());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
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
    out.extend(kind);
    out.extend(data);
    out.extend(crc32(&[kind.as_slice(), data].concat()).to_be_bytes());
    out
}

/// A 1x1 PNG carrying `tiff` in an `eXIf` chunk before IDAT.
fn png_with(tiff: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
    out.extend(chunk(b"eXIf", tiff));
    out.extend(chunk(
        b"IDAT",
        &[
            0x78, 0x9C, 0x63, 0x60, 0x60, 0x60, 0x00, 0x00, 0x00, 0x04, 0x00, 0x01,
        ],
    ));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// The TIFF block of a JPEG (first Exif APP1) or PNG (`eXIf`) file.
fn tiff_of(file: &[u8]) -> Vec<u8> {
    if file.starts_with(b"\x89PNG") {
        let mut i = 8;
        while i + 8 <= file.len() {
            let len = u32::from_be_bytes(file[i..i + 4].try_into().unwrap()) as usize;
            if &file[i + 4..i + 8] == b"eXIf" {
                return file[i + 8..i + 8 + len].to_vec();
            }
            i += 12 + len;
        }
        panic!("no eXIf chunk");
    }
    assert_eq!(&file[2..4], &[0xFF, 0xE1]);
    let len = u16::from_be_bytes([file[4], file[5]]) as usize;
    assert_eq!(&file[6..12], b"Exif\0\0");
    file[12..4 + len].to_vec()
}

/// Casio's PreviewImage (0x2000) value bytes, found by walking the written
/// block the way a reader does: IFD0 -> ExifIFD -> MakerNote -> its IFD at
/// +6, value offsets counted from the TIFF header. Independent of where the
/// writer chose to put anything.
fn casio_preview(tiff: &[u8]) -> Vec<u8> {
    let order = if &tiff[..2] == b"II" {
        Order::Ii
    } else {
        Order::Mm
    };
    let find = |ifd_at: usize, tag: u16| -> (u32, u32) {
        let n = order.read_u16(&tiff[ifd_at..]) as usize;
        (0..n)
            .map(|i| &tiff[ifd_at + 2 + 12 * i..])
            .find(|e| order.read_u16(e) == tag)
            .map(|e| (order.read_u32(&e[4..]), order.read_u32(&e[8..])))
            .unwrap_or_else(|| panic!("tag 0x{tag:04x} missing"))
    };
    let ifd0 = order.read_u32(&tiff[4..]) as usize;
    let (_, exif) = find(ifd0, 0x8769);
    let (_, note) = find(exif as usize, 0x927C);
    let (count, at) = find(note as usize + 6, 0x2000);
    tiff[at as usize..(at + count) as usize].to_vec()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// The edits of the matrix: a growing, a shrinking and a same-size one.
#[derive(Clone, Copy, Debug)]
enum Edit {
    Grow,
    Shrink,
    Same,
}

fn apply(path: &Path, edit: Edit) -> oxidex::error::Result<()> {
    match edit {
        Edit::Grow => modify_tag(
            path,
            "IFD0:ImageDescription",
            TagValue::new_string("D".repeat(3000)),
        ),
        Edit::Shrink => remove_tag(path, "IFD0:Software"),
        Edit::Same => modify_tag(
            path,
            "IFD0:ModifyDate",
            TagValue::new_datetime(
                chrono::NaiveDate::from_ymd_opt(2001, 2, 3)
                    .unwrap()
                    .and_hms_opt(4, 5, 6)
                    .unwrap()
                    .and_utc(),
            ),
        ),
    }
}

/// The reader's own view of the maker-note preview: every `PreviewImage`
/// row it produced, by key.
fn reader_previews(path: &Path) -> Vec<(String, TagValue)> {
    let map = read_metadata(path).unwrap();
    let mut rows: Vec<(String, TagValue)> = map
        .iter()
        .filter(|(key, _)| key.ends_with(":PreviewImage"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// Casio2's shape: the preview sits in a hole after the maker note. Every
/// edit, in both carriers and both byte orders, must leave it readable and
/// unchanged -- by a structural walk of the written block and by the reader.
/// Red before the fix: grow/shrink wrote other values over it.
#[test]
fn a_preview_outside_the_maker_note_survives_every_edit() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Mm, Order::Ii] {
        let (tiff, _) = casio_block(order, Target::Hole);
        assert_eq!(casio_preview(&tiff), PREVIEW);
        for (carrier, file) in [("jpg", jpeg_with(&tiff)), ("png", png_with(&tiff))] {
            let original = write(dir.path(), &format!("orig.{carrier}"), &file);
            let previews = reader_previews(&original);
            assert!(
                previews
                    .iter()
                    .any(|(_, v)| *v == TagValue::Binary(PREVIEW.to_vec())),
                "{order:?} {carrier}: the reader must see the preview before any edit: {previews:?}"
            );
            for edit in [Edit::Grow, Edit::Shrink, Edit::Same] {
                let path = write(dir.path(), &format!("{edit:?}.{carrier}"), &file);
                apply(&path, edit).unwrap_or_else(|e| {
                    panic!("{order:?} {carrier} {edit:?}: a hole-resident preview can be kept: {e}")
                });
                let out = std::fs::read(&path).unwrap();
                assert_eq!(
                    casio_preview(&tiff_of(&out)),
                    PREVIEW,
                    "{order:?} {carrier} {edit:?}: preview bytes overwritten"
                );
                assert_eq!(
                    reader_previews(&path),
                    previews,
                    "{order:?} {carrier} {edit:?}: reader's PreviewImage changed"
                );
            }
        }
    }
}

/// A maker-note value that points into another tag's value. Editing an
/// unrelated tag must keep it; deleting the tag whose bytes it borrows
/// cannot keep both, so that write is refused and the file left as it was
/// (ExifTool copies the borrowed bytes into its rebuilt note instead).
#[test]
fn a_maker_note_value_borrowing_another_tags_bytes_is_kept_or_refused() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Mm, Order::Ii] {
        let (tiff, _) = casio_block(order, Target::Software);
        let borrowed = ascii(SOFTWARE);
        assert_eq!(casio_preview(&tiff), borrowed);
        for (carrier, file) in [("jpg", jpeg_with(&tiff)), ("png", png_with(&tiff))] {
            for edit in [Edit::Grow, Edit::Same, Edit::Shrink] {
                let path = write(dir.path(), &format!("borrow-{edit:?}.{carrier}"), &file);
                match apply(&path, edit) {
                    Ok(()) => {
                        let out = std::fs::read(&path).unwrap();
                        assert_eq!(
                            casio_preview(&tiff_of(&out)),
                            borrowed,
                            "{order:?} {carrier} {edit:?}: borrowed bytes overwritten"
                        );
                    }
                    Err(e) => {
                        assert!(
                            matches!(edit, Edit::Shrink),
                            "{order:?} {carrier} {edit:?}: only the deletion of the lender may be refused: {e}"
                        );
                        assert!(e.to_string().contains("maker"), "{e}");
                        assert_eq!(std::fs::read(&path).unwrap(), file, "refused but modified");
                    }
                }
            }
        }
    }
}

/// Canon's PreviewImageInfo locates a preview in the JPEG trailer, past the
/// EXIF segment: any change in the segment's length moves it away from the
/// offset the pinned maker note still holds. Growing the segment must be
/// refused untouched; a same-size or shrinking edit may keep the segment's
/// length and so the preview's position.
#[test]
fn a_preview_in_the_jpeg_trailer_is_never_shifted_away() {
    let dir = tempfile::tempdir().unwrap();
    let preview: Vec<u8> = [&[0xFF, 0xD8, 0xFF, 0xDB][..], &[0x11; 40], &[0xFF, 0xD9]].concat();
    for order in [Order::Mm, Order::Ii] {
        // two passes: the trailer offset depends on the block's own length
        let probe = canon_block(order, 0, preview.len() as u32);
        let file_len = jpeg_with(&probe).len();
        let trailer_at = (file_len - 12) as u32; // TIFF-relative, i.e. file - 12
        let tiff = canon_block(order, trailer_at, preview.len() as u32);
        let mut file = jpeg_with(&tiff);
        assert_eq!(file.len(), file_len);
        file.extend(&preview);
        let at = |bytes: &[u8]| -> Vec<u8> {
            let start = 12 + trailer_at as usize;
            bytes
                .get(start..start + preview.len())
                .map(<[u8]>::to_vec)
                .unwrap_or_default()
        };
        assert_eq!(at(&file), preview);
        let original = write(dir.path(), "canon.jpg", &file);
        let map = read_metadata(&original).unwrap();
        let start_key = map
            .keys()
            .find(|k| k.ends_with(":PreviewImageStart"))
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "reader reports no PreviewImageStart: {:?}",
                    map.keys().collect::<Vec<_>>()
                )
            });
        let start = match map.get(&start_key) {
            Some(TagValue::Integer(v)) => Some(*v),
            Some(TagValue::String(s)) => s.parse().ok(),
            _ => None,
        };
        assert_eq!(start, Some(12 + i64::from(trailer_at)), "{order:?}");
        for edit in [Edit::Grow, Edit::Shrink, Edit::Same] {
            let path = write(dir.path(), &format!("canon-{edit:?}.jpg"), &file);
            match apply(&path, edit) {
                Ok(()) => {
                    assert!(
                        !matches!(edit, Edit::Grow),
                        "{order:?}: growth cannot keep the trailer's position"
                    );
                    let out = std::fs::read(&path).unwrap();
                    assert_eq!(
                        at(&out),
                        preview,
                        "{order:?} {edit:?}: trailer preview moved"
                    );
                }
                Err(e) => {
                    assert!(
                        matches!(edit, Edit::Grow),
                        "{order:?} {edit:?}: a length-preserving edit can keep the trailer: {e}"
                    );
                    assert!(e.to_string().contains("maker"), "{e}");
                    assert_eq!(std::fs::read(&path).unwrap(), file, "refused but modified");
                }
            }
        }
    }
}

/// [`casio_block`]'s hole-resident preview, with an IFD1 appended after it
/// whose Compression is an inline SHORT carrying two non-zero bytes in the
/// unused half of its value field -- and the preview's count stretched over
/// that IFD1 table. The corpus's Olympus FE-120 has this shape: its
/// `DataDump` runs from inside the note over the UserComment and through
/// IFD1's table. The table must come back byte for byte, padding included.
fn casio_block_over_ifd1(order: Order) -> (Vec<u8>, usize, usize) {
    let (mut tiff, preview_at) = casio_block(order, Target::Hole);
    let ifd1_at = (tiff.len() + 1) & !1;
    tiff.resize(ifd1_at, 0);
    let mut compression = order.u16(6).to_vec();
    compression.extend([0x20, 0x31]); // what the camera left in the field
    let ifd1 = [
        order.u16(2).to_vec(),
        [order.u16(0x0103), order.u16(3)].concat(),
        order.u32(1).to_vec(),
        compression,
        [order.u16(0x011A), order.u16(5)].concat(),
        order.u32(1).to_vec(),
        order.u32((ifd1_at + 2 + 24 + 4) as u32).to_vec(),
        vec![0; 4],
        [order.u32(72), order.u32(1)].concat(),
    ]
    .concat();
    tiff.extend(&ifd1);
    // IFD0's next-IFD field: IFD0 has 5 rows at 8
    let next_at = 8 + 2 + 12 * 5;
    tiff[next_at..next_at + 4].copy_from_slice(&order.u32(ifd1_at as u32));
    // stretch the preview (and its length tag) to the end of the table
    let preview_len = ifd1_at + 2 + 24 + 4 - preview_at;
    let find = |tiff: &[u8], tag: u16| -> usize {
        let ifd0 = 8;
        let n = order.read_u16(&tiff[ifd0..]) as usize;
        let exif = (0..n)
            .map(|i| ifd0 + 2 + 12 * i)
            .find(|at| order.read_u16(&tiff[*at..]) == 0x8769)
            .map(|at| order.read_u32(&tiff[at + 8..]) as usize)
            .unwrap();
        let note = (0..2)
            .map(|i| exif + 2 + 12 * i)
            .find(|at| order.read_u16(&tiff[*at..]) == 0x927C)
            .map(|at| order.read_u32(&tiff[at + 8..]) as usize)
            .unwrap();
        let rows = note + 6;
        (0..5)
            .map(|i| rows + 2 + 12 * i)
            .find(|at| order.read_u16(&tiff[*at..]) == tag)
            .unwrap()
    };
    let entry = find(&tiff, 0x2000);
    tiff[entry + 4..entry + 8].copy_from_slice(&order.u32(preview_len as u32));
    let entry = find(&tiff, 0x0003);
    tiff[entry + 8..entry + 12].copy_from_slice(&order.u32(preview_len as u32));
    (tiff, preview_at, preview_len)
}

#[test]
fn a_maker_note_value_spanning_ifd1_keeps_its_unused_value_bytes() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Mm, Order::Ii] {
        let (tiff, at, len) = casio_block_over_ifd1(order);
        let expected = tiff[at..at + len].to_vec();
        assert_eq!(casio_preview(&tiff), expected);
        for (carrier, file) in [("jpg", jpeg_with(&tiff)), ("png", png_with(&tiff))] {
            for edit in [Edit::Grow, Edit::Same] {
                let path = write(dir.path(), &format!("ifd1-{edit:?}.{carrier}"), &file);
                apply(&path, edit).unwrap_or_else(|e| {
                    panic!("{order:?} {carrier} {edit:?}: IFD1 is not edited, so it can stay: {e}")
                });
                let out = std::fs::read(&path).unwrap();
                assert_eq!(
                    casio_preview(&tiff_of(&out)),
                    expected,
                    "{order:?} {carrier} {edit:?}: bytes under the maker-note value changed"
                );
            }
        }
    }
}

/// Deleting the maker note (or a group that holds it, or the whole block)
/// is what the caller asked for: "maker note preserved" no longer applies,
/// and the guard must not refuse it. `MakerNotes:All` and `ExifIFD:All`
/// drop the note; `EXIF:All` and `IFD0:All` drop the block (pinned ExifTool
/// 13.59 behaviour, as `png_exif_surgical::group_wide_removals_match_the_oracle`
/// pins it for a note without out-of-note data).
#[test]
fn deleting_the_maker_note_or_its_block_is_never_refused_by_the_guard() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Mm, Order::Ii] {
        let (tiff, _) = casio_block(order, Target::Hole);
        for (carrier, file) in [("jpg", jpeg_with(&tiff)), ("png", png_with(&tiff))] {
            for group in ["MakerNotes:All", "ExifIFD:All", "EXIF:All", "IFD0:All"] {
                let path = write(dir.path(), &format!("del.{carrier}"), &file);
                remove_tag(&path, group).unwrap_or_else(|e| {
                    panic!("{order:?} {carrier} {group}: deletion refused: {e}")
                });
                let out = std::fs::read(&path).unwrap();
                assert_ne!(out, file, "{order:?} {carrier} {group}: nothing deleted");
                assert!(
                    reader_previews(&path).is_empty(),
                    "{order:?} {carrier} {group}: the maker note's preview is still read"
                );
            }
        }
    }
}

/// A value of one directory entry in [`pentax_shaped_block`]'s builder.
enum Val {
    /// Raw bytes (inline when 4 or fewer).
    Bytes(Vec<u8>),
    /// A LONG pointer to directory `n`.
    Dir(usize),
    /// A LONG offset / length of blob `n`.
    BlobAt(usize),
    BlobLen(usize),
}

/// t/images Pentax.jpg's shape, synthetically: every directory (IFD0,
/// ExifIFD, InteropIFD, GPS, IFD1 with a thumbnail) laid out first and the
/// maker-note preview as the very last bytes of the block, after IFD1's
/// thumbnail -- so any edit that shortens the block (`-IFD1:All=`,
/// `-InteropIFD:All=`, `-GPS:All=`) cuts it off unless it is kept where it
/// is. The note is a Casio Type2 (`QVC\0`, TIFF-relative offsets) whose
/// 0x2000 PreviewImage and 0x0004/0x0003 PreviewImageStart/Length locate
/// the preview. Pinned ExifTool 13.59 relocates such a preview and shifts
/// the pointer (Pentax.jpg under `-IFD1:All=`: PreviewImageStart 2446 ->
/// 2324, preview byte-identical, `-validate` clean). Returns the block and
/// the preview's TIFF offset.
fn pentax_shaped_block(order: Order) -> (Vec<u8>, usize) {
    const THUMB: &[u8] = &[0xFF, 0xD8, 0xFF, 0xDB, 1, 2, 3, 4, 5, 6, 0xFF, 0xD9];
    // blobs: 0 = maker note, 1 = thumbnail, 2 = preview
    let note_len = 6 + 2 + 12 * 5 + 4 + 18;
    let blobs_len = [note_len, THUMB.len(), PREVIEW.len()];
    let rational = |n: u32, d: u32| [order.u32(n), order.u32(d)].concat();
    let dirs = |note: Vec<u8>| -> Vec<Vec<(u16, u16, u32, Val)>> {
        vec![
            // 0: IFD0 (next: IFD1 = dir 4)
            vec![
                (0x010F, 2, 23, Val::Bytes(ascii("CASIO COMPUTER CO.,LTD"))),
                (0x0110, 2, 7, Val::Bytes(ascii("EX-Z55"))),
                (0x0131, 2, 46, Val::Bytes(ascii(SOFTWARE))),
                (0x0132, 2, 20, Val::Bytes(ascii("2003:02:28 09:45:00"))),
                (0x8769, 4, 1, Val::Dir(1)),
                (0x8825, 4, 1, Val::Dir(3)),
            ],
            // 1: ExifIFD
            vec![
                (0x829A, 5, 1, Val::Bytes(rational(1, 60))),
                (0x927C, 7, note.len() as u32, Val::BlobAt(0)),
                (0xA005, 4, 1, Val::Dir(2)),
            ],
            // 2: InteropIFD
            vec![(0x0001, 2, 4, Val::Bytes(b"R98\0".to_vec()))],
            // 3: GPS
            vec![
                (0x0000, 1, 4, Val::Bytes(vec![2, 2, 0, 0])),
                (0x0006, 5, 1, Val::Bytes(rational(50, 1))),
            ],
            // 4: IFD1
            vec![
                (0x0103, 3, 1, Val::Bytes(order.u16(6).to_vec())),
                (0x0201, 4, 1, Val::BlobAt(1)),
                (0x0202, 4, 1, Val::BlobLen(1)),
            ],
        ]
    };
    // Pass 1: table and value offsets, then the blobs in order at the end.
    let layout = |dirs: &[Vec<(u16, u16, u32, Val)>]| -> (Vec<usize>, Vec<usize>, usize) {
        let mut at = 8;
        let mut dir_at = Vec::new();
        for d in dirs {
            dir_at.push(at);
            at += 2 + 12 * d.len() + 4;
            for (_, _, _, v) in d {
                if let Val::Bytes(b) = v
                    && b.len() > 4
                {
                    at += b.len() + b.len() % 2;
                }
            }
        }
        let mut blob_at = Vec::new();
        for len in blobs_len {
            blob_at.push(at);
            at += len + len % 2;
        }
        (dir_at, blob_at, at)
    };
    let (dir_at, blob_at, _) = layout(&dirs(vec![0; note_len]));
    let preview_at = blob_at[2];
    // The note, now that the preview's offset is known.
    let mut note = b"QVC\0\0\0".to_vec();
    note.extend(ifd(
        order,
        blob_at[0] + 6,
        &[
            entry(0x0002, 3, 2, [order.u16(320), order.u16(240)].concat()),
            entry(0x0003, 4, 1, order.u32(PREVIEW.len() as u32).to_vec()),
            entry(0x0004, 4, 1, order.u32(preview_at as u32).to_vec()),
            entry(0x2000, 7, PREVIEW.len() as u32, vec![0; PREVIEW.len()]),
            entry(0x2001, 7, 18, b"0302\0\x0028\0\x0009\0\x0045\0\0".to_vec()),
        ],
        &[(0x2000, preview_at as u32)],
    ));
    assert_eq!(note.len(), note_len);
    let dirs = dirs(note.clone());
    // Pass 2: emit.
    let mut tiff = order.mark().to_vec();
    tiff.extend(order.u16(42));
    tiff.extend(order.u32(8));
    for (i, d) in dirs.iter().enumerate() {
        assert_eq!(tiff.len(), dir_at[i]);
        let table = 2 + 12 * d.len() + 4;
        let mut values = Vec::new();
        tiff.extend(order.u16(d.len() as u16));
        for (tag, kind, count, v) in d {
            tiff.extend(order.u16(*tag));
            tiff.extend(order.u16(*kind));
            tiff.extend(order.u32(*count));
            match v {
                Val::Dir(n) => tiff.extend(order.u32(dir_at[*n] as u32)),
                Val::BlobAt(n) => tiff.extend(order.u32(blob_at[*n] as u32)),
                Val::BlobLen(n) => tiff.extend(order.u32(blobs_len[*n] as u32)),
                Val::Bytes(b) if b.len() <= 4 => {
                    let mut f = b.clone();
                    f.resize(4, 0);
                    tiff.extend(f);
                }
                Val::Bytes(b) => {
                    tiff.extend(order.u32((dir_at[i] + table + values.len()) as u32));
                    values.extend(b);
                    if values.len() % 2 == 1 {
                        values.push(0);
                    }
                }
            }
        }
        tiff.extend(order.u32(if i == 0 { dir_at[4] as u32 } else { 0 }));
        tiff.extend(values);
    }
    for (n, bytes) in [note.as_slice(), THUMB, PREVIEW].iter().enumerate() {
        assert_eq!(tiff.len(), blob_at[n]);
        tiff.extend(*bytes);
        if bytes.len() % 2 == 1 {
            tiff.push(0);
        }
    }
    assert_eq!(&tiff[tiff.len() - PREVIEW.len()..], PREVIEW, "preview last");
    (tiff, preview_at)
}

/// The Pentax shape under every block-shrinking group removal the surgical
/// writers support. With the maker note kept (`IFD1:All`, `InteropIFD:All`,
/// `GPS:All`) the preview is still where the note's pointers say, byte for
/// byte, by a structural walk and by the reader; the edit is not refused.
/// `ExifIFD:All` deletes the maker note with ExifIFD (as pinned ExifTool
/// 13.59 does) and `IFD0:All` the whole block: no preview is read after.
/// Red at #943's head e4f512d4: the compact re-layout of the shortened block
/// cut the preview off (`IFD1:All` read back `MakerNotes:PreviewImage`
/// changed; the PNG oracle warned "PreviewImageStart is past end of file").
#[test]
fn a_preview_at_the_end_of_the_block_survives_every_shrinking_group_removal() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Mm, Order::Ii] {
        let (tiff, preview_at) = pentax_shaped_block(order);
        assert_eq!(casio_preview(&tiff), PREVIEW);
        for (carrier, file) in [("jpg", jpeg_with(&tiff)), ("png", png_with(&tiff))] {
            let original = write(dir.path(), &format!("pentax.{carrier}"), &file);
            let previews = reader_previews(&original);
            assert!(
                previews
                    .iter()
                    .any(|(_, v)| *v == TagValue::Binary(PREVIEW.to_vec())),
                "{order:?} {carrier}: the reader must see the preview first: {previews:?}"
            );
            for group in ["IFD1:All", "InteropIFD:All", "GPS:All"] {
                let path = write(dir.path(), &format!("shrink.{carrier}"), &file);
                remove_tag(&path, group).unwrap_or_else(|e| {
                    panic!("{order:?} {carrier} {group}: the preview can be kept: {e}")
                });
                let out = std::fs::read(&path).unwrap();
                let block = tiff_of(&out);
                assert_ne!(block, tiff, "{order:?} {carrier} {group}: nothing removed");
                assert_eq!(
                    block.get(preview_at..preview_at + PREVIEW.len()),
                    Some(PREVIEW),
                    "{order:?} {carrier} {group}: preview lost or mis-pointed (block now {} bytes)",
                    block.len()
                );
                assert_eq!(
                    casio_preview(&block),
                    PREVIEW,
                    "{order:?} {carrier} {group}: the note's PreviewImage no longer locates it"
                );
                assert_eq!(
                    reader_previews(&path),
                    previews,
                    "{order:?} {carrier} {group}: reader's PreviewImage changed"
                );
            }
            for group in ["ExifIFD:All", "IFD0:All"] {
                let path = write(dir.path(), &format!("drop.{carrier}"), &file);
                remove_tag(&path, group).unwrap_or_else(|e| {
                    panic!("{order:?} {carrier} {group}: deletion refused: {e}")
                });
                assert!(
                    reader_previews(&path).is_empty(),
                    "{order:?} {carrier} {group}: the maker note's preview is still read"
                );
            }
        }
    }
}

/// A maker note no decoder reads, in its own byte order (little-endian
/// inside a big-endian block, as Panasonic and `SONY PI` notes are), whose
/// one value starts inside the note and runs on through IFD1's table --
/// the corpus's SonyDSC-S1900 CameraParameters (12,000 bytes, 42 past the
/// note) and PanasonicDMC-FT20 BabyAge have this shape. `-IFD1:All=`
/// deletes the table; the bytes the note addresses must stay (pinned
/// ExifTool 13.59 copies them into its rebuilt note, WriteExif.pl
/// 858-960), and nothing but a byte check can see them. Red at 07698fb5:
/// the write succeeded and zeroed them.
#[test]
fn bytes_a_note_addresses_in_a_deleted_table_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let order = Order::Mm;
    let le16 = |v: u16| v.to_le_bytes();
    let le32 = |v: u32| v.to_le_bytes();
    // header | IFD0 {Make, ExifIFD} @8 | "Acme\0" | ExifIFD {MakerNote} |
    // note | IFD1 {Compression, thumbnail pair} | thumbnail
    let make = ascii("Acme");
    let ifd0_at = 8usize;
    let make_at = ifd0_at + 2 + 12 * 2 + 4;
    let exif_at = make_at + make.len() + make.len() % 2;
    let note_at = exif_at + 2 + 12 + 4;
    let note_ifd = 6usize;
    let note_len = note_ifd + 2 + 12 + 4 + 10; // 10 value bytes inside
    let value_at = note_at + note_ifd + 2 + 12 + 4;
    let ifd1_at = note_at + note_len;
    let value_len = (ifd1_at + 20) - value_at; // runs 20 bytes into IFD1's table
    let thumb: &[u8] = &[0xFF, 0xD8, 0xFF, 0xD9];
    let thumb_at = ifd1_at + 2 + 12 * 3 + 4;
    let mut t = order.mark().to_vec();
    t.extend(order.u16(42));
    t.extend(order.u32(ifd0_at as u32));
    t.extend(order.u16(2));
    t.extend([order.u16(0x010F), order.u16(2)].concat());
    t.extend(order.u32(make.len() as u32));
    t.extend(order.u32(make_at as u32));
    t.extend([order.u16(0x8769), order.u16(4)].concat());
    t.extend(order.u32(1));
    t.extend(order.u32(exif_at as u32));
    t.extend(order.u32(ifd1_at as u32));
    t.extend(&make);
    t.resize(exif_at, 0);
    t.extend(order.u16(1));
    t.extend([order.u16(0x927C), order.u16(7)].concat());
    t.extend(order.u32(note_len as u32));
    t.extend(order.u32(note_at as u32));
    t.extend(order.u32(0));
    assert_eq!(t.len(), note_at);
    t.extend(b"ACME\0\0");
    t.extend(le16(1));
    t.extend([le16(0x0001), le16(7)].concat());
    t.extend(le32(value_len as u32));
    t.extend(le32(value_at as u32)); // TIFF-relative
    t.extend(le32(0));
    t.extend(b"0123456789");
    assert_eq!(t.len(), ifd1_at);
    t.extend(order.u16(3));
    t.extend([order.u16(0x0103), order.u16(3)].concat());
    t.extend(order.u32(1));
    t.extend([order.u16(6).as_slice(), &[0x5A, 0xA5]].concat());
    t.extend([order.u16(0x0201), order.u16(4)].concat());
    t.extend(order.u32(1));
    t.extend(order.u32(thumb_at as u32));
    t.extend([order.u16(0x0202), order.u16(4)].concat());
    t.extend(order.u32(1));
    t.extend(order.u32(thumb.len() as u32));
    t.extend(order.u32(0));
    t.extend(thumb);
    let addressed = t[value_at..value_at + value_len].to_vec();
    for (carrier, file) in [("jpg", jpeg_with(&t)), ("png", png_with(&t))] {
        let path = write(dir.path(), &format!("borrow-ifd1.{carrier}"), &file);
        remove_tag(&path, "IFD1:All")
            .unwrap_or_else(|e| panic!("{carrier}: the addressed bytes can be kept: {e}"));
        let out = tiff_of(&std::fs::read(&path).unwrap());
        assert_eq!(
            out.get(value_at..value_at + value_len),
            Some(addressed.as_slice()),
            "{carrier}: bytes the note addresses were overwritten"
        );
        assert_eq!(
            &out[note_at..note_at + note_len],
            &t[note_at..note_at + note_len]
        );
    }
}

/// An OriginalDecisionData block, version 3 (`ReadODD`, Canon.pm
/// 13.59:10368-10460): `ff ff ff ff`, the version, then three
/// length-prefixed records, the third length counting its own word.
fn odd_block() -> Vec<u8> {
    let mut b = vec![0xFF; 4];
    b.extend(3u32.to_le_bytes());
    for (len, body) in [(4u32, *b"odd1"), (4, *b"odd2"), (8, *b"odd3")] {
        b.extend(len.to_le_bytes());
        b.extend(body);
    }
    b
}

/// A little-endian Canon EXIF block whose note (IFD at 0, TIFF-relative
/// offsets) holds an `OriginalDecisionDataOffset` (0x0083) -- a FILE offset
/// in a JPEG (Canon.pm 13.59:1786-1795: `IsOffset` only when `FILE_TYPE ne
/// "JPEG"`), with no length tag.
fn canon_odd_block(odd_file_offset: u32) -> Vec<u8> {
    let order = Order::Ii;
    let make = ascii("Canon");
    let model = ascii("Canon EOS-1D Mark III");
    let ifd0_entries = |exif_at: u32| {
        vec![
            entry(0x010F, 2, make.len() as u32, make.clone()),
            entry(0x0110, 2, model.len() as u32, model.clone()),
            entry(0x0131, 2, 46, ascii(SOFTWARE)),
            entry(0x8769, 4, 1, order.u32(exif_at).to_vec()),
        ]
    };
    let exif_at = 8 + ifd(order, 8, &ifd0_entries(0), &[]).len();
    let note_at = exif_at + 2 + 12 * 2 + 4 + 8;
    let note = ifd(
        order,
        note_at,
        &[
            entry(0x0006, 2, 13, ascii("IMG:EOS-1D M3")),
            entry(0x0083, 4, 1, order.u32(odd_file_offset).to_vec()),
        ],
        &[],
    );
    let mut tiff = order.mark().to_vec();
    tiff.extend(order.u16(42));
    tiff.extend(order.u32(8));
    tiff.extend(ifd(order, 8, &ifd0_entries(exif_at as u32), &[]));
    tiff.extend(ifd(
        order,
        exif_at,
        &[
            entry(0x829A, 5, 1, [order.u32(1), order.u32(60)].concat()),
            entry(0x927C, 7, note.len() as u32, note),
        ],
        &[],
    ));
    tiff
}

/// P1 "preserve unpaired OriginalDecisionData targets": the ODD block after
/// the image, located by the note's FILE offset. A growing JPEG edit moves
/// it; the pinned note still holds the old offset. The write must keep the
/// block where the offset says or be refused untouched. Red at 6e14d505:
/// the offset was checked by nothing, the write succeeded, and the block
/// read back as nothing (`Composite:OriginalDecisionData` gone).
#[test]
fn an_original_decision_data_block_after_the_image_is_never_shifted_away() {
    let dir = tempfile::tempdir().unwrap();
    let odd = odd_block();
    let probe = jpeg_with(&canon_odd_block(0));
    let odd_at = probe.len() as u32;
    let mut file = jpeg_with(&canon_odd_block(odd_at));
    assert_eq!(file.len(), probe.len());
    file.extend(&odd);
    let original = write(dir.path(), "odd.jpg", &file);
    let map = read_metadata(&original).unwrap();
    assert_eq!(
        map.get("Composite:OriginalDecisionData"),
        Some(&TagValue::Binary(odd.clone())),
        "the reader must see the ODD block first: {:?}",
        map.keys()
            .filter(|k| k.contains("Decision"))
            .collect::<Vec<_>>()
    );
    for edit in [Edit::Grow, Edit::Shrink, Edit::Same] {
        let path = write(dir.path(), &format!("odd-{edit:?}.jpg"), &file);
        match apply(&path, edit) {
            Ok(()) => {
                let out = std::fs::read(&path).unwrap();
                assert_eq!(
                    out.get(odd_at as usize..odd_at as usize + odd.len()),
                    Some(odd.as_slice()),
                    "{edit:?}: the ODD block moved away from its offset"
                );
                assert_eq!(
                    read_metadata(&path)
                        .unwrap()
                        .get("Composite:OriginalDecisionData"),
                    Some(&TagValue::Binary(odd.clone())),
                    "{edit:?}"
                );
            }
            Err(e) => {
                assert!(
                    e.to_string().contains("OriginalDecisionData"),
                    "{edit:?}: {e}"
                );
                assert_eq!(std::fs::read(&path).unwrap(), file, "refused but modified");
            }
        }
    }
}

/// The real Canon1DmkIII sample (its ODD block lies inside the note, at a
/// FILE offset): every edit keeps `Composite:OriginalDecisionData`.
#[test]
fn canon_1d_mark_iii_keeps_its_original_decision_data() {
    let Some(sample) = fixtures::pinned_t_images_fixture_path("Canon1DmkIII.jpg") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let original = std::fs::read(&sample).unwrap();
    let odd = read_metadata(&sample)
        .unwrap()
        .get("Composite:OriginalDecisionData")
        .cloned();
    assert!(odd.is_some(), "Canon1DmkIII.jpg has an ODD block");
    for edit in [Edit::Grow, Edit::Shrink, Edit::Same] {
        let path = write(dir.path(), &format!("1dm3-{edit:?}.jpg"), &original);
        apply(&path, edit).unwrap_or_else(|e| panic!("{edit:?}: {e}"));
        assert_eq!(
            read_metadata(&path)
                .unwrap()
                .get("Composite:OriginalDecisionData")
                .cloned(),
            odd,
            "{edit:?}"
        );
    }
}

/// P1 "pin MakerNotes stored directly in top-level IFDs": a Casio Type2
/// note in IFD0 (0x927C there, as the reader accepts it), its preview in a
/// hole. The serializer pins only ExifIFD's note, so a re-layout moved this
/// one and its TIFF-relative pointers dangled. Kept exactly or refused
/// untouched. Red at 6e14d505 (moved, reported success).
#[test]
fn a_maker_note_in_ifd0_is_kept_in_place_or_refused() {
    let dir = tempfile::tempdir().unwrap();
    let order = Order::Mm;
    let make = ascii("CASIO COMPUTER CO.,LTD");
    let software = ascii(SOFTWARE);
    let note_len = 6 + 2 + 12 * 2 + 4;
    // IFD0 {Make, Software, ModifyDate, MakerNote} @8, values, note, hole
    let ifd0_len = ifd(
        order,
        8,
        &[
            entry(0x010F, 2, make.len() as u32, make.clone()),
            entry(0x0131, 2, software.len() as u32, software.clone()),
            entry(0x0132, 2, 20, ascii("2003:02:28 09:45:00")),
            entry(0x927C, 7, note_len as u32, vec![0; note_len]),
        ],
        &[],
    )
    .len();
    let note_at = 8 + ifd0_len - note_len - note_len % 2;
    let preview_at = 8 + ifd0_len + 2;
    let mut note = b"QVC\0\0\0".to_vec();
    note.extend(ifd(
        order,
        note_at + 6,
        &[
            entry(0x0002, 3, 2, [order.u16(320), order.u16(240)].concat()),
            entry(0x2000, 7, PREVIEW.len() as u32, vec![0; PREVIEW.len()]),
        ],
        &[(0x2000, preview_at as u32)],
    ));
    assert_eq!(note.len(), note_len);
    let mut tiff = order.mark().to_vec();
    tiff.extend(order.u16(42));
    tiff.extend(order.u32(8));
    tiff.extend(ifd(
        order,
        8,
        &[
            entry(0x010F, 2, make.len() as u32, make.clone()),
            entry(0x0131, 2, software.len() as u32, software.clone()),
            entry(0x0132, 2, 20, ascii("2003:02:28 09:45:00")),
            entry(0x927C, 7, note_len as u32, note.clone()),
        ],
        &[],
    ));
    assert_eq!(
        &tiff[note_at..note_at + note_len],
        note.as_slice(),
        "note placed"
    );
    tiff.resize(preview_at, 0);
    tiff.extend(PREVIEW);
    for (carrier, file) in [("jpg", jpeg_with(&tiff)), ("png", png_with(&tiff))] {
        for edit in [Edit::Grow, Edit::Shrink, Edit::Same] {
            let path = write(dir.path(), &format!("ifd0-note-{edit:?}.{carrier}"), &file);
            match apply(&path, edit) {
                Ok(()) => {
                    let out = tiff_of(&std::fs::read(&path).unwrap());
                    assert_eq!(
                        out.get(note_at..note_at + note_len),
                        Some(note.as_slice()),
                        "{carrier} {edit:?}: the IFD0 MakerNote moved"
                    );
                    assert_eq!(
                        out.get(preview_at..preview_at + PREVIEW.len()),
                        Some(PREVIEW),
                        "{carrier} {edit:?}: its preview was overwritten"
                    );
                }
                Err(e) => {
                    assert!(
                        e.to_string().contains("MakerNote"),
                        "{carrier} {edit:?}: {e}"
                    );
                    assert_eq!(std::fs::read(&path).unwrap(), file, "refused but modified");
                }
            }
        }
    }
}

/// P1 "check every EXIF block before moving later maker notes": a JPEG
/// with two EXIF APP1 blocks, the second a Canon block whose note locates an
/// ODD block after the image by FILE offset. Editing the first block moves
/// everything after it. Every block is verified: kept or refused untouched.
/// Red at 6e14d505 (only the first block was looked at; the ODD offset of
/// the second went stale and the write reported success).
#[test]
fn every_exif_block_of_a_jpeg_is_verified() {
    let dir = tempfile::tempdir().unwrap();
    let odd = odd_block();
    let first = casio_block(Order::Ii, Target::Hole).0;
    let app1 = |tiff: &[u8]| {
        let mut s = vec![0xFF, 0xE1];
        s.extend(((tiff.len() + 8) as u16).to_be_bytes());
        s.extend(b"Exif\0\0");
        s.extend(tiff);
        s
    };
    let build = |odd_at: u32| {
        let mut f = vec![0xFF, 0xD8];
        f.extend(app1(&first));
        f.extend(app1(&canon_odd_block(odd_at)));
        f.extend(&jpeg_body()[..]);
        f
    };
    let odd_at = build(0).len() as u32;
    let mut file = build(odd_at);
    file.extend(&odd);
    for edit in [Edit::Grow, Edit::Shrink, Edit::Same] {
        let path = write(dir.path(), &format!("two-exif-{edit:?}.jpg"), &file);
        match apply(&path, edit) {
            Ok(()) => {
                let out = std::fs::read(&path).unwrap();
                assert_eq!(
                    out.get(odd_at as usize..odd_at as usize + odd.len()),
                    Some(odd.as_slice()),
                    "{edit:?}: the second block's ODD block moved away from its offset"
                );
            }
            Err(e) => {
                assert_eq!(
                    std::fs::read(&path).unwrap(),
                    file,
                    "{edit:?}: refused but modified ({e})"
                );
            }
        }
    }
}

/// Every physical MakerNote (0x927C) entry of `tiff`'s ExifIFD, in table
/// order, by a structural walk independent of the writer.
fn exif_makernotes(tiff: &[u8]) -> Vec<Vec<u8>> {
    let order = if &tiff[..2] == b"II" {
        Order::Ii
    } else {
        Order::Mm
    };
    let rows = |at: usize| {
        let n = order.read_u16(&tiff[at..]) as usize;
        (0..n).map(move |i| at + 2 + 12 * i)
    };
    let ifd0 = order.read_u32(&tiff[4..]) as usize;
    let Some(exif) = rows(ifd0)
        .find(|e| order.read_u16(&tiff[*e..]) == 0x8769)
        .map(|e| order.read_u32(&tiff[e + 8..]) as usize)
    else {
        return Vec::new();
    };
    rows(exif)
        .filter(|e| order.read_u16(&tiff[*e..]) == 0x927C)
        .map(|e| {
            let count = order.read_u32(&tiff[e + 4..]) as usize;
            let at = order.read_u32(&tiff[e + 8..]) as usize;
            tiff[at..at + count].to_vec()
        })
        .collect()
}

/// The TIFF offset of every physical MakerNote value in `tiff`'s ExifIFD.
fn exif_makernote_offsets(tiff: &[u8]) -> Vec<u32> {
    let order = if &tiff[..2] == b"II" {
        Order::Ii
    } else {
        Order::Mm
    };
    let rows = |at: usize| {
        let n = order.read_u16(&tiff[at..]) as usize;
        (0..n).map(move |i| at + 2 + 12 * i)
    };
    let ifd0 = order.read_u32(&tiff[4..]) as usize;
    let exif = rows(ifd0)
        .find(|e| order.read_u16(&tiff[*e..]) == 0x8769)
        .map(|e| order.read_u32(&tiff[e + 8..]) as usize)
        .expect("ExifIFD");
    rows(exif)
        .filter(|e| order.read_u16(&tiff[*e..]) == 0x927C)
        .map(|e| order.read_u32(&tiff[e + 8..]))
        .collect()
}

/// An EXIF block whose ExifIFD holds two physical MakerNote entries, as
/// `Apple_iPhone6.jpg` does (its second, 142-byte note is an editing app's;
/// pinned ExifTool 13.59 `-v3` lists both, warning "Duplicate tag 0x927c").
fn two_note_block(order: Order) -> (Vec<u8>, [Vec<u8>; 2]) {
    let make = ascii("Apple");
    let model = ascii("iPhone 6");
    let software = ascii(SOFTWARE);
    let ifd0_entries = |exif_at: u32| {
        vec![
            entry(0x010F, 2, make.len() as u32, make.clone()),
            entry(0x0110, 2, model.len() as u32, model.clone()),
            entry(0x0131, 2, software.len() as u32, software.clone()),
            entry(0x0132, 2, 20, ascii("2003:02:28 09:45:00")),
            entry(0x8769, 4, 1, order.u32(exif_at).to_vec()),
        ]
    };
    let exif_at = 8 + ifd(order, 8, &ifd0_entries(0), &[]).len();
    let first = b"<first physical maker note, 40 bytes..>".to_vec();
    let second = b"<second note: an editing app's>".to_vec();
    let mut tiff = order.mark().to_vec();
    tiff.extend(order.u16(42));
    tiff.extend(order.u32(8));
    tiff.extend(ifd(order, 8, &ifd0_entries(exif_at as u32), &[]));
    tiff.extend(ifd(
        order,
        exif_at,
        &[
            entry(0x829A, 5, 1, [order.u32(1), order.u32(60)].concat()),
            entry(0x927C, 7, first.len() as u32, first.clone()),
            entry(0x927C, 7, second.len() as u32, second.clone()),
        ],
        &[],
    ));
    assert_eq!(exif_makernotes(&tiff), [first.clone(), second.clone()]);
    (tiff, [first, second])
}

/// P1 "preserve every duplicate MakerNote entry": an ExifIFD with two
/// physical 0x927C entries. The serializer kept one entry per tag id, so a
/// re-laying edit dropped the second note while the guard, comparing only
/// the first, passed it. Pinned ExifTool 13.59 performs these edits and
/// keeps both notes (graded below on the same file); so does oxidex now:
/// every edit succeeds with BOTH notes byte-identical at their original
/// offsets. Red at af8e2b86 (second note silently deleted, success
/// reported) and at 3028e284 (the edit refused).
#[test]
fn every_physical_maker_note_is_kept_as_exiftool_keeps_it() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let (tiff, notes) = two_note_block(order);
        let placed = exif_makernote_offsets(&tiff);
        for (carrier, file) in [("jpg", jpeg_with(&tiff)), ("png", png_with(&tiff))] {
            for edit in [Edit::Grow, Edit::Shrink, Edit::Same] {
                let name = format!("two-notes-{order:?}-{edit:?}.{carrier}");
                let path = write(dir.path(), &name, &file);
                if let Err(e) = apply(&path, edit) {
                    panic!("{name}: pinned ExifTool performs this edit; oxidex refused: {e}");
                }
                let out = tiff_of(&std::fs::read(&path).unwrap());
                assert_ne!(out, tiff, "{name}: the edit was not applied");
                assert_eq!(
                    exif_makernotes(&out),
                    notes,
                    "{name}: a physical MakerNote was dropped or changed"
                );
                assert_eq!(
                    exif_makernote_offsets(&out),
                    placed,
                    "{name}: a MakerNote moved"
                );
            }
        }
    }

    // The oracle's behaviour on the same layout: both notes survive an edit.
    let Some(oracle) = oxidex::exiftool_oracle::graded() else {
        return;
    };
    let (tiff, notes) = two_note_block(Order::Mm);
    let src = write(dir.path(), "two-notes-oracle.jpg", &jpeg_with(&tiff));
    let out = dir.path().join("two-notes-oracle-out.jpg");
    let status = oracle
        .command()
        .args(["-q", "-q", "-IFD0:ModifyDate=2001:02:03 04:05:06", "-o"])
        .arg(&out)
        .arg(&src)
        .status()
        .unwrap();
    assert!(status.success(), "pinned ExifTool failed the edit");
    assert_eq!(
        exif_makernotes(&tiff_of(&std::fs::read(&out).unwrap())),
        notes,
        "pinned ExifTool 13.59 keeps every physical MakerNote"
    );
}

/// The CLI on the two-note layout: a grouped edit is performed and reported,
/// with both notes kept in place, as pinned ExifTool 13.59 does (its
/// `-IFD0:ImageDescription=x` writes and keeps both). Red at 3028e284 (the
/// edit refused, exit 1). A bare `-ImageDescription=x` is not used: bare
/// names are routed by the CLI write-transaction work (#945), not here.
#[test]
fn cli_edit_of_a_two_note_jpeg_is_written_with_both_notes() {
    let dir = tempfile::tempdir().unwrap();
    let (tiff, notes) = two_note_block(Order::Mm);
    let placed = exif_makernote_offsets(&tiff);
    let file = jpeg_with(&tiff);
    let path = write(dir.path(), "cli-two-notes.jpg", &file);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-IFD0:ImageDescription=cli edit"])
        .arg(&path)
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        out.status.success(),
        "exit {:?}; stderr: {stderr}",
        out.status
    );
    assert!(stdout.contains("1 image files updated"), "stdout: {stdout}");
    let written = tiff_of(&std::fs::read(&path).unwrap());
    assert_eq!(exif_makernotes(&written), notes, "a MakerNote was lost");
    assert_eq!(
        exif_makernote_offsets(&written),
        placed,
        "a MakerNote moved"
    );
    let map = read_metadata(&path).unwrap();
    assert_eq!(
        map.get("IFD0:ImageDescription"),
        Some(&TagValue::new_string("cli edit")),
        "the edit was not stored"
    );
}

/// A refusal the guard raises reaches the CLI as a failure: an error naming
/// the edit, no "updated" report, a non-zero exit and the file untouched.
/// The second note sits at an odd offset, where this writer does not pin a
/// value (ExifTool warns "Odd offset" for one), so keeping it in place is
/// not possible and the edit is refused rather than moving or dropping it.
#[test]
fn cli_reports_a_maker_note_refusal_as_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let order = Order::Mm;
    let (mut tiff, [_, second]) = two_note_block(order);
    // move the second note's value to an odd offset past the block
    if tiff.len() % 2 == 0 {
        tiff.push(0);
    }
    let odd_at = tiff.len() as u32;
    tiff.extend(&second);
    let ifd0 = order.read_u32(&tiff[4..]) as usize;
    let rows = |t: &[u8], at: usize| {
        let n = order.read_u16(&t[at..]) as usize;
        (0..n).map(move |i| at + 2 + 12 * i).collect::<Vec<_>>()
    };
    let exif = rows(&tiff, ifd0)
        .into_iter()
        .find(|e| order.read_u16(&tiff[*e..]) == 0x8769)
        .map(|e| order.read_u32(&tiff[e + 8..]) as usize)
        .unwrap();
    let second_entry = rows(&tiff, exif)
        .into_iter()
        .filter(|e| order.read_u16(&tiff[*e..]) == 0x927C)
        .nth(1)
        .unwrap();
    tiff[second_entry + 8..second_entry + 12].copy_from_slice(&order.u32(odd_at));
    assert_eq!(exif_makernote_offsets(&tiff)[1] % 2, 1);
    let file = jpeg_with(&tiff);
    let path = write(dir.path(), "cli-odd-note.jpg", &file);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["-IFD0:ImageDescription=cli edit"])
        .arg(&path)
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(!out.status.success(), "reported success; stdout: {stdout}");
    assert!(
        stderr.contains("Error:")
            && stderr.contains("IFD0:ImageDescription")
            && stderr.contains("MakerNote"),
        "stderr: {stderr}"
    );
    assert!(
        !stdout.contains("1 image files updated"),
        "stdout: {stdout}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), file, "refused but modified");
}
