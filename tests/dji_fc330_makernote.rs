use oxidex::core::operations::read_metadata;
use std::path::Path;

const DJI_FC330: &str = "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_FC330.jpg";

/// Pinned ExifTool 13.59 declares DJI::Main tags 0x0003..0x000b as floats
/// rendered with `%+.2f`.  FC330 exercises all nine Phantom-era fields.
#[test]
fn dji_fc330_makernote_float_fields_match_exiftool() {
    if !Path::new(DJI_FC330).is_file() {
        eprintln!("skipping: corpus fixture not present at {DJI_FC330}");
        return;
    }

    let metadata = read_metadata(Path::new(DJI_FC330)).expect("DJI FC330 parses");
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
