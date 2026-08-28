//! `Composite:LensID` -- ExifTool's `Image::ExifTool::Exif::PrintLensID`
//! (Exif.pm:5881-6060) and the `Image::ExifTool::Canon::PrintLensID`
//! (Canon.pm:10183-10305) it delegates to.
//!
//! # Why this composite does not live in [`super::compute`]
//!
//! Every other Composite is a function of its positional `$val[N]` inputs
//! alone, which is why [`super::compute::compute`] takes nothing but a slice of
//! strings. `LensID` is not: ExifTool's `PrintConv` for it is passed `$self`
//! and immediately reaches back into the ExifTool object for
//! `$$self{TAG_INFO}{LensType}{PrintConv}` (Exif.pm:5326) -- the identity of
//! *which manufacturer's* `LensType` won the bare name, which no positional
//! string can carry. A Samsung body writing `Pentax:LensType` (four such files
//! in the sample corpus) has `Make` = `SAMSUNG` and a Pentax lookup table, so
//! `Make` is not a stand-in for it either. [`super::apply`] therefore resolves
//! that occurrence's own family-0 group once and hands it here.
//!
//! # What is implemented, and what is deliberately absent
//!
//! Absent beats approximate, and a wrong lens name under a real ExifTool tag
//! name is exactly the failure mode this crate refuses (see
//! `super::compute`'s module doc). Each branch below either reproduces
//! ExifTool's arithmetic exactly or returns `None`; the omissions are listed
//! in [`OMITTED`] with their Perl citations.
use super::lens_alternatives::{CANON_LENS_ALTERNATIVES, PENTAX_LENS_ALTERNATIVES};

/// The `PrintLensID` branches this port does not implement, each returning
/// `None` rather than a guess. Kept as data so a test can assert the list is
/// the one the module doc claims.
pub const OMITTED: &[(&str, &str)] = &[
    (
        "SONY bodies",
        "Exif.pm:5915-5952 -- the `$$et{Make} eq 'SONY'` branch rewrites \
         LensType through %Minolta::metabonesID adapter offsets (0xef00/0xbc00/\
         0x7700), a 0x4900 Sigma MC-11 offset into %Sigma::sigmaLensTypes, and \
         a lazily-built %sonyEtype index over %Sony::sonyLensTypes2 keyed \
         `65535.N`. None of those three tables is transcribed in this tree, so \
         the branch cannot be reproduced and is not attempted.",
    ),
    (
        "LensType2 / LensType3 substitution",
        "Exif.pm:5334-5346 -- `$val[9] & 0x8000` swaps the Sony E-mount \
         LensType2 (or LensType3) in for LensType together with its own \
         PrintConv (%sonyLensTypes2), which is not transcribed here. Reached \
         only by Sony bodies, which are omitted above in any case.",
    ),
    (
        "Pentax converter check",
        "Exif.pm:5354-5358 -- `$conv = $val[1] / $val[11]` appends \
         ` + %.1fx converter` when the ratio of FocalLength to LensFocalLength \
         exceeds 1.1. The arithmetic is trivial; its input is not. \
         `Pentax:LensFocalLength` is `%Pentax::LensData` key 9 (Pentax.pm:4504-\
         4508), and LensData sits at a DIFFERENT offset in each of the five \
         LensInfo variants that embed it -- 3, 4, 13, 12 and 15 for \
         `%Pentax::LensInfo` .. `LensInfo5` (Pentax.pm:4218, 4240, 4276, 4312, \
         4349). This tree's Pentax parser hardcodes the LensInfo2 offset \
         (`let ld = &raw[4..4 + 17]`, parsers/tiff/makernotes/pentax.rs:2274, \
         whose own comment says only that layout is decoded), so every body \
         using another variant reads the field from the wrong byte: the 645D \
         reports 3.8 mm where the oracle reports 55.0 mm. Computing the branch \
         from that yields `+ 14.5x converter` on a lens with no converter. No \
         file in the 4238-file corpus has a converter in the oracle's LensID, \
         so the branch is omitted rather than fed a value known to be wrong; \
         the underlying LensFocalLength defect (19 files) is a separate gap in \
         the Pentax parser, not in this composite.",
    ),
    (
        "Panasonic LensTypeMake/LensTypeModel bodies",
        "Olympus.pm:175 and Olympus.pm's Composite `LensType` -- a Panasonic \
         body carrying BOTH `LensTypeMake` and `LensTypeModel` has its bare \
         `LensType` name won by that Composite, whose Require is those two \
         tags and whose ValueConv joins them with a space. It looks the joined \
         pair up in `%olympusLensTypes`, where `'2 20 10'` is `Lumix G Vario \
         12-32mm F3.5-5.6 Asph. Mega OIS`. That table is not transcribed in \
         this tree, and `Panasonic:LensType`'s own raw string is a DIFFERENT, \
         less specific value (`LUMIX G VARIO 12-32/F3.5-5.6`), so the \
         plain-string path is refused for these 17 corpus bodies rather than \
         allowed to stand in for the curated name. A zero make does NOT exempt \
         a body -- `PanasonicDC-GH7.jpg` is `0 20 10`. Bodies missing either \
         half (the fixed-lens compacts, and the DC-S full-frame line, which \
         writes LensTypeModel alone) are unaffected and still resolve exactly \
         -- 460 of the 477 Panasonic files score LensID correct.",
    ),
    (
        "strings carrying an unterminated tail",
        "Not an ExifTool branch but an oxidex input defect this composite \
         must not launder: `ExifIFD:LensModel` on SamsungNX3000.jpg and \
         SamsungNX3300.jpg is stored with its NUL terminator and ~90 bytes of \
         following makernote bytes still attached, so LensModel itself is \
         already scored WRONG in the before-state (2 files). Feeding that to \
         either LensID row yields the real lens name with binary garbage \
         welded to its end, which is a plausible-but-wrong value under a real \
         tag name. Any input containing a NUL is refused here; the underlying \
         EXIF ASCII-termination bug is a separate gap in the string reader.",
    ),
    (
        "user-defined lenses",
        "Exif.pm:5998-6009 and Canon.pm:10242-10257 -- `%Image::ExifTool::\
         userLens`, populated from a user's .ExifTool_config. oxidex has no \
         such configuration file, so the list is empty by construction and \
         both blocks are unreachable rather than unimplemented.",
    ),
];

/// One candidate lens name, parsed by ExifTool's `GetLensInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LensInfo {
    /// short (minimum) focal length
    sf: f64,
    /// long (maximum) focal length
    lf: f64,
    /// maximum aperture at the short focal length
    sa: f64,
    /// maximum aperture at the long focal length
    la: f64,
}

/// ExifTool's `Image::ExifTool::Exif::GetLensInfo` (Exif.pm:5825-5843), the
/// `$unk` = false form (no caller in the `LensID` path passes it):
///
/// ```text
///     my $pat = '\\d+(?:\\.\\d+)?';
///     return () unless $lens =~ /($pat)(?:-($pat))?\s*mm.*?(?:[fF]\/?\s*)($pat)(?:-($pat))?/;
///     my @a = ($1, $2, $3, $4);
///     $a[1] or $a[1] = $a[0];
///     $a[3] or $a[3] = $a[2];
/// ```
///
/// Hand-written rather than regex-crate-driven so the crate stays
/// dependency-free at this layer; the scan below is the same greedy
/// left-to-right match Perl performs, with `.*?` lazily advancing the aperture
/// search from the first `mm` onwards.
fn get_lens_info(lens: &str) -> Option<LensInfo> {
    let b = lens.as_bytes();
    // ($pat)(?:-($pat))?\s*mm -- scan for the leftmost position where a number,
    // an optional `-number`, optional space and a literal `mm` all match, which
    // is what Perl's leftmost-match rule picks.
    let mut start = 0usize;
    while start < b.len() {
        let Some((sf, mut i)) = number_at(b, start) else {
            start += 1;
            continue;
        };
        let mut lf = None;
        if b.get(i) == Some(&b'-') {
            if let Some((v, j)) = number_at(b, i + 1) {
                lf = Some(v);
                i = j;
            }
        }
        let mut j = i;
        while b.get(j).is_some_and(|c| c.is_ascii_whitespace()) {
            j += 1;
        }
        if b[j..].starts_with(b"mm") {
            // .*?(?:[fF]\/?\s*)($pat)(?:-($pat))? -- lazily find the first
            // aperture marker at or after the `mm`.
            if let Some(info) = aperture_after(b, j + 2, sf, lf.unwrap_or(sf)) {
                return Some(info);
            }
            // Perl would backtrack into a later `mm`; continue the outer scan
            // from just past this one, which is the same set of candidate
            // positions.
            start = j + 2;
            continue;
        }
        start += 1;
    }
    None
}

