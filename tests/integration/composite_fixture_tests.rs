//! Regression coverage for Composite tags evaluated from real pinned fixtures.
//!
//! The files are from the same comparison corpus used for the compatibility
//! report.  The guards intentionally keep a source checkout usable without
//! that optional cache while pinning exact ExifTool 13.59 output in CI.

use oxidex::core::operations::read_metadata;
use std::path::Path;

const KODAK: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Kodak.jpg";
const FLIR: &str = "/tmp/oxidex-exiftool-cache/combined-samples/FLIR.jpg";

/// Kodak.pm's DateCreated Composite joins YearCreated and MonthDayCreated.
/// ExifTool 13.59 reports `2002:05:01` for this corpus image.
#[test]
fn kodak_fixture_reports_composite_date_created() {
    if !Path::new(KODAK).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(KODAK)).expect("Kodak fixture parses");
    assert_eq!(
        metadata.get_string("Composite:DateCreated"),
        Some("2002:05:01")
    );
}

/// FLIR.pm derives this from PlanckB as `14387.6515 / PlanckB` and formats it
/// to one decimal micrometre. ExifTool 13.59 reports `10.5 um` here.
#[test]
fn flir_fixture_reports_composite_peak_spectral_sensitivity() {
    if !Path::new(FLIR).is_file() {
        return;
    }

    let metadata = read_metadata(Path::new(FLIR)).expect("FLIR fixture parses");
    assert_eq!(
        metadata.get_string("Composite:PeakSpectralSensitivity"),
        Some("10.5 um")
    );
}
