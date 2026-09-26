//! The library's one write transaction: every public write API -- the map API
//! (`write_metadata`, `Metadata::save`/`write_to`, `CopyBuilder::execute`,
//! `shift_metadata_dates` on PNG/PDF), the single-tag APIs (`modify_tag`,
//! `remove_tag`), `copy_metadata`, the C ABI's `exiftool_write_file` and the
//! CLI's `-TAG=VALUE` -- applies its requests through [`apply_tag_changes`].
//!
//! # The guarantee
//!
//! `Ok` means every requested change is in the file. Anything less is an
//! error and the file is byte-identical to before the call:
//!
//! 1. **Resolution before writing.** A `<group>:All` deletion is planned by
//!    #945's `plan_group_deletion` and handed to #943's group-wide
//!    expansion (whose verifier is its post-condition), or proven a no-op,
//!    or refused by name; a `<group>:All` set is refused. An EXIF-group
//!    request in a PDF is
//!    ExifTool's "unchanged" and a deletion that names nothing is a no-op
//!    (#945's `exif_group_in_pdf` / `removal_is_no_op`). Every other request
//!    is resolved to the address
//!    pinned ExifTool 13.59 writes it at, or refused (#945's
//!    `resolve_write_address` / `writers::write_request`): an ungrouped name
//!    is resolved (`XPTitle` -> `IFD0:XPTitle`) or refused, and a key the
//!    format's writer would drop -- `XMP:Title` in a JPEG, TIFF or PNG, a
//!    `File:` key, most `IFD1:` keys -- is refused. Every refused key of the
//!    request is named in one [`ExifToolError::TagsNotWritten`]; none of the
//!    request is applied.
//! 2. **The format writer's own post-condition.** The request is applied to
//!    a private copy with the format writer, which checks its own payload
//!    (#943's `exif_surgical::exif_request_is_no_op` and
//!    `verify_exif_write`, inside `operations::write_metadata_transaction`).
//! 3. **A read-back proof.** The copy is re-read and every resolved address
//!    must hold its requested value -- the exact field bytes for an
//!    IFD0/ExifIFD/GPS/IFD1 entry (`exif_surgical::stored_entry_matches`, #945's
//!    proof), the reader's value otherwise -- and every deleted address must
//!    be gone. A change the read-back cannot find refuses the write.
//! 4. **Commit.** The original is replaced atomically, and only when the
//!    bytes differ ([`WriteOutcome::Unchanged`] otherwise).
//!
//! Before this module the CLI resolved its requests (#945) while the library
//! entry points handed the caller's map straight to the format writer, which
//! skips every group it does not write: `write_metadata` with an `XMP:Title`
//! in a JPEG returned `Ok(())` and wrote nothing.

use crate::core::metadata_map::MetadataMap;
use crate::core::operations::{
    exif_group_in_pdf, field_spellings, metadata_holds, plan_group_deletion, read_metadata,
    removal_is_no_op, remove_field, resolve_write_key_for, write_metadata_transaction,
};
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result, TagNotWritten};
use crate::writers::atomic_writer::write_atomic;
use crate::writers::write_request::group_deletion;
use std::fs;
use std::path::Path;

/// One requested change to a file's metadata.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TagChange {
    /// Set `tag` (as the caller spells it: `XPTitle`, `IFD0:Artist`,
    /// `EXIF:ISO`) to `value`.
    Set {
        /// The tag as the caller spells it.
        tag: String,
        /// The value to store.
        value: TagValue,
    },
    /// Delete `tag` (as the caller spells it).
    Delete {
        /// The tag as the caller spells it.
        tag: String,
    },
}

impl TagChange {
    /// A request to set `tag` to `value`.
    pub fn set<T: Into<String>>(tag: T, value: TagValue) -> Self {
        TagChange::Set {
            tag: tag.into(),
            value,
        }
    }

    /// A request to delete `tag`.
    pub fn delete<T: Into<String>>(tag: T) -> Self {
        TagChange::Delete { tag: tag.into() }
    }

    /// The tag as the caller spelled it.
    pub fn tag(&self) -> &str {
        match self {
            TagChange::Set { tag, .. } | TagChange::Delete { tag } => tag,
        }
    }

    /// The value to set; `None` for a deletion.
    pub fn value(&self) -> Option<&TagValue> {
        match self {
            TagChange::Set { value, .. } => Some(value),
            TagChange::Delete { .. } => None,
        }
    }
}

