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

Slice I-1 adds the same census for the IFD-style tables: gate A out of
`src/exiftool_tables/ifd_tables.rs`, gate B out of
`src/exiftool_tables/enabled_ifd.rs` (`ENABLED_IFD`), and literal
`find_ifd_table("Mod", "Tbl")` call sites. It is printed as its own section
after the binary one (whose text is unchanged), and skipped with a message
when `ifd_tables.rs` is not on the tree. `--json-out` carries both: the
top-level `tables`/`enabled`/`eligible`/`refused`/`gate_a_refusal_reasons`
keys keep their binary-only meaning, every `per_table` row now says its
`kind` (`"binary"` or `"ifd"`), and an `ifd` block repeats the four counts
and the refusal reasons for the IFD kind.
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
IFD_TABLES = ROOT / "src/exiftool_tables/ifd_tables.rs"
IFD_ALLOWLIST = ROOT / "src/exiftool_tables/enabled_ifd.rs"
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
# Slice I-1: the IFD statics, same rustfmt layout, their own edge type.
IFD_TABLE_RE = re.compile(
    r'pub static \w+: IfdTable = IfdTable \{\n'
    r'    module: "(?P<module>[^"]+)",\n'
    r'    table: "(?P<table>[^"]+)",\n'
    r'(?P<body>.*?)\n\};',
    re.S,
)
IFD_EDGE_RE = re.compile(
    r'subdir: Some\(IfdSubdirEdge \{\s*module: "([^"]+)",\s*table: "([^"]+)"'
)


def parse_tables(path=TABLES, kind="binary"):
    src = path.read_text(encoding="utf-8")
    table_re, edge_re = (TABLE_RE, EDGE_RE) if kind == "binary" else (IFD_TABLE_RE, IFD_EDGE_RE)
    out = []
    for m in table_re.finditer(src):
        body = m.group("body")
        gate = GATE_RE.search(body)
        if gate is None:
            raise SystemExit(
                f"{m.group('module')}::{m.group('table')} has no gate_a literal -- "
                "regenerate with `just regen-tables`"
            )
        reasons = [(k, int(n)) for k, n in REASON_RE.findall(gate.group("list"))]
        if kind == "binary":
            fields = body.count("            name: \"") or body.count("name: \"")
        else:
            # Every `IfdTag {` in the static: plain `tags:` entries plus
            # `_variants` alternatives (an alternative is a tag entry too).
            fields = body.count("IfdTag {")
        out.append(
            {
                "module": m.group("module"),
                "table": m.group("table"),
                "gate_a": not reasons,
                "blocked_by": reasons,
                "fields": fields,
                "edges": edge_re.findall(body),
            }
        )
    return out


def parse_allowlist(path=ALLOWLIST, static_name="ENABLED"):
    src = path.read_text(encoding="utf-8")
    body = src.split(f"pub static {static_name}", 1)[1].split("];", 1)[0]
    # Ignore the `//` commentary that carries each line's evidence.
    body = "\n".join(l for l in body.splitlines() if not l.strip().startswith("//"))
    return {(m, t) for m, t in re.findall(r'\("([^"]+)",\s*"([^"]+)"\)', body)}


def parse_call_sites(fn="find_table", skip=("binary_tables.rs", "enabled.rs")):
    """Every `find_table("Mod", "Tbl")` in non-test `src/` code.

    Literal pairs only. A model-dispatched lookup
    (`find_table("Sony", table_name)`) resolves at runtime and is reported
    separately as `dynamic`, never guessed at -- `docs/reference/
    corpus-synthesis.md` records the 22-vs-21 discrepancy that came from
    counting call sites instead of live tables.

    Slice I-1 runs the same scan for `find_ifd_table` (`fn`), skipping the
    IFD artifacts instead. `\\b` in front of the name keeps the two lookups
    apart: `find_ifd_table(` does not contain `find_table(`, but the anchor
    is what makes that a rule rather than a coincidence of spelling.
    """
    static, dynamic = set(), 0
    pat = re.compile(rf'\b{fn}\(\s*"([^"]+)"\s*,\s*(?:"([^"]+)"|(\w+))\s*\)')
    for path in SRC.rglob("*.rs"):
        if path.name in skip:
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


def classify(tables, allowed, call_sites):
    """Stamp `status`/`hand_wired` on each table -> (enabled, eligible, refused)."""
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
    return enabled, eligible, refused


