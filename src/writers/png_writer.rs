//! PNG chunk writing
//!
//! This module handles writing PNG chunks with metadata modifications.
//!
//! The writer preserves image data (IDAT chunks) unchanged while updating
//! metadata chunks (tEXt, iTXt, eXIf) based on the modified MetadataMap.

use crate::core::FileReader;
use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use crate::parsers::png::chunk_parser::{
    PNG_SIGNATURE, PngChunk, PngTextRecord, parse_chunk, parse_text_record,
};
use crate::parsers::png::parse_png_metadata;
use crate::parsers::png::text_names::{TextRow, TextTagNamer, encode_latin, writable_text_name};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::writers::atomic_writer::write_atomic;
use crate::writers::tiff_writer::serialize_ifd;
use crc::{CRC_32_ISO_HDLC, Crc};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// CRC-32 instance for PNG chunk validation
const PNG_CRC: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

/// iTXt keyword under which PNG stores an XMP packet. `parse_png_metadata`
/// routes this one into the `XMP:` namespace, so no `PNG:` key names it.
const XMP_ITXT_KEYWORD: &str = "XML:com.adobe.xmp";

/// Key for a raw XMP packet to store as a new `XML:com.adobe.xmp` iTXt
/// chunk: `XMP` is that keyword's `%TextualData` Name (PNG.pm:680-681). The
/// reader parses a real packet into `XMP-*` keys, but it *does* surface
/// `PNG:XMP` for a text chunk whose keyword is the unknown `XMP`
/// (PNG.pm:1117-1124), so the key is a packet replacement only when no
/// original text chunk answers it and its value differs from the baseline
/// (see [`plan_text_chunks`]).
const XMP_PACKET_KEY: &str = "PNG:XMP";

/// Calculates CRC-32 checksum for a PNG chunk.
///
/// The CRC is calculated over the chunk type (4 bytes) and chunk data,
/// but NOT the length field.
///
/// # Parameters
///
/// - `chunk_type`: 4-byte chunk type (e.g., b"tEXt", b"IDAT")
/// - `data`: Chunk data bytes
///
/// # Returns
///
/// CRC-32 checksum as u32
fn calculate_crc(chunk_type: &[u8; 4], data: &[u8]) -> u32 {
    let mut digest = PNG_CRC.digest();
    digest.update(chunk_type);
    digest.update(data);
    digest.finalize()
}

/// Writes a PNG chunk to the output buffer.
///
/// PNG chunk format:
/// - Length: 4 bytes (big-endian u32) - length of data field only
/// - Type: 4 bytes (ASCII)
/// - Data: N bytes
/// - CRC: 4 bytes (big-endian u32) - CRC-32 of type + data
///
/// # Parameters
///
/// - `output`: Output buffer to write to
/// - `chunk_type`: 4-byte chunk type
/// - `data`: Chunk data
fn write_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    // Write length (big-endian)
    let length = data.len() as u32;
    output.extend_from_slice(&length.to_be_bytes());

    // Write type
    output.extend_from_slice(chunk_type);

    // Write data
    output.extend_from_slice(data);

    // Calculate and write CRC
    let crc = calculate_crc(chunk_type, data);
    output.extend_from_slice(&crc.to_be_bytes());
}

/// Serializes a tEXt chunk from keyword and text.
///
/// tEXt chunk format: `keyword\0text`
/// - Keyword: Latin-1 string (1-79 bytes)
/// - Null separator: 1 byte
/// - Text: Latin-1 string
///
/// # Parameters
///
/// - `keyword`: Text tag keyword (e.g., "Author", "Title")
/// - `text`: Text value
///
/// # Returns
///
/// Serialized chunk data (without length, type, or CRC)
fn serialize_text_chunk(keyword: &[u8], text: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(keyword);
    data.push(0); // Null separator
    data.extend_from_slice(text);
    data
}

/// Serializes a zTXt chunk (compressed textual data) from keyword and text.
///
/// zTXt chunk format: `keyword\0compression_method<zlib-deflated text>`
/// - Keyword: Latin-1 string (1-79 bytes)
/// - Compression method: 1 byte (0 = zlib/deflate)
fn serialize_ztxt_chunk(keyword: &[u8], text: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    let mut data = Vec::new();
    data.extend_from_slice(keyword);
    data.push(0); // Null separator
    data.push(0); // Compression method = 0 (deflate)

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing to an in-memory encoder is infallible in practice.
    let compressed = encoder.write_all(text).and_then(|_| encoder.finish());
    match compressed {
        Ok(bytes) => data.extend_from_slice(&bytes),
        // Fall back to storing the value uncompressed rather than losing it.
        Err(_) => return serialize_text_chunk(keyword, text),
    }
    data
}

