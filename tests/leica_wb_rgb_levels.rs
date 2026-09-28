//! Source-backed Leica5 white-balance levels and their two EXIF composites.

use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::{TagId, TagValue};
use std::fs;

#[path = "common/fixtures.rs"]
mod fixtures;

#[test]
#[ignore = "requires pinned combined-samples/Leica/LeicaX1.jpg"]
fn leica_x1_levels_and_balances_match_pinned_1359() {
    let path = fixtures::required_combined_fixture_path("Leica/LeicaX1.jpg");
    let metadata = read_metadata(&path).expect("LeicaX1.jpg parses");
    assert_eq!(
        metadata.get_string("Leica:WB_RGBLevels"),
        Some("0.5182186235 1 0.7231638418")
    );
    let rows: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(key, _, _)| *key == "Leica:WB_RGBLevels")
        .collect();
    assert_eq!(rows.len(), 1);
    let (_, occurrence, printed) = &rows[0];
    assert_eq!(occurrence.id, TagId::Numeric(0x0413));
    assert_eq!(occurrence.group0.as_ref(), "MakerNotes");
    assert_eq!(occurrence.group1.as_ref(), "Leica");
    assert_eq!(occurrence.group2.as_deref(), Some("Camera"));
    assert_eq!(occurrence.origin.module, Some("Panasonic"));
    assert_eq!(occurrence.origin.table, Some("Leica5"));
    assert!(occurrence.origin.byte_range.is_some());
    assert_eq!(
        printed.as_ref(),
        &TagValue::String("0.5182186235 1 0.7231638418".into())
    );
    assert_eq!(
        occurrence.value,
        Some(TagValue::String("0.5182186235 1 0.7231638418".into()))
    );
    assert_eq!(
        occurrence.stored,
        Some(TagValue::Array(vec![
            TagValue::Rational {
                numerator: 256,
                denominator: 494
            },
            TagValue::Rational {
                numerator: 512,
                denominator: 512
            },
            TagValue::Rational {
                numerator: 256,
                denominator: 354
            },
        ]))
    );
    assert_eq!(
        metadata.get_string("Composite:RedBalance"),
        Some("0.518219")
    );
    assert_eq!(
        metadata.get_string("Composite:BlueBalance"),
        Some("0.723164")
    );
}

#[test]
#[ignore = "requires pinned combined-samples/Leica/LeicaX1.jpg"]
fn leica_x1_zero_denominators_match_pinned_1359() {
    let path = fixtures::required_combined_fixture_path("Leica/LeicaX1.jpg");
    let original = fs::read(path).expect("read LeicaX1.jpg");
    // The `-v3` pinned 13.59 oracle locates 0x0413's 24 value bytes here.
    const VALUES: usize = 0x043e;
    assert_eq!(
        &original[VALUES..VALUES + 24],
        &[
            0, 1, 0, 0, 238, 1, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 98, 1, 0, 0,
        ]
    );
    for (which, edit, expected_levels, expected_red, expected_blue) in [
        (
            "middle denominator zero",
            VALUES + 12..VALUES + 16,
            "0.5182186235 inf 0.7231638418",
            Some("0"),
            Some("0"),
        ),
        (
            "first numerator and denominator zero",
            VALUES..VALUES + 8,
            "undef 1 0.7231638418",
            Some("0"),
            Some("0.723164"),
        ),
    ] {
        let mut data = original.clone();
        data[edit].fill(0);
        let file = tempfile::Builder::new()
            .suffix(".jpg")
            .tempfile()
            .expect("temporary JPEG");
        fs::write(file.path(), data).expect("write mutated JPEG");
        let metadata = read_metadata(file.path()).expect(which);
        assert_eq!(
            metadata.get_string("Leica:WB_RGBLevels"),
            Some(expected_levels),
            "{which}"
        );
        assert_eq!(
            metadata.get_string("Composite:RedBalance"),
            expected_red,
            "{which}"
        );
        assert_eq!(
            metadata.get_string("Composite:BlueBalance"),
            expected_blue,
            "{which}"
        );
    }
}
