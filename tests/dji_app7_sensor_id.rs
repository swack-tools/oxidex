#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::read_metadata;

/// ExifTool 13.59 selects DJI::Info for APP7 `DJI-DBG\0` and exposes the
/// bracketed `sensor_id` record unchanged.
#[test]
fn dji_m30t_app7_sensor_id_matches_exiftool() {
    let Some(path) = fixtures::pinned_combined_fixture_path("DJI/DJI_M30T.jpg") else {
        eprintln!("skipping: combined corpus fixture DJI/DJI_M30T.jpg is absent");
        return;
    };

    let metadata = read_metadata(&path).expect("DJI M30T parses");

    assert_eq!(metadata.get_string("APP7:SensorID"), Some("4XAGJCP02AA007"));
}
