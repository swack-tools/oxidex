"""Generated static EXIF address operands are complete and inactive."""
import unittest

from setnewvalue_address_rust_codegen import generate
from test_setnewvalue_addressing import observations, source


class SetNewValueAddressRustCodegenTests(unittest.TestCase):
    def test_emits_source_rows_native_lookup_and_owned_names(self):
        document = source()
        # Compile once to obtain the source row for the native-observation
        # fixture without introducing a tag-name or raw-id fixture list.
        from setnewvalue_addressing import compile_addressing
        addressing, _ = compile_addressing(document)
        rust, report = generate(document, observations(addressing.rows))
        self.assertTrue(report["emitted"])
        self.assertIn("SET_NEW_VALUE_ADDRESS_ROWS", rust)
        self.assertIn("SET_NEW_VALUE_LOOKUP", rust)
        self.assertIn("SET_NEW_VALUE_OWNED_NAMES", rust)
        self.assertIn("noallowlist", rust)

    def test_missing_observation_is_explicit_omission(self):
        document = source()
        from setnewvalue_addressing import compile_addressing
        addressing, _ = compile_addressing(document)
        native = observations(addressing.rows)
        native["queries"] = {}
        rust, report = generate(document, native)
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


if __name__ == "__main__":
    unittest.main()
