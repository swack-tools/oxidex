//! Sony's `Decipher`, and the `Sony::Main` entries that need it.
//!
//! # The cipher
//!
//! Sony enciphers 0x2010, 0x900b, 0x9050 and the 0x94xx blocks with a single
//! byte substitution -- no key, no state, no dependence on position. ExifTool
//! spells the table out as a literal `tr///` for speed, but documents the rule
//! it was built from (`Sony.pm`, `sub Decipher`):
//!
//! ```text
//! c = (b * b * b) % 249      for 2 <= b <= 247
//! c = b                      otherwise
//! ```
//!
//! 249 = 3 x 83, and cubing is a bijection modulo both 3 (`x^3 == x`) and 83
//! (`gcd(3, 82) == 1`), so cubing permutes 0..248; 0, 1 and 248 are fixed
//! points, which is why the rule can leave 0..1 and 248..255 alone and still
//! be one-to-one on 2..247. [`ENCIPHER`] is that formula evaluated at compile
//! time and [`DECIPHER`] its inverse, and the tests pin both against the 256
//! bytes ExifTool's own `Decipher` produces.
//!
//! Because the cipher is keyless, a wrong table cannot be detected by "the
//! output looks like garbage" -- any permutation yields bytes, and a binary
//! table will happily decode them into plausible ISO values and lens names.
//! [`tests`] therefore checks the table itself (formula, ExifTool's literal,
//! bijectivity, round-trip) rather than the values that come out of it.
//!
//! # The dispatch
//!
//! Which table decodes a block depends on the body, and for the 0x94xx blocks
//! on the leading bytes of the *enciphered* value -- ExifTool's Conditions run
//! against `$$valPt` before `ProcessEnciphered` touches it, so [`RootCond`]
//! tests do too.

use super::binary_data::{self, Ctx, Found};
use super::enciphered_tables::{TABLES, idx};
use crate::parsers::tiff::ifd_parser::ByteOrder;

// ===========================================================================
// The substitution table
// ===========================================================================

#[allow(clippy::manual_range_contains)] // `contains` is not a const fn
const fn build_encipher() -> [u8; 256] {
    let mut t = [0u8; 256];
    let mut b = 0usize;
    while b < 256 {
        t[b] = if b >= 2 && b <= 247 {
            ((b * b * b) % 249) as u8
        } else {
            b as u8
        };
        b += 1;
    }
    t
}

const fn build_decipher() -> [u8; 256] {
    let enc = build_encipher();
    let mut t = [0u8; 256];
    let mut b = 0usize;
    while b < 256 {
        t[enc[b] as usize] = b as u8;
        b += 1;
    }
    t
}

/// `b -> b^3 mod 249`, ExifTool's encipher direction.
pub const ENCIPHER: [u8; 256] = build_encipher();
/// The inverse of [`ENCIPHER`], which is what reading needs.
pub const DECIPHER: [u8; 256] = build_decipher();

/// Deciphers `data` in place with `table`.
///
/// Taking the table as an argument is what lets the tests decode a real block
/// with a deliberately wrong permutation and show that the result is still
/// structured, parseable, and wrong.
pub fn decipher_with(table: &[u8; 256], data: &mut [u8]) {
    for b in data.iter_mut() {
        *b = table[*b as usize];
    }
}

/// Deciphers a copy of `data`, twice when ExifTool would (the 9.04-9.10
/// double-cipher bug, announced by the 0x9400 block).
pub fn decipher(data: &[u8], double: bool) -> Vec<u8> {
    let mut out = data.to_vec();
    decipher_with(&DECIPHER, &mut out);
    if double {
        decipher_with(&DECIPHER, &mut out);
    }
    out
}

// ===========================================================================
// Sony::Main dispatch
// ===========================================================================