/// `(?:[fF]\/?\s*)($pat)(?:-($pat))?` applied at the first position at or after
/// `from` where it matches.
fn aperture_after(b: &[u8], from: usize, sf: f64, lf: f64) -> Option<LensInfo> {
    let mut k = from;
    while k < b.len() {
        if b[k] == b'f' || b[k] == b'F' {
            let mut m = k + 1;
            if b.get(m) == Some(&b'/') {
                m += 1;
            }
            while b.get(m).is_some_and(|c| c.is_ascii_whitespace()) {
                m += 1;
            }
            if let Some((sa, mut n)) = number_at(b, m) {
                let mut la = sa;
                if b.get(n) == Some(&b'-') {
                    if let Some((v, p)) = number_at(b, n + 1) {
                        la = v;
                        n = p;
                    }
                }
                let _ = n;
                return Some(LensInfo { sf, lf, sa, la });
            }
        }
        k += 1;
    }
    None
}

/// `\d+(?:\.\d+)?` anchored at `i`; returns the value and the index just past
/// it. Perl's `\d+` is greedy and does not accept a leading sign here.
fn number_at(b: &[u8], i: usize) -> Option<(f64, usize)> {
    let mut j = i;
    while b.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    if j == i {
        return None;
    }
    let mut end = j;
    if b.get(j) == Some(&b'.') {
        let mut k = j + 1;
        while b.get(k).is_some_and(u8::is_ascii_digit) {
            k += 1;
        }
        if k > j + 1 {
            end = k;
        }
    }
    let s = std::str::from_utf8(&b[i..end]).ok()?;
    Some((s.parse().ok()?, end))
}

/// ExifTool's `Image::ExifTool::Exif::MatchLensModel` (Exif.pm:5847-5872):
/// narrow a candidate list by whatever the camera itself wrote as `LensModel`.
/// "guaranteed not to remove all list entries" -- each filter is applied only
/// when it leaves a non-empty, strictly smaller list.
fn match_lens_model(try_: &mut Vec<String>, lens_model: Option<&str>) {
    let Some(model) = lens_model.filter(|m| !m.is_empty()) else {
        return;
    };
    if try_.len() <= 1 {
        return;
    }
    // if ($lensModel =~ /((\d+-)?\d+mm)/) { grep /$focal/ }
    if let Some(focal) = find_focal_token(model) {
        let filt: Vec<String> = try_
            .iter()
            .filter(|l| l.contains(&focal))
            .cloned()
            .collect();
        if !filt.is_empty() && filt.len() < try_.len() {
            *try_ = filt;
        }
    }
    // if (@$try > 1 and $lensModel =~ m{(?:F/?|1:)(\d+(\.\d+)?)}i) {
    //     grep m{(F/?|1:)$fnum(\b|[A-Z])}i }
    if try_.len() > 1 {
        if let Some(fnum) = find_fnumber_token(model) {
            let filt: Vec<String> = try_
                .iter()
                .filter(|l| fnumber_matches(l, &fnum))
                .cloned()
                .collect();
            if !filt.is_empty() && filt.len() < try_.len() {
                *try_ = filt;
            }
        }
    }
    // foreach $pat ('I+', 'USM') { next unless @$try > 1 and $lensModel =~
    //     /\b($pat)\b/; grep /\b$val\b/ }
    for pat in ["I+", "USM"] {
        if try_.len() <= 1 {
            break;
        }
        let Some(val) = word_match(model, pat) else {
            continue;
        };
        let filt: Vec<String> = try_
            .iter()
            .filter(|l| contains_word(l, &val))
            .cloned()
            .collect();
        if !filt.is_empty() && filt.len() < try_.len() {
            *try_ = filt;
        }
    }
}

/// `((\d+-)?\d+mm)` -- the leftmost focal-length token in a LensModel string.
fn find_focal_token(s: &str) -> Option<String> {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if !b[i].is_ascii_digit() {
            continue;
        }
        if i > 0 && b[i - 1].is_ascii_digit() {
            continue;
        }
        // Try the `\d+-` prefix first, mirroring Perl's greedy optional group.
        for with_prefix in [true, false] {
            let mut j = i;
            if with_prefix {
                while b.get(j).is_some_and(u8::is_ascii_digit) {
                    j += 1;
                }
                if b.get(j) != Some(&b'-') {
                    continue;
                }
                j += 1;
                if !b.get(j).is_some_and(u8::is_ascii_digit) {
                    continue;
                }
            }
            let k0 = j;
            while b.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            if j == k0 {
                continue;
            }
            if b[j..].starts_with(b"mm") {
                return Some(s[i..j + 2].to_string());
            }
        }
    }
    None
}

/// `(?:F/?|1:)(\d+(\.\d+)?)` case-insensitively -- the leftmost f-number token.
fn find_fnumber_token(s: &str) -> Option<String> {
    let b = s.as_bytes();
    for i in 0..b.len() {
        let j = if b[i] == b'f' || b[i] == b'F' {
            i + 1 + usize::from(b.get(i + 1) == Some(&b'/'))
        } else if b[i..].starts_with(b"1:") {
            i + 2
        } else {
            continue;
        };
        if let Some((_, end)) = number_at(b, j) {
            return Some(s[j..end].to_string());
        }
    }
    None
}

/// `m{(F/?|1:)$fnum(\b|[A-Z])}i` -- does this candidate carry the same
/// f-number, followed by a word boundary or an uppercase letter?
fn fnumber_matches(lens: &str, fnum: &str) -> bool {
    let b = lens.as_bytes();
    let f = fnum.as_bytes();
    for i in 0..b.len() {
        let j = if b[i] == b'f' || b[i] == b'F' {
            i + 1 + usize::from(b.get(i + 1) == Some(&b'/'))
        } else if b[i..].starts_with(b"1:") {
            i + 2
        } else {
            continue;
        };
        if !b[j..].starts_with(f) {
            continue;
        }
        let k = j + f.len();
        // `\b` after a digit means the next byte is not a word character;
        // `[A-Z]` is the explicit alternative Perl spells out.
        match b.get(k) {
            None => return true,
            Some(&c) if c.is_ascii_uppercase() => return true,
            Some(&c) if !(c.is_ascii_alphanumeric() || c == b'_') => return true,
            _ => {}
        }
    }
    false
}

