//! ExifTool's `-X` export preserves the source groups encoded in its RDF URI.

use oxidex::exiftool_oracle;
use serde_json::{Map, Value};
use std::path::Path;
use std::process::Command;

fn run(mut command: Command, file: &Path, groups: &str) -> Map<String, Value> {
    let output = command
        .args(["-a", groups, "-j"])
        .arg(file)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Value = serde_json::from_slice(&output.stdout).unwrap();
    rows[0].as_object().unwrap().clone()
}

fn content_entries(mut tags: Map<String, Value>) -> Map<String, Value> {
    tags.retain(|key, _| {
        key != "SourceFile" && key != "ExifTool:ExifToolVersion" && !key.starts_with("File:System:")
    });
    tags
}

#[test]
fn exiftool_x_export_matches_all_grouped_content() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping ExifTool -X parity: pinned oracle unavailable");
        return;
    };
    let file = exiftool_oracle::capability_sample(oracle)
        .unwrap()
        .parent()
        .unwrap()
        .join("XMP.xml");
    let theirs = content_entries(run(oracle.command(), &file, "-G0:1"));
    assert_eq!(theirs["XML:XML-Composite:Aperture"], 9.4);
    assert_eq!(theirs["EXIF:ExifIFD:Flash"], "No Flash");
    assert_eq!(theirs["XML:XML-Composite:LightValue"], 14.2);
    assert_eq!(
        theirs["XML:XML-Composite:ShutterSpeed"],
        "0.00469483568075117"
    );
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        &file,
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());

    // Group 4 is assigned by same-name occurrence order. Check source XML
    // copies beside newly computed Composite tags. ExifToolVersion is omitted
    // because OxiDex does not emit its own process-version pseudo-tag.
    let mut theirs = content_entries(run(oracle.command(), &file, "-G0:1:4"));
    let mut ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        &file,
        "-G0:1:4",
    ));
    // ExifTool's own process-version tag precedes the exported XML property,
    // so its family-4 copy count differs when OxiDex omits that pseudo-tag.
    let comparable = |key: &str| {
        key != "XML:XML-ExifTool:Copy1:ExifToolVersion" && key != "XML:XML-ExifTool:ExifToolVersion"
    };
    theirs.retain(|key, _| comparable(key));
    ours.retain(|key, _| comparable(key));
    assert_eq!(ours, theirs, "all static source groups and copy positions");
    for key in [
        "XML:XML-Composite:Copy1:Aperture",
        "XML:XML-Composite:Copy1:FocalLength35efl",
        "XML:XML-Composite:Copy1:ImageSize",
        "XML:XML-Composite:Copy1:Megapixels",
        "XML:XML-Composite:Copy1:ShutterSpeed",
        "XML:XML-Composite:LightValue",
    ] {
        assert_eq!(ours.get(key), theirs.get(key), "{key} copy placement");
    }
}

#[test]
fn static_uri_alias_coexists_with_ordinary_xmp_namespace() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping mixed-namespace parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/"><q:Make>STATIC</q:Make><exif:Make>ADOBE</exif:Make><q:Model>MODEL</q:Model><exif:FNumber>2.8</exif:FNumber></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1:4"));
    assert_eq!(theirs["EXIF:IFD0:Make"], "STATIC");
    assert_eq!(theirs["XMP:XMP-exif:Copy1:Make"], "ADOBE");
    assert_eq!(theirs["XMP:XMP-exif:FNumber"], 2.8);
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1:4",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn static_uri_structures_and_uppercase_ids_follow_source_namespace() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping structured static URI parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:CAMERA_ID>abc</q:CAMERA_ID><q:XML2>def</q:XML2><q:Settings rdf:parseType="Resource"><q:VALUE>ghi</q:VALUE></q:Settings></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(theirs["EXIF:IFD0:CameraId"], "abc");
    assert_eq!(theirs["EXIF:IFD0:Xml2"], "def");
    assert_eq!(theirs["EXIF:IFD0:SettingsValue"], "ghi");
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn static_uri_defaults_use_generic_rational_and_date_conversion() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static default conversions: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-exif/1.0/" xmlns:e="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:ShutterSpeedValue>0.4</q:ShutterSpeedValue><e:ModifyDate>2024-01-02T03:04:05Z</e:ModifyDate><e:DateOnly>2024-01-02</e:DateOnly><e:Foo>+1/2</e:Foo><e:Bar>1234567890123456789012345678901234567890/2</e:Bar><e:Zero>1/0</e:Zero><e:NegativeZero>-0/0</e:NegativeZero><e:LeadingZero>01/00</e:LeadingZero></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(theirs["XMP:XMP-exif:ShutterSpeedValue"], 0.4);
    assert_eq!(theirs["EXIF:IFD0:ModifyDate"], "2024:01:02 03:04:05Z");
    assert_eq!(theirs["EXIF:IFD0:DateOnly"], "2024-01-02");
    assert_eq!(theirs["EXIF:IFD0:Foo"], "+1/2");
    assert_eq!(
        theirs["EXIF:IFD0:Bar"],
        serde_json::json!(6.17283945061728e+38)
    );
    assert_eq!(theirs["EXIF:IFD0:Zero"], "inf");
    assert_eq!(theirs["EXIF:IFD0:NegativeZero"], "inf");
    assert_eq!(theirs["EXIF:IFD0:LeadingZero"], "inf");
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn static_uri_blank_nodes_and_list_structs_do_not_emit_source_field_aliases() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static RDF structure parity: pinned oracle unavailable");
        return;
    };
    for body in [
        br#"<rdf:Description><q:Settings rdf:nodeID="n1"/></rdf:Description><rdf:Description rdf:nodeID="n1"><q:VALUE>ghi</q:VALUE></rdf:Description>"#.as_slice(),
        br#"<rdf:Description xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><q:Settings r:nodeID="n1"/></rdf:Description><rdf:Description xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#" r:nodeID="n1"><q:VALUE>ghi</q:VALUE></rdf:Description>"#.as_slice(),
        br#"<rdf:Description><q:Settings><rdf:Bag><rdf:li rdf:parseType="Resource"><q:VALUE>ghi</q:VALUE></rdf:li></rdf:Bag></q:Settings></rdf:Description>"#.as_slice(),
    ] {
        let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
        let mut packet = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/">"#.to_vec();
        packet.extend_from_slice(body);
        packet.extend_from_slice(b"</rdf:RDF>");
        std::fs::write(file.path(), packet).unwrap();
        let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
        assert_eq!(theirs["EXIF:IFD0:SettingsValue"], "ghi");
        let ours = content_entries(run(Command::new(env!("CARGO_BIN_EXE_oxidex")), file.path(), "-G0:1"));
        assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
    }
}

