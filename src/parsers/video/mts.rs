//! MTS/M2TS (MPEG-2 Transport Stream) format parser
//!
//! Implements metadata extraction from MPEG Transport Stream files (.mts, .m2ts,
//! .ts), commonly used for HD video recording on camcorders and Blu-ray discs.
//!
//! # Supported Metadata
//!
//! - **PAT/PMT:** the elementary stream types carried by each program, reported
//!   as `VideoStreamType` / `AudioStreamType`
//! - **AC-3 descriptor:** `AudioBitrate`, `SurroundMode`, `AudioChannels`
//! - **AC-3 sync frame:** `AudioSampleRate`
//! - **Adaptation field PCR:** `Duration`
//!
//! # ExifTool Compatibility
//!
//! Mirrors `M2TS.pm`'s `ProcessM2TS`, `ParseAC3Descriptor` and `ParseAC3Audio`.
//! All tags land in ExifTool's family-0 `M2TS` group -- the AC-3 tags live in
//! `Image::ExifTool::M2TS::AC3`, whose family-1 group is `AC3` but whose family-0
//! group (what the parity harness compares against) is still `M2TS`.
//!
//! Before this parser walked the transport stream it emitted only `PacketSize`,
//! `PacketCount` and `FormatType` -- three tags ExifTool does not define for any
//! M2TS file, while every tag ExifTool does report was missing.
//!
//! # File Structure
//!
//! ```text
//! [TS Packet 0 - 188 bytes]
//!   ├─ Sync Byte: 0x47 (1 byte)
//!   ├─ Header: 3 bytes (flags, PID, adaptation/payload control)
//!   ├─ Adaptation field (optional, carries the PCR)
//!   └─ Payload
//! [TS Packet 1 - 188 bytes]
//! ...
//! ```
//!
//! The M2TS variant prefixes every packet with a 4-byte arrival timecode
//! (192 bytes total).
//!
//! # References
//!
//! - ISO 13818-1: MPEG-2 Transport Stream Specification
//! - ExifTool Source: `lib/Image/ExifTool/M2TS.pm`

#![allow(dead_code)]

use std::collections::HashMap;

use super::h264;
use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// MPEG-TS sync byte (appears every 188 or 192 bytes)
const TS_SYNC_BYTE: u8 = 0x47;

/// Standard TS packet size (188 bytes)
const TS_PACKET_SIZE: usize = 188;

/// M2TS packet size with the 4-byte arrival timecode (192 bytes)
const M2TS_PACKET_SIZE: usize = 192;

/// Bytes ExifTool reads to locate the first two sync bytes (`192 + 188 + 3`).
const LAYOUT_PROBE_LEN: usize = 383;

/// Highest byte offset at which the first sync byte may appear.
const MAX_LAYOUT_START: usize = 190;

/// Cap on the forward scan. PAT and PMT are always at the head of the stream,
/// so this only bounds how long we keep looking when a file has no PMT at all.
const MAX_FORWARD_BYTES: u64 = 4 * 1024 * 1024;

/// ExifTool backscans at most 512 kB from the end looking for the final PCR
/// ("have seen last PCR at -276k"). Matching that keeps Duration identical.
const MAX_BACKSCAN_BYTES: u64 = 512_000;

/// The 27 MHz transport stream clock, used to turn PCR ticks into seconds.
const PCR_CLOCK_HZ: f64 = 27_000_000.0;

/// MTS parser
pub struct MtsParser;

impl FormatParser for MtsParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let layout = PacketLayout::detect(reader)?;
        let mut scan = StreamScan::default();
        scan.run(reader, &layout)?;

        let mut metadata = MetadataMap::with_capacity(8);
        scan.emit(&mut metadata);
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::MTS)
    }
}

/// Convenience function to parse MTS metadata from a reader.
///
/// This is a wrapper around `MtsParser::parse()` to provide a simpler API
/// for the operations module.
///
/// # Arguments
///
/// * `reader` - FileReader implementation providing access to the MTS file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_mts_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = MtsParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Whether this file is an MPEG-2 transport stream, for the file-type
/// detection layer.
///
/// This deliberately returns a bool rather than exposing [`PacketLayout`].
/// Detection needs a yes/no, not the packet geometry, and publishing the
/// struct would make its field layout part of a cross-module contract for no
/// benefit -- anyone later changing how the arrival timecode is handled would
/// have to reason about a caller in `src/core`. A bool has no such coupling.
///
/// Detection is not a signature match: a transport stream has no magic number.
/// The probe validates a 0x47 sync byte repeating at the packet stride, which
/// is the only thing that distinguishes one.
pub(crate) fn is_transport_stream(reader: &dyn FileReader) -> bool {
    PacketLayout::detect(reader).is_ok()
}

/// Where the packet grid starts and how wide each packet is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PacketLayout {
    /// Byte offset of the first complete packet.
    start: u64,
    /// Length of the 4-byte arrival timecode: 0 for TS, 4 for M2TS.
    timecode_len: usize,
    /// Full packet stride (188 or 192).
    packet_len: usize,
}

impl PacketLayout {
    /// Locate the packet grid the way ExifTool's `ProcessM2TS` preamble does.
    ///
    /// A file may begin mid-packet, and the first byte of an M2TS arrival
    /// timecode can itself be 0x47, which fools a naive "sync byte at 0" test
    /// into reading the stream 4 bytes out of phase. ExifTool handles that by
    /// re-validating three following packets and retrying with a timecode
    /// assumed; this does the same.
    fn detect(reader: &dyn FileReader) -> Result<Self> {
        let file_size = reader.size();
        let probe_len = LAYOUT_PROBE_LEN.min(file_size as usize);
        if probe_len < LAYOUT_PROBE_LEN {
            return Err(ExifToolError::parse_error("File too small to be MTS"));
        }
        let probe = reader.read(0, probe_len)?;

        // Find the first sync byte followed by another one 188 or 192 bytes
        // later. ExifTool's regex is non-greedy over the leading bytes and
        // tries the 188-byte stride first at each position, so do the same.
        let mut found = None;
        'outer: for offset in 0..=MAX_LAYOUT_START {
            if probe[offset] != TS_SYNC_BYTE {
                continue;
            }
            for timecode_len in [0usize, 4] {
                let next = offset + TS_PACKET_SIZE + timecode_len;
                if next < probe.len() && probe[next] == TS_SYNC_BYTE {
                    found = Some((offset, timecode_len));
                    break 'outer;
                }
            }
        }

