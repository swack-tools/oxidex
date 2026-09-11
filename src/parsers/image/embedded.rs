//! Shared decoders for metadata blocks embedded inside container image
//! formats (BPG extensions, FLIF chunks, ...).
//!
//! These containers all wrap the *same* three payloads that JPEG carries in
//! its APP segments -- a TIFF/EXIF block, an ICC profile, and an XMP packet
//! -- so they funnel into the same converters the JPEG/TIFF paths use. That
//! matters for parity: the tag names and PrintConv'd values ExifTool prints
//! for `EXIF:Flash` do not change just because the bytes arrived inside a
//! BPG extension instead of an APP1 segment.

use crate::core::tag_conversion::exif_entry_to_tag_value;
use crate::core::tiff_helpers::{
    parse_exif_subifd, parse_gps_subifd, parse_ifd1_other_tags, parse_ifd1_thumbnail,
};
use crate::core::{MetadataMap, TagValue};
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
/// `tiff_base` is ExifTool's `Base` for the block: the number it adds to
/// every stored offset it *reports* (`IsOffset` tags such as
/// `Pentax:PreviewImageStart`, `InteropIFD:OtherImageStart`). It never
/// moves a read -- the block is read relative to its own start either way
/// -- so passing the wrong value cannot mis-parse a tag, only mis-print an
/// offset. Which value is right is the container's call, and ExifTool 13.59
/// decides it per container, not by where the header sits:
///
/// * `0` for a block ExifTool hands to ProcessTIFF with no `Base` -- the
///   PNG `eXIf` chunk (PNG.pm:1190 `Base => 0`), FLIF's inflated `eXif`
///   chunk (FLIF.pm:90-96), a Photoshop 0x0422 resource (Photoshop.pm:254)
///   -- so the printed offsets are block-relative (2416 for the same Pentax
///   block in all three);
/// * the chunk *data* position for a WebP `EXIF` chunk (RIFF.pm:557-577):
///   `Start => 6` for the `Exif\0\0` variant moves `DirStart`, not `Base`
///   (ExifTool.pm DoProcessTIFF `Base => $base`), so both variants print
///   the same number even though the TIFF header sits 6 bytes later in one;
/// * the TIFF header's own file position for a JXL `Exif` box
///   (Jpeg2000.pm:1245 `Base => $base + $dataPos + $subdirStart`, where the
///   Start expression at :474 already includes the offset word) and a HEIF
///   `Exif` item (QuickTime.pm:9464-9483 `Base => $pos + $start`).
///
/// Measured with the pinned oracle on one Pentax block wrapped in each
/// container (scratchpad `fix/base_probe.py`): 2416 / 2416 / 2416 for
/// PNG / PSD / FLIF, 2476 for WebP with or without the introducer (chunk
/// data at 60), 2460 and 2466 for JXL with offset words 0 and 6 (header at
/// 44 / 50), 2633 for HEIC (header at 217).
///
/// Returns `false` when the block has no usable TIFF header, so callers can
/// distinguish "no EXIF here" from "EXIF parsed".
pub fn parse_embedded_exif(tiff_data: &[u8], tiff_base: u64, metadata: &mut MetadataMap) -> bool {
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

        // An Exif::Main RawConv that returns undef creates no tag at all
        // (PanasonicTitle / PanasonicTitle2 when Panasonic's fixed-size field
        // is all NUL, Exif.pm 13.59:3849-3873).
        let Some(tag_value) =
            exif_entry_to_tag_value(bytes, *field_type, *value_count, *tag_id, byte_order)
        else {
            continue;
        };

        let tag_name = lookup_tag_name(*tag_id, "IFD0");
        metadata.insert(tag_name, tag_value);
    }

    if let Some(offset) = exif_ifd_offset {
        // `tiff_data` itself is the enclosing block (ExifTool's `$dataLen`);
        // `tiff_base` only shifts the offsets that get *reported*, see above.
        parse_exif_subifd(
            &reader,
            offset,
            byte_order,
            tiff_base,
            tiff_data.len() as u64,
            metadata,
        );
    }
    if let Some(offset) = gps_ifd_offset {
        parse_gps_subifd(&reader, offset, byte_order, metadata);
    }

    true
}

