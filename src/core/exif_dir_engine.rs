//! `Exif::Main` walked at DirName `ExifIFD` / `InteropIFD` (slices E-1/E-2
//! of `oxidex-ops/slices/exif-ifd/spec.md`).
//!
//! # What this module owns
//!
//! One IFD-engine walk of the generated `IFD_EXIF_MAIN`
//! (`%Image::ExifTool::Exif::Main`, Exif.pm:411-4723, pinned 13.59) over one
//! directory the hand path in `tiff_helpers` already walks, buffered so the
//! shared route can decide one owner for each physical entry position
//! ([`DirEngineRows::route_entry`]); rows whose entry the hand parser never
//! reached go in after it ([`DirEngineRows::finish`]).
//!
//! Landing E-1 wires the InteropIFD (`tiff_helpers::parse_interop_subifd`);
//! Task 9 routes IFD0, IFD1, ExifIFD, and InteropIFD through that same
//! occurrence-indexed decision. [`DirEngineRows::keep_hand`] preserves the
//! named hand-owned exceptions and [`tag_priority_is_zero`] preserves a
//! residual row's own priority. The older id/name replay entry points remain
//! only for the unleased embedded-EXIF compatibility adapter and focused
//! legacy tests.
//!
//! # Why routing follows physical entries
//!
//! Every row keeps the `order` slot (`MetadataMap`'s sink counter) the hand
//! walk gave that entry, so the bare-name, `-TAG` and Composite folds
//! (`tag_resolution.rs`, `composite/mod.rs`) see the same sequence as
//! before: only a changed value, never a changed position, can move a
//! winner. Duplicate ids and same-key pairs stay exact in an unsorted IFD:
//! each generated row carries the zero-based source entry index, so a
//! declined first occurrence cannot consume a later occurrence's row.
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
use crate::exiftool_tables::session::{MemberVal, Session};
use crate::exiftool_tables::{
    Ctx, Emitted, EntryRead, IfdDir, IfdEntry, IfdTable, MAX_IFD_ENTRIES, MemberValue, declares,
    engine_reports, read_ifd,
};
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// One engine row, held until the hand walk reaches its entry.
#[derive(Debug)]
struct Row {
    /// Physical zero-based entry position in this directory. Ownership is
    /// occurrence-based: duplicate ids and same-name ids must never borrow
    /// one another's generated row.
    entry_index: usize,
    name: &'static str,
    /// `Emitted::value` through [`engine_row_value`].
    display: TagValue,
    /// ExifTool's `-n` form: `Emitted::value_conv` when a `PrintConv`
    /// rendered `display`, else `display` itself (`TagOccurrence::value_conv`
    /// must never re-derive one from a printed string) -- kept as the
    /// fraction when `display` is an unconverted single rational
    /// (`Emitted::rational`, see [`rational_value`]).
    no_print_conv: TagValue,
    /// The entry's value as the file stores it, typed as the hand arm typed
    /// it (`TagOccurrence::stored`): what the PNG `eXIf` rebuild and
    /// `copy_metadata` serialize. See [`stored_value`].
    stored: Option<TagValue>,
    /// 0 iff the tag's effective priority is 0 (`Priority => 0`, or `Avoid`
    /// with no priority of its own; ExifTool.pm:9469-9473), else 1.
    priority: u8,
    /// The row's field was the hand arm's before a generated conversion arm
    /// took it (its static conversion is `omitted`; Autogeneration v2): it
    /// keeps the priority the hand residual recorded it at --
    /// [`tag_priority_is_zero`] -- through [`DirEngineRows::at_priority`], so
    /// a 0xfe4e WhiteBalance (`Avoid`) still cannot displace the 0xa403 row.
    keeps_priority: bool,
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
    /// For a refused directory: whether ExifTool refuses it too (see
    /// [`walk`]), so that its absence is ExifTool's.
    refused_as_exiftool: bool,
    /// The id of each entry `read_ifd` accepted, in entry order.
    ids: Vec<u16>,
    /// Per entry of `ids`: what the walk did with it
    /// (`ifd_engine::process_exif_decoded`).
    reads: Vec<EntryRead>,
    /// Engine-reported ids the caller keeps on its hand arms
    /// ([`Self::keep_hand`], every call's ids).
    hand_kept: Vec<u16>,
    /// Names of rows the walk dropped because their `ValueConv` result is
    /// not a finite number (see [`walk`]): an absence of the engine's own.
    unrenderable: Vec<&'static str>,
    /// Whether rows are recorded with their stored form
    /// ([`Self::with_stored_forms`]).
    stored_forms: bool,
    /// The session guard had already processed this exact directory. Its
    /// legacy residual must also stay silent or the second edge would replay
    /// tags the shared walk intentionally de-duplicated.
    already_processed: bool,
}

impl DirEngineRows {
    fn empty(table: &'static IfdTable) -> Self {
        Self {
            table,
            rows: Vec::new(),
            entries: None,
            refused_as_exiftool: false,
            ids: Vec::new(),
            reads: Vec::new(),
            hand_kept: Vec::new(),
            unrenderable: Vec::new(),
            stored_forms: false,
            already_processed: false,
        }
    }

    /// Who owns entry `id`. Derived from the static, never from a hand list
    /// (Canon's `owns`): a regeneration that starts reporting an id moves it
    /// to the engine, one that stops fails the residual pins. The one
    /// exception is the caller's explicit [`Self::keep_hand`] list.
    pub(crate) fn owner(&self, id: u16, silence_edges: bool) -> Owner {
        if self.hand_kept.contains(&id) {
            Owner::Hand
        } else if engine_reports(self.table, id) {
            Owner::Engine
        } else if silence_edges && is_edge_only(self.table, id) {
            Owner::Silent
        } else {
            Owner::Hand
        }
    }

    /// Resolve and publish one physical directory entry exactly once.
    ///
    /// `entry_index` is the shared identity between `read_ifd` and the hand
    /// parser. It replaces id/name replay, which cannot distinguish a
    /// declined occurrence from a later generated occurrence of the same
    /// tag. The caller invokes its one named residual only when this returns
    /// [`Owner::Hand`].
    pub(crate) fn route_entry(
        &mut self,
        entry_index: usize,
        id: u16,
        silence_edges: bool,
        requested_edge: bool,
        refused_owner: Owner,
        metadata: &mut MetadataMap,
        key: impl Fn(&str) -> String,
        keep: impl Fn(&str, &MetadataMap) -> bool,
    ) -> Owner {
        let owner = if self.already_processed {
            Owner::Silent
        } else if self.hand_kept.contains(&id) {
            Owner::Hand
        } else if engine_reports(self.table, id) {
            if self
                .rows
                .iter()
                .any(|row| !row.consumed && row.entry_index == entry_index)
            {
                Owner::Engine
            } else {
                match self.reads.get(entry_index).copied() {
                    Some(EntryRead::Unread) | None => Owner::Hand,
                    Some(EntryRead::Refused) => refused_owner,
                    Some(EntryRead::Decoded) => Owner::Silent,
                }
            }
        } else if silence_edges && is_edge_only(self.table, id) && !requested_edge {
            Owner::Silent
        } else {
            Owner::Hand
        };

        if owner == Owner::Engine {
            for row in self
                .rows
                .iter_mut()
                .filter(|row| !row.consumed && row.entry_index == entry_index)
            {
                row.consumed = true;
                if keep(row.name, metadata) {
                    record(row, self.stored_forms, metadata, key(row.name));
                }
            }
        }
        owner
    }

