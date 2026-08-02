//! Integration tests for the Apple (iPhone / iPad) MakerNote parser.
//!
//! Every expected value here is what `exiftool -a -G1 -s` prints for the bytes
//! being fed in; the bytes themselves are copied from real corpus files, dumped
//! with `exiftool -v3`. Nothing is asserted that ExifTool does not report.

use oxidex::parsers::tiff::ifd_parser::ByteOrder;
use oxidex::parsers::tiff::makernotes::apple::AppleParser;
use oxidex::parsers::tiff::makernotes::shared::MakerNoteParser;
use std::collections::HashMap;

/// `Apple_iPhone13Pro.jpg`'s tag 0x0003 (`undef[104]`) verbatim. ExifTool
/// reports `RunTimeFlags = Valid`, `RunTimeValue = 235706184764708`,
/// `RunTimeScale = 1000000000` and `RunTimeEpoch = 0` from it.
const RUNTIME_BLOB: &[u8] = &[
    0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0xd4, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
    0x08, 0x55, 0x66, 0x6c, 0x61, 0x67, 0x73, 0x55, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x59, 0x74, 0x69,
    0x6d, 0x65, 0x73, 0x63, 0x61, 0x6c, 0x65, 0x55, 0x65, 0x70, 0x6f, 0x63, 0x68, 0x10, 0x01, 0x13,
    0x00, 0x00, 0xd6, 0x5f, 0x9f, 0x6a, 0x0d, 0x24, 0x12, 0x3b, 0x9a, 0xca, 0x00, 0x10, 0x00, 0x08,
    0x11, 0x17, 0x1d, 0x27, 0x2d, 0x2f, 0x38, 0x3d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3f,
];

/// The same file's tag 0x0040 (`undef[80]`). ExifTool prints
/// `SemanticStyle = {_0=1,_1=0.5,_2=0,_3=2}`.
const SEMANTIC_STYLE_BLOB: &[u8] = &[
    0x62, 0x70, 0x6c, 0x69, 0x73, 0x74, 0x30, 0x30, 0xd4, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
    0x08, 0x51, 0x33, 0x51, 0x31, 0x51, 0x32, 0x51, 0x30, 0x10, 0x02, 0x22, 0x3f, 0x00, 0x00, 0x00,
    0x22, 0x00, 0x00, 0x00, 0x00, 0x10, 0x01, 0x08, 0x11, 0x13, 0x15, 0x17, 0x19, 0x1b, 0x20, 0x25,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x27,
];

/// One IFD entry: `(tag, field type, count, inline bytes or out-of-line value)`.
enum Entry {
    /// Four bytes or fewer, stored in the entry's own value field.
    Inline(u16, u16, u32, [u8; 4]),
    /// Longer, appended after the directory and addressed by offset.
    Offset(u16, u16, u32, &'static [u8]),
}

/// Assemble an Apple MakerNote value: `Apple iOS\0`, the two version bytes, the
/// `MM` order marker, then the IFD at byte 14 -- exactly the layout
/// `MakerNotes.pm:37-46` describes.
fn apple_makernote(entries: &[Entry]) -> Vec<u8> {
    let n = entries.len();
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"Apple iOS\x00");
    out.extend_from_slice(&[0x00, 0x01]);
    out.extend_from_slice(b"MM");
    out.extend_from_slice(&(n as u16).to_be_bytes());
    // 14 + 2 header, 12 bytes per entry, then the 4-byte next-IFD pointer.
    let mut value_pos = 16 + n * 12 + 4;
    let mut tail: Vec<u8> = Vec::new();
    for e in entries {
        match e {
            Entry::Inline(tag, ft, count, bytes) => {
                out.extend_from_slice(&tag.to_be_bytes());
                out.extend_from_slice(&ft.to_be_bytes());
                out.extend_from_slice(&count.to_be_bytes());
                out.extend_from_slice(bytes);
            }
            Entry::Offset(tag, ft, count, bytes) => {
                out.extend_from_slice(&tag.to_be_bytes());
                out.extend_from_slice(&ft.to_be_bytes());
                out.extend_from_slice(&count.to_be_bytes());
                out.extend_from_slice(&(value_pos as u32).to_be_bytes());
                tail.extend_from_slice(bytes);
                value_pos += bytes.len();
            }
        }
    }
    out.extend_from_slice(&[0, 0, 0, 0]); // next IFD
    out.extend_from_slice(&tail);
    out
}

