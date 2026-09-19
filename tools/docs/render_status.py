#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = [
#     "pyyaml>=6.0",
# ]
# ///
"""Render the docs site's autogeneration status page from committed measurements.

The maintainer's complaint was that the docs site did not show where OxiDex
stands on autogeneration, as a percentage or as a count. Every number that
answers that question already exists in a committed measurement, each written
by its own instrument at a named commit. This script reads them and writes

    docs/status/index.md                        the page (route /status/)
    docs/public/measurements/status.json        the same numbers, machine-readable

It never measures anything and never takes a number from a person. Every value
on the page comes from one of these sources, and each one is named beside its
value:

    docs/public/measurements/catalog-corpus-observed-13.59.json   read/write parity
    docs/public/measurements/catalog-hydrated-join-13.59.json     reader states
    tools/ci/parity_floors.json (+ parity_ratchet.py)             CI floors
    tools/exiftool-tables/spike/COVERAGE.md                       expression coverage
    tools/exiftool-tables/spike/SESSION_HELPER_COVERAGE.md        Session + helpers
    src/exiftool_tables/helpers.rs  (PORTS, REFUSED_HELPERS)       helper ports
    tools/exiftool-tables/artifacts.py  (the `paths` inventory)    generated outputs
    oxidex-tags-*/src/*.yaml  (scripts/generate_tag_coverage.py)   tag definitions
    docs/reference/upgrade-rehearsal-11.78-12.64.md                upgrade rehearsal
    docs/public/measurements/generated-share-13.59.json           generated share
    docs/AUTOGENERATION-PLAN.md                                    (its row must agree)

A missing source, a missing key, or a Markdown row that no longer matches its
anchored pattern is an ERROR, never a zero or a blank. A status page that
quietly rendered "0" because a snapshot moved would be the confident wrong
number AGENTS.md warns about.

Output is deterministic: no timestamps, no HEAD commit, sorted JSON. The page
changes only when a source does. `--check` re-renders in memory and fails on
any difference, which is how CI makes a snapshot, floor or report change
without a page refresh fail:

    uv run tools/docs/render_status.py            # rewrite both outputs
    uv run tools/docs/render_status.py --check    # CI: fail if either is stale

Tag definitions are counted with PyYAML (the same `parse_yaml_tags` that
`scripts/sync_tag_stats.py` uses), hence `uv run`. `tools/ci/test_render_status.py`
re-renders everything else under plain `python3` by taking the definition
counts from the committed status.json, so a stale page also fails the lint
job's stdlib unittest step.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PAGE = Path("docs/status/index.md")
STATUS_JSON = Path("docs/public/measurements/status.json")
REFRESH = "uv run tools/docs/render_status.py"

REPO_URL = "https://github.com/swack-tools/oxidex"
BRANCH = "refactor/tag-machinery"

OBSERVED = "docs/public/measurements/catalog-corpus-observed-13.59.json"
JOIN = "docs/public/measurements/catalog-hydrated-join-13.59.json"
FLOORS = "tools/ci/parity_floors.json"
RATCHET = "tools/ci/parity_ratchet.py"
COVERAGE = "tools/exiftool-tables/spike/COVERAGE.md"
SESSION = "tools/exiftool-tables/spike/SESSION_HELPER_COVERAGE.md"
HELPERS_RS = "src/exiftool_tables/helpers.rs"
ARTIFACTS = "tools/exiftool-tables/artifacts.py"
REHEARSAL = "docs/reference/upgrade-rehearsal-11.78-12.64.md"
PLAN = "docs/AUTOGENERATION-PLAN.md"
PROGRESS = "docs/AUTOGENERATION-PROGRESS.md"
GENSHARE = "docs/public/measurements/generated-share-13.59.json"
GENSHARE_TOOL = "tools/exiftool-tables/genshare"
DESIGN = "docs/AUTOGENERATION-V2-DESIGN.md"
PIN = ".exiftool-version"
TAG_COUNTER = "scripts/generate_tag_coverage.py"

# Machine-readable sources are hashed onto the page, so ANY change to them --
# even one that moves no number shown here -- requires a re-render.
HASHED = (OBSERVED, JOIN, FLOORS, GENSHARE)

READER_STATES = (
    ("generated_reader_declaration_unobserved",
     "Generated reader declaration (emits unconditionally)"),
    ("ifd_schema_declaration_eligible_unobserved",
     "Generated IFD schema declaration, eligible"),
    ("generated_reader_declaration_option_gated",
     "Generated reader declaration, behind an ExifTool option"),
    ("ifd_schema_declaration_omitted_unobserved",
     "Generated IFD schema declaration, omitted"),
    ("blocked_generated_reader_refusal",
     "Generated reader explicitly refuses the row"),
    ("ifd_schema_declaration_refused_unobserved",
     "IFD schema declaration refused (layout not representable)"),
    ("source_row_not_yet_consumed",
     "Source row not yet consumed by any generator"),
)
# The plan's "strict" and "loose" generated-declaration counts (3,666 / 5,363
# at 0f92071b) are exactly these sums of the join's reader states.
STRICT = ("generated_reader_declaration_unobserved",
          "ifd_schema_declaration_eligible_unobserved")
LOOSE = STRICT + ("generated_reader_declaration_option_gated",
                  "ifd_schema_declaration_omitted_unobserved",
                  "blocked_generated_reader_refusal")


class SourceError(Exception):
    """A source is missing, or no longer has the shape this page reads."""


# --------------------------------------------------------------------------
# Reading sources
# --------------------------------------------------------------------------

def read_text(root: Path, rel: str) -> str:
    path = root / rel
    if not path.is_file():
        raise SourceError(f"source not found: {rel}")
    return path.read_text(encoding="utf-8")


def read_json(root: Path, rel: str):
    try:
        return json.loads(read_text(root, rel))
    except json.JSONDecodeError as exc:
        raise SourceError(f"{rel} is not valid JSON: {exc}") from exc


def sha256(root: Path, rel: str) -> str:
    path = root / rel
    if not path.is_file():
        raise SourceError(f"source not found: {rel}")
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def get(doc, dotted: str, where: str):
    """Follow a dotted path; a missing key is an error naming the source."""
    node = doc
    for part in dotted.split("."):
        if not isinstance(node, dict) or part not in node:
            raise SourceError(f"{where}: missing key {dotted!r} (stopped at {part!r})")
        node = node[part]
    return node


def count(doc, dotted: str, where: str) -> int:
    value = get(doc, dotted, where)
    if isinstance(value, bool) or not isinstance(value, int):
        raise SourceError(f"{where}: {dotted!r} is {value!r}, expected an integer count")
    return value


def one(pattern: str, text: str, where: str, flags=0):
    """Exactly one regex match, or an error that says the prose moved."""
    matches = list(re.finditer(pattern, text, flags))
    if len(matches) != 1:
        raise SourceError(
            f"{where}: pattern matched {len(matches)} times, expected exactly 1: {pattern}\n"
            f"    The source was probably reworded or regenerated in a new shape. Update "
            f"the pattern in tools/docs/render_status.py; never hard-code the number.")
    return matches[0]


def section(text: str, heading: str, where: str) -> str:
    """The body under one Markdown heading, up to the next heading of any level."""
    match = one(r"^#{1,6} " + re.escape(heading) + r"[ \t]*$", text, where, re.M)
    rest = text[match.end():]
    nxt = re.search(r"^#{1,6} ", rest, re.M)
    return rest[: nxt.start()] if nxt else rest


def table_rows(body: str) -> list[list[str]]:
    """Cells of every row of the FIRST pipe table in `body` (header excluded)."""
    rows, started = [], False
    for line in body.splitlines():
        if line.startswith("|"):
            started = True
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if all(re.fullmatch(r":?-{3,}:?", c) for c in cells):
                continue
            rows.append(cells)
        elif started:
            break
    if len(rows) < 2:
        raise SourceError("expected a Markdown table with a header and at least one row")
    return rows[1:]


def row(rows: list[list[str]], first: str, where: str) -> list[str]:
    hits = [r for r in rows if r and r[0] == first]
    if len(hits) != 1:
        raise SourceError(f"{where}: {len(hits)} table rows start with {first!r}, expected 1")
    return hits[0]


def num(cell: str, where: str) -> int:
    text = cell.replace("*", "").replace(",", "").strip()
    if not re.fullmatch(r"\d+", text):
        raise SourceError(f"{where}: {cell!r} is not an integer")
    return int(text)


def pct(cell: str, where: str) -> str:
    text = cell.replace("*", "").strip()
    if not re.fullmatch(r"\d+(\.\d+)?%", text):
        raise SourceError(f"{where}: {cell!r} is not a percentage")
    return text


def plain(cell: str) -> str:
    return cell.replace("**", "").strip()


def import_file(name: str, path: Path):
    if not path.is_file():
        raise SourceError(f"source not found: {path}")
    # Registered under a private name: dataclasses look their module up in
    # sys.modules, and a private name cannot shadow a real import elsewhere.
    key = f"_render_status_{name}"
    spec = importlib.util.spec_from_file_location(key, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[key] = module
    saved = list(sys.path)
    sys.path.insert(0, str(path.parent))
    try:
        spec.loader.exec_module(module)
    finally:
        sys.path[:] = saved
    return module


def tag_definitions(root: Path) -> dict:
    """16,684 / 931: the same count `scripts/sync_tag_stats.py --check` prints."""
    module = import_file("generate_tag_coverage", root / TAG_COUNTER)
    domains = module.parse_yaml_tags(root)
    return {
        "definitions": sum(d["total_tags"] for d in domains.values()),
        "tables": sum(d["total_tables"] for d in domains.values()),
        "crates": len(domains),
    }


# --------------------------------------------------------------------------
# Collecting the numbers
# --------------------------------------------------------------------------

def ratio(part: int, whole: int) -> str:
    if whole <= 0:
        raise SourceError(f"denominator {whole} is not positive")
    return f"{100.0 * part / whole:.2f}%"


def collect_catalog_and_parity(root, observed, join, definitions):
    oj = get(observed, "observed_join", OBSERVED)
    oc = get(oj, "counts", OBSERVED)
    jc = get(join, "counts", JOIN)
    pin = read_text(root, PIN).strip()
    version = get(oj, "inputs.exiftool_version", OBSERVED)
    if version != pin or get(join, "inputs.exiftool_version", JOIN) != pin:
        raise SourceError(f"{PIN} pins {pin}, but the published measurements are for "
                          f"{version}; re-publish the snapshots before rendering status")

    catalog_entries = count(oc, "catalog_ordinary_entries", OBSERVED)
    if count(jc, "catalog_ordinary_entries", JOIN) != catalog_entries:
        raise SourceError("the join and the observed snapshot disagree on the catalog size")
    writable = count(oc, "native_writable.writable", OBSERVED)

    reads = {k: count(oc, f"observed_read.{k}", OBSERVED)
             for k in ("observed_matched_read", "native_read_not_matched", "not_observed_yet")}
    if sum(reads.values()) != catalog_entries:
        raise SourceError(f"{OBSERVED}: the three observed_read buckets sum to "
                          f"{sum(reads.values())}, not the catalog's {catalog_entries}")
    attribution = get(oc, "corpus_read_attribution", OBSERVED)
    metric_c = get(attribution, "metric_c", OBSERVED)
    corpus_ev = get(observed, "native_evidence.axes.corpus_read_evidence", OBSERVED)
    writer_ev = get(observed, "native_evidence.axes.writer_read_evidence", OBSERVED)

    readback = get(oc, "write_readback", OBSERVED)
    return {
        "exiftool": pin,
        "catalog": {
            "catalog_entries": catalog_entries,
            "hydrated_source_rows": count(oc, "hydrated_source_rows", OBSERVED),
            "native_writable": {k: count(oc, f"native_writable.{k}", OBSERVED)
                                for k in ("writable", "writable_protected",
                                          "not_writable", "not_listed")},
            "native_writable_unique_names": count(
                oc, "write_parity.native_writable_unique_case_insensitive_names", OBSERVED),
            "tag_definitions": definitions["definitions"],
            "tag_tables": definitions["tables"],
            "tag_crates": definitions["crates"],
        },
        "read": {
            **reads,
            "share_of_catalog": ratio(reads["observed_matched_read"], catalog_entries),
            "native_catalog_entries": count(attribution, "native_catalog_entries", OBSERVED),
            "credited_catalog_entries": count(attribution, "credited_catalog_entries", OBSERVED),
            "share_of_native_reads": ratio(
                count(attribution, "credited_catalog_entries", OBSERVED),
                count(attribution, "native_catalog_entries", OBSERVED)),
            "corpus_files": count(metric_c, "corpus_files", OBSERVED),
            "identities_matched": count(metric_c, "distinct_group1_tag_identities_matched", OBSERVED),
            "identities_native": count(metric_c, "distinct_group1_tag_identities_native", OBSERVED),
            "identities_print_mode_matched": count(
                metric_c, "distinct_group1_tag_identities_print_mode_matched", OBSERVED),
            "credited_source_coordinates": count(metric_c, "credited_source_coordinates", OBSERVED),
            "withheld_source_coordinates": count(metric_c, "withheld_source_coordinates", OBSERVED),
            "credited_coordinates_outside_catalog": count(
                attribution, "credited_coordinates_outside_catalog", OBSERVED),
            "instrument": "corpus_read_receipt.py",
            "schema": get(corpus_ev, "schema", OBSERVED),
            "commit": get(corpus_ev, "producer.source_commit", OBSERVED),
            "dirty": get(corpus_ev, "producer.source_dirty", OBSERVED),
        },
        "write": {
            "observed_matched_write": count(oc, "observed_write.observed_matched_write", OBSERVED),
            "writable": writable,
            "share_of_writable": ratio(
                count(oc, "observed_write.observed_matched_write", OBSERVED), writable),
            "generated_writer_declarations": count(
                oc, "write_parity.generated_writer_declarations", OBSERVED),
            "successful_write_operations": count(readback, "successful_write_operations", OBSERVED),
            "catalog_matched_write_operations": count(
                readback, "catalog_matched_write_operations", OBSERVED),
            "alternate_context_write_operations": count(
                readback, "alternate_context_write_operations", OBSERVED),
            "distinct_group1_names": count(readback, "distinct_group1_names", OBSERVED),
            "instrument": "write_readback_evidence.py",
            "schema": get(writer_ev, "schema", OBSERVED),
            "commit": get(writer_ev, "producer.source_commit", OBSERVED),
            "dirty": get(writer_ev, "producer.source_dirty", OBSERVED),
        },
    }


def collect_reader_states(observed, join):
    jc = get(join, "counts", JOIN)
    states = {key: count(jc, f"reader_implementation.{key}", JOIN) for key, _ in READER_STATES}
    total = count(jc, "catalog_ordinary_entries", JOIN)
    known = {key for key, _ in READER_STATES}
    extra = set(get(jc, "reader_implementation", JOIN)) - known
    if extra:
        raise SourceError(f"{JOIN}: new reader states {sorted(extra)} -- give each a "
                          f"label in READER_STATES before rendering")
    if sum(states.values()) != total:
        raise SourceError(f"{JOIN}: reader states sum to {sum(states.values())}, not {total}")

    proven = {key: 0 for key, _ in READER_STATES}
    for entry in get(observed, "observed_join.entries", OBSERVED):
        if entry.get("observed_read") == "observed_matched_read":
            state = entry.get("reader_implementation")
            if state not in proven:
                raise SourceError(f"{OBSERVED}: proven read on unknown reader state {state!r}")
            proven[state] += 1
    strict = sum(states[k] for k in STRICT)
    loose = sum(states[k] for k in LOOSE)
    return {
        "states": states,
        "proven_reads_by_state": proven,
        "strict": strict,
        "strict_share": ratio(strict, total),
        "loose": loose,
        "loose_share": ratio(loose, total),
        "catalog_entries": total,
    }


def repo_commit(text: str, where: str) -> str:
    return one(r"repo commit: `[^`]*` \(`([0-9a-f]{40})`\)", text, where).group(1)


def ladder(text, heading, where, first_cells):
    rows = table_rows(section(text, heading, where))
    out = {}
    for key, first in first_cells.items():
        cells = row(rows, first, f"{where} / {heading}")
        out[key] = {"uses": num(cells[1], where), "uses_pct": pct(cells[2], where),
                    "distinct": num(cells[3], where), "distinct_pct": pct(cells[4], where)}
    denominator = one(r"denominator: \*\*(\d+) uses / (\d+) distinct",
                      section(text, heading, where), f"{where} / {heading}")
    return {"uses_total": int(denominator.group(1)),
            "distinct_total": int(denominator.group(2)), "rungs": out}


def collect_expressions(root):
    cov = read_text(root, COVERAGE)
    ses = read_text(root, SESSION)

    census = section(cov, "5. Which helper subs, and whether oxidex already ports them", COVERAGE)
    helper_subs = int(one(r"distinct helper subs called across the whole dump: \*\*(\d+)\*\*",
                          census, COVERAGE).group(1))
    pure = int(one(r"pure functions of their arguments \([^)]*\): \*\*(\d+)\*\*",
                   census, COVERAGE).group(1))

    spike_rungs = {
        "today": "a. exprs.py/conds.py today (accepts)",
        "parseable": "b. parseable by the spike grammar",
        "pure": "c. evaluable PURE ($val + operators + core builtins)",
        "top25": "e. + all session keys + top 25 helpers",
    }
    spike = {
        "commit": repo_commit(cov, COVERAGE),
        "frame_a": ladder(cov, "2. Coverage ladder -- Frame A (`expr_coverage.py`'s own denominator)",
                          COVERAGE, spike_rungs),
        "frame_b": ladder(cov, "3. Coverage ladder -- Frame B (every expression in the dump)",
                          COVERAGE, spike_rungs),
    }

    session_rungs = {
        "today": "exprs.py/conds.py today, no Session (COVERAGE.md rung a)",
        "parseable": "parseable by the spike grammar",
        "typed_after": "typed Session, after: + v2 ports incl. partial",
        "member_after": "Session + member map, after: + v2 ports incl. partial",
    }
    frames = {
        "frame_a": "Frame A -- expr_coverage.py's denominator (COVERAGE.md Frame A)",
        "frame_b": "Frame B -- every expression in the dump (COVERAGE.md Frame B)",
        "frame_m": "Frame M -- Exif::Main, every slot incl. `_variants` and `*Inv`",
        "frame_mr": "Frame Mr -- Exif::Main read side (M without `ValueConvInv`/`PrintConvInv`)",
    }
    measured_ports = int(one(r"^- v2 ports \((\d+), `helpers\.rs` PORTS\)", ses, SESSION,
                             re.M).group(1))
    session = {"commit": repo_commit(ses, SESSION), "measured_with_ports": measured_ports}
    for key, heading in frames.items():
        session[key] = ladder(ses, heading, SESSION, session_rungs)

    rs = read_text(root, HELPERS_RS)
    ports_block = one(r"^pub const PORTS: &\[HelperPort\] = &\[\n(.*?)^\];", rs, HELPERS_RS,
                      re.M | re.S).group(1)
    ports = re.findall(r'^\s*perl: "([^"]+)",', ports_block, re.M)
    refused_block = one(r"^pub const REFUSED_HELPERS: &\[\(&str, &str\)\] = &\[\n(.*?)^\];",
                        rs, HELPERS_RS, re.M | re.S).group(1)
    refused = re.findall(r'^\s*"(Image::ExifTool::[^"]+)",', refused_block, re.M)
    if not ports:
        raise SourceError(f"{HELPERS_RS}: PORTS lists no helpers")
    return {
        "spike": spike,
        "session": session,
        "helper_subs": helper_subs,
        "helper_subs_pure": pure,
        "ports": ports,
        "refused": refused,
    }


def collect_artifacts(root):
    module = import_file("artifacts", root / ARTIFACTS)
    module.validate()
    items = list(module.select("all", "all", None))
    if not items:
        raise SourceError(f"{ARTIFACTS}: the inventory lists no outputs")
    tiers: dict[str, int] = {}
    rust = 0
    for item in items:
        tiers[str(item.tier)] = tiers.get(str(item.tier), 0) + 1
        rust += item.path.endswith(".rs")
    return {"total": len(items), "by_tier": dict(sorted(tiers.items())), "rust": rust}


def collect_generated_share(root):
    """The probe census result committed by tools/exiftool-tables/genshare.

    The plan quotes the same figure; its row must name this file's share and
    commit, so the plan and the page cannot drift apart.
    """
    doc = read_json(root, GENSHARE)
    cur = get(doc, "current", GENSHARE)
    commit = get(doc, "commit", GENSHARE)
    if get(cur, "commit", GENSHARE) != commit:
        raise SourceError(f"{GENSHARE}: `commit` and `current.commit` disagree")
    share = get(cur, "all_generated.share", GENSHARE)
    plan = read_text(root, PLAN)
    line = one(r"^\| Generated share of correct output \|.*$", plan, PLAN, re.M).group(0)
    if f"**{share}**" not in line or f"`{commit[:8]}`" not in line:
        raise SourceError(f"{PLAN}: the generated-share row does not quote {GENSHARE} "
                          f"({share} at {commit[:8]}); update the row from the committed result")
    if "**Stale**" in line:
        raise SourceError(f"{PLAN}: the generated-share row still says **Stale** although "
                          f"{GENSHARE} is the current measurement")
    points = []
    for p in get(doc, "points", GENSHARE):
        points.append({
            "commit": get(p, "commit", GENSHARE),
            "label": p.get("label", ""),
            "control_matched": count(p, "control_matched", GENSHARE),
            "share": get(p, "all_generated.share", GENSHARE),
            "engine_alone": get(p, "engine_alone.share", GENSHARE),
            "direct": get(p, "direct.share", GENSHARE),
            "composite_cascade": get(p, "composite_cascade.share", GENSHARE),
        })
    return {
        "share": share,
        "rows": count(cur, "all_generated.rows", GENSHARE),
        "matched_values": count(cur, "control_matched", GENSHARE),
        "engine_alone": get(cur, "engine_alone.share", GENSHARE),
        "direct": get(cur, "direct.share", GENSHARE),
        "composite_cascade": get(cur, "composite_cascade.share", GENSHARE),
        "other_cascade": get(cur, "other_cascade.share", GENSHARE),
        "commit": commit,
        "date": get(doc, "date", GENSHARE),
        "method_version": get(doc, "method_version", GENSHARE),
        "bound": get(doc, "instrument.bound", GENSHARE),
        "corpus_files": count(doc, "corpus.files", GENSHARE),
        "delta_from": get(doc, "delta_vs_first.from", GENSHARE),
        "delta_pts": get(doc, "delta_vs_first.all_generated_pts", GENSHARE),
        "engine_delta_pts": get(doc, "delta_vs_first.engine_alone_pts", GENSHARE),
        "points": points,
        "stale": False,
    }


def collect_rehearsal(root):
    text = read_text(root, REHEARSAL)
    head = one(r"Run on (\d{4}-\d{2}-\d{2}) from `refactor/tag-machinery` at `([0-9a-f]+)`",
               text, REHEARSAL)
    result = one(r"^\*\*Result: ([^*]+)\*\*", text, REHEARSAL, re.M).group(1).strip()
    releases = {}
    for release in ("11.78", "12.64"):
        stages = []
        for cells in table_rows(section(text, release, REHEARSAL)):
            if len(cells) != 4:
                raise SourceError(f"{REHEARSAL} / {release}: stage row has {len(cells)} cells")
            stages.append({"stage": plain(cells[0]), "result": plain(cells[1]),
                           "note": plain(cells[3])})
        releases[release] = stages
    if [s["stage"] for s in releases["11.78"]] != [s["stage"] for s in releases["12.64"]]:
        raise SourceError(f"{REHEARSAL}: the two releases list different stages")

    conf_rows = table_rows(section(text, "Read conformance", REHEARSAL))
    conformance = []
    for first in ("13.59 control vs 13.59", "12.64 build vs 12.64", "11.78 build vs 11.78"):
        cells = row(conf_rows, first, REHEARSAL)
        conformance.append({"build": first, "match": num(cells[1], REHEARSAL),
                            "missing": num(cells[3], REHEARSAL),
                            "extra": num(cells[5], REHEARSAL),
                            "ratio": pct(cells[6], REHEARSAL)})
    number = r"(\d{1,3}(?:,\d{3})*)"
    write = one(rf"Write matrix \*\*{number}/{number}\*\*", text, REHEARSAL)
    readback = one(rf"Readback matrix {number}/{number}", text, REHEARSAL)
    interventions = one(r"\*\*Code interventions: (\d+) per release \(target (\d+)\)\.\*\*",
                        text, REHEARSAL)
    return {
        "date": head.group(1),
        "commit": head.group(2),
        "result": result,
        "stages": releases,
        "conformance": conformance,
        "control_write_matrix": f"{write.group(1)}/{write.group(2)}",
        "control_readback_matrix": f"{readback.group(1)}/{readback.group(2)}",
        "code_interventions_per_release": int(interventions.group(1)),
        "code_interventions_target": int(interventions.group(2)),
    }


def collect_floors(root, observed, join):
    ratchet = import_file("parity_ratchet", root / RATCHET)
    floors = read_json(root, FLOORS)
    for key in ("sources", "metrics", "measured_at"):
        if key not in floors:
            raise SourceError(f"{FLOORS}: no {key!r}")
    loaded = {OBSERVED: observed, JOIN: join}
    sources = {}
    for name, spec in floors["sources"].items():
        path, _, inner = spec.partition("#")
        if path not in loaded:
            raise SourceError(f"{FLOORS}: source {name!r} reads {path}, which this page does "
                              f"not load; add it to render_status.py")
        doc = loaded[path]
        sources[name] = get(doc, inner, path) if inner else doc
    metrics = []
    for name, spec in sorted(floors["metrics"].items()):
        doc, path = ratchet.split_metric(name, sources)
        found, value = ratchet.dig(doc, path)
        if not found:
            raise SourceError(f"{FLOORS}: tracked metric {name} is absent from its source")
        reason = ratchet.compare(spec["floor"], value, spec["direction"])
        metrics.append({"metric": name, "direction": spec["direction"],
                        "floor": spec["floor"], "current": value,
                        "holds": reason is None})
    return {
        "measured_at": floors["measured_at"].get("commit", ""),
        "note": floors["measured_at"].get("note", ""),
        "metrics": metrics,
    }


def collect(root: Path, definitions: dict | None = None) -> dict:
    observed = read_json(root, OBSERVED)
    join = read_json(root, JOIN)
    for rel in (PLAN, PROGRESS, DESIGN):
        read_text(root, rel)  # linked from the page: must exist
    if definitions is None:
        definitions = tag_definitions(root)
    status = {
        "schema": "oxidex_docs_status_v1",
        "generated_by": "tools/docs/render_status.py",
        "refresh": REFRESH,
        "sources": {rel: sha256(root, rel) for rel in HASHED},
    }
    status.update(collect_catalog_and_parity(root, observed, join, definitions))
    status["generation"] = {
        "reader": collect_reader_states(observed, join),
        "expressions": collect_expressions(root),
        "artifacts": collect_artifacts(root),
    }
    status["generated_share"] = collect_generated_share(root)
    status["rehearsal"] = collect_rehearsal(root)
    status["floors"] = collect_floors(root, observed, join)
    return status


# --------------------------------------------------------------------------
# Rendering
# --------------------------------------------------------------------------

def n(value: int) -> str:
    return f"{value:,}"


def commit_link(sha: str) -> str:
    return f"[`{sha[:8]}`]({REPO_URL}/commit/{sha})"


def src(rel: str, label: str | None = None) -> str:
    return f"[`{label or rel}`]({REPO_URL}/blob/{BRANCH}/{rel})"


def meter(label, value, total, note, stale=False):
    """One <StatusMeter>; `value`/`total` are the source's own counts."""
    for text in (label, note):
        if '"' in text:
            raise SourceError(f"meter text may not contain a double quote: {text!r}")
    attrs = [f'label="{label}"', f':value="{value}"', f':total="{total}"', f'note="{note}"']
    if stale:
        attrs.append("stale")
    return "<StatusMeter " + " ".join(attrs) + " />"


