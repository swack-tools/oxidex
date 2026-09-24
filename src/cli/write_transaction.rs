//! One file's `-TAG=VALUE` requests, applied as a single transaction whose
//! reported outcome is decided by the bytes, not by the absence of an error.
//!
//! `main` used to apply each request to the file in place and then print
//! `1 image files updated` unconditionally. A request no writer performed
//! (an ungrouped `-XPTitle=v`, `-XMP:Title=v` in a JPEG) returned `Ok`, so
//! the file was reported updated with nothing written; and a request that
//! failed after an earlier one succeeded left the file half-written.
//!
//! Here every request is applied to a private copy; the original is replaced
//! only once all of them succeeded, and only if the copy's bytes differ. Equal
//! bytes are ExifTool's `image files unchanged` (13.59: `-XPTitle=` on a file
//! with no XPTitle prints `0 image files updated` / `1 image files unchanged`
//! and leaves the file alone), never an update.

use crate::cli::value_parser::parse_cli_tag_value;
use crate::core::operations::{modify_tag, remove_tag};
use crate::writers::atomic_writer::write_atomic;
use crate::writers::write_request::undefined_tag_warning;
use std::fs;
use std::path::Path;

/// What happened to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The file's bytes changed.
    Updated,
    /// Every request succeeded and the bytes are identical; the file was not
    /// touched.
    Unchanged,
}

/// Splits `modifications` the way ExifTool's `SetNewValue` does before any
/// file is opened: requests naming a tag ExifTool does not define become a
/// warning (`Tag 'X' is not defined`, Writer.pl:584) and are dropped; the
/// rest are returned in order. When nothing remains the caller prints
/// `Nothing to do.` and exits 1 (exiftool:1810-1813).
pub fn partition_defined(
    modifications: &[(String, String)],
) -> (Vec<String>, Vec<(String, String)>) {
    let mut warnings = Vec::new();
    let mut defined = Vec::new();
    for (tag, value) in modifications {
        match undefined_tag_warning(tag) {
            Some(warning) => warnings.push(warning),
            None => defined.push((tag.clone(), value.clone())),
        }
    }
    (warnings, defined)
}

/// Applies every request to `path` as one transaction.
///
/// `Err` carries the message the CLI prints after `Error: `; the original
/// file is then untouched. `on_commit` runs just before an update replaces
/// the original (the CLI's `--backup` copy), and not at all when unchanged.
pub fn write_file(
    path: &Path,
    modifications: &[(String, String)],
    on_commit: impl FnOnce() -> Result<(), String>,
) -> Result<WriteOutcome, String> {
    transact(path, on_commit, |scratch| {
        for (tag_name, value) in modifications {
            if value.is_empty() {
                // Empty value = delete tag (ExifTool -TAG= syntax)
                remove_tag(scratch, tag_name)
                    .map_err(|e| format!("Failed to remove tag '{}': {}", tag_name, e))?;
            } else {
                // Typed as the tag's registry entry declares; wrapping every
                // value as a String made Integer/Rational/DateTime tags
                // unsettable from the CLI.
                let tag_value = parse_cli_tag_value(tag_name, value)
                    .map_err(|e| format!("Invalid value for {}: {}", tag_name, e))?;
                modify_tag(scratch, tag_name, tag_value).map_err(|e| {
                    let text = e.to_string();
                    if text.contains("invalid") || text.contains("Invalid") {
                        format!("Invalid value for {}: {}", tag_name, e)
                    } else {
                        format!("Failed to modify tag '{}': {}", tag_name, e)
                    }
                })?;
            }
        }
        Ok(())
    })
}

/// Runs `apply` against a private copy of `path`, then commits the copy only
/// if `apply` succeeded and the bytes differ -- the one place any CLI write
/// (`-TAG=`, `-all=`, a date shift, `-TagsFromFile`) decides between
/// `updated` and `unchanged`.
///
/// This is the generic guard against a false update: whatever path `apply`
/// takes, a file whose bytes did not change is reported `Unchanged` and is
/// not rewritten. `-all=` on a file with nothing left to strip used to
/// rewrite identical bytes and print `1 image files updated`; ExifTool 13.59
/// prints `0 image files updated` / `1 image files unchanged`.
pub fn transact(
    path: &Path,
    on_commit: impl FnOnce() -> Result<(), String>,
    apply: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<WriteOutcome, String> {
    let original =
        fs::read(path).map_err(|e| format!("Cannot access file '{}': {}", path.display(), e))?;
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    // Keep the extension: format detection may consult it.
    let suffix = path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    let working_copy_error = |e: std::io::Error| {
        format!(
            "Cannot create a working copy of '{}': {}",
            path.display(),
            e
        )
    };
    let scratch = tempfile::Builder::new()
        .prefix(".oxidex-write-")
        .suffix(&suffix)
        .tempfile_in(dir)
        .map_err(working_copy_error)?;
    fs::write(scratch.path(), &original).map_err(working_copy_error)?;

    apply(scratch.path())?;

    let written = fs::read(scratch.path()).map_err(|e| {
        format!(
            "Cannot read the working copy of '{}': {}",
            path.display(),
            e
        )
    })?;
    if written == original {
        return Ok(WriteOutcome::Unchanged);
    }
    on_commit()?;
    write_atomic(path, &written)
        .map_err(|e| format!("Failed to write '{}': {}", path.display(), e))?;
    Ok(WriteOutcome::Updated)
}
