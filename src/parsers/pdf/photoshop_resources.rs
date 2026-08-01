//! Photoshop image-resource extraction from PDF page dictionaries.
//!
//! When Photoshop saves a PDF it parks the image's whole 8BIM resource block
//! in the page dictionary, under
//! `/PieceInfo /AdobePhotoshop /Private /ImageResources`, as an ordinary PDF
//! stream object. ExifTool walks that path in `PDF.pm` (the `PieceInfo`
//! sub-directory chain feeding `Image::ExifTool::Photoshop::Main`) and so
//! reports the file's Photoshop, IPTC and EXIF tags even though the PDF
//! itself carries none of them in its Info dictionary or XMP packet.
//!
//! The resource run holds three payloads oxidex already knows how to decode:
//!
//! | 8BIM id | Payload | Decoder |
//! |---|---|---|
//! | 0x0404 | IPTC IIM records | [`parse_all_iptc_records`] |
//! | 0x0422 | A self-contained TIFF/EXIF block | [`parse_embedded_exif`] |
//! | others | Photoshop resources | [`parse_photoshop_irb`] |
//!
//! Nothing here invents a value: when the `/ImageResources` reference is
//! absent, the object cannot be resolved, or the stream uses a filter this
//! module cannot undo, it returns an empty map rather than a guess.

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::Result;
use crate::parsers::image::embedded::parse_embedded_exif;
use crate::parsers::jpeg::app_segments::photoshop::parse_photoshop_irb;
use crate::parsers::jpeg::iptc_parser::{
    dataset_to_tag_name, decode_iptc_string, parse_all_iptc_records,
};
use crate::parsers::pdf::shared::PdfContext;

/// "Photoshop 3.0\0" preamble that JPEG's APP13 segment carries in front of
/// its 8BIM run. A PDF `/ImageResources` stream is the same run with the
/// preamble stripped, so it is prepended before reusing the APP13 decoder
/// rather than duplicating the resource table here.
const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";

/// 8BIM resource block signature.
const EIGHTBIM: &[u8] = b"8BIM";

/// `Image::ExifTool::Photoshop::Main` 0x0404 - IPTC IIM records.
const RES_IPTC: u16 = 0x0404;

/// `Image::ExifTool::Photoshop::Main` 0x0422 - a complete TIFF/EXIF block
/// ("EXIFInfo"), byte-identical to a JPEG APP1 payload minus its `Exif\0\0`.
const RES_EXIF: u16 = 0x0422;

/// Upper bound on the object slab read while resolving a stream. Photoshop
/// resource blocks are a few kB; this only exists so a corrupt `/Length`
/// cannot ask for an unbounded allocation.
const MAX_STREAM_BYTES: usize = 16 * 1024 * 1024;

/// Extracts the Photoshop image-resource block a Photoshop-authored PDF
/// stores in its page dictionary, and decodes the Photoshop, IPTC and EXIF
/// tags it carries.
///
/// Returns an empty map (not an error) for PDFs that carry no such block,
/// which is the overwhelming majority of them.
pub fn parse_photoshop_image_resources(reader: &dyn FileReader) -> Result<MetadataMap> {
    let mut metadata = MetadataMap::new();

    let Some(resources) = find_image_resources(reader) else {
        return Ok(metadata);
    };

    // Photoshop family tags: hand the run to the APP13 decoder with the
    // preamble it expects, so PDF and JPEG produce identical values for the
    // identical bytes.
    let mut with_signature = Vec::with_capacity(PHOTOSHOP_SIGNATURE.len() + resources.len());
    with_signature.extend_from_slice(PHOTOSHOP_SIGNATURE);
    with_signature.extend_from_slice(&resources);
    if let Ok(photoshop_tags) = parse_photoshop_irb(&with_signature) {
        for (key, value) in photoshop_tags.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }

    // IPTC (0x0404) and EXIF (0x0422) are sub-directories the APP13 decoder
    // deliberately leaves alone, so they are walked here.
    for (id, payload) in image_resource_blocks(&resources) {
        match id {
            RES_IPTC => insert_iptc_tags(payload, &mut metadata),
            RES_EXIF => {
                let mut exif = MetadataMap::new();
                // ExifTool reports the byte order of every TIFF block it
                // processes, including this one (Exif.pm's ExifByteOrder
                // PrintConv). The marker is the block's own first two bytes.
                match payload.get(..2) {
                    Some(b"II") => {
                        exif.insert(
                            "File:ExifByteOrder",
                            TagValue::String("Little-endian (Intel, II)".to_string()),
                        );
                    }
                    Some(b"MM") => {
                        exif.insert(
                            "File:ExifByteOrder",
                            TagValue::String("Big-endian (Motorola, MM)".to_string()),
                        );
                    }
                    _ => {}
                }
                parse_embedded_exif(payload, &mut exif);
                insert_thumbnail_ifd_tags(payload, &mut exif);
                for (key, value) in exif.iter() {
                    // A tag id the generated registry has no name for comes
                    // back as `Group:0xNNNN`. ExifTool reports no such tag
                    // without -u, so emitting one would only add a key that
                    // can never match. Drop them here rather than teach the
                    // shared walk a PDF-specific rule.
                    if is_unnamed_tag_key(key) {
                        continue;
                    }
                    metadata.insert(key.clone(), value.clone());
                }
            }
            _ => {}
        }
    }

    Ok(metadata)
}