        let (sync_offset, mut timecode_len) = found.ok_or_else(|| {
            ExifToolError::parse_error("Invalid MPEG-TS signature: sync byte pattern not found")
        })?;

        loop {
            let packet_len = TS_PACKET_SIZE + timecode_len;
            let mut start = sync_offset as i64 - timecode_len as i64;
            if start < 0 {
                // All or part of the first timecode was missing from the file.
                start += M2TS_PACKET_SIZE as i64;
            }
            let start = start as u64;

            // Require four packets and validate the sync byte in the last three.
            let need = packet_len * 4;
            if start + need as u64 > file_size {
                return Err(ExifToolError::parse_error(
                    "File too small to verify MPEG-TS sync bytes",
                ));
            }
            let window = reader.read(start, need)?;
            let valid = (1..4).all(|i| window[timecode_len + packet_len * i] == TS_SYNC_BYTE);

            if valid {
                return Ok(PacketLayout {
                    start,
                    timecode_len,
                    packet_len,
                });
            }
            if timecode_len != 0 {
                return Err(ExifToolError::parse_error(
                    "Failed to verify MPEG-TS sync bytes",
                ));
            }
            // The byte we took for a sync byte was the first byte of an
            // arrival timecode; retry one phase over.
            timecode_len = 4;
        }
    }

    /// Byte offset of packet `index`.
    fn packet_offset(&self, index: u64) -> u64 {
        self.start + index * self.packet_len as u64
    }

    /// Number of complete packets in a file of `file_size` bytes.
    fn packet_count(&self, file_size: u64) -> u64 {
        file_size.saturating_sub(self.start) / self.packet_len as u64
    }
}

/// Accumulated state for one pass over the transport stream.
#[derive(Default)]
struct StreamScan {
    /// PIDs that carry a Program Map Table, discovered from the PAT.
    pmt_pids: Vec<u16>,
    /// PIDs already assigned a name by the PMT, mirroring ExifTool's `%pidName`.
    named_pids: Vec<u16>,
    /// stream_type by elementary PID, for deciding how to parse a payload.
    pid_types: HashMap<u16, u8>,
    /// PIDs whose elementary stream we still want to inspect.
    wanted_pids: Vec<u16>,
    /// Partially accumulated PES payload per wanted PID. A single TS packet
    /// rarely holds a whole SPS or SEI, so these are joined before parsing.
    pending: HashMap<u16, Vec<u8>>,
    /// True once the PAT has been consumed.
    saw_pat: bool,

    video_stream_type: Option<u8>,
    audio_stream_type: Option<u8>,
    ac3_bitrate_code: Option<u8>,
    ac3_surround_code: Option<u8>,
    ac3_channel_code: Option<u8>,
    ac3_sample_rate_code: Option<u8>,

    /// Tags recovered from the H.264 elementary stream, kept separate because
    /// they belong to ExifTool's `H264` group rather than `M2TS`.
    h264: MetadataMap,
    h264_frames_parsed: u8,

    first_pcr: Option<u64>,
    last_pcr: Option<u64>,
}

impl StreamScan {
    fn run(&mut self, reader: &dyn FileReader, layout: &PacketLayout) -> Result<()> {
        let file_size = reader.size();
        let total_packets = layout.packet_count(file_size);
        if total_packets < 4 {
            return Err(ExifToolError::parse_error(
                "Failed to verify MPEG-TS sync bytes",
            ));
        }

        let forward_limit = total_packets
            .min(MAX_FORWARD_BYTES / layout.packet_len as u64)
            .max(1);

        let mut forward_end = 0u64;
        for index in 0..forward_limit {
            forward_end = index + 1;
            let packet = reader.read(layout.packet_offset(index), layout.packet_len)?;
            self.consume_packet(&packet[layout.timecode_len..]);
            // Stop as soon as the PAT, every PMT it named, and every audio or
            // video elementary stream those PMTs named have been parsed.
            if self.saw_pat && self.pmt_pids.is_empty() && self.wanted_pids.is_empty() {
                break;
            }
        }

        // ExifTool flushes every partially accumulated stream at EOF. That is
        // not a corner case: the M2TS.mts fixture's audio and video PIDs each
        // occur only in the final packets, so both stay under the byte
        // threshold and are only ever parsed here.
        self.flush_pending();

        // ExifTool then backscans from the end for the last PCR, so that
        // Duration spans the whole file rather than the part it read forward.
        let backscan_packets = (MAX_BACKSCAN_BYTES / layout.packet_len as u64).max(1);
        let back_start = total_packets
            .saturating_sub(backscan_packets)
            .max(forward_end);
        for index in (back_start..total_packets).rev() {
            let packet = reader.read(layout.packet_offset(index), layout.packet_len)?;
            if let Some(pcr) = read_pcr(&packet[layout.timecode_len..]) {
                self.last_pcr = Some(pcr);
                break;
            }
        }

        Ok(())
    }

