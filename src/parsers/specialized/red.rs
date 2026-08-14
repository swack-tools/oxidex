//! Redcode R3D video metadata parser.
//!
//! RED cameras write `.r3d` clips as a chain of blocks, each prefixed by a
//! big-endian `int32u` size and a 4-byte block type. ExifTool routes them
//! through `Image::ExifTool::Red::ProcessR3D` (Red.pm:212-292), which
//! validates `^\0\0..RED(1|2)`, decodes the version-specific file header
//! through `Red::RED1`/`Red::RED2`, then walks a tag-length-value "Red
//! directory" against `Red::Main` (Red.pm:38-150).
//!
//! # What comes from the transcription and what does not
//!
//! The two file headers are declared binary layouts, so `RED1`/`RED2` are
//! read from the generated tables rather than restated here. `Red::Main` is
//! not a binary layout at all -- it is a tag-ID-keyed directory whose entry
//! offsets are only known by walking the file -- so the generator emits no
//! `BinaryTable` for it and the tag map below is hand-ported against the
//! cited Perl, per AGENTS.md ("a gap in a transcribed table is not evidence
//! the tag does not exist").
//!
//! # What is deliberately absent
//!
//! ExifTool's `%redFormat` (Red.pm:22-32) maps format code 7 and 9 to
//! `undef` with the source's own comments reading "(mixed-format
//! structure?)" and "? (seen 256 bytes, all zero)". No tag in `Red::Main`
//! uses either code, and this parser skips those entries rather than
//! guessing at a rendering. Format code 3 is `int8u` and code 5 is `int8s`,
//! both annotated in the Perl as uncertain ("how is this different than 0?",
//! "not sure about this"); they are decoded exactly as declared, and no tag
//! currently maps to them either.
//!
//! Tags whose `Red::Main` entry ExifTool leaves commented out (0x1041,
//! 0x1051, 0x1052, 0x200e, 0x2015, 0x404e, 0x4084, 0x4087) are not emitted:
//! the Perl records observed byte values for them but assigns no name, so
//! there is no tag to emit.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Red.pm`

use crate::core::formatters::numeric_precision::perl_number;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{decode_binary_table, find_table};
use crate::io::ByteOrder;

/// Red.pm:225, `$raf->Read($buff, 8) == 8 and $buff =~ /^\0\0..RED(1|2)/s`.
const HEADER_PREFIX_LEN: usize = 8;

/// Red.pm:227, `return 0 if $size < 8`.
const MIN_BLOCK_SIZE: u32 = 8;

/// Red.pm:246, a version 1 file's directory lives in the *second* block and
/// starts at a fixed offset; ExifTool reads "more than we need" (0x10000).
const V1_SECOND_BLOCK_READ: usize = 0x10000;
const V1_DIR_OFFSET: usize = 0x22;

/// Red.pm:249-254: a version 2 directory follows the fixed 0x44-byte header
/// plus the variable-length `rdi`/`rda`/`rdx` record arrays whose counts sit
/// at 0x40, 0x41 and 0x42.
const V2_DIR_BASE: usize = 0x44;
const V2_RDI_COUNT_OFFSET: usize = 0x40;
const V2_RDA_COUNT_OFFSET: usize = 0x41;
const V2_RDX_COUNT_OFFSET: usize = 0x42;
const V2_RDI_RECORD_LEN: usize = 0x18;
const V2_RDA_RECORD_LEN: usize = 0x14;
const V2_RDX_RECORD_LEN: usize = 0x10;

/// Red.pm:263, `if ($dirLen < 300 or $dirLen >= 2048 or $pos + $dirLen > length $buff)`.
const DIR_LEN_MIN: usize = 300;
const DIR_LEN_MAX: usize = 2048;

