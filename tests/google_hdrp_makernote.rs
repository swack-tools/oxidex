use oxidex::core::operations::read_metadata;
use std::path::Path;

const PIXEL_10: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Google/GooglePixel10.jpg";
const PIXEL_6A: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Google/GooglePixel6a.jpg";

/// The v3 HDR+ XMP envelope is encrypted and gzip-compressed protobuf.  These
/// direct field-12 strings are pinned from ExifTool 13.59's Google table.
#[test]
fn pixel_10_hdrp_device_fields_match_exiftool() {
    if !Path::new(PIXEL_10).is_file() {
        eprintln!("skipping: corpus fixture not present at {PIXEL_10}");
        return;
    }

    let metadata = read_metadata(Path::new(PIXEL_10)).expect("Google Pixel 10 parses");
    assert_eq!(metadata.get_string("Google:DeviceMake"), Some("Google"));
    assert_eq!(metadata.get_string("Google:DeviceModel"), Some("Pixel 10"));
    assert_eq!(
        metadata.get_string("Google:DeviceCodename"),
        Some("frankel")
    );
    assert_eq!(
        metadata.get_string("Google:DeviceHardwareRevision"),
        Some("MP1.0")
    );
    assert_eq!(
        metadata.get_string("Google:HDRPSoftware"),
        Some("HDR+ 1.0.796157346")
    );
    assert_eq!(
        metadata.get_string("Google:AndroidRelease"),
        Some("google/frankel/frankel:16/BD3A.250721.001.A1/13854429:user/release-keys")
    );
    assert_eq!(
        metadata.get_string("Google:Application"),
        Some("com.google.android.GoogleCamera")
    );
    assert_eq!(
        metadata.get_string("Google:AppVersion"),
        Some("10.0.081.796157305.28")
    );
}

/// HDRP v2 is an encrypted/gzipped text stream, rather than the protobuf
/// envelope used by Pixel 10's v3 sample.  Pin the exact fields ExifTool 13.59
/// emits for the real Pixel 6a fixture, including its binary placeholders.
#[test]
fn pixel_6a_hdrp_v2_fields_match_exiftool() {
    if !Path::new(PIXEL_6A).is_file() {
        eprintln!("skipping: corpus fixture not present at {PIXEL_6A}");
        return;
    }

    let metadata = read_metadata(Path::new(PIXEL_6A)).expect("Google Pixel 6a parses");
    assert_eq!(
        metadata.get_string("MakerNotes:InitParamsText"),
        Some("(Binary data 453 bytes, use -b option to extract)")
    );
    assert_eq!(
        metadata.get_string("MakerNotes:PayloadFrame00"),
        Some("(Binary data 212 bytes, use -b option to extract)")
    );
    assert_eq!(
        metadata.get_string("MakerNotes:PayloadFrame10"),
        Some("(Binary data 212 bytes, use -b option to extract)")
    );
    assert_eq!(
        metadata.get_string("MakerNotes:PayloadMetadataText"),
        Some("(Binary data 312694 bytes, use -b option to extract)")
    );
    assert_eq!(
        metadata.get_string("MakerNotes:MeteringFrameCount"),
        Some("1")
    );
    assert_eq!(
        metadata.get_string("MakerNotes:OriginalPayloadFrameCount"),
        Some("11")
    );
    assert_eq!(
        metadata.get_string("MakerNotes:ProcessingNotes"),
        Some("Neither warping nor relighting is required -> proceeds to ContiZoom.")
    );
}
