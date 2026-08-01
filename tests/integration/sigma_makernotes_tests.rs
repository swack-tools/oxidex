//! Integration tests for the Sigma lens-name database and the Sigma MakerNote
//! tags a real SIGMA JPEG reports.
//!
//! The MakerNote tests that used to live here drove a registry-backed
//! `SigmaMakerNoteParser` and asserted the names it invented -- `LensID` at
//! 0x001b, `ColorMode` at 0x001e, numeric PrintConvs for tags `Sigma.pm`
//! stores as strings. None of that agreed with `Sigma.pm`, so both the parser
//! and these tests were removed. `parsers::tiff::makernotes::sigma` is now the
//! single Sigma MakerNote table; it is transcribed from `Sigma.pm` and carries
//! its own unit tests, and both callers -- Sigma JPEGs and the JpgFromRaw
//! preview inside an X3F -- are covered by the ExifTool comparison harness.
//!
//! What the harness cannot do is fail the build. The registry it replaced was
//! live for as long as it was because nothing in `cargo test` ever read a
//! Sigma file: the tests here asserted synthetic bytes against the same wrong
//! table, so they agreed with it. The tests at the bottom of this file close
//! that hole -- they read a committed SD10 JPEG and compare every tag against
//! ExifTool's own output, in both directions, so a name or value that drifts
//! from `Sigma.pm` fails CI rather than waiting for someone to run the harness.

