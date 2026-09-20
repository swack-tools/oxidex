//! Task 9: the four standard EXIF directories share one generated-first
//! conversion/ownership contract.  These tests exercise the public IFD engine
//! boundary; carrier-specific routing stays covered by the focused module
//! tests in `core::{jpeg_helpers,tiff_helpers,exif_dir_engine}`.

use std::collections::HashMap;
use std::io::Write;
use std::io::{self, ErrorKind};

use oxidex::core::FileReader;
use oxidex::core::MetadataMap;
use oxidex::core::TagValue;
use oxidex::core::operations::{read_metadata, read_metadata_with_detector_and_options};
use oxidex::core::read_options::ReadOptions;
use oxidex::core::tag_occurrence::ValueChannel;
use oxidex::core::tiff_helpers::parse_ifd1;
use oxidex::exiftool_tables::cond::Ctx;
use oxidex::exiftool_tables::ifd_tables::IFD_EXIF_MAIN;
use oxidex::exiftool_tables::session::{MemberVal, Session};
use oxidex::exiftool_tables::{EntryRead, IfdDir, MemberValue, process_exif_decoded};
use oxidex::io::ByteOrder;
use oxidex::parsers::DetectorMode;

/// A little-endian TIFF block whose IFD at offset 8 contains the supplied
/// `(id, type, count, bytes)` entries. Values over four bytes are stored after
/// the directory and addressed TIFF-relative, exactly like a real carrier.
fn le_tiff(entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
    let header = 8 + 2 + entries.len() * 12 + 4;
    let mut buffer = b"II\x2a\0\x08\0\0\0".to_vec();
    buffer.resize(header, 0);
    buffer[8..10].copy_from_slice(&(entries.len() as u16).to_le_bytes());
    let mut values = Vec::new();
    for (index, (id, ty, count, bytes)) in entries.iter().enumerate() {
        let at = 10 + index * 12;
        buffer[at..at + 2].copy_from_slice(&id.to_le_bytes());
        buffer[at + 2..at + 4].copy_from_slice(&ty.to_le_bytes());
        buffer[at + 4..at + 8].copy_from_slice(&count.to_le_bytes());
        if bytes.len() <= 4 {
            buffer[at + 8..at + 8 + bytes.len()].copy_from_slice(bytes);
        } else {
            let offset = (header + values.len()) as u32;
            buffer[at + 8..at + 12].copy_from_slice(&offset.to_le_bytes());
            values.extend_from_slice(bytes);
        }
    }
    buffer.extend_from_slice(&values);
    buffer
}

fn ascii(id: u16, text: &[u8]) -> (u16, u16, u32, Vec<u8>) {
    (id, 2, text.len() as u32, text.to_vec())
}

fn undefined(id: u16, bytes: &[u8]) -> (u16, u16, u32, Vec<u8>) {
    (id, 7, bytes.len() as u32, bytes.to_vec())
}

fn walk<'a>(
    tiff: &'a [u8],
    data_domain: u64,
    group1: &'static str,
    session: &mut Session,
    members: &'a mut HashMap<&'static str, MemberValue>,
) -> (
    Option<oxidex::exiftool_tables::RootReads>,
    Vec<oxidex::exiftool_tables::Emitted>,
) {
    let mut emitted = Vec::new();
    let mut ctx = Ctx::new(members);
    let reads = process_exif_decoded(
        &IFD_EXIF_MAIN,
        IfdDir {
            data: tiff,
            data_domain,
            ifd_start: 8,
            base: Some(0),
            byte_order: ByteOrder::Little,
            group1: Some(group1),
        },
        session,
        &mut ctx,
        &mut emitted,
    );
    (reads, emitted)
}

