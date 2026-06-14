//! Coverage tests for JPEG IPTC / Photoshop IRB / JUMBF / FLIR parsers.
//!
//! Targets the REMAINING uncovered paths after wave 1/2 in:
//!
//! - `iptc_parser.rs`: 8BIM IRB walk, IPTC IIM records, record-1/record-2
//!   formatter dispatch, Latin-1 fallback, unknown datasets.
//! - `app_segments/photoshop.rs`: many resource IDs (ResolutionInfo,
//!   GlobalAngle/Altitude, CopyrightFlag, URL, Caption, PrintStyle, PrintFlags,
//!   Thumbnail/Alpha/PrintInfo presence).
//! - `app_segments/jumbf.rs`: APP11 JUMBF box hierarchy
//!   (jumb/jumd/json/cbor/uuid/c2pa/c2ma/c2cl/c2as/c2cs + unknown).
//! - `flir_parser.rs`: FFF index, CameraInfo/RawData/Palette records, legacy
//!   fallback, datetime, helpers.
//! - `core/jpeg_helpers.rs`: process_iptc_segments / process_exif_segments FLIR
//!   branch via the read_metadata production path.
//!
//! Everything is reached through public API: the `parse_*` functions, the
//! `Segment` constructor, and `read_metadata` on synthetic `.jpg` tempfiles.

#[path = "common/mod.rs"]
mod common;

#[allow(unused_imports)]
use common::TestReader;

use oxidex::core::operations::read_metadata;
use oxidex::core::{MetadataMap, TagValue};
use oxidex::parsers::jpeg::app_segments::jumbf::parse_jumbf;
use oxidex::parsers::jpeg::app_segments::photoshop::parse_photoshop_irb;
use oxidex::parsers::jpeg::flir_parser::parse_flir_segment;
use oxidex::parsers::jpeg::iptc_parser::{
    IptcRecord, dataset_to_tag_name, decode_iptc_string, extract_iptc_from_segments,
    parse_all_iptc_records,
};
use oxidex::parsers::jpeg::segment_parser::Segment;
use std::io::Write;
use tempfile::NamedTempFile;

// ===========================================================================
// Low-level byte builders
// ===========================================================================

const PHOTOSHOP_SIG: &[u8] = b"Photoshop 3.0\0";

/// Build a single 8BIM Image Resource Block with an empty name.
///   "8BIM" + id(be16) + name_len(0) + pad(0) + size(be32) + data + [pad if odd]
fn bim_block(id: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"8BIM");
    out.extend_from_slice(&id.to_be_bytes());
    out.push(0x00); // pascal name length = 0
    out.push(0x00); // padding to even (1 length byte -> odd -> 1 pad)
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0x00); // even-pad the data payload
    }
    out
}

/// Build an 8BIM block carrying a non-empty Pascal name (exercises the
/// even-length name branch where total_name_length is even -> no pad).
fn bim_block_named(id: u16, name: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"8BIM");
    out.extend_from_slice(&id.to_be_bytes());
    out.push(name.len() as u8);
    out.extend_from_slice(name);
    let total = 1 + name.len();
    if total % 2 == 1 {
        out.push(0x00);
    }
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0x00);
    }
    out
}

/// Build a single IPTC IIM dataset record.
///   0x1C record dataset len(be16) payload
fn iptc_dataset(record: u8, dataset: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1C, record, dataset];
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// Build a JUMBF box: length(be32, includes 8-byte header) + type + data.
fn jumbf_box(box_type: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let len = (8 + data.len()) as u32;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(box_type);
    out.extend_from_slice(data);
    out
}

/// Wrap raw box bytes in a JUMBF APP11 segment ("JP01\0" identifier).
fn jumbf_segment(boxes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"JP01\0");
    out.extend_from_slice(boxes);
    out
}

/// A JUMBF description box payload (16-byte content UUID + toggles + optional label).
fn jumd_payload(content_type: &[u8; 16], toggles: u8, label: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(content_type);
    out.push(toggles);
    if let Some(l) = label {
        out.extend_from_slice(l);
        out.push(0x00);
    }
    out
}

/// Append a JPEG segment (marker + be16 length + payload) into a buffer.
fn push_segment(buf: &mut Vec<u8>, marker: u16, payload: &[u8]) {
    buf.extend_from_slice(&marker.to_be_bytes());
    let len = (payload.len() + 2) as u16;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
}

/// Build a minimal but structurally-valid JPEG with the given APP payloads,
/// a tiny baseline SOF0 frame, SOS + entropy + EOI so parse_segments walks it.
fn build_jpeg(app_segments: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&[0xFF, 0xD8]); // SOI
    for (marker, payload) in app_segments {
        push_segment(&mut d, *marker, payload);
    }
    // SOF0: precision=8, height=1, width=1, components=1 -> 1*3+6 byte payload
    let sof = vec![
        0x08, // precision
        0x00, 0x01, // height
        0x00, 0x01, // width
        0x01, // num components
        0x01, 0x11, 0x00, // component 1
    ];
    push_segment(&mut d, 0xFFC0, &sof);
    // SOS
    let sos = vec![0x01, 0x01, 0x00, 0x00, 0x3F, 0x00];
    push_segment(&mut d, 0xFFDA, &sos);
    d.extend_from_slice(&[0x00, 0x00]); // a little entropy-coded data
    d.extend_from_slice(&[0xFF, 0xD9]); // EOI
    d
}

fn read_jpeg(app_segments: &[(u16, Vec<u8>)]) -> MetadataMap {
    let bytes = build_jpeg(app_segments);
    let mut tmp = NamedTempFile::with_suffix(".jpg").expect("create tempfile");
    tmp.write_all(&bytes).expect("write tempfile");
    tmp.flush().expect("flush tempfile");
    read_metadata(tmp.path()).expect("read_metadata")
}

// ===========================================================================
// IPTC parser: parse_all_iptc_records + dataset_to_tag_name + decode_iptc_string
// ===========================================================================

#[test]
fn iptc_parse_all_records_multiple() {
    let mut data = Vec::new();
    data.extend(iptc_dataset(2, 5, b"Object Name"));
    data.extend(iptc_dataset(2, 25, b"keyword"));
    data.extend(iptc_dataset(2, 80, b"By Line"));
    let records = parse_all_iptc_records(&data).expect("parse records");
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].dataset_number, 5);
    assert_eq!(records[2].data, b"By Line");
}

