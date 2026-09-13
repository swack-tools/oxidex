"""Source mutation and refusal boundaries of the common unsigned reader."""
import copy
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

import directory_validation
import native_reader_contract as contract
import native_reader_facts
from test_directory_validation import helper, NAME, CALL
import test_directory_validation as validation_fixture
import verify


def snapshot():
    facts = {key: {"__perl": "CODE", "__name": name, "resolved": True,
                   "__deparse": body, "source_file": "Image/ExifTool.pm",
                   "source_sha256": "2" * 64}
             for key, name, body in contract._FUNCTIONS}
    return {"kind": "binary_unsigned_reader_contract_v1", "resolved": True,
            "isolated_functions": facts,
            "loaded_functions": copy.deepcopy(facts),
            "builtin_overrides": {"unpack": False, "pack": False},
            "isolated_builtin_overrides": {"unpack": False, "pack": False},
            "loaded_state": {"reported_byte_order": "MM", "unpack_std_s": "n"},
            "observations": {"initial_byte_order": "MM", "restore_return": 1,
                             "restored_byte_order": "MM", "restore_error": None,
                             "orders": {"II": {"set_return": 1, "reported_byte_order": "II", "unpack_std_s": "v", "error": None},
                                        "MM": {"set_return": 1, "reported_byte_order": "MM", "unpack_std_s": "n", "error": None}}},
            "get16u_probe": {"kind": "get16u_native_probe_v1", "valid_case_count": 262144,
                             "boundary_case_count": 10, "failure_count": 0, "failure_details": [],
                             "restore_return": 1, "restored_byte_order": "MM", "restore_error": None,
                             "boundary_cases": boundary_cases()}}


def boundary_cases():
    return [
        {"byte_order": order, "name": name, "offset": offset, "bytes_hex": bytes_hex,
         "expected_outcome": outcome, "expected": None, "observed": None,
         "error": "native beyond-end failure" if outcome == "error" else None,
         "matches_expected_native_outcome": True}
        for order in ("II", "MM")
        for name, offset, bytes_hex, outcome in (
            ("empty_at_zero", 0, "", "undef"),
            ("one_byte_at_zero", 0, "34", "undef"),
            ("one_byte_remaining", 2, "aabb34", "undef"),
            ("offset_at_end", 2, "aabb", "undef"),
            ("offset_beyond_end", 3, "aabb", "error"),
        )
    ]


def bound_helper(state):
    record = helper()
    record["dependencies"] = {"Image::ExifTool::Get16u": copy.deepcopy(state["loaded_functions"]["get16u"])}
    record["dependencies"]["Image::ExifTool::Get16u"]["dependencies"] = {
        "Image::ExifTool::DoUnpackStd": copy.deepcopy(state["loaded_functions"]["do_unpack_std"])}
    return record


class ReaderContract(unittest.TestCase):
    def test_full_library_bodies_and_observed_state_are_required(self):
        state = snapshot()
        self.assertEqual(len(contract.recognize(state)), 4)
        compiled = directory_validation.compile_validation(CALL, {NAME: bound_helper(state)}, {"unsigned16": state})
        self.assertEqual(compiled.reader_contract_sha256, native_reader_facts.fingerprint(state))
        for key in ("isolated_functions", "observations", "loaded_functions", "builtin_overrides", "get16u_probe"):
            changed = copy.deepcopy(state)
            del changed[key]
            with self.subTest(key=key), self.assertRaises(contract.ReaderRefused):
                contract.recognize(changed)

    def test_read_width_offset_setup_and_literal_mutations_are_not_equivalent(self):
        mutations = [
            ("get16u", "'S'", "'L'"),
            ("do_unpack_std", '"x$_[2] ', '"x$_[2] x '),
            ("set_byte_order", "$order eq 'II'", "$order eq 'XX'"),
            ("set_byte_order", "/^Big/i", "/^B ig/i"),
            ("set_byte_order", "'A '", "'A'"),
        ]
        for key, before, after in mutations:
            state = snapshot()
            state["isolated_functions"][key]["__deparse"] = state["isolated_functions"][key]["__deparse"].replace(before, after)
            state["loaded_functions"][key] = copy.deepcopy(state["isolated_functions"][key])
            with self.subTest(key=key, before=before), self.assertRaises(contract.ReaderRefused):
                contract.recognize(state)

    def test_endian_maps_and_successful_transitions_must_match(self):
        for key, value in (("unpack_std_s", "n"), ("set_return", 0), ("reported_byte_order", "MM")):
            state = snapshot()
            state["observations"]["orders"]["II"][key] = value
            with self.subTest(key=key), self.assertRaises(contract.ReaderRefused):
                contract.recognize(state)

    def test_loaded_global_state_cannot_disagree_with_unchanged_functions(self):
        state = snapshot()
        state["loaded_state"] = {"reported_byte_order": "II", "unpack_std_s": "n"}
        with self.assertRaises(contract.ReaderRefused):
            contract.recognize(state)

    def test_boundary_observations_must_describe_each_native_case(self):
        mutations = [
            (0, "name", "offset_at_end"),
            (1, "byte_order", "MM"),
            (2, "bytes_hex", "aabb"),
            (3, "offset", 3),
            (4, "expected_outcome", "undef"),
            (4, "error", None),
        ]
        for index, key, value in mutations:
            state = snapshot()
            state["get16u_probe"]["boundary_cases"][index][key] = value
            with self.subTest(index=index, key=key), self.assertRaises(contract.ReaderRefused):
                contract.recognize(state)
        state = snapshot()
        state["get16u_probe"]["boundary_cases"] = [
            {"matches_expected_native_outcome": True} for _ in range(10)
        ]
        with self.assertRaises(contract.ReaderRefused):
            contract.recognize(state)


