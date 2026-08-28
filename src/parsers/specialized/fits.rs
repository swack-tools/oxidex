//! FITS (Flexible Image Transport System) parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::{ByteOrder, EndianReader};
use crate::parsers::specialized::dicom_dict;

mod tables;

use tables::FITS_TAG_NAMES;

const FITS_RECORD_SIZE: usize = 80;
const FITS_BLOCK_SIZE: usize = 2880;

/// Parser for FITS (Flexible Image Transport System) files
///
/// Extracts metadata from FITS astronomical data files used for scientific imaging.
pub struct FITSParser;

impl FITSParser {
    /// Verifies the FITS file signature against ExifTool's magic number
    ///
    /// Asks the magic table rather than restating the pattern: the six-byte
    /// literal `b"SIMPLE"` this replaced matched any text opening those six
    /// letters, e.g. "SIMPLE ANSWER:". ExifTool's actual magic is the full
    /// 30-byte keyword record `^SIMPLE  = {20}T`, which `matches_magic`
    /// tests and which `detect_format` now asks the same way.
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 6 {
            return Ok(false);
        }
        let probe_len = reader.size().min(1024) as usize;
        Ok(crate::filetype::matches_magic(
            "FITS",
            reader.read(0, probe_len)?,
        ))
    }

    /// Parses a FITS header record (80-character fixed-width)
    /// Returns (keyword, value, comment) tuple
    fn parse_record(record: &[u8]) -> Option<(String, String, Option<String>)> {
        if record.len() != FITS_RECORD_SIZE {
            return None;
        }

        let keyword = String::from_utf8_lossy(&record[..8]).trim_end().to_string();

        // Check for END keyword
        if keyword == "END" {
            return Some(("END".to_string(), String::new(), None));
        }

        // Check for HISTORY or COMMENT records (no '=')
        if keyword == "HISTORY" || keyword == "COMMENT" {
            let content = String::from_utf8_lossy(&record[8..]).trim().to_string();
            return Some((keyword, content, None));
        }

        // Like ExifTool's ProcessFITS, accept a value only when the equals sign
        // occupies the standard columns. A slash begins a comment only outside
        // quotes: dates and identifiers routinely contain literal slashes.
        if &record[8..10] != b"= " {
            return None;
        }
        let value_part = String::from_utf8_lossy(&record[10..]);
        let value_part = value_part.as_ref();
        if let Some(quoted) = value_part.strip_prefix('\'') {
            let mut value = String::new();
            let mut chars = quoted.chars().peekable();
            let mut closed = false;
            while let Some(ch) = chars.next() {
                if ch == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                        value.push('\'');
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    value.push(ch);
                }
            }
            if closed {
                // FITS pads quoted strings on the right. Leading spaces are
                // data (DATASUM in ExifTool's fixture relies on this).
                return Some((keyword, value.trim_end().to_string(), None));
            }
        }

        let (value, comment) = if let Some(slash_pos) = value_part.find('/') {
            (
                value_part[..slash_pos].trim(),
                Some(value_part[slash_pos + 1..].trim().to_string()),
            )
        } else {
            (value_part.trim(), None)
        };
        if value.is_empty() {
            None
        } else {
            // FITS permits Fortran D exponents; ExifTool renders both D and E
            // using an `e` before passing the value on.
            let normalized_number = value.replace(['D', 'E'], "e");
            let value = if normalized_number.parse::<f64>().is_ok() {
                normalized_number
            } else {
                value.to_string()
            };
            Some((keyword, value, comment))
        }
    }

    /// Resolve FITS keywords the same way ExifTool does.
    ///
    /// Standard names are generated from `Image::ExifTool::FITS::Main`. Any
    /// other valid keyword is lowercased, title-cased, and has underscores
    /// removed while capitalizing the following character.
    fn tag_name(keyword: &str) -> String {
        if let Some((_, name)) = FITS_TAG_NAMES
            .iter()
            .find(|(candidate, _)| *candidate == keyword)
        {
            return (*name).to_string();
        }

        let mut name = String::with_capacity(keyword.len());
        let mut capitalize = true;
        for ch in keyword.chars() {
            if ch == '_' {
                capitalize = true;
            } else if capitalize {
                name.push(ch.to_ascii_uppercase());
                capitalize = false;
            } else {
                name.push(ch.to_ascii_lowercase());
            }
        }
        name
    }

    fn tag_value(value: String) -> TagValue {
        if let Ok(integer) = value.parse::<i64>() {
            TagValue::Integer(integer)
        } else if let Ok(float) = value.parse::<f64>() {
            TagValue::Float(float)
        } else {
            TagValue::String(value)
        }
    }

    /// Parses FITS header and extracts all metadata
    fn parse_header(reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let mut offset = 0usize;
        let mut naxis_values: Vec<i64> = Vec::new();

        // Read header blocks until END keyword
        loop {
            // Read one FITS block (2880 bytes)
            let block_size = FITS_BLOCK_SIZE.min(reader.size() as usize - offset);
            if block_size < FITS_RECORD_SIZE {
                break;
            }

            let block = reader.read(offset as u64, block_size)?;

            // Process 80-byte records
            for chunk in block.chunks(FITS_RECORD_SIZE) {
                if chunk.len() != FITS_RECORD_SIZE {
                    break;
                }

                if let Some((keyword, value, comment)) = Self::parse_record(chunk) {
                    // Comments after a card value describe that card; ExifTool
                    // does not emit them as separate `KeywordComment` tags.
                    let _ = comment;

                    match keyword.as_str() {
                        "END" => {
                            // Process collected data
                            Self::finalize_metadata(&mut metadata, &naxis_values);
                            return Ok(metadata);
                        }
                        // ProcessFITS consumes SIMPLE while validating the
                        // signature, so it is not reported as metadata.
                        "SIMPLE" => {}
                        "HISTORY" | "COMMENT" => {
                            // FITS.pm's Main table has `GROUPS => { 2 => 'Image'
                            // }` and no family-0/1 override, so every tag here
                            // -- including repeated COMMENT/HISTORY cards, which
                            // FITS legitimately allows many of per file -- is
                            // family-1 `FITS`. The bare (unprefixed) key this
                            // used to insert under got family-1 `""` from
                            // `TagOccurrence::from_insert_shim`
                            // (`src/core/tag_occurrence.rs`), which prints as
                            // `-G1`'s empty `[]` bracket: invisible to any
                            // instrument that expects a `[group]name: value`
                            // shape, including this repo's own
                            // `duplicate_loss_scan.py` (its `LINE_RE` requires
                            // one-or-more chars inside the brackets), which is
                            // why 6 real `-a` occurrences of Comment scored as
                            // 0 rather than PARTIAL/RETAINED. `insert()` itself
                            // already retains every occurrence (`TagSink::record`
                            // pushes each one); only the missing group prefix
                            // needed fixing here, not the retention mechanism.
                            metadata.insert(
                                format!("FITS:{}", Self::tag_name(&keyword)),
                                TagValue::String(value),
                            );
                        }
                        k if k.starts_with("NAXIS") && k.len() > 5 => {
                            if let Ok(axis_val) = value.parse::<i64>() {
                                metadata
                                    .insert(Self::tag_name(&keyword), TagValue::Integer(axis_val));
                                naxis_values.push(axis_val);
                            }
                        }
                        _ => {
                            if !value.is_empty() {
                                metadata.insert(Self::tag_name(&keyword), Self::tag_value(value));
                            }
                        }
                    }
                }
            }

            offset += FITS_BLOCK_SIZE;
            if offset >= reader.size() as usize {
                break;
            }
        }

        Self::finalize_metadata(&mut metadata, &naxis_values);
        Ok(metadata)
    }

    /// Finalizes metadata by calculating dimensions and other derived values
    fn finalize_metadata(metadata: &mut MetadataMap, naxis_values: &[i64]) {
        // Calculate image dimensions
        if naxis_values.len() >= 2 {
            let width = naxis_values[0];
            let height = naxis_values[1];

            metadata.insert("ImageWidth".to_string(), TagValue::Integer(width));
            metadata.insert("ImageHeight".to_string(), TagValue::Integer(height));

            if naxis_values.len() >= 3 {
                let depth = naxis_values[2];
                metadata.insert("ImageDepth".to_string(), TagValue::Integer(depth));
            }
        }
    }
}

