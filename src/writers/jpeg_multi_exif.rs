//! The direct JPEG date shifter edits one EXIF APP1 block. Refuse a real
//! date change when another EXIF block is present until every block can be
//! written together. This is the date-shift prerequisite of the broader
//! multi-APP1 write guard carried by the dependent branch.

use crate::core::MetadataMap;
use crate::core::date_shift::ExifDateTag;
use crate::error::{ExifToolError, Result, TagNotWritten};

/// The direct date shifter has already loaded these exact bytes and can
/// return unchanged only when every native-counted EXIF block proves every
/// requested date absent. A hidden, malformed, or nonstandard APP1 record
/// cannot justify patching only the first block.
pub(crate) fn refuse_multi_exif_app1_date_shift(
    file_bytes: &[u8],
    targets: &[ExifDateTag],
) -> Result<()> {
    let count = exif_app1_records(file_bytes);
    if count <= 1 || targets.is_empty() {
        return Ok(());
    }
    let absent = (|| {
        let ranges = super::exif_surgical::jpeg_exif_blocks(file_bytes).ok()?;
        if ranges.len() != count {
            return None;
        }
        let blocks: Vec<&[u8]> = ranges
            .iter()
            .map(|&(at, len)| file_bytes.get(at..at.checked_add(len)?))
            .collect::<Option<_>>()?;
        let removed: Vec<String> = targets
            .iter()
            .map(|target| {
                let (_, name) = target.key().split_once(':').expect("grouped date key");
                format!("EXIF:{name}")
            })
            .collect();
        let empty = MetadataMap::new();
        Some(super::exif_surgical::exif_request_is_no_op(
            &blocks,
            &blocks,
            super::exif_surgical::EXIF_BLOCK_MAGICS,
            false,
            &empty,
            &empty,
            &removed,
        ))
    })()
    .unwrap_or(false);
    if absent {
        Ok(())
    } else {
        refuse_keys(
            count,
            targets
                .iter()
                .map(|target| target.key().to_string())
                .collect(),
        )
    }
}

fn refuse_keys(blocks: usize, keys: Vec<String>) -> Result<()> {
    if keys.is_empty() {
        return Ok(());
    }
    let reason = format!(
        "the file has {blocks} EXIF APP1 blocks; ExifTool writes every one, oxidex writes one"
    );
    Err(ExifToolError::TagsNotWritten {
        tags: keys
            .into_iter()
            .map(|key| TagNotWritten::new(key, reason.clone()))
            .collect(),
    })
}

/// The number of EXIF APP1 records in a JPEG's header. Marker scanning and
/// continuation classification follow pinned ExifTool 13.59's JPEG pre-scan
/// (Writer.pl 13.59:5709-5781); identifier-only records also count because
/// its rewrite loop recognizes and populates them (Writer.pl 13.59:6352),
/// each of which its writer edits and warns `Multiple APP1 EXIF records`
/// about. The scan is ExifTool's, not the reader's segment parser: it skips
/// to the next 0xFF after each segment and over any number of 0xFF fill
/// bytes before a marker, so a padded header is counted whole (the parser
/// stopped at the fill byte, counted one record, and let the one-block
/// writer truncate the file); it stops at SOS or EOI. An APP1 is an EXIF
/// record when it matches `/^(.{0,4})Exif\0./is` (so `Exif\0\x01`,
/// `exif\0\0` and up to four bytes of leading junk count), less an
/// ExtendedEXIF continuation: one directly after an EXIF record or
/// continuation, with no junk, whose bytes after the identifier are no TIFF
/// header.
fn exif_app1_records(data: &[u8]) -> usize {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return 0;
    }
    let mut pos = 2;
    let mut records = 0;
    // Whether the previous segment was an EXIF record or continuation.
    let mut after_exif = false;
    loop {
        // `ReadLine` with `$/ = "\xff"`: up to and past the next 0xFF.
        let Some(skip) = data
            .get(pos..)
            .and_then(|rest| rest.iter().position(|&b| b == 0xFF))
        else {
            break;
        };
        pos += skip + 1;
        // Any number of 0xFF fill bytes, then the marker.
        let marker = loop {
            let Some(&byte) = data.get(pos) else {
                return records;
            };
            pos += 1;
            if byte != 0xFF {
                break byte;
            }
        };
        if matches!(marker, 0xDA | 0xD9) {
            break;
        }
        let mut exif = false;
        if marker & 0xF0 == 0xC0 && (marker == 0xC0 || marker & 0x03 != 0) {
            // SOF0-SOF15 but DHT, JPGA and DAC: ExifTool skips 7 bytes.
            pos += 7;
        } else if !matches!(marker, 0x00 | 0x01 | 0xD0..=0xD7) {
            let Some(&[high, low]) = data.get(pos..pos + 2) else {
                break;
            };
            pos += 2;
            let Some(length) = (usize::from(u16::from_be_bytes([high, low]))).checked_sub(2) else {
                break;
            };
            if marker & 0xF0 == 0xE0 {
                let Some(head) = data.get(pos..pos + length.min(64)) else {
                    break;
                };
                if marker == 0xE1
                    && let Some((junk, bytes)) = exif_identifier(head)
                {
                    let tiff = bytes.starts_with(b"MM\0\x2a") || bytes.starts_with(b"II\x2a\0");
                    // The rewrite loop also accepts an identifier-only record,
                    // which the stricter pre-scan does not call ExtendedEXIF.
                    if bytes.is_empty() || !(after_exif && junk == 0 && !tiff) {
                        records += 1;
                    }
                    exif = !bytes.is_empty();
                }
            }
            pos += length;
        }
        after_exif = exif;
    }
    records
}

/// The rewrite loop's EXIF APP1 identifier, `/^(.{0,4})Exif\0./is`:
/// leading junk length and up to four following bytes. An empty tail still
/// names a record the writer populates, although the pre-scan skips it.
fn exif_identifier(head: &[u8]) -> Option<(usize, &[u8])> {
    (0..=4).find_map(|junk| {
        let id = head.get(junk..junk + 5)?;
        if !id.eq_ignore_ascii_case(b"Exif\0") {
            return None;
        }
        let bytes = head.get(junk + 6..)?;
        Some((junk, &bytes[..bytes.len().min(4)]))
    })
}
