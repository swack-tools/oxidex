//! Pinned ExifTool 13.59 `XMP::Composite::LensID` source fixture.

use oxidex::core::operations::read_metadata;

#[test]
fn canon_numeric_xmp_aux_lens_id_uses_source_backed_composite() {
    let file = tempfile::Builder::new()
        .suffix(".xmp")
        .tempfile()
        .expect("create XMP carrier");
    std::fs::write(
        file.path(),
        include_bytes!("fixtures/xmp_aux_lens_id_canon.xmp"),
    )
    .expect("write XMP carrier");

    let metadata = read_metadata(file.path()).expect("read XMP carrier");
    // The reader keeps its legacy map key; the occurrence carries the
    // XMP-aux group for the composite's qualified dependency.
    assert_eq!(metadata.get_string("XMP:LensID"), Some("1"));
    assert_eq!(metadata.get_string("XMP-tiff:Make"), Some("Canon"));
    assert_eq!(
        metadata.get_string("Composite:LensID"),
        Some("Canon EF 50mm f/1.8"),
    );
}
