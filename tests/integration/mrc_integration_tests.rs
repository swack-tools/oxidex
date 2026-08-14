use oxidex::core::operations::read_metadata;
use std::path::Path;

/// ExifTool 13.59 reads the pinned `t/images/MRC.mrc` fixture's 1024-byte
/// `MRC::Main` header -- verified against the pinned oracle directly
/// (`exiftool-pinned.sh -a -G1 -s MRC.mrc`). The fixture's
/// `ExtendedHeaderType` is `FEI1` and its `FEI12` extended header genuinely
/// carries ~60 more tags in real ExifTool output; this parser deliberately
/// does not read that table (see `mrc.rs`'s module doc comment) so this test
/// only pins the `MRC::Main` tags it does emit, and confirms the extended
/// header's own tags are absent rather than approximated.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn mrc_fixture_matches_pinned_oracle_main_table() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/MRC.mrc",
    ))
    .expect("read pinned MRC fixture");

    assert_eq!(metadata.get_integer("File:ImageWidth"), Some(4096));
    assert_eq!(metadata.get_integer("File:ImageHeight"), Some(4096));
    assert_eq!(metadata.get_integer("File:ImageDepth"), Some(2));
    assert_eq!(
        metadata.get_string("File:ImageMode"),
        Some("16-bit unsigned integer")
    );
    assert_eq!(metadata.get_integer("File:SpaceGroupNumber"), Some(0));
    assert_eq!(metadata.get_integer("File:ExtendedHeaderSize"), Some(1536));
    assert_eq!(metadata.get_string("File:ExtendedHeaderType"), Some("FEI1"));
    assert_eq!(metadata.get_integer("File:MRCVersion"), Some(20140));
    assert_eq!(
        metadata.get_string("File:MachineStamp"),
        Some("0x44 0x44 0x00 0x00")
    );
    assert_eq!(metadata.get_integer("File:NumberOfLabels"), Some(0));
    assert_eq!(metadata.get_string("File:ImageWidthAxis"), Some("X"));
    assert_eq!(metadata.get_string("File:ImageHeightAxis"), Some("Y"));
    assert_eq!(metadata.get_string("File:ImageDepthAxis"), Some("Z"));

    // The FEI12 extended header is not modeled: honest omission, not a
    // guessed value.
    assert!(metadata.get("File:MetadataSize").is_none());
    assert!(metadata.get("File:MicroscopeType").is_none());
    assert!(metadata.get("File:Magnification").is_none());

    assert_eq!(
        metadata.get_string("Composite:ImageSize"),
        Some("4096x4096")
    );
    assert_eq!(metadata.get_string("File:FileType"), Some("MRC"));
}
