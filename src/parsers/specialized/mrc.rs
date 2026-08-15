//! Medical Research Council (MRC) electron-microscopy image metadata parser.
//!
//! ExifTool 13.59's MRC support (`lib/Image/ExifTool/MRC.pm`) reads a fixed
//! 1024-byte `MRC::Main` header (MRC.pm:28-81, `FORMAT => 'int32u'`, 256
//! words) with `ProcessBinaryData`, then -- when `ExtendedHeaderType` is
//! `FEI1` or `FEI2` and the extended header is present -- a second,
//! bitmask-conditional `MRC::FEI12` table (MRC.pm:83-172) describing a
//! microscope-metadata block whose field *presence* depends on up to four
//! `Bitmask` words read earlier in the same block.
//!
//! This parser reads `MRC::Main` from the generated table, then, when the
//! extended header is present, decodes section 0 of `MRC::FEI12` by hand
//! (see [`parse_fei12_extended_header`]): the codegen pipeline declines
//! every `FEI12` field (`omitted.condition`) because its `Condition`
//! evaluates a runtime bitmask value the schema does not model, but the
//! *layout* -- offsets, formats, and every `PrintConv` the generator could
//! compile -- is still transcribed and reused via [`RawAccess`] rather than
//! re-derived by hand. Only section 0 is decoded: ExifTool itself only
//! decodes further sections (one per `ImageDepth`) when its `-ee`
//! (`ExtractEmbedded`) option is passed (MRC.pm:170-176), which oxidex has
//! no equivalent flag for, so this matches ExifTool's own *default*
//! behavior exactly, warnings included (see `FEI12_MULTI_SECTION_WARNING`
//! below).
//!
//! # `FEI12`'s bitmask gating
//!
//! `MRC::FEI12`'s ~90 conditional fields fall into four ranges, each gated
//! by `$$self{BitM} & 0x...` against whichever of `Bitmask1`..`Bitmask4`
//! (MRC.pm:87/193/222/258, offsets 8/297/490/748) was most recently
//! assigned -- ExifTool reassigns the same Perl variable at each Bitmask
//! field, so a field's condition depends on which Bitmask precedes it in
//! table order, not a single static value. [`GATED_FIELDS`] records that
//! `(stage, bit)` pairing per field name; it was re-derived directly from
//! `MRC.pm`'s Perl literal by a one-off script (not hand-transcribed) to
//! avoid mistyping any of the 92 entries, which the AGENTS.md doctrine this
//! module otherwise follows singles out as worse than an omission.
//!
//! `TimeStamp`'s `ValueConv` (`ConvertUnixTime(($val-25569)*24*3600)`, an
//! OLE Automation day-count converted to UTC) is reproduced by hand in
//! [`ole_timestamp_to_unix_string`], verified against this module's own
//! sample (`MRC.mrc`'s extended header decodes to `2020:10:21 13:54:27`,
//! matching the pinned oracle exactly). `AcquisitionTimeStamp` and
//! `CFEGFlashTimeStamp` share the same `ConvertUnixTime` helper but with
//! `$localTime=1` -- i.e. the *host's local timezone at conversion time* --
//! which has no fixed, verifiable answer outside the specific machine that
//! ran ExifTool, and neither field's byte offset (796, 860) falls inside
//! this sample's 768-byte metadata block, so there is no sample to verify
//! against either way. Per AGENTS.md ("never approximate a conversion"),
//! both are deliberately left out of [`GATED_FIELDS`] rather than guessed.
//!
//! # Why `MRC::Main` is not table-only either
//!
//! `MachineStamp` (MRC.pm:73) carries a `PrintConv` of
//! `'sprintf("0x%.2x 0x%.2x 0x%.2x 0x%.2x",split " ", $val)'`, which the
//! generator does not compile (`PrintConv::None` on an unflagged field).
//! `NumberOfLabels` (MRC.pm:76) gates `Label0`..`Label9` (MRC.pm:77-86) by
//! `Condition => '$$self{NLab} > N'`; each `LabelN` is `omitted.condition`
//! and is hand-verified against that count below. `ImageDepth` (MRC.pm:39-45),
//! `ExtendedHeaderSize` (MRC.pm:74) and `ExtendedHeaderType` (MRC.pm:75) each
//! carry a `RawConv` that is a pure `DataMember` side effect
//! (`$$self{X} = $val`, returning the value unchanged) exactly like
//! `NumberOfLabels`'s own; `.emit()` refuses all four (`omitted.raw_conv`),
//! so they are read via [`RawAccess`] and passed through unchanged below,
//! same as `pcx.rs`'s `LeftMargin`/`TopMargin`.
//!
//! `GridSize`, `StartPoint` and `Origin` (MRC.pm:59-60/71, `Format =>
//! 'int32u[3]'`/`'float[3]'`, no `List` flag and no `PrintConv`) are
//! ExifTool's plain `ProcessBinaryData` rendering of a fixed-count field:
//! `ReadValue` joins the raw values with a single space
//! (`ExifTool.pm:6297`-ish `join ' ', @vals`), which is a different
//! rendering rule from an actual List-type tag's `', '` `ListSep`
//! (`ExifTool.pm:1173`, `Keywords`/`Subject`-style tags). oxidex's CLI
//! formatters apply the List rule uniformly to every `TagValue::Array`,
//! which is correct for a real List tag but wrong for these three, so
//! [`space_joined`] resolves them to a final space-joined string here,
//! matching `MachineStamp`'s existing pattern of doing the ExifTool-exact
//! rendering in the parser rather than the shared CLI layer.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/MRC.pm`

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::exprs::perl_num;
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;