/// Serializes an iTXt chunk from keyword and text.
///
/// iTXt chunk format: `keyword\0compression_flag\0compression_method\0language\0translated_keyword\0text`
/// - Keyword: Latin-1 string (1-79 bytes)
/// - Compression flag: 1 byte (0 = uncompressed, 1 = compressed)
/// - Compression method: 1 byte (0 = zlib, only if compressed)
/// - Language tag: UTF-8 string (can be empty)
/// - Translated keyword: UTF-8 string (can be empty)
/// - Text: UTF-8 string
///
/// This implementation creates uncompressed iTXt chunks only.
///
/// # Parameters
///
/// - `keyword`: Text tag keyword (e.g., "Title", "Description")
/// - `lang`: Language tag (empty for none)
/// - `translated`: Translated keyword (usually empty)
/// - `text`: UTF-8 text value
///
/// # Returns
///
/// Serialized chunk data (without length, type, or CRC)
fn serialize_itxt_chunk(keyword: &[u8], lang: &[u8], translated: &[u8], text: &str) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(keyword);
    data.push(0); // Null separator
    data.push(0); // Compression flag = 0 (uncompressed)
    data.push(0); // Compression method = 0
    data.extend_from_slice(lang); // Language tag
    data.push(0); // Null separator
    data.extend_from_slice(translated); // Translated keyword
    data.push(0); // Null separator
    data.extend_from_slice(text.as_bytes()); // UTF-8 text
    data
}

/// Serializes EXIF metadata to eXIf chunk data.
///
/// The eXIf chunk contains raw TIFF-formatted EXIF data, starting with
/// the byte order marker ("II" for little-endian or "MM" for big-endian).
///
/// # Parameters
///
/// - `metadata`: MetadataMap containing EXIF tags
///
/// # Returns
///
/// Serialized eXIf chunk data (TIFF format), or error if serialization fails
fn serialize_exif_chunk(metadata: &MetadataMap) -> Result<Vec<u8>> {
    // Filter only TIFF-writable EXIF tags
    let mut exif_metadata = MetadataMap::new();
    for (tag_name, tag_value) in metadata.iter() {
        // Accept all TIFF-compatible prefixes
        let is_tiff_writable = tag_name.starts_with("IFD0:")
            || tag_name.starts_with("IFD1:")
            || tag_name.starts_with("ExifIFD:")
            || tag_name.starts_with("GPS:")
            || tag_name.starts_with("EXIF:")
            || tag_name.starts_with("InteropIFD:")
            || tag_name.starts_with("MakerNotes:");

        if is_tiff_writable {
            exif_metadata.insert(tag_name, tag_value.clone());
        }
    }

    // If no EXIF tags, return empty (no eXIf chunk needed)
    if exif_metadata.is_empty() {
        return Ok(Vec::new());
    }

    // Build complete TIFF structure with header
    let mut result = Vec::new();
    let byte_order = ByteOrder::LittleEndian;

    // Write TIFF header (8 bytes)
    // "II" - Intel byte order (little-endian)
    result.extend_from_slice(&[0x49, 0x49]);
    // Magic number 42 (little-endian)
    result.extend_from_slice(&[0x2A, 0x00]);
    // First IFD offset: 8 (little-endian) - starts right after header
    result.extend_from_slice(&[0x08, 0x00, 0x00, 0x00]);

    // Serialize IFD starting at offset 8
    let ifd_bytes = serialize_ifd(&exif_metadata, byte_order, 8)?;
    result.extend_from_slice(&ifd_bytes);

    Ok(result)
}

/// What the writer does with one original tEXt / zTXt / iTXt chunk.
#[derive(Debug)]
enum TextFate {
    /// Copy the original bytes unchanged.
    Carry,
    /// Drop the chunk.
    Drop,
    /// Replace the chunk, in place, with this type and data.
    Rebuild([u8; 4], Vec<u8>),
}

/// Whether two map values are the same text: equal, or the same string
/// under different `TagValue` wrappers.
fn same_value(a: &TagValue, b: &TagValue) -> bool {
    a == b || a.as_string().is_some_and(|s| Some(s) == b.as_string())
}

