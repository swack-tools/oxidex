//! Task 15 source-backed controls for legacy camera MakerNote rows that are
//! reachable through the existing per-family routes.

use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::{TagId, TagOccurrence, TagValue};
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernote_dispatcher::dispatch_makernote_with_model;
use std::collections::HashMap;

#[path = "common/fixtures.rs"]
mod fixtures;

fn occurrence<'a>(metadata: &'a oxidex::core::MetadataMap, key: &str) -> &'a TagOccurrence {
    let rows: Vec<_> = metadata
        .project_occurrences(ValueChannel::Stored)
        .filter(|(candidate, _, _)| *candidate == key)
        .collect();
    assert_eq!(rows.len(), 1, "{key} must have one physical occurrence");
    rows[0].1
}

fn assert_groups_and_id(row: &TagOccurrence, id: u16, group1: &str, group2: &str) {
    assert_eq!(row.id, TagId::Numeric(id));
    assert_eq!(row.group0.as_ref(), "MakerNotes");
    assert_eq!(row.group1.as_ref(), group1);
    assert_eq!(row.group2.as_deref(), Some(group2));
}

#[test]
#[ignore = "requires pinned combined-samples/Kodak.jpg"]
fn kodak_remaining_rows() {
    let path = fixtures::required_combined_fixture_path("Kodak.jpg");
    let metadata = read_metadata(&path).expect("Kodak.jpg parses");

    let time = occurrence(&metadata, "Kodak:TimeCreated");
    assert_groups_and_id(time, 0x0014, "Kodak", "Time");
    assert_eq!(time.raw, TagValue::new_string("10 22 28 62"));
    assert_eq!(
        time.project(ValueChannel::Stored).as_ref(),
        &TagValue::Array(
            [10, 22, 28, 62]
                .into_iter()
                .map(TagValue::Integer)
                .collect()
        )
    );
    assert_eq!(
        time.project(ValueChannel::ValueConv).as_ref(),
        &TagValue::new_string("10:22:28.62")
    );
    assert_eq!(
        time.project(ValueChannel::PrintConv).as_ref(),
        &TagValue::new_string("10:22:28.62")
    );

    let stamp = occurrence(&metadata, "Kodak:DateTimeStamp");
    assert_groups_and_id(stamp, 0x0064, "Kodak", "Camera");
    assert_eq!(stamp.raw, TagValue::Integer(0));
    assert_eq!(
        stamp.project(ValueChannel::Stored).as_ref(),
        &TagValue::Integer(0)
    );
    assert_eq!(
        stamp.project(ValueChannel::ValueConv).as_ref(),
        &TagValue::Integer(0)
    );
    assert_eq!(
        stamp.project(ValueChannel::PrintConv).as_ref(),
        &TagValue::new_string("Off")
    );
}

#[test]
#[ignore = "requires pinned combined-samples/Casio2.jpg"]
fn casio_remaining_rows() {
    let path = fixtures::required_combined_fixture_path("Casio2.jpg");
    let metadata = read_metadata(&path).expect("Casio2.jpg parses");

    for (key, id, raw, printed) in [
        ("Casio:Quality", 0x3002, 3, "Fine"),
        ("Casio:BestShotMode", 0x3007, 0, "Off"),
        ("Casio:ArtMode", 0x301b, 0, "Normal"),
    ] {
        let row = occurrence(&metadata, key);
        assert_groups_and_id(row, id, "Casio", "Camera");
        assert_eq!(row.raw, TagValue::Integer(raw));
        assert_eq!(
            row.project(ValueChannel::Stored).as_ref(),
            &TagValue::Integer(raw)
        );
        assert_eq!(
            row.project(ValueChannel::ValueConv).as_ref(),
            &TagValue::Integer(raw)
        );
        assert_eq!(
            row.project(ValueChannel::PrintConv).as_ref(),
            &TagValue::new_string(printed)
        );
    }
}

