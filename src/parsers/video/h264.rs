//! H.264 elementary stream metadata
//!
//! Decodes the two NAL unit types that carry metadata in an AVCHD recording:
//!
//! - **Sequence parameter set (type 7):** the coded frame size, which is the
//!   only place an M2TS file states its dimensions.
//! - **SEI unregistered user data (type 6):** the "Modified DV Pack Meta"
//!   (MDPM) block camcorders use to record shooting conditions.
//!
//! # ExifTool Compatibility
//!
//! Mirrors `H264.pm`'s `ParseH264Video`, `ParseSeqParamSet`, `ProcessSEI` and
//! the `MDPM` tag table. Tags are emitted under the `H264` family, matching
//! ExifTool's family-0 group.
//!
//! Only the MDPM records this crate has a verified sample for are decoded --
//! `DateTimeOriginal` (0x18/0x19), `Camera1` (0x70), `Camera2` (0x71),
//! `Shutter` (0x7f), `MakeModel` (0xe0) and Canon's `RecInfo` (0xe1). Records
//! outside that set are skipped rather than guessed at.
//!
//! # References
//!
//! - ITU-T Rec. H.264, section 7.3.2.1 (sequence parameter set)
//! - ExifTool Source: `lib/Image/ExifTool/H264.pm`

use crate::core::{MetadataMap, TagValue};

/// Parse an accumulated H.264 elementary stream payload.
///
/// Returns `true` once the SEI user data has been found, which is ExifTool's
/// signal to stop feeding it further frames.
pub fn parse_h264_stream(data: &[u8], metadata: &mut MetadataMap) -> bool {
    let mut found_user_data = false;
    let mut parsed_sps = false;

    for unit in NalUnits::new(data) {
        match unit.kind {
            6 if !found_user_data => {
                found_user_data = parse_sei(&unit.rbsp, metadata);
            }
            7 if !parsed_sps => {
                parsed_sps = true;
                parse_seq_param_set(&unit.rbsp, metadata);
            }
            _ => {}
        }
    }

    found_user_data
}

/// One NAL unit with emulation-prevention bytes already removed.
struct NalUnit {
    kind: u8,
    rbsp: Vec<u8>,
}

/// Split a byte stream on Annex B start codes (`00 00 01` or `00 00 00 01`).
struct NalUnits<'a> {
    data: &'a [u8],
    /// Offset of the first payload byte of the pending unit, if any.
    pending: Option<usize>,
    cursor: usize,
}

impl<'a> NalUnits<'a> {
    fn new(data: &'a [u8]) -> Self {
        NalUnits {
            data,
            pending: None,
            cursor: 0,
        }
    }

    /// Index of the next start code at or after `from`, as (code_start, code_end).
    fn next_start_code(&self, from: usize) -> Option<(usize, usize)> {
        let mut i = from;
        while i + 3 <= self.data.len() {
            if self.data[i] == 0 && self.data[i + 1] == 0 {
                if self.data[i + 2] == 1 {
                    return Some((i, i + 3));
                }
                if i + 4 <= self.data.len() && self.data[i + 2] == 0 && self.data[i + 3] == 1 {
                    return Some((i, i + 4));
                }
            }
            i += 1;
        }
        None
    }
}

impl Iterator for NalUnits<'_> {
    type Item = NalUnit;

    fn next(&mut self) -> Option<NalUnit> {
        loop {
            let next = self.next_start_code(self.cursor);
            let (start, end) = match (self.pending, next) {
                // No unit open yet: open one at the first start code.
                (None, Some((_, code_end))) => {
                    self.pending = Some(code_end);
                    self.cursor = code_end;
                    continue;
                }
                (None, None) => return None,
                // A unit is open and another start code follows: the open unit
                // ends where that start code begins.
                (Some(start), Some((code_start, code_end))) => {
                    self.pending = Some(code_end);
                    self.cursor = code_end;
                    (start, code_start)
                }
                // Last unit runs to the end of the buffer.
                (Some(start), None) => {
                    self.pending = None;
                    self.cursor = self.data.len();
                    (start, self.data.len())
                }
            };

            if start >= end || start >= self.data.len() {
                if self.pending.is_none() {
                    return None;
                }
                continue;
            }

            let header = self.data[start];
            // forbidden_zero_bit must be clear.
            if header & 0x80 != 0 {
                return None;
            }
            return Some(NalUnit {
                kind: header & 0x1f,
                rbsp: remove_emulation_prevention(&self.data[start + 1..end]),
            });
        }
    }
}

