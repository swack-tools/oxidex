"""Real Git graph controls; only credential/HTTP seams are synthetic."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from lib import remote_build
from lib import infra_repository_binding as binding


class RepositoryBindingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.source = self.root / 'source'
        self.source.mkdir()
        self.git('init', '-q')
        self.git('config', 'user.name', 'swackhamer')
        self.git('config', 'user.email', 'swackhamer@users.noreply.github.com')
        self.git('config', 'commit.gpgsign', 'false')
        (self.source / 'file').write_text('anchor\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'anchor')
        self.anchor = self.git('rev-parse', 'HEAD')
        (self.source / 'file').write_text('unpublished\n')
        self.git('commit', '-qam', 'unpublished child')
        self.head = self.git('rev-parse', 'HEAD')
        self.tree = self.git('rev-parse', 'HEAD^{tree}')

    def git(self, *args, input=None):
        return subprocess.check_output(['/usr/bin/git', '-C', str(self.source), *args],
            input=input, env=remote_build.source_git_env(), stderr=subprocess.PIPE).decode().strip()

    def api(self, endpoint, token):
        values = {
            '/user': {'login':'swackhamer'},
            binding.REPO_ENDPOINT: {'id':1397480856, 'full_name':binding.REPOSITORY,
                'default_branch':'main', 'fork':False, 'archived':False},
            binding.REF_ENDPOINT: {'ref':'refs/heads/main', 'object':{'type':'commit','sha':self.anchor}},
        }
        return json.dumps(values[endpoint]).encode()

    def admit(self):
        with patch.object(binding, '_maintainer_token', return_value='fixture-token'), \
             patch.object(binding, '_api', side_effect=self.api):
            return binding.admit(self.source, self.head, self.tree, remote_build.source_git_env())

    def test_main_requires_repository_lineage_for_actual_signed_head(self):
        import qualification_source
        from types import SimpleNamespace
        key=self.root/'key'
        subprocess.run(['/usr/bin/ssh-keygen','-q','-t','ed25519','-N','','-f',str(key)],check=True)
        public=' '.join(key.with_suffix('.pub').read_text().split()[:2])
        fingerprint=subprocess.check_output(['/usr/bin/ssh-keygen','-lf',str(key)+'.pub'],text=True).split()[1]
        signer=self.root/'signers';signer.write_text(qualification_source.PRINCIPAL+' '+public+'\n')
        self.git('config','user.signingkey',str(key))
        self.git('config','gpg.ssh.allowedSignersFile',str(signer))
        self.git('remote','add','origin','git@github.com:swack-tools/spot-github-runners.git')
        for name in remote_build.INFRA_PYTHON_REQUIRED:
            path=self.source/name;path.parent.mkdir(parents=True,exist_ok=True)
            path.write_text('fixture\n')
        (self.source/'rust-toolchain.toml').write_text('[toolchain]\nchannel = "1.99.0"\n')
        self.git('add','.')
        self.git('commit','--amend','-q','-S','-m','unpublished signed child')
        self.head=self.git('rev-parse','HEAD');self.tree=self.git('rev-parse','HEAD^{tree}')
        transport=SimpleNamespace(instance_id='2',host='192.0.2.1',ssh=lambda x:['never-ssh',x])
        for unrelated in (False,True):
            if unrelated:
                self.head=self.git('commit-tree','-S',self.tree,input=b'unrelated signed source\n')
                self.git('update-ref','HEAD',self.head)
            with self.subTest(unrelated=unrelated), \
                 patch.object(qualification_source,'KEY',public), \
                 patch.object(qualification_source,'FINGERPRINT',fingerprint), \
                 patch.object(binding,'_maintainer_token',return_value='fixture'), \
                 patch.object(binding,'_api',side_effect=self.api), \
                 patch.dict(os.environ,{'OXIDEX_REMOTE_SSH_KNOWN_HOSTS':'fixture'}), \
                 patch.object(remote_build.ssh_transport,'identity',return_value=('uploader','key')), \
                 patch.object(remote_build.ssh_transport,'DirectTransport',return_value=transport), \
                 patch.object(remote_build,'verify_builder_admission',return_value={'admission_passed':True}), \
                 patch.object(remote_build,'pinned_toolchain',return_value={'channel':'1.99.0'}), \
                 patch.object(remote_build,'INFRA_PYTHON_MIN_MODULES',1), \
                 patch.object(remote_build,'make_snapshot',side_effect=RuntimeError('snapshot-sentinel')) as snapshot:
                with self.assertRaisesRegex(RuntimeError,'ancestry' if unrelated else 'snapshot-sentinel'):
                    remote_build.main(['--source',str(self.source),'--instance','builder-vm',
                        '--zone','z','--instance-id','2','--worktree-id','fixture',
                        '--evidence-dir',str(self.root/('unrelated' if unrelated else 'related')),
                        '--source-profile','infra-python-v1','--just-recipe','infra-python-tests'])
                self.assertEqual(snapshot.called,not unrelated)

    def test_unpublished_raw_descendant_and_exact_binding(self):
        record = self.admit()
        self.assertEqual(record['witness']['path'], [self.head, self.anchor])
        self.assertEqual(record['head'], self.head)
        self.assertEqual(record['anchor']['sha'], self.anchor)
        for endpoint, raw in record['anchor']['response_json'].items():
            self.assertEqual(hashlib.sha256(raw.encode()).hexdigest(),
                             record['anchor']['response_sha256'][endpoint])
        with patch.object(binding, '_maintainer_token', return_value='fixture-token'), \
             patch.object(binding, '_api', side_effect=self.api):
            binding.recheck(self.source, record, remote_build.source_git_env())
        bound = binding.bind_packet(record, 'fixture-run', 'a'*64, 'b'*64)
        binding.verify_packet(bound, self.head, self.tree, 'fixture-run', 'a'*64, 'b'*64)
        with self.assertRaisesRegex(RuntimeError, 'packet'):
            binding.verify_packet(bound, self.head, self.tree, 'other-run', 'a'*64, 'b'*64)

    def test_unrelated_origin_spoof_graft_replace_and_shallow_do_not_authorize(self):
        unrelated = self.git('commit-tree', self.tree, input=b'unrelated\n')
        self.git('update-ref', 'HEAD', unrelated)
        self.git('remote', 'add', 'origin', 'git@github.com:swack-tools/spot-github-runners.git')
        self.head = unrelated
        self.git('update-ref', 'refs/remotes/origin/main', unrelated)
        for mode in ['plain', 'graft', 'replace', 'shallow']:
            with self.subTest(mode=mode):
                if mode == 'graft':
                    (self.source/'.git/info/grafts').write_text(unrelated+' '+self.anchor+'\n')
                    graph = subprocess.run(['/usr/bin/git','-C',str(self.source),
                        'merge-base','--is-ancestor',self.anchor,unrelated],capture_output=True)
                    self.assertEqual(graph.returncode, 0)
                if mode == 'replace':
                    (self.source/'.git/info/grafts').unlink()
                    replacement = self.git('commit-tree', self.tree, '-p', self.anchor, input=b'fake parent\n')
                    self.git('replace', unrelated, replacement)
                if mode == 'shallow':
                    (self.source/'.git/shallow').write_text(unrelated+'\n')
                with self.assertRaisesRegex(RuntimeError, 'ancestry'):
                    self.admit()

    def test_identity_and_anchor_movement_refuse(self):
        for endpoint, override in [('/user', {'login':'other'}),
            (binding.REPO_ENDPOINT, {'id':1,'full_name':binding.REPOSITORY,'default_branch':'main'}),
            (binding.REF_ENDPOINT, {'ref':'refs/heads/other','object':{'type':'commit','sha':self.anchor}})]:
            with self.subTest(endpoint=endpoint), patch.object(binding,'_maintainer_token',return_value='fixture'), \
                 patch.object(binding,'_api',side_effect=lambda p,t:json.dumps(override).encode() if p==endpoint else self.api(p,t)):
                with self.assertRaises(RuntimeError):binding.observe()
        record = self.admit()
        self.anchor = 'a'*40
        with patch.object(binding,'_maintainer_token',return_value='fixture'), patch.object(binding,'_api',side_effect=self.api):
            with self.assertRaisesRegex(RuntimeError,'ANCHOR_MOVED'):
                binding.recheck(self.source,record,remote_build.source_git_env())

    def test_local_head_movement_refuses(self):
        record = self.admit()
        self.git('update-ref','HEAD',self.anchor)
        with patch.object(binding,'_maintainer_token',return_value='fixture'),patch.object(binding,'_api',side_effect=self.api):
            with self.assertRaisesRegex(RuntimeError,'candidate changed'):
                binding.recheck(self.source,record,remote_build.source_git_env())

    def test_raw_type_hash_missing_and_limits_refuse(self):
        env=remote_build.source_git_env()
        with self.assertRaisesRegex(RuntimeError,'commit object'):
            binding.witness(self.source,self.tree,self.anchor,env)
        with self.assertRaises(RuntimeError):binding.witness(self.source,'f'*40,self.anchor,env)
        with patch.object(binding,'MAX_COMMITS',1):
            with self.assertRaisesRegex(RuntimeError,'limit'):binding.witness(self.source,self.head,self.anchor,env)
        with patch.object(binding,'MAX_TOTAL_BYTES',1):
            with self.assertRaisesRegex(RuntimeError,'limit'):binding.witness(self.source,self.head,self.anchor,env)
        with patch.object(binding,'MAX_COMMIT_BYTES',1):
            with self.assertRaisesRegex(RuntimeError,'limit'):binding.witness(self.source,self.head,self.anchor,env)
        with patch.object(binding,'MAX_GRAPH_SECONDS',0):
            with self.assertRaisesRegex(RuntimeError,'deadline'):binding.witness(self.source,self.head,self.anchor,env)
        raw = subprocess.check_output(['/usr/bin/git','-C',str(self.source),'cat-file','commit',self.head],env=env)
        with self.assertRaisesRegex(RuntimeError,'hash'):binding.parse_commit(self.anchor,raw)
        forged = b'tree '+self.tree.encode()+b'\nparent nope\n\nmessage\n'
        oid=hashlib.sha1(b'commit '+str(len(forged)).encode()+b'\0'+forged).hexdigest()
        with self.assertRaisesRegex(RuntimeError,'parent'):binding.parse_commit(oid,forged)

    def test_message_and_signature_continuations_are_not_parents(self):
        raw=(b'tree '+self.tree.encode()+b'\nauthor A <a@b> 0 +0000\ncommitter A <a@b> 0 +0000\n'
             b'gpgsig fake\n parent '+self.anchor.encode()+b'\n\nparent '+self.anchor.encode()+b'\n')
        oid=hashlib.sha1(b'commit '+str(len(raw)).encode()+b'\0'+raw).hexdigest()
        self.assertEqual(binding.parse_commit(oid,raw)['parents'],[])
        for parents in [[self.anchor,self.anchor], [f'{i:040x}' for i in range(33)]]:
            raw=b'tree '+self.tree.encode()+b'\n'+b''.join(b'parent '+p.encode()+b'\n' for p in parents)+b'\nm\n'
            oid=hashlib.sha1(b'commit '+str(len(raw)).encode()+b'\0'+raw).hexdigest()
            with self.assertRaisesRegex(RuntimeError,'parent'):binding.parse_commit(oid,raw)

    def test_authority_credentials_and_http_are_fixed_and_secret_safe(self):
        secret='fixture-secret-token'
        with patch.dict(os.environ,{'GH_TOKEN':'attacker','GITHUB_TOKEN':'other',
                'GH_HOST':'attacker.invalid','GH_CONFIG_DIR':'/attacker','HTTP_PROXY':'http://attacker'}), \
             patch.object(binding,'_trusted_tool',return_value='/trusted/tool'), \
             patch.object(binding,'_bounded_command',return_value=secret.encode()) as command, \
             patch.object(Path,'exists',return_value=True):
            self.assertEqual(binding._maintainer_token(),secret)
            argv,env,*_=command.call_args.args
            self.assertEqual(argv[1:],['auth','token','--hostname','github.com','--user','swackhamer'])
            self.assertFalse(any(k.startswith(('GH_TOKEN','GITHUB_','HTTP_')) for k in env))
            self.assertNotIn('GH_CONFIG_DIR',env)
        with patch.object(binding,'_trusted_tool',return_value='/usr/bin/curl'), \
             patch.object(binding,'_bounded_command',return_value=b'{}\n200') as command:
            self.assertEqual(binding._api('/user',secret),b'{}')
            argv,env,*_=command.call_args.args
            self.assertEqual(argv[-1],'https://api.github.com/user')
            self.assertEqual(argv[1],'-q')
            self.assertNotIn(secret,str(argv));self.assertNotIn(secret,str(env))
            self.assertIn(secret.encode(),command.call_args.kwargs['data'])
            self.assertNotIn('--location',argv);self.assertNotIn('-L',argv)
            self.assertEqual(argv[argv.index('--max-redirs')+1],'0')
        with patch.object(binding,'_trusted_tool',return_value='/usr/bin/curl'), \
             patch.object(binding,'_bounded_command',side_effect=RuntimeError('tool refused')):
            with self.assertRaisesRegex(RuntimeError,'tool refused') as error:binding._api('/user',secret)
            self.assertNotIn(secret,str(error.exception))

    def test_only_standard_macos_admin_cellar_may_be_group_writable(self):
        from types import SimpleNamespace
        import stat
        target=Path('/opt/homebrew/Cellar/gh/2.102.0/bin/gh')
        cellar=Path('/opt/homebrew/Cellar')
        base={p:SimpleNamespace(st_mode=stat.S_IFDIR|0o755,st_uid=0,st_gid=0)
              for p in target.parents}
        base[target]=SimpleNamespace(st_mode=stat.S_IFREG|0o555,st_uid=os.geteuid(),st_gid=80)
        base[cellar]=SimpleNamespace(st_mode=stat.S_IFDIR|0o775,st_uid=os.geteuid(),st_gid=80)
        controls=[('approved',None,None,False),
                  ('other group',cellar,dict(st_gid=20),True),
                  ('world writable',cellar,dict(st_mode=stat.S_IFDIR|0o777),True),
                  ('binary writable',target,dict(st_mode=stat.S_IFREG|0o775),True),
                  ('package writable',target.parent,dict(st_mode=stat.S_IFDIR|0o775,st_gid=80),True),
                  ('wrong owner',cellar,dict(st_uid=999999),True)]
        for name,changed,values,refuses in controls:
            rows=dict(base)
            if changed:rows[changed]=SimpleNamespace(**(vars(rows[changed])|values))
            with self.subTest(name=name),patch.object(Path,'resolve',return_value=target), \
                 patch.object(Path,'lstat',lambda p:rows[p]),patch.object(Path,'stat',lambda p:rows[p]), \
                 patch.object(binding.sys,'platform','darwin'), \
                 patch.object(binding.grp,'getgrgid',return_value=SimpleNamespace(gr_name='admin')):
                if refuses:
                    with self.assertRaisesRegex(RuntimeError,'Untrusted'):
                        binding._trusted_tool('/opt/homebrew/bin/gh',user_owned=True)
                else:
                    self.assertEqual(binding._trusted_tool('/opt/homebrew/bin/gh',user_owned=True),str(target))
        with patch.object(Path,'resolve',return_value=target), \
             patch.object(Path,'lstat',lambda p:base[p]),patch.object(Path,'stat',lambda p:base[p]), \
             patch.object(binding.sys,'platform','darwin'), \
             patch.object(binding.grp,'getgrgid',return_value=SimpleNamespace(gr_name='untrusted')):
            with self.assertRaisesRegex(RuntimeError,'Untrusted'):
                binding._trusted_tool('/opt/homebrew/bin/gh',user_owned=True)
        with patch.object(Path,'resolve',return_value=Path('/attacker/bin/gh')):
            with self.assertRaisesRegex(RuntimeError,'location'):
                binding._trusted_tool('/opt/homebrew/bin/gh',user_owned=True)

    def test_bounded_subprocess_output_and_deadline(self):
        import sys
        env={'PATH':'/usr/bin:/bin'}
        with self.assertRaisesRegex(RuntimeError,'output limit'):
            binding._bounded_command([sys.executable,'-c','print("x"*100)'],env,10,2)
        with self.assertRaises(RuntimeError):
            binding._bounded_command([sys.executable,'-c','import time; time.sleep(1)'],env,10,.05)

    def test_json_and_transport_bounds(self):
        for raw in [b'{"login":"swackhamer","login":"other"}',b'{"x":NaN}',b'{"x":1e999}',b'[]',b'x'*(binding.MAX_API_BYTES+1)]:
            with self.assertRaises(RuntimeError):binding._json(raw)
        with patch.object(binding,'_bounded_command',return_value=b'{}\n302'):
            with self.assertRaisesRegex(RuntimeError,'HTTP'):binding._api('/user','fixture')
        with self.assertRaises(RuntimeError):binding._api('https://attacker.invalid','fixture')
        with self.assertRaises(RuntimeError):binding._api('/user','bad\nsecret')


if __name__ == '__main__':unittest.main()
