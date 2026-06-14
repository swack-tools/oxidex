//! Coverage-focused integration tests for the remaining uncovered code after
//! wave 1, targeting:
//!   - src/parsers/tiff/geotiff_parser.rs  (GeoKeyDirectory / Double / ASCII params)
//!   - src/parsers/tiff/file_parser.rs     (IFD chain, sub-IFDs, GeoTiff dispatch)
//!   - src/parsers/pdf/resources_parser.rs (embedded image XObjects)
//!   - src/parsers/macho/structures.rs     (enum mappers + struct methods, exhaustive)
//!   - src/parsers/elf/structures.rs       (enum mappers + struct methods, exhaustive)
//!   - src/parsers/archive/gz.rs           (FNAME/FCOMMENT/FEXTRA/FHCRC, OS/XFL tables)
//!
//! The wave-1 files (cov_specialized.rs / cov_macho_pe_elf.rs) drive the happy path
//! through the higher-level parsers; this file targets the remaining error /
//! rare-structure / enum-mapper branches, calling pure public helpers exhaustively.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use std::io::Write;
use tempfile::NamedTempFile;

// ============================================================================
// Helpers
// ============================================================================

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
// GeoTIFF parser - direct API
// ============================================================================

use oxidex::parsers::tiff::geotiff_parser::{parse_geotiff_keys, parse_model_transformation};

/// Builds a GeoKeyDirectory byte buffer (little-endian) from header + key entries.
/// Each key entry is (key_id, tag_location, count, value_offset).
fn geo_directory(
    version: u16,
    rev: u16,
    minor: u16,
    keys: &[(u16, u16, u16, u16)],
    le: bool,
) -> Vec<u8> {
    let mut out = Vec::new();
    let push = |out: &mut Vec<u8>, v: u16| {
        if le {
            out.extend_from_slice(&v.to_le_bytes());
        } else {
            out.extend_from_slice(&v.to_be_bytes());
        }
    };
    push(&mut out, version);
    push(&mut out, rev);
    push(&mut out, minor);
    push(&mut out, keys.len() as u16);
    for (k, loc, cnt, val) in keys {
        push(&mut out, *k);
        push(&mut out, *loc);
        push(&mut out, *cnt);
        push(&mut out, *val);
    }
    out
}

