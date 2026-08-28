# Fleet Wave 3 — Go-Live Runbook

> **HISTORICAL (2026-08-22):** the ryzen — `ubuntuwork`, the work2 pod, and the
> `work2.oxidex.net:2244` hub this runbook installs against — was removed from the fleet by
> operator decision, and the topology moved to the GitHub spine (`docs/AGENT-SERVER-SPEC.md`).
> The work2/ubuntuwork steps below are kept as history only; do not execute them.

Executes the rollout of `tools/fleet/` onto the live fleet. Everything
here is additive and reversible; the legacy path (`~/gate-nocache.sh` +
cron topup) keeps working until the final step deliberately retires it.

**Operator note:** every script defaults to dry-run; nothing writes
without `--execute`. Production writes happen only through the numbered
steps below, in order. If any verification fails, stop — do not proceed
to the next step with a wobbly previous one.

## 0. Preconditions

- Tip contains T2.1 (`fleetd.py`, `cli.py`, `units/`) — gate it in before
  installing anything that runs it.
- Per-host checkout: each host gets `~/fleet-checkout`, a clone of the
  hub pinned to the tip (`git clone --branch refactor/tag-machinery <hub>
  ~/fleet-checkout`). Updating the fleet tooling on a host is `git -C
  ~/fleet-checkout pull` — versioned distribution, no scp'd scripts.