#[test]
fn ordinary_xmp_blank_node_definition_is_suppressed_only_when_referenced() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping ordinary RDF blank-node parity: pinned oracle unavailable");
        return;
    };
    for body in [
        br#"<rdf:Description><q:Settings rdf:nodeID="n1"/></rdf:Description><rdf:Description rdf:nodeID="n1"><q:Value>ghi</q:Value></rdf:Description>"#.as_slice(),
        br#"<rdf:Description xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><q:Settings r:nodeID="n1"/></rdf:Description><rdf:Description xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#" r:nodeID="n1"><q:Value>ghi</q:Value></rdf:Description>"#.as_slice(),
        br#"<rdf:Description rdf:nodeID="n1"><q:Value>ghi</q:Value></rdf:Description>"#.as_slice(),
    ] {
        let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
        let mut packet = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:q="http://example.test/ns/">"#.to_vec();
        packet.extend_from_slice(body);
        packet.extend_from_slice(b"</rdf:RDF>");
        std::fs::write(file.path(), packet).unwrap();
        let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
        let ours = content_entries(run(Command::new(env!("CARGO_BIN_EXE_oxidex")), file.path(), "-G0:1"));
        assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
    }
}

#[test]
fn aliased_rdf_elements_and_node_ids_do_not_become_xmp_fields() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping aliased RDF structure parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><r:Description><q:Settings r:nodeID="n1"/></r:Description><r:Description r:nodeID="n1"><q:VALUE>ghi</q:VALUE></r:Description></r:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(theirs["EXIF:IFD0:SettingsValue"], "ghi");
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn static_xmp_family_and_adobe_schema_keep_distinct_value_conversions() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static/ordinary XMP collision: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-exif/1.0/" xmlns:exif="http://ns.adobe.com/exif/1.0/"><q:ShutterSpeedValue>0.4</q:ShutterSpeedValue><exif:ShutterSpeedValue>0.4</exif:ShutterSpeedValue></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1:4"));
    assert_eq!(theirs["XMP:XMP-exif:ShutterSpeedValue"], 0.4);
    assert_eq!(theirs["XMP:XMP-exif:Copy1:ShutterSpeedValue"], 0.8);
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1:4",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn static_xmp_family_keeps_source_name_when_schema_has_rename() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static XMP rename parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-exif/1.0/"><q:ExposureBiasValue>1/2</q:ExposureBiasValue></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(
        theirs.get("XMP:XMP-exif:ExposureBiasValue"),
        Some(&serde_json::json!(0.5))
    );
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn rebound_prefix_retains_effective_schema_rename_for_static_uri() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping rebound static XMP rename parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.adobe.com/exif/1.0/"><q:PixelXDimension>11</q:PixelXDimension></rdf:Description><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-exif/1.0/"><q:PixelXDimension>123</q:PixelXDimension></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1:4"));
    assert_eq!(
        theirs.get("XMP:XMP-exif:ExifImageWidth"),
        Some(&serde_json::json!(11))
    );
    assert_eq!(
        theirs.get("XMP:XMP-exif:Copy1:ExifImageWidth"),
        Some(&serde_json::json!(123))
    );
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1:4",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn rebound_prefix_selects_effective_conversion_and_source_groups() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping effective XMP namespace parity: pinned oracle unavailable");
        return;
    };
    let cases = [
        (
            "converted",
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.adobe.com/exif/1.0/"><q:ExposureBiasValue>1/2</q:ExposureBiasValue></rdf:Description><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-exif/1.0/"><q:ExposureBiasValue>3/2</q:ExposureBiasValue></rdf:Description></rdf:RDF>"#,
            "XMP:XMP-exif:Copy1:ExposureCompensation",
            serde_json::json!("+3/2"),
        ),
        (
            "standard-group",
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.adobe.com/exif/1.0/"><q:PixelXDimension>11</q:PixelXDimension></rdf:Description><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:PixelXDimension>123</q:PixelXDimension></rdf:Description></rdf:RDF>"#,
            "XMP:XMP-exif:Copy1:ExifImageWidth",
            serde_json::json!(123),
        ),
        (
            "temporary-prefix",
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://example.com/first"><q:Foo>bar</q:Foo></rdf:Description><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:ExposureBiasValue>3/2</q:ExposureBiasValue></rdf:Description></rdf:RDF>"#,
            "EXIF:IFD0:ExposureBiasValue",
            serde_json::json!(1.5),
        ),
    ];
    for (name, packet, key, expected) in cases {
        let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
        std::fs::write(file.path(), packet).unwrap();
        let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1:4"));
        let ours = content_entries(run(
            Command::new(env!("CARGO_BIN_EXE_oxidex")),
            file.path(),
            "-G0:1:4",
        ));
        assert_eq!(theirs.get(key), Some(&expected), "{name}");
        assert_eq!(ours, theirs, "{name}: current pin: {}", oracle.provenance());
    }
}

