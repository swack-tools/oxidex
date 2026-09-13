#!/usr/bin/env python3
import gzip, io, json
from pathlib import Path
import subprocess, sys, tarfile
from tempfile import TemporaryDirectory
import unittest
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import version_rehearsal_native_oracle as o
import test_version_rehearsal_catalog as c

class T(unittest.TestCase):
 def tar(self,label):
  out=io.BytesIO()
  with gzip.GzipFile(fileobj=out,mode='wb') as z:
   with tarfile.open(fileobj=z,mode='w') as a:
    for name,data in [('exiftool','program'),('lib/Image/ExifTool.pm',"$VERSION = '13.59';"),('fixture.jpg','fixture')]:
     i=tarfile.TarInfo(f'x-{label}/{name}'); b=data.encode(); i.size=len(b); a.addfile(i,io.BytesIO(b))
  return out.getvalue()
 def state(self):
  capture=c.catalog_stage.capture_tag_catalog(c.FixtureGet(c.complete_responses()),'2026-09-13T00:00:00Z'); catalog=o.rehearsal.normalize_catalog(c.catalog_stage.raw_catalog_from_capture(capture)); plan=o.rehearsal.make_plan(catalog,3,0,1,'e'*40); selected={x['release']:x for p in plan['pairs'] for x in (p['old'],p['new'])}; responses={c.catalog_stage.immutable_archive_url(r,x['peeled_commit']):c.response(self.tar(r)) for r,x in selected.items()}
  td=TemporaryDirectory(); root=Path(td.name); cache=root/'cache'; sources=root/'sources'; resolution=c.catalog_stage.resolve_selected_archives(plan,catalog,capture,c.FixtureGet(responses),cache); material=c.catalog_stage.materialize_selected_sources(plan,catalog,capture,resolution,cache,sources); return td,capture,catalog,plan,resolution,material,cache,sources,'13.59'
 def invoke(self,runner,*,missing=False,mutate=False):
  td,cap,cat,plan,res,mat,cache,sources,rel=self.state(); self.addCleanup(td.cleanup); row=next(x for x in mat['selected_releases'] if x['release']==rel); fixture=sources/row['source_directory']/('missing.jpg' if missing else 'fixture.jpg')
  if mutate:
   p=sources/row['source_directory']/'lib/Image/ExifTool.pm'; p.write_text('changed'); row['tree']=c.catalog_stage._tree_identity(sources/row['source_directory']); mat['materialization_sha256']=c.catalog_stage.sha256_json({k:v for k,v in mat.items() if k!='materialization_sha256'})
  return o.probe_materialized_native(mat,plan,cat,cap,res,cache,sources,rel,Path(sys.executable).resolve(),[{'name':'safe','fixture':fixture,'read':{'query':'FileType','expectation':'value','value':'JPEG'},'write':{'operation':'set','tag':'Comment','value':'x','readback':'x'}}],run=runner)
 def test_public_version_mismatch_stops_cases(self):
  calls=[]
  def run(argv,**kw):
   calls.append(argv); return subprocess.CompletedProcess(argv,0,'0.00\n','')
  report=self.invoke(run); self.assertEqual(report['state'],'failed'); self.assertEqual(report['cases'],[]); self.assertTrue(any(x[-1]=='-ver' for x in calls)); self.assertFalse(any('-Comment=x' in x for x in calls))
 def test_public_stale_tree_refuses_before_oracle(self):
  with self.assertRaisesRegex(Exception,'verified archive'): self.invoke(lambda *a,**k: self.fail('oracle called'),mutate=True)
 def test_public_missing_fixture_refuses(self):
  def run(argv,**kw): return subprocess.CompletedProcess(argv,0,'13.59\n' if argv[-1]=='-ver' else '', '')
  with self.assertRaisesRegex(o.Refused,'fixture'): self.invoke(run,missing=True)
 def test_public_timeout_persists_report(self):
  def run(argv,**kw):
   if argv[-1]=='-ver': return subprocess.CompletedProcess(argv,0,'13.59\n','')
   if '-MArchive::Zip' in argv: return subprocess.CompletedProcess(argv,0,'','')
   raise subprocess.TimeoutExpired(argv,1)
  report=self.invoke(run); self.assertEqual(report['state'],'failed'); self.assertEqual(report['cases'][0]['read']['state'],'timeout')
  with TemporaryDirectory() as d:
   out=Path(d)/'timeout.json'; o.write_probe_report(out,report); self.assertEqual(json.loads(out.read_text())['cases'][0]['read']['state'],'timeout')
 def test_public_spawn_persists_report(self):
  def run(argv,**kw):
   if argv[-1]=='-ver': return subprocess.CompletedProcess(argv,0,'13.59\n','')
   if '-MArchive::Zip' in argv: return subprocess.CompletedProcess(argv,0,'','')
   raise OSError('gone')
  report=self.invoke(run); self.assertEqual(report['state'],'failed'); self.assertEqual(report['cases'][0]['read']['state'],'spawn_failed')
  with TemporaryDirectory() as d:
   out=Path(d)/'spawn.json'; o.write_probe_report(out,report); self.assertEqual(json.loads(out.read_text())['cases'][0]['read']['state'],'spawn_failed')
 def test_structured_case_refusals(self):
  base={'name':'safe','fixture':'x','read':{'query':'FileType','expectation':'value','value':'JPEG'},'write':{'operation':'set','tag':'Comment','value':'x','readback':'x'}}
  bad=({**base,'name':'../escape'},{**base,'read':{'query':'../other','expectation':'value','value':'x'}},{**base,'write':{'operation':'set','tag':'Comment','value':'x'}},{**base,'write':{'operation':'delete','tag':'Comment','readback':'retained'}},{**base,'write':{'operation':'set','tag':'Comment','value':'a\0b','readback':'a\0b'}},{**base,'write':{'operation':'set','tag':'Comment','value':'a\nb','readback':'a\nb'}})
  for case in bad:
   with self.assertRaises(o.Refused): o._case(case)
 def test_symlink_and_report_overwrite_refuse(self):
  with TemporaryDirectory() as d:
   root=Path(d); target=root/'target'; target.write_text('x'); link=root/'link'; link.symlink_to(target)
   with self.assertRaises(o.Refused): o._regular(link,'fixture')
   payload={'schema':o.SCHEMA,'kind':o.KIND}; report={**payload,'probe_sha256':o.catalog_stage.sha256_json(payload)}; out=root/'report.json'; o.write_probe_report(out,report)
   with self.assertRaises(o.Refused): o.write_probe_report(out,report)
 def test_old_read_only_shape_refuses(self):
  with self.assertRaises(o.Refused): o._case({'name':'x','fixture':'x','read':{'args':['-j'],'expectation':'success'},'write':{'args':['-j'],'expectation':'success'}})
class CliT(T):
    def test_cli_requires_case_array_and_write_once_output(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            cases = root / 'cases.json'
            cases.write_text('{}')
            output = root / 'output.json'
            result = subprocess.run([
                str(Path(sys.executable).resolve()), str(HERE / 'version_rehearsal_native_oracle.py'),
                '--capture', str(root/'missing'), '--catalog', str(root/'missing'), '--plan', str(root/'missing'),
                '--resolution', str(root/'missing'), '--materialization', str(root/'missing'),
                '--archive-cache', str(root/'cache'), '--source-root', str(root/'sources'), '--release', '13.59',
                '--perl', str(Path(sys.executable).resolve()), '--cases', str(cases), '--output', str(output),
            ], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn('native readiness refused:', result.stderr)
            self.assertFalse(output.exists())

if __name__ == '__main__':
    unittest.main()
