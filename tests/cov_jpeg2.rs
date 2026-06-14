//! Coverage tests for JPEG APP-segment + XMP parsers.
//!
//! Targets the REMAINING uncovered paths after wave 1 in:
//!   - src/parsers/jpeg/iptc_record2.rs  (IPTC IIM Record 2 datasets)
//!   - src/parsers/jpeg/mpf_parser.rs    (Multi-Picture Format APP2)
//!   - src/parsers/jpeg/app_parsers.rs   (APP0/APP2/APP8/APP10/APP11/APP12/APP14/COM/DQT/SOF)
//!   - src/parsers/xmp/rdf_parser.rs     (RDF/XML: Bag/Seq/Alt, qualifiers, formatters)
//!
//! Every parser here is reachable through the public API. The strategy is to
//! build small, focused synthetic byte buffers that aim at specific
//! less-common tag IDs, error/malformed branches, optional structures, and the
//! many `decode_*`/`format_*` value formatters that wave 1 did not exercise.

#[path = "common/mod.rs"]
mod common;

#[allow(unused_imports)]
use common::TestReader;

use oxidex::core::{FileReader, MetadataMap};
use oxidex::parsers::jpeg::app_parsers::{
    estimate_quality_from_dqt, parse_activephoto_segment, parse_adobe_segment, parse_app0_extended,
    parse_comment_segment, parse_ducky_segment, parse_icc_profile_segment, parse_jpeg_hdr_segment,
    parse_jpeg_ls_segment, parse_sof_segment, parse_spiff_segment,
};
use oxidex::parsers::jpeg::iptc_record2::parse_iptc_record2;
use oxidex::parsers::jpeg::mpf_parser::parse_mpf_segment;
use oxidex::parsers::xmp::rdf_parser::parse_xmp;

// ===========================================================================
// Helpers
// ===========================================================================

/// Builds a single IPTC IIM dataset:
///   [0x1C][record][dataset][len_hi][len_lo][payload...]
fn iptc_dataset(record: u8, dataset: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1C, record, dataset];
    out.push((payload.len() >> 8) as u8);
    out.push((payload.len() & 0xFF) as u8);
    out.extend_from_slice(payload);
    out
}

/// Sanity check that TestReader still satisfies FileReader (template requirement).
#[test]
fn test_test_reader_satisfies_filereader() {
    let reader = TestReader::new(vec![9, 8, 7]);
    assert_eq!(reader.size(), 3);
    assert_eq!(reader.read(1, 2).unwrap(), &[8, 7]);
    assert!(reader.read(2, 5).is_err());
}

// ===========================================================================
// IPTC Record 2: many less-common datasets + formatters + malformed input
// ===========================================================================

#[test]
fn test_iptc_record2_editorial_string_datasets() {
    // A wide spread of Record-2 string datasets that wave 1 did not touch.
    let mut data = Vec::new();
    data.extend(iptc_dataset(2, 7, b"OK")); // EditStatus
    data.extend(iptc_dataset(2, 12, b"subj")); // Subject
    data.extend(iptc_dataset(2, 15, b"News")); // Category
    data.extend(iptc_dataset(2, 20, b"Sports")); // SupplementalCategories
    data.extend(iptc_dataset(2, 22, b"FIX-1")); // FixtureIdentifier
    data.extend(iptc_dataset(2, 25, b"sunset")); // Keywords
    data.extend(iptc_dataset(2, 26, b"US")); // LocationCode
    data.extend(iptc_dataset(2, 27, b"Denver")); // LocationName
    data.extend(iptc_dataset(2, 40, b"Handle with care")); // SpecialInstructions
    data.extend(iptc_dataset(2, 42, b"01")); // ActionAdvised
    data.extend(iptc_dataset(2, 45, b"svc")); // ReferenceService
    data.extend(iptc_dataset(2, 50, b"REF-99")); // ReferenceNumber
    data.extend(iptc_dataset(2, 65, b"OxiDex")); // OriginatingProgram
    data.extend(iptc_dataset(2, 70, b"1.0")); // ProgramVersion
    data.extend(iptc_dataset(2, 85, b"Photographer")); // ByLineTitle
    data.extend(iptc_dataset(2, 92, b"Downtown")); // SubLocation
    data.extend(iptc_dataset(2, 95, b"Colorado")); // Province-State
    data.extend(iptc_dataset(2, 100, b"USA")); // Country-PrimaryLocationCode
    data.extend(iptc_dataset(2, 101, b"United States")); // Country-PrimaryLocationName
    data.extend(iptc_dataset(2, 103, b"TX-123")); // OriginalTransmissionReference
    data.extend(iptc_dataset(2, 110, b"AP")); // Credit
    data.extend(iptc_dataset(2, 115, b"Reuters")); // Source
    data.extend(iptc_dataset(2, 118, b"newsdesk@example.com")); // Contact
    data.extend(iptc_dataset(2, 122, b"Editor Name")); // Writer-Editor
    data.extend(iptc_dataset(2, 130, b"Photo")); // ImageType
    data.extend(iptc_dataset(2, 131, b"L")); // ImageOrientation
    data.extend(iptc_dataset(2, 135, b"en")); // LanguageIdentifier
    data.extend(iptc_dataset(2, 150, b"M")); // AudioType
    data.extend(iptc_dataset(2, 152, b"end cue")); // AudioOutcue
    data.extend(iptc_dataset(2, 160, b"NTSC")); // VideoType

    let md = parse_iptc_record2(&data);
    assert_eq!(md.get_string("IPTC:EditStatus"), Some("OK"));
    assert_eq!(md.get_string("IPTC:Subject"), Some("subj"));
    assert_eq!(md.get_string("IPTC:Category"), Some("News"));
    assert_eq!(md.get_string("IPTC:SupplementalCategories"), Some("Sports"));
    assert_eq!(md.get_string("IPTC:FixtureIdentifier"), Some("FIX-1"));
    assert_eq!(md.get_string("IPTC:Keywords"), Some("sunset"));
    assert_eq!(md.get_string("IPTC:LocationCode"), Some("US"));
    assert_eq!(md.get_string("IPTC:LocationName"), Some("Denver"));
    assert_eq!(
        md.get_string("IPTC:SpecialInstructions"),
        Some("Handle with care")
    );
    assert_eq!(md.get_string("IPTC:ActionAdvised"), Some("01"));
    assert_eq!(md.get_string("IPTC:ReferenceService"), Some("svc"));
    assert_eq!(md.get_string("IPTC:ReferenceNumber"), Some("REF-99"));
    assert_eq!(md.get_string("IPTC:OriginatingProgram"), Some("OxiDex"));
    assert_eq!(md.get_string("IPTC:ProgramVersion"), Some("1.0"));
    assert_eq!(md.get_string("IPTC:By-lineTitle"), Some("Photographer"));
    assert_eq!(md.get_string("IPTC:SubLocation"), Some("Downtown"));
    assert_eq!(md.get_string("IPTC:Province-State"), Some("Colorado"));
    assert_eq!(
        md.get_string("IPTC:Country-PrimaryLocationCode"),
        Some("USA")
    );
    assert_eq!(
        md.get_string("IPTC:Country-PrimaryLocationName"),
        Some("United States")
    );
    assert_eq!(
        md.get_string("IPTC:OriginalTransmissionReference"),
        Some("TX-123")
    );
    assert_eq!(md.get_string("IPTC:Credit"), Some("AP"));
    assert_eq!(md.get_string("IPTC:Source"), Some("Reuters"));
    assert_eq!(md.get_string("IPTC:Contact"), Some("newsdesk@example.com"));
    assert_eq!(md.get_string("IPTC:Writer-Editor"), Some("Editor Name"));
    assert_eq!(md.get_string("IPTC:ImageType"), Some("Photo"));
    assert_eq!(md.get_string("IPTC:ImageOrientation"), Some("L"));
    assert_eq!(md.get_string("IPTC:LanguageIdentifier"), Some("en"));
    assert_eq!(md.get_string("IPTC:AudioType"), Some("M"));
    assert_eq!(md.get_string("IPTC:AudioOutcue"), Some("end cue"));
    assert_eq!(md.get_string("IPTC:VideoType"), Some("NTSC"));
}

