//! Fujifilm MakerNote Parser
//!
//! Parses Fujifilm-specific EXIF MakerNote tags containing camera settings,
//! lens information, film simulation modes, and other proprietary metadata.
//!
//! Supports both X-series mirrorless cameras and GFX medium format cameras.
//!
//! Based on ExifTool's Fujifilm.pm module. `FujiFilm::Main` itself is read
//! through the generated `IFD_FUJIFILM_MAIN` table and the IFD engine
//! ([`main_engine`]); this file keeps the entry points, the residual arms the
//! engine cannot produce, and the binary sub-directory edges.

/// `FujiFilm::Main` through the generated table and the IFD engine (slice I-6).
mod main_engine;
/// The `OTHER` fallbacks of `%FujiFilm`'s settings tables, hand-written.
mod print_conv;
/// `%FujiFilm` binary sub-tables, generated from ExifTool's own hashes.
pub mod settings_tables;

use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::makernote_context::MakerNoteContext;
use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_u16_array;
use super::shared::binary_subdir::{BinaryTable, decode_binary_subdir};
use crate::const_decoder;
use crate::core::formatters::numeric_precision::perl_number;
use crate::exiftool_tables::{find_ifd_table, read_ifd};
use settings_tables::{
    FUJIFILM_AFCSETTINGS, FUJIFILM_DRIVESETTINGS, FUJIFILM_FOCUSSETTINGS, FUJIFILM_PRIORITYSETTINGS,
};

// ===== The `FujiFilm::Main` ids this file still reads by hand =====
//
// Every other `FujiFilm::Main` id is read through the generated
// `IFD_FUJIFILM_MAIN` table and the IFD engine (`main_engine`). What stays
// here is the residual (`main_engine::FUJI_MAIN_RESIDUAL_IDS`, each with the
// reason the engine cannot produce it) and the four binary sub-directory edges
// the hand `settings_tables` decoders own.

// The residual.
const FUJI_VERSION: u16 = 0x0000;
const FUJI_SERIAL_NUMBER: u16 = 0x0010;
const FUJI_NOISE_REDUCTION: u16 = 0x100B;
const FUJI_SHADOW_TONE: u16 = 0x1040;
const FUJI_HIGHLIGHT_TONE: u16 = 0x1041;
const FUJI_IMAGE_STABILIZATION: u16 = 0x1422;
const FUJI_FACE_ELEMENT_TYPES: u16 = 0x4201;

// Packed settings words, each a `SubDirectory` over a ProcessBinaryData table
// in `%FujiFilm::Main` -- see `fujifilm_binary_subdir`.
const FUJI_PRIORITY_SETTINGS: u16 = 0x102B; // FujiFilm.pm:341
const FUJI_FOCUS_SETTINGS: u16 = 0x102D; // FujiFilm.pm:345
const FUJI_AFC_SETTINGS: u16 = 0x102E; // FujiFilm.pm:349
const FUJI_DRIVE_SETTINGS: u16 = 0x1103; // FujiFilm.pm:609

// Fujifilm MakerNote header signature
// Fujifilm uses "FUJIFILM" followed by IFD offset
const FUJIFILM_HEADER: &[u8] = b"FUJIFILM";

// ============================================================================
// DECODERS - the residual arms' value maps
// ============================================================================

// Decodes noise reduction (tag 0x100b). Per ExifTool's FujiFilm.pm:
// 0x40 => 'Low', 0x80 => 'Normal', 0x100 => 'n/a'.
const_decoder!(pub
    DECODE_NOISE_REDUCTION, i32, [
        (0x40, "Low"),
        (0x80, "Normal"),
        (0x100, "n/a"),
    ]
);

// Decodes ImageStabilization (tag 0x1422), element 0 of the 3x int16u array:
// the IS system in use. FujiFilm.pm:790-800 (the first of the two hashrefs in
// the array PrintConv at line 794). There is no 256 key in ExifTool's table --
// that entry, and the other four labels here, were invented; the real map is
// 0/1/2/3/258/512 only.
const_decoder!(pub
    DECODE_IMAGE_STABILIZATION, i32, [
        (0, "None"),
        (1, "Optical"), //PH FujiFilm.pm:796
        (2, "Sensor-shift"), //PH FujiFilm.pm:797 (now IBIS/OIS, ref forum13708)
        (3, "OIS Lens"), //forum9815 FujiFilm.pm:798 (optical+sensor?)
        (258, "IBIS/OIS + DIS"), //forum13708 FujiFilm.pm:799 (digital on top of IBIS/OIS)
        (512, "Digital"), //PH FujiFilm.pm:800
    ]
);

// Decodes ImageStabilization (tag 0x1422), element 1 of the 3x int16u array:
// the IS mode. FujiFilm.pm:801-804 (the second hashref in the array
// PrintConv). Element 2 (a frame/lens-shake counter) has no PrintConv in
// ExifTool and is rendered as the raw int16u.
const_decoder!(pub
    DECODE_IMAGE_STABILIZATION_MODE, i32, [
        (0, "Off"),
        (1, "On (mode 1, continuous)"),
        (2, "On (mode 2, shooting only)"),
    ]
);

/// Represents a Fujifilm MakerNote parser
pub struct FujifilmParser;

