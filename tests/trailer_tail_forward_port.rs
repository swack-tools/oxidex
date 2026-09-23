//! Public-reader controls for Samsung SEFT, Media Jukebox APP9, and Vivo trailers.

#[path = "common/fixtures.rs"]
mod fixtures;

use oxidex::Metadata;
use oxidex::core::TagValue;
use std::io::Write;

fn push_segment(jpeg: &mut Vec<u8>, marker: u8, data: &[u8]) {
    let length = u16::try_from(data.len() + 2).expect("segment fits in a JPEG length field");
    jpeg.extend_from_slice(&[0xff, marker]);
    jpeg.extend_from_slice(&length.to_be_bytes());
    jpeg.extend_from_slice(data);
}

fn base_jpeg() -> Vec<u8> {
    vec![0xff, 0xd8]
}

fn finish_jpeg(jpeg: &mut Vec<u8>) {
    push_segment(
        jpeg,
        0xc0,
        &[0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00],
    );
    push_segment(jpeg, 0xda, &[0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
    jpeg.extend_from_slice(&[0xff, 0xd9]);
}

fn samsung_soundshot_trailer() -> Vec<u8> {
    samsung_soundshot_trailer_with_name(b"SoundShot_000")
}

fn samsung_soundshot_trailer_with_name(name: &[u8]) -> Vec<u8> {
    let audio = b"sound-shot-bytes";
    let mut trailer = Vec::new();
    trailer.extend_from_slice(&0_u32.to_be_bytes());
    trailer.extend_from_slice(&(name.len() as u32).to_le_bytes());
    trailer.extend_from_slice(name);
    trailer.extend_from_slice(audio);

    let directory_at = trailer.len();
    let mut directory = Vec::new();
    directory.extend_from_slice(b"SEFH");
    directory.extend_from_slice(&101_u32.to_le_bytes());
    directory.extend_from_slice(&1_u32.to_le_bytes());
    directory.extend_from_slice(&0_u16.to_le_bytes());
    directory.extend_from_slice(&0x0100_u16.to_le_bytes());
    directory.extend_from_slice(&(directory_at as u32).to_le_bytes());
    directory.extend_from_slice(&((8 + name.len() + audio.len()) as u32).to_le_bytes());
    // Samsung.pm walks *backward* from EOF.  A literal `QDIOBS` ends a QDIO
    // block, so its first four bytes are QDIO—not SEFT.  The SEFT directory
    // is the preceding length-delimited block.
    trailer.extend_from_slice(&directory);
    trailer.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    trailer.extend_from_slice(b"SEFT");
    trailer.extend_from_slice(&[0; 20]);
    trailer.extend_from_slice(&20_u32.to_le_bytes());
    trailer.extend_from_slice(b"QDIOBS");
    trailer
}

fn samsung_direct_seft_trailer() -> Vec<u8> {
    let mut trailer = samsung_soundshot_trailer();
    // With the QDIO payload and `QDIOBS` suffix removed, the directory's
    // little-endian length ends in `\0\0` and leaves the alternate `\0\0SEFT`
    // marker ProcessSamsung accepts at EOF.
    trailer.truncate(trailer.len() - (20 + 4 + b"QDIOBS".len()));
    trailer
}

fn samsung_trailer_with_malformed_terminal_seft() -> Vec<u8> {
    let mut trailer = samsung_soundshot_trailer();
    // Put an invalid SEFH/SEFT block immediately before QDIO.  The valid
    // directory is still farther back, so a reader that merely skips a bad
    // terminal directory would incorrectly recover the older Sound & Shot.
    let qdio_payload_at = trailer.len() - (20 + 4 + b"QDIOBS".len());
    let mut malformed = b"SEFH\x65\0\0\0".to_vec();
    malformed.extend_from_slice(&u32::MAX.to_le_bytes());
    malformed.extend_from_slice(&(malformed.len() as u32).to_le_bytes());
    malformed.extend_from_slice(b"SEFT");
    trailer.splice(qdio_payload_at..qdio_payload_at, malformed);
    trailer
}

fn vivo_trailer(json: &[u8]) -> Vec<u8> {
    let mut trailer = b"vivo".to_vec();
    trailer.extend_from_slice(json);
    trailer.push(0);
    trailer.extend_from_slice(b"\xff\xff\xff\xff\x1b*9HWfu\x84\x93\xa2\xb1");
    trailer
}

fn vivo_trailer_with_two_json_markers() -> Vec<u8> {
    let mut trailer = b"vivo{\"first\":1}\0interstitialvivo{\"second\":2}\0".to_vec();
    trailer.extend_from_slice(b"\xff\xff\xff\xff\x1b*9HWfu\x84\x93\xa2\xb1");
    trailer
}

/// JPEG.pm's APP9 handler starts XML parsing after `Media Jukebox\0`, its
/// two-byte envelope, and the six-byte `<MJMD>` root -- byte 22 overall.
fn media_jukebox_payload(fields: &[u8]) -> Vec<u8> {
    let mut payload = b"Media Jukebox\0".to_vec();
    payload.extend_from_slice(b"\x01\0<MJMD>");
    payload.extend_from_slice(fields);
    payload
}

/// A source-shaped MIE trailer whose terminal `zmie` element declares the
/// enclosing `0MIE` group. It is enough to make IdentifyTrailer advance past
/// it before retrying Samsung at the positive offset.
fn mie_trailer() -> Vec<u8> {
    let mut trailer = Vec::new();
    trailer.extend_from_slice(&[b'~', 0x10, 4, 12]);
    trailer.extend_from_slice(b"0MIEbody~\0\x04\0zmie");
    trailer.extend_from_slice(b"~\0\0\x06");
    let length = u32::try_from(trailer.len() + 6).expect("MIE trailer fits");
    trailer.extend_from_slice(&length.to_be_bytes());
    trailer.extend_from_slice(&[0x10, 4]);
    trailer
}

fn public_metadata(bytes: &[u8]) -> Metadata {
    let mut file = tempfile::NamedTempFile::new().expect("create JPEG carrier");
    file.write_all(bytes).expect("write JPEG carrier");
    file.flush().expect("flush JPEG carrier");
    Metadata::from_path(file.path()).expect("public JPEG reader parses carrier")
}

/// A little-endian TIFF with one IFD0 entry (ImageWidth = 1) and no next IFD.
fn tiff_carrier() -> Vec<u8> {
    let mut tiff = b"II*\0\x08\0\0\0".to_vec();
    tiff.extend_from_slice(&1_u16.to_le_bytes());
    tiff.extend_from_slice(&0x0100_u16.to_le_bytes());
    tiff.extend_from_slice(&3_u16.to_le_bytes());
    tiff.extend_from_slice(&1_u32.to_le_bytes());
    tiff.extend_from_slice(&1_u32.to_le_bytes());
    tiff.extend_from_slice(&0_u32.to_le_bytes());
    tiff
}

fn public_metadata_named(name: &str, bytes: &[u8]) -> Metadata {
    let dir = tempfile::tempdir().expect("create carrier directory");
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).expect("write carrier");
    Metadata::from_path(&path).expect("public reader parses carrier")
}

#[test]
fn tiff_carrier_processes_terminal_samsung_trailer() {
    // DoProcessTIFF (ExifTool.pm) calls IdentifyTrailer/ProcessTrailers after
    // IFD0, so a Samsung trailer at EOF is read from a TIFF too.  Pinned
    // ExifTool 13.59 reports MakerNotes:Samsung:EmbeddedAudioFileName here.
    let mut tiff = tiff_carrier();
    tiff.extend_from_slice(&samsung_soundshot_trailer());
    let metadata = public_metadata(&tiff);
    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(
        metadata.get("MakerNotes:EmbeddedAudioFile"),
        Some(&TagValue::new_binary(b"sound-shot-bytes".to_vec()))
    );
}

#[test]
fn non_jpeg_samsung_behind_a_bounded_mie_trailer_is_reached() {
    let mut tiff = tiff_carrier();
    tiff.extend_from_slice(&samsung_soundshot_trailer());
    tiff.extend_from_slice(&mie_trailer());
    assert_eq!(
        public_metadata(&tiff).get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
}

#[test]
fn non_jpeg_vivo_trailer_is_not_read_and_ends_the_trailer_chain() {
    // ProcessVivo (Trailer.pm) returns 0 without a JPEG TrailerStart, which
    // TIFF, PSD and CRW never set when reading; ProcessTrailers then stops,
    // so neither the Vivo JSON nor a Samsung trailer inside it is extracted.
    // Pinned ExifTool 13.59 reports neither tag for any of these carriers.
    let mut tiff = tiff_carrier();
    tiff.extend_from_slice(&samsung_soundshot_trailer());
    tiff.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    let mut vivo_only = tiff_carrier();
    vivo_only.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    for carrier in [tiff, vivo_only] {
        let metadata = public_metadata(&carrier);
        assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
        assert!(metadata.get("MakerNotes:EmbeddedAudioFile").is_none());
        assert!(metadata.get("Trailer:JSONInfo").is_none());
    }
}

#[test]
fn tiff_based_raw_carrier_uses_the_same_trailer_chain() {
    // NEF is a TIFF-module type in %fileTypeLookup, read by DoProcessTIFF.
    let mut nef = b"MM\0*\0\0\0\x08".to_vec();
    nef.extend_from_slice(&1_u16.to_be_bytes());
    nef.extend_from_slice(&0x0100_u16.to_be_bytes());
    nef.extend_from_slice(&3_u16.to_be_bytes());
    nef.extend_from_slice(&1_u32.to_be_bytes());
    nef.extend_from_slice(&(1_u32 << 16).to_be_bytes());
    nef.extend_from_slice(&0_u32.to_be_bytes());
    let mut with_samsung = nef.clone();
    with_samsung.extend_from_slice(&samsung_soundshot_trailer());
    assert_eq!(
        public_metadata_named("carrier.nef", &with_samsung)
            .get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    with_samsung.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    let metadata = public_metadata_named("carrier.nef", &with_samsung);
    assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
    assert!(metadata.get("Trailer:JSONInfo").is_none());
}

#[test]
fn carrier_without_an_exiftool_trailer_pass_ignores_samsung_trailer() {
    // GIF.pm never calls IdentifyTrailer; pinned ExifTool 13.59 reports no
    // Samsung tags for a GIF ending in a valid Samsung trailer.
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&[1, 0, 1, 0, 0, 0, 0]);
    gif.extend_from_slice(&[0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
    gif.extend_from_slice(&[0x02, 0x02, 0x44, 0x01, 0x00, 0x3b]);
    gif.extend_from_slice(&samsung_soundshot_trailer());
    let metadata = public_metadata_named("carrier.gif", &gif);
    assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
    assert!(metadata.get("MakerNotes:EmbeddedAudioFile").is_none());
}

#[test]
fn vivo_marker_search_starts_at_the_jpeg_trailer_start() {
    // ProcessVivo scans from TrailerStart (just after EOI).  A `vivo{"` in a
    // JPEG COM segment is not part of the trailer.
    let mut jpeg = base_jpeg();
    push_segment(&mut jpeg, 0xfe, b"vivo{\"com\":1}\0");
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    let metadata = public_metadata(&jpeg);
    assert_eq!(
        metadata.get_string("Trailer:JSONInfo"),
        Some("{\"version\":1000}")
    );
    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
}

#[test]
fn vivo_footer_before_an_unrecognized_tail_is_not_a_trailer() {
    // IdentifyTrailer only matches the Vivo footer at EOF (or at the next
    // positive offset of a walked chain), never an interior copy.
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    jpeg.extend_from_slice(b"junk");
    assert!(public_metadata(&jpeg).get("Trailer:JSONInfo").is_none());
}

#[test]
fn samsung_then_vivo_then_mie_chain_is_walked_inward() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    jpeg.extend_from_slice(&mie_trailer());
    let metadata = public_metadata(&jpeg);
    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(
        metadata.get_string("Trailer:JSONInfo"),
        Some("{\"version\":1000}")
    );
}

#[test]
fn vivo_inside_samsung_is_reached_through_the_samsung_trailer_length() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    let metadata = public_metadata(&jpeg);
    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(
        metadata.get_string("Trailer:JSONInfo"),
        Some("{\"version\":1000}")
    );
}

#[test]
fn samsung_behind_mie_requires_the_mie_trailer_to_be_exactly_bounded() {
    // Samsung is reachable only if the MIE trailer ends at EOF and starts
    // exactly where Samsung ends. Pinned ExifTool 13.59 reports no Samsung
    // tags for either carrier below.
    let mut junk_after = base_jpeg();
    finish_jpeg(&mut junk_after);
    junk_after.extend_from_slice(&samsung_soundshot_trailer());
    junk_after.extend_from_slice(&mie_trailer());
    junk_after.extend_from_slice(b"junk");
    let mut junk_between = base_jpeg();
    finish_jpeg(&mut junk_between);
    junk_between.extend_from_slice(&samsung_soundshot_trailer());
    junk_between.extend_from_slice(b"~junk");
    junk_between.extend_from_slice(&mie_trailer());
    for carrier in [junk_after, junk_between] {
        let metadata = public_metadata(&carrier);
        assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
        assert!(metadata.get("MakerNotes:EmbeddedAudioFile").is_none());
    }
}

#[test]
fn successful_non_jpeg_carrier_with_marker_like_tail_emits_no_trailer_tags() {
    // The dispatch probe deliberately admits this EOF marker cheaply.  The
    // Samsung parser must still reject it without a bounded SEFH directory,
    // leaving the successful TIFF carrier's ordinary metadata intact.
    let mut tiff = b"II*\0\x08\0\0\0\0\0".to_vec();
    // Empty IFDs still carry a four-byte next-IFD pointer.  The positive
    // Samsung fixture happens to begin with zeroes; this false-positive one
    // must provide the pointer explicitly before its nonzero tail bytes.
    tiff.extend_from_slice(&[0; 4]);
    tiff.extend_from_slice(b"marker-like tail only QDIOBS");
    let metadata = public_metadata(&tiff);

    assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
    assert!(metadata.get("MakerNotes:EmbeddedAudioFile").is_none());
    assert!(metadata.get("Trailer:JSONInfo").is_none());
}

#[test]
fn unknown_carrier_never_activates_trailer_parsers() {
    let mut unknown = b"not-a-recognized-carrier".to_vec();
    unknown.extend_from_slice(&samsung_soundshot_trailer());
    unknown.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    let mut file = tempfile::NamedTempFile::new().expect("create unknown carrier");
    file.write_all(&unknown).expect("write unknown carrier");
    file.flush().expect("flush unknown carrier");
    assert!(Metadata::from_path(file.path()).is_err());
}

#[test]
fn public_reader_extracts_all_three_trailer_families() {
    let mut jpeg = base_jpeg();
    let xml = media_jukebox_payload(b"<Tool_Name>Media Center</Tool_Name><Tool_Version>19.0.67</Tool_Version><People>Santa</People><Places>Jamaica</Places><Album>2013-09-01</Album><Name>Glass home at night</Name><Date>41518.8418865740750334</Date>");
    push_segment(&mut jpeg, 0xe9, &xml);
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    // This is the order in pinned ExifTool.jpg: ProcessVivo consumes its
    // fixed EOF suffix, then ProcessTrailers identifies Samsung at that offset.
    jpeg.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    let metadata = public_metadata(&jpeg);

    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(
        metadata.get("MakerNotes:EmbeddedAudioFile"),
        Some(&TagValue::new_binary(b"sound-shot-bytes".to_vec()))
    );
    for (tag, expected) in [
        ("XML:Tool_Name", "Media Center"),
        ("XML:Tool_Version", "19.0.67"),
        ("XML:People", "Santa"),
        ("XML:Places", "Jamaica"),
        ("XML:Album", "2013-09-01"),
        ("XML:Name", "Glass home at night"),
        ("XML:Date", "2013:09:01 20:12:19"),
        ("Trailer:JSONInfo", "{\"version\":1000}"),
    ] {
        assert_eq!(metadata.get_string(tag), Some(expected), "{tag}");
    }
}

#[test]
fn samsung_inner_to_vivo_eof_suffix_matches_pinned_trailer_chain() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    let metadata = public_metadata(&jpeg);

    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(
        metadata.get_string("Trailer:JSONInfo"),
        Some("{\"version\":1000}")
    );
}

#[test]
fn vivo_uses_first_marker_for_json_and_samsung_trailer_boundary() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(&vivo_trailer_with_two_json_markers());
    let metadata = public_metadata(&jpeg);

    // Trailer.pm's first marker supplies both the trailer boundary and the
    // first `}\0`-bounded JSON value; it does not select the last marker.
    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(
        metadata.get_string("Trailer:JSONInfo"),
        Some("{\"first\":1}")
    );
}

