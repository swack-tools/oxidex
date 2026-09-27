//! Slices E-1 and E-2 gate B regression: the ExifIFD (IFD0 0x8769) and the
//! InteropIFD hanging off it (ExifIFD 0xa005) are read through the generated
//! `IFD_EXIF_MAIN` table and the IFD engine at DirName `ExifIFD` /
//! `InteropIFD` for the rows the table reports -- see
//! `src/core/exif_dir_engine.rs`, `tiff_helpers::parse_exif_subifd` /
//! `parse_interop_subifd` and the `("Exif", "Main")` line in
//! `src/exiftool_tables/enabled_ifd.rs`.
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
//! $ exiftool-pinned.sh -G1 -a -s -j [-n] -ExifIFD:all Canon.jpg Nikon.jpg ExifTool.jpg
//! $ cd /tmp/oxidex-exiftool-cache/combined-samples
//! $ exiftool-pinned.sh -G1 -a -s -j [-n] -InteropIFD:all Samsung/SamsungGT-S5250.jpg \
//!       Canon/CanonHG20.jpg Samsung/SamsungSPH-A800.jpg Canon/CanonXL_H1.jpg
//! ```
//!
//! Each pinned tag names its producer: `engine` (the generated row, replayed
//! at its entry) or `residual` (the hand arm `INTEROP_RESIDUAL_IDS` keeps).
//! Every row is keyed `InteropIFD:`, ExifTool's `-G1` (the DCF rows since
//! decision D-1).

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::MetadataMap;
use oxidex::core::exiftool_compat::format_tag_value;
use oxidex::core::operations::read_metadata;
use oxidex::exiftool_tables::{ENABLED_IFD, find_ifd_table};

const T_IMAGES: &str = "t-images";
const CORPUS: &str = "combined";

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
    let path = if dir == T_IMAGES {
        fixtures::pinned_t_images_fixture_path(name)
    } else if dir == CORPUS {
        fixtures::pinned_combined_fixture_path(name)
    } else if let Some(name) = combined_carrier_name(dir, name) {
        fixtures::pinned_combined_fixture_path(&name)
    } else {
        panic!("unknown fixture population {dir}")
    }?;
    Some(read_metadata(&path).unwrap_or_else(|e| panic!("{name} parses: {e}")))
}

fn combined_carrier_name(dir: &str, name: &str) -> Option<String> {
    dir.strip_prefix(&format!("{CORPUS}/"))
        .filter(|subdirectory| !subdirectory.is_empty())
        .map(|subdirectory| format!("{subdirectory}/{name}"))
}

#[test]
fn carrier_accepts_combined_subdirectory_call_shapes() {
    assert_eq!(
        combined_carrier_name("combined/Canon", "CanonHG20.jpg"),
        Some("Canon/CanonHG20.jpg".to_owned())
    );
    assert_eq!(combined_carrier_name("combined/", "file.jpg"), None);
}

/// The value as the output layer shows it (`-s` / `-j` text).
fn shown(metadata: &MetadataMap, key: &str) -> Option<String> {
    let value = format_tag_value(key, metadata.get(key)?);
    value
        .as_string()
        .map(str::to_string)
        .or_else(|| value.as_integer().map(|i| i.to_string()))
        .or_else(|| match value {
            oxidex::core::TagValue::Float(f) => Some(f.to_string()),
            oxidex::core::TagValue::DateTime(dt) => {
                Some(dt.format("%Y:%m:%d %H:%M:%S").to_string())
            }
            _ => None,
        })
}

