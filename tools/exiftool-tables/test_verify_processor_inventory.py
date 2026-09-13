"""Strict protocol tests for generic native processor inventory records."""

import json
import os
from pathlib import Path
import sys
import unittest


TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
import verify_processor_inventory as inventory  # noqa: E402


SHA = "a" * 64


def code(name="Image::ExifTool::Fixture::ProcessWords", *, resolved=True):
    fact = {"__perl": "CODE", "__opaque": True, "__name": name, "resolved": resolved,
            "__deparse": "sub { return 1; }" if resolved else None,
            "source_file": "Image/ExifTool/Fixture.pm" if resolved else None,
            "source_sha256": SHA if resolved else None}
    if not resolved:
        fact["reason"] = "code_ref_unavailable"
    return fact


def native(kind="scalar", value="x"):
    if kind == "scalar":
        return {"kind": kind, "value": value}
    return {"kind": kind}


def property(present=False, value=None):
    return {"present": True, "value": native(value=value)} if present else {"present": False}


def row(name, *, hashed=False):
    if not hashed:
        return {"entry_kind": "SCALAR", "name": name}
    properties = {key: property() for key in inventory._ROW_PROPERTIES}
    properties["Format"] = property(True, "int8u")
    return {"entry_kind": "HASH", "name": name, "properties": properties,
            "expanded_properties": {},
            "effective_flags": {key: native("undef") for key in inventory._EFFECTIVE_FLAGS}}


def table(processor, count, named):
    return {"processor": processor, "groups": property(), "format": property(), "first_entry": property(),
            "row_record_count": count, "named_row_count": named}


def stream(*, include_table=True, include_zero=True):
    proc = code()
    records = [
        ("NATIVE_PROCESSOR", "Fixture", "Main", proc),
        ("NATIVE_PROCESSOR_TABLE", "Fixture", "Main", table(proc, 3, 2)),
        ("NATIVE_PROCESSOR_ROW", "Fixture", "Main", "1", "-", row("Plain", hashed=True)),
        ("NATIVE_PROCESSOR_ROW", "Fixture", "Main", "2", "0", row("Variant")),
        ("NATIVE_PROCESSOR_ROW", "Fixture", "Main", "2", "1", row(None)),
    ]
    if include_zero:
        zero = code("Image::ExifTool::Fixture::ProcessEmpty")
        records.extend([
            ("NATIVE_PROCESSOR", "Fixture", "Empty", zero),
            ("NATIVE_PROCESSOR_TABLE", "Fixture", "Empty", table(zero, 0, 0)),
        ])
    if not include_table:
        records = [record for record in records if record[:3] != ("NATIVE_PROCESSOR_TABLE", "Fixture", "Main")]
    return "\n".join("\t".join((*record[:-1], json.dumps(record[-1], sort_keys=True, separators=(",", ":")))) for record in records)


class ProcessorInventoryTests(unittest.TestCase):
    def test_parses_plain_and_variant_identities_and_zero_table(self):
        got = inventory.parse_processor_inventory("BINARY\tignored\n" + stream())
        self.assertEqual(len(got.processors), 2)
        self.assertEqual(len(got.tables), 2)
        self.assertEqual(len(got.rows), 3)
        self.assertIn(inventory.ProcessorRowKey("Fixture", "Main", "1", "-"), got.rows,
                      msg="plain rows retain the '-' identity")
        self.assertIn(inventory.ProcessorRowKey("Fixture", "Main", "2", "0"), got.rows,
                      msg="variant rows retain their indexed identity")
        self.assertEqual(got.tables[inventory.ProcessorKey("Fixture", "Empty")]["row_record_count"], 0)


    @unittest.skipUnless(os.environ.get("OXIDEX_PROCESSOR_INVENTORY"),
                         "set OXIDEX_PROCESSOR_INVENTORY to a pinned oracle inventory")
    def test_full_pinned_inventory_is_strictly_parseable(self):
        path = Path(os.environ["OXIDEX_PROCESSOR_INVENTORY"])
        text = path.read_text(encoding="utf-8")
        got = inventory.parse_processor_inventory(text)
        self.assertGreater(len(got.processors), 0)
        self.assertEqual(set(got.processors), set(got.tables))
        self.assertEqual(sum(table["row_record_count"] for table in got.tables.values()), len(got.rows))


    def test_accepts_explicit_unresolved_processor_and_dependency_facts(self):
        processor = code(resolved=False)
        text = "\n".join((
            "\t".join(("NATIVE_PROCESSOR", "Fixture", "Unavailable", json.dumps(processor))),
            "\t".join(("NATIVE_PROCESSOR_TABLE", "Fixture", "Unavailable", json.dumps(table(processor, 0, 0)))),
        ))
        got = inventory.parse_processor_inventory(text)
        self.assertFalse(got.processors[inventory.ProcessorKey("Fixture", "Unavailable")]["resolved"])

    def test_rejects_extra_table_and_duplicate_row(self):
        processor = code("Image::ExifTool::Fixture::ProcessExtra")
        extra = "\t".join(("NATIVE_PROCESSOR_TABLE", "Fixture", "Extra", json.dumps(table(processor, 0, 0))))
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(stream() + "\n" + extra)
        duplicate = next(line for line in stream().splitlines() if line.startswith("NATIVE_PROCESSOR_ROW\tFixture\tMain\t1\t-\t"))
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(stream() + "\n" + duplicate)

    def test_rejects_duplicate_and_malformed_records(self):
        good = stream()
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(good + "\n" + good.splitlines()[0])
        bad_json = good.replace('{"__deparse"', '{not-json"__deparse"', 1)
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(bad_json)
        bad_variant = good.replace("\t2\t0\t", "\t2\t00\t", 1)
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(bad_variant)

    def test_rejects_missing_entire_table_or_zero_table_against_processor_universe(self):
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(stream(include_table=False))
        text = stream()
        text = "\n".join(line for line in text.splitlines() if not line.startswith("NATIVE_PROCESSOR_TABLE\tFixture\tEmpty\t"))
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(text)

    def test_rejects_missing_or_extra_rows_by_declared_table_counts(self):
        text = stream()
        text = "\n".join(line for line in text.splitlines() if not line.startswith("NATIVE_PROCESSOR_ROW\tFixture\tMain\t2\t1\t"))
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(text)
        extra = "\t".join(("NATIVE_PROCESSOR_ROW", "Fixture", "Main", "9", "-", json.dumps(row("Extra"))))
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(stream() + "\n" + extra)

    def test_rejects_table_processor_disagreement_and_raw_fact_malformation(self):
        text = stream().replace('"source_sha256":"' + SHA + '"', '"source_sha256":"bad"', 1)
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(text)
        text = stream().replace('"row_record_count":3', '"row_record_count":2', 1)
        with self.assertRaises(inventory.ProcessorInventoryError):
            inventory.parse_processor_inventory(text)


if __name__ == "__main__":
    unittest.main()
