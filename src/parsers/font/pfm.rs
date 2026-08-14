//! Windows Printer Font Metrics (PFM) header extraction.
//!
//! ExifTool 13.59 reads this format in `Font.pm`'s `ProcessFont`
//! (Font.pm:844-871): a `.pfm` whose first two bytes are `\0\x01` or `\0\x02`
//! is a Printer Font Metrics file, and its 117-byte header is handed to
//! `ProcessBinaryData` through `%Image::ExifTool::Font::PFM`
//! (Font.pm:278-315). Two further tags -- `FontName` and
//! `PostScriptFontName` -- are not in that table: `ProcessFont` follows a
//! file offset out of the header and reads them itself (Font.pm:864-870).
//!
//! # Why this module exists
//!
//! `FileFormat::PFM` covers two unrelated formats that share the extension
//! and the `FileType: PFM` name (see [`crate::filetype`]'s `refine`), and
//! only the Portable FloatMap one had a parser. A Printer Font Metrics file
//! therefore reported a correct `File:FileType`, `FileTypeExtension` and
//! `MIMEType` and nothing else -- `Status: IdentifiedOnly`, 26 tags behind
//! the pinned oracle under `tools/exiftool-tables/conformance.py`, with no
//! error raised anywhere. That is `AGENTS.md`'s "Detected is not parsed".
//!
//! # The table is the layout, and it is gated
//!
//! Every offset, format and `PrintConv` below comes from the generated
//! `Font::PFM` table, not from a retyped copy of the Perl. The table is
//! walked only when [`BinaryTable::enabled`](crate::exiftool_tables::BinaryTable::enabled)
//! says both Step 28 gates pass, so removing its one line from
//! `src/exiftool_tables/enabled.rs` removes exactly these 24 tags and
//! nothing else -- the allowlist line and the revert are the same object.
//! `FontName`/`PostScriptFontName` are outside the table and outside that
//! gate, because they are not `ProcessBinaryData` output.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{decode_binary_table, find_table};
use crate::io::ByteOrder;

/// `$raf->Read($buff,117) == 117` (Font.pm:848).
const PFM_HEADER_LEN: usize = 117;

/// `Get32u(\$buff, 101)` -- the file offset of the `"PostScript\0"`
/// device-type string ExifTool validates (Font.pm:850-853).
const DEVICE_TYPE_OFFSET_AT: usize = 101;

/// `Get32u(\$buff, 105)` -- the file offset of the NUL-separated
/// `FontName`/`PostScriptFontName` pair (Font.pm:864).
const NAME_OFFSET_AT: usize = 105;

/// `$raf->Read($buf2, 11) == 11 and lc($buf2) eq "postscript\0"` (Font.pm:853).
/// FontForge writes `Postscript\0`, which is why ExifTool compares case-
/// insensitively; matching that exactly is the difference between reading a
/// real-world file and refusing it.
const DEVICE_TYPE: &[u8] = b"postscript\0";

/// `$raf->Read($buff, 256)` (Font.pm:865).
const NAME_PROBE_LEN: usize = 256;

/// ExifTool's own acceptance test for a Printer Font Metrics file
/// (Font.pm:844-853), in order:
///
/// ```text
///     } elsif ($buff =~ /^\0[\x01\x02]/ and $raf->Seek(0, 2) and    # PFM
///              # validate file size
///              $raf->Tell() > 117 and $raf->Tell() == unpack('x2V',$buff) and
///              # read PFM header
///              $raf->Seek(0,0) and $raf->Read($buff,117) == 117 and
///              # validate "DeviceType" string (must be "PostScript\0")
///              SetByteOrder('II') and $raf->Seek(Get32u(\$buff, 101), 0) and
///              $raf->Read($buf2, 11) == 11 and lc($buf2) eq "postscript\0")
/// ```
///
/// Every clause is load-bearing, and the self-describing size check at offset
/// 2 is what makes this safe to try before the Portable FloatMap parser: a
/// FloatMap file begins `P` (0x50), so it cannot reach the second clause at
/// all.
fn read_validated_header(reader: &dyn FileReader) -> Option<Vec<u8>> {
    let size = reader.size();
    if size <= PFM_HEADER_LEN as u64 {
        return None;
    }
    let header = reader.read(0, PFM_HEADER_LEN).ok()?;
    if !matches!(header.first(), Some(0)) || !matches!(header.get(1), Some(1 | 2)) {
        return None;
    }
    // `unpack('x2V', $buff)` -- little-endian int32u at offset 2.
    let declared = u32::from_le_bytes(header.get(2..6)?.try_into().ok()?);
    if u64::from(declared) != size {
        return None;
    }
    let device_at = u64::from(u32::from_le_bytes(
        header
            .get(DEVICE_TYPE_OFFSET_AT..DEVICE_TYPE_OFFSET_AT + 4)?
            .try_into()
            .ok()?,
    ));
    let device = reader.read(device_at, DEVICE_TYPE.len()).ok()?;
    if !device.eq_ignore_ascii_case(DEVICE_TYPE) {
        return None;
    }
    Some(header.to_vec())
}