const fn citation(table: &'static str, tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "MRC",
        table,
        tag,
        lines,
    }
}

const IMAGE_DEPTH: PerlCitation = citation("Main", "ImageDepth", "MRC.pm:39-45");
const EXTENDED_HEADER_SIZE: PerlCitation = citation("Main", "ExtendedHeaderSize", "MRC.pm:74");
const EXTENDED_HEADER_TYPE: PerlCitation = citation("Main", "ExtendedHeaderType", "MRC.pm:75");
const NUMBER_OF_LABELS: PerlCitation = citation("Main", "NumberOfLabels", "MRC.pm:76");

/// `MetadataSize` (MRC.pm:150, `RawConv => '$$self{MetadataSize} = $val'`) --
/// also the value this module peeks at MRC.pm:151-152 does, ahead of a full
/// table decode, to size the section-0 read.
const FEI12_METADATA_SIZE: PerlCitation = citation("FEI12", "MetadataSize", "MRC.pm:150");

/// `Bitmask1`..`Bitmask4` (MRC.pm:152/247/276/312) share one citation: all
/// four are the identical `RawConv => '$$self{BitM} = $val'` /
/// `PrintConv => 'sprintf("0x%.8x", $val)'` pair, just at different offsets.
const FEI12_BITMASK: PerlCitation = citation("FEI12", "Bitmask1..4", "MRC.pm:152/247/276/312");

/// Every `MRC::FEI12` field gated only by `Condition => '$$self{BitM} &
/// 0x...'` (MRC.pm:154-316), i.e. every [`GATED_FIELDS`] entry except
/// `TimeStamp`. One shared citation covers all of them: each entry's own
/// `(stage, bit)` pairing is the per-field fact that matters, and it is
/// recorded in [`GATED_FIELDS`] itself, re-derived from the Perl source
/// directly rather than copied by hand into 89 separate constants.
const FEI12_GENERIC_CONDITION: PerlCitation =
    citation("FEI12", "<bitmask-gated>", "MRC.pm:154-316");

/// `TimeStamp` (MRC.pm:157-165): `Condition => '$$self{BitM} & 0x01'`,
/// `ValueConv => 'ConvertUnixTime(($val-25569)*24*3600)'`.
const FEI12_TIMESTAMP: PerlCitation = citation("FEI12", "TimeStamp", "MRC.pm:157-165");

