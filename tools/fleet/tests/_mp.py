"""ONE explicit multiprocessing start method for every pool in this suite.

WHY THIS FILE EXISTS. `tests/test_intent.py` used to run, at IMPORT time:

    multiprocessing.set_start_method("fork", force=True)

That is a process-GLOBAL setting, and `python3 -m unittest <many modules>`
imports every module named on the command line into ONE interpreter before
running a single test. So importing `test_intent` flipped the default start
method for the whole suite -- including for pools in files that never
mention `fork`, never import `test_intent`, and run long after it.

On macOS (and, since 3.14, on Linux) the interpreter default is not `fork`
for a reason: `fork()` clones exactly the calling thread and none of the
others, but it clones every LOCK those other threads were holding, in
whatever state they were in at the instant of the fork. Under
`FLEET_TEST_HUB=server` the parent is thread-heavy by construction --
`_fixtures._ServerHubFixture` runs a real `KeelHTTPServer` (accept loop,
per-connection handler threads, watchdog) plus the store's sweep thread --
and it holds listening sockets, whose file descriptors a forked child
inherits and keeps open after the parent has closed them. The observed
consequences, all nondeterministic:

  * children parked forever in `_multiprocessing_SemLock_acquire_impl` on a
    semaphore that was cloned while another thread held it -- the whole
    suite hangs, and `tools/fleet/gate.sh`'s 1800 s fleet-tests budget
    turns that into a GATE FAILURE with no failing assertion anywhere;
  * `TestConcurrentCreate` erroring out of a fixture keel-server whose
    backing bare repo had already been torn down.

Neither symptom names `fork`, and neither is reproducible on demand. That
is the shape of an instrument defect rather than an ordinary bug: the
suite's answer stops being a function of the code under test and starts
being a function of which modules the invocation happened to import.

WHAT THIS MODULE ESTABLISHES.

  1. No module in this suite ever calls `multiprocessing.set_start_method`.
     The process-global default is left exactly as the interpreter set it,
     so importing any subset of the suite in any order cannot change how
     an unrelated pool starts its children.
  2. Every `ProcessPoolExecutor`/`multiprocessing.Pool` in this suite
     passes `mp_context=` (or `context=`) EXPLICITLY, from here, so each
     pool's start method is a property of that pool's own call site.

`tests/test_mp_isolation.py` is the fence for both: one test asserts (in a
fresh interpreter) that importing every `test_*.py` in this directory
leaves `multiprocessing.get_start_method(allow_none=True)` at `None`, and
another greps every pool construction in this directory for an explicit
context argument.

WHY `spawn`. A `spawn` child starts from a fresh interpreter: no cloned
locks, no inherited listening sockets, no half-copied server. It costs a
re-import of the worker's defining module per child, which for this suite
is fractions of a second and buys a start method that is safe regardless
of how many threads the parent is running -- and every pool here races
real OS processes against a real remote precisely so the property under
test is not defined into passing, which is worth paying for honestly.

The `fork`-only hazard `test_intent`'s original comment cited -- a
`tools/fleet/queue.py` shadowing the stdlib `queue` module for a `spawn`
child that re-derives `sys.path` -- no longer exists: that file was
renamed to `workqueue.py` for this exact reason (see its module docstring),
and `tools/fleet` has carried no `queue.py` since. A `spawn` child here
re-imports the stdlib `queue` safely.
"""

from __future__ import annotations

import multiprocessing

__all__ = ["START_METHOD", "pool_context"]

# The one start method this suite's pools use. Named, not defaulted: a
# pool that inherits the process default inherits whatever the last
# imported module did to it, which is the defect this module exists to
# retire.
START_METHOD = "spawn"


def pool_context(method: str = START_METHOD):
    """The `mp_context=` every `ProcessPoolExecutor` in this suite passes.

    Returns a real `multiprocessing` context object; asking for one does
    NOT change the process-global default (that is `set_start_method`'s
    job, and nothing in this suite may call it).
    """
    return multiprocessing.get_context(method)
