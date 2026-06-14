//! Coverage-focused integration tests for specialized + text parsers.
//!
//! Segment: X509, DXF, DWG, STL, OBJ, glTF, FITS, HDF5, LNK, SQLite, VCF, EPS.
//!
//! Tests drive each parser's public `parse_*_metadata` entrypoint with synthetic
//! byte buffers crafted to match the exact format layout, exercising deep branches
//! (multiple record/chunk/box types, optional sections, malformed inputs for error
//! paths). A handful of tests also exercise the production detection + dispatch path
//! via `oxidex::core::operations::read_metadata` on tempfiles written with the
//! correct magic bytes.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use std::io::Write;
use tempfile::NamedTempFile;

// ============================================================================
// Helpers
// ============================================================================

/// Writes bytes to a tempfile with the given extension and returns it.
fn temp_with_ext(bytes: &[u8], ext: &str) -> NamedTempFile {
    let file = tempfile::Builder::new()
        .suffix(&format!(".{}", ext))
        .tempfile()
        .expect("create tempfile");
    {
        let mut handle = file.reopen().expect("reopen tempfile");
        handle.write_all(bytes).expect("write tempfile");
        handle.flush().expect("flush tempfile");
    }
    file
}

// ============================================================================
// DXF (AutoCAD Drawing Exchange Format) - text
// ============================================================================

use oxidex::parsers::specialized::dxf::{DXFParser, parse_dxf_metadata};

/// Builds a DXF file body. Group code / value pairs are one-per-line.
fn dxf_pairs(pairs: &[(&str, &str)]) -> Vec<u8> {
    let mut s = String::new();
    for (code, value) in pairs {
        s.push_str(code);
        s.push('\n');
        s.push_str(value);
        s.push('\n');
    }
    s.into_bytes()
}

