"""Small component-proof worker boundaries; no real archive or Perl execution."""
from pathlib import Path
import tempfile
import shutil
import json
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

from tools.release import approved_linux_perl as approved
from tools.release import bootstrap_oracle as bootstrap
from tools.release import prove_linux_perl_component as worker


class ComponentWorkerTests(unittest.TestCase):
    def test_signed_worker_requires_approved_envelope_before_install_or_probe(self):
        with tempfile.TemporaryDirectory() as directory:
            base=Path(directory).resolve()
            checkout=base/'checkout';checkout.mkdir()
            source=base/'source';source.mkdir()
            target=base/'target';target.mkdir()
            envelope=source/'approved-linux-perl.json';envelope.write_text('{}')
            (source/'fleet-source-head').write_text('a'*40+'\n')
            (source/'repository.bundle').write_bytes(b'signed bundle')
            release=checkout/'tools/release';release.mkdir(parents=True)
            (release/'oracle-lock.json').write_text('{}')
            (release/'oracle-linux-perl-identity.json').write_text('{}')
            run_id='component-'+'1'*32
            def git(*args):
                return {'HEAD':'a'*40,'HEAD^{tree}':'b'*40}.get(args[-1],'')
            patches=(mock.patch.object(worker,'ROOT',checkout),
                     mock.patch.object(worker,'FLEET_CHECKOUT',checkout),
                     mock.patch.object(worker,'SOURCE',source),
                     mock.patch.object(worker,'TARGET',target),
                     mock.patch.object(worker,'OPS',target/'ops'),
                     mock.patch.object(worker,'ENVELOPE',envelope),
                     mock.patch.object(worker,'PROOF',target/'linux-perl-component-proof.json'),
                     mock.patch.object(worker.Path,'cwd',return_value=checkout),
                     mock.patch.object(worker,'git',side_effect=git),
                     mock.patch.dict(sys.modules,{'route':SimpleNamespace(verified_fleet_checkout=lambda: True)}))
            with context(patches), mock.patch.object(bootstrap,'_materialize_perl') as install, \
                 mock.patch.object(bootstrap,'run') as probe, \
                 mock.patch.object(approved,'load',side_effect=approved.Refused('archive mismatch')):
                with self.assertRaisesRegex(approved.Refused,'archive mismatch'):
                    worker.main(run_id)
                install.assert_not_called();probe.assert_not_called()
                self.assertFalse((target/'ops').exists())
                self.assertFalse((target/'linux-perl-component-proof.json').exists())
            descriptor={'archive_sha256':'c'*64,'archive_bytes':17,
                        'zip_relative_path':'lib/Archive/Zip.pm'}
            prefix=target/'ops/toolchains/perl-5.38.2/prefix'
            prefix.mkdir(parents=True)
            perl=prefix/'bin/perl5.38.2';perl.parent.mkdir();perl.write_bytes(b'perl')
            zip_module=prefix/'lib/Archive/Zip.pm';zip_module.parent.mkdir(parents=True)
            zip_module.write_bytes(b'zip')
            # The actual worker refuses a reused namespace before any probe.
            with context(patches), mock.patch.object(approved,'load',return_value=(descriptor,b'archive')), \
                 mock.patch.object(bootstrap,'_materialize_perl') as install, \
                 mock.patch.object(bootstrap,'run') as probe:
                with self.assertRaisesRegex(RuntimeError,'fresh private target'):
                    worker.main(run_id)
                install.assert_not_called();probe.assert_not_called()
            shutil.rmtree(target/'ops')
            def install_phase(_root, _perl, _zip, _approval):
                if not prefix.exists():
                    prefix.mkdir(parents=True)
                    perl.parent.mkdir();perl.write_bytes(b'perl')
                    zip_module.parent.mkdir(parents=True);zip_module.write_bytes(b'zip')
                    return 'installed'
                return 'reused'
            with context(patches), mock.patch.object(approved,'load',return_value=(descriptor,b'archive')), \
                 mock.patch.object(bootstrap,'_materialize_perl',side_effect=install_phase) as install, \
                 mock.patch.object(bootstrap,'perl_prefix',return_value=prefix), \
                 mock.patch.object(bootstrap,'manifest_path',return_value=target/'ops/manifest.json'), \
                 mock.patch.object(bootstrap,'sha256_tree',return_value='d'*64), \
                 mock.patch.object(bootstrap,'staged_perl_environment',return_value={}), \
                 mock.patch.object(bootstrap,'run',side_effect=[str(prefix),'1.68']) as probe, \
                 mock.patch.object(approved,'check_tree'), \
                 mock.patch.object(approved,'sha',side_effect=lambda path: 'e'*64 if path==perl else 'f'*64):
                worker.main(run_id)
                self.assertEqual(install.call_count,2)
                self.assertEqual(probe.call_count,2)
            proof=json.loads((target/'linux-perl-component-proof.json').read_text())
            self.assertEqual(proof['kind'],'linux_perl_component_proof')
            self.assertEqual((proof['cold'],proof['warm']),('installed','reused'))
            self.assertEqual(proof['status'],'COMPONENT_ONLY_PASS')
            self.assertEqual(proof['source_head'],'a'*40)
            self.assertEqual(proof['archive_sha256'],'c'*64)
            self.assertEqual(proof['tree_sha256'],'d'*64)
            self.assertEqual(proof['exe_sha256'],'e'*64)
            self.assertEqual(proof['zip_sha256'],'f'*64)
            shutil.rmtree(target/'ops')
            (target/'linux-perl-component-proof.json').unlink()
            with context(patches), mock.patch.object(approved,'load',return_value=(descriptor,b'archive')), \
                 mock.patch.object(bootstrap,'_materialize_perl',return_value='installed') as install, \
                 mock.patch.object(bootstrap,'perl_prefix',return_value=prefix), \
                 mock.patch.object(bootstrap,'manifest_path',return_value=target/'ops/manifest.json'), \
                 mock.patch.object(bootstrap,'run') as probe:
                with self.assertRaisesRegex(RuntimeError,'warm phase'):
                    worker.main(run_id)
                self.assertEqual(install.call_count,2)
                probe.assert_not_called()
                self.assertFalse((target/'linux-perl-component-proof.json').exists())


def context(patches):
    from contextlib import ExitStack
    stack=ExitStack()
    for patcher in patches:
        stack.enter_context(patcher)
    return stack


if __name__ == '__main__':
    unittest.main()
