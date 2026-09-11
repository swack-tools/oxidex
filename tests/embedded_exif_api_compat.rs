//! Public EXIF helper compatibility: these calls compile from outside the crate.
use oxidex::core::MetadataMap;
use oxidex::parsers::image::embedded::{parse_embedded_exif, parse_embedded_exif_at};
use oxidex::parsers::png::chunk_parser::parse_exif_chunk;

fn artist_tiff(big_endian: bool) -> Vec<u8> {
    fn u16_bytes(value: u16, big: bool) -> [u8; 2] {
        if big {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }
    fn u32_bytes(value: u32, big: bool) -> [u8; 4] {
        if big {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }
    let mut tiff = if big_endian {
        b"MM".to_vec()
    } else {
        b"II".to_vec()
    };
    tiff.extend(u16_bytes(42, big_endian));
    tiff.extend(u32_bytes(8, big_endian));
    tiff.extend(u16_bytes(1, big_endian));
    tiff.extend(u16_bytes(0x013b, big_endian)); // Artist
    tiff.extend(u16_bytes(2, big_endian)); // ASCII
    tiff.extend(u32_bytes(4, big_endian));
    tiff.extend_from_slice(b"Art\0");
    tiff.extend(u32_bytes(0, big_endian));
    tiff
}

#[test]
fn original_public_exif_helpers_accept_both_byte_orders() {
    for big_endian in [false, true] {
        let tiff = artist_tiff(big_endian);
        let entries = parse_exif_chunk(&tiff).expect("valid public PNG TIFF reader");
        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].0, entries[0].1, entries[0].2), (0x013b, 2, 4));
        assert_eq!(entries[0].3.as_ref(), b"Art\0");

        let mut original = MetadataMap::new();
        assert!(parse_embedded_exif(&tiff, &mut original));
        assert_eq!(original.get_string("IFD0:Artist"), Some("Art"));
        let mut explicit = MetadataMap::new();
        assert!(parse_embedded_exif_at(&tiff, 0, &mut explicit));
        assert_eq!(original, explicit, "original helper keeps base zero");
    }
}

#[test]
fn original_public_exif_helpers_reject_malformed_headers() {
    let good = artist_tiff(false);
    let mut wrong_order = good.clone();
    wrong_order[..2].copy_from_slice(b"ZZ");
    let mut big_tiff = good.clone();
    big_tiff[2..4].copy_from_slice(&43u16.to_le_bytes());
    let mut invalid_offset = good.clone();
    invalid_offset[4..8].copy_from_slice(&1000u32.to_le_bytes());
    for malformed in [
        vec![],
        good[..7].to_vec(),
        wrong_order,
        big_tiff,
        invalid_offset,
    ] {
        assert!(parse_exif_chunk(&malformed).is_err());
        let mut metadata = MetadataMap::new();
        assert!(!parse_embedded_exif(&malformed, &mut metadata));
        assert!(metadata.is_empty());
    }
}