impl FormatParser for FITSParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid FITS signature"));
        }

        // Identity is not this parser's to report, even correctly prefixed.
        // `add_identity_tags` resolves all three from the generated tables,
        // which carry FITS in full -- the `SIMPLE  = {20}T` magic number, the
        // `fits` extension row, and `("FITS", "image/fits")` -- and answer
        // exactly as these literals did, verified on the corpus FITS.fits.
        //
        // Being hardcoded rather than looked up is what made them a second
        // detector, and being already in the `File:` group is what put them out
        // of `normalize_identity_tags`' reach: it drops the *ungrouped* copies,
        // so this was the last place a parser could still outrank the tables.
        // `merge` gives format metadata precedence over the `File:` group, so
        // had the two ever drifted, this copy would have won silently.
        Self::parse_header(reader)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::FITS)
    }
}

/// Parses metadata from FITS files.
///
/// This is a convenience wrapper around FITSParser that provides a functional API.
pub fn parse_fits_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = FITSParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

const DICOM_MAGIC_OFFSET: usize = 128;
const DICOM_DATA_OFFSET: usize = 132;

#[derive(Clone, Copy)]
struct DicomEncoding {
    order: ByteOrder,
    explicit_vr: bool,
}

impl DicomEncoding {
    const EXPLICIT_LE: Self = Self {
        order: ByteOrder::Little,
        explicit_vr: true,
    };
    const IMPLICIT_LE: Self = Self {
        order: ByteOrder::Little,
        explicit_vr: false,
    };
    const EXPLICIT_BE: Self = Self {
        order: ByteOrder::Big,
        explicit_vr: true,
    };
}

/// Outcome of ExifTool's transfer-syntax dispatch (the `$transferSyntax`
/// block in the pinned DICOM.pm `ProcessDICOM`).
#[derive(Clone, Copy)]
enum DicomSyntax {
    Encoding(DicomEncoding),
    /// Deflated (1.2.840.10008.1.2.1.99) or unrecognized transfer syntaxes.
    /// ExifTool inflates the former with Compress::Zlib and warns-and-stops
    /// on the latter; we have no inflater wired here, so both stop the walk
    /// gracefully (an honest omission) instead of parsing raw compressed
    /// bytes as if they were elements.
    Unsupported,
}

struct DicomElement<'a> {
    group: u16,
    element: u16,
    /// The VR from the element header in explicit-VR syntax; `None` for
    /// implicit-VR syntax and for the FFFE item/delimiter tags, which carry
    /// no VR field in any syntax.
    vr: Option<[u8; 2]>,
    value: &'a [u8],
    next_offset: usize,
}

/// ExifTool 13.59's `%vr32`: the VRs framed with a 32-bit length (12-byte
/// header) in explicit VR syntax. The 2013+ DICOM standard also frames
/// OD/OL/OV/UC/UR that way, but the pinned oracle does not, and the oracle's
/// framing is authoritative here: diverging would desynchronize every
/// element that follows one of those VRs.
fn dicom_long_vr(vr: [u8; 2]) -> bool {
    matches!(&vr, b"OB" | b"OW" | b"OF" | b"SQ" | b"UT" | b"UN")
}

/// ExifTool's `%implicitVR`: item/delimiter tags that never carry a VR
/// field, even in explicit-VR syntax.
fn dicom_implicit_tag(group: u16, element: u16) -> bool {
    group == 0xFFFE && matches!(element, 0xE000 | 0xE00D | 0xE0DD)
}

fn parse_dicom_element<'a>(
    data: &'a [u8],
    offset: usize,
    encoding: DicomEncoding,
) -> Result<DicomElement<'a>> {
    let reader = EndianReader::new(data, encoding.order);
    let group = reader
        .u16_at(offset)
        .ok_or_else(|| ExifToolError::parse_error_at("truncated DICOM tag group", offset))?;
    let element = reader
        .u16_at(offset + 2)
        .ok_or_else(|| ExifToolError::parse_error_at("truncated DICOM tag element", offset))?;

    let (header_len, value_len, vr) = if !encoding.explicit_vr || dicom_implicit_tag(group, element)
    {
        (
            8usize,
            reader.u32_at(offset + 4).ok_or_else(|| {
                ExifToolError::parse_error_at("truncated DICOM value length", offset)
            })?,
            None,
        )
    } else {
        let vr_bytes = data.get(offset + 4..offset + 6).ok_or_else(|| {
            ExifToolError::parse_error_at("truncated DICOM value representation", offset)
        })?;
        let vr = [vr_bytes[0], vr_bytes[1]];
        // ExifTool stops the walk on a VR that is not two uppercase letters
        // (`last unless $vr =~ /^[A-Z]{2}$/`).
        if !vr.iter().all(u8::is_ascii_uppercase) {
            return Err(ExifToolError::parse_error_at(
                "invalid DICOM value representation",
                offset,
            ));
        }
        if dicom_long_vr(vr) {
            let len = reader.u32_at(offset + 8).ok_or_else(|| {
                ExifToolError::parse_error_at("truncated DICOM value length", offset)
            })?;
            // ExifTool forces the length to 0 for SQ so the walk simply
            // continues into the sequence contents ("just recurse into
            // sequences"), extracting whatever elements it meets there.
            let len = if &vr == b"SQ" { 0 } else { len };
            (12usize, len, Some(vr))
        } else {
            (
                8usize,
                u32::from(reader.u16_at(offset + 6).ok_or_else(|| {
                    ExifToolError::parse_error_at("truncated DICOM value length", offset)
                })?),
                Some(vr),
            )
        }
    };

    // Undefined length: ExifTool reads no value (`$len = 0`) and keeps
    // walking -- the enclosed items are themselves element-framed, so the
    // walk stays in sync and the file is never failed outright.
    let value_len = if value_len == u32::MAX { 0 } else { value_len };
    let value_len = usize::try_from(value_len)
        .map_err(|_| ExifToolError::parse_error_at("DICOM value is too large", offset))?;
    let value_start = offset
        .checked_add(header_len)
        .ok_or_else(|| ExifToolError::parse_error_at("DICOM offset overflow", offset))?;
    let next_offset = value_start
        .checked_add(value_len)
        .ok_or_else(|| ExifToolError::parse_error_at("DICOM value length overflow", offset))?;
    let value = data
        .get(value_start..next_offset)
        .ok_or_else(|| ExifToolError::parse_error_at("DICOM value extends beyond file", offset))?;

    Ok(DicomElement {
        group,
        element,
        vr,
        value,
        next_offset,
    })
}

