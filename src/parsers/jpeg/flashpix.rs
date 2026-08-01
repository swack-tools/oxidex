//! FlashPix (FPXR) data carried in JPEG APP2/APP4 segments.
//!
//! FlashPix streams are the same OLE property-set structures used by the
//! standalone FPX format, but instead of living in a compound-document
//! filesystem they are chopped into JPEG application segments whose payload
//! starts with `FPXR\0`. FujiFilm, Kodak, HP, Pentax and Sanyo cameras all
//! write them.
//!
//! An FPXR run is a small state machine (ExifTool's
//! `Image::ExifTool::FlashPix::ProcessFPXR`):
//!
//! * a **contents list** segment (type 1) names each stream and gives its
//!   total length, and
//! * one or more **stream data** segments (type 2) carry the bytes, keyed by
//!   the stream's index in that contents list.
//!
//! A stream larger than one segment is split across several, so the bytes must
//! be concatenated before the property set inside can be parsed. Only once a
//! stream reaches its declared length (or the run ends) is it handed to a
//! parser.
//!
//! Note that the marker is not fixed: most cameras use APP2, but some Pentax
//! models write byte-identical FPXR data in APP4 (ExifTool.pm:7949 and 8066).
//!
//! Reference: `Image::ExifTool::FlashPix` (`ProcessFPXR`, `%FlashPix::Main`,
//! `%FlashPix::Extensions`, `%FlashPix::PreviewInfo`, `%FlashPix::Composite`).

use std::collections::BTreeMap;

use crate::core::{MetadataMap, TagValue};
use crate::parsers::archive::ole_properties::{PropertySet, parse_property_stream};
use crate::parsers::jpeg::segment_parser::Segment;

/// Payload prefix identifying an FPXR application segment.
const FPXR_SIGNATURE: &[u8] = b"FPXR\0";

/// `FPXR\0` + version + type + (index, offset) -- the fixed part of a segment.
const FPXR_HEADER_LEN: usize = 13;

/// Stream header skipped by the `ScreenNail` / `AudioStream` ValueConvs.
const STREAM_HEADER_LEN: usize = 0x1c;

/// FujiFilm prepends this much of its own header to a `Preview` stream.
const FUJI_PREVIEW_HEADER_LEN: usize = 47;

/// Contents-list index some FujiFilm models use for a preview they never list.
const FUJI_UNLISTED_INDEX: usize = 512;

/// JPEG start-of-image, used to find the real image inside a wrapper stream.
const JPEG_SOI: [u8; 3] = [0xFF, 0xD8, 0xFF];

/// One entry of an FPXR contents list.
struct ContentsEntry {
    /// Stream name with any directory prefix removed.
    name: String,
    /// Declared total length; `0xffffffff` marks a storage rather than a stream.
    size: u32,
}

/// Accumulates stream fragments across the segments of one FPXR run.
#[derive(Default)]
struct FpxrState {
    contents: Option<Vec<ContentsEntry>>,
    /// Partially assembled streams, keyed by contents-list index.
    streams: BTreeMap<usize, Vec<u8>>,
    /// Bytes gathered by the unlisted-index FujiFilm preview path.
    fuji_preview: Vec<u8>,
    /// `ScreenNail` payload, kept for the composite `PreviewImage`.
    screen_nail: Option<Vec<u8>>,
}

/// Extracts FlashPix metadata from a JPEG's APP2/APP4 FPXR segments.
///
/// # Arguments
///
/// * `segments` - Parsed JPEG segments, in file order
/// * `metadata` - MetadataMap to populate with `FlashPix:*` tags
pub fn process_fpxr_segments(segments: &[Segment], metadata: &mut MetadataMap) {
    // ExifTool flags the last segment of a same-marker FPXR run so it can flush
    // streams that never reached their declared length.
    let is_fpxr =
        |s: &Segment| matches!(s.marker, 0xFFE2 | 0xFFE4) && s.data.starts_with(FPXR_SIGNATURE);

    let mut state = FpxrState::default();
    let mut seen_any = false;

    for (i, segment) in segments.iter().enumerate() {
        if !is_fpxr(segment) {
            continue;
        }
        seen_any = true;
        let last = !segments
            .get(i + 1)
            .is_some_and(|next| next.marker == segment.marker && is_fpxr(next));
        state.process_segment(segment.data, last, metadata);
    }

    if !seen_any {
        return;
    }
    // Anything still buffered when the file ends (a truncated final run).
    state.flush(metadata);
    state.emit_composite_preview(metadata);
}

