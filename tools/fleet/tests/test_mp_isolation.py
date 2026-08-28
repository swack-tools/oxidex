#!/usr/bin/env python3
"""The fence around the suite's multiprocessing start method.

THE DEFECT THIS PINS. `tests/test_intent.py` ran, at IMPORT time,
`multiprocessing.set_start_method("fork", force=True)`. `python3 -m
unittest <modules>` imports every module named on the command line into
ONE interpreter, so importing `test_intent` -- for any reason, in any
order, including as a side effect of naming it beside twenty other
modules -- flipped the start method for pools in files that never mention
`fork`. Under `FLEET_TEST_HUB=server` the parent process is thread-heavy
by construction (`_fixtures` runs a real `KeelHTTPServer` plus a store
sweep thread), and `fork()` clones one thread while cloning every lock the
others held: the observed result was children parked indefinitely in
`_multiprocessing_SemLock_acquire_impl` and, on another run, a fixture
server erroring against a bare repo that had already been torn down.

The failure has no failing assertion and no stable reproduction. It
expresses itself as `gate.sh`'s fleet-tests stage hitting its 1800 s
budget, i.e. as a GATE result that is a function of module import order
rather than of the code under test. That is an instrument defect, and the
two tests below are its instrument check:

  1. `TestStartMethodIsNotChangedByImport` -- in a FRESH interpreter,
     import every `test_*.py` in this directory and require that
     `multiprocessing.get_start_method(allow_none=True)` is still `None`.
     `allow_none=True` is the load-bearing detail: it reports whether
     anything has FIXED the default, not merely what the default resolves
     to, so this stays red even on a platform where the forced value and
     the interpreter default happen to agree.
  2. `TestEveryPoolNamesItsStartMethod` -- every `ProcessPoolExecutor` /
     `multiprocessing.Pool` construction in this directory passes an
     explicit context, so no pool's behaviour depends on the global
     default that test 1 now guarantees nobody sets.

Both were confirmed to FAIL with the defect present: restoring the
`set_start_method("fork", force=True)` block at the top of
`test_intent.py` turns test 1 red ("fork" instead of None), and dropping
the `mp_context=` argument from any one pool turns test 2 red naming that
file and line.

Plain `unittest`, standard library only.
"""

from __future__ import annotations

import ast
import multiprocessing
import subprocess
import sys
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _env import HermeticCase  # noqa: E402
from _mp import START_METHOD, pool_context  # noqa: E402

TESTS_DIR = Path(__file__).resolve().parent

# `test_seams*.py` is excluded from the suite's own runs (it drives real
# hosts); it is still scanned here, because a pool it starts would corrupt
# the same interpreter if it were ever run beside the rest.
_PY_FILES = sorted(p for p in TESTS_DIR.glob("*.py") if not p.name.startswith("__"))
_TEST_MODULES = sorted(p.stem for p in TESTS_DIR.glob("test_*.py"))


def _called_name(node: ast.Call) -> str:
    """The dotted tail of a call's callee: `Pool` for `multiprocessing.Pool(...)`,
    `set_start_method` for a bare or attribute-qualified call. Parsed rather
    than grepped, so this file's own prose about the defect -- and every
    docstring quoting the offending line -- is not itself an offender."""
    func = node.func
    if isinstance(func, ast.Attribute):
        return func.attr
    if isinstance(func, ast.Name):
        return func.id
    return ""


def _calls(path: Path):
    tree = ast.parse(path.read_text(), filename=str(path))
    for node in ast.walk(tree):
        if isinstance(node, ast.Call):
            yield node


