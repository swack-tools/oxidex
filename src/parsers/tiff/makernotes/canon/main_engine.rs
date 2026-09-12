//! `Canon::Main` through the generated `IFD_CANON_MAIN` table and the IFD
//! engine (slice I-5, landing 1).
//!
//! # What this module owns
//!
//! The scalar rows of `%Image::ExifTool::Canon::Main` (Canon.pm:1222-2210,
//! pinned 13.59) that the generated table reports -- every `Omitted::NONE`,
//! non-`SubDirectory`, non-`Unknown` tag, plus the D30 alternative of the
//! 0x000c `SerialNumber` group:
//!
//! | id | tag | id | tag |
//! |---|---|---|---|
//! | 0x0006 | CanonImageType | 0x0081 | RawDataOffset |
//! | 0x0007 | CanonFirmwareVersion | 0x0082 | RawDataLength |
//! | 0x0009 | OwnerName | 0x0095 | LensModel |
//! | 0x000c | SerialNumber (`/EOS D30\b/` only) | 0x0097 | DustRemovalData |
//! | 0x000e | CanonFileLength | 0x00ae | ColorTemperature |
//! | 0x0010 | CanonModelID | 0x00b4 | ColorSpace |
//! | 0x0013 | ThumbnailImageValidArea | 0x4010 | CustomPictureStyleFileName |
//! | 0x0015 | SerialNumberFormat | 0x001a | SuperMacro |
//! | 0x001c | DateStampMode | | |
//!
//! Which ids those are is never written down by hand: [`MainEngineRows::owns`]
//! derives it from the static, and [`CANON_MAIN_RESIDUAL_IDS`] -- the ids whose
//! hand arm stays the producer -- is pinned against the generated withholding
//! (`canon_main_residual_matches_the_generated_withholding`).
//!
//! # Why the rows are buffered and replayed
//!
//! `parse_canon_makernote_directory`'s hand walk inserts into one
//! `HashMap<String, String>` in IFD entry order, so the last insert under a
//! key wins -- ExifTool's rule for two tags of equal priority, which is what
//! `Canon::Main`, `Canon::Processing` and `Canon::ColorInfo` are (none has a
//! `PRIORITY`). Two Main names are redeclared by such a sub-table and land
//! under the same `Canon:<Name>` key:
//!
//! * 0x00ae `ColorTemperature` against `%Canon::Processing` key 9 (0x00a0);
//! * 0x00b4 `ColorSpace` against `%Canon::ColorInfo` key 3 (0x4003; not
//!   decoded by `binary_tables` today, which carries ColorTone only).
//!
//! Inserting every engine row before the hand walk would let a later
//! Processing record overwrite the Main 0x00ae value; inserting them after
//! would let Main beat a later Processing record. ExifTool processes a
//! `SubDirectory` inline, in entry order (Exif.pm:6919-7153), so the engine's
//! row for an entry goes into the map exactly when the hand walk reaches that
//! entry ([`MainEngineRows::replay`]), and only rows for entries the hand walk
//! never reached go in afterwards ([`MainEngineRows::drain`]).
//!
//! Last-wins is ExifTool's visible winner only while the newest copy of the
//! name also holds ExifTool's primary tag key, as it does for
//! `ColorTemperature`: pinned 13.59 prints the later Canon copy under both
//! `-j -G1` and text `-G1`, in both IFD orders (crafted from
//! Canon1DmkIII.jpg, 0x00ae placed before and after 0x00a0). `ColorSpace` is
//! different: `ExifIFD:ColorSpace` is found after the maker note and takes
//! the primary key, so the Canon copies are all numbered and `-j -G1` prints
//! the EARLIEST of them while text `-G1` prints the latest (crafted from
//! CanonEOS-1DmkII.jpg, 0x00b4 = 2 placed before and after 0x4003). Today
//! there is one Canon `ColorSpace` producer, so the replay is exact; whoever
//! decodes ColorInfo index 3 has to settle that winner against the `-j`
//! oracle first, because the replay here would give the text winner.
//!
//! # The fence
//!
//! The walk is top level only. A Main `SubDirectory` edge descends the day
//! its target lands on an allowlist -- even for another call site -- and 58
//! of them have no `Validate` to stop it, several with a live hand decoder
//! (FocalLength, WBInfo, CropInfo, AspectInfo, ColorInfo, LensInfo,
//! CameraInfo*, ColorData). [`is_canon_main_row`] drops every row that is not
//! `Canon::Main`'s own, and `canon_main_edges_reach_no_enabled_table` turns
//! the day a target gets enabled into a red test instead of a silent change.

use std::collections::HashMap;

use crate::exiftool_tables::{
    Ctx, Emitted, IfdDir, IfdTable, IfdTag, MemberValue, process_exif, read_ifd,
};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::parsers::tiff::makernotes::shared::engine_value::engine_value_text;
use crate::parsers::tiff::makernotes::shared::ifd_parser_base::IfdParserConfig;
use crate::parsers::tiff::makernotes::shared::tag_priority::insert_low_priority;