/// What a successful write did to the file: ExifTool's `WriteInfo` return
/// values (ExifTool.pod: 1 = "file written OK", 2 = "file written but no
/// changes made"). Every library write call returns it; the CLI's
/// `image files updated` / `unchanged` counts are these, and the C ABI's
/// `exiftool_write_file_with_outcome` reports them as
/// `EXIFTOOL_WRITE_UPDATED` (1) / `EXIFTOOL_WRITE_UNCHANGED` (2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum WriteOutcome {
    /// The file's bytes changed.
    Updated,
    /// Every change was already in effect (or nothing was requested) and the
    /// bytes are identical; the file was not rewritten.
    #[default]
    Unchanged,
}

/// Applies `changes` to the file at `path` as one transaction (see the
/// module docs): all of them, proven by a read-back, or none.
///
/// # Errors
///
/// [`ExifToolError::TagsNotWritten`] naming every key that would not be
/// written; any read, validation or I/O error. The file is untouched then.
pub fn apply_tag_changes(path: &Path, changes: &[TagChange]) -> Result<WriteOutcome> {
    apply_tag_changes_counted(path, changes).map(|(outcome, _)| outcome)
}

/// [`apply_tag_changes`], also returning how many sets were applied and
/// proven -- as opposed to requests decided no-ops up front (an EXIF-group
/// request in a PDF, a deletion that names nothing). The CLI needs it: a
/// byte-identical result is ExifTool's `updated` only when a set was proven
/// in effect, never when every request was a no-op.
pub(crate) fn apply_tag_changes_counted(
    path: &Path,
    changes: &[TagChange],
) -> Result<(WriteOutcome, usize)> {
    // Every request is resolved, and every no-op decided, against the file
    // itself -- before any scratch file exists. A request that is all no-ops
    // (an absent tag's deletion, an EXIF group in a PDF, nothing at all)
    // writes nothing and so needs no writable directory.
    let plan = plan_changes(path, changes)?;
    if plan.steps.is_empty() {
        return Ok((WriteOutcome::Unchanged, 0));
    }
    let mut proven_sets = 0;
    let outcome = transact(path, |scratch| {
        proven_sets = execute_plan(scratch, &plan)?;
        Ok(())
    })?;
    Ok((outcome, proven_sets))
}

/// Groups whose rows describe the file (or the read) rather than being
/// stored in it: the reader derives them. A map row of one of these groups
/// that the file also carries is never a deletion request.
const DESCRIPTIVE_GROUPS: &[&str] = &["File", "System", "Composite", "ExifTool"];

/// The file-system facts among them (ExifTool's family-1 `System` group plus
/// the reader's `FileSize`): they change without any write -- reading the
/// file moves `FileAccessDate` -- and describe the directory entry, not the
/// metadata. A map row naming one is carried, never a request; the map API
/// does not rename files or set their dates.
const FILE_SYSTEM_FACTS: &[&str] = &[
    "FileName",
    "Directory",
    "FileSize",
    "FileModifyDate",
    "FileAccessDate",
    "FileInodeChangeDate",
    "FileCreateDate",
    "FilePermissions",
    "FileAttributes",
];

fn group_and_name(key: &str) -> (Option<&str>, &str) {
    match key.split_once(':') {
        Some((group, name)) => (Some(group), name),
        None => (None, key),
    }
}

fn is_descriptive(key: &str) -> bool {
    matches!(group_and_name(key).0, Some(group) if DESCRIPTIVE_GROUPS.contains(&group))
}

fn is_file_system_fact(key: &str) -> bool {
    match group_and_name(key) {
        (Some("ExifTool"), _) => true,
        (Some("File" | "System"), name) => FILE_SYSTEM_FACTS.contains(&name),
        _ => false,
    }
}

