//! Apple (iPhone / iPad) MakerNotes.
//!
//! # Where the directory is
//!
//! `MakerNotes.pm:37-46` dispatches an EXIF 0x927C value beginning `Apple iOS\0`
//! to `%Image::ExifTool::Apple::Main` with
//!
//! ```text
//! Start     => '$valuePtr + 14',
//! Base      => '$start - 14',
//! ByteOrder => 'Unknown',
//! ```
//!
//! so the IFD begins **14** bytes into the value -- `Apple iOS\0` is ten bytes,
//! then two version bytes and the two-byte order marker -- and `Base` resolves
//! to the start of the value, which is what the entries' offsets are measured
//! from. A reader that starts the IFD at byte 10 instead reads the order marker
//! as an entry count and then decodes the first entry out of the count field and
//! the first tag: on `Apple_iPhone13Pro.jpg` that yields a single entry with tag
//! id 0x4d4d ("MM") which no table has, so the file produced *no* Apple tag at
//! all while ExifTool reported 28.
//!
//! `ByteOrder => 'Unknown'` is resolved by `Exif.pm:6982-6993`, which reads the
//! entry count in the enclosing file's order and flips only when it is
//! implausibly large; [`resolve_byte_order`] is that test.
//!
//! # The binary plists
//!
//! Four of the tags do not hold a value, they hold a `bplist00` blob:
//! `0x0003 RunTime` is a `SubDirectory` over `%Apple::RunTime` and
//! `0x0040`/`0x0041`/`0x0042` carry `ValueConv => \&ConvertPLIST`. Both are
//! handled by [`super::shared::binary_plist`], which transcribes
//! `Image::ExifTool::PLIST`.
//!
//! # Tags this reports
//!
//! Every row of [`APPLE_MAIN`] is one entry of `%Apple::Main` (Apple.pm:30-320),
//! with that entry's `Writable` format and its `PrintConv` verbatim. The
//! table's `Unknown => 1` entries (`AEMatrix`, `ImageProcessingFlags`,
//! `QualityHint`, `ImageCaptureRequestID`, `SceneFlags`,
//! `SignalToNoiseRatioType`, `ColorCorrectionMatrix`,
//! `GreenGhostMitigationStatus` and the four `Apple_0x00xx` placeholders) are
//! absent on purpose: ExifTool reports them only under `-u`.

use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::binary_plist;
use super::shared::table_ifd::{self, Conv, OlyVal, TagDef, ftype, read_ifd};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext;

/// `Condition => '$$valPt =~ /^Apple iOS\0/'` (MakerNotes.pm:39).
const APPLE_SIGNATURE: &[u8] = b"Apple iOS\x00";

/// `Start => '$valuePtr + 14'` (MakerNotes.pm:42).
const IFD_START: usize = 14;

/// `%Image::ExifTool::Apple::RunTime` (Apple.pm:324-345): a `bplist00`
/// dictionary whose keys name tags.
const RUNTIME_KEYS: &[(&str, &str)] = &[
    ("timescale", "RunTimeScale"),
    ("epoch", "RunTimeEpoch"),
    ("value", "RunTimeValue"),
    ("flags", "RunTimeFlags"),
];

/// `RunTimeFlags`' `PrintConv => { BITMASK => { ... } }` (Apple.pm:336-343).
const RUNTIME_FLAG_BITS: &[(u32, &str)] = &[
    (0, "Valid"),
    (1, "Has been rounded"),
    (2, "Positive infinity"),
    (3, "Negative infinity"),
    (4, "Indefinite"),
];