fn trim_trailing_spaces(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|&byte| byte != b' ')
        .map_or(0, |pos| pos + 1);
    &bytes[..end]
}

fn leading_space_count(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|&&byte| byte == b' ').count()
}

/// DA conversion: `s/^ *(\d{4})(\d{2})(\d{2})/$1:$2:$3/` (pinned DICOM.pm).
/// A prefix match, so multi-value dates ("20010316\\20010317") convert their
/// first value and space-padded dates convert too; anything without eight
/// leading digits passes through untouched.
fn dicom_date_bytes(bytes: &[u8]) -> Vec<u8> {
    let rest = &bytes[leading_space_count(bytes)..];
    if rest.len() >= 8 && rest[..8].iter().all(u8::is_ascii_digit) {
        let mut out = Vec::with_capacity(rest.len() + 2);
        out.extend_from_slice(&rest[..4]);
        out.push(b':');
        out.extend_from_slice(&rest[4..6]);
        out.push(b':');
        out.extend_from_slice(&rest[6..]);
        out
    } else {
        bytes.to_vec()
    }
}

/// TM conversion: `s/^ *(\d{2})(\d{2})(\d{2}[^ ]*)/$1:$2:$3/` (pinned
/// DICOM.pm). Six leading digits are required, so legal partial times like
/// "1434" are reported verbatim, exactly as ExifTool does.
fn dicom_time_bytes(bytes: &[u8]) -> Vec<u8> {
    let rest = &bytes[leading_space_count(bytes)..];
    if rest.len() >= 6 && rest[..6].iter().all(u8::is_ascii_digit) {
        let mut out = Vec::with_capacity(rest.len() + 2);
        out.extend_from_slice(&rest[..2]);
        out.push(b':');
        out.extend_from_slice(&rest[2..4]);
        out.push(b':');
        out.extend_from_slice(&rest[4..]);
        out
    } else {
        bytes.to_vec()
    }
}

/// DT conversion:
/// `s/^ *(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2}[^ ]*)/$1:$2:$3 $4:$5:$6/`.
fn dicom_datetime_bytes(bytes: &[u8]) -> Vec<u8> {
    let rest = &bytes[leading_space_count(bytes)..];
    if rest.len() >= 14 && rest[..14].iter().all(u8::is_ascii_digit) {
        let mut out = Vec::with_capacity(rest.len() + 5);
        out.extend_from_slice(&rest[..4]);
        out.push(b':');
        out.extend_from_slice(&rest[4..6]);
        out.push(b':');
        out.extend_from_slice(&rest[6..8]);
        out.push(b' ');
        out.extend_from_slice(&rest[8..10]);
        out.push(b':');
        out.extend_from_slice(&rest[10..12]);
        out.push(b':');
        out.extend_from_slice(&rest[12..]);
        out
    } else {
        bytes.to_vec()
    }
}

/// Applies ProcessDICOM's string-path conversions for the effective VR and
/// returns the converted BYTES. Kept as bytes (not `String`) because the
/// `Binary => 1` placeholder needs `length($$val)` of exactly these bytes
/// even when they are not valid UTF-8.
fn dicom_string_bytes(vr: [u8; 2], value: &[u8], order: ByteOrder) -> Option<Vec<u8>> {
    // `$buff =~ s/ $// unless $format or length($buff) & 0x01;` -- exactly
    // one trailing space (the even-length pad) is removed before any VR
    // rule. Format VRs never reach this function.
    let mut bytes = value;
    if bytes.len() % 2 == 0 {
        if let Some(stripped) = bytes.strip_suffix(b" ") {
            bytes = stripped;
        }
    }

    let converted: Vec<u8> = match &vr {
        b"DA" => dicom_date_bytes(bytes),
        b"TM" => dicom_time_bytes(bytes),
        b"DT" => dicom_datetime_bytes(bytes),
        // `$val =~ s/\0.*//s;` -- only UI truncates at a null byte.
        b"UI" => bytes
            .iter()
            .position(|&byte| byte == 0)
            .map_or_else(|| bytes.to_vec(), |pos| bytes[..pos].to_vec()),
        // A 4-byte AT value renders as a hex attribute-tag ID.
        b"AT" if bytes.len() == 4 => {
            let reader = EndianReader::new(bytes, order);
            let group = reader.u16_at(0)?;
            let element = reader.u16_at(2)?;
            format!("{group:04X},{element:04X}").into_bytes()
        }
        // `s/ +$//; s/^ +//` -- leading/trailing spaces not significant.
        b"AE" | b"CS" | b"DS" | b"IS" | b"LO" | b"PN" | b"SH" => {
            let trimmed = trim_trailing_spaces(bytes);
            trimmed[leading_space_count(trimmed)..].to_vec()
        }
        // `s/ +$//` -- trailing spaces not significant.
        b"LT" | b"ST" | b"UT" => trim_trailing_spaces(bytes).to_vec(),
        // Every other string VR keeps its bytes (after the pad trim above);
        // in particular trailing nulls are NOT stripped outside UI.
        _ => bytes.to_vec(),
    };
    Some(converted)
}

/// Converts a string element exactly as ProcessDICOM does for its effective
/// VR. Returns `None` when the resulting bytes are not valid UTF-8: the
/// oracle emits raw bytes (Perl strings are byte strings), which a Rust
/// `String` cannot hold losslessly, so the tag is omitted rather than
/// approximated with U+FFFD replacement characters.
fn dicom_string_value(vr: [u8; 2], value: &[u8], order: ByteOrder) -> Option<String> {
    String::from_utf8(dicom_string_bytes(vr, value, order)?).ok()
}

/// ExifTool's `%dicomFormat` integer formats, decoded exactly as
/// `ReadValue(\$buff, 0, $format, undef, $len)` does: count is
/// floor(len / size) -- stray trailing bytes are ignored, not an error --
/// and multiple values join with a single space.
///
/// The float members of `%dicomFormat` (FD => double, FL/OF => float) are
/// deliberately NOT here: their text form is Perl's NV stringification,
/// which this crate does not reproduce digit-for-digit, so float-valued
/// elements are omitted and counted rather than approximated (repo rule;
/// see `%dicomFormat` in the pinned DICOM.pm).
fn dicom_int_values(format: DicomIntFormat, value: &[u8], order: ByteOrder) -> String {
    let reader = EndianReader::new(value, order);
    let size = format.size();
    let count = value.len() / size;
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let at = index * size;
        let Some(text) = (match format {
            DicomIntFormat::Int8u => value.get(at).map(ToString::to_string),
            DicomIntFormat::Int16u => reader.u16_at(at).map(|v| v.to_string()),
            #[allow(clippy::cast_possible_wrap)]
            DicomIntFormat::Int16s => reader.u16_at(at).map(|v| (v as i16).to_string()),
            DicomIntFormat::Int32u => reader.u32_at(at).map(|v| v.to_string()),
            #[allow(clippy::cast_possible_wrap)]
            DicomIntFormat::Int32s => reader.u32_at(at).map(|v| (v as i32).to_string()),
        }) else {
            break;
        };
        values.push(text);
    }
    values.join(" ")
}

#[derive(Clone, Copy)]
enum DicomIntFormat {
    Int8u,
    Int16u,
    Int16s,
    Int32u,
    Int32s,
}

