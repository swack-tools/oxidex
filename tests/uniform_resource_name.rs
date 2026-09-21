#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::read_metadata;

/// ExifTool 13.59 exposes an APP2 payload beginning with `urn:` unchanged.
#[test]
fn apple_iphone_16_pro_app2_uniform_resource_name_matches_exiftool() {
    let Some(path) = fixtures::pinned_combined_fixture_path("Apple/Apple_iPhone16Pro.jpg") else {
        eprintln!("skipping: combined corpus fixture Apple_iPhone16Pro.jpg is absent");
        return;
    };

    let metadata = read_metadata(&path).expect("Apple JPEG parses");

    assert_eq!(
        metadata.get_string("JPEG:UniformResourceName"),
        Some("urn:iso:std:iso:ts:21496:-1")
    );
}