#[test]
fn iptc_parse_all_records_stops_on_non_marker() {
    // Leading non-0x1C byte -> immediate break, empty result.
    let data = vec![0x00, 0x1C, 0x02, 0x05, 0x00, 0x01, b'X'];
    let records = parse_all_iptc_records(&data).expect("parse");
    assert!(records.is_empty());
}

#[test]
fn iptc_parse_all_records_stops_on_truncated() {
    // Valid first record, then a truncated record (length claims 99 bytes).
    let mut data = iptc_dataset(2, 5, b"Hi");
    data.extend_from_slice(&[0x1C, 0x02, 0x05, 0x00, 0x63, b'a']); // claims 0x63=99
    let records = parse_all_iptc_records(&data).expect("parse");
    assert_eq!(records.len(), 1);
}

#[test]
fn iptc_parse_all_records_empty_input() {
    let records = parse_all_iptc_records(&[]).expect("parse");
    assert!(records.is_empty());
}

#[test]
fn iptc_dataset_name_record2_spread() {
    // A spread of less-common Record-2 dataset IDs covering many match arms.
    let cases: &[(u8, &str)] = &[
        (0, "IPTC:ApplicationRecordVersion"),
        (7, "IPTC:EditStatus"),
        (15, "IPTC:Category"),
        (20, "IPTC:SupplementalCategories"),
        (22, "IPTC:FixtureIdentifier"),
        (26, "IPTC:ContentLocationCode"),
        (27, "IPTC:ContentLocationName"),
        (30, "IPTC:ReleaseDate"),
        (35, "IPTC:ReleaseTime"),
        (37, "IPTC:ExpirationDate"),
        (38, "IPTC:ExpirationTime"),
        (40, "IPTC:SpecialInstructions"),
        (42, "IPTC:ActionAdvised"),
        (45, "IPTC:ReferenceService"),
        (47, "IPTC:ReferenceDate"),
        (50, "IPTC:ReferenceNumber"),
        (62, "IPTC:DigitalCreationDate"),
        (63, "IPTC:DigitalCreationTime"),
        (65, "IPTC:OriginatingProgram"),
        (70, "IPTC:ProgramVersion"),
        (75, "IPTC:ObjectCycle"),
        (85, "IPTC:By-lineTitle"),
        (92, "IPTC:Sub-location"),
        (95, "IPTC:Province-State"),
        (101, "IPTC:Country-PrimaryLocationName"),
        (103, "IPTC:OriginalTransmissionReference"),
        (105, "IPTC:Headline"),
        (110, "IPTC:Credit"),
        (115, "IPTC:Source"),
        (116, "IPTC:CopyrightNotice"),
        (118, "IPTC:Contact"),
        (121, "IPTC:LocalCaption"),
        (122, "IPTC:Writer-Editor"),
        (125, "IPTC:RasterizedCaption"),
        (130, "IPTC:ImageType"),
        (131, "IPTC:ImageOrientation"),
        (135, "IPTC:LanguageIdentifier"),
        (150, "IPTC:AudioType"),
        (151, "IPTC:AudioSamplingRate"),
        (152, "IPTC:AudioSamplingResolution"),
        (153, "IPTC:AudioDuration"),
        (154, "IPTC:AudioOutcue"),
        (200, "IPTC:ObjectPreviewFileFormat"),
        (201, "IPTC:ObjectPreviewFileFormatVer"),
        (202, "IPTC:ObjectPreviewData"),
    ];
    for (ds, expect) in cases {
        assert_eq!(dataset_to_tag_name(2, *ds), *expect);
    }
}

#[test]
fn iptc_dataset_name_record1_spread() {
    let cases: &[(u8, &str)] = &[
        (0, "IPTC:EnvelopeRecordVersion"),
        (5, "IPTC:Destination"),
        (20, "IPTC:FileFormat"),
        (22, "IPTC:FileVersion"),
        (30, "IPTC:ServiceIdentifier"),
        (40, "IPTC:EnvelopeNumber"),
        (50, "IPTC:ProductID"),
        (60, "IPTC:EnvelopePriority"),
        (70, "IPTC:DateSent"),
        (80, "IPTC:TimeSent"),
        (90, "IPTC:CodedCharacterSet"),
        (100, "IPTC:UniqueObjectName"),
        (120, "IPTC:ARMIdentifier"),
        (122, "IPTC:ARMVersion"),
    ];
    for (ds, expect) in cases {
        assert_eq!(dataset_to_tag_name(1, *ds), *expect);
    }
}

#[test]
fn iptc_dataset_name_unknown_arms() {
    assert_eq!(dataset_to_tag_name(2, 250), "IPTC:Unknown-2-250");
    assert_eq!(dataset_to_tag_name(1, 250), "IPTC:Unknown-1-250");
    assert_eq!(dataset_to_tag_name(7, 5), "IPTC:Unknown-7-5");
}

#[test]
fn iptc_decode_string_latin1_fallback() {
    // 0xE9 = 'é' in Latin-1 but invalid as a lone UTF-8 byte -> Latin-1 path.
    let decoded = decode_iptc_string(&[b'c', b'a', b'f', 0xE9, b' ', b' ']);
    assert_eq!(decoded, "café");
    // Pure-UTF8 trimmed path.
    assert_eq!(decode_iptc_string(b"  hi  "), "hi");
    // Empty input.
    assert_eq!(decode_iptc_string(b""), "");
}

#[test]
fn iptc_record_struct_construction() {
    let r = IptcRecord {
        record_number: 2,
        dataset_number: 5,
        data: vec![1, 2, 3],
    };
    assert_eq!(r.record_number, 2);
    assert_eq!(r.dataset_number, 5);
    assert_eq!(r.data.len(), 3);
    // exercise derive(Clone/Debug/PartialEq)
    let r2 = r.clone();
    assert_eq!(r, r2);
    let _ = format!("{r:?}");
}

// ===========================================================================
// IPTC via extract_iptc_from_segments: record-1 + record-2 formatter dispatch
// ===========================================================================