/// Decides every original text chunk's fate. The writer's source for a
/// chunk's shape (type, keyword bytes, language, translated keyword) is the
/// original file's own chunk, re-parsed here -- never the public key, which
/// is ExifTool's tag name and cannot be inverted (`exif:Make` and
/// `exif-Make` both print as `ExifMake`).
///
/// Each chunk is named exactly as the reader names it (same
/// [`TextTagNamer`], same file order), which gives the `PNG:<Name>` key the
/// map holds it under. Whether the caller *changed* that key is decided
/// against `baseline` -- the map the reader produced for the original file,
/// which is what the caller's map was derived from -- not against the text
/// chunk's own value: a text keyword can share its `PNG:` name with a
/// non-text chunk that the reader files later (a tEXt `ModifyDate` and a
/// tIME, a tEXt `Gamma` and a gAMA), and then the map holds the other
/// chunk's value. Then:
///
/// - key absent from the map: the caller removed it -- drop;
/// - key present with its baseline value: unchanged -- carry the original
///   bytes (every duplicate of the name, too);
/// - key changed, but its baseline value is not this text's (a non-text
///   chunk owns the visible value, so the caller cannot have been editing
///   this text): carry;
/// - key changed and the text owns it: rebuild the chunk that surfaced the
///   value (the last one with that name) in place and drop earlier
///   duplicates; a non-string value cannot be text and is ignored (carry);
/// - a `(Binary data ...)` placeholder row is never text to write back, so
///   while its key is present the chunk is carried;
/// - chunks the reader surfaces under no `PNG:` key (an XMP packet, a known
///   `Raw profile type`, an undecodable chunk) are carried -- except an XMP
///   packet when the caller supplies a replacement under `PNG:XMP` that no
///   original text chunk answers and that differs from the baseline.
///
/// Returns the fate of each text chunk by index, and the set of `PNG:` keys
/// the original chunks answer (so they are not also written as new chunks).
fn plan_text_chunks(
    chunks: &[PngChunk],
    metadata: &MetadataMap,
    baseline: &MetadataMap,
) -> (HashMap<usize, TextFate>, HashSet<String>) {
    struct Surfaced {
        index: usize,
        record: PngTextRecord,
        key: String,
        value: TagValue,
        binary: bool,
    }
    let mut fates = HashMap::new();
    let mut surfaced = Vec::new();
    let mut xmp_chunks = Vec::new();
    let mut last_of: HashMap<String, usize> = HashMap::new();
    let mut namer = TextTagNamer::new();
    for (index, chunk) in chunks.iter().enumerate() {
        if !matches!(&chunk.chunk_type, b"tEXt" | b"zTXt" | b"iTXt") {
            continue;
        }
        let Some(record) = parse_text_record(&chunk.chunk_type, &chunk.data) else {
            fates.insert(index, TextFate::Carry);
            continue;
        };
        match namer.row(&record) {
            TextRow::Tag {
                name,
                value,
                binary,
            } => {
                let key = format!("PNG:{name}");
                last_of.insert(key.clone(), surfaced.len());
                surfaced.push(Surfaced {
                    index,
                    record,
                    key,
                    value,
                    binary,
                });
            }
            TextRow::Xmp(_) => xmp_chunks.push(index),
            TextRow::Omit => {
                fates.insert(index, TextFate::Carry);
            }
        }
    }
    let mut answered = HashSet::new();
    for (position, row) in surfaced.iter().enumerate() {
        let last = &surfaced[last_of[&row.key]];
        let before = baseline.get(&row.key);
        let fate = match metadata.get(&row.key) {
            None => TextFate::Drop,
            Some(_) if row.binary => TextFate::Carry,
            Some(value) if before.is_some_and(|b| same_value(value, b)) => TextFate::Carry,
            Some(value) if same_value(value, &last.value) => TextFate::Carry,
            // A non-text chunk owns the visible value of this key.
            Some(_) if before.is_some_and(|b| !same_value(b, &last.value)) => TextFate::Carry,
            Some(value) if last_of[&row.key] == position => match value.as_string() {
                Some(text) => {
                    let (chunk_type, data) = rebuild_text_chunk(&row.record, text);
                    TextFate::Rebuild(chunk_type, data)
                }
                None => TextFate::Carry,
            },
            Some(value) if value.as_string().is_none() => TextFate::Carry,
            Some(_) => TextFate::Drop,
        };
        fates.insert(row.index, fate);
        answered.insert(row.key.clone());
    }
    let replace_xmp = !answered.contains(XMP_PACKET_KEY)
        && metadata.get(XMP_PACKET_KEY).is_some_and(|v| {
            v.as_string().is_some()
                && !baseline
                    .get(XMP_PACKET_KEY)
                    .is_some_and(|b| same_value(v, b))
        });
    for index in xmp_chunks {
        let fate = if replace_xmp {
            TextFate::Drop
        } else {
            TextFate::Carry
        };
        fates.insert(index, fate);
    }
    (fates, answered)
}

/// Rebuilds an original text chunk around a new value, keeping its type,
/// keyword bytes, language tag and translated keyword. tEXt and zTXt hold
/// `Latin` (cp1252) text, the charset the reader decodes them with; a value
/// with no cp1252 encoding moves to iTXt, as ExifTool's `BuildTextChunk`
/// does for special characters (WritePNG.pl:199-200).
fn rebuild_text_chunk(record: &PngTextRecord, text: &str) -> ([u8; 4], Vec<u8>) {
    if record.chunk_type == *b"iTXt" {
        let lang = record.lang.as_deref().unwrap_or_default();
        let translated = record.translated.as_deref().unwrap_or_default();
        return (
            *b"iTXt",
            serialize_itxt_chunk(&record.keyword, lang, translated, text),
        );
    }
    match encode_latin(text) {
        Some(latin) if record.chunk_type == *b"zTXt" => {
            (*b"zTXt", serialize_ztxt_chunk(&record.keyword, &latin))
        }
        Some(latin) => (*b"tEXt", serialize_text_chunk(&record.keyword, &latin)),
        None => (
            *b"iTXt",
            serialize_itxt_chunk(&record.keyword, b"", b"", text),
        ),
    }
}

