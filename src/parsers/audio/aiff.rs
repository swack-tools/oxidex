//! AIFF / AIFC audio format parser
//!
//! AIFF is an IFF container: a `FORM` wrapper whose form type is `AIFF`
//! (uncompressed) or `AIFC` (compressed), followed by a flat sequence of
//! chunks. ExifTool reads it in `AIFF.pm`, whose `ProcessAIFF` also serves
//! DjVu -- the two share the IFF walker and nothing else.
//!
//! # Supported Metadata
//!
//! - **COMM (Common):** NumChannels, NumSampleFrames, SampleSize, SampleRate,
//!   CompressionType (AIFC only)
//! - **COMT (Comment):** CommentTime, MarkerID, Comment
//! - **Text chunks:** NAME → Name, AUTH → Author, `(c) ` → Copyright,
//!   ANNO → Annotation
//! - **`ID3 `:** a standard ID3v2 blob, read by the shared ID3 reader in
//!   [`crate::parsers::audio::mp3`]
//!
//! # ExifTool Compatibility
//!
//! Maps to ExifTool tags from `AIFF.pm` (v1.13 in the pinned 13.59):
//! - `AIFF:SampleRate` → COMM bytes 8..18, an 80-bit IEEE 754 extended float
//! - `AIFF:CommentTime` → COMT per-comment timestamp, a 1904-based Mac epoch
//! - `File:ID3Size` → emitted by `ID3::ProcessID3`, not by AIFF.pm itself
//!
//! # File Structure
//!
//! ```text
//! [FORM][size: u32 BE][AIFF|AIFC]
//! [chunk id: 4][size: u32 BE][payload, padded to an even length]
//! [chunk id: 4][size: u32 BE][payload, padded to an even length]
//! ...
//! ```
//!
//! The pad byte is part of the container, not of the chunk: a chunk declaring
//! an odd size is followed by one filler byte that the next chunk header sits
//! after. Skipping only the declared size desynchronises the whole walk, so
//! every advance below rounds up to an even length.
//!
//! # References
//!
//! - AIFF Spec: <http://www-mmsp.ece.mcgill.ca/Documents/AudioFormats/AIFF/AIFF.html>
//! - ExifTool Source: `lib/Image/ExifTool/AIFF.pm`

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{PrintConv, find_table};
use crate::parsers::audio::mp3::{parse_id3v2_frames, parse_id3v2_header};
use encoding_rs::MACINTOSH;

/// IFF container signature shared by AIFF, AIFC and (behind an `AT&T` prefix)
/// DjVu.
const FORM_SIGNATURE: &[u8] = b"FORM";

/// Seconds between the Mac epoch (1904-01-01) and the Unix epoch (1970-01-01).
///
/// AIFF.pm spells this `(66 * 365 + 17) * 24 * 3600` -- 66 years plus the 17
/// leap days in between.
const MAC_UNIX_EPOCH_DELTA: i64 = 2_082_844_800;

/// Consecutive zero-length unknown chunks after which ExifTool gives up.
///
/// AIFF.pm:258-261 reads `next if ++$n < 100`, but `$n` is *also* incremented
/// by the enclosing `for` loop that `next` jumps to, so it advances by two per
/// empty chunk and the scan actually aborts on the 51st. Reproducing the
/// threshold rather than the expression keeps the stopping point identical
/// without importing the Perl loop's quirk.
const EMPTY_CHUNK_LIMIT: usize = 51;

/// AIFF parser
pub struct AiffParser;

/// Parses metadata from an AIFF or AIFC file.
///
/// This is a convenience wrapper that creates an AiffParser instance and calls parse().
///
/// # Arguments
///
/// * `reader` - File reader providing access to the AIFF file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_aiff_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = AiffParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

impl FormatParser for AiffParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // AIFF.pm:191 -- 12 readable bytes are the minimum ProcessAIFF accepts.
        if reader.size() < 12 {
            return Err(ExifToolError::parse_error("File too small to be AIFF"));
        }

        let header = reader.read(0, 12)?;
        let (Some(signature), Some(form_type)) = (header.get(0..4), header.get(8..12)) else {
            return Err(ExifToolError::parse_error("Truncated AIFF header"));
        };
        // AIFF.pm:209 -- `/^FORM....(AIF(F|C))/s`. The four bytes between are
        // the FORM length, which ProcessAIFF never consults: it walks chunks to
        // end of file regardless of what the wrapper claims.
        if signature != FORM_SIGNATURE || !matches!(form_type, b"AIFF" | b"AIFC") {
            return Err(ExifToolError::parse_error(format!(
                "Invalid AIFF signature: expected FORM....AIF[FC], found {:?}{:?}",
                signature, form_type
            )));
        }

        let mut metadata = MetadataMap::with_capacity(16);
        walk_chunks(reader, 12, reader.size(), &mut metadata);
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::AIFF)
    }
}

