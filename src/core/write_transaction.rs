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

use crate::core::FileReader;
use crate::core::metadata_map::{MetadataMap, SourceIdentity, handle_identity};
use crate::core::operations::{
    bare_removal_is_no_op_with_reader, exif_group_in_pdf_with_reader, field_spellings,
    group_removal_takes_effect_with_reader, mie_census_with_reader,
    plan_group_deletion_with_reader, read_metadata, removal_is_no_op_with_reader, remove_field,
    resolve_write_key_in_request_with_reader, write_metadata_transaction_among,
};
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result, TagNotWritten};
use crate::io::MMapReader;
use crate::writers::write_request::{MieCensus, ensure_no_mie_copy, group_deletion};
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
    let receipt = apply_tag_changes_with_receipt(path, changes)?;
    Ok((receipt.outcome, receipt.sets))
}

/// Addresses of surviving set requests proven by the same transaction plan.
/// Seeds, cancelled requests and failed writes never produce caller attribution.
pub(crate) struct AppliedWrite {
    pub(crate) outcome: WriteOutcome,
    pub(crate) sets: usize,
    pub(crate) caller_fields: Vec<(String, String)>,
    pub(crate) original: Option<SourceIdentity>,
    pub(crate) written: Option<SourceIdentity>,
}

pub(crate) fn apply_tag_changes_with_receipt(
    path: &Path,
    changes: &[TagChange],
) -> Result<AppliedWrite> {
    apply_with_planning_hook(path, changes, || {})
}

/// Count proven sets while protecting dates shifted by the enclosing CLI
/// command. Planning, no-op decisions and copying retain the same opened file.
pub(crate) fn apply_tag_changes_counted_among(
    path: &Path,
    changes: &[TagChange],
    siblings: &[String],
) -> Result<(WriteOutcome, usize)> {
    let original = super::filesystem_metadata::open_destination(path)?;
    let receipt = apply_on_opened_with_hook(path, changes, original, siblings, || {})?;
    Ok((receipt.outcome, receipt.sets))
}

fn apply_with_planning_hook(
    path: &Path,
    changes: &[TagChange],
    after_plan: impl FnOnce(),
) -> Result<AppliedWrite> {
    let original = super::filesystem_metadata::open_destination(path)?;
    apply_on_opened_with_hook(path, changes, original, &[], after_plan)
}

pub(crate) fn apply_tag_changes_on_opened(
    path: &Path,
    changes: &[TagChange],
    original: fs::File,
) -> Result<AppliedWrite> {
    apply_on_opened_with_hook(path, changes, original, &[], || {})
}

fn apply_on_opened_with_hook(
    path: &Path,
    changes: &[TagChange],
    original: fs::File,
    siblings: &[String],
    after_plan: impl FnOnce(),
) -> Result<AppliedWrite> {
    // Every request is resolved, and every no-op decided, against the file
    // itself -- before any scratch file exists. A request that is all no-ops
    // (an absent tag's deletion, an EXIF group in a PDF, nothing at all)
    // writes nothing and so needs no writable directory.
    // Planning and copying share one authoritative opened inode. A mapped
    // reader never resolves the destination again for field/no-op decisions.
    let reader = MMapReader::from_file(original.try_clone()?)?;
    let original_identity = handle_identity(&original);
    let plan = plan_changes(path, changes, &reader, &original)?;
    drop(reader);
    after_plan();
    super::filesystem_metadata::check_identity(path, &original)?;
    if plan.steps.is_empty() {
        return Ok(AppliedWrite {
            outcome: WriteOutcome::Unchanged,
            sets: 0,
            caller_fields: Vec::new(),
            original: original_identity,
            written: original_identity,
        });
    }
    let mut proven_sets = 0;
    let mut caller_fields = Vec::new();
    let (outcome, written) = transact_opened_with(
        path,
        original,
        |scratch| {
            (proven_sets, caller_fields) = execute_plan(scratch, &plan, siblings)?;
            Ok(())
        },
        || Ok(()),
        |_, err| err,
    )?;
    Ok(AppliedWrite {
        outcome,
        sets: proven_sets,
        caller_fields,
        original: original_identity,
        written,
    })
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

/// Whether `name` is one of `names`, compared as request resolution
/// compares group and tag names: without regard to case (ExifTool's
/// `-file:filename=` names `File:FileName`).
fn one_of(names: &[&str], name: &str) -> bool {
    names.iter().any(|known| known.eq_ignore_ascii_case(name))
}

/// `map`'s row for `key` under any spelling of its case, with the key it is
/// stored under: the exact key first, then a differently cased one
/// (`ifd0:artist` for `IFD0:Artist`).
fn row_of<'m>(map: &'m MetadataMap, key: &str) -> Option<(String, &'m TagValue)> {
    if let Some(value) = map.get(key) {
        return Some((key.to_string(), value));
    }
    map.iter()
        .find(|(row, _)| row.eq_ignore_ascii_case(key))
        .map(|(row, value)| (row.clone(), value))
}

fn is_descriptive(key: &str) -> bool {
    matches!(group_and_name(key).0, Some(group) if one_of(DESCRIPTIVE_GROUPS, group))
}

fn is_file_system_fact(key: &str) -> bool {
    match group_and_name(key) {
        (Some(group), _) if group.eq_ignore_ascii_case("ExifTool") => true,
        (Some(group), name) if one_of(&["File", "System"], group) => {
            one_of(FILE_SYSTEM_FACTS, name)
        }
        _ => false,
    }
}

