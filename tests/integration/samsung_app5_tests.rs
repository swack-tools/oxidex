//! Regression coverage for Samsung's `ssuniqueid\0` JPEG APP5 record.

use oxidex::Metadata;
use std::io::Write;

const UNIQUE_ID_BYTES: [u8; 32] = [
    0x3d, 0x7a, 0x41, 0xf0, 0x8b, 0x2c, 0x19, 0xde, 0x60, 0x94, 0xa5, 0x3f, 0x77, 0x0e, 0xc1, 0x52,
    0xbe, 0x28, 0x6d, 0x03, 0x91, 0xf4, 0x4a, 0xc8, 0x15, 0xe7, 0x5b, 0x9d, 0x20, 0x6f, 0xb3, 0x0a,
];

fn push_segment(jpeg: &mut Vec<u8>, marker: u8, data: &[u8]) {
    let length = u16::try_from(data.len() + 2).expect("segment fits in a JPEG length field");
    jpeg.extend_from_slice(&[0xff, marker]);
    jpeg.extend_from_slice(&length.to_be_bytes());
    jpeg.extend_from_slice(data);
}

fn samsung_unique_id_jpeg_with_payload(unique_id: &[u8]) -> Vec<u8> {
    let mut jpeg = vec![0xff, 0xd8];
    let mut app5 = b"ssuniqueid\0".to_vec();
    app5.extend_from_slice(unique_id);
    push_segment(&mut jpeg, 0xe5, &app5);
    push_segment(
        &mut jpeg,
        0xc0,
        &[0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00],
    );
    push_segment(&mut jpeg, 0xda, &[0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg
}

fn samsung_unique_id_jpeg() -> Vec<u8> {
    samsung_unique_id_jpeg_with_payload(&UNIQUE_ID_BYTES)
}

#[test]
fn samsung_app5_unique_id_is_lowercase_hex() {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(&samsung_unique_id_jpeg())
        .expect("write jpeg");
    file.flush().expect("flush");

    let metadata = Metadata::from_path(file.path()).expect("Samsung JPEG parses");

    assert_eq!(
        metadata.get_string("Samsung:UniqueID"),
        Some("3d7a41f08b2c19de6094a53f770ec152be286d0391f44ac815e75b9d206fb30a"),
    );
}

#[test]
fn samsung_app5_unique_id_preserves_the_complete_payload() {
    // ExifTool's Samsung APP5 handler passes every byte following
    // `ssuniqueid\0` to `unpack("H*", ...)`; it does not require a 32-byte
    // payload.
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(&samsung_unique_id_jpeg_with_payload(&[0xaa, 0x00, 0xf1]))
        .expect("write jpeg");
    file.flush().expect("flush");

    let metadata = Metadata::from_path(file.path()).expect("Samsung JPEG parses");

    assert_eq!(metadata.get_string("Samsung:UniqueID"), Some("aa00f1"));
}
