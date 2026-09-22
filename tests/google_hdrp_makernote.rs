//! Google HDR+ maker notes (`Google.pm`, pinned 13.59): the v3 protobuf
//! envelope's named fields, the v2 text stream (XMP `GCamera:hdrp_makernote`
//! and the EXIF `MakerNoteGoogle` payload), `GCamera:shot_log_data`, and the
//! Google Depth `Device` container tags.
//!
//! Forward-ported from `origin/main` (ed2982e1, 95d16186, 47037a04,
//! f6d86743, 8a264d7e), where these fixes landed after this branch diverged;
//! docs/reference/main-divergence-2026-09-18.md counted them as matched on
//! main and MISSING on the tip. main's versions returned early when the
//! fixture was absent; here they are `#[ignore]`d instead, so a checkout
//! without the corpus reports them as skipped rather than passed.
//!
//! Expected values are the pinned oracle's (`exiftool -G0:1:4 -a -s -j`).

use oxidex::core::operations::read_metadata;
#[path = "common/fixtures.rs"]
mod fixtures;

fn assert_fields(file: &str, expected: &[(&str, &str)]) {
    let Some(path) = fixtures::pinned_combined_fixture_path(&format!("Google/{file}")) else {
        return;
    };
    let metadata = read_metadata(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
    for (tag, value) in expected {
        assert_eq!(metadata.get_string(tag), Some(*value), "{file} {tag}");
    }
}

fn binary(len: usize) -> String {
    format!("(Binary data {len} bytes, use -b option to extract)")
}

#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Google"]
fn pixel_10_pro_xl_hdrp_v3_named_fields() {
    let image_data = binary(9236);
    assert_fields(
        "GooglePixel10ProXL.jpg",
        &[
            ("MakerNotes:ImageName", "Finished image"),
            ("MakerNotes:ImageData", &image_data),
            ("MakerNotes:ExposureTimeMin", "0.000122550003230572"),
            ("MakerNotes:ExposureTimeMax", "24"),
            ("MakerNotes:ISOMin", "33.0032997131348"),
            ("MakerNotes:ISOMax", "4224.42236328125"),
            ("MakerNotes:MaxAnalogISO", "264.026397705078"),
        ],
    );
}

#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Google"]
fn pixel_6a_hdrp_v2_xmp_stream_and_shot_log_data() {
    let (init, frame, payload) = (binary(453), binary(212), binary(312_694));
    assert_fields(
        "GooglePixel6a.jpg",
        &[
            ("MakerNotes:InitParamsText", &init),
            ("MakerNotes:PayloadFrame00", &frame),
            ("MakerNotes:PayloadFrame10", &frame),
            ("MakerNotes:PayloadMetadataText", &payload),
            ("MakerNotes:MeteringFrameCount", "1"),
            ("MakerNotes:OriginalPayloadFrameCount", "11"),
            (
                "MakerNotes:ProcessingNotes",
                "Neither warping nor relighting is required -> proceeds to ContiZoom.",
            ),
        ],
    );
}

/// `ProcessingNotes` is any heading-shaped line that is neither `Name:` nor a
/// single word (Google.pm:650-653) -- not one fixed sentence.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Google"]
fn pixel_3_hdrp_v2_processing_notes() {
    assert_fields(
        "GooglePixel3.jpg",
        &[(
            "MakerNotes:ProcessingNotes",
            "Face and lens correction are not requested. Skip Rectiface.",
        )],
    );
}

/// The original Pixel writes its HDRP-v2 stream directly in EXIF MakerNote
/// 0x927c (`MakerNoteGoogle`), not in a GCamera XMP property.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Google"]
fn original_pixel_exif_makernote_hdrp_v2() {
    let logging = binary(1754);
    assert_fields(
        "GooglePixel.jpg",
        &[("MakerNotes:LoggingMetadataText", &logging)],
    );
}

/// Pixel 5's EXIF-resident stream has an indented ` Rectiface:` heading.
#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Google"]
fn pixel_5_exif_makernote_rectiface() {
    let rectiface = binary(886);
    assert_fields(
        "GooglePixel5.jpg",
        &[("MakerNotes:RectifaceText", &rectiface)],
    );
}

#[test]
#[ignore = "needs /tmp/oxidex-exiftool-cache/combined-samples/Google"]
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
