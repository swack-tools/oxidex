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