#[test]
#[ignore = "requires pinned combined-samples/JVC.jpg"]
fn jvc_remaining_rows() {
    let path = fixtures::required_combined_fixture_path("JVC.jpg");
    let metadata = read_metadata(&path).expect("JVC.jpg parses");

    let versions = occurrence(&metadata, "JVC:CPUVersions");
    assert_groups_and_id(versions, 0x0002, "JVC", "Camera");
    assert_eq!(
        versions.raw,
        TagValue::Binary(b"CPU1 2.00\0\x30\0CPU2 0496\0\x30\0".to_vec())
    );
    assert_eq!(
        versions.project(ValueChannel::Stored).as_ref(),
        &TagValue::Binary(b"CPU1 2.00\0\x30\0CPU2 0496\0\x30\0".to_vec())
    );
    assert_eq!(
        versions.project(ValueChannel::ValueConv).as_ref(),
        &TagValue::new_string("CPU1 2.00, 0, CPU2 0496, 0")
    );
    assert_eq!(
        versions.project(ValueChannel::PrintConv).as_ref(),
        &TagValue::new_string("CPU1 2.00, 0, CPU2 0496, 0")
    );

    let quality = occurrence(&metadata, "JVC:Quality");
    assert_groups_and_id(quality, 0x0003, "JVC", "Camera");
    assert_eq!(quality.raw, TagValue::Integer(1));
    assert_eq!(
        quality.project(ValueChannel::Stored).as_ref(),
        &TagValue::Integer(1)
    );
    assert_eq!(
        quality.project(ValueChannel::ValueConv).as_ref(),
        &TagValue::Integer(1)
    );
    assert_eq!(
        quality.project(ValueChannel::PrintConv).as_ref(),
        &TagValue::new_string("Normal")
    );
}

#[test]
#[ignore = "requires pinned combined-samples/Pentax/PentaxEI-200.jpg"]
fn kodak_type2_routes_pentax_carrier_before_make_fallback() {
    let path = fixtures::required_combined_fixture_path("Pentax/PentaxEI-200.jpg");
    let metadata = read_metadata(&path).expect("Pentax EI-200 parses");
    for (key, id, printed) in [
        ("Kodak:KodakMaker", 0x0008, TagValue::new_string("PENTAX")),
        (
            "Kodak:KodakModel",
            0x0028,
            TagValue::new_string("PENTAX EI-200"),
        ),
        ("Kodak:KodakImageWidth", 0x006c, TagValue::Integer(800)),
        ("Kodak:KodakImageHeight", 0x0070, TagValue::Integer(600)),
    ] {
        let row = occurrence(&metadata, key);
        assert_groups_and_id(row, id, "Kodak", "Camera");
        assert_eq!(row.project(ValueChannel::PrintConv).as_ref(), &printed);
        assert_eq!(row.origin.table, Some("Type2"));
    }
}

#[test]
#[ignore = "requires pinned combined-samples/Pentax/PentaxOptioE65.jpg"]
fn hp_remaining_rows() {
    let path = fixtures::required_combined_fixture_path("Pentax/PentaxOptioE65.jpg");
    let metadata = read_metadata(&path).expect("Pentax Optio E65 parses");
    let date = occurrence(&metadata, "HP:CameraDateTime");
    assert_groups_and_id(date, 0x0014, "HP", "Time");
    assert_eq!(
        date.project(ValueChannel::PrintConv).as_ref(),
        &TagValue::new_string("2216/02/28 03:49:48")
    );
    assert_eq!(date.origin.table, Some("Type4"));

    let iso = occurrence(&metadata, "HP:ISO");
    assert_groups_and_id(iso, 0x0034, "HP", "Camera");
    assert_eq!(iso.raw, TagValue::Integer(125));
    assert_eq!(
        iso.project(ValueChannel::PrintConv).as_ref(),
        &TagValue::Integer(125)
    );
    assert_eq!(iso.origin.table, Some("Type4"));

    for (key, id, printed) in [
        ("HP:MaxAperture", 0x000c, TagValue::Integer(29)),
        ("HP:ExposureTime", 0x0010, TagValue::new_string("1/40")),
    ] {
        let row = occurrence(&metadata, key);
        assert_groups_and_id(row, id, "HP", "Camera");
        assert_eq!(row.project(ValueChannel::PrintConv).as_ref(), &printed);
        assert_eq!(row.origin.table, Some("Type4"));
    }
}

#[test]
#[ignore = "requires pinned combined-samples/Pentax/PentaxXG-1.jpg"]
fn ricoh_remaining_rows() {
    let path = fixtures::required_combined_fixture_path("Pentax/PentaxXG-1.jpg");
    let metadata = read_metadata(&path).expect("Pentax XG-1 parses");
    let make = occurrence(&metadata, "Ricoh:RicohMake");
    assert_groups_and_id(make, 0x0300, "Ricoh", "Camera");
    assert_eq!(
        make.project(ValueChannel::PrintConv).as_ref(),
        &TagValue::new_string("XG-1Pentax")
    );
    assert_eq!(make.origin.table, Some("Type2"));

    let model = occurrence(&metadata, "Ricoh:RicohModel");
    assert_groups_and_id(model, 0x0207, "Ricoh", "Camera");
    assert_eq!(
        model.project(ValueChannel::PrintConv).as_ref(),
        &TagValue::new_string("")
    );
    assert_eq!(model.origin.table, Some("Type2"));
}