/// Undo the `00 00 03` escaping the H.264 byte stream uses so that payload
/// bytes can never be mistaken for a start code.
fn remove_emulation_prevention(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut zeros = 0usize;
    for &byte in data {
        if zeros >= 2 && byte == 0x03 {
            zeros = 0;
            continue;
        }
        if byte == 0 {
            zeros += 1;
        } else {
            zeros = 0;
        }
        out.push(byte);
    }
    out
}

// ---------------------------------------------------------------------------
// Sequence parameter set
// ---------------------------------------------------------------------------

/// A most-significant-bit-first bit reader over an RBSP.
struct BitStream<'a> {
    data: &'a [u8],
    pos: usize,
    word: u8,
    mask: u8,
}

impl<'a> BitStream<'a> {
    fn new(data: &'a [u8]) -> Option<Self> {
        let mut stream = BitStream {
            data,
            pos: 0,
            word: 0,
            mask: 0,
        };
        stream.read_next_word().then_some(stream)
    }

    fn read_next_word(&mut self) -> bool {
        match self.data.get(self.pos) {
            Some(&byte) => {
                self.word = byte;
                self.pos += 1;
                self.mask = 0x80;
                true
            }
            None => {
                self.mask = 0;
                false
            }
        }
    }

    /// True while bits remain. ExifTool uses the same test as a validity check
    /// after parsing an SPS: a stream that ran dry produced garbage.
    fn has_bits(&self) -> bool {
        self.mask != 0
    }

    fn bits(&mut self, count: u32) -> u32 {
        let mut value = 0u32;
        for _ in 0..count {
            value <<= 1;
            if self.mask & self.word != 0 {
                value += 1;
            }
            self.mask >>= 1;
            if self.mask == 0 && !self.read_next_word() {
                break;
            }
        }
        value
    }

    /// Unsigned exponential-Golomb code.
    fn golomb(&mut self) -> u32 {
        let mut leading_zeros = 0u32;
        while self.mask & self.word == 0 {
            leading_zeros += 1;
            self.mask >>= 1;
            if self.mask == 0 && !self.read_next_word() {
                break;
            }
            // A corrupt stream could otherwise spin here for a very long time.
            if leading_zeros > 32 {
                break;
            }
        }
        self.bits(leading_zeros + 1).wrapping_sub(1)
    }

    /// Signed exponential-Golomb code.
    fn golomb_signed(&mut self) -> i32 {
        let value = self.golomb().wrapping_add(1);
        if value & 1 != 0 {
            -((value >> 1) as i32)
        } else {
            (value >> 1) as i32
        }
    }
}

/// Consume the optional scaling matrices so the bit position stays aligned.
fn skip_scaling_matrices(stream: &mut BitStream) {
    if stream.bits(1) == 0 {
        return;
    }
    for i in 0..8 {
        let size = if i < 6 { 16 } else { 64 };
        if stream.bits(1) == 0 {
            continue;
        }
        let last = 8i32;
        let mut next = 8i32;
        for j in 0..size {
            if next != 0 {
                next = (last + stream.golomb_signed()) & 0xff;
            }
            if j == 0 && next == 0 {
                break;
            }
        }
    }
}

