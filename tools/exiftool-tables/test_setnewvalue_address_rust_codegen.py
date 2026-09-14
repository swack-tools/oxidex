"""Generated static EXIF address operands are complete and inactive."""
import unittest

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
