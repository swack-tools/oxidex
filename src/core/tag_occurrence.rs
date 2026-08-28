//! `TagOccurrence` — Step 18, Phase A of the tag-machinery overhaul.
//!
//! This is the record ExifTool actually keeps per extracted tag, modelled on
//! `FoundTag` (`ExifTool.pm:9448`+ in the pinned 13.59 tree). Today's
//! `MetadataMap` collapses every occurrence of a key down to a single
//! `TagValue` the instant a second `insert()` call overwrites the first; this
//! type is the additive scaffolding that lets a future step retain what that
//! overwrite throws away, without changing what any of today's ~4,034
//! `insert()` call sites observe. See `OVERHAUL_STEP18_DESIGN.md`.
//!
//! Phase A never constructs one of these by hand from a real parser -- every
//! occurrence in the tree right now is minted by [`TagOccurrence::from_insert_shim`],
//! the thin wrapper behind `MetadataMap::insert()`. Phase B/C (Step 19+) is
//! where parsers start attaching real groups, priorities and provenance.

use super::tag_value::TagValue;
use oxidex_tags::TagId;
use std::sync::Arc;

/// An interned group/tag-name string.
///
/// Aliased rather than newtyped: every place this appears (`group0`,
/// `group1`, `group2`, and the pool [`intern`] draws from) wants exactly
/// `Arc<str>`'s behavior -- cheap clone, `Deref<Target = str>`, structural
/// equality -- and nothing else.
pub type Group = Arc<str>;

/// Per-instance identity within a multi-document or multi-track file (a
/// QuickTime track, a multi-page TIFF, an embedded sub-document). Defaults to
/// `0`, meaning "the main document" / "no sub-instance" -- which is what
/// every shim-minted occurrence gets today, since `insert()` call sites have
/// no notion of instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Instance(pub u32);

/// Where an occurrence's bytes came from: which parser module, which
/// transcribed table, which byte range of the source file. Entirely absent
/// for shim-minted occurrences -- `insert()` callers never had this
/// information to give, and Phase A does not go hunting for it. Populating
/// this is Phase B/C work, one migrated call site at a time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Provenance {
    pub module: Option<&'static str>,
    pub table: Option<&'static str>,
    pub byte_range: Option<std::ops::Range<u64>>,
}

/// The priority every occurrence minted through the `insert()` shim carries.
///
/// ExifTool's `FoundTag` defaults an ordinary tag's priority to `1` --
/// "the normal default" (`ExifTool.pm:9562`) -- unless the tag config sets
/// `Priority`, `Avoid`, or the enclosing table sets `PRIORITY`/
/// `LOW_PRIORITY_DIR`, none of which the shim has visibility into. Using the
/// same default here means every shim occurrence for a given key ties on
/// priority, which is exactly the property Phase A's zero-behavior-change
/// invariant relies on (see [`TagSink::record`]).
pub const SHIM_DEFAULT_PRIORITY: u8 = 1;

/// The table `PRIORITY` for a shim-minted occurrence, keyed on its family-0
/// group -- the one piece of "enclosing table" configuration the shim *can*
/// see, because the XMP parsers encode the schema namespace into the key's
/// group prefix.
///
/// ExifTool demotes exactly three XMP schema tables below ordinary tags:
///
/// ```text
/// XMP.pm:1900    PRIORITY => 0, # not as reliable as actual TIFF tags
/// XMP.pm:1992    PRIORITY => 0, # not as reliable as actual EXIF tags
/// XMP.pm:2462    PRIORITY => 0, # not as reliable as actual EXIF tags
/// ```
///
/// (`%Image::ExifTool::XMP::tiff`, `::exif` and `::exifEX` respectively.)
/// Without this, an `XMP-exif:FocalLength` recorded after the EXIF IFD's own
/// `ExifIFD:FocalLength` ties it on priority and -- by `FoundTag`'s
/// newer-wins tiebreak -- takes the bare `FocalLength` key away from it, so
/// every composite that Requires `FocalLength` consumes the XMP packet's
/// PrintConv-rounded copy ("11.1 mm") instead of the EXIF rational's
/// ValueConv (11.109). `BuildCompositeTags` hands `@val` the post-ValueConv
/// store of the *priority winner* (ExifTool.pm:4008), and with these tables
/// at `PRIORITY => 0` that winner is the EXIF tag whenever one exists --
/// which is precisely ExifTool's observed `-G1 -s -FocalLength` answer.
fn shim_group_priority(group0: &str) -> u8 {
    match group0 {
        "XMP-tiff" | "XMP-exif" | "XMP-exifEX" => 0,
        _ => SHIM_DEFAULT_PRIORITY,
    }
}

