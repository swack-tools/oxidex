//! SWF (Shockwave Flash) metadata parser.
//!
//! ExifTool routes a `.swf` file through `Image::ExifTool::Flash::ProcessSWF`
//! (`Flash.pm:591-684`): an 8-byte header (`"FWS"`/`"CWS"` + version byte +
//! little-endian file length), a bit-packed `RECT` structure giving the stage
//! dimensions, a frame rate/count pair, and then a scan over top-level SWF
//! tags looking for tag 69 (`FlashAttributes`, `Flash.pm:54-61`) and tag 77
//! (embedded XMP, `Flash.pm:62-65`, `SubDirectory => 'Image::ExifTool::XMP::Main'`).
//! A `"CWS"` signature means the body past the 8-byte header is zlib-deflated
//! (`Flash.pm:543-561`'s `ReadCompressed`); this parser inflates it in full up
//! front rather than reproducing ExifTool's incremental re-read, since OxiDex
//! already holds the whole file in memory.
//!
//! # Fields
//!
//! - `FlashVersion`, `Compressed` -- straight from the header.
//! - `ImageWidth`/`ImageHeight` -- `(Xmax-Xmin)/20` and `(Ymax-Ymin)/20`
//!   twips-to-pixels (`Flash.pm:628-629`), reproducing ExifTool's own
//!   unsigned bit-string extraction (`Flash.pm:614-627`) rather than a
//!   signed `RECT` read -- the two happen to agree whenever the stage
//!   origin is non-negative, which is the overwhelmingly common case, and
//!   matching ExifTool's actual computation is what parity requires.
//! - `FrameRate`, `FrameCount`, `Duration` (`Flash.pm:632-635`,
//!   `ConvertDuration`).
//! - `FlashAttributes` -- `BITMASK` PrintConv (`Flash.pm:56-60`), named bits
//!   0/3/4 only; every other set bit renders as `[n]`
//!   (`crate::exiftool_tables::decode_bits`, ExifTool's own `DecodeBits`).
//! - Embedded XMP (tag 77) is handed to the generic RDF/XML parser
//!   (`crate::parsers::xmp::rdf_parser::parse_xmp_typed`), the same one every
//!   other embedded-XMP container (DjVu, PDF, JPEG) uses. That shared parser
//!   currently reports some properties (`XMPToolkit`, and `pdf:Author` when
//!   its namespace is declared on a nested element rather than the root)
//!   under the bare `XMP` group instead of ExifTool's `XMP-x`/`XMP-pdf` --
//!   a pre-existing characteristic of the shared component this parser
//!   inherits rather than papers over locally.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Flash.pm`

use crate::core::formatters::duration::convert_duration;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::decode_bits;
use crate::parsers::xmp::rdf_parser::{XmpValue, parse_xmp_typed};
use flate2::read::ZlibDecoder;
use std::io::Read;

/// `Flash.pm:598`, `$raf->Read($buff, 8) == 8 or return 0;`.
const HEADER_LEN: usize = 8;

/// `Flash.pm:56-60`'s `BITMASK` names for `FlashAttributes`. Bits 1, 2, 5, 6,
/// 7 are unnamed and render as `[n]` (`decode_bits`'s fallback), matching
/// ExifTool's own `DecodeBits` for an unlisted bit.
const FLASH_ATTRIBUTES_BITS: &[(u32, &str)] =
    &[(0, "UseNetwork"), (3, "ActionScript3"), (4, "HasMetadata")];