#[test]
fn report_commits_generated_writes_once_in_source_order() {
    // 0x010f Make's generated RawConv trims and writes $$self{Make}. Two
    // physical entries must produce two occurrences in source order while
    // leaving the file-scoped member at the second value.
    let tiff = le_tiff(&[ascii(0x010f, b"First  \0"), ascii(0x010f, b"Second \0")]);
    let mut session = Session::new();
    let mut members = HashMap::new();
    let (reads, emitted) = walk(&tiff, 0x1000, "IFD0", &mut session, &mut members);

    assert_eq!(
        reads.expect("IFD is admitted").entries,
        [EntryRead::Decoded; 2]
    );
    assert_eq!(
        emitted
            .iter()
            .map(|row| (row.group1, row.name))
            .collect::<Vec<_>>(),
        [("IFD0", "Make"), ("IFD0", "Make")]
    );
    assert_eq!(session.member("Make"), MemberVal::Str("Second".into()));

    // The same physical directory/table identity is already processed: a
    // second route cannot repeat either occurrences or writes.
    let (again, repeated) = walk(&tiff, 0x1000, "ExifIFD", &mut session, &mut members);
    assert!(again.is_none());
    assert!(repeated.is_empty());
    assert_eq!(session.member("Make"), MemberVal::Str("Second".into()));
}

#[test]
fn suppress_emits_nothing_and_never_requests_a_residual() {
    // PanasonicTitle's generated RawConv returns undef for the empty string.
    // Decoded (rather than Unread) is the ownership signal: callers must not
    // fall back to a hand producer after the generated Suppress.
    let tiff = le_tiff(&[ascii(0xc6d2, b"\0")]);
    let mut session = Session::new();
    let mut members = HashMap::new();
    let (reads, emitted) = walk(&tiff, 0x2000, "ExifIFD", &mut session, &mut members);

    assert_eq!(
        reads.expect("IFD is admitted").entries,
        [EntryRead::Decoded]
    );
    assert!(emitted.is_empty());
    assert!(session.warnings().is_empty());
}

#[test]
fn decline_discards_staged_effects_before_one_residual() {
    // UserComment's RawConv calls ConvertExifText. An invalid 8-byte coding
    // prefix asks ExifTool to warn; generated decode then declines because a
    // Warning occurrence is not modelled. The attempt's warning is staged:
    // the named residual is the sole owner and sees the unmodified Session.
    let tiff = le_tiff(&[undefined(0x9286, b"BADCODE!\xffpayload")]);
    let mut session = Session::new();
    let mut members = HashMap::new();
    let (reads, emitted) = walk(&tiff, 0x3000, "ExifIFD", &mut session, &mut members);
    let reads = reads.expect("IFD is admitted");

    assert_eq!(reads.entries, [EntryRead::Unread]);
    assert!(emitted.is_empty());
    assert!(
        session.warnings().is_empty(),
        "declined generated effects must be discarded before residual dispatch"
    );

    // Exercise the production adapter, not a detached callback counter: the
    // declined physical occurrence is emitted once by the named hand
    // residual, with its original bytes, while the following occurrence is
    // still owned by the generated report.
    let (carrier, declined, _) = duplicate_user_comment_tiff();
    let mut file = tempfile::Builder::new()
        .suffix(".tiff")
        .tempfile()
        .expect("temporary TIFF");
    file.write_all(&carrier).expect("write TIFF");
    let metadata = read_metadata(file.path()).expect("production read");
    let occurrences: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(key, _, _)| *key == "ExifIFD:UserComment")
        .map(|(_, _, value)| value.into_owned())
        .collect();
    assert_eq!(occurrences.len(), 2);
    assert_eq!(occurrences[0], TagValue::Binary(declined));
    assert_eq!(occurrences[1], TagValue::new_string("second"));
}

#[test]
fn distinct_directories_share_session_but_keep_group_and_order() {
    let ifd0 = le_tiff(&[ascii(0x010f, b"Camera\0")]);
    let exif = le_tiff(&[ascii(0x9003, b"2026:09:20 12:34:56\0")]);
    let mut session = Session::new();
    let mut members = HashMap::new();

    let (_, first) = walk(&ifd0, 0x4000, "IFD0", &mut session, &mut members);
    let (_, second) = walk(&exif, 0x4001, "ExifIFD", &mut session, &mut members);

    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!((first[0].group1, first[0].name), ("IFD0", "Make"));
    assert_eq!(
        (second[0].group1, second[0].name),
        ("ExifIFD", "DateTimeOriginal")
    );
    assert_eq!(session.member("Make"), MemberVal::Str("Camera".into()));
}

