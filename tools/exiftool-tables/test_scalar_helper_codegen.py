"""Generated Rust operands must follow source semantics, not just fingerprints."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from checkexif_recipes import RecipeMalformed
import scalar_helper_codegen as generator
from test_checkvalue_recipes import fact as check_fact
from test_writevalue_recipes import fact as write_fact


def document():
    return {'native_write_helpers': {'check_value': check_fact(), 'write_value': write_fact()}}


class ScalarHelperCodegenTests(unittest.TestCase):
    def test_source_operands_change_in_generated_rust(self):
        doc = document()
        check = doc['native_write_helpers']['check_value']
        before, after = check['__deparse'].rsplit("'string'", 1)
        check['__deparse'] = before + "'undef'" + after
        write = doc['native_write_helpers']['write_value']
        write['__deparse'] = write['__deparse'].replace('$count > 0', '$count > 2')
        output, report = generator.generate(doc)
        self.assertIn('first_format: "undef"', output)
        self.assertIn('count_positive_bound: 2', output)
        self.assertEqual(report['helpers']['check_value']['state'], 'compiled')
        self.assertEqual(report['helpers']['write_value']['state'], 'compiled')

    def test_unknown_source_rule_emits_none_and_named_gap(self):
        doc = document()
        doc['native_write_helpers']['check_value']['__deparse'] = 'unknown upstream check'
        output, report = generator.generate(doc)
        self.assertIn('CHECK_VALUE: Option<ScalarCheckRecipe> = None', output)
        self.assertNotIn('CHECK_VALUE: Option<ScalarCheckRecipe> = Some', output)
        self.assertEqual(report['helpers']['check_value']['state'], 'unsupported')
        self.assertTrue(report['helpers']['check_value']['reason'])
        self.assertEqual(report['helpers']['write_value']['state'], 'compiled')

    def test_live_dispatch_override_cannot_emit_old_scalar_rule(self):
        doc = document()
        helper = doc['native_write_helpers']['write_value']
        helper['lexical_hashes']['bindings']['%writeValueProc']['entries']['string'] = {'__perl': 'CODE'}
        output, report = generator.generate(doc)
        self.assertIn('WRITE_VALUE: Option<ScalarWriteRecipe> = None', output)
        self.assertIn('intercepted', report['helpers']['write_value']['reason'])

    def test_unrepresentable_source_count_bound_is_an_explicit_gap(self):
        doc = document()
        helper = doc['native_write_helpers']['write_value']
        helper['__deparse'] = helper['__deparse'].replace('$count > 0', '$count > ' + str(1 << 63))
        output, report = generator.generate(doc)
        self.assertIn('WRITE_VALUE: Option<ScalarWriteRecipe> = None', output)
        self.assertIn('i64', report['helpers']['write_value']['reason'])

    def test_missing_capture_is_malformed_not_ordinary_unsupported_source(self):
        with self.assertRaises(RecipeMalformed):
            generator.generate({})
        doc = document()
        del doc['native_write_helpers']['check_value']
        with self.assertRaises(RecipeMalformed):
            generator.generate(doc)

    def test_rust_literals_preserve_unicode_controls_quotes_and_backslashes(self):
        self.assertEqual(generator.rust_string('é\0\n"\\'), '"é\\u{0}\\u{a}\\"\\\\"')


@unittest.skipUnless(os.environ.get('OXIDEX_TABLES_JSON'),
                     'requires fresh captured helper facts from the pinned release')
class NativeArtifactFreshnessTests(unittest.TestCase):
    def test_committed_rules_and_ledger_equal_fresh_native_generation(self):
        # CI supplies a fresh dump from .exiftool-version, not an artifact's
        # own version stamp. Check the files we ship as well as temporary Rust.
        root = Path(__file__).resolve().parents[2]
        source, report = generator.generate(json.loads(Path(os.environ['OXIDEX_TABLES_JSON']).read_text()))
        with tempfile.TemporaryDirectory(prefix='oxidex-scalar-freshness-') as directory:
            output = Path(directory) / 'rules.rs'
            output.write_text(source)
            subprocess.run(['rustfmt', '--edition', '2024', '--config-path', str(root / 'rustfmt.toml'),
                            str(output)], check=True, capture_output=True, timeout=30)
            self.assertEqual(output.read_text(), (root / 'src/writers/generated_scalar_rules.rs').read_text(),
                             'committed scalar rules are stale; run official regeneration')
        self.assertEqual(json.dumps(report, sort_keys=True, indent=2) + '\n',
                         (Path(__file__).parent / 'scalar_helper_ledger.json').read_text(),
                         'committed scalar helper ledger is stale; run official regeneration')


if __name__ == '__main__':
    unittest.main()