#[test]
fn test_iptc_record2_date_and_time_formatters() {
    // Drive all the date/time formatting branches (format_iptc_date / _time).
    let mut data = Vec::new();
    data.extend(iptc_dataset(2, 30, b"20240101")); // ReleaseDate
    data.extend(iptc_dataset(2, 35, b"120000+0000")); // ReleaseTime
    data.extend(iptc_dataset(2, 37, b"20241231")); // ExpirationDate
    data.extend(iptc_dataset(2, 38, b"235959")); // ExpirationTime
    data.extend(iptc_dataset(2, 47, b"20230615")); // ReferenceDate
    data.extend(iptc_dataset(2, 55, b"20040809")); // DateCreated
    data.extend(iptc_dataset(2, 60, b"083000")); // TimeCreated
    data.extend(iptc_dataset(2, 62, b"20050102")); // DigitalCreationDate
    data.extend(iptc_dataset(2, 63, b"091500")); // DigitalCreationTime

    let md = parse_iptc_record2(&data);
    assert!(md.get_string("IPTC:ReleaseDate").is_some());
    assert!(md.get_string("IPTC:ReleaseTime").is_some());
    assert!(md.get_string("IPTC:ExpirationDate").is_some());
    assert!(md.get_string("IPTC:ExpirationTime").is_some());
    assert!(md.get_string("IPTC:ReferenceDate").is_some());
    assert!(md.get_string("IPTC:DateCreated").is_some());
    assert!(md.get_string("IPTC:TimeCreated").is_some());
    assert!(md.get_string("IPTC:DigitalCreationDate").is_some());
    assert!(md.get_string("IPTC:DigitalCreationTime").is_some());
}

#[test]
fn test_iptc_record2_binary_and_char_datasets() {
    // EditorialUpdate (single byte), ObjectCycle (single char), AudioDuration
    // (2-byte BE integer), and ApplicationRecordVersion (2-byte BE integer).
    let mut data = Vec::new();
    data.extend(iptc_dataset(2, 0, &[0x00, 0x04])); // ApplicationRecordVersion = 4
    data.extend(iptc_dataset(2, 8, &[0x07])); // EditorialUpdate = 7
    data.extend(iptc_dataset(2, 75, b"a")); // ObjectCycle = 'a'
    data.extend(iptc_dataset(2, 151, &[0x01, 0x2C])); // AudioDuration = 300

    let md = parse_iptc_record2(&data);
    assert_eq!(md.get_integer("IPTC:ApplicationRecordVersion"), Some(4));
    assert_eq!(md.get_integer("IPTC:EditorialUpdate"), Some(7));
    assert_eq!(md.get_string("IPTC:ObjectCycle"), Some("a"));
    assert_eq!(md.get_integer("IPTC:AudioDuration"), Some(300));
}

#[test]
fn test_iptc_record2_unknown_dataset_generic_name() {
    // Dataset 200 isn't in the table -> generic "IPTC:Application-200".
    let data = iptc_dataset(2, 200, b"custom-value");
    let md = parse_iptc_record2(&data);
    assert_eq!(md.get_string("IPTC:Application-200"), Some("custom-value"));
}

#[test]
fn test_iptc_record2_latin1_fallback() {
    // Bytes >= 128 that are not valid UTF-8 trigger the Latin-1 fallback decoder.
    let payload = vec![b'C', b'a', b'f', 0xE9]; // "Caf" + é in Latin-1 (invalid UTF-8)
    let data = iptc_dataset(2, 5, &payload);
    let md = parse_iptc_record2(&data);
    let name = md.get_string("IPTC:ObjectName").unwrap();
    assert!(name.starts_with("Caf"));
}

#[test]
fn test_iptc_record2_extended_length_skipped() {
    // length_high has bit 15 set -> extended format branch -> dataset skipped.
    // [0x1C][2][5][0x80][0x00] then trailing bytes; parser advances by 5 and
    // continues. We append a normal dataset after to confirm parsing resumes.
    let mut data = vec![0x1C, 2, 5, 0x80, 0x00];
    data.extend(iptc_dataset(2, 90, b"Paris")); // City after the skipped extended entry
    let md = parse_iptc_record2(&data);
    // The extended ObjectName entry was skipped; City should still parse.
    assert_eq!(md.get_string("IPTC:City"), Some("Paris"));
}

#[test]
fn test_iptc_record2_truncated_payload_stops() {
    // Claims a 100-byte payload but provides far fewer bytes -> truncated -> stop.
    let data = vec![0x1C, 2, 5, 0x00, 100, b'h', b'i'];
    let md = parse_iptc_record2(&data);
    assert!(md.get("IPTC:ObjectName").is_none());
}

#[test]
fn test_iptc_record2_bad_marker_breaks() {
    // First byte isn't 0x1C -> loop breaks immediately, empty map.
    let data = vec![0x00, 2, 5, 0x00, 0x02, b'h', b'i'];
    let md = parse_iptc_record2(&data);
    assert!(md.is_empty());
}

#[test]
fn test_iptc_record2_non_record2_ignored() {
    // Record 1 dataset must be ignored; record 2 still parsed.
    let mut data = Vec::new();
    data.extend(iptc_dataset(1, 0, &[0x00, 0x04])); // Record 1 ignored
    data.extend(iptc_dataset(2, 5, b"Title")); // Record 2 parsed
    let md = parse_iptc_record2(&data);
    assert_eq!(md.get_string("IPTC:ObjectName"), Some("Title"));
}

#[test]
fn test_iptc_record2_empty_input() {
    let md = parse_iptc_record2(&[]);
    assert!(md.is_empty());
}

// ===========================================================================
// MPF parser: less-common tags, attribute IFD, generic IFD value branches,
// big-endian, malformed input
// ===========================================================================

/// Helpers to push little-endian primitives into an MPF builder.
fn mpf_push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}
fn mpf_push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

