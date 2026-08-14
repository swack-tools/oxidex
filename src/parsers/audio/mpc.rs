//! MPC (Musepack) audio format parser
//!
//! Implements metadata extraction from Musepack files: the leading/trailing
//! ID3 tags MPC shares with MP3, the MPC v7 bit-packed audio header, and the
//! trailing APE tag MPC shares with APE.
//!
//! # File Structure
//!
//! ```text
//! [ID3v2 tag - optional, at start]
//! [MP+ header - 32 bytes, v7 only]
//! [audio frames]
//! [APE tag - optional]
//! [ID3v1 tag - optional, last 128 bytes]
//! ```
//!
//! # ExifTool Compatibility
//!
//! `MPC.pm:79-116`'s `ProcessMPC`:
//!
//! ```text
//! 79  sub ProcessMPC($$)
//! 80  {
//! 81      my ($et, $dirInfo) = @_;
//! 83      # must first check for leading ID3 information
//! 84      unless ($$et{DoneID3}) {
//! 85          require Image::ExifTool::ID3;
//! 86          Image::ExifTool::ID3::ProcessID3($et, $dirInfo) and return 1;
//! 87      }
//! ...
//! 91      # check MPC signature
//! 92      $raf->Read($buff, 32) == 32 and $buff =~ /^MP\+(.)/s or return 0;
//! 93      my $vers = ord($1) & 0x0f;
//! ...
//! 96      # extract audio information (currently only from version 7 MPC files)
//! 97      if ($vers == 0x07) {
//! ...
//! 104         my $tagTablePtr = GetTagTable('Image::ExifTool::MPC::Main');
//! ...
//! 111     # process APE trailer if it exists
//! 112     require Image::ExifTool::APE;
//! 113     Image::ExifTool::APE::ProcessAPE($et, $dirInfo);
//! 116 }
//! ```
//!
//! `ID3.pm:1691-1697`'s `@audioFormats` dispatch (`ID3::ProcessID3`, called
//! first per `MPC.pm:84-87`) is what actually reads the MP+ header: it
//! processes the leading ID3v2 tag's own frames, then re-enters
//! `MPC::ProcessMPC` with the file positioned right after the ID3v2 block --
//! which is why `APE.mpc`'s real MP+ signature sits at byte 263, behind a
//! 263-byte ID3v2.2 tag, rather than at offset 0. `parse()` below mirrors
//! that ordering directly rather than reusing `ID3::ProcessID3`'s recursive
//! dispatch, since MPC is this crate's only caller that needs it.
//!
//! `MPC::Main`'s bit-packed fields (`MPC.pm:20-71`) are read by
//! `FLAC::ProcessBitStream` (`FLAC.pm:158-224`) under `SetByteOrder('II')`
//! (`MPC.pm:98`): `'II'` numbers bit 0 as each byte's least-significant bit,
//! so `'Bit080-081'` is byte 10's bits 0-1, `'Bit084-087'` is byte 10's bits
//! 4-7, and so on -- see [`parse_mpc_header`] for the field-by-field mapping,
//! verified against `APE.mpc`'s actual header bytes.
//!
//! # References
//!
//! - ExifTool Source: `lib/Image/ExifTool/MPC.pm`, `lib/Image/ExifTool/ID3.pm`

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

use super::ape::parse_ape_trailer;
use super::mp3::{parse_id3v1, parse_id3v2_frames, parse_id3v2_header};

const ID3V2_SIGNATURE: &[u8] = b"ID3";
const ID3V1_SIGNATURE: &[u8] = b"TAG";
const MPC_SIGNATURE: &[u8] = b"MP+";
const MPC_HEADER_LEN: u64 = 32;

/// Quality codes, `MPC.pm:38-52`.
const QUALITY: &[(u8, &str)] = &[
    (1, "Unstable/Experimental"),
    (5, "0"),
    (6, "1"),
    (7, "2 (Telephone)"),
    (8, "3 (Thumb)"),
    (9, "4 (Radio)"),
    (10, "5 (Standard)"),
    (11, "6 (Xtreme)"),
    (12, "7 (Insane)"),
    (13, "8 (BrainDead)"),
    (14, "9"),
    (15, "10"),
];

/// Sample rate codes, `MPC.pm:29-35`.
const SAMPLE_RATE: [u32; 4] = [44100, 48000, 37800, 32000];

/// MPC parser
pub struct MpcParser;

/// Parses metadata from an MPC (Musepack) file.
///
/// This is a convenience wrapper that creates an MpcParser instance and calls parse().
pub fn parse_mpc_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = MpcParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

