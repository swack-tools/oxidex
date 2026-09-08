//! Slice I-2 gate B regression: the top-level `Olympus::Main` MakerNote IFD
//! is read through the generated `IFD_OLYMPUS_MAIN` table and the IFD engine
//! (`src/exiftool_tables/ifd_engine.rs`), not through the hand `tables::MAIN`
//! walk -- see `src/parsers/tiff/makernotes/olympus.rs::parse_located` and
//! the `("Olympus", "Main")` line in `src/exiftool_tables/enabled_ifd.rs`.
//!
//! # Why real carriers and not a builder
//!
//! `olympus.rs`'s own unit tests build synthetic MakerNotes, which is fine for
//! offset arithmetic but cannot observe a conversion defect: a builder writes
//! the bytes its author believes in. Every expectation below is the pinned
//! ExifTool 13.59 oracle's own output for files this repository did not
//! write. Both probes were run first, as `AGENTS.md` requires:
//!
//! ```text
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -ver
//! 13.59
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -FileType /tmp/oxidex-exiftool-cache/exiftool/t/images/OOXML.docx
//! File Type                       : DOCX
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -j -G1 /tmp/oxidex-exiftool-cache/exiftool/t/images/Olympus.jpg
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -j -G1 /tmp/oxidex-exiftool-cache/exiftool/t/images/Olympus2.jpg
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -j -G1 /tmp/oxidex-exiftool-cache/exiftool/t/images/OlympusE1.jpg
//! ```
//!
//! The three `t/images` carriers are old bodies (C2000Z, u760, E-1) whose
//! Main directories hold only rows the hand table already converted, so on
//! them the line is a FOLD: the engine must reproduce the hand path's value
//! for every row it now owns, and the two hand producers that survive the
//! fold -- `tables::MAIN_RESIDUAL` (SpecialMode, DigitalZoom, the preview
//! offsets) and `parse_camera_type_and_quality` (CameraType, Quality) -- must
//! still land. Each pinned tag names its producer. The new coverage the line
//! buys (top-level 0x0403-0x0405 and the 0x4000 `MainInfo` re-walk) has no
//! carrier under `t/images`, so it is pinned on two corpus files in an
//! `#[ignore]`d test that is run explicitly on a machine with the corpus.
//!
//! No `Olympus::Main` row is "hand RAW, generated converts": every `TagDef::
//! raw` row of the hand table has no PrintConv/ValueConv in Olympus.pm
//! either, so that class the brief asked to pin does not exist in this table.

use oxidex::cli::tag_resolution::{family1_label, resolve_requested_tags};
use oxidex::core::MetadataMap;
use oxidex::core::operations::read_metadata;
use oxidex::exiftool_tables::{ENABLED_IFD, find_ifd_table};
use std::path::Path;

const T_IMAGES: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images";
const CORPUS: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Olympus";

/// The allowlist line is the reviewable unit; this asserts the line is in
/// force, so a revert of it fails loudly here rather than silently switching
/// the Main directory back to the hand walk.
#[test]
fn olympus_main_is_on_the_gate_b_allowlist() {
    assert!(
        ENABLED_IFD.contains(&("Olympus", "Main")),
        "ENABLED_IFD must carry the (\"Olympus\", \"Main\") line"
    );
    let table = find_ifd_table("Olympus", "Main").expect("Olympus::Main is generated");
    assert!(
        table.gate_a.passes(),
        "gate A must pass: {:?}",
        table.gate_a.blocked_by
    );
    assert!(
        table.enabled(),
        "Olympus::Main must be enabled (gate A and the gate B line together)"
    );
}

/// The seven Olympus sub-tables stay hand-walked in this slice (I-3 lands
/// them); pinning that keeps the engine's refusal of their edges from
/// silently changing when someone adds a line without measuring.
#[test]
fn olympus_sub_tables_are_deliberately_not_enabled_yet() {
    for name in [
        "Equipment",
        "CameraSettings",
        "RawDevelopment",
        "RawDevelopment2",
        "ImageProcessing",
        "FocusInfo",
        "RawInfo",
    ] {
        assert!(
            !ENABLED_IFD.contains(&("Olympus", name)),
            "Olympus::{name} is slice I-3's; it must not be on the allowlist yet"
        );
    }
}