/// `%Image::ExifTool::Apple::Main` (Apple.pm:30-320), default-visible entries.
static APPLE_MAIN: &[TagDef] = &[
    // 0x0001 Writable => 'int32s'
    TagDef::raw(0x0001, "MakerNoteVersion"),
    // 0x0004 PrintConv => { 0 => 'No', 1 => 'Yes' }
    TagDef::lookup(0x0004, "AEStable", &[(0, "No"), (1, "Yes")]),
    TagDef::raw(0x0005, "AETarget"),
    TagDef::raw(0x0006, "AEAverage"),
    TagDef::lookup(0x0007, "AFStable", &[(0, "No"), (1, "Yes")]),
    // 0x0008 Writable => 'rational64s', Count => 3
    TagDef::raw(0x0008, "AccelerationVector"),
    // 0x000a PrintConv => { 3 => 'HDR Image', 4 => 'Original Image' }
    TagDef::lookup(
        0x000a,
        "HDRImageType",
        &[(3, "HDR Image"), (4, "Original Image")],
    ),
    TagDef::text(0x000b, "BurstUUID"),
    TagDef {
        id: 0x000c,
        name: "FocusDistanceRange",
        force_type: None,
        conv: Conv::Func(print_focus_distance_range),
    },
    // 0x000f has no PrintConv in Apple.pm -- "seen: 2,3,5" is a comment
    TagDef::raw(0x000f, "OISMode"),
    TagDef::text(0x0011, "ContentIdentifier"),
    // 0x0014 PrintConv (Apple.pm:127-132, #forum15096 / #forum16044)
    TagDef::lookup(
        0x0014,
        "ImageCaptureType",
        &[
            (1, "ProRAW"),
            (2, "Portrait"),
            (10, "Photo"),
            (11, "Manual Focus"),
            (12, "Scene"),
        ],
    ),
    TagDef::text(0x0015, "ImageUniqueID"),
    // 0x0017 has no Writable: the stored field type governs, and real files
    // use both int32s and int64u for it.
    TagDef::raw(0x0017, "LivePhotoVideoIndex"),
    TagDef::raw(0x001d, "LuminanceNoiseAmplitude"),
    TagDef::raw(0x001f, "PhotosAppFeatureFlags"),
    TagDef::raw(0x0021, "HDRHeadroom"),
    TagDef {
        id: 0x0023,
        name: "AFPerformance",
        force_type: None,
        conv: Conv::Func(print_af_performance),
    },
    TagDef::raw(0x0027, "SignalToNoiseRatio"),
    TagDef::text(0x002b, "PhotoIdentifier"),
    TagDef::raw(0x002d, "ColorTemperature"),
    // 0x002e PrintConv (Apple.pm:219-223)
    TagDef::lookup(
        0x002e,
        "CameraType",
        &[(0, "Back Wide Angle"), (1, "Back Normal"), (6, "Front")],
    ),
    TagDef::raw(0x002f, "FocusPosition"),
    TagDef::raw(0x0030, "HDRGain"),
    TagDef::raw(0x0038, "AFMeasuredDepth"),
    TagDef::raw(0x003d, "AFConfidence"),
];

/// `0x000c FocusDistanceRange`'s PrintConv (Apple.pm:98-101):
///
/// ```text
/// my @a = split ' ', $val;
/// sprintf('%.2f - %.2f m', $a[0] <= $a[1] ? @a : reverse @a);
/// ```
///
/// `$val` is the value form, so a `rational64s` pair has already become two
/// quotients by the time the sprintf sees it.
fn print_focus_distance_range(val: &OlyVal) -> Option<String> {
    let OlyVal::Rat(r) = val else { return None };
    if r.len() < 2 {
        return None;
    }
    let q = |(n, d): (i64, i64)| {
        if d == 0 {
            None
        } else {
            Some(n as f64 / d as f64)
        }
    };
    let (a, b) = (q(r[0])?, q(r[1])?);
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    Some(format!("{lo:.2} - {hi:.2} m"))
}

/// `0x0023 AFPerformance`'s PrintConv (Apple.pm:187):
///
/// ```text
/// my @a=split " ",$val; sprintf("%d %d %d",$a[0],$a[1]>>28,$a[1]&0xfffffff)
/// ```
///
/// Perl's `>>` and `&` promote their operands to a 64-bit unsigned, so a
/// negative second element shifts as its two's-complement bit pattern rather
/// than sign-extending.
fn print_af_performance(val: &OlyVal) -> Option<String> {
    let ints = val.ints()?;
    if ints.len() < 2 {
        return None;
    }
    let b = ints[1] as u64;
    Some(format!("{} {} {}", ints[0], b >> 28, b & 0xfff_ffff))
}

