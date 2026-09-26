//! RealAudio (`.ra`) metadata parser.
//!
//! ExifTool's `Image::ExifTool::Real::ProcessReal` first reads an eight-byte
//! `.ra` header, then chooses `Real::AudioV3`, `::AudioV4`, or `::AudioV5`
//! from its version (`Real.pm:563-587`).  This caller deliberately enables
//! only the source-generated `Real::AudioV4` serial descriptor.  The other
//! versions remain inactive until their carrier semantics have equivalent
//! native proof; a descriptor's presence alone is not a route.
//!
//! `ProcessReal` asks its file handle for 512 bytes after the header, but Perl
//! accepts any nonzero short read.  The carrier therefore passes at most the
//! first 512 available bytes into the shared `ProcessSerialData` reader rather
//! than requiring a complete 512-byte audio header.  The shared reader owns
//! AudioV4's 31 serial slots, dynamic string lengths, `Unknown` cursor rows,
//! NUL handling, and source groups.  This module retains only carrier
//! signature/version selection and that bounded file read.
//!
//! References: pinned ExifTool 13.59 `lib/Image/ExifTool/Real.pm:289-322,
//! 559-587`; serial descriptor generated from `Canon::ProcessSerialData`.

use std::collections::HashMap;

use crate::core::{FileReader, Instance, MetadataMap};
use crate::exiftool_tables::{
    Ctx, Emitted, SerialDir, SerialEmissionSink, SerialTable, find_serial_table,
    process_serial_directory,
};
use crate::io::ByteOrder;

/// `Real.pm:523`, `$buff =~ m{^(.RMF|.ra\\xfd|...)}`.
const RA_SIGNATURE: &[u8] = b".ra\xfd";
/// `Real.pm:566`, `$raf->Read($buff, 512)`.
const BODY_READ_LEN: usize = 512;

/// Project source-described `FoundTag` rows into the parser's existing
/// occurrence store. The visible key remains ExifTool group 1 plus name, as
/// the old RealAudio parser used; priority and the `-n` form remain attached
/// to the occurrence. This insertion API derives occurrence group 0 from that
/// visible key (`Real-RA4`), keeps generated group 1 (`Real-RA4`), and has no
/// group-2 parameter. Native AudioV4 group 0 (`Real`) and group 2 (including
/// per-row `Author`) therefore remain descriptor facts, not occurrence-level
/// metadata claims in this narrow carrier migration.
struct MetadataSink<'a> {
    metadata: &'a mut MetadataMap,
}

impl SerialEmissionSink for MetadataSink<'_> {
    fn emit(&mut self, row: Emitted) {
        let key = format!("{}:{}", row.group1, row.name);
        let priority = u8::from(!(row.low_priority || row.avoid));
        match row.value_conv {
            Some(value_conv) => {
                self.metadata.insert_occurrence_with_raw(
                    key,
                    row.value,
                    value_conv,
                    priority,
                    row.group1,
                    Instance::default(),
                );
            }
            None => {
                self.metadata.insert_occurrence(
                    key,
                    row.value,
                    priority,
                    row.group1,
                    Instance::default(),
                );
            }
        }
    }

    fn serial_enabled(&self, _table: &'static SerialTable) -> bool {
        true
    }
}

