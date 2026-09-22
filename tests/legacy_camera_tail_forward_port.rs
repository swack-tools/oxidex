//! Task 15 source-backed controls for legacy camera MakerNote rows that are
//! reachable through the existing per-family routes.

use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::{TagId, TagOccurrence, TagValue};

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
