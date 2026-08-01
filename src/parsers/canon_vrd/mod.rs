//! Canon VRD (Digital Photo Professional "recipe") trailer parser
//! (`Image::ExifTool::CanonVRD`)
//!
//! Canon DPP stores its non-destructive edit recipe as a *trailer*: a block
//! appended after the end of the image, rather than inside an APP segment. The
//! same block shape is written to JPEG, TIFF, CRW and CR2 files, and stands
//! alone as a `.VRD` / `.DR4` file, so this module is deliberately
//! format-agnostic -- it takes the whole file as bytes and finds the trailer
//! itself.
//!
//! Layout (CanonVRD.pm:2020-2056). A 0x1c-byte header and a 0x40-byte footer
//! each begin with `CANON OPTIONAL DATA\0`; each also carries an int32u giving
//! the size of the data between them, at byte 0x18 of the header and byte 0x14
//! of the footer. Everything is big-endian (`SetByteOrder('MM')`,
//! CanonVRD.pm:2148). Between header and footer sit typed blocks, each an
//! int32u type followed by an int32u length.
//!
//! Only the `EditData` block (0xffff00f4) is decoded here, and within it only
//! the `VRD1` section -- the fixed 0x272-byte version 1 record whose 43 tags
//! are `%CanonVRD::Ver1`. The blocks this module deliberately skips are:
//!
//! * 0xffff00f5 `IHLData`   -- embedded TIFF/EXIF plus preview JPEGs
//! * 0xffff00f6 `XMP`       -- an XMP packet
//! * 0xffff00f7 `Edit4Data` -- DPP version 4 "DR4" data, a separate format
//!
//! and, inside `EditData`, the `VRDStampTool` and `VRD2` sections (DPP 2.0 and
//! later). No file in the ExifTool sample corpus exercises those paths from a
//! JPEG, so decoding them here could not be verified against ExifTool and is
//! left undone rather than guessed.
//!
//! ExifTool reaches the trailer by peeling off whatever trailers follow it one
//! at a time and passing the accumulated offset to `ProcessCanonVRD`. oxidex
//! has no trailer chain -- see [`crate::parsers::trailer`] -- so this module
//! scans backwards from the end of the file for a footer whose declared size
//! lands exactly on a matching header. That two-point check matters: in
//! `ExifTool.jpg` the VRD trailer is not last -- PhotoMechanic, MIE, Samsung
//! and Vivo trailers all follow it.

mod ver1_table;

use crate::core::formatters::numeric_precision::{perl_g, perl_number};
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use ver1_table::VER1;

/// Opens both the header and the footer (CanonVRD.pm:62-63).
const SIGNATURE: &[u8; 20] = b"CANON OPTIONAL DATA\0";

/// Bytes of header preceding the first block.
const HEADER_LEN: usize = 0x1c;

/// Bytes of footer following the last block.
const FOOTER_LEN: usize = 0x40;

/// `$dirLen = unpack('N',$1) + 0x5c` -- the data size plus header and footer.
const OVERHEAD: usize = HEADER_LEN + FOOTER_LEN;

/// Offset of the int32u data size within the footer.
const FOOTER_SIZE_OFFSET: usize = 0x14;

/// `%CanonVRD::Main` block holding the DPP 1.x-3.x edit record.
const BLOCK_EDIT_DATA: u32 = 0xffff00f4;

/// `%CanonVRD::Edit` index 0: `VRD1`, `Size => 0x272`.
const VRD1_SIZE: usize = 0x272;

/// Storage formats `%Ver1` entries use, all read big-endian.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    Int8u,
    Int8s,
    Int16u,
    Int16s,
    Int32s,
    Float,
}

impl Format {
    /// Bytes one element occupies.
    fn width(self) -> usize {
        match self {
            Format::Int8u | Format::Int8s => 1,
            Format::Int16u | Format::Int16s => 2,
            Format::Int32s | Format::Float => 4,
        }
    }
}

/// The PrintConv of a `%Ver1` entry.
pub(super) enum Conv {
    /// Value passes through as ExifTool's raw string.
    None,
    /// A PrintConv lookup table.
    Map(&'static [(i64, &'static str)]),
    /// `$val =~ s/^(\d)(\d*)(\d)$/$1.$2.$3/` -- 100 becomes "1.0.0".
    VrdVersion,
    /// `CanonVRD::ToneCurvePrint($val)`.
    ToneCurve,
    /// `sprintf("%.2f",$val)`.
    Sprintf2f,
    /// `sprintf("%.7g",$val)`.
    Sprintf7g,
}