impl MakerNoteParser for FujifilmParser {
    fn manufacturer_name(&self) -> &'static str {
        // ExifTool spells the group with a capital F on both halves:
        // MakerNotes.pm:121 `Name => 'MakerNoteFujiFilm'`, which is what the
        // family-1 group is named after, and `exiftool -a -G1 -s` prints
        // `[FujiFilm]` for every Fuji sample in the corpus.
        "FujiFilm"
    }

    fn tag_prefix(&self) -> &'static str {
        "FujiFilm:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Fujifilm MakerNotes start with "FUJIFILM" (8 bytes) followed by offset
        data.len() >= 12 && &data[0..8] == FUJIFILM_HEADER
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_with_model(data, byte_order, None, tags)
    }

    fn parse_with_model(
        &self,
        data: &[u8],
        _byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_note(data, model, tags, None)
    }

    fn parse_with_context_and_values(
        &self,
        ctx: &MakerNoteContext<'_>,
        _byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
        value_forms: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        // The payload, not the window: the residual arms and the sub-table
        // decoders read `data` (`entry_bytes`, `extract_*`), and the engine
        // must read the same bytes. `parse_with_context` and
        // `parse_with_model_and_values` keep their trait defaults, which
        // forward the payload to `parse_with_model`.
        self.parse_note(ctx.payload(), model, tags, Some(value_forms))
    }

    fn parse_with_context_and_values_and_session(
        &self,
        ctx: &MakerNoteContext<'_>,
        _byte_order: ByteOrder,
        model: Option<&str>,
        session: &mut crate::exiftool_tables::session::Session,
        cond_ctx: &mut crate::exiftool_tables::Ctx<'_>,
        tags: &mut HashMap<String, String>,
        value_forms: &mut HashMap<String, String>,
    ) -> std::result::Result<(), String> {
        self.parse_note_with_session(
            ctx.payload(),
            // Fuji offsets and the engine's directory start are relative to
            // this payload, not the enclosing TIFF shared by other notes.
            ctx.payload_base(),
            model,
            session,
            cond_ctx,
            tags,
            Some(value_forms),
        )
    }
}

