//! Scoped raw IFD edits for a generated writer's resolved operations.
//!
//! The caller must supply source-authorized tag identity, type and already
//! encoded bytes in the file's byte order. This layer never consults tag names
//! or the tag registry. It copies changed directories to the end of the file,
//! retaining every untouched record, value offset and next-directory pointer.
//! Public generated writers use this primitive for their admitted directories.
//! IFD1 carrier support awaits source-derived public operation admission.

use super::*;
use crate::parsers::common::exif_types::ExifType;

#[derive(Debug, Clone)]
pub(crate) enum EntryMutation {
    Set {
        field_type: u16,
        count: u32,
        bytes: Vec<u8>,
    },
    Delete,
}

#[derive(Debug, Clone)]
pub(crate) struct ScopedEntryEdit {
    pub ifd: IfdKind,
    pub tag_id: u16,
    pub mutation: EntryMutation,
}

fn invalid(message: &str) -> ExifToolError {
    ExifToolError::parse_error(message)
}

/// Apply one resolved edit per physical directory/tag identity. Errors return
/// no output and never mutate the input. Set(empty encoded ASCII) must still
/// include its native terminal NUL; deletion is a distinct operation.
pub(crate) fn apply_entry_edits(file: &[u8], edits: &[ScopedEntryEdit]) -> Result<Vec<u8>> {
    let scan = scan_tiff(file)?;
    let bo = scan.byte_order;
    let directories = validate_directory_layout(file, &scan)?;
    checked_append_bounds(file.len(), 0)?;
    for (index, edit) in edits.iter().enumerate() {
        if !matches!(
            edit.ifd,
            IfdKind::Ifd0 | IfdKind::ExifIfd | IfdKind::Gps | IfdKind::Ifd1
        ) {
            return Err(invalid("Raw edits require IFD0, ExifIFD, GPS or IFD1"));
        }
        if matches!(edit.tag_id, EXIF_IFD_POINTER | GPS_IFD_POINTER) {
            return Err(invalid("Directory links are managed by the carrier"));
        }
        if edits[..index]
            .iter()
            .any(|prior| prior.ifd == edit.ifd && prior.tag_id == edit.tag_id)
        {
            return Err(invalid("Multiple edits address the same raw IFD entry"));
        }
        if let EntryMutation::Set {
            field_type,
            count,
            bytes,
        } = &edit.mutation
        {
            let width = ExifType::from_u16(*field_type)
                .ok_or_else(|| invalid("Unsupported encoded TIFF field type"))?
                .size_in_bytes();
            if (*count as usize).checked_mul(width) != Some(bytes.len()) {
                return Err(invalid("Encoded bytes do not match TIFF type and count"));
            }
        }
    }

    let mut out = file.to_vec();
    let mut root_edits: Vec<_> = edits
        .iter()
        .filter(|edit| edit.ifd == IfdKind::Ifd0)
        .cloned()
        .collect();
    for (group, offset, pointer_tag) in [
        (IfdKind::ExifIfd, scan.exif_ifd_offset, EXIF_IFD_POINTER),
        (IfdKind::Gps, scan.gps_ifd_offset, GPS_IFD_POINTER),
    ] {
        let changes: Vec<_> = edits
            .iter()
            .filter(|edit| edit.ifd == group)
            .cloned()
            .collect();
        if changes.is_empty() {
            continue;
        }
        if let DirectoryRewrite::Present(new_at) =
            rewrite_directory(&mut out, offset, &changes, bo, None, false)?
        {
            let new_at = u32::try_from(new_at).map_err(too_big)?;
            let mut encoded = vec![0; 4];
            put_u32(&mut encoded, new_at, bo);
            root_edits.push(ScopedEntryEdit {
                ifd: IfdKind::Ifd0,
                tag_id: pointer_tag,
                mutation: EntryMutation::Set {
                    field_type: LONG_TYPE,
                    count: 1,
                    bytes: encoded,
                },
            });
        }
    }
    let ifd1_changes: Vec<_> = edits
        .iter()
        .filter(|edit| edit.ifd == IfdKind::Ifd1)
        .cloned()
        .collect();
    let ifd1_next = if ifd1_changes.is_empty() {
        None
    } else {
        match rewrite_directory(
            &mut out,
            directories.ifd1_offset,
            &ifd1_changes,
            bo,
            None,
            true,
        )? {
            DirectoryRewrite::Unchanged => None,
            DirectoryRewrite::Present(offset) => Some(u32::try_from(offset).map_err(too_big)?),
            // WriteExif omits an emptied IFD1 rather than leaving a linked
            // zero-entry directory. Repoint the copied root to no next IFD.
            DirectoryRewrite::Removed => Some(0),
        }
    };

    if let DirectoryRewrite::Present(new_at) = rewrite_directory(
        &mut out,
        Some(scan.ifd0_offset),
        &root_edits,
        bo,
        ifd1_next,
        false,
    )? {
        let new_at = u32::try_from(new_at).map_err(too_big)?;
        put_u32(&mut out[4..8], new_at, bo);
    }
    Ok(out)
}

/// Return the first linked IFD1's physical entry count.  This is structural
/// carrier state, used by the public transaction to apply source mandatory
/// defaults only while native would be creating that directory.
pub(crate) fn ifd1_entry_count(file: &[u8]) -> Result<Option<u16>> {
    let scan = scan_tiff(file)?;
    let layout = validate_directory_layout(file, &scan)?;
    Ok(layout
        .ifd1_offset
        .map(|at| read_u16(&file[at..at + 2], scan.byte_order)))
}