/// A complete TIFF carrier with IFD0 pointing at an ExifIFD containing two
/// physical occurrences of the same id. The first UserComment deliberately
/// declines generated conversion; the second reports normally.
fn duplicate_user_comment_tiff() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let declined = b"BADCODE!\xfffirst".to_vec();
    let reported = b"ASCII\0\0\0second".to_vec();
    let exif_offset = 8 + 2 + 12 + 4;
    let value_offset = exif_offset + 2 + 2 * 12 + 4;

    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0x8769u16.to_le_bytes());
    tiff.extend(4u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend((exif_offset as u32).to_le_bytes());
    tiff.extend(0u32.to_le_bytes());

    tiff.extend(2u16.to_le_bytes());
    for (bytes, offset) in [
        (&declined, value_offset),
        (&reported, value_offset + declined.len()),
    ] {
        tiff.extend(0x9286u16.to_le_bytes());
        tiff.extend(7u16.to_le_bytes());
        tiff.extend((bytes.len() as u32).to_le_bytes());
        tiff.extend((offset as u32).to_le_bytes());
    }
    tiff.extend(0u32.to_le_bytes());
    tiff.extend_from_slice(&declined);
    tiff.extend_from_slice(&reported);
    (tiff, declined, reported)
}

fn requested_edge_tiff() -> Vec<u8> {
    let payload = b"<x:xmp/>";
    let exif_offset = 26u32;
    let value_offset = 44u32;
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0x8769u16.to_le_bytes());
    tiff.extend(4u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(exif_offset.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0x02bcu16.to_le_bytes());
    tiff.extend(7u16.to_le_bytes());
    tiff.extend((payload.len() as u32).to_le_bytes());
    tiff.extend(value_offset.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    tiff.extend_from_slice(payload);
    tiff
}

fn jpeg_with_exif(tiff: &[u8]) -> Vec<u8> {
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(tiff);
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
    jpeg.extend(
        u16::try_from(app1.len() + 2)
            .expect("APP1 length")
            .to_be_bytes(),
    );
    jpeg.extend(app1);
    jpeg.extend([0xff, 0xd9]);
    jpeg
}

#[test]
fn public_jpeg_and_tiff_reads_preserve_an_explicitly_requested_edge() {
    let options = ReadOptions::new(&["ExifIFD:ApplicationNotes".to_string()], false);
    let tiff = requested_edge_tiff();
    for (suffix, bytes) in [(".tiff", tiff.clone()), (".jpg", jpeg_with_exif(&tiff))] {
        let mut file = tempfile::Builder::new()
            .suffix(suffix)
            .tempfile()
            .expect("temporary carrier");
        file.write_all(&bytes).expect("write carrier");

        let default = read_metadata(file.path()).expect("default read");
        assert!(
            default.get("ExifIFD:ApplicationNotes").is_none(),
            "{suffix}"
        );

        let requested =
            read_metadata_with_detector_and_options(file.path(), DetectorMode::Signature, &options)
                .expect("requested read");
        assert!(
            matches!(
                requested.get("ExifIFD:ApplicationNotes"),
                Some(TagValue::Binary(value)) if value == b"<x:xmp/>"
            ),
            "{suffix}"
        );
        assert_eq!(
            requested
                .project_occurrences(ValueChannel::PrintConv)
                .filter(|(key, _, _)| *key == "ExifIFD:ApplicationNotes")
                .count(),
            1,
            "{suffix}"
        );
    }
}

#[test]
fn mixed_decline_and_report_duplicates_keep_physical_order_and_owner() {
    // The production change that makes this pass is occurrence-index routing.
    // An id/name replay can consume the second entry's generated row while
    // visiting the first, then run the hand residual on the second bytes.
    let (tiff, declined, _) = duplicate_user_comment_tiff();
    let mut file = tempfile::Builder::new()
        .suffix(".tiff")
        .tempfile()
        .expect("temporary TIFF");
    file.write_all(&tiff).expect("write TIFF");

    let metadata = read_metadata(file.path()).expect("synthetic TIFF parses");
    let occurrences: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(key, _, _)| *key == "ExifIFD:UserComment")
        .map(|(_, occurrence, value)| (occurrence, value.into_owned()))
        .collect();
    assert_eq!(occurrences.len(), 2, "one owner for each physical entry");
    assert_eq!(
        occurrences[0].1,
        TagValue::Binary(declined),
        "the first declined entry belongs to the residual"
    );
    assert_eq!(
        occurrences[1].1,
        TagValue::new_string("second"),
        "the second entry belongs to the generated report"
    );
    assert!(occurrences[0].0.order < occurrences[1].0.order);
    assert_eq!(&*occurrences[0].0.group1, "");
    assert_eq!(&*occurrences[1].0.group1, "");
}

