//! Sony MakerNote parser
//!
//! Sony stores its metadata as an IFD whose entry offsets are relative to the
//! *TIFF header*, not to the MakerNote block, and hangs several
//! `ProcessBinaryData` sub-directories off it. Which layout a sub-directory
//! uses is decided per camera - by byte count for the A-mount DSLR tables, by
//! model string for others - so a table that decodes one body will silently
//! mis-decode another.
//!
//! ## Layout
//! * [`main_table`] - tags stored directly as IFD entries
//! * [`amount`] - the `CameraInfo2` / `FocusInfo` / `CameraSettings`
//!   sub-directories written by the A-mount DSLRs
//! * [`binary`] - the shared `ProcessBinaryData` addressing and print helpers
//! * [`lens_spec`] - the packed eight-byte `LensSpec`
//! * [`value`] - typed access to one IFD entry's bytes
//!
//! ## Resolving values
//! Only values of four bytes or fewer live inside an IFD entry. Everything
//! else - every string, array and sub-directory - is addressed by a
//! TIFF-relative offset, so the parser needs `data_base`, the TIFF-relative
//! position of the MakerNote itself, to turn that offset into an index into
//! the blob it was handed. Without it those tags cannot be read at all, which
//! is why [`MakerNoteParser::parse_with_context`] is the real entry point and
//! [`parse_sony_makernote`] can only see the inline tags.

pub mod amount;
pub mod binary;
pub mod lens_spec;
pub mod main_table;
pub mod value;

use crate::error::Result;
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::sony_lens_database::lookup_lens_name;
use main_table::main_tag;
use value::SonyValue;

// ============================================================================
// MakerNote header formats
// ============================================================================
// Sony MakerNote header formats vary by camera model:
//   DSC-W/DSC-S series: "SONY DSC " + null padding + IFD
//   Some Alpha cameras: just the IFD, with no header at all
//   Other models:       "SONY CAM " or a bare "SONY" prefix

const SONY_DSC_SIGNATURE: &[u8] = b"SONY DSC ";
const SONY_CAM_SIGNATURE: &[u8] = b"SONY CAM ";
const SONY_SIGNATURE: &[u8] = b"SONY";

/// Length of the "SONY xxx " header, including the three NUL bytes that pad it.
/// ExifTool starts the IFD at a fixed `$valuePtr + 12`.
const SONY_HEADER_LEN: usize = 12;

/// Sub-directory tag ids handled by [`amount`].
const TAG_CAMERA_INFO: u16 = 0x0010;
const TAG_FOCUS_INFO: u16 = 0x0020;
const TAG_CAMERA_SETTINGS: u16 = 0x0114;
/// 0xb028 is not a value but a TIFF-relative pointer to a whole nested Minolta
/// MakerNote, which the DSLR-A100 writes alongside its Sony one. A stored zero
/// means the IFD is absent.
const TAG_MINOLTA_MAKERNOTE: u16 = 0xb028;

/// Largest plausible entry count for a Sony MakerNote IFD.
const MAX_ENTRIES: u16 = 200;

/// Represents a Sony MakerNote parser
pub struct SonyParser;

impl MakerNoteParser for SonyParser {
    fn manufacturer_name(&self) -> &'static str {
        "Sony"
    }

    fn tag_prefix(&self) -> &'static str {
        "Sony:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        is_sony_makernote(data)
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_with_context(
            &crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext::detached(data),
            byte_order,
            None,
            tags,
        )
    }

    fn parse_with_context(
        &self,
        ctx: &crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // `payload_tiff_offset` is the `data_base` this decoder subtracts from a
        // TIFF-relative value offset: `None` rather than 0 when the caller had
        // no enclosing block, so the subtraction is skipped instead of landing
        // somewhere plausible and wrong.
        let data = ctx.payload();
        match parse_sony_makernote_impl(data, byte_order, model, ctx.payload_tiff_offset()) {
            Ok(parsed_tags) => {
                tags.extend(parsed_tags);
                Ok(())
            }
            Err(e) => Err(format!("Sony MakerNote parse error: {}", e)),
        }
    }

    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        lookup_lens_name(lens_id)
    }
}