impl DicomIntFormat {
    const fn size(self) -> usize {
        match self {
            Self::Int8u => 1,
            Self::Int16u | Self::Int16s => 2,
            Self::Int32u | Self::Int32s => 4,
        }
    }
}

/// The integer members of ExifTool's `%dicomFormat` (pinned DICOM.pm):
/// OB => int8u, OW/US => int16u, SS => int16s, UL => int32u, SL => int32s.
/// FD/FL/OF (the float members) return `Float` so callers can omit them.
enum DicomFormat {
    Int(DicomIntFormat),
    Float,
    None,
}

fn dicom_format(vr: [u8; 2]) -> DicomFormat {
    match &vr {
        b"OB" => DicomFormat::Int(DicomIntFormat::Int8u),
        b"OW" | b"US" => DicomFormat::Int(DicomIntFormat::Int16u),
        b"SS" => DicomFormat::Int(DicomIntFormat::Int16s),
        b"UL" => DicomFormat::Int(DicomIntFormat::Int32u),
        b"SL" => DicomFormat::Int(DicomIntFormat::Int32s),
        b"FD" | b"FL" | b"OF" => DicomFormat::Float,
        _ => DicomFormat::None,
    }
}

/// Resolves the `%Image::ExifTool::DICOM::Main` entry for a tag, mirroring
/// ProcessDICOM's lookup: exact `'%.4X,%.4X'` key first, then the five
/// wildcard substitutions in order --
/// `s/^(..)../$1xx/`, `s/..$/xx/`, `s/.(.)$/x$1/`, `s/...(.)$/xxx$1/`,
/// `s/....$/xxxx/` -- each applied to the ORIGINAL tag string.
fn dicom_dict_entry(group: u16, element: u16) -> Option<&'static dicom_dict::DicomDictEntry> {
    let tag = format!("{group:04X},{element:04X}");
    if let Some(entry) = dicom_dict::dicom_main_entry(&tag) {
        return Some(entry);
    }
    // Each substitution patches a copy of the 9-byte "GGGG,EEEE" key:
    // byte ranges 2..4 (group low byte), 7..9 / 7..8 / 5..8 / 5..9 (element).
    let original: &[u8; 9] = tag.as_bytes().try_into().ok()?;
    let ranges: [(usize, usize); 5] = [
        (2, 4), // '60xx,1203'
        (7, 9), // '0020,31xx'
        (7, 8), // '0028,04x2'
        (5, 8), // '1000,xxx0'
        (5, 9), // '1010,xxxx'
    ];
    for (start, end) in ranges {
        let mut candidate = *original;
        candidate[start..end].fill(b'x');
        let key = std::str::from_utf8(&candidate).ok()?;
        if let Some(entry) = dicom_dict::dicom_main_entry(key) {
            return Some(entry);
        }
    }
    None
}

/// The `(Binary data N bytes, use -b option to extract)` placeholder the
/// oracle prints for binary values when `-b` is not given.
fn dicom_binary_placeholder(byte_count: usize) -> TagValue {
    TagValue::String(format!(
        "(Binary data {byte_count} bytes, use -b option to extract)"
    ))
}

/// Converts an element's value; `None` omits the tag (per repo rule: omit
/// when the oracle's exact output cannot be reproduced).
fn dicom_value(
    element: &DicomElement<'_>,
    encoding: DicomEncoding,
    entry: &dicom_dict::DicomDictEntry,
) -> Option<TagValue> {
    // `if ($len > 1024)`: ExifTool stores `\"Binary data $len bytes"` for
    // ANY oversized element and the app renders the scalar ref as
    // "($$val, use -b option to extract)" -- N here is the raw element
    // length.
    if element.value.len() > 1024 {
        return Some(dicom_binary_placeholder(element.value.len()));
    }

    // In explicit VR the header's VR is authoritative even when it disagrees
    // with the table; the table VR applies only to implicit VR syntax
    // (`$vr = $$tagInfo{VR} || '  ' if $tagInfo and not $vr`).
    let mut vr = element.vr.or(entry.vr).unwrap_or(*b"  ");
    // `$vr = 'UL' if $element == 0` -- a group length is always int32u.
    if element.element == 0 {
        vr = *b"UL";
    }

    let value = match dicom_format(vr) {
        DicomFormat::Int(format) => dicom_int_values(format, element.value, encoding.order),
        // FD/FL/OF: Perl NV stringification is not reproduced here -- omit
        // and count (this also covers Binary float tags, whose placeholder
        // length would depend on that stringification).
        DicomFormat::Float => return None,
        DicomFormat::None => {
            if entry.binary {
                // `Binary => 1` with no format: the placeholder length is
                // the CONVERTED string's byte length, valid UTF-8 or not.
                let bytes = dicom_string_bytes(vr, element.value, encoding.order)?;
                return Some(dicom_binary_placeholder(bytes.len()));
            }
            dicom_string_value(vr, element.value, encoding.order)?
        }
    };

    // `$$tagInfo{PrintConv} = \%uid if $uid{$val}` -- registered UIDs print
    // their names; unregistered UIDs print verbatim (no "Unknown (...)").
    if &vr == b"UI" {
        if let Some(name) = dicom_dict::dicom_uid_name(&value) {
            return Some(TagValue::String(name.to_string()));
        }
        return Some(TagValue::String(value));
    }

    // `Binary => 1` on a formatted tag (e.g. PixelData, OB/OW): GetValue's
    // implicit `\$val` ValueConv makes the DECODED string binary, so the
    // placeholder length is that string's length -- the oracle prints
    // "(Binary data 53 bytes, ...)" for an 18-byte OW PixelData that
    // decodes to a 53-character int16u string.
    if entry.binary {
        return Some(dicom_binary_placeholder(value.len()));
    }

    // The single inline PrintConv in the pinned table
    // (`{ 0 => 'Unsigned', 1 => 'Signed' }` on PixelRepresentation), with
    // GetValue's hash-PrintConv fallback for unmapped values.
    if entry.unsigned_signed {
        let printed = match value.as_str() {
            "0" => "Unsigned".to_string(),
            "1" => "Signed".to_string(),
            other => format!("Unknown ({other})"),
        };
        return Some(TagValue::String(printed));
    }

    Some(TagValue::String(value))
}

/// Dispatches TransferSyntaxUID exactly as ExifTool's
/// `/^1\.2\.840\.10008\.1\.2(\.\d+)?(\.\d+)?/` prefix match does.
fn dicom_transfer_syntax(value: &[u8]) -> DicomSyntax {
    // The stored $transferSyntax already went through the UI string rules:
    // one even-length pad space removed, truncated at the first null.
    let mut bytes = value;
    if bytes.len() % 2 == 0 {
        if let Some(stripped) = bytes.strip_suffix(b" ") {
            bytes = stripped;
        }
    }
    let bytes = bytes
        .iter()
        .position(|&byte| byte == 0)
        .map_or(bytes, |pos| &bytes[..pos]);

    let Some(rest) = bytes.strip_prefix(b"1.2.840.10008.1.2".as_slice()) else {
        // ExifTool: "Unrecognized transfer syntax" warning, then stop.
        return DicomSyntax::Unsupported;
    };
    let (first, rest) = dicom_take_dot_digits(rest);
    let (second, _) = dicom_take_dot_digits(rest);
    match (first, second) {
        // 1.2.840.10008.1.2 = implicit VR little endian
        (None, _) => DicomSyntax::Encoding(DicomEncoding::IMPLICIT_LE),
        // 1.2.840.10008.1.2.2 = explicit VR big endian
        (Some(first), _) if first == b".2".as_slice() => {
            DicomSyntax::Encoding(DicomEncoding::EXPLICIT_BE)
        }
        // 1.2.840.10008.1.2.1.99 = deflated
        (Some(first), Some(second)) if first == b".1".as_slice() && second == b".99".as_slice() => {
            DicomSyntax::Unsupported
        }
        // 1.2.840.10008.1.2.x = explicit VR little endian
        (Some(_), _) => DicomSyntax::Encoding(DicomEncoding::EXPLICIT_LE),
    }
}