def meters(*items) -> str:
    """Meters as ONE html block: no blank line may split it into paragraphs."""
    return "\n".join(['<div class="status-meters">', *items, "</div>"])


def table(header, rows):
    """A pipe table. A header ending in a space is a right-aligned (numeric) column."""
    out = ["| " + " | ".join(h.strip() for h in header) + " |",
           "| " + " | ".join("---:" if h.endswith(" ") else "---" for h in header) + " |"]
    for r in rows:
        if len(r) != len(header):
            raise SourceError(f"table row has {len(r)} cells for {len(header)} columns")
        out.append("| " + " | ".join(str(c) for c in r) + " |")
    return "\n".join(out)


def render_markdown(s: dict) -> str:
    cat, rd, wr = s["catalog"], s["read"], s["write"]
    gen = s["generation"]
    rdr, ex, art = gen["reader"], gen["expressions"], gen["artifacts"]
    gs, rh, fl = s["generated_share"], s["rehearsal"], s["floors"]
    et = s["exiftool"]
    sp, se = ex["spike"], ex["session"]
    obs_src = src(OBSERVED, "catalog-corpus-observed-" + et + ".json")
    join_src = src(JOIN, "catalog-hydrated-join-" + et + ".json")
    today_b = se["frame_b"]["rungs"]["today"]
    after_b = se["frame_b"]["rungs"]["member_after"]
    L = []
    w = L.append

    w("---")
    w("title: Autogeneration status")
    w("description: Where OxiDex stands on ExifTool tag parity and autogeneration, "
      "rendered from committed measurements.")
    w("outline: [2, 3]")
    w("---")
    w("")
    w("<script setup>")
    w("import StatusMeter from './StatusMeter.vue'")
    w("</script>")
    w("")
    w("<!-- GENERATED by tools/docs/render_status.py. Do not edit by hand: "
      f"change a source and run `{REFRESH}`. CI fails when this page is stale. -->")
    w("")
    w("# Autogeneration status")
    w("")
    w(f"How much of ExifTool {et} OxiDex reads and writes correctly, and how much of "
      "that comes from code generated from ExifTool's own tables rather than typed in "
      "by hand. This page is generated from committed measurement files. It "
      "measures nothing itself, and no number on it was typed by hand. Each figure "
      "names the instrument that produced it and the commit it was measured at. CI "
      "re-renders the page and fails if any source changed without a refresh.")
    w("")
    w("Where these numbers are going, and the rules for what counts as progress: "
      "[Autogeneration plan](/AUTOGENERATION-PLAN) "
      "(progress log: [Autogeneration progress](/AUTOGENERATION-PROGRESS); "
      "mechanism: [v2 design](/AUTOGENERATION-V2-DESIGN)).")
    w("")
    w("## At a glance")
    w("")
    share_pct = float(gs["share"].rstrip("%"))
    w(meters(
        meter("Read parity: catalog entries proven", rd["observed_matched_read"],
              cat["catalog_entries"], f"observed_matched_read / ExifTool {et} catalog entries"),
        meter("Of what ExifTool reads in the corpus", rd["credited_catalog_entries"],
              rd["native_catalog_entries"],
              "credited catalog entries / entries ExifTool reads in the corpus"),
        meter("Write parity: writable entries proven", wr["observed_matched_write"],
              wr["writable"], "observed_matched_write / writable catalog entries"),
        meter("Catalog entries with a generated reader (strict)", rdr["strict"],
              rdr["catalog_entries"], "unconditional generated declarations / catalog entries"),
        meter("Expression uses today's translators accept", today_b["uses"],
              se["frame_b"]["uses_total"], "exprs.py and conds.py, every expression in the dump"),
        meter("Reachable with Session + ported helpers", after_b["uses"],
              se["frame_b"]["uses_total"],
              f"with the {se['measured_with_ports']} v2 helper ports, same denominator"),
        meter("Helper subs ported", len(ex["ports"]), ex["helper_subs"],
              "helpers.rs PORTS / distinct helper subs the tables call"),
        meter("Generated share of correct output (a floor)", share_pct, 100,
              f"probe census at {gs['commit'][:8]} on {gs['date']}; engine alone "
              f"{gs['engine_alone']}"),
    ))
    w("")

    # -- 1. catalog --------------------------------------------------------
    w("## Catalog size")
    w("")
    w("Three different counts describe \"how many tags\". They measure different "
      "things and should not be compared with each other.")
    w("")
    w(table(["What", "Count ", "What it counts", "Source"], [
        [f"ExifTool {et} catalog entries", f"**{n(cat['catalog_entries'])}**",
         "Every ordinary tag entry ExifTool's own tag lookup lists, one per table "
         "coordinate. This is the read-parity denominator.",
         f"{obs_src} `counts.catalog_ordinary_entries`"],
        ["Writable catalog entries", f"**{n(cat['native_writable']['writable'])}**",
         f"The entries ExifTool can write ({n(cat['native_writable_unique_names'])} "
         "distinct names, case-insensitive). This is the write-parity denominator.",
         f"{obs_src} `counts.native_writable.writable`"],
        ["Tag definitions in `oxidex-tags-*`", f"**{n(cat['tag_definitions'])}**",
         f"Tag definitions in {n(cat['tag_tables'])} tables across {cat['tag_crates']} "
         "`oxidex-tags-*` YAML databases. Knowing a tag exists is not the same as "
         "extracting it, so this is **not** a coverage figure.",
         src(TAG_COUNTER, "parse_yaml_tags") + " (as `scripts/sync_tag_stats.py --check`)"],
    ]))
    w("")
    nw = cat["native_writable"]
    w(f"Writability of the {n(cat['catalog_entries'])} catalog entries: "
      f"{n(nw['writable'])} writable, {n(nw['writable_protected'])} writable but "
      f"protected, {n(nw['not_writable'])} read-only, {n(nw['not_listed'])} not listed. "
      f"The {n(cat['hydrated_source_rows'])} hydrated source rows are the ExifTool "
      "table rows the catalog was joined to.")
    w("")

    # -- 2. read parity ----------------------------------------------------
    w("## Read parity")
    w("")
    w(f"Instrument: `{rd['instrument']}` (receipt schema `{rd['schema']}`), "
      f"{n(rd['corpus_files'])} corpus files, run against pinned ExifTool {et} at "
      f"{commit_link(rd['commit'])}{' (dirty tree)' if rd['dirty'] else ', clean tree'}. "
      f"Published in {obs_src} (`observed_join.counts`).")
    w("")
    w("Every catalog entry is in exactly one of three buckets:")
    w("")
    w(table(["Bucket", "Entries ", "Share ", "Meaning"], [
        ["`observed_matched_read`", n(rd["observed_matched_read"]),
         ratio(rd["observed_matched_read"], cat["catalog_entries"]),
         "ExifTool reads it in the corpus and OxiDex's value matches"],
        ["`native_read_not_matched`", n(rd["native_read_not_matched"]),
         ratio(rd["native_read_not_matched"], cat["catalog_entries"]),
         "ExifTool reads it in the corpus and OxiDex does not match: **OxiDex gap**"],
        ["`not_observed_yet`", n(rd["not_observed_yet"]),
         ratio(rd["not_observed_yet"], cat["catalog_entries"]),
         "No corpus file exercises it: **corpus gap**. This says nothing either way "
         "about OxiDex"],
        ["**Total**", f"**{n(cat['catalog_entries'])}**", "100.00%", "catalog entries"],
    ]))
    w("")
    w(table(["Corpus measure", "Value ", "Denominator "], [
        ["Share of what ExifTool reads in the corpus",
         f"**{rd['share_of_native_reads']}** ({n(rd['credited_catalog_entries'])})",
         f"{n(rd['native_catalog_entries'])} catalog entries ExifTool reads"],
        ["`Group1:TagName` identities matched (print and raw modes)",
         n(rd["identities_matched"]), f"{n(rd['identities_native'])} native identities"],
        ["Identities matched in print mode", n(rd["identities_print_mode_matched"]),
         f"{n(rd['identities_native'])} native identities"],
        ["Credited source coordinates", n(rd["credited_source_coordinates"]),
         f"{n(rd['credited_coordinates_outside_catalog'])} of them outside the catalog; "
         f"{n(rd['withheld_source_coordinates'])} withheld"],
    ]))
    w("")

    # -- 3. write parity ---------------------------------------------------
    w("## Write parity")
    w("")
    w(f"Instrument: `{wr['instrument']}` over the public-API write matrix "
      f"(evidence schema `{wr['schema']}`) at "
      f"{commit_link(wr['commit'])}{' (dirty tree)' if wr['dirty'] else ', clean tree'}, "
      f"joined into {obs_src}.")
    w("")
    w(table(["Measure", "Value "], [
        ["`observed_matched_write` / writable entries",
         f"**{n(wr['observed_matched_write'])} / {n(wr['writable'])}** ({wr['share_of_writable']})"],
        ["Generated writer declarations", n(wr["generated_writer_declarations"])],
        ["Public-API write operations read back successfully",
         n(wr["successful_write_operations"])],
        ["... at a catalog coordinate / in an alternate context",
         f"{n(wr['catalog_matched_write_operations'])} / "
         f"{n(wr['alternate_context_write_operations'])}"],
        ["Distinct `Group1` names written", n(wr["distinct_group1_names"])],
        [f"Public-API write matrix, {et} control ({commit_link(rh['commit'])})",
         f"{rh['control_write_matrix']} rows; readback {rh['control_readback_matrix']}"],
    ]))
    w("")
    w(f"The write-matrix rows come from the {et} control run in the "
      "[upgrade rehearsal](/reference/upgrade-rehearsal-11.78-12.64).")
    w("")

    # -- 4. generation -----------------------------------------------------
    w("## Generation")
    w("")
    w("### Reader declarations")
    w("")
    w(f"How each of the {n(rdr['catalog_entries'])} catalog entries is implemented on the "
      f"read side today. Source: {join_src} `counts.reader_implementation`. The "
      "proven-reads column counts `observed_matched_read` entries in each state, from "
      f"{obs_src}.")
    w("")
    w(table(["Reader state", "Entries ", "Share ", "Proven reads "], [
        [f"{label} (`{key}`)", n(rdr["states"][key]),
         ratio(rdr["states"][key], rdr["catalog_entries"]),
         n(rdr["proven_reads_by_state"][key])]
        for key, label in READER_STATES
    ]))
    w("")
    w(f"- **Strict** (a generated declaration that emits unconditionally, the first two "
      f"rows): **{n(rdr['strict'])}** entries ({rdr['strict_share']}).")
    w(f"- **Loose** (any generated declaration, including option-gated, omitted and "
      f"refusing ones, the first five rows): **{n(rdr['loose'])}** entries "
      f"({rdr['loose_share']}).")
    no_decl = (rdr["proven_reads_by_state"]["source_row_not_yet_consumed"]
               + rdr["proven_reads_by_state"]["ifd_schema_declaration_refused_unobserved"])
    w(f"- {n(no_decl)} of the {n(rd['observed_matched_read'])} proven reads "
      f"({ratio(no_decl, rd['observed_matched_read'])}) are on entries with no usable "
      "generated declaration, so hand parsers produce them. Reads and generation are "
      "still largely independent.")
    w("")

    w("### Expression coverage")
    w("")
    w("ExifTool's tables embed Perl expressions (`Condition`, `RawConv`, `ValueConv`, "
      "`PrintConv` and their inverses). A *use* is one table field that references one. "
      "These are dependency ceilings measured over the pinned dump, not evaluations. "
      "The *today* rung is what the committed translators accept. The last rung is what "
      "the v2 `Session`, its member map and the helper ports reach.")
    w("")
    w(f"Instrument: `session_helper_coverage.py` at {commit_link(se['commit'])} "
      f"({src(SESSION, 'SESSION_HELPER_COVERAGE.md')}), measured with "
      f"{se['measured_with_ports']} helper ports.")
    w("")
    frame_labels = [("frame_a", "A: `expr_coverage.py`'s denominator"),
                    ("frame_b", "B: every expression"),
                    ("frame_m", "M: `Exif::Main`, all slots"),
                    ("frame_mr", "Mr: `Exif::Main`, read side")]
    w(table(["Frame", "Uses ", "Today ", "Parseable ", "Session + helpers "], [
        [label, n(se[key]["uses_total"]),
         se[key]["rungs"]["today"]["uses_pct"],
         se[key]["rungs"]["parseable"]["uses_pct"],
         f"**{se[key]['rungs']['member_after']['uses_pct']}**"]
        for key, label in frame_labels
    ]))
    w("")
    fa, fb = sp["frame_a"]["rungs"], sp["frame_b"]["rungs"]
    w(f"From the coverage spike (`run_spike.py` at {commit_link(sp['commit'])}, "
      f"{src(COVERAGE, 'COVERAGE.md')}): a grammar alone parses "
      f"{fb['parseable']['uses_pct']} of all uses, but an interpreter with no helpers "
      f"evaluates only {fa['pure']['uses_pct']} of Frame A, less than today's "
      f"{fa['today']['uses_pct']}. The work is in the helpers and the session. With all "
      f"session keys and the top 25 helpers, Frame B reaches {fb['top25']['uses_pct']}.")
    w("")

    w("### Helper ports")
    w("")
    w(f"**{len(ex['ports'])}** of the **{ex['helper_subs']}** distinct helper subs the "
      f"tables call ({ex['helper_subs_pure']} of them pure functions of their arguments) "
      f"are ported and proven byte-identical against the pinned Perl. Source: "
      f"{src(HELPERS_RS, 'helpers.rs')} `PORTS`.")
    w("")
    if len(ex["ports"]) != se["measured_with_ports"]:
        w(f"::: warning Coverage is behind the ports")
        w(f"`PORTS` now lists {len(ex['ports'])} helpers, but the coverage above was "
          f"measured with {se['measured_with_ports']}. Re-run "
          "`session_helper_coverage.py` to refresh it.")
        w(":::")
        w("")
    w("<details><summary>Ported helpers (" + str(len(ex["ports"])) + ")</summary>")
    w("")
    for perl in ex["ports"]:
        w(f"- `{perl.replace('Image::ExifTool::', '')}`")
    w("")
    w("</details>")
    w("")
    w(f"Explicitly refused, with a recorded reason ({len(ex['refused'])}): "
      + ", ".join(f"`{p.replace('Image::ExifTool::', '')}`" for p in ex["refused"]) + ".")
    w("")

    w("### Generated artifacts")
    w("")
    tiers = ", ".join(f"tier {t}: {n(c)}" for t, c in art["by_tier"].items())
    w(f"**{n(art['total'])}** files are regenerated from ExifTool's source ({tiers}; "
      f"{n(art['rust'])} of them Rust). Source: the output inventory in "
      f"{src(ARTIFACTS, 'artifacts.py')}, the same list `artifacts.py paths` prints.")
    w("")

    # -- 5. generated share ------------------------------------------------
    w("## Generated share of correct output")
    w("")
    w(f"**{gs['share']}** of {n(gs['matched_values'])} matched values came from generated "
      f"code at {commit_link(gs['commit'])}, measured on {gs['date']} over "
      f"{n(gs['corpus_files'])} corpus files ({gs['direct']} directly, {gs['composite_cascade']} "
      f"as Composite values computed from generated rows, {gs['other_cascade']} other cascade). "
      f"The generic table engine alone accounts for {gs['engine_alone']}. That is "
      f"{gs['delta_pts']:+.2f} points since `{gs['delta_from'][:8]}` "
      f"(engine alone {gs['engine_delta_pts']:+.2f}).")
    w("")
    w(f"The figure is a **{gs['bound']}**. A probe build drops the rows each generated route "
      "emits, and a pinned-oracle census counts the correct rows lost. A row that hand code "
      "also writes under the same key survives the probe and counts as hand. Generated "
      "conversion lookups inside hand walkers are not counted at all. Method "
      f"`{gs['method_version']}`; instrument, probe patch and caveats: "
      f"{src(GENSHARE_TOOL + '/README.md', 'genshare/README.md')}; result: "
      f"{src(GENSHARE, 'generated-share-13.59.json')}.")
    w("")
    w(table(["Commit", "What", "Share ", "Direct ", "Composite cascade ", "Engine alone ",
             "Matched values "], [
        [commit_link(p["commit"]), p["label"], p["share"], p["direct"],
         p["composite_cascade"], p["engine_alone"], n(p["control_matched"])]
        for p in gs["points"]
    ]))
    w("")

    # -- 6. rehearsal ------------------------------------------------------
    w("## Upgrade rehearsal")
    w("")
    w(f"Can a different ExifTool release be dropped in and regenerated? The rehearsal ran on "
      f"{rh['date']} at {commit_link(rh['commit'])}. See "
      "[upgrade rehearsal 11.78 / 12.64](/reference/upgrade-rehearsal-11.78-12.64) for the "
      "full record.")
    w("")
    w(f"**Result: {rh['result']}** Code interventions needed: "
      f"**{rh['code_interventions_per_release']} per release** "
      f"(target {rh['code_interventions_target']}).")
    w("")
    stages = zip(rh["stages"]["11.78"], rh["stages"]["12.64"])
    w(table(["Stage", "11.78", "12.64"],
            [[a["stage"], a["result"], b["result"]] for a, b in stages]))
    w("")
    w(table(["Read conformance (vs that release's own ExifTool)", "MATCH ", "MISSING ",
             "EXTRA ", "Ratio "], [
        [c["build"], n(c["match"]), n(c["missing"]), n(c["extra"]), c["ratio"]]
        for c in rh["conformance"]
    ]))
    w("")

    # -- 7. floors ---------------------------------------------------------
    w("## What CI guarantees")
    w("")
    w(f"`{RATCHET}` holds {len(fl['metrics'])} parity floors in {src(FLOORS, 'parity_floors.json')} "
      f"(last raised at `{fl['measured_at']}`). *at least* and *at most* may only move "
      "in the improving direction. *exact* marks a denominator that must not change "
      "without re-reading every ratio. Moving a floor backwards needs an explicit, "
      "reviewed `parity_ratchet.py raise`.")
    w("")
    w(table(["Metric", "Rule", "Floor ", "Now ", ""], [
        [f"`{m['metric']}`", m["direction"].replace("_", " "), n(m["floor"]),
         n(m["current"]), "holds" if m["holds"] else "**BROKEN**"]
        for m in fl["metrics"]
    ]))
    w("")

    # -- sources -----------------------------------------------------------
    w("## Sources and refresh")
    w("")
    w("The machine-readable form of this page is "
      "[`/measurements/status.json`](/measurements/status.json). The committed "
      "measurements it was rendered from are listed below. Any change to them fails CI "
      "until the page is refreshed:")
    w("")
    w(table(["Source", "sha256"], [[src(rel), f"`{digest[:16]}`"]
                                    for rel, digest in sorted(s["sources"].items())]))
    w("")
    w(f"Also read: {src(COVERAGE)}, {src(SESSION)}, {src(HELPERS_RS)}, {src(ARTIFACTS)}, "
      f"{src(REHEARSAL)}, {src(PLAN)}, and the `oxidex-tags-*` YAML databases.")
    w("")
    w(f"Refresh with `{REFRESH}`. CI runs `{REFRESH} --check` and "
      "`tools/ci/test_render_status.py`.")
    w("")
    return "\n".join(L)


