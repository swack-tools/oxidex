//! MetadataMap structure for storing extracted metadata
//!
//! This module defines the core MetadataMap data structure.
//!
//! As of Step 18 (Phase A of the tag-machinery overhaul, see
//! `OVERHAUL_STEP18_DESIGN.md`), the storage behind this type is a
//! [`TagSink`] of [`TagOccurrence`]s rather than a bare `HashMap<String,
//! TagValue>`. `MetadataMap` itself is unchanged from the outside: it is
//! "the projected view" the design document promises -- every method here
//! reproduces the exact observable behavior the old `HashMap`-backed version
//! had (see each method's doc comment for the specific old behavior it is
//! matching). `insert()` is the shim described there: it mints a
//! [`TagOccurrence`] with a default priority and the next file-order value,
//! so none of this crate's ~4,034 `insert()` call sites need to change.

#![allow(dead_code)]

use super::tag_occurrence::TagOccurrence;
use super::tag_sink::TagSink;
use super::tag_value::TagValue;
use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, Serializer};
use std::collections::HashMap;

/// Uninterpreted metadata retained without inventing a public tag name or value.
///
/// `context` identifies the source container/table, `identifier` retains its raw
/// key, and `payload` holds the bytes after that container entry's outer header.
/// These blocks are not readable-tag evidence and are excluded from tag JSON.
/// They do not by themselves promise byte-identical file rewriting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawMetadataBlock {
    /// Parser/table context, for example `QuickTime::ItemList`.
    pub context: String,
    /// Raw entry identifier; no text decoding or alias substitution.
    pub identifier: Vec<u8>,
    /// Uninterpreted entry payload, including any nested headers.
    pub payload: Vec<u8>,
}

/// A collection of metadata tags extracted from a file.
///
/// MetadataMap stores key-value pairs where keys are tag names (e.g., "EXIF:Make")
/// and values are TagValue enums that can represent different data types.
///
/// This structure is the primary in-memory representation of file metadata
/// and can be serialized to JSON for output or deserialized from existing data.
#[derive(Debug, Clone, PartialEq)]
pub struct MetadataMap {
    /// Every occurrence recorded through `insert()`, plus the winner
    /// projection over them. See the module doc comment.
    ///
    /// Full-precision `ValueConv` forms -- ExifTool keeps a tag's converted
    /// value separate from its `PrintConv` display; a rounded display such
    /// as Nikon's `FocusDistance` must not be fed back into DOF arithmetic
    /// -- live on each occurrence's own [`TagOccurrence::value`] field
    /// rather than in a separate sidecar keyed by the same string twice.
    /// That sidecar (`value_forms: HashMap<String, String>`) was Step 8(b)'s
    /// tactical carriage, explicitly deferred past Step 18 Phase A by design
    /// decision D4 in `OVERHAUL_STEP18_DESIGN.md` ("leave `value_forms`
    /// until Step 22, which consumes the occurrence winner view anyway").
    /// This is Step 22: [`MetadataMap::set_value_form`]/[`MetadataMap::
    /// value_form`] below now read and write `TagOccurrence.value` via
    /// [`TagSink::set_winner_value`] instead of a second map, so serde
    /// skipping it is automatic (occurrences were never serialized to begin
    /// with -- only the winner projection's `raw` form is, via
    /// `Serialize for MetadataMap` below) rather than a field the old
    /// sidecar had to be deliberately excluded from.
    sink: TagSink,
    raw_blocks: Vec<RawMetadataBlock>,
}

// Hand-rolled rather than `#[derive(Serialize, Deserialize)]` +
// `#[serde(flatten)]`: `TagSink` is no longer a bare map serde can flatten
// through automatically, so this reproduces the exact old wire format (a
// flat JSON object of `"Group:Tag": TagValue`, `value_forms` excluded)
// directly against the winner projection.
impl Serialize for MetadataMap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.iter())
    }
}

impl<'de> Deserialize<'de> for MetadataMap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let tags = HashMap::<String, TagValue>::deserialize(deserializer)?;
        let mut map = MetadataMap::with_capacity(tags.len());
        for (key, value) in tags {
            map.insert(key, value);
        }
        Ok(map)
    }
}