def report(tables, enabled, eligible, refused, call_sites, dynamic, fn, target_status):
    """Print one kind's census. `target_status` maps `(module, table)` of
    EVERY known table (both kinds -- an IFD edge may point at a binary
    table and vice versa) to its status, for the edge listing."""
    known = {(t["module"], t["table"]) for t in tables}
    print(f"tables emitted            {len(tables)}")
    print(f"  enabled  (gate A + measured allowlist)   {len(enabled)}")
    print(f"  eligible (gate A, awaiting a gate B run) {len(eligible)}")
    print(f"  refused  (gate A blocks)                 {len(refused)}")
    print()
    print(f"hand-wired {fn} call sites (a DIFFERENT axis to enablement):")
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
    print(f"SubDirectory edges: {sum(edge_targets.values())} from "
          f"{sum(1 for t in tables if t['edges'])} tables "
          f"({live_sources} of which are hand-wired today)")
    for target, n in sorted(edge_targets.items()):
        print(f"  -> {target[0]}::{target[1]:<22} x{n:<3} {target_status.get(target, 'no-layout')}")
    return reasons


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--json-out")
    args = ap.parse_args()

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "reachability.py")
    ifd_present = IFD_TABLES.is_file()
    instrument.print_header(
        tool="reachability.py",
        git=git,
        dirty_overridden=dirty_overridden,
        extra=[f"reads:   {TABLES.relative_to(ROOT)}, {ALLOWLIST.relative_to(ROOT)} "
               "(committed artifacts, not a fresh dump -- no ExifTool or oxidex involved)"]
              + ([f"         {IFD_TABLES.relative_to(ROOT)}, {IFD_ALLOWLIST.relative_to(ROOT)} "
                  "(slice I-1's IFD-style tables)"] if ifd_present else []),
    )

    tables = parse_tables()
    allowed = parse_allowlist()
    call_sites, dynamic = parse_call_sites()
    enabled, eligible, refused = classify(tables, allowed, call_sites)

    ifd_tables, ifd_enabled, ifd_eligible, ifd_refused = [], [], [], []
    ifd_call_sites, ifd_dynamic = set(), 0
    if ifd_present:
        ifd_tables = parse_tables(IFD_TABLES, "ifd")
        ifd_allowed = parse_allowlist(IFD_ALLOWLIST, "ENABLED_IFD")
        ifd_call_sites, ifd_dynamic = parse_call_sites(
            "find_ifd_table", ("ifd_tables.rs", "enabled_ifd.rs")
        )
        ifd_enabled, ifd_eligible, ifd_refused = classify(ifd_tables, ifd_allowed, ifd_call_sites)

    # Edge targets are resolved across BOTH kinds: `IfdSubdirEdge`'s doc says
    # the edge only says where the pointer leads, and a binary table's edge
    # can name an IFD table just as an IFD table's can name a binary one.
    by_status = {(x["module"], x["table"]): x["status"] for x in tables}
    by_status.update({(x["module"], x["table"]): f"ifd:{x['status']}" for x in ifd_tables})
    binary_status = {k: v for k, v in by_status.items() if not v.startswith("ifd:")}
    binary_status.update({(x["module"], x["table"]): x["status"] for x in tables})

    reasons = report(tables, enabled, eligible, refused, call_sites, dynamic, "find_table",
                     binary_status)

    ifd_reasons = Counter()
    if ifd_present:
        print()
        print("=" * 66)
        print("IFD-style tables (slice I-1): src/exiftool_tables/ifd_tables.rs "
              "vs enabled_ifd.rs")
        print("=" * 66)
        ifd_status = {k: (v[len("ifd:"):] if v.startswith("ifd:") else f"binary:{v}")
                      for k, v in by_status.items()}
        ifd_reasons = report(
            ifd_tables, ifd_enabled, ifd_eligible, ifd_refused, ifd_call_sites, ifd_dynamic,
            "find_ifd_table", ifd_status,
        )
    else:
        print()
        print(f"IFD-style tables (slice I-1): SKIPPED -- {IFD_TABLES.relative_to(ROOT)} does not "
              "exist on this tree (the generator has not produced it)")

    if args.json_out:
        for t in tables:
            t["kind"] = "binary"
        for t in ifd_tables:
            t["kind"] = "ifd"
        pathlib.Path(args.json_out).write_text(
            json.dumps(
                {
                    "tables": len(tables),
                    "enabled": len(enabled),
                    "eligible": len(eligible),
                    "refused": len(refused),
                    "gate_a_refusal_reasons": dict(reasons),
                    "ifd": {
                        "present": ifd_present,
                        "tables": len(ifd_tables),
                        "enabled": len(ifd_enabled),
                        "eligible": len(ifd_eligible),
                        "refused": len(ifd_refused),
                        "gate_a_refusal_reasons": dict(ifd_reasons),
                    },
                    "per_table": tables + ifd_tables,
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
