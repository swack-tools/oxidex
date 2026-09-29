"""Keep real packed LNK tags while removing proven PrintConv rows."""
import unittest

from prune_printconv_tag_entries import GroundTruth, classify


class PrunePackedIds(unittest.TestCase):
    def test_native_lnk_stringdata_aliases_survive_numeric_flags(self):
        # ExifTool 11.78 and 12.64 listx uses numeric Flags keys; 13.59
        # changes these to Bit2/Bit3. The real StringData IDs stay unchanged.
        gt = GroundTruth(
            {"LNK::Main": {"Description", "RelativePath"}}, {},
            {"LNK::Main": {"Description": {0x30004}, "RelativePath": {0x30008}}},
            {"LNK::Main": {4: {"Description"}, 8: {"RelativePath"}}},
        )
        for name, tag_id in (("Description", "0x0004"), ("RelativePath", "0x0008")):
            self.assertIsNone(classify("LNK::Main", tag_id, name, gt))

    def test_alias_does_not_hide_a_different_section_or_table_collision(self):
        for table, real_id in (("LNK::Main", 0x20004), ("Exif::Main", 0x30004)):
            gt = GroundTruth(
                {table: {"Description"}}, {},
                {table: {"Description": {real_id}}},
                {table: {4: {"Description"}}},
            )
            self.assertEqual(
                classify(table, "0x0004", "Description", gt),
                "printconv-value-shadowing-a-real-tag-name",
            )

    def test_native_nikon_vehicle_display_value_is_removed(self):
        table = "Nikon::AutoCaptureInfo"
        gt = GroundTruth(
            {table: {"AutoCaptureCriteriaSubjectType"}},
            {table: {"Vehicle"}},
            {table: {"AutoCaptureCriteriaSubjectType": {106}}},
            {table: {3: {"Vehicle"}}},
        )
        self.assertEqual(classify(table, "0x0003", "Vehicle", gt), "printconv-value-as-tag")
        self.assertIsNone(classify(table, "106", "AutoCaptureCriteriaSubjectType", gt))


if __name__ == "__main__":
    unittest.main()