/// Walk the flat IFF chunk sequence (AIFF.pm:220-270).
///
/// Reads to end of file rather than to the end of the FORM wrapper, because
/// that is what ExifTool does -- a FORM length that undercounts the file must
/// not hide trailing chunks, and one that overcounts must not read past EOF.
///
/// Every failure path stops the walk instead of propagating: a chunk table that
/// runs off the end of a truncated file still leaves the tags read so far
/// valid, which is ExifTool's behaviour (it warns and returns what it has).
fn walk_chunks(reader: &dyn FileReader, start: u64, end: u64, metadata: &mut MetadataMap) {
    let mut offset = start;
    let mut empty_unknown = 0usize;

    while offset.saturating_add(8) <= end {
        let Ok(header) = reader.read(offset, 8) else {
            break;
        };
        let (Some(id), Some(size_bytes)) = (header.get(0..4), header.get(4..8)) else {
            break;
        };
        let (Ok(chunk_id), Ok(size_bytes)) =
            (<[u8; 4]>::try_from(id), <[u8; 4]>::try_from(size_bytes))
        else {
            break;
        };
        let size = u64::from(u32::from_be_bytes(size_bytes));

        let payload = offset + 8;
        // AIFF.pm:227 -- `my $len2 = $len + ($len & 0x01)`.
        let Some(next) = payload.checked_add(size + (size & 1)) else {
            break;
        };

        if is_known_chunk(&chunk_id) {
            // AIFF.pm:248 -- `$raf->Read($buff, $len2) >= $len or $err=1, last`.
            // The pad byte may be missing at end of file, but the declared
            // payload may not be.
            if payload.saturating_add(size) > end {
                break;
            }
            let want = (next - payload).min(end - payload) as usize;
            let Ok(data) = reader.read(payload, want) else {
                break;
            };
            handle_chunk(&chunk_id, data, size as usize, metadata);
            empty_unknown = 0;
        } else if size == 0 {
            empty_unknown += 1;
            if empty_unknown >= EMPTY_CHUNK_LIMIT {
                break;
            }
        } else {
            empty_unknown = 0;
        }

        offset = next;
    }
}

/// Chunks AIFF.pm defines a tag for, and therefore reads rather than seeks past.
fn is_known_chunk(chunk_id: &[u8; 4]) -> bool {
    matches!(
        chunk_id,
        b"COMM" | b"COMT" | b"NAME" | b"AUTH" | b"(c) " | b"ANNO" | b"ID3 "
    )
}

/// Dispatch one chunk payload.
///
/// `data` is the padded read; `declared` is the chunk's own length field, which
/// is what ExifTool passes as `Size` to a SubDirectory and therefore what bounds
/// the fields inside it. The two differ by the pad byte, and the difference is
/// load-bearing: a text chunk keeps the padded buffer (ExifTool strips trailing
/// NULs from it, which is how the pad byte disappears) while a binary
/// sub-directory must not see it as a field byte.
fn handle_chunk(chunk_id: &[u8; 4], data: &[u8], declared: usize, metadata: &mut MetadataMap) {
    let dir_len = declared.min(data.len());
    match chunk_id {
        b"COMM" => parse_common(&data[..dir_len], metadata),
        b"COMT" => parse_comment(&data[..dir_len], metadata),
        b"ID3 " => parse_id3(&data[..dir_len], metadata),
        // AIFF.pm:250 -- `$buff =~ s/\0+$//` for every non-SubDirectory,
        // non-Binary tag, applied to the padded buffer.
        b"NAME" => insert_text("AIFF:Name", data, metadata),
        b"AUTH" => insert_text("AIFF:Author", data, metadata),
        b"(c) " => insert_text("AIFF:Copyright", data, metadata),
        b"ANNO" => insert_text("AIFF:Annotation", data, metadata),
        _ => {}
    }
}

