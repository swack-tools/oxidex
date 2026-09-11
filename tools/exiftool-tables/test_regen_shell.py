"""Exercise real regeneration shells/manifest with synthetic leaf executables.

These controls prove source selection, invocation order and refusal propagation;
separate native producer checks prove the emitted dictionary facts. No Rust
build, downloads, persistent source writes, or second output manifest are used.
"""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SOURCE_ROOT = Path(__file__).resolve().parents[2]

LEAF = r'''
import json,os,pathlib,sys
mode=pathlib.Path(sys.argv[0]).name
args=sys.argv[1:]
if mode=='python3' and (args[0]=='-' or pathlib.Path(args[0]).name=='artifacts.py'):
    os.execv(sys.executable,[sys.executable,*args])
root=pathlib.Path(os.environ['CONTROL_ROOT'])
sys.path.insert(0,str(root/'tools/exiftool-tables'))
import artifacts
lib=pathlib.Path(os.environ['OXIDEX_EXIFTOOL_LIB']).resolve()
assert (lib/'marker').read_text()=='explicit-A'
name='rustfmt' if mode=='rustfmt' else pathlib.Path(args.pop(0)).name
key=name+(':check' if '--check' in args else '')
with open(os.environ['CONTROL_LOG'],'a') as log:
    log.write(json.dumps({'tool':key,'argv':args,'source':str(lib),
                          'target':os.environ.get('CARGO_TARGET_DIR')})+'\n')
def flag(key): return pathlib.Path(args[args.index(key)+1]).resolve()
def output(path,value): path.write_text('generated explicit-A '+value+'\n')
def dump(path): assert json.loads(pathlib.Path(path).read_text())['marker']=='explicit-A'
def artifact(producer):
    items=artifacts.select(producer=producer)
    assert len(items)==1
    return root/items[0].path
if mode=='rustfmt':
    paths={str(pathlib.Path(x).resolve()) for x in args if x.endswith('.rs')}
    assert paths in [{str(root/a.path) for a in artifacts.select(tier,kind='rust')} for tier in (1,2)]
elif mode=='chosen-perl':
    if name in ('dump_tables.pl','dump_filetypes.pl'):
        assert pathlib.Path(args[0]).resolve()==lib
        print(json.dumps({'exiftool_version':(root/'.exiftool-version').read_text().strip(),'marker':'explicit-A'}))
    elif name=='dump_af_points.pl':
        assert pathlib.Path(args[0]).resolve()==lib/'Image/ExifTool/Nikon.pm'
        pathlib.Path(args[1]).write_text(json.dumps({'marker':'explicit-A'}))
    elif name=='dump_lens_alternatives.pl':
        assert flag('--exiftool-dir')==lib.parent
        path=flag('--check' if '--check' in args else '--out')
        assert path==artifact('dump_lens_alternatives')
        if '--check' in args:
            assert path.read_text()=='generated explicit-A '+name+'\n'
        else: output(path,name)
    else:
        assert name.startswith('gen_'),name
        print('generated explicit-A '+name)
else:
    if name=='analyze.py': dump(args[0])
    elif name=='verify_exprs.py':
        dump(args[0]);assert flag('--et-lib')==lib
        assert flag('--perl')==pathlib.Path(os.environ['EXIFTOOL_PERL'])
        pathlib.Path(os.environ['CARGO_TARGET_DIR']).mkdir(parents=True,exist_ok=True)
        output(flag('--ledger-out'),'expr-ledger')
    elif name=='codegen.py':
        dump(args[0]);output(flag('-o'),'binary');output(flag('--ifd-out'),'ifd')
        output(flag('--value-conv-ledger-out'),'value-ledger')
    elif name in ('codegen_filetypes.py','codegen_fits.py','gen_sony_main_extra_tables.py','gen_minolta_a100_tables.py','gen_nikon_settings_tables.py'):
        dump(args[0]);output(flag('-o'),name)
    elif name=='codegen_composite.py':
        dump(args[0]);output(flag('-o'),'composite');output(flag('--generated-out'),'composite-compute')
    elif name=='codegen_subdirs.py':
        dump(args[0]);output(flag('-o'),name)
        if 'Pentax' in args: flag('-o').write_text('const PENTAX_CONV6: &[(i64, &str)] = &[\n];\n')
    elif name=='codegen_af_points.py': dump(args[0]);output(pathlib.Path(args[1]),'af-points')
    elif name=='splice_leica.py':
        assert 'explicit-A' in pathlib.Path(args[0]).read_text()
        output(pathlib.Path(args[1]),'leica')
    elif name=='generate_tables.py':
        assert pathlib.Path(args[0]).resolve()==lib/'Image/ExifTool/Charset'
        for item in artifacts.select(producer='generate_charsets'): output(root/item.path,item.key)
    elif name=='verify.py': assert pathlib.Path(args[1]).resolve()==lib
    elif name in ('gen_geotiff_printconv.py','gen_dicom_dict.py'):
        assert flag('--exiftool-dir')==lib.parent
        assert flag('--perl')==pathlib.Path(os.environ['EXIFTOOL_PERL'])
        assert flag('--out')==artifact(name[:-3])
        output(flag('--out'),name)
    elif name=='verify_lens_alternatives.py':
        assert flag('--exiftool-dir')==lib.parent
        assert flag('--perl')==pathlib.Path(os.environ['EXIFTOOL_PERL'])
        path=pathlib.Path(args[0]).resolve()
        assert path==artifact('dump_lens_alternatives')
        assert path.read_text()=='generated explicit-A dump_lens_alternatives.pl\n'
    elif name in ('verify_geotiff.py','verify_dicom_dict.py','verify_nikon_settings.py'):
        assert flag('--exiftool-dir')==lib.parent
        assert flag('--perl')==pathlib.Path(os.environ['EXIFTOOL_PERL'])
        producer={'verify_geotiff.py':'gen_geotiff_printconv','verify_dicom_dict.py':'gen_dicom_dict','verify_nikon_settings.py':'gen_nikon_settings_tables'}[name]
        path=flag('--rust-file' if name=='verify_geotiff.py' else '--input')
        assert path==artifact(producer)
        assert path.read_text()=='generated explicit-A '+producer+'.py\n'
    else: raise AssertionError('unexpected leaf '+name)
if os.environ.get('CONTROL_UNDECLARED')==key:
    (root/'handwritten.rs').write_text('unauthorized write\n')
if os.environ.get('CONTROL_FAIL')==key:
    print('injected leaf failure: '+key,file=sys.stderr)
    sys.exit(47)
'''


class RegenerationShellTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='oxidex regen controls ')
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name).resolve()
        self.root = self.base / 'repo'
        self.tools = self.root / 'tools/exiftool-tables'
        self.tools.mkdir(parents=True)
        for name in ('regen-all.sh', 'regen.sh', 'artifact-env.sh', 'artifacts.py'):
            shutil.copy2(SOURCE_ROOT / 'tools/exiftool-tables' / name, self.tools / name)
        spec = importlib.util.spec_from_file_location('shell_control_artifacts', self.tools / 'artifacts.py')
        self.manifest = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = self.manifest
        self.addCleanup(sys.modules.pop, spec.name, None)
        spec.loader.exec_module(self.manifest)
        for item in self.manifest.select():
            path = self.root / item.path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('initial ' + item.key + '\n')
        self.pin = (SOURCE_ROOT / '.exiftool-version').read_text().strip()
        (self.root / '.exiftool-version').write_text(self.pin + '\n')
        (self.root / 'rustfmt.toml').write_text('edition = "2024"\n')
        (self.root / '.gitignore').write_text('__pycache__/\n')
        self.source = self.base / 'explicit source A'
        for path, marker in ((self.source, 'explicit-A'), (self.base / 'cache source B', 'cached-B')):
            lib = path / 'lib'
            (lib / 'Image').mkdir(parents=True)
            (lib / 'Image/ExifTool.pm').write_text(f"$VERSION = '{self.pin}';\n")
            (lib / 'marker').write_text(marker)
        self.cache = self.base / 'cache'
        self.cache.mkdir()
        (self.cache / ('exiftool-' + self.pin)).symlink_to(self.base / 'cache source B')
        self.bin = self.base / 'bin'
        self.bin.mkdir()
        for name in ('python3', 'chosen-perl', 'rustfmt'):
            path = self.bin / name
            path.write_text('#!' + sys.executable + '\n' + LEAF)
            path.chmod(0o755)
        (self.bin / 'perl').write_text('#!/bin/sh\necho unexpected PATH Perl >&2\nexit 99\n')
        (self.bin / 'perl').chmod(0o755)
        self.log = self.base / 'calls.jsonl'
        (self.base / 'tmp').mkdir()
        self.env = {k: v for k, v in os.environ.items()
                    if not k.startswith(('GIT_', 'PERL', 'OXIDEX_', 'EXIFTOOL_'))}
        self.env.update({
            'PATH': str(self.bin) + os.pathsep + os.environ['PATH'],
            'EXIFTOOL_PERL': str(self.bin / 'chosen-perl'),
            'OXIDEX_EXIFTOOL_LIB': str(self.source / 'lib'),
            'OXIDEX_ET_CACHE': str(self.cache),
            'EXIFTOOL_CACHE_DIR': str(self.base / 'hostile-cache'),
            'CARGO_TARGET_DIR': str(self.base / 'oracle-target'),
            'CONTROL_ROOT': str(self.root), 'CONTROL_LOG': str(self.log),
            'PYTHONDONTWRITEBYTECODE': '1', 'TMPDIR': str(self.base / 'tmp'),
        })
        for args in (['init', '-q', '-b', 'codex/shell-control'],
                     ['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'add', '.'],
                     ['-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'fixture']):
            subprocess.run(['git', '-c', 'core.hooksPath=/dev/null', '-c', 'commit.gpgsign=false',
                            '-C', str(self.root), *args], env=self.env,
                           check=True, capture_output=True)

    def run_regeneration(self, *, full=False, env=None):
        self.log.unlink(missing_ok=True)
        (self.cache / f'tables-{self.pin}.json').write_text(json.dumps({'marker': 'cached-B'}))
        command = ['bash', str(self.tools / 'regen-all.sh')]
        if not full:
            command.append('--tier2-only')
        result = subprocess.run(command, cwd=self.root, env=self.env | (env or {}),
                                text=True, capture_output=True, timeout=45)
        calls = [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []
        return result, calls

    def extra_leaves(self):
        # This is the invocation contract, not another output-path manifest.
        return ['gen_geotiff_printconv.py', 'gen_dicom_dict.py',
                'dump_lens_alternatives.pl', 'verify_geotiff.py',
                'verify_dicom_dict.py', 'verify_lens_alternatives.py',
                'gen_nikon_settings_tables.py', 'verify_nikon_settings.py']

    def test_both_tiers_and_tier2_use_selected_source_and_complete_checks(self):
        for full in (True, False):
            with self.subTest(full=full):
                result, calls = self.run_regeneration(full=full)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                names = [c['tool'] for c in calls]
                for name in self.extra_leaves():
                    self.assertEqual(names.count(name), 1, names)
                self.assertEqual(names.count('verify_exprs.py'), int(full))
                self.assertEqual(names.count('rustfmt'), 2 if full else 1)
                format_calls = [c for c in calls if c['tool'] == 'rustfmt']
                for call, tier in zip(format_calls, (1, 2) if full else (2,)):
                    observed = {str(Path(arg).resolve()) for arg in call['argv'] if arg.endswith('.rs')}
                    expected = {str(self.root / a.path) for a in self.manifest.select(tier, kind='rust')}
                    self.assertEqual(observed, expected)
                self.assertTrue(all(c['source'] == str(self.source / 'lib') for c in calls))
                self.assertTrue(all(c['target'] == str(self.base / 'oracle-target') for c in calls))
                for name in (n for n in self.extra_leaves() if n.startswith('verify_')):
                    self.assertGreater(names.index(name), max(i for i, n in enumerate(names) if n == 'rustfmt'))
                self.assertEqual(json.loads((self.cache / f'tables-{self.pin}.json').read_text())['marker'], 'explicit-A')
                self.assertIn('regeneration write-set PASS', result.stdout)
                self.assertEqual('unexpected PATH Perl' in result.stderr, False)

    def test_each_new_producer_or_verifier_failure_survives_exit_guard(self):
        for leaf in self.extra_leaves():
            with self.subTest(leaf=leaf):
                result, calls = self.run_regeneration(env={'CONTROL_FAIL': leaf})
                self.assertEqual(result.returncode, 47, result.stdout + result.stderr)
                self.assertEqual(calls[-1]['tool'], leaf)
                self.assertIn('injected leaf failure: ' + leaf, result.stderr)
                self.assertIn('regeneration write-set PASS', result.stdout)
                self.assertNotIn('>> done:', result.stdout)

    def test_undeclared_writes_refuse_on_success_and_on_leaf_failure(self):
        for fail in (False, True):
            with self.subTest(fail=fail):
                (self.root / 'handwritten.rs').unlink(missing_ok=True)
                env = {'CONTROL_UNDECLARED': 'gen_dicom_dict.py'}
                if fail:
                    env['CONTROL_FAIL'] = 'gen_dicom_dict.py'
                result, calls = self.run_regeneration(env=env)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('gen_dicom_dict.py', [c['tool'] for c in calls])
                self.assertIn('handwritten.rs', result.stderr)
                self.assertIn('write-set rejected', result.stderr)
                if fail:
                    self.assertEqual(result.returncode, 47)
                    self.assertEqual(calls[-1]['tool'], 'gen_dicom_dict.py')

    def test_missing_explicit_source_refuses_before_any_leaf(self):
        result, calls = self.run_regeneration(full=True, env={
            'OXIDEX_EXIFTOOL_LIB': str(self.base / 'missing source/lib')})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(calls, [])
        self.assertIn('no ExifTool lib', result.stderr)


if __name__ == '__main__':
    unittest.main()