impl MetadataMap {
    /// Creates a new empty MetadataMap
    ///
    /// # Examples
    ///
    /// ```
    /// use oxidex::core::metadata_map::MetadataMap;
    ///
    /// let metadata = MetadataMap::new();
    /// assert_eq!(metadata.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            sink: TagSink::new(),
            raw_blocks: Vec::new(),
        }
    }

    /// Creates a new MetadataMap with the specified capacity
    ///
    /// This pre-allocates space for at least `capacity` tags, which can
    /// improve performance when the approximate number of tags is known.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            sink: TagSink::with_capacity(capacity),
            raw_blocks: Vec::new(),
        }
    }

    /// Uninterpreted blocks in parser encounter order, separate from named tags.
    pub fn raw_blocks(&self) -> &[RawMetadataBlock] {
        &self.raw_blocks
    }

    /// Preserve an unrecognized entry without guessing its tag identity or value.
    pub(crate) fn retain_raw_block(&mut self, block: RawMetadataBlock) {
        self.raw_blocks.push(block);
    }

    /// Inserts a tag into the metadata map
    ///
    /// If the tag already exists, its value is replaced and the old value is returned.
    ///
    /// Internally this is the Step 18 Phase-A migration shim: it mints a
    /// [`TagOccurrence`] with the default priority
    /// ([`super::tag_occurrence::SHIM_DEFAULT_PRIORITY`]) and the sink's
    /// next file-order value, and records it. Because every occurrence a
    /// given `MetadataMap` mints this way shares that same priority, a
    /// second `insert()` under the same key always ties on priority against
    /// the first -- and `TagSink::record`'s tie rule (matching `FoundTag`,
    /// `ExifTool.pm:9564`) always gives the win to the newer arrival. So the
    /// return value and the map's subsequent `get()` behavior are exactly
    /// what the old `HashMap::insert()` gave: the previous value comes back
    /// here, and the new value is what `get()` returns from now on.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxidex::core::metadata_map::MetadataMap;
    /// use oxidex::core::tag_value::TagValue;
    ///
    /// let mut metadata = MetadataMap::new();
    /// metadata.insert("EXIF:Make", TagValue::new_string("Canon"));
    /// ```
    pub fn insert<K: Into<String>>(&mut self, key: K, value: TagValue) -> Option<TagValue> {
        let key = key.into();
        // Replacing the visible tag invalidates any ValueConv form belonging
        // to its predecessor: `TagOccurrence::from_insert_shim` always
        // starts a fresh occurrence at `value: None`, so the old form simply
        // does not carry forward to whichever occurrence wins the key next
        // (Step 22 -- see the `sink` field's own doc comment). Parsers
        // attach a new form explicitly afterwards via `set_value_form`.
        let previous = self.sink.get(&key).cloned();
        let order = self.sink.next_order();
        let occurrence = TagOccurrence::from_insert_shim(&key, value, order);
        self.sink.record(key, occurrence);
        previous
    }

    /// [`insert`](Self::insert) with ExifTool's family-1 group recorded
    /// beside the key.
    ///
    /// Everything `insert()` decides stays exactly as it was -- the lookup
    /// key, the priority the shim derives from the key's own group, the
    /// default instance -- so default winners, bare-name lookups and
    /// composites keyed on `"{family0}:{name}"` see no change. Only the
    /// family-1 label `-G1`/`-j -G1`/group-qualified requests report moves
    /// to `group1`. An empty `group1` makes this exactly `insert()`.
    ///
    /// For readers whose stored key prefix is already ExifTool's family-0
    /// group but whose table names a different family-1 group:
    /// `ID3:Title` under `ID3v2_3`, `APP14:ColorTransform` under `Adobe`.
    pub(crate) fn insert_with_group1<K: Into<String>>(
        &mut self,
        key: K,
        value: TagValue,
        group1: &str,
    ) -> Option<TagValue> {
        let key = key.into();
        let previous = self.sink.get(&key).cloned();
        let order = self.sink.next_order();
        let mut occurrence = TagOccurrence::from_insert_shim(&key, value, order);
        occurrence.group1 = super::tag_occurrence::intern(group1);
        self.sink.record(key, occurrence);
        previous
    }

    /// [`insert_with_group1`](Self::insert_with_group1), also attaching the
    /// value `--no-print-conv` shows instead of `display_value`.
    ///
    /// For readers that apply a PrintConv before storing: without the
    /// ValueConv form, `-n` reprints the label (`RMETA:Azimuth` `E` where
    /// ExifTool's `-n` prints `5`). Priority and instance are exactly
    /// `insert_with_group1`'s, so the default winner does not move.
    pub(crate) fn insert_with_group1_and_value<K: Into<String>>(
        &mut self,
        key: K,
        display_value: TagValue,
        no_print_conv_value: TagValue,
        group1: &str,
    ) -> Option<TagValue> {
        let key = key.into();
        let previous = self.sink.get(&key).cloned();
        let order = self.sink.next_order();
        let mut occurrence = TagOccurrence::from_insert_shim(&key, display_value, order);
        occurrence.group1 = super::tag_occurrence::intern(group1);
        occurrence.value = Some(no_print_conv_value);
        self.sink.record(key, occurrence);
        previous
    }

    /// Copies every winner of `source` into this map through
    /// [`insert_with_group1`](Self::insert_with_group1), so a family-1 group
    /// the sub-parser recorded survives the copy -- and so does the
    /// `--no-print-conv` form it attached
    /// ([`insert_with_group1_and_value`](Self::insert_with_group1_and_value)):
    /// dropping it made `-n` reprint the label (APP6 `NITF:ImageColor`
    /// `Monochrome` where ExifTool prints `0`). A source built only with
    /// `insert()` copies exactly as `for (k, v) in source.iter() {
    /// self.insert(k, v) }` would.
    pub(crate) fn merge_winners_keeping_group1(&mut self, source: &MetadataMap) {
        for (key, occurrence) in source.sink.winner_occurrences() {
            match &occurrence.value {
                Some(value) => self.insert_with_group1_and_value(
                    key.clone(),
                    occurrence.raw.clone(),
                    value.clone(),
                    &occurrence.group1,
                ),
                None => {
                    self.insert_with_group1(key.clone(), occurrence.raw.clone(), &occurrence.group1)
                }
            };
        }
    }

    /// Records an occurrence with an explicit priority, family-1 group and
    /// instance identity, following ExifTool's `FoundTag` arbitration
    /// (`ExifTool.pm:9448`+) instead of `insert()`'s flat
    /// `SHIM_DEFAULT_PRIORITY` / `Instance::default()`.
    ///
    /// Step 19's exemplar families (`OVERHAUL_STEP18_DESIGN.md` §2.3
    /// Phase B) use this wherever real `Priority => 0` or per-track/
    /// sub-document semantics apply -- JPEG COM segments, QuickTime track
    /// headers, diagnostic warnings, and one Pentax MakerNote duplicate
    /// pair. Every other call site keeps going through `insert()`
    /// unchanged. See each call site for its own `ExifTool.pm` citation and
    /// [`super::tag_sink::TagSink::record`] for the arbitration rule this
    /// feeds.
    pub(crate) fn insert_occurrence<K: Into<String>>(
        &mut self,
        key: K,
        value: TagValue,
        priority: u8,
        group1: &str,
        instance: super::tag_occurrence::Instance,
    ) -> Option<TagValue> {
        let key = key.into();
        let previous = self.sink.get(&key).cloned();
        let order = self.sink.next_order();
        let mut occurrence = TagOccurrence::from_insert_shim(&key, value, order);
        occurrence.priority = priority;
        occurrence.group1 = super::tag_occurrence::intern(group1);
        occurrence.instance = instance;
        self.sink.record(key, occurrence);
        previous
    }

    /// Like [`insert_occurrence`](Self::insert_occurrence), but also attaches
    /// the value `--no-print-conv` should show instead of `display_value`.
    ///
    /// Step 20 (`OVERHAUL_STEP18_DESIGN.md` §2.3 Phase C) needs this at
    /// sites that fuse ExifTool's ValueConv/PrintConv into one formatted
    /// string before ever calling `insert()` -- AGENTS.md's tagmodel/1.5
    /// finding, "`--no-print-conv` cannot restore raw, because formatting
    /// happens before storage". `extract_file_metadata`'s `File:FileSize`
    /// (`file_metadata.rs`) is the one call site this step migrates: it
    /// stores `"26 kB"` as `display_value` (unchanged from today, so every
    /// existing reader -- `get_string`, JSON/CSV/human output,
    /// `format_for_exiftool`'s pass-through -- observes no change) and the
    /// byte count `26106` as `no_print_conv_value`, which only
    /// [`MetadataMap::without_print_conv`] and the CLI's request-resolution
    /// path (`cli::tag_resolution`) ever read back out.
    pub(crate) fn insert_occurrence_with_raw<K: Into<String>>(
        &mut self,
        key: K,
        display_value: TagValue,
        no_print_conv_value: TagValue,
        priority: u8,
        group1: &str,
        instance: super::tag_occurrence::Instance,
    ) -> Option<TagValue> {
        self.insert_occurrence_with_forms(
            key,
            display_value,
            no_print_conv_value,
            None,
            priority,
            group1,
            instance,
        )
    }

    /// [`insert_occurrence_with_raw`](Self::insert_occurrence_with_raw),
    /// also attaching the value as the file stores it
    /// ([`TagOccurrence::stored`]) -- for a producer whose display value is
    /// ExifTool's printed one (the ExifIFD engine rows).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert_occurrence_with_forms<K: Into<String>>(
        &mut self,
        key: K,
        display_value: TagValue,
        no_print_conv_value: TagValue,
        stored: Option<TagValue>,
        priority: u8,
        group1: &str,
        instance: super::tag_occurrence::Instance,
    ) -> Option<TagValue> {
        let key = key.into();
        let previous = self.sink.get(&key).cloned();
        let order = self.sink.next_order();
        let mut occurrence = TagOccurrence::from_insert_shim(&key, display_value, order);
        occurrence.priority = priority;
        occurrence.group1 = super::tag_occurrence::intern(group1);
        occurrence.instance = instance;
        occurrence.value = Some(no_print_conv_value);
        occurrence.stored = stored;
        self.sink.record(key, occurrence);
        previous
    }

    /// Records a copy of another map's winning `source` occurrence under
    /// `key` at this map's `priority` / `group1` / `instance` -- its value,
    /// AND its `--no-print-conv` form when it carries one
    /// ([`TagOccurrence::value`]), with its stored form
    /// ([`TagOccurrence::stored`]). For a parser that re-homes another walk's
    /// winners (a container re-entering the JPEG/TIFF/embedded-EXIF parsers):
    /// copying the flattened [`MetadataMap::iter`] value through `insert()`
    /// keeps only the printed form, so `--no-print-conv` would show the label
    /// of every row whose producer stores ExifTool's PrintConv output with its
    /// ValueConv form beside it -- the IFD engine's IFD1, ExifIFD and
    /// InteropIFD rows (`ColorSpace` `sRGB` where ExifTool's `-n` prints `1`).
    pub(crate) fn insert_copied_occurrence<K: Into<String>>(
        &mut self,
        key: K,
        source: &TagOccurrence,
        priority: u8,
        group1: &str,
        instance: super::tag_occurrence::Instance,
    ) -> Option<TagValue> {
        match &source.value {
            Some(value) => self.insert_occurrence_with_forms(
                key,
                source.raw.clone(),
                value.clone(),
                source.stored.clone(),
                priority,
                group1,
                instance,
            ),
            None => self.insert_occurrence(key, source.raw.clone(), priority, group1, instance),
        }
    }

    /// [`insert()`](Self::insert) of `source`'s value under `key` -- the
    /// shim's priority, group1 and instance, a fresh file-order slot -- that
    /// keeps `source`'s `--no-print-conv` form ([`TagOccurrence::value`])
    /// and stored form ([`TagOccurrence::stored`]).
    /// For a parser that used to flatten another walk's winners through
    /// `iter()` + `insert()` (the PDF resource and DCT-image merges, MIFF's
    /// APP1 profile): exactly that copy, minus the loss of the form, which
    /// the ExifIFD engine's rows need (`ColorSpace` `sRGB` / `-n` 1) and
    /// Composite inputs read. [`insert_copied_occurrence`](Self::
    /// insert_copied_occurrence) is the same for a caller that sets its own
    /// priority.
    pub(crate) fn insert_carrying_forms<K: Into<String>>(
        &mut self,
        key: K,
        source: &TagOccurrence,
    ) -> Option<TagValue> {
        let key = key.into();
        let previous = self.sink.get(&key).cloned();
        let order = self.sink.next_order();
        let mut occurrence = TagOccurrence::from_insert_shim(&key, source.raw.clone(), order);
        occurrence.value = source.value.clone();
        occurrence.stored = source.stored.clone();
        self.sink.record(key, occurrence);
        previous
    }

    /// Copies `occurrence` into this map under `new_key`, preserving every
    /// field -- `raw`, `value` (the ValueConv form Step 22 folded into
    /// `TagOccurrence`), `print`, priority, family-1 group and instance --
    /// and re-deriving only `id`/`name`/`group0` from `new_key`'s own split.
    ///
    /// Used by [`tag_normalization::normalize_metadata_map`
    /// ](super::tag_normalization::normalize_metadata_map) to rename a
    /// family prefix (`ExifIFD:` -> `EXIF:`, `Fujifilm:` -> `FujiFilm:`,
    /// ...) without flattening the occurrence back to `insert()`'s shim
    /// defaults. Before this step, that call site went through
    /// [`insert_occurrence`](Self::insert_occurrence) (which has no `value`
    /// parameter) plus a separate `value_forms` sidecar re-attachment, which
    /// silently dropped any `TagOccurrence.value` a migrated call site had
    /// already attached (Step 20's `insert_occurrence_with_raw` sites, in
    /// particular) the moment a JPEG-family tag passed through renaming --
    /// invisible until this step folded `value_forms` away and this method
    /// took over carrying the whole occurrence across the rename instead.
    pub(crate) fn insert_renamed_occurrence(
        &mut self,
        new_key: String,
        occurrence: &TagOccurrence,
    ) {
        let order = self.sink.next_order();
        let (group0, name) = match new_key.split_once(':') {
            Some((g, n)) => (
                super::tag_occurrence::intern(g),
                super::tag_occurrence::intern(n),
            ),
            None => (
                super::tag_occurrence::intern(""),
                super::tag_occurrence::intern(&new_key),
            ),
        };
        let mut renamed = occurrence.clone();
        renamed.id = oxidex_tags::TagId::Named(new_key.clone());
        renamed.group0 = group0;
        renamed.name = name;
        renamed.order = order;
        self.sink.record(new_key, renamed);
    }

    /// Every occurrence recorded for `key`, winners and losers alike, in
    /// file order. Exists to let Step 19's migrated call sites verify real
    /// duplicate retention (`cargo test --lib`) without a `-a` output mode,
    /// which is Step 20+'s job; nothing in default output reads this.
    #[cfg(test)]
    pub(crate) fn occurrences_for(&self, key: &str) -> Vec<&TagOccurrence> {
        self.sink
            .occurrences()
            .filter(|o| o.lookup_key() == key)
            .collect()
    }

    /// Every occurrence recorded so far, winners and losers alike, in file
    /// order, paired with the lookup key it was recorded under.
    ///
    /// Exists for consumers that rebuild a whole `MetadataMap` from another
    /// one's contents -- [`normalize_metadata_map`](super::tag_normalization::normalize_metadata_map),
    /// in particular -- so they can replay every occurrence (preserving
    /// priority, family-1 group and instance) instead of iterating
    /// [`MetadataMap::iter`]'s winner-only projection and silently
    /// flattening every retained duplicate back to `insert()`'s
    /// `SHIM_DEFAULT_PRIORITY`, the same failure mode `merge()` had before
    /// Step 19 fixed it.
    pub(crate) fn all_occurrences(&self) -> impl Iterator<Item = (String, &TagOccurrence)> {
        self.sink.occurrences().map(|o| (o.lookup_key(), o))
    }

    /// [`all_occurrences`](Self::all_occurrences) without the lookup key:
    /// the same active occurrences in the same file order, for consumers
    /// that only read an occurrence's own fields. Building the key costs one
    /// `format!` per occurrence, which the Composite layer's dependency
    /// resolution was paying per dependency per pass (#821's profile).
    pub(crate) fn occurrences(&self) -> impl Iterator<Item = &TagOccurrence> {
        self.sink.occurrences()
    }

    /// How many occurrences this map has ever recorded, retired ones
    /// included. Positions `0..recorded_len()` are what
    /// [`active_occurrence`](Self::active_occurrence) accepts, and a later
    /// insert only ever appends past the end -- so a consumer can index what
    /// it has seen and pick up only what arrived since.
    pub(crate) fn recorded_len(&self) -> usize {
        self.sink.recorded_len()
    }

    /// The occurrence at file-order position `idx`, or `None` if it was
    /// removed (or never recorded).
    pub(crate) fn active_occurrence(&self, idx: usize) -> Option<&TagOccurrence> {
        self.sink.active_occurrence(idx)
    }

    /// Attaches a full-precision value form to an existing visible tag.
    ///
    /// The value is intentionally absent from iteration and serialization
    /// (only the winner projection's `raw` form is ever emitted -- see
    /// `Serialize for MetadataMap`, above). Consumed by the Composite layer
    /// (`src/composite/mod.rs`) and by [`MetadataMap::without_print_conv`]/
    /// the CLI's `--no-print-conv` resolution (`cli::tag_resolution`), which
    /// read it back via [`TagOccurrence::value`] alongside every other
    /// migrated call site's ValueConv form -- Step 22 folded this method's
    /// old private `value_forms` sidecar into that same field rather than
    /// keeping two mechanisms for the same concept (see the `sink` field's
    /// doc comment).
    pub(crate) fn set_value_form<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        let key = key.into();
        self.sink
            .set_winner_value(&key, TagValue::new_string(value.into()));
    }

    /// Returns the full-precision value form attached to `key`, if any.
    pub(crate) fn value_form(&self, key: &str) -> Option<&str> {
        self.sink
            .winner_occurrence(key)?
            .value
            .as_ref()?
            .as_string()
    }

    /// Merges another map, replaying every occurrence -- not just the
    /// winner projection -- from `other` into `self`'s own sink via
    /// [`TagSink::record_carrying_over`], preserving each occurrence's
    /// priority, family-1 group, instance and `value` (ValueConv) form
    /// rather than flattening it back to `insert()`'s
    /// `SHIM_DEFAULT_PRIORITY`/`Instance::default()`.
    ///
    /// This matters because `merge()` is the one place a parser's own
    /// sub-map (`format_metadata`, built by an entire segment pipeline) enters
    /// the file's final `MetadataMap` (`operations.rs` Step 5) -- so the
    /// original shape here (`other.sink.into_winner_map()` then a bare
    /// `self.insert()` per key) would have silently thrown away every one
    /// of Step 19's retained duplicates and real priorities the instant a
    /// JPEG's `process_com_segments`-built sub-map crossed this boundary,
    /// making the retention promise those call sites document a lie for
    /// every multi-stage parser. Replaying occurrences one at a time
    /// reproduces `other`'s own winner exactly (the tie-break rule is
    /// deterministic over a fixed relative order) while keeping every
    /// occurrence reachable afterward. Since `value` now lives on the
    /// occurrence itself (Step 22), replaying the occurrence carries its
    /// ValueConv form across the boundary automatically -- no separate
    /// `value_forms` pass is needed anymore.
    pub(crate) fn merge(&mut self, other: MetadataMap) {
        self.raw_blocks.extend(other.raw_blocks);
        for occurrence in other.sink.into_occurrences() {
            self.sink.record_carrying_over(occurrence);
        }
    }

    /// Retrieves a tag value by name
    ///
    /// Returns `None` if the tag doesn't exist.
    pub fn get(&self, key: &str) -> Option<&TagValue> {
        self.sink.get(key)
    }

    /// Retrieves a mutable reference to a tag value by name
    ///
    /// Returns `None` if the tag doesn't exist.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut TagValue> {
        // A mutable reference can change the visible value without another
        // call into this map, so its old ValueConv form is no longer sound.
        // `TagSink::get_mut` itself clears the winner occurrence's `value`
        // field for exactly this reason -- see its own doc comment.
        self.sink.get_mut(key)
    }

    /// Removes a tag from the map
    ///
    /// Returns the value if the tag existed, `None` otherwise.
    pub fn remove(&mut self, key: &str) -> Option<TagValue> {
        self.sink.remove(key)
    }

    /// Checks if a tag exists in the map
    pub fn contains_key(&self, key: &str) -> bool {
        self.sink.contains_key(key)
    }

    /// Returns the number of tags in the map
    pub fn len(&self) -> usize {
        self.sink.len()
    }

    /// Returns true if the map contains no tags
    pub fn is_empty(&self) -> bool {
        self.sink.is_empty()
    }

    /// Clears all tags from the map
    pub fn clear(&mut self) {
        self.sink.clear();
        self.raw_blocks.clear();
    }

    /// Returns an iterator over tag names and values
    pub fn iter(&self) -> impl Iterator<Item = (&String, &TagValue)> {
        self.sink.iter()
    }

    /// Returns an iterator over tag names
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.sink.keys()
    }

    /// Returns an iterator over tag values
    pub fn values(&self) -> impl Iterator<Item = &TagValue> {
        self.sink.values()
    }

    /// Every key's current winner, paired with its full [`TagOccurrence`]
    /// rather than the flattened display value [`MetadataMap::iter`] gives.
    /// Used by Step 20's `--no-print-conv` handling and the CLI's
    /// group/priority-aware request resolution (`cli::tag_resolution`).
    pub(crate) fn winner_occurrences(&self) -> impl Iterator<Item = (&String, &TagOccurrence)> {
        self.sink.winner_occurrences()
    }

    /// [`MetadataMap::winner_occurrences`] in file order (`TagOccurrence::
    /// order`), for a parser that re-homes another walk's winners into its
    /// own map. The winners live in a `HashMap`, so iterating them directly
    /// visits them in a per-process random order, and the order they are
    /// copied in is the order the receiving map's folds (bare-name `-TAG`
    /// answers, Composite inputs) see: two copies of one tag under different
    /// keys at one priority -- `ExifIFD:FNumber` and a maker note's
    /// `Canon:FNumber` -- then fold differently from run to run.
    pub(crate) fn winners_in_file_order(&self) -> Vec<(&String, &TagOccurrence)> {
        let mut winners: Vec<_> = self.winner_occurrences().collect();
        winners.sort_by_key(|(_, occurrence)| occurrence.order);
        winners
    }

    /// A copy of this map with each tag's stored value swapped for the form
    /// `--no-print-conv` should show.
    ///
    /// For every key whose winning occurrence carries a value attached via
    /// [`MetadataMap::insert_occurrence_with_raw`], that value is used.
    /// Legacy APEX storage is converted with the same ValueConv consumed by
    /// composites. Other keys retain their stored value. This does not infer
    /// ValueConv from printed labels for parser sites not yet migrated.
    /// This is the whole-map counterpart to the CLI's
    /// occurrence-aware resolution for a specific `-TAG` request; the
    /// unfiltered/default-listing path uses this one so `--no-print-conv`
    /// behaves consistently whether or not a tag filter is given.
    pub fn without_print_conv(&self) -> MetadataMap {
        let mut out = MetadataMap::with_capacity(self.len());
        out.raw_blocks.clone_from(&self.raw_blocks);
        for (key, occurrence) in self.winner_occurrences() {
            let value = occurrence.value_conv();
            out.insert(key.clone(), value);
        }
        out
    }

    /// Typed getter for string values
    ///
    /// Returns `None` if the tag doesn't exist or isn't a String variant.
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.as_string())
    }

    /// Typed getter for integer values
    ///
    /// Returns `None` if the tag doesn't exist or isn't an Integer variant.
    pub fn get_integer(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.as_integer())
    }

    /// Typed getter for float values
    ///
    /// Returns `None` if the tag doesn't exist or isn't a Float variant.
    pub fn get_float(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|v| v.as_float())
    }

    /// Typed getter for datetime values
    ///
    /// Returns `None` if the tag doesn't exist or isn't a DateTime variant.
    pub fn get_datetime(&self, key: &str) -> Option<&chrono::DateTime<chrono::Utc>> {
        self.get(key).and_then(|v| v.as_datetime())
    }
}

