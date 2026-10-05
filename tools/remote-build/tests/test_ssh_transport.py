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