#[test]
fn samsung_internal_marker_without_bounded_vivo_suffix_is_withheld() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(b"unrelated trailing bytes");
    let metadata = public_metadata(&jpeg);

    assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
    assert!(metadata.get("MakerNotes:EmbeddedAudioFile").is_none());
}

#[test]
fn non_finite_media_jukebox_date_is_withheld() {
    let mut jpeg = base_jpeg();
    push_segment(
        &mut jpeg,
        0xe9,
        &media_jukebox_payload(b"<Tool_Name>Media Center</Tool_Name><Date>NaN</Date>"),
    );
    finish_jpeg(&mut jpeg);
    let metadata = public_metadata(&jpeg);

    assert_eq!(metadata.get_string("XML:Tool_Name"), Some("Media Center"));
    assert!(metadata.get("XML:Date").is_none());
}

#[test]
fn media_jukebox_requires_the_complete_22_byte_directory_start() {
    let mut immediate = base_jpeg();
    push_segment(
        &mut immediate,
        0xe9,
        b"Media Jukebox\0<MJMD><Name>wrong immediate XML</Name></MJMD>",
    );
    finish_jpeg(&mut immediate);
    assert!(public_metadata(&immediate).get("XML:Name").is_none());

    let mut enveloped = base_jpeg();
    push_segment(
        &mut enveloped,
        0xe9,
        &media_jukebox_payload(b"<Name>right envelope</Name>"),
    );
    finish_jpeg(&mut enveloped);
    assert_eq!(
        public_metadata(&enveloped).get_string("XML:Name"),
        Some("right envelope")
    );
}

