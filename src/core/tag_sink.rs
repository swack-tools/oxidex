//! `TagSink` — Step 18, Phase A of the tag-machinery overhaul.
//!
//! Accumulates [`TagOccurrence`]s in file order and maintains a winner
//! projection over them: for each lookup key, which occurrence is currently
//! "the" value for that key, in the sense `MetadataMap::get()` means it
//! today. This is decision D1 from `OVERHAUL_STEP18_DESIGN.md`: a
//! `Vec<TagOccurrence>` (file order is intrinsic to the vec, needing no
//! bookkeeping) plus an index kept alongside it for `O(1)` lookup, rather
//! than an `IndexMap`.
//!
//! The winner projection is maintained incrementally in [`TagSink::record`]
//! using the same rule ExifTool's `FoundTag` applies per call
//! (`ExifTool.pm:9541-9564`): a newly-recorded occurrence displaces the
//! current winner for its key whenever `new.priority >= existing.priority`,
//! except that an existing `Priority => 0` winner is first promoted to `1`
//! for the comparison (so two `Priority => 0` arrivals tie in the *first*
//! arrival's favor), and an occurrence recorded for a sub-document/track
//! `Instance` never displaces a winner recorded under a *different*
//! instance. See [`TagSink::record`]'s own doc comment for the full citation
//! and worked examples. Losing occurrences are retained in `occurrences` --
//! reachable later via [`TagSink::occurrences`] -- but the projection (and
//! therefore `MetadataMap`'s default view) never surfaces them; that is
//! Step 20+'s `-a`/`-G*` output-mode work.
//!
//! This module intentionally maintains the index eagerly on every `record`
//! call rather than lazily rebuilding it on first access after invalidation.
//! Both satisfy D1's "Vec plus an index" shape; eager maintenance was chosen
//! here because it lets `MetadataMap::iter()`/`keys()` keep returning
//! `&String` (borrowed from the index's own owned keys) without a `RefCell`
//! or unsafe self-reference, which matters because several existing call
//! sites (e.g. `src/core/operations.rs`, `src/bin/tag-comparison/...`) name
//! `Vec<(&String, &TagValue)>` explicitly. If a benchmark later shows this
//! is a hot path, D1 explicitly leaves room to revisit the choice.

use super::tag_occurrence::{Instance, TagOccurrence};
use super::tag_value::TagValue;
use std::collections::HashMap;
use std::collections::hash_map::Entry;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TagSink {
    /// Every occurrence ever recorded, in file order. Index `i` was recorded
    /// at `order == i` (as a `u32`).
    occurrences: Vec<TagOccurrence>,
    /// Winner projection: lookup key -> index into `occurrences` of the
    /// occurrence that currently wins that key.
    winners: HashMap<String, usize>,
}

impl TagSink {
    pub fn new() -> Self {
        Self {
            occurrences: Vec::new(),
            winners: HashMap::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            occurrences: Vec::with_capacity(capacity),
            winners: HashMap::with_capacity(capacity),
        }
    }

    /// The file-order position the next recorded occurrence should carry.
    pub fn next_order(&self) -> u32 {
        self.occurrences.len() as u32
    }

