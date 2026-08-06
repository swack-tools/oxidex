//! MIFF (Magick Image File Format) parser
//!
//! Per ExifTool's `MIFF.pm`: MIFF is a text-header image format used by
//! ImageMagick. The file starts with the literal bytes `id=ImageMagick`,
//! followed by whitespace-separated `tag=value` entries terminated by
//! `:\x1a` (new-style) or `:\n` (old-style). Values may be brace-delimited
//! (`{...}`), in which case they can contain embedded spaces and must be
//! rejoined across whitespace-split tokens until the closing `}` is found.
//! Curly-brace tokens starting with `{` outside of a `tag=value` pair are
//! comments and are skipped up to their closing `}`.
//!
//! ExifTool decodes *any* tag name found in the header (arbitrary tags are
//! allowed), mapping a fixed set of known keys to friendly names and passing
//! everything else through verbatim as the tag name.
//!
//! `profile-*` entries name an embedded profile (ICC/IPTC/EXIF/XMP) whose
//! value is the profile's byte length; the profile bytes immediately follow
//! the text header in the file. Profile sub-parsing is not implemented here.

use crate::core::{FileReader, MetadataMap, TagValue};

const MIFF_HEADER: &[u8] = b"id=ImageMagick";
/// New-style MIFF text section terminator.
const NEW_TERMINATOR: &[u8] = b":\x1a";

/// Maps MIFF's lowercase/hyphenated key names to ExifTool's tag names.
/// From `Image::ExifTool::MIFF::Main`.
fn known_tag_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "background-color" => "BackgroundColor",
        "blue-primary" => "BluePrimary",
        "border-color" => "BorderColor",
        "matt-color" => "MattColor",
        "class" => "Class",
        "colors" => "Colors",
        "colorspace" => "ColorSpace",
        "columns" => "ImageWidth",
        "compression" => "Compression",
        "delay" => "Delay",
        "depth" => "Depth",
        "dispose" => "Dispose",
        "gamma" => "Gamma",
        "green-primary" => "GreenPrimary",
        "id" => "ID",
        "iterations" => "Iterations",
        "label" => "Label",
        "matte" => "Matte",
        "montage" => "Montage",
        "packets" => "Packets",
        "page" => "Page",
        "red-primary" => "RedPrimary",
        "rendering-intent" => "RenderingIntent",
        "resolution" => "Resolution",
        "rows" => "ImageHeight",
        "scene" => "Scene",
        "signature" => "Signature",
        "units" => "Units",
        "white-point" => "WhitePoint",
        _ => return None,
    })
}