/// Font.pm:864-870 -- the two names `%Font::PFM` does not describe.
///
/// ```text
///     my $nameOff = Get32u(\$buff, 105);
///     if ($raf->Seek($nameOff, 0) and $raf->Read($buff, 256) and
///         $buff =~ /^([\x20-\xff]+)\0([\x20-\xff]+)\0/)
///     {
///         $et->HandleTag($tagTablePtr, 'fontname', $1);
///         $et->HandleTag($tagTablePtr, 'postfont', $2);
///     }
/// ```
///
/// The character class is `[\x20-\xff]`, so both runs are non-empty and stop
/// at the first NUL; a short final read is fine (`Read` is not length-checked
/// here), but the pattern is anchored, so a leading NUL yields neither tag.
fn read_font_names(reader: &dyn FileReader, header: &[u8]) -> Option<(String, String)> {
    let name_at = u64::from(u32::from_le_bytes(
        header
            .get(NAME_OFFSET_AT..NAME_OFFSET_AT + 4)?
            .try_into()
            .ok()?,
    ));
    let available = reader
        .size()
        .saturating_sub(name_at)
        .min(NAME_PROBE_LEN as u64) as usize;
    let buf = reader.read(name_at, available).ok()?;

    let mut runs = buf.split(|byte| *byte == 0);
    let first = runs.next()?;
    let second = runs.next()?;
    // The regex requires a NUL after each run, so the second run must be
    // terminated too -- `split` yields a trailing element for an unterminated
    // tail, which `runs.next()` below rejects by requiring it to exist.
    runs.next()?;
    if first.is_empty() || second.is_empty() {
        return None;
    }
    if !first.iter().chain(second).all(|byte| *byte >= 0x20) {
        return None;
    }
    Some((
        String::from_utf8_lossy(first).into_owned(),
        String::from_utf8_lossy(second).into_owned(),
    ))
}

/// Parser for Windows Printer Font Metrics files.
pub struct PrinterFontMetricsParser;

impl PrinterFontMetricsParser {
    /// Whether `reader` is a Printer Font Metrics file by ExifTool's own
    /// test. Used by the `FileFormat::PFM` dispatch arm to tell the two
    /// `.pfm` formats apart before choosing a parser.
    #[must_use]
    pub fn verify_signature(reader: &dyn FileReader) -> bool {
        read_validated_header(reader).is_some()
    }
}

impl FormatParser for PrinterFontMetricsParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let header = read_validated_header(reader).ok_or_else(|| {
            ExifToolError::parse_error("not a Windows Printer Font Metrics header")
        })?;

        let mut metadata = MetadataMap::new();

        // Font.pm:857-860 -- `SetByteOrder('II')`, then the 117-byte header
        // through `%Image::ExifTool::Font::PFM`.
        let table = find_table("Font", "PFM")
            .ok_or_else(|| ExifToolError::parse_error("missing generated Font::PFM table"))?;
        if table.enabled() {
            for decoded in decode_binary_table(table, &header, ByteOrder::Little).fields() {
                // Every `Font::PFM` field is `Omitted::NONE` (no ValueConv,
                // RawConv, Condition, Hook or SubDirectory), so `emit` is the
                // whole conversion -- there is no field here that needs a
                // hand-written `RawAccess` acknowledgment.
                if let Some(value) = decoded.emit() {
                    metadata.insert(format!("Font:{}", decoded.field.name), value);
                }
            }
        }

        if let Some((font_name, post_name)) = read_font_names(reader, &header) {
            metadata.insert("Font:FontName".to_string(), TagValue::String(font_name));
            metadata.insert(
                "Font:PostScriptFontName".to_string(),
                TagValue::String(post_name),
            );
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::PFM)
    }
}

