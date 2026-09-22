//! Public-reader controls for Samsung SEFT, Media Jukebox APP9, and Vivo trailers.

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

fn vivo_trailer(json: &[u8]) -> Vec<u8> {
    let mut trailer = b"vivo".to_vec();
    trailer.extend_from_slice(json);
    trailer.push(0);
    trailer.extend_from_slice(b"\xff\xff\xff\xff\x1b*9HWfu\x84\x93\xa2\xb1");
    trailer
}

fn public_metadata(bytes: &[u8]) -> Metadata {
    let mut file = tempfile::NamedTempFile::new().expect("create JPEG carrier");
    file.write_all(bytes).expect("write JPEG carrier");
    file.flush().expect("flush JPEG carrier");
    Metadata::from_path(file.path()).expect("public JPEG reader parses carrier")
}

#[test]
fn public_reader_extracts_all_three_trailer_families() {
    let mut jpeg = base_jpeg();
    let xml = b"Media Jukebox\0<MJMD><Tool_Name>Media Center</Tool_Name><Tool_Version>19.0.67</Tool_Version><People>Santa</People><Places>Jamaica</Places><Album>2013-09-01</Album><Name>Glass home at night</Name><Date>41518.8418865740750334</Date></MJMD>";
    push_segment(&mut jpeg, 0xe9, xml);
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    // This is the order in pinned ExifTool.jpg: ProcessVivo consumes its
    // fixed EOF suffix, then ProcessTrailers identifies Samsung at that offset.
    jpeg.extend_from_slice(&vivo_trailer(b"{\"version\":1000}"));
    let metadata = public_metadata(&jpeg);

    assert_eq!(
        metadata.get_string("Samsung:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(
        metadata.get("Samsung:EmbeddedAudioFile"),
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
        ("Vivo:JSONInfo", "{\"version\":1000}"),
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
        metadata.get_string("Samsung:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
    assert_eq!(
        metadata.get_string("Vivo:JSONInfo"),
        Some("{\"version\":1000}")
    );
}

#[test]
fn samsung_internal_marker_without_bounded_vivo_suffix_is_withheld() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer());
    jpeg.extend_from_slice(b"unrelated trailing bytes");
    let metadata = public_metadata(&jpeg);

    assert!(metadata.get("Samsung:EmbeddedAudioFileName").is_none());
    assert!(metadata.get("Samsung:EmbeddedAudioFile").is_none());
}

#[test]
fn non_finite_media_jukebox_date_is_withheld() {
    let mut jpeg = base_jpeg();
    push_segment(
        &mut jpeg,
        0xe9,
        b"Media Jukebox\0<MJMD><Tool_Name>Media Center</Tool_Name><Date>NaN</Date></MJMD>",
    );
    finish_jpeg(&mut jpeg);
    let metadata = public_metadata(&jpeg);

    assert_eq!(metadata.get_string("XML:Tool_Name"), Some("Media Center"));
    assert!(metadata.get("XML:Date").is_none());
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
        "Samsung:EmbeddedAudioFileName",
        "Samsung:EmbeddedAudioFile",
        "XML:Tool_Name",
        "Vivo:JSONInfo",
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

    assert!(metadata.get("Samsung:EmbeddedAudioFileName").is_none());
    assert!(metadata.get("Vivo:JSONInfo").is_none());
}

#[test]
fn non_utf8_samsung_name_uses_pinned_native_public_representation() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_soundshot_trailer_with_name(b"A\xffB"));

    // Pinned ExifTool 13.59 renders this raw name as `A?B` in its public JSON
    // output (while `-b` preserves the original `41 ff 42` bytes).
    assert_eq!(
        public_metadata(&jpeg).get_string("Samsung:EmbeddedAudioFileName"),
        Some("A?B")
    );
}

#[test]
fn samsung_direct_seft_footer_is_bounded_and_publicly_readable() {
    let mut jpeg = base_jpeg();
    finish_jpeg(&mut jpeg);
    jpeg.extend_from_slice(&samsung_direct_seft_trailer());
    assert_eq!(
        public_metadata(&jpeg).get_string("Samsung:EmbeddedAudioFileName"),
        Some("SoundShot_000")
    );
}
