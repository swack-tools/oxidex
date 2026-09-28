//! A directory chain past IFD1 -- IFD2, a Leica JPEG's PreviewImage IFD --
//! is carried through every EXIF write that keeps it, its preview re-pointed
//! as pinned ExifTool 13.59 re-points it.
//!
//! Before (#943 at 53f3f0da): the planner refused to re-lay out a block whose
//! IFD1 links on, and a JPEG write that changed the APP1's length was refused
//! when IFD2 located a preview after the image. So on a Leica-layout file a
//! legacy deletion or set, a mixed write and any length-changing edit were
//! all refused, while the oracle keeps the chain on each:
//!
//! * data IFD2 locates inside the block moves with the re-laid-out block
//!   (`WriteExif.pl` 13.59:2466-2468);
//! * in a JPEG, IFD2's PreviewImageStart outside the block is re-pointed on
//!   every rewrite (`$$et{PREVIEW_INFO}`, `WriteExif.pl` 13.59:2513-2547,
//!   2657-2685; `Writer.pl` 13.59:6177-6245): at the same preview bytes when
//!   ExifTool can load them, else at the end of the image's EOI (plus junk
//!   before a JPEG header) -- which is where a Leica preview sits, and where
//!   ExifTool points the preview of its own truncated Leica samples;
//! * any other chain data outside the block is refused by the oracle
//!   ("Error reading ... data in IFD2") and so by oxidex.
//!
//! Every case runs the CLI (`CARGO_BIN_EXE_oxidex`) and the pinned oracle
//! on copies of one input and compares the oracle's read-back of both
//! outputs -- every IFD0, IFD1, IFD2, ExifIFD and MakerNotes tag, and the
//! PreviewImage and ThumbnailImage bytes -- less the offsets a different
//! layout moves (PreviewImageStart is then checked by what it locates), and
//! asserts `-validate` parity with the oracle's own edit. Skipped without a
//! usable oracle.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::exiftool_oracle;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    fn tag(self) -> &'static [u8] {
        match self {
            Order::Ii => b"II",
            Order::Mm => b"MM",
        }
    }
}

/// An 8x8 baseline JPEG with no metadata segment (DQT, SOF0, DHT, SOS, EOI).
const BASE_JPEG_HEX: &str = "ffd8ffdb0084001410101912192717172732261f26322e262626262e3e35353535353e44414141414141444444444444444444444444444444444444444444444444444444444401151919201c2026181826362620263644362b2b364444444235424444444444444444444444444444444444444444444444444444444444444444444444444444ffc00011080008000803012200021101031101ffc4004b00010100000000000000000000000000000006010100000000000000000000000000000000100100000000000000000000000000000000110100000000000000000000000000000000ffda000c03010002110311003f00b3001fffd9";

fn base_jpeg() -> Vec<u8> {
    (0..BASE_JPEG_HEX.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&BASE_JPEG_HEX[i..i + 2], 16).unwrap())
        .collect()
}

/// Where the fixture's IFD2 preview lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Preview {
    /// Inside the EXIF block.
    InBlock,
    /// After the JPEG's EOI, a real JPEG ExifTool loads (`\xff\xd8\xff\xdb`).
    Trailer,
    /// After the EOI, bytes ExifTool does not take for a JPEG
    /// (`\xff\xd8\xff\xfe`): it `LOAD_PREVIEW`s and points at the EOI's end.
    TrailerUnloadable,
    /// Past the end of the file (a truncated sample).
    Truncated,
}

