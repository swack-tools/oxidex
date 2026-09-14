"""Execute freshly generated helper operands in the real Rust scalar executor.

The native return records, not the Python reference evaluator, supply the Rust
expectations. The native fixture runners additionally authenticate loaded CV
source/body hashes and check their own Python references before returning data.
The helper-local write count is checked separately against the source reference;
native return records cannot observe that local variable.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import scalar_helper_codegen as generator
import test_checkvalue_recipes as native_check
import test_writevalue_recipes as native_write
from checkvalue_recipes import NativeScalar
from writevalue_recipes import compile_scalar_write, evaluate_scalar_write

ROOT = Path(__file__).resolve().parents[2]


def _scalar(value):
    if value.get('kind') == 'undefined' or value.get('defined') is False:
        return 'Scalar::Undefined'
    if value.get('kind') == 'bytes' or value.get('utf8') is False:
        raw = bytes.fromhex(value.get('value', value.get('hex')))
        return 'Scalar::Bytes(vec![' + ','.join(str(byte) for byte in raw) + '])'
    text = value['value'] if 'kind' in value else bytes.fromhex(value['hex']).decode('utf8')
    return 'Scalar::Utf8(' + generator.rust_string(text) + '.to_owned())'


@unittest.skipUnless(os.environ.get('OXIDEX_PINNED_EXIFTOOL') and os.environ.get('OXIDEX_TABLES_JSON'),
                     'requires explicit native source and matching captured helper facts')
class GeneratedScalarRustTests(unittest.TestCase):
    def test_generated_rust_matches_authenticated_native_helper_returns(self):
        document = json.loads(Path(os.environ['OXIDEX_TABLES_JSON']).read_text())
        source, report = generator.generate(document)
        self.assertTrue(all(row['state'] == 'compiled' for row in report['helpers'].values()), report)
        write_recipe = compile_scalar_write(document['native_write_helpers']['write_value'])
        real_run = subprocess.run
        observations = {}
        for kind, test in (
            ('check', native_check.NativeScalarDifferential().test_actual_native_helper_matches_all_scalar_states_and_counts),
            ('write', native_write.NativeScalarDifferential().test_actual_native_helper_matches_scalar_states_and_counts),
        ):
            def capture(*args, **kwargs):
                result = real_run(*args, **kwargs)
                observations[kind] = (json.loads(kwargs['input']), json.loads(result.stdout)['results'])
                return result
            with patch.object(subprocess, 'run', capture):
                test()
        with tempfile.TemporaryDirectory(prefix='oxidex-generated-scalar-') as directory:
            root = Path(directory)
            rules = root / 'rules.rs'
            rules.write_text(source)
            harness = [
                '#![allow(dead_code)]',
                '#[path=' + generator.rust_string(str(ROOT / 'src/error/mod.rs')) + '] mod error;',
                'mod writers { #[path=' + generator.rust_string(str(ROOT / 'src/writers/generated_scalar.rs'))
                + '] pub(crate) mod generated_scalar; }',
                '#[path=' + generator.rust_string(str(rules)) + '] mod rules;',
                'use writers::generated_scalar::*;',
            ]
            for kind, (cases, results) in observations.items():
                self.assertGreaterEqual(len(cases), 240)
                self.assertEqual(len(cases), len(results))
                harness.append('#[test] fn native_' + kind + '() {')
                for index, (case, result) in enumerate(zip(cases, results, strict=True)):
                    count = 'None' if case['count'] is None else 'Some(' + str(case['count']) + ')'
                    function = 'validate_scalar' if kind == 'check' else 'serialize_scalar'
                    rule = 'CHECK_VALUE' if kind == 'check' else 'WRITE_VALUE'
                    harness.append(f'let value = {function}(&rules::{rule}.unwrap(), {_scalar(case)}, '
                                   f'{generator.rust_string(case["format"])}, {count}).unwrap();')
                    harness.append(f'assert_eq!(value.value, {_scalar(result)}, "{kind} case {index}");')
                    if kind == 'check':
                        error = 'None' if result['error'] is None else 'Some(' + generator.rust_string(result['error']) + ')'
                        harness.append(f'assert_eq!(value.error, {error}, "check error {index}");')
                    else:
                        # Native JSON's count is the unchanged caller argument.
                        # This distinct assertion checks only reference agreement.
                        raw = bytes.fromhex(case['value']) if case['kind'] == 'bytes' else case['value']
                        reference = evaluate_scalar_write(
                            write_recipe, NativeScalar(case['kind'], raw), case['format'], case['count'])
                        harness.append(f'assert_eq!(value.count, {reference.count}, '
                                       f'"write source-reference local count {index}");')
                harness.append('}')
            program = root / 'proof.rs'
            program.write_text('\n'.join(harness))
            binary = root / 'proof'
            compile_result = real_run(['rustc', '--edition=2024', '--test', str(program), '-o', str(binary)],
                                      capture_output=True, text=True, timeout=60)
            self.assertEqual(compile_result.returncode, 0, compile_result.stdout + compile_result.stderr)
            execution = real_run([str(binary), '--nocapture'], capture_output=True, text=True, timeout=60)
            self.assertEqual(execution.returncode, 0, execution.stdout + execution.stderr)


if __name__ == '__main__':
    unittest.main()