    /// Handle one 188-byte packet (timecode already stripped).
    fn consume_packet(&mut self, packet: &[u8]) {
        if packet.len() < 4 || packet[0] != TS_SYNC_BYTE {
            return;
        }
        let payload_unit_start = packet[1] & 0x40 != 0;
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        let has_adaptation = packet[3] & 0x20 != 0;
        let has_payload = packet[3] & 0x10 != 0;

        let mut pos = 4usize;
        if has_adaptation {
            let Some(&len) = packet.get(pos) else {
                return;
            };
            pos += 1;
            let len = len as usize;
            if pos + len > packet.len() {
                return;
            }
            if len > 6
                && let Some(pcr) = read_pcr_field(&packet[pos..pos + len])
            {
                self.first_pcr.get_or_insert(pcr);
                self.last_pcr = Some(pcr);
            }
            pos += len;
        }

        if !has_payload || pos >= packet.len() {
            return;
        }

        let is_pat = pid == 0;
        let is_pmt = self.pmt_pids.contains(&pid);
        if is_pat || is_pmt {
            // A section starts after the pointer field. Sections spanning
            // several packets are rare for PAT/PMT and are simply skipped,
            // exactly like a truncated section.
            if !payload_unit_start {
                return;
            }
            let pointer = packet[pos] as usize;
            pos += 1 + pointer;
            if pos >= packet.len() {
                return;
            }
            self.consume_section(pid, is_pat, &packet[pos..]);
            return;
        }

        if self.wanted_pids.contains(&pid) {
            self.accumulate_elementary(pid, payload_unit_start, &packet[pos..]);
        }
    }

    /// Append one packet's worth of PES payload to this PID's buffer, parsing
    /// the previous buffer whenever a new PES packet starts or enough bytes
    /// have piled up.
    fn accumulate_elementary(&mut self, pid: u16, payload_unit_start: bool, payload: &[u8]) {
        if payload_unit_start {
            if self.pending.contains_key(&pid) {
                self.parse_pending(pid);
                if !self.wanted_pids.contains(&pid) {
                    return;
                }
            }
            let Some(body) = strip_pes_header(payload) else {
                return;
            };
            self.pending.insert(pid, body.to_vec());
        } else {
            let Some(buffer) = self.pending.get_mut(&pid) else {
                // Nothing to append to: this stream started before the point
                // we joined it, so there is no PES boundary to trust.
                return;
            };
            buffer.extend_from_slice(payload);
        }

        // ExifTool holds 1 kB of an H.264 or unrecognised stream but only
        // 256 bytes of anything else before parsing what it has.
        let save_len = match self.pid_types.get(&pid) {
            Some(&0x1b) | None => 1024,
            _ => 256,
        };
        if self.pending.get(&pid).is_some_and(|b| b.len() >= save_len) {
            self.parse_pending(pid);
        }
    }

    /// Parse and discard whatever has accumulated for one PID.
    fn parse_pending(&mut self, pid: u16) {
        let Some(buffer) = self.pending.remove(&pid) else {
            return;
        };
        let Some(&stream_type) = self.pid_types.get(&pid) else {
            return;
        };
        self.consume_elementary(pid, stream_type, &buffer);
    }

    /// Parse every stream still buffered when the scan ends.
    fn flush_pending(&mut self) {
        let pids: Vec<u16> = self.pending.keys().copied().collect();
        for pid in pids {
            self.parse_pending(pid);
        }
    }

    /// Parse a PSI section (PAT when `is_pat`, otherwise PMT).
    fn consume_section(&mut self, pid: u16, is_pat: bool, section: &[u8]) {
        if section.len() < 8 {
            return;
        }
        let table_id = section[0];
        let expected_id = if is_pat { 0x00 } else { 0x02 };
        if table_id != expected_id {
            // A PMT PID may also carry other tables; ExifTool skips them and
            // stops needing this PID.
            self.pmt_pids.retain(|&p| p != pid);
            return;
        }
        if section[1] & 0xc0 != 0x80 {
            return; // bad section_syntax_indicator
        }
        let section_length = usize::from(u16::from_be_bytes([section[1], section[2]]) & 0x0fff);
        if section_length > 1021 || section.len() < section_length + 3 {
            return; // truncated: we only handle single-packet sections
        }
        let program_number = u16::from_be_bytes([section[3], section[4]]);
        // section_length counts from just after itself; drop the 4-byte CRC.
        let end = section_length + 3 - 4;
        let mut pos = 8usize;

        if is_pat {
            self.saw_pat = true;
            while pos + 4 <= end {
                let program_map_pid =
                    u16::from_be_bytes([section[pos + 2], section[pos + 3]]) & 0x1fff;
                if !self.pmt_pids.contains(&program_map_pid) {
                    self.pmt_pids.push(program_map_pid);
                }
                self.named_pids.push(program_map_pid);
                pos += 4;
            }
            return;
        }

        // PMT
        if pos + 4 > end {
            return;
        }
        let program_info_length =
            usize::from(u16::from_be_bytes([section[pos + 2], section[pos + 3]]) & 0x0fff);
        pos += 4;
        if pos + program_info_length > end {
            return;
        }
        pos += program_info_length;

        while pos + 5 <= end {
            let stream_type = section[pos];
            let elementary_pid = u16::from_be_bytes([section[pos + 1], section[pos + 2]]) & 0x1fff;
            let es_info_length =
                usize::from(u16::from_be_bytes([section[pos + 3], section[pos + 4]]) & 0x0fff);

            // ExifTool classifies the stream by searching its *description*
            // for "Audio" or "Video", so an entry only claims a slot when its
            // PID has not already been named by an earlier program.
            let already_named = self.named_pids.contains(&elementary_pid);
            match stream_kind(stream_type) {
                Some(StreamKind::Video) if !already_named => {
                    self.video_stream_type.get_or_insert(stream_type);
                    self.want(elementary_pid);
                }
                Some(StreamKind::Audio) if !already_named => {
                    self.audio_stream_type.get_or_insert(stream_type);
                    self.want(elementary_pid);
                }
                _ => {}
            }
            self.named_pids.push(elementary_pid);
            self.pid_types.insert(elementary_pid, stream_type);

            pos += 5;
            if pos + es_info_length > section.len() {
                return;
            }

            // Elementary stream descriptors.
            let mut j = 0usize;
            while j + 2 <= es_info_length {
                let descriptor_tag = section[pos + j];
                let descriptor_len = section[pos + j + 1] as usize;
                j += 2;
                if j + descriptor_len > es_info_length {
                    break;
                }
                let descriptor = &section[pos + j..pos + j + descriptor_len];
                j += descriptor_len;
                if descriptor_tag == 0x81 {
                    self.parse_ac3_descriptor(descriptor);
                }
            }
            pos += es_info_length;
        }

        // This program map is fully consumed.
        self.pmt_pids.retain(|&p| p != pid);
        let _ = program_number;
    }