#[test]
fn rebound_prefix_keeps_effective_schema_for_structure_fields() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping rebound structure parity: pinned oracle unavailable");
        return;
    };
    let cases = [
        (
            "nested",
            r#"<q:Settings rdf:parseType="Resource"><q:Value>ghi</q:Value></q:Settings>"#,
        ),
        ("shorthand", r#"<q:Settings q:Value="ghi"/>"#),
        (
            "blank-node",
            r#"<q:Settings rdf:nodeID="n1"/><rdf:Description rdf:nodeID="n1"><q:Value>ghi</q:Value></rdf:Description>"#,
        ),
    ];
    for (name, body) in cases {
        let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
        let packet = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.adobe.com/exif/1.0/"><q:PixelXDimension>11</q:PixelXDimension></rdf:Description><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/">{body}</rdf:Description></rdf:RDF>"#,
        );
        std::fs::write(file.path(), packet).unwrap();
        let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1:4"));
        assert_eq!(
            theirs.get("XMP:XMP-exif:SettingsValue"),
            Some(&serde_json::json!("ghi")),
            "{name}"
        );
        let ours = content_entries(run(
            Command::new(env!("CARGO_BIN_EXE_oxidex")),
            file.path(),
            "-G0:1:4",
        ));
        assert_eq!(ours, theirs, "{name}: current pin: {}", oracle.provenance());
    }
}

#[test]
fn static_uri_groups_and_print_values_are_source_defined() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static URI source parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:e="http://ns.exiftool.org/EXIF/ExifIFD/1.0/" xmlns:g="http://ns.exiftool.org/EXIF/Google/1.0/" xmlns:i="http://ns.exiftool.org/IFD0/Custom/1.0/"><e:FocalLength>50</e:FocalLength><g:Foo>bar</g:Foo><i:Foo>baz</i:Foo></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(theirs["EXIF:ExifIFD:FocalLength"], 50);
    assert_eq!(theirs["EXIF:Google:Foo"], "bar");
    assert_eq!(theirs["IFD0:Custom:Foo"], "baz");
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn static_default_does_not_change_colliding_adobe_description() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static/ordinary default collision: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    let long_description = "x".repeat(65_537);
    let packet = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-dc/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/"><q:description>short</q:description><dc:description>{long_description}</dc:description></rdf:Description></rdf:RDF>"#
    );
    std::fs::write(file.path(), packet).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1:4"));
    assert!(theirs.values().any(|value| value == &long_description));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1:4",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn static_makernote_properties_do_not_activate_native_composites() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping maker-note activation parity: pinned oracle unavailable");
        return;
    };
    for group0 in ["MakerNotes", "EXIF", "IFD0"] {
        let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
        let packet = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/{group0}/Canon/1.0/"><q:MinFocalLength>24</q:MinFocalLength><q:MaxFocalLength>70</q:MaxFocalLength></rdf:Description></rdf:RDF>"#
        );
        std::fs::write(file.path(), packet).unwrap();
        let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
        assert_eq!(theirs[&format!("{group0}:Canon:MinFocalLength")], 24);
        assert_eq!(theirs[&format!("{group0}:Canon:MaxFocalLength")], 70);
        assert!(!theirs.contains_key("Composite:Lens"));
        let ours = content_entries(run(
            Command::new(env!("CARGO_BIN_EXE_oxidex")),
            file.path(),
            "-G0:1",
        ));
        assert_eq!(
            ours,
            theirs,
            "{group0}: current pin: {}",
            oracle.provenance()
        );
    }
}

#[test]
fn static_canoncustom_does_not_activate_native_canon_composites() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping CanonCustom composite activation parity: pinned oracle unavailable");
        return;
    };
    for group0 in ["MakerNotes", "EXIF", "Canon"] {
        let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
        let packet = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/{group0}/CanonCustom/1.0/"><q:MinFocalLength>18</q:MinFocalLength><q:MaxFocalLength>55</q:MaxFocalLength></rdf:Description></rdf:RDF>"#
        );
        std::fs::write(file.path(), packet).unwrap();
        let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
        assert!(!theirs.contains_key("Composite:Lens"));
        let ours = content_entries(run(
            Command::new(env!("CARGO_BIN_EXE_oxidex")),
            file.path(),
            "-G0:1",
        ));
        assert_eq!(
            ours,
            theirs,
            "{group0}: current pin: {}",
            oracle.provenance()
        );
    }
}