/// Checks if data appears to be Sony MakerNote.
///
/// Sony MakerNotes may optionally start with "SONY" signature,
/// but always contain a valid IFD structure.
///
/// # Parameters
/// - `data`: Raw byte data to check
///
/// # Returns
/// `true` if the data appears to be a Sony MakerNote, `false` otherwise
pub fn is_sony_makernote(data: &[u8]) -> bool {
    if data.len() < 2 {
        return false;
    }

    if data.starts_with(SONY_DSC_SIGNATURE)
        || data.starts_with(SONY_CAM_SIGNATURE)
        || data.starts_with(SONY_SIGNATURE)
    {
        return true;
    }

    let le_reader = EndianReader::little_endian(data);
    let be_reader = EndianReader::big_endian(data);
    let entry_count_le = le_reader.u16_at(0).unwrap_or(0);
    let entry_count_be = be_reader.u16_at(0).unwrap_or(0);

    let is_reasonable = |count: u16| (1..=MAX_ENTRIES).contains(&count);

    is_reasonable(entry_count_le) || is_reasonable(entry_count_be)
}

/// Find the start of the IFD by skipping null padding
/// Some Sony cameras add null bytes between the signature and the IFD
fn find_ifd_start(data: &[u8]) -> usize {
    let mut offset = 0;
    while offset < data.len() && data[offset] == 0 {
        offset += 1;
    }

    // Sanity check: don't skip more than 16 bytes
    if offset > 16 { 0 } else { offset }
}

/// Byte length of one component of a TIFF field type, or `None` for types Sony
/// never writes.
fn type_size(field_type: u16) -> Option<usize> {
    Some(match field_type {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => return None,
    })
}

/// The MakerNote IFD plus everything needed to resolve an entry's value.
struct SonyIfd<'a> {
    /// The whole MakerNote blob, including any signature.
    data: &'a [u8],
    /// Slice index at which the IFD itself begins.
    ifd_start: usize,
    /// TIFF-relative offset of `data[0]`, when the caller knew it.
    data_base: Option<u32>,
    byte_order: ByteOrder,
}

impl<'a> SonyIfd<'a> {
    /// Resolves an entry to its bytes.
    ///
    /// Values of four bytes or fewer are stored inside the entry; larger ones
    /// live at a TIFF-relative offset, which is why `data_base` is required to
    /// read them. When it is unknown the entry is skipped rather than read
    /// from a guessed position - a MakerNote read at the wrong base yields
    /// plausible-looking values that belong to a different tag.
    fn value(&self, entry: &IfdEntry) -> Option<SonyValue<'a>> {
        let size = type_size(entry.field_type)?;
        let total = size.checked_mul(entry.value_count as usize)?;

        if total <= 4 {
            // Inline: the entry's own 4-byte field, in file order.
            let inline = match self.byte_order {
                ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
                ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
            };
            return Some(SonyValue::new(
                entry.field_type,
                entry.value_count,
                inline[..total].to_vec(),
                self.byte_order,
            ));
        }

        let base = self.data_base?;
        let index = entry.value_offset.checked_sub(base)? as usize;
        let bytes = self.data.get(index..index.checked_add(total)?)?;
        Some(SonyValue::new(
            entry.field_type,
            entry.value_count,
            bytes,
            self.byte_order,
        ))
    }
}

