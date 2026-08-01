//! Reach PrintIM through a MakerNote IFD's declared `0x0e00` edge.

use crate::io::EndianReader;
use crate::parsers::common::print_im::decode_print_im_version;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::parsers::tiff::makernotes::makernote_context::{
    MakerNoteContext, value_overlaps_directory,
};

/// Find MakerNote tag `0x0e00`, resolve its TIFF-relative value, and decode it.
///
/// `ifd_in_payload` is the directory's byte offset from the MakerNote payload
/// (zero for headerless Nikon, eight for the `RICOH` and `SANYO` wrappers).
/// Out-of-line values are followed only when the enclosing TIFF block is known;
/// a detached MakerNote has no trustworthy base for those offsets.
#[must_use]
pub fn decode_print_im_from_ifd(
    ctx: &MakerNoteContext<'_>,
    ifd_in_payload: usize,
    byte_order: ByteOrder,
) -> Option<String> {
    let ifd_at = ctx.payload_offset().checked_add(ifd_in_payload)?;
    let data = ctx.tiff();
    let reader = EndianReader::new(data, byte_order.to_io_byte_order());
    let count = usize::from(reader.u16_at(ifd_at)?);
    if count == 0 || count > 512 {
        return None;
    }
    let dir_end = ifd_at.checked_add(2 + count.checked_mul(12)? + 4)?;
    if dir_end > data.len() {
        return None;
    }

    for index in 0..count {
        let entry_at = ifd_at.checked_add(2 + index.checked_mul(12)?)?;
        if reader.u16_at(entry_at)? != 0x0E00 {
            continue;
        }
        let field_type = reader.u16_at(entry_at + 2)?;
        let value_count = reader.u32_at(entry_at + 4)?;
        let value_or_offset = reader.u32_at(entry_at + 8)?;
        let value_len = field_size(field_type)?.checked_mul(value_count as usize)?;
        if value_len == 0 {
            return None;
        }

        let value = if value_len <= 4 {
            data.get(entry_at + 8..entry_at + 8 + value_len)?
        } else {
            if !ctx.is_located() {
                return None;
            }
            let value_at = value_or_offset as usize;
            if value_overlaps_directory(value_at, value_len, ifd_at, dir_end) {
                return None;
            }
            data.get(value_at..value_at.checked_add(value_len)?)?
        };
        return decode_print_im_version(value, byte_order);
    }

    None
}

fn field_size(field_type: u16) -> Option<usize> {
    Some(match field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_a_tiff_relative_print_im_value() {
        let mut tiff = vec![0; 96];
        tiff[..2].copy_from_slice(b"II");
        // MakerNote payload begins at 16; its IFD is behind an 8-byte header.
        tiff[24..26].copy_from_slice(&1u16.to_le_bytes());
        tiff[26..28].copy_from_slice(&0x0E00u16.to_le_bytes());
        tiff[28..30].copy_from_slice(&7u16.to_le_bytes());
        tiff[30..34].copy_from_slice(&22u32.to_le_bytes());
        tiff[34..38].copy_from_slice(&64u32.to_le_bytes());
        tiff[64..72].copy_from_slice(b"PrintIM\0");
        tiff[72..76].copy_from_slice(b"0100");
        // reserved bytes + one little-endian entry
        tiff[76..80].copy_from_slice(&[0, 0, 1, 0]);
        let ctx = MakerNoteContext::in_tiff(&tiff, 16, 44, 0);

        assert_eq!(
            decode_print_im_from_ifd(&ctx, 8, ByteOrder::LittleEndian).as_deref(),
            Some("0100")
        );
    }

    #[test]
    fn refuses_an_external_offset_without_a_verified_tiff_base() {
        let mut payload = vec![0; 32];
        payload[..2].copy_from_slice(&1u16.to_le_bytes());
        payload[2..4].copy_from_slice(&0x0E00u16.to_le_bytes());
        payload[4..6].copy_from_slice(&7u16.to_le_bytes());
        payload[6..10].copy_from_slice(&22u32.to_le_bytes());
        payload[10..14].copy_from_slice(&64u32.to_le_bytes());
        let ctx = MakerNoteContext::detached(&payload);

        assert!(decode_print_im_from_ifd(&ctx, 0, ByteOrder::LittleEndian).is_none());
    }
}
