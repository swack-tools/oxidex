use oxidex::core::operations::read_metadata;
use std::path::Path;

const DJI_ZH20N: &str = "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_ZH20N.jpg";

/// ExifTool 13.59 selects DJI::ThermalParams2 for this APP4 payload and
/// renders its little-endian float at byte 32 with `sprintf("%.1f C", $val)`.
#[test]
fn dji_zh20n_app4_ambient_temperature_matches_exiftool() {
    if !Path::new(DJI_ZH20N).is_file() {
        eprintln!("skipping: corpus fixture not present at {DJI_ZH20N}");
        return;
    }

    let metadata = read_metadata(Path::new(DJI_ZH20N)).expect("DJI ZH20N parses");

    assert_eq!(
        metadata.get_string("APP4:AmbientTemperature"),
        Some("25.0 C")
    );
}

/// ExifTool 13.59 selects DJI::ThermalParams2 for this APP4 payload and
/// renders its little-endian float at byte 44 as a percentage.
#[test]
fn dji_zh20n_app4_relative_humidity_matches_exiftool() {
    if !Path::new(DJI_ZH20N).is_file() {
        eprintln!("skipping: corpus fixture not present at {DJI_ZH20N}");
        return;
    }

    let metadata = read_metadata(Path::new(DJI_ZH20N)).expect("DJI ZH20N parses");

    assert_eq!(metadata.get_string("APP4:RelativeHumidity"), Some("50 %"));
}