/// Walks the thumbnail IFD (IFD1) of a self-contained TIFF/EXIF block.
///
/// [`parse_embedded_exif`] covers IFD0, the EXIF sub-IFD and the GPS sub-IFD
/// but stops before IFD0's next-IFD pointer, so `Compression`,
/// `ThumbnailOffset` and `ThumbnailLength` need this second pass, and the
/// rest of the directory (XResolution, YResolution, ResolutionUnit on the
/// Photoshop blocks) follows through [`parse_ifd1_other_tags`] -- ProcessTIFF
/// walks IFD1 as a full Exif::Main directory and ExifTool prints every entry.
/// The block is self-contained -- its offsets are relative to its own TIFF
/// header and its position inside the container is not part of them -- so
/// the TIFF base added to `ThumbnailOffset` is 0, which is the 842 ExifTool
/// prints for ExifTool's own PDF.pdf (Photoshop resource 0x0422) and the 390
/// it prints for Photoshop.psd.
pub fn parse_embedded_thumbnail_ifd(tiff_data: &[u8], metadata: &mut MetadataMap) {
    if tiff_data.len() < 8 {
        return;
    }
    let (byte_order, io_order) = match &tiff_data[0..2] {
        b"II" => (ByteOrder::LittleEndian, IoByteOrder::Little),
        b"MM" => (ByteOrder::BigEndian, IoByteOrder::Big),
        _ => return,
    };

    let header = EndianReader::new(tiff_data, io_order);
    // BigTIFF (0x002B) uses 8-byte offsets `parse_ifd` cannot walk.
    if header.u16_at(2).unwrap_or(0) != 0x002A {
        return;
    }
    let Some(ifd0_offset) = header.u32_at(4).map(u64::from) else {
        return;
    };

    let reader = BufferedReader::from_bytes(tiff_data);
    let Ok(entries) = parse_ifd(&reader, ifd0_offset, byte_order) else {
        return;
    };

    parse_ifd1_thumbnail(&reader, ifd0_offset, entries.len(), byte_order, 0, metadata);
    parse_ifd1_other_tags(&reader, ifd0_offset, entries.len(), byte_order, metadata);
}

/// Parses an embedded ICC profile and inserts its tags under the
/// `ICC_Profile:` family ExifTool reports them in.
pub fn parse_embedded_icc(icc_data: &[u8], metadata: &mut MetadataMap) -> bool {
    match crate::parsers::icc::parse_icc_profile_data(icc_data) {
        Ok(tags) => {
            let found = !tags.is_empty();
            crate::parsers::icc::insert_icc_tags(metadata, tags);
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
                metadata.insert(name, TagValue::new_string(value));
            }
            found
        }
        Err(_) => false,
    }
}