fn parse(entries: &[Entry]) -> HashMap<String, String> {
    let data = apple_makernote(entries);
    let mut tags = HashMap::new();
    AppleParser::new()
        .parse(&data, ByteOrder::BigEndian, &mut tags)
        .expect("Apple parse");
    tags
}

const INT32S: u16 = 9;
const UNDEF: u16 = 7;
const SRATIONAL: u16 = 10;

#[test]
fn parser_identity() {
    let parser = AppleParser::new();
    assert_eq!(parser.manufacturer_name(), "Apple");
    assert_eq!(parser.tag_prefix(), "Apple:");
}

#[test]
fn validate_header_requires_the_apple_ios_signature() {
    let parser = AppleParser::new();
    assert!(parser.validate_header(b"Apple iOS\x00\x00\x01MM\x00\x31"));
    // MakerNotes.pm:39 keys on the signature, not on a plausible entry count.
    assert!(!parser.validate_header(&[0x05, 0x00]));
    assert!(!parser.validate_header(b"Nikon\x00\x02\x00\x00\x00II\x2a\x00"));
}

#[test]
fn descends_into_the_runtime_binary_plist() {
    // Apple.pm:40-43 makes 0x0003 a SubDirectory over %Apple::RunTime, whose
    // PROCESS_PROC is PLIST::ProcessBinaryPLIST.
    let tags = parse(&[Entry::Offset(0x0003, UNDEF, 104, RUNTIME_BLOB)]);
    assert_eq!(
        tags.get("Apple:RunTimeFlags").map(String::as_str),
        Some("Valid")
    );
    assert_eq!(
        tags.get("Apple:RunTimeValue").map(String::as_str),
        Some("235706184764708")
    );
    assert_eq!(
        tags.get("Apple:RunTimeScale").map(String::as_str),
        Some("1000000000")
    );
    assert_eq!(
        tags.get("Apple:RunTimeEpoch").map(String::as_str),
        Some("0")
    );
    // The pointer itself is never a value.
    assert!(!tags.contains_key("Apple:RunTime"));
    assert_eq!(tags.len(), 4);
}

#[test]
fn serializes_the_semantic_style_plist_dictionary() {
    // Apple.pm:276 -- ValueConv => \&ConvertPLIST, then SerializeStruct.
    let tags = parse(&[Entry::Offset(0x0040, UNDEF, 80, SEMANTIC_STYLE_BLOB)]);
    assert_eq!(
        tags.get("Apple:SemanticStyle").map(String::as_str),
        Some("{_0=1,_1=0.5,_2=0,_3=2}")
    );
}

#[test]
fn reads_the_scalar_tags_apple_pm_declares() {
    let tags = parse(&[
        // MakerNoteVersion = 14 (Apple_iPhone13Pro.jpg)
        Entry::Inline(0x0001, INT32S, 1, [0, 0, 0, 14]),
        // AEStable = Yes; Apple.pm:47 PrintConv => { 0 => 'No', 1 => 'Yes' }
        Entry::Inline(0x0004, INT32S, 1, [0, 0, 0, 1]),
        // AETarget = 198
        Entry::Inline(0x0005, INT32S, 1, [0, 0, 0, 198]),
        // ImageCaptureType = Scene; Apple.pm:131 12 => 'Scene'
        Entry::Inline(0x0014, INT32S, 1, [0, 0, 0, 12]),
        // CameraType = Back Normal; Apple.pm:221 1 => 'Back Normal'
        Entry::Inline(0x002e, INT32S, 1, [0, 0, 0, 1]),
        // OISMode has no PrintConv in Apple.pm, so it stays numeric
        Entry::Inline(0x000f, INT32S, 1, [0, 0, 0, 2]),
    ]);
    assert_eq!(
        tags.get("Apple:MakerNoteVersion").map(String::as_str),
        Some("14")
    );
    assert_eq!(tags.get("Apple:AEStable").map(String::as_str), Some("Yes"));
    assert_eq!(tags.get("Apple:AETarget").map(String::as_str), Some("198"));
    assert_eq!(
        tags.get("Apple:ImageCaptureType").map(String::as_str),
        Some("Scene")
    );
    assert_eq!(
        tags.get("Apple:CameraType").map(String::as_str),
        Some("Back Normal")
    );
    assert_eq!(tags.get("Apple:OISMode").map(String::as_str), Some("2"));
}

