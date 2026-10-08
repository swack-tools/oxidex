"""Local control checks for the fixed remote qualification suite inventory."""
import io
import json
from contextlib import redirect_stdout
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import qualification_test_runner as runner


class QualificationTestRunnerControls(unittest.TestCase):
    def test_fixed_inventory_and_guarded_execution(self):
        names = [f"tools/exiftool-tables/{name}.py" for name in runner.TABLE_MODULES]
        self.assertEqual(len(names), len(set(names)))
        self.assertIn("tools/exiftool-tables/test_verify_task19_results.py", names)
        self.assertIn("tools/exiftool-tables/test_version_transition_read_policy.py", names)
        rehearsal = {path.stem for path in runner.TABLES.glob("test_version_rehearsal*.py")}
        self.assertEqual({name for name in runner.TABLE_MODULES
                          if name.startswith("test_version_rehearsal")}, rehearsal)
        self.assertEqual(runner.SUITES[0][1][-2:], ("-p", "test_qualification*.py"))
        seen = []
        with patch("route.main", side_effect=lambda args: seen.append(("guard", args))), \
             patch.object(runner.subprocess, "check_output", return_value="a" * 40), \
             patch.object(runner.subprocess, "run", side_effect=lambda args, **kw: (
                 seen.append(("run", Path(kw["cwd"]), args)) or SimpleNamespace(returncode=0))):
            with redirect_stdout(io.StringIO()) as output:
                self.assertEqual(runner.main(), 0)
        self.assertEqual(seen[0], ("guard", ["--require-local-context", "test-qualification"]))
        self.assertEqual(len([event for event in seen if event[0] == "run"]), 2)
        manifest = json.loads(next(line.split(" ", 1)[1] for line in output.getvalue().splitlines()
                                   if line.startswith("QUALIFICATION_PYTHON_MANIFEST ")))
        self.assertEqual(manifest["source_head"], "a" * 40)
        self.assertEqual(len(manifest["suites"]), 13)
        self.assertIn("no Task19 rows or corpus gate", manifest["claim"])

    def test_suite_failure_is_not_reported_as_success(self):
        with patch("route.main"), \
             patch.object(runner.subprocess, "check_output", return_value="a" * 40), \
             patch.object(runner.subprocess, "run", side_effect=[SimpleNamespace(returncode=1),
                                                                SimpleNamespace(returncode=0)]), \
             redirect_stdout(io.StringIO()):
            self.assertEqual(runner.main(), 1)
