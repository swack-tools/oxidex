//! IPTC segment parser for JPEG
//!
//! This module handles parsing of IPTC data in JPEG APP13 segments.
//! IPTC data is stored in Adobe Photoshop Image Resource Blocks (8BIM).

use crate::core::TagValue;
use crate::core::value_formatter::{
    format_iptc_coded_charset, format_iptc_date, format_iptc_time, format_iptc_urgency,
};
use crate::error::Result;
use crate::parsers::jpeg::segment_parser::Segment;
use nom::{
    IResult,
    bytes::complete::{tag, take},
    number::complete::{be_u16, be_u32, u8 as nom_u8},
};

// Constants
const PHOTOSHOP_SIGNATURE: &[u8] = b"Photoshop 3.0\0";
const EIGHTBIM_SIGNATURE: &[u8] = b"8BIM";
const IPTC_RESOURCE_ID: u16 = 0x0404;
const IPTC_TAG_MARKER: u8 = 0x1C;
const APP13_MARKER: u16 = 0xFFED;

/// Application-record datasets ExifTool reports as lists because the IIM spec
/// marks them repeatable (`IPTC.pm`, `List => 1`): 20 = SupplementalCategories,
/// 25 = Keywords. Every other dataset keeps last-wins semantics, which is what
/// a single-valued map insert already does.
///
/// This is the one place the rule lives -- `parsers::pdf::photoshop_resources`
/// decodes the same 0x0404 resource out of a PDF and asks here rather than
/// keeping a second copy that can drift.
pub fn is_repeatable_iptc_dataset(record_number: u8, dataset_number: u8) -> bool {
    record_number == 2 && matches!(dataset_number, 20 | 25)
}

/// Represents an Adobe Photoshop Image Resource Block
#[derive(Debug, Clone, PartialEq)]
struct ImageResourceBlock<'a> {
    /// Resource ID (e.g., 0x0404 for IPTC)
    id: u16,
    /// Resource name (Pascal string)
    name: &'a [u8],
    /// Resource data payload
    data: &'a [u8],
}

/// Represents a single IPTC IIM record
#[derive(Debug, Clone, PartialEq)]
pub struct IptcRecord {
    /// Record number (usually 2 for Application Record)
    pub record_number: u8,
    /// Dataset number (identifies the specific tag)
    pub dataset_number: u8,
    /// Record data
    pub data: Vec<u8>,
}

/// Parses a single Adobe Photoshop Image Resource Block (8BIM).
///
/// # Format
/// - Signature: "8BIM" (4 bytes)
/// - ID: 2 bytes (big-endian)
/// - Name: Pascal string (1 byte length + data), padded to even length
/// - Size: 4 bytes (big-endian)
/// - Data: variable length
fn parse_image_resource_block(input: &[u8]) -> IResult<&[u8], ImageResourceBlock<'_>> {
    // Parse 8BIM signature
    let (input, _) = tag(EIGHTBIM_SIGNATURE)(input)?;

    // Parse resource ID (2 bytes, big-endian)
    let (input, id) = be_u16(input)?;

    // Parse Pascal string name (1 byte length + data)
    let (input, name_length) = nom_u8(input)?;
    let (input, name) = take(name_length as usize)(input)?;

    // Pascal string must be padded to even length (including length byte)
    // Total length so far: 1 (length byte) + name_length
    // If odd, add 1 byte padding
    let total_name_length = 1 + name_length as usize;
    let (input, _) = if total_name_length % 2 == 1 {
        take(1usize)(input)? // Take 1 byte padding
    } else {
        (input, &b""[..]) // No padding needed
    };

    // Parse data size (4 bytes, big-endian)
    let (input, data_size) = be_u32(input)?;

    // Parse data
    let (input, data) = take(data_size as usize)(input)?;

    Ok((input, ImageResourceBlock { id, name, data }))
}

/// Parses a single IPTC IIM record.
///
/// # Format
/// - Tag marker: 0x1C (1 byte)
/// - Record number: 1 byte (usually 2 for Application Record)
/// - Dataset number: 1 byte
/// - Length: 2 bytes (big-endian), or extended format for > 32767 bytes
/// - Data: variable length
fn parse_iptc_record(input: &[u8]) -> IResult<&[u8], IptcRecord> {
    // Parse tag marker (must be 0x1C)
    let (input, _) = tag(&[IPTC_TAG_MARKER][..])(input)?;

    // Parse record number (1 byte)
    let (input, record_number) = nom_u8(input)?;

    // Parse dataset number (1 byte)
    let (input, dataset_number) = nom_u8(input)?;

    // Parse length (2 bytes, big-endian)
    let (input, length) = be_u16(input)?;

    // Check for extended format (if length > 32767, it's actually a marker)
    // For now, we'll just support standard format (< 32768 bytes)
    let data_length = length as usize;

    // Parse data
    let (input, data_bytes) = take(data_length)(input)?;

    Ok((
        input,
        IptcRecord {
            record_number,
            dataset_number,
            data: data_bytes.to_vec(),
        },
    ))
}