    fn want(&mut self, pid: u16) {
        if !self.wanted_pids.contains(&pid) {
            self.wanted_pids.push(pid);
        }
    }

    /// `ParseAC3Descriptor`: bitrate, surround mode and channel count all live
    /// in the second and third bytes of the AC-3 registration descriptor.
    fn parse_ac3_descriptor(&mut self, descriptor: &[u8]) {
        if descriptor.len() < 3 {
            return;
        }
        self.ac3_bitrate_code.get_or_insert(descriptor[1] >> 2);
        self.ac3_surround_code.get_or_insert(descriptor[1] & 0x03);
        self.ac3_channel_code
            .get_or_insert((descriptor[2] >> 1) & 0x0f);
    }

    /// Inspect one elementary stream payload.
    fn consume_elementary(&mut self, pid: u16, stream_type: u8, payload: &[u8]) {
        match stream_type {
            0x81 | 0x87 | 0x91 => {
                // `ParseAC3Audio`: the sample rate code is the top 2 bits of
                // the byte 4 past an AC-3 sync word.
                if let Some(code) = find_ac3_sample_rate_code(payload) {
                    self.ac3_sample_rate_code.get_or_insert(code);
                    self.wanted_pids.retain(|&p| p != pid);
                }
            }
            0x1b => {
                let found_user_data = h264::parse_h264_stream(payload, &mut self.h264);
                self.h264_frames_parsed += 1;
                // Panasonic cameras do not put the SEI in the first frame, so
                // ExifTool allows exactly one more before giving up.
                if found_user_data || self.h264_frames_parsed >= 2 {
                    self.wanted_pids.retain(|&p| p != pid);
                }
            }
            // Nothing else is decoded yet; stop asking for it so the forward
            // scan can finish.
            _ => self.wanted_pids.retain(|&p| p != pid),
        }
    }

    fn emit(&self, metadata: &mut MetadataMap) {
        if let Some(code) = self.video_stream_type {
            metadata.insert(
                "M2TS:VideoStreamType".to_string(),
                TagValue::new_string(stream_type_name(code)),
            );
        }
        if let Some(code) = self.audio_stream_type {
            metadata.insert(
                "M2TS:AudioStreamType".to_string(),
                TagValue::new_string(stream_type_name(code)),
            );
        }
        if let Some(code) = self.ac3_bitrate_code {
            metadata.insert("M2TS:AudioBitrate".to_string(), ac3_bitrate(code));
        }
        if let Some(code) = self.ac3_surround_code {
            metadata.insert(
                "M2TS:SurroundMode".to_string(),
                TagValue::new_string(ac3_surround_mode(code)),
            );
        }
        if let Some(code) = self.ac3_channel_code {
            metadata.insert("M2TS:AudioChannels".to_string(), ac3_channels(code));
        }
        if let (Some(start), Some(end)) = (self.first_pcr, self.last_pcr) {
            // A 33-bit program clock reference wraps; ExifTool adds the period
            // back rather than reporting a negative duration.
            let mut end = end;
            if start > end {
                end += 0x8000_0000u64 * 1200;
            }
            metadata.insert(
                "M2TS:Duration".to_string(),
                TagValue::new_string(convert_duration((end - start) as f64 / PCR_CLOCK_HZ)),
            );
        }
        if let Some(code) = self.ac3_sample_rate_code {
            metadata.insert("M2TS:AudioSampleRate".to_string(), ac3_sample_rate(code));
        }
        for (key, value) in self.h264.iter() {
            metadata.insert(key.clone(), value.clone());
        }
    }
}

/// Stream IDs whose PES packets carry no optional header, so their payload
/// starts immediately after the 6-byte packet header.
/// (`%noSyntax` in M2TS.pm.)
const PES_STREAM_IDS_WITHOUT_HEADER: &[u8] = &[
    0xbc, // program_stream_map
    0xbe, // padding_stream
    0xbf, // private_stream_2
    0xf0, // ECM_stream
    0xf1, // EMM_stream
    0xf2, // DSMCC_stream
    0xf8, // ITU-T Rec. H.222.1 type E stream
    0xff, // program_stream_directory
];

/// Skip the PES packet header so only elementary stream bytes accumulate.
///
/// Returns `None` when the payload does not start with a PES packet, which is
/// how ExifTool treats a payload it cannot align.
fn strip_pes_header(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < 6 {
        return None;
    }
    if payload[0] != 0x00 || payload[1] != 0x00 || payload[2] != 0x01 {
        return None;
    }
    let stream_id = payload[3];
    let mut pos = 6usize;
    if !PES_STREAM_IDS_WITHOUT_HEADER.contains(&stream_id) {
        if pos + 3 > payload.len() {
            return None;
        }
        // The two high bits of the first optional-header byte are always 0b10.
        if payload[pos] & 0xc0 != 0x80 {
            return None;
        }
        pos += 3 + payload[pos + 2] as usize;
    }
    payload.get(pos..)
}

