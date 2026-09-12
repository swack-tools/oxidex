//! `FujiFilm::Main` through the generated `IFD_FUJIFILM_MAIN` table and the
//! IFD engine (slice I-6, landing 1).
//!
//! # What this module owns
//!
//! Every row of `%Image::ExifTool::FujiFilm::Main` (FujiFilm.pm:84-1022,
//! pinned 13.59) the generated table reports -- every `Omitted::NONE`,
//! non-`SubDirectory`, non-`Unknown` tag, both alternatives of the 0x1100
//! `AutoBracketing` group -- except 0x0000 `Version`, whose engine value is
//! an `undef[4]` run (`TagValue::Binary`) the string map cannot carry
//! (`version_is_binary_and_the_residual_owns_it`). That is 88 ids; which
//! ones is never written down by hand outside the pin: [`is_residual`] and
//! the generated withholding decide it, and
//! `fuji_main_residual_matches_the_generated_withholding` pins the result.
//!
//! # Why the rows go in last, with no replay
//!
//! `FujifilmParser`'s hand loop inserts into one `HashMap<String, String>`
//! in IFD entry order (last insert under a key wins -- ExifTool's rule for
//! two tags of equal priority). With the engine on, that loop runs only the
//! residual arms and the four hand sub-table decoders, and the engine rows
//! are inserted after it, in emission order (= IFD entry order). That is
//! exact whenever no residual or sub-table row shares a key with an engine
//! row that precedes it in the IFD:
//!
//! * no `FujiFilm::Main` name is a field name of the hand sub-tables
//!   (`fuji_main_names_are_disjoint_from_the_hand_sub_tables`);
//! * the one name a residual id shares with an engine-owned id is
//!   `NoiseReduction`, 0x100b against 0x100e
//!   (`the_only_name_a_residual_and_a_reported_id_share_is_noise_reduction`).
//!   Every corpus note carrying both writes 0x100b first (sorted IFD), and
//!   there 0x100b = 0x100, which ExifTool's `RawConv` drops anyway
//!   (FujiFilm.pm:237). "Engine last" is ExifTool's IFD-order winner on
//!   every such note; its one wrong case needs 0x100e written BEFORE a
//!   non-0x100 0x100b, which no corpus note does.
//!
//! Canon::Main's buffered replay (`canon/main_engine.rs`) pairs a row with
//! its entry by name, which `FujiFilm::Main` cannot do: `Contrast` is
//! declared at both 0x1004 and 0x1006.
//!
//! Neither order reproduces ExifTool's `-j -G1` winner for two FujiFilm
//! `Contrast` copies when an `ExifIFD:Contrast` exists: `FoundTag` makes
//! ExifIFD 0xa408 (found after the maker note) the primary key, and the JSON
//! writer then prints the EARLIEST FujiFilm copy, 0x1004 (exiftool:2745-2746,
//! :2948-2953). No corpus note carries both 0x1004 and 0x1006.
//!
//! # The fence
//!
//! The walk is top level only. The five `FujiFilm::Main` edges
//! (PrioritySettings, FocusSettings, AFCSettings, DriveSettings, FaceRecInfo)
//! stay with the hand `fujifilm_binary_subdir` decoders or have no layout;
//! [`is_fuji_main_row`] drops any row that is not `FujiFilm::Main`'s own, and
//! `fuji_main_edges_reach_no_enabled_table` turns the day an edge target
//! gets enabled into a red test instead of a silent double producer.

use std::collections::HashMap;

use crate::exiftool_tables::{Ctx, Emitted, IfdDir, IfdTable, IfdTag, MemberValue, process_exif};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::parsers::tiff::makernotes::shared::engine_value::engine_value_text;
use crate::parsers::tiff::makernotes::shared::tag_priority::insert_low_priority;

