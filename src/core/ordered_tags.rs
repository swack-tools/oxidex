//! An insertion-ordered, string-keyed map for a parser's own tag accumulator.
//!
//! A parser that walks a directory in file order and collects what it finds
//! into a `HashMap<String, _>` loses that order: std seeds every `HashMap`'s
//! iteration order afresh, so the loop that later records the tags into a
//! [`MetadataMap`](super::MetadataMap) stamps them in a different order on
//! every run, and `-a` renders occurrences in the order they were stamped.
//! [`OrderedTags`] keeps the order the parser found them in -- ExifTool's own
//! file order -- while keeping `HashMap::insert`'s one-value-per-key rule: a
//! repeated key keeps its first position and takes the newer value, so the
//! set of tags and their winning values are exactly what the `HashMap` held.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

/// String keys to values, iterated in first-insertion order.
///
/// Lookups go through a hash index, so hostile input with many keys stays
/// linear rather than turning every insert into a scan.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderedTags<V> {
    entries: Vec<(String, V)>,
    index: HashMap<String, usize>,
}

impl<V> Default for OrderedTags<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<V> OrderedTags<V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// `HashMap::insert`: a new key is appended; a repeated key keeps its
    /// position, takes `value`, and the previous value is returned.
    pub fn insert(&mut self, key: String, value: V) -> Option<V> {
        match self.index.entry(key) {
            Entry::Occupied(slot) => {
                Some(std::mem::replace(&mut self.entries[*slot.get()].1, value))
            }
            Entry::Vacant(slot) => {
                self.entries.push((slot.key().clone(), value));
                slot.insert(self.entries.len() - 1);
                None
            }
        }
    }

    /// `HashMap::entry(key).or_insert_with(default)`.
    pub fn get_or_insert_with(&mut self, key: String, default: impl FnOnce() -> V) -> &mut V {
        let idx = match self.index.entry(key) {
            Entry::Occupied(slot) => *slot.get(),
            Entry::Vacant(slot) => {
                self.entries.push((slot.key().clone(), default()));
                *slot.insert(self.entries.len() - 1)
            }
        };
        &mut self.entries[idx].1
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.index.get(key).map(|&idx| &self.entries[idx].1)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry, in first-insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}

impl<V> IntoIterator for OrderedTags<V> {
    type Item = (String, V);
    type IntoIter = std::vec::IntoIter<(String, V)>;

    /// Every entry, in first-insertion order.
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a, V> IntoIterator for &'a OrderedTags<V> {
    type Item = (&'a String, &'a V);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (String, V)>,
        fn(&'a (String, V)) -> (&'a String, &'a V),
    >;

    /// Every entry, in first-insertion order.
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}

impl<V> Extend<(String, V)> for OrderedTags<V> {
    fn extend<T: IntoIterator<Item = (String, V)>>(&mut self, iter: T) {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }
}

impl<V> FromIterator<(String, V)> for OrderedTags<V> {
    fn from_iter<T: IntoIterator<Item = (String, V)>>(iter: T) -> Self {
        let mut tags = Self::new();
        tags.extend(iter);
        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterates_in_first_insertion_order() {
        let mut tags = OrderedTags::new();
        for (key, value) in [("Zeta", 1), ("Alpha", 2), ("Mu", 3)] {
            tags.insert(key.to_string(), value);
        }
        let keys: Vec<&str> = tags.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["Zeta", "Alpha", "Mu"]);
        let owned: Vec<(String, i32)> = tags.into_iter().collect();
        assert_eq!(owned[2], ("Mu".to_string(), 3));
    }

    /// A repeated key behaves as `HashMap::insert` does -- one entry, the
    /// newer value, the old one returned -- and keeps its first position.
    #[test]
    fn a_repeated_key_keeps_its_position_and_takes_the_newer_value() {
        let mut tags = OrderedTags::new();
        tags.insert("A".to_string(), "first");
        tags.insert("B".to_string(), "b");
        assert_eq!(tags.insert("A".to_string(), "second"), Some("first"));
        assert_eq!(tags.len(), 2);
        assert_eq!(tags.get("A"), Some(&"second"));
        let pairs: Vec<(&str, &str)> = tags.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(pairs, [("A", "second"), ("B", "b")]);
    }

    #[test]
    fn get_or_insert_with_creates_once_then_returns_the_same_slot() {
        let mut tags: OrderedTags<Vec<&str>> = OrderedTags::new();
        tags.get_or_insert_with("K".to_string(), Vec::new).push("x");
        tags.get_or_insert_with("J".to_string(), Vec::new).push("y");
        tags.get_or_insert_with("K".to_string(), Vec::new).push("z");
        let pairs: Vec<(&str, &Vec<&str>)> = tags.iter().map(|(k, v)| (k.as_str(), v)).collect();
        assert_eq!(pairs, [("K", &vec!["x", "z"]), ("J", &vec!["y"])]);
    }

    #[test]
    fn collect_and_extend_follow_insert() {
        let tags: OrderedTags<i32> = [("B", 1), ("A", 2), ("B", 3)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let pairs: Vec<(&str, i32)> = tags.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(pairs, [("B", 3), ("A", 2)]);
        assert!(tags.contains_key("A"));
        assert!(!tags.contains_key("C"));
    }
}
