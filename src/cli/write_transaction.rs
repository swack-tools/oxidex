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
//! Every request is now applied through the library's one write transaction
//! (`core::write_transaction`, the path `write_metadata`, `modify_tag` and
//! the C ABI take too): all of a file's `-TAG=` requests are resolved
//! together, applied to a private copy, proven by a read-back and committed
//! only if every one of them is in the file. Equal bytes are ExifTool's
//! `image files unchanged` (13.59: `-XPTitle=` on a file with no XPTitle
//! prints `0 image files updated` / `1 image files unchanged` and leaves the
//! file alone) -- with one exception, matching ExifTool: a set whose value
//! the file already holds is `updated` (13.59 prints `1 image files updated`
//! for `-Make=<stored value>`). The transaction's read-back proves every set
//! is in effect before it returns, so an unchanged file after a successful
//! set is exactly that case.

use crate::cli::args::CliArgs;
use crate::cli::value_parser::parse_cli_tag_value_os;
use crate::core::date_shift::{ShiftOperation, shift_metadata_dates};
use crate::core::operations::{CopyReport, clear_all_metadata, copy_metadata_report};
use crate::core::write_transaction::{
    ScratchStep, TagChange, apply_tag_changes_counted, transact_with,
};
use crate::error::ExifToolError;
use crate::writers::write_request::{
    canonical_request_tag, sorry_not_writable, undefined_tag_warning,
};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// What happened to one file (the library's [`WriteOutcome`]).
///
/// [`WriteOutcome`]: crate::core::write_transaction::WriteOutcome
pub use crate::core::write_transaction::WriteOutcome;

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
        // The warning quotes the name as typed, as ExifTool's does; a defined
        // name continues in its one canonical spelling
        // (`write_request::canonical_request_tag`), so value typing,
        // resolution and the transaction all read the same key.
        match undefined_tag_warning(tag) {
            Some(warning) => warnings.push(warning),
            None => defined.push((canonical_request_tag(tag), value.clone())),
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
/// transaction in ExifTool's order, or is refused before any file is
/// touched.
///
/// ExifTool applies a command's requests in argument order, and `-all=`
/// removes every value assigned before it (`Writer.pl`'s
/// `RemoveNewValuesForGroup`) but none assigned after it. So a `-TAG=VALUE`
/// before the last `-all=` is superseded and dropped (13.59:
/// `-IFD0:Artist=x -all=` leaves no EXIF; `-all= -IFD0:Artist=x` leaves
/// Artist), and a `-TagsFromFile` before it is applied before the clear
/// (13.59: `-TagsFromFile SRC -all= DST` leaves DST without SRC's tags;
/// `-all= -TagsFromFile SRC DST` leaves them). Applying the clear first
/// whatever the order used to keep both.
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
    /// Whether `-TagsFromFile` came before the last `-all=`: the copy is
    /// then applied first and cleared with the rest.
    copy_before_clear: bool,
}

/// Date tags `AllDates` shifts (ExifTool's `AllDates` shortcut).
const ALL_DATES: &[&str] = &["DateTimeOriginal", "CreateDate", "ModifyDate"];

/// The field a date request addresses, as far as it is known before any
/// file is opened: `(group, name)`, lower-cased, with ExifTool's other names
/// for the EXIF dates folded in (`DateTime` is `ModifyDate`,
/// `DateTimeDigitized` is `CreateDate`) and the `EXIF` family resolved to
/// the directory ExifTool writes the date in (`EXIF:CreateDate` is
/// `ExifIFD:CreateDate`). `group` is `None` for an ungrouped request, which
/// may land in any group (a shift of `CreateDate` shifts `PDF:CreateDate` in
/// a PDF), and stays `exif` for a family request naming another tag.
fn date_address(tag: &str) -> (Option<String>, String) {
    let (group, name) = match tag.rsplit_once(':') {
        Some((group, name)) => (Some(group.to_ascii_lowercase()), name),
        None => (None, tag),
    };
    let name = match name.to_ascii_lowercase().as_str() {
        "datetime" => "modifydate".to_string(),
        "datetimedigitized" => "createdate".to_string(),
        other => other.to_string(),
    };
    let group = group.map(|group| match (group.as_str(), name.as_str()) {
        ("exif", "modifydate") => "ifd0".to_string(),
        ("exif", "datetimeoriginal" | "createdate") => "exififd".to_string(),
        _ => group,
    });
    (group, name)
}

/// EXIF directories the `EXIF` family spans.
const EXIF_DIRECTORIES: &[&str] = &["ifd0", "ifd1", "exififd", "gps", "interopifd", "subifd"];

/// Whether two requests may address the same field ([`date_address`]): the
/// same name, and groups that are equal, or one of them ungrouped or the
/// `EXIF` family spanning the other's directory. Requests naming two
/// different directories (`ExifIFD:CreateDate`, `IFD0:CreateDate`) are two
/// fields, which ExifTool writes independently.
fn may_address_same_field(a: &str, b: &str) -> bool {
    let ((group_a, name_a), (group_b, name_b)) = (date_address(a), date_address(b));
    if name_a != name_b {
        return false;
    }
    match (group_a.as_deref(), group_b.as_deref()) {
        (None, _) | (_, None) => true,
        (Some(a), Some(b)) => {
            a == b
                || (a == "exif" && EXIF_DIRECTORIES.contains(&b))
                || (b == "exif" && EXIF_DIRECTORIES.contains(&a))
        }
    }
}

