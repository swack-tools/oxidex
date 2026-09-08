//! Slice I-3 gate B regression: the seven Olympus MakerNote sub-tables that
//! pass gate A -- `Equipment`, `CameraSettings`, `RawDevelopment`,
//! `RawDevelopment2`, `ImageProcessing`, `RawInfo`, and `FocusInfo`, whose
//! line followed the other six as its own commit -- are read through their
//! generated `IFD_OLYMPUS_*` tables and the IFD engine, which follows
//! `Olympus::Main`'s 0x2010/0x2020/0x2030/0x2031/0x2040/0x2050/0x3000 edges
//! during the Main walk (`src/exiftool_tables/ifd_engine.rs::descend`), not
//! through the hand `tables::EQUIPMENT` (etc.) walks -- see
//! `src/parsers/tiff/makernotes/olympus.rs::parse_located` (`sub_table_rows`)
//! and the seven lines in `src/exiftool_tables/enabled_ifd.rs`.
//!
//! # Why real carriers and not a builder
//!
//! Same reason as `tests/olympus_main_ifd_table.rs`: a builder writes the
//! bytes its author believes in and cannot observe a conversion defect.
//! Every expectation below is the pinned ExifTool 13.59 oracle's own output
//! for files this repository did not write. Both probes were run first, as
//! `AGENTS.md` requires:
//!
//! ```text
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -ver
//! 13.59
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -s3 -FileType /tmp/oxidex-exiftool-cache/exiftool/t/images/OOXML.docx
//! DOCX
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -a -G1 -s /tmp/oxidex-exiftool-cache/exiftool/t/images/OlympusE1.jpg
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -a -G1 -s /tmp/oxidex-exiftool-cache/exiftool/t/images/Olympus2.jpg
//! ```
//!
//! Every value is the `-j -G1` form (the `-s` text above hides trailing
//! spaces; `InternalSerialNumber` keeps fifteen). The one place the two
//! forms differ the other way -- a numeric-looking string the JSON writer
//! emits as a number, `BodyFirmwareVersion` `1.300` -> `1.3` -- is pinned
//! as the string the map stores and `-s` prints, which the conformance
//! harness's numeric comparison accepts (0 VALUE rows on 184 carriers).
//!
//! Each pinned tag names its producer: `engine` for a row the generated
//! table reports (the line's new path), `residual` for a row the generator
//! withholds and `tables::<TABLE>_RESIDUAL` still hand-converts, `override`
//! for a residual row that replaces an engine rendering the pinned oracle
//! contradicts (`tables::<TABLE>_ENGINE_MISRENDERS`), and `hand` for a
//! FocusInfo row that is not a `TagDef` at all -- `FocusDistance` and the
//! withheld `_variants` alternatives of `AFPoint`, `AFPointDetails` and
//! `SensorTemperature`, which `parse_focus_info_*` still produces while
//! skipping the two alternatives the engine reports
//! (`focus_info_engine_reports`). On the two `t/images` carriers every row
//! is a FOLD -- the hand walk already produced it -- so the assertion is
//! that switching the producer changed no byte. The coverage the lines buy
//! (rows the hand tables never had, the RawDevelopment2 directory, the
//! joined-key overrides on a modern body, FocusInfo's
//! `AntiShockWaitingTime`) has no `t/images` carrier and is pinned on
//! corpus files in an `#[ignore]`d test.

use oxidex::core::MetadataMap;
use oxidex::core::operations::read_metadata;
use oxidex::exiftool_tables::{ENABLED_IFD, find_ifd_table};
use std::path::Path;

const T_IMAGES: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images";
const CORPUS: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Olympus";

/// The seven sub-tables slice I-3 enables.
const SUB_TABLES: [&str; 7] = [
    "Equipment",
    "CameraSettings",
    "RawDevelopment",
    "RawDevelopment2",
    "ImageProcessing",
    "RawInfo",
    "FocusInfo",
];

/// Each allowlist line is the reviewable unit; this asserts every one of the
/// seven is in force, so a revert of any fails loudly here rather than
/// silently switching that directory back to the hand walk.
#[test]
fn olympus_sub_tables_are_on_the_gate_b_allowlist() {
    for name in SUB_TABLES {
        assert!(
            ENABLED_IFD.contains(&("Olympus", name)),
            "ENABLED_IFD must carry the (\"Olympus\", \"{name}\") line"
        );
        let table = find_ifd_table("Olympus", name)
            .unwrap_or_else(|| panic!("Olympus::{name} is generated"));
        assert!(
            table.gate_a.passes(),
            "Olympus::{name}: gate A must pass: {:?}",
            table.gate_a.blocked_by
        );
        assert!(
            table.enabled(),
            "Olympus::{name} must be enabled (gate A and the gate B line together)"
        );
    }
}

