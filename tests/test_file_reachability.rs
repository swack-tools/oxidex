//! Guards against test files that are never compiled.
//!
//! Cargo builds only `tests/*.rs` as test roots. Files in subdirectories are
//! reached solely through `mod` / `#[path]` declarations, so a `.rs` file under
//! `tests/` that nothing declares is silently ignored: it never compiles, its
//! `#[test]` functions never run, and it is indistinguishable from a comment.
//!
//! This is not hypothetical. 21 files holding 114 `#[test]` functions sat
//! undeclared in this repository, including vendor MakerNote suites asserting
//! tag names that do not exist in ExifTool. Because they never built, nothing
//! ever contradicted them.
//!
//! This test re-derives the reachable set from the roots and fails if any file
//! under `tests/` is orphaned. If you add a file under `tests/`, declare it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Strip `//` line comments and `/* */` block comments so that commented-out
/// `mod` declarations are not treated as live edges.
fn strip_comments(src: &str) -> String {
    let bytes: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < bytes.len() {
        // String literals are skipped verbatim: a `//` inside one is not a comment.
        if bytes[i] == '"' {
            out.push(bytes[i]);
            i += 1;
            while i < bytes.len() {
                if bytes[i] == '\\' && i + 1 < bytes.len() {
                    out.push(bytes[i]);
                    out.push(bytes[i + 1]);
                    i += 2;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
                if bytes[i - 1] == '"' {
                    break;
                }
            }
            continue;
        }
        if bytes[i] == '/' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == '/' && i + 1 < bytes.len() && bytes[i + 1] == '*' {
            i += 2;
            let mut depth = 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == '/' && i + 1 < bytes.len() && bytes[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == '*' && i + 1 < bytes.len() && bytes[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Extract `(optional #[path] value, module name)` for each `mod NAME;` in `src`.
///
/// Handles `#[path = "..."]` on its own line or inline, with or without `pub`,
/// and ignores inline `mod NAME { .. }` blocks (which declare no new file).
fn module_edges(src: &str) -> Vec<(Option<String>, String)> {
    let mut edges = Vec::new();
    let mut pending_path: Option<String> = None;

    for raw in src.lines() {
        let line = raw.trim();

        if let Some(rest) = line.strip_prefix("#[path") {
            if let Some(start) = rest.find('"')
                && let Some(len) = rest[start + 1..].find('"')
            {
                pending_path = Some(rest[start + 1..start + 1 + len].to_string());
            }
            // A `#[path = ".."] mod name;` may share the line with the mod item.
            if !line.contains("mod ") {
                continue;
            }
        }

        let after_vis = line.strip_prefix("pub ").unwrap_or(line);
        if let Some(rest) = after_vis
            .split("mod ")
            .nth(1)
            .filter(|_| after_vis.contains("mod "))
        {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let tail = rest[name.len()..].trim_start();
            // Only file-backed declarations (`mod name;`), not `mod name { .. }`.
            if !name.is_empty() && tail.starts_with(';') {
                edges.push((pending_path.take(), name));
                continue;
            }
        }

        // An attribute followed by something that is not a `mod` item: drop it.
        if !line.is_empty() && !line.starts_with("#[") && !line.starts_with("//") {
            pending_path = None;
        }
    }
    edges
}

fn visit(tests_dir: &Path, rel: &Path, reachable: &mut BTreeSet<PathBuf>) {
    if !reachable.insert(rel.to_path_buf()) {
        return;
    }
    let abs = tests_dir.join(rel);
    let Ok(src) = fs::read_to_string(&abs) else {
        return;
    };
    let src = strip_comments(&src);

    let parent = rel.parent().unwrap_or(Path::new(""));
    let stem = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    // Non-`mod.rs` files own a same-named directory for their children.
    let child_dir = if stem == "mod" {
        parent.to_path_buf()
    } else {
        parent.join(stem)
    };

    for (path_attr, name) in module_edges(&src) {
        let candidate = if let Some(p) = path_attr {
            // `#[path]` is relative to the *containing directory* of the file.
            Some(normalize(&parent.join(p)))
        } else {
            let file = child_dir.join(format!("{name}.rs"));
            let dir_mod = child_dir.join(&name).join("mod.rs");
            if tests_dir.join(&file).is_file() {
                Some(file)
            } else if tests_dir.join(&dir_mod).is_file() {
                Some(dir_mod)
            } else {
                None
            }
        };
        if let Some(c) = candidate
            && tests_dir.join(&c).is_file()
        {
            visit(tests_dir, &c, reachable);
        }
    }
}

/// Collapse `a/../b` into `b` without touching the filesystem.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn collect_rs(dir: &Path, base: &Path, into: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, base, into);
        } else if path.extension().is_some_and(|e| e == "rs")
            && let Ok(rel) = path.strip_prefix(base)
        {
            into.insert(rel.to_path_buf());
        }
    }
}

#[test]
fn every_test_file_is_reachable_from_a_cargo_test_root() {
    let tests_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");

    let mut all = BTreeSet::new();
    collect_rs(&tests_dir, &tests_dir, &mut all);
    assert!(
        all.len() > 20,
        "sanity: expected to find many test files under {}, found {}",
        tests_dir.display(),
        all.len()
    );

    // Cargo compiles exactly the top-level `tests/*.rs` files as test roots.
    let roots: Vec<PathBuf> = all
        .iter()
        .filter(|p| p.parent() == Some(Path::new("")))
        .cloned()
        .collect();

    let mut reachable = BTreeSet::new();
    for root in &roots {
        visit(&tests_dir, root, &mut reachable);
    }

    // `tests/ffi/build.rs` is a Cargo build script, not a test module. Cargo
    // only honours a build script at the package root, so this copy is inert,
    // but it is deliberately not a test target and must not be declared.
    let exempt: BTreeSet<PathBuf> = [PathBuf::from("ffi/build.rs")].into_iter().collect();

    let orphans: Vec<&PathBuf> = all
        .difference(&reachable)
        .filter(|p| !exempt.contains(*p))
        .collect();

    assert!(
        orphans.is_empty(),
        "{} file(s) under tests/ are not reachable from any `tests/*.rs` root, so \
         Cargo never compiles them and their #[test] functions never run.\n\
         Declare each one (e.g. `#[path = \"integration/foo.rs\"] mod foo;` in \
         tests/integration.rs) or delete it:\n{}",
        orphans.len(),
        orphans
            .iter()
            .map(|p| format!("  tests/{}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