impl WritePlan {
    /// Classifies `args`. `Err` is a refusal to print after `Error: ` (exit 1,
    /// nothing touched): a combination oxidex cannot apply faithfully.
    pub fn from_args(args: &CliArgs) -> Result<Self, String> {
        let raw_sets = args.plain_tag_modifications_with_positions();
        let clear_at = args.clear_all_position();
        // Every name is judged (and an undefined one warned about) wherever
        // it stands, as ExifTool's `SetNewValue` judges each; only then does
        // a later `-all=` remove the values assigned before it.
        let mut warnings = Vec::new();
        let mut sets = Vec::new();
        for (at, tag, value) in &raw_sets {
            // ExifTool's `AllDates` shortcut (Shortcuts.pm): the three EXIF
            // dates, in its order, each under the shortcut's group if any.
            let (group, name) = match tag.rsplit_once(':') {
                Some((group, name)) => (Some(group), name),
                None => (None, tag.as_str()),
            };
            let expanded: Vec<(String, OsString)> = if name.eq_ignore_ascii_case("AllDates") {
                ["DateTimeOriginal", "CreateDate", "ModifyDate"]
                    .iter()
                    .map(|date| {
                        let tag = group.map_or_else(|| date.to_string(), |g| format!("{g}:{date}"));
                        (tag, value.clone())
                    })
                    .collect()
            } else {
                vec![(tag.clone(), value.clone())]
            };
            let (mut warned, defined) = partition_defined(&expanded);
            warnings.append(&mut warned);
            if clear_at.is_none_or(|clear| *at > clear) {
                sets.extend(defined);
            }
        }
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
            copy_before_clear: args.tags_from_file.is_some()
                && matches!(
                    (args.tags_from_file_position, clear_at),
                    (Some(copy), Some(clear)) if copy <= clear
                ),
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
        // A set and a shift of one field cannot both be applied; requests
        // naming two different fields can (13.59: an `ExifIFD:CreateDate`
        // shift beside an `IFD0:CreateDate` set writes both), so the
        // addresses are compared, not the leaf names.
        for (shift_tag, _, _) in &plan.shifts {
            let shifted: Vec<&str> = if shift_tag.eq_ignore_ascii_case("AllDates") {
                ALL_DATES.to_vec()
            } else {
                vec![shift_tag.as_str()]
            };
            if let Some((set_tag, _)) = plan.sets.iter().find(|(set_tag, _)| {
                shifted
                    .iter()
                    .any(|shifted| may_address_same_field(shifted, set_tag))
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
    let mut proven_sets = 0;
    let outcome = transact(
        path,
        || on_commit.take().map_or(Ok(()), |commit| commit()),
        |scratch| {
            let clear = || {
                clear_all_metadata(scratch).map_err(|e| {
                    format!("Failed to clear metadata from '{}': {}", path.display(), e)
                })
            };
            // `-all=` where it stood relative to `-TagsFromFile` (the sets
            // before it were already dropped, see `WritePlan`).
            if plan.clear_all && !plan.copy_before_clear {
                clear()?;
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
            if plan.clear_all && plan.copy_before_clear {
                clear()?;
            }
            for (tag_pattern, operation, offset) in &plan.shifts {
                shift_metadata_dates(scratch, tag_pattern, offset, *operation)
                    .map_err(|e| format!("Failed to shift dates for '{}': {}", tag_pattern, e))?;
            }
            proven_sets = apply_sets(scratch, &plan.sets)?;
            Ok(())
        },
    )?;
    if outcome == WriteOutcome::Unchanged && plan.sets_only() && proven_sets > 0 {
        // Nothing was rewritten, yet the transaction's read-back proved a set
        // in effect (and every other request in effect or a no-op): the
        // request is what the file holds, which ExifTool reports as an
        // update. A request made only of deletions and no-ops stays
        // `unchanged` (13.59: `-XPTitle=` with no XPTitle; `-IFD0:Artist=you`
        // on a PDF). The `--backup` copy still accompanies an update.
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
/// Identical bytes are `Unchanged` -- unless the request sets a value, whose
/// read-back then proved the file already holds every requested value, which
/// ExifTool 13.59 reports as `1 image files updated` (`-Make=<stored value>`
/// rewrites to the same bytes and still counts as an update there).
pub fn write_file(
    path: &Path,
    modifications: &[(String, OsString)],
    on_commit: impl FnOnce() -> Result<(), String>,
) -> Result<WriteOutcome, String> {
    let plan = WritePlan {
        sets: modifications
            .iter()
            .map(|(tag, value)| (canonical_request_tag(tag), value.clone()))
            .collect(),
        requested_sets: !modifications.is_empty(),
        ..Default::default()
    };
    write_plan_file(path, &plan, on_commit).map(|done| done.outcome)
}

/// Applies every `-TAG=VALUE` of one file through the library's write
/// transaction ([`apply_tag_changes_counted`]): each value is parsed first
/// (typed as the tag's registry entry declares; wrapping every value as a
/// String made Integer/Rational/DateTime tags unsettable), then all of them
/// are resolved, written in one pass, and proven together -- or none is.
/// Returns how many sets were proven in effect.
fn apply_sets(scratch: &Path, sets: &[(String, OsString)]) -> Result<usize, String> {
    if sets.is_empty() {
        return Ok(0);
    }
    let mut changes = Vec::with_capacity(sets.len());
    for (tag_name, value) in sets {
        if value.is_empty() {
            // Empty value = delete tag (ExifTool -TAG= syntax)
            changes.push(TagChange::delete(tag_name.clone()));
        } else {
            let tag_value = parse_cli_tag_value_os(tag_name, value)
                .map_err(|e| format!("Invalid value for {}: {}", tag_name, e))?;
            changes.push(TagChange::set(tag_name.clone(), tag_value));
        }
    }
    apply_tag_changes_counted(scratch, &changes)
        .map(|(_, proven_sets)| proven_sets)
        .map_err(|e| describe_set_failure(&e, sets))
}

/// When every refused tag is one ExifTool itself names this way
/// (`write_request::sorry_not_writable` -- currently the PNG `XMP`
/// literal-text-chunk case), the CLI must echo ExifTool's own `Warning:
/// <reason>` / `Nothing to do.` shape instead of oxidex's `Failed to ...`
/// wrapping below: [`finish_write`] (`main.rs`) prints this verbatim, with
/// none of oxidex's own `Error:` prefix, because it *is* ExifTool's own
/// warning text, not oxidex's diagnosis of a failure.
///
/// [`finish_write`]: crate::cli
fn sorry_refusal_message(refused: &[crate::error::TagNotWritten]) -> Option<String> {
    if refused.is_empty()
        || !refused
            .iter()
            .all(|tag| tag.reason == sorry_not_writable(&tag.tag))
    {
        return None;
    }
    let mut message = refused
        .iter()
        .map(|tag| format!("Warning: {}", tag.reason))
        .collect::<Vec<_>>()
        .join("\n");
    message.push_str("\nNothing to do.");
    Some(message)
}

/// Whether [`describe_set_failure`] produced ExifTool's own warning text
/// (see [`sorry_refusal_message`]) rather than oxidex's `Failed to ...`
/// wrapping -- the two need different framing in `main.rs`'s `finish_write`:
/// ExifTool's own words are printed as-is, oxidex's diagnosis gets an
/// `Error:` prefix.
pub fn is_exiftool_refusal_message(message: &str) -> bool {
    message.starts_with("Warning: Sorry, ") && message.ends_with("\nNothing to do.")
}

/// The CLI's message for a failed `-TAG=` transaction: which request failed,
/// and why. A refusal names each tag it refused; any other error is
/// attributed to the one request when there is only one.
fn describe_set_failure(err: &ExifToolError, sets: &[(String, OsString)]) -> String {
    let verb = |tag: &str| {
        let deletion = sets
            .iter()
            .any(|(name, value)| name == tag && value.is_empty());
        if deletion { "remove" } else { "modify" }
    };
    let refused = err.tags_not_written();
    if !refused.is_empty() {
        if let Some(message) = sorry_refusal_message(refused) {
            return message;
        }
        return refused
            .iter()
            .map(|tag| format!("Failed to {} tag '{}': {}", verb(&tag.tag), tag.tag, tag))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let text = err.to_string();
    let invalid = text.contains("invalid") || text.contains("Invalid");
    match (sets, err) {
        ([(tag_name, _)], _) if invalid => format!("Invalid value for {}: {}", tag_name, err),
        ([(tag_name, _)], _) => format!("Failed to {} tag '{}': {}", verb(tag_name), tag_name, err),
        (_, ExifToolError::InvalidTagValue { tag_name, .. }) => {
            format!("Invalid value for {}: {}", tag_name, err)
        }
        _ => format!("Failed to write tags: {}", err),
    }
}

/// Runs `apply` against a private copy of `path`, then commits the copy only
/// if `apply` succeeded and the bytes differ -- the library's transaction
/// ([`transact_with`]) with the CLI's messages. This is the one place any CLI
/// write (`-TAG=`, `-all=`, a date shift, `-TagsFromFile`) decides between
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
    transact_with(path, apply, on_commit, |step, e| match step {
        ScratchStep::ReadOriginal => format!("Cannot access file '{}': {}", path.display(), e),
        ScratchStep::CreateCopy => format!(
            "Cannot create a working copy of '{}': {}",
            path.display(),
            e
        ),
        ScratchStep::ReadCopy => format!(
            "Cannot read the working copy of '{}': {}",
            path.display(),
            e
        ),
        ScratchStep::Commit => format!("Failed to write '{}': {}", path.display(), e),
    })
}