/// One `(\.\d+)?` capture: a dot followed by at least one ASCII digit.
fn dicom_take_dot_digits(bytes: &[u8]) -> (Option<&[u8]>, &[u8]) {
    if bytes.first() != Some(&b'.') {
        return (None, bytes);
    }
    let digits = bytes[1..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return (None, bytes);
    }
    (Some(&bytes[..1 + digits]), &bytes[1 + digits..])
}

/// Parses DICOM Part 10 metadata using this existing specialty parser module.
pub fn parse_dicom_metadata(reader: &dyn FileReader) -> Result<MetadataMap> {
    let size = usize::try_from(reader.size())
        .map_err(|_| ExifToolError::parse_error("DICOM file is too large"))?;
    let data = reader.read(0, size)?;

    if data.get(DICOM_MAGIC_OFFSET..DICOM_DATA_OFFSET) != Some(b"DICM") {
        return Err(ExifToolError::parse_error("invalid DICOM signature"));
    }

    let mut metadata = MetadataMap::new();
    let mut offset = DICOM_DATA_OFFSET;
    let mut data_syntax = DicomSyntax::Encoding(DicomEncoding::EXPLICIT_LE);
    let mut file_meta = true;

    // Mid-file failures never fail the file: each `break` below mirrors a
    // `last` in ProcessDICOM, which warns "Error reading DICOM file
    // (corrupted?)" and still reports everything already extracted.
    while offset + 8 <= data.len() {
        if file_meta {
            // The file-meta group is always explicit little-endian; the data
            // syntax takes over at the first element outside group 0x0002.
            let little = EndianReader::little_endian(data);
            let Some(group) = little.u16_at(offset) else {
                break;
            };
            if group != 0x0002 {
                file_meta = false;
            }
        }

        let encoding = if file_meta {
            DicomEncoding::EXPLICIT_LE
        } else {
            match data_syntax {
                DicomSyntax::Encoding(encoding) => encoding,
                // Deflated or unrecognized transfer syntax: stop the walk,
                // keep what the file-meta group gave us.
                DicomSyntax::Unsupported => break,
            }
        };
        let Ok(element) = parse_dicom_element(data, offset, encoding) else {
            // Truncated or malformed element: ExifTool warns "(corrupted?)"
            // and reports everything already read.
            break;
        };

        if element.group == 0x0002 && element.element == 0x0010 {
            data_syntax = dicom_transfer_syntax(element.value);
        }

        if let Some(entry) = dicom_dict_entry(element.group, element.element) {
            if let Some(value) = dicom_value(&element, encoding, entry) {
                metadata.insert(format!("DICOM:{}", entry.name), value);
            }
        }
        if element.next_offset <= offset {
            break;
        }
        offset = element.next_offset;
    }

    metadata.insert("File:FileType", TagValue::new_string("DICOM"));
    metadata.insert("File:FileTypeExtension", TagValue::new_string("dcm"));
    metadata.insert("File:MIMEType", TagValue::new_string("application/dicom"));
    Ok(metadata)
}

#[cfg(test)]
mod dicom_tests {
    use super::*;
    use crate::test_support::TestReader;

