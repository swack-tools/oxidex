//! Integration tests for Sony MakerNotes parser
//!
//! Every expected value here comes from real ExifTool output on a corpus
//! sample, quoted in the test that uses it. The Sony `PrintConv` space is dense
//! with near-identical strings, so an assertion invented to match the
//! implementation would look exactly like one that matches the camera.

use oxidex::core::operations::read_metadata;
use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use oxidex::parsers::tiff::makernotes::sony::{SonyParser, parse_sony_makernote};
use oxidex::parsers::tiff::makernotes::sony_lens_database::lookup_lens_name;
use std::collections::HashMap;
use std::path::Path;

const A900: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Sony/SonyDSLR-A900.jpg";
const A580: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Sony/SonyDSLR-A580.jpg";
const DSC_H300: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Sony/SonyDSC-H300.jpg";
const ZV_E10M2: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Sony/SonyZV-E10M2.jpg";

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
fn test_sony_a900_camera_info_matches_exiftool() {
    if !Path::new(A900).is_file() {
        eprintln!("skipping: corpus fixture not present at {A900}");
        return;
    }
    let metadata = read_metadata(Path::new(A900)).expect("Sony DSLR-A900 parses");
    // Pinned ExifTool 13.59 reports these from CameraInfo (tag 0x0010,
    // 5478-byte A900 layout). The AF words use Sony's reversed int16 order.
    assert_eq!(metadata.get_string("Sony:AFPoint"), Some("Lower-right"));
    assert_eq!(
        metadata.get_string("Sony:AFStatusFarRight"),
        Some("Front Focus (-65)")
    );
    assert_eq!(
        metadata.get_string("Sony:AFStatusTop"),
        Some("Out of Focus")
    );
    assert_eq!(metadata.get_string("Sony:AFMicroAdjMode"), Some("Off"));
    assert_eq!(
        metadata.get_string("Sony:AFMicroAdjRegisteredLenses"),
        Some("0")
    );
    // ExtraInfo (0x0116) is a separate, model-gated 30-byte directory.
    assert_eq!(
        metadata.get_string("Sony:BatteryTemperature"),
        Some("46.1 C")
    );
    assert_eq!(
        metadata.get_string("Sony:ExtraInfoVersion"),
        Some("0.1.0.0")
    );
}

#[test]
fn test_sony_dsc_h300_pic_text_blocks_match_exiftool() {
    if !Path::new(DSC_H300).is_file() {
        eprintln!("skipping: corpus fixture not present at {DSC_H300}");
        return;
    }
    let metadata = read_metadata(Path::new(DSC_H300)).expect("Sony DSC-H300 parses");
    // Pinned ExifTool 13.59's ProcessSonyPIC reports the two text blocks as
    // binary data (769 and 671 bytes) and extracts `BC:` as this barcode.
    assert_eq!(
        metadata.get_string("Sony:TextInfo1"),
        Some("(Binary data 769 bytes, use -b option to extract)")
    );
    assert_eq!(
        metadata.get_string("Sony:TextInfo2"),
        Some("(Binary data 671 bytes, use -b option to extract)")
    );
    assert_eq!(metadata.get_string("Sony:Barcode"), Some("A0D9P7016135"));
}

#[test]
fn test_sony_zv_e10m2_hidden_and_pixel_shift_info_match_exiftool() {
    if !Path::new(ZV_E10M2).is_file() {
        eprintln!("skipping: corpus fixture not present at {ZV_E10M2}");
        return;
    }
    // Pinned ExifTool 13.59, SonyZV-E10M2.jpg:
    //   HiddenDataOffset = 13938688, HiddenDataLength = 53248,
    //   PixelShiftInfo = n/a.  The first two are the 0x2044 HiddenInfo
    // int32u pair; PixelShiftInfo is 0x202f's all-zero six-byte sentinel.
    let metadata = read_metadata(Path::new(ZV_E10M2)).expect("Sony ZV-E10M2 parses");
    assert_eq!(
        metadata.get_string("Sony:HiddenDataOffset"),
        Some("13938688")
    );
    assert_eq!(metadata.get_string("Sony:HiddenDataLength"), Some("53248"));
    assert_eq!(metadata.get_string("Sony:PixelShiftInfo"), Some("n/a"));
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

#[test]
fn test_sony_more_settings_a550_fields_follow_exiftool_layout() {
    // `Sony::MoreInfo` is a little-endian offset directory. Its block id 1
    // selects `Sony::MoreSettings`; ExifTool 13.59 defines these A550 rows at
    // 0x1a (`int16uRev[2]`), 0x24 (`int16s / 8`) and 0x28 (Orientation map).
    let mut more_info = vec![0u8; 20480];
    more_info[0..2].copy_from_slice(&1u16.to_le_bytes());
    more_info[2..4].copy_from_slice(&20480u16.to_le_bytes());
    more_info[4..6].copy_from_slice(&1u16.to_le_bytes());
    more_info[6..8].copy_from_slice(&8u16.to_le_bytes());
    more_info[8 + 0x1a..8 + 0x1e].copy_from_slice(&[0, 4, 0, 8]);
    more_info[8 + 0x24..8 + 0x26].copy_from_slice(&(-8i16).to_le_bytes());
    more_info[8 + 0x28] = 6;

    let mut maker_note = build_makernote(&[(0x0020, 7, 20480, 1018)], &[]);
    maker_note.extend_from_slice(&more_info);
    let mut tiff = vec![0u8; 1000];
    tiff.extend_from_slice(&maker_note);
    let ctx = MakerNoteContext::in_tiff(&tiff, 1000, maker_note.len(), 0);
    let mut tags = HashMap::new();
    SonyParser
        .parse_with_context(&ctx, ByteOrder::LittleEndian, Some("DSLR-A550"), &mut tags)
        .unwrap();

    assert_eq!(tags.get("Sony:CustomWB_RBLevels"), Some(&"4 8".to_string()));
    assert_eq!(
        tags.get("Sony:ExposureCompensation2"),
        Some(&"-1.0".to_string())
    );
    assert_eq!(
        tags.get("Sony:Orientation2"),
        Some(&"Rotate 90 CW".to_string())
    );
}

#[test]
fn test_sony_a580_conditional_maker_note_fields_match_exiftool() {
    if !Path::new(A580).is_file() {
        eprintln!("skipping: corpus fixture not present at {A580}");
        return;
    }
    // Pinned ExifTool 13.59 reports these from the model-gated MoreSettings
    // (0x20 / 0x7c) and ExtraInfo3 (0x14) alternatives. The generated schema
    // omits those alternatives, so this guards the manual selection against
    // real camera bytes rather than an invented layout.
    let metadata = read_metadata(Path::new(A580)).expect("Sony DSLR-A580 parses");
    assert_eq!(
        metadata.get_string("Sony:LiveViewAFMethod"),
        Some("Phase-detect AF")
    );
    assert_eq!(
        metadata.get_string("Sony:FlashActionExternal"),
        Some("Did not fire")
    );
    assert_eq!(metadata.get_string("Sony:ModeDialPosition"), Some("Manual"));
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