#[test]
fn media_jukebox_unescapes_text_and_uses_pinned_unix_time_conversion() {
    let mut jpeg = base_jpeg();
    push_segment(
        &mut jpeg,
        0xe9,
        &media_jukebox_payload(b"<Name>Rock &amp; Roll</Name><Date>25569</Date>"),
    );
    finish_jpeg(&mut jpeg);
    let metadata = public_metadata(&jpeg);
    assert_eq!(metadata.get_string("XML:Name"), Some("Rock & Roll"));
    assert_eq!(metadata.get_string("XML:Date"), Some("0000:00:00 00:00:00"));
}

#[test]
fn samsung_before_a_bounded_mie_trailer_remains_visible() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(&mie_trailer());
    let metadata = public_metadata(&jpeg);
    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(metadata.get_string("MIE:TrailerSignature"), Some(""));
}

#[test]
fn samsung_before_mie_then_vivo_chain_remains_visible() {
    // ProcessTrailers advances at each independently bounded suffix.  This is
    // the composed form: Samsung becomes reachable only after MIE and then
    // Vivo have each exposed the next positive offset from EOF.
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(&mie_trailer());
    jpeg.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    let metadata = public_metadata(&jpeg);

    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(metadata.get_string("MIE:TrailerSignature"), Some(""));
    assert_eq!(
        metadata.get_string("Trailer:JSONInfo"),
        Some("{\"version\":1000}")
    );
}

