use oxidex::core::{TagValue, operations::read_metadata};
use std::path::Path;

const DJI_M30T: &str = "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_M30T.jpg";
const DJI_MAVIC2_ENTERPRISE_ADVANCED: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg";
const DJI_XT2: &str = "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_XT2.jpg";

/// ExifTool's comparison normalizes the `drone-dji` schema as XMP. The XT2
/// fixture's `RtkFlag` is an integer-valued property with no PrintConv, so
/// ExifTool 13.59 reports its text value unchanged.
#[test]
fn dji_xt2_xmp_rtk_flag_matches_exiftool() {
    if !Path::new(DJI_XT2).is_file() {
        eprintln!("skipping: corpus fixture not present at {DJI_XT2}");
        return;
    }

    let metadata = read_metadata(Path::new(DJI_XT2)).expect("DJI XT2 parses");
    assert_eq!(metadata.get_string("XMP:RtkFlag"), Some("0"));
}

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
    assert_eq!(metadata.get_string("DJI:FlightSpeed"), Some("9,0,0"));
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