/// The `--no-print-conv` value (ExifTool's `-n`).
fn shown_n(metadata: &MetadataMap, key: &str) -> Option<String> {
    let projected = metadata.without_print_conv();
    let value = projected.get(key)?;
    value
        .as_string()
        .map(str::to_string)
        .or_else(|| value.as_integer().map(|i| i.to_string()))
        .or_else(|| match value {
            // As the CLI's own `--no-print-conv` path renders a `Float`
            // (`format_tag_value`'s `no_print_conv` arm,
            // `src/cli/output_formatter.rs`): Perl's default double
            // stringification, 15 significant digits, not Rust's
            // shortest-round-trip `Display` (17 digits, no exponent switch).
            oxidex::core::TagValue::Float(f) => {
                Some(oxidex::core::formatters::numeric_precision::perl_number(*f))
            }
            oxidex::core::TagValue::DateTime(dt) => {
                Some(dt.format("%Y:%m:%d %H:%M:%S").to_string())
            }
            oxidex::core::TagValue::Rational {
                numerator,
                denominator,
            } if *denominator != 0 => {
                // As `-j -n` prints it: the quotient to 10 significant digits
                // (ExifTool's `RoundFloat($n / $d, 10)`).
                let quotient = f64::from(*numerator) / f64::from(*denominator);
                let rounded: f64 = format!("{quotient:.9e}").parse().ok()?;
                Some(rounded.to_string())
            }
            _ => None,
        })
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
            ("InteropIFD:InteropIndex", "THM - DCF thumbnail file"),
            ("InteropIFD:RelatedImageWidth", "3072"),
            ("InteropIFD:RelatedImageHeight", "2048"),
            // residual (0x0002, `omitted.raw_conv`)
            ("InteropIFD:InteropVersion", "0100"),
        ],
    );
    assert_eq!(
        shown_n(&metadata, "InteropIFD:InteropIndex").as_deref(),
        Some("THM")
    );
    assert_eq!(
        interop_keys(&metadata),
        [
            "InteropIFD:InteropIndex",
            "InteropIFD:InteropVersion",
            "InteropIFD:RelatedImageHeight",
            "InteropIFD:RelatedImageWidth"
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
            ("InteropIFD:InteropIndex", "R98 - DCF basic file (sRGB)"),
            // residual
            ("InteropIFD:InteropVersion", "0100"),
        ],
    );
    assert_eq!(
        shown_n(&metadata, "InteropIFD:InteropIndex").as_deref(),
        Some("R98")
    );
    assert_eq!(
        interop_keys(&metadata),
        ["InteropIFD:InteropIndex", "InteropIFD:InteropVersion"]
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
            shown(&metadata, "InteropIFD:InteropIndex").as_deref(),
            Some(want),
            "{file}"
        );
        assert_eq!(
            shown_n(&metadata, "InteropIFD:InteropIndex").as_deref(),
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

// ---------------------------------------------------------------------
// Slice E-2: the ExifIFD. Engine rows are the generated table's; the
// residual (ExifVersion, ISO, ComponentsConfiguration, ShutterSpeedValue,
// Flash, UserComment, FlashpixVersion ... and, until D-2, ApertureValue /
// MaxApertureValue) is the hand arm's. Every expected value below is the
// pinned oracle's `-G1 -a -s -j -ExifIFD:all` for the file.
// ---------------------------------------------------------------------

/// Every ExifIFD key oxidex reports for a file (the visible ones: hex
/// fallbacks such as `ExifIFD:0x927C` are hidden without `--extended-output`).
fn exif_ifd_keys(metadata: &MetadataMap) -> Vec<String> {
    let mut keys: Vec<String> = metadata
        .iter()
        .map(|(key, _)| key.clone())
        .filter(|key| key.starts_with("ExifIFD:") && !key.contains(":0x"))
        .collect();
    keys.sort();
    keys
}

fn assert_exif_ifd(metadata: &MetadataMap, file: &str, expected: &[(&str, &str)]) {
    assert_tags(metadata, file, expected);
    let mut want: Vec<String> = expected.iter().map(|(k, _)| k.to_string()).collect();
    want.sort();
    assert_eq!(exif_ifd_keys(metadata), want, "{file}: ExifIFD key set");
}

const CANON_JPG_EXIF_IFD: &[(&str, &str)] = &[
    ("ExifIFD:ExposureTime", "4"),
    ("ExifIFD:FNumber", "14.0"),
    ("ExifIFD:ISO", "100"),
    ("ExifIFD:ExifVersion", "0221"),
    ("ExifIFD:DateTimeOriginal", "2003:12:04 06:46:52"),
    ("ExifIFD:CreateDate", "2003:12:04 06:46:52"),
    ("ExifIFD:ComponentsConfiguration", "Y, Cb, Cr, -"),
    ("ExifIFD:CompressedBitsPerPixel", "9"),
    ("ExifIFD:ShutterSpeedValue", "0"),
    ("ExifIFD:ApertureValue", "14.0"),
    ("ExifIFD:ExposureCompensation", "0"),
    ("ExifIFD:MaxApertureValue", "4.5"),
    ("ExifIFD:MeteringMode", "Average"),
    ("ExifIFD:Flash", "No Flash"),
    ("ExifIFD:FocalLength", "34.0 mm"),
    ("ExifIFD:UserComment", ""),
    ("ExifIFD:FlashpixVersion", "0100"),
    ("ExifIFD:ColorSpace", "sRGB"),
    ("ExifIFD:ExifImageWidth", "160"),
    ("ExifIFD:ExifImageHeight", "120"),
    ("ExifIFD:FocalPlaneXResolution", "3443.946188"),
    ("ExifIFD:FocalPlaneYResolution", "3442.016807"),
    ("ExifIFD:FocalPlaneResolutionUnit", "inches"),
    ("ExifIFD:SensingMethod", "One-chip color area"),
    ("ExifIFD:FileSource", "Digital Camera"),
    ("ExifIFD:CustomRendered", "Normal"),
    ("ExifIFD:ExposureMode", "Manual"),
    ("ExifIFD:WhiteBalance", "Auto"),
    ("ExifIFD:SceneCaptureType", "Standard"),
];

/// `Canon.jpg` (EOS 300D): 29 ExifIFD rows, and their `-n` forms (pinned
/// `-n`: ExposureTime 4, MeteringMode 1, ColorSpace 1,
/// FocalPlaneResolutionUnit 2,
/// SensingMethod 2, FileSource 3, ExposureMode 1, WhiteBalance 0).
#[test]
fn canon_jpg_exif_ifd_rows_match_the_pinned_oracle() {
    let Some(metadata) = carrier(T_IMAGES, "Canon.jpg") else {
        return;
    };
    assert_exif_ifd(&metadata, "Canon.jpg", CANON_JPG_EXIF_IFD);
    for (key, want) in [
        ("ExifIFD:ExposureTime", "4"),
        ("ExifIFD:MeteringMode", "1"),
        ("ExifIFD:ColorSpace", "1"),
        ("ExifIFD:FocalPlaneResolutionUnit", "2"),
        ("ExifIFD:SensingMethod", "2"),
        ("ExifIFD:FileSource", "3"),
        ("ExifIFD:ExposureMode", "1"),
        ("ExifIFD:WhiteBalance", "0"),
    ] {
        assert_eq!(
            shown_n(&metadata, key).as_deref(),
            Some(want),
            "Canon.jpg -n {key}"
        );
    }
    // Canon's sensor-size Composite reads FocalPlaneX/YResolution's
    // fraction (Canon.pm CalcSensorDiag): pinned `ScaleFactor35efl 1.6`,
    // `FocalLength35efl 34.0 mm (35 mm equivalent: 54.0 mm)`.
    assert_tags(
        &metadata,
        "Canon.jpg",
        &[(
            "Composite:FocalLength35efl",
            "34.0 mm (35 mm equivalent: 54.0 mm)",
        )],
    );
}

#[test]
fn nikon_jpg_exif_ifd_rows_match_the_pinned_oracle() {
    let Some(metadata) = carrier(T_IMAGES, "Nikon.jpg") else {
        return;
    };
    assert_exif_ifd(
        &metadata,
        "Nikon.jpg",
        &[
            ("ExifIFD:ExposureTime", "1/213"),
            ("ExifIFD:FNumber", "9.4"),
            ("ExifIFD:ExposureProgram", "Program AE"),
            ("ExifIFD:ISO", "100"),
            ("ExifIFD:ExifVersion", "0210"),
            ("ExifIFD:DateTimeOriginal", "2001:08:01 12:57:23"),
            ("ExifIFD:CreateDate", "2001:08:01 12:57:23"),
            ("ExifIFD:ComponentsConfiguration", "Y, Cb, Cr, -"),
            ("ExifIFD:CompressedBitsPerPixel", "3"),
            ("ExifIFD:ExposureCompensation", "0"),
            ("ExifIFD:MaxApertureValue", "3.4"),
            ("ExifIFD:MeteringMode", "Multi-segment"),
            ("ExifIFD:LightSource", "Unknown"),
            ("ExifIFD:Flash", "No Flash"),
            ("ExifIFD:FocalLength", "8.6 mm"),
            ("ExifIFD:UserComment", ""),
            ("ExifIFD:FlashpixVersion", "0100"),
            ("ExifIFD:ColorSpace", "sRGB"),
            ("ExifIFD:ExifImageWidth", "1600"),
            ("ExifIFD:ExifImageHeight", "1200"),
            ("ExifIFD:FileSource", "Digital Camera"),
            ("ExifIFD:SceneType", "Directly photographed"),
        ],
    );
    // pinned `-n`: ExposureTime 0.004686035614, SceneType 1, FileSource 3.
    for (key, want) in [
        ("ExifIFD:ExposureTime", "0.004686035614"),
        ("ExifIFD:SceneType", "1"),
        ("ExifIFD:FileSource", "3"),
    ] {
        assert_eq!(
            shown_n(&metadata, key).as_deref(),
            Some(want),
            "Nikon.jpg -n {key}"
        );
    }
}

#[test]
fn exiftool_jpg_exif_ifd_rows_match_the_pinned_oracle() {
    let Some(metadata) = carrier(T_IMAGES, "ExifTool.jpg") else {
        return;
    };
    assert_exif_ifd(
        &metadata,
        "ExifTool.jpg",
        &[
            ("ExifIFD:FNumber", "3.5"),
            ("ExifIFD:ExposureProgram", "Program AE"),
            ("ExifIFD:ISO", "100"),
            ("ExifIFD:ExifVersion", "0210"),
            ("ExifIFD:DateTimeOriginal", "2001:05:19 18:36:41"),
            ("ExifIFD:CreateDate", "2001:05:19 18:36:41"),
            ("ExifIFD:ComponentsConfiguration", "Y, Cb, Cr, -"),
            ("ExifIFD:CompressedBitsPerPixel", "1.6"),
            ("ExifIFD:ShutterSpeedValue", "1/64"),
            ("ExifIFD:ApertureValue", "3.5"),
            ("ExifIFD:BrightnessValue", "2"),
            ("ExifIFD:ExposureCompensation", "0"),
            ("ExifIFD:MaxApertureValue", "3.5"),
            ("ExifIFD:MeteringMode", "Multi-segment"),
            ("ExifIFD:Flash", "Fired"),
            ("ExifIFD:FocalLength", "6.0 mm"),
            ("ExifIFD:FlashpixVersion", "0100"),
            ("ExifIFD:ColorSpace", "sRGB"),
            ("ExifIFD:ExifImageWidth", "100"),
            ("ExifIFD:ExifImageHeight", "80"),
            ("ExifIFD:FocalPlaneXResolution", "3053"),
            ("ExifIFD:FocalPlaneYResolution", "3053"),
            ("ExifIFD:FocalPlaneResolutionUnit", "cm"),
            ("ExifIFD:SensingMethod", "One-chip color area"),
            ("ExifIFD:FileSource", "Digital Camera"),
            ("ExifIFD:SceneType", "Directly photographed"),
        ],
    );
}

/// Census files whose ExifIFD rows moved in E-2 (and the rules it keeps),
/// each against the pinned oracle. Run with
/// `cargo test --test exif_ifd_table -- --ignored`.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples"]
fn census_exif_ifd_carriers_match_the_pinned_oracle() {
    let file = |path: &str| {
        let (dir, name) = path.rsplit_once('/').expect("dir/name");
        carrier(&format!("{CORPUS}/{dir}"), name)
            .unwrap_or_else(|| panic!("{path} is part of the pinned corpus"))
    };
    // TIFF/EP ids 0x920e-0x9210, which the hand name table never named.
    let leica = file("Leica/LeicaM9.jpg");
    assert_tags(
        &leica,
        "LeicaM9.jpg",
        &[
            ("ExifIFD:FocalPlaneXResolution", "3700"),
            ("ExifIFD:FocalPlaneYResolution", "3689"),
            ("ExifIFD:FocalPlaneResolutionUnit", "inches"),
        ],
    );
    // K-O: the ExifIFD sits after its value block; the engine reads the
    // values stored before it (pinned `-n` ExposureTime 0.003125; the hand
    // arm's `-n` was the printed `1/320`).
    let sony = file("Sony/SonyILCE-6100.jpg");
    assert_eq!(
        shown_n(&sony, "ExifIFD:ExposureTime").as_deref(),
        Some("0.003125")
    );
    assert_tags(
        &sony,
        "SonyILCE-6100.jpg",
        &[("ExifIFD:DateTimeOriginal", "2019:01:01 03:07:51")],
    );
    // A signed rational stored before the directory: pinned `-2` (the hand
    // arm printed the unsigned 4294967294).
    let fx3 = file("Sony/SonyILME-FX3.jpg");
    assert_tags(
        &fx3,
        "SonyILME-FX3.jpg",
        &[("ExifIFD:ExposureCompensation", "-2")],
    );
    // NUL-truncated as ReadValue's `string` (the hand arm kept the bytes
    // after the NUL: `EF50mm f/1.8 II.5-5.6 IS`).
    let eos60d = file("Canon/CanonEOS60D.jpg");
    assert_tags(
        &eos60d,
        "CanonEOS60D.jpg",
        &[("ExifIFD:LensModel", "EF50mm f/1.8 II")],
    );
    // Three UserComment copies in one IFD (a residual id): the last one, as
    // pinned `-j -G1` prints it (`\n\x07...`; oxidex's rendering of the
    // rest is a pre-existing VALUE the residual keeps).
    let nexus = file("Google/GoogleNexusS.jpg");
    let comment = shown(&nexus, "ExifIFD:UserComment").expect("UserComment");
    assert!(
        comment.starts_with('\n') && comment.contains('\u{7}'),
        "GoogleNexusS.jpg: the last copy, got {comment:?}"
    );
    // Bare -WhiteBalance answers the MakerNotes copy, as pinned
    // `-WhiteBalance` does (`Auto`, all three copies agree).
    let eos40d = file("Canon/CanonEOS40D.jpg");
    let winner = oxidex::cli::tag_resolution::resolve_requested_tags(
        &eos40d,
        &["WhiteBalance".to_string()],
        false,
    );
    assert_eq!(winner.len(), 1);
    assert_eq!(winner[0].lookup_key, "Canon:WhiteBalance");
    assert_tags(
        &eos40d,
        "CanonEOS40D.jpg",
        &[("ExifIFD:WhiteBalance", "Auto")],
    );
    // The yield-to-IFD0 rule is kept until E-3: pinned `-a -G1` prints an
    // `[ExifIFD] Padding` beside the IFD0 one, oxidex only IFD0's.
    for path in ["Canon/CanonIXY640.jpg", "Canon/CanonPowerShotELPH330HS.jpg"] {
        let metadata = file(path);
        assert!(metadata.get("IFD0:Padding").is_some(), "{path}");
        assert!(
            metadata.get("ExifIFD:Padding").is_none(),
            "{path}: until E-3"
        );
    }
}

/// Decision D-2 (802a459e / 5181163c, 2026-09-12) moved MaxApertureValue to
/// the engine of that day (`DecodedValue::number()`), whose `2**($val/2)`
/// could not numify the two-count `2.971 1` SamsungGT-B2710.jpg writes: the
/// tag came back absent, not the hand arm's wrong `2.971 1` (pinned
/// ExifTool numifies the leading `2.971` and prints `2.8`). The commit
/// named the future fix "construct K-N" and it landed in 2d8ff775 ("v2
/// steps 1-2: Exif::Main conversions generated from ExifTool source,
/// per-field mixed mode (0 lost, +94 reads)", #838, 2026-09-18): the
/// Session runtime's `perl_num`/`numify_bytes` give `vc_9205`'s
/// `rt::div`/`rt::pow` (`src/exiftool_tables/conv/exif_main.rs`,
/// `src/exiftool_tables/conv/rt.rs`) the same leading-numeric-prefix
/// coercion Perl applies in numeric context, so `2.971 1` numifies to
/// `2.971` and the row now matches. Bisected by building
/// `git log --oneline -S"0x9202, 0x9205" -- src/core/tiff_helpers.rs`'s
/// single non-D-2 hit, 2d8ff775, against its parent 8cceb4a7: this test
/// passes (absent) at 8cceb4a7 and fails (present) at 2d8ff775. Reverting
/// 2d8ff775 alone would restore the absence this test used to pin.
///
/// Pinned ExifTool 13.59 (`-j -G1 -ExifIFD:MaxApertureValue`): `2.8`;
/// (`-n -j -G1`): `2.80014201831531`. oxidex's CLI matches both: `-j`
/// prints `2.8`, and `oxidex --no-print-conv -j -G1
/// -ExifIFD:MaxApertureValue` prints the oracle's exact `2.80014201831531`
/// (`-s` too) -- `format_tag_value`'s `no_print_conv` arm for a `Float`
/// (`src/cli/output_formatter.rs`) renders it with
/// `numeric_precision::perl_number`, Perl's own `%.15g` double
/// stringification, not Rust's 17-digit shortest-round-trip `Display`.
/// `shown_n` now calls the same `perl_number` rather than `f.to_string()`,
/// so it renders this tag exactly as the CLI and the oracle do.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples"]
fn census_d2_max_aperture_value_matches_exiftool() {
    let b2710 = carrier(&format!("{CORPUS}/Samsung"), "SamsungGT-B2710.jpg")
        .expect("Samsung/SamsungGT-B2710.jpg is part of the pinned corpus");
    assert_eq!(
        shown(&b2710, "ExifIFD:MaxApertureValue").as_deref(),
        Some("2.8")
    );
    assert_eq!(
        shown_n(&b2710, "ExifIFD:MaxApertureValue").as_deref(),
        Some("2.80014201831531")
    );
}

/// PNG `eXIf` (entry point C3, `embedded.rs`): no corpus file carries an
/// InteropIFD this way, so this wraps the TIFF block of t/images Canon.jpg
/// in a minimal PNG and pins what the pinned oracle prints for the same
/// bytes (`exiftool-pinned.sh -G1 -a -s -j -InteropIFD:all` on the crafted
/// file: `THM - DCF thumbnail file`, 3072 x 2048).
#[test]
fn png_exif_chunk_reaches_the_interop_engine() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("Canon.jpg") else {
        return;
    };
    let jpeg = std::fs::read(&path).expect("Canon.jpg should be readable");
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
            ("InteropIFD:InteropIndex", "THM - DCF thumbnail file"),
            ("InteropIFD:RelatedImageWidth", "3072"),
            ("InteropIFD:RelatedImageHeight", "2048"),
            ("InteropIFD:InteropVersion", "0100"),
        ],
    );
    assert_eq!(
        shown_n(&metadata, "InteropIFD:InteropIndex").as_deref(),
        Some("THM")
    );
    // The ExifIFD of the same block (pinned `-G1 -a -s -j -ExifIFD:all` on
    // the crafted PNG prints the same 29 rows as for Canon.jpg; `-n`
    // ColorSpace 1).
    assert_exif_ifd(&metadata, "crafted Canon.png", CANON_JPG_EXIF_IFD);
    assert_eq!(
        shown_n(&metadata, "ExifIFD:ColorSpace").as_deref(),
        Some("1")
    );
    // Entry point C3 reaches the ExifIFD engine, not just the hand arm: every
    // assertion above is one the hand arm satisfies too (review finding,
    // E-2: the test passed at b4808958 and with the engine switched off).
    // Only the engine gives FileSource its `-n` number -- pinned 13.59 `-j
    // -n` on this PNG: 3; the hand arm's is the 1-byte `undef` run,
    // `(Binary data 1 bytes, ...)`.
    assert_eq!(
        shown_n(&metadata, "ExifIFD:FileSource").as_deref(),
        Some("3")
    );
}