/// The requests a whole-map write makes of the file whose current map is
/// `baseline`. When `deletions` -- `desired` is a read of this same file
/// (`MetadataMap::read_from`) -- the rows its caller assigned are the sets,
/// and every row the view observed or its caller successfully saved
/// (`MetadataMap::read_saw`) that `baseline` still holds and `desired`
/// lacks is a row its caller removed, and a deletion. Writer seeds and
/// rows excluded by a projection never establish deletion intent. Otherwise
/// every row of `desired` that is new or
/// differs is a set, and (a map built from
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
/// (`PDF:CreateDate` / `PDF:CreationDate`) is one field: removing either
/// spelling deletes the field -- under ExifTool's name for it, the first of
/// [`field_spellings`] (13.59 knows no tag `PDF:CreationDate`) -- unless the
/// other spelling is itself a set of the field (assigned, or changed), which
/// then replaces it. The untouched alias the reader left in the map is not a
/// reason to keep the date: that silently skipped the deletion.
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
        // Provenance first (`MetadataMap::is_assigned`): a value the
        // caller assigned is a set whatever it equals -- an explicit
        // same-text XP set can need different bytes. In a read of this file,
        // a row the caller did not assign is the read's own and never a set
        // (every public mutation, `get_mut` included, assigns): not because
        // a read with options (a requested `File:JPEGQualityEstimate`)
        // produced a row the default read lacks, not because a projection
        // (`without_print_conv`) shows it in another form, and not because an
        // earlier write from the same map changed it in the file since (a
        // stale row must not revert the file). Any other map is judged by
        // value.
        // A descriptive row (`File:`, `Composite:`, ...) is not stored in the
        // file, so it has no bytes an explicit same-value set could change:
        // an assigned one is a request (and refused) only when it is new or
        // differs. A map cleared and refilled with the file's own rows
        // (#949's `values_reinserted_after_clear_are_assignments`) sets
        // nothing there.
        let held = row_of(baseline, key).map(|(_, held)| held);
        let assigned = desired.is_assigned(key) && !(is_descriptive(key) && held == Some(value));
        let is_set = if deletions {
            assigned
        } else {
            assigned || held != Some(value)
        };
        if is_set {
            changes.push(TagChange::set(key.clone(), value.clone()));
        }
    }
    if !deletions {
        return changes;
    }
    for (key, _) in baseline.iter() {
        // Read rows and successfully saved caller rows can be removals.
        // Shared identity refresh does not make writer seeds or filtered-out
        // rows visible to this view's caller.
        let held = row_of(desired, key);
        if (held.as_ref().is_some_and(|(spelling, _)| {
            !desired.removed_saved_field(key) || desired.is_assigned(spelling)
        })) || is_descriptive(key)
            || !desired.read_saw(key)
        {
            continue;
        }
        let spellings = field_spellings(key);
        // Any row spelling the alias, in any case, that sets it (a read
        // map can hold the file's `PDF:CreateDate` beside the caller's
        // `pdf:createdate`).
        let set_under_alias = spellings.iter().any(|alias| {
            !alias.eq_ignore_ascii_case(key)
                && desired.iter().any(|(row, value)| {
                    row.eq_ignore_ascii_case(alias)
                        && (desired.is_assigned(row)
                            || row_of(baseline, alias).map(|(_, held)| held) != Some(value))
                })
        });
        if set_under_alias {
            continue;
        }
        let field = spellings.first().copied().unwrap_or(key.as_str());
        let deletion = TagChange::delete(field);
        if !changes.contains(&deletion) {
            changes.push(deletion);
        }
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
    /// Deletions already satisfied by the baseline must remain satisfied
    /// after other writes, which can create mandatory fields implicitly.
    absent_deletions: Vec<Resolved<'a>>,
}

/// Whether two resolved keys address the same field (a PDF Info field has
/// two spellings).
fn same_field(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || one_of(field_spellings(a), b)
}

/// A field request before its writer-address check.
struct Pending<'a> {
    request: Resolved<'a>,
    addressed: Result<()>,
    /// Position among all steps, so group removals keep their place.
    at: usize,
}

/// The group deletions of one request, each planned once: its position in
/// the request, the `<group>:All` key [`plan_group_deletion_with_reader`] planned for it
/// (`MakerNotes:*` is `MakerNotes:All`), and whether the format's writer
/// really makes it ([`group_removal_takes_effect_with_reader`]).
#[derive(Debug, Default)]
pub(crate) struct GroupDeletions(Vec<(usize, String, bool)>);

impl GroupDeletions {
    /// Plans the group deletions among `deletions` -- each a request's
    /// position and tag -- against the opened file. Only a deletion
    /// [`plan_group_deletion_with_reader`] plans for real counts; one it proves a
    /// no-op (a note ExifTool files under EXIF) removes nothing.
    pub(crate) fn plan<'a>(
        path: &Path,
        deletions: impl IntoIterator<Item = (usize, &'a str)>,
        mie: Option<&MieCensus>,
    ) -> Self {
        let Ok(reader) = MMapReader::new(path) else {
            return Self::default();
        };
        let Ok(baseline) = read_metadata(path) else {
            return Self::default();
        };
        let owned;
        let mie = match mie {
            Some(mie) => mie,
            None => {
                owned = mie_census_with_reader(&reader).ok();
                let Some(mie) = owned.as_ref() else {
                    return Self::default();
                };
                mie
            }
        };
        Self::plan_with_reader(&baseline, &reader, deletions, mie)
    }

    fn plan_with_reader<'a>(
        baseline: &MetadataMap,
        reader: &MMapReader,
        deletions: impl IntoIterator<Item = (usize, &'a str)>,
        mie: &MieCensus,
    ) -> Self {
        Self(
            deletions
                .into_iter()
                .filter_map(|(at, tag)| {
                    let group = group_deletion(tag)?;
                    let key = plan_group_deletion_with_reader(tag, group, baseline, reader, mie)
                        .ok()??;
                    let effective = group_removal_takes_effect_with_reader(&key, baseline, reader);
                    Some((at, key, effective))
                })
                .collect(),
        )
    }

