#!/usr/bin/env python3
import gzip, io, json, os
from pathlib import Path
import subprocess, sys, tarfile
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import version_rehearsal_native_oracle as o
import test_version_rehearsal_catalog as c

def archive_bytes(label):
    output = io.BytesIO()
    with gzip.GzipFile(fileobj=output, mode='wb') as compressed:
        with tarfile.open(fileobj=compressed, mode='w') as archive:
            files = {
                'exiftool': label,
                'lib/Image/ExifTool.pm': f"$VERSION = '{label}';",
                'fixture.jpg': json.dumps({'FileType': 'JPEG', 'Comment': 'before'}),
                't/images/OOXML.docx': json.dumps({'FileType': 'DOCX'}),
            }
            for name, data in files.items():
                info = tarfile.TarInfo(f'x-{label}/{name}')
                encoded = data.encode()
                info.size = len(encoded)
                archive.addfile(info, io.BytesIO(encoded))
    return output.getvalue()


def make_state():
    capture = c.catalog_stage.capture_tag_catalog(
        c.FixtureGet(c.complete_responses()), '2026-09-13T00:00:00Z')
    catalog = o.rehearsal.normalize_catalog(c.catalog_stage.raw_catalog_from_capture(capture))
    plan = o.rehearsal.make_plan(catalog, 3, 0, 1, 'e' * 40)
    selected = {x['release']: x for pair in plan['pairs'] for x in (pair['old'], pair['new'])}
    responses = {
        c.catalog_stage.immutable_archive_url(release, row['peeled_commit']):
            c.response(archive_bytes(release))
        for release, row in selected.items()
    }
    temporary = TemporaryDirectory()
    root = Path(temporary.name)
    cache, sources = root / 'cache', root / 'sources'
    resolution = c.catalog_stage.resolve_selected_archives(
        plan, catalog, capture, c.FixtureGet(responses), cache)
    materialization = c.catalog_stage.materialize_selected_sources(
        plan, catalog, capture, resolution, cache, sources)
    return temporary, capture, catalog, plan, resolution, materialization, cache, sources, '13.59'


