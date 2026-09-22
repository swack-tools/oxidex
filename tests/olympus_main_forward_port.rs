//! Source-backed Task11 Phase B controls for the generated Olympus route.

use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::{TagId, TagValue};
use oxidex::exiftool_tables::session::{MemberVal, Session};
use oxidex::exiftool_tables::{Ctx, MemberValue};
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernote_dispatcher::{
    dispatch_makernote, dispatch_makernote_with_context_and_values_and_session_and_occurrences,
};
use oxidex::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

fn entry(tag: u16, data_type: u16, count: u32, value: u32, order: ByteOrder) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(12);
    match order {
        ByteOrder::LittleEndian => {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&data_type.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        ByteOrder::BigEndian => {
            bytes.extend_from_slice(&tag.to_be_bytes());
            bytes.extend_from_slice(&data_type.to_be_bytes());
            bytes.extend_from_slice(&count.to_be_bytes());
            bytes.extend_from_slice(&value.to_be_bytes());
        }
    }
    bytes
}

fn push_u16(bytes: &mut Vec<u8>, value: u16, order: ByteOrder) {
    match order {
        ByteOrder::LittleEndian => bytes.extend_from_slice(&value.to_le_bytes()),
        ByteOrder::BigEndian => bytes.extend_from_slice(&value.to_be_bytes()),
    }
}

fn push_u32(bytes: &mut Vec<u8>, value: u32, order: ByteOrder) {
    match order {
        ByteOrder::LittleEndian => bytes.extend_from_slice(&value.to_le_bytes()),
        ByteOrder::BigEndian => bytes.extend_from_slice(&value.to_be_bytes()),
    }
}

fn stacked_note(first: u32, second: u32) -> Vec<u8> {
    stacked_note_pairs(&[(first, second)])
}

fn stacked_note_pairs(pairs: &[(u32, u32)]) -> Vec<u8> {
    const CAMERA_SETTINGS: u32 = 40;
    let order = ByteOrder::LittleEndian;
    let mut note = b"OLYMPUS\0II\x03\0".to_vec();
    push_u16(&mut note, 1, order);
    note.extend_from_slice(&entry(0x2020, 4, 1, CAMERA_SETTINGS, order));
    push_u32(&mut note, 0, order);
    note.resize(CAMERA_SETTINGS as usize, 0);
    push_u16(&mut note, pairs.len() as u16, order);
    let values_start = CAMERA_SETTINGS as usize + 2 + pairs.len() * 12 + 4;
    for (index, _) in pairs.iter().enumerate() {
        note.extend_from_slice(&entry(
            0x0804,
            4,
            2,
            (values_start + index * 8) as u32,
            order,
        ));
    }
    push_u32(&mut note, 0, order);
    note.resize(values_start, 0);
    for (first, second) in pairs {
        push_u32(&mut note, *first, order);
        push_u32(&mut note, *second, order);
    }
    note
}

fn orf_with_makernote(note: &[u8]) -> Vec<u8> {
    let order = ByteOrder::LittleEndian;
    let make = b"OLYMPUS CORPORATION\0";
    let make_offset = 8 + 2 + 2 * 12 + 4;
    let note_offset = make_offset + make.len();
    let mut bytes = b"IIRO".to_vec();
    push_u32(&mut bytes, 8, order);
    push_u16(&mut bytes, 2, order);
    bytes.extend_from_slice(&entry(
        0x010f,
        2,
        make.len() as u32,
        make_offset as u32,
        order,
    ));
    bytes.extend_from_slice(&entry(
        0x927c,
        7,
        note.len() as u32,
        note_offset as u32,
        order,
    ));
    push_u32(&mut bytes, 0, order);
    bytes.extend_from_slice(make);
    bytes.extend_from_slice(note);
    bytes
}

fn minolta2_note(
    signature: &[u8; 6],
    order: ByteOrder,
    payload_tiff_offset: u32,
    payload: &[u8],
) -> Vec<u8> {
    const PAYLOAD_OFFSET: u32 = 32;
    let mut note = signature.to_vec();
    note.extend_from_slice(&[0, 0]);
    push_u16(&mut note, 1, order);
    note.extend_from_slice(&entry(
        0x2050,
        7,
        payload.len() as u32,
        payload_tiff_offset + PAYLOAD_OFFSET,
        order,
    ));
    push_u32(&mut note, 0, order);
    note.resize(PAYLOAD_OFFSET as usize, 0);
    note.extend_from_slice(payload);
    note
}

