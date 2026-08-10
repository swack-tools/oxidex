//! Radiance RGBE (HDR) high dynamic range image parser
//!
//! ExifTool source: `Image::ExifTool::Radiance::ProcessHDR` and
//! `%Image::ExifTool::Radiance::Main` in `lib/Image/ExifTool/Radiance.pm`.
//!
//! A Radiance file opens with a one-line signature, a header of
//! `KEY=value` assignments, comments and bare command lines, a blank line,
//! then a resolution line before the RLE-compressed RGBE scanlines:
//!
//! ```text
//! #?RADIANCE
//! oconv mat.rad sky.rad surfaces.rad
//! SOFTWARE= RADIANCE 3.1.8 lastmod ...
//! FORMAT=32-bit_rle_rgbe
//!
//! -Y 1 +X 1
//! ```
//!
//! Before this parser existed the format was detected and not parsed: the
//! magic table gave `File:FileType: HDR` while `read_metadata` fell through
//! to the plain-text parser, so all nine of ExifTool's content tags were
//! missing behind a correct-looking identity.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// ExifTool's magic number for the format: `#\?(RADIANCE|RGBE)\x0a`
const SIGNATURES: [&[u8]; 2] = [b"#?RADIANCE\n", b"#?RGBE\n"];

/// How much of the file the header can occupy before we stop looking
///
/// The header is a handful of short lines in every real file; the cap only
/// bounds the read for a file that lies about being one.
const MAX_HEADER_BYTES: u64 = 64 * 1024;

/// ExifTool abandons the header at a line this long (`Radiance.pm:77`):
/// `last unless length($buff) > 0 and length($buff) < 4096`
const MAX_LINE_LEN: usize = 4096;

/// `%Image::ExifTool::Radiance::Main`, less the two synthesized `_` entries
///
/// The keys are ExifTool's lowercased header names; the values are the tag
/// names it reports. A header key absent from this list still becomes a tag,
/// via [`synthesized_tag_name`] -- that is ExifTool's `AddTagToTable`
/// fallback, not a silent drop.
const HEADER_TAGS: [(&str, &str); 8] = [
    ("software", "Software"),
    ("view", "View"),
    ("format", "Format"),
    ("exposure", "Exposure"),
    ("gamma", "Gamma"),
    ("colorcorr", "ColorCorrection"),
    ("pixaspect", "PixelAspectRatio"),
    ("primaries", "ColorPrimaries"),
];

/// `PrintConv` of the `_orient` tag (`Radiance.pm:33-42`)
///
/// The key is the pair of axis signs from the resolution line, in the order
/// they appear there.
const ORIENTATION: [(&str, &str); 8] = [
    ("-Y +X", "Horizontal (normal)"),
    ("-Y -X", "Mirror horizontal"),
    ("+Y -X", "Rotate 180"),
    ("+Y +X", "Mirror vertical"),
    ("+X -Y", "Mirror horizontal and rotate 270 CW"),
    ("+X +Y", "Rotate 90 CW"),
    ("-X +Y", "Mirror horizontal and rotate 90 CW"),
    ("-X -Y", "Rotate 270 CW"),
];

/// Whether the buffer opens with the Radiance signature line
///
/// This is `matches_magic("HDR", ..)`'s question, answered without a regex
/// for the parser's own re-check.
#[must_use]
pub fn looks_like_radiance(bytes: &[u8]) -> bool {
    SIGNATURES
        .iter()
        .any(|signature| bytes.starts_with(signature))
}

/// The tag name ExifTool invents for a header key its table does not list
///
/// `Radiance.pm:90-93`:
///
/// ```text
///     my $name = $tag;
///     $name =~ tr/-_a-zA-Z0-9//dc;
///     next unless length($name) > 1;
///     $name = ucfirst $name;
/// ```
///
/// The key has already been lowercased, so `ucfirst` is the only casing.
fn synthesized_tag_name(key: &str) -> Option<String> {
    let name: String = key
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || *character == '-' || *character == '_'
        })
        .collect();
    if name.chars().count() < 2 {
        return None;
    }

    let mut characters = name.chars();
    let first = characters.next()?;
    Some(first.to_ascii_uppercase().to_string() + characters.as_str())
}

/// Splits a header line into its key and value the way ExifTool's regex does
///
/// `unless ($buff =~ /^(.*)?\s*=\s*(.*)/)`: `.*` is greedy, so it backtracks
/// from the end of the line and the split lands on the *last* `=`, not the
/// first. Whitespace is stripped after the `=` (by `\s*`) but the greedy
/// `.*` keeps any that precedes it, which is why `FORMAT =x` yields the key
/// `"format "` -- with the space -- and reaches the synthesized-name path
/// rather than the table.
fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let index = line.rfind('=')?;
    Some((&line[..index], line[index + 1..].trim_start()))
}

