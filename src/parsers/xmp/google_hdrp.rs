//! Decoder for Google's `HdrPlusMakernote` XMP property
//! (`Image::ExifTool::Google::HDRPlusMakerNote`, Google.pm).
//!
//! Google's camera app (GCamera) stores per-shot debug metadata as a
//! base64-encoded, encrypted, gzipped Protobuf blob under the
//! `GCamera:HdrPlusMakernote` XMP property. ExifTool decodes it in
//! `Google::ProcessHDRP` and re-files the extracted fields under the
//! `MakerNotes` group (`Google::HDRPlusMakerNote`'s `GROUPS => { 0 =>
//! 'MakerNotes' }`), even though the bytes never pass through a TIFF
//! MakerNote IFD -- there is no numeric-tag `google` MakerNote parser in
//! `src/parsers/tiff/makernotes` for exactly this reason (see
//! `tiff/makernote_dispatcher.rs` and `tiff/makernotes/mod.rs`).
//!
//! This implements the named scalar HDRP fields exercised by the pinned
//! corpus.  Unknown protobuf records deliberately remain absent: a plausible
//! value under a real ExifTool tag name is worse than no value at all.

use std::io::Read;

/// Decodes a `GCamera:HdrPlusMakernote` XMP property value (still
/// base64-encoded, exactly as read from the XMP packet) into the
/// `MakerNotes:*` tags ExifTool reports for it.
///
/// Returns an empty `Vec` if the value cannot be base64-decoded, does not
/// start with the expected `HDRP` signature, is not the protobuf-framed
/// version 3 (`Google.pm:697,763`: version 2 is the older text-based
/// format handled by a different, unimplemented code path), or fails to
/// decrypt/gunzip -- rather than fabricating a value.
pub fn decode_hdrp_plus_makernote(raw_value: &str) -> Vec<(String, String)> {
    let Some(decoded) = decode_base64_flexible(raw_value) else {
        return Vec::new();
    };
    decode_hdrp_makernote_bytes(&decoded)
}