/// A Leica-layout EXIF block: IFD0 {Make, Model, Artist, ExifIFD} ->
/// ExifIFD {ISO 100} ; IFD1 {Compression, XResolution, thumbnail} -> IFD2
/// {ImageWidth, ImageHeight, Compression 7, StripOffsets, StripByteCounts},
/// the strip (PreviewImage in a JPEG APP1) at `preview_at`, or appended
/// inside the block when `None`.
fn leica_tiff(order: Order, preview: &[u8], preview_at: Option<u32>) -> Vec<u8> {
    let o = order;
    let entry = |tag: u16, typ: u16, count: u32, field: [u8; 4]| {
        [o.u16(tag).as_slice(), &o.u16(typ), &o.u32(count), &field].concat()
    };
    let short = |v: u16| {
        let mut f = [0u8; 4];
        f[..2].copy_from_slice(&o.u16(v));
        f
    };
    let thumb = base_jpeg();
    // Layout: header 8 | IFD0 4 rows @8 (ends 62) | Make @62 (6) | Model @68
    // (8) | ExifIFD 1 row @76 (ends 94) | IFD1 4 rows @94 (ends 148) |
    // XResolution @148 | thumbnail @156 | IFD2 5 rows @ifd2 | preview.
    let (ifd0, make, model, exif, ifd1, xres, thumb_at) = (8u32, 62u32, 68, 76, 94, 148, 156);
    let ifd2 = thumb_at + thumb.len() as u32 + (thumb.len() as u32 & 1);
    let in_block = ifd2 + 2 + 5 * 12 + 4;
    let mut t = o.tag().to_vec();
    t.extend(o.u16(42));
    t.extend(o.u32(ifd0));
    t.extend(o.u16(4));
    t.extend(entry(0x010F, 2, 6, o.u32(make)));
    t.extend(entry(0x0110, 2, 8, o.u32(model)));
    t.extend(entry(0x013B, 2, 3, *b"me\0\0"));
    t.extend(entry(0x8769, 4, 1, o.u32(exif)));
    t.extend(o.u32(ifd1));
    t.extend(b"LEICA\0CL-TEST\0");
    assert_eq!(t.len(), exif as usize);
    t.extend(o.u16(1));
    t.extend(entry(0x8827, 3, 1, short(100)));
    t.extend(o.u32(0));
    assert_eq!(t.len(), ifd1 as usize);
    t.extend(o.u16(4));
    t.extend(entry(0x0103, 3, 1, short(6)));
    t.extend(entry(0x011A, 5, 1, o.u32(xres)));
    t.extend(entry(0x0201, 4, 1, o.u32(thumb_at)));
    t.extend(entry(0x0202, 4, 1, o.u32(thumb.len() as u32)));
    t.extend(o.u32(ifd2));
    t.extend(o.u32(72));
    t.extend(o.u32(1));
    t.extend(&thumb);
    if thumb.len() % 2 == 1 {
        t.push(0);
    }
    assert_eq!(t.len(), ifd2 as usize);
    t.extend(o.u16(5));
    t.extend(entry(0x0100, 4, 1, o.u32(8)));
    t.extend(entry(0x0101, 4, 1, o.u32(8)));
    t.extend(entry(0x0103, 3, 1, short(7)));
    t.extend(entry(0x0111, 4, 1, o.u32(preview_at.unwrap_or(in_block))));
    t.extend(entry(0x0117, 4, 1, o.u32(preview.len() as u32)));
    t.extend(o.u32(0));
    assert_eq!(t.len(), in_block as usize);
    if preview_at.is_none() {
        t.extend(preview);
    }
    t
}

/// The base JPEG with `tiff` as its EXIF APP1 (TIFF header at offset 12).
fn jpeg_with(tiff: &[u8]) -> Vec<u8> {
    let base = base_jpeg();
    let mut out = base[..2].to_vec();
    out.extend([0xFF, 0xE1]);
    out.extend(((tiff.len() + 8) as u16).to_be_bytes());
    out.extend(b"Exif\0\0");
    out.extend(tiff);
    out.extend(&base[2..]);
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
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

fn png_with(tiff: &[u8]) -> Vec<u8> {
    let chunk = |kind: &[u8; 4], data: &[u8]| {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend(kind);
        out.extend(data);
        out.extend(crc32(&[kind.as_slice(), data].concat()).to_be_bytes());
        out
    };
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]));
    out.extend(chunk(b"eXIf", tiff));
    out.extend(chunk(
        b"IDAT",
        &[0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01],
    ));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// A real JPEG preview (ExifTool loads it) and one it does not take for a
/// JPEG (`\xfe` after the SOI's `\xff`).
fn loadable_preview() -> Vec<u8> {
    base_jpeg()
}
fn unloadable_preview() -> Vec<u8> {
    b"\xFF\xD8\xFF\xFEpreview-data\xFF\xD9".to_vec()
}

/// The fixture file for `preview` placement in a JPEG.
fn leica_jpeg(order: Order, preview: Preview) -> Vec<u8> {
    let bytes = match preview {
        Preview::TrailerUnloadable => unloadable_preview(),
        _ => loadable_preview(),
    };
    match preview {
        Preview::InBlock => jpeg_with(&leica_tiff(order, &bytes, None)),
        Preview::Truncated => jpeg_with(&leica_tiff(order, &bytes, Some(7_000_000))),
        Preview::Trailer | Preview::TrailerUnloadable => {
            let probe = jpeg_with(&leica_tiff(order, &bytes, Some(0)));
            let file = jpeg_with(&leica_tiff(order, &bytes, Some(probe.len() as u32 - 12)));
            [file, bytes].concat()
        }
    }
}

