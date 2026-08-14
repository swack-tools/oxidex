//! APE (Monkey's Audio) format parser
//!
//! Implements metadata extraction from APE audio files, parsing the MAC
//! header and the APEv1/APEv2 tag block.
//!
//! # File Structure
//!
//! ```text
//! [MAC descriptor]
//!   ├─ Signature: "MAC " (4 bytes)
//!   ├─ Version: int16u  (e.g. 3990 == 3.99)
//!   ├─ Descriptor length: int32u at offset 8   (v3.98+)
//!   └─ Header length: int32u at offset 12      (v3.98+)
//! [MAC header]  -- at <descriptor length>, <header length> bytes (v3.98+)
//! [APE frames]
//! [APE tag]
//!   ├─ Optional 32-byte header "APETAGEX"
//!   ├─ Tag items: <int32u value length><int32u flags><key>\0<value>
//!   └─ 32-byte footer "APETAGEX" (size field COVERS the footer)
//! ```
//!
//! # References
//!
//! - Monkey's Audio SDK
//! - <http://www.personal.uni-jena.de/~pfk/mpp/sv8/apetag.html>
//! - ExifTool Source: `lib/Image/ExifTool/APE.pm`

use crate::core::formatters::convert_duration;
use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;
use encoding_rs::UTF_8;

/// MAC file signature
const MAC_SIGNATURE: &[u8] = b"MAC ";

/// APE tag signature, used by both the optional header and the footer
const APE_TAG_SIGNATURE: &[u8] = b"APETAGEX";

/// Size of an APE tag header/footer block.
const APE_TAG_BLOCK_LEN: u64 = 32;

/// ID3v1 signature and fixed trailer length, checked so the APE footer scan
/// can skip past a trailing ID3v1 tag (`APE.pm:167-169`).
const ID3V1_SIGNATURE: &[u8] = b"TAG";
const ID3V1_TRAILER_LEN: u64 = 128;

/// Last MAC version that uses the pre-3.98 header layout.
const MAC_OLD_HEADER_MAX_VERSION: u16 = 3970;

/// Refuse to buffer an implausibly large tag block.
const MAX_APE_TAG_SIZE: u64 = 1_000_000;

/// Cap on tag items, so a corrupt count cannot spin.
const MAX_APE_ITEMS: u32 = 1000;

/// Item flag bits selecting the value's type; `0b10` means binary.
const APE_ITEM_TYPE_MASK: u32 = 0x06;
const APE_ITEM_TYPE_BINARY: u32 = 0x02;

/// APE parser
pub struct ApeParser;

/// Parses metadata from an APE (Monkey's Audio) file.
///
/// This is a convenience wrapper that creates an ApeParser instance and calls parse().
///
/// # Arguments
///
/// * `reader` - File reader providing access to the APE file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_ape_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = ApeParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

impl FormatParser for ApeParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let file_size = reader.size();

        if file_size < APE_TAG_BLOCK_LEN {
            return Err(ExifToolError::parse_error("File too small to be APE"));
        }

        let descriptor = reader.read(0, 32)?;
        if &descriptor[0..4] != MAC_SIGNATURE {
            return Err(ExifToolError::parse_error(format!(
                "Invalid APE signature: expected {:?}, found {:?}",
                MAC_SIGNATURE,
                &descriptor[0..4]
            )));
        }

        let mut metadata = MetadataMap::with_capacity(24);

        parse_mac_audio_header(reader, descriptor, &mut metadata)?;
        parse_ape_trailer(reader, &mut metadata)?;

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::APE)
    }
}