/// True for keys like `ExifIFD:0x920D`, i.e. an IFD entry whose tag id the
/// generated registry carries no name for.
fn is_unnamed_tag_key(key: &str) -> bool {
    let name = key.rsplit(':').next().unwrap_or(key);
    name.len() > 2
        && (name.starts_with("0x") || name.starts_with("0X"))
        && name[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Walks a run of 8BIM blocks, yielding `(resource id, payload)`.
///
/// Block layout: `"8BIM"`, id (BE u16), Pascal-string name padded to an even
/// total length, data size (BE u32), data padded to an even length. Stops at
/// the first byte run that is not a well-formed block, so a truncated tail
/// drops its remaining resources instead of misreading them.
fn image_resource_blocks(data: &[u8]) -> Vec<(u16, &[u8])> {
    let mut blocks = Vec::new();
    let mut pos = 0usize;

    while pos + 4 <= data.len() && &data[pos..pos + 4] == EIGHTBIM {
        let Some(id) = read_be_u16(data, pos + 4) else {
            break;
        };

        let name_len_pos = pos + 6;
        let Some(&name_len) = data.get(name_len_pos) else {
            break;
        };
        // The Pascal string is padded so that the length byte plus the name
        // occupies an even number of bytes.
        let name_field = 1 + name_len as usize;
        let name_field = name_field + (name_field % 2);
        let size_pos = name_len_pos + name_field;

        let Some(size) = read_be_u32(data, size_pos) else {
            break;
        };
        let size = size as usize;
        let data_pos = size_pos + 4;
        let Some(payload) = data.get(data_pos..data_pos.saturating_add(size)) else {
            break;
        };

        blocks.push((id, payload));
        pos = data_pos + size + (size % 2);
    }

    blocks
}

/// Decodes the IPTC IIM records in resource 0x0404 into `IPTC:` tags.
///
/// Datasets IPTC.pm marks `List => 1` are accumulated into an array; every
/// other dataset is single-valued. Formatting (record version as an integer,
/// `YYYYMMDD` -> `YYYY:MM:DD` dates, `HHMMSS+HHMM` -> `HH:MM:SS+HH:MM`
/// times) matches `Image::ExifTool::IPTC`, and the numeric PrintConvs
/// (`Urgency` -> "8 (least urgent)") are applied by the exiftool-compat
/// layer from the raw value.
fn insert_iptc_tags(payload: &[u8], metadata: &mut MetadataMap) {
    use crate::core::value_formatter::{format_iptc_date, format_iptc_time, format_iptc_urgency};

    let Ok(records) = parse_all_iptc_records(payload) else {
        return;
    };

    let mut lists: Vec<(String, Vec<TagValue>)> = Vec::new();

    for record in records {
        // Record 1 is the envelope and record 2 the application record;
        // ExifTool reports both under the IPTC family.
        let tag_name = dataset_to_tag_name(record.record_number, record.dataset_number);

        // ApplicationRecordVersion / EnvelopeRecordVersion are binary int16u,
        // not text (IPTC.pm `Format => 'int16u'`).
        if record.dataset_number == 0 {
            if record.data.len() >= 2 {
                let version = u16::from_be_bytes([record.data[0], record.data[1]]);
                metadata.insert(tag_name, TagValue::Integer(version as i64));
            }
            continue;
        }

        let text = decode_iptc_string(&record.data);
        let text = match (record.record_number, record.dataset_number) {
            (1, 70) | (2, 30) | (2, 37) | (2, 47) | (2, 55) | (2, 62) | (2, 70) => {
                format_iptc_date(&text)
            }
            (1, 80) | (2, 35) | (2, 38) | (2, 60) | (2, 63) => format_iptc_time(&text),
            // Urgency PrintConv: 0 => '0 (reserved)', 1 => '1 (most urgent)',
            // 5 => '5 (normal)', 8 => '8 (least urgent)' (IPTC.pm).
            (2, 10) => format_iptc_urgency(&text),
            _ => text,
        };

        // Which datasets repeat is owned by the JPEG IPTC parser, which
        // decodes the identical 0x0404 resource out of an APP13 segment.
        if crate::parsers::jpeg::iptc_parser::is_repeatable_iptc_dataset(
            record.record_number,
            record.dataset_number,
        ) {
            match lists.iter_mut().find(|(name, _)| *name == tag_name) {
                Some((_, values)) => values.push(TagValue::String(text)),
                None => lists.push((tag_name, vec![TagValue::String(text)])),
            }
            continue;
        }

        metadata.insert(tag_name, TagValue::String(text));
    }

    for (tag_name, values) in lists {
        // ExifTool prints a one-element list as a bare scalar.
        let value = if values.len() == 1 {
            values
                .into_iter()
                .next()
                .unwrap_or(TagValue::String(String::new()))
        } else {
            TagValue::Array(values)
        };
        metadata.insert(tag_name, value);
    }
}

/// Walks the thumbnail IFD (IFD1) of the TIFF block in resource 0x0422.
///
/// [`parse_embedded_exif`] covers IFD0, the EXIF sub-IFD and the GPS sub-IFD
/// but stops before IFD0's next-IFD pointer, so `Compression`,
/// `ThumbnailOffset` and `ThumbnailLength` need this second pass. The block
/// is self-contained -- its offsets are relative to its own TIFF header and
/// its position inside the PDF is not part of them -- so the TIFF base added
/// to `ThumbnailOffset` is 0, which is the 842 ExifTool prints for
/// ExifTool's own PDF.pdf.
fn insert_thumbnail_ifd_tags(tiff: &[u8], metadata: &mut MetadataMap) {
    use crate::core::tiff_helpers::parse_ifd1_thumbnail;
    use crate::io::buffered_reader::BufferedReader;
    use crate::io::{ByteOrder as IoByteOrder, EndianReader};
    use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};

    if tiff.len() < 8 {
        return;
    }
    let (byte_order, io_order) = match &tiff[0..2] {
        b"II" => (ByteOrder::LittleEndian, IoByteOrder::Little),
        b"MM" => (ByteOrder::BigEndian, IoByteOrder::Big),
        _ => return,
    };

    let header = EndianReader::new(tiff, io_order);
    // BigTIFF (0x002B) uses 8-byte offsets `parse_ifd` cannot walk.
    if header.u16_at(2).unwrap_or(0) != 0x002A {
        return;
    }
    let Some(ifd0_offset) = header.u32_at(4).map(u64::from) else {
        return;
    };

    let reader = BufferedReader::from_bytes(tiff);
    let Ok(entries) = parse_ifd(&reader, ifd0_offset, byte_order) else {
        return;
    };

    parse_ifd1_thumbnail(&reader, ifd0_offset, entries.len(), byte_order, 0, metadata);
}

/// Locates and decodes the `/ImageResources` stream.
fn find_image_resources(reader: &dyn FileReader) -> Option<Vec<u8>> {
    let file_size = reader.size();
    let scan_len = file_size.min(MAX_STREAM_BYTES as u64) as usize;
    let data = reader.read(0, scan_len).ok()?;

    // `/ImageResources` only ever appears as an indirect reference: the
    // resource block is a stream, and PDF streams cannot be inline values.
    let key_pos = find_bytes(data, b"/ImageResources")?;
    let object_num = parse_indirect_reference(&data[key_pos + b"/ImageResources".len()..])?;

    let context = PdfContext::load(reader).ok()?;
    let offset = context.xref_map.get(&object_num).copied()?;

    read_stream_object(reader, &context, offset)
}

/// Reads the stream payload of the object starting at `offset`, undoing a
/// FlateDecode filter when one is declared.
///
/// Returns `None` for any filter this module cannot undo, so an undecoded
/// payload is never mistaken for resource bytes.
fn read_stream_object(
    reader: &dyn FileReader,
    context: &PdfContext,
    offset: u64,
) -> Option<Vec<u8>> {
    let available = reader.size().saturating_sub(offset) as usize;
    let slab = reader.read(offset, available.min(MAX_STREAM_BYTES)).ok()?;

    let stream_kw = find_bytes(slab, b"stream")?;
    let dict = &slab[..stream_kw];

    // Only the filters this module can undo are accepted. `/FlateDecode` is
    // the only compressed form Photoshop writes here.
    let filter = dict_name_value(dict, b"/Filter");
    if let Some(name) = filter.as_deref()
        && name != "FlateDecode"
        && name != "Fl"
    {
        return None;
    }

    // The PDF spec requires CRLF or LF (never a bare CR) after the keyword,
    // but Photoshop's classic-Mac line endings make a lone CR worth
    // tolerating.
    let mut start = stream_kw + b"stream".len();
    if slab.get(start) == Some(&b'\r') {
        start += 1;
    }
    if slab.get(start) == Some(&b'\n') {
        start += 1;
    }

    // `/Length` is authoritative and often an indirect reference (PDF.pdf
    // writes `/Length 10 0 R`). Searching for `endstream` is the fallback
    // for the malformed files where it is unusable.
    let declared = dict_length(dict, reader, context);
    let end = match declared {
        Some(len) if start.saturating_add(len) <= slab.len() => start + len,
        _ => start + find_bytes(&slab[start..], b"endstream")?,
    };

    let raw = slab.get(start..end)?;

    match filter.as_deref() {
        None => Some(raw.to_vec()),
        Some(_) => {
            use std::io::Read;
            let mut decoder = flate2::read::ZlibDecoder::new(raw);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).ok()?;
            Some(out)
        }
    }
}