impl FujifilmParser {
    /// Every entry point funnels here: the residual and sub-table walk of the
    /// Main IFD, then (with the `("FujiFilm", "Main")` line in force) the
    /// engine rows.
    /// `model` is `$$self{Model}` for the engine; `forms` receives the
    /// engine rows' `-n` forms (`None` on the detached, value-less paths).
    fn parse_note(
        &self,
        data: &[u8],
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
        forms: Option<&mut HashMap<String, String>>,
    ) -> std::result::Result<(), String> {
        let mut session = crate::exiftool_tables::session::Session::new();
        let mut members = HashMap::new();
        let mut ctx = crate::exiftool_tables::Ctx::new(&mut members);
        self.parse_note_with_session(data, 0, model, &mut session, &mut ctx, tags, forms)
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_note_with_session(
        &self,
        data: &[u8],
        data_domain: u64,
        model: Option<&str>,
        session: &mut crate::exiftool_tables::session::Session,
        ctx: &mut crate::exiftool_tables::Ctx<'_>,
        tags: &mut HashMap<String, String>,
        forms: Option<&mut HashMap<String, String>>,
    ) -> std::result::Result<(), String> {
        if data.is_empty() {
            return Ok(());
        }

        // Validate Fujifilm header
        if !self.validate_header(data) {
            return Err("Invalid Fujifilm MakerNote header".to_string());
        }

        // CRITICAL: Fujifilm MakerNotes ALWAYS use little-endian byte order,
        // regardless of the main EXIF byte order. This is a Fujifilm-specific
        // quirk that differs from most other camera manufacturers.
        let fuji_byte_order = ByteOrder::LittleEndian;

        // Fujifilm header structure:
        // - Bytes 0-7: "FUJIFILM" signature
        // - Bytes 8-11: IFD offset (4 bytes, little-endian, typically 0x0C = 12)
        // - Byte 12+: IFD data starts

        // Read IFD offset using little-endian byte order
        let reader = EndianReader::new(data, fuji_byte_order.to_io_byte_order());
        let ifd_offset = reader.u32_at(8).unwrap_or(0) as usize;

        // Fujifilm offsets are relative to the MakerNote start. One entry list
        // for the residual arms, the sub-table edges and (inside
        // `insert_rows`) the engine: `read_ifd`'s fit rule (the whole entry
        // array must fit; a count of 0 or above 512 is refused) is the one
        // both walks use.
        let Some(entries) = read_ifd(data, ifd_offset, fuji_byte_order.to_io_byte_order()) else {
            return Ok(());
        };

        // Slice I-6: `FujiFilm::Main` through the generated table and the IFD
        // engine, behind Gate B. The lookup is spelled with literal arguments
        // because `tools/exiftool-tables/reachability.py` counts literal
        // `find_ifd_table` call sites, and `enabled()` re-checks Gate A and
        // the line at runtime. Without the `("FujiFilm", "Main")` line in
        // `enabled_ifd.rs` `main_table` is `None` and the engine rows are
        // simply absent: landing 2 retired the hand arms this branch used to
        // fall back to, so the generated table is the only producer for the
        // 88 ids it reports, and the block keeps only the residual and the
        // sub-table rows below. That state is unreachable in any build whose
        // tests pass -- `tests/fujifilm_main_ifd_table.rs::fujifilm_main_is_
        // on_the_gate_b_allowlist` asserts the line is present AND
        // `table.enabled()` (gate A and gate B together) -- and an un-enabled
        // table therefore fails that test loudly. A visibly partial FujiFilm
        // block is preferred to one that silently looks complete.
        let main_table = find_ifd_table("FujiFilm", "Main").filter(|table| table.enabled());

        for (index, raw) in entries.iter().enumerate() {
            // Exif.pm:6463-6478, as the engine applies it
            // (`ifd_engine::accepted_type`): an entry whose type code
            // `ProcessExif` refuses is skipped, and the directory is abandoned
            // when that entry is the first one.
            if !main_engine::entry_type_accepted(raw.field_type) {
                if index == 0 {
                    break;
                }
                continue;
            }
            let entry = IfdEntry {
                tag_id: raw.tag_id,
                field_type: raw.field_type,
                value_count: raw.count,
                value_offset: raw.value_offset,
            };

            // Binary sub-directories. `%FujiFilm::Main` gives these four tags a
            // `SubDirectory => { TagTable => ... }` with no Condition and no
            // Start/Base/ByteOrder override (FujiFilm.pm:341, :345, :349, :609),
            // so ExifTool descends into the record and reports its fields, and
            // reports nothing for the tag itself.
            if let Some(table) = fujifilm_binary_subdir(entry.tag_id) {
                if let Some(record) = entry_bytes(&entry, data) {
                    decode_binary_subdir(table, &record, fuji_byte_order, "FujiFilm", tags);
                }
                continue;
            }

            // Only the residual arms run here. Every engine-owned id, and the
            // hand ids `FujiFilm::Main` does not declare, is skipped; the
            // engine rows go in after this loop (`main_engine`'s module doc
            // argues the order).
            if !main_engine::is_residual(entry.tag_id) {
                continue;
            }

            match entry.tag_id {
                // Version (tag 0x0000): `undef[4]` with no conversion, which
                // the engine emits as `TagValue::Binary` and the string map
                // cannot carry.
                FUJI_VERSION => {
                    if let Some(value) = extract_string_value(&entry, data) {
                        tags.insert("FujiFilm:Version".to_string(), value);
                    }
                }

                // InternalSerialNumber (tag 0x0010): a string with a
                // model-specific PrintConv that decodes an embedded
                // hex-encoded body number and manufacture date.
                FUJI_SERIAL_NUMBER => {
                    if let Some(raw) = extract_string_value_raw(&entry, data) {
                        tags.insert(
                            "FujiFilm:InternalSerialNumber".to_string(),
                            decode_internal_serial_number(&raw),
                        );
                    }
                }

                // NoiseReduction (tag 0x100b; 0x100e, also named
                // NoiseReduction, is an engine row inserted after this loop).
                FUJI_NOISE_REDUCTION => {
                    let value = entry.value_offset as i32;
                    // `RawConv => '$val == 0x100 ? undef : $val'`
                    // (FujiFilm.pm:237): ExifTool reports no tag for 0x100.
                    if value == 0x100 {
                        continue;
                    }
                    tags.insert(
                        "FujiFilm:NoiseReduction".to_string(),
                        DECODE_NOISE_REDUCTION.decode(value).to_string(),
                    );
                }

                // ShadowTone/HighlightTone (tags 0x1040/0x1041):
                // FujiFilm.pm:439-480 -- a hash PrintConv with named
                // breakpoints at every multiple of 16 cameras actually write,
                // plus an `OTHER` fallback (`-$val/16`) for anything else.
                // This printed the bare signed raw value (e.g. "+0") instead
                // of ExifTool's named strings (e.g. "0 (normal)") -- verified
                // wrong against FujiFilmGFX100II.jpg.
                FUJI_SHADOW_TONE => {
                    tags.insert(
                        "FujiFilm:ShadowTone".to_string(),
                        decode_fuji_tone(entry.value_offset as i32),
                    );
                }
                FUJI_HIGHLIGHT_TONE => {
                    tags.insert(
                        "FujiFilm:HighlightTone".to_string(),
                        decode_fuji_tone(entry.value_offset as i32),
                    );
                }

                // Image Stabilization (tag 0x1422). FujiFilm.pm:790-806: a 3x
                // int16u array (Count => 3). 3 * 2 = 6 bytes never fits in the
                // 4-byte inline value_offset field, so value_offset is always
                // a pointer into `data` -- reading it directly as a scalar
                // (the old code) decoded the file offset as if it were the
                // tag value. Element 0 and element 1 each have their own
                // PrintConv hash (array PrintConv); element 2 has none and
                // prints as the raw number, joined with "; " to match
                // ExifTool's list rendering.
                FUJI_IMAGE_STABILIZATION => {
                    if let Some(array) = extract_u16_array(&entry, data, fuji_byte_order)
                        && array.len() >= 3
                    {
                        let parts = [
                            DECODE_IMAGE_STABILIZATION.decode(array[0] as i32),
                            DECODE_IMAGE_STABILIZATION_MODE.decode(array[1] as i32),
                            array[2].to_string(),
                        ];
                        tags.insert("FujiFilm:ImageStabilization".to_string(), parts.join("; "));
                    }
                }

                // FaceElementTypes (tag 0x4201): FujiFilm.pm:954-981 --
                // `Writable => 'int8u'`, but this is a genuine per-entry TIFF
                // IFD (not a ProcessBinaryData record), so what actually gets
                // read is whatever type+count the file's own IFD entry
                // declares -- verified on FujiFilmFinePixZ900EXR.jpg, whose
                // entry is `int16u[1]` (field_type SHORT), not `int8u`.
                // `extract_uint_array` below reads either width; each value
                // (regardless of width) is looked up in the same PrintConv
                // map, joined by ", " ('REPEAT' PrintConv over an array).
                FUJI_FACE_ELEMENT_TYPES => {
                    let width = match entry.field_type {
                        1 => Some(1usize), // BYTE
                        3 => Some(2usize), // SHORT
                        _ => None,
                    };
                    if let Some(width) = width
                        && let Some(values) =
                            extract_uint_array(&entry, data, fuji_byte_order, width)
                        && !values.is_empty()
                    {
                        let types: Vec<String> = values
                            .iter()
                            .map(|&v| decode_face_element_type(v))
                            .collect();
                        tags.insert("FujiFilm:FaceElementTypes".to_string(), types.join(", "));
                    }
                }

                _ => {}
            }
        }

        // Engine rows LAST, in emission order (= IFD entry order).
        if let Some(table) = main_table {
            main_engine::insert_rows(
                table,
                data,
                data_domain,
                ifd_offset,
                model,
                session,
                ctx,
                tags,
                forms,
            );
        }

        Ok(())
    }
}

/// Extracts string value from IFD entry
///
/// Handles both inline strings (≤4 bytes) and offset-based strings
fn extract_string_value(entry: &IfdEntry, full_data: &[u8]) -> Option<String> {
    let byte_count = entry.value_count as usize;

    // For inline strings (≤4 bytes), value is in value_offset field
    if byte_count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        let s = std::str::from_utf8(&bytes[0..byte_count])
            .ok()?
            .trim_end_matches('\0')
            .trim();
        return Some(s.to_string());
    }