impl FpxrState {
    fn process_segment(&mut self, data: &[u8], last: bool, metadata: &mut MetadataMap) {
        if data.len() < FPXR_HEADER_LEN {
            return;
        }
        match data[6] {
            1 => self.read_contents_list(data),
            2 => self.read_stream_data(data, metadata),
            // 3 is "Reserved"; anything else is unknown to ExifTool too.
            _ => {}
        }
        if last {
            self.flush(metadata);
        }
    }

    /// Parse a type-1 segment: the list of streams this run will deliver.
    fn read_contents_list(&mut self, data: &[u8]) {
        let count = u16::from_be_bytes([data[7], data[8]]) as usize;
        let mut entries = Vec::new();
        let mut pos = 9usize;

        for _ in 0..count {
            let Some(size_bytes) = data.get(pos..pos + 4) else {
                // A truncated list is unusable: ExifTool abandons the whole run
                // rather than indexing into a partial one.
                self.contents = None;
                return;
            };
            let size =
                u32::from_be_bytes([size_bytes[0], size_bytes[1], size_bytes[2], size_bytes[3]]);

            // The pathname is UTF-16LE (even though the size above is
            // big-endian), must open with '/', and ends at a NUL unit.
            let name_start = pos + 5;
            if data.get(name_start) != Some(&b'/') || data.get(name_start + 1) != Some(&0) {
                self.contents = None;
                return;
            }
            let Some(name_end) = utf16_terminator(data, name_start) else {
                self.contents = None;
                return;
            };
            let name = decode_utf16le(&data[name_start..name_end]);
            let mut next = name_end + 2;

            // A storage (rather than a stream) is followed by its class ID.
            if size == u32::MAX {
                next += 16;
                if next > data.len() {
                    self.contents = None;
                    return;
                }
            }
            pos = next;

            // "remove directory specification"
            let base = match name.rfind('/') {
                Some(i) => name[i + 1..].to_string(),
                None => name,
            };
            entries.push(ContentsEntry { name: base, size });
        }

        self.contents = Some(entries);
    }

    /// Parse a type-2 segment: a slice of one stream's bytes.
    fn read_stream_data(&mut self, data: &[u8], metadata: &mut MetadataMap) {
        let index = u16::from_be_bytes([data[7], data[8]]) as usize;
        let offset = u32::from_be_bytes([data[9], data[10], data[11], data[12]]) as usize;
        let payload = &data[FPXR_HEADER_LEN..];

        let Some(entry) = self
            .contents
            .as_ref()
            .and_then(|entries| entries.get(index))
        else {
            self.absorb_unlisted_segment(index, data);
            return;
        };
        let (name, size) = (entry.name.clone(), entry.size as usize);

        match self.streams.get_mut(&index) {
            // The offset of a stream's first segment is not always 0 even when
            // it should be, so it is deliberately ignored.
            None => {
                self.streams.insert(index, payload.to_vec());
            }
            Some(buffer) => {
                // Segments may repeat bytes already stored; by convention the
                // overlap is dropped rather than appended twice.
                let overlap = buffer.len() as i64 - offset as i64;
                let start = if overlap < 0 || overlap > payload.len() as i64 {
                    // A nonsensical offset: keep the bytes rather than lose them.
                    0
                } else {
                    overlap as usize
                };
                buffer.extend_from_slice(&payload[start..]);
            }
        }

        if self.streams.get(&index).is_some_and(|b| b.len() >= size) {
            let mut stream = self.streams.remove(&index).unwrap_or_default();
            // Trailing bytes beyond the declared length are not part of it.
            stream.truncate(size);
            self.handle_stream(&name, &stream, metadata);
        }
    }