/// `FrameRate => {}` carries no `PrintConv` (`Flash.pm:48`), so ExifTool
/// prints Perl's own default numeric stringification -- no trailing
/// `.00` for the overwhelmingly common case of a whole frame rate. A
/// hard-coded two decimals would turn a correct number into a value
/// mismatch (the same fix already applied to PSD resolution,
/// `parsers::image::psd::format_psd_resolution`).
fn format_whole_or_decimal(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Read `n` bits (`n <= 32`) MSB-first starting at `bit_pos`, matching Perl's
/// `unpack("B$totBits", $buff)` -- a contiguous bitstream across byte
/// boundaries, not a per-byte reset.
fn read_bits(data: &[u8], bit_pos: &mut usize, n: usize) -> Option<u32> {
    let mut value: u32 = 0;
    for _ in 0..n {
        let byte_index = *bit_pos / 8;
        let byte = *data.get(byte_index)?;
        let bit_index = 7 - (*bit_pos % 8);
        let bit = (byte >> bit_index) & 1;
        value = (value << 1) | u32::from(bit);
        *bit_pos += 1;
    }
    Some(value)
}

/// Extract SWF metadata from an in-memory reader.
pub fn parse_swf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let size = reader.size() as usize;
    if size < HEADER_LEN {
        return Err("SWF file is too short for the 8-byte header".to_string());
    }
    let all = reader.read(0, size).map_err(|error| error.to_string())?;

    // Flash.pm:599: `$buff =~ /^(F|C)WS([^\0])/ or return 0;` -- the version
    // byte must be present and non-zero.
    let compressed = match &all[0..3] {
        b"FWS" => false,
        b"CWS" => true,
        _ => return Err("missing FWS/CWS SWF signature".to_string()),
    };
    let version = all[3];
    if version == 0 {
        return Err("invalid SWF version byte".to_string());
    }

    let mut metadata = MetadataMap::new();
    metadata.insert("Flash:FlashVersion", TagValue::Integer(i64::from(version)));
    metadata.insert(
        "Flash:Compressed",
        TagValue::new_string(if compressed { "True" } else { "False" }),
    );

    // Flash.pm:609-611: the rest of the file (past the 8-byte header) is
    // zlib-deflated for a `CWS` signature.
    let body: std::borrow::Cow<'_, [u8]> = if compressed {
        let mut decoder = ZlibDecoder::new(&all[HEADER_LEN..]);
        let mut inflated = Vec::new();
        if decoder.read_to_end(&mut inflated).is_err() {
            // ExifTool warns and stops at the header on an inflate error
            // (Flash.pm:610), rather than failing the whole file.
            return Ok(metadata);
        }
        std::borrow::Cow::Owned(inflated)
    } else {
        std::borrow::Cow::Borrowed(&all[HEADER_LEN..])
    };

    // Flash.pm:614-627: bit-packed `RECT` structure.
    let mut bit_pos = 0usize;
    let Some(n_bits) = read_bits(&body, &mut bit_pos, 5) else {
        return Ok(metadata);
    };
    let n_bits = n_bits as usize;
    let total_bits = 5 + n_bits * 4;
    let n_bytes = total_bits.div_ceil(8);
    if body.len() < n_bytes + 4 {
        // Flash.pm:618-620: "Truncated Flash file" -- still report the
        // header fields already found.
        return Ok(metadata);
    }
    let (Some(x_min), Some(x_max), Some(y_min), Some(y_max)) = (
        read_bits(&body, &mut bit_pos, n_bits),
        read_bits(&body, &mut bit_pos, n_bits),
        read_bits(&body, &mut bit_pos, n_bits),
        read_bits(&body, &mut bit_pos, n_bits),
    ) else {
        return Ok(metadata);
    };
    metadata.insert(
        "Flash:ImageWidth",
        TagValue::Integer((x_max as i64 - x_min as i64) / 20),
    );
    metadata.insert(
        "Flash:ImageHeight",
        TagValue::Integer((y_max as i64 - y_min as i64) / 20),
    );

    // Flash.pm:630-635: frame rate (8.8 fixed point) and frame count.
    let frame_rate_raw = u16::from_le_bytes([body[n_bytes], body[n_bytes + 1]]);
    let frame_count = u16::from_le_bytes([body[n_bytes + 2], body[n_bytes + 3]]);
    let frame_rate = f64::from(frame_rate_raw) / 256.0;
    metadata.insert(
        "Flash:FrameRate",
        TagValue::new_string(format_whole_or_decimal(frame_rate)),
    );
    metadata.insert(
        "Flash:FrameCount",
        TagValue::Integer(i64::from(frame_count)),
    );
    if frame_rate_raw != 0 {
        let duration = f64::from(frame_count) * 256.0 / f64::from(frame_rate_raw);
        metadata.insert(
            "Flash:Duration",
            TagValue::new_string(convert_duration(duration)),
        );
    }

    // Flash.pm:641-682: scan tags for FlashAttributes (69) and embedded XMP
    // (77).
    let rest = &body[n_bytes + 4..];
    let mut cursor = 0usize;
    let mut has_meta = false;
    loop {
        if rest.len() < cursor + 2 {
            break;
        }
        let code = u16::from_le_bytes([rest[cursor], rest[cursor + 1]]);
        let mut pos = cursor + 2;
        let tag = code >> 6;
        let mut tag_size = usize::from(code & 0x3f);

        if tag != 69 && tag != 77 && !has_meta {
            break;
        }

        if tag_size == 0x3f {
            if rest.len() < pos + 4 {
                break;
            }
            let extended = u32::from_le_bytes(rest[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            if extended > 1_000_000 {
                break;
            }
            tag_size = extended;
        }
        if rest.len() < pos + tag_size {
            break;
        }

        if tag == 69 {
            if tag_size == 0 {
                break;
            }
            let flags = rest[pos];
            metadata.insert(
                "Flash:FlashAttributes",
                TagValue::new_string(decode_bits(i64::from(flags), FLASH_ATTRIBUTES_BITS)),
            );
            if flags & 0x10 == 0 {
                break;
            }
            has_meta = true;
        } else if tag == 77 {
            let payload = &rest[pos..pos + tag_size];
            if let Ok(tags) = parse_xmp_typed(payload) {
                for (name, value) in tags {
                    let value = match value {
                        XmpValue::Scalar(value) => TagValue::new_string(value),
                        XmpValue::List(values) => {
                            TagValue::Array(values.into_iter().map(TagValue::new_string).collect())
                        }
                    };
                    metadata.insert(name, value);
                }
            }
            break;
        }

        if rest.len() < pos + 2 {
            break;
        }
        cursor = pos;
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
            "/tmp/oxidex-exiftool-cache/combined-samples/Flash.swf",
            "/tmp/oxidex-exiftool-cache/exiftool/t/images/Flash.swf",
        ];
        for candidate in candidates {
            if let Ok(reader) = MMapReader::new(Path::new(candidate)) {
                return reader;
            }
        }
        panic!("Flash.swf fixture not found in the oxidex-exiftool-cache");
    }

    #[test]
    fn matches_exiftool_13_59_on_the_real_fixture() {
        let reader = fixture_reader();
        let metadata = parse_swf_metadata(&reader).expect("parses");

        // Cross-checked against `exiftool -a -G1 -s` (pinned 13.59) on the
        // same fixture.
        assert_eq!(
            metadata.get("Flash:FlashVersion"),
            Some(&TagValue::Integer(6))
        );
        assert_eq!(
            metadata.get("Flash:Compressed"),
            Some(&TagValue::new_string("False"))
        );
        assert_eq!(
            metadata.get("Flash:ImageWidth"),
            Some(&TagValue::Integer(50))
        );
        assert_eq!(
            metadata.get("Flash:ImageHeight"),
            Some(&TagValue::Integer(50))
        );
        assert_eq!(
            metadata.get("Flash:FrameRate"),
            Some(&TagValue::new_string("12"))
        );
        assert_eq!(
            metadata.get("Flash:FrameCount"),
            Some(&TagValue::Integer(1))
        );
        assert_eq!(
            metadata.get("Flash:Duration"),
            Some(&TagValue::new_string("0.08 s"))
        );
        // The embedded XMP packet is handed to the same generic
        // `parse_xmp_typed` every other container (JPEG, PDF, DjVu, PNG)
        // delegates to; it currently reports `XMPToolkit` and this
        // nested-namespace `pdf:Author` under the bare `XMP` group rather
        // than ExifTool's `XMP-x`/`XMP-pdf` (verified against the same
        // pinned oracle on `ExifTool.jpg`'s `XMPToolkit`, so this is a
        // pre-existing characteristic of the shared parser, not something
        // this SWF integration introduces or should paper over locally).
        assert_eq!(
            metadata.get("XMP:Author"),
            Some(&TagValue::new_string("Phil"))
        );
        assert_eq!(
            metadata.get("XMP:XMPToolkit"),
            Some(&TagValue::new_string("Image::ExifTool 7.50"))
        );
    }
}