/// The `FujiFilm::Main` ids whose hand arm in `FujifilmParser::parse_note`
/// stays the producer while the engine runs. Sorted (binary-searched by
/// [`is_residual`]). Each is withheld by the generated table, except 0x0000:
///
/// * 0x0000 `Version` -- reported, but `undef[4]` with no conversion, so the
///   engine emits `TagValue::Binary`, which `engine_value_text` drops;
/// * 0x0010 `InternalSerialNumber` -- `omitted.print_conv` (multi-statement
///   `q{}`, FujiFilm.pm:93-120);
/// * 0x100b `NoiseReduction` -- `omitted.raw_conv`
///   (`$val == 0x100 ? undef : $val`, FujiFilm.pm:237; the arm applies it);
/// * 0x1040 `ShadowTone`, 0x1041 `HighlightTone` -- `omitted.print_conv`
///   (hash with an `OTHER` sub, FujiFilm.pm:439-480);
/// * 0x1422 `ImageStabilization` -- `omitted.print_conv` (list PrintConv,
///   FujiFilm.pm:790-806);
/// * 0x4201 `FaceElementTypes` -- `omitted.print_conv` (`[{..}, 'REPEAT']`,
///   FujiFilm.pm:953-982).
pub(super) const FUJI_MAIN_RESIDUAL_IDS: &[u16] =
    &[0x0000, 0x0010, 0x100b, 0x1040, 0x1041, 0x1422, 0x4201];

/// `FujiFilm::Main` ids the generated table withholds and nothing in this
/// build produces -- an honest absence, each with its class. Sorted.
///
/// * 0x1304 `GEImageSize` -- `omitted.condition`
///   (`$$self{Make} =~ /^GENERAL IMAGING/`, FujiFilm.pm:709-714; ExifTool is
///   silent on every FUJIFILM note that carries it);
/// * 0x1446 `FlickerReduction` -- `omitted.print_conv` (`q{}` +
///   `sprintf('%s (0x%.4x)')`, FujiFilm.pm:861-870);
/// * 0x4282 `FaceRecInfo` -- an edge (`ProcessFaceRec`) with no transcribed
///   layout (FujiFilm.pm:999).
pub(super) const FUJI_MAIN_UNSUPPLIED: &[u16] = &[0x1304, 0x1446, 0x4282];

/// Whether `id`'s hand arm runs while the engine is on.
pub(super) fn is_residual(id: u16) -> bool {
    FUJI_MAIN_RESIDUAL_IDS.binary_search(&id).is_ok()
}

/// `FujiFilm:<Name>` into the map the way `FoundTag` records it
/// (`shared::tag_priority`), with the `-n` form attached to the same
/// occurrence: a `Priority => 0` row (0x1431 `Rating`, FujiFilm.pm:821-826)
/// never displaces a value already under its key and leaves its form alone;
/// any other row replaces the display, then records its `-n` form -- or
/// removes the form under the key when it has none, because a form belongs to
/// the occurrence it was recorded with.
fn insert_row(
    name: &str,
    text: String,
    no_print_conv: Option<String>,
    low_priority: bool,
    tags: &mut HashMap<String, String>,
    forms: &mut Option<&mut HashMap<String, String>>,
) {
    let key = format!("FujiFilm:{name}");
    if low_priority {
        if tags.contains_key(&key) {
            return;
        }
        insert_low_priority(tags, key.clone(), text);
    } else {
        tags.insert(key.clone(), text);
    }
    if let Some(forms) = forms.as_deref_mut() {
        match no_print_conv {
            Some(value) => {
                forms.insert(key, value);
            }
            None => {
                forms.remove(&key);
            }
        }
    }
}