/// `ByteOrder => 'Unknown'` as `Exif.pm:6982-6993` resolves it: read the entry
/// count in the enclosing file's order, and flip only when the high byte is
/// non-zero *and* larger than the low byte, which no plausible entry count is.
fn resolve_byte_order(data: &[u8], ifd_start: usize, file_order: ByteOrder) -> ByteOrder {
    let Some(bytes) = data.get(ifd_start..ifd_start + 2) else {
        return file_order;
    };
    let num = match file_order {
        ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]),
        ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]),
    };
    if num & 0xff00 != 0 && (num >> 8) > (num & 0xff) {
        match file_order {
            ByteOrder::BigEndian => ByteOrder::LittleEndian,
            ByteOrder::LittleEndian => ByteOrder::BigEndian,
        }
    } else {
        file_order
    }
}

/// `ExtractObject`'s tag-name generation for a dictionary key with no entry in
/// the table (PLIST.pm:363-368):
///
/// ```text
/// $name =~ s/([^A-Za-z])([a-z])/$1\u$2/g;   # capitalize words
/// $name =~ tr/-_a-zA-Z0-9//dc;              # remove illegal characters
/// $name = 'Tag'.ucfirst($name) if length($name) < 2 or $name =~ /^[-0-9]/;
/// ... { Name => ucfirst($name) }
/// ```
fn generated_tag_name(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let mut name = String::with_capacity(key.len());
    for (i, &c) in chars.iter().enumerate() {
        let prev_non_alpha = i > 0 && !chars[i - 1].is_ascii_alphabetic();
        if prev_non_alpha && c.is_ascii_lowercase() {
            name.push(c.to_ascii_uppercase());
        } else {
            name.push(c);
        }
    }
    name.retain(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let needs_prefix = name.chars().count() < 2
        || name
            .chars()
            .next()
            .is_some_and(|c| c == '-' || c.is_ascii_digit());
    if needs_prefix {
        let mut c = name.chars();
        name = match c.next() {
            Some(f) => format!("Tag{}{}", f.to_ascii_uppercase(), c.as_str()),
            None => "Tag".to_string(),
        };
    }
    let mut c = name.chars();
    match c.next() {
        Some(f) => format!("{}{}", f.to_ascii_uppercase(), c.as_str()),
        None => name,
    }
}

/// `ExifTool::DecodeBits` (ExifTool.pm:6385-6407) over a 32-bit value.
///
/// Delegates to the canonical port (Step 25,
/// `crate::exiftool_tables::decode_bits`) rather than keeping its own copy of
/// the bit-walking loop -- this was one of several near-identical local
/// implementations the generated `PrintConv::Bitmask` schema variant
/// consolidated onto a single, unit-tested definition.
fn decode_bits(value: u64, lookup: &[(u32, &str)]) -> String {
    crate::exiftool_tables::decode_bits(value as i64, lookup)
}

/// Descend into `0x0003 RunTime`: a `bplist00` dictionary each of whose keys
/// names a tag in `%Apple::RunTime`.
fn read_runtime(blob: &[u8], tags: &mut HashMap<String, String>) {
    for (key, obj) in binary_plist::parse_dict(blob) {
        // "next if ref($obj) eq 'HASH'" (PLIST.pm:362) -- and an aggregate has
        // no scalar rendering to report either way.
        let Some(scalar) = obj.scalar() else { continue };
        match RUNTIME_KEYS
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, name)| *name)
        {
            Some("RunTimeFlags") => {
                let binary_plist::PlistValue::Int(bits) = obj else {
                    continue;
                };
                tags.insert(
                    "Apple:RunTimeFlags".to_string(),
                    decode_bits(bits, RUNTIME_FLAG_BITS),
                );
            }
            Some(name) => {
                tags.insert(format!("Apple:{name}"), scalar);
            }
            // A key the table does not name still becomes a tag, under the name
            // ExifTool generates for it (PLIST.pm:359-372).
            None => {
                tags.insert(format!("Apple:{}", generated_tag_name(&key)), scalar);
            }
        }
    }
}