/// Build an APP13 Photoshop segment carrying an IPTC 8BIM (id 0x0404) block.
fn app13_iptc(records: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(PHOTOSHOP_SIG);
    payload.extend_from_slice(&bim_block(0x0404, records));
    payload
}

#[test]
fn iptc_extract_record1_formatters() {
    // Record 1 special formatter branches: version(0), date(70), time(80), charset(90).
    let mut recs = Vec::new();
    recs.extend(iptc_dataset(1, 0, &[0x00, 0x04])); // EnvelopeRecordVersion -> u16
    recs.extend(iptc_dataset(1, 70, b"20240115")); // DateSent -> YYYY:MM:DD
    recs.extend(iptc_dataset(1, 80, b"153000+0100")); // TimeSent
    recs.extend(iptc_dataset(1, 90, &[0x1B, 0x25, 0x47])); // CodedCharacterSet (ESC % G = UTF-8)
    recs.extend(iptc_dataset(1, 5, b"dest")); // generic string branch

    let seg_data = app13_iptc(&recs);
    let segment = Segment::new(0xFFED, 0, &seg_data);
    let tags = extract_iptc_from_segments(&[segment]).expect("extract");

    let find = |k: &str| {
        tags.iter()
            .find(|(name, _)| name == k)
            .map(|(_, v)| v.clone())
    };
    assert!(find("IPTC:EnvelopeRecordVersion").is_some());
    assert_eq!(find("IPTC:DateSent").as_deref(), Some("2024:01:15"));
    assert!(find("IPTC:TimeSent").is_some());
    assert!(find("IPTC:CodedCharacterSet").is_some());
    assert_eq!(find("IPTC:Destination").as_deref(), Some("dest"));
}

#[test]
fn iptc_extract_record2_formatters() {
    // Record 2 special formatter branches: version(0), urgency(10),
    // date fields (30/37/47/55/62/70), time fields (35/38/60/63), generic.
    let mut recs = Vec::new();
    recs.extend(iptc_dataset(2, 0, &[0x00, 0x02])); // ApplicationRecordVersion
    recs.extend(iptc_dataset(2, 10, b"1")); // Urgency -> mapped description
    recs.extend(iptc_dataset(2, 30, b"20230301")); // ReleaseDate
    recs.extend(iptc_dataset(2, 37, b"20231231")); // ExpirationDate
    recs.extend(iptc_dataset(2, 47, b"20230401")); // ReferenceDate
    recs.extend(iptc_dataset(2, 55, b"20230501")); // DateCreated
    recs.extend(iptc_dataset(2, 62, b"20230601")); // DigitalCreationDate
    recs.extend(iptc_dataset(2, 35, b"120000")); // ReleaseTime
    recs.extend(iptc_dataset(2, 38, b"130000")); // ExpirationTime
    recs.extend(iptc_dataset(2, 60, b"140000")); // TimeCreated
    recs.extend(iptc_dataset(2, 63, b"150000")); // DigitalCreationTime
    recs.extend(iptc_dataset(2, 105, b"Big Headline")); // generic string

    let seg_data = app13_iptc(&recs);
    let segment = Segment::new(0xFFED, 0, &seg_data);
    let tags = extract_iptc_from_segments(&[segment]).expect("extract");

    let names: Vec<&str> = tags.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"IPTC:ApplicationRecordVersion"));
    assert!(names.contains(&"IPTC:Urgency"));
    assert!(names.contains(&"IPTC:ReleaseDate"));
    assert!(names.contains(&"IPTC:DigitalCreationTime"));
    assert!(names.contains(&"IPTC:Headline"));
    let dc = tags.iter().find(|(n, _)| n == "IPTC:DateCreated").unwrap();
    assert_eq!(dc.1, "2023:05:01");
}

#[test]
fn iptc_extract_skips_non_app13_and_non_photoshop() {
    // Non-APP13 segment is skipped (marker != 0xFFED).
    let other = Segment::new(0xFFE1, 0, b"Exif\0\0xx");
    // APP13 but not Photoshop signature -> skipped.
    let bad13 = Segment::new(0xFFED, 0, b"NotPhotoshop data here");
    let tags = extract_iptc_from_segments(&[other, bad13]).expect("extract");
    assert!(tags.is_empty());
}

#[test]
fn iptc_extract_app13_non_iptc_resource_ignored() {
    // APP13 Photoshop with a non-0x0404 resource block -> no IPTC tags emitted.
    let mut payload = Vec::new();
    payload.extend_from_slice(PHOTOSHOP_SIG);
    payload.extend_from_slice(&bim_block(0x040D, &[0, 0, 0, 30])); // GlobalAngle, not IPTC
    let seg = Segment::new(0xFFED, 0, &payload);
    let tags = extract_iptc_from_segments(&[seg]).expect("extract");
    assert!(tags.is_empty());
}

#[test]
fn iptc_extract_app13_named_resource_block() {
    // Exercise the even-length Pascal name branch inside the IRB walk.
    let mut payload = Vec::new();
    payload.extend_from_slice(PHOTOSHOP_SIG);
    payload.extend_from_slice(&bim_block_named(
        0x0404,
        b"abc",
        &iptc_dataset(2, 5, b"Named"),
    ));
    let seg = Segment::new(0xFFED, 0, &payload);
    let tags = extract_iptc_from_segments(&[seg]).expect("extract");
    assert_eq!(
        tags.iter()
            .find(|(n, _)| n == "IPTC:ObjectName")
            .map(|(_, v)| v.as_str()),
        Some("Named")
    );
}

// ===========================================================================
// Photoshop IRB: many resource IDs through parse_photoshop_irb
// ===========================================================================

