use oxidex::core::operations::read_metadata;
use std::path::Path;

/// ExifTool 13.59 reads the pinned `t/images/ITC.itc` fixture's `itch` and
/// `item` blocks -- verified against the pinned oracle directly
/// (`exiftool-pinned.sh -a -G1 -s ITC.itc`). This fails if ITC is not routed
/// to `itc.rs`'s block walk, if the variable-length item-header skip drifts,
/// or if any of the hand-implemented conversions (hex IDs, `DataType`,
/// `DataLocation`, `ImageType`) differ from ExifTool.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn itc_fixture_matches_pinned_oracle() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/ITC.itc",
    ))
    .expect("read pinned ITC fixture");

    assert_eq!(metadata.get_string("ITC:DataType"), Some("Artwork"));
    assert_eq!(
        metadata.get_string("ITC:LibraryID"),
        Some("914A6DE01A279611")
    );
    assert_eq!(metadata.get_string("ITC:TrackID"), Some("195770115E4BA2B6"));
    assert_eq!(
        metadata.get_string("ITC:DataLocation"),
        Some("Local Music File")
    );
    assert_eq!(metadata.get_string("ITC:ImageType"), Some("PNG"));
    assert_eq!(metadata.get_integer("ITC:ImageWidth"), Some(8));
    assert_eq!(metadata.get_integer("ITC:ImageHeight"), Some(8));
    assert!(
        matches!(metadata.get("ITC:ImageData"), Some(oxidex::core::TagValue::Binary(bytes)) if bytes.len() == 180),
        "expected 180-byte ImageData, got {:?}",
        metadata.get("ITC:ImageData")
    );
    assert_eq!(metadata.get_string("File:FileType"), Some("ITC"));
}
