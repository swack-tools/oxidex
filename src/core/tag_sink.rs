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
//! (`ExifTool.pm:9564`): a newly-recorded occurrence displaces the current
//! winner for its key whenever `new.priority >= existing.priority`. Losing
//! occurrences are retained in `occurrences` -- reachable later via
//! [`TagSink::occurrences`] -- but the projection (and therefore
//! `MetadataMap`) does not expose them in Phase A; that is Step 19's job.
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

use super::tag_occurrence::TagOccurrence;
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
    /// if ($priority >= $oldPriority and ...) {
    ///     # move existing tag out of the way since this tag is higher priority
    ///     ...
    /// } else {
    ///     $tag = $nextTag;        # don't override the existing tag
    /// }
    /// ```
    ///
    /// i.e. on a priority *tie* the newer occurrence wins and the older one
    /// is the one relegated to a duplicate slot -- last arrival wins, not
    /// first. This is also, independently, what today's
    /// `HashMap<String, TagValue>::insert()` does on every call regardless
    /// of priority (unconditional overwrite = last write wins). Because
    /// every occurrence the `insert()` shim mints for a given process shares
    /// [`super::tag_occurrence::SHIM_DEFAULT_PRIORITY`], every duplicate key
    /// recorded through the shim ties on priority, so this rule reproduces
    /// today's overwrite behavior exactly -- which is the whole of Phase A's
    /// zero-behavior-change requirement (design decision D3).
    pub fn record(&mut self, key: String, occurrence: TagOccurrence) {
        let idx = self.occurrences.len();
        let priority = occurrence.priority;
        self.occurrences.push(occurrence);
        match self.winners.entry(key) {
            Entry::Occupied(mut e) => {
                let existing_idx = *e.get();
                if priority >= self.occurrences[existing_idx].priority {
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