#[test]
fn photoshop_resolution_info() {
    // 16-byte ResolutionInfo (0x03ED): h-res 300 dpi, units, v-res 300, units.
    let mut res = Vec::new();
    res.extend_from_slice(&(300i32 * 65536).to_be_bytes()); // h res 16.16
    res.extend_from_slice(&2u16.to_be_bytes()); // h res unit = cm
    res.extend_from_slice(&3u16.to_be_bytes()); // width unit = points
    res.extend_from_slice(&(300i32 * 65536).to_be_bytes()); // v res
    res.extend_from_slice(&2u16.to_be_bytes()); // v res unit
    res.extend_from_slice(&4u16.to_be_bytes()); // height unit = picas

    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    data.extend_from_slice(&bim_block(0x03ED, &res));
    let md = parse_photoshop_irb(&data).expect("irb");
    assert_eq!(md.get_string("Photoshop:ResolutionUnit"), Some("cm"));
    assert_eq!(md.get_string("Photoshop:WidthUnit"), Some("points"));
    assert_eq!(md.get_string("Photoshop:HeightUnit"), Some("picas"));
    assert!(md.get("Photoshop:XResolution").is_some());
}

#[test]
fn photoshop_resolution_info_unknown_units() {
    // Units that fall into the "Unknown" arms.
    let mut res = Vec::new();
    res.extend_from_slice(&(72i32 * 65536).to_be_bytes());
    res.extend_from_slice(&9u16.to_be_bytes()); // h res unit -> Unknown
    res.extend_from_slice(&9u16.to_be_bytes()); // width unit -> Unknown
    res.extend_from_slice(&(72i32 * 65536).to_be_bytes());
    res.extend_from_slice(&9u16.to_be_bytes());
    res.extend_from_slice(&9u16.to_be_bytes()); // height unit -> Unknown
    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    data.extend_from_slice(&bim_block(0x03ED, &res));
    let md = parse_photoshop_irb(&data).expect("irb");
    assert_eq!(md.get_string("Photoshop:ResolutionUnit"), Some("Unknown"));
    assert_eq!(md.get_string("Photoshop:WidthUnit"), Some("Unknown"));
    assert_eq!(md.get_string("Photoshop:HeightUnit"), Some("Unknown"));
}

#[test]
fn photoshop_resolution_info_too_short() {
    // < 16 bytes -> early Ok(empty) inside parse_resolution_info.
    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    data.extend_from_slice(&bim_block(0x03ED, &[0, 0, 0, 0]));
    let md = parse_photoshop_irb(&data).expect("irb");
    assert!(md.get_string("Photoshop:ResolutionUnit").is_none());
}

#[test]
fn photoshop_global_angle_and_altitude() {
    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    data.extend_from_slice(&bim_block(0x040D, &30i32.to_be_bytes())); // GlobalAngle
    data.extend_from_slice(&bim_block(0x0419, &(-100i32).to_be_bytes())); // GlobalAltitude
    let md = parse_photoshop_irb(&data).expect("irb");
    assert_eq!(md.get_integer("Photoshop:GlobalAngle"), Some(30));
    assert_eq!(md.get_integer("Photoshop:GlobalAltitude"), Some(-100));
}

#[test]
fn photoshop_copyright_flag_true_false() {
    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    data.extend_from_slice(&bim_block(0x040A, &[0x01])); // CopyrightFlag true
    let md = parse_photoshop_irb(&data).expect("irb");
    assert_eq!(md.get_string("Photoshop:CopyrightFlag"), Some("True"));

    let mut data2 = Vec::new();
    data2.extend_from_slice(PHOTOSHOP_SIG);
    data2.extend_from_slice(&bim_block(0x040A, &[0x00])); // false
    let md2 = parse_photoshop_irb(&data2).expect("irb");
    assert_eq!(md2.get_string("Photoshop:CopyrightFlag"), Some("False"));
}

#[test]
fn photoshop_url_and_caption() {
    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    data.extend_from_slice(&bim_block(0x040B, b"https://oxidex.test/\0")); // URL
    // Caption (0x03F0) is a Pascal string: len byte then text.
    let caption = {
        let text = b"A caption";
        let mut v = vec![text.len() as u8];
        v.extend_from_slice(text);
        v
    };
    data.extend_from_slice(&bim_block(0x03F0, &caption));
    let md = parse_photoshop_irb(&data).expect("irb");
    assert_eq!(md.get_string("Photoshop:URL"), Some("https://oxidex.test/"));
    assert_eq!(md.get_string("Photoshop:Caption"), Some("A caption"));
}

#[test]
fn photoshop_print_flags_all_bits() {
    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    // PrintFlagsInfo (0x2710), all low 5 bits set.
    data.extend_from_slice(&bim_block(0x2710, &0x001Fu16.to_be_bytes()));
    let md = parse_photoshop_irb(&data).expect("irb");
    assert_eq!(md.get_integer("Photoshop:PrintFlags"), Some(0x1F));
    assert_eq!(md.get_string("Photoshop:PrintLabels"), Some("True"));
    assert_eq!(md.get_string("Photoshop:PrintCropMarks"), Some("True"));
    assert_eq!(md.get_string("Photoshop:PrintColorBars"), Some("True"));
    assert_eq!(
        md.get_string("Photoshop:PrintRegistrationMarks"),
        Some("True")
    );
    assert_eq!(md.get_string("Photoshop:PrintNegative"), Some("True"));
}

#[test]
fn photoshop_print_style_and_presence_markers() {
    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    data.extend_from_slice(&bim_block(0x043B, &[0xAA, 0xBB])); // PrintStyle
    data.extend_from_slice(&bim_block(0x040C, &[0x00, 0x01, 0x02, 0x03])); // Thumbnail
    data.extend_from_slice(&bim_block(0x03EE, &[0x01])); // AlphaChannels
    data.extend_from_slice(&bim_block(0x042F, &[0x09, 0x09])); // PrintInfo
    let md = parse_photoshop_irb(&data).expect("irb");
    assert_eq!(md.get_string("Photoshop:PrintStylePresent"), Some("Yes"));
    assert_eq!(md.get_string("Photoshop:ThumbnailPresent"), Some("Yes"));
    assert_eq!(md.get_string("Photoshop:AlphaChannelsPresent"), Some("Yes"));
    assert_eq!(md.get_string("Photoshop:PrintInfoPresent"), Some("Yes"));
}

#[test]
fn photoshop_unknown_resource_is_noop() {
    // A resource ID not handled in the match -> default arm, still walks cleanly.
    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    data.extend_from_slice(&bim_block(0x0BB7, &[0xDE, 0xAD])); // arbitrary id
    data.extend_from_slice(&bim_block(0x040D, &7i32.to_be_bytes())); // followed by known
    let md = parse_photoshop_irb(&data).expect("irb");
    assert_eq!(md.get_integer("Photoshop:GlobalAngle"), Some(7));
}

