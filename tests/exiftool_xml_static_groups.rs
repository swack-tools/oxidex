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