/// Review finding (E-2): an ExifIFD entry ExifTool refuses reports nothing,
/// though the hand reader reaches its bytes. Crafted files, byte for byte
/// (`crafted/overlap.tif` and `crafted/pastpayload.jpg` of the E-2 review;
/// pinned `exiftool-pinned.sh -j -G1 -a -ExifIFD:all`):
/// * `overlap.tif` (C2; and the same block as PNG `eXIf`, C3): ExposureTime,
///   FNumber and DateTimeOriginal point into the directory or the bytes
///   before its count word, BrightnessValue into its last entry --
///   "Suspicious ExifIFD offset" (Exif.pm:6549) for each -- while
///   ExposureCompensation starts exactly at `$dirEnd`. The oracle prints
///   ExposureCompensation `0`, LensModel `LensX12` and FocalPlaneXResolution
///   `3000`, nothing else.
/// * `pastpayload.jpg` (C1): LensModel and DateTimeOriginal lie past the APP1
///   payload, in the next segment -- "Bad offset for ExifIFD ..."
///   (Exif.pm:6551-6552); the oracle prints ExposureTime `1/100` alone.
/// * `hdroff0.jpg` (C1, the E-2 recheck's): ExposureTime's offset, and IFD1
///   XResolution's, is 0 -- inside the TIFF header, "Suspicious ExifIFD
///   offset for ExposureTime" / "Suspicious IFD1 offset for XResolution"
///   (Exif.pm:6539); the oracle prints ExifIFD FNumber `2.8` alone and no
///   IFD1 XResolution (control printed ExposureTime 346409.1, the hand
///   arm's reading of the header bytes).
#[test]
fn entries_exiftool_refuses_report_nothing() {
    const OVERLAP_TIF: &str = "49492a000800000002000f01020006000000260000006987040001000000\
        300000000000000043616e6f6e004d00000007009a820500010000003a0000009d820500010000002c0000\
        0003900200140000002000000004920a00010000008600000003920a00010000008200000034a402000800\
        00008a0000000ea205000100000092000000000000004c656e7358313200b80b000001000000";
    const PAST_PAYLOAD_JPG: &str = "ffd8ffe1006645786966000049492a000800000002000f0102000600\
        00002600000069870400010000002c0000000000000043616e6f6e0003009a820500010000005600000034\
        a402001000000066000000039002001400000076000000000000000100000064000000ffe2002a50414453\
        4c656e73506173745061796c6f616400323032313a30323a30332030343a30353a303600ffd9";
    const HEADER_OFFSET_JPG: &str = "ffd8ffe1010845786966000049492a000800000002000f010200060000003c000000698704000100000064\
        000000a00000000000000000000000000000000000000000000000000043616e6f6e00000000001c000000\
        0a0000000000600000000100000000000000000000000000000002009a82050001000000000000009d8205\
        00010000004600000000000000000000000000000000000000000000000000000000000000000000000000\
        02001a01050001000000000000001b01050001000000500000000000000000000000000000000000000000\
        00000000000000000000000000000000000000000000000000000000000000000000000000000000000000\
        00000000000000000000ffd9";
    let read = |suffix: &str, bytes: &[u8]| {
        let file = tempfile::Builder::new()
            .suffix(suffix)
            .tempfile()
            .expect("temp file");
        std::fs::write(file.path(), bytes).expect("write the crafted file");
        read_metadata(file.path()).expect("the crafted file parses")
    };
    let overlap = hex(OVERLAP_TIF);
    let expected: &[(&str, &str)] = &[
        ("ExifIFD:ExposureCompensation", "0"),
        ("ExifIFD:LensModel", "LensX12"),
        ("ExifIFD:FocalPlaneXResolution", "3000"),
    ];
    assert_exif_ifd(&read(".tif", &overlap), "crafted overlap.tif", expected);
    assert_exif_ifd(
        &read(".png", &png_with_exif(&overlap)),
        "crafted overlap.png",
        expected,
    );
    assert_exif_ifd(
        &read(".jpg", &hex(PAST_PAYLOAD_JPG)),
        "crafted pastpayload.jpg",
        &[("ExifIFD:ExposureTime", "1/100")],
    );
    let header_offset = read(".jpg", &hex(HEADER_OFFSET_JPG));
    assert_exif_ifd(
        &header_offset,
        "crafted hdroff0.jpg",
        &[("ExifIFD:FNumber", "2.8")],
    );
    assert!(header_offset.get("IFD1:XResolution").is_none());
}

