use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::nikon::sub_tables::parse_maker_notes_0x56;
use std::collections::HashMap;
use std::process::Command;

fn tiff_with_nikon_0x56(record: &[u8; 16]) -> Vec<u8> {
    const EXIF_IFD: usize = 38;
    const MAKE: usize = 56;
    const MAKERNOTE: usize = 74;
    const EMBEDDED_TIFF: usize = MAKERNOTE + 10;
    const RECORD: usize = EMBEDDED_TIFF + 26;
    let mut tiff = vec![0_u8; RECORD];
    tiff[..8].copy_from_slice(b"II\x2a\0\x08\0\0\0");

    tiff[8..10].copy_from_slice(&2_u16.to_le_bytes());
    tiff[10..12].copy_from_slice(&0x010f_u16.to_le_bytes());
    tiff[12..14].copy_from_slice(&2_u16.to_le_bytes());
    tiff[14..18].copy_from_slice(&18_u32.to_le_bytes());
    tiff[18..22].copy_from_slice(&(MAKE as u32).to_le_bytes());
    tiff[22..24].copy_from_slice(&0x8769_u16.to_le_bytes());
    tiff[24..26].copy_from_slice(&4_u16.to_le_bytes());
    tiff[26..30].copy_from_slice(&1_u32.to_le_bytes());
    tiff[30..34].copy_from_slice(&(EXIF_IFD as u32).to_le_bytes());

    tiff[EXIF_IFD..EXIF_IFD + 2].copy_from_slice(&1_u16.to_le_bytes());
    tiff[EXIF_IFD + 2..EXIF_IFD + 4].copy_from_slice(&0x927c_u16.to_le_bytes());
    tiff[EXIF_IFD + 4..EXIF_IFD + 6].copy_from_slice(&7_u16.to_le_bytes());
    tiff[EXIF_IFD + 6..EXIF_IFD + 10].copy_from_slice(&52_u32.to_le_bytes());
    tiff[EXIF_IFD + 10..EXIF_IFD + 14].copy_from_slice(&(MAKERNOTE as u32).to_le_bytes());

    tiff[MAKE..MAKERNOTE].copy_from_slice(b"NIKON CORPORATION\0");
    tiff[MAKERNOTE..EMBEDDED_TIFF].copy_from_slice(b"Nikon\0\x02\0\0\0");
    tiff[EMBEDDED_TIFF..EMBEDDED_TIFF + 8].copy_from_slice(b"II\x2a\0\x08\0\0\0");
    tiff[EMBEDDED_TIFF + 8..EMBEDDED_TIFF + 10].copy_from_slice(&1_u16.to_le_bytes());
    tiff[EMBEDDED_TIFF + 10..EMBEDDED_TIFF + 12].copy_from_slice(&0x0056_u16.to_le_bytes());
    tiff[EMBEDDED_TIFF + 12..EMBEDDED_TIFF + 14].copy_from_slice(&7_u16.to_le_bytes());
    tiff[EMBEDDED_TIFF + 14..EMBEDDED_TIFF + 18].copy_from_slice(&16_u32.to_le_bytes());
    tiff[EMBEDDED_TIFF + 18..EMBEDDED_TIFF + 22].copy_from_slice(&26_u32.to_le_bytes());
    tiff.extend_from_slice(record);
    tiff
}

fn read_nikon_0x56(record: &[u8; 16]) -> oxidex::core::MetadataMap {
    let file = tempfile::Builder::new()
        .suffix(".tif")
        .tempfile()
        .expect("create synthetic Nikon TIFF");
    std::fs::write(file.path(), tiff_with_nikon_0x56(record)).expect("write synthetic Nikon TIFF");
    oxidex::core::operations::read_metadata(file.path()).expect("synthetic Nikon TIFF parses")
}

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
fn nikon_public_reader_reads_pixel_shift_active_as_one_byte() {
    let metadata = read_nikon_0x56(&[b'A', b'1', b'2', b'3', 1, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0]);

    assert_eq!(
        metadata.get_string("Nikon:PixelShiftActive"),
        Some("Unknown (2)")
    );
}

#[test]
fn nikon_public_reader_retains_unknown_burst_start_image_type() {
    let metadata = read_nikon_0x56(&[b'A', b'1', b'2', b'3', 1, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0]);

    assert_eq!(
        metadata.get_string("Nikon:BurstStartImageType"),
        Some("Unknown (1)")
    );
}

#[test]
#[ignore = "direct replay requires EXIFTOOL_PERL and OXIDEX_PINNED_EXIFTOOL"]
fn nikon_maker_notes_0x56_unusual_values_match_the_pinned_oracle() {
    let perl = std::env::var("EXIFTOOL_PERL").expect("set canonical Perl path");
    let exiftool = std::env::var("OXIDEX_PINNED_EXIFTOOL").expect("set pinned ExifTool tree");
    let script = r#"
        my $et = Image::ExifTool->new;
        my $data = 'A123' . pack('V2', 1, 0) . pack('C4', 2, 1, 0, 0);
        my %dir = (DataPt => \$data, DirStart => 0, DirLen => length($data), DirName => 'Nikon');
        Image::ExifTool::SetByteOrder('II');
        Image::ExifTool::ProcessBinaryData($et, \%dir, \%Image::ExifTool::Nikon::MakerNotes0x56);
        print $et->GetValue('FirmwareVersion56'), "\n",
            $et->GetValue('BurstStartImageType'), "\n",
            $et->GetValue('PixelShiftActive'), "\n";
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
        &[b'A', b'1', b'2', b'3', 1, 0, 0, 0, 0, 0, 0, 0, 2, 1, 0, 0],
        ByteOrder::LittleEndian,
        &mut tags,
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("oracle emits UTF-8"),
        format!(
            "{}\n{}\n{}\n",
            tags["Nikon:FirmwareVersion56"],
            tags["Nikon:BurstStartImageType"],
            tags["Nikon:PixelShiftActive"]
        )
    );
}