impl Default for MetadataMap {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIterator<(String, TagValue)> for MetadataMap {
    fn from_iter<T: IntoIterator<Item = (String, TagValue)>>(iter: T) -> Self {
        // `HashMap::from_iter` keeps the last value for a repeated key (it
        // is built via repeated `insert()`, which overwrites); driving every
        // pair through our own `insert()` shim reproduces that exactly, via
        // the same tie-break `TagSink::record` documents.
        let mut map = MetadataMap::new();
        for (key, value) in iter {
            map.insert(key, value);
        }
        map
    }
}

/// Implements IntoIterator for MetadataMap to allow consuming iteration.
///
/// This implementation enables move semantics when iterating over a MetadataMap,
/// avoiding unnecessary clones when the map is being consumed.
///
/// # Performance
///
/// Using `into_iter()` instead of `iter()` followed by clones eliminates
/// heap allocations for String keys and TagValue variants, improving
/// performance in metadata merge operations by 5-10%.
///
/// # Examples
///
/// ```
/// use oxidex::core::metadata_map::MetadataMap;
/// use oxidex::core::tag_value::TagValue;
///
/// let mut map1 = MetadataMap::new();
/// map1.insert("EXIF:Make", TagValue::new_string("Canon"));
///
/// let mut map2 = MetadataMap::new();
///
/// // Consume map1 and move its entries into map2 (no clones needed)
/// for (key, value) in map1 {
///     map2.insert(key, value);
/// }
/// ```
impl IntoIterator for MetadataMap {
    type Item = (String, TagValue);
    type IntoIter = std::vec::IntoIter<(String, TagValue)>;