/// Parses the MAC audio header.
///
/// Which layout applies depends on the version in the descriptor: 3.97 and
/// earlier inline the header right after the 4-byte signature, while 3.98+
/// place it at a descriptor-relative offset that must be read from the
/// descriptor itself. Reading 3.98+ fields at fixed offsets in the
/// descriptor -- the shape of the previous implementation -- lands in the
/// MD5/reserved area and yields zeros.
fn parse_mac_audio_header(
    reader: &dyn FileReader,
    descriptor: &[u8],
    metadata: &mut MetadataMap,
) -> Result<()> {
    let desc = EndianReader::little_endian(descriptor);
    let version = desc.u16_at(4).unwrap_or(0);

    if version <= MAC_OLD_HEADER_MAX_VERSION {
        // Old layout: fields are indexed from just past the signature.
        let header = &descriptor[4..];
        let h = EndianReader::little_endian(header);
        // ExifTool's ValueConv is $val / 1000, printed as e.g. 3.97.
        metadata.insert(
            "APE:APEVersion".to_string(),
            TagValue::new_string(format_version(h.u16_at(0).unwrap_or(0))),
        );
        insert_int(metadata, "CompressionLevel", h.u16_at(2).map(i64::from));
        insert_int(metadata, "Channels", h.u16_at(6).map(i64::from));
        insert_int(metadata, "SampleRate", h.u32_at(8).map(i64::from));
        insert_int(metadata, "TotalFrames", h.u32_at(20).map(i64::from));
        insert_int(metadata, "FinalFrameBlocks", h.u32_at(24).map(i64::from));
        return Ok(());
    }

    // New layout (3.98+): the descriptor tells us where the header lives.
    let descriptor_len = u64::from(desc.u32_at(8).unwrap_or(0));
    let header_len = u64::from(desc.u32_at(12).unwrap_or(0));
    // ExifTool rejects lengths with the high bit set rather than trusting them.
    if descriptor_len & 0x8000_0000 != 0 || header_len & 0x8000_0000 != 0 {
        return Ok(());
    }
    if header_len < 24 || descriptor_len.saturating_add(header_len) > reader.size() {
        return Ok(());
    }

    let header = reader.read(descriptor_len, header_len as usize)?;
    let h = EndianReader::little_endian(header);

    // ExifTool reports CompressionLevel as the raw code (1000/2000/...),
    // with no PrintConv, so it must not be mapped to a name here.
    let blocks_per_frame = h.u32_at(4);
    let final_frame_blocks = h.u32_at(8);
    let total_frames = h.u32_at(12);
    let sample_rate = h.u32_at(20);

    insert_int(metadata, "CompressionLevel", h.u16_at(0).map(i64::from));
    insert_int(metadata, "BlocksPerFrame", blocks_per_frame.map(i64::from));
    insert_int(
        metadata,
        "FinalFrameBlocks",
        final_frame_blocks.map(i64::from),
    );
    insert_int(metadata, "TotalFrames", total_frames.map(i64::from));
    insert_int(metadata, "BitsPerSample", h.u16_at(16).map(i64::from));
    insert_int(metadata, "Channels", h.u16_at(18).map(i64::from));
    insert_int(metadata, "SampleRate", sample_rate.map(i64::from));

    // Image::ExifTool::APE::Composite::Duration:
    // ((TotalFrames - 1) * BlocksPerFrame + FinalFrameBlocks) / SampleRate
    if let (Some(sample_rate), Some(total_frames), Some(blocks_per_frame), Some(final_frame_blocks)) = (
        sample_rate,
        total_frames,
        blocks_per_frame,
        final_frame_blocks,
    ) && sample_rate != 0
        && total_frames != 0
    {
        let samples = u64::from(total_frames - 1) * u64::from(blocks_per_frame)
            + u64::from(final_frame_blocks);
        metadata.insert(
            "APE:Duration".to_string(),
            TagValue::new_string(convert_duration(samples as f64 / f64::from(sample_rate))),
        );
    }

    Ok(())
}

fn insert_int(metadata: &mut MetadataMap, name: &str, value: Option<i64>) {
    if let Some(value) = value {
        metadata.insert(format!("APE:{}", name), TagValue::new_integer(value));
    }
}

/// Formats a MAC version code the way ExifTool's `$val / 1000` does.
fn format_version(version: u16) -> String {
    format!("{}", f64::from(version) / 1000.0)
}

