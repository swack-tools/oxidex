# Keel — staged migration plan

Companion to `docs/AGENT-SERVER-SPEC.md`. Seven stages; each lands on a `staging/keel-N-*` branch
of the code repo and must PASS `tools/fleet/gate.sh` (GATE_VERSION 7, whose fleet-tests stage runs
the whole `tools/fleet/tests` suite, `gate.sh` L478-630) before the next begins. Every acceptance
criterion names the instrument that proves it; a stage whose acceptance cannot be shown with that
instrument is not done. Hours are focused engineering hours for the implementing agent(s)
including tests and the acceptance run; add ~20 % for the adversarial-review passes this tree has
been getting (two so far: 19 agents/12 findings, 14 agents/8 findings).

Tiers: **Opus-class** for anything touching CAS semantics, leases, election, fencing, the train
adapter, budgets, policy, or security; **Sonnet-class** for plumbing, config, rendering, docs,
mechanical test adaptation. "∥" marks tasks that can run in parallel with the others in the same
stage (distinct files, no shared state).

Starting state *(measured 2026-08-21 from m5)*: fleet drained; ryzen (hub + pod) dark; tree
`fd154c5d` gate-PASSED; code repo `swack-tools/oxidex` PUBLIC with one active ruleset (`main`);
`git ls-remote origin 'refs/fleet/*'` → 0 refs; `tailscale status` on m5 → not running;
`python3` 3.14.7 on m5; `uv`, `claude`, `codex` present on m5; i7 up (only regen/oracle host).

Human preconditions (not engineering hours, not automatable, listed once): create the private repo
`swack-tools/oxidex-fleet-state`; create the deploy key and fine-grained PATs; bring Tailscale up on
m5 and i7 (and on m4/ryzen when they return); distribute the secrets bundle to the i7.

---

## Stage 1 — Hubless fleet on the GitHub spine (m5 + i7, no old hub) — 9 h

**Goal.** End the "hub is dead so hosts idle" state today, reversibly, with the *existing* fleetd
and no server. Prove GitHub honours the CAS at the volume we need, prove tip protection, and give
the human a `fleet status` that says *why* nothing is starting.

**Deliverables.**
- Code repo rulesets: `tip-update` (restrict updates on `refs/heads/refactor/tag-machinery`,
  bypass = the repo's write-capable deploy keys **as a class** — GitHub stores the actor as
  `actor_id: null` and grants every one of them, so "only `keel-train`" is a standing invariant
  the rollout re-checks, not a one-time setup step; SPEC §8), `tip-guard` (block deletion +
  force-push, no bypass),
  `rescued-guard` (same on `refs/heads/rescued/*`), identical `proof-update`/`proof-guard` pair on
  `refs/heads/keel-proof/*`; `tests/live/test_tip_ruleset.py` (opt-in `FLEET_LIVE_GITHUB=1`).
- `fleetlib.py`: `code_url` + `code_push_url` + `tip_push_url` constructor args (each defaulting
  to the one before it, ending at `url`, so a single-repo fleet is unchanged);
  `code_sha`/`code_list`/`push_code_ref`/`push_tip_ref`/`delete_code_ref` — and `push_code_ref`/
  `delete_code_ref` take NO `ssh_command`, so only the tip can carry the deploy key;
  `ssh_command(identity_file=None)` composing an identity on top of the pinned
  `BatchMode=yes`/`ConnectTimeout=10`/`StrictHostKeyChecking=accept-new`, and a per-subprocess
  `ssh_command=` parameter on the one git spawner, module-level `run_git` (which
  `Hub._raw_run` and `workqueue.Queue._git` both delegate to), which sets `GIT_SSH_COMMAND`
  unconditionally rather than honouring an ambient value; credential-helper env in `run_git`
  reading `fleetlib.git_token_file()` — `FLEET_GIT_TOKEN_FILE`, else `~/.keel/secrets/git-token`
  when it exists, so the hand-run steps of this stage inherit the PAT the units already had;
  `tools/fleet/keel/git-credential-file`; three rate-limit strings in `_TRANSPORT_HINTS`;
  `fetch_namespace(prefix)`.