/// Decision D-3 (its own commit): an ExifIFD `SubDirectory` edge id reports
/// nothing -- DJI_XT2.jpg's 0x02bc ApplicationNotes, an XMP sub-directory;
/// pinned `-a -G1 -ExifIFD:all` does not list it (only a by-name request
/// extracts it). Reverting D-3 alone restores the hand row and drops this.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples"]
fn census_d3_an_edge_id_reports_nothing() {
    let xt2 = carrier(&format!("{CORPUS}/DJI"), "DJI_XT2.jpg")
        .expect("DJI/DJI_XT2.jpg is part of the pinned corpus");
    assert!(xt2.get("ExifIFD:ApplicationNotes").is_none());
}

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// Review finding (E-2): a JP2 whose Exif `uuid` box carries Canon.jpg's
/// TIFF block copies the embedded walk's winners into the JP2's map at one
/// priority, so the order of the copy decides which of `ExifIFD:FNumber`
/// (`-n` 14) and `Canon:FNumber` (`-n` 14.25...) Composite `Aperture`
/// reads. Copied in `HashMap` order, `-j` printed `14.0` or `14.3` from run
/// to run (4 of 12 reads of this very file); in file order it is the
/// oracle's every time (pinned `-G1 -s -Composite:all` on the file this test
/// writes: Aperture `14.0`, LightValue `5.6`, HyperfocalDistance `4.37 m`).
/// Each read builds new maps, so a per-process random order shows up
/// within one test run.
#[test]
fn a_jp2_exif_box_copies_its_winners_in_file_order() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("Canon.jpg") else {
        return;
    };
    let jpeg = std::fs::read(&path).expect("Canon.jpg should be readable");
    let tiff = tiff_block_of(&jpeg);
    let mut jp2 = b"\0\0\0\x0cjP  \r\n\x87\n".to_vec();
    jp2.extend_from_slice(b"\0\0\0\x14ftypjp2 \0\0\0\0jp2 ");
    jp2.extend_from_slice(&((8 + 16 + tiff.len()) as u32).to_be_bytes());
    jp2.extend_from_slice(b"uuidJpgTiffExif->JP2");
    jp2.extend_from_slice(&tiff);
    let file = tempfile::Builder::new()
        .suffix(".jp2")
        .tempfile()
        .expect("temp file");
    std::fs::write(file.path(), &jp2).expect("write the crafted JP2");
    for run in 0..24 {
        let metadata = read_metadata(file.path()).expect("the crafted JP2 parses");
        assert_tags(
            &metadata,
            &format!("crafted Canon.jp2, read {run}"),
            &[
                ("Composite:Aperture", "14.0"),
                ("Composite:LightValue", "5.6"),
                ("Composite:HyperfocalDistance", "4.37 m"),
            ],
        );
    }
}

