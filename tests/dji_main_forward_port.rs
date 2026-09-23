//! Remaining DJI forward-port: `DJI::Info` debug records (MakerNotes and
//! APP7 routes) and the `DJI::XMP` (`XMP-drone-dji`) table's renamed,
//! print-converted GPS properties and family-1 group identity.
//!
//! Every expected value below is the pinned ExifTool 13.59 oracle's output
//! (`perl5.38.2 -I<13.59>/lib <13.59>/exiftool -j -G0:1 -a`, with `-n` for
//! the ValueConv column), never a prior note. Source coordinates, pinned
//! 13.59 tree:
//!
//! - `DJI.pm:74-92` `%Image::ExifTool::DJI::Info` (names, `LONG_TAGS`);
//! - `DJI.pm:960-983` `ProcessDJIInfo` (record regex, printable test);
//! - `ExifTool.pm:9310-9318` `HandleTag(MakeTagInfo => 1)` names unknown
//!   records (`awb_dbg_data_v2` -> `Awb_Dbg_Data_V2`);
//! - `MakerNotes.pm:93-98` `MakerNoteDJIInfo` (`/^\[ae_dbg_info:/`);
//! - `ExifTool.pm:8224-8230` APP7 `DJI-DBG\0` (`DirStart 8`, group 0 APP7);
//! - `DJI.pm:160-225` `%Image::ExifTool::DJI::XMP` (`GpsLatitude` ->
//!   `GPSLatitude`, `GpsLongitude` -> `GPSLongitude`, `ToDMS` PrintConv).
//!
//! Carriers (combined-samples/DJI): FC2204 and MAVIC2-ENTERPRISE-ADVANCED
//! carry `DJI::Info` as the whole MakerNote; FC9313 carries it in APP7;
//! FC2204, MAVIC2, FC9313 and M3T carry `drone-dji:GpsLatitude`; FC9313 and
//! M3T carry both `drone-dji:Version` (1.6) and `crs:Version` (7.0).

use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::{MetadataMap, TagValue};
#[path = "common/fixtures.rs"]
mod fixtures;

fn read(file: &str) -> Option<MetadataMap> {
    let name = format!("DJI/{file}");
    let Some(path) = fixtures::pinned_combined_fixture_path(&name) else {
        eprintln!("skipping: combined corpus fixture {name} is absent");
        return None;
    };
    Some(read_metadata(&path).unwrap_or_else(|e| panic!("{file}: {e}")))
}

fn binary(len: usize) -> String {
    format!("(Binary data {len} bytes, use -b option to extract)")
}

/// A tag's printed form: text as stored, a binary value as ExifTool's
/// placeholder.
fn printed(metadata: &MetadataMap, key: &str) -> Option<String> {
    match metadata.get(key)? {
        TagValue::Binary(bytes) => Some(binary(bytes.len())),
        value => value.as_string().map(str::to_string),
    }
}

fn assert_rows(file: &str, metadata: &MetadataMap, expected: &[(&str, String)]) {
    for (key, value) in expected {
        assert_eq!(
            printed(metadata, key).as_deref(),
            Some(value.as_str()),
            "{file} {key}"
        );
    }
}

/// The family-1 group of the visible occurrence stored under `key`.
fn group1(metadata: &MetadataMap, key: &str) -> Option<String> {
    let winner = metadata.get(key)?;
    metadata
        .project_occurrences(ValueChannel::PrintConv)
        .find(|(k, _, value)| *k == key && value.as_ref() == winner)
        .map(|(_, occurrence, _)| occurrence.group1.to_string())
}