- Route every code call site per SPEC §4.4's table: `workqueue.py` (tip sha, `staging/*` listing,
  ancestry fetch — the last through `fleetlib.run_git`, not a bare `subprocess.run`),
  `dispatch.py` (object fetch, branch sha), `train.py` (clone source, tip reads, and all four code
  PUSHES — the tip at `tip_push_url` with the deploy key, `rescued/*`, `staging/*` retirement and
  `staging/train-tmp-*` at `code_push_url` with the PAT), `agentworker.py` (clone + branch/tip
  probes), `fleetd.py` (three `refs/heads/*` probes), `cli.py` (`_hub` needs `--code` /
  `FLEET_CODE_URL`: `cmd_status` computes the queue, which is a CODE read, and without it the
  QUEUE line is a permanent error no flag can fix).
  `workqueue.Queue.compute_or_refusal()` turns a missing tip into a `refused` reason instead of a
  `QueueError` that the reconcile loop does not catch. `tests/test_code_url_split.py` (two bare
  repos: `state.git` with no `refs/heads/*` at all).
- De-hardcode: `gate.sh` L243 + L373 → `FLEET_CODE_URL` required; `gate.sh` L223/L441,
  `fleetd._exiftool_cache_dir` → `EXIFTOOL_CACHE_DIR`, whose default is spelled in exactly two
  mirrored places — `tools/fleet/config.py` (`DEFAULT_EXIFTOOL_CACHE_DIR`, imported by
  `fleetd.py`/`doctor.py`/`ledger.py`) and `units/fleet-env.sh` (sourced by `gate.sh`) — with
  `tests/test_no_hardcoded_hosts.py` forbidding both the contiguous literal and the two-piece
  basename reassembly everywhere else (R6); `units/fleetd.service` L22, `com.oxidex.fleetd.plist`
  L14, `cron-backstop.txt` L45 → `FLEET_HUB_URL=<state repo https>`, `FLEET_CODE_URL=<code repo
  https>`; `fleetd --hub <state> --code <code>`.
- The heartbeat payload `fleetd.reconcile_once` builds (the `hb` dict, written by
  `fleetd.write_heartbeat`) gains `refused: ReconcileResult.refused`; `cli.cmd_status`
  gains `--why` rendering it per host. Stage 1d (T3) adds `warnings: ReconcileResult.warnings`
  beside it — durable conditions swept from the `~/gatelogs` marker files by
  `fleetd.HostWarnings.scan` every reconcile, rendered by `--why` as `warning:` lines (SPEC §3.1).
- `train.py`: after a successful tip push, CAS-bump `refs/fleet/signals/tip` via `Hub.update`
  (replaces post-receive for the hubless interim); `tip_push_options` returns `[]` when
  `FLEET_TRAIN_TOKEN_FILE` is absent (already the behaviour); the tip push goes through
  `hub.push_tip_ref` and carries the deploy key as a per-subprocess `ssh_command=` argument, never
  `os.environ` (the singleton's renewer thread pushes state refs from the same process throughout
  the gate); `--code` / `--code-push` / `--tip-push` on the CLI (`FLEET_CODE_URL`,
  `FLEET_CODE_PUSH_URL`, `FLEET_TIP_PUSH_URL`), with `--tip-push` the ssh URL a deploy key can
  actually authenticate and `--code-push` left on HTTPS so `rescued/*` and the retirements keep
  the PAT. Pointing `--code-push` at ssh still works and now warns, because it is the shape that
  ran three pushes under an ambient ssh identity.
- `rollout/seed_desired.py` seeds the state repo with all hosts at 0/0 + `server_candidates`,
  `train_platforms`.
- Import of the i7's `~/gatelogs/gate-*.json` into the cache via `verdict.py store`.
- `fleetd` on the i7 (targets 1/0); m5 at 0/0 (laptop; `gates: 0` by policy). The merged
  `units/fleetd.service` `ExecStart` passes no `--interval`, so the i7 runs at fleetd's default
  `fleetd.LOOP_SECONDS` (15); `--interval 30` is a hand-start knob, and putting it on
  the unit is a one-line unit change, not something this deliverable already did.

