//! Slice I-6 gate B regression: the top-level `FujiFilm::Main` MakerNote IFD
//! is read through the generated `IFD_FUJIFILM_MAIN` table and the IFD engine
//! for the 88 ids the table reports (all but 0x0000 `Version`) -- see
//! `src/parsers/tiff/makernotes/fujifilm/main_engine.rs` and the
//! `("FujiFilm", "Main")` line in `src/exiftool_tables/enabled_ifd.rs`.
//!
//! # Why real carriers
//!
//! `main_engine.rs`'s unit tests build synthetic notes, which is right for
//! insertion order and cannot observe a conversion defect: a builder writes
//! the bytes its author believes in. Every expectation below is the pinned
//! ExifTool 13.59 oracle's own output for files this repository did not
//! write. Both probes were run first, as `AGENTS.md` requires:
//!
//! ```text
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -ver
//! 13.59
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -s3 -FileType /tmp/oxidex-exiftool-cache/exiftool/t/images/OOXML.docx
//! DOCX
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -j -G1 -a -FujiFilm:all t/images/FujiFilm.jpg
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -j -n -G1 -a -FujiFilm:all t/images/FujiFilm.jpg
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -j -G1 -a -FujiFilm:all t/images/FujiFilm.raf
//! ```
//!
//! Each pinned tag names its producer: `engine` (the generated row, inserted
//! after the hand loop) or `residual` (the hand arm `FUJI_MAIN_RESIDUAL_IDS`
//! keeps).

use oxidex::core::MetadataMap;
use oxidex::core::operations::read_metadata;
use std::path::Path;
use std::process::Command;

const T_IMAGES: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images";
const CORPUS: &str = "/tmp/oxidex-exiftool-cache/combined-samples/FujiFilm";

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

/// `FujiFilm.jpg` (JPEG APP1 path, IFD0 Model and value forms): the 14
/// FujiFilm rows the oracle prints, all equal to the hand path's before the
/// line (a fold) -- the engine must reproduce every one, including
/// `Quality`'s trailing blank.
#[test]
fn fujifilm_jpg_main_tags_match_the_pinned_oracle() {
    let Some(metadata) = carrier(T_IMAGES, "FujiFilm.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "FujiFilm.jpg",
        &[
            // residual: 0x0000 (the engine value is undef/Binary)
            ("FujiFilm:Version", "0130"),
            // engine
            ("FujiFilm:Quality", "NORMAL "),
            ("FujiFilm:Sharpness", "0 (normal)"),
            ("FujiFilm:WhiteBalance", "Auto"),
            ("FujiFilm:FujiFlashMode", "Red-eye reduction"),
            ("FujiFilm:FlashExposureComp", "0"),
            ("FujiFilm:Macro", "Off"),
            ("FujiFilm:FocusMode", "Auto"),
            ("FujiFilm:SlowSync", "Off"),
            ("FujiFilm:PictureMode", "Auto"),
            ("FujiFilm:AutoBracketing", "Off"),
            ("FujiFilm:BlurWarning", "None"),
            ("FujiFilm:FocusWarning", "Good"),
            ("FujiFilm:ExposureWarning", "Good"),
        ],
    );
}

/// `FujiFilm.raf` (the RAF path: detached payload, no Model, a throwaway
/// forms map): the oracle's 26 `-FujiFilm:all` rows.
#[test]
fn fujifilm_raf_main_tags_match_the_pinned_oracle() {
    let Some(metadata) = carrier(T_IMAGES, "FujiFilm.raf") else {
        return;
    };
    assert_tags(
        &metadata,
        "FujiFilm.raf",
        &[
            // residual: 0x0000, 0x0010, 0x100b (= 128, alone)
            ("FujiFilm:Version", "0130"),
            (
                "FujiFilm:InternalSerialNumber",
                "FPX20582698 Y-1146 2007:02:19 8C0020100A84",
            ),
            ("FujiFilm:NoiseReduction", "Normal"),
            // engine
            ("FujiFilm:Quality", "NORMAL "),
            ("FujiFilm:Sharpness", "0 (normal)"),
            ("FujiFilm:WhiteBalance", "Auto"),
            ("FujiFilm:Saturation", "0 (normal)"),
            ("FujiFilm:Contrast", "Normal"),
            ("FujiFilm:WhiteBalanceFineTune", "Red +0, Blue +0"),
            ("FujiFilm:FujiFlashMode", "Off"),
            ("FujiFilm:FlashExposureComp", "0"),
            ("FujiFilm:FocusMode", "Auto"),
            ("FujiFilm:AFMode", "Single Point"),
            ("FujiFilm:SlowSync", "Off"),
            ("FujiFilm:PictureMode", "Manual"),
            ("FujiFilm:ExposureCount", "1"),
            ("FujiFilm:AutoBracketing", "Off"),
            ("FujiFilm:SequenceNumber", "0"),
            ("FujiFilm:DynamicRange", "Wide"),
            ("FujiFilm:FilmMode", "F0/Standard (Provia)"),
            ("FujiFilm:DynamicRangeSetting", "Auto"),
            ("FujiFilm:MinFocalLength", "28"),
            ("FujiFilm:MaxFocalLength", "70"),
            ("FujiFilm:MaxApertureAtMinFocal", "2.8"),
            ("FujiFilm:MaxApertureAtMaxFocal", "2.8"),
            ("FujiFilm:FacesDetected", "0"),
        ],
    );
    // known EXTRA: raf_parser back-fill, not FujiFilm::Main (unchanged by the
    // slice; retiring `raf_parser::parse_raf_makernote` is its own change).
    for key in ["FujiFilm:ColorSpace", "FujiFilm:SensorInfo"] {
        assert!(metadata.get(key).is_some(), "FujiFilm.raf: {key}");
    }
}