fn dispatched(make: &str, model: Option<&str>, note: &[u8]) -> HashMap<String, String> {
    let mut tags = HashMap::new();
    dispatch_makernote_with_model(make, model, note, ByteOrder::LittleEndian, &mut tags)
        .expect("bounded MakerNote dispatch");
    tags
}

#[test]
fn earlier_source_conditions_block_hp4_and_kodak2_signature_routes() {
    // MakerNotes.pm:61 Canon Make precedes HP4 (:206) and Kodak2 (:275).
    // This valid HP4 record would otherwise emit ISO 125 under Canon Make.
    let mut hp = vec![0; 120];
    hp[..6].copy_from_slice(b"IIII\x04\0");
    hp[20..39].copy_from_slice(b"2216/02/28 03:49:48");
    hp[52..54].copy_from_slice(&125u16.to_le_bytes());
    let mut tags = HashMap::new();
    let _ = dispatch_makernote_with_model("Canon", None, &hp, ByteOrder::LittleEndian, &mut tags);
    assert!(!tags.keys().any(|name| name.starts_with("HP:")));

    // Canon Make likewise precedes Kodak2. Kodak2's first alternative leaves
    // its initial eight bytes arbitrary, so an earlier signature may occupy
    // them. FujiFilm (:119) and JVC (:237) are signature-first examples.
    for (make, prefix) in [
        ("Canon", *b"abcdefgh"),
        ("PENTAX", *b"FUJIFILM"),
        ("PENTAX", *b"JVC abc\0"),
    ] {
        let mut note = vec![0; 116];
        note[..8].copy_from_slice(&prefix);
        note[8..21].copy_from_slice(b"Eastman Kodak");
        note[108..112].copy_from_slice(&800u32.to_be_bytes());
        let mut tags = HashMap::new();
        let _ =
            dispatch_makernote_with_model(make, None, &note, ByteOrder::LittleEndian, &mut tags);
        assert!(
            !tags.keys().any(|name| name.starts_with("Kodak:")),
            "earlier source condition {prefix:?} / {make} must block Kodak2"
        );
    }
}

#[test]
fn source_order_routes_kodak_type2_by_payload_without_kodak_make() {
    // MakerNotes.pm:275-286: eight arbitrary bytes followed by the literal
    // Eastman Kodak is enough, even when EXIF Make names another camera.
    let mut note = vec![0; 116];
    note[8..21].copy_from_slice(b"Eastman Kodak");
    note[108..112].copy_from_slice(&800u32.to_be_bytes());
    let tags = dispatched("PENTAX", None, &note);
    assert_eq!(
        tags.get("Kodak:KodakMaker"),
        Some(&"Eastman Kodak".to_string())
    );
    assert_eq!(tags.get("Kodak:KodakImageWidth"), Some(&"800".to_string()));

    note[20] = b'X';
    let near_match = dispatched("PENTAX", None, &note);
    assert!(!near_match.contains_key("Kodak:KodakMaker"));

    // The second source alternative is byte-shaped and also Make-independent.
    let mut second = vec![0; 116];
    second[..12].copy_from_slice(b"\x01\0\x01\0\0\0\x04\0ABCD");
    let tags = dispatched("MINOLTA", None, &second);
    assert_eq!(tags.get("Kodak:KodakMaker"), Some(&"ABCD".to_string()));
    second[11] = b'0';
    assert!(!dispatched("MINOLTA", None, &second).contains_key("Kodak:KodakMaker"));
}