/// `ParseSeqParamSet`: walk the SPS far enough to recover the frame size.
fn parse_seq_param_set(rbsp: &[u8], metadata: &mut MetadataMap) {
    let Some(mut stream) = BitStream::new(rbsp) else {
        return;
    };

    let profile_idc = stream.bits(8);
    stream.bits(16); // constraint flags and level_idc
    stream.golomb(); // seq_parameter_set_id

    if profile_idc >= 100 {
        let chroma_format_idc = stream.golomb();
        if chroma_format_idc == 3 {
            stream.bits(1); // separate_colour_plane_flag
        }
        stream.golomb(); // bit_depth_luma_minus8
        stream.golomb(); // bit_depth_chroma_minus8
        stream.bits(1); // qpprime_y_zero_transform_bypass_flag
        skip_scaling_matrices(&mut stream);
    }

    stream.golomb(); // log2_max_frame_num_minus4
    let pic_order_cnt_type = stream.golomb();
    if pic_order_cnt_type == 0 {
        stream.golomb(); // log2_max_pic_order_cnt_lsb_minus4
    } else if pic_order_cnt_type == 1 {
        stream.bits(1); // delta_pic_order_always_zero_flag
        stream.golomb(); // offset_for_non_ref_pic
        stream.golomb(); // offset_for_top_to_bottom_field
        let cycle_len = stream.golomb();
        for _ in 0..cycle_len.min(256) {
            stream.golomb(); // offset_for_ref_frame[i]
        }
    }

    stream.golomb(); // num_ref_frames
    stream.bits(1); // gaps_in_frame_num_value_allowed_flag
    let width_mbs = stream.golomb();
    let height_map_units = stream.golomb();
    let frame_mbs_only = stream.bits(1);
    if frame_mbs_only == 0 {
        stream.bits(1); // mb_adaptive_frame_field_flag
    }
    stream.bits(1); // direct_8x8_inference_flag

    let mut width = (width_mbs as i64 + 1) * 16;
    let mut height = (2 - frame_mbs_only as i64) * (height_map_units as i64 + 1) * 16;

    if stream.bits(1) != 0 {
        // frame_cropping_flag
        let vertical_scale = 4 - i64::from(frame_mbs_only) * 2;
        width -= 4 * i64::from(stream.golomb());
        width -= 4 * i64::from(stream.golomb());
        height -= vertical_scale * i64::from(stream.golomb());
        height -= vertical_scale * i64::from(stream.golomb());
    }

    // ExifTool's sanity check: a stream that ran out of bits, or produced a
    // size outside what H.264 can code, is not reported at all.
    if !stream.has_bits() {
        return;
    }
    if (160..=4096).contains(&width) && (120..=3072).contains(&height) {
        metadata.insert("H264:ImageWidth".to_string(), TagValue::new_integer(width));
        metadata.insert(
            "H264:ImageHeight".to_string(),
            TagValue::new_integer(height),
        );
    }
}

// ---------------------------------------------------------------------------
// SEI user data / MDPM
// ---------------------------------------------------------------------------

/// UUID + magic that marks the Modified DV Pack Meta payload.
const MDPM_SIGNATURE: &[u8] =
    b"\x17\xee\x8c\x60\xf8\x4d\x11\xd9\x8c\xd6\x08\x00\x20\x0c\x9a\x66MDPM";

/// `ProcessSEI`: find the type-5 unregistered user data payload and, if it is
/// an MDPM block, decode its records.
fn parse_sei(rbsp: &[u8], metadata: &mut MetadataMap) -> bool {
    let mut pos = 0usize;

    loop {
        // Both payload type and size are encoded as a run of 0xff bytes
        // followed by a final byte, all summed.
        let Some(payload_type) = read_sei_varint(rbsp, &mut pos) else {
            return false;
        };
        if payload_type == 0x80 {
            return false; // terminator
        }
        let Some(size) = read_sei_varint(rbsp, &mut pos) else {
            return false;
        };
        if pos + size > rbsp.len() {
            return false;
        }
        if payload_type == 5 {
            break;
        }
        pos += size;
    }

    if !rbsp[pos..].starts_with(MDPM_SIGNATURE) {
        return false;
    }
    parse_mdpm(&rbsp[pos + MDPM_SIGNATURE.len()..], metadata);
    true
}

/// SEI payload type and size use the same "sum bytes until one is not 0xff"
/// encoding.
fn read_sei_varint(data: &[u8], pos: &mut usize) -> Option<usize> {
    let mut total = 0usize;
    loop {
        let byte = *data.get(*pos)?;
        *pos += 1;
        total += byte as usize;
        if byte != 0xff {
            return Some(total);
        }
    }
}

