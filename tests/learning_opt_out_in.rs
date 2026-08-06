use oxidex::core::operations::read_metadata;
use tempfile::NamedTempFile;

/// ExifTool 13.59's Exif.pm 0x9287 PrintConv reads the first `int16u` as the
/// number of usage/choice pairs, then renders each pair in order.
#[test]
fn jpeg_exif_learning_opt_out_in_matches_exiftool_13_59() {
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1, 0x00, 0x3e];
    jpeg.extend_from_slice(b"Exif\0\0");
    jpeg.extend_from_slice(b"II\x2a\0\x08\0\0\0");

    // IFD0: ExifIFD pointer at TIFF offset 26.
    jpeg.extend_from_slice(&[1, 0]);
    jpeg.extend_from_slice(&[0x69, 0x87, 4, 0, 1, 0, 0, 0, 26, 0, 0, 0]);
    jpeg.extend_from_slice(&[0, 0, 0, 0]);

    // ExifIFD: LearningOptOutIn (0x9287), SHORT[5], values at TIFF offset 44.
    jpeg.extend_from_slice(&[1, 0]);
    jpeg.extend_from_slice(&[0x87, 0x92, 3, 0, 5, 0, 0, 0, 44, 0, 0, 0]);
    jpeg.extend_from_slice(&[0, 0, 0, 0]);
    jpeg.extend_from_slice(&[2, 0, 1, 0, 0, 0, 4, 0, 1, 0]);
    jpeg.extend_from_slice(&[0xff, 0xd9]);

    let file = NamedTempFile::new_in("/dev/shm").expect("creates JPEG fixture");
    std::fs::write(file.path(), jpeg).expect("writes JPEG fixture");

    let metadata = read_metadata(file.path()).expect("reads JPEG fixture");
    assert_eq!(
        metadata.get_string("ExifIFD:LearningOptOutIn"),
        Some(
            "Non-Generative AI/ML Training; Opt-out; Input to Foundation Model (Trained AI/ML Model); Opt-in"
        )
    );
}