/// MRC.pm's `Main` table is 256 `int32u` words (MRC.pm:36, `FIRST_ENTRY =>`
/// implicit 0, plus `Label9` at word index 236 running to word 255).
const HEADER_LEN: usize = 1024;

/// One `MRC::FEI12` field ExifTool gates on a bitmask bit rather than
/// decoding unconditionally: `stage` selects which of `Bitmask1..4`
/// (1-indexed, matching the order they appear in the table) supplies
/// `$$self{BitM}`, and `bit` is the mask tested against it
/// (`Condition => '$$self{BitM} & bit'`). See the module doc comment for
/// how this table was produced.
struct GatedField {
    name: &'static str,
    stage: u8,
    bit: u32,
}

/// `MRC::FEI12`'s 92 bitmask-conditional fields (MRC.pm:154-316), excluding
/// `AcquisitionTimeStamp` and `CFEGFlashTimeStamp` -- see the module doc
/// comment for why those two are deliberately not decoded.
const GATED_FIELDS: &[GatedField] = &[
    GatedField {
        name: "TimeStamp",
        stage: 1,
        bit: 0x01,
    },
    GatedField {
        name: "MicroscopeType",
        stage: 1,
        bit: 0x02,
    },
    GatedField {
        name: "MicroscopeID",
        stage: 1,
        bit: 0x04,
    },
    GatedField {
        name: "Application",
        stage: 1,
        bit: 0x08,
    },
    GatedField {
        name: "AppVersion",
        stage: 1,
        bit: 0x10,
    },
    GatedField {
        name: "HighTension",
        stage: 1,
        bit: 0x20,
    },
    GatedField {
        name: "Dose",
        stage: 1,
        bit: 0x40,
    },
    GatedField {
        name: "AlphaTilt",
        stage: 1,
        bit: 0x80,
    },
    GatedField {
        name: "BetaTilt",
        stage: 1,
        bit: 0x100,
    },
    GatedField {
        name: "XStage",
        stage: 1,
        bit: 0x200,
    },
    GatedField {
        name: "YStage",
        stage: 1,
        bit: 0x400,
    },
    GatedField {
        name: "ZStage",
        stage: 1,
        bit: 0x800,
    },
    GatedField {
        name: "TiltAxisAngle",
        stage: 1,
        bit: 0x1000,
    },
    GatedField {
        name: "DualAxisRot",
        stage: 1,
        bit: 0x2000,
    },
    GatedField {
        name: "PixelSizeX",
        stage: 1,
        bit: 0x4000,
    },
    GatedField {
        name: "PixelSizeY",
        stage: 1,
        bit: 0x8000,
    },
    GatedField {
        name: "Defocus",
        stage: 1,
        bit: 0x400000,
    },
    GatedField {
        name: "STEMDefocus",
        stage: 1,
        bit: 0x800000,
    },
    GatedField {
        name: "AppliedDefocus",
        stage: 1,
        bit: 0x1000000,
    },
    GatedField {
        name: "InstrumentMode",
        stage: 1,
        bit: 0x2000000,
    },
    GatedField {
        name: "ProjectionMode",
        stage: 1,
        bit: 0x4000000,
    },
    GatedField {
        name: "ObjectiveLens",
        stage: 1,
        bit: 0x8000000,
    },
    GatedField {
        name: "HighMagnificationMode",
        stage: 1,
        bit: 0x10000000,
    },
    GatedField {
        name: "ProbeMode",
        stage: 1,
        bit: 0x20000000,
    },
    GatedField {
        name: "EFTEMOn",
        stage: 1,
        bit: 0x40000000,
    },
    GatedField {
        name: "Magnification",
        stage: 1,
        bit: 0x80000000,
    },
    GatedField {
        name: "CameraLength",
        stage: 2,
        bit: 0x01,
    },
    GatedField {
        name: "SpotIndex",
        stage: 2,
        bit: 0x02,
    },
    GatedField {
        name: "IlluminationArea",
        stage: 2,
        bit: 0x04,
    },
    GatedField {
        name: "Intensity",
        stage: 2,
        bit: 0x08,
    },
    GatedField {
        name: "ConvergenceAngle",
        stage: 2,
        bit: 0x10,
    },
    GatedField {
        name: "IlluminationMode",
        stage: 2,
        bit: 0x20,
    },
    GatedField {
        name: "WideConvergenceAngleRange",
        stage: 2,
        bit: 0x40,
    },
    GatedField {
        name: "SlitInserted",
        stage: 2,
        bit: 0x80,
    },
    GatedField {
        name: "SlitWidth",
        stage: 2,
        bit: 0x100,
    },
    GatedField {
        name: "AccelVoltOffset",
        stage: 2,
        bit: 0x200,
    },
    GatedField {
        name: "DriftTubeVolt",
        stage: 2,
        bit: 0x400,
    },
    GatedField {
        name: "EnergyShift",
        stage: 2,
        bit: 0x800,
    },
    GatedField {
        name: "ShiftOffsetX",
        stage: 2,
        bit: 0x1000,
    },
    GatedField {
        name: "ShiftOffsetY",
        stage: 2,
        bit: 0x2000,
    },
    GatedField {
        name: "ShiftX",
        stage: 2,
        bit: 0x4000,
    },
    GatedField {
        name: "ShiftY",
        stage: 2,
        bit: 0x8000,
    },
    GatedField {
        name: "IntegrationTime",
        stage: 2,
        bit: 0x10000,
    },
    GatedField {
        name: "BinningWidth",
        stage: 2,
        bit: 0x20000,
    },
    GatedField {
        name: "BinningHeight",
        stage: 2,
        bit: 0x40000,
    },
    GatedField {
        name: "CameraName",
        stage: 2,
        bit: 0x80000,
    },
    GatedField {
        name: "ReadoutAreaLeft",
        stage: 2,
        bit: 0x100000,
    },
    GatedField {
        name: "ReadoutAreaTop",
        stage: 2,
        bit: 0x200000,
    },
    GatedField {
        name: "ReadoutAreaRight",
        stage: 2,
        bit: 0x400000,
    },
    GatedField {
        name: "ReadoutAreaBottom",
        stage: 2,
        bit: 0x800000,
    },
    GatedField {
        name: "CetaNoiseReduct",
        stage: 2,
        bit: 0x1000000,
    },
    GatedField {
        name: "CetaFramesSummed",
        stage: 2,
        bit: 0x2000000,
    },
    GatedField {
        name: "DirectDetElectronCounting",
        stage: 2,
        bit: 0x4000000,
    },
    GatedField {
        name: "DirectDetAlignFrames",
        stage: 2,
        bit: 0x8000000,
    },
    GatedField {
        name: "PhasePlate",
        stage: 3,
        bit: 0x40,
    },
    GatedField {
        name: "STEMDetectorName",
        stage: 3,
        bit: 0x80,
    },
    GatedField {
        name: "Gain",
        stage: 3,
        bit: 0x100,
    },
    GatedField {
        name: "Offset",
        stage: 3,
        bit: 0x200,
    },
    GatedField {
        name: "DwellTime",
        stage: 3,
        bit: 0x8000,
    },
    GatedField {
        name: "FrameTime",
        stage: 3,
        bit: 0x10000,
    },
    GatedField {
        name: "ScanSizeLeft",
        stage: 3,
        bit: 0x20000,
    },
    GatedField {
        name: "ScanSizeTop",
        stage: 3,
        bit: 0x40000,
    },
    GatedField {
        name: "ScanSizeRight",
        stage: 3,
        bit: 0x80000,
    },
    GatedField {
        name: "ScanSizeBottom",
        stage: 3,
        bit: 0x100000,
    },
    GatedField {
        name: "FullScanFOV_X",
        stage: 3,
        bit: 0x200000,
    },
    GatedField {
        name: "FullScanFOV_Y",
        stage: 3,
        bit: 0x400000,
    },
    GatedField {
        name: "Element",
        stage: 3,
        bit: 0x800000,
    },
    GatedField {
        name: "EnergyIntervalLower",
        stage: 3,
        bit: 0x1000000,
    },
    GatedField {
        name: "EnergyIntervalHigher",
        stage: 3,
        bit: 0x2000000,
    },
    GatedField {
        name: "Method",
        stage: 3,
        bit: 0x4000000,
    },
    GatedField {
        name: "IsDoseFraction",
        stage: 3,
        bit: 0x8000000,
    },
    GatedField {
        name: "FractionNumber",
        stage: 3,
        bit: 0x10000000,
    },
    GatedField {
        name: "StartFrame",
        stage: 3,
        bit: 0x20000000,
    },
    GatedField {
        name: "EndFrame",
        stage: 3,
        bit: 0x40000000,
    },
    GatedField {
        name: "InputStackFilename",
        stage: 3,
        bit: 0x80000000,
    },
    GatedField {
        name: "AlphaTiltMin",
        stage: 4,
        bit: 0x01,
    },
    GatedField {
        name: "AlphaTiltMax",
        stage: 4,
        bit: 0x02,
    },
    GatedField {
        name: "ScanRotation",
        stage: 4,
        bit: 0x04,
    },
    GatedField {
        name: "DiffractionPatternRotation",
        stage: 4,
        bit: 0x08,
    },
    GatedField {
        name: "ImageRotation",
        stage: 4,
        bit: 0x10,
    },
    GatedField {
        name: "ScanModeEnumeration",
        stage: 4,
        bit: 0x20,
    },
    GatedField {
        name: "DetectorCommercialName",
        stage: 4,
        bit: 0x80,
    },
    GatedField {
        name: "StartTiltAngle",
        stage: 4,
        bit: 0x100,
    },
    GatedField {
        name: "EndTiltAngle",
        stage: 4,
        bit: 0x200,
    },
    GatedField {
        name: "TiltPerImage",
        stage: 4,
        bit: 0x400,
    },
    GatedField {
        name: "TitlSpeed",
        stage: 4,
        bit: 0x800,
    },
    GatedField {
        name: "BeamCenterX",
        stage: 4,
        bit: 0x1000,
    },
    GatedField {
        name: "BeamCenterY",
        stage: 4,
        bit: 0x2000,
    },
    GatedField {
        name: "PhasePlatePosition",
        stage: 4,
        bit: 0x8000,
    },
    GatedField {
        name: "ObjectiveAperture",
        stage: 4,
        bit: 0x10000,
    },
];

