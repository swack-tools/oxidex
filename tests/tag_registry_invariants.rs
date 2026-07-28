//! Guards against PrintConv display values being listed as tags.
//!
//! `exiftool -f -listx` nests a tag's PrintConv table inside the tag:
//!
//! ```xml
//! <tag id='41986' name='ExposureMode'>
//!   <values>
//!     <key id='1'><val lang='en'>Manual</val></key>
//!     <key id='2'><val lang='en'>Auto bracket</val></key>
//!   </values>
//! </tag>
//! ```
//!
//! Reading every element with an `id` attribute as a tag turns those `<key>`
//! rows into sibling *tag* entries, which is how the registry came to hold
//! 16,005 of them — ids restarting at `0x0001` mid-table because they are
//! PrintConv keys, not tag ids.
//!
//! They did not reach output, but only because `src/tag_db/mod.rs` grew a
//! hand-maintained blocklist (`is_valid_tag_name`) that filters names like
//! `Manual` and `Portrait` back out at index-build time. These tests exist so
//! the data stays correct at the source rather than being suppressed downstream
//! by a list that has to keep growing.
//!
//! Display strings belong in the enum decoders that already own them (see
//! `1 => "Reduced-resolution image"` in src/parsers/tiff/tiff_enums.rs), not
//! in the tag registry.

use oxidex::tag_sync::{
    DOMAINS, parse_listx, parse_listx_print_conv_keys, parse_listx_print_conv_values,
    parse_tag_id as parse_id,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One `(table, id, name)` row of a domain YAML.
struct Entry {
    table: String,
    id: String,
    name: String,
    line: usize,
}

fn domain_yaml(domain: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("oxidex-tags-{domain}/src/{domain}_tags.yaml"))
}

/// Deliberately a line scanner rather than `serde_yaml` + the crate's own
/// types: a guard that deserialized through the code it guards would inherit
/// any bug in that path, and it reports line numbers for free.
fn entries(domain: &str) -> Vec<Entry> {
    let path = domain_yaml(domain);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    let mut out = Vec::new();
    let mut table = String::new();
    let mut id = String::new();
    for (idx, line) in text.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("  - name: ") {
            table = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("      - id: ") {
            id = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("        name: ") {
            out.push(Entry {
                table: table.clone(),
                id: id.clone(),
                name: rest.trim().trim_matches('"').to_string(),
                line: idx + 1,
            });
        }
    }
    assert!(!out.is_empty(), "{} parsed as empty", path.display());
    out
}

fn exiftool_listx() -> Option<String> {
    let out = Command::new("exiftool")
        .arg("-f")
        .arg("-listx")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Every one of ExifTool's ~32k tag names matches `[A-Za-z0-9_][A-Za-z0-9_-]*`.
/// A name carrying a space, colon, comma, arrow or parenthesis is therefore a
/// display value or a Composite dependency reference, never a tag.
fn is_shaped_like_a_tag_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[test]
fn registry_tag_names_are_shaped_like_exiftool_tag_names() {
    let mut violations = Vec::new();
    for domain in DOMAINS {
        for e in entries(domain) {
            if !is_shaped_like_a_tag_name(&e.name) {
                violations.push(format!(
                    "  {domain}_tags.yaml:{} {} id={} name={:?}",
                    e.line, e.table, e.id, e.name
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "{} registry entr(ies) are named like display values, not tags.\n\
         PrintConv display strings belong in the enum decoders, not the tag \
         registry:\n{}",
        violations.len(),
        violations.join("\n")
    );
}

#[test]
fn registry_lists_no_print_conv_display_value_as_a_tag() {
    let Some(xml) = exiftool_listx() else {
        eprintln!("skipping: exiftool not found on PATH");
        return;
    };

    let real: HashSet<(String, String)> = parse_listx(&xml)
        .expect("parse_listx")
        .into_iter()
        .map(|t| (t.table, t.name))
        .collect();
    let values = parse_listx_print_conv_values(&xml).expect("parse_listx_print_conv_values");

    // A name is only damning when it is a display value of *its own* table and
    // is not also a tag there. Restricting to the same table avoids convicting
    // a legitimate tag that happens to share a word with some other table's
    // PrintConv (e.g. `Compression` is both a real EXIF tag and a value
    // elsewhere).
    let mut violations = Vec::new();
    for domain in DOMAINS {
        for e in entries(domain) {
            if real.contains(&(e.table.clone(), e.name.clone())) {
                continue;
            }
            let is_display_value = values
                .get(&e.table)
                .is_some_and(|v: &HashSet<String>| v.contains(&e.name));
            if is_display_value {
                violations.push(format!(
                    "  {domain}_tags.yaml:{} {} id={} name={:?} is a PrintConv \
                     display value of that table, not a tag",
                    e.line, e.table, e.id, e.name
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "{} registry entr(ies) are PrintConv display values:\n{}",
        violations.len(),
        violations.join("\n")
    );
}

/// The subtlest shape of the same bug, and the one that actually misnames tags.
///
/// A display value can collide with a real tag of its own table, so the name
/// check above waves it through. `Exif::Main` has a genuine `Saturation` tag at
/// 0xA409 *and* carries `RenderingIntent`'s PrintConv value 2, also spelled
/// "Saturation" — which landed in the registry as `id: "0x0002"`. Since
/// `src/tag_db/mod.rs` builds its id→name index straight from these ids, such
/// an entry can claim a tag id that belongs to something else entirely.
///
/// Conviction requires proof, because a plain id disagreement is usually a
/// benign encoding difference: ExifTool packs a property-set index into the
/// high word for FlashPix (`0x10000`) and LNK (`0x30004`) where the registry
/// stores the low word, and there are 260 of those. So an entry is only
/// reported when its id matches no real id for that name, *and* that same id,
/// read as a PrintConv key of that table, maps to that very name.
#[test]
fn registry_tag_ids_are_not_print_conv_keys_in_disguise() {
    let Some(xml) = exiftool_listx() else {
        eprintln!("skipping: exiftool not found on PATH");
        return;
    };

    let mut real_ids: HashMap<(String, String), HashSet<i64>> = HashMap::new();
    for t in parse_listx(&xml).expect("parse_listx") {
        if let Some(id) = parse_id(&t.id) {
            real_ids.entry((t.table, t.name)).or_default().insert(id);
        }
    }
    let keys = parse_listx_print_conv_keys(&xml).expect("parse_listx_print_conv_keys");

    let mut violations = Vec::new();
    for domain in DOMAINS {
        for e in entries(domain) {
            let (Some(here), Some(wanted)) = (
                parse_id(&e.id),
                real_ids.get(&(e.table.clone(), e.name.clone())),
            ) else {
                continue;
            };
            if wanted.contains(&here) {
                continue;
            }
            let shadows = keys
                .get(&e.table)
                .and_then(|m| m.get(&here))
                .is_some_and(|v| v.contains(&e.name));
            if shadows {
                violations.push(format!(
                    "  {domain}_tags.yaml:{} {} name={:?} claims id={}, but that is a \
                     PrintConv key mapping to {:?}; the real tag is at {:?}",
                    e.line,
                    e.table,
                    e.name,
                    e.id,
                    e.name,
                    wanted
                        .iter()
                        .map(|i| format!("0x{i:X}"))
                        .collect::<Vec<_>>()
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "{} registry entr(ies) carry a PrintConv key as their tag id:\n{}",
        violations.len(),
        violations.join("\n")
    );
}
