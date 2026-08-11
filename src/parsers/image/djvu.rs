//! DjVu annotation-metadata extraction.
//!
//! ExifTool 13.59 reads DjVu as an IFF container (`AIFF.pm:184-288`), then
//! decodes `ANTz` with the DjVu BZZ codec and processes its s-expressions
//! (`DjVu.pm:179-300`).  This module deliberately implements only the
//! `metadata` path, including `annote` and standard metadata fields.

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::parsers::xmp::rdf_parser::{XmpValue, parse_xmp_typed};
use djvu_bzz::bzz_decode;
use djvu_iff::{Chunk, parse};

#[derive(Debug)]
enum Expression {
    Atom(String),
    String(String),
    List(Vec<Expression>),
}

struct ExpressionParser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ExpressionParser<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            position: 0,
        }
    }

    fn expressions(&mut self) -> Option<Vec<Expression>> {
        let mut expressions = Vec::new();
        while self.skip_whitespace().is_some() {
            expressions.push(self.expression()?);
        }
        Some(expressions)
    }

    fn expression(&mut self) -> Option<Expression> {
        match self.skip_whitespace()? {
            b'(' => self.list(),
            b'"' => self.string().map(Expression::String),
            b')' => None,
            _ => self.atom().map(Expression::Atom),
        }
    }

    fn list(&mut self) -> Option<Expression> {
        self.position += 1;
        let mut expressions = Vec::new();
        loop {
            match self.skip_whitespace()? {
                b')' => {
                    self.position += 1;
                    return Some(Expression::List(expressions));
                }
                _ => expressions.push(self.expression()?),
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        self.position += 1;
        let mut string = String::new();
        loop {
            let byte = *self.bytes.get(self.position)?;
            self.position += 1;
            match byte {
                b'"' => return Some(string),
                b'\\' => {
                    let escaped = *self.bytes.get(self.position)?;
                    self.position += 1;
                    match escaped {
                        b'a' => string.push('\u{7}'),
                        b'b' => string.push('\u{8}'),
                        b'f' => string.push('\u{c}'),
                        b'n' => string.push('\n'),
                        b'r' => string.push('\r'),
                        b't' => string.push('\t'),
                        b'"' => string.push('"'),
                        b'\\' => string.push('\\'),
                        other => {
                            string.push('\\');
                            string.push(char::from(other));
                        }
                    }
                }
                byte if byte.is_ascii() => string.push(char::from(byte)),
                _ => return None,
            }
        }
    }

    fn atom(&mut self) -> Option<String> {
        let start = self.position;
        while let Some(byte) = self.bytes.get(self.position) {
            if byte.is_ascii_whitespace() || matches!(byte, b'(' | b')' | b'"') {
                break;
            }
            self.position += 1;
        }
        (self.position > start)
            .then(|| std::str::from_utf8(&self.bytes[start..self.position]).ok())
            .flatten()
            .map(ToOwned::to_owned)
    }

    fn skip_whitespace(&mut self) -> Option<u8> {
        while self
            .bytes
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
        self.bytes.get(self.position).copied()
    }
}

fn metadata_from_expression(expression: Expression, metadata: &mut MetadataMap) {
    let Expression::List(items) = expression else {
        return;
    };
    let Some(Expression::Atom(tag)) = items.first() else {
        return;
    };
    if tag != "metadata" {
        return;
    }

    for item in items.into_iter().skip(1) {
        let Expression::List(pair) = item else {
            continue;
        };
        let Some(Expression::Atom(name)) = pair.first() else {
            continue;
        };
        let value = match pair.get(1) {
            Some(Expression::Atom(value) | Expression::String(value)) => value,
            _ => continue,
        };
        match name.as_str() {
            "annote" => metadata.insert(
                "DjVu:Annotation".to_string(),
                TagValue::new_string(value.clone()),
            ),
            "Author" => {
                metadata.insert(
                    "DjVu-Meta:Author".to_string(),
                    TagValue::new_string(value.clone()),
                );
                metadata.insert(
                    "DjVu:Author".to_string(),
                    TagValue::new_string(value.clone()),
                )
            }
            "Title" => {
                metadata.insert(
                    "DjVu-Meta:Title".to_string(),
                    TagValue::new_string(value.clone()),
                );
                metadata.insert(
                    "DjVu:Title".to_string(),
                    TagValue::new_string(value.clone()),
                )
            }
            "url" => metadata.insert("DjVu:URL".to_string(), TagValue::new_string(value.clone())),
            "CreationDate" => djvu_date(value).and_then(|value| {
                metadata.insert("DjVu:CreateDate".to_string(), TagValue::new_string(value))
            }),
            "ModDate" => djvu_date(value).and_then(|value| {
                metadata.insert("DjVu:ModifyDate".to_string(), TagValue::new_string(value))
            }),
            "Trapped" => metadata.insert(
                "DjVu:Trapped".to_string(),
                TagValue::new_string(value.trim_start_matches('/').to_string()),
            ),
            "note" => metadata.insert(
                "DjVu:Note".to_string(),
                TagValue::new_string(value.clone()),
            ),
            "Subject" | "Keywords" | "Creator" | "Producer" => {
                metadata.insert(format!("DjVu:{name}"), TagValue::new_string(value.clone()))
            }
            _ => None,
        };
    }
}

/// Convert the RFC 3339-like PDF DocInfo date form accepted by ExifTool's
/// `ConvertXMPDate`.  Decline incomplete or non-conforming values rather than
/// inventing a timestamp.
fn djvu_date(value: &str) -> Option<String> {
    if value.len() == 25
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
        && value.as_bytes().get(10) == Some(&b'T')
        && value.as_bytes().get(13) == Some(&b':')
        && value.as_bytes().get(16) == Some(&b':')
        && matches!(value.as_bytes().get(19), Some(b'+' | b'-'))
        && value.as_bytes().get(22) == Some(&b':')
        && value
            .bytes()
            .enumerate()
            .filter(|(index, _)| !matches!(index, 4 | 7 | 10 | 13 | 16 | 19 | 22))
            .all(|(_, byte)| byte.is_ascii_digit())
    {
        return Some(format!(
            "{}:{}:{} {}:{}:{}{}{}:{}",
            &value[..4],
            &value[5..7],
            &value[8..10],
            &value[11..13],
            &value[14..16],
            &value[17..19],
            &value[19..20],
            &value[20..22],
            &value[23..25]
        ));
    }
    let value = value.strip_prefix("D:")?;
    let bytes = value.as_bytes();
    if bytes.len() != 21
        || !bytes[..14].iter().all(u8::is_ascii_digit)
        || !matches!(bytes[14], b'+' | b'-')
        || !bytes[15..17].iter().all(u8::is_ascii_digit)
        || bytes[17] != b'\''
        || !bytes[18..20].iter().all(u8::is_ascii_digit)
        || bytes[20] != b'\''
    {
        return None;
    }
    let timezone_minutes = value.get(18..20)?;
    Some(format!(
        "{}:{}:{} {}:{}:{}{}{}:{}",
        &value[..4],
        &value[4..6],
        &value[6..8],
        &value[8..10],
        &value[10..12],
        &value[12..14],
        &value[14..15],
        &value[15..17],
        timezone_minutes
    ))
}

fn collect_info(data: &[u8], metadata: &mut MetadataMap) {
    let Some(width) = data.get(..2).and_then(|bytes| bytes.try_into().ok()) else {
        return;
    };
    let Some(height) = data.get(2..4).and_then(|bytes| bytes.try_into().ok()) else {
        return;
    };
    let Some(version) = data.get(4..6) else {
        return;
    };
    let Some(resolution) = data.get(6..8).and_then(|bytes| bytes.try_into().ok()) else {
        return;
    };
    let Some(&gamma) = data.get(8) else {
        return;
    };
    let Some(&orientation) = data.get(9) else {
        return;
    };

    metadata.insert(
        "DjVu:ImageWidth".to_string(),
        TagValue::new_integer(i64::from(u16::from_be_bytes(width))),
    );
    metadata.insert(
        "DjVu:ImageHeight".to_string(),
        TagValue::new_integer(i64::from(u16::from_be_bytes(height))),
    );
    metadata.insert(
        "DjVu:DjVuVersion".to_string(),
        TagValue::new_string(format!("{}.{}", version[1], version[0])),
    );
    // DjVu's INFO resolution bytes are little-endian even though chunks use
    // big-endian lengths (ExifTool DjVu.pm:76-80).
    metadata.insert(
        "DjVu:SpatialResolution".to_string(),
        TagValue::new_integer(i64::from(u16::from_le_bytes(resolution))),
    );
    metadata.insert(
        "DjVu:Gamma".to_string(),
        TagValue::Float(f64::from(gamma) / 10.0),
    );
    let orientation = match orientation & 0x07 {
        1 => "Horizontal (normal)".to_string(),
        2 => "Rotate 180".to_string(),
        5 => "Rotate 90 CW".to_string(),
        6 => "Rotate 270 CW".to_string(),
        value => format!("Unknown ({value})"),
    };
    metadata.insert(
        "DjVu:Orientation".to_string(),
        TagValue::new_string(orientation),
    );
}

fn collect_djvu_chunks(chunk: &Chunk, metadata: &mut MetadataMap) {
    match chunk {
        Chunk::Form {
            secondary_id,
            children,
            ..
        } => {
            let subfile_type = match secondary_id.as_slice() {
                b"DJVU" => Some("Single-page image"),
                b"DJVM" => Some("Multi-page document"),
                b"DJVI" => Some("Shared component"),
                b"THUM" => Some("Thumbnail image"),
                b"PM44" => Some("Color IW44"),
                b"BM44" => Some("Grayscale IW44"),
                _ => None,
            };
            if let Some(subfile_type) = subfile_type
                && metadata.get("DjVu:SubfileType").is_none()
            {
                metadata.insert(
                    "DjVu:SubfileType".to_string(),
                    TagValue::new_string(subfile_type),
                );
            }
            for child in children {
                collect_djvu_chunks(child, metadata);
            }
        }
        Chunk::Leaf { id, data } if id == b"INFO" => collect_info(data, metadata),
        Chunk::Leaf { id, data } if id == b"INCL" => {
            let value = data.split(|byte| *byte == 0).next().unwrap_or_default();
            if let Ok(value) = std::str::from_utf8(value) {
                metadata.insert(
                    "DjVu:IncludedFileID".to_string(),
                    TagValue::new_string(value),
                );
            }
        }
        Chunk::Leaf { id, data } if id == b"ANTa" => collect_chunk_metadata(data, metadata),
        Chunk::Leaf { id, data } if id == b"ANTz" => {
            if let Ok(decoded) = bzz_decode(data) {
                collect_chunk_metadata(&decoded, metadata);
            }
        }
        Chunk::Leaf { .. } => {}
    }
}

fn collect_chunk_metadata(data: &[u8], metadata: &mut MetadataMap) {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Some(expressions) = ExpressionParser::new(text).expressions() else {
        return;
    };
    for expression in expressions {
        collect_annotation_expression(expression, metadata);
    }
}

/// Dispatch the two annotation forms ExifTool handles: `(metadata ...)` and
/// top-level `(xmp XML)`.  XMP is not a child of the metadata form.
fn collect_annotation_expression(expression: Expression, metadata: &mut MetadataMap) {
    if let Expression::List(items) = &expression
        && let Some(Expression::Atom(tag)) = items.first()
        && tag == "xmp"
        && let Some(Expression::Atom(xml) | Expression::String(xml)) = items.get(1)
    {
        if let Ok(tags) = parse_xmp_typed(xml.as_bytes()) {
            for (name, value) in tags {
                let value = match value {
                    XmpValue::Scalar(value) => TagValue::new_string(value),
                    XmpValue::List(values) => {
                        TagValue::Array(values.into_iter().map(TagValue::new_string).collect())
                    }
                };
                metadata.insert(name, value);
            }
        }
        return;
    }
    metadata_from_expression(expression, metadata);
}

/// Extract DjVu INFO, included-file and annotation metadata from a DjVu image
/// or multi-page document.
pub fn parse_djvu_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let data = reader
        .read(0, reader.size() as usize)
        .map_err(|error| error.to_string())?;
    let file = parse(&data).map_err(|error| error.to_string())?;

    let Chunk::Form {
        secondary_id,
        children,
        ..
    } = &file.root
    else {
        return Err("DjVu root must be a FORM chunk".to_string());
    };
    if !matches!(secondary_id, b"DJVU" | b"DJVM") {
        return Err("DjVu root FORM type must be DJVU or DJVM".to_string());
    }

    let mut metadata = MetadataMap::new();
    // The outer DJVM form is a directory, not the first image page.  ExifTool
    // takes the first nested FORM's SubfileType for this case.
    if secondary_id == b"DJVM" {
        for child in children {
            collect_djvu_chunks(child, &mut metadata);
        }
    } else {
        collect_djvu_chunks(&file.root, &mut metadata);
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::{ExpressionParser, collect_chunk_metadata, collect_info, metadata_from_expression};
    use crate::core::{MetadataMap, TagValue};

    #[test]
    fn extracts_standard_annotation_metadata_with_exiftool_names() {
        let expressions = ExpressionParser::new(
            r#"(metadata (Author "Phil Harvey") (annote "Did you get this?") (CreationDate "D:20080923123134-04'00'") (Trapped "/Unknown"))"#,
        )
        .expressions()
        .unwrap();
        let mut metadata = MetadataMap::new();
        for expression in expressions {
            metadata_from_expression(expression, &mut metadata);
        }

        assert_eq!(metadata.get_string("DjVu:Author"), Some("Phil Harvey"));
        assert_eq!(
            metadata.get_string("DjVu:Annotation"),
            Some("Did you get this?")
        );
        assert_eq!(
            metadata.get_string("DjVu:CreateDate"),
            Some("2008:09:23 12:31:34-04:00")
        );
        assert_eq!(metadata.get_string("DjVu:Trapped"), Some("Unknown"));
    }

    #[test]
    fn decodes_info_using_djvu_specific_byte_order_and_conversions() {
        let mut metadata = MetadataMap::new();
        collect_info(&[0, 8, 0, 8, 24, 0, 100, 0, 22, 0], &mut metadata);

        assert_eq!(metadata.get_integer("DjVu:ImageWidth"), Some(8));
        assert_eq!(metadata.get_integer("DjVu:ImageHeight"), Some(8));
        assert_eq!(metadata.get_string("DjVu:DjVuVersion"), Some("0.24"));
        assert_eq!(metadata.get_integer("DjVu:SpatialResolution"), Some(100));
        assert_eq!(metadata.get_float("DjVu:Gamma"), Some(2.2));
        assert_eq!(metadata.get_string("DjVu:Orientation"), Some("Unknown (0)"));
    }

    #[test]
    fn extracts_typed_xmp_from_top_level_annotation_expression() {
        let annotation = r#"(xmp "<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:dc='http://purl.org/dc/elements/1.1/'><dc:title><rdf:Alt><rdf:li>DjVu Metadata Sample</rdf:li></rdf:Alt></dc:title><dc:subject><rdf:Bag><rdf:li>ExifTool</rdf:li><rdf:li>Test</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>")"#;
        let mut metadata = MetadataMap::new();
        collect_chunk_metadata(annotation.as_bytes(), &mut metadata);

        assert_eq!(
            metadata.get_string("XMP:Title"),
            Some("DjVu Metadata Sample")
        );
        assert_eq!(
            metadata.get("XMP:Subject"),
            Some(&TagValue::Array(vec![
                TagValue::new_string("ExifTool"),
                TagValue::new_string("Test"),
            ]))
        );
    }
}