    fn into_iter(self) -> Self::IntoIter {
        // Only the winner projection is consumed, in file order (see
        // `TagSink::into_winners`); losing occurrences are dropped along with
        // the rest of the sink.
        self.sink.into_winners().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two ways parsers copy a sub-map into the file's map -- `iter()`
    /// with clones, and consuming `for (k, v) in sub` -- both hand the tags
    /// over in the order the sub-map recorded them, so the copies' file order
    /// (what `-a` renders) is the same on every run.
    #[test]
    fn copying_a_sub_map_keeps_its_file_order_on_every_run() {
        let names = ["Make", "Model", "Orientation", "XResolution", "YResolution"];
        let names = names.map(|n| format!("IFD0:{n}"));
        for run in 0..32 {
            let mut sub = MetadataMap::new();
            for name in &names {
                sub.insert(name.clone(), TagValue::new_string(name.clone()));
            }
            let mut by_ref = MetadataMap::new();
            for (key, value) in sub.iter() {
                by_ref.insert(key.clone(), value.clone());
            }
            let mut by_value = MetadataMap::new();
            for (key, value) in sub {
                by_value.insert(key, value);
            }
            for copy in [by_ref, by_value] {
                let order: Vec<String> = copy.all_occurrences().map(|(key, _)| key).collect();
                assert_eq!(order, names, "copy in run {run}");
            }
        }
    }

    #[test]
    fn test_new_metadata_map() {
        let map = MetadataMap::new();
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
    }

    /// `insert_copied_occurrence` carries the `--no-print-conv` form a
    /// plain `insert()` of the flattened value would drop.
    #[test]
    fn a_copied_occurrence_keeps_its_no_print_conv_form() {
        use super::super::tag_occurrence::{Instance, SHIM_DEFAULT_PRIORITY};
        let mut source = MetadataMap::new();
        source.insert_occurrence_with_raw(
            "ExifIFD:ColorSpace",
            TagValue::new_string("sRGB"),
            TagValue::Integer(1),
            SHIM_DEFAULT_PRIORITY,
            "",
            Instance::default(),
        );
        source.insert("ExifIFD:ExifVersion", TagValue::new_string("0232"));
        let mut copy = MetadataMap::new();
        for (key, occurrence) in source.winner_occurrences() {
            copy.insert_copied_occurrence(key.clone(), occurrence, 0, "", Instance::default());
        }
        assert_eq!(copy.get_string("ExifIFD:ColorSpace"), Some("sRGB"));
        let n = copy.without_print_conv();
        assert_eq!(n.get("ExifIFD:ColorSpace"), Some(&TagValue::Integer(1)));
        assert_eq!(n.get_string("ExifIFD:ExifVersion"), Some("0232"));
        assert_eq!(copy.occurrences_for("ExifIFD:ColorSpace")[0].priority, 0);
    }

    /// `winners_in_file_order` is the source's recording order, whatever
    /// the winners' `HashMap` visits; `insert_carrying_forms` is `insert()`
    /// (the shim's priority: 0 for `XMP-exif`, 1 otherwise) plus the `-n`
    /// form.
    #[test]
    fn winners_copy_in_file_order_with_their_forms() {
        use super::super::tag_occurrence::{Instance, SHIM_DEFAULT_PRIORITY};
        let mut source = MetadataMap::new();
        let keys: Vec<String> = (0..64).map(|i| format!("ExifIFD:Tag{i}")).collect();
        for key in &keys {
            source.insert(key.as_str(), TagValue::Integer(1));
        }
        source.insert_occurrence_with_raw(
            "ExifIFD:ColorSpace",
            TagValue::new_string("sRGB"),
            TagValue::Integer(1),
            SHIM_DEFAULT_PRIORITY,
            "",
            Instance::default(),
        );
        source.insert("XMP-exif:ColorSpace", TagValue::new_string("sRGB"));
        let order: Vec<&String> = source
            .winners_in_file_order()
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        let mut expected: Vec<&String> = keys.iter().collect();
        let tail = [
            "ExifIFD:ColorSpace".to_string(),
            "XMP-exif:ColorSpace".to_string(),
        ];
        expected.extend(tail.iter());
        assert_eq!(order, expected);

        let mut copy = MetadataMap::new();
        for (key, occurrence) in source.winners_in_file_order() {
            copy.insert_carrying_forms(key.clone(), occurrence);
        }
        assert_eq!(copy.get_string("ExifIFD:ColorSpace"), Some("sRGB"));
        let n = copy.without_print_conv();
        assert_eq!(n.get("ExifIFD:ColorSpace"), Some(&TagValue::Integer(1)));
        assert_eq!(copy.occurrences_for("ExifIFD:ColorSpace")[0].priority, 1);
        assert_eq!(copy.occurrences_for("XMP-exif:ColorSpace")[0].priority, 0);
    }

    #[test]
    fn test_insert_and_get() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));

