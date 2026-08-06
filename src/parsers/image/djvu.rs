//! DjVu annotation-metadata extraction.
//!
//! ExifTool 13.59 reads DjVu as an IFF container (`AIFF.pm:184-288`), then
//! decodes `ANTz` with the DjVu BZZ codec and processes its s-expressions
//! (`DjVu.pm:179-300`).  This module deliberately implements only the
//! `metadata` path, including `annote` and standard metadata fields.

use crate::core::{FileReader, MetadataMap, TagValue};
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
            "Author" => metadata.insert(
                "DjVu-Meta:Author".to_string(),
                TagValue::new_string(value.clone()),
            ),
            "Title" => metadata.insert(
                "DjVu-Meta:Title".to_string(),
                TagValue::new_string(value.clone()),
            ),
            _ => None,
        };
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
        metadata_from_expression(expression, metadata);
    }
}

fn collect_annotation(chunk: &Chunk, metadata: &mut MetadataMap) {
    match chunk {
        Chunk::Form { children, .. } => {
            for child in children {
                collect_annotation(child, metadata);
            }
        }
        Chunk::Leaf { id, data } if id == b"ANTa" => {
            collect_chunk_metadata(data, metadata);
        }
        Chunk::Leaf { id, data } if id == b"ANTz" => {
            if let Ok(decoded) = bzz_decode(data) {
                collect_chunk_metadata(&decoded, metadata);
            }
        }
        Chunk::Leaf { .. } => {}
    }
}

/// Extract annotation metadata from a DjVu image or multi-page document.
pub fn parse_djvu_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let data = reader
        .read(0, reader.size() as usize)
        .map_err(|error| error.to_string())?;
    let file = parse(&data).map_err(|error| error.to_string())?;

    let Chunk::Form { secondary_id, .. } = &file.root else {
        return Err("DjVu root must be a FORM chunk".to_string());
    };
    if !matches!(secondary_id, b"DJVU" | b"DJVM") {
        return Err("DjVu root FORM type must be DJVU or DJVM".to_string());
    }

    let mut metadata = MetadataMap::new();
    collect_annotation(&file.root, &mut metadata);
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::{ExpressionParser, metadata_from_expression};
    use crate::core::MetadataMap;

    #[test]
    fn extracts_author_from_annotation_metadata() {
        let expressions = ExpressionParser::new(
            r#"(metadata (Author "Phil Harvey") (annote "Did you get this?"))"#,
        )
        .expressions()
        .unwrap();
        let mut metadata = MetadataMap::new();
        for expression in expressions {
            metadata_from_expression(expression, &mut metadata);
        }

        assert_eq!(metadata.get_string("DjVu-Meta:Author"), Some("Phil Harvey"));
        assert_eq!(
            metadata.get_string("DjVu:Annotation"),
            Some("Did you get this?")
        );
    }
}
