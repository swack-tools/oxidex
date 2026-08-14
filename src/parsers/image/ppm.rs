//! PPM/PGM/PBM (NetPBM) metadata parser.
//!
//! ExifTool routes any `P[1-6]` file through `Image::ExifTool::PPM::ProcessPPM`
//! (`PPM.pm:25-133`), a plain-ASCII header shared by all three NetPBM
//! variants: PBM (bitmap, `P1`/`P4`), PGM (graymap, `P2`/`P5`) and PPM
//! (pixmap, `P3`/`P6`). The digit after `P` selects the reported
//! `File:FileType` via `$num % 3` -- `[qw{PPM PBM PGM}]->[$num % 3]`
//! (`PPM.pm:60`) -- independent of the file's on-disk extension, so this
//! parser sets `File:FileType`/`MIMEType` itself rather than leaving them to
//! `add_identity_tags`'s extension-based guess.
//!
//! The header is: `P[1-6]` + whitespace, an optional `#`-comment block,
//! whitespace-separated `ImageWidth`/`ImageHeight` tokens, and -- for every
//! type but PBM (`PPM.pm:61`, "no MaxVal for PBM images") -- another
//! optional comment block and a `MaxVal` token (`PPM.pm:36-71`). Comment
//! blocks can appear either right after the magic or between the
//! dimensions and `MaxVal`, and multiple contiguous `#` lines merge into one
//! `Comment` tag with each line's leading `"# "` stripped
//! (`PPM.pm:78`, `s/^# ?//mg`).
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/PPM.pm`

use crate::core::{FileReader, MetadataMap, TagValue};

/// `PPM.pm:40,43`: ExifTool grows its read buffer in 1024-byte steps while
/// scanning for the header. Real headers are a handful of lines; this reads
/// generously past that without pulling in image pixel data for a typical
/// file.
const MAX_HEADER_SCAN: usize = 8192;

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0c)
}

struct Scanner<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(is_whitespace) {
            self.pos += 1;
        }
    }

    /// `\s+` -- at least one whitespace byte.
    fn skip_whitespace_required(&mut self) -> bool {
        let start = self.pos;
        self.skip_whitespace();
        self.pos > start
    }

    /// `\S+` -- a run of non-whitespace bytes.
    fn read_token(&mut self) -> Option<&'a [u8]> {
        let start = self.pos;
        while let Some(byte) = self.peek() {
            if is_whitespace(byte) {
                break;
            }
            self.pos += 1;
        }
        (self.pos > start).then(|| &self.data[start..self.pos])
    }

    /// Consumes through the next `\n`/`\r`(`\n`)? or the end of the buffer,
    /// returning the content before it (the newline itself is not part of
    /// the returned slice).
    fn read_line(&mut self) -> &'a [u8] {
        let start = self.pos;
        while let Some(byte) = self.peek() {
            match byte {
                b'\n' => {
                    let content = &self.data[start..self.pos];
                    self.pos += 1;
                    return content;
                }
                b'\r' => {
                    let content = &self.data[start..self.pos];
                    self.pos += 1;
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    return content;
                }
                _ => self.pos += 1,
            }
        }
        &self.data[start..self.pos]
    }

    /// One or more contiguous `#`-prefixed lines (`PPM.pm:49-53,62-66`),
    /// each with a leading `#` and one optional following space stripped
    /// (`PPM.pm:78`, `s/^# ?//mg`, applied here per line rather than after
    /// the fact -- equivalent for every comment shape a real NetPBM header
    /// carries).
    fn read_comment_block(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while self.peek() == Some(b'#') {
            self.pos += 1;
            if self.peek() == Some(b' ') {
                self.pos += 1;
            }
            let line = self.read_line();
            lines.push(String::from_utf8_lossy(line).into_owned());
        }
        lines
    }
}

struct Header {
    file_type: &'static str,
    width: u64,
    height: u64,
    max_val: Option<u64>,
    comment: Option<String>,
}