/// The `Canon::Main` ids whose hand arm in `parse_canon_makernote_directory`
/// stays the producer while the engine runs. Sorted (binary-searched by
/// [`MainEngineRows::owns`]). Each is withheld by the generated table or not
/// transcribed at all, except 0x000c, which is split per alternative:
///
/// * 0x0008 `FileNumber` -- `omitted.print_conv`
///   (`$_=$val,s/(\d+)(\d{4})/$1-$2/,$_`, Canon.pm:1260);
/// * 0x000c `SerialNumber` -- alternatives 2 (`/EOS-1D/`, `sprintf("%.6u")`)
///   and 3 (Always, `sprintf("%.10u")`) are `omitted.print_conv`; alternative
///   1 (`/EOS D30\b/`) is the engine's, which the arm defers to through
///   [`serial_number_is_d30`];
/// * 0x001e `FirmwareRevision` -- `omitted.print_conv` (multi-statement
///   `q{}`);
/// * 0x0023 `Categories` -- `omitted.value_conv` + `condition`
///   (`$$valPt =~ /^\x08\0\0\0/`);
/// * 0x0028 `ImageUniqueID` -- `omitted.raw_conv` (all-zero -> undef);
/// * 0x0038 `BatteryType` -- `omitted.raw_conv` + `condition`
///   (`$count == 76`);
/// * 0x0083 `OriginalDecisionDataOffset` -- not transcribed
///   (`IsOffset`/`OffsetPair`/`DataTag`, codegen `ifd_isoffset_unsupported`);
/// * 0x0096 `InternalSerialNumber` -- alternative 2 is `omitted.value_conv`
///   (`$val=~s/\xff+$//`); alternative 1 is the `/EOS 5D/` SerialInfo edge;
/// * 0x00d0 `VRDOffset` -- not transcribed (`OffsetPair`/`DataTag`);
/// * 0x4008 `PictureStyleUserDef`, 0x4009 `PictureStylePC` --
///   `omitted.print_conv` (`[\%pictureStyles x3]` with PrintHex).
pub(super) const CANON_MAIN_RESIDUAL_IDS: &[u16] = &[
    0x0008, 0x000c, 0x001e, 0x0023, 0x0028, 0x0038, 0x0083, 0x0096, 0x00d0, 0x4008, 0x4009,
];

/// `Canon::Main` ids the generated table withholds and nothing in this build
/// produces -- an honest absence, each with its class. Sorted.
///
/// * 0x0094 `AFPointsInFocus1D` -- `omitted.print_conv`
///   (`Canon::PrintAFPoints1D`);
/// * 0x00a1-0x00a4 (Tone/Sharpness/SharpnessFreq/WhiteBalance tables),
///   0x00b2 `ToneCurveMatching`, 0x00b3 `WhiteBalanceMatching` --
///   `omitted.value_conv` (`%longBin`: `length($val) > 64 ? \$val : $val`);
/// * edges with no hand decoder: 0x0005 `CanonPanorama`, 0x000a
///   `UnknownD30`, 0x0011 `MovieInfo`, 0x0025 `FaceDetect2`, 0x00a9
///   `ColorBalance`, 0x4026 `LogInfo`, 0x403f `RawBurstModeRoll`.
pub(super) const CANON_MAIN_UNSUPPLIED: &[u16] = &[
    0x0005, 0x000a, 0x0011, 0x0025, 0x0094, 0x00a1, 0x00a2, 0x00a3, 0x00a4, 0x00a9, 0x00b2, 0x00b3,
    0x4026, 0x403f,
];

/// One engine row, rendered to the string the hand map stores.
#[derive(Debug)]
struct Row {
    name: &'static str,
    text: String,
    /// `Emitted::value_conv` rendered the same way: the `-n` form when a
    /// `PrintConv` rendered `text`, else `None` (`text` is already it).
    no_print_conv: Option<String>,
    low_priority: bool,
    consumed: bool,
}

/// What one engine walk of `Canon::Main` reported, held until the hand walk
/// reaches each row's entry. See the module doc.
#[derive(Debug)]
pub(super) struct MainEngineRows {
    table: &'static IfdTable,
    /// In emission order, which is IFD entry order.
    rows: Vec<Row>,
    /// How many entries the engine's `read_ifd` accepted, `None` when it
    /// refused the directory (then no row exists).
    entries: Option<usize>,
}

impl MainEngineRows {
    fn empty(table: &'static IfdTable) -> Self {
        Self {
            table,
            rows: Vec::new(),
            entries: None,
        }
    }

    /// Whether the engine, not a hand arm, is the producer for entry `id`:
    /// the generated table reports it (a plain tag, or a `_variants` group
    /// with a reported alternative) and it is not a residual id.
    ///
    /// Derived from the static, never from a hand list, so a regeneration
    /// that starts reporting a new Main row moves it to the engine and one
    /// that stops reporting one fails the residual pin.
    pub(super) fn owns(&self, id: u16) -> bool {
        engine_reports(self.table, id) && CANON_MAIN_RESIDUAL_IDS.binary_search(&id).is_err()
    }

