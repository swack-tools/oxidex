#!/usr/bin/env python3
"""One `Hub` CAS operation per process, so `hyperfine` can time it.

WHY A SCRIPT AND NOT A LOOP. The number that matters is the latency of a
single `create`/`update` against the real spine, and the only honest way to
get a distribution of those is to run the operation once per process and let
`hyperfine -N` do the timing and the statistics. A Python loop would measure
a warm connection pool that the fleet never has: every fleetd tick, every
lease renewal and every verdict store is a cold `git push` from a fresh
process.

WHAT IS AND IS NOT IN THE NUMBER. Everything a real caller pays: interpreter
start, `import fleetlib`, `Hub()` (which `stat`s the cache repo), the local
`hash-object`/`mktree`/`commit-tree`, and the network `git push`. That means
the interpreter floor is in there too, which is why `noop` exists -- it does
the identical setup and no network at all, so `create - noop` is the round
trip and `noop` is what you subtract if you want it. REPORT BOTH; a `create`
p50 quoted without its floor is a claim about Python's startup as much as
about GitHub.

`update` needs a CAS witness, and resolving one is itself a round trip. To
keep that out of the measurement, `prepare-update` writes the current sha to
a side file and is run from hyperfine's `--prepare` hook (untimed); the timed
`update` reads the file. Without that the `update` number is silently
`sha() + update()`, i.e. two round trips wearing one name.

Usage (the acceptance invocation, from the repo root):

    export FLEET_BENCH_HUB_URL=https://github.com/<owner>/<scratch>.git
    export FLEET_GIT_TOKEN_FILE=<path>   # HTTPS only, and only if the PAT is not at ~/.keel/secrets/git-token
    export FLEET_BENCH_DIR=/tmp/fleet-bench
    python3 tools/fleet/keel/bench_hub.py setup
    hyperfine -N --warmup 2 --runs 20 --export-json create.json \\
        'python3 tools/fleet/keel/bench_hub.py create'
    hyperfine -N --warmup 2 --runs 20 --export-json update.json \\
        --prepare 'python3 tools/fleet/keel/bench_hub.py prepare-update' \\
        'python3 tools/fleet/keel/bench_hub.py update'
    python3 tools/fleet/keel/bench_hub.py sweep       # deletes every ref it made

Every ref it creates lives under `refs/fleet/bench/<run-id>/`, and `sweep`
deletes that namespace. Never point this at a repo whose refs you would
mind losing under that prefix.
"""

from __future__ import annotations

import os
import sys
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from fleetlib import Hub  # noqa: E402


def _bench_dir() -> Path:
    d = Path(os.environ.get("FLEET_BENCH_DIR", "/tmp/fleet-bench"))
    d.mkdir(parents=True, exist_ok=True)
    return d


def _hub() -> Hub:
    url = os.environ.get("FLEET_BENCH_HUB_URL", "").strip()
    if not url:
        sys.exit("FLEET_BENCH_HUB_URL is not set")
    return Hub(url=url, workdir=str(_bench_dir() / "cache"))


def _namespace() -> str:
    path = _bench_dir() / "namespace"
    if not path.exists():
        sys.exit("run `bench_hub.py setup` first")
    return path.read_text().strip()


def main(argv):
    if len(argv) != 2:
        sys.exit(__doc__)
    op = argv[1]

    if op == "setup":
        ns = f"refs/fleet/bench/{uuid.uuid4().hex}/"
        (_bench_dir() / "namespace").write_text(ns)
        hub = _hub()
        ref = ns + "updated"
        if not hub.create(ref, {"n": 0}):
            sys.exit(f"could not create {ref}")
        print(ns)
        return 0

    if op == "noop":
        # Identical setup, zero network: the floor to subtract.
        _hub()
        return 0

    hub = _hub()
    ns = _namespace()

    if op == "create":
        ref = ns + uuid.uuid4().hex
        if not hub.create(ref, {"bench": "create"}):
            sys.exit(f"create lost a race on a fresh uuid ref: {ref}")
        return 0

    if op == "prepare-update":
        ref = ns + "updated"
        sha = hub.sha(ref)
        if sha is None:
            sys.exit(f"{ref} is absent -- run setup")
        (_bench_dir() / "expect_sha").write_text(sha)
        return 0

    if op == "update":
        ref = ns + "updated"
        sha = (_bench_dir() / "expect_sha").read_text().strip()
        if not hub.update(ref, {"bench": "update"}, expect_sha=sha):
            sys.exit(f"update lost the CAS on {ref} -- prepare-update did not run")
        return 0

    if op == "sweep":
        removed = 0
        for ref, sha in sorted(hub.list(ns).items()):
            if hub.delete(ref, expect_sha=sha):
                removed += 1
            else:
                print(f"WARNING: lost a race deleting {ref}", file=sys.stderr)
        print(f"swept {removed} ref(s) under {ns}")
        return 0

    sys.exit(f"unknown op {op!r}")


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
