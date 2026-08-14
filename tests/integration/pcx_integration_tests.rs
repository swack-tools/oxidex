use oxidex::core::operations::read_metadata;
use std::path::Path;

/// ExifTool 13.59 reads the pinned `t/images/PCX.pcx` fixture's every tag
/// under the `File` group -- verified against the pinned oracle directly
/// (`exiftool-pinned.sh -a -G1 -s PCX.pcx`). `ImageWidth`/`ImageHeight`
/// exercise the `LeftMargin`/`TopMargin`-subtracting `ValueConv`; this fails
/// if PCX is not routed to `pcx.rs`'s `PCX::Main` layout or if that
/// conversion differs from ExifTool.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn pcx_fixture_matches_pinned_oracle() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/PCX.pcx",
    ))
    .expect("read pinned PCX fixture");

    assert_eq!(metadata.get_string("File:Manufacturer"), Some("ZSoft"));
    assert_eq!(
        metadata.get_string("File:Software"),
        Some("PC Paintbrush 3.0+")
    );
    assert_eq!(metadata.get_string("File:Encoding"), Some("RLE"));
    assert_eq!(metadata.get_integer("File:BitsPerPixel"), Some(8));
    assert_eq!(metadata.get_integer("File:LeftMargin"), Some(0));
    assert_eq!(metadata.get_integer("File:TopMargin"), Some(0));
    assert_eq!(metadata.get_integer("File:ImageWidth"), Some(8));
    assert_eq!(metadata.get_integer("File:ImageHeight"), Some(8));
    assert_eq!(metadata.get_integer("File:XResolution"), Some(72));
    assert_eq!(metadata.get_integer("File:YResolution"), Some(72));
    assert_eq!(metadata.get_integer("File:ColorPlanes"), Some(3));
    assert_eq!(metadata.get_integer("File:BytesPerLine"), Some(8));
    assert_eq!(metadata.get_string("File:ColorMode"), Some("Color Palette"));
    assert_eq!(metadata.get_string("File:FileType"), Some("PCX"));
}
