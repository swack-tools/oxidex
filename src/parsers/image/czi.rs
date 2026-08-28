//! ZISRAW (CZI) metadata parser -- Zeiss Integrated Software RAW.
//!
//! ExifTool routes `.czi` files through `Image::ExifTool::ZISRAW::ProcessCZI`
//! (ZISRAW.pm:165-201), which validates a `ZISRAWFILE\0{6}` signature, reads
//! the first 100 bytes as `ZISRAW::Main` (ZISRAW.pm:19-41), then follows a
//! 64-bit offset at byte 92 to a `ZISRAWMETADATA` section holding a block of
//! XML.
//!
//! # What comes from the transcription
//!
//! `ZISRAW::Main` is a real `ProcessBinaryData` layout, so all three of its
//! fields are read from the generated table. Two carry a `ValueConv` the
//! transcription declines to model (`unpack("H*",$val)` over a 16-byte GUID,
//! ZISRAW.pm:30-40) and one carries a `PrintConv` it drops
//! (`$val =~ tr/ /./` over an `int32u[2]`, ZISRAW.pm:23-27); all three are
//! hand-implemented below against the cited Perl.
//!
//! # The XML metadata section
//!
//! ZISRAW.pm:184-201 follows the 64-bit offset at byte 92 to a
//! `ZISRAWMETADATA` section, reports its XML block verbatim as the `XML` tag,
//! and hands the block to `XMP::ProcessXMP` against `XMP::XML` with two knobs
//! set:
//!
//! ```perl
//!     $$et{XmpIgnoreProps} = [ 'ImageDocument', 'Metadata', 'Information' ];
//!     $$et{ShortenXmpTags} = \&ShortenTagNames;
//! ```
//!
//! The walk itself is the schema-less one
//! [`crate::parsers::xmp::generic_xml`] already implements; this module
//! supplies those two knobs. `ShortenTagNames` (ZISRAW.pm:44-160) is 106
//! ordered `s///` substitutions over the concatenated property path, and the
//! order and each rule's `/g` flag are both load-bearing -- several rules only
//! match because an earlier rule already rewrote the string. Getting one wrong
//! does not fail loudly: it emits a plausible-looking tag *name* that is not
//! the one ExifTool produces, while the real tag goes missing. They are
//! therefore transcribed mechanically rather than by hand (see
//! [`SHORTEN_RULES`]), and [`tests::pinned_fixture_names_match_the_oracle`]
//! pins the names the pinned `t/images/ZISRAW.czi` actually produces.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/ZISRAW.pm`

use std::sync::LazyLock;

use regex::Regex;

use crate::core::tag_occurrence::Instance;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;
use crate::parsers::xmp::generic_xml::{XmlWalkOptions, extract_xml_properties_with};

/// ZISRAW.pm:173, `$raf->Read($buff, 100) == 100`.
const HEADER_LEN: usize = 100;

/// ZISRAW.pm:174, `$buff =~ /^ZISRAWFILE\0{6}/`.
const CZI_SIGNATURE: &[u8] = b"ZISRAWFILE\0\0\0\0\0\0";

const fn citation(tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "ZISRAW",
        table: "Main",
        tag,
        lines,
    }
}

const ZISRAW_VERSION: PerlCitation = citation("ZISRAWVersion", "ZISRAW.pm:23-27");
const PRIMARY_FILE_GUID: PerlCitation = citation("PrimaryFileGUID", "ZISRAW.pm:30-34");
const FILE_GUID: PerlCitation = citation("FileGUID", "ZISRAW.pm:35-40");

