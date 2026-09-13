//! Scoped raw IFD edits for a generated writer's resolved operations.
//!
//! The caller must supply source-authorized tag identity, type and already
//! encoded bytes in the file's byte order. This layer never consults tag names
//! or the tag registry. It copies changed directories to the end of the file,
//! retaining every untouched record, value offset and next-directory pointer.
//! Production writers do not call this primitive yet.

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
    validate_directory_layout(file, &scan)?;
    checked_append_bounds(file.len(), 0)?;
    for (index, edit) in edits.iter().enumerate() {
        if !matches!(edit.ifd, IfdKind::Ifd0 | IfdKind::ExifIfd | IfdKind::Gps) {
            return Err(invalid("Raw edits require IFD0, ExifIFD or GPS"));
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
        if let Some(new_at) = rewrite_directory(&mut out, offset, &changes, bo)? {
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
    if let Some(new_at) = rewrite_directory(&mut out, Some(scan.ifd0_offset), &root_edits, bo)? {
        let new_at = u32::try_from(new_at).map_err(too_big)?;
        put_u32(&mut out[4..8], new_at, bo);
    }
    Ok(out)
}

/// Validate every directory this primitive may rewrite, including children
/// without edits. A present link with offset zero is malformed, not absent.
fn validate_directory_layout(file: &[u8], scan: &TiffScan) -> Result<()> {
    let bo = scan.byte_order;
    let mut spans = vec![directory_span(file, scan.ifd0_offset, bo)?];
    let (records, _) = directory_records(file, scan.ifd0_offset, bo)?;
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
        let span = directory_span(file, read_u32(&record[8..12], bo) as usize, bo)?;
        if spans
            .iter()
            .any(|prior| span.start < prior.end && prior.start < span.end)
        {
            return Err(invalid("IFD record spans alias or overlap"));
        }
        spans.push(span);
    }
    Ok(())
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
fn rewrite_directory(
    out: &mut Vec<u8>,
    at: Option<usize>,
    changes: &[ScopedEntryEdit],
    bo: ByteOrder,
) -> Result<Option<usize>> {
    if changes.is_empty() {
        return Ok(None);
    }
    let (mut records, next) = match at {
        Some(at) => directory_records(out, at, bo)?,
        None => (Vec::new(), [0; 4]),
    };
    let mut changed = false;
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
        return Ok(None);
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
    Ok(Some(offset))
}

#[cfg(test)]
mod tests {
    use super::checked_append_bounds;

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
}
