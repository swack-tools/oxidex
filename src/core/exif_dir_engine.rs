//! `Exif::Main` walked at DirName `ExifIFD` / `InteropIFD` (slices E-1/E-2
//! of `oxidex-ops/slices/exif-ifd/spec.md`).
//!
//! # What this module owns
//!
//! One IFD-engine walk of the generated `IFD_EXIF_MAIN`
//! (`%Image::ExifTool::Exif::Main`, Exif.pm:411-4723, pinned 13.59) over one
//! directory the hand path in `tiff_helpers` already walks, buffered so the
//! hand walk can put each row into the map at its own entry's position
//! ([`DirEngineRows::replay`]); rows whose entry the hand walk never reached
//! go in after it ([`DirEngineRows::drain`]). This is
//! `canon/main_engine.rs`'s owns/replay/drain reshaped for [`MetadataMap`].
//!
//! Landing E-1 wires the InteropIFD (`tiff_helpers::parse_interop_subifd`);
//! E-2 is to wire the ExifIFD (`parse_exif_subifd`) through the same
//! [`walk`], [`DirEngineRows::owner`] and fence. `ifd1_engine_rows` can move
//! here later; it already shares [`engine_row_value`].
//!
//! # Why replay, not "engine rows, then residual rows"
//!
//! Every row keeps the `order` slot (`MetadataMap`'s sink counter) the hand
//! walk gave that entry, so the bare-name, `-TAG` and Composite folds
//! (`tag_resolution.rs`, `composite/mod.rs`) see the same sequence as
//! before: only a changed value, never a changed position, can move a
//! winner. And same-key pairs stay exact in an unsorted IFD: 24 names are
//! declared by two ids inside `Exif::Main`, all landing under one
//! `<Dir>:<Name>` key, and replay keeps ExifTool's entry order -- and so its
//! winner -- in every case.
//!
//! # The fence
//!
//! The walk is top level only: [`is_exif_main_row`] drops every row that is
//! not `Exif::Main`'s own under this DirName. Five edges target `Exif::Main`
//! itself, which is enabled (0x0190, 0x8769, 0xa005, 0xc51b, 0xc6f5
//! ProfileIFD); this generation emits all five unwalked, so today the fence
//! drops nothing -- it is what keeps a regeneration that walks one (a
//! ProfileIFD reported under group 1 `ProfileIFD`) from landing rows under
//! this directory's keys. The InteropIFD is a second root walk behind the
//! caller's PROCESSED guard, never a descent. The tripwire
//! `exif_main_edges_reach_no_enabled_table_but_itself` turns the day any
//! other edge target (GPS::Main, ICC_Profile, PrintIM, XMP, ...) gets an
//! allowlist line, or a self-edge gets walked, into a red test instead of a
//! silent double producer.

use std::collections::HashMap;

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_occurrence::{Instance, SHIM_DEFAULT_PRIORITY};
use crate::core::tag_value::TagValue;
use crate::core::tiff_helpers::trimmed_data_member;
use crate::exiftool_tables::{
    Ctx, Emitted, IfdDir, IfdTable, MemberValue, declares, engine_reports, process_exif, read_ifd,
};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// One engine row, held until the hand walk reaches its entry.
#[derive(Debug)]
struct Row {
    name: &'static str,
    /// `Emitted::value` through [`engine_row_value`].
    display: TagValue,
    /// ExifTool's `-n` form: `Emitted::value_conv` when a `PrintConv`
    /// rendered `display`, else `display` itself (`TagOccurrence::value_conv`
    /// must never re-derive one from a printed string).
    no_print_conv: TagValue,
    /// 0 iff the tag's effective priority is 0 (`Priority => 0`, or `Avoid`
    /// with no priority of its own; ExifTool.pm:9469-9473), else 1.
    priority: u8,
    consumed: bool,
}

/// Who produces the rows of one entry id while the engine runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    /// The generated table reports the id: the buffered engine row is the
    /// producer, the hand arm is unreachable.
    Engine,
    /// Withheld, absent, `Unknown`, or not in the table: the hand arm stays
    /// the producer (the residual).
    Hand,
    /// A `SubDirectory` edge the caller has decided reports nothing
    /// (Exif.pm:7103-7104: ExifTool processes the sub-directory and never
    /// reports the tag).
    Silent,
}

/// What one engine walk of `Exif::Main` reported for one directory, in
/// emission order (which is IFD entry order). See the module doc.
#[derive(Debug)]
pub(crate) struct DirEngineRows {
    table: &'static IfdTable,
    rows: Vec<Row>,
    /// How many entries the engine's `read_ifd` accepted; `None` when it
    /// refused the directory (then no row exists).
    entries: Option<usize>,
}

impl DirEngineRows {
    fn empty(table: &'static IfdTable) -> Self {
        Self {
            table,
            rows: Vec::new(),
            entries: None,
        }
    }