class TestStartMethodIsNotChangedByImport(HermeticCase):
    """Importing this suite must not touch the process-global start method."""

    def _probe(self, modules):
        """Import `modules` in a fresh interpreter; return what
        `get_start_method(allow_none=True)` says before and after."""
        script = textwrap.dedent(
            """
            import json, sys, multiprocessing
            sys.path.insert(0, sys.argv[1])
            before = multiprocessing.get_start_method(allow_none=True)
            failed = {}
            for name in sys.argv[2:]:
                try:
                    __import__(name)
                except Exception as exc:          # an import error is a
                    failed[name] = f"{type(exc).__name__}: {exc}"
            after = multiprocessing.get_start_method(allow_none=True)
            print(json.dumps({"before": before, "after": after,
                              "default": multiprocessing.get_start_method(),
                              "failed": failed}))
            """
        )
        proc = subprocess.run(
            [sys.executable, "-c", script, str(TESTS_DIR), *modules],
            capture_output=True,
            text=True,
            env=self.hermetic_env(),
            cwd=str(TESTS_DIR),
            timeout=300,
        )
        self.assertEqual(
            proc.returncode, 0, msg=f"probe interpreter died: {proc.stderr[-4000:]}"
        )
        import json

        return json.loads(proc.stdout.strip().splitlines()[-1])

    def test_importing_every_test_module_leaves_the_start_method_unset(self):
        self.assertTrue(_TEST_MODULES, "no test modules found to import")
        result = self._probe(_TEST_MODULES)

        # An import that BLEW UP would make this test vacuous: it would
        # never reach whatever the module does at import time. Name the
        # casualties rather than passing quietly.
        self.assertEqual(
            result["failed"],
            {},
            msg="a test module failed to import, so this check proved nothing about it",
        )
        self.assertIsNone(
            result["before"], msg="a bare interpreter already had a start method fixed"
        )
        self.assertIsNone(
            result["after"],
            msg=(
                "importing this suite set the process-global multiprocessing start "
                f"method to {result['after']!r}. That is a GLOBAL flip: every pool in "
                "every other module of the run inherits it, and forking a "
                "thread-heavy parent (FLEET_TEST_HUB=server runs a real keel-server "
                "in-process) can deadlock the whole suite with no failing assertion. "
                "A pool that needs a particular start method passes its own "
                "`mp_context=` -- see tests/_mp.py."
            ),
        )

    def test_no_module_in_this_directory_calls_set_start_method(self):
        """The static half: the call itself must not exist here.

        `set_start_method` is process-global whatever guards it, so an
        occurrence anywhere in this directory is the defect regardless of
        the surrounding `try:`/`force=` details that made the original
        look local.
        """
        offenders = []
        for path in _PY_FILES:
            for call in _calls(path):
                if _called_name(call) == "set_start_method":
                    offenders.append(f"{path.name}:{call.lineno}")
        self.assertEqual(
            offenders,
            [],
            msg=(
                "multiprocessing.set_start_method() is process-global and reaches "
                "every other module in the same `python3 -m unittest` invocation:\n  "
                + "\n  ".join(offenders)
            ),
        )


class TestEveryPoolNamesItsStartMethod(HermeticCase):
    """Every pool in this directory carries its own explicit context."""

    # `Pool`/`pool` also covers `multiprocessing.pool.ThreadPool`, which is
    # harmless -- it takes no context and would be a false positive -- so
    # only the process-starting constructions are named.
    _POOL_NAMES = {"ProcessPoolExecutor", "Pool"}
    _CONTEXT_KWARGS = {"mp_context", "context"}

    def test_every_process_pool_construction_passes_an_explicit_context(self):
        offenders = []
        for path in _PY_FILES:
            for call in _calls(path):
                name = _called_name(call)
                if name not in self._POOL_NAMES:
                    continue
                kwargs = {kw.arg for kw in call.keywords if kw.arg}
                if kwargs & self._CONTEXT_KWARGS:
                    continue
                offenders.append(f"{path.name}:{call.lineno}: {name}(...)")
        self.assertEqual(
            offenders,
            [],
            msg=(
                "these pools inherit the process-global start method, which any "
                "module imported into the same run can change under them:\n  "
                + "\n  ".join(offenders)
            ),
        )

    def test_pool_context_does_not_mutate_the_global_default(self):
        """Asking `_mp` for a context is a read, not a write."""
        before = multiprocessing.get_start_method(allow_none=True)
        ctx = pool_context()
        self.assertEqual(ctx.get_start_method(), START_METHOD)
        self.assertEqual(multiprocessing.get_start_method(allow_none=True), before)


if __name__ == "__main__":
    unittest.main()
