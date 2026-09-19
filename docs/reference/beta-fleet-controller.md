# Beta fleet controller

`tools/release/fleet_controller.py` is the durable single-writer controller for
the Beta 1 functional plan. Its root contains an atomic `fleet-state.json`, an
append-only sequenced `fleet-events.jsonl`, an atomic `receipt-index.json`,
canonical task PRDs, reports, reviews, and numbered process segments.

The command surface is:

```text
init materialize event checkpoint reconcile recover launch monitor status
heartbeat resume stop
```

`fixture` is the local total-process-loss rehearsal. Run any command with
`--help` for its arguments.

`init` is non-destructive and idempotent: an existing state with the same
plan/spec hashes, target ref/SHA, and expected parent is returned unchanged,
including its controller identity, event log, and receipt index. A conflicting
identity is refused.

## Recovery model

The snapshot contains the reviewed plan/spec hashes, target ref and resolved
SHA, expected merge parent, exclusive merge lease, controller identity, and a
record for every task. Each task records dependencies, exact file lease,
worker kind/model/effort/identity, paths, state history, local/pushed/merge
SHAs, PR/CI state, receipt hashes, blocker/ruling, and its exact next command.

CLI worker identity is not a PID alone. Before `Popen`, launch atomically writes
an intent containing the token, executable, exact argv boundaries, task, paths,
and model, then holds an advisory single-flight fence through child exec. The
child's durable ready file is only a wake-up notification: adoption checks the
kernel executable, exact kernel argv, nonempty kernel start identity, token, and
process group before a process record or launchable snapshot is written. A
prepared intent can retry only after recovery proves the fence is free. A record
contains the PID, kernel start time, executable, unique token, exact argv,
kernel argv, process group, task number, session ID, log/final paths, and
segment. `monitor` incrementally parses `thread.started` and advances the
heartbeat. `stop` refuses an identity mismatch. `resume` requires a dead
recorded process and recorded session ID; it uses that ID explicitly and never
uses `--last`.

Dependencies are released only by a reconciled remote merge SHA with the
expected squash parent. Repeating the same reconciliation is idempotent. Only
one controller identity may hold the integration merge lease.

`recover` reads remote branch heads with `git ls-remote` and PR, CI, and merge
state with the GitHub CLI. Each task PR must target the controller's integration
branch; another PR base is refused. The production CLI accepts no caller-supplied
remote observation. These authenticated reads do not fetch, check out, create,
update, or delete refs. A remote outage is reported as `remote_error` while
local recovery still adopts an authenticated live worker. Recovery selects the
newest durable process record across legacy and normal record names, and adopts
a newer live record even when the snapshot still contains an older dead record.
A dead CLI worker is resumed only after the replacement's executable, task,
fresh token, exact argv, kernel start time, and observed command authenticate;
no snapshot, process record, or resume event is written before that check.
If a caught process-record write fails after authentication, the controller
uses the recorded process group (not leader liveness), sends TERM, waits for a
kernel `killpg(..., 0)` ESRCH absence proof, and escalates to KILL when needed.

## Materialization

`materialize` writes one self-contained PRD containing the plan's global
constraints, the complete selected task, resolved base SHA, literal paths,
dependencies, exact file lease, launch command, handoff/report contract, and
required signed checkpoint. It atomically records the PRD hash in the task and
receipt index.

The launch instruction matches the declared owner. CLI tasks receive the
controller's detached-launch command, Desktop tasks receive the exact
`collaboration.spawn_agent` payload for the primary session, and
Controller-owned tasks explicitly prohibit launching an implementation worker.
Checkpoint recording authenticates the SHA against the task worktree branch,
or against the same task branch in the controller repository when the worktree
is absent, before accepting its signed commit shape.

## Rehearsal

```bash
python3 tools/release/fleet_controller.py fixture \
  --root /Users/allen/oxidex-ops/evidence/20260919-beta1-functional/controller-tests/task0-step8
```

The fixture persists a running Task 0 with an old dead process in the snapshot,
one committed-but-unpushed task, one pushed draft-PR task, one other dead CLI
process, one missing Desktop identity, and one dependency-blocked task. It then
writes two newer live durable records and starts a fresh production `recover`
process with no supplied remote JSON. The receipt proves that the fresh process
adopted the newest live record across the resume crash window without launching
another worker, and terminates the fixture workers after the receipt is written.