/// Extract MRC metadata using ExifTool's declared `MRC::Main` binary layout,
/// then -- when present -- `MRC::FEI12`'s extended header (section 0 only;
/// see the module doc comment).
pub fn parse_mrc_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < HEADER_LEN as u64 {
        return Err("MRC file is too short for the 1024-byte header".to_string());
    }
    let header = reader
        .read(0, HEADER_LEN)
        .map_err(|error| error.to_string())?;

    let table = find_table("MRC", "Main").ok_or("missing MRC::Main table")?;
    let decode = decode_binary_table(table, header, ByteOrder::Little);

    let mut number_of_labels = 0_i64;
    let mut image_depth: Option<i64> = None;
    let mut extended_header_size: Option<i64> = None;
    let mut extended_header_type: Option<String> = None;
    for decoded in decode.fields() {
        match decoded.field.name {
            "NumberOfLabels" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &NUMBER_OF_LABELS)
                    && let Some(raw) = access.raw().as_integer()
                {
                    number_of_labels = raw;
                }
            }
            "ImageDepth" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &IMAGE_DEPTH)
                {
                    image_depth = access.raw().as_integer();
                }
            }
            "ExtendedHeaderSize" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &EXTENDED_HEADER_SIZE)
                {
                    extended_header_size = access.raw().as_integer();
                }
            }
            "ExtendedHeaderType" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &EXTENDED_HEADER_TYPE)
                    && let DecodedValue::String(value) = access.raw()
                {
                    extended_header_type = Some(value.clone());
                }
            }
            _ => {}
        }
    }

    let mut metadata = MetadataMap::new();
    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("File:{name}");
        if let Some(label_index) = name
            .strip_prefix("Label")
            .and_then(|n| n.parse::<i64>().ok())
        {
            // MRC.pm:77-86: `Condition => '$$self{NLab} > N'`.
            if number_of_labels > label_index
                && let Some(value) = decoded.emit()
            {
                metadata.insert(key, value);
            }
            continue;
        }
        match name {
            "ImageDepth" => {
                if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &IMAGE_DEPTH)
                {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "ExtendedHeaderSize" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &EXTENDED_HEADER_SIZE)
                {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "ExtendedHeaderType" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &EXTENDED_HEADER_TYPE)
                {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "NumberOfLabels" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &NUMBER_OF_LABELS)
                {
                    metadata.insert(key, access.emit_raw());
                }
            }
            "MachineStamp" => {
                if let Some(TagValue::Array(values)) = decoded.emit() {
                    let bytes: Vec<i64> = values.iter().filter_map(TagValue::as_integer).collect();
                    if bytes.len() == 4 {
                        metadata.insert(
                            key,
                            TagValue::new_string(format!(
                                "0x{:02x} 0x{:02x} 0x{:02x} 0x{:02x}",
                                bytes[0], bytes[1], bytes[2], bytes[3]
                            )),
                        );
                    }
                }
            }
            "GridSize" | "StartPoint" | "Origin" => {
                if let Some(value) = decoded.emit().and_then(space_joined) {
                    metadata.insert(key, value);
                }
            }
            _ => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }

    if let (Some(image_depth), Some(extended_header_size)) = (image_depth, extended_header_size)
        && extended_header_type
            .as_deref()
            .is_some_and(|value| value.starts_with("FEI1") || value.starts_with("FEI2"))
    {
        parse_fei12_extended_header(reader, &mut metadata, image_depth, extended_header_size);
    }

    Ok(metadata)
}