#[test]
fn test_mpf_index_ifd_uncommon_tags() {
    // ImageUIDList (0xB003) and TotalFrames (0xB004) plus an unknown tag id that
    // routes through parse_generic_ifd_value (LONG inline).
    let mut data = Vec::new();
    data.extend_from_slice(b"MPF\0");
    data.extend_from_slice(b"II");
    mpf_push_u16(&mut data, 42);
    mpf_push_u32(&mut data, 8); // IFD offset

    // 3 entries
    mpf_push_u16(&mut data, 3);

    // ImageUIDList (0xB003), UNDEFINED, count=66
    mpf_push_u16(&mut data, 0xB003);
    mpf_push_u16(&mut data, 7);
    mpf_push_u32(&mut data, 66);
    mpf_push_u32(&mut data, 0); // offset (unused for this tag)

    // TotalFrames (0xB004), LONG, count=1, value=4
    mpf_push_u16(&mut data, 0xB004);
    mpf_push_u16(&mut data, 4);
    mpf_push_u32(&mut data, 1);
    mpf_push_u32(&mut data, 4);

    // Unknown tag 0xB0FF, LONG inline value 7 -> generic "MPF:0xB0FF"
    mpf_push_u16(&mut data, 0xB0FF);
    mpf_push_u16(&mut data, 4);
    mpf_push_u32(&mut data, 1);
    mpf_push_u32(&mut data, 7);

    // next IFD = 0
    mpf_push_u32(&mut data, 0);

    let mut md = MetadataMap::new();
    let res = parse_mpf_segment(&data, &mut md);
    assert!(res.is_ok(), "MPF parse failed: {:?}", res);
    assert!(md.contains_key("MPF:ImageUIDList"));
    assert_eq!(md.get_integer("MPF:TotalFrames"), Some(4));
    assert_eq!(md.get_integer("MPF:0xB0FF"), Some(7));
}

#[test]
fn test_mpf_attribute_ifd_via_next_ifd_pointer() {
    // Build an MP Index IFD with a non-zero next-IFD pointer that points at an
    // MP Attribute IFD carrying many of the 3D/panorama tags. This exercises
    // parse_mp_attribute_ifd and the generic IFD value parser for SHORT/LONG.
    let mut data = Vec::new();
    data.extend_from_slice(b"MPF\0");
    data.extend_from_slice(b"II");
    mpf_push_u16(&mut data, 42);
    mpf_push_u32(&mut data, 8); // index IFD offset (tiff-relative)

    // --- MP Index IFD at tiff offset 8 ---
    // 1 entry (NumberOfImages) then next-IFD pointer to the attribute IFD.
    mpf_push_u16(&mut data, 1);
    mpf_push_u16(&mut data, 0xB001); // NumberOfImages
    mpf_push_u16(&mut data, 4); // LONG
    mpf_push_u32(&mut data, 1);
    mpf_push_u32(&mut data, 2);

    // The attribute IFD will start right after this next-IFD pointer.
    // tiff offset of next-IFD pointer = 8 + 2 + 12 = 22; pointer itself is 4 bytes.
    // So attribute IFD begins at tiff offset 26.
    mpf_push_u32(&mut data, 26);

    // --- MP Attribute IFD at tiff offset 26 ---
    let attr_tags: [u16; 8] = [
        0xB101, // MPIndividualNum
        0xB201, // PanOrientation
        0xB204, // BaseViewpointNum
        0xB208, // AxisDistanceX
        0xB20B, // YawAngle
        0xB20C, // PitchAngle
        0xB20D, // RollAngle
        0xBEEF, // unknown -> generic name
    ];
    mpf_push_u16(&mut data, attr_tags.len() as u16);
    for (i, tag) in attr_tags.iter().enumerate() {
        mpf_push_u16(&mut data, *tag);
        mpf_push_u16(&mut data, 4); // LONG inline
        mpf_push_u32(&mut data, 1);
        mpf_push_u32(&mut data, (i as u32) + 1);
    }
    mpf_push_u32(&mut data, 0); // next IFD = 0

    let mut md = MetadataMap::new();
    let res = parse_mpf_segment(&data, &mut md);
    assert!(res.is_ok(), "MPF attribute parse failed: {:?}", res);
    assert_eq!(md.get_integer("MPF:NumberOfImages"), Some(2));
    assert!(md.contains_key("MPF:MPIndividualNum"));
    assert!(md.contains_key("MPF:PanOrientation"));
    assert!(md.contains_key("MPF:BaseViewpointNum"));
    assert!(md.contains_key("MPF:AxisDistanceX"));
    assert!(md.contains_key("MPF:YawAngle"));
    assert!(md.contains_key("MPF:PitchAngle"));
    assert!(md.contains_key("MPF:RollAngle"));
    assert!(md.contains_key("MPF:0xBEEF"));
}

#[test]
fn test_mpf_attribute_ifd_rational_and_ascii_offset_values() {
    // Attribute IFD with a RATIONAL (5), an SRATIONAL (10), and an offset ASCII
    // (2) value to drive the at-offset arms of parse_generic_ifd_value.
    let mut data = Vec::new();
    data.extend_from_slice(b"MPF\0");
    data.extend_from_slice(b"II");
    mpf_push_u16(&mut data, 42);
    mpf_push_u32(&mut data, 8); // index IFD offset

    // Index IFD: 0 entries, next IFD pointer -> attribute IFD.
    mpf_push_u16(&mut data, 0);
    // next-IFD pointer at tiff offset 10; attribute IFD will begin at offset 14.
    mpf_push_u32(&mut data, 14);

    // Attribute IFD at tiff offset 14: 3 entries.
    mpf_push_u16(&mut data, 3);

    // We need offsets for the out-of-line values. Compute layout:
    // Attribute IFD header: 2 (count) + 3*12 (entries) + 4 (next) = 42 bytes.
    // Starts at tiff offset 14 -> trailing data begins at tiff offset 56.
    let trailing_start: u32 = 56;
    let rational_off = trailing_start; // 8 bytes: num=3, den=4
    let srational_off = trailing_start + 8; // 8 bytes: num=-5, den=2
    let ascii_off = trailing_start + 16; // 6 bytes "test\0"

    // ConvergenceAngle (0xB205) RATIONAL at offset
    mpf_push_u16(&mut data, 0xB205);
    mpf_push_u16(&mut data, 5);
    mpf_push_u32(&mut data, 1);
    mpf_push_u32(&mut data, rational_off);

    // ColorCompFilter-like SRATIONAL via unknown tag 0xB210
    mpf_push_u16(&mut data, 0xB210);
    mpf_push_u16(&mut data, 10);
    mpf_push_u32(&mut data, 1);
    mpf_push_u32(&mut data, srational_off);

    // ASCII (2) value at offset, count 5 -> string branch
    mpf_push_u16(&mut data, 0xB211);
    mpf_push_u16(&mut data, 2);
    mpf_push_u32(&mut data, 5);
    mpf_push_u32(&mut data, ascii_off);

    mpf_push_u32(&mut data, 0); // next IFD = 0

    // Trailing data (tiff-relative offsets above are relative to "II"):
    // pad so trailing actually begins at tiff offset 56. Current tiff length:
    // 8 (header) + 6 (index ifd) + [2 + 36 + 4] (attr ifd) = 8+6+42 = 56. Good.
    // RATIONAL num=3 den=4
    mpf_push_u32(&mut data, 3);
    mpf_push_u32(&mut data, 4);
    // SRATIONAL num=-5 den=2
    data.extend_from_slice(&(-5i32).to_le_bytes());
    data.extend_from_slice(&2i32.to_le_bytes());
    // ASCII "test\0"
    data.extend_from_slice(b"test\0");

    let mut md = MetadataMap::new();
    let res = parse_mpf_segment(&data, &mut md);
    assert!(res.is_ok(), "MPF rational parse failed: {:?}", res);
    assert!(md.contains_key("MPF:ConvergenceAngle"));
    assert!(md.contains_key("MPF:0xB210"));
    assert!(md.contains_key("MPF:0xB211"));
}

