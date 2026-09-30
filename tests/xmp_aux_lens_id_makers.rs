//! Results captured with pinned ExifTool 13.59 and Perl 5.38.2. Source
//! fixtures and command output are retained in the task evidence. The test
//! XML starts at the XMP root to exercise OxiDex's signature detector.

use oxidex::core::operations::read_metadata;

fn xmp(make: &str, id: &str, info: Option<&str>, focal: Option<&str>) -> String {
    let mut fields = format!("<tiff:Make>{make}</tiff:Make><aux:LensID>{id}</aux:LensID>");
    if let Some(info) = info {
        fields.push_str(&format!("<aux:LensInfo>{info}</aux:LensInfo>"));
    }
    if let Some(focal) = focal {
        fields.push_str(&format!("<exif:FocalLength>{focal}</exif:FocalLength>"));
    }
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         <rdf:Description rdf:about=\"\" xmlns:tiff=\"http://ns.adobe.com/tiff/1.0/\" \
         xmlns:aux=\"http://ns.adobe.com/exif/1.0/aux/\" \
         xmlns:exif=\"http://ns.adobe.com/exif/1.0/\">{fields}</rdf:Description>\n\
         </rdf:RDF></x:xmpmeta>\n"
    )
}

fn converted(make: &str, id: &str, info: Option<&str>, focal: Option<&str>) -> Option<String> {
    let file = tempfile::Builder::new()
        .suffix(".xmp")
        .tempfile()
        .expect("create XMP carrier");
    std::fs::write(file.path(), xmp(make, id, info, focal)).expect("write XMP carrier");
    let metadata = read_metadata(file.path()).expect("read XMP carrier");
    assert_eq!(metadata.get_string("XMP:LensID"), Some(id));
    assert_eq!(metadata.get_string("XMP-tiff:Make"), Some(make));
    metadata.get_string("Composite:LensID").map(str::to_owned)
}

#[test]
fn pinned_oracle_maker_matrix() {
    for (make, id, expected) in [
        ("Canon", "1", "Canon EF 50mm f/1.8"),
        (
            "Canon",
            "2",
            "Canon EF 28mm f/2.8 or Sigma 24mm f/2.8 Super Wide II",
        ),
        ("Pentax", "787", "smc PENTAX-F 24-50mm F4"),
        ("Ricoh", "787", "smc PENTAX-F 24-50mm F4"),
        ("Nikon", "4", "AF Nikkor 28mm f/2.8"),
        ("SONY", "1", "Minolta AF 80-200mm F2.8 HS-APO G"),
        ("Sigma", "1", "Unknown (1)"),
        ("Samsung", "1", "Samsung NX 30mm F2 Pancake"),
        ("Leica", "1", "Elmarit-M 21mm f/2.8"),
        ("Other", "1", "Unknown (1)"),
    ] {
        assert_eq!(
            converted(make, id, None, None).as_deref(),
            Some(expected),
            "{make}/{id}"
        );
    }
}

#[test]
fn canon_lens_info_and_focal_length_narrow_ambiguous_id() {
    assert_eq!(
        converted("Canon", "2", Some("24 24 2.8 2.8"), Some("24")).as_deref(),
        Some("Sigma 24mm f/2.8 Super Wide II"),
    );
}

#[test]
fn sony_lens_info_and_focal_length_narrow_generated_alternatives() {
    assert_eq!(
        converted("SONY", "128", Some("18 200 3.5 6.3"), Some("50")).as_deref(),
        Some("Sigma 17-70mm F2.8-4 DC Macro HSM"),
    );
}

#[test]
fn ambiguous_nikon_prefix_is_explicitly_refused() {
    // ID 1 expands to several distinct source labels. Perl iterates its hash
    // keys in process-dependent order, so an ordered Composite value is not
    // source-stable without an additional ordering contract.
    assert_eq!(converted("Nikon", "1", None, None), None);
}