/// A single extracted tag occurrence, in the shape ExifTool's `FoundTag`
/// actually tracks: a value in up to three forms, a priority, a file-order
/// position, and group/instance identity -- rather than the one `TagValue`
/// today's `MetadataMap` keeps per key.
#[derive(Debug, Clone, PartialEq)]
pub struct TagOccurrence {
    /// Canonical numeric/table identity where one exists. Shim-minted
    /// occurrences have no table to consult, so this is `TagId::Named` of
    /// the full lookup key (e.g. `"EXIF:Make"`) -- a real identity, just not
    /// yet the canonical one a migrated parser would attach.
    pub id: TagId,
    /// The tag's own name, interned. For a shim-minted occurrence this is
    /// whatever followed the first `:` in the lookup key (or the whole key,
    /// if there was no `:`).
    pub name: Group,
    /// Family 0 group (EXIF, File, XMP, MakerNotes, Composite, ...). For a
    /// shim-minted occurrence this is whatever preceded the first `:` in the
    /// lookup key, or the interned empty string if the key had none.
    pub group0: Group,
    /// Family 1 group (IFD0, Track3, System, ICC-header, ...). Unknown to
    /// the shim, always the interned empty string in Phase A.
    pub group1: Group,
    /// Family 2 group, where known. Always `None` in Phase A.
    pub group2: Option<Group>,
    /// Per-instance (sub-document/track) identity. Always the default
    /// (`Instance(0)`) in Phase A.
    pub instance: Instance,
    /// Pre-conversion form. For a shim-minted occurrence this is simply
    /// whatever `TagValue` was passed to `insert()` -- which, for the
    /// `format!`-before-store parsers this step's report measures, is
    /// already the *print* form, mislabeled. See the report's
    /// format-before-store count for how many storage sites that affects.
    pub raw: TagValue,
    /// `ValueConv` form. Always `None` for shim-minted occurrences -- the
    /// shim has no way to distinguish it from `raw`.
    pub value: Option<TagValue>,
    /// `PrintConv` form. Always `None` for shim-minted occurrences, for the
    /// same reason as `value`.
    pub print: Option<TagValue>,
    /// `FoundTag`'s `Priority` (`ExifTool.pm:9539`+): higher wins, ties
    /// broken by file order. See [`SHIM_DEFAULT_PRIORITY`] for what
    /// shim-minted occurrences get and why.
    pub priority: u8,
    /// Whether this occurrence belongs to a `List`-type tag. Always `false`
    /// for shim-minted occurrences -- `insert()` has no such concept.
    pub is_list: bool,
    /// Position in file order: the tiebreak, matching `FoundTag`'s
    /// `FILE_ORDER` / `NUM_FOUND` (`ExifTool.pm:9563`). Assigned as
    /// `TagSink::next_order()` at record time, so it is dense and strictly
    /// increasing across every occurrence ever recorded into one sink.
    pub order: u32,
    /// Module, table and byte-range provenance. Always the default (all
    /// `None`) for shim-minted occurrences.
    pub origin: Provenance,
}