/// `$lensModel =~ /\b($pat)\b/` for the two literal patterns ExifTool uses:
/// `I+` (one or more capital I) and `USM`.
fn word_match(s: &str, pat: &str) -> Option<String> {
    let b = s.as_bytes();
    if pat == "USM" {
        let mut i = 0;
        while i + 3 <= b.len() {
            if &b[i..i + 3] == b"USM" && at_word_boundary(b, i, i + 3) {
                return Some("USM".to_string());
            }
            i += 1;
        }
        return None;
    }
    // 'I+': greedy run of capital I, word-bounded on both sides.
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'I' {
            let mut j = i;
            while b.get(j) == Some(&b'I') {
                j += 1;
            }
            if at_word_boundary(b, i, j) {
                return Some("I".repeat(j - i));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    None
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn at_word_boundary(b: &[u8], start: usize, end: usize) -> bool {
    let before = start == 0 || !is_word(b[start - 1]);
    let after = end >= b.len() || !is_word(b[end]);
    before && after
}

/// `/\b$val\b/` over a candidate lens name.
fn contains_word(lens: &str, val: &str) -> bool {
    let b = lens.as_bytes();
    let v = val.as_bytes();
    if v.is_empty() || v.len() > b.len() {
        return false;
    }
    (0..=b.len() - v.len()).any(|i| &b[i..i + v.len()] == v && at_word_boundary(b, i, i + v.len()))
}

/// ExifTool's `Image::ExifTool::Canon::LensWithTC` (Canon.pm:10120-10136):
///
/// ```text
///     if (not $lens =~ /x$/ and $lens =~ /(\d+)/) {
///         my $sf = $1;
///         foreach $tc (1, 1.4, 2, 2.8) {
///             next if abs($shortFocal - $sf * $tc) > 0.9;
///             $lens .= " + ${tc}x" if $tc > 1;
///             last;
///         }
///     }
/// ```
fn lens_with_tc(lens: &str, short_focal: f64) -> String {
    if lens.ends_with('x') {
        return lens.to_string();
    }
    let b = lens.as_bytes();
    let Some(i) = b.iter().position(u8::is_ascii_digit) else {
        return lens.to_string();
    };
    // `(\d+)` -- integer digits only, no decimal part.
    let mut j = i;
    while b.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    let Ok(sf) = lens[i..j].parse::<f64>() else {
        return lens.to_string();
    };
    for (tc, label) in [(1.0, "1"), (1.4, "1.4"), (2.0, "2"), (2.8, "2.8")] {
        if (short_focal - sf * tc).abs() > 0.9 {
            continue;
        }
        return if tc > 1.0 {
            format!("{lens} + {label}x")
        } else {
            lens.to_string()
        };
    }
    lens.to_string()
}

/// The candidate list for one `LensType` print string: ExifTool's
/// `$lens =~ s/ or .*//s` plus the fractional-key alternatives.
///
/// Returns `None` when the string carries no alternatives at all, which is the
/// `unless $$printConv{"$lensType.1"}` early return in both `PrintLensID`s.
fn candidates(
    table: &'static [(&'static str, &'static [&'static str])],
    lens: &str,
) -> Option<Vec<String>> {
    let alts = table.iter().find(|(base, _)| *base == lens)?.1;
    let mut out = vec![strip_or(lens).to_string()];
    out.extend(alts.iter().map(|s| (*s).to_string()));
    Some(out)
}

/// `s/ or .*//s` -- everything from the first " or " onwards.
fn strip_or(lens: &str) -> &str {
    match lens.find(" or ") {
        Some(i) => &lens[..i],
        None => lens,
    }
}

/// Which manufacturer's `LensType` lookup won the bare `LensType` name, as
/// determined by the family-0 group of the occurrence [`super::apply`]
/// resolved. This is the Rust stand-in for ExifTool's
/// `$$self{TAG_INFO}{LensType}{PrintConv}` (Exif.pm:5326, 5890).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LensTable {
    /// `%canonLensTypes` -- a HASH PrintConv, with fractional keys.
    Canon,
    /// `%pentaxLensTypes` -- a HASH PrintConv, with fractional keys.
    Pentax,
    /// `%olympusLensTypes` -- a HASH PrintConv with *no* fractional keys at
    /// all (verified against the pinned tree: 143 keys, 0 fractional), so
    /// every id resolves to the string already stored.
    Olympus,
    /// No `PrintConv` at all: `%Panasonic::Main` key 0x51 is a plain string
    /// (Panasonic.pm:945-949, `Writable => 'string'`, `ValueConv` trims
    /// trailing spaces and nothing else). This is `PrintLensID`'s
    /// `unless (ref $printConv eq 'HASH')` fall-through.
    None_,
}

impl LensTable {
    /// Map the winning `LensType` occurrence's family-0 group onto its table.
    ///
    /// Groups not listed here return `None` and the composite does not fire --
    /// see [`OMITTED`]. `Sony`, `Nikon`, `Leica`, `Minolta`, `Sigma`,
    /// `Samsung` and `Ricoh` all reach `PrintLensID` through tables this tree
    /// has not transcribed the fractional half of.
    fn for_group(group: &str) -> Option<Self> {
        match group {
            "Canon" => Some(Self::Canon),
            "Pentax" => Some(Self::Pentax),
            "Olympus" => Some(Self::Olympus),
            "Panasonic" => Some(Self::None_),
            // A Panasonic/Olympus body that writes LensTypeMake +
            // LensTypeModel instead of a single LensType has the bare name won
            // by `Composite:LensType` (Olympus.pm:4313-4322), whose own
            // `PrintConv => \%olympusLensTypes` is the hash `PrintLensID` then
            // gets handed. That is the ONLY Composite in the generated table
            // named `LensType` (asserted by `only_one_composite_lens_type`
            // below), so the mapping is unambiguous rather than a guess.
            "Composite" => Some(Self::Olympus),
            _ => None,
        }
    }

    fn alternatives(self) -> Option<&'static [(&'static str, &'static [&'static str])]> {
        match self {
            Self::Canon => Some(&CANON_LENS_ALTERNATIVES),
            Self::Pentax => Some(&PENTAX_LENS_ALTERNATIVES),
            Self::Olympus => Some(&[]),
            Self::None_ => None,
        }
    }
}

/// The positional inputs of `%Image::ExifTool::Exif::Composite{LensID}`
/// (Exif.pm:5303-5360), named so the port below reads like the Perl.
struct Args<'a> {
    lens_type: &'a str,
    focal_length: Option<f64>,
    max_aperture: Option<f64>,
    max_aperture_value: Option<f64>,
    short_focal: Option<f64>,
    long_focal: Option<f64>,
    lens_model: Option<&'a str>,
    lens_focal_range: Option<&'a str>,
    lens_focal_length: Option<f64>,
    rf_lens_type: Option<&'a str>,
}