/// One entry of `%Image::ExifTool::CanonVRD::Ver1`.
pub(super) struct Ver1Entry {
    /// Byte offset within the VRD1 record.
    pub offset: usize,
    pub name: &'static str,
    pub format: Format,
    /// Elements, i.e. the `[n]` of a Perl format like `int16u[21]`.
    pub count: usize,
    pub conv: Conv,
    /// ValueConv divisor applied before the PrintConv, when there is one.
    pub scale: Option<f64>,
}

/// A single decoded element, before any conversion.
#[derive(Clone, Copy)]
enum Scalar {
    Int(i64),
    Float(f64),
}

impl Scalar {
    fn as_f64(self) -> f64 {
        match self {
            Scalar::Int(v) => v as f64,
            Scalar::Float(v) => v,
        }
    }

    /// How ExifTool stringifies the element with no PrintConv in play.
    fn render(self) -> String {
        match self {
            Scalar::Int(v) => v.to_string(),
            Scalar::Float(v) => perl_number(v),
        }
    }
}

/// Extracts CanonVRD trailer tags from a whole file.
///
/// # Arguments
///
/// * `file` - The complete file contents
///
/// # Returns
///
/// A metadata map keyed `CanonVRD:<Name>`; empty when the file carries no
/// CanonVRD trailer.
pub fn parse_canon_vrd_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    let Some(trailer) = find_trailer(file) else {
        return metadata;
    };
    for (block_type, block) in blocks(trailer) {
        if block_type == BLOCK_EDIT_DATA {
            parse_edit_data(block, &mut metadata);
        }
    }
    metadata
}

/// Reads a `.VRD` file, the standalone form of the same record.
///
/// DPP writes the identical `CANON OPTIONAL DATA\0` block either appended to an
/// image or as a file of its own (`ExifTool.pm:1039` gives the magic number for
/// the standalone form), so this is `parse_canon_vrd_trailer` over the whole
/// file. `find_trailer` already searches from the end inwards and validates the
/// header against the size the footer declares, which a file that *is* the
/// record satisfies at offset 0.
///
/// The `File:` identity tags come from `filetype`'s tables rather than literals
/// so there is one source for them, and they are needed here because a format
/// this dispatcher recognises never reaches `add_identity_tags`.
pub fn parse_vrd_file(reader: &dyn FileReader) -> Result<MetadataMap> {
    let file = reader.read(0, reader.size() as usize)?;
    let mut metadata = parse_canon_vrd_trailer(file);
    if metadata.is_empty() {
        return Err(ExifToolError::parse_error("No valid CanonVRD record found"));
    }

    if let Some(id) = crate::filetype::identify_by_extension("vrd") {
        metadata.insert("File:FileType", TagValue::new_string(id.file_type));
        metadata.insert(
            "File:FileTypeExtension",
            TagValue::new_string(id.extension.as_ref()),
        );
        if let Some(mime) = id.mime_type {
            metadata.insert("File:MIMEType", TagValue::new_string(mime));
        }
    }

    Ok(metadata)
}

/// Finds the outermost valid CanonVRD trailer, header through footer.
///
/// Mirrors the validation in CanonVRD.pm:2038-2053: read the footer, take the
/// size it declares, and require the header to sit exactly that far back and
/// carry the same signature.
fn find_trailer(file: &[u8]) -> Option<&[u8]> {
    // The footer opens with the signature and runs 0x40 bytes to the end of
    // the trailer.
    crate::parsers::trailer::find_last(file, OVERHEAD, SIGNATURE, FOOTER_LEN, |file, end| {
        let footer = &file[end - FOOTER_LEN..end];
        let size = be_u32(footer, FOOTER_SIZE_OFFSET)? as usize;
        // `$dirLen < 0x80000000 and $raf->Seek(-$dirLen, 1)`
        let dir_len = size.checked_add(OVERHEAD)?;
        if dir_len > end {
            return None;
        }
        let trailer = &file[end - dir_len..end];
        trailer.starts_with(SIGNATURE).then_some(trailer)
    })
}