impl FormatParser for MpcParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let file_size = reader.size();
        let mut metadata = MetadataMap::with_capacity(32);

        // Leading ID3v2 tag (MPC.pm:83-87 defers to ID3::ProcessID3, which
        // reads its own frames before the audio-format dispatch loop
        // re-enters ProcessMPC positioned right after this block --
        // ID3.pm:1679-1698).
        let mut mpc_header_start = 0u64;
        let mut id3_size = 0u64;
        let mut pending_id3v2: Option<(Vec<u8>, u8)> = None; // (frame bytes, version)

        if file_size >= 10 {
            let header = reader.read(0, 10)?;
            if &header[0..3] == ID3V2_SIGNATURE {
                let (_, id3v2_header) = parse_id3v2_header(header).map_err(|e| {
                    ExifToolError::parse_error(format!("Failed to parse ID3v2 header: {:?}", e))
                })?;
                mpc_header_start = 10 + u64::from(id3v2_header.size);
                id3_size += mpc_header_start;
                let frames_size = id3v2_header.size as usize;
                if frames_size > 0 {
                    let frames_data = reader.read(10, frames_size)?;
                    pending_id3v2 = Some((frames_data.to_vec(), id3v2_header.version));
                }
            }
        }

        // MP+ v7 bit header (MPC.pm:91-109).
        if mpc_header_start + MPC_HEADER_LEN <= file_size {
            let buff = reader.read(mpc_header_start, MPC_HEADER_LEN as usize)?;
            if &buff[0..3] == MPC_SIGNATURE {
                let version = buff[3] & 0x0f;
                if version == 7 {
                    parse_mpc_header(buff, &mut metadata);
                }
            }
        }

        // APE trailer, container-independent (MPC.pm:111-113).
        parse_ape_trailer(reader, &mut metadata)?;

        // `ID3Size` (ID3.pm:1606, `File:` group via the Extra table's
        // default GROUPS) is recorded before the ID3v2/ID3v1 tag emission
        // below, matching ProcessID3's own FoundTag order
        // (ID3.pm:1598-1624).
        let trailing_id3v1 = if file_size >= 128 {
            let id3v1_offset = file_size - 128;
            let id3v1_data = reader.read(id3v1_offset, 128)?;
            (&id3v1_data[0..3] == ID3V1_SIGNATURE).then(|| id3v1_data.to_vec())
        } else {
            None
        };
        if trailing_id3v1.is_some() {
            id3_size += 128;
        }
        if id3_size > 0 {
            metadata.insert("ID3Size", TagValue::new_integer(id3_size as i64));
        }

        if let Some((frames, version)) = pending_id3v2 {
            parse_id3v2_frames(&frames, version, &mut metadata)?;
        }
        if let Some(id3v1_data) = trailing_id3v1 {
            parse_id3v1(&id3v1_data, &mut metadata)?;
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::MPC)
    }
}

