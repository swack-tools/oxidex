import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import catalog_snapshot as snapshot
from test_hydrated_catalog_universe import CANONICAL_PERL, CANONICAL_LIB, NATIVE_READY

ARTIFACT = HERE.parents[1] / 'docs/public/measurements/catalog-source-13.59.json'


def fixture():
    table = 'Image::ExifTool::Example::Main'
    entries = [dict(table=table, raw_key=str(i), variant_index=0, name=name,
                    normalized_name=name.lower(), groups={'0':'Example','1':'Example','2':'Other'})
               for i, name in enumerate(('Sample', 'SAMPLE'))]
    return dict(schema='oxidex_hydrated_catalog_universe_v1', exiftool_version='13.59',
                entries=entries, unique_names=['sample'], container_rows_outside_total=[],
                counts=dict(catalog_total_tag_entries=2, catalog_unique_tag_names=1,
                            distinct_case_insensitive_entry_names=1, hydrated_tables=1,
                            catalog_container_rows_outside_total=0),
                families={'hydrated_tables':[{'full_name':table}]},
                producer={'sources':{'Image/ExifTool.pm':{},'Image/ExifTool/BuildTagLookup.pm':{}}})


class CatalogSnapshot(unittest.TestCase):
    def test_names_are_case_insensitive_but_source_rows_are_distinct(self):
        snapshot.validate(fixture(), '13.59')

    def test_missing_or_duplicate_row_refuses(self):
        for mutation in ('delete','duplicate'):
            doc = fixture()
            if mutation == 'delete': doc['entries'].pop()
            else: doc['entries'][1] = copy.deepcopy(doc['entries'][0])
            with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, 'denominator|duplicate catalog'):
                snapshot.validate(doc, '13.59')

    def test_name_table_and_container_conservation(self):
        for mutate in (
            lambda d: d['unique_names'].append('missing'),
            lambda d: d['entries'][0].update(table='Other'),
            lambda d: d['counts'].update(catalog_container_rows_outside_total=1),
            lambda d: d['entries'][0].update(normalized_name='Sample'),
        ):
            doc = fixture(); mutate(doc)
            with self.assertRaises(ValueError): snapshot.validate(doc, '13.59')

    def test_native_counter_is_not_assumed_to_be_set_cardinality(self):
        doc = fixture(); doc['counts']['catalog_unique_tag_names'] = 3
        snapshot.validate(doc, '13.59')
        self.assertEqual(len(doc['unique_names']), 1)

    def test_report_counts_are_generated_from_snapshot(self):
        doc = fixture()
        # Only the generated region changes, preserving the explanatory text.
        template = 'before\n<!-- catalog-counts:start -->\nstale\n<!-- catalog-counts:end -->\nafter\n'
        doc['counts']['shortcut_entries'] = 0
        report = snapshot.rendered_report(doc, template)
        self.assertIn('| Native table-entry denominator | 2 |', report)
        self.assertTrue(report.startswith('before\n'))
        self.assertTrue(report.endswith('\nafter\n'))
        with self.assertRaises(ValueError): snapshot.rendered_report(doc, 'no markers')

    def test_wrong_pin_refuses(self):
        with self.assertRaisesRegex(ValueError, 'version'):
            snapshot.validate(fixture(), '99.99')

    def test_checked_in_snapshot_conserves_all_entries(self):
        doc = json.loads(ARTIFACT.read_text())
        snapshot.validate(doc, (HERE.parents[1] / '.exiftool-version').read_text().strip())
        # Availability of a name is explicitly not a read/write capability.
        self.assertNotIn('observed_read', doc)
        self.assertNotIn('observed_write', doc)

    @unittest.skipUnless(NATIVE_READY, 'configured pinned ExifTool is unavailable')
    def test_native_regeneration_matches_committed_snapshot(self):
        actual = snapshot.capture(CANONICAL_LIB, CANONICAL_PERL)
        expected = json.loads(ARTIFACT.read_text())
        self.assertEqual(actual, expected)

    @unittest.skipUnless(NATIVE_READY, 'configured pinned ExifTool is unavailable')
    def test_user_config_is_not_loaded(self):
        with tempfile.TemporaryDirectory() as directory:
            config = Path(directory) / '.ExifTool_config'
            config.write_text('die "HOSTILE USER CONFIG WAS LOADED\\n";\n')
            old = os.environ.get('EXIFTOOL_HOME')
            os.environ['EXIFTOOL_HOME'] = directory
            try:
                result = snapshot.capture(CANONICAL_LIB, CANONICAL_PERL)
            finally:
                if old is None: os.environ.pop('EXIFTOOL_HOME', None)
                else: os.environ['EXIFTOOL_HOME'] = old
            self.assertEqual(result, json.loads(ARTIFACT.read_text()))

    @unittest.skipUnless(NATIVE_READY, 'configured pinned ExifTool is unavailable')
    def test_stale_snapshot_is_rejected_without_overwriting_it(self):
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / 'stale.json'
            out.write_text('{}\n')
            env = os.environ.copy(); env['OXIDEX_ALLOW_DIRTY_TREE'] = '1'
            result = subprocess.run([sys.executable, str(HERE/'catalog_snapshot.py'),
                                     '--exiftool-dir', str(CANONICAL_LIB), '--perl', CANONICAL_PERL,
                                     '--output', str(out), '--check'], env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('stale', result.stderr)
            self.assertEqual(out.read_text(), '{}\n')


if __name__ == '__main__': unittest.main()
