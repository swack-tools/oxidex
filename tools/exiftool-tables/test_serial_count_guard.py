"""Fail-closed guards for ProcessSerialData count-controller domains.

A count expression consumes the raw value already read at an earlier serial
slot.  The shared Rust reader intentionally refuses a negative count, whereas
native Perl can coerce some signed arithmetic expressions to zero.  Dynamic
counts therefore require every possible selected controller alternative to be a
fixed-count unsigned scalar.  Payloads themselves may still be signed.
"""

import tempfile
from pathlib import Path
import unittest

import serial_directory
import verify_serial_directory as audit
from test_serial_directory import table


REASON = "serial_count_controller_unproved_unsigned"


def document(payload):
    return {"modules": {"Fixture": {"tables": {"Serial": payload}}}}


def compiled(payload):
    return serial_directory.compile_serial_inventory("Fixture", "Serial", payload)


def verifier_rows(payload):
    return audit.native_population(document(payload))[("Fixture", "Serial")].rows


class SerialCountControllerGuardTests(unittest.TestCase):
    def assert_consumer_guarded(self, payload):
        descriptor = compiled(payload)
        self.assertEqual(descriptor["gate_a"]["blocked_by"], [[REASON, 2]])
        self.assertEqual(descriptor["entries"][0]["alternatives"][0]["refusals"], [])
        for index in (1, 2):
            self.assertEqual(descriptor["entries"][index]["alternatives"][0]["refusals"], [REASON])

        rows = verifier_rows(payload)
        controller_rows = [row for key, row in rows.items() if key.index == 0]
        self.assertTrue(controller_rows)
        self.assertTrue(all(row["reasons"] == () for row in controller_rows))
        for index in (1, 2):
            self.assertEqual(rows[audit.RowKey("Fixture", "Serial", index, False, 0)]["reasons"], (REASON,))

    def test_signed_default_count_controller_is_withheld(self):
        payload = table()
        payload["meta"]["FORMAT"] = "int16s"
        self.assert_consumer_guarded(payload)

    def test_non_scalar_count_controller_is_withheld(self):
        payload = table()
        payload["tags"]["0"]["Format"] = "int16u[2]"
        self.assert_consumer_guarded(payload)

    def test_variant_with_a_signed_controller_is_withheld(self):
        payload = table()
        controller = payload["tags"]["0"]
        payload["tags"]["0"] = {
            "_variants": [
                controller,
                {"Name": "SignedCount", "Format": "int16s"},
            ]
        }
        self.assert_consumer_guarded(payload)

    def test_signed_payload_without_count_dependency_remains_modeled(self):
        payload = table()
        payload["tags"]["1"]["Format"] = "int16s[2]"
        descriptor = compiled(payload)
        self.assertEqual(descriptor["gate_a"]["blocked_by"], [])
        signed = descriptor["entries"][1]["alternatives"][0]
        self.assertEqual(signed["format"]["format"], "int16s")
        self.assertEqual(signed["format"]["count"], {"kind": "fixed", "value": 2})
        self.assertEqual(signed["format"]["format_source"], "field")
        self.assertEqual(signed["refusals"], [])

        rows = verifier_rows(payload)
        self.assertEqual(rows[audit.RowKey("Fixture", "Serial", 1, False, 0)]["reasons"], ())
        self.assertEqual(rows[audit.RowKey("Fixture", "Serial", 2, False, 0)]["reasons"], ())

    def test_emitted_guard_is_independently_verified(self):
        payload = table()
        payload["meta"]["FORMAT"] = "int16s"
        source = serial_directory.serial_rust_source(
            serial_directory.compile_serial_population(document(payload))
        )
        with tempfile.TemporaryDirectory() as directory:
            artifact = Path(directory) / "serial_tables.rs"
            artifact.write_text(source, encoding="utf-8")
            result = audit.audit(document(payload), audit.parse_artifact(artifact))
        self.assertTrue(result.ok, result.mismatches)
        self.assertEqual((result.expected_alternatives, result.emitted_alternatives, result.omitted_alternatives), (6, 4, 2))


if __name__ == "__main__":
    unittest.main()
