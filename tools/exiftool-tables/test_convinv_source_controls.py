"""Closed ConvInv token-template controls against a fresh canonical fact."""
import json, os, subprocess, unittest
from pathlib import Path
from convinv_recipes import RecipeRefused, compile_convinv

ROOT=Path(__file__).resolve().parents[2]
@unittest.skipUnless(os.environ.get('EXIFTOOL_PERL') and os.environ.get('OXIDEX_PINNED_EXIFTOOL'),'requires canonical capture')
class Controls(unittest.TestCase):
 def test_unmodelled_statements_and_operands_refuse(self):
  env={k:v for k,v in os.environ.items()if k not in {'PERL5LIB','PERLLIB','PERL5OPT'}};lib=Path(os.environ['OXIDEX_PINNED_EXIFTOOL']);lib=lib/'lib' if (lib/'lib').is_dir() else lib
  doc=json.loads(subprocess.run([os.environ['EXIFTOOL_PERL'],str(ROOT/'tools/exiftool-tables/dump_tables.pl'),str(lib),'Exif'],env=env,capture_output=True,check=True).stdout); fact=doc['native_write_helpers']['conv_inv'];body=fact['__deparse']
  edits=[('my($err, $type);','my($err, $type); ($val = \'corrupt\');'),('(return $val, $err);','(return $val, \'changed\');'),("'CHECK_PROC'","'OTHER_PROC'"),("'ValueConv'","'RawConv'"),("'RawConvInv'","'OtherGate'"),('"$err2 for ${wgrp1}:$tag"','"$err2 $val ${wgrp1}:$tag"'),('"$err2 for ${wgrp1}:$tag"','"$err2 \\n${wgrp1}:$tag"')]
  for before,after in edits:
   with self.subTest(after=after):
    changed=dict(fact);changed['__deparse']=body.replace(before,after,1)
    with self.assertRaises(RecipeRefused):compile_convinv(changed)