    /// The FujiFilm path for a preview stream that no contents list announces.
    ///
    /// Such segments claim index 512 and carry a 47-byte FujiFilm header after
    /// the 13-byte FPXR one; the first is recognised by the JPEG signature that
    /// follows, and later ones simply continue the run.
    fn absorb_unlisted_segment(&mut self, index: usize, data: &[u8]) {
        const SKIP: usize = FPXR_HEADER_LEN + FUJI_PREVIEW_HEADER_LEN;

        if index != FUJI_UNLISTED_INDEX || data.len() <= SKIP {
            return;
        }
        let continues = !self.fuji_preview.is_empty();
        let starts = data
            .get(SKIP..SKIP + 4)
            .is_some_and(|sig| sig == [0xFF, 0xD8, 0xFF, 0xDB]);
        if continues || starts {
            self.fuji_preview.extend_from_slice(&data[SKIP..]);
        }
    }

    /// Route a fully assembled stream to the parser its name selects.
    fn handle_stream(&mut self, name: &str, stream: &[u8], metadata: &mut MetadataMap) {
        // Instance numbers ("Audio Stream 000000") and class IDs ("\x05Screen
        // Nail_bd0100609719a180") are suffixes on an otherwise known name.
        let base = strip_instance_suffix(name);

        match base {
            "\u{5}Extension List" => {
                parse_property_stream(stream, PropertySet::Extensions, metadata);
            }
            "\u{5}Audio Info" => {
                parse_property_stream(stream, PropertySet::AudioInfo, metadata);
            }
            "\u{5}SummaryInformation" => {
                parse_property_stream(stream, PropertySet::SummaryInfo, metadata);
            }
            "\u{5}DocumentSummaryInformation" => {
                parse_property_stream(stream, PropertySet::DocumentInfo, metadata);
            }
            "\u{5}Screen Nail" => {
                let value = strip_stream_header(stream);
                metadata.insert("FlashPix:ScreenNail", binary_placeholder(value.len()));
                // Held back: the composite PreviewImage is the JPEG inside it.
                self.screen_nail = Some(value.to_vec());
            }
            "Audio Stream" => {
                let value = strip_stream_header(stream);
                metadata.insert("FlashPix:AudioStream", binary_placeholder(value.len()));
            }
            "Property" => insert_preview_info(stream, metadata),
            "Preview" => {
                // A FujiFilm header precedes the JPEG; if what follows is not
                // an image, there is nothing to report.
                if stream.len() > FUJI_PREVIEW_HEADER_LEN {
                    let value = &stream[FUJI_PREVIEW_HEADER_LEN..];
                    if value.starts_with(&JPEG_SOI) {
                        metadata.insert("FlashPix:PreviewImage", binary_placeholder(value.len()));
                    }
                }
            }
            _ => {}
        }
    }

    /// End of an FPXR run: parse whatever arrived short of its declared length.
    fn flush(&mut self, metadata: &mut MetadataMap) {
        if let Some(contents) = self.contents.take() {
            for (index, stream) in std::mem::take(&mut self.streams) {
                if stream.is_empty() {
                    continue;
                }
                if let Some(entry) = contents.get(index) {
                    let name = entry.name.clone();
                    self.handle_stream(&name, &stream, metadata);
                }
            }
        }
        self.streams.clear();

        if !self.fuji_preview.is_empty() {
            let preview = std::mem::take(&mut self.fuji_preview);
            metadata.insert("FlashPix:PreviewImage", binary_placeholder(preview.len()));
        }
    }

    /// `%FlashPix::Composite`: the embedded JPEG inside a `ScreenNail`.
    fn emit_composite_preview(&mut self, metadata: &mut MetadataMap) {
        let Some(screen_nail) = self.screen_nail.take() else {
            return;
        };
        // The image starts at the first SOI; everything before it is wrapper.
        let Some(start) = find_subslice(&screen_nail, &JPEG_SOI) else {
            return;
        };
        let len = screen_nail.len() - start;
        metadata.insert("FlashPix:PreviewImage", binary_placeholder(len));
    }
}

