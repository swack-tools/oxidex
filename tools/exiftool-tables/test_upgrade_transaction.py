"""Exercise the real bump orchestration in tiny Git repos with synthetic tools.

These controls prove transaction integrity, never ExifTool or parser coverage.
The complete production artifact manifest is copied, not redefined by tests.
"""
from __future__ import annotations

import types
import json
import os
from pathlib import Path
import shutil
import signal
import time
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import artifacts
import upgrade_transaction as tx

HERE = Path(__file__).resolve().parent


def write(path, text, executable=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    if executable:
        path.chmod(0o755)


class UpgradeTransactionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="upgrade-control-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name).resolve()
        self.root = self.base / "caller"
        self.root.mkdir()
        self.tools = self.root / "tools/exiftool-tables"
        self.tools.mkdir(parents=True)
        for name in ("artifacts.py", "upgrade_transaction.py", "process_groups.py", "bump-exiftool.sh", "bump_conformance_gate.py"):
            shutil.copy2(HERE / name, self.tools / name)
        for a in artifacts.select():
            write(self.root / a.path, f"committed-13.59|{a.key}\n")
        write(self.root / tx.PIN, "13.59\n")
        write(self.root / "handwritten.rs", "original\n")
        write(self.root / ".gitignore", "target/\nignored.tmp\n")
        write(self.root / "tests/fixtures/example.jpg", "synthetic corpus control\n")
        self.bin = self.base / "bin"
        self.perl = self.bin / "chosen-perl"
        write(self.perl, f"#!{sys.executable}\n" + '''import json,sys,pathlib
lib=pathlib.Path(sys.argv[-1]); print(json.dumps({'version':(lib/'version').read_text().strip()}))
''', True)
        write(self.tools / "dump_tables.pl", "# synthetic dump invoked by chosen-perl\n")
        write(self.root / "scripts/exiftool_oracle.py", '''from pathlib import Path
import os
class Oracle:
    def __init__(self, tree):
        self.version=(Path(tree)/'lib/version').read_text().strip()
        self.missing_modules=[]
    def provenance(self): return 'synthetic capability control'
    def check_container_support(self,p): assert Path(p).read_text()=='synthetic DOCX control'
def resolve_tree(tree): return Oracle(tree)
def choose_perl(): return os.environ.get('EXIFTOOL_PERL')
''')
        write(self.tools / "regen-all.sh", '''#!/usr/bin/env bash
set -euo pipefail
exec python3 "$(dirname "$0")/stub_regen.py"
''', True)
        write(self.tools / "stub_regen.py", '''import json,os,pathlib,shutil,sys
sys.path.insert(0,str(pathlib.Path(__file__).parent));import artifacts
root=pathlib.Path.cwd(); version=(root/'.exiftool-version').read_text().strip()
assert (pathlib.Path(os.environ['OXIDEX_EXIFTOOL_LIB'])/'version').read_text().strip()==version
assert pathlib.Path(shutil.which('perl')).resolve()==pathlib.Path(os.environ['EXIFTOOL_PERL']).resolve()
assert not os.environ.get('PERL5OPT') and not os.environ.get('OXIDEX_ALLOW_EXIFTOOL_SKEW')
label=root.parent.name; fail=os.environ.get('TX_FAIL','')
# Real verify_exprs builds an oracle during regeneration, before oxidex build.
pathlib.Path(os.environ['CARGO_TARGET_DIR']).mkdir(parents=True,exist_ok=True)
for a in artifacts.select():
    (root/a.path).write_text(version+'|'+a.key+'\\n')
    if fail=='generate-'+label and a.tier==1: sys.exit(7)
if fail=='undeclared': (root/'surprise.rs').write_text('oops')
if fail=='ignored-write': (root/'ignored.tmp').write_text('oops')
if fail=='deleted-output': (root/artifacts.select()[0].path).unlink()
if fail=='symlink-output':
    p=root/artifacts.select()[0].path;p.unlink();p.symlink_to(root/'handwritten.rs')
if fail=='changed-mode': (root/artifacts.select()[0].path).chmod(0o755)
if fail=='wrong-dump': (pathlib.Path(os.environ['OXIDEX_ET_CACHE'])/('tables-'+version+'.json')).write_text('{}')
''')
        write(self.tools / "verify.py", '''import os,pathlib,sys
if os.environ.get('TX_FAIL')=='verify-'+pathlib.Path.cwd().parent.name: sys.exit(8)
print('synthetic independent verifier success')
''')
        write(self.tools / "triage_bump.py", '''import json,os,pathlib,sys
if os.environ.get('TX_FAIL')=='triage': sys.exit(9)
for flag,content in [('--json-out',json.dumps({'old_version':json.loads(pathlib.Path(sys.argv[1]).read_text())['version'],'new_version':json.loads(pathlib.Path(sys.argv[2]).read_text())['version'],'total':1,'deltas':[{'bucket':'AUTO'}],'counts':{'AUTO':1,'EXPR':0,'COND':0,'HAND':0}})),('--markdown-out','triage control')]:
    pathlib.Path(sys.argv[sys.argv.index(flag)+1]).write_text(content)
''')
        write(self.bin / "cargo", f"#!{sys.executable}\n" + '''import json,os,pathlib,sys
if '--version' in sys.argv: print('cargo synthetic transaction control');sys.exit(0)
root=pathlib.Path.cwd(); sys.path.insert(0,str(root/'tools/exiftool-tables'));import artifacts
label=root.parent.name; fail=os.environ.get('TX_FAIL','')
if fail=='build-'+label: sys.exit(10)
if fail=='hang-build':
    import subprocess,time
    child=subprocess.Popen([sys.executable,'-c',"import subprocess,sys,time; subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)']); time.sleep(60)"])
    pathlib.Path(os.environ['TX_CHILD']).write_text(str(child.pid));time.sleep(60)
versions={(root/a.path).read_text().strip().split('|')[0] for a in artifacts.select()}
assert all((root/a.path).read_text().strip().split('|')[1]==a.key for a in artifacts.select())
assert len(versions)==1, 'mixed-tier comparison binary'
version=versions.pop();target=pathlib.Path(sys.argv[sys.argv.index('--target-dir')+1]); assert str(target)==os.environ['CARGO_TARGET_DIR']
binary=target/'actual/custom/oxidex'; binary.parent.mkdir(parents=True)
binary.write_text('#!'+sys.executable+'\\nprint('+repr(version)+')\\n');binary.chmod(0o755)
if fail=='build-write': (root/'handwritten.rs').write_text('bad build script')
if fail=='wrong-binary': binary=pathlib.Path(os.environ['TX_STALE_BINARY'])
if fail=='missing-binary': sys.exit(0)
print(json.dumps({'reason':'compiler-artifact','target':{'name':'oxidex'},'executable':str(binary),'fresh':fail=='cached-binary'}))
''', True)
        write(self.tools / "conformance.py", '''import json,os,pathlib,subprocess,sys
binary=pathlib.Path(sys.argv[sys.argv.index('--oxidex')+1]);label='before' if '/before/' in str(binary) else 'after'
fail=os.environ.get('TX_FAIL',''); assert pathlib.Path.cwd().parent.name=='after'
assert '/sources/new' in sys.argv[sys.argv.index('--exiftool-dir')+1]
assert int(sys.argv[sys.argv.index('--min-files')+1])>=1
if fail=='conformance-'+label: sys.exit(11)
if fail=='alias-swap' and label=='after':
    corpus=pathlib.Path(sys.argv[1]);(corpus/'a.jpg').unlink();(corpus/'b.jpg').unlink();(corpus/'a.jpg').symlink_to('two.jpg');(corpus/'b.jpg').symlink_to('one.jpg')
if fail=='missing-result': sys.exit(0)
corpus=pathlib.Path(sys.argv[1]); key=str(sorted(p for p in corpus.rglob('*') if p.is_file() and p.suffix.lower() not in {'.sh','.md','.py','.json'})[0])
value=subprocess.check_output([str(binary)],text=True).strip()
pathlib.Path(os.environ['TX_EVIDENCE']+'-'+label).write_text(value)
if fail=='concurrent-file' and label=='after': pathlib.Path(os.environ['TX_CALLER'],'handwritten.rs').write_text('concurrent')
if fail=='concurrent-index' and label=='after':
    p=pathlib.Path(os.environ['TX_CALLER'],'handwritten.rs');p.write_text('staged');subprocess.run(['git','-C',os.environ['TX_CALLER'],'add','handwritten.rs'],check=True)
if fail=='corpus-change' and label=='after': (corpus/'example.jpg').write_text('changed')
if fail=='different-files' and label=='after': key+='different'
diff=[['Group:Tag',1,2,'numeric']] if fail=='gate' and label=='after' else []
doc={'per_file':{key:{'format':'JPEG','value_diff':diff,'missing':{},'extra':{}}},'per_format':{'JPEG':{'files':1,'matched':1000,'missing':0,'value_diff':0,'renames':0,'extra':0}}}
if fail=='empty-result': doc={}
if fail=='malformed-result': doc={'per_file':{key:{}},'per_format':{'JPEG':{}}}
if fail=='zero-tags': doc['per_format']['JPEG']['matched']=0
pathlib.Path(sys.argv[sys.argv.index('--json-out')+1]).write_text(json.dumps(doc))
''')
        for version in ("13.55", "13.59", "13.60"):
            tree = self.base / f"exiftool-{version}"
            write(tree / "lib/version", version + "\n")
            write(tree / "exiftool", "synthetic source control")
            write(tree / "t/images/OOXML.docx", "synthetic DOCX control")
        self.git("init", "-q", "-b", "codex/test")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "Transaction control")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        self.env = {**os.environ, "PATH": str(self.bin) + os.pathsep + os.environ['PATH'],
                    "GIT_OPTIONAL_LOCKS": "0", "PYTHONDONTWRITEBYTECODE": "1", "TX_CALLER": str(self.root),
                    "TX_EVIDENCE": str(self.base / 'binary-seen')}
        self.before = tx.entry(self.root)

    def git(self, *args):
        return subprocess.check_output(['git','-C',str(self.root),*args],stderr=subprocess.STDOUT,
                                       env={**os.environ,'GIT_OPTIONAL_LOCKS':'0'})

    def args(self, version="13.60", old="13.59", floor=True):
        return [version, '--report-dir',str(self.base/'reports'),'--old-exiftool-dir',str(self.base/f'exiftool-{old}'),
                '--new-exiftool-dir',str(self.base/f'exiftool-{version}'),'--perl',str(self.perl)]+(['--min-files','1'] if floor else [])

    def run_bump(self, extra=(), fail="", version="13.60", old="13.59", env=None, floor=True):
        return subprocess.run(['bash',str(self.tools/'bump-exiftool.sh'),*self.args(version,old,floor),*extra],
            cwd=self.root,env={**self.env,'TX_FAIL':fail,**(env or {})},text=True,capture_output=True)

    def report(self):
        pointer=json.loads((self.root/'.git/oxidex-upgrade.json').read_text())
        return json.loads(Path(pointer['journal']).read_text())

    def unchanged(self):
        self.assertEqual(tx.entry(self.root),self.before)

    def test_ordinary_before_is_committed_after_regenerates_every_manifest_output(self):
        result=self.run_bump(['--dry-run']);self.assertEqual(result.returncode,0,result.stderr)
        self.unchanged()
        self.assertEqual((self.base/'binary-seen-before').read_text(),'committed-13.59')
        self.assertEqual((self.base/'binary-seen-after').read_text(),'13.60')
        self.assertEqual(self.report()['phase'],'dry-run-passed')
        self.assertEqual(len(self.git('worktree','list','--porcelain').split(b'worktree '))-1,1)

    def test_retrospective_before_and_after_have_no_mixed_tiers(self):
        result=self.run_bump(['--dry-run','--from','13.55'],version='13.59',old='13.55')
        self.assertEqual(result.returncode,0,result.stderr);self.unchanged()
        self.assertEqual((self.base/'binary-seen-before').read_text(),'13.55')
        self.assertEqual((self.base/'binary-seen-after').read_text(),'13.59')
        self.assertEqual(self.report()['baseline'],'retrospective-all-tiers')

    def test_hostile_environment_and_stale_binary_cannot_redirect_build(self):
        stale=self.root/'target/debug/oxidex';write(stale,'stale',True)
        result=self.run_bump(['--dry-run'],env={'CARGO_TARGET_DIR':str(self.base/'hostile'),
            'OXIDEX_EXIFTOOL_LIB':str(self.base/'exiftool-13.55/lib'),'PERL5OPT':'hostile',
            'EXIFTOOL_PERL':'/not/a/perl','OXIDEX_ALLOW_EXIFTOOL_SKEW':'1','TX_STALE_BINARY':str(stale)})
        self.assertEqual(result.returncode,0,result.stderr);self.unchanged();self.assertEqual(stale.read_text(),'stale')

    def test_each_generation_build_verification_and_measurement_failure_preserves_caller(self):
        for fail in ('generate-before','generate-after','verify-before','verify-after','build-before','build-after',
                     'triage','conformance-before','conformance-after','gate','undeclared','ignored-write',
                     'deleted-output','symlink-output','wrong-dump','build-write','missing-binary','cached-binary',
                     'missing-result','empty-result','malformed-result','zero-tags','different-files'):
            with self.subTest(fail=fail):
                result=self.run_bump(['--dry-run','--from','13.55'],fail=fail,version='13.59',old='13.55')
                self.assertNotEqual(result.returncode,0,result.stdout);self.unchanged()
                expected = {
                    'undeclared':'generate-before','ignored-write':'generate-before',
                    'deleted-output':'generate-before','symlink-output':'generate-before','wrong-dump':'generate-before',
                    'build-write':'build-before','missing-binary':'build-before','cached-binary':'build-before',
                    'missing-result':'conformance-before','empty-result':'conformance-before',
                    'malformed-result':'conformance-before','zero-tags':'conformance-before','different-files':'conformance-after',
                }.get(fail,fail)
                self.assertEqual(self.report()['failed_stage'],expected,result.stderr)

    def test_wrong_cargo_executable_rejected(self):
        stale=self.root/'target/debug/oxidex';write(stale,'stale',True)
        result=self.run_bump(['--dry-run'],fail='wrong-binary',env={'TX_STALE_BINARY':str(stale)})
        self.assertNotEqual(result.returncode,0);self.assertIn('outside the explicit target',result.stderr);self.unchanged()

    def test_dirty_caller_refused_without_erasing_changes(self):
        write(self.root/'handwritten.rs','mine')
        start=tx.entry(self.root); result=self.run_bump(['--dry-run'])
        self.assertNotEqual(result.returncode,0);self.assertEqual(tx.entry(self.root),start)
        self.assertFalse((self.base/'reports').exists())

    def test_preexisting_ignored_content_preserved(self):
        write(self.root/'ignored.tmp','mine');self.before=tx.entry(self.root)
        result=self.run_bump(['--dry-run']);self.assertEqual(result.returncode,0,result.stderr);self.unchanged()

    def test_concurrent_file_or_index_change_blocks_live_promotion(self):
        for fail in ('concurrent-file','concurrent-index'):
            with self.subTest(fail=fail):
                result=self.run_bump(fail=fail);self.assertNotEqual(result.returncode,0)
                self.assertIn('caller source',result.stderr)
                self.assertEqual((self.root/tx.PIN).read_text(),'13.59\n')
                self.assertIn((self.root/'handwritten.rs').read_text(),('concurrent','staged'))
                self.git('reset','--hard','HEAD')
                self.before=tx.entry(self.root)

    def test_corpus_change_blocks_live_promotion(self):
        result=self.run_bump(fail='corpus-change');self.assertNotEqual(result.returncode,0)
        self.assertIn('corpus changed',result.stderr);self.assertEqual((self.root/tx.PIN).read_text(),'13.59\n')

    def test_skip_is_incomplete_and_cannot_promote(self):
        result=self.run_bump(['--skip-conformance']);self.assertNotEqual(result.returncode,0);self.unchanged()
        result=self.run_bump(['--dry-run','--skip-conformance']);self.assertEqual(result.returncode,2,result.stderr);self.unchanged()
        self.assertEqual(self.report()['phase'],'incomplete-conformance-skipped')
        self.assertFalse((self.base/'binary-seen-before').exists())

    def test_live_promotion_changes_only_full_manifest_files_and_pin_without_index_edits(self):
        result=self.run_bump(fail='changed-mode');self.assertEqual(result.returncode,0,result.stderr)
        after=tx.entry(self.root)
        self.assertEqual(after['index_bytes'],self.before['index_bytes'])
        self.assertEqual(after['identity'],self.before['identity'])
        changes={p for p in after['files'] if after['files'][p]!=self.before['files'][p]}
        self.assertEqual(changes,{a.path for a in artifacts.select()}|{tx.PIN})
        self.assertEqual((self.root/tx.PIN).read_text(),'13.60\n')
        self.assertEqual(self.report()['phase'],'promoted')
        for a in artifacts.select():self.assertEqual((self.root/a.path).read_text(),f'13.60|{a.key}\n')

    def prepared(self):
        result=self.run_bump(['--dry-run']);self.assertEqual(result.returncode,0,result.stderr)
        journal=self.report();run=Path(journal['run']);journal['after']={}
        for rel in journal['paths']:
            for side,source in (('before',self.root),('after',run/'after/repo')):
                dest=run/side/'payload'/rel;dest.parent.mkdir(parents=True,exist_ok=True)
                shutil.copy2(source/rel,dest)
                if side=='after':journal['after'][rel]=tx.file_record(source,rel)
        journal['phase']='promoting';journal['staging']=tx.staging_paths(run,journal['paths']);path=run/'transaction.json';tx.save(path,journal)
        return path,journal

    def test_interrupted_promotion_is_detected_then_recovered_including_modes(self):
        path,journal=self.prepared();rel=journal['paths'][0]
        tx.install(self.root,rel,Path(journal['run'])/'after/payload'/rel,journal['after'][rel])
        result=self.run_bump();self.assertNotEqual(result.returncode,0);self.assertIn('unfinished promotion',result.stderr)
        result=subprocess.run(['bash',str(self.tools/'bump-exiftool.sh'),'--recover'],env=self.env,capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr);self.unchanged()
        self.assertEqual(json.loads(path.read_text())['phase'],'recovered')

    def test_recovery_refuses_unrecognized_concurrent_content_before_any_restore(self):
        path,journal=self.prepared();rel=journal['paths'][0]
        tx.install(self.root,rel,Path(journal['run'])/'after/payload'/rel,journal['after'][rel])
        write(self.root/journal['paths'][1],'concurrent change')
        concurrent=tx.entry(self.root)
        with self.assertRaisesRegex(tx.Refused,'unrecognized concurrent'):
            tx.recover(self.root,path)
        self.assertEqual(tx.entry(self.root),concurrent)
        self.assertEqual(json.loads(path.read_text())['phase'],'recovery-blocked')

    def test_recovery_refuses_modified_backup_and_preserves_caller(self):
        path,journal=self.prepared();write(Path(journal['run'])/'before/payload'/journal['paths'][0],'corrupt')
        with self.assertRaisesRegex(tx.Refused,'payload changed'):tx.recover(self.root,path)
        self.unchanged()

    def test_corpus_alias_retarget_is_detected_even_when_real_file_set_is_equal(self):
        corpus=self.root/'tests/fixtures'
        write(corpus/'one.jpg','one');write(corpus/'two.jpg','two')
        (corpus/'a.jpg').symlink_to('one.jpg');(corpus/'b.jpg').symlink_to('two.jpg')
        self.git('add','.');self.git('commit','-qm','alias corpus')
        result=self.run_bump(fail='alias-swap')
        self.assertNotEqual(result.returncode,0);self.assertIn('corpus changed',result.stderr)
        self.assertEqual((self.root/tx.PIN).read_text(),'13.59\n')

    def test_signal_kills_command_process_group_and_preserves_caller(self):
        pidfile=self.base/'child-pid'
        proc=subprocess.Popen(['bash',str(self.tools/'bump-exiftool.sh'),*self.args(),'--dry-run'],
            cwd=self.root,env={**self.env,'TX_FAIL':'hang-build','TX_CHILD':str(pidfile)},
            stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True)
        try:
            deadline=time.monotonic()+15
            while not pidfile.exists() and proc.poll() is None and time.monotonic()<deadline:time.sleep(.02)
            self.assertTrue(pidfile.exists(),'build child did not start')
            child=int(pidfile.read_text())
            grandchildren=[]
            while not grandchildren and time.monotonic()<deadline:
                grandchildren=subprocess.run(['pgrep','-P',str(child)],text=True,capture_output=True).stdout.split()
                if not grandchildren:time.sleep(.02)
            self.assertTrue(grandchildren)
            proc.send_signal(signal.SIGTERM);_,err=proc.communicate(timeout=10)
            self.assertNotEqual(proc.returncode,0,err);self.unchanged()
            for pid in [str(child),*grandchildren]:
                status=subprocess.run(['ps','-o','stat=','-p',pid],text=True,capture_output=True).stdout.strip()
                self.assertTrue(not status or status.startswith('Z'),(pid,status))
        finally:
            if proc.poll() is None:
                proc.send_signal(signal.SIGTERM)
                proc.communicate(timeout=10)
            if proc.stderr:proc.stderr.close()

    def run_in_process(self):
        oracle=types.ModuleType('exiftool_oracle')
        exec(compile((self.root/'scripts/exiftool_oracle.py').read_text(),'oracle-control','exec'),oracle.__dict__)
        handlers={s:signal.getsignal(s) for s in (signal.SIGINT,signal.SIGTERM,signal.SIGHUP)}
        try:
            with patch.object(tx,'ROOT',self.root),patch.dict(os.environ,self.env,clear=True),patch.dict(sys.modules,{'exiftool_oracle':oracle}):
                return tx.main(self.args())
        finally:
            for signum,handler in handlers.items():signal.signal(signum,handler)

    def test_failure_halfway_through_actual_promotion_restores_caller(self):
        original=tx.install;count=0
        def fail_once(*args,**kwargs):
            nonlocal count
            original(*args,**kwargs);count+=1
            if count==4:raise tx.Refused('simulated promotion interruption')
        with patch.object(tx,'install',side_effect=fail_once):
            result=self.run_in_process()
        self.assertEqual(result,1);self.unchanged();self.assertEqual(self.report()['phase'],'recovered')

    def test_failure_writing_terminal_journal_still_recovers(self):
        original=tx.save
        def fail_terminal(path,value):
            if value.get('phase')=='promoted':raise tx.Refused('interrupted final journal write')
            return original(path,value)
        with patch.object(tx,'save',side_effect=fail_terminal):result=self.run_in_process()
        self.assertEqual(result,1);self.unchanged();self.assertEqual(self.report()['phase'],'recovered')

    def test_tampered_journal_paths_manifest_or_payload_root_refuse_before_writes(self):
        path,journal=self.prepared()
        for field,value in (('schema',99),('manifest','bad'),('paths',['../escape']),('run',str(self.base/'elsewhere'))):
            with self.subTest(field=field):
                changed={**journal,field:value};tx.save(path,changed)
                with self.assertRaisesRegex(tx.Refused,'journal schema'):tx.recover(self.root,path)
                self.unchanged()
        tx.save(path,journal)

    def test_exact_journaled_complete_staging_file_recovers_partial_file_blocks(self):
        path,journal=self.prepared();rel=journal['paths'][0];staging=self.root/journal['staging'][rel]
        shutil.copy2(Path(journal['run'])/'after/payload'/rel,staging)
        tx.recover(self.root,path);self.unchanged();self.assertFalse(staging.exists())
        journal['phase']='promoting';tx.save(path,journal);write(staging,'partial')
        with self.assertRaisesRegex(tx.Refused,'partial or changed staging'):tx.recover(self.root,path)
        self.assertEqual(staging.read_text(),'partial')


    def test_report_comparable_population_does_not_reinterpret_raw_tag_floor(self):
        doc={'per_file':{'sample':{'format':'JPEG','value_diff':[],'missing':{},'extra':{}}},
             'per_format':{'JPEG':{'files':1,'matched':1,'missing':0,'value_diff':0,'renames':0,'extra':0}}}
        tx.validate_conformance(doc,min_files=1,min_tags=1000)


    def test_wrong_source_version_is_refused_at_probe_even_with_python_optimization(self):
        result=self.run_bump(['--dry-run','--new-exiftool-dir',str(self.base/'exiftool-13.55')],
                             env={'PYTHONOPTIMIZE':'1'})
        self.assertNotEqual(result.returncode,0)
        self.assertEqual(self.report()['failed_stage'],'probe-new')
        self.unchanged()


    def test_default_floor_excludes_scaffolding_and_deduplicates_aliases_and_roots(self):
        corpus=self.root/'tests/fixtures'
        for i in range(10):write(corpus/f'note-{i}.json','{}')
        for i in range(5):(corpus/f'alias-{i}.jpg').symlink_to('example.jpg')
        self.git('add','.');self.git('commit','-qm','corpus aliases and scaffolding')
        self.before=tx.entry(self.root)
        result=self.run_bump(['--dry-run','--corpus',str(corpus),'--corpus',str(corpus)],floor=False)
        self.assertEqual(result.returncode,0,result.stderr);self.unchanged()
        self.assertEqual(self.report()['corpus']['min_files'],1)
        self.assertEqual(len(self.report()['corpus']['selected']),1)
        result=self.run_bump(['--dry-run','--min-files','2'],floor=False)
        self.assertNotEqual(result.returncode,0);self.unchanged()


    def test_alternate_clean_index_cannot_hide_real_staged_caller_changes(self):
        path=self.root/'handwritten.rs'
        path.write_text('staged actual change');self.git('add','handwritten.rs');path.write_text('original\n')
        actual=tx.entry(self.root);alternate=self.base/'alternate-index'
        subprocess.run(['git','-C',str(self.root),'read-tree','HEAD'],check=True,
                       env={**self.env,'GIT_INDEX_FILE':str(alternate)})
        result=self.run_bump(['--dry-run'],env={'GIT_INDEX_FILE':str(alternate)})
        self.assertNotEqual(result.returncode,0)
        self.assertIn('caller must be clean',result.stderr)
        self.assertEqual(tx.entry(self.root),actual)
        self.assertFalse((self.base/'reports').exists())

    def test_git_repository_redirection_is_ignored_before_root_discovery(self):
        result=self.run_bump(['--dry-run'],env={'GIT_DIR':str(self.base/'nonexistent-git'),
            'GIT_WORK_TREE':str(self.base/'nonexistent-worktree'),'GIT_COMMON_DIR':str(self.base/'nonexistent-common'),
            'GIT_OBJECT_DIRECTORY':str(self.base/'nonexistent-objects')})
        self.assertEqual(result.returncode,0,result.stderr);self.unchanged()



class CommandJournalTests(unittest.TestCase):
    def test_cleanup_failure_preserves_actual_command_status_and_journal(self):
        for exit_code in (0, 7):
            with self.subTest(exit_code=exit_code), tempfile.TemporaryDirectory() as tmp:
                transaction = object.__new__(tx.Transaction)
                transaction.root = transaction.run = Path(tmp)
                transaction.report = Path(tmp) / 'transaction.json'
                transaction.doc = {'commands': []}
                transaction.env = os.environ.copy()
                with patch.object(tx, 'terminate_group', side_effect=PermissionError('injected cleanup denial')):
                    with self.assertRaises(tx.Refused) as error:
                        transaction.command('controlled-command', [sys.executable, '-c', f'raise SystemExit({exit_code})'])
                journal = json.loads(transaction.report.read_text())['commands'][0]
                self.assertEqual(journal['returncode'], exit_code)
                self.assertGreaterEqual(journal['finished'], journal['started'])
                self.assertIn('injected cleanup denial', journal['cleanup_error'])
                self.assertIn('cleanup', str(error.exception))
                if exit_code:
                    self.assertIn('failed (7)', str(error.exception))


if __name__=='__main__': unittest.main()