    // For longer strings, read from offset
    // Fujifilm offsets are relative to MakerNote start
    let offset = entry.value_offset as usize;

    if offset + byte_count <= full_data.len() {
        let bytes = &full_data[offset..offset + byte_count];
        let s = std::str::from_utf8(bytes)
            .ok()?
            .trim_end_matches('\0')
            .trim();
        return Some(s.to_string());
    }

    None
}

/// Extracts a string value from an IFD entry without trimming internal or
/// trailing whitespace (only null terminators are stripped).
///
/// Some Fujifilm string tags (e.g. Quality, stored as `"NORMAL \0"`) include
/// a meaningful trailing space that ExifTool preserves in its output;
/// [`extract_string_value`] would incorrectly strip it via `.trim()`.
fn extract_string_value_raw(entry: &IfdEntry, full_data: &[u8]) -> Option<String> {
    let byte_count = entry.value_count as usize;

    if byte_count <= 4 {
        let bytes = entry.value_offset.to_le_bytes();
        let s = std::str::from_utf8(&bytes[0..byte_count])
            .ok()?
            .trim_end_matches('\0');
        return Some(s.to_string());
    }

    let offset = entry.value_offset as usize;

    if offset + byte_count <= full_data.len() {
        let bytes = &full_data[offset..offset + byte_count];
        let s = std::str::from_utf8(bytes).ok()?.trim_end_matches('\0');
        return Some(s.to_string());
    }

    None
}

/// Reads `count` (`entry.value_count`) unsigned integers of `width` bytes (1
/// or 2) from an IFD entry, handling both the inline case (the whole array
/// fits in the entry's own 4-byte `value_offset` field) and the out-of-line
/// case, widened to `u32`.
///
/// The generic `extract_array`/`extract_u16_array` this file otherwise uses
/// only handles the out-of-line case -- correct for an array that is always
/// larger than 4 bytes (`FacePositions`, `Count => -1`), wrong for one that
/// can be small enough to be inline (`FaceElementTypes` at `Count => 1`,
/// verified against `FujiFilmFinePixZ900EXR.jpg`).
fn extract_uint_array(
    entry: &IfdEntry,
    data: &[u8],
    byte_order: ByteOrder,
    width: usize,
) -> Option<Vec<u32>> {
    let count = entry.value_count as usize;
    if count == 0 || width == 0 {
        return None;
    }
    let total = count.checked_mul(width)?;
    let src: std::borrow::Cow<'_, [u8]> = if total <= 4 {
        let bytes = match byte_order {
            ByteOrder::LittleEndian => entry.value_offset.to_le_bytes(),
            ByteOrder::BigEndian => entry.value_offset.to_be_bytes(),
        };
        std::borrow::Cow::Owned(bytes[..total].to_vec())
    } else {
        let offset = entry.value_offset as usize;
        std::borrow::Cow::Borrowed(data.get(offset..offset.checked_add(total)?)?)
    };
    let reader = EndianReader::new(&src, byte_order.to_io_byte_order());
    (0..count)
        .map(|i| match width {
            1 => src.get(i).map(|&b| u32::from(b)),
            2 => reader.u16_at(i * 2).map(u32::from),
            _ => None,
        })
        .collect()
}

/// FujiFilm.pm:439-480 (`ShadowTone`/`HighlightTone`'s shared `PrintConv`
/// shape -- two separate hashes with identical keys/values). The named
/// breakpoints take priority; `OTHER` (`-$val/16`) covers anything else a
/// camera might write outside them.
fn decode_fuji_tone(value: i32) -> String {
    match value {
        -64 => "+4 (hardest)".to_string(),
        -48 => "+3 (very hard)".to_string(),
        -32 => "+2 (hard)".to_string(),
        -16 => "+1 (medium hard)".to_string(),
        0 => "0 (normal)".to_string(),
        16 => "-1 (medium soft)".to_string(),
        32 => "-2 (soft)".to_string(),
        other => perl_number(f64::from(-other) / 16.0),
    }
}

/// FujiFilm.pm:954-981 (`FaceElementTypes`'s `PrintConv`, `'REPEAT'`'d over
/// the array). An unlisted value prints ExifTool's default `Unknown (n)`
/// (`ExifTool.pm:3633`), the same fallback `exiftool_tables::PrintConv`
/// documents.
fn decode_face_element_type(value: u32) -> String {
    match value {
        1 => "Face".to_string(),
        2 => "Left Eye".to_string(),
        3 => "Right Eye".to_string(),
        7 => "Body".to_string(),
        8 => "Head".to_string(),
        9 => "Both Eyes".to_string(),
        11 => "Bike".to_string(),
        12 => "Body of Car".to_string(),
        13 => "Front of Car".to_string(),
        14 => "Animal Body".to_string(),
        15 => "Animal Head".to_string(),
        16 => "Animal Face".to_string(),
        17 => "Animal Left Eye".to_string(),
        18 => "Animal Right Eye".to_string(),
        19 => "Bird Body".to_string(),
        20 => "Bird Head".to_string(),
        21 => "Bird Left Eye".to_string(),
        22 => "Bird Right Eye".to_string(),
        23 => "Aircraft Body".to_string(),
        25 => "Aircraft Cockpit".to_string(),
        26 => "Train Front".to_string(),
        27 => "Train Cockpit".to_string(),
        28 => "Animal Head (28)".to_string(),
        29 => "Animal Body (29)".to_string(),
        other => format!("Unknown ({other})"),
    }
}