/// Read the PCR out of a whole packet, adaptation field included.
fn read_pcr(packet: &[u8]) -> Option<u64> {
    if packet.len() < 5 || packet[0] != TS_SYNC_BYTE || packet[3] & 0x20 == 0 {
        return None;
    }
    let len = packet[4] as usize;
    if len <= 6 || 5 + len > packet.len() {
        return None;
    }
    read_pcr_field(&packet[5..5 + len])
}

/// Read the PCR out of an adaptation field body (the byte after its length).
fn read_pcr_field(field: &[u8]) -> Option<u64> {
    if field.len() < 7 || field[0] & 0x10 == 0 {
        return None;
    }
    let base = u64::from(u32::from_be_bytes([field[1], field[2], field[3], field[4]]));
    let ext = u64::from(u16::from_be_bytes([field[5], field[6]]));
    Some(300 * (2 * base + (ext >> 15)) + (ext & 0x01ff))
}

/// Locate an AC-3 sync word (0x0B77) and return the sample rate code that
/// follows it, matching `ParseAC3Audio`'s `/\x0b\x77..(.)/` probe.
fn find_ac3_sample_rate_code(data: &[u8]) -> Option<u8> {
    data.windows(5)
        .find(|w| w[0] == 0x0b && w[1] == 0x77)
        .map(|w| w[4] >> 6)
}

/// Whether a stream type describes audio or video, per ExifTool's
/// `if ($str =~ /(Audio|Video)/)` test against the stream type description.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamKind {
    Audio,
    Video,
}

fn stream_kind(stream_type: u8) -> Option<StreamKind> {
    let name = STREAM_TYPES
        .iter()
        .find(|(code, _)| *code == stream_type)
        .map(|(_, name)| *name)?;
    // Case-sensitive, and the first of the two words wins -- the same as the
    // Perl capture group. No current description contains both.
    match (name.find("Audio"), name.find("Video")) {
        (Some(a), Some(v)) if a < v => Some(StreamKind::Audio),
        (Some(_), Some(_)) => Some(StreamKind::Video),
        (Some(_), None) => Some(StreamKind::Audio),
        (None, Some(_)) => Some(StreamKind::Video),
        (None, None) => None,
    }
}

/// `%streamType` from M2TS.pm, with ExifTool's `PrintHex` fallback for codes
/// the table does not name.
fn stream_type_name(stream_type: u8) -> String {
    STREAM_TYPES
        .iter()
        .find(|(code, _)| *code == stream_type)
        .map(|(_, name)| (*name).to_string())
        .unwrap_or_else(|| format!("Unknown (0x{:x})", stream_type))
}

/// `%streamType` from `Image::ExifTool::M2TS`.
static STREAM_TYPES: &[(u8, &str)] = &[
    (0x00, "Reserved"),
    (0x01, "MPEG-1 Video"),
    (0x02, "MPEG-2 Video"),
    (0x03, "MPEG-1 Audio"),
    (0x04, "MPEG-2 Audio"),
    (0x05, "ISO 13818-1 private sections"),
    (0x06, "ISO 13818-1 PES private data"),
    (0x07, "ISO 13522 MHEG"),
    (0x08, "ISO 13818-1 DSM-CC"),
    (0x09, "ISO 13818-1 auxiliary"),
    (0x0a, "ISO 13818-6 multi-protocol encap"),
    (0x0b, "ISO 13818-6 DSM-CC U-N msgs"),
    (0x0c, "ISO 13818-6 stream descriptors"),
    (0x0d, "ISO 13818-6 sections"),
    (0x0e, "ISO 13818-1 auxiliary"),
    (0x0f, "MPEG-2 AAC Audio"),
    (0x10, "MPEG-4 Video"),
    (0x11, "MPEG-4 LATM AAC Audio"),
    (0x12, "MPEG-4 generic"),
    (0x13, "ISO 14496-1 SL-packetized"),
    (0x14, "ISO 13818-6 Synchronized Download Protocol"),
    (0x15, "Packetized metadata"),
    (0x16, "Sectioned metadata"),
    (0x17, "ISO/IEC 13818-6 DSM CC Data Carousel metadata"),
    (0x18, "ISO/IEC 13818-6 DSM CC Object Carousel metadata"),
    (
        0x19,
        "ISO/IEC 13818-6 Synchronized Download Protocol metadata",
    ),
    (0x1a, "ISO/IEC 13818-11 IPMP"),
    (0x1b, "H.264 (AVC) Video"),
    (0x1c, "ISO/IEC 14496-3 (MPEG-4 raw audio)"),
    (0x1d, "ISO/IEC 14496-17 (MPEG-4 text)"),
    (0x1e, "ISO/IEC 23002-3 (MPEG-4 auxiliary video)"),
    (0x1f, "ISO/IEC 14496-10 SVC (MPEG-4 AVC sub-bitstream)"),
    (0x20, "ISO/IEC 14496-10 MVC (MPEG-4 AVC sub-bitstream)"),
    (0x21, "ITU-T Rec. T.800 and ISO/IEC 15444 (JPEG 2000 video)"),
    (0x24, "H.265 (HEVC) Video"),
    (0x42, "Chinese Video Standard"),
    (0x7f, "ISO/IEC 13818-11 IPMP (DRM)"),
    (0x80, "DigiCipher II Video"),
    (0x81, "A52/AC-3 Audio"),
    (0x82, "HDMV DTS Audio"),
    (0x83, "LPCM Audio"),
    (0x84, "SDDS Audio"),
    (0x85, "ATSC Program ID"),
    (0x86, "DTS-HD Audio"),
    (0x87, "E-AC-3 Audio"),
    (0x8a, "DTS Audio"),
    (0x90, "Presentation Graphic Stream (subtitle)"),
    (0x91, "A52b/AC-3 Audio"),
    (0x92, "DVD_SPU vls Subtitle"),
    (0x94, "SDDS Audio"),
    (0xa0, "MSCODEC Video"),
    (0xea, "Private ES (VC-1)"),
];

