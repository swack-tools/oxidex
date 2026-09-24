//! One file's `-TAG=VALUE` requests, applied as a single transaction whose
//! reported outcome is decided by the bytes (or a read-back proof), not by the
//! absence of an error.
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
//! and leaves the file alone) -- with one exception, matching ExifTool: a set
//! whose value the file provably already holds is `updated` (13.59 prints
//! `1 image files updated` for `-Make=<stored value>`), and only after a
//! read-back of every requested address proves it (see `already_satisfied`).

use crate::cli::args::CliArgs;
use crate::cli::value_parser::parse_cli_tag_value_os;
use crate::core::date_shift::{ShiftOperation, shift_metadata_dates};
use crate::core::operations::{
    CopyReport, clear_all_metadata, copy_metadata_report, modify_tag, read_metadata, remove_tag,
    resolve_write_tag,
};
use crate::writers::atomic_writer::write_atomic;
use crate::writers::exif_surgical::stored_entry_matches;
use crate::writers::write_request::undefined_tag_warning;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

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
    modifications: &[(String, OsString)],
) -> (Vec<String>, Vec<(String, OsString)>) {
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

/// Every write request of one command line, classified once.
///
/// `main` used to dispatch on the first write mode it recognised -- `-all=`,
/// then a date shift, then `-TagsFromFile` -- and drop every other request
/// on the line: `-all= -XPTitle=x` cleared the file and reported an update
/// with no XPTitle written, and `-DateTimeOriginal+=1 -XPTitle=x` only
/// shifted the date. A plan carries all of them, applied together in one
/// transaction in ExifTool's order (clear, then copy, then shifts and sets),
/// or is refused before any file is touched.
#[derive(Debug, Clone, Default)]
pub struct WritePlan {
    /// `-all=`.
    pub clear_all: bool,
    /// `-TagsFromFile SRC`, with its tag arguments (empty = copy all).
    pub copy_from: Option<(PathBuf, Vec<String>)>,
    /// Date shifts (`-AllDates+=1:0:0 0:0:0`, an absolute `-ModifyDate=...`).
    pub shifts: Vec<(String, ShiftOperation, String)>,
    /// Plain `-TAG=VALUE` requests whose names ExifTool defines.
    pub sets: Vec<(String, OsString)>,
    /// ExifTool's warnings for the `-TAG=VALUE` names it does not define.
    pub warnings: Vec<String>,
    /// Whether any `-TAG=VALUE` was given, defined or not.
    requested_sets: bool,
}

/// Date tags `AllDates` shifts (ExifTool's `AllDates` shortcut).
const ALL_DATES: &[&str] = &["DateTimeOriginal", "CreateDate", "ModifyDate"];

fn tag_name(tag: &str) -> &str {
    tag.rsplit(':').next().unwrap_or(tag)
}

impl WritePlan {
    /// Classifies `args`. `Err` is a refusal to print after `Error: ` (exit 1,
    /// nothing touched): a combination oxidex cannot apply faithfully.
    pub fn from_args(args: &CliArgs) -> Result<Self, String> {
        let raw_sets = args.plain_tag_modifications();
        let (warnings, sets) = partition_defined(&raw_sets);
        let mut shifts = Vec::new();
        for (tag, op, value) in args.date_shift_operations() {
            let operation = match op.as_str() {
                "+=" => ShiftOperation::Add,
                "-=" => ShiftOperation::Subtract,
                "=" => ShiftOperation::Set,
                _ => {
                    return Err(format!(
                        "Invalid date shift operation '{}'\nSupported operations: +=, -=, =",
                        op
                    ));
                }
            };
            shifts.push((tag, operation, value));
        }
        let plan = WritePlan {
            clear_all: args.is_clear_all_metadata(),
            copy_from: args.tags_from_file.as_ref().map(|src| {
                (
                    PathBuf::from(src),
                    args.copy_tag_filters().unwrap_or_default(),
                )
            }),
            shifts,
            sets,
            warnings,
            requested_sets: !raw_sets.is_empty(),
        };
        if plan.clear_all && !plan.shifts.is_empty() {
            return Err(
                "Combining -all= with a date shift is not supported: the shift would \
                 apply to dates -all= removes"
                    .to_string(),
            );
        }
        // The sets that survive `partition_defined`: an undefined name is only
        // ExifTool's warning (13.59, `-TagsFromFile src -XPTitle -NoSuchTag=x`:
        // `Warning: Tag 'NoSuchTag' is not defined`, then the copy), never a
        // reason to refuse the copy.
        if plan.copy_from.is_some() && (!plan.shifts.is_empty() || !plan.sets.is_empty()) {
            return Err(
                "Combining -TagsFromFile with -TAG=VALUE or a date shift is not \
                 supported yet; run them as separate commands"
                    .to_string(),
            );
        }
        for (shift_tag, _, _) in &plan.shifts {
            let shifted: Vec<&str> = if shift_tag.eq_ignore_ascii_case("AllDates") {
                ALL_DATES.to_vec()
            } else {
                vec![tag_name(shift_tag)]
            };
            if let Some((set_tag, _)) = plan.sets.iter().find(|(set_tag, _)| {
                shifted
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(tag_name(set_tag)))
            }) {
                return Err(format!(
                    "'{set_tag}' is both set and shifted ({shift_tag}); give one request per tag"
                ));
            }
        }
        Ok(plan)
    }

    /// Whether the command line asks for any write at all.
    pub fn is_write(&self) -> bool {
        self.clear_all || self.copy_from.is_some() || !self.shifts.is_empty() || self.requested_sets
    }

    /// Whether every request was an undefined name (`Nothing to do.`).
    pub fn nothing_to_do(&self) -> bool {
        !self.clear_all
            && self.copy_from.is_none()
            && self.shifts.is_empty()
            && self.sets.is_empty()
    }

    /// Plain `-TAG=VALUE` requests and nothing else.
    pub fn sets_only(&self) -> bool {
        !self.clear_all && self.copy_from.is_none() && self.shifts.is_empty()
    }
}

