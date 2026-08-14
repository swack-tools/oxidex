//! RealAudio (.ra) binary metadata parser.
//!
//! ExifTool routes a `.ra` file through `Image::ExifTool::Real::ProcessReal`
//! (`Real.pm:516-587`). The file opens with an 8-byte record: the signature
//! `".ra\xfd"`, a big-endian `u16` version, and a big-endian `u16` "extra"
//! field (`Real.pm:565`, `unpack('x4nn', $buff)`). The version selects one of
//! three sub-tables -- `Real::AudioV3`, `Real::AudioV4`, `Real::AudioV5`
//! (`Real.pm:72-74`) -- each walked by `Canon::ProcessSerialData`: fields
//! decode in declared order at cumulative byte offsets, several of them
//! (`TitleLen`/`Title`, `ArtistLen`/`Artist`, `CopyrightLen`/`Copyright`,
//! `CommentLen`/`Comment`) a length-prefixed pair where the earlier field's
//! *value* -- not a fixed offset -- sizes the later one
//! (`Real.pm:313-322`, `Format => 'string[$val{N}]'`).
//!
//! This parser reproduces `Real::AudioV4` (`Real.pm:289-322`), the only
//! version the pinned test corpus exercises. `Real::AudioV3`/`AudioV5`
//! (`Real.pm:270-286`, `327-346`) are declared but not implemented: per
//! AGENTS.md, an unimplemented version is left absent (just the header, no
//! `Real-RA3`/`Real-RA5` tags) rather than approximated against the wrong
//! table -- the same outcome ExifTool itself produces for a version with no
//! matching `.ra$vers` tag at all (`Real.pm:568-577`, "Unsupported
//! RealAudio version").
//!
//! `ProcessSerialData`'s per-field `Unknown => 1` flag (`Real.pm`'s own
//! table) hides a field from ExifTool's default (non-`-u`) output; this
//! parser skips those fields' bytes without emitting a tag, the same
//! visibility ExifTool's default run has.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Real.pm`

use crate::core::{FileReader, MetadataMap, TagValue};

/// `Real.pm:523`, `$buff =~ m{^(\.RMF|\.ra\xfd|pnm://|rtsp://|http://)}`.
const RA_SIGNATURE: &[u8] = b".ra\xfd";

/// `Real.pm:565`, `$raf->Read($buff, 512)` -- the body read following the
/// 8-byte header, once the version is known.
const BODY_READ_LEN: usize = 512;

/// Sequential cursor over `Real::AudioV4`'s big-endian fields
/// (`Real.pm:289-322`), mirroring `Canon::ProcessSerialData`'s cumulative
/// offset tracking.
struct FieldCursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> FieldCursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let bytes = self.data.get(self.pos..self.pos + len)?;
        self.pos += len;
        Some(bytes)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// Skip a length-prefixed string field (`Format => 'string[$val{N}]'`)
    /// whose length was captured by an earlier `*Len` field, returning the
    /// bytes when the length is non-zero.
    ///
    /// A zero-length field is a decline, not an empty value: ExifTool's own
    /// `Real.ra` sample carries `ArtistLen: 0` and `CommentLen: 0`, and
    /// `Artist`/`Comment` do not appear in its `-a -G1 -s` output at all.
    fn string(&mut self, len: u8) -> Option<&'a [u8]> {
        if len == 0 {
            return None;
        }
        self.take(usize::from(len))
    }
}

