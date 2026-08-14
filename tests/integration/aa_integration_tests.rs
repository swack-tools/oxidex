use oxidex::core::operations::read_metadata;
use std::path::Path;

/// ExifTool 13.59 reads the pinned `t/images/Audible.aa` fixture's TOC and
/// metadata dictionary -- verified against the pinned oracle directly
/// (`exiftool-pinned.sh -a -G1 -s Audible.aa`). Covers both the four
/// pre-declared `Audible::Main` keys (`Author`, `Copyright`, `PublishDate`)
/// and the dynamic `MakeTagName` + snake_case-to-CamelCase path
/// (`TitleId`, `LongDescription`, and the digit-leading `Tag7eb298ac1328`),
/// plus HTML-entity/UTF-8 decoding on `Copyright`'s embedded `©`.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn aa_fixture_matches_pinned_oracle() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/Audible.aa",
    ))
    .expect("read pinned AA fixture");

    assert_eq!(
        metadata.get_string("Audible:ProductId"),
        Some("BK_ADBL_123456a_mp332")
    );
    assert_eq!(
        metadata.get_string("Audible:Author"),
        Some("Philip J Harvey")
    );
    assert_eq!(
        metadata.get_string("Audible:Copyright"),
        Some("\u{a9}2015, Philip J Harvey; (P)2015 ExifTool Publisher")
    );
    assert_eq!(
        metadata.get_string("Audible:PublishDate"),
        Some("08-APR-2015")
    );
    assert_eq!(
        metadata.get_string("Audible:PublishDateStart"),
        Some("08-APR-2015")
    );
    assert_eq!(
        metadata.get_string("Audible:TitleId"),
        Some("BK_ADBL_123456a")
    );
    assert_eq!(
        metadata.get_string("Audible:LongDescription"),
        Some("This is the long book description")
    );
    assert_eq!(
        metadata.get_string("Audible:IsAggregation"),
        Some("collection")
    );
    assert_eq!(
        metadata.get_string("Audible:Tag7eb298ac1328"),
        Some("64863450EA7B67906FE619AC697E60D13630E760")
    );
    assert!(
        matches!(metadata.get("Audible:CoverArt"), Some(oxidex::core::TagValue::Binary(bytes)) if bytes.len() == 18),
        "expected 18-byte CoverArt, got {:?}",
        metadata.get("Audible:CoverArt")
    );
    assert_eq!(metadata.get_string("File:FileType"), Some("AA"));
}
