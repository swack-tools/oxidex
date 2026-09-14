"""Source-selected ordinary EXIF addressing tests."""
from copy import deepcopy
from dataclasses import replace
import json
from pathlib import Path
import unittest

from checkexif_recipes import RecipeRefused
from convinv_rows import compile_rows
from setnewvalue_addressing import compile_addressing, resolve, resolve_batch
from test_checkexif_recipes import fact
from test_convinv_rows import ready


ROOT = Path(__file__).parent
SETNEW = json.loads((ROOT / "setnewvalue_convinv_full_template.json").read_text())
FIND = json.loads((ROOT / "findtaginfo_full_template.json").read_text())


def source():
    value = ready()
    helpers = value["native_write_helpers"]
    conv = fact("Image::ExifTool::ConvInv", "return;", source="Image/ExifTool/Writer.pl")
    helpers["set_new_value"] = fact(
        "Image::ExifTool::SetNewValue", " ".join(SETNEW),
        requested="Image::ExifTool::SetNewValue", source="Image/ExifTool/Writer.pl")
    helpers["set_new_value"]["dependencies"] = {"Image::ExifTool::ConvInv": conv}
    helpers["find_tag_info"] = fact(
        "Image::ExifTool::TagLookup::FindTagInfo", " ".join(FIND),
        requested="Image::ExifTool::TagLookup::FindTagInfo", source="Image/ExifTool/TagLookup.pm")
    table = value["native_write_tables"]["Exif"]["Main"]
    table["table_properties"]["GROUPS"] = {
        "present": True, "value": {"0": "EXIF", "1": "IFD0", "2": "Image"}}
    return value


def observations(rows, *, external=False):
    row = rows[0]
    candidate = {"module": row.module, "table": row.table, "full_name": row.full_name,
                 "raw_id": row.raw_id, "name": row.name,
                 "groups": {"0": "EXIF", "1": "IFD0", "2": "Image"}}
    values = [candidate]
    if external:
        values.append({"external_table": "Image::ExifTool::XMP::Main", "raw_id": "other",
                       "name": row.name, "groups": {"0": "XMP", "1": "XMP", "2": "Image"}})
    return {"schema": "native_setnewvalue_addressing_v1",
            "queries": {row.name.lower(): {"query": row.name, "candidates": values}}}


class SetNewValueAddressingTests(unittest.TestCase):
    def compiled(self):
        addressing, report = compile_addressing(source())
        self.assertEqual(report["rows_emitted"], 1)
        self.assertEqual(addressing.rows[0].name, "NoAllowlist")
        return addressing

    def test_case_insensitive_name_and_exif_ifd0_qualifiers_resolve_identity(self):
        addressing = self.compiled()
        native = observations(addressing.rows)
        expected = addressing.rows[0].identity
        for spelling in ("NoAllowlist", "nOaLlOwLiSt", "EXIF:NoAllowlist", "ifd0:nOaLlOwLiSt"):
            with self.subTest(spelling=spelling):
                result = resolve(addressing, native, spelling)
                self.assertEqual(result.state, "resolved")
                self.assertEqual(result.row.identity, expected)

    def test_ambiguity_and_external_native_candidates_are_terminal(self):
        addressing = self.compiled()
        native = observations(addressing.rows, external=True)
        result = resolve(addressing, native, "NoAllowlist")
        self.assertEqual(result.state, "owned_unsupported")
        self.assertIn("outside generated", result.reason)

        duplicate_row = replace(addressing.rows[0], raw_id="other")
        addressing = replace(addressing, rows=(addressing.rows[0], duplicate_row))
        native = observations(addressing.rows)
        duplicate = deepcopy(native["queries"]["noallowlist"]["candidates"][0])
        duplicate["raw_id"] = duplicate_row.raw_id
        native["queries"]["noallowlist"]["candidates"].append(duplicate)
        result = resolve(addressing, native, "NoAllowlist")
        self.assertEqual(result.state, "owned_unsupported")
        self.assertIn("ambiguous", result.reason)

    def test_owned_unsupported_never_becomes_outside_scope(self):
        addressing = self.compiled()
        native = observations(addressing.rows)
        self.assertEqual(resolve(addressing, native, "ExifIFD:NoAllowlist").state, "owned_unsupported")
        self.assertEqual(resolve(addressing, native, "UnknownTag").state, "outside_migrated_scope")
        self.assertEqual(resolve(addressing, {"schema": "native_setnewvalue_addressing_v1", "queries": {}},
                                 "NoAllowlist").state, "owned_unsupported")

    def test_aliases_deduplicate_and_conflicts_refuse_same_physical_field(self):
        addressing = self.compiled()
        native = observations(addressing.rows)
        accepted, failures = resolve_batch(addressing, native, {
            "NoAllowlist": "one", "IFD0:noallowlist": "one"})
        self.assertEqual(len(accepted), 1)
        self.assertEqual(failures, ())
        accepted, failures = resolve_batch(addressing, native, {
            "NoAllowlist": "one", "EXIF:noallowlist": "two"})
        self.assertEqual(len(accepted), 1)
        self.assertEqual(len(failures), 1)
        self.assertIn("conflicting duplicate", failures[0].reason)

    def test_source_name_and_group_changes_do_not_retain_old_address_recipe(self):
        changed = source()
        table = changed["native_write_tables"]["Exif"]["Main"]
        table["table_properties"]["GROUPS"]["value"]["1"] = "Other"
        addressing, report = compile_addressing(changed)
        self.assertEqual(addressing.rows, ())
        self.assertEqual(report["rows_omitted"], 1)
        result = resolve(addressing, {"schema": "native_setnewvalue_addressing_v1", "queries": {}},
                         "NoAllowlist")
        self.assertEqual(result.state, "owned_unsupported")

        changed = source()
        changed["native_write_helpers"]["find_tag_info"]["__deparse"] = " ".join(FIND).replace("lc", "uc", 1)
        with self.assertRaisesRegex(RecipeRefused, "FindTagInfo"):
            compile_addressing(changed)

    def test_address_rows_are_projected_from_source_rows_not_a_name_or_id_list(self):
        doc = source()
        raw = doc["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        raw["properties"]["Name"]["value"] = "ChangedName"
        raw["effective_properties"]["Name"]["value"] = "ChangedName"
        addressing, _report = compile_addressing(doc)
        self.assertEqual(addressing.rows[0].name, "ChangedName")
        self.assertEqual(addressing.rows[0].raw_id, "raw-not-name")


if __name__ == "__main__":
    unittest.main()
