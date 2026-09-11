"""Upgrade triage must distinguish emitted IFD declarations from refusals.

Synthetic dumps exercise the public diff path and the real generator. No
ExifTool binary, generated output, activation change, or oracle ledger needed.
"""
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import triage_bump as triage


def table(tags=None, meta=None):
    return {"meta": meta or {}, "tags": tags or {}}


def doc(tables):
    return {"modules": {"Audit": {"tables": tables}}}


def changes(tag, old=None, key="256", meta=None):
    return list(triage.diff_table(
        "Audit", "Main", table({key: old} if old is not None else {}, meta),
        table({key: tag}, meta)))


class IfdUpgradeTriage(unittest.TestCase):
    def test_new_ifd_table_and_plain_fields_are_auto(self):
        for meta in ({}, {"PROCESS_PROC": "Image::ExifTool::Exif::ProcessExif"},
                     {"PROCESS_PROC": {"__name": "Image::ExifTool::Exif::ProcessExif"}}):
            with self.subTest(meta=meta):
                deltas = list(triage.diff_table("Audit", "Main", None, table({
                    "256": {"Name": "ImageWidth", "Writable": "int32u"}}, meta)))
                self.assertEqual([d.bucket for d in deltas], ["AUTO", "AUTO", "AUTO"])

    def test_changed_emitted_fields_are_auto(self):
        old = {"Name": "Width", "Format": "int16u", "Count": 1,
               "Groups": {"1": "Old"}}
        new = {"Name": "ImageWidth", "Format": "int32u", "Count": 2,
               "Groups": {"1": "New"}}
        self.assertEqual({d.field: d.bucket for d in changes(new, old)}, {
            "Name": "AUTO", "Format": "AUTO", "Count": "AUTO", "Groups": "AUTO"})

    def test_unsized_string_and_enum_map_are_auto(self):
        tag = {"Name": "Mode", "Format": "string", "PrintConv": {
            "kind": "enum", "map": {"A": "Auto"}}}
        self.assertEqual({d.bucket for d in changes(tag)}, {"AUTO"})

    def test_rawconv_member_assignment_is_auto(self):
        tag = {"Name": "Model", "Format": "string", "RawConv": {
            "kind": "expr", "expr": "$$self{Model} = $val"}}
        self.assertEqual({d.bucket for d in changes(tag)}, {"AUTO"})

    def test_selector_does_not_credit_refused_id_or_format(self):
        for key, tag in (("title", {"Name": "Title"}),
                         ("256", {"Name": "Title", "Format": "unicode"})):
            with self.subTest(key=key, tag=tag):
                deltas = list(triage.diff_table("Audit", "Main", None, table({key: tag})))
                self.assertEqual({d.bucket for d in deltas}, {"HAND"})

    def test_offset_and_unrepresented_facts_are_hand(self):
        for field, value in (("Flags", "IsOffset"), ("ChangeBase", 1),
                             ("OffsetPair", "Length"), ("DataTag", "Data"),
                             ("Count", -1), ("Writable", "unicode"),
                             ("Priority", "high"), ("Flags", [3])):
            with self.subTest(field=field):
                self.assertEqual({d.bucket for d in changes({"Name": "X", field: value})},
                                 {"HAND"})

    def test_ignored_fact_is_not_auto(self):
        delta = changes({"Name": "X", "Mask": 3}, {"Name": "X"})[0]
        self.assertEqual((delta.field, delta.bucket), ("Mask", "HAND"))

    def test_undumped_extra_keys_remain_review_work(self):
        deltas = changes({"Name": "X", "_extra_keys": ["NewSemantic"]})
        self.assertEqual({d.field: d.bucket for d in deltas}, {
            "Name": "AUTO", "_extra_keys": "HAND"})

    def test_partly_ignored_compound_facts_are_not_auto(self):
        for field, value in (("Flags", ["Unknown", "NewSemantic"]),
                             ("Groups", {"1": "MakerNotes", "3": "Document"})):
            with self.subTest(field=field):
                delta = changes({"Name": "X", field: value}, {"Name": "X"})[0]
                self.assertEqual((delta.field, delta.bucket), (field, "HAND"))
        deltas = list(triage.diff_table("Audit", "Main", table(), table(meta={
            "GROUPS": {"1": "MakerNotes", "3": "Document"}})))
        self.assertEqual([(d.field, d.bucket) for d in deltas], [("GROUPS", "HAND")])

    def test_compilable_expression_without_domain_is_not_auto(self):
        for field in ("ValueConv", "PrintConv"):
            for extra in ({}, {"Format": "int16u", "Count": -1}):
                with self.subTest(field=field, extra=extra):
                    tag = {"Name": "X", field: {"kind": "expr", "expr": "$val / 2"}, **extra}
                    self.assertEqual({d.bucket for d in changes(tag)}, {"HAND"})

    def test_expression_without_oracle_evidence_is_not_auto(self):
        deltas = changes({"Name": "X", "Format": "int16u", "ValueConv": {
            "kind": "expr", "expr": "$val / 2"}})
        self.assertEqual({d.bucket for d in deltas}, {"HAND"})
        self.assertTrue(any("oracle" in d.note for d in deltas))

    def test_unsupported_conversion_remains_work(self):
        for field, conv, bucket in (
            ("RawConv", {"kind": "expr", "expr": "$val / 2"}, "HAND"),
            ("PrintConv", {"kind": "code", "deparse": "sub { die }"}, "HAND"),
            ("ValueConv", {"kind": "expr", "expr": "something_new($val)"}, "EXPR"),
        ):
            with self.subTest(field=field):
                self.assertEqual({d.bucket for d in changes({
                    "Name": "X", "Format": "int16u", field: conv})}, {bucket})

    def test_standalone_condition_is_cond(self):
        self.assertEqual({d.bucket for d in changes({
            "Name": "X", "Condition": '$$self{Model} eq "A"'})}, {"COND"})

    def test_hook_is_hand(self):
        self.assertEqual({d.bucket for d in changes({
            "Name": "X", "Hook": {"kind": "expr", "expr": "$val"}})}, {"HAND"})

    def test_variants_require_both_conditions_and_fields(self):
        for alternative, bucket in (
            ({"Name": "X", "Condition": '$$self{Model} eq "A"'}, "AUTO"),
            ({"Name": "X", "Condition": '$$self{Model} lt "A"'}, "COND"),
            ({"Name": "X", "Format": "unicode"}, "HAND"),
            ({"Name": "X", "Mask": 3}, "HAND"),
            ({"Name": "X", "RawConv": {"kind": "expr", "expr": "$val / 2"}}, "HAND"),
        ):
            with self.subTest(alternative=alternative):
                deltas = changes({"_variants": [alternative, {"Name": "Default"}]})
                self.assertEqual([(d.field, d.bucket) for d in deltas], [("_variants", bucket)])

    def test_validated_subdirectory_is_hand_even_when_edge_emitted(self):
        tag = {"Name": "Child", "SubDirectory": {
            "TagTable": "Image::ExifTool::Audit::Main", "Validate": "$val == 1"}}
        self.assertEqual({d.bucket for d in changes(tag)}, {"HAND"})

    def test_subdirectory_uses_full_document_target_context(self):
        old = doc({"Main": table(), "Child": table()})
        tag = {"Name": "Child", "SubDirectory": {
            "TagTable": "Image::ExifTool::Audit::Child", "Start": "$val"}}
        new = doc({"Main": table({"1": tag}), "Child": table()})
        deltas = [d for d in triage.run_triage(old, new) if d.kind != "standing"]
        self.assertEqual({d.bucket for d in deltas}, {"AUTO"})
        new["modules"]["Audit"]["tables"].pop("Child")
        deltas = [d for d in triage.run_triage(old, new) if d.tag == "1"]
        self.assertEqual({d.bucket for d in deltas}, {"HAND"})

    def test_emitted_table_metadata_is_auto_but_ignored_metadata_is_hand(self):
        old = table(meta={"GROUPS": {"1": "Old"}})
        new = table(meta={"GROUPS": {"1": "New"}, "SET_GROUP1": 1, "FORMAT": "int16u"})
        deltas = list(triage.diff_table("Audit", "Main", old, new))
        self.assertEqual({d.field: d.bucket for d in deltas}, {
            "GROUPS": "AUTO", "SET_GROUP1": "AUTO", "FORMAT": "HAND"})

    def test_process_change_and_custom_processors_remain_hand(self):
        old = table(meta={"PROCESS_PROC": "Image::ExifTool::Exif::ProcessExif"})
        new = table(meta={"PROCESS_PROC": "Image::ExifTool::Sony::ProcessEnciphered"})
        self.assertEqual([d.bucket for d in triage.diff_table("Audit", "Main", old, new)], ["HAND"])
        deltas = list(triage.diff_table("Audit", "Main", None, table({
            "1": {"Name": "X"}}, new["meta"])))
        self.assertEqual({d.bucket for d in deltas}, {"HAND"})

    def test_invalid_table_priority_is_not_auto_when_old_value_was_emitted(self):
        deltas = list(triage.diff_table("Audit", "Main", table(meta={"PRIORITY": 1}),
                                        table(meta={"PRIORITY": "high"})))
        self.assertEqual([(d.field, d.bucket) for d in deltas], [("PRIORITY", "HAND")])

    def test_removals_regenerate_without_new_work(self):
        deltas = list(triage.diff_table("Audit", "Main", table({"1": {"Name": "X"}}), table()))
        self.assertEqual([(d.bucket, d.kind) for d in deltas], [("AUTO", "removed")])

    def test_cli_json_preserves_counts_and_standing_work(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "old.json").write_text(json.dumps(doc({"Main": table()})))
            (root / "new.json").write_text(json.dumps(doc({"Main": table({
                "1": {"Name": "Width", "Writable": "int32u"}})})))
            subprocess.run([sys.executable, str(Path(triage.__file__)),
                            str(root / "old.json"), str(root / "new.json"),
                            "--json-out", str(root / "report.json")],
                           check=True, capture_output=True, text=True)
            report = json.loads((root / "report.json").read_text())
        added = [d for d in report["deltas"] if d["kind"] == "added"]
        self.assertEqual([d["bucket"] for d in added], ["AUTO", "AUTO"])
        self.assertEqual(report["counts"]["AUTO"], 2)
        self.assertTrue(any(d["kind"] == "standing" and d["bucket"] == "HAND"
                            for d in report["deltas"]))
        self.assertEqual(report["total"], sum(report["counts"].values()))


if __name__ == "__main__":
    unittest.main()