fn preview_note(start: u32, length: u32, order: ByteOrder) -> Vec<u8> {
    let mut note = b"OLYMPUS\0".to_vec();
    note.extend_from_slice(match order {
        ByteOrder::LittleEndian => b"II",
        ByteOrder::BigEndian => b"MM",
    });
    push_u16(&mut note, 3, order);
    push_u16(&mut note, 2, order);
    note.extend_from_slice(&entry(0x0f04, 4, 1, start, order));
    note.extend_from_slice(&entry(0x0f05, 4, 1, length, order));
    push_u32(&mut note, 0, order);
    note
}

fn preview_main_info_note(
    top_level: Option<(u32, u32)>,
    main_info: (u32, u32),
    order: ByteOrder,
) -> Vec<u8> {
    const MAIN_INFO_OFFSET: u32 = 80;
    let mut note = b"OLYMPUS\0".to_vec();
    note.extend_from_slice(match order {
        ByteOrder::LittleEndian => b"II",
        ByteOrder::BigEndian => b"MM",
    });
    push_u16(&mut note, 3, order);
    push_u16(&mut note, if top_level.is_some() { 3 } else { 1 }, order);
    if let Some((start, length)) = top_level {
        note.extend_from_slice(&entry(0x0f04, 4, 1, start, order));
        note.extend_from_slice(&entry(0x0f05, 4, 1, length, order));
    }
    note.extend_from_slice(&entry(0x4000, 4, 1, MAIN_INFO_OFFSET, order));
    push_u32(&mut note, 0, order);
    note.resize(MAIN_INFO_OFFSET as usize, 0);
    push_u16(&mut note, 2, order);
    note.extend_from_slice(&entry(0x0f04, 4, 1, main_info.0, order));
    note.extend_from_slice(&entry(0x0f05, 4, 1, main_info.1, order));
    push_u32(&mut note, 0, order);
    note
}

fn selected_preview_orf(order: ByteOrder) -> (Vec<u8>, Vec<u8>, u32) {
    const IFD0_OFFSET: u32 = 8;
    const IFD0_ENTRIES: usize = 5;
    const EXIF_ENTRIES: usize = 1;
    let ifd0_end = IFD0_OFFSET as usize + 2 + IFD0_ENTRIES * 12 + 4;
    let exif_offset = ifd0_end as u32;
    let exif_end = ifd0_end + 2 + EXIF_ENTRIES * 12 + 4;
    let make = b"OLYMPUS CORPORATION\0";
    let make_offset = exif_end as u32;
    let note_len = 42u32;
    let first_payload = b"\xff\xd8first\xff\xd9";
    let selected_payload = b"\xff\xd8selected\xff\xd9".to_vec();
    let first_note_offset = make_offset + make.len() as u32;
    let first_payload_offset = first_note_offset + note_len;
    let duplicate_note_offset = first_payload_offset + first_payload.len() as u32;
    let duplicate_payload_offset = duplicate_note_offset + note_len;
    let exif_note_offset = duplicate_payload_offset + selected_payload.len() as u32;
    let exif_payload_offset = exif_note_offset + note_len;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(match order {
        ByteOrder::LittleEndian => b"IIRO",
        ByteOrder::BigEndian => b"MMOR",
    });
    push_u32(&mut bytes, IFD0_OFFSET, order);
    push_u16(&mut bytes, IFD0_ENTRIES as u16, order);
    bytes.extend_from_slice(&entry(0x010f, 2, make.len() as u32, make_offset, order));
    bytes.extend_from_slice(&entry(0x1234, 13, 1, 0, order));
    bytes.extend_from_slice(&entry(0x927c, 7, note_len, first_note_offset, order));
    bytes.extend_from_slice(&entry(0x927c, 7, note_len, duplicate_note_offset, order));
    bytes.extend_from_slice(&entry(0x8769, 4, 1, exif_offset, order));
    push_u32(&mut bytes, 0, order);

    push_u16(&mut bytes, EXIF_ENTRIES as u16, order);
    bytes.extend_from_slice(&entry(0x927c, 7, note_len, exif_note_offset, order));
    push_u32(&mut bytes, 0, order);
    bytes.extend_from_slice(make);
    bytes.extend_from_slice(&preview_note(
        first_payload_offset,
        first_payload.len() as u32,
        order,
    ));
    bytes.extend_from_slice(first_payload);
    bytes.extend_from_slice(&preview_note(
        duplicate_payload_offset,
        selected_payload.len() as u32,
        order,
    ));
    bytes.extend_from_slice(&selected_payload);
    bytes.extend_from_slice(&preview_note(
        exif_payload_offset,
        selected_payload.len() as u32,
        order,
    ));
    bytes.extend_from_slice(&selected_payload);
    (bytes, selected_payload, exif_payload_offset)
}