/// Splits MIFF header bytes into whitespace-separated tokens the same way
/// Perl's `split ' '` does: any run of ASCII whitespace is a separator, and
/// leading whitespace produces no empty leading token.
fn split_ws(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// Parses metadata from a MIFF image, following `ProcessMIFF()` in
/// `Image::ExifTool::MIFF.pm`.
pub fn parse_miff_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let size = reader.size();
    if size < MIFF_HEADER.len() as u64 {
        return Err("File too small for MIFF signature".to_string());
    }
    let hdr = reader
        .read(0, MIFF_HEADER.len())
        .map_err(|e| e.to_string())?;
    if hdr != MIFF_HEADER {
        return Err("Invalid MIFF signature".to_string());
    }

    // Read the text header up to the new-style terminator ":\x1a", capped so
    // a malformed/old-style file (terminated by ":\n") can't force reading
    // the whole (possibly huge) file into memory.
    const MAX_HEADER: u64 = 1024 * 1024;
    let scan_len = size.min(MAX_HEADER);
    let buf = reader
        .read(0, scan_len as usize)
        .map_err(|e| e.to_string())?;
    let text_end = buf
        .windows(NEW_TERMINATOR.len())
        .position(|w| w == NEW_TERMINATOR)
        .map(|pos| pos) // text runs [0, pos), terminator excludes trailing ":\x1a"
        .unwrap_or(buf.len());
    let text = String::from_utf8_lossy(&buf[..text_end]);
    let text = text.trim_end_matches([':']); // in case terminator wasn't found and trailing ':' remains

    let mut metadata = MetadataMap::new();
    metadata.insert(
        "FileType".to_string(),
        TagValue::String("MIFF".to_string()),
    );
    metadata.insert("FileSize".to_string(), TagValue::Integer(size as i64));

    let mut entries = split_ws(text);
    // Put the id= header token back at the front, matching ExifTool
    // unshifting $hdr onto @entries.
    let hdr_str = "id=ImageMagick";
    entries.insert(0, hdr_str);

    #[derive(PartialEq)]
    enum Mode {
        None,
        Comment,
        Value,
    }
    let mut mode = Mode::None;
    let mut tag = String::new();
    let mut val = String::new();

    for entry in entries {
        match mode {
            Mode::Comment => {
                if entry.ends_with('}') {
                    mode = Mode::None;
                }
                continue;
            }
            Mode::Value => {
                val.push(' ');
                val.push_str(entry);
                if !entry.ends_with('}') {
                    continue;
                }
                mode = Mode::None;
                // strip a single leading '{' and trailing '}'
                if let Some(stripped) = val.strip_prefix('{') {
                    val = stripped.to_string();
                }
                if let Some(stripped) = val.strip_suffix('}') {
                    val = stripped.to_string();
                }
            }
            Mode::None => {
                if entry.starts_with('{') {
                    // Mirrors ExifTool's ProcessMIFF(): entering comment
                    // mode does not check for a closing brace in the same
                    // token, so a self-contained `{comment}` token still
                    // requires a later token ending in `}` to exit.
                    mode = Mode::Comment;
                    continue;
                } else if entry.rfind('=').is_some_and(|eq| eq > 0 && eq < entry.len() - 1) {
                    // Perl's greedy `/(.+)=(.+)/` backtracks to the
                    // rightmost '=' that still leaves >=1 char on each side.
                    let eq = entry.rfind('=').unwrap();
                    tag = entry[..eq].to_string();
                    val = entry[eq + 1..].to_string();
                    if val.starts_with('{') {
                        mode = Mode::Value;
                        continue;
                    }
                } else if entry.starts_with(':') {
                    break;
                } else {
                    // Unrecognized data -- stop parsing (mirrors ExifTool's Warn + last)
                    break;
                }
            }
        }

        // A completed tag=value pair.
        if tag.starts_with("profile-") {
            // Embedded profile (profile-icc/profile-iptc/profile-APP1/...):
            // ExifTool decodes the following `val` bytes as an ICC/IPTC
            // (Photoshop)/EXIF/XMP sub-directory and reports those tags
            // under their own real group, not a synthetic "Profile-*" tag.
            // That sub-directory decode isn't implemented here, so we omit
            // it rather than invent a tag name ExifTool never emits.
        } else if let Some(name) = known_tag_name(&tag) {
            metadata.insert(name.to_string(), TagValue::String(val.clone()));
        } else {
            // Arbitrary tag: ExifTool passes the raw key through as the tag
            // name verbatim.
            metadata.insert(tag.clone(), TagValue::String(val.clone()));
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::buffered_reader::BufferedReader;

    fn text(metadata: &MetadataMap, key: &str) -> String {
        match metadata.get(key) {
            Some(TagValue::String(s)) => s.clone(),
            Some(TagValue::Integer(i)) => i.to_string(),
            other => panic!("{key} not found or not printable scalar: {other:?}"),
        }
    }

    #[test]
    fn parses_basic_header() {
        let data =
            b"id=ImageMagick\nclass=DirectClass  matte=False\ncolumns=8  rows=8  depth=8\n\
              Resolution=72x72  units=pixels-per-inch\n:\x1a\x00\x00rest-of-file";
        let reader = BufferedReader::from_bytes(data);
        let meta = parse_miff_metadata(&reader).expect("parse should succeed");
        assert_eq!(text(&meta, "Class"), "DirectClass");
        assert_eq!(text(&meta, "Matte"), "False");
        assert_eq!(text(&meta, "ImageWidth"), "8");
        assert_eq!(text(&meta, "ImageHeight"), "8");
        assert_eq!(text(&meta, "Depth"), "8");
        assert_eq!(text(&meta, "Resolution"), "72x72");
        assert_eq!(text(&meta, "Units"), "pixels-per-inch");
        assert_eq!(text(&meta, "FileType"), "MIFF");
    }

    #[test]
    fn rejects_non_miff() {
        let reader = BufferedReader::from_bytes(b"not a miff file");
        assert!(parse_miff_metadata(&reader).is_err());
    }

    #[test]
    fn passes_through_arbitrary_tags() {
        let data = b"id=ImageMagick\nmy-custom-tag=hello\n:\x1a";
        let reader = BufferedReader::from_bytes(data);
        let meta = parse_miff_metadata(&reader).expect("parse should succeed");
        assert_eq!(text(&meta, "my-custom-tag"), "hello");
    }

    #[test]
    fn handles_braced_values() {
        let data = b"id=ImageMagick\nlabel={hello world value}\n:\x1a";
        let reader = BufferedReader::from_bytes(data);
        let meta = parse_miff_metadata(&reader).expect("parse should succeed");
        assert_eq!(text(&meta, "Label"), "hello world value");
    }
}
