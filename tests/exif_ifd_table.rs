//! Slice E-1 gate B regression: the InteropIFD hanging off the ExifIFD
//! (ExifIFD 0xa005) is read through the generated `IFD_EXIF_MAIN` table and
//! the IFD engine at DirName `InteropIFD` for the rows the table reports --
//! see `src/core/exif_dir_engine.rs`, `tiff_helpers::parse_interop_subifd`
//! and the `("Exif", "Main")` line in `src/exiftool_tables/enabled_ifd.rs`.
//!
//! # Why real carriers
//!
//! The unit tests build synthetic directories, which is right for replay and
//! the PROCESSED guard and cannot observe a conversion defect: a builder
//! writes the bytes its author believes in. Every expectation below is the
//! pinned ExifTool 13.59 oracle's own output for files this repository did
//! not write. Both probes were run first, as `AGENTS.md` requires:
//!
//! ```text
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -ver
//! 13.59
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -s3 -FileType /tmp/oxidex-exiftool-cache/exiftool/t/images/OOXML.docx
//! DOCX
//! $ cd /tmp/oxidex-exiftool-cache/exiftool/t/images
//! $ exiftool-pinned.sh -G1 -a -s -j [-n] -InteropIFD:all Canon.jpg Nikon.jpg ExifTool.jpg
//! $ cd /tmp/oxidex-exiftool-cache/combined-samples
//! $ exiftool-pinned.sh -G1 -a -s -j [-n] -InteropIFD:all Samsung/SamsungGT-S5250.jpg \
//!       Canon/CanonHG20.jpg Samsung/SamsungSPH-A800.jpg Canon/CanonXL_H1.jpg
//! ```
//!
//! Each pinned tag names its producer: `engine` (the generated row, replayed
//! at its entry) or `residual` (the hand arm `INTEROP_RESIDUAL_IDS` keeps).
//! The DCF rows are keyed `EXIF:` (today's key; ExifTool's `-G1` is
//! `InteropIFD`, decision D-1).

use oxidex::core::MetadataMap;
use oxidex::core::exiftool_compat::format_tag_value;
use oxidex::core::operations::read_metadata;
use oxidex::exiftool_tables::{ENABLED_IFD, find_ifd_table};
use std::path::Path;

const T_IMAGES: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images";
const CORPUS: &str = "/tmp/oxidex-exiftool-cache/combined-samples";

/// The line is in force: without it the engine is `None` and the hand arms
/// (the E-1 fallback) produce the Interop rows instead, so the carrier pins
/// below would still pass on the rows both paths agree on. This makes a
/// revert of the line a red test (and protects IFD1, which rides the same
/// line).
#[test]
fn exif_main_is_on_the_gate_b_allowlist() {
    assert!(
        ENABLED_IFD.contains(&("Exif", "Main")),
        "ENABLED_IFD must carry the (\"Exif\", \"Main\") line"
    );
    let table = find_ifd_table("Exif", "Main").expect("Exif::Main is generated");
    assert!(
        table.gate_a.passes(),
        "gate A must pass: {:?}",
        table.gate_a.blocked_by
    );
    assert!(
        table.enabled(),
        "Exif::Main must be enabled (gate A and the gate B line together)"
    );
}

fn carrier(dir: &str, name: &str) -> Option<MetadataMap> {
    let path = Path::new(dir).join(name);
    if !path.is_file() {
        eprintln!(
            "skipping: {} is not present (the pinned ExifTool 13.59 checkout or corpus is not on this machine)",
            path.display()
        );
        return None;
    }
    Some(read_metadata(&path).unwrap_or_else(|e| panic!("{name} parses: {e}")))
}

/// The value as the output layer shows it (`-s` / `-j` text).
fn shown(metadata: &MetadataMap, key: &str) -> Option<String> {
    let value = format_tag_value(key, metadata.get(key)?);
    value
        .as_string()
        .map(str::to_string)
        .or_else(|| value.as_integer().map(|i| i.to_string()))
}

