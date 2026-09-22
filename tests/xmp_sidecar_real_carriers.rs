//! Real-carrier regressions for newly dispatched formats.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::core::operations::read_metadata;

#[test]
fn tnef_correlation_keys_are_read_from_the_real_carrier() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("TNEF.tnef") else {
        eprintln!("skipping: pinned fixture TNEF.tnef is absent");
        return;
    };

    let metadata = read_metadata(&path).expect("TNEF parses");
    assert_eq!(
        metadata.get_string("File:CorrelationKey"),
        Some("<2896107D7E52DF4DB5D10536DBFEFAD07E37@user.example.com>")
    );
}

#[test]
fn jpeg2000_codestream_comments_are_read_from_the_real_carrier() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("Jpeg2000.j2c") else {
        eprintln!("skipping: pinned fixture Jpeg2000.j2c is absent");
        return;
    };

    let metadata = read_metadata(&path).expect("J2C parses");
    let comment = metadata
        .get_string("File:Comment")
        .expect("second real J2C COM marker retained");
    assert!(comment.starts_with("Kdu-Layer-Info: log_2{Delta-D(MSE)/"));
    assert!(comment.ends_with("-256.0,  6.2e+02\n"));
}
