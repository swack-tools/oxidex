#!/usr/bin/env python3
"""Tests for `doctor.py --json` / `doctor.registration_payload` (PLAN
Stage 3 task 7: "doctor.py gains --json (machine-readable check results,
used as the runner's registration payload: platform_id, rustc_id, cores,
free disk/mem, has_oracle, gate_version, keel_version)").

`registration_payload` is tested directly against hand-built `Check`
lists (no subprocess, no real toolchain/oracle/corpus dependency); the
`--json` CLI surface is tested by running `doctor.main()` in-process with
`sys.argv` patched, capturing stdout, and asserting it parses as ONE JSON
object with nothing else mixed in -- the exact property a runner
consuming this as a registration payload depends on.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import io
import json
import sys
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

FLEET_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(FLEET_DIR))

import doctor  # noqa: E402
from _env import HermeticCase  # noqa: E402


def _check(name, ok=True, detail="fine", value=None):
    c = doctor.Check(name)
    if ok is None:
        c.info(detail)
    elif ok:
        c.passed(detail)
    else:
        c.failed(detail)
    c.value = value
    return c


class TestRegistrationPayloadShape(HermeticCase):
    def _checks(self, **overrides):
        checks = {
            "toolchain id": _check("toolchain id"),
            "linker version": _check("linker version", ok=None),
            "oracle (-ver + OOXML.docx capability)": _check(
                "oracle (-ver + OOXML.docx capability)"
            ),
            "corpus count": _check("corpus count"),
            "free disk": _check("free disk", value=123.456),
            "clock (NTP offset)": _check("clock (NTP offset)", value=0.042),
            "git token file": _check("git token file", ok=None),
        }
        checks.update(overrides)
        return list(checks.values())

    def test_field_list_matches_the_task_spec(self):
        payload = doctor.registration_payload("m5", self._checks())
        for field in (
            "host", "platform_id", "rustc_id", "cores", "free_disk_gb",
            "free_mem_gb", "has_oracle", "gate_version", "keel_version",
            "ntp_offset_s", "checks", "failed", "ok",
        ):
            self.assertIn(field, payload, msg=f"missing field {field!r}")

    def test_host_label_passed_through_verbatim(self):
        payload = doctor.registration_payload("oldair", self._checks())
        self.assertEqual(payload["host"], "oldair")

    def test_free_disk_gb_comes_from_the_disk_checks_value_not_recomputed(self):
        payload = doctor.registration_payload("m5", self._checks())
        self.assertEqual(payload["free_disk_gb"], 123.5)  # rounded to 1dp

    def test_ntp_offset_s_comes_from_the_clock_checks_value(self):
        payload = doctor.registration_payload("m5", self._checks())
        self.assertEqual(payload["ntp_offset_s"], 0.042)

    def test_ntp_offset_s_is_null_when_the_clock_check_failed_unmeasurably(self):
        checks = self._checks(**{
            "clock (NTP offset)": _check("clock (NTP offset)", ok=False, value=None),
        })
        payload = doctor.registration_payload("m5", checks)
        self.assertIsNone(payload["ntp_offset_s"])

    def test_has_oracle_true_when_oracle_check_passed(self):
        payload = doctor.registration_payload("m5", self._checks())
        self.assertIs(payload["has_oracle"], True)

    def test_has_oracle_false_when_oracle_check_failed(self):
        checks = self._checks(**{
            "oracle (-ver + OOXML.docx capability)": _check(
                "oracle (-ver + OOXML.docx capability)", ok=False
            ),
        })
        payload = doctor.registration_payload("m5", checks)
        self.assertIs(payload["has_oracle"], False)

    def test_has_oracle_false_when_oracle_check_absent_entirely(self):
        """A caller that hands a partial checks list (a future refactor
        that renames the oracle check, a test double) must fail closed
        -- `has_oracle` defaults False, never True by the absence of
        evidence."""
        checks = [c for c in self._checks() if c.name != "oracle (-ver + OOXML.docx capability)"]
        payload = doctor.registration_payload("m5", checks)
        self.assertIs(payload["has_oracle"], False)

    def test_failed_count_matches_checks_that_failed(self):
        checks = self._checks(**{
            "corpus count": _check("corpus count", ok=False),
            "free disk": _check("free disk", ok=False, value=1.0),
        })
        payload = doctor.registration_payload("m5", checks)
        self.assertEqual(payload["failed"], 2)
        self.assertIs(payload["ok"], False)

    def test_ok_true_when_nothing_failed(self):
        payload = doctor.registration_payload("m5", self._checks())
        self.assertEqual(payload["failed"], 0)
        self.assertIs(payload["ok"], True)

    def test_informational_checks_never_count_as_failed(self):
        """`linker version`/`git token file` are `ok=None` (informational)
        in a healthy run -- must not be swept into `failed`."""
        payload = doctor.registration_payload("m5", self._checks())
        self.assertEqual(payload["failed"], 0)

    def test_checks_list_carries_name_ok_and_detail_for_every_check(self):
        payload = doctor.registration_payload("m5", self._checks())
        self.assertEqual(len(payload["checks"]), 7)
        for entry in payload["checks"]:
            self.assertIn("name", entry)
            self.assertIn("ok", entry)
            self.assertIn("detail", entry)

    def test_gate_version_reads_the_real_gate_version_file(self):
        payload = doctor.registration_payload("m5", self._checks())
        expected = (FLEET_DIR / "gate_version.txt").read_text().strip()
        self.assertEqual(payload["gate_version"], expected)

    def test_keel_version_matches_keel_election_module(self):
        payload = doctor.registration_payload("m5", self._checks())
        self.assertEqual(payload["keel_version"], doctor.KEEL_VERSION)

    def test_payload_is_json_serializable(self):
        payload = doctor.registration_payload("m5", self._checks())
        json.dumps(payload)  # must not raise


class TestJsonCliOutputIsPureJson(HermeticCase):
    """`--json` must print exactly one JSON object and nothing else to
    stdout, so a runner can `json.loads(subprocess_stdout)` directly --
    the whole reason this mode exists rather than asking a runner to
    strip the human instrument header first."""

    def _run_json_mode(self, monkeypatches):
        buf = io.StringIO()
        with mock.patch.object(sys, "argv", ["doctor.py", "test-host", "--json"]):
            with mock.patch.multiple(doctor, **monkeypatches):
                with redirect_stdout(buf):
                    rc = doctor.main()
        return rc, buf.getvalue()

    def _stub_checks(self):
        return dict(
            check_toolchain=lambda: _check("toolchain id"),
            check_linker=lambda: _check("linker version", ok=None),
            check_oracle=lambda: _check("oracle (-ver + OOXML.docx capability)"),
            check_corpus=lambda: _check("corpus count"),
            check_disk=lambda: _check("free disk", value=42.0),
            check_ntp_offset=lambda: _check("clock (NTP offset)", value=0.01),
            check_git_token_file=lambda: _check("git token file", ok=None),
        )

    def test_stdout_is_exactly_one_json_object(self):
        rc, out = self._run_json_mode(self._stub_checks())
        self.assertEqual(rc, 0)
        # Nothing before or after the JSON -- a strict single-object parse,
        # not "the output contains valid JSON somewhere".
        payload = json.loads(out)
        self.assertEqual(payload["host"], "test-host")
        self.assertEqual(out.count("\n"), 1, "exactly one line: the JSON object plus print()'s newline")

    def test_exit_code_is_the_failed_check_count_in_json_mode_too(self):
        stubs = self._stub_checks()
        stubs["check_corpus"] = lambda: _check("corpus count", ok=False)
        rc, out = self._run_json_mode(stubs)
        self.assertEqual(rc, 1)
        payload = json.loads(out)
        self.assertEqual(payload["failed"], 1)

    def test_no_instrument_header_leaks_into_json_mode(self):
        rc, out = self._run_json_mode(self._stub_checks())
        self.assertNotIn("=== instrument", out)


if __name__ == "__main__":
    unittest.main()