/// Compute `Composite:LensID` from the primary (`Require => 'LensType'`)
/// definition, Exif.pm:5303-5360.
///
/// `lens_type_group` is the family-0 group of the occurrence that won the bare
/// `LensType` name; `inputs` are the composite's `$val[0..12]` in ExifTool's
/// own order. `olympus_lens_type_pair` is true when the file carries BOTH
/// `LensTypeMake` and `LensTypeModel` -- see [`OMITTED`]'s "Panasonic
/// LensTypeMake/LensTypeModel bodies" entry.
pub(super) fn compute_primary(
    inputs: &[Option<&str>],
    lens_type_group: Option<&str>,
    make: Option<&str>,
    olympus_lens_type_pair: bool,
) -> Option<String> {
    let get = |i: usize| inputs.get(i).copied().flatten();
    let f = super::compute::f;

    let lens_type = get(0)?;
    // A NUL byte means the string was never terminated where its own format
    // says it ends, so everything after it is unrelated bytes rather than lens
    // text -- see OMITTED's "strings carrying an unterminated tail".
    if inputs.iter().flatten().any(|s| s.contains('\0')) {
        return None;
    }
    // Exif.pm:5915 -- `if ($$et{Make} eq 'SONY')`. Omitted; see OMITTED.
    if make.is_some_and(|m| m == "SONY") {
        return None;
    }
    // Exif.pm:5334-5346 -- LensType2/LensType3 substitution. Omitted; the only
    // bodies that write them are Sony, already refused above, but refuse
    // explicitly rather than by implication.
    if get(9).is_some() || get(10).is_some() {
        return None;
    }

    let table = LensTable::for_group(lens_type_group?)?;

    // A Panasonic body carrying BOTH LensTypeMake and LensTypeModel: ExifTool
    // joins them (`"$val[0] $val[1]"`) and resolves the pair through
    // `%olympusLensTypes` -- Olympus.pm's Composite `LensType`
    // (`Require => {0 => 'LensTypeMake', 1 => 'LensTypeModel'}`), whose
    // Olympus.pm:175 entry is `'2 20 10' => 'Lumix G Vario 12-32mm F3.5-5.6
    // Asph. Mega OIS'`. That Composite then wins the bare `LensType` name over
    // Panasonic's own plain string. The table is not transcribed here, so the
    // curated name is unreachable and the raw string is not a substitute for
    // it -- see OMITTED. Note a zero *make* still counts: `PanasonicDC-GH7.jpg`
    // is `0 20 10` and the oracle still answers from the Olympus table.
    if table == LensTable::None_ && olympus_lens_type_pair {
        return None;
    }

    // RawConv (Exif.pm:5326-5330):
    //     return $val if ref $printConv eq 'HASH' or (ARRAY of HASH)
    //                 or $val[0] =~ /(mm|\d\/F)/;
    //     return undef;
    if table == LensTable::None_ && !has_mm_or_slash_f(lens_type) {
        return None;
    }

    let args = Args {
        lens_type,
        focal_length: f(get(1)),
        max_aperture: f(get(2)),
        max_aperture_value: f(get(3)),
        short_focal: f(get(4)),
        long_focal: f(get(5)),
        lens_model: get(6),
        lens_focal_range: get(7),
        lens_focal_length: f(get(11)),
        rf_lens_type: get(12),
    };

    // PrintConv (Exif.pm:5347-5351): a Canon RFLensType displaces LensType,
    // together with its own PrintConv -- which is the same %canonLensTypes
    // hash (Canon.pm:7048's RFLensType shares `SeparateTable => 'canonLensTypes'`
    // ... in 13.59 it is `PrintConv => \%canonLensTypes` on the RF ids), so the
    // table selection is unchanged.
    //
    // `if ($val[12])` is Perl truth on the *value*, and oxidex stores
    // RFLensType's print form: the falsy value 0 prints "n/a". Refuse rather
    // than guess whether a non-"n/a" print came from a zero value.
    let (lens_type_prt, table) = match args.rf_lens_type {
        Some(rf) if rf != "n/a" && !rf.is_empty() => (rf, LensTable::Canon),
        _ => (args.lens_type, table),
    };

    let lens = print_lens_id(lens_type_prt, table, &args)?;

    // Exif.pm:5354-5358 -- the Pentax K-3 teleconverter check:
    //     if ($val[11] and $val[1] and $lens) {
    //         my $conv = $val[1] / $val[11];
    //         $lens .= sprintf(' + %.1fx converter', $conv) if $conv > 1.1;
    //     }
    //
    // NOT implemented, because `$val[11]` (`LensFocalLength`) is not decoded
    // correctly in this tree and the branch is pure arithmetic on it -- see
    // OMITTED's "Pentax converter check" entry. Applying it to the value oxidex
    // currently has appends a converter to eight corpus files that the oracle
    // says have none (`Pentax645D.jpg`: oxidex `LensFocalLength` 3.8 mm vs the
    // oracle's 55.0 mm, so `55/3.8 = 14.5` and the tag reads
    // `... SDM AW + 14.5x converter`). Returning the bare lens name is the
    // exact answer for every one of them.
    let _ = args.lens_focal_length;
    Some(lens)
}

/// `$val[0] =~ /(mm|\d\/F)/`
fn has_mm_or_slash_f(s: &str) -> bool {
    if s.contains("mm") {
        return true;
    }
    let b = s.as_bytes();
    (0..b.len().saturating_sub(2)).any(|i| b[i].is_ascii_digit() && &b[i + 1..i + 3] == b"/F")
}

/// `Image::ExifTool::Exif::PrintLensID` (Exif.pm:5881-6060), minus the SONY
/// branch and the userLens blocks (see [`OMITTED`]).
fn print_lens_id(lens_type_prt: &str, table: LensTable, args: &Args) -> Option<String> {
    // Exif.pm:5893-5902 -- `unless (ref $printConv eq 'HASH')`. The ARRAY-of-
    // HASH sub-branch is Sony-only and already refused upstream.
    let Some(alt_table) = table.alternatives() else {
        // return $lensTypePrt if $lensTypePrt =~ /mm/;
        if lens_type_prt.contains("mm") {
            return Some(lens_type_prt.to_string());
        }
        // return $lensTypePrt if $lensTypePrt =~ s/(\d)\/F/$1mm F/;
        let (sub, changed) = subst_digit_slash_f(lens_type_prt);
        return changed.then_some(sub);
    };

    // Exif.pm:5910 -- `$maxAperture = $maxApertureValue unless $maxAperture;`
    let max_aperture = match args.max_aperture {
        Some(v) if v != 0.0 => Some(v),
        _ => args.max_aperture_value,
    };
    // Exif.pm:5911-5913 -- LensFocalRange overrides Min/MaxFocalLength:
    //     if ($lensFocalRange =~ /^(\d+)(?: (?:to )?(\d+))?$/) {
    //         ($shortFocal, $longFocal) = ($1, $2 || $1);
    //     }
    let (mut short_focal, mut long_focal) = (args.short_focal, args.long_focal);
    if let Some(range) = args.lens_focal_range {
        if let Some((a, b)) = parse_focal_range(range) {
            short_focal = Some(a);
            long_focal = Some(b);
        }
    }

    // Exif.pm:5954-5960 -- the non-SONY delegation:
    //     } elsif ($shortFocal and $longFocal and
    //              (not $lensModel or $lensModel !~ /^TAMRON.*-\d+mm/)) {
    //         return Image::ExifTool::Canon::PrintLensID(...);
    //     }
    let nonzero = |v: Option<f64>| v.filter(|x| *x != 0.0);
    if let (Some(sf), Some(lf)) = (nonzero(short_focal), nonzero(long_focal)) {
        let tamron = args
            .lens_model
            .is_some_and(|m| m.starts_with("TAMRON") && tamron_dash_mm(m));
        if !tamron {
            return canon_print_lens_id(
                table,
                lens_type_prt,
                sf,
                lf,
                max_aperture,
                args.lens_model,
            );
        }
    }

    // Exif.pm:5962-6060 -- the generic narrowing path.
    //     my $lens = $$printConv{$lensType};
    //     return ($lensModel || $lensTypePrt) unless $lens;
    //     return $lens unless $$printConv{"$lensType.1"};
    //
    // `$$printConv{$lensType}` IS `$lensTypePrt` here: oxidex stores the
    // PrintConv result, and an id the table does not carry is stored as
    // "Unknown (N)" -- for which ExifTool's `$lens` is undef and the answer is
    // `$lensModel || $lensTypePrt`.
    if let Some(unknown) = as_unknown_id(lens_type_prt) {
        let _ = unknown;
        return Some(
            args.lens_model
                .filter(|m| !m.is_empty())
                .unwrap_or(lens_type_prt)
                .to_string(),
        );
    }
    let Some(lenses) = candidates(alt_table, lens_type_prt) else {
        return Some(lens_type_prt.to_string());
    };

    // Exif.pm:5974-6037 -- narrow by FocalLength and MaxAperture.
    // The `$sf0` (Sony LensSpec) sub-branch is unreachable here: LensSpec is
    // a Sony tag and Sony is refused upstream.
    let mut matches: Vec<String> = Vec::new();
    let mut best: Vec<String> = Vec::new();
    let mut diff: Option<f64> = None;
    for lens in &lenses {
        let Some(mut info) = get_lens_info(lens) else {
            continue;
        };
        if info.sf == 0.0 {
            continue;
        }
        // Exif.pm:6019-6023 -- Minolta teleconverter scaling:
        //     if ($lens =~ / \+ .*? (\d+(\.\d+)?)x( |$)/) { $sf *= $1; ... }
        if let Some(x) = teleconverter_factor(lens) {
            info.sf *= x;
            info.lf *= x;
            info.sa *= x;
            info.la *= x;
        }
        if let Some(fl) = args.focal_length.filter(|v| *v != 0.0) {
            if fl < info.sf - 0.5 || fl > info.lf + 0.5 {
                continue;
            }
        }
        if let Some(ma) = max_aperture.filter(|v| *v != 0.0) {
            if ma < info.sa - 0.15 || ma > info.la + 0.15 {
                continue;
            }
            let fl = args.focal_length.unwrap_or(0.0);
            let aa = if info.sf == info.lf || info.sa == info.la || fl <= info.sf {
                info.sa
            } else if fl >= info.lf {
                info.la
            } else {
                // exp(log(sa) + (log(la)-log(sa)) / (log(lf)-log(sf)) *
                //                (log(fl)-log(sf)))
                (info.sa.ln()
                    + (info.la.ln() - info.sa.ln()) / (info.lf.ln() - info.sf.ln())
                        * (fl.ln() - info.sf.ln()))
                .exp()
            };
            let d = (ma - aa).abs();
            if let Some(prev) = diff {
                if d > prev + 0.15 {
                    continue;
                }
                if d < prev - 0.15 {
                    best.clear();
                }
            }
            diff = Some(d);
            best.push(lens.clone());
        }
        matches.push(lens.clone());
    }

    // Exif.pm:6050-6055 -- @best = @matches unless @best; then MatchLensModel.
    if best.is_empty() {
        best = matches;
    }
    if !best.is_empty() {
        match_lens_model(&mut best, args.lens_model);
        return Some(best.join(" or "));
    }
    // Exif.pm:6056-6058:
    //     $lens = $$printConv{$lensType};
    //     return $lensModel if $lensModel and $lens =~ / or /;
    //     return $lens;
    if let Some(model) = args.lens_model.filter(|m| !m.is_empty()) {
        if lens_type_prt.contains(" or ") {
            return Some(model.to_string());
        }
    }
    Some(lens_type_prt.to_string())
}