    /// What they leave of the file for the bare name set at position `at`
    /// ([`RequestDeletions`](crate::writers::exif_surgical::RequestDeletions)),
    /// as pinned 13.59 applies a command line in order:
    ///
    /// - a deletion *before* the set deletes the group, and the set then
    ///   writes every copy that remains -- so it counts only where the
    ///   format's writer really makes it: a TIFF-structured file drops
    ///   `IFD0:All`, and a raw type's ExifIFD/MakerNotes removals, as 13.59's
    ///   no-ops, and its maker note survives to be edited
    ///   (`-MakerNotes:All= -WhiteBalance#=1` on t/images/Nikon.nef writes
    ///   `[Nikon]` and `[ExifIFD] WhiteBalance`);
    /// - a deletion *after* the set cancels the set's new values in the
    ///   groups it names, whether or not the deletion itself takes effect
    ///   (`-WhiteBalance#=1 -MakerNotes:All=` on Nikon.nef writes `[ExifIFD]`
    ///   alone and keeps the note; on Canon.jpg it also deletes the note).
    /// What the deletions that take effect remove, wherever they sit in the
    /// request: what is gone from the file once it is written.
    fn effective(&self) -> crate::writers::exif_surgical::RequestDeletions {
        use crate::writers::exif_surgical::RequestDeletions;
        self.0
            .iter()
            .filter(|(_, _, effective)| *effective)
            .map(|(_, key, _)| RequestDeletions::of(key))
            .fold(RequestDeletions::default(), RequestDeletions::union)
    }