/// Locates and parses the APE tag block at the end of the file.
///
/// Container-independent: it looks only at the *last* 32 bytes of whatever
/// `reader` wraps, the same way `APE::ProcessAPE` does in ExifTool --
/// `APE.pm`'s trailer scan never checks for a leading `MAC ` descriptor,
/// which is how ExifTool reads an APEv2 tag off the tail of an MP3
/// (`ID3::ProcessMP3`, `ID3.pm:1718-1721`) or, per `MPC.pm:111-113`
/// (`ProcessMPC`'s final `Image::ExifTool::APE::ProcessAPE($et, $dirInfo)`
/// call), a Musepack file. `mpc.rs` is exactly that second caller: it never
/// sees a `MAC ` descriptor at all, only the trailing tag this function
/// finds by walking backward from EOF.
pub(crate) fn parse_ape_trailer(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
    let file_size = reader.size();
    if file_size < APE_TAG_BLOCK_LEN {
        return Ok(());
    }

    // The footer sits 32 bytes before EOF -- unless a trailing ID3v1 tag
    // follows it, in which case it sits 32 bytes before *that* instead
    // (`APE.pm:167-169`: `my $footPos = -32; $footPos -= $$et{DoneID3} if
    // $$et{DoneID3} > 1`, where `DoneID3` is the ID3v1 trailer's own byte
    // length once `ID3::ProcessID3` has found one). `APE.mpc` is laid out
    // exactly `[audio][APE tag][ID3v1]`: without this adjustment the last 32
    // bytes are the ID3v1 tag's own tail, not `APETAGEX`, and the trailer is
    // silently missed.
    let id3v1_trailer_len = if file_size >= ID3V1_TRAILER_LEN {
        let tail = reader.read(file_size - ID3V1_TRAILER_LEN, ID3V1_TRAILER_LEN as usize)?;
        if tail.starts_with(ID3V1_SIGNATURE) {
            ID3V1_TRAILER_LEN
        } else {
            0
        }
    } else {
        0
    };
    let Some(footer_offset) = file_size.checked_sub(APE_TAG_BLOCK_LEN + id3v1_trailer_len) else {
        return Ok(());
    };

    // The footer is the LAST 32 bytes (of the audio+tag region), not the
    // first "APETAGEX" found in a trailing window: when a tag carries both
    // a header and a footer, the first match is the header, whose own
    // `size` field then points the data start 32 bytes too far and drops
    // the first item.
    let footer = reader.read(footer_offset, APE_TAG_BLOCK_LEN as usize)?;
    if &footer[0..8] != APE_TAG_SIGNATURE {
        return Ok(());
    }

    let f = EndianReader::little_endian(footer);
    // `size` counts the item data PLUS this 32-byte footer.
    let total_size = u64::from(f.u32_at(12).unwrap_or(0));
    let item_count = f.u32_at(16).unwrap_or(0);
    if total_size < APE_TAG_BLOCK_LEN || total_size > MAX_APE_TAG_SIZE {
        return Ok(());
    }
    let data_size = total_size - APE_TAG_BLOCK_LEN;
    let Some(data_start) = (footer_offset + APE_TAG_BLOCK_LEN).checked_sub(total_size) else {
        return Ok(());
    };

    let data = reader.read(data_start, data_size as usize)?;
    parse_ape_tag_items(data, item_count, metadata);

    Ok(())
}

/// Walks the `<length><flags><key>\0<value>` items of an APE tag block.
fn parse_ape_tag_items(data: &[u8], item_count: u32, metadata: &mut MetadataMap) {
    let mut pos = 0usize;
    for _ in 0..item_count.min(MAX_APE_ITEMS) {
        if pos + 8 > data.len() {
            break;
        }
        let r = EndianReader::little_endian(data);
        let value_len = r.u32_at(pos).unwrap_or(0) as usize;
        let flags = r.u32_at(pos + 4).unwrap_or(0);
        pos += 8;

        let Some(key_end) = data[pos..].iter().position(|&b| b == 0).map(|i| pos + i) else {
            break;
        };
        let (key, _, _) = UTF_8.decode(&data[pos..key_end]);
        pos = key_end + 1;

        let Some(value_end) = pos.checked_add(value_len).filter(|e| *e <= data.len()) else {
            break;
        };
        let mut value_bytes = &data[pos..value_end];
        pos = value_end;

        let name = make_tag_name(&key);

        if flags & APE_ITEM_TYPE_MASK == APE_ITEM_TYPE_BINARY {
            // Cover art items lead with a printable filename terminated by
            // a NUL; ExifTool splits that off into a "<tag> Desc" tag and
            // reports the remaining bytes as the binary value.
            if key.starts_with("Cover Art")
                && let Some(nul) = value_bytes.iter().position(|&b| b == 0)
                && value_bytes[..nul]
                    .iter()
                    .all(|&b| (0x20..=0x7e).contains(&b))
            {
                if nul > 0 {
                    let (desc, _, _) = UTF_8.decode(&value_bytes[..nul]);
                    metadata.insert(
                        format!("APE:{}", make_tag_name(&format!("{} Desc", key))),
                        TagValue::new_string(desc.to_string()),
                    );
                }
                value_bytes = &value_bytes[nul + 1..];
            }
            metadata.insert(
                format!("APE:{}", name),
                TagValue::Binary(value_bytes.to_vec()),
            );
        } else {
            let (value, _, _) = UTF_8.decode(value_bytes);
            metadata.insert(
                format!("APE:{}", name),
                TagValue::new_string(value.trim_end_matches('\0').to_string()),
            );
        }
    }
}

