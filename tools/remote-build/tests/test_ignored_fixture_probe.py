"""Synthetic controls for ignored-input admission; no Cargo or native oracle."""
import hashlib
from concurrent.futures import ThreadPoolExecutor
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch
import zipfile
from urllib.error import URLError
from urllib.request import Request

import ignored_fixture_probe as probe
import route


class IgnoredFixtureProbeTests(unittest.TestCase):
    def test_relative_path_rejects_escape_and_absolute_target(self):
        for name in ('../escape', 'a/../escape', '/tmp/input', '.', 'a//b'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                probe.relative_path(name)
        self.assertEqual(probe.relative_path('tests/fixtures/real/canon.cr2'),
                         Path('tests/fixtures/real/canon.cr2'))

    def test_cached_payload_rechecks_publisher_digest(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            row = {'id': 'raw-sony', 'source': {'kind': 'download', 'url': 'https://example.org/a.arw'},
                   'sha256': hashlib.sha256(b'authentic').hexdigest(), 'max_bytes': 100,
                   'required_tags': ['Make', 'Model', 'FileType']}
            (cache / 'raw-sony.source').write_bytes(b'altered')
            with patch.object(probe, 'download') as fetch:
                with self.assertRaisesRegex(ValueError, 'SHA-256 mismatch'):
                    probe.materialize(row, cache)
            fetch.assert_not_called()

    def test_each_redirect_hop_must_remain_https(self):
        redirect = probe.HTTPSOnlyRedirect()
        request = Request('https://source.example/fixture')
        first = redirect.redirect_request(request, None, 302, 'redirect', {},
                                          'https://mirror.example/stage')
        self.assertEqual(first.full_url, 'https://mirror.example/stage')
        with self.assertRaisesRegex(URLError, 'outside HTTPS'):
            redirect.redirect_request(first, None, 302, 'redirect', {},
                                      'http://middle.example/fixture')
        with self.assertRaisesRegex(URLError, 'outside HTTPS'):
            redirect.redirect_request(request, None, 302, 'redirect', {},
                                      'ftp://middle.example/fixture')

    def test_total_deadline_kills_slow_trickle_child_and_cleans_partial_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            child = root / 'slow_child.py'
            child.write_text('from pathlib import Path\nimport sys,time\n'
                             'target=Path(sys.argv[3])\n'
                             'with target.open("xb") as output:\n'
                             '    while True:\n'
                             '        output.write(b"x")\n'
                             '        output.flush()\n'
                             '        time.sleep(0.03)\n')
            destination = root / 'fixture'
            started = time.monotonic()
            with patch.object(probe, '__file__', str(child)), \
                 patch.object(probe, 'DOWNLOAD_DEADLINE_SECONDS', 0.25):
                with self.assertRaisesRegex(TimeoutError, 'total deadline'):
                    probe.download('https://source.example/fixture', destination,
                                   {'id': 'slow', 'max_bytes': 100, 'bytes': 1,
                                    'sha256': hashlib.sha256(b'x').hexdigest()})
            self.assertLess(time.monotonic() - started, 2)
            self.assertFalse(destination.exists())
            self.assertEqual(list(root.glob('*.part')), [])

    def test_bounded_download_rejects_http_and_child_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'file'
            with self.assertRaisesRegex(ValueError, 'bounded HTTPS'):
                probe.download('http://example.org/file', target,
                               {'id': 'file', 'max_bytes': 2, 'bytes': 2,
                                'sha256': hashlib.sha256(b'xx').hexdigest()})
            def fail(command, **kwargs):
                Path(command[4]).write_bytes(b'partial')
                return subprocess.CompletedProcess(command, 1, '', 'source exceeds bounded download size')
            with patch.object(probe.subprocess, 'run', side_effect=fail):
                with self.assertRaisesRegex(ValueError, 'bounded download size'):
                    probe.download('https://example.org/file', target,
                                   {'id': 'file', 'max_bytes': 2, 'bytes': 2,
                                    'sha256': hashlib.sha256(b'xx').hexdigest()})
            self.assertFalse(target.exists())
            self.assertEqual(list(Path(directory).glob('*.part')), [])

    def test_bad_200_never_promotes_and_valid_retry_authenticates(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            authentic = b'authentic-source'
            row = {'id': 'raw-sony', 'source': {'kind': 'download',
                   'url': 'https://example.org/a.arw'}, 'bytes': len(authentic),
                   'sha256': hashlib.sha256(authentic).hexdigest(), 'max_bytes': 100}
            target = cache / 'raw-sony.source'
            payloads = iter((b'<html>bad 200</html>', b'x' * len(authentic), authentic))
            def serve(command, **_kwargs):
                Path(command[4]).write_bytes(next(payloads))
                return subprocess.CompletedProcess(command, 0, '', '')
            with patch.object(probe.subprocess, 'run', side_effect=serve):
                with self.assertRaisesRegex(ValueError, 'size'):
                    probe.materialize(row, cache)
                self.assertFalse(target.exists())
                with self.assertRaisesRegex(ValueError, 'SHA-256 mismatch'):
                    probe.materialize(row, cache)
                self.assertFalse(target.exists())
                path, verified = probe.materialize(row, cache)
            self.assertEqual(path, target)
            self.assertEqual(verified['sha256'], row['sha256'])
            self.assertEqual(list(cache.glob('*.part')), [])

    def test_shared_cache_serializes_two_owned_consumers(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            payload = b'authenticated'
            row = {'id': 'raw-sony', 'source': {'kind': 'download',
                   'url': 'https://example.org/a.arw'}, 'bytes': len(payload),
                   'sha256': hashlib.sha256(payload).hexdigest(), 'max_bytes': 100}
            calls = []
            def serve(command, **_kwargs):
                calls.append(command)
                Path(command[4]).write_bytes(payload)
                time.sleep(0.03)
                return subprocess.CompletedProcess(command, 0, '', '')
            with patch.object(probe.subprocess, 'run', side_effect=serve):
                with ThreadPoolExecutor(max_workers=2) as pool:
                    results = list(pool.map(lambda _: probe.materialize(row, cache), range(2)))
            self.assertEqual(len(calls), 1)
            self.assertEqual([verified['sha256'] for _, verified in results],
                             [row['sha256'], row['sha256']])

    def test_zip_archive_digest_checked_before_promotion_and_member_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            buffer = io.BytesIO()
            with zipfile.ZipFile(buffer, 'w') as bundle:
                bundle.writestr('notepad++.exe', b'MZ-real')
            authentic = buffer.getvalue()
            row = {'id': 'pe-notepad', 'source': {'kind': 'zip_member',
                   'url': 'https://example.org/npp.zip', 'member': 'notepad++.exe'},
                   'archive_bytes': len(authentic), 'archive_sha256': hashlib.sha256(authentic).hexdigest(),
                   'max_bytes': 1000, 'max_member_bytes': 100}
            payloads = iter((b'x' * len(authentic), authentic))
            def serve(command, **_kwargs):
                Path(command[4]).write_bytes(next(payloads))
                return subprocess.CompletedProcess(command, 0, '', '')
            with patch.object(probe.subprocess, 'run', side_effect=serve):
                with self.assertRaisesRegex(ValueError, 'SHA-256 mismatch'):
                    probe.materialize(row, cache)
                self.assertFalse((cache / 'pe-notepad.zip').exists())
                path, verified = probe.materialize(row, cache)
            self.assertEqual(path.read_bytes(), b'MZ-real')
            self.assertEqual(verified['sha256'], hashlib.sha256(b'MZ-real').hexdigest())

    def test_child_stream_enforces_byte_ceiling(self):
        class Response(io.BytesIO):
            headers = {}
            def geturl(self):
                return 'https://mirror.example/file'
        class Opener:
            def open(self, request, timeout):
                self.timeout = timeout
                self.request = request
                return Response(b'1234')
        opener = Opener()
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(probe, 'build_opener', return_value=opener):
            target = Path(directory) / 'fixture'
            with self.assertRaisesRegex(ValueError, 'bounded download size'):
                probe.download_child('https://source.example/file', target, 2)
            self.assertEqual(opener.timeout, 30)
            self.assertEqual(opener.request.get_header('User-agent'), probe.USER_AGENT)

    def test_zip_member_is_exact_and_reextracted_from_verified_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            archive = cache / 'pe-notepad.zip'
            with zipfile.ZipFile(archive, 'w') as bundle:
                bundle.writestr('notepad++.exe', b'MZ-real')
                bundle.writestr('other.exe', b'MZ-other')
            row = {'id': 'pe-notepad', 'kind': 'pe',
                   'source': {'kind': 'zip_member', 'url': 'https://example.org/npp.zip',
                              'member': 'notepad++.exe'},
                   'archive_bytes': archive.stat().st_size,
                   'archive_sha256': probe.digest(archive), 'max_bytes': 1000,
                   'max_member_bytes': 100}
            output, record = probe.materialize(row, cache)
            self.assertEqual(output.read_bytes(), b'MZ-real')
            self.assertEqual(record['sha256'], hashlib.sha256(b'MZ-real').hexdigest())
            output.write_bytes(b'MZ-wrong')
            output, _ = probe.materialize(row, cache)
            self.assertEqual(output.read_bytes(), b'MZ-real')

    def test_failed_inputs_remain_named_in_receipt(self):
        rows = [{'id': f'media-{number}', 'kind': 'media',
                 'target': f'test_data/audio/sample-{number}.aac',
                 'source': {'kind': 'repo', 'path': 'sample.aac'},
                 'required_tags': ['AAC:SampleRate']} for number in range(25)]
        with tempfile.TemporaryDirectory() as directory:
            def materialize(row, _cache):
                if row['id'] == 'media-1':
                    raise ValueError('fixture missing')
                if row['id'] == 'media-3':
                    raise TimeoutError('HTTPS download exceeded total deadline')
                return Path(directory) / 'sample.aac', {'bytes': 1, 'sha256': 'test'}
            def native(_path, row):
                if row['id'] == 'media-2':
                    raise ValueError('zero native fields')
                return {'compared_count': 1}
            with patch.object(probe, 'materialize', side_effect=materialize), \
                 patch.object(probe, 'native_probe', side_effect=native):
                receipt_path, receipt = probe.run({'schema': 1, 'oracle_pin': '13.59',
                                                   'inputs': rows}, Path(directory))
            self.assertEqual(receipt['status'], 'FAILED')
            self.assertEqual(len(receipt['inputs']), 25)
            self.assertIn('fixture missing', receipt['inputs'][1]['error'])
            self.assertIn('zero native fields', receipt['inputs'][2]['error'])
            self.assertIn('total deadline', receipt['inputs'][3]['error'])
            self.assertEqual(json.loads(receipt_path.read_text())['status'], 'FAILED')
            self.assertTrue(receipt_path.is_relative_to(Path(directory) / 'evidence'))
            self.assertFalse(receipt_path.is_relative_to(Path(directory) / 'target'))

    def test_interrupted_probe_keeps_last_atomic_partial_receipt(self):
        rows = [{'id': f'media-{number}', 'kind': 'media',
                 'target': f'test_data/audio/sample-{number}.aac',
                 'source': {'kind': 'repo', 'path': 'sample.aac'},
                 'required_tags': ['AAC:SampleRate']} for number in range(25)]
        with tempfile.TemporaryDirectory() as directory:
            ops = Path(directory)
            seen = []
            def materialize(row, cache):
                self.assertEqual(cache, ops / 'cache/ignored-inputs')
                seen.append(row['id'])
                if len(seen) == 2:
                    raise KeyboardInterrupt()
                return ops / 'sample.aac', {'bytes': 1, 'sha256': 'test'}
            with patch.object(probe, 'materialize', side_effect=materialize), \
                 patch.object(probe, 'native_probe', return_value={'compared_count': 1}):
                with self.assertRaises(KeyboardInterrupt):
                    probe.run({'schema': 1, 'oracle_pin': '13.59', 'inputs': rows}, ops)
            paths = list((ops / 'evidence/ignored-inputs/runs').glob('*/receipt.json'))
            self.assertEqual(len(paths), 1)
            receipt = json.loads(paths[0].read_text())
            self.assertEqual(receipt['status'], 'FAILED')
            self.assertEqual([row['status'] for row in receipt['inputs']], ['PASS', 'FAILED'])
            self.assertIn('interrupted', receipt['inputs'][1]['error'])
            self.assertEqual(list(paths[0].parent.glob('*.tmp')), [])

    def test_worker_uses_ops_root_even_when_cargo_target_is_elsewhere(self):
        with tempfile.TemporaryDirectory() as directory:
            ops = Path(directory) / 'persistent-ops'
            cargo = Path(directory) / 'regenerable-target'
            with patch.object(route, 'local_worker_context', return_value=True), \
                 patch.object(probe, 'ops_root', return_value=ops), \
                 patch.object(probe, 'run', return_value=(ops / 'receipt.json',
                      {'status': 'PASS'})) as run, \
                 patch.dict(os.environ, {'CARGO_TARGET_DIR': str(cargo)}), \
                 patch.object(probe.sys, 'argv', ['ignored_fixture_probe.py']):
                self.assertEqual(probe.main(), 0)
            self.assertEqual(run.call_args.args[1], ops)
            self.assertNotEqual(run.call_args.args[1], cargo)

    def test_failed_atomic_promotion_preserves_last_valid_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'receipt.json'
            probe.write_receipt_atomic(path, {'status': 'FAILED', 'inputs': [{'id': 'first'}]})
            with patch.object(probe.Path, 'replace', side_effect=OSError('replace interrupted')):
                with self.assertRaisesRegex(OSError, 'replace interrupted'):
                    probe.write_receipt_atomic(path, {'status': 'PASS', 'inputs': []})
            self.assertEqual(json.loads(path.read_text())['inputs'], [{'id': 'first'}])
            self.assertEqual(list(Path(directory).glob('*.tmp')), [])

    def test_makernote_uses_family_one_vendor_group_and_requires_it(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            perl = root / 'perl'; perl.touch()
            (root / 'exiftool').mkdir(); (root / 'exiftool/exiftool').touch()
            row = {'id': 'makernote-canon', 'kind': 'makernote',
                   'required_tags': ['Canon:*', 'Make', 'Model']}
            env = {'EXIFTOOL_CACHE_DIR': str(root), 'EXIFTOOL_PERL': str(perl)}
            def outcome(fields):
                return subprocess.CompletedProcess([], 0, json.dumps([fields]), '')
            fields = {'Canon:OwnerName': 'Phil Harvey', 'IFD0:Make': 'Canon',
                      'IFD0:Model': 'Canon EOS DIGITAL REBEL'}
            with patch.dict(os.environ, env), patch.object(probe.subprocess, 'run',
                    return_value=outcome(fields)) as native:
                result = probe.native_probe(root / 'canon.jpg', row)
            self.assertEqual(native.call_args.args[0][2], '-G1')
            self.assertEqual(result['group_family'], '-G1')
            self.assertIn('Canon:OwnerName', result['compared_fields'])
            fields['MakerNotes:OwnerName'] = fields.pop('Canon:OwnerName')
            with patch.dict(os.environ, env), patch.object(probe.subprocess, 'run',
                    return_value=outcome(fields)):
                with self.assertRaisesRegex(ValueError, r'Canon:\*'):
                    probe.native_probe(root / 'canon.jpg', row)

    def test_flv_manifest_matches_actual_consumer_group(self):
        manifest = json.loads(probe.MANIFEST.read_text())
        row = next(item for item in manifest['inputs'] if item['id'] == 'media-flv')
        self.assertEqual(row['required_tags'], ['Flash:HasVideo', 'Flash:HasAudio'])
        self.assertEqual(row['expected_values'], {'Flash:HasVideo': 'Yes',
                                                  'Flash:HasAudio': 'Yes'})
        consumer = (probe.ROOT / 'tests/integration/flv_integration_tests.rs').read_text()
        self.assertIn('let tags_to_compare = ["Flash:HasVideo", "Flash:HasAudio"]', consumer)
        self.assertNotIn('"FLV:HasVideo"', consumer)
        webm = next(item for item in manifest['inputs'] if item['id'] == 'media-webm')
        self.assertEqual(webm['sha256'],
                         'c6a21a3a7619ca7fbcbc1b4d012d7098d18f599f0200f735d31cae15a4772dd1')

    def test_aac_manifest_requires_both_native_fields_and_consumer_comparisons(self):
        manifest = json.loads(probe.MANIFEST.read_text())
        row = next(item for item in manifest['inputs'] if item['id'] == 'media-aac')
        self.assertEqual(row['required_tags'], ['AAC:Channels', 'AAC:SampleRate'])
        self.assertEqual(row['expected_values'], {'AAC:Channels': 2,
                                                  'AAC:SampleRate': 44100})
        consumer = (probe.ROOT / 'tests/integration/aac_integration_tests.rs').read_text()
        self.assertIn('("AAC:Channels", "2")', consumer)
        self.assertIn('("AAC:SampleRate", "44100")', consumer)
        self.assertNotIn('AAC:AudioChannels', consumer)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            perl = root / 'perl'; perl.touch()
            (root / 'exiftool').mkdir(); (root / 'exiftool/exiftool').touch()
            env = {'EXIFTOOL_CACHE_DIR': str(root), 'EXIFTOOL_PERL': str(perl)}
            def outcome(fields):
                return subprocess.CompletedProcess([], 0, json.dumps([fields]), '')
            native = {'AAC:Channels': 2, 'AAC:SampleRate': 44100}
            with patch.dict(os.environ, env), patch.object(probe.subprocess, 'run',
                    return_value=outcome(native)):
                result = probe.native_probe(root / 'sample.aac', row)
            self.assertEqual(result['compared_fields'], native)
            for fields, error in (({'AAC:SampleRate': 44100}, 'missing native fields'),
                                  ({'AAC:Channels': 2}, 'missing native fields'),
                                  ({'AAC:Channels': 1, 'AAC:SampleRate': 44100}, 'native AAC:')):
                with self.subTest(fields=fields), patch.dict(os.environ, env), \
                     patch.object(probe.subprocess, 'run', return_value=outcome(fields)):
                    with self.assertRaisesRegex(ValueError, error):
                        probe.native_probe(root / 'sample.aac', row)

    def test_every_declared_media_field_is_required_by_native_admission(self):
        manifest = json.loads(probe.MANIFEST.read_text())
        media_rows = [row for row in manifest['inputs'] if row['kind'] == 'media']
        self.assertEqual(len(media_rows), 12)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            perl = root / 'perl'; perl.touch()
            (root / 'exiftool').mkdir(); (root / 'exiftool/exiftool').touch()
            env = {'EXIFTOOL_CACHE_DIR': str(root), 'EXIFTOOL_PERL': str(perl)}
            for row in media_rows:
                required = row['required_tags']
                self.assertTrue(required, row['id'])
                self.assertEqual(len(required), len(set(required)), row['id'])
                fields = {tag: row.get('expected_values', {}).get(tag, 'native-value')
                          for tag in required}
                def outcome(values):
                    return subprocess.CompletedProcess([], 0, json.dumps([values]), '')
                with self.subTest(row=row['id']), patch.dict(os.environ, env), \
                     patch.object(probe.subprocess, 'run', return_value=outcome(fields)):
                    self.assertEqual(probe.native_probe(root / 'media', row)['compared_count'],
                                     len(required))
                for missing in required:
                    partial = {tag: value for tag, value in fields.items() if tag != missing}
                    with self.subTest(row=row['id'], missing=missing), patch.dict(os.environ, env), \
                         patch.object(probe.subprocess, 'run', return_value=outcome(partial)):
                        with self.assertRaisesRegex(ValueError,
                                'missing native fields' if partial else 'zero native fields'):
                            probe.native_probe(root / 'media', row)
                with self.subTest(row=row['id'], missing='all'), patch.dict(os.environ, env), \
                     patch.object(probe.subprocess, 'run', return_value=outcome({'File:FileType': 'media'})):
                    with self.assertRaisesRegex(ValueError, 'zero native fields'):
                        probe.native_probe(root / 'media', row)

    def test_ogg_and_opus_channel_names_match_consumers(self):
        manifest = json.loads(probe.MANIFEST.read_text())
        by_id = {row['id']: row for row in manifest['inputs']}
        self.assertIn('Vorbis:AudioChannels', by_id['media-ogg']['required_tags'])
        self.assertIn('Opus:AudioChannels', by_id['media-opus']['required_tags'])
        ogg = (probe.ROOT / 'tests/integration/ogg_integration_tests.rs').read_text()
        opus = (probe.ROOT / 'tests/integration/opus_integration_tests.rs').read_text()
        self.assertIn('"Vorbis:AudioChannels"', ogg)
        self.assertIn('"Opus:AudioChannels"', opus)
        self.assertNotIn('"Vorbis:Channels"', ogg)

    def test_opus_admission_covers_all_parity_consumer_fields(self):
        manifest = json.loads(probe.MANIFEST.read_text())
        row = next(row for row in manifest['inputs'] if row['id'] == 'media-opus')
        required = {'Opus:OpusVersion', 'Opus:AudioChannels',
                    'Opus:SampleRate', 'Opus:OutputGain'}
        self.assertEqual(set(row['required_tags']), required)
        consumer = (probe.ROOT / 'tests/integration/opus_integration_tests.rs').read_text()
        for field in required:
            self.assertIn('"' + field + '"', consumer)

    def test_native_media_zero_fields_and_raw_identity_refuse(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            perl = root / 'perl'; perl.touch()
            (root / 'exiftool').mkdir(); (root / 'exiftool/exiftool').touch()
            media = {'id': 'media-aac', 'kind': 'media', 'required_tags': ['AAC:SampleRate']}
            raw = {'id': 'raw-sony', 'kind': 'raw', 'required_tags': ['Make', 'Model', 'FileType'],
                   'source': {'publisher_make': 'Sony', 'publisher_model': 'ILCE-7S'}}
            env = {'EXIFTOOL_CACHE_DIR': str(root), 'EXIFTOOL_PERL': str(perl)}
            def result(values):
                return subprocess.CompletedProcess([], 0, json.dumps([values]), '')
            with patch.dict(os.environ, env), patch.object(probe.subprocess, 'run', return_value=result({'File:FileType': 'AAC'})):
                with self.assertRaisesRegex(ValueError, 'zero native fields'):
                    probe.native_probe(root / 'a.aac', media)
            with patch.dict(os.environ, env), patch.object(probe.subprocess, 'run',
                    return_value=result({'EXIF:Make': 'Sony', 'EXIF:Model': 'Different', 'File:FileType': 'ARW'})):
                with self.assertRaisesRegex(ValueError, 'differs from publisher'):
                    probe.native_probe(root / 'a.arw', raw)


if __name__ == '__main__':
    unittest.main()
