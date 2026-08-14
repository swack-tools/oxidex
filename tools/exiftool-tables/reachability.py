#!/usr/bin/env python3
"""Step 28's reachability census -- GENERATED, not hand-audited.

    python3 tools/exiftool-tables/reachability.py [--json-out FILE]

Answers, for every table `codegen.py` emitted, exactly one of:

    enabled     both gates pass AND the table is on `src/exiftool_tables/
                enabled.rs`'s allowlist -- the generic engine may walk it.
    eligible    gate A passes, but no measured allowlist line. NOT enabled
                (design D1 is opt-in): this is the count that says how much
                coverage is being held back for want of a gate-B measurement,
                and publishing it is what stops "eligible" from quietly
                becoming "enabled".
    refused     gate A blocks it, with the generator's own counter names as
                the reason.

Plus, orthogonally, whether a table is reached today by a hand-wired
`find_table(...)` call site in `src/` -- the pre-Step-28 notion of
"reachable", which is a different axis from enablement and is reported as
such rather than conflated with it.

Why this is a script and not a paragraph in a doc: the numbers move on every
regeneration, and `docs/reference/corpus-synthesis.md` records what happens
when they are not re-derived (a 22-vs-21 discrepancy that survived because
the count was hand-made). Reading gate A out of the generated Rust means the
report cannot disagree with the artifact it describes.

Instrument: parses `src/exiftool_tables/binary_tables.rs` and
`src/exiftool_tables/enabled.rs` directly -- the committed artifacts, not a
fresh dump -- so it needs neither Perl nor a corpus and can run in CI.
"""

import argparse
import json
import pathlib
import re
import sys
from collections import Counter

ROOT = pathlib.Path(__file__).resolve().parents[2]
TABLES = ROOT / "src/exiftool_tables/binary_tables.rs"
ALLOWLIST = ROOT / "src/exiftool_tables/enabled.rs"
SRC = ROOT / "src"

sys.path.insert(0, str(ROOT / "scripts"))
import instrument  # noqa: E402 -- git/instrument identity header

# `module: "X",\n    table: "Y",` ... up to that table's `gate_a` literal.
TABLE_RE = re.compile(
    r'pub static \w+: BinaryTable = BinaryTable \{\n'
    r'    module: "(?P<module>[^"]+)",\n'
    r'    table: "(?P<table>[^"]+)",\n'
    r'(?P<body>.*?)\n\};',
    re.S,
)
GATE_RE = re.compile(r"gate_a: GateA \{\s*blocked_by: (?P<list>&\[.*?\]),?\s*\},", re.S)
REASON_RE = re.compile(r'\("([a-z_]+)", (\d+)\)')
EDGE_RE = re.compile(r'subdir: Some\(SubdirEdge \{\s*module: "([^"]+)",\s*table: "([^"]+)"')


def parse_tables():
    src = TABLES.read_text(encoding="utf-8")
    out = []
    for m in TABLE_RE.finditer(src):
        body = m.group("body")
        gate = GATE_RE.search(body)
        if gate is None:
            raise SystemExit(
                f"{m.group('module')}::{m.group('table')} has no gate_a literal -- "
                "regenerate with `just regen-tables`"
            )
        reasons = [(k, int(n)) for k, n in REASON_RE.findall(gate.group("list"))]
        out.append(
            {
                "module": m.group("module"),
                "table": m.group("table"),
                "gate_a": not reasons,
                "blocked_by": reasons,
                "fields": body.count("            name: \"") or body.count("name: \""),
                "edges": EDGE_RE.findall(body),
            }
        )
    return out


def parse_allowlist():
    src = ALLOWLIST.read_text(encoding="utf-8")
    body = src.split("pub static ENABLED", 1)[1].split("];", 1)[0]
    # Ignore the `//` commentary that carries each line's evidence.
    body = "\n".join(l for l in body.splitlines() if not l.strip().startswith("//"))
    return {(m, t) for m, t in re.findall(r'\("([^"]+)",\s*"([^"]+)"\)', body)}


