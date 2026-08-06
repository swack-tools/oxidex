//! Narrow DjVu annotation extraction.
//!
//! DjVu files use an IFF container and store annotations in `ANTa` (plain) or
//! `ANTz` (BZZ-compressed) chunks. This module intentionally emits only the
//! `annote` metadata item assigned to `DjVu:Annotation`.

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

pub fn extract_annotation(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
    let data = reader.read(0, reader.size() as usize)?;
    if !data.starts_with(b"AT&TFORM") {
        return Ok(());
    }
    walk_chunks(&data[4..], metadata)
}

fn walk_chunks(data: &[u8], metadata: &mut MetadataMap) -> Result<()> {
    let mut offset = 0usize;
    while offset + 8 <= data.len() {
        let id = &data[offset..offset + 4];
        let len = u32::from_be_bytes(data[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let payload_start = offset + 8;
        let payload_end = payload_start
            .checked_add(len)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| ExifToolError::parse_error("truncated DjVu IFF chunk"))?;
        let payload = &data[payload_start..payload_end];

        if id == b"FORM" {
            if payload.len() >= 4 {
                walk_chunks(&payload[4..], metadata)?;
            }
        } else if id == b"ANTa" {
            insert_annotation(payload, metadata);
        } else if id == b"ANTz" {
            let decoded = djvu_bzz::decode(payload)
                .map_err(|e| ExifToolError::parse_error(format!("invalid DjVu ANTz chunk: {e}")))?;
            insert_annotation(&decoded, metadata);
        }

        offset = payload_end + (len & 1);
    }
    Ok(())
}

fn insert_annotation(annotation: &[u8], metadata: &mut MetadataMap) {
    if let Some(value) = find_annote(annotation) {
        metadata.insert("DjVu:Annotation", TagValue::new_string(value));
    }
    if let Some(value) = find_metadata_value(annotation, b"Author") {
        metadata.insert("DjVu-Meta:Author", TagValue::new_string(value));
    }
}

fn find_metadata_value(data: &[u8], wanted: &[u8]) -> Option<String> {
    let mut pos = 0usize;
    while pos < data.len() {
        let open = data[pos..].iter().position(|&b| b == b'(')? + pos;
        let mut cursor = open + 1;
        skip_space(data, &mut cursor);
        if !data.get(cursor..)?.starts_with(b"metadata") {
            pos = open + 1;
            continue;
        }
        cursor += b"metadata".len();
        if data
            .get(cursor)
            .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'(')
        {
            pos = open + 1;
            continue;
        }

        loop {
            skip_space(data, &mut cursor);
            match data.get(cursor) {
                Some(b')') | None => return None,
                Some(b'(') => cursor += 1,
                Some(_) => return None,
            }
            skip_space(data, &mut cursor);
            let key_end = data[cursor..]
                .iter()
                .position(|b| b.is_ascii_whitespace() || *b == b')')?
                + cursor;
            let key = &data[cursor..key_end];
            cursor = key_end;
            skip_space(data, &mut cursor);
            if key == wanted {
                return parse_value(data, cursor);
            }
            cursor = skip_list(data, cursor)?;
        }
    }
    None
}

fn skip_list(data: &[u8], mut cursor: usize) -> Option<usize> {
    let mut quoted = false;
    let mut escaped = false;
    let mut depth = 1usize;
    while let Some(&byte) = data.get(cursor) {
        cursor += 1;
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_annote(data: &[u8]) -> Option<String> {
    let mut pos = 0usize;
    while pos < data.len() {
        let open = data[pos..].iter().position(|&b| b == b'(')? + pos;
        let mut cursor = open + 1;
        skip_space(data, &mut cursor);
        if !data.get(cursor..)?.starts_with(b"annote") {
            pos = open + 1;
            continue;
        }
        cursor += b"annote".len();
        if data.get(cursor).is_some_and(|b| !b.is_ascii_whitespace()) {
            pos = open + 1;
            continue;
        }
        skip_space(data, &mut cursor);
        return parse_value(data, cursor);
    }
    None
}

fn skip_space(data: &[u8], cursor: &mut usize) {
    while data.get(*cursor).is_some_and(u8::is_ascii_whitespace) {
        *cursor += 1;
    }
}

fn parse_value(data: &[u8], mut cursor: usize) -> Option<String> {
    if data.get(cursor) != Some(&b'"') {
        let end = data[cursor..]
            .iter()
            .position(|b| b.is_ascii_whitespace() || *b == b')')?
            + cursor;
        return String::from_utf8(data[cursor..end].to_vec()).ok();
    }

    cursor += 1;
    let mut value = Vec::new();
    while let Some(&byte) = data.get(cursor) {
        cursor += 1;
        match byte {
            b'"' => return String::from_utf8(value).ok(),
            b'\\' => {
                let escaped = *data.get(cursor)?;
                cursor += 1;
                match escaped {
                    b'a' => value.push(0x07),
                    b'b' => value.push(0x08),
                    b'f' => value.push(0x0c),
                    b'n' => value.push(b'\n'),
                    b'r' => value.push(b'\r'),
                    b't' => value.push(b'\t'),
                    b'"' => value.push(b'"'),
                    b'\\' => value.push(b'\\'),
                    other => value.extend_from_slice(&[b'\\', other]),
                }
            }
            other => value.push(other),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_annotation;
    use crate::core::{FileReader, MetadataMap};
    use std::io;

    struct SliceReader(Vec<u8>);

    impl FileReader for SliceReader {
        fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
            let start = offset as usize;
            Ok(&self.0[start..start + length])
        }

        fn size(&self) -> u64 {
            self.0.len() as u64
        }
    }

    #[test]
    fn extracts_author_from_annotation_metadata() {
        let annotation = br#"(metadata (Author "Phil Harvey") (Title "Sample"))"#;
        let form_len = 4 + 8 + annotation.len();
        let mut data = b"AT&TFORM".to_vec();
        data.extend_from_slice(&(form_len as u32).to_be_bytes());
        data.extend_from_slice(b"DJVUANTa");
        data.extend_from_slice(&(annotation.len() as u32).to_be_bytes());
        data.extend_from_slice(annotation);

        let mut metadata = MetadataMap::new();
        extract_annotation(&SliceReader(data), &mut metadata).unwrap();

        assert_eq!(metadata.get_string("DjVu-Meta:Author"), Some("Phil Harvey"));
    }
}