/// Reads a `t/images` carrier through the library's normal entry point, or
/// `None` -- never silently -- when the pinned ExifTool checkout is not
/// present on this machine.
fn t_images_carrier(name: &str) -> Option<MetadataMap> {
    let path = Path::new(T_IMAGES).join(name);
    if !path.is_file() {
        eprintln!(
            "skipping: {} is not present (the pinned ExifTool 13.59 checkout's t/images is not on this machine)",
            path.display()
        );
        return None;
    }
    Some(read_metadata(&path).unwrap_or_else(|e| panic!("{name} parses: {e}")))
}

/// The one-line form `-s` prints: the MakerNote path stores every Olympus
/// value as an already-formatted string, but read integers too so a change
/// of storage shape shows up as a value diff rather than a missing tag.
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

/// `Olympus.jpg` (C2000Z, `OLYMP\0` header, 11 entries):
///
/// ```text
/// "Olympus:SpecialMode": "Normal, Sequence: 0, Panorama: (none)",
/// "Olympus:Quality": "SQ (Low)",
/// "Olympus:Macro": "Off",
/// "Olympus:BWMode": "Off",
/// "Olympus:DigitalZoom": 0.0,
/// "Olympus:FocalPlaneDiagonal": "7.8 mm",
/// "Olympus:LensDistortionParams": "-215 -388 -418 -185 -310 -314",
/// "Olympus:CameraType": "C2000Z",
/// "Olympus:CameraID": "OLYMPUS DIGITAL CAMERA",
/// "Olympus:DataDump": "(Binary data 186 bytes, use -b option to extract)",
/// ```
#[test]
fn olympus_jpg_main_tags_match_the_pinned_oracle() {
    let Some(metadata) = t_images_carrier("Olympus.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "Olympus.jpg",
        &[
            // engine: 0x0202 / 0x0203 int enums
            ("Olympus:Macro", "Off"),
            ("Olympus:BWMode", "Off"),
            // engine: 0x0205 rational64u through PrintConv `"$val mm"`
            ("Olympus:FocalPlaneDiagonal", "7.8 mm"),
            // engine: 0x0206 int16s[6], one space-joined value (ExifTool.pm:6330)
            (
                "Olympus:LensDistortionParams",
                "-215 -388 -418 -185 -310 -314",
            ),
            // engine: 0x0209 `Format => 'string'` over an undef entry, NUL-truncated
            ("Olympus:CameraID", "OLYMPUS DIGITAL CAMERA"),
            // engine: 0x0f00 `Binary => 1` placeholder with the entry's byte length
            (
                "Olympus:DataDump",
                "(Binary data 186 bytes, use -b option to extract)",
            ),
            // MAIN_RESIDUAL: 0x0200 (Perl-sub PrintConv) and 0x0204
            // (`$val.=".0"`), withheld by the generator, hand-converted
            (
                "Olympus:SpecialMode",
                "Normal, Sequence: 0, Panorama: (none)",
            ),
            ("Olympus:DigitalZoom", "0.0"),
            // parse_camera_type_and_quality: 0x0207 (DataMember + Condition)
            // and 0x0201 (PrintConv reads `$$self{CameraType}`), both withheld
            ("Olympus:CameraType", "C2000Z"),
            ("Olympus:Quality", "SQ (Low)"),
        ],
    );
}

/// `Olympus2.jpg` (u760, `OLYMPUS\0II` header, sub-IFDs present):
///
/// ```text
/// "Olympus:SpecialMode": "Normal, Sequence: 0, Panorama: (none)",
/// "Olympus:Quality": "SHQ (Fine)",
/// "Olympus:Macro": "Off",
/// "Olympus:BWMode": "Off",
/// "Olympus:DigitalZoom": 1.0,
/// "Olympus:LensDistortionParams": "0 0 0 0 0 0",
/// "Olympus:CameraType": "u760,S760",
/// "Olympus:CameraID": "OLYMPUS DIGITAL CAMERA         ",
/// "Olympus:SceneMode": "Standard",
/// ```
///
/// `CameraID` keeps its nine trailing spaces: `string` is NUL-truncated
/// only (ExifTool.pm:6306-6308). `SceneMode` here is CameraSettings 0x0509
/// (hand-walked), not Main 0x0403 -- pinned because the engine's Main walk
/// must not displace it (Main has no 0x0403 in this file).
#[test]
fn olympus2_jpg_main_tags_match_the_pinned_oracle() {
    let Some(metadata) = t_images_carrier("Olympus2.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "Olympus2.jpg",
        &[
            ("Olympus:Macro", "Off"),
            ("Olympus:BWMode", "Off"),
            ("Olympus:LensDistortionParams", "0 0 0 0 0 0"),
            ("Olympus:CameraID", "OLYMPUS DIGITAL CAMERA         "),
            (
                "Olympus:SpecialMode",
                "Normal, Sequence: 0, Panorama: (none)",
            ),
            ("Olympus:DigitalZoom", "1.0"),
            ("Olympus:CameraType", "u760,S760"),
            ("Olympus:Quality", "SHQ (Fine)"),
            ("Olympus:SceneMode", "Standard"),
        ],
    );
}