#[test]
fn test_mpf_version_at_offset() {
    // MPFVersion with count > 4 forces the at-offset version-string branch.
    let mut data = Vec::new();
    data.extend_from_slice(b"MPF\0");
    data.extend_from_slice(b"II");
    mpf_push_u16(&mut data, 42);
    mpf_push_u32(&mut data, 8);

    mpf_push_u16(&mut data, 1); // 1 entry
    // MPFVersion (0xB000), ASCII, count=8 -> value at offset
    mpf_push_u16(&mut data, 0xB000);
    mpf_push_u16(&mut data, 2);
    mpf_push_u32(&mut data, 8);
    // header so far: 8 (tiff) + 2 (count) + 12 (entry) + 4 (next) = 26 -> version bytes at tiff offset 26
    mpf_push_u32(&mut data, 26);
    mpf_push_u32(&mut data, 0); // next IFD
    data.extend_from_slice(b"01000000"); // 8 bytes; first 4 used

    let mut md = MetadataMap::new();
    let res = parse_mpf_segment(&data, &mut md);
    assert!(res.is_ok(), "MPF version-at-offset failed: {:?}", res);
    assert!(md.contains_key("MPF:MPFVersion"));
}

#[test]
fn test_mpf_entry_array_dependent_images_and_formats() {
    // MP Entry array with a dependent-parent image and a panorama type to drive
    // the dep_flag/representative/format/type decode branches plus the generic
    // "last non-primary image" tags.
    let mut data = Vec::new();
    data.extend_from_slice(b"MPF\0");
    data.extend_from_slice(b"II");
    mpf_push_u16(&mut data, 42);
    mpf_push_u32(&mut data, 8);

    mpf_push_u16(&mut data, 2); // 2 entries

    // NumberOfImages = 2
    mpf_push_u16(&mut data, 0xB001);
    mpf_push_u16(&mut data, 4);
    mpf_push_u32(&mut data, 1);
    mpf_push_u32(&mut data, 2);

    // MPEntry (0xB002), UNDEFINED, count=32 (2*16), offset after header
    let mp_entry_off = 8 + 2 + (2 * 12) + 4; // tiff offset of entry array
    mpf_push_u16(&mut data, 0xB002);
    mpf_push_u16(&mut data, 7);
    mpf_push_u32(&mut data, 32);
    mpf_push_u32(&mut data, mp_entry_off as u32);

    mpf_push_u32(&mut data, 0); // next IFD = 0

    // Entry 1: dependent-parent (bits 31-30 = 01), representative, JPEG,
    // type = Multi-Frame Panorama (0x020001).
    let attr1: u32 = (1 << 30) | (1 << 29) | 0x020001;
    mpf_push_u32(&mut data, attr1);
    mpf_push_u32(&mut data, 111_111); // size
    mpf_push_u32(&mut data, 0); // offset
    mpf_push_u16(&mut data, 2); // dependent image 1
    mpf_push_u16(&mut data, 0); // dependent image 2

    // Entry 2: dependent-child (bits = 10), type = Multi-Angle (0x020003).
    let attr2: u32 = (2 << 30) | 0x020003;
    mpf_push_u32(&mut data, attr2);
    mpf_push_u32(&mut data, 22_222); // size
    mpf_push_u32(&mut data, 111_111); // offset
    mpf_push_u16(&mut data, 0); // dependent image 1
    mpf_push_u16(&mut data, 1); // dependent image 2

    let mut md = MetadataMap::new();
    let res = parse_mpf_segment(&data, &mut md);
    assert!(res.is_ok(), "MPF entry parse failed: {:?}", res);
    assert_eq!(md.get_string("MPF:MPImage1Flags"), Some("Dependent parent"));
    assert_eq!(md.get_string("MPF:MPImage1Representative"), Some("Yes"));
    assert_eq!(md.get_string("MPF:MPImage1Format"), Some("JPEG"));
    assert!(md.contains_key("MPF:MPImage1Type"));
    assert_eq!(md.get_integer("MPF:MPImage1Size"), Some(111_111));
    assert_eq!(md.get_integer("MPF:MPImage1DependentImage1"), Some(2));
    assert_eq!(md.get_integer("MPF:MPImage2DependentImage2"), Some(1));
    // Generic last-non-primary tags (entry 2 is last non-primary).
    assert_eq!(
        md.get_string("MPF:MPImageFlags"),
        Some("Dependent child image")
    );
    assert!(md.contains_key("MPF:MPImageType"));
    assert_eq!(md.get_integer("MPF:MPImageLength"), Some(22_222));
}

#[test]
fn test_mpf_big_endian_index() {
    // Big-endian ("MM") index IFD with ImageUIDList + NumberOfImages.
    let mut data = Vec::new();
    data.extend_from_slice(b"MPF\0");
    data.extend_from_slice(b"MM");
    data.extend_from_slice(&42u16.to_be_bytes());
    data.extend_from_slice(&8u32.to_be_bytes());

    data.extend_from_slice(&1u16.to_be_bytes()); // 1 entry
    data.extend_from_slice(&0xB001u16.to_be_bytes()); // NumberOfImages
    data.extend_from_slice(&4u16.to_be_bytes());
    data.extend_from_slice(&1u32.to_be_bytes());
    data.extend_from_slice(&5u32.to_be_bytes());
    data.extend_from_slice(&0u32.to_be_bytes()); // next IFD

    let mut md = MetadataMap::new();
    let res = parse_mpf_segment(&data, &mut md);
    assert!(res.is_ok());
    assert_eq!(md.get_integer("MPF:NumberOfImages"), Some(5));
}

