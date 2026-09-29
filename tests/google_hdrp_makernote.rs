//! Google HDR+ maker notes (`Google.pm`, pinned 13.59): the v3 protobuf
//! envelope's named fields, the v2 text stream (XMP `GCamera:hdrp_makernote`
//! and the EXIF `MakerNoteGoogle` payload), `GCamera:shot_log_data`, and the
//! Google Depth `Device` container tags.
//!
//! Forward-ported from `origin/main` (ed2982e1, 95d16186, 47037a04,
//! f6d86743, 8a264d7e), where these fixes landed after this branch diverged;
//! docs/reference/main-divergence-2026-09-18.md counted them as matched on
//! main and MISSING on the tip. main's versions returned early when the
//! fixture was absent; here they are `#[ignore]`d instead, and an explicitly
//! selected ignored target fails loudly if its named fixture is unavailable.
//!
//! Expected values are the pinned oracle's (`exiftool -G0:1:4 -a -s -j`).

use chrono::{DateTime, SecondsFormat, Utc};
use oxidex::core::operations::read_metadata;
use oxidex::core::tag_occurrence::ValueChannel;
#[path = "common/fixtures.rs"]
mod fixtures;

fn assert_fields(file: &str, expected: &[(&str, &str)]) {
    let path = fixtures::required_combined_fixture_path(&format!("Google/{file}"));
    let metadata = read_metadata(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    for (tag, value) in expected {
        assert_eq!(metadata.get_string(tag), Some(*value), "{file} {tag}");
    }
}

fn binary(len: usize) -> String {
    format!("(Binary data {len} bytes, use -b option to extract)")
}

fn assert_hdrp_groups(file: &str, key: &str) {
    let path = fixtures::required_combined_fixture_path(&format!("Google/{file}"));
    let metadata = read_metadata(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    assert!(metadata.get(key).is_some(), "{file} {key} must be public");
    let rows: Vec<_> = metadata
        .project_occurrences(ValueChannel::PrintConv)
        .filter(|(candidate, _, _)| *candidate == key)
        .collect();
    assert_eq!(rows.len(), 1, "{file} {key} physical rows");
    assert_eq!(rows[0].1.group0.as_ref(), "MakerNotes");
    assert_eq!(rows[0].1.group1.as_ref(), "Google");
}

/// Pixel 10's direct field-12 v3 strings are distinct from the Pro XL's
/// named MakerNote fields and must remain observable after the forward port.
#[test]
#[ignore = "requires pinned 13.59 combined-samples/Google/GooglePixel10.jpg"]
fn pixel_10_hdrp_device_fields() {
    assert_fields(
        "GooglePixel10.jpg",
        &[
            ("Google:DeviceMake", "Google"),
            ("Google:DeviceModel", "Pixel 10"),
            ("Google:DeviceCodename", "frankel"),
            ("Google:DeviceHardwareRevision", "MP1.0"),
            ("Google:HDRPSoftware", "HDR+ 1.0.796157346"),
            (
                "Google:AndroidRelease",
                "google/frankel/frankel:16/BD3A.250721.001.A1/13854429:user/release-keys",
            ),
            ("Google:Application", "com.google.android.GoogleCamera"),
            ("Google:AppVersion", "10.0.081.796157305.28"),
        ],
    );
}

#[test]
#[ignore = "requires pinned 13.59 combined-samples/Google"]
fn pixel_10_pro_xl_hdrp_v3_named_fields() {
    let image_data = binary(9236);
    let file = "GooglePixel10ProXL.jpg";
    assert_fields(
        file,
        &[
            ("Google:ImageName", "Finished image"),
            ("Google:ImageData", &image_data),
            ("Google:ExposureTimeMin", "0.000122550003230572"),
            ("Google:ExposureTimeMax", "24"),
            ("Google:ISOMin", "33.0032997131348"),
            ("Google:ISOMax", "4224.42236328125"),
            ("Google:MaxAnalogISO", "264.026397705078"),
        ],
    );

    let path = fixtures::required_combined_fixture_path(&format!("Google/{file}"));
    let metadata = read_metadata(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    let rendered = metadata
        .get_string("Google:SoftwareDate")
        .expect("Google:SoftwareDate");
    assert_eq!(rendered.as_bytes().get(19..23), Some(b".000".as_slice()));
    let date = DateTime::parse_from_str(rendered, "%Y:%m:%d %H:%M:%S%.3f%:z")
        .expect("Google:SoftwareDate with a numeric UTC offset");
    assert_eq!(
        date.with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        "2025-07-30T03:29:30.000Z"
    );
    assert_hdrp_groups(file, "Google:SoftwareDate");
}

#[test]
#[ignore = "requires pinned 13.59 combined-samples/Google"]
fn pixel_6a_hdrp_v2_xmp_stream_and_shot_log_data() {
    let (init, frame, payload) = (binary(453), binary(212), binary(312_694));
    assert_fields(
        "GooglePixel6a.jpg",
        &[
            ("Google:InitParamsText", &init),
            ("Google:PayloadFrame00", &frame),
            ("Google:PayloadFrame10", &frame),
            ("Google:PayloadMetadataText", &payload),
            ("Google:MeteringFrameCount", "1"),
            ("Google:OriginalPayloadFrameCount", "11"),
            (
                "Google:ProcessingNotes",
                "Neither warping nor relighting is required -> proceeds to ContiZoom.",
            ),
        ],
    );
}

/// `ProcessingNotes` is any heading-shaped line that is neither `Name:` nor a
/// single word (Google.pm:650-653) -- not one fixed sentence.
#[test]
#[ignore = "requires pinned 13.59 combined-samples/Google"]
fn pixel_3_hdrp_v2_processing_notes() {
    assert_fields(
        "GooglePixel3.jpg",
        &[(
            "Google:ProcessingNotes",
            "Face and lens correction are not requested. Skip Rectiface.",
        )],
    );
}

/// The original Pixel writes its HDRP-v2 stream directly in EXIF MakerNote
/// 0x927c (`MakerNoteGoogle`), not in a GCamera XMP property.
#[test]
#[ignore = "requires pinned 13.59 combined-samples/Google"]
fn original_pixel_exif_makernote_hdrp_v2() {
    let logging = binary(1754);
    assert_fields(
        "GooglePixel.jpg",
        &[("Google:LoggingMetadataText", &logging)],
    );
    assert_hdrp_groups("GooglePixel.jpg", "Google:LoggingMetadataText");
}

/// Pixel 5's EXIF-resident stream has an indented ` Rectiface:` heading.
#[test]
#[ignore = "requires pinned 13.59 combined-samples/Google"]
fn pixel_5_exif_makernote_rectiface() {
    let rectiface = binary(886);
    assert_fields("GooglePixel5.jpg", &[("Google:RectifaceText", &rectiface)]);
}

#[test]
#[ignore = "requires pinned 13.59 combined-samples/Google"]
fn pixel_4a_device_container_tags() {
    assert_fields(
        "GooglePixel4a.jpg",
        &[
            (
                "XMP:Cameras",
                "http://ns.google.com/photos/dd/1.0/device/:Camera",
            ),
            (
                "XMP:Profiles",
                "http://ns.google.com/photos/dd/1.0/device/:Profile",
            ),
            ("XMP:ContainerDirectoryMime", "image/jpeg"),
            ("XMP:ContainerDirectoryLength", "0"),
            ("XMP:ContainerDirectoryDataURI", "primary_image"),
        ],
    );
}