/// The `--no-print-conv` value (ExifTool's `-n`).
fn shown_n(metadata: &MetadataMap, key: &str) -> Option<String> {
    let projected = metadata.without_print_conv();
    let value = projected.get(key)?;
    value
        .as_string()
        .map(str::to_string)
        .or_else(|| value.as_integer().map(|i| i.to_string()))
}

fn assert_tags(metadata: &MetadataMap, file: &str, expected: &[(&str, &str)]) {
    for (key, want) in expected {
        assert_eq!(
            shown(metadata, key).as_deref(),
            Some(*want),
            "{file}: {key}"
        );
    }
}

/// Every Interop-family key oxidex reports for a file.
fn interop_keys(metadata: &MetadataMap) -> Vec<String> {
    let mut keys: Vec<String> = metadata
        .iter()
        .map(|(key, _)| key.clone())
        .filter(|key| {
            key.starts_with("InteropIFD:")
                || key.starts_with("EXIF:Interop")
                || key.starts_with("EXIF:RelatedImage")
        })
        .collect();
    keys.sort();
    keys
}

/// `Canon.jpg` (EOS 300D): a THM-indexed InteropIFD with the RelatedImage
/// pair.
#[test]
fn canon_jpg_interop_tags_match_the_pinned_oracle() {
    let Some(metadata) = carrier(T_IMAGES, "Canon.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "Canon.jpg",
        &[
            // engine
            ("EXIF:InteropIndex", "THM - DCF thumbnail file"),
            ("EXIF:RelatedImageWidth", "3072"),
            ("EXIF:RelatedImageHeight", "2048"),
            // residual (0x0002, `omitted.raw_conv`)
            ("EXIF:InteropVersion", "0100"),
        ],
    );
    assert_eq!(
        shown_n(&metadata, "EXIF:InteropIndex").as_deref(),
        Some("THM")
    );
    assert_eq!(
        interop_keys(&metadata),
        [
            "EXIF:InteropIndex",
            "EXIF:InteropVersion",
            "EXIF:RelatedImageHeight",
            "EXIF:RelatedImageWidth"
        ]
    );
}

#[test]
fn nikon_jpg_interop_tags_match_the_pinned_oracle() {
    let Some(metadata) = carrier(T_IMAGES, "Nikon.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "Nikon.jpg",
        &[
            // engine
            ("EXIF:InteropIndex", "R98 - DCF basic file (sRGB)"),
            // residual
            ("EXIF:InteropVersion", "0100"),
        ],
    );
    assert_eq!(
        shown_n(&metadata, "EXIF:InteropIndex").as_deref(),
        Some("R98")
    );
    assert_eq!(
        interop_keys(&metadata),
        ["EXIF:InteropIndex", "EXIF:InteropVersion"]
    );
}

/// `ExifTool.jpg` has no InteropIFD: the oracle prints nothing, and neither
/// producer may invent a row.
#[test]
fn exiftool_jpg_has_no_interop_rows() {
    let Some(metadata) = carrier(T_IMAGES, "ExifTool.jpg") else {
        return;
    };
    assert_eq!(interop_keys(&metadata), Vec::<String>::new());
}

/// Two of the sixteen census files whose InteropIndex was a VALUE row
/// before E-1: ExifTool prints an undeclared index as `Unknown (...)`, the
/// hand arm printed the bare string. Run with
/// `cargo test --test exif_ifd_table -- --ignored`.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples"]
fn census_interop_index_carriers_match_the_pinned_oracle() {
    for (dir, file, want, want_n) in [
        (
            "Samsung",
            "SamsungGT-S5250.jpg",
            "Unknown (R 9 8 )",
            "R 9 8 ",
        ),
        ("Canon", "CanonHG20.jpg", "Unknown ()", ""),
    ] {
        let metadata = carrier(&format!("{CORPUS}/{dir}"), file)
            .unwrap_or_else(|| panic!("{dir}/{file} is part of the pinned corpus"));
        assert_eq!(
            shown(&metadata, "EXIF:InteropIndex").as_deref(),
            Some(want),
            "{file}"
        );
        assert_eq!(
            shown_n(&metadata, "EXIF:InteropIndex").as_deref(),
            Some(want_n),
            "{file} -n"
        );
    }
}