/// Walk the MDPM records: a count byte followed by 5-byte (tag, value) entries
/// in ascending tag order.
fn parse_mdpm(data: &[u8], metadata: &mut MetadataMap) {
    let Some(&count) = data.first() else {
        return;
    };
    let mut pos = 1usize;
    let mut last_tag: Option<u8> = None;
    let mut make: Option<&'static str> = None;

    let mut index = 0u8;
    while index < count && pos + 5 <= data.len() {
        let tag = data[pos];
        // ExifTool bails out here rather than trying to resynchronise.
        if let Some(previous) = last_tag
            && tag <= previous
        {
            return;
        }
        last_tag = Some(tag);
        let value: [u8; 4] = [data[pos + 1], data[pos + 2], data[pos + 3], data[pos + 4]];

        match tag {
            // Combined with the following record (0x19) into one timestamp.
            0x18 => {
                if pos + 10 <= data.len() && data[pos + 5] == 0x19 {
                    let rest = &data[pos + 6..pos + 10];
                    if let Some(stamp) = mdpm_date_time_original(&value, rest) {
                        metadata.insert(
                            "H264:DateTimeOriginal".to_string(),
                            TagValue::new_string(stamp),
                        );
                    }
                    pos += 5;
                    index += 1;
                    last_tag = Some(0x19);
                }
            }
            0x70 => decode_camera1(&value, metadata),
            0x71 => decode_camera2(&value, metadata),
            0x7f => decode_shutter(&value, metadata),
            0xe0 => make = decode_make_model(&value, metadata),
            // RecInfo is Canon-only; ExifTool gates it on the Make just
            // decoded from record 0xe0.
            0xe1 if make == Some("Canon") => decode_rec_info(&value, metadata),
            _ => {}
        }

        pos += 5;
        index += 1;
    }
}

/// MDPM 0x18 + 0x19: a timezone byte followed by BCD date and time fields.
fn mdpm_date_time_original(head: &[u8; 4], tail: &[u8]) -> Option<String> {
    if tail.len() < 4 {
        return None;
    }
    let tz = head[0];
    // The remaining seven bytes are BCD: century, year, month, day, hour,
    // minute, second.
    let bcd = [
        head[1], head[2], head[3], tail[0], tail[1], tail[2], tail[3],
    ];
    let sign = if tz & 0x20 != 0 { '-' } else { '+' };
    let hours = (tz >> 1) & 0x0f;
    let minutes = if tz & 0x01 != 0 { "30" } else { "00" };
    let dst = if tz & 0x40 != 0 { " DST" } else { "" };

    Some(format!(
        "{:02x}{:02x}:{:02x}:{:02x} {:02x}:{:02x}:{:02x}{}{:02}:{}{}",
        bcd[0], bcd[1], bcd[2], bcd[3], bcd[4], bcd[5], bcd[6], sign, hours, minutes, dst
    ))
}