class T(unittest.TestCase):
 def state(self):
  return make_state()
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
 def test_capability_requires_exact_modules_and_docx(self):
  calls=[]
  def run(argv,**kw):
   calls.append(argv)
   if argv[-1]=='print $^V': output='v5.38.2'
   elif argv[-1]=='-ver': output='13.59\n'
   elif '-FileType' in argv: output='DOCX\n' if str(argv[-1]).endswith('.docx') else 'JPEG\n'
   elif any(item == '-Comment' for item in argv): output='x\n'
   else: output=''
   return subprocess.CompletedProcess(argv,0,output,'')
  report=self.invoke(run)
  self.assertEqual(report['state'],'ready')
  self.assertEqual(report['perl_capability']['required_modules'],
                   ['strict','warnings','Archive::Zip','Compress::Zlib'])
  self.assertEqual(report['docx_capability']['stdout'],'DOCX\n')
  self.assertTrue(any('-config' in argv and '' in argv and '-FileType' in argv for argv in calls))
 def test_public_stale_tree_refuses_before_oracle(self):
  with self.assertRaisesRegex(Exception,'verified archive'): self.invoke(lambda *a,**k: self.fail('oracle called'),mutate=True)
 def test_public_missing_fixture_refuses(self):
  def run(argv,**kw):
   output = ('v5.38.2' if argv[-1]=='print $^V' else
             ('13.59\n' if argv[-1]=='-ver' else ('DOCX\n' if '-FileType' in argv else '')))
   return subprocess.CompletedProcess(argv,0,output,'')
  with self.assertRaisesRegex(o.Refused,'fixture'): self.invoke(run,missing=True)
 def test_public_timeout_persists_report(self):
  def run(argv,**kw):
   if argv[-1]=='print $^V': return subprocess.CompletedProcess(argv,0,'v5.38.2','')
   if argv[-1]=='-ver': return subprocess.CompletedProcess(argv,0,'13.59\n','')
   if any(item.startswith('-M') for item in argv): return subprocess.CompletedProcess(argv,0,'','')
   if '-FileType' in argv and str(argv[-1]).endswith('.docx'): return subprocess.CompletedProcess(argv,0,'DOCX\n','')
   raise subprocess.TimeoutExpired(argv,1)
  report=self.invoke(run); self.assertEqual(report['state'],'failed'); self.assertEqual(report['cases'][0]['read']['state'],'timeout')
  with TemporaryDirectory() as d:
   out=Path(d)/'timeout.json'; o.write_probe_report(out,report); self.assertEqual(json.loads(out.read_text())['cases'][0]['read']['state'],'timeout')
 def test_public_spawn_persists_report(self):
  def run(argv,**kw):
   if argv[-1]=='print $^V': return subprocess.CompletedProcess(argv,0,'v5.38.2','')
   if argv[-1]=='-ver': return subprocess.CompletedProcess(argv,0,'13.59\n','')
   if any(item.startswith('-M') for item in argv): return subprocess.CompletedProcess(argv,0,'','')
   if '-FileType' in argv and str(argv[-1]).endswith('.docx'): return subprocess.CompletedProcess(argv,0,'DOCX\n','')
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
# A controlled command-line stand-in, not a native ExifTool compatibility test.
# Every subprocess goes through the production CLI and verified materializer.
NATIVE_STUB = r"""
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
with open(os.environ['OXIDEX_TEST_NATIVE_LOG'], 'a') as log:
    log.write(json.dumps(args) + '\n')
if any(item.startswith('-M') for item in args):
    raise SystemExit(0)
if args == ['-e', 'print $^V']:
    print('v5.38.2', end='')
    raise SystemExit(0)
program = Path(args[1])
assert args[2:4] == ['-config', ''], 'ambient ExifTool configuration must be disabled'
if args[-1] == '-ver':
    print(os.environ.get('OXIDEX_TEST_VERSION', program.read_text()))
    raise SystemExit(0)
target = Path(args[-1])
metadata = json.loads(target.read_text())
if '-overwrite_original' in args:
    tag, value = args[-2][1:].split('=', 1)
    if value:
        metadata[tag] = value
    else:
        metadata.pop(tag, None)
    target.write_text(json.dumps(metadata))
    print('1 image files updated')
else:
    value = metadata.get(args[-2][1:])
    if value is not None:
        print(value)
"""


