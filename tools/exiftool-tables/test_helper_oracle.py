#!/usr/bin/env python3
"""The v2 helper library's oracle capture (helper_oracle.py) is what the Rust
differential test replays, so the capture itself is pinned here:

- it was produced from THIS probe set and THIS registry (a probe or status
  edited without re-capturing fails, instead of the Rust test silently
  checking a stale set);
- every recorded sub_source hashes to the recorded digest, and a comment or
  whitespace edit keeps the digest while any code edit changes it (the exact
  source rule #805/#818 select ports by);
- against the pinned tree, when one is on this host, each sub still folds to
  the captured digest (skipped, loudly, without one -- CI's pinned-tree jobs
  run `helper_oracle.py --check`, which re-runs the Perl too).
"""

import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path

import codegen_charsets
import helper_oracle as H

CAPTURE = json.loads(H.CAPTURE.read_text(encoding="utf-8"))
PINNED = Path(os.environ.get("OXIDEX_PINNED_EXIFTOOL", "/tmp/oxidex-exiftool-cache/exiftool"))


class Capture(unittest.TestCase):
    def test_registry_and_capture_agree(self):
        self.assertEqual([h["perl"] for h in H.HELPERS if h["perl"] in CAPTURE["helpers"]],
                         [h["perl"] for h in H.HELPERS])
        self.assertEqual(set(CAPTURE["helpers"]), {h["perl"] for h in H.HELPERS})
        for h in H.HELPERS:
            c = CAPTURE["helpers"][h["perl"]]
            self.assertEqual((c["status"], c["module"], c["spike_uses"], c["spike_rank"]),
                             (h["status"], h["module"], h["uses"], h["rank"]), h["perl"])
            if h["status"] == H.REFUSED:
                self.assertEqual(c["cases"], [], h["perl"])
            else:
                self.assertTrue(c["cases"], h["perl"])

    def test_capture_was_taken_from_this_probe_set(self):
        by_helper = {}
        for case in H.cases():
            by_helper.setdefault(case["helper"], []).append(
                (case["args"],) + tuple(case.get(k) for k in H.CASE_KEYS))
        for name, h in CAPTURE["helpers"].items():
            got = [(c["args"],) + tuple(c.get(k) for k in H.CASE_KEYS) for c in h["cases"]]
            self.assertEqual(got, by_helper.get(name, []), name)
        self.assertEqual([t["value"] for t in CAPTURE["truthiness"]],
                         [c["truthy"] for c in H.truthiness_cases()])

    def test_every_case_has_a_perl_answer(self):
        for name, h in CAPTURE["helpers"].items():
            for c in h["cases"]:
                self.assertTrue(("out" in c) ^ ("die" in c), (name, c["args"]))

    def test_side_effects_are_recorded_for_the_session_mutating_ports_only(self):
        for name, h in CAPTURE["helpers"].items():
            mutating = name in ("Image::ExifTool::Decode", "Image::ExifTool::Encode",
                                "Image::ExifTool::Exif::ConvertExifText",
                                "Image::ExifTool::Exif::DecodeCFAPattern")
            for c in h["cases"]:
                self.assertEqual("set_members" in c and "warnings" in c, mutating,
                                 (name, c["args"]))

    def test_probe_charsets_are_the_generated_cs_type(self):
        cs, tables, _ = codegen_charsets.generated_tables()
        self.assertEqual(sorted(H.CHARSETS), sorted(cs))
        self.assertFalse(set(H.NOT_CHARSETS) & set(cs))
        self.assertEqual(sorted(H.FIXED_MULTI), sorted(n for n, t in cs.items() if t & 0x600))
        self.assertEqual(set(tables), {n for n, t in cs.items() if t & 0x001})

    def test_decode_dependencies_are_recorded(self):
        for name in [h["perl"] for h in H.HELPERS if h.get("deps")]:
            deps = CAPTURE["helpers"][name]["dependencies"]
            want = [h["deps"] for h in H.HELPERS if h["perl"] == name][0]
            self.assertEqual(sorted(deps), sorted(want))
            self.assertTrue(all(len(d) == 64 for d in deps.values()), name)

    def test_recorded_digests_are_of_the_recorded_sources(self):
        for name, h in CAPTURE["helpers"].items():
            self.assertEqual(hashlib.sha256(h["sub_source"].encode("latin-1")).hexdigest(),
                             h["source_sha256"], name)

    def test_the_capture_names_the_pinned_instrument(self):
        cap = CAPTURE["capture"]
        self.assertEqual(cap["perl_version"], H.PINNED_PERL_VERSION)
        self.assertEqual(cap["exiftool_version"],
                         (H.REPO / ".exiftool-version").read_text().strip())
        self.assertEqual(cap["tz"], "UTC")


