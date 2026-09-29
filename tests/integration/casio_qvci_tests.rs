//! Regression coverage for Casio's legacy APP1 QVCI segment.

use crate::fixtures;
use oxidex::core::operations::read_metadata;

/// ExifTool 13.59's `%Image::ExifTool::Casio::QVCI` maps byte 0x2c value 1
/// through `CasioQuality`'s `PrintConv` to `Economy`.
#[test]
#[ignore = "requires pinned combined-samples/CasioQVCI.jpg"]
fn casio_qvci_reports_economy_quality() {
    let path = fixtures::required_combined_fixture_path("CasioQVCI.jpg");
    let metadata = read_metadata(&path).expect("Casio QVCI parses");
    assert_eq!(metadata.get_string("Casio:CasioQuality"), Some("Economy"));
}

/// `Casio.pm` Type2 tag 0x301b applies its ArtMode PrintConv to the inline
/// `int16u` value. The pinned Casio2 fixture stores zero, which is Normal.
#[test]
#[ignore = "requires pinned combined-samples/Casio2.jpg"]
fn casio_type2_reports_art_mode() {
    let path = fixtures::required_combined_fixture_path("Casio2.jpg");
    let metadata = read_metadata(&path).expect("Casio Type2 parses");
    assert_eq!(metadata.get_string("Casio:ArtMode"), Some("Normal"));
}