/// A `Condition` on a `Sony::Main` sub-directory entry.
pub enum RootCond {
    Always,
    /// `$$self{Model} =~ /RE/` (`true` for the negated `!~`)
    ModelRe(bool, &'static str),
    /// `not $$self{Panorama}` (0x1003's `$$valPt =~ /^(\0\0)?\x01\x01/`)
    NotPanorama,
    /// Byte tests on the *enciphered* value: `(offset, allowed bytes)`,
    /// negated as a whole when the flag is set.
    ValPt(bool, &'static [(usize, &'static [u8])]),
    All(&'static [RootCond]),
    Any(&'static [RootCond]),
}

pub struct Root {
    /// The `Sony::Main` tag id this variant belongs to.
    pub tag: u16,
    pub cond: RootCond,
    /// The table number in [`super::enciphered_tables::TABLES`], or `None` for
    /// ExifTool's `%unknownCipherData` catch-all, which extracts nothing.
    pub table: Option<usize>,
    /// `SubDirectory => { ByteOrder => 'LittleEndian' }`
    pub force_le: bool,
    /// `$$self{DoubleCipher} = 1` as a side effect of matching.
    pub sets_double_cipher: bool,
    /// Whether the block is enciphered at all: 0x202a's table is a plain
    /// `ProcessBinaryData`.
    pub enciphered: bool,
}

const fn root(tag: u16, cond: RootCond, table: usize) -> Root {
    Root {
        tag,
        cond,
        table: Some(table),
        force_le: false,
        sets_double_cipher: false,
        enciphered: true,
    }
}

const fn root_le(tag: u16, cond: RootCond, table: usize) -> Root {
    Root {
        force_le: true,
        ..root(tag, cond, table)
    }
}

/// Every enciphered `Sony::Main` sub-directory, in the order ExifTool lists its
/// `Condition` variants. The first variant whose Condition holds wins.
#[rustfmt::skip]
pub static ROOTS: &[Root] = &[
    // 0x2010 -- one table per body generation, all sharing the same offsets
    // for the 0x11xx region and differing below it.
    root(0x2010, RootCond::ModelRe(false, r"^NEX-5N$"), idx::TAG2010A),
    root(0x2010, RootCond::ModelRe(false, r"^(SLT-A(65|77)V?|NEX-(7|VG20E)|Lunar)$"), idx::TAG2010B),
    root(0x2010, RootCond::ModelRe(false, r"^(SLT-A(37|57)|NEX-F3)$"), idx::TAG2010C),
    root(0x2010, RootCond::All(&[
        RootCond::ModelRe(false, r"^(DSC-(HX10V|HX20V|HX30V|HX200V|TX66|TX200V|TX300V|WX50|WX70|WX100|WX150))$"),
        RootCond::NotPanorama,
    ]), idx::TAG2010D),
    root(0x2010, RootCond::Any(&[
        RootCond::ModelRe(false, r"^(SLT-A99V?|HV|SLT-A58|ILCE-(3000|3500)|NEX-(3N|5R|5T|6|VG900|VG30E)|DSC-(RX100|RX1|RX1R)|Stellar)$"),
        RootCond::All(&[
            RootCond::ModelRe(false, r"^(DSC-(HX300|HX50|HX50V|TX30|WX60|WX80|WX200|WX300))$"),
            RootCond::NotPanorama,
        ]),
    ]), idx::TAG2010E),
    root(0x2010, RootCond::ModelRe(false, r"^(DSC-(RX100M2|QX10|QX100))$"), idx::TAG2010F),
    root(0x2010, RootCond::ModelRe(false, r"^(DSC-(QX30|RX10|RX100M3|HX60V|HX350|HX400V|WX220|WX350)|ILCE-(7(R|S|M2)?|[56]000|5100|QX1)|ILCA-(68|77M2))\b"), idx::TAG2010G),
    root(0x2010, RootCond::ModelRe(false, r"^(DSC-(RX0|RX1RM2|RX10M2|RX10M3|RX100M4|RX100M5|HX80|HX90V?|WX500)|ILCE-(6300|6500|7RM2|7SM2)|ILCA-99M2)\b"), idx::TAG2010H),
    root(0x2010, RootCond::ModelRe(false, r"^(ILCE-(6100A?|6400A?|6600|7C|7M3|7RM3A?|7RM4A?|9|9M2)|DSC-(RX10M4|RX100M6|RX100M5A|RX100M7A?|HX95|HX99|RX0M2)|ZV-(1[AF]?|1M2|E10))\b"), idx::TAG2010I),

    // 0x202a -- listed here for the dispatch, but its table is a plain
    // ProcessBinaryData: Tag202a uses %binaryDataAttrs, not ProcessEnciphered.
    Root { enciphered: false, ..root(0x202a, RootCond::ValPt(false, &[(0, &[0x01])]), idx::TAG202A) },

    root(0x900b, RootCond::ValPt(false, &[(0, &[0xae])]), idx::TAG900B),

    // 0x9050 -- ByteOrder is forced little-endian whatever the MakerNote uses.
    root_le(0x9050, RootCond::ModelRe(true, r"^(DSC-|Stellar|ILCE-(1|6100|6300|6400|6500|6600|6700|7C|7M3|7M4|7M5|7RM2|7RM3A?|7RM4A?|7RM5|7SM2|7SM3|9|9M2)|ILCA-99M2|ILME-(FX2|FX3)|ZV-)"), idx::TAG9050A),
    root_le(0x9050, RootCond::ModelRe(false, r"^(ILCE-(6100A?|6300|6400A?|6500|6600|7C|7M3|7RM2|7RM3A?|7RM4A?|7SM2|9|9M2)|ILCA-99M2|ZV-E10)\b"), idx::TAG9050B),
    root_le(0x9050, RootCond::ModelRe(false, r"^(ILCE-(1\b|7M4|7RM5|7SM3)|ILME-FX3)"), idx::TAG9050C),
    root_le(0x9050, RootCond::Any(&[
        RootCond::ModelRe(false, r"^(ILCE-(6700|7CM2|7CR)|ILME-FX2|ZV-(E1|E10M2))\b"),
        RootCond::All(&[
            RootCond::ModelRe(false, r"^(ILCE-(1M2|7M5))"),
            RootCond::ValPt(false, &[(0, &[0x00]), (1, &[0x00]), (2, &[0x00]), (3, &[0x00]), (4, &[0x00])]),
        ]),
    ]), idx::TAG9050D),

    // 0x9400 -- selected by the enciphered first byte. The second alternative
    // of Tag9400a is the ExifTool 9.04-9.10 double-cipher signature.
    root(0x9400, RootCond::ValPt(false, &[(0, &[0x07, 0x09, 0x0a])]), idx::TAG9400A),
    Root { sets_double_cipher: true,
           ..root(0x9400, RootCond::ValPt(false, &[(0, &[0x5e, 0xe7, 0x04])]), idx::TAG9400A) },
    root(0x9400, RootCond::ValPt(false, &[(0, &[0x0c])]), idx::TAG9400B),
    root(0x9400, RootCond::ValPt(false, &[(0, &[0x23, 0x24, 0x26, 0x28, 0x31, 0x32, 0x33, 0x41])]), idx::TAG9400C),

    root(0x9401, RootCond::Always, idx::TAG9401),
    root(0x9402, RootCond::All(&[
        RootCond::ModelRe(true, r"^(SLT-|HV|ILCA-)"),
        RootCond::ValPt(true, &[(0, &[0x05, 0xff])]),
    ]), idx::TAG9402),
    root(0x9403, RootCond::Always, idx::TAG9403),

    root(0x9404, RootCond::ValPt(false, &[(0, &[0x40, 0x7d]), (3, &[0x01])]), idx::TAG9404A),
    root(0x9404, RootCond::ValPt(false, &[(0, &[0xe7, 0xea, 0xcd, 0x8a, 0x70]), (3, &[0x08])]), idx::TAG9404B),
    root(0x9404, RootCond::ValPt(false, &[(0, &[0xb6]), (3, &[0x01])]), idx::TAG9404C),

    root(0x9405, RootCond::ValPt(false, &[(0, &[0x1b, 0x40, 0x7d])]), idx::TAG9405A),
    root(0x9405, RootCond::ValPt(false, &[(0, &[0x3a, 0xb3, 0x7e, 0x9a, 0x25, 0xe1, 0x76, 0x8b])]), idx::TAG9405B),

    root(0x9406, RootCond::ValPt(false, &[(0, &[0x01, 0x08, 0x1b]), (2, &[0x08, 0x1b])]), idx::TAG9406),
    root(0x9406, RootCond::ValPt(false, &[(0, &[0x40])]), idx::TAG9406B),

    root(0x940a, RootCond::ModelRe(false, r"^(SLT-|HV)"), idx::TAG940A),
    root(0x940c, RootCond::ModelRe(false, r"^(NEX-|ILCE-|ILME-|Lunar|ZV-E10|ZV-E10M2|ZV-E1)\b"), idx::TAG940C),

    root(0x940e, RootCond::ModelRe(false, r"^(SLT-|HV|ILCA-)"), idx::AFINFO),
    root(0x940e, RootCond::ModelRe(false, r"^(NEX-|ILCE-|Lunar)"), idx::TAG940E),

    root(0x9416, RootCond::Always, idx::TAG9416),
];

/// Whether any `Sony::Main` variant of this tag id exists.
pub fn is_root_tag(tag: u16) -> bool {
    ROOTS.iter().any(|r| r.tag == tag)
}

fn cond_holds(cond: &RootCond, ctx: &Ctx, panorama: bool, val: &[u8]) -> bool {
    match cond {
        RootCond::Always => true,
        RootCond::ModelRe(neg, re) => binary_data::model_matches(re, &ctx.model) != *neg,
        RootCond::NotPanorama => !panorama,
        RootCond::ValPt(neg, tests) => {
            let hit = tests
                .iter()
                .all(|(off, allowed)| val.get(*off).is_some_and(|b| allowed.contains(b)));
            hit != *neg
        }
        RootCond::All(list) => list.iter().all(|c| cond_holds(c, ctx, panorama, val)),
        RootCond::Any(list) => list.iter().any(|c| cond_holds(c, ctx, panorama, val)),
    }
}

/// Decodes one `Sony::Main` entry that carries an enciphered sub-directory.
///
/// `value` is the entry's bytes exactly as stored -- still enciphered, which is
/// what the `$$valPt` Conditions are written against. Returns nothing when no
/// variant matches, which is ExifTool's `%unknownCipherData` fall-through: a
/// hidden, Unknown tag that never reaches the output.
pub fn decode_root(
    tag: u16,
    value: &[u8],
    byte_order: ByteOrder,
    panorama: bool,
    ctx: &mut Ctx,
) -> Vec<Found> {
    let mut out = Vec::new();
    let Some(root) = ROOTS
        .iter()
        .find(|r| r.tag == tag && cond_holds(&r.cond, ctx, panorama, value))
    else {
        return out;
    };
    if root.sets_double_cipher {
        ctx.double_cipher = true;
    }
    let Some(table) = root.table else {
        return out;
    };
    let order = if root.force_le {
        ByteOrder::LittleEndian
    } else {
        byte_order
    };
    if root.enciphered {
        let plain = decipher(value, ctx.double_cipher);
        binary_data::process(TABLES, table, &plain, order, ctx, &mut out);
    } else {
        binary_data::process(TABLES, table, value, order, ctx, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ExifTool's hardcoded decipher `tr///` table, byte for byte: the output
    /// of `Image::ExifTool::Sony::Decipher` applied to the bytes 0..255 of
    /// ExifTool 13.59. Transcribed here so the formula in [`build_encipher`]
    /// is checked against ExifTool's own literal rather than against itself.
    #[rustfmt::skip]
    const EXIFTOOL_DECIPHER: [u8; 256] = [
        0x00, 0x01, 0x32, 0xb1, 0x0a, 0x0e, 0x87, 0x28, 0x02, 0xcc, 0xca, 0xad, 0x1b, 0xdc, 0x08, 0xed,
        0x64, 0x86, 0xf0, 0x4f, 0x8c, 0x6c, 0xb8, 0xcb, 0x69, 0xc4, 0x2c, 0x03, 0x97, 0xb6, 0x93, 0x7c,
        0x14, 0xf3, 0xe2, 0x3e, 0x30, 0x8e, 0xd7, 0x60, 0x1c, 0xa1, 0xab, 0x37, 0xec, 0x75, 0xbe, 0x23,
        0x15, 0x6a, 0x59, 0x3f, 0xd0, 0xb9, 0x96, 0xb5, 0x50, 0x27, 0x88, 0xe3, 0x81, 0x94, 0xe0, 0xc0,
        0x04, 0x5c, 0xc6, 0xe8, 0x5f, 0x4b, 0x70, 0x38, 0x9f, 0x82, 0x80, 0x51, 0x2b, 0xc5, 0x45, 0x49,
        0x9b, 0x21, 0x52, 0x53, 0x54, 0x85, 0x0b, 0x5d, 0x61, 0xda, 0x7b, 0x55, 0x26, 0x24, 0x07, 0x6e,
        0x36, 0x5b, 0x47, 0xb7, 0xd9, 0x4a, 0xa2, 0xdf, 0xbf, 0x12, 0x25, 0xbc, 0x1e, 0x7f, 0x56, 0xea,
        0x10, 0xe6, 0xcf, 0x67, 0x4d, 0x3c, 0x91, 0x83, 0xe1, 0x31, 0xb3, 0x6f, 0xf4, 0x05, 0x8a, 0x46,
        0xc8, 0x18, 0x76, 0x68, 0xbd, 0xac, 0x92, 0x2a, 0x13, 0xe9, 0x0f, 0xa3, 0x7a, 0xdb, 0x3d, 0xd4,
        0xe7, 0x3a, 0x1a, 0x57, 0xaf, 0x20, 0x42, 0xb2, 0x9e, 0xc3, 0x8b, 0xf2, 0xd5, 0xd3, 0xa4, 0x7e,
        0x1f, 0x98, 0x9c, 0xee, 0x74, 0xa5, 0xa6, 0xa7, 0xd8, 0x5e, 0xb0, 0xb4, 0x34, 0xce, 0xa8, 0x79,
        0x77, 0x5a, 0xc1, 0x89, 0xae, 0x9a, 0x11, 0x33, 0x9d, 0xf5, 0x39, 0x19, 0x65, 0x78, 0x16, 0x71,
        0xd2, 0xa9, 0x44, 0x63, 0x40, 0x29, 0xba, 0xa0, 0x8f, 0xe4, 0xd6, 0x3b, 0x84, 0x0d, 0xc2, 0x4e,
        0x58, 0xdd, 0x99, 0x22, 0x6b, 0xc9, 0xbb, 0x17, 0x06, 0xe5, 0x7d, 0x66, 0x43, 0x62, 0xf6, 0xcd,
        0x35, 0x90, 0x2e, 0x41, 0x8d, 0x6d, 0xaa, 0x09, 0x73, 0x95, 0x0c, 0xf1, 0x1d, 0xde, 0x4c, 0x2f,
        0x2d, 0xf7, 0xd1, 0x72, 0xeb, 0xef, 0x48, 0xc7, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xff,
    ];

    #[test]
    fn the_table_is_exiftools_literal_table() {
        assert_eq!(DECIPHER, EXIFTOOL_DECIPHER);
    }

    #[test]
    fn the_table_is_the_documented_cube_formula() {
        // Independently of ExifTool's literal: c = b^3 mod 249 on 2..=247.
        for b in 0usize..256 {
            let want = if (2..=247).contains(&b) {
                ((b * b * b) % 249) as u8
            } else {
                b as u8
            };
            assert_eq!(ENCIPHER[b], want, "encipher({b})");
        }
    }

    #[test]
    fn the_table_is_a_bijection_and_round_trips() {
        let mut seen = [false; 256];
        for b in 0..256 {
            assert!(!seen[DECIPHER[b] as usize], "decipher is not one-to-one");
            seen[DECIPHER[b] as usize] = true;
            assert_eq!(DECIPHER[ENCIPHER[b] as usize] as usize, b);
            assert_eq!(ENCIPHER[DECIPHER[b] as usize] as usize, b);
        }
        assert!(seen.iter().all(|s| *s));
    }

    /// The exact fixed points ExifTool's comment names, and no others. A table
    /// with the wrong number of fixed points is a different permutation.
    #[test]
    fn the_fixed_points_are_the_ones_exiftool_documents() {
        let fixed: Vec<usize> = (0..256).filter(|b| DECIPHER[*b] as usize == *b).collect();
        assert_eq!(
            fixed,
            vec![
                0, 1, 82, 83, 84, 165, 166, 167, 248, 249, 250, 251, 252, 253, 254, 255
            ]
        );
    }

    /// Deciphering with any wrong permutation still produces bytes -- which is
    /// the reason the tests above check the table and not the output. 246 of
    /// the 256 byte values move under the right table, so a block deciphered
    /// with the encipher table (the obvious direction error) differs almost
    /// everywhere, yet is just as parseable.
    #[test]
    fn a_wrong_table_still_produces_bytes_so_plausibility_proves_nothing() {
        let block: Vec<u8> = (0u8..=255).cycle().take(1024).collect();
        let right = decipher(&block, false);
        let mut wrong = block.clone();
        decipher_with(&ENCIPHER, &mut wrong);
        let differing = right.iter().zip(&wrong).filter(|(a, b)| a != b).count();
        assert!(
            differing * 100 >= right.len() * 90,
            "only {differing} of {} bytes differ",
            right.len()
        );
        // ...and every single-transposition perturbation of the table changes
        // the decode of a block that contains both swapped values.
        for i in 2u8..247 {
            let mut t = DECIPHER;
            t.swap(i as usize, (i as usize) + 1);
            let mut perturbed = block.clone();
            decipher_with(&t, &mut perturbed);
            assert_ne!(right, perturbed, "swap({i},{}) changed nothing", i + 1);
        }
    }

    /// The first 64 bytes of the *enciphered* 0x9050 block of
    /// `SonySLT-A77.jpg`, exactly as the file stores them.
    #[rustfmt::skip]
    const A77_9050_HEAD: [u8; 64] = [
        0x95, 0xd0, 0x00, 0x00, 0xff, 0x6d, 0x00, 0x00, 0xa0, 0x00, 0xf1, 0x1b, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x4f, 0x04, 0x2e, 0x70, 0x05, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xc4, 0x48, 0x1b, 0x00, 0x00, 0x13, 0x00, 0x00, 0x96, 0x00, 0x81, 0x00, 0x88, 0x00, 0x00,
    ];

    /// Every Tag9050a tag that falls in those 64 bytes, with the value
    /// `exiftool -G1 -s SonySLT-A77.jpg` reports for it (ExifTool 13.59).
    const A77_9050_TRUTH: &[(&str, &str)] = &[
        ("SonyMaxAperture", "2.8"),
        ("SonyMinAperture", "31"),
        ("Shutter", "Mechanical (2633 4286 5390)"),
        ("FlashStatus", "Built-in Flash present"),
        ("ShutterCount", "927"),
        ("SonyExposureTime", "1/256"),
        ("SonyFNumber", "2.8"),
        ("ReleaseMode2", "Normal"),
    ];

    /// Where each of those tags reads from: `(name, offset, byte length)`.
    const A77_9050_SPANS: &[(&str, usize, usize)] = &[
        ("SonyMaxAperture", 0x00, 1),
        ("SonyMinAperture", 0x01, 1),
        ("Shutter", 0x20, 6),
        ("FlashStatus", 0x31, 1),
        ("ShutterCount", 0x32, 4),
        ("SonyExposureTime", 0x3a, 2),
        ("SonyFNumber", 0x3c, 2),
        ("ReleaseMode2", 0x3f, 1),
    ];

    /// The byte values those tags read, which is what decides whether a
    /// perturbed table can change their decode at all.
    fn a77_read_bytes() -> Vec<u8> {
        A77_9050_SPANS
            .iter()
            .flat_map(|(_, off, len)| A77_9050_HEAD[*off..*off + *len].iter().copied())
            .collect()
    }

    /// Whether every byte a tag reads is one the cipher leaves alone.
    fn all_fixed_points(off: usize, len: usize) -> bool {
        A77_9050_HEAD[off..off + len]
            .iter()
            .all(|b| DECIPHER[*b as usize] == *b)
    }

    fn decode_a77_9050_with(table: &[u8; 256]) -> Vec<(String, String)> {
        let mut plain = A77_9050_HEAD.to_vec();
        decipher_with(table, &mut plain);
        let mut ctx = Ctx::new(Some("SLT-A77"), None);
        let mut out = Vec::new();
        binary_data::process(
            TABLES,
            idx::TAG9050A,
            &plain,
            ByteOrder::LittleEndian,
            &mut ctx,
            &mut out,
        );
        out.into_iter()
            .map(|f| (f.name.to_string(), f.value))
            .collect()
    }

    #[test]
    fn a_real_block_decodes_to_exiftools_values() {
        let got = decode_a77_9050_with(&DECIPHER);
        let want: Vec<(String, String)> = A77_9050_TRUTH
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        assert_eq!(got, want);
    }

    /// The end-to-end form of the table tests: a wrong permutation does not
    /// fail, it *lies*. Deciphered with the encipher table, the same 64 bytes
    /// still yield an aperture, a shutter triple, a flash state, a shutter
    /// count and an exposure time -- well-formed values, and every tag that
    /// reads a byte the cipher actually moves comes out different. Nothing
    /// about the output says which table was right.
    ///
    /// The exception is exact, not luck: 16 of the 256 byte values are fixed
    /// points (`b**3 == b` mod 249), so a tag reading only those decodes the
    /// same under *any* substitution table. `ReleaseMode2` reads a single
    /// `0x00` here and is therefore right by construction under a table that
    /// is wrong everywhere else -- which is the whole reason a spot-check of
    /// individual values cannot validate a cipher.
    #[test]
    fn the_wrong_table_yields_plausible_wrong_values() {
        let right: std::collections::HashMap<_, _> =
            decode_a77_9050_with(&DECIPHER).into_iter().collect();
        let wrong: std::collections::HashMap<_, _> =
            decode_a77_9050_with(&ENCIPHER).into_iter().collect();
        let mut immune = 0;
        for (name, off, len) in A77_9050_SPANS {
            let (r, w) = (right.get(*name), wrong.get(*name));
            if all_fixed_points(*off, *len) {
                immune += 1;
                assert_eq!(r, w, "{name} reads only fixed points, so it cannot move");
            } else {
                assert_ne!(r, w, "{name} survived the wrong table");
            }
        }
        assert_eq!(immune, 1, "exactly ReleaseMode2 reads only fixed points");
        // ...and the wrong values are the sort a reviewer would accept.
        assert!(
            wrong
                .get("Shutter")
                .is_some_and(|v| v.starts_with("Mechanical ("))
        );
        assert!(
            wrong
                .get("ShutterCount")
                .is_some_and(|v| v.parse::<u32>().is_ok())
        );
    }

    /// Every perturbation of the table that touches a byte value these tags
    /// actually read changes at least one of them: the decode depends on the
    /// exact permutation, not on a happy accident of the block's contents.
    #[test]
    fn every_perturbation_that_can_matter_does() {
        let right = decode_a77_9050_with(&DECIPHER);
        let read = a77_read_bytes();
        let mut tested = 0;
        for i in 2u8..247 {
            if !read.contains(&i) && !read.contains(&(i + 1)) {
                continue;
            }
            let mut t = DECIPHER;
            t.swap(i as usize, (i as usize) + 1);
            tested += 1;
            assert_ne!(
                right,
                decode_a77_9050_with(&t),
                "swapping {i} and {} changed nothing",
                i + 1
            );
        }
        assert_eq!(tested, 25, "the block reads a fixed set of byte values");
    }

    #[test]
    fn every_root_names_a_real_table() {
        for r in ROOTS {
            if let Some(t) = r.table {
                assert!(
                    t < TABLES.len(),
                    "root 0x{:04x} points past the table list",
                    r.tag
                );
            }
        }
    }
}