/// Remove IFD1 when each physical survivor is one captured mandatory value.
/// The caller owns the source-derived value matcher because WriteExif packs a
/// mandatory value using each surviving record's format and count.
pub(crate) fn remove_ifd1_if_matching(
    file: &[u8],
    mandatory_tag_ids: &[u16],
    source_predicate: bool,
    matches_value: impl Fn(u16, u16, u32, &[u8], ByteOrder) -> Result<bool>,
) -> Result<Vec<u8>> {
    if !source_predicate {
        return Ok(file.to_vec());
    }
    let scan = scan_tiff(file)?;
    let layout = validate_directory_layout(file, &scan)?;
    let Some(ifd1_at) = layout.ifd1_offset else {
        return Ok(file.to_vec());
    };
    let (records, next) = directory_records(file, ifd1_at, scan.byte_order)?;
    if nonzero_offset(&next, scan.byte_order).is_some() || records.len() > mandatory_tag_ids.len() {
        return Ok(file.to_vec());
    }
    for record in &records {
        let tag_id = read_u16(&record[..2], scan.byte_order);
        if mandatory_tag_ids.iter().filter(|id| **id == tag_id).count() != 1 {
            return Ok(file.to_vec());
        }
        let field_type = read_u16(&record[2..4], scan.byte_order);
        let count = read_u32(&record[4..8], scan.byte_order);
        let width = ExifType::from_u16(field_type)
            .ok_or_else(|| invalid("Mandatory IFD1 field type is unsupported"))?
            .size_in_bytes();
        let size = (count as usize)
            .checked_mul(width)
            .ok_or_else(|| invalid("Mandatory IFD1 value length overflows"))?;
        let value = if size <= 4 {
            &record[8..8 + size]
        } else {
            let at = read_u32(&record[8..12], scan.byte_order) as usize;
            file.get(
                at..at
                    .checked_add(size)
                    .ok_or_else(|| invalid("Mandatory IFD1 value length overflows"))?,
            )
            .ok_or_else(|| invalid("Mandatory IFD1 value is outside the TIFF carrier"))?
        };
        if !matches_value(tag_id, field_type, count, value, scan.byte_order)? {
            return Ok(file.to_vec());
        }
    }
    let removals: Vec<_> = mandatory_tag_ids
        .iter()
        .map(|tag_id| ScopedEntryEdit {
            ifd: IfdKind::Ifd1,
            tag_id: *tag_id,
            mutation: EntryMutation::Delete,
        })
        .collect();
    apply_entry_edits(file, &removals)
}

/// Apply the captured WriteExif mandatory-only cleanup rule for IFD1.
///
/// WriteExif removes a directory after a deletion when every remaining entry
/// is an exact mandatory value and it has no next IFD. The caller supplies
/// the immutable, source-derived default encodings; this layer compares only
/// physical TIFF records and has no tag-name or default knowledge of its own.
pub(crate) fn remove_ifd1_if_only_mandatory(
    file: &[u8],
    mandatory: &[ScopedEntryEdit],
    source_predicate: bool,
) -> Result<Vec<u8>> {
    if !source_predicate {
        return Ok(file.to_vec());
    }
    let scan = scan_tiff(file)?;
    let layout = validate_directory_layout(file, &scan)?;
    let Some(ifd1_at) = layout.ifd1_offset else {
        return Ok(file.to_vec());
    };
    let (records, next) = directory_records(file, ifd1_at, scan.byte_order)?;
    if nonzero_offset(&next, scan.byte_order).is_some() || records.len() > mandatory.len() {
        return Ok(file.to_vec());
    }
    for record in &records {
        let tag_id = read_u16(&record[..2], scan.byte_order);
        let matches: Vec<_> = mandatory
            .iter()
            .filter(|expected| expected.tag_id == tag_id)
            .collect();
        if matches.len() != 1 {
            return Ok(file.to_vec());
        }
        let expected = matches[0];
        let EntryMutation::Set {
            field_type,
            count,
            bytes,
        } = &expected.mutation
        else {
            return Err(invalid(
                "Mandatory cleanup requires only encoded set operands",
            ));
        };
        if expected.ifd != IfdKind::Ifd1 {
            return Err(invalid("Mandatory cleanup operands must target IFD1"));
        }
        if read_u16(&record[2..4], scan.byte_order) != *field_type
            || read_u32(&record[4..8], scan.byte_order) != *count
        {
            return Ok(file.to_vec());
        }
        let value = if bytes.len() <= 4 {
            &record[8..8 + bytes.len()]
        } else {
            let at = read_u32(&record[8..12], scan.byte_order) as usize;
            let Some(value) = at
                .checked_add(bytes.len())
                .and_then(|end| file.get(at..end))
            else {
                return Err(invalid("Mandatory IFD1 value is outside the TIFF carrier"));
            };
            value
        };
        if value != bytes {
            return Ok(file.to_vec());
        }
    }
    let removals: Vec<_> = mandatory
        .iter()
        .map(|entry| ScopedEntryEdit {
            ifd: IfdKind::Ifd1,
            tag_id: entry.tag_id,
            mutation: EntryMutation::Delete,
        })
        .collect();
    apply_entry_edits(file, &removals)
}

/// Validate every directory this primitive may rewrite, including children
/// without edits. A present link with offset zero is malformed, not absent.
pub(super) struct DirectoryLayout {
    /// First directory reached by IFD0's next-IFD link. Any later directory
    /// in that chain is preserved by copying the first IFD1 record's link.
    ifd1_offset: Option<usize>,
}

/// Validate every directory link this carrier retains or may repoint. This is
/// intentionally structural: tag identity and serialization belong to the
/// caller's source-derived operation, while malformed aliases or a cycle make
/// every scoped mutation unsafe.
pub(super) fn validate_directory_layout(file: &[u8], scan: &TiffScan) -> Result<DirectoryLayout> {
    let bo = scan.byte_order;
    let mut spans = vec![directory_span(file, scan.ifd0_offset, bo)?];
    let (records, next) = directory_records(file, scan.ifd0_offset, bo)?;
    for pointer_tag in [EXIF_IFD_POINTER, GPS_IFD_POINTER] {
        let mut links = records
            .iter()
            .filter(|record| read_u16(&record[..2], bo) == pointer_tag);
        let Some(record) = links.next() else {
            continue;
        };
        if links.next().is_some()
            || read_u16(&record[2..4], bo) != LONG_TYPE
            || read_u32(&record[4..8], bo) != 1
        {
            return Err(invalid("Ambiguous or malformed subdirectory link"));
        }
        let child_at = read_u32(&record[8..12], bo) as usize;
        add_directory_chain(file, child_at, bo, &mut spans)?;
    }
    let ifd1_offset = nonzero_offset(&next, bo);
    if let Some(ifd1_at) = ifd1_offset {
        add_directory_chain(file, ifd1_at, bo, &mut spans)?;
    }
    Ok(DirectoryLayout { ifd1_offset })
}