#[test]
fn source_order_kodak_type2_beats_later_signature_routes() {
    // MakerNotes.pm:275 Kodak2 precedes Minolta2 (:508) and PhaseOne
    // (:841). Its first alternative leaves the initial eight bytes free,
    // so both later signatures can coexist with a valid Kodak2 header.
    for prefix in [*b"MINOL\0AB", *b"IIIIxwaR"] {
        let mut note = vec![0; 116];
        note[..8].copy_from_slice(&prefix);
        note[8..21].copy_from_slice(b"Eastman Kodak");
        note[108..112].copy_from_slice(&800u32.to_be_bytes());
        let tags = dispatched("PENTAX", None, &note);
        assert_eq!(
            tags.get("Kodak:KodakMaker"),
            Some(&"Eastman Kodak".to_string()),
            "Kodak2 must win over later signature {prefix:?}"
        );
        assert_eq!(tags.get("Kodak:KodakImageWidth"), Some(&"800".to_string()));
    }
}

#[test]
fn source_order_routes_hp_type4_by_payload_without_hp_make() {
    // MakerNotes.pm:206-214: the Type4 selector precedes Make-based Pentax.
    let mut note = vec![0; 120];
    note[..6].copy_from_slice(b"IIII\x04\0");
    note[20..39].copy_from_slice(b"2216/02/28 03:49:48");
    note[52..54].copy_from_slice(&125u16.to_le_bytes());
    let tags = dispatched("PENTAX", None, &note);
    assert_eq!(
        tags.get("HP:CameraDateTime"),
        Some(&"2216/02/28 03:49:48".to_string())
    );
    assert_eq!(tags.get("HP:ISO"), Some(&"125".to_string()));

    // Both source selectors match this payload. MakerNoteHP4 precedes
    // MakerNoteKodak2 in MakerNotes.pm, so exactly the HP decoder owns it.
    note[8..21].copy_from_slice(b"Eastman Kodak");
    let first_match = dispatched("PENTAX", None, &note);
    assert_eq!(first_match.get("HP:ISO"), Some(&"125".to_string()));
    assert!(!first_match.contains_key("Kodak:KodakMaker"));

    note[4] = 7;
    let near_match = dispatched("PENTAX", None, &note);
    assert!(!near_match.contains_key("HP:CameraDateTime"));

    note[4] = 5;
    assert_eq!(
        dispatched("PENTAX", None, &note).get("HP:ISO"),
        Some(&"125".to_string())
    );
    note[4] = b'|'; // the pinned Perl character class also admits literal '|'
    assert_eq!(
        dispatched("PENTAX", None, &note).get("HP:ISO"),
        Some(&"125".to_string())
    );
}

#[test]
fn ricoh_type2_requires_source_make_and_padded_tiff_shape() {
    // MakerNotes.pm:924-938: Ricoh2 wins before the later Make fallback.
    let mut note = vec![0; 100];
    note[..8].copy_from_slice(b"II*\0\x08\0\0\0");
    note[8..10].copy_from_slice(&2u16.to_le_bytes());
    note[12..14].copy_from_slice(&0x0207u16.to_le_bytes());
    note[14..16].copy_from_slice(&2u16.to_le_bytes());
    note[16..20].copy_from_slice(&4u32.to_le_bytes());
    note[20..24].copy_from_slice(b"ABCD");
    note[24..26].copy_from_slice(&0x0300u16.to_le_bytes());
    note[26..28].copy_from_slice(&7u16.to_le_bytes());
    note[28..32].copy_from_slice(&8u32.to_le_bytes());
    note[32..36].copy_from_slice(&80u32.to_le_bytes());
    note[80..88].copy_from_slice(b"Make    ");
    let tags = dispatched("RICOH IMAGING COMPANY, LTD.", Some("PENTAX XG-1"), &note);
    assert_eq!(tags.get("Ricoh:RicohModel"), Some(&"ABCD".to_string()));
    assert_eq!(tags.get("Ricoh:RicohMake"), Some(&"Make".to_string()));

    let wrong_make = dispatched("ricoh imaging company, ltd.", Some("PENTAX XG-1"), &note);
    assert!(!wrong_make.contains_key("Ricoh:RicohModel"));
    note[9] = 0x09; // no padded entry-count shape
    let wrong_shape = dispatched("RICOH IMAGING COMPANY, LTD.", Some("PENTAX XG-1"), &note);
    assert!(!wrong_shape.contains_key("Ricoh:RicohModel"));

    // The source's model override accepts WG-M1 without a TIFF-shaped prefix.
    note[..8].copy_from_slice(b"WG-M1foo");
    note[8..10].copy_from_slice(&2u16.to_le_bytes());
    let wg = dispatched("RICOH", Some("RICOH WG-M1"), &note);
    assert_eq!(wg.get("Ricoh:RicohModel"), Some(&"ABCD".to_string()));
}
