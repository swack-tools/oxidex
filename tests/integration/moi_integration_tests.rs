use oxidex::core::operations::read_metadata;
use std::path::Path;

/// ExifTool 13.59 reads the pinned `t/images/MOI.moi` fixture as:
/// `MOIVersion V6`, `DateTimeOriginal 2011:05:15 17:58:48.000`,
/// `Duration 8.16 s`, `AspectRatio 4:3 PAL`, `AudioCodec AC3`,
/// `AudioBitrate 224 kbps`, `VideoBitrate 8.5 Mbps` -- verified against the
/// pinned oracle directly (`exiftool-pinned.sh -a -G1 -s MOI.moi`). This
/// fails if MOI is not routed to `moi.rs`'s `MOI::Main` layout or if any of
/// the hand-implemented conversions (date/time, duration, aspect ratio,
/// bitrates) differ from ExifTool.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn moi_fixture_matches_pinned_oracle() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/MOI.moi",
    ))
    .expect("read pinned MOI fixture");

    assert_eq!(metadata.get_string("MOI:MOIVersion"), Some("V6"));
    assert_eq!(
        metadata.get_string("MOI:DateTimeOriginal"),
        Some("2011:05:15 17:58:48.000")
    );
    assert_eq!(metadata.get_string("MOI:Duration"), Some("8.16 s"));
    assert_eq!(metadata.get_string("MOI:AspectRatio"), Some("4:3 PAL"));
    assert_eq!(metadata.get_string("MOI:AudioCodec"), Some("AC3"));
    assert_eq!(metadata.get_string("MOI:AudioBitrate"), Some("224 kbps"));
    assert_eq!(metadata.get_string("MOI:VideoBitrate"), Some("8.5 Mbps"));
    assert_eq!(metadata.get_string("File:FileType"), Some("MOI"));
}
