"""Source and live-state facts for the binary unsigned-reader contract."""

import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
DUMP = REPO_ROOT / "tools/exiftool-tables/dump_binary_reader_contract.pl"
TABLE_DUMP = REPO_ROOT / "tools/exiftool-tables/dump_tables.pl"


class BinaryReaderContract(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.lib = Path(self.tmp.name) / "lib"
        self.core = self.lib / "Image/ExifTool.pm"
        self.core.parent.mkdir(parents=True)
        self.write_core("v")
        self.fixture = self.lib / "Image/ExifTool/Fixture.pm"
        self.fixture.parent.mkdir(parents=True, exist_ok=True)
        self.fixture.write_text("package Image::ExifTool::Fixture; our %Main = (1 => { Name => 'One' }); 1;\n", encoding="utf-8")

    def write_core(self, intel_s):
        self.core.write_text(textwrap.dedent(f"""\
            package Image::ExifTool;
            our $VERSION = 'fixture';
            our ($currentByteOrder, %unpackStd);
            my %unpackMotorola = (S => 'n', L => 'N');
            my %unpackIntel = (S => '{intel_s}', L => 'V');
            $currentByteOrder = 'MM';
            %unpackStd = %unpackMotorola;
            sub DoUnpackStd {{ return $_[2] ? unpack("x$_[2] $unpackStd{{$_[0]}}", ${{$_[1]}}) : unpack($unpackStd{{$_[0]}}, ${{$_[1]}}); }}
            sub Get16u {{ return DoUnpackStd('S', @_); }}
            sub GetByteOrder {{ return $currentByteOrder; }}
            sub SetByteOrder {{
                my ($order) = @_;
                if ($order eq 'MM') {{ %unpackStd = %unpackMotorola; }}
                elsif ($order eq 'II') {{ %unpackStd = %unpackIntel; }}
                else {{ return 0; }}
                $currentByteOrder = $order;
                return 1;
            }}
            1;
        """), encoding="utf-8")

    def dump(self):
        result = subprocess.run(
            ["/usr/bin/perl", str(DUMP), str(self.lib)],
            check=True, text=True, capture_output=True,
        )
        return json.loads(result.stdout)

    def test_captures_live_ii_mm_templates_and_restores_entry_order(self):
        result = self.dump()
        self.assertEqual(result["kind"], "binary_unsigned_reader_contract_v1")
        self.assertEqual(result["observations"]["initial_byte_order"], "MM")
        self.assertEqual(result["observations"]["orders"]["II"], {
            "set_return": 1,
            "reported_byte_order": "II",
            "unpack_std_s": "v",
            "error": None,
        })
        self.assertEqual(result["observations"]["orders"]["MM"], {
            "set_return": 1,
            "reported_byte_order": "MM",
            "unpack_std_s": "n",
            "error": None,
        })
        self.assertEqual(result["observations"]["restore_return"], 1)
        self.assertEqual(result["observations"]["restored_byte_order"], "MM")
        self.assertIsNone(result["observations"]["restore_error"])
        probe = result["get16u_probe"]
        self.assertEqual(probe["kind"], "get16u_native_probe_v1")
        self.assertEqual(probe["valid_case_count"], 4 * 65536)
        self.assertEqual(probe["boundary_case_count"], 10)
        self.assertEqual(probe["failure_count"], 0)
        self.assertEqual(probe["failure_details"], [])
        self.assertEqual(probe["restored_byte_order"], "MM")
        self.assertIsNone(probe["restore_error"])
        for fact in result["loaded_functions"].values():
            self.assertTrue(fact["resolved"])
            self.assertEqual(fact["source_file"], "Image/ExifTool.pm")
            self.assertEqual(fact["source_sha256"], hashlib.sha256(self.core.read_bytes()).hexdigest())

    def test_canonical_table_dump_uses_isolated_contract_snapshot(self):
        standalone = self.dump()
        result = subprocess.run(
            ["/usr/bin/perl", str(TABLE_DUMP), str(self.lib), "Fixture"],
            check=True, text=True, capture_output=True,
        )
        table_dump = json.loads(result.stdout)
        contract = table_dump["native_reader_contracts"]["unsigned16"]
        self.assertTrue(contract["resolved"])
        self.assertNotIn("reason", contract)
        self.assertEqual(contract["loaded_functions"], standalone["loaded_functions"])
        self.assertEqual(contract["observations"], standalone["observations"])
        self.assertEqual(contract["get16u_probe"], standalone["get16u_probe"])
        self.assertEqual(standalone["builtin_overrides"], {
            "unpack": False,
            "pack": False,
        })
        self.assertEqual(contract["builtin_overrides"], standalone["builtin_overrides"])
        self.assertEqual(contract["builtin_override_facts"], standalone["builtin_override_facts"])
        self.assertEqual(contract["isolated_functions"], standalone["loaded_functions"])
        expected_names = {
            "get16u": "Image::ExifTool::Get16u",
            "do_unpack_std": "Image::ExifTool::DoUnpackStd",
            "set_byte_order": "Image::ExifTool::SetByteOrder",
            "get_byte_order": "Image::ExifTool::GetByteOrder",
        }
        for name, fact in contract["loaded_functions"].items():
            self.assertTrue(fact["resolved"])
            self.assertEqual(fact["__name"], expected_names[name])
        self.assertIn("Fixture", table_dump["modules"])

    def test_parent_loaded_builtin_override_blocks_contract(self):
        self.fixture.write_text(textwrap.dedent("""\
            package Image::ExifTool::Fixture;
            BEGIN { *CORE::GLOBAL::unpack = sub { return 10; }; }
            our %Main = (1 => { Name => 'One' });
            1;
        """), encoding="utf-8")
        result = subprocess.run(
            ["/usr/bin/perl", str(TABLE_DUMP), str(self.lib), "Fixture"],
            check=True, text=True, capture_output=True,
        )
        contract = json.loads(result.stdout)["native_reader_contracts"]["unsigned16"]
        self.assertFalse(contract["resolved"])
        self.assertEqual(contract["reason"], "builtin_override_present")
        self.assertTrue(contract["builtin_overrides"]["unpack"])
        self.assertEqual(
            contract["builtin_override_facts"]["unpack"]["function"]["source_file"],
            "Image/ExifTool/Fixture.pm",
        )

    def test_template_only_source_edit_changes_live_ii_observation(self):
        before = self.dump()
        self.write_core("q")
        after = self.dump()
        self.assertEqual(
            before["loaded_functions"]["get16u"]["__deparse"],
            after["loaded_functions"]["get16u"]["__deparse"],
        )
        self.assertEqual(
            before["loaded_functions"]["do_unpack_std"]["__deparse"],
            after["loaded_functions"]["do_unpack_std"]["__deparse"],
        )
        self.assertEqual(before["observations"]["orders"]["MM"]["unpack_std_s"], "n")
        self.assertEqual(before["observations"]["orders"]["II"]["unpack_std_s"], "v")
        self.assertEqual(after["observations"]["orders"]["II"]["unpack_std_s"], "q")


if __name__ == "__main__":
    unittest.main()