#[test]
fn malformed_or_false_positive_tail_data_emits_nothing() {
    let mut jpeg = base_jpeg();
    push_segment(
        &mut jpeg,
        0xe9,
        b"Media Jukebox<MJMD><Tool_Name>wrong prefix</Tool_Name>",
    );
    finish_jpeg(&mut jpeg);
    // A literal footer alone is not a Samsung carrier: ProcessSamsung must
    // find a bounded SEFT/SEFH block while walking backward through QDIO.
    jpeg.extend_from_slice(b"SEFH\x65\0\0\0\x01\0\0\0\x14\0\0\0SEFT");
    jpeg.extend_from_slice(&[0; 20]);
    jpeg.extend_from_slice(b"\x14\0\0\0QDIOBS");
    jpeg.extend_from_slice(b"vivo{\"version\":1000}\0");
    let metadata = public_metadata(&jpeg);

    for tag in [
        "MakerNotes:EmbeddedAudioFileName",
        "MakerNotes:EmbeddedAudioFile",
        "XML:Tool_Name",
        "Trailer:JSONInfo",
    ] {
        assert!(metadata.get(tag).is_none(), "false-positive {tag}");
    }
}

#[test]
fn truncated_samsung_entry_and_vivo_terminator_are_withheld() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    let mut samsung = samsung_soundshot_trailer();
    samsung.truncate(samsung.len() - 12);
    jpeg.extend_from_slice(&samsung);
    let mut malformed_vivo = b"vivo{\"version\":1000".to_vec();
    malformed_vivo.extend_from_slice(b"\xff\xff\xff\xff\x1b*9HWfu\x84\x93\xa2\xb1");
    jpeg.extend_from_slice(&malformed_vivo);
    let metadata = public_metadata(&jpeg);

    assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
    assert!(metadata.get("Trailer:JSONInfo").is_none());
}

#[test]
fn non_utf8_samsung_name_uses_pinned_native_public_representation() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer_with_name(b"A\xffB"));

    // Pinned ExifTool 13.59 renders this raw name as `A?B` in its public JSON
    // output (while `-b` preserves the original `41 ff 42` bytes).
    assert_eq!(
        public_metadata(&jpeg).get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("A?B")
    );
}

#[test]
fn samsung_direct_seft_footer_is_bounded_and_publicly_readable() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_direct_seft_trailer());
    assert_eq!(
        public_metadata(&jpeg).get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
}

#[test]
fn malformed_terminal_samsung_seft_does_not_fall_back_to_earlier_directory() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_trailer_with_malformed_terminal_seft());
    let metadata = public_metadata(&jpeg);

    // Samsung.pm stops the backward walk when `12 + 12 * count` exceeds the
    // terminal SEFT block. It must not continue to an older valid-looking one.
    assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
    assert!(metadata.get("MakerNotes:EmbeddedAudioFile").is_none());
}

#[test]
fn media_jukebox_preserves_field_whitespace_and_resolves_references() {
    // XMP.pm keeps an element's text verbatim (it trims only rdf:Description)
    // and UnescapeXML resolves predefined and numeric references, leaving an
    // unknown `&foo;` as written.
    let mut jpeg = base_jpeg();
    push_segment(
        &mut jpeg,
        0xe9,
        &media_jukebox_payload(
            b"<Name>  padded  </Name><Caption>A&#66;C &lt;x&gt; &foo;</Caption><Album>\n x \n</Album>",
        ),
    );
    finish_jpeg(&mut jpeg);
    let metadata = public_metadata(&jpeg);
    assert_eq!(metadata.get_string("XML:Name"), Some("  padded  "));
    assert_eq!(metadata.get_string("XML:Caption"), Some("ABC <x> &foo;"));
    assert_eq!(metadata.get_string("XML:Album"), Some("\n x \n"));
}

#[test]
fn media_jukebox_date_rounds_half_seconds_to_even() {
    // 25569 + 3/256 days is exactly 1012.5 s after the Unix epoch.  Pinned
    // ConvertUnixTime rounds the tie to even: 00:16:52, not 00:16:53.
    let mut jpeg = base_jpeg();
    push_segment(
        &mut jpeg,
        0xe9,
        &media_jukebox_payload(b"<Date>25569.01171875</Date>"),
    );
    finish_jpeg(&mut jpeg);
    assert_eq!(
        public_metadata(&jpeg).get_string("XML:Date"),
        Some("1970:01:01 00:16:52")
    );
}

