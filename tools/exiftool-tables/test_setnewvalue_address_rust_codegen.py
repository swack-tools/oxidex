"""Generated static EXIF address operands are complete and inactive."""
from copy import deepcopy
from pathlib import Path
import subprocess
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

    def test_removed_ledger_name_survives_unsupported_next_source(self):
        from setnewvalue_ownership_ledger import build_ledger
        first = build_ledger(source(), None, bootstrap=True)
        removed = source()
        del removed["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        refresh_find_tag_info_warmup(removed)
        prior = build_ledger(removed, first, bootstrap=False)
        unsupported = deepcopy(removed)
        unsupported["native_write_helpers"]["set_new_value"]["__deparse"] = "sub { return 0; }"
        rust, report = generate(unsupported, {}, prior_ledger=prior)
        self.assertFalse(report["emitted"])
        self.assertEqual(report["removed_owned_names"], 1)
        self.assertIn('"noallowlist"', rust)
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


if __name__ == "__main__":
    unittest.main()
