//! Qualcomm APP7 "Camera Attributes" parser
//! (ExifTool `Image::ExifTool::Qualcomm::ProcessQualcomm`).
//!
//! ExifTool routes a JPEG APP7 segment whose payload starts with
//! `\x1aQualcomm Camera Attributes` into `Qualcomm::Main`
//! (ExifTool.pm:8230), skipping the 27-byte signature
//! (`DirStart(\%dirInfo, 27)`, ExifTool.pm:8235) and walking the rest as a
//! sequence of self-describing entries.
//!
//! # Format
//!
//! Little-endian throughout (`SetByteOrder('II')`), one entry after another:
//!
//! ```text
//! int16u valLen      length of the value bytes
//! int8u  tagLen      length of the tag-id string
//! char   tag[tagLen] tag id, e.g. "aec_current_sensor_luma"
//! int8u  fmt         format code, indexes qualcomm_tables::FORMATS
//! int16u cnt1        always 1 in ExifTool's samples; ExifTool ignores both
//! int16u cnt2        counts, so this parser does too
//! byte   val[valLen] the value
//! ```
//!
//! The loop runs `while pos + 3 < end` and stops early when
//! `pos + 8 + tagLen + valLen > end`, which is what keeps a truncated final
//! entry from being reported. Both bounds are ExifTool's own
//! (Qualcomm.pm `ProcessQualcomm`).
//!
//! # Tag naming
//!
//! There is no lookup table to consult. `Qualcomm::Main` declares
//! `VARS => { ID_FMT => 'none', NO_LOOKUP => 1 }` and every one of its 1188
//! entries is an EMPTY hash in the source: the `Name` is generated at module
//! load by `Qualcomm::MakeNameAndDesc`, and ExifTool runs that same function
//! over any id it meets that the table does not list, adding it on the fly.
//! So the names are an algorithm, ported in [`make_name`], not a table --
//! and [`qualcomm_tables::NAME_FIXTURE`] holds ExifTool's own output for all
//! 1188 listed ids so the port can be checked against every one of them.
//!
//! All tags are emitted under the `Qualcomm:` family. `Qualcomm::Main`
//! declares `GROUPS => { 0 => 'MakerNotes', 2 => 'Camera' }` and no family-1
//! group, so ExifTool falls back to the table name for family 1, which is
//! what `-G1` prints.

use super::qualcomm_tables::{self, Fmt};
use crate::core::MetadataMap;
use crate::core::TagValue;
use crate::parsers::jpeg::app_segments::perl_number;

/// Family-1 group ExifTool prints for this table.
const GROUP: &str = "Qualcomm";

/// Bytes between the tag id and the value: the format byte plus the two
/// counts ExifTool reads past (`$pos += 5`, Qualcomm.pm `ProcessQualcomm`).
const HEADER_AFTER_TAG: usize = 5;

