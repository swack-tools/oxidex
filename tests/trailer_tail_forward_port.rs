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
