#!/usr/bin/env python3
"""Nikon PrintAFPointsLeftRight / PrintAFPointsUpDown port selection
(exprs.HELPER_SEMANTICS), the second user of #805's ConvertUnixTime mechanism.

ExifTool 12.64 computes `$center = 1 + ($ncol + 1)/2`; the pinned 13.59
computes `($ncol + 1) / 2`; 11.78 has neither sub. The 12.64 rehearsal's
verify_exprs.py run failed exactly the two expressions reaching these subs
(518/520) because every translation compiled to the 13.59 arithmetic. These
tests pin that the port is chosen from the pinned tree's SOURCE -- the
verbatim sub text each release carries, captured with its outputs in
testdata/nikon_af_points_relative_outputs.json -- that an unrecognised or
absent body refuses rather than guesses, and that codegen compiles with the
ports the oracle ledger was proven with. The outputs themselves are pinned
against the Rust ports by exprs.rs's
`print_af_points_relative_ports_match_each_release`.
"""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

import codegen
import exprs

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
CAPTURE = json.loads((HERE / "testdata" / "nikon_af_points_relative_outputs.json")
                     .read_text(encoding="utf-8"))
SUBS = ("PrintAFPointsLeftRight", "PrintAFPointsUpDown")
LR29 = "Image::ExifTool::Nikon::PrintAFPointsLeftRight($val, 29)"
UD17 = "Image::ExifTool::Nikon::PrintAFPointsUpDown($val, 17)"
LR29_DEPARSE = ("{ package Image::ExifTool::Nikon; use strict; (my($val) = @_); "
                "PrintAFPointsLeftRight($val, 29); }")


def source(release, sub):
    return CAPTURE["releases"][release]["subs"][sub]["sub_source"]


def write_nikon(root, *sub_texts):
    pm = Path(root) / "Image" / "ExifTool" / "Nikon.pm"
    pm.parent.mkdir(parents=True)
    pm.write_text("package Image::ExifTool::Nikon;\n# preamble\n"
                  + "\n#-----\n".join(sub_texts)
                  + "\n#-----\nsub PrintPC($;$$$)\n{\n    1;\n}\n1;\n", encoding="latin-1")
    return root


def reset():
    exprs.set_helper_semantics(exprs.default_helper_semantics())


class CapturedSources(unittest.TestCase):
    def test_capture_is_what_the_task_describes(self):
        for sub in SUBS:
            self.assertIsNone(source("11.78", sub))
            self.assertIn("my $center = 1 + (", source("12.64", sub))
            self.assertIn(" + 1)/2;", source("12.64", sub))
            self.assertIn(" + 1) / 2;", source("13.59", sub))
            self.assertNotIn("1 + (", source("13.59", sub))
        # A real input the two releases print differently.
        lr = {r: CAPTURE["releases"][r]["subs"]["PrintAFPointsLeftRight"]["outputs"]["29"]
              for r in ("12.64", "13.59")}
        self.assertEqual((lr["12.64"]["16"], lr["13.59"]["16"]), ("C", "1R of Center"))
        self.assertEqual((lr["12.64"]["15"], lr["13.59"]["15"]), ("1L of Center", "C"))
        self.assertEqual(lr["12.64"]["0"], lr["13.59"]["0"])


