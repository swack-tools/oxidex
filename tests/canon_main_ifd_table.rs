//! Slice I-5 gate B regression: the top-level `Canon::Main` MakerNote IFD is
//! read through the generated `IFD_CANON_MAIN` table and the IFD engine for
//! the rows the table reports -- see
//! `src/parsers/tiff/makernotes/canon/main_engine.rs` and the
//! `("Canon", "Main")` line in `src/exiftool_tables/enabled_ifd.rs`.
//!
//! # Why real carriers
//!
//! `main_engine.rs`'s unit tests build synthetic notes, which is right for
//! replay order and cannot observe a conversion defect: a builder writes the
//! bytes its author believes in. Every expectation below is the pinned
//! ExifTool 13.59 oracle's own output for files this repository did not
//! write. Both probes were run first, as `AGENTS.md` requires:
//!
//! ```text
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -ver
//! 13.59
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -s3 -FileType /tmp/oxidex-exiftool-cache/exiftool/t/images/OOXML.docx
//! DOCX
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -a -s -G1 -D -Canon:all t/images/Canon.jpg
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -a -s -G1 -D -Canon:all t/images/Canon1DmkIII.jpg
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -n -a -s -G1 -Canon:CanonModelID \
//!       -Canon:ColorSpace -Canon:SerialNumberFormat t/images/Canon{,1DmkIII}.jpg
//! ```
//!
//! Each pinned tag names its producer: `engine` (the generated row, replayed
//! at its entry) or `residual` (the hand arm `CANON_MAIN_RESIDUAL_IDS` keeps).

use oxidex::core::MetadataMap;
use oxidex::core::operations::read_metadata;
use oxidex::exiftool_tables::find_ifd_table;
use std::path::Path;
use std::process::Command;

const T_IMAGES: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images";
const CORPUS: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Canon";

