use oxidex::core::FormatParser;
use oxidex::io::BufferedReader;
use oxidex::parsers::audio::mp3::Mp3Parser;

#[test]
fn test_mp3_id3v2_magic() {
    // ID3v2 header
    let data = b"ID3\x04\x00\x00\x00\x00\x00\x00...";
    let reader = BufferedReader::from_bytes(data);
    let parser = Mp3Parser;
    let result = parser.parse(&reader);

    assert!(result.is_ok());
}

#[test]
fn test_mp3_frame_sync() {
    // MP3 frame sync
    let data = b"\xFF\xFB\x90\x00...";
    let reader = BufferedReader::from_bytes(data);
    let parser = Mp3Parser;
    let result = parser.parse(&reader);

    assert!(result.is_ok());
}
