"""Small synthetic installations exercising Task19's real bootstrap verifier.

Only the locked input data and git tag answer are synthetic. Manifest validation,
archive/tree hashing, capability dispatch and process execution are production code.
The Perl stand-in uses system Perl so hostile startup hooks really execute.
"""
from contextlib import ExitStack
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import version_transition_qualification as q


class BootstrapBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.stack = ExitStack()
        self.addCleanup(self.stack.close)
        parent = Path(os.environ.get("OXIDEX_TEST_EVIDENCE_ROOT", q.ops_paths.ops_root() / "evidence"))
        parent.mkdir(parents=True, exist_ok=True)
        self.root = Path(self.stack.enter_context(tempfile.TemporaryDirectory(dir=parent))).resolve()
        spec = importlib.util.spec_from_file_location("boundary_bootstrap", q.REPOSITORY_ROOT / "tools/release/bootstrap_oracle.py")
        self.b = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.b)
        b = self.b
        self.stack.enter_context(patch.object(b, "DURABLE_ROOT", self.root))
        self.stack.enter_context(patch.object(b, "MIN_CORPUS_FILES", 1))
        lock = copy.deepcopy(b.LOCK)
        self.stack.enter_context(patch.object(b, "LOCK", lock))
        self.perl = b.perl_path(self.root)
        self.perl.parent.mkdir(parents=True)
        self.perl.write_text('''#!/usr/bin/perl
use strict;
if ($ENV{BOUNDARY_DAMAGE}) { open(my $f, '>>', $ENV{BOUNDARY_DAMAGE}) or die $!; print $f "drift"; close($f); }
if ($ENV{BOUNDARY_PID}) { open(my $p, '>', $ENV{BOUNDARY_PID}) or die $!; print $p $$; close($p); }
my $args = join(' ', @ARGV);
if ($ENV{BOUNDARY_SLEEP} && (!$ENV{BOUNDARY_MATCH} || index($args, $ENV{BOUNDARY_MATCH}) >= 0)) {
    select(undef, undef, undef, $ENV{BOUNDARY_SLEEP});
}
if ($args =~ /Config/) { print "PREFIX"; }
elsif ($args =~ /Archive::Zip/) { print "1.68"; }
elsif ($args =~ /-ver/) { print "13.59"; }
elsif ($args =~ /FileType/) { print "DOCX"; }
else { print "v5.38.2"; }
'''.replace('PREFIX', str(b.perl_prefix(self.root))))
        self.perl.chmod(0o755)
        for path in (b.perl_prefix(self.root) / 'lib/Archive/Zip.pm', b.exiftool_path(self.root),
                     b.exiftool_library_path(self.root), b.exiftool_root(self.root) / 't/images/OOXML.docx',
                     b.corpus_path(self.root) / 'sample'):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('synthetic fixture\n')
        lock['corpus_tree_sha256'] = b.sha256_tree(b.corpus_path(self.root))
        for name, item in lock['archives'].items():
            path = self.root / 'cache/downloads' / item['filename']
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name)
            item['sha256'] = b.sha256_file(path)
        # A real child supplies the locked git answer; no git checkout or network.
        bindir = self.root / 'bin'
        bindir.mkdir()
        git = bindir / 'git'
        git.write_text('#!/bin/sh\nprintf "%s\\n" "' + lock['exiftool']['tag_object'] + '"\n')
        git.chmod(0o755)
        self.stack.enter_context(patch.dict(os.environ, {'PATH': str(bindir) + os.pathsep + os.environ['PATH']}))
        self.manifest = b.verify(self.root, b.VERSION, None)
        self.original = self.manifest.read_bytes()
        self.corpus_manifest = b.corpus_path(self.root).parent / 'combined-samples.manifest'
        self.corpus_original = self.corpus_manifest.read_bytes()
        # Load the real verifier above, then route Task19's dynamic import to it.
        self.stack.enter_context(patch('importlib.util.spec_from_file_location', return_value=SimpleNamespace(loader=SimpleNamespace(exec_module=lambda module: None))))
        self.stack.enter_context(patch('importlib.util.module_from_spec', return_value=b))
        self.stack.enter_context(patch.object(q, 'sys', SimpleNamespace(platform='linux')))
        self.stack.enter_context(patch.object(q.platform, 'machine', return_value='x86_64'))
        self.stack.enter_context(patch.object(q, 'PERL_PROBE_TIMEOUT_SECONDS', 0.2, create=True))

    def assert_evidence_unchanged(self):
        self.assertEqual(self.manifest.read_bytes(), self.original)
        self.assertEqual(self.corpus_manifest.read_bytes(), self.corpus_original)

    def test_probe_compatible_installed_drift_is_refused_without_refresh(self):
        with self.perl.open('a') as stream:
            stream.write('\n# different probe-compatible installed build\n')
        with self.assertRaisesRegex(q.Refused, 'hash mismatch'):
            q._perl(self.root)
        self.assert_evidence_unchanged()

    def test_missing_manifest_refuses_without_creating_one(self):
        self.manifest.unlink()
        with self.assertRaisesRegex(q.Refused, 'provisioned manifest is missing'):
            q._perl(self.root)
        self.assertFalse(self.manifest.exists())

    def test_library_drift_is_refused_before_any_probe(self):
        library = self.b.archive_zip_library_path(self.root)
        library.write_text('probe-compatible library drift')
        marker = self.root / 'probe.pid'
        with patch.dict(os.environ, {'BOUNDARY_PID': str(marker)}):
            with self.assertRaisesRegex(q.Refused, 'hash mismatch'):
                q._perl(self.root)
        self.assertFalse(marker.exists())
        self.assert_evidence_unchanged()

    def test_probe_cannot_refresh_identity_after_mutating_library(self):
        with patch.dict(os.environ, {'BOUNDARY_DAMAGE': str(self.b.archive_zip_library_path(self.root))}):
            with self.assertRaisesRegex(q.Refused, 'hash mismatch'):
                q._perl(self.root)
        self.assert_evidence_unchanged()

    def test_corpus_manifest_drift_is_not_overwritten_before_validation(self):
        self.corpus_manifest.write_text('drift')
        with self.assertRaisesRegex(q.Refused, 'hash mismatch'):
            q._perl(self.root)
        self.assertEqual(self.corpus_manifest.read_text(), 'drift')
        self.assertEqual(self.manifest.read_bytes(), self.original)

    def test_locked_archive_is_still_checked_independently_of_manifest(self):
        archive = next(iter(self.b.LOCK['archives'].values()))
        path = self.root / 'cache/downloads' / archive['filename']
        path.write_text('drift')
        payload = json.loads(self.manifest.read_text())
        for item in payload['artifacts'].values():
            if item['path'] == str(path):
                item['sha256'] = self.b.sha256_file(path)
        self.manifest.write_text(json.dumps(payload))
        with self.assertRaisesRegex(q.Refused, 'locked archive hash mismatch'):
            q._perl(self.root)

    def test_hostile_perl_startup_is_scrubbed_at_actual_capability_probe(self):
        foreign = self.root / 'foreign'
        foreign.mkdir()
        marker = self.root / 'hook-ran'
        (foreign / 'BoundaryHook.pm').write_text('package BoundaryHook; BEGIN { open(my $f, ">", "' + str(marker) + '"); print $f "ran"; die "foreign startup hook executed"; } 1;')
        hostile = {'PERL5LIB': str(foreign), 'PERLLIB': str(foreign), 'PERL5OPT': '-MBoundaryHook'}
        with patch.dict(os.environ, hostile):
            raw = subprocess.run([str(self.perl), '-e', 'print $^V'], capture_output=True, timeout=5)
            self.assertNotEqual(raw.returncode, 0)
            self.assertTrue(marker.exists(), 'negative control must actually execute Perl startup')
            marker.unlink()
            before = dict(os.environ)
            self.assertEqual(q._perl(self.root), self.perl)
            self.assertEqual(dict(os.environ), before)
            self.assertFalse(marker.exists())

    def test_hung_probe_has_named_timeout_and_is_reaped(self):
        pid_path = self.root / 'probe.pid'
        started = time.monotonic()
        with patch.dict(os.environ, {'BOUNDARY_SLEEP': '1', 'BOUNDARY_PID': str(pid_path)}):
            with self.assertRaisesRegex(q.Refused, 'timed out'):
                q._perl(self.root)
        self.assertLess(time.monotonic() - started, 5)
        pid = int(pid_path.read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)
        self.assertEqual(q.executor.unproven_children(), [])
        self.assert_evidence_unchanged()

    def test_all_verifier_commands_have_bounded_owned_execution(self):
        calls = []
        tracked = q.executor._tracked_run
        def observed(argv, **kwargs):
            self.assertEqual(kwargs['timeout'], 0.2)
            self.assertTrue(kwargs['start_new_session'])
            self.assertFalse(kwargs['close_fds'])
            for key in ('PERL5LIB', 'PERLLIB', 'PERL5OPT'):
                self.assertNotIn(key, kwargs['env'])
                self.assertEqual(os.environ[key], 'hostile')
            calls.append(argv)
            return tracked(argv, **kwargs)
        with patch.dict(os.environ, {key: 'hostile' for key in ('PERL5LIB', 'PERLLIB', 'PERL5OPT')}), \
             patch.object(q.executor, '_tracked_run', side_effect=observed):
            self.assertEqual(q._perl(self.root), self.perl)
        self.assertEqual(len(calls), 7)  # git, five bootstrap Perl probes, final version
        self.assertEqual(calls[0], ['git', 'rev-parse', 'HEAD'])
        self.assertTrue(any('-FileType' in argv for argv in calls))
        self.assertTrue(any('-MArchive::Zip' in argv for argv in calls))
        self.assertTrue(any('-MConfig' in argv for argv in calls))

    def test_git_probe_timeout_reaps_process(self):
        git = self.root / 'bin/git'
        pid_path = self.root / 'git.pid'
        git.write_text('#!/usr/bin/perl\nopen(my $f, ">", "' + str(pid_path) + '") or die $!; print $f $$; close($f); sleep 1;')
        with self.assertRaisesRegex(q.Refused, 'timed out: git'):
            q._perl(self.root)
        with self.assertRaises(ProcessLookupError):
            os.kill(int(pid_path.read_text()), 0)
        self.assertEqual(q.executor.unproven_children(), [])
        self.assert_evidence_unchanged()

    def test_exiftool_probe_timeout_reaps_process(self):
        pid_path = self.root / 'exiftool.pid'
        with patch.dict(os.environ, {'BOUNDARY_SLEEP': '1', 'BOUNDARY_MATCH': '-FileType',
                                    'BOUNDARY_PID': str(pid_path)}):
            with self.assertRaisesRegex(q.Refused, 'timed out'):
                q._perl(self.root)
        with self.assertRaises(ProcessLookupError):
            os.kill(int(pid_path.read_text()), 0)
        self.assertEqual(q.executor.unproven_children(), [])
        self.assert_evidence_unchanged()

    def test_bootstrap_timeout_releases_actual_host_lease_after_reaping(self):
        from test_version_transition_qualification import WrapperCallTests, _contend
        wrapper = WrapperCallTests()
        wrapper.setUp()
        self.addCleanup(wrapper.doCleanups)
        pid_path = self.root / 'lease-probe.pid'
        with patch.dict(os.environ, {'BOUNDARY_SLEEP': '1', 'BOUNDARY_PID': str(pid_path)}):
            with self.assertRaisesRegex(q.Refused, 'timed out'):
                real_perl = q._perl
                wrapper.invoke(None, perl_probe=lambda _root: real_perl(self.root))
        with self.assertRaises(ProcessLookupError):
            os.kill(int(pid_path.read_text()), 0)
        self.assertEqual(_contend(wrapper.lease), 'acquired')
        release = json.loads(wrapper.receipts['release_receipt'].read_text())
        self.assertTrue(release['flock_release_confirmed'])
        self.assertEqual(release['terminal_status'], 'failed')
        self.assertFalse(wrapper.row_output.exists())
        self.assert_evidence_unchanged()


if __name__ == '__main__':
    unittest.main()