/// `Image::ExifTool::Canon::PrintLensID` (Canon.pm:10183-10305), minus the
/// userLens block (see [`OMITTED`]).
///
/// `lens_type_prt` stands in for both `$lensType` and `$$printConv{$lensType}`:
/// see [`super::lens_alternatives`] for why the integer-key string is a
/// sufficient handle.
fn canon_print_lens_id(
    table: LensTable,
    lens_type_prt: &str,
    short_focal: f64,
    long_focal: f64,
    max_aperture: Option<f64>,
    lens_model: Option<&str>,
) -> Option<String> {
    // Canon.pm:10186-10187:
    //     $lens = $$printConv{$lensType} unless $lensType eq '-1' or eq '65535';
    // oxidex prints both of those ids as "n/a" (lens_data.rs's generator
    // refuses to emit the table unless that stays true), and an id absent
    // from the table as "Unknown (N)" -- both of which mean `$lens` is false.
    let unknown_id = as_unknown_id(lens_type_prt);
    let is_na = lens_type_prt == "n/a";
    if !is_na && unknown_id.is_none() {
        // Canon.pm:10190 -- `return LensWithTC($lens, $shortFocal) unless
        // $$printConv{"$lensType.1"};`
        let Some(lenses) = candidates(table.alternatives()?, lens_type_prt) else {
            return Some(lens_with_tc(lens_type_prt, short_focal));
        };

        // Canon.pm:10202-10203 -- teleconverter scaling factors. The
        // `$lensModel =~ / \+ ((EXTENDER )?RF)?(\d+(\.\d*)?)x\b/` override is
        // an RF-body case; reproduce it only when it matches unambiguously.
        let tcs: Vec<(f64, String)> = match lens_model.and_then(model_tc_override) {
            Some(one) => vec![one],
            None => ["1", "1.4", "2", "2.8"]
                .iter()
                .map(|t| (t.parse::<f64>().expect("literal"), (*t).to_string()))
                .collect(),
        };

        let (mut maybe, mut likely, mut matches): (Vec<String>, Vec<String>, Vec<String>) =
            (Vec::new(), Vec::new(), Vec::new());
        for (tc, tc_label) in tcs {
            for lens in &lenses {
                let Some(mut info) = get_lens_info_canon(lens) else {
                    continue;
                };
                // Canon.pm:10216-10219 -- converter-specific LensType names
                // ending in " + #.#x" scale their own parsed range.
                if let Some(x) = trailing_tc_factor(lens) {
                    info.sf *= x;
                    info.lf *= x;
                    info.sa *= x;
                    info.la *= x;
                }
                if (short_focal - info.sf * tc).abs() > 0.9 {
                    continue;
                }
                let mut tclens = lens.clone();
                if let Some(suffix_tc) = trailing_tc_label(lens) {
                    // Canon.pm:10222-10229
                    if suffix_tc != tc_label.as_str() {
                        continue;
                    }
                    let lns = lens.rsplit_once(" + ").map(|(a, _)| a).unwrap_or(lens);
                    for v in [&mut maybe, &mut likely, &mut matches] {
                        if v.last().is_some_and(|l| l.starts_with(lns)) {
                            v.pop();
                        }
                    }
                } else if tc > 1.0 {
                    tclens = format!("{lens} + {tc_label}x");
                }
                maybe.push(tclens.clone());
                if (long_focal - info.lf * tc).abs() > 0.9 {
                    continue;
                }
                likely.push(tclens.clone());
                if let Some(ma) = max_aperture.filter(|v| *v != 0.0) {
                    if ma < info.sa * tc - 0.18 || ma > info.la * tc + 0.18 {
                        continue;
                    }
                }
                matches.push(tclens);
            }
            if !maybe.is_empty() {
                break;
            }
        }

        // Canon.pm:10259-10267 -- Sigma Art/Contemporary/Sports narrowing.
        if matches.len() > 1 {
            if let Some(t) = lens_model.and_then(sigma_line_marker) {
                let best: Vec<String> =
                    matches.iter().filter(|l| l.contains(&t)).cloned().collect();
                if !best.is_empty() {
                    matches = best;
                }
            }
        }
        if matches.is_empty() {
            matches = likely;
        }
        if matches.is_empty() {
            matches = maybe;
        }
        // Canon.pm:10270-10283 -- narrow by the LensModel's own mm/f-stop.
        if matches.len() > 1 {
            if let Some((mm, fstop)) = lens_model.and_then(model_mm_fstop) {
                let best: Vec<String> = matches
                    .iter()
                    .filter(|l| model_mm_fstop(l).is_some_and(|(m, f)| m == mm && f == fstop))
                    .cloned()
                    .collect();
                if !best.is_empty() {
                    matches = best;
                }
            }
        }
        match_lens_model(&mut matches, lens_model);
        if !matches.is_empty() {
            return Some(matches.join(" or "));
        }
    } else if let Some(model) = lens_model.filter(|m| m.bytes().any(|c| c.is_ascii_digit())) {
        // Canon.pm:10287-10294 -- `} elsif ($lensModel and $lensModel =~ /\d/)`:
        //     if ($printConv eq \%canonLensTypes) { return "Canon $lensModel" }
        //     else                                { return $lensModel }
        // a reference-identity test on the PrintConv hash, which `LensTable`
        // is the faithful stand-in for.
        return Some(if table == LensTable::Canon {
            format!("Canon {model}")
        } else {
            model.to_string()
        });
    }

    // Canon.pm:10296-10304:
    //     my $str = '';
    //     if ($shortFocal) {
    //         $str .= sprintf(' %d', $shortFocal);
    //         $str .= sprintf('-%d', $longFocal) if $longFocal and $longFocal != $shortFocal;
    //         $str .= 'mm';
    //     }
    //     return "Unknown$str" if $lensType eq '-1' or $lensType eq '65535';
    //     return "Unknown ($lensType)$str";
    let mut str_ = String::new();
    if short_focal != 0.0 {
        str_.push_str(&format!(" {}", short_focal as i64));
        if long_focal != 0.0 && long_focal != short_focal {
            str_.push_str(&format!("-{}", long_focal as i64));
        }
        str_.push_str("mm");
    }
    if is_na {
        return Some(format!("Unknown{str_}"));
    }
    let id = unknown_id?;
    Some(format!("Unknown ({id}){str_}"))
}