/// Parses all IPTC IIM records from a data block.
///
/// Returns a vector of all successfully parsed records.
/// Stops at first parse error or end of data.
pub fn parse_all_iptc_records(input: &[u8]) -> Result<Vec<IptcRecord>> {
    let mut records = Vec::new();
    let mut current = input;

    while !current.is_empty() {
        // Check if next byte is tag marker
        if current[0] != IPTC_TAG_MARKER {
            break;
        }

        match parse_iptc_record(current) {
            Ok((remaining, record)) => {
                records.push(record);
                current = remaining;
            }
            Err(_) => {
                // Stop on parse error
                break;
            }
        }
    }

    Ok(records)
}

/// Maps IPTC dataset numbers to tag names.
///
/// # Parameters
/// - `record_number`: The record number (usually 2 for Application Record)
/// - `dataset_number`: The dataset number identifying the tag
///
/// # Returns
/// Maps IPTC dataset numbers to tag names.
///
/// Returns static string slices for known datasets to avoid allocations.
/// Tag name in the format "IPTC:TagName"
pub fn dataset_to_tag_name(record_number: u8, dataset_number: u8) -> String {
    match known_dataset_name(record_number, dataset_number) {
        Some(name) => name.to_string(),
        None => format!("IPTC:Unknown-{}-{}", record_number, dataset_number),
    }
}

/// Returns the ExifTool tag name for a dataset, or `None` when IPTC.pm has no
/// entry for it.
///
/// ExifTool registers unlisted datasets with `Unknown => 1`, which keeps them
/// out of a default dump, so callers that want ExifTool's tag set should skip
/// the `None` cases rather than invent a name for them.
pub fn known_dataset_name(record_number: u8, dataset_number: u8) -> Option<&'static str> {
    // Handle Record 2 (Application Record)
    if record_number == 2 {
        let tag_name = match dataset_number {
            0 => "IPTC:ApplicationRecordVersion",
            3 => "IPTC:ObjectTypeReference",
            4 => "IPTC:ObjectAttributeReference",
            5 => "IPTC:ObjectName",
            7 => "IPTC:EditStatus",
            8 => "IPTC:EditorialUpdate",
            10 => "IPTC:Urgency",
            12 => "IPTC:SubjectReference",
            15 => "IPTC:Category",
            20 => "IPTC:SupplementalCategories",
            22 => "IPTC:FixtureIdentifier",
            25 => "IPTC:Keywords",
            26 => "IPTC:ContentLocationCode",
            27 => "IPTC:ContentLocationName",
            30 => "IPTC:ReleaseDate",
            35 => "IPTC:ReleaseTime",
            37 => "IPTC:ExpirationDate",
            38 => "IPTC:ExpirationTime",
            40 => "IPTC:SpecialInstructions",
            42 => "IPTC:ActionAdvised",
            45 => "IPTC:ReferenceService",
            47 => "IPTC:ReferenceDate",
            50 => "IPTC:ReferenceNumber",
            55 => "IPTC:DateCreated",
            60 => "IPTC:TimeCreated",
            62 => "IPTC:DigitalCreationDate",
            63 => "IPTC:DigitalCreationTime",
            65 => "IPTC:OriginatingProgram",
            70 => "IPTC:ProgramVersion",
            75 => "IPTC:ObjectCycle",
            80 => "IPTC:By-line",
            85 => "IPTC:By-lineTitle",
            90 => "IPTC:City",
            92 => "IPTC:Sub-location",
            95 => "IPTC:Province-State",
            100 => "IPTC:Country-PrimaryLocationCode",
            101 => "IPTC:Country-PrimaryLocationName",
            103 => "IPTC:OriginalTransmissionReference",
            105 => "IPTC:Headline",
            110 => "IPTC:Credit",
            115 => "IPTC:Source",
            116 => "IPTC:CopyrightNotice",
            118 => "IPTC:Contact",
            120 => "IPTC:Caption-Abstract",
            121 => "IPTC:LocalCaption",
            122 => "IPTC:Writer-Editor",
            125 => "IPTC:RasterizedCaption",
            130 => "IPTC:ImageType",
            131 => "IPTC:ImageOrientation",
            135 => "IPTC:LanguageIdentifier",
            150 => "IPTC:AudioType",
            151 => "IPTC:AudioSamplingRate",
            152 => "IPTC:AudioSamplingResolution",
            153 => "IPTC:AudioDuration",
            154 => "IPTC:AudioOutcue",
            184 => "IPTC:JobID",
            185 => "IPTC:MasterDocumentID",
            186 => "IPTC:ShortDocumentID",
            187 => "IPTC:UniqueDocumentID",
            188 => "IPTC:OwnerID",
            200 => "IPTC:ObjectPreviewFileFormat",
            // ExifTool calls 2:201 ObjectPreviewFileVersion (IPTC.pm ApplicationRecord),
            // not "...FileFormatVer".
            201 => "IPTC:ObjectPreviewFileVersion",
            202 => "IPTC:ObjectPreviewData",
            221 => "IPTC:Prefs",
            225 => "IPTC:ClassifyState",
            228 => "IPTC:SimilarityIndex",
            230 => "IPTC:DocumentNotes",
            231 => "IPTC:DocumentHistory",
            232 => "IPTC:ExifCameraInfo",
            255 => "IPTC:CatalogSets",
            _ => return None,
        };
        return Some(tag_name);
    }

    // Handle Record 1 (Envelope Record)
    if record_number == 1 {
        let tag_name = match dataset_number {
            0 => "IPTC:EnvelopeRecordVersion",
            5 => "IPTC:Destination",
            20 => "IPTC:FileFormat",
            22 => "IPTC:FileVersion",
            30 => "IPTC:ServiceIdentifier",
            40 => "IPTC:EnvelopeNumber",
            50 => "IPTC:ProductID",
            60 => "IPTC:EnvelopePriority",
            70 => "IPTC:DateSent",
            80 => "IPTC:TimeSent",
            90 => "IPTC:CodedCharacterSet",
            100 => "IPTC:UniqueObjectName",
            120 => "IPTC:ARMIdentifier",
            122 => "IPTC:ARMVersion",
            _ => return None,
        };
        return Some(tag_name);
    }

    None
}