#[test]
fn test_sigma_lens_database_art_primes() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series prime lenses
    assert_eq!(
        lookup_lens_name(1),
        Some("Sigma 14mm f/1.8 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(3),
        Some("Sigma 24mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(6),
        Some("Sigma 35mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(10),
        Some("Sigma 50mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(13),
        Some("Sigma 85mm f/1.4 DG HSM Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_art_telephoto() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series telephoto primes
    assert_eq!(
        lookup_lens_name(15),
        Some("Sigma 105mm f/1.4 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(16),
        Some("Sigma 135mm f/1.8 DG HSM Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_art_macro() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series macro lenses
    assert_eq!(
        lookup_lens_name(20),
        Some("Sigma 70mm f/2.8 DG Macro Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(21),
        Some("Sigma 105mm f/2.8 DG DN Macro Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_art_zooms() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Art series zoom lenses
    assert_eq!(
        lookup_lens_name(30),
        Some("Sigma 14-24mm f/2.8 DG HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(31),
        Some("Sigma 18-35mm f/1.8 DC HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(33),
        Some("Sigma 24-70mm f/2.8 DG OS HSM Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(35),
        Some("Sigma 50-100mm f/1.8 DC HSM Art".to_string())
    );
}

#[test]
fn test_sigma_lens_database_contemporary_primes() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Contemporary series primes
    assert_eq!(
        lookup_lens_name(50),
        Some("Sigma 16mm f/1.4 DC DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(51),
        Some("Sigma 23mm f/1.4 DC DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(52),
        Some("Sigma 30mm f/1.4 DC DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(53),
        Some("Sigma 56mm f/1.4 DC DN Contemporary".to_string())
    );
}

#[test]
fn test_sigma_lens_database_contemporary_zooms() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Contemporary series zoom lenses
    assert_eq!(
        lookup_lens_name(54),
        Some("Sigma 17-70mm f/2.8-4.0 DC Macro OS HSM Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(57),
        Some("Sigma 100-400mm f/5.0-6.3 DG OS HSM Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(58),
        Some("Sigma 150-600mm f/5.0-6.3 DG OS HSM Contemporary".to_string())
    );
}

#[test]
fn test_sigma_lens_database_sports_series() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma Sports series lenses
    assert_eq!(
        lookup_lens_name(70),
        Some("Sigma 120-300mm f/2.8 DG OS HSM Sports".to_string())
    );

    assert_eq!(
        lookup_lens_name(71),
        Some("Sigma 150-600mm f/5.0-6.3 DG OS HSM Sports".to_string())
    );

    assert_eq!(
        lookup_lens_name(72),
        Some("Sigma 500mm f/4.0 DG OS HSM Sports".to_string())
    );
}

#[test]
fn test_sigma_lens_database_legacy_sa_mount_zooms() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test legacy SA-mount zoom lenses
    assert_eq!(
        lookup_lens_name(100),
        Some("Sigma 8-16mm f/4.5-5.6 DC HSM".to_string())
    );

    assert_eq!(
        lookup_lens_name(102),
        Some("Sigma 17-50mm f/2.8 EX DC OS HSM".to_string())
    );

    assert_eq!(
        lookup_lens_name(107),
        Some("Sigma 50-500mm f/4.5-6.3 APO DG OS HSM".to_string())
    );
}

#[test]
fn test_sigma_lens_database_legacy_sa_mount_primes() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test legacy SA-mount prime lenses
    assert_eq!(
        lookup_lens_name(120),
        Some("Sigma 8mm f/3.5 EX DG Circular Fisheye".to_string())
    );

    assert_eq!(
        lookup_lens_name(123),
        Some("Sigma 30mm f/1.4 EX DC HSM".to_string())
    );

    assert_eq!(
        lookup_lens_name(125),
        Some("Sigma 180mm f/2.8 EX DG OS HSM APO Macro".to_string())
    );
}

#[test]
fn test_sigma_lens_database_dg_dn_mirrorless() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test Sigma DG DN mirrorless lenses
    assert_eq!(
        lookup_lens_name(150),
        Some("Sigma 14-24mm f/2.8 DG DN Art".to_string())
    );

    assert_eq!(
        lookup_lens_name(151),
        Some("Sigma 20mm f/2.0 DG DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(154),
        Some("Sigma 35mm f/2.0 DG DN Contemporary".to_string())
    );

    assert_eq!(
        lookup_lens_name(157),
        Some("Sigma 90mm f/2.8 DG DN Contemporary".to_string())
    );
}

#[test]
fn test_sigma_lens_database_not_found() {
    use oxidex::parsers::tiff::makernotes::sigma_lens_database::lookup_lens_name;

    // Test that unknown lens IDs return None
    assert_eq!(lookup_lens_name(9999), None);
    assert_eq!(lookup_lens_name(0), None);
    assert_eq!(lookup_lens_name(500), None);
}

/// The MakerNote tags a real SIGMA JPEG must report, taken verbatim from
/// `exiftool -a -G0 -s -json -MakerNotes:all tests/fixtures/jpeg/makernotes/sigma_sd10.jpg`
/// (ExifTool 13.55). ExifTool reads this MakerNote through `MakerNotes.pm`'s
/// `MakerNoteSigma` entry into `Image::ExifTool::Sigma::Main`, so these 23
/// names and values are the whole of what the table yields here.
///
/// Keyed under family-0 `MakerNotes`, which is what this path emits. ExifTool's
/// family-1 name for the same tags is `Sigma`.
const SIGMA_SD10_ORACLE: &[(&str, &str)] = &[
    ("MakerNotes:SerialNumber", "02000019"),
    ("MakerNotes:DriveMode", "SINGLE"),
    ("MakerNotes:ResolutionMode", "HI"),
    ("MakerNotes:AFMode", "AF-S"),
    ("MakerNotes:FocusSetting", "AF"),
    ("MakerNotes:WhiteBalance", "Sunlight"),
    ("MakerNotes:ExposureMode", "Program AE"),
    ("MakerNotes:MeteringMode", "Multi-segment"),
    ("MakerNotes:LensFocalRange", "24 to 70"),
    ("MakerNotes:ColorSpace", "sRGB"),
    ("MakerNotes:ExposureCompensation", "+0.8"),
    ("MakerNotes:Contrast", "+0.0"),
    ("MakerNotes:Shadow", "+0.0"),
    ("MakerNotes:Highlight", "+0.0"),
    ("MakerNotes:Saturation", "+0.4"),
    ("MakerNotes:Sharpness", "+1.0"),
    ("MakerNotes:X3FillLight", "+0.0"),
    ("MakerNotes:ColorAdjustment", "0"),
    ("MakerNotes:AdjustmentMode", "X3F Setting Mode"),
    ("MakerNotes:Quality", "12"),
    ("MakerNotes:Firmware", "2.0.4.1642 Release"),
    ("MakerNotes:Software", "SIGMA PhotoPro 2.0.0.1586"),
    ("MakerNotes:AutoBracket", " "),
];

fn sigma_sd10_tags() -> Vec<(String, String)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/jpeg/makernotes/sigma_sd10.jpg");
    let metadata = oxidex::core::operations::read_metadata(&path).unwrap();
    let mut tags: Vec<(String, String)> = metadata
        .iter()
        .filter(|(key, _)| key.starts_with("MakerNotes:"))
        .map(|(key, value)| {
            (
                key.clone(),
                value.as_string().map(str::to_string).unwrap_or_default(),
            )
        })
        .collect();
    tags.sort();
    tags
}

#[test]
fn sigma_jpeg_reports_the_values_exiftool_reports() {
    let tags = sigma_sd10_tags();

    for (key, expected) in SIGMA_SD10_ORACLE {
        let found = tags.iter().find(|(k, _)| k == key);
        assert_eq!(
            found.map(|(_, v)| v.as_str()),
            Some(*expected),
            "{key} disagrees with ExifTool"
        );
    }
}

/// The key set has to match in both directions. A missing tag is a gap, but an
/// extra one is a fabrication: it reports Sigma data the camera never wrote.
/// The registry this replaced had `LensRange` at 0x000a,
/// `LensType`/`LensID`/`LensModel` over the preview-image ids 0x001a-0x001c,
/// and `X3FillLight` at 0x0020 -- an id `Sigma.pm` does not name at all.
#[test]
fn sigma_jpeg_reports_exactly_the_tags_exiftool_reports() {
    let tags = sigma_sd10_tags();

    let expected: std::collections::BTreeSet<&str> =
        SIGMA_SD10_ORACLE.iter().map(|(key, _)| *key).collect();
    let actual: std::collections::BTreeSet<&str> =
        tags.iter().map(|(key, _)| key.as_str()).collect();

    let fabricated: Vec<&&str> = actual.difference(&expected).collect();
    let missing: Vec<&&str> = expected.difference(&actual).collect();
    assert!(
        fabricated.is_empty() && missing.is_empty(),
        "Sigma tag set differs from ExifTool's -- fabricated: {fabricated:?}, missing: {missing:?}"
    );
}
