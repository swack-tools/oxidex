use oxidex::core::operations::read_metadata;
use std::path::Path;

/// ExifTool 13.59 reads the pinned `t/images/PGF.pgf` fixture's 24-byte
/// header plus its trailing embedded-PNG metadata blob -- verified against
/// the pinned oracle directly (`exiftool-pinned.sh -a -G1 -s PGF.pgf`). The
/// embedded PNG is a deliberately different (1x1) image from the 8x8 PGF
/// image it describes, so `Composite:ImageSize` resolving to `8x8` (the PGF
/// header's own `ImageWidth`/`ImageHeight`, not the embedded PNG's) is the
/// specific regression this test guards: without `PGF::Main`'s `PRIORITY =>
/// 2` carried through via `insert_occurrence`, the composite engine's
/// bare-name resolution ties on order and would pick the embedded PNG's `1`
/// instead.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn pgf_fixture_matches_pinned_oracle() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/PGF.pgf",
    ))
    .expect("read pinned PGF fixture");

    assert_eq!(metadata.get_string("File:PGFVersion"), Some("0x36"));
    assert_eq!(metadata.get_integer("File:ImageWidth"), Some(8));
    assert_eq!(metadata.get_integer("File:ImageHeight"), Some(8));
    assert_eq!(metadata.get_integer("File:BitsPerPixel"), Some(24));
    assert_eq!(metadata.get_integer("File:ColorComponents"), Some(3));
    assert_eq!(metadata.get_string("File:ColorMode"), Some("RGB"));

    // The embedded PNG metadata blob re-enters this crate's own PNG parser.
    assert_eq!(metadata.get_integer("PNG:ImageWidth"), Some(1));
    assert_eq!(metadata.get_integer("PNG:ImageHeight"), Some(1));

    // PGF::Main's PRIORITY => 2 must win Composite:ImageSize's bare-name
    // `ImageWidth`/`ImageHeight` resolution over the embedded PNG's.
    assert_eq!(metadata.get_string("Composite:ImageSize"), Some("8x8"));
    assert_eq!(metadata.get_string("File:FileType"), Some("PGF"));
}