/// MDPM 0x70 (`ConsumerCamera1`), big-endian binary data.
fn decode_camera1(value: &[u8; 4], metadata: &mut MetadataMap) {
    let aperture = value[0];
    let aperture_text = match aperture {
        0xff => "Auto".to_string(),
        0xfe => "Closed".to_string(),
        // ExifTool's OTHER handler: the low 6 bits are an eighth-stop index.
        other => format!("{:.1}", 2f64.powf(f64::from(other & 0x3f) / 8.0)),
    };
    metadata.insert(
        "H264:ApertureSetting".to_string(),
        TagValue::new_string(aperture_text),
    );

    let gain_code = i32::from(value[1] & 0x0f);
    let gain = (gain_code - 1) * 3;
    metadata.insert(
        "H264:Gain".to_string(),
        TagValue::new_string(if gain == 42 {
            // 0x0f would decode to 42 dB, but cameras use it for any
            // out-of-range value, so it must not be reported as a real gain.
            "Out of range".to_string()
        } else {
            format!("{} dB", gain)
        }),
    );

    // 15 means "not recorded" for both of the following.
    let exposure_program = (value[1] & 0xf0) >> 4;
    if exposure_program != 15 {
        metadata.insert(
            "H264:ExposureProgram".to_string(),
            TagValue::new_string(match exposure_program {
                0 => "Program AE".to_string(),
                1 => "Gain".to_string(),
                2 => "Shutter speed priority AE".to_string(),
                3 => "Aperture-priority AE".to_string(),
                4 => "Manual".to_string(),
                other => format!("Unknown ({})", other),
            }),
        );
    }

    let white_balance = (value[2] & 0xe0) >> 5;
    if white_balance != 7 {
        metadata.insert(
            "H264:WhiteBalance".to_string(),
            TagValue::new_string(match white_balance {
                0 => "Auto".to_string(),
                1 => "Hold".to_string(),
                2 => "1-Push".to_string(),
                3 => "Daylight".to_string(),
                other => format!("Unknown ({})", other),
            }),
        );
    }

    let focus = value[3];
    if focus != 0xff {
        let divisor = if focus & 0x01 != 0 { 40.0 } else { 400.0 };
        let distance = f64::from(focus & 0x7e) / divisor;
        let mode = if focus & 0x80 != 0 { "Manual" } else { "Auto" };
        metadata.insert(
            "H264:Focus".to_string(),
            TagValue::new_string(format!("{} ({})", mode, format_perl_number(distance))),
        );
    }
}

/// MDPM 0x71 (`ConsumerCamera2`).
fn decode_camera2(value: &[u8; 4], metadata: &mut MetadataMap) {
    let code = value[1];
    let text = match code {
        0x00 => "Off".to_string(),
        0x3f => "On (0x3f)".to_string(),
        0xbf => "Off (0xbf)".to_string(),
        0xff => "n/a".to_string(),
        other => format!(
            "{} (0x{:02x})",
            if other & 0x10 != 0 { "On" } else { "Off" },
            other
        ),
    };
    metadata.insert(
        "H264:ImageStabilization".to_string(),
        TagValue::new_string(text),
    );
}

/// MDPM 0x7f (`Shutter`). This block is little-endian -- ExifTool calls that
/// out as "weird", but it is what the cameras write.
fn decode_shutter(value: &[u8; 4], metadata: &mut MetadataMap) {
    let raw = u16::from_le_bytes([value[2], value[3]]) & 0x7fff;
    if raw == 0x7fff {
        return; // sentinel for "not recorded"
    }
    let seconds = f64::from(raw) / 28125.0;
    metadata.insert(
        "H264:ExposureTime".to_string(),
        TagValue::new_string(print_exposure_time(seconds)),
    );
}

/// MDPM 0xe0 (`MakeModel`). Returns the decoded make so the caller can gate
/// the manufacturer-specific records that follow.
fn decode_make_model(value: &[u8; 4], metadata: &mut MetadataMap) -> Option<&'static str> {
    let code = u16::from_be_bytes([value[0], value[1]]);
    let make = match code {
        0x0103 => Some("Panasonic"),
        0x0108 => Some("Sony"),
        0x1011 => Some("Canon"),
        0x1104 => Some("JVC"),
        _ => None,
    };
    metadata.insert(
        "H264:Make".to_string(),
        TagValue::new_string(
            make.map(str::to_string)
                .unwrap_or_else(|| format!("Unknown (0x{:x})", code)),
        ),
    );
    make
}

/// MDPM 0xe1 (`RecInfo`), written by some Canon camcorders.
fn decode_rec_info(value: &[u8; 4], metadata: &mut MetadataMap) {
    let text = match value[0] {
        0x02 => "XP+".to_string(), // High Quality 12 Mbps
        0x04 => "SP".to_string(),  // Standard Play 7 Mbps
        0x05 => "LP".to_string(),  // Long Play 5 Mbps
        0x06 => "FXP".to_string(), // High Quality 17 Mbps
        0x07 => "MXP".to_string(), // High Quality 24 Mbps
        other => format!("Unknown ({})", other),
    };
    metadata.insert("H264:RecordingMode".to_string(), TagValue::new_string(text));
}