/// The `Format` ExifTool's IPTC tag tables declare for a dataset.
///
/// `ProcessIPTC` (IPTC.pm:1200-1245) branches on this string to decide how the
/// raw dataset payload becomes a value, so the payload alone never determines
/// the result -- the tag table does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IptcFormat {
    /// `int8u`/`int16u`/`int32u`: big-endian unsigned integer, but only when the
    /// payload is 8 bytes or shorter. Longer payloads stay raw text -- this is
    /// how FotoStation's 15-byte "Custom Field 01" survives in a 2:200 slot
    /// declared `int16u`.
    Int,
    /// `string[...]`: strip trailing NULs (some writers NUL-pad to the declared
    /// field width) and keep everything else, including interior spaces.
    Str,
    /// `digits[...]`: same trailing-NUL strip as `string`.
    Digits,
    /// `undef[...]`: opaque bytes, no conversion at all.
    Undef,
    /// No `Format` in the tag table. ExifTool then guesses `int` for short
    /// payloads that contain a control byte, else leaves the bytes as text.
    Auto,
}

/// Returns the `Format` ExifTool declares for an IPTC dataset.
///
/// Mirrors `%Image::ExifTool::IPTC::EnvelopeRecord` and
/// `%Image::ExifTool::IPTC::ApplicationRecord`. Datasets absent from those
/// tables get [`IptcFormat::Auto`], matching ExifTool's `AddTagToTable` fallback
/// for unknown datasets.
pub fn dataset_format(record_number: u8, dataset_number: u8) -> IptcFormat {
    match record_number {
        1 => match dataset_number {
            0 | 20 | 22 | 120 | 122 => IptcFormat::Int,
            40 | 60 | 70 => IptcFormat::Digits,
            5 | 30 | 50 | 80 | 90 | 100 => IptcFormat::Str,
            _ => IptcFormat::Auto,
        },
        2 => match dataset_number {
            0 | 200 | 201 => IptcFormat::Int,
            8 | 10 | 30 | 37 | 42 | 47 | 50 | 55 | 62 | 151 | 152 | 153 => IptcFormat::Digits,
            125 | 202 => IptcFormat::Undef,
            3..=5
            | 7
            | 12
            | 15
            | 20
            | 22
            | 25..=27
            | 35
            | 38
            | 40
            | 45
            | 60
            | 63
            | 65
            | 70
            | 75
            | 80
            | 85
            | 90
            | 92
            | 95
            | 100
            | 101
            | 103
            | 105
            | 110
            | 115
            | 116
            | 118
            | 120..=122
            | 130
            | 131
            | 135
            | 150
            | 154
            | 184..=188
            | 221
            | 225
            | 228
            | 230..=232
            | 255 => IptcFormat::Str,
            _ => IptcFormat::Auto,
        },
        _ => IptcFormat::Auto,
    }
}

/// ExifTool's `%fileFormat` PrintConv (IPTC.pm:56-86), shared by the Envelope
/// record's FileFormat (1:20) and the Application record's
/// ObjectPreviewFileFormat (2:200).
fn file_format_print_conv(value: &str) -> Option<&'static str> {
    Some(match value {
        "0" => "No ObjectData",
        "1" => "IPTC-NAA Digital Newsphoto Parameter Record",
        "2" => "IPTC7901 Recommended Message Format",
        "3" => "Tagged Image File Format (Adobe/Aldus Image data)",
        "4" => "Illustrator (Adobe Graphics data)",
        "5" => "AppleSingle (Apple Computer Inc)",
        "6" => "NAA 89-3 (ANPA 1312)",
        "7" => "MacBinary II",
        "8" => "IPTC Unstructured Character Oriented File Format (UCOFF)",
        "9" => "United Press International ANPA 1312 variant",
        "10" => "United Press International Down-Load Message",
        "11" => "JPEG File Interchange (JFIF)",
        "12" => "Photo-CD Image-Pac (Eastman Kodak)",
        "13" => "Bit Mapped Graphics File [.BMP] (Microsoft)",
        "14" => "Digital Audio File [.WAV] (Microsoft & Creative Labs)",
        "15" => "Audio plus Moving Video [.AVI] (Microsoft)",
        "16" => "PC DOS/Windows Executable Files [.COM][.EXE]",
        "17" => "Compressed Binary File [.ZIP] (PKWare Inc)",
        "18" => "Audio Interchange File Format AIFF (Apple Computer Inc)",
        "19" => "RIFF Wave (Microsoft Corporation)",
        "20" => "Freehand (Macromedia/Aldus)",
        "21" => "Hypertext Markup Language [.HTML] (The Internet Society)",
        "22" => "MPEG 2 Audio Layer 2 (Musicom), ISO/IEC",
        "23" => "MPEG 2 Audio Layer 3, ISO/IEC",
        "24" => "Portable Document File [.PDF] Adobe",
        "25" => "News Industry Text Format (NITF)",
        "26" => "Tape Archive [.TAR]",
        "27" => "Tidningarnas Telegrambyra NITF version (TTNITF DTD)",
        "28" => "Ritzaus Bureau NITF version (RBNITF DTD)",
        "29" => "Corel Draw [.CDR]",
        _ => return None,
    })
}