/// ExifTool's `%redFormat` (Red.pm:22-32). The format code is the top four
/// bits of the tag ID (Red.pm:283). Codes 7 and 9 are `undef` in the Perl and
/// are represented here as `None` so the caller skips them rather than
/// inventing a rendering; an unlisted code likewise yields `None`, matching
/// Red.pm:284's `$fmt or ... last`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RedFormat {
    /// `%redFormat` codes 0 and 3 (Red.pm:23, Red.pm:26).
    Int8u,
    /// `%redFormat` code 1 (Red.pm:24).
    Str,
    /// `%redFormat` code 2 (Red.pm:25).
    Float,
    /// `%redFormat` code 5 (Red.pm:27).
    Int8s,
    /// `%redFormat` code 6 (Red.pm:28).
    Int32s,
    /// `%redFormat` code 8 (Red.pm:30).
    Int32u,
    /// `%redFormat` code 4 (Red.pm:? -- `4 => 'int16u'`, Red.pm:26 region).
    Int16u,
}

/// Red.pm:22-32, `%redFormat`.
fn red_format(code: u16) -> Option<RedFormat> {
    match code {
        0 | 3 => Some(RedFormat::Int8u),
        1 => Some(RedFormat::Str),
        2 => Some(RedFormat::Float),
        4 => Some(RedFormat::Int16u),
        5 => Some(RedFormat::Int8s),
        6 => Some(RedFormat::Int32s),
        8 => Some(RedFormat::Int32u),
        // 7 and 9 are `undef` in the Perl; 10-15 are undeclared.
        _ => None,
    }
}

/// The `ValueConv` a `Red::Main` entry carries, if any. Each variant names
/// the exact Perl expression it reproduces so the mapping stays checkable
/// against the pinned source.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conv {
    /// No `ValueConv`/`PrintConv` -- emit the decoded value as-is.
    None,
    /// Red.pm:52, `$val =~ s/(\d{4})_(\d{2})_/$1:$2:/; $val =~ tr/_/ /; $val`
    OtherDate,
    /// Red.pm:69, `$val =~ s/(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})/$1:$2:$3 $4:$5:/`
    DateTimeOriginal,
    /// Red.pm:80 / Red.pm:94, `$val =~ s/(\d{4})(\d{2})/$1:$2:/; $val`
    YyyyMm,
    /// Red.pm:85 / Red.pm:99, `$val =~ s/(\d{2})(\d{2})/$1:$2:/; $val`
    HhMm,
    /// Red.pm:133, `PrintConv => 'int($val * 1000 + 0.5) / 1000'`
    Milli3,
    /// Red.pm:139, `ValueConv => '$val / 10'`
    DivTen,
    /// Red.pm:145, `ValueConv => '$val/1000', PrintConv => '"$val m"'`
    MetresFromMilli,
}