/// Canon's own candidate parse (Canon.pm:10207):
/// `/(\d+)(?:-(\d+))?mm.*?(?:[fF]\/?)(\d+(?:\.\d+)?)(?:-(\d+(?:\.\d+)?))?/`
/// then `$lf = $sf if $sf and not $lf; $la = $sa if $sa and not $la;`
///
/// It differs from `GetLensInfo` in two ways that matter: focal lengths here
/// are integers only, and there is no `\s*` before `mm` or after `f/`.
fn get_lens_info_canon(lens: &str) -> Option<LensInfo> {
    let b = lens.as_bytes();
    let mut start = 0usize;
    while start < b.len() {
        let Some((sf, mut i)) = integer_at(b, start) else {
            start += 1;
            continue;
        };
        let mut lf = None;
        if b.get(i) == Some(&b'-') {
            if let Some((v, j)) = integer_at(b, i + 1) {
                lf = Some(v);
                i = j;
            }
        }
        if b[i..].starts_with(b"mm") {
            let mut k = i + 2;
            while k < b.len() {
                if b[k] == b'f' || b[k] == b'F' {
                    let mut m = k + 1;
                    if b.get(m) == Some(&b'/') {
                        m += 1;
                    }
                    if let Some((sa, mut n)) = number_at(b, m) {
                        let mut la = sa;
                        if b.get(n) == Some(&b'-') {
                            if let Some((v, p)) = number_at(b, n + 1) {
                                la = v;
                                n = p;
                            }
                        }
                        let _ = n;
                        return Some(LensInfo {
                            sf,
                            lf: lf.unwrap_or(sf),
                            sa,
                            la,
                        });
                    }
                }
                k += 1;
            }
            start = i + 2;
            continue;
        }
        start += 1;
    }
    None
}

/// `(\d+)` -- integers only, as Canon.pm's own pattern spells it.
fn integer_at(b: &[u8], i: usize) -> Option<(f64, usize)> {
    let mut j = i;
    while b.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    if j == i {
        return None;
    }
    Some((std::str::from_utf8(&b[i..j]).ok()?.parse().ok()?, j))
}

/// `/^(.*) \+ (RF)?(\d+(\.\d*)?)x$/` -- the teleconverter label a LensType name
/// carries in its own suffix (Canon.pm:10222).
fn trailing_tc_label(lens: &str) -> Option<&str> {
    let rest = lens.strip_suffix('x')?;
    let (_, tail) = rest.rsplit_once(" + ")?;
    let tail = tail.strip_prefix("RF").unwrap_or(tail);
    let ok = !tail.is_empty()
        && tail.bytes().all(|c| c.is_ascii_digit() || c == b'.')
        && tail.bytes().next().is_some_and(|c| c.is_ascii_digit());
    ok.then_some(tail)
}

/// `/ \+ (\d+(\.\d+)?)x$/` -- the numeric form of the same suffix
/// (Canon.pm:10216).
fn trailing_tc_factor(lens: &str) -> Option<f64> {
    let rest = lens.strip_suffix('x')?;
    let (_, tail) = rest.rsplit_once(" + ")?;
    tail.parse::<f64>()
        .ok()
        .filter(|_| tail.bytes().all(|c| c.is_ascii_digit() || c == b'.'))
}

/// `/ \+ .*? (\d+(\.\d+)?)x( |$)/` (Exif.pm:6019) -- the Minolta form, which
/// unlike Canon's need not sit at the very end of the string.
///
/// Note the *literal space* between `.*?` and the capture: a name ending
/// "` + 1.4x`" does NOT match (nothing can supply that space), while
/// "`... + AF 1.4x APO`" does. Getting that backwards would scale the wrong
/// entries, so the space is reproduced rather than smoothed over.
fn teleconverter_factor(lens: &str) -> Option<f64> {
    let plus = lens.find(" + ")?;
    let rest = lens[plus + 3..].as_bytes();
    for i in 1..rest.len() {
        if rest[i - 1] != b' ' {
            continue;
        }
        let Some((v, j)) = number_at(rest, i) else {
            continue;
        };
        if rest.get(j) == Some(&b'x') && matches!(rest.get(j + 1), None | Some(b' ')) {
            return Some(v);
        }
    }
    None
}

/// `@tcs = ( $3 ) if $lensModel =~ / \+ ((EXTENDER )?RF)?(\d+(\.\d*)?)x\b/;`
/// (Canon.pm:10202-10203) -- replaces the default `(1, 1.4, 2, 2.8)` list with
/// the single factor the camera itself named.
///
/// `$3` is used both numerically (`$sf * $tc`) and as text (`" + ${tc}x"`, and
/// `next unless $3 eq $tc`), so both forms are returned: rounding the text
/// through an `f64` and back would turn ExifTool's `2` into `2.0` in an output
/// string, which is exactly the plausible-but-wrong value this port refuses.
fn model_tc_override(model: &str) -> Option<(f64, String)> {
    let idx = model.find(" + ")?;
    let mut rest = &model[idx + 3..];
    rest = rest.strip_prefix("EXTENDER ").unwrap_or(rest);
    rest = rest.strip_prefix("RF").unwrap_or(rest);
    let b = rest.as_bytes();
    // `(\d+(\.\d*)?)` -- unlike `$pat` elsewhere, the fractional part here may
    // be empty ("2." matches).
    let mut j = 0;
    while b.get(j).is_some_and(u8::is_ascii_digit) {
        j += 1;
    }
    if j == 0 {
        return None;
    }
    if b.get(j) == Some(&b'.') {
        j += 1;
        while b.get(j).is_some_and(u8::is_ascii_digit) {
            j += 1;
        }
    }
    if b.get(j) != Some(&b'x') {
        return None;
    }
    // `\b` after `x`: the next character must not be a word character.
    if b.get(j + 1).is_some_and(|c| is_word(*c)) {
        return None;
    }
    let text = &rest[..j];
    Some((text.trim_end_matches('.').parse().ok()?, text.to_string()))
}

/// `/(\| [ACS])/` -- the Sigma product-line marker (Canon.pm:10260).
fn sigma_line_marker(model: &str) -> Option<String> {
    let b = model.as_bytes();
    (0..b.len().saturating_sub(2))
        .find(|&i| b[i] == b'|' && b[i + 1] == b' ' && matches!(b[i + 2], b'A' | b'C' | b'S'))
        .map(|i| model[i..i + 3].to_string())
}

/// `/(\d+(?:\.\d+)?(?:-\d+(?:\.\d+)?)?) ?mm ?f\/?(\d+(?:\.\d+)?(?:-\d+(?:\.\d+)?)?)/i`
/// (Canon.pm:10272) -- returns the two captured strings verbatim, because
/// ExifTool compares them with `eq`, not numerically.
fn model_mm_fstop(s: &str) -> Option<(String, String)> {
    let b = s.as_bytes();
    for start in 0..b.len() {
        if !b[start].is_ascii_digit() {
            continue;
        }
        if start > 0 && (b[start - 1].is_ascii_digit() || b[start - 1] == b'.') {
            continue;
        }
        let Some((_, mut i)) = number_at(b, start) else {
            continue;
        };
        if b.get(i) == Some(&b'-') {
            if let Some((_, j)) = number_at(b, i + 1) {
                i = j;
            }
        }
        let mm_text = s[start..i].to_string();
        let mut j = i;
        if b.get(j) == Some(&b' ') {
            j += 1;
        }
        if !b[j..].starts_with(b"mm") {
            continue;
        }
        j += 2;
        if b.get(j) == Some(&b' ') {
            j += 1;
        }
        if !matches!(b.get(j), Some(b'f') | Some(b'F')) {
            continue;
        }
        j += 1;
        if b.get(j) == Some(&b'/') {
            j += 1;
        }
        let Some((_, mut k)) = number_at(b, j) else {
            continue;
        };
        let f_start = j;
        if b.get(k) == Some(&b'-') {
            if let Some((_, m)) = number_at(b, k + 1) {
                k = m;
            }
        }
        return Some((mm_text, s[f_start..k].to_string()));
    }
    None
}