    /// Records `occurrence` under `key`, updating the winner projection.
    ///
    /// Matches `FoundTag`'s duplicate-handling rule at `ExifTool.pm:9564`:
    ///
    /// ```perl
    /// if ($priority >= $oldPriority and (not $$self{DOC_NUM} or ...) and ...) {
    ///     # move existing tag out of the way since this tag is higher priority
    ///     ...
    /// } else {
    ///     $tag = $nextTag;        # don't override the existing tag
    /// }
    /// ```
    ///
    /// Two rules combine here, both against `ExifTool.pm:9541-9564`:
    ///
    /// 1. **Priority-0 promotion** (`:9541-9551`, "promote existing
    ///    0-priority tag so it takes precedence over a new 0-tag"): when the
    ///    *existing* winner's own priority is `0`, the comparison treats it
    ///    as `1` instead. Combined with `:9564`'s `>=`, this makes a
    ///    `Priority => 0` tag (e.g. JPEG `Comment`, `ExifTool.pm:1311-1315`
    ///    "to preserve order of JPEG COM segments") **never** displace a
    ///    same-priority incumbent -- first arrival wins -- while a normal
    ///    (priority-1) arrival still displaces a `0`-priority one on either
    ///    side of it, and two ordinary priority-1 arrivals still tie to the
    ///    newest (Step 18's original, still-correct rule for the common
    ///    case).
    /// 2. **The `DOC_NUM` guard** (the `(not $$self{DOC_NUM} or ...)` half
    ///    of `:9564`'s `and`): an occurrence recorded for a sub-document/
    ///    track instance (`Instance` other than the main-document default)
    ///    never displaces a winner recorded under a *different* instance,
    ///    regardless of priority. This is why `CanonRaw.cr3`'s four `tkhd`
    ///    copies each keep their own `TrackID`, `TrackDuration`, etc., but
    ///    only `Track1`'s stays under the bare key: every later track's
    ///    normal-priority `TrackID` still loses to `Track1`'s, because
    ///    `Track2`/`Track3`/`Track4` are a different instance than the
    ///    incumbent. Main-document occurrences (`Instance`'s `Default`, `0`)
    ///    are unaffected and fall straight through to rule 1.
    ///
    /// This intentionally does not reproduce the rarer combination the full
    /// Perl condition allows for -- a `Priority => 0` *main-document* tag
    /// arriving after an existing `Priority => 0` *sub-document* tag stays
    /// unpromoted (`:9546-9550`'s `else` branch) so the later main-document
    /// tag can still win outright. None of Step 19's exemplar families
    /// combine `Priority => 0` with a non-default `Instance` on the same
    /// key, so the simpler always-promote rule above is exact for every
    /// occurrence recorded so far; the narrower case is left for whichever
    /// later step first needs it.
    pub fn record(&mut self, key: String, occurrence: TagOccurrence) {
        let idx = self.occurrences.len();
        let new_priority = occurrence.priority;
        let new_instance = occurrence.instance;
        self.occurrences.push(occurrence);
        match self.winners.entry(key) {
            Entry::Occupied(mut e) => {
                let existing_idx = *e.get();
                let existing = &self.occurrences[existing_idx];
                // ExifTool.pm:9541-9551.
                let effective_old_priority = if existing.priority == 0 {
                    1
                } else {
                    existing.priority
                };
                // ExifTool.pm:9564's `(not $$self{DOC_NUM} or ...)`.
                let instance_ok =
                    new_instance == Instance::default() || new_instance == existing.instance;
                if new_priority >= effective_old_priority && instance_ok {
                    e.insert(idx);
                }
            }
            Entry::Vacant(e) => {
                e.insert(idx);
            }
        }
    }

    pub fn get(&self, key: &str) -> Option<&TagValue> {
        self.winners.get(key).map(|&idx| &self.occurrences[idx].raw)
    }

    /// The current winner occurrence for `key`, not just its `raw` value.
    ///
    /// Step 20's output-projection work (`-a`/`-G*`/`--no-print-conv`) needs
    /// the whole occurrence -- `priority`, `group0`/`group1`, `order`, and
    /// the `value` slot a parser may have attached via
    /// [`super::metadata_map::MetadataMap::insert_occurrence_with_raw`] --
    /// not just the flattened display value `get()` returns.
    pub fn winner_occurrence(&self, key: &str) -> Option<&TagOccurrence> {
        self.winners.get(key).map(|&idx| &self.occurrences[idx])
    }

