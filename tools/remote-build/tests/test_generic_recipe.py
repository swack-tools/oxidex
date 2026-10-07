"""Bounded Just dispatch and signed fleet packet controls; no remote jobs."""
import json
import hashlib
from pathlib import Path
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import os
import shutil
import subprocess
import sys

from lib import remote_build
import route


class GenericRecipeTests(unittest.TestCase):
    def test_linux_perl_route_and_packet_share_fleet_policy(self):
        self.assertIs(route.FLEET_RECIPES, remote_build.FLEET_RECIPES)
        receipt, commands, _, extras, heads = self.exercise(0, 'verify-linux-perl')
        self.assertEqual(heads, ['a' * 40])
        self.assertEqual(set(extras), {'repository.bundle', 'maintainer.allowed_signers',
                                      'fleet-source-head'})
        self.assertEqual(receipt['fleet_source_bundle_sha256'],
                         hashlib.sha256(b'synthetic signed bundle').hexdigest())
        self.assertTrue(any('bundle' in command and 'create' in command
                            for command in commands))

    def test_candidate_receipt_download_caps_real_child_during_transfer(self):
        with tempfile.TemporaryDirectory() as directory:
            local = Path(directory) / 'candidate-receipt.json'
            excess = b'x' * (64 * 1024 + 1)
            transport = SimpleNamespace(scp=lambda destination, remote, download=False: [
                sys.executable, '-c',
                'import pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(b"x" * (64 * 1024 + 1))',
                str(destination),
            ])
            with self.assertRaisesRegex(RuntimeError, 'bound'):
                remote_build._download_checked_candidate(
                    transport, '/target/perl-candidate-export/candidate-receipt.json',
                    local, hashlib.sha256(excess).hexdigest(),
                    remote_build.MAX_CANDIDATE_RECEIPT_BYTES)
            self.assertFalse(local.exists())
            failures = list(Path(directory).glob('.perl-candidate-*.failed'))
            self.assertEqual(len(failures), 1)
            self.assertLessEqual(failures[0].stat().st_size, 64 * 1024)

    def test_candidate_bad_hash_retains_exact_short_failure_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            local = Path(directory) / 'candidate-receipt.json'
            partial = b'partial receipt bytes'
            transport = SimpleNamespace(scp=lambda destination, remote, download=False: [
                sys.executable, '-c',
                'import pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(b"partial receipt bytes")',
                str(destination),
            ])
            with self.assertRaisesRegex(RuntimeError, 'checksum mismatch'):
                remote_build._download_checked_candidate(
                    transport, '/target/perl-candidate-export/candidate-receipt.json',
                    local, '0' * 64, remote_build.MAX_CANDIDATE_RECEIPT_BYTES)
            self.assertFalse(local.exists())
            failures = list(Path(directory).glob('.perl-candidate-*.failed'))
            self.assertEqual(len(failures), 1)
            self.assertEqual(failures[0].read_bytes(), partial)

    def test_candidate_archive_download_caps_real_child_during_transfer(self):
        with tempfile.TemporaryDirectory() as directory:
            local = Path(directory) / 'perl-5.38.2-prefix.tar.gz'
            excess = b'x' * 4097
            transport = SimpleNamespace(scp=lambda destination, remote, download=False: [
                sys.executable, '-c',
                'import pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(b"x" * 4097)',
                str(destination),
            ])
            with self.assertRaisesRegex(RuntimeError, 'bound'):
                remote_build._download_checked_candidate(
                    transport, '/target/perl-candidate-export/perl-5.38.2-prefix.tar.gz',
                    local, hashlib.sha256(excess).hexdigest(), 4096, expected_size=4096)
            self.assertFalse(local.exists())
            failures = list(Path(directory).glob('.perl-candidate-*.failed'))
            self.assertEqual(len(failures), 1)
            self.assertLessEqual(failures[0].stat().st_size, 4096)

    def test_perl_candidate_retrieval_checks_both_durable_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            lock=root/'oracle-lock.json';lock.write_bytes(b'locked source')
            evidence=root/'evidence';evidence.mkdir()
            archive=root/'remote-archive';archive.write_bytes(b'synthetic frozen Perl bytes')
            head='a'*40;tree='b'*40
            candidate={'schema_version':1,'kind':'linux_perl_unapproved_candidate',
                       'source_head':head,'source_tree':tree,
                       'source_clean_context':'remote_verified_signed_fleet_checkout',
                       'source_bundle_sha256':'d'*64,
                       'config_prefix':'/target/ops/toolchains/perl-5.38.2/prefix',
                       'archive_path':'/target/ops/evidence/linux-perl-independent-identity/perl-5.38.2-prefix.tar.gz',
                       'candidate_export_path':'/target/perl-candidate-export',
                       'archive_sha256':hashlib.sha256(archive.read_bytes()).hexdigest(),
                       'archive_bytes':archive.stat().st_size,
                       'lock_sha256':hashlib.sha256(lock.read_bytes()).hexdigest(),
                       'perl_tree_sha256':'c'*64,'replay_tree_sha256':'c'*64,
                       'status':'candidate_only_requires_independent_review'}
            receipt=root/'remote-receipt';receipt.write_text(json.dumps(candidate))
            def ssh(command):return [command]
            def output(command,**kwargs):
                self.assertIn('/perl-candidate-export/',command[0])
                self.assertNotIn('/ops/',command[0])
                path=receipt if 'candidate-receipt.json' in command[0] else archive
                return hashlib.sha256(path.read_bytes()).hexdigest()+'  remote\n'
            def run(command,**kwargs):
                self.assertIn('/perl-candidate-export/',command[1])
                self.assertNotIn('/ops/',command[1])
                source=receipt if 'candidate-receipt.json' in command[1] else archive
                shutil.copyfile(source,command[2])
                return SimpleNamespace(returncode=0)
            transport=SimpleNamespace(scp=lambda local,remote,download=False:
                                      ['mock-scp',remote,str(local)])
            with patch.object(remote_build.subprocess,'check_output',side_effect=output), \
                 patch.object(remote_build.subprocess,'run',side_effect=run):
                result=remote_build.retrieve_perl_candidate(transport,ssh,'run',evidence,head,tree,'d'*64,lock)
            self.assertEqual(result['status'],'unapproved_candidate_retrieved')
            self.assertEqual(Path(result['archive']).read_bytes(),archive.read_bytes())
            self.assertEqual(json.loads(Path(result['receipt']).read_text()),candidate)
            candidate['archive_sha256']='0'*64
            receipt.write_text(json.dumps(candidate))
            retry=evidence/'retry';retry.mkdir()
            with patch.object(remote_build.subprocess,'check_output',side_effect=output), \
                 patch.object(remote_build.subprocess,'run',side_effect=run):
                with self.assertRaisesRegex(RuntimeError,'differs from candidate receipt'):
                    remote_build.retrieve_perl_candidate(transport,ssh,'run2',retry,head,tree,'d'*64,lock)
            candidate['archive_sha256']=hashlib.sha256(archive.read_bytes()).hexdigest()
            candidate['archive_bytes']=remote_build.MAX_CANDIDATE_ARCHIVE_BYTES + 1
            receipt.write_text(json.dumps(candidate))
            oversized=evidence/'oversized';oversized.mkdir()
            transfers=[]
            def bounded_run(command,**kwargs):
                transfers.append(command[1])
                return run(command,**kwargs)
            with patch.object(remote_build.subprocess,'check_output',side_effect=output), \
                 patch.object(remote_build.subprocess,'run',side_effect=bounded_run):
                with self.assertRaisesRegex(RuntimeError,'receipt does not bind'):
                    remote_build.retrieve_perl_candidate(transport,ssh,'run3',oversized,head,tree,'d'*64,lock)
            self.assertEqual(len(transfers),1)
            self.assertIn('candidate-receipt.json',transfers[0])

    def exercise(self, code, recipe="test-package", source_status="", producer_tree='a'*40,
                 retrieval_fail=False):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);commands=[]
            signer=root/'maintainer.allowed_signers'
            signer.write_text('fixture signer')
            extras_seen=[]
            signed_heads=[]
            def snapshot(source,archive,extra_files=None,*,signed_head=None):
                signed_heads.append(signed_head)
                archive.write_bytes(b'synthetic source')
                extras_seen.extend((extra_files or {}).keys())
                return {'archive_sha256':'0'*64,
                        'files':[{'path':name,
                                  'sha256':remote_build.hashlib.sha256(path.read_bytes()).hexdigest()}
                                 for name,path in (extra_files or {}).items()]}
            def run(command, **kwargs):
                commands.append(command)
                if 'bundle' in command and 'create' in command:
                    Path(command[-2]).write_bytes(b'synthetic signed bundle')
                if 'stdout' in kwargs and hasattr(kwargs['stdout'],'write'):
                    kwargs['stdout'].write('synthetic recipe result\n')
                return SimpleNamespace(returncode=code if ' just ' in ' '.join(command) else 0)
            transport=SimpleNamespace(instance_id='2',host='192.0.2.1',ssh=lambda command:[command],
                                      scp=lambda local,remote,download=False:['scp',str(local),remote])
            def output(command, **kwargs):
                if 'sha256sum' in ' '.join(command):
                    return 'b'*64+'  /target/debug/oxidex\n'
                if 'config' in command and 'gpg.ssh.allowedSignersFile' in command:
                    return str(signer)+'\n'
                if 'status' in command:
                    return source_status
                return 'a'*40+'\n'
            def download(instance,zone,project,binary,artifact,digest,**kwargs):
                artifact.write_bytes(b'verified synthetic binary')
            with patch.dict(os.environ,{'OXIDEX_REMOTE_SSH_KNOWN_HOSTS':'/synthetic/known'}), \
                 patch.object(remote_build.ssh_transport,'identity',return_value=('oxidex-uploader','/synthetic/key')), \
                 patch.object(remote_build.ssh_transport,'DirectTransport',return_value=transport), \
                 patch.object(remote_build,'verify_builder_admission',return_value={'admission_passed':True}), \
                 patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.99.0'}), \
                 patch.object(remote_build,'verify_signed_source'), \
                 patch('qualification_source.verify_source'), \
                 patch.object(remote_build,'make_snapshot',side_effect=snapshot), \
                 patch.object(remote_build,'retrieve_perl_candidate',
                              side_effect=RuntimeError('synthetic transfer loss') if retrieval_fail else None,
                              return_value={'status':'unapproved_candidate_retrieved'}) as retrieve, \
                 patch.object(remote_build,'verify_remote_toolchain'), \
                 patch.object(remote_build.subprocess,'check_output',side_effect=output), \
                 patch.object(remote_build,'download_artifact',side_effect=download), \
                 patch.object(remote_build.subprocess,'run',side_effect=run):
                argv=['--source',str(root),'--instance','builder-vm','--zone','z',
                      '--instance-id','2','--worktree-id','checkout',
                      '--evidence-dir',str(root/'evidence'),'--just-recipe',recipe]
                if recipe=='test-package':
                    argv += ['--just-arg=package with spaces']
                if recipe=='freeze-linux-perl':
                    argv += ['--just-arg='+'a'*40,'--just-arg='+producer_tree]
                if code:
                    with self.assertRaisesRegex(RuntimeError,'recipe failed'):
                        remote_build.main(argv)
                elif source_status and recipe=='freeze-linux-perl':
                    with self.assertRaisesRegex(RuntimeError,'clean exact-HEAD'):
                        remote_build.main(argv)
                elif producer_tree!='a'*40 and recipe=='freeze-linux-perl':
                    with self.assertRaisesRegex(RuntimeError,'selected signed HEAD/tree'):
                        remote_build.main(argv)
                elif retrieval_fail and recipe=='freeze-linux-perl':
                    with self.assertRaisesRegex(RuntimeError,'synthetic transfer loss'):
                        remote_build.main(argv)
                else:
                    self.assertEqual(remote_build.main(argv),0)
                    if recipe=='freeze-linux-perl':
                        retrieve.assert_called_once()
            receipt=json.loads((root/'evidence/remote-build.json').read_text())
            return receipt,commands,root,extras_seen,signed_heads

    def test_success_runs_exact_recipe_without_fetch_or_cleanup_and_retains_targets(self):
        receipt,commands,_,_,_=self.exercise(0)
        joined='\n'.join(map(str,commands))
        self.assertIn("just test-package 'package with spaces'",joined)
        self.assertNotIn('cargo fetch',joined)
        self.assertNotIn('cleanup',joined)
        self.assertEqual(receipt['stage'],'complete_retained')
        self.assertEqual(receipt['recipe_state'],'DIRECT_SUCCESS_RETAINED')
        self.assertTrue(receipt['remote_retained'])
        self.assertEqual(receipt['recipe_log_bytes'],24)
        self.assertIn('source',receipt['remote_paths'])
        self.assertIn('target',receipt['remote_paths'])

    def test_docs_build_receipt_names_retained_remote_rustdoc_directory(self):
        receipt, commands, _, _, _ = self.exercise(0, recipe='docs-build')
        self.assertEqual(receipt['rustdoc_path'],
                         receipt['remote_paths']['target'] + '/doc')
        self.assertEqual(receipt['recipe_state'], 'DIRECT_SUCCESS_RETAINED')
        self.assertFalse(any('cleanup' in ' '.join(command) for command in commands))

    def test_ssh_255_retains_unknown_and_never_retries_or_cleans(self):
        receipt,commands,_,_,_=self.exercise(255)
        self.assertEqual(receipt['recipe_state'],'UNKNOWN_RETAINED')
        self.assertEqual(receipt['recipe_exit_code'],255)
        self.assertFalse(receipt['retryable'])
        self.assertTrue(receipt['remote_retained'])
        self.assertEqual(sum(' just ' in ' '.join(c) for c in commands),1)
        self.assertFalse(any('cleanup' in ' '.join(c) for c in commands))

    def test_missing_explicit_uploader_refuses_before_provider_or_process(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(remote_build.ssh_transport,'identity',return_value=None), \
             patch.object(remote_build.subprocess,'run') as run, \
             patch.object(remote_build.subprocess,'check_output') as output:
            root=Path(directory)
            with self.assertRaisesRegex(ValueError,'explicit uploader'):
                remote_build.main(['--source',str(root),'--instance','builder-vm','--zone','z',
                    '--instance-id','2','--worktree-id','checkout','--evidence-dir',str(root/'evidence'),
                    '--just-recipe','build'])
            run.assert_not_called();output.assert_not_called()

    def test_build_downloads_checked_linux_binary_and_retains_full_target(self):
        receipt,commands,root,_,_=self.exercise(0,'build')
        self.assertEqual(receipt['binary_sha256'],'b'*64)
        self.assertIn('/target/remote-linux/debug/',receipt['artifact'])
        self.assertTrue(receipt['remote_retained'])
        self.assertFalse(any('cleanup' in ' '.join(c) for c in commands))

    def test_fleet_recipe_carries_only_signed_head_history(self):
        receipt,commands,_,extras,heads=self.exercise(0,'fleet-test')
        self.assertEqual(set(extras), {'repository.bundle', 'maintainer.allowed_signers',
                                       'fleet-source-head'})
        self.assertEqual(heads, ['a'*40])
        self.assertEqual(receipt['fleet_source_bundle_sha256'],
                         remote_build.hashlib.sha256(b'synthetic signed bundle').hexdigest())
        self.assertTrue(any('bundle' in command and 'create' in command
                            and command[-1] == 'HEAD' for command in commands))
        self.assertNotIn('--all', '\n'.join(map(str, commands)))

    def test_perl_producer_uses_signed_packet_and_refuses_dirty_snapshot(self):
        receipt,_,_,extras,heads=self.exercise(0,'freeze-linux-perl')
        self.assertEqual(heads,['a'*40])
        self.assertEqual(set(extras),{'repository.bundle','maintainer.allowed_signers','fleet-source-head'})
        self.assertEqual(receipt['candidate_retrieval']['status'],'unapproved_candidate_retrieved')
        refused,commands,_,_,_=self.exercise(0,'freeze-linux-perl',source_status=' M justfile\n')
        self.assertIn('clean exact-HEAD',refused['error'])
        self.assertFalse(any(' just ' in ' '.join(command) for command in commands))
        mismatch,commands,_,_,_=self.exercise(0,'freeze-linux-perl',producer_tree='b'*40)
        self.assertIn('selected signed HEAD/tree',mismatch['error'])
        self.assertFalse(any(' just ' in ' '.join(command) for command in commands))
        lost,_,_,_,_=self.exercise(0,'freeze-linux-perl',retrieval_fail=True)
        self.assertEqual(lost['stage'],'verify')
        self.assertTrue(lost['remote_retained'])
        self.assertNotIn('verified',lost)

    def test_snapshot_carries_bundle_bytes_with_source_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            source=root/'source';source.mkdir()
            (source/'tracked.txt').write_text('tracked')
            bundle=root/'repository.bundle';bundle.write_bytes(b'signed bundle')
            archive=root/'source.tar.gz'
            with patch.object(remote_build,'eligible_snapshot_paths',return_value=['tracked.txt']):
                snapshot=remote_build.make_snapshot(source,archive,
                    extra_files={'repository.bundle':bundle})
            with tarfile.open(archive,'r:gz') as stream:
                self.assertEqual(stream.getnames(),['tracked.txt','repository.bundle'])
                self.assertEqual(stream.extractfile('repository.bundle').read(),b'signed bundle')
            self.assertEqual(snapshot['files'][-1]['sha256'],
                             remote_build.hashlib.sha256(b'signed bundle').hexdigest())

    def test_signed_fleet_packet_uses_verified_head_and_rejects_hidden_edits(self):
        if shutil.which('ssh-keygen') is None:
            self.skipTest('ssh-keygen is unavailable')
        import qualification_source
        from qualification_bootstrap import verify_staged_checkout
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            source=root/'source';source.mkdir()
            key=root/'signing-key'
            subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(key)],check=True)
            public=' '.join((root/'signing-key.pub').read_text().split()[:2])
            fingerprint=subprocess.check_output(['ssh-keygen','-lf',str(key)+'.pub'],text=True).split()[1]
            signers=root/'signers'
            signers.write_text(qualification_source.PRINCIPAL+' '+public+'\n')
            def git(*args):
                return subprocess.check_output(['git','-C',str(source),*args],text=True).strip()
            git('init','-q')
            for name,value in {'.exiftool-version':'13.59\n',
                               'rust-toolchain.toml':'[toolchain]\nchannel = "1.97.1"\n',
                               'justfile':'fleet-test:\n  echo signed\n',
                               'tools/remote-build/route.py':'print("signed")\n',
                               'tools/remote-build/qualification_bootstrap.py':'# signed\n',
                               'tools/remote-build/qualification_source.py':'# signed\n',
                               'tools/remote-build/test_runner.py':'# signed\n',
                               'tools/release/bootstrap_oracle.py':'# signed\n'}.items():
                path=source/name;path.parent.mkdir(parents=True,exist_ok=True)
                path.write_text(value)
            git('config','user.name','swackhamer')
            git('config','user.email',qualification_source.PRINCIPAL)
            git('config','gpg.format','ssh')
            git('config','user.signingkey',str(key))
            git('config','gpg.ssh.allowedSignersFile',str(signers))
            git('add','.')
            git('commit','-q','-S','-m','Route SWF')
            head=git('rev-parse','HEAD')
            bundle=root/'repository.bundle'
            git('bundle','create',str(bundle),'HEAD')
            checkout=root/'checkout'
            subprocess.run(['git','clone','-q',str(bundle),str(checkout)],check=True)
            subprocess.run(['git','-C',str(checkout),'checkout','-q','--detach',head],check=True)
            with patch.object(qualification_source,'KEY',public), \
                 patch.object(qualification_source,'FINGERPRINT',fingerprint):
                remote_build.verify_signed_source(source,head)
                verified=verify_staged_checkout(checkout,head,signers,None,
                    remote_build.hashlib.sha256(bundle.read_bytes()).hexdigest())
                self.assertEqual(verified['head'],head)
                archive=root/'snapshot.tar.gz'
                snapshot=remote_build.make_snapshot(source,archive,signed_head=head)
                with tarfile.open(archive,'r:gz') as stream:
                    self.assertEqual(stream.extractfile('justfile').read(),
                                     b'fleet-test:\n  echo signed\n')
                self.assertEqual(snapshot['file_count'],8)
                git('update-index','--skip-worktree','justfile')
                (source/'justfile').write_text('fleet-test:\n  echo unsigned\n')
                self.assertEqual(git('status','--porcelain','--untracked-files=all'),'')
                with self.assertRaisesRegex(RuntimeError,'differs from signed HEAD: justfile'):
                    remote_build.make_snapshot(source,archive,signed_head=head)
                transport=SimpleNamespace(instance_id='2',host='192.0.2.1',
                                          ssh=lambda _: self.fail('remote execution started'))
                with patch.dict(os.environ,{'OXIDEX_REMOTE_SSH_KNOWN_HOSTS':'fixture'}), \
                     patch.object(remote_build.ssh_transport,'identity',
                                  return_value=('fixture-uploader','fixture-key')), \
                     patch.object(remote_build.ssh_transport,'DirectTransport',return_value=transport), \
                     patch.object(remote_build,'verify_builder_admission',
                                  return_value={'admission_passed':True}), \
                     patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.97.1'}):
                    with self.assertRaisesRegex(RuntimeError,'differs from signed HEAD: justfile'):
                        remote_build.main(['--source',str(source),'--instance','builder-vm',
                            '--zone','fixture','--instance-id','2','--worktree-id','fixture',
                            '--evidence-dir',str(root/'evidence'),'--just-recipe','fleet-test'])
                self.assertEqual(json.loads((root/'evidence/remote-build.json').read_text())['stage'],
                                 'local_toolchain')
                (source/'justfile').write_text('fleet-test:\n  echo signed\n')
                git('update-index','--no-skip-worktree','justfile')
                git('update-index','--assume-unchanged','tools/remote-build/route.py')
                (source/'tools/remote-build/route.py').write_text('print("unsigned")\n')
                self.assertEqual(git('status','--porcelain','--untracked-files=all'),'')
                with self.assertRaisesRegex(RuntimeError,'differs from signed HEAD: tools/remote-build/route.py'):
                    remote_build.make_snapshot(source,archive,signed_head=head)
                (source/'tools/remote-build/route.py').write_text('print("signed")\n')
                git('update-index','--no-assume-unchanged','tools/remote-build/route.py')
                git('update-index','--skip-worktree','justfile')
                (source/'justfile').chmod(0o755)
                self.assertEqual(git('status','--porcelain','--untracked-files=all'),'')
                with self.assertRaisesRegex(RuntimeError,'mode differs from signed HEAD: justfile'):
                    remote_build.make_snapshot(source,archive,signed_head=head)