/// The requests a whole-map write makes of the file whose current map is
/// `baseline`: every row of `desired` that is new or differs is a set. When
/// `deletions` -- `desired` is a complete read of this same file
/// (`MetadataMap::read_from`) -- every row of `baseline` that `desired` lacks
/// is a row its caller removed, and a deletion. Otherwise (a map built from
/// scratch, or read from another file) nothing is deleted: ExifTool's
/// SetNewValue model, where a tag nobody named is never touched (ExifTool.pod,
/// SetNewValue / WriteInfo; maintainer decision on #951, 2026-09-24).
///
/// The rows that describe the file rather than being stored in it are the
/// exception ([`DESCRIPTIVE_GROUPS`]): the file-system facts are never
/// requests, and a descriptive row `desired` lacks is not a deletion -- but a
/// descriptive row it adds or changes (`File:Comment` on a file without one,
/// a different `File:ImageWidth`) is a set, which no writer makes and is
/// refused.
///
/// A PDF Info field the reader surfaces under two spellings
/// (`PDF:CreateDate` / `PDF:CreationDate`) is one field: dropping one
/// spelling while the other stays is not a deletion of it.
pub(crate) fn changes_between(
    baseline: &MetadataMap,
    desired: &MetadataMap,
    deletions: bool,
) -> Vec<TagChange> {
    let mut changes = Vec::new();
    for (key, value) in desired.iter() {
        if is_file_system_fact(key) {
            continue;
        }
        // Provenance first (`MetadataMap::assigned_after_read`): a value the
        // caller assigned is a set whatever it equals -- an explicit
        // same-text XP set can need different bytes. In a read of this file,
        // a row the caller did not assign is the file's own: a set only if
        // the file holds that row with another value (an in-place
        // `get_mut`), never because a read with options (a requested
        // `File:JPEGQualityEstimate`) produced a row the default read lacks.
        // Any other map is judged by value.
        let assigned = desired.assigned_after_read(key);
        let is_set = if deletions {
            assigned || baseline.get(key).is_some_and(|held| held != value)
        } else {
            assigned || baseline.get(key) != Some(value)
        };
        if is_set {
            changes.push(TagChange::set(key.clone(), value.clone()));
        }
    }
    if !deletions {
        return changes;
    }
    for (key, _) in baseline.iter() {
        if desired.contains_key(key) || is_descriptive(key) || metadata_holds(desired, key) {
            continue;
        }
        changes.push(TagChange::delete(key.clone()));
    }
    changes
}

/// One request resolved to the address it is written at.
struct Resolved<'a> {
    requested: &'a str,
    key: String,
    value: Option<&'a TagValue>,
}

/// One step of a planned transaction, in request order.
enum Step<'a> {
    /// A set or deletion of one resolved field.
    Field(Resolved<'a>),
    /// A `<group>:All` removal handed to the writers' group-wide expansion
    /// (#943), as `operations::plan_group_deletion` (#945) decides it.
    Group(String),
}

/// A transaction, resolved against the file before anything is written.
struct Plan<'a> {
    /// The file's map at planning time: what the proof compares against.
    baseline: MetadataMap,
    /// The requests that are not no-ops, in request order.
    steps: Vec<Step<'a>>,
}

/// Whether two resolved keys address the same field (a PDF Info field has
/// two spellings).
fn same_field(a: &str, b: &str) -> bool {
    a == b || field_spellings(a).contains(&b)
}

/// A field request before its writer-address check.
struct Pending<'a> {
    request: Resolved<'a>,
    addressed: Result<()>,
    /// Position among all steps, so group removals keep their place.
    at: usize,
}