fn nonzero_offset(next: &[u8; 4], bo: ByteOrder) -> Option<usize> {
    let offset = read_u32(next, bo) as usize;
    (offset != 0).then_some(offset)
}

/// Add a complete next-IFD chain. A repeated directory is necessarily an
/// alias or cycle, and any partial range is rejected before an output exists.
fn add_directory_chain(
    file: &[u8],
    mut at: usize,
    bo: ByteOrder,
    spans: &mut Vec<std::ops::Range<usize>>,
) -> Result<()> {
    loop {
        let span = directory_span(file, at, bo)?;
        if spans
            .iter()
            .any(|prior| span.start < prior.end && prior.start < span.end)
        {
            return Err(invalid("IFD record spans alias or overlap"));
        }
        spans.push(span);
        let (_, next) = directory_records(file, at, bo)?;
        let Some(next_at) = nonzero_offset(&next, bo) else {
            return Ok(());
        };
        at = next_at;
    }
}

fn directory_span(file: &[u8], at: usize, bo: ByteOrder) -> Result<std::ops::Range<usize>> {
    if at < 8 {
        return Err(invalid("IFD offset points inside the TIFF header"));
    }
    let start = at
        .checked_add(2)
        .filter(|end| *end <= file.len())
        .ok_or_else(|| invalid("IFD count is outside the file"))?;
    let count = read_u16(&file[at..start], bo) as usize;
    let end = count
        .checked_mul(12)
        .and_then(|size| start.checked_add(size))
        .filter(|end| end.checked_add(4).is_some_and(|after| after <= file.len()))
        .ok_or_else(|| invalid("Truncated IFD records or next-directory pointer"))?;
    Ok(at..end + 4)
}

fn directory_records(file: &[u8], at: usize, bo: ByteOrder) -> Result<(Vec<[u8; 12]>, [u8; 4])> {
    let span = directory_span(file, at, bo)?;
    let start = at + 2;
    let end = span.end - 4;
    let records = file[start..end]
        .chunks_exact(12)
        .map(|record| record.try_into().expect("exact record width"))
        .collect();
    let next = file[end..end + 4]
        .try_into()
        .expect("checked pointer width");
    Ok((records, next))
}

/// This classic-TIFF carrier caps the complete output at u32::MAX bytes.
/// Check alignment and the full payload before any append allocation.
fn checked_append_bounds(len: usize, size: usize) -> Result<(usize, usize)> {
    let start = len
        .checked_add(len & 1)
        .ok_or_else(|| invalid("TIFF append alignment overflows"))?;
    let end = start
        .checked_add(size)
        .filter(|end| *end <= u32::MAX as usize)
        .ok_or_else(|| invalid("TIFF output exceeds the 32-bit carrier limit"))?;
    Ok((start, end))
}

fn append_checked(out: &mut Vec<u8>, bytes: &[u8]) -> Result<usize> {
    let (start, _) = checked_append_bounds(out.len(), bytes.len())?;
    out.resize(start, 0);
    out.extend_from_slice(bytes);
    Ok(start)
}

/// Return a new offset only when a directory actually changes. Unchanged and
/// absent deletions preserve the entire original byte sequence.
enum DirectoryRewrite {
    Unchanged,
    Present(usize),
    Removed,
}