/// Parses the 32-byte MPC v7 bit header (`MPC::Main`, `MPC.pm:20-71`) under
/// `SetByteOrder('II')` (`MPC.pm:98`): `FLAC::ProcessBitStream`'s `'II'`
/// branch (`FLAC.pm:196-215`) numbers bit 0 as each byte's low bit, so a
/// field's byte index is `bit / 8` and its bit offset within that byte is
/// `bit % 8` in ordinary (LSB-first) order -- no cross-byte bit shuffling is
/// needed for any field this table declares, since none of them straddle a
/// byte except the two full 32-bit ints, which are already little-endian.
///
/// Verified field-by-field against `APE.mpc`'s real header bytes
/// (`t/images/APE.mpc`, offset 263): `TotalFrames=102`,
/// `SampleRate=44100 (code 0)`, `Quality=5 (Standard) (code 10)`,
/// `MaxBand=28`, all four ReplayGain fields `0`, `FastSeek=No`,
/// `Gapless=Yes`, `EncoderVersion=1.1.5` (raw byte `115`).
fn parse_mpc_header(buff: &[u8], metadata: &mut MetadataMap) {
    let r = EndianReader::little_endian(buff);

    // Bit032-063: TotalFrames
    if let Some(total_frames) = r.u32_at(4) {
        metadata.insert(
            "MPC:TotalFrames",
            TagValue::new_integer(i64::from(total_frames)),
        );
    }

    let byte10 = buff[10];
    // Bit080-081: SampleRate, 2 bits at byte 10 bits 0-1.
    let sample_rate_code = byte10 & 0x03;
    if let Some(&rate) = SAMPLE_RATE.get(sample_rate_code as usize) {
        metadata.insert("MPC:SampleRate", TagValue::new_integer(i64::from(rate)));
    }
    // Bit084-087: Quality, 4 bits at byte 10 bits 4-7.
    let quality_code = (byte10 >> 4) & 0x0f;
    if let Some(&(_, name)) = QUALITY.iter().find(|(code, _)| *code == quality_code) {
        metadata.insert("MPC:Quality", TagValue::new_string(name));
    }

    // Bit088-093: MaxBand, 6 bits at byte 11 bits 0-5.
    let max_band = buff[11] & 0x3f;
    metadata.insert("MPC:MaxBand", TagValue::new_integer(i64::from(max_band)));

    // Bit096-111 / Bit112-127 / Bit128-143 / Bit144-159: four raw 16-bit
    // ReplayGain fields, no PrintConv in MPC::Main.
    if let Some(v) = r.u16_at(12) {
        metadata.insert(
            "MPC:ReplayGainTrackPeak",
            TagValue::new_integer(i64::from(v)),
        );
    }
    if let Some(v) = r.u16_at(14) {
        metadata.insert(
            "MPC:ReplayGainTrackGain",
            TagValue::new_integer(i64::from(v)),
        );
    }
    if let Some(v) = r.u16_at(16) {
        metadata.insert(
            "MPC:ReplayGainAlbumPeak",
            TagValue::new_integer(i64::from(v)),
        );
    }
    if let Some(v) = r.u16_at(18) {
        metadata.insert(
            "MPC:ReplayGainAlbumGain",
            TagValue::new_integer(i64::from(v)),
        );
    }

    // Bit179: FastSeek, 1 bit at byte 22 bit 3.
    let fast_seek = (buff[22] >> 3) & 1;
    metadata.insert(
        "MPC:FastSeek",
        TagValue::new_string(if fast_seek != 0 { "Yes" } else { "No" }),
    );
    // Bit191: Gapless, 1 bit at byte 23 bit 7.
    let gapless = (buff[23] >> 7) & 1;
    metadata.insert(
        "MPC:Gapless",
        TagValue::new_string(if gapless != 0 { "Yes" } else { "No" }),
    );

    // Bit216-223: EncoderVersion, byte 27. ValueConv
    // `$val =~ s/(\d)(\d)(\d)$/$1.$2.$3/` -- a u8 is at most 3 decimal
    // digits, so this only ever fires on exactly 3 of them.
    let raw = buff[27];
    let digits = raw.to_string();
    let formatted = if digits.len() == 3 {
        format!("{}.{}.{}", &digits[0..1], &digits[1..2], &digits[2..3])
    } else {
        digits
    };
    metadata.insert("MPC:EncoderVersion", TagValue::new_string(formatted));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(metadata: &MetadataMap, key: &str) -> String {
        match metadata.get(key) {
            Some(TagValue::String(s)) => s.clone(),
            Some(TagValue::Integer(i)) => i.to_string(),
            Some(other) => panic!("{key} is not a printable scalar: {other:?}"),
            None => panic!("missing tag {key}"),
        }
    }

    /// A minimal v7 MPC header with the exact bit pattern this module's doc
    /// comment cites: `TotalFrames=102`, `SampleRate` code 0 (44100),
    /// `Quality` code 10 ("5 (Standard)"), `MaxBand=28`, `FastSeek=No`,
    /// `Gapless=Yes`, `EncoderVersion` raw byte 115 ("1.1.5").
    fn mpc_header() -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0..3].copy_from_slice(b"MP+");
        h[3] = 0x07;
        h[4..8].copy_from_slice(&102u32.to_le_bytes());
        h[10] = 0xa0; // SampleRate=0, Quality=10
        h[11] = 0x5c; // MaxBand=28 (0x3f mask)
        h[22] = 0x60; // FastSeek bit (bit 3) = 0
        h[23] = 0x82; // Gapless bit (bit 7) = 1
        h[27] = 115;
        h
    }

    #[test]
    fn parses_v7_bit_header_fields() {
        let mut metadata = MetadataMap::new();
        parse_mpc_header(&mpc_header(), &mut metadata);

        assert_eq!(text(&metadata, "MPC:TotalFrames"), "102");
        assert_eq!(text(&metadata, "MPC:SampleRate"), "44100");
        assert_eq!(text(&metadata, "MPC:Quality"), "5 (Standard)");
        assert_eq!(text(&metadata, "MPC:MaxBand"), "28");
        assert_eq!(text(&metadata, "MPC:FastSeek"), "No");
        assert_eq!(text(&metadata, "MPC:Gapless"), "Yes");
        assert_eq!(text(&metadata, "MPC:EncoderVersion"), "1.1.5");
    }

    /// The pinned oracle's ground truth for the real corpus fixture: an MPC
    /// file wrapped in a leading ID3v2.2 tag (`APE.mpc`, MP+ header at byte
    /// 263) still reaches its own audio header and APE trailer.
    #[test]
    fn ape_mpc_matches_pinned_oracle_shape() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new("/tmp/oxidex-exiftool-cache/exiftool/t/images/APE.mpc");
        if !path.is_file() {
            return;
        }
        let reader = crate::io::MMapReader::new(path).expect("mmap APE.mpc");
        let metadata = MpcParser.parse(&reader).expect("parse APE.mpc");

        assert_eq!(text(&metadata, "MPC:TotalFrames"), "102");
        assert_eq!(text(&metadata, "MPC:SampleRate"), "44100");
        assert_eq!(text(&metadata, "MPC:Quality"), "5 (Standard)");
        assert_eq!(text(&metadata, "MPC:EncoderVersion"), "1.1.5");

        assert_eq!(text(&metadata, "APE:Artist"), "Kraftwerk");
        assert_eq!(text(&metadata, "APE:Title"), "Men Machine Live");
        assert_eq!(text(&metadata, "APE:Track"), "4");

        assert_eq!(text(&metadata, "ID3v1:Artist"), "Who Knows");
        assert_eq!(text(&metadata, "ID3:Title"), "ExifTool Test");
    }
}