/// `/^(\d+)(?: (?:to )?(\d+))?$/` over LensFocalRange (Exif.pm:5911).
fn parse_focal_range(s: &str) -> Option<(f64, f64)> {
    let b = s.as_bytes();
    let (a, i) = integer_at(b, 0)?;
    if i == b.len() {
        return Some((a, a));
    }
    let rest = &s[i..];
    let rest = rest.strip_prefix(' ')?;
    let rest = rest.strip_prefix("to ").unwrap_or(rest);
    let rb = rest.as_bytes();
    let (c, j) = integer_at(rb, 0)?;
    (j == rb.len()).then_some((a, if c == 0.0 { a } else { c }))
}

/// `s/(\d)\/F/$1mm F/` applied once, Perl-style (leftmost only).
fn subst_digit_slash_f(s: &str) -> (String, bool) {
    let b = s.as_bytes();
    for i in 0..b.len().saturating_sub(2) {
        if b[i].is_ascii_digit() && &b[i + 1..i + 3] == b"/F" {
            return (format!("{}mm F{}", &s[..=i], &s[i + 3..]), true);
        }
    }
    (s.to_string(), false)
}

/// `/^TAMRON.*-\d+mm/` (Exif.pm:5954).
fn tamron_dash_mm(model: &str) -> bool {
    let b = model.as_bytes();
    (0..b.len()).any(|i| {
        b[i] == b'-'
            && number_at(b, i + 1).is_some_and(|(_, j)| {
                b[i + 1..j].iter().all(u8::is_ascii_digit) && b[j..].starts_with(b"mm")
            })
    })
}

/// oxidex's own rendering of an id its lens table does not carry:
/// `format!("Unknown ({})", lens_id)` (canon.rs, the `Canon:LensType` arm).
/// Returns the numeric id, which is what Canon.pm:10304's
/// `"Unknown ($lensType)$str"` needs and which nothing else in the pipeline
/// still carries.
fn as_unknown_id(s: &str) -> Option<&str> {
    let inner = s.strip_prefix("Unknown (")?.strip_suffix(')')?;
    (!inner.is_empty() && inner.bytes().all(|c| c.is_ascii_digit())).then_some(inner)
}

/// Compute `Composite:LensID` from the `LensID-2` fallback definition
/// (Exif.pm:5362-5385), whose inputs are `LensModel`, `Lens`, `XMP-aux:LensID`
/// and `Make`:
///
/// ```text
///     RawConv => q{
///         return undef if defined $val[2] and defined $val[3];
///         return $val if defined $val[0] and $val[0] =~ /(mm|\d\/F)/;
///         return $val if defined $val[1] and $val[1] =~ /(mm|\d\/F)/;
///         return undef;
///     },
///     ValueConv => q{
///         return $val[0] if defined $val[0] and $val[0] =~ /(mm|\d\/F)/;
///         return $val[1];
///     },
///     PrintConv => '$_=$val; s/(\d)\/F/$1mm F/; s/mmF/mm F/; s/(\d) mm/${1}mm/; s/ - /-/; $_',
/// ```
pub(super) fn compute_fallback(inputs: &[Option<&str>]) -> Option<(String, String)> {
    let get = |i: usize| inputs.get(i).copied().flatten();
    // See OMITTED's "strings carrying an unterminated tail".
    if inputs.iter().flatten().any(|s| s.contains('\0')) {
        return None;
    }
    if get(2).is_some() && get(3).is_some() {
        return None;
    }
    let m0 = get(0).filter(|v| has_mm_or_slash_f(v));
    let m1 = get(1).filter(|v| has_mm_or_slash_f(v));
    if m0.is_none() && m1.is_none() {
        return None;
    }
    // ValueConv: LensModel when it matched, else Lens -- *whether or not* Lens
    // itself matched, which is why this reads get(1) and not m1.
    let value = match m0 {
        Some(v) => v.to_string(),
        None => get(1)?.to_string(),
    };
    Some((print_conv_fallback(&value), value))
}