/// Resolves every request against the file at `path`, in request order, and
/// drops the no-ops: all refusals are collected into one
/// [`ExifToolError::TagsNotWritten`]. Nothing is written.
///
/// ExifTool applies a file's requests in order, a later one for the same
/// field replacing an earlier one (13.59: `-IFD0:Artist=x -IFD0:Artist=`
/// deletes Artist; `-IFD0:XPTitle=x -IFD0:XPTitle=` on a file without one is
/// `unchanged`). So same-field requests are reduced to the last one *first*,
/// and only then is a surviving deletion asked whether it names nothing
/// (#945's `removal_is_no_op`) -- an earlier set of the field cannot be
/// written by a deletion the baseline alone calls a no-op.
fn plan_changes<'a>(path: &Path, changes: &'a [TagChange]) -> Result<Plan<'a>> {
    let baseline = read_metadata(path)?;
    let mut refused: Vec<TagNotWritten> = Vec::new();
    let mut groups: Vec<(usize, String)> = Vec::new();
    let mut pending: Vec<Pending<'a>> = Vec::new();
    for (at, change) in changes.iter().enumerate() {
        // `-GROUP:All=` is a group deletion, never a tag named `All`
        // (`write_request::group_deletion`): it only deletes.
        if let Some(group) = group_deletion(change.tag()) {
            if change.value().is_some() {
                refused.push(TagNotWritten::new(
                    change.tag(),
                    format!(
                        "{group}:All names every tag of the group; it can only be \
                         deleted (-{group}:All=)"
                    ),
                ));
                continue;
            }
            match plan_group_deletion(path, change.tag(), group) {
                Ok(Some(key)) => groups.push((at, key)),
                Ok(None) => {} // provably nothing to delete
                Err(ExifToolError::TagsNotWritten { tags }) => refused.extend(tags),
                Err(other) => return Err(other),
            }
            continue;
        }
        // #945: an EXIF-group request in a PDF is ExifTool's "unchanged"
        // (a PDF carries no EXIF block), a set or a deletion alike -- for a
        // name `SetNewValue` accepts in that group; any other is refused by
        // name with the rest of the request.
        match exif_group_in_pdf(path, change.tag()) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(ExifToolError::TagsNotWritten { tags }) => {
                refused.extend(tags);
                continue;
            }
            Err(other) => return Err(other),
        }
        match resolve_write_key_for(path, change.tag(), &baseline) {
            Ok((key, addressed)) => pending.push(Pending {
                request: Resolved {
                    requested: change.tag(),
                    key,
                    value: change.value(),
                },
                addressed,
                at,
            }),
            Err(ExifToolError::TagsNotWritten { tags }) => refused.extend(tags),
            Err(other) => return Err(other),
        }
    }

    // The last request for a field replaces every earlier one.
    let replaced: Vec<bool> = (0..pending.len())
        .map(|index| {
            pending[index + 1..]
                .iter()
                .any(|later| same_field(&later.request.key, &pending[index].request.key))
        })
        .collect();
    let last: Vec<Pending<'a>> = pending
        .into_iter()
        .zip(replaced)
        .filter_map(|(candidate, replaced)| (!replaced).then_some(candidate))
        .collect();

    let mut fields: Vec<(usize, Resolved<'a>)> = Vec::new();
    for candidate in last {
        // #945 / #943: a deletion that names nothing -- no row under any
        // spelling, and no entry of any EXIF block (`exif_surgical::
        // exif_request_is_no_op`) -- is a no-op, decided before the writer's
        // address guard (ExifTool 13.59: `1 image files unchanged`).
        if candidate.request.value.is_none()
            && removal_is_no_op(path, &candidate.request.key, &baseline)?
        {
            continue;
        }
        match candidate.addressed {
            Ok(()) => fields.push((candidate.at, candidate.request)),
            Err(ExifToolError::TagsNotWritten { tags }) => refused.extend(tags),
            Err(other) => return Err(other),
        }
    }
    if !refused.is_empty() {
        return Err(ExifToolError::TagsNotWritten { tags: refused });
    }

    let mut steps: Vec<(usize, Step<'a>)> = fields
        .into_iter()
        .map(|(at, request)| (at, Step::Field(request)))
        .chain(groups.into_iter().map(|(at, key)| (at, Step::Group(key))))
        .collect();
    steps.sort_by_key(|(at, _)| *at);
    Ok(Plan {
        baseline,
        steps: steps.into_iter().map(|(_, step)| step).collect(),
    })
}

/// Runs a [`Plan`] on the private copy at `path`; returns the number of sets
/// applied and proven.
///
/// Field requests and `<group>:All` removals keep their request order: each
/// maximal run of field requests is one writer pass, as is each run of
/// group removals (13.59: `-EXIF:All= -IFD0:Artist=x` leaves exactly
/// `[IFD0] Artist`, `-IFD0:Artist=x -EXIF:All=` leaves no EXIF). A plan with
/// no group removal is one pass, as ExifTool applies all of a file's tags at
/// once (a mandatory-tag seeding decision, for instance, sees the whole
/// request). The writer's own no-op decision and post-write check
/// (`exif_surgical::{exif_request_is_no_op, verify_exif_write}`, #943) run
/// inside every pass (`write_metadata_transaction`).
fn execute_plan(path: &Path, plan: &Plan<'_>) -> Result<usize> {
    let mut index = 0;
    while index < plan.steps.len() {
        let is_group = matches!(plan.steps[index], Step::Group(_));
        let end = plan.steps[index..]
            .iter()
            .position(|step| matches!(step, Step::Group(_)) != is_group)
            .map_or(plan.steps.len(), |offset| index + offset);
        let mut desired = read_metadata(path)?;
        let mut removed: Vec<String> = Vec::new();
        // The keys this pass sets: explicit sets, whatever their value
        // (#943's `write_metadata_transaction`: a same-value set a removal
        // covers is still a set, not a carried row).
        let mut assigned: Vec<String> = Vec::new();
        for step in &plan.steps[index..end] {
            match step {
                // A group removal's post-condition is the writer's: #943's
                // expansion decides which blocks the group names (and
                // whether the request is a no-op for this file), and its
                // verifier refuses a write that leaves any of them.
                Step::Group(key) => removed.push(key.clone()),
                Step::Field(request) => {
                    remove_field(&mut desired, &request.key);
                    match request.value {
                        Some(value) => {
                            desired.insert(request.key.clone(), value.clone());
                            assigned.push(request.key.clone());
                        }
                        // The key goes along: an EXIF entry the reader
                        // surfaces no row for has no key to take out of the
                        // map, yet `-ExifIFD:ApplicationNotes=` still names
                        // it for deletion.
                        None => removed.push(request.key.clone()),
                    }
                }
            }
        }
        write_metadata_transaction(path, &desired, &removed, &assigned).map_err(typed_refusal)?;
        index = end;
    }
    // The read-back proves every field request no later group removal can
    // have overridden; one a later `<group>:All` covers was proven by the
    // writer's verifier in its own pass, and the group removal decides its
    // final state (as in ExifTool).
    let last_group = plan
        .steps
        .iter()
        .rposition(|step| matches!(step, Step::Group(_)));
    let proven: Vec<&Resolved<'_>> = plan
        .steps
        .iter()
        .enumerate()
        .filter(|(at, _)| last_group.is_none_or(|group| *at > group))
        .filter_map(|(_, step)| match step {
            Step::Field(request) => Some(request),
            Step::Group(_) => None,
        })
        .collect();
    prove_in_effect(path, &proven, &plan.baseline)?;
    Ok(proven
        .iter()
        .filter(|request| request.value.is_some())
        .count())
}

/// A format writer's own refusal of one key (`exif_surgical`,
/// `tiff_surgical`: `Cannot write tag '<tag>': <reason>`) as the typed
/// [`ExifToolError::TagsNotWritten`]; every other error is returned as is.
fn typed_refusal(err: ExifToolError) -> ExifToolError {
    if let ExifToolError::UnsupportedFormat { message } = &err {
        if let Some(rest) = message.strip_prefix("Cannot write tag '")
            && let Some((tag, reason)) = rest.split_once("': ")
        {
            return ExifToolError::tag_not_written(tag, reason);
        }
        // #943's post-write check (`exif_surgical::verify_exif_write`)
        // names the key whose set or deletion the writer did not make.
        if let Some(rest) = message.strip_prefix("EXIF write verification failed: '")
            && let Some((tag, _)) = rest.split_once('\'')
        {
            return ExifToolError::tag_not_written(tag, message.clone());
        }
    }
    err
}

/// Groups `EXIF:<name>` may land in: `EXIF` is family 0, not a directory.
const EXIF_DIRECTORIES: &[&str] = &["IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD", "SubIFD"];

/// The values `stored` holds at the address `key` names: the key itself and
/// its other spellings; for the family spelling `EXIF:<name>`, that name in
/// any EXIF directory; for an ungrouped name (only the generated route of a
/// Panasonic RAW keeps one), that name in any group that stores metadata.
fn rows_at<'a>(stored: &'a MetadataMap, key: &str) -> Vec<&'a TagValue> {
    match group_and_name(key) {
        (Some(group), name) if group.eq_ignore_ascii_case("EXIF") => stored
            .iter()
            .filter(|(row, _)| {
                matches!(group_and_name(row), (Some(g), n)
                    if EXIF_DIRECTORIES.contains(&g) && n.eq_ignore_ascii_case(name))
            })
            .map(|(_, value)| value)
            .collect(),
        (Some(_), _) => std::iter::once(key)
            .chain(field_spellings(key).iter().copied())
            .filter_map(|spelling| stored.get(spelling))
            .collect(),
        (None, name) => stored
            .iter()
            .filter(|(row, _)| {
                !is_descriptive(row) && group_and_name(row).1.eq_ignore_ascii_case(name)
            })
            .map(|(_, value)| value)
            .collect(),
    }
}