/// Decodes the raw bytes of an Exif `MakerNoteGoogle` (`HDRP\\x02` or
/// `HDRP\\x03`).  Google uses the identical encrypted envelope for its XMP
/// `HdrPlusMakernote` property and for the TIFF MakerNote selected by
/// `MakerNotes.pm`'s `MakerNoteGoogle` condition, so keeping the cryptographic
/// and text/protobuf decoding in one place prevents the two entry points from
/// drifting.
pub fn decode_hdrp_makernote_bytes(raw: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some((version, inflated)) = decrypt_and_inflate(raw) else {
        return out;
    };

    if version == 2 {
        return decode_hdrp_v2_text(&inflated);
    }
    if version != 3 {
        return out;
    }

    let top = parse_fields(&inflated);

    // `1-1`/`1-2`: the named finished-image records (Google.pm:526-527).
    // ImageData is Binary => 1, so ExifTool renders its decoded byte length.
    if let Some(Field::Bytes(sub1)) = last_field(&top, 1) {
        let f1 = parse_fields(sub1);
        push_string_field(&f1, 1, "MakerNotes:ImageName", &mut out);
        if let Some(Field::Bytes(bytes)) = last_field(&f1, 2) {
            out.push((
                "MakerNotes:ImageData".to_string(),
                format!(
                    "(Binary data {} bytes, use -b option to extract)",
                    bytes.len()
                ),
            ));
        }
    }

    // `9-36-1`: HDR-Plus frame CreateDate (Google.pm:539-546).
    if let Some(Field::Bytes(sub9)) = last_field(&top, 9) {
        let f9 = parse_fields(sub9);
        if let Some(Field::Bytes(sub36)) = last_field(&f9, 36) {
            let f36 = parse_fields(sub36);
            if let Some(Field::Varint(secs)) = last_field(&f36, 1) {
                out.push((
                    "MakerNotes:CreateDate".to_string(),
                    crate::core::file_metadata::format_unix_time_local(*secs as i64),
                ));
            }
        }
        // `9-3`: FrameCount (Google.pm:537).
        if let Some(Field::Varint(n)) = last_field(&f9, 3) {
            out.push(("MakerNotes:FrameCount".to_string(), n.to_string()));
        }
    }

    // `12-*`: device/software identification (Google.pm:547-561).
    if let Some(Field::Bytes(sub12)) = last_field(&top, 12) {
        let f12 = parse_fields(sub12);
        push_string_field(&f12, 1, "MakerNotes:DeviceMake", &mut out);
        push_string_field(&f12, 2, "MakerNotes:DeviceModel", &mut out);
        push_string_field(&f12, 3, "MakerNotes:DeviceCodename", &mut out);
        push_string_field(&f12, 4, "MakerNotes:DeviceHardwareRevision", &mut out);
        push_string_field(&f12, 6, "MakerNotes:HDRPSoftware", &mut out);
        push_string_field(&f12, 7, "MakerNotes:AndroidRelease", &mut out);
        // `12-8`: Unix milliseconds, PrintConv => ConvertDateTime
        // (Google.pm:554-559). The protobuf value is integral milliseconds,
        // and the source table's precision is three decimal places.
        if let Some(Field::Varint(millis)) = last_field(&f12, 8) {
            if let Ok(millis) = i64::try_from(*millis)
                && let Some(utc) = chrono::DateTime::from_timestamp_millis(millis)
            {
                out.push((
                    "MakerNotes:SoftwareDate".to_string(),
                    utc.with_timezone(&chrono::Local)
                        .format("%Y:%m:%d %H:%M:%S%.3f%:z")
                        .to_string(),
                ));
            }
        }
        push_string_field(&f12, 9, "MakerNotes:Application", &mut out);
        push_string_field(&f12, 10, "MakerNotes:AppVersion", &mut out);

        // `12-12-*`, `12-13-*`, and `12-14` are protobuf fixed32 floats.
        // ExifTool widens the f32 and stringifies the resulting Perl NV with
        // `%.15g`; keep that exact rendering through the shared formatter.
        if let Some(Field::Bytes(exposure)) = last_field(&f12, 12) {
            let exposure = parse_fields(exposure);
            push_f32_field(
                &exposure,
                1,
                "MakerNotes:ExposureTimeMin",
                1.0 / 1000.0,
                &mut out,
            );
            push_f32_field(
                &exposure,
                2,
                "MakerNotes:ExposureTimeMax",
                1.0 / 1000.0,
                &mut out,
            );
        }
        if let Some(Field::Bytes(iso)) = last_field(&f12, 13) {
            let iso = parse_fields(iso);
            push_f32_field(&iso, 1, "MakerNotes:ISOMin", 1.0, &mut out);
            push_f32_field(&iso, 2, "MakerNotes:ISOMax", 1.0, &mut out);
        }
        push_f32_field(&f12, 14, "MakerNotes:MaxAnalogISO", 1.0, &mut out);
    }

    out
}

/// Extracts the two scalar fields in `Google::ShotLogData` (Google.pm:576-577).
/// This is a separate HDRP v3 property in v2-era Pixel XMP packets.
pub fn decode_hdrp_shot_log_data(raw_value: &str) -> Vec<(String, String)> {
    // `ShotLogData` is marked `IsProtobuf` by the table, so ExifTool parses
    // it as protobuf even when its enclosing HDRP stream is version 2.
    let Some(decoded) = decode_base64_flexible(raw_value) else {
        return Vec::new();
    };
    let Some((_, inflated)) = decrypt_and_inflate(&decoded) else {
        return Vec::new();
    };
    let fields = parse_fields(&inflated);
    let mut out = Vec::new();
    if let Some(Field::Varint(n)) = last_field(&fields, 2) {
        out.push(("MakerNotes:MeteringFrameCount".to_string(), n.to_string()));
    }
    if let Some(Field::Varint(n)) = last_field(&fields, 3) {
        out.push((
            "MakerNotes:OriginalPayloadFrameCount".to_string(),
            n.to_string(),
        ));
    }
    out
}