/// Decodes Fujifilm's InternalSerialNumber (tag 0x0010) using the same
/// heuristic as ExifTool's FujiFilm.pm PrintConv.
///
/// The raw string ends with a hex-encoded camera body number followed by a
/// 6-digit manufacture date (`yymmdd`) and a fixed 12-character trailer. For
/// example, the raw string `"FPX20582698 592D313134360702198C0020100A84"`
/// decodes to `"FPX20582698 Y-1146 2007:02:19 8C0020100A84"`.
///
/// Falls back to the (already-trimmed) raw string unchanged if it doesn't
/// match the expected shape (e.g. some models use a slightly different
/// layout that ExifTool handles via a separate substitution, which is not
/// replicated here).
fn decode_internal_serial_number(raw: &str) -> String {
    let trimmed = raw.trim_end_matches(['\0', ' ', '\t', '\r', '\n']);
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() < 18 {
        return trimmed.to_string();
    }

    let split_at = chars.len() - 18;
    let prefix_chars = &chars[..split_at];
    let suffix: String = chars[split_at..].iter().collect();

    let yy = &suffix[0..2];
    let mm = &suffix[2..4];
    let dd = &suffix[4..6];
    let rest12 = &suffix[6..18];

    let (Some(_yy_num), Some(mm_num), Some(dd_num)) = (
        yy.parse::<u32>().ok(),
        mm.parse::<u32>().ok(),
        dd.parse::<u32>().ok(),
    ) else {
        return trimmed.to_string();
    };
    if !(1..=12).contains(&mm_num) || !(1..=31).contains(&dd_num) {
        return trimmed.to_string();
    }
    let yy_num: u32 = yy.parse().unwrap_or(0);

    // group2: the maximal suffix of the prefix consisting only of hex digits
    // (mirrors the greedy `[0-9a-fA-F]*` capture in ExifTool's regex, given
    // the lazy prefix capture ahead of it).
    let mut hex_start = prefix_chars.len();
    while hex_start > 0 && prefix_chars[hex_start - 1].is_ascii_hexdigit() {
        hex_start -= 1;
    }
    let group1: String = prefix_chars[..hex_start].iter().collect();
    let hex_run: Vec<char> = prefix_chars[hex_start..].to_vec();

    // pack('H*', ...): decode pairs of hex digits into bytes. A trailing
    // lone hex digit is treated as a high nibble with an implicit zero low
    // nibble, matching Perl's pack behavior for odd-length hex strings.
    let mut decoded_bytes = Vec::with_capacity(hex_run.len().div_ceil(2));
    let mut i = 0;
    while i < hex_run.len() {
        let hi = hex_run[i].to_digit(16).unwrap_or(0);
        let lo = if i + 1 < hex_run.len() {
            hex_run[i + 1].to_digit(16).unwrap_or(0)
        } else {
            0
        };
        decoded_bytes.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    let sn: String = decoded_bytes
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '.'
            }
        })
        .collect();

    let year = if yy_num < 70 {
        yy_num + 2000
    } else {
        yy_num + 1900
    };

    format!("{}{} {}:{}:{} {}", group1, sn, year, mm, dd, rest12)
}

/// Public function to parse Fujifilm MakerNotes
///
/// This is the main entry point for parsing Fujifilm MakerNote data.
///
/// # Parameters
/// - `data`: Raw MakerNote data (including Fujifilm header)
/// - `byte_order`: Byte order for parsing multi-byte values
/// - `tags`: HashMap to populate with extracted tags
/// The `%FujiFilm::Main` tags whose ExifTool entry is a `SubDirectory` over a
/// `ProcessBinaryData` table, and the table each one selects.
///
/// All four are packed settings words: one `int16u` or `int32u` holding several
/// nibble-wide fields that ExifTool splits with `Mask`. Reading the word as a
/// value would report a single meaningless number; not reading it at all, which
/// is what happened before, reports nothing.
const fn fujifilm_binary_subdir(tag_id: u16) -> Option<&'static BinaryTable> {
    match tag_id {
        FUJI_PRIORITY_SETTINGS => Some(&FUJIFILM_PRIORITYSETTINGS),
        FUJI_FOCUS_SETTINGS => Some(&FUJIFILM_FOCUSSETTINGS),
        FUJI_AFC_SETTINGS => Some(&FUJIFILM_AFCSETTINGS),
        FUJI_DRIVE_SETTINGS => Some(&FUJIFILM_DRIVESETTINGS),
        _ => None,
    }
}

/// The raw bytes of an entry's value.
///
/// Same offset convention as [`extract_string_value`]: up to four bytes live in
/// the entry's own value field, in the MakerNote's (always little-endian) byte
/// order, and anything longer is at an offset measured from the MakerNote start.
/// All four settings tags are a single `int16u`/`int32u` and so always inline,
/// but a record read from an offset is the general shape of a sub-directory.
fn entry_bytes(entry: &IfdEntry, full_data: &[u8]) -> Option<Vec<u8>> {
    let byte_count = (entry.value_count as usize)
        .checked_mul(ifd_type_size(entry.field_type))
        .filter(|n| *n > 0)?;
    if byte_count <= 4 {
        return Some(entry.value_offset.to_le_bytes()[..byte_count].to_vec());
    }
    let offset = entry.value_offset as usize;
    full_data
        .get(offset..offset.checked_add(byte_count)?)
        .map(<[u8]>::to_vec)
}

/// Bytes per element of a TIFF field type, or 0 for one this reader does not
/// know -- the caller then produces nothing rather than a mis-sized record.
const fn ifd_type_size(field_type: u16) -> usize {
    match field_type {
        1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => 2,         // SHORT, SSHORT
        4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
        5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
        _ => 0,
    }
}

pub fn parse_fujifilm_makernotes(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    let parser = FujifilmParser;
    if let Err(e) = parser.parse(data, byte_order, tags) {
        eprintln!("FujiFilm MakerNotes parse error: {}", e);
    }
}