/// ExifTool's ObjectCycle PrintConv (IPTC.pm:456-463).
fn object_cycle_print_conv(value: &str) -> Option<&'static str> {
    Some(match value {
        "a" => "Morning",
        "p" => "Evening",
        "b" => "Both Morning and Evening",
        _ => return None,
    })
}

/// Renders ExifTool's PrintConv-miss fallback, `Unknown (VALUE)`.
fn unknown_print_conv(value: &str) -> String {
    format!("Unknown ({})", value)
}

/// ExifTool's Prefs PrintConv (IPTC.pm:2:221), which rewrites PhotoMechanic's
/// colon-packed `tagged:colorclass:rating:framenum` into labelled fields:
///
/// ```text
/// 0:0:5:003344  ->  Tagged:0, ColorClass:0, Rating:5, FrameNum:003344
/// ```
///
/// Like the Perl `s///`, this replaces only the first match and returns the
/// value untouched when the pattern does not appear.
fn prefs_print_conv(value: &str) -> String {
    // s[\s*(\d+):\s*(\d+):\s*(\d+):\s*(\S*)]
    //  [Tagged:$1, ColorClass:$2, Rating:$3, FrameNum:$4]
    let bytes = value.as_bytes();
    for start in 0..=bytes.len() {
        let mut pos = start;
        // Leading \s*
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let Some((tagged, pos1)) = scan_digits(bytes, pos) else {
            continue;
        };
        let Some(pos2) = expect_colon_then_space(bytes, pos1) else {
            continue;
        };
        let Some((color_class, pos3)) = scan_digits(bytes, pos2) else {
            continue;
        };
        let Some(pos4) = expect_colon_then_space(bytes, pos3) else {
            continue;
        };
        let Some((rating, pos5)) = scan_digits(bytes, pos4) else {
            continue;
        };
        let Some(pos6) = expect_colon_then_space(bytes, pos5) else {
            continue;
        };
        // (\S*) -- greedy run of non-whitespace, possibly empty
        let mut end = pos6;
        while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
            end += 1;
        }
        let frame_num = &value[pos6..end];
        return format!(
            "{}Tagged:{}, ColorClass:{}, Rating:{}, FrameNum:{}{}",
            &value[..start],
            tagged,
            color_class,
            rating,
            frame_num,
            &value[end..],
        );
    }
    value.to_string()
}

/// Matches `(\d+)` at `pos`, returning the digits and the position after them.
fn scan_digits(bytes: &[u8], pos: usize) -> Option<(&str, usize)> {
    let mut end = pos;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == pos {
        return None;
    }
    // Digits are ASCII, so this slice is always on a char boundary.
    Some((std::str::from_utf8(&bytes[pos..end]).ok()?, end))
}

/// Matches `:\s*` at `pos`, returning the position after it.
fn expect_colon_then_space(bytes: &[u8], pos: usize) -> Option<usize> {
    if bytes.get(pos) != Some(&b':') {
        return None;
    }
    let mut end = pos + 1;
    while end < bytes.len() && bytes[end].is_ascii_whitespace() {
        end += 1;
    }
    Some(end)
}

/// Converts one IPTC dataset payload into the value ExifTool prints for it.
///
/// This is ExifTool's `ProcessIPTC` value pipeline: apply the tag table's
/// `Format`, then its `PrintConv`. Zero-length payloads are still values --
/// ExifTool calls `FoundTag` for them and prints an empty string -- so callers
/// must not treat an empty result as "no tag".
pub fn dataset_value_to_string(record_number: u8, dataset_number: u8, data: &[u8]) -> String {
    // ObjectPreviewData is flagged Binary in IPTC.pm, so ExifTool substitutes a
    // placeholder unless -b was given.
    if record_number == 2 && dataset_number == 202 {
        return format!(
            "(Binary data {} bytes, use -b option to extract)",
            data.len()
        );
    }

    let format = dataset_format(record_number, dataset_number);
    let value = apply_iptc_format(format, data);

    // Formatters that stand in for ExifTool's ValueConv/PrintConv on the
    // date, time and character-set datasets.
    match (record_number, dataset_number) {
        (1, 70) => return format_iptc_date(&value),
        (1, 80) => return format_iptc_time(&value),
        (1, 90) => return format_iptc_coded_charset(strip_trailing_nuls(data)),
        (2, 30 | 37 | 47 | 55 | 62) => return format_iptc_date(&value),
        (2, 35 | 38 | 60 | 63) => return format_iptc_time(&value),
        _ => {}
    }

    match (record_number, dataset_number) {
        (1, 20) | (2, 200) => file_format_print_conv(&value)
            .map(str::to_string)
            .unwrap_or_else(|| unknown_print_conv(&value)),
        // EnvelopePriority (1:60) and Urgency (2:10) share one PrintConv table.
        (1, 60) | (2, 10) => format_iptc_urgency(&value),
        (2, 75) => object_cycle_print_conv(&value)
            .map(str::to_string)
            .unwrap_or_else(|| unknown_print_conv(&value)),
        (2, 221) => prefs_print_conv(&value),
        _ => value,
    }
}

