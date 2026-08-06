use oxidex::core::operations::read_metadata;
use std::path::Path;

/// The pinned ExifTool 13.59 fixture is a RealAudio metafile whose sole line
/// is a streaming URL. This exercises the production detector and dispatch
/// path, and fails if either stops treating the line as the Real `URL` tag.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn ram_fixture_reports_url() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/Real.ram",
    ))
    .expect("read pinned RAM fixture");

    assert_eq!(
        metadata
            .get_string("Real:URL")
            .expect("OxiDex missing Real:URL"),
        "rtsp://media.real.com/showcase/service/samples/rob_h_realvideo9_28.rm"
    );
}
