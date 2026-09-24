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
//! 1. **Resolution before writing.** An EXIF-group request in a PDF is
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
//!    `verify_exif_write`, inside `operations::write_metadata_with_removals`).
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
    exif_group_in_pdf, field_spellings, metadata_holds, read_metadata, removal_is_no_op,
    remove_field, resolve_write_key_for, write_metadata_with_removals,
};
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result, TagNotWritten};
use crate::writers::atomic_writer::write_atomic;
use std::fs;
use std::path::Path;

/// One requested change to a file's metadata.
#[derive(Debug, Clone, PartialEq)]
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

/// What a successful write did to the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The file's bytes changed.
    Updated,
    /// Every change was already in effect and the bytes are identical; the
    /// file was not rewritten.
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
    let mut proven_sets = 0;
    let outcome = transact(path, |scratch| {
        proven_sets = apply_on(scratch, changes)?;
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
/// `baseline`: every row of `desired` that is new or differs is a set, and
/// every row of `baseline` that `desired` lacks is a deletion. The rows that
/// describe the file rather than being stored in it are the exception
/// ([`DESCRIPTIVE_GROUPS`]): the file-system facts are never requests, and a
/// descriptive row `desired` lacks is not a deletion -- but a descriptive row
/// it adds or changes (`File:Comment` on a file without one, a different
/// `File:ImageWidth`) is a set, which no writer makes and is refused.
///
/// A PDF Info field the reader surfaces under two spellings
/// (`PDF:CreateDate` / `PDF:CreationDate`) is one field: dropping one
/// spelling while the other stays is not a deletion of it.
pub(crate) fn changes_between(baseline: &MetadataMap, desired: &MetadataMap) -> Vec<TagChange> {
    let mut changes = Vec::new();
    for (key, value) in desired.iter() {
        if is_file_system_fact(key) || baseline.get(key) == Some(value) {
            continue;
        }
        changes.push(TagChange::set(key.clone(), value.clone()));
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

/// Whether two resolved keys address the same field (a PDF Info field has
/// two spellings).
fn same_field(a: &str, b: &str) -> bool {
    a == b || field_spellings(a).contains(&b)
}

/// The transaction body, run on the private copy at `path`.
/// Returns the number of sets applied and proven.
fn apply_on(path: &Path, changes: &[TagChange]) -> Result<usize> {
    let baseline = read_metadata(path)?;
    let mut refused: Vec<TagNotWritten> = Vec::new();
    let mut resolved: Vec<Resolved<'_>> = Vec::new();
    for change in changes {
        // #945: an EXIF-group request in a PDF is ExifTool's "unchanged"
        // (a PDF carries no EXIF block), a set or a deletion alike.
        if exif_group_in_pdf(path, change.tag())? {
            continue;
        }
        let (key, addressed) = match resolve_write_key_for(path, change.tag(), &baseline) {
            Ok(resolution) => resolution,
            Err(ExifToolError::TagsNotWritten { tags }) => {
                refused.extend(tags);
                continue;
            }
            Err(other) => return Err(other),
        };
        // #945 / #943: a deletion that names nothing -- no row under any
        // spelling, and no entry of any EXIF block (`exif_surgical::
        // exif_request_is_no_op`) -- is a no-op, decided before the writer's
        // address guard (ExifTool 13.59: `1 image files unchanged`).
        if change.value().is_none() && removal_is_no_op(path, &key, &baseline)? {
            continue;
        }
        match addressed {
            Ok(()) => resolved.push(Resolved {
                requested: change.tag(),
                key,
                value: change.value(),
            }),
            Err(ExifToolError::TagsNotWritten { tags }) => refused.extend(tags),
            Err(other) => return Err(other),
        }
    }
    if !refused.is_empty() {
        return Err(ExifToolError::TagsNotWritten { tags: refused });
    }

    // A later request for the same field replaces an earlier one, as a later
    // `-TAG=` does in ExifTool; only the last is applied and proven.
    let effective: Vec<&Resolved<'_>> = resolved
        .iter()
        .enumerate()
        .filter(|(at, request)| {
            !resolved[at + 1..]
                .iter()
                .any(|later| same_field(&later.key, &request.key))
        })
        .map(|(_, request)| request)
        .collect();
    if effective.is_empty() {
        return Ok(0); // every request was a no-op: nothing to write
    }

    // All requests in one writer pass, as ExifTool applies all of a file's
    // tags at once (a mandatory-tag seeding decision, for instance, sees
    // the whole request). The writer's own no-op decision and post-write
    // check (`exif_surgical::{exif_request_is_no_op, verify_exif_write}`,
    // #943) run inside `write_metadata_with_removals`.
    let mut desired = baseline.clone();
    let mut removed: Vec<String> = Vec::new();
    for request in &effective {
        remove_field(&mut desired, &request.key);
        match request.value {
            Some(value) => {
                desired.insert(request.key.clone(), value.clone());
            }
            // The key goes along: an EXIF entry the reader surfaces no row
            // for has no key to take out of the map, yet
            // `-ExifIFD:ApplicationNotes=` still names it for deletion.
            None => removed.push(request.key.clone()),
        }
    }
    write_metadata_with_removals(path, &desired, &removed).map_err(typed_refusal)?;
    prove_in_effect(path, &effective)?;
    Ok(effective
        .iter()
        .filter(|request| request.value.is_some())
        .count())
}

/// A format writer's own refusal of one key (`exif_surgical`,
/// `tiff_surgical`: `Cannot write tag '<tag>': <reason>`) as the typed
/// [`ExifToolError::TagsNotWritten`]; every other error is returned as is.
fn typed_refusal(err: ExifToolError) -> ExifToolError {
    if let ExifToolError::UnsupportedFormat { message } = &err
        && let Some(rest) = message.strip_prefix("Cannot write tag '")
        && let Some((tag, reason)) = rest.split_once("': ")
    {
        return ExifToolError::tag_not_written(tag, reason);
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
    if let Some(packets) = png_xmp_packets(file_bytes, key) {
        let requested = value.as_string().map(str::as_bytes);
        return (!packets.iter().any(|packet| Some(*packet) == requested)).then(|| {
            "after writing, the PNG holds no XMP packet equal to the requested one; \
             nothing was written"
                .to_string()
        });
    }
    let held = rows_at(stored, key);
    if held.iter().any(|held| values_agree(held, value)) {
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
fn prove_in_effect(path: &Path, requests: &[&Resolved<'_>]) -> Result<()> {
    let stored = read_metadata(path)?;
    let file_bytes = fs::read(path)?;
    let mut failed = Vec::new();
    for request in requests {
        let reason = match request.value {
            Some(value) => set_not_in_effect(&file_bytes, &stored, &request.key, value),
            None if png_xmp_packets(&file_bytes, &request.key)
                .map_or(!rows_at(&stored, &request.key).is_empty(), |packets| {
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

    #[test]
    fn a_whole_map_write_requests_what_differs() {
        let baseline = map(&[
            ("File:FileName", s("a.jpg")),
            ("File:FileAccessDate", s("then")),
            ("File:FileType", s("JPEG")),
            ("Composite:ImageSize", s("8x6")),
            ("IFD0:Make", s("Canon")),
            ("IFD0:Model", s("R6")),
            ("XMP:Title", s("t")),
        ]);
        let desired = map(&[
            ("File:FileName", s("b.jpg")),
            ("File:FileAccessDate", s("now")),
            ("IFD0:Make", s("Canon")),
            ("IFD0:Model", s("R5")),
            ("XPTitle", s("v")),
            ("File:Comment", s("c")),
        ]);
        assert_eq!(
            changes_between(&baseline, &desired),
            vec![
                TagChange::set("IFD0:Model", s("R5")),
                TagChange::set("XPTitle", s("v")),
                TagChange::set("File:Comment", s("c")),
                TagChange::delete("XMP:Title"),
            ]
        );
        // The file's own map requests nothing.
        assert!(changes_between(&baseline, &baseline).is_empty());
    }

    #[test]
    fn a_pdf_info_field_is_one_field_under_two_spellings() {
        let baseline = map(&[
            ("PDF:CreateDate", s("2024:01:01 00:00:00")),
            ("PDF:CreationDate", s("2024:01:01 00:00:00")),
        ]);
        let desired = map(&[("PDF:CreateDate", s("2024:01:01 00:00:00"))]);
        assert!(changes_between(&baseline, &desired).is_empty());
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
        let other = typed_refusal(ExifToolError::unsupported_format("BMP"));
        assert!(other.tags_not_written().is_empty());
    }
}