/// What a plan did to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanOutcome {
    pub outcome: WriteOutcome,
    /// The copy report, when the plan copies.
    pub copy: Option<CopyReport>,
}

/// Applies `plan` to `path` as one transaction (see [`transact`]).
pub fn write_plan_file(
    path: &Path,
    plan: &WritePlan,
    on_commit: impl FnOnce() -> Result<(), String>,
) -> Result<PlanOutcome, String> {
    let mut on_commit = Some(on_commit);
    let mut copy = None;
    let outcome = transact(
        path,
        || on_commit.take().map_or(Ok(()), |commit| commit()),
        |scratch| {
            if plan.clear_all {
                clear_all_metadata(scratch).map_err(|e| {
                    format!("Failed to clear metadata from '{}': {}", path.display(), e)
                })?;
            }
            if let Some((src, filters)) = &plan.copy_from {
                let filters = (!filters.is_empty()).then_some(filters.as_slice());
                copy = Some(copy_metadata_report(src, scratch, filters).map_err(|e| {
                    format!(
                        "Failed to copy metadata from '{}' to '{}': {}",
                        src.display(),
                        path.display(),
                        e
                    )
                })?);
            }
            for (tag_pattern, operation, offset) in &plan.shifts {
                shift_metadata_dates(scratch, tag_pattern, offset, *operation)
                    .map_err(|e| format!("Failed to shift dates for '{}': {}", tag_pattern, e))?;
            }
            apply_sets(scratch, &plan.sets)
        },
    )?;
    if outcome == WriteOutcome::Unchanged && plan.sets_only() && already_satisfied(path, &plan.sets)
    {
        // Nothing was rewritten, but the request is exactly what the file
        // holds: report it as ExifTool does. The `--backup` copy still
        // accompanies an update.
        if let Some(commit) = on_commit.take() {
            commit()?;
        }
        return Ok(PlanOutcome {
            outcome: WriteOutcome::Updated,
            copy,
        });
    }
    Ok(PlanOutcome { outcome, copy })
}