type StructuredDispatchOutput = (
    HashMap<String, String>,
    HashMap<String, String>,
    Vec<(String, oxidex::core::TagOccurrence)>,
    HashMap<&'static str, MemberValue>,
    Session,
);

fn dispatch_structured(make: &str, note: &[u8], inherited: ByteOrder) -> StructuredDispatchOutput {
    const NOTE_OFFSET: usize = 96;
    let mut tiff = vec![0u8; NOTE_OFFSET];
    tiff.extend_from_slice(note);
    dispatch_structured_in_tiff(make, &tiff, NOTE_OFFSET, note.len(), 0, inherited)
}

fn dispatch_structured_in_tiff(
    make: &str,
    tiff: &[u8],
    note_offset: usize,
    note_len: usize,
    tiff_base: u64,
    inherited: ByteOrder,
) -> StructuredDispatchOutput {
    let ctx = MakerNoteContext::in_tiff(tiff, note_offset, note_len, tiff_base);
    let mut session = Session::new();
    let mut members = HashMap::new();
    let mut tags = HashMap::new();
    let mut values = HashMap::new();
    let mut rows = Vec::new();
    {
        let mut cond_ctx = Ctx::new(&mut members);
        dispatch_makernote_with_context_and_values_and_session_and_occurrences(
            make,
            None,
            &ctx,
            inherited,
            &mut session,
            &mut cond_ctx,
            &mut tags,
            &mut values,
            &mut rows,
        )
        .expect("synthetic MakerNote dispatches");
    }
    (tags, values, rows, members, session)
}

#[test]
fn stacked_image_uses_generated_direct_wildcard_and_unknown_forms() {
    for (pair, expected) in [
        ((0, 0), "No"),
        ((1, 7), "Live Composite (7 images)"),
        ((3, 64), "ND64 (6EV)"),
        ((7, 3), "Unknown (7 3)"),
    ] {
        let mut tags = HashMap::new();
        dispatch_makernote(
            "OLYMPUS CORPORATION",
            &stacked_note(pair.0, pair.1),
            ByteOrder::LittleEndian,
            &mut tags,
        )
        .expect("StackedImage fixture parses");
        assert_eq!(
            tags.get("Olympus:StackedImage").map(String::as_str),
            Some(expected),
            "pair {pair:?}"
        );
    }
}

#[test]
fn minolta2_headers_route_before_make_and_resolve_unknown_byte_order() {
    let payload = b"camera-parameters\0\xff";
    for (signature, order, inherited) in [
        (b"CAMER\0", ByteOrder::LittleEndian, ByteOrder::BigEndian),
        (b"CAMER\0", ByteOrder::BigEndian, ByteOrder::LittleEndian),
        (b"MINOL\0", ByteOrder::LittleEndian, ByteOrder::BigEndian),
        (b"MINOL\0", ByteOrder::BigEndian, ByteOrder::LittleEndian),
    ] {
        let note = minolta2_note(signature, order, 96, payload);
        let (tags, _values, rows, members, session) =
            dispatch_structured("PENTAX Corporation", &note, inherited);
        assert_eq!(members.get("OlympusCAMER"), Some(&MemberValue::Num(1)));
        assert_eq!(session.member("OlympusCAMER"), MemberVal::Int(1));
        assert!(
            !tags.contains_key("Olympus:CameraParameters"),
            "a generated owner must not be duplicated into the residual map"
        );
        let (_, occurrence) = rows
            .iter()
            .find(|(key, _)| key == "Olympus:CameraParameters")
            .expect("generated CameraParameters occurrence");
        assert_eq!(occurrence.group0.as_ref(), "MakerNotes");
        assert_eq!(occurrence.group1.as_ref(), "Olympus");
        assert_eq!(
            occurrence.project(ValueChannel::Stored).as_ref(),
            &TagValue::Binary(payload.to_vec())
        );
    }
}