class ExactSource(unittest.TestCase):
    def digest(self, text):
        with tempfile.TemporaryDirectory() as tmp:
            pm = Path(tmp) / "Image" / "ExifTool.pm"
            pm.parent.mkdir(parents=True)
            pm.write_text("package Image::ExifTool;\n" + text + "1;\n", encoding="latin-1")
            src = H.helper_source(pm.read_text(encoding="latin-1"), "IsInt")
            return None if src is None else hashlib.sha256(src.encode("latin-1")).hexdigest()

    def test_fold_ignores_comments_and_whitespace_only(self):
        # A multi-line body: helper_source defers to exprs.sub_source.
        base = "sub IsInt($)\n{\n    return scalar($_[0] =~ /^[+-]?\\d+$/);\n}\n"
        same = "sub IsInt($)\n{\n\t# comment\n  return  scalar($_[0] =~ /^[+-]?\\d+$/);\n}\n"
        other = "sub IsInt($)\n{\n    return scalar($_[0] =~ /^[+-]?\\d*$/);\n}\n"
        self.assertIsNotNone(self.digest(base))
        self.assertEqual(self.digest(base), self.digest(same))
        self.assertNotEqual(self.digest(base), self.digest(other))
        self.assertIsNone(self.digest("sub IsFloat($)\n{\n}\n"))

    def test_declarations_are_skipped_and_one_line_subs_stand_alone(self):
        one = "sub IsInt($)      { return scalar($_[0] =~ /^[+-]?\\d+$/); }\n"
        text = ("sub IsInt($);\n" + one
                + "sub IsHex($) { return 1; }\nsub Round($$)\n{\n    1;\n}\n")
        with tempfile.TemporaryDirectory() as tmp:
            pm = Path(tmp) / "x.pm"
            pm.write_text(text, encoding="latin-1")
            src = H.helper_source(pm.read_text(encoding="latin-1"), "IsInt")
        self.assertEqual(src, "sub IsInt($) { return scalar($_[0] =~ /^[+-]?\\d+$/); }")


@unittest.skipUnless((PINNED / "lib" / "Image" / "ExifTool.pm").is_file(),
                     f"pinned ExifTool tree not at {PINNED}")
class PinnedTree(unittest.TestCase):
    def test_pinned_subs_fold_to_the_captured_digests(self):
        sources = H.pinned_sources(PINNED / "lib")
        for name, h in CAPTURE["helpers"].items():
            self.assertEqual(sources[name][1], h["source_sha256"], name)

    def test_pinned_dependencies_and_charset_modules_match_the_capture(self):
        deps = H.dependency_sources(PINNED / "lib")
        for name, h in CAPTURE["helpers"].items():
            for dep, digest in h.get("dependencies", {}).items():
                self.assertEqual(deps[dep], digest, (name, dep))
        self.assertEqual(codegen_charsets.charset_sources(PINNED / "lib"),
                         CAPTURE["charset_sources"])


class GeneratedTables(unittest.TestCase):
    """The committed charset_tables.rs names the sources the capture names."""

    def test_generated_file_records_the_captured_sources(self):
        text = codegen_charsets.OUT.read_text(encoding="utf-8")
        for path, digest in list(CAPTURE["charset_sources"].items()) + list(
                CAPTURE["perl_sources"].items()):
            self.assertIn(f'("{path}", "{digest}"),', text)

    def test_generator_refuses_what_the_rust_port_relies_on_not_happening(self):
        ok = {"csType": {"Latin": 0x101, "MacThai": 0x803},
              "unicode2byte": {"Latin": {"8364": 128}},
              "tables": {"Latin": {"128": {"u": 8364}},
                         "MacThai": {"128": {"u": 65}, "129": {"u": 65}}}}
        codegen_charsets.validate(ok)   # a source-only table may repeat values
        dup = json.loads(json.dumps(ok))
        dup["tables"]["Latin"]["129"] = {"u": 8364}
        with self.assertRaises(SystemExit):
            codegen_charsets.validate(dup)
        pre = json.loads(json.dumps(ok))
        pre["unicode2byte"]["Latin"] = {"8364": 129}
        with self.assertRaises(SystemExit):
            codegen_charsets.validate(pre)


if __name__ == "__main__":
    unittest.main()