    fn dicom_file(elements: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; DICOM_MAGIC_OFFSET];
        data.extend_from_slice(b"DICM");
        data.extend_from_slice(elements);
        data
    }

    #[test]
    fn tm_conversion_requires_six_leading_digits() {
        // DICOM.pm: s/^ *(\d{2})(\d{2})(\d{2}[^ ]*)/$1:$2:$3/
        assert_eq!(dicom_time_bytes(b"143415"), b"14:34:15".to_vec());
        assert_eq!(dicom_time_bytes(b"143415.5"), b"14:34:15.5".to_vec());
        assert_eq!(dicom_time_bytes(b" 143415"), b"14:34:15".to_vec());
        // Partial times are legal DICOM; ExifTool reports them verbatim.
        assert_eq!(dicom_time_bytes(b"1434"), b"1434".to_vec());
        assert_eq!(dicom_time_bytes(b"14"), b"14".to_vec());
    }

    #[test]
    fn non_utf8_time_value_is_omitted_not_a_panic() {
        // Regression: the old text-based conversion sliced a lossy string at
        // byte 6, which fell inside a replacement character and panicked.
        assert_eq!(
            dicom_string_value(*b"TM", b"1\xFF\xFF", ByteOrder::Little),
            None
        );
        assert_eq!(
            dicom_string_value(*b"TM", b"143415", ByteOrder::Little),
            Some("14:34:15".to_string())
        );
    }

    #[test]
    fn da_conversion_is_prefix_anchored_like_exiftool() {
        assert_eq!(
            dicom_string_value(*b"DA", b" 20010316", ByteOrder::Little),
            Some("2001:03:16".to_string())
        );
        assert_eq!(
            dicom_string_value(*b"DA", b"20010316\\20010317", ByteOrder::Little),
            Some("2001:03:16\\20010317".to_string())
        );
        assert_eq!(
            dicom_string_value(*b"DA", b"2001031", ByteOrder::Little),
            Some("2001031".to_string())
        );
    }

    #[test]
    fn per_vr_trimming_matches_exiftool() {
        // SH: leading and trailing spaces are not significant.
        assert_eq!(
            dicom_string_value(*b"SH", b" A123 ", ByteOrder::Little),
            Some("A123".to_string())
        );
        // LT: trailing spaces only.
        assert_eq!(
            dicom_string_value(*b"LT", b" note  ", ByteOrder::Little),
            Some(" note".to_string())
        );
        // UI truncates at the first null; other VRs keep nulls.
        assert_eq!(
            dicom_string_value(*b"UI", b"1.2.840\0", ByteOrder::Little),
            Some("1.2.840".to_string())
        );
        assert_eq!(
            dicom_string_value(*b"SH", b"AB\0", ByteOrder::Little),
            Some("AB\0".to_string())
        );
        // Default VRs get only the single even-length pad space removed.
        assert_eq!(
            dicom_string_value(*b"UN", b"ab  ", ByteOrder::Little),
            Some("ab ".to_string())
        );
    }

    #[test]
    fn odd_length_int16u_ignores_the_stray_byte() {
        assert_eq!(
            dicom_int_values(DicomIntFormat::Int16u, &[1, 0, 2, 0, 9], ByteOrder::Little),
            "1 2"
        );
        assert_eq!(
            dicom_int_values(DicomIntFormat::Int16u, &[7], ByteOrder::Little),
            ""
        );
    }

    #[test]
    fn int_formats_decode_like_readvalue() {
        // int8u (OB): FileMetaInfoVersion b"\x00\x01" -> "0 1" (oracle-quoted
        // for the pinned corpus DICOM.dcm).
        assert_eq!(
            dicom_int_values(DicomIntFormat::Int8u, &[0, 1], ByteOrder::Little),
            "0 1"
        );
        // int16s (SS): LargestImagePixelValue b"\x10\x03" -> "784".
        assert_eq!(
            dicom_int_values(DicomIntFormat::Int16s, &[0x10, 0x03], ByteOrder::Little),
            "784"
        );
        assert_eq!(
            dicom_int_values(DicomIntFormat::Int16s, &[0xFF, 0xFF], ByteOrder::Little),
            "-1"
        );
        // int32u (UL): FileMetaInfoGroupLength 180.
        assert_eq!(
            dicom_int_values(DicomIntFormat::Int32u, &[0xB4, 0, 0, 0], ByteOrder::Little),
            "180"
        );
        // int32s (SL).
        assert_eq!(
            dicom_int_values(
                DicomIntFormat::Int32s,
                &[0xFF, 0xFF, 0xFF, 0xFF],
                ByteOrder::Little
            ),
            "-1"
        );
        // Big-endian order is honored (explicit VR big endian syntax).
        assert_eq!(
            dicom_int_values(DicomIntFormat::Int16u, &[0x01, 0x00], ByteOrder::Big),
            "256"
        );
    }

    #[test]
    fn dictionary_lookup_matches_processdicom_wildcards() {
        // Exact key.
        let entry = dicom_dict_entry(0x0002, 0x0000).expect("FileMetaInfoGroupLength");
        assert_eq!(entry.name, "FileMetaInfoGroupLength");
        assert_eq!(entry.vr, Some(*b"UL"));
        // s/^(..)../$1xx/: '60xx,1203' matches any overlay group 60NN.
        let entry = dicom_dict_entry(0x6012, 0x1203).expect("60xx wildcard");
        assert_eq!(entry.name, "OverlaysBlue");
        // s/..$/xx/: '0020,31xx' (SourceImageIDs).
        let entry = dicom_dict_entry(0x0020, 0x3101).expect("31xx wildcard");
        assert_eq!(entry.name, "SourceImageIDs");
        // s/.(.)$/x$1/: '0028,04x2' (CoefficientCoding).
        let entry = dicom_dict_entry(0x0028, 0x0412).expect("04x2 wildcard");
        assert_eq!(entry.name, "CoefficientCoding");
        // s/...(.)$/xxx$1/: '1000,xxx0' (EscapeTriplet).
        let entry = dicom_dict_entry(0x1000, 0x0010).expect("xxx0 wildcard");
        assert_eq!(entry.name, "EscapeTriplet");
        // s/....$/xxxx/: '1010,xxxx' (ZonalMap).
        let entry = dicom_dict_entry(0x1010, 0xABCD).expect("xxxx wildcard");
        assert_eq!(entry.name, "ZonalMap");
        // '7Fxx,0010' PixelData carries Binary => 1.
        let entry = dicom_dict_entry(0x7FE0, 0x0010).expect("PixelData");
        assert_eq!(entry.name, "PixelData");
        assert!(entry.binary);
        // Perl hash-literal duplicate keys: the LATER entry wins.
        let entry = dicom_dict_entry(0x0021, 0x1019).expect("duplicate key");
        assert_eq!(entry.name, "AcqreconRecordChecksum");
        // Unmapped private tag: no entry, tag omitted (matches the oracle
        // without -u).
        assert!(dicom_dict_entry(0x0009, 0x0001).is_none());
        // The three lowercase-hex keys are transcribed verbatim but stay
        // unreachable, exactly as in ExifTool: ProcessDICOM formats every
        // lookup key with uppercase '%.4X,%.4X' against a case-sensitive
        // hash. Byte-verified: the pinned oracle reports nothing for element
        // (0043,106F) without -u, and generic DICOM_0043_106F with it --
        // never ScannerTableEntry.
        assert!(dicom_dict::dicom_main_entry("0043,106f").is_some());
        assert!(dicom_dict_entry(0x0043, 0x106F).is_none());
        assert!(dicom_dict_entry(0x0074, 0x100A).is_none());
        assert!(dicom_dict_entry(0x0074, 0x100C).is_none());
    }

    #[test]
    fn transcribed_dictionary_is_complete() {
        // 13.59's %Image::ExifTool::DICOM::Main has 5674 tag-ID lines, five
        // of which are Perl hash-literal duplicate keys (later wins), and
        // %uid has 1979 lines with one duplicate. A shrink here means the
        // generator or its input changed; re-run
        // tools/exiftool-tables/gen_dicom_dict.py against the pinned tree.
        assert_eq!(dicom_dict::DICOM_MAIN.len(), 5669);
        assert_eq!(dicom_dict::DICOM_UID.len(), 1978);
        // Sorted (byte order) so the binary-search lookups are sound.
        assert!(dicom_dict::DICOM_MAIN.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(dicom_dict::DICOM_UID.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn uid_printconv_applies_only_to_registered_uids() {
        assert_eq!(
            dicom_dict::dicom_uid_name("1.2.840.10008.1.2.1"),
            Some("Explicit VR Little Endian")
        );
        assert_eq!(
            dicom_dict::dicom_uid_name("1.2.840.10008.5.1.4.1.1.4"),
            Some("MR Image Storage")
        );
        // Unregistered UIDs print verbatim; there is no Unknown() fallback.
        assert_eq!(dicom_dict::dicom_uid_name("0.0.0.0"), None);
    }

    #[test]
    fn oversized_and_unmapped_printconv_values_match_the_oracle() {
        // Both expectations quoted from the pinned 13.59 oracle
        // (`-a -G1 -s` on this exact synthesized file):
        //   PixelRepresentation : Unknown (2)
        //   PixelData           : (Binary data 1026 bytes, use -b option to extract)
        let mut body = Vec::new();
        // (0028,0103) US PixelRepresentation = 2: not in the inline
        // PrintConv { 0 => 'Unsigned', 1 => 'Signed' }, so GetValue's
        // hash-PrintConv fallback prints "Unknown (2)".
        body.extend_from_slice(&[0x28, 0x00, 0x03, 0x01]);
        body.extend_from_slice(b"US");
        body.extend_from_slice(&2u16.to_le_bytes());
        body.extend_from_slice(&2u16.to_le_bytes());
        // (7FE0,0010) OW PixelData, 1026 bytes: `if ($len > 1024)` stores
        // the placeholder with the RAW element length, taking priority over
        // both the int16u format and the Binary => 1 decoded-length rule.
        body.extend_from_slice(&[0xE0, 0x7F, 0x10, 0x00]);
        body.extend_from_slice(b"OW");
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&1026u32.to_le_bytes());
        body.extend_from_slice(&[0u8; 1026]);

        let reader = TestReader::new(dicom_file(&body));
        let metadata = parse_dicom_metadata(&reader).unwrap();
        assert_eq!(
            metadata.get_string("DICOM:PixelRepresentation"),
            Some("Unknown (2)")
        );
        assert_eq!(
            metadata.get_string("DICOM:PixelData"),
            Some("(Binary data 1026 bytes, use -b option to extract)")
        );
    }

    #[test]
    fn undefined_length_elements_are_walked_not_fatal() {
        let mut body = Vec::new();
        // (0008,0022) DA "20010316"
        body.extend_from_slice(&[0x08, 0x00, 0x22, 0x00]);
        body.extend_from_slice(b"DA");
        body.extend_from_slice(&8u16.to_le_bytes());
        body.extend_from_slice(b"20010316");
        // (7FE0,0010) OB with undefined length: encapsulated pixel data.
        body.extend_from_slice(&[0xE0, 0x7F, 0x10, 0x00]);
        body.extend_from_slice(b"OB");
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&u32::MAX.to_le_bytes());
        //   item (FFFE,E000) with 4 bytes of fragment data (no VR field)
        body.extend_from_slice(&[0xFE, 0xFF, 0x00, 0xE0]);
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(&[1, 2, 3, 4]);
        //   sequence delimiter (FFFE,E0DD), zero length
        body.extend_from_slice(&[0xFE, 0xFF, 0xDD, 0xE0]);
        body.extend_from_slice(&0u32.to_le_bytes());
        // (0020,0012) IS "31763 " (space-padded to an even length)
        body.extend_from_slice(&[0x20, 0x00, 0x12, 0x00]);
        body.extend_from_slice(b"IS");
        body.extend_from_slice(&6u16.to_le_bytes());
        body.extend_from_slice(b"31763 ");

        let reader = TestReader::new(dicom_file(&body));
        let metadata = parse_dicom_metadata(&reader).unwrap();
        assert_eq!(
            metadata.get("DICOM:AcquisitionDate"),
            Some(&TagValue::String("2001:03:16".to_string()))
        );
        // Extraction continues past the undefined-length element.
        assert_eq!(
            metadata.get("DICOM:AcquisitionNumber"),
            Some(&TagValue::String("31763".to_string()))
        );
        assert_eq!(metadata.get_string("File:FileType"), Some("DICOM"));
    }

    #[test]
    fn truncated_tail_keeps_tags_already_extracted() {
        let mut body = Vec::new();
        // (0008,0032) TM "143415"
        body.extend_from_slice(&[0x08, 0x00, 0x32, 0x00]);
        body.extend_from_slice(b"TM");
        body.extend_from_slice(&6u16.to_le_bytes());
        body.extend_from_slice(b"143415");
        // A truncated element: the header promises more bytes than remain.
        body.extend_from_slice(&[0x08, 0x00, 0x50, 0x00]);
        body.extend_from_slice(b"SH");
        body.extend_from_slice(&64u16.to_le_bytes());
        body.extend_from_slice(b"abc");

        let reader = TestReader::new(dicom_file(&body));
        let metadata = parse_dicom_metadata(&reader).unwrap();
        assert_eq!(
            metadata.get("DICOM:AcquisitionTime"),
            Some(&TagValue::String("14:34:15".to_string()))
        );
        assert_eq!(metadata.get_string("File:FileType"), Some("DICOM"));
    }

    #[test]
    fn od_is_short_form_like_the_pinned_oracle() {
        // ExifTool 13.59's %vr32 holds only OB/OW/OF/SQ/UT/UN, so OD (like
        // OL/OV/UC/UR) uses the 8-byte header; matching that framing keeps
        // the walk aligned with the oracle for everything that follows.
        let mut body = Vec::new();
        body.extend_from_slice(&[0x18, 0x00, 0x00, 0x99]); // arbitrary tag
        body.extend_from_slice(b"OD");
        body.extend_from_slice(&8u16.to_le_bytes());
        body.extend_from_slice(&[0u8; 8]);
        body.extend_from_slice(&[0x08, 0x00, 0x22, 0x00]);
        body.extend_from_slice(b"DA");
        body.extend_from_slice(&8u16.to_le_bytes());
        body.extend_from_slice(b"20010316");

        let reader = TestReader::new(dicom_file(&body));
        let metadata = parse_dicom_metadata(&reader).unwrap();
        assert_eq!(
            metadata.get("DICOM:AcquisitionDate"),
            Some(&TagValue::String("2001:03:16".to_string()))
        );
    }

    #[test]
    fn deflated_transfer_syntax_stops_gracefully_after_file_meta() {
        let mut body = Vec::new();
        // (0002,0010) UI TransferSyntaxUID = deflated explicit LE
        body.extend_from_slice(&[0x02, 0x00, 0x10, 0x00]);
        body.extend_from_slice(b"UI");
        body.extend_from_slice(&22u16.to_le_bytes());
        body.extend_from_slice(b"1.2.840.10008.1.2.1.99");
        // Compressed garbage follows; parsing it as elements would fabricate
        // values, so the walk must stop while keeping the File tags.
        body.extend_from_slice(&[
            0x78, 0x9C, 0x08, 0x00, 0x22, 0x00, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
        ]);

        let reader = TestReader::new(dicom_file(&body));
        let metadata = parse_dicom_metadata(&reader).unwrap();
        assert_eq!(metadata.get_string("File:FileType"), Some("DICOM"));
        // The file-meta group itself was still read (the oracle reports it
        // before warning about the compressed stream), and the registered
        // UID prints its %uid name.
        assert_eq!(
            metadata.get("DICOM:TransferSyntaxUID"),
            Some(&TagValue::String(
                "Deflated Explicit VR Little Endian".to_string()
            ))
        );
        // ...but nothing was fabricated from the compressed bytes that
        // follow: the only DICOM tag is the transfer syntax itself.
        assert_eq!(
            metadata
                .keys()
                .filter(|name| name.starts_with("DICOM:"))
                .count(),
            1
        );
    }

    #[test]
    fn implicit_vr_uses_the_table_vr_for_conversion() {
        let mut body = Vec::new();
        // (0002,0010) UI TransferSyntaxUID = implicit VR little endian
        body.extend_from_slice(&[0x02, 0x00, 0x10, 0x00]);
        body.extend_from_slice(b"UI");
        body.extend_from_slice(&18u16.to_le_bytes());
        body.extend_from_slice(b"1.2.840.10008.1.2\0");
        // Implicit-VR data element: (0008,0032) with a 32-bit length.
        body.extend_from_slice(&[0x08, 0x00, 0x32, 0x00]);
        body.extend_from_slice(&6u32.to_le_bytes());
        body.extend_from_slice(b"143415");

        let reader = TestReader::new(dicom_file(&body));
        let metadata = parse_dicom_metadata(&reader).unwrap();
        assert_eq!(
            metadata.get("DICOM:AcquisitionTime"),
            Some(&TagValue::String("14:34:15".to_string()))
        );
    }

    #[test]
    fn parses_requested_tags_from_real_dicom_sample() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let path = format!("{}/DICOM.dcm", crate::test_support::PINNED_CORPUS_ROOT);
        let data = std::fs::read(path).expect("pinned DICOM sample should be readable");
        let metadata = parse_dicom_metadata(&crate::test_support::TestReader::new(data))
            .expect("pinned DICOM sample should parse");

        assert_eq!(
            metadata.get("DICOM:AccessionNumber"),
            Some(&TagValue::String(String::new()))
        );
        assert_eq!(
            metadata.get("DICOM:AcquisitionDate"),
            Some(&TagValue::String("2001:03:16".to_string()))
        );
        assert_eq!(
            metadata.get("DICOM:AcquisitionMatrix"),
            Some(&TagValue::String("0 256 256 0".to_string()))
        );
        assert_eq!(
            metadata.get("DICOM:AcquisitionNumber"),
            Some(&TagValue::String("31763".to_string()))
        );
        assert_eq!(
            metadata.get("DICOM:AcquisitionTime"),
            Some(&TagValue::String("14:34:15".to_string()))
        );
        assert_eq!(
            metadata.get("DICOM:AdditionalPatientHistory"),
            Some(&TagValue::String(String::new()))
        );

        // One pin per newly-decoded class, each quoted from the pinned
        // oracle (`exiftool-pinned.sh -a -G1 -s` on this sample).
        // UL group length.
        assert_eq!(
            metadata.get_string("DICOM:FileMetaInfoGroupLength"),
            Some("180")
        );
        // OB -> int8u multi-value.
        assert_eq!(
            metadata.get_string("DICOM:FileMetaInfoVersion"),
            Some("0 1")
        );
        // Registered UID PrintConv (%uid).
        assert_eq!(
            metadata.get_string("DICOM:TransferSyntaxUID"),
            Some("Explicit VR Little Endian")
        );
        assert_eq!(
            metadata.get_string("DICOM:MediaStorageSOPClassUID"),
            Some("MR Image Storage")
        );
        // Unregistered UID prints verbatim.
        assert_eq!(
            metadata.get_string("DICOM:ImplementationClassUID"),
            Some("0.0.0.0")
        );
        // US single value.
        assert_eq!(metadata.get_string("DICOM:Rows"), Some("256"));
        // SS (int16s).
        assert_eq!(
            metadata.get_string("DICOM:LargestImagePixelValue"),
            Some("784")
        );
        // Inline PrintConv { 0 => 'Unsigned', 1 => 'Signed' }.
        assert_eq!(
            metadata.get_string("DICOM:PixelRepresentation"),
            Some("Signed")
        );
        // Binary => 1: the placeholder length is the DECODED int16u
        // string's length (18 OW bytes -> 9 numbers -> 53 characters).
        assert_eq!(
            metadata.get_string("DICOM:PixelData"),
            Some("(Binary data 53 bytes, use -b option to extract)")
        );
        // DS keeps ExifTool's exact string form (no numeric reformatting).
        assert_eq!(metadata.get_string("DICOM:PatientWeight"), Some("61.2350"));
        assert_eq!(
            metadata.get_string("DICOM:ImagePositionPatient"),
            Some("-110.500\\-96.2063\\59.0425")
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn card(text: &str) -> [u8; FITS_RECORD_SIZE] {
        assert!(text.len() <= FITS_RECORD_SIZE);
        let mut card = [b' '; FITS_RECORD_SIZE];
        card[..text.len()].copy_from_slice(text.as_bytes());
        card
    }

    fn fits(cards: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for text in cards {
            bytes.extend_from_slice(&card(text));
        }
        bytes.resize(FITS_BLOCK_SIZE, b' ');
        bytes
    }

    #[test]
    fn canonical_names_cover_the_value_confirmed_fits_renames() {
        let expected = [
            ("BITPIX", "Bitpix"),
            ("ORIGIN", "Origin"),
            ("CREATOR", "Creator"),
            ("TIME-OBS", "ObservationTime"),
            ("TIME-END", "ObservationTimeEnd"),
            ("TIMESYS", "Timesys"),
            ("MJDREFI", "Mjdrefi"),
            ("MJDREFF", "Mjdreff"),
            ("TIMEZERO", "Timezero"),
            ("TIMEUNIT", "Timeunit"),
            ("TIMEREF", "Timeref"),
            ("TASSIGN", "Tassign"),
            ("TIERRELA", "Tierrela"),
            ("TIERABSO", "Tierabso"),
            ("OBJECT", "Object"),
            ("RA_OBJ", "RaObj"),
            ("DEC_OBJ", "DecObj"),
            ("EQUINOX", "Equinox"),
            ("RADECSYS", "Radecsys"),
            ("OBSERVER", "Observer"),
            ("OBS_ID", "ObsId"),
            ("CHECKSUM", "Checksum"),
        ];
        for (keyword, name) in expected {
            assert_eq!(FITSParser::tag_name(keyword), name);
        }
    }

    #[test]
    fn quoted_slashes_and_escaped_quotes_are_values_not_comments() {
        assert_eq!(
            FITSParser::parse_record(&card(
                "TIMVERSN= 'XFF/95-004'         / XFF design document"
            )),
            Some(("TIMVERSN".into(), "XFF/95-004".into(), None))
        );
        assert_eq!(
            FITSParser::parse_record(&card("OBJECT  = 'O''Brien / field'   / observer target")),
            Some(("OBJECT".into(), "O'Brien / field".into(), None))
        );
        assert_eq!(
            FITSParser::parse_record(&card("DATASUM = '         0'         / data unit checksum")),
            Some(("DATASUM".into(), "         0".into(), None))
        );
    }

    #[test]
    fn unquoted_card_comments_are_separated_from_values() {
        assert_eq!(
            FITSParser::parse_record(&card(
                "MJDREFF =   6.965740740000D-04 / fractional reference"
            )),
            Some((
                "MJDREFF".into(),
                "6.965740740000e-04".into(),
                Some("fractional reference".into()),
            ))
        );
    }

    #[test]
    fn parser_uses_exiftool_names_and_does_not_emit_card_comments() {
        let reader = TestReader::new(fits(&[
            "SIMPLE  =                    T / conforms to FITS",
            "BITPIX  =                    8 / bits per pixel",
            "NAXIS   =                    0 / axes",
            "DATE    = '28/01/97'           / creation date",
            "TIME-OBS= '11:56:26'           / start time",
            "TIMVERSN= 'XFF/95-004'         / design document",
            "DATASUM = '         0'         / data checksum",
            "END",
        ]));

        let metadata = FITSParser.parse(&reader).unwrap();
        assert_eq!(metadata.get_integer("Bitpix"), Some(8));
        assert_eq!(metadata.get_integer("Naxis"), Some(0));
        assert_eq!(metadata.get_string("CreateDate"), Some("28/01/97"));
        assert_eq!(metadata.get_string("ObservationTime"), Some("11:56:26"));
        assert_eq!(metadata.get_string("Timversn"), Some("XFF/95-004"));
        assert_eq!(metadata.get_string("Datasum"), Some("         0"));
        assert!(!metadata.keys().any(|name| name.ends_with("Comment")));
    }

    /// The gate used to be a bare six-byte `b"SIMPLE"` comparison, which
    /// accepted any text opening those six letters. ExifTool's magic is the
    /// full 30-byte keyword record `SIMPLE  = {20}T`.
    #[test]
    fn rejects_prose_that_merely_opens_with_simple() {
        let reader = TestReader::new(b"SIMPLE ANSWER: 42. Not a FITS file.".to_vec());
        assert!(!FITSParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn accepts_a_real_simple_keyword_record() {
        let reader = TestReader::new(fits(&["SIMPLE  =                    T", "END"]));
        assert!(FITSParser::verify_signature(&reader).unwrap());
    }

    /// The detector's dedicated FITS check and this gate now both ask the
    /// magic table, so they cannot answer a given header two different ways.
    #[test]
    fn signature_check_agrees_with_the_magic_table() {
        for header in [
            fits(&["SIMPLE  =                    T", "END"]),
            b"SIMPLE ANSWER: 42. Not a FITS file.".to_vec(),
            b"SIMPLEX is a linear-programming method.".to_vec(),
        ] {
            let reader = TestReader::new(header.clone());
            assert_eq!(
                FITSParser::verify_signature(&reader).unwrap(),
                crate::filetype::matches_magic("FITS", &header),
                "gate and magic table disagree about {header:?}"
            );
        }
    }
}