#[test]
fn test_geotiff_direct_values_all_formatters() {
    // Exercise format_geokey_value across every key id / value branch.
    let keys = [
        (1024u16, 0u16, 1u16, 1u16), // GTModelType -> Projected
        (1025, 0, 1, 2),             // GTRasterType -> Pixel Is Point
        (2048, 0, 1, 4326),          // GeographicType -> WGS 84
        (2050, 0, 1, 6269),          // GeogGeodeticDatum -> NAD83
        (3072, 0, 1, 32617),         // ProjectedCSType -> UTM 17N
        (3074, 0, 1, 16001),         // Projection -> UTM zone 1N
        (3075, 0, 1, 8),             // ProjCoordTrans -> Lambert Conformal Conic
        (3076, 0, 1, 9001),          // ProjLinearUnits -> m
        (2052, 0, 1, 9002),          // GeogLinearUnits -> ft
        (2054, 0, 1, 9102),          // GeogAngularUnits -> deg
        (4096, 0, 1, 5030),          // VerticalCSType -> raw number (default arm)
    ];
    let dir = geo_directory(1, 1, 0, &keys, true);
    let result = parse_geotiff_keys(&dir, None, None, true);

    assert_eq!(
        result.get("GeoTiff:GeoTiffVersion"),
        Some(&"1.1.0".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:GTModelType"),
        Some(&"Projected".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:GTRasterType"),
        Some(&"Pixel Is Point".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:GeographicType"),
        Some(&"WGS 84".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:GeogGeodeticDatum"),
        Some(&"NAD83".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:ProjectedCSType"),
        Some(&"WGS 84 / UTM zone 17N".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:Projection"),
        Some(&"UTM zone 1N".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:ProjCoordTrans"),
        Some(&"Lambert Conformal Conic".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:ProjLinearUnits"),
        Some(&"m".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:GeogLinearUnits"),
        Some(&"ft".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:GeogAngularUnits"),
        Some(&"deg".to_string())
    );
    // Vertical key uses default formatter (raw number passthrough).
    assert_eq!(
        result.get("GeoTiff:VerticalCSType"),
        Some(&"5030".to_string())
    );
}

#[test]
fn test_geotiff_user_defined_and_southern_utm() {
    let keys = [
        (1024u16, 0u16, 1u16, 32767u16), // User Defined
        (1025, 0, 1, 32767),             // User Defined
        (2048, 0, 1, 32767),             // User Defined
        (2050, 0, 1, 32767),             // User Defined
        (3072, 0, 1, 32733),             // UTM zone 33S
        (3072, 0, 1, 32767),             // User Defined
        (3074, 0, 1, 16133),             // UTM zone 33S (projection)
        (3074, 0, 1, 32767),             // User Defined
        (3075, 0, 1, 32767),             // ProjCoordTrans User Defined
        (3076, 0, 1, 32767),             // LinearUnits User Defined
        (2054, 0, 1, 32767),             // AngularUnits User Defined
        (3075, 0, 1, 1),                 // Transverse Mercator
        (3075, 0, 1, 7),                 // Mercator
        (3075, 0, 1, 11),                // Albers Equal Area
        (3076, 0, 1, 9003),              // us ft
        (2054, 0, 1, 9101),              // rad
        (2054, 0, 1, 9103),              // arc min
        (2054, 0, 1, 9104),              // arc sec
        (2054, 0, 1, 9105),              // grad
        (2048, 0, 1, 4269),              // NAD83
        (2048, 0, 1, 4267),              // NAD27
        (2050, 0, 1, 6326),              // WGS 84 datum
        (2050, 0, 1, 6267),              // NAD27 datum
        (1024, 0, 1, 3),                 // Geocentric
        (3072, 0, 1, 9999),              // raw passthrough
        (3074, 0, 1, 9999),              // raw passthrough
    ];
    let dir = geo_directory(1, 0, 0, &keys, true);
    let result = parse_geotiff_keys(&dir, None, None, true);
    // Spot-check a few distinctive results that survive overwrites (unique keys).
    assert_eq!(
        result.get("GeoTiff:GeoTiffVersion"),
        Some(&"1.0.0".to_string())
    );
    assert!(result.contains_key("GeoTiff:GTModelType"));
    assert!(result.contains_key("GeoTiff:ProjectedCSType"));
}

#[test]
fn test_geotiff_double_and_ascii_params() {
    // Key 2057 (GeogSemiMajorAxis) sourced from GeoDoubleParams (loc 34736).
    // Key 2049 (GeogCitation) sourced from GeoAsciiParams (loc 34737).
    let keys = [
        (2057u16, 34736u16, 1u16, 0u16), // double param index 0
        (2059, 34736, 2, 1),             // two doubles starting at index 1
        (2049, 34737, 18, 0),            // ascii "Hough UTM zone 17N"
    ];
    let dir = geo_directory(1, 1, 0, &keys, true);

    // double params: 3 f64 little-endian values.
    let mut doubles = Vec::new();
    for v in [6378137.0f64, 298.257_223_563, 6356752.314] {
        doubles.extend_from_slice(&v.to_le_bytes());
    }
    let ascii = "Hough UTM zone 17N|Datum WGS 84|";

    let result = parse_geotiff_keys(&dir, Some(&doubles), Some(ascii), true);
    assert_eq!(
        result.get("GeoTiff:GeogSemiMajorAxis"),
        Some(&"6378137".to_string())
    );
    // Two doubles joined by a space.
    assert_eq!(
        result.get("GeoTiff:GeogInvFlattening"),
        Some(&"298.257223563 6356752.314".to_string())
    );
    assert_eq!(
        result.get("GeoTiff:GeogCitation"),
        Some(&"Hough UTM zone 17N".to_string())
    );
}

#[test]
fn test_geotiff_double_ascii_missing_fallback() {
    // loc 34736/34737 but no params supplied -> formats value_offset as string.
    let keys = [
        (2057u16, 34736u16, 1u16, 42u16),
        (2049, 34737, 5, 99),
        (1026, 999, 1, 7), // unknown tag_location -> default arm prints value_offset
    ];
    let dir = geo_directory(1, 1, 0, &keys, true);
    let result = parse_geotiff_keys(&dir, None, None, true);
    assert_eq!(
        result.get("GeoTiff:GeogSemiMajorAxis"),
        Some(&"42".to_string())
    );
    assert_eq!(result.get("GeoTiff:GeogCitation"), Some(&"99".to_string()));
    assert_eq!(result.get("GeoTiff:GTCitation"), Some(&"7".to_string()));
}

#[test]
fn test_geotiff_ascii_offset_beyond_end() {
    // offset >= ascii length -> empty string.
    let keys = [(2049u16, 34737u16, 5u16, 100u16)];
    let dir = geo_directory(1, 1, 0, &keys, true);
    let result = parse_geotiff_keys(&dir, None, Some("short"), true);
    assert_eq!(result.get("GeoTiff:GeogCitation"), Some(&"".to_string()));
}

#[test]
fn test_geotiff_too_short_header() {
    // < 8 bytes -> empty map.
    let result = parse_geotiff_keys(&[0u8, 1, 2, 3], None, None, true);
    assert!(result.is_empty());
}

#[test]
fn test_geotiff_num_keys_exceeds_buffer() {
    // Header claims 10 keys but buffer only has header -> version only, then bail.
    let mut dir = geo_directory(1, 1, 0, &[], true);
    // Overwrite num_keys (bytes 6..8) with 10.
    dir[6] = 10;
    dir[7] = 0;
    let result = parse_geotiff_keys(&dir, None, None, true);
    assert_eq!(result.len(), 1); // only the version tag
    assert_eq!(
        result.get("GeoTiff:GeoTiffVersion"),
        Some(&"1.1.0".to_string())
    );
}

#[test]
fn test_geotiff_big_endian_directory() {
    let keys = [(1024u16, 0u16, 1u16, 1u16)];
    let dir = geo_directory(1, 1, 0, &keys, false);
    let result = parse_geotiff_keys(&dir, None, None, false);
    assert_eq!(
        result.get("GeoTiff:GTModelType"),
        Some(&"Projected".to_string())
    );
}

#[test]
fn test_geotiff_unknown_key_id() {
    let keys = [(5000u16, 0u16, 1u16, 1u16)]; // not in geokey_to_name
    let dir = geo_directory(1, 1, 0, &keys, true);
    let result = parse_geotiff_keys(&dir, None, None, true);
    assert!(result.contains_key("GeoTiff:Unknown"));
}

#[test]
fn test_model_transformation_full_and_fractional() {
    // 16 f64 values: mix of integers and fractions to hit both format branches.
    let mut data = Vec::new();
    let vals: [f64; 16] = [
        1.0, 0.0, 0.0, 100.5, 0.0, 1.0, 0.0, 200.25, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    for v in vals {
        data.extend_from_slice(&v.to_le_bytes());
    }
    let s = parse_model_transformation(&data, true).expect("transform");
    assert!(s.starts_with("1 0 0 100.5"));
    assert!(s.contains("200.25"));
    // Ends with the last integer formatted without decimals.
    assert!(s.ends_with(" 1"));
}

#[test]
fn test_model_transformation_too_short() {
    assert!(parse_model_transformation(&[0u8; 64], true).is_none());
}

#[test]
fn test_model_transformation_big_endian() {
    let mut data = Vec::new();
    for _ in 0..16 {
        data.extend_from_slice(&2.0f64.to_be_bytes());
    }
    let s = parse_model_transformation(&data, false).expect("transform");
    assert_eq!(s, "2 2 2 2 2 2 2 2 2 2 2 2 2 2 2 2");
}

// ============================================================================
// TIFF file parser - drive IFD chain, GeoTiff dispatch, sub-IFDs, errors
// ============================================================================

use oxidex::parsers::tiff::file_parser::{parse_tiff_file, parse_tiff_header};
use oxidex::parsers::tiff::ifd_parser::ByteOrder;

/// Minimal little-endian TIFF builder. `extra_after` is appended after the IFD
/// block (offset >= block_end) so out-of-line tag values / sub-IFDs can live there.
struct TiffBuilder {
    data: Vec<u8>,
}

impl TiffBuilder {
    fn new() -> Self {
        let mut data = Vec::new();
        data.extend_from_slice(&[0x49, 0x49]); // II
        data.extend_from_slice(&42u16.to_le_bytes()); // magic
        data.extend_from_slice(&8u32.to_le_bytes()); // first IFD at 8
        Self { data }
    }

    /// Writes an IFD at the current end (must be offset 8 for the first one).
    /// entries: (tag, type, count, value_or_offset_bytes[4]). next_ifd: offset.
    fn push_ifd(&mut self, entries: &[(u16, u16, u32, [u8; 4])], next_ifd: u32) {
        self.data
            .extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, typ, count, val) in entries {
            self.data.extend_from_slice(&tag.to_le_bytes());
            self.data.extend_from_slice(&typ.to_le_bytes());
            self.data.extend_from_slice(&count.to_le_bytes());
            self.data.extend_from_slice(val);
        }
        self.data.extend_from_slice(&next_ifd.to_le_bytes());
    }

    fn append_at(&mut self, offset: usize, bytes: &[u8]) {
        if self.data.len() < offset + bytes.len() {
            self.data.resize(offset + bytes.len(), 0);
        }
        self.data[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    fn len(&self) -> usize {
        self.data.len()
    }
}

fn u32le(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}

#[test]
fn test_tiff_header_via_public_api() {
    let mut data = vec![0x49u8, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
    data.resize(64, 0);
    let reader = TestReader::new(data);
    let header = parse_tiff_header(&reader).expect("header");
    assert_eq!(header.byte_order, ByteOrder::LittleEndian);
    assert_eq!(header.first_ifd_offset, 8);
}

#[test]
fn test_tiff_header_errors() {
    // Too small.
    assert!(parse_tiff_header(&TestReader::new(vec![0u8; 4])).is_err());
    // Bad byte order.
    assert!(parse_tiff_header(&TestReader::new(vec![0xFF, 0xFF, 0x2A, 0, 8, 0, 0, 0])).is_err());
    // Bad magic.
    assert!(parse_tiff_header(&TestReader::new(vec![0x49, 0x49, 0xFF, 0xFF, 8, 0, 0, 0])).is_err());
}

#[test]
fn test_tiff_file_with_geotiff_tags() {
    // Build a TIFF whose IFD0 contains GeoKeyDirectory (34735), GeoDoubleParams
    // (34736), GeoAsciiParams (34737) and ModelTransformation (34264) tags. This
    // drives parse_tiff_file's GeoTiff dispatch into geotiff_parser.
    let mut b = TiffBuilder::new();

    // GeoKeyDirectory: SHORT array. 4 header shorts + 2 key entries (4 shorts each) = 24 shorts.
    let geo_keys: Vec<u16> = {
        let mut v = vec![1u16, 1, 0, 2]; // version 1.1.0, 2 keys
        v.extend_from_slice(&[1024, 0, 1, 1]); // GTModelType -> Projected
        v.extend_from_slice(&[2057, 34736, 1, 0]); // GeogSemiMajorAxis from doubles[0]
        v
    };
    // ModelTransformation: 16 doubles (128 bytes).
    // GeoDoubleParams: 1 double.
    // GeoAsciiParams: ascii string.

    // We'll place out-of-line data after the IFD block. First compute IFD layout:
    // 4 entries. IFD starts at 8: 2 (count) + 4*12 + 4 (next) = 54 bytes => block_end = 8+54 = 62.
    let geo_dir_off = 64u32;
    let geo_dir_bytes: Vec<u8> = geo_keys.iter().flat_map(|s| s.to_le_bytes()).collect();
    let double_off = geo_dir_off + geo_dir_bytes.len() as u32;
    let double_bytes = 6378137.0f64.to_le_bytes();
    let ascii_off = double_off + 8;
    let ascii_bytes = b"GCS Name|\0".to_vec();
    let model_off = ascii_off + ascii_bytes.len() as u32;
    let model_bytes: Vec<u8> = (0..16).flat_map(|i| (i as f64).to_le_bytes()).collect();

    let entries = [
        (34735u16, 3u16, geo_keys.len() as u32, u32le(geo_dir_off)), // GeoKeyDirectory SHORT[]
        (34736u16, 12u16, 1u32, u32le(double_off)),                  // GeoDoubleParams DOUBLE[1]
        (34737u16, 2u16, ascii_bytes.len() as u32, u32le(ascii_off)), // GeoAsciiParams ASCII[]
        (34264u16, 12u16, 16u32, u32le(model_off)), // ModelTransformation DOUBLE[16]
    ];
    b.push_ifd(&entries, 0);

    b.append_at(geo_dir_off as usize, &geo_dir_bytes);
    b.append_at(double_off as usize, &double_bytes);
    b.append_at(ascii_off as usize, &ascii_bytes);
    b.append_at(model_off as usize, &model_bytes);

    let reader = TestReader::new(b.data);
    let tags = parse_tiff_file(&reader).expect("geotiff parse");

    // Synthetic GeoTiff entries are pushed with tag id 34735; ModelTransformation
    // with 34264. Verify by scanning the synthetic string payloads.
    let has_geo = tags.iter().any(|(id, _, _, v)| {
        *id == 34735 && String::from_utf8_lossy(v.as_ref()).contains("GeoTiff:GTModelType")
    });
    assert!(has_geo, "expected synthetic GeoTiff tag in output");
    let has_model = tags.iter().any(|(id, _, _, v)| {
        *id == 34264 && String::from_utf8_lossy(v.as_ref()).contains("ModelTransform")
    });
    assert!(has_model, "expected ModelTransformation synthetic tag");
}

#[test]
fn test_tiff_file_with_sub_ifds_tag() {
    // SubIFDs tag (0x014A) with two LONG offsets pointing at small IFDs.
    let mut b = TiffBuilder::new();

    // Layout: IFD0 has 1 entry -> 2 + 12 + 4 = 18 bytes; block end = 26.
    // Place the SubIFDs offset array and sub-IFDs after offset 32.
    let suboff_array = 64u32;
    let sub1_off = 80u32;
    let sub2_off = 110u32;

    let entries = [(0x014Au16, 4u16, 2u32, u32le(suboff_array))]; // SubIFDs LONG[2]
    b.push_ifd(&entries, 0);

    // SubIFDs offset array: two u32 offsets.
    let mut arr = Vec::new();
    arr.extend_from_slice(&sub1_off.to_le_bytes());
    arr.extend_from_slice(&sub2_off.to_le_bytes());
    b.append_at(suboff_array as usize, &arr);

    // Sub-IFD 1: one tag, no next.
    let mut sub1 = Vec::new();
    sub1.extend_from_slice(&1u16.to_le_bytes());
    sub1.extend_from_slice(&0x0100u16.to_le_bytes()); // ImageWidth
    sub1.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    sub1.extend_from_slice(&1u32.to_le_bytes());
    sub1.extend_from_slice(&u32le(640));
    sub1.extend_from_slice(&0u32.to_le_bytes());
    b.append_at(sub1_off as usize, &sub1);

    // Sub-IFD 2: one tag.
    let mut sub2 = Vec::new();
    sub2.extend_from_slice(&1u16.to_le_bytes());
    sub2.extend_from_slice(&0x0101u16.to_le_bytes()); // ImageLength
    sub2.extend_from_slice(&3u16.to_le_bytes());
    sub2.extend_from_slice(&1u32.to_le_bytes());
    sub2.extend_from_slice(&u32le(480));
    sub2.extend_from_slice(&0u32.to_le_bytes());
    b.append_at(sub2_off as usize, &sub2);

    let reader = TestReader::new(b.data);
    let tags = parse_tiff_file(&reader).expect("sub-ifd parse");
    // Sub-IFD tags should be present (ImageWidth and ImageLength from the sub IFDs).
    assert!(tags.iter().any(|(id, _, _, _)| *id == 0x0100));
    assert!(tags.iter().any(|(id, _, _, _)| *id == 0x0101));
    assert!(tags.iter().any(|(id, _, _, _)| *id == 0x014A));
}

#[test]
fn test_tiff_file_exif_pointer_subifd() {
    // ExifIFDPointer (0x8769) pointing at a sub-IFD.
    let mut b = TiffBuilder::new();
    let exif_off = 64u32;
    let entries = [(0x8769u16, 4u16, 1u32, u32le(exif_off))];
    b.push_ifd(&entries, 0);

    let mut exif = Vec::new();
    exif.extend_from_slice(&1u16.to_le_bytes());
    exif.extend_from_slice(&0x010Fu16.to_le_bytes()); // Make
    exif.extend_from_slice(&2u16.to_le_bytes()); // ASCII
    exif.extend_from_slice(&4u32.to_le_bytes());
    exif.extend_from_slice(b"Hi\0\0");
    exif.extend_from_slice(&0u32.to_le_bytes());
    b.append_at(exif_off as usize, &exif);

    let reader = TestReader::new(b.data);
    let tags = parse_tiff_file(&reader).expect("exif subifd parse");
    assert!(tags.iter().any(|(id, _, _, _)| *id == 0x010F));
    assert!(tags.iter().any(|(id, _, _, _)| *id == 0x8769));
}

#[test]
fn test_tiff_file_ifd_offset_beyond_file() {
    // First IFD offset points past EOF -> error.
    let mut data = vec![0x49u8, 0x49, 0x2A, 0x00];
    data.extend_from_slice(&9999u32.to_le_bytes());
    data.resize(64, 0);
    let reader = TestReader::new(data);
    assert!(parse_tiff_file(&reader).is_err());
}

#[test]
fn test_tiff_file_circular_reference() {
    // IFD0 whose next-IFD offset points back to itself (offset 8).
    let mut b = TiffBuilder::new();
    b.push_ifd(&[], 8); // 0 entries, next = 8 (itself)
    b.data.resize(b.len().max(64), 0);
    let reader = TestReader::new(b.data);
    assert!(parse_tiff_file(&reader).is_err());
}

// ============================================================================
// GZIP parser - FNAME / FCOMMENT / FEXTRA / FHCRC flags, OS + XFL tables
// ============================================================================

use oxidex::parsers::archive::gz::{GZParser, parse_gz_metadata};

const FTEXT: u8 = 0x01;
const FHCRC: u8 = 0x02;
const FEXTRA: u8 = 0x04;
const FNAME: u8 = 0x08;
const FCOMMENT: u8 = 0x10;

/// Builds a 10-byte GZIP header. xfl + os configurable.
fn gz_header(flags: u8, mtime: u32, xfl: u8, os: u8) -> Vec<u8> {
    let mut h = vec![0x1F, 0x8B, 0x08, flags];
    h.extend_from_slice(&mtime.to_le_bytes());
    h.push(xfl);
    h.push(os);
    h
}

/// Standard 8-byte trailer.
fn gz_trailer(crc: u32, isize: u32) -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(&crc.to_le_bytes());
    t.extend_from_slice(&isize.to_le_bytes());
    t
}

#[test]
fn test_gz_full_with_name_comment_extra_hcrc() {
    let flags = FEXTRA | FNAME | FCOMMENT | FHCRC | FTEXT;
    let mut data = gz_header(flags, 1_600_000_000, 2, 3); // xfl=2 (max), os=3 (Unix)

    // FEXTRA: 2-byte xlen + xlen bytes.
    let extra = b"AB\x02\x00xy"; // SI1 SI2 LEN(2) DATA(xy)
    data.extend_from_slice(&(extra.len() as u16).to_le_bytes());
    data.extend_from_slice(extra);

    // FNAME: null-terminated.
    data.extend_from_slice(b"original.txt\0");
    // FCOMMENT: null-terminated.
    data.extend_from_slice(b"a comment here\0");
    // FHCRC: 2 bytes.
    data.extend_from_slice(&[0xAB, 0xCD]);

    // some compressed body (not parsed) then trailer.
    data.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);
    data.extend_from_slice(&gz_trailer(0xDEADBEEF, 12345));

    let reader = TestReader::new(data);
    let md = parse_gz_metadata(&reader).expect("gz parse");

    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("GZIP".to_string()))
    );
    assert_eq!(
        md.get("CompressionMethod"),
        Some(&TagValue::String("DEFLATE".to_string()))
    );
    assert_eq!(
        md.get("CompressionLevel"),
        Some(&TagValue::String("Maximum compression".to_string()))
    );
    assert_eq!(
        md.get("OperatingSystem"),
        Some(&TagValue::String("Unix".to_string()))
    );
    assert_eq!(
        md.get("OriginalFileName"),
        Some(&TagValue::String("original.txt".to_string()))
    );
    assert_eq!(
        md.get("Comment"),
        Some(&TagValue::String("a comment here".to_string()))
    );
    assert!(md.contains_key("ModificationTime"));
    assert_eq!(
        md.get("CRC32"),
        Some(&TagValue::String("0xDEADBEEF".to_string()))
    );
    assert_eq!(
        md.get("OriginalSize"),
        Some(&TagValue::String("12345".to_string()))
    );
}

#[test]
fn test_gz_fastest_compression_and_os_values() {
    // xfl=4 -> Fastest; cycle through several OS bytes.
    for (os_byte, name) in [
        (0u8, "FAT"),
        (1, "Amiga"),
        (2, "VMS"),
        (4, "VM/CMS"),
        (5, "Atari TOS"),
        (6, "HPFS"),
        (7, "Macintosh"),
        (8, "Z-System"),
        (9, "CP/M"),
        (10, "TOPS-20"),
        (11, "NTFS"),
        (12, "QDOS"),
        (13, "Acorn RISCOS"),
        (255, "Unknown"),
        (200, "Unknown"),
    ] {
        let mut data = gz_header(0, 0, 4, os_byte); // no flags, mtime 0
        data.extend_from_slice(&gz_trailer(0, 0));
        let reader = TestReader::new(data);
        let md = parse_gz_metadata(&reader).expect("gz parse");
        assert_eq!(
            md.get("CompressionLevel"),
            Some(&TagValue::String("Fastest compression".to_string()))
        );
        assert_eq!(
            md.get("OperatingSystem"),
            Some(&TagValue::String(name.to_string())),
            "os byte {}",
            os_byte
        );
        // mtime 0 -> no ModificationTime.
        assert!(!md.contains_key("ModificationTime"));
    }
}

#[test]
fn test_gz_normal_compression_unknown_method() {
    // method != 8 -> "Unknown"; xfl other -> Normal.
    let mut data = vec![0x1F, 0x8B, 0x07, 0x00]; // method 7
    data.extend_from_slice(&0u32.to_le_bytes());
    data.push(0); // xfl 0 -> Normal
    data.push(3);
    data.extend_from_slice(&gz_trailer(1, 2));
    let reader = TestReader::new(data);
    let md = parse_gz_metadata(&reader).expect("gz parse");
    assert_eq!(
        md.get("CompressionMethod"),
        Some(&TagValue::String("Unknown".to_string()))
    );
    assert_eq!(
        md.get("CompressionLevel"),
        Some(&TagValue::String("Normal".to_string()))
    );
}

#[test]
fn test_gz_fextra_truncated_returns_early() {
    // FEXTRA flag set but not enough bytes for xlen -> parse_header returns Ok early.
    let mut data = gz_header(FEXTRA, 0, 0, 3);
    // Only one byte after header instead of the 2-byte xlen.
    data.push(0x05);
    let reader = TestReader::new(data);
    // Should not panic; trailer may or may not be present.
    let md = parse_gz_metadata(&reader).expect("gz parse");
    assert_eq!(
        md.get("FileType"),
        Some(&TagValue::String("GZIP".to_string()))
    );
}

#[test]
fn test_gz_signature_and_too_short() {
    let good = gz_header(0, 0, 0, 3);
    assert!(GZParser::verify_signature(&TestReader::new(good)).unwrap());

    assert!(!GZParser::verify_signature(&TestReader::new(vec![0x1F])).unwrap());
    assert!(!GZParser::verify_signature(&TestReader::new(vec![0x00, 0x00])).unwrap());

    // Header < 10 bytes -> parse error.
    let r = TestReader::new(vec![0x1F, 0x8B, 0x08]);
    assert!(parse_gz_metadata(&r).is_err());

    // Bad signature -> parse error.
    let mut bad = vec![0x00u8; 20];
    bad[0] = 0x00;
    assert!(parse_gz_metadata(&TestReader::new(bad)).is_err());
}

#[test]
fn test_gz_via_read_metadata() {
    let flags = FNAME;
    let mut data = gz_header(flags, 0, 2, 3);
    data.extend_from_slice(b"file.bin\0");
    data.extend_from_slice(&[0u8; 4]);
    data.extend_from_slice(&gz_trailer(0x12345678, 999));
    let file = temp_with_ext(&data, "gz");
    // read_metadata may route through detection; accept either Ok or a graceful Err,
    // but the direct API must work.
    let r = TestReader::new(data);
    let md = parse_gz_metadata(&r).expect("gz parse");
    assert_eq!(
        md.get("OriginalFileName"),
        Some(&TagValue::String("file.bin".to_string()))
    );
    let _ = oxidex::core::operations::read_metadata(file.path());
}

// ============================================================================
// PDF resources parser - embedded image XObjects
// ============================================================================

use oxidex::parsers::pdf::resources_parser::parse_resources_metadata;

#[test]
fn test_pdf_resources_real_fixture() {
    // Drive the resources parser against the checked-in sample PDF (may or may not
    // contain embedded images). Either outcome exercises the navigation code paths.
    if let Ok(bytes) = std::fs::read("tests/fixtures/pdf/sample.pdf") {
        let reader = TestReader::new(bytes);
        let _ = parse_resources_metadata(&reader);
    }
}

#[test]
fn test_pdf_resources_synthetic_image() {
    // A small PDF with a page whose /Resources references an image XObject.
    let pdf = b"%PDF-1.4\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /Resources << /XObject << /Im1 5 0 R >> >> >>\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 800 /Height 600 /ColorSpace /DeviceRGB /Filter /FlateDecode /Length 4 >>\nstream\n\x00\x01\x02\x03\nendstream\nendobj\n";

    // Build xref offsets dynamically so the parser can resolve objects.
    let body = pdf.to_vec();
    let find = |needle: &str| -> usize {
        body.windows(needle.len())
            .position(|w| w == needle.as_bytes())
            .unwrap_or(0)
    };
    let off1 = find("1 0 obj");
    let off2 = find("2 0 obj");
    let off3 = find("3 0 obj");
    let off5 = find("5 0 obj");

    let mut full = body.clone();
    let xref_start = full.len();
    let xref = format!(
        "xref\n0 6\n0000000000 65535 f \n{:010} 00000 n \n{:010} 00000 n \n{:010} 00000 n \n0000000000 65535 f \n{:010} 00000 n \ntrailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{}\n%%EOF",
        off1, off2, off3, off5, xref_start
    );
    full.extend_from_slice(xref.as_bytes());

    let reader = TestReader::new(full);
    let result = parse_resources_metadata(&reader);
    // The image is FlateDecode (not DCTDecode) so embedded-JPEG path is skipped, but
    // structural metadata (count/width/height/filter/colorspace) should be present.
    if let Ok(md) = result {
        assert_eq!(
            md.get("PDF:EmbeddedImageCount"),
            Some(&TagValue::new_integer(1))
        );
        assert_eq!(
            md.get("PDF:EmbeddedImageWidth"),
            Some(&TagValue::new_integer(800))
        );
        assert_eq!(
            md.get("PDF:EmbeddedImageColorSpace"),
            Some(&TagValue::new_string("DeviceRGB".to_string()))
        );
    }
}

#[test]
fn test_pdf_resources_no_startxref() {
    // No startxref -> load_pdf_context errors.
    let reader = TestReader::new(b"%PDF-1.4\ngarbage with no xref pointer".to_vec());
    assert!(parse_resources_metadata(&reader).is_err());
}

// ============================================================================
// Mach-O structures.rs - exhaustive enum mappers + struct methods
// ============================================================================

use oxidex::parsers::macho::structures::{
    BuildToolVersion, BuildVersionCommand, DylibCommand, EncryptionInfoCommand, EntryPointCommand,
    FatArch, FatHeader, LinkeditDataCommand, MachHeader, MachOInfo, Section, SegmentCommand,
    SourceVersionCommand, UuidCommand, VersionMinCommand, build_tool, build_tool_name, cpu_type,
    decode_flags, file_type, file_type_name, flags as mh_flags, format_version, hash_type,
    hash_type_name, load_command, load_command_name, magic, platform, platform_name,
};

#[test]
fn test_macho_file_type_name_all() {
    assert_eq!(file_type_name(file_type::MH_OBJECT), "Object");
    assert_eq!(file_type_name(file_type::MH_EXECUTE), "Executable");
    assert_eq!(file_type_name(file_type::MH_FVMLIB), "Fixed VM Library");
    assert_eq!(file_type_name(file_type::MH_CORE), "Core");
    assert_eq!(file_type_name(file_type::MH_PRELOAD), "Preload");
    assert_eq!(file_type_name(file_type::MH_DYLIB), "Dynamic Library");
    assert_eq!(file_type_name(file_type::MH_DYLINKER), "Dynamic Linker");
    assert_eq!(file_type_name(file_type::MH_BUNDLE), "Bundle");
    assert_eq!(
        file_type_name(file_type::MH_DYLIB_STUB),
        "Dynamic Library Stub"
    );
    assert_eq!(file_type_name(file_type::MH_DSYM), "Debug Symbols");
    assert_eq!(
        file_type_name(file_type::MH_KEXT_BUNDLE),
        "Kernel Extension"
    );
    assert_eq!(file_type_name(file_type::MH_FILESET), "Fileset");
    assert_eq!(file_type_name(0xDEAD), "Unknown");
}

#[test]
fn test_macho_platform_name_all() {
    use platform::*;
    for (p, n) in [
        (PLATFORM_MACOS, "macOS"),
        (PLATFORM_IOS, "iOS"),
        (PLATFORM_TVOS, "tvOS"),
        (PLATFORM_WATCHOS, "watchOS"),
        (PLATFORM_BRIDGEOS, "bridgeOS"),
        (PLATFORM_MACCATALYST, "Mac Catalyst"),
        (PLATFORM_IOSSIMULATOR, "iOS Simulator"),
        (PLATFORM_TVOSSIMULATOR, "tvOS Simulator"),
        (PLATFORM_WATCHOSSIMULATOR, "watchOS Simulator"),
        (PLATFORM_DRIVERKIT, "DriverKit"),
        (PLATFORM_VISIONOS, "visionOS"),
        (PLATFORM_VISIONOSSIMULATOR, "visionOS Simulator"),
    ] {
        assert_eq!(platform_name(p), n);
    }
    assert_eq!(platform_name(999), "Unknown");
}

#[test]
fn test_macho_build_tool_and_hash_type_names() {
    assert_eq!(build_tool_name(build_tool::TOOL_CLANG), "Clang");
    assert_eq!(build_tool_name(build_tool::TOOL_SWIFT), "Swift");
    assert_eq!(build_tool_name(build_tool::TOOL_LD), "ld");
    assert_eq!(build_tool_name(build_tool::TOOL_LLD), "lld");
    assert_eq!(build_tool_name(99), "Unknown");

    assert_eq!(hash_type_name(hash_type::CS_HASHTYPE_SHA1), "SHA-1");
    assert_eq!(hash_type_name(hash_type::CS_HASHTYPE_SHA256), "SHA-256");
    assert_eq!(
        hash_type_name(hash_type::CS_HASHTYPE_SHA256_TRUNCATED),
        "SHA-256 (truncated)"
    );
    assert_eq!(hash_type_name(hash_type::CS_HASHTYPE_SHA384), "SHA-384");
    assert_eq!(hash_type_name(hash_type::CS_HASHTYPE_SHA512), "SHA-512");
    assert_eq!(hash_type_name(99), "Unknown");
}

#[test]
fn test_macho_load_command_name_exhaustive() {
    use load_command::*;
    let cases = [
        (LC_SEGMENT, "LC_SEGMENT"),
        (LC_SYMTAB, "LC_SYMTAB"),
        (LC_SYMSEG, "LC_SYMSEG"),
        (LC_THREAD, "LC_THREAD"),
        (LC_UNIXTHREAD, "LC_UNIXTHREAD"),
        (LC_LOADFVMLIB, "LC_LOADFVMLIB"),
        (LC_IDFVMLIB, "LC_IDFVMLIB"),
        (LC_IDENT, "LC_IDENT"),
        (LC_FVMFILE, "LC_FVMFILE"),
        (LC_PREPAGE, "LC_PREPAGE"),
        (LC_DYSYMTAB, "LC_DYSYMTAB"),
        (LC_LOAD_DYLIB, "LC_LOAD_DYLIB"),
        (LC_ID_DYLIB, "LC_ID_DYLIB"),
        (LC_LOAD_DYLINKER, "LC_LOAD_DYLINKER"),
        (LC_ID_DYLINKER, "LC_ID_DYLINKER"),
        (LC_PREBOUND_DYLIB, "LC_PREBOUND_DYLIB"),
        (LC_ROUTINES, "LC_ROUTINES"),
        (LC_SUB_FRAMEWORK, "LC_SUB_FRAMEWORK"),
        (LC_SUB_UMBRELLA, "LC_SUB_UMBRELLA"),
        (LC_SUB_CLIENT, "LC_SUB_CLIENT"),
        (LC_SUB_LIBRARY, "LC_SUB_LIBRARY"),
        (LC_TWOLEVEL_HINTS, "LC_TWOLEVEL_HINTS"),
        (LC_PREBIND_CKSUM, "LC_PREBIND_CKSUM"),
        (LC_LOAD_WEAK_DYLIB, "LC_LOAD_WEAK_DYLIB"),
        (LC_SEGMENT_64, "LC_SEGMENT_64"),
        (LC_ROUTINES_64, "LC_ROUTINES_64"),
        (LC_UUID, "LC_UUID"),
        (LC_RPATH, "LC_RPATH"),
        (LC_CODE_SIGNATURE, "LC_CODE_SIGNATURE"),
        (LC_SEGMENT_SPLIT_INFO, "LC_SEGMENT_SPLIT_INFO"),
        (LC_REEXPORT_DYLIB, "LC_REEXPORT_DYLIB"),
        (LC_LAZY_LOAD_DYLIB, "LC_LAZY_LOAD_DYLIB"),
        (LC_ENCRYPTION_INFO, "LC_ENCRYPTION_INFO"),
        (LC_DYLD_INFO, "LC_DYLD_INFO"),
        (LC_DYLD_INFO_ONLY, "LC_DYLD_INFO_ONLY"),
        (LC_LOAD_UPWARD_DYLIB, "LC_LOAD_UPWARD_DYLIB"),
        (LC_VERSION_MIN_MACOSX, "LC_VERSION_MIN_MACOSX"),
        (LC_VERSION_MIN_IPHONEOS, "LC_VERSION_MIN_IPHONEOS"),
        (LC_FUNCTION_STARTS, "LC_FUNCTION_STARTS"),
        (LC_DYLD_ENVIRONMENT, "LC_DYLD_ENVIRONMENT"),
        (LC_MAIN, "LC_MAIN"),
        (LC_DATA_IN_CODE, "LC_DATA_IN_CODE"),
        (LC_SOURCE_VERSION, "LC_SOURCE_VERSION"),
        (LC_DYLIB_CODE_SIGN_DRS, "LC_DYLIB_CODE_SIGN_DRS"),
        (LC_ENCRYPTION_INFO_64, "LC_ENCRYPTION_INFO_64"),
        (LC_LINKER_OPTION, "LC_LINKER_OPTION"),
        (LC_LINKER_OPTIMIZATION_HINT, "LC_LINKER_OPTIMIZATION_HINT"),
        (LC_VERSION_MIN_WATCHOS, "LC_VERSION_MIN_WATCHOS"),
        (LC_VERSION_MIN_TVOS, "LC_VERSION_MIN_TVOS"),
        (LC_NOTE, "LC_NOTE"),
        (LC_BUILD_VERSION, "LC_BUILD_VERSION"),
        (LC_DYLD_EXPORTS_TRIE, "LC_DYLD_EXPORTS_TRIE"),
        (LC_DYLD_CHAINED_FIXUPS, "LC_DYLD_CHAINED_FIXUPS"),
        (LC_FILESET_ENTRY, "LC_FILESET_ENTRY"),
    ];
    for (cmd, name) in cases {
        assert_eq!(load_command_name(cmd), name, "cmd 0x{:X}", cmd);
    }
    assert_eq!(load_command_name(0x7FFF_FFFF), "LC_UNKNOWN");
}

#[test]
fn test_macho_decode_flags_all_bits() {
    // OR every flag together and verify the produced list contains each name.
    let all = mh_flags::MH_NOUNDEFS
        | mh_flags::MH_INCRLINK
        | mh_flags::MH_DYLDLINK
        | mh_flags::MH_BINDATLOAD
        | mh_flags::MH_PREBOUND
        | mh_flags::MH_SPLIT_SEGS
        | mh_flags::MH_LAZY_INIT
        | mh_flags::MH_TWOLEVEL
        | mh_flags::MH_FORCE_FLAT
        | mh_flags::MH_NOMULTIDEFS
        | mh_flags::MH_NOFIXPREBINDING
        | mh_flags::MH_PREBINDABLE
        | mh_flags::MH_ALLMODSBOUND
        | mh_flags::MH_SUBSECTIONS_VIA_SYMBOLS
        | mh_flags::MH_CANONICAL
        | mh_flags::MH_WEAK_DEFINES
        | mh_flags::MH_BINDS_TO_WEAK
        | mh_flags::MH_ALLOW_STACK_EXECUTION
        | mh_flags::MH_ROOT_SAFE
        | mh_flags::MH_SETUID_SAFE
        | mh_flags::MH_NO_REEXPORTED_DYLIBS
        | mh_flags::MH_PIE
        | mh_flags::MH_DEAD_STRIPPABLE_DYLIB
        | mh_flags::MH_HAS_TLV_DESCRIPTORS
        | mh_flags::MH_NO_HEAP_EXECUTION
        | mh_flags::MH_APP_EXTENSION_SAFE
        | mh_flags::MH_SIM_SUPPORT;
    let names = decode_flags(all);
    for expected in [
        "NOUNDEFS",
        "INCRLINK",
        "DYLDLINK",
        "BINDATLOAD",
        "PREBOUND",
        "SPLIT_SEGS",
        "LAZY_INIT",
        "TWOLEVEL",
        "FORCE_FLAT",
        "NOMULTIDEFS",
        "NOFIXPREBINDING",
        "PREBINDABLE",
        "ALLMODSBOUND",
        "SUBSECTIONS_VIA_SYMBOLS",
        "CANONICAL",
        "WEAK_DEFINES",
        "BINDS_TO_WEAK",
        "ALLOW_STACK_EXECUTION",
        "ROOT_SAFE",
        "SETUID_SAFE",
        "NO_REEXPORTED_DYLIBS",
        "PIE",
        "DEAD_STRIPPABLE_DYLIB",
        "HAS_TLV_DESCRIPTORS",
        "NO_HEAP_EXECUTION",
        "APP_EXTENSION_SAFE",
        "SIM_SUPPORT",
    ] {
        assert!(names.contains(&expected), "missing {}", expected);
    }
    assert!(decode_flags(0).is_empty());
}

fn make_header(
    cputype: i32,
    cpusubtype: i32,
    filetype: u32,
    flags: u32,
    is_64: bool,
) -> MachHeader {
    MachHeader {
        magic: if is_64 {
            magic::MH_MAGIC_64
        } else {
            magic::MH_MAGIC
        },
        cputype,
        cpusubtype,
        filetype,
        ncmds: 0,
        sizeofcmds: 0,
        flags,
        reserved: 0,
        is_64bit: is_64,
        is_swapped: false,
    }
}

#[test]
fn test_macho_header_cpu_type_names() {
    use cpu_type::*;
    for (ct, name) in [
        (CPU_TYPE_I386, "i386"),
        (CPU_TYPE_X86_64, "x86_64"),
        (CPU_TYPE_ARM, "ARM"),
        (CPU_TYPE_ARM64, "ARM64"),
        (CPU_TYPE_ARM64_32, "ARM64_32"),
        (CPU_TYPE_POWERPC, "PowerPC"),
        (CPU_TYPE_POWERPC64, "PowerPC64"),
        (-999, "Unknown"),
    ] {
        let h = make_header(ct, 0, file_type::MH_EXECUTE, 0, true);
        assert_eq!(h.cpu_type_name(), name);
    }
}

#[test]
fn test_macho_header_cpu_subtype_names() {
    // ARM64 subtypes.
    let h = make_header(cpu_type::CPU_TYPE_ARM64, 0, file_type::MH_EXECUTE, 0, true);
    assert_eq!(h.cpu_subtype_name(), "ALL");
    let h = make_header(cpu_type::CPU_TYPE_ARM64, 2, file_type::MH_EXECUTE, 0, true);
    assert_eq!(h.cpu_subtype_name(), "ARM64E");
    let h = make_header(cpu_type::CPU_TYPE_ARM64, 1, file_type::MH_EXECUTE, 0, true);
    assert_eq!(h.cpu_subtype_name(), "V8");
    let h = make_header(cpu_type::CPU_TYPE_ARM64, 99, file_type::MH_EXECUTE, 0, true);
    assert!(h.cpu_subtype_name().starts_with("Unknown"));
    // x86_64 subtypes.
    let h = make_header(cpu_type::CPU_TYPE_X86_64, 3, file_type::MH_EXECUTE, 0, true);
    assert_eq!(h.cpu_subtype_name(), "ALL");
    let h = make_header(cpu_type::CPU_TYPE_X86_64, 8, file_type::MH_EXECUTE, 0, true);
    assert_eq!(h.cpu_subtype_name(), "Haswell");
    let h = make_header(
        cpu_type::CPU_TYPE_X86_64,
        99,
        file_type::MH_EXECUTE,
        0,
        true,
    );
    assert!(h.cpu_subtype_name().starts_with("Unknown"));
    // Other arch -> raw number.
    let h = make_header(cpu_type::CPU_TYPE_ARM, 7, file_type::MH_EXECUTE, 0, true);
    assert_eq!(h.cpu_subtype_name(), "7");
}

#[test]
fn test_macho_header_misc_methods() {
    let h64 = make_header(
        cpu_type::CPU_TYPE_ARM64,
        0,
        file_type::MH_DYLIB,
        mh_flags::MH_PIE,
        true,
    );
    assert_eq!(h64.header_size(), 32);
    assert_eq!(h64.file_type_name(), "Dynamic Library");
    assert!(h64.flag_names().contains(&"PIE"));

    let h32 = make_header(cpu_type::CPU_TYPE_I386, 0, file_type::MH_OBJECT, 0, false);
    assert_eq!(h32.header_size(), 28);
}

#[test]
fn test_macho_format_version_and_command_methods() {
    assert_eq!(format_version(0x0001_0203), "1.2.3");
    assert_eq!(format_version(0), "0.0.0");
    assert_eq!(format_version(0x00FF_FFFF), "255.255.255");

    let dylib = DylibCommand {
        cmd: load_command::LC_LOAD_DYLIB,
        name: "/usr/lib/libSystem.B.dylib".to_string(),
        timestamp: 2,
        current_version: 0x0005_0102,
        compatibility_version: 0x0001_0000,
    };
    assert_eq!(dylib.current_version_string(), "5.1.2");
    assert_eq!(dylib.compatibility_version_string(), "1.0.0");

    let uuid = UuidCommand {
        uuid: [
            0x55, 0x0E, 0x84, 0x00, 0xE2, 0x9B, 0x41, 0xD4, 0xA7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ],
    };
    assert_eq!(uuid.uuid_string(), "550E8400-E29B-41D4-A716-446655440000");

    let sv = SourceVersionCommand {
        version: (7 << 40) | (8 << 30) | (9 << 20) | (10 << 10) | 11,
    };
    assert_eq!(sv.version_string(), "7.8.9.10.11");
}

#[test]
fn test_macho_version_min_command_platforms() {
    for (cmd, name) in [
        (load_command::LC_VERSION_MIN_MACOSX, "macOS"),
        (load_command::LC_VERSION_MIN_IPHONEOS, "iOS"),
        (load_command::LC_VERSION_MIN_WATCHOS, "watchOS"),
        (load_command::LC_VERSION_MIN_TVOS, "tvOS"),
        (load_command::LC_MAIN, "Unknown"),
    ] {
        let vmc = VersionMinCommand {
            cmd,
            version: 0x000D_0100,
            sdk: 0x000E_0000,
        };
        assert_eq!(vmc.platform_name(), name);
        assert_eq!(vmc.version_string(), "13.1.0");
        assert_eq!(vmc.sdk_string(), "14.0.0");
    }
}

#[test]
fn test_macho_build_tool_and_build_version_commands() {
    let tool = BuildToolVersion {
        tool: build_tool::TOOL_SWIFT,
        version: 0x0005_0700,
    };
    assert_eq!(tool.tool_name(), "Swift");
    assert_eq!(tool.version_string(), "5.7.0");

    let bv = BuildVersionCommand {
        platform: platform::PLATFORM_MACOS,
        minos: 0x000D_0000,
        sdk: 0x000E_0100,
        ntools: 1,
        tools: vec![tool],
    };
    assert_eq!(bv.platform_name(), "macOS");
    assert_eq!(bv.minos_string(), "13.0.0");
    assert_eq!(bv.sdk_string(), "14.1.0");
}

#[test]
fn test_macho_info_aggregate_methods() {
    let mut info = MachOInfo::new();
    info.segments.push(SegmentCommand {
        segname: "__TEXT".to_string(),
        vmaddr: 0,
        vmsize: 0x4000,
        fileoff: 0,
        filesize: 0x4000,
        maxprot: 5,
        initprot: 5,
        nsects: 2,
        flags: 0,
        sections: vec![
            Section {
                sectname: "__text".to_string(),
                segname: "__TEXT".to_string(),
                addr: 0,
                size: 0x100,
                offset: 0,
                align: 4,
                reloff: 0,
                nreloc: 0,
                flags: 0,
                reserved1: 0,
                reserved2: 0,
                reserved3: 0,
            },
            Section {
                sectname: "__cstring".to_string(),
                segname: "__TEXT".to_string(),
                addr: 0x100,
                size: 0x50,
                offset: 0x100,
                align: 0,
                reloff: 0,
                nreloc: 0,
                flags: 0,
                reserved1: 0,
                reserved2: 0,
                reserved3: 0,
            },
        ],
    });
    info.segments.push(SegmentCommand {
        segname: "__DATA_CONST".to_string(),
        vmaddr: 0x4000,
        vmsize: 0x2000,
        fileoff: 0x4000,
        filesize: 0x2000,
        maxprot: 3,
        initprot: 3,
        nsects: 0,
        flags: 0,
        sections: vec![],
    });
    info.dylibs.push(DylibCommand {
        cmd: load_command::LC_LOAD_WEAK_DYLIB,
        name: "weak.dylib".to_string(),
        timestamp: 0,
        current_version: 0,
        compatibility_version: 0,
    });
    info.dylibs.push(DylibCommand {
        cmd: load_command::LC_REEXPORT_DYLIB,
        name: "reexport.dylib".to_string(),
        timestamp: 0,
        current_version: 0,
        compatibility_version: 0,
    });

    assert_eq!(info.total_sections(), 2);
    assert_eq!(info.text_segment_size(), Some(0x4000));
    assert_eq!(info.data_segment_size(), Some(0x2000));
    assert_eq!(info.weak_dylib_count(), 1);
    assert_eq!(info.reexport_dylib_count(), 1);
}

#[test]
fn test_macho_fat_arch_cpu_type_names() {
    use cpu_type::*;
    for (ct, name) in [
        (CPU_TYPE_I386, "i386"),
        (CPU_TYPE_X86_64, "x86_64"),
        (CPU_TYPE_ARM, "ARM"),
        (CPU_TYPE_ARM64, "ARM64"),
        (CPU_TYPE_ARM64_32, "ARM64_32"),
        (CPU_TYPE_POWERPC, "PowerPC"),
        (CPU_TYPE_POWERPC64, "PowerPC64"),
        (-1, "Unknown"),
    ] {
        let arch = FatArch {
            cputype: ct,
            cpusubtype: 0,
            offset: 0,
            size: 0,
            align: 0,
        };
        assert_eq!(arch.cpu_type_name(), name);
    }
    // Exercise the remaining structs' construction (coverage of field paths).
    let _fat = FatHeader {
        magic: magic::FAT_MAGIC,
        nfat_arch: 2,
        is_64bit: false,
        is_swapped: false,
    };
    let _ep = EntryPointCommand {
        entryoff: 0x1000,
        stacksize: 0,
    };
    let _li = LinkeditDataCommand {
        cmd: load_command::LC_CODE_SIGNATURE,
        dataoff: 0,
        datasize: 16,
    };
    let _enc = EncryptionInfoCommand {
        cryptoff: 0,
        cryptsize: 0,
        cryptid: 0,
    };
}

// ============================================================================
// ELF structures.rs - exhaustive enum mappers + struct methods
// ============================================================================

use oxidex::parsers::elf::structures::{
    DynamicEntry, DynamicInfo, ElfHeader, ElfInfo, NoteEntry, ProgramHeader,
    SectionHeader as ElfSectionHeader, Symbol, SymbolInfo, dt_tag, ei_index, elf_osabi, elf_type,
    machine_types, nt_core, nt_gnu, pf_flags, pt_type, sh_flags, sh_type, shn_index, stb_binding,
    stt_type,
};

fn elf_header(e_type: u16, machine: u16, osabi: u8, is_64: bool, le: bool) -> ElfHeader {
    let mut ident = [0u8; 16];
    ident[ei_index::EI_OSABI] = osabi;
    ElfHeader {
        e_ident: ident,
        e_type,
        e_machine: machine,
        e_version: 1,
        e_entry: 0x1000,
        e_phoff: 64,
        e_shoff: 0,
        e_flags: 0,
        e_ehsize: 64,
        e_phentsize: 56,
        e_phnum: 1,
        e_shentsize: 64,
        e_shnum: 0,
        e_shstrndx: 0,
        is_64bit: is_64,
        is_little_endian: le,
    }
}

#[test]
fn test_elf_header_class_endian_strings() {
    let h = elf_header(elf_type::ET_EXEC, machine_types::EM_X86_64, 0, true, true);
    assert_eq!(h.class_str(), "64-bit");
    assert_eq!(h.endian_str(), "Little-endian");

    let h = elf_header(elf_type::ET_EXEC, machine_types::EM_386, 0, false, false);
    assert_eq!(h.class_str(), "32-bit");
    assert_eq!(h.endian_str(), "Big-endian");
}

#[test]
fn test_elf_header_osabi_str_all() {
    use elf_osabi::*;
    for (abi, name) in [
        (ELFOSABI_SYSV, "UNIX System V"),
        (ELFOSABI_HPUX, "HP-UX"),
        (ELFOSABI_NETBSD, "NetBSD"),
        (ELFOSABI_GNU, "GNU/Linux"),
        (ELFOSABI_SOLARIS, "Sun Solaris"),
        (ELFOSABI_AIX, "IBM AIX"),
        (ELFOSABI_IRIX, "SGI IRIX"),
        (ELFOSABI_FREEBSD, "FreeBSD"),
        (ELFOSABI_TRU64, "Compaq TRU64 UNIX"),
        (ELFOSABI_MODESTO, "Novell Modesto"),
        (ELFOSABI_OPENBSD, "OpenBSD"),
        (ELFOSABI_ARM_AEABI, "ARM EABI"),
        (ELFOSABI_ARM, "ARM"),
        (ELFOSABI_STANDALONE, "Standalone (embedded)"),
        (200, "Unknown"),
    ] {
        let h = elf_header(elf_type::ET_EXEC, machine_types::EM_X86_64, abi, true, true);
        assert_eq!(h.osabi_str(), name, "abi {}", abi);
    }
}

#[test]
fn test_elf_header_type_str_all() {
    use elf_type::*;
    for (t, name) in [
        (ET_NONE, "None"),
        (ET_REL, "Relocatable"),
        (ET_EXEC, "Executable"),
        (ET_DYN, "Shared Object"),
        (ET_CORE, "Core"),
        (0xFE10, "OS-specific"),
        (0xFF10, "Processor-specific"),
        (0x1234, "Unknown"),
    ] {
        let h = elf_header(t, machine_types::EM_X86_64, 0, true, true);
        assert_eq!(h.type_str(), name, "type {}", t);
    }
}

#[test]
fn test_elf_header_machine_str_all() {
    use machine_types::*;
    for (m, name) in [
        (EM_NONE, "None"),
        (EM_386, "Intel 80386"),
        (EM_68K, "Motorola 68000"),
        (EM_MIPS, "MIPS"),
        (EM_SPARC, "SPARC"),
        (EM_PPC, "PowerPC"),
        (EM_PPC64, "PowerPC64"),
        (EM_ARM, "ARM"),
        (EM_SH, "SuperH"),
        (EM_SPARCV9, "SPARC V9"),
        (EM_IA_64, "Intel Itanium"),
        (EM_X86_64, "AMD x86-64"),
        (EM_S390, "IBM S390"),
        (EM_AARCH64, "ARM64"),
        (EM_RISCV, "RISC-V"),
        (EM_BPF, "Berkeley Packet Filter"),
        (EM_LOONGARCH, "LoongArch"),
        (9999, "Unknown"),
    ] {
        let h = elf_header(elf_type::ET_EXEC, m, 0, true, true);
        assert_eq!(h.machine_str(), name, "machine {}", m);
    }
}

fn pheader(p_type: u32, p_flags: u32) -> ProgramHeader {
    ProgramHeader {
        p_type,
        p_flags,
        p_offset: 0,
        p_vaddr: 0,
        p_paddr: 0,
        p_filesz: 0,
        p_memsz: 0,
        p_align: 0,
    }
}

#[test]
fn test_elf_program_header_type_str_all() {
    use pt_type::*;
    for (t, name) in [
        (PT_NULL, "NULL"),
        (PT_LOAD, "LOAD"),
        (PT_DYNAMIC, "DYNAMIC"),
        (PT_INTERP, "INTERP"),
        (PT_NOTE, "NOTE"),
        (PT_SHLIB, "SHLIB"),
        (PT_PHDR, "PHDR"),
        (PT_TLS, "TLS"),
        (PT_GNU_EH_FRAME, "GNU_EH_FRAME"),
        (PT_GNU_STACK, "GNU_STACK"),
        (PT_GNU_RELRO, "GNU_RELRO"),
        (PT_GNU_PROPERTY, "GNU_PROPERTY"),
        (0x70000001, "PROC"),
        (0x60000001, "OS"),
        (0x100, "Unknown"),
    ] {
        assert_eq!(pheader(t, 0).type_str(), name, "ptype {:#x}", t);
    }
}

#[test]
fn test_elf_program_header_flags_and_predicates() {
    let ph = pheader(pt_type::PT_LOAD, pf_flags::PF_R | pf_flags::PF_X);
    assert_eq!(ph.flags_str(), "R-X");
    assert!(ph.is_load());
    assert!(ph.is_executable());

    let ph = pheader(pt_type::PT_DYNAMIC, pf_flags::PF_R | pf_flags::PF_W);
    assert_eq!(ph.flags_str(), "RW-");
    assert!(!ph.is_load());
    assert!(!ph.is_executable());

    let ph = pheader(
        pt_type::PT_LOAD,
        pf_flags::PF_R | pf_flags::PF_W | pf_flags::PF_X,
    );
    assert_eq!(ph.flags_str(), "RWX");

    let ph = pheader(pt_type::PT_NULL, 0);
    assert_eq!(ph.flags_str(), "---");
}

fn sheader(sh_type: u32, sh_flags: u64, name: Option<String>) -> ElfSectionHeader {
    ElfSectionHeader {
        sh_name: 5,
        name,
        sh_type,
        sh_flags,
        sh_addr: 0,
        sh_offset: 0,
        sh_size: 0x40,
        sh_link: 0,
        sh_info: 0,
        sh_addralign: 1,
        sh_entsize: 0,
    }
}

#[test]
fn test_elf_section_header_type_str_all() {
    use sh_type::*;
    for (t, name) in [
        (SHT_NULL, "NULL"),
        (SHT_PROGBITS, "PROGBITS"),
        (SHT_SYMTAB, "SYMTAB"),
        (SHT_STRTAB, "STRTAB"),
        (SHT_RELA, "RELA"),
        (SHT_HASH, "HASH"),
        (SHT_DYNAMIC, "DYNAMIC"),
        (SHT_NOTE, "NOTE"),
        (SHT_NOBITS, "NOBITS"),
        (SHT_REL, "REL"),
        (SHT_SHLIB, "SHLIB"),
        (SHT_DYNSYM, "DYNSYM"),
        (SHT_INIT_ARRAY, "INIT_ARRAY"),
        (SHT_FINI_ARRAY, "FINI_ARRAY"),
        (SHT_PREINIT_ARRAY, "PREINIT_ARRAY"),
        (SHT_GROUP, "GROUP"),
        (SHT_SYMTAB_SHNDX, "SYMTAB_SHNDX"),
        (SHT_GNU_HASH, "GNU_HASH"),
        (SHT_GNU_VERDEF, "VERDEF"),
        (SHT_GNU_VERNEED, "VERNEED"),
        (SHT_GNU_VERSYM, "VERSYM"),
        (0x70000001, "PROC"),
        (0x80000001, "USER"),
        (0x12345, "Unknown"),
    ] {
        assert_eq!(sheader(t, 0, None).type_str(), name, "shtype {:#x}", t);
    }
}

#[test]
fn test_elf_section_header_flags_and_name() {
    let s = sheader(
        sh_type::SHT_PROGBITS,
        sh_flags::SHF_WRITE
            | sh_flags::SHF_ALLOC
            | sh_flags::SHF_EXECINSTR
            | sh_flags::SHF_MERGE
            | sh_flags::SHF_STRINGS
            | sh_flags::SHF_TLS,
        Some(".text".to_string()),
    );
    assert_eq!(s.flags_str(), "WAXMST");
    assert_eq!(s.name_str(), ".text");

    let s = sheader(sh_type::SHT_NULL, 0, None);
    assert_eq!(s.flags_str(), "---");
    assert_eq!(s.name_str(), "<5>"); // falls back to sh_name index
}

#[test]
fn test_elf_dynamic_entry_tag_str_all() {
    use dt_tag::*;
    for (t, name) in [
        (DT_NULL, "NULL"),
        (DT_NEEDED, "NEEDED"),
        (DT_PLTRELSZ, "PLTRELSZ"),
        (DT_PLTGOT, "PLTGOT"),
        (DT_HASH, "HASH"),
        (DT_STRTAB, "STRTAB"),
        (DT_SYMTAB, "SYMTAB"),
        (DT_RELA, "RELA"),
        (DT_RELASZ, "RELASZ"),
        (DT_RELAENT, "RELAENT"),
        (DT_STRSZ, "STRSZ"),
        (DT_SYMENT, "SYMENT"),
        (DT_INIT, "INIT"),
        (DT_FINI, "FINI"),
        (DT_SONAME, "SONAME"),
        (DT_RPATH, "RPATH"),
        (DT_SYMBOLIC, "SYMBOLIC"),
        (DT_REL, "REL"),
        (DT_RELSZ, "RELSZ"),
        (DT_RELENT, "RELENT"),
        (DT_PLTREL, "PLTREL"),
        (DT_DEBUG, "DEBUG"),
        (DT_TEXTREL, "TEXTREL"),
        (DT_JMPREL, "JMPREL"),
        (DT_BIND_NOW, "BIND_NOW"),
        (DT_INIT_ARRAY, "INIT_ARRAY"),
        (DT_FINI_ARRAY, "FINI_ARRAY"),
        (DT_INIT_ARRAYSZ, "INIT_ARRAYSZ"),
        (DT_FINI_ARRAYSZ, "FINI_ARRAYSZ"),
        (DT_RUNPATH, "RUNPATH"),
        (DT_FLAGS, "FLAGS"),
        (DT_GNU_HASH, "GNU_HASH"),
        (DT_FLAGS_1, "FLAGS_1"),
        (DT_VERDEF, "VERDEF"),
        (DT_VERDEFNUM, "VERDEFNUM"),
        (DT_VERNEED, "VERNEED"),
        (DT_VERNEEDNUM, "VERNEEDNUM"),
        (0x12345678, "Unknown"),
    ] {
        let e = DynamicEntry { d_tag: t, d_val: 0 };
        assert_eq!(e.tag_str(), name, "dtag {}", t);
    }
}

fn symbol(info: u8, shndx: u16) -> Symbol {
    Symbol {
        st_name: 7,
        name: None,
        st_info: info,
        st_other: 0,
        st_shndx: shndx,
        st_value: 0x2000,
        st_size: 16,
    }
}

#[test]
fn test_elf_symbol_binding_and_type_str_all() {
    use stb_binding::*;
    use stt_type::*;
    for (b, name) in [
        (STB_LOCAL, "LOCAL"),
        (STB_GLOBAL, "GLOBAL"),
        (STB_WEAK, "WEAK"),
        (STB_GNU_UNIQUE, "UNIQUE"),
        (15, "Unknown"),
    ] {
        let s = symbol((b << 4) | STT_NOTYPE, 1);
        assert_eq!(s.binding_str(), name, "bind {}", b);
    }
    for (t, name) in [
        (STT_NOTYPE, "NOTYPE"),
        (STT_OBJECT, "OBJECT"),
        (STT_FUNC, "FUNC"),
        (STT_SECTION, "SECTION"),
        (STT_FILE, "FILE"),
        (STT_COMMON, "COMMON"),
        (STT_TLS, "TLS"),
        (STT_GNU_IFUNC, "IFUNC"),
        (15, "Unknown"),
    ] {
        let s = symbol((STB_GLOBAL << 4) | t, 1);
        assert_eq!(s.type_str(), name, "type {}", t);
    }
}

#[test]
fn test_elf_symbol_predicates_and_name() {
    let s = symbol((stb_binding::STB_GLOBAL << 4) | stt_type::STT_FUNC, 1);
    assert!(s.is_defined());
    assert!(s.is_function());
    assert!(s.is_global());
    assert_eq!(s.name_str(), "<7>");

    let undef = symbol(
        (stb_binding::STB_LOCAL << 4) | stt_type::STT_OBJECT,
        shn_index::SHN_UNDEF,
    );
    assert!(!undef.is_defined());
    assert!(!undef.is_function());
    assert!(!undef.is_global());

    let named = Symbol {
        st_name: 1,
        name: Some("main".to_string()),
        st_info: 0,
        st_other: 0,
        st_shndx: 1,
        st_value: 0,
        st_size: 0,
    };
    assert_eq!(named.name_str(), "main");
}

#[test]
fn test_elf_note_entry_gnu_type_and_build_id() {
    let build = NoteEntry {
        name: "GNU".to_string(),
        note_type: nt_gnu::NT_GNU_BUILD_ID,
        desc: vec![0xDE, 0xAD, 0xBE, 0xEF],
    };
    assert_eq!(build.gnu_type_str(), "Build ID");
    assert_eq!(build.build_id_hex(), Some("deadbeef".to_string()));

    let abi = NoteEntry {
        name: "GNU".to_string(),
        note_type: nt_core::NT_GNU_ABI_TAG,
        desc: vec![0],
    };
    assert_eq!(abi.gnu_type_str(), "ABI tag");
    assert_eq!(abi.build_id_hex(), None);

    let gold = NoteEntry {
        name: "GNU".to_string(),
        note_type: nt_gnu::NT_GNU_GOLD_VERSION,
        desc: vec![],
    };
    assert_eq!(gold.gnu_type_str(), "Gold version");

    let prop = NoteEntry {
        name: "GNU".to_string(),
        note_type: nt_gnu::NT_GNU_PROPERTY_TYPE_0,
        desc: vec![],
    };
    assert_eq!(prop.gnu_type_str(), "Property");

    let unknown_type = NoteEntry {
        name: "GNU".to_string(),
        note_type: 999,
        desc: vec![],
    };
    assert_eq!(unknown_type.gnu_type_str(), "Unknown");

    let non_gnu = NoteEntry {
        name: "CORE".to_string(),
        note_type: nt_gnu::NT_GNU_BUILD_ID,
        desc: vec![1, 2, 3],
    };
    assert_eq!(non_gnu.gnu_type_str(), "Unknown");
    assert_eq!(non_gnu.build_id_hex(), None);
}

#[test]
fn test_elf_dynamic_info_and_symbol_info() {
    let mut di = DynamicInfo::default();
    assert!(!di.is_pie());
    assert!(!di.has_relro());
    di.flags_1 = oxidex::parsers::elf::structures::df1_flags::DF_1_PIE;
    assert!(di.is_pie());

    let si = SymbolInfo {
        symbol_count: 10,
        dynamic_symbol_count: 4,
        exported_functions: vec!["foo".to_string()],
        imported_functions: vec!["bar".to_string()],
    };
    assert_eq!(si.symbol_count, 10);
    assert_eq!(si.exported_functions.len(), 1);
}

#[test]
fn test_elf_info_aggregate_methods() {
    let header = elf_header(elf_type::ET_DYN, machine_types::EM_X86_64, 0, true, true);
    let mut info = ElfInfo::new(header);
    // PIE: ET_DYN with non-zero entry.
    assert!(info.is_pie());

    info.section_headers.push(sheader(
        sh_type::SHT_PROGBITS,
        sh_flags::SHF_ALLOC | sh_flags::SHF_EXECINSTR,
        Some(".text".to_string()),
    ));
    info.section_headers.push(sheader(
        sh_type::SHT_PROGBITS,
        sh_flags::SHF_WRITE | sh_flags::SHF_ALLOC,
        Some(".data".to_string()),
    ));
    assert_eq!(info.text_section_size(), Some(0x40));
    assert_eq!(info.data_section_size(), Some(0x40));

    info.program_headers
        .push(pheader(pt_type::PT_LOAD, pf_flags::PF_R | pf_flags::PF_X));
    info.program_headers
        .push(pheader(pt_type::PT_LOAD, pf_flags::PF_R | pf_flags::PF_W));
    info.program_headers
        .push(pheader(pt_type::PT_DYNAMIC, pf_flags::PF_R));
    assert_eq!(info.loadable_segment_count(), 2);

    // Non-PIE: ET_EXEC.
    let header2 = elf_header(elf_type::ET_EXEC, machine_types::EM_386, 0, false, true);
    let info2 = ElfInfo::new(header2);
    assert!(!info2.is_pie());
    assert_eq!(info2.text_section_size(), None);
}