/// Review finding (E-2): a PDF whose DCTDecode image XObject is t/images
/// Canon.jpg byte for byte re-enters the JPEG parser, and the PDF parser
/// merges those rows into its own map. Flattened through `iter()` +
/// `insert()`, the merge kept only each ExifIFD engine row's printed value,
/// so FocalPlaneX/YResolution lost the fraction Canon's sensor-size
/// Composite reads and `-n` FocalLength became `34.0 mm`. The pinned oracle
/// reads nothing from the image stream (with or without `-ee`), so these
/// rows are oxidex's own; they must at least agree with the JPEG they come
/// from (read directly: FocalLength35efl `34.0 mm (35 mm equivalent: 54.0
/// mm)`, HyperfocalDistance `4.37 m`, `-n` FocalLength 34, as the oracle
/// prints for t/images Canon.jpg).
#[test]
fn a_pdf_dct_image_keeps_its_exif_ifd_forms() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("Canon.jpg") else {
        return;
    };
    let jpeg = std::fs::read(&path).expect("Canon.jpg should be readable");
    let direct = read_metadata(&path).expect("Canon.jpg");
    let pdf = pdf_with_dct_image(&jpeg);
    let file = tempfile::Builder::new()
        .suffix(".pdf")
        .tempfile()
        .expect("temp file");
    std::fs::write(file.path(), &pdf).expect("write the crafted PDF");
    let metadata = read_metadata(file.path()).expect("the crafted PDF parses");
    for key in [
        "Composite:FocalLength35efl",
        "Composite:HyperfocalDistance",
        "Composite:FOV",
    ] {
        assert!(shown(&direct, key).is_some(), "{key} from Canon.jpg");
        assert_eq!(shown(&metadata, key), shown(&direct, key), "{key}");
    }
    assert_eq!(
        shown(&metadata, "Composite:FocalLength35efl").as_deref(),
        Some("34.0 mm (35 mm equivalent: 54.0 mm)")
    );
    assert_eq!(
        shown_n(&metadata, "ExifIFD:FocalLength").as_deref(),
        Some("34")
    );
}