/// Walks the typed blocks between the header and the footer
/// (CanonVRD.pm:2151-2166), yielding each block's type and body.
fn blocks(trailer: &[u8]) -> Vec<(u32, &[u8])> {
    let mut out = Vec::new();
    // `my $end = $dirLen - 0x40` -- the last block ends where the footer begins.
    let end = trailer.len() - FOOTER_LEN;
    let mut pos = HEADER_LEN;
    while pos + 8 <= end {
        let (Some(block_type), Some(block_len)) = (be_u32(trailer, pos), be_u32(trailer, pos + 4))
        else {
            break;
        };
        pos += 8;
        let block_len = block_len as usize;
        // "Possibly corrupt CanonVRD block"
        if block_len > end - pos {
            break;
        }
        out.push((block_type, &trailer[pos..pos + block_len]));
        pos += block_len;
    }
    out
}

/// `ProcessEditData` (CanonVRD.pm:1530).
///
/// The edit data is a sequence of length-prefixed records, but only record 0
/// carries tags (`next if $recNum`, CanonVRD.pm:1579), so the later records are
/// not walked. Record 0 is then divided into the sections of `%CanonVRD::Edit`,
/// of which the first is the fixed-size `VRD1`.
fn parse_edit_data(block: &[u8], metadata: &mut MetadataMap) {
    let Some(rec_len) = be_u32(block, 0).map(|n| n as usize) else {
        return;
    };
    let Some(record) = block.get(4..4 + rec_len) else {
        return;
    };
    // `%CanonVRD::Edit` index 0: VRD1, Size => 0x272. `$subLen > $maxLen and
    // $subLen = $maxLen` truncates it against a short record.
    let vrd1 = &record[..VRD1_SIZE.min(record.len())];
    parse_ver1(vrd1, metadata);
}

/// `%CanonVRD::Ver1` read as `ProcessBinaryData` would (big-endian).
fn parse_ver1(record: &[u8], metadata: &mut MetadataMap) {
    for entry in VER1 {
        let width = entry.format.width();
        let Some(bytes) = record.get(entry.offset..entry.offset + width * entry.count) else {
            continue;
        };
        let values: Vec<Scalar> = bytes
            .chunks_exact(width)
            .map(|chunk| read_scalar(chunk, entry.format))
            .collect();
        if let Some(value) = convert(entry, &values) {
            metadata.insert(format!("CanonVRD:{}", entry.name), value);
        }
    }
}

/// Decodes one big-endian element.
fn read_scalar(chunk: &[u8], format: Format) -> Scalar {
    match format {
        Format::Int8u => Scalar::Int(chunk[0] as i64),
        Format::Int8s => Scalar::Int(chunk[0] as i8 as i64),
        Format::Int16u => Scalar::Int(u16::from_be_bytes([chunk[0], chunk[1]]) as i64),
        Format::Int16s => Scalar::Int(i16::from_be_bytes([chunk[0], chunk[1]]) as i64),
        Format::Int32s => {
            Scalar::Int(i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as i64)
        }
        Format::Float => {
            Scalar::Float(f32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64)
        }
    }
}

/// Applies an entry's ValueConv and PrintConv, as ExifTool prints them.
fn convert(entry: &Ver1Entry, values: &[Scalar]) -> Option<TagValue> {
    let first = *values.first()?;
    // ValueConv, which in %Ver1 is only ever a divisor.
    let scaled = entry.scale.map(|divisor| first.as_f64() / divisor);
    // The raw value as ExifTool assembles it: elements joined with spaces.
    let raw = || {
        values
            .iter()
            .map(|v| v.render())
            .collect::<Vec<_>>()
            .join(" ")
    };

    let printed = match entry.conv {
        Conv::None => {
            // A lone plain integer stays typed; anything else is a string.
            return Some(match (values, scaled) {
                ([Scalar::Int(v)], None) => TagValue::Integer(*v),
                _ => TagValue::String(scaled.map_or_else(raw, perl_number)),
            });
        }
        Conv::Map(table) => {
            let Scalar::Int(v) = first else { return None };
            table
                .iter()
                .find(|(key, _)| *key == v)
                .map(|(_, label)| (*label).to_string())
                // ExifTool's fallback for a value the table does not list.
                .unwrap_or_else(|| format!("Unknown ({})", v))
        }
        Conv::VrdVersion => vrd_version(&raw()),
        Conv::ToneCurve => tone_curve_print(values),
        Conv::Sprintf2f => format!("{:.2}", scaled.unwrap_or_else(|| first.as_f64())),
        Conv::Sprintf7g => perl_g(scaled.unwrap_or_else(|| first.as_f64()), 7),
    };
    Some(TagValue::String(printed))
}