/// Extract ZISRAW (CZI) header metadata (`Image::ExifTool::ZISRAW::ProcessCZI`).
pub fn parse_czi_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < HEADER_LEN as u64 {
        return Err("CZI file is too short for the 100-byte header".to_string());
    }
    let header = reader.read(0, HEADER_LEN).map_err(|e| e.to_string())?;
    if !header.starts_with(CZI_SIGNATURE) {
        return Err("invalid ZISRAW signature".to_string());
    }

    let table = find_table("ZISRAW", "Main").ok_or("missing ZISRAW::Main table")?;
    // ZISRAW.pm:176, `SetByteOrder('II')`.
    let decode = decode_binary_table(table, &header, ByteOrder::Little);

    let mut metadata = MetadataMap::new();
    for decoded in decode.fields() {
        let name = decoded.field.name;
        let key = format!("File:{name}");
        match name {
            // ZISRAW.pm:23-27: `Format => 'int32u[2]'` with
            // `PrintConv => '$val =~ tr/ /./; $val'` -- ExifTool renders an
            // array as space-separated, so the PrintConv turns "1 0" into
            // "1.0". The generator drops this PrintConv, so the raw array
            // reaches here through the ordinary `.emit()` path.
            "ZISRAWVersion" => {
                // ExifTool renders the `int32u[2]` space-separated and the
                // PrintConv transliterates each space to a dot, so the two
                // elements end up joined by ".". Read the decoded array
                // directly rather than re-splitting a rendered string.
                if let Some(access) = RawAccess::new(decoded, Acknowledged::NONE, &ZISRAW_VERSION)
                    && let Some(rendered) = version_string(access.raw())
                {
                    metadata.insert(key, TagValue::new_string(rendered));
                }
            }
            // ZISRAW.pm:30-40: `Format => 'undef[16]'` with
            // `ValueConv => 'unpack("H*",$val)'` -- the 16 raw GUID bytes as
            // lowercase hex, in file order (`H*` is high-nibble-first).
            "PrimaryFileGUID" | "FileGUID" => {
                let cite = if name == "PrimaryFileGUID" {
                    &PRIMARY_FILE_GUID
                } else {
                    &FILE_GUID
                };
                if let Some(access) = RawAccess::new(decoded, Acknowledged::VALUE_CONV, cite)
                    && let DecodedValue::Undefined(bytes) = access.raw()
                {
                    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    metadata.insert(key, TagValue::new_string(hex));
                }
            }
            _ => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(key, value);
                }
            }
        }
    }

    process_metadata_section(reader, header, &mut metadata);

    Ok(metadata)
}

/// ZISRAW.pm:23-27's `int32u[2]` under `PrintConv => '$val =~ tr/ /./; $val'`.
fn version_string(raw: &DecodedValue) -> Option<String> {
    let DecodedValue::Array(items) = raw else {
        return None;
    };
    let parts: Vec<String> = items
        .iter()
        .filter_map(DecodedValue::as_integer)
        .map(|v| v.to_string())
        .collect();
    if parts.len() < 2 {
        return None;
    }
    Some(parts.join("."))
}

// ---------------------------------------------------------------------------
// ZISRAW.pm:44-160, `ShortenTagNames`
// ---------------------------------------------------------------------------

/// ZISRAW.pm:186-199, `$raf->Read($buff, 288)` for the metadata header.
const METADATA_HEADER_LEN: usize = 288;
/// ZISRAW.pm:190, `$buff =~ /^ZISRAWMETADATA\0\0/`.
const METADATA_SIGNATURE: &[u8] = b"ZISRAWMETADATA\0\0";
/// ZISRAW.pm:185, `Get64u(\$buff, 92)` -- the metadata section's file offset.
const METADATA_OFFSET_AT: usize = 92;
/// ZISRAW.pm:191, `Get32u(\$buff, 32)` -- the XML block's length.
const METADATA_LENGTH_AT: usize = 32;
/// ZISRAW.pm:192, `$len < 200000000 or $et->Warn('Metadata section too large')`.
const METADATA_MAX_LEN: u32 = 200_000_000;

/// ZISRAW.pm:196, `$$et{XmpIgnoreProps} = [ 'ImageDocument', 'Metadata', 'Information' ]`.
const XMP_IGNORE_PROPS: &[&str] = &["ImageDocument", "Metadata", "Information"];

