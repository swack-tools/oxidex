//! Shared-reader migration of Sony::Tag202a. Expected values were obtained
//! from the pinned native reader on complete TIFF carriers, including short
//! records, rejected signatures and both byte orders. Native-source mutation
//! tests separately protect the generated layout and selection rules.

use oxidex::core::operations::read_metadata;
use oxidex::exiftool_tables::find_table;
use std::collections::BTreeMap;

fn word(data: &mut [u8], at: usize, value: u16, big: bool) {
    data[at..at + 2].copy_from_slice(&if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    });
}

fn long(data: &mut [u8], at: usize, value: u32, big: bool) {
    data[at..at + 4].copy_from_slice(&if big {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    });
}

fn ifd(data: &mut [u8], at: usize, entries: &[(u16, u16, u32, u32)], big: bool) {
    word(data, at, entries.len() as u16, big);
    for (index, &(tag, format, count, value)) in entries.iter().enumerate() {
        let entry = at + 2 + index * 12;
        word(data, entry, tag, big);
        word(data, entry + 2, format, big);
        long(data, entry + 4, count, big);
        long(data, entry + 8, value, big);
    }
    long(data, at + 2 + entries.len() * 12, 0, big);
}

fn carrier(locations: u8, length: usize, big: bool, signature: u8) -> Vec<u8> {
    let mut data = vec![0; 322];
    data[..2].copy_from_slice(if big { b"MM" } else { b"II" });
    word(&mut data, 2, 42, big);
    long(&mut data, 4, 8, big);
    ifd(
        &mut data,
        8,
        &[(0x010f, 2, 5, 64), (0x0110, 2, 10, 72), (0x8769, 4, 1, 96)],
        big,
    );
    data[64..69].copy_from_slice(b"SONY\0");
    data[72..82].copy_from_slice(b"ILCE-6300\0");
    ifd(&mut data, 96, &[(0x927c, 7, 32 + length as u32, 224)], big);
    data[224..236].copy_from_slice(b"SONY DSC \0\0\0");
    ifd(&mut data, 236, &[(0x202a, 7, length as u32, 256)], big);
    data[256] = signature;
    data[257] = locations;
    word(&mut data, 258, 640, big);
    word(&mut data, 260, 428, big);
    for index in 0..15 {
        word(&mut data, 262 + index * 4, 11 * (index as u16 + 1), big);
        word(&mut data, 264 + index * 4, 7 * (index as u16 + 1), big);
    }
    data.truncate(256 + length);
    data
}

fn focus_tags(data: &[u8]) -> BTreeMap<String, String> {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("focus.tif");
    std::fs::write(&path, data).unwrap();
    let metadata = read_metadata(&path).expect("read complete TIFF carrier");
    let mut found = BTreeMap::new();
    for name in [
        "FocalPlaneAFPointsUsed".to_owned(),
        "FocalPlaneAFPointArea".to_owned(),
    ]
    .into_iter()
    .chain((1..=15).map(|index| format!("FocalPlaneAFPointLocation{index}")))
    {
        if let Some(value) = metadata.get(&format!("Sony:{name}")) {
            let text = value
                .as_string()
                .map(str::to_string)
                .or_else(|| value.as_integer().map(|n| n.to_string()))
                .expect("native scalar or space-joined coordinates");
            found.insert(name, text);
        }
    }
    found
}

#[test]
fn focus_table_uses_the_shared_path_without_a_hand_root_rule() {
    let table = find_table("Sony", "Tag202a").expect("native table is generated");
    assert!(
        table.enabled(),
        "migration requires the measured shared route"
    );
    assert_eq!(table.fields.len(), 17);
    assert!(
        !oxidex::parsers::tiff::makernotes::sony::enciphered::is_root_tag(0x202a),
        "the hand root selection must be retired"
    );
}

#[test]
fn native_saved_count_controls_later_fields_in_both_byte_orders() {
    for big in [false, true] {
        for locations in [0, 1, 15] {
            let found = focus_tags(&carrier(locations, 66, big, 1));
            let mut expected =
                BTreeMap::from([("FocalPlaneAFPointsUsed".to_owned(), locations.to_string())]);
            if locations > 0 {
                expected.insert("FocalPlaneAFPointArea".to_owned(), "640 428".to_owned());
            }
            for index in 1..=locations {
                expected.insert(
                    format!("FocalPlaneAFPointLocation{index}"),
                    format!("{} {}", u16::from(index) * 11, u16::from(index) * 7),
                );
            }
            assert_eq!(found, expected, "big={big}, locations={locations}");
        }
    }
}

#[test]
fn native_short_reads_and_parent_rejection_are_preserved() {
    for big in [false, true] {
        assert!(focus_tags(&carrier(15, 66, big, 2)).is_empty());
        let area_only = focus_tags(&carrier(1, 6, big, 1));
        assert_eq!(area_only.len(), 2);
        assert_eq!(area_only["FocalPlaneAFPointArea"], "640 428");
        let one_location = focus_tags(&carrier(15, 10, big, 1));
        assert_eq!(one_location.len(), 3);
        assert_eq!(one_location["FocalPlaneAFPointLocation1"], "11 7");
        let partial_pair = focus_tags(&carrier(15, 65, big, 1));
        assert_eq!(partial_pair.len(), 17);
        assert_eq!(partial_pair["FocalPlaneAFPointLocation15"], "165");
    }
}