/// COMM -- `Image::ExifTool::AIFF::Common`, a `ProcessBinaryData` table whose
/// FORMAT is `int16u`, so a tag at index N sits at byte offset 2N.
///
/// ExifTool extracts a field only when the whole field fits inside the declared
/// chunk length (`ReadValue` returns undef once the remaining count drops below
/// one), so each read below is gated on the same bound rather than on however
/// many bytes happened to be readable.
fn parse_common(data: &[u8], metadata: &mut MetadataMap) {
    if let Some(bytes) = data.get(0..2) {
        metadata.insert(
            "AIFF:NumChannels",
            TagValue::new_integer(i64::from(u16::from_be_bytes([bytes[0], bytes[1]]))),
        );
    }
    if let Some(bytes) = data.get(2..6) {
        metadata.insert(
            "AIFF:NumSampleFrames",
            TagValue::new_integer(i64::from(u32::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ]))),
        );
    }
    if let Some(bytes) = data.get(6..8) {
        metadata.insert(
            "AIFF:SampleSize",
            TagValue::new_integer(i64::from(u16::from_be_bytes([bytes[0], bytes[1]]))),
        );
    }
    if let Some(bytes) = data.get(8..18)
        && let Some(rate) = decode_extended(bytes)
        // ExifTool hands the extended float straight to Perl, which prints an
        // integral value without a decimal point -- `22050`, not `22050.0`. A
        // non-integral rate would need Perl's `%.15g` stringification to match,
        // and guessing a Rust float format instead would put a wrong string
        // under a real tag name, so it is omitted. No sample rate in the wild
        // is fractional: the 80-bit encoding exists precisely to make them
        // exact.
        && rate.fract() == 0.0
        && rate.abs() < 9.007_199_254_740_992e15
    {
        metadata.insert("AIFF:SampleRate", TagValue::new_integer(rate as i64));
    }
    // AIFC only: `Format => 'string[4]'` at index 9 (byte 18). Requiring all
    // four bytes departs from ExifTool, which would shorten the count and
    // report a truncated code as `Unknown (xx)`; a partial four-character
    // identifier is not a value worth publishing.
    if let Some(bytes) = data.get(18..22) {
        // `string` truncates at the first NUL (ExifTool.pm:6311).
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let code = String::from_utf8_lossy(&bytes[..end]).to_string();
        if !code.is_empty() {
            metadata.insert(
                "AIFF:CompressionType",
                TagValue::new_string(compression_type_name(&code)),
            );
        }
    }
}

/// The `CompressionType` PrintConv, read from the transcribed
/// `Image::ExifTool::AIFF::Common` table so the map has exactly one home.
///
/// An unlisted code renders as `Unknown (val)`, which is ExifTool's fallback for
/// a hash PrintConv miss (ExifTool.pm:3633).
fn compression_type_name(code: &str) -> String {
    let mapped = find_table("AIFF", "Common")
        .and_then(|table| table.fields.iter().find(|f| f.name == "CompressionType"))
        .and_then(|field| match field.print_conv {
            PrintConv::StrEnum(entries) => entries
                .iter()
                .find(|(key, _)| *key == code)
                .map(|(_, name)| (*name).to_string()),
            _ => None,
        });
    mapped.unwrap_or_else(|| format!("Unknown ({code})"))
}