/// A new text chunk for a caller-authored `PNG:<name>` key that no original
/// chunk answers. Only `%TextualData` text names (with an optional
/// `-<lang>` suffix) are writable, as in ExifTool, plus `PNG:XMP` for a raw
/// XMP packet. The chunk type follows `BuildTextChunk` (WritePNG.pl:182-241,
/// without the `Compress` option): XMP as uncompressed iTXt with no
/// encoding; a language code, or any non-ASCII character, as iTXt; anything
/// else as tEXt.
fn build_new_text_chunk(name: &str, text: &str) -> Option<([u8; 4], Vec<u8>)> {
    if name == "XMP" {
        return Some((
            *b"iTXt",
            serialize_itxt_chunk(XMP_ITXT_KEYWORD.as_bytes(), b"", b"", text),
        ));
    }
    let (keyword, lang) = writable_text_name(name)?;
    if lang.is_some() || !text.is_ascii() {
        let lang = lang.unwrap_or_default().as_bytes();
        return Some((
            *b"iTXt",
            serialize_itxt_chunk(keyword.as_bytes(), lang, b"", text),
        ));
    }
    Some((
        *b"tEXt",
        serialize_text_chunk(keyword.as_bytes(), text.as_bytes()),
    ))
}

/// Writes modified metadata to a PNG file.
///
/// Which keys the caller changed is judged against the reader's own map of
/// the original file (`parse_png_metadata`); callers that already hold the
/// map theirs was derived from use [`write_png_metadata_with_baseline`].
///
/// This function:
/// 1. Parses existing PNG chunk structure from the original file
/// 2. Decides each original text chunk's fate from the `PNG:<Name>` key the
///    reader surfaces it under (see [`plan_text_chunks`]) and builds new
///    chunks for caller-authored text keys and eXIf from modified_metadata
/// 3. Preserves non-metadata chunks (IHDR, IDAT, etc.) unchanged
/// 4. Reassembles PNG with updated metadata
/// 5. Writes atomically to prevent corruption
///
/// # Parameters
///
/// - `path`: Output file path
/// - `original_reader`: File reader for the original PNG file
/// - `modified_metadata`: Modified metadata to write
///
/// # Returns
///
/// - `Ok(())` on success
/// - `Err` if file is not valid PNG, parsing fails, or write fails
///
/// # Example
///
/// ```no_run
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
/// use oxidex::io::buffered_reader::BufferedReader;
/// use oxidex::writers::png_writer::write_png_metadata;
/// use std::path::Path;
///
/// let path = Path::new("image.png");
/// let reader = BufferedReader::new(path)?;
/// let mut metadata = MetadataMap::new();
/// metadata.insert("PNG:Author", TagValue::new_string("John Doe"));
/// write_png_metadata(path, &reader, &metadata)?;
/// # Ok::<(), oxidex::error::ExifToolError>(())
/// ```
pub fn write_png_metadata(
    path: &Path,
    original_reader: &dyn FileReader,
    modified_metadata: &MetadataMap,
) -> Result<()> {
    let baseline = parse_png_metadata(original_reader).unwrap_or_default();
    write_png_metadata_with_baseline(path, original_reader, modified_metadata, &baseline)
}