/// Resolves a stream dictionary's `/Length`, following an indirect reference
/// when the value is one.
fn dict_length(dict: &[u8], reader: &dyn FileReader, context: &PdfContext) -> Option<usize> {
    let pos = find_bytes(dict, b"/Length")?;
    let after = &dict[pos + b"/Length".len()..];

    if let Some(object_num) = parse_indirect_reference(after) {
        let offset = context.xref_map.get(&object_num).copied()?;
        let available = reader.size().saturating_sub(offset) as usize;
        let slab = reader.read(offset, available.min(64)).ok()?;
        // "10 0 obj\r2798\rendobj" - the value follows the `obj` keyword.
        let body_start = find_bytes(slab, b"obj")? + 3;
        return parse_unsigned(&slab[body_start..]).map(|n| n as usize);
    }

    parse_unsigned(after).map(|n| n as usize)
}

/// Reads a `/Key /Name` value out of a dictionary slab.
fn dict_name_value(dict: &[u8], key: &[u8]) -> Option<String> {
    let pos = find_bytes(dict, key)?;
    let after = &dict[pos + key.len()..];
    let start = after.iter().position(|b| !is_pdf_whitespace(*b))?;
    if after[start] != b'/' {
        return None;
    }
    let name: Vec<u8> = after[start + 1..]
        .iter()
        .copied()
        .take_while(|b| b.is_ascii_alphanumeric())
        .collect();
    if name.is_empty() {
        None
    } else {
        String::from_utf8(name).ok()
    }
}

