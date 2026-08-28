#!/usr/bin/env python3
"""Tests for gate.sh's PLAN Stage 3 task 7 addition: the optional
`KEEL_SERVER_URL`/`KEEL_TOKEN_FILE` -> `VERDICT_SERVER_ARGS` ->
`verdict.py --server-url/--token-file` plumbing.

Same extraction techniques `test_gate_script.py` already uses for this
file (`_extract_shell_function`, reused here rather than re-implemented)
-- gate.sh's own TEXT is exercised in a real `bash` subprocess, not a
paraphrase of what it is supposed to do. Two things are pinned
separately because they are two different failure modes:

  1. The ARRAY-BUILDING block (`KEEL_SERVER_URL="${KEEL_SERVER_URL:-}"`
     through the closing `fi`) -- unconditional/free-standing lines, not
     a function -- correctly turns zero/one/two env vars into the right
     `VERDICT_SERVER_ARGS` argv, under `set -u`.
  2. `store_verdict()`'s ARGV to its stub `verdict.py` actually carries
     (or omits) `--server-url`/`--token-file`, proving the array is not
     just built correctly but also actually reaches the command line --
     the gap a correct-looking array with a forgotten `"${...[@]}"` at
     the call site would leave invisible to (1) alone.

Run with:
    python3 -m unittest discover -s tools/fleet/tests -v
"""

from __future__ import annotations

import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

FLEET_DIR = Path(__file__).resolve().parents[1]
GATE_SH = FLEET_DIR / "gate.sh"

sys.path.insert(0, str(Path(__file__).resolve().parent))
from test_gate_script import _extract_shell_function  # noqa: E402
from _env import HermeticCase  # noqa: E402


def _extract_verdict_server_args_block(source: str) -> str:
    """The free-standing `KEEL_SERVER_URL=... VERDICT_SERVER_ARGS=() if
    ... fi` block gate.sh sets up once, before `store_verdict()` and the
    `verdict.py lookup` call site both consume it. Anchored on its own
    distinctive first and last lines rather than a line-count slice, so
    a comment added above/below it does not silently desync the
    extraction from the real block."""
    start_marker = 'KEEL_SERVER_URL="${KEEL_SERVER_URL:-}"'
    start = source.index(start_marker)
    end_marker = "\nfi\n"
    end = source.index(end_marker, start) + len(end_marker)
    return source[start:end]