/// The sub-table lines are reached only through the Main walk (nothing
/// walks a sub-table at top level), so the Main line is a precondition of
/// every pin below; without it the six lines would enable nothing and the
/// hand walks would silently return.
#[test]
fn olympus_main_line_is_still_in_force_for_the_edges() {
    assert!(
        find_ifd_table("Olympus", "Main").is_some_and(|t| t.enabled()),
        "the sub-table lines depend on the (\"Olympus\", \"Main\") line"
    );
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

/// `OlympusE1.jpg` (E-1, `OLYMP\0` header with sub-IFDs: Equipment,
/// CameraSettings, RawDevelopment, ImageProcessing, FocusInfo).
///
/// ```text
/// [Olympus]       EquipmentVersion                : 010
/// [Olympus]       CameraType2                     : E-1
/// [Olympus]       SerialNumber                    : 143008611
/// [Olympus]       InternalSerialNumber            : 4001310004375102
/// [Olympus]       FocalPlaneDiagonal              : 21.6 mm
/// [Olympus]       BodyFirmwareVersion             : 1.300
/// [Olympus]       LensType                        : Olympus Zuiko Digital 14-54mm F2.8-3.5
/// [Olympus]       LensSerialNumber                : 050105886
/// [Olympus]       LensFirmwareVersion             : 1.100
/// [Olympus]       MaxApertureAtMaxFocal           : 3.5
/// [Olympus]       MinFocalLength                  : 14
/// [Olympus]       MaxFocalLength                  : 54
/// [Olympus]       MaxAperture                     : 2.8
/// [Olympus]       LensProperties                  : 0xc043
/// [Olympus]       Extender                        : None
/// [Olympus]       ExtenderFirmwareVersion         : 0
/// [Olympus]       FlashType                       : None
/// [Olympus]       FlashModel                      : None
/// [Olympus]       FlashFirmwareVersion            : 0
/// [Olympus]       CameraSettingsVersion           : 010
/// [Olympus]       PreviewImageValid               : Yes
/// [Olympus]       PreviewImageStart               : 2568
/// [Olympus]       PreviewImageLength              : 26
/// [Olympus]       ExposureMode                    : Program
/// [Olympus]       AELock                          : Off
/// [Olympus]       MeteringMode                    : ESP
/// [Olympus]       MacroMode                       : Off
/// [Olympus]       FocusMode                       : Single AF
/// [Olympus]       FocusProcess                    : AF Used
/// [Olympus]       AFSearch                        : Ready
/// [Olympus]       AFAreas                         : Center (121,121)-(133,133)
/// [Olympus]       FlashMode                       : Off
/// [Olympus]       FlashExposureComp               : 0
/// [Olympus]       WhiteBalance2                   : Auto
/// [Olympus]       WhiteBalanceTemperature         : Auto
/// [Olympus]       WhiteBalanceBracket             : 0
/// [Olympus]       ModifiedSaturation              : Off
/// [Olympus]       ContrastSetting                 : 0 (min -2, max 2)
/// [Olympus]       SharpnessSetting                : 1 (min -3, max 5)
/// [Olympus]       ColorSpace                      : sRGB
/// [Olympus]       NoiseReduction                  : (none)
/// [Olympus]       DistortionCorrection            : Off
/// [Olympus]       ShadingCompensation             : Off
/// [Olympus]       CompressionFactor               : 2.7
/// [Olympus]       DriveMode                       : Continuous Shooting, Shot 2
/// [Olympus]       ImageQuality2                   : SHQ
/// [Olympus]       RawDevVersion                   : 010
/// [Olympus]       RawDevExposureBiasValue         : 0
/// [Olympus]       RawDevGrayPoint                 : 0 0 0
/// [Olympus]       RawDevColorSpace                : sRGB
/// [Olympus]       RawDevEngine                    : High Speed
/// [Olympus]       RawDevNoiseReduction            : (none)
/// [Olympus]       RawDevEditStatus                : Original
/// [Olympus]       RawDevSettings                  : (none)
/// [Olympus]       ImageProcessingVersion          : 010
/// [Olympus]       WB_RBLevels                     : 412 290
/// [Olympus]       WB_GLevel                       : 256
/// [Olympus]       ColorMatrix                     : 356 -34 -66 -22 308 -30 6 -126 376
/// [Olympus]       Enhancer                        : 576
/// [Olympus]       CoringFilter                    : 1536
/// [Olympus]       BlackLevel2                     : 69 69 69 68
/// [Olympus]       GainBase                        : 256
/// [Olympus]       ValidBits                       : 12 0
/// [Olympus]       CropLeft                        : 0 0
/// [Olympus]       CropWidth                       : 2560
/// [Olympus]       CropHeight                      : 1920
/// [Olympus]       NoiseReduction2                 : (none)
/// [Olympus]       DistortionCorrection2           : Off
/// [Olympus]       ShadingCompensation2            : Off
/// [Olympus]       FocusInfoVersion                : 010
/// [Olympus]       SceneDetect                     : 0
/// [Olympus]       ZoomStepCount                   : 3
/// [Olympus]       FocusStepCount                  : 332
/// [Olympus]       FocusDistance                   : 0.705 m
/// [Olympus]       AFPoint                         : Center (horizontal)
/// [Olympus]       AFPointDetails                  : 0
/// [Olympus]       ExternalFlash                   : Off
/// [Olympus]       ExternalFlashBounce             : Bounce or Off
/// [Olympus]       ExternalFlashZoom               : 0
/// [Olympus]       InternalFlash                   : Off
/// [Olympus]       SensorTemperature               : 20 C
/// [Composite]     DOF                             : 0.17 m (0.63 - 0.80 m)
/// [Composite]     FOV                             : 47.2 deg (0.62 m)
/// [Composite]     HyperfocalDistance              : 5.93 m
/// ```
///
/// `FocalPlaneDiagonal`, `ColorMatrix`, `CoringFilter` and `ValidBits` are
/// each reported twice by `-a` (Main and the sub-table, equal values here);
/// the plain name is the LATER one, i.e. the sub-table's, in ExifTool's
/// entry order -- the order the engine's descent reproduces and the
/// residual loop preserves. `CustomSaturation` is not pinned: the E-1's
/// model-conditional PrintConv (Olympus.pm:2092-2099) is a pre-existing gap
/// of the hand row this slice does not touch.
#[test]
fn olympus_e1_jpg_sub_table_tags_match_the_pinned_oracle() {
    let Some(metadata) = t_images_carrier("OlympusE1.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "OlympusE1.jpg",
        &[
            // --- Equipment (0x2010) ---
            // residual: RawConv-stripped undef[4]
            ("Olympus:EquipmentVersion", "010"),
            // engine: string hash (%olympusCameraTypes)
            ("Olympus:CameraType2", "E-1"),
            // residual: `s/\s+$//`
            ("Olympus:SerialNumber", "143008611"),
            // engine: plain `string[32]`, NUL-truncated only, so the 15
            // trailing spaces survive (ExifTool.pm:6306-6308; `-s` hides
            // them, `-j` keeps them)
            (
                "Olympus:InternalSerialNumber",
                "4001310004375102               ",
            ),
            // override: rational64u `"$val mm"`, Equipment's copy last
            ("Olympus:FocalPlaneDiagonal", "21.6 mm"),
            // residual: sprintf("%x") + inserted point
            ("Olympus:BodyFirmwareVersion", "1.300"),
            // residual: split/sprintf ValueConv into %olympusLensTypes
            ("Olympus:LensType", "Olympus Zuiko Digital 14-54mm F2.8-3.5"),
            ("Olympus:LensSerialNumber", "050105886"),
            ("Olympus:LensFirmwareVersion", "1.100"),
            // engine: `sqrt(2)**($val/256)` then sprintf("%.1f")
            ("Olympus:MaxApertureAtMaxFocal", "3.5"),
            ("Olympus:MaxAperture", "2.8"),
            // engine: plain int16u
            ("Olympus:MinFocalLength", "14"),
            ("Olympus:MaxFocalLength", "54"),
            // engine: sprintf("0x%x")
            ("Olympus:LensProperties", "0xc043"),
            // residual: Extender ValueConv into its hash
            ("Olympus:Extender", "None"),
            ("Olympus:ExtenderFirmwareVersion", "0"),
            // engine: int enums
            ("Olympus:FlashType", "None"),
            ("Olympus:FlashModel", "None"),
            ("Olympus:FlashFirmwareVersion", "0"),
            // --- CameraSettings (0x2020) ---
            ("Olympus:CameraSettingsVersion", "010"),
            ("Olympus:PreviewImageValid", "Yes"),
            // residual: IsOffset rows, rebased by absolutise_preview_image_start
            ("Olympus:PreviewImageStart", "2568"),
            ("Olympus:PreviewImageLength", "26"),
            ("Olympus:ExposureMode", "Program"),
            ("Olympus:AELock", "Off"),
            ("Olympus:MeteringMode", "ESP"),
            ("Olympus:MacroMode", "Off"),
            // residual: list-form PrintConvs
            ("Olympus:FocusMode", "Single AF"),
            ("Olympus:FocusProcess", "AF Used"),
            ("Olympus:AFSearch", "Ready"),
            // residual: PrintAFAreas
            ("Olympus:AFAreas", "Center (121,121)-(133,133)"),
            // engine: BITMASK hash, exact 0
            ("Olympus:FlashMode", "Off"),
            ("Olympus:FlashExposureComp", "0"),
            ("Olympus:WhiteBalance2", "Auto"),
            // engine: `$val ? $val : "Auto"`
            ("Olympus:WhiteBalanceTemperature", "Auto"),
            ("Olympus:WhiteBalanceBracket", "0"),
            ("Olympus:ModifiedSaturation", "Off"),
            // engine: `"$v[0] (min $v[1], max $v[2])"`
            ("Olympus:ContrastSetting", "0 (min -2, max 2)"),
            ("Olympus:SharpnessSetting", "1 (min -3, max 5)"),
            ("Olympus:ColorSpace", "sRGB"),
            ("Olympus:NoiseReduction", "(none)"),
            ("Olympus:DistortionCorrection", "Off"),
            ("Olympus:ShadingCompensation", "Off"),
            // engine: rational64u, exact quotient
            ("Olympus:CompressionFactor", "2.7"),
            // residual: q{} sub
            ("Olympus:DriveMode", "Continuous Shooting, Shot 2"),
            ("Olympus:ImageQuality2", "SHQ"),
            // --- RawDevelopment (0x2030) ---
            ("Olympus:RawDevVersion", "010"),
            ("Olympus:RawDevExposureBiasValue", "0"),
            ("Olympus:RawDevGrayPoint", "0 0 0"),
            ("Olympus:RawDevColorSpace", "sRGB"),
            ("Olympus:RawDevEngine", "High Speed"),
            ("Olympus:RawDevNoiseReduction", "(none)"),
            ("Olympus:RawDevEditStatus", "Original"),
            ("Olympus:RawDevSettings", "(none)"),
            // --- ImageProcessing (0x2040) ---
            ("Olympus:ImageProcessingVersion", "010"),
            ("Olympus:WB_RBLevels", "412 290"),
            ("Olympus:WB_GLevel", "256"),
            // engine: `Format => 'int16s'` over an int16u entry
            ("Olympus:ColorMatrix", "356 -34 -66 -22 308 -30 6 -126 376"),
            ("Olympus:Enhancer", "576"),
            ("Olympus:CoringFilter", "1536"),
            ("Olympus:BlackLevel2", "69 69 69 68"),
            ("Olympus:GainBase", "256"),
            ("Olympus:ValidBits", "12 0"),
            ("Olympus:CropLeft", "0 0"),
            ("Olympus:CropWidth", "2560"),
            ("Olympus:CropHeight", "1920"),
            ("Olympus:NoiseReduction2", "(none)"),
            ("Olympus:DistortionCorrection2", "Off"),
            ("Olympus:ShadingCompensation2", "Off"),
            // --- FocusInfo (0x2050) ---
            // residual: RawConv-stripped undef[4]
            ("Olympus:FocusInfoVersion", "010"),
            // engine: plain int16u
            ("Olympus:SceneDetect", "0"),
            ("Olympus:ZoomStepCount", "3"),
            ("Olympus:FocusStepCount", "332"),
            // hand: `Format => 'int32u'` over the rational, `$a / 1000`
            ("Olympus:FocusDistance", "0.705 m"),
            // hand: the third AFPoint alternative (`!~ /^(E-M|OM-)/`),
            // withheld for its E-P1 RawConv
            ("Olympus:AFPoint", "Center (horizontal)"),
            // engine: AFPointDetails' second alternative (older bodies),
            // printed raw; the hand pass skips it
            ("Olympus:AFPointDetails", "0"),
            // engine: `Count => 2` joined-key hash, `'0 0' => 'Off'`
            ("Olympus:ExternalFlash", "Off"),
            ("Olympus:ExternalFlashBounce", "Bounce or Off"),
            // engine: rational64u
            ("Olympus:ExternalFlashZoom", "0"),
            // engine: `Count => -1` joined-key hash
            ("Olympus:InternalFlash", "Off"),
            // hand: the E-1 / count != 1 alternative, `"$val C"`
            ("Olympus:SensorTemperature", "20 C"),
            // Composite: read FocusDistance's ValueConv side channel (0.705)
            ("Composite:DOF", "0.17 m (0.63 - 0.80 m)"),
            ("Composite:FOV", "47.2 deg (0.62 m)"),
            ("Composite:HyperfocalDistance", "5.93 m"),
        ],
    );
}

/// `Olympus2.jpg` (u760, `OLYMPUS\0II` header: Equipment, CameraSettings,
/// RawDevelopment, ImageProcessing, FocusInfo).
///
/// ```text
/// [Olympus]       EquipmentVersion                : 0100
/// [Olympus]       CameraType2                     : u760,S760
/// [Olympus]       SerialNumber                    : C90500055
/// [Olympus]       InternalSerialNumber            : 0171612001128001
/// [Olympus]       FocalPlaneDiagonal              : 7.58 mm
/// [Olympus]       BodyFirmwareVersion             : 1.001
/// [Olympus]       CameraSettingsVersion           : 0100
/// [Olympus]       PreviewImageValid               : No
/// [Olympus]       PreviewImageLength              : 0
/// [Olympus]       FocusMode                       : Single AF
/// [Olympus]       FocusProcess                    : AF Not Used
/// [Olympus]       AFSearch                        : Not Ready
/// [Olympus]       AFAreas                         : none
/// [Olympus]       WhiteBalanceBracket             : 0 0
/// [Olympus]       CustomSaturation                : 0 (min -2, max 2)
/// [Olympus]       SceneMode                       : Standard
/// [Olympus]       CompressionFactor               : 4
/// [Olympus]       Gradation                       : Normal
/// [Olympus]       DriveMode                       : Single Shot
/// [Olympus]       PanoramaMode                    : Off
/// [Olympus]       ManometerPressure               : 0 kPa
/// [Olympus]       ManometerReading                : 0 m, 0 ft
/// [Olympus]       RawDevVersion                   : 0100
/// [Olympus]       RawDevEditStatus                : Original
/// [Olympus]       ImageProcessingVersion          : 0111
/// [Olympus]       WB_RBLevels                     : 536 390
/// [Olympus]       WB_GLevel                       : 0
/// [Olympus]       Enhancer                        : 38
/// [Olympus]       CoringFilter                    : 15
/// [Olympus]       BlackLevel2                     : 63 63 63 63
/// [Olympus]       ShadingCompensation2            : On
/// [Olympus]       FocusInfoVersion                : 0100
/// [Olympus]       SceneDetect                     : 0
/// [Olympus]       ZoomStepCount                   : 13
/// [Olympus]       FocusStepCount                  : 321
/// [Olympus]       FocusStepInfinity               : 320
/// [Olympus]       FocusStepNear                   : 375
/// [Olympus]       FocusDistance                   : 385.8 m
/// [Olympus]       AFPoint                         : Left (or n/a)
/// [Olympus]       ExternalFlash                   : Off
/// [Olympus]       ExternalFlashBounce             : Bounce or Off
/// [Olympus]       ExternalFlashZoom               : 0
/// [Olympus]       InternalFlash                   : Off
/// [Olympus]       ImageStabilization              : On, Mode 2
/// [Composite]     DOF                             : inf (4.46 m - inf)
/// [Composite]     FOV                             : 18.4 deg (124.77 m)
/// [Composite]     HyperfocalDistance              : 4.51 m
/// ```
///
/// `ImageStabilization` here is FocusInfo's 0x1600 (this u760 writes no
/// CameraSettings 0x0604): the generator withholds its `Condition` and no
/// hand row ever existed, so it is MISSING before and after the line and
/// is not pinned -- a follow-up, recorded in `enabled_ifd.rs`.
///
/// `PreviewImageStart` (stored 4294966614, a wrapped negative the oracle
/// prints verbatim) is not pinned here; the Main-slice test covers the
/// preview rows.
#[test]
fn olympus2_jpg_sub_table_tags_match_the_pinned_oracle() {
    let Some(metadata) = t_images_carrier("Olympus2.jpg") else {
        return;
    };
    assert_tags(
        &metadata,
        "Olympus2.jpg",
        &[
            ("Olympus:EquipmentVersion", "0100"),
            ("Olympus:CameraType2", "u760,S760"),
            ("Olympus:SerialNumber", "C90500055"),
            (
                "Olympus:InternalSerialNumber",
                "0171612001128001               ",
            ),
            // Main has no 0x0205 in this file: Equipment's copy alone
            ("Olympus:FocalPlaneDiagonal", "7.58 mm"),
            ("Olympus:BodyFirmwareVersion", "1.001"),
            ("Olympus:CameraSettingsVersion", "0100"),
            ("Olympus:PreviewImageValid", "No"),
            ("Olympus:PreviewImageLength", "0"),
            ("Olympus:FocusMode", "Single AF"),
            ("Olympus:FocusProcess", "AF Not Used"),
            ("Olympus:AFSearch", "Not Ready"),
            ("Olympus:AFAreas", "none"),
            // engine: int16s[2], space-joined
            ("Olympus:WhiteBalanceBracket", "0 0"),
            // residual: the non-E-1 branch of the model-conditional sub
            ("Olympus:CustomSaturation", "0 (min -2, max 2)"),
            ("Olympus:SceneMode", "Standard"),
            ("Olympus:CompressionFactor", "4"),
            // residual: Relist
            ("Olympus:Gradation", "Normal"),
            ("Olympus:DriveMode", "Single Shot"),
            ("Olympus:PanoramaMode", "Off"),
            // engine: `$val / 10` then `"$val kPa"`
            ("Olympus:ManometerPressure", "0 kPa"),
            // residual: split-and-divide ValueConv
            ("Olympus:ManometerReading", "0 m, 0 ft"),
            ("Olympus:RawDevVersion", "0100"),
            ("Olympus:RawDevEditStatus", "Original"),
            ("Olympus:ImageProcessingVersion", "0111"),
            ("Olympus:WB_RBLevels", "536 390"),
            ("Olympus:WB_GLevel", "0"),
            ("Olympus:Enhancer", "38"),
            ("Olympus:CoringFilter", "15"),
            ("Olympus:BlackLevel2", "63 63 63 63"),
            ("Olympus:ShadingCompensation2", "On"),
            // --- FocusInfo (0x2050) ---
            ("Olympus:FocusInfoVersion", "0100"),
            ("Olympus:SceneDetect", "0"),
            ("Olympus:ZoomStepCount", "13"),
            ("Olympus:FocusStepCount", "321"),
            ("Olympus:FocusStepInfinity", "320"),
            ("Olympus:FocusStepNear", "375"),
            // hand: 385800 / 1000
            ("Olympus:FocusDistance", "385.8 m"),
            // hand: third alternative, a zero on a non-E-P1 body is kept
            ("Olympus:AFPoint", "Left (or n/a)"),
            ("Olympus:ExternalFlash", "Off"),
            ("Olympus:ExternalFlashBounce", "Bounce or Off"),
            ("Olympus:ExternalFlashZoom", "0"),
            ("Olympus:InternalFlash", "Off"),
            ("Composite:DOF", "inf (4.46 m - inf)"),
            ("Composite:FOV", "18.4 deg (124.77 m)"),
            ("Composite:HyperfocalDistance", "4.51 m"),
        ],
    );
}

/// The coverage the lines buy, on the corpus. Ignored rather than
/// skip-if-absent: the corpus is not a fixture this test may quietly do
/// without. Run with `cargo test --test olympus_sub_tables_ifd -- --ignored`
/// on a machine that has `/tmp/oxidex-exiftool-cache/combined-samples`.
///
/// `OlympusE-M5.jpg` -- a modern body: rows the hand tables never
/// converted the engine's way (`LensModel`, `ConversionLens`,
/// `ExposureShift`, `AFFineTuneAdj`, `SensorCalibration`, `AspectFrame`,
/// the face-detect block), the four joined-key overrides on a real value
/// (`NoiseFilter`/`PictureModeEffect` `Standard`, `AspectRatio` `4:3`), and
/// the list-form residuals:
///
/// ```text
/// $ exiftool-pinned.sh -a -G1 -s OlympusE-M5.jpg
/// [Olympus]       BodyFirmwareVersion             : .953
/// [Olympus]       LensType                        : Olympus M.Zuiko Digital ED 12-50mm F3.5-6.3 EZ
/// [Olympus]       LensModel                       : OLYMPUS M.12-50mm F3.5-6.3
/// [Olympus]       MaxApertureAtMinFocal           : 3.5
/// [Olympus]       MaxApertureAtMaxFocal           : 6.3
/// [Olympus]       MaxAperture                     : 5.2
/// [Olympus]       LensProperties                  : 0xc140
/// [Olympus]       ExposureShift                   : 0
/// [Olympus]       FocusMode                       : Continuous AF; C-AF, Imager AF
/// [Olympus]       FocusProcess                    : AF Used; 64
/// [Olympus]       AFPointSelected                 : (49%,49%) (49%,49%)
/// [Olympus]       AFFineTune                      : Off
/// [Olympus]       AFFineTuneAdj                   : 0 0 0
/// [Olympus]       FlashRemoteControl              : Off
/// [Olympus]       FlashControlMode                : Off; 0; 0; 0
/// [Olympus]       FlashIntensity                  : n/a (x4)
/// [Olympus]       WhiteBalance2                   : Auto (Keep Warm Color Off)
/// [Olympus]       NoiseReduction                  : Auto
/// [Olympus]       Gradation                       : Normal; User-Selected
/// [Olympus]       PictureMode                     : Natural; 2
/// [Olympus]       PictureModeSaturation           : 0 (min -2, max 2)
/// [Olympus]       PictureModeBWFilter             : n/a
/// [Olympus]       NoiseFilter                     : Standard
/// [Olympus]       ArtFilter                       : Off; 0; 0; 0
/// [Olympus]       PictureModeEffect               : Standard
/// [Olympus]       ImageStabilization              : On, S-IS1 (All Direction Shake IS)
/// [Olympus]       ExtendedWBDetect                : Off
/// [Olympus]       SensorCalibration               : 4016 0
/// [Olympus]       MultipleExposureMode            : Off; 1
/// [Olympus]       AspectRatio                     : 4:3
/// [Olympus]       AspectFrame                     : 0 0 4607 3455
/// [Olympus]       FacesDetected                   : 0 0 0
/// [Olympus]       FaceDetectArea                  : (Binary data 383 bytes, use -b option to extract)
/// [Olympus]       MaxFaces                        : 8 8 0
/// [Olympus]       FaceDetectFrameSize             : 640 480 640 480 0 0
/// [Olympus]       FocusInfoVersion                : 0100
/// [Olympus]       FocusStepInfinity               : 0
/// [Olympus]       FocusStepNear                   : 0
/// [Olympus]       FocusDistance                   : 0.35 m
/// [Olympus]       AFPoint                         : 0
/// [Olympus]       AFPointDetails                  : No Subject Detection; Face Priority; Normal AF; No Eye-AF; No Face Detection; No MF; AF Priority; No Object found; Unknown (0x5)
/// [Olympus]       ManualFlash                     : Off
/// [Olympus]       MacroLED                        : Off
/// [Olympus]       SensorTemperature               : 24 C
/// [Composite]     DOF                             : 0.03 m (0.34 - 0.36 m)
/// ```
///
/// `OlympusE-M1MarkII.jpg` carries the one FocusInfo row the hand table
/// never had, and `OlympusE-P1.jpg` is the body whose `AFPoint` ExifTool
/// drops (Olympus.pm:3447's RawConv on a zero) while reporting the raw
/// `AFPointDetails` of an older body and the calibrated
/// `SensorTemperature`:
///
/// ```text
/// $ exiftool-pinned.sh -a -G1 -s OlympusE-M1MarkII.jpg
/// [Olympus]       AntiShockWaitingTime            : 2000
/// [Olympus]       SensorTemperature               : 34 34 0 C
/// $ exiftool-pinned.sh -a -G1 -s OlympusE-P1.jpg
/// [Olympus]       FocusStepCount                  : 0
/// [Olympus]       FocusDistance                   : 0.905 m
/// [Olympus]       AFPointDetails                  : 8
/// [Olympus]       ExternalFlash                   : On
/// [Olympus]       ExternalFlashBounce             : Direct
/// [Olympus]       SensorTemperature               : 35.5 C
/// [Composite]     DOF                             : 0.15 m (0.84 - 0.99 m)
/// ```
///
/// `OlympusXZ-1.jpg` -- writes BOTH 0x2030 `RawDevelopment` and 0x2031
/// `RawDevelopment2`, under shared names; ExifTool reports the 0x2031 copy
/// last, so it owns the plain name (`-a` shows both). The engine walks the
/// two edges in entry order and the residual loop keeps that order:
///
/// ```text
/// $ exiftool-pinned.sh -a -G1 -s OlympusXZ-1.jpg
/// [Olympus]       RawDevVersion                   : ..            (0x2030)
/// [Olympus]       RawDevEngine                    : High Speed    (0x2030)
/// [Olympus]       RawDevNoiseReduction            : (none)        (0x2030)
/// [Olympus]       RawDevEditStatus                : Original      (0x2030 only)
/// [Olympus]       RawDevSettings                  : (none)        (0x2030 only)
/// [Olympus]       RawDevVersion                   : 0100          (0x2031)
/// [Olympus]       RawDevWhiteBalance              : Unknown (3)   (0x2031 only)
/// [Olympus]       RawDevWBFineAdjustment          : 0 0           (0x2031)
/// [Olympus]       RawDevContrastValue             : 0 -2 2        (0x2031)
/// [Olympus]       RawDevNoiseReduction            : Noise Filter  (0x2031)
/// [Olympus]       RawDevEngine                    : Unknown (2)   (0x2031)
/// [Olympus]       RawDevPictureMode               : Natural       (0x2031 only)
/// [Olympus]       RawDevPM_BWFilter               : Unknown (0)   (0x2031 only)
/// [Olympus]       RawDevAutoGradation             : Off           (0x2031 only)
/// [Olympus]       RawDevPMNoiseFilter             : 2 0 -2 1      (0x2031 only)
/// [Olympus]       RawDevArtFilter                 : Off; 0; 0; 0  (0x2031 only)
/// [Olympus]       AspectRatio                     : Unknown (0 0)
/// ```
#[test]
#[ignore = "requires the combined-samples corpus"]
fn corpus_carriers_gain_the_rows_the_hand_tables_lacked_in_exiftools_order() {
    let em5 = Path::new(CORPUS).join("OlympusE-M5.jpg");
    let metadata = read_metadata(&em5).expect("OlympusE-M5.jpg parses");
    assert_tags(
        &metadata,
        "OlympusE-M5.jpg",
        &[
            ("Olympus:BodyFirmwareVersion", ".953"),
            (
                "Olympus:LensType",
                "Olympus M.Zuiko Digital ED 12-50mm F3.5-6.3 EZ",
            ),
            ("Olympus:LensModel", "OLYMPUS M.12-50mm F3.5-6.3"),
            ("Olympus:MaxApertureAtMinFocal", "3.5"),
            ("Olympus:MaxApertureAtMaxFocal", "6.3"),
            ("Olympus:MaxAperture", "5.2"),
            ("Olympus:LensProperties", "0xc140"),
            ("Olympus:ExposureShift", "0"),
            ("Olympus:FocusMode", "Continuous AF; C-AF, Imager AF"),
            ("Olympus:FocusProcess", "AF Used; 64"),
            ("Olympus:AFPointSelected", "(49%,49%) (49%,49%)"),
            ("Olympus:AFFineTune", "Off"),
            ("Olympus:AFFineTuneAdj", "0 0 0"),
            ("Olympus:FlashRemoteControl", "Off"),
            ("Olympus:FlashControlMode", "Off; 0; 0; 0"),
            ("Olympus:FlashIntensity", "n/a (x4)"),
            ("Olympus:WhiteBalance2", "Auto (Keep Warm Color Off)"),
            ("Olympus:NoiseReduction", "Auto"),
            ("Olympus:Gradation", "Normal; User-Selected"),
            ("Olympus:PictureMode", "Natural; 2"),
            ("Olympus:PictureModeSaturation", "0 (min -2, max 2)"),
            ("Olympus:PictureModeBWFilter", "n/a"),
            ("Olympus:NoiseFilter", "Standard"),
            ("Olympus:ArtFilter", "Off; 0; 0; 0"),
            ("Olympus:PictureModeEffect", "Standard"),
            (
                "Olympus:ImageStabilization",
                "On, S-IS1 (All Direction Shake IS)",
            ),
            ("Olympus:ExtendedWBDetect", "Off"),
            ("Olympus:SensorCalibration", "4016 0"),
            ("Olympus:MultipleExposureMode", "Off; 1"),
            ("Olympus:AspectRatio", "4:3"),
            ("Olympus:AspectFrame", "0 0 4607 3455"),
            ("Olympus:FacesDetected", "0 0 0"),
            (
                "Olympus:FaceDetectArea",
                "(Binary data 383 bytes, use -b option to extract)",
            ),
            ("Olympus:MaxFaces", "8 8 0"),
            ("Olympus:FaceDetectFrameSize", "640 480 640 480 0 0"),
            // --- FocusInfo (0x2050) ---
            ("Olympus:FocusInfoVersion", "0100"),
            ("Olympus:FocusStepInfinity", "0"),
            ("Olympus:FocusStepNear", "0"),
            ("Olympus:FocusDistance", "0.35 m"),
            // engine: AFPoint's fourth alternative (E-Mxxx / OM-x), raw;
            // the hand pass skips it
            ("Olympus:AFPoint", "0"),
            // hand: the E-M/OM AFPointDetails bit-field decomposition,
            // withheld for its ValueConv; the PrintHex miss
            (
                "Olympus:AFPointDetails",
                "No Subject Detection; Face Priority; Normal AF; No Eye-AF; No Face Detection; No MF; AF Priority; No Object found; Unknown (0x5)",
            ),
            // residual: the q{} PrintConv
            ("Olympus:ManualFlash", "Off"),
            ("Olympus:MacroLED", "Off"),
            // hand: the E-M5 alternative
            ("Olympus:SensorTemperature", "24 C"),
            ("Composite:DOF", "0.03 m (0.34 - 0.36 m)"),
        ],
    );

    let em1ii = Path::new(CORPUS).join("OlympusE-M1MarkII.jpg");
    let metadata = read_metadata(&em1ii).expect("OlympusE-M1MarkII.jpg parses");
    assert_tags(
        &metadata,
        "OlympusE-M1MarkII.jpg",
        &[
            // engine: 0x2100, a row `tables::FOCUS_INFO` never had (MISSING
            // before the line on all 21 carriers)
            ("Olympus:AntiShockWaitingTime", "2000"),
            // hand: the count != 1 alternative, `s/ 0 0$//`
            ("Olympus:SensorTemperature", "34 34 0 C"),
        ],
    );

    let ep1 = Path::new(CORPUS).join("OlympusE-P1.jpg");
    let metadata = read_metadata(&ep1).expect("OlympusE-P1.jpg parses");
    assert_tags(
        &metadata,
        "OlympusE-P1.jpg",
        &[
            ("Olympus:FocusStepCount", "0"),
            ("Olympus:FocusDistance", "0.905 m"),
            // engine: AFPointDetails' second alternative, raw
            ("Olympus:AFPointDetails", "8"),
            ("Olympus:ExternalFlash", "On"),
            ("Olympus:ExternalFlashBounce", "Direct"),
            // hand: the calibrated alternative, `84 - 3 * $val / 26`
            ("Olympus:SensorTemperature", "35.5 C"),
            ("Composite:DOF", "0.15 m (0.84 - 0.99 m)"),
        ],
    );
    // hand: the third AFPoint alternative's RawConv drops a zero on the
    // E-P1 (Olympus.pm:3447), and the engine withholds that alternative, so
    // neither producer reports it.
    assert!(
        metadata.get("Olympus:AFPoint").is_none(),
        "OlympusE-P1.jpg: AFPoint must not be reported (RawConv undef)"
    );

    let xz1 = Path::new(CORPUS).join("OlympusXZ-1.jpg");
    let metadata = read_metadata(&xz1).expect("OlympusXZ-1.jpg parses");
    assert_tags(
        &metadata,
        "OlympusXZ-1.jpg",
        &[
            // 0x2031's copies own the plain name
            ("Olympus:RawDevVersion", "0100"),
            ("Olympus:RawDevEngine", "Unknown (2)"),
            ("Olympus:RawDevNoiseReduction", "Noise Filter"),
            ("Olympus:RawDevWBFineAdjustment", "0 0"),
            ("Olympus:RawDevContrastValue", "0 -2 2"),
            // 0x2030-only rows survive untouched
            ("Olympus:RawDevEditStatus", "Original"),
            ("Olympus:RawDevSettings", "(none)"),
            // 0x2031-only rows
            ("Olympus:RawDevWhiteBalance", "Unknown (3)"),
            ("Olympus:RawDevPictureMode", "Natural"),
            ("Olympus:RawDevPM_BWFilter", "Unknown (0)"),
            ("Olympus:RawDevAutoGradation", "Off"),
            ("Olympus:RawDevPMNoiseFilter", "2 0 -2 1"),
            ("Olympus:RawDevArtFilter", "Off; 0; 0; 0"),
            // override: the joined-key miss renders ExifTool's way
            ("Olympus:AspectRatio", "Unknown (0 0)"),
        ],
    );
}