    /// Compatibility route for the unleased embedded-EXIF adapter and
    /// focused legacy tests. Standard Exif directories use
    /// [`Self::route_entry`], whose physical index distinguishes duplicate
    /// occurrences.
    ///
    /// The engine's row for the entry `id` the hand walk has just reached:
    /// the next unconsumed row whose name `id` declares goes into `metadata`
    /// now, at this entry's position, under `key(name)`. When `keep(name,
    /// metadata)` is false the row is dropped (the yield-to-IFD0 rule), but
    /// consumed either way, so [`Self::drain`] cannot resurrect it.
    ///
    /// Returns whether a row existed (recorded or dropped by `keep`). `false`
    /// means the engine reported nothing for the entry, and the caller
    /// decides what that absence is: the InteropIFD caller falls back to its
    /// hand arm, the pre-engine producer, for that entry; the ExifIFD caller
    /// does so only where the engine's absence is its own
    /// ([`Self::undecoded`]).
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
            record(row, self.stored_forms, metadata, key(row.name));
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
        for row in self.rows.iter_mut().filter(|row| !row.keeps_priority) {
            row.priority = priority;
        }
        self
    }

    /// Records every row with the value its entry stores
    /// (`TagOccurrence::stored`, see [`stored_value`]) beside the printed
    /// one: for a directory whose hand arm stored that typed value as the
    /// map value, which the writers that rebuild or copy a tag without the
    /// original bytes (the PNG `eXIf` rebuild, `copy_metadata`) serialize
    /// (the ExifIFD caller, slice E-2).
    pub(crate) fn with_stored_forms(mut self) -> Self {
        self.stored_forms = true;
        self
    }

    /// Keeps the engine-reported `ids` on the caller's hand arms, one
    /// landing at a time: [`Self::owner`] answers [`Owner::Hand`] for them,
    /// and their buffered rows are dropped so neither [`Self::replay`] nor
    /// [`Self::drain`] records one beside the hand row. A row is matched by
    /// the names the ids declare, so no id in `ids` may share a name with an
    /// id the engine keeps (the caller's pin checks it). Calls accumulate.
    pub(crate) fn keep_hand(mut self, ids: &[u16]) -> Self {
        let table = self.table;
        for row in &mut self.rows {
            if ids.iter().any(|&id| declares(table, id, row.name)) {
                row.consumed = true;
            }
        }
        self.hand_kept.extend_from_slice(ids);
        self
    }

    /// Whether the engine's absence for entry `id` is its own, one ExifTool
    /// does not share: it refused the whole directory on a bound ExifTool
    /// does not have (more than `MAX_IFD_ENTRIES` entries), never saw such
    /// an entry, or saw one whose value it left [`EntryRead::Unread`] (a
    /// value it cannot locate without a base, an unmodelled `Format`
    /// override, an unresolved tag or `Condition` ...; see
    /// `ifd_engine::process_exif_decoded`). A [`Self::replay`] miss for such
    /// an id is an absence the engine cannot vouch for, and a caller may
    /// fall back to its hand arm, the pre-engine reader.
    ///
    /// Everything else is an absence ExifTool shares, and falling back there
    /// would put back exactly what it refuses:
    /// * a value the engine DID decode and whose conversion withheld the tag
    ///   (a `ValueConv` that cannot numify its input, a `RawConv` returning
    ///   undef) -- the engine refused to approximate it;
    /// * an entry [`EntryRead::Refused`]: an out-of-line value whose offset
    ///   points into the TIFF header or that overlaps the directory
    ///   ("Suspicious ExifIFD offset", Exif.pm:6539 / 6549) or lying
    ///   past the TIFF block ("Bad offset", Exif.pm:6551-6552 -- a JPEG's
    ///   APP1 payload is ExifTool's whole `DataPt`, with no `RAF` to seek),
    ///   a bad type code, an entry past the exhausted warning budget;
    /// * a directory whose entry array runs past the block ("Bad ExifIFD
    ///   directory", Exif.pm:6385-6389).
    ///
    /// A row the walk dropped as unrenderable (a non-finite `ValueConv`
    /// result, see [`walk`]) is the engine's own absence too.
    pub(crate) fn undecoded(&self, id: u16) -> bool {
        let table = self.table;
        if self
            .unrenderable
            .iter()
            .any(|name| declares(table, id, name))
        {
            return true;
        }
        if self.entries.is_none() {
            return !self.refused_as_exiftool;
        }
        let mut seen = false;
        for (&entry_id, &read) in self.ids.iter().zip(&self.reads) {
            if entry_id == id {
                if read == EntryRead::Unread {
                    return true;
                }
                seen = true;
            }
        }
        !seen
    }

    /// Rows whose entry the hand walk never reached (`parse_ifd` drops a
    /// malformed entry the engine's `read_ifd` may accept), in emission
    /// order, with the same `key`/`keep`. Later in IFD order than anything
    /// the hand walk visited, so last is their place.
    pub(crate) fn finish(
        self,
        metadata: &mut MetadataMap,
        key: impl Fn(&str) -> String,
        keep: impl Fn(&str, &MetadataMap) -> bool,
    ) {
        for row in self.rows.iter().filter(|row| !row.consumed) {
            if keep(row.name, metadata) {
                record(row, self.stored_forms, metadata, key(row.name));
            }
        }
    }

    /// Compatibility name for focused legacy tests. Standard Exif directory
    /// callers use [`Self::route_entry`] plus [`Self::finish`].
    #[cfg(test)]
    pub(crate) fn drain(
        self,
        metadata: &mut MetadataMap,
        key: impl Fn(&str) -> String,
        keep: impl Fn(&str, &MetadataMap) -> bool,
    ) {
        self.finish(metadata, key, keep);
    }

    /// Records every row at `priority`, including the rows
    /// [`Self::at_priority`] leaves at their own (`keeps_priority`): for a
    /// directory whose hand arm recorded every entry at one priority (IFD0's
    /// `insert()`), so moving an entry to the engine moves no `-TAG` winner.
    pub(crate) fn at_uniform_priority(mut self, priority: u8) -> Self {
        for row in &mut self.rows {
            row.priority = priority;
        }
        self
    }

    /// Preserve the forms the pre-shared IFD1 adapter exposed while using
    /// the common occurrence route. That adapter kept EXIF date text as a
    /// string and used the engine's integral/float display form for an
    /// unconverted rational; ExifIFD intentionally keeps its typed datetime
    /// and exact rational form for writers and composites.
    pub(crate) fn with_ifd1_forms(mut self) -> Self {
        for row in &mut self.rows {
            if let TagValue::DateTime(value) = &row.display {
                row.display = TagValue::new_string(value.format("%Y:%m:%d %H:%M:%S").to_string());
            }
            if let TagValue::DateTime(value) = &row.no_print_conv {
                row.no_print_conv =
                    TagValue::new_string(value.format("%Y:%m:%d %H:%M:%S").to_string());
            } else if matches!(row.no_print_conv, TagValue::Rational { .. })
                && matches!(row.display, TagValue::Integer(_) | TagValue::Float(_))
            {
                row.no_print_conv = row.display.clone();
            }
        }
        self
    }

    /// The IFD0 hand walks' per-entry question (see [`ifd0_walk`]): whether
    /// the engine produced entry `id` -- its row recorded now, at this
    /// entry's position, as `IFD0:<name>` -- or vouches for its absence (an
    /// absence ExifTool shares, see [`Self::undecoded`]). `false` means the
    /// hand arm produces the entry, exactly as before the engine: an id the
    /// engine does not report (withheld, a `SubDirectory` edge, `Unknown`,
    /// untranscribed, [`IFD0_HAND_KEPT`]) or an absence that is the
    /// engine's own (a generated arm declined a withheld field, an entry
    /// `read_ifd` never saw).
    pub(crate) fn take_ifd0(&mut self, id: u16, metadata: &mut MetadataMap) -> bool {
        self.owner(id, false) == Owner::Engine
            && (self.replay(id, metadata, ifd0_key, |_, _| true) || !self.undecoded(id))
    }

    /// [`Self::drain`] for IFD0: rows whose entry the hand walk never reached.
    pub(crate) fn finish_ifd0(self, metadata: &mut MetadataMap) {
        self.finish(metadata, ifd0_key, |_, _| true);
    }

    /// Compatibility entry point for the unleased embedded-EXIF adapter.
    /// JPEG and standalone TIFF use the exact-once route directly.
    pub(crate) fn drain_ifd0(self, metadata: &mut MetadataMap) {
        self.finish_ifd0(metadata);
    }

    /// The table this walk read.
    pub(crate) fn table(&self) -> &'static IfdTable {
        self.table
    }

    /// Whether the file-scoped session had already walked this exact table at
    /// this exact address. Callers must stop the whole directory adapter in
    /// this case, including structural hand arms that run before or after the
    /// ordinary per-entry route (MakerNotes, previews and nested edges).
    pub(crate) fn already_processed(&self) -> bool {
        self.already_processed
    }

    /// The entry count the engine's `read_ifd` accepted (`None` = refused):
    /// the "whole directory refused where `parse_ifd` succeeded" diagnostic
    /// of the E-2 A/B (spec 6.3).
    pub(crate) fn entries(&self) -> Option<usize> {
        self.entries
    }
}