#[test]
fn minolta2_header_match_is_literal_and_malformed_forms_do_not_seed_state() {
    for bad in [
        b"CAMERX\0\0".as_slice(),
        b"MINOLX\0\0".as_slice(),
        b"CAMER".as_slice(),
    ] {
        let (tags, values, rows, members, session) =
            dispatch_structured("PENTAX Corporation", bad, ByteOrder::LittleEndian);
        assert!(tags.is_empty());
        assert!(values.is_empty());
        assert!(rows.is_empty());
        assert!(!members.contains_key("OlympusCAMER"));
        assert!(!session.has_member("OlympusCAMER"));
    }
}

#[test]
fn zoomed_preview_pair_reaches_public_metadata_as_exact_bytes() {
    let path = Path::new("tests/fixtures/tiff/olympus/zoomed_preview_pair.tiff");
    let metadata = oxidex::core::operations::read_metadata(path).expect("synthetic TIFF parses");
    assert_eq!(
        metadata.get("Olympus:ZoomedPreviewStart"),
        Some(&TagValue::Integer(118))
    );
    assert_eq!(
        metadata.get("Olympus:ZoomedPreviewLength"),
        Some(&TagValue::Integer(624))
    );

    let expected = std::fs::read("tests/fixtures/jpeg/complex/sample_with_exif_xmp.jpg")
        .expect("source payload fixture");
    assert_eq!(
        metadata.get("Olympus:ZoomedPreviewImage"),
        Some(&TagValue::Binary(expected.clone()))
    );
    let occurrences: Vec<_> = metadata
        .project_occurrences(ValueChannel::Stored)
        .filter(|(key, _, _)| *key == "Olympus:ZoomedPreviewImage")
        .collect();
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].1.group0.as_ref(), "MakerNotes");
    assert_eq!(occurrences[0].1.group1.as_ref(), "Olympus");
    assert_eq!(occurrences[0].2.as_ref(), &TagValue::Binary(expected));
}

#[test]
fn zoomed_preview_binary_provenance_includes_nonzero_tiff_base() {
    const TIFF_BASE: u64 = 4_096;
    const NOTE_OFFSET: usize = 96;
    let preview = b"\xff\xd8embedded-preview\xff\xd9";
    let note_len = preview_note(0, preview.len() as u32, ByteOrder::LittleEndian).len();
    let preview_start = NOTE_OFFSET + note_len;
    let note = preview_note(
        preview_start as u32,
        preview.len() as u32,
        ByteOrder::LittleEndian,
    );
    let mut tiff = vec![0u8; NOTE_OFFSET];
    tiff.extend_from_slice(&note);
    tiff.extend_from_slice(preview);

    let (_tags, _values, rows, _members, _session) = dispatch_structured_in_tiff(
        "OLYMPUS CORPORATION",
        &tiff,
        NOTE_OFFSET,
        note.len(),
        TIFF_BASE,
        ByteOrder::LittleEndian,
    );
    let (_, occurrence) = rows
        .iter()
        .find(|(key, _)| key == "Olympus:ZoomedPreviewImage")
        .expect("valid embedded preview occurrence");
    assert_eq!(
        occurrence.project(ValueChannel::Stored).as_ref(),
        &TagValue::Binary(preview.to_vec())
    );
    assert_eq!(
        occurrence.origin.byte_range,
        Some(
            TIFF_BASE + preview_start as u64
                ..TIFF_BASE + preview_start as u64 + preview.len() as u64
        )
    );
}

