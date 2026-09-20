use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernote_dispatcher::dispatch_makernote;
use oxidex::parsers::tiff::makernotes::pentax::PentaxParser;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;
use std::process::Command;

fn tiff_with_makernote(make: &str, payload: &[u8]) -> Vec<u8> {
    const EXIF_IFD: usize = 38;
    const DATA_START: usize = 56;
    let make = [make.as_bytes(), b"\0"].concat();
    let make_offset = DATA_START;
    let makernote_offset = make_offset + make.len();
    let mut tiff = vec![0_u8; DATA_START];
    tiff[..8].copy_from_slice(b"II\x2a\0\x08\0\0\0");

    tiff[8..10].copy_from_slice(&2_u16.to_le_bytes());
    tiff[10..12].copy_from_slice(&0x010f_u16.to_le_bytes());
    tiff[12..14].copy_from_slice(&2_u16.to_le_bytes());
    tiff[14..18].copy_from_slice(&u32::try_from(make.len()).unwrap().to_le_bytes());
    tiff[18..22].copy_from_slice(&u32::try_from(make_offset).unwrap().to_le_bytes());
    tiff[22..24].copy_from_slice(&0x8769_u16.to_le_bytes());
    tiff[24..26].copy_from_slice(&4_u16.to_le_bytes());
    tiff[26..30].copy_from_slice(&1_u32.to_le_bytes());
    tiff[30..34].copy_from_slice(&(EXIF_IFD as u32).to_le_bytes());

    tiff[EXIF_IFD..EXIF_IFD + 2].copy_from_slice(&1_u16.to_le_bytes());
    tiff[EXIF_IFD + 2..EXIF_IFD + 4].copy_from_slice(&0x927c_u16.to_le_bytes());
    tiff[EXIF_IFD + 4..EXIF_IFD + 6].copy_from_slice(&7_u16.to_le_bytes());
    tiff[EXIF_IFD + 6..EXIF_IFD + 10]
        .copy_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    tiff[EXIF_IFD + 10..EXIF_IFD + 14]
        .copy_from_slice(&u32::try_from(makernote_offset).unwrap().to_le_bytes());
    tiff.extend_from_slice(&make);
    tiff.extend_from_slice(payload);
    tiff
}

fn read_tiff_makernote(make: &str, payload: &[u8]) -> oxidex::core::MetadataMap {
    let file = tempfile::Builder::new()
        .suffix(".tif")
        .tempfile()
        .expect("create synthetic Panasonic TIFF");
    std::fs::write(file.path(), tiff_with_makernote(make, payload))
        .expect("write synthetic Panasonic TIFF");
    oxidex::core::operations::read_metadata(file.path()).expect("synthetic Panasonic TIFF parses")
}

fn pentax_caf_tags(grid: u8, point_bytes: &[u8]) -> HashMap<String, String> {
    let mut raw = vec![0, grid];
    raw.extend_from_slice(point_bytes);
    assert!(raw.len() <= 4, "focused fixture stores the value inline");
    let mut inline = [0_u8; 4];
    inline[..raw.len()].copy_from_slice(&raw);

    let mut data = b"PENTAX \0II".to_vec();
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&0x0238u16.to_le_bytes());
    data.extend_from_slice(&7u16.to_le_bytes());
    data.extend_from_slice(&u32::try_from(raw.len()).unwrap().to_le_bytes());
    data.extend_from_slice(&inline);
    data.extend_from_slice(&0u32.to_le_bytes());

    let mut tags = HashMap::new();
    PentaxParser::default()
        .parse(&data, ByteOrder::LittleEndian, &mut tags)
        .expect("synthetic Pentax CAF MakerNote parses");
    tags
}

/// `Pentax::CAFPointInfo` stores four two-bit point states in each byte. The
/// pinned source tests mask `0x02` and mask `0x03` independently, so every
/// state sharing a set mask bit is reported; points are one-based and packed
/// most-significant pair first.
#[test]
fn pentax_caf_point_info_decodes_focus_and_selected_points() {
    let tags = pentax_caf_tags(0x22, &[0xb2]);

    assert_eq!(tags.get("Pentax:NumCAFPoints"), Some(&"4".to_string()));
    assert_eq!(tags.get("Pentax:CAFGridSize"), Some(&"2x2".to_string()));
    assert_eq!(
        tags.get("Pentax:CAFPointsInFocus"),
        Some(&"1,2,4".to_string())
    );
    assert_eq!(
        tags.get("Pentax:CAFPointsSelected"),
        Some(&"1,2,4".to_string())
    );
}