#[test]
fn skipped_entry_does_not_compress_the_generated_rows_physical_index() {
    let declined = b"BADCODE!\xfffirst";
    let reported = b"ASCII\0\0\0after-gap";
    let exif_offset = 8 + 2 + 12 + 4;
    let value_offset = exif_offset + 2 + 4 * 12 + 4;
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0x8769u16.to_le_bytes());
    tiff.extend(4u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend((exif_offset as u32).to_le_bytes());
    tiff.extend(0u32.to_le_bytes());

    tiff.extend(4u16.to_le_bytes());
    // A surviving hand-owned row before the gap.
    tiff.extend(0xdeadu16.to_le_bytes());
    tiff.extend(3u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    // parse_ifd drops this physical entry, but the generated reader retains
    // its slot as Refused. The next survivor is therefore physical index 2,
    // not vector index 1.
    tiff.extend(0xbeefu16.to_le_bytes());
    tiff.extend(0u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    for (bytes, offset) in [
        (declined.as_slice(), value_offset),
        (reported.as_slice(), value_offset + declined.len()),
    ] {
        tiff.extend(0x9286u16.to_le_bytes());
        tiff.extend(7u16.to_le_bytes());
        tiff.extend((bytes.len() as u32).to_le_bytes());
        tiff.extend((offset as u32).to_le_bytes());
    }
    tiff.extend(0u32.to_le_bytes());
    tiff.extend_from_slice(declined);
    tiff.extend_from_slice(reported);

    let mut file = tempfile::Builder::new()
        .suffix(".tiff")
        .tempfile()
        .expect("temporary TIFF");
    file.write_all(&tiff).expect("write TIFF");
    let metadata = read_metadata(file.path()).expect("synthetic TIFF parses");

    let occurrences: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(key, _, _)| *key == "ExifIFD:UserComment")
        .map(|(_, occurrence, value)| (occurrence, value.into_owned()))
        .collect();
    assert_eq!(occurrences.len(), 2, "one owner per surviving occurrence");
    assert_eq!(occurrences[0].1, TagValue::Binary(declined.to_vec()));
    assert_eq!(occurrences[1].1, TagValue::new_string("after-gap"));
    assert!(occurrences[0].0.order < occurrences[1].0.order);
}

#[test]
fn two_top_level_ifds_sharing_one_exif_directory_emit_it_once() {
    let page2_offset = 26u32;
    let exif_offset = 44u32;
    let value_offset = 74u32;
    let interop_offset = 88u32;
    let comment = b"ASCII\0\0\0shared";
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();

    for next_ifd in [page2_offset, 0] {
        tiff.extend(1u16.to_le_bytes());
        tiff.extend(0x8769u16.to_le_bytes());
        tiff.extend(4u16.to_le_bytes());
        tiff.extend(1u32.to_le_bytes());
        tiff.extend(exif_offset.to_le_bytes());
        tiff.extend(next_ifd.to_le_bytes());
    }

    tiff.extend(2u16.to_le_bytes());
    tiff.extend(0x9286u16.to_le_bytes());
    tiff.extend(7u16.to_le_bytes());
    tiff.extend((comment.len() as u32).to_le_bytes());
    tiff.extend(value_offset.to_le_bytes());
    tiff.extend(0xa005u16.to_le_bytes());
    tiff.extend(4u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(interop_offset.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    tiff.extend_from_slice(comment);

    tiff.extend(1u16.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(4u32.to_le_bytes());
    tiff.extend(*b"R98\0");
    tiff.extend(0u32.to_le_bytes());

    let mut file = tempfile::Builder::new()
        .suffix(".tiff")
        .tempfile()
        .expect("temporary TIFF");
    file.write_all(&tiff).expect("write TIFF");
    let metadata = read_metadata(file.path()).expect("synthetic TIFF parses");

    assert_eq!(metadata.get_string("ExifIFD:UserComment"), Some("shared"));
    assert_eq!(
        metadata
            .project_occurrences(ValueChannel::PrintConv)
            .filter(|(key, _, _)| *key == "ExifIFD:UserComment")
            .count(),
        1,
        "the session guard suppresses both generated and residual replay"
    );
    assert_eq!(
        metadata.get_string("InteropIFD:InteropIndex"),
        Some("R98 - DCF basic file (sRGB)")
    );
    assert_eq!(
        metadata
            .project_occurrences(ValueChannel::PrintConv)
            .filter(|(key, _, _)| *key == "InteropIFD:InteropIndex")
            .count(),
        1,
        "the repeated parent does not traverse its structural edge twice"
    );
}

#[test]
fn exif_ifd_aliasing_ifd1_skips_the_whole_repeated_adapter() {
    let alias_offset = 26u32;
    let comment_offset = 68u32;
    let interop_offset = 83u32;
    let gps_offset = 101u32;
    let comment = b"ASCII\0\0\0aliased";
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();

    // IFD0 reaches the same physical directory as both ExifIFD and IFD1.
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0x8769u16.to_le_bytes());
    tiff.extend(4u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(alias_offset.to_le_bytes());
    tiff.extend(alias_offset.to_le_bytes());

    // The aliased directory has one ordinary row, an Interop child that the
    // ExifIFD adapter owns, and a GPS child that only the repeated IFD1
    // adapter would traverse. The latter makes a late guard observable.
    tiff.extend(3u16.to_le_bytes());
    for (tag, value) in [
        (0x9286u16, comment_offset),
        (0xa005u16, interop_offset),
        (0x8825u16, gps_offset),
    ] {
        tiff.extend(tag.to_le_bytes());
        tiff.extend(if tag == 0x9286 { 7u16 } else { 4u16 }.to_le_bytes());
        tiff.extend(
            if tag == 0x9286 {
                comment.len() as u32
            } else {
                1u32
            }
            .to_le_bytes(),
        );
        tiff.extend(value.to_le_bytes());
    }
    tiff.extend(0u32.to_le_bytes());
    tiff.extend_from_slice(comment);

    // Interop child: observable once from the first ExifIFD traversal.
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(4u32.to_le_bytes());
    tiff.extend(*b"R98\0");
    tiff.extend(0u32.to_le_bytes());

    // GPS child: would become observable only if the repeated IFD1 adapter
    // reaches its structural handlers before the whole-directory guard.
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0u16.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(4u32.to_le_bytes());
    tiff.extend([2, 3, 0, 0]);
    tiff.extend(0u32.to_le_bytes());

    let mut file = tempfile::Builder::new()
        .suffix(".tiff")
        .tempfile()
        .expect("temporary TIFF");
    file.write_all(&tiff).expect("write TIFF");
    let metadata = read_metadata(file.path()).expect("synthetic TIFF parses");

    assert_eq!(metadata.get_string("ExifIFD:UserComment"), Some("aliased"));
    assert_eq!(
        metadata.get_string("InteropIFD:InteropIndex"),
        Some("R98 - DCF basic file (sRGB)")
    );
    assert_eq!(
        metadata
            .project_occurrences(ValueChannel::PrintConv)
            .filter(|(key, _, _)| *key == "InteropIFD:InteropIndex")
            .count(),
        1,
        "the observable structural child is traversed exactly once"
    );
    assert!(
        metadata.get("GPS:GPSVersionID").is_none(),
        "the repeated IFD1 adapter must not run its GPS structural handler"
    );
}

fn all_four_standard_directories_tiff() -> Vec<u8> {
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();

    // IFD0 at 8: generated Make, structural ExifIFD edge, next IFD1 at 44.
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(0x010fu16.to_le_bytes());
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(5u32.to_le_bytes());
    tiff.extend(38u32.to_le_bytes());
    tiff.extend(0x8769u16.to_le_bytes());
    tiff.extend(4u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(80u32.to_le_bytes());
    tiff.extend(44u32.to_le_bytes());
    tiff.extend_from_slice(b"Root\0");
    tiff.push(0); // align IFD1 at 44

    // IFD1 at 44: generated Model plus a hand-owned unknown entry.
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(0x0110u16.to_le_bytes());
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(6u32.to_le_bytes());
    tiff.extend(74u32.to_le_bytes());
    tiff.extend(0xdeadu16.to_le_bytes());
    tiff.extend(3u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(7u32.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    tiff.extend_from_slice(b"Thumb\0");

    // ExifIFD at 80: generated UserComment and structural Interop edge.
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(0x9286u16.to_le_bytes());
    tiff.extend(7u16.to_le_bytes());
    tiff.extend(12u32.to_le_bytes());
    tiff.extend(110u32.to_le_bytes());
    tiff.extend(0xa005u16.to_le_bytes());
    tiff.extend(4u16.to_le_bytes());
    tiff.extend(1u32.to_le_bytes());
    tiff.extend(122u32.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    tiff.extend_from_slice(b"ASCII\0\0\0exif");

    // InteropIFD at 122: generated InteropIndex.
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(2u16.to_le_bytes());
    tiff.extend(4u32.to_le_bytes());
    tiff.extend_from_slice(b"R98\0");
    tiff.extend(0u32.to_le_bytes());
    tiff
}

#[test]
fn all_four_standard_directories_keep_one_owner_group_and_physical_order() {
    let mut file = tempfile::Builder::new()
        .suffix(".tiff")
        .tempfile()
        .expect("temporary TIFF");
    file.write_all(&all_four_standard_directories_tiff())
        .expect("write TIFF");
    let metadata = read_metadata(file.path()).expect("synthetic TIFF parses");

    let expected = [
        ("IFD0:Make", TagValue::new_string("Root")),
        ("ExifIFD:UserComment", TagValue::new_string("exif")),
        (
            "InteropIFD:InteropIndex",
            TagValue::new_string("R98 - DCF basic file (sRGB)"),
        ),
        ("IFD1:Model", TagValue::new_string("Thumb")),
    ];
    let mut prior_order = None;
    for (key, expected_value) in expected {
        let occurrences: Vec<_> = metadata
            .project_occurrences(ValueChannel::PrintConv)
            .filter(|(candidate, _, _)| *candidate == key)
            .collect();
        assert_eq!(
            occurrences.len(),
            1,
            "Owner::Engine is the one named producer for {key}"
        );
        let (_, occurrence, value) = &occurrences[0];
        assert_eq!(value.as_ref(), &expected_value, "{key}");
        assert_eq!(&*occurrence.group1, "", "{key}");
        if let Some(prior) = prior_order {
            assert!(prior < occurrence.order, "physical order for {key}");
        }
        prior_order = Some(occurrence.order);
    }

    assert_eq!(
        metadata.get("IFD1:0xDEAD"),
        Some(&TagValue::new_integer(7)),
        "Owner::Hand keeps the unknown IFD1 entry on the named residual"
    );
    assert!(
        metadata.get("IFD0:ExifOffset").is_none(),
        "Owner::Silent withholds the ExifIFD parent edge"
    );
    assert!(
        metadata.get("ExifIFD:InteropOffset").is_none(),
        "Owner::Silent withholds the InteropIFD parent edge"
    );
}

struct SliceReader(Vec<u8>);

impl FileReader for SliceReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = usize::try_from(offset)
            .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "offset"))?;
        self.0
            .get(start..start.saturating_add(length))
            .ok_or_else(|| io::Error::new(ErrorKind::UnexpectedEof, "read"))
    }

    fn size(&self) -> u64 {
        self.0.len() as u64
    }
}

fn duplicate_ifd1_make_tiff() -> Vec<u8> {
    // IFD0 is empty and points to IFD1 at 14. In IFD1 the first Make cannot
    // become UTF-8 and declines; the second reports from the generated arm.
    let first = [0xff, 0x00];
    let second = b"Camera\0";
    let ifd1_offset = 14usize;
    let values_offset = ifd1_offset + 2 + 2 * 12 + 4;
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend(0u16.to_le_bytes());
    tiff.extend((ifd1_offset as u32).to_le_bytes());
    tiff.extend(2u16.to_le_bytes());
    for (bytes, offset) in [
        (first.as_slice(), values_offset),
        (second.as_slice(), values_offset + first.len()),
    ] {
        tiff.extend(0x010fu16.to_le_bytes());
        tiff.extend(2u16.to_le_bytes());
        tiff.extend((bytes.len() as u32).to_le_bytes());
        if bytes.len() <= 4 {
            let mut inline = [0u8; 4];
            inline[..bytes.len()].copy_from_slice(bytes);
            tiff.extend(inline);
        } else {
            tiff.extend((offset as u32).to_le_bytes());
        }
    }
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(first);
    tiff.extend(second);
    tiff
}

#[test]
fn ifd1_mixed_decline_and_report_runs_one_owner_per_occurrence() {
    let tiff = duplicate_ifd1_make_tiff();
    let reader = SliceReader(tiff.clone());
    let mut metadata = MetadataMap::new();
    parse_ifd1(
        &reader,
        &tiff,
        8,
        0,
        oxidex::parsers::tiff::ifd_parser::ByteOrder::LittleEndian,
        0,
        true,
        &mut metadata,
    );

    let occurrences: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(key, _, _)| *key == "IFD1:Make")
        .map(|(_, occurrence, value)| (occurrence.order, value.into_owned()))
        .collect();
    assert_eq!(occurrences.len(), 2, "one owner per physical IFD1 entry");
    assert_eq!(occurrences[0].1, TagValue::new_string("?"));
    assert_eq!(occurrences[1].1, TagValue::new_string("Camera"));
    assert!(occurrences[0].0 < occurrences[1].0);
}

#[test]
fn standalone_tiff_next_ifd_uses_the_shared_ifd1_converter() {
    let comment = b"ASCII\0\0\0linked-ifd1";
    let ifd1_offset = 14usize;
    let value_offset = ifd1_offset + 2 + 12 + 4;
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend(0u16.to_le_bytes());
    tiff.extend((ifd1_offset as u32).to_le_bytes());
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0x9286u16.to_le_bytes());
    tiff.extend(7u16.to_le_bytes());
    tiff.extend((comment.len() as u32).to_le_bytes());
    tiff.extend((value_offset as u32).to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    tiff.extend_from_slice(comment);

    let mut file = tempfile::Builder::new()
        .suffix(".tiff")
        .tempfile()
        .expect("temporary TIFF");
    file.write_all(&tiff).expect("write TIFF");
    let metadata = read_metadata(file.path()).expect("synthetic TIFF parses");

    assert_eq!(metadata.get_string("IFD1:UserComment"), Some("linked-ifd1"));
    assert_eq!(
        metadata
            .project_occurrences(ValueChannel::PrintConv)
            .filter(|(key, _, _)| *key == "IFD1:UserComment")
            .count(),
        1
    );
}