**Tasks.**
| task | tier | ∥ | h |
|---|---|---|---|
| Rulesets ×5 via `gh api`, deploy key as bypass actor, live ruleset test | Opus | ∥ | 1.5 |
| `fleetlib` credential helper + hints + `code_url` + `fetch_namespace`; `test_fleetlib` parametrized `FLEET_TEST_HUB_URL` (scratch private GitHub repo) | Opus | ∥ | 2.0 |
| `code_url` in the three borrowers + `test_code_url_split.py` | Sonnet | ∥ | 1.0 |
| De-hardcode URLs/paths in gate.sh, fleetd, units; `rg` fence | Sonnet | ∥ | 1.0 |
| `refused[]` in heartbeat + `fleet status --why` | Sonnet | ∥ | 1.0 |
| Tip-signal bump in train + deploy-key push; `seed_desired` fields | Sonnet |  | 1.0 |
| Verdict import from i7 gatelogs; bring up i7 then m5; one real gate | Sonnet |  | 1.5 |

**Acceptance (instrument).**
- `FLEET_TEST_HUB_URL=https://github.com/swack-tools/keel-scratch.git python3 -m unittest
  tools.fleet.tests.test_fleetlib` → 0 failures including `TestConcurrentCreate` (8 racers, exactly
  one winner) and `TestReadIsCoherentUnderConcurrentWrites` (instrument: that unittest run's
  output, pasted in the PR); p50/p95 of `create`/`update` recorded with `hyperfine -N` in the commit.
