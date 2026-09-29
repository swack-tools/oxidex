use oxidex::core::operations::read_metadata;
#[path = "common/fixtures.rs"]
mod fixtures;

const DJI_FC330: &str = "DJI/DJI_FC330.jpg";

/// Pinned ExifTool 13.59 declares DJI::Main tags 0x0003..0x000b as floats
/// rendered with `%+.2f`.  FC330 exercises all nine Phantom-era fields.
#[test]
#[ignore = "requires pinned ExifTool 13.59 combined-samples"]
fn dji_fc330_makernote_float_fields_match_exiftool() {
    let path = fixtures::required_combined_fixture_path(DJI_FC330);

    let metadata = read_metadata(&path).expect("DJI FC330 parses");
    for (tag, expected) in [
        ("SpeedX", "+0.00"),
        ("SpeedY", "+0.00"),
        ("SpeedZ", "-0.30"),
        ("Pitch", "-4.10"),
        ("Yaw", "+32.00"),
        ("Roll", "-2.10"),
        ("CameraPitch", "-25.10"),
        ("CameraYaw", "+31.90"),
        ("CameraRoll", "+0.00"),
    ] {
        assert_eq!(metadata.get_string(&format!("DJI:{tag}")), Some(expected));
    }
}