/// Applies every request to `path` as one transaction.
///
/// `Err` carries the message the CLI prints after `Error: `; the original
/// file is then untouched. `on_commit` runs just before an update replaces
/// the original (the CLI's `--backup` copy), and not at all when unchanged.
///
/// Identical bytes are `Unchanged` -- unless a read-back proves the file
/// already holds every requested value ([`already_satisfied`]), which
/// ExifTool 13.59 reports as `1 image files updated` (`-Make=<stored value>`
/// rewrites to the same bytes and still counts as an update there).
pub fn write_file(
    path: &Path,
    modifications: &[(String, OsString)],
    on_commit: impl FnOnce() -> Result<(), String>,
) -> Result<WriteOutcome, String> {
    let plan = WritePlan {
        sets: modifications.to_vec(),
        requested_sets: !modifications.is_empty(),
        ..Default::default()
    };
    write_plan_file(path, &plan, on_commit).map(|done| done.outcome)
}

fn apply_sets(scratch: &Path, sets: &[(String, OsString)]) -> Result<(), String> {
    for (tag_name, value) in sets {
        if value.is_empty() {
            // Empty value = delete tag (ExifTool -TAG= syntax)
            remove_tag(scratch, tag_name)
                .map_err(|e| format!("Failed to remove tag '{}': {}", tag_name, e))?;
        } else {
            // Typed as the tag's registry entry declares; wrapping every value
            // as a String made Integer/Rational/DateTime tags unsettable from
            // the CLI.
            let tag_value = parse_cli_tag_value_os(tag_name, value)
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
}

/// Whether a read-back of `path` proves every request is already in effect:
/// at least one set, each set's resolved address holding exactly the
/// requested value, and each deletion's address absent.
///
/// This is the only way a byte-identical write is reported as an update, and
/// it fails closed: a request that cannot be resolved, a value that cannot be
/// parsed, a stored value that differs in type or spelling, or a
/// deletion-only request (ExifTool 13.59: `-XPTitle=` with no XPTitle is
/// `0 image files updated` / `1 image files unchanged`) all leave the answer
/// `unchanged`. A dropped or refused request never reaches here: refusals are
/// errors, and the resolved address is the one the writer was handed.
fn already_satisfied(path: &Path, modifications: &[(String, OsString)]) -> bool {
    if !modifications.iter().any(|(_, value)| !value.is_empty()) {
        return false;
    }
    let Ok(stored) = read_metadata(path) else {
        return false;
    };
    let file_bytes = fs::read(path).ok();
    modifications.iter().all(|(tag_name, value)| {
        let Ok(key) = resolve_write_tag(path, tag_name) else {
            return false;
        };
        // The exact address the writer was handed -- `EXIF:<name>` already
        // resolved to its own directory -- and no other.
        if value.is_empty() {
            return !stored.contains_key(&key);
        }
        let Ok(requested) = parse_cli_tag_value_os(tag_name, value) else {
            return false;
        };
        // An EXIF entry must hold exactly the bytes the writer would emit: the
        // reader's value is normalized (a stored `"Canon   "` reads as
        // `"Canon"`), so matching it would prove nothing.
        if matches!(
            key.split_once(':'),
            Some(("IFD0" | "ExifIFD" | "GPS" | "IFD1", _))
        ) {
            return file_bytes
                .as_deref()
                .is_some_and(|bytes| stored_entry_matches(bytes, &key, &requested) == Some(true));
        }
        stored.get(&key).is_some_and(|held| {
            *held == requested
                || value
                    .to_str()
                    .is_some_and(|text| held.as_string() == Some(text))
        })
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