/// AC-3 `AudioBitrate`: index into the rate table, then `ConvertBitrate`.
/// Codes 32..=50 mark a maximum rather than a constant rate.
fn ac3_bitrate(code: u8) -> TagValue {
    const RATES: [u32; 19] = [
        32_000, 40_000, 48_000, 56_000, 64_000, 80_000, 96_000, 112_000, 128_000, 160_000, 192_000,
        224_000, 256_000, 320_000, 384_000, 448_000, 512_000, 576_000, 640_000,
    ];
    let (index, suffix) = match code {
        0..=18 => (code as usize, ""),
        32..=50 => ((code - 32) as usize, " max"),
        _ => return TagValue::new_string(format!("Unknown ({})", code)),
    };
    TagValue::new_string(format!(
        "{}{}",
        convert_bitrate(f64::from(RATES[index])),
        suffix
    ))
}

/// AC-3 `SurroundMode` PrintConv.
fn ac3_surround_mode(code: u8) -> String {
    match code {
        0 => "Not indicated".to_string(),
        1 => "Not Dolby surround".to_string(),
        2 => "Dolby surround".to_string(),
        _ => format!("Unknown ({})", code),
    }
}

/// AC-3 `AudioChannels` PrintConv. Some entries are plain counts, others
/// describe a front/rear split or an upper bound, so the emitted type varies.
fn ac3_channels(code: u8) -> TagValue {
    match code {
        1 | 8 => TagValue::new_integer(1),
        2 => TagValue::new_integer(2),
        3 => TagValue::new_integer(3),
        0 => TagValue::new_string("1 + 1"),
        4 => TagValue::new_string("2/1"),
        5 => TagValue::new_string("3/1"),
        6 => TagValue::new_string("2/2"),
        7 => TagValue::new_string("3/2"),
        9 => TagValue::new_string("2 max"),
        10 => TagValue::new_string("3 max"),
        11 => TagValue::new_string("4 max"),
        12 => TagValue::new_string("5 max"),
        13 => TagValue::new_string("6 max"),
        _ => TagValue::new_string(format!("Unknown ({})", code)),
    }
}

/// AC-3 `AudioSampleRate` PrintConv.
fn ac3_sample_rate(code: u8) -> TagValue {
    match code {
        0 => TagValue::new_integer(48000),
        1 => TagValue::new_integer(44100),
        2 => TagValue::new_integer(32000),
        _ => TagValue::new_string(format!("Unknown ({})", code)),
    }
}

/// ExifTool's `ConvertBitrate`: step up through bps/kbps/Mbps/Gbps, then print
/// with 3 significant digits below 100 and no decimals at or above it.
fn convert_bitrate(bitrate: f64) -> String {
    const UNITS: [&str; 4] = ["bps", "kbps", "Mbps", "Gbps"];
    let mut value = bitrate;
    for (index, unit) in UNITS.iter().enumerate() {
        let is_last = index == UNITS.len() - 1;
        if value >= 1000.0 && !is_last {
            value /= 1000.0;
            continue;
        }
        return if value < 100.0 {
            format!("{} {}", format_significant_3(value), unit)
        } else {
            format!("{:.0} {}", value, unit)
        };
    }
    unreachable!("UNITS is non-empty so the loop always returns")
}

/// Perl's `%.3g`, which drops trailing zeros and any trailing decimal point.
fn format_significant_3(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let magnitude = value.abs().log10().floor() as i32;
    let decimals = (2 - magnitude).max(0) as usize;
    let rendered = format!("{:.*}", decimals, value);
    if rendered.contains('.') {
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        rendered
    }
}

