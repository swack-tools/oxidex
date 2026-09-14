"""Generated static EXIF address operands are complete and inactive."""
from copy import deepcopy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from checkexif_recipes import RecipeRefused

from setnewvalue_address_rust_codegen import generate
from test_setnewvalue_addressing import observations, refresh_find_tag_info_warmup, source


class SetNewValueAddressRustCodegenTests(unittest.TestCase):
    def test_emits_source_rows_native_lookup_and_owned_names(self):
        document = source()
        # Compile once to obtain the source row for the native-observation
        # fixture without introducing a tag-name or raw-id fixture list.
        from setnewvalue_addressing import compile_addressing
        addressing, _ = compile_addressing(document)
        rust, report = generate(document, observations(addressing.rows), bootstrap_ownership_ledger=True)
        self.assertTrue(report["emitted"])
        self.assertIn("SET_NEW_VALUE_ADDRESS_ROWS", rust)
        self.assertIn("SET_NEW_VALUE_LOOKUP", rust)
        self.assertIn("StaticNativeLookupFamily", rust)
        self.assertIn("source_identity_present", rust)
        self.assertIn("SET_NEW_VALUE_ADMITTED_QUALIFIER_SCOPE", rust)
        self.assertIn("SET_NEW_VALUE_OWNED_NAMES", rust)
        self.assertIn("SET_NEW_VALUE_OWNED_QUALIFIED", rust)
        self.assertIn("StaticSetNewValueAddressCapture", rust)
        self.assertIn("SET_NEW_VALUE_ADDRESS_CAPTURE", rust)
        self.assertEqual(report["source_capture_identity"], {
            "exiftool_version": "13.59", "main_source_sha256": "c" * 64,
            "write_exif_source_sha256": "b" * 64, "writer_source_sha256": "a" * 64,
            "exif_source_sha256": "d" * 64,
        })
        self.assertIn("noallowlist", rust)

    def test_missing_observation_is_explicit_omission(self):
        document = source()
        from setnewvalue_addressing import compile_addressing
        addressing, _ = compile_addressing(document)
        native = observations(addressing.rows)
        native["queries"] = {}
        rust, report = generate(document, native, bootstrap_ownership_ledger=True)
        self.assertFalse(report["emitted"])
        self.assertIn("query set", report["reason"])
        self.assertIn("SET_NEW_VALUE_OWNED_NAMES", rust)
        self.assertIn("noallowlist", rust)
        self.assertIn("= None", rust)

    def test_source_template_failure_still_emits_current_owned_names(self):
        document = source()
        document["native_write_helpers"]["set_new_value"]["__deparse"] = "sub { return 0; }"
        rust, report = generate(document, {})
        self.assertFalse(report["emitted"])
        self.assertIn("noallowlist", rust)
        self.assertIn("SET_NEW_VALUE_ADDRESSING: Option", rust)
        for constant in ("SET_NEW_VALUE_ADDRESS_ROWS", "SET_NEW_VALUE_LOOKUP",
                         "SET_NEW_VALUE_ADMITTED_QUALIFIER_SCOPE",
                         "SET_NEW_VALUE_OWNED_QUALIFIED"):
            self.assertIn(constant, rust)

    def test_prior_ledger_keeps_renamed_name_in_generated_terminal_union(self):
        from setnewvalue_addressing import compile_addressing
        from setnewvalue_ownership_ledger import build_ledger
        first = source()
        prior = build_ledger(first, None, bootstrap=True)
        changed = source()
        row = changed["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        row["properties"]["Name"]["value"] = "NewName"
        row["effective_properties"]["Name"]["value"] = "NewName"
        refresh_find_tag_info_warmup(changed)
        addressing, _ = compile_addressing(changed)
        rust, report = generate(changed, observations(addressing.rows, addressing=addressing),
                                prior_ledger=prior)
        self.assertTrue(report["emitted"])
        self.assertEqual(report["removed_owned_names"], 1)
        self.assertIn('"noallowlist"', rust)
        self.assertIn('"newname"', rust)
        self.assertIn("removed: true", rust)

    def test_added_and_removed_names_survive_later_source_join_refusal(self):
        from setnewvalue_addressing import compile_addressing
        from setnewvalue_ownership_ledger import build_ledger
        first = build_ledger(source(), None, bootstrap=True)
        changed = source()
        row = changed["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        row["properties"]["Name"]["value"] = "NewName"
        row["effective_properties"]["Name"]["value"] = "NewName"
        refresh_find_tag_info_warmup(changed)
        addressing, _ = compile_addressing(changed)
        # This is checked after the completed ownership ledger has merged the
        # old/removed and new/current names, so it models a later source-join
        # refusal without treating an unknown current source as authenticated.
        changed["native_write_capture_context"]["loaded_modules"]["Image/ExifTool/Exif.pm"] = "e" * 64
        rust, report = generate(changed, observations(addressing.rows, addressing=addressing),
                                prior_ledger=first)
        self.assertFalse(report["emitted"])
        self.assertEqual(report["removed_owned_names"], 1)
        self.assertIn('"noallowlist"', rust)
        self.assertIn('"newname"', rust)
        self.assertIn("removed: true", rust)
        for constant in ("SET_NEW_VALUE_ADDRESS_ROWS", "SET_NEW_VALUE_LOOKUP",
                         "SET_NEW_VALUE_ADMITTED_QUALIFIER_SCOPE",
                         "SET_NEW_VALUE_OWNED_QUALIFIED"):
            self.assertIn(constant, rust)
        with tempfile.TemporaryDirectory() as directory:
            source_file = Path(directory) / "omitted.rs"
            source_file.write_text(rust, encoding="utf-8")
            subprocess.run(["rustc", "--crate-type", "lib", str(source_file),
                            "-o", str(Path(directory) / "omitted.rlib")], check=True,
                           capture_output=True, text=True)

    def test_unauthenticated_current_ownership_refuses_with_valid_prior(self):
        from setnewvalue_ownership_ledger import build_ledger
        prior = build_ledger(source(), None, bootstrap=True)
        unsupported = source()
        unsupported["native_write_helpers"]["set_new_value"]["__deparse"] = "sub { return 0; }"
        with self.assertRaisesRegex(RecipeRefused, "current ownership could not authenticate"):
            generate(unsupported, {}, prior_ledger=prior)

    def test_cli_build_failure_leaves_existing_artifacts_unchanged(self):
        from setnewvalue_ownership_ledger import build_ledger
        prior = build_ledger(source(), None, bootstrap=True)
        unsupported = source()
        unsupported["native_write_helpers"]["set_new_value"]["__deparse"] = "sub { return 0; }"
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            tables = directory / "tables.json"
            observations_path = directory / "observations.json"
            prior_path = directory / "prior.json"
            output = directory / "generated.rs"
            report = directory / "report.json"
            ledger = directory / "ledger.json"
            tables.write_text(json.dumps(unsupported), encoding="utf-8")
            observations_path.write_text(json.dumps({}), encoding="utf-8")
            prior_path.write_text(json.dumps(prior), encoding="utf-8")
            before = {output: b"old rust", report: b"old report", ledger: b"old ledger"}
            for path, contents in before.items():
                path.write_bytes(contents)
            result = subprocess.run([
                sys.executable, str(Path(__file__).with_name("setnewvalue_address_rust_codegen.py")),
                str(tables), str(observations_path), "--ownership-ledger", str(prior_path),
                "--write-ownership-ledger", str(ledger), "--output", str(output),
                "--report", str(report),
            ], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("SetNewValue", result.stderr)
            self.assertEqual({path: path.read_bytes() for path in before}, before)

    def test_tampered_prior_ledger_refuses_before_omission_render(self):
        from setnewvalue_ownership_ledger import build_ledger
        prior = build_ledger(source(), None, bootstrap=True)
        tampered = deepcopy(prior)
        tampered["entries"][0]["name"] = "forged"
        with self.assertRaisesRegex(RecipeRefused, "ledger digest"):
            generate(source(), {}, prior_ledger=tampered)

    def test_mixed_final_source_closure_is_an_explicit_omission(self):
        document = source()
        from setnewvalue_addressing import compile_addressing
        addressing, _ = compile_addressing(document)
        document["native_write_capture_context"]["loaded_modules"]["Image/ExifTool/Exif.pm"] = "e" * 64
        rust, report = generate(document, observations(addressing.rows), bootstrap_ownership_ledger=True)
        self.assertFalse(report["emitted"])
        self.assertIn("closures disagree", report["reason"])
        self.assertIn("SET_NEW_VALUE_ADDRESS_CAPTURE: Option<StaticSetNewValueAddressCapture> = None", rust)

    def test_lookup_emits_all_native_families_and_identity_presence(self):
        document = source()
        from setnewvalue_addressing import compile_addressing
        addressing, _ = compile_addressing(document)
        rust, report = generate(document, observations(addressing.rows, external=True),
                                bootstrap_ownership_ledger=True)
        self.assertTrue(report["emitted"])
        self.assertIn("family: 2", rust)
        self.assertIn("source_identity_present: true", rust)
        self.assertIn("source_identity_present: false", rust)
        # The runtime must distinguish a same-named candidate in a foreign
        # native table from an omitted physical field in the selected table.
        # Keep every source-observed identity/control operand in the generated
        # contract; groups or a numeric id alone cannot make that distinction.
        for field in ("module", "table", "full_name", "raw_id", "writable",
                      "permanent", "write_group"):
            self.assertIn(f"pub {field}:", rust)
            self.assertIn(f"{field}:", rust)


if __name__ == "__main__":
    unittest.main()