class ReaderArtifact(unittest.TestCase):
    def test_native_reader_facts_are_mandatory_and_reject_stale_source_or_probe(self):
        fixture = validation_fixture.GeneratedValidation()
        state = snapshot()
        with TemporaryDirectory() as tmp:
            path = fixture.artifact(Path(tmp), fact=bound_helper(state), reader_state=state)
            edge = verify.parse_keyed_rust(path).facts[("Any", "Main", "4097")].edge
            self.assertNotIn("validate_reader_contract", edge[3])
            self.assertIsNotNone(edge[4][6])
            self.assertEqual(fixture.audit(path, reader_state=state).keyed_fact_mismatches, ())
            self.assertTrue(fixture.audit(path).keyed_fact_mismatches)
            changed = copy.deepcopy(state)
            for where in (changed["loaded_functions"], changed["isolated_functions"]):
                for fact in where.values():
                    fact["source_sha256"] = "3" * 64
            self.assertTrue(fixture.audit(path, reader_state=changed).keyed_fact_mismatches)
            changed = copy.deepcopy(state)
            changed["get16u_probe"]["failure_count"] = 1
            self.assertTrue(fixture.audit(path, reader_state=changed).keyed_fact_mismatches)
            changed = copy.deepcopy(state)
            changed["get16u_probe"]["valid_case_count"] -= 1
            self.assertTrue(fixture.audit(path, reader_state=changed).keyed_fact_mismatches)

    def test_fresh_generation_retains_blocker_for_changed_core_body(self):
        fixture = validation_fixture.GeneratedValidation()
        state = snapshot()
        for where in (state["loaded_functions"], state["isolated_functions"]):
            where["get16u"]["__deparse"] = where["get16u"]["__deparse"].replace("'S'", "'L'")
        with TemporaryDirectory() as tmp:
            path = fixture.artifact(Path(tmp), fact=bound_helper(state), reader_state=state)
            edge = verify.parse_keyed_rust(path).facts[("Any", "Main", "4097")].edge
            self.assertIn("validate_reader_contract", edge[3])
            self.assertIsNone(edge[4][6])

    def test_loaded_alias_and_builtin_override_keep_validation_blocked(self):
        state = snapshot()
        record = bound_helper(state)
        record["dependencies"]["Image::ExifTool::Get16u"]["__name"] = "Image::ExifTool::Get32u"
        self.assertIsNone(directory_validation.compile_validation(CALL, {NAME: record}, {"unsigned16": state}).reader_contract_sha256)
        state["builtin_overrides"]["unpack"] = True
        self.assertIsNone(directory_validation.compile_validation(CALL, {NAME: bound_helper(state)}, {"unsigned16": state}).reader_contract_sha256)

    def test_deparser_scalar_binder_variation_is_only_a_format_change(self):
        state = snapshot()
        original = contract.fingerprint(state)
        for where in (state["isolated_functions"], state["loaded_functions"]):
            where["set_byte_order"]["__deparse"] = where["set_byte_order"]["__deparse"].replace("my $order", "my($order)")
        self.assertEqual(contract.fingerprint(state), original)

    def test_source_change_and_conditional_loaded_body_change_alter_identity(self):
        state = snapshot()
        first = contract.fingerprint(state)
        for where in (state["isolated_functions"], state["loaded_functions"]):
            for key, _name, _body in contract._FUNCTIONS:
                where[key]["source_sha256"] = "3" * 64
        self.assertNotEqual(contract.fingerprint(state), first)
        state["loaded_functions"]["get16u"]["__deparse"] = contract._GET16U.replace("'S'", "'L'")
        with self.assertRaises(contract.ReaderRefused):
            contract.recognize(state)


if __name__ == "__main__":
    unittest.main()