class Selection(unittest.TestCase):
    def setUp(self):
        self.addCleanup(reset)
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name)

    def detect(self, name, *sub_texts):
        root = write_nikon(self.tmp / name, *sub_texts)
        return {sub: exprs.detect_helper_semantics(root, sub)[0] for sub in SUBS}

    def test_each_release_body_selects_its_own_port(self):
        self.assertEqual(self.detect("r1264", source("12.64", SUBS[0]), source("12.64", SUBS[1])),
                         dict.fromkeys(SUBS, "center_one_plus_half"))
        self.assertEqual(self.detect("r1359", source("13.59", SUBS[0]), source("13.59", SUBS[1])),
                         dict.fromkeys(SUBS, "center_half"))
        # 11.78: neither sub exists -- nothing is proven, so nothing is selected.
        self.assertEqual(self.detect("r1178"), dict.fromkeys(SUBS, None))
        # Each sub is judged on its own body.
        self.assertEqual(self.detect("mixed", source("12.64", SUBS[0]), source("13.59", SUBS[1])),
                         {SUBS[0]: "center_one_plus_half", SUBS[1]: "center_half"})

    def test_the_version_label_plays_no_part(self):
        # A tree claiming to be 13.59 but carrying 12.64's bodies selects
        # 12.64's port, and the reverse: only the source decides.
        for label, release, port in (("13.59", "12.64", "center_one_plus_half"),
                                     ("12.64", "13.59", "center_half"),
                                     ("11.78", "13.59", "center_half")):
            root = write_nikon(self.tmp / f"as{label}-{release}",
                               source(release, SUBS[0]), source(release, SUBS[1]))
            (Path(root) / "Image" / "ExifTool.pm").write_text(
                f"package Image::ExifTool;\n$VERSION = '{label}';\n1;\n", encoding="latin-1")
            self.assertEqual({s: exprs.detect_helper_semantics(root, s)[0] for s in SUBS},
                             dict.fromkeys(SUBS, port), (label, release))

    def test_comment_and_indentation_edits_are_not_semantic(self):
        text = source("12.64", SUBS[0]).replace("    ", "\t").replace(
            "#out of focus", "# column zero: out of focus")
        self.assertEqual(self.detect("reflowed", text)[SUBS[0]], "center_one_plus_half")

    def test_any_code_change_selects_nothing(self):
        for i, edit in enumerate((("1 + (", "2 + ("), (")/2;", ")/3;"), ("'C'", "'c'"))):
            text = source("12.64", SUBS[0]).replace(*edit)
            self.assertIsNone(self.detect(f"edit{i}", text)[SUBS[0]], edit)
        self.assertEqual(exprs.detect_helper_semantics(self.tmp / "no-tree", SUBS[0]),
                         (None, None))

    def test_translations_follow_the_selected_port(self):
        for port, lr_fn, ud_fn in (
                ("center_one_plus_half", "print_af_points_left_right_one_plus_center",
                 "print_af_points_up_down_one_plus_center"),
                ("center_half", "print_af_points_left_right", "print_af_points_up_down")):
            exprs.set_helper_semantics(dict.fromkeys(SUBS, port))
            self.assertEqual(exprs.translate(LR29)[1],
                             f"crate::exiftool_tables::exprs::{lr_fn}({{v}}, 29.0)")
            self.assertEqual(exprs.translate(UD17)[1],
                             f"crate::exiftool_tables::exprs::{ud_fn}({{v}}, 17.0)")
            self.assertEqual(exprs.code_ref_expr(LR29_DEPARSE), LR29)

    def test_unproven_source_refuses_only_that_helper(self):
        exprs.set_helper_semantics({SUBS[0]: None})
        for n in (19, 21, 29):
            self.assertIsNone(exprs.translate_or_compile_any(
                f"Image::ExifTool::Nikon::PrintAFPointsLeftRight($val, {n})"))
        # The code ref reaching it is now dropped (counted), never compiled.
        self.assertIsNone(exprs.code_ref_expr(LR29_DEPARSE))
        self.assertIsNotNone(exprs.translate(UD17))
        self.assertIsNotNone(exprs.translate("Image::ExifTool::ConvertFileSize($val)"))

    def test_selecting_one_helper_leaves_the_others(self):
        exprs.set_helper_semantics({"ConvertUnixTime": "trunc_epsilon"})
        exprs.set_helper_semantics({SUBS[0]: "center_one_plus_half"})
        self.assertEqual(exprs.helper_semantics(), {
            "ConvertUnixTime": "trunc_epsilon",
            SUBS[0]: "center_one_plus_half", SUBS[1]: "center_half"})
        with self.assertRaises(ValueError):
            exprs.set_helper_semantics({SUBS[0]: "trunc_epsilon"})
        with self.assertRaises(ValueError):
            exprs.set_helper_semantics({"PrintAFPoints": "center_half"})

    def test_every_port_exists_in_the_rust_helpers(self):
        rust = (REPO / "src" / "exiftool_tables" / "exprs.rs").read_text(encoding="utf-8")
        for sub, arg in ((SUBS[0], "col: f64, ncol: f64"), (SUBS[1], "row: f64, nrow: f64")):
            for fn, _body in exprs.HELPER_SEMANTICS[sub]["ports"].values():
                self.assertIn(f"pub fn {fn}({arg}) -> String", rust)

    @unittest.skipUnless(Path("/tmp/oxidex-exiftool-cache/exiftool/lib").is_dir(),
                         "pinned 13.59 tree not cached on this host")
    def test_pinned_tree_is_the_default_port(self):
        for sub in SUBS:
            name, _ = exprs.detect_helper_semantics("/tmp/oxidex-exiftool-cache/exiftool/lib", sub)
            self.assertEqual(name, exprs.HELPER_SEMANTICS[sub]["default"])


class LedgerCarriesThePorts(unittest.TestCase):
    def setUp(self):
        self.addCleanup(reset)
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tables = Path(tmp.name) / "tables.json"
        self.tables.write_text(json.dumps({"exiftool_version": "12.64", "modules": {}}))
        self.ledger = Path(tmp.name) / "ledger.json"

    def load(self, **extra):
        ledger = {
            "schema": codegen.LEDGER_SCHEMA, "exiftool_version": "12.64",
            "perl_version": "v5.38.2",
            "tables_sha256": hashlib.sha256(self.tables.read_bytes()).hexdigest(),
            "probe_counts": {"pass": 1, "fail": 0, "skip": 0},
            "verified_expressions": [LR29, UD17], **extra,
        }
        self.ledger.write_text(json.dumps(ledger))
        return codegen.load_oracle_ledger(str(self.ledger), str(self.tables), "12.64")

    def test_recorded_ports_are_applied(self):
        self.load(helper_semantics={"ConvertUnixTime": "trunc_epsilon",
                                    SUBS[0]: "center_one_plus_half",
                                    SUBS[1]: "center_one_plus_half"})
        self.assertIn("print_af_points_left_right_one_plus_center(", exprs.translate(LR29)[1])
        self.assertIn("print_af_points_up_down_one_plus_center(", exprs.translate(UD17)[1])
        self.assertEqual(exprs.helper_semantics()["ConvertUnixTime"], "trunc_epsilon")

    def test_absent_entries_mean_the_default_port(self):
        exprs.set_helper_semantics(dict.fromkeys(SUBS, "center_one_plus_half"))
        self.load(helper_semantics={"ConvertUnixTime": "trunc_epsilon"})
        self.assertEqual(exprs.helper_semantics(), {
            "ConvertUnixTime": "trunc_epsilon",
            SUBS[0]: "center_half", SUBS[1]: "center_half"})

    def test_recorded_refusal_is_applied(self):
        self.load(helper_semantics={SUBS[0]: None, SUBS[1]: None})
        self.assertIsNone(exprs.translate(LR29))
        self.assertIsNone(exprs.translate(UD17))

    def test_unknown_port_is_refused_loudly(self):
        for bad in ({SUBS[0]: "trunc_epsilon"}, {SUBS[1]: "center"}, {"PrintAFPoints": None}):
            with self.assertRaises(SystemExit) as ctx:
                self.load(helper_semantics=bad)
            self.assertIn("helper_semantics", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
