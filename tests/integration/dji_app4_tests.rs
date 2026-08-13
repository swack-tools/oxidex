use oxidex::core::{TagValue, operations::read_metadata};
use std::path::Path;

const DJI_ZH20N: &str = "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_ZH20N.jpg";
const DJI_MAVIC2_ENTERPRISE_ADVANCED: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/DJI/DJI_MAVIC2-ENTERPRISE-ADVANCED.jpg";

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
/// `DJI::ThermalParams2` is a table-backed APP4 record.  The Mavic 2 sample
/// has the optional 32-byte prefix, so the record begins after that prefix.
/// These values are the printed output of the pinned ExifTool 13.59 oracle.
#[test]
fn dji_mavic2_app4_thermal_params2_matches_exiftool() {
    if !std::path::Path::new(DJI_MAVIC2_ENTERPRISE_ADVANCED).is_file() {
        eprintln!(
            "skipping: corpus fixture not present at {}",
            DJI_MAVIC2_ENTERPRISE_ADVANCED
        );
        return;
    }
    let metadata = read_metadata(Path::new(DJI_MAVIC2_ENTERPRISE_ADVANCED))
        .expect("DJI Mavic 2 Enterprise Advanced parses");

    assert_eq!(metadata.get_string("APP4:ObjectDistance"), Some("5.0 m"));
    assert_eq!(metadata.get_string("APP4:Emissivity"), Some("0.95"));
    assert_eq!(metadata.get_string("APP4:RelativeHumidity"), Some("50 %"));
    assert_eq!(
        metadata.get_string("APP4:ReflectedTemperature"),
        Some("25.0 C")
    );
    assert_eq!(metadata.get_string("APP4:IDString"), Some("Mini_640"));
}

/// ExifTool 13.59 treats this MakerNote payload as a sequence of bracketed
/// DJI::Info fields. These eleven opaque values must retain their exact byte
/// counts; their contents are vendor diagnostic blobs, not text metadata.
#[test]
fn dji_mavic2_info_binary_tags_match_exiftool() {
    if !Path::new(DJI_MAVIC2_ENTERPRISE_ADVANCED).is_file() {
        eprintln!("skipping: corpus fixture not present at {DJI_MAVIC2_ENTERPRISE_ADVANCED}");
        return;
    }

    let metadata = read_metadata(Path::new(DJI_MAVIC2_ENTERPRISE_ADVANCED))
        .expect("DJI Mavic 2 Enterprise Advanced parses");

    for (tag, bytes) in [
        ("AEDebugInfo", 256),
        ("AEHistogramInfo", 4096),
        ("AELocalHistogram", 2048),
        ("AELiveViewHistogramInfo", 4096),
        ("AELiveViewLocalHistogram", 2048),
        ("AWBDebugInfo", 4096),
        ("AFDebugInfo", 256),
        ("Histogram", 1024),
        ("Xidiri", 512),
        ("ADJDebugInfo", 1024),
        ("HyperlapsDebugInfo", 8),
    ] {
        assert!(
            matches!(
                metadata.get(&format!("DJI:{tag}")),
                Some(TagValue::Binary(value)) if value.len() == bytes
            ),
            "{tag} should be a {bytes}-byte binary value"
        );
    }
}