/// Extract RealAudio (`.ra`) metadata using ExifTool's `Real::AudioV4`
/// sequential layout (version 4 only -- see the module doc for why other
/// versions are left unimplemented rather than approximated).
pub fn parse_real_audio_metadata(
    reader: &dyn FileReader,
) -> std::result::Result<MetadataMap, String> {
    let header = reader.read(0, 8).map_err(|error| error.to_string())?;
    if !header.starts_with(RA_SIGNATURE) {
        return Err("missing RealAudio '.ra\\xfd' signature".to_string());
    }
    // Real.pm:565: `unpack('x4nn', $buff)` -- big-endian u16 version at
    // offset 4, "extra" (unused here) at offset 6.
    let version = u16::from_be_bytes([header[4], header[5]]);

    let mut metadata = MetadataMap::new();
    if version != 4 {
        // Real.pm:568-577: no `.ra$vers` tag table for this version --
        // ExifTool warns and stops after `SetFileType`, reporting no audio
        // tags. `File:FileType`/`MIMEType` still come from
        // `add_identity_tags` in `core::operations`.
        return Ok(metadata);
    }

    let available = reader.size().saturating_sub(8).min(BODY_READ_LEN as u64) as usize;
    let body = reader
        .read(8, available)
        .map_err(|error| error.to_string())?;

    let mut cursor = FieldCursor::new(body);
    let _four_cc1 = cursor.take(4); // Unknown => 1
    let _audio_file_size = cursor.u32(); // Unknown => 1
    let _version2 = cursor.u16(); // Unknown => 1
    let _header_size = cursor.u32(); // Unknown => 1
    let _codec_flavor_id = cursor.u16(); // Unknown => 1
    let _coded_frame_size = cursor.u32(); // Unknown => 1
    let audio_bytes = cursor.u32();
    let bytes_per_minute = cursor.u32();
    let _unknown8 = cursor.u32(); // Unknown => 1
    let _sub_packet_h = cursor.u16(); // Unknown => 1
    let audio_frame_size = cursor.u16();
    let _sub_packet_size = cursor.u16(); // Unknown => 1
    let _unknown12 = cursor.u16(); // Unknown => 1
    let sample_rate = cursor.u16();
    let _unknown14 = cursor.u16(); // Unknown => 1
    let bits_per_sample = cursor.u16();
    let channels = cursor.u16();
    let _four_cc2_len = cursor.u8(); // Unknown => 1
    let _four_cc2 = cursor.take(4); // Unknown => 1
    let _four_cc3_len = cursor.u8(); // Unknown => 1
    let _four_cc3 = cursor.take(4); // Unknown => 1
    let _unknown21 = cursor.u8(); // Unknown => 1
    let _unknown22 = cursor.u16(); // Unknown => 1
    let title_len = cursor.u8();
    let title = title_len.and_then(|len| cursor.string(len));
    let artist_len = cursor.u8();
    let artist = artist_len.and_then(|len| cursor.string(len));
    let copyright_len = cursor.u8();
    let copyright = copyright_len.and_then(|len| cursor.string(len));
    let comment_len = cursor.u8();
    let comment = comment_len.and_then(|len| cursor.string(len));

    const GROUP: &str = "Real-RA4";
    if let Some(value) = audio_bytes {
        metadata.insert(
            format!("{GROUP}:AudioBytes"),
            TagValue::Integer(i64::from(value)),
        );
    }
    if let Some(value) = bytes_per_minute {
        metadata.insert(
            format!("{GROUP}:BytesPerMinute"),
            TagValue::Integer(i64::from(value)),
        );
    }
    if let Some(value) = audio_frame_size {
        metadata.insert(
            format!("{GROUP}:AudioFrameSize"),
            TagValue::Integer(i64::from(value)),
        );
    }
    if let Some(value) = sample_rate {
        metadata.insert(
            format!("{GROUP}:SampleRate"),
            TagValue::Integer(i64::from(value)),
        );
    }
    if let Some(value) = bits_per_sample {
        metadata.insert(
            format!("{GROUP}:BitsPerSample"),
            TagValue::Integer(i64::from(value)),
        );
    }
    if let Some(value) = channels {
        metadata.insert(
            format!("{GROUP}:Channels"),
            TagValue::Integer(i64::from(value)),
        );
    }
    if let Some(bytes) = title {
        metadata.insert(
            format!("{GROUP}:Title"),
            TagValue::new_string(String::from_utf8_lossy(bytes).into_owned()),
        );
    }
    if let Some(bytes) = artist {
        metadata.insert(
            format!("{GROUP}:Artist"),
            TagValue::new_string(String::from_utf8_lossy(bytes).into_owned()),
        );
    }
    if let Some(bytes) = copyright {
        metadata.insert(
            format!("{GROUP}:Copyright"),
            TagValue::new_string(String::from_utf8_lossy(bytes).into_owned()),
        );
    }
    if let Some(bytes) = comment {
        metadata.insert(
            format!("{GROUP}:Comment"),
            TagValue::new_string(String::from_utf8_lossy(bytes).into_owned()),
        );
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::MMapReader;
    use std::path::Path;

    fn fixture_reader() -> MMapReader {
        // Real ExifTool test-suite fixture, not hand-authored bytes: see
        // AGENTS.md's rule that regression fixtures must be real files.
        let candidates = [
            "/tmp/oxidex-exiftool-cache/combined-samples/Real.ra",
            "/tmp/oxidex-exiftool-cache/exiftool/t/images/Real.ra",
        ];
        for candidate in candidates {
            if let Ok(reader) = MMapReader::new(Path::new(candidate)) {
                return reader;
            }
        }
        panic!("Real.ra fixture not found in the oxidex-exiftool-cache");
    }

    #[test]
    fn matches_exiftool_13_59_on_the_real_fixture() {
        let reader = fixture_reader();
        let metadata = parse_real_audio_metadata(&reader).expect("parses");

        // Cross-checked against `exiftool -a -G1 -s` (pinned 13.59) on the
        // same fixture.
        assert_eq!(
            metadata.get("Real-RA4:AudioBytes"),
            Some(&TagValue::Integer(704352))
        );
        assert_eq!(
            metadata.get("Real-RA4:BytesPerMinute"),
            Some(&TagValue::Integer(299743))
        );
        assert_eq!(
            metadata.get("Real-RA4:AudioFrameSize"),
            Some(&TagValue::Integer(348))
        );
        assert_eq!(
            metadata.get("Real-RA4:SampleRate"),
            Some(&TagValue::Integer(22050))
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
        // ArtistLen/CommentLen are 0 in this fixture -- absent, not empty.
        assert_eq!(metadata.get("Real-RA4:Artist"), None);
        assert_eq!(metadata.get("Real-RA4:Comment"), None);
    }
}