/// `--no-print-conv` shows `Emitted::value_conv` for the rows a PrintConv
/// rendered -- the pinned oracle's `-n` for the same tags. oxidex's own `-n`
/// is dry-run, so the CLI spelling is `--no-print-conv`. `Version` and
/// `Quality` have no PrintConv and print the same either way.
#[test]
fn no_print_conv_shows_the_engine_value_conv() {
    let path = Path::new(T_IMAGES).join("FujiFilm.jpg");
    if !path.is_file() {
        eprintln!("skipping: {} is not present", path.display());
        return;
    }
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(["--no-print-conv", "-j", "-G1", "-a"])
        .arg(&path)
        .output()
        .expect("run oxidex");
    assert!(output.status.success(), "oxidex failed: {output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    let object = &json[0];
    for (tag, want) in [
        ("Sharpness", "3"),
        ("WhiteBalance", "0"),
        ("FujiFlashMode", "3"),
        ("Macro", "0"),
        ("FocusMode", "0"),
        ("SlowSync", "0"),
        ("PictureMode", "0"),
        ("AutoBracketing", "0"),
        ("BlurWarning", "0"),
        ("FocusWarning", "0"),
        ("ExposureWarning", "0"),
        ("FlashExposureComp", "0"),
        ("Version", "0130"),
        ("Quality", "NORMAL "),
    ] {
        let key = format!("FujiFilm:{tag}");
        let got = match &object[&key] {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            other => panic!("FujiFilm.jpg: --no-print-conv {key} is {other:?}"),
        };
        assert_eq!(got, want, "FujiFilm.jpg: --no-print-conv {key}");
    }
}

/// Corpus carriers for the classes `t/images` has no example of. Run
/// explicitly on a machine with the corpus:
/// `cargo test --test fujifilm_main_ifd_table -- --ignored`.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/FujiFilm"]
fn corpus_main_tags_match_the_pinned_oracle() {
    for (file, expected) in [
        // engine: 0x1006 Contrast (no hand arm)
        (
            "FujiFilmFinePixS9000.jpg",
            &[("FujiFilm:Contrast", "Normal")][..],
        ),
        // engine: ImageCount `& 0x7fff` (FujiFilm.pm:835-841)
        ("FujiFilmX-E3.jpg", &[("FujiFilm:ImageCount", "13")][..]),
        // engine: PrintHex unknown fallback
        (
            "FujiFilmFinePixA403.jpg",
            &[("FujiFilm:ColorMode", "Unknown (0x1e0)")][..],
        ),
        // engine: a 2-byte scalar read at its declared width
        (
            "FujiFilmS3Pro_UVIR.jpg",
            &[("FujiFilm:Sharpness", "n/a")][..],
        ),
        // engine: 0x100f Clarity
        ("FujiFilmX-H2S.jpg", &[("FujiFilm:Clarity", "0")][..]),
        // engine: RollAngle, a rational
        (
            "FujiFilmGFX100II.jpg",
            &[("FujiFilm:RollAngle", "88.8")][..],
        ),
        // engine: plain int16u ColorTemperature (no " K")
        (
            "FujiFilmX-HF1.jpg",
            &[("FujiFilm:ColorTemperature", "10000")][..],
        ),
    ] {
        if let Some(metadata) = carrier(CORPUS, file) {
            assert_tags(&metadata, file, expected);
        }
    }
    // unsupplied: 0x1446 FlickerReduction (`omitted.print_conv`); the oracle
    // prints `Off (0x0001)`, nothing here produces it.
    if let Some(metadata) = carrier(CORPUS, "FujiFilmX-H2S.jpg") {
        assert_eq!(
            shown(&metadata, "FujiFilm:FlickerReduction"),
            None,
            "FujiFilmX-H2S.jpg: FlickerReduction is unsupplied"
        );
    }
}
