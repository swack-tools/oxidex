"""Seed `refs/fleet/desired` from CURRENT reality, so fleetd's first
reconcile on every host is a NO-OP -- a mass restart on first contact is
the failure this exists to prevent.

Prints the payload for review; writes only with --execute. The initial
counts are all zeros with enabled=true (except where noted): fleetd
STARTS nothing until an operator raises targets with `fleet up`, and a
running hand-launched gate is simply not fleetd's to manage -- it drains
naturally as those finish. That is the safest possible first contact: the
reconciler observes, heartbeats, and touches nothing.

    python3 tools/fleet/rollout/seed_desired.py [--execute]

Host facts encoded below were measured 2026-08-14/15 (docs/FLEET.md
addenda). Adjust with `fleet up/down` after go-live rather than editing
this file.

PLAN Stage 1 task 6 / SPEC §3.1 additions: `server_candidates` and
`train_platforms`, the two `desired` fields Stage 3/4 need that did not
exist before this task (`server_candidates` for the automatic re-host
election, SPEC §3.4: "a `server_eligible` runner (`desired.
server_candidates`, Linux only; laptops are never eligible)"; `train_platforms`
for the Stage 4 `remote_gate` capability match). Both are seeded
CONSERVATIVELY, the same "start from a safe, inert default" spirit as the
all-zero host counts above:

  * `server_candidates` lists every host FLEET.md documents as Linux and
    non-laptop -- `server` (i7), `ubuntuwork` and `work2pod` (both on the
    ryzen) -- ranked in the order PLAN Stage 1's "Starting state" measured
    them coming back up (i7 first: "only regen/oracle host", currently the
    only one up; the ryzen host, then its pod). `oldair` (M4) and `m5` are
    macOS (`docs/FLEET.md` L120: "launchd ... on macOS (M4 `oldair`, m5)")
    and excluded on that basis alone; `m5` is additionally the maintainer's
    laptop, SPEC §3.4's separate "laptops are never eligible" exclusion.
    `advertise_urls` is left `[]` for every candidate: a host's tailnet/LAN
    IP is a runtime fact the elected server measures for itself at
    election time (SPEC §3.4 step 1, `tailscale ip`/`hostname -I`), not
    static config this script can know or should guess -- see
    `docs/TRANSCRIPTION.md`'s and this project's `AGENTS.md`'s "never
    approximate" rule, which applies here exactly as it does to a
    generated tag table: an invented IP is worse than an empty list,
    because nothing downstream can tell it apart from a measured one.
  * `train_platforms` is seeded EMPTY. A `platform_id` is
    `sha256(rustc -vV)` of one specific host's toolchain output
    (`claim.compute_platform_id`) -- a fact that can only be measured BY
    running `rustc -vV` ON that host, never inferred from docs or from
    this laptop. No Linux gate host has had that command run against it as
    part of this task, so there is nothing correct to write here; an empty
    list is Stage 4's `remote_gate` capability match finding no eligible
    platform (fails closed: it schedules nothing rather than a WRONG
    platform), not a bug. Populate it by running
    `python3 -c "from claim import compute_platform_id; print(compute_platform_id())"`
    on each Linux gate host once Stage 4 exists, or via `fleet up`.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from fleetlib import Hub

DESIRED_REF = "refs/fleet/desired"

SEED = {
    "generation": 1,
    "hosts": {
        # i7: primary gate runner, the ONLY regen-capable host (oracle
        # ledger digest matches its Perl 5.38.2 alone).
        "server": {"gates": 0, "agents": 0, "enabled": True},
        # ryzen host-side: fleet work there is owned by user `swackhamer`;
        # fleetd must be installed AS that user or this entry stays idle.
        "ubuntuwork": {"gates": 0, "agents": 0, "enabled": True},
        # work2 pod (on the ryzen; hostname work2box-*): 24 cores, hub is
        # local disk. FLEET_HOST=work2pod must be set in its fleetd env
        # since its k8s hostname is unstable across pod restarts.
        "work2pod": {"gates": 0, "agents": 0, "enabled": True},
        # M4: gate-capable since the strip fix + python3 symlink; ~19-21G
        # free is tight against the 14G floor. Not regen-capable.
        "oldair": {"gates": 0, "agents": 0, "enabled": True},
        # m5: the maintainer's dev machine. Enabled but zero-target; raise
        # only when the maintainer isn't using it. Full-gate validation
        # was interrupted twice by session crashes -- attempt before
        # trusting it with real verdicts. /tmp oracle cache gets purged
        # by macOS on this host; the installer must use a home-dir copy
        # plus symlink (same layout as the Linux hosts).
        "m5": {"gates": 0, "agents": 0, "enabled": True},
    },
    "limits": {"min_free_gb": 14, "min_free_mem_gb": 8},
    # SPEC §3.1 / PLAN Stage 1 task 6 -- see the module docstring for why
    # each is populated (or deliberately left empty) the way it is.
    "server_candidates": [
        {"host": "server", "rank": 1, "advertise_urls": []},
        {"host": "ubuntuwork", "rank": 2, "advertise_urls": []},
        {"host": "work2pod", "rank": 3, "advertise_urls": []},
    ],
    "train_platforms": [],
}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--hub", default=os.environ.get("FLEET_HUB_URL"))
    ap.add_argument("--execute", action="store_true")
    args = ap.parse_args(argv)

    print(json.dumps(SEED, indent=2))
    if not args.execute:
        print("\nDRY-RUN (default). Re-run with --execute to write "
              f"{DESIRED_REF}. Refuses if the ref already exists.", file=sys.stderr)
        return 0

    if not args.hub:
        print("seed_desired: no hub URL (--hub or FLEET_HUB_URL)", file=sys.stderr)
        return 2
    hub = Hub(args.hub, workdir=Path.home() / ".fleetd" / "seedcache")
    if hub.sha(DESIRED_REF) is not None:
        print(f"seed_desired: {DESIRED_REF} already exists -- refusing to "
              "overwrite a live desired state. Use `fleet up/down` to edit it.",
              file=sys.stderr)
        return 3
    ok = hub.create(DESIRED_REF, SEED)
    print(f"created {DESIRED_REF}: {ok}")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