#[test]
fn test_mpf_error_paths() {
    let mut md = MetadataMap::new();

    // Too short.
    assert!(parse_mpf_segment(b"MPF\0II*", &mut md).is_err());

    // Wrong identifier.
    let mut md2 = MetadataMap::new();
    let bad = b"XXX\0II*\0\x08\0\0\0".to_vec();
    assert!(parse_mpf_segment(&bad, &mut md2).is_err());

    // Bad byte-order marker.
    let mut md3 = MetadataMap::new();
    let mut bad_bo = Vec::new();
    bad_bo.extend_from_slice(b"MPF\0");
    bad_bo.extend_from_slice(b"ZZ");
    bad_bo.extend_from_slice(&[0u8; 8]);
    assert!(parse_mpf_segment(&bad_bo, &mut md3).is_err());

    // Bad magic number (not 42).
    let mut md4 = MetadataMap::new();
    let mut bad_magic = Vec::new();
    bad_magic.extend_from_slice(b"MPF\0");
    bad_magic.extend_from_slice(b"II");
    bad_magic.extend_from_slice(&99u16.to_le_bytes());
    bad_magic.extend_from_slice(&8u32.to_le_bytes());
    bad_magic.extend_from_slice(&[0u8; 4]);
    assert!(parse_mpf_segment(&bad_magic, &mut md4).is_err());
}

// ===========================================================================
// app_parsers: less-common segments + error branches
// ===========================================================================

#[test]
fn test_icc_profile_classes_and_errors() {
    // Build first-segment ICC profile with each profile class to drive the match.
    for (class, expect) in [
        (b"scnr", "Input Device Profile"),
        (b"prtr", "Output Device Profile"),
        (b"link", "DeviceLink Profile"),
        (b"spac", "ColorSpace Conversion Profile"),
        (b"abst", "Abstract Profile"),
        (b"nmcl", "Named Color Profile"),
    ] {
        let mut data = Vec::new();
        data.extend_from_slice(b"ICC_PROFILE\0");
        data.push(1); // sequence 1
        data.push(1); // total 1
        let mut header = vec![0u8; 128];
        header[0..4].copy_from_slice(&[0x00, 0x00, 0x01, 0x00]); // size
        header[8] = 0x02; // version major
        header[9] = 0x10; // version minor nibble
        header[12..16].copy_from_slice(class);
        header[16..20].copy_from_slice(b"GRAY");
        data.extend_from_slice(&header);

        let mut md = MetadataMap::new();
        assert!(parse_icc_profile_segment(&data, &mut md).is_ok());
        assert_eq!(md.get_string("ICC_Profile:ProfileClass"), Some(expect));
        assert_eq!(md.get_string("ICC_Profile:ColorSpace"), Some("GRAY"));
        assert!(md.contains_key("ICC_Profile:ProfileSize"));
        assert!(md.contains_key("ICC_Profile:ProfileVersion"));
    }

    // Non-first segment (sequence 2) skips the header extraction branch.
    let mut multi = Vec::new();
    multi.extend_from_slice(b"ICC_PROFILE\0");
    multi.push(2); // sequence 2 of 3
    multi.push(3);
    multi.extend_from_slice(&[0u8; 4]);
    let mut md2 = MetadataMap::new();
    assert!(parse_icc_profile_segment(&multi, &mut md2).is_ok());
    assert_eq!(
        md2.get_string("ICC_Profile:ProfileSequence"),
        Some("2 of 3")
    );

    // Errors: too short, wrong identifier.
    let mut md3 = MetadataMap::new();
    assert!(parse_icc_profile_segment(b"short", &mut md3).is_err());
    let mut md4 = MetadataMap::new();
    assert!(parse_icc_profile_segment(b"NOTICCPROFILE\0", &mut md4).is_err());
}

#[test]
fn test_ducky_segment_all_tags() {
    // Quality (0x0001), Comment (0x0002), Copyright (0x0003), and an unknown tag.
    let mut data = Vec::new();
    data.extend_from_slice(b"Ducky");
    // Quality tag
    data.extend_from_slice(&0x0001u16.to_be_bytes());
    data.extend_from_slice(&4u16.to_be_bytes());
    data.extend_from_slice(&90i32.to_be_bytes());
    // Comment tag
    let comment = b"hello";
    data.extend_from_slice(&0x0002u16.to_be_bytes());
    data.extend_from_slice(&(comment.len() as u16).to_be_bytes());
    data.extend_from_slice(comment);
    // Copyright tag
    let copyright = b"(c) 2024";
    data.extend_from_slice(&0x0003u16.to_be_bytes());
    data.extend_from_slice(&(copyright.len() as u16).to_be_bytes());
    data.extend_from_slice(copyright);
    // Unknown tag 0x00FF
    let unk = b"\x01\x02";
    data.extend_from_slice(&0x00FFu16.to_be_bytes());
    data.extend_from_slice(&(unk.len() as u16).to_be_bytes());
    data.extend_from_slice(unk);

    let mut md = MetadataMap::new();
    assert!(parse_ducky_segment(&data, &mut md).is_ok());
    assert_eq!(md.get_integer("Ducky:Quality"), Some(90));
    assert_eq!(md.get_string("Ducky:Comment"), Some("hello"));
    assert_eq!(md.get_string("Ducky:Copyright"), Some("(c) 2024"));
    assert!(md.contains_key("Ducky:Tag_00FF"));

    // Error branches.
    let mut e1 = MetadataMap::new();
    assert!(parse_ducky_segment(b"Duc", &mut e1).is_err());
    let mut e2 = MetadataMap::new();
    assert!(parse_ducky_segment(b"NotDuck", &mut e2).is_err());
}

#[test]
fn test_adobe_segment_color_transforms() {
    for (transform, expect) in [
        (0u8, "Unknown (RGB or CMYK)"),
        (1, "YCbCr"),
        (2, "YCCK"),
        (9, "Unknown"),
    ] {
        let data = [
            b'A', b'd', b'o', b'b', b'e', 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, transform,
        ];
        let mut md = MetadataMap::new();
        assert!(parse_adobe_segment(&data, &mut md).is_ok());
        assert_eq!(md.get_string("Adobe:ColorTransform"), Some(expect));
        assert_eq!(md.get_integer("Adobe:DCTEncodeVersion"), Some(100));
    }

    // Errors.
    let mut e1 = MetadataMap::new();
    assert!(parse_adobe_segment(b"Adob", &mut e1).is_err());
    let mut e2 = MetadataMap::new();
    assert!(parse_adobe_segment(b"NotAdobe1234", &mut e2).is_err());
}

#[test]
fn test_comment_segment_utf8_and_binary() {
    let mut md = MetadataMap::new();
    assert!(parse_comment_segment(b"plain text", &mut md).is_ok());
    assert_eq!(md.get_string("JPEG:Comment"), Some("plain text"));

    // Invalid UTF-8 -> binary branch.
    let mut md2 = MetadataMap::new();
    assert!(parse_comment_segment(&[0xFF, 0xFE, 0x00], &mut md2).is_ok());
    assert!(md2.contains_key("JPEG:Comment"));
}

