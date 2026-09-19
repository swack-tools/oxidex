//! Integration tests for Sony MakerNotes parser
//!
//! Every expected value here comes from real ExifTool output on a corpus
//! sample, quoted in the test that uses it. The Sony `PrintConv` space is dense
//! with near-identical strings, so an assertion invented to match the
//! implementation would look exactly like one that matches the camera.

use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use oxidex::parsers::tiff::makernotes::sony::{SonyParser, parse_sony_makernote};
use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;
use std::collections::HashMap;

/// Builds a headerless little-endian Sony MakerNote from `(tag, type, count,
/// value)` entries, followed by `trailing` bytes for any out-of-line values.
fn build_makernote(entries: &[(u16, u16, u32, u32)], trailing: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, ty, count, value) in entries {
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&ty.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        data.extend_from_slice(&value.to_le_bytes());
    }
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(trailing);
    data
}

fn parse(data: &[u8]) -> HashMap<String, String> {
    let mut tags = HashMap::new();
    parse_sony_makernote(data, ByteOrder::LittleEndian, &mut tags);
    tags
}

#[test]
fn test_sony_lens_database_uses_exiftool_spellings() {
    // `%Image::ExifTool::Sony::sonyLensTypes`. The spellings are ExifTool's
    // ("F2.8", not "f/2.8") because parity is compared character-for-character.
    assert_eq!(
        lookup_lens_name(11),
        Some("Minolta AF 300mm F4 HS-APO G".to_string())
    );
    assert_eq!(
        lookup_lens_name(33),
        Some("Minolta/Sony AF 70-200mm F2.8 G".to_string())
    );
    assert_eq!(
        lookup_lens_name(63),
        Some("Sony DT 16-50mm F2.8 SSM (SAL1650)".to_string())
    );
    assert_eq!(
        lookup_lens_name(25501),
        Some("Minolta AF 50mm F1.7".to_string())
    );
}

#[test]
fn test_sony_lens_database_disambiguation_strings_are_preserved() {
    // Several ids cover more than one lens; ExifTool prints the whole "or"
    // string rather than picking one, and so must we.
    assert_eq!(
        lookup_lens_name(25),
        Some("Minolta AF 100-300mm F4.5-5.6 APO (D) or Sigma Lens".to_string())
    );
    assert_eq!(
        lookup_lens_name(128),
        Some("Tamron or Sigma Lens (128)".to_string())
    );
    assert_eq!(
        lookup_lens_name(65535),
        Some("E-Mount, T-Mount, Other Lens or no lens".to_string())
    );
}

#[test]
fn test_sony_lens_database_unknown() {
    // Ids ExifTool has no name for must stay unresolved rather than fall back
    // to a neighbouring lens.
    assert_eq!(lookup_lens_name(60000), None);
    assert_eq!(lookup_lens_name(9999), None);
}

#[test]
fn test_sony_is_sony_makernote() {
    use oxidex::parsers::tiff::makernotes::sony::is_sony_makernote;

    assert!(is_sony_makernote(b"SONY\x05\x00"));
    assert!(is_sony_makernote(b"\x05\x00"));
    assert!(is_sony_makernote(b"\x00\x05"));
    assert!(!is_sony_makernote(b"\xFF\xFF"));
    assert!(!is_sony_makernote(b"\x01"));
    assert!(!is_sony_makernote(b"\x00\x00"));
}

#[test]
fn test_sony_inline_tags_carry_their_printconv() {
    // SonyDSLR-A350.jpg: `[Sony] Quality : Fine` (0x0102 = 2),
    // `[Sony] WhiteBalance : Custom` (0x0115 = 0x70),
    // `[Sony] Teleconverter : None` (0x0105 = 0).
    let tags = parse(&build_makernote(
        &[(0x0102, 4, 1, 2), (0x0115, 4, 1, 0x70), (0x0105, 4, 1, 0)],
        &[],
    ));
    assert_eq!(tags.get("Sony:Quality"), Some(&"Fine".to_string()));
    assert_eq!(tags.get("Sony:WhiteBalance"), Some(&"Custom".to_string()));
    assert_eq!(tags.get("Sony:Teleconverter"), Some(&"None".to_string()));
}

#[test]
fn test_sony_lens_type_resolves_through_the_lens_table() {
    // SonyDSLR-A350.jpg stores LensType 25 and exiftool reports the full
    // ambiguous name.
    let tags = parse(&build_makernote(&[(0xb027, 4, 1, 25)], &[]));
    assert_eq!(
        tags.get("Sony:LensType"),
        Some(&"Minolta AF 100-300mm F4.5-5.6 APO (D) or Sigma Lens".to_string())
    );
}