/// Parses metadata from a Windows Printer Font Metrics (`.pfm`) file.
pub fn parse_printer_font_metrics(
    reader: &dyn FileReader,
) -> std::result::Result<MetadataMap, String> {
    PrinterFontMetricsParser
        .parse(reader)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::buffered_reader::BufferedReader;

    /// The one Printer Font Metrics carrier in the pinned corpus.
    ///
    /// Every expected value below was read off `exiftool-pinned.sh -G1 -s -a`
    /// (ExifTool 13.59) for this exact file, NOT off a byte pattern chosen
    /// here. A synthetic 117-byte header assembled to produce these numbers
    /// would confirm its own arithmetic and could not observe a wrong offset,
    /// a wrong byte order or a missing `PrintConv`.
    const CARRIER: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Font.pfm";

    fn carrier() -> Option<BufferedReader> {
        if !crate::test_support::pinned_corpus_available() {
            return None;
        }
        BufferedReader::new(std::path::Path::new(CARRIER)).ok()
    }

    /// The whole `Font::PFM` table against the pinned oracle's output for the
    /// real file, tag by tag. `PFMVersion` is the one that exercises the
    /// generated `PrintConv` expression (`sprintf("%x.%.2x",$val>>8,$val&0xff)`,
    /// Font.pm:286) rather than a plain integer read.
    #[test]
    fn pinned_font_pfm_header_matches_exiftool() {
        let Some(reader) = carrier() else { return };
        let metadata = PrinterFontMetricsParser
            .parse(&reader)
            .expect("parse pinned Font.pfm fixture");

        assert_eq!(metadata.get_string("Font:PFMVersion"), Some("1.00"));
        assert_eq!(
            metadata.get_string("Font:Copyright"),
            Some("Copyright URW Software, Copyright 1992 by URW")
        );
        for (tag, expected) in [
            ("Font:FontType", 129),
            ("Font:PointSize", 10),
            ("Font:YResolution", 300),
            ("Font:XResolution", 300),
            ("Font:Ascent", 700),
            ("Font:InternalLeading", 0),
            ("Font:ExternalLeading", 0),
            ("Font:Italic", 0),
            ("Font:Underline", 0),
            ("Font:Strikeout", 0),
            ("Font:Weight", 600),
            ("Font:CharacterSet", 0),
            ("Font:PixWidth", 0),
            ("Font:PixHeight", 0),
            ("Font:PitchAndFamily", 1),
            ("Font:AvgWidth", 578),
            ("Font:MaxWidth", 1092),
            ("Font:FirstChar", 32),
            ("Font:LastChar", 255),
            ("Font:DefaultChar", 32),
            ("Font:BreakChar", 0),
            ("Font:WidthBytes", 0),
        ] {
            assert_eq!(
                metadata.get_integer(tag),
                Some(expected),
                "{tag} disagrees with pinned ExifTool 13.59"
            );
        }
    }

    /// Font.pm:864-870's pointer-following pair, which is NOT in the
    /// `Font::PFM` table and so is not behind the Step 28 allowlist. Reading
    /// them proves the `Get32u(\$buff, 105)` offset is followed, not guessed.
    #[test]
    fn pinned_font_pfm_names_come_from_the_header_offset() {
        let Some(reader) = carrier() else { return };
        let metadata = PrinterFontMetricsParser
            .parse(&reader)
            .expect("parse pinned Font.pfm fixture");

        assert_eq!(metadata.get_string("Font:FontName"), Some("URWGroT"));
        assert_eq!(
            metadata.get_string("Font:PostScriptFontName"),
            Some("URWGroteskT-Bold")
        );
    }

    /// `.pfm` is two formats. The Portable FloatMap carrier must not be
    /// claimed by this parser -- if it were, `PFM.pfm` would lose its four
    /// image tags to a silent misroute rather than to a visible error.
    #[test]
    fn the_floatmap_carrier_is_not_a_printer_font_metrics_file() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = std::path::Path::new("/tmp/oxidex-exiftool-cache/combined-samples/PFM.pfm");
        let reader = BufferedReader::new(path).expect("read pinned PFM.pfm fixture");
        assert!(!PrinterFontMetricsParser::verify_signature(&reader));
        assert!(PrinterFontMetricsParser.parse(&reader).is_err());
    }

    /// The gate is the point (Step 28 D1): the 24 table tags exist only
    /// because `enabled.rs` carries a measured line for `Font::PFM`. If that
    /// line is ever removed, this test says so rather than the tags quietly
    /// vanishing from a corpus report.
    #[test]
    fn the_table_is_allowlisted_and_that_is_what_emits_the_header() {
        let table = find_table("Font", "PFM").expect("generated Font::PFM table");
        assert!(
            table.enabled(),
            "Font::PFM must stay on the Step 28 allowlist; \
             src/parsers/font/pfm.rs emits its 24 header tags only when it is"
        );
        assert_eq!(table.fields.len(), 24);
    }
}
