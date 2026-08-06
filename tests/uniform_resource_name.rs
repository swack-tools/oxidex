use oxidex::core::operations::read_metadata;
use std::path::Path;

const APPLE_IPHONE_16_PRO: &str =
    "/tmp/oxidex-exiftool-cache/combined-samples/Apple/Apple_iPhone16Pro.jpg";

/// ExifTool 13.59 exposes an APP2 payload beginning with `urn:` unchanged.
#[test]
fn apple_iphone_16_pro_app2_uniform_resource_name_matches_exiftool() {
    let metadata = read_metadata(Path::new(APPLE_IPHONE_16_PRO)).expect("Apple JPEG parses");

    assert_eq!(
        metadata.get_string("JPEG:UniformResourceName"),
        Some("urn:iso:std:iso:ts:21496:-1")
    );
}