fn rewrite_directory(
    out: &mut Vec<u8>,
    at: Option<usize>,
    changes: &[ScopedEntryEdit],
    bo: ByteOrder,
    next_override: Option<u32>,
    remove_if_empty: bool,
) -> Result<DirectoryRewrite> {
    if changes.is_empty() && next_override.is_none() {
        return Ok(DirectoryRewrite::Unchanged);
    }
    let (mut records, original_next) = match at {
        Some(at) => directory_records(out, at, bo)?,
        None => (Vec::new(), [0; 4]),
    };
    let mut next = original_next;
    if let Some(next_at) = next_override {
        put_u32(&mut next, next_at, bo);
    }
    let mut changed = next != original_next;
    for edit in changes {
        let matching: Vec<_> = records
            .iter()
            .enumerate()
            .filter(|(_, record)| read_u16(&record[..2], bo) == edit.tag_id)
            .map(|(index, _)| index)
            .collect();
        if matching.len() > 1 {
            return Err(invalid("Ambiguous duplicate target entry"));
        }
        let existing = matching.first().copied();
        match &edit.mutation {
            EntryMutation::Delete => {
                if let Some(index) = existing {
                    records.remove(index);
                    changed = true;
                }
            }
            EntryMutation::Set {
                field_type,
                count,
                bytes,
            } => {
                if let Some(index) = existing {
                    let old = &records[index];
                    let value_at = if bytes.len() <= 4 {
                        None
                    } else {
                        Some(read_u32(&old[8..12], bo) as usize)
                    };
                    let old_bytes = match value_at {
                        None => Some(&old[8..8 + bytes.len()]),
                        Some(start) => start
                            .checked_add(bytes.len())
                            .and_then(|end| out.get(start..end)),
                    };
                    if read_u16(&old[2..4], bo) == *field_type
                        && read_u32(&old[4..8], bo) == *count
                        && old_bytes == Some(bytes.as_slice())
                    {
                        continue;
                    }
                }
                let mut record = [0; 12];
                put_u16(&mut record[..2], edit.tag_id, bo);
                put_u16(&mut record[2..4], *field_type, bo);
                put_u32(&mut record[4..8], *count, bo);
                if bytes.len() <= 4 {
                    record[8..8 + bytes.len()].copy_from_slice(bytes);
                } else {
                    let offset = u32::try_from(append_checked(out, bytes)?).map_err(too_big)?;
                    put_u32(&mut record[8..12], offset, bo);
                }
                if let Some(index) = existing {
                    records[index] = record;
                } else {
                    records.push(record);
                }
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(DirectoryRewrite::Unchanged);
    }
    if remove_if_empty && records.is_empty() {
        return Ok(DirectoryRewrite::Removed);
    }
    let count =
        u16::try_from(records.len()).map_err(|_| invalid("IFD exceeds its entry-count limit"))?;
    checked_append_bounds(out.len(), 2 + records.len() * 12 + 4)?;
    records.sort_by_key(|record| read_u16(&record[..2], bo));
    let mut table = vec![0; 2];
    put_u16(&mut table, count, bo);
    for record in records {
        table.extend_from_slice(&record);
    }
    table.extend_from_slice(&next);
    let offset = append_checked(out, &table)?;
    Ok(DirectoryRewrite::Present(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// External, independently scored real-carrier replay. The normal test
    /// suite reports this as ignored instead of passing without fixture input.
    #[test]
    #[ignore = "requires OXIDEX_RAW_EDIT_REQUESTS and OXIDEX_RAW_EDIT_RESULTS"]
    fn raw_scoped_native_fixture_driver() {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            input: std::path::PathBuf,
            output: std::path::PathBuf,
            scope: String,
            tag_id: u16,
            op: Option<String>,
            #[serde(rename = "type")]
            field_type: Option<u16>,
            count: Option<u32>,
            value_hex: Option<String>,
        }
        fn apply(request: &Request) -> std::result::Result<(), String> {
            let ifd = match request.scope.as_str() {
                "IFD0" => IfdKind::Ifd0,
                "ExifIFD" => IfdKind::ExifIfd,
                "GPS" => IfdKind::Gps,
                "IFD1" => IfdKind::Ifd1,
                _ => return Err("unsupported scope".into()),
            };
            let mutation = match request.op.as_deref().unwrap_or("set") {
                "delete" => {
                    if request.field_type.is_some()
                        || request.count.is_some()
                        || request.value_hex.is_some()
                    {
                        return Err("delete request must not carry set fields".into());
                    }
                    EntryMutation::Delete
                }
                "set" => {
                    let hex = request.value_hex.as_deref().ok_or("missing value_hex")?;
                    if !hex.is_ascii() || !hex.len().is_multiple_of(2) {
                        return Err("invalid value_hex".into());
                    }
                    let bytes = (0..hex.len())
                        .step_by(2)
                        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16))
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map_err(|err| err.to_string())?;
                    EntryMutation::Set {
                        field_type: request.field_type.ok_or("missing type")?,
                        count: request.count.ok_or("missing count")?,
                        bytes,
                    }
                }
                _ => return Err("unsupported op".into()),
            };
            let input = std::fs::read(&request.input).map_err(|err| err.to_string())?;
            let edits = [ScopedEntryEdit {
                ifd,
                tag_id: request.tag_id,
                mutation,
            }];
            let output = if input.starts_with(&[0xff, 0xd8]) {
                let reader = crate::test_support::TestReader::new(input);
                crate::writers::jpeg_writer::apply_raw_exif_edits(&reader, &edits)
            } else {
                apply_entry_edits(&input, &edits)
            }
            .map_err(|err| err.to_string())?;
            use std::io::Write;
            // Evidence may never overwrite an existing fixture or result.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&request.output)
                .map_err(|err| err.to_string())?;
            file.write_all(&output).map_err(|err| err.to_string())
        }
        let requests =
            std::fs::read_to_string(std::env::var("OXIDEX_RAW_EDIT_REQUESTS").unwrap()).unwrap();
        let mut results = String::new();
        let mut count = 0;
        for line in requests.lines() {
            let request: Request = serde_json::from_str(line).unwrap();
            let result = apply(&request);
            results.push_str(
                &serde_json::json!({
                    "input": request.input, "output": request.output,
                    "ok": result.is_ok(), "error": result.err()
                })
                .to_string(),
            );
            results.push('\n');
            count += 1;
        }
        assert!(count > 0, "empty external replay is not a passing test");
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(std::env::var("OXIDEX_RAW_EDIT_RESULTS").unwrap())
            .unwrap();
        file.write_all(results.as_bytes()).unwrap();
    }

    #[test]
    fn raw_scoped_append_bounds_reject_overflow_without_allocating() {
        let limit = u32::MAX as usize;
        assert_eq!(checked_append_bounds(9, 6).unwrap(), (10, 16));
        assert_eq!(
            checked_append_bounds(limit - 1, 1).unwrap(),
            (limit - 1, limit)
        );
        assert!(checked_append_bounds(limit - 1, 2).is_err());
        assert!(checked_append_bounds(limit, 0).is_err());
        assert!(checked_append_bounds(usize::MAX, 0).is_err());
        assert!(checked_append_bounds(usize::MAX - 1, 2).is_err());
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct PhysicalEntry {
        tag_id: u16,
        field_type: u16,
        count: u32,
        storage: [u8; 4],
    }

    struct Ifd1Fixture {
        file: Vec<u8>,
        root_next_at: usize,
        original_ifd1_at: Option<usize>,
        downstream_at: Option<usize>,
        image: std::ops::Range<usize>,
        thumbnail: std::ops::Range<usize>,
    }

    // These small test-only readers deliberately do not call scan_tiff or the
    // carrier's directory helpers. They inspect the returned TIFF bytes as a
    // consumer of the directory wire format would.
    fn test_u16(bytes: &[u8], bo: ByteOrder) -> u16 {
        match bo {
            ByteOrder::LittleEndian => u16::from_le_bytes(bytes.try_into().unwrap()),
            ByteOrder::BigEndian => u16::from_be_bytes(bytes.try_into().unwrap()),
        }
    }

    fn test_u32(bytes: &[u8], bo: ByteOrder) -> u32 {
        match bo {
            ByteOrder::LittleEndian => u32::from_le_bytes(bytes.try_into().unwrap()),
            ByteOrder::BigEndian => u32::from_be_bytes(bytes.try_into().unwrap()),
        }
    }

    fn push_test_u16(out: &mut Vec<u8>, value: u16, bo: ByteOrder) {
        out.extend_from_slice(&match bo {
            ByteOrder::LittleEndian => value.to_le_bytes(),
            ByteOrder::BigEndian => value.to_be_bytes(),
        });
    }

    fn push_test_u32(out: &mut Vec<u8>, value: u32, bo: ByteOrder) {
        out.extend_from_slice(&match bo {
            ByteOrder::LittleEndian => value.to_le_bytes(),
            ByteOrder::BigEndian => value.to_be_bytes(),
        });
    }

    fn write_test_u32(out: &mut [u8], value: u32, bo: ByteOrder) {
        out.copy_from_slice(&match bo {
            ByteOrder::LittleEndian => value.to_le_bytes(),
            ByteOrder::BigEndian => value.to_be_bytes(),
        });
    }

    fn append_test_payload(out: &mut Vec<u8>, payload: &[u8]) -> usize {
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let at = out.len();
        out.extend_from_slice(payload);
        at
    }

    fn append_test_directory(
        out: &mut Vec<u8>,
        entries: &[PhysicalEntry],
        next: u32,
        bo: ByteOrder,
    ) -> usize {
        if out.len() % 2 == 1 {
            out.push(0);
        }
        let at = out.len();
        push_test_u16(out, entries.len() as u16, bo);
        for entry in entries {
            push_test_u16(out, entry.tag_id, bo);
            push_test_u16(out, entry.field_type, bo);
            push_test_u32(out, entry.count, bo);
            out.extend_from_slice(&entry.storage);
        }
        push_test_u32(out, next, bo);
        at
    }

    fn read_test_directory(file: &[u8], at: usize, bo: ByteOrder) -> (Vec<PhysicalEntry>, usize) {
        let count = test_u16(&file[at..at + 2], bo) as usize;
        let mut entries = Vec::with_capacity(count);
        for index in 0..count {
            let record = at + 2 + index * 12;
            entries.push(PhysicalEntry {
                tag_id: test_u16(&file[record..record + 2], bo),
                field_type: test_u16(&file[record + 2..record + 4], bo),
                count: test_u32(&file[record + 4..record + 8], bo),
                storage: file[record + 8..record + 12].try_into().unwrap(),
            });
        }
        let next_at = at + 2 + count * 12;
        (entries, test_u32(&file[next_at..next_at + 4], bo) as usize)
    }

    fn test_entry_value(file: &[u8], entry: &PhysicalEntry, bo: ByteOrder) -> Vec<u8> {
        let width = match entry.field_type {
            2 => 1,
            3 => 2,
            4 => 4,
            5 => 8,
            _ => panic!("fixture uses only ASCII, SHORT, LONG and RATIONAL"),
        };
        let size = width * entry.count as usize;
        if size <= 4 {
            entry.storage[..size].to_vec()
        } else {
            let at = test_u32(&entry.storage, bo) as usize;
            file[at..at + size].to_vec()
        }
    }

    fn entry(entries: &[PhysicalEntry], tag_id: u16) -> &PhysicalEntry {
        entries
            .iter()
            .find(|entry| entry.tag_id == tag_id)
            .unwrap_or_else(|| panic!("missing fixture tag {tag_id:#06x}"))
    }

    fn raw_ascii(tag_id: u16, bytes: &[u8]) -> ScopedEntryEdit {
        ScopedEntryEdit {
            ifd: IfdKind::Ifd1,
            tag_id,
            mutation: EntryMutation::Set {
                field_type: 2,
                count: bytes.len() as u32,
                bytes: bytes.to_vec(),
            },
        }
    }

    fn ifd1_fixture_entries(fixture: &Ifd1Fixture, bo: ByteOrder) -> Vec<ScopedEntryEdit> {
        let at = fixture.original_ifd1_at.unwrap();
        let (entries, _) = read_test_directory(&fixture.file, at, bo);
        entries
            .iter()
            .map(|entry| ScopedEntryEdit {
                ifd: IfdKind::Ifd1,
                tag_id: entry.tag_id,
                mutation: EntryMutation::Set {
                    field_type: entry.field_type,
                    count: entry.count,
                    bytes: test_entry_value(&fixture.file, entry, bo),
                },
            })
            .collect()
    }

    fn ifd1_fixture(bo: ByteOrder, with_ifd1: bool) -> Ifd1Fixture {
        let mut file = Vec::new();
        file.extend_from_slice(match bo {
            ByteOrder::LittleEndian => b"II",
            ByteOrder::BigEndian => b"MM",
        });
        push_test_u16(&mut file, 42, bo);
        push_test_u32(&mut file, 8, bo);
        push_test_u16(&mut file, 3, bo);
        // Make ASCII@62, StripOffsets LONG=80, Orientation SHORT inline.
        for entry in [
            PhysicalEntry {
                tag_id: 0x010f,
                field_type: 2,
                count: 6,
                storage: match bo {
                    ByteOrder::LittleEndian => 62u32.to_le_bytes(),
                    ByteOrder::BigEndian => 62u32.to_be_bytes(),
                },
            },
            PhysicalEntry {
                tag_id: 0x0111,
                field_type: 4,
                count: 1,
                storage: match bo {
                    ByteOrder::LittleEndian => 80u32.to_le_bytes(),
                    ByteOrder::BigEndian => 80u32.to_be_bytes(),
                },
            },
            PhysicalEntry {
                tag_id: 0x0112,
                field_type: 3,
                count: 1,
                storage: match bo {
                    ByteOrder::LittleEndian => 1u32.to_le_bytes(),
                    ByteOrder::BigEndian => 1u32.to_be_bytes(),
                },
            },
        ] {
            push_test_u16(&mut file, entry.tag_id, bo);
            push_test_u16(&mut file, entry.field_type, bo);
            push_test_u32(&mut file, entry.count, bo);
            file.extend_from_slice(&entry.storage);
        }
        let root_next_at = file.len();
        push_test_u32(&mut file, 0, bo);
        file.resize(62, 0);
        file.extend_from_slice(b"Canon\0");
        file.resize(80, 0);
        let image = file.len()..file.len() + 8;
        file.extend_from_slice(&[0xa5; 8]);
        let thumbnail = file.len()..file.len() + 6;
        file.extend_from_slice(&[0x5a; 6]);
        if !with_ifd1 {
            return Ifd1Fixture {
                file,
                root_next_at,
                original_ifd1_at: None,
                downstream_at: None,
                image,
                thumbnail,
            };
        }

        let artist_at = append_test_payload(&mut file, b"old!!\0");
        let description_at = append_test_payload(&mut file, b"desc!\0");
        let downstream_at = append_test_directory(
            &mut file,
            &[PhysicalEntry {
                tag_id: 0x0201,
                field_type: 4,
                count: 1,
                storage: match bo {
                    ByteOrder::LittleEndian => 80u32.to_le_bytes(),
                    ByteOrder::BigEndian => 80u32.to_be_bytes(),
                },
            }],
            0,
            bo,
        );
        let original_ifd1_at = append_test_directory(
            &mut file,
            &[
                PhysicalEntry {
                    tag_id: 0x010e,
                    field_type: 2,
                    count: 6,
                    storage: match bo {
                        ByteOrder::LittleEndian => (description_at as u32).to_le_bytes(),
                        ByteOrder::BigEndian => (description_at as u32).to_be_bytes(),
                    },
                },
                PhysicalEntry {
                    tag_id: 0x013b,
                    field_type: 2,
                    count: 6,
                    storage: match bo {
                        ByteOrder::LittleEndian => (artist_at as u32).to_le_bytes(),
                        ByteOrder::BigEndian => (artist_at as u32).to_be_bytes(),
                    },
                },
                PhysicalEntry {
                    tag_id: 0x0201,
                    field_type: 4,
                    count: 1,
                    storage: match bo {
                        ByteOrder::LittleEndian => (thumbnail.start as u32).to_le_bytes(),
                        ByteOrder::BigEndian => (thumbnail.start as u32).to_be_bytes(),
                    },
                },
                PhysicalEntry {
                    tag_id: 0x0202,
                    field_type: 4,
                    count: 1,
                    storage: match bo {
                        ByteOrder::LittleEndian => (thumbnail.len() as u32).to_le_bytes(),
                        ByteOrder::BigEndian => (thumbnail.len() as u32).to_be_bytes(),
                    },
                },
            ],
            downstream_at as u32,
            bo,
        );
        write_test_u32(
            &mut file[root_next_at..root_next_at + 4],
            original_ifd1_at as u32,
            bo,
        );
        Ifd1Fixture {
            file,
            root_next_at,
            original_ifd1_at: Some(original_ifd1_at),
            downstream_at: Some(downstream_at),
            image,
            thumbnail,
        }
    }

    #[test]
    fn raw_scoped_ifd1_creation_repoints_a_copied_root_and_preserves_image_bytes() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let fixture = ifd1_fixture(bo, false);
            let (original_root, original_next) = read_test_directory(&fixture.file, 8, bo);
            assert_eq!(original_next, 0);
            let out = apply_entry_edits(&fixture.file, &[raw_ascii(0x013b, b"created\0")]).unwrap();
            let root_at = test_u32(&out[4..8], bo) as usize;
            assert!(root_at >= fixture.file.len());
            let (root, ifd1_at) = read_test_directory(&out, root_at, bo);
            assert_eq!(root, original_root);
            assert!(ifd1_at >= fixture.file.len());
            let (ifd1, next) = read_test_directory(&out, ifd1_at, bo);
            assert_eq!(next, 0);
            assert_eq!(ifd1.len(), 1);
            assert_eq!(
                test_entry_value(&out, entry(&ifd1, 0x013b), bo),
                b"created\0"
            );
            assert_eq!(&out[8..fixture.file.len()], &fixture.file[8..]);
            assert_eq!(&out[fixture.image], &[0xa5; 8]);
            assert_eq!(&out[fixture.thumbnail], &[0x5a; 6]);
        }
    }

    #[test]
    fn raw_scoped_ifd1_delete_of_every_entry_omits_the_directory() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let fixture = ifd1_fixture(bo, true);
            let (original_root, _) = read_test_directory(&fixture.file, 8, bo);
            let out = apply_entry_edits(
                &fixture.file,
                &[0x010e, 0x013b, 0x0201, 0x0202].map(|tag_id| ScopedEntryEdit {
                    ifd: IfdKind::Ifd1,
                    tag_id,
                    mutation: EntryMutation::Delete,
                }),
            )
            .unwrap();
            let root_at = test_u32(&out[4..8], bo) as usize;
            let (root, next) = read_test_directory(&out, root_at, bo);
            assert!(root_at >= fixture.file.len());
            assert_eq!(root, original_root);
            assert_eq!(next, 0, "an empty IFD1 must be omitted");
            assert_eq!(ifd1_entry_count(&out).unwrap(), None);
            // The old thumbnail directory becomes unreachable but existing
            // carrier bytes, including the image and thumbnail payloads, are
            // never rewritten by a directory-only removal.
            assert_eq!(&out[8..fixture.file.len()], &fixture.file[8..]);
            assert_eq!(&out[fixture.image], &[0xa5; 8]);
            assert_eq!(&out[fixture.thumbnail], &[0x5a; 6]);
        }
    }

    #[test]
    fn raw_scoped_ifd1_mandatory_cleanup_honors_source_predicates() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let fixture = ifd1_fixture(bo, true);
            let mandatory = ifd1_fixture_entries(&fixture, bo);
            let ifd1_at = fixture.original_ifd1_at.unwrap();
            let (entries, _) = read_test_directory(&fixture.file, ifd1_at, bo);
            let ifd1_next_at = ifd1_at + 2 + entries.len() * 12;
            let mut no_next = fixture.file.clone();
            write_test_u32(&mut no_next[ifd1_next_at..ifd1_next_at + 4], 0, bo);

            // Existing mandatory-only data has not shrunk and must survive.
            assert_eq!(
                remove_ifd1_if_only_mandatory(&no_next, &mandatory, false).unwrap(),
                no_next
            );
            // A nonmandatory survivor defeats the allMandatory predicate.
            let survivor =
                apply_entry_edits(&no_next, &[raw_ascii(0x013c, b"survivor\0")]).unwrap();
            assert_eq!(
                remove_ifd1_if_only_mandatory(&survivor, &mandatory, true).unwrap(),
                survivor
            );
            // A changed directory that now contains only source-provided
            // mandatory values is removed when it has no following IFD.
            let shrunk = apply_entry_edits(
                &no_next,
                &[ScopedEntryEdit {
                    ifd: IfdKind::Ifd1,
                    tag_id: 0x010e,
                    mutation: EntryMutation::Delete,
                }],
            )
            .unwrap();
            let removed = remove_ifd1_if_only_mandatory(&shrunk, &mandatory, true).unwrap();
            assert_eq!(ifd1_entry_count(&removed).unwrap(), None);
            // A following IFD keeps the mandatory-only directory reachable.
            assert_eq!(
                remove_ifd1_if_only_mandatory(&fixture.file, &mandatory, true).unwrap(),
                fixture.file
            );
        }
    }

    #[test]
    fn raw_scoped_ifd1_cleanup_matches_captured_writeexif_defaults_after_delete() {
        use crate::writers::{
            generated_mandatory_defaults::MANDATORY_DEFAULTS,
            mandatory_defaults_runtime as mandatory,
        };

        mandatory::require_ifd1_mandatory_cleanup(&MANDATORY_DEFAULTS).unwrap();
        let defaults = MANDATORY_DEFAULTS
            .directories
            .iter()
            .find(|directory| directory.directory == "IFD1")
            .expect("tier-1 capture includes WriteExif IFD1 defaults");
        for (bo, order) in [
            (ByteOrder::LittleEndian, mandatory::TiffByteOrder::Little),
            (ByteOrder::BigEndian, mandatory::TiffByteOrder::Big),
        ] {
            let fixture = ifd1_fixture(bo, true);
            let ifd1_at = fixture.original_ifd1_at.unwrap();
            let (original_entries, _) = read_test_directory(&fixture.file, ifd1_at, bo);
            let mut no_next = fixture.file.clone();
            let next_at = ifd1_at + 2 + original_entries.len() * 12;
            write_test_u32(&mut no_next[next_at..next_at + 4], 0, bo);

            let mandatory =
                mandatory::encode_mandatory_defaults(&MANDATORY_DEFAULTS, defaults.defaults, order)
                    .unwrap()
                    .into_iter()
                    .map(|entry| ScopedEntryEdit {
                        ifd: IfdKind::Ifd1,
                        tag_id: entry.tag_id,
                        mutation: EntryMutation::Set {
                            field_type: entry.tiff_type,
                            count: entry.count,
                            bytes: entry.bytes,
                        },
                    })
                    .collect::<Vec<_>>();

            // Recreate an IFD1 containing the exact capture defaults plus a
            // normal public tag. Its selected deletion is the source case.
            let remove_originals = original_entries
                .iter()
                .map(|entry| ScopedEntryEdit {
                    ifd: IfdKind::Ifd1,
                    tag_id: entry.tag_id,
                    mutation: EntryMutation::Delete,
                })
                .collect::<Vec<_>>();
            // `apply_entry_edits` intentionally rejects two mutations to one
            // physical entry in one transaction, so clear the seed IFD before
            // creating the source-default carrier.
            let empty_ifd1 = apply_entry_edits(&no_next, &remove_originals).unwrap();
            let mut setup = mandatory.clone();
            setup.push(raw_ascii(0x013b, b"Artist\0"));
            let before_delete = apply_entry_edits(&empty_ifd1, &setup).unwrap();
            assert_eq!(ifd1_entry_count(&before_delete).unwrap(), Some(5));

            let shrunk = apply_entry_edits(
                &before_delete,
                &[ScopedEntryEdit {
                    ifd: IfdKind::Ifd1,
                    tag_id: 0x013b,
                    mutation: EntryMutation::Delete,
                }],
            )
            .unwrap();
            assert_eq!(ifd1_entry_count(&shrunk).unwrap(), Some(4));
            assert_eq!(
                remove_ifd1_if_only_mandatory(&shrunk, &mandatory, false).unwrap(),
                shrunk,
                "without the selected-delete condition, pre-existing defaults survive"
            );
            let removed = remove_ifd1_if_only_mandatory(&shrunk, &mandatory, true).unwrap();
            assert_eq!(ifd1_entry_count(&removed).unwrap(), None);
        }
    }

    #[test]
    fn raw_scoped_ifd1_count_two_survivor_keeps_directory_after_selected_delete() {
        use crate::writers::{
            generated_mandatory_defaults::MANDATORY_DEFAULTS,
            mandatory_defaults_runtime as mandatory,
        };

        let defaults = MANDATORY_DEFAULTS
            .directories
            .iter()
            .find(|directory| directory.directory == "IFD1")
            .expect("tier-1 capture includes WriteExif IFD1 defaults");
        // Choose the captured source value rather than a tag-specific fixture.
        let selected = defaults
            .defaults
            .iter()
            .find(|default| matches!(default.value, mandatory::MandatoryValue::Integer(_)))
            .expect("captured IFD1 defaults include an integer WriteValue operand");
        let mandatory_tag_ids = defaults
            .defaults
            .iter()
            .map(|default| default.tag_id)
            .collect::<Vec<_>>();

        for (bo, order) in [
            (ByteOrder::LittleEndian, mandatory::TiffByteOrder::Little),
            (ByteOrder::BigEndian, mandatory::TiffByteOrder::Big),
        ] {
            let fixture = ifd1_fixture(bo, true);
            let ifd1_at = fixture.original_ifd1_at.unwrap();
            let (original_entries, _) = read_test_directory(&fixture.file, ifd1_at, bo);
            let mut no_next = fixture.file.clone();
            let next_at = ifd1_at + 2 + original_entries.len() * 12;
            write_test_u32(&mut no_next[next_at..next_at + 4], 0, bo);
            let empty_ifd1 = apply_entry_edits(
                &no_next,
                &original_entries
                    .iter()
                    .map(|entry| ScopedEntryEdit {
                        ifd: IfdKind::Ifd1,
                        tag_id: entry.tag_id,
                        mutation: EntryMutation::Delete,
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            let mut setup =
                mandatory::encode_mandatory_defaults(&MANDATORY_DEFAULTS, defaults.defaults, order)
                    .unwrap()
                    .into_iter()
                    .map(|entry| ScopedEntryEdit {
                        ifd: IfdKind::Ifd1,
                        tag_id: entry.tag_id,
                        mutation: EntryMutation::Set {
                            field_type: entry.tiff_type,
                            count: entry.count,
                            bytes: entry.bytes,
                        },
                    })
                    .collect::<Vec<_>>();
            setup.push(raw_ascii(0x013b, b"Artist\0"));
            let with_defaults = apply_entry_edits(&empty_ifd1, &setup).unwrap();

            // WriteValue(6, int32u, 2) is undef: native does delete Artist,
            // but does not classify this survivor as mandatory.  The cleanup
            // must therefore retain IFD1 instead of rejecting the transaction.
            let mandatory_value = match selected.value {
                mandatory::MandatoryValue::Integer(value) => u32::try_from(value).unwrap(),
                mandatory::MandatoryValue::Text(_) => unreachable!(),
            };
            let twice = match bo {
                ByteOrder::LittleEndian => {
                    [mandatory_value.to_le_bytes(), mandatory_value.to_le_bytes()].concat()
                }
                ByteOrder::BigEndian => {
                    [mandatory_value.to_be_bytes(), mandatory_value.to_be_bytes()].concat()
                }
            };
            let with_count_two = apply_entry_edits(
                &with_defaults,
                &[ScopedEntryEdit {
                    ifd: IfdKind::Ifd1,
                    tag_id: selected.tag_id,
                    mutation: EntryMutation::Set {
                        field_type: 4,
                        count: 2,
                        bytes: twice,
                    },
                }],
            )
            .unwrap();
            let shrunk = apply_entry_edits(
                &with_count_two,
                &[ScopedEntryEdit {
                    ifd: IfdKind::Ifd1,
                    tag_id: 0x013b,
                    mutation: EntryMutation::Delete,
                }],
            )
            .unwrap();
            let kept = remove_ifd1_if_matching(
                &shrunk,
                &mandatory_tag_ids,
                true,
                |tag_id, field_type, count, value, actual_order| {
                    let default = defaults
                        .defaults
                        .iter()
                        .find(|default| default.tag_id == tag_id)
                        .expect("captured id remains bound");
                    let actual = match actual_order {
                        ByteOrder::LittleEndian => mandatory::TiffByteOrder::Little,
                        ByteOrder::BigEndian => mandatory::TiffByteOrder::Big,
                    };
                    mandatory::matches_existing_mandatory_value(
                        &MANDATORY_DEFAULTS,
                        *default,
                        field_type,
                        count,
                        value,
                        actual,
                    )
                    .map_err(|reason| invalid(&reason))
                },
            )
            .unwrap();
            assert_eq!(kept, shrunk, "count-two survivor is a native non-match");
            assert_eq!(ifd1_entry_count(&kept).unwrap(), Some(4));
        }
    }

    #[test]
    fn raw_scoped_ifd1_update_delete_and_add_keep_downstream_chain_byte_identical() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let fixture = ifd1_fixture(bo, true);
            let downstream_at = fixture.downstream_at.unwrap();
            let rational = match bo {
                ByteOrder::LittleEndian => [72u32.to_le_bytes(), 1u32.to_le_bytes()].concat(),
                ByteOrder::BigEndian => [72u32.to_be_bytes(), 1u32.to_be_bytes()].concat(),
            };
            let out = apply_entry_edits(
                &fixture.file,
                &[
                    raw_ascii(0x013b, b"replacement\0"),
                    ScopedEntryEdit {
                        ifd: IfdKind::Ifd1,
                        tag_id: 0x010e,
                        mutation: EntryMutation::Delete,
                    },
                    ScopedEntryEdit {
                        ifd: IfdKind::Ifd1,
                        tag_id: 0x011a,
                        mutation: EntryMutation::Set {
                            field_type: 5,
                            count: 1,
                            bytes: rational.clone(),
                        },
                    },
                ],
            )
            .unwrap();
            let root_at = test_u32(&out[4..8], bo) as usize;
            let (_, new_ifd1_at) = read_test_directory(&out, root_at, bo);
            assert!(root_at >= fixture.file.len());
            assert!(new_ifd1_at >= fixture.file.len());
            let (ifd1, next) = read_test_directory(&out, new_ifd1_at, bo);
            assert_eq!(next, downstream_at);
            assert!(!ifd1.iter().any(|entry| entry.tag_id == 0x010e));
            assert_eq!(
                test_entry_value(&out, entry(&ifd1, 0x013b), bo),
                b"replacement\0"
            );
            assert_eq!(test_entry_value(&out, entry(&ifd1, 0x011a), bo), rational);
            let (downstream, downstream_next) = read_test_directory(&out, downstream_at, bo);
            assert_eq!(downstream_next, 0);
            assert_eq!(downstream.len(), 1);
            assert_eq!(downstream[0].tag_id, 0x0201);
            assert_eq!(&out[8..fixture.file.len()], &fixture.file[8..]);
            assert_eq!(&out[fixture.image], &[0xa5; 8]);
            assert_eq!(&out[fixture.thumbnail], &[0x5a; 6]);
        }
    }

    #[test]
    fn raw_scoped_ifd1_rejects_malformed_overlapping_and_cyclic_links_atomically() {
        for bo in [ByteOrder::LittleEndian, ByteOrder::BigEndian] {
            let fixture = ifd1_fixture(bo, true);
            let ifd1_at = fixture.original_ifd1_at.unwrap();
            let (original_ifd1_entries, _) = read_test_directory(&fixture.file, ifd1_at, bo);
            let ifd1_next_at = ifd1_at + 2 + original_ifd1_entries.len() * 12;
            let mut cases = Vec::new();
            for (label, offset) in [
                ("root next inside header", 4),
                ("root next aliases root", 8),
                ("root next overlaps an IFD", ifd1_at + 2),
                ("root next is truncated", fixture.file.len() - 2),
            ] {
                let mut file = fixture.file.clone();
                write_test_u32(
                    &mut file[fixture.root_next_at..fixture.root_next_at + 4],
                    offset as u32,
                    bo,
                );
                cases.push((label, file));
            }
            for (label, offset) in [
                ("IFD1 next is a cycle", ifd1_at),
                ("IFD1 next aliases root", 8),
                ("IFD1 next overlaps itself", ifd1_at + 2),
            ] {
                let mut file = fixture.file.clone();
                write_test_u32(&mut file[ifd1_next_at..ifd1_next_at + 4], offset as u32, bo);
                cases.push((label, file));
            }
            for (label, file) in cases {
                let before = file.clone();
                assert!(
                    apply_entry_edits(&file, &[raw_ascii(0x013b, b"safe\0")]).is_err(),
                    "must reject {label} in {bo:?}"
                );
                assert_eq!(file, before, "failed {label} must leave input untouched");
            }
        }
    }
}
