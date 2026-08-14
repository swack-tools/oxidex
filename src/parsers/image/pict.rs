//! PICT (Macintosh QuickDraw Picture) metadata parser.
//!
//! ExifTool routes a `.pict` file through `Image::ExifTool::PICT::ProcessPICT`
//! (`PICT.pm:1082-1149`). Only the header is read in non-verbose mode (the
//! default -- this parser matches that): a `PICT` *file* prepends a 512-byte
//! header a `PICT` *resource* does not, so ExifTool tries the first 12 bytes
//! as-is and, on a miss, retries once after seeking past that 512-byte pad
//! (`PICT.pm:1092-1116`).
//!
//! Those 12 bytes are a 2-byte picture size (ignored) followed by five
//! big-endian `u16`s: a QuickDraw `Rect` (`top`, `left`, `bottom`, `right`)
//! and a version opcode. `0x1101` is version 1; `0x0011` means version 2 and
//! gates a further 28-byte read whose first six bytes distinguish plain
//! version 2 (`\x02\xff\x0c\x00\xff\xff`) from *extended* version 2
//! (`\x02\xff\x0c\x00\xff\xfe`), which carries an `XResolution`/`YResolution`
//! pair as 16.16 fixed-point values at that buffer's own offsets 8 and 12
//! (`PICT.pm:1130-1131`, `GetFixed32s`).
//!
//! `ImageWidth`/`ImageHeight` are `right-left`/`bottom-top` from the `Rect`
//! (each treated as signed 16-bit, `PICT.pm:1119-1120`); for an extended
//! header they are further rescaled from the 72-dpi bounding box to the
//! declared resolution (`PICT.pm:1133-1135`,
//! `int($w * $hRes / 72 + 0.5)`).
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/PICT.pm`

use crate::core::{FileReader, MetadataMap, TagValue};

/// `PICT.pm:1091`: the version 1/2 opcode probe.
const HEADER_LEN: usize = 12;
/// `PICT.pm:1104`: the second read once a version-2 opcode is seen.
const V2_EXTRA_LEN: usize = 28;
/// `PICT.pm:1115`: a `PICT` *file* (as opposed to a `PICT` *resource*)
/// prepends this many bytes of header ExifTool ignores.
const FILE_HEADER_PAD: usize = 512;

#[derive(Clone, Copy)]
struct Header {
    width: f64,
    height: f64,
    h_res: Option<f64>,
    v_res: Option<f64>,
}

/// Neither `XResolution` nor `YResolution` carries a `PrintConv`
/// (`PICT.pm:1132-1133`), so ExifTool prints Perl's own default numeric
/// stringification -- no trailing `.00` for the overwhelmingly common case
/// of a whole DPI value. A hard-coded two decimals would turn a correct
/// number into a value mismatch (the same fix already applied to PSD
/// resolution, `parsers::image::psd::format_psd_resolution`).
fn format_whole_or_decimal(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// `GetFixed32s`: a signed 16.16 fixed-point value, big-endian.
fn fixed_32s(bytes: &[u8]) -> f64 {
    let raw = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    f64::from(raw) / 65536.0
}

/// Treat a raw `u16` `Rect` component as signed 16-bit
/// (`PICT.pm:1119-1120`, `$_ >= 0x8000 and $_ -= 0x10000`).
fn signed16(value: u16) -> i32 {
    if value >= 0x8000 {
        i32::from(value) - 0x10000
    } else {
        i32::from(value)
    }
}

/// Try to read a PICT header starting at `offset` within `all`. `None` when
/// this offset is not a recognizable version 1/2 opcode -- the caller
/// retries once at the 512-byte file-header offset before giving up.
fn try_header(all: &[u8], offset: usize) -> Option<Header> {
    let probe = all.get(offset..offset + HEADER_LEN)?;
    // `unpack('x2n5', $buff)`: skip the 2-byte picture size, then five
    // big-endian u16s -- the Rect, then the opcode.
    let mut values = [0u16; 5];
    for (index, value) in values.iter_mut().enumerate() {
        let at = 2 + index * 2;
        *value = u16::from_be_bytes([probe[at], probe[at + 1]]);
    }
    let [top, left, bottom, right, op] = values;

    let (extended, extra) = match op {
        0x1101 => (false, None),
        0x0011 => {
            let extra = all.get(offset + HEADER_LEN..offset + HEADER_LEN + V2_EXTRA_LEN)?;
            if extra.starts_with(b"\x02\xff\x0c\x00\xff\xff") {
                (false, None)
            } else if extra.starts_with(b"\x02\xff\x0c\x00\xff\xfe") {
                (true, Some(extra))
            } else {
                return None;
            }
        }
        _ => return None,
    };

    let top = signed16(top);
    let left = signed16(left);
    let bottom = signed16(bottom);
    let right = signed16(right);
    let mut width = f64::from(right - left);
    let mut height = f64::from(bottom - top);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let mut h_res = None;
    let mut v_res = None;
    if extended {
        let extra = extra?;
        let hr = fixed_32s(&extra[8..12]);
        let vr = fixed_32s(&extra[12..16]);
        if hr == 0.0 || vr == 0.0 {
            return None;
        }
        // PICT.pm:1134-1135: 72-dpi bounding box rescaled to the declared
        // resolution, rounded half-up.
        width = (width * hr / 72.0 + 0.5).floor();
        height = (height * vr / 72.0 + 0.5).floor();
        h_res = Some(hr);
        v_res = Some(vr);
    }

    Some(Header {
        width,
        height,
        h_res,
        v_res,
    })
}

/// Extract PICT metadata (header-only, matching ExifTool's non-verbose
/// default -- image opcodes are only walked under `-v`/`-U`).
pub fn parse_pict_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let size = reader.size() as usize;
    let want = size.min(FILE_HEADER_PAD + HEADER_LEN + V2_EXTRA_LEN);
    let all = reader.read(0, want).map_err(|error| error.to_string())?;

    let header = try_header(all, 0)
        .or_else(|| try_header(all, FILE_HEADER_PAD))
        .ok_or_else(|| "invalid PICT header".to_string())?;

    let mut metadata = MetadataMap::new();
    metadata.insert("File:ImageWidth", TagValue::Integer(header.width as i64));
    metadata.insert("File:ImageHeight", TagValue::Integer(header.height as i64));
    if let Some(h_res) = header.h_res {
        metadata.insert(
            "File:XResolution",
            TagValue::new_string(format_whole_or_decimal(h_res)),
        );
    }
    if let Some(v_res) = header.v_res {
        metadata.insert(
            "File:YResolution",
            TagValue::new_string(format_whole_or_decimal(v_res)),
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
            "/tmp/oxidex-exiftool-cache/combined-samples/PICT.pict",
            "/tmp/oxidex-exiftool-cache/exiftool/t/images/PICT.pict",
        ];
        for candidate in candidates {
            if let Ok(reader) = MMapReader::new(Path::new(candidate)) {
                return reader;
            }
        }
        panic!("PICT.pict fixture not found in the oxidex-exiftool-cache");
    }

    #[test]
    fn matches_exiftool_13_59_on_the_real_fixture() {
        let reader = fixture_reader();
        let metadata = parse_pict_metadata(&reader).expect("parses");

        // Cross-checked against `exiftool -a -G1 -s` (pinned 13.59) on the
        // same fixture.
        assert_eq!(metadata.get("File:ImageWidth"), Some(&TagValue::Integer(8)));
        assert_eq!(
            metadata.get("File:ImageHeight"),
            Some(&TagValue::Integer(8))
        );
        assert_eq!(
            metadata.get("File:XResolution"),
            Some(&TagValue::new_string("72"))
        );
        assert_eq!(
            metadata.get("File:YResolution"),
            Some(&TagValue::new_string("72"))
        );
    }
}
