"""Manifest selectors and observed filesystem changes, using tiny Git repos."""
from dataclasses import replace
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import artifacts
import table_modules

REPO_ROOT = Path(__file__).resolve().parents[2]


class ManifestTests(unittest.TestCase):
    def test_unique_valid_partition_and_selectors(self):
        artifacts.validate()
        all_items = artifacts.select()
        self.assertTrue(all_items)
        self.assertTrue(artifacts.select(1))
        self.assertTrue(artifacts.select(2))
        members = len(artifacts.BINARY_MODULE_STEMS) + len(artifacts.IFD_MODULE_STEMS)
        self.assertEqual(len(artifacts.STATIC_ARTIFACTS), 69)
        self.assertEqual(len(all_items), 69 + members)
        self.assertEqual({item.key for item in artifacts.select(producer="quicktime_keys_specs")},
                         {"quicktime-keys-specs", "quicktime-keys-ledger"})
        self.assertEqual({item.key for item in artifacts.select(producer="quicktime_userdata_specs")},
                         {"quicktime-userdata-specs", "quicktime-userdata-ledger"})
        self.assertEqual({item.key for item in artifacts.select(producer="conv_codegen")},
                         {"conv-exif-main", "conv-exif-main-ledger"})
        self.assertEqual(len(artifacts.select(1)), 46 + members)
        self.assertEqual(len(artifacts.select(2)), 23)
        self.assertEqual(len(all_items), len(artifacts.select(1)) + len(artifacts.select(2)))
        self.assertEqual(set(all_items), set(artifacts.select(1) + artifacts.select(2)))
        self.assertTrue(all(a.path.endswith('.rs') for a in artifacts.select(kind='rust')))
        for producer in {a.producer for a in all_items}:
            self.assertEqual(artifacts.select(producer=producer),
                             [a for a in all_items if a.producer == producer])

    def test_module_files_are_the_committed_hubs_module_lines(self):
        """The per-module entries are read from the hubs, so the inventory
        follows whichever release was generated; what a static list used to
        guarantee -- no orphan, no missing file, rustfmt's order -- is
        `family_errors`, which `check` applies after every regeneration."""
        self.assertEqual(artifacts.family_errors(REPO_ROOT), [])
        by_key = {item.key: item for item in artifacts.select(producer="codegen")}
        for kind, stems in (("binary", artifacts.BINARY_MODULE_STEMS), ("ifd", artifacts.IFD_MODULE_STEMS)):
            with self.subTest(kind=kind):
                hub = REPO_ROOT / "src" / "exiftool_tables" / kind / table_modules.MOD_RS
                self.assertTrue(stems)
                self.assertEqual(list(stems), sorted(set(stems)))
                self.assertEqual(table_modules.declared_stems(hub), list(stems))
                on_disk = sorted(p.stem for p in hub.parent.glob("*.rs") if p.name != table_modules.MOD_RS)
                self.assertEqual(on_disk, list(stems))
                for stem in stems:
                    item = by_key[f"{kind}-{stem}"]
                    self.assertEqual((item.tier, item.path), (1, f"src/exiftool_tables/{kind}/{stem}.rs"))
                    self.assertTrue(artifacts.is_family_member(item.path))
                self.assertFalse(artifacts.is_family_member(f"src/exiftool_tables/{kind}/mod.rs"))
                self.assertFalse(artifacts.is_family_member(f"src/exiftool_tables/{kind}/sub/x.rs"))

    def test_hub_grammar_matches_table_modules(self):
        self.assertEqual(artifacts.HUB, table_modules.MOD_RS)
        self.assertEqual(artifacts.MOD_LINE_RE.pattern, table_modules.MOD_LINE_RE.pattern)
        self.assertEqual(artifacts.MOD_LINE_RE.flags, table_modules.MOD_LINE_RE.flags)

    def test_missing_hub_declares_nothing_and_is_reported(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(artifacts.module_stems("binary", tmp), ())
            self.assertEqual(len(artifacts.inventory(tmp)), 66)
            self.assertIn("missing split table hub src/exiftool_tables/binary/mod.rs",
                          artifacts.family_errors(tmp))

    def test_manifest_digest_does_not_depend_on_the_release_module_set(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for kind in artifacts.MODULE_FAMILIES:
                hub = root / artifacts.family_dir(kind) / table_modules.MOD_RS
                hub.parent.mkdir(parents=True)
                hub.write_text(table_modules.render_hub("", ["only"], ""))
            self.assertEqual(len(artifacts.inventory(root)), len(artifacts.STATIC_ARTIFACTS) + 2)
            self.assertNotEqual(artifacts.inventory(root), artifacts.inventory(REPO_ROOT))
        self.assertEqual(artifacts.manifest_digest(), artifacts.manifest_digest())

    def test_invalid_and_duplicate_declarations_fail(self):
        first = artifacts.ARTIFACTS[0]
        for path in ('../escape', '/absolute', './relative', 'x//y', 'x/../y',
                     'x y', 'x\x00y', 'target/out.rs', 'x\\y'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                artifacts.validate([replace(first, path=path)])
        for item in (first, replace(first, key='other'), replace(first, path='other.rs')):
            with self.assertRaises(ValueError):
                artifacts.validate([first, item])
        for kwargs in ({'tier': '3'}, {'kind': 'json'}, {'producer': 'missing'}):
            with self.assertRaises(ValueError):
                artifacts.select(**kwargs)


class WriteSetTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.root = self.base / 'repo'
        self.root.mkdir()
        self.git('init', '-q')
        # The test has no second output list. Fixture paths derive from the
        # production inventory; real generator runs separately test completeness.
        for item in artifacts.ARTIFACTS:
            path = self.root / item.path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(self.hub(artifacts.module_stems(item.key)) if item.key in artifacts.MODULE_FAMILIES
                            else 'initial\n')
        (self.root / 'unrelated.rs').write_text('original\n')
        (self.root / '.gitignore').write_text('ignored.rs\n/target/\n')
        self.git('add', '.')
        self.git('-c', 'user.name=Artifact test', '-c', 'user.email=test@example.invalid',
                 'commit', '-qm', 'fixture')

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.root), *args], stderr=subprocess.STDOUT)

    @staticmethod
    def hub(stems):
        return table_modules.render_hub('//! generated hub; DO NOT EDIT\n', stems, '')

    def regenerate_family(self, kind, stems):
        """What codegen does for a release with this module set: rewrite the
        hub, write every declared module file, delete the ones it dropped."""
        directory = self.root / artifacts.family_dir(kind)
        files = {f'{stem}.rs': f'//! {stem}; DO NOT EDIT\n' for stem in stems}
        files[table_modules.MOD_RS] = self.hub(stems)
        for path in directory.glob('*.rs'):
            if path.name not in files:
                path.unlink()
        for name, text in files.items():
            (directory / name).write_text(text)

    def test_release_that_adds_and_drops_modules_passes(self):
        # 11.78 against 13.59: no DJI (dropped), a JSON module (added).
        saved = self.snap(tier='1')
        stems = sorted((set(artifacts.module_stems('binary', self.root)) - {'dji'}) | {'json'})
        self.regenerate_family('binary', stems)
        changes = artifacts.check(self.root, saved)
        self.assertIn('src/exiftool_tables/binary/dji.rs', changes)
        self.assertIn('src/exiftool_tables/binary/json.rs', changes)
        self.assertIn('src/exiftool_tables/binary/mod.rs', changes)
        self.assertEqual([a.key for a in artifacts.select(1, root=self.root) if a.key.startswith('binary-')],
                         [f'binary-{stem}' for stem in stems])

    def test_orphan_or_missing_module_file_fails(self):
        saved = self.snap(tier='1')
        (self.root / 'src/exiftool_tables/ifd/orphan.rs').write_text('//! orphan; DO NOT EDIT\n')
        self.reject(saved)
        (self.root / 'src/exiftool_tables/ifd/orphan.rs').unlink()
        (self.root / 'src/exiftool_tables/ifd/exif.rs').unlink()   # the hub still declares it
        self.reject(saved)

    def test_module_file_is_not_writable_from_the_other_tier(self):
        saved = self.snap(tier='2')
        stems = sorted(set(artifacts.module_stems('binary', self.root)) - {'dji'})
        self.regenerate_family('binary', stems)
        self.reject(saved)

    def snap(self, tier='2', caches=()):
        return artifacts.snapshot(self.root, tier, caches)

    def reject(self, saved):
        with self.assertRaises(ValueError):
            artifacts.check(self.root, saved)

    def test_noop_and_selected_changes_pass(self):
        saved = self.snap()
        self.assertEqual(artifacts.check(self.root, saved), [])
        path = artifacts.select(2)[0].path
        (self.root / path).write_text('regenerated\n')
        self.assertEqual(artifacts.check(self.root, saved), [path])

    def test_dirty_file_changed_again_with_same_size_and_mtime_fails(self):
        path = self.root / 'unrelated.rs'
        path.write_text('dirty AAA\n')
        saved = self.snap()
        times = path.stat()
        path.write_text('dirty BBB\n')
        os.utime(path, ns=(times.st_atime_ns, times.st_mtime_ns))
        self.reject(saved)

    def test_new_untracked_ignored_and_unusual_names_fail(self):
        for name in ('new.rs', 'ignored.rs', 'space and\nnewline.rs'):
            with self.subTest(name=name):
                saved = self.snap()
                path = self.root / name
                path.write_text('undeclared\n')
                self.reject(saved)
                path.unlink()

    def test_untracked_existing_change_fails(self):
        path = self.root / 'untracked.rs'
        path.write_text('before')
        saved = self.snap()
        path.write_text('after')
        self.reject(saved)

    def test_delete_and_mode_change_fail(self):
        path = self.root / 'unrelated.rs'
        saved = self.snap()
        path.chmod(0o755)
        self.reject(saved)
        saved = self.snap()
        path.unlink()
        self.reject(saved)

    def test_cross_tier_write_and_missing_required_output_fail(self):
        saved = self.snap()
        (self.root / artifacts.select(1)[0].path).write_text('wrong tier')
        self.reject(saved)
        saved = self.snap()
        (self.root / artifacts.select(2)[0].path).unlink()
        self.reject(saved)

    def test_index_change_fails(self):
        saved = self.snap()
        path = artifacts.select(2)[0].path
        (self.root / path).write_text('allowed file, unexpected staging')
        self.git('add', path)
        self.reject(saved)

    def test_ci_diff_requires_committed_outputs_and_catches_staged_changes(self):
        self.assertEqual(artifacts.diff_committed(self.root, 2), 0)
        path = artifacts.select(2)[0].path
        (self.root / path).write_text('changed and staged')
        self.git('add', path)
        self.assertEqual(artifacts.diff_committed(self.root, 2), 1)
        self.git('rm', '--cached', path)
        self.git('-c', 'user.name=Artifact test', '-c', 'user.email=test@example.invalid',
                 'commit', '-qm', 'leave output untracked')
        with self.assertRaises(subprocess.CalledProcessError):
            artifacts.diff_committed(self.root, 2)

    def test_symlinks_are_not_followed_and_outputs_cannot_be_symlinks(self):
        outside = self.base / 'outside'
        outside.mkdir()
        (outside / 'data').write_text('before')
        (self.root / 'linked-dir').symlink_to(outside, target_is_directory=True)
        saved = self.snap()
        (outside / 'data').write_text('external changes are outside scope')
        self.assertEqual(artifacts.check(self.root, saved), [])
        (self.root / 'linked-dir').unlink()
        self.reject(saved)
        path = self.root / artifacts.select(2)[0].path
        path.unlink()
        path.symlink_to(outside / 'data')
        with self.assertRaises(ValueError):
            self.snap()

    def test_cache_exclusions_cannot_hide_source(self):
        for cache in (self.root, self.base, self.root / 'src',
                      self.root / artifacts.select(2)[0].path):
            with self.subTest(cache=cache), self.assertRaises(ValueError):
                self.snap(caches=[cache])
        saved = self.snap(caches=[self.root / 'local-cache'])
        (self.root / 'local-cache').mkdir()
        (self.root / 'local-cache/data').write_text('cache')
        (self.root / 'target').mkdir()
        (self.root / 'target/build').write_text('cache')
        self.assertEqual(artifacts.check(self.root, saved), [])

    def test_snapshot_identity_or_schema_mismatch_fails(self):
        for key, value in (('root', '/elsewhere'), ('schema', 0), ('manifest', 'bad')):
            saved = self.snap()
            saved[key] = value
            self.reject(saved)

    def run_guard(self, body):
        env = dict(os.environ, ROOT=str(self.root), HERE=str(Path(artifacts.__file__).parent),
                   TMPDIR=str(self.base), PYTHONDONTWRITEBYTECODE='1')
        env.pop('CARGO_TARGET_DIR', None)
        return subprocess.run(['bash', '-c', '''set -euo pipefail
source "$HERE/artifact-env.sh"
begin_regeneration 2 "$ROOT/target/cache"
''' + body], env=env, cwd=self.root, text=True, capture_output=True)

    def test_real_guard_rejects_successful_undeclared_write(self):
        result = self.run_guard('printf bad > "$ROOT/ignored.rs"')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('unexpected=', result.stderr)
        self.assertIn('ignored.rs', result.stderr)

    def test_failed_producer_is_audited_and_original_failure_preserved(self):
        result = self.run_guard('printf bad > "$ROOT/rogue.rs"\nexit 7')
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assertIn('write-set rejected', result.stderr)
        self.assertIn('rogue.rs', result.stderr)

    def test_clean_failure_preserves_failure_and_success_cleans_snapshot(self):
        failed = self.run_guard('exit 9')
        self.assertEqual(failed.returncode, 9)
        self.assertIn('write-set PASS', failed.stdout)
        for path in self.base.glob('oxidex-regen.*'):
            path.unlink()
        passed = self.run_guard('true')
        self.assertEqual(passed.returncode, 0, passed.stderr)
        self.assertEqual(list(self.base.glob('oxidex-regen.*')), [])

    def test_ambient_cleanup_path_is_not_owned(self):
        unrelated = self.base / 'preserve-me'
        unrelated.write_text('caller file')
        result = self.run_guard('true')
        self.assertEqual(result.returncode, 0, result.stderr)
        # Exercise an exported ambient value before begin_regeneration.
        env = dict(os.environ, ROOT=str(self.root), HERE=str(Path(artifacts.__file__).parent),
                   TMPDIR=str(self.base), LEICA_RAW=str(unrelated),
                   REGEN_CLEANUP_FILE=str(unrelated))
        env.pop('CARGO_TARGET_DIR', None)
        result = subprocess.run(['bash', '-c', '''set -euo pipefail
source "$HERE/artifact-env.sh"
begin_regeneration 2 "$ROOT/target/cache"
exit 7
'''], env=env, cwd=self.root, text=True, capture_output=True)
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assertEqual(unrelated.read_text(), 'caller file')

    def test_absolute_cli_selector_resolves_relative_root(self):
        result = subprocess.run(['python3', artifacts.__file__, '--root', '.', 'paths',
                                 '--tier', '1', '--absolute'], cwd=self.root,
                                text=True, capture_output=True, check=True)
        self.assertTrue(all(Path(p).is_absolute() for p in result.stdout.splitlines()))

    def test_missing_or_malformed_snapshot_cli_fails(self):
        path = self.base / 'snapshot.json'
        for contents in (None, '{', '{}'):
            if contents is not None:
                path.write_text(contents)
            result = subprocess.run(['python3', artifacts.__file__, '--root', str(self.root),
                                     'check', str(path)], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main()