        assert_eq!(map.len(), 1);
        assert!(!map.is_empty());
        assert_eq!(map.get_string("EXIF:Make"), Some("Canon"));
    }

    #[test]
    fn test_insert_multiple_tags() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Nikon"));
        map.insert("EXIF:Model", TagValue::new_string("D850"));
        map.insert("EXIF:ISO", TagValue::new_integer(400));

        assert_eq!(map.len(), 3);
        assert_eq!(map.get_string("EXIF:Make"), Some("Nikon"));
        assert_eq!(map.get_string("EXIF:Model"), Some("D850"));
        assert_eq!(map.get_integer("EXIF:ISO"), Some(400));
    }

    #[test]
    fn test_replace_existing_tag() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        let old = map.insert("EXIF:Make", TagValue::new_string("Sony"));

        assert_eq!(
            old.and_then(|v| v.as_string().map(String::from)),
            Some("Canon".to_string())
        );
        assert_eq!(map.get_string("EXIF:Make"), Some("Sony"));
    }

    #[test]
    fn replacing_or_mutating_a_tag_invalidates_its_value_form() {
        let mut map = MetadataMap::new();
        map.insert("Nikon:FocusDistance", TagValue::new_string("0.71 m"));
        map.set_value_form("Nikon:FocusDistance", "0.707945784384138");

        map.insert("Nikon:FocusDistance", TagValue::new_string("1.00 m"));
        assert_eq!(map.value_form("Nikon:FocusDistance"), None);

        map.set_value_form("Nikon:FocusDistance", "1");
        assert!(map.get_mut("Nikon:FocusDistance").is_some());
        assert_eq!(map.value_form("Nikon:FocusDistance"), None);
    }

    #[test]
    fn merge_preserves_value_forms_without_serializing_them() {
        let mut source = MetadataMap::new();
        source.insert("Nikon:FocusDistance", TagValue::new_string("0.71 m"));
        source.set_value_form("Nikon:FocusDistance", "0.707945784384138");

        let mut target = MetadataMap::new();
        target.merge(source);

        assert_eq!(
            target.value_form("Nikon:FocusDistance"),
            Some("0.707945784384138")
        );
        let json = serde_json::to_string(&target).unwrap();
        assert!(json.contains("0.71 m"));
        assert!(!json.contains("0.707945784384138"));
    }

    #[test]
    fn test_remove_tag() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        assert_eq!(map.len(), 1);

        let removed = map.remove("EXIF:Make");
        assert!(removed.is_some());
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn test_contains_key() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));

        assert!(map.contains_key("EXIF:Make"));
        assert!(!map.contains_key("EXIF:Model"));
    }

    #[test]
    fn test_clear() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        map.insert("EXIF:Model", TagValue::new_string("EOS R5"));

        assert_eq!(map.len(), 2);
        map.clear();
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn test_typed_getters() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        map.insert("EXIF:ISO", TagValue::new_integer(800));
        map.insert("EXIF:FNumber", TagValue::new_float(2.8));

        assert_eq!(map.get_string("EXIF:Make"), Some("Canon"));
        assert_eq!(map.get_integer("EXIF:ISO"), Some(800));
        assert_eq!(map.get_float("EXIF:FNumber"), Some(2.8));

        // Wrong type should return None
        assert_eq!(map.get_integer("EXIF:Make"), None);
        assert_eq!(map.get_string("EXIF:ISO"), None);
    }

    #[test]
    fn test_clone() {
        let mut map1 = MetadataMap::new();
        map1.insert("EXIF:Make", TagValue::new_string("Canon"));

        let map2 = map1.clone();
        assert_eq!(map1, map2);
        assert_eq!(map2.get_string("EXIF:Make"), Some("Canon"));
    }

    #[test]
    fn test_debug() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));

        let debug_str = format!("{:?}", map);
        assert!(debug_str.contains("MetadataMap"));
    }

    #[test]
    fn test_serde_serialization() {
        let mut map = MetadataMap::new();
        map.insert("EXIF:Make", TagValue::new_string("Canon"));
        map.insert("EXIF:ISO", TagValue::new_integer(400));

        let json = serde_json::to_string(&map).unwrap();
        assert!(json.contains("EXIF:Make"));
        assert!(json.contains("Canon"));
        assert!(json.contains("EXIF:ISO"));
    }

    #[test]
    fn test_serde_deserialization() {
        let json = r#"{"EXIF:Make":{"type":"String","value":"Nikon"},"EXIF:ISO":{"type":"Integer","value":800}}"#;
        let map: MetadataMap = serde_json::from_str(json).unwrap();

        assert_eq!(map.len(), 2);
        assert_eq!(map.get_string("EXIF:Make"), Some("Nikon"));
        assert_eq!(map.get_integer("EXIF:ISO"), Some(800));
    }

    #[test]
    fn test_from_iterator() {
        let tags = vec![
            ("EXIF:Make".to_string(), TagValue::new_string("Canon")),
            ("EXIF:Model".to_string(), TagValue::new_string("EOS R5")),
        ];

        let map: MetadataMap = tags.into_iter().collect();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get_string("EXIF:Make"), Some("Canon"));
    }
}