/// Parses an indirect object reference (`12 0 R`) at the start of `data`,
/// after leading whitespace. Returns the object number.
fn parse_indirect_reference(data: &[u8]) -> Option<u32> {
    let mut pos = data.iter().position(|b| !is_pdf_whitespace(*b))?;

    let num_start = pos;
    while pos < data.len() && data[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos == num_start {
        return None;
    }
    let object_num: u32 = std::str::from_utf8(&data[num_start..pos])
        .ok()?
        .parse()
        .ok()?;

    // generation number
    let gen_gap = pos;
    while pos < data.len() && is_pdf_whitespace(data[pos]) {
        pos += 1;
    }
    if pos == gen_gap {
        return None;
    }
    let gen_start = pos;
    while pos < data.len() && data[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos == gen_start {
        return None;
    }

    // "R" keyword
    while pos < data.len() && is_pdf_whitespace(data[pos]) {
        pos += 1;
    }
    if data.get(pos) != Some(&b'R') {
        return None;
    }

    Some(object_num)
}

/// Parses the first unsigned integer in `data`, skipping leading whitespace.
fn parse_unsigned(data: &[u8]) -> Option<u64> {
    let start = data.iter().position(|b| !is_pdf_whitespace(*b))?;
    let digits: Vec<u8> = data[start..]
        .iter()
        .copied()
        .take_while(|b| b.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

/// PDF white-space characters (PDF 32000-1 table 1).
fn is_pdf_whitespace(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | 0x0C | b'\r' | b' ')
}

/// Byte-level substring search.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn read_be_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_be_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one 8BIM block with an empty resource name.
    fn block(id: u16, payload: &[u8]) -> Vec<u8> {
        let mut out = EIGHTBIM.to_vec();
        out.extend_from_slice(&id.to_be_bytes());
        out.extend_from_slice(&[0u8, 0u8]); // empty Pascal name, padded
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    #[test]
    fn walks_consecutive_resource_blocks() {
        let mut run = block(0x0404, b"iptc");
        run.extend_from_slice(&block(0x0422, b"exif!"));
        run.extend_from_slice(&block(0x040B, b"url"));

        let blocks = image_resource_blocks(&run);
        assert_eq!(
            blocks
                .iter()
                .map(|(id, payload)| (*id, payload.to_vec()))
                .collect::<Vec<_>>(),
            vec![
                (0x0404u16, b"iptc".to_vec()),
                (0x0422, b"exif!".to_vec()),
                (0x040B, b"url".to_vec()),
            ]
        );
    }

    #[test]
    fn stops_at_a_truncated_block() {
        let mut run = block(0x0404, b"iptc");
        run.extend_from_slice(b"8BIM\x04\x22"); // header only, no size
        assert_eq!(image_resource_blocks(&run).len(), 1);
    }

    #[test]
    fn rejects_a_run_that_is_not_8bim() {
        assert!(image_resource_blocks(b"not a resource block").is_empty());
    }

    #[test]
    fn parses_indirect_references() {
        assert_eq!(parse_indirect_reference(b" 10 0 R >>"), Some(10));
        assert_eq!(parse_indirect_reference(b"\r8 0 R"), Some(8));
        // A direct number is not a reference.
        assert_eq!(parse_indirect_reference(b" 2798 >>"), None);
        assert_eq!(parse_indirect_reference(b" 10 0 Q"), None);
    }

    #[test]
    fn reads_direct_and_name_dictionary_values() {
        assert_eq!(parse_unsigned(b"\r2798\rendobj"), Some(2798));
        assert_eq!(
            dict_name_value(b"<</Filter /FlateDecode /Length 12>>", b"/Filter"),
            Some("FlateDecode".to_string())
        );
        assert_eq!(dict_name_value(b"<</Length 12>>", b"/Filter"), None);
    }

    /// A repeated IPTC dataset becomes a list; a single one stays scalar.
    #[test]
    fn aggregates_repeatable_iptc_datasets() {
        // 0x1C 0x02 0x19 (Keywords) with two values, then 0x1C 0x02 0x05
        // (ObjectName) with one.
        let mut iptc = Vec::new();
        for value in [&b"one"[..], &b"two"[..]] {
            iptc.extend_from_slice(&[0x1C, 0x02, 25]);
            iptc.extend_from_slice(&(value.len() as u16).to_be_bytes());
            iptc.extend_from_slice(value);
        }
        iptc.extend_from_slice(&[0x1C, 0x02, 5]);
        iptc.extend_from_slice(&3u16.to_be_bytes());
        iptc.extend_from_slice(b"pic");

        let mut metadata = MetadataMap::new();
        insert_iptc_tags(&iptc, &mut metadata);

        assert_eq!(
            metadata.get("IPTC:Keywords"),
            Some(&TagValue::Array(vec![
                TagValue::String("one".to_string()),
                TagValue::String("two".to_string()),
            ]))
        );
        assert_eq!(
            metadata.get("IPTC:ObjectName"),
            Some(&TagValue::String("pic".to_string()))
        );
    }

    #[test]
    fn application_record_version_is_an_integer() {
        let iptc = [0x1C, 0x02, 0x00, 0x00, 0x02, 0x00, 0x02];
        let mut metadata = MetadataMap::new();
        insert_iptc_tags(&iptc, &mut metadata);
        assert_eq!(
            metadata.get("IPTC:ApplicationRecordVersion"),
            Some(&TagValue::Integer(2))
        );
    }
}