#[test]
fn dji_info_makernote_records_match_pinned_exiftool() {
    // `exiftool -j -G0:1 -a -MakerNotes:all DJI_FC2204.jpg`
    if let Some(metadata) = read("DJI_FC2204.jpg") {
        assert_rows(
            "DJI_FC2204.jpg",
            &metadata,
            &[
                ("DJI:AEDebugInfo", binary(256)),
                ("DJI:AEHistogramInfo", binary(4096)),
                ("DJI:AELocalHistogram", binary(2048)),
                ("DJI:AELiveViewHistogramInfo", binary(4096)),
                ("DJI:AELiveViewLocalHistogram", binary(2048)),
                ("DJI:AWBDebugInfo", binary(4096)),
                ("DJI:AFDebugInfo", binary(256)),
                ("DJI:Histogram", "disable, ".to_string()),
                ("DJI:Xidiri", binary(512)),
                ("DJI:GimbalDegree", "1279,-900,0".to_string()),
                ("DJI:FlightDegree", "1321,81,37".to_string()),
                ("DJI:ADJDebugInfo", binary(1024)),
                ("DJI:SensorID", "0K8HF7H00102EB".to_string()),
                ("DJI:FlightSpeed", "53,-55,0".to_string()),
                ("DJI:HyperlapsDebugInfo", binary(8)),
            ],
        );
    }
    // `exiftool -j -G0:1 -a -MakerNotes:all DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg`
    if let Some(metadata) = read("DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg") {
        assert_rows(
            "DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg",
            &metadata,
            &[
                ("DJI:AEDebugInfo", binary(256)),
                ("DJI:AEHistogramInfo", binary(4096)),
                ("DJI:AELocalHistogram", binary(2048)),
                ("DJI:AELiveViewHistogramInfo", binary(4096)),
                ("DJI:AELiveViewLocalHistogram", binary(2048)),
                ("DJI:AWBDebugInfo", binary(4096)),
                ("DJI:AFDebugInfo", binary(256)),
                ("DJI:Histogram", binary(1024)),
                ("DJI:Xidiri", binary(512)),
                ("DJI:GimbalDegree", "-69,-900,0".to_string()),
                ("DJI:FlightDegree", "-7,-45,-28".to_string()),
                ("DJI:ADJDebugInfo", binary(1024)),
                ("DJI:SensorID", "1TCTJ8803BJ07G".to_string()),
                ("DJI:FlightSpeed", "9,0,0".to_string()),
                ("DJI:HyperlapsDebugInfo", binary(8)),
            ],
        );
    }
}

#[test]
fn dji_info_app7_records_match_pinned_exiftool() {
    // `exiftool -j -G0:1 -a -APP7:all DJI_FC9313.jpg` -- every row is
    // `APP7:DJI:<Name>`; the three MakeTagInfo names are not in the table.
    let Some(metadata) = read("DJI_FC9313.jpg") else {
        return;
    };
    let expected = [
        ("APP7:AEDebugInfo", binary(10240)),
        ("APP7:AEHistogramInfo", binary(1024)),
        ("APP7:AELocalHistogram", binary(2048)),
        ("APP7:AELiveViewHistogramInfo", binary(2048)),
        ("APP7:AELiveViewLocalHistogram", binary(10000)),
        ("APP7:Awb_Dbg_Data_V2", binary(14336)),
        ("APP7:AFDebugInfo", binary(5120)),
        ("APP7:Histogram", binary(1024)),
        ("APP7:ADJDebugInfo", binary(4096)),
        ("APP7:SensorID", "98JFN4G5S00G2S".to_string()),
        ("APP7:HyperlapsDebugInfo", binary(8)),
        ("APP7:Scap_Info", binary(8192)),
        ("APP7:Sisr_Info", binary(4096)),
    ];
    assert_rows("DJI_FC9313.jpg", &metadata, &expected);
    for (key, _) in &expected {
        assert_eq!(
            group1(&metadata, key).as_deref(),
            Some("DJI"),
            "DJI_FC9313.jpg {key} family-1 group"
        );
    }
}

