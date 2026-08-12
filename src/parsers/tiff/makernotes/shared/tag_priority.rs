//! ExifTool's tag priority, for the case where one file reports the same tag
//! name twice.
//!
//! A MakerNote routinely carries the same tag in two places: once in the
//! vendor's `Main` table and once inside a binary sub-directory. ExifTool keeps
//! both -- they are separate tag keys, `LensType` and `LensType (1)` -- but only
//! one of them is the tag that prints under the plain name, and `Priority`
//! decides which. oxidex reports one value per `Group:Name`, so the same
//! decision has to be made at insert time.
//!
//! # ExifTool's rule
//!
//! `Image::ExifTool::FoundTag` (`ExifTool.pm:9448`) reads the tag's priority at
//! `:9469-9473` -- the tag's own `Priority`, else the table's `PRIORITY`, else
//! `0` when the tag is marked `Avoid`. When the name is already present
//! (`:9515`) it compares against the priority already recorded for it:
//!
//! ```text
//! my $oldPriority = $$self{PRIORITY}{$tag};      # ExifTool.pm:9544
//! unless ($oldPriority) { ... $oldPriority = 1; } #             :9545-9551
//! ...
//! if ($priority >= $oldPriority and ...) {        #             :9564
//!     # the new value takes the plain name, the old one moves to "$tag (1)"
//! } else {
//!     $tag = $nextTag;                            #             :9585
//!     # the new value goes to "$tag (1)"; the old one keeps the plain name
//! }
//! ```
//!
//! Two details make the rule what it is. A value stored with priority 0 records
//! no entry in `PRIORITY` at all (`:9589` stores it "only if exists and is
//! non-zero"), and a missing entry is *promoted to 1* at `:9545-9551` -- the
//! comment there reads "promote existing 0-priority tag so it takes precedence
//! over a new 0-tag". So the comparison at `:9564` resolves the four possible
//! orderings like this:
//!
//! | first | second | winner |
//! |-------|--------|--------|
//! | normal | `Priority => 0` | first — `0 >= 1` is false |
//! | `Priority => 0` | normal | second — old promoted to 1, `1 >= 1` holds |
//! | `Priority => 0` | `Priority => 0` | first — old promoted to 1, `0 >= 1` is false |
//! | normal | normal | second — `1 >= 1` holds |
//!
//! Which is exactly two rules, and they compose to the same outcome for any
//! number of instances in any order:
//!
//! * a `Priority => 0` value never displaces a value already present;
//! * a normal-priority value always displaces whatever is present.
//!
//! The second is what a plain [`HashMap::insert`] already does, so only the
//! first needs a function. Writing a `Priority => 0` tag with `insert` is the
//! bug this exists to prevent: it does not fail, it does not drop a tag, it
//! silently prints the sub-directory's value under a real ExifTool tag name.
//!
//! [`HashMap::insert`]: std::collections::HashMap::insert

use std::collections::HashMap;

/// Records a value ExifTool declares `Priority => 0`.
///
/// Keeps whatever is already under `key` and discards `value`; stores `value`
/// only when the name has not been reported yet. See the module documentation
/// for why this, plus an ordinary `insert` for every normal-priority tag,
/// reproduces `FoundTag`'s comparison exactly.
pub(crate) fn insert_low_priority(tags: &mut HashMap<String, String>, key: String, value: String) {
    tags.entry(key).or_insert(value);
}

