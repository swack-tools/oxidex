//! Sub-directories reached from the Nikon MakerNote IFD.
//!
//! Three of the tags in `Nikon::Main` are pointers rather than values, and each
//! needs its own walk:
//!
//! * 0x0011 `PreviewIFD`     -- a nested TIFF IFD (`Nikon::PreviewIFD`)
//! * 0x0e0e `NikonCaptureOffsets` -- a packed record list (`Nikon::CaptureOffsets`)
//! * 0x0e10 `NikonScanIFD`   -- a nested TIFF IFD (`Nikon::Scan`)
//!
//! ExifTool reports all three under family-0 group `MakerNotes`, so the tags
//! below carry the ordinary `Nikon:` prefix even though ExifTool's family-1
//! groups are `PreviewIFD` and `NikonScan`.

use std::collections::HashMap;

use super::value_reader::{ascii_value, format_number, rational_value, read_u32, value_bytes};
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use crate::parsers::tiff::makernotes::shared::ifd_parser_base::{
    IfdParserConfig, parse_ifd_entries,
};

/// Sub-IFDs here are small; anything larger is corrupt rather than interesting.
const MAX_SUB_IFD_ENTRIES: usize = 64;

/// `%Image::ExifTool::Exif::compression`, restricted to the codes a Nikon
/// preview or scan IFD can carry. Unlisted codes report themselves.
fn compression_name(value: u32) -> String {
    match value {
        1 => "Uncompressed".to_string(),
        2 => "CCITT 1D".to_string(),
        3 => "T4/Group 3 Fax".to_string(),
        4 => "T6/Group 4 Fax".to_string(),
        5 => "LZW".to_string(),
        6 => "JPEG (old-style)".to_string(),
        7 => "JPEG".to_string(),
        8 => "Adobe Deflate".to_string(),
        32773 => "PackBits".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// `Image::ExifTool::Nikon::PreviewIFD` tag 0x128.
fn resolution_unit_name(value: u32) -> String {
    match value {
        1 => "None".to_string(),
        2 => "inches".to_string(),
        3 => "cm".to_string(),
        other => format!("Unknown ({})", other),
    }
}

/// Read an entry that holds a single unsigned integer (BYTE/SHORT/LONG).
fn scalar_u32(entry: &IfdEntry, data: &[u8], tiff_start: usize, order: ByteOrder) -> Option<u32> {
    let bytes = value_bytes(entry, data, tiff_start, order)?;
    match bytes.len() {
        1 => Some(bytes[0] as u32),
        2 => super::value_reader::read_u16(&bytes, 0, order).map(u32::from),
        4 => read_u32(&bytes, 0, order),
        _ => None,
    }
}

/// Walk `Nikon::PreviewIFD` (MakerNote tag 0x0011).
///
/// `PreviewImageStart` is reported only when the absolute file offset of the
/// embedded Nikon TIFF header is known. Emitting the MakerNote-relative value
/// without that base would be a wrong number rather than a missing one.
pub fn parse_preview_ifd(
    data: &[u8],
    tiff_start: usize,
    ifd_offset: usize,
    order: ByteOrder,
    preview_ifd_base: Option<u64>,
    tags: &mut HashMap<String, String>,
) {
    let Some(start) = tiff_start.checked_add(ifd_offset) else {
        return;
    };
    let Some(ifd) = data.get(start..) else {
        return;
    };
    let config = IfdParserConfig {
        signature: None,
        signature_offset: 0,
        max_entries: MAX_SUB_IFD_ENTRIES,
    };
    let _ = parse_ifd_entries(ifd, order, &config, |entry, _| match entry.tag_id {
        0x0103 => {
            if let Some(value) = scalar_u32(entry, data, tiff_start, order) {
                tags.insert("Nikon:Compression".to_string(), compression_name(value));
            }
        }
        0x011a | 0x011b => {
            if let Some(bytes) = value_bytes(entry, data, tiff_start, order)
                && let Some(value) = rational_value(&bytes, 0, order, false)
            {
                let name = if entry.tag_id == 0x011a {
                    "Nikon:XResolution"
                } else {
                    "Nikon:YResolution"
                };
                tags.insert(name.to_string(), format_number(value));
            }
        }
        0x0128 => {
            if let Some(value) = scalar_u32(entry, data, tiff_start, order) {
                tags.insert(
                    "Nikon:ResolutionUnit".to_string(),
                    resolution_unit_name(value),
                );
            }
        }
        0x0201 => {
            if let (Some(value), Some(base)) =
                (scalar_u32(entry, data, tiff_start, order), preview_ifd_base)
                && let Some(value) = base.checked_add(u64::from(value))
            {
                tags.insert("Nikon:PreviewImageStart".to_string(), value.to_string());
            }
        }
        0x0202 => {
            if let Some(value) = scalar_u32(entry, data, tiff_start, order) {
                tags.insert("Nikon:PreviewImageLength".to_string(), value.to_string());
            }
        }
        0x0213 => {
            if let Some(value) = scalar_u32(entry, data, tiff_start, order) {
                let printed = match value {
                    1 => "Centered".to_string(),
                    2 => "Co-sited".to_string(),
                    other => format!("Unknown ({})", other),
                };
                tags.insert("Nikon:YCbCrPositioning".to_string(), printed);
            }
        }
        _ => {}
    });
}

/// Walk `Nikon::Scan` (MakerNote tag 0x0e10, the IFD written by Nikon Scan).
///
/// Note that this table has no `PRINT_CONV`, so its string values keep the
/// trailing spaces the scanner software wrote -- `FilmType` really is
/// `"POSITIVE       "` and not `"POSITIVE"`.
pub fn parse_scan_ifd(
    data: &[u8],
    tiff_start: usize,
    ifd_offset: usize,
    order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let Some(start) = tiff_start.checked_add(ifd_offset) else {
        return;
    };
    let Some(ifd) = data.get(start..) else {
        return;
    };
    let config = IfdParserConfig {
        signature: None,
        signature_offset: 0,
        max_entries: MAX_SUB_IFD_ENTRIES,
    };
    let _ = parse_ifd_entries(ifd, order, &config, |entry, _| {
        let name = match entry.tag_id {
            0x0002 => "FilmType",
            0x0040 => "MultiSample",
            0x0041 => "BitDepth",
            0x0050 => "MasterGain",
            0x0051 => "ColorGain",
            0x0060 => "ScanImageEnhancer",
            0x0100 => "DigitalICE",
            0x0200 => "DigitalDEEShadowAdj",
            0x0201 => "DigitalDEEThreshold",
            0x0202 => "DigitalDEEHighlightAdj",
            _ => return,
        };
        let key = format!("Nikon:{}", name);
        match entry.tag_id {
            // Writable => 'string'
            0x0002 | 0x0040 | 0x0100 => {
                if let Some(bytes) = value_bytes(entry, data, tiff_start, order) {
                    tags.insert(key, ascii_value(&bytes));
                }
            }
            // Writable => 'rational64s', PrintConv => sprintf("%.2f", $val)
            0x0050 => {
                if let Some(bytes) = value_bytes(entry, data, tiff_start, order)
                    && let Some(value) = rational_value(&bytes, 0, order, true)
                {
                    tags.insert(key, format!("{:.2}", value));
                }
            }
            // Count => 3, PrintConv => sprintf("%.2f %.2f %.2f", split(" ",$val))
            0x0051 => {
                if let Some(bytes) = value_bytes(entry, data, tiff_start, order) {
                    let gains: Vec<String> = (0..3)
                        .filter_map(|i| rational_value(&bytes, i, order, true))
                        .map(|v| format!("{:.2}", v))
                        .collect();
                    if gains.len() == 3 {
                        tags.insert(key, gains.join(" "));
                    }
                }
            }
            // PrintConv => \%offOn
            0x0060 => {
                if let Some(value) = scalar_u32(entry, data, tiff_start, order) {
                    let printed = match value {
                        0 => "Off".to_string(),
                        1 => "On".to_string(),
                        other => format!("Unknown ({})", other),
                    };
                    tags.insert(key, printed);
                }
            }
            // Writable => 'int16u' / 'int32u', no PrintConv
            _ => {
                if let Some(value) = scalar_u32(entry, data, tiff_start, order) {
                    tags.insert(key, value.to_string());
                }
            }
        }
    });
}

/// Parse `Nikon::CaptureOffsets` (MakerNote tag 0x0e0e).
///
/// The block is validated by ExifTool with `$val =~ /^0100/` and its directory
/// starts four bytes in. That directory is a `u16` record count followed by
/// 12-byte records; only the first eight bytes of each record are used, holding
/// a 32-bit tag id and a 32-bit value.
pub fn parse_capture_offsets(block: &[u8], order: ByteOrder, tags: &mut HashMap<String, String>) {
    if block.len() < 4 || &block[..4] != b"0100" {
        return;
    }
    let dir = &block[4..];
    let Some(count) = super::value_reader::read_u16(dir, 0, order) else {
        return;
    };
    let count = count as usize;
    if count == 0 || count * 12 + 2 > dir.len() {
        return;
    }
    for index in 0..count {
        let pos = 12 * index + 2;
        let (Some(tag_id), Some(value)) =
            (read_u32(dir, pos, order), read_u32(dir, pos + 4, order))
        else {
            continue;
        };
        let name = match tag_id {
            1 => "IFD0_Offset",
            2 => "PreviewIFD_Offset",
            3 => "SubIFD_Offset",
            _ => continue,
        };
        tags.insert(format!("Nikon:{}", name), value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a MakerNote-shaped buffer: 10 bytes of Nikon header, then a TIFF
    /// header, then the caller's payload starting at `tiff_start + payload_at`.
    fn makernote(payload_at: usize, payload: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 10 + payload_at + payload.len()];
        data[..10].copy_from_slice(b"Nikon\0\x02\x00\x00\x00");
        data[10 + payload_at..].copy_from_slice(payload);
        data
    }

    /// One little-endian IFD entry.
    fn entry(tag: u16, field_type: u16, count: u32, value: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity(12);
        v.extend_from_slice(&tag.to_le_bytes());
        v.extend_from_slice(&field_type.to_le_bytes());
        v.extend_from_slice(&count.to_le_bytes());
        v.extend_from_slice(&value.to_le_bytes());
        v
    }

    #[test]
    fn parses_the_d70_preview_ifd() {
        // PreviewIFD from Nikon.nef: Compression=6, XRes/YRes=72/1,
        // ResolutionUnit=2, PreviewImageLength=26.
        let mut ifd = Vec::new();
        ifd.extend_from_slice(&4u16.to_le_bytes());
        ifd.extend(entry(0x0103, 3, 1, 6));
        ifd.extend(entry(0x011a, 5, 1, 200)); // rational at tiff-relative 200
        ifd.extend(entry(0x0128, 3, 1, 2));
        ifd.extend(entry(0x0202, 4, 1, 26));
        ifd.extend_from_slice(&0u32.to_le_bytes());
        let mut payload = ifd;
        payload.resize(200 - 100, 0); // pad out to tiff-relative offset 200
        payload.extend_from_slice(&72u32.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());

        let data = makernote(100, &payload);
        let mut tags = HashMap::new();
        parse_preview_ifd(&data, 10, 100, ByteOrder::LittleEndian, None, &mut tags);

        assert_eq!(tags.get("Nikon:Compression").unwrap(), "JPEG (old-style)");
        assert_eq!(tags.get("Nikon:XResolution").unwrap(), "72");
        assert_eq!(tags.get("Nikon:ResolutionUnit").unwrap(), "inches");
        assert_eq!(tags.get("Nikon:PreviewImageLength").unwrap(), "26");
        // Deliberately absent: it would need the MakerNote's file offset.
        assert!(!tags.contains_key("Nikon:PreviewImageStart"));
    }

    #[test]
    fn parses_the_d70_scan_ifd() {
        // NikonScan IFD from Nikon.nef.
        let mut ifd = Vec::new();
        ifd.extend_from_slice(&4u16.to_le_bytes());
        ifd.extend(entry(0x0002, 2, 16, 200)); // FilmType at tiff-relative 200
        ifd.extend(entry(0x0041, 3, 1, 8)); // BitDepth
        ifd.extend(entry(0x0060, 4, 1, 0)); // ScanImageEnhancer
        ifd.extend(entry(0x0100, 2, 7, 216)); // DigitalICE
        ifd.extend_from_slice(&0u32.to_le_bytes());
        let mut payload = ifd;
        payload.resize(200 - 100, 0);
        payload.extend_from_slice(b"POSITIVE       \x00");
        payload.extend_from_slice(b"Normal\x00");

        let data = makernote(100, &payload);
        let mut tags = HashMap::new();
        parse_scan_ifd(&data, 10, 100, ByteOrder::LittleEndian, &mut tags);

        // ExifTool does NOT trim these trailing spaces for the NikonScan table.
        assert_eq!(tags.get("Nikon:FilmType").unwrap(), "POSITIVE       ");
        assert_eq!(tags.get("Nikon:BitDepth").unwrap(), "8");
        assert_eq!(tags.get("Nikon:ScanImageEnhancer").unwrap(), "Off");
        assert_eq!(tags.get("Nikon:DigitalICE").unwrap(), "Normal");
    }

    #[test]
    fn parses_the_d70_capture_offsets() {
        // 0x0e0e block from Nikon.nef.
        let mut block = b"0100".to_vec();
        block.extend_from_slice(&3u16.to_le_bytes());
        for (id, value) in [(1u32, 8u32), (2, 11362), (3, 1440)] {
            block.extend_from_slice(&id.to_le_bytes());
            block.extend_from_slice(&value.to_le_bytes());
            block.extend_from_slice(&[0u8; 4]); // remainder of the 12-byte record
        }
        let mut tags = HashMap::new();
        parse_capture_offsets(&block, ByteOrder::LittleEndian, &mut tags);
        assert_eq!(tags.get("Nikon:IFD0_Offset").unwrap(), "8");
        assert_eq!(tags.get("Nikon:PreviewIFD_Offset").unwrap(), "11362");
        assert_eq!(tags.get("Nikon:SubIFD_Offset").unwrap(), "1440");
    }

    #[test]
    fn capture_offsets_requires_the_0100_signature() {
        let mut block = b"0200".to_vec();
        block.extend_from_slice(&1u16.to_le_bytes());
        block.extend_from_slice(&1u32.to_le_bytes());
        block.extend_from_slice(&8u32.to_le_bytes());
        block.extend_from_slice(&[0u8; 4]);
        let mut tags = HashMap::new();
        parse_capture_offsets(&block, ByteOrder::LittleEndian, &mut tags);
        assert!(tags.is_empty());
    }

    #[test]
    fn capture_offsets_rejects_a_count_that_overruns_the_block() {
        let mut block = b"0100".to_vec();
        block.extend_from_slice(&99u16.to_le_bytes());
        let mut tags = HashMap::new();
        parse_capture_offsets(&block, ByteOrder::LittleEndian, &mut tags);
        assert!(tags.is_empty());
    }

    #[test]
    fn out_of_range_sub_ifd_offsets_are_ignored() {
        let data = makernote(0, &[0u8; 4]);
        let mut tags = HashMap::new();
        parse_preview_ifd(&data, 10, 4096, ByteOrder::LittleEndian, None, &mut tags);
        parse_scan_ifd(&data, 10, 4096, ByteOrder::LittleEndian, &mut tags);
        assert!(tags.is_empty());
    }

    #[test]
    fn unknown_enum_codes_report_themselves() {
        assert_eq!(compression_name(9999), "Unknown (9999)");
        assert_eq!(resolution_unit_name(7), "Unknown (7)");
    }
}
