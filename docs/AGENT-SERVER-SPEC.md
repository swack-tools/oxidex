# Keel — agent server specification

Status: design, post-review. Supersedes the topology in `docs/FLEET.md` §3–§4 (ref hub on one
bare repo, per-host polling `fleetd`, cron train, chat-session operator). Carries every mechanism
in `docs/FLEET.md` §4 that the 2026-08-14..20 incidents proved; replaces only the substrate those
mechanisms sit on and the process model that hosts them. Base tree: `fd154c5d`
(`tools/fleet/**`, `docs/FLEET.md`, `docs/ROLLOUT.md`, `fleet/domains.toml`).

Provenance: three independent designs, three independent judges. Two judges chose Design 1
("Keel, GitHub-spined, server provably a cache"); the third chose Design 3 by one point and told
us to graft Design 1's claim protocol into it. This document is Design 1 with the grafts all three
judges named and a disposition for every fatal flaw they found (§12). Facts below marked
*(measured)* were re-measured on 2026-08-21 from m5 before this was written.

---

## 0. Goals and non-goals

**Goals**
1. No single box is critical — including the server's. Durable state survives any host dying and
   the server is re-hostable from that state on any eligible host in minutes, automatically.
2. A server outage costs *scheduling* only: every running gate and agent finishes, renews its
   lease, and records its result without the server.
3. Runners connect outbound only (NAT/hotspot/VPN agnostic); spin-up/spin-down/drain are API
   calls; logs stream; `keel why` answers "why is nothing doing anything" from data the daemon
   already computes (`fleetd.py` L821-824 `ReconcileResult.refused`).
4. Agents are first-class: role, budget, tool surface bound to the server API, structured result
   verified by the store, transcript. The OPERATOR role is a server-side loop; the human's chat
   session stops being load-bearing.
5. Keep the proven primitives byte-identical where they fit and re-home them: `fleetlib.Hub` CAS,
   `claim.py` leases, `verdict.py` cache, `workqueue.py` formula, `dispatch.py` economics,
   `intent.py`/`ledger.py` checks, `train.py` batching/bisect/exact-set push, `gate.sh` v7.
6. Incremental migration from the current state (fleet drained, hub down, `fd154c5d` validated),
   each stage gate-able with a named instrument.

**Non-goals**
- Fixing macOS gate viability or i7-only regen (`docs/FLEET.md` §7). Keel routes by capability;
  it does not create capacity.
- A general CI system, a k8s operator, a message broker, Postgres, an object-store dependency.
- Relaxing the pessimistic lost-lease kill (`claim.py` L694-715) — Design 3's "witness rule" is
  explicitly **not** adopted; see §12.