/// Synthetic TIFF blocks shared by the tests of every container that hands
/// its EXIF payload to [`parse_embedded_exif`], so one builder pins the same
/// bytes for PNG, PSD, WebP, JXL, FLIF and HEIF alike.
#[cfg(test)]
pub(crate) mod test_fixtures {
    /// TIFF -> ExifIFD -> Canon MakerNote with an actual 20 mm FocalLength.
    /// Unlike a detached note, its array offsets address the whole TIFF.
    pub(crate) fn canon_lens_tiff(lens: u16, rf: Option<u16>) -> Vec<u8> {
        fn entry(data: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
            data.extend(tag.to_le_bytes());
            data.extend(kind.to_le_bytes());
            data.extend(count.to_le_bytes());
            data.extend(value.to_le_bytes());
        }

        let note_entries = if rf.is_some() { 2u16 } else { 1 };
        let note_header = 2 + u32::from(note_entries) * 12 + 4;
        let note_len = note_header + 46 + if rf.is_some() { 124 } else { 0 };
        let note_offset = 74;
        let mut data = b"II".to_vec();
        data.extend(42u16.to_le_bytes());
        data.extend(8u32.to_le_bytes());
        // IFD0 at 8..38, Make at 38..44, ExifIFD at 44..74.
        data.extend(2u16.to_le_bytes());
        entry(&mut data, 0x010f, 2, 6, 38);
        entry(&mut data, 0x8769, 4, 1, 44);
        data.extend(0u32.to_le_bytes());
        data.extend(b"Canon\0");
        data.extend(2u16.to_le_bytes());
        entry(&mut data, 0x920a, 5, 1, note_offset + note_len);
        entry(&mut data, 0x927c, 7, note_len, note_offset);
        data.extend(0u32.to_le_bytes());
        assert_eq!(data.len(), note_offset as usize);

        data.extend(note_entries.to_le_bytes());
        entry(&mut data, 1, 3, 23, note_offset + note_header);
        if rf.is_some() {
            entry(&mut data, 0x0093, 3, 62, note_offset + note_header + 46);
        }
        data.extend(0u32.to_le_bytes());
        let mut settings = [0u16; 23];
        settings[0] = 46;
        settings[22] = lens;
        for value in settings {
            data.extend(value.to_le_bytes());
        }
        if let Some(rf) = rf {
            let mut info = [0u16; 62];
            info[0] = 124;
            info[61] = rf;
            for value in info {
                data.extend(value.to_le_bytes());
            }
        }
        assert_eq!(data.len(), (note_offset + note_len) as usize);
        data.extend(20u32.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        data
    }

    /// Minimal little-endian TIFF block whose IFD0 carries the given
    /// `(tag, field_type, payload)` entries; payloads longer than four bytes
    /// are stored after the IFD. The entry count is `payload.len()`, so
    /// only byte-sized types (BYTE 1, ASCII 2, SBYTE 6, UNDEFINED 7) are described
    /// correctly.
    pub(crate) fn tiff_with_entries(entries: &[(u16, u16, &[u8])]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&0x002Au16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let ifd_end = 8 + 2 + entries.len() * 12 + 4;
        let mut tail: Vec<u8> = Vec::new();
        for (tag, field_type, payload) in entries {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&field_type.to_le_bytes());
            data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            if payload.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..payload.len()].copy_from_slice(payload);
                data.extend_from_slice(&inline);
            } else {
                data.extend_from_slice(&((ifd_end + tail.len()) as u32).to_le_bytes());
                tail.extend_from_slice(payload);
            }
        }
        data.extend_from_slice(&0u32.to_le_bytes()); // next IFD
        data.extend_from_slice(&tail);
        data
    }

    /// The PanasonicTitle shapes every container test asserts on
    /// (Exif.pm 13.59:3849-3873): a Make with a trailing blank, an Artist,
    /// an all-NUL PanasonicTitle and a NUL-padded PanasonicTitle2.
    pub(crate) fn panasonic_title_block_a() -> Vec<u8> {
        let mut title2 = b"9999:99:99 00:00:00".to_vec();
        title2.resize(128, 0);
        tiff_with_entries(&[
            (0x010F, 2, b"Panasonic \0"),
            (0x013B, 2, b"Ph\0"),
            (0xC6D2, 7, &[0u8; 64]),
            (0xC6D3, 7, &title2),
        ])
    }

    /// An ASCII-typed empty PanasonicTitle and an all-NUL PanasonicTitle2:
    /// the same empty `string` read, both dropped.
    pub(crate) fn panasonic_title_block_b() -> Vec<u8> {
        tiff_with_entries(&[
            (0x013B, 2, b"Ph\0"),
            (0xC6D2, 2, b"\0"),
            (0xC6D3, 7, &[0u8; 128]),
        ])
    }

    /// A little-endian TIFF block whose IFD0 -> ExifIFD -> InteropIFD chain
    /// carries `OtherImageStart`/`OtherImageLength` (0x0201/0x0202), the
    /// offset pair ExifTool reports as stored value + `Base` (Exif.pm
    /// `IsOffset`). Every container test wraps it to pin the base it passes
    /// to [`super::parse_embedded_exif`]; the expected numbers were read off
    /// the pinned oracle for the same bytes in the same containers
    /// (`InteropIFD:OtherImageStart`: 1000 + 20 WebP with or without the
    /// introducer, + 44 / + 50 JXL offset word 0 / 6, + 217 HEIC, + 0 PNG,
    /// PSD and FLIF).
    pub(crate) fn tiff_with_interop_other_image(start: u32, length: u32) -> Vec<u8> {
        fn entry(data: &mut Vec<u8>, tag: u16, value: u32) {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&4u16.to_le_bytes()); // LONG
            data.extend_from_slice(&1u32.to_le_bytes());
            data.extend_from_slice(&value.to_le_bytes());
        }
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&0x002Au16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        // IFD0 at 8..26: ExifOffset -> 26.
        data.extend_from_slice(&1u16.to_le_bytes());
        entry(&mut data, 0x8769, 26);
        data.extend_from_slice(&0u32.to_le_bytes());
        // ExifIFD at 26..44: InteropOffset -> 44.
        data.extend_from_slice(&1u16.to_le_bytes());
        entry(&mut data, 0xA005, 44);
        data.extend_from_slice(&0u32.to_le_bytes());
        // InteropIFD at 44: the offset pair.
        data.extend_from_slice(&2u16.to_le_bytes());
        entry(&mut data, 0x0201, start);
        entry(&mut data, 0x0202, length);
        data.extend_from_slice(&0u32.to_le_bytes());
        data
    }

    /// The stored `OtherImageStart` every container test wraps.
    pub(crate) const OTHER_IMAGE_START: u32 = 1000;

    /// [`tiff_with_interop_other_image`] with the shared start and a 4-byte
    /// length.
    pub(crate) fn interop_offset_block() -> Vec<u8> {
        tiff_with_interop_other_image(OTHER_IMAGE_START, 4)
    }

    /// Asserts the reported `InteropIFD:OtherImageStart` is the stored
    /// value plus `base`, the number the pinned oracle prints for that
    /// container.
    pub(crate) fn assert_other_image_start(metadata: &crate::core::MetadataMap, base: u64) {
        assert_eq!(
            metadata.get_integer("InteropIFD:OtherImageStart"),
            Some(u64::from(OTHER_IMAGE_START) as i64 + base as i64),
            "OtherImageStart must be stored value {OTHER_IMAGE_START} + base {base}; rows: {:?}",
            metadata.keys().collect::<Vec<_>>()
        );
        assert_eq!(metadata.get_integer("InteropIFD:OtherImageLength"), Some(4));
    }

    /// The assertions for [`panasonic_title_block_a`] once it has been
    /// decoded through the core path.
    pub(crate) fn assert_block_a(metadata: &crate::core::MetadataMap) {
        assert_eq!(
            metadata.get("IFD0:PanasonicTitle"),
            None,
            "64 NUL bytes must create no PanasonicTitle, got {:?}",
            metadata.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            metadata.get_string("IFD0:PanasonicTitle2"),
            Some("9999:99:99 00:00:00")
        );
        // Exif.pm 13.59:585 Make RawConv `$val =~ s/\s+$//`.
        assert_eq!(metadata.get_string("IFD0:Make"), Some("Panasonic"));
        assert_eq!(metadata.get_string("IFD0:Artist"), Some("Ph"));
    }

    /// The assertions for [`panasonic_title_block_b`].
    pub(crate) fn assert_block_b(metadata: &crate::core::MetadataMap) {
        assert_eq!(metadata.get("IFD0:PanasonicTitle"), None);
        assert_eq!(metadata.get("IFD0:PanasonicTitle2"), None);
        assert_eq!(metadata.get_string("IFD0:Artist"), Some("Ph"));
    }
}