#[test]
fn zoomed_preview_binary_fails_closed_when_absolute_range_overflows() {
    const NOTE_OFFSET: usize = 96;
    let preview = b"\xff\xd8overflow-preview\xff\xd9";
    let note_len = preview_note(0, preview.len() as u32, ByteOrder::LittleEndian).len();
    let preview_start = NOTE_OFFSET + note_len;
    let note = preview_note(
        preview_start as u32,
        preview.len() as u32,
        ByteOrder::LittleEndian,
    );
    let mut tiff = vec![0u8; NOTE_OFFSET];
    tiff.extend_from_slice(&note);
    tiff.extend_from_slice(preview);

    let (_tags, _values, rows, _members, _session) = dispatch_structured_in_tiff(
        "OLYMPUS CORPORATION",
        &tiff,
        NOTE_OFFSET,
        note.len(),
        u64::MAX - NOTE_OFFSET as u64,
        ByteOrder::LittleEndian,
    );
    assert!(
        rows.iter()
            .any(|(key, _)| key == "Olympus:ZoomedPreviewStart")
    );
    assert!(
        rows.iter()
            .any(|(key, _)| key == "Olympus:ZoomedPreviewLength")
    );
    assert!(
        !rows
            .iter()
            .any(|(key, _)| key == "Olympus:ZoomedPreviewImage"),
        "binary output without an absolute source-file range must fail closed"
    );
}

#[test]
fn preview_pair_main_info_emits_exact_scalars_without_invalid_image() {
    let note = preview_main_info_note(None, (50_000, 624), ByteOrder::LittleEndian);
    let (tags, _values, rows, _members, _session) =
        dispatch_structured("OLYMPUS CORPORATION", &note, ByteOrder::LittleEndian);
    assert!(!tags.contains_key("Olympus:ZoomedPreviewStart"));
    assert!(!tags.contains_key("Olympus:ZoomedPreviewLength"));

    let zoomed: Vec<_> = rows
        .iter()
        .filter(|(key, _)| key.starts_with("Olympus:ZoomedPreview"))
        .collect();
    assert_eq!(
        zoomed
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        ["Olympus:ZoomedPreviewStart", "Olympus:ZoomedPreviewLength",]
    );
    for (index, id, value) in [(0, 0x0f04, 50_000), (1, 0x0f05, 624)] {
        let occurrence = &zoomed[index].1;
        assert_eq!(occurrence.id, TagId::Numeric(id));
        assert_eq!(occurrence.group0.as_ref(), "MakerNotes");
        assert_eq!(occurrence.group1.as_ref(), "Olympus");
        assert_eq!(
            occurrence.group2.as_ref().map(|group| group.as_ref()),
            Some("Camera")
        );
        assert_eq!(occurrence.raw, TagValue::Integer(value));
        assert_eq!(
            occurrence.project(ValueChannel::ValueConv).as_ref(),
            &TagValue::Integer(value)
        );
        assert_eq!(
            occurrence.project(ValueChannel::PrintConv).as_ref(),
            &TagValue::Integer(value)
        );
        assert_eq!(
            occurrence.project(ValueChannel::Stored).as_ref(),
            &TagValue::Integer(value)
        );
    }
    assert!(
        !rows
            .iter()
            .any(|(key, _)| key == "Olympus:ZoomedPreviewImage")
    );
}

#[test]
fn preview_pair_top_level_then_main_info_preserves_order_and_later_winner() {
    let note = preview_main_info_note(Some((40_000, 512)), (50_000, 624), ByteOrder::LittleEndian);
    let (_tags, _values, rows, _members, _session) =
        dispatch_structured("OLYMPUS CORPORATION", &note, ByteOrder::LittleEndian);
    let scalars: Vec<_> = rows
        .iter()
        .filter(|(key, _)| key.starts_with("Olympus:ZoomedPreview"))
        .map(|(key, occurrence)| (key.as_str(), occurrence.raw.clone()))
        .collect();
    assert_eq!(
        scalars,
        [
            ("Olympus:ZoomedPreviewStart", TagValue::Integer(40_000),),
            ("Olympus:ZoomedPreviewLength", TagValue::Integer(512),),
            ("Olympus:ZoomedPreviewStart", TagValue::Integer(50_000),),
            ("Olympus:ZoomedPreviewLength", TagValue::Integer(624),),
        ]
    );

    let mut legacy = HashMap::new();
    dispatch_makernote(
        "OLYMPUS CORPORATION",
        &note,
        ByteOrder::LittleEndian,
        &mut legacy,
    )
    .expect("combined preview pair dispatches");
    assert_eq!(
        legacy.get("Olympus:ZoomedPreviewStart").map(String::as_str),
        Some("50000")
    );
    assert_eq!(
        legacy
            .get("Olympus:ZoomedPreviewLength")
            .map(String::as_str),
        Some("624")
    );
    assert!(!legacy.contains_key("Olympus:ZoomedPreviewImage"));
}