/// `value` as text, where it has one plain spelling.
fn text_of(value: &TagValue) -> Option<String> {
    match value {
        TagValue::String(text) => Some(text.clone()),
        TagValue::Integer(number) => Some(number.to_string()),
        TagValue::Float(number) => Some(number.to_string()),
        TagValue::Rational {
            numerator,
            denominator,
        } => Some(format!("{numerator}/{denominator}")),
        TagValue::DateTime(date) => Some(date.format("%Y:%m:%d %H:%M:%S").to_string()),
        _ => None,
    }
}

/// `value` as a number, where it is one (a numeric string included).
fn number_of(value: &TagValue) -> Option<f64> {
    match value {
        TagValue::Integer(number) => Some(*number as f64),
        TagValue::Float(number) => Some(*number),
        TagValue::Rational {
            numerator,
            denominator,
        } if *denominator != 0 => Some(f64::from(*numerator) / f64::from(*denominator)),
        TagValue::String(text) => {
            let text = text.trim();
            match text.split_once('/') {
                Some((n, d)) => {
                    let (n, d): (f64, f64) = (n.trim().parse().ok()?, d.trim().parse().ok()?);
                    (d != 0.0).then(|| n / d)
                }
                None => text.parse().ok(),
            }
        }
        _ => None,
    }
}