#[test]
fn pentax_caf_masks_accept_every_source_state_with_a_set_bit() {
    for (state, expected_focus, expected_selected) in
        [(1_u8, "", "1"), (2, "1", "1"), (3, "1", "1")]
    {
        let tags = pentax_caf_tags(0x11, &[state << 6]);
        assert_eq!(
            tags["Pentax:CAFPointsInFocus"], expected_focus,
            "state {state}"
        );
        assert_eq!(
            tags["Pentax:CAFPointsSelected"], expected_selected,
            "state {state}"
        );
    }

    let empty = pentax_caf_tags(0, &[]);
    assert_eq!(empty["Pentax:CAFPointsInFocus"], "(none)");
    assert_eq!(empty["Pentax:CAFPointsSelected"], "(none)");
}

#[test]
#[ignore = "direct replay requires EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL"]
fn pentax_caf_masks_match_the_pinned_oracle_directly() {
    let perl = std::env::var("EXIFTOOL_PERL").expect("set canonical Perl path");
    let exiftool = std::env::var("OXIDEX_PINNED_EXIFTOOL").expect("set pinned ExifTool tree");
    let script = r#"
        my ($value, $points) = @ARGV;
        print Image::ExifTool::Pentax::DecodeAFPoints($value, $points, 2, 2), "\n";
        print Image::ExifTool::Pentax::DecodeAFPoints($value, $points, 2, 3), "\n";
    "#;

    for (grid, point_bytes, oracle_value, point_count) in [
        (0_u8, &[][..], "", "0"),
        (0x11, &[0x40][..], "64", "1"),
        (0x11, &[0x80][..], "128", "1"),
        (0x11, &[0xc0][..], "192", "1"),
        (0x22, &[0xb2][..], "178", "4"),
    ] {
        let output = Command::new(&perl)
            .arg(format!("-I{exiftool}/lib"))
            .arg("-MImage::ExifTool::Pentax")
            .arg("-e")
            .arg(script)
            .arg(oracle_value)
            .arg(point_count)
            .output()
            .expect("run pinned Pentax oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let oracle = String::from_utf8(output.stdout).expect("oracle emits UTF-8");
        let tags = pentax_caf_tags(grid, point_bytes);
        assert_eq!(
            oracle,
            format!(
                "{}\n{}\n",
                tags["Pentax:CAFPointsInFocus"], tags["Pentax:CAFPointsSelected"]
            )
        );
    }
}

/// Panasonic's legacy `MKE*` MakerNote is a fixed `Panasonic::Type2` binary
/// record, not a TIFF IFD.  The generated 13.59 table owns its layout:
/// string[4] at offset 0 and an int16u Gain at offset 6.
#[test]
fn panasonic_mke_type2_uses_generated_binary_layout() {
    let mut tags = HashMap::new();
    oxidex::parsers::tiff::makernotes::panasonic::parse_panasonic_makernotes(
        b"MKE\0\0\0\x34\x12",
        ByteOrder::BigEndian,
        &mut tags,
    );

    assert_eq!(
        tags.get("Panasonic:MakerNoteType"),
        Some(&"MKE".to_string())
    );
    assert_eq!(tags.get("Panasonic:Gain"), Some(&"4660".to_string()));
}

#[test]
fn panasonic_type2_dispatch_requires_a_panasonic_make() {
    let payload = b"MKE\0\0\0\x34\x12";
    let mut panasonic = HashMap::new();
    dispatch_makernote(
        "Panasonic Corporation",
        payload,
        ByteOrder::BigEndian,
        &mut panasonic,
    )
    .expect("Panasonic Type2 dispatch succeeds");
    assert_eq!(
        panasonic.get("Panasonic:MakerNoteType"),
        Some(&"MKE".to_string())
    );
    assert_eq!(panasonic.get("Panasonic:Gain"), Some(&"4660".to_string()));

    for make in ["LEICA", "Leica Camera AG", "Not Panasonic"] {
        let mut tags = HashMap::new();
        dispatch_makernote(make, payload, ByteOrder::BigEndian, &mut tags)
            .expect("non-Panasonic MKE is withheld without a parse error");
        assert!(
            !tags.keys().any(|key| key.starts_with("Panasonic:")),
            "{make} must not enter Panasonic::Type2: {tags:?}"
        );
    }
}

#[test]
fn panasonic_type2_public_reader_uses_a_case_sensitive_make_gate() {
    let payload = b"MKE\0\0\0\x34\x12";
    let normal = read_tiff_makernote("Panasonic Corporation", payload);
    assert_eq!(normal.get_string("Panasonic:MakerNoteType"), Some("MKE"));
    assert_eq!(normal.get_string("Panasonic:Gain"), Some("4660"));

    for make in ["panasonic corporation", "PANASONIC CORPORATION"] {
        let metadata = read_tiff_makernote(make, payload);
        assert!(
            metadata.get("Panasonic:MakerNoteType").is_none(),
            "{make} must not enter Panasonic::Type2"
        );
        assert!(metadata.get("Panasonic:Gain").is_none());
    }
}
