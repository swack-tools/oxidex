//! Shared decoders for metadata blocks embedded inside container image
//! formats (BPG extensions, FLIF chunks, ...).
//!
//! These containers all wrap the *same* three payloads that JPEG carries in
//! its APP segments -- a TIFF/EXIF block, an ICC profile, and an XMP packet
//! -- so they funnel into the same converters the JPEG/TIFF paths use. That
//! matters for parity: the tag names and PrintConv'd values ExifTool prints
//! for `EXIF:Flash` do not change just because the bytes arrived inside a
//! BPG extension instead of an APP1 segment.

use crate::core::MetadataMap;
use crate::core::tag_conversion::{parse_string_to_tag_value, raw_bytes_to_tag_value};
use crate::core::tiff_helpers::{parse_exif_subifd, parse_gps_subifd};
use crate::io::buffered_reader::BufferedReader;
use crate::io::{ByteOrder as IoByteOrder, EndianReader};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::parsers::xmp::rdf_parser::parse_xmp;
use crate::tag_db::lookup_tag_name;

/// EXIF sub-IFD pointer (`ExifOffset`).
const EXIF_IFD_POINTER: u16 = 0x8769;
/// GPS sub-IFD pointer (`GPSInfo`).
const GPS_IFD_POINTER: u16 = 0x8825;

/// Parses a self-contained TIFF/EXIF block and inserts its tags.
///
/// `tiff_data` must start at the TIFF header ("II"/"MM"), i.e. any
/// container-specific preamble such as `Exif\0\0` has already been
/// stripped by the caller. Offsets inside the block are relative to its
/// own start, which is exactly what [`BufferedReader::from_bytes`] gives
/// us.
///
/// Returns `false` when the block has no usable TIFF header, so callers can
/// distinguish "no EXIF here" from "EXIF parsed".
pub fn parse_embedded_exif(tiff_data: &[u8], metadata: &mut MetadataMap) -> bool {
    if tiff_data.len() < 8 {
        return false;
    }

    let byte_order = match &tiff_data[0..2] {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return false,
    };
    let io_order = match byte_order {
        ByteOrder::LittleEndian => IoByteOrder::Little,
        ByteOrder::BigEndian => IoByteOrder::Big,
    };

    let header = EndianReader::new(tiff_data, io_order);
    // 0x002A is plain TIFF. BigTIFF (0x002B) uses 8-byte offsets that
    // `parse_ifd` cannot walk, so it is rejected rather than misread.
    if header.u16_at(2).unwrap_or(0) != 0x002A {
        return false;
    }
    let ifd0_offset = header.u32_at(4).unwrap_or(0) as u64;

    let reader = BufferedReader::from_bytes(tiff_data);
    let Ok(entries) = parse_ifd(&reader, ifd0_offset, byte_order) else {
        return false;
    };

    let mut exif_ifd_offset = None;
    let mut gps_ifd_offset = None;

    for (tag_id, field_type, value_count, raw_bytes) in &entries {
        let bytes = raw_bytes.as_ref();

        // Sub-IFD pointers are structural, not tags ExifTool reports here.
        if *tag_id == EXIF_IFD_POINTER && bytes.len() >= 4 {
            exif_ifd_offset = EndianReader::new(bytes, io_order).u32_at(0).map(u64::from);
            continue;
        }
        if *tag_id == GPS_IFD_POINTER && bytes.len() >= 4 {
            gps_ifd_offset = EndianReader::new(bytes, io_order).u32_at(0).map(u64::from);
            continue;
        }

        let tag_name = lookup_tag_name(*tag_id, "IFD0");
        let tag_value =
            raw_bytes_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order);
        metadata.insert(tag_name, tag_value);
    }

    if let Some(offset) = exif_ifd_offset {
        // The block is self-contained: its offsets are relative to its own
        // start and its absolute file position is unknown here, so the TIFF
        // base added to stored offsets (e.g. OtherImageStart) is 0, as for a
        // standalone TIFF.
        parse_exif_subifd(&reader, offset, byte_order, 0, metadata);
    }
    if let Some(offset) = gps_ifd_offset {
        parse_gps_subifd(&reader, offset, byte_order, metadata);
    }

    true
}

/// Parses an embedded ICC profile and inserts its tags under the
/// `ICC_Profile:` family ExifTool reports them in.
pub fn parse_embedded_icc(icc_data: &[u8], metadata: &mut MetadataMap) -> bool {
    match crate::parsers::icc::parse_icc_profile_data(icc_data) {
        Ok(tags) => {
            let found = !tags.is_empty();
            for (name, value) in tags {
                metadata.insert(format!("ICC_Profile:{}", name), value);
            }
            found
        }
        Err(_) => false,
    }
}

/// Parses an embedded XMP packet and inserts its tags.
///
/// Only the decoded properties are inserted. The raw packet is deliberately
/// not kept as a tag of its own: ExifTool reports no such tag, so emitting
/// one would just be an invented key in every comparison.
pub fn parse_embedded_xmp(xmp_data: &[u8], metadata: &mut MetadataMap) -> bool {
    if std::str::from_utf8(xmp_data).is_err() {
        return false;
    }
    match parse_xmp(xmp_data) {
        Ok(tags) => {
            let found = !tags.is_empty();
            for (name, value) in tags {
                metadata.insert(name, parse_string_to_tag_value(&value));
            }
            found
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal little-endian TIFF block carrying a single ASCII Artist tag.
    fn tiff_with_artist() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&0x002Au16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes()); // entry count
        data.extend_from_slice(&0x013Bu16.to_le_bytes()); // Artist
        data.extend_from_slice(&2u16.to_le_bytes()); // ASCII
        data.extend_from_slice(&4u32.to_le_bytes()); // count
        data.extend_from_slice(b"Ph\0\0");
        data.extend_from_slice(&0u32.to_le_bytes()); // next IFD
        data
    }

    #[test]
    fn parses_embedded_tiff_block() {
        let mut metadata = MetadataMap::new();
        assert!(parse_embedded_exif(&tiff_with_artist(), &mut metadata));
        assert!(
            metadata.keys().any(|k| k.ends_with(":Artist")),
            "expected an Artist tag, got {:?}",
            metadata.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_non_tiff_block() {
        let mut metadata = MetadataMap::new();
        assert!(!parse_embedded_exif(b"not a tiff header", &mut metadata));
        assert!(metadata.is_empty());
    }

    #[test]
    fn rejects_bigtiff_block() {
        // BigTIFF magic (0x002B) uses 8-byte offsets `parse_ifd` cannot walk.
        let mut data = tiff_with_artist();
        data[2] = 0x2B;
        let mut metadata = MetadataMap::new();
        assert!(!parse_embedded_exif(&data, &mut metadata));
    }

    #[test]
    fn parses_embedded_xmp_packet() {
        let xmp = br#"<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/">
   <dc:format>image/bpg</dc:format>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;
        let mut metadata = MetadataMap::new();
        assert!(parse_embedded_xmp(xmp, &mut metadata));
        assert!(
            metadata.keys().any(|k| k.ends_with(":Format")),
            "expected dc:format to decode, got {:?}",
            metadata.keys().collect::<Vec<_>>()
        );
        // The raw packet must not become a tag of its own.
        assert!(!metadata.contains_key("XMP:RawXMP"));
    }
}