#[test]
fn test_dqt_quality_estimation_branches() {
    // High quality (avg <= 10).
    let mut high = vec![0x00];
    high.extend(vec![3u8; 64]);
    let mut md_h = MetadataMap::new();
    assert!(estimate_quality_from_dqt(&high, &mut md_h).is_ok());
    assert!(md_h.get_integer("JPEG:EstimatedQuality").unwrap() > 90);

    // Mid quality (10 < avg <= 50).
    let mut mid = vec![0x00];
    mid.extend(vec![30u8; 64]);
    let mut md_m = MetadataMap::new();
    assert!(estimate_quality_from_dqt(&mid, &mut md_m).is_ok());
    assert!(md_m.contains_key("JPEG:EstimatedQuality"));

    // Low quality (avg > 50).
    let mut low = vec![0x00];
    low.extend(vec![120u8; 64]);
    let mut md_l = MetadataMap::new();
    assert!(estimate_quality_from_dqt(&low, &mut md_l).is_ok());
    let q = md_l.get_integer("JPEG:EstimatedQuality").unwrap();
    assert!((1..=100).contains(&q));

    // Errors: empty + too short.
    let mut e1 = MetadataMap::new();
    assert!(estimate_quality_from_dqt(&[], &mut e1).is_err());
    let mut e2 = MetadataMap::new();
    assert!(estimate_quality_from_dqt(&[0x00, 0x01], &mut e2).is_err());
}

#[test]
fn test_app0_jfif_and_jfxx_variants() {
    // JFIF with thumbnail dimensions.
    let mut jfif = Vec::new();
    jfif.extend_from_slice(b"JFIF\x00");
    jfif.extend_from_slice(&[0u8; 9]); // pad to 14 bytes total minus what we add
    // Ensure length >= 14 with thumbnail w/h at 12,13.
    while jfif.len() < 14 {
        jfif.push(0);
    }
    jfif[12] = 4; // thumbnail width
    jfif[13] = 4; // thumbnail height
    let mut md = MetadataMap::new();
    assert!(parse_app0_extended(&jfif, &mut md).is_ok());
    assert_eq!(md.get_string("JFIF:HasThumbnail"), Some("Yes"));

    // JFXX extension variants.
    for (code, expect) in [
        (0x10u8, "Thumbnail JPEG"),
        (0x11, "Thumbnail 1 byte/pixel"),
        (0x13, "Thumbnail 3 bytes/pixel"),
        (0x99, "Unknown"),
    ] {
        let mut jfxx = Vec::new();
        jfxx.extend_from_slice(b"JFXX\x00");
        jfxx.push(code);
        let mut m = MetadataMap::new();
        assert!(parse_app0_extended(&jfxx, &mut m).is_ok());
        assert_eq!(m.get_string("JFIF:ThumbnailType"), Some(expect));
    }

    // Errors: too short, and an unrecognized identifier.
    let mut e1 = MetadataMap::new();
    assert!(parse_app0_extended(b"JF", &mut e1).is_err());
    let mut e2 = MetadataMap::new();
    assert!(parse_app0_extended(b"OTHER", &mut e2).is_err());
}

#[test]
fn test_sof_progressive_and_grayscale_and_subsampling() {
    // Progressive marker + 1 component (grayscale) -> no YCbCrSubSampling branch.
    let gray = [
        0x08, // precision
        0x00, 0xC8, // height 200
        0x00, 0xFA, // width 250
        0x01, // 1 component
        0x01, 0x11, 0x00, // Y, sampling 1x1
    ];
    let mut md = MetadataMap::new();
    assert!(parse_sof_segment(0xFFC2, &gray, &mut md).is_ok());
    assert_eq!(
        md.get_string("File:EncodingProcess"),
        Some("Progressive DCT, Huffman coding")
    );
    assert_eq!(md.get_integer("File:ColorComponents"), Some(1));
    assert!(md.contains_key("JPEG:SamplingFactors"));

    // 3 components with 2x1 sampling -> 4:2:2 subsampling branch + arithmetic marker.
    let ycbcr = [
        0x08, 0x01, 0x00, 0x01, 0x40, 0x03, // 3 components
        0x01, 0x21, 0x00, // Y 2x1
        0x02, 0x11, 0x01, // Cb 1x1
        0x03, 0x11, 0x01, // Cr 1x1
    ];
    let mut md2 = MetadataMap::new();
    assert!(parse_sof_segment(0xFFCA, &ycbcr, &mut md2).is_ok());
    assert_eq!(
        md2.get_string("File:EncodingProcess"),
        Some("Progressive DCT, Arithmetic coding")
    );
    assert_eq!(
        md2.get_string("File:YCbCrSubSampling"),
        Some("YCbCr4:2:2 (2 1)")
    );

    // Unknown marker -> "Unknown" encoding; also 4:4:4 subsampling (1x1).
    let normal = [
        0x08, 0x00, 0x10, 0x00, 0x10, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01,
    ];
    let mut md3 = MetadataMap::new();
    assert!(parse_sof_segment(0xFFDA, &normal, &mut md3).is_ok());
    assert_eq!(md3.get_string("File:EncodingProcess"), Some("Unknown"));
    assert_eq!(
        md3.get_string("File:YCbCrSubSampling"),
        Some("YCbCr4:4:4 (1 1)")
    );

    // Error: too short.
    let mut e = MetadataMap::new();
    assert!(parse_sof_segment(0xFFC0, &[0x08, 0x00], &mut e).is_err());
}

#[test]
fn test_spiff_profiles_and_errors() {
    for (profile, expect) in [
        (0u8, "Baseline"),
        (1, "Progressive"),
        (2, "Lossless"),
        (9, "Unknown"),
    ] {
        let mut data = Vec::new();
        data.extend_from_slice(b"SPIFF\0");
        data.push(2); // version major
        data.push(5); // version minor
        data.push(profile);
        data.push(1); // components
        let mut md = MetadataMap::new();
        assert!(parse_spiff_segment(&data, &mut md).is_ok());
        assert_eq!(md.get_string("APP8:SPIFFProfile"), Some(expect));
        assert_eq!(md.get_string("APP8:SPIFFVersion"), Some("2.5"));
    }

    // Errors.
    let mut e1 = MetadataMap::new();
    assert!(parse_spiff_segment(b"SPIF", &mut e1).is_err());
    let mut e2 = MetadataMap::new();
    assert!(parse_spiff_segment(b"NOTSPIFF", &mut e2).is_err());
}

#[test]
fn test_activephoto_variants() {
    // Recognized "ActiveP" prefix branch (needs len >= 20).
    let mut data = b"ActivePhoto v1 metadata!!".to_vec();
    while data.len() < 20 {
        data.push(b'x');
    }
    let mut md = MetadataMap::new();
    assert!(parse_activephoto_segment(&data, &mut md).is_ok());
    assert_eq!(md.get_string("APP10:Format"), Some("ActivePhoto"));
    assert!(md.contains_key("APP10:Version"));

    // Generic APP10 branch (no ActiveP prefix).
    let mut md2 = MetadataMap::new();
    assert!(parse_activephoto_segment(b"random app10 data", &mut md2).is_ok());
    assert!(md2.contains_key("APP10:DataSize"));

    // Empty -> error.
    let mut e = MetadataMap::new();
    assert!(parse_activephoto_segment(b"", &mut e).is_err());
}