const LONG_ARTIST: &str =
    "-IFD0:Artist=a much longer artist name that cannot fit where the old one was";

/// The edit classes, each one CLI invocation (and one oracle invocation).
const EDITS: [(&str, &[&str]); 6] = [
    ("legacy delete", &["-IFD0:Model="]),
    ("legacy set", &["-ExifIFD:ISO=321"]),
    ("mixed", &["-IFD0:Model=", "-IFD0:Artist=you"]),
    ("length-changing generated", &[LONG_ARTIST]),
    ("IFD1:All", &["-IFD1:All="]),
    ("EXIF:All", &["-EXIF:All="]),
];

fn oracle() -> Option<&'static exiftool_oracle::Oracle> {
    if !exiftool_oracle::available() {
        return None;
    }
    exiftool_oracle::shared().ok()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn oxidex(args: &[&str], path: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .arg(path)
        .output()
        .unwrap()
}

/// Tags whose values are offsets a different layout moves: compared by what
/// they locate instead (the PreviewImage and ThumbnailImage bytes, the
/// sub-directories' contents).
const LAYOUT_OFFSETS: [&str; 4] = [
    "IFD0:ExifOffset",
    "IFD1:ThumbnailOffset",
    "IFD2:PreviewImageStart",
    "IFD2:StripOffsets",
];

/// The oracle's read-back of `path`: every IFD0, IFD1, IFD2, ExifIFD and
/// MakerNotes tag, binary values included (`-b`: base64 under `-j`), less
/// [`LAYOUT_OFFSETS`].
fn readback(oracle: &exiftool_oracle::Oracle, path: &Path) -> BTreeMap<String, String> {
    let out = oracle
        .command()
        .args(["-j", "-a", "-G1", "-b", "-n"])
        .args([
            "-IFD0:all",
            "-IFD1:all",
            "-IFD2:all",
            "-ExifIFD:all",
            "-MakerNotes:all",
        ])
        .arg(path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("oracle read-back of {}: {e}: {text}", path.display()));
    json[0]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| *k != "SourceFile" && !LAYOUT_OFFSETS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.to_string()))
        .collect()
}