    /// The engine's row for the entry `id` the hand walk has just reached:
    /// the next unconsumed row of a name `id` declares goes into `tags` now,
    /// at the entry's own position. The n-th visit of an id pairs with the
    /// n-th row of its name (names are unique per id across the table --
    /// `every_reported_name_belongs_to_one_id`). A row the engine refused
    /// never exists, so the entry then produces nothing: that is the
    /// engine's absence, which is ExifTool's.
    pub(super) fn replay(
        &mut self,
        id: u16,
        tags: &mut HashMap<String, String>,
        forms: &mut Option<&mut HashMap<String, String>>,
    ) {
        let table = self.table;
        if let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| !row.consumed && declares(table, id, row.name))
        {
            row.consumed = true;
            insert_row(row, tags, forms);
        }
    }

    /// Rows whose entry the hand walk never reached -- `parse_ifd_entries`
    /// stops at the first truncated entry and refuses more than 200, while
    /// `read_ifd` accepts up to 512 -- in emission order. Later in IFD order
    /// than anything the hand walk visited, so last is their place.
    pub(super) fn drain(
        self,
        tags: &mut HashMap<String, String>,
        forms: &mut Option<&mut HashMap<String, String>>,
    ) {
        for row in self.rows.iter().filter(|row| !row.consumed) {
            insert_row(row, tags, forms);
        }
    }

    /// The entry count the engine's `read_ifd` accepted (`None` = refused).
    pub(super) fn entries(&self) -> Option<usize> {
        self.entries
    }
}

/// `Canon:<Name>` into the map the way `FoundTag` records it
/// (`shared::tag_priority`), with the `-n` form attached to the same
/// occurrence through the Canon value-form channel (`record_canon_value` ->
/// `tiff_helpers::parse_makernote` / `raw::metadata::attach_canon_value`).
///
/// A row with no `value_conv` removes any form already under its key: a form
/// belongs to the occurrence it was recorded with, and the display this row
/// replaces is not that occurrence any more. No `Canon::Main` row is low
/// priority (`no_canon_main_row_is_low_priority`); the branch is kept so the
/// insertion rule is `FoundTag`'s and not an assumption about this table.
fn insert_row(
    row: &Row,
    tags: &mut HashMap<String, String>,
    forms: &mut Option<&mut HashMap<String, String>>,
) {
    let key = format!("Canon:{}", row.name);
    if row.low_priority {
        if tags.contains_key(&key) {
            return;
        }
        insert_low_priority(tags, key.clone(), row.text.clone());
    } else {
        tags.insert(key.clone(), row.text.clone());
    }
    if let Some(forms) = forms.as_deref_mut() {
        match &row.no_print_conv {
            Some(value) => {
                forms.insert(key, value.clone());
            }
            None => {
                forms.remove(&key);
            }
        }
    }
}

/// One `ProcessExif` walk of the generated `Canon::Main`, over exactly the
/// bytes `parse_ifd_entries` walks, buffered for replay.
///
/// * The slice is the post-signature one when the optional `Canon`
///   signature is present (the hand walk's `config`), else the whole window.
/// * `base` is the hand path's `canon_makernote_base` for this entry point --
///   the value the hand extractors subtract from every value offset,
///   relative to that slice. It is NOT recomputed here: the footer,
///   TIFF-offset and vote logic is the hand path's, and every out-of-line
///   Main value must be read at the same place as the residual's.
/// * `order` is the RESOLVED note order (`MakerNoteCanon` is `ByteOrder =>
///   'Unknown'`, MakerNotes.pm:67), not the enclosing TIFF's.
/// * `model` is `$$self{Model}`: the hand path's `self_model`, the one string
///   every hand Condition in the walk reads, so the engine's 0x000c/0x0096
///   resolution and the residual arms' predicates can never read different
///   models. Absent (not `""`) when empty.
/// * `MakerNoteCanon` has no `Start` (MakerNotes.pm:61-69): the IFD is byte 0
///   of the slice. `Canon::Main` has no `SET_GROUP1`, so every row reports
///   the table's group 1, `Canon`.
pub(super) fn walk(
    table: &'static IfdTable,
    data: &[u8],
    order: ByteOrder,
    config: &IfdParserConfig,
    base: u32,
    model: &str,
) -> MainEngineRows {
    let start = match config.signature {
        Some(sig) if data.starts_with(sig) => config.signature_offset,
        _ => 0,
    };
    let mut rows = MainEngineRows::empty(table);
    let Some(ifd_data) = data.get(start..) else {
        return rows;
    };
    rows.entries = read_ifd(ifd_data, 0, order.to_io_byte_order()).map(|entries| entries.len());
    let mut members: HashMap<&'static str, MemberValue> = HashMap::new();
    if !model.is_empty() {
        members.insert("Model", MemberValue::Str(model.to_string()));
    }
    let mut ctx = Ctx::new(&mut members);
    let mut emitted = Vec::new();
    process_exif(
        table,
        IfdDir {
            data: ifd_data,
            ifd_start: 0,
            base: Some(-i64::from(base)),
            byte_order: order.to_io_byte_order(),
            group1: Some("Canon"),
        },
        &mut ctx,
        &mut emitted,
    );
    for row in emitted {
        // FENCE: `Canon::Main`'s own rows only. See the module doc.
        if !is_canon_main_row(&row) {
            continue;
        }
        let Some(text) = engine_value_text(&row.value) else {
            continue;
        };
        rows.rows.push(Row {
            name: row.name,
            text,
            no_print_conv: row.value_conv.as_ref().and_then(engine_value_text),
            low_priority: row.low_priority,
            consumed: false,
        });
    }
    rows
}