    /// Who owns entry `id`. Derived from the static, never from a hand list
    /// (Canon's `owns`): a regeneration that starts reporting an id moves it
    /// to the engine, one that stops fails the residual pins.
    pub(crate) fn owner(&self, id: u16, silence_edges: bool) -> Owner {
        if engine_reports(self.table, id) {
            Owner::Engine
        } else if silence_edges && is_edge_only(self.table, id) {
            Owner::Silent
        } else {
            Owner::Hand
        }
    }

    /// The engine's row for the entry `id` the hand walk has just reached:
    /// the next unconsumed row whose name `id` declares goes into `metadata`
    /// now, at this entry's position, under `key(name)`. When `keep(name,
    /// metadata)` is false the row is dropped (the yield-to-IFD0 rule), but
    /// consumed either way, so [`Self::drain`] cannot resurrect it.
    ///
    /// Names are not unique per id in `Exif::Main` (24 are declared by two
    /// ids), but the n-th-visit pairing is still exact: emission order is
    /// entry order, and two ids sharing a name share one key, so taking a
    /// same-named row one entry "early" changes neither the key nor the
    /// relative order of the two rows.
    ///
    /// Returns whether a row existed (recorded or dropped by `keep`). `false`
    /// means the engine refused the entry, and that absence is NOT always
    /// ExifTool's: `ifd_engine::locate`'s `table_ifd.rs` floor refuses an
    /// out-of-line value stored anywhere before the end of the directory,
    /// where ExifTool refuses only one that overlaps it (Exif.pm:6549; the
    /// K-O construct, E-2 commit 1), and a directory `read_ifd` refuses has
    /// no rows at all. So the caller decides: the InteropIFD caller falls
    /// back to its hand arm, the pre-engine producer, for that entry.
    pub(crate) fn replay(
        &mut self,
        id: u16,
        metadata: &mut MetadataMap,
        key: impl Fn(&str) -> String,
        keep: impl Fn(&str, &MetadataMap) -> bool,
    ) -> bool {
        let table = self.table;
        let Some(row) = self
            .rows
            .iter_mut()
            .find(|row| !row.consumed && declares(table, id, row.name))
        else {
            return false;
        };
        row.consumed = true;
        if keep(row.name, metadata) {
            record(row, metadata, key(row.name));
        }
        true
    }

    /// Records every row at `priority` instead of its table priority.
    ///
    /// The table priority is ExifTool's, but only against producers that
    /// model theirs: `Exif::Main`'s X/YResolution and ResolutionUnit carry
    /// `Priority => 0`, which in ExifTool still outranks the JFIF copies
    /// (`Priority => -1`, ExifTool.pm:2218-2233), while oxidex records JFIF
    /// at 1 (`jpeg_helpers.rs`). Until JFIF's -1 is modelled, a walk whose
    /// hand arms recorded at [`SHIM_DEFAULT_PRIORITY`] keeps that priority,
    /// so `-TAG` selection stays where the hand arm left it (the InteropIFD
    /// caller). E-2 chooses per directory.
    pub(crate) fn at_priority(mut self, priority: u8) -> Self {
        for row in &mut self.rows {
            row.priority = priority;
        }
        self
    }

    /// Rows whose entry the hand walk never reached (`parse_ifd` drops a
    /// malformed entry the engine's `read_ifd` may accept), in emission
    /// order, with the same `key`/`keep`. Later in IFD order than anything
    /// the hand walk visited, so last is their place.
    pub(crate) fn drain(
        self,
        metadata: &mut MetadataMap,
        key: impl Fn(&str) -> String,
        keep: impl Fn(&str, &MetadataMap) -> bool,
    ) {
        for row in self.rows.iter().filter(|row| !row.consumed) {
            if keep(row.name, metadata) {
                record(row, metadata, key(row.name));
            }
        }
    }

    /// The entry count the engine's `read_ifd` accepted (`None` = refused):
    /// the "whole directory refused where `parse_ifd` succeeded" diagnostic
    /// of the E-2 A/B (spec 6.3).
    pub(crate) fn entries(&self) -> Option<usize> {
        self.entries
    }
}

/// `<Dir>:<Name>` into the map the way `FoundTag` records it: the row's
/// priority, group1 `""` (the IFD1 convention, `tiff_helpers::IFD1_GROUP1`:
/// the key prefix is the family-1 label and `resolve_family0` maps
/// `ExifIFD`/`InteropIFD` to `EXIF`), and the `-n` form on the same
/// occurrence.
fn record(row: &Row, metadata: &mut MetadataMap, key: String) {
    metadata.insert_occurrence_with_raw(
        key,
        row.display.clone(),
        row.no_print_conv.clone(),
        row.priority,
        "",
        Instance::default(),
    );
}

