use oxidex::core::operations::read_metadata;
use std::path::Path;

const DJI_M30T: &str = "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_M30T.jpg";

/// ExifTool 13.59 selects DJI::Info for APP7 `DJI-DBG\0` and exposes the
/// bracketed `sensor_id` record unchanged.
#[test]
fn dji_m30t_app7_sensor_id_matches_exiftool() {
    let metadata = read_metadata(Path::new(DJI_M30T)).expect("DJI M30T parses");

    assert_eq!(metadata.get_string("APP7:SensorID"), Some("4XAGJCP02AA007"));
}