/// Derives a tag name from an APE item key.
///
/// Mirrors ExifTool's `APE::MakeTag`: lowercase the key, capitalize the
/// first letter, then drop every run of non-word characters while
/// capitalizing whatever follows it -- so `Media Jukebox: Date` becomes
/// `MediaJukeboxDate` and `Cover Art (front)` becomes `CoverArtFront`.
/// Underscores between alphanumerics are treated the same way.
fn make_tag_name(key: &str) -> String {
    // Two APE keys have explicit names in ExifTool's table that the
    // generic rule alone would not produce a different result for, but
    // which are listed there for documentation; the generic rule already
    // yields ToolVersion / ToolName, so no special-casing is needed.
    let lowered = key.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut capitalize_next = true;
    let mut prev_alnum = false;

    for ch in lowered.chars() {
        let is_word = ch.is_alphanumeric() || ch == '_';
        if !is_word && ch != '-' {
            // Run of invalid characters: skip it, capitalize what follows.
            capitalize_next = true;
            prev_alnum = false;
            continue;
        }
        if ch == '_' {
            // `([a-z0-9])_([a-z])` -> drop the underscore, uppercase next.
            if prev_alnum {
                capitalize_next = true;
                continue;
            }
            out.push('_');
            prev_alnum = false;
            continue;
        }
        if capitalize_next {
            out.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(ch);
        }
        prev_alnum = ch.is_alphanumeric();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Renders a tag the way a reader would see it, so expectations can be
    /// written against ExifTool's printed strings regardless of whether the
    /// parser stored a string or an integer.
    fn text(metadata: &MetadataMap, key: &str) -> String {
        match metadata.get(key) {
            Some(TagValue::String(s)) => s.clone(),
            Some(TagValue::Integer(i)) => i.to_string(),
            Some(other) => panic!("{key} is not a printable scalar: {other:?}"),
            None => panic!("missing tag {key}"),
        }
    }

    /// Builds a v3.99 APE file: 32-byte descriptor, 24-byte header at
    /// offset 52, then an APE tag block terminated by a footer.
    fn build_ape(items: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let mut data = vec![0u8; 52];
        data[0..4].copy_from_slice(b"MAC ");
        data[4..6].copy_from_slice(&3990u16.to_le_bytes());
        data[8..12].copy_from_slice(&52u32.to_le_bytes()); // descriptor length
        data[12..16].copy_from_slice(&24u32.to_le_bytes()); // header length

        let mut header = vec![0u8; 24];
        header[0..2].copy_from_slice(&3000u16.to_le_bytes()); // CompressionLevel
        header[4..8].copy_from_slice(&73728u32.to_le_bytes()); // BlocksPerFrame
        header[8..12].copy_from_slice(&42662u32.to_le_bytes()); // FinalFrameBlocks
        header[12..16].copy_from_slice(&2u32.to_le_bytes()); // TotalFrames
        header[16..18].copy_from_slice(&16u16.to_le_bytes()); // BitsPerSample
        header[18..20].copy_from_slice(&2u16.to_le_bytes()); // Channels
        header[20..24].copy_from_slice(&44100u32.to_le_bytes()); // SampleRate
        data.extend_from_slice(&header);

        let mut tag = Vec::new();
        for (key, flags, value) in items {
            tag.extend_from_slice(&(value.len() as u32).to_le_bytes());
            tag.extend_from_slice(&flags.to_le_bytes());
            tag.extend_from_slice(key.as_bytes());
            tag.push(0);
            tag.extend_from_slice(value);
        }

        let total_size = tag.len() as u32 + 32;
        data.extend_from_slice(&tag);
        data.extend_from_slice(b"APETAGEX");
        data.extend_from_slice(&2000u32.to_le_bytes());
        data.extend_from_slice(&total_size.to_le_bytes());
        data.extend_from_slice(&(items.len() as u32).to_le_bytes());
        data.extend_from_slice(&0x4000_0000u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 8]);
        data
    }

    #[test]
    fn reads_new_layout_audio_header() {
        let reader = TestReader::new(build_ape(&[]));
        let metadata = ApeParser.parse(&reader).unwrap();

        // ExifTool reports the raw compression code, not a friendly name.
        assert_eq!(
            metadata.get("APE:CompressionLevel").unwrap().as_integer(),
            Some(3000)
        );
        assert_eq!(
            metadata.get("APE:BlocksPerFrame").unwrap().as_integer(),
            Some(73728)
        );
        assert_eq!(
            metadata.get("APE:FinalFrameBlocks").unwrap().as_integer(),
            Some(42662)
        );
        assert_eq!(
            metadata.get("APE:TotalFrames").unwrap().as_integer(),
            Some(2)
        );
        assert_eq!(
            metadata.get("APE:BitsPerSample").unwrap().as_integer(),
            Some(16)
        );
        assert_eq!(metadata.get("APE:Channels").unwrap().as_integer(), Some(2));
        assert_eq!(
            metadata.get("APE:SampleRate").unwrap().as_integer(),
            Some(44100)
        );
    }

    #[test]
    fn reads_tag_items_from_a_footer_only_tag() {
        let reader = TestReader::new(build_ape(&[
            ("Artist", 0, b"Kraftwerk".as_slice()),
            ("Tool Name", 0, b"Media Center".as_slice()),
            ("Media Jukebox: Date", 0, b"38353".as_slice()),
        ]));
        let metadata = ApeParser.parse(&reader).unwrap();

        assert_eq!(text(&metadata, "APE:Artist"), "Kraftwerk");
        assert_eq!(text(&metadata, "APE:ToolName"), "Media Center");
        assert_eq!(text(&metadata, "APE:MediaJukeboxDate"), "38353");
    }

    #[test]
    fn splits_cover_art_description_from_binary_value() {
        let mut value = b"C:\\art.jpg".to_vec();
        value.push(0);
        value.extend_from_slice(&[0xff, 0xd8, 0xff, 0xe0]);
        let reader = TestReader::new(build_ape(&[("Cover Art (front)", 0x02, &value)]));
        let metadata = ApeParser.parse(&reader).unwrap();

        assert_eq!(text(&metadata, "APE:CoverArtFrontDesc"), "C:\\art.jpg");
        match metadata.get("APE:CoverArtFront").unwrap() {
            TagValue::Binary(bytes) => assert_eq!(bytes.len(), 4),
            other => panic!("expected binary cover art, got {:?}", other),
        }
    }

    #[test]
    fn make_tag_name_matches_exiftool_rules() {
        assert_eq!(make_tag_name("Artist"), "Artist");
        assert_eq!(make_tag_name("Tool Version"), "ToolVersion");
        assert_eq!(make_tag_name("Media Jukebox: Date"), "MediaJukeboxDate");
        assert_eq!(make_tag_name("Cover Art (front)"), "CoverArtFront");
        assert_eq!(make_tag_name("Cover Art (front) Desc"), "CoverArtFrontDesc");
        assert_eq!(
            make_tag_name("replay_gain_track_gain"),
            "ReplayGainTrackGain"
        );
    }

    #[test]
    fn test_ape_signature_invalid() {
        let data = b"INVALID DATA XXXXXXXXXXXXXXXXXXXXXXX";
        let reader = TestReader::from_slice(data);
        let parser = ApeParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_ape_file_too_small() {
        let data = b"MAC ";
        let reader = TestReader::from_slice(data);
        let parser = ApeParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }
}