/// Like [`insert_low_priority`], but for callers migrated to Step 19's real
/// occurrence retention (`OVERHAUL_STEP18_DESIGN.md` §2.3 Phase B):
/// currently just Pentax's `LensType`/`LensFocalLength`/`PentaxModelID`
/// duplicate pairs (`pentax.rs`).
///
/// A shadowed value is not discarded outright as [`insert_low_priority`]
/// does -- it is stashed under a synthetic `"<key> (N)"` companion key,
/// mirroring `FoundTag`'s own `"$tag ($nextInd)"` duplicate-key convention
/// (`ExifTool.pm:9532`). The `HashMap<String, String>` this trait's callers
/// pass has no way to hold two values under one key, so the companion key is
/// the seam: `tiff_helpers.rs`'s makernote merge recognizes it and records
/// the shadowed value as a real, always-losing `TagOccurrence` under its
/// base key instead of literally inserting a garbage `"Tag (N)"` tag name.
///
/// Every other `MakerNoteParser` -- everyone except Pentax -- keeps calling
/// [`insert_low_priority`] unchanged (`shared/binary_subdir.rs`'s generic
/// dispatch, in particular), so this function's behavior never reaches any
/// other manufacturer's output.
pub(crate) fn insert_low_priority_retained(
    tags: &mut HashMap<String, String>,
    key: String,
    value: String,
) {
    if !tags.contains_key(&key) {
        tags.insert(key, value);
        return;
    }
    let mut n = 1u32;
    while tags.contains_key(&format!("{key} ({n})")) {
        n += 1;
    }
    tags.insert(format!("{key} ({n})"), value);
}

/// The other half of [`insert_low_priority_retained`]: records one
/// `MakerNoteParser`-produced `(tag_name, value)` pair into a real
/// `MetadataMap`, recognizing the `"<key> (N)"` synthetic duplicate marker
/// and routing it to [`crate::core::MetadataMap::insert_occurrence`] as a
/// real, always-losing `TagOccurrence` under its base key -- instead of what
/// every makernote-merge call site used to do unconditionally, which would
/// otherwise literally insert a tag named e.g. `"Pentax:LensType (1)"` that
/// no ExifTool output ever produces.
///
/// Every non-Pentax manufacturer's tags pass straight through unaffected:
/// only [`insert_low_priority_retained`] ever mints a `"(N)"`-suffixed key,
/// and only Pentax's four call sites (`pentax.rs`) use it, so
/// [`strip_duplicate_marker`] never matches anything else's tag name.
///
/// Every makernote-merge call site in the tree (JPEG/TIFF's
/// `tiff_helpers.rs`, `avi.rs`'s Pentax `hymn`/`mknt` chunks, and RAW's
/// `DNGPrivateData` MakN record) goes through this rather than a bare
/// `metadata.insert()`, because Pentax MakerNotes can reach any of them.
pub(crate) fn record_makernote_tag(
    metadata: &mut crate::core::MetadataMap,
    tag_name: String,
    tag_value: crate::core::TagValue,
) {
    if let Some(base) = strip_duplicate_marker(&tag_name) {
        metadata.insert_occurrence(base, tag_value, 0, "", crate::core::Instance::default());
    } else {
        metadata.insert(tag_name, tag_value);
    }
}

