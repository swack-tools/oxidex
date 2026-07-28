"""Hermetic tests for verify_enum_maps.py.

Almost everything here is a pure text fixture: tiny .pm files written into a
tempdir and unified diffs built as string literals, so no test reads the live
repo, ~/.oxidex, or /tmp/oxidex-exiftool-cache.

The exception is AcceptanceAgainstRealCommits at the bottom, which is the whole
point of the component: it re-runs the three commits from 2026-07-26 that
motivated this tier (TTF 5249a506 fabricated, RAR a998b8fc fabricated, RW2
e0900a27 clean) against the real ExifTool checkout. Those tests skip -- loudly,
with a reason -- when the repo or the ExifTool cache is not present, so the file
still passes in a bare checkout; they must never be weakened to fit the code.

Perl fixtures below are copied byte-for-byte from the real modules wherever the
byte sequence is what is being tested (Font.pm's multi-column %ttLang rows,
Exif.pm's q{} block containing an apostrophe, QuickTime.pm's apostrophe-bearing
double-quoted values).
"""
import io
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

import verify_enum_maps
from verify_enum_maps import (
    canonical_key,
    extract_rust_pairs,
    main,
    parse_perl_table,
    parse_perl_table_detail,
    verify,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
PERL_LIB = verify_enum_maps.DEFAULT_PERL_LIB


def write_pm(tmpdir, name, text):
    path = Path(tmpdir) / name
    path.write_text(textwrap.dedent(text), encoding="utf-8")
    return path


def diff(path, hunk_start, lines):
    """Build a unified diff. `lines` are already prefixed with ' ', '+' or '-'."""
    added = sum(1 for l in lines if l[0] in " +")
    removed = sum(1 for l in lines if l[0] in " -")
    head = (
        f"diff --git a/{path} b/{path}\n"
        f"--- a/{path}\n"
        f"+++ b/{path}\n"
        f"@@ -{hunk_start},{removed} +{hunk_start},{added} @@\n"
    )
    return head + "\n".join(lines) + "\n"


# --------------------------------------------------------------------------


class CanonicalKeyTests(unittest.TestCase):
    def test_hex_folds_to_decimal_so_both_languages_meet(self):
        # Rust writes 0x0c0a, Font.pm writes 0x0c0a, Exif.pm writes 0xa401 in
        # lower case while the Rust diff writes 0xA401. All four must land on
        # the same string or the pair diff silently compares nothing.
        self.assertEqual(canonical_key("0x0c0a"), "3082")
        self.assertEqual(canonical_key("0x0C0A"), "3082")
        self.assertEqual(canonical_key("0xa401"), canonical_key("0xA401"))
        self.assertEqual(canonical_key("12"), "12")
        self.assertEqual(canonical_key("012"), "12")

    def test_fractional_keys_are_not_numeric_normalised(self):
        # Canon.pm carries 215 fractional lens-ID keys (2.1, 8.1, 169.7).
        # Coercing them to int merges 2.1 into 2 and the diff starts comparing
        # the wrong lens.
        self.assertEqual(canonical_key("2.1"), "2.1")
        self.assertNotEqual(canonical_key("2.1"), canonical_key("2"))

    def test_negative_and_string_keys_survive(self):
        self.assertEqual(canonical_key("-1"), "-1")
        self.assertEqual(canonical_key("-559038737"), "-559038737")
        # EXE.pm '0401' is a zero-padded STRING key, not 401 and not 0x401.
        self.assertEqual(canonical_key("aax "), "aax ")


class PerlParserTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = self.tmp.name

    def test_inline_printconv_with_commas_inside_values(self):
        # Exif.pm:184-193 flash values contain commas; a comma-splitter loses
        # half of every one of them.
        pm = write_pm(
            self.dir,
            "Demo.pm",
            """\
            %Image::ExifTool::Demo::Main = (
                0x9209 => {
                    Name => 'Flash',
                    PrintConv => {
                        0x05 => 'Fired, Return not detected',
                        0x14 => 'Off, Did not fire, Return not detected',
                    },
                },
            );
            """,
        )
        table = parse_perl_table(pm, "Main.0x9209")
        self.assertEqual(
            table, {"5": "Fired, Return not detected", "20": "Off, Did not fire, Return not detected"}
        )

    def test_multi_column_layout_and_trailing_comments(self):
        # Verbatim shapes from Font.pm:51/59 and Nikon.pm:1292 -- several pairs
        # per source line, a trailing comment whose text contains a comma, an
        # empty-string value, and a bare unquoted value (Nikon.pm:793).
        pm = write_pm(
            self.dir,
            "Demo.pm",
            """\
            %cols = (
              7 => 'MacCyrillic',   24 => 'MacArmenian', # 7=Russian
              15 => 'MacTelugu',    32 => '', # 32=uninterpreted
              112 => 'f/5.0', 313 => 'N/A',     #camera menu shows "--", value not set,
              99 => 0,
            );
            """,
        )
        self.assertEqual(
            parse_perl_table(pm, "cols"),
            {
                "7": "MacCyrillic",
                "24": "MacArmenian",
                "15": "MacTelugu",
                "32": "",
                "112": "f/5.0",
                "313": "N/A",
                "99": "0",
            },
        )

    def test_commented_out_speculative_pairs_are_not_extracted(self):
        # Canon.pm:146 `# 27 => 'Carl Zeiss Distagon T* 28mm f/2 ZF'` is a
        # SUSPECTED mapping ExifTool has not confirmed. Harvesting it would
        # bless a guess as ground truth and let a fabrication through.
        pm = write_pm(
            self.dir,
            "Demo.pm",
            """\
            %lens = (
                1 => 'Canon EF 50mm f/1.8',
            #   27 => 'Carl Zeiss Distagon T* 28mm f/2 ZF', #PH
                # 22 - Custom 2?
                2 => 'Canon EF 28mm f/2.8',
            );
            """,
        )
        self.assertEqual(parse_perl_table(pm, "lens"), {"1": "Canon EF 50mm f/1.8", "2": "Canon EF 28mm f/2.8"})

    def test_double_quoted_values_containing_apostrophes(self):
        # QuickTime.pm:3666/3767. A single-quote-only tokenizer drops these
        # silently, which reads as "ExifTool does not define that key" -- i.e.
        # it turns a correct Rust value into a reported fabrication.
        pm = write_pm(
            self.dir,
            "Demo.pm",
            """\
            %genre = (
                4 => "Music|Children's Music",
                1049 => "Music|Dance|Jungle/Drum'n'bass",
            );
            """,
        )
        self.assertEqual(
            parse_perl_table(pm, "genre"),
            {"4": "Music|Children's Music", "1049": "Music|Dance|Jungle/Drum'n'bass"},
        )

    def test_q_block_containing_an_apostrophe_does_not_desync_the_parser(self):
        # THE Exif.pm regression, verbatim from %Main tag 0x117: a q{...} whose
        # body contains "various IFD's of DNG images". Treating that apostrophe
        # as an opening quote ran the scanner to the next apostrophe several
        # entries later, ate a closing brace, and collapsed %Main from ~600
        # top-level entries to 24 -- so every Exif.pm lookup came back
        # "no-such-table", a cannot-verify that looked principled and was a bug.
        pm = write_pm(
            self.dir,
            "Demo.pm",
            """\
            %Image::ExifTool::Demo::Main = (
                0x117 => {
                    Name => 'StripByteCounts',
                    Notes => q{
                        called StripByteCounts in most locations, but it is
                        PreviewImageLength in IFD0 of CR2 images and various IFD's
                        of DNG images except for SubIFD2
                    },
                    Writable => 'int32u',
                },
                0xa402 => {
                    Name => 'ExposureMode',
                    PrintConv => {
                        0 => 'Auto',
                        1 => 'Manual',
                        2 => 'Auto bracket',
                    },
                },
            );
            """,
        )
        self.assertEqual(parse_perl_table(pm, "Main.0xa402"), {"0": "Auto", "1": "Manual", "2": "Auto bracket"})

    def test_nested_subtable_selection(self):
        pm = write_pm(
            self.dir,
            "Demo.pm",
            """\
            %ttLang = (
              Macintosh => {
                3 => 'it',     27 => 'et',
                4 => 'nl-NL',  28 => 'lv',
                6 => 'es',     30 => 'fo',
                12 => 'ar',    36 => 'sq',
              },
              Windows => {
                0x0410 => 'it-IT', 0x0c0a => 'es-ES',
              },
            );
            """,
        )
        mac = parse_perl_table(pm, "ttLang.Macintosh")
        self.assertEqual(mac["12"], "ar")
        self.assertEqual(mac["4"], "nl-NL")
        self.assertEqual(mac["6"], "es")
        self.assertEqual(mac["3"], "it")
        self.assertNotIn("3082", mac, "Windows keys must not leak into the Macintosh sub-table")
        self.assertEqual(parse_perl_table(pm, "%ttLang{Windows}")["3082"], "es-ES")

    def test_hashref_printconv_is_followed_within_the_module(self):
        pm = write_pm(
            self.dir,
            "Demo.pm",
            """\
            my %wb = (
                0 => 'Auto',
                1 => 'Daylight',
            );
            %Image::ExifTool::Demo::Main = (
                0x7 => {
                    Name => 'WhiteBalance',
                    PrintConv => \\%wb,
                },
            );
            """,
        )
        self.assertEqual(parse_perl_table(pm, "Main.0x7"), {"0": "Auto", "1": "Daylight"})

    def test_other_bitmask_empty_and_code_all_return_none(self):
        pm = write_pm(
            self.dir,
            "Demo.pm",
            """\
            %withOther = (
                0 => 'Normal',
                OTHER => \\&PrintParameter,
            );
            %withBitmask = (
                BITMASK => {
                    0 => '2-Dimensional encoding',
                    1 => 'Uncompressed',
                },
            );
            %emptyish = (
            );
            %Image::ExifTool::Demo::Main = (
                0x132 => {
                    Name => 'ModifyDate',
                    PrintConv => '$self->ConvertDateTime($val)',
                },
                0x133 => {
                    Name => 'Coded',
                    PrintConv => sub { my $v = shift; return "x$v" },
                },
                0x134 => {
                    Name => 'Missing',
                    Writable => 'int16u',
                },
            );
            """,
        )
        for hint, reason in [
            ("withOther", "table-has-OTHER"),
            ("withBitmask", "table-is-BITMASK"),
            ("emptyish", "table-empty"),
            ("Main.0x132", "printconv-is-code"),
            ("Main.0x133", "printconv-is-code"),
            ("Main.0x134", "no-printconv"),
        ]:
            with self.subTest(hint=hint):
                detail = parse_perl_table_detail(pm, hint)
                self.assertIsNone(detail.pairs)
                self.assertEqual(detail.reason, reason)

    def test_ambiguous_tag_name_names_every_candidate(self):
        # The ZIP.pm shape: three tables define OperatingSystem and they
        # disagree (value 2 is 'VMS (or OpenVMS)', 'Win32', and undefined).
        pm = write_pm(
            self.dir,
            "ZIPish.pm",
            """\
            %Image::ExifTool::ZIPish::GZIP = (
                9 => {
                    Name => 'OperatingSystem',
                    PrintConv => { 0 => 'FAT filesystem', 2 => 'VMS (or OpenVMS)', 3 => 'Unix' },
                },
            );
            %Image::ExifTool::ZIPish::RAR = (
                8 => {
                    Name => 'OperatingSystem',
                    PrintConv => { 0 => 'MS-DOS', 2 => 'Win32' },
                },
            );
            %Image::ExifTool::ZIPish::RAR5 = (
                OperatingSystem => {
                    PrintConv => { 0 => 'Win32', 1 => 'Unix' },
                },
            );
            """,
        )
        detail = parse_perl_table_detail(pm, "OperatingSystem")
        self.assertIsNone(detail.pairs)
        self.assertEqual(detail.reason, "ambiguous-table")
        self.assertEqual(len(detail.candidates), 3)
        # ...but the qualified hint resolves to exactly one.
        self.assertEqual(parse_perl_table(pm, "RAR5.OperatingSystem"), {"0": "Win32", "1": "Unix"})
        self.assertEqual(parse_perl_table(pm, "RAR.8")["2"], "Win32")

    def test_missing_module_and_missing_table(self):
        self.assertIsNone(parse_perl_table(Path(self.dir) / "Nope.pm", "Main"))
        pm = write_pm(self.dir, "Demo.pm", "%a = ( 0 => 'x' );\n")
        self.assertEqual(parse_perl_table_detail(pm, "NotThere").reason, "no-such-table")


class RustExtractionTests(unittest.TestCase):
    def test_plain_arms_in_every_wrapper_this_codebase_uses(self):
        d = diff(
            "src/x.rs",
            10,
            [
                " fn f(v: u8) -> Option<String> {",
                "     match v {",
                '+        0 => Some("Normal".to_string()),',
                '+        1 => Some(String::from("Custom")),',
                '+        2 => "Bare",',
                '+        3 => "Owned".to_string(),',
                "     }",
                " }",
            ],
        )
        pairs = extract_rust_pairs(d)
        self.assertEqual(
            [(p.key, p.value) for p in pairs if p.kind == "pair"],
            [("0", "Normal"), ("1", "Custom"), ("2", "Bare"), ("3", "Owned")],
        )
        self.assertEqual([p.file for p in pairs], ["src/x.rs"] * 4)
        self.assertEqual([p.line for p in pairs], [12, 13, 14, 15])

    def test_first_four_fields_are_the_promised_contract(self):
        d = diff("src/x.rs", 1, ['+        7 => "Seven",'])
        key, value, file, line = extract_rust_pairs(d)[0][:4]
        self.assertEqual((key, value, file, line), ("7", "Seven", "src/x.rs", 1))

    def test_const_paired_tuple_arms_spanning_two_lines(self):
        # The TTF 5249a506 shape exactly: the const is on an added line, the
        # tuple pattern is on one line and its `=>` on the next.
        d = diff(
            "src/parsers/font/ttf.rs",
            24,
            [
                " const PLATFORM_MACINTOSH: u16 = 1;",
                " const PLATFORM_WINDOWS: u16 = 3;",
                "+const LANGUAGE_SPANISH_MACINTOSH: u16 = 12;",
                "+const LANGUAGE_SPANISH_WINDOWS: u16 = 0x0c0a;",
                "         match (platform_id, language_id) {",
                "+            (PLATFORM_MACINTOSH, LANGUAGE_SPANISH_MACINTOSH)",
                '+            | (PLATFORM_WINDOWS, LANGUAGE_SPANISH_WINDOWS) => Some("es"),',
                "             _ => None,",
                "         }",
            ],
        )
        pairs = [p for p in extract_rust_pairs(d) if p.kind == "pair"]
        self.assertEqual([(p.key, p.value) for p in pairs], [("12", "es"), ("3082", "es")])
        self.assertEqual(
            pairs[0].key_parts,
            (("PLATFORM_MACINTOSH", "1"), ("LANGUAGE_SPANISH_MACINTOSH", "12")),
        )
        # The Windows const is hex; both sides must canonicalise the same way.
        self.assertEqual(pairs[1].key_parts[1], ("LANGUAGE_SPANISH_WINDOWS", "3082"))

    def test_consts_the_diff_does_not_show_come_from_extra_consts(self):
        d = diff(
            "src/x.rs",
            5,
            [
                "+            (PLATFORM_MACINTOSH, LANG_IT) => Some(\"it\"),",
            ],
        )
        pairs = extract_rust_pairs(d, extra_consts={"PLATFORM_MACINTOSH": "1", "LANG_IT": "3"})
        self.assertEqual([(p.key, p.value) for p in pairs], [("3", "it")])

    def test_catch_all_string_is_its_own_kind_and_none_is_silent(self):
        d = diff(
            "src/x.rs",
            1,
            [
                '+        0 => "Win32",',
                '+        _ => "Unknown",',
                "+        _ => None,",
            ],
        )
        pairs = extract_rust_pairs(d)
        kinds = [(p.kind, p.value) for p in pairs]
        self.assertIn(("catch-all", "Unknown"), kinds)
        self.assertIn(("catch-all", None), kinds)
        self.assertIn(("pair", "Win32"), kinds)

    def test_context_only_arms_are_ignored(self):
        # This tier judges what the commit INTRODUCES. Flagging inherited arms
        # would make every quarantine re-adjudication re-litigate main.
        d = diff(
            "src/x.rs",
            1,
            [
                '         0 => "Inherited",',
                '+        1 => "Added",',
            ],
        )
        self.assertEqual([(p.key, p.value) for p in extract_rust_pairs(d)], [("1", "Added")])

    def test_arrows_and_commas_inside_strings_and_comments_do_not_split_arms(self):
        d = diff(
            "src/x.rs",
            1,
            [
                '+        0 => "Off, Did not fire, Return not detected", // 5 => not a pair',
                '+        1 => "a => b",',
            ],
        )
        self.assertEqual(
            [(p.key, p.value) for p in extract_rust_pairs(d)],
            [("0", "Off, Did not fire, Return not detected"), ("1", "a => b")],
        )

    def test_two_unrelated_tables_in_one_diff_get_separate_blocks(self):
        # RW2 e0900a27 adds CustomRendered {0: Normal} and ExposureMode
        # {0: Auto} in one commit. Flattened, key 0 collides and one of them is
        # reported as a fabrication of the other.
        d = diff(
            "src/parsers/raw/metadata.rs",
            860,
            [
                "         // CustomRendered: SHORT[1].",
                "+        0xA401 if field_type == 3 => match read_u16(bytes)? {",
                '+            0 => Some("Normal".to_string()),',
                "+            _ => None,",
                "+        },",
                "+        // ExposureMode: SHORT[1].",
                "+        0xA402 if field_type == 3 => match read_u16(bytes)? {",
                '+            0 => Some("Auto".to_string()),',
                "+            _ => None,",
                "+        },",
            ],
        )
        pairs = [p for p in extract_rust_pairs(d) if p.kind == "pair"]
        self.assertEqual(len({p.block for p in pairs}), 2)
        self.assertEqual(sorted(p.block_key for p in pairs), ["41985", "41986"])

    def test_non_rust_hunks_are_ignored(self):
        d = diff("docs/notes.md", 1, ['+        0 => "Win32",'])
        self.assertEqual(extract_rust_pairs(d), [])

    def test_empty_diff(self):
        self.assertEqual(extract_rust_pairs(""), [])


class VerifyTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = self.tmp.name
        self.pm = write_pm(
            self.dir,
            "Demo.pm",
            """\
            %ttLang = (
              Macintosh => {
                1 => 'fr',     3 => 'it',     4 => 'nl-NL',
                6 => 'es',     12 => 'ar',    13 => 'fi',
              },
              Windows => {
                0x0c0a => 'es-ES', 0x0410 => 'it-IT',
              },
            );
            %Image::ExifTool::Demo::RAR5 = (
                OperatingSystem => {
                    PrintConv => { 0 => 'Win32', 1 => 'Unix' },
                },
            );
            %Image::ExifTool::Demo::RAR = (
                8 => {
                    Name => 'OperatingSystem',
                    PrintConv => { 0 => 'MS-DOS', 2 => 'Win32' },
                },
            );
            %Image::ExifTool::Demo::Main = (
                0xa401 => {
                    Name => 'CustomRendered',
                    PrintConv => { 0 => 'Normal', 1 => 'Custom' },
                },
                0xa402 => {
                    Name => 'ExposureMode',
                    PrintConv => { 0 => 'Auto', 1 => 'Manual', 2 => 'Auto bracket' },
                },
                0x132 => {
                    Name => 'ModifyDate',
                    PrintConv => '$self->ConvertDateTime($val)',
                },
            );
            """,
        )

    def _ttf_diff(self):
        return diff(
            "src/parsers/font/ttf.rs",
            24,
            [
                " const PLATFORM_MACINTOSH: u16 = 1;",
                " const PLATFORM_WINDOWS: u16 = 3;",
                "+const LANGUAGE_SPANISH_MACINTOSH: u16 = 12;",
                "+const LANGUAGE_SPANISH_WINDOWS: u16 = 0x0c0a;",
                "+const LANGUAGE_ITALIAN_MACINTOSH: u16 = 4;",
                "+const LANGUAGE_ITALIAN_WINDOWS: u16 = 0x0410;",
                "+const LANGUAGE_FRENCH_MACINTOSH: u16 = 1;",
                "+const LANGUAGE_FRENCH_WINDOWS: u16 = 0x040c;",
                "         match (platform_id, language_id) {",
                "+            (PLATFORM_MACINTOSH, LANGUAGE_SPANISH_MACINTOSH)",
                '+            | (PLATFORM_WINDOWS, LANGUAGE_SPANISH_WINDOWS) => Some("es"),',
                "+            (PLATFORM_MACINTOSH, LANGUAGE_ITALIAN_MACINTOSH)",
                '+            | (PLATFORM_WINDOWS, LANGUAGE_ITALIAN_WINDOWS) => Some("it"),',
                "+            (PLATFORM_MACINTOSH, LANGUAGE_FRENCH_MACINTOSH)",
                '+            | (PLATFORM_WINDOWS, LANGUAGE_FRENCH_WINDOWS) => Some("fr"),',
                "             _ => None,",
                "         }",
            ],
        )

    def test_wrong_pairing_is_fabricated_even_though_every_string_exists(self):
        # This is the exact failure check_printconv cannot see: "es" and "it"
        # both appear in the table, at keys 6 and 3. The pairing is what is
        # wrong. A substring check passes; a pair diff does not.
        v = verify(self._ttf_diff(), self.pm, "ttLang.Macintosh")
        self.assertEqual(v.status, "fabricated")
        by_key = {m.key: m for m in v.mismatches}
        self.assertEqual(by_key["12"].rust_says, "es")
        self.assertEqual(by_key["12"].exiftool_says, "ar")
        self.assertEqual(by_key["12"].exiftool_key_for_value, "6")
        self.assertEqual(by_key["4"].exiftool_says, "nl-NL")
        self.assertEqual(by_key["4"].exiftool_key_for_value, "3")
        # French 1 => 'fr' is correct and must NOT be flagged.
        self.assertNotIn("1", by_key)
        self.assertEqual(v.pairs_checked, 3)

    def test_subtable_selection_filters_the_other_platform_into_unreachable(self):
        v = verify(self._ttf_diff(), self.pm, "ttLang.Macintosh")
        reasons = {u.reason for u in v.unreachable}
        self.assertEqual(reasons, {"subtable-filtered:PLATFORM_WINDOWS"})
        self.assertEqual({u.key for u in v.unreachable}, {"3082", "1040", "1036"})

    def test_the_other_subtable_can_be_selected_too(self):
        v = verify(self._ttf_diff(), self.pm, "ttLang.Windows")
        self.assertEqual(v.status, "fabricated")
        by_key = {m.key: m for m in v.mismatches}
        # Windows 0x0c0a is 'es-ES', not 'es'.
        self.assertEqual(by_key["3082"].exiftool_says, "es-ES")

    def test_absent_keys_and_a_catch_all_are_separate_finding_classes(self):
        d = diff(
            "src/parsers/archive/rar.rs",
            560,
            [
                " fn rar5_host_os(raw: u8) -> &'static str {",
                "     match raw {",
                '+        0 => "Win32",',
                '+        1 => "Unix",',
                '+        2 => "MacOS",',
                '+        _ => "Unknown",',
                "     }",
                " }",
            ],
        )
        v = verify(d, self.pm, "RAR5.OperatingSystem")
        self.assertEqual(v.status, "fabricated")
        self.assertEqual([(m.key, m.rust_says, m.exiftool_says) for m in v.mismatches], [("2", "MacOS", None)])
        self.assertEqual([c.value for c in v.catch_all_arms], ["Unknown"])
        self.assertTrue(v.catch_all_arms[0].data_replacing)

    def test_a_catch_all_alone_is_enough_to_fail(self):
        # No mismatched pair at all -- every listed key is right -- but the
        # catch-all still replaces ExifTool's "Unknown (2)" for every value the
        # table does not cover.
        d = diff("src/x.rs", 1, ['+        0 => "Win32",', '+        1 => "Unix",', '+        _ => "Unknown",'])
        v = verify(d, self.pm, "RAR5.OperatingSystem")
        self.assertEqual(v.status, "fabricated")
        self.assertEqual(v.mismatches, [])
        self.assertEqual(len(v.catch_all_arms), 1)

    def test_a_catch_all_alone_does_NOT_convict_when_no_table_resolved(self):
        # The mirror of the test above, and the distinction that matters:
        # there the table RESOLVED, so we know ExifTool has no fallback and a
        # `_ =>` arm really does replace a raw number. Here nothing resolved,
        # so we cannot know whether ExifTool has an OTHER sub covering exactly
        # those keys -- and a `_ =>` fallback is idiomatic Rust present in most
        # parsers.
        #
        # Measured 2026-07-27: convicting unconditionally produced FALSE
        # REJECTS for two verified-good archived patches (pdf-a1a411f67e3f,
        # which measures a real -4 gap closure, and elf-4b5a26e97cb8, whose
        # 9/9 CPUType and 5/5 ObjectFileType pairs match EXE.pm). "fabricated"
        # is terminal, so a wrong one permanently discards good work, while
        # "cannot-verify" merely defers.
        # NO table hint, deliberately. With an explicit hint an unresolvable
        # table already short-circuits to cannot-verify before the verdict is
        # computed, so only the AUTO-BINDING path could ever exhibit this --
        # and that is exactly how pdf/elf were adjudicated (no --table).
        # An earlier version of this test passed a bogus hint instead and was
        # therefore vacuous: it stayed green with the fix reverted.
        d = diff("src/x.rs", 1, ['+        0 => "Whatever",', '+        _ => "Unknown",'])
        v = verify(d, self.pm, None)
        self.assertEqual(v.status, "cannot-verify")
        self.assertEqual(v.pairs_checked, 0)
        self.assertTrue(v.catch_all_arms, "the catch-all must still be REPORTED, just not convicted on")

    def test_none_catch_all_does_not_fail(self):
        d = diff("src/x.rs", 1, ['+        0 => Some("Win32".to_string()),', "+        _ => None,"])
        v = verify(d, self.pm, "RAR5.OperatingSystem")
        self.assertEqual(v.status, "clean")
        self.assertEqual(v.catch_all_arms, [])

    def test_clean_run_reports_what_it_actually_checked(self):
        d = diff("src/x.rs", 1, ['+        0 => "Win32",', '+        1 => "Unix",'])
        v = verify(d, self.pm, "RAR5.OperatingSystem")
        self.assertEqual(v.status, "clean")
        self.assertEqual(v.pairs_checked, 2)
        self.assertEqual(v.mismatches, [])

    def test_ambiguous_hint_refuses_and_names_the_candidates(self):
        d = diff("src/x.rs", 1, ['+        0 => "Win32",'])
        v = verify(d, self.pm, "OperatingSystem")
        self.assertEqual(v.status, "cannot-verify")
        self.assertEqual(v.reason, "ambiguous-table")
        self.assertEqual(len(v.candidates), 2)
        # The two candidates disagree about key 0 -- picking either would be a
        # coin flip dressed up as a verdict.
        self.assertEqual(v.pairs_checked, 0)

    def test_code_printconv_refuses_rather_than_passing(self):
        d = diff("src/x.rs", 1, ['+        0 => "2026:01:01",'])
        v = verify(d, self.pm, "Main.0x132")
        self.assertEqual(v.status, "cannot-verify")
        self.assertEqual(v.reason, "printconv-is-code")
        self.assertEqual([u.reason for u in v.unreachable], ["printconv-is-code"])

    def test_missing_module_refuses(self):
        d = diff("src/x.rs", 1, ['+        0 => "Win32",'])
        v = verify(d, Path(self.dir) / "Nope.pm", "RAR5.OperatingSystem")
        self.assertEqual(v.status, "cannot-verify")
        self.assertEqual(v.reason, "no-such-module")

    def test_per_block_mode_binds_each_match_to_its_own_tag_id(self):
        d = diff(
            "src/parsers/raw/metadata.rs",
            860,
            [
                "+        0xA401 if field_type == 3 => match read_u16(bytes)? {",
                '+            0 => Some("Normal".to_string()),',
                '+            1 => Some("Custom".to_string()),',
                "+            _ => None,",
                "+        },",
                "+        0xA402 if field_type == 3 => match read_u16(bytes)? {",
                '+            0 => Some("Auto".to_string()),',
                '+            2 => Some("Auto bracket".to_string()),',
                "+            _ => None,",
                "+        },",
            ],
        )
        v = verify(d, self.pm, None)
        self.assertEqual(v.status, "clean")
        self.assertEqual(v.pairs_checked, 4)
        self.assertIn("0xa401", v.table)
        self.assertIn("0xa402", v.table)

    def test_per_block_mode_still_catches_a_fabrication_in_one_block_only(self):
        d = diff(
            "src/parsers/raw/metadata.rs",
            860,
            [
                "+        0xA401 if field_type == 3 => match read_u16(bytes)? {",
                '+            0 => Some("Normal".to_string()),',
                "+        },",
                "+        0xA402 if field_type == 3 => match read_u16(bytes)? {",
                '+            1 => Some("Auto bracket".to_string()),',
                "+        },",
            ],
        )
        v = verify(d, self.pm, None)
        self.assertEqual(v.status, "fabricated")
        self.assertEqual(
            [(m.key, m.rust_says, m.exiftool_says) for m in v.mismatches],
            [("1", "Auto bracket", "Manual")],
        )

    def test_per_block_mode_refuses_when_a_block_cannot_be_bound(self):
        d = diff("src/x.rs", 1, ['+        0 => "Whatever",'])
        v = verify(d, self.pm, None)
        self.assertEqual(v.status, "cannot-verify")

    def test_exercised_keys_makes_the_recheck_blind_spot_explicit(self):
        # Both fabrications on 2026-07-26 survived because the sample did not
        # exercise them. Handing verify() the keys the sample DOES exercise
        # turns that silence into a listed finding.
        d = diff("src/x.rs", 1, ['+        0 => "Win32",', '+        1 => "Unix",'])
        v = verify(d, self.pm, "RAR5.OperatingSystem", exercised_keys={"0"})
        self.assertEqual(v.pairs_checked, 1)
        self.assertEqual([(u.key, u.reason) for u in v.unreachable], [("1", "sample-cannot-exercise")])

    def test_a_diff_with_no_enum_pairs_says_so_instead_of_claiming_a_check(self):
        d = diff("src/x.rs", 1, ["+    let n = compute(x);"])
        v = verify(d, self.pm, "RAR5.OperatingSystem")
        self.assertEqual(v.status, "clean")
        self.assertEqual(v.reason, "no-enum-pairs-in-diff")
        self.assertEqual(v.pairs_checked, 0)

    def test_verdict_serialises(self):
        v = verify(self._ttf_diff(), self.pm, "ttLang.Macintosh")
        d = v.to_dict()
        self.assertEqual(d["status"], "fabricated")
        self.assertEqual(d["verifier_version"], verify_enum_maps.VERIFIER_VERSION)
        self.assertTrue(all(isinstance(m["key"], str) for m in d["mismatches"]))


class CliTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.pm = write_pm(
            self.tmp.name,
            "Demo.pm",
            """\
            %Image::ExifTool::Demo::RAR5 = (
                OperatingSystem => { PrintConv => { 0 => 'Win32', 1 => 'Unix' } },
            );
            """,
        )

    def test_main_uses_the_injected_run_git_and_exits_nonzero_on_a_fabrication(self):
        captured = []

        def fake_git(args):
            captured.append(args)
            return diff("src/x.rs", 1, ['+        2 => "MacOS",'])

        out = io.StringIO()
        rc = main(
            ["--sha", "deadbeef", "--pm", str(self.pm), "--table", "RAR5.OperatingSystem"],
            run_git=fake_git,
            stdout=out,
        )
        self.assertEqual(rc, 1)
        self.assertEqual(captured, [["show", "--format=%B", "deadbeef"]])
        self.assertIn("status: fabricated", out.getvalue())
        self.assertIn("MISMATCH key=2", out.getvalue())

    def test_main_exits_zero_on_clean_and_can_emit_json(self):
        out = io.StringIO()
        rc = main(
            ["--sha", "x", "--pm", str(self.pm), "--table", "RAR5.OperatingSystem", "--json"],
            run_git=lambda args: diff("src/x.rs", 1, ['+        0 => "Win32",']),
            stdout=out,
        )
        self.assertEqual(rc, 0)
        self.assertIn('"status": "clean"', out.getvalue())


# --------------------------------------------------------------------------


def _have_real_fixtures():
    if not (PERL_LIB / "Font.pm").is_file():
        return False
    if not (REPO_ROOT / ".git").exists():
        return False
    try:
        subprocess.run(
            ["git", "cat-file", "-e", "5249a506^{commit}"],
            cwd=REPO_ROOT,
            check=True,
            capture_output=True,
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        return False
    return True


HAVE_REAL = _have_real_fixtures()


@unittest.skipUnless(
    HAVE_REAL,
    "needs the oxidex repo with commits 5249a506/a998b8fc/e0900a27 and the ExifTool "
    f"checkout at {PERL_LIB}",
)
class AcceptanceAgainstRealCommits(unittest.TestCase):
    """The three verdicts this component exists to produce. Do not weaken these."""

    @staticmethod
    def show(sha):
        return subprocess.run(
            ["git", "show", "--format=%B", sha],
            cwd=REPO_ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout

    def test_ttf_5249a506_is_fabricated_against_font_pm_ttlang_macintosh(self):
        v = verify(self.show("5249a506"), PERL_LIB / "Font.pm", "ttLang.Macintosh")
        self.assertEqual(v.status, "fabricated")
        by_key = {m.key: m for m in v.mismatches}
        self.assertEqual(
            (by_key["12"].rust_says, by_key["12"].exiftool_says, by_key["12"].exiftool_key_for_value),
            ("es", "ar", "6"),
            "Spanish is Macintosh 6, not 12",
        )
        self.assertEqual(
            (by_key["4"].rust_says, by_key["4"].exiftool_says, by_key["4"].exiftool_key_for_value),
            ("it", "nl-NL", "3"),
            "Italian is Macintosh 3, not 4; 4 is the Dutch record Font.ttf really carries",
        )
        # fi at 13 and fr at 1 are correct and must not be flagged.
        self.assertEqual(set(by_key), {"12", "4"})

    def test_rar_a998b8fc_is_fabricated_against_zip_pm_rar5(self):
        v = verify(self.show("a998b8fc"), PERL_LIB / "ZIP.pm", "RAR5.OperatingSystem")
        self.assertEqual(v.status, "fabricated")
        self.assertEqual({m.key for m in v.mismatches}, {"2", "3", "4"})
        self.assertTrue(all(m.exiftool_says is None for m in v.mismatches))
        self.assertEqual([c.value for c in v.catch_all_arms], ["Unknown"])

    def test_rw2_e0900a27_is_clean_against_exif_pm(self):
        v = verify(self.show("e0900a27"), PERL_LIB / "Exif.pm", None)
        self.assertEqual(v.status, "clean", f"mismatches={v.mismatches} reason={v.reason}")
        self.assertEqual(v.pairs_checked, 5)

    def test_the_real_zip_pm_operating_system_hint_is_ambiguous(self):
        v = verify(self.show("a998b8fc"), PERL_LIB / "ZIP.pm", "OperatingSystem")
        self.assertEqual(v.status, "cannot-verify")
        self.assertEqual(v.reason, "ambiguous-table")
        self.assertEqual(len(v.candidates), 3)

    def test_every_exiftool_module_parses_without_crashing(self):
        # 172 modules, ~2200 package-level hashes. A crash here means the
        # daemon gets an exception instead of a verdict.
        for pm in sorted(PERL_LIB.glob("*.pm")):
            with self.subTest(module=pm.name):
                bodies = verify_enum_maps._find_hash_bodies(
                    pm.read_text(encoding="utf-8", errors="replace")
                )
                for short, (full, body) in list(bodies.items()):
                    if short != full:
                        continue
                    verify_enum_maps._entries(body)

    def test_exif_pm_main_tokenises_fully(self):
        # Regression lock for the q{}-apostrophe bug: %Main must expose
        # hundreds of top-level tags, not the 24 it collapsed to.
        src = (PERL_LIB / "Exif.pm").read_text(encoding="utf-8", errors="replace")
        _, body = verify_enum_maps._find_hash_bodies(src)["Main"]
        self.assertGreater(len(verify_enum_maps._entries(body)), 400)


if __name__ == "__main__":
    unittest.main()