/// Whether the reader's `held` value is the `requested` one: equal, or the
/// same text, or the same number (the reader types a value by its registry
/// entry, so `"300"` may read back as `300`).
fn values_agree(held: &TagValue, requested: &TagValue) -> bool {
    held == requested
        || text_of(held).is_some_and(|text| Some(text) == text_of(requested))
        || number_of(held).is_some_and(|number| Some(number) == number_of(requested))
}

/// The chunks of a PNG, as (type, data); empty for any other file.
fn png_chunks(file_bytes: &[u8]) -> Vec<(&[u8], &[u8])> {
    const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    let mut chunks = Vec::new();
    if !file_bytes.starts_with(PNG_SIGNATURE) {
        return chunks;
    }
    let mut at = PNG_SIGNATURE.len();
    while let Some(header) = file_bytes.get(at..at + 8) {
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let data = at + 8;
        let Some(chunk) = data
            .checked_add(length)
            .and_then(|end| file_bytes.get(data..end))
        else {
            break;
        };
        chunks.push((&header[4..8], chunk));
        at = data + length + 4; // skip the CRC
    }
    chunks
}

/// The bytes [`stored_entry_matches`] reads EXIF entries from: a PNG's
/// `eXIf` chunk data (a bare TIFF structure); the file itself otherwise (a
/// JPEG, whose APP1 it finds, or a TIFF-structured file).
///
/// [`stored_entry_matches`]: crate::writers::exif_surgical::stored_entry_matches
fn exif_payload(file_bytes: &[u8]) -> &[u8] {
    if png_chunks(file_bytes).is_empty() {
        return file_bytes;
    }
    png_chunks(file_bytes)
        .into_iter()
        .find(|(kind, _)| *kind == b"eXIf")
        .map_or(&[], |(_, data)| data)
}

/// `PNG:XMP` is the PNG writer's key for a raw XMP packet
/// (`png_writer::XMP_ITXT_KEYWORD`); the reader surfaces the packet as
/// parsed `XMP:` rows, never under that key. The packets of the file's
/// uncompressed `XML:com.adobe.xmp` iTXt chunks, which is how the writer
/// stores it; `None` when `key` is not `PNG:XMP` or the file is not a PNG.
fn png_xmp_packets<'a>(file_bytes: &'a [u8], key: &str) -> Option<Vec<&'a [u8]>> {
    let chunks = png_chunks(file_bytes);
    if key != "PNG:XMP" || chunks.is_empty() {
        return None;
    }
    Some(
        chunks
            .into_iter()
            .filter(|(kind, _)| *kind == b"iTXt")
            .filter_map(|(_, data)| {
                let rest = data.strip_prefix(b"XML:com.adobe.xmp\0")?;
                // compression flag 0, method, then language and translated
                // keyword, each NUL-terminated
                let rest = rest.strip_prefix(&[0u8])?.get(1..)?;
                let language_end = rest.iter().position(|b| *b == 0)?;
                let rest = &rest[language_end + 1..];
                let translated_end = rest.iter().position(|b| *b == 0)?;
                Some(&rest[translated_end + 1..])
            })
            .collect(),
    )
}

/// Why the file, read back, does not hold `value` at `key` -- `None` when it
/// does: the exact field bytes this writer emits for an IFD0/ExifIFD/GPS
/// entry it can locate (the reader normalizes -- a stored `"Canon   "` reads
/// as `"Canon"` -- so its map cannot prove those), the reader's value
/// otherwise.
fn set_not_in_effect(
    file_bytes: &[u8],
    stored: &MetadataMap,
    key: &str,
    value: &TagValue,
    packets: Option<Vec<&[u8]>>,
) -> Option<String> {
    match crate::writers::exif_surgical::stored_entry_matches(exif_payload(file_bytes), key, value)
    {
        Some(true) => return None,
        Some(false) => {
            return Some(format!(
                "after writing, the stored {key} entry does not hold exactly the \
                 requested value (the writer left it as it was); nothing was written"
            ));
        }
        None => {}
    }
    if let Some(packets) = packets {
        let requested = value.as_string().map(str::as_bytes);
        return (!packets.iter().any(|packet| Some(*packet) == requested)).then(|| {
            "after writing, the PNG holds no XMP packet equal to the requested one; \
             nothing was written"
                .to_string()
        });
    }
    let held = rows_at(stored, key);
    // A PDF Info date is proven by the date the writer stores
    // (`pdf_writer::stored_pdf_date`: sub-seconds dropped, as ExifTool's
    // `WritePDFValue` drops them), not by the requested text (#945).
    let stored_date = crate::writers::pdf_writer::stored_pdf_date(key, value);
    if held.iter().any(|held| {
        values_agree(held, value)
            || stored_date.is_some()
                && crate::writers::pdf_writer::stored_pdf_date(key, held) == stored_date
    }) {
        return None;
    }
    Some(if held.is_empty() {
        format!("after writing, {key} is absent; nothing was written")
    } else {
        format!(
            "after writing, {key} reads back as {held:?}, not the requested value; \
             nothing was written"
        )
    })
}