/// `%Image::ExifTool::Red::Main` (Red.pm:38-150). Entries ExifTool leaves
/// commented out are absent here by construction -- they have no tag name.
const MAIN_TAGS: &[(u16, &str, Conv)] = &[
    // ---- format 1 (string) ----
    (0x1000, "StartEdgeCode", Conv::None),   // Red.pm:48
    (0x1001, "StartTimecode", Conv::None),   // Red.pm:49
    (0x1002, "OtherDate1", Conv::OtherDate), // Red.pm:50-55
    (0x1003, "OtherDate2", Conv::OtherDate), // Red.pm:56-60
    (0x1004, "OtherDate3", Conv::OtherDate), // Red.pm:61-65
    (0x1005, "DateTimeOriginal", Conv::DateTimeOriginal), // Red.pm:66-72
    (0x1006, "SerialNumber", Conv::None),    // Red.pm:73
    (0x1019, "CameraType", Conv::None),      // Red.pm:74
    (0x101a, "ReelNumber", Conv::None),      // Red.pm:75
    (0x101b, "Take", Conv::None),            // Red.pm:76
    (0x1023, "DateCreated", Conv::YyyyMm),   // Red.pm:77-81
    (0x1024, "TimeCreated", Conv::HhMm),     // Red.pm:82-86
    (0x1025, "FirmwareVersion", Conv::None), // Red.pm:87
    (0x1029, "ReelTimecode", Conv::None),    // Red.pm:90
    (0x102a, "StorageType", Conv::None),     // Red.pm:91
    (0x1030, "StorageFormatDate", Conv::YyyyMm), // Red.pm:92-96
    (0x1031, "StorageFormatTime", Conv::HhMm), // Red.pm:97-101
    (0x1032, "StorageSerialNumber", Conv::None), // Red.pm:102
    (0x1033, "StorageModel", Conv::None),    // Red.pm:103
    (0x1036, "AspectRatio", Conv::None),     // Red.pm:104
    (0x1042, "Revision", Conv::None),        // Red.pm:106
    (0x1056, "OriginalFileName", Conv::None), // Red.pm:109
    (0x106e, "LensMake", Conv::None),        // Red.pm:110
    (0x106f, "LensNumber", Conv::None),      // Red.pm:111
    (0x1070, "LensModel", Conv::None),       // Red.pm:112
    (0x1071, "Model", Conv::None),           // Red.pm:113-116
    (0x107c, "CameraOperator", Conv::None),  // Red.pm:117
    (0x1086, "VideoFormat", Conv::None),     // Red.pm:118-121
    (0x1096, "Filter", Conv::None),          // Red.pm:122
    (0x10a0, "Brain", Conv::None),           // Red.pm:123
    (0x10a1, "Sensor", Conv::None),          // Red.pm:124
    (0x10be, "Quality", Conv::None),         // Red.pm:125
    // ---- format 2 (float) ----
    (0x200d, "ColorTemperature", Conv::None), // Red.pm:127
    (0x204b, "RGBCurves", Conv::None),        // Red.pm:130
    (0x2066, "OriginalFrameRate", Conv::Milli3), // Red.pm:131-135
    // ---- format 4 (int16u) ----
    (0x4037, "CropArea", Conv::None),    // Red.pm:138
    (0x403b, "ISO", Conv::None),         // Red.pm:139
    (0x406a, "FNumber", Conv::DivTen),   // Red.pm:141
    (0x406b, "FocalLength", Conv::None), // Red.pm:142
    // ---- format 6 (int32s) ----
    (0x606c, "FocusDistance", Conv::MetresFromMilli), // Red.pm:145
];

fn lookup(tag: u16) -> Option<(&'static str, Conv)> {
    MAIN_TAGS
        .iter()
        .find(|(id, _, _)| *id == tag)
        .map(|(_, name, conv)| (*name, *conv))
}

