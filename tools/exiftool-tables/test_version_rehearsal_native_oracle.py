#!/usr/bin/env python3
from pathlib import Path
import subprocess,sys
from tempfile import TemporaryDirectory
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parent)); import version_rehearsal_native_oracle as o
class T(unittest.TestCase):
 def test_rejects_argv_escape_and_fake_write(self):
  base={'name':'safe','fixture':'x','read':{'query':'FileType','expectation':'value','value':'JPEG'},'write':{'operation':'set','tag':'Comment','value':'x','readback':'x'}}
  for bad in ({**base,'name':'../x'},{**base,'read':{'query':'../x','expectation':'value','value':'x'}},{**base,'write':{'operation':'set','tag':'Comment','value':'a\0b'}}):
   with self.assertRaises(o.Refused): o._case(bad)
 def test_timeout_and_spawn_recorded(self):
  self.assertEqual(o._run(['x'],lambda *a,**k: (_ for _ in ()).throw(subprocess.TimeoutExpired(['x'],1)))['state'],'timeout')
  self.assertEqual(o._run(['x'],lambda *a,**k: (_ for _ in ()).throw(OSError('no')))['state'],'spawn_failed')
 def test_no_overwrite(self):
  with TemporaryDirectory() as d:
   p=Path(d)/'x'; payload={'schema':o.SCHEMA,'kind':o.KIND}; r={**payload,'probe_sha256':o.catalog_stage.sha256_json(payload)}; o.write_probe_report(p,r)
   with self.assertRaises(o.Refused): o.write_probe_report(p,r)
if __name__=='__main__': unittest.main()