/// The read-back proof: every set's address holds its value and every
/// deletion's address is gone, in the file at `path` as written.
fn prove_in_effect(path: &Path, requests: &[&Resolved<'_>], baseline: &MetadataMap) -> Result<()> {
    let stored = read_metadata(path)?;
    let file_bytes = fs::read(path)?;
    let mut failed = Vec::new();
    for request in requests {
        // `PNG:XMP` names an ordinary text chunk when the file had one whose
        // keyword is literally `XMP` (the reader reported it, and the PNG
        // writer edits that chunk: `png_writer::plan_text_chunks`); only
        // otherwise is it the raw `XML:com.adobe.xmp` packet route.
        let packets = if baseline.contains_key(&request.key) {
            None
        } else {
            png_xmp_packets(&file_bytes, &request.key)
        };
        let reason = match request.value {
            Some(value) => set_not_in_effect(&file_bytes, &stored, &request.key, value, packets),
            None if packets.map_or(!rows_at(&stored, &request.key).is_empty(), |packets| {
                !packets.is_empty()
            }) =>
            {
                Some(format!(
                    "after writing, {} is still present; nothing was written",
                    request.key
                ))
            }
            None => None,
        };
        if let Some(reason) = reason {
            failed.push(TagNotWritten::new(request.requested, reason));
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(ExifToolError::TagsNotWritten { tags: failed })
    }
}

/// Which step of [`transact_with`] failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScratchStep {
    /// Reading the original file.
    ReadOriginal,
    /// Creating or filling the private copy.
    CreateCopy,
    /// Reading the private copy back after the changes.
    ReadCopy,
    /// Replacing the original with the copy.
    Commit,
}

/// Runs `apply` on a private copy of `path` (same directory and extension:
/// format detection may consult it), then replaces `path` with the copy only
/// if `apply` succeeded and the bytes differ -- the one place a write
/// decides between [`WriteOutcome::Updated`] and
/// [`WriteOutcome::Unchanged`]. `on_commit` runs just before an update
/// replaces the original (the CLI's `--backup`), and not at all otherwise.
pub(crate) fn transact_with<E>(
    path: &Path,
    apply: impl FnOnce(&Path) -> std::result::Result<(), E>,
    on_commit: impl FnOnce() -> std::result::Result<(), E>,
    fail: impl Fn(ScratchStep, ExifToolError) -> E,
) -> std::result::Result<WriteOutcome, E> {
    let original = fs::read(path).map_err(|e| fail(ScratchStep::ReadOriginal, e.into()))?;
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut suffix = std::ffi::OsString::new();
    if let Some(extension) = path.extension() {
        suffix.push(".");
        suffix.push(extension);
    }
    let scratch = tempfile::Builder::new()
        .prefix(".oxidex-write-")
        .suffix(&suffix)
        .tempfile_in(dir)
        .map_err(|e| fail(ScratchStep::CreateCopy, e.into()))?;
    fs::write(scratch.path(), &original).map_err(|e| fail(ScratchStep::CreateCopy, e.into()))?;

    apply(scratch.path())?;

    let written = fs::read(scratch.path()).map_err(|e| fail(ScratchStep::ReadCopy, e.into()))?;
    if written == original {
        return Ok(WriteOutcome::Unchanged);
    }
    on_commit()?;
    write_atomic(path, &written).map_err(|e| fail(ScratchStep::Commit, e))?;
    Ok(WriteOutcome::Updated)
}

/// [`transact_with`] for library callers: errors are returned as they are.
pub(crate) fn transact(
    path: &Path,
    apply: impl FnOnce(&Path) -> Result<()>,
) -> Result<WriteOutcome> {
    transact_with(path, apply, || Ok(()), |_, err| err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(rows: &[(&str, TagValue)]) -> MetadataMap {
        let mut map = MetadataMap::new();
        for (key, value) in rows {
            map.insert(*key, value.clone());
        }
        map
    }

    fn s(text: &str) -> TagValue {
        TagValue::new_string(text)
    }

    /// A map read from the file, then edited: the rows the caller
    /// assigned (whatever their value) and the rows it removed are the
    /// requests; the read's own rows are not.
    #[test]
    fn a_read_map_requests_what_its_caller_assigned_and_removed() {
        let baseline = map(&[
            ("File:FileName", s("a.jpg")),
            ("File:FileType", s("JPEG")),
            ("Composite:ImageSize", s("8x6")),
            ("IFD0:Make", s("Canon")),
            ("IFD0:Model", s("R6")),
            ("IFD0:Artist", s("me")),
            ("XMP:Title", s("t")),
        ]);
        let mut desired = baseline.clone();
        // A row an optioned read adds that the default read lacks.
        desired.insert("File:JPEGQualityEstimate", s("92"));
        desired.mark_read_complete();
        desired.insert("IFD0:Model", s("R5"));
        desired.insert("IFD0:Artist", s("me")); // explicit same-value set
        desired.insert("XPTitle", s("v"));
        desired.insert("File:Comment", s("c"));
        desired.insert("File:FileName", s("b.jpg")); // a file-system fact
        desired.remove("XMP:Title");
        desired.remove("File:FileType"); // descriptive: never a deletion
        assert_eq!(
            changes_between(&baseline, &desired, true),
            vec![
                TagChange::set("IFD0:Model", s("R5")),
                TagChange::set("IFD0:Artist", s("me")),
                TagChange::set("XPTitle", s("v")),
                TagChange::set("File:Comment", s("c")),
                TagChange::delete("XMP:Title"),
            ]
        );
        // The file's own map, read and untouched, requests nothing.
        let mut read = baseline.clone();
        read.mark_read_complete();
        assert!(changes_between(&baseline, &read, true).is_empty());
    }

    /// A map built from scratch: every row is the caller's, a set whatever
    /// it equals, and nothing is deleted.
    #[test]
    fn a_from_scratch_map_sets_every_row_and_deletes_nothing() {
        let baseline = map(&[("IFD0:Make", s("Canon")), ("XMP:Title", s("t"))]);
        let desired = map(&[
            ("IFD0:Make", s("Canon")),
            ("IFD0:Model", s("R5")),
            ("File:FileAccessDate", s("now")),
        ]);
        assert_eq!(
            changes_between(&baseline, &desired, false),
            vec![
                TagChange::set("IFD0:Make", s("Canon")),
                TagChange::set("IFD0:Model", s("R5")),
            ]
        );
    }

    #[test]
    fn a_pdf_info_field_is_one_field_under_two_spellings() {
        let baseline = map(&[
            ("PDF:CreateDate", s("2024:01:01 00:00:00")),
            ("PDF:CreationDate", s("2024:01:01 00:00:00")),
        ]);
        let mut desired = map(&[("PDF:CreateDate", s("2024:01:01 00:00:00"))]);
        desired.mark_read_complete();
        assert!(changes_between(&baseline, &desired, true).is_empty());
    }

    #[test]
    fn read_back_values_agree_across_reader_typing() {
        assert!(values_agree(&TagValue::Integer(300), &s("300")));
        assert!(values_agree(&s("2.8"), &TagValue::Float(2.8)));
        assert!(values_agree(
            &TagValue::Rational {
                numerator: 1,
                denominator: 100
            },
            &s("1/100")
        ));
        assert!(!values_agree(&s("Canon"), &s("canon")));
        assert!(!values_agree(&TagValue::Integer(300), &s("301")));
    }

    #[test]
    fn a_writer_refusal_of_one_key_is_typed() {
        let err = typed_refusal(ExifToolError::unsupported_format(
            "Cannot write tag 'IFD1:Make': it requires a SubIFD",
        ));
        assert_eq!(
            err.tags_not_written(),
            [TagNotWritten::new("IFD1:Make", "it requires a SubIFD")]
        );
        let verified = typed_refusal(ExifToolError::unsupported_format(
            "EXIF write verification failed: 'IFD1:XResolution' was set but no entry \
             exists at its address; nothing was written",
        ));
        assert_eq!(verified.tags_not_written()[0].tag, "IFD1:XResolution");
        let other = typed_refusal(ExifToolError::unsupported_format("BMP"));
        assert!(other.tags_not_written().is_empty());
    }
}