#[test]
fn preview_pair_legacy_dispatch_projects_top_level_and_main_info_scalars_only() {
    for (note, start, length) in [
        (
            preview_note(40_000, 512, ByteOrder::LittleEndian),
            "40000",
            "512",
        ),
        (
            preview_main_info_note(None, (50_000, 624), ByteOrder::LittleEndian),
            "50000",
            "624",
        ),
    ] {
        let mut tags = HashMap::new();
        dispatch_makernote(
            "OLYMPUS CORPORATION",
            &note,
            ByteOrder::LittleEndian,
            &mut tags,
        )
        .expect("legacy preview pair dispatches");
        assert_eq!(
            tags.get("Olympus:ZoomedPreviewStart").map(String::as_str),
            Some(start)
        );
        assert_eq!(
            tags.get("Olympus:ZoomedPreviewLength").map(String::as_str),
            Some(length)
        );
        assert!(!tags.contains_key("Olympus:ZoomedPreviewImage"));
    }
}

#[test]
fn zoomed_preview_pair_reaches_the_real_olympus_raw_reader() {
    let mut bytes = std::fs::read("tests/fixtures/tiff/olympus/zoomed_preview_pair.tiff")
        .expect("source TIFF fixture");
    bytes[2..4].copy_from_slice(b"RO");
    let mut raw = tempfile::Builder::new()
        .prefix("task11-zoomed-preview-")
        .suffix(".orf")
        .tempfile()
        .expect("durable test temp file");
    raw.write_all(&bytes).expect("write synthetic ORF");
    raw.flush().expect("flush synthetic ORF");

    let metadata =
        oxidex::core::operations::read_metadata(raw.path()).expect("synthetic ORF parses");
    let expected = std::fs::read("tests/fixtures/jpeg/complex/sample_with_exif_xmp.jpg")
        .expect("source payload fixture");
    assert_eq!(metadata.get_string("File:FileType"), Some("ORF"));
    assert_eq!(
        metadata.get("Olympus:ZoomedPreviewStart"),
        Some(&TagValue::Integer(118))
    );
    assert_eq!(
        metadata.get("Olympus:ZoomedPreviewLength"),
        Some(&TagValue::Integer(624))
    );
    assert_eq!(
        metadata.get("Olympus:ZoomedPreviewImage"),
        Some(&TagValue::Binary(expected.clone()))
    );

    let occurrences: Vec<_> = metadata
        .project_occurrences(ValueChannel::Stored)
        .filter(|(key, _, _)| *key == "Olympus:ZoomedPreviewImage")
        .collect();
    assert_eq!(occurrences.len(), 1);
    let (key, occurrence, stored) = &occurrences[0];
    assert_eq!(*key, "Olympus:ZoomedPreviewImage");
    assert_eq!(
        occurrence.id,
        TagId::Named("DataTag:ZoomedPreviewImage".to_string())
    );
    assert_eq!(occurrence.group0.as_ref(), "MakerNotes");
    assert_eq!(occurrence.group1.as_ref(), "Olympus");
    assert_eq!(
        occurrence.group2.as_ref().map(|group| group.as_ref()),
        Some("Preview")
    );
    assert_eq!(occurrence.origin.module, Some("Olympus"));
    assert_eq!(occurrence.origin.table, Some("Main"));
    assert_eq!(occurrence.origin.byte_range, Some(118..742));
    assert_eq!(stored.as_ref(), &TagValue::Binary(expected.clone()));
    assert_eq!(
        occurrence.project(ValueChannel::ValueConv).as_ref(),
        &TagValue::Binary(expected)
    );
    assert_eq!(
        occurrence.project(ValueChannel::PrintConv).as_ref(),
        &TagValue::new_string("(Binary data 624 bytes, use -b option to extract)")
    );

    let zoomed: Vec<_> = metadata
        .project_occurrences(ValueChannel::Stored)
        .filter(|(key, _, _)| key.starts_with("Olympus:ZoomedPreview"))
        .collect();
    assert_eq!(
        zoomed.iter().map(|(key, _, _)| *key).collect::<Vec<_>>(),
        [
            "Olympus:ZoomedPreviewStart",
            "Olympus:ZoomedPreviewLength",
            "Olympus:ZoomedPreviewImage",
        ]
    );
    for (index, key, id, value) in [
        (0, "Olympus:ZoomedPreviewStart", 0x0f04, 118),
        (1, "Olympus:ZoomedPreviewLength", 0x0f05, 624),
    ] {
        let (_, occurrence, stored) = &zoomed[index];
        assert_eq!(zoomed.iter().find(|row| row.0 == key).unwrap().0, key);
        assert_eq!(occurrence.id, TagId::Numeric(id));
        assert_eq!(occurrence.group0.as_ref(), "MakerNotes");
        assert_eq!(occurrence.group1.as_ref(), "Olympus");
        assert_eq!(
            occurrence.group2.as_ref().map(|group| group.as_ref()),
            Some("Camera")
        );
        assert_eq!(occurrence.origin.module, Some("Olympus"));
        assert_eq!(occurrence.origin.table, Some("Main"));
        assert_eq!(stored.as_ref(), &TagValue::Integer(value));
        assert_eq!(
            occurrence.project(ValueChannel::ValueConv).as_ref(),
            &TagValue::Integer(value)
        );
        assert_eq!(
            occurrence.project(ValueChannel::PrintConv).as_ref(),
            &TagValue::Integer(value)
        );
    }
}

