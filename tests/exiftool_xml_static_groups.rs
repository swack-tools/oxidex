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
        br#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-exif/1.0/" xmlns:e="http://ns.exiftool.org/EXIF/IFD0/1.0/"><q:ShutterSpeedValue>0.4</q:ShutterSpeedValue><e:ModifyDate>2024-01-02T03:04:05Z</e:ModifyDate><e:DateOnly>2024-01-02</e:DateOnly></rdf:Description></rdf:RDF>"#,
    )
    .unwrap();
    let theirs = content_entries(run(oracle.command(), file.path(), "-G0:1"));
    assert_eq!(theirs["XMP:XMP-exif:ShutterSpeedValue"], 0.4);
    assert_eq!(theirs["EXIF:IFD0:ModifyDate"], "2024:01:02 03:04:05Z");
    assert_eq!(theirs["EXIF:IFD0:DateOnly"], "2024-01-02");
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
    let packet = br#"<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:q="http://ns.exiftool.org/XMP/XMP-dc/1.0/"><q:Title>XMP TITLE</q:Title></rdf:Description></rdf:RDF></x:xmpmeta>"#;
    let mut udta = atom(b"\xa9nam", b"\0\x0c\0\0NATIVE TITLE");
    udta.extend_from_slice(&atom(b"XMP_", packet));
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
    assert_eq!(theirs, "XMP TITLE");
    let ours = read_title(Command::new(env!("CARGO_BIN_EXE_oxidex")));
    assert_eq!(ours, theirs, "current pin: {}", oracle.provenance());
}