#[test]
fn photoshop_invalid_signature_errors() {
    let result = parse_photoshop_irb(b"NotPhotoshop at all");
    assert!(result.is_err());
}

#[test]
fn photoshop_stops_on_non_8bim() {
    // Photoshop signature then garbage that is not an 8BIM block -> loop breaks.
    let mut data = Vec::new();
    data.extend_from_slice(PHOTOSHOP_SIG);
    data.extend_from_slice(b"XXXXjunk");
    let md = parse_photoshop_irb(&data).expect("irb");
    assert_eq!(md.iter().count(), 0);
}

// ===========================================================================
// JUMBF (APP11) box hierarchy
// ===========================================================================

#[test]
fn jumbf_not_a_jumbf_segment_errors() {
    assert!(parse_jumbf(b"EXIF\0somedata").is_err());
    assert!(parse_jumbf(b"JP").is_err()); // too short
    assert!(parse_jumbf(b"JP01X").is_err()); // no null terminator at expected place
}

#[test]
fn jumbf_empty_after_identifier() {
    let md = parse_jumbf(b"JP01\0").expect("parse");
    assert_eq!(md.iter().count(), 0);
}

#[test]
fn jumbf_description_box_with_label_and_requestable() {
    let payload = jumd_payload(&[0xAB; 16], 0x01, Some(b"my-label"));
    let boxes = jumbf_box(b"jumd", &payload);
    let seg = jumbf_segment(&boxes);
    let md = parse_jumbf(&seg).expect("parse");
    assert!(md.get_string("JUMBF:ContentType").is_some());
    assert_eq!(md.get_string("JUMBF:Requestable"), Some("True"));
    assert_eq!(md.get_string("JUMBF:Label"), Some("my-label"));
}

#[test]
fn jumbf_superbox_with_nested_description() {
    // jumb superbox containing a jumd description box.
    let desc = jumbf_box(b"jumd", &jumd_payload(&[0u8; 16], 0x00, None));
    let superbox = jumbf_box(b"jumb", &desc);
    let seg = jumbf_segment(&superbox);
    let md = parse_jumbf(&seg).expect("parse");
    assert!(md.get_string("JUMBF:ContentType").is_some());
}