/// Applies an [`IptcFormat`] to a raw dataset payload.
fn apply_iptc_format(format: IptcFormat, data: &[u8]) -> String {
    match format {
        IptcFormat::Int => {
            if data.len() <= 8 {
                let mut value: u64 = 0;
                for &byte in data {
                    value = value * 256 + byte as u64;
                }
                value.to_string()
            } else {
                // ExifTool caps integer conversion at 8 bytes and leaves longer
                // payloads as text.
                decode_iptc_bytes(data)
            }
        }
        IptcFormat::Str | IptcFormat::Digits => decode_iptc_bytes(strip_trailing_nuls(data)),
        IptcFormat::Undef => decode_iptc_bytes(data),
        IptcFormat::Auto => {
            // ExifTool: `$format = 'int' if $len <= 4 and $len != 3 and $val =~ /[\0-\x08]/`
            if data.len() <= 4 && data.len() != 3 && data.iter().any(|&b| b <= 0x08) {
                apply_iptc_format(IptcFormat::Int, data)
            } else {
                decode_iptc_bytes(data)
            }
        }
    }
}

/// Drops the trailing NUL padding some writers add to fixed-width IPTC fields.
fn strip_trailing_nuls(data: &[u8]) -> &[u8] {
    let mut end = data.len();
    while end > 0 && data[end - 1] == 0 {
        end -= 1;
    }
    &data[..end]
}

/// Decodes IPTC bytes as text without altering their content.
///
/// Unlike [`decode_iptc_string`] this preserves surrounding whitespace, which
/// ExifTool also keeps.
fn decode_iptc_bytes(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(text) => text.to_string(),
        // Latin-1: every byte is its own code point.
        Err(_) => data.iter().map(|&b| b as char).collect(),
    }
}

/// Decodes an IPTC string from bytes.
///
/// IPTC strings are typically Latin-1 encoded, but may also be UTF-8.
/// This function attempts UTF-8 first, falls back to Latin-1, and trims whitespace.
pub fn decode_iptc_string(data: &[u8]) -> String {
    // Try UTF-8 first
    if let Ok(s) = std::str::from_utf8(data) {
        return s.trim().to_string();
    }

    // Fall back to Latin-1 (ISO-8859-1)
    // In Latin-1, each byte maps directly to a Unicode code point
    let s: String = data.iter().map(|&b| b as char).collect();
    s.trim().to_string()
}

/// Decodes a raw IPTC IIM block into `IPTC:*` tag/value pairs.
///
/// The block is the payload ExifTool hands to `ProcessIPTC`: a run of datasets,
/// each `0x1C <record> <dataset> <be16 length> <payload>`. The same bytes reach
/// oxidex from two places in a JPEG -- a Photoshop 8BIM resource in APP13, and
/// the IFD0 `IPTC-NAA` tag (0x83BB) -- so both share this decoder.
///
/// Zero-length datasets are emitted, not dropped: ExifTool prints them as empty
/// strings, and a file such as `Canon/CanonEOS-1D.jpg` writes most of its
/// Envelope record that way.
///
/// Datasets IPTC.pm does not name are skipped, matching the `Unknown => 1`
/// entries ExifTool adds for them -- FotoStation alone stuffs two dozen private
/// datasets into the Application record.
pub fn extract_iptc_from_block(data: &[u8]) -> Vec<(String, String)> {
    extract_iptc_entries_from_block(data)
        .into_iter()
        .map(|(_, _, name, value)| (name, value))
        .collect()
}

/// Same as [`extract_iptc_from_block`], but keeps each entry's record and
/// dataset numbers so a caller can tell a repeatable dataset from a
/// single-valued one.
fn extract_iptc_entries_from_block(data: &[u8]) -> Vec<(u8, u8, String, String)> {
    match parse_all_iptc_records(data) {
        Ok(records) => records
            .into_iter()
            .filter_map(|record| {
                let name = known_dataset_name(record.record_number, record.dataset_number)?;
                Some((
                    record.record_number,
                    record.dataset_number,
                    name.to_string(),
                    dataset_value_to_string(
                        record.record_number,
                        record.dataset_number,
                        &record.data,
                    ),
                ))
            })
            .collect(),
        Err(e) => {
            eprintln!("Warning: Failed to parse IPTC records: {}", e);
            Vec::new()
        }
    }
}