/// [`write_png_metadata`] with the caller's baseline: the map read from the
/// original file that `modified_metadata` was derived from. A `PNG:` key
/// whose value still equals its baseline value is unchanged, and its
/// original chunks are carried byte-for-byte (see [`plan_text_chunks`]).
pub fn write_png_metadata_with_baseline(
    path: &Path,
    original_reader: &dyn FileReader,
    modified_metadata: &MetadataMap,
    baseline: &MetadataMap,
) -> Result<()> {
    // Verify PNG signature
    if original_reader.size() < 8 {
        return Err(ExifToolError::parse_error("File too small to be valid PNG"));
    }

    let signature_bytes = original_reader.read(0, 8)?;
    if signature_bytes != PNG_SIGNATURE {
        return Err(ExifToolError::parse_error("Invalid PNG signature"));
    }

    // Parse all existing chunks
    let mut chunks = Vec::new();
    let mut offset = 8; // Start after signature

    while offset < original_reader.size() {
        let (next_offset, chunk) = parse_chunk(original_reader, offset)?;
        let is_iend = chunk.chunk_type == *b"IEND";
        chunks.push(chunk);
        if is_iend {
            break;
        }
        offset = next_offset;
    }

    if chunks.is_empty() {
        return Err(ExifToolError::parse_error("No PNG chunks found"));
    }

    // Decide the original text chunks' fates (see `plan_text_chunks`).
    let (mut text_fates, answered) = plan_text_chunks(&chunks, modified_metadata, baseline);

    // Categorize chunks
    let mut ihdr_chunk: Option<&PngChunk> = None;
    let mut idat_chunks = Vec::new();
    let mut iend_chunk: Option<&PngChunk> = None;
    let mut other_chunks: Vec<([u8; 4], Cow<'_, [u8]>)> = Vec::new();

    for (index, chunk) in chunks.iter().enumerate() {
        match &chunk.chunk_type {
            b"IHDR" => ihdr_chunk = Some(chunk),
            b"IDAT" => idat_chunks.push(chunk),
            b"IEND" => iend_chunk = Some(chunk),
            b"tEXt" | b"iTXt" | b"zTXt" => match text_fates.remove(&index) {
                Some(TextFate::Rebuild(chunk_type, data)) => {
                    other_chunks.push((chunk_type, Cow::Owned(data)));
                }
                Some(TextFate::Drop) => {}
                Some(TextFate::Carry) | None => {
                    other_chunks.push((chunk.chunk_type, Cow::Borrowed(&chunk.data)));
                }
            },
            b"eXIf" => {
                // Skip old metadata chunk - it'll be replaced
            }
            _ => {
                // Preserve other chunks (PLTE, tRNS, etc.)
                other_chunks.push((chunk.chunk_type, Cow::Borrowed(&chunk.data)));
            }
        }
    }

    // Verify critical chunks exist
    let ihdr = ihdr_chunk.ok_or_else(|| ExifToolError::parse_error("Missing IHDR chunk"))?;
    let iend = iend_chunk.ok_or_else(|| ExifToolError::parse_error("Missing IEND chunk"))?;

    if idat_chunks.is_empty() {
        return Err(ExifToolError::parse_error("Missing IDAT chunks"));
    }

    // Build new metadata chunks from modified_metadata
    let mut metadata_chunks: Vec<([u8; 4], Vec<u8>)> = Vec::new();

    // New text chunks for caller-authored `PNG:<Name>` keys that no original
    // chunk answers (those were rebuilt in place above).
    for (tag_name, tag_value) in modified_metadata.iter() {
        let Some(name) = tag_name.strip_prefix("PNG:") else {
            continue;
        };
        if answered.contains(tag_name.as_str()) {
            continue;
        }
        // Unchanged from what the reader surfaced: whatever chunk produced
        // it is still in the file (a non-text chunk, or nothing writable).
        if baseline
            .get(tag_name)
            .is_some_and(|b| same_value(tag_value, b))
        {
            continue;
        }
        if let Some(text) = tag_value.as_string()
            && let Some(chunk) = build_new_text_chunk(name, text)
        {
            metadata_chunks.push(chunk);
        }
    }

    // Process eXIf chunk
    let exif_data = serialize_exif_chunk(modified_metadata)?;
    if !exif_data.is_empty() {
        metadata_chunks.push((*b"eXIf", exif_data));
    }

    // Reassemble PNG file
    let mut output = Vec::new();

    // Write PNG signature
    output.extend_from_slice(&PNG_SIGNATURE);

    // Write IHDR (must be first)
    write_chunk(&mut output, &ihdr.chunk_type, &ihdr.data);

    // Write metadata chunks (before IDAT for better compatibility)
    for (chunk_type, data) in metadata_chunks {
        write_chunk(&mut output, &chunk_type, &data);
    }

    // Write other chunks (PLTE, tRNS, carried or rebuilt text, etc.)
    for (chunk_type, data) in other_chunks {
        write_chunk(&mut output, &chunk_type, &data);
    }

    // Write IDAT chunks (preserve image data unchanged)
    for chunk in idat_chunks {
        write_chunk(&mut output, &chunk.chunk_type, &chunk.data);
    }

    // Write IEND (must be last)
    write_chunk(&mut output, &iend.chunk_type, &iend.data);

    // Write atomically to prevent corruption
    write_atomic(path, &output)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn png_with(text: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        write_chunk(&mut out, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]);
        for (chunk_type, data) in text {
            write_chunk(&mut out, chunk_type, data);
        }
        write_chunk(
            &mut out,
            b"IDAT",
            &[0x78, 0x9C, 0x62, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01],
        );
        write_chunk(&mut out, b"IEND", &[]);
        out
    }

    /// Every tEXt / zTXt / iTXt chunk of a PNG, in order.
    fn text_chunks_of(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 8;
        while at + 8 <= png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let chunk_type: [u8; 4] = png[at + 4..at + 8].try_into().unwrap();
            if matches!(&chunk_type, b"tEXt" | b"zTXt" | b"iTXt") {
                out.push((chunk_type, png[at + 8..at + 8 + len].to_vec()));
            }
            at += 12 + len;
        }
        out
    }

    fn write_and_read(original: Vec<u8>, map: &MetadataMap) -> (Vec<u8>, MetadataMap) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.png");
        write_png_metadata(&path, &TestReader::new(original), map).unwrap();
        let written = std::fs::read(&path).unwrap();
        let reread = parse_png_metadata(&TestReader::new(written.clone())).unwrap();
        (written, reread)
    }

    #[test]
    fn round_trip_rebuilds_from_each_chunks_own_keyword_type_and_language() {
        let ztxt = serialize_ztxt_chunk(b"Software", b"zsw");
        let raw_profile = b"Raw profile type foo\0\nfoo\n  3\n616263\n".to_vec();
        let original = png_with(&[
            (*b"tEXt", b"exif:Make\0PNG Cam".to_vec()),
            (
                *b"iTXt",
                serialize_itxt_chunk(b"Comment", b"fr", b"Kommentar", "bonjour"),
            ),
            (*b"zTXt", ztxt.clone()),
            (*b"tEXt", raw_profile.clone()),
            (*b"tEXt", b"Title\0drop me".to_vec()),
        ]);
        let mut map = parse_png_metadata(&TestReader::new(original.clone())).unwrap();
        assert_eq!(map.get_string("PNG:ExifMake"), Some("PNG Cam"));
        assert_eq!(map.get_string("PNG:Comment-fr"), Some("bonjour"));
        assert_eq!(map.get_string("PNG:Software"), Some("zsw"));
        assert!(map.contains_key("PNG:RawProfileTypeFoo"));
        assert_eq!(map.get_string("PNG:Title"), Some("drop me"));

        map.insert("PNG:ExifMake", TagValue::new_string("Edited"));
        map.insert("PNG:Comment-fr", TagValue::new_string("salut"));
        map.remove("PNG:Title");
        map.insert("PNG:Comment-de", TagValue::new_string("hallo"));
        map.insert("PNG:Description", TagValue::new_string("caf\u{e9}"));
        let (written, reread) = write_and_read(original, &map);

        let chunks = text_chunks_of(&written);
        // new caller-authored chunks: iTXt for a language code or non-ASCII
        // text (WritePNG.pl:196-200)
        assert!(chunks.contains(&(
            *b"iTXt",
            serialize_itxt_chunk(b"Comment", b"de", b"", "hallo")
        )));
        assert!(chunks.contains(&(
            *b"iTXt",
            serialize_itxt_chunk(b"Description", b"", b"", "caf\u{e9}")
        )));
        // edited chunks keep the original keyword bytes (`exif:Make`, which
        // the public name `ExifMake` cannot be inverted to), type, language
        // and translated keyword
        assert!(chunks.contains(&(*b"tEXt", b"exif:Make\0Edited".to_vec())));
        assert!(chunks.contains(&(
            *b"iTXt",
            serialize_itxt_chunk(b"Comment", b"fr", b"Kommentar", "salut")
        )));
        // untouched chunks are carried byte-for-byte, a binary row too
        assert!(chunks.contains(&(*b"zTXt", ztxt)));
        assert!(chunks.contains(&(*b"tEXt", raw_profile)));
        // a removed key removes its chunk
        assert!(!chunks.iter().any(|(_, d)| d.starts_with(b"Title\0")));
        assert_eq!(chunks.len(), 6);

        assert_eq!(reread.get_string("PNG:ExifMake"), Some("Edited"));
        assert_eq!(reread.get_string("PNG:Comment-fr"), Some("salut"));
        assert_eq!(reread.get_string("PNG:Comment-de"), Some("hallo"));
        assert_eq!(reread.get_string("PNG:Description"), Some("caf\u{e9}"));
        assert_eq!(reread.get_string("PNG:Software"), Some("zsw"));
        assert!(reread.get_string("PNG:Title").is_none());
    }

    #[test]
    fn latin_text_round_trips_and_moves_to_itxt_only_when_it_must() {
        let original = png_with(&[(*b"tEXt", b"Comment\0caf\xe9".to_vec())]);
        let map = parse_png_metadata(&TestReader::new(original.clone())).unwrap();
        assert_eq!(map.get_string("PNG:Comment"), Some("caf\u{e9}"));

        // unchanged: carried, so the Latin byte survives
        let (written, _) = write_and_read(original.clone(), &map);
        assert_eq!(
            text_chunks_of(&written),
            vec![(*b"tEXt", b"Comment\0caf\xe9".to_vec())]
        );

        // edited to other Latin text: still tEXt, cp1252-encoded
        let mut edited = map.clone();
        edited.insert("PNG:Comment", TagValue::new_string("na\u{ef}ve \u{20ac}"));
        let (written, reread) = write_and_read(original.clone(), &edited);
        assert_eq!(
            text_chunks_of(&written),
            vec![(*b"tEXt", b"Comment\0na\xefve \x80".to_vec())]
        );
        assert_eq!(
            reread.get_string("PNG:Comment"),
            Some("na\u{ef}ve \u{20ac}")
        );

        // edited to text cp1252 cannot hold: iTXt, same keyword
        let mut cjk = map;
        cjk.insert("PNG:Comment", TagValue::new_string("\u{4f60}\u{597d}"));
        let (written, reread) = write_and_read(original, &cjk);
        assert_eq!(
            text_chunks_of(&written),
            vec![(
                *b"iTXt",
                serialize_itxt_chunk(b"Comment", b"", b"", "\u{4f60}\u{597d}")
            )]
        );
        assert_eq!(reread.get_string("PNG:Comment"), Some("\u{4f60}\u{597d}"));
    }

    #[test]
    fn only_textual_data_names_become_new_chunks() {
        let mut map = MetadataMap::new();
        // read-back keys from other PNG chunks and unknown names are not text
        map.insert("PNG:ImageWidth", TagValue::new_integer(1));
        map.insert(
            "PNG:ModifyDate",
            TagValue::new_string("2020:01:01 00:00:00"),
        );
        map.insert("PNG:ExifMake", TagValue::new_string("no keyword to invert"));
        map.insert(
            "PNG:CreationTime",
            TagValue::new_string("2020:01:01 00:00:00"),
        );
        let (written, _) = write_and_read(png_with(&[]), &map);
        assert_eq!(
            text_chunks_of(&written),
            vec![(*b"tEXt", b"Creation Time\02020:01:01 00:00:00".to_vec())]
        );
    }

    /// A PNG with `pre` between IHDR and IDAT and `post` between IDAT and
    /// IEND.
    fn png_around_idat(pre: &[([u8; 4], Vec<u8>)], post: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = png_with(pre);
        let iend = out.len() - 12;
        let mut tail = Vec::new();
        for (chunk_type, data) in post {
            write_chunk(&mut tail, chunk_type, data);
        }
        out.splice(iend..iend, tail);
        out
    }

    /// Every chunk of a PNG, in order.
    fn all_chunks_of(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 8;
        while at + 8 <= png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let chunk_type: [u8; 4] = png[at + 4..at + 8].try_into().unwrap();
            out.push((chunk_type, png[at + 8..at + 8 + len].to_vec()));
            at += 12 + len;
        }
        out
    }

    /// Reads `original`, applies `edit` to the map, writes, and returns the
    /// written bytes with the original and re-read maps.
    fn edit_round_trip(
        original: Vec<u8>,
        edit: impl FnOnce(&mut MetadataMap),
    ) -> (Vec<u8>, MetadataMap, MetadataMap) {
        let before = parse_png_metadata(&TestReader::new(original.clone())).unwrap();
        let mut map = before.clone();
        edit(&mut map);
        let (written, reread) = write_and_read(original, &map);
        (written, before, reread)
    }

    fn author(map: &mut MetadataMap) {
        map.insert("PNG:Author", TagValue::new_string("newauthor"));
    }

    #[test]
    fn unrelated_edit_carries_text_whose_name_a_later_chunk_also_answers() {
        // A tEXt keyword can print under the same PNG: name as a non-text
        // chunk the reader files later; the map then holds the other
        // chunk's value, which must not be mistaken for an edit.
        // c2: tEXt 'ModifyDate' before IDAT, tIME after it.
        let text = (*b"tEXt", b"ModifyDate\0textdate".to_vec());
        let time = (*b"tIME", vec![0x07, 0xE5, 2, 3, 4, 5, 6]);
        let original = png_around_idat(std::slice::from_ref(&text), std::slice::from_ref(&time));
        let (written, before, reread) = edit_round_trip(original.clone(), author);
        assert_eq!(
            before.get_string("PNG:ModifyDate"),
            Some("2021:02:03 04:05:06")
        );
        let chunks = all_chunks_of(&written);
        assert!(
            chunks.contains(&text),
            "tEXt ModifyDate carried: {chunks:?}"
        );
        assert!(chunks.contains(&time));
        assert_eq!(reread.get("PNG:ModifyDate"), before.get("PNG:ModifyDate"));
        assert_eq!(reread.get_string("PNG:Author"), Some("newauthor"));
        // Changing that key edits what the map shows (the tIME row), which
        // is not this text: the text chunk is still carried, not rewritten
        // with the date.
        let (written, _, _) = edit_round_trip(original, |m| {
            m.insert(
                "PNG:ModifyDate",
                TagValue::new_string("2022:01:01 00:00:00"),
            );
        });
        assert!(all_chunks_of(&written).contains(&text));

        // c4: tEXt 'Gamma' before gAMA (a float row, so not text at all).
        let text = (*b"tEXt", b"Gamma\0textgamma".to_vec());
        let gama = (*b"gAMA", 45455u32.to_be_bytes().to_vec());
        let (written, before, reread) =
            edit_round_trip(png_with(&[text.clone(), gama.clone()]), author);
        assert!(
            before
                .get("PNG:Gamma")
                .is_some_and(|v| v.as_string().is_none())
        );
        let chunks = all_chunks_of(&written);
        assert!(chunks.contains(&text), "tEXt Gamma carried: {chunks:?}");
        assert!(chunks.contains(&gama));
        assert_eq!(reread.get("PNG:Gamma"), before.get("PNG:Gamma"));

        // c7: tEXt Palette and BackgroundColor before PLTE and bKGD.
        let palette = (*b"tEXt", b"Palette\0textpalette".to_vec());
        let background = (*b"tEXt", b"BackgroundColor\0textbg".to_vec());
        let plte = (*b"PLTE", vec![1, 2, 3, 4, 5, 6]);
        let bkgd = (*b"bKGD", vec![0, 1, 0, 2, 0, 3]);
        let (written, before, reread) = edit_round_trip(
            png_with(&[palette.clone(), background.clone(), plte, bkgd]),
            author,
        );
        let chunks = all_chunks_of(&written);
        assert!(
            chunks.contains(&palette),
            "tEXt Palette carried: {chunks:?}"
        );
        assert!(chunks.contains(&background), "tEXt BackgroundColor carried");
        assert_eq!(reread.get("PNG:Palette"), before.get("PNG:Palette"));
        assert_eq!(
            reread.get("PNG:BackgroundColor"),
            before.get("PNG:BackgroundColor")
        );
    }

    #[test]
    fn a_text_chunk_named_xmp_is_text_not_a_packet_replacement() {
        // c1: an unknown tEXt keyword 'XMP' prints as PNG:XMP
        // (PNG.pm:1117-1124) next to a real XML:com.adobe.xmp packet.
        let packet = "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF \
                      xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\
                      <rdf:Description xmlns:dc='http://purl.org/dc/elements/1.1/' \
                      dc:creator='Phil Harvey'/></rdf:RDF></x:xmpmeta>";
        let text = (*b"tEXt", b"XMP\0hello".to_vec());
        let xmp = (
            *b"iTXt",
            serialize_itxt_chunk(XMP_ITXT_KEYWORD.as_bytes(), b"", b"", packet),
        );
        let original = png_with(&[text.clone(), xmp.clone()]);
        let (written, before, reread) = edit_round_trip(original.clone(), author);
        assert_eq!(before.get_string("PNG:XMP"), Some("hello"));
        let chunks = all_chunks_of(&written);
        assert!(chunks.contains(&text), "tEXt XMP carried: {chunks:?}");
        assert!(
            chunks.contains(&xmp),
            "the XMP packet carried byte-for-byte"
        );
        assert!(reread.iter().any(|(k, _)| k.starts_with("XMP")));

        // Editing PNG:XMP edits that text chunk; the packet stays.
        let (written, _, reread) = edit_round_trip(original, |m| {
            m.insert("PNG:XMP", TagValue::new_string("bye"));
        });
        let chunks = all_chunks_of(&written);
        assert!(chunks.contains(&(*b"tEXt", b"XMP\0bye".to_vec())));
        assert!(chunks.contains(&xmp));
        assert_eq!(reread.get_string("PNG:XMP"), Some("bye"));

        // With no text chunk answering it, a new PNG:XMP value replaces
        // the packet.
        let replacement = packet.replace("Phil Harvey", "Someone Else");
        let (written, _, _) = edit_round_trip(png_with(std::slice::from_ref(&xmp)), |m| {
            m.insert("PNG:XMP", TagValue::new_string(replacement.clone()));
        });
        let chunks = text_chunks_of(&written);
        assert!(!chunks.contains(&xmp));
        assert!(chunks.contains(&(
            *b"iTXt",
            serialize_itxt_chunk(XMP_ITXT_KEYWORD.as_bytes(), b"", b"", &replacement)
        )));
    }

    #[test]
    fn test_calculate_crc() {
        // Test CRC calculation for a simple chunk
        let chunk_type = b"tEXt";
        let data = b"Test\0Data";

        let crc1 = calculate_crc(chunk_type, data);
        let crc2 = calculate_crc(chunk_type, data);

        // CRC should be deterministic
        assert_eq!(crc1, crc2);

        // Different data should produce different CRC
        let data2 = b"Test\0Different";
        let crc3 = calculate_crc(chunk_type, data2);
        assert_ne!(crc1, crc3);
    }

    #[test]
    fn test_serialize_text_chunk() {
        let data = serialize_text_chunk(b"Author", b"John Doe");
        assert_eq!(data, b"Author\0John Doe");
    }

    #[test]
    fn test_serialize_itxt_chunk() {
        let data = serialize_itxt_chunk(b"Title", b"", b"", "Test Image");
        // keyword\0 compression_flag compression_method language\0 translated\0 text
        assert_eq!(data, b"Title\0\0\0\0\0Test Image");
    }

    #[test]
    fn test_write_chunk() {
        let mut output = Vec::new();
        let chunk_type = b"tEXt";
        let data = b"Author\0Test";

        write_chunk(&mut output, chunk_type, data);

        // Verify structure: length (4) + type (4) + data (11) + crc (4) = 23 bytes
        assert_eq!(output.len(), 23);

        // Verify length field (big-endian)
        assert_eq!(&output[0..4], &11u32.to_be_bytes());

        // Verify type field
        assert_eq!(&output[4..8], b"tEXt");

        // Verify data field
        assert_eq!(&output[8..19], b"Author\0Test");

        // CRC is in last 4 bytes (we don't verify the value here)
    }
}
