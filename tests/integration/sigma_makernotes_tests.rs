//! Integration tests for the Sigma MakerNote tags a real SIGMA JPEG reports.
//!
//! The lens-name tests that used to live here drove `SIGMA_LENSES`, a 61-entry
//! hand-written table keyed 1..157. `%sigmaLensTypes` is keyed by hexadecimal
//! id (0x10, 0x103, 0x145, ...) and spells its names ExifTool's way ("Sigma
//! 50mm F2.8 EX DG MACRO"), so not one of those 61 ids or spellings came from
//! it. Nothing in `src/` ever called the table, and the real transcription --
//! 203 entries from `Sigma.pm` -- has been live at
//! `parsers::raw::sigma_lens_types` all along, so the table and these tests
//! were removed together.
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