- The push split and the token default, hermetically, in one run:
  `cd tools/fleet/tests && FLEET_TESTS_HERMETIC=1 python3 -m unittest test_code_url_split
  test_fleetlib test_train test_queue`. The three classes that make it a measurement rather than a
  restatement of the code: `TestTipPushUrlIsSeparateFromTheOtherCodeWrites` (the tip lands on
  `tip_push_url`, `rescued/*` does not, and `push_code_ref`/`delete_code_ref` raise `TypeError` on
  `ssh_command=`), `TestTrainDeployKeyIsScopedToOneSubprocess` (with `FLEET_TEST_RENEW_S=1` the
  train singleton's OWN claim renewer pushes while the tip push is held open, and that push's
  `env=` carries the pinned default rather than `-i <deploy key>`), and `TestDefaultTokenFile`
  (with HOME redirected and `FLEET_GIT_TOKEN_FILE` unset, `git credential fill` returns the token
  from `~/.keel/secrets/git-token`). Every one reads the `env=` dict handed to `subprocess.run`,
  not a spy on a `Hub` method — the instrument that was green while this feature was broken in
  three ways. (Instrument: that unittest run's `Ran N tests / OK`, pasted in the commit.)
- `tests/live/test_tip_ruleset.py` on `keel-proof/x`, **and the expected matrix depends on which
  half is deployed** — the earlier "keyless FF → `GH013`" line was wrong for the state the repo is
  actually in. Guard half only (today): keyless FF → rc 0 (a guard blocks deletion and force-push,
  not updates); keyless `--force` → rc≠0; keyless delete → rc≠0; PAT push to `rescued/proof` then
  delete → rc≠0. Both halves (once the deploy key exists and `proof-update` is created): keyless
  FF → rc≠0 with `GH013` in stderr; deploy-key FF → rc 0; deploy-key `--force` → rc≠0;
  deploy-key delete → rc≠0. (Instrument: captured `git push` rc + stderr per case.)
- One real gate on the i7 of a real `staging/*` branch: `~/gatelogs/gate-<tag>.verdict` = PASS and
  `git ls-remote <state> 'refs/fleet/verdicts/*'` lists `(tree,7,<i7 platform_id>)` (instrument:
  both commands' output).
- `fleet status --why` from m5 shows i7 `up` with heartbeat age < 60 s and m5 `refused:
  target-zero (gates 0 / agents 0)` (instrument: the command output; `disabled (<reason>)` is the
  line only for a host an operator took down with `fleet down`, and m5 is seeded ENABLED at 0/0).
  A host that is ENABLED but computes nothing must also produce a line rather than an empty
  `refused[]` — `target-zero` for the by-design idle case above, `queue-empty: N in queue, …`
  when every queued branch is running/awaiting-train/needs-author, and `queue-unavailable: tip
  ref 'refs/heads/refactor/tag-machinery' does not exist on the code repo …` when `--code` is
  misconfigured, which is the failure this stage's split makes possible. The hermetic bring-up
  simulation `tools/fleet/tests/test_bringup_split.py` pins the `target-zero` line end to end
  (real `seed_desired.py`, real `fleetd.py --hub <state> --code <code>`, real `cli.py status
  --why`, two local bare repos).
- `rg -n "work2.oxidex.net|/home/allen/git/oxidex.git|/tmp/oxidex-exiftool-cache" tools/fleet
  units` returns only comments in the incident history (instrument: that ripgrep).
- Full suite green under `gate.sh` fleet-tests stage (instrument: the gate verdict JSON).

**What the human sees.** `fleet status --why` works from the laptop with the ryzen dark; the i7
gates staging branches again; a reason line replaces silence; the tip cannot be pushed by anyone
but the train's key. Reversible by flipping `FLEET_HUB_URL`/`FLEET_CODE_URL` back.

---

## Stage 2 — `keel-server` core, provably a cache; `ServerHub`/`FallbackHub` — 16 h

**Goal.** A server that can die without anyone noticing except the scheduler. No runner yet
(fleetd keeps polling); the server only indexes, brokers CAS, serves status/events, and holds its
own lease.

**Deliverables.** `tools/fleet/keel/{server.py, cachedhub.py, serverhub.py, fallbackhub.py,
election.py}`; `/v1/health`, `/v1/refs` CAS façade (`?fresh=1`), `/v1/status`, `/v1/events`
(sqlite ring, `since`, keepalive comments), `/v1/desired`; bearer auth with hashed tokens;
connection cap + listener watchdog; server lease `Claim(kind="server")` with `advertise_urls`;
settle flag; `keel status|events|server {status,rehost}`; test fixture switch `FLEET_TEST_HUB=server`
(fixture server on 127.0.0.1 over a `git init --bare` state repo + a second bare code repo);
`tests/test_serverhub.py`, `tests/test_fallbackhub.py` (fault-injecting primary).

**Tasks.**
| task | tier | ∥ | h |
|---|---|---|---|
| `CachedHub`: index, monotonic sweep rule, write-through, **fresh-claims rule** (§4.3 r1) | Opus |  | 3.0 |
| `FallbackHub`: **no-write-retry-after-ambiguous-failure** (§4.3 r2), sticky 30 s, `.url/.workdir/.code_url` of the GitHub half; `test_fallbackhub.py` incl. timeout-after-CAS case | Opus | ∥ | 2.5 |
| `ServerHub` + `test_serverhub.py` (409/404/503 mapping, ProcessPool one-winner through the server) | Opus | ∥ | 1.5 |
| HTTP/1.1 keep-alive + chunked SSE + long-poll plumbing in `ThreadingHTTPServer`, connection cap, watchdog | Sonnet | ∥ | 4.0 |
| Server lease/election skeleton (`election.py`), settle flag, `advertise_urls`, unreachable-demotion timer | Opus |  | 2.0 |
| `keel` CLI over FallbackHub (`status/events/server`), `--direct` | Sonnet | ∥ | 1.5 |
| Fixture switch `FLEET_TEST_HUB=server` in `FleetlibTestCase.setUp` (test_fleetlib L41-72) and the other fixtures; justfile recipe running the suite twice | Sonnet | ∥ | 1.5 |

**Acceptance (instrument).**
- Every `test_*` except seams green under both `FLEET_TEST_HUB=bare` and `=server` (instrument:
  the gate's fleet-tests stage, run twice by the justfile recipe; 0 failures each).
- `test_fallbackhub.py`: a primary that times out *after* executing the CAS makes `update` raise
  `HubUnreachableError` (never `False`, never a second write); a primary that refuses connection
  falls back and returns GitHub's result (instrument: unittest).
- `test_serverhub.py`: claim-namespace `sha()` hits the store live — a test that corrupts the
  index entry for a claim still gets the true sha (instrument: unittest with an injected stale
  index).
- Re-host drill in the fixture: kill server A mid-test, `keel server rehost` on port B,
  `diff <(A status --json) <(B status --json) | jq 'del(.ts,.server)'` empty (instrument: the
  test's jq diff).
- Live: `keel-server` on the i7's tailnet address; `keel status` from m5 over the tailnet; kill
  it; `keel status --direct` still answers (instrument: both commands' output with timestamps).

**What the human sees.** A dashboard-less but live `keel status`/`keel events --follow` from the
laptop; killing the server on the i7 changes nothing but the `server:` line.

---

## Stage 3 — `keel-runner`: outbound protocol, journal, fallback renew, election — 18 h

**Goal.** Replace polling fleetd with a runner that holds its own leases through the server or
around it, re-announces on reconnect, and can elect/host the server.

**Deliverables.** `tools/fleet/keel/runner.py` (fleetd local half verbatim + register/heartbeat/
long-poll/commands/log spool + journal + offline start + `autonomous_when_serverless` gates-only);
`~/.keel/runner.toml`; `doctor.py --json` + NTP-offset check as the registration payload;
`verdict.py --server-url`; `gate.sh` verdict args; `units/*` re-pointed (logs under `~/.keel/log`);
`tests/test_seams_keel.py` with seams 1, 2, 4, 6, 7, **8, 9, 10, 11**; `tests/test_journal.py`.

**Tasks.**
| task | tier | ∥ | h |
|---|---|---|---|
| fleetd split: move the local half verbatim; wire `FallbackHub`; keep reconcile ordering and lost-lease kill | Opus |  | 4.0 |
| Job journal + `adopt_workers` journal evidence + offline start (no rc 5) | Opus | ∥ | 2.5 |
| Register/heartbeat/long-poll/commands/spool+replay; `live_workers[]` join on the server; settle honoured | Opus |  | 3.0 |
| Election in the runner (rank backoff, laptop exclusion, fork server in own session, unreachable-demotion) | Opus | ∥ | 2.0 |
| `autonomous_when_serverless` (gates only, `--interval 60`, 60 s lease-absence gate) | Opus | ∥ | 1.0 |
| Seams 8–11 + re-created 1/2/4/6/7 | Opus |  | 3.0 |
| `doctor --json` + NTP check; `verdict.py --server-url`; gate.sh arg; units; runner.toml | Sonnet | ∥ | 2.5 |

**Acceptance (instrument).**
- `test_seams_keel.py` green in CI mode: seam 8 (server SIGKILLed mid-gate: `ClaimWatcher` shows
  the ref renewed, never absent; verdict on the store; spawns = 1), seam 9 (route flip never marks
  `lost`; negative control red), seam 10 (re-host mid-gate; spawns = 1; status diff empty), seam 11
  (ambiguous write raises; lease stays held) (instrument: the seams' own counters/samplers).
- Seam 6 negative control still goes red when `start_renewer` is disabled (instrument: unittest).
- `FLEET_SEAMS_SLOW=1` 13-minute burn green once through `ServerHub` (instrument: unittest log).
- Live: i7 + m5 runners registered; `keel up server --gates 1` gates one real staging branch; the
  verdict ref's `written_by` names the i7 (instrument: `git ls-remote <state>` + payload); kill
  the server on the i7 mid-gate → the gate finishes, verdict lands, runner log shows `degraded →
  direct renew` lines, and `keel events` after restart shows `runner.registered` with
  `live_workers: 1` and no second spawn (instrument: `~/gatelogs` count + events).
- `kill -STOP` the i7 runner: `runner.down` at ≤ 75 s live; `SIGCONT` → re-register, no kill
  (instrument: event timestamps).

**What the human sees.** `keel status` shows runners with capabilities, live WORK, and a
`degraded` flag when they are routing around the server; spinning a host up/down is an API call;
logs stream with `keel logs <job> --follow`; the fleet keeps working through a server kill.

---

## Stage 4 — Scheduler and train server-side; `keel why` — 14 h

**Goal.** The server decides; runners execute. The train is a thread with a remote gate; the
tip watcher replaces the hook.

**Deliverables.** `tools/fleet/keel/scheduler.py` (`classify_branch`, economics, desired targets,
capability matching, offers; `ReconcileResult.refused` → `/v1/why`); `remote_gate` in `train.py`;
train thread + temp-ref GC + `train_platforms`; tip watcher (`signals/tip` CAS bump, `tip.foreign`);
`written_by`-registered-runner check for PASS admission; verdict-ref GC (30 d, never the current
tip's tree); `keel why|jobs|train {status,run,dry-run}`; dashboard v0 (hosts, queue, jobs, log
tail; zero external requests).

**Tasks.**
| task | tier | ∥ | h |
|---|---|---|---|
| Scheduler: selection, capability match, offers, `record_dispatch`-before-offer, settle/renew-first | Opus |  | 4.0 |
| `remote_gate` (cache-first, temp ref, wait on `verdict.stored` for the exact tree, 90-min ABORT, CAS-delete) + GC | Opus | ∥ | 3.0 |
| Tip watcher + `test_tip_watcher.py` (monotonic under N processes); `tip.foreign` alert | Sonnet | ∥ | 1.5 |
| `/v1/why` + `keel why`; `keel jobs/train`; dashboard v0 | Sonnet | ∥ | 3.0 |
| Verdict admission check + verdict GC | Opus | ∥ | 1.0 |
| Seam 3 re-created through `remote_gate` + stub runner gates; `test_train`, `test_queue_truth` under `=server` | Sonnet |  | 1.5 |

**Acceptance (instrument).**
- `test_train.py`, `test_queue_truth.py`, `test_dispatch.TestFleetdDispatch` equivalents green
  under `=server` (instrument: unittest).
- Seam 3: poison ejected, survivors land, fixture tip advances exactly once, union-failed set never
  pushed, intent flips `done`; SIGKILL the server mid-bisect and re-host → `gate_invocations`
  equal before/after restart because re-gates are cache hits (instrument: `keel train status --json`).
- Live: one train run lands one real `staging/*` branch — `git ls-remote code
  refactor/tag-machinery` advances to the squash commit whose tree has a PASS verdict at gate
  version 7 on the i7's `platform_id`; `rescued/<slug>` = the member sha; staging ref CAS-retired;
  `refs/fleet/signals/tip` generation +1 (instrument: those four reads).
- `keel why` on a deliberately disabled runner prints `disabled` within one tick; on a runner
  with `free_gb` forced below 14 prints `limits: free … < 14G` (instrument: command output).
- `grep -cE 'https?://' tools/fleet/keel/dashboard.html` = 0 (instrument: grep).

**What the human sees.** Branches flow staging → gate → AWAITING_TRAIN → tip without cron, ssh,
or a human; `keel why` answers the question; the dashboard shows the queue and live logs.

---

## Stage 5 — Agents first-class — 16 h

**Goal.** Replace blind CLI spawns with server-managed agents: roles, budgets, scoped tools,
structural doctrine enforcement, results verified by the store.

**Deliverables.** Agent registry (`refs/fleet/agents/*`), HMAC-derivable tokens, `keel/mcp.py`,
`keel/agentrun.py` (converger, author; codex adapter), sandbox one-ref remote
(`keel/sandbox_hook.sh`), `keel/settings_hooks.json` (PreToolUse denials), transcript streaming +
disk retention, reviewer role + `refs/fleet/reviews/*` feeding dispatch, `measure_baseline` probe
job, daily USD cap, `keel agents {list,show,tail,cancel}`, `tests/test_agents.py`,
`tests/test_no_secrets_in_refs.py`, `test_no_raw_hub_push` extended (HTTP writes only in
`serverhub.py`; one allowed push in `agentrun.py`).

**Tasks.**
| task | tier | ∥ | h |
|---|---|---|---|
| Registry, HMAC tokens, role allowlists enforced server-side, budgets + kill paths | Opus |  | 4.0 |
| `agentrun.py`: sandbox clone + one-ref remote hook + settings hooks + stream-json usage parsing + structured result; prompts verbatim | Opus |  | 3.5 |
| `mcp.py` stdio shim → `/v1/agents/{id}/tools/*` | Sonnet | ∥ | 1.5 |
| Reviewer role + classification → dispatch; `measure_baseline` probe job on `has_oracle` runner | Opus | ∥ | 2.5 |
| Transcript ring/tail, disk retention, `keel agents` CLI, dashboard agents panel | Sonnet | ∥ | 2.0 |
| `test_agents.py` (stub CLI calling MCP tools), fences (`no_secrets_in_refs`, extended raw-push fence) | Sonnet | ∥ | 2.5 |

**Acceptance (instrument).**
- `test_agents.py`: result verified by ref movement not by the agent's word; USD stream past
  budget → group killed, status `over_budget`, attempt counted; rc 8 refunds; rc 9 counts and the
  cap binds at 3 with cooldown 1800 s (`test_dispatch` unchanged); a converger calling
  `set_desired` → 403 + `agent.denied` event; a token verified by a *second* fixture server with
  only the HMAC key (instrument: unittest).
- Sandbox: a test prompt that pushes `HEAD:refactor/tag-machinery` and a second ref is rejected by
  the sandbox `pre-receive` with both attempts logged; `git ls-remote code` unchanged (instrument:
  hook log + ls-remote).
- Settings hooks: `exiftool -ver` (bare), `Edit src/exiftool_tables/binary_tables.rs`, and
  `git push origin a b` are denied and appear as `agent.denied` (instrument: events).
- Live: one convergence agent on a deliberately stale branch moves the ref on the code repo and
  `git merge-base --is-ancestor <tip> <new_sha>` holds on the server mirror; ledger count resets
  to 0 (instrument: `keel agents show` + `GET /v1/refs/refs/fleet/attempts/<key>`); the
  transcript exists only at `~/.keel/agents/<id>/transcript.jsonl` and `git ls-remote <state>
  'refs/fleet/transcripts/*'` is empty (instrument: both).
- One author run on a real intent: `staging/<slug>` appears, and the server-dispatched probe's
  MISSING count ≤ the baseline quoted in the agent's result (instrument: probe job result JSON,
  which names `scripts/compare_file.py` and the pinned oracle).

**What the human sees.** `keel agents` lists who is working on what, with spend; an agent cannot
push anywhere but its branch, cannot run bare exiftool, and its success is what the store says.

---

## Stage 6 — OPERATOR agent, alerts, human channel, chaos drill — 12 h

**Goal.** The human's chat session stops being load-bearing.

**Deliverables.** Alerts (runner DOWN, lease lost, spine unreachable, train stalled, budget cap,
verdict conflict, sweep disarmed, `tip.foreign`, unknown verdict writer); OPERATOR loop +
`keel/operator_prompt.md` + allowlist + rate limits + loop guard + `report_only`; GitHub issue
channel on the private repo + `keel inbox|answer|report` + `KEEL_NOTIFY_URL`; operator journal
on the spine; dashboard v1 (alerts, operator, agents, train); `docs/KEEL-RUNBOOK.md` (re-host,
secrets bundle with checksums, host facts).

**Tasks.**
| task | tier | ∥ | h |
|---|---|---|---|
| OPERATOR loop: wake conditions, Sonnet tick / Opus escalation, budgets, loop guard, journal resume | Opus |  | 4.0 |
| Operator prompt + tool allowlist + `report_only` gating | Opus | ∥ | 2.0 |
| Alerts + events wiring | Sonnet | ∥ | 1.5 |
| Issue channel + inbox/answer + notify webhook | Sonnet | ∥ | 2.0 |
| Dashboard v1; runbook | Sonnet | ∥ | 2.5 |

**Acceptance (instrument).** Scripted chaos drill on the real fleet with zero human ssh: (a)
`kill -STOP` the m5 runner; (b) `kill -9` the server on the i7; (c) revoke m5's PAT; (d) wedge a
gate with a lost lease (`FLEET_TEST_TTL_S` on one spawn). Within 10 min each incident is an alert
AND an `operator.report` that names it with the correct instrument (`heartbeat age`, `keel why`,
`events.seq` delta, `claim.lost` reason), and every action taken is within the allowlist and
logged (instrument: `keel events --since <drill-start>` export reconciled line by line against the
drill script; `gh issue view --comments` on the private repo shows the reports). A forced
`keel server move` makes the next wake carry `resumed_from` the prior journal seq (instrument:
events). Daily cap forced to $0.01 → only `post_report` is callable and the report says so
(instrument: events + 403s).

**What the human sees.** Reports arrive on the phone (issue comments / ntfy) naming the
instrument; questions land in `keel inbox`; the human answers in the issue; no watcher loop, no
ssh, no chat session required.

---

## Stage 7 — Hardening, decommission, burn-in — 6 h + 24 h wall

**Goal.** Remove the legacy path, prove the steady state, write it down.

**Deliverables.** Delete `hooks/`, `rollout/install_hook.sh`, `drift.py`, cron train entry,
fleetd units; `fleetd.py` → shim exiting 2 ("use keel-runner"); `test_drift_hook.py`/
`test_update_hook.py` deleted (replaced in Stages 1/4); `docs/KEEL.md` with `FLEET.md` §7 carried
verbatim; blackhole drills (runner-only; runner+server) and the laptop-sleep drill recorded in the
runbook with numbers; `fleetd --autonomous` path kept only as `autonomous_when_serverless`.

**Tasks.**
| task | tier | ∥ | h |
|---|---|---|---|
| Blackhole + sleep drills, recorded | Opus | ∥ | 2.0 |
| Deletions, shim, fences, docs | Sonnet | ∥ | 3.0 |
| Burn-in review (every `job.lost`/`runner.down` explained) | Opus |  | 1.0 |

**Acceptance (instrument).** `/etc/hosts` blackhole of github.com on m5 for 15 min with the server
up → its gate finishes and the verdict lands via the brokered route (instrument: `git ls-remote
<state>`); blackhole on the i7 (server + runner) for 15 min → runners elsewhere continue, gates on
the i7 are killed at ≈ TTL−renew with `claim.lost` reason logged, no duplicate verdict (`conflict`
count 0) (instrument: events + verdict refs); `pmset sleepnow` 20 min on m5 mid-gate → on wake
either renew-within-TTL continues or `lost → killed`, never a duplicate landing (instrument:
events); `rg -n "work2.oxidex.net|hooks/|install_hook|drift\.py" tools/ docs/` returns only the
changelog/incident table (instrument: ripgrep); gate PASS at v7 with the legacy files gone
(instrument: verdict JSON); 24 h with all available hosts enabled: every `job.lost` and
`runner.down` reconciled to a deliberate action or an OPERATOR-reported outage; cache-hit rate and
merges/hour from `/v1/verdicts` and `signals/tip` generation deltas in the burn-in report
(instrument: `keel events --kinds job.lost,runner.down` export + the report).

**What the human sees.** One runbook, one dashboard, one CLI; the old hub is a historical note;
the fleet runs unattended and tells the human when it needs a decision.

---

## Totals and ordering

| stage | hours | Opus-class | Sonnet-class | parallel width |
|---|---|---|---|---|
| 1 Hubless spine | 9 | 3.5 | 5.5 | 5 |
| 2 Server core | 16 | 9 | 7 | 4 |
| 3 Runner | 18 | 15.5 | 2.5 | 3 |
| 4 Scheduler + train | 14 | 8 | 6 | 4 |
| 5 Agents | 16 | 10 | 6 | 4 |
| 6 OPERATOR | 12 | 6 | 6 | 4 |
| 7 Hardening | 6 (+24 h wall) | 3 | 3 | 2 |
| **total** | **91** (+20 % review ≈ 109) | 55 | 36 | |

Dependencies: 1 → 2 → 3 → 4 → 5 → 6 → 7 strictly; inside a stage the ∥ tasks may be farmed to
separate agent sessions on separate `staging/keel-N-<task>` branches, each gated independently,
and the stage's integration branch is gated last. Stage 1 alone restores an automatic fleet on the
hosts that exist today (m5 + i7); Stages 2–3 make the server optional; nothing after Stage 3 is
required for resilience, only for visibility and autonomy, so each later stage can be deferred
without losing what the earlier ones bought.

What is deliberately not scheduled: the witness rule (SPEC §12), Litestream/S3, FastAPI/venv,
macOS gate qualification, a second regen host. Each would be a new stage with its own
negative-control seam; none blocks the goals in SPEC §0.
