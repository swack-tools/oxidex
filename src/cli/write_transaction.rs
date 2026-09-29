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
use crate::cli::value_parser::{declared_alias, parse_cli_tag_value_os_with_mode};
use crate::core::date_shift::{ShiftOperation, shift_metadata_dates};
use crate::core::metadata_map::MetadataMap;
use crate::core::operations::{
    CopyReport, clear_all_metadata, conversion_makernote_census, copy_metadata_report_retaining,
    read_metadata, resolve_write_tag_in_request,
};
use crate::core::tag_value::TagValue;
use crate::core::write_transaction::{
    GroupDeletions, ScratchStep, TagChange, apply_tag_changes_counted_among, transact_with,
};
use crate::error::ExifToolError;
use crate::writers::write_request::{
    candidate_applies_to_file, canonical_request_tag, expand_write_shortcut, group_deletion,
    rejected_bare_conversion, sorry_not_writable, undefined_tag_warning,
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
    /// ExifTool's `-n` (oxidex's own `-n` is already dry-run, so this is
    /// `--no-print-conv` here): every `-TAG=VALUE` in `sets` is the tag's
    /// raw/machine value, with no PrintConv label lookup at all. A single
    /// `-TAG#=VALUE` gets the same treatment regardless of this flag
    /// (`apply_sets` checks the tag name's own trailing `#`, independent of
    /// this global setting).
    pub raw_values: bool,
    /// Whether `-TagsFromFile` came before the last `-all=`: the copy is
    /// then applied first and cleared with the rest.
    copy_before_clear: bool,
    /// How many of `sets` precede `-TagsFromFile` in the argument order:
    /// they are applied before the copy, the rest after it, as ExifTool
    /// applies a command's requests in order (13.59: `-TagsFromFile SRC
    /// -Make -IFD0:Make=` deletes the copied Make).
    sets_before_copy: usize,
    /// Defined sets removed by a later `-all=` still need their conversion
    /// warning, which ExifTool emits while reading the command line.
    sets_before_clear: Vec<(String, OsString)>,
}

/// Date tags `AllDates` shifts (ExifTool's `AllDates` shortcut).
const ALL_DATES: &[&str] = &["DateTimeOriginal", "CreateDate", "ModifyDate"];

