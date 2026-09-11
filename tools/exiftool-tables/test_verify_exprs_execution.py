"""Exercise the real expression-oracle CLI with synthetic external processes.

These tests prove build/execution orchestration, protocol validation and output
preservation. They do not replace native Perl/Rust expression comparisons. No
real Cargo build, selected ExifTool installation or persistent repo edit runs.
When running this file outside tools/exiftool-tables, set
OXIDEX_EXECUTION_TEST_SOURCE_ROOT to the production repository being reviewed.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SOURCE_ROOT = Path(os.environ.get(
    "OXIDEX_EXECUTION_TEST_SOURCE_ROOT", Path(__file__).resolve().parents[2]
)).resolve()

STUB_COMMON = r'''
import json, os, pathlib, re, sys, time
def event(name, **values):
    with open(os.environ['CONTROL_EVENTS'], 'a') as stream:
        stream.write(json.dumps({'event':name, 'tz':os.environ.get('TZ'), **values})+'\n')
def emit(rows, mode):
    if mode == 'fail':
        print('controlled probe failure', file=sys.stderr)
        raise SystemExit(37)
    if mode == 'timeout':
        time.sleep(15)
    if mode == 'empty': rows = []
    elif mode == 'missing': rows = rows[1:]
    elif mode == 'duplicate': rows = rows + rows[:1]
    elif mode == 'unknown': rows = rows + [('J999999', '0')]
    elif mode == 'mismatch': rows = [(rows[0][0], '123456789')] + rows[1:]
    for jid, value in rows:
        print(jid+'\t'+value)
'''

PERL_STUB = STUB_COMMON + r'''
if sys.argv[1] == '-e':
    event('perl_capability')
    print('VERSION\t'+os.environ['CONTROL_PIN'])
    print('PERLVER\tv5.34.1')
    print('PROBE\t1/8')
    raise SystemExit(0)
script = pathlib.Path(sys.argv[1])
text = script.read_text()
# Evaluate only the fixture's explicit $val / 10 expression. Production main
# still supplies its actual probe population and generates both harnesses.
matches = re.findall(r'my \$val = ([-+0-9.e]+);\s*my \$r = eval \{ \$val / 10 \};\s*if \(\$@\) \{ print "(J\d+)\\tERROR', text)
assert len(matches) >= 20, ('unexpected Perl harness', text[:400])
rows = [(jid, format(float(value)/10, '.15g')) for value,jid in matches]
event('perl_probe', script=str(script), jobs=len(rows))
emit(rows, os.environ.get('CONTROL_PERL_MODE', 'ok'))
'''

CARGO_STUB = STUB_COMMON + r'''
args = sys.argv[1:]
assert args[0] == 'build', args
assert '--locked' in args, args
assert args[args.index('--jobs')+1] == '2', args
assert args[args.index('--bin')+1] == 'expr_oracle_harness', args
assert '--message-format=json-render-diagnostics' in args, args
root = pathlib.Path.cwd()
harness = root/'src/bin/expr_oracle_harness.rs'
assert harness.is_file()
matches = re.findall(r'out\.push_str.*?"(J\d+)\\t.*?perl_num\(\(([-+0-9.e]+)_f64\)', harness.read_text())
assert len(matches) >= 20, ('unexpected Rust harness', harness.read_text()[:400])
rows = [(jid, format(float(value)/10, '.15g')) for jid,value in matches]
target = pathlib.Path(os.environ['CARGO_TARGET_DIR'])
binary = target/'custom-build/location/reported-harness'
binary.parent.mkdir(parents=True, exist_ok=True)
event('cargo_build', argv=args, harness=str(harness), target=str(target), jobs=len(rows))
mode = os.environ.get('CONTROL_CARGO_MODE', 'ok')
if mode == 'slow': time.sleep(1.4)
if mode == 'timeout': time.sleep(15)
if mode == 'fail':
    print('controlled build failure', file=sys.stderr)
    raise SystemExit(31)
program = '#!'+sys.executable+'\n'+os.environ['CONTROL_STUB_COMMON']
program += '\nevent("rust_probe", executable=__file__, cwd=str(pathlib.Path.cwd()))\n'
program += 'emit('+repr(rows)+', os.environ.get("CONTROL_RUST_MODE", "ok"))\n'
binary.write_text(program)
binary.chmod(0o755)
artifact = {'reason':'compiler-artifact', 'target':{'name':'expr_oracle_harness',
            'kind':['bin'], 'src_path':str(harness)}, 'executable':str(binary),
            'fresh':os.environ.get('CONTROL_FRESH') == '1'}
if mode == 'missing-file': binary.unlink()
if mode == 'non-executable': binary.chmod(0o644)
if mode == 'directory':
    binary.unlink()
    binary.mkdir()
if mode == 'wrong-source': artifact['target']['src_path'] = str(root/'src/bin/neighbor.rs')
if mode == 'wrong-kind': artifact['target']['kind'] = ['example']
if mode == 'wrong-name': artifact['target']['name'] = 'neighbor'
if mode == 'null-executable': artifact['executable'] = None
# Real Cargo emits non-executable dependency and completion messages too.
print(json.dumps({'reason':'compiler-artifact','target':{'name':'dependency','kind':['lib'],
      'src_path':str(root/'src/lib.rs')},'executable':None,'fresh':True}))
if mode != 'no-artifact': print(json.dumps(artifact))
if mode == 'ambiguous': print(json.dumps(artifact))
print(json.dumps({'reason':'build-finished','success':True}))
'''


class VerifyExprsExecution(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='oxidex-expr-cli-')
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.root = self.base/'repository with spaces'
        self.tools = self.root/'tools/exiftool-tables'
        self.tools.mkdir(parents=True)
        (self.root/'scripts').mkdir()
        (self.root/'src/bin').mkdir(parents=True)
        for name in ('verify_exprs.py', 'exprs.py'):
            shutil.copy2(SOURCE_ROOT/'tools/exiftool-tables'/name, self.tools/name)
        shutil.copy2(SOURCE_ROOT/'scripts/instrument.py', self.root/'scripts/instrument.py')
        # Copy the same process wrapper imported by the production caller.
        self.copy_process_helper()
        self.pin = (SOURCE_ROOT/'.exiftool-version').read_text().strip()
        (self.root/'.exiftool-version').write_text(self.pin+'\n')
        (self.root/'.gitignore').write_text('__pycache__/\n')
        self.ledger = self.root/'existing-ledger.json'
        self.ledger.write_bytes(b'previous verified ledger\n')
        self.ledger.chmod(0o640)
        self.ledger_before = self.file_identity(self.ledger)
        self.harness = self.root/'src/bin/expr_oracle_harness.rs'
        self.dump = self.base/'tiny-tables.json'
        self.dump.write_text(json.dumps({'exiftool_version':self.pin,'modules':{
            'Control':{'tables':{'Main':{'meta':{},'tags':{'1':{
                'Name':'ControlValue','ValueConv':{'kind':'expr','expr':'$val / 10'}
            }}}}}
        }}))
        self.bin = self.base/'bin'
        self.bin.mkdir()
        for name, code in [('cargo', CARGO_STUB), ('chosen-perl', PERL_STUB)]:
            path = self.bin/name
            path.write_text('#!'+sys.executable+'\n'+code)
            path.chmod(0o755)
        self.events_path = self.base/'events.jsonl'
        self.target = self.base/'nondefault target'
        self.env = {k:v for k,v in os.environ.items()
                    if not k.startswith(('GIT_', 'PERL', 'OXIDEX_', 'EXIFTOOL_', 'CARGO_',
                                         'RUST', 'CONTROL_')) and k not in ('PYTHONPATH', 'PYTHONHOME')}
        self.env.update(PATH=str(self.bin)+os.pathsep+os.environ.get('PATH',''),
                        PYTHONDONTWRITEBYTECODE='1', OXIDEX_ALLOW_DIRTY_TREE='1',
                        CARGO_TARGET_DIR=str(self.target), CONTROL_PIN=self.pin,
                        CONTROL_EVENTS=str(self.events_path), CONTROL_STUB_COMMON=STUB_COMMON,
                        GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull)
        for args in (['init','-q','-b','codex/expression-cli-control'], ['add','.'],
                     ['-c','user.name=Test','-c','user.email=test@example.invalid','commit','-qm','fixture']):
            subprocess.run(['git','-c','core.hooksPath=/dev/null','-c','commit.gpgsign=false',
                            '-C',str(self.root),*args],env=self.env,check=True,capture_output=True)

    def copy_process_helper(self):
        """Copy the production helper without importing/mocking production main."""
        shutil.copy2(SOURCE_ROOT/'tools/exiftool-tables/process_groups.py',
                     self.tools/'process_groups.py')

    @staticmethod
    def file_identity(path):
        info = path.stat()
        return hashlib.sha256(path.read_bytes()).hexdigest(), info.st_mode & 0o777, info.st_mtime_ns

    def invoke(self, *, cargo='ok', rust='ok', perl='ok', timeout=2, build_timeout=5, extra_env=None):
        self.events_path.unlink(missing_ok=True)
        env = self.env | {'CONTROL_CARGO_MODE':cargo, 'CONTROL_RUST_MODE':rust,
                          'CONTROL_PERL_MODE':perl} | (extra_env or {})
        command = [sys.executable,str(self.tools/'verify_exprs.py'),str(self.dump),
                   '--perl',str(self.bin/'chosen-perl'),'--et-lib',str(self.base/'selected lib'),
                   '--timeout',str(timeout),'--build-timeout',str(build_timeout),
                   '--sample-lines','0','--ledger-out',str(self.ledger)]
        result = subprocess.run(command,cwd=self.root,env=env,text=True,capture_output=True,timeout=25)
        events = [json.loads(line) for line in self.events_path.read_text().splitlines()] if self.events_path.exists() else []
        for event in events:
            if event['event'] == 'perl_probe':
                self.assertFalse(Path(event['script']).exists(),'temporary Perl harness survived')
        return result, events

    def assert_failed_preserving(self, result, *, owned_harness=True):
        self.assertNotEqual(result.returncode,0,result.stdout+result.stderr)
        self.assertEqual(self.file_identity(self.ledger),self.ledger_before)
        if owned_harness:
            self.assertFalse(os.path.lexists(self.harness),'owned harness survived failure')
        self.assertNotIn('RESULT: PASS',result.stdout)

    def assert_passed(self, result, events):
        self.assertEqual(result.returncode,0,result.stdout+result.stderr)
        self.assertFalse(os.path.lexists(self.harness))
        ledger = json.loads(self.ledger.read_text())
        self.assertEqual(ledger['verified_expressions'],['$val / 10'])
        self.assertEqual(ledger['expression_counts']['verified'],1)
        self.assertGreaterEqual(ledger['probe_counts']['pass'],20)
        self.assertEqual(ledger['probe_counts']['fail'],0)
        self.assertEqual(ledger['probe_counts']['skip'],0)
        names = [e['event'] for e in events]
        self.assertEqual(names,['perl_capability','perl_probe','cargo_build','rust_probe'])
        self.assertEqual(ledger['probe_counts']['pass'],events[1]['jobs'])
        self.assertEqual(events[1]['jobs'],events[2]['jobs'])
        self.assertEqual(events[3]['executable'],str(self.target/'custom-build/location/reported-harness'))
        self.assertEqual(events[3]['cwd'],str(self.root))
        self.assertTrue(all(e['tz']=='UTC' for e in events if e['event']!='perl_capability'))
        self.assertEqual(events[2]['target'],str(self.target))

    def test_reported_nondefault_executable_runs_and_warm_build_is_allowed(self):
        result,events = self.invoke(extra_env={'CONTROL_FRESH':'1'})
        self.assert_passed(result,events)

    def test_build_has_its_own_longer_budget(self):
        result,events = self.invoke(cargo='slow',timeout=1,build_timeout=5)
        self.assert_passed(result,events)

    def test_build_failure_preserves_ledger_and_cleans_harness(self):
        result,events = self.invoke(cargo='fail')
        self.assert_failed_preserving(result)
        self.assertIn('cargo_build',[e['event'] for e in events])
        self.assertNotIn('rust_probe',[e['event'] for e in events])
        self.assertIn('controlled build failure',result.stderr)

    def test_build_timeout_preserves_ledger_and_cleans_harness(self):
        result,events = self.invoke(cargo='timeout',build_timeout=1)
        self.assert_failed_preserving(result)
        self.assertIn('cargo_build',[e['event'] for e in events])
        self.assertNotIn('rust_probe',[e['event'] for e in events])
        self.assertIn('timed out after 1 seconds',result.stderr)

    def test_probe_failure_preserves_ledger_and_cleans_harness(self):
        result,events = self.invoke(rust='fail')
        self.assert_failed_preserving(result)
        self.assertIn('rust_probe',[e['event'] for e in events])
        self.assertIn('controlled probe failure',result.stderr)

    def test_probe_timeout_keeps_its_short_budget(self):
        result,events = self.invoke(rust='timeout',timeout=1,build_timeout=5)
        self.assert_failed_preserving(result)
        self.assertIn('rust_probe',[e['event'] for e in events])
        self.assertIn('timed out after 1 seconds',result.stderr)

    def test_perl_failure_preserves_ledger_before_any_cargo_build(self):
        result,events = self.invoke(perl='fail')
        self.assert_failed_preserving(result)
        self.assertIn('perl_probe',[e['event'] for e in events])
        self.assertNotIn('cargo_build',[e['event'] for e in events])
        self.assertIn('controlled probe failure',result.stderr)

    def test_perl_timeout_preserves_ledger_before_any_cargo_build(self):
        result,events = self.invoke(perl='timeout',timeout=1)
        self.assert_failed_preserving(result)
        self.assertIn('perl_probe',[e['event'] for e in events])
        self.assertNotIn('cargo_build',[e['event'] for e in events])
        self.assertIn('timed out after 1 seconds',result.stderr)

    def test_cargo_artifact_population_and_identity_must_match(self):
        for mode in ('no-artifact','ambiguous','wrong-name','wrong-kind','wrong-source',
                     'null-executable','missing-file','non-executable','directory'):
            with self.subTest(mode=mode):
                result,events = self.invoke(cargo=mode)
                self.assert_failed_preserving(result)
                self.assertIn('cargo_build',[e['event'] for e in events])
                self.assertNotIn('rust_probe',[e['event'] for e in events])

    def test_rust_job_protocol_rejects_empty_missing_duplicate_unknown(self):
        for mode in ('empty','missing','duplicate','unknown'):
            with self.subTest(mode=mode):
                result,events = self.invoke(rust=mode)
                self.assert_failed_preserving(result)
                self.assertIn('rust_probe',[e['event'] for e in events])

    def test_perl_job_protocol_rejects_empty_missing_duplicate_unknown(self):
        for mode in ('empty','missing','duplicate','unknown'):
            with self.subTest(mode=mode):
                result,events = self.invoke(perl=mode)
                self.assert_failed_preserving(result)
                self.assertIn('perl_probe',[e['event'] for e in events])

    def test_both_empty_outputs_cannot_manufacture_a_pass(self):
        result,_ = self.invoke(perl='empty',rust='empty')
        self.assert_failed_preserving(result)

    def test_complete_but_disagreeing_probe_results_preserve_ledger(self):
        result,events = self.invoke(rust='mismatch')
        self.assert_failed_preserving(result)
        self.assertIn('rust_probe',[e['event'] for e in events])
        self.assertIn('RESULT: FAIL',result.stdout)

    def test_existing_harness_is_never_overwritten_or_deleted(self):
        self.harness.write_bytes(b'pre-existing owner source\n')
        self.harness.chmod(0o640)
        before = self.file_identity(self.harness)
        result,events = self.invoke()
        self.assert_failed_preserving(result,owned_harness=False)
        self.assertEqual(self.file_identity(self.harness),before)
        self.assertNotIn('cargo_build',[e['event'] for e in events])

    def test_dangling_harness_symlink_is_never_followed_or_deleted(self):
        target = self.base/'must-not-be-created.rs'
        self.harness.symlink_to(target)
        result,events = self.invoke()
        self.assert_failed_preserving(result,owned_harness=False)
        self.assertTrue(self.harness.is_symlink())
        self.assertEqual(os.readlink(self.harness),str(target))
        self.assertFalse(target.exists())
        self.assertNotIn('cargo_build',[e['event'] for e in events])

    def test_invalid_timeout_values_refuse_before_external_processes(self):
        for argument in ('timeout','build_timeout'):
            for value in (0,-1):
                with self.subTest(argument=argument,value=value):
                    result,events = self.invoke(**{argument:value})
                    self.assert_failed_preserving(result)
                    self.assertEqual(events,[])


if __name__ == '__main__':
    unittest.main()