#[cfg(test)]
mod tests {
    use super::test_fixtures::tiff_with_entries;
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

    /// Exif.pm 13.59:3849-3873: `RawConv => 'length($val) ? $val : undef'`
    /// on PanasonicTitle (0xc6d2) / PanasonicTitle2 (0xc6d3) means an
    /// all-NUL field produces no tag at all -- not "", not a binary
    /// placeholder -- while a NUL-padded value prints exactly.
    #[test]
    fn panasonic_title_rawconv_undef_creates_no_tag() {
        let mut title2 = b"9999:99:99 00:00:00".to_vec();
        title2.resize(128, 0);
        let block = tiff_with_entries(&[
            (0x013B, 2, b"Ph\0"),
            (0xC6D2, 7, &[0u8; 64]),
            (0xC6D3, 7, &title2),
        ]);
        let mut metadata = MetadataMap::new();
        assert!(parse_embedded_exif(&block, 0, &mut metadata));
        assert_eq!(metadata.get_string("IFD0:Artist"), Some("Ph"));
        assert_eq!(
            metadata.get("IFD0:PanasonicTitle"),
            None,
            "64 NUL bytes must create no PanasonicTitle, got {:?}",
            metadata.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            metadata.get_string("IFD0:PanasonicTitle2"),
            Some("9999:99:99 00:00:00")
        );

        // 128 NUL bytes for PanasonicTitle2, and a type-2 (ASCII) empty
        // string for PanasonicTitle: the same empty `string` read, dropped.
        let block = tiff_with_entries(&[
            (0x013B, 2, b"Ph\0"),
            (0xC6D2, 2, b"\0"),
            (0xC6D3, 7, &[0u8; 128]),
        ]);
        let mut metadata = MetadataMap::new();
        assert!(parse_embedded_exif(&block, 0, &mut metadata));
        assert_eq!(metadata.get_string("IFD0:Artist"), Some("Ph"));
        assert_eq!(metadata.get("IFD0:PanasonicTitle"), None);
        assert_eq!(metadata.get("IFD0:PanasonicTitle2"), None);
    }

    #[test]
    fn parses_embedded_tiff_block() {
        let mut metadata = MetadataMap::new();
        assert!(parse_embedded_exif(&tiff_with_artist(), 0, &mut metadata));
        assert!(
            metadata.keys().any(|k| k.ends_with(":Artist")),
            "expected an Artist tag, got {:?}",
            metadata.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn rejects_non_tiff_block() {
        let mut metadata = MetadataMap::new();
        assert!(!parse_embedded_exif(b"not a tiff header", 0, &mut metadata));
        assert!(metadata.is_empty());
    }

    #[test]
    fn rejects_bigtiff_block() {
        // BigTIFF magic (0x002B) uses 8-byte offsets `parse_ifd` cannot walk.
        let mut data = tiff_with_artist();
        data[2] = 0x2B;
        let mut metadata = MetadataMap::new();
        assert!(!parse_embedded_exif(&data, 0, &mut metadata));
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