/// Extract RealAudio metadata through the source-generated AudioV4 descriptor.
///
/// The handwritten carrier selection is intentionally narrow: ExifTool's
/// version-four route is the only Real Audio route whose generated reader has
/// native replay coverage.  Unsupported versions retain the existing empty
/// audio-tag result rather than borrowing a nearby serial layout.
pub fn parse_real_audio_metadata(
    reader: &dyn FileReader,
) -> std::result::Result<MetadataMap, String> {
    // Every row here is read from the file (`metadata_map::file_rows`):
    // a caller's later `insert`/`get_mut` is what counts as assigned.
    crate::core::metadata_map::file_rows(|| -> std::result::Result<MetadataMap, String> {
        let header = reader.read(0, 8).map_err(|error| error.to_string())?;
        if !header.starts_with(RA_SIGNATURE) {
            return Err("missing RealAudio '.ra\\xfd' signature".to_string());
        }
        // Real.pm:565: `unpack('x4nn', $buff)`.
        let version = u16::from_be_bytes([header[4], header[5]]);
        let mut metadata = MetadataMap::new();
        if version != 4 {
            return Ok(metadata);
        }

        // Perl IO's Read succeeds with a positive short read.  FileReader is
        // exact-length, so request only the bytes actually available while still
        // preserving ProcessReal's 512-byte upper bound.
        let available = reader.size().saturating_sub(8).min(BODY_READ_LEN as u64) as usize;
        let body = reader
            .read(8, available)
            .map_err(|error| error.to_string())?;
        if body.is_empty() {
            // Native warns and returns after a zero-byte body read.  This parser's
            // Result has no warning channel; preserving its prior output behavior
            // means returning the header-only map without invented AudioV4 rows.
            return Ok(metadata);
        }

        let table = find_serial_table("Real", "AudioV4")
            .expect("generated Real::AudioV4 descriptor must accompany its opted-in carrier");
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let mut sink = MetadataSink {
            metadata: &mut metadata,
        };
        let _result = process_serial_directory(
            table,
            SerialDir {
                data: body,
                dir_start: 0,
                dir_len: body.len(),
                base: 0,
                data_pos: 8,
                byte_order: ByteOrder::Big,
            },
            &mut ctx,
            &mut sink,
        );
        Ok(metadata)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{FileReader, TagValue};
    use crate::test_support::{TestReader, pinned_fixture_reader};
    use std::cell::RefCell;
    use std::io;

    fn u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    /// A carrier-level AudioV4 sample.  The descriptor, rather than this
    /// fixture helper, defines slot order and names.
    fn audio_v4(title: &[u8], artist: &[u8], copyright: &[u8], comment: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(b".ra4"); // FourCC1, Unknown
        u32(&mut body, 0); // AudioFileSize, Unknown
        u16(&mut body, 4); // Version2, Unknown
        u32(&mut body, 0); // HeaderSize, Unknown
        u16(&mut body, 0); // CodecFlavorID, Unknown
        u32(&mut body, 0); // CodedFrameSize, Unknown
        u32(&mut body, 9_000); // AudioBytes
        u32(&mut body, 1_200); // BytesPerMinute
        u32(&mut body, 0); // Unknown
        u16(&mut body, 0); // SubPacketH, Unknown
        u16(&mut body, 256); // AudioFrameSize
        u16(&mut body, 0); // SubPacketSize, Unknown
        u16(&mut body, 0); // Unknown
        u16(&mut body, 44_100); // SampleRate
        u16(&mut body, 0); // Unknown
        u16(&mut body, 16); // BitsPerSample
        u16(&mut body, 2); // Channels
        body.push(4); // FourCC2Len, Unknown
        body.extend_from_slice(b"cook"); // FourCC2, Unknown
        body.push(4); // FourCC3Len, Unknown
        body.extend_from_slice(b"genr"); // FourCC3, Unknown
        body.push(0); // Unknown
        u16(&mut body, 0); // Unknown
        for text in [title, artist, copyright, comment] {
            body.push(u8::try_from(text.len()).expect("fixture string fits native int8u length"));
            body.extend_from_slice(text);
        }

        let mut file = Vec::with_capacity(8 + body.len());
        file.extend_from_slice(RA_SIGNATURE);
        u16(&mut file, 4);
        u16(&mut file, 0);
        file.extend_from_slice(&body);
        file
    }

    #[test]
    fn matches_exiftool_13_59_on_the_real_fixture() {
        let Some(reader) = pinned_fixture_reader("Real.ra") else {
            return;
        };
        let metadata = parse_real_audio_metadata(&reader).expect("parses");

        assert_eq!(
            metadata.get("Real-RA4:AudioBytes"),
            Some(&TagValue::Integer(704_352))
        );
        assert_eq!(
            metadata.get("Real-RA4:BytesPerMinute"),
            Some(&TagValue::Integer(299_743))
        );
        assert_eq!(
            metadata.get("Real-RA4:AudioFrameSize"),
            Some(&TagValue::Integer(348))
        );
        assert_eq!(
            metadata.get("Real-RA4:SampleRate"),
            Some(&TagValue::Integer(22_050))
        );
        assert_eq!(
            metadata.get("Real-RA4:BitsPerSample"),
            Some(&TagValue::Integer(16))
        );
        assert_eq!(
            metadata.get("Real-RA4:Channels"),
            Some(&TagValue::Integer(1))
        );
        assert_eq!(
            metadata.get("Real-RA4:Title"),
            Some(&TagValue::new_string("The Sewing Girls"))
        );
        assert_eq!(
            metadata.get("Real-RA4:Copyright"),
            Some(&TagValue::new_string(
                "** institut f?r universelle zusammenh?nge"
            ))
        );
        // ArtistLen/CommentLen are zero in this fixture: absent, not empty.
        assert_eq!(metadata.get("Real-RA4:Artist"), None);
        assert_eq!(metadata.get("Real-RA4:Comment"), None);
    }

    #[test]
    fn generated_v4_reader_preserves_short_body_and_string_boundaries() {
        let bytes = audio_v4(b"\0tail", b"", b"\xe9", b"ok");
        let metadata = parse_real_audio_metadata(&TestReader::new(bytes)).expect("parses");

        assert_eq!(
            metadata.get("Real-RA4:AudioBytes"),
            Some(&TagValue::Integer(9_000))
        );
        // Positive count with a leading/trailing NUL is reported as the
        // NUL-truncated string; count zero does not call FoundTag.
        assert_eq!(
            metadata.get("Real-RA4:Title"),
            Some(&TagValue::new_string(""))
        );
        assert_eq!(metadata.get("Real-RA4:Artist"), None);
        // Native's output repair occurs only at reporting time.
        assert_eq!(
            metadata.get("Real-RA4:Copyright"),
            Some(&TagValue::new_string("?"))
        );
        assert_eq!(
            metadata.get("Real-RA4:Comment"),
            Some(&TagValue::new_string("ok"))
        );
    }

    #[test]
    fn one_byte_or_header_only_body_invents_no_v4_rows() {
        let mut one_byte = Vec::from(RA_SIGNATURE);
        u16(&mut one_byte, 4);
        u16(&mut one_byte, 0);
        one_byte.push(0);
        assert!(
            parse_real_audio_metadata(&TestReader::new(one_byte))
                .expect("native accepts a nonzero short body read")
                .is_empty()
        );

        let mut header_only = Vec::from(RA_SIGNATURE);
        u16(&mut header_only, 4);
        u16(&mut header_only, 0);
        assert!(
            parse_real_audio_metadata(&TestReader::new(header_only))
                .expect("parser has no native warning channel")
                .is_empty()
        );
    }

    #[test]
    fn overlarge_dynamic_length_stops_later_rows_but_keeps_prior_scalars() {
        let mut bytes = audio_v4(b"title", b"artist", b"copy", b"comment");
        // AudioV4's TitleLen is the byte immediately before the title.  Make
        // it extend beyond this bounded body: ProcessSerialData retains the
        // numeric slots already read and stops before Title/remaining rows.
        let title_len = 8 + 61;
        bytes[title_len] = u8::MAX;
        bytes.truncate(title_len + 4);
        let metadata = parse_real_audio_metadata(&TestReader::new(bytes)).expect("parses");
        assert_eq!(
            metadata.get("Real-RA4:SampleRate"),
            Some(&TagValue::Integer(44_100))
        );
        assert_eq!(metadata.get("Real-RA4:Title"), None);
        assert_eq!(metadata.get("Real-RA4:Artist"), None);
    }

    #[test]
    fn incomplete_artist_is_not_reinterpreted_as_copyright_or_comment() {
        // Native ProcessSerialData stops when the declared Artist span does
        // not fit in ProcessReal's 512-byte read. The former manual cursor
        // stayed at that span after failure, then reused its bytes as later
        // string lengths and invented Copyright and Comment values.
        let title = vec![b'A'; 255];
        let artist = vec![b'B'; 255];
        let bytes = audio_v4(&title, &artist, b"copyright", b"comment");
        let metadata = parse_real_audio_metadata(&TestReader::new(bytes)).expect("parses");
        assert_eq!(
            metadata.get("Real-RA4:Title"),
            Some(&TagValue::new_string("A".repeat(255)))
        );
        for name in ["Artist", "Copyright", "Comment"] {
            assert_eq!(metadata.get(&format!("Real-RA4:{name}")), None);
        }
    }

    struct RecordingReader {
        data: Vec<u8>,
        reads: RefCell<Vec<(u64, usize)>>,
    }

    impl FileReader for RecordingReader {
        fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
            self.reads.borrow_mut().push((offset, length));
            let start = usize::try_from(offset).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "offset does not fit usize")
            })?;
            let end = start.checked_add(length).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "read range overflows")
            })?;
            self.data
                .get(start..end)
                .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "read past fixture"))
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    #[test]
    fn carrier_reads_at_most_native_512_byte_window() {
        let mut bytes = audio_v4(b"title", b"artist", b"copy", b"comment");
        bytes.resize(8 + BODY_READ_LEN + 88, 0xa5);
        let reader = RecordingReader {
            data: bytes,
            reads: RefCell::new(Vec::new()),
        };
        let metadata = parse_real_audio_metadata(&reader).expect("parses");
        assert_eq!(
            metadata.get("Real-RA4:Title"),
            Some(&TagValue::new_string("title"))
        );
        assert_eq!(reader.reads.into_inner(), vec![(0, 8), (8, BODY_READ_LEN)]);
    }
}