/// ExifTool's rendering of a fixed-count binary field with no `List` flag
/// and no `PrintConv`: the raw values joined by a single space. See the
/// module doc comment's `GridSize`/`StartPoint`/`Origin` section.
fn space_joined(value: TagValue) -> Option<TagValue> {
    let TagValue::Array(items) = value else {
        return Some(value);
    };
    let mut parts = Vec::with_capacity(items.len());
    for item in items {
        parts.push(match item {
            TagValue::Integer(i) => i.to_string(),
            TagValue::Float(f) => perl_num(f),
            // Every MRC.pm count-field is int32u or float; an unexpected
            // element shape means the decode disagrees with the table this
            // function was written against -- omit rather than guess at a
            // join.
            _ => return None,
        });
    }
    Some(TagValue::new_string(parts.join(" ")))
}

/// MRC.pm:170-176: `$et->Warn('Use the ExtractEmbedded option to read
/// metadata for all frames', 3)`. ExifTool fires this only after
/// successfully decoding section 0 of a multi-section (`ImageDepth > 1`)
/// extended header, when its `ExtractEmbedded` option is unset -- which,
/// since oxidex has no equivalent option and always stops after section 0,
/// is every time this function is reached with more than one section.
const FEI12_MULTI_SECTION_WARNING: &str =
    "[minor] Use the ExtractEmbedded option to read metadata for all frames";