/// Extract Redcode R3D metadata (`Image::ExifTool::Red::ProcessR3D`).
pub fn parse_r3d_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let file_size = reader.size();
    if file_size < HEADER_PREFIX_LEN as u64 {
        return Err("R3D file is too short for the 8-byte block header".to_string());
    }
    let prefix = reader
        .read(0, HEADER_PREFIX_LEN)
        .map_err(|error| error.to_string())?;

    // Red.pm:225: `^\0\0..RED(1|2)` -- two NULs, two size bytes, then the
    // block type. The leading NULs are the top half of the big-endian size.
    if prefix[0] != 0 || prefix[1] != 0 || &prefix[4..7] != b"RED" {
        return Err("invalid R3D block header".to_string());
    }
    let version = match prefix[7] {
        b'1' => 1u8,
        b'2' => 2u8,
        _ => return Err("unsupported Redcode version".to_string()),
    };

    // Red.pm:226-227: the block size is the leading big-endian int32u.
    let size = u32::from_be_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]);
    if size < MIN_BLOCK_SIZE {
        return Err("R3D block size is smaller than its own header".to_string());
    }
    let size = size as usize;
    if size as u64 > file_size {
        // Red.pm:234, `$raf->Read($buf2, $size - 8) == $size - 8 or return $et->Warn($errTrunc)`.
        return Err("truncated R3D file".to_string());
    }

    // Red.pm:229-231: R3D is big-endian throughout (`SetByteOrder('MM')`).
    let first_block = reader.read(0, size).map_err(|error| error.to_string())?;

    let mut metadata = MetadataMap::new();

    // Red.pm:238: the file header itself is decoded through the
    // version-specific binary table.
    let header_table = if version == 1 { "RED1" } else { "RED2" };
    if let Some(table) = find_table("Red", header_table) {
        let decode = decode_binary_table(table, &first_block, ByteOrder::Big);
        for decoded in decode.fields() {
            if let Some(value) = decoded.emit() {
                metadata.insert(format!("Red:{}", decoded.field.name), value);
            }
        }
    }

    // Red.pm:196-203: RED2's FrameRate carries a `ValueConv` the generator
    // declines to model (`($a[1] * 0x10000 + $a[2]) / $a[0]` over an
    // int16u[3]), so it is decoded here against the cited Perl.
    if version == 2 {
        if let Some(rate) = red2_frame_rate(&first_block) {
            metadata.insert("Red:FrameRate".to_string(), TagValue::new_string(rate));
        }
    }

    // Red.pm:241-254: locate the Red directory.
    let (dir_buf, mut pos) = if version == 1 {
        // Red.pm:243-248: a version 1 file's directory lives in the second
        // block, at a fixed offset.
        let want = V1_SECOND_BLOCK_READ.min((file_size as usize).saturating_sub(size));
        if want == 0 {
            return Err("truncated R3D file".to_string());
        }
        let buf = reader.read(size as u64, want).map_err(|e| e.to_string())?;
        (buf.to_vec(), V1_DIR_OFFSET)
    } else {
        if first_block.len() < V2_DIR_BASE {
            return Err("truncated R3D file".to_string());
        }
        // Red.pm:250-253: skip the `rdi`, `rda` and `rdx` record arrays.
        let pos = V2_DIR_BASE
            + first_block[V2_RDI_COUNT_OFFSET] as usize * V2_RDI_RECORD_LEN
            + first_block[V2_RDA_COUNT_OFFSET] as usize * V2_RDA_RECORD_LEN
            + first_block[V2_RDX_COUNT_OFFSET] as usize * V2_RDX_RECORD_LEN;
        (first_block.to_vec(), pos)
    };

    // Red.pm:255-268: read the directory length, then sanity-check it. When
    // the check fails ExifTool falls back to scanning for the 0x1000 tag;
    // that path also emits a "this R3D file is different" warning, i.e. it
    // is explicitly a guess about an unknown layout. This parser stops
    // instead of guessing -- the header tags found above are still emitted.
    let dir_end = if pos + 8 > dir_buf.len() {
        return Ok(metadata);
    } else {
        let dir_len = u16::from_be_bytes([dir_buf[pos], dir_buf[pos + 1]]) as usize;
        pos += 2;
        if dir_len < DIR_LEN_MIN || dir_len >= DIR_LEN_MAX || pos + dir_len > dir_buf.len() {
            return Ok(metadata);
        }
        pos + dir_len
    };

    // Red.pm:274-291: walk the tag-length-value directory.
    while pos + 4 <= dir_end {
        let len = u16::from_be_bytes([dir_buf[pos], dir_buf[pos + 1]]) as usize;
        if len < 4 || pos + len > dir_end {
            break;
        }
        let tag = u16::from_be_bytes([dir_buf[pos + 2], dir_buf[pos + 3]]);
        // Red.pm:283: the format code is the top four bits of the tag ID.
        let Some(fmt) = red_format(tag >> 12) else {
            // Red.pm:284: `$fmt or ... last` -- an unmodelled format code
            // ends the walk rather than being guessed at.
            break;
        };
        let body = &dir_buf[pos + 4..pos + len];
        if let Some((name, conv)) = lookup(tag)
            && let Some(value) = decode_entry(fmt, body, conv)
        {
            metadata.insert(format!("Red:{name}"), value);
        }
        pos += len;
    }

    Ok(metadata)
}