def render_json(status: dict) -> str:
    return json.dumps(status, indent=2, sort_keys=True) + "\n"


def render(root: Path, definitions: dict | None = None) -> dict[Path, str]:
    status = collect(root, definitions)
    return {PAGE: render_markdown(status), STATUS_JSON: render_json(status)}


def stale_outputs(root: Path, outputs: dict[Path, str]) -> list[Path]:
    stale = []
    for rel, text in outputs.items():
        path = root / rel
        if not path.is_file() or path.read_text(encoding="utf-8") != text:
            stale.append(rel)
    return stale


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--check", action="store_true",
                        help="re-render in memory; exit 1 if the committed outputs differ")
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    root = args.root.resolve()
    try:
        outputs = render(root)
    except SourceError as exc:
        print(f"render_status: {exc}", file=sys.stderr)
        return 2
    if args.check:
        stale = stale_outputs(root, outputs)
        if stale:
            for rel in stale:
                print(f"render_status: STALE {rel}", file=sys.stderr)
            print(f"render_status: the status page no longer matches its committed sources. "
                  f"Run `{REFRESH}` and commit the result.", file=sys.stderr)
            return 1
        print(f"render_status: OK -- {PAGE} and {STATUS_JSON} match their sources")
        return 0
    for rel, text in outputs.items():
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        (root / rel).write_text(text, encoding="utf-8")
        print(f"render_status: wrote {rel}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
