use crate::fixtures::pinned_fixture_path;
use oxidex::core::operations::read_metadata;

const DJI_ZH20N: &str = "DJI/DJI_ZH20N.jpg";
const DJI_MAVIC2_ENTERPRISE_ADVANCED: &str = "DJI/DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg";

/// ExifTool 13.59 selects DJI::ThermalParams2 for this APP4 payload and
/// renders its little-endian float at byte 32 with `sprintf("%.1f C", $val)`.
#[test]
fn dji_zh20n_app4_ambient_temperature_matches_exiftool() {
    let Some(path) = pinned_fixture_path(DJI_ZH20N) else {
        return;
    };

    let metadata = read_metadata(&path).expect("DJI ZH20N parses");

    let expected = match oxidex::exiftool_oracle::repo_pin() {
        "11.78" => None,
        "12.64" | "13.59" => Some("25.0 C"),
        pin => panic!("unreviewed ExifTool pin {pin}"),
    };
    assert_eq!(metadata.get_string("APP4:AmbientTemperature"), expected);
}
/// ExifTool 13.59 selects DJI::ThermalParams2 for this APP4 payload and
/// renders its little-endian float at byte 44 as a percentage.
#[test]
fn dji_zh20n_app4_relative_humidity_matches_exiftool() {
    let Some(path) = pinned_fixture_path(DJI_ZH20N) else {
        return;
    };

    let metadata = read_metadata(&path).expect("DJI ZH20N parses");

    let expected = match oxidex::exiftool_oracle::repo_pin() {
        "11.78" => None,
        "12.64" | "13.59" => Some("50 %"),
        pin => panic!("unreviewed ExifTool pin {pin}"),
    };
    assert_eq!(metadata.get_string("APP4:RelativeHumidity"), expected);
}
/// `DJI::ThermalParams2` is a table-backed APP4 record.  The Mavic 2 sample
/// has the optional 32-byte prefix, so the record begins after that prefix.
/// These values are the printed output of the pinned ExifTool 13.59 oracle.
#[test]
fn dji_mavic2_app4_thermal_params2_matches_exiftool() {
    let Some(path) = pinned_fixture_path(DJI_MAVIC2_ENTERPRISE_ADVANCED) else {
        return;
    };
    let metadata = read_metadata(&path).expect("DJI Mavic 2 Enterprise Advanced parses");

    let later = match oxidex::exiftool_oracle::repo_pin() {
        "11.78" => false,
        "12.64" | "13.59" => true,
        pin => panic!("unreviewed ExifTool pin {pin}"),
    };
    for (tag, value) in [
        ("APP4:ObjectDistance", "5.0 m"),
        ("APP4:Emissivity", "0.95"),
        ("APP4:RelativeHumidity", "50 %"),
        ("APP4:ReflectedTemperature", "25.0 C"),
        ("APP4:IDString", "Mini_640"),
    ] {
        assert_eq!(metadata.get_string(tag), later.then_some(value), "{tag}");
    }
}