/// ExifTool's `ConvertDuration`.
fn convert_duration(seconds: f64) -> String {
    if seconds == 0.0 {
        return "0 s".to_string();
    }
    let (sign, mut time) = if seconds > 0.0 {
        ("", seconds)
    } else {
        ("-", -seconds)
    };
    if time < 30.0 {
        return format!("{}{:.2} s", sign, time);
    }
    time += 0.5; // round off to the nearest second
    let mut hours = (time / 3600.0) as i64;
    time -= hours as f64 * 3600.0;
    let minutes = (time / 60.0) as i64;
    time -= minutes as f64 * 60.0;

    let mut prefix = sign.to_string();
    if hours > 24 {
        let days = hours / 24;
        hours -= days * 24;
        prefix = format!("{}{} days ", sign, days);
    }
    format!("{}{}:{:02}:{:02}", prefix, hours, minutes, time as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Build a 188-byte TS packet with the given PID and payload.
    fn ts_packet(pid: u16, payload_unit_start: bool, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0xffu8; TS_PACKET_SIZE];
        packet[0] = TS_SYNC_BYTE;
        packet[1] = ((pid >> 8) as u8) & 0x1f | if payload_unit_start { 0x40 } else { 0 };
        packet[2] = (pid & 0xff) as u8;
        packet[3] = 0x10; // payload only
        packet[4..4 + payload.len()].copy_from_slice(payload);
        packet
    }

    /// Wrap a PSI section in a payload with a zero pointer field.
    fn psi_payload(section: &[u8]) -> Vec<u8> {
        let mut payload = vec![0u8];
        payload.extend_from_slice(section);
        payload
    }

    /// A PAT naming a single program carried on `pmt_pid`.
    fn pat_section(pmt_pid: u16) -> Vec<u8> {
        // table_id, syntax+length, program_number, version, section numbers
        let body = [
            0x00u8,
            0x01,
            0xc1,
            0x00,
            0x00,
            0x00,
            0x01,
            (pmt_pid >> 8) as u8 | 0xe0,
            (pmt_pid & 0xff) as u8,
        ];
        let mut section = vec![0x00u8, 0x80, 0x00];
        section.extend_from_slice(&body[3..]);
        section.extend_from_slice(&[0, 0, 0, 0]); // CRC
        let len = (section.len() - 3) as u16;
        section[1] = 0x80 | ((len >> 8) as u8 & 0x0f);
        section[2] = (len & 0xff) as u8;
        section
    }

    /// A PMT declaring one elementary stream, optionally with a descriptor.
    fn pmt_section(stream_type: u8, elementary_pid: u16, descriptor: &[u8]) -> Vec<u8> {
        let mut section = vec![0x02u8, 0x80, 0x00];
        section.extend_from_slice(&[0x00, 0x01]); // program_number
        section.extend_from_slice(&[0xc1, 0x00, 0x00]); // version, section numbers
        section.extend_from_slice(&[0xe1, 0x00]); // PCR PID
        section.extend_from_slice(&[0xf0, 0x00]); // program_info_length = 0
        section.push(stream_type);
        section.extend_from_slice(&[
            (elementary_pid >> 8) as u8 | 0xe0,
            (elementary_pid & 0xff) as u8,
        ]);
        let es_len = descriptor.len() as u16;
        section.extend_from_slice(&[0xf0 | (es_len >> 8) as u8, (es_len & 0xff) as u8]);
        section.extend_from_slice(descriptor);
        section.extend_from_slice(&[0, 0, 0, 0]); // CRC
        let len = (section.len() - 3) as u16;
        section[1] = 0x80 | ((len >> 8) as u8 & 0x0f);
        section[2] = (len & 0xff) as u8;
        section
    }

    fn null_packet() -> Vec<u8> {
        ts_packet(0x1fff, false, &[])
    }

    /// Render a tag for comparison against ExifTool's printed value.
    fn shown(metadata: &MetadataMap, key: &str) -> String {
        match metadata
            .get(key)
            .unwrap_or_else(|| panic!("missing {}", key))
        {
            TagValue::String(s) => s.clone(),
            TagValue::Integer(i) => i.to_string(),
            other => panic!("unexpected value type for {}: {:?}", key, other),
        }
    }

    /// Render a standalone TagValue the same way.
    fn shown_value(value: &TagValue) -> String {
        match value {
            TagValue::String(s) => s.clone(),
            TagValue::Integer(i) => i.to_string(),
            other => panic!("unexpected value type: {:?}", other),
        }
    }

    #[test]
    fn test_mts_signature_valid() {
        let mut data = Vec::new();
        for _ in 0..5 {
            data.extend_from_slice(&null_packet());
        }
        let reader = TestReader::from_slice(&data);
        let layout = PacketLayout::detect(&reader).unwrap();
        assert_eq!(layout.packet_len, TS_PACKET_SIZE);
        assert_eq!(layout.timecode_len, 0);
        assert_eq!(layout.start, 0);
    }

    #[test]
    fn test_m2ts_signature_valid() {
        // 192-byte packets: a 4-byte arrival timecode then the TS packet.
        let mut data = Vec::new();
        for _ in 0..5 {
            data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
            data.extend_from_slice(&null_packet());
        }
        let reader = TestReader::from_slice(&data);
        let layout = PacketLayout::detect(&reader).unwrap();
        assert_eq!(layout.packet_len, M2TS_PACKET_SIZE);
        assert_eq!(layout.timecode_len, 4);
        assert_eq!(layout.start, 0);
    }

    /// The file-type detection layer's entry point. It must agree with
    /// `PacketLayout::detect` without exposing the layout itself.
    #[test]
    fn test_is_transport_stream() {
        let mut data = Vec::new();
        for _ in 0..5 {
            data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
            data.extend_from_slice(&null_packet());
        }
        assert!(is_transport_stream(&TestReader::from_slice(&data)));

        // No sync byte at any stride: not a transport stream.
        assert!(!is_transport_stream(&TestReader::from_slice(
            &[0xffu8; M2TS_PACKET_SIZE * 5]
        )));
        // Too short to validate the four packets ExifTool requires.
        assert!(!is_transport_stream(
            &TestReader::from_slice(&null_packet())
        ));
    }

    /// A timecode whose first byte is 0x47 must not shift the packet grid --
    /// this is the edge case ExifTool retries the validation for.
    #[test]
    fn test_m2ts_timecode_starting_with_sync_byte() {
        let mut data = Vec::new();
        for _ in 0..5 {
            data.extend_from_slice(&[TS_SYNC_BYTE, 0x00, 0x00, 0x00]);
            data.extend_from_slice(&null_packet());
        }
        let reader = TestReader::from_slice(&data);
        let layout = PacketLayout::detect(&reader).unwrap();
        assert_eq!(layout.packet_len, M2TS_PACKET_SIZE);
        assert_eq!(layout.timecode_len, 4);
    }

    #[test]
    fn test_mts_signature_invalid() {
        let data = vec![0u8; 1024];
        let reader = TestReader::from_slice(&data);
        let parser = MtsParser;
        assert!(parser.parse(&reader).is_err());
    }

    #[test]
    fn test_mts_file_too_small() {
        let data = vec![TS_SYNC_BYTE; 100];
        let reader = TestReader::from_slice(&data);
        let parser = MtsParser;
        assert!(parser.parse(&reader).is_err());
    }

    #[test]
    fn test_pat_pmt_stream_types() {
        let mut data = Vec::new();
        data.extend_from_slice(&ts_packet(0, true, &psi_payload(&pat_section(0x0100))));
        data.extend_from_slice(&ts_packet(
            0x0100,
            true,
            &psi_payload(&pmt_section(0x1b, 0x1011, &[])),
        ));
        data.extend_from_slice(&null_packet());
        data.extend_from_slice(&null_packet());

        let reader = TestReader::from_slice(&data);
        let metadata = MtsParser.parse(&reader).unwrap();
        assert_eq!(
            shown(&metadata, "M2TS:VideoStreamType"),
            "H.264 (AVC) Video"
        );
        assert!(metadata.get("M2TS:AudioStreamType").is_none());
    }

    /// The AC-3 registration descriptor carries bitrate, surround mode and
    /// channel count; these are the exact bytes from the M2TS.mts fixture.
    #[test]
    fn test_ac3_descriptor_tags() {
        let mut data = Vec::new();
        data.extend_from_slice(&ts_packet(0, true, &psi_payload(&pat_section(0x0100))));
        data.extend_from_slice(&ts_packet(
            0x0100,
            true,
            &psi_payload(&pmt_section(
                0x81,
                0x1100,
                &[0x81, 0x04, 0x04, 0x30, 0x04, 0x00],
            )),
        ));
        data.extend_from_slice(&null_packet());
        data.extend_from_slice(&null_packet());

        let reader = TestReader::from_slice(&data);
        let metadata = MtsParser.parse(&reader).unwrap();
        assert_eq!(shown(&metadata, "M2TS:AudioStreamType"), "A52/AC-3 Audio");
        assert_eq!(shown(&metadata, "M2TS:AudioBitrate"), "256 kbps");
        assert_eq!(shown(&metadata, "M2TS:SurroundMode"), "Not indicated");
        assert_eq!(shown(&metadata, "M2TS:AudioChannels"), "2");
    }

    /// An AC-3 sync word in the elementary stream yields the sample rate.
    #[test]
    fn test_ac3_audio_sample_rate() {
        let mut data = Vec::new();
        data.extend_from_slice(&ts_packet(0, true, &psi_payload(&pat_section(0x0100))));
        data.extend_from_slice(&ts_packet(
            0x0100,
            true,
            &psi_payload(&pmt_section(0x81, 0x1100, &[])),
        ));
        // PES payload: sample rate code 0 in the top 2 bits after the sync word.
        data.extend_from_slice(&ts_packet(
            0x1100,
            true,
            &[0x00, 0x00, 0x0b, 0x77, 0xaa, 0xbb, 0x3f],
        ));
        data.extend_from_slice(&null_packet());

        let reader = TestReader::from_slice(&data);
        let metadata = MtsParser.parse(&reader).unwrap();
        assert_eq!(shown(&metadata, "M2TS:AudioSampleRate"), "48000");
    }

    /// A stream type the table does not name must report its own code rather
    /// than borrowing a neighbouring label.
    #[test]
    fn test_unknown_stream_type_reports_its_code() {
        assert_eq!(stream_type_name(0x99), "Unknown (0x99)");
        assert_eq!(stream_type_name(0x23), "Unknown (0x23)");
        assert_eq!(ac3_surround_mode(3), "Unknown (3)");
        assert_eq!(shown_value(&ac3_channels(14)), "Unknown (14)");
        assert_eq!(shown_value(&ac3_sample_rate(3)), "Unknown (3)");
        assert_eq!(shown_value(&ac3_bitrate(19)), "Unknown (19)");
    }

    /// Only descriptions containing "Audio" or "Video" claim a stream slot --
    /// the lowercase "audio" in 0x1c must not, matching the Perl regex.
    #[test]
    fn test_stream_kind_classification() {
        assert_eq!(stream_kind(0x1b), Some(StreamKind::Video));
        assert_eq!(stream_kind(0x81), Some(StreamKind::Audio));
        assert_eq!(stream_kind(0x0f), Some(StreamKind::Audio));
        assert_eq!(stream_kind(0x1c), None); // "MPEG-4 raw audio" is lowercase
        assert_eq!(stream_kind(0x15), None); // "Packetized metadata"
        assert_eq!(stream_kind(0x99), None); // not in the table at all
    }

    #[test]
    fn test_convert_bitrate_matches_exiftool() {
        assert_eq!(convert_bitrate(256_000.0), "256 kbps");
        assert_eq!(convert_bitrate(32_000.0), "32 kbps");
        assert_eq!(convert_bitrate(640_000.0), "640 kbps");
        assert_eq!(convert_bitrate(112_000.0), "112 kbps");
        assert_eq!(convert_bitrate(1_500_000.0), "1.5 Mbps");
        assert_eq!(convert_bitrate(999.0), "999 bps");
    }

    #[test]
    fn test_convert_duration_matches_exiftool() {
        assert_eq!(convert_duration(0.0), "0 s");
        assert_eq!(convert_duration(4.97), "4.97 s");
        assert_eq!(convert_duration(29.999), "30.00 s");
        assert_eq!(convert_duration(60.0), "0:01:00");
        assert_eq!(convert_duration(3661.0), "1:01:01");
    }

    #[test]
    fn test_pcr_duration() {
        // Two packets carrying a PCR one second apart (27 MHz clock).
        let mut first = null_packet();
        first[3] = 0x20; // adaptation field only
        first[4] = 7; // adaptation field length
        first[5] = 0x10; // PCR_flag
        first[6..10].copy_from_slice(&0u32.to_be_bytes());
        first[10..12].copy_from_slice(&0u16.to_be_bytes());

        let mut last = first.clone();
        // 27,000,000 ticks == 1 s, encoded as base = ticks/300/2.
        let base = 27_000_000u32 / 600;
        last[6..10].copy_from_slice(&base.to_be_bytes());

        let mut data = Vec::new();
        data.extend_from_slice(&first);
        data.extend_from_slice(&null_packet());
        data.extend_from_slice(&null_packet());
        data.extend_from_slice(&last);

        let reader = TestReader::from_slice(&data);
        let metadata = MtsParser.parse(&reader).unwrap();
        assert_eq!(shown(&metadata, "M2TS:Duration"), "1.00 s");
    }
}
