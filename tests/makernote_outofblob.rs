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