#[test]
fn test_sony_rawconv_drops_the_not_applicable_sentinel() {
    // 0xb047 JPEGQuality has `RawConv => '$val == 65535 ? undef : $val'`, so
    // SonySLT-A77.jpg - which stores 65535 - reports no JPEGQuality at all
    // rather than "n/a".
    let tags = parse(&build_makernote(&[(0xb047, 3, 1, 65535)], &[]));
    assert!(!tags.contains_key("Sony:JPEGQuality"));

    let tags = parse(&build_makernote(&[(0xb047, 3, 1, 2)], &[]));
    assert_eq!(
        tags.get("Sony:JPEGQuality"),
        Some(&"Extra Fine".to_string())
    );
}

#[test]
fn test_sony_unknown_tags_are_not_invented() {
    // 0x2003 and 0x200c are named `Sony_0xNNNN` and flagged Unknown, so
    // ExifTool never prints them. Emitting a `Sony:Tag2003` would add a key no
    // comparison can ever match.
    let tags = parse(&build_makernote(
        &[(0x2003, 3, 1, 7), (0x200c, 4, 1, 9)],
        &[],
    ));
    assert!(tags.is_empty(), "unexpected tags: {:?}", tags);
}

#[test]
fn test_sony_out_of_line_values_need_the_tiff_base() {
    // 0xb020 CreativeStyle is a 16-byte string, so its bytes live outside the
    // IFD entry at a TIFF-relative offset. Without the base the offset cannot
    // be turned into an index, and reading from a guessed one would report
    // whatever happened to sit there.
    let mut trailing = vec![0u8; 2];
    trailing.extend_from_slice(b"Standard\0\0\0\0\0\0\0\0");
    let data = build_makernote(&[(0xb020, 2, 16, 1000 + 20)], &trailing);

    assert!(!parse(&data).contains_key("Sony:CreativeStyle"));

    // The MakerNote sits 1000 bytes into its enclosing TIFF block, which is
    // what its entries' offsets are measured from.
    let mut tiff = vec![0u8; 1000];
    let payload_len = data.len();
    tiff.extend_from_slice(&data);
    let ctx = MakerNoteContext::in_tiff(&tiff, 1000, payload_len, 0);

    let mut tags = HashMap::new();
    SonyParser
        .parse_with_context(&ctx, ByteOrder::LittleEndian, Some("SLT-A77"), &mut tags)
        .unwrap();
    assert_eq!(
        tags.get("Sony:CreativeStyle"),
        Some(&"Standard".to_string())
    );
}

#[test]
fn test_sony_duplicate_names_resolve_by_priority() {
    // SonySLT-A77.jpg carries DynamicRangeOptimizer twice: 0xb025 = 3 ("Auto")
    // and 0xb04f = 1 ("Standard"). 0xb04f is `Priority => 0` - ExifTool's own
    // comment calls it "unreliable for the A77" - so "Auto" wins even though
    // 0xb04f is listed later.
    let tags = parse(&build_makernote(
        &[(0xb025, 4, 1, 3), (0xb04f, 3, 1, 1)],
        &[],
    ));
    assert_eq!(
        tags.get("Sony:DynamicRangeOptimizer"),
        Some(&"Auto".to_string())
    );
}

/// Parses `maker_note` as if it sat 1000 bytes into a TIFF block whose header
/// is `tiff_base` bytes into the file, so out-of-line values resolve.
fn parse_in_tiff(maker_note: &[u8], model: &str, tiff_base: u64) -> HashMap<String, String> {
    let mut tiff = vec![0u8; 1000];
    tiff.extend_from_slice(maker_note);
    let ctx = MakerNoteContext::in_tiff(&tiff, 1000, maker_note.len(), tiff_base);
    let mut tags = HashMap::new();
    SonyParser
        .parse_with_context(&ctx, ByteOrder::LittleEndian, Some(model), &mut tags)
        .unwrap();
    tags
}

