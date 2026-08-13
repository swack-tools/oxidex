//! Regression coverage for Casio's legacy APP1 QVCI segment.

const CASIO_QVCI: &str = "/tmp/oxidex-exiftool-cache/combined-samples/CasioQVCI.jpg";
const CASIO_TYPE2: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Casio2.jpg";

/// ExifTool 13.59's `%Image::ExifTool::Casio::QVCI` maps byte 0x2c value 1
/// through `CasioQuality`'s `PrintConv` to `Economy`.
#[test]
fn casio_qvci_reports_economy_quality() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(CASIO_QVCI).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(CASIO_QVCI)).expect("Casio QVCI parses");
    assert_eq!(metadata.get_string("Casio:CasioQuality"), Some("Economy"));
}

/// `Casio.pm` Type2 tag 0x301b applies its ArtMode PrintConv to the inline
/// `int16u` value.  The pinned Casio2 fixture stores zero, which is Normal.
#[test]
fn casio_type2_reports_art_mode() {
    use oxidex::core::operations::read_metadata;
    use std::path::Path;

    if !Path::new(CASIO_TYPE2).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(CASIO_TYPE2)).expect("Casio Type2 parses");
    assert_eq!(metadata.get_string("Casio:ArtMode"), Some("Normal"));
}