    pub(crate) fn for_set_at(&self, at: usize) -> crate::writers::exif_surgical::RequestDeletions {
        use crate::writers::exif_surgical::RequestDeletions;
        self.0
            .iter()
            .filter(|(position, _, effective)| *position > at || *effective)
            .map(|(_, key, _)| RequestDeletions::of(key))
            .fold(RequestDeletions::default(), RequestDeletions::union)
    }
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
fn plan_changes<'a>(
    path: &Path,
    changes: &'a [TagChange],
    reader: &MMapReader,
    original: &fs::File,
) -> Result<Plan<'a>> {
    let mut baseline = super::operations::read_metadata_from_reader(
        path,
        crate::parsers::DetectorMode::Signature,
        &crate::core::ReadOptions::default_full_listing(),
        reader,
    )?;
    baseline.mark_read_complete();
    baseline.set_read_handle(original);
    // The file's bytes, for the Panasonic RAW no-op decisions below (#956's
    // `rw2_ifd0`, which answer `false` for any other file) -- read only for a
    // Panasonic RAW, never the whole of every file (PR #957 review, Codex).
    let head = reader.read(0, reader.size().min(4) as usize)?;
    let file_bytes = if matches!(head, [b'I', b'I', 0x55, 0] | [b'M', b'M', 0, 0x55]) {
        reader.read(0, reader.size() as usize)?
    } else {
        &[]
    };
    let mie = mie_census_with_reader(reader)?;
    let mut refused: Vec<TagNotWritten> = Vec::new();
    // Refusals of one request, by its position: a later group deletion can
    // still cancel the request (see below), and its refusal with it.
    let mut request_refusals: Vec<(usize, &'a str, TagNotWritten)> = Vec::new();
    let mut groups: Vec<(usize, String)> = Vec::new();
    let mut pending: Vec<Pending<'a>> = Vec::new();
    // What the request's group deletions remove, for the bare names it
    // also sets (`GroupDeletions::for_set_at`).
    let deletions = GroupDeletions::plan_with_reader(
        &baseline,
        reader,
        changes
            .iter()
            .enumerate()
            .filter(|(_, change)| change.value().is_none())
            .map(|(at, change)| (at, change.tag())),
        &mie,
    );
    let gone = deletions.effective();
    // Resolve CIFF presence from its APP0 bytes only if a surviving request
    // can address it. Decoded rows cannot prove that an APP0 is absent.
    let mut ciff_presence = None;
    let mut physical_ciff = || -> Result<bool> {
        if let Some(present) = ciff_presence {
            return Ok(present);
        }
        let present = if head.starts_with(&[0xff, 0xd8]) {
            crate::writers::exif_surgical::jpeg_has_ciff(reader.read(0, reader.size() as usize)?)?
        } else {
            false
        };
        ciff_presence = Some(present);
        Ok(present)
    };
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
            // ExifTool's group deletion removes every value set in the group
            // before it (Writer.pl `SetNewValue` -> `RemoveNewValuesForGroup`),
            // whatever the file holds: the earlier requests it covers are
            // cancelled here, with their refusals, before the deletion itself
            // is planned. 13.59: `-XMP:Title=x -XMP:All=` on a file without
            // XMP is `unchanged` (#957, PRRT_kwDOQNbr5M6mQbb-); `-IFD1:Make=x
            // -EXIF:All=` on a JPEG with EXIF clears the EXIF and never asks
            // for the IFD1 set oxidex cannot make (PRRT_kwDOQNbr5M6mRRX8);
            // `-XMP-dc:Title=x -XMP:All=` where oxidex cannot delete the XMP
            // is refused for `XMP:All` alone.
            pending.retain(|earlier| {
                !group_covers(group, earlier.request.requested)
                    && !group_covers(group, &earlier.request.key)
            });
            request_refusals.retain(|(_, requested, _)| !group_covers(group, requested));
            match plan_group_deletion_with_reader(change.tag(), group, &baseline, reader, &mie) {
                Ok(Some(key)) => groups.push((at, key)),
                // Provably nothing of the group in the file.
                Ok(None) => {}
                // Kept at its place among the requests' refusals; no later
                // deletion cancels a deletion's own refusal.
                Err(ExifToolError::TagsNotWritten { tags }) => {
                    request_refusals.extend(tags.into_iter().map(|tag| (at, "", tag)));
                }
                Err(other) => return Err(other),
            }
            continue;
        }
        // #956: a bare name on a Panasonic RAW that no writable table of
        // any module names (`SensorWidth`) is pinned 13.59's "1 image files
        // unchanged", a set or a deletion alike.
        if matches!(
            crate::writers::rw2_ifd0::route_rw2_name(file_bytes, change.tag()),
            crate::writers::rw2_ifd0::Rw2Name::NoOp
        ) {
            continue;
        }
        // #945: an EXIF-group request in a PDF is ExifTool's "unchanged"
        // (a PDF carries no EXIF block), a set or a deletion alike -- for a
        // name `SetNewValue` accepts in that group; any other is refused by
        // name with the rest of the request.
        match exif_group_in_pdf_with_reader(change.tag(), reader) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(ExifToolError::TagsNotWritten { tags }) => {
                request_refusals.extend(tags.into_iter().map(|tag| (at, change.tag(), tag)));
                continue;
            }
            Err(other) => return Err(other),
        }
        if change.value().is_some()
            && let Err(error) =
                crate::writers::exif_cross_delete::date_set_keys(&baseline, change.tag())
        {
            request_refusals.push((
                at,
                change.tag(),
                TagNotWritten::new(change.tag(), error.to_string()),
            ));
            continue;
        }
        // A maker-note request in a request whose group deletion takes the
        // maker note away is a no-op: ExifTool never creates a maker-note
        // tag, and a tag or entry of a note that is gone is nothing to
        // delete. 13.59: `-MakerNotes:All= -MakerNotes:FocusMode=` on
        // t/images/Nikon.jpg, `-MakerNotes:All= -ExifIFD:MakerNoteCanon=`
        // (either order) and `-MakerNotes:All= -MakerNotes:WhiteBalance#=1`
        // on Canon.jpg each delete the note and nothing else. Where the
        // deletion does not take effect (a raw type) the note is edited.
        if makernote_request_gone(change, gone, path, &mut physical_ciff)? {
            continue;
        }
        match resolve_write_key_in_request_with_reader(
            change.tag(),
            &baseline,
            deletions.for_set_at(at),
            reader,
        ) {
            Ok((key, addressed)) => pending.push(Pending {
                request: Resolved {
                    requested: change.tag(),
                    key: if change.value().is_none() {
                        crate::writers::write_request::unit_suffix_family_key(change.tag())
                            .unwrap_or(key)
                    } else {
                        key
                    },
                    value: change.value(),
                },
                addressed,
                at,
            }),
            // A bare name whose deletion provably removes nothing is
            // ExifTool's `unchanged`, whatever the resolver would refuse to
            // write (`operations::bare_removal_is_no_op_with_reader`).
            Err(ExifToolError::TagsNotWritten { .. })
                if change.value().is_none()
                    && bare_removal_is_no_op_with_reader(change.tag(), &baseline, reader) => {}
            Err(ExifToolError::TagsNotWritten { tags }) => {
                request_refusals.extend(tags.into_iter().map(|tag| (at, change.tag(), tag)));
            }
            Err(other) => return Err(other),
        }
    }
    // In request order (a stable sort keeps one request's refusals in order).
    request_refusals.sort_by_key(|(at, _, _)| *at);
    refused.extend(request_refusals.into_iter().map(|(_, _, tag)| tag));

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
    let mut absent_deletions = Vec::new();
    for candidate in last {
        // A request pinned 13.59 also applies to a MIE trailer's EXIF copy,
        // which oxidex does not write, is refused -- asked only now, of the
        // request that survived the same-field reduction (a set a later
        // deletion overrides is never written), and before the no-op
        // decision below, which sees the main EXIF alone (a deletion MIE's
        // copy holds is no no-op). `write_request::ensure_no_mie_copy`, on
        // the census taken once above.
        let requested = candidate.request.requested;
        match ensure_no_mie_copy(
            requested.strip_suffix('#').unwrap_or(requested),
            &candidate.request.key,
            &baseline,
            candidate.request.value.is_none(),
            &mie,
        ) {
            Ok(()) => {}
            Err(ExifToolError::TagsNotWritten { tags }) => {
                refused.extend(tags);
                continue;
            }
            Err(other) => return Err(other),
        }
        // #945 / #943: a deletion that names nothing -- no row under any
        // spelling, and no entry of any EXIF block (`exif_surgical::
        // exif_request_is_no_op`) -- is a no-op, decided before the writer's
        // address guard (ExifTool 13.59: `1 image files unchanged`).
        if candidate.request.value.is_none()
            && removal_is_no_op_with_reader(&candidate.request.key, &baseline, reader)?
        {
            absent_deletions.push(candidate.request);
            continue;
        }
        // #956: a set pinned 13.59 makes nowhere in a Panasonic RAW (no outer
        // PanasonicRaw entry it writes, and the embedded JpgFromRaw absent or
        // already holding the value) leaves the file unchanged there.
        if let Some(value) = candidate.request.value
            && crate::writers::rw2_ifd0::rw2_set_is_no_op(
                file_bytes,
                &baseline,
                &candidate.request.key,
                value,
            )
        {
            continue;
        }
        // A PDF Info date the writer cannot store as one (PR #957 review,
        // Codex: `modify_tag(pdf, "PDF:CreateDate", "2020:13:02 03:04:05")`
        // bypasses the CLI's range checks).
        if let Some(value) = candidate.request.value
            && let Some(reason) =
                crate::writers::pdf_writer::pdf_date_refusal(&candidate.request.key, value)
        {
            refused.push(TagNotWritten::new(candidate.request.requested, reason));
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
        absent_deletions,
    })
}