/// Applies ExifTool's `Qualcomm::MakeNameAndDesc` to a raw tag id.
///
/// Returns `None` for an id that reduces to the empty string, which is
/// ExifTool's `return 0 unless length` -- such an id is not added to the
/// table and never reaches the output.
///
/// The Perl, in order:
///
/// ```text
/// s/^(asf|awb|aec|afr|af_|la_|r2_tl|tl)/\U$1/ or $_ = ucfirst;
/// s/_([a-z])/_\u$1/g;
/// s/\[(\d+)\]$/sprintf("_%.2d",$1)/e;
/// tr/-_a-zA-Z0-9//dc;
/// my $desc = $_;
/// if ($desc =~ tr/_/ /) {
///     s/_([A-Z][a-z])/$1/g;
///     s/([a-z0-9])_([A-Z])/$1$2/g;
///     s/([A-Za-z])_(\d)/$1$2/g;
/// }
/// ```
pub(crate) fn make_name(id: &str) -> Option<String> {
    // s/^(asf|awb|aec|afr|af_|la_|r2_tl|tl)/\U$1/ or $_ = ucfirst
    // Perl alternation is leftmost-first, so the prefixes are tried in the
    // order they are written; `af_` must lose to `afr` on "afr...".
    const PREFIXES: [&str; 8] = ["asf", "awb", "aec", "afr", "af_", "la_", "r2_tl", "tl"];
    let mut s = match PREFIXES.iter().find(|p| id.starts_with(**p)) {
        Some(p) => format!("{}{}", p.to_ascii_uppercase(), &id[p.len()..]),
        None => {
            let mut c = id.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        }
    };

    // s/_([a-z])/_\u$1/g -- uppercase the letter after each underscore.
    s = uppercase_after_underscore(&s);

    // s/\[(\d+)\]$/sprintf("_%.2d",$1)/e -- a trailing [n] becomes _NN.
    s = trailing_subscript(&s);

    // tr/-_a-zA-Z0-9//dc -- delete everything outside that class.
    s.retain(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if s.is_empty() {
        return None;
    }

    // The description is the underscored form with spaces; the name drops the
    // underscores. `tr/_/ /` is false when there was nothing to replace, and
    // ExifTool leaves the name alone in that case.
    if s.contains('_') {
        s = strip_name_underscores(&s);
    }
    Some(s)
}

/// `s/_([a-z])/_\u$1/g`.
fn uppercase_after_underscore(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        out.push(b[i]);
        if b[i] == '_' && i + 1 < b.len() && b[i + 1].is_ascii_lowercase() {
            out.push(b[i + 1].to_ascii_uppercase());
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

/// `s/\[(\d+)\]$/sprintf("_%.2d",$1)/e`.
///
/// Only a `[digits]` group at the very end is rewritten, and `%.2d` is a
/// minimum width -- a three-digit subscript keeps all three digits.
fn trailing_subscript(s: &str) -> String {
    let Some(open) = s.rfind('[') else {
        return s.to_string();
    };
    if !s.ends_with(']') {
        return s.to_string();
    }
    let digits = &s[open + 1..s.len() - 1];
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    // Perl's %.2d zero-pads to two places but never truncates.
    let n: u64 = match digits.parse() {
        Ok(n) => n,
        Err(_) => return s.to_string(),
    };
    format!("{}_{:02}", &s[..open], n)
}

/// The three name-only substitutions ExifTool applies when the description
/// contained underscores.
fn strip_name_underscores(s: &str) -> String {
    let mut out = remove_if(s, |next, after| {
        // s/_([A-Z][a-z])/$1/g
        next.is_ascii_uppercase() && after.is_some_and(|c| c.is_ascii_lowercase())
    });
    out = remove_if_prev(&out, |prev, next| {
        // s/([a-z0-9])_([A-Z])/$1$2/g
        (prev.is_ascii_lowercase() || prev.is_ascii_digit()) && next.is_ascii_uppercase()
    });
    remove_if_prev(&out, |prev, next| {
        // s/([A-Za-z])_(\d)/$1$2/g
        prev.is_ascii_alphabetic() && next.is_ascii_digit()
    })
}

/// Drops each `_` whose following characters satisfy `keep`.
///
/// Perl's `s///g` resumes scanning after the replacement, so a match cannot
/// overlap the characters it consumed; stepping past them here does the same.
fn remove_if(s: &str, keep: impl Fn(char, Option<char>) -> bool) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '_' && i + 1 < c.len() && keep(c[i + 1], c.get(i + 2).copied()) {
            // The underscore is dropped and the matched run is copied through.
            out.push(c[i + 1]);
            if let Some(&a) = c.get(i + 2) {
                out.push(a);
            }
            i += 3;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

/// Drops each `_` that sits between characters satisfying `keep`.
fn remove_if_prev(s: &str, keep: impl Fn(char, char) -> bool) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(c.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '_'
            && i + 1 < c.len()
            && out.last().is_some_and(|&p| keep(p, c[i + 1]))
        {
            out.push(c[i + 1]);
            i += 2;
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out.into_iter().collect()
}

/// Reads one value the way ExifTool's `ReadValue` does with an undefined
/// count: as many whole elements as `len` holds, joined by a space.
fn read_value(data: &[u8], fmt: Fmt, len: usize) -> TagValue {
    let size = match fmt {
        Fmt::Int8u | Fmt::Int8s => 1,
        Fmt::Int16u | Fmt::Int16s => 2,
        Fmt::Int32u | Fmt::Int32s | Fmt::Float => 4,
        Fmt::Double => 8,
    };
    let count = len / size;
    if count == 0 {
        // ExifTool's ReadValue returns undef when not even one element fits,
        // and an undefined value is not stored.
        return TagValue::Binary(data.to_vec());
    }

    let mut ints: Vec<i64> = Vec::new();
    let mut floats: Vec<f64> = Vec::new();
    for i in 0..count {
        let b = &data[i * size..(i + 1) * size];
        match fmt {
            Fmt::Int8u => ints.push(b[0] as i64),
            Fmt::Int8s => ints.push(b[0] as i8 as i64),
            Fmt::Int16u => ints.push(u16::from_le_bytes([b[0], b[1]]) as i64),
            Fmt::Int16s => ints.push(i16::from_le_bytes([b[0], b[1]]) as i64),
            Fmt::Int32u => ints.push(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64),
            Fmt::Int32s => ints.push(i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64),
            Fmt::Float => floats.push(f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64),
            Fmt::Double => floats.push(f64::from_le_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ])),
        }
    }

    if !floats.is_empty() {
        if floats.len() == 1 {
            return TagValue::String(perl_number(floats[0]));
        }
        return TagValue::String(
            floats
                .iter()
                .map(|f| perl_number(*f))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    if ints.len() == 1 {
        return TagValue::Integer(ints[0]);
    }
    TagValue::String(
        ints.iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// True when this APP7 payload is a Qualcomm Camera Attributes segment.
pub fn is_qualcomm_app7(data: &[u8]) -> bool {
    data.starts_with(qualcomm_tables::SIGNATURE)
}

/// Parses a Qualcomm APP7 segment.
///
/// `data` is the raw APP7 payload, signature included. Returns the tags found,
/// keyed `Qualcomm:<Name>`. A payload that is not Qualcomm's, or that holds no
/// complete entry, yields an empty map -- which is what ExifTool reports for
/// such a segment.
pub fn parse_qualcomm_app7(data: &[u8]) -> MetadataMap {
    let mut out = MetadataMap::new();
    if !is_qualcomm_app7(data) {
        return out;
    }
    let end = data.len();
    let mut pos = qualcomm_tables::DIR_START;

    // ExifTool: while ($pos + 3 < $dirEnd)
    while pos + 3 < end {
        let val_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        let tag_len = data[pos + 2] as usize;

        // ExifTool: last if $pos + 8 + $tagLen + $valLen > $dirEnd
        if pos + 8 + tag_len + val_len > end {
            break;
        }

        let tag = &data[pos + 3..pos + 3 + tag_len];
        pos += 3 + tag_len;
        let fmt_code = data[pos] as usize;
        pos += HEADER_AFTER_TAG;
        let raw = &data[pos..pos + val_len];
        pos += val_len;

        // A tag id that is not valid UTF-8 cannot be named; ExifTool works on
        // bytes but every id it can name is ASCII after the tr/// class filter.
        let Ok(id) = std::str::from_utf8(tag) else {
            continue;
        };
        let Some(name) = make_name(id) else {
            continue;
        };

        let value = match qualcomm_tables::FORMATS.get(fmt_code) {
            Some(&fmt) => read_value(raw, fmt, val_len),
            // ExifTool keeps the raw bytes for an unknown format code.
            None => TagValue::Binary(raw.to_vec()),
        };
        out.insert(format!("{GROUP}:{name}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every id ExifTool lists in `%Qualcomm::Main`, named by the Rust port,
    /// must equal the `Name` ExifTool's own `MakeNameAndDesc` produced for it.
    ///
    /// This is the check that makes the port a port rather than a guess: the
    /// expected column is ExifTool's output, generated by
    /// `scripts/gen_qualcomm_tables.pl`, not a transcription.
    #[test]
    fn make_name_matches_exiftool_for_every_listed_id() {
        let mut bad = Vec::new();
        for (id, want, _desc) in qualcomm_tables::NAME_FIXTURE {
            match make_name(id) {
                Some(got) if got == want => {}
                Some(got) => bad.push(format!("{id}: got {got}, want {want}")),
                None => bad.push(format!("{id}: got None, want {want}")),
            }
        }
        assert!(
            bad.is_empty(),
            "{} of {} ids disagree with ExifTool:\n{}",
            bad.len(),
            qualcomm_tables::NAME_FIXTURE.len(),
            bad.join("\n")
        );
    }

    /// The nine entries of the APP7 segment in ExifTool.jpg, with the values
    /// `exiftool -a -G1 -s` prints for them.
    #[test]
    fn parses_the_exiftool_jpg_segment() {
        let mut seg = Vec::from(qualcomm_tables::SIGNATURE);
        // (tag, format code, little-endian value bytes)
        let entries: [(&str, u8, &[u8]); 9] = [
            ("aec_current_sensor_luma", 0, &[0x16]),
            ("af_position", 5, &[0x1a, 0, 0, 0]),
            ("aec_current_exp_index", 3, &[0x34, 0x01]),
            ("awb_sample_decision", 5, &[0x07, 0, 0, 0]),
            ("asf5_enable", 0, &[0x01]),
            ("asf5_filter_mode", 4, &[0, 0, 0, 0]),
            ("asf5_exposure_index_1", 2, &[0xb4, 0x00]),
            ("asf5_exposure_index_2", 2, &[0x0e, 0x01]),
            ("asf5_max_exposure_index", 2, &[0x22, 0x01]),
        ];
        for (tag, fmt, val) in entries {
            seg.extend_from_slice(&(val.len() as u16).to_le_bytes());
            seg.push(tag.len() as u8);
            seg.extend_from_slice(tag.as_bytes());
            seg.push(fmt);
            seg.extend_from_slice(&1u16.to_le_bytes()); // cnt1
            seg.extend_from_slice(&1u16.to_le_bytes()); // cnt2
            seg.extend_from_slice(val);
        }

        let m = parse_qualcomm_app7(&seg);
        let want = [
            ("Qualcomm:AECCurrentSensorLuma", 22),
            ("Qualcomm:AFPosition", 26),
            ("Qualcomm:AECCurrentExpIndex", 308),
            ("Qualcomm:AWBSampleDecision", 7),
            ("Qualcomm:ASF5Enable", 1),
            ("Qualcomm:ASF5FilterMode", 0),
            ("Qualcomm:ASF5ExposureIndex1", 180),
            ("Qualcomm:ASF5ExposureIndex2", 270),
            ("Qualcomm:ASF5MaxExposureIndex", 290),
        ];
        for (key, value) in want {
            assert_eq!(m.get_integer(key), Some(value), "{key}");
        }
        assert_eq!(m.len(), want.len());
    }

    #[test]
    fn ignores_a_non_qualcomm_payload() {
        assert!(parse_qualcomm_app7(b"PENTAX \0II").is_empty());
        assert!(parse_qualcomm_app7(&[]).is_empty());
    }

    /// A final entry whose declared lengths run past the segment is dropped,
    /// not half-reported: ExifTool's `last if $pos + 8 + ... > $dirEnd`.
    #[test]
    fn drops_a_truncated_final_entry() {
        let mut seg = Vec::from(qualcomm_tables::SIGNATURE);
        seg.extend_from_slice(&64u16.to_le_bytes()); // claims 64 value bytes
        seg.push(3);
        seg.extend_from_slice(b"abc");
        seg.push(0);
        seg.extend_from_slice(&[1, 0, 1, 0]);
        seg.push(0xff); // only one value byte actually present
        assert!(parse_qualcomm_app7(&seg).is_empty());
    }

    /// A format code past the end of the table keeps the raw bytes rather
    /// than inventing a numeric reading.
    #[test]
    fn unknown_format_code_keeps_raw_bytes() {
        let mut seg = Vec::from(qualcomm_tables::SIGNATURE);
        seg.extend_from_slice(&2u16.to_le_bytes());
        seg.push(3);
        seg.extend_from_slice(b"abc");
        seg.push(9); // no such format
        seg.extend_from_slice(&[1, 0, 1, 0]);
        seg.extend_from_slice(&[0xde, 0xad]);
        let m = parse_qualcomm_app7(&seg);
        assert_eq!(m.get("Qualcomm:Abc"), Some(&TagValue::Binary(vec![0xde, 0xad])));
    }

    /// The prefix list is ordered, and `afr` must win over `af_`-style
    /// shortening rules; `tl` must not swallow the front of `r2_tl...`.
    #[test]
    fn prefix_alternation_is_leftmost_first() {
        assert_eq!(make_name("afr_test").unwrap(), "AFRTest");
        assert_eq!(make_name("af_position").unwrap(), "AFPosition");
        assert_eq!(make_name("tl_gamma").unwrap(), "TLGamma");
    }

    /// A subscript becomes a two-digit suffix, and the underscore before it
    /// is dropped from the name.
    #[test]
    fn trailing_subscript_becomes_two_digits() {
        assert_eq!(make_name("asf5_luma_filter[0]").unwrap(), "ASF5LumaFilter00");
    }
}

#[cfg(test)]
mod perl_differential {
    use super::make_name;

    /// Ids checked directly against `Qualcomm::MakeNameAndDesc` under perl:
    ///
    /// ```text
    /// perl -MImage::ExifTool::Qualcomm -e 'my %t;
    ///   Image::ExifTool::Qualcomm::MakeNameAndDesc($_, \%t); print "$t{Name}\n"'
    /// ```
    ///
    /// `r2_tl_x` is the interesting one: the trailing `_X` survives because
    /// `s/([a-z0-9])_([A-Z])/$1$2/g` needs a lowercase or digit before the
    /// underscore and `L` is neither, so the name keeps it.
    #[test]
    fn matches_perl_on_ids_outside_the_table() {
        for (id, want) in [
            ("afr_test", "AFRTest"),
            ("af_position", "AFPosition"),
            ("tl_gamma", "TLGamma"),
            ("asf5_luma_filter[0]", "ASF5LumaFilter00"),
            ("r2_tl_x", "R2TL_X"),
            ("la_gain", "LAGain"),
            ("abc", "Abc"),
            ("x", "X"),
        ] {
            assert_eq!(make_name(id).as_deref(), Some(want), "{id}");
        }
    }
}