/// `-validate` warnings of `path`, less the summary line.
fn validate(oracle: &exiftool_oracle::Oracle, path: &Path) -> BTreeSet<String> {
    let out = oracle
        .command()
        .args(["-a", "-s3", "-validate", "-Warning"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| {
            !line.split_once(' ').is_some_and(|(n, rest)| {
                n.bytes().all(|b| b.is_ascii_digit()) && rest.starts_with("Warning")
            })
        })
        .map(str::to_string)
        .collect()
}

/// The TIFF header offset and IFD2 `(PreviewImageStart, PreviewImageLength)`
/// of a JPEG, and where its image's EOI ends (the first `\xff\xd9` after the
/// SOS header).
fn jpeg_preview_pointer(file: &[u8]) -> Option<(usize, usize, usize, usize)> {
    let mut i = 2;
    let (mut header, mut sos_end) = (None, None);
    while i + 4 <= file.len() && file[i] == 0xFF {
        let len = u16::from_be_bytes([file[i + 2], file[i + 3]]) as usize;
        if file[i + 1] == 0xE1 && file[i + 4..].starts_with(b"Exif\0\0") && header.is_none() {
            header = Some((i + 10, len - 8));
        }
        if file[i + 1] == 0xDA {
            sos_end = Some(i + 2 + len);
            break;
        }
        i += 2 + len;
    }
    let ((at, len), sos_end) = (header?, sos_end?);
    let t = &file[at..at + len];
    let order = if &t[..2] == b"II" {
        Order::Ii
    } else {
        Order::Mm
    };
    let u16_at = |p: usize| {
        let b = [t[p], t[p + 1]];
        match order {
            Order::Ii => u16::from_le_bytes(b),
            Order::Mm => u16::from_be_bytes(b),
        }
    };
    let u32_at = |p: usize| {
        let b = [t[p], t[p + 1], t[p + 2], t[p + 3]];
        match order {
            Order::Ii => u32::from_le_bytes(b),
            Order::Mm => u32::from_be_bytes(b),
        }
    };
    let next = |p: usize| u32_at(p + 2 + 12 * u16_at(p) as usize) as usize;
    let ifd1 = next(u32_at(4) as usize);
    if ifd1 == 0 {
        return None;
    }
    let ifd2 = next(ifd1);
    if ifd2 == 0 {
        return None;
    }
    let field = |tag: u16| {
        (0..u16_at(ifd2) as usize)
            .map(|k| ifd2 + 2 + 12 * k)
            .find(|&r| u16_at(r) == tag)
            .map(|r| u32_at(r + 8) as usize)
    };
    let eoi = sos_end + file[sos_end..].windows(2).position(|w| w == [0xFF, 0xD9])? + 2;
    Some((at, field(0x0111)?, field(0x0117)?, eoi))
}

/// Run `edit` with oxidex and the oracle on `original` and require the same
/// result: both succeed (or both refuse and leave the file as it was), both
/// change the file or neither does, the same read-back, and no `-validate`
/// warning that neither the input nor the oracle's own edit draws.
fn assert_matches_oracle(
    oracle: &exiftool_oracle::Oracle,
    dir: &Path,
    name: &str,
    original: &[u8],
    edit: &[&str],
    label: &str,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let ours = write(dir, &format!("ours-{name}"), original);
    let theirs = write(dir, &format!("theirs-{name}"), original);
    let ran = oxidex(edit, &ours);
    let status = oracle
        .command()
        .args(["-q", "-q", "-overwrite_original"])
        .args(edit)
        .arg(&theirs)
        .status()
        .unwrap();
    let (ours_bytes, theirs_bytes) = (
        std::fs::read(&ours).unwrap(),
        std::fs::read(&theirs).unwrap(),
    );
    if !status.success() {
        assert!(
            !ran.status.success(),
            "{label}: the oracle refuses {edit:?}, oxidex wrote it"
        );
        assert!(
            ours_bytes == original,
            "{label}: refused but the file changed"
        );
        assert!(
            theirs_bytes == original,
            "{label}: oracle refused but changed the file"
        );
        return None;
    }
    assert!(
        ran.status.success(),
        "{label}: oxidex refused {edit:?}, which the oracle writes: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    // A no-op for the oracle (the value already there) is one for oxidex too.
    assert_eq!(
        ours_bytes != original,
        theirs_bytes != original,
        "{label}: oxidex and the oracle disagree on whether {edit:?} changes the file"
    );
    assert_eq!(
        readback(oracle, &ours),
        readback(oracle, &theirs),
        "{label}: read-back differs from the oracle's own {edit:?}"
    );
    // Warnings the input already draws (a real sample's odd offsets, which
    // the in-place path keeps and the oracle's rewrite fixes) are not the
    // write's doing.
    let source = write(dir, &format!("source-{name}"), original);
    let known: BTreeSet<String> = validate(oracle, &theirs)
        .union(&validate(oracle, &source))
        .cloned()
        .collect();
    let extra: Vec<String> = validate(oracle, &ours)
        .difference(&known)
        .cloned()
        .collect();
    assert!(
        extra.is_empty(),
        "{label}: -validate warnings neither the input nor the oracle's own {edit:?} draws: \
         {extra:?}"
    );
    Some((ours_bytes, theirs_bytes))
}

/// In both outputs, IFD2's PreviewImageStart outside the block points where
/// the oracle's rule puts it: at the preview bytes it can load, or at the
/// end of the image's EOI (plus junk; there is none here).
fn assert_preview_pointer(original: &[u8], outputs: &(Vec<u8>, Vec<u8>), label: &str) {
    let Some((at, start, len, _)) = jpeg_preview_pointer(original) else {
        return;
    };
    let loaded = original
        .get(at + start..at + start + len)
        .filter(|b| b[1] == 0xD8 && b[2] == 0xFF && matches!(b[3], 0xC4 | 0xDB | 0xE0..=0xEF));
    for (who, file) in [("oxidex", &outputs.0), ("oracle", &outputs.1)] {
        let Some((at2, start2, len2, eoi2)) = jpeg_preview_pointer(file) else {
            panic!("{label}: {who}'s output lost IFD2");
        };
        assert_eq!(len2, len, "{label}: {who} PreviewImageLength");
        let located = file.get(at2 + start2..at2 + start2 + len2);
        match loaded {
            // In the block, or after the image: the same bytes.
            Some(bytes) => assert_eq!(located, Some(bytes), "{label}: {who} preview bytes"),
            None => assert_eq!(
                at2 + start2,
                eoi2,
                "{label}: {who} points an unloadable preview at the EOI's end"
            ),
        }
    }
}

/// Every edit class on the Leica-layout fixtures, JPEG (preview in the
/// block, after the image, after the image but not loadable, past the end
/// of the file) and PNG (in the block), both byte orders: oxidex writes what
/// the oracle writes. At 53f3f0da the legacy delete, legacy set and mixed
/// writes were refused on every fixture, and the length-changing one on
/// every trailer fixture.
#[test]
fn every_edit_class_keeps_the_chain_as_the_oracle_does() {
    let Some(oracle) = oracle() else {
        eprintln!("skipping: no usable ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let mut cases: Vec<(String, Vec<u8>)> = [
            Preview::InBlock,
            Preview::Trailer,
            Preview::TrailerUnloadable,
            Preview::Truncated,
        ]
        .into_iter()
        .map(|p| (format!("{p:?}.jpg"), leica_jpeg(order, p)))
        .collect();
        cases.push((
            "InBlock.png".to_string(),
            png_with(&leica_tiff(order, &loadable_preview(), None)),
        ));
        for (name, original) in &cases {
            for (class, edit) in EDITS {
                let label = format!("{order:?} {name} {class}");
                let outputs =
                    assert_matches_oracle(oracle, dir.path(), name, original, edit, &label)
                        .unwrap_or_else(|| panic!("{label}: the oracle refused"));
                if name.ends_with(".jpg") && !matches!(class, "IFD1:All" | "EXIF:All") {
                    assert_preview_pointer(original, &outputs, &label);
                }
                if matches!(class, "IFD1:All" | "EXIF:All") && name.ends_with(".jpg") {
                    assert!(
                        jpeg_preview_pointer(&outputs.0).is_none(),
                        "{label}: chain kept"
                    );
                }
            }
        }
    }
}

/// [`Preview::Trailer`] whose IFD2 also has a JPEGInterchangeFormat pair
/// (0x0201/0x0202) locating the same trailer preview -- LeicaTL2's
/// JpgFromRaw shape, which ExifTool refuses to rewrite ("Error reading
/// JpgFromRaw data in IFD2").
fn jpgfromraw_jpeg(order: Order) -> Vec<u8> {
    let mut jpeg = leica_jpeg(order, Preview::Trailer);
    let (at, start, len, _) = jpeg_preview_pointer(&jpeg).unwrap();
    let ifd2 = {
        let t = &jpeg[at..];
        let rd32 = |p: usize| {
            let b = [t[p], t[p + 1], t[p + 2], t[p + 3]];
            match order {
                Order::Ii => u32::from_le_bytes(b),
                Order::Mm => u32::from_be_bytes(b),
            }
        };
        let rd16 = |p: usize| {
            let b = [t[p], t[p + 1]];
            match order {
                Order::Ii => u16::from_le_bytes(b),
                Order::Mm => u16::from_be_bytes(b),
            }
        };
        let next = |p: usize| rd32(p + 2 + 12 * rd16(p) as usize) as usize;
        at + next(next(rd32(4) as usize))
    };
    // Replace ImageWidth/ImageHeight with JPEGInterchangeFormat/Length.
    let row = |tag: u16, value: u32| {
        [
            order.u16(tag).as_slice(),
            &order.u16(4),
            &order.u32(1),
            &order.u32(value),
        ]
        .concat()
    };
    jpeg[ifd2 + 2..ifd2 + 14].copy_from_slice(&row(0x0201, start as u32));
    jpeg[ifd2 + 14..ifd2 + 26].copy_from_slice(&row(0x0202, len as u32));
    // Keep the rows sorted: 0x0103, 0x0111, 0x0117, 0x0201, 0x0202.
    let rows: Vec<Vec<u8>> = (0..5)
        .map(|k| jpeg[ifd2 + 2 + 12 * k..ifd2 + 14 + 12 * k].to_vec())
        .collect();
    let sorted = [&rows[2], &rows[3], &rows[4], &rows[0], &rows[1]].map(|r| r.clone());
    for (k, r) in sorted.iter().enumerate() {
        jpeg[ifd2 + 2 + 12 * k..ifd2 + 14 + 12 * k].copy_from_slice(r);
    }
    jpeg
}

/// Chain data outside the block that ExifTool will not re-point -- IFD2's
/// strip outside a PNG's `eXIf` chunk, a JPEG IFD2's JpgFromRaw after the
/// image -- is refused by the oracle ("Error reading ... data in IFD2") on
/// every write that keeps the chain, and so by oxidex; `IFD1:All` deletes it.
#[test]
fn chain_data_outside_the_block_is_refused_as_the_oracle_refuses() {
    let Some(oracle) = oracle() else {
        eprintln!("skipping: no usable ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let png = png_with(&leica_tiff(order, &loadable_preview(), Some(5000)));
        let jpeg = jpgfromraw_jpeg(order);
        for (name, original) in [("outside.png", png), ("jpgfromraw.jpg", jpeg)] {
            for (class, edit) in EDITS {
                let label = format!("{order:?} {name} {class}");
                let kept = assert_matches_oracle(oracle, dir.path(), name, &original, edit, &label);
                assert_eq!(
                    kept.is_some(),
                    matches!(class, "IFD1:All" | "EXIF:All"),
                    "{label}: only a write dropping the chain succeeds"
                );
            }
        }
    }
}

/// ExifTool's own Leica samples carry IFD2 with a preview offset past the
/// end of the (truncated) file: the oracle re-points it at the image's EOI
/// on every write, and reads every maker-note tag back unchanged. Every
/// edit class, compared with the oracle; LeicaTL2's IFD2 JpgFromRaw makes
/// the oracle refuse all but the chain-dropping writes.
#[test]
fn leica_samples_match_the_oracle() {
    let Some(oracle) = oracle() else {
        eprintln!("skipping: no usable ExifTool oracle");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    for sample in [
        "Leica/LeicaCL.jpg",
        "Leica/LeicaQ2.jpg",
        "Leica/LeicaS_Typ007.jpg",
        "Leica/LeicaTL2.jpg",
    ] {
        let Some(path) = fixtures::pinned_fixture_path(sample) else {
            continue;
        };
        let name = sample.trim_start_matches("Leica/");
        let original = std::fs::read(&path).unwrap();
        for (class, edit) in EDITS {
            let label = format!("{name} {class}");
            let outputs = assert_matches_oracle(oracle, dir.path(), name, &original, edit, &label);
            if let Some(outputs) = outputs.filter(|_| !matches!(class, "IFD1:All" | "EXIF:All")) {
                assert_preview_pointer(&original, &outputs, &label);
            }
        }
    }
}

/// A write through the public library writer `write_exif_to_jpeg`, which
/// runs no CLI post-condition: the metadata map read back from `original`,
/// with `IFD0:Artist` set to `artist`.
fn public_write(dir: &Path, original: &[u8], artist: &str) -> oxidex::error::Result<Vec<u8>> {
    let path = write(dir, "public.jpg", original);
    let mut map = oxidex::core::operations::read_metadata(&path).unwrap();
    map.insert(
        "IFD0:Artist",
        oxidex::core::tag_value::TagValue::new_string(artist),
    );
    let reader = oxidex::io::buffered_reader::BufferedReader::from_bytes(original);
    oxidex::writers::jpeg_writer::write_exif_to_jpeg(&reader, &map)
}

/// Codex on #954 (ifd_chain.rs:491). An edit that grows the EXIF block past
/// the old trailer-preview offset (carried verbatim until re-pointed) made
/// the re-scan of the new block take IFD2's 0x0111 for in-block data, so
/// the preview was not re-pointed: the public `write_exif_to_jpeg` returned
/// a PreviewImageStart inside the APP1, and the CLI's post-condition
/// refused an edit the oracle makes. Inside/outside is now the original
/// layout's call. At 16fcb986 the CLI refused, and the public writer left
/// the pointer in the block.
#[test]
fn a_block_grown_past_the_old_preview_offset_still_re_points_it() {
    let dir = tempfile::tempdir().unwrap();
    // Longer than everything between the APP1 and the preview.
    let artist = "a".repeat(700);
    let edit = format!("-IFD0:Artist={artist}");
    for order in [Order::Ii, Order::Mm] {
        for preview in [Preview::Trailer, Preview::TrailerUnloadable] {
            let original = leica_jpeg(order, preview);
            let label = format!("{order:?} {preview:?}");
            let (at, start, len, _) = jpeg_preview_pointer(&original).unwrap();
            let preview_bytes = original[at + start..at + start + len].to_vec();

            // The public writer points at the preview, now after the EOI.
            let out = public_write(dir.path(), &original, &artist)
                .unwrap_or_else(|e| panic!("{label}: public writer: {e}"));
            let (at2, start2, len2, eoi2) = jpeg_preview_pointer(&out).unwrap();
            // The new block reaches past the old preview offset.
            let new_block = usize::from(u16::from_be_bytes([out[4], out[5]])) - 8;
            assert!(
                new_block > start,
                "{label}: fixture does not grow past {start}"
            );
            assert_eq!(
                at2 + start2,
                eoi2,
                "{label}: public writer's PreviewImageStart"
            );
            assert_eq!(
                &out[at2 + start2..at2 + start2 + len2],
                &preview_bytes[..],
                "{label}"
            );

            // The CLI makes the oracle's edit.
            let Some(oracle) = oracle() else {
                eprintln!("skipping the oracle half: no usable ExifTool oracle");
                continue;
            };
            let outputs = assert_matches_oracle(
                oracle,
                dir.path(),
                &format!("grown-{preview:?}.jpg"),
                &original,
                &[edit.as_str()],
                &label,
            )
            .unwrap_or_else(|| panic!("{label}: the oracle refused"));
            assert_preview_pointer(&original, &outputs, &label);
        }
    }
}

/// Codex on #954 (jpeg_writer.rs:185). Chain data outside the APP1 other
/// than IFD2's preview (here IFD2's JpgFromRaw pair) is refused by ExifTool
/// and was refused by the CLI's post-condition only: the public
/// `write_exif_to_jpeg` accepted it and copied its offset stale. The public
/// writer refuses it itself now; at 16fcb986 it returned Ok.
#[test]
fn the_public_writer_refuses_chain_data_it_cannot_re_point() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let original = jpgfromraw_jpeg(order);
        for artist in ["x", "a much longer artist than the one before"] {
            let err = public_write(dir.path(), &original, artist)
                .expect_err("JpgFromRaw after the image must be refused");
            assert!(
                err.to_string().contains("locates data outside"),
                "{order:?} {artist}: {err}"
            );
        }
    }
}

/// Codex on #954 (exif_surgical.rs:2982). A JPEG whose header ends in an
/// EOI with no SOS: `transform_exif` searches the EOI from the EOI marker
/// and re-points an unloadable IFD2 preview there, but the verifier only
/// looked for an SOS and refused the correct edit. Both now take the same
/// boundary (`jpeg_scan_boundary`). (Pinned ExifTool 13.59 refuses to
/// write any JPEG without a scan -- "Corrupted JPEG image" -- chain or
/// not; oxidex writes one at tip too, a separate difference.) At 16fcb986
/// the edit was refused by the verifier.
#[test]
fn a_jpeg_without_a_scan_re_points_and_verifies_at_its_eoi() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        let tiff = leica_tiff(order, &loadable_preview(), Some(7_000_000));
        let mut original = vec![0xFF, 0xD8, 0xFF, 0xE1];
        original.extend(((tiff.len() + 8) as u16).to_be_bytes());
        original.extend(b"Exif\0\0");
        original.extend(&tiff);
        original.extend([0xFF, 0xD9]);
        let path = write(dir.path(), "noscan.jpg", &original);
        let ran = oxidex(&[LONG_ARTIST], &path);
        assert!(
            ran.status.success(),
            "{order:?}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
        let out = std::fs::read(&path).unwrap();
        // IFD2's PreviewImageStart: the end of the (last) EOI, TIFF-relative.
        let at = 12;
        let t = &out[at..];
        let rd = |p: usize, n: usize| -> usize {
            let b = &t[p..p + n];
            let mut v = 0usize;
            for i in 0..n {
                let byte = match order {
                    Order::Ii => b[n - 1 - i],
                    Order::Mm => b[i],
                };
                v = (v << 8) | usize::from(byte);
            }
            v
        };
        let next = |p: usize| rd(p + 2 + 12 * rd(p, 2), 4);
        let ifd2 = next(next(rd(4, 4)));
        let start = (0..rd(ifd2, 2))
            .map(|k| ifd2 + 2 + 12 * k)
            .find(|&r| rd(r, 2) == 0x0111)
            .map(|r| rd(r + 8, 4))
            .unwrap();
        assert_eq!(
            at + start,
            out.len(),
            "{order:?}: PreviewImageStart at the EOI's end"
        );
    }
}

/// Codex on #954 (exif_surgical.rs:1411). An `IFD2:` set or removal (or a
/// surfaced IFD2 row dropped from the map) beside another edit reported
/// success with the chain carried unchanged. The writers never edit the
/// chain, so -- as for IFD1 -- such a write is refused, file untouched; a
/// removal naming nothing IFD2 holds stays a no-op. (Pinned ExifTool 13.59
/// writes no IFD2 tag in a JPEG either: it reports these edits unchanged,
/// `IFD2:All` "Not a deletable group".) At 16fcb986 every refused case here
/// reported success. `-IFD2:All=` itself is ExifTool's warning, not a
/// refusal (#945's `write_request::undefined_tag_warning`, roll-up): pinned
/// 13.59 on leica-trailer-II.jpg / leica-inblock-II.jpg prints `Warning: Not
/// a deletable group: IFD2`, writes the other tags and keeps the chain, and
/// so does oxidex.
#[test]
fn edits_of_the_chain_past_ifd1_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    for order in [Order::Ii, Order::Mm] {
        for (name, original) in [
            ("trailer.jpg", leica_jpeg(order, Preview::Trailer)),
            ("inblock.jpg", leica_jpeg(order, Preview::InBlock)),
            (
                "inblock.png",
                png_with(&leica_tiff(order, &loadable_preview(), None)),
            ),
        ] {
            for args in [
                &["-IFD2:ImageWidth=5", "-IFD0:Artist=x"][..],
                &["-IFD2:ImageWidth=", "-IFD0:Artist=x"],
                &["-IFD2:ImageWidth=5"],
            ] {
                let path = write(dir.path(), name, &original);
                let ran = oxidex(args, &path);
                assert!(
                    !ran.status.success(),
                    "{order:?} {name} {args:?}: reported success"
                );
                assert!(
                    std::fs::read(&path).unwrap() == original,
                    "{order:?} {name} {args:?}: refused but changed"
                );
            }
            // `-IFD2:All=`: ExifTool's "Not a deletable group" warning; the
            // other tag is written and the chain carried.
            let path = write(dir.path(), name, &original);
            let before = oxidex::core::operations::read_metadata(&path).unwrap();
            let ran = oxidex(&["-IFD2:All=", "-IFD0:Artist=x"], &path);
            assert!(ran.status.success(), "{order:?} {name}: IFD2:All");
            assert!(
                String::from_utf8_lossy(&ran.stderr)
                    .contains("Warning: Not a deletable group: IFD2"),
                "{order:?} {name}: {}",
                String::from_utf8_lossy(&ran.stderr)
            );
            let after = oxidex::core::operations::read_metadata(&path).unwrap();
            assert_eq!(
                after.get_string("IFD0:Artist"),
                Some("x"),
                "{order:?} {name}"
            );
            let ifd2_rows = |map: &oxidex::core::metadata_map::MetadataMap| {
                map.keys().filter(|key| key.starts_with("IFD2:")).count()
            };
            assert_eq!(ifd2_rows(&after), ifd2_rows(&before), "{order:?} {name}");

            // Naming nothing IFD2 holds: a no-op beside a real edit.
            let path = write(dir.path(), name, &original);
            let ran = oxidex(&["-IFD2:GPSAltitude=", "-IFD0:Artist=x"], &path);
            assert!(
                ran.status.success(),
                "{order:?} {name}: {}",
                String::from_utf8_lossy(&ran.stderr)
            );

            // A surfaced IFD2 row dropped from the map (library path).
            let path = write(dir.path(), name, &original);
            let mut map = oxidex::core::operations::read_metadata(&path).unwrap();
            let Some(key) = map.keys().find(|key| key.starts_with("IFD2:")).cloned() else {
                continue; // the reader surfaces no IFD2 row for this carrier
            };
            map.remove(&key);
            map.insert(
                "IFD0:Artist",
                oxidex::core::tag_value::TagValue::new_string("x"),
            );
            assert!(
                oxidex::core::operations::write_metadata(&path, &map).is_err(),
                "{order:?} {name}: dropping {key} reported success"
            );
            assert!(
                std::fs::read(&path).unwrap() == original,
                "{order:?} {name}"
            );
        }
    }
}