#[test]
fn jpeg_trailer_start_is_found_by_walking_entropy_coded_data() {
    // TrailerStart is the byte after EOI, which ProcessJPEG reaches by
    // walking on from SOS: stuffed `ff 00` and RST markers stand alone, and a
    // DHT between scans is skipped by its length (its fake `ff d9` and
    // `vivo{"` are payload). Pinned ExifTool 13.59 reports the trailer's JSON.
    let mut jpeg = base_jpeg();
    push_segment(
        &mut jpeg,
        0xc0,
        &[0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00],
    );
    push_segment(&mut jpeg, 0xda, &[0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
    jpeg.extend_from_slice(b"\x12\xff\x00\x34\xff\xd0vivo{\"scan\":1}\0\x56");
    push_segment(&mut jpeg, 0xc4, b"\0\xff\xd9vivo{\"early\":1}\0");
    push_segment(&mut jpeg, 0xda, &[0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
    jpeg.extend_from_slice(b"\x9a\xff\xff\x00\xbc\xff\xd9");
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    let metadata = public_metadata(&jpeg);
    assert_eq!(
        metadata.get_string("Trailer:JSONInfo"),
        Some("{\"version\":1000}")
    );
    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
}

#[test]
fn pinned_exiftool_jpeg_walks_vivo_then_samsung() {
    let Some(path) = fixtures::pinned_t_images_fixture_path("ExifTool.jpg") else {
        eprintln!("skipping: pinned fixture ExifTool.jpg is absent");
        return;
    };
    let metadata = Metadata::from_path(&path).expect("ExifTool.jpg parses");
    // Pinned ExifTool 13.59: Vivo (340 bytes at 0x64a6), then Samsung.
    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(
        metadata.get_string("Trailer:JSONInfo"),
        Some(concat!(
            "{\"com.android.camera.joint.fullview.orientation\":0,",
            "\"com.android.camera.hdr\":20737,\"com.android.camera.fisheye\":-1,",
            "\"com.android.camera.joint.conshoot\":0,",
            "\"com.android.camera.moduleid\":\"photo\",",
            "\"com.android.camera.joint.motioncapture\":0,\"version\":1000,",
            "\"com.android.camera.joint.fullview\":false}"
        ))
    );
}

/// SOI, a JFIF APP0 and SOF0, without SOS: the prefix of the reviewer's
/// no-scan probes.
fn jpeg_prefix_without_scan() -> Vec<u8> {
    let mut jpeg = base_jpeg();
    push_segment(&mut jpeg, 0xe0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0");
    push_segment(
        &mut jpeg,
        0xc0,
        &[0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00],
    );
    jpeg
}

#[test]
fn jpeg_that_never_reaches_sos_reads_no_trailers() {
    // ProcessJPEG calls IdentifyTrailer/ProcessTrailers only on reaching SOS
    // (ExifTool.pm:7627-7634); at EOI it merely finishes a walk begun there.
    // Pinned ExifTool 13.59 reports "Missing JPEG SOS" / "JPEG format error"
    // and no Samsung or Vivo tags for any of these carriers.
    let mut eoi_then_samsung = jpeg_prefix_without_scan();
    eoi_then_samsung.extend_from_slice(b"\xff\xd9");
    eoi_then_samsung.extend_from_slice(&samsung_soundshot_trailer());
    let mut eoi_then_vivo = jpeg_prefix_without_scan();
    eoi_then_vivo.extend_from_slice(b"\xff\xd9");
    eoi_then_vivo.extend_from_slice(&vivo_trailer(b"{\"a\":1}"));
    let mut no_eoi_samsung = jpeg_prefix_without_scan();
    no_eoi_samsung.extend_from_slice(&samsung_soundshot_trailer());
    for carrier in [eoi_then_samsung, eoi_then_vivo, no_eoi_samsung] {
        let metadata = public_metadata(&carrier);
        assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
        assert!(metadata.get("MakerNotes:EmbeddedAudioFile").is_none());
        assert!(metadata.get("Trailer:JSONInfo").is_none());
    }
}

fn media_jukebox_metadata(fields: &[u8]) -> Metadata {
    let mut jpeg = base_jpeg();
    push_segment(&mut jpeg, 0xe9, &media_jukebox_payload(fields));
    finish_jpeg(&mut jpeg);
    public_metadata(&jpeg)
}

#[test]
fn media_jukebox_date_skips_only_ascii_whitespace() {
    // Perl numification skips ASCII whitespace only: a leading NBSP makes the
    // value 0 (pinned: 1899:12:30 00:00:00), never 45000 days.
    let metadata = media_jukebox_metadata(b"<Date>\xc2\xa045000</Date>");
    assert_ne!(metadata.get_string("XML:Date"), Some("2023:03:15 00:00:00"));
    assert_eq!(
        media_jukebox_metadata(b"<Date>\t45000 \n</Date>").get_string("XML:Date"),
        Some("2023:03:15 00:00:00")
    );
}

#[test]
fn media_jukebox_publishes_only_leaf_children_of_the_root() {
    // XMP.pm names a nested element by its path (FooCaption, CaptionName,
    // CaptionFoo in pinned 13.59); none of these is a plain Caption or Name.
    for fields in [
        &b"<Foo><Caption>x</Caption></Foo>"[..],
        b"<Caption><Name>x</Name></Caption>",
        b"<Caption>a<Foo/>b</Caption>",
    ] {
        let metadata = media_jukebox_metadata(fields);
        assert!(metadata.get("XML:Caption").is_none(), "{fields:?}");
        assert!(metadata.get("XML:Name").is_none(), "{fields:?}");
    }
    // Empty and self-closing leaves are still values ("" in pinned 13.59).
    let metadata = media_jukebox_metadata(b"<Caption></Caption><Name/>");
    assert_eq!(metadata.get_string("XML:Caption"), Some(""));
    assert_eq!(metadata.get_string("XML:Name"), Some(""));
}

#[test]
fn media_jukebox_repeated_field_keeps_the_last_value() {
    // Pinned 13.59 reports one Caption, "b", for two Caption elements, even
    // with -a (XMP.pm's later property replaces the earlier one).
    let mut jpeg = base_jpeg();
    push_segment(
        &mut jpeg,
        0xe9,
        &media_jukebox_payload(b"<Caption>a</Caption><Caption>b</Caption>"),
    );
    finish_jpeg(&mut jpeg);
    let mut file = tempfile::NamedTempFile::new().expect("create JPEG carrier");
    file.write_all(&jpeg).expect("write JPEG carrier");
    let map = oxidex::core::operations::read_metadata(file.path()).expect("JPEG parses");
    let captions: Vec<TagValue> = map
        .project_occurrences(oxidex::core::tag_occurrence::ValueChannel::PrintConv)
        .filter(|(key, _, _)| *key == "XML:Caption")
        .map(|(_, _, value)| value.into_owned())
        .collect();
    assert_eq!(captions, [TagValue::new_string("b")]);
}

#[test]
fn media_jukebox_unresolvable_numeric_reference_withholds_the_field() {
    // UnescapeXML turns these into raw code points that are not valid text;
    // pinned 13.59 prints "ab", "???" and "????". Never publish them literally.
    let metadata = media_jukebox_metadata(
        b"<Caption>a&#0;b</Caption><Name>&#xD800;</Name><Album>&#x110000;</Album><People>&nbsp;</People><Places>&#x41;</Places>",
    );
    assert!(metadata.get("XML:Caption").is_none());
    assert!(metadata.get("XML:Name").is_none());
    assert!(metadata.get("XML:Album").is_none());
    // An unknown named reference is kept as written, as pinned 13.59 does.
    assert_eq!(metadata.get_string("XML:People"), Some("&nbsp;"));
    assert_eq!(metadata.get_string("XML:Places"), Some("A"));
}

#[test]
fn media_jukebox_empty_field_with_shorthand_attributes_is_not_a_value() {
    // XMP.pm turns attributes into shorthand properties (CaptionA) and then
    // publishes an empty element only `if (length $val or not $shorthand)`.
    // Pinned 13.59: no Caption for the first two; the earlier value stands.
    for fields in [&b"<Caption a=\"1\"/>"[..], b"<Caption a=\"1\"></Caption>"] {
        assert!(
            media_jukebox_metadata(fields).get("XML:Caption").is_none(),
            "{fields:?}"
        );
    }
    // Pinned 13.59 keeps the earlier "real" here. Whether an attribute is
    // shorthand is not transcribed, so the field is withheld instead: never
    // "" over the earlier value.
    for fields in [
        &b"<Caption>real</Caption><Caption a=\"1\"/>"[..],
        b"<Caption>real</Caption><Caption a=\"1\"><!-- c --></Caption>",
    ] {
        assert!(
            matches!(
                media_jukebox_metadata(fields).get_string("XML:Caption"),
                None | Some("real")
            ),
            "{fields:?}"
        );
    }
    // A non-empty value is still published, and xml:lang / xmlns are not
    // shorthand: pinned 13.59 reports " " and "" here.
    assert_eq!(
        media_jukebox_metadata(b"<Caption a=\"1\"> </Caption>").get_string("XML:Caption"),
        Some(" ")
    );
    assert_eq!(
        media_jukebox_metadata(b"<Caption>real</Caption><Caption xml:lang=\"en\"/>")
            .get_string("XML:Caption"),
        Some("")
    );
    assert_eq!(
        media_jukebox_metadata(b"<Caption xmlns:q=\"u\"/>").get_string("XML:Caption"),
        Some("")
    );
}

#[test]
fn media_jukebox_rdf_attribute_value_is_withheld() {
    // An empty element takes its value from rdf:value / rdf:resource /
    // rdf:about (pinned 13.59: Caption "z", Name "v", Caption "q"). That
    // value is not reproduced here, so the field is withheld -- including an
    // earlier value it would have replaced.
    for (fields, tag) in [
        (&b"<Caption rdf:resource=\"z\"/>"[..], "XML:Caption"),
        (b"<Name rdf:value=\"v\"></Name>", "XML:Name"),
        (
            b"<Caption>real</Caption><Caption rdf:about=\"q\"/>",
            "XML:Caption",
        ),
        (
            b"<Name>real</Name><Name rdf:resource=\"z\"></Name>",
            "XML:Name",
        ),
    ] {
        assert!(
            media_jukebox_metadata(fields).get(tag).is_none(),
            "{fields:?}"
        );
    }
}

#[test]
fn media_jukebox_field_with_processing_instruction_is_withheld() {
    // Pinned 13.59 keeps the PI text verbatim ("a<?pi x?>b"); never publish
    // "ab", nor leave an earlier value it replaces.
    for fields in [
        &b"<Caption>a<?pi x?>b</Caption>"[..],
        b"<Caption>real</Caption><Caption>a<?pi x?>b</Caption>",
    ] {
        assert!(
            media_jukebox_metadata(fields).get("XML:Caption").is_none(),
            "{fields:?}"
        );
    }
}

/// A published Media Jukebox value must equal pinned 13.59's or be absent.
fn assert_media_jukebox_absent_or(fields: &[u8], tag: &str, oracle: Option<&str>) {
    let actual = media_jukebox_metadata(fields)
        .get_string(tag)
        .map(str::to_owned);
    assert!(
        actual.is_none() || actual.as_deref() == oracle,
        "{}: {tag} = {actual:?}, pinned 13.59 {oracle:?}",
        String::from_utf8_lossy(fields)
    );
}

#[test]
fn media_jukebox_empty_field_with_any_attribute_is_withheld() {
    // Attributes in namespaces XMP.pm ignores (`x:`, rdf:parseType, rdf:ID,
    // rdf:foo) are not shorthand there, so the empty element publishes ""
    // over the earlier value; a bare default `xmlns` IS shorthand
    // (KeywordsXmlns), so nothing is published. Only xml:lang and
    // xmlns:<prefix> are treated as neutral here; otherwise an empty field
    // is withheld rather than guessed.
    for (fields, oracle) in [
        (&b"<Name>real</Name><Name x:c=\"3\"></Name>"[..], Some("")),
        (
            b"<Name>real</Name><Name rdf:parseType=\"Resource\"/>",
            Some(""),
        ),
        (b"<Name>real</Name><Name rdf:ID=\"i\"></Name>", Some("")),
        (b"<Name>real</Name><Name rdf:foo=\"f\"/>", Some("")),
    ] {
        assert_media_jukebox_absent_or(fields, "XML:Name", oracle);
    }
    // Neutral attributes and non-empty values still publish.
    assert_eq!(
        media_jukebox_metadata(b"<Caption>real</Caption><Caption xml:lang=\"en\"/>")
            .get_string("XML:Caption"),
        Some("")
    );
    assert_eq!(
        media_jukebox_metadata(b"<Caption a=\"1\">x</Caption>").get_string("XML:Caption"),
        Some("x")
    );
}

#[test]
fn media_jukebox_field_with_cdata_is_withheld() {
    // XMP.pm counts `<![CDATA[]]>` as raw text (pinned 13.59: Name "" over
    // the earlier "real"); a field holding CDATA is withheld.
    for fields in [
        &b"<Name>real</Name><Name xmp:y=\"6\"><![CDATA[]]></Name>"[..],
        b"<Name><![CDATA[]]></Name>",
    ] {
        assert_media_jukebox_absent_or(fields, "XML:Name", Some(""));
        assert_ne!(
            media_jukebox_metadata(fields).get_string("XML:Name"),
            Some("real")
        );
    }
}

#[test]
fn media_jukebox_field_with_rdf_node_id_is_withheld() {
    // Pinned 13.59 hits "internal error parsing nodeID's" and drops the
    // nodeID field while keeping its siblings.
    let metadata = media_jukebox_metadata(
        b"<Caption>a</Caption><Name rdf:nodeID=\"n\">b</Name><Album>c</Album>",
    );
    assert!(metadata.get("XML:Name").is_none());
    assert_eq!(metadata.get_string("XML:Caption"), Some("a"));
    assert_eq!(metadata.get_string("XML:Album"), Some("c"));
    assert_media_jukebox_absent_or(
        b"<Album/><Album rdf:nodeID=\"n\">45000</Album>",
        "XML:Album",
        Some(""),
    );
}

#[test]
fn media_jukebox_empty_field_with_default_xmlns_is_withheld() {
    // A bare default `xmlns` is shorthand in XMP.pm (KeywordsXmlns), so
    // pinned 13.59 publishes no Keywords for the empty element.
    assert_media_jukebox_absent_or(b"<Keywords xmlns=\"u\"/>", "XML:Keywords", None);
    assert_media_jukebox_absent_or(
        b"<Keywords>real</Keywords><Keywords xmlns=\"u\"/>",
        "XML:Keywords",
        Some("real"),
    );
}

#[test]
fn tiff_backed_raw_carrier_uses_the_trailer_chain() {
    // `RAW => [['RAW','TIFF'], ...]` with magic `(.{25}ARECOYK|II|MM)`: a
    // `.raw` that is not Kyocera is read by DoProcessTIFF, which walks
    // trailers. Pinned 13.59 reports Samsung:EmbeddedAudioFileName for the
    // II, MM and Panasonic (0x55) headers below.
    let mut little = tiff_carrier();
    little.extend_from_slice(&samsung_soundshot_trailer());
    let mut big = b"MM\0*\0\0\0\x08".to_vec();
    big.extend_from_slice(&1_u16.to_be_bytes());
    big.extend_from_slice(&0x0100_u16.to_be_bytes());
    big.extend_from_slice(&3_u16.to_be_bytes());
    big.extend_from_slice(&1_u32.to_be_bytes());
    big.extend_from_slice(&(1_u32 << 16).to_be_bytes());
    big.extend_from_slice(&0_u32.to_be_bytes());
    big.extend_from_slice(&samsung_soundshot_trailer());
    let mut panasonic = tiff_carrier();
    panasonic[2] = 0x55;
    panasonic.extend_from_slice(&samsung_soundshot_trailer());
    for carrier in [little, big, panasonic] {
        assert_eq!(
            public_metadata_named("carrier.raw", &carrier)
                .get_string("MakerNotes:EmbeddedAudioFileName"),
            Some("SoundShot_000")
        );
    }
}

#[test]
fn kyocera_or_unreadable_raw_carrier_reads_no_trailers() {
    // KyoceraRaw::ProcessRAW never walks trailers, a header without II/MM
    // and an IFD0 offset >= 8 never reaches DoProcessTIFF's trailer pass, and
    // Vivo still ends the TIFF chain. Pinned 13.59: no Samsung tags for any.
    let mut kyocera = vec![0_u8; 156];
    kyocera[0x19..0x19 + 7].copy_from_slice(b"ARECOYK");
    kyocera.extend_from_slice(&samsung_soundshot_trailer());
    let mut bad_offset = b"II*\0\x04\0\0\0".to_vec();
    bad_offset.extend_from_slice(&samsung_soundshot_trailer());
    let mut junk = b"JUNKJUNKJUNKJUNK".to_vec();
    junk.extend_from_slice(&samsung_soundshot_trailer());
    let mut vivo_chain = tiff_carrier();
    vivo_chain.extend_from_slice(&samsung_soundshot_trailer());
    vivo_chain.extend_from_slice(&vivo_trailer(b"{\"a\":1}"));
    for carrier in [kyocera, bad_offset, junk, vivo_chain] {
        let dir = tempfile::tempdir().expect("create carrier directory");
        let path = dir.path().join("carrier.raw");
        std::fs::write(&path, &carrier).expect("write carrier");
        // An unrecognized carrier is an error, which reads no trailers either.
        if let Ok(metadata) = Metadata::from_path(&path) {
            assert!(metadata.get("MakerNotes:EmbeddedAudioFileName").is_none());
            assert!(metadata.get("Trailer:JSONInfo").is_none());
        }
    }
}

/// A two-scan JPEG whose second SOS header carries component selector
/// `cs` and table selector `tables`, followed by `trailer`.
fn multiscan_jpeg(cs: u8, tables: u8, trailer: &[u8]) -> Vec<u8> {
    let mut jpeg = base_jpeg();
    push_segment(
        &mut jpeg,
        0xc0,
        &[0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0xff, 0x11, 0x00],
    );
    push_segment(&mut jpeg, 0xda, &[0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
    jpeg.extend_from_slice(b"\x12\x34");
    let mut dht = vec![0x10];
    dht.extend_from_slice(&[0; 16]);
    push_segment(&mut jpeg, 0xc4, &dht);
    push_segment(&mut jpeg, 0xda, &[0x01, cs, tables, 0x00, 0x3f, 0x00]);
    jpeg.extend_from_slice(b"\x56\xff\xd9");
    jpeg.extend_from_slice(trailer);
    jpeg
}

#[test]
fn later_sos_headers_are_scanned_as_exiftool_scans_them() {
    // ProcessJPEG treats SOS as a stand-alone marker (`%markerLenBytes`
    // 0xda => 0) and scans on from right after it, header bytes included.
    // Pinned 13.59 for Samsung + Vivo: selector `ff 00` -> both tags;
    // `ff 11` -> "JPEG format error", neither; `ff d9` -> an early EOI whose
    // TrailerStart still finds both.
    let mut trailer = samsung_soundshot_trailer();
    trailer.extend_from_slice(&vivo_trailer(b"{\"a\":1}"));
    for (tables, reads_trailers) in [(0x00, true), (0x11, false), (0xd9, true)] {
        let metadata = public_metadata(&multiscan_jpeg(0xff, tables, &trailer));
        let expected_name = reads_trailers.then_some("SoundShot_000");
        let expected_json = reads_trailers.then_some("{\"a\":1}");
        assert_eq!(
            metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
            expected_name,
            "{tables:#x}"
        );
        assert_eq!(
            metadata.get_string("Trailer:JSONInfo"),
            expected_json,
            "{tables:#x}"
        );
    }
}

#[test]
fn media_jukebox_node_id_is_an_attribute_key_not_attribute_text() {
    // Pinned 13.59: `note="rdf:nodeID"` is an ordinary attribute (NameNote),
    // so Name "Album" is published.
    assert_eq!(
        media_jukebox_metadata(b"<Name note=\"rdf:nodeID\">Album</Name>").get_string("XML:Name"),
        Some("Album")
    );
    // XMP.pm resolves the prefix: `r:nodeID` with `xmlns:r` bound to the RDF
    // namespace is still rdf:nodeID, and pinned 13.59 drops Name. Any
    // `*:nodeID` key is withheld here rather than resolved.
    let rdf = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
    for fields in [
        format!("<Caption>a</Caption><Name xmlns:r=\"{rdf}\" r:nodeID=\"n\">b</Name>"),
        format!("<Caption>a</Caption><Name r:nodeID=\"n\" xmlns:r=\"{rdf}\">b</Name>"),
        "<Caption>a</Caption><Name rdf:nodeID = \"n\">b</Name>".to_string(),
    ] {
        let metadata = media_jukebox_metadata(fields.as_bytes());
        assert!(metadata.get("XML:Name").is_none(), "{fields}");
        assert_eq!(metadata.get_string("XML:Caption"), Some("a"), "{fields}");
    }
    // `rdf` rebound to another namespace, or a nested prefix, is not
    // rdf:nodeID in pinned 13.59 (Name "b"): published or withheld, never
    // anything else.
    for fields in [
        &b"<Caption>a</Caption><Name xmlns:rdf=\"urn:other\" rdf:nodeID=\"n\">b</Name>"[..],
        b"<Caption>a</Caption><Name x:rdf:nodeID=\"n\">b</Name>",
    ] {
        assert_media_jukebox_absent_or(fields, "XML:Name", Some("b"));
    }
}

#[test]
fn zero_size_photo_mechanic_trailer_still_exposes_samsung() {
    // ProcessPhotoMechanic accepts `size == 0` (a 12-byte trailer); pinned
    // 13.59 then reaches Samsung, and Vivo outside it, on JPEG and TIFF.
    let zero_photo_mechanic = b"\0\0\0\0cbipcbbl";
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(zero_photo_mechanic);
    let mut tiff = tiff_carrier();
    tiff.extend_from_slice(&samsung_soundshot_trailer());
    tiff.extend_from_slice(zero_photo_mechanic);
    for carrier in [&jpeg, &tiff] {
        assert_eq!(
            public_metadata(carrier).get_string("MakerNotes:EmbeddedAudioFileName"),
            Some("SoundShot_000")
        );
    }
    jpeg.extend_from_slice(&vivo_trailer(b"{\"a\":1}"));
    let metadata = public_metadata(&jpeg);
    assert_eq!(
        metadata.get_string("MakerNotes:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(metadata.get_string("Trailer:JSONInfo"), Some("{\"a\":1}"));
}

#[test]
fn tiff_form_hasselblad_fff_uses_the_trailer_chain() {
    // `FFF => [['TIFF','FLIR'], ...]`: a TIFF-form `.fff` is read by
    // DoProcessTIFF, which walks trailers. Pinned 13.59 reports
    // Samsung:EmbeddedAudioFileName for II and MM headers, including one with
    // Kyocera-looking bytes at 0x19 (only `.raw` tries KyoceraRaw first).
    let mut little = tiff_carrier();
    little.extend_from_slice(&samsung_soundshot_trailer());
    let mut big = b"MM\0*\0\0\0\x08".to_vec();
    big.extend_from_slice(&1_u16.to_be_bytes());
    big.extend_from_slice(&0x0100_u16.to_be_bytes());
    big.extend_from_slice(&3_u16.to_be_bytes());
    big.extend_from_slice(&1_u32.to_be_bytes());
    big.extend_from_slice(&(1_u32 << 16).to_be_bytes());
    big.extend_from_slice(&0_u32.to_be_bytes());
    big.extend_from_slice(&samsung_soundshot_trailer());
    let mut kyocera_bytes = tiff_carrier();
    kyocera_bytes.resize(kyocera_bytes.len() + 160, 0);
    kyocera_bytes[0x19..0x19 + 7].copy_from_slice(b"ARECOYK");
    kyocera_bytes.extend_from_slice(&samsung_soundshot_trailer());
    let mut behind_mie = tiff_carrier();
    behind_mie.extend_from_slice(&samsung_soundshot_trailer());
    behind_mie.extend_from_slice(&mie_trailer());
    for carrier in [little, big, kyocera_bytes, behind_mie] {
        assert_eq!(
            public_metadata_named("carrier.fff", &carrier)
                .get_string("MakerNotes:EmbeddedAudioFileName"),
            Some("SoundShot_000")
        );
    }
}

/// Reads `carrier` under `name`; an unrecognized carrier is an error, which
/// reads no trailers either.
fn assert_no_trailer_tags(name: &str, carrier: &[u8]) {
    let dir = tempfile::tempdir().expect("create carrier directory");
    let path = dir.path().join(name);
    std::fs::write(&path, carrier).expect("write carrier");
    if let Ok(metadata) = Metadata::from_path(&path) {
        assert!(
            metadata.get("MakerNotes:EmbeddedAudioFileName").is_none(),
            "{name}"
        );
        assert!(
            metadata.get("MakerNotes:EmbeddedAudioFile").is_none(),
            "{name}"
        );
        assert!(metadata.get("Trailer:JSONInfo").is_none(), "{name}");
    }
}

#[test]
fn flir_form_or_unreadable_fff_reads_no_trailers() {
    // FLIR-form (`FFF\0`/`AFF\0`) files are read by FLIR.pm, which never
    // walks trailers; a TIFF header whose IFD0 offset is below 8 never
    // reaches DoProcessTIFF's trailer pass; Vivo still ends the TIFF chain.
    // Pinned 13.59: no Samsung or Vivo tags for any of these.
    let mut flir_body = b"Test".to_vec();
    flir_body.resize(16, 0);
    flir_body.extend_from_slice(&100_u32.to_be_bytes());
    flir_body.extend_from_slice(&[0; 40]);
    for magic in [b"FFF\0", b"AFF\0"] {
        let mut flir = magic.to_vec();
        flir.extend_from_slice(&flir_body);
        flir.extend_from_slice(&samsung_soundshot_trailer());
        assert_no_trailer_tags("carrier.fff", &flir);
    }
    let mut bad_offset = b"II*\0\x04\0\0\0".to_vec();
    bad_offset.extend_from_slice(&samsung_soundshot_trailer());
    assert_no_trailer_tags("carrier.fff", &bad_offset);
    let mut vivo_chain = tiff_carrier();
    vivo_chain.extend_from_slice(&samsung_soundshot_trailer());
    vivo_chain.extend_from_slice(&vivo_trailer(b"{\"a\":1}"));
    assert_no_trailer_tags("carrier.fff", &vivo_chain);
}

#[test]
fn tiff_family_raw_with_a_broken_ifd_still_walks_trailers() {
    // DoProcessTIFF ignores ProcessDirectory's result and walks trailers once
    // the header (II/MM, IFD0 offset >= 8) is accepted. Pinned 13.59 reports
    // Samsung:EmbeddedAudioFileName, with a "Bad IFD0/IFD1 directory" or
    // "Error reading value" warning, for every carrier below.
    let entry = |tag: u16| {
        let mut bytes = tag.to_le_bytes().to_vec();
        bytes.extend_from_slice(&3_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes
    };
    let offset_past_eof = [&b"II*\0"[..], &0x10_0000_u32.to_le_bytes()].concat();
    let count_past_eof = [
        &b"II*\0\x08\0\0\0"[..],
        &500_u16.to_le_bytes(),
        &entry(0x100),
    ]
    .concat();
    let truncated_entry = [
        &b"II*\0\x08\0\0\0"[..],
        &2_u16.to_le_bytes(),
        &entry(0x100),
        b"\x01\x01\x03",
    ]
    .concat();
    let bad_next_ifd = [
        &b"II*\0\x08\0\0\0"[..],
        &1_u16.to_le_bytes(),
        &entry(0x100),
        &0x7fff_ffff_u32.to_le_bytes(),
    ]
    .concat();
    for body in [
        offset_past_eof,
        count_past_eof,
        truncated_entry,
        bad_next_ifd,
    ] {
        let mut carrier = body.clone();
        carrier.extend_from_slice(&samsung_soundshot_trailer());
        for name in ["carrier.fff", "carrier.raw", "carrier.nef", "carrier.dng"] {
            assert_eq!(
                public_metadata_named(name, &carrier)
                    .get_string("MakerNotes:EmbeddedAudioFileName"),
                Some("SoundShot_000"),
                "{name}"
            );
        }
    }
}