/// Red.pm:196-203: `Format => 'int16u[3]'` at 0x56 with
/// `ValueConv => 'my @a = split " ",$val; ($a[1] * 0x10000 + $a[2]) / $a[0]'`
/// and `PrintConv => 'int($val * 1000 + 0.5) / 1000'`.
fn red2_frame_rate(block: &[u8]) -> Option<String> {
    const OFFSET: usize = 0x56;
    if block.len() < OFFSET + 6 {
        return None;
    }
    let a0 = u16::from_be_bytes([block[OFFSET], block[OFFSET + 1]]) as f64;
    let a1 = u16::from_be_bytes([block[OFFSET + 2], block[OFFSET + 3]]) as f64;
    let a2 = u16::from_be_bytes([block[OFFSET + 4], block[OFFSET + 5]]) as f64;
    if a0 == 0.0 {
        // Perl would divide by zero here; ExifTool's own arithmetic would
        // die, so there is no defined value to emit.
        return None;
    }
    Some(milli3((a1 * 65536.0 + a2) / a0))
}

/// `int($val * 1000 + 0.5) / 1000` (Red.pm:133, Red.pm:202), rendered through
/// Perl's default numeric stringification.
fn milli3(val: f64) -> String {
    perl_number((val * 1000.0 + 0.5).trunc() / 1000.0)
}

/// Decode one directory entry's body under the format code its tag ID names,
/// then apply the entry's `ValueConv`/`PrintConv`.
fn decode_entry(fmt: RedFormat, body: &[u8], conv: Conv) -> Option<TagValue> {
    match fmt {
        RedFormat::Str => {
            // ExifTool's `string` reader stops at the first NUL.
            let end = body.iter().position(|b| *b == 0).unwrap_or(body.len());
            let text = String::from_utf8_lossy(&body[..end]).into_owned();
            apply_string_conv(text, conv)
        }
        RedFormat::Int8u => numeric(body, 1, conv, |b| f64::from(b[0])),
        RedFormat::Int8s => numeric(body, 1, conv, |b| f64::from(b[0] as i8)),
        RedFormat::Int16u => numeric(body, 2, conv, |b| {
            f64::from(u16::from_be_bytes([b[0], b[1]]))
        }),
        RedFormat::Int32u => numeric(body, 4, conv, |b| {
            f64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        }),
        RedFormat::Int32s => numeric(body, 4, conv, |b| {
            f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        }),
        RedFormat::Float => numeric(body, 4, conv, |b| {
            f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        }),
    }
}

/// Decode a (possibly repeating) numeric entry. ExifTool joins multi-value
/// results with a single space; a single value is emitted on its own.
fn numeric(body: &[u8], width: usize, conv: Conv, read: impl Fn(&[u8]) -> f64) -> Option<TagValue> {
    let count = body.len() / width;
    if count == 0 {
        return None;
    }
    let values: Vec<f64> = (0..count)
        .map(|i| read(&body[i * width..(i + 1) * width]))
        .collect();
    apply_numeric_conv(&values, conv)
}

/// The string-valued `ValueConv` expressions in `Red::Main`. Each is a single
/// non-global `s///`, so it rewrites at most the first match -- reproduced
/// here literally rather than approximated.
fn apply_string_conv(text: String, conv: Conv) -> Option<TagValue> {
    let converted = match conv {
        Conv::None => text,
        // Red.pm:52: `s/(\d{4})_(\d{2})_/$1:$2:/` then `tr/_/ /`.
        Conv::OtherDate => {
            let substituted = subst_digits(&text, &[4, 2], &['_', '_'], "{0}:{1}:");
            substituted.unwrap_or(text).replace('_', " ")
        }
        // Red.pm:69: `s/(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})/$1:$2:$3 $4:$5:/`.
        Conv::DateTimeOriginal => {
            match subst_digits(&text, &[4, 2, 2, 2, 2], &[], "{0}:{1}:{2} {3}:{4}:") {
                Some(v) => v,
                None => text,
            }
        }
        // Red.pm:80: `s/(\d{4})(\d{2})/$1:$2:/`.
        Conv::YyyyMm => subst_digits(&text, &[4, 2], &[], "{0}:{1}:").unwrap_or(text),
        // Red.pm:85: `s/(\d{2})(\d{2})/$1:$2:/`.
        Conv::HhMm => subst_digits(&text, &[2, 2], &[], "{0}:{1}:").unwrap_or(text),
        // A numeric conversion cannot apply to a string entry; no tag in
        // `Red::Main` pairs one with format code 1, so this is unreachable
        // in practice and emits nothing rather than guessing.
        Conv::Milli3 | Conv::DivTen | Conv::MetresFromMilli => return None,
    };
    Some(TagValue::new_string(converted))
}