#[cfg(test)]
mod step19_duplicate_retention_regression {
    //! Part V §1.1 of the merged tag review found ~209-215 repeated
    //! `group:name` cases across 53/194 `t/images` files that
    //! `HashMap`-backed `MetadataMap` silently collapsed to one instance
    //! each. Step 19 closes that for five specific tags across five
    //! specific pinned files -- not the whole corpus, which stays
    //! unmigrated until later exemplar families follow the same pattern.
    //!
    //! This is also the regression pin for two flattening points Step 19
    //! discovered and fixed while wiring these families up end to end:
    //! `MetadataMap::merge` (used wherever a parser's own sub-map enters the
    //! final map -- the JPEG pipeline, in particular) and
    //! `normalize_metadata_map` (the JPEG parser's own last step) each used
    //! to iterate only the winner projection and rebuild a fresh map from
    //! it, which silently re-flattened every retained duplicate straight
    //! back to `insert()`'s `SHIM_DEFAULT_PRIORITY` the moment either ran --
    //! so `ExifTool.jpg` reached this test with only one `File:Comment`
    //! occurrence even after `parse_comment_segment`/
    //! `parse_app10_unicode_comment_segment` correctly recorded two.

    use std::path::Path;

    fn occurrence_count(path: &str, key: &str) -> usize {
        let path = Path::new(path);
        if !path.is_file() {
            eprintln!("skip: pinned fixture {} not present", path.display());
            return usize::MAX; // never equals an asserted expectation
        }
        let report = crate::core::operations::read_metadata_report(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        report.metadata.occurrences_for(key).len()
    }

    /// Two `Comment` sources -- the COM marker and the APP10 "UNICODE"
    /// variant JPEG.pm declares as the same tag -- both `Priority => 0`.
    #[test]
    fn exiftool_jpg_retains_both_comment_sources() {
        let n = occurrence_count(
            "/tmp/oxidex-exiftool-cache/exiftool/t/images/ExifTool.jpg",
            "File:Comment",
        );
        if n == usize::MAX {
            return;
        }
        assert_eq!(n, 2);
    }

    /// One `tkhd` per track; each is a `TrackID` occurrence, and only
    /// `Track1`'s wins the bare key (`TagSink::record`'s DOC_NUM guard).
    #[test]
    fn quicktime_mov_retains_both_track_ids() {
        let n = occurrence_count(
            "/tmp/oxidex-exiftool-cache/exiftool/t/images/QuickTime.mov",
            "QuickTime:TrackID",
        );
        if n == usize::MAX {
            return;
        }
        assert_eq!(n, 2);
    }

    #[test]
    fn canonraw_cr3_retains_all_four_track_ids() {
        let n = occurrence_count(
            "/tmp/oxidex-exiftool-cache/exiftool/t/images/CanonRaw.cr3",
            "QuickTime:TrackID",
        );
        if n == usize::MAX {
            return;
        }
        assert_eq!(n, 4);
    }

    /// Pentax's 0x003f `LensRec` + 0x0207 `LensInfo` duplicate pair, and
    /// the 0x0005 Main + 0x0215 `CameraInfo` `PentaxModelID` pair.
    #[test]
    fn pentax_jpg_retains_lens_type_and_model_id_duplicates() {
        let root = "/tmp/oxidex-exiftool-cache/exiftool/t/images/Pentax.jpg";
        let lens = occurrence_count(root, "Pentax:LensType");
        let model = occurrence_count(root, "Pentax:PentaxModelID");
        if lens == usize::MAX {
            return;
        }
        assert_eq!(lens, 2);
        assert_eq!(model, 2);
    }

    #[test]
    fn pentax_avi_retains_lens_type_and_model_id_duplicates() {
        let root = "/tmp/oxidex-exiftool-cache/exiftool/t/images/Pentax.avi";
        let lens = occurrence_count(root, "Pentax:LensType");
        let model = occurrence_count(root, "Pentax:PentaxModelID");
        if lens == usize::MAX {
            return;
        }
        assert_eq!(lens, 2);
        assert_eq!(model, 2);
    }
}
