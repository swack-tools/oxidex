"""Differential export derives expectations from validated resolver results."""
import unittest

from setnewvalue_address_differential import export
from setnewvalue_addressing import compile_addressing
from test_setnewvalue_addressing import observations, source


class SetNewValueAddressDifferentialTests(unittest.TestCase):
    def test_export_has_source_derived_aliases_grammar_and_identity(self):
        document = source()
        addressing, _ = compile_addressing(document)
        fixture = export(document, observations(addressing.rows), dump_sha256="a" * 64,
                         observations_sha256="b" * 64, operands_sha256="c" * 64)
        self.assertEqual(fixture["schema"], "setnewvalue_address_differential_v1")
        self.assertEqual(len(fixture["rows"]), 1)
        ordinary = next(case for case in fixture["cases"] if case["kind"] == "ordinary")
        self.assertEqual(ordinary["identity"], list(addressing.rows[0].identity))
        self.assertTrue(any(case["kind"] == "multi_group_last_colon" for case in fixture["cases"]))
        self.assertTrue(any(case["kind"] == "valueconv_hash" for case in fixture["cases"]))


if __name__ == "__main__":
    unittest.main()