/// An image-carrying InteropIFD: Compression and the OtherImage pair stay
/// with their residual arms; the engine's X/YResolution and ResolutionUnit
/// still yield to the IFD0 twins (the yield rule E-3 retires -- pinned
/// 13.59 `-a` prints them, its default duplicate-suppressed view does not).
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples"]
fn image_carrying_interop_keeps_its_residual_and_the_yield() {
    for (dir, file, start, length) in [
        ("Samsung", "SamsungSPH-A800.jpg", "528", "5146"),
        ("Canon", "CanonXL_H1.jpg", "1274", "2400"),
    ] {
        let metadata = carrier(&format!("{CORPUS}/{dir}"), file)
            .unwrap_or_else(|| panic!("{dir}/{file} is part of the pinned corpus"));
        assert_tags(
            &metadata,
            file,
            &[
                ("InteropIFD:Compression", "JPEG (old-style)"),
                ("InteropIFD:OtherImageStart", start),
                ("InteropIFD:OtherImageLength", length),
            ],
        );
        for name in ["XResolution", "YResolution", "ResolutionUnit"] {
            assert!(
                metadata.get(&format!("IFD0:{name}")).is_some(),
                "{file}: the IFD0 twin"
            );
            assert!(
                metadata.get(&format!("InteropIFD:{name}")).is_none(),
                "{file}: InteropIFD:{name} yields to IFD0 until E-3"
            );
        }
    }
}

/// PNG `eXIf` (entry point C3, `embedded.rs`): no corpus file carries an
/// InteropIFD this way, so this wraps the TIFF block of t/images Canon.jpg
/// in a minimal PNG and pins what the pinned oracle prints for the same
/// bytes (`exiftool-pinned.sh -G1 -a -s -j -InteropIFD:all` on the crafted
/// file: `THM - DCF thumbnail file`, 3072 x 2048).
#[test]
fn png_exif_chunk_reaches_the_interop_engine() {
    let Ok(jpeg) = std::fs::read(Path::new(T_IMAGES).join("Canon.jpg")) else {
        eprintln!("skipping: t/images/Canon.jpg is not on this machine");
        return;
    };
    let png = png_with_exif(&tiff_block_of(&jpeg));
    let file = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .expect("temp file");
    std::fs::write(file.path(), &png).expect("write the crafted PNG");
    let metadata = read_metadata(file.path()).expect("the crafted PNG parses");
    assert_tags(
        &metadata,
        "crafted Canon.png",
        &[
            ("EXIF:InteropIndex", "THM - DCF thumbnail file"),
            ("EXIF:RelatedImageWidth", "3072"),
            ("EXIF:RelatedImageHeight", "2048"),
            ("EXIF:InteropVersion", "0100"),
        ],
    );
    assert_eq!(
        shown_n(&metadata, "EXIF:InteropIndex").as_deref(),
        Some("THM")
    );
}

/// The TIFF block of a JPEG's first `Exif\0\0` APP1 segment.
fn tiff_block_of(jpeg: &[u8]) -> Vec<u8> {
    let mut at = 2;
    while at + 4 <= jpeg.len() {
        assert_eq!(jpeg[at], 0xff, "JPEG marker at {at}");
        let marker = jpeg[at + 1];
        let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        let body = &jpeg[at + 4..at + 2 + len];
        if marker == 0xe1 && body.starts_with(b"Exif\0\0") {
            return body[6..].to_vec();
        }
        at += 2 + len;
    }
    panic!("no Exif APP1 segment");
}

fn png_with_exif(tiff: &[u8]) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    // 1x1 truecolour, 8 bits.
    chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
    chunk(&mut png, b"eXIf", tiff);
    chunk(&mut png, b"IEND", &[]);
    png
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