- Re-deriving lease semantics (Design 2's epochs/settle/grace) — `claim.py` is the lease
  implementation, unmodified.

**Hard constraints (from the brief, unchanged):** never push to/merge into `main`
(`refactor/tag-machinery` is the tip); never invoke bare `exiftool`; never weaken a gate to pass
it; every claim of working names its instrument; no heavyweight infra to babysit; macOS + Linux;
secrets in files/env, never in refs.

---

## 1. Architecture in one paragraph

Two GitHub repos are the **spine**: the public code repo `swack-tools/oxidex` (code refs, tip
protected by rulesets) and a new **private** state repo `swack-tools/oxidex-fleet-state` that holds
every coordination ref under `refs/fleet/*` with the same names as today, written through the
unchanged `fleetlib.Hub` CAS (`tools/fleet/fleetlib.py` L397-436: create = non-forced push of a new
ref; update/delete = `--force-with-lease=<ref>:<sha>`), which GitHub's receive-pack honours
identically to the old bare hub. In front of the spine runs ONE `keel-server` (Python stdlib,
zero dependency files) that is **provably a cache**: it indexes `refs/fleet/*` in memory, writes
through to the state repo before acking, runs the scheduler (fleetd's selection half), the train
(a thread, not cron), the agent registry and the OPERATOR loop, and serves an HTTP/JSON API + SSE
to a CLI, a one-page dashboard and per-host `keel-runner`s that connect **outbound** by long-poll.
Runners acquire and renew their own leases (claim-before-launch, exactly `fleetd.start_gate`
today, `fleetd.py` L714-773) through `FallbackHub(ServerHub, GitHubHub)` — server when up, state
repo directly when not — so a server death never lapses a lease or loses a verdict. The server's
own singleton is a `Claim(kind="server")` on the state repo; eligible Linux runners win it by CAS
and re-host from spine state in under five minutes with no human. Agents run under `claude -p`
/`codex exec` in a sandbox clone whose only remote accepts exactly one ref, with a `keel-mcp`
tool server scoped to their role and a settings-file hook that mechanically denies the doctrine
violations the prompts currently only forbid in prose (`agentworker.py` L111-163).

---

## 2. Components

| # | component | runtime | runs on | owns (durable) | fails → |
|---|---|---|---|---|---|
| C1 | **Code spine** `swack-tools/oxidex` (PUBLIC *(measured: `gh repo view` → visibility PUBLIC)*) | GitHub | — | `refs/heads/refactor/tag-machinery` (tip), `staging/*`, `rescued/*`, `staging/train-tmp-*`, `keel-proof/*` (ruleset test refs) | nothing of ours can corrupt it; see §7 "GitHub down" |
| C2 | **State spine** `swack-tools/oxidex-fleet-state` (PRIVATE, new, empty bare repo; no code) | GitHub | — | `refs/fleet/*` (§3) | same |
| C3 | `GitHubHub` = `fleetlib.Hub` unchanged + `code_url` attribute + credential-helper env + three transport hints | py stdlib | everywhere | none (`~/.keel/state.git` object cache is disposable) | raises `HubUnreachableError` (`fleetlib.py` L68), every consumer already handles it |
| C4 | `ServerHub` — the 8-method contract over HTTP (`tools/fleet/keel/serverhub.py`) | py stdlib | runners, CLI, gate.sh via `verdict.py` | none | raises `HubUnreachableError` |
| C5 | `FallbackHub(primary=ServerHub, fallback=GitHubHub)` with the **no-write-retry rule** (§4.3) | py stdlib | runners, CLI, `verdict.py`, `agentrun.py` | none | raises only if both routes fail |
| C6 | `keel-server` (`tools/fleet/keel/server.py`; `http.server.ThreadingHTTPServer`, HTTP/1.1 keep-alive, chunked SSE) | py ≥3.10 stdlib | the current lease holder (i7 primary; ryzen host; m4 only if explicitly ranked) | NONE of truth. Local only: `~/.keel/events.db` (sqlite, lossy ring), `~/.keel/code.git` mirror, `~/.keel/logs/` | runners fall back (C5); election re-hosts (§3.4) |
| C7 | `keel-runner` (`tools/fleet/keel/runner.py` = `fleetd.py`'s local half verbatim + outbound protocol + job journal) | py stdlib | every host, as the owning user, under the existing `units/` supervisors re-pointed | `~/.keel/runner.toml`, `~/.keel/jobs/<key>.json` (journal), `~/gatelogs/`, `~/tgt/nc-*`, `~/.keel/agents/<id>/` (transcripts), process groups | supervisor restarts it; `adopt_workers` re-adopts (`fleetd.py` L1264-1524); host death ⇒ leases expire ≤ 600 s |
| C8 | `gate.sh` GATE_VERSION 7, stage order unchanged | bash | runners | `~/gatelogs/gate-<tag>.{log,verdict,json}` | exit 0/1/7/9 as today (`gate.sh` L355-391) |
| C9 | `agentrun.py` (ex-`agentworker.py`) + `keel-mcp` (`tools/fleet/keel/mcp.py`, stdio JSON-RPC) + sandbox remote + settings hooks | py stdlib | agent-capable runners | `~/.keel/agents/<id>/{transcript.jsonl,result.json,stdout.log}` | killed by group on budget/timeout; outcome recorded in `refs/fleet/attempts/<key>` |
| C10 | `keel` CLI (ex-`cli.py`) with `--direct` | py stdlib | anywhere with a token | — | `--direct` works with the server down |
| C11 | Dashboard: one static `dashboard.html`, inline CSS/JS, zero external requests | — | served at `/` | — | — |
| C12 | OPERATOR agent loop (§6.5) | `claude -p` + `keel-mcp` | inside C6 | journal pointer in `refs/fleet/operator/journal` | stops with the server; resumes on re-host |
| C13 | Tailnet (Tailscale; installed on m5 but not running *(measured: `tailscale status` → "failed to connect to local Tailscale service")*) | — | every host | — | **precondition, not a recommendation**: the server binds only to its tailnet address; there is no public port and no TLS to manage |

Runtime choice: stdlib Python, no `pyproject`, no venv. Reason: the 6,897 lines being re-homed are
stdlib Python already factored through one 8-method interface; m5 runs Python 3.14.7 *(measured)*,
which the Claude Agent SDK may not support, and a venv per host is one more thing to babysit. The
cost is hand-rolled keep-alive/chunked SSE in `BaseHTTPRequestHandler`; the plan budgets a day for
it (PLAN Stage 2) and caps the server at 64 connections with a listener watchdog.

Supervision: `units/fleetd-wrapper.sh` (rc 0 = deliberate stop, pidfile + `kill -0`, forwards
TERM only, never touches gates — L53-113), `units/restart-fleetd.sh`, `units/fleetd.service`
(`Restart=always`), `units/com.oxidex.fleetd.plist` (`KeepAlive`), `units/cron-backstop.txt` are
kept as-is and re-pointed at `keel/runner.py`; logs go under `~/.keel/log/`, never `/tmp` (macOS
purges it; the plist currently logs to `/tmp/fleetd.log`, L12-20). The server is not a separate
unit: it is started by the runner that wins the server lease (§3.4), so "which host runs the
server" is state, not configuration.

---

## 3. State model

### 3.1 Where truth lives

| entity | durable home | writer(s) | payload |
|---|---|---|---|
| desired | state repo `refs/fleet/desired` (unchanged name) | `keel up/down/drain` + OPERATOR, via server or `--direct`; `cli._edit_desired` read-modify-CAS with `generation++` (`cli.py` L52-70) | `generation, hosts{name:{gates,agents,enabled,reason,max_gates,max_agents}}, limits{min_free_gb:14,min_free_mem_gb:8,max_agent_usd_per_day:50}, server_candidates:[{host,rank,advertise_urls[]}], train_platforms:[<linux platform_id>], operator{enabled,report_only}` |
| lease | `refs/fleet/claims/<kind>/<key>`, kinds `gate,agent,host,train` (unchanged) + `server/singleton` (new) | the holder only (runner, or server for `train`/`server`), through `claim.py` unchanged | `claim.py` `_payload` L374-390 unchanged: `holder_host,pid,pgid,work_kind,work_key,started_at,expires_at,gate_version,rustc_id,platform_id,[workdir]`; `server` adds `advertise_urls[],boot_id,keel_version` |
| verdict | `refs/fleet/verdicts/<tree>/<gate_version>/<platform_id>` (unchanged) | `gate.sh store_verdict` → `verdict.py store` (L186-230) via FallbackHub | unchanged schema (`verdict.py` L63-95) |
| attempts | `refs/fleet/attempts/<key>` (unchanged) | server scheduler (`dispatch.record_dispatch` L254 **before** offer), runner (`record_outcome` L269, `not-paid` refund) | unchanged |
| intent | `refs/fleet/intents/<slug>` (unchanged) | `intent.register/withdraw/mark_done` (L281/362/379) via server; ledger check runs as a probe job on a `has_oracle` runner | unchanged |
| tip signal | `refs/fleet/signals/tip` (unchanged payload) | server tip watcher (replaces `hooks/post-receive` + `drift.bump_tip_signal` L133-186; same CAS/monotonic rule via `Hub.update`) | `sha, generation, ts, by∈{train,foreign}` |
| host heartbeat (durable copy) | `refs/fleet/hosts/<host>` (unchanged payload + `refused[]` + `warnings[]`) | server every **5 min** per host (not 60 s: push volume, Judge 2); runner directly when in fallback ≥ 5 min | the `hb` dict `fleetd.reconcile_once` builds, written by `fleetd.write_heartbeat`, including `refused: [(reason, detail)]` from `ReconcileResult.refused` (this loop's scheduling answer, rewritten every reconcile) and `warnings: [(reason, detail)]` from `ReconcileResult.warnings` (Stage 1d, T3: DURABLE host conditions, re-derived every reconcile by `fleetd.HostWarnings.scan` from the marker files in `~/gatelogs` — today exactly one reason, `verdict-store-failed`, for any `gate-<tag>` marker `gate.sh store_verdict` left there, whoever ran that gate — and present in every heartbeat for as long as the marker exists; an absent or unreadable log directory leaves the list unchanged rather than clearing it; `fleet status --why` prints each as `warning: <reason> (<detail>)` under the host's `refused:` lines) |
| agent | `refs/fleet/agents/<id>` (new) | server on transitions (CAS); runner at result via fallback | `id, role, key, runner, model, budget{usd,tokens,seconds,turns}, spent{…}, status, result{outcome,branch,sha,summary,instrument}, token_issued_at, token_expires_at, created_at, ended_at, transcript_sha256, transcript_host` (≤ 2 KB; **no transcript content**) |
| review | `refs/fleet/reviews/<slug>/<sha>` (new) | reviewer agent via server | `class∈{defect,drift,flake,infra}, evidence[], model, agent_id` |
| events checkpoint | `refs/fleet/events/<boot_id>` (new; **lossy by design**) | server every 5 min | one blob, JSONL since last checkpoint, ≤ 10k lines |
| operator journal | `refs/fleet/operator/journal` (new) | OPERATOR loop at end of each wake | `last_event_seq, last_wake_at, last_actions[], open_questions[]` |
| human channel | GitHub issue "Keel operator log" on the **private** state repo | OPERATOR (`post_report`, `ask_human`), human | comments |
| transcripts | runner disk `~/.keel/agents/<id>/transcript.jsonl` only | `agentrun.py` | stream-json; never pushed to any repo (§8) |
| code | code repo `refs/heads/*` | authors/agents (`staging/*`), train (tip via deploy key at `tip_push_url`; `rescued/*`, `staging/<slug>` retirement, `staging/train-tmp-*` via PAT at `code_push_url`) | code |

`refs/fleet/train/<epoch>` from `FLEET.md` §3 remains unimplemented; the train uses
`claims/train/singleton` as today (`train.py` L79-82).

### 3.2 Server memory (cache, never truth)
`RefIndex {ref: (sha, payload, observed_at, source∈{write,sweep})}` rebuilt at boot by
`Hub.fetch_namespace("refs/fleet/")` (new, ~30 lines: one `git fetch '+refs/fleet/*:refs/fleet/*'
--prune` + local `cat-file`; 2 round trips for the whole namespace instead of 2 per ref as
`Hub._read` costs today, `fleetlib.py` L298-395); runner registry; job view (join of live claims
↔ runner-reported workers ↔ log rings); queue recomputed per tick by
`workqueue.Queue(cached_hub).compute()` (never stored, `workqueue.py` L12-19); train progress;
alerts; event ring (sqlite, 50k rows) — the SSE source.

**Index monotonicity rule.** A write-through updates the index entry with `source=write` and the
sha GitHub returned; the sweep may replace an entry only if its `observed_at` is older than the
sweep's own `ls-remote` start time. A sweep can therefore never regress the server's own
write-through. This is necessary but not sufficient (a fallback-direct write by a runner is
invisible until the next sweep), hence §4.3.

### 3.3 Consistency rules
1. **Spine is truth.** Every decision that starts work or spends money is preceded by a CAS that
   lands on the state repo before the server acks (claim create, `record_dispatch`). A stale index
   can make the scheduler *offer* something the CAS then refuses; it can never produce two holders.
2. **Two writer classes, one CAS.** Server (normal path) and runners/gate.sh/agentrun (fallback)
   write the same refs with the same `--force-with-lease` witness; `Hub._augment` provenance
   (`fleetlib.py` L516-525, `written_by=user@host:pid`) shows which path wrote.
3. **Ownership token** stays `(holder_host, started_at)` (`claim.py` L426-442) so a lease renewed
   via the server and then via fallback is one lease; `adopt` continues it (`claim.py` L497-623).
4. **Claims are read fresh.** `ServerHub.sha()/read()/read_with_sha()` on any ref under
   `refs/fleet/claims/` are answered by the server with a live `ls-remote` (+ fetch for `read`),
   never from the index (§4.3). Reads elsewhere may be served from the index with `observed_at`.
5. **Lease numbers unchanged:** TTL 600 s / renew 120 s / clamp ≤ ttl/2 / "declare lost one
   interval before hub-side expiry" (`claim.py` L139-140, L344-346, L694-715); host singleton
   120 s (`fleetd.py` L136-168); train singleton 600 s. New: server lease TTL 120 s / renew 30 s.
6. **The only kill is a lost lease** (`fleetd.py` L1793-1832), executed by the runner that holds
   it. The server never kills a process; it can only send `drain|stop|sweep-orphans|restart-runner`.
7. **Offers are advice, claims are the commitment.** The server offers; the runner
   `Claim.acquire_or_reap()`s (`claim.py` L473-494) with `work_key=branch` exactly as
   `fleetd.start_gate` does (`fleetd.py` L714-773). There is no "offered" claim state and no
   server-side renewer on a worker claim (Design 3's dual-renewer race cannot exist).
8. **Settle after (re)election:** for 180 s the new server registers runners, serves reads, writes
   nothing but its own lease, offers nothing, and does not start the train. Expired claims are never
   reaped by the server at all — reaping stays lazy in the acquirer (`acquire_or_reap`) — and
   during settle the scheduler treats any claim a runner reported in `live_workers[]` as live
   regardless of `expires_at` (renew-first/reap-second).
9. **Clock skew:** `expires_at` is compared across hosts today already; `doctor.py` gains an
   NTP-offset check and the runner refuses to register when |offset| > 30 s.

### 3.4 Re-hosting the server (automatic; the exact procedure)
Trigger: a `server_eligible` runner (`desired.server_candidates`, Linux only; laptops are never
eligible) has lost its server for > 30 s and, polling `refs/fleet/claims/server/singleton`
directly every 30 s, sees `now > expires_at + rank×60 s` (or absent).
1. `Claim(hub=GitHubHub(state), kind="server", key="singleton", ttl=120, renew=30).acquire_or_reap()`
   with payload `advertise_urls=[tailnet-ip:8470, lan-ip:8470], boot_id, keel_version`. CAS elects
   exactly one; losers log and keep polling.
2. Winner forks `keel-server` in its own session (it is a child of the runner process, so a
   server crash never takes the runner's renewers with it — the inverse of Design 3's "leader
   inside the runner").
3. Server: `Hub.fetch_namespace` → `RefIndex` (one fetch); `git fetch` `~/.keel/code.git` mirror
   from the code repo; load the last `refs/fleet/events/*` checkpoint into the ring; read
   `refs/fleet/operator/journal`.
4. Enter **settle** (180 s, rule 8). Accept `register{live_workers[]}`; rebuild the job view by
   joining reported workers to claims by `holder_host` + `claim_ref`; runners whose heartbeat is
   missing are DOWN, their claims untouched.
5. Start sweep → scheduler → train thread (first GC: `staging/train-tmp-*` older than 2 h with no
   live train claim, CAS-deleted at the observed sha) → tip watcher → OPERATOR loop.
6. Emit `server.elected`; OPERATOR posts one line to the issue.
7. **Unreachable-leader demotion:** if no runner has registered 180 s after settle ends, the server
   releases its lease, logs `server.unreachable-demoted` to the state repo's durable heartbeat of
   its host, and exits 4; the next-ranked candidate proceeds. (Closes Judge 3's "healthy lease,
   nobody can reach it".)
Runner side: on server loss, `FallbackHub` goes direct; log chunks spool to
`~/.keel/spool/<job>/`; every 30 s re-read the server lease; when `advertise_urls`/`boot_id`
changes, reconnect, `register{live_workers}`, replay spooled `result`/`log` posts (idempotent by
`job_id` + `X-Log-Offset`).
Manual: `keel server rehost` on any host holding the secrets bundle (§8) performs 1–6 if the lease
is expired; a live lease makes it refuse (exit 3). `keel server move <host>`: current holder
releases after setting `handoff_to`; that host skips its rank backoff.
Acceptance instrument for every drill: `diff <(keel status --json --direct | jq 'del(.ts,.server)')`
before the kill vs after re-host is empty, and `events.seq` max before vs after bounds the loss.

---

## 4. Store seam

### 4.1 The contract (unchanged)
`tests/test_no_raw_hub_push.py` (`_PATTERN` L55, allowlist L71-114) already forces every
coordination write through `fleetlib.Hub`. Its eight methods (`fleetlib.py` L212-465:
`sha, read, read_with_sha, create, update, delete, push_ref, list`) and their semantics — `False`
= lost race (never error), `None` = absent, transport failure **raises**, "transport wins over
absence" (`_is_transport_failure` L142-152), coherent read (`_read` L298-395), provenance
(`_augment` L516-522) — are the interface everything else consumes.

### 4.2 `ServerHub` (HTTP)
`GET /v1/refs/{ref}` → `{sha,payload}` | 404; `PUT` + `If-None-Match: *` → 201 | 409; `PUT` +
`If-Match: <sha>` → 200 | 409; `DELETE` + `If-Match` → 204 | 409; `GET /v1/refs?prefix=` →
`{ref:{sha,observed_at}}`. 409 → `False`; 404 → `None`; 401/403/5xx/connect-error/timeout →
`HubUnreachableError` (never 404/409 on transport). `push_ref` raises `NotImplementedError`: branch
pushes never go through the server. Connect 5 s, read 20 s. The server answers every write by
executing the same `Hub.create/update/delete` against the state repo and returning the sha GitHub
produced, so a sha obtained via either route is valid on the other.

### 4.3 `FallbackHub` — two rules that make two routes safe
The two judges' fatal flaw for Design 1 is real in `claim.py` L644-690: `renew()` re-reads
`hub.sha(ref)`; if it differs from `self._sha` it reads the payload, and if `_owns` says it is ours
it **adopts that sha** (L661-663) and updates against it; a rejected update calls `_mark_lost`
unconditionally (L677-682), and a lost worker is killed by group (`fleetd.py` L1793-1832). So a
stale `sha()` on our own claim is a killed healthy gate. Therefore:
1. **Fresh claims.** `ServerHub` reads under `refs/fleet/claims/` are served live by the server
   (one `ls-remote <ref>` ≈ 0.6 s *(measured: `git ls-remote` to GitHub from m5 563 ms ± 78 ms,
   hyperfine, 5 runs, from the Design 1 measurement)* — trivial at a 120 s renew cadence). The
   index is never consulted for a claim's sha.
2. **No write retry after an ambiguous failure.** `FallbackHub` re-issues `create/update/delete`
   against GitHub only when the primary failed *before the request was sent* (connection refused,
   DNS, TLS handshake, server `503 not-ready`). A timeout after send, a dropped connection, or a
   5xx after the server may have executed the CAS **raises `HubUnreachableError` instead** — which
   is exactly today's behaviour on a blip: `_note_renew_failure` tolerates it (L694-715) and the
   next renewal's top-of-loop re-read + `_owns` adopts our own landed write (L651-663). Reads
   (`sha/read/read_with_sha/list`) always fall back.
3. Fallback is sticky for 30 s then re-probes; `degraded_since` is reported in the runner
   heartbeat; `.url`/`.workdir`/`.code_url` are those of the GitHub half so the GIT-CODE borrowers
   (§9) work unchanged.
Seam tests pinning this: `TestSeam8ServerKilledMidGate` and `TestSeam9RouteFlipNeverMarksLost`
(§11).

### 4.4 The code/state routing table (four URLs, one rule)
`Hub` carries four remotes (`fleetlib.Hub.__init__`). `url` is the PRIVATE state repo and answers
`refs/fleet/*`. `code_url` is the PUBLIC code repo and answers `refs/heads/*` **reads** — the tip's
sha, the `staging/*` listing, and the object fetches behind `merge-base`/`merge-tree`.
`code_push_url` is where ORDINARY `refs/heads/*` **writes** go — `rescued/*`, the `staging/<slug>`
retirement, `staging/train-tmp-*` — and defaults to `code_url`, which is the HTTPS remote the
host PAT authenticates. `tip_push_url` is where the ONE ruleset-bypassing write goes, the tip
advance, and defaults to `code_push_url`. Each defaults to the one before it, ending at `url`, so
every existing test, every `git init --bare` fixture and the single-repo topology are unchanged.

**Why the tip has its own URL: because it has its own credential.** The `tip-update` ruleset's
bypass actor is a *deploy key* (§8), which is an ssh credential and cannot authenticate an HTTPS
push at all; the per-host PAT is HTTPS and is not a bypass actor. With one `code_push_url` shared
by all four writes there is no setting that is correct, and both wrong settings are silent:

* pointed at `git@github.com:swack-tools/oxidex.git` so the deploy key can reach the tip, the
  three non-tip pushes run with **no pinned identity at all** — `push_code_ref` passes no
  `ssh_command`, so ssh offers whatever the ambient agent holds, i.e. the operator's personal key,
  from the daemon of a host whose entire point is that it carries a *scoped* credential;
* pointed at the HTTPS URL so the PAT authenticates those three, the deploy key becomes inert —
  `GIT_SSH_COMMAND` is not consulted for an HTTPS remote — and the tip push fails the ruleset with
  a permission error that names neither the key nor the transport.

So the tip is `hub.push_tip_ref(refspec, ssh_command=<deploy key>)` at `tip_push_url`, and the
other three are `push_code_ref`/`delete_code_ref` at `code_push_url`, **which take no
`ssh_command` parameter at all**. Not "must not pass one" — cannot: a reviewer can check two call
sites, but no reviewer can check that an argument was omitted at three call sites, one of them
inside a retry loop. `train.py` exposes the split as `--tip-push` / `FLEET_TIP_PUSH_URL` alongside
`--code-push` / `FLEET_CODE_PUSH_URL`, and warns when `--code-push` names an ssh URL while the tip
pushes elsewhere — the pre-split rollout shape, which still works and no longer passes unremarked.

The rule is mechanical: **`refs/fleet/*` → `url`; reading `refs/heads/*` → `code_url`; writing
`refs/heads/*` → `code_push_url`; writing the tip → `tip_push_url`.** The routing is expressed as
*distinct method names* (`sha`/`code_sha`, `list`/`code_list`,
`push_ref`/`push_code_ref`/`push_tip_ref`, `delete`/`delete_code_ref`) rather than a `url=`
keyword, because every failure in this class is silent: a `refs/heads/*` `ls-remote` against a
state repo does not error, it returns "absent", and the caller reads that as a fact about the
code. A defaulted keyword leaves the wrong answer one forgotten argument away at every call site;
a different method name makes the routing visible in the call and greppable.

**(a) Code READS — `code_url`.** Every site below moved off `hub.url`.

| site | file:line | call |
|---|---|---|
| queue: tip sha | `workqueue.py:129` | `hub.code_sha(TIP_REF)` |
| queue: `staging/*` listing | `workqueue.py:200` | `hub.code_list("refs/heads/staging")` |
| queue: ancestry fetch | `workqueue.py:270` | `git fetch <code_url> +tip,+staging/*` into `hub.workdir`, through `fleetlib.run_git` |
| dispatch: object fetch | `dispatch.py:461` | `git fetch <code_url> <refs>` into `hub.workdir` |
| dispatch: branch sha | `dispatch.py:577` | `hub.code_sha("refs/heads/<branch>")` |
| train: clone source | `train.py:508-509` | `git clone <code_url>` |
| train: tip sha | `train.py:511` | `hub.code_sha(TIP_REF)` |
| train: tip re-check after gating | `train.py:560` | `hub.code_sha(TIP_REF)` |
| train: staging sha after a refused retirement | `train.py:803` | `hub.code_sha(ref)` |
| train: rescue verification | `train.py:841` | `hub.code_sha(rescued_ref)` |
| agentworker: tip sha | `agentworker.py:211` | `hub.code_sha(TIP_REF)` |
| agentworker: branch sha before/after | `agentworker.py:220,227,284` | `hub.code_sha(ref)` |
| agentworker: the agent's clone | `agentworker.py:243` | `git clone <code_url>` |
| fleetd: tip sha (agent path) | `fleetd.py:1620` | `hub.code_sha(TIP_REF)` |
| fleetd: "does this intent already have a branch" | `fleetd.py:1665` | `hub.code_sha("refs/heads/staging/<slug>")` |
| fleetd: tip sha (gate path) | `fleetd.py:1962` | `hub.code_sha(TIP_REF)` |
| gate.sh: fetch + clone of the branch under test | `gate.sh:440` (`CODE_URL="${FLEET_CODE_URL:-$HUB_URL}"`) | `$CODE_URL` |

**(b) Code WRITES — `tip_push_url` for the tip, `code_push_url` for the other three.** All four
were pushing at `hub.url`, i.e. at the state repo; all four then shared one URL, which is the
defect §4.4's opening now describes.

| site | file:line | remote | call |
|---|---|---|---|
| tip advance (the one ruleset-bypass push) | `train._push_tip` | `tip_push_url` (ssh, deploy key) | `hub.push_tip_ref(f"{head}:{TIP_REF}", ssh_command=<deploy key>)` |
| `rescued/<slug>` | `train.py:839` | `code_push_url` (HTTPS, PAT) | `hub.push_code_ref` (was a raw `git push origin` from the clone — no credential helper, aimed at the read URL) |
| `staging/<slug>` retirement | `train.py:799` via `_delete_code_ref_cas` (L748) | `code_push_url` (HTTPS, PAT) | `hub.delete_code_ref(ref, expect_sha)` |
| `staging/train-tmp-*` push + CAS delete | `train.py:871`, `train.py:888` (via `_delete_code_ref_cas`) | `code_push_url` (HTTPS, PAT) | `hub.push_code_ref` / `hub.delete_code_ref` |

The deploy key is attached to the **tip push only** (§3.1: `rescued/*`, the `staging/<slug>`
retirement and `staging/train-tmp-*` go via the PAT), and `push_code_ref`/`delete_code_ref` have
no `ssh_command` parameter for it to be passed to. It is threaded as a per-subprocess
`ssh_command=` parameter, never `os.environ` — see `fleetlib.run_git` (which
`Hub._raw_run` and `workqueue.Queue._git` both delegate to) and
`fleetlib.ssh_command`. Setting it process-wide put the code repo's deploy
key, with `IdentitiesOnly=yes`, on every concurrent
claim-renewal push to the state repo; the train singleton's renewer thread pushes on a 120 s timer
throughout a 20-45 minute gate, so that overlap is the normal case. `ssh_command(identity_file=…)`
composes the identity **on top of** the pinned `BatchMode=yes -o ConnectTimeout=10 -o
StrictHostKeyChecking=accept-new`, which the hand-rolled string dropped.

`run_git` sets `GIT_SSH_COMMAND` unconditionally and no longer honours an ambient value: the
`env.get("GIT_SSH_COMMAND", <default>)` shape let any value inherited from a shell, unit file or
parent process replace all three pinned options for every fleet git operation, with failure modes
(a daemon blocked on a passphrase prompt; a two-minute connect stall inside a 30 s timeout) that
name none of them.

**One git spawner.** `fleetlib.run_git` is the only sanctioned way fleet code starts a git
process, and it carries four properties no caller supplies for itself: `credential_env()` (the
HTTPS PAT helper plus the empty-valued `credential.helper` that stops a host helper answering
first), the unconditional pinned `GIT_SSH_COMMAND`, `GIT_TERMINAL_PROMPT=0`, and a timeout that
raises `HubUnreachableError` instead of hanging. `Hub._raw_run` delegates to it, and so does
`workqueue.Queue._git` (`workqueue.py:294`) — which was a bare `subprocess.run(["git", ...])`
until R5. Two of that helper's three call sites are local, but the third
(`_fetch_for_ancestry`) fetches from `hub.code_url`, so the daemon's queue computation was running
against a real remote with no credential helper, an ambient ssh command and a live terminal
prompt. All three now route through `run_git`: "which of these talks to a remote" is exactly the
judgement that was wrong the first time.

**(c) Coordination — `url`.** Unchanged, and asserted to stay that way: `claim.py` (all of it),
`verdict.py:186-230`, `intent.py:141-409`, `dispatch.py:177-300` (attempt ledger) and
`dispatch.py:530` (verdict lookup), `fleetd.py:600-616,1065,1217,1370,1533,1850,1867`,
`train.py:208-279` (tip signal) and `train.py`'s intent close. `test_code_url_split.py` asserts
`git ls-remote <state> 'refs/heads/*'` is empty and `git ls-remote <code> 'refs/fleet/*'` is empty
after a full train run — the second matters because `refs/fleet/*` carries `user@host:pid`
provenance and the code repo is public (§8).

**`cli.py` is NOT coordination-only, and earlier drafts of this row said it was.** `cli._hub`
used to build its `Hub` from `--hub`/`FLEET_HUB_URL` alone, with no `code_url`,
while `cli.cmd_status` calls `workqueue.Queue(hub).compute()` — a CODE read. On a
split spine that asked the state repo for the tip, got `None`, and printed
`QUEUE error: tip ref … does not exist on the code repo <state repo>` on every invocation, with
no flag or variable able to fix it (R1). `_hub` now passes `Hub(code_url=--code or
FLEET_CODE_URL)` (the global `--code` flag `cli.main` declares mirrors `fleetd --code`), and
`tests/test_cli.py::TestStatusCodeUrlPlumbing` pins flag, env fallback, flag-over-env precedence
and the still-broken-if-unconfigured case against two real bare repos. The CLI's *writes* remain
coordination-only (`_edit_desired` and the claim/heartbeat reads all sit on `url`).

**Neither.** `train._fetch_into_hub_cache` (`train.py:677`) is a LOCAL fetch from the train's
working clone into `hub.workdir`; it names no remote. It is cited in earlier drafts of this section
as a code-read site, which is wrong — the real code read in `train.py` is `_run_train_locked`'s
`git clone <clone_src>`. `train._git` (`train.py:318`) is a raw `subprocess.run` with no
`credential_env` and no pinned `GIT_SSH_COMMAND`; every one of its call sites operates on the local
clone except the clone itself, which is a read of the PUBLIC code repo and needs no credential.

**Failure containment.** `workqueue.Queue.compute()` raises `QueueError` when the tip is absent,
and `QueueError` is not a `HubError`, which `fleetd`'s reconcile loop catches on purpose. Routed at
the state repo the tip is *always* absent, so the daemon died in a traceback before its first
heartbeat. `Queue.compute_or_refusal()` (`workqueue.py:159`) returns `({}, ("queue-unavailable",
detail))` instead, which `fleetd` records in `ReconcileResult.refused` (`fleetd.py:1645,1943`) and
`fleet status --why` prints. `HubUnreachableError` still propagates: a network outage is a degraded
step, not a permanent configuration verdict.

Tests: `test_queue`, `test_dispatch`, `test_train` run unchanged against a fixture whose
`code_url == url`; `tests/test_code_url_split.py` runs the same code against two real bare repos
(`state.git` with no `refs/heads/*` at all, `code.git` with a tip that has moved past a staging
branch) and pins every row above, plus three properties the table alone does not state:
`TestTipPushUrlIsSeparateFromTheOtherCodeWrites` (the tip lands on `tip_push_url` and `rescued/*`
does not; `push_code_ref`/`delete_code_ref` raise `TypeError` on `ssh_command=`),
`TestTrainDeployKeyIsScopedToOneSubprocess` (with `FLEET_TEST_RENEW_S=1` the singleton's OWN claim
renewer pushes while the tip push is held open, and that push's `env=` carries the pinned default
rather than the deploy key's `-i`), and `TestWorkqueueGitGoesThroughFleetlib` (the ancestry fetch's
`env=` is the one `run_git` builds). Every one of them reads the `env=` dict handed to
`subprocess.run`, because a spy on the `Hub` method passes whether or not the value reaches git —
the instrument that was green while this feature was broken in three separate ways.

---

## 5. API, events, runner protocol

Bind: tailnet address, port 8470, plain HTTP (WireGuard underneath; no public port; a
`KEEL_BIND` override exists for the fixture server on 127.0.0.1). Auth: `Authorization: Bearer`.
Token classes: **runner** (one per host, `~/.keel/runner.token`), **operator** (CLI/dashboard/
human), **agent** (per agent, derivable — §8). Server stores only sha256 hashes
(`~/.keel/auth.json`), compares with `hmac.compare_digest`.

### 5.1 Resources
- `GET /v1/health` → `{boot_id, lease_expires_at, index_observed_at, github_ok, settle_until, degraded}`.
- **KV-CAS façade** — §4.2. `?fresh=1` forces a live read for any ref.
- `GET /v1/status` (hosts, runners, queue, claims, train, agents, alerts, server lease, staleness); `GET /v1/queue`; `GET /v1/verdicts/{tree}[/{gv}/{platform}]` (ABORT never served — `verdict.py` L58-61, L168-183).
- `GET /v1/why` → per runner: last tick's `ReconcileResult.refused` (`disabled`, `limits: free 9.1G < 14G`, `no free slots`, `queue empty`, `agent-cooldown`, `economics: cached-pass`, `parked: NEEDS_AUTHOR at <sha>`), the durable `ReconcileResult.warnings` (§3.1), `degraded_since`, `spine_unreachable_since`, `settle_until`. `keel why` renders it. This is the literal answer to the week's question.
- `GET|PUT /v1/desired` (`If-Match`; generation++ server-side; `cli._edit_desired` retry semantics).
- Runners: `POST /v1/runners/{id}/register {capabilities, live_workers[]}` → `{boot_id, settle_until, lease_expires_at}`; `POST …/heartbeat` → 204 (+ commands); `GET …/assignments?wait=30` long-poll → `{offers[], commands[]}` | 204; `POST …/commands {cmd}` (operator → queued); `GET /v1/runners`.
- Jobs (view over claims): `GET /v1/jobs[/{id}]`; `POST /v1/jobs/{id}/logs` (`X-Log-Offset`, ≤ 16 KB, ≤ 1/s, 64 KB ring); `GET /v1/jobs/{id}/logs?follow=1` (SSE; `?full=1` relays a one-shot read of `~/gatelogs` through the runner's next long-poll — no inbound port); `POST /v1/jobs/{id}/result {rc,outcome,verdict_json?,duration_s}` (idempotent by `job_id`).
- Train: `GET /v1/train`; `POST /v1/train/{run,dry-run}`.
- Agents: `POST /v1/agents {role,key,budget,model}` → `{id,token}`; `GET /v1/agents[/{id}]`; `POST /v1/agents/{id}/transcript {seq,events[]}` (ring only); `POST /v1/agents/{id}/result`; `POST /v1/agents/{id}/tools/{name} {args}` (role allowlist enforced server-side, §6.3).
- Intents: `POST /v1/intents {slug,title,scope}` → runs `intent.register`'s three checks (`intent.py` L307-337) with the ledger check dispatched as a `probe` job; `POST /v1/intents/{slug}/withdraw`.
- Events/alerts: `GET /v1/events?since=<seq>&follow=1` (SSE with `id:` = seq, replay from `since`, `: keepalive` comment every 15 s for hotspot NATs); `GET /v1/alerts`; `POST /v1/alerts/{id}/ack`.
- Inbox: `GET /v1/inbox` (open `ask_human` questions), `POST /v1/inbox/{qid}/answer` (also consumed from issue comments).
- Server: `POST /v1/server/{step-down,move}`.

### 5.2 Event kinds
`runner.{registered,up,down,drained,degraded}`, `claim.{acquired,renewed,lost,released,reaped}`,
`job.{offered,started,stage,finished,infra}`, `verdict.{stored,cache-hit,conflict}`,
`tip.{advanced,foreign}`, `train.{started,gating,bisect,ejected,advanced,restarted,stalled}`,
`agent.{queued,started,tool,denied,finished,over_budget,blocked,killed}`,
`operator.{wake,action,report,question,budget_exhausted}`, `server.{elected,settle_end,demoted,
unreachable-demoted,step-down}`, `spine.{unreachable,reachable}`, `alert.{raised,acked}`.

### 5.3 Runner protocol (all outbound)
1. `register` on start and every reconnect: `doctor.py`'s measurements (`platform_id`, `rustc_id`
   — `claim.py` L224-262; oracle `-ver`==13.59 ∧ DOCX; corpus count 4238; disk; mem; cores; os;
   `gate_version` 7; `tools_tree_sha`; `clis:[claude,codex]` whose auth probe passed; `has_oracle`,
   `can_regen` (i7), `server_eligible`, `rank`, `ntp_offset_s`) + `live_workers[{claim_ref,
   claim_sha, pgid, tag, kind, started_at}]` from the job journal ∩ `ps`.
2. `heartbeat` every 15 s: `free_gb, free_mem_gb, load, oracle_ok, degraded_since,
   workers[{claim_ref,pgid,alive,stage,rc?}], killed_this_loop, refused[]`. DOWN after 60 s silence
   (live view); durable copy every 5 min (§3.1).
3. `offer` in the long-poll response: `{offer_id, kind:gate|agent|train-gate|probe, branch, sha,
   tag, tip_sha, gate_version, expected_platform_id, agent{id,role,model,budget}?, expires_at}`.
   Runner: write journal entry → `Claim.acquire_or_reap` (via FallbackHub) → spawn in own session
   with trailing `fleet-scope=<token>` argv (`fleetd.py` L329-339, token now derived from the
   **state repo URL**, stable across re-hosts) → post-spawn `renew` persists the real pgid
   (`fleetd.py` L754-773). `ClaimHeldError` ⇒ `declined:[offer_id]` in the next heartbeat.
4. `command` ∈ `drain | stop | restart-runner | sweep-orphans | update-tools {sha} | elect-server |
   notify {text}`, acked by id; re-delivery is a no-op.
5. `logs` chunks; `result` posts; spool + replay when the server is away.
6. Renewals are the runner's own `Claim` renewer thread → `FallbackHub.update` (§4.3).
Reconnect: long-poll 30 s, connect 5 s, backoff 1→60 s with jitter; after 3 failures poll the
server lease at the state repo every 30 s.

**Job journal** (`~/.keel/jobs/<claim_key>.json`, written **before** spawn; Design 2 graft): `{offer,
claim_ref, started_at, pgid, workdir, tag}`. `adopt_workers` gains it as a second evidence source:
with the store reachable the hub claim is truth exactly as today (`fleetd.py` L1264-1524, all
three dispositions, unreadable-disarms); with *both* routes unreachable at start the runner no
longer exits 5 (`fleetd.py` L2118-2128) — it adopts journaled groups that are alive and
identity-verified, starts their renewers (which will mark `lost` and kill per `claim.py` if the
store stays away), sweeps nothing, spawns nothing, and retries the store every 30 s.

`autonomous_when_serverless` (config, default false; enabled on the i7 only): when the server
lease is absent/expired > 60 s, the runner runs today's `reconcile_once` **gate** selection
(`fleetd.py` L1721-1873, `classify_branch` L1128-1195) directly against the state repo at
`--interval 60`, targets from `desired`. **Gates only — no agent dispatch**, so Judge 1/2's
double-`record_dispatch` cannot occur; gate claims are CAS-arbitrated and verdicts are
content-addressed, so the worst case is a lost claim race. This is Design 3's best resilience idea
with its one hazard removed: the fleet is never less capable than today's hubless Stage 1.

---

## 6. Agents

### 6.1 Entity and lifecycle
`refs/fleet/agents/<id>` (§3.1) + a derivable token + a transcript on the runner's disk.
`queued` (created by scheduler/OPERATOR/CLI; `dispatch.record_dispatch` **before** the offer,
`dispatch.py` L29-43) → offered to a runner advertising the CLI → runner claims
`claims/agent/<key>` (`attempt_key`, `dispatch.py` L135-145) and spawns `agentrun.py` in its own
session → `running` → terminal `done` (verified by the ref moving/appearing on the **code repo**,
never by the agent's word — `agentworker.run`), `blocked` (rc 9; Stage 1d F1: the BLOCKED verdict is read BEFORE any delivery and pushes nothing, whatever the agent committed locally), `no-progress` (7), `push-auth-failed` (rc 10, T5: the work is committed locally and the worker's own delivery push through `Hub.push_code_ref` failed to authenticate — a host credential condition, not a branch verdict),
`not-paid` (8, refund), `timeout` (6, killed by group), `over_budget`, `killed` (lost lease),
`infra` (rc 4/5). rc → outcome map (`fleetd._AGENT_RC_OUTCOMES`) unchanged but for rc 10; adopted-across-restart
agents record `unknown-adopted` as today but the log ring + journal make the case diagnosable.

### 6.2 Invocation
- Claude: `claude -p "<prompt>" --model <tier> --output-format stream-json --verbose --max-turns N
  --mcp-config ~/.keel/agents/<id>/mcp.json --dangerously-skip-permissions` (permissions are
  already skipped today, `agentworker.py` L7-11) in a **sandbox clone** `~/.keel/agents/<id>/r`.
  `cwd/.claude/settings.json` carries `PreToolUse` hooks (the mechanical doctrine guard, Design 2
  graft): deny `Bash` matching `\bexiftool\b` unless the path is the pinned wrapper; deny
  `Edit|Write` on `src/exiftool_tables/binary_tables.rs`; deny `git push` whose remote is not
  `origin` (the sandbox) or that names more than one refspec; deny `git push --force` without
  `--force-with-lease`. Hook denials are posted as `agent.denied` events.
- Codex: `codex exec --json --yolo -m <model> "<prompt>"` with `~/.codex/config.toml` gaining
  `[mcp_servers.keel]` → `python3 tools/fleet/keel/mcp.py --agent <id>`. No settings hooks exist
  for codex; the sandbox remote is its guard.
- **Sandbox remote** (Design 3 graft): the clone's `origin` is a local bare repo
  `~/.keel/agents/<id>/origin.git` whose `pre-receive` accepts exactly one refspec (the assigned
  `staging/<slug>`), rejects everything else, and logs attempts. After the run `agentrun.py`
  pushes that one ref to GitHub with the runner's PAT. "Exactly one branch, never tip/main" is now
  structural for both CLIs; the ruleset (§8) is the second layer.
- CLI choice: random among installed+authenticated, auth-failure fallback once
  (`agentworker.py` L80-108, L236-254); `FLEET_AGENT_CLI_OVERRIDE` keeps the test stub.
- Prompts: `build_prompt`/`build_authoring_prompt` verbatim (`agentworker.py` L111-163) with the
  push instructions reduced to "push to origin".

### 6.3 Roles, tiers, budgets, tools
| role | tier | budget (usd / wall / turns) | trigger | tools (via `keel-mcp`, allowlisted server-side) | result schema |
|---|---|---|---|---|---|
| gate-runner | process, not LLM | gate's own (ABORT at 3 h) | scheduler | `gate.sh` | verdict JSON |
| converger | Sonnet-class; Opus-class on the 3rd consecutive attempt | 5 / 3600 s / 60 | stale branch / NEEDS_AUTHOR via dispatch economics | common | `{outcome: converged\|blocked, sha, conflicts[]}` |
| author | Opus-class | 15 / 7200 s / 150 | open intent, reserved authoring slot (`dispatch.order_candidates` L376-426) | common + `register_intent, withdraw_intent, request_regen, measure_baseline, find_table` | `{outcome: authored\|blocked, branch, sha, missing_before, missing_after, instrument}` — the server **re-runs** `measure_baseline` as a `probe` job on a `has_oracle` runner; the agent's numbers are not the instrument |
| reviewer | Sonnet-class | 2 / 900 s / 30 | every FAIL verdict and every `blocked` before re-dispatch | `read_diff, read_gate_log, read_verdicts, classify_failure` | `{class: defect\|drift\|flake\|infra, evidence[]}` feeds dispatch: drift ⇒ converger; infra ⇒ ABORT-like retry once; flake ⇒ re-gate once; defect ⇒ NEEDS_AUTHOR + alert |
| OPERATOR | Sonnet-class tick; Opus-class escalation | 2/wake, 15/day; 900 s; 30 turns; ≤ 8 wakes/h, 10 min cooldown | §6.5 | operator list | `{diagnosis, evidence[{claim,instrument}], actions_taken[], actions_proposed[], needs_human}` |

Common tools: `get_task, tip, read_verdict, read_gate_log, report_progress, report_result,
ask_operator`. Operator tools: `fleet_status, why, list_jobs, read_log, list_alerts, ack_alert,
list_agents, read_transcript_tail, set_desired` (within `max_gates/max_agents` caps), `drain,
restart_runner, sweep_orphans, rearm` (`dispatch.clear`), `retry_gate` (drops a local
NEEDS_AUTHOR memo, never a verdict), `withdraw_intent, train_dry_run, train_run, spawn_reviewer,
cancel_agent, server_move, escalate, post_report, ask_human`. **Absent for every role:** writing a
verdict, deleting a ref, pushing a branch, touching the tip, editing `gate.sh`/`GATE_VERSION`,
changing `limits`, reading secrets. The tool list is the guardrail; the prompt is advice.

Budgets are enforced where the cost is paid: wall clock by the runner (`FLEET_AGENT_TIMEOUT_S`
default 3600, `agentworker.py` L67); turns by `--max-turns`; USD/tokens parsed from stream-json
`result.usage/total_cost_usd` (codex `--json` usage events) streamed to the runner, which kills
the group on breach; attempts by `dispatch.py` unchanged (cap 3, cooldown 1800 s, L89-94); fleet
by `limits.max_agent_usd_per_day` summed over agent refs.

### 6.4 Transcripts
Streamed to `POST /agents/{id}/transcript` for a 256 KB server ring (`keel agents tail`), kept
whole on the runner at `~/.keel/agents/<id>/transcript.jsonl`, `sha256` + host recorded on the
agent ref. **Never pushed to any repo** (a permissions-skipped session's transcript can contain
`env`, paths, and file contents; Judges 1–3).

### 6.5 OPERATOR loop
Server-side, leader-only. Wakes on: `runner.down` > 3 min, `claim.lost`, `verdict.conflict`,
`train.restarted`×3/h, `train.stalled` (AWAITING_TRAIN ≥ 1 and no train claim for 30 min),
`tip.foreign`, queue non-empty with zero dispatch for 30 min on an enabled runner,
`spine.unreachable` > 5 min, `agent.over_budget`, `alert.raised`; plus a 15-min tick.
Sonnet-class tick reads `fleet_status`, `why`, recent events → `nothing | act | escalate`;
`escalate` re-runs Opus-class with full log tails. Every action is an event
`{actor:"agent:operator:<id>", tool, args, result}`; two consecutive wakes taking the same action
on the same target make the third refuse and `ask_human`. Reports (≤ 20 lines, every number naming
its instrument) go to the issue on the private state repo, `keel report`, the dashboard panel,
and optionally `KEEL_NOTIFY_URL`. First weeks run `desired.operator.report_only=true`
(`set_desired/train_run/restart_runner` disabled) until ≥ 10 incidents are judged accurate. Daily
cap reached ⇒ only `post_report` remains callable. Journal pointer on the spine lets a re-hosted
server resume without re-reporting.

---

## 7. Failure modes

| scenario | what happens | guaranteed | lost | recovery |
|---|---|---|---|---|
| **Server host dies** (the ryzen) | runners' long-polls fail; after 3 failures `FallbackHub` renews leases and stores verdicts directly on the state repo (same CAS, same refs); logs spool; eligible runners poll the server lease and elect (§3.4) | no running gate/agent is killed; no lease lapses while GitHub is reachable; verdicts land; tip cannot move (deploy key is only used by the train thread under a live train claim) | ≤ 5 min of events, in-memory log rings, new assignments for ≈ TTL 120 s + rank×60 s + settle 180 s ≈ 4–6 min | automatic; i7 `autonomous_when_serverless` starts gates meanwhile |
| **Runner process dies mid-gate** (gate alive in its group) | renewer dies; supervisor restarts runner; `adopt_workers` re-adopts by marker+scope token+pgid, `Claim.adopt` continues the lease (`claim.py` L497-623; seam 4) | no double gate; unreadable claims disarm the sweep | nothing | automatic |
| **Runner host dies** | claims expire at TTL (600 s); next acquirer reaps (`acquire_or_reap`); branch re-queued; no verdict was written so the tree is re-gated; a later duplicate is `cache-hit` or `conflict` (`verdict.py` L186-230) | ≤ 1 verdict per `(tree,gv,platform)` | ≤ 1 gate of CPU | DOWN at 60 s live / 5 min durable; OPERATOR reports |
| **Runner ↔ server partition** (GitHub fine) | fallback direct; finishes; no new offers unless autonomous; server sees heartbeat silence but its sweep still sees the claims renewing, so nothing is reaped and the job rows keep updating | no kill, no duplicate | new work on that runner | reconnect → `register{live_workers}` → spool replay |
| **Runner ↔ GitHub partition** (server fine) | renewals brokered; `gate.sh` clone/fetch fails → exit 9 (`gate.sh` L383-391; `FAIL clone` never stored since `TREE_SHA` is empty) → runner maps to outcome `infra`, branch returns to queue, **not** NEEDS_AUTHOR; agents rc 5 → `not-paid` refund | no cache pollution, no park | that runner's work | alert names the host |
| **Server ↔ GitHub partition** | server 503s writes, serves reads flagged `stale`, stops offering; train's claim renew fails → `lost` → no push, `restarted` later; server lease renew fails → degraded, re-acquire every 30 s; runners' `ServerHub` 503 → direct | tip safe; no false kills (runners renew direct) | scheduling until another eligible host wins | automatic |
| **Laptop sleeps / hops hotspot** (m5, m4) | never server-eligible; renewer stops; on wake a renew within TTL succeeds, else `lost` → kill by group (duplicate-safe); long-poll reconnects across IP changes; no `/tmp` state | human's session holds no watcher | ≤ 1 gate on a long sleep | automatic; `desired` default for laptops is 0/0 |
| **GitHub down (both repos)** | no CAS anywhere: no claims, no attempts, no tip push — safe freeze; renewals fail on both routes → every worker lease declared `lost` one interval before `expires_at` (`claim.py` L694-715) → runners kill gates/agents | no duplicate landing, no divergence | CPU (≈ TTL−renew = 8 min of grace, then kills) | **accepted risk** (§12 #witness): the pessimistic rule is the one seam 6 proves; gates are cheap to repeat, duplicate author runs are not |
| **Two servers** | impossible to progress twice: server lease is a CAS'd claim; `rehost` against a live lease exits 3; even a rogue second server only *offers*, and runners commit only via claims | one holder | duplicated offers lose the claim race | — |
| **Stale ryzen hub copy returns** | import only with a non-forced `git push state 'refs/fleet/*:refs/fleet/*'` (creates where absent; conflicts refused); verdict `conflict`s reported, never merged; its old `fleetd` exits 2 (shim) | no overwrite | — | human, once |
| **Wrong instrument / stale binary** | unchanged defences: `=== instrument ===` headers, `staleness_note`, `resolve_binary` fail-loud; `tools_tree_sha` in registration; `update-tools` command | — | — | — |

---

## 8. Security

- **Two repos, two exposures.** Code repo is PUBLIC and stays so (rulesets are available there
  *(measured: one active ruleset `main`)*). Coordination state (`refs/fleet/*` — claims carry
  `user@host:pid`, paths, hostnames) lives on the PRIVATE state repo. Transcripts live on runner
  disk only. Event checkpoints, agent summaries, operator journal and the operator's GitHub issue
  are on the private repo.
- **Tip protection = two rulesets on the code repo** (Judge 1 #4: a bypass actor bypasses the
  *whole* ruleset it is attached to): `tip-update` = restrict updates on
  `refs/heads/refactor/tag-machinery`, bypass actor = **the repo's write-capable deploy keys as a
  class** (the measurement below; in practice `keel-train`, which is a standing invariant rather
  than a setup step);
  `tip-guard` = block deletion + block force-push on the same ref, **no bypass actors**. Plus
  `rescued-guard` = block deletion + block force-push on `refs/heads/rescued/*`, no bypass
  (`rescued/*` "never auto-deleted" becomes enforced, not convention — `train.py:799-812`). `main`
  keeps its existing ruleset. An identical pair on `refs/heads/keel-proof/*` is the live test
  target so the real tip is never exercised.

  Acceptance depends on **which half is deployed**, and conflating the two produced a wrong
  criterion that survived into the PLAN. With BOTH halves active: keyless FF → non-zero with
  `GH013`; deploy-key FF → 0; deploy-key `--force` → non-zero; deploy-key delete → non-zero.
  With the **guard half alone** — the state on the repo today — a keyless FF **succeeds (rc 0)**,
  because `block deletion` + `block force-push` do exactly that and nothing else: restricting
  *who may update a ref at all* is the `*-update` ruleset's job, and it is skipped by default
  until the deploy key exists. Guard-only acceptance is therefore: keyless FF → 0; keyless
  `--force` → non-zero; keyless delete → non-zero. A `GH013` expectation run against a
  guard-only repo fails for the right reason and reads as a broken ruleset.
  *(Measured 2026-08-21, `gh api -X POST repos/swack-tools/oxidex/rulesets` on a throwaway
  disabled ruleset: **a `DeployKey` bypass actor is not a particular key.** GitHub accepted the
  deliberately-nonexistent `actor_id: 999999999` without a 422 and read it back as
  `actor_id: null` — the bypass grants EVERY write-capable deploy key on the repo, with no
  per-key granularity to request and no error when the id is wrong. So "bypass actor = deploy
  key `keel-train`" is only true while `keel-train` is the **only** write deploy key on
  `swack-tools/oxidex`; that is a standing precondition, not a one-time setup step, and
  `tools/fleet/rollout/rulesets.py::_resolve_bypass_actors` refuses to create either
  restrict-updates ruleset unless it holds. The guard half is unaffected — it has no bypass
  actors at all.

  Two consequences are invariants, not notes. (1) Adding ANY second write-capable deploy key to
  `swack-tools/oxidex` silently grants it the tip — no per-key scoping to ask for, no error to
  observe — so "keel-train is the only write deploy key" must be re-checked, not established
  once. (2) The bypass belongs to the KEY, not to the train: any process holding that key can
  move the tip. Hence 0600 on server-eligible hosts only, and hence `fleetlib` passing it as a
  per-subprocess `ssh_command` (§4.4) rather than exporting it into a process whose other
  threads push elsewhere.)*
  Guard half landed 2026-08-21 (`tip-guard` 21158427, `rescued-guard` 21158428, `proof-guard`
  21158429); the `*-update` half waits on the deploy key and is skipped by default.
- **Credentials.** Per-host fine-grained PATs, never the owner's `gh` login on every host (Judge
  1/2 on Design 3): every runner gets `state: contents write` and `code: contents read`;
  agent-capable runners additionally `code: contents write` (staging pushes); the train deploy key
  (`~/.keel/train_deploy_key`, 0600, named by `FLEET_TRAIN_DEPLOY_KEY`) exists only on
  server-eligible hosts. `fleetlib.run_git` appends
  `credential.helper=` (empty, to reset any host helper) and
  `credential.helper=!tools/fleet/keel/git-credential-file` at `GIT_CONFIG_COUNT`, reading the
  token from the file named by `FLEET_GIT_TOKEN_FILE` (HTTPS; the 1Password ssh agent is out of
  the path entirely).

  **The PAT path has a default, and it is `~/.keel/secrets/git-token`** — spelled once, as
  `config.DEFAULT_GIT_TOKEN_FILE_REL` (`tools/fleet/config.py`, the same module that owns the
  `EXIFTOOL_CACHE_DIR` default; `units/fleet-env.sh` is its shell mirror for `gate.sh`).
  `fleetlib.git_token_file()` returns `FLEET_GIT_TOKEN_FILE` when set and otherwise that path when
  the file exists and is readable (`doctor.py`'s `check_git_token_file` asks the same resolver, so
  the health check and the daemon cannot disagree), and `credential_env` exports the resolved value so the helper
  subprocess (a separate process, which reads the path from its own environment) sees it. This is
  where `rollout/install_secrets.sh` writes it (0600) and what all three unit templates
  (`units/fleetd.service:36`, `com.oxidex.fleetd.plist:33-34`, `cron-backstop.txt:62`) set the
  variable to. Before the default existed the token worked under the daemon and was INERT
  everywhere else: `install_secrets.sh` itself, `seed_desired.py`, `fleet up`,
  `fleet status --why`, a hand-started `fleetd` and a hand-run `train.py` all run from an operator
  shell that exported nothing, so each talked to a private GitHub repo with no credential and
  failed with an authentication error naming the remote and never the missing export. Earlier
  drafts of this section named `~/.keel/github.token`, which no tool has ever read. An explicitly
  set variable still wins and still fails LOUD when it names a missing or unreadable path — naming
  a path is a statement that it is there — while an absent default file leaves the environment
  untouched, which is what keeps every ssh-spine caller and `git init --bare` fixture unchanged.
  A default file with group/world bits is used but warned about once per process; refusing it
  would leave the host with no credential at all, which is the worse failure.

  The train pushes the tip with a `GIT_SSH_COMMAND` built by
  `fleetlib.ssh_command(identity_file=~/.keel/train_deploy_key)`: `-o IdentitiesOnly=yes -o
  IdentityAgent=none` (the documented agent bypass) **composed on top of** the pinned `-o
  BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new` every fleet git command
  runs under — the hand-rolled string it replaced dropped all three, so the one push that most
  needs to fail fast and never prompt was the only one that could hang on a passphrase prompt.
  It is passed as a per-subprocess `env=` override for that single `git push`, never written to
  `os.environ`: the train singleton's claim renewer runs on another thread of the same process
  and pushes `refs/fleet/*` to the STATE repo throughout the 20–45 minute gate, and a
  process-global override hands that thread the CODE repo's deploy key with
  `IdentitiesOnly=yes`. A deploy key is ssh-only, so the tip push also requires an **ssh
  `tip_push_url`** (§4.4) — `train --tip-push` / `FLEET_TIP_PUSH_URL`; the override is inert
  against an HTTPS remote. It is `tip_push_url` and not `code_push_url` because the other three
  code writes (`rescued/*`, the `staging/<slug>` retirement, `staging/train-tmp-*`) authenticate
  with the PAT over HTTPS: pointing one shared URL at ssh ran them with no pinned identity at all,
  offering whatever key the ambient agent held from the daemon of the one host carrying a scoped
  credential.
- **Forged verdicts.** Every runner PAT can write `refs/fleet/verdicts/*` — the same trust model as
  today's ssh hub, now on a private repo. Mitigation: the train accepts a PASS only if its
  `written_by` host is a registered runner at that `platform_id`, and `verdict.stored` events with
  an unknown `written_by` raise an alert.
- **Agent tokens are derivable, not minted in memory** (Judge 1/2): `token = HMAC(secret,
  agent_id || token_issued_at)` with `secret` from the secrets bundle, so a re-hosted server
  verifies a running author's MCP calls without state; validity = agent ref status non-terminal
  and `now < token_expires_at`. Revocation = terminal status (CAS).
- **Secrets bundle** (server-eligible hosts; 0600 in a 0700 `~/.keel/secrets`; distributed once,
  rotated by hand; checksum list in the runbook): `~/.keel/secrets/git-token` (state+code PAT —
  the path `install_secrets.sh` writes, every unit template names, and `fleetlib.git_token_file()`
  defaults to), `~/.keel/train_deploy_key`, `~/.keel/auth.json` (token hashes),
  `~/.keel/hmac.key`, optional `ANTHROPIC_API_KEY` only if the host's `claude` login is not used.
  Runners: `~/.keel/runner.token`, `~/.keel/secrets/git-token` (scoped as above). Nothing in refs
  or argv (`test_no_secrets_in_refs.py`, PLAN Stage 5).
- **Transport.** Tailnet only; bearer over plain HTTP is acceptable on WireGuard and nowhere else;
  the server refuses to bind a non-tailnet, non-loopback address unless `KEEL_ALLOW_PUBLIC_BIND=1`.
- **Agent blast radius.** Sandbox clone; one-ref remote; settings hooks; scoped MCP token; the only
  host credential reachable is the runner's PAT, bounded by the rulesets. Operator cannot weaken a
  gate because no tool exists for it.

---

## 9. Reuse map (every existing file)

| file | disposition | notes / reason |
|---|---|---|
| `fleetlib.py` (1239) | **keep as-is** + `code_url`/`code_push_url`/`tip_push_url` attrs (each defaulting to the one before it), `push_tip_ref` (the one deploy-key push) beside `push_code_ref`/`delete_code_ref` (PAT over HTTPS), credential-helper env (`credential_env`) in the module-level `run_git` (`Hub._raw_run` and `workqueue.Queue._git` delegate to it) reading `git_token_file()` (`FLEET_GIT_TOKEN_FILE`, else `config.DEFAULT_GIT_TOKEN_FILE_REL` under `$HOME` when present), `fetch_namespace(prefix)` helper, `_TRANSPORT_HINTS` += `rate limit`, `secondary rate`, `abuse detection` so a 429 raises instead of reading as a lost race | it *is* the spine client; CAS proven at the git level (`test_fleetlib.TestCreate` raw-git proofs) |
| `claim.py` (898) | **keep as-is**; `kind="server"` is just a string | renewer-owned-by-acquire, ownership token, `lost` semantics, `adopt`, reap — the core; seam-tested |
| `verdict.py` (286) | **keep**; CLI gains `--server-url` (builds `FallbackHub`) | only thing `gate.sh` knows about the cache (L238-260) |
| `workqueue.py` (254) | **keep**; `_fetch_for_ancestry` uses `hub.code_url` (1 line) | formula FREE; never a local queue file |
| `dispatch.py` (594) | **keep**; `_have_objects` fetches from `hub.code_url` (1 line); called server-side (budget/economics/order/record_dispatch) and runner-side (preflight, outcomes) | durable ledger, count-before-spawn, refund, cap/cooldown, alternation slot |
| `intent.py` (466) | **keep**; `check_history(repo_root=<server code mirror>)` (already takes a path, L209); `check_capability_ledger` runs as a `probe` job | three checks, priority, fail-closed on ledger-unusable |
| `ledger.py` (684) | **keep as-is**; paths from `EXIFTOOL_CACHE_DIR` (already env, L80) | runner capability (`has_oracle`), author's `measure_baseline`, server's re-measure probe |
| `doctor.py` (331) | **keep** + NTP-offset check + `--json` | its checks *are* the registration payload |
| `drift.py` (219) | **delete**; `bump_tip_signal`'s CAS loop (L133-186) becomes ~20 lines in the server tip watcher via `Hub.update`; `test_tip_watcher.py` pins monotonicity under N processes | it is a hook; GitHub has none |
| `train.py` (711) | **re-home into the server thread**: keep `load_domains, assemble_batch, merge_members, _gate_and_bisect, _Memo, _eject_union, _push_tip_and_retire, _retire_staging_ref, _mark_intent_done, run_train`; **rewrite** `real_gate` (L645-673, reads `~/gatelogs` on the same host) → `remote_gate` (cache-first at `(tree,7,train_platform)`, push `staging/train-tmp-<tag>` to the code repo, create a `train-gate` offer pinned to `train_platforms`, wait on `verdict.stored` for the exact tree, 90-min ABORT, CAS-delete the temp ref); **delete** `tip_push_options`/push-option retry (L110-136, L522-549 — GitHub has no push options) and the cron entry; `_push_tip` uses the deploy key | batching/bisect/exact-set push are the crown jewels; only the gate adapter and the remote change |
| `fleetd.py` (2228) | **split**: → `keel/runner.py` verbatim: `Worker, live_pgids, fleet_worker_pgids, session_of, _ps_env, kill_process_group, kill_worker, start_gate, start_agent, adopt_workers` (+ journal evidence), `reap_dead_same_host_singleton, fleetd_marker_in_group, free_disk_gb, free_mem_gb, _limits_ok, _oracle_ok, host_identity`, reconcile ORDER (local reap + lost-lease kill before any remote call, L1721-1752), singleton, bounded-failure main loop; → `keel/scheduler.py`: `classify_branch` (L1128-1195, incl. "a FAIL must be about the current sha"), `_TreeResolver` (memo), `dispatch_agents` (L1545-1675), `_AGENT_RC_OUTCOMES`, heartbeat aggregation, `ReconcileResult.refused` + `.warnings` (`HostWarnings`, §3.1) → `/v1/why`; `reconcile_once` gate-selection kept in the runner **only** behind `autonomous_when_serverless`; **delete** `_VerdictIndex`/one-fetch budget (moot), push-retry ladders (L115-121), `/tmp` oracle paths (`fleetd._exiftool_cache_dir` → `EXIFTOOL_CACHE_DIR`); `fleet_scope_token` keyed on the state-repo URL; `fleetd.py` itself becomes a shim that exits 2 "use keel-runner" at the last stage | process ownership is a runner concern; selection is a server concern; the split is the boundary `reconcile_once` already draws |
| `agentworker.py` (313) | **rewrite as `keel/agentrun.py`** keeping `build_prompt`, `build_authoring_prompt`, `preflight`, verify-by-ref, rc map, auth fallback; adding sandbox remote, settings hooks, MCP config, stream-json capture, budget kill, structured result | the blind spawn is what "agents first-class" replaces; the doctrine text is the asset |
| `cli.py` | **keep** `_hub` (builds the `Hub` with `code_url` from `--code`/`FLEET_CODE_URL`, R1), the `cmd_status` renderings (incl. `--why`'s `refused:`/`warning:` lines) and `_edit_desired`; transport → FallbackHub; `--direct` = today's ref path; new `why, jobs, logs, events, agents, train, alerts, report, inbox, answer, server {status,rehost,move}` | |
| `gate.sh` (641) | **keep**, GATE_VERSION 7: L243 and L373 hard-coded `HUB_URL` defaults → `FLEET_CODE_URL` (required, fail loud); `verdict.py … --server-url "$KEEL_SERVER_URL"`; `EXIFTOOL`/DOCX paths (L223, L441) from `EXIFTOOL_CACHE_DIR` with the `/tmp` default kept on Linux and `~/oxidex-cache` on macOS; nothing in stage order, `classify_failure` (L321-352), `write_json` (L254-290), flake handling or exit codes changes — `test_gate_script.TestGateVersionMatchesFile` enforces | proven gate executable |
| `gate_version.txt` | keep | |
| `hooks/{pre-receive,update,post-receive}` | **delete**; policies → rulesets (§8) + tip watcher | GitHub runs no hooks |
| `rollout/install_hook.sh` | **delete** | |
| `rollout/seed_desired.py` | **keep** + `server_candidates`, `train_platforms`, `max_agent_usd_per_day`, `operator`; host facts (L36-55) → `docs/KEEL-HOSTS.md` | |
| `units/*` (5 files) | **keep**, re-pointed at `keel/runner.py`; env `FLEET_STATE_URL`, `FLEET_CODE_URL`, `KEEL_SERVER_URL` from `~/.keel/runner.toml`; logs `~/.keel/log/` | wrapper semantics are right |
| `fleet/domains.toml` | keep (fix the stale `verdict.is_admissible` header comment) | |
| `docs/FLEET.md`, `docs/ROLLOUT.md` | rewrite as `docs/KEEL.md` + `docs/KEEL-RUNBOOK.md`; §7 addenda/incident table carried verbatim | |
| tests | see §11 | |
| new | `keel/{server.py, cachedhub.py, serverhub.py, fallbackhub.py, scheduler.py, runner.py, election.py, agentrun.py, mcp.py, operator_prompt.md, dashboard.html, git-credential-file, sandbox_hook.sh, settings_hooks.json}` | |

---

## 10. Invariants carried over (keep verbatim) and the tests that pin them

| # | invariant | source | pinned by (today) | pinned by (Keel) |
|---|---|---|---|---|
| I1 | CAS semantics: create-if-absent; version-guarded update/delete; `False` = lost race; transport raises; absent ≠ unreachable; fail closed on unknown | `fleetlib.py` L30-43, L142-152, L397-436, L554-560 | `test_fleetlib.TestCreate/Update/Delete/Concurrent/Unreachable/FetchFailureClassification` | same classes parametrized over `GitHubHub` (local bare + opt-in live scratch repo) and `ServerHub` (fixture server); `test_serverhub.py` adds 409↔False, 503↔raises, stale `If-Match`→409, ProcessPool one-winner through the server |
| I2 | Coherent read (payload ↔ sha from one commit) | `fleetlib._read` L298-395 | `TestReadIsCoherentUnderConcurrentWrites` | same + `?fresh=1` variant |
| I3 | Lease: TTL 600/renew 120/clamp; renewer owned by acquire; no hold without renewing; ownership token `(holder_host, started_at)`; `lost` sticky with reason; declared one interval before expiry; adopt continues, never recreates | `claim.py` L120-140, L344-346, L395-442, L446-471, L497-623, L625-715, L780-817 | `test_claim`, `test_lease_protocol` (`TestRenewerLifecycle`, `TestLostLeaseDetection`, `TestClaimOutlivesItsTTL`…), `test_adoption.TestClaimAdopt` | unchanged files, fixture-switched; **new** `TestSeam9RouteFlipNeverMarksLost` |
| I4 | Lost lease ⇒ kill by group (the only kill); everything else drains; host-lease-lost ⇒ exit without killing | `fleetd.py` L1793-1832, L1877-1882, L2176-2196 | `test_lease_protocol.TestFleetdStopsWorkOnLostLease`, `test_fleetd.TestConvergence`, `TestLostLeaseIsKilledWhileTheHubIsUnreachable` | same, against `keel/runner.py` |
| I5 | Reconcile order: local reap + lost-lease kill before any remote call; per-read degradation; bounded consecutive failure then exit | `fleetd.py` L1721-1752, L2139-2217 | `test_fleetd.TestMainLoopSurvivesHubErrors`, `TestLostLeaseIsKilled…` | same |
| I6 | Orphan sweep: marker proves shape, token proves ownership, kin by session (`getsid`), unreadable claim disarms, own pgid never, pgid ≤ 1 never | `fleetd.py` L329-374, L637-683, L1264-1524 | `test_adoption.TestAdoptWorkers` (16), `TestRestartAdoption`, `test_lease_protocol.TestFixtureDaemonCannotSweepUnscopedWorkers` | same + journal-evidence cases (journal present/absent/unreadable ⇒ disarmed) |
| I7 | Verdict key `(tree, gate_version, platform_id)`; PASS/FAIL/ABORT; ABORT never served; `conflict` refused never overwritten; store best-effort; lookup 0/1/2 | `verdict.py` L58-61, L148-230, L238-260 | `test_verdict.TestLookupStore` | same over both hubs |
| I8 | Gate contract: stage order 1–10; `classify_failure`; fleet-tests isolation round + flakes; exit codes 0/1/7/9; GATE_VERSION file pin | `gate.sh` L144, L175-215, L321-352, L355-641 | `test_gate_script` (real gate.sh with stubbed cargo/just), `TestFleetTestsFlakeRetry`, `TestGateVersionMatchesFile` | unchanged |
| I9 | Queue = staging − merged − claimed − withdrawn, recomputed, never stored; claim match by slug/`refs/heads/`/`staging/` | `workqueue.py` L117-148, L171-199 | `test_queue` (13), `test_queue_truth.TestClaimExclusionRoundTrip` | same, `code_url` fixture |
| I10 | Dispatch: durable ledger; count before spawn; `not-paid` refund; progress resets; cap 3 / cooldown 1800; alternation slot; four refusals; fail-open on unanswerable; `cached_pass` any platform | `dispatch.py` L29-43, L89-94, L230-348, L356-426, L501-594 | `test_dispatch` (all classes) | same; `TestFleetdDispatch` equivalents through the server |
| I11 | Verdict-aware selection; "a FAIL must prove it is about the current sha"; other platform/version never park; unreachable fails open | `fleetd.py` L837-907, L1128-1195 | `test_queue_truth.TestVerdictAwareSelection`, `TestNeedsAuthorIsNotAPermanentLockout` | same against `keel/scheduler.py` |
| I12 | Train: solo domains; disjoint batch ≤ 8; eject never resolve; ABORT retry once; bisect memo; reassembled union re-gated; **push only an exactly-gated set**; restart on tip move; verified rescue before CAS retire; intent → done | `train.py` L75, L144-218, L243-284, L386-497, L568-638 | `test_train.TestBatching/UnionRegate/CasRetire/Singleton`, seam 3 | same with `gate_fn=remote_gate` stub; seam 3 re-created |
| I13 | Intent: three checks, priority, history suppressed by measured ledger, ledger-unusable ⇒ HIT; "detected is not parsed" | `intent.py` L33-41, L245-337; `ledger.py` L17-33, L175-238 | `test_intent`, `test_ledger` | same (ledger as probe job) |
| I14 | Tip protection: secret-gated writes; no delete; no non-FF; other refs unaffected; monotonic tip generation | `hooks/*`, `drift.py` | `test_update_hook.TestDenyMatrix`, `test_drift_hook` | **replaced** by `tests/live/test_tip_ruleset.py` (opt-in `FLEET_LIVE_GITHUB=1`, `keel-proof/*`) + `test_tip_watcher.py` |
| I15 | Instrument truth: `rustc_id` vs `platform_id` under the gate's PATH; oracle probe `-ver`∧DOCX; `ps -eo` not `pgrep`; success verified by the store not the agent; `=== instrument ===` headers | `claim.py` L224-262, `doctor.py`, `fleetd.py` L246-468, `agentworker.py` L259-271 | `test_claim.TestToolchainIdentity`, `test_verdict.TestComputeIds`, `test_dispatch.TestAgentworkerPreflight` | same + `test_agents.py` (result by ref, not by word) |
| I16 | Raw-push fence: no `push`/`update-ref` outside `fleetlib.py` | `tests/test_no_raw_hub_push.py` | itself | extended: forbid `urllib`/`http.client` writes outside `serverhub.py`; allow `agentrun.py`'s single sandbox-ref push |
| I17 | Supervisor: rc 0 = deliberate stop; pidfile + `kill -0`; TERM forwarded to child only; never touches gates | `units/fleetd-wrapper.sh` L53-113 | seam 4 (`SupervisedFleetd`) | same |

---

## 11. Seam tests that must exist before each stage is called done
(`tests/test_seams.py` drivers reused: `StubGate` L494, `SubprocessFleetd` L609,
`SupervisedFleetd` L696, `InProcessFleetd` L807, `SeamFixture`/`FleetdSeamFixture` L910/L1115;
CI mode at the production 5:1 TTL:renew ratio and `FLEET_SEAMS_SLOW=1`.)

- Seam 1 lease-through-work, Seam 2 exclusion round-trip, Seam 4 restart adoption under the real
  wrapper, Seam 6 negative control, Seam 7 read race under renewal — re-created as
  `test_seams_keel.py` against fixture server + `keel/runner.py` + `StubGate`, for both
  `FLEET_TEST_HUB=bare` and `=server`.
- Seam 3 train e2e (poison ejected, survivors land, tip advances exactly once, union-failed never
  pushed, intent flips done) — against fixture server + stub runner gates via `remote_gate`.
- Seam 5 (hook path) — deleted; replaced by the live ruleset test.
- **Seam 8 — server killed mid-gate:** SIGKILL the fixture server while a `StubGate` holds a
  lease for 3×TTL; `ClaimWatcher` (test_adoption L152) samples the ref: sha changes every renew,
  never absent; verdict lands on the fixture store; spawn count = 1.
- **Seam 9 — route flip never marks lost:** renew via server → server killed → renew direct →
  server restarted with a deliberately stale index → next renew via server must succeed and
  `claim.lost` stay False; negative control: serve the claim sha from the index ⇒ goes red.
- **Seam 10 — re-host mid-gate:** elect a second fixture server on another port during a gate;
  its `/v1/jobs` lists the job as adopted from `register{live_workers}`; spawn count still 1;
  `keel status --json` diff (minus `ts/server.*`) empty.
- **Seam 11 — ambiguous write:** a fault-injecting `ServerHub` whose `update` times out *after*
  the server executed the CAS must raise (not fall back); the next renew adopts the landed sha
  (claim.py L661-663) and the lease stays held.

---

## 12. Judge findings → disposition

| finding (judge) | disposition |
|---|---|
| Stale index → false `lost` → healthy gate killed (J1, J2, J3) | **Fixed**: §4.3 rules 1–2; seams 9 and 11 |
| Fallback re-issues a write after a timeout (J3) | **Fixed**: §4.3 rule 2 |
| Agent tokens minted in memory die with the server (J1, J2) | **Fixed**: derivable HMAC tokens, §8 |
| Transcripts/events/agent records on a PUBLIC repo (J1, J2, J3) | **Fixed**: private state repo for all `refs/fleet/*`; transcripts never leave runner disk; issue on the private repo |
| Single ruleset with bypass lets the deploy key force-push/delete the tip (J1, J2, J3) | **Fixed**: two rulesets (`tip-update` with bypass, `tip-guard` without) + `rescued-guard`; acceptance asserts deploy-key `--force` and delete are rejected |
| Plain HTTP + bearer on a public port (J1) | **Fixed**: tailnet precondition; server refuses non-tailnet bind |
| Stage-0 interim is chatty/slow against GitHub (J1, J2, J3) | **Mitigated**: targets ≤ 1 until the runner lands (the units pass no `--interval`, so fleetd reconciles at its `LOOP_SECONDS` default of 15 s; `--interval 30` is a hand-start knob only); `fetch_namespace` bulk read in the server; measured p95 in Stage 1 |
| Per-host PATs can delete `rescued/*` (J1) | **Fixed**: `rescued-guard` ruleset |
| Unreachable elected server holds a healthy lease (J3) | **Fixed**: `advertise_urls` + unreachable-demotion (§3.4 step 7); laptops never eligible; i7 autonomous gates |
| No mode starts work without a server (J3) | **Fixed**: `autonomous_when_serverless`, gates only |
| Runner refuses to start with both routes down (J2) | **Fixed**: job journal + offline start (§5.3) |
| No `/why` (J1, J2, J3) | **Fixed**: `/v1/why`, `keel why`, `refused[]` in the durable heartbeat from Stage 1 |
| Doctrine prompt-only (J1, J2, J3) | **Fixed**: sandbox one-ref remote + settings `PreToolUse` hooks + server-side tool allowlist |
| Author numbers are not the instrument (J2, J3) | **Fixed**: server re-runs `measure_baseline` as a probe on a `has_oracle` runner |
| Offered-claim dual renewer (J1, J2, J3 on D3) | **Not adopted**: offers are advice; runner acquires |
| Witness rule relaxing the kill (J1, J2, J3 on D3) | **Not adopted**; GitHub-down kills after ≈ 8 min are an **accepted risk** (CPU only); revisit only after Stage 7 blackhole drills produce numbers and a negative-control seam exists |
| Two schedulers double-count `record_dispatch` (J1, J2 on D3) | **Avoided**: autonomous mode dispatches gates only |
| Forged PASS by any runner PAT (J2) | **Accepted with mitigation**: train checks `written_by` against registered runners; alert on unknown writers; same trust model as today |
| Events lossy (≤ 5 min) (J1) | **Accepted**: all decisions reconstructible from claims/verdicts/attempts/intents/agent refs; `events.seq` delta is the named loss instrument in every drill |
| Stdlib HTTP SSE/long-poll is hand-rolled (J1, J3) | **Accepted with budget**: one extra day in Stage 2; connection cap 64; listener watchdog; escape hatch is a single-file `pyproject` with uvicorn if the watchdog fires in burn-in |
| Stage 2 of D1 under-budgeted (J3) | **Fixed**: PLAN budgets 18 h for the runner stage and 87 h overall |
| Durable heartbeat every 60 s is push-heavy (J2) | **Fixed**: 5 min |
| Clock skew (J1, J2) | **Mitigated**: NTP-offset check in doctor refuses registration > 30 s |
| macOS gate viability / i7-only regen (all) | **Non-goal**; routed by `train_platforms`/`can_regen` |
