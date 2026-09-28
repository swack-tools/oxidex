use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::exiftool_tables::EXIFTOOL_VERSION;
use tempfile::Builder;

fn entry(tag: u16, field_type: u16, count: u32, value: u32) -> [u8; 12] {
    let mut row = [0; 12];
    row[..2].copy_from_slice(&tag.to_le_bytes());
    row[2..4].copy_from_slice(&field_type.to_le_bytes());
    row[4..8].copy_from_slice(&count.to_le_bytes());
    row[8..12].copy_from_slice(&value.to_le_bytes());
    row
}

#[test]
fn rw2_outer_ifd_names_follow_selected_panasonic_raw_main() {
    // Native 11.78/12.64 report ISO=100 from 0x0017 but omit 0x0037.
    // Native 13.59 reports both occurrences. Artist is declared in 12.64+.
    let mut data = b"II\x55\0\x08\0\0\0".to_vec();
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&entry(1, 7, 4, u32::from_le_bytes(*b"0300")));
    data.extend_from_slice(&entry(0x17, 3, 1, 100));
    data.extend_from_slice(&entry(0x37, 4, 1, 200));
    data.extend_from_slice(&entry(0x13b, 2, 4, u32::from_le_bytes(*b"Art\0")));
    data.extend_from_slice(&0u32.to_le_bytes());

    let file = Builder::new().suffix(".rw2").tempfile().unwrap();
    std::fs::write(file.path(), data).unwrap();
    let metadata = read_metadata(file.path()).unwrap();
    assert_eq!(
        metadata.get_string("IFD0:PanasonicRawVersion"),
        Some("0300")
    );
    let iso_occurrences = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(key, _, _)| *key == "IFD0:ISO")
        .count();
    match EXIFTOOL_VERSION {
        "11.78" => {
            assert_eq!(metadata.get_integer("IFD0:ISO"), Some(100));
            assert_eq!(iso_occurrences, 1);
            assert_eq!(metadata.get_string("IFD0:Artist"), None);
        }
        "12.64" => {
            assert_eq!(metadata.get_integer("IFD0:ISO"), Some(100));
            assert_eq!(iso_occurrences, 1);
            assert_eq!(metadata.get_string("IFD0:Artist"), Some("Art"));
        }
        "13.59" => {
            assert_eq!(metadata.get_integer("IFD0:ISO"), Some(200));
            assert_eq!(iso_occurrences, 2);
            assert_eq!(metadata.get_string("IFD0:Artist"), Some("Art"));
        }
        other => panic!("unreviewed PanasonicRaw source: {other}"),
    }
}

#[test]
fn dng_exif_ifd_omits_rows_absent_from_selected_exif_main() {
    let mut data = b"II\x2a\0\x08\0\0\0".to_vec();
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&entry(0x8769, 4, 1, 38));
    data.extend_from_slice(&entry(0xc612, 1, 4, 0x00000401));
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&entry(0x9287, 3, 5, 68));
    data.extend_from_slice(&entry(0x829a, 5, 1, 78));
    data.extend_from_slice(&0u32.to_le_bytes());
    for value in [2u16, 1, 0, 4, 1] {
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&125u32.to_le_bytes());

    let file = Builder::new().suffix(".dng").tempfile().unwrap();
    std::fs::write(file.path(), data).unwrap();
    let metadata = read_metadata(file.path()).unwrap();
    assert_eq!(metadata.get_string("ExifIFD:ExposureTime"), Some("1/125"));
    match EXIFTOOL_VERSION {
        "11.78" | "12.64" => {
            assert_eq!(metadata.get_string("ExifIFD:LearningOptOutIn"), None);
        }
        "13.59" => assert_eq!(
            metadata.get_string("ExifIFD:LearningOptOutIn"),
            Some(
                "Non-Generative AI/ML Training; Opt-out; Input to Foundation Model (Trained AI/ML Model); Opt-in"
            )
        ),
        other => panic!("unreviewed Exif source: {other}"),
    }
}