#[test]
fn test_sony_more_settings_a550_fields_follow_exiftool_layout() {
    // Forward-ported from origin/main 9b215f03/badda311. `Sony::MoreInfo` is a
    // little-endian offset directory; its block id 1 selects
    // `Sony::MoreSettings`, whose A450/A500/A550 rows are 0x1a
    // (`int16uRev[2]`), 0x23 (`10 * 2 ** (($val-28)/16)`), 0x24 and 0x26
    // (`int16s / 8`), 0x28 (Orientation map) and 0x29 (raw) in Sony.pm 13.59.
    let mut more_info = vec![0u8; 20480];
    more_info[0..2].copy_from_slice(&1u16.to_le_bytes());
    more_info[2..4].copy_from_slice(&20480u16.to_le_bytes());
    more_info[4..6].copy_from_slice(&1u16.to_le_bytes());
    more_info[6..8].copy_from_slice(&8u16.to_le_bytes());
    more_info[8 + 0x1a..8 + 0x1e].copy_from_slice(&[0, 4, 0, 8]);
    more_info[8 + 0x23] = 44;
    more_info[8 + 0x24..8 + 0x26].copy_from_slice(&(-8i16).to_le_bytes());
    more_info[8 + 0x26..8 + 0x28].copy_from_slice(&4i16.to_le_bytes());
    more_info[8 + 0x28] = 6;
    more_info[8 + 0x29] = 190;

    let mut maker_note = build_makernote(&[(0x0020, 7, 20480, 1018)], &[]);
    maker_note.extend_from_slice(&more_info);
    let tags = parse_in_tiff(&maker_note, "DSLR-A550", 0);

    assert_eq!(tags.get("Sony:CustomWB_RBLevels"), Some(&"4 8".to_string()));
    assert_eq!(tags.get("Sony:FocalLength2"), Some(&"20.0 mm".to_string()));
    assert_eq!(
        tags.get("Sony:ExposureCompensation2"),
        Some(&"-1.0".to_string())
    );
    assert_eq!(
        tags.get("Sony:FlashExposureCompSet2"),
        Some(&"+0.5".to_string())
    );
    assert_eq!(
        tags.get("Sony:Orientation2"),
        Some(&"Rotate 90 CW".to_string())
    );
    assert_eq!(tags.get("Sony:FocusPosition2"), Some(&"190".to_string()));
    // The later-body arms at 0x20/0x7c are not A550 rows.
    assert!(!tags.contains_key("Sony:LiveViewAFMethod"));

    // The same bytes on an SLT body take the other alternatives instead.
    let mut more_info_slt = more_info.clone();
    more_info_slt[8 + 0x20] = 2;
    more_info_slt[8 + 0x7c] = 182;
    let mut maker_note = build_makernote(&[(0x0020, 7, 20480, 1018)], &[]);
    maker_note.extend_from_slice(&more_info_slt);
    let tags = parse_in_tiff(&maker_note, "SLT-A55V", 0);
    assert_eq!(
        tags.get("Sony:LiveViewAFMethod"),
        Some(&"Contrast AF".to_string())
    );
    assert_eq!(
        tags.get("Sony:FlashActionExternal"),
        Some(&"Fired, HSS".to_string())
    );
    assert!(!tags.contains_key("Sony:Orientation2"));
}

#[test]
fn test_sony_camera_info3_focal_length_tele_zoom() {
    // Forward-ported from origin/main badda311. 0x0010 with count 15360 is
    // `CameraInfo3` (Sony.pm:737-742); its 0x10 int16u is
    // `FocalLengthTeleZoom`, `$val * 2 / 3`, for every body but the
    // A450/A500/A550. 105 * 2 / 3 = 70.0, SonyDSLR-A580.jpg's value.
    let mut camera_info3 = vec![0u8; 15360];
    camera_info3[0x10..0x12].copy_from_slice(&105u16.to_le_bytes());
    let mut maker_note = build_makernote(&[(0x0010, 7, 15360, 1018)], &[]);
    maker_note.extend_from_slice(&camera_info3);

    let tags = parse_in_tiff(&maker_note, "DSLR-A580", 0);
    assert_eq!(
        tags.get("Sony:FocalLengthTeleZoom"),
        Some(&"70.0 mm".to_string())
    );
    let tags = parse_in_tiff(&maker_note, "DSLR-A550", 0);
    assert!(!tags.contains_key("Sony:FocalLengthTeleZoom"));
}

#[test]
fn test_sony_pixel_shift_info_applies_rawconv_and_printconv() {
    // Forward-ported from origin/main 12a2b7d7. 0x202f is `undef[6]`: an
    // int32u group id then shot and total bytes. Expected strings computed by
    // running Sony.pm 13.59's RawConv and PrintConv code under perl 5.38.2 for
    // group 4862599 (0x1 << 22 | 05/03/10/07).
    let group = 4_862_599u32.to_le_bytes();
    let pixel_shift = |shot: u8, total: u8| {
        let mut value = group.to_vec();
        value.extend_from_slice(&[shot, total]);
        let mut maker_note = build_makernote(&[(0x202f, 7, 6, 1018)], &[]);
        maker_note.extend_from_slice(&value);
        parse_in_tiff(&maker_note, "ILCE-7RM4", 0)
    };
    assert_eq!(
        pixel_shift(2, 4).get("Sony:PixelShiftInfo"),
        Some(&"Group 05031007, Shot 2/4 (0x1)".to_string())
    );
    assert_eq!(
        pixel_shift(0, 4).get("Sony:PixelShiftInfo"),
        Some(&"Group 05031007, Composed 4-shot (0x1)".to_string())
    );

    // All-zero is ExifTool's explicit `n/a`.
    let mut maker_note = build_makernote(&[(0x202f, 7, 6, 1018)], &[]);
    maker_note.extend_from_slice(&[0; 6]);
    assert_eq!(
        parse_in_tiff(&maker_note, "ILCE-7RM4", 0).get("Sony:PixelShiftInfo"),
        Some(&"n/a".to_string())
    );
}