/// The field a date request addresses, as far as it is known before any
/// file is opened: `(group, name)`, lower-cased, with the `EXIF` family resolved to
/// the directory ExifTool writes the date in (`EXIF:CreateDate` is
/// `ExifIFD:CreateDate`). `group` is `None` for an ungrouped request, which
/// may land in any group (a shift of `CreateDate` shifts `PDF:CreateDate` in
/// a PDF), and stays `exif` for a family request naming another tag.
fn date_address(tag: &str) -> (Option<String>, String) {
    // A per-tag raw suffix (`-DateTimeOriginal#=`) names the same field:
    // comparing `datetimeoriginal#` with `datetimeoriginal` let a raw set
    // slip past the set/shift conflict refusal and overwrite the shifted
    // date, where pinned 13.59 keeps the shift (Codex pre-review of PR #959,
    // round 5).
    let tag = tag.strip_suffix('#').unwrap_or(tag);
    let (group, name) = match tag.rsplit_once(':') {
        Some((group, name)) => (Some(group.to_ascii_lowercase()), name),
        None => (None, tag),
    };
    // ExifTool does not treat DateTime or DateTimeDigitized as writable
    // EXIF date aliases here. In particular, a rejected shift under either
    // spelling must not conflict with a real ModifyDate/CreateDate set.
    let name = name.to_ascii_lowercase();
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
        let raw_values = !args.exiftool_compat();
        let clear_at = args.clear_all_position();
        // Every name is judged (and an undefined one warned about) wherever
        // it stands, as ExifTool's `SetNewValue` judges each; only then does
        // a later `-all=` remove the values assigned before it.
        let mut warnings = Vec::new();
        let mut sets = Vec::new();
        let mut sets_before_clear = Vec::new();
        let mut sets_before_copy = 0;
        for (at, tag, value) in &raw_sets {
            // A `Shortcuts::Main` name (`AllDates`, `CommonIFD0`, ...) stands
            // for its tags, each set in turn as pinned ExifTool's
            // `SetNewValue` sets them (`write_request::expand_write_shortcut`),
            // before any of them is judged or resolved.
            let expanded: Vec<(String, OsString)> = match expand_write_shortcut(tag) {
                Some(members) => members
                    .into_iter()
                    .map(|member| (member, value.clone()))
                    .collect(),
                None => vec![(tag.clone(), value.clone())],
            };
            let (mut warned, defined) = partition_defined(&expanded);
            warnings.append(&mut warned);
            // A value `SetNewValue` cannot convert is ExifTool's warning, and
            // that one request is dropped before any file is opened -- once
            // per command, wherever the request stands (a set a later `-all=`
            // supersedes is still judged): pinned 13.59 prints `Can't convert
            // IFD0:Orientation (not in PrintConv)` once for two files and for
            // `-IFD0:Orientation=6 -all=`, writes Artist beside it in
            // `-IFD0:Orientation=6 -IFD0:Artist=x`, and says `Nothing to do.`
            // only when no request is left (Codex pre-review of PR #959,
            // round 5).
            let defined: Vec<(String, OsString)> = defined
                .into_iter()
                .filter(|(tag, value)| {
                    let warning = if tag.contains(':') {
                        unconvertible_value_warning(tag, value, raw_values)
                    } else {
                        bare_conversion_warning(tag, value, raw_values, |_| true)
                    };
                    match warning {
                        Some(warning) => {
                            warnings.push(warning);
                            false
                        }
                        None => true,
                    }
                })
                .collect();
            if clear_at.is_none_or(|clear| *at > clear) {
                if args
                    .tags_from_file_position
                    .is_some_and(|copy| args.tags_from_file.is_some() && *at < copy)
                {
                    sets_before_copy += defined.len();
                }
                sets.extend(defined);
            } else {
                sets_before_clear.extend(defined);
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
            raw_values,
            copy_before_clear: args.tags_from_file.is_some()
                && matches!(
                    (args.tags_from_file_position, clear_at),
                    (Some(copy), Some(clear)) if copy <= clear
                ),
            sets_before_copy,
            sets_before_clear,
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
        // A copy with sets is applied in argument order (`sets_before_copy`);
        // with a date shift it is refused: the shift runs before every set
        // and copy, whatever its place.
        if plan.copy_from.is_some() && !plan.shifts.is_empty() {
            return Err(
                "Combining -TagsFromFile with a date shift is not supported yet; run \
                 them as separate commands"
                    .to_string(),
            );
        }
        // A set and a shift of one field cannot both be applied; requests
        // naming two different fields can (13.59: an `ExifIFD:CreateDate`
        // shift beside an `IFD0:CreateDate` set writes both), so the
        // addresses are compared, not the leaf names.
        for (shift_tag, _, _) in &plan.shifts {
            let shifted: Vec<&str> = if shift_tag
                .strip_suffix('#')
                .unwrap_or(shift_tag)
                .eq_ignore_ascii_case("AllDates")
            {
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

    /// Classify bare values at the destination this file would write. This
    /// must run before the transaction: a failed PrintConv lookup is a
    /// per-request warning, while an unresolved/unsupported destination is
    /// still the transaction's whole-file refusal.
    pub fn for_file(&self, path: &Path) -> (Self, Vec<String>) {
        let mut plan = self.clone();
        let mut warnings = Vec::new();
        let deletions = GroupDeletions::plan(
            path,
            self.sets
                .iter()
                .enumerate()
                .filter(|(_, (_, value))| value.is_empty())
                .map(|(at, (tag, _))| (at, tag.strip_suffix('#').unwrap_or(tag))),
            None,
        );
        let baseline = read_metadata(path).ok();
        let census = conversion_makernote_census(path);
        let cleared_baseline = MetadataMap::new();
        let cleared_census = crate::writers::exif_surgical::MakerNoteCensus::default();
        let classify = |at: usize, tag: &str, value: &OsString, with_deletions: bool| {
            if value.is_empty() {
                return None;
            }
            let bare = tag.strip_suffix('#').unwrap_or(tag);
            if bare.contains(':') {
                return None; // already classified in from_args
            }
            // A surviving set after `-all=` is applied to the stripped
            // scratch carrier. The original maker note cannot make one of
            // its native candidates eligible there. A later TagsFromFile
            // may repopulate the carrier; only sets before that copy have a
            // provably empty effective carrier.
            let cleared = with_deletions
                && self.clear_all
                && (self.copy_from.is_none()
                    || self.copy_before_clear
                    || at < self.sets_before_copy);
            let effective = if cleared {
                Some(&cleared_baseline)
            } else {
                baseline.as_ref()
            };
            let effective_census = if cleared { cleared_census } else { census };
            if let Some(metadata) = effective
                && let Some(warning) =
                    bare_conversion_warning(tag, value, self.raw_values, |candidate| {
                        candidate_applies_to_file(candidate, bare, metadata, effective_census)
                    })
            {
                return Some(warning);
            }
            let typed = effective.and_then(|metadata| {
                resolve_write_tag_in_request(
                    path,
                    bare,
                    metadata,
                    if with_deletions {
                        deletions.for_set_at(at)
                    } else {
                        Default::default()
                    },
                )
                .ok()
            });
            // An unresolved name is left to the write planner. Parsing it
            // against an arbitrary EXIF alias could hide a real refusal.
            typed.and_then(|mut key| {
                // Resolution returns the physical key without request
                // syntax. Keep `#` so this request still bypasses PrintConv.
                if tag.ends_with('#') {
                    key.push('#');
                }
                unconvertible_value_warning(&key, value, self.raw_values)
            })
        };
        let mut sets_before_copy = 0;
        plan.sets = self
            .sets
            .iter()
            .enumerate()
            .filter_map(|(at, (tag, value))| match classify(at, tag, value, true) {
                Some(warning) => {
                    warnings.push(warning);
                    None
                }
                None => {
                    if at < self.sets_before_copy {
                        sets_before_copy += 1;
                    }
                    Some((tag.clone(), value.clone()))
                }
            })
            .collect();
        plan.sets_before_copy = sets_before_copy;
        for (at, (tag, value)) in self.sets_before_clear.iter().enumerate() {
            if let Some(warning) = classify(at, tag, value, false) {
                warnings.push(warning);
            }
        }
        (plan, warnings)
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
            // The sets given before `-TagsFromFile`, then the copy, then the
            // rest, in argument order.
            let (before_copy, after_copy) = plan
                .sets
                .split_at(plan.sets_before_copy.min(plan.sets.len()));
            // A later replacement cancels an earlier pending set or single-tag
            // deletion even when TagsFromFile appears between them. Group-wide
            // deletions still apply, and a later deletion does not cancel an
            // earlier deletion: both must retain their refusal checks.
            let before_copy: Vec<_> = before_copy
                .iter()
                .filter(|(tag, value)| {
                    (value.is_empty()
                        && crate::writers::write_request::group_deletion(tag).is_some())
                        || !after_copy.iter().any(|(later, later_value)| {
                            (!value.is_empty() || !later_value.is_empty())
                                && supersedes_prior_assignment(later, later_value, tag)
                        })
                })
                .cloned()
                .collect();
            // The core receipt excludes cancelled sets and gives the physical
            // destinations actually proven, so later passes protect only live
            // assignments from this command's earlier phase.
            let (_, before_destinations, mut prior_values) =
                apply_sets(scratch, &before_copy, plan.raw_values, &[], &[])?;
            let mut copied_values = Vec::new();
            if let Some((src, filters)) = &plan.copy_from {
                let filters = (!filters.is_empty()).then_some(filters.as_slice());
                // ExifTool evaluates the physical maker note block against
                // the Make after later CLI requests. A later deletion leaves
                // no Make; a later set supplies that exact final value.
                let final_make_override = after_copy
                    .iter()
                    .rev()
                    .find(|(tag, _)| supersedes_copy(tag, "IFD0:Make"))
                    .map(|(_, value)| value.to_str().filter(|v| !v.is_empty()).map(str::to_owned));
                copy = Some(
                    copy_metadata_report_retaining(
                        src,
                        scratch,
                        filters,
                        |key| {
                            !plan.copy_before_clear
                                && !after_copy.iter().any(|(tag, value)| {
                                    supersedes_prior_assignment(tag, value, key)
                                })
                        },
                        final_make_override,
                        &before_destinations,
                        &mut copied_values,
                    )
                    .map_err(|e| {
                        format!(
                            "Failed to copy metadata from '{}' to '{}': {}",
                            src.display(),
                            path.display(),
                            e
                        )
                    })?,
                );
            }
            if plan.clear_all && plan.copy_before_clear {
                clear()?;
                prior_values.clear();
            } else {
                for (key, value) in &copied_values {
                    prior_values.retain(|(prior, _)| !prior.eq_ignore_ascii_case(key));
                    prior_values.push((key.clone(), value.clone()));
                }
            }
            if !plan.copy_before_clear {
                proven_sets += before_destinations
                    .iter()
                    .filter(|key| !copied_values.iter().any(|(copied, _)| copied == *key))
                    .count();
            }
            let mut siblings: Vec<String> = before_destinations;
            siblings.extend(plan.shifts.iter().map(|(tag, _, _)| tag.clone()));
            // A retained TagsFromFile destination is a set in this command,
            // even though the CLI applied it in an earlier transaction.
            // Cross-directory deletion must see it beside later explicit
            // sets, just as ExifTool's one WriteExif pass does.
            if !plan.copy_before_clear {
                if let Some(report) = &copy {
                    siblings.extend(report.copied_destinations.iter().cloned());
                }
            }
            for (tag_pattern, operation, offset) in &plan.shifts {
                shift_metadata_dates(scratch, tag_pattern, offset, *operation)
                    .map_err(|e| format!("Failed to shift dates for '{}': {}", tag_pattern, e))?;
            }
            let replay = retained_ifd0_family_assignments(&prior_values, after_copy);
            let (after_proven, after_destinations, _) =
                apply_sets(scratch, after_copy, plan.raw_values, &siblings, &replay)?;
            if replay
                .iter()
                .any(|(key, _)| !after_destinations.iter().any(|dest| dest == key))
            {
                return Err(
                    "A retained EXIF assignment was not proven; nothing was written".into(),
                );
            }
            proven_sets += after_proven.saturating_sub(replay.len());
            Ok(())
        },
    )?;
    let proven_copies = copy.as_ref().map_or(0, |report| report.copied);
    if outcome == WriteOutcome::Unchanged && proven_sets + proven_copies > 0 {
        // Nothing was rewritten, yet the transaction's read-back proved a set
        // in effect (and every other request in effect or a no-op): the
        // request is what the file holds, which ExifTool reports as an
        // update -- a copied tag is such a set too (13.59: `-TagsFromFile
        // SRC -Make` onto the same Make is `1 image files updated`). A
        // request made only of deletions and no-ops stays `unchanged`
        // (13.59: `-XPTitle=` with no XPTitle; `-IFD0:Artist=you` on a PDF).
        // The `--backup` copy still accompanies an update.
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

/// Whether a later request replaces a copied destination's pending set.
fn supersedes_copy(tag: &str, copied: &str) -> bool {
    use crate::writers::write_request::group_deletion;
    if let Some(group) = group_deletion(tag) {
        return crate::core::write_transaction::group_covers(group, copied);
    }
    let tag = tag.strip_suffix('#').unwrap_or(tag);
    let copied = copied.strip_suffix('#').unwrap_or(copied);
    let (wanted_group, wanted_name) = date_address(tag);
    let (copied_group, copied_name) = date_address(copied);
    if wanted_name != copied_name {
        return false;
    }
    match wanted_group.as_deref() {
        None => true,
        Some(group) if copied_group.as_deref() == Some(group) => true,
        Some("exif") => copied_group
            .as_deref()
            .is_some_and(|group| EXIF_DIRECTORIES.contains(&group)),
        // Directory deletion includes descendants; an individual field does not.
        Some(group) if EXIF_DIRECTORIES.contains(&group) => false,
        Some(group) => crate::core::write_transaction::group_covers(group, copied),
    }
}

/// Whether a later CLI request removes one pending assignment from an earlier
/// explicit or copy phase. A family-0 spelling with a source-derived native
/// write group addresses that directory's pending new value; only an actual
/// group deletion (handled first) removes the entire EXIF carrier. The four
/// unit-bearing family deletions also remove physical copies elsewhere, but
/// a pending explicit IFD0 new value survives the deletion, as it does in a
/// single core transaction.
fn supersedes_prior_assignment(tag: &str, value: &OsString, prior: &str) -> bool {
    if group_deletion(tag).is_some() {
        return supersedes_copy(tag, prior);
    }
    let tag = tag.strip_suffix('#').unwrap_or(tag);
    let family = crate::writers::write_request::unit_suffix_family_key(tag)
        .or_else(|| {
            (!value.is_empty()
                && matches!(
                    tag.to_ascii_lowercase().as_str(),
                    "createdate" | "modifydate" | "datetimeoriginal"
                ))
            .then(|| format!("EXIF:{tag}"))
        })
        .or_else(|| {
            (!value.is_empty()
                && tag
                    .split_once(':')
                    .is_some_and(|(group, _)| group.eq_ignore_ascii_case("EXIF")))
            .then(|| tag.to_owned())
        });
    if let Some(family) = family
        && let Some((_, leaf)) = family.split_once(':')
        && let Some((prior_group, prior_leaf)) = prior.split_once(':')
        && prior_leaf.eq_ignore_ascii_case(leaf)
        && (prior_group.eq_ignore_ascii_case("IFD0") || prior_group.eq_ignore_ascii_case("ExifIFD"))
        && let Some(preferred) = crate::writers::exif_cross_delete::exif_main_write_group(leaf)
        && !prior_group.eq_ignore_ascii_case(preferred)
    {
        return false;
    }
    supersedes_copy(tag, prior)
}

/// Reintroduce a proven earlier IFD0 assignment beside a later family-name
/// deletion. The later core pass removes physical copies in other directories
/// before writing this typed value. A group clear or explicit same-destination
/// request in the later phase cancels the earlier assignment.
fn retained_ifd0_family_assignments(
    prior_values: &[(String, TagValue)],
    after_copy: &[(String, OsString)],
) -> Vec<(String, TagValue)> {
    let families: Vec<_> = after_copy
        .iter()
        .filter(|(_, value)| value.is_empty())
        .filter_map(|(tag, _)| crate::writers::write_request::unit_suffix_family_key(tag))
        .collect();
    if families.is_empty() {
        return Vec::new();
    }
    prior_values
        .iter()
        .filter(|(key, _)| {
            key.split_once(':').is_some_and(|(group, leaf)| {
                group.eq_ignore_ascii_case("IFD0")
                    && families.iter().any(|family| {
                        family
                            .split_once(':')
                            .is_some_and(|(_, name)| name.eq_ignore_ascii_case(leaf))
                    })
            })
        })
        .filter(|(key, _)| {
            !after_copy.iter().any(|(tag, value)| {
                group_deletion(tag)
                    .is_some_and(|group| crate::core::write_transaction::group_covers(group, key))
                    || ((!value.is_empty()
                        || crate::writers::write_request::unit_suffix_family_key(tag).is_none())
                        && supersedes_prior_assignment(tag, value, key))
            })
        })
        .cloned()
        .collect()
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
    raw_values: bool,
    on_commit: impl FnOnce() -> Result<(), String>,
) -> Result<WriteOutcome, String> {
    write_file_with_warnings(path, modifications, raw_values, on_commit).0
}

/// Classify a file's sets before permission checks or transaction execution.
/// Warnings belong to the command even when a later preflight or write fails.
pub fn prepare_write_file(
    path: &Path,
    modifications: &[(String, OsString)],
    raw_values: bool,
) -> (WritePlan, Vec<String>) {
    // `raw_values` is `--no-print-conv` (ExifTool's `-n`), as
    // [`WritePlan::from_args`] reads it for one file: the multi-file path
    // used to build its plan without it, so `--no-print-conv
    // -IFD0:Orientation=6 a.jpg b.jpg` refused the raw code the one-file
    // form writes (pinned 13.59 writes both files).
    let plan = WritePlan {
        sets: modifications
            .iter()
            .map(|(tag, value)| (canonical_request_tag(tag), value.clone()))
            .collect(),
        requested_sets: !modifications.is_empty(),
        raw_values,
        ..Default::default()
    };
    plan.for_file(path)
}

/// Variant of [`write_file`] that retains destination-aware warnings alongside
/// either a successful write or a transaction error.
pub fn write_file_with_warnings(
    path: &Path,
    modifications: &[(String, OsString)],
    raw_values: bool,
    on_commit: impl FnOnce() -> Result<(), String>,
) -> (Result<WriteOutcome, String>, Vec<String>) {
    let (plan, warnings) = prepare_write_file(path, modifications, raw_values);
    (
        write_plan_file(path, &plan, on_commit).map(|done| done.outcome),
        warnings,
    )
}

/// Applies every `-TAG=VALUE` of one file through the library's write
/// transaction ([`apply_tag_changes_counted`]): each value is parsed first
/// (typed as the tag's registry entry declares; wrapping every value as a
/// String made Integer/Rational/DateTime tags unsettable), then all of them
/// are resolved, written in one pass, and proven together -- or none is.
/// Returns the proven set count and their resolved surviving destinations.
///
/// `global_raw_values` is `plan.raw_values` (ExifTool's `-n`, always applying
/// to every set here). A tag's own trailing `#` (`canonical_request_tag`
/// leaves it on the name, for example `"IFD0:Orientation#"`) is stripped here,
/// before the name reaches either the value parser or the write key: the
/// address resolvers downstream (`write_request::canonical_write_key`,
/// `resolve_write_key`) have no `#` handling of their own and either pass a
/// malformed `"IFD0:Orientation#"` key straight into the writer (which then
/// refuses it as "not a known EXIF tag") or, for a bare name, resolve the
/// address correctly but only after the value was already parsed -- and
/// mistyped -- as a `String`. Stripping it here, once, before either of
/// those things happens, is what makes `#` actually mean "this one tag's
/// value is raw" instead of failing outright.
fn apply_sets(
    scratch: &Path,
    sets: &[(String, OsString)],
    global_raw_values: bool,
    siblings: &[String],
    replay: &[(String, TagValue)],
) -> Result<(usize, Vec<String>, Vec<(String, TagValue)>), String> {
    if sets.is_empty() && replay.is_empty() {
        return Ok((0, Vec::new(), Vec::new()));
    }
    // The request's group deletions, so a bare name is typed by the address
    // the transaction writes it at in their presence
    // (`-MakerNotes:All= -ColorSpace#=2`: ExifIFD:ColorSpace, an integer).
    let deletions = GroupDeletions::plan(
        scratch,
        sets.iter()
            .enumerate()
            .filter(|(_, (_, value))| value.is_empty())
            .map(|(at, (tag, _))| (at, tag.strip_suffix('#').unwrap_or(tag))),
        None,
    );
    // Read once for every bare name the loop types (`Some(None)` when the
    // file cannot be read: the name is typed as spelled, and the
    // transaction reports the read failure).
    let mut baseline = None;
    let mut changes = Vec::with_capacity(sets.len());
    for (at, (set_tag, value)) in sets.iter().enumerate() {
        let (write_tag, raw_mode) = match set_tag.strip_suffix('#') {
            Some(base) => (base, true),
            None => (set_tag.as_str(), global_raw_values),
        };
        if value.is_empty() {
            // Empty value = delete tag (ExifTool -TAG= syntax)
            changes.push(TagChange::delete(write_tag.to_string()));
            continue;
        }
        // A bare name the value parser has no declared type for is typed by
        // the address the transaction will write it at (`ColorSpace` ->
        // `ExifIFD:ColorSpace`): typed by the bare name, `-ColorSpace#=1`
        // stayed a string and was refused as a type mismatch. A name that
        // does not resolve is left to the transaction, which refuses it
        // with the resolver's reason -- not a value error for an address
        // that was never going to be written.
        let resolved = if !write_tag.contains(':') && declared_alias(write_tag).is_none() {
            baseline
                .get_or_insert_with(|| read_metadata(scratch).ok())
                .as_ref()
                .map(|metadata| {
                    resolve_write_tag_in_request(
                        scratch,
                        write_tag,
                        metadata,
                        deletions.for_set_at(at),
                    )
                })
        } else {
            None
        };
        let typed_as = match &resolved {
            Some(Ok(key)) => key.as_str(),
            _ => write_tag,
        };
        // A later group deletion may cancel this set (13.59: `-ColorSpace#=junk
        // -EXIF:All=` warns and deletes EXIF), which only the transaction's
        // planner decides: a value its address cannot type then goes on as
        // the string it was, for the planner to cancel or the writer to refuse.
        let cancellable = sets[at + 1..].iter().any(|(tag, later)| {
            later.is_empty() && group_deletion(tag.strip_suffix('#').unwrap_or(tag)).is_some()
        });
        let tag_value = match parse_cli_tag_value_os_with_mode(typed_as, value, raw_mode) {
            Ok(tag_value) => tag_value,
            Err(_) if matches!(resolved, Some(Err(_))) || (resolved.is_some() && cancellable) => {
                TagValue::String(value.to_string_lossy().into_owned())
            }
            Err(e) => return Err(format!("Invalid value for {}: {}", write_tag, e)),
        };
        changes.push(TagChange::set(write_tag.to_string(), tag_value));
    }
    changes.extend(
        replay
            .iter()
            .map(|(key, value)| TagChange::set(key.clone(), value.clone())),
    );
    apply_tag_changes_counted_among(scratch, &changes, siblings)
        .map(|(_, proven_sets, destinations, values)| (proven_sets, destinations, values))
        .map_err(|e| describe_set_failure(&e, sets))
}

/// ExifTool's warning (without the `Warning: ` prefix `main` adds) when
/// `SetNewValue` cannot convert `value` for `tag` -- see
/// [`is_not_in_print_conv_reason`] -- and `None` for a value that converts, a
/// deletion, or a refusal of any other kind (which the transaction reports).
fn unconvertible_value_warning(tag: &str, value: &OsString, raw_values: bool) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let (tag, raw_mode) = match tag.strip_suffix('#') {
        Some(base) => (base, true),
        None => (tag, raw_values),
    };
    let err = parse_cli_tag_value_os_with_mode(tag, value, raw_mode).err()?;
    err.invalid_tag_value_reason()
        .filter(|reason| is_not_in_print_conv_reason(reason))
        .map(str::to_string)
}

fn bare_conversion_warning(
    tag: &str,
    value: &OsString,
    raw_values: bool,
    applicable: impl Fn(
        &crate::writers::generated_setnewvalue_address_rules::StaticNativeLookupCandidate,
    ) -> bool,
) -> Option<String> {
    if raw_values || value.is_empty() || tag.ends_with('#') {
        return None;
    }
    let text = value.to_str()?;
    let (group, reason) = rejected_bare_conversion(tag, text, applicable)?;
    Some(format!("Can't convert {group}:{tag} ({reason})"))
}

/// Whether `reason` is a refusal ExifTool's `SetNewValue` reports as a
/// warning and drops the one request for: a value that matches no entry (or
/// more than one) of a tag's PrintConv hash, or a string longer than its
/// Count allows. [`WritePlan::from_args`] warns once and drops that set.
/// A failed PrintConvInv routine (`ConvertParameter`: `Error converting
/// value ... (PrintConvInv)`) is not one of these: 13.59 reports the file
/// unchanged there, not `Nothing to do.`.
fn is_not_in_print_conv_reason(reason: &str) -> bool {
    reason.ends_with("(not in PrintConv)")
        || reason.ends_with("(matches more than one PrintConv)")
        || reason.starts_with("String too long for ")
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

/// Whether a failure message is ExifTool's own warning text -- either
/// [`sorry_refusal_message`]'s (`Warning: Sorry, ... \nNothing to do.`, the
/// PNG `XMP` literal-text-chunk case) -- rather than oxidex's
/// own `Failed to ...` / `Invalid value for ...` wrapping. The two need
/// different framing in `main.rs`'s `finish_write`: ExifTool's own words are
/// printed as-is, oxidex's diagnosis gets an `Error:` prefix.
pub fn is_exiftool_refusal_message(message: &str) -> bool {
    message.starts_with("Warning: ") && message.ends_with("\nNothing to do.")
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
