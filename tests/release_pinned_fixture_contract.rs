//! Required-mode parsing contract for the canonical pinned source fixtures.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::{TagValue, operations::read_metadata};

#[test]
#[ignore = "runs in the configured pinned-fixture qualification stage"]
fn required_mode_executes_real_ra_and_swf_parsing_assertions() {
    let real = fixtures::required_t_images_fixture_path("Real.ra");
    let real_metadata = read_metadata(&real).expect("Real.ra must parse after resolution");
    assert_eq!(
        real_metadata.get_string("Real-RA4:Title"),
        Some("The Sewing Girls")
    );
    assert_eq!(
        real_metadata.get("Real-RA4:SampleRate"),
        Some(&TagValue::Integer(22_050))
    );
    eprintln!("release-fixture-contract: Real.ra assertions executed");

    let flash = fixtures::required_t_images_fixture_path("Flash.swf");
    let flash_metadata = read_metadata(&flash).expect("Flash.swf must parse after resolution");
    assert_eq!(
        flash_metadata.get("Flash:FlashVersion"),
        Some(&TagValue::Integer(6))
    );
    assert_eq!(flash_metadata.get_string("Flash:Duration"), Some("0.08 s"));
    eprintln!("release-fixture-contract: Flash.swf assertions executed");
}