/// `%FlashPix::PreviewInfo`: a fixed big-endian block written by FujiFilm.
fn insert_preview_info(stream: &[u8], metadata: &mut MetadataMap) {
    if let Some(b) = stream.get(0x0d..0x0f) {
        metadata.insert(
            "FlashPix:PreviewImageWidth",
            TagValue::Integer(u16::from_be_bytes([b[0], b[1]]) as i64),
        );
    }
    if let Some(b) = stream.get(0x17..0x19) {
        metadata.insert(
            "FlashPix:PreviewImageHeight",
            TagValue::Integer(u16::from_be_bytes([b[0], b[1]]) as i64),
        );
    }
}

/// ExifTool's stand-in for binary values that `-b` was not requested for.
fn binary_placeholder(len: usize) -> TagValue {
    TagValue::String(format!(
        "(Binary data {} bytes, use -b option to extract)",
        len
    ))
}

/// Drop the OLE stream header that precedes `ScreenNail` / `AudioStream` data.
fn strip_stream_header(stream: &[u8]) -> &[u8] {
    if stream.len() > STREAM_HEADER_LEN {
        &stream[STREAM_HEADER_LEN..]
    } else {
        stream
    }
}

/// Remove a trailing instance number (` 000000`) or class ID (`_<16 hex>`).
///
/// Both suffixes are pure ASCII, so a byte-level match also proves the cut
/// point is a character boundary -- which matters because these names come
/// from the file and may hold arbitrary UTF-16.
fn strip_instance_suffix(name: &str) -> &str {
    let bytes = name.as_bytes();

    // " 000000"
    if let Some(cut) = bytes.len().checked_sub(7)
        && cut > 0
        && bytes[cut] == b' '
        && bytes[cut + 1..].iter().all(u8::is_ascii_digit)
    {
        return &name[..cut];
    }

    // "_bd0100609719a180"
    if let Some(cut) = bytes.len().checked_sub(17)
        && cut > 0
        && bytes[cut] == b'_'
        && bytes[cut + 1..]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return &name[..cut];
    }

    name
}

/// Offset of the NUL unit ending a UTF-16LE string starting at `start`.
fn utf16_terminator(data: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            return Some(i);
        }
        i += 2;
    }
    None
}