def parse_call_sites():
    """Every `find_table("Mod", "Tbl")` in non-test `src/` code.

    Literal pairs only. A model-dispatched lookup
    (`find_table("Sony", table_name)`) resolves at runtime and is reported
    separately as `dynamic`, never guessed at -- `docs/reference/
    corpus-synthesis.md` records the 22-vs-21 discrepancy that came from
    counting call sites instead of live tables.
    """
    static, dynamic = set(), 0
    pat = re.compile(r'find_table\(\s*"([^"]+)"\s*,\s*(?:"([^"]+)"|(\w+))\s*\)')
    for path in SRC.rglob("*.rs"):
        if path.name in {"binary_tables.rs", "enabled.rs"}:
            continue
        # Strip `//` line comments before matching. Without this the census
        # counts a call site that does not exist: `ricoh.rs:215` NAMES
        # `find_table("Ricoh","ImageInfo")` in prose explaining why that
        # module does NOT call it, and the first version of this script
        # reported Ricoh::ImageInfo as hand-wired on the strength of that
        # sentence -- then a candidate table got allowlisted on it. Same
        # class of error as `AGENTS.md`'s "name the instrument": the tool was
        # measuring the wrong thing, confidently.
        text = "\n".join(
            line.split("//", 1)[0] if "//" in line else line
            for line in path.read_text(encoding="utf-8").splitlines()
        )
        for module, table, ident in pat.findall(text):
            if table:
                static.add((module, table))
            elif ident:
                dynamic += 1
    return static, dynamic


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--json-out")
    args = ap.parse_args()

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "reachability.py")
    instrument.print_header(
        tool="reachability.py",
        git=git,
        dirty_overridden=dirty_overridden,
        extra=[f"reads:   {TABLES.relative_to(ROOT)}, {ALLOWLIST.relative_to(ROOT)} "
               "(committed artifacts, not a fresh dump -- no ExifTool or oxidex involved)"],
    )

    tables = parse_tables()
    allowed = parse_allowlist()
    call_sites, dynamic = parse_call_sites()
    known = {(t["module"], t["table"]) for t in tables}

    enabled, eligible, refused = [], [], []
    for t in tables:
        key = (t["module"], t["table"])
        t["hand_wired"] = key in call_sites
        if t["gate_a"] and key in allowed:
            t["status"] = "enabled"
            enabled.append(t)
        elif t["gate_a"]:
            t["status"] = "eligible"
            eligible.append(t)
        else:
            t["status"] = "refused"
            refused.append(t)

    print(f"tables emitted            {len(tables)}")
    print(f"  enabled  (gate A + measured allowlist)   {len(enabled)}")
    print(f"  eligible (gate A, awaiting a gate B run) {len(eligible)}")
    print(f"  refused  (gate A blocks)                 {len(refused)}")
    print()
    print(f"hand-wired find_table call sites (a DIFFERENT axis to enablement):")
    print(f"  distinct tables named by a literal lookup {len(call_sites & known)}")
    print(f"  literal lookups naming no emitted table   "
          f"{sorted(call_sites - known)}")
    print(f"  model-dispatched lookups (runtime name)   {dynamic}")
    print()

    # The intersection is what a gate-B run can actually measure today: a
    # table with no live call site produces no tags on the corpus, so
    # "enabling" it would be enabling it on no evidence (design D3).
    print("hand-wired AND gate A -- the tables a corpus A/B can measure today:")
    for t in sorted(tables, key=lambda t: (t["module"], t["table"])):
        if t["hand_wired"] and t["gate_a"]:
            print(f"  {t['module']}::{t['table']:<22} {t['status']}")
    print("hand-wired but gate A refuses (cannot be enabled without new "
          "transcription):")
    for t in sorted(tables, key=lambda t: (t["module"], t["table"])):
        if t["hand_wired"] and not t["gate_a"]:
            why = ", ".join(f"{k}={n}" for k, n in t["blocked_by"])
            print(f"  {t['module']}::{t['table']:<22} {why}")
    print()

    reasons = Counter()
    for t in refused:
        for key, n in t["blocked_by"]:
            reasons[key] += 1
    print("gate A refusal reasons (tables affected; a table can trip several):")
    for key, n in reasons.most_common():
        print(f"  {key:<34} {n}")
    print()

    # Edges are the engine's only automatic enablement path, so the census
    # has to say where they land.
    edge_targets = Counter()
    live_sources = 0
    for t in tables:
        if not t["edges"]:
            continue
        if t["hand_wired"]:
            live_sources += 1
        for target in t["edges"]:
            edge_targets[target] += 1
    by_status = {(x["module"], x["table"]): x["status"] for x in tables}
    print(f"SubDirectory edges: {sum(edge_targets.values())} from "
          f"{sum(1 for t in tables if t['edges'])} tables "
          f"({live_sources} of which are hand-wired today)")
    for target, n in sorted(edge_targets.items()):
        print(f"  -> {target[0]}::{target[1]:<22} x{n:<3} {by_status.get(target, 'no-layout')}")

    if args.json_out:
        pathlib.Path(args.json_out).write_text(
            json.dumps(
                {
                    "tables": len(tables),
                    "enabled": len(enabled),
                    "eligible": len(eligible),
                    "refused": len(refused),
                    "gate_a_refusal_reasons": dict(reasons),
                    "per_table": tables,
                },
                indent=2,
                sort_keys=True,
            ),
            encoding="utf-8",
        )
        print(f"\nwrote {args.json_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