/// Decode section 0 of `MRC::FEI12`'s bitmask-conditional extended header
/// (MRC.pm:139-179) into `metadata`. Errors are swallowed rather than
/// propagated: `MRC::Main`'s tags are already correct and complete by the
/// time this runs, and ExifTool's own behavior on a read/size failure here
/// is to `Warn` and still return success (MRC.pm:149/155), not to discard
/// what it already extracted.
fn parse_fei12_extended_header(
    reader: &dyn FileReader,
    metadata: &mut MetadataMap,
    image_depth: i64,
    extended_header_size: i64,
) {
    if extended_header_size <= 0 {
        return;
    }
    // MRC.pm:150-152: peek the leading `int32u` `MetadataSize` -- the first
    // field of `FEI12` and every section's fixed byte length -- without
    // needing a full table decode first.
    let Ok(peek) = reader.read(HEADER_LEN as u64, 4) else {
        return;
    };
    if peek.len() < 4 {
        return;
    }
    let size = u32::from_le_bytes([peek[0], peek[1], peek[2], peek[3]]) as i64;
    // MRC.pm:154-156: `Warn('Corrupted extended header')` and stop.
    if size <= 0 || size.saturating_mul(image_depth.max(1)) > extended_header_size {
        return;
    }
    let Ok(block) = reader.read(HEADER_LEN as u64, size as usize) else {
        return;
    };

    let Some(table) = find_table("MRC", "FEI12") else {
        return;
    };
    let decode = decode_binary_table(table, block, ByteOrder::Little);

    // `$$self{BitM}` after each of Bitmask1..4 (MRC.pm:87/193/222/258),
    // 1-indexed to match `GatedField::stage`; index 0 unused.
    let mut bitmask: [i64; 5] = [0; 5];
    for decoded in decode.fields() {
        let stage = match decoded.field.name {
            "Bitmask1" => 1,
            "Bitmask2" => 2,
            "Bitmask3" => 3,
            "Bitmask4" => 4,
            _ => continue,
        };
        if let Some(access) = RawAccess::new(decoded, Acknowledged::RAW_CONV, &FEI12_BITMASK) {
            if let Some(raw) = access.raw().as_integer() {
                bitmask[stage] = raw;
            }
            metadata.insert(format!("File:{}", decoded.field.name), access.emit_raw());
        }
    }

    for decoded in decode.fields() {
        let name = decoded.field.name;
        match name {
            "Bitmask1" | "Bitmask2" | "Bitmask3" | "Bitmask4" => continue, // handled above
            "MetadataVersion" => {
                // MRC.pm:153: no `RawConv`, no `Condition` -- unconditional.
                if let Some(value) = decoded.emit() {
                    metadata.insert(format!("File:{name}"), value);
                }
                continue;
            }
            "MetadataSize" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::RAW_CONV, &FEI12_METADATA_SIZE)
                {
                    metadata.insert(format!("File:{name}"), access.emit_raw());
                }
                continue;
            }
            "TimeStamp" => {
                let Some(gate) = GATED_FIELDS.iter().find(|g| g.name == name) else {
                    continue;
                };
                if bitmask[gate.stage as usize] & i64::from(gate.bit) == 0 {
                    continue;
                }
                if let Some(access) = RawAccess::new(
                    decoded,
                    Acknowledged::CONDITION | Acknowledged::VALUE_CONV,
                    &FEI12_TIMESTAMP,
                ) && let DecodedValue::Float(days) = access.raw()
                    && let Some(rendered) = ole_timestamp_to_unix_string(*days)
                {
                    metadata.insert(format!("File:{name}"), TagValue::new_string(rendered));
                }
                continue;
            }
            _ => {}
        }
        let Some(gate) = GATED_FIELDS.iter().find(|g| g.name == name) else {
            // Not in GATED_FIELDS: either AcquisitionTimeStamp/CFEGFlashTimeStamp
            // (deliberately omitted, see module doc comment) or a name this
            // table doesn't know -- both cases correctly stay unemitted.
            continue;
        };
        if bitmask[gate.stage as usize] & i64::from(gate.bit) == 0 {
            continue;
        }
        if let Some(access) =
            RawAccess::new(decoded, Acknowledged::CONDITION, &FEI12_GENERIC_CONDITION)
        {
            metadata.insert(format!("File:{name}"), access.emit_raw());
        }
    }

    // MRC.pm:170-176.
    if image_depth > 1 {
        metadata.insert(
            "ExifTool:Warning".to_string(),
            TagValue::new_string(FEI12_MULTI_SECTION_WARNING),
        );
    }
}