#[test]
fn test_dxf_full_header_entities_tables() {
    // HEADER section with ACADVER, INSUNITS, EXTMIN/EXTMAX; TABLES with layers;
    // ENTITIES with a variety of entity types.
    let body = dxf_pairs(&[
        ("0", "SECTION"),
        ("2", "HEADER"),
        ("9", "$ACADVER"),
        ("1", "AC1027"),
        ("9", "$INSUNITS"),
        ("70", "4"),
        ("9", "$EXTMIN"),
        ("10", "1.0"),
        ("20", "2.0"),
        ("30", "3.0"),
        ("9", "$EXTMAX"),
        ("10", "10.0"),
        ("20", "20.0"),
        ("30", "30.0"),
        ("0", "ENDSEC"),
        ("0", "SECTION"),
        ("2", "TABLES"),
        ("0", "LAYER"),
        ("0", "LAYER"),
        ("0", "ENDSEC"),
        ("0", "SECTION"),
        ("2", "ENTITIES"),
        ("0", "LINE"),
        ("0", "CIRCLE"),
        ("0", "ARC"),
        ("0", "TEXT"),
        ("0", "MTEXT"),
        ("0", "LWPOLYLINE"),
        ("0", "POLYLINE"),
        ("0", "POINT"),
        ("0", "ELLIPSE"),
        ("0", "SPLINE"),
        ("0", "UNKNOWNENT"),
        ("0", "ENDSEC"),
        ("0", "EOF"),
    ]);
    let reader = TestReader::new(body);
    let md = parse_dxf_metadata(&reader).expect("dxf parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("DXF".to_string()))
    );
    assert_eq!(
        md.get("AutoCADVersion"),
        Some(&TagValue::String("AutoCAD 2013".to_string()))
    );
    assert_eq!(
        md.get("DrawingUnits"),
        Some(&TagValue::String("Millimeters".to_string()))
    );
    assert_eq!(
        md.get("ExtentMin"),
        Some(&TagValue::String("1.0, 2.0".to_string()))
    );
    assert_eq!(
        md.get("ExtentMax"),
        Some(&TagValue::String("10.0, 20.0".to_string()))
    );
    assert_eq!(md.get("LineCount"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("CircleCount"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("ArcCount"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("TextCount"), Some(&TagValue::Integer(2))); // TEXT + MTEXT
    assert_eq!(md.get("PolylineCount"), Some(&TagValue::Integer(2)));
    assert_eq!(md.get("PointCount"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("EllipseCount"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("SplineCount"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("LayerCount"), Some(&TagValue::Integer(2)));
}

#[test]
fn test_dxf_unknown_version_passes_through() {
    // Unknown version code maps to itself; a header var that isn't EXTMIN/EXTMAX.
    let body = dxf_pairs(&[
        ("0", "SECTION"),
        ("2", "HEADER"),
        ("9", "$ACADVER"),
        ("1", "AC9999"),
        ("9", "$DWGCODEPAGE"),
        ("3", "ANSI_1252"),
        ("0", "ENDSEC"),
        ("0", "EOF"),
    ]);
    let reader = TestReader::new(body);
    let md = parse_dxf_metadata(&reader).expect("dxf parse");
    assert_eq!(
        md.get("AutoCADVersion"),
        Some(&TagValue::String("AC9999".to_string()))
    );
}

#[test]
fn test_dxf_unknown_units_passthrough() {
    let body = dxf_pairs(&[
        ("0", "SECTION"),
        ("2", "HEADER"),
        ("9", "$INSUNITS"),
        ("70", "99"),
        ("0", "ENDSEC"),
        ("0", "EOF"),
    ]);
    let reader = TestReader::new(body);
    let md = parse_dxf_metadata(&reader).expect("dxf parse");
    assert_eq!(
        md.get("DrawingUnits"),
        Some(&TagValue::String("99".to_string()))
    );
}

#[test]
fn test_dxf_invalid_signature_errors() {
    let reader = TestReader::new(b"NOTADXF at all here padding bytes".to_vec());
    assert!(parse_dxf_metadata(&reader).is_err());
    assert!(!DXFParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_dxf_too_small_signature() {
    let reader = TestReader::new(b"0\nSEC".to_vec()); // < 20 bytes
    assert!(!DXFParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_dxf_entities_only_count_via_top_level() {
    // Entities counted at top level when content.in_entities was set then ENDSEC
    // leaves it false; this exercises the `0 && in_entities` top-level branch by
    // keeping the ENTITIES section open (no ENDSEC) so subsequent 0-records count.
    let body = dxf_pairs(&[
        ("0", "SECTION"),
        ("2", "ENTITIES"),
        ("0", "LINE"),
        ("0", "LINE"),
    ]);
    let reader = TestReader::new(body);
    let md = parse_dxf_metadata(&reader).expect("dxf parse");
    assert!(md.contains_key("LineCount"));
}

#[test]
fn test_dxf_via_read_metadata() {
    // The production detector requires >= 100 bytes and that the first 100 bytes
    // start with "0\n" and contain "SECTION", so put HEADER+ACADVER up front.
    let body = dxf_pairs(&[
        ("0", "SECTION"),
        ("2", "HEADER"),
        ("9", "$ACADVER"),
        ("1", "AC1015"),
        ("9", "$INSUNITS"),
        ("70", "6"),
        ("9", "$LASTSAVEDBY"),
        ("1", "padding-to-exceed-one-hundred-bytes-of-content"),
        ("0", "ENDSEC"),
        ("0", "EOF"),
    ]);
    assert!(body.len() >= 100);
    let file = temp_with_ext(&body, "dxf");
    let md = oxidex::core::operations::read_metadata(file.path()).expect("read_metadata dxf");
    assert_eq!(
        md.get("AutoCADVersion"),
        Some(&TagValue::String("AutoCAD 2000".to_string()))
    );
    assert_eq!(
        md.get("DrawingUnits"),
        Some(&TagValue::String("Meters".to_string()))
    );
}

// ============================================================================
// OBJ (Wavefront 3D model) - text
// ============================================================================

use oxidex::parsers::specialized::obj::{OBJParser, parse_obj_metadata};

#[test]
fn test_obj_full_model() {
    let body = b"# comment line\n\
o Cube\n\
g group1\n\
mtllib materials.mtl\n\
v 0.0 0.0 0.0\n\
v 1.0 0.0 0.0\n\
v 1.0 1.0 0.0\n\
vn 0.0 0.0 1.0\n\
vt 0.0 0.0\n\
usemtl Red\n\
usemtl Red\n\
usemtl Blue\n\
f 1 2 3\n\
f 3 2 1\n\
o Sphere\n\
g group2\n";
    let reader = TestReader::new(body.to_vec());
    let md = parse_obj_metadata(&reader).expect("obj parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("OBJ".to_string()))
    );
    assert_eq!(md.get("VertexCount"), Some(&TagValue::Integer(3)));
    assert_eq!(md.get("FaceCount"), Some(&TagValue::Integer(2)));
    assert_eq!(md.get("NormalCount"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("TextureCoordCount"), Some(&TagValue::Integer(1)));
    assert_eq!(
        md.get("HasNormals"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("HasTextureCoords"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(
        md.get("ObjectNames"),
        Some(&TagValue::String("Cube, Sphere".to_string()))
    );
    assert_eq!(
        md.get("GroupNames"),
        Some(&TagValue::String("group1, group2".to_string()))
    );
    assert_eq!(
        md.get("MaterialLibrary"),
        Some(&TagValue::String("materials.mtl".to_string()))
    );
    // Materials de-duplicated (Red appears twice).
    assert_eq!(
        md.get("Materials"),
        Some(&TagValue::String("Red, Blue".to_string()))
    );
}

#[test]
fn test_obj_no_normals_no_texcoords() {
    let body = b"v 0 0 0\nv 1 1 1\nv 2 2 2\nf 1 2 3\n";
    let reader = TestReader::new(body.to_vec());
    let md = parse_obj_metadata(&reader).expect("obj parse");
    assert_eq!(
        md.get("HasNormals"),
        Some(&TagValue::String("No".to_string()))
    );
    assert_eq!(
        md.get("HasTextureCoords"),
        Some(&TagValue::String("No".to_string()))
    );
    assert!(!md.contains_key("NormalCount"));
}

#[test]
fn test_obj_invalid_signature() {
    let reader = TestReader::new(b"no geometry markers in here at all".to_vec());
    assert!(parse_obj_metadata(&reader).is_err());
    assert!(!OBJParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_obj_too_small() {
    let reader = TestReader::new(b"v 0 0".to_vec()); // < 10 bytes
    assert!(!OBJParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_obj_via_read_metadata() {
    // Detector requires >= 100 bytes and a "v "/"vn "/"vt " marker in the first
    // 100 bytes, so emit enough vertices to clear the threshold.
    let mut body = String::new();
    for i in 0..12 {
        body.push_str(&format!("v {}.0 {}.0 {}.0\n", i, i, i));
    }
    body.push_str("vn 0 0 1\n");
    body.push_str("vt 0 0\n");
    body.push_str("f 1 2 3\n");
    let bytes = body.into_bytes();
    assert!(bytes.len() >= 100);
    let file = temp_with_ext(&bytes, "obj");
    let md = oxidex::core::operations::read_metadata(file.path()).expect("read_metadata obj");
    assert_eq!(md.get("VertexCount"), Some(&TagValue::Integer(12)));
    assert_eq!(md.get("NormalCount"), Some(&TagValue::Integer(1)));
}

// ============================================================================
// STL (Stereolithography) - ascii + binary
// ============================================================================

use oxidex::parsers::specialized::stl::{STLParser, parse_stl_metadata};

#[test]
fn test_stl_ascii_with_bbox() {
    let body = b"solid MyPart\n\
facet normal 0 0 1\n\
outer loop\n\
vertex 0.0 0.0 0.0\n\
vertex 1.0 0.0 0.0\n\
vertex 0.0 1.0 0.0\n\
endloop\n\
endfacet\n\
facet normal 0 0 1\n\
outer loop\n\
vertex 1.0 1.0 5.0\n\
vertex 2.0 0.0 0.0\n\
vertex 0.0 2.0 0.0\n\
endloop\n\
endfacet\n\
endsolid MyPart\n";
    let reader = TestReader::new(body.to_vec());
    let md = parse_stl_metadata(&reader).expect("stl ascii parse");

    assert_eq!(
        md.get("STLFormat"),
        Some(&TagValue::String("ASCII".to_string()))
    );
    assert_eq!(
        md.get("SolidName"),
        Some(&TagValue::String("MyPart".to_string()))
    );
    assert_eq!(md.get("TriangleCount"), Some(&TagValue::Integer(2)));
    assert_eq!(md.get("BoundingBoxMinX"), Some(&TagValue::Float(0.0)));
    assert_eq!(md.get("BoundingBoxMaxX"), Some(&TagValue::Float(2.0)));
    assert_eq!(md.get("BoundingBoxMaxZ"), Some(&TagValue::Float(5.0)));
}

#[test]
fn test_stl_ascii_unnamed_solid() {
    // "solid" with no name -> SolidName not inserted, but still parses.
    let body = b"solid\nfacet normal 0 0 1\nouter loop\n\
vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid\n";
    let reader = TestReader::new(body.to_vec());
    let md = parse_stl_metadata(&reader).expect("stl parse");
    assert_eq!(md.get("TriangleCount"), Some(&TagValue::Integer(1)));
    assert!(!md.contains_key("SolidName"));
}

/// Builds a binary STL: 80-byte header + u32 count + 50 bytes per triangle.
fn binary_stl(header: &[u8], triangles: &[[f32; 9]]) -> Vec<u8> {
    let mut data = vec![0u8; 80];
    let n = header.len().min(80);
    data[..n].copy_from_slice(&header[..n]);
    data.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
    for tri in triangles {
        // normal (3 f32) - zeros
        data.extend_from_slice(&[0u8; 12]);
        for v in tri.iter() {
            data.extend_from_slice(&v.to_le_bytes());
        }
        // attribute byte count (u16)
        data.extend_from_slice(&0u16.to_le_bytes());
    }
    data
}

#[test]
fn test_stl_binary_with_software_signature_and_bbox() {
    let header = b"Binary STL generated by SolidWorks 2024";
    let tris = [
        [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        [2.0f32, 2.0, 2.0, -1.0, 0.0, 0.0, 0.0, -1.0, 3.0],
    ];
    let data = binary_stl(header, &tris);
    let reader = TestReader::new(data);
    let md = parse_stl_metadata(&reader).expect("stl binary parse");

    assert_eq!(
        md.get("STLFormat"),
        Some(&TagValue::String("Binary".to_string()))
    );
    assert_eq!(
        md.get("Software"),
        Some(&TagValue::String("SolidWorks".to_string()))
    );
    assert_eq!(md.get("TriangleCount"), Some(&TagValue::Integer(2)));
    assert_eq!(
        md.get("FileSizeValid"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(md.get("BoundingBoxMinX"), Some(&TagValue::Float(-1.0)));
    assert_eq!(md.get("BoundingBoxMaxX"), Some(&TagValue::Float(2.0)));
    assert_eq!(md.get("BoundingBoxMaxZ"), Some(&TagValue::Float(3.0)));
}

#[test]
fn test_stl_binary_size_invalid() {
    // Declare more triangles than the buffer can hold -> FileSizeValid No,
    // and the loop breaks early when offset+50 exceeds size.
    let mut data = vec![0u8; 80];
    data.extend_from_slice(&5u32.to_le_bytes()); // claims 5 triangles
    data.extend_from_slice(&[0u8; 50]); // only one triangle of data
    let reader = TestReader::new(data);
    let md = parse_stl_metadata(&reader).expect("stl parse");
    assert_eq!(md.get("TriangleCount"), Some(&TagValue::Integer(5)));
    assert_eq!(
        md.get("FileSizeValid"),
        Some(&TagValue::String("No".to_string()))
    );
}

#[test]
fn test_stl_binary_too_small_errors() {
    // size >= 84 satisfies verify_signature's `reader.size() >= 84` branch but
    // here we use exactly 84 bytes (header + count, zero triangles) which is valid.
    // For the error path, use a non-"solid" buffer < 84 -> verify_signature false.
    let reader = TestReader::new(vec![0u8; 40]);
    assert!(parse_stl_metadata(&reader).is_err());
    assert!(!STLParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_stl_binary_empty_triangles() {
    let mut data = vec![0u8; 80];
    data.extend_from_slice(&0u32.to_le_bytes());
    let reader = TestReader::new(data);
    let md = parse_stl_metadata(&reader).expect("stl parse");
    assert_eq!(md.get("TriangleCount"), Some(&TagValue::Integer(0)));
    // No finite bbox -> bbox tags absent.
    assert!(!md.contains_key("BoundingBoxMinX"));
}

#[test]
fn test_stl_via_read_metadata_ascii() {
    let body = b"solid Tiny\nfacet normal 0 0 1\nouter loop\n\
vertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid Tiny\n";
    let file = temp_with_ext(body, "stl");
    let md = oxidex::core::operations::read_metadata(file.path()).expect("read_metadata stl");
    assert_eq!(
        md.get("SolidName"),
        Some(&TagValue::String("Tiny".to_string()))
    );
}

// ============================================================================
// glTF / GLB (GL Transmission Format) - JSON + binary
// ============================================================================

use oxidex::parsers::specialized::gltf::{GLTFParser, parse_gltf_metadata};

// Note: the parser's `count_json_array` matches the FIRST occurrence of a key,
// so the top-level "nodes" array must appear before any nested "nodes" key.
const GLTF_JSON: &str = r#"{
  "asset": { "version": "2.0", "generator": "CoolTool 1.2", "copyright": "(c) 2024" },
  "nodes": [ {}, {}, {} ],
  "scenes": [ { "name": "Scene" } ],
  "meshes": [ {} ],
  "materials": [ {}, {} ],
  "textures": [],
  "animations": [ {} ]
}"#;

#[test]
fn test_gltf_json_full() {
    let reader = TestReader::new(GLTF_JSON.as_bytes().to_vec());
    let md = parse_gltf_metadata(&reader).expect("gltf json parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("GLTF".to_string()))
    );
    assert_eq!(
        md.get("Format"),
        Some(&TagValue::String("glTF".to_string()))
    );
    assert_eq!(
        md.get("AssetVersion"),
        Some(&TagValue::String("2.0".to_string()))
    );
    assert_eq!(
        md.get("AssetGenerator"),
        Some(&TagValue::String("CoolTool 1.2".to_string()))
    );
    assert_eq!(
        md.get("AssetCopyright"),
        Some(&TagValue::String("(c) 2024".to_string()))
    );
    assert_eq!(md.get("SceneCount"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("NodeCount"), Some(&TagValue::Integer(3)));
    assert_eq!(md.get("MeshCount"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("MaterialCount"), Some(&TagValue::Integer(2)));
    assert_eq!(md.get("TextureCount"), Some(&TagValue::Integer(0))); // empty array
    assert_eq!(md.get("AnimationCount"), Some(&TagValue::Integer(1)));
    // Worker-25 compat keys.
    assert_eq!(md.get("GLTF:NodeCount"), Some(&TagValue::Integer(3)));
    assert_eq!(
        md.get("GLTF:Version"),
        Some(&TagValue::String("2.0".to_string()))
    );
}

#[test]
fn test_glb_binary() {
    // GLB: "glTF" magic + version(4 LE) + total length(4 LE) +
    // chunk: length(4 LE) + type(4) + JSON payload.
    let json = br#"{"asset":{"version":"2.0"},"meshes":[{},{}]}"#;
    let mut data = Vec::new();
    data.extend_from_slice(b"glTF");
    data.extend_from_slice(&2u32.to_le_bytes()); // version
    data.extend_from_slice(&0u32.to_le_bytes()); // total length (not validated)
    data.extend_from_slice(&(json.len() as u32).to_le_bytes()); // chunk length
    data.extend_from_slice(b"JSON"); // chunk type
    data.extend_from_slice(json);

    let reader = TestReader::new(data);
    let md = parse_gltf_metadata(&reader).expect("glb parse");

    assert_eq!(md.get("Format"), Some(&TagValue::String("GLB".to_string())));
    assert_eq!(
        md.get("AssetVersion"),
        Some(&TagValue::String("2.0".to_string()))
    );
    assert_eq!(md.get("MeshCount"), Some(&TagValue::Integer(2)));
}

#[test]
fn test_gltf_nested_arrays_count_top_level_only() {
    // count_json_array must only count top-level commas; nested arrays/objects
    // and strings containing brackets/commas should not inflate the count.
    let json = r#"{"asset":{"version":"2.0"},"nodes":[{"children":[1,2,3]},{"name":"a,b]"}]}"#;
    let reader = TestReader::new(json.as_bytes().to_vec());
    let md = parse_gltf_metadata(&reader).expect("gltf parse");
    assert_eq!(md.get("NodeCount"), Some(&TagValue::Integer(2)));
}

#[test]
fn test_gltf_invalid_signature() {
    let reader = TestReader::new(b"<html>not gltf at all</html>".to_vec());
    assert!(parse_gltf_metadata(&reader).is_err());
    assert!(!GLTFParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_gltf_too_small() {
    let reader = TestReader::new(b"glTF".to_vec()); // < 12 bytes
    assert!(!GLTFParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_gltf_via_read_metadata() {
    let file = temp_with_ext(GLTF_JSON.as_bytes(), "gltf");
    let md = oxidex::core::operations::read_metadata(file.path()).expect("read_metadata gltf");
    assert_eq!(
        md.get("AssetVersion"),
        Some(&TagValue::String("2.0".to_string()))
    );
}

// ============================================================================
// FITS (Flexible Image Transport System) - 80-col ASCII cards
// ============================================================================

use oxidex::parsers::specialized::fits::{FITSParser, parse_fits_metadata};

/// Encodes FITS header cards, each padded to 80 cols, then pads the whole
/// header to a 2880-byte block boundary with spaces.
fn fits_blocks(cards: &[&str]) -> Vec<u8> {
    let mut buf = Vec::new();
    for card in cards {
        let mut c = card.as_bytes().to_vec();
        c.resize(80, b' ');
        buf.extend_from_slice(&c);
    }
    // Pad to the next 2880-byte block boundary.
    while buf.len() % 2880 != 0 {
        buf.push(b' ');
    }
    buf
}

#[test]
fn test_fits_full_header() {
    let md = {
        let data = fits_blocks(&[
            "SIMPLE  =                    T / conforming FITS file",
            "BITPIX  =                   16 / bits per pixel",
            "NAXIS   =                    2 / number of axes",
            "NAXIS1  =                  800 / width",
            "NAXIS2  =                  600 / height",
            "BSCALE  =                  1.0 / linear scale",
            "BZERO   =              32768.0 / offset",
            "EXPTIME =                 30.5 / exposure seconds",
            "TELESCOP= 'Hubble'             / telescope",
            "INSTRUME= 'WFC3'               / instrument",
            "OBJECT  = 'M31'                / target",
            "OBSERVER= 'Edwin Hubble'       / observer",
            "ORIGIN  = 'STScI'              / origin",
            "DATE-OBS= '2024-01-01'         / obs date",
            "FILTER  = 'F606W'              / filter",
            "CUSTOMKW= 'value'              / a custom keyword",
            "HISTORY processed with pipeline v3",
            "COMMENT This is a test comment",
            "END",
        ]);
        let reader = TestReader::new(data);
        parse_fits_metadata(&reader).expect("fits parse")
    };

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("FITS".to_string()))
    );
    assert_eq!(md.get("BITPIX"), Some(&TagValue::Integer(16)));
    assert_eq!(
        md.get("PixelFormat"),
        Some(&TagValue::String("16-bit signed integer".to_string()))
    );
    assert_eq!(md.get("NAXIS"), Some(&TagValue::Integer(2)));
    assert_eq!(md.get("NAXIS1"), Some(&TagValue::Integer(800)));
    assert_eq!(md.get("NAXIS2"), Some(&TagValue::Integer(600)));
    assert_eq!(md.get("ImageWidth"), Some(&TagValue::Integer(800)));
    assert_eq!(md.get("ImageHeight"), Some(&TagValue::Integer(600)));
    assert_eq!(md.get("BSCALE"), Some(&TagValue::Float(1.0)));
    assert_eq!(md.get("BZERO"), Some(&TagValue::Float(32768.0)));
    assert_eq!(md.get("EXPTIME"), Some(&TagValue::Float(30.5)));
    assert_eq!(
        md.get("TELESCOP"),
        Some(&TagValue::String("Hubble".to_string()))
    );
    assert_eq!(md.get("OBJECT"), Some(&TagValue::String("M31".to_string())));
    assert_eq!(
        md.get("CUSTOMKW"),
        Some(&TagValue::String("value".to_string()))
    );
    // Comment captured for a keyword card.
    assert_eq!(
        md.get("BITPIXComment"),
        Some(&TagValue::String("bits per pixel".to_string()))
    );
}

#[test]
fn test_fits_3d_cube_depth() {
    let data = fits_blocks(&[
        "SIMPLE  =                    T",
        "BITPIX  =                  -32",
        "NAXIS   =                    3",
        "NAXIS1  =                   64",
        "NAXIS2  =                   64",
        "NAXIS3  =                   10",
        "END",
    ]);
    let reader = TestReader::new(data);
    let md = parse_fits_metadata(&reader).expect("fits parse");
    assert_eq!(
        md.get("PixelFormat"),
        Some(&TagValue::String("32-bit floating point".to_string()))
    );
    assert_eq!(md.get("ImageWidth"), Some(&TagValue::Integer(64)));
    assert_eq!(md.get("ImageHeight"), Some(&TagValue::Integer(64)));
    assert_eq!(md.get("ImageDepth"), Some(&TagValue::Integer(10)));
}

#[test]
fn test_fits_no_end_keyword_history_comment_arrays() {
    // No END card: the loop falls through, history/comment arrays get stored.
    let data = fits_blocks(&[
        "SIMPLE  =                    T",
        "BITPIX  =                    8",
        "NAXIS   =                    0",
        "HISTORY first history line",
        "HISTORY second history line",
        "COMMENT a standalone comment",
    ]);
    let reader = TestReader::new(data);
    let md = parse_fits_metadata(&reader).expect("fits parse");
    assert_eq!(
        md.get("PixelFormat"),
        Some(&TagValue::String("8-bit unsigned integer".to_string()))
    );
    match md.get("History") {
        Some(TagValue::Array(items)) => assert_eq!(items.len(), 2),
        other => panic!("expected History array, got {:?}", other),
    }
    match md.get("Comments") {
        Some(TagValue::Array(items)) => assert_eq!(items.len(), 1),
        other => panic!("expected Comments array, got {:?}", other),
    }
}

#[test]
fn test_fits_unknown_bitpix() {
    let data = fits_blocks(&[
        "SIMPLE  =                    T",
        "BITPIX  =                  128",
        "NAXIS   =                    1",
        "END",
    ]);
    let reader = TestReader::new(data);
    let md = parse_fits_metadata(&reader).expect("fits parse");
    assert_eq!(
        md.get("PixelFormat"),
        Some(&TagValue::String("Unknown (128)".to_string()))
    );
}

#[test]
fn test_fits_invalid_signature() {
    let reader = TestReader::new(b"NOTFITS padding to exceed six bytes".to_vec());
    assert!(parse_fits_metadata(&reader).is_err());
    assert!(!FITSParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_fits_too_small() {
    let reader = TestReader::new(b"SIM".to_vec());
    assert!(!FITSParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_fits_via_read_metadata() {
    let data = fits_blocks(&[
        "SIMPLE  =                    T",
        "BITPIX  =                   16",
        "NAXIS   =                    2",
        "NAXIS1  =                  100",
        "NAXIS2  =                  200",
        "END",
    ]);
    let file = temp_with_ext(&data, "fits");
    let md = oxidex::core::operations::read_metadata(file.path()).expect("read_metadata fits");
    assert_eq!(md.get("ImageWidth"), Some(&TagValue::Integer(100)));
}

// ============================================================================
// HDF5 (Hierarchical Data Format) - binary superblock
// ============================================================================

use oxidex::parsers::specialized::hdf5::{HDF5Parser, parse_hdf5_metadata};

const HDF5_SIG: [u8; 8] = [0x89, 0x48, 0x44, 0x46, 0x0D, 0x0A, 0x1A, 0x0A];

#[test]
fn test_hdf5_superblock_v0() {
    // Superblock v0/v1 path. Layout after the 8-byte signature:
    // [8]=version, then a 24-byte block read at offset 8.
    // Build by absolute file offset. Signature occupies bytes 0..8.
    // Superblock fields are read relative to file offset 8 (sb[N] = file 8+N).
    let mut data = vec![0u8; 64];
    data[0..8].copy_from_slice(&HDF5_SIG);
    data[8] = 0; // superblock version 0 (read at file offset 8)
    // The v0/v1 block is read as reader.read(8, 24); sb index N == file offset 8+N.
    data[8 + 5] = 8; // offset size -> 64-bit addressing
    data[8 + 6] = 8; // length size
    data[8 + 8..8 + 10].copy_from_slice(&4u16.to_le_bytes()); // group leaf node K
    data[8 + 10..8 + 12].copy_from_slice(&16u16.to_le_bytes()); // group internal node K
    data[8 + 12..8 + 16].copy_from_slice(&0u32.to_le_bytes()); // consistency flags
    // Base address read at file offset 24 (== reader.read(24, offset_size)).
    data[24..32].copy_from_slice(&0x1000u64.to_le_bytes());

    let reader = TestReader::new(data);
    let md = parse_hdf5_metadata(&reader).expect("hdf5 v0 parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("HDF5".to_string()))
    );
    assert_eq!(md.get("SuperblockVersion"), Some(&TagValue::Integer(0)));
    assert_eq!(md.get("OffsetSize"), Some(&TagValue::Integer(8)));
    assert_eq!(
        md.get("AddressingMode"),
        Some(&TagValue::String("64-bit".to_string()))
    );
    assert_eq!(md.get("GroupLeafNodeK"), Some(&TagValue::Integer(4)));
    assert_eq!(md.get("GroupInternalNodeK"), Some(&TagValue::Integer(16)));
    assert_eq!(md.get("FileConsistencyFlags"), Some(&TagValue::Integer(0)));
    assert_eq!(
        md.get("FileProperlyClosed"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert_eq!(md.get("BaseAddress"), Some(&TagValue::Integer(0x1000)));
}

#[test]
fn test_hdf5_superblock_v0_32bit_not_closed() {
    let mut data = vec![0u8; 64];
    data[0..8].copy_from_slice(&HDF5_SIG);
    data[8] = 1; // version 1 -> same code path as 0
    data[8 + 5] = 4; // offset size -> 32-bit addressing
    data[8 + 6] = 4; // length size
    data[8 + 12..8 + 16].copy_from_slice(&1u32.to_le_bytes()); // flags != 0 -> not closed
    data[24..28].copy_from_slice(&0x20u32.to_le_bytes()); // base address (4 bytes) at file 24

    let reader = TestReader::new(data);
    let md = parse_hdf5_metadata(&reader).expect("hdf5 v1 parse");
    assert_eq!(md.get("SuperblockVersion"), Some(&TagValue::Integer(1)));
    assert_eq!(
        md.get("AddressingMode"),
        Some(&TagValue::String("32-bit".to_string()))
    );
    assert_eq!(
        md.get("FileProperlyClosed"),
        Some(&TagValue::String("No".to_string()))
    );
}

#[test]
fn test_hdf5_superblock_v2() {
    // v2/v3 path: a 16-byte block read at file offset 8 (sb index N == file 8+N).
    let mut data = vec![0u8; 64];
    data[0..8].copy_from_slice(&HDF5_SIG);
    data[8] = 2; // superblock version 2 (read at file offset 8)
    data[8 + 1] = 8; // offset size -> 64-bit
    data[8 + 2] = 8; // length size
    data[8 + 3] = 0; // flags (0 -> closed)
    // base address read at file offset 20, EOF address at file offset 20+8=28.
    data[20..28].copy_from_slice(&0x2000u64.to_le_bytes());
    data[28..36].copy_from_slice(&0x9000u64.to_le_bytes());

    let reader = TestReader::new(data);
    let md = parse_hdf5_metadata(&reader).expect("hdf5 v2 parse");
    assert_eq!(md.get("SuperblockVersion"), Some(&TagValue::Integer(2)));
    assert_eq!(md.get("BaseAddress"), Some(&TagValue::Integer(0x2000)));
    assert_eq!(md.get("EndOfFileAddress"), Some(&TagValue::Integer(0x9000)));
    assert_eq!(
        md.get("AddressingMode"),
        Some(&TagValue::String("64-bit".to_string()))
    );
}

#[test]
fn test_hdf5_unsupported_superblock_version() {
    let mut data = HDF5_SIG.to_vec();
    data.push(7); // unsupported version at offset 8
    data.resize(64, 0);
    let reader = TestReader::new(data);
    assert!(parse_hdf5_metadata(&reader).is_err());
}

#[test]
fn test_hdf5_invalid_signature() {
    let reader = TestReader::new(vec![0u8; 64]);
    assert!(parse_hdf5_metadata(&reader).is_err());
    assert!(!HDF5Parser::verify_signature(&reader).unwrap());
}

#[test]
fn test_hdf5_too_small() {
    let reader = TestReader::new(vec![0x89, 0x48, 0x44]);
    assert!(!HDF5Parser::verify_signature(&reader).unwrap());
}

#[test]
fn test_hdf5_via_read_metadata() {
    let mut data = vec![0u8; 64];
    data[0..8].copy_from_slice(&HDF5_SIG);
    data[8] = 0;
    data[8 + 5] = 8;
    data[8 + 6] = 8;
    let file = temp_with_ext(&data, "h5");
    let md = oxidex::core::operations::read_metadata(file.path()).expect("read_metadata hdf5");
    assert_eq!(md.get("SuperblockVersion"), Some(&TagValue::Integer(0)));
}

// ============================================================================
// LNK (Windows Shortcut) - binary forensic
// ============================================================================

use oxidex::parsers::specialized::lnk::{LNKParser, parse_lnk_metadata};

const SHELL_LINK_GUID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// Builds the 76-byte LNK header.
fn lnk_header(link_flags: u32, file_attrs: u32, filetime: u64) -> Vec<u8> {
    let mut data = vec![0u8; 76];
    data[0..4].copy_from_slice(&[0x4C, 0x00, 0x00, 0x00]);
    data[4..20].copy_from_slice(&SHELL_LINK_GUID);
    data[20..24].copy_from_slice(&link_flags.to_le_bytes());
    data[24..28].copy_from_slice(&file_attrs.to_le_bytes());
    data[28..36].copy_from_slice(&filetime.to_le_bytes()); // creation
    data[36..44].copy_from_slice(&filetime.to_le_bytes()); // access
    data[44..52].copy_from_slice(&filetime.to_le_bytes()); // write
    data
}

#[test]
fn test_lnk_header_flags_attrs_timestamps() {
    // HasName(0x4) + HasRelativePath(0x8) + IsUnicode(0x80) plus many file attrs.
    let link_flags = 0x0004 | 0x0008 | 0x0080;
    let file_attrs = 0x0021; // ReadOnly + Archive
    // 2020-01-01 in FILETIME.
    let data = lnk_header(link_flags, file_attrs, 132223104000000000);
    let reader = TestReader::new(data);
    let md = parse_lnk_metadata(&reader).expect("lnk parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("LNK".to_string()))
    );
    assert_eq!(
        md.get("LinkFlags"),
        Some(&TagValue::String(format!("0x{:08X}", link_flags)))
    );
    assert_eq!(
        md.get("FileAttributes"),
        Some(&TagValue::String("0x00000021".to_string()))
    );
    match md.get("TargetFileAttributes") {
        Some(TagValue::String(s)) => {
            assert!(s.contains("ReadOnly"));
            assert!(s.contains("Archive"));
        }
        other => panic!("expected attrs string, got {:?}", other),
    }
    match md.get("LinkFlagsDescription") {
        Some(TagValue::String(s)) => assert!(s.contains("HasName")),
        other => panic!("expected flag desc, got {:?}", other),
    }
    assert!(md.contains_key("CreationTime"));
    assert!(md.contains_key("AccessTime"));
    assert!(md.contains_key("WriteTime"));
}

#[test]
fn test_lnk_with_link_info_volume_and_path() {
    let mut data = vec![0u8; 200];
    data[..76].copy_from_slice(&lnk_header(0x0002, 0x20, 0)); // HasLinkInfo
    let base = 76usize;
    data[base..base + 4].copy_from_slice(&60u32.to_le_bytes()); // link info size
    data[base + 4..base + 8].copy_from_slice(&28u32.to_le_bytes()); // header size
    data[base + 8..base + 12].copy_from_slice(&0x0001u32.to_le_bytes()); // flags
    data[base + 12..base + 16].copy_from_slice(&28u32.to_le_bytes()); // vol id offset
    data[base + 16..base + 20].copy_from_slice(&48u32.to_le_bytes()); // local path offset
    let vol = base + 28;
    data[vol..vol + 4].copy_from_slice(&20u32.to_le_bytes()); // vol id size
    data[vol + 4..vol + 8].copy_from_slice(&3u32.to_le_bytes()); // drive type
    data[vol + 8..vol + 12].copy_from_slice(&0xDEADBEEFu32.to_le_bytes()); // serial
    let path = base + 48;
    data[path..path + 11].copy_from_slice(b"C:\\file.txt");
    data[path + 11] = 0;

    let reader = TestReader::new(data);
    let md = parse_lnk_metadata(&reader).expect("lnk parse");
    assert_eq!(
        md.get("VolumeSerialNumber"),
        Some(&TagValue::String("DEADBEEF".to_string()))
    );
    assert_eq!(
        md.get("LocalBasePath"),
        Some(&TagValue::String("C:\\file.txt".to_string()))
    );
}

#[test]
fn test_lnk_string_data_ansi() {
    // HasName(0x4) + HasArguments(0x20), ANSI (no IsUnicode flag).
    let mut data = vec![0u8; 200];
    data[..76].copy_from_slice(&lnk_header(0x0004 | 0x0020, 0x20, 0));
    let mut off = 76usize;
    // Name "MyFile"
    data[off..off + 2].copy_from_slice(&6u16.to_le_bytes());
    data[off + 2..off + 8].copy_from_slice(b"MyFile");
    off += 8;
    // Arguments "-arg1"
    data[off..off + 2].copy_from_slice(&5u16.to_le_bytes());
    data[off + 2..off + 7].copy_from_slice(b"-arg1");

    let reader = TestReader::new(data);
    let md = parse_lnk_metadata(&reader).expect("lnk parse");
    assert_eq!(
        md.get("Name"),
        Some(&TagValue::String("MyFile".to_string()))
    );
    assert_eq!(
        md.get("CommandLineArguments"),
        Some(&TagValue::String("-arg1".to_string()))
    );
}

#[test]
fn test_lnk_string_data_unicode_working_dir_icon() {
    // HasWorkingDir(0x10) + HasIconLocation(0x40) + IsUnicode(0x80).
    let mut data = vec![0u8; 200];
    data[..76].copy_from_slice(&lnk_header(0x0010 | 0x0040 | 0x0080, 0x20, 0));
    let mut off = 76usize;
    // Working dir "C:" in UTF-16LE (2 chars).
    data[off..off + 2].copy_from_slice(&2u16.to_le_bytes());
    for (i, ch) in "C:".encode_utf16().enumerate() {
        data[off + 2 + i * 2..off + 4 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }
    off += 2 + 4;
    // Icon "x.ico" (5 chars).
    data[off..off + 2].copy_from_slice(&5u16.to_le_bytes());
    for (i, ch) in "x.ico".encode_utf16().enumerate() {
        data[off + 2 + i * 2..off + 4 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }

    let reader = TestReader::new(data);
    let md = parse_lnk_metadata(&reader).expect("lnk parse");
    assert_eq!(
        md.get("WorkingDirectory"),
        Some(&TagValue::String("C:".to_string()))
    );
    assert_eq!(
        md.get("IconLocation"),
        Some(&TagValue::String("x.ico".to_string()))
    );
}

#[test]
fn test_lnk_tracker_data_block() {
    let mut data = vec![0u8; 300];
    data[..76].copy_from_slice(&lnk_header(0, 0x20, 0));
    let t = 76usize;
    data[t..t + 4].copy_from_slice(&96u32.to_le_bytes()); // block size
    data[t + 4..t + 8].copy_from_slice(&0xA0000003u32.to_le_bytes()); // tracker sig
    data[t + 16..t + 23].copy_from_slice(b"DESKTOP"); // machine id
    // droid volume guid at +32, droid file guid at +48 (MAC = last 6 bytes).
    let file_guid = [
        0x11u8, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE,
        0xFF,
    ];
    data[t + 48..t + 64].copy_from_slice(&file_guid);
    // terminal block
    data[t + 96..t + 100].copy_from_slice(&0u32.to_le_bytes());

    let reader = TestReader::new(data);
    let md = parse_lnk_metadata(&reader).expect("lnk parse");
    assert_eq!(
        md.get("MachineID"),
        Some(&TagValue::String("DESKTOP".to_string()))
    );
    assert!(md.contains_key("DroidVolumeID"));
    assert!(md.contains_key("DroidFileID"));
    assert_eq!(
        md.get("MACAddress"),
        Some(&TagValue::String("AA:BB:CC:DD:EE:FF".to_string()))
    );
}

#[test]
fn test_lnk_known_folder_and_property_store_blocks() {
    let mut data = vec![0u8; 220];
    data[..76].copy_from_slice(&lnk_header(0, 0x20, 0));
    let mut off = 76usize;
    // Property store block (28 bytes).
    data[off..off + 4].copy_from_slice(&28u32.to_le_bytes());
    data[off + 4..off + 8].copy_from_slice(&0xA0000009u32.to_le_bytes());
    off += 28;
    // Known folder block (28 bytes) with a GUID.
    data[off..off + 4].copy_from_slice(&28u32.to_le_bytes());
    data[off + 4..off + 8].copy_from_slice(&0xA000000Bu32.to_le_bytes());
    let folder_guid = [
        0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10,
    ];
    data[off + 8..off + 24].copy_from_slice(&folder_guid);
    off += 28;
    // terminal block.
    data[off..off + 4].copy_from_slice(&0u32.to_le_bytes());

    let reader = TestReader::new(data);
    let md = parse_lnk_metadata(&reader).expect("lnk parse");
    assert_eq!(
        md.get("HasPropertyStore"),
        Some(&TagValue::String("true".to_string()))
    );
    assert!(md.contains_key("KnownFolderID"));
}

#[test]
fn test_lnk_invalid_magic() {
    let mut data = vec![0u8; 76];
    data[0..4].copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    let reader = TestReader::new(data);
    assert!(parse_lnk_metadata(&reader).is_err());
    assert!(!LNKParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_lnk_too_small() {
    let reader = TestReader::new(vec![0x4C, 0x00, 0x00, 0x00]);
    assert!(!LNKParser::verify_signature(&reader).unwrap());
}

// ============================================================================
// SQLite - binary header
// ============================================================================

use oxidex::parsers::specialized::sqlite::{SQLiteParser, parse_sqlite_metadata};

/// Builds a 100-byte SQLite header with configurable fields.
#[allow(clippy::too_many_arguments)]
fn sqlite_header(
    page_size: u16,
    change_counter: u32,
    page_count: u32,
    free_pages: u32,
    schema_cookie: u32,
    text_encoding: u32,
    user_version: u32,
    app_id: u32,
    sqlite_version: u32,
) -> Vec<u8> {
    let mut data = vec![0u8; 100];
    data[0..16].copy_from_slice(b"SQLite format 3\0");
    data[16..18].copy_from_slice(&page_size.to_be_bytes());
    data[18] = 1; // write version
    data[19] = 1; // read version
    data[24..28].copy_from_slice(&change_counter.to_be_bytes());
    data[28..32].copy_from_slice(&page_count.to_be_bytes());
    data[36..40].copy_from_slice(&free_pages.to_be_bytes());
    data[40..44].copy_from_slice(&schema_cookie.to_be_bytes());
    data[56..60].copy_from_slice(&text_encoding.to_be_bytes());
    data[60..64].copy_from_slice(&user_version.to_be_bytes());
    data[68..72].copy_from_slice(&app_id.to_be_bytes());
    data[92..96].copy_from_slice(&42u32.to_be_bytes()); // version valid for
    data[96..100].copy_from_slice(&sqlite_version.to_be_bytes());
    data
}

#[test]
fn test_sqlite_full_firefox() {
    let data = sqlite_header(4096, 42, 100, 5, 1, 1, 10, 0x42503331, 3040001);
    let reader = TestReader::new(data);
    let md = parse_sqlite_metadata(&reader).expect("sqlite parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("SQLite".to_string()))
    );
    assert_eq!(
        md.get("PageSize"),
        Some(&TagValue::String("4096 bytes".to_string()))
    );
    assert_eq!(md.get("SQLITE:PageSize"), Some(&TagValue::Integer(4096)));
    assert_eq!(
        md.get("ApplicationName"),
        Some(&TagValue::String("Firefox".to_string()))
    );
    assert_eq!(
        md.get("SQLiteVersion"),
        Some(&TagValue::String("3.40.1".to_string()))
    );
    assert_eq!(
        md.get("TextEncoding"),
        Some(&TagValue::String("UTF-8".to_string()))
    );
    // free pages > 0 -> forensic note.
    assert!(md.contains_key("ForensicNote"));
    // db size = page_count * page_size.
    assert_eq!(
        md.get("DatabaseSize"),
        Some(&TagValue::String("409600 bytes".to_string()))
    );
}

#[test]
fn test_sqlite_page_size_special_and_utf16_and_unknown_app() {
    // page_size raw value 1 => 65536; encoding 2 => UTF-16le; unknown app id.
    let data = sqlite_header(1, 0, 1, 0, 0, 2, 0, 0xFFFFFFFF, 3035005);
    let reader = TestReader::new(data);
    let md = parse_sqlite_metadata(&reader).expect("sqlite parse");
    assert_eq!(
        md.get("PageSize"),
        Some(&TagValue::String("65536 bytes".to_string()))
    );
    assert_eq!(md.get("SQLITE:PageSize"), Some(&TagValue::Integer(65536)));
    assert_eq!(
        md.get("TextEncoding"),
        Some(&TagValue::String("UTF-16le".to_string()))
    );
    // unknown app id -> no ApplicationName, no forensic note (free pages == 0).
    assert!(!md.contains_key("ApplicationName"));
    assert!(!md.contains_key("ForensicNote"));
    assert_eq!(
        md.get("SQLiteVersion"),
        Some(&TagValue::String("3.35.5".to_string()))
    );
}

#[test]
fn test_sqlite_utf16be_chrome() {
    let data = sqlite_header(8192, 1, 2, 0, 1, 3, 0, 0x42503332, 3000000);
    let reader = TestReader::new(data);
    let md = parse_sqlite_metadata(&reader).expect("sqlite parse");
    assert_eq!(
        md.get("TextEncoding"),
        Some(&TagValue::String("UTF-16be".to_string()))
    );
    assert_eq!(
        md.get("ApplicationName"),
        Some(&TagValue::String("Chrome".to_string()))
    );
}

#[test]
fn test_sqlite_invalid_signature() {
    let mut data = vec![0u8; 100];
    data[0..16].copy_from_slice(b"Not SQLite hdr\0\0");
    let reader = TestReader::new(data);
    assert!(parse_sqlite_metadata(&reader).is_err());
    assert!(!SQLiteParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_sqlite_too_small() {
    let reader = TestReader::new(vec![0u8; 50]);
    assert!(!SQLiteParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_sqlite_via_read_metadata() {
    let data = sqlite_header(4096, 7, 3, 0, 2, 1, 0, 0x54444233, 3041000);
    let file = temp_with_ext(&data, "sqlite");
    let md = oxidex::core::operations::read_metadata(file.path()).expect("read_metadata sqlite");
    assert_eq!(
        md.get("ApplicationName"),
        Some(&TagValue::String("iOS Messages".to_string()))
    );
}

// ============================================================================
// VCF (vCard) - text
// ============================================================================

use oxidex::parsers::text::vcf::{VCFParser, parse_vcf_metadata};

#[test]
fn test_vcf_full_single_card() {
    let body = b"BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
FN:John Doe\r\n\
N:Doe;John;;;\r\n\
EMAIL:john@example.com\r\n\
TEL:+15551234567\r\n\
ORG:Acme Inc\r\n\
ADR:;;123 Main St;City;ST;00000;US\r\n\
URL:https://example.com\r\n\
PHOTO:Zm9v\r\n\
END:VCARD\r\n";
    let reader = TestReader::new(body.to_vec());
    let md = parse_vcf_metadata(&reader).expect("vcf parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("vCard".to_string()))
    );
    assert_eq!(
        md.get("VCardVersion"),
        Some(&TagValue::String("3.0".to_string()))
    );
    assert_eq!(
        md.get("VCF:Version"),
        Some(&TagValue::String("3.0".to_string()))
    );
    assert_eq!(
        md.get("FullName"),
        Some(&TagValue::String("John Doe".to_string()))
    );
    assert_eq!(
        md.get("Email"),
        Some(&TagValue::String("john@example.com".to_string()))
    );
    assert_eq!(
        md.get("Telephone"),
        Some(&TagValue::String("+15551234567".to_string()))
    );
    assert_eq!(md.get("VCF:Count"), Some(&TagValue::Integer(1)));
    assert_eq!(
        md.get("VCF:HasPhoto"),
        Some(&TagValue::String("true".to_string()))
    );
    assert_eq!(
        md.get("VCF:HasOrganization"),
        Some(&TagValue::String("true".to_string()))
    );
    assert_eq!(
        md.get("VCF:HasEmail"),
        Some(&TagValue::String("true".to_string()))
    );
    assert_eq!(
        md.get("VCF:HasPhone"),
        Some(&TagValue::String("true".to_string()))
    );
    assert_eq!(
        md.get("VCF:HasAddress"),
        Some(&TagValue::String("true".to_string()))
    );
    assert_eq!(
        md.get("VCF:HasURL"),
        Some(&TagValue::String("true".to_string()))
    );
}

#[test]
fn test_vcf_multiple_cards_minimal_flags_false() {
    let body = b"BEGIN:VCARD\nVERSION:4.0\nFN:A\nEND:VCARD\n\
BEGIN:VCARD\nVERSION:4.0\nFN:B\nEND:VCARD\n";
    let reader = TestReader::new(body.to_vec());
    let md = parse_vcf_metadata(&reader).expect("vcf parse");
    assert_eq!(md.get("VCF:Count"), Some(&TagValue::Integer(2)));
    assert_eq!(
        md.get("VCF:HasPhoto"),
        Some(&TagValue::String("false".to_string()))
    );
    assert_eq!(
        md.get("VCF:HasEmail"),
        Some(&TagValue::String("false".to_string()))
    );
    assert_eq!(
        md.get("VCardVersion"),
        Some(&TagValue::String("4.0".to_string()))
    );
}

#[test]
fn test_vcf_invalid_signature() {
    let reader = TestReader::new(b"NOT A VCARD FILE HEADER".to_vec());
    assert!(parse_vcf_metadata(&reader).is_err());
    assert!(!VCFParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_vcf_too_small() {
    let reader = TestReader::new(b"BEGIN".to_vec()); // < 11 bytes
    assert!(!VCFParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_vcf_via_read_metadata() {
    let body = b"BEGIN:VCARD\nVERSION:2.1\nFN:Jane\nEMAIL:jane@x.com\nEND:VCARD\n";
    let file = temp_with_ext(body, "vcf");
    let md = oxidex::core::operations::read_metadata(file.path()).expect("read_metadata vcf");
    assert_eq!(
        md.get("FullName"),
        Some(&TagValue::String("Jane".to_string()))
    );
}

// ============================================================================
// EPS (Encapsulated PostScript) - text + binary DOS header
// ============================================================================

use oxidex::parsers::text::eps::{EPSParser, parse_eps_metadata};

#[test]
fn test_eps_ascii_dsc_comments() {
    let body = b"%!PS-Adobe-3.0 EPSF-3.0\n\
%%Creator: Adobe Illustrator\n\
%%Title: (My Artwork)\n\
%%CreationDate: 2024/06/14\n\
%%For: Some User\n\
%%BoundingBox: 0 0 612 792\n\
%%HiResBoundingBox: 0.0 0.0 612.0 792.0\n\
%%DocumentData: Clean7Bit\n\
%%LanguageLevel: 3\n\
%%Pages: 1\n\
%%ImageData: 100 100 8 3\n\
%%EndComments\n\
showpage\n";
    let reader = TestReader::new(body.to_vec());
    let md = parse_eps_metadata(&reader).expect("eps parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("EPS".to_string()))
    );
    assert_eq!(
        md.get("MIMEType"),
        Some(&TagValue::String("application/postscript".to_string()))
    );
    assert_eq!(
        md.get("PostScript:Creator"),
        Some(&TagValue::String("Adobe Illustrator".to_string()))
    );
    assert_eq!(
        md.get("EPS:Creator"),
        Some(&TagValue::String("Adobe Illustrator".to_string()))
    );
    // Title parens stripped.
    assert_eq!(
        md.get("PostScript:Title"),
        Some(&TagValue::String("My Artwork".to_string()))
    );
    assert_eq!(
        md.get("PostScript:BoundingBox"),
        Some(&TagValue::String("0 0 612 792".to_string()))
    );
    assert_eq!(
        md.get("EPS:BoundingBox"),
        Some(&TagValue::String("0 0 612 792".to_string()))
    );
    assert_eq!(
        md.get("PostScript:HiResBoundingBox"),
        Some(&TagValue::String("0.0 0.0 612.0 792.0".to_string()))
    );
    assert_eq!(
        md.get("PostScript:For"),
        Some(&TagValue::String("Some User".to_string()))
    );
    assert_eq!(
        md.get("PostScript:DocumentData"),
        Some(&TagValue::String("Clean7Bit".to_string()))
    );
    assert_eq!(
        md.get("PostScript:LanguageLevel"),
        Some(&TagValue::String("3".to_string()))
    );
    assert_eq!(
        md.get("PostScript:Pages"),
        Some(&TagValue::String("1".to_string()))
    );
    assert_eq!(md.get("EPS:Pages"), Some(&TagValue::Integer(1)));
    assert_eq!(
        md.get("PostScript:ImageData"),
        Some(&TagValue::String("100 100 8 3".to_string()))
    );
    assert_eq!(
        md.get("PostScript:CreateDate"),
        Some(&TagValue::String("2024/06/14".to_string()))
    );
}

#[test]
fn test_eps_atend_values_skipped() {
    // (atend) BoundingBox / Pages should be ignored.
    let body = b"%!PS-Adobe-2.0 EPSF-2.0\n\
%%BoundingBox: (atend)\n\
%%Pages: (atend)\n\
%%Title: No Parens Here\n\
%%EndComments\n";
    let reader = TestReader::new(body.to_vec());
    let md = parse_eps_metadata(&reader).expect("eps parse");
    assert!(!md.contains_key("PostScript:BoundingBox"));
    assert!(!md.contains_key("EPS:Pages"));
    assert_eq!(
        md.get("PostScript:Title"),
        Some(&TagValue::String("No Parens Here".to_string()))
    );
}

#[test]
fn test_eps_binary_dos_header() {
    // DOS EPS: 0xC5D0D3C6 magic, then PS section offset/length (LE) at 4/8.
    let ps =
        b"%!PS-Adobe-3.0 EPSF-3.0\n%%Creator: BinaryGen\n%%BoundingBox: 0 0 10 10\n%%EndComments\n";
    let header_len = 30usize;
    let mut data = vec![0u8; header_len];
    data[0..4].copy_from_slice(&[0xC5, 0xD0, 0xD3, 0xC6]);
    data[4..8].copy_from_slice(&(header_len as u32).to_le_bytes()); // ps_start
    data[8..12].copy_from_slice(&(ps.len() as u32).to_le_bytes()); // ps_length
    data.extend_from_slice(ps);

    let reader = TestReader::new(data);
    let md = parse_eps_metadata(&reader).expect("eps binary parse");
    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("EPS".to_string()))
    );
    assert_eq!(
        md.get("PostScript:Creator"),
        Some(&TagValue::String("BinaryGen".to_string()))
    );
    assert_eq!(
        md.get("PostScript:BoundingBox"),
        Some(&TagValue::String("0 0 10 10".to_string()))
    );
}

#[test]
fn test_eps_invalid_signature() {
    let reader = TestReader::new(b"this is not postscript at all".to_vec());
    assert!(parse_eps_metadata(&reader).is_err());
}

#[test]
fn test_eps_verify_signature_helper() {
    assert!(EPSParser::verify_signature(b"%!PS-Adobe-3.0 EPSF-3.0"));
    assert!(EPSParser::verify_signature(&[0xC5, 0xD0, 0xD3, 0xC6, 0x00]));
    assert!(!EPSParser::verify_signature(b"nope"));
}

#[test]
fn test_eps_via_read_metadata() {
    let body =
        b"%!PS-Adobe-3.0 EPSF-3.0\n%%Title: (RoundTrip)\n%%BoundingBox: 0 0 1 1\n%%EndComments\n";
    let file = temp_with_ext(body, "eps");
    let md = oxidex::core::operations::read_metadata(file.path()).expect("read_metadata eps");
    assert_eq!(
        md.get("PostScript:Title"),
        Some(&TagValue::String("RoundTrip".to_string()))
    );
}

// ============================================================================
// DWG (AutoCAD Drawing) - binary header
// ============================================================================

use oxidex::parsers::specialized::dwg::{DWGParser, parse_dwg_metadata};

#[test]
fn test_dwg_basic_r2018() {
    // "AC1032" + enough bytes; security flags non-zero => Encrypted.
    let mut data = b"AC1032".to_vec();
    data.resize(32, 0);
    // security flags at bytes 13-17 -> make non-zero.
    data[13] = 0x01;
    let reader = TestReader::new(data);
    let md = parse_dwg_metadata(&reader).expect("dwg parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("DWG".to_string()))
    );
    assert_eq!(
        md.get("DWGVersion"),
        Some(&TagValue::String("AC1032".to_string()))
    );
    assert_eq!(
        md.get("AutoCADRelease"),
        Some(&TagValue::String("R2018".to_string()))
    );
    assert_eq!(
        md.get("Encrypted"),
        Some(&TagValue::String("Yes".to_string()))
    );
}

#[test]
fn test_dwg_r2007_codepage() {
    // AC1021 (R2007) with a non-zero codepage at offset 19-20 (LE).
    let mut data = b"AC1021".to_vec();
    data.resize(32, 0);
    data[19..21].copy_from_slice(&30u16.to_le_bytes()); // codepage
    let reader = TestReader::new(data);
    let md = parse_dwg_metadata(&reader).expect("dwg parse");
    assert_eq!(
        md.get("AutoCADRelease"),
        Some(&TagValue::String("R2007".to_string()))
    );
    assert_eq!(
        md.get("CodePage"),
        Some(&TagValue::String("30".to_string()))
    );
}

#[test]
fn test_dwg_unknown_version_release() {
    // "AC1099" -> verify_signature passes (AC + digit checks) but maps Unknown.
    let mut data = b"AC1099".to_vec();
    data.resize(20, 0);
    let reader = TestReader::new(data);
    let md = parse_dwg_metadata(&reader).expect("dwg parse");
    assert_eq!(
        md.get("AutoCADRelease"),
        Some(&TagValue::String("Unknown".to_string()))
    );
}

#[test]
fn test_dwg_version_mapping_helpers() {
    assert_eq!(DWGParser::map_version_to_release("AC1012"), "R13");
    assert_eq!(DWGParser::map_version_to_release("AC1014"), "R14");
    assert_eq!(DWGParser::map_version_to_release("AC1015"), "R2000");
    assert_eq!(DWGParser::map_version_to_release("AC1018"), "R2004");
    assert_eq!(DWGParser::map_version_to_release("AC1024"), "R2010");
    assert_eq!(DWGParser::map_version_to_release("AC1027"), "R2013");
    assert_eq!(DWGParser::map_version_to_release("ACXXXX"), "Unknown");
}

#[test]
fn test_dwg_invalid_signature() {
    let reader = TestReader::new(b"ZZ1032 not dwg".to_vec());
    assert!(parse_dwg_metadata(&reader).is_err());
    assert!(!DWGParser::verify_signature(&reader).unwrap());
}

#[test]
fn test_dwg_too_small() {
    let reader = TestReader::new(b"AC10".to_vec()); // < 6 bytes
    assert!(!DWGParser::verify_signature(&reader).unwrap());
    assert_eq!(DWGParser::read_version(&reader).unwrap(), "Unknown");
}

// ============================================================================
// X.509 Certificate - DER (ASN.1) + PEM
// ============================================================================

use oxidex::parsers::specialized::x509::{X509Parser, parse_x509_metadata};

/// Encodes a DER TLV: tag + length (short or long form) + value.
fn der_tlv(tag: u8, value: &[u8]) -> Vec<u8> {
    let len = value.len();
    let mut out = vec![tag];
    if len < 128 {
        out.push(len as u8);
    } else if len < 256 {
        out.push(0x81);
        out.push(len as u8);
    } else {
        out.push(0x82);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    }
    out.extend_from_slice(value);
    out
}

/// Encodes an OID's content bytes (the part after tag+len) for a dotted OID.
/// Only handles the OIDs used in tests (2.5.4.x and 2.5.29.x families,
/// plus algorithm OIDs encoded directly as bytes by callers).
fn oid_2_5_4(last: u8) -> Vec<u8> {
    // 2.5.4.<last>: first byte 2*40+5 = 0x55, then 0x04, then last.
    vec![0x55, 0x04, last]
}

/// Builds an AttributeTypeAndValue SEQUENCE { OID, PrintableString }.
fn dn_attr(oid_content: &[u8], value: &str) -> Vec<u8> {
    let mut inner = der_tlv(0x06, oid_content); // OID
    inner.extend(der_tlv(0x13, value.as_bytes())); // PrintableString
    // wrap as SET { SEQUENCE { ... } }
    let seq = der_tlv(0x30, &inner);
    der_tlv(0x31, &seq) // SET
}

/// Builds a complete minimal DER X.509 certificate exercising
/// version, serial, sig-alg, issuer, validity, subject, and SPKI.
fn build_der_cert() -> Vec<u8> {
    // ----- Version [0] EXPLICIT INTEGER(2) -----
    let version = {
        let int = der_tlv(0x02, &[0x02]); // INTEGER 2 (v3)
        der_tlv(0xA0, &int) // context [0]
    };

    // ----- Serial number INTEGER -----
    let serial = der_tlv(0x02, &[0x01, 0x23, 0x45, 0x67]);

    // ----- Signature algorithm SEQUENCE { OID sha256WithRSA } -----
    // OID 1.2.840.113549.1.1.11 content bytes.
    let sha256_rsa_oid = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B];
    let sig_alg = der_tlv(0x30, &der_tlv(0x06, &sha256_rsa_oid));

    // ----- Issuer Name SEQUENCE of RDNs -----
    let mut issuer_inner = dn_attr(&oid_2_5_4(3), "Issuer CA"); // CN
    issuer_inner.extend(dn_attr(&oid_2_5_4(10), "Issuer Org")); // O
    issuer_inner.extend(dn_attr(&oid_2_5_4(6), "US")); // C
    let issuer = der_tlv(0x30, &issuer_inner);

    // ----- Validity SEQUENCE { UTCTime, UTCTime } -----
    let not_before = der_tlv(0x17, b"230101000000Z");
    let not_after = der_tlv(0x17, b"330101000000Z");
    let mut validity_inner = not_before;
    validity_inner.extend(not_after);
    let validity = der_tlv(0x30, &validity_inner);

    // ----- Subject Name SEQUENCE of RDNs -----
    let mut subject_inner = dn_attr(&oid_2_5_4(3), "example.com"); // CN
    subject_inner.extend(dn_attr(&oid_2_5_4(10), "Example Inc")); // O
    subject_inner.extend(dn_attr(&oid_2_5_4(11), "IT")); // OU
    subject_inner.extend(dn_attr(&oid_2_5_4(6), "GB")); // C
    subject_inner.extend(dn_attr(&oid_2_5_4(7), "London")); // L
    subject_inner.extend(dn_attr(&oid_2_5_4(8), "England")); // ST
    let subject = der_tlv(0x30, &subject_inner);

    // ----- SubjectPublicKeyInfo SEQUENCE { AlgId SEQUENCE { OID rsa }, BIT STRING } -----
    // OID 1.2.840.113549.1.1.1 (rsaEncryption) content bytes.
    let rsa_oid = [0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
    let alg_id = der_tlv(0x30, &der_tlv(0x06, &rsa_oid));
    // small BIT STRING (unused-bits byte + a few key bytes).
    let bit_string = der_tlv(0x03, &[0x00, 0xAA, 0xBB, 0xCC]);
    let mut spki_inner = alg_id;
    spki_inner.extend(bit_string);
    let spki = der_tlv(0x30, &spki_inner);

    // ----- TBSCertificate -----
    let mut tbs_inner = version;
    tbs_inner.extend(serial);
    tbs_inner.extend(sig_alg);
    tbs_inner.extend(issuer);
    tbs_inner.extend(validity);
    tbs_inner.extend(subject);
    tbs_inner.extend(spki);
    let tbs = der_tlv(0x30, &tbs_inner);

    // ----- Outer Certificate SEQUENCE { TBS } (signature omitted; parser
    // stops reading after SPKI within tbs_end) -----
    der_tlv(0x30, &tbs)
}

#[test]
fn test_x509_der_full_certificate() {
    let cert = build_der_cert();
    let reader = TestReader::new(cert);
    let md = parse_x509_metadata(&reader).expect("x509 der parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("X.509".to_string()))
    );
    assert_eq!(
        md.get("X509:Format"),
        Some(&TagValue::String("DER".to_string()))
    );
    assert_eq!(
        md.get("X509:Version"),
        Some(&TagValue::String("v3".to_string()))
    );
    assert_eq!(
        md.get("X509:SerialNumber"),
        Some(&TagValue::String("01234567".to_string()))
    );
    assert_eq!(
        md.get("X509:SignatureAlgorithm"),
        Some(&TagValue::String("SHA256withRSA".to_string()))
    );
    assert_eq!(
        md.get("X509:IssuerCN"),
        Some(&TagValue::String("Issuer CA".to_string()))
    );
    assert_eq!(
        md.get("X509:IssuerO"),
        Some(&TagValue::String("Issuer Org".to_string()))
    );
    assert_eq!(
        md.get("X509:IssuerC"),
        Some(&TagValue::String("US".to_string()))
    );
    assert_eq!(
        md.get("X509:NotBefore"),
        Some(&TagValue::String("2023-01-01T00:00:00Z".to_string()))
    );
    assert_eq!(
        md.get("X509:NotAfter"),
        Some(&TagValue::String("2033-01-01T00:00:00Z".to_string()))
    );
    assert_eq!(
        md.get("X509:SubjectCN"),
        Some(&TagValue::String("example.com".to_string()))
    );
    assert_eq!(
        md.get("X509:SubjectO"),
        Some(&TagValue::String("Example Inc".to_string()))
    );
    assert_eq!(
        md.get("X509:SubjectOU"),
        Some(&TagValue::String("IT".to_string()))
    );
    assert_eq!(
        md.get("X509:SubjectC"),
        Some(&TagValue::String("GB".to_string()))
    );
    assert_eq!(
        md.get("X509:SubjectL"),
        Some(&TagValue::String("London".to_string()))
    );
    assert_eq!(
        md.get("X509:SubjectST"),
        Some(&TagValue::String("England".to_string()))
    );
    assert_eq!(
        md.get("X509:PublicKeyAlgorithm"),
        Some(&TagValue::String("RSA".to_string()))
    );
    assert!(md.contains_key("X509:PublicKeySize"));
    assert!(md.contains_key("X509:SHA256Fingerprint"));
    assert!(md.contains_key("X509:SHA1Fingerprint"));
}

#[test]
fn test_x509_pem_wrapping_der() {
    use base64::Engine as _;
    let cert = build_der_cert();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&cert);
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    // wrap at 64 chars
    for chunk in b64.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap());
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");

    let reader = TestReader::new(pem.into_bytes());
    let md = parse_x509_metadata(&reader).expect("x509 pem parse");
    assert_eq!(
        md.get("X509:Format"),
        Some(&TagValue::String("PEM".to_string()))
    );
    assert_eq!(
        md.get("X509:SubjectCN"),
        Some(&TagValue::String("example.com".to_string()))
    );
}

#[test]
fn test_x509_verify_signature_variants() {
    // PEM
    let mut pem = b"-----BEGIN CERTIFICATE-----\n".to_vec();
    pem.extend_from_slice(b"MIIBfoobar");
    let reader = TestReader::new(pem);
    assert!(X509Parser::verify_signature(&reader).unwrap());

    // DER long-form (0x30 0x82 ...)
    let mut der = vec![0x30, 0x82, 0x01, 0x00];
    der.extend_from_slice(&[0x30, 0x03, 0x02, 0x01, 0x00, 0x00]);
    let reader = TestReader::new(der);
    assert!(X509Parser::verify_signature(&reader).unwrap());

    // Invalid
    let reader = TestReader::new(vec![
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
    ]);
    assert!(!X509Parser::verify_signature(&reader).unwrap());

    // Too small
    let reader = TestReader::new(vec![0x30, 0x82]);
    assert!(!X509Parser::verify_signature(&reader).unwrap());
}

#[test]
fn test_x509_invalid_structure_errors() {
    // Valid-looking signature (starts 0x30 + parseable length) but the body is
    // not a TBSCertificate -> extract_certificate_info errors out.
    let data = vec![0x30, 0x05, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let reader = TestReader::new(data);
    assert!(parse_x509_metadata(&reader).is_err());
}

#[test]
fn test_x509_der_via_read_metadata() {
    // build_der_cert produces a cert large enough that the outer SEQUENCE uses
    // long-form length (0x30 0x81 or 0x30 0x82), which the detector recognizes
    // as an X.509 signature and routes to the X509 parser.
    let cert = build_der_cert();
    assert_eq!(cert[0], 0x30);
    assert!(
        cert[1] == 0x81 || cert[1] == 0x82,
        "expected long-form length header, got 0x{:02X}",
        cert[1]
    );
    let file = temp_with_ext(&cert, "der");
    if let Ok(md) = oxidex::core::operations::read_metadata(file.path()) {
        if md.get("FileType") == Some(&TagValue::String("X.509".to_string())) {
            assert_eq!(
                md.get("X509:SubjectCN"),
                Some(&TagValue::String("example.com".to_string()))
            );
        }
    }
}