/// COMT -- `Image::ExifTool::AIFF::ProcessComment` (AIFF.pm:155-178).
///
/// Layout: `int16u numComments`, then per comment `int32u timeStamp`,
/// `int16u markerID`, `int16u textSize`, `textSize` bytes of MacRoman text
/// padded to an even length.
///
/// The two `last if` guards below are placed exactly where ExifTool puts them:
/// the header bound is checked before the timestamp is emitted and the text
/// bound after, so a record whose text overruns the chunk still contributes its
/// CommentTime. Getting that order wrong would silently drop a tag ExifTool
/// reports on a truncated file.
fn parse_comment(data: &[u8], metadata: &mut MetadataMap) {
    // AIFF.pm:161 -- `return 0 unless $dirLen > 2`.
    if data.len() <= 2 {
        return;
    }
    let Some(count_bytes) = data.get(0..2) else {
        return;
    };
    let num_comments = u16::from_be_bytes([count_bytes[0], count_bytes[1]]);
    let mut pos = 2usize;

    for _ in 0..num_comments {
        // AIFF.pm:167 -- `last if $pos + 8 > $dirLen`.
        let Some(record) = pos.checked_add(8).and_then(|end| data.get(pos..end)) else {
            break;
        };
        let time = u32::from_be_bytes([record[0], record[1], record[2], record[3]]);
        let marker_id = u16::from_be_bytes([record[4], record[5]]);
        let text_size = usize::from(u16::from_be_bytes([record[6], record[7]]));

        if let Some(formatted) = format_mac_time(i64::from(time)) {
            metadata.insert("AIFF:CommentTime", TagValue::new_string(formatted));
        }
        // AIFF.pm:170 -- `HandleTag(1, $markerID) if $markerID`.
        if marker_id != 0 {
            metadata.insert("AIFF:MarkerID", TagValue::new_integer(i64::from(marker_id)));
        }

        pos += 8;
        // AIFF.pm:172 -- `last if $pos + $size > $dirLen`, checked only after
        // the timestamp above has already been handed over.
        let Some(text) = pos
            .checked_add(text_size)
            .and_then(|end| data.get(pos..end))
        else {
            break;
        };
        metadata.insert("AIFF:Comment", TagValue::new_string(decode_macroman(text)));

        // A second comment overwrites the first: ExifTool's FoundTag moves the
        // existing same-priority tag aside to `Comment (1)` and gives the base
        // name to the newcomer (ExifTool.pm:9563-9578), so its default output
        // shows the last one.
        pos += text_size + (text_size & 1);
    }
}

/// `ID3 ` -- handed to `ID3::ProcessID3` by AIFF.pm's SubDirectory.
///
/// Deliberately not routed through mp3.rs's `parse_id3v2` wrapper: that adds
/// `MP3:ID3Version` and `ID3TagSize`, which ExifTool emits for MP3 files and
/// not for an AIFF carrying an ID3 chunk.
fn parse_id3(data: &[u8], metadata: &mut MetadataMap) {
    let Some(header_bytes) = data.get(0..10) else {
        return;
    };
    let Ok((_, header)) = parse_id3v2_header(header_bytes) else {
        return;
    };

    let frames_len = header.size as usize;
    let Some(frames) = data.get(10..10 + frames_len) else {
        // ID3.pm:1463-1466 warns 'Truncated ID3 data' and extracts nothing.
        return;
    };

    // ID3.pm:1496 -- `$id3Len += length($hBuff) + 10`, i.e. the synchsafe frame
    // length plus the header, not the size of the chunk carrying it. The two
    // agree whenever the chunk is sized to its contents; only the former is
    // what ExifTool reports.
    metadata.insert(
        "File:ID3Size",
        TagValue::new_integer(frames_len as i64 + 10),
    );

    let _ = parse_id3v2_frames(frames, header.version, metadata);
}

/// Store a text chunk after stripping the trailing NULs ExifTool removes
/// (AIFF.pm:250), which is also how the container's pad byte disappears.
fn insert_text(key: &str, data: &[u8], metadata: &mut MetadataMap) {
    let end = data
        .iter()
        .rposition(|&b| b != 0)
        .map_or(0, |last| last + 1);
    let text = decode_macroman(&data[..end]);
    if !text.is_empty() {
        metadata.insert(key, TagValue::new_string(text));
    }
}

/// `$self->Decode($val, "MacRoman")`, the ValueConv on every AIFF text tag.
///
/// `encoding_rs::MACINTOSH` is the WHATWG `macintosh` index. It was checked
/// byte-for-byte against `Charset/MacRoman.pm` over all 256 values and agrees
/// everywhere, including the entries where Mac Roman implementations usually
/// part company -- 0xBD (U+03A9 Ω, not the U+2126 ohm sign Apple's ROMAN.TXT
/// lists), 0xDB (U+20AC €, the Mac OS 8.5 replacement for the old currency
/// sign) and 0xF0 (U+F8FF, the private-use Apple logo).
/// `macroman_decoder_matches_exiftool` below pins that set. Mac Roman defines
/// every byte, so the decode is total and never yields a replacement character.
fn decode_macroman(bytes: &[u8]) -> String {
    let (decoded, _, _) = MACINTOSH.decode(bytes);
    decoded.into_owned()
}