/// Internal implementation of Sony MakerNote parsing.
fn parse_sony_makernote_impl(
    data: &[u8],
    _byte_order: ByteOrder,
    model: Option<&str>,
    data_base: Option<u32>,
) -> Result<HashMap<String, String>> {
    if data.is_empty() {
        return Ok(HashMap::new());
    }

    // Skip whichever header this body writes; the IFD follows it.
    //
    // The "SONY xxx " headers are a *fixed* 12 bytes - ExifTool's MakerNoteSony
    // entry is `Start => '$valuePtr + 12'`. Scanning for the first non-NUL byte
    // instead overshoots whenever the entry count's high byte is zero, which is
    // exactly what a big-endian MakerNote with fewer than 256 entries looks
    // like: the DSLR-A100 writes `SONY DSC \0\0\0` followed by the big-endian
    // count 0x001c, and a NUL scan swallows the 0x00 and reads a nonsense
    // count from the wrong pair of bytes.
    let header_skip =
        if data.starts_with(SONY_DSC_SIGNATURE) || data.starts_with(SONY_CAM_SIGNATURE) {
            SONY_HEADER_LEN
        } else if data.starts_with(SONY_SIGNATURE) {
            find_ifd_start(&data[SONY_SIGNATURE.len()..]) + SONY_SIGNATURE.len()
        } else {
            0
        };

    let ifd_start = if header_skip < data.len() {
        header_skip
    } else {
        0
    };
    let ifd_data = &data[ifd_start..];
    if ifd_data.len() < 2 {
        return Ok(HashMap::new());
    }

    // Sony writes the MakerNote in the file's own byte order, but a bare count
    // is the only thing available to confirm it here.
    let entry_count_le = u16::from_le_bytes([ifd_data[0], ifd_data[1]]);
    let entry_count_be = u16::from_be_bytes([ifd_data[0], ifd_data[1]]);
    let is_reasonable = |count: u16| (1..=MAX_ENTRIES).contains(&count);

    let (entry_count, byte_order) = if is_reasonable(entry_count_le) {
        (entry_count_le, ByteOrder::LittleEndian)
    } else if is_reasonable(entry_count_be) {
        (entry_count_be, ByteOrder::BigEndian)
    } else {
        return Ok(HashMap::new());
    };

    let entries = read_entries(&ifd_data[2..], entry_count, byte_order);
    let ifd = SonyIfd {
        data,
        ifd_start,
        data_base,
        byte_order,
    };
    let _ = ifd.ifd_start;

    // ExifTool suppresses duplicate tag *names* - across groups, not just
    // within one - keeping the highest-priority copy and, among equals, the
    // last one extracted. Collect candidates in extraction order and resolve
    // at the end; anything less loses the distinction between the DSLR-A100's
    // Sony `LensType` (overridden by the nested Minolta one written later) and
    // the SLT-A77's `DynamicRangeOptimizer` (0xb04f is `Priority => 0` and
    // loses to 0xb025 despite coming later).
    let mut found: Vec<Found> = Vec::new();

    for entry in &entries {
        let Some(value) = ifd.value(entry) else {
            continue;
        };

        match entry.tag_id {
            TAG_CAMERA_INFO => {
                let mut tags = HashMap::new();
                amount::extract_camera_info2(value.bytes(), &mut tags);
                push_all(&mut found, tags, SUB_DIRECTORY_PRIORITY);
            }
            TAG_FOCUS_INFO => {
                let mut tags = HashMap::new();
                amount::extract_focus_info(value.bytes(), model, &mut tags);
                push_all(&mut found, tags, SUB_DIRECTORY_PRIORITY);
            }
            TAG_CAMERA_SETTINGS => {
                let mut tags = HashMap::new();
                amount::extract_camera_settings(value.bytes(), &mut tags);
                push_all(&mut found, tags, SUB_DIRECTORY_PRIORITY);
            }
            TAG_MINOLTA_MAKERNOTE => {
                let Some(start) = value.first_int().filter(|v| *v != 0) else {
                    continue;
                };
                let Some(base) = data_base else { continue };
                let Some(index) = (start as u64).checked_sub(base as u64) else {
                    continue;
                };
                let (main, sub) = crate::parsers::tiff::makernotes::minolta::parse_minolta_ifd(
                    data,
                    index as usize,
                    byte_order,
                    data_base,
                    true,
                );
                // The nested Main table carries ExifTool's default priority,
                // the same as Sony's own, so it overrides the 0xb0xx scalars
                // listed before it and is in turn overridden by 0xb029, which
                // the DSLR-A100 lists after it.
                for (key, printed) in main {
                    found.push(Found::new(key, printed, DEFAULT_PRIORITY));
                }
                for (key, printed) in sub {
                    found.push(Found::new(key, printed, SUB_DIRECTORY_PRIORITY));
                }
            }
            _ => {
                // Tags ExifTool has no name for are named `Sony_0xNNNN` and
                // flagged Unknown, so they never appear in its output; emitting
                // them here would only add keys no comparison can match.
                let Some(tag) = main_tag(entry.tag_id) else {
                    continue;
                };
                if let Some(printed) = tag.render(&value, model) {
                    found.push(Found::new(
                        format!("Sony:{}", tag.name),
                        printed,
                        tag.priority,
                    ));
                }
            }
        }
    }

    Ok(resolve_duplicates(found))
}