/// Reads a `\x0a`-terminated line, ExifTool's `local $/ = "\x0a"`
///
/// A lone `\r` is not a terminator and stays in the line, matching `chomp`
/// under that `$/`.
fn next_line(rest: &mut &[u8]) -> Option<Vec<u8>> {
    if rest.is_empty() {
        return None;
    }
    let end = rest.iter().position(|byte| *byte == b'\n');
    let (line, remainder) = match end {
        Some(index) => (&rest[..index], &rest[index + 1..]),
        None => (&rest[..], &rest[rest.len()..]),
    };
    *rest = remainder;
    Some(line.to_vec())
}

/// Parses the resolution line: `([-+][XY])\s*(\d+)\s*([-+][XY])\s*(\d+)`
///
/// Returns `(orientation, height, width)` -- ExifTool takes the *first*
/// number as the height (`Radiance.pm:102-103`), because a Radiance
/// resolution string names the rows before the columns.
///
/// The pattern is unanchored and its separators are optional, so this
/// searches the line and does not require whitespace. Splitting on
/// whitespace instead would read every well-formed file correctly and
/// silently report no dimensions for `-Y480 +X640`, which ExifTool reads.
fn parse_resolution(line: &str) -> Option<(String, u32, u32)> {
    let bytes = line.as_bytes();
    (0..bytes.len()).find_map(|start| match_resolution(&bytes[start..]))
}

/// Matches the resolution pattern at the very start of `bytes`.
fn match_resolution(bytes: &[u8]) -> Option<(String, u32, u32)> {
    let (first_axis, rest) = take_axis(bytes)?;
    let (height, rest) = take_number(skip_whitespace(rest))?;
    let (second_axis, rest) = take_axis(skip_whitespace(rest))?;
    let (width, _) = take_number(skip_whitespace(rest))?;

    Some((format!("{first_axis} {second_axis}"), height, width))
}

/// Splits off a leading `[-+][XY]`.
fn take_axis(bytes: &[u8]) -> Option<(&str, &[u8])> {
    if !matches!(bytes, [b'+' | b'-', b'X' | b'Y', ..]) {
        return None;
    }
    let (axis, rest) = bytes.split_at(2);
    // Both bytes are ASCII, so the pair is valid UTF-8.
    Some((std::str::from_utf8(axis).ok()?, rest))
}

/// Splits off a leading run of ASCII digits, `\d+`.
fn take_number(bytes: &[u8]) -> Option<(u32, &[u8])> {
    let end = bytes
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(bytes.len());
    let (digits, rest) = bytes.split_at(end);
    if digits.is_empty() {
        return None;
    }
    let value: u32 = std::str::from_utf8(digits).ok()?.parse().ok()?;
    Some((value, rest))
}

/// Skips a run of whitespace, `\s*`.
fn skip_whitespace(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    &bytes[end..]
}

/// Appends a value under `tag`, keeping earlier ones
///
/// A Radiance header may carry several `Command` lines -- the sample in the
/// shared corpus has four, one per program in the render pipeline -- and
/// ExifTool reports every one. Overwriting would report only the last and
/// look like a correct single answer.
fn push_value(metadata: &mut MetadataMap, tag: &str, value: String) {
    let value = TagValue::String(value);
    match metadata.remove(tag) {
        Some(TagValue::Array(mut values)) => {
            values.push(value);
            metadata.insert(tag.to_string(), TagValue::Array(values));
        }
        Some(existing) => {
            metadata.insert(tag.to_string(), TagValue::Array(vec![existing, value]));
        }
        None => {
            metadata.insert(tag.to_string(), value);
        }
    }
}

/// Parser for Radiance RGBE (HDR) images.
pub struct RadianceParser;

impl RadianceParser {
    /// Verifies the file opens with `#?RADIANCE` or `#?RGBE`.
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        let probe_len = reader.size().min(16) as usize;
        if probe_len == 0 {
            return Ok(false);
        }
        Ok(looks_like_radiance(reader.read(0, probe_len)?))
    }
}

