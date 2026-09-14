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
def artifact_path(key):
    items=[item for item in artifacts.ARTIFACTS if item.key==key]
    assert len(items)==1
    return root/items[0].path
if mode=='rustfmt':
    paths={str(pathlib.Path(x).resolve()) for x in args if x.endswith('.rs')}
    assert paths in [{str(root/a.path) for a in artifacts.select(tier,kind='rust')} for tier in (1,2)]
elif mode=='chosen-perl':
    if name in ('capture_exif_mandatory_fact.pl', 'capture_raw_jfif_fact.pl'):
        assert pathlib.Path(args[0]).resolve()==lib
        print(json.dumps({'marker':'explicit-A'}))
    elif name in ('dump_tables.pl','dump_filetypes.pl'):
        reader_only=args.pop(0)=='--reader-only' if args[0]=='--reader-only' else False
        hydrated=args.pop(0)=='--hydrated-layouts' if args[0]=='--hydrated-layouts' else False
        assert pathlib.Path(args[0]).resolve()==lib
        captured={'exiftool_version':(root/'.exiftool-version').read_text().strip(),'marker':'explicit-A'}
        if reader_only: captured['reader_only']=True
        if hydrated: captured['hydrated_layouts']={'selection':'full_hydrated_catalog'}
        print(json.dumps(captured))
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
    elif name=='setnewvalue_address_probe.pl':
        assert pathlib.Path(args[0]).resolve()==lib
        assert json.loads(pathlib.Path(args[1]).read_text())['marker']=='explicit-A'
        print(json.dumps({'marker':'explicit-A'}))
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
        output(flag('--ifd-identity-ledger-out'),'ifd-identity-ledger')
        output(flag('--keyed-out'),'keyed')
        output(flag('--value-conv-ledger-out'),'value-ledger')
    elif name=='serial_directory.py':
        dump(args[0]);output(flag('--rust-output'),'serial')
        assert flag('--rust-output')==artifact('serial_directory')
        output(flag('--output'),'serial-report')
    elif name in ('scalar_helper_codegen.py', 'checkexif_rust_codegen.py', 'sanitize_rust_codegen.py', 'convinv_rust_codegen.py', 'convinv_row_codegen.py', 'final_scalar_stage.py', 'mandatory_defaults_codegen.py', 'raw_jfif_codegen.py'):
        dump(args[0])
        selected={root/item.path for item in artifacts.select(producer=name.removesuffix('.py'))}
        assert {flag('--output'),flag('--report')}==selected
        if name=='mandatory_defaults_codegen.py':
            assert json.loads(flag('--writer-tables').read_text())['marker']=='explicit-A'
            assert flag('--selected-perl') == pathlib.Path(os.environ['EXIFTOOL_PERL'])
        if name=='raw_jfif_codegen.py':
            assert flag('--selected-perl') == pathlib.Path(os.environ['EXIFTOOL_PERL'])
        output(flag('--output'),name);output(flag('--report'),name+'-ledger')
    elif name=='setnewvalue_addressing.py':
        assert 'hydrated_layouts' not in json.loads(pathlib.Path(args[0]).read_text())
        assert 'reader_only' not in json.loads(pathlib.Path(args[0]).read_text())
        dump(args[0]); flag('--rows').write_text(json.dumps({'marker':'explicit-A'})); output(flag('--report'),name+'-report')
    elif name=='setnewvalue_address_rust_codegen.py':
        dump(args[0]); assert json.loads(pathlib.Path(args[1]).read_text())['marker']=='explicit-A'
        selected={root/item.path for item in artifacts.select(producer='setnewvalue_address_rust_codegen')}
        assert {flag('--output'),flag('--report'),flag('--write-ownership-ledger')}==selected
        if '--ownership-ledger' in args:
            assert flag('--ownership-ledger')==artifact_path('setnewvalue-ownership-ledger')
        else:
            assert '--bootstrap-ownership-ledger' in args
        output(flag('--output'),name); output(flag('--report'),name+'-report'); output(flag('--write-ownership-ledger'),name+'-ownership')
    elif name=='setnewvalue_public_migration_ledger.py':
        dump(args[0])
        selected={root/item.path for item in artifacts.select(producer='setnewvalue_public_migration_ledger')}
        assert {flag('--output'),flag('--write-ledger')}==selected
        if '--prior-ledger' in args:
            assert flag('--prior-ledger')==artifact_path('setnewvalue-public-migration-ledger')
        else:
            assert '--bootstrap' in args
        output(flag('--output'),name); output(flag('--report'),name+'-report'); output(flag('--write-ledger'),name+'-ledger')
    elif name=='fresh_jpeg_byte_order_native.py':
        assert flag('--perl')==pathlib.Path(os.environ['EXIFTOOL_PERL'])
        assert flag('--exiftool-dir')==lib
        flag('--output').write_text(json.dumps({'marker':'explicit-A'}))
    elif name=='fresh_jpeg_byte_order_codegen.py':
        assert json.loads(pathlib.Path(args[0]).read_text())['marker']=='explicit-A'
        dump(flag('--writer-tables'))
        selected={root/item.path for item in artifacts.select(producer='fresh_jpeg_byte_order_codegen')}
        assert {flag('--output'),flag('--report')}==selected
        output(flag('--output'),name); output(flag('--report'),name+'-ledger')
    elif name=='verify_serial_directory.py':
        assert pathlib.Path(args[0]).resolve()==artifact('serial_directory')
        assert pathlib.Path(args[0]).read_text()=='generated explicit-A serial\n'
        dump(args[1])
    elif name=='gen_nikon_encrypted_tables.py':
        dump(args[0]);assert flag('-o')==artifact('gen_nikon_encrypted_tables')
        output(flag('-o'),name)
    elif name in ('codegen_filetypes.py','codegen_fits.py','gen_sony_main_extra_tables.py','gen_minolta_a100_tables.py','gen_nikon_settings_tables.py','gen_sony_plain_tables.py'):
        dump(args[0]);output(flag('-o'),name)
    elif name=='codegen_composite.py':
        dump(args[0]);output(flag('-o'),'composite');output(flag('--generated-out'),'composite-compute')
    elif name in ('quicktime_generated_specs.py', 'quicktime_keys_specs.py', 'quicktime_userdata_specs.py'):
        # This producer intentionally owns fixed manifest paths rather than
        # accepting output flags.  Its fresh dump is still part of the tier-1
        # source-selection contract, and both artifacts must be declared.
        assert args == ['--dump', str(pathlib.Path(args[1]).resolve()), '--replace']
        dump(flag('--dump'))
        assert json.loads(flag('--dump').read_text())['hydrated_layouts']['selection']=='full_hydrated_catalog'
        assert json.loads(flag('--dump').read_text())['reader_only'] is True
        assert flag('--dump').name.startswith('tables-hydrated-reader-')
        for item in artifacts.select(producer=name.removesuffix('.py')):
            output(root/item.path,name)
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
    elif name=='verify.py':
        assert pathlib.Path(args[1]).resolve()==lib
        assert flag('--keyed-generated')==root/next(a.path for a in artifacts.select() if a.key=='keyed')
        assert flag('--keyed-generated').read_text()=='generated explicit-A keyed\n'
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
    elif name in ('verify_geotiff.py','verify_dicom_dict.py','verify_nikon_settings.py','verify_sony_plain.py'):
        assert flag('--exiftool-dir')==lib.parent
        assert flag('--perl')==pathlib.Path(os.environ['EXIFTOOL_PERL'])
        producer={'verify_geotiff.py':'gen_geotiff_printconv','verify_dicom_dict.py':'gen_dicom_dict','verify_nikon_settings.py':'gen_nikon_settings_tables','verify_sony_plain.py':'gen_sony_plain_tables'}[name]
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

    def test_tier2_only_keeps_a_complete_cache_and_writes_a_distinct_reader_cache(self):
        self.log.unlink(missing_ok=True)
        full_cache = self.cache / f'tables-{self.pin}.json'
        full_bytes = b'{"complete-writer-capture":"must-survive"}\n'
        full_cache.write_bytes(full_bytes)
        reader_cache = self.cache / f'tables-reader-{self.pin}.json'
        reader_cache.unlink(missing_ok=True)
        result = subprocess.run(
            ['bash', str(self.tools / 'regen-all.sh'), '--tier2-only'],
            cwd=self.root, env=self.env, text=True, capture_output=True, timeout=45,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(full_cache.read_bytes(), full_bytes)
        self.assertEqual(json.loads(reader_cache.read_text())['marker'], 'explicit-A')
        calls = [json.loads(line) for line in self.log.read_text().splitlines()]
        dump_calls = [call for call in calls if call['tool'] == 'dump_tables.pl']
        self.assertEqual(len(dump_calls), 1)
        self.assertEqual(dump_calls[0]['argv'][0], '--reader-only')

    def test_regeneration_refuses_unknown_arguments_before_any_leaf_runs(self):
        self.log.unlink(missing_ok=True)
        result = subprocess.run(
            ['bash', str(self.tools / 'regen-all.sh'), '--not-a-mode'],
            cwd=self.root, env=self.env, text=True, capture_output=True, timeout=45,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn('usage:', result.stderr)
        self.assertFalse(self.log.exists())

    def extra_leaves(self):
        # This is the invocation contract, not another output-path manifest.
        return ['gen_geotiff_printconv.py', 'gen_dicom_dict.py',
                'dump_lens_alternatives.pl', 'verify_geotiff.py',
                'verify_dicom_dict.py', 'verify_lens_alternatives.py',
                'gen_nikon_settings_tables.py', 'verify_nikon_settings.py',
                'gen_nikon_encrypted_tables.py',
                'gen_sony_plain_tables.py', 'verify_sony_plain.py']

    def test_both_tiers_and_tier2_use_selected_source_and_complete_checks(self):
        for full in (True, False):
            with self.subTest(full=full):
                result, calls = self.run_regeneration(full=full)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                names = [c['tool'] for c in calls]
                for name in self.extra_leaves():
                    self.assertEqual(names.count(name), 1, names)
                # QuickTime owns fixed manifest paths, so it runs only with
                # tier 1's dedicated hydrated reader dump and not in the
                # tier-2 refresh.
                self.assertEqual(names.count('quicktime_generated_specs.py'), int(full), names)
                self.assertEqual(names.count('quicktime_keys_specs.py'), int(full), names)
                self.assertEqual(names.count('quicktime_userdata_specs.py'), int(full), names)
                self.assertEqual(names.count('verify_exprs.py'), int(full))
                self.assertEqual(names.count('serial_directory.py'), int(full))
                self.assertEqual(names.count('scalar_helper_codegen.py'), int(full))
                self.assertEqual(names.count('checkexif_rust_codegen.py'), int(full))
                self.assertEqual(names.count('sanitize_rust_codegen.py'), int(full))
                self.assertEqual(names.count('convinv_rust_codegen.py'), int(full))
                self.assertEqual(names.count('convinv_row_codegen.py'), int(full))
                self.assertEqual(names.count('final_scalar_stage.py'), int(full))
                self.assertEqual(names.count('capture_exif_mandatory_fact.pl'), int(full))
                self.assertEqual(names.count('mandatory_defaults_codegen.py'), int(full))
                self.assertEqual(names.count('capture_raw_jfif_fact.pl'), int(full))
                self.assertEqual(names.count('raw_jfif_codegen.py'), int(full))
                self.assertEqual(names.count('setnewvalue_addressing.py'), int(full))
                self.assertEqual(names.count('setnewvalue_address_probe.pl'), int(full))
                self.assertEqual(names.count('setnewvalue_address_rust_codegen.py'), int(full))
                self.assertEqual(names.count('setnewvalue_public_migration_ledger.py'), int(full))
                self.assertEqual(names.count('fresh_jpeg_byte_order_native.py'), int(full))
                self.assertEqual(names.count('fresh_jpeg_byte_order_codegen.py'), int(full))
                self.assertEqual(names.count('verify_serial_directory.py'), int(full))
                dump_calls = [c for c in calls if c['tool'] == 'dump_tables.pl']
                if full:
                    self.assertEqual(len(dump_calls), 3)
                    self.assertEqual(dump_calls[0]['argv'], [str(self.source / 'lib')])
                    self.assertEqual(dump_calls[1]['argv'],
                                     ['--reader-only', '--hydrated-layouts', str(self.source / 'lib')])
                    self.assertEqual(dump_calls[2]['argv'], ['--reader-only', str(self.source / 'lib')])
                else:
                    self.assertEqual(len(dump_calls), 1)
                    self.assertEqual(dump_calls[0]['argv'][0], '--reader-only')
                if full:
                    self.assertLess(names.index('serial_directory.py'), names.index('rustfmt'))
                    self.assertLess(names.index('scalar_helper_codegen.py'), names.index('rustfmt'))
                    self.assertLess(names.index('checkexif_rust_codegen.py'), names.index('rustfmt'))
                    self.assertLess(names.index('sanitize_rust_codegen.py'), names.index('rustfmt'))
                    self.assertLess(names.index('convinv_rust_codegen.py'), names.index('convinv_row_codegen.py'))
                    self.assertLess(names.index('convinv_row_codegen.py'), names.index('final_scalar_stage.py'))
                    self.assertLess(names.index('final_scalar_stage.py'), names.index('capture_exif_mandatory_fact.pl'))
                    self.assertLess(names.index('capture_exif_mandatory_fact.pl'), names.index('mandatory_defaults_codegen.py'))
                    self.assertLess(names.index('mandatory_defaults_codegen.py'), names.index('rustfmt'))
                    self.assertLess(names.index('capture_raw_jfif_fact.pl'), names.index('raw_jfif_codegen.py'))
                    self.assertLess(names.index('raw_jfif_codegen.py'), names.index('rustfmt'))
                    # Address operands are a source-only prebuild for the
                    # expression oracle and must precede conversion stages.
                    self.assertLess(names.index('setnewvalue_addressing.py'), names.index('setnewvalue_address_probe.pl'))
                    self.assertLess(names.index('setnewvalue_address_probe.pl'), names.index('setnewvalue_address_rust_codegen.py'))
                    self.assertLess(names.index('setnewvalue_address_rust_codegen.py'), names.index('final_scalar_stage.py'))
                    self.assertLess(names.index('setnewvalue_address_rust_codegen.py'), names.index('setnewvalue_public_migration_ledger.py'))
                    self.assertLess(names.index('setnewvalue_public_migration_ledger.py'), names.index('rustfmt'))
                    self.assertLess(names.index('fresh_jpeg_byte_order_native.py'), names.index('fresh_jpeg_byte_order_codegen.py'))
                    self.assertLess(names.index('fresh_jpeg_byte_order_codegen.py'), names.index('rustfmt'))
                    self.assertGreater(names.index('verify_serial_directory.py'), names.index('rustfmt'))
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
                dump_cache = self.cache / f'tables-reader-{self.pin}.json'
                self.assertEqual(json.loads(dump_cache.read_text())['marker'], 'explicit-A')
                self.assertIs(json.loads(dump_cache.read_text())['reader_only'], True)
                if full:
                    full_cache = self.cache / f'tables-{self.pin}.json'
                    self.assertEqual(json.loads(full_cache.read_text())['marker'], 'explicit-A')
                    self.assertNotIn('reader_only', json.loads(full_cache.read_text()))
                self.assertIn('regeneration write-set PASS', result.stdout)
                self.assertEqual('unexpected PATH Perl' in result.stderr, False)

    def test_missing_address_ownership_uses_only_explicit_bootstrap(self):
        ownership = next(item for item in self.manifest.ARTIFACTS
                         if item.key == 'setnewvalue-ownership-ledger')
        (self.root / ownership.path).unlink()
        result, calls = self.run_regeneration(full=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        call = next(call for call in calls if call['tool'] == 'setnewvalue_address_rust_codegen.py')
        self.assertIn('--bootstrap-ownership-ledger', call['argv'])
        self.assertNotIn('--ownership-ledger', call['argv'])
        self.assertTrue((self.root / ownership.path).is_file())

    def test_missing_public_migration_ledger_uses_only_explicit_bootstrap(self):
        ledger = next(item for item in self.manifest.ARTIFACTS
                      if item.key == 'setnewvalue-public-migration-ledger')
        (self.root / ledger.path).unlink()
        result, calls = self.run_regeneration(full=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        call = next(call for call in calls if call['tool'] == 'setnewvalue_public_migration_ledger.py')
        self.assertIn('--bootstrap', call['argv'])
        self.assertNotIn('--prior-ledger', call['argv'])
        self.assertTrue((self.root / ledger.path).is_file())

    def test_public_migration_regeneration_is_stable_with_valid_prior(self):
        result, _ = self.run_regeneration(full=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        rules = next(item for item in self.manifest.ARTIFACTS
                     if item.key == 'setnewvalue-public-migration-rules')
        ledger = next(item for item in self.manifest.ARTIFACTS
                      if item.key == 'setnewvalue-public-migration-ledger')
        before = ((self.root / rules.path).read_bytes(), (self.root / ledger.path).read_bytes())
        result, calls = self.run_regeneration(full=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        call = next(call for call in calls if call['tool'] == 'setnewvalue_public_migration_ledger.py')
        self.assertIn('--prior-ledger', call['argv'])
        self.assertEqual(before, ((self.root / rules.path).read_bytes(), (self.root / ledger.path).read_bytes()))

    def test_each_new_producer_or_verifier_failure_survives_exit_guard(self):
        for leaf in self.extra_leaves():
            with self.subTest(leaf=leaf):
                result, calls = self.run_regeneration(env={'CONTROL_FAIL': leaf})
                self.assertEqual(result.returncode, 47, result.stdout + result.stderr)
                self.assertEqual(calls[-1]['tool'], leaf)
                self.assertIn('injected leaf failure: ' + leaf, result.stderr)
                self.assertIn('regeneration write-set PASS', result.stdout)
                self.assertNotIn('>> done:', result.stdout)

    def test_tier_one_producer_and_verifier_failures_survive_exit_guard(self):
        for leaf in ('serial_directory.py', 'scalar_helper_codegen.py', 'checkexif_rust_codegen.py', 'sanitize_rust_codegen.py', 'convinv_rust_codegen.py', 'convinv_row_codegen.py', 'final_scalar_stage.py', 'capture_exif_mandatory_fact.pl', 'mandatory_defaults_codegen.py', 'capture_raw_jfif_fact.pl', 'raw_jfif_codegen.py', 'setnewvalue_addressing.py', 'setnewvalue_address_probe.pl', 'setnewvalue_address_rust_codegen.py', 'setnewvalue_public_migration_ledger.py', 'verify_serial_directory.py'):
            with self.subTest(leaf=leaf):
                result, calls = self.run_regeneration(full=True, env={'CONTROL_FAIL': leaf})
                self.assertEqual(result.returncode, 47, result.stdout + result.stderr)
                self.assertEqual(calls[-1]['tool'], leaf)
                self.assertIn('injected leaf failure: ' + leaf, result.stderr)
                self.assertNotIn('>> done:', result.stdout)

    def test_quicktime_fixed_path_producer_failure_survives_exit_guard(self):
        result, calls = self.run_regeneration(
            full=True, env={'CONTROL_FAIL': 'quicktime_generated_specs.py'}
        )
        self.assertEqual(result.returncode, 47, result.stdout + result.stderr)
        self.assertEqual(calls[-1]['tool'], 'quicktime_generated_specs.py')
        self.assertIn('injected leaf failure: quicktime_generated_specs.py', result.stderr)
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