/// Collapses IPTC entries into one value per tag, keeping the repeatable
/// datasets as lists.
///
/// `Keywords` and `SupplementalCategories` are written as one IIM record per
/// entry. Inserting them into a map one at a time keeps only the last, which is
/// why `IPTC.jpg` reported a single keyword where ExifTool reports three.
pub fn collapse_iptc_entries(entries: Vec<(u8, u8, String, String)>) -> Vec<(String, TagValue)> {
    let mut out: Vec<(String, TagValue)> = Vec::new();
    let mut lists: Vec<(String, Vec<TagValue>)> = Vec::new();

    for (record_number, dataset_number, name, value) in entries {
        if is_repeatable_iptc_dataset(record_number, dataset_number) {
            match lists.iter_mut().find(|(tag, _)| *tag == name) {
                Some((_, values)) => values.push(TagValue::new_string(value)),
                None => lists.push((name, vec![TagValue::new_string(value)])),
            }
            continue;
        }
        let stored = TagValue::new_string(value);
        match out.iter_mut().find(|(tag, _)| *tag == name) {
            Some(entry) => entry.1 = stored,
            None => out.push((name, stored)),
        }
    }

    for (name, mut values) in lists {
        // ExifTool prints a one-element list as a bare scalar.
        let stored = if values.len() == 1 {
            values.remove(0)
        } else {
            TagValue::Array(values)
        };
        match out.iter_mut().find(|(tag, _)| *tag == name) {
            Some(entry) => entry.1 = stored,
            None => out.push((name, stored)),
        }
    }

    out
}

/// Extracts IPTC metadata from JPEG segments, keeping repeatable datasets as
/// lists. See [`extract_iptc_from_segments`] for the string-valued variant.
pub fn extract_iptc_values_from_segments(segments: &[Segment]) -> Result<Vec<(String, TagValue)>> {
    Ok(collapse_iptc_entries(iptc_entries_from_segments(segments)))
}

/// Extracts IPTC metadata from JPEG segments.
///
/// This function scans through all segments, identifies APP13 segments with
/// the Photoshop signature, extracts IPTC data from 8BIM resource blocks,
/// and parses IPTC IIM records.
///
/// # Parameters
///
/// - `segments`: Slice of parsed JPEG segments (from `parse_segments()`)
///
/// # Returns
///
/// Vector of (tag_name, value) tuples where tag_name is in the format
/// "IPTC:PropertyName" (e.g., "IPTC:ObjectName", "IPTC:By-line").
///
/// Returns an empty vector if no IPTC segments are found (not an error).
///
/// # Errors
///
/// Returns `ParseError` if:
/// - APP13 segment is malformed
/// - 8BIM resource blocks are invalid
/// - IPTC records cannot be parsed
pub fn extract_iptc_from_segments(segments: &[Segment]) -> Result<Vec<(String, String)>> {
    Ok(iptc_entries_from_segments(segments)
        .into_iter()
        .map(|(_, _, name, value)| (name, value))
        .collect())
}

