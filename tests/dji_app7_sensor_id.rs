use oxidex::core::{TagValue, operations::read_metadata};
use std::path::Path;

const DJI_M30T: &str = "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_M30T.jpg";
const DJI_MAVIC2_ENTERPRISE_ADVANCED: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg";

/// ExifTool 13.59 selects DJI::Info for APP7 `DJI-DBG\0` and exposes the
/// bracketed `sensor_id` record unchanged.
#[test]
fn dji_m30t_app7_sensor_id_matches_exiftool() {
    if !Path::new(DJI_M30T).is_file() {
        eprintln!("skipping: corpus fixture not present at {DJI_M30T}");
        return;
    }

    let metadata = read_metadata(Path::new(DJI_M30T)).expect("DJI M30T parses");

    assert_eq!(metadata.get_string("APP7:SensorID"), Some("4XAGJCP02AA007"));
}

/// The Mavic 2 Enterprise Advanced `MakerNoteDJIInfo` record stream carries
/// the `DJI::Info` attitude triples unchanged. These values are from the
/// pinned ExifTool 13.59 `ProcessDJIInfo` output.
#[test]
fn dji_mavic2_app7_attitude_records_match_exiftool() {
    if !Path::new(DJI_MAVIC2_ENTERPRISE_ADVANCED).is_file() {
        eprintln!("skipping: corpus fixture not present at {DJI_MAVIC2_ENTERPRISE_ADVANCED}");
        return;
    }

    let metadata = read_metadata(Path::new(DJI_MAVIC2_ENTERPRISE_ADVANCED))
        .expect("DJI Mavic 2 Enterprise Advanced parses");

    assert_eq!(metadata.get_string("DJI:FlightDegree"), Some("-7,-45,-28"));
    assert_eq!(metadata.get_string("DJI:GimbalDegree"), Some("-69,-900,0"));
}

/// ExifTool's `DJI::Info` table preserves non-printable records as binary
/// values.  The pinned Mavic 2 fixture has 256 bytes of `AEDebugInfo`.
#[test]
fn dji_mavic2_app7_ae_debug_info_is_binary() {
    if !Path::new(DJI_MAVIC2_ENTERPRISE_ADVANCED).is_file() {
        eprintln!("skipping: corpus fixture not present at {DJI_MAVIC2_ENTERPRISE_ADVANCED}");
        return;
    }

    let metadata = read_metadata(Path::new(DJI_MAVIC2_ENTERPRISE_ADVANCED))
        .expect("DJI Mavic 2 Enterprise Advanced parses");

    assert!(matches!(
        metadata.get("DJI:AEDebugInfo"),
        Some(TagValue::Binary(bytes)) if bytes.len() == 256
    ));
}