impl FormatParser for RadianceParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let read_len = reader.size().min(MAX_HEADER_BYTES) as usize;
        let data = reader.read(0, read_len)?;
        let mut rest = data;

        let signature = next_line(&mut rest)
            .ok_or_else(|| ExifToolError::parse_error("Empty Radiance file"))?;
        // `next_line` has already removed the `\x0a` the magic number requires.
        if !SIGNATURES
            .iter()
            .any(|expected| signature.as_slice() == &expected[..expected.len() - 1])
        {
            return Err(ExifToolError::parse_error("Invalid Radiance signature"));
        }

        let mut metadata = MetadataMap::new();
        // A fallback name for `normalize_identity_tags`; the magic table
        // already supplies `File:FileType`, `FileTypeExtension` and the
        // `image/vnd.radiance` MIME type, and a parser's MIMEType is never
        // promoted, so writing one here could only duplicate it.
        metadata.insert("FileType".to_string(), TagValue::String("HDR".to_string()));

        while let Some(line) = next_line(&mut rest) {
            let line = String::from_utf8_lossy(&line).into_owned();
            // The blank line ends the header and precedes the resolution.
            if line.is_empty() || line.len() >= MAX_LINE_LEN {
                break;
            }

            if let Some(comment) = line.strip_prefix('#') {
                let comment = comment.trim_start();
                if !comment.is_empty() {
                    push_value(&mut metadata, "Comment", comment.to_string());
                }
                continue;
            }

            let Some((key, value)) = split_assignment(&line) else {
                push_value(&mut metadata, "Command", line.clone());
                continue;
            };

            let key = key.to_ascii_lowercase();
            let tag = HEADER_TAGS
                .iter()
                .find_map(|(header, tag)| (*header == key).then_some((*tag).to_string()))
                .or_else(|| synthesized_tag_name(&key));
            if let Some(tag) = tag {
                push_value(&mut metadata, &tag, value.to_string());
            }
        }

        // The line after the header carries the orientation and dimensions.
        if let Some(line) = next_line(&mut rest) {
            let line = String::from_utf8_lossy(&line);
            if let Some((axes, height, width)) = parse_resolution(&line) {
                let orientation = ORIENTATION
                    .iter()
                    .find_map(|(axes_key, label)| (*axes_key == axes).then_some(*label));
                // ExifTool prints the raw axis pair when the PrintConv has no
                // entry for it, rather than inventing a label.
                metadata.insert(
                    "Orientation".to_string(),
                    TagValue::String(orientation.unwrap_or(&axes).to_string()),
                );
                metadata.insert("ImageHeight".to_string(), TagValue::Integer(height as i64));
                metadata.insert("ImageWidth".to_string(), TagValue::Integer(width as i64));
            }
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::HDR)
    }
}