/// A one-page PDF whose only image XObject is `jpeg`, DCTDecode.
fn pdf_with_dct_image(jpeg: &[u8]) -> Vec<u8> {
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    let objects: [&[u8]; 3] = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 8 8] /Resources << /XObject << /Im1 4 0 R >> >> >>",
    ];
    for (index, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    offsets.push(pdf.len());
    pdf.extend_from_slice(
        format!(
            "4 0 obj\n<< /Type /XObject /Subtype /Image /Width 8 /Height 8 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .as_bytes(),
    );
    pdf.extend_from_slice(jpeg);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

/// Review finding (E-2): the PNG writer rebuilds the whole `eXIf` chunk from
/// the map, and an ExifIFD engine row's map value is ExifTool's printed one,
/// so any write to a PNG with an `eXIf` chunk wrote `MeteringMode` as the
/// ASCII `Average`, `FocalLength` as `34.0 mm`, `FileSource` as `Digital
/// Camera`. The rebuild serializes each row's stored form
/// (`TagOccurrence::stored`) -- what it serialized before the engine, byte
/// for byte (control b4808958 and this tree write identical files for
/// `-PNG:Comment`, `-EXIF:Artist`, `-ExifIFD:ExposureTime`, `-AllDates+=`).
/// Expectations: pinned `exiftool-pinned.sh -v3` on the rewritten file
/// (MeteringMode int16u 1, FocalLength rational64u 34/1, ColorSpace int16u
/// 1, SensingMethod int16u 2, FileSource undef 03, FNumber rational64u
/// 14/1).
#[test]
fn a_png_exif_rebuild_writes_stored_values() {
    use oxidex::core::TagValue;
    let Some(path) = fixtures::pinned_t_images_fixture_path("Canon.jpg") else {
        return;
    };
    let jpeg = std::fs::read(&path).expect("Canon.jpg should be readable");
    let file = tempfile::Builder::new()
        .suffix(".png")
        .tempfile()
        .expect("temp file");
    std::fs::write(file.path(), png_with_exif(&tiff_block_of(&jpeg))).expect("write the PNG");
    oxidex::core::operations::modify_tag(file.path(), "PNG:Comment", TagValue::new_string("hello"))
        .expect("the PNG write succeeds");
    let written = std::fs::read(file.path()).expect("read back");
    let tiff = png_chunk(&written, b"eXIf").expect("the rebuilt eXIf chunk");
    let entries = tiff_entries(&tiff);
    let entry = |id: u16| {
        entries
            .iter()
            .find(|(tag, ..)| *tag == id)
            .unwrap_or_else(|| panic!("tag {id:#06x} written"))
            .clone()
    };
    assert_eq!(entry(0x9207), (0x9207, 3, 1, vec![1, 0]), "MeteringMode");
    assert_eq!(entry(0xa001), (0xa001, 3, 1, vec![1, 0]), "ColorSpace");
    assert_eq!(entry(0xa217), (0xa217, 3, 1, vec![2, 0]), "SensingMethod");
    assert_eq!(entry(0xa300), (0xa300, 7, 1, vec![3]), "FileSource");
    assert_eq!(
        entry(0x920a),
        (
            0x920a,
            5,
            1,
            [34u32.to_le_bytes(), 1u32.to_le_bytes()].concat()
        ),
        "FocalLength"
    );
    assert_eq!(
        entry(0x829d),
        (
            0x829d,
            5,
            1,
            [14u32.to_le_bytes(), 1u32.to_le_bytes()].concat()
        ),
        "FNumber"
    );
}

/// The data of the first `kind` chunk of a PNG.
fn png_chunk(png: &[u8], kind: &[u8; 4]) -> Option<Vec<u8>> {
    let mut at = 8;
    while at + 8 <= png.len() {
        let len = u32::from_be_bytes(png[at..at + 4].try_into().ok()?) as usize;
        if &png[at + 4..at + 8] == kind {
            return png.get(at + 8..at + 8 + len).map(<[u8]>::to_vec);
        }
        at += 12 + len;
    }
    None
}

/// `(tag, type, count, value bytes)` of every entry of a TIFF block's IFD0
/// and, when it points to one, its ExifIFD (value bytes as stored).
fn tiff_entries(tiff: &[u8]) -> Vec<(u16, u16, u32, Vec<u8>)> {
    let little = &tiff[..2] == b"II";
    let u16_at = |at: usize| {
        let bytes = [tiff[at], tiff[at + 1]];
        if little {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        }
    };
    let u32_at = |at: usize| {
        let bytes: [u8; 4] = tiff[at..at + 4].try_into().unwrap();
        if little {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    };
    let size = |ty: u16| match ty {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        _ => 8,
    };
    let mut out = Vec::new();
    let mut dirs = vec![u32_at(4) as usize];
    while let Some(dir) = dirs.pop() {
        for index in 0..usize::from(u16_at(dir)) {
            let at = dir + 2 + 12 * index;
            let (tag, ty, count) = (u16_at(at), u16_at(at + 2), u32_at(at + 4));
            let len = size(ty) * count as usize;
            let value = if len <= 4 {
                tiff[at + 8..at + 8 + len].to_vec()
            } else {
                let start = u32_at(at + 8) as usize;
                tiff[start..start + len].to_vec()
            };
            if tag == 0x8769 {
                dirs.push(u32_at(at + 8) as usize);
            }
            out.push((tag, ty, count, value));
        }
    }
    out
}

/// Review finding (E-2, found while fixing the PNG writer): `-TagsFromFile`
/// handed a binary tag's `(Binary data 4 bytes, ...)` placeholder to the
/// writer as text. A copy now converts what pinned 13.59 copies -- the value
/// as it prints it, or a binary tag's data -- and copies only what 13.59
/// copies (#957 round 8): Exif.pm's 0xa40b DeviceSettingDescription has no
/// `Writable`, so pinned 13.59 `-TagsFromFile SRC -ExifIFD:DeviceSettingDescription
/// -ExifIFD:ColorSpace` onto t/images/Nikon.jpg writes no 0xa40b and
/// ColorSpace as its SHORT (`-n` 2).
#[test]
fn copy_metadata_copies_what_exiftool_copies() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("Nikon.jpg") else {
        return;
    };
    let dest_bytes = std::fs::read(&path).expect("Nikon.jpg should be readable");
    // IFD0 at 8: Make "Canon", ExifOffset -> 38; ExifIFD: 0xa001 = 2 (SHORT),
    // 0xa40b = 01 02 03 04 (undef[4]).
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend(2u16.to_le_bytes());
    tiff.extend([0x0f, 0x01, 2, 0, 6, 0, 0, 0, 38, 0, 0, 0]);
    tiff.extend([0x69, 0x87, 4, 0, 1, 0, 0, 0, 44, 0, 0, 0]);
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(b"Canon\0");
    tiff.extend(2u16.to_le_bytes());
    tiff.extend([0x01, 0xa0, 3, 0, 1, 0, 0, 0, 2, 0, 0, 0]);
    tiff.extend([0x0b, 0xa4, 7, 0, 4, 0, 0, 0, 1, 2, 3, 4]);
    tiff.extend(0u32.to_le_bytes());
    let mut source = vec![0xff, 0xd8, 0xff, 0xe1];
    source.extend(((tiff.len() + 8) as u16).to_be_bytes());
    source.extend(b"Exif\0\0");
    source.extend(&tiff);
    source.extend([0xff, 0xd9]);
    let src = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile()
        .expect("temp");
    let dst = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile()
        .expect("temp");
    std::fs::write(src.path(), &source).expect("write the source");
    std::fs::write(dst.path(), &dest_bytes).expect("write the destination");
    let source_map = read_metadata(src.path()).expect("the source parses");
    assert_eq!(
        shown(&source_map, "ExifIFD:DeviceSettingDescription").as_deref(),
        Some("(Binary data 4 bytes, use -b option to extract)")
    );
    let tags = [
        "ExifIFD:DeviceSettingDescription".to_string(),
        "ExifIFD:ColorSpace".to_string(),
    ];
    oxidex::core::operations::copy_metadata(src.path(), dst.path(), Some(&tags))
        .expect("the copy succeeds");
    let written = std::fs::read(dst.path()).expect("read back");
    let entries = tiff_entries(&tiff_block_of(&written));
    assert!(
        !entries.iter().any(|(tag, ..)| *tag == 0xa40b),
        "{entries:?}"
    );
    let copied = read_metadata(dst.path()).expect("the destination parses");
    assert_eq!(shown_n(&copied, "ExifIFD:ColorSpace").as_deref(), Some("2"));
}

/// Review finding (E-2, D-2/D-3): a by-name write reaches its entry whether
/// or not the reader surfaces a row for it. D-3 silences an ExifIFD edge
/// (0x02bc ApplicationNotes, DJI_XT2.jpg's) and D-2 withholds a
/// MaxApertureValue its ValueConv cannot numify (0/0 here; SamsungGT-B2710's
/// `2.971 1`); before them the reader surfaced both, and `oxidex
/// -ExifIFD:<name>=value` rewrote the entry and `-ExifIFD:<name>=` deleted
/// it, as pinned ExifTool 13.59 does. Row-less, the edit was refused as a
/// second name for the carried entry and the deletion silently kept it.
/// Holds with and without D-2/D-3, so it survives reverting either.
#[test]
fn row_less_exif_ifd_entries_are_still_written_and_deleted_by_name() {
    // II; IFD0 at 8: ExifOffset -> 26; ExifIFD at 26: 0x02bc UNDEFINED[8]
    // at 68, 0x9202 RATIONAL 5/1 at 76, 0x9205 RATIONAL 0/0 at 84.
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend(1u16.to_le_bytes());
    tiff.extend([0x69, 0x87, 4, 0, 1, 0, 0, 0]);
    tiff.extend(26u32.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(3u16.to_le_bytes());
    for (tag, ty, count, at) in [
        (0x02bcu16, 7u16, 8u32, 68u32),
        (0x9202, 5, 1, 76),
        (0x9205, 5, 1, 84),
    ] {
        tiff.extend(tag.to_le_bytes());
        tiff.extend(ty.to_le_bytes());
        tiff.extend(count.to_le_bytes());
        tiff.extend(at.to_le_bytes());
    }
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(b"<x:xmp/>");
    for (num, den) in [(5u32, 1u32), (0, 0)] {
        tiff.extend(num.to_le_bytes());
        tiff.extend(den.to_le_bytes());
    }
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
    jpeg.extend(((tiff.len() + 8) as u16).to_be_bytes());
    jpeg.extend(b"Exif\0\0");
    jpeg.extend(&tiff);
    jpeg.extend([0xff, 0xd9]);

    let write = |arg: &str| {
        let file = tempfile::Builder::new()
            .suffix(".jpg")
            .tempfile()
            .expect("temp file");
        std::fs::write(file.path(), &jpeg).expect("write the carrier");
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .arg(arg)
            .arg(file.path())
            .output()
            .expect("run oxidex");
        assert!(
            out.status.success(),
            "{arg}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let written = std::fs::read(file.path()).expect("read back");
        tiff_entries(&tiff_block_of(&written))
    };
    let ids = |entries: &[(u16, u16, u32, Vec<u8>)]| -> Vec<u16> {
        entries
            .iter()
            .map(|e| e.0)
            .filter(|id| *id != 0x8769)
            .collect()
    };

    let edited = write("-ExifIFD:ApplicationNotes=abc");
    assert_eq!(ids(&edited), [0x02bc, 0x9202, 0x9205]);
    let notes = edited.iter().find(|e| e.0 == 0x02bc).unwrap();
    assert_eq!((notes.1, notes.3.as_slice()), (7, &b"abc"[..]));
    assert_eq!(ids(&write("-ExifIFD:ApplicationNotes=")), [0x9202, 0x9205]);

    let edited = write("-ExifIFD:MaxApertureValue=2.8");
    assert_eq!(ids(&edited), [0x02bc, 0x9202, 0x9205]);
    let max = edited.iter().find(|e| e.0 == 0x9205).unwrap();
    assert_eq!(max.1, 5, "RATIONAL, the entry's own type");
    assert_ne!(max.3, [0u8; 8], "the 0/0 is replaced");
    assert_eq!(ids(&write("-ExifIFD:MaxApertureValue=")), [0x02bc, 0x9202]);
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
    // One scanline (filter 0, one black pixel) as a stored deflate block, so
    // the writer, which requires image data, accepts the file.
    chunk(
        &mut png,
        b"IDAT",
        &[
            0x78, 0x01, 0x01, 0x04, 0x00, 0xfb, 0xff, 0, 0, 0, 0, 0x00, 0x04, 0x00, 0x01,
        ],
    );
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
