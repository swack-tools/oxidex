use oxidex::core::operations::read_metadata;
use std::path::Path;

/// ExifTool 13.59 reads the pinned `t/images/MRC.mrc` fixture's 1024-byte
/// `MRC::Main` header, then its `FEI1` extended header's `MRC::FEI12` table
/// -- verified against the pinned oracle directly (`exiftool-pinned.sh -G1
/// -s MRC.mrc`, `uv run scripts/compare_file.py .../MRC.mrc`: MISSING 0,
/// WRONG 0). This pins both: `MRC::Main`'s own tags, and a representative
/// sample of `FEI12`'s bitmask-gated section-0 fields (see `mrc.rs`'s
/// module doc comment for how the gating works and why
/// `AcquisitionTimeStamp`/`CFEGFlashTimeStamp` are the two fields still
/// deliberately omitted).
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
    assert_eq!(metadata.get_string("File:StartPoint"), Some("0 0 0"));
    assert_eq!(metadata.get_string("File:GridSize"), Some("4096 4096 2"));
    assert_eq!(metadata.get_integer("File:SpaceGroupNumber"), Some(0));
    assert_eq!(metadata.get_integer("File:ExtendedHeaderSize"), Some(1536));
    assert_eq!(metadata.get_string("File:ExtendedHeaderType"), Some("FEI1"));
    assert_eq!(metadata.get_integer("File:MRCVersion"), Some(20140));
    assert_eq!(metadata.get_string("File:Origin"), Some("0 0 0"));
    assert_eq!(
        metadata.get_string("File:MachineStamp"),
        Some("0x44 0x44 0x00 0x00")
    );
    assert_eq!(metadata.get_integer("File:NumberOfLabels"), Some(0));
    assert_eq!(metadata.get_string("File:ImageWidthAxis"), Some("X"));
    assert_eq!(metadata.get_string("File:ImageHeightAxis"), Some("Y"));
    assert_eq!(metadata.get_string("File:ImageDepthAxis"), Some("Z"));

    // `MRC::FEI12`'s section-0 extended header, now decoded (bitmask-gated
    // fields, MRC.pm:83-172; see `mrc.rs`'s module doc comment).
    assert_eq!(metadata.get_integer("File:MetadataSize"), Some(768));
    assert_eq!(metadata.get_integer("File:MetadataVersion"), Some(0));
    assert_eq!(metadata.get_string("File:Bitmask1"), Some("0xffffffff"));
    assert_eq!(metadata.get_string("File:Bitmask2"), Some("0x0cfff01f"));
    assert_eq!(
        metadata.get_string("File:TimeStamp"),
        Some("2020:10:21 13:54:27")
    );
    assert_eq!(
        metadata.get_string("File:MicroscopeType"),
        Some("TALOS-D5197")
    );
    assert_eq!(metadata.get_string("File:Application"), Some("Tomography"));
    assert_eq!(metadata.get_float("File:HighTension"), Some(200000.0));
    assert_eq!(metadata.get_string("File:InstrumentMode"), Some("TEM"));
    assert_eq!(metadata.get_string("File:ProjectionMode"), Some("Imaging"));
    assert_eq!(metadata.get_string("File:EFTEMOn"), Some("No"));
    assert_eq!(metadata.get_float("File:Magnification"), Some(28000.0));
    assert_eq!(metadata.get_string("File:PhasePlate"), Some("Yes"));
    assert_eq!(metadata.get_integer("File:ReadoutAreaRight"), Some(4096));

    // `ImageDepth` (2) exceeds the one section this parser decodes, so
    // ExifTool's own "read the rest with -ee" warning applies (MRC.pm:170-176).
    assert_eq!(
        metadata.get_string("ExifTool:Warning"),
        Some("[minor] Use the ExtractEmbedded option to read metadata for all frames")
    );

    // `AcquisitionTimeStamp`/`CFEGFlashTimeStamp`: deliberately not decoded
    // (local-timezone `ValueConv`, unverifiable -- see `mrc.rs`'s module doc
    // comment). Also out of bounds for this fixture's 768-byte section 0
    // either way (offsets 796/860).
    assert!(metadata.get("File:AcquisitionTimeStamp").is_none());
    assert!(metadata.get("File:CFEGFlashTimeStamp").is_none());

    assert_eq!(
        metadata.get_string("Composite:ImageSize"),
        Some("4096x4096")
    );
    assert_eq!(metadata.get_string("File:FileType"), Some("MRC"));
}