/// Walks the APP13 Photoshop resources and returns every IPTC entry with its
/// record and dataset numbers intact.
fn iptc_entries_from_segments(segments: &[Segment]) -> Vec<(u8, u8, String, String)> {
    let mut all_iptc_tags = Vec::new();

    // Iterate through all segments looking for APP13 segments
    for segment in segments {
        // Check if this is an APP13 segment (0xFFED)
        if segment.marker != APP13_MARKER {
            continue;
        }

        // Check if this APP13 segment contains Photoshop data
        if !segment.data.starts_with(PHOTOSHOP_SIGNATURE) {
            continue;
        }

        // Skip past the Photoshop signature
        let mut current = &segment.data[PHOTOSHOP_SIGNATURE.len()..];

        // Parse all 8BIM resource blocks
        while current.len() > 4 {
            // Check if this looks like a 8BIM block
            if !current.starts_with(EIGHTBIM_SIGNATURE) {
                break;
            }

            match parse_image_resource_block(current) {
                Ok((remaining, block)) => {
                    // Check if this is the IPTC resource block (ID 0x0404)
                    if block.id == IPTC_RESOURCE_ID {
                        all_iptc_tags.extend(extract_iptc_entries_from_block(block.data));
                    }

                    current = remaining;
                }
                Err(_) => {
                    // Failed to parse block, stop processing this segment
                    break;
                }
            }
        }
    }

    all_iptc_tags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_photoshop_signature() {
        assert_eq!(PHOTOSHOP_SIGNATURE, b"Photoshop 3.0\0");
        assert_eq!(PHOTOSHOP_SIGNATURE.len(), 14);
    }

    #[test]
    fn test_8bim_signature() {
        assert_eq!(EIGHTBIM_SIGNATURE, b"8BIM");
        assert_eq!(EIGHTBIM_SIGNATURE.len(), 4);
    }

    #[test]
    fn test_iptc_resource_id() {
        assert_eq!(IPTC_RESOURCE_ID, 0x0404);
    }

    #[test]
    fn test_parse_image_resource_block() {
        // Create a minimal 8BIM resource block
        let mut data = Vec::new();
        data.extend_from_slice(b"8BIM"); // Signature
        data.extend_from_slice(&[0x04, 0x04]); // ID: 0x0404 (IPTC)
        data.push(0x00); // Name: empty Pascal string (length = 0)
        data.push(0x00); // Padding to make name even length
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x04]); // Size: 4 bytes
        data.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4 bytes of data

        let result = parse_image_resource_block(&data);
        assert!(result.is_ok());

        let (remaining, block) = result.unwrap();
        assert_eq!(block.id, 0x0404);
        assert_eq!(block.name, &[] as &[u8]);
        assert_eq!(block.data, &[0xAA, 0xBB, 0xCC, 0xDD]);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_parse_image_resource_block_with_name() {
        let mut data = Vec::new();
        data.extend_from_slice(b"8BIM");
        data.extend_from_slice(&[0x04, 0x04]); // ID
        data.push(0x04); // Name length: 4
        data.extend_from_slice(b"TEST"); // Name: "TEST"
        data.push(0x00); // Padding (4+1 = 5, need 1 byte padding for even)
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x02]); // Size: 2 bytes
        data.extend_from_slice(&[0x11, 0x22]); // Data

        let result = parse_image_resource_block(&data);
        assert!(result.is_ok());

        let (remaining, block) = result.unwrap();
        assert_eq!(block.id, 0x0404);
        assert_eq!(block.name, b"TEST");
        assert_eq!(block.data, &[0x11, 0x22]);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_parse_iptc_record() {
        // Create a minimal IPTC record
        // Record 2, Dataset 5 (ObjectName), Data: "Test"
        let data = vec![
            0x1C, // Tag marker
            0x02, // Record number (Application Record)
            0x05, // Dataset number (ObjectName)
            0x00, 0x04, // Length: 4 bytes
            b'T', b'e', b's', b't', // Data: "Test"
        ];

        let result = parse_iptc_record(&data);
        assert!(result.is_ok());

        let (remaining, record) = result.unwrap();
        assert_eq!(record.record_number, 2);
        assert_eq!(record.dataset_number, 5);
        assert_eq!(record.data, b"Test");
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_parse_multiple_iptc_records() {
        let mut data = Vec::new();

        // Record 1
        data.push(0x1C);
        data.extend_from_slice(&[0x02, 0x05]); // Record 2, Dataset 5
        data.extend_from_slice(&[0x00, 0x05]); // Length: 5
        data.extend_from_slice(b"Title");

        // Record 2
        data.push(0x1C);
        data.extend_from_slice(&[0x02, 0x50]); // Record 2, Dataset 80 (ByLine)
        data.extend_from_slice(&[0x00, 0x06]); // Length: 6
        data.extend_from_slice(b"Author");

        let result = parse_all_iptc_records(&data);
        assert!(result.is_ok());

        let records = result.unwrap();
        assert_eq!(records.len(), 2);

        assert_eq!(records[0].dataset_number, 5);
        assert_eq!(records[0].data, b"Title");

        assert_eq!(records[1].dataset_number, 80);
        assert_eq!(records[1].data, b"Author");
    }

    #[test]
    fn test_dataset_to_tag_name() {
        // Application Record (Record 2)
        assert_eq!(dataset_to_tag_name(2, 0), "IPTC:ApplicationRecordVersion");
        assert_eq!(dataset_to_tag_name(2, 5), "IPTC:ObjectName");
        assert_eq!(dataset_to_tag_name(2, 25), "IPTC:Keywords");
        assert_eq!(dataset_to_tag_name(2, 80), "IPTC:By-line");
        assert_eq!(dataset_to_tag_name(2, 90), "IPTC:City");
        assert_eq!(dataset_to_tag_name(2, 120), "IPTC:Caption-Abstract");

        // Envelope Record (Record 1)
        assert_eq!(dataset_to_tag_name(1, 0), "IPTC:EnvelopeRecordVersion");
        assert_eq!(dataset_to_tag_name(1, 90), "IPTC:CodedCharacterSet");

        // Datasets ExifTool's ApplicationRecord names but oxidex used to miss
        assert_eq!(dataset_to_tag_name(2, 75), "IPTC:ObjectCycle");
        assert_eq!(dataset_to_tag_name(2, 200), "IPTC:ObjectPreviewFileFormat");
        assert_eq!(dataset_to_tag_name(2, 201), "IPTC:ObjectPreviewFileVersion");
        assert_eq!(dataset_to_tag_name(2, 202), "IPTC:ObjectPreviewData");
        assert_eq!(dataset_to_tag_name(2, 221), "IPTC:Prefs");
        assert_eq!(dataset_to_tag_name(2, 230), "IPTC:DocumentNotes");
        assert_eq!(dataset_to_tag_name(2, 255), "IPTC:CatalogSets");

        // Envelope datasets
        assert_eq!(dataset_to_tag_name(1, 20), "IPTC:FileFormat");
        assert_eq!(dataset_to_tag_name(1, 60), "IPTC:EnvelopePriority");

        // Unknown dataset should return generic name
        assert_eq!(dataset_to_tag_name(2, 199), "IPTC:Unknown-2-199");
        assert_eq!(dataset_to_tag_name(3, 5), "IPTC:Unknown-3-5");
    }

    #[test]
    fn envelope_datasets_use_exiftool_formats_and_printconvs() {
        // Canon/CanonEOS-1D.jpg: 1:20 FileFormat is int16u 0x0003 -> TIFF.
        assert_eq!(
            dataset_value_to_string(1, 20, &[0x00, 0x03]),
            "Tagged Image File Format (Adobe/Aldus Image data)"
        );
        // 1:22 FileVersion is int16u with no PrintConv.
        assert_eq!(dataset_value_to_string(1, 22, &[0x00, 0x02]), "2");
        // 1:60 EnvelopePriority is digits[1] sharing Urgency's PrintConv.
        assert_eq!(dataset_value_to_string(1, 60, b"5"), "5 (normal urgency)");
        // NUL-padded fixed-width fields are values, not absences.
        assert_eq!(dataset_value_to_string(1, 5, &[0u8; 16]), "");
        assert_eq!(dataset_value_to_string(1, 40, &[0u8; 8]), "");
        assert_eq!(dataset_value_to_string(1, 70, &[0u8; 8]), "");
        assert_eq!(dataset_value_to_string(1, 80, &[0u8; 11]), "");
        // 1:90 CodedCharacterSet spells out unrecognised ISO 2022 sequences.
        assert_eq!(
            dataset_value_to_string(1, 90, &[0x1B, 0x28, 0x42, 0x1B, 0x26, 0x40]),
            "ESC ( B, ESC & @"
        );
    }

    #[test]
    fn oversized_int_dataset_stays_text() {
        // FotoStation.jpg puts 15-byte strings in 2:200/2:201, both declared
        // int16u. ExifTool caps integer conversion at 8 bytes, so the text
        // survives -- and 2:200's %fileFormat lookup then misses.
        assert_eq!(
            dataset_value_to_string(2, 200, b"Custom Field 01"),
            "Unknown (Custom Field 01)"
        );
        assert_eq!(
            dataset_value_to_string(2, 201, b"Custom Field 02"),
            "Custom Field 02"
        );
        // 2:202 is flagged Binary, so only its length is printed.
        assert_eq!(
            dataset_value_to_string(2, 202, b"Custom Field 03"),
            "(Binary data 15 bytes, use -b option to extract)"
        );
        // 2:75 ObjectCycle: "Afternoon" is not in the a/p/b PrintConv.
        assert_eq!(
            dataset_value_to_string(2, 75, b"Afternoon"),
            "Unknown (Afternoon)"
        );
        assert_eq!(dataset_value_to_string(2, 75, b"a"), "Morning");
    }

    #[test]
    fn prefs_printconv_matches_exiftool() {
        // Olympus/OlympusOM-3.jpg
        assert_eq!(
            dataset_value_to_string(2, 221, b"0:0:5:003344"),
            "Tagged:0, ColorClass:0, Rating:5, FrameNum:003344"
        );
        // Google/GooglePixel9Pro.jpg uses a negative frame number.
        assert_eq!(
            dataset_value_to_string(2, 221, b"1:0:0:-00001"),
            "Tagged:1, ColorClass:0, Rating:0, FrameNum:-00001"
        );
        // Perl's s/// leaves a non-matching value alone.
        assert_eq!(dataset_value_to_string(2, 221, b"not prefs"), "not prefs");
    }

    #[test]
    fn string_datasets_keep_interior_spacing() {
        // ExifTool strips trailing NUL padding and nothing else.
        assert_eq!(dataset_value_to_string(2, 5, b"Title\0\0\0"), "Title");
        assert_eq!(
            dataset_value_to_string(2, 230, b"Document Notes"),
            "Document Notes"
        );
    }

    #[test]
    fn test_decode_iptc_string() {
        // Test ASCII string
        let ascii_data = b"Hello World";
        assert_eq!(decode_iptc_string(ascii_data), "Hello World");

        // Test string with trailing spaces (should be trimmed)
        let padded_data = b"Test    ";
        assert_eq!(decode_iptc_string(padded_data), "Test");
    }

    #[test]
    fn test_extract_iptc_from_segments() {
        // Create a complete APP13 segment with IPTC data
        let mut app13_data = Vec::new();

        // Photoshop signature
        app13_data.extend_from_slice(PHOTOSHOP_SIGNATURE);

        // 8BIM resource block
        app13_data.extend_from_slice(b"8BIM");
        app13_data.extend_from_slice(&[0x04, 0x04]); // ID: IPTC
        app13_data.push(0x00); // Empty name
        app13_data.push(0x00); // Padding

        // IPTC data
        let mut iptc_data = Vec::new();
        // Record: ObjectName (dataset 5)
        iptc_data.push(0x1C);
        iptc_data.extend_from_slice(&[0x02, 0x05]);
        iptc_data.extend_from_slice(&[0x00, 0x0A]);
        iptc_data.extend_from_slice(b"Test Title");

        // Record: By-line (dataset 80)
        iptc_data.push(0x1C);
        iptc_data.extend_from_slice(&[0x02, 0x50]);
        iptc_data.extend_from_slice(&[0x00, 0x0B]);
        iptc_data.extend_from_slice(b"Test Author");

        // Add IPTC data size and data to 8BIM block
        let iptc_size = iptc_data.len() as u32;
        app13_data.extend_from_slice(&iptc_size.to_be_bytes());
        app13_data.extend_from_slice(&iptc_data);

        // Create APP13 segment
        let segment = Segment::new(APP13_MARKER, 0, &app13_data);
        let segments = vec![segment];

        // Extract IPTC
        let result = extract_iptc_from_segments(&segments);
        assert!(result.is_ok());

        let tags = result.unwrap();
        assert_eq!(tags.len(), 2);

        // Check tags
        let title = tags.iter().find(|(k, _)| k == "IPTC:ObjectName");
        assert!(title.is_some());
        assert_eq!(title.unwrap().1, "Test Title");

        let author = tags.iter().find(|(k, _)| k == "IPTC:By-line");
        assert!(author.is_some());
        assert_eq!(author.unwrap().1, "Test Author");
    }

    #[test]
    fn test_extract_iptc_no_app13_segments() {
        // Empty segments
        let segments = vec![];
        let result = extract_iptc_from_segments(&segments);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }
}