/// The fence: a row of `Canon::Main` itself, reported under family 1
/// `Canon`. Every other row arrived through a `SubDirectory` edge whose
/// target a hand decoder in `canon.rs` owns.
pub(super) fn is_canon_main_row(row: &Emitted) -> bool {
    row.module == "Canon" && row.table == "Main" && row.group1 == "Canon"
}

/// Whether `id` declares `name`: the plain tag's name, or any alternative's.
fn declares(table: &IfdTable, id: u16, name: &str) -> bool {
    table.tag(id).is_some_and(|tag| tag.name == name)
        || table
            .variant_group(id)
            .is_some_and(|group| group.alternatives.iter().any(|(_, tag)| tag.name == name))
}

/// Whether the generated table reports `id` at all: a plain tag the walk
/// reports, or a `_variants` group with an alternative it reports.
fn engine_reports(table: &IfdTable, id: u16) -> bool {
    table.tag(id).is_some_and(tag_is_reported)
        || table.variant_group(id).is_some_and(|group| {
            group
                .alternatives
                .iter()
                .any(|(_, tag)| alternative_is_reported(tag))
        })
}

/// The walk's own filters for a plain tag (`ifd_engine::walk`): not
/// `Unknown`, nothing omitted, not a `SubDirectory` edge.
fn tag_is_reported(tag: &IfdTag) -> bool {
    !tag.omitted.any() && tag.subdir.is_none() && !tag.flags.unknown
}

/// The same for a `_variants` alternative, whose `Condition` the walk
/// resolves itself (`condition_resolved` clears `omitted.condition`).
fn alternative_is_reported(tag: &IfdTag) -> bool {
    let mut omitted = tag.omitted;
    omitted.condition = false;
    !omitted.any() && tag.subdir.is_none() && !tag.flags.unknown
}

