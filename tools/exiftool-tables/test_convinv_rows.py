"""Closed admission tests for source-derived static ConvInv rows."""
from copy import deepcopy
import unittest

from checkexif_recipes import RecipeMalformed, RecipeRefused
from convinv_rows import compile_rows
from test_checkexif_recipes import BODY, document


def fact(value=None, *, present=True):
    return {"present": present, **({"value": value} if present else {})}


def ready():
    value = document(BODY)
    helper = value["native_write_helpers"]["check_value"]
    helper["__deparse"] = "($$) { package Image::ExifTool; return $_[0]; }"
    table = value["native_write_tables"]["Exif"]["Main"]
    table["effective_row_resolver"] = {
        "actual_name": "Image::ExifTool::GetTagInfo", "resolved": True,
        "source_file": "Image/ExifTool.pm", "source_sha256": "b" * 64,
    }
    table["effective_row_context"] = {
        "is_writing": True, "selection": "native_get_tag_info_write_context",
    }
    properties = {"Name": fact("NoAllowlist"), "WriteGroup": fact("IFD0"), "Writable": fact("string")}
    effective = {key: fact(raw["value"]) for key, raw in properties.items()}
    effective.update({"Format": fact(present=False), "Count": fact(present=False), "Groups": fact(present=False),
                      "PrintConv": fact(present=False), "PrintConvInv": fact(present=False),
                      "ValueConv": fact(present=False), "ValueConvInv": fact(present=False),
                      "List": fact(present=False), "RawJoin": fact(present=False),
                      "WriteCheck": fact(present=False), "RawConvInv": fact(present=False)})
    controls = {key: fact(present=False) for key in ("Writable", "WriteGroup", "PrintConvInv", "ValueConvInv", "RawConvInv", "WriteCheck")}
    controls.update({"Writable": fact("string"), "WriteGroup": fact("IFD0")})
    table["table_properties"] = {"WRITABLE": fact(present=False)}
    table["write_controls"] = {"WRITABLE": fact(present=False)}
    table["rows"] = {"raw-not-name": {"entry_kind": "HASH", "properties": properties,
                                         "write_controls": controls, "unknown_properties": {},
                                         "effective_properties": effective,
                                         "effective_resolution": "native_get_tag_info",
                                         "effective_table_binding": {
                                             "kind": "containing_table",
                                             "ref_identical_to_containing": True,
                                         }}}
    return value


class ConvInvRowsTests(unittest.TestCase):
    def test_empty_perl_hash_key_is_preserved_instead_of_aborting_generation(self):
        source = ready()
        table = source["native_write_tables"]["Exif"]["Main"]
        table["rows"][""] = table["rows"].pop("raw-not-name")
        rows, report = compile_rows(source)
        self.assertEqual(rows[0].raw_id, "")
        self.assertEqual(report["rows_omitted"], 0)
        # A valid empty-key map without a tag name is still an omission,
        # not a synthesized tag and not a malformed whole native dump.
        table["rows"][""]["effective_properties"]["Name"] = fact(present=False)
        rows, report = compile_rows(source)
        self.assertEqual(rows, ())
        self.assertEqual(report["omitted_rows"][0]["raw_id"], "")
        self.assertIn("Name or WriteGroup", report["omitted_rows"][0]["reason"])

    def test_non_text_hash_key_remains_malformed(self):
        source = ready()
        table = source["native_write_tables"]["Exif"]["Main"]
        table["rows"][316] = table["rows"].pop("raw-not-name")
        with self.assertRaisesRegex(RecipeMalformed, "invalid raw id"):
            compile_rows(source)

    def test_projected_states_and_raw_id_need_no_name_or_id_allowlist(self):
        source = ready()
        effective = source["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]["effective_properties"]
        effective["PrintConv"] = fact("0")
        effective["RawJoin"] = fact("0")
        rows, report = compile_rows(source)
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0].raw_id, "raw-not-name")
        self.assertEqual(rows[0].conversion[0], "Defined")
        self.assertFalse(rows[0].gates[1])
        self.assertEqual(report["rows_omitted"], 0)

    def test_source_control_mismatch_is_malformed_not_a_convenient_projection(self):
        source = ready()
        source["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]["write_controls"]["Writable"] = fact("int16u")
        with self.assertRaisesRegex(RecipeMalformed, "disagree"):
            compile_rows(source)

    def test_unrepresented_effective_selection_is_a_named_omission(self):
        source = ready()
        source["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]["effective_resolution"] = "unrepresented_get_tag_info_condition"
        rows, report = compile_rows(source)
        self.assertEqual(rows, ())
        self.assertIn("effective-property resolution is unrepresented", report["omitted_rows"][0]["reason"])

    def test_condition_is_refused_without_evaluating_a_context_free_branch(self):
        source = ready()
        source["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]["properties"]["Condition"] = fact("die 'must not run'")
        rows, report = compile_rows(source)
        self.assertEqual(rows, ())
        self.assertIn("Condition requires an unrepresented native selection context",
                      report["omitted_rows"][0]["reason"])

    def test_read_context_cannot_be_substituted_for_writer_selection(self):
        source = ready()
        source["native_write_tables"]["Exif"]["Main"]["effective_row_context"]["is_writing"] = False
        with self.assertRaisesRegex(RecipeRefused, "write context"):
            compile_rows(source)

    def test_redirected_effective_table_cannot_borrow_containing_checker(self):
        source = ready()
        row = source["native_write_tables"]["Exif"]["Main"]["rows"]["raw-not-name"]
        row["effective_table_binding"] = {
            "kind": "redirected_table", "ref_identical_to_containing": False,
        }
        rows, report = compile_rows(source)
        self.assertEqual(rows, ())
        self.assertIn("effective Table binding is not the authenticated containing table",
                      report["omitted_rows"][0]["reason"])


if __name__ == "__main__":
    unittest.main()