/// One `ProcessExif` walk of the generated `Exif::Main` over the directory at
/// `ifd_start` of `tiff`, buffered for replay.
///
/// * `tiff` is ExifTool's `$$dirInfo{DataPt}`: the TIFF block, byte 0 = the
///   TIFF header, stored offsets relative to it -- so `base` is `Some(0)`,
///   the IFD1 shape. For a JPEG it is exactly the APP1 payload (`$dataLen`):
///   a value past it is refused, as ExifTool refuses one with no RAF
///   (Exif.pm:6551-6552).
/// * `dir` is the DirName: group 1 of every row (`SET_GROUP1`, Exif.pm:416,
///   7183) and what the fence admits.
/// * `$$self{Make}`/`{Model}` are IFD0's rows, trimmed as their RawConv
///   trims them (Exif.pm:585,595 `$val =~ s/\s+$//`) and as the MakerNote
///   Condition list reads them (`trimmed_data_member`). No compiled
///   `Exif::Main` condition can tell trimmed from untrimmed today
///   (`ifd1_engine_rows` seeds the untrimmed string).
pub(crate) fn walk(
    table: &'static IfdTable,
    tiff: &[u8],
    ifd_start: u64,
    order: ByteOrder,
    dir: &'static str,
    metadata: &MetadataMap,
) -> DirEngineRows {
    let mut rows = DirEngineRows::empty(table);
    let Ok(start) = usize::try_from(ifd_start) else {
        return rows;
    };
    rows.entries = read_ifd(tiff, start, order.to_io_byte_order()).map(|entries| entries.len());
    let mut members: HashMap<&'static str, MemberValue> = HashMap::new();
    for (member, key) in [("Make", "IFD0:Make"), ("Model", "IFD0:Model")] {
        let text = trimmed_data_member(metadata, key);
        if !text.is_empty() {
            members.insert(member, MemberValue::Str(text));
        }
    }
    let mut ctx = Ctx::new(&mut members);
    let mut emitted = Vec::new();
    process_exif(
        table,
        IfdDir {
            data: tiff,
            ifd_start: start,
            base: Some(0),
            byte_order: order.to_io_byte_order(),
            group1: Some(dir),
        },
        &mut ctx,
        &mut emitted,
    );
    for row in emitted {
        // FENCE: this directory's own `Exif::Main` rows only. See the module
        // doc.
        if !is_exif_main_row(&row, dir) {
            continue;
        }
        let display = engine_row_value(row.value);
        let no_print_conv = row
            .value_conv
            .map_or_else(|| display.clone(), engine_row_value);
        rows.rows.push(Row {
            name: row.name,
            display,
            no_print_conv,
            priority: if row.low_priority {
                0
            } else {
                SHIM_DEFAULT_PRIORITY
            },
            consumed: false,
        });
    }
    rows
}

/// The fence: a row of `Exif::Main` itself, walked under DirName `dir`.
/// Every other row arrived through a `SubDirectory` edge (a walked 0xc6f5
/// ProfileIFD would re-enter `Exif::Main` under group 1 `ProfileIFD`).
pub(crate) fn is_exif_main_row(row: &Emitted, dir: &str) -> bool {
    row.module == "Exif" && row.table == "Main" && row.group1 == dir
}

/// The value an engine row is stored as. The engine carries an unconverted
/// rational as the `Float` Perl numifies `RoundFloat($n/$d, 10)` to
/// (`ifd_engine::round_rationals`); Perl stringifies an integral one with no
/// fraction (`72`), which is what ExifTool's `-j` prints, while the JSON
/// writer prints a `Float` as `72.0`. So an integral float inside f64's
/// exact-integer range becomes `Integer` (the text writer's `perl_num`
/// already prints it that way); everything else passes through, so 145/2
/// stays `72.5`. Shared with `tiff_helpers::ifd1_engine_rows`.
pub(crate) fn engine_row_value(value: TagValue) -> TagValue {
    const EXACT: f64 = 9_007_199_254_740_992.0; // 2^53
    match value {
        TagValue::Float(f) if f.fract() == 0.0 && f.abs() < EXACT => TagValue::Integer(f as i64),
        other => other,
    }
}

