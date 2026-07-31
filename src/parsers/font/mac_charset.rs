//! Macintosh CJK character sets used by TrueType `name` records.
//!
//! A `name` record on the Macintosh platform (platform ID 1) names its script
//! in the encoding ID. ExifTool maps those IDs through `%ttCharset{Macintosh}`
//! in `Font.pm` and hands the bytes to `Image::ExifTool::Charset::Decode`,
//! which decomposes them with the matching `Charset/Mac*.pm` table.
//!
//! The four CJK scripts are Apple's *variants* of Shift_JIS, Big5, EUC-KR and
//! GBK, not those codecs. Decoding them with a stock codec produces text that
//! differs from ExifTool's: measured against ExifTool 13.55's own tables,
//! MacJapanese agrees with Shift_JIS on 6878 of 7192 two-byte sequences (1
//! differs, 313 have no Shift_JIS mapping) and additionally remaps 68
//! single-byte values (0x5c is U+00A5 YEN SIGN, not a backslash); MacKorean
//! agrees with EUC-KR on 8212 of 9361 (13 differ, 1136 unmapped);
//! MacChineseTW agrees with Big5 on 13435 of 13461 (26 differ); MacChineseCN
//! agrees with GBK on 7462 of 7480 (6 differ, 12 unmapped). So the tables
//! themselves are carried verbatim -- see `generate_tables.py` in
//! `mac_charset/`, which transcribes them mechanically from the `.pm` files.
//!
//! [`decode`] reimplements the "variable-width characters" branch of
//! `Charset::Decompose` plus `Charset::Recompose`'s UTF-8 output, including
//! its two quirks: an unmapped trail byte emits `?` and is then reconsidered
//! as a lead byte, and the result is truncated at a NUL.

mod mac_chinese_cn;
mod mac_chinese_tw;
mod mac_japanese;
mod mac_korean;

/// One of ExifTool's `Charset/Mac*.pm` tables.
pub(super) struct MacCharset {
    /// Bytes that decode on their own, sorted by byte. A byte absent from
    /// both this and [`MacCharset::leads`] decodes to its own value, matching
    /// `Decompose`'s `$cv or push(@uni, $ch), next;`.
    single: &'static [(u8, &'static str)],
    /// Bytes that introduce a two-byte sequence (a nested hash in the Perl
    /// table), sorted. Held separately from `double` so that a lead byte with
    /// no valid trail bytes still consumes a byte and emits `?`.
    leads: &'static [u8],
    /// `lead << 8 | trail` -> replacement text, sorted by key.
    double: &'static [(u16, &'static str)],
    /// Whether the table remaps any byte below 0x80 -- the `0x080` bit of
    /// ExifTool's `%csType`. `ExifTool::Decode` skips the conversion entirely
    /// for an all-ASCII string unless this is set, so MacJapanese rewrites
    /// 0x5c in an otherwise-ASCII name and the other three do not.
    remaps_ascii: bool,
}

impl MacCharset {
    fn single(&self, byte: u8) -> Option<&'static str> {
        self.single
            .binary_search_by_key(&byte, |&(key, _)| key)
            .ok()
            .map(|index| self.single[index].1)
    }

    fn is_lead(&self, byte: u8) -> bool {
        self.leads.binary_search(&byte).is_ok()
    }

    fn double(&self, lead: u8, trail: u8) -> Option<&'static str> {
        let key = (u16::from(lead) << 8) | u16::from(trail);
        self.double
            .binary_search_by_key(&key, |&(k, _)| k)
            .ok()
            .map(|index| self.double[index].1)
    }
}

/// Returns the table for a Macintosh `name`-record encoding ID, if this
/// module carries one.
///
/// IDs are from `%ttCharset{Macintosh}` in ExifTool's `Font.pm`.
pub(super) fn for_mac_encoding(encoding_id: u16) -> Option<&'static MacCharset> {
    match encoding_id {
        1 => Some(&mac_japanese::MAC_JAPANESE),
        2 => Some(&mac_chinese_tw::MAC_CHINESE_TW),
        3 => Some(&mac_korean::MAC_KOREAN),
        25 => Some(&mac_chinese_cn::MAC_CHINESE_CN),
        _ => None,
    }
}

