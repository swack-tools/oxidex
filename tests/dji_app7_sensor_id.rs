use oxidex::core::operations::read_metadata;
use oxidex::parsers::jpeg::app_segments::dji_dbg::parse_dji_dbg_app7;
use std::path::Path;

const DJI_M30T: &str = "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_M30T.jpg";

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

/// ExifTool 13.59's `DJI::Info` table maps the APP7 diagnostic key
/// `FlightSpeed(X,Y,Z)` to `FlightSpeed` without trying to interpret its
/// comma-separated payload.  The main-side mapping was absent after the
/// app-segment parser was forward-ported.
#[test]
fn dji_app7_flight_speed_record_is_preserved_verbatim() {
    let metadata = parse_dji_dbg_app7(b"DJI-DBG\0[FlightSpeed(X,Y,Z):9,0,0]");

    assert_eq!(metadata.get_string("APP7:FlightSpeed"), Some("9,0,0"));
}
