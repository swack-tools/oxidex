"""Unit controls for the independent selected word-directory row audit."""

from collections import namedtuple
import json
from pathlib import Path
from types import SimpleNamespace
import os
import sys
import unittest

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
import verify as keyed_verify  # noqa: E402
import verify_processor_inventory as processor  # noqa: E402
import verify_word_rows as rows  # noqa: E402


SHA = "a" * 64
PROCESSOR = "Image::ExifTool::Fixture::ProcessWords"
Fact = namedtuple("Fact", "format count condition raw_conv value_conv groups edge flags")
Source = namedtuple("Source", "format count condition groups subdir flags")
Generated = namedtuple("Generated", "fields enums pc_refused table_scope table_groups facts source_facts layouts", defaults=(None,))
Omissions = namedtuple("Omissions", "present rows")


def code(name=PROCESSOR):
    return {"__perl": "CODE", "__opaque": True, "__name": name, "resolved": True,
            "__deparse": "sub { return 1; }", "source_file": "Image/ExifTool/Fixture.pm",
            "source_sha256": SHA}


def native(kind="scalar", value="x"):
    return {"kind": kind, "value": value} if kind == "scalar" else {"kind": kind}


def prop(value=None):
    return {"present": False} if value is None else {"present": True, "value": value}


def row(name, *, enum=None, condition=None, raw_conv=None, flags=None):
    props = {key: prop() for key in processor._ROW_PROPERTIES}
    if enum is not None:
        props["PrintConv"] = prop({"kind": "hash", "map": {str(k): native("scalar", v) for k, v in enum.items()}})
    if condition is not None:
        props["Condition"] = prop(native("scalar", condition))
    if raw_conv is not None:
        props["RawConv"] = prop(native("scalar", raw_conv))
    effective = {name: native("undef") for name in processor._EFFECTIVE_FLAGS}
    if flags:
        effective.update({name: native("scalar", value) for name, value in flags.items()})
    return {"entry_kind": "HASH", "name": name, "properties": props, "expanded_properties": {},
            "effective_flags": effective}


def table(proc, count, named, *, groups=None, metadata=True):
    fact = {"processor": proc, "groups": prop(groups), "format": prop(), "first_entry": prop(),
            "row_record_count": count, "named_row_count": named}
    if metadata:
        fact["metadata"] = {
            "GROUPS": prop(groups),
            "PROCESS_PROC": prop({"kind": "code", "name": proc["__name"]}),
            "NOTES": prop(), "WRITABLE": prop(), "WRITE_PROC": prop(), "CHECK_PROC": prop(),
        }
    return fact


def inventory_text(*, drop_zero=False, mutation=None):
    first = code()
    second = code(PROCESSOR)
    unknown = code("Image::ExifTool::Fixture::ProcessElse")
    first_row = row("Plain", enum={"0": "Off", "1": "On"})
    second_row = row("Variant", enum={"0": "No", "1": "Yes"})
    if mutation:
        mutation(first_row)
    data = [
        ("NATIVE_PROCESSOR", "Fixture", "Words", first),
        ("NATIVE_PROCESSOR_TABLE", "Fixture", "Words", table(first, 2, 2, groups={"kind": "hash", "map": {"0": native("scalar", "MakerNotes")}})),
        ("NATIVE_PROCESSOR_ROW", "Fixture", "Words", "1", "-", first_row),
        ("NATIVE_PROCESSOR_ROW", "Fixture", "Words", "2", "0", second_row),
        ("NATIVE_PROCESSOR", "Fixture", "Empty", second),
        ("NATIVE_PROCESSOR_TABLE", "Fixture", "Empty", table(second, 0, 0)),
        ("NATIVE_PROCESSOR", "Fixture", "Other", unknown),
        ("NATIVE_PROCESSOR_TABLE", "Fixture", "Other", table(unknown, 0, 0)),
    ]
    if drop_zero:
        data = [item for item in data if item[2] != "Empty"]
    return "\n".join("\t".join((*item[:-1], json.dumps(item[-1], sort_keys=True, separators=(",", ":")))) for item in data)