/// Whether `change` names only the maker note, which the request's effective
/// group deletions (`gone`) remove from the file at `baseline`: a tag of a
/// maker-note group (`MakerNotes:FocusMode`, `Canon:WhiteBalance`), set or
/// deleted, or the deletion of a maker-note entry (`ExifIFD:MakerNoteCanon`,
/// `MakerNotes:MakerNoteCanon`). A JPEG's CIFF segment is a maker note too,
/// which only `MakerNotes:All` removes: while one survives, nothing is gone.
fn makernote_request_gone(
    change: &TagChange,
    gone: crate::writers::exif_surgical::RequestDeletions,
    path: &Path,
    physical_ciff: &mut impl FnMut() -> Result<bool>,
) -> Result<bool> {
    use crate::writers::generated_makernote_groups::MAKERNOTE_ROOTS;
    if !gone.makernotes {
        return Ok(false);
    }
    let Some((group, name)) = change.tag().split_once(':') else {
        return Ok(false);
    };
    let name = name.strip_suffix('#').unwrap_or(name);
    let entry = MAKERNOTE_ROOTS
        .iter()
        .any(|root| root.entry != "CIFF" && root.entry.eq_ignore_ascii_case(name));
    let makernote_group = group.eq_ignore_ascii_case("MakerNotes")
        || crate::writers::exif_surgical::is_makernote_group(group);
    let named = if entry {
        change.value().is_none()
            && (makernote_group
                || group.eq_ignore_ascii_case("ExifIFD")
                || group.eq_ignore_ascii_case("EXIF"))
    } else {
        makernote_group
    };
    if !named {
        return Ok(false);
    }
    // ExifIFD:All leaves a direct IFD0 note in the JPEG/TIFF carrier. A
    // grouped setter can still reach it even if our reader decoded no row.
    // Entry deletions explicitly naming ExifIFD remain gone.
    if !entry
        && gone.exif_ifd_only()
        && crate::core::operations::conversion_makernote_census(path).surviving_exif_ifd_clear > 0
    {
        return Ok(false);
    }
    // A JPEG's CIFF segment survives every deletion but `MakerNotes:All`;
    // only a request that can address it stays live beside it: `MakerNotes:`
    // or a group of the CIFF root's closure (`Canon:FocalLength`), never an
    // EXIF maker-note entry (13.59: `-EXIF:All= -ExifIFD:MakerNoteCanon=` on
    // Canon.jpg carrying ExifTool.jpg's CIFF deletes the EXIF note).
    let can_address_ciff = !entry
        && (group.eq_ignore_ascii_case("MakerNotes")
            || MAKERNOTE_ROOTS.iter().any(|root| {
                root.entry == "CIFF"
                    && root
                        .closure
                        .iter()
                        .any(|reached| reached.eq_ignore_ascii_case(group))
            }));
    Ok(gone.ciff || !can_address_ciff || !physical_ciff()?)
}

