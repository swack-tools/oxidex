//! Real-carrier regressions for newly dispatched formats.

use oxidex::core::operations::read_metadata;
use std::path::Path;

const IMAGE_DIR: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images";

#[test]
fn tnef_correlation_keys_are_read_from_the_real_carrier() {
    let path = Path::new(IMAGE_DIR).join("TNEF.tnef");
    if !path.is_file() {
        eprintln!("skipping: pinned fixture not present at {}", path.display());
        return;
    }

    let metadata = read_metadata(&path).expect("TNEF parses");
    assert_eq!(
        metadata.get_string("File:CorrelationKey"),
        Some("<2896107D7E52DF4DB5D10536DBFEFAD07E37@user.example.com>")
    );
}

#[test]
fn jpeg2000_codestream_comments_are_read_from_the_real_carrier() {
    let path = Path::new(IMAGE_DIR).join("Jpeg2000.j2c");
    if !path.is_file() {
        eprintln!("skipping: pinned fixture not present at {}", path.display());
        return;
    }

    let metadata = read_metadata(&path).expect("J2C parses");
    let comment = metadata
        .get_string("File:Comment")
        .expect("second real J2C COM marker retained");
    assert!(comment.starts_with("Kdu-Layer-Info: log_2{Delta-D(MSE)/"));
    assert!(comment.ends_with("-256.0,  6.2e+02\n"));
}