/// Recognizes [`insert_low_priority_retained`]'s `"<key> (N)"` synthetic
/// duplicate-marker convention and returns the base key, or `None` if
/// `tag_name` carries no such marker.
fn strip_duplicate_marker(tag_name: &str) -> Option<&str> {
    let rest = tag_name.strip_suffix(')')?;
    let paren = rest.rfind(" (")?;
    let (base, digits) = rest.split_at(paren);
    let digits = &digits[2..];
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        Some(base)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// `normal` then `Priority => 0`: `0 >= 1` is false, so the first value
    /// keeps the plain tag name (ExifTool.pm:9564, :9585).
    #[test]
    fn low_priority_does_not_displace_a_present_value() {
        let mut tags = map(&[("Pentax:LensType", "smc PENTAX-DA 21mm F3.2 AL Limited")]);
        insert_low_priority(
            &mut tags,
            "Pentax:LensType".to_string(),
            "M-42 or No Lens".to_string(),
        );
        assert_eq!(
            tags["Pentax:LensType"],
            "smc PENTAX-DA 21mm F3.2 AL Limited"
        );
    }

    /// Two `Priority => 0` instances: the first is promoted to 1 when the
    /// second arrives, so the first still wins (ExifTool.pm:9541-9551).
    #[test]
    fn first_of_two_low_priority_values_wins() {
        let mut tags = HashMap::new();
        insert_low_priority(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );
        assert_eq!(tags["Pentax:LensType"], "first");
    }

    /// A `Priority => 0` value is still the reported one when nothing else
    /// reports the tag -- the rule suppresses a clobber, not the tag.
    #[test]
    fn low_priority_value_is_kept_when_it_is_the_only_one() {
        let mut tags = HashMap::new();
        insert_low_priority(
            &mut tags,
            "Pentax:PentaxModelID".to_string(),
            "Optio SV".to_string(),
        );
        assert_eq!(tags["Pentax:PentaxModelID"], "Optio SV");
    }

    /// `Priority => 0` then normal: the stored value recorded no priority, so
    /// it is promoted to 1 and the normal value's `1 >= 1` displaces it
    /// (ExifTool.pm:9545-9551, :9564). That is a plain `insert`, and this test
    /// pins that the helper does not turn it into an `or_insert` too.
    #[test]
    fn a_normal_priority_value_still_displaces_a_low_priority_one() {
        let mut tags = HashMap::new();
        insert_low_priority(&mut tags, "Pentax:LensType".to_string(), "low".to_string());
        tags.insert("Pentax:LensType".to_string(), "normal".to_string());
        assert_eq!(tags["Pentax:LensType"], "normal");
    }

    #[test]
    fn insert_low_priority_retained_keeps_the_winner_at_the_bare_key() {
        let mut tags = HashMap::new();
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );
        assert_eq!(tags["Pentax:LensType"], "first");
    }

    #[test]
    fn insert_low_priority_retained_stashes_the_shadowed_value_under_a_companion_key() {
        let mut tags = HashMap::new();
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );
        assert_eq!(
            tags.get("Pentax:LensType (1)").map(String::as_str),
            Some("second")
        );
    }

    #[test]
    fn insert_low_priority_retained_numbers_a_third_shadowed_value_distinctly() {
        let mut tags = HashMap::new();
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "third".to_string(),
        );
        assert_eq!(tags["Pentax:LensType"], "first");
        assert_eq!(
            tags.get("Pentax:LensType (1)").map(String::as_str),
            Some("second")
        );
        assert_eq!(
            tags.get("Pentax:LensType (2)").map(String::as_str),
            Some("third")
        );
    }

    #[test]
    fn insert_low_priority_retained_shadows_a_value_an_earlier_plain_insert_established() {
        // PentaxModelID's shape: the normal (0x0005) copy is a plain
        // `tags.insert`, and the low-priority (0x0215 CameraInfo) copy
        // arrives after it and must still be retained, not silently
        // dropped, even though it never displaces the winner.
        let mut tags = HashMap::new();
        tags.insert("Pentax:PentaxModelID".to_string(), "K10D".to_string());
        insert_low_priority_retained(
            &mut tags,
            "Pentax:PentaxModelID".to_string(),
            "K10D".to_string(),
        );
        assert_eq!(tags["Pentax:PentaxModelID"], "K10D");
        assert_eq!(
            tags.get("Pentax:PentaxModelID (1)").map(String::as_str),
            Some("K10D")
        );
    }

    #[test]
    fn strip_duplicate_marker_recognizes_the_synthetic_key() {
        assert_eq!(
            strip_duplicate_marker("Pentax:LensType (1)"),
            Some("Pentax:LensType")
        );
        assert_eq!(
            strip_duplicate_marker("Pentax:LensType (12)"),
            Some("Pentax:LensType")
        );
    }

    #[test]
    fn strip_duplicate_marker_ignores_ordinary_tag_names() {
        assert_eq!(strip_duplicate_marker("Pentax:LensType"), None);
        assert_eq!(strip_duplicate_marker("Pentax:LensType ()"), None);
        assert_eq!(strip_duplicate_marker("Pentax:LensType (x)"), None);
    }

    #[test]
    fn record_makernote_tag_ends_a_full_pentax_duplicate_pair_at_the_real_key_only() {
        let mut metadata = crate::core::MetadataMap::new();
        let mut tags = HashMap::new();
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );

        for (tag_name, value) in tags {
            record_makernote_tag(
                &mut metadata,
                tag_name,
                crate::core::TagValue::String(value),
            );
        }

        assert_eq!(metadata.get_string("Pentax:LensType"), Some("first"));
        assert!(
            metadata.get_string("Pentax:LensType (1)").is_none(),
            "the synthetic marker key must never become a real tag name"
        );
        assert_eq!(metadata.occurrences_for("Pentax:LensType").len(), 2);
    }
}