/// Decode an 80-bit IEEE 754 extended float, big-endian.
///
/// This is `GetExtended` (Writer.pl:4498-4507):
///
/// ```text
/// my $exp = Get16u($dataPt, $pos);
/// my $sig = Get64u($dataPt, $pos + 2);
/// my $sign = $exp & 0x8000 ? -1 : 1;
/// $exp = ($exp & 0x7fff) - 16383 - 63; # (-63 to fractionalize significand)
/// return $sign * $sig * 2 ** $exp;
/// ```
///
/// Note there is no implicit leading bit: the 64-bit significand carries its
/// own integer bit, which is why the bias is `16383 + 63` rather than `16383`.
///
/// Returns `None` for the `0x7FFF` exponent field. ExifTool would compute
/// `$sig * 2**16322` there and hand back an infinity or an astronomically large
/// float; a sample rate of that magnitude is not a value worth publishing under
/// a real tag name, so the tag is dropped instead.
fn decode_extended(bytes: &[u8]) -> Option<f64> {
    let raw: [u8; 10] = bytes.try_into().ok()?;
    let exponent_field = u32::from(u16::from_be_bytes([raw[0], raw[1]]));
    let significand = u64::from_be_bytes([
        raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8], raw[9],
    ]);

    if exponent_field & 0x7fff == 0x7fff {
        return None;
    }

    let exponent = (exponent_field & 0x7fff) as i32 - 16383 - 63;
    let magnitude = significand as f64 * 2f64.powi(exponent);
    Some(if exponent_field & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    })
}