/// Whether ExifTool's `-<group>:All=` removes a value set earlier for `tag`
/// (the requested spelling, or the address it resolved to) -- Writer.pl's
/// `RemoveNewValuesForGroup` with its `%removeGroups`: a tag of that group;
/// for `EXIF` and `IFD0` (which also removes `EXIF` and `MakerNotes`
/// values), every EXIF directory and maker-note group; for `ExifIFD` (also
/// `MakerNotes`, `InteropIFD`), those; for the XMP (XML) family, any `XMP-*`
/// (`XML-*`) group, and an `XMP:Name` request the namespace of whose tag is
/// the deleted one (`XMP:Title` is `XMP-dc:Title`, which 13.59's
/// `-XMP-dc:All=` cancels). An ungrouped name, or a namespace the registry
/// does not tell, is not covered: its request stays, and is written or
/// refused by name.
pub(crate) fn group_covers(group: &str, tag: &str) -> bool {
    let Some((tag_group, _)) = tag.rsplit_once(':') else {
        return false;
    };
    if tag_group.eq_ignore_ascii_case(group) {
        return true;
    }
    let (group_lower, tag_lower) = (group.to_ascii_lowercase(), tag_group.to_ascii_lowercase());
    let exif_directory = is_exif_directory(tag_group) || tag_lower == "exif";
    let makernote =
        tag_lower == "makernotes" || crate::writers::exif_surgical::is_makernote_group(tag_group);
    match group_lower.as_str() {
        "exif" | "ifd0" if exif_directory || makernote => return true,
        "exififd" if tag_lower == "interopifd" || makernote => return true,
        "makernotes" if makernote => return true,
        _ => {}
    }
    match group_lower.split_once('-') {
        None if group_lower == "xmp" || group_lower == "xml" => {
            tag_lower.starts_with(&format!("{group_lower}-"))
        }
        Some((family, _)) if (family == "xmp" || family == "xml") && tag_lower == family => {
            crate::tag_db::tag_registry::get_tag_descriptor(tag).is_some_and(|descriptor| {
                matches!(descriptor.id(), oxidex_tags::TagId::Named(id)
                if id.split_once(':').is_some_and(|(namespace, _)| {
                    namespace.eq_ignore_ascii_case(group)
                }))
            })
        }
        _ => false,
    }
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
fn execute_plan(
    path: &Path,
    plan: &Plan<'_>,
    protected: &[String],
) -> Result<(usize, Vec<(String, String)>)> {
    let mut siblings: Vec<String> = plan
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::Field(request) if request.value.is_some() => Some(request.key.clone()),
            _ => None,
        })
        .collect();
    siblings.extend_from_slice(protected);
    let mut index = 0;
    while index < plan.steps.len() {
        let is_group = matches!(plan.steps[index], Step::Group(_));
        let end = plan.steps[index..]
            .iter()
            .position(|step| matches!(step, Step::Group(_)) != is_group)
            .map_or(plan.steps.len(), |offset| index + offset);
        let mut desired = read_metadata(path)?;
        let mut removed: Vec<String> = Vec::new();
        for step in &plan.steps[index..end] {
            match step {
                // A group removal's post-condition is the writer's: #943's
                // expansion decides which blocks the group names (and
                // whether the request is a no-op for this file), and its
                // verifier refuses a write that leaves any of them.
                Step::Group(key) => removed.push(key.clone()),
                Step::Field(request) => {
                    remove_field(&mut desired, &request.key);
                    // A family-0 deletion names all native EXIF rows with
                    // this leaf. The TIFF writer decides whether a surfaced
                    // entry was deleted from the native key's absence in the
                    // desired map, so remove those rows before it runs.
                    if request.value.is_none()
                        && crate::writers::write_request::unit_suffix_family_key(request.requested)
                            .is_some()
                        && let (Some(group), leaf) = group_and_name(&request.key)
                        && group.eq_ignore_ascii_case("EXIF")
                    {
                        let native: Vec<String> = desired
                            .iter()
                            .filter(|(key, _)| {
                                matches!(group_and_name(key), (Some(directory), name)
                                    if is_exif_directory(directory) && name.eq_ignore_ascii_case(leaf))
                            })
                            .map(|(key, _)| key.clone())
                            .collect();
                        for key in native {
                            remove_field(&mut desired, &key);
                        }
                    }
                    match request.value {
                        Some(value) => {
                            // `insert` marks the occurrence assigned
                            // (#949's per-occurrence provenance): an explicit
                            // set whatever its value, which
                            // `write_metadata_transaction` reads back off the
                            // map (a same-value set a removal covers is still
                            // a set, not a carried row).
                            desired.insert(request.key.clone(), value.clone());
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
        write_metadata_transaction_among(path, &desired, &removed, &siblings)
            .map_err(typed_refusal)?;
        index = end;
    }
    // The read-back proves every field request no later group removal
    // covers -- which, since planning cancels the requests a later deletion
    // covers, is every one. Proving only the requests after the last group
    // removal left an unrelated earlier set unproven and uncounted: 13.59's
    // `-IFD0:Artist=<its value> -GPS:All=` on a JPEG without GPS is
    // `1 image files updated` by the same-value-set rule, and oxidex said
    // `unchanged` (#957, PRRT_kwDOQNbr5M6mRRYE).
    let proven: Vec<&Resolved<'_>> = plan
        .steps
        .iter()
        .enumerate()
        .filter_map(|(at, step)| match step {
            Step::Field(request) => {
                let covered = plan.steps[at + 1..].iter().any(|later| {
                    matches!(later, Step::Group(key)
                    if group_deletion(key).is_some_and(|group| {
                        group_covers(group, request.requested)
                            || group_covers(group, &request.key)
                    }))
                });
                (!covered).then_some(request)
            }
            Step::Group(_) => None,
        })
        .chain(plan.absent_deletions.iter())
        .collect();
    prove_in_effect(path, &proven, &plan.baseline)?;
    let caller_fields: Vec<_> = proven
        .iter()
        .filter(|request| request.value.is_some())
        .map(|request| (request.requested.to_owned(), request.key.clone()))
        .collect();
    Ok((caller_fields.len(), caller_fields))
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

/// Whether `group` is an EXIF directory ([`EXIF_DIRECTORIES`], any case),
/// counting a numbered SubIFD (`SubIFD0`, `SubIFD1`, ... -- the keys a DNG's
/// SubIFD chain is read under) as the SubIFD it is: an `EXIF:All=` or
/// `IFD0:All=` deletion cancels an earlier `SubIFD1:` set as it cancels a
/// `SubIFD:` one (PR #957 review, Codex).
fn is_exif_directory(group: &str) -> bool {
    one_of(EXIF_DIRECTORIES, group)
        || group
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("SubIFD"))
            && group.len() > 6
            && group[6..].bytes().all(|b| b.is_ascii_digit())
}

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
                    if is_exif_directory(g) && n.eq_ignore_ascii_case(name))
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
    // Streamed, never held whole (PR #957 review, Codex): a large JPEG, PNG
    // or PDF used to cost three whole-file buffers here (the original, the
    // written copy, and the commit's) on top of the writer's own.
    let original = super::filesystem_metadata::open_destination(path)
        .map_err(|e| fail(ScratchStep::ReadOriginal, e.into()))?;
    transact_opened_with(path, original, apply, on_commit, fail).map(|(outcome, _)| outcome)
}

fn transact_opened_with<E>(
    path: &Path,
    mut original: fs::File,
    apply: impl FnOnce(&Path) -> std::result::Result<(), E>,
    on_commit: impl FnOnce() -> std::result::Result<(), E>,
    fail: impl Fn(ScratchStep, ExifToolError) -> E,
) -> std::result::Result<(WriteOutcome, Option<SourceIdentity>), E> {
    let metadata = super::filesystem_metadata::Snapshot::read(&original)
        .map_err(|e| fail(ScratchStep::ReadOriginal, e.into()))?;
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
    std::io::copy(&mut original, &mut scratch.as_file())
        .map_err(|e| fail(ScratchStep::CreateCopy, e.into()))?;
    apply(scratch.path())?;

    super::filesystem_metadata::check_identity(path, &original)
        .map_err(|e| fail(ScratchStep::ReadOriginal, e.into()))?;
    if same_bytes_opened(&mut original, scratch.path())
        .map_err(|e| fail(ScratchStep::ReadCopy, e.into()))?
    {
        return Ok((WriteOutcome::Unchanged, handle_identity(&original)));
    }
    // The commit is `write_atomic`'s: the replacement (the scratch copy,
    // which lives beside `path`) takes the original's filesystem metadata,
    // is flushed, and is renamed over `path`. A writer may have replaced
    // the scratch file by a rename of its own, so it is reopened by path.
    let commit = || -> std::io::Result<Option<SourceIdentity>> {
        let replacement = super::filesystem_metadata::open_destination(scratch.path())?;
        metadata.restore(&original, &replacement)?;
        replacement.sync_all()?;
        super::filesystem_metadata::check_identity(path, &original)?;
        Ok(handle_identity(&replacement))
    };
    let written = commit().map_err(|e| fail(ScratchStep::Commit, e.into()))?;
    on_commit()?;
    scratch
        .persist(path)
        .map_err(|e| fail(ScratchStep::Commit, e.error.into()))?;
    Ok((WriteOutcome::Updated, written))
}

/// Whether the files at `a` and `b` hold the same bytes, compared in chunks.
#[cfg(test)]
fn same_bytes(a: &Path, b: &Path) -> std::io::Result<bool> {
    same_bytes_opened(&mut fs::File::open(a)?, b)
}

fn same_bytes_opened(a: &mut fs::File, b: &Path) -> std::io::Result<bool> {
    use std::io::{Read, Seek};
    a.rewind()?;
    let mut b = fs::File::open(b)?;
    if a.metadata()?.len() != b.metadata()?.len() {
        return Ok(false);
    }
    let (mut left, mut right) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let n = a.read(&mut left)?;
        if n == 0 {
            // Equal lengths: `b` is exhausted too.
            return Ok(true);
        }
        b.read_exact(&mut right[..n])?;
        if left[..n] != right[..n] {
            return Ok(false);
        }
    }
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

    /// `transact_with` compares the written copy with the original in chunks.
    #[test]
    fn same_bytes_compares_length_and_content() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            path
        };
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let mut changed = big.clone();
        changed[150_000] ^= 1;
        let (a, b) = (write("a", &big), write("b", &big));
        assert!(same_bytes(&a, &b).unwrap());
        assert!(!same_bytes(&a, &write("c", &changed)).unwrap());
        assert!(!same_bytes(&a, &write("d", &big[..big.len() - 1])).unwrap());
        assert!(same_bytes(&write("e", b""), &write("f", b"")).unwrap());
    }

    /// A group deletion before a set counts only where it takes effect; one
    /// after it cancels the set's copies in its groups either way (#960
    /// review 4113017923; pinned 13.59 on t/images/Nikon.nef:
    /// `-MakerNotes:All= -WhiteBalance#=1` writes `[Nikon]` too,
    /// `-WhiteBalance#=1 -MakerNotes:All=` writes `[ExifIFD]` alone).
    #[test]
    fn group_deletions_follow_argument_order() {
        let raw = GroupDeletions(vec![(1, "MakerNotes:All".to_string(), false)]);
        assert!(raw.for_set_at(0).makernotes);
        assert!(!raw.for_set_at(2).makernotes);
        let jpeg = GroupDeletions(vec![(1, "MakerNotes:All".to_string(), true)]);
        assert!(jpeg.for_set_at(0).makernotes && jpeg.for_set_at(2).makernotes);
        assert!(jpeg.for_set_at(2).ciff);
        let carrier = GroupDeletions(vec![(0, "IFD0:All".to_string(), false)]);
        assert_eq!(
            carrier.for_set_at(1),
            crate::writers::exif_surgical::RequestDeletions::default()
        );
    }

    #[test]
    fn replacement_after_planning_refuses_active_and_absent_delete_plans() {
        let mut accepted = Vec::new();
        for active in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("original.jpg");
            let replacement = dir.path().join("replacement.jpg");
            fs::copy("tests/fixtures/jpeg/simple/synthetic_001.jpg", &path).unwrap();
            apply_tag_changes(&path, &[TagChange::delete("IFD0:Artist")]).unwrap();
            fs::copy(&path, &replacement).unwrap();
            apply_tag_changes(
                &replacement,
                &[TagChange::set("IFD0:Artist", s("replacement"))],
            )
            .unwrap();
            let bytes = fs::read(&replacement).unwrap();
            let changes = if active {
                vec![TagChange::set("IFD0:Make", s("planned on original"))]
            } else {
                vec![TagChange::delete("IFD0:Artist")]
            };
            let result = apply_with_planning_hook(&path, &changes, || {
                fs::rename(&replacement, &path).unwrap();
            });
            if result.is_ok() {
                accepted.push(active);
            }
            if result.is_err() {
                assert_eq!(fs::read(&path).unwrap(), bytes);
            }
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        }
        assert!(
            accepted.is_empty(),
            "replacement accepted for active plans: {accepted:?}"
        );
    }

    #[test]
    fn cancelled_and_superseded_sets_have_only_surviving_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("receipt.jpg");
        fs::copy("tests/fixtures/jpeg/simple/synthetic_001.jpg", &path).unwrap();
        let cancelled = apply_tag_changes_with_receipt(
            &path,
            &[
                TagChange::set("EXIF:Artist", s("cancelled")),
                TagChange::delete("EXIF:All"),
            ],
        )
        .unwrap();
        assert_eq!(cancelled.sets, 0);
        assert!(cancelled.caller_fields.is_empty());
        let surviving = apply_tag_changes_with_receipt(
            &path,
            &[
                TagChange::set("Artist", s("superseded")),
                TagChange::set("EXIF:Artist", s("surviving")),
            ],
        )
        .unwrap();
        assert_eq!(surviving.sets, 1);
        assert_eq!(
            surviving.caller_fields,
            vec![("EXIF:Artist".to_string(), "IFD0:Artist".to_string())]
        );
        assert!(
            apply_tag_changes_with_receipt(
                &path,
                &[
                    TagChange::set("Artist", s("surviving")),
                    TagChange::set("NotAGroup:NotATag", s("refused")),
                ]
            )
            .is_err()
        );
    }

    #[test]
    fn planning_reads_the_opened_inode_even_after_path_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original.jpg");
        let replacement = dir.path().join("replacement.jpg");
        fs::copy("tests/fixtures/jpeg/simple/synthetic_001.jpg", &path).unwrap();
        apply_tag_changes(&path, &[TagChange::delete("IFD0:Artist")]).unwrap();
        fs::copy(&path, &replacement).unwrap();
        apply_tag_changes(
            &replacement,
            &[TagChange::set("IFD0:Artist", s("replacement"))],
        )
        .unwrap();
        let original = super::super::filesystem_metadata::open_destination(&path).unwrap();
        fs::rename(&replacement, &path).unwrap();
        let reader = MMapReader::from_file(original.try_clone().unwrap()).unwrap();
        let changes = [TagChange::delete("IFD0:Artist")];
        let plan = plan_changes(&path, &changes, &reader, &original).unwrap();
        assert!(!plan.baseline.contains_key("IFD0:Artist"));
        assert!(plan.steps.is_empty());
        drop(reader);
        assert!(apply_tag_changes_on_opened(&path, &changes, original).is_err());
        assert_eq!(
            read_metadata(&path).unwrap().get_string("IFD0:Artist"),
            Some("replacement")
        );
    }

    #[test]
    fn replaced_original_is_refused_even_when_scratch_matches_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.jpg");
        let replacement = dir.path().join("replacement.jpg");
        fs::write(&path, b"original").unwrap();
        fs::write(&replacement, b"replacement").unwrap();
        let result = transact_with(
            &path,
            |scratch| -> Result<()> {
                fs::rename(&replacement, &path)?;
                fs::write(scratch, b"replacement")?;
                Ok(())
            },
            || panic!("no backup for a refused replacement"),
            |_, error| error,
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

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

    /// PR #966 review 4112736391: a transaction walks the file's MIE
    /// trailers once, whatever its number of requests -- not once per
    /// surviving request and group deletion (each walk is file-sized).
    #[test]
    fn a_transaction_takes_one_mie_census() {
        let Some(source) = crate::test_support::pinned_t_images_fixture_path("ExifTool.jpg") else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mie.jpg");
        std::fs::copy(&source, &path).unwrap();
        let changes = [
            TagChange::delete("IFD0:Artist"),
            TagChange::delete("IFD0:Software"),
            TagChange::delete("ExifIFD:UserComment"),
            TagChange::delete("IFD0:Copyright"),
            TagChange::delete("GPS:All"),
            TagChange::delete("IFD1:All"),
        ];
        crate::parsers::mie::TRAILER_WALKS.with(|walks| walks.set(0));
        let original = super::super::filesystem_metadata::open_destination(&path).unwrap();
        let reader = MMapReader::from_file(original.try_clone().unwrap()).unwrap();
        let _ = plan_changes(&path, &changes, &reader, &original);
        assert_eq!(
            crate::parsers::mie::TRAILER_WALKS.with(std::cell::Cell::get),
            1,
            "one MIE census for {} requests",
            changes.len()
        );
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
        desired.set_read_source(Path::new("a.jpg"));
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
        read.set_read_source(Path::new("a.jpg"));
        assert!(changes_between(&baseline, &read, true).is_empty());
    }

    /// Codex thread PRRT_kwDOQNbr5M6mO8E4 (#957): a read map written again
    /// after a write is compared with a file that has changed since its
    /// read. A row the file gained (an ExifIFD's seeded entries) is not one
    /// the caller removed, and a row the read saw that the file now holds
    /// with another value is not one the caller set: neither is a request.
    /// What the caller did remove, and did assign, still are.
    #[test]
    fn a_stale_read_map_requests_only_what_its_caller_changed() {
        let read_rows = map(&[("IFD0:Make", s("Acme")), ("IFD0:Model", s("R5"))]);
        let mut desired = read_rows.clone();
        desired.mark_read_complete();
        desired.set_read_source(Path::new("a.jpg"));
        desired.insert("ExifIFD:LensModel", s("L1"));
        desired.insert("IFD0:Artist", s("x"));
        desired.remove("IFD0:Model");
        // The file after a first write from `desired`, plus a side effect on
        // Make that the map never saw.
        let file = map(&[
            ("IFD0:Make", s("Acme (rewritten)")),
            ("IFD0:Model", s("R5")),
            ("ExifIFD:LensModel", s("L1")),
            ("ExifIFD:ExifVersion", s("0232")),
            ("ExifIFD:ColorSpace", s("Uncalibrated")),
        ]);
        assert_eq!(
            changes_between(&file, &desired, true),
            vec![
                TagChange::set("ExifIFD:LensModel", s("L1")),
                TagChange::set("IFD0:Artist", s("x")),
                TagChange::delete("IFD0:Model"),
            ]
        );
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

    /// Codex thread PRRT_kwDOQNbr5M6mOo0z (#957): removing either spelling
    /// of a PDF Info date from a read map deletes the field, under ExifTool's
    /// tag name (13.59 has no `PDF:CreationDate` tag); the alias the reader
    /// left behind no longer suppresses it. A set under the other spelling
    /// replaces the field instead.
    #[test]
    fn a_pdf_info_field_is_one_field_under_two_spellings() {
        let date = || s("2024:01:01 00:00:00");
        let baseline = map(&[("PDF:CreateDate", date()), ("PDF:CreationDate", date())]);
        // Each read saw both spellings; `rows` is what is left of it.
        let read = |rows: &[(&str, TagValue)]| {
            let mut read = baseline_read(&baseline);
            for key in ["PDF:CreateDate", "PDF:CreationDate"] {
                if !rows.iter().any(|(kept, _)| *kept == key) {
                    read.remove(key);
                }
            }
            read
        };
        let deleted = vec![TagChange::delete("PDF:CreateDate")];
        for kept in ["PDF:CreateDate", "PDF:CreationDate"] {
            assert_eq!(
                changes_between(&baseline, &read(&[(kept, date())]), true),
                deleted,
                "only {kept} left"
            );
        }
        assert_eq!(changes_between(&baseline, &read(&[]), true), deleted);
        // Unchanged, both spellings: nothing.
        assert!(changes_between(&baseline, &baseline_read(&baseline), true).is_empty());
        // The field set under its other spelling: that set, no deletion.
        let mut desired = read(&[("PDF:CreationDate", date())]);
        desired.insert("PDF:CreationDate", s("2020:01:01 00:00:00"));
        assert_eq!(
            changes_between(&baseline, &desired, true),
            vec![TagChange::set("PDF:CreationDate", s("2020:01:01 00:00:00"))]
        );
    }

    fn baseline_read(baseline: &MetadataMap) -> MetadataMap {
        let mut read = baseline.clone();
        read.mark_read_complete();
        read.set_read_source(Path::new("a.pdf"));
        read
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