impl TagOccurrence {
    /// Mints an occurrence from a `MetadataMap::insert()` call site.
    ///
    /// This is the Phase-A migration shim described in
    /// `OVERHAUL_STEP18_DESIGN.md` §2.2: it is what makes the ~4,034
    /// `insert()` call sites tractable without touching a single one of
    /// them. Every field this constructor cannot honestly derive from a bare
    /// `(key, value)` pair is left at its most conservative default rather
    /// than guessed at.
    pub(crate) fn from_insert_shim(key: &str, value: TagValue, order: u32) -> Self {
        let (group0, name) = match key.split_once(':') {
            Some((g, n)) => (intern(g), intern(n)),
            None => (intern(""), intern(key)),
        };
        let priority = shim_group_priority(&group0);
        TagOccurrence {
            id: TagId::Named(key.to_string()),
            name,
            group0,
            group1: intern(""),
            group2: None,
            instance: Instance::default(),
            raw: value,
            value: None,
            print: None,
            priority,
            is_list: false,
            order,
            origin: Provenance::default(),
        }
    }

    /// Reconstructs the lookup key this occurrence was recorded under:
    /// `"{group0}:{name}"`, or bare `name` when `group0` is empty. The exact
    /// inverse of the split `from_insert_shim` performs, and equally exact
    /// for Step 19's non-shim call sites -- none of them ever puts a colon
    /// in `group0`. Used wherever an occurrence needs to be re-recorded
    /// under its own key without the caller having to have kept the
    /// original string around (`MetadataMap::merge`, in particular).
    pub(crate) fn lookup_key(&self) -> String {
        if self.group0.is_empty() {
            self.name.to_string()
        } else {
            format!("{}:{}", self.group0, self.name)
        }
    }
}

/// A process-wide interner for tag/group names.
///
/// Backed by a mutex-guarded `HashSet<Arc<str>>` rather than anything
/// lock-free: Phase A's call volume (per-insert, not per-byte) does not
/// justify the complexity, and every existing hot path in this crate that
/// needs lock-free sharing already avoids going through `MetadataMap` at
/// all. Covers both `&'static` names from generated tables and names
/// discovered at parse time (XMP properties), per design decision D2 --
/// there is deliberately no separate code path for the two.
mod interner {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex, OnceLock};

    static POOL: OnceLock<Mutex<HashSet<Arc<str>>>> = OnceLock::new();

    pub(crate) fn intern(s: &str) -> Arc<str> {
        let pool = POOL.get_or_init(|| Mutex::new(HashSet::new()));
        let mut guard = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = guard.get(s) {
            return existing.clone();
        }
        let arc: Arc<str> = Arc::from(s);
        guard.insert(arc.clone());
        arc
    }
}

pub(crate) use interner::intern;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_the_same_text_twice_yields_the_same_allocation() {
        let a = intern("EXIF:Make");
        let b = intern("EXIF:Make");
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn from_insert_shim_splits_on_the_first_colon() {
        let occ = TagOccurrence::from_insert_shim("EXIF:Make", TagValue::new_string("Canon"), 3);
        assert_eq!(&*occ.group0, "EXIF");
        assert_eq!(&*occ.name, "Make");
        assert_eq!(occ.order, 3);
        assert_eq!(occ.priority, SHIM_DEFAULT_PRIORITY);
        assert_eq!(occ.raw, TagValue::new_string("Canon"));
        assert!(occ.value.is_none());
        assert!(occ.print.is_none());
    }

    #[test]
    fn from_insert_shim_tolerates_a_colonless_key() {
        let occ = TagOccurrence::from_insert_shim("Comment", TagValue::new_string("hi"), 0);
        assert_eq!(&*occ.group0, "");
        assert_eq!(&*occ.name, "Comment");
    }

    #[test]
    fn a_key_with_two_colons_only_splits_on_the_first() {
        // XMP-exif:FocalPlaneXResolution-style keys are single-colon in
        // practice, but a hypothetical "Group:Sub:Name" key should still
        // treat everything after the first colon as the name -- there is no
        // group1 encoded in today's key strings for the shim to find.
        let occ = TagOccurrence::from_insert_shim("A:B:C", TagValue::new_string("v"), 0);
        assert_eq!(&*occ.group0, "A");
        assert_eq!(&*occ.name, "B:C");
    }
}