/// Decodes `data` the way `ExifTool::Decode(..., $charset)` does.
pub(super) fn decode(data: &[u8], charset: &MacCharset) -> String {
    // `ExifTool::Decode` short-circuits when no character can need remapping,
    // which for these tables means an all-ASCII string with no sub-0x80
    // overrides. Skipping the conversion also skips the NUL truncation below,
    // so this is not merely an optimisation.
    if !charset.remaps_ascii && data.iter().all(|&byte| byte < 0x80) {
        return data.iter().map(|&byte| char::from(byte)).collect();
    }

    let mut out = String::with_capacity(data.len());
    let mut index = 0;
    while index < data.len() {
        let lead = data[index];
        index += 1;

        if let Some(text) = charset.single(lead) {
            out.push_str(text);
        } else if charset.is_lead(lead) {
            match data.get(index) {
                // A trail byte with no mapping is an encoding error: ExifTool
                // emits '?' and pushes the byte back, so it is reconsidered as
                // a lead byte rather than swallowed.
                Some(&trail) => match charset.double(lead, trail) {
                    Some(text) => {
                        out.push_str(text);
                        index += 1;
                    }
                    None => out.push('?'),
                },
                // Lead byte at the end of the string.
                None => out.push('?'),
            }
        } else {
            // Untranslated bytes keep their value as a codepoint.
            out.push(char::from(lead));
        }
    }

    // `Recompose` truncates its UTF-8 output at the first NUL.
    if let Some(nul) = out.find('\0') {
        out.truncate(nul);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four Macintosh CJK `FontSubfamily` records in the shared
    /// `Font.ttf` corpus sample, with the values ExifTool 13.55 prints for
    /// them (`exiftool -s -FontSubfamily-ja ... Font.ttf`, and so on).
    #[test]
    fn decodes_font_ttf_name_records() {
        let cases: &[(u16, &[u8], &str)] = &[
            (
                1,
                &[0x83, 0x8c, 0x83, 0x4d, 0x83, 0x85, 0x83, 0x89, 0x81, 0x5b],
                "レギュラー",
            ),
            (2, &[0xbc, 0xd0, 0xb7, 0xc7, 0xc5, 0xe9], "標準體"),
            (3, &[0xc0, 0xcf, 0xb9, 0xdd], "일반"),
            (25, &[0xb3, 0xa3, 0xb9, 0xe6], "常规"),
        ];
        for &(encoding_id, bytes, expected) in cases {
            let charset = for_mac_encoding(encoding_id).expect("charset is carried");
            assert_eq!(decode(bytes, charset), expected, "encoding {encoding_id}");
        }
    }

    #[test]
    fn only_the_four_cjk_scripts_are_carried() {
        // MacRoman (0) and MacHebrew (5) are decoded by TTFParser itself, and
        // the remaining Macintosh scripts are still an open gap.
        for encoding_id in [0, 4, 5, 6, 7, 9, 21, 26, 32] {
            assert!(for_mac_encoding(encoding_id).is_none(), "{encoding_id}");
        }
    }

    #[test]
    fn ascii_passes_through_except_where_the_table_overrides_it() {
        // MacJapanese is ExifTool csType 0x883: the 0x080 bit says bytes below
        // 0x80 are remapped, and 0x5c is one of them.
        let japanese = for_mac_encoding(1).unwrap();
        assert_eq!(decode(b"a\\b", japanese), "a\u{a5}b");
        // MacChineseCN is 0x803 and leaves ASCII alone.
        let chinese = for_mac_encoding(25).unwrap();
        assert_eq!(decode(b"a\\b", chinese), "a\\b");
    }

    #[test]
    fn one_byte_can_yield_several_characters() {
        // MacJapanese 0xff => [0x2026, 0xf87f]; ExifTool keeps the private-use
        // marker that Apple's mapping table carries.
        let japanese = for_mac_encoding(1).unwrap();
        assert_eq!(decode(&[0xff], japanese), "\u{2026}\u{f87f}");
        // MacKorean 0xa141 => [0x300c, 0xf87f]; 843 of its two-byte sequences
        // decompose into more than one character.
        let korean = for_mac_encoding(3).unwrap();
        assert_eq!(decode(&[0xa1, 0x41], korean), "\u{300c}\u{f87f}");
    }

    #[test]
    fn encoding_errors_match_exiftool() {
        let japanese = for_mac_encoding(1).unwrap();
        // 0x81 is a lead byte but 0x81 0x20 has no mapping: ExifTool emits '?'
        // and then reconsiders 0x20, which decodes as a space.
        assert_eq!(decode(&[0x81, 0x20], japanese), "? ");
        // A lead byte at the end of the string is also '?'.
        assert_eq!(decode(&[0x81], japanese), "?");
        // An untranslated byte keeps its own value as a codepoint.
        assert_eq!(decode(&[0xf0], japanese), "\u{f0}");
    }

    #[test]
    fn output_is_truncated_at_a_nul() {
        let japanese = for_mac_encoding(1).unwrap();
        assert_eq!(decode(b"ab\0cd", japanese), "ab");
    }

    #[test]
    fn tables_are_sorted_and_consistent() {
        for encoding_id in [1, 2, 3, 25] {
            let charset = for_mac_encoding(encoding_id).unwrap();
            assert!(
                charset.single.windows(2).all(|w| w[0].0 < w[1].0),
                "single table for {encoding_id} must be sorted for binary search"
            );
            assert!(
                charset.leads.windows(2).all(|w| w[0] < w[1]),
                "lead table for {encoding_id} must be sorted for binary search"
            );
            assert!(
                charset.double.windows(2).all(|w| w[0].0 < w[1].0),
                "double table for {encoding_id} must be sorted for binary search"
            );
            // A byte is either a standalone character or a lead byte, never
            // both -- they are distinct values of one Perl hash key.
            assert!(
                charset
                    .single
                    .iter()
                    .all(|&(byte, _)| !charset.leads.contains(&byte)),
                "a byte cannot be both standalone and a lead byte ({encoding_id})"
            );
            // Every two-byte key must have its lead byte registered, or the
            // decoder would never reach it.
            assert!(
                charset
                    .double
                    .iter()
                    .all(|&(key, _)| charset.leads.contains(&((key >> 8) as u8))),
                "two-byte key without a registered lead byte ({encoding_id})"
            );
        }
    }

    #[test]
    fn table_sizes_match_exiftool() {
        // Guards against a regeneration silently dropping entries. Counts are
        // from ExifTool 13.55's Charset/Mac*.pm.
        let expected: &[(u16, usize, usize)] =
            &[(1, 68, 7192), (2, 6, 13461), (3, 6, 9361), (25, 6, 7480)];
        for &(encoding_id, singles, doubles) in expected {
            let charset = for_mac_encoding(encoding_id).unwrap();
            assert_eq!(charset.single.len(), singles, "singles {encoding_id}");
            assert_eq!(charset.double.len(), doubles, "doubles {encoding_id}");
        }
    }
}