/// Whether entry `id` of `table` is a `SubDirectory` edge and nothing else:
/// a plain tag with `subdir`, or a `_variants` group every alternative of
/// which is one.
pub(crate) fn is_edge_only(table: &IfdTable, id: u16) -> bool {
    table.tag(id).is_some_and(|tag| tag.subdir.is_some())
        || table
            .variant_group(id)
            .is_some_and(|group| group.alternatives.iter().all(|(_, t)| t.subdir.is_some()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool_tables::ifd_tables::IFD_EXIF_MAIN;
    use crate::exiftool_tables::{find_ifd_table, find_table};

    // -----------------------------------------------------------------
    // The pins (spec 3.3): a snapshot of what "the generated table does
    // not report" means at this generation, asserted EQUAL to the compiled
    // table's classification -- never a regex over `ifd_tables.rs`.
    // -----------------------------------------------------------------

    /// Plain `Exif::Main` tags with a non-empty `omitted` and no `subdir`:
    /// the engine withholds them, the hand arm stays their producer (80 at
    /// this generation; spec 3.1 lists each with its reason -- `raw_conv`
    /// for InteropVersion, Make, Model, ExifVersion, UserComment,
    /// FlashpixVersion ...; `print_conv` for ISO, ComponentsConfiguration,
    /// Flash, LensInfo ...; `value_conv` for ShutterSpeedValue, SubSecTime*,
    /// the Microsoft/Adobe 0xfexx and 0xfdxx tags ...). Sorted.
    const EXIF_MAIN_WITHHELD: &[u16] = &[
        0x0002, 0x00fe, 0x00ff, 0x0103, 0x010f, 0x0110, 0x0131, 0x013b, 0x0153, 0x80a6, 0x8298,
        0x87af, 0x87b0, 0x8827, 0x9000, 0x9101, 0x9201, 0x9206, 0x9209, 0x9216, 0x9286, 0x9287,
        0x9290, 0x9291, 0x9292, 0xa000, 0xa20c, 0xa216, 0xa302, 0xa40d, 0xa40e, 0xa432, 0xa462,
        0xbc01, 0xc5e0, 0xc612, 0xc613, 0xc615, 0xc616, 0xc61b, 0xc61c, 0xc630, 0xc65d, 0xc68b,
        0xc6d2, 0xc6d3, 0xc6f3, 0xc6f4, 0xc6f6, 0xc6f8, 0xc6fa, 0xc6fb, 0xc6fc, 0xc6fe, 0xc716,
        0xc717, 0xc718, 0xc71b, 0xc726, 0xc740, 0xc741, 0xc74e, 0xc763, 0xc772, 0xc7aa, 0xcd39,
        0xfde8, 0xfde9, 0xfdea, 0xfe4c, 0xfe4d, 0xfe4e, 0xfe51, 0xfe52, 0xfe53, 0xfe54, 0xfe55,
        0xfe56, 0xfe57, 0xfe58,
    ];

    /// Plain `Exif::Main` tags that are `SubDirectory` edges (28): the
    /// engine descends or marks them and never reports the tag itself.
    /// Sorted.
    const EXIF_MAIN_EDGES: &[u16] = &[
        0x0190, 0x02bc, 0x4748, 0x8290, 0x83bb, 0x8568, 0x8606, 0x8649, 0x8769, 0x8773, 0x8825,
        0x888a, 0x935c, 0x9999, 0xa005, 0xc4a5, 0xc519, 0xc51b, 0xc68c, 0xc68f, 0xc691, 0xc6f5,
        0xc7d5, 0xcd41, 0xcd44, 0xcd47, 0xcea1, 0xfe00,
    ];

    /// Ids ExifTool's `Exif::Main` declares that the static carries in no
    /// form (no tag, no `_variants` group): the generator refused every
    /// alternative. 0x927c is the MakerNote (94 alternatives,
    /// `ifd_variant_makernotes_dispatch`); the rest are the offset class
    /// (`IsOffset`/`OffsetPair`/`DataTag`). The hand path reads each of
    /// them today. Sorted.
    const EXIF_MAIN_ABSENT: &[u16] = &[
        0x0111, 0x0117, 0x0120, 0x0121, 0x0144, 0x0145, 0x014a, 0x0201, 0x0202, 0x0207, 0x0208,
        0x0209, 0x8781, 0x927c, 0xa010, 0xa011, 0xbcc0, 0xbcc1, 0xbcc2, 0xbcc3,
    ];

    const SNAPSHOT_MOVED: &str = "a regeneration moved an id between engine and hand: re-run the \
                                  spec 7.2 A/B, move the id in the snapshot in the same commit, \
                                  and name it in the commit body";

    fn all_ids(table: &IfdTable) -> Vec<u16> {
        let mut ids: Vec<u16> = table.tags.iter().map(|t| t.id).collect();
        ids.extend(table.variants.iter().map(|g| g.id));
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn exif_main_rows() -> DirEngineRows {
        DirEngineRows::empty(&IFD_EXIF_MAIN)
    }

    #[test]
    fn exif_main_withholding_is_the_snapshot() {
        for (name, list) in [
            ("EXIF_MAIN_WITHHELD", EXIF_MAIN_WITHHELD),
            ("EXIF_MAIN_EDGES", EXIF_MAIN_EDGES),
            ("EXIF_MAIN_ABSENT", EXIF_MAIN_ABSENT),
        ] {
            assert!(
                list.windows(2).all(|w| w[0] < w[1]),
                "{name} must be sorted and free of duplicates"
            );
        }
        let table = &IFD_EXIF_MAIN;
        let withheld: Vec<u16> = table
            .tags
            .iter()
            .filter(|t| t.omitted.any() && t.subdir.is_none())
            .map(|t| t.id)
            .collect();
        assert_eq!(
            withheld, EXIF_MAIN_WITHHELD,
            "EXIF_MAIN_WITHHELD: {SNAPSHOT_MOVED}"
        );
        let edges: Vec<u16> = table
            .tags
            .iter()
            .filter(|t| t.subdir.is_some())
            .map(|t| t.id)
            .collect();
        assert_eq!(edges, EXIF_MAIN_EDGES, "EXIF_MAIN_EDGES: {SNAPSHOT_MOVED}");
        for &id in EXIF_MAIN_ABSENT {
            assert!(
                table.tag(id).is_none() && table.variant_group(id).is_none(),
                "{id:#06x} is now transcribed: {SNAPSHOT_MOVED}"
            );
        }
        // The one compiled `_variants` group: 0xc634's six edge
        // alternatives (SR2Private, DNGAdobeData, the Pentax/Ricoh/DJI maker
        // notes) beside the reported `DNGPrivateData`, so the group is the
        // engine's.
        let groups: Vec<u16> = table.variants.iter().map(|g| g.id).collect();
        assert_eq!(groups, [0xc634], "Exif::Main _variants: {SNAPSHOT_MOVED}");
        let group = table.variant_group(0xc634).expect("0xc634 group");
        let reported: Vec<&str> = group
            .alternatives
            .iter()
            .filter(|(_, t)| crate::exiftool_tables::alternative_is_reported(t))
            .map(|(_, t)| t.name)
            .collect();
        assert_eq!(reported, ["DNGPrivateData"], "0xc634: {SNAPSHOT_MOVED}");
        let edge_alternatives = group
            .alternatives
            .iter()
            .filter(|(_, t)| t.subdir.is_some())
            .count();
        assert_eq!(edge_alternatives, 6, "0xc634: {SNAPSHOT_MOVED}");
    }

    #[test]
    fn hand_ids_are_never_engine_owned() {
        let rows = exif_main_rows();
        let table = &IFD_EXIF_MAIN;
        assert_eq!(rows.owner(0x927c, false), Owner::Hand, "MakerNote");
        assert_eq!(rows.owner(0x927c, true), Owner::Hand, "MakerNote");
        assert_ne!(rows.owner(0xa005, false), Owner::Engine, "InteropOffset");
        assert_ne!(rows.owner(0xa005, true), Owner::Engine, "InteropOffset");
        for &id in EXIF_MAIN_WITHHELD.iter().chain(EXIF_MAIN_ABSENT) {
            for silence in [false, true] {
                assert_eq!(rows.owner(id, silence), Owner::Hand, "{id:#06x}");
            }
        }
        let unknown: Vec<u16> = table
            .tags
            .iter()
            .filter(|t| t.flags.unknown)
            .map(|t| t.id)
            .collect();
        assert_eq!(
            unknown,
            [0xa102],
            "Exif::Main Unknown tags: {SNAPSHOT_MOVED}"
        );
        for &id in &unknown {
            assert_eq!(rows.owner(id, true), Owner::Hand, "{id:#06x}");
        }
        for &id in EXIF_MAIN_EDGES {
            assert_eq!(rows.owner(id, false), Owner::Hand, "{id:#06x}");
            assert_eq!(rows.owner(id, true), Owner::Silent, "{id:#06x}");
        }
        // Every id the engine reports is the engine's -- and the survey
        // correction by id, not name: a name-keyed residual would double
        // these (WhiteBalance, Contrast, Saturation, Sharpness, OwnerName,
        // SerialNumber each have a withheld 0xfexx/0xfdxx twin).
        let emit: Vec<u16> = all_ids(table)
            .into_iter()
            .filter(|&id| engine_reports(table, id))
            .collect();
        for &id in &emit {
            assert_eq!(rows.owner(id, true), Owner::Engine, "{id:#06x}");
        }
        for id in [0xa403u16, 0xa408, 0xa409, 0xa40a, 0xa430, 0xa431] {
            assert_eq!(rows.owner(id, false), Owner::Engine, "{id:#06x}");
        }
        // The partition is total: every declared id is exactly one of
        // engine, withheld, edge, unknown.
        assert_eq!(
            emit.len() + EXIF_MAIN_WITHHELD.len() + EXIF_MAIN_EDGES.len() + unknown.len(),
            all_ids(table).len(),
            "engine + withheld + edges + unknown must cover every declared id once"
        );
    }

    /// Spec 4.1 item 3: the fence, on hand-built rows.
    fn emitted(module: &'static str, table: &'static str, group1: &'static str) -> Emitted {
        Emitted {
            module,
            table,
            group0: "EXIF",
            group1,
            group2: "Image",
            name: "XResolution",
            value: TagValue::Integer(72),
            value_conv: None,
            low_priority: false,
            avoid: false,
        }
    }

    #[test]
    fn the_fence_admits_this_directorys_exif_main_rows_only() {
        assert!(is_exif_main_row(
            &emitted("Exif", "Main", "ExifIFD"),
            "ExifIFD"
        ));
        assert!(is_exif_main_row(
            &emitted("Exif", "Main", "InteropIFD"),
            "InteropIFD"
        ));
        assert!(!is_exif_main_row(
            &emitted("Exif", "Main", "ProfileIFD"),
            "ExifIFD"
        ));
        assert!(!is_exif_main_row(
            &emitted("Exif", "Main", "IFD1"),
            "ExifIFD"
        ));
        assert!(!is_exif_main_row(
            &emitted("Exif", "Main", "ExifIFD"),
            "InteropIFD"
        ));
        assert!(!is_exif_main_row(&emitted("GPS", "Main", "GPS"), "ExifIFD"));
        assert!(!is_exif_main_row(
            &emitted("GPS", "Main", "InteropIFD"),
            "InteropIFD"
        ));
    }

    /// Spec 4.1 item 2: no `Exif::Main` edge reaches an enabled table other
    /// than `Exif::Main` itself, and every edge into `Exif::Main` is emitted
    /// unwalked at this generation (0x0190 GlobalParametersIFD, 0x8769
    /// ExifOffset, 0xa005 InteropOffset, 0xc51b HasselbladExif, 0xc6f5
    /// ProfileIFD). The spec, read at d1c777b1, took 0xc6f5 to be walked;
    /// this generation marks it unwalked (same-table recursion, ProcessTiffIFD,
    /// `Magic`). Should a regeneration walk one, its rows arrive under group 1
    /// `ProfileIFD`/... and the fence drops them -- this test then names the
    /// edge so the change is a conscious one.
    #[test]
    fn exif_main_edges_reach_no_enabled_table_but_itself() {
        let table = &IFD_EXIF_MAIN;
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
        let mut into_itself = Vec::new();
        for (tag, edge) in edges {
            checked += 1;
            if (edge.module, edge.table) == ("Exif", "Main") {
                into_itself.push((tag.id, edge.unwalked.is_some()));
                continue;
            }
            let enabled = find_ifd_table(edge.module, edge.table).is_some_and(|t| t.enabled())
                || find_table(edge.module, edge.table).is_some_and(|t| t.enabled());
            assert!(
                !enabled,
                "Exif::Main {:#06x} {} reaches {}::{}, which is enabled: widen \
                 `is_exif_main_row` to this table AND retire its hand producer in the same change",
                tag.id, tag.name, edge.module, edge.table
            );
        }
        assert_eq!(
            into_itself,
            [
                (0x0190, true),
                (0x8769, true),
                (0xa005, true),
                (0xc51b, true),
                (0xc6f5, true)
            ],
            "the Exif::Main -> Exif::Main edges and whether each is emitted unwalked"
        );
        assert_eq!(checked, 28 + 6, "28 plain edges plus 0xc634's six");
    }

    // -----------------------------------------------------------------
    // walk / replay / drain on synthetic directories.
    // -----------------------------------------------------------------

    /// A little-endian TIFF block whose one IFD sits at offset 8 and holds
    /// `(id, type, count, bytes)` entries: values of four bytes or fewer
    /// inline, the rest after the IFD at TIFF-relative offsets.
    pub(crate) fn le_tiff(entries: &[(u16, u16, u32, Vec<u8>)]) -> Vec<u8> {
        let header = 8 + 2 + entries.len() * 12 + 4;
        let mut buffer = b"II\x2a\0\x08\0\0\0".to_vec();
        buffer.resize(header, 0);
        buffer[8..10].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        let mut values = Vec::new();
        for (index, (id, ty, count, bytes)) in entries.iter().enumerate() {
            let at = 10 + index * 12;
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

    fn ascii(id: u16, text: &[u8]) -> (u16, u16, u32, Vec<u8>) {
        (id, 2, text.len() as u32, text.to_vec())
    }

    fn short(id: u16, value: u16) -> (u16, u16, u32, Vec<u8>) {
        (id, 3, 1, value.to_le_bytes().to_vec())
    }

    fn interop_walk(tiff: &[u8]) -> DirEngineRows {
        walk(
            &IFD_EXIF_MAIN,
            tiff,
            8,
            ByteOrder::LittleEndian,
            "InteropIFD",
            &MetadataMap::new(),
        )
    }

    #[test]
    fn walk_buffers_interop_rows_with_their_no_print_conv_form() {
        let tiff = le_tiff(&[ascii(0x0001, b"R98\0"), short(0x1001, 640)]);
        let rows = interop_walk(&tiff);
        assert_eq!(rows.entries(), Some(2));
        let names: Vec<&str> = rows.rows.iter().map(|r| r.name).collect();
        assert_eq!(names, ["InteropIndex", "RelatedImageWidth"]);
        let index = &rows.rows[0];
        assert_eq!(
            index.display,
            TagValue::new_string("R98 - DCF basic file (sRGB)")
        );
        assert_eq!(index.no_print_conv, TagValue::new_string("R98"));
        assert_eq!(index.priority, SHIM_DEFAULT_PRIORITY);
        assert_eq!(rows.rows[1].display, TagValue::Integer(640));
        assert_eq!(rows.rows[1].no_print_conv, TagValue::Integer(640));
    }

    #[test]
    fn replay_inserts_at_the_entry_and_drain_only_the_unreached() {
        let tiff = le_tiff(&[ascii(0x0001, b"R98\0"), short(0x1001, 640)]);
        let mut rows = interop_walk(&tiff);
        let mut metadata = MetadataMap::new();
        let key = |name: &str| format!("InteropIFD:{name}");
        let keep = |_: &str, _: &MetadataMap| true;
        assert!(rows.replay(0x0001, &mut metadata, key, keep));
        assert_eq!(
            metadata.get_string("InteropIFD:InteropIndex"),
            Some("R98 - DCF basic file (sRGB)")
        );
        assert!(metadata.get("InteropIFD:RelatedImageWidth").is_none());
        // A second visit of the same id finds no unconsumed row, and says so.
        assert!(!rows.replay(0x0001, &mut metadata, key, keep));
        rows.drain(&mut metadata, key, keep);
        assert_eq!(
            metadata.get("InteropIFD:RelatedImageWidth"),
            Some(&TagValue::Integer(640))
        );
        let occurrences = metadata.occurrences_for("InteropIFD:InteropIndex");
        assert_eq!(occurrences.len(), 1, "one occurrence, recorded once");
        assert_eq!(occurrences[0].priority, SHIM_DEFAULT_PRIORITY);
        assert_eq!(
            metadata
                .without_print_conv()
                .get_string("InteropIFD:InteropIndex"),
            Some("R98"),
            "--no-print-conv shows the ValueConv form"
        );
    }

    #[test]
    fn a_dropped_row_is_consumed_and_never_drained() {
        let tiff = le_tiff(&[short(0x0128, 2)]);
        let mut rows = interop_walk(&tiff);
        let mut metadata = MetadataMap::new();
        let key = |name: &str| format!("InteropIFD:{name}");
        assert!(
            rows.replay(0x0128, &mut metadata, key, |_, _| false),
            "a row dropped by `keep` still existed: no hand fallback"
        );
        rows.drain(&mut metadata, key, |_, _| true);
        assert!(metadata.get("InteropIFD:ResolutionUnit").is_none());
    }

    /// The floor refusal `replay` reports as `false`: an out-of-line value
    /// stored before the directory, which ExifTool reads (Exif.pm:6549 only
    /// refuses an overlap) and `ifd_engine::locate`'s floor refuses.
    #[test]
    fn a_value_stored_before_the_directory_has_no_row_and_replay_says_so() {
        // Header, then the 8-byte rational at 8, then the IFD at 16.
        let mut tiff = b"II\x2a\0\x10\0\0\0".to_vec();
        tiff.extend([72u32.to_le_bytes(), 1u32.to_le_bytes()].concat());
        tiff.extend(1u16.to_le_bytes());
        tiff.extend(0x011au16.to_le_bytes());
        tiff.extend(5u16.to_le_bytes());
        tiff.extend(1u32.to_le_bytes());
        tiff.extend(8u32.to_le_bytes());
        tiff.extend(0u32.to_le_bytes());
        let mut rows = walk(
            &IFD_EXIF_MAIN,
            &tiff,
            16,
            ByteOrder::LittleEndian,
            "InteropIFD",
            &MetadataMap::new(),
        );
        assert_eq!(rows.entries(), Some(1));
        assert!(rows.rows.is_empty(), "{:?}", rows.rows);
        let mut metadata = MetadataMap::new();
        assert!(!rows.replay(
            0x011a,
            &mut metadata,
            |name: &str| format!("InteropIFD:{name}"),
            |_, _| true
        ));
        assert!(metadata.is_empty());
    }

    #[test]
    fn at_priority_overrides_the_table_priority_of_every_row() {
        // 0x011a XResolution is `Priority => 0` in Exif::Main.
        let tiff = le_tiff(&[
            (
                0x011a,
                5,
                1,
                [72u32.to_le_bytes(), 1u32.to_le_bytes()].concat(),
            ),
            short(0x1001, 640),
        ]);
        let rows = interop_walk(&tiff);
        assert_eq!(rows.rows[0].priority, 0, "table Priority => 0");
        assert_eq!(rows.rows[1].priority, SHIM_DEFAULT_PRIORITY);
        let mut rows = rows.at_priority(SHIM_DEFAULT_PRIORITY);
        assert!(
            rows.rows
                .iter()
                .all(|r| r.priority == SHIM_DEFAULT_PRIORITY)
        );
        let mut metadata = MetadataMap::new();
        rows.replay(
            0x011a,
            &mut metadata,
            |n: &str| format!("InteropIFD:{n}"),
            |_, _| true,
        );
        assert_eq!(
            metadata.occurrences_for("InteropIFD:XResolution")[0].priority,
            SHIM_DEFAULT_PRIORITY
        );
    }

    #[test]
    fn a_refused_directory_has_no_rows() {
        // Four entries declared, three present: `read_ifd` refuses the
        // whole non-maker directory (Exif.pm:6385-6389 "Bad ... directory").
        let mut tiff = le_tiff(&[short(0x0128, 2), short(0x1001, 1), short(0x1002, 1)]);
        tiff[8..10].copy_from_slice(&4u16.to_le_bytes());
        tiff.truncate(10 + 3 * 12);
        let rows = interop_walk(&tiff);
        assert_eq!(rows.entries(), None);
        assert!(rows.rows.is_empty());
    }

    /// Spec 7.1 test 12, for the InteropIFD names: the output layer's
    /// name-keyed rules (`format_tag_value_rules`, which every stored value
    /// goes through on its way to `-j`) leave every engine-rendered Interop
    /// value alone. The values are the pinned 13.59 oracle's own
    /// (`-G1 -a -s -j -InteropIFD:all`: t/images Canon.jpg `THM`, Nikon.jpg
    /// `R98`; combined-samples SamsungGT-S5250 `R 9 8 `, CanonHG20 ``,
    /// SamsungVP-D73 `R99`, SamsungSGH-F700 `[None]`, SamsungSPH-A800
    /// `inches`) as the engine renders them from the raw strings.
    #[test]
    fn output_rules_are_a_no_op_on_engine_interop_values() {
        let raw_indexes: [&[u8]; 8] = [
            b"THM\0",
            b"R98\0",
            b"R03\0",
            b"R 9 8 \0",
            b"\0\0\0\0",
            b"R99\0",
            b"[None]\0",
            b"\r\r\r\x11",
        ];
        let mut checked = 0;
        for raw in raw_indexes {
            let tiff = le_tiff(&[
                ascii(0x0001, raw),
                short(0x0128, 2),
                short(0x1001, 3072),
                short(0x1002, 2048),
                (
                    0x011a,
                    5,
                    1,
                    [72u32.to_le_bytes(), 1u32.to_le_bytes()].concat(),
                ),
                (
                    0x011b,
                    5,
                    1,
                    [145u32.to_le_bytes(), 2u32.to_le_bytes()].concat(),
                ),
            ]);
            let rows = interop_walk(&tiff);
            assert_eq!(rows.rows.len(), 6, "{raw:?}: {:?}", rows.rows);
            for row in &rows.rows {
                for prefix in ["EXIF", "InteropIFD"] {
                    let key = format!("{prefix}:{}", row.name);
                    assert_eq!(
                        crate::core::exiftool_compat::format_tag_value_rules(&key, &row.display),
                        row.display,
                        "{key} re-converted by the output layer"
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 8 * 6 * 2);
        for unit in [1u16, 2, 3, 7] {
            let rows = interop_walk(&le_tiff(&[short(0x0128, unit)]));
            let display = &rows.rows[0].display;
            assert_eq!(
                crate::core::exiftool_compat::format_tag_value_rules(
                    "InteropIFD:ResolutionUnit",
                    display
                ),
                *display
            );
        }
        // The rendered strings themselves, against the oracle.
        let index = |raw: &[u8]| {
            interop_walk(&le_tiff(&[ascii(0x0001, raw)])).rows[0]
                .display
                .clone()
        };
        assert_eq!(
            index(b"THM\0"),
            TagValue::new_string("THM - DCF thumbnail file")
        );
        assert_eq!(index(b"R 9 8 \0"), TagValue::new_string("Unknown (R 9 8 )"));
        assert_eq!(index(b"R99\0"), TagValue::new_string("Unknown (R99)"));
        assert_eq!(index(b"[None]\0"), TagValue::new_string("Unknown ([None])"));
    }

    #[test]
    fn engine_row_value_integralises_exact_floats_only() {
        assert_eq!(
            engine_row_value(TagValue::Float(300.0)),
            TagValue::Integer(300)
        );
        assert_eq!(engine_row_value(TagValue::Float(0.5)), TagValue::Float(0.5));
        assert_eq!(
            engine_row_value(TagValue::new_string("8 8 8")),
            TagValue::new_string("8 8 8")
        );
    }
}