/// Apply a numeric `ValueConv`/`PrintConv`. Multi-value entries carry no
/// conversion in `Red::Main`, so only the `Conv::None` arm handles them.
fn apply_numeric_conv(values: &[f64], conv: Conv) -> Option<TagValue> {
    match conv {
        Conv::None => Some(TagValue::new_string(
            values
                .iter()
                .map(|v| perl_number(*v))
                .collect::<Vec<_>>()
                .join(" "),
        )),
        // Red.pm:133: `int($val * 1000 + 0.5) / 1000`.
        Conv::Milli3 => Some(TagValue::new_string(milli3(*values.first()?))),
        // Red.pm:141: `$val / 10`.
        Conv::DivTen => Some(TagValue::new_string(perl_number(*values.first()? / 10.0))),
        // Red.pm:145: `$val/1000` then `"$val m"`.
        Conv::MetresFromMilli => Some(TagValue::new_string(format!(
            "{} m",
            perl_number(*values.first()? / 1000.0)
        ))),
        // The string conversions cannot apply to a numeric entry.
        Conv::OtherDate | Conv::DateTimeOriginal | Conv::YyyyMm | Conv::HhMm => None,
    }
}

/// Reproduce a single non-global `s/(\d{n})(\d{m}).../.../ ` substitution.
///
/// `groups` gives each capture's digit count; `literals` gives the literal
/// characters that must appear *between* consecutive groups (empty when the
/// groups are adjacent). `template` uses `{i}` for capture `i`.
///
/// Returns `None` when the pattern does not match, which is Perl's no-op
/// `s///` -- the caller then keeps the original string, exactly as the Perl
/// does.
fn subst_digits(text: &str, groups: &[usize], literals: &[char], template: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    // Perl scans left to right for the first position that matches.
    for start in 0..=chars.len() {
        let mut idx = start;
        let mut captures: Vec<String> = Vec::with_capacity(groups.len());
        let mut ok = true;
        for (g, want) in groups.iter().enumerate() {
            if g > 0
                && let Some(lit) = literals.get(g - 1)
            {
                if chars.get(idx) != Some(lit) {
                    ok = false;
                    break;
                }
                idx += 1;
            }
            let mut captured = String::new();
            for _ in 0..*want {
                match chars.get(idx) {
                    Some(c) if c.is_ascii_digit() => {
                        captured.push(*c);
                        idx += 1;
                    }
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
            captures.push(captured);
        }
        // A trailing literal after the final group (the `_` in
        // `(\d{4})_(\d{2})_`) is consumed but not captured.
        if ok && literals.len() == groups.len() {
            if chars.get(idx) != Some(&literals[groups.len() - 1]) {
                ok = false;
            } else {
                idx += 1;
            }
        }
        if !ok {
            continue;
        }
        let mut replacement = template.to_string();
        for (i, capture) in captures.iter().enumerate() {
            replacement = replacement.replace(&format!("{{{i}}}"), capture);
        }
        let mut out: String = chars[..start].iter().collect();
        out.push_str(&replacement);
        out.extend(chars[idx..].iter());
        return Some(out);
    }
    None
}