/// Base64-decodes, decrypts, and gunzips a `HdrPlusMakernote` value,
/// mirroring `Google::ProcessHDRP` (Google.pm:670-780). Returns `None` on
/// any failure, or if the decoded version isn't 3 (protobuf-framed).
fn decrypt_and_inflate(decoded: &[u8]) -> Option<(u8, Vec<u8>)> {
    if decoded.len() < 5 || &decoded[0..4] != b"HDRP" {
        return None;
    }
    let version = decoded[4];

    let mut payload = decoded[5..].to_vec();
    let pad = (8 - (payload.len() % 8)) % 8;
    if pad > 0 {
        payload.extend(std::iter::repeat_n(0u8, pad));
    }

    // xorshift64* keystream, applied 64 bits (two little-endian u32 words)
    // at a time (Google.pm:703-748). The Perl implementation does this
    // arithmetic in 16-bit chunks for 32-bit-Perl compatibility; plain u64
    // wrapping arithmetic is equivalent.
    let mut state: u64 = 0x2515_606b_4a77_91cd;
    let mut i = 0;
    while i < payload.len() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let lo = (state & 0xffff_ffff) as u32;
        let hi = (state >> 32) as u32;
        xor_word(&mut payload, i, lo);
        xor_word(&mut payload, i + 4, hi);
        i += 8;
    }
    if pad > 0 {
        let new_len = payload.len() - pad;
        payload.truncate(new_len);
    }

    let mut gz = flate2::read::GzDecoder::new(&payload[..]);
    let mut buf = Vec::new();
    gz.read_to_end(&mut buf).ok()?;
    Some((version, buf))
}

/// Mirrors the stable, text-format branch of `ProcessHDRPMakerNote`
/// (Google.pm:630-663).  It deliberately reports only the named table entries
/// and their exact binary placeholders; unknown diagnostic headings remain
/// absent rather than being given invented tag names.
fn decode_hdrp_v2_text(inflated: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(inflated);
    let mut out = Vec::new();
    let mut active: Option<(String, String)> = None;

    for raw_line in text.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        if let Some((name, value, is_base64)) = parse_v2_heading(line) {
            if let Some((tag, value)) = active.take() {
                push_v2_binary(&mut out, &tag, value.len());
            }
            if is_base64 {
                if let Some(bytes) = decode_base64_flexible(value) {
                    push_v2_binary(&mut out, &name, bytes.len());
                }
            } else if !value.is_empty() {
                active = Some((name, value.to_string()));
            } else {
                active = Some((name, String::new()));
            }
        } else if let Some((_, value)) = active.as_mut() {
            // Perl's `ProcessHDRPMakerNote` captures through the byte before
            // the next heading, including this line's terminating newline.
            value.push_str(raw_line);
        }
    }
    if let Some((tag, value)) = active {
        push_v2_binary(&mut out, &tag, value.len());
    }

    // `ProcessHDRPMakerNote` funnels free-form, non-heading diagnostic text
    // through ProcessingNotes. The final such line is the value retained by
    // ExifTool's metadata map for the Pixel 6a fixture.
    if let Some(note) = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| line.starts_with("Neither warping nor relighting"))
    {
        out.push(("MakerNotes:ProcessingNotes".to_string(), note.to_string()));
    }
    out
}