#[test]
fn static_panasonicraw_name_does_not_suppress_native_rw2_composites() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping PanasonicRaw native activation parity: pinned oracle unavailable");
        return;
    };
    for uri in [
        "http://ns.exiftool.org/PanasonicRaw/Other/1.0/",
        "http://ns.exiftool.org/EXIF/PanasonicRawExtra/1.0/",
    ] {
        let xml = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="{uri}"><q:Foo>bar</q:Foo></rdf:Description></rdf:RDF>"#
        );
        let entries = [
            (4u16, 3u16, 1u32, 6u32),
            (5, 3, 1, 8),
            (6, 3, 1, 2742),
            (7, 3, 1, 3656),
            (
                700,
                1,
                u32::try_from(xml.len()).unwrap(),
                8 + 2 + 5 * 12 + 4,
            ),
        ];
        let mut rw2 = b"II\x55\0".to_vec();
        rw2.extend_from_slice(&8u32.to_le_bytes());
        rw2.extend_from_slice(&u16::try_from(entries.len()).unwrap().to_le_bytes());
        for (id, typ, count, value) in entries {
            rw2.extend_from_slice(&id.to_le_bytes());
            rw2.extend_from_slice(&typ.to_le_bytes());
            rw2.extend_from_slice(&count.to_le_bytes());
            rw2.extend_from_slice(&value.to_le_bytes());
        }
        rw2.extend_from_slice(&0u32.to_le_bytes());
        rw2.extend_from_slice(xml.as_bytes());
        let file = tempfile::Builder::new().suffix(".rw2").tempfile().unwrap();
        std::fs::write(file.path(), rw2).unwrap();
        let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
        let ours = content_entries(run(
            Command::new(env!("CARGO_BIN_EXE_oxidex")),
            file.path(),
            "-G0:1",
        ));
        for tag in ["Composite:ImageWidth", "Composite:ImageHeight"] {
            assert!(
                theirs.contains_key(tag),
                "current pin: {}",
                oracle.provenance()
            );
            assert_eq!(ours.get(tag), theirs.get(tag), "{uri}: {tag}");
        }
    }
}

#[test]
fn static_rdf_base64_binary_uses_decoded_bytes() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static RDF base64 parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:Foo rdf:datatype="http://www.w3.org/2001/XMLSchema#base64Binary">YWJj</q:Foo><q:Short rdf:datatype="http://www.w3.org/2001/XMLSchema#base64Binary">YQ==</q:Short><q:Alias rdf:datatype="base64">YWJj</q:Alias><q:Partial rdf:datatype="base64">not-valid?</q:Partial><q:One rdf:datatype="base64">A</q:One><q:Extra rdf:datatype="base64">abcde</q:Extra><q:Entity rdf:datatype="base64">JmFtcDs=</q:Entity><q:Rational rdf:datatype="base64">MTIvMw==</q:Rational><q:Replacement rdf:datatype="base64">77+9</q:Replacement><q:Mixed rdf:datatype="base64">77+9/w==</q:Mixed></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours.get("EXIF:IFD0:Foo"), theirs.get("EXIF:IFD0:Foo"));
    assert_eq!(
        ours.get("EXIF:IFD0:Foo"),
        Some(&serde_json::json!(
            "(Binary data 3 bytes, use -b option to extract)"
        ))
    );
    assert_eq!(ours.get("EXIF:IFD0:Short"), theirs.get("EXIF:IFD0:Short"));
    assert_eq!(ours.get("EXIF:IFD0:Short"), Some(&serde_json::json!("a")));
    for tag in [
        "EXIF:IFD0:Alias",
        "EXIF:IFD0:Partial",
        "EXIF:IFD0:One",
        "EXIF:IFD0:Extra",
        "EXIF:IFD0:Entity",
        "EXIF:IFD0:Rational",
        "EXIF:IFD0:Replacement",
        "EXIF:IFD0:Mixed",
    ] {
        assert_eq!(ours.get(tag), theirs.get(tag), "{tag}");
    }
    assert_eq!(ours.get("EXIF:IFD0:Mixed"), Some(&serde_json::json!("�?")));
    assert_eq!(ours.get("EXIF:IFD0:Entity"), Some(&serde_json::json!("&")));
    assert_eq!(ours.get("EXIF:IFD0:Rational"), Some(&serde_json::json!(4)));
    let bytes = |mut command: Command| {
        command
            .args(["-b", "-Foo"])
            .arg(file.path())
            .output()
            .unwrap()
            .stdout
    };
    assert_eq!(
        bytes(Command::new(env!("CARGO_BIN_EXE_oxidex"))),
        bytes(oracle.command())
    );
    let partial_bytes = |mut command: Command| {
        command
            .args(["-b", "-Partial"])
            .arg(file.path())
            .output()
            .unwrap()
            .stdout
    };
    assert_eq!(partial_bytes(oracle.command()), [0x9e, 0x8b]);
    assert_eq!(
        partial_bytes(Command::new(env!("CARGO_BIN_EXE_oxidex"))),
        partial_bytes(oracle.command())
    );
    for tag in ["One", "Extra", "Entity", "Rational", "Replacement", "Mixed"] {
        let extract = |mut command: Command| {
            command.arg("-b").arg(format!("-{tag}")).arg(file.path());
            command.output().unwrap().stdout
        };
        assert_eq!(
            extract(Command::new(env!("CARGO_BIN_EXE_oxidex"))),
            extract(oracle.command()),
            "-b -{tag}"
        );
    }
}