/// `$val =~ s/^(\d)(\d*)(\d)$/$1.$2.$3/` (CanonVRD.pm:204).
///
/// The version is stored as a plain integer, so 100 prints as "1.0.0". A value
/// the pattern cannot match -- a single digit, or a negative -- passes through.
fn vrd_version(val: &str) -> String {
    if val.len() < 2 || !val.bytes().all(|b| b.is_ascii_digit()) {
        return val.to_string();
    }
    format!(
        "{}.{}.{}",
        &val[..1],
        &val[1..val.len() - 1],
        &val[val.len() - 1..]
    )
}

/// `CanonVRD::ToneCurvePrint` (CanonVRD.pm:1499).
///
/// A curve is 21 int16u: a point count followed by up to 10 (x,y) pairs. A
/// count outside 2..=10, or a value that is not 21 elements long, prints raw.
fn tone_curve_print(values: &[Scalar]) -> String {
    let raw = || {
        values
            .iter()
            .map(|v| v.render())
            .collect::<Vec<_>>()
            .join(" ")
    };
    if values.len() != 21 {
        return raw();
    }
    let Scalar::Int(count) = values[0] else {
        return raw();
    };
    if !(2..=10).contains(&count) {
        return raw();
    }
    (0..count as usize)
        .map(|i| {
            format!(
                "({},{})",
                values[1 + i * 2].render(),
                values[2 + i * 2].render()
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads a big-endian int32u, or `None` if it does not fit.
fn be_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wraps `body` in the CanonVRD header and footer, laid out as the real
    /// ones in combined-samples/ExifTool.jpg are: the header carries the data
    /// size at byte 0x18 and the footer carries it at byte 0x14, and the
    /// footer ends with an EOI marker (`$blankFooter`, CanonVRD.pm:63).
    fn trailer(body: &[u8]) -> Vec<u8> {
        let size = (body.len() as u32).to_be_bytes();
        let mut out = Vec::new();
        // Header: signature, 4 bytes ExifTool does not name, then the size.
        out.extend_from_slice(SIGNATURE);
        out.extend_from_slice(&[0, 1, 0, 0]);
        out.extend_from_slice(&size);
        assert_eq!(out.len(), HEADER_LEN);

        out.extend_from_slice(body);

        // Footer: signature, the size at 0x14, zero padding, then FFD9.
        let footer_start = out.len();
        out.extend_from_slice(SIGNATURE);
        out.extend_from_slice(&size);
        assert_eq!(out.len() - footer_start, FOOTER_SIZE_OFFSET + 4);
        out.resize(footer_start + FOOTER_LEN - 2, 0);
        out.extend_from_slice(&[0xff, 0xd9]);
        assert_eq!(out.len() - footer_start, FOOTER_LEN);
        out
    }

    /// Wraps a VRD1 record in an EditData block and its record header.
    fn edit_block(vrd1: &[u8]) -> Vec<u8> {
        let mut record = (vrd1.len() as u32).to_be_bytes().to_vec();
        record.extend_from_slice(vrd1);
        let mut out = BLOCK_EDIT_DATA.to_be_bytes().to_vec();
        out.extend_from_slice(&(record.len() as u32).to_be_bytes());
        out.extend_from_slice(&record);
        out
    }

    /// The VRD1 record of combined-samples/ExifTool.jpg, byte for byte.
    fn exiftool_jpg_vrd1() -> Vec<u8> {
        let mut r = vec![0u8; VRD1_SIZE];
        let mut put16 = |offset: usize, value: u16| {
            r[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
        };
        put16(0x002, 100); // VRDVersion
        put16(0x018, 31); // WhiteBalanceAdj -> Shot Settings
        put16(0x01a, 5600); // WBAdjColorTemp
        put16(0x02e, 0); // RawColorAdj -> Shot Settings
        put16(0x07c, 4095); // DynamicRangeMax
        put16(0x116, 100); // SaturationAdj
        put16(0x26e, 1); // Rotation -> 90
        // Every curve is the identity: element 0 is the point count, then the
        // pairs, so 2 / (0,0) / (255,255) occupies elements 0 through 4.
        for curve in [0x126, 0x160, 0x19a, 0x1d4, 0x20e] {
            put16(curve, 2); // point count
            put16(curve + 6, 255); // x of the second point
            put16(curve + 8, 255); // y of the second point
        }
        // Limits are four plain int16u printed raw: "255 0 255 0".
        for limits in [0x150, 0x18a, 0x1c4, 0x1fe, 0x238] {
            put16(limits, 255);
            put16(limits + 4, 255);
        }
        r
    }

    fn parse_sample() -> MetadataMap {
        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        file.extend_from_slice(&trailer(&edit_block(&exiftool_jpg_vrd1())));
        parse_canon_vrd_trailer(&file)
    }

    /// Every assertion is `exiftool -a -G1 -s combined-samples/ExifTool.jpg`
    /// (ExifTool 13.55), byte for byte.
    #[test]
    fn test_exiftool_jpg_trailer_matches_exiftool() {
        let m = parse_sample();
        assert_eq!(m.get_string("CanonVRD:VRDVersion"), Some("1.0.0"));
        assert_eq!(m.get_string("CanonVRD:WBAdjRGGBLevels"), Some("0 0 0 0"));
        assert_eq!(
            m.get_string("CanonVRD:WhiteBalanceAdj"),
            Some("Shot Settings")
        );
        assert_eq!(m.get_integer("CanonVRD:WBAdjColorTemp"), Some(5600));
        assert_eq!(m.get_string("CanonVRD:WBFineTuneActive"), Some("No"));
        assert_eq!(m.get_integer("CanonVRD:WBFineTuneSaturation"), Some(0));
        assert_eq!(m.get_integer("CanonVRD:WBFineTuneTone"), Some(0));
        assert_eq!(m.get_string("CanonVRD:RawColorAdj"), Some("Shot Settings"));
        assert_eq!(m.get_integer("CanonVRD:RawCustomSaturation"), Some(0));
        assert_eq!(m.get_integer("CanonVRD:RawCustomTone"), Some(0));
        assert_eq!(m.get_string("CanonVRD:RawBrightnessAdj"), Some("0.00"));
        assert_eq!(
            m.get_string("CanonVRD:ToneCurveProperty"),
            Some("Shot Settings")
        );
        assert_eq!(m.get_integer("CanonVRD:DynamicRangeMin"), Some(0));
        assert_eq!(m.get_integer("CanonVRD:DynamicRangeMax"), Some(4095));
        assert_eq!(m.get_string("CanonVRD:ToneCurveActive"), Some("No"));
        assert_eq!(m.get_string("CanonVRD:ToneCurveMode"), Some("RGB"));
        assert_eq!(m.get_integer("CanonVRD:BrightnessAdj"), Some(0));
        assert_eq!(m.get_integer("CanonVRD:ContrastAdj"), Some(0));
        assert_eq!(m.get_integer("CanonVRD:SaturationAdj"), Some(100));
        assert_eq!(m.get_integer("CanonVRD:ColorToneAdj"), Some(0));
        assert_eq!(
            m.get_string("CanonVRD:ToneCurveInterpolation"),
            Some("Curve")
        );
        assert_eq!(m.get_string("CanonVRD:CropActive"), Some("No"));
        assert_eq!(m.get_integer("CanonVRD:CropLeft"), Some(0));
        assert_eq!(m.get_integer("CanonVRD:CropTop"), Some(0));
        assert_eq!(m.get_integer("CanonVRD:CropWidth"), Some(0));
        assert_eq!(m.get_integer("CanonVRD:CropHeight"), Some(0));
        assert_eq!(m.get_integer("CanonVRD:SharpnessAdj"), Some(0));
        assert_eq!(m.get_string("CanonVRD:CropAspectRatio"), Some("Free"));
        assert_eq!(m.get_string("CanonVRD:ConstrainedCropWidth"), Some("0"));
        assert_eq!(m.get_string("CanonVRD:ConstrainedCropHeight"), Some("0"));
        assert_eq!(m.get_string("CanonVRD:CheckMark"), Some("Clear"));
        assert_eq!(m.get_string("CanonVRD:Rotation"), Some("90"));
        assert_eq!(m.get_string("CanonVRD:WorkColorSpace"), Some("sRGB"));
        // All five curves print through ToneCurvePrint, and their limits raw.
        for curve in [
            "LuminanceCurvePoints",
            "RedCurvePoints",
            "GreenCurvePoints",
            "BlueCurvePoints",
            "RGBCurvePoints",
        ] {
            assert_eq!(
                m.get_string(&format!("CanonVRD:{curve}")),
                Some("(0,0) (255,255)"),
                "{curve}"
            );
        }
        for limits in [
            "LuminanceCurveLimits",
            "RedCurveLimits",
            "GreenCurveLimits",
            "BlueCurveLimits",
            "RGBCurveLimits",
        ] {
            assert_eq!(
                m.get_string(&format!("CanonVRD:{limits}")),
                Some("255 0 255 0"),
                "{limits}"
            );
        }
        // ExifTool reports exactly these 43 tags for this file.
        assert_eq!(m.len(), 43);
    }

    #[test]
    fn test_trailer_is_found_when_other_trailers_follow_it() {
        // ExifTool.jpg chains a FotoStation trailer and an Android one after
        // the VRD trailer, so the footer is not the end of the file.
        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        file.extend_from_slice(&trailer(&edit_block(&exiftool_jpg_vrd1())));
        file.extend_from_slice(b"whatever trailing junk follows");
        assert_eq!(parse_canon_vrd_trailer(&file).len(), 43);
    }

    #[test]
    fn test_file_without_trailer_yields_nothing() {
        assert!(parse_canon_vrd_trailer(b"\xff\xd8\xff\xd9 no trailer here").is_empty());
        assert!(parse_canon_vrd_trailer(b"").is_empty());
    }

    struct SliceReader(Vec<u8>);

    impl FileReader for SliceReader {
        fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = start
                .checked_add(length)
                .filter(|e| *e <= self.0.len())
                .ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "past end of file")
                })?;
            Ok(&self.0[start..end])
        }

        fn size(&self) -> u64 {
            self.0.len() as u64
        }
    }

    /// A `.VRD` file is the same record with nothing wrapped around it, so the
    /// standalone entry point must read the same 43 tags the trailer form does
    /// and label the file the way ExifTool's own tables do.
    #[test]
    fn test_standalone_vrd_file_reads_the_same_record() {
        let reader = SliceReader(trailer(&edit_block(&exiftool_jpg_vrd1())));
        let m = parse_vrd_file(&reader).expect("standalone VRD parses");

        assert_eq!(m.get_string("CanonVRD:VRDVersion"), Some("1.0.0"));
        assert_eq!(m.get_string("CanonVRD:WorkColorSpace"), Some("sRGB"));
        assert_eq!(
            m.get_string("CanonVRD:RGBCurvePoints"),
            Some("(0,0) (255,255)")
        );
        assert_eq!(m.get_string("File:FileType"), Some("VRD"));
        assert_eq!(m.get_string("File:FileTypeExtension"), Some("vrd"));
        assert_eq!(
            m.get_string("File:MIMEType"),
            Some("application/octet-stream")
        );
        // 43 record tags plus the three File: identity tags.
        assert_eq!(m.len(), 46);
    }

    #[test]
    fn test_standalone_reader_without_a_record_is_an_error() {
        let reader = SliceReader(b"CANON OPTIONAL DATA\0 but nothing valid after".to_vec());
        assert!(parse_vrd_file(&reader).is_err());
    }

    #[test]
    fn test_signature_without_a_matching_header_is_ignored() {
        // The footer magic alone is not enough: the size it declares must land
        // on a header carrying the same signature.
        let mut file = vec![0u8; 0x200];
        let footer = file.len() - FOOTER_LEN;
        file[footer..footer + SIGNATURE.len()].copy_from_slice(SIGNATURE);
        file[footer + FOOTER_SIZE_OFFSET..footer + FOOTER_SIZE_OFFSET + 4]
            .copy_from_slice(&0x40u32.to_be_bytes());
        assert!(parse_canon_vrd_trailer(&file).is_empty());

        // A size larger than everything before the footer is rejected too.
        file[footer + FOOTER_SIZE_OFFSET..footer + FOOTER_SIZE_OFFSET + 4]
            .copy_from_slice(&0xffff_ffffu32.to_be_bytes());
        assert!(parse_canon_vrd_trailer(&file).is_empty());
    }

    #[test]
    fn test_unknown_printconv_value_reports_unknown() {
        let mut vrd1 = exiftool_jpg_vrd1();
        // 42 is not a %Ver1 WhiteBalanceAdj key.
        vrd1[0x018..0x01a].copy_from_slice(&42u16.to_be_bytes());
        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        file.extend_from_slice(&trailer(&edit_block(&vrd1)));
        assert_eq!(
            parse_canon_vrd_trailer(&file).get_string("CanonVRD:WhiteBalanceAdj"),
            Some("Unknown (42)")
        );
    }

    #[test]
    fn test_value_conversions() {
        let mut vrd1 = exiftool_jpg_vrd1();
        // RawBrightnessAdj: int32s / 6000, printed "%.2f"
        vrd1[0x038..0x03c].copy_from_slice(&(-3000i32).to_be_bytes());
        // ConstrainedCropWidth: float, printed "%.7g"
        vrd1[0x262..0x266].copy_from_slice(&3456.75f32.to_be_bytes());
        // Signed 8-bit adjustments
        vrd1[0x114] = 0xfb; // BrightnessAdj = -5
        vrd1[0x115] = 0x03; // ContrastAdj = 3
        // SaturationAdj is int16s
        vrd1[0x116..0x118].copy_from_slice(&(-4i16).to_be_bytes());
        // A four-point tone curve: the count, then four (x,y) pairs.
        for (i, v) in [4u16, 0, 0, 10, 20, 30, 40, 50, 60].iter().enumerate() {
            vrd1[0x126 + i * 2..0x128 + i * 2].copy_from_slice(&v.to_be_bytes());
        }
        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        file.extend_from_slice(&trailer(&edit_block(&vrd1)));
        let m = parse_canon_vrd_trailer(&file);

        assert_eq!(m.get_string("CanonVRD:RawBrightnessAdj"), Some("-0.50"));
        assert_eq!(
            m.get_string("CanonVRD:ConstrainedCropWidth"),
            Some("3456.75")
        );
        assert_eq!(m.get_integer("CanonVRD:BrightnessAdj"), Some(-5));
        assert_eq!(m.get_integer("CanonVRD:ContrastAdj"), Some(3));
        assert_eq!(m.get_integer("CanonVRD:SaturationAdj"), Some(-4));
        assert_eq!(
            m.get_string("CanonVRD:LuminanceCurvePoints"),
            Some("(0,0) (10,20) (30,40) (50,60)")
        );
    }

    #[test]
    fn test_tone_curve_with_out_of_range_count_prints_raw() {
        let mut vrd1 = exiftool_jpg_vrd1();
        // ToneCurvePrint returns $val unchanged unless 2 <= $n <= 10.
        vrd1[0x126..0x128].copy_from_slice(&11u16.to_be_bytes());
        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        file.extend_from_slice(&trailer(&edit_block(&vrd1)));
        assert_eq!(
            parse_canon_vrd_trailer(&file).get_string("CanonVRD:LuminanceCurvePoints"),
            Some("11 0 0 255 255 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0")
        );
    }

    #[test]
    fn test_truncated_record_drops_later_tags() {
        let mut file = b"\xff\xd8\xff\xd9".to_vec();
        let short = exiftool_jpg_vrd1()[..0x100].to_vec();
        file.extend_from_slice(&trailer(&edit_block(&short)));
        let m = parse_canon_vrd_trailer(&file);
        assert_eq!(m.get_string("CanonVRD:VRDVersion"), Some("1.0.0"));
        assert!(m.get("CanonVRD:WorkColorSpace").is_none());
    }

    #[test]
    fn test_vrd_version_formatting() {
        assert_eq!(vrd_version("100"), "1.0.0");
        assert_eq!(vrd_version("3410"), "3.41.0");
        // Too short for the pattern, so unchanged.
        assert_eq!(vrd_version("5"), "5");
    }
}