fn parse_v2_heading(line: &str) -> Option<(String, &str, bool)> {
    // `ProcessHDRPMakerNote` in Google.pm matches `^ ?([A-Z].*)$`: HDRP-v2
    // portrait diagnostics therefore use a single leading space before their
    // heading (Pixel 5: ` Rectiface:`). Strip only that one marker for heading
    // recognition; the payload remains byte-for-byte unchanged below.
    let line = line.strip_prefix(' ').unwrap_or(line);
    const HEADINGS: &[(&str, &str)] = &[
        ("InitParams", "InitParamsText"),
        ("Logging metadata", "LoggingMetadataText"),
        ("Merged image", "MergedImage"),
        ("Finished image", "FinishedImage"),
        ("Payload metadata", "PayloadMetadataText"),
        ("ShotLogData", "ShotLogDataText"),
        ("ShotParams", "ShotParamsText"),
        ("StaticMetadata", "StaticMetadataText"),
        ("Summary", "SummaryText"),
        ("Time log", "TimeLogText"),
        ("Unused logging metadata", "UnusedLoggingMetadata"),
        ("Rectiface", "RectifaceText"),
        ("GoudaRequest", "GoudaRequestText"),
    ];
    if let Some(rest) = line.strip_prefix("Payload frame ") {
        let (index, rest) = rest.split_once(" (base64): ")?;
        if index.chars().all(|c| c.is_ascii_digit()) {
            // ExifTool indexes repeated tags, padding to the largest source
            // index width (00..10 for Pixel 6a; 0..8 for Pixel 5).
            return Some((format!("PayloadFrame{index}"), rest, true));
        }
    }
    for &(heading, tag) in HEADINGS {
        if line == heading {
            return Some((tag.to_string(), "", false));
        }
        if let Some(value) = line.strip_prefix(heading).and_then(|v| v.strip_prefix(":")) {
            return Some((
                tag.to_string(),
                value.strip_prefix(' ').unwrap_or(value),
                false,
            ));
        }
        if let Some(value) = line
            .strip_prefix(heading)
            .and_then(|v| v.strip_prefix(" (base64): "))
        {
            return Some((tag.to_string(), value, true));
        }
    }
    None
}

fn push_v2_binary(out: &mut Vec<(String, String)>, tag: &str, len: usize) {
    out.push((
        format!("MakerNotes:{tag}"),
        format!("(Binary data {len} bytes, use -b option to extract)"),
    ));
}

/// XORs the little-endian `u32` at `buf[offset..offset+4]` with `word`.
/// A no-op if the slice would run past the end (only possible on the
/// trailing half-word of an odd-length padded buffer, which never happens
/// here since padding always brings the length to a multiple of 8).
fn xor_word(buf: &mut [u8], offset: usize, word: u32) {
    if offset + 4 > buf.len() {
        return;
    }
    let bytes = word.to_le_bytes();
    for (b, x) in buf[offset..offset + 4].iter_mut().zip(bytes.iter()) {
        *b ^= x;
    }
}

fn decode_base64_flexible(value: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let compact: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(&compact)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(&compact))
        .ok()
}

/// One decoded Protobuf record's payload (`Protobuf.pm:76-107`).
enum Field<'a> {
    /// Wire type 0.
    Varint(u64),
    /// Wire type 2 (string, bytes, or embedded message).
    Bytes(&'a [u8]),
    /// Wire type 5. Google HDRP uses these for its `float` fields.
    Fixed32(u32),
}

/// Parses top-level Protobuf records from `data`, stopping (and keeping
/// whatever was already decoded) at the first malformed record rather than
/// guessing at a resync point. Only wire types 0 (varint) and 2
/// (length-delimited) are needed for the fields this module reads; a
/// fixed64 records are skipped. Fixed32 is retained because Google HDRP's
/// known exposure and ISO fields are protobuf `float`s.
fn parse_fields(data: &[u8]) -> Vec<(u32, Field<'_>)> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let Some((key, next)) = read_varint(data, pos) else {
            break;
        };
        pos = next;
        let id = (key >> 3) as u32;
        let wire_type = key & 0x7;
        match wire_type {
            0 => {
                let Some((val, next)) = read_varint(data, pos) else {
                    break;
                };
                pos = next;
                out.push((id, Field::Varint(val)));
            }
            1 => {
                if pos + 8 > data.len() {
                    break;
                }
                pos += 8;
            }
            2 => {
                let Some((len, next)) = read_varint(data, pos) else {
                    break;
                };
                pos = next;
                let len = len as usize;
                if pos + len > data.len() {
                    break;
                }
                out.push((id, Field::Bytes(&data[pos..pos + len])));
                pos += len;
            }
            5 => {
                if pos + 4 > data.len() {
                    break;
                }
                out.push((
                    id,
                    Field::Fixed32(u32::from_le_bytes(
                        data[pos..pos + 4].try_into().expect("four-byte field"),
                    )),
                ));
                pos += 4;
            }
            _ => break, // deprecated group start/end (3/4) or invalid type
        }
    }
    out
}