#[test]
fn test_jpeg_hdr_rendering_intents() {
    for (intent, expect) in [
        (0u8, "Perceptual"),
        (1, "Relative Colorimetric"),
        (2, "Saturation"),
        (3, "Absolute Colorimetric"),
        (9, "Unknown"),
    ] {
        let mut data = b"HDR_RI".to_vec();
        data.push(intent);
        let mut md = MetadataMap::new();
        assert!(parse_jpeg_hdr_segment(&data, &mut md).is_ok());
        assert_eq!(md.get_string("APP11:Format"), Some("HDR_RI"));
        assert_eq!(md.get_string("APP11:RenderingIntent"), Some(expect));
    }

    // Non-HDR_RI generic branch.
    let mut md2 = MetadataMap::new();
    assert!(parse_jpeg_hdr_segment(b"other hdr data", &mut md2).is_ok());
    assert!(md2.contains_key("APP11:DataSize"));

    // Empty -> error.
    let mut e = MetadataMap::new();
    assert!(parse_jpeg_hdr_segment(b"", &mut e).is_err());
}

#[test]
fn test_jpeg_ls_markers_and_errors() {
    // Identifier branch.
    let mut md = MetadataMap::new();
    assert!(parse_jpeg_ls_segment(b"JPEGLS data", &mut md).is_ok());
    assert_eq!(md.get_string("APP15:Format"), Some("JPEG-LS"));

    // Marker-type branches.
    for (b2, expect) in [
        (0xF7u8, "SOF-LS (Start of Frame for JPEG-LS)"),
        (0xF8, "LSE (JPEG-LS Parameters Extension)"),
        (0xF9, "RES (Reserved)"),
        (0x00, "Unknown marker"),
    ] {
        let data = [0xFF, b2, 0x01, 0x02];
        let mut m = MetadataMap::new();
        assert!(parse_jpeg_ls_segment(&data, &mut m).is_ok());
        assert_eq!(m.get_string("APP15:MarkerType"), Some(expect));
        assert!(m.contains_key("APP15:DataSize"));
    }

    // Empty -> error.
    let mut e = MetadataMap::new();
    assert!(parse_jpeg_ls_segment(b"", &mut e).is_err());
}

// ===========================================================================
// XMP rdf_parser: collections, qualifiers, entities, and value formatters
// ===========================================================================

fn xmp_find<'a>(results: &'a [(String, String)], key: &str) -> Option<&'a str> {
    results
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[test]
fn test_xmp_collection_bag_seq_alt() {
    // rdf:Bag, rdf:Seq, and rdf:Alt collections become comma-joined strings.
    let xml = br#"
        <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                 xmlns:dc="http://purl.org/dc/elements/1.1/">
          <rdf:Description>
            <dc:subject>
              <rdf:Bag>
                <rdf:li>alpha</rdf:li>
                <rdf:li>beta</rdf:li>
                <rdf:li>gamma</rdf:li>
              </rdf:Bag>
            </dc:subject>
            <dc:creator>
              <rdf:Seq>
                <rdf:li>First Author</rdf:li>
                <rdf:li>Second Author</rdf:li>
              </rdf:Seq>
            </dc:creator>
            <dc:title>
              <rdf:Alt>
                <rdf:li xml:lang="x-default">Default Title</rdf:li>
                <rdf:li xml:lang="fr">Titre</rdf:li>
              </rdf:Alt>
            </dc:title>
          </rdf:Description>
        </rdf:RDF>
    "#;
    let res = parse_xmp(xml).unwrap();

    let subject = xmp_find(&res, "XMP:Subject").expect("XMP:Subject present");
    assert!(subject.contains("alpha"));
    assert!(subject.contains("beta"));
    assert!(subject.contains("gamma"));
    assert!(subject.contains(", "));

    let creator = xmp_find(&res, "XMP:Creator").expect("XMP:Creator present");
    assert!(creator.contains("First Author"));
    assert!(creator.contains("Second Author"));

    let title = xmp_find(&res, "XMP:Title").expect("XMP:Title present");
    assert!(title.contains("Default Title"));
}

#[test]
fn test_xmp_entity_references_and_escapes() {
    // GeneralRef handling for &amp; &lt; &gt; &apos; &quot; and a numeric ref.
    let xml = br#"
        <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                 xmlns:dc="http://purl.org/dc/elements/1.1/">
          <rdf:Description>
            <dc:rights>Tom &amp; Jerry &lt;x&gt; said &quot;hi&quot; &#65;</dc:rights>
          </rdf:Description>
        </rdf:RDF>
    "#;
    let res = parse_xmp(xml).unwrap();
    let rights = xmp_find(&res, "XMP:Rights").expect("XMP:Rights present");
    assert!(rights.contains('&'));
    assert!(rights.contains('<'));
    assert!(rights.contains('>'));
    assert!(rights.contains('"'));
    assert!(rights.contains('A')); // &#65;
}

#[test]
fn test_xmp_value_formatters_exif_enums() {
    // Drive the decode_xmp_* enum formatters via exif: namespaced properties.
    let xml = br#"
        <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                 xmlns:exif="http://ns.adobe.com/exif/1.0/"
                 xmlns:tiff="http://ns.adobe.com/tiff/1.0/">
          <rdf:Description>
            <exif:ColorSpace>1</exif:ColorSpace>
            <exif:CustomRendered>1</exif:CustomRendered>
            <exif:ExposureMode>2</exif:ExposureMode>
            <exif:FileSource>3</exif:FileSource>
            <exif:MeteringMode>5</exif:MeteringMode>
            <exif:SceneCaptureType>1</exif:SceneCaptureType>
            <exif:SensingMethod>2</exif:SensingMethod>
            <exif:WhiteBalance>0</exif:WhiteBalance>
            <tiff:Orientation>6</tiff:Orientation>
            <tiff:YCbCrPositioning>2</tiff:YCbCrPositioning>
            <tiff:ResolutionUnit>2</tiff:ResolutionUnit>
            <tiff:PhotometricInterpretation>6</tiff:PhotometricInterpretation>
          </rdf:Description>
        </rdf:RDF>
    "#;
    let res = parse_xmp(xml).unwrap();
    assert_eq!(xmp_find(&res, "XMP-exif:ColorSpace"), Some("sRGB"));
    assert_eq!(xmp_find(&res, "XMP-exif:CustomRendered"), Some("Custom"));
    assert_eq!(
        xmp_find(&res, "XMP-exif:ExposureMode"),
        Some("Auto bracket")
    );
    assert_eq!(
        xmp_find(&res, "XMP-exif:FileSource"),
        Some("Digital Camera")
    );
    assert_eq!(
        xmp_find(&res, "XMP-exif:MeteringMode"),
        Some("Multi-segment")
    );
    assert_eq!(
        xmp_find(&res, "XMP-exif:SceneCaptureType"),
        Some("Landscape")
    );
    assert_eq!(
        xmp_find(&res, "XMP-exif:SensingMethod"),
        Some("One-chip color area")
    );
    assert_eq!(xmp_find(&res, "XMP-exif:WhiteBalance"), Some("Auto"));
    assert_eq!(xmp_find(&res, "XMP-tiff:Orientation"), Some("Rotate 90 CW"));
    assert_eq!(
        xmp_find(&res, "XMP-tiff:YCbCrPositioning"),
        Some("Co-sited")
    );
    assert_eq!(xmp_find(&res, "XMP-tiff:ResolutionUnit"), Some("inches"));
    assert_eq!(
        xmp_find(&res, "XMP-tiff:PhotometricInterpretation"),
        Some("YCbCr")
    );
}