#[test]
fn jumbf_json_and_cbor_and_uuid_boxes() {
    let mut boxes = Vec::new();
    boxes.extend(jumbf_box(b"json", br#"{"a":1}"#));
    boxes.extend(jumbf_box(b"cbor", &[0xA1, 0x01, 0x02]));
    let mut uuid_payload = vec![0x11u8; 16];
    uuid_payload.extend_from_slice(&[0xFF, 0xEE]); // trailing data after UUID
    boxes.extend(jumbf_box(b"uuid", &uuid_payload));
    let seg = jumbf_segment(&boxes);
    let md = parse_jumbf(&seg).expect("parse");
    assert!(md.get_string("JUMBF:JSONData").is_some());
    assert!(md.get_string("JUMBF:CBORData").is_some());
    assert!(md.get_string("JUMBF:UUID").is_some());
    assert!(md.get_string("JUMBF:UUIDData").is_some());
}

#[test]
fn jumbf_json_invalid_utf8_binary_branch() {
    // Invalid UTF-8 inside a json box -> "(Binary data N bytes)" branch.
    let boxes = jumbf_box(b"json", &[0xFF, 0xFE, 0xFD]);
    let seg = jumbf_segment(&boxes);
    let md = parse_jumbf(&seg).expect("parse");
    let v = md.get_string("JUMBF:JSONData").unwrap_or("");
    assert!(v.contains("Binary data"));
}

#[test]
fn jumbf_c2pa_manifest_store_and_assertion_store() {
    // c2pa manifest store contains nested c2ma manifest + c2cl claim + c2cs sig.
    let claim_json = br#"{"dc:title":"x","actions":[],"assertions":[],"ingredients":[]}"#;
    let mut inner = Vec::new();
    inner.extend(jumbf_box(b"c2ma", &[0xA2, 0x01, 0x02])); // CBOR-ish manifest
    inner.extend(jumbf_box(b"c2cl", claim_json)); // JSON claim
    inner.extend(jumbf_box(b"c2cs", &[0xA3, 0x01, 0x02])); // COSE signature
    let store = jumbf_box(b"c2pa", &inner);

    // also an assertion store box
    let assertion_store = jumbf_box(b"c2as", &jumbf_box(b"json", br#"{"k":1}"#));

    let mut boxes = Vec::new();
    boxes.extend_from_slice(&store);
    boxes.extend_from_slice(&assertion_store);
    let seg = jumbf_segment(&boxes);
    let md = parse_jumbf(&seg).expect("parse");

    assert_eq!(md.get_string("C2PA:ManifestStore"), Some("Present"));
    assert_eq!(md.get_string("C2PA:Manifest"), Some("Present"));
    assert_eq!(md.get_string("C2PA:Claim"), Some("Present"));
    assert_eq!(md.get_string("C2PA:AssertionStore"), Some("Present"));
    assert_eq!(md.get_string("C2PA:ManifestFormat"), Some("CBOR"));
    assert!(md.get_integer("C2PA:ClaimSize").is_some());
    assert_eq!(
        md.get_string("C2PA:ClaimGenerator"),
        Some("Present in claim")
    );
    assert_eq!(md.get_string("C2PA:Actions"), Some("Present in claim"));
    assert_eq!(md.get_string("C2PA:Assertions"), Some("Present in claim"));
    assert_eq!(md.get_string("C2PA:Ingredients"), Some("Present in claim"));
    assert!(md.get_string("C2PA:ClaimSignature").is_some());
    assert_eq!(md.get_string("C2PA:SignatureFormat"), Some("COSE"));
}

#[test]
fn jumbf_unknown_box_type_recorded() {
    let boxes = jumbf_box(b"zzzz", &[0x01, 0x02, 0x03]);
    let seg = jumbf_segment(&boxes);
    let md = parse_jumbf(&seg).expect("parse");
    assert!(md.get_string("JUMBF:UnknownBox_zzzz").is_some());
}

#[test]
fn jumbf_manifest_non_cbor_first_byte() {
    // c2ma manifest whose first byte is NOT in CBOR map/array range -> no format tag,
    // but size is still recorded.
    let inner = jumbf_box(b"c2ma", &[0x00, 0x01, 0x02]);
    let store = jumbf_box(b"c2pa", &inner);
    let seg = jumbf_segment(&store);
    let md = parse_jumbf(&seg).expect("parse");
    assert!(md.get_integer("C2PA:ManifestSize").is_some());
    assert!(md.get_string("C2PA:ManifestFormat").is_none());
}

#[test]
fn jumbf_box_with_length_zero_extends_to_end() {
    // A box whose length field is 0 means "extends to end of data".
    let mut boxes = Vec::new();
    boxes.extend_from_slice(&0u32.to_be_bytes()); // length 0
    boxes.extend_from_slice(b"json");
    boxes.extend_from_slice(br#"{"end":true}"#);
    let seg = jumbf_segment(&boxes);
    let md = parse_jumbf(&seg).expect("parse");
    assert!(md.get_string("JUMBF:JSONData").is_some());
}

// ===========================================================================
// FLIR (APP1) thermal segments
// ===========================================================================

/// Wrap FFF data in a single-segment FLIR APP1 payload:
///   "FLIR\0" + marker(1) + index(0) + reserved(0) + fff_data
fn flir_single_segment(fff: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"FLIR\x00");
    out.push(0x01); // marker/version
    out.push(0x00); // index 0
    out.push(0x00); // reserved
    out.extend_from_slice(fff);
    out
}

/// Build a standard FFF block with a record index pointing at one record.
/// Layout: 64-byte header, then a 32-byte index entry, then the record data.
fn build_fff(record_type: u16, record_data: &[u8]) -> Vec<u8> {
    let header_len = 64usize;
    let index_offset = header_len; // index right after header
    let entry_size = 32usize;
    let record_offset = index_offset + entry_size;

    let mut d = vec![0u8; record_offset + record_data.len()];
    // FFF magic
    d[0..4].copy_from_slice(b"FFF\0");
    // CreatorSoftware at 0x08 (16 bytes)
    d[0x08..0x08 + 7].copy_from_slice(b"OxiCam\0");
    // record_count at offset 28 (u32 LE)
    d[28..32].copy_from_slice(&1u32.to_le_bytes());
    // index_offset at offset 32 (u32 LE)
    d[32..36].copy_from_slice(&(index_offset as u32).to_le_bytes());

    // Index entry (LE): type at +0, offset at +12, length at +16
    let e = index_offset;
    d[e..e + 2].copy_from_slice(&record_type.to_le_bytes());
    d[e + 12..e + 16].copy_from_slice(&(record_offset as u32).to_le_bytes());
    d[e + 16..e + 20].copy_from_slice(&(record_data.len() as u32).to_le_bytes());

    // Record data
    d[record_offset..record_offset + record_data.len()].copy_from_slice(record_data);
    d
}

fn le_f32(v: f32) -> [u8; 4] {
    v.to_le_bytes()
}

#[test]
fn flir_too_short_and_wrong_signature_errors() {
    let mut md = MetadataMap::new();
    assert!(parse_flir_segment(b"FLIR", &mut md).is_err()); // < MIN length
    let mut md2 = MetadataMap::new();
    assert!(parse_flir_segment(b"NOTFLIRDATA1234", &mut md2).is_err());
}

#[test]
fn flir_camera_info_record_full() {
    // Build a CameraInfo record (type 0x0020) large enough to reach the
    // late offsets (frame rate at 0x0464).
    let mut rec = vec![0u8; 0x0470];
    // Emissivity (0x20) = 0.95
    rec[0x20..0x24].copy_from_slice(&le_f32(0.95));
    // Object distance (0x24) = 2.5 m
    rec[0x24..0x28].copy_from_slice(&le_f32(2.5));
    // Reflected apparent temp (0x28) = 295 K
    rec[0x28..0x2C].copy_from_slice(&le_f32(295.0));
    // Atmospheric temp (0x2C) = 293 K
    rec[0x2C..0x30].copy_from_slice(&le_f32(293.0));
    // IR window temp (0x30) = 293 K
    rec[0x30..0x34].copy_from_slice(&le_f32(293.0));
    // IR window transmission (0x34) = 0.9
    rec[0x34..0x38].copy_from_slice(&le_f32(0.9));
    // Relative humidity (0x3C) = 50%
    rec[0x3C..0x40].copy_from_slice(&le_f32(50.0));
    // Planck R1/B/F (0x58/0x5C/0x60)
    rec[0x58..0x5C].copy_from_slice(&le_f32(17096.0));
    rec[0x5C..0x60].copy_from_slice(&le_f32(1428.0));
    rec[0x60..0x64].copy_from_slice(&le_f32(1.0));
    // Atmospheric trans alpha1/alpha2/beta1/beta2/X (0x70..0x84)
    rec[0x70..0x74].copy_from_slice(&le_f32(0.006569));
    rec[0x74..0x78].copy_from_slice(&le_f32(0.012620));
    rec[0x78..0x7C].copy_from_slice(&le_f32(-0.002276));
    rec[0x7C..0x80].copy_from_slice(&le_f32(-0.006670));
    rec[0x80..0x84].copy_from_slice(&le_f32(1.9));
    // Camera temp range max/min (0x90/0x94)
    rec[0x90..0x94].copy_from_slice(&le_f32(393.0));
    rec[0x94..0x98].copy_from_slice(&le_f32(253.0));
    // Camera model (0xD4, 32 bytes)
    rec[0xD4..0xD4 + 8].copy_from_slice(b"FLIR E60");
    // Camera part number (0xF4)
    rec[0xF4..0xF4 + 6].copy_from_slice(b"PN1234");
    // Camera serial (0x104, 16 bytes)
    rec[0x104..0x104 + 6].copy_from_slice(b"SN5678");
    // Camera software (0x114)
    rec[0x114..0x114 + 5].copy_from_slice(b"1.2.3");
    // Lens model (0x170)
    rec[0x170..0x170 + 7].copy_from_slice(b"FOL18mm");
    // Field of view (0x1B4) = 45 deg
    rec[0x1B4..0x1B8].copy_from_slice(&le_f32(45.0));
    // Peak spectral sensitivity (0x1B8) = 10 micrometers
    rec[0x1B8..0x1BC].copy_from_slice(&le_f32(10.0));
    // Planck O (i32, 0x308) and R2 (f32, 0x30C)
    rec[0x308..0x30C].copy_from_slice(&(-100i32).to_le_bytes());
    rec[0x30C..0x310].copy_from_slice(&le_f32(0.05));
    // Raw value range min/max (0x310/0x312, u16)
    rec[0x310..0x312].copy_from_slice(&100u16.to_le_bytes());
    rec[0x312..0x314].copy_from_slice(&5000u16.to_le_bytes());
    // Raw value median/range (0x338/0x33C)
    rec[0x338..0x33A].copy_from_slice(&2500u16.to_le_bytes());
    rec[0x33C..0x33E].copy_from_slice(&4900u16.to_le_bytes());
    // DateTimeOriginal (0x384, f64 unix seconds ~ 2023)
    rec[0x384..0x38C].copy_from_slice(&1_700_000_000f64.to_le_bytes());
    // Focus step count (i16, 0x390)
    rec[0x390..0x392].copy_from_slice(&123i16.to_le_bytes());
    // Focus distance (f32, 0x45C)
    rec[0x45C..0x460].copy_from_slice(&le_f32(3.0));
    // Frame rate (u16, 0x464)
    rec[0x464..0x466].copy_from_slice(&30u16.to_le_bytes());

    let fff = build_fff(0x0020, &rec);
    let seg = flir_single_segment(&fff);
    let mut md = MetadataMap::new();
    parse_flir_segment(&seg, &mut md).expect("flir");

    assert!(md.get_string("FLIR:CreatorSoftware").is_some());
    assert_eq!(md.get_string("FLIR:CameraModel"), Some("FLIR E60"));
    assert_eq!(md.get_string("FLIR:CameraSerialNumber"), Some("SN5678"));
    assert_eq!(md.get_string("FLIR:LensModel"), Some("FOL18mm"));
    assert!(md.get_float("FLIR:Emissivity").is_some());
    assert!(md.get_float("FLIR:PlanckR1").is_some());
    assert!(md.get_integer("FLIR:PlanckO").is_some());
    assert!(md.get_float("FLIR:ReflectedApparentTemperature").is_some());
    assert!(md.get_integer("FLIR:RawValueRangeMax").is_some());
    assert!(md.get_string("FLIR:DateTimeOriginal").is_some());
    assert_eq!(md.get_integer("FLIR:FocusStepCount"), Some(123));
    assert_eq!(md.get_integer("FLIR:FrameRate"), Some(30));
}

#[test]
fn flir_raw_data_record() {
    // RawData record (type 0x0001): byte order, width, height, image type, then payload.
    let mut rec = vec![0u8; 64];
    rec[0x00..0x02].copy_from_slice(&0u16.to_le_bytes()); // byte order = little
    rec[0x02..0x04].copy_from_slice(&320u16.to_le_bytes()); // width
    rec[0x04..0x06].copy_from_slice(&240u16.to_le_bytes()); // height
    rec[0x10..0x12].copy_from_slice(&2u16.to_le_bytes()); // image type = U16 compressed

    let fff = build_fff(0x0001, &rec);
    let seg = flir_single_segment(&fff);
    let mut md = MetadataMap::new();
    parse_flir_segment(&seg, &mut md).expect("flir");

    assert_eq!(
        md.get_string("FLIR:RawDataByteOrder"),
        Some("Little-endian")
    );
    assert_eq!(md.get_integer("FLIR:RawThermalImageWidth"), Some(320));
    assert_eq!(md.get_integer("FLIR:RawThermalImageHeight"), Some(240));
    assert_eq!(
        md.get_string("FLIR:RawThermalImageType"),
        Some("U16 (Compressed)")
    );
    assert!(md.get_string("FLIR:RawThermalImage").is_some());
}

#[test]
fn flir_palette_info_record() {
    // PaletteInfo record (type 0x0022).
    let mut rec = vec![0u8; 0x90];
    rec[0x00] = 224; // palette colors
    // color triplets
    rec[0x06..0x09].copy_from_slice(&[0xFF, 0x00, 0x00]); // above
    rec[0x09..0x0C].copy_from_slice(&[0x00, 0xFF, 0x00]); // below
    rec[0x0C..0x0F].copy_from_slice(&[0x00, 0x00, 0xFF]); // overflow
    rec[0x0F..0x12].copy_from_slice(&[0x11, 0x22, 0x33]); // underflow
    rec[0x12..0x15].copy_from_slice(&[0x44, 0x55, 0x66]); // isotherm1
    rec[0x15..0x18].copy_from_slice(&[0x77, 0x88, 0x99]); // isotherm2
    rec[0x1A] = 1; // method = Color Bar
    rec[0x1B] = 2; // stretch = Manual
    rec[0x30..0x30 + 8].copy_from_slice(b"pal.pal\0"); // file name
    rec[0x50..0x50 + 4].copy_from_slice(b"Iron"); // palette name

    let fff = build_fff(0x0022, &rec);
    let seg = flir_single_segment(&fff);
    let mut md = MetadataMap::new();
    parse_flir_segment(&seg, &mut md).expect("flir");

    assert_eq!(md.get_integer("FLIR:PaletteColors"), Some(224));
    assert_eq!(md.get_string("FLIR:AboveColor"), Some("#FF0000"));
    assert_eq!(md.get_string("FLIR:PaletteMethod"), Some("Color Bar"));
    assert_eq!(md.get_string("FLIR:PaletteStretch"), Some("Manual"));
    assert_eq!(md.get_string("FLIR:PaletteName"), Some("Iron"));
    assert!(md.get_string("FLIR:Palette").is_some());
}

#[test]
fn flir_embedded_image_record() {
    // EmbeddedImage record (type 0x000E) -> just notes presence.
    let rec = vec![0u8; 128];
    let fff = build_fff(0x000E, &rec);
    let seg = flir_single_segment(&fff);
    let mut md = MetadataMap::new();
    parse_flir_segment(&seg, &mut md).expect("flir");
    assert!(md.get_string("FLIR:EmbeddedImage").is_some());
}

#[test]
fn flir_legacy_fallback_short_payload() {
    // Payload < 64 bytes -> parse_flir_legacy_format path. Put emissivity at 0x20,
    // a camera model at 0x00, and dimensions at 0x02/0x04.
    let mut payload = vec![0u8; 60];
    payload[0x00..0x08].copy_from_slice(b"FLIRcam\0");
    payload[0x02..0x04].copy_from_slice(&64u16.to_le_bytes()); // width (also overlaps model)
    payload[0x04..0x06].copy_from_slice(&48u16.to_le_bytes()); // height
    payload[0x20..0x24].copy_from_slice(&le_f32(0.95)); // emissivity
    let seg = flir_single_segment(&payload);
    let mut md = MetadataMap::new();
    parse_flir_segment(&seg, &mut md).expect("flir legacy");
    // At least the emissivity should be parsed in the legacy path.
    assert!(md.get_float("FLIR:Emissivity").is_some());
}

#[test]
fn flir_fff_unreasonable_record_count_falls_back() {
    // record_count > 100 triggers the legacy fallback inside parse_fff_with_index.
    let mut d = vec![0u8; 128];
    d[0..4].copy_from_slice(b"FFF\0");
    d[28..32].copy_from_slice(&999u32.to_le_bytes()); // record_count too large
    d[32..36].copy_from_slice(&64u32.to_le_bytes());
    // emissivity for the legacy fallback
    d[0x20..0x24].copy_from_slice(&le_f32(0.8));
    let seg = flir_single_segment(&d);
    let mut md = MetadataMap::new();
    parse_flir_segment(&seg, &mut md).expect("flir");
    // Should not panic; legacy path may or may not extract, just ensure Ok.
    let _ = md.get_float("FLIR:Emissivity");
}

#[test]
fn flir_payload_only_header_returns_ok() {
    // data.len() == 8 -> the `else { return Ok(()) }` branch (no payload).
    let seg = b"FLIR\x00\x01\x00\x00"; // exactly 8 bytes
    let mut md = MetadataMap::new();
    // 8 bytes is below MIN_FLIR_SEGMENT_LENGTH (11) so this errors; use 11+ with no payload.
    assert!(parse_flir_segment(seg, &mut md).is_err());

    // 11 bytes: header(8) + 3 payload bytes -> single-segment legacy path, Ok.
    let seg2 = b"FLIR\x00\x01\x00\x00\xAA\xBB\xCC";
    let mut md2 = MetadataMap::new();
    assert!(parse_flir_segment(seg2, &mut md2).is_ok());
}

// ===========================================================================
// Production path via read_metadata on synthetic JPEGs
// ===========================================================================

#[test]
fn read_metadata_jpeg_with_app13_iptc() {
    let mut recs = Vec::new();
    recs.extend(iptc_dataset(2, 5, b"Prod Title"));
    recs.extend(iptc_dataset(2, 80, b"Prod Author"));
    recs.extend(iptc_dataset(2, 25, b"tagword"));
    let app13 = app13_iptc(&recs);

    let md = read_jpeg(&[(0xFFED, app13)]);
    assert_eq!(
        md.get("IPTC:ObjectName"),
        Some(&TagValue::String("Prod Title".into()))
    );
    assert_eq!(
        md.get("IPTC:By-line"),
        Some(&TagValue::String("Prod Author".into()))
    );
    assert_eq!(
        md.get("IPTC:Keywords"),
        Some(&TagValue::String("tagword".into()))
    );
}

#[test]
fn read_metadata_jpeg_with_app13_photoshop_resources() {
    // APP13 with Photoshop resources but no IPTC block -> photoshop helper path
    // is NOT directly invoked by read_metadata, but the IPTC walk still runs over
    // the same segment (and finds no 0x0404). This exercises process_iptc_segments
    // skipping non-IPTC resource blocks via the production path.
    let mut payload = Vec::new();
    payload.extend_from_slice(PHOTOSHOP_SIG);
    payload.extend_from_slice(&bim_block(0x040D, &30i32.to_be_bytes()));
    payload.extend_from_slice(&bim_block(0x040A, &[0x01]));
    let md = read_jpeg(&[(0xFFED, payload)]);
    // No IPTC tags, but the read should succeed and have File metadata.
    assert!(md.get("IPTC:ObjectName").is_none());
    assert!(md.iter().count() > 0);
}

#[test]
fn read_metadata_jpeg_with_app1_flir() {
    // APP1 FLIR segment routed through process_exif_segments -> parse_flir_segment.
    let mut rec = vec![0u8; 0x140];
    rec[0x20..0x24].copy_from_slice(&le_f32(0.97)); // emissivity
    rec[0xD4..0xD4 + 8].copy_from_slice(b"FLIR T62"); // camera model
    let fff = build_fff(0x0020, &rec);
    let app1 = flir_single_segment(&fff);

    let md = read_jpeg(&[(0xFFE1, app1)]);
    assert_eq!(
        md.get("FLIR:CameraModel"),
        Some(&TagValue::String("FLIR T62".into()))
    );
    assert!(md.get("FLIR:Emissivity").is_some());
}

#[test]
fn read_metadata_jpeg_combined_iptc_and_flir() {
    // One JPEG carrying both an APP1 FLIR and an APP13 IPTC segment.
    let mut rec = vec![0u8; 0x100];
    rec[0xD4..0xD4 + 6].copy_from_slice(b"FLIRX1");
    let fff = build_fff(0x0020, &rec);
    let app1 = flir_single_segment(&fff);

    let app13 = app13_iptc(&iptc_dataset(2, 105, b"Combined"));

    let md = read_jpeg(&[(0xFFE1, app1), (0xFFED, app13)]);
    assert_eq!(
        md.get("IPTC:Headline"),
        Some(&TagValue::String("Combined".into()))
    );
    assert_eq!(
        md.get("FLIR:CameraModel"),
        Some(&TagValue::String("FLIRX1".into()))
    );
}
