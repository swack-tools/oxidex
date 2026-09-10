use crate::fixtures::pinned_fixture_path;
use oxidex::core::operations::read_metadata;

/// The pinned ExifTool 13.59 fixture is a RealAudio metafile whose sole line
/// is a streaming URL. This exercises the production detector and dispatch
/// path, and fails if either stops treating the line as the Real `URL` tag.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn ram_fixture_reports_url() {
    let Some(path) = pinned_fixture_path("Real.ram") else {
        return;
    };
    let metadata = read_metadata(&path).expect("read pinned RAM fixture");

    assert_eq!(
        metadata
            .get_string("Real:URL")
            .expect("OxiDex missing Real:URL"),
        "rtsp://media.real.com/showcase/service/samples/rob_h_realvideo9_28.rm"
    );
}