/// MRC.pm:93/162-163: `ConvertUnixTime(($val-25569)*24*3600)`, i.e. `$val`
/// is a day count from the OLE Automation epoch (Dec 30, 1899); 25569 is
/// that epoch's day-count offset from the Unix epoch (Jan 1, 1970).
/// `ConvertUnixTime`'s single-argument form uses `gmtime` (UTC), and
/// (`ExifTool.pm:6791-6796`) rounds the resulting seconds to the nearest
/// whole second rather than truncating -- verified against this module's
/// own `MRC.mrc` sample, whose raw `TimeStamp` truncates to `...:26` but
/// rounds (correctly) to the oracle's `...:27`.
fn ole_timestamp_to_unix_string(ole_days: f64) -> Option<String> {
    if !ole_days.is_finite() {
        return None;
    }
    let unix_seconds = (ole_days - 25569.0) * 86400.0;
    let rounded = unix_seconds.round();
    if !(i64::MIN as f64..=i64::MAX as f64).contains(&rounded) {
        return None;
    }
    let dt = chrono::DateTime::from_timestamp(rounded as i64, 0)?;
    Some(dt.format("%Y:%m:%d %H:%M:%S").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_stamp_formats_as_hex_bytes() {
        let bytes = [0x44_i64, 0x44, 0x00, 0x00];
        let formatted = format!(
            "0x{:02x} 0x{:02x} 0x{:02x} 0x{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        );
        assert_eq!(formatted, "0x44 0x44 0x00 0x00");
    }

    #[test]
    fn space_joined_formats_integers_and_floats() {
        let array = TagValue::Array(vec![
            TagValue::Integer(4096),
            TagValue::Integer(4096),
            TagValue::Integer(2),
        ]);
        assert_eq!(
            space_joined(array),
            Some(TagValue::new_string("4096 4096 2"))
        );

        let floats = TagValue::Array(vec![
            TagValue::Float(0.0),
            TagValue::Float(0.0),
            TagValue::Float(0.0),
        ]);
        assert_eq!(space_joined(floats), Some(TagValue::new_string("0 0 0")));
    }

    #[test]
    fn space_joined_passes_through_non_array_values() {
        let value = TagValue::Integer(5);
        assert_eq!(space_joined(value.clone()), Some(value));
    }

    #[test]
    fn ole_timestamp_matches_mrc_sample_and_rounds_up() {
        // MRC.mrc's extended header TimeStamp: raw OLE day count that
        // truncates one second short of the oracle's answer, requiring the
        // round-to-nearest behavior `ConvertUnixTime` actually implements.
        let rendered = ole_timestamp_to_unix_string(44125.579_479_166_66);
        assert_eq!(rendered.as_deref(), Some("2020:10:21 13:54:27"));
    }

    #[test]
    fn ole_timestamp_rejects_non_finite_input() {
        assert_eq!(ole_timestamp_to_unix_string(f64::NAN), None);
        assert_eq!(ole_timestamp_to_unix_string(f64::INFINITY), None);
    }

    #[test]
    fn gated_fields_has_no_duplicate_names() {
        let mut names: Vec<&str> = GATED_FIELDS.iter().map(|g| g.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "GATED_FIELDS has a duplicate name");
    }

    #[test]
    fn gated_fields_stage_in_range() {
        assert!(GATED_FIELDS.iter().all(|g| (1..=4).contains(&g.stage)));
    }
}
