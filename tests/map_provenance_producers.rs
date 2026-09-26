//! Tripwire for per-occurrence provenance (#949 review): every public
//! function that hands out a `MetadataMap` built from a file must mark its
//! rows read, or a caller's untouched rows look like assignments to the
//! writers (an XP string re-encoded, a removal skipped). Readers record
//! through the same `MetadataMap::insert` a caller's assignment uses, so the
//! marking is each producer's job: its body runs inside
//! `crate::core::metadata_map::file_rows`, or it calls
//! `mark_read_complete` itself.
//!
//! This scans `src/` for every `pub fn ... -> Result<MetadataMap>` /
//! `-> MetadataMap` and every `FormatParser::parse` impl, and fails naming
//! each one that does neither. A function that returns a copy or a
//! projection of a map it was given (never a file's rows) is listed in
//! `DERIVED` with the reason; it carries each row's own provenance instead.

use regex::Regex;
use std::path::Path;

/// Public functions that return a map derived from a map the caller gave
/// them, not from a file: they carry each row's provenance
/// (`MetadataMap::copy_provenance_from`, `set_last_assigned`, a clone).
const DERIVED: &[(&str, &str)] = &[
    ("format_for_exiftool", "formatted copy of its input"),
    ("strip_extended_only", "filtered copy of its input"),
    ("filter_tiff_writable_tags", "filtered copy of its input"),
    ("normalize_metadata_map", "renamed copy of its input"),
    ("without_print_conv", "ValueConv projection of its input"),
    ("into_map", "moves the wrapped map out"),
    ("into_metadata", "moves the report's map out"),
    ("new", "empty map"),
    ("with_capacity", "empty map"),
];

/// Producers whose marking is not in the first lines of their body: they
/// delegate to a marking producer, or mark on their way out.
const MARKED_ELSEWHERE: &[(&str, &str)] = &[
    (
        "read_metadata",
        "delegates to read_metadata_with_detector_and_options, which marks",
    ),
    (
        "read_metadata_with_detector",
        "delegates to read_metadata_with_detector_and_options, which marks",
    ),
    (
        "build_display_map",
        "a display projection; marks read before it returns",
    ),
];

#[test]
fn every_public_map_producer_marks_its_rows_read() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let producer = Regex::new(
        r"(?m)^[ \t]*pub fn (\w+)(?:<[^>]*>)?\s*\([^{;]*?\)\s*->\s*(?:crate::error::)?(?:Result<\s*(?:crate::core::)?MetadataMap\s*>|(?:crate::core::)?MetadataMap)\s*\{",
    )
    .unwrap();
    let trait_impl = Regex::new(
        r"(?m)^[ \t]*fn parse\(&self, reader: &dyn FileReader\) -> Result<MetadataMap>\s*\{",
    )
    .unwrap();
    let marks = |body: &str| body.contains("file_rows(") || body.contains("mark_read_complete()");
    let mut unmarked = Vec::new();
    let mut seen = 0;
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.unwrap();
        if entry.path().extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let rel = entry
            .path()
            .strip_prefix(&root)
            .unwrap()
            .display()
            .to_string();
        for (name, start) in producer
            .captures_iter(&text)
            .map(|c| (c[1].to_string(), c.get(0).unwrap().end()))
            .chain(
                trait_impl
                    .find_iter(&text)
                    .map(|m| ("FormatParser::parse".to_string(), m.end())),
            )
        {
            seen += 1;
            if DERIVED
                .iter()
                .chain(MARKED_ELSEWHERE)
                .any(|(listed, _)| *listed == name)
            {
                continue;
            }
            let body = &text[start..(start + 600).min(text.len())];
            if !marks(body) {
                unmarked.push(format!("{rel}: {name}"));
            }
        }
    }
    assert!(seen > 100, "the scan found only {seen} producers");
    assert!(
        unmarked.is_empty(),
        "public producers of file rows that do not mark them read \
         (wrap the body in `crate::core::metadata_map::file_rows`):\n{}",
        unmarked.join("\n")
    );
}