#[test]
fn static_rdf_base64_list_item_keeps_decoded_extractable_bytes() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static RDF list base64 parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:Tags><rdf:Bag><rdf:li rdf:datatype="base64">YWJj</rdf:li><rdf:li>plain</rdf:li></rdf:Bag></q:Tags></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours.get("EXIF:IFD0:Tags"), theirs.get("EXIF:IFD0:Tags"));
    let extract = |mut command: Command| {
        let output = command
            .args(["-b", "-Tags"])
            .arg(file.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    assert_eq!(extract(oracle.command()), b"abc\nplain");
    assert_eq!(
        extract(Command::new(env!("CARGO_BIN_EXE_oxidex"))),
        b"abc\nplain"
    );
}

#[test]
fn flattened_static_base64_leaf_keeps_decoded_extractable_bytes() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping flattened static RDF base64 parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(
        file.path(),
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:Settings rdf:parseType="Resource"><q:Blob rdf:datatype="base64Binary">YWJj</q:Blob></q:Settings></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(
        ours.get("EXIF:IFD0:SettingsBlob"),
        theirs.get("EXIF:IFD0:SettingsBlob")
    );
    let extract = |mut command: Command| {
        command
            .args(["-b", "-SettingsBlob"])
            .arg(file.path())
            .output()
            .unwrap()
            .stdout
    };
    assert_eq!(extract(oracle.command()), b"abc");
    assert_eq!(extract(Command::new(env!("CARGO_BIN_EXE_oxidex"))), b"abc");
}

#[test]
fn embedded_static_makernote_keeps_native_canon_composites_active() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping mixed native/static MakerNotes parity: pinned oracle unavailable");
        return;
    };
    let canon = exiftool_oracle::capability_sample(oracle)
        .unwrap()
        .parent()
        .unwrap()
        .join("Canon.jpg");
    let original = std::fs::read(canon).unwrap();
    assert!(original.starts_with(&[0xff, 0xd8]));
    let packet = br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/MakerNotes/Canon/1.0/"><q:MinFocalLength>24</q:MinFocalLength><q:MaxFocalLength>70</q:MaxFocalLength></rdf:Description></rdf:RDF>"#;
    let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    payload.extend_from_slice(packet);
    let length = u16::try_from(payload.len() + 2).unwrap();
    let mut mixed = vec![0xff, 0xd8, 0xff, 0xe1];
    mixed.extend_from_slice(&length.to_be_bytes());
    mixed.extend_from_slice(&payload);
    mixed.extend_from_slice(&original[2..]);
    let file = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
    std::fs::write(file.path(), mixed).unwrap();

    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(theirs["Composite:Lens"], "18.0 - 55.0 mm");
    assert_eq!(ours.get("Composite:Lens"), theirs.get("Composite:Lens"));
    assert_eq!(
        ours.get("Composite:Lens35efl"),
        theirs.get("Composite:Lens35efl")
    );
}

#[test]
fn static_xmp_title_keeps_mov_priority_directory_promotion() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping MOV static XMP priority parity: pinned oracle unavailable");
        return;
    };
    fn atom(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = u32::try_from(payload.len() + 8)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(payload);
        bytes
    }
    for (uri, title) in [
        ("http://ns.exiftool.org/XMP/XMP-dc/1.0/", "XMP TITLE"),
        ("http://ns.exiftool.org/EXIF/IFD0/1.0/", "STATIC EXIF TITLE"),
    ] {
        let packet = format!(
            r#"<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="{uri}"><q:Title>{title}</q:Title></rdf:Description></rdf:RDF></x:xmpmeta>"#
        );
        let mut udta = atom(b"\xa9nam", b"\0\x0c\0\0NATIVE TITLE");
        udta.extend_from_slice(&atom(b"XMP_", packet.as_bytes()));
        let mut mov = atom(b"ftyp", b"qt  \0\0\0\0qt  ");
        mov.extend_from_slice(&atom(b"moov", &atom(b"udta", &udta)));
        let file = tempfile::Builder::new().suffix(".mov").tempfile().unwrap();
        std::fs::write(file.path(), mov).unwrap();
        let read_title = |mut command: Command| -> Value {
            let output = command
                .args(["-j", "-Title"])
                .arg(file.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let rows: Value = serde_json::from_slice(&output.stdout).unwrap();
            let mut tags = rows[0].as_object().unwrap().clone();
            tags.remove("SourceFile");
            assert_eq!(tags.len(), 1, "{tags:?}");
            tags.into_values().next().unwrap()
        };
        let theirs = read_title(oracle.command());
        assert_eq!(theirs, title);
        let ours = read_title(Command::new(env!("CARGO_BIN_EXE_oxidex")));
        assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
    }
}