/// ExifTool's priority for a tag whose table declares none.
const DEFAULT_PRIORITY: u8 = 1;
/// The priority the Sony binary sub-directory tables declare (`PRIORITY => 0`).
const SUB_DIRECTORY_PRIORITY: u8 = 0;

/// One extracted tag, before duplicate names are resolved.
struct Found {
    /// Full `Group:Name` key.
    key: String,
    /// Printed value.
    value: String,
    /// ExifTool priority.
    priority: u8,
}

impl Found {
    fn new(key: String, value: String, priority: u8) -> Self {
        Found {
            key,
            value,
            priority,
        }
    }

    /// The bare tag name, which is what ExifTool de-duplicates on - a Sony
    /// `LensType` and a Minolta `LensType` are the same tag as far as its
    /// default output is concerned.
    fn name(&self) -> &str {
        self.key.split_once(':').map_or(&self.key, |(_, name)| name)
    }
}

fn push_all(found: &mut Vec<Found>, tags: HashMap<String, String>, priority: u8) {
    // A binary table yields its tags in one batch with no meaningful order
    // between them; sort so a run-to-run difference cannot change which of two
    // same-named tags wins.
    let mut tags: Vec<_> = tags.into_iter().collect();
    tags.sort();
    found.extend(
        tags.into_iter()
            .map(|(key, value)| Found::new(key, value, priority)),
    );
}

/// Keeps the copy of each tag name ExifTool would display: highest priority,
/// and among equals the one extracted last.
fn resolve_duplicates(found: Vec<Found>) -> HashMap<String, String> {
    let mut winners: HashMap<String, (u8, String, String)> = HashMap::new();
    for tag in found {
        let name = tag.name().to_string();
        match winners.get(&name) {
            Some((priority, _, _)) if *priority > tag.priority => continue,
            _ => {
                winners.insert(name, (tag.priority, tag.key, tag.value));
            }
        }
    }
    winners
        .into_values()
        .map(|(_, key, value)| (key, value))
        .collect()
}

/// Reads `count` 12-byte IFD entries, stopping early if the data runs out.
fn read_entries(data: &[u8], count: u16, byte_order: ByteOrder) -> Vec<IfdEntry> {
    let reader = EndianReader::new(data, byte_order.to_io_byte_order());
    (0..count as usize)
        .map_while(|i| {
            let base = i * 12;
            Some(IfdEntry {
                tag_id: reader.u16_at(base)?,
                field_type: reader.u16_at(base + 2)?,
                value_count: reader.u32_at(base + 4)?,
                value_offset: reader.u32_at(base + 8)?,
            })
        })
        .collect()
}

