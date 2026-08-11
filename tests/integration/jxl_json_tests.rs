//! JSON-mode regression tests for the JXL `ftyp` box.
//!
//! Step 7 of `OVERHAUL_OXIDEX_PLAN.md`: `Jpeg2000:CompatibleBrands` was being
//! inserted as a stringified JSON array (`"[\"jxl \"]"`) instead of a real
//! `TagValue::Array`, so `-j` output quoted the whole list as one string. The
//! same file also surfaced a second divergence: `Composite:Megapixels`
//! rendered as a quoted JSON string (`"0.026"`) where ExifTool's own `-j`
//! writer emits an unquoted number, because any composite's derived value is
//! stored as `TagValue::String` and the JSON formatter had no path back to a
//! number for it.
//!
//! Model for the `CompatibleBrands` fix: `Jpeg2000.pm:574-579` /
//! `QuickTime.pm:1045-1050` -- `CompatibleBrands`' `ValueConv` is
//! `my @a=($val=~/.{4}/sg); @a=grep(!/\0/,@a); \@a`, a real list
//! (`List => 1`), not a joined string.
//!
//! Model for the `Megapixels` fix: the `exiftool` script's `EscapeJSON`
//! (around line 3807 of the pinned tree's `exiftool`) emits a string value
//! unquoted whenever it looks like a JSON number, regardless of tag.
//!
//! These fixtures are the same box layout as ExifTool's own
//! `t/images/JXL2.jxl` (`ftyp` box with major brand `jxl ` and one compatible
//! brand `jxl `, plus a `jxlp` box carrying the SizeHeader for 200x130), built
//! inline so the test is hermetic -- no path into `/tmp` or the oracle cache.

use oxidex::cli::output_formatter::{JsonFormatter, OutputFormatter};
use oxidex::core::read_metadata;
use std::io::Write;

/// JXL container signature box: size(4) + "JXL "(4) + 4-byte signature payload.
/// Byte-for-byte what `src/parsers/image/jxl.rs`'s own `container()` test
/// helper uses.
fn signature_box() -> Vec<u8> {
    b"\0\0\0\x0cJXL \x0d\x0a\x87\x0a".to_vec()
}

/// `ftyp` box: major_brand "jxl ", minor_version 0, one compatible brand
/// "jxl " -- the same bytes as ExifTool's own `t/images/JXL2.jxl` ftyp box.
fn ftyp_box() -> Vec<u8> {
    b"\0\0\0\x14ftypjxl \0\0\0\0jxl ".to_vec()
}

/// `jxlp` (partial codestream) box carrying the SizeHeader ExifTool's own
/// `t/images/JXL.jxl` sample uses, which decodes to 200x130
/// (`ProcessJXLCodestream`, Jpeg2000.pm). Layout: size(4) + "jxlp"(4) +
/// jxlp-index(4) + codestream bytes.
fn jxlp_box() -> Vec<u8> {
    let mut codestream = vec![0xFF, 0x0A, 0x08, 0x04, 0x8E, 0x81, 0x3C];
    codestream.resize(64, 0);
    let mut data = Vec::new();
    let box_size = 8 + 4 + codestream.len() as u32;
    data.extend_from_slice(&box_size.to_be_bytes());
    data.extend_from_slice(b"jxlp");
    data.extend_from_slice(&[0, 0, 0, 0]); // jxlp index
    data.extend_from_slice(&codestream);
    data
}

/// Writes the synthetic JXL2.jxl-shaped container to a temp file and reads it
/// back through the full `read_metadata` pipeline (parser + Composite
/// derivation), matching what `oxidex -j` does.
fn read_synthetic_jxl2() -> serde_json::Value {
    let mut data = Vec::new();
    data.extend_from_slice(&signature_box());
    data.extend_from_slice(&ftyp_box());
    data.extend_from_slice(&jxlp_box());

    let mut file = tempfile::Builder::new()
        .suffix(".jxl")
        .tempfile()
        .expect("create temp file");
    file.write_all(&data).expect("write temp jxl bytes");
    file.flush().expect("flush temp jxl bytes");

    let metadata = read_metadata(file.path()).expect("read_metadata on synthetic JXL container");
    let formatter = JsonFormatter;
    let json_text = formatter.format(&metadata, None);
    let parsed: serde_json::Value = serde_json::from_str(&json_text).expect("valid JSON output");
    parsed[0].clone()
}

/// `Jpeg2000:CompatibleBrands` must serialize as a real JSON array, matching
/// the oracle's `-j` shape on JXL2.jxl: `"Jpeg2000:CompatibleBrands": ["jxl "]`.
/// It must not be the old stringified form `"[\"jxl \"]"`.
#[test]
fn compatible_brands_is_a_real_json_array() {
    let obj = read_synthetic_jxl2();
    let brands = obj
        .get("Jpeg2000:CompatibleBrands")
        .unwrap_or_else(|| panic!("Jpeg2000:CompatibleBrands missing from {obj}"));

    assert!(
        brands.is_array(),
        "CompatibleBrands should be a JSON array, got {brands:?}"
    );
    assert_eq!(
        brands.as_array().unwrap(),
        &[serde_json::Value::String("jxl ".to_string())],
    );
}

/// `Composite:Megapixels` must serialize as a JSON number, matching the
/// oracle's `-j` shape on JXL2.jxl: `"Composite:Megapixels": 0.026`
/// (200x130 -> 200*130/1_000_000). It must not be the quoted string "0.026".
#[test]
fn megapixels_is_a_json_number() {
    let obj = read_synthetic_jxl2();
    let megapixels = obj
        .get("Composite:Megapixels")
        .unwrap_or_else(|| panic!("Composite:Megapixels missing from {obj}"));

    assert!(
        megapixels.is_number(),
        "Megapixels should be a JSON number, got {megapixels:?}"
    );
    assert_eq!(megapixels.as_f64(), Some(0.026));
}