- Oracle cache: every host needs `/tmp/oxidex-exiftool-cache` →
  `~/oxidex-cache` (symlink + persistent home copy). **On the Macs the
  home copy is mandatory** — macOS purges `/tmp` periodically; it deleted
  the m5's cache mid-day on 2026-08-15. `rsync -a -e "ssh -p 2244"
  allen@work2.oxidex.net:/home/allen/oxidex-cache/ ~/oxidex-cache/` then
  `ln -sfn ~/oxidex-cache /tmp/oxidex-exiftool-cache`.
- `python3 tools/fleet/doctor.py <host>` green (or explained) per host.

## 1. Hook install (on the hub — the work2 pod)

```
ssh -p 2244 allen@work2.oxidex.net
cd ~/fleet-checkout && git pull
tools/fleet/rollout/install_hook.sh ~/git/oxidex.git            # review dry-run
tools/fleet/rollout/install_hook.sh ~/git/oxidex.git --execute
```

The installer preserves the existing fastcheck hook verbatim as
`hooks/post-receive.legacy` and chains it (stdin captured once, fed to
both halves). Its `~/.train-queue` append becomes dead weight after step
4 — harmless, and keeping it preserves byte-exact rollback.

**Verify:** push any throwaway staging branch; then
`git --git-dir ~/git/oxidex.git for-each-ref refs/fleet/signals/` must
show `tip` after the NEXT tip advance (not after a staging push — the
fleet half only bumps on `refactor/tag-machinery`).

## 2. Seed the desired state

```
python3 tools/fleet/rollout/seed_desired.py                     # review
FLEET_HUB_URL=<hub> python3 tools/fleet/rollout/seed_desired.py --execute
```

All targets start at ZERO with `enabled: true`: fleetd's first act on
every host is to observe and heartbeat, never to start or kill anything.
Hand-running gates continues to work untouched. Refuses to overwrite an
existing `refs/fleet/desired`.

## 3. fleetd bring-up — one host at a time, i7 first

Per host, as the OWNING user (`swackhamer` on ubuntuwork — the only step
that needs someone with that login):

```
git clone --branch refactor/tag-machinery <hub> ~/fleet-checkout   # or pull
# Linux:  cp ~/fleet-checkout/tools/fleet/units/fleetd.service ~/.config/systemd/user/
#         systemctl --user daemon-reload && systemctl --user enable --now fleetd
#         loginctl enable-linger $USER
# macOS:  cp ~/fleet-checkout/tools/fleet/units/com.oxidex.fleetd.plist ~/Library/LaunchAgents/
#         launchctl bootstrap gui/$UID ~/Library/LaunchAgents/com.oxidex.fleetd.plist
# Both:   add units/cron-backstop.txt line via crontab -e
# work2 pod only: FLEET_HOST=work2pod in the unit env (k8s hostname is unstable)
```

**Set `FLEET_HOST` in the unit on EVERY host**, to that host's key under `hosts`
in `tools/fleet/rollout/seed_desired.py` — `server`, `oldair`, `m5`. It is not
`hostname -s`, and the two only happen to coincide on the i7.
`fleetd.host_identity()` falls back to the hostname when the variable is unset,
which made this look like a pod-only quirk: in the Stage 1 LIVE run
(2026-08-27/28) the m5 reported as `Allens-Air`, a name `refs/fleet/desired` has
never heard of, and `fleet status --why` printed `refused: disabled ()` — an
EMPTY reason — with no `m5` row at all, which reads as "an operator took this
host down" and was the opposite of the truth. The committed `fleetd.service`,
`com.oxidex.fleetd.plist` and `cron-backstop.txt` now carry `FLEET_HOST`; the
plist's and the cron line's values are hand-substituted per host, exactly like
the `/Users/allen/...` paths beside them. A wrong or missing name now refuses
with `unknown-host` instead of `disabled`.

**Both units also set `PATH` with `~/.cargo/bin` first.** `fleetd.service` did
not until 2026-08-28 — see the `platform_id` row in the table below.

**Verify before the next host** (this is the trust gate):
0. `python3 tools/fleet/toolchain.py ids --format sh` on the host, and the same
   value in the heartbeat's `platform_id`. fleetd refuses to start when its own
   `platform_id` differs from the one its gate command computes, so a mismatch
   shows up as a refusing daemon in `systemctl --user status fleetd` /
   `/tmp/fleetd.log` rather than as silence.
1. `fleet status` shows the host `up` **under its seeded name**, heartbeat < 30s,
   correct `owning_user`, oracle ✓.
2. `gates_running` in the heartbeat matches an independent
   `ps -eo pid,pgid,cmd | grep "[g]ate"` count on that host.
3. Watch one full cycle: `fleet up <host> --gates 1`, see fleetd claim a
   queue branch and start a gate; `fleet drain <host>`, see it start
   nothing new while the gate runs on.

Order: **i7 → work2 pod → M4 → ubuntuwork (needs swackhamer) → m5 last**
(maintainer's machine; leave targets at 0 unless they say otherwise).

## 4. Retire the legacy machinery — ONLY after all heartbeats are trusted

Per host: remove the topup/fetch cron lines (`crontab -e`), delete
`~/.train-queue`, rename `~/gate-nocache.sh` → `~/gate-nocache.sh.retired`
(rollback path — do not delete). The repo's `tools/fleet/gate.sh` is the
only gate entry point from here on.

## 5. Rollback (any point)

- Stop starting work fleet-wide: `fleet down <host> --reason "..."` per
  host — running gates finish, nothing new starts.
- Full retreat: `systemctl --user disable --now fleetd` / `launchctl
  bootout gui/$UID/com.oxidex.fleetd`; remove the cron backstop line;
  restore `hooks/post-receive` from `hooks/post-receive.legacy`; rename
  `~/gate-nocache.sh.retired` back; re-add the old cron lines.
- `refs/fleet/*` refs are inert without fleetd reading them — safe to
  leave in place during a retreat.

## What can go wrong (measured, not hypothetical)

| symptom | cause | response |
|---|---|---|
| pushes fail in bursts, succeed singly | hub post-receive lock + 1Password agent dropping rapid signature requests | fleetd/cli already retry with backoff; for manual ops, space pushes or use one ssh session with server-side `update-ref` |
| a Mac host's oracle probe goes ✗ overnight | macOS purged /tmp | re-run the symlink line from step 0; the home copy survives |
| heartbeat DOWN, host reachable | fleetd died AND its cron backstop was removed | reinstall backstop; `systemctl --user status fleetd` / `/tmp/fleetd.log` |
| gate FAIL on both Macs, same code green on Linux | platform-specific verdict — this is why `platform_id` is in the cache key | never let a Linux PASS satisfy a Mac slot; investigate the macOS-only failure |
| fleetd on work2 pod heartbeats under a `work2box-*` name | FLEET_HOST not set in unit env | set `FLEET_HOST=work2pod`, restart fleetd, delete the stray hosts ref |
| a host heartbeats under its `hostname -s` and `fleet status --why` says `refused: disabled ()` with an empty reason (m5 as `Allens-Air`, live run 2026-08-27/28) | same cause as the row above, and it is NOT pod-specific: no unit set `FLEET_HOST` at all, so every host whose seeded name differs from its hostname reported under a name `refs/fleet/desired` does not contain | set `FLEET_HOST=<seeded name>` in the unit, restart fleetd, delete the stray `refs/fleet/hosts/<hostname>`; the reason line is now `unknown-host (… set FLEET_HOST)` instead of `disabled` |
| a host re-gates the same branch every ~21 min while a PASS for that tree is already on the state repo; `classify_branch` never says AWAITING_TRAIN (i7, live run 2026-08-27/28) | fleetd and the gate it spawns computed different `platform_id`s (`b2bdf493…` vs `b6613b19…`) for the SAME rustc, so the scheduler read a cache slot nothing writes. Root cause was a trailing newline: `$(rustc -vV)` strips it, `subprocess.run().stdout` keeps it — three implementations of one formula, none of them compared | one resolver now (`tools/fleet/toolchain.py`, sourced into shell by `units/fleet-toolchain.sh`); fleetd refuses to start on a mismatch. Verify with `python3 tools/fleet/toolchain.py ids --format sh` and the last path segment of `refs/fleet/verdicts/<tree>/<gv>/<platform_id>` |
| regen fails on any non-i7 host with a digest-mismatch abort | oracle ledger is Perl-version-bound (i7 5.38.2 only) — the abort being loud is `fix-ledger-loud` working | route regen work to the i7; do not "fix" by regenerating the ledger casually |