/// `OlympusE1.jpg` (E-1, the richest Main directory of the three: the
/// 0x1000-0x103f block plus every sub-IFD):
///
/// ```text
/// "Olympus:SpecialMode": "Fast, Sequence: 2, Panorama: (none)",
/// "Olympus:Quality": "SHQ (Fine)",
/// "Olympus:Macro": "Off",
/// "Olympus:BWMode": "Off",
/// "Olympus:DigitalZoom": 0.0,
/// "Olympus:LensDistortionParams": "0 0 0 0 0 0",
/// "Olympus:CameraType": "E-1",
/// "Olympus:CameraID": "OLYMPUS DIGITAL CAMERA         ",
/// "Olympus:Sharpness": "Hard",
/// "Olympus:BlackLevel": "69 69 69 68",
/// "Olympus:RedBalance": 1.609375,
/// "Olympus:BlueBalance": 1.1328125,
/// "Olympus:Contrast": "Normal",
/// "Olympus:SharpnessFactor": 576,
/// "Olympus:ColorControl": "96 4096 2944 4096 16 128",
/// "Olympus:OlympusImageWidth": 2560,
/// "Olympus:OlympusImageHeight": 1920,
/// ```
#[test]
fn olympus_e1_jpg_main_tags_match_the_pinned_oracle() {
    let Some(metadata) = t_images_carrier("OlympusE1.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "OlympusE1.jpg",
        &[
            ("Olympus:Macro", "Off"),
            ("Olympus:BWMode", "Off"),
            ("Olympus:LensDistortionParams", "0 0 0 0 0 0"),
            ("Olympus:CameraID", "OLYMPUS DIGITAL CAMERA         "),
            // engine: 0x100f `Priority => 0` -- recorded low priority
            // (`shared::tag_priority`), still reported under its own key
            ("Olympus:Sharpness", "Hard"),
            // engine: 0x1012 int16u[4] (not the ERF-only 0x0401, which the
            // generator withholds on its `$$self{TIFF_TYPE}` Condition)
            ("Olympus:BlackLevel", "69 69 69 68"),
            ("Olympus:Contrast", "Normal"),
            ("Olympus:SharpnessFactor", "576"),
            ("Olympus:ColorControl", "96 4096 2944 4096 16 128"),
            ("Olympus:OlympusImageWidth", "2560"),
            ("Olympus:OlympusImageHeight", "1920"),
            ("Olympus:SpecialMode", "Fast, Sequence: 2, Panorama: (none)"),
            ("Olympus:DigitalZoom", "0.0"),
            ("Olympus:CameraType", "E-1"),
            ("Olympus:Quality", "SHQ (Fine)"),
        ],
    );

    // ExifIFD carries its own Sharpness (0xa40a, raw 2 = "Hard" here too);
    // the Olympus copy is `Priority => 0` (Olympus.pm:1000) and must not win
    // the bare-name projection -- `exiftool -Sharpness` prints the ExifIFD
    // one -- even though it was recorded later (`shared::tag_priority`).
    let resolved = resolve_requested_tags(&metadata, &["Sharpness".to_string()], false);
    assert_eq!(resolved.len(), 1, "one occurrence answers the bare name");
    assert_eq!(family1_label(resolved[0].occurrence), "ExifIFD");

    // 0x1017 RedBalance / 0x1018 BlueBalance carry
    // `ValueConv => '$val=~s/ .*//; $val / 256'` (Olympus.pm:1042-1055): the
    // first element of the `int16u[2]`, over 256. Slice I-2 pinned these as
    // WITHHELD (the grammar did not compile the first-element form, so the
    // generator set `omitted.value_conv` and the engine reported nothing --
    // a MISSING that stayed MISSING rather than an approximation); slice I-3
    // taught the grammar the form, the i7 oracle approved it (verify_exprs
    // PASS 607/607), and the regenerated table now carries the conversion,
    // so the pin is the oracle's value: 412/256 and 290/256.
    let table = find_ifd_table("Olympus", "Main").expect("Olympus::Main");
    for (id, key, want) in [
        (0x1017u16, "Olympus:RedBalance", "1.609375"),
        (0x1018, "Olympus:BlueBalance", "1.1328125"),
    ] {
        let tag = table.tag(id).expect("row is transcribed");
        assert!(
            !tag.omitted.any(),
            "{key}: the generated row must be fully transcribed ({:?})",
            tag.omitted
        );
        assert_eq!(
            shown(&metadata, key).as_deref(),
            Some(want),
            "OlympusE1.jpg: {key}"
        );
    }
}

