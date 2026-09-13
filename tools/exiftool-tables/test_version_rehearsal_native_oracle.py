#!/usr/bin/env python3
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parent))
import version_rehearsal_native_oracle as o

class T(unittest.TestCase):
    def test_rejects_argv_escape_and_fake_write(self):
        base={'name':'safe','fixture':'x','read':{'query':'FileType','expectation':'value','value':'JPEG'},'write':{'operation':'set','tag':'Comment','value':'x','readback':'x'}}
        for bad in ({**base,'name':'../x'}, {**base,'read':{'query':'../x','expectation':'value','value':'x'}}, {**base,'write':{'operation':'set','tag':'Comment','value':'a\0b'}}):
            with self.assertRaises(o.Refused): o._case(bad)
        for bad in ({**base,'write':{'operation':'set','tag':'Comment','value':'x'}}, {**base,'write':{'operation':'delete','tag':'Comment','readback':'retained'}}, {**base,'write':{'operation':'set','tag':'Comment','value':'x\ny','readback':'x\ny'}}):
            with self.assertRaises(o.Refused): o._case(bad)
    def test_old_read_only_write_shape_is_refused(self):
        old={'name':'read-only','fixture':'x','read':{'args':['-j'],'expectation':'success'},'write':{'args':['-j'],'expectation':'success'}}
        with self.assertRaises(o.Refused): o._case(old)
    def test_missing_fixture_and_stale_source_paths_refuse(self):
        with self.assertRaisesRegex(o.Refused,'fixture'): o._regular(Path('/definitely-missing-oxidex-fixture'),'case fixture')
        with self.assertRaisesRegex(o.Refused,'program'): o._regular(Path('/definitely-missing-oxidex-program'),'materialized ExifTool program')
        with TemporaryDirectory() as d:
            target=Path(d)/'target'; target.write_text('x'); link=Path(d)/'link'; link.symlink_to(target)
            with self.assertRaisesRegex(o.Refused,'symbolic link'): o._regular(link,'fixture')
    def test_timeout_and_spawn_recorded(self):
        self.assertEqual(o._run(['x'],lambda *a,**k: (_ for _ in ()).throw(subprocess.TimeoutExpired(['x'],1)))['state'],'timeout')
        self.assertEqual(o._run(['x'],lambda *a,**k: (_ for _ in ()).throw(OSError('no')))['state'],'spawn_failed')
        self.assertNotIn('PERLLIB',o._env())
    def test_version_mismatch_control(self):
        record=o._run(['perl','-ver'],lambda argv,**k: subprocess.CompletedProcess(argv,0,'12.63\n',''))
        self.assertNotEqual(record['stdout'].strip(),'12.64')
    def test_no_overwrite(self):
        with TemporaryDirectory() as d:
            p=Path(d)/'x'; payload={'schema':o.SCHEMA,'kind':o.KIND}; r={**payload,'probe_sha256':o.catalog_stage.sha256_json(payload)}; o.write_probe_report(p,r)
            with self.assertRaises(o.Refused): o.write_probe_report(p,r)
if __name__=='__main__': unittest.main()
