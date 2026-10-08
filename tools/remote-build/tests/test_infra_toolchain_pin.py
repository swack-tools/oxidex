"""External pins must be authenticated bounded Git blobs, not caller file reads."""
import json
import os
from pathlib import Path
import runpy
import signal
import subprocess
import sys
import tempfile
import unittest
from contextlib import ExitStack, contextmanager
from types import SimpleNamespace
from unittest.mock import patch
from lib import remote_build, infra_repository_binding as binding
import qualification_source


class ExternalPinTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve(); self.source = self.root/'source'; self.source.mkdir()
        self.git('init', '-q')
        self.key = self.root/'key'
        subprocess.run(['/usr/bin/ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(self.key)], check=True)
        self.public = ' '.join(self.key.with_suffix('.pub').read_text().split()[:2])
        self.fingerprint = subprocess.check_output(['/usr/bin/ssh-keygen', '-lf', str(self.key)+'.pub'], text=True).split()[1]
        signers = self.root/'signers'; signers.write_text(qualification_source.PRINCIPAL+' '+self.public+'\n')
        for name, value in [('user.name','swackhamer'),('user.email',qualification_source.PRINCIPAL),
                            ('gpg.format','ssh'),('user.signingkey',str(self.key)),
                            ('gpg.ssh.allowedSignersFile',str(signers)),('commit.gpgsign','false')]:
            self.git('config', name, value)
        self.git('remote','add','origin','git@github.com:swack-tools/spot-github-runners.git')
        for name in remote_build.INFRA_PYTHON_REQUIRED:
            p=self.source/name; p.parent.mkdir(parents=True,exist_ok=True); p.write_text('fixture\n')
        self.pin=self.source/'rust-toolchain.toml'; self.pin.write_text('[toolchain]\nchannel = "1.99.0"\n')
        self.git('add','.'); self.git('commit','-qm','unsigned source')
        self.counter=0

    def git(self,*args):
        return subprocess.check_output(['/usr/bin/git','-C',str(self.source),*args],env=remote_build.source_git_env(),text=True).strip()

    @contextmanager
    def bounded(self):
        old=signal.signal(signal.SIGALRM,lambda *_: (_ for _ in ()).throw(TimeoutError('external pin read blocked')))
        signal.setitimer(signal.ITIMER_REAL,3)
        try: yield
        finally: signal.setitimer(signal.ITIMER_REAL,0); signal.signal(signal.SIGALRM,old)

    def invoke(self, direct=False):
        self.counter+=1
        evidence=self.root/('evidence-'+str(self.counter))
        transport=SimpleNamespace(instance_id='2',host='192.0.2.1',ssh=lambda _:['never-remote'])
        def api(endpoint, token):
            return json.dumps({'/user':{'login':'swackhamer'},binding.REPO_ENDPOINT:{'id':1397480856,'full_name':binding.REPOSITORY,'default_branch':'main','fork':False,'archived':False},binding.REF_ENDPOINT:{'ref':'refs/heads/main','object':{'type':'commit','sha':self.git('rev-parse','HEAD')}}}[endpoint]).encode()
        real_output=subprocess.check_output
        def tools(argv,*a,**kw):
            if argv[:1]==['rustup']: return '/fixture/'+argv[-1]+'\n'
            if argv==['/fixture/rustc','-vV']: return 'release: 1.99.0\ncommit-hash: '+'a'*40+'\n'
            if argv==['/fixture/cargo','-V']: return 'cargo 1.99.0 (fixture)\n'
            return real_output(argv,*a,**kw)
        with ExitStack() as stack:
            for target,name,value in [(qualification_source,'KEY',self.public),(qualification_source,'FINGERPRINT',self.fingerprint)]:stack.enter_context(patch.object(target,name,value))
            stack.enter_context(patch.dict(os.environ,{'OXIDEX_REMOTE_SSH_KNOWN_HOSTS':'fixture','OXIDEX_REMOTE_INSTANCE':'builder-fixture','OXIDEX_REMOTE_ZONE':'z','OXIDEX_REMOTE_PROJECT':'p','OXIDEX_REMOTE_INSTANCE_ID':'2','OXIDEX_REMOTE_WORKTREE':'fixture'}))
            stack.enter_context(patch.object(remote_build.ssh_transport,'identity',return_value=('uploader','key')))
            stack.enter_context(patch.object(remote_build.ssh_transport,'DirectTransport',return_value=transport))
            stack.enter_context(patch.object(remote_build,'verify_builder_admission',return_value={'admission_passed':True}))
            stack.enter_context(patch.object(binding,'_maintainer_token',return_value='fixture'))
            stack.enter_context(patch.object(binding,'_api',side_effect=api))
            stack.enter_context(patch.object(subprocess,'check_output',side_effect=tools))
            stack.enter_context(patch.object(remote_build,'INFRA_PYTHON_MIN_MODULES',1))
            stack.enter_context(patch.object(remote_build,'make_snapshot',side_effect=RuntimeError('signed-pin-reached-snapshot')))
            if direct:
                stack.enter_context(patch('lib.config.configured_remote_env',return_value={}))
                stack.enter_context(patch.object(sys,'argv',['direct.py','--source',str(self.source),'--source-profile','infra-python-v1','--just-recipe','infra-python-tests','--evidence-dir',str(evidence)]))
                runpy.run_path(str(Path(remote_build.__file__).parents[1]/'direct.py'),run_name='__main__')
            else:
                remote_build.main(['--source',str(self.source),'--source-profile','infra-python-v1','--just-recipe','infra-python-tests','--instance','builder-fixture','--zone','z','--instance-id','2','--worktree-id','fixture','--evidence-dir',str(evidence)])

    def test_unsigned_fifo_refuses_without_open_in_both_entrypoints(self):
        self.pin.unlink(); os.mkfifo(self.pin)
        for direct in (False,True):
            with self.subTest(direct=direct),self.bounded(),self.assertRaisesRegex(RuntimeError,'signed maintainer HEAD'):
                self.invoke(direct)

    def test_unsigned_large_declared_file_is_not_read_before_authentication(self):
        with self.pin.open('wb') as f:f.truncate(1024**3)
        original=Path.read_text
        def read(path,*a,**kw):
            if path==self.pin:raise AssertionError('unadmitted pin read attempted')
            return original(path,*a,**kw)
        for direct in (False,True):
            with self.subTest(direct=direct),patch.object(Path,'read_text',read),self.assertRaisesRegex(RuntimeError,'signed maintainer HEAD'):
                self.invoke(direct)

    def test_signed_valid_pin_and_bounded_refusals(self):
        self.git('commit','--amend','-S','-qm','signed source')
        original=Path.read_text
        def read(path,*a,**kw):
            if path==self.pin:raise AssertionError('signed pin parsed from worktree')
            return original(path,*a,**kw)
        for direct in (False,True):
            with self.subTest(direct=direct),patch.object(Path,'read_text',read),self.assertRaisesRegex(RuntimeError,'signed-pin-reached-snapshot'):
                self.invoke(direct)
        with self.pin.open('wb') as f:f.truncate(1024**3)
        with self.bounded(),self.assertRaisesRegex(RuntimeError,'Toolchain pin worktree'):
            self.invoke()
        self.pin.unlink();os.mkfifo(self.pin)
        with self.bounded(),self.assertRaisesRegex(RuntimeError,'Toolchain pin worktree'):
            self.invoke()
        self.pin.unlink();self.pin.write_text('#'+'x'*16384+'\n')
        self.git('add','rust-toolchain.toml');self.git('commit','-S','-qm','oversized signed pin')
        with self.assertRaisesRegex(RuntimeError,'Toolchain pin blob.*bound'):
            self.invoke()


if __name__=='__main__':unittest.main()