class TestVerdictServerArgsBlock(HermeticCase):
    """(1): the array-building block alone, under `set -u`."""

    def setUp(self):
        super().setUp()
        self.source = GATE_SH.read_text(encoding="utf-8")
        self.block = _extract_verdict_server_args_block(self.source)
        self.assertIn("VERDICT_SERVER_ARGS", self.block)
        self.assertIn("KEEL_TOKEN_FILE", self.block)

    def _run(self, env: dict) -> list:
        """Runs the extracted block under `set -u`, then dumps
        `VERDICT_SERVER_ARGS` word-by-word via a `for` loop (never
        `printf "%s" "${arr[@]}"` directly -- `printf` runs its format at
        least ONCE even with zero data arguments, which would misreport
        an empty array as one empty-string element; a `for` loop over an
        empty list correctly emits nothing) so this test reads the
        ARRAY's real contents, not a `declare -p` string it would have to
        itself re-parse."""
        script = (
            "set -u\n"
            f"{self.block}\n"
            'for _a in "${VERDICT_SERVER_ARGS[@]+"${VERDICT_SERVER_ARGS[@]}"}"; do '
            'printf "%s\\n" "$_a"; done\n'
        )
        result = subprocess.run(
            ["bash", "-c", script], env=env, capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(result.returncode, 0, msg=result.stderr)
        return result.stdout.splitlines()

    def test_neither_var_set_yields_empty_args(self):
        args = self._run({"PATH": "/usr/bin:/bin"})
        self.assertEqual(args, [])

    def test_server_url_alone_yields_just_that_flag(self):
        args = self._run({"PATH": "/usr/bin:/bin", "KEEL_SERVER_URL": "http://127.0.0.1:8470"})
        self.assertEqual(args, ["--server-url", "http://127.0.0.1:8470"])

    def test_server_url_and_token_file_yields_both_flags_in_order(self):
        args = self._run({
            "PATH": "/usr/bin:/bin",
            "KEEL_SERVER_URL": "http://127.0.0.1:8470",
            "KEEL_TOKEN_FILE": "/home/allen/.keel/token",
        })
        self.assertEqual(
            args,
            ["--server-url", "http://127.0.0.1:8470", "--token-file", "/home/allen/.keel/token"],
        )

    def test_token_file_alone_without_server_url_is_ignored(self):
        """`--token-file` is meaningless without a server -- the block
        must never emit it on its own (verdict.py's own argparse would
        accept it harmlessly, but a stray, unused KEEL_TOKEN_FILE
        env var should not silently start passing a flag nothing
        reads)."""
        args = self._run({"PATH": "/usr/bin:/bin", "KEEL_TOKEN_FILE": "/home/allen/.keel/token"})
        self.assertEqual(args, [])

    def test_empty_string_server_url_is_treated_as_unset(self):
        args = self._run({"PATH": "/usr/bin:/bin", "KEEL_SERVER_URL": ""})
        self.assertEqual(args, [])


class TestStoreVerdictPassesServerArgsThrough(HermeticCase):
    """(2): `store_verdict()`'s real argv to its stub `verdict.py`."""

    def setUp(self):
        super().setUp()
        self.tmp = Path(tempfile.mkdtemp(prefix="fleet-store-verdict-server-args-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.self_dir = self.tmp / "self_dir"
        self.self_dir.mkdir()
        self.L = self.tmp / "gate-x.log"
        self.J = self.tmp / "gate-x.json"
        self.SV = self.tmp / "gate-x.verdict-store-failed"
        self.J.write_text("{}")
        self.L.write_text("")
        self.argv_capture = self.tmp / "captured-argv"
        (self.self_dir / "verdict.py").write_text(
            "import sys\n"
            f"open({str(self.argv_capture)!r}, 'w').write(' '.join(sys.argv[1:]))\n"
            "sys.exit(0)\n"
        )

    def _run_store_verdict(self, *, verdict_server_args_literal: str) -> None:
        source = GATE_SH.read_text(encoding="utf-8")
        func_src = _extract_shell_function(source, "store_verdict")
        script = (
            "set -u\n"
            f"TREE_SHA={shlex.quote('a' * 40)}\n"
            f"SELF_DIR={shlex.quote(str(self.self_dir))}\n"
            "HUB_URL='https://example.invalid/state.git'\n"
            f"VERDICT_WORKDIR={shlex.quote(str(self.tmp / 'workdir'))}\n"
            f"J={shlex.quote(str(self.J))}\n"
            f"L={shlex.quote(str(self.L))}\n"
            f"SV={shlex.quote(str(self.SV))}\n"
            f"{verdict_server_args_literal}\n"
            f"{func_src}\n"
            "store_verdict\n"
        )
        result = subprocess.run(["bash", "-c", script], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_empty_verdict_server_args_reaches_no_extra_flags(self):
        self._run_store_verdict(verdict_server_args_literal="VERDICT_SERVER_ARGS=()")
        captured = self.argv_capture.read_text()
        self.assertNotIn("--server-url", captured)
        self.assertNotIn("--token-file", captured)
        self.assertIn("--hub-url", captured)  # sanity: the normal args are still there

    def test_populated_verdict_server_args_reaches_the_stub(self):
        self._run_store_verdict(
            verdict_server_args_literal=(
                'VERDICT_SERVER_ARGS=(--server-url "http://127.0.0.1:8470" '
                '--token-file "/home/allen/.keel/token")'
            )
        )
        captured = self.argv_capture.read_text()
        self.assertIn("--server-url http://127.0.0.1:8470", captured)
        self.assertIn("--token-file /home/allen/.keel/token", captured)

    def test_unset_verdict_server_args_under_nounset_does_not_crash(self):
        """The exact scenario `TestStoreVerdictLoudFailure` in
        `test_gate_script.py` already exercises without `set -u` --
        repeated HERE under `set -u` explicitly, since that is gate.sh's
        own real mode (`set -u` near its top) and the one this task's
        `"${VERDICT_SERVER_ARGS[@]+...}"` idiom exists to survive."""
        source = GATE_SH.read_text(encoding="utf-8")
        func_src = _extract_shell_function(source, "store_verdict")
        script = (
            "set -u\n"
            f"TREE_SHA={shlex.quote('a' * 40)}\n"
            f"SELF_DIR={shlex.quote(str(self.self_dir))}\n"
            "HUB_URL='https://example.invalid/state.git'\n"
            f"VERDICT_WORKDIR={shlex.quote(str(self.tmp / 'workdir'))}\n"
            f"J={shlex.quote(str(self.J))}\n"
            f"L={shlex.quote(str(self.L))}\n"
            f"SV={shlex.quote(str(self.SV))}\n"
            # deliberately NOT setting VERDICT_SERVER_ARGS at all
            f"{func_src}\n"
            "store_verdict\n"
        )
        result = subprocess.run(["bash", "-c", script], capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