#[test]
fn test_sony_hidden_info_offset_is_file_relative() {
    // Forward-ported from origin/main 12a2b7d7. 0x2044 `HiddenInfo` is an
    // int32u[2] sub-directory; `HiddenDataOffset` is IsOffset, so ExifTool
    // adds the TIFF header's file position (12 in a JPEG's APP1).
    let mut value = Vec::new();
    value.extend_from_slice(&13_938_676u32.to_le_bytes());
    value.extend_from_slice(&53_248u32.to_le_bytes());
    let mut maker_note = build_makernote(&[(0x2044, 4, 2, 1018)], &[]);
    maker_note.extend_from_slice(&value);

    let tags = parse_in_tiff(&maker_note, "ZV-E10M2", 12);
    assert_eq!(
        tags.get("Sony:HiddenDataOffset"),
        Some(&"13938688".to_string())
    );
    assert_eq!(
        tags.get("Sony:HiddenDataLength"),
        Some(&"53248".to_string())
    );
}

#[test]
fn test_sony_pic_text_blocks_follow_process_sony_pic() {
    // Forward-ported from origin/main c8887915/badda311. `SONY PIC\0` is not
    // an IFD: ProcessSonyPIC keeps printable runs longer than 32 bytes as
    // binary `TextInfoN` tags and reads `/\b$key\s*([^\s;,:]+)/` fields out
    // of them.
    let mut data = b"SONY PIC\0\0\0\0\x01\x02\x03".to_vec();
    let first = b"BC: A0D9P7016135IP1XYZ ; Temp:Clbt:28, padding padding";
    data.extend_from_slice(first);
    data.extend_from_slice(&[0, 0xff, 0]);
    // Too short to be a TextInfo block.
    data.extend_from_slice(b"barcode:SHORT");
    data.extend_from_slice(&[0]);
    let second = b"AFLOG xBC:IGNORED BarCode:  0123456789ABCDEF trailing text";
    data.extend_from_slice(second);

    let mut tags = HashMap::new();
    SonyParser
        .parse(&data, ByteOrder::LittleEndian, &mut tags)
        .unwrap();

    assert_eq!(
        tags.get("Sony:TextInfo1"),
        Some(&format!(
            "(Binary data {} bytes, use -b option to extract)",
            first.len()
        ))
    );
    assert_eq!(
        tags.get("Sony:TextInfo2"),
        Some(&format!(
            "(Binary data {} bytes, use -b option to extract)",
            second.len()
        ))
    );
    assert!(!tags.contains_key("Sony:TextInfo3"));
    assert_eq!(tags.get("Sony:BoardTemperature"), Some(&"28 C".to_string()));
    // The second block's `BarCode:` (truncated to 12) is extracted after the
    // first block's `BC:`; `xBC:` has no word boundary and never matches.
    assert_eq!(tags.get("Sony:Barcode"), Some(&"0123456789AB".to_string()));
}

#[test]
fn test_sony_parse_empty_data() {
    assert!(parse(&[]).is_empty());
    assert!(parse(b"\x01").is_empty());
}

#[test]
fn test_sony_parse_with_signature() {
    // The "SONY DSC " header is a fixed 12 bytes. Scanning for the first
    // non-NUL byte instead breaks a big-endian MakerNote, whose entry count
    // starts with a 0x00 - which is exactly what SonyDSLR-A100.jpg writes.
    let mut data = Vec::new();
    data.extend_from_slice(b"SONY DSC \0\0\0");
    data.extend_from_slice(&[0x00, 0x01]); // big-endian: 1 entry
    data.extend_from_slice(&[0xb0, 0x27]); // tag 0xb027 LensType
    data.extend_from_slice(&[0x00, 0x04]); // type LONG
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]); // count 1
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x28]); // value 40
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    // SonyDSLR-A100.jpg stores LensType 40 in a big-endian MakerNote.
    let tags = parse(&data);
    assert_eq!(
        tags.get("Sony:LensType"),
        Some(&"Minolta/Sony AF DT 18-70mm F3.5-5.6 (D)".to_string())
    );
}

#[test]
fn test_sony_lens_database_coverage() {
    // `%sonyLensTypes` is a large table; a truncated copy would still satisfy
    // the spot checks above.
    let count = (0u16..=u16::MAX)
        .filter(|id| lookup_lens_name(*id).is_some())
        .count();
    assert!(
        count >= 200,
        "expected at least 200 lens ids from ExifTool's table, found {}",
        count
    );
}