/// The coverage the line buys, on the corpus (no `t/images` carrier has
/// either shape). Ignored rather than skip-if-absent: the corpus is not a
/// fixture this test may quietly do without. Run with
/// `cargo test --test olympus_main_ifd_table -- --ignored` on a machine that
/// has `/tmp/oxidex-exiftool-cache/combined-samples`.
///
/// `OlympusSP510UZ.jpg` -- rows the hand `MAIN` table never had, at the top
/// level (0x0403 SceneMode, 0x0404 SerialNumber, 0x0405 Firmware):
///
/// ```text
/// $ exiftool-pinned.sh -j -G1 OlympusSP510UZ.jpg
/// "Olympus:SceneMode": "Normal",
/// "Olympus:SerialNumber": "000J92216937",
/// "Olympus:Firmware": 74,
/// "Olympus:OneTouchWB": "On",
/// "Olympus:BWMode": "(none)",
/// ```
///
/// `OlympusFE370.jpg` -- the 0x4000 `MainInfo` re-walk of the same table,
/// which ExifTool performs LAST, so its `WhiteBalanceBracket` (0x0303, `0`)
/// is the one projected over CameraSettings' 0x0503 (`0 0`) -- the ordering
/// `parse_located`'s second `walk_main_through_engine` call exists for:
///
/// ```text
/// $ exiftool-pinned.sh -j -G1 OlympusFE370.jpg
/// "Olympus:SerialNumber": "000N45J00078",
/// "Olympus:WhiteBalanceBracket": 0,
/// "Olympus:Macro": "Off",
/// "Olympus:PreCaptureFrames": 0,
/// "Olympus:DataDump": "(Binary data 2540 bytes, use -b option to extract)",
/// $ exiftool-pinned.sh -a -G1 -s OlympusFE370.jpg | grep WhiteBalanceBracket
/// [Olympus]       WhiteBalanceBracket             : 0 0
/// [Olympus]       WhiteBalanceBracket             : 0
/// ```
#[test]
#[ignore = "requires the combined-samples corpus"]
fn corpus_carriers_gain_the_rows_the_hand_table_lacked_in_exiftools_order() {
    let sp510 = Path::new(CORPUS).join("OlympusSP510UZ.jpg");
    let metadata = read_metadata(&sp510).expect("OlympusSP510UZ.jpg parses");
    assert_tags(
        &metadata,
        "OlympusSP510UZ.jpg",
        &[
            ("Olympus:SceneMode", "Normal"),
            ("Olympus:SerialNumber", "000J92216937"),
            ("Olympus:Firmware", "74"),
            ("Olympus:OneTouchWB", "On"),
            ("Olympus:BWMode", "(none)"),
        ],
    );

    let fe370 = Path::new(CORPUS).join("OlympusFE370.jpg");
    let metadata = read_metadata(&fe370).expect("OlympusFE370.jpg parses");
    assert_tags(
        &metadata,
        "OlympusFE370.jpg",
        &[
            ("Olympus:SerialNumber", "000N45J00078"),
            ("Olympus:WhiteBalanceBracket", "0"),
            ("Olympus:Macro", "Off"),
            ("Olympus:PreCaptureFrames", "0"),
            (
                "Olympus:DataDump",
                "(Binary data 2540 bytes, use -b option to extract)",
            ),
        ],
    );
}