#[test]
fn static_exif_and_gps_sources_keep_global_composites_active() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping global composite parity: pinned oracle unavailable");
        return;
    };
    for (uri, fields, required) in [
        (
            "http://ns.exiftool.org/EXIF/Exif/1.0/",
            "<q:FNumber>2.8</q:FNumber><q:ExposureTime>1/100</q:ExposureTime><q:ISO>100</q:ISO>",
            "Composite:Aperture",
        ),
        (
            "http://ns.exiftool.org/GPS/GPS/1.0/",
            "<q:GPSLatitude>10</q:GPSLatitude><q:GPSLatitudeRef>N</q:GPSLatitudeRef><q:GPSLongitude>20</q:GPSLongitude><q:GPSLongitudeRef>E</q:GPSLongitudeRef>",
            "Composite:GPSPosition",
        ),
    ] {
        let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
        let packet = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="{uri}">{fields}</rdf:Description></rdf:RDF>"#
        );
        std::fs::write(file.path(), packet).unwrap();
        let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
        assert!(
            theirs.contains_key(required),
            "pinned source changed: {theirs:?}"
        );
        let ours = content_entries(run(
            Command::new(env!("CARGO_BIN_EXE_oxidex")),
            file.path(),
            "-G0:1",
        ));
        assert_eq!(ours, theirs, "{uri}: current pin: {}", oracle.provenance());
        if uri.contains("/GPS/") {
            let ungrouped = |mut command: Command| {
                let output = command.arg("-j").arg(file.path()).output().unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let rows: Value = serde_json::from_slice(&output.stdout).unwrap();
                rows[0].as_object().unwrap().clone()
            };
            let theirs = ungrouped(oracle.command());
            let ours = ungrouped(Command::new(env!("CARGO_BIN_EXE_oxidex")));
            let text = Command::new(env!("CARGO_BIN_EXE_oxidex"))
                .arg(file.path())
                .output()
                .unwrap();
            assert!(text.status.success());
            let text = String::from_utf8(text.stdout).unwrap();
            for tag in ["GPSLatitudeRef", "GPSLongitudeRef"] {
                assert_eq!(
                    ours.get(&format!("GPS:{tag}")),
                    theirs.get(tag),
                    "ungrouped {tag}"
                );
                let line = text
                    .lines()
                    .find(|line| line.starts_with(&format!("GPS:{tag}: ")))
                    .unwrap();
                assert_eq!(
                    line.rsplit_once(": ").unwrap().1,
                    theirs[tag].as_str().unwrap(),
                    "text {tag}"
                );
            }
            for tag in ["GPSLatitudeRef", "GPSLongitudeRef"] {
                let read_short = |mut command: Command| {
                    let output = command
                        .args(["-s3", &format!("-{tag}")])
                        .arg(file.path())
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    output.stdout
                };
                assert_eq!(
                    read_short(Command::new(env!("CARGO_BIN_EXE_oxidex"))),
                    read_short(oracle.command()),
                    "{tag} short display"
                );
            }
        }
    }
}

#[test]
fn binary_list_extraction_uses_newlines_for_static_and_ordinary_xmp() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping XMP list byte parity: pinned oracle unavailable");
        return;
    };
    for (uri, tag, first, second) in [
        (
            "http://ns.exiftool.org/XMP/XMP-GImage/1.0/",
            "ImageData",
            "alpha".to_string(),
            "beta".to_string(),
        ),
        (
            "http://ns.exiftool.org/XMP/XMP-GImage/1.0/",
            "ImageData",
            "A".repeat(40_000),
            "B".repeat(40_000),
        ),
        (
            "http://ns.exiftool.org/XMP/XMP-GImage/1.0/",
            "ImageData",
            "A".repeat(65_537),
            "B".to_string(),
        ),
        (
            "http://purl.org/dc/elements/1.1/",
            "Subject",
            "alpha".to_string(),
            "beta".to_string(),
        ),
        (
            "http://example.com/unknown/",
            "Blob",
            "A".repeat(65_537),
            "tail".to_string(),
        ),
    ] {
        let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
        let packet = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="{uri}"><q:{tag}><rdf:Bag><rdf:li>{first}</rdf:li><rdf:li>{second}</rdf:li></rdf:Bag></q:{tag}></rdf:Description></rdf:RDF>"#
        );
        std::fs::write(file.path(), packet).unwrap();
        let extract = |mut command: Command| {
            let output = command
                .arg("-b")
                .arg(format!("-{tag}"))
                .arg(file.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{tag}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        let theirs = extract(oracle.command());
        let expected = format!("{first}\n{second}");
        assert_eq!(theirs, expected.as_bytes(), "pinned list shape changed");
        let ours = extract(Command::new(env!("CARGO_BIN_EXE_oxidex")));
        assert!(
            ours == theirs,
            "{tag} list bytes differ: ours {} bytes, oracle {} bytes; current pin: {}",
            ours.len(),
            theirs.len(),
            oracle.provenance()
        );
    }
}

