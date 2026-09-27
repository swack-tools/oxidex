//! Public clear-all boundary regressions; fixture provenance is beside the JPEGs.
use oxidex::core::WriteOutcome;
use oxidex::core::operations::clear_all_metadata;
use std::fs;

const SOURCE: &[u8] = include_bytes!("fixtures/jpeg/progressive_clear/inter_scan_comment.jpg");
const IMAGE: &[u8] = include_bytes!("fixtures/jpeg/progressive_clear/image_only.jpg");

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xff, marker];
    bytes.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn check_clear(source: &[u8], expected: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("clear.jpg");
    fs::write(&path, source).unwrap();
    assert_eq!(clear_all_metadata(&path).unwrap(), WriteOutcome::Updated);
    assert_eq!(fs::read(&path).unwrap(), expected, "image bytes changed");
    assert_eq!(clear_all_metadata(&path).unwrap(), WriteOutcome::Unchanged);
    assert_eq!(fs::read(&path).unwrap(), expected);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn clear_preserves_all_progressive_scans_despite_eoi_bytes_in_comment() {
    assert_eq!(IMAGE.windows(2).filter(|w| *w == [0xff, 0xda]).count(), 10);
    check_clear(SOURCE, IMAGE);
}

#[test]
fn clear_removes_ordinary_inter_scan_metadata_and_only_real_trailer() {
    let second = IMAGE
        .windows(2)
        .enumerate()
        .filter(|(_, w)| *w == [0xff, 0xda])
        .nth(1)
        .unwrap()
        .0;
    let mut source = IMAGE[..second].to_vec();
    for marker in 0xe0..=0xef {
        source.extend(segment(marker, b"ordinary metadata \xff\xd9 \xff\xda"));
    }
    source.extend(segment(0xfe, b"ordinary comment"));
    let adobe = segment(0xee, b"Adobe\0\xff\xd9");
    source.extend(&adobe);
    source.extend_from_slice(&IMAGE[second..]);
    source.extend_from_slice(b"post-EOI trailer \xff\xd9");
    let expected = [&IMAGE[..second], adobe.as_slice(), &IMAGE[second..]].concat();
    check_clear(&source, &expected);
}

#[test]
fn clear_preserves_stuffing_restarts_fill_and_multiple_scan_headers() {
    // Framing-only stream exercises byte preservation, not pixel decoding.
    let image = b"\xff\xd8\xff\xff\xda\0\x04\xff\xd9\x12\xff\0\xd9\xff\xd0\xff\xff\xd7\x34\xff\xff\xc4\0\x04\xff\xd9\xff\xda\0\x02\x56\xff\x01\x78\xff\xff\xd9";
    let mut source = image[..2].to_vec();
    source.extend(segment(0xfe, b"remove"));
    source.extend_from_slice(&image[2..]);
    source.extend_from_slice(b"trailer");
    check_clear(&source, image);
}

#[test]
fn malformed_boundaries_refuse_without_replacing_original() {
    let cases: &[&[u8]] = &[
        b"\xff\xd8\xff\xda",                             // missing SOS length
        b"\xff\xd8\xff\xda\0\x01\xff\xd9",               // length below two
        b"\xff\xd8\xff\xda\0\x08\0\xff\xd9",             // truncated SOS header
        b"\xff\xd8\xff\xda\0\x02\x12\xff",               // unfinished entropy marker
        b"\xff\xd8\xff\xda\0\x02\x12",                   // no EOI
        b"\xff\xd8\xff\xda\0\x02\xff\xfe\0\x09\xff\xd9", // truncated inter-scan COM
        b"\xff\xd8\xff\xda\0\x02\xff\xfe\0\x01\xff\xd9",
        b"\xff\xd8\xff\xda\0\x02\xff\xfe\0\x02\x12\xff\xd9", // junk between segments
        b"\xff\xd8\xff\0\xff\xd9",                           // stuffing outside entropy
        b"\xff\xd8\xff\xd8\xff\xd9",                         // nested SOI
    ];
    for bytes in cases {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.jpg");
        fs::write(&path, bytes).unwrap();
        #[cfg(unix)]
        let ino = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&path).unwrap().ino()
        };
        assert!(clear_all_metadata(&path).is_err(), "accepted {bytes:x?}");
        assert_eq!(fs::read(&path).unwrap(), *bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&path).unwrap().ino(), ino);
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
