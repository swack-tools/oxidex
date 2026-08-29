# Fleet Coordination — Implementation Spec

**Status:** HISTORICAL — superseded by `docs/AGENT-SERVER-SPEC.md` (§3–§4 topology) and by the
ryzen's removal from the fleet (operator decision, 2026-08-22): the ryzen (`ubuntuwork`, the
work2 pod) and its `work2.oxidex.net:2244` hub below are history, not current hosts. Kept for the
incident table and mechanism rationale. · **Scope (as written):** 4 hosts (i7 `server`, ryzen
`ubuntuwork` (removed 2026-08-22), M4 `oldair`, m5 localhost) · **Substrate (as written):** the
old hub at `work2.oxidex.net:2244` (retired)

---

## 1. What we are actually fixing

Every mechanism below traces to an incident from today. This is not a greenfield design; it is a set of interlocks for failures that already happened.

| # | Incident | Root cause | Mechanism |
|---|---|---|---|
| 1 | `solo-ryzen5` reimplemented SWF/PICT/PPM/RA/Kyocera routing already merged as `5cef5b3d` | no way to ask "is this already done?" before starting | M5 Intent registry |
| 2 | i7 and ryzen both gated `solo-ryzen4` simultaneously | no claim/lease on work items | M1 Claims |
| 3 | i7 gated `staging/solo-ryzen5` an hour after it was retired | per-host `~/.train-queue` file copies drift from the hub | M1 + queue-from-refs only |
| 4 | M4 produced FAIL on every branch incl. the known-good tip | rustc 1.90.0 vs fleet 1.97.1 — verdict depended on which host ran it | M7 Toolchain pinning |
| 5 | `vf-fix-conds12` failed a `615 vs 613` assertion the tip no longer contains | gated a branch 24 commits behind tip | M3 Drift budget + M6 gate-the-merge-result |
| 6 | Census `assert_eq!(enabled, 5)` broke after two independently-green merges | no gate on the *combination* | M6 Merge train |
| 7 | `verify_subdirs.py` 8-vs-11 tuple broke after Step 25 | same | M6 + conflict domains |
| 8 | ryzen cron dead all day; m5 launchd respawn loop into OOM | schedulers fail silently, no health signal | M2 Reconciler + heartbeats |
| 9 | Killing a gate orphaned `cargo`/`rustc` holding ~10 G each | no process-group ownership | M8 Supervision |
| 10 | i7 kept 31 stale staging refs (`fetch --prune` doesn't prune into local branches) | refs mirrored by copy | queue-from-refs only |
| 11 | 35 queue branches were ~10 distinct items | no dedup at submit time | M5 |
| 12 | `rm -rf ~/tgt/*` killed 7 live gates | cleanup didn't know what was live | M1 leases as liveness truth |
| 13 | ~11 measurement errors (`pgrep -c`, wrong binary, wrong profile, wrong remote) | instruments not self-identifying | M9 (already partly built) |

**The single most expensive failure class is drift**, and it has two faces: a branch drifting from the tip (5, 6, 7) and an agent's *knowledge* drifting from reality (1, 3, 11). Both are addressed by making the hub the only source of truth and forcing continuous convergence rather than batch reconciliation.

---

## 2. Design principles

1. **The hub is the only truth.** No host keeps a queue file, a branch list, or a verdict cache that isn't derived from hub refs on read. Copies drift; today they did.
2. **Coordination state lives in git refs, not a daemon.** Ref creation is an atomic compare-and-swap, every host already has authenticated ssh to the hub, and the state survives every reboot and every dead cron. We have watched two schedulers die today — the design must not put truth inside a process.
3. **One ref per key.** Writers touch disjoint refs, so there is no contention and no lock.
4. **Gate the merge result, never the branch.** A verdict about a branch is not a verdict about the tree that will exist after merging it.
5. **Verdicts are content-addressed.** Keyed by the tree hash of the merge result plus the toolchain identity, so identical work is never repeated and a verdict can never be silently attributed to a different tree.
6. **Refuse rather than approximate.** Same doctrine as the tag work: an unresolvable state is reported and counted, never guessed. A wrong-but-green merge is the most expensive outcome available.
7. **Every number names its instrument.** Already enforced by `scripts/instrument.py`; extended here to verdicts.

---

## 3. Ref layout on the hub

```
refs/heads/refactor/tag-machinery     # the tip — ONLY the merge train writes here
refs/heads/staging/<slug>             # candidate work
refs/heads/rescued/<slug>             # preserved; never auto-deleted
refs/fleet/desired                    # commit; tree holds desired-state JSON
refs/fleet/hosts/<host>               # commit; heartbeat + observed state (only that host writes)
refs/fleet/claims/<kind>/<key>        # existence == claimed; commit body carries holder + expiry
refs/fleet/verdicts/<tree-sha>        # commit; the verdict record
refs/fleet/intents/<slug>             # commit; intent manifest
refs/fleet/train/<epoch>              # the batch currently under test
refs/fleet/signals/tip                # last known tip + monotonic generation counter
```

A ref may point at a commit whose tree carries a single `payload.json`. Reading is `git cat-file`; no checkout needed.

---

## 4. Mechanisms

### M1 — Claims and leases (atomic, no lock server)

Claiming is a ref *create*, which fails if the ref exists. That is the compare-and-swap.

```bash
# claim: succeeds only if nobody holds it
git push hub $(build_claim_commit) :refs/fleet/claims/gate/<tree-sha>   # create-only, no --force
# renew (holder only): force-push a new expiry
git push --force-with-lease=refs/fleet/claims/gate/<key>:<observed> hub <new>:<same-ref>
# release
git push hub :refs/fleet/claims/gate/<key>
```

Claim payload: `{holder_host, pid, pgid, work_kind, work_key, started_at, expires_at, gate_version, toolchain_id}`.

- **Lease TTL** 10 min, renewed every 2 min by the holder. A crashed holder's claim expires on its own.
- **Reaping is CAS'd**: any host may delete an expired claim, but only with `--force-with-lease` against the sha it observed — so two reapers cannot race.
- **Leases are the liveness truth for cleanup.** The reaper deletes a `~/tgt/nc-*` directory only when no live claim names it. This is the direct fix for incident 12; `rm -rf ~/tgt/*` becomes structurally impossible.

Claim kinds: `gate/<tree-sha>`, `agent/<intent-slug>`, `train/<epoch>`, `host/<name>` (reconciler singleton).

### M2 — Desired-state reconciler (spin up/down on any host)

`refs/fleet/desired` holds:

```json
{
  "generation": 47,
  "hosts": {
    "server":     {"gates": 3, "agents": 1, "enabled": true},
    "ubuntuwork": {"gates": 2, "agents": 2, "enabled": true},
    "oldair":     {"gates": 0, "agents": 0, "enabled": false,
                   "reason": "rustc 1.90.0 mis-links release dylib; quarantined 2026-08-14"},
    "m5":         {"gates": 1, "agents": 0, "enabled": true}
  },
  "limits": {"min_free_gb": 14, "min_free_mem_gb": 8}
}
```

`fleetd` runs on each host, loops every 15 s:

1. Fetch `refs/fleet/desired` and `refs/fleet/signals/tip`.
2. Count **its own** live gates/agents by claim refs it holds, cross-checked against live process groups. Never `pgrep -c` — that matches the invoking ssh command line and has over-reported all day.
3. Reconcile: start or drain to reach the desired counts, subject to `limits`.
4. Write its heartbeat to `refs/fleet/hosts/<host>`: observed counts, free disk/mem, `rustc -vV` hash, oracle probe result, gate script version.

Spin up/down is then one command that edits one ref — no ssh fan-out, no per-host config:

```bash
fleet up server --gates 4 --agents 2
fleet down oldair --reason "toolchain quarantine"
fleet drain ubuntuwork          # finish current work, start nothing new
```

`enabled: false` is a hard stop: `fleetd` refuses to start work and releases nothing already running (drain, don't kill).

**Surviving dead schedulers.** `fleetd` is started by systemd (`Restart=always`) on Linux hosts that have it (i7 `server`) and launchd (`KeepAlive`) on macOS (M4 `oldair`, m5), *plus* a cron backstop that starts the supervisor if it is absent. The work2 pod is neither — a k8s container with no systemd or launchd — so it runs `tools/fleet/units/fleetd-wrapper.sh` (T7, ARCH-FIX-SPEC.md R8: loop { run fleetd in the foreground; restart on a non-zero exit; exit, don't restart, on a deliberate zero-exit stop }, with a pid-file guard against double-start) kept alive by `tools/fleet/units/cron-backstop.txt`'s cron line. That cron line used to run `fleetd --once` unconditionally every 5 minutes; Stage 1 found this **actively harmful** before R2's lease fix landed — `--once` still ran the full reconcile step including the host-singleton claim's `acquire_or_reap()`, and because the live daemon back then never renewed that claim, roughly every third tick reaped the live daemon's own expired singleton and ran a second scheduler alongside it, all day, on 2026-08-14. The cron line now starts the wrapper only if it isn't already running (a local pid-file check, no hub round-trip) rather than unconditionally reconciling. A host whose heartbeat is older than 3 min renders as **DOWN** in `fleet status` — the ryzen's dead cron would have been visible within minutes instead of a full day.

### M3 — Tip propagation and forced convergence

This is the direct answer to "drift from head is a major issue."

On every update to `refs/heads/refactor/tag-machinery`, the hub's `post-receive` hook bumps `refs/fleet/signals/tip` to `{sha, generation, ts}` (`tools/fleet/drift.py`'s `bump_tip_signal`); `fleetd` polls that ref every 15 s and surfaces the generation it observed in its heartbeat. That much is real and load-bearing.

The rest of this section as originally written — per-worker broadcast on a generation bump, an automatic `git rebase` onto the moved tip mid-engineering, a `MAX_DRIFT_COMMITS`/`MAX_DRIFT_MINUTES` budget enforced at claim time — was never implemented as more than prose: `drift.py` grew a `check()` and a `converge()` with no production caller (no `fleetd`/`workqueue` code path ever shelled out to either), and both were deleted 2026-08-15 (ARCH-FIX R9) rather than left advertising a protection that never ran. Convergence onto a moved tip is superseded by tree-keyed verdict caching (M6) plus LLM convergence agents doing the rebase work directly; the tip signal above is unaffected by the deletion and remains the cheap poll target it always was.

### M4 — Queue derived from refs, never copied

`~/.train-queue` is **deleted**. The queue is computed on every read:

```
queue = {staging/* on hub}
      − {branches already ancestors of tip}
      − {branches with a live claim}
      − {branches whose intent is withdrawn}
```

Incidents 3 and 10 — gating a retired branch, and 31 stale refs surviving `fetch --prune` — both become unrepresentable, because there is no local copy to go stale.

### M5 — Intent registry and duplicate detection

The `solo-ryzen5` fix. Before any implementation work begins, the agent registers an intent:

```json
{
  "slug": "route-legacy-formats",
  "title": "Route SWF, PICT, PPM, RA, Kyocera RAW",
  "scope": {
    "formats": ["SWF", "PICT", "PPM", "RA", "KyoceraRAW"],
    "tags": [],
    "files": ["src/parsers/**", "src/core/format_dispatch.rs"]
  },
  "status": "open", "claimed_by": "ubuntuwork", "created_at": "..."
}
```

Registration runs three checks and **refuses** on a hit:

1. **Open-intent overlap** — any other open intent sharing a format, tag, or file glob.
2. **History** — `git log` over the scope tokens on the tip.
3. **Capability ledger — the strong check.** Ask the *running binary at the tip* whether the capability already exists, rather than pattern-matching text. For formats: is the variant present in `format_dispatch`, and does `just compare-file` on a sample report a non-trivial MISSING count? For tags: is the tag in the enabled allowlist? `solo-ryzen5` fails check 3 outright — SWF/PICT/PPM/RA/Kyocera were all routed and measured at 97.3% before it started.

This is the check worth building carefully, because it is the only one that distinguishes "someone is working on it" from "it is already true." The other two are cheap dedup; this one is correctness.

### M6 — Merge train: gate the result, batch it, bisect on failure

**Gate the merge result.** The gate input is always `tip + branch`, never the branch alone. The gate records the tip it merged against.

**Verdict validity.** A verdict admits a branch to the tip if:
- the tip it was gated against is an ancestor of the current tip, **and**
- no commit in between touched a file in the branch's write set, **and**
- neither touched a declared **conflict domain**.

**Conflict domains** are files encoding cross-cutting invariants, where file-disjointness is *not* sufficient to prove independence. Declared in `fleet/domains.toml`:

```toml
domains = [
  "src/exiftool_tables/mod.rs",          # the census invariants
  "src/exiftool_tables/enabled.rs",      # Gate B allowlist
  "tools/exiftool-tables/verify_subdirs.py",
  "src/bin/jpeg-tag-matrix/baseline.json",
  "src/exiftool_tables/binary_tables.rs",  # GENERATED -- see note below
  "src/exiftool_tables/runtime.rs",        # hand-written, references the generated ExprId enum
]
```

A branch touching any of these goes through the train **solo** and is always re-gated against the exact current tip. Incidents 6 and 7 were both conflict-domain collisions between file-disjoint branches — this is the interlock that catches green-alone-red-together.

**Batching — the main throughput win.** The gate costs 20–45 min, and today we paid it per branch. Instead:

1. Collect up to `BATCH_MAX` (default 8) branches with valid verdicts and disjoint write sets.
2. Merge them onto the tip → one candidate tree.
3. Gate that candidate **once**.
4. **PASS** → push the tip. *N merges for one gate.*
5. **FAIL** → bisect: split the batch, gate the halves, recurse. Eject the culprit, re-gate the remainder, and record the culprit's failure against its intent.

Cost: 1 gate in the common case, `2·log₂(N)+1` when one branch is bad. At N=8 that is 8 merges for 1 gate instead of 8 — the difference between clearing a queue in an hour and clearing it in a day.

**Verdict cache.** Keyed by `(merge-result tree sha, gate version, toolchain id)`. Two hosts computing the same merge derive the same key, so the second one reuses the first's verdict instead of rebuilding. A no-op rebase costs nothing.

### M7 — Toolchain pinning

The M4 incident made a gate verdict a function of *which host ran it*, which means it was not a gate.

1. Commit `rust-toolchain.toml` at the repo root pinning channel and components. Every host then resolves the identical rustc automatically, with no per-host setup.
2. Stamp every verdict with `toolchain_id = sha256(rustc -vV)`.
3. The train **rejects** verdicts whose `toolchain_id` is not the canonical one. A quarantined host physically cannot contribute a verdict.
4. `fleet doctor <host>` asserts toolchain id, linker version, oracle `-ver` **and** the `OOXML.docx → DOCX` capability probe, and corpus file count 4238.

The M4's own defect (root-owned files inside `~/.rustup` blocking `rustup update`, from a past `sudo rustup`) is repaired separately by `chown -R` on the toolchain dir; pinning is what stops the *class* from recurring.

### M8 — Process supervision

Every gate and agent runs inside its own process group — `systemd-run --scope --unit=fleet-<kind>-<key>` on Linux, a dedicated pgid on macOS — recorded in its claim.

- Kill = kill the whole group. Orphaned `cargo`/`rustc` become impossible (incident 9).
- **Leak detector**: a worker-shaped process group (marker match) with no live claim **and carrying this daemon's own scope token** is orphaned; the reaper kills it and logs the leak. The token (`fleet-scope=<12 hex of sha256(hub URL)>`, an inert extra argv stamped at spawn — see `fleetd.fleet_scope_token`) is what entitles a sweep to kill: marker match alone proves a process is gate-*shaped*, not that it is *ours*. Marker-matched groups without the sweeping daemon's token are reported (`unscoped` in the adoption summary) and left alone — a hand-launched gate is never sweepable (caveat: launching by copy-pasting a fleet worker's *exact* argv, token included, re-opts into sweeping — the token is visible in `ps` and is provenance, not a secret), and a test's fixture daemon can never kill the real fleet. Two companion rules share the provenance principle: **kin** — a group whose *session* belongs to a claimed/adopted worker (gate.sh's `set -m` stage subshells: own pgid, the gate's argv, no claim of their own) is part of that worker and never an orphan (`kin` in the adoption summary); and **identity-verified adoption** — a claim is only adopted if its recorded pgid currently names a same-uid group carrying a marker *and* this daemon's token, otherwise it is released (work requeues) and the process — a recycled-pid bystander, or a pre-scope worker across the upgrade boundary — is left alone, closing the recycled-pgid → lost-lease-kill path. Incident 2026-08-20: `TestFleetdSingletonRenews`' fixture daemon, on an empty fixture hub with production markers, swept and killed a live manually-launched gate on the i7 mid-run — the gate's own fleet-tests stage was running the test that murdered it. Pinned end-to-end by `test_lease_protocol.TestFixtureDaemonCannotSweepUnscopedWorkers` (with a verified negative control) and in-process by `test_adoption`'s unscoped/foreign-token tests.
- Disk/memory guards run *before* start, from `limits` in the desired state, and drain rather than evict when a host crosses a threshold.

### M9 — Observability

`fleet status` reads only hub refs:

```
HOST        STATE   GATES  AGENTS  FREE   RUSTC     ORACLE  HEARTBEAT
server      up      3/3    1/1     148G   1.97.1 ✓  13.59 ✓ 12s ago
ubuntuwork  up      2/2    2/2      73G   1.97.1 ✓  13.59 ✓  8s ago
oldair      QUAR    0/0    0/0      25G   1.90.0 ✗  13.59 ✓ 21s ago   toolchain quarantine
m5          up      1/1    0/0     2827G  1.97.1 ✓  13.59 ✓  4s ago

QUEUE 6   CLAIMED 3   TRAIN epoch-19 (4 branches, gating, 12m elapsed)
VERDICT CACHE 31% hit   MERGES/H 5.2   MEDIAN DRIFT 1 commit
```

`MEDIAN DRIFT` is the health metric that matters most; if it climbs above the budget, M3 is not keeping up.

---

## 5. Rollout, ordered by leverage

Not all of this is equally valuable. Recommended order — each phase is independently useful and shippable:

| Phase | Work | Fixes | Effort |
|---|---|---|---|
| **P0** | `rust-toolchain.toml`; stamp verdicts with toolchain id; repair M4 `~/.rustup` ownership | 4 — a whole host is offline today | ~1 h |
| **P1** | Claims (M1) + queue-from-refs (M4) | 2, 3, 10, 11, 12 — duplicate and zombie gating | ~3 h |
| **P2** | Gate the merge result + verdict cache (M6 parts 1–2) | 5, 6, 7 — the correctness gap | ~4 h |
| **P3** | `fleetd` reconciler + desired state + heartbeats (M2, M8) | 8, 9 — and delivers spin up/down | ~6 h |
| **P4** | Batching + bisect (M6 part 3) | throughput — the big speedup | ~4 h |
| **P5** | Tip signal + forced rebase + drift budget (M3) | drift at the source | ~3 h |
| **P6** | Intent registry (M5) | 1 — duplicate implementation | ~5 h |

**P0–P2 are the ones I would not skip.** P0 restores a quarantined machine and removes a whole class of false verdicts for about an hour of work. P2 closes the only *correctness* hole in the list — everything else costs time, but P2 is what stops a wrong-but-green merge reaching the tip.

P4 is where the throughput actually arrives; P3 is a prerequisite for managing it comfortably but the batching logic can be driven by hand first.

P6 is the most speculative and the easiest to over-build. The cheap 80% is check 3 alone — query the capability ledger before starting — and that could be a 30-line script run manually at intent time, well before any registry exists.

---

## 6. Honest risks

- **This is a lot of machinery for four machines.** The coordination layer must not become the thing we maintain instead of oxidex. Everything above is deliberately git-refs-and-shell, with no database, no message broker, and no service to keep alive — because we have watched two schedulers die today and the design has to assume that keeps happening.
- **Batching trades latency for throughput.** A bad branch in a batch delays seven good ones by a bisect. At the current failure rate that is clearly worth it; if branch quality drops, lower `BATCH_MAX`.
- **Forced rebase can interrupt an agent mid-thought** and will occasionally cost work when a rebase conflicts. That is the intended trade: today's alternative was discovering the same conflict 24 commits later, when it was much more expensive.
- **Conflict domains are a hand-maintained list**, so they will go stale. Mitigation: when the train bisects to a culprit whose write set was file-disjoint from the batch, that is evidence of a *missing* domain — log it and require it be added.
- **The capability ledger check is only as good as the instrument behind it.** If it grades against a degraded oracle it will confidently report that everything is already implemented. It must run the same capability probe as the gate.

---

## 7. Addenda from the 2026-08-14 rollout

Three failure classes surfaced while building this and are not covered above.

**A verdict must distinguish FAIL from ABORT.** `staging/rb-s26` holds two
verdicts from the same host: `gate-s26-fix` → `PASS`, and
`gate-staging-rb-s26-c` → `FAIL tests`. The second was rustc taking
`signal: 9, SIGKILL` during the `-C lto -C codegen-units=1` link of the test
binary — an OOM kill, not a code defect. The two are indistinguishable in the
current schema, and that ambiguity is what made two readers disagree about
whether Step 26 was green. Add `result: PASS | FAIL | ABORT` where ABORT covers
OOM, low disk, lost oracle, and killed process, and make ABORT non-admissible
but also non-damning: it schedules a retry rather than condemning the branch.
Memory headroom belongs in the desired-state `limits` alongside disk.

**Work identity is per-host, not global.** The ryzen (`ubuntuwork`) has three
accounts — `allen`, `runner`, `swackhamer` — and its fleet gates run as
`swackhamer`, whose home and `~/gatelogs/` are unreadable to `allen`, the user
ssh lands as. Measuring `allen`'s home there reports no oracle, no corpus and no
verdicts while gates run fine under the other identity; `kill` against them
returns "Operation not permitted". `refs/fleet/hosts/<host>` must therefore
record the **owning user**, and `fleet status` must measure that user's paths.
Without it the reconciler will double-start work it cannot see.

**Toolchain must be measured the way the gate resolves it.** On both Macs a
login shell resolves Homebrew's rustc from `/opt/homebrew/bin`, while the gate
prepends `$HOME/.cargo/bin` and gets rustup's. The two disagreed for most of a
day, and the discrepancy is invisible unless the probe replicates the gate's
exact `PATH`. As of 2026-08-28 exactly one place decides this:
`tools/fleet/toolchain.py` (the PATH prefix, the `rustc -vV` capture, and both
digests), reached from shell by sourcing `units/fleet-toolchain.sh` — which is
what `gate.sh` now does; `doctor.py`, `claim.py` and `verdict.py` all delegate
to it. **"Replicated here" is not a mechanism.** Each of those files used to
carry its own copy of the rule, with prose promising they agreed; see the entry
below for what that cost.

**Still open: macOS gate viability.** `tests/ffi_c_integration.rs` fails in
`--release` on the M4 under rustc 1.90.0, 1.95.0 and 1.97.1 alike, with
`ld: mis-aligned LINKEDIT string pool` in `liboxidex.dylib`; `-Wl,-ld_classic`
is rejected as obsolete by that linker. No macOS host has yet been *verified*
to pass it, so "the M4 is broken" and "no Apple Silicon host can run this gate"
are both still consistent with the evidence. Until one Mac is measured green,
treat macOS gate slots as unproven rather than merely degraded.

**Two tasks computed `toolchain_id` differently, and both were right.** T0.1's
`doctor.py` hashes `rustc -vV` with the `host:` line **stripped**, giving all
four hosts one canonical id (`b5d14336…`). T0.3's `gate.sh` hashes it
**unstripped**, giving the i7 `b6613b19…`. Keying verdict admissibility on
either alone is wrong, because they answer different questions:

- *Is this host on the canonical compiler?* — needs the host line stripped, so
  a Mac and a Linux box on the same rustc release compare equal.
- *Is this verdict transferable to that host?* — needs the host line **kept**.
  `ffi_c_integration` passes on Linux and fails on both Macs at the identical
  rustc release, so a Linux verdict must not satisfy a macOS gate slot.

Carry both: `rustc_id` (stripped, for `doctor.py` and desired-state health) and
`platform_id` (unstripped, part of the verdict cache key). Collapsing them would
let a Linux PASS silently satisfy a macOS host, which is precisely the
cross-platform skew that cost a day here.

**And carry them from ONE implementation.** This section previously recorded
the i7's `platform_id` as `b2bdf493…`; that was never what `gate.sh` wrote. It
is what `claim.py` wrote — `subprocess.run(...).stdout` keeps the trailing
newline that `RUSTC_VV=$(rustc -vV)` strips, so the same compiler hashed to two
values and even the doctrine page blessed the wrong one. On 2026-08-27/28 that
one character meant `fleetd` looked up `refs/fleet/verdicts/<tree>/8/b2bdf493…`
while its own gate published to `…/b6613b19…`: `classify_branch` never returned
AWAITING_TRAIN and the i7 re-gated the same merge tree every ~21 minutes with a
correct PASS sitting unread. Every gate host now derives both ids from
`toolchain.compute_ids`, and `fleetd` refuses to start if the id it computes
differs from the one its own gate command computes.

**`regen.sh` has a hidden host dependency.** The committed oracle ledger's
digest must match the `tables.json` that a host's *Perl* produces, and only the
i7's Perl (5.38.2) currently does — a Mac's Perl yields a different digest, and
`load_oracle_ledger()` then silently returns `None`, refusing **every**
oracle-gated expression (observed: 248 → 7 `ExprId` variants, discovered only
when one of the 241 missing ones broke compilation). Consequences: (a) any
automation that calls `regen.sh` — including `drift.converge()`'s
generated-file resolution — is currently i7-only, and the fleet must route
regen work there until the ledger digest is host-independent; (b) a silent
`None` from a failed digest check is an omit-without-count and must become a
loud refusal. Separately, `verify_exprs.py` carries a pre-existing 1e18-probe
precision mismatch (Nikon PrintAFPoints) that will block the next from-scratch
regen on a non-i7 host — flagged for its own fix, not papered over here.
