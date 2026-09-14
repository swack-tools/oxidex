"""Source/capability join controls; native runtime proof lives in its own test."""
from copy import deepcopy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import sanitize_rust_codegen as generator
from checkexif_recipes import RecipeMalformed
from test_sanitize_recipes import source_fact
from test_utf8_primitive_contract import capture


def document(contract):
    # Synthetic source grammar fixture joined to the real interpreter's
    # primitive observation. This exercises the generator, not ExifTool parity.
    helper = source_fact()
    for key, body in (("encode", "($$;$) ;"), ("is_utf8", "($;$) ;")):
        observed = contract["final"]["bindings"][key]
        # Match the actual deparser prototype spelling rather than pretending
        # this unit fixture supplies the XS implementation source.
        prototype = observed["prototype"]
        body = ("(" + prototype + ") ;") if prototype is not None else "{\n    package Encode;\n    ();\n}"
        if hashlib.sha256(body.encode()).hexdigest() != observed["body_sha256"]:
            raise AssertionError("unmodeled native primitive prototype in unit fixture")
        helper["dependencies"]["Encode::" + key].update(
            __name=observed["actual_name"], __deparse=body)
    return {"exiftool_version": "synthetic-source", "native_write_helpers": {"sanitize": helper},
            "native_runtime_contracts": {"utf8": contract}}


class SanitizeCodegen(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ready = document(capture())

    def test_emits_only_after_source_and_primitive_join(self):
        source, report = generator.generate(self.ready)
        self.assertEqual(report["omissions"], [])
        self.assertIn("Some(SanitizeRecipe", source)
        self.assertIn("downgrade_at_or_after: 5006000", source)
        self.assertIn("EncodingPrimitive::Utf8", source)
        self.assertNotIn("captured", source)

    def test_source_guard_change_changes_generated_operand(self):
        changed = deepcopy(self.ready)
        changed["native_write_helpers"]["sanitize"]["__deparse"] = changed["native_write_helpers"]["sanitize"]["__deparse"].replace("5.006", "10.006")
        source, report = generator.generate(changed)
        self.assertEqual(report["omissions"], [])
        self.assertIn("downgrade_at_or_after: 10006000", source)

    def test_missing_contract_or_changed_callback_fingerprint_refuses(self):
        for mutation in ("missing", "capture_flag", "binding", "body", "version"):
            changed = deepcopy(self.ready)
            helper = changed["native_write_helpers"]["sanitize"]
            if mutation in {"missing", "capture_flag"}:
                changed.pop("native_runtime_contracts")
                helper["runtime_dependencies"] = {"Encode": {"captured": True}}
            elif mutation == "binding":
                helper["dependencies"]["Encode::encode"]["__name"] = "Other::encode"
            elif mutation == "body":
                helper["dependencies"]["Encode::encode"]["__deparse"] = None
            else:
                helper["__deparse"] = helper["__deparse"].replace("5.006", str(2**64) + ".006")
            source, report = generator.generate(changed)
            with self.subTest(mutation=mutation):
                self.assertIn("= None", source)
                self.assertTrue(report["omissions"])

    def test_malformed_capture_is_not_reported_as_an_ordinary_gap(self):
        with self.assertRaises(RecipeMalformed):
            generator.generate({"native_write_helpers": None})


@unittest.skipUnless(os.environ.get("OXIDEX_TABLES_JSON"), "requires fresh official native capture")
class NativeSanitizeFreshness(unittest.TestCase):
    def test_committed_rules_and_ledger_match_fresh_generation(self):
        root = Path(__file__).resolve().parents[2]
        source, report = generator.generate(json.loads(Path(os.environ["OXIDEX_TABLES_JSON"]).read_text()))
        with tempfile.TemporaryDirectory(prefix="oxidex-sanitize-freshness-") as temporary:
            output = Path(temporary) / "rules.rs"
            output.write_text(source)
            subprocess.run(["rustfmt", "--edition", "2024", "--config-path", str(root / "rustfmt.toml"), str(output)],
                           check=True, capture_output=True, timeout=30)
            self.assertEqual(output.read_text(), (root / "src/writers/generated_sanitize_rules.rs").read_text())
        self.assertEqual(json.dumps(report, sort_keys=True, indent=2) + "\n",
                         (root / "tools/exiftool-tables/sanitize_ledger.json").read_text())


if __name__ == "__main__":
    unittest.main()