/// ExifTool's `Exif::PrintExposureTime`.
fn print_exposure_time(seconds: f64) -> String {
    if seconds > 0.0 && seconds < 0.25001 {
        return format!("1/{}", (0.5 + 1.0 / seconds) as i64);
    }
    let rendered = format!("{:.1}", seconds);
    rendered
        .strip_suffix(".0")
        .map(str::to_string)
        .unwrap_or(rendered)
}

/// Perl prints a float with as few digits as round-trip requires; Rust's
/// default `{}` for f64 does the same, but renders whole values as "3" rather
/// than "3.0" only after trimming.
fn format_perl_number(value: f64) -> String {
    let rendered = format!("{}", value);
    rendered
        .strip_suffix(".0")
        .map(str::to_string)
        .unwrap_or(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// The exact SPS from the M2TS.mts fixture, which ExifTool reports as
    /// 1920x1080.
    const FIXTURE_SPS: &[u8] = &[
        0x67, 0x64, 0x00, 0x28, 0xad, 0x00, 0xec, 0x07, 0x80, 0x44, 0x7d, 0xe0, 0x22, 0x00, 0x00,
        0x03, 0x00, 0x02, 0x00, 0x00, 0x03, 0x00, 0x65, 0xd1, 0x40, 0x01, 0xe8, 0x48, 0x00, 0x3d,
        0x09, 0x3e, 0xf7, 0x06, 0x8a,
    ];

    #[test]
    fn test_sequence_parameter_set_image_size() {
        let mut stream = vec![0x00, 0x00, 0x00, 0x01];
        stream.extend_from_slice(FIXTURE_SPS);
        let mut metadata = MetadataMap::new();
        parse_h264_stream(&stream, &mut metadata);
        assert_eq!(shown(&metadata, "H264:ImageWidth"), "1920");
        assert_eq!(shown(&metadata, "H264:ImageHeight"), "1080");
    }

    /// Emulation-prevention bytes must be stripped before the RBSP is read,
    /// or every field after the first `00 00 03` decodes at the wrong offset.
    #[test]
    fn test_emulation_prevention_removed() {
        assert_eq!(
            remove_emulation_prevention(&[0x00, 0x00, 0x03, 0x01]),
            vec![0x00, 0x00, 0x01]
        );
        // Only a 0x03 following two zeros is an escape.
        assert_eq!(
            remove_emulation_prevention(&[0x00, 0x03, 0x00, 0x03]),
            vec![0x00, 0x03, 0x00, 0x03]
        );
    }

    /// Build an SEI NAL carrying the MDPM records from the M2TS.mts fixture.
    fn fixture_sei() -> Vec<u8> {
        let records: [(u8, [u8; 4]); 7] = [
            (0x18, [0x02, 0x20, 0x08, 0x12]),
            (0x19, [0x31, 0x23, 0x16, 0x24]),
            (0x70, [0xca, 0xf4, 0xff, 0xff]),
            (0x71, [0xff, 0xff, 0xff, 0xff]),
            (0x7f, [0x00, 0x00, 0x65, 0x84]),
            (0xe0, [0x10, 0x11, 0x30, 0x00]),
            (0xe1, [0x06, 0xff, 0xff, 0xff]),
        ];
        let mut payload = MDPM_SIGNATURE.to_vec();
        payload.push(records.len() as u8);
        for (tag, value) in records {
            payload.push(tag);
            payload.extend_from_slice(&value);
        }

        let mut nal = vec![0x00, 0x00, 0x00, 0x01, 0x06]; // start code + SEI
        nal.push(5); // payload type: unregistered user data
        nal.push(payload.len() as u8);
        nal.extend_from_slice(&payload);
        nal
    }

    #[test]
    fn test_mdpm_records_match_exiftool() {
        let mut metadata = MetadataMap::new();
        assert!(parse_h264_stream(&fixture_sei(), &mut metadata));

        assert_eq!(
            shown(&metadata, "H264:DateTimeOriginal"),
            "2008:12:31 23:16:24+01:00"
        );
        assert_eq!(shown(&metadata, "H264:ApertureSetting"), "2.4");
        assert_eq!(shown(&metadata, "H264:Gain"), "9 dB");
        assert_eq!(shown(&metadata, "H264:ImageStabilization"), "n/a");
        assert_eq!(shown(&metadata, "H264:ExposureTime"), "1/25");
        assert_eq!(shown(&metadata, "H264:Make"), "Canon");
        assert_eq!(shown(&metadata, "H264:RecordingMode"), "FXP");

        // Sentinel values must suppress the tag, not be reported as data.
        assert!(metadata.get("H264:ExposureProgram").is_none());
        assert!(metadata.get("H264:WhiteBalance").is_none());
        assert!(metadata.get("H264:Focus").is_none());
    }

    /// An SEI whose payload is not MDPM must not be mistaken for one.
    #[test]
    fn test_non_mdpm_user_data_ignored() {
        let mut nal = vec![0x00, 0x00, 0x00, 0x01, 0x06, 5, 20];
        nal.extend_from_slice(&[0xaa; 20]);
        let mut metadata = MetadataMap::new();
        assert!(!parse_h264_stream(&nal, &mut metadata));
        assert!(metadata.is_empty());
    }

    /// Codes outside each PrintConv table must report themselves rather than
    /// borrowing a neighbouring label.
    #[test]
    fn test_unknown_codes_report_themselves() {
        let mut metadata = MetadataMap::new();
        decode_rec_info(&[0x03, 0, 0, 0], &mut metadata);
        assert_eq!(shown(&metadata, "H264:RecordingMode"), "Unknown (3)");

        let mut metadata = MetadataMap::new();
        assert_eq!(decode_make_model(&[0x12, 0x34, 0, 0], &mut metadata), None);
        assert_eq!(shown(&metadata, "H264:Make"), "Unknown (0x1234)");

        let mut metadata = MetadataMap::new();
        decode_camera2(&[0x00, 0x10, 0, 0], &mut metadata);
        assert_eq!(shown(&metadata, "H264:ImageStabilization"), "On (0x10)");

        let mut metadata = MetadataMap::new();
        decode_camera2(&[0x00, 0x20, 0, 0], &mut metadata);
        assert_eq!(shown(&metadata, "H264:ImageStabilization"), "Off (0x20)");
    }

    /// RecInfo is Canon-only: a Sony MakeModel record must leave it undecoded.
    #[test]
    fn test_rec_info_is_canon_only() {
        let mut payload = MDPM_SIGNATURE.to_vec();
        payload.push(2);
        payload.extend_from_slice(&[0xe0, 0x01, 0x08, 0x30, 0x00]); // Sony
        payload.extend_from_slice(&[0xe1, 0x06, 0xff, 0xff, 0xff]);

        let mut nal = vec![0x00, 0x00, 0x00, 0x01, 0x06, 5, payload.len() as u8];
        nal.extend_from_slice(&payload);

        let mut metadata = MetadataMap::new();
        parse_h264_stream(&nal, &mut metadata);
        assert_eq!(shown(&metadata, "H264:Make"), "Sony");
        assert!(metadata.get("H264:RecordingMode").is_none());
    }

    #[test]
    fn test_camera1_optional_fields() {
        // ExposureProgram 4 (Manual), WhiteBalance 3 (Daylight), auto focus.
        let mut metadata = MetadataMap::new();
        decode_camera1(&[0xff, 0x41, 0x60, 0x28], &mut metadata);
        assert_eq!(shown(&metadata, "H264:ApertureSetting"), "Auto");
        assert_eq!(shown(&metadata, "H264:ExposureProgram"), "Manual");
        assert_eq!(shown(&metadata, "H264:WhiteBalance"), "Daylight");
        assert_eq!(shown(&metadata, "H264:Focus"), "Auto (0.1)");
        // Gain code 1 decodes to 0 dB.
        assert_eq!(shown(&metadata, "H264:Gain"), "0 dB");
    }

    #[test]
    fn test_print_exposure_time() {
        assert_eq!(print_exposure_time(0.04), "1/25");
        assert_eq!(print_exposure_time(1.0), "1");
        assert_eq!(print_exposure_time(2.5), "2.5");
    }
}