/// Checks if data appears to be a Fujifilm MakerNote
///
/// # Parameters
/// - `data`: Raw byte data to check
///
/// # Returns
/// `true` if the data appears to be a Fujifilm MakerNote, `false` otherwise
pub fn is_fujifilm_makernote(data: &[u8]) -> bool {
    data.len() >= 12 && &data[0..8] == FUJIFILM_HEADER
}

/// Staleness/consistency test (tag-machinery overhaul Step 16): registers the
/// Stage 1 Step 2 fact named in `OVERHAUL_PROGRESS.md` -- `ImageStabilization`
/// (tag 0x1422)'s two element hashes -- against `dump_tables.pl`'s output for
/// the pinned ExifTool tree.
///
/// Calls the real production decoders, `DECODE_IMAGE_STABILIZATION` and
/// `DECODE_IMAGE_STABILIZATION_MODE`, for every key ExifTool's current
/// `FujiFilm.pm:790-804` declares.
///
/// Fixture: `tools/exiftool-tables/fixtures/fujifilm_image_stabilization.json`.
#[cfg(test)]
mod staleness_tests {
    use super::*;
    use std::collections::BTreeMap;

    const FIXTURE: &str = include_str!(
        "../../../../tools/exiftool-tables/fixtures/fujifilm_image_stabilization.json"
    );

    #[derive(serde::Deserialize)]
    struct Fixture {
        element0: BTreeMap<String, String>,
        element1: BTreeMap<String, String>,
    }