/// Parses metadata from a Radiance RGBE (HDR) file.
///
/// This is a convenience wrapper around [`RadianceParser`] that provides a
/// functional API matching the other format parsers.
pub fn parse_radiance_metadata(
    reader: &dyn FileReader,
) -> std::result::Result<MetadataMap, String> {
    let parser = RadianceParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// The header of `Radiance.hdr` in the shared corpus, abridged in the
    /// command lines only. Every tag ExifTool 13.59 reports for that file is
    /// pinned below.
    fn corpus_header() -> Vec<u8> {
        let mut data = concat!(
            "#?RADIANCE\n",
            "oconv mat.rad sky.rad surfaces.rad\n",
            "oconv -f -i test4.oct ila01728\n",
            "SOFTWARE= RADIANCE 3.1.8 lastmod Thu Sep 17 20:49:56 PDT 1998 by droberts on escher\n",
            "VIEW= -vtv -vp 0.832108 2.26053 1.8 -vh 100 -vv 100\n",
            "FORMAT=32-bit_rle_rgbe\n",
            "EXPOSURE=3.512179e-001\n",
            "\n",
            "-Y 1 +X 1\n",
        )
        .as_bytes()
        .to_vec();
        // Stand in for the RLE scanline that follows the resolution line.
        data.extend_from_slice(&[0x02, 0x02, 0x00, 0x01]);
        data
    }

    fn parse(data: Vec<u8>) -> MetadataMap {
        parse_radiance_metadata(&TestReader::new(data)).expect("parse should succeed")
    }

    fn string(metadata: &MetadataMap, tag: &str) -> String {
        match metadata.get(tag) {
            Some(TagValue::String(value)) => value.clone(),
            other => panic!("{tag} was {other:?}, expected a string"),
        }
    }

    #[test]
    fn extracts_the_header_assignments() {
        let metadata = parse(corpus_header());
        assert_eq!(
            string(&metadata, "Software"),
            "RADIANCE 3.1.8 lastmod Thu Sep 17 20:49:56 PDT 1998 by droberts on escher"
        );
        assert_eq!(
            string(&metadata, "View"),
            "-vtv -vp 0.832108 2.26053 1.8 -vh 100 -vv 100"
        );
        assert_eq!(string(&metadata, "Format"), "32-bit_rle_rgbe");
        assert_eq!(string(&metadata, "Exposure"), "3.512179e-001");
    }

    /// The resolution line names rows before columns, so the first number is
    /// the height. Reading it as the width transposes every non-square image
    /// and no test of a 1x1 sample would notice.
    #[test]
    fn reads_the_resolution_line_rows_first() {
        let mut data = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 480 +X 640\n".to_vec();
        data.extend_from_slice(&[0x02, 0x02]);
        let metadata = parse(data);
        assert_eq!(metadata.get("ImageHeight"), Some(&TagValue::Integer(480)));
        assert_eq!(metadata.get("ImageWidth"), Some(&TagValue::Integer(640)));
        assert_eq!(string(&metadata, "Orientation"), "Horizontal (normal)");
    }

    /// ExifTool's pattern is unanchored and its `\s*` separators are
    /// optional. Reading the line by splitting on whitespace passes every
    /// well-formed file and then silently reports no dimensions at all for
    /// these, which ExifTool reads without complaint.
    #[test]
    fn reads_resolutions_the_regex_accepts_but_whitespace_splitting_does_not() {
        assert_eq!(
            parse_resolution("-Y480 +X640"),
            Some(("-Y +X".to_string(), 480, 640))
        );
        assert_eq!(
            parse_resolution("\t-Y 480 +X 640"),
            Some(("-Y +X".to_string(), 480, 640))
        );
        assert_eq!(
            parse_resolution("resolution -Y 2 +X 3"),
            Some(("-Y +X".to_string(), 2, 3))
        );
        assert_eq!(parse_resolution("-Y 480"), None);
        assert_eq!(parse_resolution("nothing here"), None);
    }

    #[test]
    fn maps_every_orientation_the_print_conv_lists() {
        for (axes, label) in ORIENTATION {
            let (first, second) = axes.split_once(' ').expect("axis pair");
            let data = format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n{first} 2 {second} 3\n")
                .into_bytes();
            assert_eq!(string(&parse(data), "Orientation"), label, "for {axes}");
        }
    }

    /// The sample carries four `Command` lines; reporting only the last would
    /// look like one correct answer.
    #[test]
    fn keeps_every_command_line() {
        let metadata = parse(corpus_header());
        let Some(TagValue::Array(commands)) = metadata.get("Command") else {
            panic!(
                "expected an array of commands, got {:?}",
                metadata.get("Command")
            );
        };
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0],
            TagValue::String("oconv mat.rad sky.rad surfaces.rad".to_string())
        );
    }

    #[test]
    fn reports_comments_without_their_marker() {
        let data =
            b"#?RADIANCE\n#  rendered overnight\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 1\n".to_vec();
        assert_eq!(string(&parse(data), "Comment"), "rendered overnight");
    }

    /// A header key the table does not list still becomes a tag, the way
    /// `AddTagToTable` does it -- stripped of punctuation and capitalised.
    #[test]
    fn invents_a_name_for_an_unlisted_key() {
        let data =
            b"#?RADIANCE\nCAPDATE= 1998:09:17\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 1\n".to_vec();
        let metadata = parse(data);
        assert_eq!(string(&metadata, "Capdate"), "1998:09:17");

        // `tr/-_a-zA-Z0-9//dc` leaves under two characters here, so ExifTool
        // skips the line rather than reporting a one-letter tag.
        let data = b"#?RADIANCE\n%*= dropped\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 1\n".to_vec();
        assert!(!parse(data).contains_key("Dropped"));
    }

    /// `.*` is greedy, so the split lands on the last `=`, not the first.
    #[test]
    fn splits_an_assignment_at_its_last_equals_sign() {
        assert_eq!(
            split_assignment("VIEW= -vf a=b"),
            Some(("VIEW= -vf a", "b"))
        );
        assert_eq!(
            split_assignment("FORMAT=32-bit"),
            Some(("FORMAT", "32-bit"))
        );
        assert_eq!(split_assignment("oconv mat.rad"), None);
    }

    #[test]
    fn rejects_a_file_without_the_signature() {
        let reader = TestReader::new(b"#?SOMETHINGELSE\nFORMAT=x\n".to_vec());
        assert!(!RadianceParser::verify_signature(&reader).unwrap());
        assert!(parse_radiance_metadata(&reader).is_err());
    }

    #[test]
    fn accepts_the_rgbe_spelling_of_the_signature() {
        let data = b"#?RGBE\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 1\n".to_vec();
        assert_eq!(string(&parse(data), "Format"), "32-bit_rle_rgbe");
    }
}
