"""Native table instruments must not inherit a user's .ExifTool_config."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
DUMP = REPO_ROOT / "tools/exiftool-tables/dump_tables.pl"
ORACLE = REPO_ROOT / "tools/exiftool-tables/oracle.pl"
READER_CONTRACT = REPO_ROOT / "tools/exiftool-tables/dump_binary_reader_contract.pl"
PERL = os.environ.get("EXIFTOOL_PERL", "/usr/bin/perl")


class ExifToolConfigIsolation(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.home = root / "hostile-home"
        self.home.mkdir()
        self.lib = root / "lib"
        package = self.lib / "Image/ExifTool"
        package.mkdir(parents=True)
        (self.home / ".ExifTool_config").write_text(
            "package Image::ExifTool; warn \"HOSTILE_CONFIG_EXECUTED\\n\"; $VERSION = 'HOSTILE_CONFIG'; 1;\n",
            encoding="utf-8",
        )
        (self.lib / "Image/ExifTool.pm").write_text(textwrap.dedent("""\
            package Image::ExifTool;
            use strict;
            our $VERSION = 'clean';
            our $configFile;
            our $currentByteOrder = 'II';
            our %unpackStd = ( S => 'v' );
            our %specialTags = map { $_ => 1 } qw(GROUPS FORMAT FIRST_ENTRY);
            sub SetByteOrder { $currentByteOrder = shift; $unpackStd{S} = $currentByteOrder eq 'II' ? 'v' : 'n'; return 1; }
            sub GetByteOrder { return $currentByteOrder; }
            sub Get16u { my ($data, $offset) = @_; return undef if length($$data) < $offset + 2; return unpack($unpackStd{S}, substr($$data, $offset, 2)); }
            sub DoUnpackStd { return Get16u(@_); }
            if (!defined $configFile) {
                my $file = ($ENV{EXIFTOOL_HOME} || $ENV{HOME} || '.') . '/.ExifTool_config';
                require $file if -r $file;
            } elsif (length $configFile) {
                require $configFile;
            }
            1;
        """), encoding="utf-8")
        (package / "Fixture.pm").write_text(textwrap.dedent("""\
            package Image::ExifTool::Fixture;
            our %Main = ( GROUPS => { 0 => 'EXIF' }, 1 => { Name => 'CleanTag' } );
            1;
        """), encoding="utf-8")

    def run_dump(self, script=DUMP):
        env = os.environ.copy()
        env["EXIFTOOL_HOME"] = str(self.home)
        result = subprocess.run([PERL, str(script), str(self.lib), "Fixture"],
                                check=True, text=True, capture_output=True, env=env)
        return json.loads(result.stdout)

    def run_instrument(self, script, *arguments):
        env = os.environ.copy()
        env["EXIFTOOL_HOME"] = str(self.home)
        return subprocess.run([PERL, str(script), str(self.lib), *arguments],
                              text=True, capture_output=True, env=env)

    def unfenced_control(self, instrument):
        directory = Path(tempfile.mkdtemp(prefix="unfenced-" + instrument.stem + "-", dir=self.tmp.name))
        control = directory / instrument.name
        fence = "BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }\n"
        text = instrument.read_text(encoding="utf-8")
        self.assertIn(fence, text)
        control.write_text(text.replace(fence, "", 1), encoding="utf-8")
        shutil.copytree(DUMP.parent / "OxiDex", directory / "OxiDex")
        return control

    def test_config_control_proves_the_dump_fence(self):
        # The copied control has only the canonical early config assignment
        # removed.  It proves the hostile home config is a real input, rather
        # than merely asserting a line of Perl text exists.
        control = self.unfenced_control(DUMP)
        self.assertEqual(self.run_dump(control)["exiftool_version"], "HOSTILE_CONFIG")
        self.assertEqual(self.run_dump()["exiftool_version"], "clean")

    def test_each_capture_instrument_blocks_hostile_config_at_load(self):
        for instrument, arguments in ((DUMP, ("Fixture",)), (ORACLE, ()), (READER_CONTRACT, ())):
            with self.subTest(instrument=instrument.name):
                fenced = self.run_instrument(instrument, *arguments)
                self.assertEqual(fenced.returncode, 0, fenced.stderr)
                self.assertNotIn("HOSTILE_CONFIG_EXECUTED", fenced.stdout + fenced.stderr)
                control = self.run_instrument(self.unfenced_control(instrument), *arguments)
                self.assertEqual(control.returncode, 0, control.stderr)
                self.assertIn("HOSTILE_CONFIG_EXECUTED", control.stderr)


if __name__ == "__main__":
    unittest.main()