/// Read the Apple MakerNote IFD out of `data`, whose index 0 is the first byte
/// of the `Apple iOS\0` signature -- the base its entries' offsets are measured
/// from (`Base => '$start - 14'`).
fn parse_apple_ifd(data: &[u8], file_order: ByteOrder, tags: &mut HashMap<String, String>) {
    if data.len() < IFD_START + 2 || !data.starts_with(APPLE_SIGNATURE) {
        return;
    }
    let order = resolve_byte_order(data, IFD_START, file_order);
    let Some(entries) = read_ifd(data, IFD_START, order) else {
        return;
    };
    let floor = IFD_START + 2 + entries.len() * 12 + 4;
    for entry in &entries {
        match entry.tag_id {
            // 0x0003 SubDirectory => { TagTable => 'Apple::RunTime' }
            0x0003 => {
                if let Some(blob) = raw_bytes(data, entry, order, floor) {
                    read_runtime(&blob, tags);
                }
            }
            // ValueConv => \&ConvertPLIST (Apple.pm:276, :280, :284)
            0x0040 | 0x0041 | 0x0042 => {
                let name = match entry.tag_id {
                    0x0040 => "SemanticStyle",
                    0x0041 => "SemanticStyleRenderingVer",
                    _ => "SemanticStylePreset",
                };
                if let Some(blob) = raw_bytes(data, entry, order, floor)
                    && let Some(printed) = binary_plist::convert_plist(&blob)
                {
                    tags.insert(format!("Apple:{name}"), printed);
                }
            }
            id => {
                let Some(def) = APPLE_MAIN.iter().find(|d| d.id == id) else {
                    continue;
                };
                let Some(val) = table_ifd::decode_entry_with_floor(
                    data,
                    entry,
                    Some(0),
                    order,
                    def.force_type,
                    floor,
                ) else {
                    continue;
                };
                if let Some(printed) = table_ifd::apply_conv(def, &val) {
                    tags.insert(format!("Apple:{}", def.name), printed);
                }
            }
        }
    }
}

/// The entry's payload as raw bytes, for the tags whose value is a whole
/// `bplist00` blob rather than a number.
fn raw_bytes(
    data: &[u8],
    entry: &table_ifd::RawEntry,
    order: ByteOrder,
    floor: usize,
) -> Option<Vec<u8>> {
    // Forcing `undef` keeps every byte: the blob is not a string and must not
    // be cut at its first NUL.
    match table_ifd::decode_entry_with_floor(
        data,
        entry,
        Some(0),
        order,
        Some(ftype::TIFF_UNDEF),
        floor,
    ) {
        Some(OlyVal::Bytes(b)) => Some(b),
        _ => None,
    }
}

/// Apple MakerNote parser.
pub struct AppleParser;

impl Default for AppleParser {
    fn default() -> Self {
        Self::new()
    }
}

impl AppleParser {
    /// Creates a new Apple parser instance.
    pub fn new() -> Self {
        AppleParser
    }
}

impl MakerNoteParser for AppleParser {
    fn manufacturer_name(&self) -> &'static str {
        "Apple"
    }

    fn tag_prefix(&self) -> &'static str {
        "Apple:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        parse_apple_ifd(data, byte_order, tags);
        Ok(())
    }

    fn parse_with_context(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        _model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // An Apple entry's offsets are measured from the start of the MakerNote
        // value, but nothing bounds them by its declared length, so the window
        // -- the payload extended to the end of the enclosing TIFF block, same
        // index 0 -- is the reach ExifTool has.
        parse_apple_ifd(ctx.window(), byte_order, tags);
        Ok(())
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        data.starts_with(APPLE_SIGNATURE)
    }
}

/// Public entry point for Apple MakerNotes parsing.
pub fn parse_apple_makernotes(
    data: &[u8],
    byte_order: ByteOrder,
    tags: &mut HashMap<String, String>,
) {
    parse_apple_ifd(data, byte_order, tags);
}

/// Whether `data` is an Apple MakerNote.
pub fn is_apple_makernote(data: &[u8]) -> bool {
    data.starts_with(APPLE_SIGNATURE)
}

#[cfg(test)]
mod tests {
    use super::super::shared::table_ifd::print_rational;
    use super::*;

    /// The first 16 bytes of `Apple_iPhone13Pro.jpg`'s MakerNote value:
    /// `Apple iOS\0`, then `00 01`, then the order marker `MM`, then the entry
    /// count `00 31` = 49.
    const HEADER: &[u8] = b"Apple iOS\x00\x00\x01MM\x00\x31";