/// One `ProcessExif` walk of the generated `FujiFilm::Main`, inserted into
/// `tags` in emission order AFTER the hand loop's residual and sub-table rows
/// (see the module doc).
///
/// * `data` is the declared payload, starting at `FUJIFILM` -- the same
///   bytes the residual arms and the sub-table decoders read.
/// * `ifd_start` is the little-endian `u32` at payload byte 8
///   (MakerNotes.pm:128 `OffsetPt => '$valuePtr+8'`).
/// * `base: Some(0)`: value offsets are relative to the `FUJIFILM` byte
///   (MakerNotes.pm:131 `Base => '$start'`; on RAF `ProcessRAF` raises
///   `$$et{BASE}` by the JPEG offset, so it stays note-relative too).
/// * The byte order is little-endian, always (MakerNotes.pm:132
///   `ByteOrder => 'LittleEndian'`), never the enclosing file's.
/// * `model` is `$$self{Model}` (EXIF `IFD0:Model` from the dispatcher;
///   `None` on the RAF and detached paths), absent when empty. Only the
///   0x1100 `AutoBracketing` X-T3 alternative reads it. `Make` is not
///   available to a `MakerNoteParser` and is not guessed at; its only reader,
///   0x1304, is withheld.
/// * `hand_entries` is the hand walk's entry count, for the debug check that
///   both walks see the same directory.
pub(super) fn insert_rows(
    table: &'static IfdTable,
    data: &[u8],
    ifd_start: usize,
    hand_entries: usize,
    model: Option<&str>,
    tags: &mut HashMap<String, String>,
    mut forms: Option<&mut HashMap<String, String>>,
) {
    let order = ByteOrder::LittleEndian.to_io_byte_order();
    // Both walks require the whole entry array to fit (nom `count` in the
    // hand walk, `read_ifd`'s fit rule); `read_ifd` also refuses a count of
    // 0 or above 512, and then no engine row exists at all.
    debug_assert!(
        crate::exiftool_tables::read_ifd(data, ifd_start, order)
            .is_none_or(|entries| entries.len() == hand_entries),
        "the engine and the hand walk disagree on the FujiFilm::Main entry count"
    );
    let mut members: HashMap<&'static str, MemberValue> = HashMap::new();
    if let Some(model) = model.filter(|m| !m.is_empty()) {
        members.insert("Model", MemberValue::Str(model.to_string()));
    }
    let mut ctx = Ctx::new(&mut members);
    let mut emitted = Vec::new();
    process_exif(
        table,
        IfdDir {
            data,
            ifd_start,
            base: Some(0),
            byte_order: order,
            group1: Some("FujiFilm"),
        },
        &mut ctx,
        &mut emitted,
    );
    for row in emitted {
        // FENCE: `FujiFilm::Main`'s own rows only. See the module doc.
        if !is_fuji_main_row(&row) {
            continue;
        }
        // 0x0000 `Version` is `TagValue::Binary` and stops here; the residual
        // arm is its producer.
        let Some(text) = engine_value_text(&row.value) else {
            continue;
        };
        let no_print_conv = row.value_conv.as_ref().and_then(engine_value_text);
        insert_row(
            row.name,
            text,
            no_print_conv,
            row.low_priority,
            tags,
            &mut forms,
        );
    }
}