/// Every `s///` in `ShortenTagNames` (ZISRAW.pm:48-158), in source order, as
/// `(pattern, replacement, is_global)`.
///
/// The order is load-bearing and so is the `/g` flag: several rules only match
/// because an earlier rule already rewrote the string, and a Perl `s///`
/// without `/g` rewrites the leftmost match only. All 106 are transcribed
/// mechanically from the pinned tree -- `awk '/^sub ShortenTagNames/,/^}/'`
/// over ZISRAW.pm yields exactly 106 substitution lines and nothing else -- so
/// this list is complete by construction rather than by inspection.
#[rustfmt::skip]
const SHORTEN_RULES: &[(&str, &str, bool)] = &[
    (r"^HardwareSetting", "", false),
    (r"^DevicesDevice", "Device", false),
    (r"LightPathNode", "", true),
    (r"Successors", "", true),
    (r"ExperimentExperiment", "Experiment", true),
    (r"ObjectivesObjective", "Objective", false),
    (r"ChannelsChannel", "Channel", false),
    (r"TubeLensesTubeLens", "TubeLens", false),
    (r"^ExperimentHardwareSettingsPoolHardwareSetting", "HardwareSetting", false),
    (r"SharpnessMeasureSetSharpnessMeasure", "Sharpness", false),
    (r"FocusSetupAutofocusSetup", "Autofocus", false),
    (r"TracksTrack", "Track", false),
    (r"ChannelRefsChannelRef", "ChannelRef", false),
    (r"ChangerChanger", "Changer", false),
    (r"ElementsChangerElement", "Changer", false),
    (r"ChangerElements", "Changer", false),
    (r"ContrastChangerContrast", "Contrast", false),
    (r"KeyFunctionsKeyFunction", "KeyFunction", false),
    (r"ManagerContrastManager(Contrast)?", "ManagerContrast", false),
    (r"ObjectiveChangerObjective", "ObjectiveChanger", false),
    (r"ManagerLightManager", "ManagerLight", false),
    (r"WavelengthAreasWavelengthArea", "WavelengthArea", false),
    (r"ReflectorChangerReflector", "ReflectorChanger", false),
    (r"^StageStageAxesStageAxis", "StageAxis", false),
    (r"ShutterChangerShutter", "ShutterChanger", false),
    (r"OnOffChangerOnOff", "OnOffChanger", false),
    (r"UnsharpMaskStateUnsharpMask", "UnsharpMask", false),
    (r"Acquisition", "Acq", false),
    (r"Continuous", "Cont", false),
    (r"Resolution", "Res", false),
    (r"Experiment", "Expt", true),
    (r"Threshold", "Thresh", false),
    (r"Reference", "Ref", false),
    (r"Magnification", "Mag", false),
    (r"Original", "Orig", false),
    (r"FocusSetupFocusStrategySetup", "Focus", false),
    (r"ParametersParameter", "Parameter", false),
    (r"IntervalInfo", "Interval", false),
    (r"ExptBlocksAcqBlock", "AcqBlock", false),
    (r"MicroscopesMicroscope", "Microscope", false),
    (r"TimeSeriesInterval", "TimeSeries", false),
    (r"Interval(.*Interval)", "${1}", false),
    (r"SingleTileRegionsSingleTileRegion", "SingleTileRegion", false),
    (r"AcquisitionMode", "", false),
    (r"DetectorsDetector", "Detector", false),
    (r"Setup", "", false),
    (r"Setting", "", false),
    (r"TrackTrack", "Track", false),
    (r"AnalogOutMaximumsAnalogOutMaximum", "AnalogOutMaximum", false),
    (r"AnalogOutMinimumsAnalogOutMinimum", "AnalogOutMinimum", false),
    (r"DigitalOutLabelsDigitalOutLabelLabel", "DigitalOutLabelLabel", false),
    (r"(VivaTomeOpticalSectionInformation)+VivaTomeOpticalSectionInformation", "VivaTomeOpticalSectionInformation", false),
    (r"FocusDefiniteFocus", "FocusDefinite", false),
    (r"ChangerChanger", "Changer", false),
    (r"Calibration", "Cal", false),
    (r"LightSwitchChangerRLTLSwitch", "LightSwitchChangerRLTL", false),
    (r"Parameters", "", false),
    (r"Fluorescence", "Fluor", false),
    (r"CameraGeometryCameraGeometry", "CameraGeometry", false),
    (r"CameraCamera", "Camera", false),
    (r"DetectorsCamera", "Camera", false),
    (r"FilterChangerLeftChangerEmissionFilter", "LeftChangerEmissionFilter", false),
    (r"SwitchingStatesSwitchingState", "SwitchingState", false),
    (r"Information", "Info", false),
    (r"SubDimensions?", "", true),
    (r"Setups?", "", false),
    (r"Parameters?", "", false),
    (r"Calculate", "Calc", false),
    (r"Visibility", "Vis", false),
    (r"Orientation", "Orient", false),
    (r"ListItems", "Items", false),
    (r"Increment", "Incr", false),
    (r"Parameter", "Param", false),
    (r"(ParfocalParcentralValues)+ParfocalParcentralValue", "Parcentral", false),
    (r"ParcentralParcentral", "Parcentral", false),
    (r"CorrFocusCorrection", "FocusCorr", false),
    (r"(ApoTomeDepthInfo)+Element", "ApoTomeDepth", false),
    (r"(ApoTomeClickStopInfo)+Element", "ApoTomeClickStop", false),
    (r"DepthDepth", "Depth", false),
    (r"(Devices?)+Device", "Device", false),
    (r"(BeamPathNode)+", "BeamPathNode", false),
    (r"BeamPathsBeamPath", "BeamPath", true),
    (r"BeamPathBeamPath", "BeamPath", true),
    (r"Configuration", "Config", false),
    (r"StageAxesStageAxis", "StageAxis", false),
    (r"RangesRange", "Range", false),
    (r"DataGridDatasGridData(Grid)?", "DataGrid", false),
    (r"DataMicroscopeDatasMicroscopeData(Microscope)?", "DataMicroscope", false),
    (r"DataWegaDatasWegaData", "DataWega", false),
    (r"ClickStopPositionsClickStopPosition", "ClickStopPosition", false),
    (r"LightSourcess?LightSource(Settings)?(LightSource)?", "LightSource", false),
    (r"FilterSetsFilterSet", "FilterSet", false),
    (r"EmissionFiltersEmissionFilter", "EmissionFilter", false),
    (r"ExcitationFiltersExcitationFilter", "ExcitationFilter", false),
    (r"FiltersFilter", "Filter", false),
    (r"DichroicsDichroic", "Dichronic", false),
    (r"WavelengthsWavelength", "Wavelength", false),
    (r"MultiTrackSetup", "MultiTrack", false),
    (r"TrackTrack", "Track", false),
    (r"DataGrabberSetup", "DataGrabber", false),
    (r"CameraFrameSetup", "CameraFrame", false),
    (r"TimeSeries(TimeSeries|Setups)", "TimeSeries", false),
    (r"FocusFocus", "Focus", false),
    (r"FocusAutofocus", "Autofocus", false),
    (r"Focus(Hardware|Software)(Autofocus)+", "Autofocus${1}", false),
    (r"AutofocusAutofocus", "Autofocus", false),
];