/// IFD0 entries every hand walk consumes structurally before it would record
/// a row, so the engine's row for them is never replayed nor drained: the
/// IPTC-NAA block (0x83bb, parsed into `IPTC:*`), the GeoTIFF key directory
/// and its parameter blocks (0x87af, 0x87b0, 0x87b1, parsed into GeoTIFF
/// keys) and ModelTransform (0x85d8, printed as `EXIF:ModelTransform`), and
/// PrintIM (0xc4a5, `PrintIM:PrintIMVersion`). Their hand treatment is not a
/// conversion of the entry and stays as it is.
///
/// And the five Windows XP strings, 0x9c9b-0x9c9f XPTitle, XPComment,
/// XPAuthor, XPKeywords, XPSubject (`ValueConv =>
/// '$self->Decode($val,"UCS2","II")'`, Exif.pm): the generated backend
/// refuses them (`Decode` has no proven port, `conv::exif_main::REFUSED`), and
/// the static table's `exprs::decode_ucs2` keeps a leading U+0000 as a
/// character where ExifTool's value ends at it -- FujiFilmFinePixZ100fd.jpg
/// (and Z200fd, Z250fd), whose XPTitle is `00 00` then fifteen UCS-2 spaces,
/// prints `""` under the pinned 13.59 (`-j`, `-b` empty) and fifteen spaces
/// from the engine. The hand arm prints ExifTool's value. Sorted.
pub(crate) const IFD0_HAND_KEPT: &[u16] = &[
    0x83bb, 0x85d8, 0x87af, 0x87b0, 0x87b1, 0x9c9b, 0x9c9c, 0x9c9d, 0x9c9e, 0x9c9f, 0xc4a5,
];

/// The key an engine-produced IFD0 row is recorded under: ExifTool's family
/// 1, as the hand walks key it (`lookup_tag_name(id, "IFD0")`).
fn ifd0_key(name: &str) -> String {
    format!("IFD0:{name}")
}

/// One engine walk of IFD0 (DirName `IFD0`, slice v2-ifd0) for the three
/// hand walks of it -- `jpeg_helpers::process_ifd0_tags` (a JPEG's APP1),
/// `tiff_helpers::process_tiff_ifd_tags` (a standalone TIFF's first
/// directory) and `embedded::parse_embedded_exif_at` (PNG `eXIf`, PSD,
/// HEIF, WebP, JXL). Each keeps its own walk, order, pointers and
/// structural entries; for every ordinary entry it asks
/// [`DirEngineRows::take_ifd0`] first and runs its hand arm only on `false`
/// (per-field mixed mode, as the ExifIFD, InteropIFD and IFD1 walks).
///
/// * `tiff` is the TIFF block, byte 0 = the TIFF header (ExifTool's
///   `DataPt`), and `ifd0` IFD0's offset in it.
/// * Rows are recorded at the hand walks' own priority,
///   [`SHIM_DEFAULT_PRIORITY`], every one of them
///   ([`DirEngineRows::at_uniform_priority`]): IFD0's hand arm recorded
///   every entry through `insert()`, and IFD0 is the directory a bare
///   request answers from, so no winner moves.
/// * `SubDirectory` edges are not silenced: IFD0's hand arm reports what it
///   reported before (the ICC_Profile blob, the MakerNote row).
///
/// `None` when the generated `Exif::Main` is not in force (Gate A or the
/// allowlist): then every entry is the hand arm's, as before the slice.
pub(crate) fn ifd0_walk_with_session(
    tiff: &[u8],
    data_domain: u64,
    ifd0: u64,
    order: ByteOrder,
    metadata: &MetadataMap,
    session: &mut Session,
    ctx: &mut Ctx<'_>,
) -> Option<DirEngineRows> {
    // The lookup is spelled with literal arguments because
    // `tools/exiftool-tables/reachability.py` counts literal call sites;
    // `enabled()` re-checks Gate A and the allowlist at runtime.
    let table = crate::exiftool_tables::find_ifd_table("Exif", "Main").filter(|t| t.enabled())?;
    Some(
        walk_with_session(
            table,
            tiff,
            data_domain,
            ifd0,
            order,
            "IFD0",
            metadata,
            session,
            ctx,
        )
        .at_uniform_priority(SHIM_DEFAULT_PRIORITY)
        .keep_hand(IFD0_HAND_KEPT)
        .with_stored_forms(),
    )
}