/// `ConvertUnixTime($val - ((66 * 365 + 17) * 24 * 3600))`, the ValueConv AIFF.pm
/// shares between CommentTime and FormatVersionTime, followed by the default
/// `ConvertDateTime` which is the identity when no `-d` format is set.
///
/// `ConvertUnixTime` is called without its `$isLocal` argument, so it uses
/// `gmtime`: the printed time is UTC, not the reader's local time.
fn format_mac_time(mac_seconds: i64) -> Option<String> {
    let unix = mac_seconds.checked_sub(MAC_UNIX_EPOCH_DELTA)?;
    let dt = chrono::DateTime::from_timestamp(unix, 0)?;
    Some(dt.format("%Y:%m:%d %H:%M:%S").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{PINNED_CORPUS_ROOT, TestReader, pinned_corpus_available};

    /// Build a `FORM....AIF[FC]` wrapper around pre-built chunk bytes.
    fn aiff_file(form_type: &[u8; 4], chunks: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(12 + chunks.len());
        data.extend_from_slice(b"FORM");
        data.extend_from_slice(&((4 + chunks.len()) as u32).to_be_bytes());
        data.extend_from_slice(form_type);
        data.extend_from_slice(chunks);
        data
    }

    /// One IFF chunk, padded to an even length like the container requires.
    fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + payload.len() + 1);
        out.extend_from_slice(id);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    #[test]
    fn rejects_non_aiff_containers() {
        for header in [
            &b"RIFF\x00\x00\x00\x10WAVE"[..],
            &b"AT&TFORM\x00\x00\x00\x10"[..],
        ] {
            let reader = TestReader::from_slice(header);
            assert!(AiffParser.parse(&reader).is_err());
        }
    }

    #[test]
    fn rejects_a_file_shorter_than_the_header() {
        let reader = TestReader::from_slice(b"FORM\x00\x00");
        assert!(AiffParser.parse(&reader).is_err());
    }

    #[test]
    fn truncated_and_malformed_chunks_do_not_panic() {
        // A chunk header claiming far more payload than the file holds.
        let mut data = aiff_file(b"AIFF", b"");
        data.extend_from_slice(b"COMM");
        data.extend_from_slice(&0xffff_fff0u32.to_be_bytes());
        data.extend_from_slice(b"\x00\x01");
        let reader = TestReader::from_slice(&data);
        assert!(AiffParser.parse(&reader).unwrap().is_empty());

        // A COMT whose declared comment count outruns the chunk, and a COMM cut
        // off mid-field. Neither may panic, and neither may invent a value.
        let chunks = [
            chunk(b"COMT", &[0x00, 0x09, 0x00, 0x00, 0x00, 0x01]),
            chunk(b"COMM", &[0x00, 0x02, 0x00, 0x00]),
        ]
        .concat();
        let data = aiff_file(b"AIFF", &chunks);
        let reader = TestReader::from_slice(&data);
        let metadata = AiffParser.parse(&reader).unwrap();
        assert_eq!(metadata.get_string("AIFF:CommentTime"), None);
        assert!(metadata.get("AIFF:NumChannels").is_some());
        assert_eq!(metadata.get("AIFF:NumSampleFrames"), None);

        // Every truncation of a real-shaped file must also be survivable.
        let full = aiff_file(
            b"AIFF",
            &[
                chunk(b"NAME", b"odd length!"),
                chunk(b"COMM", &[0x00, 0x01, 0x00, 0x00, 0x2d, 0x22, 0x00, 0x08]),
            ]
            .concat(),
        );
        for len in 0..full.len() {
            let reader = TestReader::from_slice(&full[..len]);
            let _ = AiffParser.parse(&reader);
        }
    }

    #[test]
    fn honours_the_pad_byte_after_an_odd_sized_chunk() {
        // "Phil Harvey" is 11 bytes. Without the pad byte the following COMM
        // header would be read one byte early and every field after it would be
        // garbage -- this is the failure mode the padding rule exists to stop.
        let chunks = [
            chunk(b"AUTH", b"Phil Harvey"),
            chunk(b"ANNO", b"odd"),
            chunk(
                b"COMM",
                &[
                    0x00, 0x01, // NumChannels
                    0x00, 0x00, 0x2d, 0x22, // NumSampleFrames = 11554
                    0x00, 0x08, // SampleSize
                    0x40, 0x0d, 0xac, 0x44, 0, 0, 0, 0, 0, 0, // SampleRate = 22050
                ],
            ),
        ]
        .concat();
        let data = aiff_file(b"AIFF", &chunks);
        let reader = TestReader::from_slice(&data);
        let metadata = AiffParser.parse(&reader).unwrap();

        assert_eq!(metadata.get_string("AIFF:Author"), Some("Phil Harvey"));
        assert_eq!(metadata.get_string("AIFF:Annotation"), Some("odd"));
        assert_eq!(
            metadata.get("AIFF:NumChannels"),
            Some(&TagValue::new_integer(1))
        );
        assert_eq!(
            metadata.get("AIFF:NumSampleFrames"),
            Some(&TagValue::new_integer(11554))
        );
        assert_eq!(
            metadata.get("AIFF:SampleSize"),
            Some(&TagValue::new_integer(8))
        );
        assert_eq!(
            metadata.get("AIFF:SampleRate"),
            Some(&TagValue::new_integer(22050))
        );
    }

    #[test]
    fn decodes_eighty_bit_extended_floats() {
        // The COMM sample rate of the pinned AIFF.aif sample.
        let sample = [0x40, 0x0d, 0xac, 0x44, 0, 0, 0, 0, 0, 0];
        assert_eq!(decode_extended(&sample), Some(22050.0));

        // A true zero: exponent field and significand both clear.
        assert_eq!(decode_extended(&[0u8; 10]), Some(0.0));

        // 44100 Hz, the other rate that actually occurs.
        let cd = [0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0];
        assert_eq!(decode_extended(&cd), Some(44100.0));

        // The sign bit is bit 15 of the exponent word.
        let negative = [0xc0, 0x0d, 0xac, 0x44, 0, 0, 0, 0, 0, 0];
        assert_eq!(decode_extended(&negative), Some(-22050.0));

        // 1.0 has an integer bit and nothing else: exponent field 16383.
        let one = [0x3f, 0xff, 0x80, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(decode_extended(&one), Some(1.0));

        // Inf/NaN is dropped rather than rendered.
        assert_eq!(
            decode_extended(&[0x7f, 0xff, 0x80, 0, 0, 0, 0, 0, 0, 0]),
            None
        );
        assert_eq!(decode_extended(&[0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0]), None);

        // Short input is not a float.
        assert_eq!(decode_extended(&[0x40, 0x0d]), None);
    }

    #[test]
    fn a_non_integral_sample_rate_is_omitted_rather_than_guessed() {
        // 22050.5: significand 22050.5 * 2^49, exponent field 16383 + 14.
        let mut payload = vec![0x00, 0x01, 0x00, 0x00, 0x2d, 0x22, 0x00, 0x08];
        payload.extend_from_slice(&[0x40, 0x0d]);
        payload.extend_from_slice(&((22050.5f64 * 2f64.powi(49)) as u64).to_be_bytes());
        let data = aiff_file(b"AIFF", &chunk(b"COMM", &payload));
        let reader = TestReader::from_slice(&data);
        let metadata = AiffParser.parse(&reader).unwrap();

        assert_eq!(metadata.get("AIFF:SampleRate"), None);
        // The rest of the chunk still comes through.
        assert_eq!(
            metadata.get("AIFF:NumChannels"),
            Some(&TagValue::new_integer(1))
        );
    }

    #[test]
    fn reads_aifc_compression_types_through_the_transcribed_print_conv() {
        let mut payload = vec![0x00, 0x01, 0x00, 0x00, 0x2d, 0x22, 0x00, 0x08];
        payload.extend_from_slice(&[0x40, 0x0d, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(b"sowt");
        let data = aiff_file(b"AIFC", &chunk(b"COMM", &payload));
        let reader = TestReader::from_slice(&data);
        let metadata = AiffParser.parse(&reader).unwrap();
        assert_eq!(
            metadata.get_string("AIFF:CompressionType"),
            Some("Little-endian, no compression")
        );

        // An unlisted code takes ExifTool's hash-PrintConv fallback.
        assert_eq!(compression_type_name("zzzz"), "Unknown (zzzz)");
        assert_eq!(compression_type_name("GSM "), "GSM");
    }

    #[test]
    fn comment_records_stop_at_the_chunk_boundary() {
        // Two declared comments but only one record's worth of bytes: the
        // second must be abandoned without touching the first's tags.
        let mut payload = vec![0x00, 0x02];
        payload.extend_from_slice(&3_161_568_526u32.to_be_bytes());
        payload.extend_from_slice(&7u16.to_be_bytes()); // markerID
        payload.extend_from_slice(&8u16.to_be_bytes());
        payload.extend_from_slice(b"ding.wav");
        let data = aiff_file(b"AIFF", &chunk(b"COMT", &payload));
        let reader = TestReader::from_slice(&data);
        let metadata = AiffParser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get_string("AIFF:CommentTime"),
            Some("2004:03:08 05:28:46")
        );
        assert_eq!(
            metadata.get("AIFF:MarkerID"),
            Some(&TagValue::new_integer(7))
        );
        assert_eq!(metadata.get_string("AIFF:Comment"), Some("ding.wav"));
    }

    #[test]
    fn a_zero_marker_id_is_not_emitted() {
        let mut payload = vec![0x00, 0x01];
        payload.extend_from_slice(&3_161_568_526u32.to_be_bytes());
        payload.extend_from_slice(&0u16.to_be_bytes());
        payload.extend_from_slice(&8u16.to_be_bytes());
        payload.extend_from_slice(b"ding.wav");
        let data = aiff_file(b"AIFF", &chunk(b"COMT", &payload));
        let reader = TestReader::from_slice(&data);
        let metadata = AiffParser.parse(&reader).unwrap();
        assert_eq!(metadata.get("AIFF:MarkerID"), None);
    }

    #[test]
    fn mac_epoch_conversion_is_utc() {
        // AIFF.aif's COMT timestamp.
        assert_eq!(
            format_mac_time(3_161_568_526).as_deref(),
            Some("2004:03:08 05:28:46")
        );
        // The epoch itself.
        assert_eq!(
            format_mac_time(MAC_UNIX_EPOCH_DELTA).as_deref(),
            Some("1970:01:01 00:00:00")
        );
    }

    #[test]
    fn macroman_decoder_matches_exiftool() {
        // Spot checks against `Charset/MacRoman.pm`, quoted verbatim:
        //   0x8e => 0xe9, 0xa5 => 0x2022, 0xbd => 0x03a9, 0xc3 => 0x221a,
        //   0xd9 => 0x0178, 0xdb => 0x20ac, 0xde => 0xfb01, 0xf0 => 0xf8ff
        // plus 0xa2, which the table omits because Mac Roman and Unicode agree.
        let cases: [(u8, char); 9] = [
            (0x8e, '\u{00e9}'),
            (0xa2, '\u{00a2}'),
            (0xa5, '\u{2022}'),
            (0xbd, '\u{03a9}'),
            (0xc3, '\u{221a}'),
            (0xd9, '\u{0178}'),
            (0xdb, '\u{20ac}'),
            (0xde, '\u{fb01}'),
            (0xf0, '\u{f8ff}'),
        ];
        for (byte, expected) in cases {
            assert_eq!(
                decode_macroman(&[byte]),
                expected.to_string(),
                "MacRoman 0x{byte:02x}"
            );
        }
        // ASCII passes through untouched.
        assert_eq!(decode_macroman(b"ExifTool test AIFF"), "ExifTool test AIFF");
    }

    #[test]
    fn a_run_of_empty_unknown_chunks_ends_the_scan() {
        let mut chunks = Vec::new();
        for _ in 0..EMPTY_CHUNK_LIMIT {
            chunks.extend_from_slice(&chunk(b"junk", b""));
        }
        chunks.extend_from_slice(&chunk(b"NAME", b"unreachable"));
        let data = aiff_file(b"AIFF", &chunks);
        let reader = TestReader::from_slice(&data);
        assert_eq!(AiffParser.parse(&reader).unwrap().get("AIFF:Name"), None);

        // One fewer and the walk still reaches the NAME chunk.
        let mut chunks = Vec::new();
        for _ in 0..(EMPTY_CHUNK_LIMIT - 1) {
            chunks.extend_from_slice(&chunk(b"junk", b""));
        }
        chunks.extend_from_slice(&chunk(b"NAME", b"reachable"));
        let data = aiff_file(b"AIFF", &chunks);
        let reader = TestReader::from_slice(&data);
        assert_eq!(
            AiffParser.parse(&reader).unwrap().get_string("AIFF:Name"),
            Some("reachable")
        );
    }

    #[test]
    fn matches_exiftool_on_the_pinned_aiff_sample() {
        if !pinned_corpus_available() {
            return;
        }
        let path = format!("{PINNED_CORPUS_ROOT}/AIFF.aif");
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let reader = TestReader::new(bytes);
        let metadata = AiffParser
            .parse(&reader)
            .expect("pinned AIFF sample parses");

        // `exiftool -a -G1 -s /tmp/oxidex-exiftool-cache/combined-samples/AIFF.aif`
        // on the pinned 13.59.
        assert_eq!(
            metadata.get("AIFF:NumChannels"),
            Some(&TagValue::new_integer(1))
        );
        assert_eq!(
            metadata.get("AIFF:NumSampleFrames"),
            Some(&TagValue::new_integer(11554))
        );
        assert_eq!(
            metadata.get("AIFF:SampleSize"),
            Some(&TagValue::new_integer(8))
        );
        assert_eq!(
            metadata.get("AIFF:SampleRate"),
            Some(&TagValue::new_integer(22050))
        );
        assert_eq!(metadata.get_string("AIFF:Name"), Some("ExifTool test AIFF"));
        assert_eq!(metadata.get_string("AIFF:Author"), Some("Phil Harvey"));
        assert_eq!(metadata.get_string("AIFF:Comment"), Some("ding.wav"));
        assert_eq!(
            metadata.get_string("AIFF:CommentTime"),
            Some("2004:03:08 05:28:46")
        );
        // MarkerID is 0 in this sample, which ExifTool does not report.
        assert_eq!(metadata.get("AIFF:MarkerID"), None);
        assert_eq!(
            metadata.get("File:ID3Size"),
            Some(&TagValue::new_integer(172))
        );
        // The ID3v2.2 frames come from the shared reader.
        assert_eq!(metadata.get_string("ID3:Artist"), Some("the artist"));
        assert_eq!(metadata.get_string("ID3:Album"), Some("the album"));
        assert_eq!(metadata.get_string("ID3:Composer"), Some("Composer"));
        assert_eq!(metadata.get_string("ID3:Year"), Some("2006"));
        // Both go through PrintConvs that AIFF is the first format to exercise
        // here: the file stores them as `(18)` and `1`.
        assert_eq!(metadata.get_string("ID3:Genre"), Some("Techno"));
        assert_eq!(metadata.get_string("ID3:Compilation"), Some("Yes"));

        // Deliberately not checked with `assert_no_divergent_prefixed_duplicates`:
        // this file legitimately carries two different Comments, and ExifTool
        // reports both -- `[AIFF] Comment: ding.wav` from COMT and
        // `[ID3v2_2] Comment: comments` from the ID3 chunk. They are separate
        // tags in separate groups, not the alias-of-one-value pattern that
        // assertion exists to catch.
        assert_eq!(metadata.get_string("ID3:Comment"), Some("comments"));
    }

    #[test]
    fn derives_duration_from_the_pinned_sample() {
        if !pinned_corpus_available() {
            return;
        }
        let path = format!("{PINNED_CORPUS_ROOT}/AIFF.aif");
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let reader = TestReader::new(bytes);
        let mut metadata = AiffParser
            .parse(&reader)
            .expect("pinned AIFF sample parses");
        crate::composite::apply(&mut metadata);
        assert_eq!(metadata.get_string("Composite:Duration"), Some("0.52 s"));
    }
}