fn parse_header(data: &[u8]) -> Option<Header> {
    if data.len() < 2 || data[0] != b'P' {
        return None;
    }
    let digit = data[1];
    if !digit.is_ascii_digit() || digit == b'0' || digit > b'6' {
        return None;
    }
    let num = digit - b'0';

    let mut scanner = Scanner { data, pos: 2 };
    if !scanner.skip_whitespace_required() {
        return None;
    }

    let mut comment_lines = scanner.read_comment_block();

    let width: u64 = std::str::from_utf8(scanner.read_token()?)
        .ok()?
        .parse()
        .ok()?;
    if !scanner.skip_whitespace_required() {
        return None;
    }
    let height: u64 = std::str::from_utf8(scanner.read_token()?)
        .ok()?
        .parse()
        .ok()?;
    if !scanner.skip_whitespace_required() {
        return None;
    }

    // PPM.pm:60, `[qw{PPM PBM PGM}]->[$num % 3]`.
    let file_type = match num % 3 {
        1 => "PBM",
        2 => "PGM",
        _ => "PPM",
    };

    let max_val = if file_type == "PBM" {
        // PPM.pm:61: "no MaxVal for PBM images".
        None
    } else {
        comment_lines.extend(scanner.read_comment_block());
        let token = scanner.read_token()?;
        let value: u64 = std::str::from_utf8(token).ok()?.parse().ok()?;
        // PPM.pm:68: `\G(\S+)\s` -- exactly one trailing whitespace byte
        // (not `\s+`) separates MaxVal from the pixel data that follows.
        if !scanner.peek().is_some_and(is_whitespace) {
            return None;
        }
        Some(value)
    };

    let comment = (!comment_lines.is_empty()).then(|| {
        let mut joined = comment_lines.join("\n");
        // PPM.pm:79: `s/[\n\r]+$//` -- trailing newline stripped from the
        // whole comment, not per line.
        while joined.ends_with(['\n', '\r']) {
            joined.pop();
        }
        joined
    });

    Some(Header {
        file_type,
        width,
        height,
        max_val,
        comment,
    })
}

/// Extract PPM/PGM/PBM metadata from a NetPBM image.
pub fn parse_ppm_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let size = reader.size() as usize;
    let want = size.min(MAX_HEADER_SCAN);
    let data = reader.read(0, want).map_err(|error| error.to_string())?;

    let header = parse_header(data).ok_or_else(|| "invalid PPM/PGM/PBM header".to_string())?;

    let mut metadata = MetadataMap::new();
    // PPM.pm:82, `$et->SetFileType($type)` -- content-derived, not the
    // extension `add_identity_tags` would otherwise guess.
    metadata.insert("File:FileType", TagValue::new_string(header.file_type));
    metadata.insert(
        "File:FileTypeExtension",
        TagValue::new_string(header.file_type.to_ascii_lowercase()),
    );
    let mime = match header.file_type {
        "PBM" => "image/x-portable-bitmap",
        "PGM" => "image/x-portable-graymap",
        _ => "image/x-portable-pixmap",
    };
    metadata.insert("File:MIMEType", TagValue::new_string(mime));

    // PPM.pm:129: `foreach $tag (qw{Comment ImageWidth ImageHeight MaxVal})`.
    if let Some(comment) = header.comment {
        metadata.insert("File:Comment", TagValue::new_string(comment));
    }
    metadata.insert("File:ImageWidth", TagValue::Integer(header.width as i64));
    metadata.insert("File:ImageHeight", TagValue::Integer(header.height as i64));
    if let Some(max_val) = header.max_val {
        metadata.insert("File:MaxVal", TagValue::Integer(max_val as i64));
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::MMapReader;
    use std::path::Path;

    fn fixture_reader() -> MMapReader {
        // Real ExifTool test-suite fixture, not hand-authored bytes: see
        // AGENTS.md's rule that regression fixtures must be real files.
        let candidates = [
            "/tmp/oxidex-exiftool-cache/combined-samples/PPM.ppm",
            "/tmp/oxidex-exiftool-cache/exiftool/t/images/PPM.ppm",
        ];
        for candidate in candidates {
            if let Ok(reader) = MMapReader::new(Path::new(candidate)) {
                return reader;
            }
        }
        panic!("PPM.ppm fixture not found in the oxidex-exiftool-cache");
    }

    #[test]
    fn matches_exiftool_13_59_on_the_real_fixture() {
        let reader = fixture_reader();
        let metadata = parse_ppm_metadata(&reader).expect("parses");

        // Cross-checked against `exiftool -a -G1 -s` (pinned 13.59) on the
        // same fixture.
        assert_eq!(
            metadata.get("File:FileType"),
            Some(&TagValue::new_string("PPM"))
        );
        assert_eq!(
            metadata.get("File:Comment"),
            Some(&TagValue::new_string("ExifTool PPM test"))
        );
        assert_eq!(metadata.get("File:ImageWidth"), Some(&TagValue::Integer(8)));
        assert_eq!(
            metadata.get("File:ImageHeight"),
            Some(&TagValue::Integer(8))
        );
        assert_eq!(metadata.get("File:MaxVal"), Some(&TagValue::Integer(255)));
    }
}