def generated(*, include_empty=True, extra=False, layouts=True):
    flags = (False, False, False, False, False, None)
    scope = {("Fixture", "Words")}
    if include_empty:
        scope.add(("Fixture", "Empty"))
    fields = {
        ("Fixture", "Words", "1"): "Plain",
        ("Fixture", "Words", "2#0"): "Variant",
    }
    if extra:
        scope.add(("Fixture", "Extra"))
        fields[("Fixture", "Extra", "1")] = "Nope"
    facts = {key: Fact("None", "None", False, False, False, (None, None, None), None, flags) for key in fields}
    source = {key: Source(None, None, None, (None, None, None), None, flags) for key in fields}
    table_groups = {("Fixture", "Words"): ("MakerNotes", "", ""), ("Fixture", "Empty"): ("", "", "")}
    if extra:
        table_groups[("Fixture", "Extra")] = ("", "", "")
    enum = {("Fixture", "Words", "1"): {"0": "Off", "1": "On"},
            ("Fixture", "Words", "2#0"): {"0": "No", "1": "Yes"}}
    descriptor = object()
    layout_map = {key: descriptor for key in scope} if layouts else None
    return Generated(fields, enum, set(), scope, table_groups, facts, source, layout_map)


class WordRowsTests(unittest.TestCase):
    def audit(self, **kwargs):
        inv = processor.parse_processor_inventory(inventory_text())
        return rows.audit_word_rows(generated(**kwargs), Omissions(True, {}), inv, {PROCESSOR})

    def test_exact_selected_scope_includes_zero_table_and_variant_identity(self):
        got = self.audit()
        self.assertTrue(got.ok, got.mismatches)
        self.assertEqual((got.expected_tables, got.expected_rows), (2, 2))
        self.assertEqual((got.generated_tables, got.generated_rows), (2, 2))

    def test_missing_zero_table_and_missing_processor_are_fail_closed(self):
        got = self.audit(include_empty=False)
        self.assertTrue(any("selected native table is absent" in item.reason for item in got.mismatches))
        inv = processor.parse_processor_inventory(inventory_text())
        absent = rows.audit_word_rows(generated(), Omissions(True, {}), inv, {"Image::ExifTool::Fixture::Missing"})
        self.assertTrue(any("PROCESS_PROC is absent" in item.reason for item in absent.mismatches))

    def test_extra_generated_table_and_row_fail(self):
        got = self.audit(extra=True)
        self.assertTrue(any("no selected native processor table" in item.reason for item in got.mismatches))
        self.assertTrue(any("no named native row" in item.reason for item in got.mismatches))

    def test_name_enum_flags_and_default_shape_are_exact(self):
        item = generated()
        bad_names = dict(item.fields)
        bad_names[("Fixture", "Words", "1")] = "Broken"
        bad_enums = dict(item.enums)
        bad_enums[("Fixture", "Words", "1")] = {"0": "Changed", "1": "On"}
        bad_facts = dict(item.facts)
        bad_facts[("Fixture", "Words", "1")] = Fact("Some(Fmt::Int8u)", "Some(1)", False, False, False, (None, None, None), None, (False,) * 5 + (None,))
        changed = item._replace(fields=bad_names, enums=bad_enums, facts=bad_facts)
        inv = processor.parse_processor_inventory(inventory_text())
        got = rows.audit_word_rows(changed, Omissions(True, {}), inv, {PROCESSOR})
        reasons = "\n".join(item.reason for item in got.mismatches)
        self.assertIn("name differs", reasons)
        self.assertIn("enum differs", reasons)
        self.assertIn("Format does not match", reasons)

    def test_explicit_int8u_and_one_retain_exact_raw_projection(self):
        def mutate(native_row):
            native_row["properties"]["Format"] = prop(native("scalar", "int8u"))
            native_row["properties"]["Count"] = prop(native("scalar", "1"))
        inv = processor.parse_processor_inventory(inventory_text(mutation=mutate))
        base = generated()
        key = ("Fixture", "Words", "1")
        facts = dict(base.facts)
        sources = dict(base.source_facts)
        facts[key] = Fact("Some(Fmt::Int8u)", "Some(1)", False, False, False, (None, None, None), None, (False,) * 5 + (None,))
        sources[key] = Source("int8u", "1", None, (None, None, None), None, (False,) * 5 + (None,))
        self.assertTrue(rows.audit_word_rows(base._replace(facts=facts, source_facts=sources), Omissions(True, {}), inv, {PROCESSOR}).ok)

    def test_condition_and_effective_flags_must_match_native_facts(self):
        inv = processor.parse_processor_inventory(inventory_text(mutation=lambda r: (
            r["properties"].__setitem__("Condition", prop(native("scalar", "$val == 1"))),
            r["effective_flags"].__setitem__("Avoid", native("scalar", "1")),
        )))
        got = rows.audit_word_rows(generated(), Omissions(True, {}), inv, {PROCESSOR})
        reasons = "\n".join(item.reason for item in got.mismatches)
        self.assertIn("Condition", reasons)
        self.assertIn("Flags", reasons)

    def test_unmodeled_read_properties_and_table_defaults_fail_closed(self):
        for property_name in ("Mask", "BitShift", "ByteOrder", "DataMember", "Hook", "Offset", "PrintHex", "Require"):
            with self.subTest(property_name=property_name):
                inv = processor.parse_processor_inventory(inventory_text(
                    mutation=lambda r, name=property_name: r["properties"].__setitem__(name, prop(native("scalar", "1")))
                ))
                got = rows.audit_word_rows(generated(), Omissions(True, {}), inv, {PROCESSOR})
                self.assertTrue(any(f"unsupported native {property_name}" in item.reason for item in got.mismatches))
        inv = processor.parse_processor_inventory(inventory_text())
        table_fact = dict(inv.tables)
        key = processor.ProcessorKey("Fixture", "Words")
        mutated = dict(table_fact[key])
        mutated["format"] = prop(native("scalar", "int16u"))
        table_fact[key] = mutated
        changed = processor.ProcessorInventory(inv.processors, table_fact, inv.rows)
        got = rows.audit_word_rows(generated(), Omissions(True, {}), changed, {PROCESSOR})
        self.assertTrue(any("table FORMAT" in item.reason for item in got.mismatches))

    def test_omission_and_unmodeled_conversion_never_pass(self):
        inv = processor.parse_processor_inventory(inventory_text(mutation=lambda r: r["properties"].__setitem__("RawConv", prop(native("scalar", "$val")))))
        omitted = Omissions(True, {("Fixture", "Words", "1"): object()})
        got = rows.audit_word_rows(generated(), omitted, inv, {PROCESSOR})
        reasons = "\n".join(item.reason for item in got.mismatches)
        self.assertIn("unverified word omission", reasons)
        self.assertIn("RawConv", reasons)

    @unittest.skipUnless(os.environ.get("OXIDEX_WORD_KEYED_RUST") and os.environ.get("OXIDEX_PROCESSOR_INVENTORY"),
                         "set OXIDEX_WORD_KEYED_RUST and OXIDEX_PROCESSOR_INVENTORY to canonical artifacts")
    def test_canonical_generated_word_artifact_matches_full_native_processor_inventory(self):
        inventory = processor.parse_processor_inventory(
            Path(os.environ["OXIDEX_PROCESSOR_INVENTORY"]).read_text(encoding="utf-8")
        )
        parsed = keyed_verify.parse_keyed_rust(Path(os.environ["OXIDEX_WORD_KEYED_RUST"]))
        processor_name = os.environ.get("OXIDEX_WORD_PROCESSOR", "Image::ExifTool::CanonCustom::ProcessCanonCustom")
        selected = {key for key, fact in inventory.processors.items() if fact.get("__name") == processor_name}
        generated = SimpleNamespace(**parsed._asdict(), layouts={(key.module, key.table): object() for key in selected})
        omissions = keyed_verify.parse_omitted_keyed_native_rows(Path(os.environ["OXIDEX_WORD_KEYED_RUST"]))
        result = rows.audit_word_rows(generated, omissions, inventory, {processor_name})
        self.assertTrue(result.ok, result.mismatches)

    def test_layouts_filter_all_keyed_tables_but_legacy_projection_is_exact(self):
        inv = processor.parse_processor_inventory(inventory_text())
        item = generated()
        full = item._replace(table_scope=item.table_scope | {("Fixture", "Ciff")},
                             fields={**item.fields, ("Fixture", "Ciff", "99"): "Else"})
        self.assertTrue(rows.audit_word_rows(full, Omissions(True, {}), inv, {PROCESSOR}).ok)
        legacy = item._replace(layouts=None)
        self.assertTrue(rows.audit_word_rows(legacy, Omissions(True, {}), inv, {PROCESSOR}).ok)


if __name__ == "__main__":
    unittest.main()