/// The four ordered substitutions of `LensID-2`'s PrintConv, each `s///`
/// without `/g`, so each replaces its own leftmost match only.
fn print_conv_fallback(val: &str) -> String {
    let (mut s, _) = subst_digit_slash_f(val);
    if let Some(i) = s.find("mmF") {
        s = format!("{}mm F{}", &s[..i], &s[i + 3..]);
    }
    // s/(\d) mm/${1}mm/
    let b = s.as_bytes();
    if let Some(i) = (0..b.len().saturating_sub(3))
        .find(|&i| b[i].is_ascii_digit() && &b[i + 1..i + 4] == b" mm")
    {
        s = format!("{}mm{}", &s[..=i], &s[i + 4..]);
    }
    // s/ - /-/
    if let Some(i) = s.find(" - ") {
        s = format!("{}-{}", &s[..i], &s[i + 3..]);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternatives_keys_are_unique() {
        for (label, table) in [
            ("canon", &CANON_LENS_ALTERNATIVES[..]),
            ("pentax", &PENTAX_LENS_ALTERNATIVES[..]),
        ] {
            let mut keys: Vec<&str> = table.iter().map(|(k, _)| *k).collect();
            let before = keys.len();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(
                keys.len(),
                before,
                "{label} has duplicate integer-key strings"
            );
            assert!(
                table.iter().all(|(_, alts)| !alts.is_empty()),
                "{label} carries an entry with no alternatives"
            );
        }
        assert_eq!(CANON_LENS_ALTERNATIVES.len(), 71);
        assert_eq!(PENTAX_LENS_ALTERNATIVES.len(), 14);
    }

    #[test]
    fn get_lens_info_parses_canon_names() {
        assert_eq!(
            get_lens_info("Canon EF 28-70mm f/2.8L USM"),
            Some(LensInfo {
                sf: 28.0,
                lf: 70.0,
                sa: 2.8,
                la: 2.8
            })
        );
        assert_eq!(
            get_lens_info("Sigma 28-70mm f/2.8 EX"),
            Some(LensInfo {
                sf: 28.0,
                lf: 70.0,
                sa: 2.8,
                la: 2.8
            })
        );
        assert_eq!(
            get_lens_info("Canon EF 35-105mm f/3.5-4.5"),
            Some(LensInfo {
                sf: 35.0,
                lf: 105.0,
                sa: 3.5,
                la: 4.5
            })
        );
        assert_eq!(
            get_lens_info("Canon EF 50mm f/1.8"),
            Some(LensInfo {
                sf: 50.0,
                lf: 50.0,
                sa: 1.8,
                la: 1.8
            })
        );
        // No aperture: GetLensInfo returns the empty list.
        assert_eq!(get_lens_info("Canon EF 50mm"), None);
    }

    /// The exact repro from the gap report: the pinned oracle emits
    /// `[Composite] LensID : Canon EF 28-70mm f/2.8L USM or Sigma 28-70mm
    /// f/2.8 EX` for `Canon/CanonEOS-1D.jpg`, whose `Canon:LensType` prints
    /// `Canon EF 28-70mm f/2.8L USM or Other Lens` -- id 24 in
    /// `%canonLensTypes`, which carries one alternative.
    #[test]
    fn canon_eos_1d_disambiguates_to_both_lenses() {
        let inputs = [
            Some("Canon EF 28-70mm f/2.8L USM or Other Lens"),
            Some("47"),  // FocalLength
            Some("2.8"), // MaxAperture
            None,
            Some("28"), // MinFocalLength
            Some("70"), // MaxFocalLength
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ];
        assert_eq!(
            compute_primary(&inputs, Some("Canon"), Some("Canon"), false).as_deref(),
            Some("Canon EF 28-70mm f/2.8L USM or Sigma 28-70mm f/2.8 EX")
        );
    }

    /// Olympus: `%olympusLensTypes` has no fractional keys at all, so
    /// `Canon::PrintLensID`'s `unless $$printConv{"$lensType.1"}` fires on the
    /// first line and the answer is `LensWithTC(name, shortFocal)`.
    #[test]
    fn olympus_returns_lens_type_unchanged() {
        let inputs = [
            Some("Olympus Zuiko Digital ED 7-14mm F4.0"),
            Some("7.0"),
            Some("3.0"),
            Some("4.0"),
            Some("7"),
            Some("14"),
            Some("OLYMPUS  7-14mm Lens"),
            None,
            None,
            None,
            None,
            None,
            None,
        ];
        assert_eq!(
            compute_primary(
                &inputs,
                Some("Olympus"),
                Some("OLYMPUS IMAGING CORP."),
                false
            )
            .as_deref(),
            Some("Olympus Zuiko Digital ED 7-14mm F4.0")
        );
    }

    /// Panasonic's `LensType` (Panasonic.pm:945-949) is a plain string with no
    /// PrintConv, so `PrintLensID` takes its `unless (ref $printConv eq
    /// 'HASH')` branch: `/mm/` first, then `s/(\d)\/F/$1mm F/`.
    #[test]
    fn panasonic_rewrites_slash_f_into_mm_f() {
        let mut inputs = [None; 13];
        inputs[0] = Some("LUMIX G VARIO 12-32/F3.5-5.6");
        assert_eq!(
            compute_primary(&inputs, Some("Panasonic"), Some("Panasonic"), false).as_deref(),
            Some("LUMIX G VARIO 12-32mm F3.5-5.6")
        );
        inputs[0] = Some("LEICA DG SUMMILUX 15mm/F1.7");
        assert_eq!(
            compute_primary(&inputs, Some("Panasonic"), Some("Panasonic"), false).as_deref(),
            Some("LEICA DG SUMMILUX 15mm/F1.7")
        );
    }

    /// The same Panasonic string is REFUSED once the body carries BOTH
    /// LensTypeMake and LensTypeModel, because ExifTool then answers from
    /// `%olympusLensTypes` instead (`PanasonicDC-G100.jpg`: `2` + `20 10`,
    /// oracle `Lumix G Vario 12-32mm F3.5-5.6 Asph. Mega OIS`). A body missing
    /// either half keeps Panasonic's own string (`PanasonicDC-S1.jpg`).
    #[test]
    fn panasonic_olympus_lens_type_pair_refuses_the_untranscribed_lookup() {
        let mut inputs = [None; 13];
        inputs[0] = Some("LUMIX G VARIO 12-32/F3.5-5.6");
        assert_eq!(
            compute_primary(&inputs, Some("Panasonic"), Some("Panasonic"), true),
            None,
            "LensTypeMake+LensTypeModel means %olympusLensTypes owns the answer"
        );
        assert_eq!(
            compute_primary(&inputs, Some("Panasonic"), Some("Panasonic"), false).as_deref(),
            Some("LUMIX G VARIO 12-32mm F3.5-5.6"),
            "without both halves, Panasonic's own string stands"
        );
    }

    /// An input still carrying its NUL terminator and the makernote bytes
    /// after it is refused by both rows -- see [`OMITTED`].
    #[test]
    fn unterminated_string_is_refused_by_both_rows() {
        let dirty = "NX 16-50mm F3.5-5.6 Power Zoom\u{0}\u{10}XL1401";
        let mut inputs = [None; 13];
        inputs[0] = Some(dirty);
        assert_eq!(
            compute_primary(&inputs, Some("Canon"), Some("Canon"), false),
            None
        );
        assert_eq!(compute_fallback(&[Some(dirty), None, None, None]), None);
    }

    /// Sony is refused outright rather than approximated -- see [`OMITTED`].
    #[test]
    fn sony_is_omitted_not_guessed() {
        let mut inputs = [None; 13];
        inputs[0] = Some("Sony FE 24-70mm F2.8 GM");
        assert_eq!(
            compute_primary(&inputs, Some("Sony"), Some("SONY"), false),
            None
        );
        assert_eq!(OMITTED.len(), 6);
    }

    /// The Pentax converter suffix (Exif.pm:5354-5358) is omitted, not
    /// computed from this tree's mis-offset `LensFocalLength` -- the
    /// `Pentax645D.jpg` inputs that used to append `+ 14.5x converter`.
    #[test]
    fn pentax_converter_suffix_is_omitted() {
        let mut inputs = [None; 13];
        inputs[0] = Some("smc PENTAX-D FA 645 55mm F2.8 AL [IF] SDM AW");
        inputs[1] = Some("55.0 mm"); // FocalLength
        inputs[11] = Some("3.8 mm"); // LensFocalLength, decoded from the wrong byte
        assert_eq!(
            compute_primary(&inputs, Some("Pentax"), Some("PENTAX"), false).as_deref(),
            Some("smc PENTAX-D FA 645 55mm F2.8 AL [IF] SDM AW"),
        );
    }

    /// `compute_fallback` still computes the LensID-2 text form; it is
    /// [`super::apply`] that refuses to *call* it once a maker `LensType`
    /// exists (Exif.pm:5371-5373's `Inhibit`). Pin the value this would have
    /// produced on `Nikon.nef`, so the inhibit in `apply` is visibly the thing
    /// standing between it and the output.
    #[test]
    fn nikon_fallback_value_is_the_one_apply_must_inhibit() {
        let inputs = [None, Some("18-70mm f/3.5-4.5"), None, Some("NIKON")];
        assert_eq!(
            compute_fallback(&inputs).map(|(p, _)| p).as_deref(),
            Some("18-70mm f/3.5-4.5"),
            "the oracle says `AF-S DX Zoom-Nikkor 18-70mm f/3.5-4.5G IF-ED`; \
             this raw text is why apply() must inhibit the row"
        );
    }

    #[test]
    fn fallback_matches_exiftool_substitutions() {
        // Apple: LensModel "iPad back camera 3.3mm f/2.4" already has "mm".
        let inputs = [Some("iPad back camera 3.3mm f/2.4"), None, None, None];
        assert_eq!(
            compute_fallback(&inputs).map(|(p, _)| p).as_deref(),
            Some("iPad back camera 3.3mm f/2.4")
        );
        // `s/ - /-/` and `s/(\d) mm/${1}mm/`.
        let inputs = [Some("18 mm - 55 mm"), None, None, None];
        assert_eq!(
            compute_fallback(&inputs).map(|(p, _)| p).as_deref(),
            Some("18mm-55 mm")
        );
        // Inhibited when both XMP-aux:LensID and Make are present.
        let inputs = [Some("50.0 mm"), None, Some("123"), Some("NIKON")];
        assert_eq!(compute_fallback(&inputs), None);
        // Neither input carries `mm` or `\d/F`.
        let inputs = [Some("Unknown"), None, None, None];
        assert_eq!(compute_fallback(&inputs), None);
    }

    #[test]
    fn lens_with_tc_appends_only_a_matching_factor() {
        assert_eq!(
            lens_with_tc("Canon EF 300mm f/2.8L USM", 300.0),
            "Canon EF 300mm f/2.8L USM"
        );
        assert_eq!(
            lens_with_tc("Canon EF 300mm f/2.8L USM", 420.0),
            "Canon EF 300mm f/2.8L USM + 1.4x"
        );
        // Already carries a factor: left alone.
        assert_eq!(
            lens_with_tc("Canon EF 300mm f/2.8L USM + 2x", 600.0),
            "Canon EF 300mm f/2.8L USM + 2x"
        );
    }

    #[test]
    fn unknown_id_round_trips_through_the_print_form() {
        assert_eq!(as_unknown_id("Unknown (1234)"), Some("1234"));
        assert_eq!(as_unknown_id("Canon EF 50mm f/1.8"), None);
        assert_eq!(as_unknown_id("Unknown ()"), None);
    }
}
