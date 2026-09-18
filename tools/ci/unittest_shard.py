#!/usr/bin/env python3
"""Run one deterministic shard of a unittest discovery suite.

Every test id from `unittest discover` is assigned to exactly one of N shards
by greedy weight balancing, so the shards together run the whole suite once.
Weights come from a committed timing file keyed by module or by full test id
(module.Class.method); a module's weight is spread over its tests unless an
id carries its own. Unknown modules get a small default, so a new test file is
still run -- it just is not balanced until its timing is recorded.

Usage (from the suite directory):
  python3 unittest_shard.py --weights weights.json --shard 2 --of 6 [--list]
"""
from __future__ import annotations

import argparse
import json
import sys
import unittest
from pathlib import Path

DEFAULT_MODULE_SECONDS = 5.0


def flatten(suite):
    for item in suite:
        if isinstance(item, unittest.TestSuite):
            yield from flatten(item)
        else:
            yield item


def weigh(tests, seconds):
    by_module = {}
    for test in tests:
        by_module.setdefault(test.id().split(".", 1)[0], []).append(test)
    weights = {}
    for module, members in by_module.items():
        explicit = {t.id(): float(seconds[t.id()]) for t in members if t.id() in seconds}
        rest = [t for t in members if t.id() not in explicit]
        module_seconds = float(seconds.get(module, DEFAULT_MODULE_SECONDS))
        remaining = max(module_seconds - sum(explicit.values()), 0.0)
        share = remaining / len(rest) if rest else 0.0
        for test in members:
            weights[test.id()] = explicit.get(test.id(), share)
    return weights


def unweighted_modules(tests, seconds):
    """Discovered modules with no whole-module entry in the timing file.

    A synthetic `unittest.loader._FailedTest` (a module that failed to
    import) is not a module of the suite and is not reported.
    """
    modules = {test.id().split(".", 1)[0] for test in tests}
    modules.discard("unittest")
    return sorted(modules - set(seconds))


def partition(weights, shards):
    """-> list of sorted id lists; each id appears in exactly one shard."""
    if shards < 1:
        raise ValueError("shard count must be positive")
    loads = [0.0] * shards
    members = [[] for _ in range(shards)]
    for test_id in sorted(weights, key=lambda key: (-weights[key], key)):
        target = min(range(shards), key=lambda index: (loads[index], index))
        loads[target] += weights[test_id]
        members[target].append(test_id)
    return [sorted(ids) for ids in members]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--weights", type=Path, required=True)
    parser.add_argument("--shard", type=int, required=True, help="1-based shard index")
    parser.add_argument("--of", type=int, required=True, dest="shards")
    parser.add_argument("--start-dir", default=".")
    parser.add_argument("--pattern", default="test_*.py")
    parser.add_argument("--list", action="store_true", help="print the shard's test ids and exit")
    args = parser.parse_args(argv)
    if not 1 <= args.shard <= args.shards:
        parser.error("--shard must be between 1 and --of")
    seconds = json.loads(args.weights.read_text())["seconds"]
    suite = unittest.defaultTestLoader.discover(args.start_dir, pattern=args.pattern)
    tests = list(flatten(suite))
    # A module that fails to import surfaces as a synthetic failing test;
    # partitioning keeps it, so some shard reports the error.
    ids = [test.id() for test in tests]
    if len(set(ids)) != len(ids):
        raise SystemExit("duplicate test ids; cannot partition deterministically")
    weights = weigh(tests, seconds)
    partitions = partition(weights, args.shards)
    selected = set(partitions[args.shard - 1])
    chosen = [test for test in tests if test.id() in selected]
    if args.list:
        # The ids go to stdout; the predicted balance goes to stderr so a
        # consumer of the id list is unaffected.
        loads = [sum(weights[i] for i in part) for part in partitions]
        for index, (part, load) in enumerate(zip(partitions, loads), 1):
            print(f"predicted shard {index}/{args.shards}: {len(part)} tests, {load:.0f}s",
                  file=sys.stderr)
        print(f"predicted max/min: {max(loads) / max(min(loads), 1e-9):.2f}; "
              f"unweighted modules: {', '.join(unweighted_modules(tests, seconds)) or 'none'}",
              file=sys.stderr)
        print("\n".join(test.id() for test in chosen))
        return 0
    print(f"shard {args.shard}/{args.shards}: {len(chosen)} of {len(tests)} tests", flush=True)
    result = unittest.TextTestRunner(verbosity=2).run(unittest.TestSuite(chosen))
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