    /// Every key's current winner, paired with its full occurrence. The
    /// occurrence-aware counterpart to [`TagSink::iter`].
    pub fn winner_occurrences(&self) -> impl Iterator<Item = (&String, &TagOccurrence)> {
        self.winners
            .iter()
            .map(move |(k, &idx)| (k, &self.occurrences[idx]))
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut TagValue> {
        let idx = *self.winners.get(key)?;
        Some(&mut self.occurrences[idx].raw)
    }

    /// Removes `key` from the winner projection. The occurrence itself stays
    /// physically in `occurrences` (it is simply no longer anyone's winner)
    /// so that other occurrences' indices remain valid.
    pub fn remove(&mut self, key: &str) -> Option<TagValue> {
        let idx = self.winners.remove(key)?;
        Some(self.occurrences[idx].raw.clone())
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.winners.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.winners.len()
    }

    pub fn is_empty(&self) -> bool {
        self.winners.is_empty()
    }

    pub fn clear(&mut self) {
        self.occurrences.clear();
        self.winners.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &TagValue)> {
        self.winners
            .iter()
            .map(move |(k, &idx)| (k, &self.occurrences[idx].raw))
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.winners.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &TagValue> {
        self.winners
            .values()
            .map(move |&idx| &self.occurrences[idx].raw)
    }

    /// Consumes the sink, returning only the winner projection as a plain
    /// map -- the shape `MetadataMap::into_iter()`/`merge()` need. Losing
    /// occurrences are dropped here; Phase A has nothing that reads them
    /// across a consuming boundary yet.
    pub fn into_winner_map(self) -> HashMap<String, TagValue> {
        let TagSink {
            mut occurrences,
            winners,
        } = self;
        let mut out = HashMap::with_capacity(winners.len());
        for (key, idx) in winners {
            let value = std::mem::replace(
                &mut occurrences[idx].raw,
                TagValue::new_string(String::new()),
            );
            out.insert(key, value);
        }
        out
    }

    /// Every occurrence recorded so far, winners and losers alike, in file
    /// order. Nothing in Phase A reads a loser through this -- it exists for
    /// the `FoundTag`-parity unit tests and for Step 19's real
    /// duplicate-retention work to build on.
    pub fn occurrences(&self) -> &[TagOccurrence] {
        &self.occurrences
    }

    /// Consumes the sink, returning every occurrence ever recorded --
    /// winners and losers alike -- in file order.
    ///
    /// Unlike [`TagSink::into_winner_map`], nothing is dropped and nothing
    /// is flattened to a bare value: each occurrence keeps its own
    /// priority, family-1 group and instance. This is what
    /// [`MetadataMap::merge`](super::metadata_map::MetadataMap::merge) uses
    /// so a parser's real duplicate retention survives crossing a merge
    /// boundary instead of being silently collapsed to a single
    /// `SHIM_DEFAULT_PRIORITY` winner -- exactly the shape every multi-stage
    /// parser (JPEG's segment pipeline, in particular) uses.
    pub fn into_occurrences(self) -> Vec<TagOccurrence> {
        self.occurrences
    }

    /// Re-records `occurrence` into this sink under its own
    /// [`TagOccurrence::lookup_key`], carrying its priority, family-1 group
    /// and instance over unchanged -- only `order` is reassigned, to this
    /// sink's own [`TagSink::next_order`], so it still fits this sink's
    /// monotonic file-order sequence rather than colliding with (or
    /// stale-ordering against) whatever this sink already holds.
    ///
    /// This is [`TagSink::record`] plus the bookkeeping
    /// [`MetadataMap::merge`](super::metadata_map::MetadataMap::merge)
    /// would otherwise have to repeat at every call site: replaying a whole
    /// sub-sink's occurrences one at a time through this reproduces that
    /// sub-sink's own winner exactly (the tie-break rule is deterministic
    /// over a fixed relative order), while every occurrence -- winner and
    /// loser alike -- ends up retained in the target sink too.
    pub fn record_carrying_over(&mut self, mut occurrence: TagOccurrence) {
        let key = occurrence.lookup_key();
        occurrence.order = self.next_order();
        self.record(key, occurrence);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_occurrence::TagOccurrence;

    fn occ(value: &str, priority: u8, order: u32) -> TagOccurrence {
        TagOccurrence {
            priority,
            order,
            ..TagOccurrence::from_insert_shim("EXIF:Make", TagValue::new_string(value), order)
        }
    }

    fn occ_with_instance(
        value: &str,
        priority: u8,
        order: u32,
        instance: Instance,
    ) -> TagOccurrence {
        TagOccurrence {
            instance,
            ..occ(value, priority, order)
        }
    }

    #[test]
    fn equal_priority_ties_go_to_the_most_recent_arrival() {
        // Matches FoundTag's `$priority >= $oldPriority` (ExifTool.pm:9564):
        // on a tie the *newer* occurrence displaces the older one.
        let mut sink = TagSink::new();
        sink.record("EXIF:Make".to_string(), occ("first", 1, 0));
        sink.record("EXIF:Make".to_string(), occ("second", 1, 1));
        assert_eq!(sink.get("EXIF:Make"), Some(&TagValue::new_string("second")));
    }

    #[test]
    fn a_strictly_higher_priority_arrival_always_wins() {
        let mut sink = TagSink::new();
        sink.record("EXIF:Make".to_string(), occ("low", 1, 0));
        sink.record("EXIF:Make".to_string(), occ("high", 5, 1));
        sink.record("EXIF:Make".to_string(), occ("low-again", 1, 2));
        assert_eq!(sink.get("EXIF:Make"), Some(&TagValue::new_string("high")));
    }

    #[test]
    fn a_strictly_lower_priority_arrival_never_displaces_the_winner() {
        let mut sink = TagSink::new();
        sink.record("EXIF:Make".to_string(), occ("high", 5, 0));
        sink.record("EXIF:Make".to_string(), occ("low", 1, 1));
        assert_eq!(sink.get("EXIF:Make"), Some(&TagValue::new_string("high")));
    }

    #[test]
    fn losers_are_retained_but_not_projected() {
        let mut sink = TagSink::new();
        sink.record("EXIF:Make".to_string(), occ("first", 1, 0));
        sink.record("EXIF:Make".to_string(), occ("second", 1, 1));
        assert_eq!(
            sink.occurrences().len(),
            2,
            "both occurrences kept in file order"
        );
        assert_eq!(sink.len(), 1, "but exactly one wins the projection");
    }

    #[test]
    fn two_priority_zero_arrivals_tie_in_favor_of_the_first() {
        // ExifTool.pm:9541-9551: an existing 0-priority winner is promoted
        // to 1 before the comparison, so a second 0-priority arrival's
        // `0 >= 1` is false. This is JPEG COM's `Priority => 0`
        // (ExifTool.pm:1311-1315) -- the first comment must win.
        let mut sink = TagSink::new();
        sink.record("File:Comment".to_string(), occ("first comment", 0, 0));
        sink.record("File:Comment".to_string(), occ("second comment", 0, 1));
        assert_eq!(
            sink.get("File:Comment"),
            Some(&TagValue::new_string("first comment"))
        );
        assert_eq!(sink.occurrences().len(), 2, "the loser is still retained");
    }

    #[test]
    fn a_normal_priority_arrival_displaces_a_priority_zero_incumbent() {
        let mut sink = TagSink::new();
        sink.record("File:Comment".to_string(), occ("low", 0, 0));
        sink.record("File:Comment".to_string(), occ("normal", 1, 1));
        assert_eq!(
            sink.get("File:Comment"),
            Some(&TagValue::new_string("normal"))
        );
    }

    #[test]
    fn a_priority_zero_arrival_never_displaces_a_normal_incumbent() {
        let mut sink = TagSink::new();
        sink.record("File:Comment".to_string(), occ("normal", 1, 0));
        sink.record("File:Comment".to_string(), occ("low", 0, 1));
        assert_eq!(
            sink.get("File:Comment"),
            Some(&TagValue::new_string("normal"))
        );
    }

    #[test]
    fn a_different_instance_never_displaces_the_incumbent_regardless_of_priority() {
        // ExifTool.pm:9564's DOC_NUM guard: CanonRaw.cr3's Track2..Track4
        // TrackID never displace Track1's, even though every one of them is
        // an ordinary priority-1 tag that would otherwise tie-and-win.
        let mut sink = TagSink::new();
        sink.record(
            "QuickTime:TrackID".to_string(),
            occ_with_instance("1", 1, 0, Instance(1)),
        );
        sink.record(
            "QuickTime:TrackID".to_string(),
            occ_with_instance("2", 1, 1, Instance(2)),
        );
        sink.record(
            "QuickTime:TrackID".to_string(),
            occ_with_instance("3", 1, 2, Instance(3)),
        );
        assert_eq!(
            sink.get("QuickTime:TrackID"),
            Some(&TagValue::new_string("1")),
            "Track1 keeps the bare key no matter how many later tracks arrive"
        );
        assert_eq!(sink.occurrences().len(), 3, "every track is still retained");
    }

    #[test]
    fn a_main_document_arrival_still_uses_the_ordinary_priority_rule() {
        // Instance::default() (the main document) is exempt from the
        // DOC_NUM guard, so two main-document ties still go to the newest --
        // this is Step 18's original rule, unaffected by Step 19's addition.
        let mut sink = TagSink::new();
        sink.record(
            "EXIF:Make".to_string(),
            occ_with_instance("first", 1, 0, Instance::default()),
        );
        sink.record(
            "EXIF:Make".to_string(),
            occ_with_instance("second", 1, 1, Instance::default()),
        );
        assert_eq!(sink.get("EXIF:Make"), Some(&TagValue::new_string("second")));
    }

    #[test]
    fn re_recording_the_same_instance_still_uses_the_priority_rule() {
        // The DOC_NUM guard only blocks a *different* instance; ExifTool.pm
        // :9564's `$$self{TAG_EXTRA}{$tag}{G3} eq $$self{DOC_NUM}` case
        // (re-processing the same sub-document) still falls through to the
        // ordinary priority comparison.
        let mut sink = TagSink::new();
        sink.record(
            "QuickTime:TrackID".to_string(),
            occ_with_instance("first", 1, 0, Instance(1)),
        );
        sink.record(
            "QuickTime:TrackID".to_string(),
            occ_with_instance("second", 1, 1, Instance(1)),
        );
        assert_eq!(
            sink.get("QuickTime:TrackID"),
            Some(&TagValue::new_string("second"))
        );
    }

    #[test]
    fn remove_clears_the_projection_without_touching_other_indices() {
        let mut sink = TagSink::new();
        sink.record("EXIF:Make".to_string(), occ("a", 1, 0));
        sink.record("EXIF:Model".to_string(), occ("b", 1, 1));
        let removed = sink.remove("EXIF:Make");
        assert_eq!(removed, Some(TagValue::new_string("a")));
        assert!(!sink.contains_key("EXIF:Make"));
        assert_eq!(sink.get("EXIF:Model"), Some(&TagValue::new_string("b")));
    }
}
