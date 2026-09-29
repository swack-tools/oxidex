#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::{TagValue, operations::read_metadata};

/// ExifTool 13.59 selects DJI::Info for APP7 `DJI-DBG\0` and exposes the
/// bracketed `sensor_id` record unchanged.
#[test]
#[ignore = "requires pinned combined-samples/DJI/DJI_M30T.jpg"]
fn dji_m30t_app7_sensor_id_matches_exiftool() {
    let path = fixtures::required_combined_fixture_path("DJI/DJI_M30T.jpg");
    let metadata = read_metadata(&path).expect("DJI M30T parses");
    assert_eq!(metadata.get_string("APP7:SensorID"), Some("4XAGJCP02AA007"));
}

/// DJI_M3T.jpg's APP7 `DJI-DBG\0` carries only `sensor_id`; pinned 13.59
/// `exiftool -j -G1 -a -APP7:all` reports `DJI:SensorID` = 5L4SK7A02AA00Q.
#[test]
#[ignore = "requires pinned combined-samples/DJI/DJI_M3T.jpg"]
fn dji_m3t_app7_sensor_id_matches_exiftool() {
    let path = fixtures::required_combined_fixture_path("DJI/DJI_M3T.jpg");
    let metadata = read_metadata(&path).expect("DJI M3T parses");
    assert_eq!(metadata.get_string("APP7:SensorID"), Some("5L4SK7A02AA00Q"));
}

/// The XT2 fixture's XMP `RtkFlag` has no PrintConv, so the pinned native
/// processor reports its text value unchanged.
#[test]
#[ignore = "requires pinned combined-samples/DJI/DJI_XT2.jpg"]
fn dji_xt2_xmp_rtk_flag_matches_exiftool() {
    let path = fixtures::required_combined_fixture_path("DJI/DJI_XT2.jpg");
    let metadata = read_metadata(&path).expect("DJI XT2 parses");
    assert_eq!(metadata.get_string("XMP:RtkFlag"), Some("0"));
}

/// The Mavic 2 Enterprise Advanced `MakerNoteDJIInfo` stream carries these
/// `DJI::Info` attitude triples in the pinned ExifTool 13.59 fixture.
#[test]
#[ignore = "requires pinned combined-samples/DJI/DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg"]
fn dji_mavic2_app7_attitude_records_match_exiftool() {
    let path = fixtures::required_combined_fixture_path("DJI/DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg");
    let metadata = read_metadata(&path).expect("DJI Mavic 2 Enterprise Advanced parses");
    assert_eq!(metadata.get_string("DJI:FlightDegree"), Some("-7,-45,-28"));
    assert_eq!(metadata.get_string("DJI:GimbalDegree"), Some("-69,-900,0"));
    assert_eq!(metadata.get_string("DJI:FlightSpeed"), Some("9,0,0"));
}

/// The pinned Mavic 2 fixture has 256 bytes of non-printable `AEDebugInfo`.
#[test]
#[ignore = "requires pinned combined-samples/DJI/DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg"]
fn dji_mavic2_app7_ae_debug_info_is_binary() {
    let path = fixtures::required_combined_fixture_path("DJI/DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg");
    let metadata = read_metadata(&path).expect("DJI Mavic 2 Enterprise Advanced parses");
    assert!(matches!(
        metadata.get("DJI:AEDebugInfo"),
        Some(TagValue::Binary(bytes)) if bytes.len() == 256
    ));
}
