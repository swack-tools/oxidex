//! DjVu annotation extraction.
//!
//! ExifTool 13.59 reads DjVu as an IFF container (`AIFF.pm:184-288`), then
//! decodes `ANTz` with the DjVu BZZ codec and processes its s-expressions
//! (`DjVu.pm:179-300`).  This module deliberately implements only the
//! `metadata`/`annote` path, which ExifTool maps to `DjVu:Annotation`.

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

fn annotation_from_expression(expression: Expression) -> Option<String> {
    let Expression::List(items) = expression else {
        return None;
    };
    let Some(Expression::Atom(tag)) = items.first() else {
        return None;
    };
    if tag != "metadata" {
        return None;
    }

    items.into_iter().skip(1).find_map(|item| {
        let Expression::List(pair) = item else {
            return None;
        };
        let Some(Expression::Atom(name)) = pair.first() else {
            return None;
        };
        let value = match pair.get(1) {
            Some(Expression::Atom(value) | Expression::String(value)) => value,
            _ => return None,
        };
        (name == "annote").then(|| value.clone())
    })
}

fn annotation_from_chunk(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    ExpressionParser::new(text)
        .expressions()?
        .into_iter()
        .find_map(annotation_from_expression)
}

fn collect_annotation(chunk: &Chunk, metadata: &mut MetadataMap) {
    match chunk {
        Chunk::Form { children, .. } => {
            for child in children {
                collect_annotation(child, metadata);
            }
        }
        Chunk::Leaf { id, data } if id == b"ANTa" => {
            if let Some(annotation) = annotation_from_chunk(data) {
                metadata.insert("DjVu:Annotation", TagValue::new_string(annotation));
            }
        }
        Chunk::Leaf { id, data } if id == b"ANTz" => {
            if let Ok(decoded) = bzz_decode(data)
                && let Some(annotation) = annotation_from_chunk(&decoded)
            {
                metadata.insert("DjVu:Annotation", TagValue::new_string(annotation));
            }
        }
        Chunk::Leaf { .. } => {}
    }
}

/// Extract the annotation tag from a DjVu image or multi-page document.
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