/// The compiled form of [`SHORTEN_RULES`]. A pattern that fails to compile
/// would silently skip a rule and mint a tag name ExifTool never produces, so
/// the constructor panics instead -- and [`tests::every_shorten_rule_compiles`]
/// turns that into a test failure rather than a runtime one.
static SHORTEN_REGEXES: LazyLock<Vec<(Regex, &'static str, bool)>> = LazyLock::new(|| {
    SHORTEN_RULES
        .iter()
        .map(|(pattern, replacement, global)| {
            let regex = Regex::new(pattern)
                .unwrap_or_else(|error| panic!("ZISRAW shorten rule {pattern:?}: {error}"));
            (regex, *replacement, *global)
        })
        .collect()
});

/// `Image::ExifTool::ZISRAW::ShortenTagNames` (ZISRAW.pm:44-160).
fn shorten_tag_names(name: &str) -> String {
    let mut value = name.to_string();
    for (regex, replacement, global) in SHORTEN_REGEXES.iter() {
        value = if *global {
            regex.replace_all(&value, *replacement).into_owned()
        } else {
            // Perl's `s///` without `/g` rewrites the leftmost match only.
            regex.replace(&value, *replacement).into_owned()
        };
    }
    value
}

/// ZISRAW.pm:184-201, the `ZISRAWMETADATA` section.
fn process_metadata_section(reader: &dyn FileReader, header: &[u8], metadata: &mut MetadataMap) {
    let Some(offset) = header
        .get(METADATA_OFFSET_AT..METADATA_OFFSET_AT + 8)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_le_bytes)
    else {
        return;
    };
    // ZISRAW.pm:185, `my $pos = Get64u(\$buff, 92) or return 1`.
    if offset == 0 {
        return;
    }
    let Ok(section) = reader.read(offset, METADATA_HEADER_LEN) else {
        return;
    };
    if !section.starts_with(METADATA_SIGNATURE) {
        return;
    }
    let Some(len) = section
        .get(METADATA_LENGTH_AT..METADATA_LENGTH_AT + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_le_bytes)
    else {
        return;
    };
    if len == 0 || len >= METADATA_MAX_LEN {
        return;
    }
    let Ok(xml) = reader.read(offset + METADATA_HEADER_LEN as u64, len as usize) else {
        return;
    };

    // ZISRAW.pm:194, `$et->FoundTag('XML', $buff)` -- "extract as a block".
    metadata.insert(
        "XML:XML",
        TagValue::new_string(format!(
            "(Binary data {len} bytes, use -b option to extract)"
        )),
    );

    let options = XmlWalkOptions {
        // `%Image::ExifTool::XMP::XML`'s `GROUPS => { 0 => 'XML', 1 => 'XML' }`.
        group0: "XML",
        xmp_ignore_props: XMP_IGNORE_PROPS,
        shorten: Some(shorten_tag_names),
        ..XmlWalkOptions::default()
    };
    let Ok(properties) = extract_xml_properties_with(xml, &options) else {
        return;
    };
    for property in properties {
        metadata.insert_occurrence(
            format!("{}:{}", property.group1, property.name),
            TagValue::new_string(property.value),
            // `FoundXMP` mints an unknown tag at priority 0 (XMP.pm:3595).
            0,
            &property.group1,
            Instance::default(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rule that fails to compile would be skipped silently and mint a tag
    /// name ExifTool never produces. [`SHORTEN_REGEXES`] panics on that; this
    /// turns it into a test failure and pins the transcribed count.
    #[test]
    fn every_shorten_rule_compiles() {
        assert_eq!(SHORTEN_RULES.len(), 106);
        assert_eq!(SHORTEN_REGEXES.len(), SHORTEN_RULES.len());
    }

    /// Every name the pinned `t/images/ZISRAW.czi` produces, quoted from
    /// `exiftool -a -G1 -s` against the 13.59 tree. The left column is the
    /// property path `GetXMPTagID` builds from that file's XML, the right is
    /// what the oracle prints.
    #[test]
    fn pinned_fixture_names_match_the_oracle() {
        let cases = [
            ("HardwareSettingMicroscopeId", "MicroscopeId"),
            ("HardwareSettingMicroscopeName", "MicroscopeName"),
            (
                "HardwareSettingMicroscopeUniqueName",
                "MicroscopeUniqueName",
            ),
            ("HardwareSettingMicroscopeModel", "MicroscopeModel"),
            (
                "HardwareSettingMicroscopeIsAvailable",
                "MicroscopeIsAvailable",
            ),
            ("HardwareSettingMicroscopeIsBroken", "MicroscopeIsBroken"),
            (
                "HardwareSettingMicroscopeIsBrokenReason",
                "MicroscopeIsBrokenReason",
            ),
            (
                "HardwareSettingMicroscopeMotorization",
                "MicroscopeMotorization",
            ),
            (
                "HardwareSettingMicroscopeIsLightSource",
                "MicroscopeIsLightSource",
            ),
            (
                "HardwareSettingMicroscopeIsLightSink",
                "MicroscopeIsLightSink",
            ),
            (
                "HardwareSettingMicroscopeStandSpecification",
                "MicroscopeStandSpecification",
            ),
            // `s/(Devices?)+Device/Device/` collapses `DevicesDeviceRef`.
            (
                "HardwareSettingMicroscopeDevicesDeviceRefId",
                "MicroscopeDeviceRefId",
            ),
            ("HardwareSettingEyePieceId", "EyePieceId"),
            ("HardwareSettingEyePieceName", "EyePieceName"),
            ("HardwareSettingEyePieceUniqueName", "EyePieceUniqueName"),
            // `s/Magnification/Mag/`.
            ("HardwareSettingEyePieceMagnification", "EyePieceMag"),
            (
                "HardwareSettingEyePieceTotalMagnification",
                "EyePieceTotalMag",
            ),
            (
                "HardwareSettingEyePieceDepthOfField",
                "EyePieceDepthOfField",
            ),
            ("HardwareSettingEyePieceFieldOfView", "EyePieceFieldOfView"),
            (
                "HardwareSettingEyePieceTotalFieldOfView",
                "EyePieceTotalFieldOfView",
            ),
        ];
        for (path, expected) in cases {
            assert_eq!(shorten_tag_names(path), expected, "path {path}");
        }
    }

    /// Two rules whose order is what makes them work, kept honest here because
    /// swapping them would still produce a plausible-looking name.
    #[test]
    fn the_rule_order_is_load_bearing() {
        // `s/Acquisition/Acq/` (rule 28) runs before
        // `s/ExptBlocksAcqBlock/AcqBlock/` (rule 39), which can only match a
        // string the earlier rule already rewrote -- and `s/Experiment/Expt/g`
        // (rule 31) has to have run first as well.
        assert_eq!(
            shorten_tag_names("ExperimentBlocksAcquisitionBlockX"),
            "AcqBlockX"
        );
    }
}