/// The line is in force: without it `main_engine` is `None` and the hand
/// arms (landing 1's fallback) produce these rows instead, so the carrier
/// pins below would still pass on the rows both paths agree on. This makes
/// a revert of the line a red test.
#[test]
fn canon_main_line_is_in_force() {
    let table = find_ifd_table("Canon", "Main").expect("Canon::Main is generated");
    assert!(
        table.gate_a.passes(),
        "gate A must pass: {:?}",
        table.gate_a.blocked_by
    );
    assert!(
        table.enabled(),
        "Canon::Main must be enabled (gate A and the gate B line together)"
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

fn shown(metadata: &MetadataMap, key: &str) -> Option<String> {
    let value = metadata.get(key)?;
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

/// `Canon.jpg` (EOS 300D): the two rows no hand arm ever produced
/// (CanonFileLength 0x000e and the Main 0x00ae ColorTemperature) are the
/// line's gain on this file; the rest is a fold of rows the hand path had.
#[test]
fn canon_jpg_main_tags_match_the_pinned_oracle() {
    let Some(metadata) = carrier(T_IMAGES, "Canon.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "Canon.jpg",
        &[
            // engine
            ("Canon:CanonImageType", "CRW:EOS DIGITAL REBEL CMOS RAW"),
            ("Canon:CanonFirmwareVersion", "Firmware Version 1.1.1"),
            ("Canon:OwnerName", "Phil Harvey"),
            (
                "Canon:CanonModelID",
                "EOS Digital Rebel / 300D / Kiss Digital",
            ),
            ("Canon:SerialNumberFormat", "Format 1"),
            ("Canon:ThumbnailImageValidArea", "0 159 7 112"),
            ("Canon:ColorSpace", "sRGB"),
            ("Canon:CanonFileLength", "4480822"),
            ("Canon:ColorTemperature", "5200"),
            // residual: 0x000c alternative 3 (`%.10u`), 0x0008
            ("Canon:SerialNumber", "0560018150"),
            ("Canon:FileNumber", "118-1861"),
        ],
    );
}

/// `Canon1DmkIII.jpg`: an EOS-1D body, so 0x000c takes its `/EOS-1D/`
/// alternative, and the note carries the empty Main strings ExifTool reports
/// as `""`.
#[test]
fn canon_1dmkiii_main_tags_match_the_pinned_oracle() {
    let Some(metadata) = carrier(T_IMAGES, "Canon1DmkIII.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "Canon1DmkIII.jpg",
        &[
            // engine
            ("Canon:CanonImageType", "Canon EOS-1D Mark III"),
            ("Canon:CanonFirmwareVersion", "Firmware Version 5.3.1"),
            ("Canon:OwnerName", ""),
            ("Canon:CanonModelID", "EOS-1D Mark III"),
            ("Canon:ThumbnailImageValidArea", "0 159 7 112"),
            ("Canon:SerialNumberFormat", "Format 2"),
            ("Canon:LensModel", "EF16-35mm f/2.8L II USM"),
            (
                "Canon:DustRemovalData",
                "(Binary data 1024 bytes, use -b option to extract)",
            ),
            ("Canon:ColorSpace", "sRGB"),
            ("Canon:CustomPictureStyleFileName", ""),
            // residual: 0x000c alternative 2 (`%.6u`), 0x0083, 0x0096, 0x00d0, 0x4008
            ("Canon:SerialNumber", "500292"),
            ("Canon:OriginalDecisionDataOffset", "3326"),
            ("Canon:InternalSerialNumber", "G002669"),
            ("Canon:VRDOffset", "0"),
            ("Canon:PictureStyleUserDef", "Standard; Standard; Standard"),
            // residual, KNOWN DEFECT: the oracle prints `n/a; n/a; n/a`;
            // `print_canon_picture_style` has no 0xffff entry. Pinned as
            // today's hand output so a change is deliberate, not credited to
            // this slice (VALUE 11 on the Canon subset, pre-existing).
            ("Canon:PictureStylePC", "65535; 65535; 65535"),
        ],
    );
}

/// `--no-print-conv` shows `Emitted::value_conv` for the rows a PrintConv
/// rendered, through the Canon value-form channel -- the pinned oracle's
/// `-n` for the same tags. oxidex's own `-n` is dry-run, so the CLI spelling
/// is `--no-print-conv` (`tests/step20_output_projection_matrix.rs`).
#[test]
fn no_print_conv_shows_the_engine_value_conv() {
    for (file, expected) in [
        (
            "Canon.jpg",
            [
                ("CanonModelID", "2147484016"),
                ("ColorSpace", "1"),
                ("SerialNumberFormat", "2415919104"),
            ],
        ),
        (
            "Canon1DmkIII.jpg",
            [
                ("CanonModelID", "2147484009"),
                ("ColorSpace", "1"),
                ("SerialNumberFormat", "2684354560"),
            ],
        ),
    ] {
        let path = Path::new(T_IMAGES).join(file);
        if !path.is_file() {
            eprintln!("skipping: {} is not present", path.display());
            return;
        }
        let mut args = vec!["--no-print-conv".to_string(), "-s".to_string()];
        args.extend(expected.iter().map(|(tag, _)| format!("-Canon:{tag}")));
        args.push(path.to_string_lossy().into_owned());
        let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(&args)
            .output()
            .expect("run oxidex");
        let stdout = String::from_utf8(output.stdout).expect("UTF-8 stdout");
        for (tag, want) in expected {
            let line = stdout
                .lines()
                .find(|line| line.split(':').next().map(str::trim) == Some(tag))
                .unwrap_or_else(|| panic!("{file}: no {tag} line in {stdout:?}"));
            assert_eq!(
                line.split_once(':').map(|(_, v)| v.trim()),
                Some(want),
                "{file}: --no-print-conv {tag}"
            );
        }
    }
}

/// Corpus carriers for the four classes `t/images` has no example of. Run
/// explicitly on a machine with the corpus:
/// `cargo test --test canon_main_ifd_table -- --ignored`.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Canon"]
fn corpus_main_tags_match_the_pinned_oracle() {
    // engine: 0x000c alternative 1 (`/EOS D30\b/`), and 0x000e.
    if let Some(metadata) = carrier(CORPUS, "CanonEOS_D30.jpg") {
        assert_tags(
            &metadata,
            "CanonEOS_D30.jpg",
            &[
                ("Canon:SerialNumber", "093107059"),
                ("Canon:CanonFileLength", "1305916"),
            ],
        );
    }
    // engine: a non-UTF-8 string is `fix_utf8`'d to `?`, as ExifTool's -j.
    if let Some(metadata) = carrier(CORPUS, "CanonDIGITAL_IXUS_IIs.jpg") {
        assert_tags(
            &metadata,
            "CanonDIGITAL_IXUS_IIs.jpg",
            &[("Canon:OwnerName", "Ren? Kuunders")],
        );
    }
    // engine: 0x0082 RawDataLength, a zero value.
    if let Some(metadata) = carrier(CORPUS, "CanonEOS-1DS.jpg") {
        assert_tags(
            &metadata,
            "CanonEOS-1DS.jpg",
            &[
                ("Canon:RawDataLength", "0"),
                ("Canon:SerialNumber", "101215"),
            ],
        );
    }
}