#[test]
fn drone_dji_gps_properties_are_renamed_and_print_converted() {
    // (file, GPSLatitude print, -n, GPSLongitude print, -n) --
    // `exiftool -a -G1 -s [-n] -XMP-drone-dji:GPSLatitude -XMP-drone-dji:GPSLongitude`
    let rows = [
        (
            "DJI_FC2204.jpg",
            "32 deg 2' 5.34\" N",
            "+32.0348174",
            "34 deg 47' 47.33\" E",
            "+34.7964801",
        ),
        (
            "DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg",
            "30 deg 34' 46.41\" N",
            "+30.5795583",
            "72 deg 54' 4.77\" E",
            "+72.9013252",
        ),
        (
            "DJI_FC9313.jpg",
            "51 deg 7' 28.28\" N",
            "+51.124522459",
            "6 deg 21' 47.54\" E",
            "+6.363205003",
        ),
        (
            "DJI_M3T.jpg",
            "33 deg 59' 44.31\" N",
            "+33.995641872",
            "118 deg 25' 18.90\" W",
            "-118.421917247",
        ),
    ];
    for (file, lat, lat_n, lon, lon_n) in rows {
        let Some(metadata) = read(file) else {
            continue;
        };
        let raw = metadata.without_print_conv();
        for (key, print, value) in [
            ("XMP-drone-dji:GPSLatitude", lat, lat_n),
            ("XMP-drone-dji:GPSLongitude", lon, lon_n),
        ] {
            assert_eq!(metadata.get_string(key), Some(print), "{file} {key}");
            assert_eq!(raw.get_string(key), Some(value), "{file} {key} -n");
            assert_eq!(
                group1(&metadata, key).as_deref(),
                Some("XMP-drone-dji"),
                "{file} {key} family-1 group"
            );
        }
        // The schema's own spelling is not a tag ExifTool reports.
        for stale in ["XMP:GpsLatitude", "XMP:GpsLongitude"] {
            assert_eq!(metadata.get(stale), None, "{file} {stale}");
        }
    }
}

#[test]
fn drone_dji_and_crs_version_are_distinct_family1_tags() {
    // `exiftool -a -G1 -s -XMP:Version`: [XMP-drone-dji] 1.6, [XMP-crs] 7.0.
    for file in ["DJI_FC9313.jpg", "DJI_M3T.jpg"] {
        let Some(metadata) = read(file) else {
            continue;
        };
        assert_eq!(
            metadata.get_string("XMP-drone-dji:Version"),
            Some("1.6"),
            "{file}"
        );
        assert_eq!(metadata.get_string("XMP:Version"), Some("7.0"), "{file}");
        assert_eq!(
            group1(&metadata, "XMP:Version").as_deref(),
            Some("XMP-crs"),
            "{file}"
        );
    }
}

#[test]
fn drone_dji_typed_numeric_text_is_kept_verbatim() {
    // XMP `real` properties without a PrintConv print the packet's text:
    // `exiftool -a -G1 -s -XMP-drone-dji:all` on DJI_FC330.jpg / DJI_M3T.jpg.
    if let Some(metadata) = read("DJI_FC330.jpg") {
        assert_rows(
            "DJI_FC330.jpg",
            &metadata,
            &[
                ("XMP-drone-dji:FlightXSpeed", "+0.00".to_string()),
                ("XMP-drone-dji:FlightYSpeed", "+0.00".to_string()),
                ("XMP-drone-dji:FlightZSpeed", "-0.30".to_string()),
            ],
        );
    }
    if let Some(metadata) = read("DJI_M3T.jpg") {
        assert_rows(
            "DJI_M3T.jpg",
            &metadata,
            &[
                ("XMP-drone-dji:FlightXSpeed", "0.0".to_string()),
                ("XMP-drone-dji:FlightYSpeed", "0.1".to_string()),
                ("XMP-drone-dji:FlightZSpeed", "0.0".to_string()),
                ("XMP-drone-dji:GimbalYawDegree", "-133.40".to_string()),
                (
                    "XMP-drone-dji:UTCAtExposure",
                    "2022:10:27 05:08:32.100476".to_string(),
                ),
                ("XMP-drone-dji:DroneModel", "M3T".to_string()),
            ],
        );
    }
}
