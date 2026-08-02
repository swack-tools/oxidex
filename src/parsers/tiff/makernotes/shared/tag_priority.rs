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
}