/// Decode UTF-16LE, mapping unpaired units the way ExifTool's Latin fallback does.
fn decode_utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds one FPXR segment payload.
    fn contents_list(entries: &[(&str, u32)]) -> Vec<u8> {
        let mut out = b"FPXR\0".to_vec();
        out.push(0); // version
        out.push(1); // type: contents list
        out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
        for (name, size) in entries {
            out.extend_from_slice(&size.to_be_bytes());
            out.push(0); // default value
            for unit in format!("/{}", name).encode_utf16() {
                out.extend_from_slice(&unit.to_le_bytes());
            }
            out.extend_from_slice(&[0, 0]);
            if *size == u32::MAX {
                out.extend_from_slice(&[0u8; 16]);
            }
        }
        out
    }

    fn stream_data(index: u16, offset: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = b"FPXR\0".to_vec();
        out.push(0);
        out.push(2); // type: stream data
        out.extend_from_slice(&index.to_be_bytes());
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn run(payloads: &[(u16, Vec<u8>)]) -> MetadataMap {
        let segments: Vec<Segment<'_>> = payloads
            .iter()
            .map(|(marker, data)| Segment::new(*marker, 0, data.as_slice()))
            .collect();
        let mut metadata = MetadataMap::new();
        process_fpxr_segments(&segments, &mut metadata);
        metadata
    }

    fn screen_nail_stream(preamble: usize, jpeg: usize) -> Vec<u8> {
        let mut out = vec![0xAA; STREAM_HEADER_LEN + preamble];
        out.extend_from_slice(&JPEG_SOI);
        out.extend(std::iter::repeat_n(0x42, jpeg - JPEG_SOI.len()));
        out
    }

    #[test]
    fn assembles_a_stream_split_across_segments() {
        let body: Vec<u8> = (0..200u32).map(|i| i as u8).collect();
        let list = contents_list(&[("Audio Stream 000000", body.len() as u32)]);
        let payloads = vec![
            (0xFFE2, list),
            (0xFFE2, stream_data(0, 0, &body[..120])),
            (0xFFE2, stream_data(0, 120, &body[120..])),
        ];
        let metadata = run(&payloads);
        assert_eq!(
            metadata.get("FlashPix:AudioStream"),
            Some(&binary_placeholder(200 - STREAM_HEADER_LEN))
        );
    }

    #[test]
    fn drops_bytes_a_later_segment_repeats() {
        // The second segment restates the last 20 bytes the first delivered.
        let body: Vec<u8> = (0..200u32).map(|i| i as u8).collect();
        let list = contents_list(&[("Audio Stream 000000", body.len() as u32)]);
        let payloads = vec![
            (0xFFE2, list),
            (0xFFE2, stream_data(0, 0, &body[..120])),
            (0xFFE2, stream_data(0, 100, &body[100..])),
        ];
        let metadata = run(&payloads);
        assert_eq!(
            metadata.get("FlashPix:AudioStream"),
            Some(&binary_placeholder(200 - STREAM_HEADER_LEN))
        );
    }

    #[test]
    fn reads_fpxr_from_app4_as_well_as_app2() {
        let stream = screen_nail_stream(140, 300);
        let list = contents_list(&[("\u{5}Screen Nail_bd0100609719a180", stream.len() as u32)]);
        for marker in [0xFFE2u16, 0xFFE4] {
            let metadata = run(&[(marker, list.clone()), (marker, stream_data(0, 0, &stream))]);
            assert_eq!(
                metadata.get("FlashPix:ScreenNail"),
                Some(&binary_placeholder(140 + 300)),
                "marker {:#06x}",
                marker
            );
        }
    }

    #[test]
    fn composite_preview_starts_at_the_screen_nail_soi() {
        let stream = screen_nail_stream(140, 300);
        let list = contents_list(&[("\u{5}Screen Nail_bd0100609719a180", stream.len() as u32)]);
        let metadata = run(&[(0xFFE2, list), (0xFFE2, stream_data(0, 0, &stream))]);
        assert_eq!(
            metadata.get("FlashPix:PreviewImage"),
            Some(&binary_placeholder(300)),
        );
    }

    #[test]
    fn flushes_a_stream_that_never_reaches_its_declared_length() {
        // The contents list promises 500 bytes but only 300 arrive.
        let stream = screen_nail_stream(140, 300);
        let list = contents_list(&[("\u{5}Screen Nail_bd0100609719a180", 5000)]);
        let metadata = run(&[(0xFFE2, list), (0xFFE2, stream_data(0, 0, &stream))]);
        assert_eq!(
            metadata.get("FlashPix:ScreenNail"),
            Some(&binary_placeholder(stream.len() - STREAM_HEADER_LEN))
        );
    }

    #[test]
    fn reads_fujifilm_preview_info_as_big_endian() {
        let mut property = vec![0u8; 37];
        property[0x0d] = 0x02;
        property[0x0e] = 0x80; // 640
        property[0x17] = 0x01;
        property[0x18] = 0xe0; // 480
        let list = contents_list(&[("Property", property.len() as u32)]);
        let metadata = run(&[(0xFFE2, list), (0xFFE2, stream_data(0, 0, &property))]);
        assert_eq!(
            metadata.get("FlashPix:PreviewImageWidth"),
            Some(&TagValue::Integer(640))
        );
        assert_eq!(
            metadata.get("FlashPix:PreviewImageHeight"),
            Some(&TagValue::Integer(480))
        );
    }

    #[test]
    fn preview_stream_skips_the_fujifilm_header() {
        let mut preview = vec![0u8; FUJI_PREVIEW_HEADER_LEN];
        preview.extend_from_slice(&JPEG_SOI);
        preview.extend(std::iter::repeat_n(0x11, 997));
        let list = contents_list(&[("Preview", preview.len() as u32)]);
        let metadata = run(&[(0xFFE2, list), (0xFFE2, stream_data(0, 0, &preview))]);
        assert_eq!(
            metadata.get("FlashPix:PreviewImage"),
            Some(&binary_placeholder(1000))
        );
    }

    #[test]
    fn preview_stream_without_a_jpeg_reports_nothing() {
        let preview = vec![0u8; FUJI_PREVIEW_HEADER_LEN + 100];
        let list = contents_list(&[("Preview", preview.len() as u32)]);
        let metadata = run(&[(0xFFE2, list), (0xFFE2, stream_data(0, 0, &preview))]);
        assert_eq!(metadata.get("FlashPix:PreviewImage"), None);
    }

    #[test]
    fn gathers_the_unlisted_fujifilm_preview() {
        // No contents list at all: index 512 with the FujiFilm signature.
        // Every such segment carries its own 47-byte header, continuations
        // included, so each contributes only what follows it.
        let mut first = vec![0u8; FUJI_PREVIEW_HEADER_LEN];
        first.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xDB]);
        first.extend(std::iter::repeat_n(0x33, 96));

        let mut second = vec![0u8; FUJI_PREVIEW_HEADER_LEN];
        second.extend(std::iter::repeat_n(0x44, 400));

        let metadata = run(&[
            (0xFFE2, stream_data(512, 0, &first)),
            (0xFFE2, stream_data(512, 0, &second)),
        ]);
        assert_eq!(
            metadata.get("FlashPix:PreviewImage"),
            Some(&binary_placeholder(100 + 400))
        );
    }

    #[test]
    fn ignores_an_unlisted_index_without_the_fujifilm_signature() {
        let payload = vec![0u8; FUJI_PREVIEW_HEADER_LEN + 100];
        let metadata = run(&[(0xFFE2, stream_data(512, 0, &payload))]);
        assert_eq!(metadata.get("FlashPix:PreviewImage"), None);
    }

    #[test]
    fn a_truncated_contents_list_is_abandoned() {
        let mut list = contents_list(&[("Audio Stream 000000", 40)]);
        list.truncate(list.len() - 6);
        let metadata = run(&[(0xFFE2, list), (0xFFE2, stream_data(0, 0, &[0u8; 40]))]);
        assert_eq!(metadata.get("FlashPix:AudioStream"), None);
    }

    #[test]
    fn non_fpxr_app2_segments_are_left_alone() {
        let mut metadata = MetadataMap::new();
        let icc = b"ICC_PROFILE\0\x01\x01rest".to_vec();
        let segments = vec![Segment::new(0xFFE2, 0, icc.as_slice())];
        process_fpxr_segments(&segments, &mut metadata);
        assert_eq!(metadata.len(), 0);
    }

    #[test]
    fn strips_instance_numbers_and_class_ids() {
        assert_eq!(strip_instance_suffix("Audio Stream 000000"), "Audio Stream");
        assert_eq!(
            strip_instance_suffix("\u{5}Screen Nail_bd0100609719a180"),
            "\u{5}Screen Nail"
        );
        // Neither shape: left untouched.
        assert_eq!(strip_instance_suffix("Preview"), "Preview");
        assert_eq!(
            strip_instance_suffix("Audio Stream 00000z"),
            "Audio Stream 00000z"
        );
        // Uppercase hex is not the class-ID shape ExifTool matches.
        assert_eq!(
            strip_instance_suffix("Thing_BD0100609719A180"),
            "Thing_BD0100609719A180"
        );
    }

    #[test]
    fn suffix_stripping_survives_multi_byte_names() {
        // A stream name is arbitrary UTF-16 from the file, so the suffix test
        // must never split in the middle of a character.
        for name in [
            "\u{5}\u{4e2d}\u{6587}\u{30c6}",
            "\u{1f600}\u{1f600}",
            "\u{e9}",
        ] {
            assert_eq!(strip_instance_suffix(name), name);
        }
        // Same, but long enough to reach both suffix windows.
        let padded = "\u{1f600}".repeat(8);
        assert_eq!(strip_instance_suffix(&padded), padded);
    }
}
