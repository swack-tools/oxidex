import copy
import unittest

from quicktime_atom_tables import inventory


def source():
    tables = {name: {"meta": {"GROUPS": {"1": name}}, "tag_count": 0, "tags": {}}
              for name in ("ItemList", "UserData", "Keys")}
    tables["ItemList"].update(tag_count=1, tags={"plID": {
        "Name": "AlbumID", "Format": "int64u", "Writable": "int32s"}})
    return {"exiftool_version": "13.59", "modules": {"QuickTime": {"tables": tables}}}


class CapabilityTests(unittest.TestCase):
    def test_new_source_row_appears_without_a_name_list(self):
        data = source()
        table = data["modules"]["QuickTime"]["tables"]["ItemList"]
        table["tags"]["new!"] = {"Name": "FutureCounter", "Format": "int64u"}
        table["tag_count"] += 1
        result = inventory(data)["families"][0]
        self.assertEqual(result["declarative_candidates"], 2)
        specs = {r["spec"]["name"]: r["spec"] for r in result["records"]}
        self.assertEqual(specs["AlbumID"]["format"], "int64u")
        self.assertEqual(specs["FutureCounter"]["format"], "int64u")
        self.assertFalse(any(r["runtime_connected"] for r in result["records"]))

    def test_new_reading_callback_refuses_instead_of_reusing_a_name(self):
        data = source()
        data["modules"]["QuickTime"]["tables"]["ItemList"]["tags"]["plID"]["ValueConv"] = {
            "kind": "perl", "expr": "$val + 1"}
        record = inventory(data)["families"][0]["records"][0]
        self.assertIn("unsupported_source_property:ValueConv", record["reasons"])
        self.assertNotIn("spec", record)

    def test_conditional_alternatives_remain_separate_and_refused(self):
        data = source()
        tags = data["modules"]["QuickTime"]["tables"]["ItemList"]["tags"]
        tags["plID"] = {"_variants": [tags["plID"], {"Name": "Alternate"}]}
        family = inventory(data)["families"][0]
        self.assertEqual((family["raw_rows"], family["variant_records"]), (1, 2))
        self.assertEqual(family["refused_records"], 2)

    def test_userdata_does_not_borrow_itemlist_protocol(self):
        data = source()
        tables = data["modules"]["QuickTime"]["tables"]
        tables["UserData"] = copy.deepcopy(tables["ItemList"])
        record = inventory(data)["families"][1]["records"][0]
        self.assertIn("protocol_userdata_language_records", record["reasons"])
        self.assertNotIn("spec", record)

    def test_count_mismatch_cannot_define_denominator(self):
        data = source()
        data["modules"]["QuickTime"]["tables"]["ItemList"]["tag_count"] = 99
        with self.assertRaises(ValueError):
            inventory(data)


if __name__ == "__main__":
    unittest.main()