/// The fence: a row of `FujiFilm::Main` itself, reported under family 1
/// `FujiFilm`. Every other row arrived through a `SubDirectory` edge whose
/// target a hand decoder owns.
pub(super) fn is_fuji_main_row(row: &Emitted) -> bool {
    row.module == "FujiFilm" && row.table == "Main" && row.group1 == "FujiFilm"
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

/// Whether the engine, not a hand arm, is the producer for `id` while the
/// line is in force.
#[cfg(test)]
fn engine_owns(table: &IfdTable, id: u16) -> bool {
    engine_reports(table, id) && !is_residual(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::TagValue;
    use crate::exiftool_tables::{PrintConv, find_ifd_table, find_table, first_match_ifd};
    use crate::parsers::tiff::makernotes::fujifilm::FujifilmParser;
    use crate::parsers::tiff::makernotes::fujifilm::settings_tables::{
        FUJIFILM_AFCSETTINGS, FUJIFILM_DRIVESETTINGS, FUJIFILM_FOCUSSETTINGS,
        FUJIFILM_PRIORITYSETTINGS,
    };
    use crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
    use crate::parsers::tiff::makernotes::shared::MakerNoteParser;

    fn fuji_main() -> &'static IfdTable {
        find_ifd_table("FujiFilm", "Main").expect("FujiFilm::Main is generated")
    }

    fn engine_is_on() -> bool {
        fuji_main().enabled()
    }

    /// Every id the table declares, plain or `_variants`, sorted and unique.
    fn declared_ids(table: &IfdTable) -> Vec<u16> {
        let mut ids: Vec<u16> = table.tags.iter().map(|t| t.id).collect();
        ids.extend(table.variants.iter().map(|g| g.id));
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Every name `id` declares: the plain tag's, and each alternative's.
    fn names_of(table: &IfdTable, id: u16) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = table.tag(id).map(|t| t.name).into_iter().collect();
        if let Some(group) = table.variant_group(id) {
            names.extend(group.alternatives.iter().map(|(_, t)| t.name));
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    fn is_edge(table: &IfdTable, id: u16) -> bool {
        table.tag(id).is_some_and(|tag| tag.subdir.is_some())
            || table
                .variant_group(id)
                .is_some_and(|group| group.alternatives.iter().any(|(_, t)| t.subdir.is_some()))
    }

    fn hand_subtable(id: u16) -> bool {
        super::super::fujifilm_binary_subdir(id).is_some()
    }

    /// The ids the engine produces while the FujiFilm::Main line is in force
    /// (spec section 3.1): every reported id except 0x0000.
    const FUJI_MAIN_ENGINE_IDS: &[u16] = &[
        0x1000, 0x1001, 0x1002, 0x1003, 0x1004, 0x1005, 0x1006, 0x100a, 0x100e, 0x100f, 0x1010,
        0x1011, 0x1020, 0x1021, 0x1022, 0x1023, 0x1030, 0x1031, 0x1032, 0x1033, 0x1034, 0x1037,
        0x1044, 0x1045, 0x1047, 0x1048, 0x1049, 0x104b, 0x104c, 0x104d, 0x104e, 0x1050, 0x1051,
        0x1052, 0x1053, 0x1100, 0x1101, 0x1102, 0x1105, 0x1106, 0x1150, 0x1151, 0x1152, 0x1153,
        0x1154, 0x1201, 0x1210, 0x1300, 0x1301, 0x1302, 0x1400, 0x1401, 0x1402, 0x1403, 0x1404,
        0x1405, 0x1406, 0x1407, 0x140b, 0x1425, 0x1431, 0x1436, 0x1438, 0x1443, 0x1444, 0x1445,
        0x1447, 0x1448, 0x144a, 0x144b, 0x144c, 0x144d, 0x3803, 0x3804, 0x3806, 0x3820, 0x3821,
        0x3822, 0x3824, 0x4005, 0x4100, 0x4103, 0x4200, 0x4203, 0x8000, 0x8002, 0x8003, 0xb211,
    ];

    #[test]
    fn lists_are_sorted_and_unique() {
        for (name, list) in [
            ("FUJI_MAIN_RESIDUAL_IDS", FUJI_MAIN_RESIDUAL_IDS),
            ("FUJI_MAIN_UNSUPPLIED", FUJI_MAIN_UNSUPPLIED),
            ("FUJI_MAIN_ENGINE_IDS", FUJI_MAIN_ENGINE_IDS),
        ] {
            assert!(
                list.windows(2).all(|w| w[0] < w[1]),
                "{name} must be sorted and free of duplicates"
            );
        }
        assert_eq!(FUJI_MAIN_ENGINE_IDS.len(), 88);
    }

    /// The residual against the GENERATED table alone (the port of
    /// `canon_main_residual_matches_the_generated_withholding`); every row of
    /// the classification comes from the compiled `IFD_FUJIFILM_MAIN`:
    ///
    /// 1. one sanctioned reported residual: 0x0000, and only in the shape the
    ///    Version tripwire exercises (`undef`, no Format, no conversion);
    /// 2. no silent loss: every withheld id is a residual id, a hand
    ///    sub-table id, or named in `FUJI_MAIN_UNSUPPLIED` -- a regeneration
    ///    that starts withholding a row this build reports fails here;
    /// 3. no stale entries: an `UNSUPPLIED` id is withheld and has no
    ///    producer; every id `fujifilm_binary_subdir` selects is an edge the
    ///    engine does not report;
    /// 4. every residual id is declared (FujiFilm has no untranscribed
    ///    residual id);
    /// 5. the engine-owned set is exactly `FUJI_MAIN_ENGINE_IDS`.
    #[test]
    fn fuji_main_residual_matches_the_generated_withholding() {
        let table = fuji_main();
        let ids = declared_ids(table);
        assert!(
            !table.tags.iter().any(|t| t.flags.unknown),
            "FujiFilm::Main has no Unknown tag"
        );
        let reported: Vec<u16> = ids
            .iter()
            .copied()
            .filter(|&id| engine_reports(table, id))
            .collect();
        let withheld: Vec<u16> = ids
            .iter()
            .copied()
            .filter(|&id| !engine_reports(table, id))
            .collect();

        // Rule 1.
        let doubled: Vec<u16> = FUJI_MAIN_RESIDUAL_IDS
            .iter()
            .copied()
            .filter(|id| reported.contains(id))
            .collect();
        assert_eq!(
            doubled,
            [0x0000],
            "a residual id the engine reports would be produced twice"
        );
        assert!(table.variant_group(0x0000).is_none());
        let version = table.tag(0x0000).expect("0x0000 Version is a plain tag");
        assert_eq!(version.name, "Version");
        assert_eq!(version.writable, Some("undef"));
        assert!(version.format.is_none());
        assert!(version.value_conv.is_none());
        assert!(matches!(version.print_conv, PrintConv::None));
        assert!(!version.flags.binary);

        // Rule 2.
        for id in &withheld {
            assert!(
                is_residual(*id) || hand_subtable(*id) || FUJI_MAIN_UNSUPPLIED.contains(id),
                "{id:#06x} is withheld by the generated FujiFilm::Main and nothing supplies it: \
                 add a residual arm, or name it in FUJI_MAIN_UNSUPPLIED with the reason"
            );
        }

        // Rule 3.
        for id in FUJI_MAIN_UNSUPPLIED {
            assert!(
                withheld.contains(id),
                "{id:#06x} is in FUJI_MAIN_UNSUPPLIED but the generated table does not withhold it (stale entry)"
            );
            assert!(
                !is_residual(*id) && !hand_subtable(*id),
                "{id:#06x} is in FUJI_MAIN_UNSUPPLIED and also has a producer"
            );
        }
        let mut dispatched = Vec::new();
        for id in (0..=u16::MAX).filter(|&id| hand_subtable(id)) {
            assert!(
                is_edge(table, id),
                "{id:#06x} is dispatched as a FujiFilm sub-table by hand but is not a FujiFilm::Main edge"
            );
            assert!(
                !reported.contains(&id),
                "{id:#06x} is a hand sub-table the engine also reports"
            );
            dispatched.push(id);
        }
        assert_eq!(dispatched, [0x102b, 0x102d, 0x102e, 0x1103]);

        // Rule 4.
        for id in FUJI_MAIN_RESIDUAL_IDS {
            assert!(
                ids.contains(id),
                "{id:#06x} is a residual id the generated table does not declare"
            );
        }

        // Rule 5.
        let owned: Vec<u16> = ids
            .iter()
            .copied()
            .filter(|&id| engine_owns(table, id))
            .collect();
        assert_eq!(
            owned, FUJI_MAIN_ENGINE_IDS,
            "the ids the engine produces while the FujiFilm::Main line is in force"
        );
        assert_eq!(reported.len(), 89, "88 engine ids plus 0x0000 Version");
    }

    /// Test 4(a): the no-replay premise, half one. Engine-last insertion is
    /// ExifTool's IFD-order winner only while `NoiseReduction` is the one
    /// name a residual id shares with an engine-owned id, and `Contrast` the
    /// one reported name two ids declare. If this fails, revisit the module
    /// doc's argument (and adopt Canon's replay).
    #[test]
    fn the_only_name_a_residual_and_a_reported_id_share_is_noise_reduction() {
        let table = fuji_main();
        let ids = declared_ids(table);
        let residual_names: Vec<&str> = FUJI_MAIN_RESIDUAL_IDS
            .iter()
            .flat_map(|&id| names_of(table, id))
            .collect();
        let mut owned_names: Vec<(&str, u16)> = ids
            .iter()
            .copied()
            .filter(|&id| engine_owns(table, id))
            .flat_map(|id| names_of(table, id).into_iter().map(move |n| (n, id)))
            .collect();
        let mut shared: Vec<&str> = owned_names
            .iter()
            .filter(|(name, _)| residual_names.contains(name))
            .map(|(name, _)| *name)
            .collect();
        shared.sort_unstable();
        shared.dedup();
        assert_eq!(shared, ["NoiseReduction"]);

        owned_names.sort_unstable();
        let mut multi: Vec<&str> = owned_names
            .windows(2)
            .filter(|w| w[0].0 == w[1].0)
            .map(|w| w[0].0)
            .collect();
        multi.dedup();
        assert_eq!(
            multi,
            ["Contrast"],
            "reported names declared by more than one id"
        );
    }

    /// Test 4(b): the no-replay premise, half two. No field of the four hand
    /// sub-tables `fujifilm_binary_subdir` decodes shares a name with any
    /// `FujiFilm::Main` tag, so sub-table rows and engine rows never meet
    /// under one key whatever the order.
    #[test]
    fn fuji_main_names_are_disjoint_from_the_hand_sub_tables() {
        let table = fuji_main();
        let main_names: Vec<&str> = declared_ids(table)
            .into_iter()
            .flat_map(|id| names_of(table, id))
            .collect();
        for sub in [
            &FUJIFILM_PRIORITYSETTINGS,
            &FUJIFILM_FOCUSSETTINGS,
            &FUJIFILM_AFCSETTINGS,
            &FUJIFILM_DRIVESETTINGS,
        ] {
            assert!(!sub.fields.is_empty());
            for field in sub.fields {
                assert!(
                    !main_names.contains(&field.name),
                    "{}::{} is also a FujiFilm::Main name",
                    sub.name,
                    field.name
                );
            }
        }
    }

    fn emitted(module: &'static str, table: &'static str, group1: &'static str) -> Emitted {
        Emitted {
            module,
            table,
            group0: "MakerNotes",
            group1,
            group2: "Camera",
            name: "AF-SPriority",
            value: TagValue::Integer(1),
            value_conv: None,
            low_priority: false,
            avoid: false,
        }
    }

    #[test]
    fn the_fence_admits_fuji_main_rows_only() {
        assert!(is_fuji_main_row(&emitted("FujiFilm", "Main", "FujiFilm")));
        assert!(!is_fuji_main_row(&emitted(
            "FujiFilm",
            "PrioritySettings",
            "FujiFilm"
        )));
        assert!(!is_fuji_main_row(&emitted(
            "FujiFilm",
            "FaceRecInfo",
            "FujiFilm"
        )));
        assert!(!is_fuji_main_row(&emitted(
            "FujiFilm",
            "Main",
            "MakerNotes"
        )));
        assert!(!is_fuji_main_row(&emitted("Olympus", "Main", "FujiFilm")));
    }

    /// No edge out of `FujiFilm::Main` may reach an enabled table: the engine
    /// would descend into it and emit rows a hand decoder already produces
    /// (PrioritySettings is binary-eligible: one allowlist line away from
    /// double-producing AF-SPriority/AF-CPriority). The fence drops them, so
    /// this is not about correctness today -- it makes enabling a target a
    /// conscious change.
    #[test]
    fn fuji_main_edges_reach_no_enabled_table() {
        let table = fuji_main();
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
                "FujiFilm::Main {:#06x} {} reaches {}::{}, which is enabled: widen \
                 `is_fuji_main_row` to this table AND retire its hand decoder \
                 (`fujifilm_binary_subdir` + `settings_tables.rs`) in the same change",
                tag.id, tag.name, edge.module, edge.table
            );
        }
        assert_eq!(checked, 5, "FujiFilm::Main declares five edges");
    }

    /// Test 6: the one `Priority => 0` row (ExifTool.pm:9469-9473: the tag's
    /// `Priority`, else the table's `PRIORITY`, else 0 when `Avoid`) is
    /// 0x1431 `Rating` (FujiFilm.pm:821-826).
    #[test]
    fn only_rating_is_low_priority() {
        let table = fuji_main();
        let all = table.tags.iter().chain(
            table
                .variants
                .iter()
                .flat_map(|g| g.alternatives.iter().map(|(_, t)| t)),
        );
        let low: Vec<u16> = all
            .filter(|tag| {
                tag.flags
                    .priority
                    .or(table.priority)
                    .or(if tag.flags.avoid { Some(0) } else { None })
                    == Some(0)
            })
            .map(|tag| tag.id)
            .collect();
        assert_eq!(low, [0x1431]);

        let (tags, _) = parse(&le_note(&[long(0x1431, 3)]), None);
        assert_eq!(tags.get("FujiFilm:Rating").map(String::as_str), Some("3"));
    }

    // -----------------------------------------------------------------
    // Synthetic notes through the full parser.
    // -----------------------------------------------------------------

    /// A FujiFilm note: `FUJIFILM`, `u32le` 12, then a little-endian IFD at
    /// byte 12: `(id, type, count, bytes)` per entry, values of four bytes or
    /// fewer inline, the rest after the next-IFD link at note-relative
    /// offsets (`Base => '$start'`).
    fn le_note(entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
        let ifd = 12;
        let header = ifd + 2 + entries.len() * 12 + 4;
        let mut buffer = vec![0u8; header];
        buffer[..8].copy_from_slice(b"FUJIFILM");
        buffer[8..12].copy_from_slice(&12u32.to_le_bytes());
        buffer[ifd..ifd + 2].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        let mut values = Vec::new();
        for (index, (id, ty, count, bytes)) in entries.iter().enumerate() {
            let at = ifd + 2 + index * 12;
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
        buffer
    }

    fn short(id: u16, value: u16) -> (u16, u16, u32, Vec<u8>) {
        (id, 3, 1, value.to_le_bytes().to_vec())
    }

    fn long(id: u16, value: u32) -> (u16, u16, u32, Vec<u8>) {
        (id, 4, 1, value.to_le_bytes().to_vec())
    }

    fn version(bytes: &[u8; 4]) -> (u16, u16, u32, Vec<u8>) {
        (0x0000, 7, 4, bytes.to_vec())
    }

    /// Through `parse_with_context_and_values`, detached, with a forms map.
    fn parse(
        note: &[u8],
        model: Option<&str>,
    ) -> (HashMap<String, String>, HashMap<String, String>) {
        let mut tags = HashMap::new();
        let mut forms = HashMap::new();
        FujifilmParser
            .parse_with_context_and_values(
                &MakerNoteContext::detached(note),
                ByteOrder::LittleEndian,
                model,
                &mut tags,
                &mut forms,
            )
            .expect("the synthetic note parses");
        (tags, forms)
    }

    fn get<'m>(map: &'m HashMap<String, String>, key: &str) -> Option<&'m str> {
        map.get(key).map(String::as_str)
    }

    /// Test 3: 0x0000 is reported by the table but its engine value is an
    /// `undef[4]` run, which the string map drops; the residual arm is the
    /// only producer.
    #[test]
    fn version_is_binary_and_the_residual_owns_it() {
        let note = le_note(&[version(b"0130"), short(0x1001, 3)]);
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let mut out = Vec::new();
        process_exif(
            fuji_main(),
            IfdDir {
                data: &note,
                ifd_start: 12,
                base: Some(0),
                byte_order: ByteOrder::LittleEndian.to_io_byte_order(),
                group1: Some("FujiFilm"),
            },
            &mut ctx,
            &mut out,
        );
        let row = out
            .iter()
            .find(|row| row.name == "Version")
            .expect("the engine emits a Version row");
        assert!(
            matches!(row.value, TagValue::Binary(_)),
            "Version is {:?}",
            row.value
        );
        assert_eq!(
            engine_value_text(&row.value),
            None,
            "0x0000 now renders: move it to the engine, delete the residual arm, \
             re-measure Version on all 373 notes"
        );

        let (tags, forms) = parse(&note, None);
        assert_eq!(get(&tags, "FujiFilm:Version"), Some("0130"));
        assert!(!forms.contains_key("FujiFilm:Version"), "{forms:?}");
        assert_eq!(get(&tags, "FujiFilm:Sharpness"), Some("0 (normal)"));
        assert_eq!(get(&forms, "FujiFilm:Sharpness"), Some("3"));
    }

    /// Test 7(a): 0x100b = 0x100 is dropped by its RawConv (FujiFilm.pm:237)
    /// and 0x100e, later in the IFD, is the value.
    #[test]
    fn noise_reduction_0x100b_dropped_then_0x100e() {
        let (tags, forms) = parse(&le_note(&[short(0x100b, 0x100), short(0x100e, 0)]), None);
        assert_eq!(get(&tags, "FujiFilm:NoiseReduction"), Some("0 (normal)"));
        assert_eq!(get(&forms, "FujiFilm:NoiseReduction"), Some("0"));
    }

    /// Test 7(b): 0x100b = 0x100 alone produces no row at all.
    #[test]
    fn noise_reduction_0x100b_of_0x100_alone_is_absent() {
        let (tags, forms) = parse(&le_note(&[short(0x100b, 0x100)]), None);
        assert!(!tags.contains_key("FujiFilm:NoiseReduction"), "{tags:?}");
        assert!(!forms.contains_key("FujiFilm:NoiseReduction"));
    }

    /// Test 7(c): the residual 0x100b values (FujiFilm.pm:238-242).
    #[test]
    fn noise_reduction_0x100b_values() {
        let (tags, _) = parse(&le_note(&[short(0x100b, 0x80)]), None);
        assert_eq!(get(&tags, "FujiFilm:NoiseReduction"), Some("Normal"));
        let (tags, _) = parse(&le_note(&[short(0x100b, 0x40)]), None);
        assert_eq!(get(&tags, "FujiFilm:NoiseReduction"), Some("Low"));
    }

    /// Test 7(d): a kept 0x100b then 0x100e: the later entry wins, and the
    /// form under the key is 0x100e's, never paired with 0x100b's display.
    #[test]
    fn noise_reduction_0x100e_after_0x100b_wins_with_its_form() {
        let (tags, forms) = parse(&le_note(&[short(0x100b, 0x80), short(0x100e, 0x180)]), None);
        assert_eq!(
            get(&tags, "FujiFilm:NoiseReduction"),
            Some("+1 (medium strong)")
        );
        assert_eq!(get(&forms, "FujiFilm:NoiseReduction"), Some("384"));
    }

    /// Test 7(e): two engine `Contrast` ids -- the later (0x1006) wins.
    ///
    /// Neither insertion order reproduces ExifTool's `-j -G1` winner when an
    /// `ExifIFD:Contrast` also exists: `FoundTag` makes ExifIFD 0xa408
    /// (Exif.pm:2925, found after 0x927c) the primary, and the JSON writer
    /// then prints the FujiFilm copy found EARLIEST, 0x1004 (exiftool:2745-
    /// 2746, :2948-2953). No corpus note carries both 0x1004 and 0x1006, and
    /// a `-G0:1:4` census keys every copy separately; this pins today's
    /// choice (last).
    #[test]
    fn contrast_0x1006_after_0x1004_wins() {
        let (tags, forms) = parse(&le_note(&[short(0x1004, 0), short(0x1006, 0x100)]), None);
        assert_eq!(get(&tags, "FujiFilm:Contrast"), Some("High"));
        assert_eq!(get(&forms, "FujiFilm:Contrast"), Some("256"));
    }

    /// Test 7(f): with the engine on, the hand arms for ids `FujiFilm::Main`
    /// does not declare (0x1039 `DriveMode`, 0xf001 `RawImageFullWidth`) are
    /// unreachable.
    #[test]
    fn undeclared_hand_ids_are_skipped_with_the_engine_on() {
        assert!(
            engine_is_on(),
            "the (\"FujiFilm\", \"Main\") line is in force"
        );
        let (tags, _) = parse(&le_note(&[short(0x1039, 1), long(0xf001, 4000)]), None);
        assert!(!tags.contains_key("FujiFilm:DriveMode"), "{tags:?}");
        assert!(!tags.contains_key("FujiFilm:RawImageFullWidth"), "{tags:?}");
    }

    /// Test 8: 0x1100 `AutoBracketing` takes its X-T3 alternative from
    /// `$$self{Model}` (FujiFilm.pm:574-594).
    #[test]
    fn auto_bracketing_follows_the_model() {
        let note = le_note(&[short(0x1100, 2)]);
        let (tags, forms) = parse(&note, Some("X-T3"));
        assert_eq!(get(&tags, "FujiFilm:AutoBracketing"), Some("Pre-shot"));
        assert_eq!(get(&forms, "FujiFilm:AutoBracketing"), Some("2"));
        for model in [Some("X-T30"), Some("X-T2"), Some(""), None] {
            let (tags, _) = parse(&note, model);
            assert_eq!(
                get(&tags, "FujiFilm:AutoBracketing"),
                Some("No flash & flash"),
                "Model {model:?}"
            );
        }
        let (tags, _) = parse(&le_note(&[short(0x1100, 6)]), None);
        assert_eq!(get(&tags, "FujiFilm:AutoBracketing"), Some("Pixel Shift"));

        let group = fuji_main()
            .variant_group(0x1100)
            .expect("0x1100 is a _variants group");
        for (model, want) in [
            (Some("X-T3"), 0usize),
            (Some("X-T30"), 1),
            (Some("X-T2"), 1),
            (None, 1),
        ] {
            let mut members = HashMap::new();
            if let Some(model) = model {
                members.insert("Model", MemberValue::Str(model.to_string()));
            }
            let mut ctx = Ctx::new(&mut members).with_count(1);
            let winner = first_match_ifd(group.alternatives, &mut ctx)
                .expect("a Cond::Always alternative ends 0x1100");
            let index = group
                .alternatives
                .iter()
                .position(|(_, tag)| std::ptr::eq(tag, winner))
                .expect("the winner is one of the group's alternatives");
            assert_eq!(index, want, "0x1100 for Model {model:?}");
            assert!(alternative_is_reported(winner));
        }
    }
}