#[test]
fn oversized_static_default_text_keeps_its_extractable_source_bytes() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping oversized static source parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    let raw = "A".repeat(65_537);
    let packet = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-GImage/1.0/"><q:ImageData>{raw}</q:ImageData></rdf:Description></rdf:RDF>"#
    );
    std::fs::write(file.path(), packet).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(
        theirs["XMP:XMP-GImage:ImageData"],
        "(Binary data 65537 bytes, use -b option to extract)"
    );
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
    let extract = |mut command: Command| {
        let output = command
            .args(["-b", "-ImageData"])
            .arg(file.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    assert_eq!(extract(oracle.command()), raw.as_bytes());
    assert_eq!(
        extract(Command::new(env!("CARGO_BIN_EXE_oxidex"))),
        raw.as_bytes()
    );
}

#[test]
fn static_binary_named_text_is_extractable_verbatim() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static binary text parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(file.path(), br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:g="http://ns.exiftool.org/XMP/XMP-GImage/1.0/" xmlns:c="http://ns.exiftool.org/XMP/XMP-GCamera/1.0/"><g:ImageData>YWJj</g:ImageData><c:ShotLogData>abcd</c:ShotLogData></rdf:Description></rdf:RDF>"#).unwrap();
    for tag in ["ImageData", "ShotLogData"] {
        let read = |mut command: Command| {
            command
                .arg("-b")
                .arg(format!("-{tag}"))
                .arg(file.path())
                .output()
                .unwrap()
        };
        let theirs = read(oracle.command());
        assert!(theirs.status.success());
        let ours = read(Command::new(env!("CARGO_BIN_EXE_oxidex")));
        assert!(
            ours.status.success(),
            "{tag}: {}",
            String::from_utf8_lossy(&ours.stderr)
        );
        assert_eq!(
            ours.stdout,
            theirs.stdout,
            "{tag}: current pin: {}",
            oracle.provenance()
        );
    }
}

#[test]
fn short_static_xmp_list_in_quicktime_retains_array_and_binary_elements() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping QuickTime static XMP list parity: pinned oracle unavailable");
        return;
    };
    fn atom(name: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = u32::try_from(payload.len() + 8)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(payload);
        bytes
    }
    for (first, second, expected_json, expected_bytes) in [
        (
            "alpha",
            "beta",
            serde_json::json!(["alpha", "beta"]),
            b"alpha\nbeta".as_slice(),
        ),
        (
            "2024-01-02T03:04:05Z",
            "1/2",
            serde_json::json!(["2024:01:02 03:04:05Z", 0.5]),
            b"2024:01:02 03:04:05Z\n0.5".as_slice(),
        ),
    ] {
        let packet = format!(
            r#"<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-dc/1.0/"><q:Tags><rdf:Bag><rdf:li>{first}</rdf:li><rdf:li>{second}</rdf:li></rdf:Bag></q:Tags></rdf:Description></rdf:RDF></x:xmpmeta>"#
        );
        let mov = [
            atom(b"ftyp", b"qt  \0\0\0\0qt  "),
            atom(b"moov", &atom(b"udta", &atom(b"XMP_", packet.as_bytes()))),
        ]
        .concat();
        let file = tempfile::Builder::new().suffix(".mov").tempfile().unwrap();
        std::fs::write(file.path(), mov).unwrap();
        let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
        let ours = content_entries(run(
            Command::new(env!("CARGO_BIN_EXE_oxidex")),
            file.path(),
            "-G0:1",
        ));
        assert_eq!(theirs["XMP:XMP-dc:Tags"], expected_json);
        assert_eq!(ours["XMP:XMP-dc:Tags"], theirs["XMP:XMP-dc:Tags"]);
        for mut command in [oracle.command(), Command::new(env!("CARGO_BIN_EXE_oxidex"))] {
            let output = command
                .args(["-b", "-Tags"])
                .arg(file.path())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, expected_bytes);
        }
    }
}

#[test]
fn short_static_xmp_list_in_eps_retains_array_and_binary_elements() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping EPS static XMP list parity: pinned oracle unavailable");
        return;
    };
    let packet = br#"<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-dc/1.0/"><q:Tags><rdf:Bag><rdf:li>alpha</rdf:li><rdf:li>beta</rdf:li></rdf:Bag></q:Tags></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end='w'?>"#;
    let mut eps = b"%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 1 1\n".to_vec();
    eps.extend_from_slice(packet);
    eps.extend_from_slice(b"\n%%EOF\n");
    let file = tempfile::Builder::new().suffix(".eps").tempfile().unwrap();
    std::fs::write(file.path(), eps).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(
        theirs["XMP:XMP-dc:Tags"],
        serde_json::json!(["alpha", "beta"])
    );
    assert_eq!(ours["XMP:XMP-dc:Tags"], theirs["XMP:XMP-dc:Tags"]);
    for mut command in [oracle.command(), Command::new(env!("CARGO_BIN_EXE_oxidex"))] {
        let output = command
            .args(["-b", "-Tags"])
            .arg(file.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"alpha\nbeta");
    }
}

#[test]
fn static_rational_overflow_spelling_matches_pinned_perl() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static rational overflow parity: pinned oracle unavailable");
        return;
    };
    let big = "9".repeat(309);
    let xml = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:Positive>{big}/1</q:Positive><q:Negative>-{big}/1</q:Negative><q:Indeterminate>{big}/{big}</q:Indeterminate><q:Underflow>1/{big}</q:Underflow><q:ZeroDenominator>{big}/0</q:ZeroDenominator></rdf:Description></rdf:RDF>"#
    );
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(file.path(), xml).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(theirs["EXIF:IFD0:Positive"], "Inf");
    assert_eq!(theirs["EXIF:IFD0:Negative"], "-Inf");
    assert_eq!(theirs["EXIF:IFD0:Indeterminate"], "NaN");
    assert_eq!(theirs["EXIF:IFD0:Underflow"], 0);
    assert_eq!(theirs["EXIF:IFD0:ZeroDenominator"], "inf");
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn named_xmp_literal_infinity_keeps_original_spelling() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping named XMP infinity parity: pinned oracle unavailable");
        return;
    };
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(file.path(), br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:e="http://ns.adobe.com/exif/1.0/"><e:FocalPlaneXResolution>inf</e:FocalPlaneXResolution><e:FocalPlaneYResolution>-inf</e:FocalPlaneYResolution></rdf:Description></rdf:RDF>"#).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(theirs["XMP:XMP-exif:FocalPlaneXResolution"], "inf");
    assert_eq!(theirs["XMP:XMP-exif:FocalPlaneYResolution"], "-inf");
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn named_xmp_computed_rational_overflow_uses_perl_spelling() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping named XMP overflow parity: pinned oracle unavailable");
        return;
    };
    let big = "9".repeat(309);
    let xml = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:e="http://ns.adobe.com/exif/1.0/"><e:FocalPlaneXResolution>{big}/1</e:FocalPlaneXResolution><e:FocalPlaneYResolution>-{big}/1</e:FocalPlaneYResolution></rdf:Description></rdf:RDF>"#
    );
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(file.path(), xml).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(theirs["XMP:XMP-exif:FocalPlaneXResolution"], "Inf");
    assert_eq!(theirs["XMP:XMP-exif:FocalPlaneYResolution"], "-Inf");
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}

