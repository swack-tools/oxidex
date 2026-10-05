import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from lib import ssh_transport

class AuthenticationTests(unittest.TestCase):
    def test_missing_key_refuses_before_ambient_fallback(self):
        with patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_USER':'oxidex-uploader',
                'OXIDEX_REMOTE_SSH_KEY':'/missing/key'}, clear=True):
            with self.assertRaisesRegex(ValueError, 'readable'):
                ssh_transport.target('builder-test')

    def test_partial_identity_is_refused(self):
        with patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_USER':'oxidex-uploader'}, clear=True):
            with self.assertRaisesRegex(ValueError, 'both'):
                ssh_transport.flags('ssh')

    def test_both_transports_bind_same_explicit_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            key=Path(directory)/'key';key.write_text('test fixture')
            with patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_USER':'oxidex-uploader',
                    'OXIDEX_REMOTE_SSH_KEY':str(key)}, clear=True):
                self.assertEqual(ssh_transport.target('builder-test'),'oxidex-uploader@builder-test')
                for kind in ('ssh','scp'):
                    flags=ssh_transport.flags(kind)
                    self.assertIn('--ssh-key-file='+str(key),flags)
                    self.assertIn('--'+kind+'-flag=-F/dev/null',flags)
                    self.assertIn('--'+kind+'-flag=-oIdentityAgent=none',flags)
                    self.assertIn('--'+kind+'-flag=-oIdentitiesOnly=yes',flags)

    def test_invalid_user_is_refused(self):
        with patch.dict(os.environ, {'OXIDEX_REMOTE_SSH_USER':'bad@other-host',
                'OXIDEX_REMOTE_SSH_KEY':'/some/key'}, clear=True):
            with self.assertRaisesRegex(ValueError, 'both'):
                ssh_transport.identity()

class DirectTransportTests(unittest.TestCase):
    def fixture(self, directory):
        key=Path(directory)/'key';key.write_text('fixture')
        known=Path(directory)/'known_hosts';known.write_text('192.0.2.1 ssh-ed25519 fixture')
        return {'OXIDEX_REMOTE_SSH_USER':'oxidex-uploader',
                'OXIDEX_REMOTE_SSH_KEY':str(key),
                'OXIDEX_REMOTE_SSH_KNOWN_HOSTS':str(known)}

    def test_pre_enrolled_transport_never_invokes_metadata_enrollment(self):
        import json
        actual={'id':'123','status':'RUNNING','networkInterfaces':[
            {'accessConfigs':[{'natIP':'192.0.2.1'}]}]}
        with tempfile.TemporaryDirectory() as directory, \
             patch.dict(os.environ,self.fixture(directory),clear=True), \
             patch.object(ssh_transport.subprocess,'check_output',return_value=json.dumps(actual)) as call:
            transport=ssh_transport.DirectTransport('builder-test','z','p','123')
            self.assertEqual(call.call_args.args[0][:4],['gcloud','compute','instances','describe'])
            commands=[transport.ssh('true'),transport.scp('/local','~/upload'),
                      transport.scp('/local','/binary',download=True)]
            self.assertEqual([c[0] for c in commands],['ssh','scp','scp'])
            for command in commands:
                self.assertIn('StrictHostKeyChecking=yes',command)
                self.assertIn('GlobalKnownHostsFile=/dev/null',command)
                self.assertIn('IdentityAgent=none',command)
                self.assertIn('BatchMode=yes',command)
                self.assertNotIn('gcloud',command)
            self.assertEqual(commands[0][-2],'oxidex-uploader@192.0.2.1')
            self.assertEqual(commands[2][-2],'oxidex-uploader@192.0.2.1:/binary')

    def test_replaced_provider_identity_refuses_before_ssh(self):
        import json
        with tempfile.TemporaryDirectory() as directory, \
             patch.dict(os.environ,self.fixture(directory),clear=True), \
             patch.object(ssh_transport.subprocess,'check_output',return_value=json.dumps({'id':'124','status':'RUNNING'})):
            with self.assertRaisesRegex(RuntimeError,'identity changed'):
                ssh_transport.DirectTransport('builder-test','z','p','123')

    def test_missing_trust_file_refuses_before_provider_call(self):
        with tempfile.TemporaryDirectory() as directory:
            env=self.fixture(directory);env.pop('OXIDEX_REMOTE_SSH_KNOWN_HOSTS')
            with patch.dict(os.environ,env,clear=True), \
                 patch.object(ssh_transport.subprocess,'check_output') as call:
                with self.assertRaisesRegex(ValueError,'trusted'):
                    ssh_transport.DirectTransport('builder-test','z','p','123')
                call.assert_not_called()
