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
    sink: TagSink,

    /// Full-precision `ValueConv` forms used by derived tags.
    ///
    /// ExifTool keeps a tag's converted value separate from its `PrintConv`
    /// display. Most OxiDex tags need only one form, but a rounded display such
    /// as Nikon's `FocusDistance` must not be fed back into DOF arithmetic.
    /// This sidecar is deliberately private and skipped by serde: it augments
    /// an existing tag and is never itself an emitted metadata tag.
    ///
    /// Design decision D4 (Step 18): left alone in Phase A rather than
    /// folded into `TagOccurrence.value`, since doing so would touch the
    /// Composite layer this sidecar feeds and break the purely-additive
    /// property the A/B gate depends on. Step 22 is where this sidecar
    /// retires.
    value_forms: HashMap<String, String>,
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
            value_forms: HashMap::new(),
        }
    }

    /// Creates a new MetadataMap with the specified capacity
    ///
    /// This pre-allocates space for at least `capacity` tags, which can
    /// improve performance when the approximate number of tags is known.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            sink: TagSink::with_capacity(capacity),
            value_forms: HashMap::new(),
        }
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
        // to its predecessor. Parsers attach a new form explicitly afterwards.
        self.value_forms.remove(&key);
        let previous = self.sink.get(&key).cloned();
        let order = self.sink.next_order();
        let occurrence = TagOccurrence::from_insert_shim(&key, value, order);
        self.sink.record(key, occurrence);
        previous
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
        self.value_forms.remove(&key);
        let previous = self.sink.get(&key).cloned();
        let order = self.sink.next_order();
        let mut occurrence = TagOccurrence::from_insert_shim(&key, value, order);
        occurrence.priority = priority;
        occurrence.group1 = super::tag_occurrence::intern(group1);
        occurrence.instance = instance;
        self.sink.record(key, occurrence);
        previous
    }

    /// Every occurrence recorded for `key`, winners and losers alike, in
    /// file order. Exists to let Step 19's migrated call sites verify real
    /// duplicate retention (`cargo test --lib`) without a `-a` output mode,
    /// which is Step 20+'s job; nothing in default output reads this.
    #[cfg(test)]
    pub(crate) fn occurrences_for(&self, key: &str) -> Vec<&TagOccurrence> {
        self.sink
            .occurrences()
            .iter()
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
        self.sink.occurrences().iter().map(|o| (o.lookup_key(), o))
    }

    /// Attaches a full-precision value form to an existing visible tag.
    ///
    /// The value is intentionally absent from iteration and serialization. It
    /// is currently consumed only by the Composite layer.
    pub(crate) fn set_value_form<K: Into<String>, V: Into<String>>(&mut self, key: K, value: V) {
        let key = key.into();
        if self.sink.contains_key(&key) {
            self.value_forms.insert(key, value.into());
        }
    }

    /// Returns the full-precision value form attached to `key`, if any.
    pub(crate) fn value_form(&self, key: &str) -> Option<&str> {
        self.value_forms.get(key).map(String::as_str)
    }

    /// Merges another map while retaining private value forms.
    ///
    /// Every occurrence from `other` -- not just its winner projection -- is
    /// replayed into `self`'s own sink via
    /// [`TagSink::record_carrying_over`], preserving each occurrence's
    /// priority, family-1 group and instance rather than flattening it back
    /// to `insert()`'s `SHIM_DEFAULT_PRIORITY`/`Instance::default()`.
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
    /// occurrence reachable afterward.
    pub(crate) fn merge(&mut self, other: MetadataMap) {
        let MetadataMap { sink, value_forms } = other;
        for occurrence in sink.into_occurrences() {
            self.sink.record_carrying_over(occurrence);
        }
        for (key, value) in value_forms {
            self.set_value_form(key, value);
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
        self.value_forms.remove(key);
        self.sink.get_mut(key)
    }

    /// Removes a tag from the map
    ///
    /// Returns the value if the tag existed, `None` otherwise.
    pub fn remove(&mut self, key: &str) -> Option<TagValue> {
        self.value_forms.remove(key);
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
        self.value_forms.clear();
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
    type IntoIter = std::collections::hash_map::IntoIter<String, TagValue>;

    fn into_iter(self) -> Self::IntoIter {
        // Only the winner projection is consumed; losing occurrences (none
        // exist yet outside this module's own tests, since nothing else
        // constructs duplicates) are dropped along with the rest of the sink.
        self.sink.into_winner_map().into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metadata_map() {
        let map = MetadataMap::new();
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
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