/// `Condition => '$$self{Model} =~ /EOS D30\b/'` (Canon.pm:1286), 0x000c's
/// first alternative -- the one the engine reports. The residual 0x000c arm
/// defers to the engine exactly when this holds, and renders alternatives 2
/// and 3 otherwise; `canon_main_residual_emits_exactly_the_withheld_
/// alternatives` pins it as the complement of `first_match_ifd`. Perl's
/// `\w` on a byte string is `[A-Za-z0-9_]`.
pub(super) fn serial_number_is_d30(model: &str) -> bool {
    model.match_indices("EOS D30").any(|(at, needle)| {
        model[at + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
    })
}

/// `Condition => '$$self{Model} =~ /EOS 5D/'` (Canon.pm:1841), 0x0096's
/// first alternative, is the `%Canon::SerialInfo` edge; the second
/// (Always) is the Main `InternalSerialNumber` value the residual arm
/// renders. True when the value alternative wins -- `/EOS 5D/` also matches
/// the 5D Mark II/III/IV and 5DS/5DS R, all of which ExifTool routes to
/// SerialInfo. Pinned as the complement of `first_match_ifd` by
/// `canon_main_residual_emits_exactly_the_withheld_alternatives`.
pub(super) fn internal_serial_is_main_value(model: &str) -> bool {
    !model.contains("EOS 5D")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::TagValue;
    use crate::exiftool_tables::{find_ifd_table, find_table, first_match_ifd};

    fn canon_main() -> &'static IfdTable {
        find_ifd_table("Canon", "Main").expect("Canon::Main is generated")
    }

    /// Every `Canon::Main` id the hand dispatch in `canon.rs` decodes as a
    /// sub-table, beside `binary_tables::handles_tag` (asserted through the
    /// function itself, not a copy): the inline decoders and the module
    /// dispatch arms of `parse_canon_makernote_directory`.
    const CANON_MAIN_SUBTABLE_IDS: &[u16] = &[
        0x0001, // CameraSettings
        0x0002, // FocalLength
        0x0004, // ShotInfo
        0x000d, // CameraInfo (camera_info.rs)
        0x000f, // CustomFunctions
        0x0012, // AFInfo
        0x0026, // AFInfo2
        0x003c, // AFInfo3
        0x0090, // CustomFunctions1D
        0x0091, // PersonalFunctions
        0x0092, // PersonalFunctionValues
        0x0093, // FileInfo
        0x0099, // CustomFunctions2 (custom_functions2.rs)
        0x00a0, // Processing
        0x00aa, // MeasuredColor
        0x00e0, // SensorInfo
        0x4001, // ColorData (color_data.rs)
        0x4019, // LensInfo
        0x4024, // FilterInfo (filter_info.rs)
        0x4059, // LevelInfo
    ];

    fn hand_subtable(id: u16) -> bool {
        CANON_MAIN_SUBTABLE_IDS.contains(&id) || super::super::binary_tables::handles_tag(id)
    }

    fn is_edge(table: &IfdTable, id: u16) -> bool {
        table.tag(id).is_some_and(|tag| tag.subdir.is_some())
            || table
                .variant_group(id)
                .is_some_and(|group| group.alternatives.iter().any(|(_, t)| t.subdir.is_some()))
    }

    fn is_unknown(table: &IfdTable, id: u16) -> bool {
        table.tag(id).is_some_and(|tag| tag.flags.unknown)
    }

    #[test]
    fn lists_are_sorted_and_unique() {
        for (name, list) in [
            ("CANON_MAIN_RESIDUAL_IDS", CANON_MAIN_RESIDUAL_IDS),
            ("CANON_MAIN_UNSUPPLIED", CANON_MAIN_UNSUPPLIED),
            ("CANON_MAIN_SUBTABLE_IDS", CANON_MAIN_SUBTABLE_IDS),
        ] {
            assert!(
                list.windows(2).all(|w| w[0] < w[1]),
                "{name} must be sorted and free of duplicates"
            );
        }
    }

    /// The residual against the GENERATED table alone -- the port of
    /// `olympus/tables.rs::assert_residual_matches_the_generated_withholding`
    /// to a vendor whose Main "table" is a `match`, so there is no hand table
    /// to walk and every row of the classification comes from the compiled
    /// `IFD_CANON_MAIN`:
    ///
    /// 1. no double producer: the only residual id the engine reports is
    ///    0x000c, and there its reported alternatives are exactly `{0}` and
    ///    its withheld ones exactly `{1, 2}` (the arm's split);
    /// 2. no silent loss: every withheld id is a residual id, a hand
    ///    sub-table id, or named in `CANON_MAIN_UNSUPPLIED` -- a
    ///    regeneration that starts withholding a row this build reports
    ///    fails here instead of dropping the tag;
    /// 3. no stale entries: an `UNSUPPLIED` id must be withheld and have no
    ///    producer; a hand sub-table id must be an edge (were it
    ///    engine-reported, the hand decoder would double a value);
    /// 4. a residual id absent from the static (0x0083, 0x00d0) is
    ///    legitimate -- the generator never transcribed it -- and is asserted
    ///    absent, so a regeneration that transcribes it turns this red.
    #[test]
    fn canon_main_residual_matches_the_generated_withholding() {
        let table = canon_main();
        let mut ids: Vec<u16> = table.tags.iter().map(|t| t.id).collect();
        ids.extend(table.variants.iter().map(|g| g.id));
        ids.sort_unstable();
        ids.dedup();

        let reported: Vec<u16> = ids
            .iter()
            .copied()
            .filter(|&id| engine_reports(table, id))
            .collect();
        let withheld: Vec<u16> = ids
            .iter()
            .copied()
            .filter(|&id| !engine_reports(table, id) && !is_unknown(table, id))
            .collect();

        // Rule 1.
        let doubled: Vec<u16> = CANON_MAIN_RESIDUAL_IDS
            .iter()
            .copied()
            .filter(|id| reported.contains(id))
            .collect();
        assert_eq!(
            doubled,
            [0x000c],
            "a residual id the engine reports would be produced twice"
        );
        let serial = table
            .variant_group(0x000c)
            .expect("0x000c SerialNumber is a _variants group");
        let split: (Vec<usize>, Vec<usize>) = (0..serial.alternatives.len())
            .partition(|&i| alternative_is_reported(&serial.alternatives[i].1));
        assert_eq!(split, (vec![0], vec![1, 2]), "0x000c alternative split");

        // Rule 2.
        for id in &withheld {
            assert!(
                CANON_MAIN_RESIDUAL_IDS.contains(id)
                    || hand_subtable(*id)
                    || CANON_MAIN_UNSUPPLIED.contains(id),
                "{id:#06x} is withheld by the generated Canon::Main and nothing supplies it: \
                 add a residual arm, or name it in CANON_MAIN_UNSUPPLIED with the reason"
            );
        }

        // Rule 3.
        for id in CANON_MAIN_UNSUPPLIED {
            assert!(
                withheld.contains(id),
                "{id:#06x} is in CANON_MAIN_UNSUPPLIED but the generated table does not withhold it (stale entry)"
            );
            assert!(
                !CANON_MAIN_RESIDUAL_IDS.contains(id) && !hand_subtable(*id),
                "{id:#06x} is in CANON_MAIN_UNSUPPLIED and also has a producer"
            );
        }
        for id in (0..=u16::MAX).filter(|&id| hand_subtable(id)) {
            assert!(
                is_edge(table, id),
                "{id:#06x} is dispatched as a Canon sub-table by hand but is not a Canon::Main edge"
            );
            assert!(
                !reported.contains(&id),
                "{id:#06x} is a hand sub-table the engine also reports"
            );
        }

        // Rule 4.
        for id in CANON_MAIN_RESIDUAL_IDS {
            if !ids.contains(id) {
                assert!(
                    [0x0083, 0x00d0].contains(id),
                    "{id:#06x} is a residual id the generated table does not declare"
                );
            }
        }
        for id in [0x0083u16, 0x00d0] {
            assert!(
                !ids.contains(&id),
                "{id:#06x} is now transcribed: re-classify it (it may belong to the engine)"
            );
        }

        // The engine-owned set is exactly the spec's seventeen rows: sixteen
        // plain tags plus 0x000c's D30 alternative.
        let rows = MainEngineRows::empty(table);
        let owned: Vec<u16> = ids.iter().copied().filter(|&id| rows.owns(id)).collect();
        assert_eq!(
            owned,
            [
                0x0006, 0x0007, 0x0009, 0x000e, 0x0010, 0x0013, 0x0015, 0x001a, 0x001c, 0x0081,
                0x0082, 0x0095, 0x0097, 0x00ae, 0x00b4, 0x4010
            ],
            "the ids the engine produces while the Canon::Main line is in force"
        );
    }

    /// `replay` pairs an entry with a row by name, which is sound only if no
    /// reported name is declared by two ids.
    #[test]
    fn every_reported_name_belongs_to_one_id() {
        let table = canon_main();
        let mut names: Vec<(&str, u16)> = table
            .tags
            .iter()
            .filter(|t| tag_is_reported(t))
            .map(|t| (t.name, t.id))
            .collect();
        for group in table.variants {
            for (_, tag) in group.alternatives {
                if alternative_is_reported(tag) {
                    names.push((tag.name, group.id));
                }
            }
        }
        for (name, id) in &names {
            let declared_by: Vec<u16> = table
                .tags
                .iter()
                .map(|t| t.id)
                .chain(table.variants.iter().map(|g| g.id))
                .filter(|&other| declares(table, other, name))
                .collect();
            assert_eq!(
                declared_by,
                [*id],
                "Canon::Main name {name} is declared by more than one id"
            );
        }
    }

    /// The 0x000c residual arm renders exactly the alternatives the engine
    /// withholds: the hand predicate against `cond::first_match_ifd` over the
    /// generated group, for a body of every class the conditions separate.
    /// `""` is the Model-less entry point. The FocusInfo precedent is
    /// `olympus.rs::focus_info_hand_pass_emits_exactly_the_withheld_alternatives`.
    #[test]
    fn canon_main_residual_emits_exactly_the_withheld_alternatives() {
        let table = canon_main();
        let models = [
            "Canon EOS D30",
            "IMG:EOS D30 JPEG",
            "Canon EOS D300",
            "Canon EOS-1D",
            "Canon EOS-1DS",
            "Canon EOS-1D Mark III",
            "Canon EOS-1D X",
            "Canon EOS 5D",
            "Canon EOS 5D Mark II",
            "Canon EOS 5D Mark III",
            "Canon EOS 5D Mark IV",
            "Canon EOS 5DS",
            "Canon EOS 5DS R",
            "Canon EOS 50D",
            "Canon EOS R5",
            "Canon PowerShot A570 IS",
            "",
        ];
        let group = table
            .variant_group(0x000c)
            .expect("0x000c is a _variants group");
        for model in models {
            let mut members = HashMap::new();
            if !model.is_empty() {
                members.insert("Model", MemberValue::Str(model.to_string()));
            }
            let mut ctx = Ctx::new(&mut members).with_count(1);
            let winner = first_match_ifd(group.alternatives, &mut ctx)
                .expect("a Cond::Always alternative ends 0x000c");
            let winner_index = group
                .alternatives
                .iter()
                .position(|(_, tag)| std::ptr::eq(tag, winner))
                .expect("the winner is one of the group's alternatives");
            assert_eq!(
                serial_number_is_d30(model),
                winner_index == 0,
                "0x000c for Model {model:?}: first_match_ifd picks alternative {winner_index}"
            );
            assert_eq!(
                alternative_is_reported(winner),
                winner_index == 0,
                "0x000c for Model {model:?}: only the D30 alternative is engine-reported"
            );
        }

        // 0x0096: alternative 0 is the /EOS 5D/ SerialInfo edge, alternative
        // 1 the withheld value the residual arm renders. Neither is
        // engine-reported, so the residual must render exactly when the
        // value alternative wins -- and stay silent when the edge does.
        let group = table
            .variant_group(0x0096)
            .expect("0x0096 is a _variants group");
        assert!(
            group.alternatives[0].1.subdir.is_some(),
            "0x0096 alternative 0 is the SerialInfo edge"
        );
        assert!(
            group.alternatives[1].1.subdir.is_none()
                && !alternative_is_reported(&group.alternatives[1].1),
            "0x0096 alternative 1 is the withheld InternalSerialNumber value"
        );
        for model in models {
            let mut members = HashMap::new();
            if !model.is_empty() {
                members.insert("Model", MemberValue::Str(model.to_string()));
            }
            let mut ctx = Ctx::new(&mut members).with_count(9);
            let winner = first_match_ifd(group.alternatives, &mut ctx)
                .expect("a Cond::Always alternative ends 0x0096");
            assert_eq!(
                internal_serial_is_main_value(model),
                winner.subdir.is_none(),
                "0x0096 for Model {model:?}: first_match_ifd picks {}",
                winner.name
            );
        }
    }

    fn emitted(module: &'static str, table: &'static str, group1: &'static str) -> Emitted {
        Emitted {
            module,
            table,
            group0: "MakerNotes",
            group1,
            group2: "Camera",
            name: "FocalLength",
            value: TagValue::Integer(1),
            value_conv: None,
            low_priority: false,
            avoid: false,
        }
    }

    #[test]
    fn the_fence_admits_canon_main_rows_only() {
        assert!(is_canon_main_row(&emitted("Canon", "Main", "Canon")));
        assert!(!is_canon_main_row(&emitted(
            "Canon",
            "FocalLength",
            "Canon"
        )));
        assert!(!is_canon_main_row(&emitted("Canon", "SerialInfo", "Canon")));
        assert!(!is_canon_main_row(&emitted("Canon", "Main", "MakerNotes")));
        assert!(!is_canon_main_row(&emitted("Olympus", "Main", "Canon")));
    }

    /// No edge out of `Canon::Main` may reach an enabled table: the engine
    /// would descend into it and emit rows a hand decoder in `canon.rs`
    /// already produces. The fence drops them, so this is not about
    /// correctness today -- it makes enabling a target a conscious change.
    #[test]
    fn canon_main_edges_reach_no_enabled_table() {
        let table = canon_main();
        let edges = table
            .tags
            .iter()
            .chain(
                table
                    .variants
                    .iter()
                    .flat_map(|g| g.alternatives.iter().map(|(_, t)| t)),
            )
            .filter_map(|tag| tag.subdir.map(|edge| (tag, edge)));
        let mut checked = 0;
        for (tag, edge) in edges {
            checked += 1;
            let enabled = find_ifd_table(edge.module, edge.table).is_some_and(|t| t.enabled())
                || find_table(edge.module, edge.table).is_some_and(|t| t.enabled());
            assert!(
                !enabled,
                "Canon::Main {:#06x} {} reaches {}::{}, which is enabled: widen \
                 `is_canon_main_row` to this table AND retire its hand decoder in the same change",
                tag.id, tag.name, edge.module, edge.table
            );
        }
        assert!(checked > 100, "only {checked} Canon::Main edges found");
    }

    /// Plain `insert` is exact only if no `Canon::Main` row is low priority
    /// (ExifTool.pm:9469-9473: the tag's `Priority`, else the table's
    /// `PRIORITY`, else 0 when `Avoid`).
    #[test]
    fn no_canon_main_row_is_low_priority() {
        let table = canon_main();
        let all = table.tags.iter().chain(
            table
                .variants
                .iter()
                .flat_map(|g| g.alternatives.iter().map(|(_, t)| t)),
        );
        for tag in all {
            let priority = tag
                .flags
                .priority
                .or(table.priority)
                .or(if tag.flags.avoid { Some(0) } else { None });
            assert_ne!(
                priority,
                Some(0),
                "{:#06x} {} is low priority",
                tag.id,
                tag.name
            );
        }
    }

    // -----------------------------------------------------------------
    // Replay order, through the full parser, on synthetic notes.
    // -----------------------------------------------------------------

    /// A little-endian `Canon::Main` IFD with no signature: `(id, type,
    /// count, bytes)` per entry, values of four bytes or fewer inline, the
    /// rest after the IFD at buffer-absolute offsets. Eight NULs close it so
    /// the footer test cannot match, and it is parsed with a directory
    /// offset of 0, so the base is 0 by construction (the CIFF builder's
    /// argument, `parse_canon_ciff_records`).
    fn le_note(entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
        let header = 2 + entries.len() * 12 + 4;
        let mut buffer = vec![0u8; header];
        buffer[..2].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        let mut values = Vec::new();
        for (index, (id, ty, count, bytes)) in entries.iter().enumerate() {
            let at = 2 + index * 12;
            buffer[at..at + 2].copy_from_slice(&id.to_le_bytes());
            buffer[at + 2..at + 4].copy_from_slice(&ty.to_le_bytes());
            buffer[at + 4..at + 8].copy_from_slice(&count.to_le_bytes());
            if bytes.len() <= 4 {
                buffer[at + 8..at + 8 + bytes.len()].copy_from_slice(bytes);
            } else {
                let offset = (header + values.len()) as u32;
                buffer[at + 8..at + 12].copy_from_slice(&offset.to_le_bytes());
                values.extend_from_slice(bytes);
            }
        }
        buffer.extend_from_slice(&values);
        buffer.extend_from_slice(&[0u8; 8]);
        buffer
    }

    fn short(id: u16, value: u16) -> (u16, u16, u32, Vec<u8>) {
        (id, 3, 1, value.to_le_bytes().to_vec())
    }

    /// `%Canon::Processing` (0x00a0), `FORMAT => 'int16s'`, `FIRST_ENTRY =>
    /// 1`: word 0 is the record's byte length, key 9 is ColorTemperature.
    fn processing(color_temperature: i16) -> (u16, u16, u32, Vec<u8>) {
        let mut words = [0i16; 16];
        words[0] = 32;
        words[9] = color_temperature;
        let bytes = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        (0x00a0, 3, 16, bytes)
    }

    fn parse(note: &[u8]) -> (HashMap<String, String>, HashMap<String, String>) {
        let mut forms = HashMap::new();
        let tags = super::super::parse_canon_makernote_impl_located_with_values(
            note,
            note,
            ByteOrder::LittleEndian,
            None,
            Some(0),
            Some(&mut forms),
        )
        .expect("the synthetic note parses");
        (tags, forms)
    }

    fn engine_is_on() -> bool {
        canon_main().enabled()
    }

    /// Test 6(a): ExifTool's IFD-order last-wins between Main 0x00ae and
    /// Processing's ColorTemperature, in both file orders. No corpus file
    /// carries both (spec section 1.4), so this is the only guard.
    #[test]
    fn color_temperature_follows_ifd_order_against_processing() {
        assert!(engine_is_on(), "the (\"Canon\", \"Main\") line is in force");
        let (tags, forms) = parse(&le_note(&[processing(6000), short(0x00ae, 5200)]));
        assert_eq!(
            tags.get("Canon:ColorTemperature").map(String::as_str),
            Some("5200"),
            "Main 0x00ae after Processing wins"
        );
        assert!(!forms.contains_key("Canon:ColorTemperature"));

        let (tags, _) = parse(&le_note(&[short(0x00ae, 5200), processing(6000)]));
        assert_eq!(
            tags.get("Canon:ColorTemperature").map(String::as_str),
            Some("6000"),
            "Processing after Main 0x00ae wins"
        );
    }

    /// Test 6(b): ColorSpace prints through the engine's PrintConv and its
    /// `-n` form (`1`) rides the Canon value-form channel, alone under the
    /// key.
    #[test]
    fn color_space_carries_its_value_conv_form() {
        let (tags, forms) = parse(&le_note(&[short(0x00b4, 1)]));
        assert_eq!(
            tags.get("Canon:ColorSpace").map(String::as_str),
            Some("sRGB")
        );
        assert_eq!(forms.get("Canon:ColorSpace").map(String::as_str), Some("1"));
        assert_eq!(
            forms.keys().filter(|k| k.contains("ColorSpace")).count(),
            1,
            "one form under the key: {forms:?}"
        );
    }

    /// Test 6(c): a directory that declares four entries where the buffer
    /// holds three. The hand walk (`parse_ifd_entries`) visits the three
    /// complete entries and stops; the engine's `read_ifd` refuses the whole
    /// directory (`ifd_engine.rs`: the entry array must fit), so no engine
    /// row exists and the two engine-owned entries the hand walk did reach produce
    /// nothing -- the engine's absence, never a hand value under an
    /// engine-owned name. ExifTool would re-read the directory from the file
    /// or, failing that, read what it can (Exif.pm:6359-6390); the engine's
    /// port withholds instead (`read_ifd`'s doc), which is the safe direction.
    /// The residual entry the hand walk reached is still produced.
    #[test]
    fn a_truncated_directory_produces_no_engine_row() {
        let mut note = le_note(&[
            short(0x001a, 1),
            (0x001e, 4, 1, 0x0101_0000u32.to_le_bytes().to_vec()),
            short(0x001c, 1),
        ]);
        // Claim a fourth entry the buffer cannot hold.
        note[..2].copy_from_slice(&4u16.to_le_bytes());
        note.truncate(2 + 3 * 12);
        let (tags, _) = parse(&note);
        assert!(!tags.contains_key("Canon:SuperMacro"), "{tags:?}");
        assert!(!tags.contains_key("Canon:DateStampMode"), "{tags:?}");
        assert!(tags.contains_key("Canon:FirmwareRevision"), "{tags:?}");
    }

    /// A row whose entry the hand walk never reaches -- more than the hand
    /// walk's 200-entry bound, within the engine's 512 -- is inserted once,
    /// by `drain`.
    #[test]
    fn rows_past_the_hand_walk_are_drained_once() {
        let mut entries: Vec<_> = (0..200).map(|_| short(0x0003, 0)).collect();
        entries.push(short(0x00ae, 4800));
        let (tags, _) = parse(&le_note(&entries));
        assert_eq!(
            tags.get("Canon:ColorTemperature").map(String::as_str),
            Some("4800")
        );
    }

    /// Test 10: CIFF never runs the engine. `parse_canon_ciff_records` builds
    /// a synthetic `Canon::Main`-shaped IFD whose ids are edges (here 0x0002
    /// FocalLength, which carries no `Validate`), and passes `walk_main:
    /// false`: the keys are the `%Canon::FocalLength` decoder's alone, and
    /// no value form comes from an engine row.
    #[test]
    fn ciff_records_yield_only_the_hand_sub_table_keys() {
        // CIFF 0x1029 -> MakerNote 0x0002: FocalType 1 (fixed), FocalLength 50.
        let words: [i16; 4] = [1, 50, 0, 0];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        let mut forms = HashMap::new();
        let tags = super::super::parse_canon_ciff_records(
            &[(0x1029, bytes.as_slice())],
            Some("Canon EOS D30"),
            &mut forms,
        );
        let mut keys: Vec<&str> = tags.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["Canon:FocalLength", "Canon:FocalType"], "{tags:?}");
        let table = canon_main();
        for key in forms.keys() {
            let name = key.trim_start_matches("Canon:");
            assert!(
                !table
                    .tags
                    .iter()
                    .any(|t| t.name == name && tag_is_reported(t)),
                "{key}: a Canon::Main engine row's form on the CIFF path"
            );
        }
    }

    #[test]
    fn serial_number_is_d30_matches_perls_trailing_word_boundary() {
        assert!(serial_number_is_d30("Canon EOS D30"));
        assert!(serial_number_is_d30("IMG:EOS D30 JPEG"));
        assert!(serial_number_is_d30("Canon EOS D300 and EOS D30"));
        assert!(!serial_number_is_d30("Canon EOS D300"));
        assert!(!serial_number_is_d30("Canon EOS D30_"));
        assert!(!serial_number_is_d30(""));
    }
}