class CliT(unittest.TestCase):
    def setUp(self):
        td, capture, catalog, plan, resolution, material, cache, sources, release = make_state()
        self.addCleanup(td.cleanup)
        self.root = Path(td.name)
        self.sources, self.material = sources, material
        row = next(x for x in material['selected_releases'] if x['release'] == release)
        self.source = sources / row['source_directory']
        self.fixture = self.source / 'fixture.jpg'
        self.original = self.fixture.read_bytes()
        self.output = self.root / 'report.json'
        self.log = self.root / 'native.jsonl'
        self.env = dict(os.environ, OXIDEX_TEST_NATIVE_LOG=str(self.log))
        self.env.pop('OXIDEX_TEST_VERSION', None)
        stub = self.root / 'native-stub'
        stub.write_text('#!' + str(Path(sys.executable).resolve()) + '\n' + NATIVE_STUB)
        stub.chmod(0o700)
        self.argv = [sys.executable, str(HERE / 'version_rehearsal_native_oracle.py')]
        for name, value in [('capture', capture), ('catalog', catalog), ('plan', plan),
                            ('resolution', resolution), ('materialization', material)]:
            path = self.root / (name + '.json')
            path.write_text(json.dumps(value))
            self.argv.extend(['--' + name, str(path)])
        self.cases = [self.case('set', value='café', readback='café'), self.case('delete')]
        self.manifest = self.root / 'cases.json'
        self.manifest.write_text(json.dumps(self.cases))
        for name, value in [('archive-cache', cache), ('source-root', sources),
                            ('release', release), ('perl', stub), ('cases', self.manifest),
                            ('output', self.output)]:
            self.argv.extend(['--' + name, str(value)])

    def case(self, operation, **write):
        return {'name': 'comment-' + operation, 'fixture': str(self.fixture),
                'read': {'query': 'FileType', 'expectation': 'value', 'value': 'JPEG'},
                'write': {'operation': operation, 'tag': 'Comment', **write}}

    def invoke(self):
        return subprocess.run(self.argv, env=self.env, capture_output=True, text=True, timeout=20)

    def assert_refused_before_native(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn('native readiness refused:', result.stderr)
        self.assertFalse(self.log.exists())
        self.assertFalse(self.output.exists())

    def test_cli_materialized_read_set_delete_and_write_once(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(self.output.read_text())
        self.assertEqual(report['state'], 'ready')
        self.assertEqual(len(report['cases']), 2)
        self.assertEqual(report['cases'][0]['readback']['stdout'], 'café\n')
        self.assertEqual(report['cases'][1]['readback']['stdout'], '')
        self.assertEqual(report['execution']['conformance'], 'unrun')
        self.assertEqual(self.fixture.read_bytes(), self.original)
        for case in report['cases']:
            self.assertNotEqual(case['copy_sha256_before'], case['copy_sha256_after'])
            self.assertNotEqual(case['write']['command'][-1], str(self.fixture))
        before_report, before_log = self.output.read_bytes(), self.log.read_bytes()
        again = self.invoke()
        self.assertEqual(again.returncode, 2)
        self.assertIn('output already exists', again.stderr)
        self.assertEqual(self.output.read_bytes(), before_report)
        self.assertEqual(self.log.read_bytes(), before_log)

    def test_cli_wrong_version_persists_failed_report_without_writes(self):
        self.env['OXIDEX_TEST_VERSION'] = '0.00'
        result = self.invoke()
        self.assertEqual(result.returncode, 2, result.stderr)
        report = json.loads(self.output.read_text())
        self.assertEqual(report['state'], 'failed')
        self.assertEqual(report['cases'], [])
        self.assertNotIn('-overwrite_original', self.log.read_text())

    def test_cli_missing_fixture(self):
        self.cases[0]['fixture'] = str(self.root / 'missing.jpg')
        self.manifest.write_text(json.dumps(self.cases))
        result = self.invoke()
        self.assertEqual(result.returncode, 2)
        self.assertIn('fixture', result.stderr)
        self.assertFalse(self.output.exists())
        self.assertNotIn('-overwrite_original', self.log.read_text())

    def test_cli_changed_plan(self):
        path = self.root / 'plan.json'
        plan = json.loads(path.read_text())
        plan['seed'] = 'changed'
        path.write_text(json.dumps(plan))
        self.assert_refused_before_native()

    def test_cli_rehashed_modified_source(self):
        (self.source / 'lib/Image/ExifTool.pm').write_text('changed')
        row = next(x for x in self.material['selected_releases'] if x['release'] == '13.59')
        row['tree'] = c.catalog_stage._tree_identity(self.source)
        self.material['materialization_sha256'] = c.catalog_stage.sha256_json(
            {k: v for k, v in self.material.items() if k != 'materialization_sha256'})
        (self.root / 'materialization.json').write_text(json.dumps(self.material))
        self.assert_refused_before_native()

    def test_cli_requires_case_array(self):
        self.manifest.write_text('{}')
        self.assert_refused_before_native()

    def test_cli_filesystem_actions_refuse_before_native(self):
        for tag in ('FileName', 'Directory', 'HardLink', 'SymLink', 'TestName',
                    'System:filename', 'File:System:SYMLINK'):
            with self.subTest(tag=tag):
                self.cases[0]['write']['tag'] = tag
                self.manifest.write_text(json.dumps(self.cases))
                self.assert_refused_before_native()

    def test_report_publication_race_keeps_winner(self):
        payload = {'schema': o.SCHEMA, 'kind': o.KIND}
        report = {**payload, 'probe_sha256': o.catalog_stage.sha256_json(payload)}
        real_link = os.link
        def racing_link(source, destination):
            Path(destination).write_bytes(b'other completed report')
            return real_link(source, destination)
        with patch.object(o.os, 'link', side_effect=racing_link):
            with self.assertRaisesRegex(o.Refused, 'output already exists'):
                o.write_probe_report(self.output, report)
        self.assertEqual(self.output.read_bytes(), b'other completed report')
        self.assertEqual(list(self.root.glob('.report.json.*')), [])

if __name__ == '__main__':
    unittest.main()
