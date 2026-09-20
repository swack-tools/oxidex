use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::nikon::sub_tables::parse_maker_notes_0x56;
use std::collections::HashMap;
use std::process::Command;

/// Nikon.pm's `MakerNotes0x56` has a fixed Z-series record layout.  Its
/// firmware, packed burst fields, and independent pixel-shift flag are
/// source-derived fields; a missing parser must not collapse this carrier to
/// an opaque binary blob.
#[test]
fn nikon_maker_notes_0x56_decodes_burst_and_pixel_shift() {
    let data = [
        b'0', b'1', b'0', b'0', 0xa0, 0xe7, 0x30, 0x03, 1, 0, 0, 0, 1, 0, 0, 0,
    ];
    let mut tags = HashMap::new();

    parse_maker_notes_0x56(&data, ByteOrder::LittleEndian, &mut tags);

    assert_eq!(
        tags.get("Nikon:FirmwareVersion56"),
        Some(&"01.00".to_string())
    );
    assert_eq!(
        tags.get("Nikon:BurstStartSlotNumber"),
        Some(&"1".to_string())
    );
    assert_eq!(
        tags.get("Nikon:BurstStartFolderNumber"),
        Some(&"102".to_string())
    );
    assert_eq!(
        tags.get("Nikon:BurstStartImageNumber"),
        Some(&"1853".to_string())
    );
    assert_eq!(
        tags.get("Nikon:BurstStartImageType"),
        Some(&"JPG".to_string())
    );
    assert_eq!(tags.get("Nikon:BurstShotNumber"), Some(&"1".to_string()));
    assert_eq!(tags.get("Nikon:PixelShiftActive"), Some(&"Yes".to_string()));
}

#[test]
fn nikon_maker_notes_0x56_keeps_source_conversions_for_unusual_values() {
    let data = [b'A', b'1', b'2', b'3', 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0];
    let mut tags = HashMap::new();

    parse_maker_notes_0x56(&data, ByteOrder::LittleEndian, &mut tags);

    // Pinned Nikon.pm applies `s/(\d{2})/$1./` to every string[4], not only
    // four-digit versions, and generic hash PrintConv retains unknown values.
    assert_eq!(
        tags.get("Nikon:FirmwareVersion56"),
        Some(&"A12.3".to_string())
    );
    assert_eq!(
        tags.get("Nikon:PixelShiftActive"),
        Some(&"Unknown (2)".to_string())
    );
}

#[test]
#[ignore = "direct replay requires EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL"]
fn nikon_maker_notes_0x56_unusual_values_match_the_pinned_oracle() {
    let perl = std::env::var("EXIFTOOL_PERL").expect("set canonical Perl path");
    let exiftool = std::env::var("OXIDEX_PINNED_EXIFTOOL").expect("set pinned ExifTool tree");
    let script = r#"
        my $et = Image::ExifTool->new;
        my $data = 'A123' . pack('V3', 0, 0, 2);
        my %dir = (DataPt => \$data, DirStart => 0, DirLen => length($data), DirName => 'Nikon');
        Image::ExifTool::ProcessBinaryData($et, \%dir, \%Image::ExifTool::Nikon::MakerNotes0x56);
        print $et->GetValue('FirmwareVersion56'), "\n", $et->GetValue('PixelShiftActive'), "\n";
    "#;
    let output = Command::new(perl)
        .arg(format!("-I{exiftool}/lib"))
        .arg("-MImage::ExifTool")
        .arg("-MImage::ExifTool::Nikon")
        .arg("-e")
        .arg(script)
        .output()
        .expect("run pinned Nikon oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut tags = HashMap::new();
    parse_maker_notes_0x56(
        &[b'A', b'1', b'2', b'3', 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0],
        ByteOrder::LittleEndian,
        &mut tags,
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("oracle emits UTF-8"),
        format!(
            "{}\n{}\n",
            tags["Nikon:FirmwareVersion56"], tags["Nikon:PixelShiftActive"]
        )
    );
}