/// Parses Sony MakerNote data into a map of tag names to values.
///
/// This entry point has no TIFF context, so it can only read the tags a Sony
/// MakerNote stores inline in its IFD entries - four bytes or fewer. Callers
/// that know where the block sits in the TIFF should use
/// [`MakerNoteParser::parse_with_context`], which also reaches the strings,
/// arrays and binary sub-directories.
///
/// # Parameters
/// - `data`: Raw MakerNote data (may include Sony signature)
/// - `byte_order`: Byte order for parsing (usually LittleEndian for Sony)
/// - `tags`: Mutable reference to HashMap to populate with extracted tags
pub fn parse_sony_makernote(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = SonyParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("Sony MakerNotes parse error: {}", e);
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sony_header_validation() {
        assert!(is_sony_makernote(b"SONY\x01\x00"));
        assert!(is_sony_makernote(b"\x05\x00"));
        assert!(!is_sony_makernote(b"\xFF\xFF"));
        assert!(!is_sony_makernote(b"\x01"));
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = SonyParser;
        assert_eq!(parser.manufacturer_name(), "Sony");
        assert_eq!(parser.tag_prefix(), "Sony:");
    }

    #[test]
    fn test_validate_header() {
        let parser = SonyParser;
        assert!(parser.validate_header(b"SONY\x01\x00extra"));
        assert!(parser.validate_header(b"\x05\x00"));
        assert!(!parser.validate_header(b"\xFF\xFF"));
    }

    /// Builds a headerless little-endian Sony MakerNote from `(tag, type,
    /// count, value)` entries.
    fn build_makernote(entries: &[(u16, u16, u32, u32)]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, ty, count, value) in entries {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&ty.to_le_bytes());
            data.extend_from_slice(&count.to_le_bytes());
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.extend_from_slice(&0u32.to_le_bytes());
        data
    }

    #[test]
    fn inline_tags_are_decoded_with_their_printconv() {
        // Quality 0x0102 = 2 is "Fine"; SonyModelID 0xb001 = 260 is DSLR-A350.
        let data = build_makernote(&[(0x0102, 4, 1, 2), (0xb001, 3, 1, 260)]);
        let mut tags = HashMap::new();
        parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut tags);
        assert_eq!(tags.get("Sony:Quality"), Some(&"Fine".to_string()));
        assert_eq!(tags.get("Sony:SonyModelID"), Some(&"DSLR-A350".to_string()));
    }

    #[test]
    fn lens_type_resolves_through_the_exiftool_lens_table() {
        // LensType 0xb027 = 25 is the entry DSLR-A350.jpg carries.
        let data = build_makernote(&[(0xb027, 4, 1, 25)]);
        let mut tags = HashMap::new();
        parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut tags);
        assert_eq!(
            tags.get("Sony:LensType"),
            Some(&"Minolta AF 100-300mm F4.5-5.6 APO (D) or Sigma Lens".to_string())
        );
    }

    #[test]
    fn tags_exiftool_does_not_name_are_not_invented() {
        // 0x2003 is `Sony_0x2003`, flagged Unknown: no output tag.
        let data = build_makernote(&[(0x2003, 3, 1, 7)]);
        let mut tags = HashMap::new();
        parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(tags.is_empty(), "unexpected tags: {:?}", tags);
    }

    #[test]
    fn out_of_line_values_are_skipped_without_a_tiff_base() {
        // 0xb020 CreativeStyle is a 16-byte string, so its value lives outside
        // the entry. With no data_base the offset cannot be resolved, and a
        // guess would read whatever happens to sit at that index.
        let data = build_makernote(&[(0xb020, 2, 16, 0x1234)]);
        let mut tags = HashMap::new();
        parse_sony_makernote(&data, ByteOrder::LittleEndian, &mut tags);
        assert!(!tags.contains_key("Sony:CreativeStyle"));
    }

    #[test]
    fn a_tiff_base_makes_out_of_line_values_readable() {
        let mut data = build_makernote(&[(0xb020, 2, 16, 1000 + 20)]);
        // Entry list is 2 + 12 + 4 = 18 bytes; pad to the offset we claimed.
        data.resize(20, 0);
        data.extend_from_slice(b"Standard\0\0\0\0\0\0\0\0");

        // The MakerNote sits 1000 bytes into its enclosing TIFF block, which is
        // what its entries' offsets are measured from.
        let mut tiff = vec![0u8; 1000];
        let payload_len = data.len();
        tiff.extend_from_slice(&data);
        let ctx = crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext::in_tiff(
            &tiff,
            1000,
            payload_len,
            0,
        );

        let mut tags = HashMap::new();
        let parser = SonyParser;
        parser
            .parse_with_context(&ctx, ByteOrder::LittleEndian, Some("SLT-A77"), &mut tags)
            .unwrap();
        assert_eq!(
            tags.get("Sony:CreativeStyle"),
            Some(&"Standard".to_string())
        );
    }
}