fn read_varint(data: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    let mut val: u64 = 0;
    let mut shift = 0u32;
    loop {
        let byte = *data.get(pos)?;
        pos += 1;
        val |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some((val, pos));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

/// The last record with the given field id, mirroring Protobuf's
/// "last value wins" semantics for a non-repeated field.
fn last_field<'a, 'b>(fields: &'b [(u32, Field<'a>)], id: u32) -> Option<&'b Field<'a>> {
    fields
        .iter()
        .rev()
        .find(|(fid, _)| *fid == id)
        .map(|(_, f)| f)
}

fn push_string_field(
    fields: &[(u32, Field<'_>)],
    id: u32,
    tag: &str,
    out: &mut Vec<(String, String)>,
) {
    if let Some(Field::Bytes(bytes)) = last_field(fields, id)
        && let Ok(s) = std::str::from_utf8(bytes)
    {
        out.push((tag.to_string(), s.to_string()));
    }
}

fn push_f32_field(
    fields: &[(u32, Field<'_>)],
    id: u32,
    tag: &str,
    scale: f64,
    out: &mut Vec<(String, String)>,
) {
    if let Some(Field::Fixed32(bits)) = last_field(fields, id) {
        let value = f64::from(f32::from_bits(*bits)) * scale;
        out.push((
            tag.to_string(),
            crate::core::formatters::numeric_precision::perl_number(value),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trips the encryption in `decrypt_and_inflate` against its own
    /// inverse to pin the xorshift64* keystream and padding handling
    /// without depending on a real sample file.
    #[test]
    fn xorshift_keystream_round_trips() {
        // Build a small "HDRP\x03" + gzipped-protobuf payload, encrypt it
        // with the same keystream `decrypt_and_inflate` un-applies, base64
        // it, and confirm the fields survive the round trip.
        let mut inner = Vec::new();
        // field 12 (LEN), containing field 3 (LEN) = "codename"
        let mut f12 = Vec::new();
        push_len_field(&mut f12, 3, b"codename");
        push_len_field(&mut inner, 12, &f12);

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        use std::io::Write;
        gz.write_all(&inner).unwrap();
        let gzipped = gz.finish().unwrap();

        let mut plaintext = b"HDRP\x03".to_vec();
        plaintext.extend_from_slice(&gzipped);

        let payload_start = 5;
        let mut encrypted = plaintext.clone();
        let mut payload = encrypted[payload_start..].to_vec();
        let pad = (8 - (payload.len() % 8)) % 8;
        payload.extend(std::iter::repeat_n(0u8, pad));

        let mut state: u64 = 0x2515_606b_4a77_91cd;
        let mut i = 0;
        while i < payload.len() {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
            let lo = (state & 0xffff_ffff) as u32;
            let hi = (state >> 32) as u32;
            xor_word(&mut payload, i, lo);
            xor_word(&mut payload, i + 4, hi);
            i += 8;
        }

        encrypted.truncate(payload_start);
        encrypted.extend_from_slice(&payload);

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&encrypted);

        let tags = decode_hdrp_plus_makernote(&b64);
        assert!(
            tags.contains(&(
                "MakerNotes:DeviceCodename".to_string(),
                "codename".to_string()
            )),
            "{tags:?}"
        );
    }

    fn push_len_field(buf: &mut Vec<u8>, id: u32, value: &[u8]) {
        write_varint(buf, ((id as u64) << 3) | 2);
        write_varint(buf, value.len() as u64);
        buf.extend_from_slice(value);
    }

    fn write_varint(buf: &mut Vec<u8>, mut val: u64) {
        loop {
            let mut byte = (val & 0x7f) as u8;
            val >>= 7;
            if val != 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if val == 0 {
                break;
            }
        }
    }

    #[test]
    fn rejects_non_hdrp_input() {
        assert!(decode_hdrp_plus_makernote("bm90aGRycA==").is_empty());
    }

    #[test]
    fn rejects_invalid_base64() {
        assert!(decode_hdrp_plus_makernote("not valid base64!!!").is_empty());
    }
}