#[test]
fn rationals_are_rounded_to_ten_significant_digits() {
    // Apple_iPhone13Pro.jpg's AccelerationVector, rational64s[3], which
    // ExifTool prints as "-0.9245480894 0.00592365628 0.2826257348" --
    // GetRational64s ends in RoundFloat($num/$den, 10).
    static VECTOR: &[u8] = &[
        0xff, 0xff, 0x42, 0x99, 0x00, 0x00, 0xcc, 0xdc, 0x00, 0x00, 0x0c, 0xcb, 0x00, 0x08, 0x6f,
        0xa4, 0x00, 0x00, 0x10, 0xe7, 0x00, 0x00, 0x3b, 0xce,
    ];
    // The same file's FocusDistanceRange, rational64s[2] = 515/128 and 37/256,
    // which Apple.pm:98-101 sorts and prints as "0.14 - 4.02 m".
    static RANGE: &[u8] = &[
        0x00, 0x00, 0x02, 0x03, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0x25, 0x00, 0x00, 0x01,
        0x00,
    ];
    let tags = parse(&[
        Entry::Offset(0x0008, SRATIONAL, 3, VECTOR),
        Entry::Offset(0x000c, SRATIONAL, 2, RANGE),
    ]);
    assert_eq!(
        tags.get("Apple:AccelerationVector").map(String::as_str),
        Some("-0.9245480894 0.00592365628 0.2826257348")
    );
    assert_eq!(
        tags.get("Apple:FocusDistanceRange").map(String::as_str),
        Some("0.14 - 4.02 m")
    );
}

#[test]
fn af_performance_reports_three_numbers_from_two_words() {
    // Apple_iPhone13Pro.jpg: int32s[2] = 682, 268435509. Apple.pm:187 splits
    // the second word into its top nibble and low 28 bits: "682 1 53".
    static PERF: &[u8] = &[0x00, 0x00, 0x02, 0xaa, 0x10, 0x00, 0x00, 0x35];
    let tags = parse(&[Entry::Offset(0x0023, INT32S, 2, PERF)]);
    assert_eq!(
        tags.get("Apple:AFPerformance").map(String::as_str),
        Some("682 1 53")
    );
}

#[test]
fn reads_a_sixty_four_bit_live_photo_video_index() {
    // Apple_iPhone15Pro.jpg stores 0x0017 as int64u[1] = 4294967700, which
    // ExifTool reports in full. Format code 16 is ExifTool's `int64u`.
    static IDX: &[u8] = &[0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x94];
    let tags = parse(&[Entry::Offset(0x0017, 16, 1, IDX)]);
    assert_eq!(
        tags.get("Apple:LivePhotoVideoIndex").map(String::as_str),
        Some("4294967700")
    );
}

#[test]
fn unknown_and_unlisted_ids_report_nothing() {
    let tags = parse(&[
        // 0x0002 AEMatrix is Unknown => 1, so ExifTool hides it by default.
        Entry::Inline(0x0002, INT32S, 1, [0, 0, 0, 1]),
        // 0x0032 is not a tag %Apple::Main has at all.
        Entry::Inline(0x0032, INT32S, 1, [0, 0, 0, 1]),
        // 0x0035 likewise -- the old registry called it LensModel.
        Entry::Inline(0x0035, INT32S, 1, [0, 0, 0, 1]),
    ]);
    assert!(tags.is_empty(), "unexpected tags: {tags:?}");
}

#[test]
fn a_directory_that_is_not_apples_yields_nothing() {
    let parser = AppleParser::new();
    for data in [
        vec![0x01u8],
        vec![0x00, 0x02],
        b"Apple iOS\x00".to_vec(),
        b"Apple iOS\x00\x00\x01MM\x00\x00".to_vec(), // zero entries
    ] {
        let mut tags = HashMap::new();
        parser
            .parse(&data, ByteOrder::BigEndian, &mut tags)
            .expect("parse must not error");
        assert!(tags.is_empty(), "unexpected tags from {data:?}: {tags:?}");
    }
}