#[test]
fn oversized_static_list_and_rational_follow_converted_values() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping oversized static conversion parity: pinned oracle unavailable");
        return;
    };
    let long = "A".repeat(65_537);
    let numerator = "9".repeat(65_537);
    let xml = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:Tags><rdf:Bag><rdf:li>{long}</rdf:li><rdf:li>1/2</rdf:li></rdf:Bag></q:Tags><q:Huge>{numerator}/2</q:Huge></rdf:Description></rdf:RDF>"#
    );
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(file.path(), xml).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(theirs["EXIF:IFD0:Huge"], "Inf");
    assert_eq!(ours.get("EXIF:IFD0:Huge"), theirs.get("EXIF:IFD0:Huge"));
    assert_eq!(ours.get("EXIF:IFD0:Tags"), theirs.get("EXIF:IFD0:Tags"));
    for tag in ["-Tags", "-Huge"] {
        let mut expected = oracle.command();
        let expected = expected
            .args(["-b", tag])
            .arg(file.path())
            .output()
            .unwrap();
        assert!(expected.status.success());
        let mut candidate = Command::new(env!("CARGO_BIN_EXE_oxidex"));
        let candidate = candidate
            .args(["-b", tag])
            .arg(file.path())
            .output()
            .unwrap();
        assert!(
            candidate.status.success(),
            "{}",
            String::from_utf8_lossy(&candidate.stderr)
        );
        assert_eq!(candidate.stdout, expected.stdout, "{tag}");
    }
}

#[test]
fn literal_binary_summary_in_static_list_does_not_mark_later_items_binary() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static literal binary summary parity: pinned oracle unavailable");
        return;
    };
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:Tags><rdf:Bag><rdf:li>(Binary data 3 bytes, use -b option to extract)</rdf:li><rdf:li>1/2</rdf:li></rdf:Bag></q:Tags></rdf:Description></rdf:RDF>"#;
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(file.path(), xml).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours.get("EXIF:IFD0:Tags"), theirs.get("EXIF:IFD0:Tags"));
}

#[test]
fn oversized_static_list_marks_earlier_items_binary() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping reverse oversized static list parity: pinned oracle unavailable");
        return;
    };
    let long = "A".repeat(65_537);
    let xml = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:Tags><rdf:Bag><rdf:li>1/2</rdf:li><rdf:li>{long}</rdf:li></rdf:Bag></q:Tags></rdf:Description></rdf:RDF>"#
    );
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(file.path(), xml).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(ours.get("EXIF:IFD0:Tags"), theirs.get("EXIF:IFD0:Tags"));
    let mut expected = oracle.command();
    let expected = expected
        .args(["-b", "-Tags"])
        .arg(file.path())
        .output()
        .unwrap();
    assert!(expected.status.success());
    let mut candidate = Command::new(env!("CARGO_BIN_EXE_oxidex"));
    let candidate = candidate
        .args(["-b", "-Tags"])
        .arg(file.path())
        .output()
        .unwrap();
    assert!(candidate.status.success());
    assert_eq!(candidate.stdout, expected.stdout);
}

#[test]
fn static_xmpdsa_list_preserves_lexical_values() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping static xmpDSA parity: pinned oracle unavailable");
        return;
    };
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-xmpDSA/1.0/"><q:NormalizedCropCorners><rdf:Seq><rdf:li>01.00</rdf:li><rdf:li>0.50</rdf:li></rdf:Seq></q:NormalizedCropCorners></rdf:Description></rdf:RDF>"#;
    let file = tempfile::Builder::new().suffix(".xmp").tempfile().unwrap();
    std::fs::write(file.path(), xml).unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    let ours = content_entries(run(
        Command::new(env!("CARGO_BIN_EXE_oxidex")),
        file.path(),
        "-G0:1",
    ));
    assert_eq!(
        ours.get("XMP:XMP-xmpDSA:NormalizedCropCorners"),
        theirs.get("XMP:XMP-xmpDSA:NormalizedCropCorners")
    );
    let mut expected = oracle.command();
    let expected = expected
        .args(["-b", "-NormalizedCropCorners"])
        .arg(file.path())
        .output()
        .unwrap();
    assert!(expected.status.success());
    let mut candidate = Command::new(env!("CARGO_BIN_EXE_oxidex"));
    let candidate = candidate
        .args(["-b", "-NormalizedCropCorners"])
        .arg(file.path())
        .output()
        .unwrap();
    assert!(candidate.status.success());
    assert_eq!(candidate.stdout, expected.stdout);
}
