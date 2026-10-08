#!/usr/bin/env python3
"""Small refusal controls for the read-only Task19 receipt adapter.

These do not claim that synthetic results pass the production replay verifier.
"""
from __future__ import annotations

from pathlib import Path
import sys
from tempfile import TemporaryDirectory
import unittest

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

import verify_task19_results as adapter


class Task19AdapterControls(unittest.TestCase):
    def test_all_tool_hashes_must_be_explicit_and_unique(self) -> None:
        items = [f"{name}={'a' * 64}" for name in adapter.TOOL_FILES]
        self.assertEqual(len(adapter.tool_expectations(items)), len(adapter.TOOL_FILES))
        for invalid in (items[:-1], items + items[:1], ["unknown.py=" + "a" * 64] + items):
            with self.subTest(invalid=invalid), self.assertRaises(adapter.qualification.Refused):
                adapter.tool_expectations(invalid)

    def test_six_side_binary_identities_must_be_explicit(self) -> None:
        items = [f"{row}:{side}:/target/{row}/{side}/oxidex:{'b' * 64}"
                 for row in adapter.ROWS for side in adapter.qualification.SIDES]
        self.assertEqual(len(adapter.binary_expectations(items)), 6)
        for invalid in (items[:-1], items + items[:1], [items[0].replace('/target/', 'relative/')]+items[1:]):
            with self.subTest(invalid=invalid), self.assertRaises(adapter.qualification.Refused):
                adapter.binary_expectations(invalid)

    def test_stale_policy_and_matrix_refuse_before_receipt_replay(self) -> None:
        with TemporaryDirectory() as directory:
            absent = Path(directory) / 'absent' / 'qualification-result.json'
            binaries = {(row, side): {"path": f"/target/{row}/{side}", "sha256": 'b' * 64}
                        for row in adapter.ROWS for side in adapter.qualification.SIDES}
            tools = {name: adapter.sha(adapter.ROOT / name) for name in adapter.TOOL_FILES}
            common = dict(paths=(absent, absent, absent), expected_head='a' * 40,
                          expected_tree='c' * 40, expected_tools=tools,
                          expected_binaries=binaries)
            with self.assertRaisesRegex(adapter.qualification.Refused, 'accepted read-policy'):
                adapter.verify_results(expected_matrix_sha256='d' * 64,
                                       expected_policy_sha256='e' * 64, **common)
            with self.assertRaisesRegex(adapter.qualification.Refused, 'local canonical matrix'):
                adapter.verify_results(expected_matrix_sha256='d' * 64,
                                       expected_policy_sha256=adapter.POLICY_SHA256, **common)

    def test_missing_committed_final_cannot_pass(self) -> None:
        with TemporaryDirectory() as directory:
            paths = tuple(Path(directory) / name / 'qualification-result.json'
                          for name in ('same', 'forward', 'reverse'))
            binaries = {(row, side): {"path": f"/target/{row}/{side}", "sha256": 'b' * 64}
                        for row in adapter.ROWS for side in adapter.qualification.SIDES}
            tools = {name: adapter.sha(adapter.ROOT / name) for name in adapter.TOOL_FILES}
            with self.assertRaises(adapter.qualification.Refused):
                adapter.verify_results(
                    paths=paths, expected_head='a' * 40, expected_tree='c' * 40,
                    expected_matrix_sha256=adapter.sha(adapter.qualification.CANONICAL_MATRIX),
                    expected_policy_sha256=adapter.POLICY_SHA256,
                    expected_tools=tools, expected_binaries=binaries)


if __name__ == '__main__':
    unittest.main()