#[test]
fn raw_reader_replays_generated_duplicates_with_typed_forms_and_attribution() {
    let data = orf_with_makernote(&stacked_note_pairs(&[(1, 7), (3, 64)]));
    let metadata = oxidex::parsers::raw::metadata::parse_raw_metadata(
        &data,
        oxidex::parsers::raw::RawFormat::OlympusORF,
    )
    .expect("synthetic ORF parses");
    let occurrences: Vec<_> = metadata
        .project_occurrences(ValueChannel::Stored)
        .filter(|(key, _, _)| *key == "Olympus:StackedImage")
        .collect();
    let engine_off = std::env::var("OXIDEX_GENSHARE_SILENCE")
        .ok()
        .is_some_and(|tokens| tokens.split(',').any(|token| token == "engine"));
    if engine_off {
        assert!(occurrences.is_empty());
        assert!(!metadata.contains_key("Olympus:StackedImage"));
        return;
    }

    assert_eq!(occurrences.len(), 2);
    assert_eq!(
        occurrences
            .iter()
            .map(|(_, occurrence, _)| { occurrence.project(ValueChannel::PrintConv).into_owned() })
            .collect::<Vec<_>>(),
        [
            TagValue::new_string("Live Composite (7 images)"),
            TagValue::new_string("ND64 (6EV)"),
        ]
    );
    assert_eq!(
        occurrences
            .iter()
            .map(|(_, occurrence, _)| { occurrence.project(ValueChannel::ValueConv).into_owned() })
            .collect::<Vec<_>>(),
        [TagValue::new_string("1 7"), TagValue::new_string("3 64")]
    );
    assert_eq!(
        occurrences
            .iter()
            .map(|(_, occurrence, _)| occurrence.id.clone())
            .collect::<Vec<_>>(),
        [TagId::Numeric(0x0804), TagId::Numeric(0x0804)]
    );
    assert_eq!(
        metadata.get_string("Olympus:StackedImage"),
        Some("ND64 (6EV)")
    );
}

#[test]
fn raw_reader_prefers_exif_ifd_and_last_surviving_duplicates_in_both_byte_orders() {
    for order in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
        let (bytes, expected, expected_start) = selected_preview_orf(order);
        let metadata = oxidex::parsers::raw::metadata::parse_raw_metadata(
            &bytes,
            oxidex::parsers::raw::RawFormat::OlympusORF,
        )
        .expect("synthetic ORF parses");
        assert_eq!(
            metadata.get("Olympus:ZoomedPreviewStart"),
            Some(&TagValue::Integer(i64::from(expected_start)))
        );
        assert_eq!(
            metadata.get("Olympus:ZoomedPreviewImage"),
            Some(&TagValue::Binary(expected))
        );
    }
}