    #[test]
    fn ifd_starts_fourteen_bytes_in() {
        // Byte 14 is where the count lives; byte 10 is the version word.
        assert_eq!(&HEADER[IFD_START..], &[0x00, 0x31]);
    }

    #[test]
    fn byte_order_follows_exiftools_entry_count_test() {
        // 0x0031 read big-endian: high byte zero, so the file order stands.
        assert_eq!(
            resolve_byte_order(HEADER, IFD_START, ByteOrder::BigEndian),
            ByteOrder::BigEndian
        );
        // The same bytes read little-endian give 0x3100: high byte non-zero and
        // larger than the low byte, so ExifTool flips.
        assert_eq!(
            resolve_byte_order(HEADER, IFD_START, ByteOrder::LittleEndian),
            ByteOrder::BigEndian
        );
    }

    #[test]
    fn validate_header_requires_the_signature() {
        let parser = AppleParser::new();
        assert!(parser.validate_header(HEADER));
        assert!(!parser.validate_header(b"Nikon\x00\x02\x00\x00\x00II\x2a\x00"));
        assert!(!parser.validate_header(&[0x05, 0x00]));
    }

    #[test]
    fn af_performance_splits_the_second_word() {
        // Apple_iPhone13Pro.jpg: int32s[2] = 682, 268435509 -> "682 1 53"
        let v = OlyVal::Int(vec![682, 268_435_509]);
        assert_eq!(print_af_performance(&v).as_deref(), Some("682 1 53"));
        // Apple_iPadPro_12.9-inch_4th_generation.jpg -> "2033332 6 0"
        let v = OlyVal::Int(vec![2_033_332, 1_610_612_736]);
        assert_eq!(print_af_performance(&v).as_deref(), Some("2033332 6 0"));
    }

    #[test]
    fn focus_distance_range_sorts_its_pair() {
        // Apple_iPhone13Pro.jpg: rational64s[2] = 515/128, 37/256, which
        // ExifTool prints as "0.14 - 4.02 m".
        let v = OlyVal::Rat(vec![(515, 128), (37, 256)]);
        assert_eq!(
            print_focus_distance_range(&v).as_deref(),
            Some("0.14 - 4.02 m")
        );
    }

    #[test]
    fn runtime_flags_decode_as_a_bitmask() {
        assert_eq!(decode_bits(1, RUNTIME_FLAG_BITS), "Valid");
        assert_eq!(decode_bits(3, RUNTIME_FLAG_BITS), "Valid, Has been rounded");
        assert_eq!(decode_bits(0, RUNTIME_FLAG_BITS), "(none)");
        assert_eq!(decode_bits(1 << 7, RUNTIME_FLAG_BITS), "[7]");
    }

    #[test]
    fn generated_names_follow_exiftools_rule() {
        assert_eq!(generated_tag_name("timescale"), "Timescale");
        assert_eq!(generated_tag_name("some_key"), "Some_Key");
        assert_eq!(generated_tag_name("a"), "TagA");
        // `s/([^A-Za-z])([a-z])/$1\u$2/` capitalises the letter after the
        // digit before the `Tag` prefix goes on:
        // `perl -e '$_="9lives"; s/([^A-Za-z])([a-z])/$1\u$2/g; print'` gives 9Lives.
        assert_eq!(generated_tag_name("9lives"), "Tag9Lives");
    }

    #[test]
    fn every_table_row_is_a_distinct_apple_pm_id() {
        let mut ids: Vec<u16> = APPLE_MAIN.iter().map(|d| d.id).collect();
        ids.sort_unstable();
        let mut deduped = ids.clone();
        deduped.dedup();
        assert_eq!(ids, deduped, "duplicate tag id in APPLE_MAIN");
        // The four ids handled outside the table are not in it.
        for id in [0x0003u16, 0x0040, 0x0041, 0x0042] {
            assert!(!ids.contains(&id));
        }
    }

    #[test]
    fn rational_printing_matches_exiftool() {
        // HDRGain = 0 (0/1) on Apple_iPhone13Pro.jpg
        assert_eq!(print_rational(0, 1), "0");
        // HDRGain = 0.00989481714 (1349/136334) on Apple_iPhone15Pro.jpg
        assert_eq!(print_rational(1349, 136_334), "0.00989481714");
    }
}