    #[test]
    fn image_stabilization_matches_fujifilm_pm() {
        let f: Fixture =
            serde_json::from_str(FIXTURE).expect("fujifilm_image_stabilization.json is valid JSON");
        assert_eq!(
            f.element0.len(),
            6,
            "ImageStabilization element-0 map size changed"
        );
        assert_eq!(
            f.element1.len(),
            3,
            "ImageStabilization element-1 map size changed"
        );

        let mut mismatches = Vec::new();
        for (k, expected) in &f.element0 {
            let id: i32 = k.parse().expect("fixture key is not an integer");
            let got = DECODE_IMAGE_STABILIZATION.decode(id);
            if &got != expected {
                mismatches.push(format!(
                    "element0 id {id}: got {got:?}, ExifTool says {expected:?}"
                ));
            }
        }
        for (k, expected) in &f.element1 {
            let id: i32 = k.parse().expect("fixture key is not an integer");
            let got = DECODE_IMAGE_STABILIZATION_MODE.decode(id);
            if &got != expected {
                mismatches.push(format!(
                    "element1 id {id}: got {got:?}, ExifTool says {expected:?}"
                ));
            }
        }
        assert!(
            mismatches.is_empty(),
            "FujiFilm ImageStabilization decoders have drifted from FujiFilm.pm:790-804:\n  {}",
            mismatches.join("\n  ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal FujiFilm MakerNote carrying exactly one IFD entry:
    /// "FUJIFILM" + LE u32 IFD offset (12) + LE u16 entry count + the entry.
    fn fuji_makernote(tag_id: u16, field_type: u16, count: u32, value: u32) -> Vec<u8> {
        let mut out = Vec::from(*b"FUJIFILM");
        out.extend_from_slice(&12u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&tag_id.to_le_bytes());
        out.extend_from_slice(&field_type.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&value.to_le_bytes());
        out
    }

    /// FujiFilm.pm:481-486 gives tag 0x1044 `ValueConv => '$val / 8'` and no
    /// PrintConv, so ExifTool prints the bare quotient.
    ///
    /// Ground truth, ExifTool 13.59 via
    /// `/tmp/oxidex-exiftool-cache/exiftool-pinned.sh -s3 -DigitalZoom`
    /// (`-ver` 13.59 and the OOXML.docx capability probe both asserted first):
    ///
    ///   combined-samples/FujiFilm/FujiFilmFinePixZ950EXR.jpg -> `4`
    ///     (`-v3` shows `Tag 0x1044 (4 bytes, int32u[1]): 20 00 00 00`, raw 32)
    ///   combined-samples/FujiFilm/FujiFilmX-S1.jpg           -> `0`
    ///
    /// Before this pin the arm divided by 100 and appended an `x` ExifTool
    /// never emits, printing "0.32x" and "0.00x" for those two files.
    #[test]
    fn fujifilm_digital_zoom_is_raw_over_eight_with_no_unit() {
        let mut tags = HashMap::new();
        // int32u = field type 4, count 1.
        parse_fujifilm_makernotes(
            &fuji_makernote(0x1044, 4, 1, 32),
            ByteOrder::LittleEndian,
            &mut tags,
        );
        assert_eq!(
            tags.get("FujiFilm:DigitalZoom").map(String::as_str),
            Some("4")
        );

        let mut tags = HashMap::new();
        parse_fujifilm_makernotes(
            &fuji_makernote(0x1044, 4, 1, 0),
            ByteOrder::LittleEndian,
            &mut tags,
        );
        assert_eq!(
            tags.get("FujiFilm:DigitalZoom").map(String::as_str),
            Some("0")
        );
    }

    /// The quotient is not always an integer: Perl stringifies `$val / 8`, so
    /// a raw that is not a multiple of 8 prints as a decimal (12/8 -> "1.5",
    /// 1/8 -> "0.125"), and never with a trailing ".0" or an "x".
    #[test]
    fn fujifilm_digital_zoom_prints_fractional_quotients_like_perl() {
        for (raw, expected) in [(12u32, "1.5"), (1, "0.125"), (4, "0.5"), (7, "0.875")] {
            let mut tags = HashMap::new();
            parse_fujifilm_makernotes(
                &fuji_makernote(0x1044, 4, 1, raw),
                ByteOrder::LittleEndian,
                &mut tags,
            );
            assert_eq!(
                tags.get("FujiFilm:DigitalZoom").map(String::as_str),
                Some(expected),
                "raw {raw}"
            );
        }
    }

    #[test]
    fn test_fujifilm_header_validation() {
        let parser = FujifilmParser;

        // Valid Fujifilm header
        let valid_header = b"FUJIFILM\x0C\x00\x00\x00extra data";
        assert!(parser.validate_header(valid_header));

        // Invalid header (wrong signature)
        let invalid = b"CANON\0\x00\x00\x00\x00\x00\x00";
        assert!(!parser.validate_header(invalid));

        // Too short
        let too_short = b"FUJIFILM\x0C";
        assert!(!parser.validate_header(too_short));
    }

    #[test]
    fn test_is_fujifilm_makernote() {
        assert!(is_fujifilm_makernote(b"FUJIFILM\x0C\x00\x00\x00test"));
        assert!(!is_fujifilm_makernote(b"NIKON\0\x00\x00"));
        assert!(!is_fujifilm_makernote(b"FUJIFILM\x0C")); // Too short
    }

    #[test]
    fn test_parser_trait_implementation() {
        let parser = FujifilmParser;
        assert_eq!(parser.manufacturer_name(), "FujiFilm");
        assert_eq!(parser.tag_prefix(), "FujiFilm:");
    }

    /// Exactly the four tags with a `SubDirectory` select a table, and no
    /// neighbour does -- binding one to the wrong id would print a real
    /// ExifTool name over an unrelated word.
    #[test]
    fn test_only_the_four_settings_tags_select_a_table() {
        for tag in [0x102Bu16, 0x102D, 0x102E, 0x1103] {
            assert!(fujifilm_binary_subdir(tag).is_some(), "{tag:#06x}");
        }
        for tag in [0x102Au16, 0x102C, 0x102F, 0x1102, 0x1104, 0x1105] {
            assert!(fujifilm_binary_subdir(tag).is_none(), "{tag:#06x}");
        }
    }

    fn decode_settings(table: &BinaryTable, record: &[u8]) -> HashMap<String, String> {
        let mut tags = HashMap::new();
        decode_binary_subdir(
            table,
            record,
            ByteOrder::LittleEndian,
            "FujiFilm",
            &mut tags,
        );
        tags
    }

    /// `combined-samples/FujiFilm/FujiFilmX-S20.jpg`: the exact record bytes
    /// `exiftool -v3` prints for tags 0x102b/0x102d/0x102e/0x1103, and the exact
    /// values `exiftool -a -G1 -s` reports for them.
    ///
    /// This body is the one in the corpus that exercises `AFAreaZoneSize`'s
    /// `OTHER` sub: 0x102d is `01 01 63 00`, whose 0xff0000 field is 0x63, and
    /// ExifTool prints `3 x 3` -- `$val & 0x0f` and `$val >> 5`, which a `>> 4`
    /// would render `3 x 6`.
    #[test]
    fn test_settings_match_exiftool_on_x_s20_bytes() {
        let tags = decode_settings(&FUJIFILM_PRIORITYSETTINGS, &[0x12, 0x00]);
        assert_eq!(tags["FujiFilm:AF-SPriority"], "Focus");
        assert_eq!(tags["FujiFilm:AF-CPriority"], "Release");

        let tags = decode_settings(&FUJIFILM_FOCUSSETTINGS, &[0x01, 0x01, 0x63, 0x00]);
        assert_eq!(tags["FujiFilm:FocusMode2"], "AF-S");
        assert_eq!(tags["FujiFilm:PreAF"], "Off");
        assert_eq!(tags["FujiFilm:AFAreaMode"], "Zone");
        assert_eq!(tags["FujiFilm:AFAreaPointSize"], "n/a");
        assert_eq!(tags["FujiFilm:AFAreaZoneSize"], "3 x 3");

        let tags = decode_settings(&FUJIFILM_AFCSETTINGS, &[0x02, 0x01, 0x00, 0x00]);
        assert_eq!(tags["FujiFilm:AF-CSetting"], "Set 1 (multi-purpose)");
        assert_eq!(tags["FujiFilm:AF-CTrackingSensitivity"], "2");
        assert_eq!(tags["FujiFilm:AF-CSpeedTrackingSensitivity"], "0");
        assert_eq!(tags["FujiFilm:AF-CZoneAreaSwitching"], "Auto");

        let tags = decode_settings(&FUJIFILM_DRIVESETTINGS, &[0x00, 0x00, 0x00, 0x00]);
        assert_eq!(tags["FujiFilm:DriveMode"], "Single");
        assert_eq!(tags["FujiFilm:DriveSpeed"], "n/a");
    }

    /// `combined-samples/FujiFilm/FujiFilmGFX50S_II.jpg` tag 0x102d, the corpus
    /// case for `AFAreaPointSize`'s `OTHER` sub: 0x40 in the 0xf000 field is 4,
    /// not one of the hash's keys, so ExifTool prints the number itself.
    #[test]
    fn test_af_area_point_size_falls_through_to_the_number() {
        let tags = decode_settings(&FUJIFILM_FOCUSSETTINGS, &[0x01, 0x40, 0x00, 0x00]);
        assert_eq!(tags["FujiFilm:AFAreaPointSize"], "4");
        assert_eq!(tags["FujiFilm:AFAreaMode"], "Single Point");
        assert_eq!(tags["FujiFilm:AFAreaZoneSize"], "n/a");
    }

    /// `combined-samples/FujiFilm/FujiFilmX-H2S.jpg` tag 0x102d: the low nibble
    /// is the whole of `FocusMode2`, so an unmasked read of the int32u would
    /// report 514 instead of `AF-C`.
    #[test]
    fn test_masks_split_one_word_into_its_fields() {
        let tags = decode_settings(&FUJIFILM_FOCUSSETTINGS, &[0x02, 0x02, 0x00, 0x00]);
        assert_eq!(tags["FujiFilm:FocusMode2"], "AF-C");
        assert_eq!(tags["FujiFilm:AFAreaMode"], "Wide/Tracking");
    }

    // ImageStabilization (tag 0x1422). FujiFilm.pm:790-806 (ExifTool 13.59).
    //
    //     0x1422 => {
    //         Name => 'ImageStabilization',
    //         Writable => 'int16u',
    //         Count => 3,
    //         PrintConv => [{
    //             0 => 'None',
    //             1 => 'Optical', #PH
    //             2 => 'Sensor-shift', #PH (now IBIS/OIS, ref forum13708)
    //             3 => 'OIS Lens', #forum9815 (optical+sensor?)
    //             258 => 'IBIS/OIS + DIS', #forum13708 (digital on top of IBIS/OIS)
    //             512 => 'Digital', #PH
    //         },{
    //             0 => 'Off',
    //             1 => 'On (mode 1, continuous)',
    //             2 => 'On (mode 2, shooting only)',
    //         }],
    //     },
    //
    // verified against the pinned oracle (13.59) on
    // stage1-samples/FujiFilm/FujiFilmX-S10.jpg:
    //   `[FujiFilm]      ImageStabilization              : OIS Lens; On (mode 1, continuous); 0`

    /// Element 0 -- the IS system (FujiFilm.pm:795-800). There is no 256 key
    /// in ExifTool's hash; that entry (and the other four labels) were
    /// invented in an earlier revision of this file.
    #[test]
    fn test_decode_image_stabilization_system() {
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(0), "None");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(1), "Optical");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(2), "Sensor-shift");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(3), "OIS Lens");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(258), "IBIS/OIS + DIS");
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(512), "Digital");
        // 256 was the invented key this replaced; it is not in FujiFilm.pm
        // and must not decode to any of the real labels.
        assert_eq!(DECODE_IMAGE_STABILIZATION.decode(256), "Unknown (256)");
    }

    /// Element 1 -- the IS mode (FujiFilm.pm:801-804).
    #[test]
    fn test_decode_image_stabilization_mode() {
        assert_eq!(DECODE_IMAGE_STABILIZATION_MODE.decode(0), "Off");
        assert_eq!(
            DECODE_IMAGE_STABILIZATION_MODE.decode(1),
            "On (mode 1, continuous)"
        );
        assert_eq!(
            DECODE_IMAGE_STABILIZATION_MODE.decode(2),
            "On (mode 2, shooting only)"
        );
    }

    /// End-to-end pointer decode: 6 bytes (3x int16u) never fit in the 4-byte
    /// inline `value_offset` field, so `value_offset` must be read as a
    /// pointer into the MakerNote body, not as the tag's value itself (the
    /// bug this test pins). Bytes are hand-embedded, not read from any /tmp
    /// path:
    //
    //   offset  0..8   "FUJIFILM"
    //   offset  8..12  IFD offset = 12                (u32 LE)
    //   offset 12..14  entry_count = 1                (u16 LE)
    //   offset 14..16  tag_id = 0x1422                (u16 LE)
    //   offset 16..18  field_type = 3 (SHORT)         (u16 LE)
    //   offset 18..22  value_count = 3                (u32 LE)
    //   offset 22..26  value_offset = 26 (pointer)     (u32 LE)
    //   offset 26..32  array data: 3, 1, 0             (3x u16 LE)
    //
    // array data (3, 1, 0) mirrors the oracle-observed
    // FujiFilmX-S10.jpg encoding: element 0 = 3 ("OIS Lens"), element 1 = 1
    // ("On (mode 1, continuous)"), element 2 = 0 (no PrintConv, raw number).
    #[test]
    fn test_image_stabilization_reads_pointer_not_inline_value() {
        let data: &[u8] = &[
            0x46, 0x55, 0x4a, 0x49, 0x46, 0x49, 0x4c, 0x4d, // "FUJIFILM"
            0x0c, 0x00, 0x00, 0x00, // IFD offset = 12
            0x01, 0x00, // entry_count = 1
            0x22, 0x14, // tag_id = 0x1422
            0x03, 0x00, // field_type = SHORT
            0x03, 0x00, 0x00, 0x00, // value_count = 3
            0x1a, 0x00, 0x00, 0x00, // value_offset = 26
            0x03, 0x00, 0x01, 0x00, 0x00, 0x00, // array data: 3, 1, 0
        ];

        let parser = FujifilmParser;
        let mut tags = HashMap::new();
        parser
            .parse(data, ByteOrder::LittleEndian, &mut tags)
            .expect("synthetic FujiFilm MakerNote should parse");

        assert_eq!(
            tags["FujiFilm:ImageStabilization"],
            "OIS Lens; On (mode 1, continuous); 0"
        );
    }

    /// Never-approximate rule (AGENTS.md): if the pointed-to array data is
    /// truncated (value_offset run off the end of the buffer),
    /// `extract_u16_array` returns `None` and the tag must be omitted
    /// entirely -- not inserted with a value read from garbage/out-of-bounds
    /// memory, and not partially filled from 1 or 2 of the 3 elements.
    #[test]
    fn test_image_stabilization_omits_when_pointer_is_out_of_bounds() {
        let data: &[u8] = &[
            0x46, 0x55, 0x4a, 0x49, 0x46, 0x49, 0x4c, 0x4d, // "FUJIFILM"
            0x0c, 0x00, 0x00, 0x00, // IFD offset = 12
            0x01, 0x00, // entry_count = 1
            0x22, 0x14, // tag_id = 0x1422
            0x03, 0x00, // field_type = SHORT
            0x03, 0x00, 0x00, 0x00, // value_count = 3
            0xff, 0x00, 0x00, 0x00, // value_offset = 255 -- past end of buffer
        ];

        let parser = FujifilmParser;
        let mut tags = HashMap::new();
        parser
            .parse(data, ByteOrder::LittleEndian, &mut tags)
            .expect("a truncated array pointer must not fail the whole parse");

        assert!(
            !tags.contains_key("FujiFilm:ImageStabilization"),
            "out-of-bounds array pointer must omit the tag, not approximate one"
        );
    }
}