#[test]
fn test_xmp_value_formatters_numeric_and_photoshop() {
    // Drive the format_* numeric formatters and Photoshop/ColorMode/Urgency.
    let xml = br#"
        <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                 xmlns:exif="http://ns.adobe.com/exif/1.0/"
                 xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
                 xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
                 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
          <rdf:Description>
            <exif:ISO>400</exif:ISO>
            <exif:ShutterSpeed>1/250</exif:ShutterSpeed>
            <exif:Aperture>2.8</exif:Aperture>
            <exif:ExposureCompensation>-0.5</exif:ExposureCompensation>
            <exif:FocalLength>50.0</exif:FocalLength>
            <tiff:XResolution>72.000000</tiff:XResolution>
            <tiff:YResolution>300</tiff:YResolution>
            <photoshop:Quality>85</photoshop:Quality>
            <photoshop:ColorMode>3</photoshop:ColorMode>
            <photoshop:Urgency>8</photoshop:Urgency>
            <crs:ProcessingParameters>1.5</crs:ProcessingParameters>
          </rdf:Description>
        </rdf:RDF>
    "#;
    let res = parse_xmp(xml).unwrap();
    assert_eq!(xmp_find(&res, "XMP-exif:ISO"), Some("400"));
    assert_eq!(xmp_find(&res, "XMP-exif:ShutterSpeed"), Some("1/250"));
    assert_eq!(xmp_find(&res, "XMP-exif:Aperture"), Some("f/2.8"));
    assert_eq!(
        xmp_find(&res, "XMP-exif:ExposureCompensation"),
        Some("-0.50")
    );
    assert_eq!(xmp_find(&res, "XMP-exif:FocalLength"), Some("50 mm"));
    assert_eq!(xmp_find(&res, "XMP-tiff:XResolution"), Some("72"));
    assert_eq!(xmp_find(&res, "XMP-tiff:YResolution"), Some("300"));
    assert_eq!(xmp_find(&res, "XMP-photoshop:Quality"), Some("85%"));
    assert_eq!(xmp_find(&res, "XMP-photoshop:ColorMode"), Some("RGB"));
    // Urgency formatting appends a human-readable suffix.
    let urgency = xmp_find(&res, "XMP-photoshop:Urgency").expect("Urgency present");
    assert!(urgency.starts_with('8'));
    // crs: namespace is unmapped, so it falls back to the generic "XMP:" prefix.
    assert_eq!(xmp_find(&res, "XMP:ProcessingParameters"), Some("1.5"));
}

#[test]
fn test_xmp_aperture_whole_and_focal_decimal() {
    // Whole-number aperture -> "f/4"; decimal focal -> "35.5 mm".
    let xml = br#"
        <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                 xmlns:exif="http://ns.adobe.com/exif/1.0/">
          <rdf:Description>
            <exif:Aperture>4</exif:Aperture>
            <exif:FocalLength>35.5</exif:FocalLength>
            <exif:ShutterSpeed>0.004</exif:ShutterSpeed>
          </rdf:Description>
        </rdf:RDF>
    "#;
    let res = parse_xmp(xml).unwrap();
    assert_eq!(xmp_find(&res, "XMP-exif:Aperture"), Some("f/4"));
    assert_eq!(xmp_find(&res, "XMP-exif:FocalLength"), Some("35.5 mm"));
    assert_eq!(xmp_find(&res, "XMP-exif:ShutterSpeed"), Some("0.004"));
}

#[test]
fn test_xmp_non_numeric_formatter_passthrough() {
    // Non-numeric values exercise the else-branch of every numeric formatter.
    let xml = br#"
        <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                 xmlns:exif="http://ns.adobe.com/exif/1.0/"
                 xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
                 xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/">
          <rdf:Description>
            <exif:ISO>auto</exif:ISO>
            <exif:Aperture>n/a</exif:Aperture>
            <exif:FocalLength>unknown</exif:FocalLength>
            <exif:ExposureCompensation>none</exif:ExposureCompensation>
            <tiff:XResolution>nan-text</tiff:XResolution>
            <photoshop:Quality>200</photoshop:Quality>
            <exif:ColorSpace>999</exif:ColorSpace>
          </rdf:Description>
        </rdf:RDF>
    "#;
    let res = parse_xmp(xml).unwrap();
    assert_eq!(xmp_find(&res, "XMP-exif:ISO"), Some("auto"));
    assert_eq!(xmp_find(&res, "XMP-exif:Aperture"), Some("n/a"));
    assert_eq!(xmp_find(&res, "XMP-exif:FocalLength"), Some("unknown"));
    assert_eq!(
        xmp_find(&res, "XMP-exif:ExposureCompensation"),
        Some("none")
    );
    assert_eq!(xmp_find(&res, "XMP-tiff:XResolution"), Some("nan-text"));
    // Quality > 100 passes through unchanged.
    assert_eq!(xmp_find(&res, "XMP-photoshop:Quality"), Some("200"));
    // Unknown ColorSpace passes through.
    assert_eq!(xmp_find(&res, "XMP-exif:ColorSpace"), Some("999"));
}

#[test]
fn test_xmp_nested_struct_is_skipped() {
    // A nested rdf:Description (struct) inside a property is a complex structure;
    // simple sibling properties still parse.
    let xml = br#"
        <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
                 xmlns:xmp="http://ns.adobe.com/xap/1.0/"
                 xmlns:dc="http://purl.org/dc/elements/1.1/">
          <rdf:Description>
            <xmp:Creator>Simple Value</xmp:Creator>
            <dc:title>
              <rdf:Description>
                <rdf:value>Struct Inner</rdf:value>
              </rdf:Description>
            </dc:title>
          </rdf:Description>
        </rdf:RDF>
    "#;
    let res = parse_xmp(xml).unwrap();
    assert_eq!(xmp_find(&res, "XMP:Creator"), Some("Simple Value"));
}

#[test]
fn test_xmp_empty_and_malformed() {
    // Empty Description -> no props.
    let empty = br#"
        <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
          <rdf:Description />
        </rdf:RDF>
    "#;
    assert_eq!(parse_xmp(empty).unwrap().len(), 0);

    // Invalid UTF-8 in a tag name -> ParseError.
    let mut bad = Vec::new();
    bad.extend_from_slice(
        b"<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description><",
    );
    bad.push(0xFF);
    bad.push(0xFE);
    bad.extend_from_slice(b":t>v</t></rdf:Description></rdf:RDF>");
    assert!(parse_xmp(&bad).is_err());
}