/// `<Dir>:<Name>` into the map the way `FoundTag` records it: the row's
/// priority, group1 `""` (the IFD1 convention, `tiff_helpers::IFD1_GROUP1`:
/// the key prefix is the family-1 label and `resolve_family0` maps
/// `ExifIFD`/`InteropIFD` to `EXIF`), and the `-n` form on the same
/// occurrence, and its stored form when the caller asked for it.
fn record(row: &Row, stored_forms: bool, metadata: &mut MetadataMap, key: String) {
    metadata.insert_occurrence_with_forms(
        key,
        row.display.clone(),
        row.no_print_conv.clone(),
        row.stored.clone().filter(|_| stored_forms),
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
pub(crate) fn walk_with_session(
    table: &'static IfdTable,
    tiff: &[u8],
    data_domain: u64,
    ifd_start: u64,
    order: ByteOrder,
    dir: &'static str,
    metadata: &MetadataMap,
    session: &mut Session,
    ctx: &mut Ctx<'_>,
) -> DirEngineRows {
    let mut rows = DirEngineRows::empty(table);
    let Ok(start) = usize::try_from(ifd_start) else {
        return rows;
    };
    let ifd_entries = read_ifd(tiff, start, order.to_io_byte_order());
    if let Some(entries) = &ifd_entries {
        rows.entries = Some(entries.len());
        rows.ids = entries.iter().map(|entry| entry.tag_id).collect();
    } else {
        rows.refused_as_exiftool = directory_refused_as_exiftool(tiff, start, order);
    }
    for (member, key) in [("Make", "IFD0:Make"), ("Model", "IFD0:Model")] {
        let text = trimmed_data_member(metadata, key);
        if !text.is_empty() {
            ctx.members.insert(member, MemberValue::Str(text.clone()));
            session
                .set_member(member, MemberVal::Str(text))
                .expect("Make and Model are UTF-8 metadata strings");
        }
    }
    let mut emitted = Vec::new();
    let root = crate::exiftool_tables::ifd_engine::process_exif_decoded_outcome(
        table,
        IfdDir {
            data: tiff,
            data_domain,
            ifd_start: start,
            base: Some(0),
            byte_order: order.to_io_byte_order(),
            group1: Some(dir),
        },
        session,
        ctx,
        &mut emitted,
    );
    let root = match root {
        crate::exiftool_tables::ifd_engine::ProcessExifDecoded::Read(root) => root,
        crate::exiftool_tables::ifd_engine::ProcessExifDecoded::AlreadyProcessed => {
            rows.already_processed = true;
            Default::default()
        }
        crate::exiftool_tables::ifd_engine::ProcessExifDecoded::Refused => Default::default(),
    };
    rows.reads = root.entries;
    // Which entry each root row came from, for its stored form.
    let row_entry: HashMap<usize, usize> = root.rows.into_iter().collect();
    for (index, row) in emitted.into_iter().enumerate() {
        // FENCE: this directory's own `Exif::Main` rows only. See the module
        // doc.
        if !is_exif_main_row(&row, dir) {
            continue;
        }
        // A `ValueConv` that overflows (ApertureValue's `2**($val/2)` on a
        // raw 2147483648/1, CanonEOS20Da.jpg) hands its `PrintConv` an
        // infinity, and the compiled `sprintf("%.1f",$val)` is Rust's
        // `format!`, which prints `inf` where Perl prints `Inf`. The row is
        // dropped as the engine's own absence (`undecoded`), so the entry
        // keeps its hand arm, which prints ExifTool's `Inf`. A generated arm
        // (Autogeneration v2) prints the infinity as Perl does (`rt::sprintf`
        // spells `Inf`), so only Rust's lowercase spelling is dropped.
        if matches!(&row.value_conv, Some(TagValue::Float(f)) if !f.is_finite())
            && matches!(&row.value, TagValue::String(s) if s.contains("inf"))
        {
            rows.unrenderable.push(row.name);
            continue;
        }
        let entry = row_entry
            .get(&index)
            .and_then(|&entry| ifd_entries.as_deref()?.get(entry));
        let stored = entry.and_then(|entry| stored_value(tiff, entry, order));
        let taken_from_hand = entry.is_some_and(|entry| {
            table
                .tag(entry.tag_id)
                .is_some_and(|tag| tag.omitted.any() && tag.name == row.name)
        });
        let mut display = datetime_typed(engine_row_value(row.value));
        if matches!(&display, TagValue::Binary(_))
            && let Some(TagValue::String(text)) = &stored
        {
            display = TagValue::new_string(text.clone());
        }
        let no_print_conv = match (row.value_conv, row.rational) {
            (Some(value_conv), _) => datetime_typed(engine_row_value(value_conv)),
            (None, Some(fraction)) => rational_value(fraction).unwrap_or_else(|| display.clone()),
            (None, None) => display.clone(),
        };
        rows.rows.push(Row {
            entry_index: *row_entry
                .get(&index)
                .expect("a fenced root row has a root entry"),
            name: row.name,
            display,
            no_print_conv,
            stored,
            priority: if row.low_priority {
                0
            } else {
                SHIM_DEFAULT_PRIORITY
            },
            keeps_priority: taken_from_hand,
            consumed: false,
        });
    }
    rows
}

#[cfg(test)]
pub(crate) fn ifd0_walk(
    tiff: &[u8],
    ifd0: u64,
    order: ByteOrder,
    metadata: &MetadataMap,
) -> Option<DirEngineRows> {
    let mut session = Session::new();
    let mut members = HashMap::new();
    let mut ctx = Ctx::new(&mut members);
    ifd0_walk_with_session(tiff, 0, ifd0, order, metadata, &mut session, &mut ctx)
}

#[cfg(test)]
pub(crate) fn walk(
    table: &'static IfdTable,
    tiff: &[u8],
    ifd_start: u64,
    order: ByteOrder,
    dir: &'static str,
    metadata: &MetadataMap,
) -> DirEngineRows {
    let mut session = Session::new();
    let mut members = HashMap::new();
    let mut ctx = Ctx::new(&mut members);
    walk_with_session(
        table,
        tiff,
        0,
        ifd_start,
        order,
        dir,
        metadata,
        &mut session,
        &mut ctx,
    )
}

/// The value `entry` stores, typed as the hand arm types an ExifIFD entry
/// (`tag_conversion::raw_bytes_to_tag_value`, over the same bytes
/// `ifd_parser::parse_ifd` hands it: the value field for four bytes or
/// fewer, else `value_offset` into the TIFF block). This is what a writer
/// that rebuilds or copies the tag without the original bytes serializes --
/// the SHORT `1` behind `ColorSpace` `sRGB`, the RATIONAL 499038/65536
/// behind `ApertureValue` `14.0`, the bytes behind `Padding`'s placeholder
/// -- and exactly what the hand arm stored before the engine (`None` for a
/// type the hand reader does not size, which it skipped).
fn stored_value(tiff: &[u8], entry: &IfdEntry, order: ByteOrder) -> Option<TagValue> {
    use crate::parsers::common::exif_types::ExifType;
    let size = ExifType::from_u16(entry.field_type)?
        .size_in_bytes()
        .checked_mul(usize::try_from(entry.count).ok()?)?;
    let start = if size <= 4 {
        entry.value_field_pos
    } else {
        usize::try_from(entry.value_offset).ok()?
    };
    let bytes = tiff.get(start..start.checked_add(size)?)?;
    Some(crate::core::tag_conversion::raw_bytes_to_tag_value(
        bytes,
        entry.field_type,
        entry.count,
        entry.tag_id,
        order,
    ))
}

/// Whether ExifTool refuses the directory at `start` of `tiff` that
/// `read_ifd` refused: its entry count or entry array does not fit the block
/// ("Bad $dir directory", Exif.pm:6344-6389: `return 0` for a directory
/// outside a maker note when there is no `RAF` to read the rest from), or it
/// declares no entries (ExifTool's loop then reads nothing). Only the
/// engine's own bound -- more than `MAX_IFD_ENTRIES` entries that do fit --
/// is a refusal ExifTool does not make.
fn directory_refused_as_exiftool(tiff: &[u8], start: usize, order: ByteOrder) -> bool {
    let Some(count) = start
        .checked_add(2)
        .and_then(|end| tiff.get(start..end))
        .map(|bytes| match order {
            ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
        })
    else {
        return true;
    };
    let fits = start + 2 + 12 * usize::from(count) <= tiff.len();
    !(fits && usize::from(count) > MAX_IFD_ENTRIES)
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

/// A string in EXIF's `YYYY:MM:DD HH:MM:SS` shape that is a real date,
/// stored as [`TagValue::DateTime`] -- the type the hand arm gave every such
/// value (`tag_conversion::handle_ascii_type`: `is_datetime_string` +
/// `parse_exif_datetime`), which the output layer prints back as the same
/// 19 characters and which the library's date consumers read
/// (`TagValue::as_datetime`: `date_shift`'s map path, the `-AllDates`
/// shifts of PNG/PDF). Only the type changes, never the text: ExifTool's
/// ConvertDateTime output for DateTimeOriginal/CreateDate is the stored
/// string itself; anything else (a NUL-cut `29 16:13:49`, `?\n`) stays a
/// string, as it did on the hand arm.
fn datetime_typed(value: TagValue) -> TagValue {
    use crate::core::operations_helpers::{is_datetime_string, parse_exif_datetime};
    match &value {
        TagValue::String(text) if is_datetime_string(text) => {
            parse_exif_datetime(text).map_or(value, TagValue::DateTime)
        }
        _ => value,
    }
}

/// An unconverted single rational's `-n` form: the fraction itself, as the
/// hand arm (`raw_bytes_to_tag_value`) stores it. It prints the same number
/// the engine's `RoundFloat` float prints (`-j`/`-n` render a
/// `TagValue::Rational` as its quotient), and it is what Composite inputs
/// read (`TagOccurrence::value_conv`): ExifTool's `Canon::CalcSensorDiag`
/// reads the fraction of FocalPlaneX/YResolution, not its number
/// (`TAG_EXTRA{Rational}`, Canon.pm:10145-10175), and oxidex's port reads
/// it from the `n/d` text of this value (`composite::compute`'s
/// `canon_sensor_diag`). `None` when a part does not fit `TagValue`'s
/// `i32` (the float stays).
fn rational_value((numerator, denominator): (i64, i64)) -> Option<TagValue> {
    Some(TagValue::new_rational(
        i32::try_from(numerator).ok()?,
        i32::try_from(denominator).ok()?,
    ))
}

/// The name the engine reports an ExifIFD entry `id` under -- the plain
/// tag's, or the one reported alternative of its `_variants` group -- when
/// the generated `Exif::Main` reports it at all. The surgical writer maps a
/// raw ExifIFD entry back to the reader's key with it: 178 reported ids
/// have no `tag_db` name, so `lookup_tag_name` spells them `ExifIFD:0xNNNN`
/// while the engine row is `ExifIFD:<name>` (the TIFF/EP FocalPlane ids
/// 0x920e-0x9210 of the Leica M8/M9 among them).
pub(crate) fn exif_main_reported_name(id: u16) -> Option<&'static str> {
    use crate::exiftool_tables::alternative_is_reported;
    use crate::exiftool_tables::ifd_tables::IFD_EXIF_MAIN;
    if !engine_reports(&IFD_EXIF_MAIN, id) {
        return None;
    }
    if let Some(tag) = IFD_EXIF_MAIN.tag(id) {
        return Some(tag.name);
    }
    let mut names = IFD_EXIF_MAIN
        .variant_group(id)?
        .alternatives
        .iter()
        .filter(|(_, tag)| alternative_is_reported(tag))
        .map(|(_, tag)| tag.name);
    let name = names.next()?;
    names.all(|other| other == name).then_some(name)
}

/// Whether `table` gives the plain tag `id` ExifTool's effective priority 0
/// (ExifTool.pm:9469-9473: its own `Priority`, else the table's
/// `PRIORITY`, else 0 for `Avoid`) -- `Emitted::low_priority` for a tag the
/// engine withholds, which a hand residual arm records itself. `false` for
/// an id the static carries no plain tag for.
pub(crate) fn tag_priority_is_zero(table: &IfdTable, id: u16) -> bool {
    table.tag(id).is_some_and(|tag| {
        tag.flags
            .priority
            .or(table.priority)
            .or(if tag.flags.avoid { Some(0) } else { None })
            == Some(0)
    })
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

    /// The withheld ids a generated conversion arm now takes
    /// (Autogeneration v2 mixed mode, `exiftool_tables::conv`): the engine
    /// reports them through the arm, and the hand arm runs only for an entry
    /// the arm declines (`EntryRead::Unread`). 70 of the 80; the other 10
    /// are `conv::exif_main::REFUSED`. Sorted.
    const EXIF_MAIN_GENERATED_TAKES: &[u16] = &[
        0x0002, 0x010f, 0x0110, 0x0131, 0x013b, 0x0153, 0x80a6, 0x87af, 0x87b0, 0x8827, 0x9000,
        0x9101, 0x9201, 0x9206, 0x9209, 0x9216, 0x9286, 0x9290, 0x9291, 0x9292, 0xa000, 0xa20c,
        0xa216, 0xa302, 0xa40d, 0xa40e, 0xa432, 0xbc01, 0xc5e0, 0xc612, 0xc613, 0xc615, 0xc616,
        0xc61b, 0xc61c, 0xc630, 0xc65d, 0xc68b, 0xc6d2, 0xc6d3, 0xc6f3, 0xc6f4, 0xc6f6, 0xc6f8,
        0xc6fa, 0xc6fb, 0xc6fc, 0xc6fe, 0xc716, 0xc717, 0xc718, 0xc71b, 0xc726, 0xc772, 0xc7aa,
        0xcd39, 0xfde8, 0xfde9, 0xfdea, 0xfe4c, 0xfe4d, 0xfe4e, 0xfe51, 0xfe52, 0xfe53, 0xfe54,
        0xfe55, 0xfe56, 0xfe57, 0xfe58,
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
        let taken: Vec<u16> = withheld
            .iter()
            .copied()
            .filter(|&id| crate::exiftool_tables::conv::claims(table, table.tag(id).unwrap()))
            .collect();
        assert_eq!(
            taken, EXIF_MAIN_GENERATED_TAKES,
            "EXIF_MAIN_GENERATED_TAKES: {SNAPSHOT_MOVED}"
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
        let hand_withheld: Vec<u16> = EXIF_MAIN_WITHHELD
            .iter()
            .copied()
            .filter(|id| !EXIF_MAIN_GENERATED_TAKES.contains(id))
            .collect();
        for &id in hand_withheld.iter().chain(EXIF_MAIN_ABSENT) {
            for silence in [false, true] {
                assert_eq!(rows.owner(id, silence), Owner::Hand, "{id:#06x}");
            }
        }
        for &id in EXIF_MAIN_GENERATED_TAKES {
            assert_eq!(rows.owner(id, true), Owner::Engine, "{id:#06x}");
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
            emit.len() + hand_withheld.len() + EXIF_MAIN_EDGES.len() + unknown.len(),
            all_ids(table).len(),
            "engine (incl. generated arms) + hand-withheld + edges + unknown must cover every \
             declared id once"
        );
    }

    #[test]
    fn four_standard_directories_name_every_owner_outcome() {
        let tiff = le_tiff(&[
            ascii(0x010f, b"Canon\0"),
            (0x927c, 7, 4, vec![1, 2, 3, 4]),
            (0x8769, 4, 1, 8u32.to_le_bytes().to_vec()),
        ]);
        for directory in ["IFD0", "IFD1", "ExifIFD", "InteropIFD"] {
            let mut rows = walk(
                &IFD_EXIF_MAIN,
                &tiff,
                8,
                ByteOrder::LittleEndian,
                directory,
                &MetadataMap::new(),
            );
            let mut metadata = MetadataMap::new();
            let key = |name: &str| format!("{directory}:{name}");
            assert_eq!(
                rows.route_entry(
                    0,
                    0x010f,
                    true,
                    false,
                    Owner::Hand,
                    &mut metadata,
                    key,
                    |_, _| true,
                ),
                Owner::Engine,
                "{directory}: physical Make occurrence is Owner::Engine"
            );
            assert_eq!(
                rows.route_entry(
                    1,
                    0x927c,
                    true,
                    false,
                    Owner::Hand,
                    &mut metadata,
                    key,
                    |_, _| true,
                ),
                Owner::Hand,
                "{directory}: physical MakerNote occurrence is Owner::Hand"
            );
            assert_eq!(
                rows.route_entry(
                    2,
                    0x8769,
                    true,
                    false,
                    Owner::Hand,
                    &mut metadata,
                    key,
                    |_, _| true,
                ),
                Owner::Silent,
                "{directory}: physical unrequested parent edge is Owner::Silent"
            );
            assert_eq!(
                metadata.get_string(&format!("{directory}:Make")),
                Some("Canon")
            );
            assert!(metadata.get(&format!("{directory}:MakerNote")).is_none());
            assert!(metadata.get(&format!("{directory}:ExifOffset")).is_none());
        }
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
            rational: None,
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
    fn generated_exif_replay_projects_its_display_and_value_conv_forms() {
        use crate::core::tag_occurrence::ValueChannel;

        let tiff = le_tiff(&[(
            0x829a,
            5,
            1,
            [1u32.to_le_bytes(), 80u32.to_le_bytes()].concat(),
        )]);
        let mut rows = walk(
            &IFD_EXIF_MAIN,
            &tiff,
            8,
            ByteOrder::LittleEndian,
            "ExifIFD",
            &MetadataMap::new(),
        )
        .with_stored_forms();
        let mut metadata = MetadataMap::new();
        assert!(rows.replay(
            0x829a,
            &mut metadata,
            |name| format!("ExifIFD:{name}"),
            |_, _| true,
        ));

        let occurrence = metadata.occurrences_for("ExifIFD:ExposureTime")[0];
        assert_eq!(
            occurrence.project(ValueChannel::PrintConv).as_ref(),
            &TagValue::new_string("1/80")
        );
        assert_eq!(
            occurrence.project(ValueChannel::ValueConv).as_ref(),
            &TagValue::Float(0.0125)
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

    /// K-O (E-2 commit 1, spec 7.1 test 13 on the real table): an
    /// out-of-line value stored entirely BEFORE the directory, past the TIFF
    /// header, is read, as ExifTool reads it (Exif.pm:6549 refuses an
    /// overlap, Exif.pm:6539 an offset below 8; this one, at 8, is neither).
    /// Before K-O the `table_ifd.rs` floor refused it and `replay` reported
    /// `false`.
    #[test]
    fn a_value_stored_before_the_directory_is_read() {
        // Header, then the 8-byte rational at 8, then the IFD at 16.
        let mut tiff = b"II\x2a\0\x10\0\0\0".to_vec();
        tiff.extend([72u32.to_le_bytes(), 1u32.to_le_bytes()].concat());
        tiff.extend(1u16.to_le_bytes());
        tiff.extend(0x011au16.to_le_bytes());
        tiff.extend(5u16.to_le_bytes());
        tiff.extend(1u32.to_le_bytes());
        tiff.extend(8u32.to_le_bytes());
        tiff.extend(0u32.to_le_bytes());
        let walk_as = |tiff: &[u8], dir| {
            walk(
                &IFD_EXIF_MAIN,
                tiff,
                16,
                ByteOrder::LittleEndian,
                dir,
                &MetadataMap::new(),
            )
        };
        for dir in ["InteropIFD", "ExifIFD"] {
            let mut rows = walk_as(&tiff, dir);
            assert_eq!(rows.entries(), Some(1));
            let mut metadata = MetadataMap::new();
            assert!(rows.replay(
                0x011a,
                &mut metadata,
                |name: &str| format!("{dir}:{name}"),
                |_, _| true
            ));
            assert_eq!(
                metadata.get(&format!("{dir}:XResolution")),
                Some(&TagValue::Integer(72)),
                "{dir}"
            );
        }
        // A value that overlaps the entry array is still refused: the
        // rational at 16 starts on the entry count itself.
        tiff[26..30].copy_from_slice(&16u32.to_le_bytes());
        let mut rows = walk_as(&tiff, "ExifIFD");
        assert!(rows.rows.is_empty(), "{:?}", rows.rows);
        assert!(!rows.replay(
            0x011a,
            &mut MetadataMap::new(),
            |name: &str| format!("ExifIFD:{name}"),
            |_, _| true
        ));
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

    /// `keep_hand`: the kept ids answer `Hand`, and their rows are gone for
    /// both `replay` and `drain`.
    #[test]
    fn keep_hand_drops_the_rows_of_the_kept_ids() {
        let tiff = le_tiff(&[short(0xa001, 1), short(0x9207, 5)]);
        let walk_it = || {
            walk(
                &IFD_EXIF_MAIN,
                &tiff,
                8,
                ByteOrder::LittleEndian,
                "ExifIFD",
                &MetadataMap::new(),
            )
        };
        let rows = walk_it();
        assert_eq!(rows.owner(0x9207, false), Owner::Engine);
        let mut rows = walk_it().keep_hand(&[0x9207]);
        assert_eq!(rows.owner(0x9207, false), Owner::Hand);
        assert_eq!(rows.owner(0xa001, false), Owner::Engine);
        let mut metadata = MetadataMap::new();
        let key = |name: &str| format!("ExifIFD:{name}");
        assert!(!rows.replay(0x9207, &mut metadata, key, |_, _| true));
        rows.drain(&mut metadata, key, |_, _| true);
        assert!(metadata.get("ExifIFD:MeteringMode").is_none());
        assert_eq!(metadata.get_string("ExifIFD:ColorSpace"), Some("sRGB"));
    }

    /// `undecoded` tells an absence only the engine makes from one ExifTool
    /// shares. Shared (false): a value read and then withheld by its
    /// conversion (ApertureValue's `2**($val/2)` cannot numify a 0/0
    /// rational), a value past the block ("Bad offset", Exif.pm:6551-6552),
    /// a value overlapping the directory ("Suspicious", Exif.pm:6549), a
    /// directory whose entries run past the block ("Bad ExifIFD directory").
    /// The engine's own (true): an id it never saw, and a directory refused
    /// only for its `MAX_IFD_ENTRIES` bound, which ExifTool does not have.
    #[test]
    fn undecoded_separates_the_engines_own_refusals_from_exiftools() {
        // 0x9202 = 0/0 (decoded, withheld); 0x829a points past the block;
        // 0x829d overlaps the entry array.
        let mut tiff = le_tiff(&[
            (
                0x9202,
                5,
                1,
                [0u32.to_le_bytes(), 0u32.to_le_bytes()].concat(),
            ),
            (
                0x829a,
                5,
                1,
                [1u32.to_le_bytes(), 80u32.to_le_bytes()].concat(),
            ),
        ]);
        tiff.truncate(tiff.len() - 8);
        let rows = walk(
            &IFD_EXIF_MAIN,
            &tiff,
            8,
            ByteOrder::LittleEndian,
            "ExifIFD",
            &MetadataMap::new(),
        );
        assert_eq!(rows.entries(), Some(2));
        // 0/0 reads as the string `undef`, which `2 ** ($val / 2)` numifies
        // to 0: the generated arm reports ExifTool's `1.0`.
        let names: Vec<&str> = rows.rows.iter().map(|row| row.name).collect();
        assert_eq!(names, ["ApertureValue"]);
        assert_eq!(rows.rows[0].display, TagValue::String("1.0".to_string()));
        assert!(!rows.undecoded(0x9202), "decoded");
        assert!(
            !rows.undecoded(0x829a),
            "past the block: ExifTool's Bad offset"
        );
        assert!(rows.undecoded(0xa001), "no such entry");

        // An FNumber whose 8 bytes start inside the entry array.
        let mut overlap = b"II\x2a\0\x08\0\0\0".to_vec();
        overlap.extend(1u16.to_le_bytes());
        overlap.extend([0x9d, 0x82, 5, 0, 1, 0, 0, 0]);
        overlap.extend(12u32.to_le_bytes());
        overlap.extend(0u32.to_le_bytes());
        let rows = walk(
            &IFD_EXIF_MAIN,
            &overlap,
            8,
            ByteOrder::LittleEndian,
            "ExifIFD",
            &MetadataMap::new(),
        );
        assert!(rows.rows.is_empty(), "{:?}", rows.rows);
        assert!(
            !rows.undecoded(0x829d),
            "overlap: ExifTool's Suspicious offset"
        );

        // A directory whose entry array runs past the block: ExifTool's Bad
        // ExifIFD directory, so nothing is the engine's own.
        let refused = walk(
            &IFD_EXIF_MAIN,
            &tiff[..12],
            8,
            ByteOrder::LittleEndian,
            "ExifIFD",
            &MetadataMap::new(),
        );
        assert_eq!(refused.entries(), None);
        assert!(!refused.undecoded(0x9202));

        // 513 entries that fit: refused on the engine's own bound only.
        let mut big = b"II\x2a\0\x08\0\0\0".to_vec();
        big.extend(513u16.to_le_bytes());
        for _ in 0..513 {
            big.extend([0x01, 0xa0, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0]);
        }
        big.extend(0u32.to_le_bytes());
        let bounded = walk(
            &IFD_EXIF_MAIN,
            &big,
            8,
            ByteOrder::LittleEndian,
            "ExifIFD",
            &MetadataMap::new(),
        );
        assert_eq!(bounded.entries(), None);
        assert!(bounded.undecoded(0xa001), "the engine's own bound");
    }

    /// Review finding (E-2, D-2): a `ValueConv` that overflows to infinity
    /// (ApertureValue's `2**($val/2)` on 2147483648/1, CanonEOS20Da.jpg)
    /// would print Rust's `inf` through the compiled `sprintf("%.1f")`
    /// where pinned ExifTool prints `Inf` (crafted `ap_inf.jpg`,
    /// `exiftool-pinned.sh -j -G1 -a -ExifIFD:all`). The walk drops that row
    /// as its own absence, so the caller's hand arm keeps the entry; a
    /// finite value (5/1: `5.7`) is the engine's.
    #[test]
    fn a_non_finite_value_conv_is_the_engines_own_absence() {
        let tiff = le_tiff(&[
            (
                0x9202,
                5,
                1,
                [2_147_483_648u32.to_le_bytes(), 1u32.to_le_bytes()].concat(),
            ),
            (
                0x9205,
                5,
                1,
                [5u32.to_le_bytes(), 1u32.to_le_bytes()].concat(),
            ),
        ]);
        let rows = walk(
            &IFD_EXIF_MAIN,
            &tiff,
            8,
            ByteOrder::LittleEndian,
            "ExifIFD",
            &MetadataMap::new(),
        );
        // The generated arm spells the overflow as Perl does, so the row is
        // kept (Autogeneration v2); only Rust's `inf` spelling is dropped.
        let names: Vec<&str> = rows.rows.iter().map(|row| row.name).collect();
        assert_eq!(names, ["ApertureValue", "MaxApertureValue"]);
        assert_eq!(rows.rows[0].display, TagValue::String("Inf".to_string()));
        assert_eq!(rows.rows[1].display, TagValue::String("5.7".to_string()));
        assert!(!rows.undecoded(0x9202));
        assert!(!rows.undecoded(0x9205));
    }

    /// The `-n` form of an unconverted single rational is its fraction (what
    /// the hand arm stored, and what Canon's sensor-size Composite reads);
    /// a rational with a PrintConv keeps its ValueConv number.
    #[test]
    fn an_unconverted_rational_keeps_its_fraction_as_the_n_form() {
        let tiff = le_tiff(&[
            (
                0xa20e,
                5,
                1,
                [3_072_000u32.to_le_bytes(), 892u32.to_le_bytes()].concat(),
            ),
            (
                0x829a,
                5,
                1,
                [1u32.to_le_bytes(), 80u32.to_le_bytes()].concat(),
            ),
        ]);
        let rows = walk(
            &IFD_EXIF_MAIN,
            &tiff,
            8,
            ByteOrder::LittleEndian,
            "ExifIFD",
            &MetadataMap::new(),
        );
        assert_eq!(rows.rows[0].name, "FocalPlaneXResolution");
        assert_eq!(rows.rows[0].display, TagValue::Float(3443.946188));
        assert_eq!(
            rows.rows[0].no_print_conv,
            TagValue::new_rational(3_072_000, 892)
        );
        assert_eq!(rows.rows[1].name, "ExposureTime");
        assert_eq!(rows.rows[1].display, TagValue::new_string("1/80"));
        assert_eq!(rows.rows[1].no_print_conv, TagValue::Float(0.0125));
    }

    /// Spec 7.1 test 12, for the ExifIFD: the output layer's name-keyed
    /// rules (`format_tag_value_rules`, which every stored value passes on
    /// its way to `-j`) leave every engine-rendered ExifIFD value alone,
    /// except an unconverted zero-denominator rational, which prints
    /// `undef` (0/0) or `inf` (n/0) as ExifTool does. The values are real ones: every ExifIFD the engine walks in
    /// the pinned t/images JPEGs and, when present, [`CORPUS_JPEGS`] from
    /// the pinned corpus.
    #[test]
    fn output_rules_are_a_no_op_on_engine_exif_ifd_values() {
        use crate::parsers::tiff::ifd_parser::ByteOrder as Order;
        let mut paths: Vec<std::path::PathBuf> =
            std::fs::read_dir("/tmp/oxidex-exiftool-cache/exiftool/t/images")
                .map(|dir| {
                    dir.filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.extension().is_some_and(|x| x == "jpg"))
                        .collect()
                })
                .unwrap_or_default();
        // The spec's census-named and semantic-case JPEGs (`slices/exif-ifd/
        // work/named.txt`, `special.txt`), from the pinned corpus.
        let root = std::path::Path::new(crate::test_support::PINNED_CORPUS_ROOT);
        paths.extend(CORPUS_JPEGS.iter().map(|name| root.join(name)));
        paths.sort();
        let mut checked = 0;
        let mut names = std::collections::BTreeSet::new();
        let mut changed = Vec::new();
        for path in &paths {
            let Ok(jpeg) = std::fs::read(path) else {
                continue;
            };
            let Some(tiff) = app1_tiff(&jpeg) else {
                continue;
            };
            let order = match tiff.get(..2) {
                Some(b"II") => Order::LittleEndian,
                Some(b"MM") => Order::BigEndian,
                _ => continue,
            };
            let Some(exif) = exif_ifd_offset(&tiff, order) else {
                continue;
            };
            let rows = walk(
                &IFD_EXIF_MAIN,
                &tiff,
                exif,
                order,
                "ExifIFD",
                &MetadataMap::new(),
            );
            for row in &rows.rows {
                let key = format!("ExifIFD:{}", row.name);
                let shown =
                    crate::core::exiftool_compat::format_tag_value_rules(&key, &row.display);
                checked += 1;
                names.insert(row.name);
                // A zero-denominator rational the engine left unconverted is
                // printed by the output layer as ExifTool prints it:
                // `undef` for 0/0, `inf` for n/0 (pinned 13.59 on
                // SamsungAnycallSPH-A503.jpg's ExposureTime 1/0: `inf`).
                let zero_denominator = match row.display {
                    TagValue::Rational {
                        numerator,
                        denominator: 0,
                    } => Some(if numerator == 0 { "undef" } else { "inf" }),
                    _ => None,
                };
                if let Some(want) = zero_denominator {
                    if shown != TagValue::new_string(want) {
                        changed.push(format!("{}: {key} n/0 -> {shown:?}", path.display()));
                    }
                } else if shown != row.display {
                    changed.push(format!(
                        "{}: {key} {:?} -> {shown:?}",
                        path.display(),
                        row.display
                    ));
                }
            }
        }
        if checked == 0 {
            eprintln!("skipping: no pinned t/images or corpus JPEGs on this machine");
            return;
        }
        assert!(
            changed.is_empty(),
            "{} re-converted: {changed:#?}",
            changed.len()
        );
        eprintln!("{checked} engine ExifIFD values, {} names", names.len());
    }

    /// Slice v2-ifd0, the IFD0 counterpart of the test above: the output
    /// layer's name-keyed rules leave every engine-rendered IFD0 value
    /// alone (bar an unconverted zero-denominator rational, printed `undef`
    /// / `inf` as ExifTool prints it). Real values: IFD0 of every pinned
    /// t/images JPEG and of [`CORPUS_JPEGS`], walked by [`ifd0_walk`].
    #[test]
    fn output_rules_are_a_no_op_on_engine_ifd0_values() {
        let mut paths: Vec<std::path::PathBuf> =
            std::fs::read_dir("/tmp/oxidex-exiftool-cache/exiftool/t/images")
                .map(|dir| {
                    dir.filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|p| p.extension().is_some_and(|x| x == "jpg"))
                        .collect()
                })
                .unwrap_or_default();
        let root = std::path::Path::new(crate::test_support::PINNED_CORPUS_ROOT);
        paths.extend(CORPUS_JPEGS.iter().map(|name| root.join(name)));
        paths.sort();
        let mut checked = 0;
        let mut names = std::collections::BTreeSet::new();
        let mut changed = Vec::new();
        for path in &paths {
            let Ok(jpeg) = std::fs::read(path) else {
                continue;
            };
            let Some(tiff) = app1_tiff(&jpeg) else {
                continue;
            };
            let order = match tiff.get(..2) {
                Some(b"II") => ByteOrder::LittleEndian,
                Some(b"MM") => ByteOrder::BigEndian,
                _ => continue,
            };
            let word = |at: usize| -> Option<u32> {
                let b: [u8; 4] = tiff.get(at..at + 4)?.try_into().ok()?;
                Some(match order {
                    ByteOrder::LittleEndian => u32::from_le_bytes(b),
                    ByteOrder::BigEndian => u32::from_be_bytes(b),
                })
            };
            let Some(ifd0) = word(4) else {
                continue;
            };
            let Some(rows) = ifd0_walk(&tiff, u64::from(ifd0), order, &MetadataMap::new()) else {
                eprintln!("skipping: Exif::Main is not in force");
                return;
            };
            for row in rows.rows.iter().filter(|row| !row.consumed) {
                let key = format!("IFD0:{}", row.name);
                let shown =
                    crate::core::exiftool_compat::format_tag_value_rules(&key, &row.display);
                checked += 1;
                names.insert(row.name);
                let zero_denominator = match row.display {
                    TagValue::Rational {
                        numerator,
                        denominator: 0,
                    } => Some(if numerator == 0 { "undef" } else { "inf" }),
                    _ => None,
                };
                if let Some(want) = zero_denominator {
                    if shown != TagValue::new_string(want) {
                        changed.push(format!("{}: {key} n/0 -> {shown:?}", path.display()));
                    }
                } else if shown != row.display {
                    changed.push(format!(
                        "{}: {key} {:?} -> {shown:?}",
                        path.display(),
                        row.display
                    ));
                }
            }
        }
        if checked == 0 {
            eprintln!("skipping: no pinned t/images or corpus JPEGs on this machine");
            return;
        }
        assert!(
            changed.is_empty(),
            "{} re-converted: {changed:#?}",
            changed.len()
        );
        eprintln!("{checked} engine IFD0 values, {} names", names.len());
    }

    const CORPUS_JPEGS: &[&str] = &[
        "Apple/Apple_iPadPro10.5.jpg",
        "Apple/Apple_iPhone6.jpg",
        "Canon/CanonCanoScanFB630U.jpg",
        "Canon/CanonEOS40D.jpg",
        "Canon/CanonEOS60D.jpg",
        "Canon/CanonEOS_REBEL_T5i.jpg",
        "Canon/CanonHG20.jpg",
        "Canon/CanonIXY640.jpg",
        "Canon/CanonPowerShotELPH330HS.jpg",
        "Canon/CanonXL_H1.jpg",
        "DJI/DJI_FC300X.jpg",
        "DJI/DJI_XT2.jpg",
        "FujiFilm/FujiFilmDS-10.jpg",
        "FujiFilm/FujiFilmFinePixHS35EXR.jpg",
        "FujiFilm/FujiFilmFinePixZ100fd.jpg",
        "GoPro/GoProHD2.jpg",
        "GoPro/GoProHERO10Black.jpg",
        "Google/GoogleNexusS.jpg",
        "Leica/LeicaM8.jpg",
        "Leica/LeicaM9.jpg",
        "Leica/LeicaS_Typ007.jpg",
        "Nikon/NikonCoolpix7900.jpg",
        "Nikon/NikonCoolpixS710.jpg",
        "Nikon/NikonSUPER_COOLSCAN4000ED.jpg",
        "Olympus/OlympusE-M10MarkIV.jpg",
        "Panasonic/PanasonicDMC-F7.jpg",
        "Panasonic/PanasonicDMC-ZS19.jpg",
        "Samsung/SamsungAnycallSPH-A503.jpg",
        "Samsung/SamsungAnycallSPH-B6650.jpg",
        "Samsung/SamsungDigimax220SE.jpg",
        "Samsung/SamsungDigimaxS500.jpg",
        "Samsung/SamsungGT-B2710.jpg",
        "Samsung/SamsungGT-S5250.jpg",
        "Samsung/SamsungGalaxyA55_5G.jpg",
        "Samsung/SamsungHMX-H300.jpg",
        "Samsung/SamsungNX3000.jpg",
        "Samsung/SamsungSGH-D980.jpg",
        "Samsung/SamsungSM-T800.jpg",
        "Samsung/SamsungSPH-A800.jpg",
        "Samsung/SamsungVP-D73.jpg",
        "Sigma.jpg",
        "Sony/SonyDCR-DVD201E.jpg",
        "Sony/SonyDPP-FP60.jpg",
        "Sony/SonyILCE-6100.jpg",
        "Sony/SonyILCE-7CM2.jpg",
        "Sony/SonyILCE-7M4.jpg",
        "Sony/SonyILME-FX3.jpg",
        "Sony/SonyMVC-CD1000.jpg",
    ];

    /// The TIFF block of a JPEG's first `Exif\0\0` APP1 segment.
    fn app1_tiff(jpeg: &[u8]) -> Option<Vec<u8>> {
        let mut at = 2;
        while at + 4 <= jpeg.len() && jpeg[at] == 0xff {
            let marker = jpeg[at + 1];
            let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
            let body = jpeg.get(at + 4..at + 2 + len)?;
            if marker == 0xe1 && body.starts_with(b"Exif\0\0") {
                return Some(body[6..].to_vec());
            }
            if marker == 0xda {
                return None;
            }
            at += 2 + len;
        }
        None
    }

    /// IFD0's 0x8769 ExifOffset in a TIFF block.
    fn exif_ifd_offset(tiff: &[u8], order: ByteOrder) -> Option<u64> {
        let io = order.to_io_byte_order();
        let word = |at: usize| -> Option<u32> {
            let b: [u8; 4] = tiff.get(at..at + 4)?.try_into().ok()?;
            Some(match order {
                ByteOrder::LittleEndian => u32::from_le_bytes(b),
                ByteOrder::BigEndian => u32::from_be_bytes(b),
            })
        };
        let ifd0 = usize::try_from(word(4)?).ok()?;
        read_ifd(tiff, ifd0, io)?
            .into_iter()
            .find(|entry| entry.tag_id == 0x8769)
            .map(|entry| u64::from(entry.value_offset))
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
