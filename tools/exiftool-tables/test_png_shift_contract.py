"""The selected PNG shift contract follows the AddChunks branch, not a release label."""
from pathlib import Path
import tempfile
import unittest

from png_shift_contract import UnsupportedSource, source_fact


class PngShiftContractTests(unittest.TestCase):
    def source(self, guard: str, *, fake: bool) -> Path:
        self.addCleanup(self.directory.cleanup)
        lib = Path(self.directory.name)
        base = lib / "Image/ExifTool"
        base.mkdir(parents=True)
        (base / "WritePNG.pl").write_text(
            "sub AddChunks($$;@)\n{\n"
            "    foreach $tag (sort keys %$addTags) {\n"
            "        my $tagInfo = $$addTags{$tag};\n"
            + ("        next if $$tagInfo{FakeTag}; # (iCCP-name)\n" if fake else "")
            + "        my $nvHash = $et->GetNewValueHash($tagInfo);\n"
            "        # (native PNG information is always preferred, so don't rely on just IsCreating)\n"
            f"        next unless {guard};\n"
            "        my $val = $et->GetNewValue($nvHash);\n"
            "    }\n}\n"
        )
        (base / "Writer.pl").write_text(
            "if ($permanent or $shift) {\n"
            "    # don't create permanent or Shift-ed tag but define IsCreating\n"
            "    # so we know that it is the preferred tag\n"
            "    $$nvHash{IsCreating} = 0;\n}\n"
        )
        (base / "PNG.pm").write_text(
            "'create-date'=> {\n Name => 'CreateDate',\n"
            " Groups => { 2 => 'Time' },\n Shift => 'Time',\n},\n"
        )
        (base / "Shortcuts.pm").write_text(
            "AllDates => [ 'DateTimeOriginal', 'CreateDate', 'ModifyDate', ],\n"
        )
        return lib

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()

    def test_old_unknown_overwrite_creates_literal(self):
        lib = self.source("$et->IsOverwriting($nvHash)", fake=False)
        self.assertIs(source_fact(lib)[0], True)

    def test_positive_overwrite_guard_skips_absent_shift(self):
        lib = self.source("$$nvHash{IsCreating} or $et->IsOverwriting($nvHash) > 0", fake=True)
        self.assertIs(source_fact(lib)[0], False)

    def test_unreviewed_guard_refused(self):
        lib = self.source("$et->IsOverwriting($nvHash) >= 0", fake=True)
        with self.assertRaises(UnsupportedSource):
            source_fact(lib)

    def test_missing_shift_declaration_refused(self):
        lib = self.source("$et->IsOverwriting($nvHash)", fake=False)
        png = lib / "Image/ExifTool/PNG.pm"
        png.write_text(png.read_text().replace("Shift => 'Time'", "Shift => 'Number'"))
        with self.assertRaises(UnsupportedSource):
            source_fact(lib)


if __name__ == "__main__":
    unittest.main()
