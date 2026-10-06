import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import tempfile
import json
import build as auto

class RetryTests(unittest.TestCase):
    def test_interruption_refreshes_selection_and_switches_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            def build(command, env):
                self.assertEqual(env['OXIDEX_REMOTE_INSTANCE_ID'],env['OXIDEX_REMOTE_INSTANCE'])
                receipt=Path(command[command.index('--evidence-dir')+1])/'remote-build.json'
                receipt.write_text(json.dumps({'retryable':env['OXIDEX_REMOTE_INSTANCE']=='a'}))
                return 1 if env['OXIDEX_REMOTE_INSTANCE']=='a' else 0
            workers=[(SimpleNamespace(name=n,zone='z',instance_id=n),(.1,.2)) for n in ('a','b')]
            with patch.object(auto,'select_worker',side_effect=workers) as select, patch.object(auto.subprocess,'call',side_effect=build):
                self.assertEqual(auto.run_attempts('p',root,['--profile','debug'],'debug',root/'evidence',3,{}),0)
            self.assertEqual(select.call_count,2)
            self.assertEqual(select.call_args_list[1].kwargs['excluded_ids'],{'a'})

    def test_compiler_failure_does_not_retry(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(auto,'select_worker',return_value=(SimpleNamespace(name='a',zone='z',instance_id='1'),(.1,.2))) as select, patch.object(auto.subprocess,'call',return_value=1):
                self.assertEqual(auto.run_attempts('p',root,['--profile','debug'],'debug',root/'evidence',3,{}),1)
            self.assertEqual(select.call_count,1)
