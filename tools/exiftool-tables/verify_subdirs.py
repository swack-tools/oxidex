#!/usr/bin/env python3
"""Step 27's differential ``SubDirectory`` Start/Base oracle.

``verify.py`` deliberately proves a different claim: it independently
re-derives which live-Perl ``SubDirectory`` hashes *should* compile to a
``SubdirEdge`` and checks the generated structure.  That cannot prove that a
Start/Base string which looks valid has the same runtime meaning in Perl and
Rust.  This script supplies that missing, live-evaluation check.

Corpus: every ``Some(SubdirEdge { ... })`` actually committed in
``binary_tables.rs``.  The edge is joined by its (module, table, tag-index)
to raw Start/Base text read independently from ExifTool's live Perl hashes by
``oracle.pl``.  Thus it checks the 64 shipped edges, rather than a prospective
grammar accepted by ``subdirs.py``.

For each edge, the Perl side executes the same two snippets from
``Image::ExifTool::ProcessBinaryData``:

* Start: ExifTool.pm:10124-10137.  A dollar-bearing Start is evaluated with
  ``$val,$dirStart``; a literal (or absent) Start is field-relative and adds
  ``$dirStart,$entry``.
* Base: ExifTool.pm:10118-10123.  A defined Base is evaluated with
  ``$start,$base``.  This oracle compares the expression result before the
  common ``+ $base`` at line 10122, exactly matching ``BaseExpr::eval``; the
  production caller applies that common addition afterwards.

The Rust side embeds the *actual generated* ``Start``/``BaseExpr`` source in a
throwaway Cargo binary and calls ``StartExpr::eval``/``BaseExpr::eval`` through
the public shipped types.  It is not a Python reimplementation of the
compiler.  ByteOrder and Validate are intentionally absent: ProcessBinaryData
never reads them in this branch (unlike Exif.pm's ProcessExif), and Step 27
refuses rather than models any such edge.

Probe battery: every relevant variable gets zero, small signed values and
large 32-bit boundaries.  Each integer literal present in that edge's raw
Start/Base text is added together with its immediate neighbours and negative.
Field-relative Starts also vary ``$entry`` because that addition is part of
the actual ProcessBinaryData branch.  A dollar Start is directly eval'd at
``$val == 0`` too: the real walker skips that directory before it evaluates
the expression (ExifTool.pm:10128), but evaluating it here is the only way to
test the compiled arithmetic at the requested zero input.  This does not
claim to implement the walker's skip/bounds policy -- ``Start`` intentionally
does not own it (see ``src/exiftool_tables/subdir.rs``).

Instruments, per AGENTS.md:
  * capability probe: an explicitly named pinned ExifTool executable (never a
    bare ``exiftool``), checked for its exact version AND for correct
    container-format detection on a real carrier file, since a matching
    ``-ver`` alone survives a degraded interpreter (see ``capability_probe``);
  * Perl evaluation: an explicitly named Perl interpreter with the exact
    pinned ExifTool ``lib`` prepended to ``@INC``;
  * Rust evaluation: a generated, deleted ``src/bin/subdir_oracle_harness.rs``
    run through Cargo against this checkout's ``oxidex`` library.
"""

import argparse
import itertools
import re
import subprocess
import sys
import tempfile
from pathlib import Path

# ``verify.py`` owns the deliberately robust parser for the committed Rust
# Field literals.  Reusing its artifact reader is appropriate here: this
# oracle's independence is Perl's raw facts + runtime evaluation, while using
# the exact parsed generated source is what prevents a hypothetical compiler
# output from standing in for the code that ships.
import verify

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))
import instrument  # noqa: E402 -- git/instrument identity header

HARNESS_PATH = REPO_ROOT / "src" / "bin" / "subdir_oracle_harness.rs"
ORACLE_PL = REPO_ROOT / "tools" / "exiftool-tables" / "oracle.pl"
DEFAULT_EXIFTOOL = "/tmp/oxidex-exiftool-cache/exiftool-pinned.sh"
DEFAULT_ET_LIB = "/tmp/oxidex-exiftool-cache/exiftool/lib"
# The capability probe's carrier, relative to the pinned tree (``<tree>/lib``
# is this script's ``et_lib``).  It must be a REAL container-format file, and
# it must come from the pinned tree itself: ``t/images/OOXML.docx`` is in
# ExifTool's MANIFEST, so the release tarball CI already fetches carries it,
# and no separate corpus is needed.  See ``capability_probe``.
PROBE_CARRIER = Path("t") / "images" / "OOXML.docx"
PROBE_CARRIER_FILETYPE = "DOCX"

# Signed, exact in both Perl's IV and Rust i64.  The two 32-bit limits are
# deliberately present because directory offsets commonly inhabit that range.
BASE_PROBES = [0, 1, -1, 2, 16, 255, 65_535, 2_147_483_647, 4_294_967_295]
ENTRY_PROBES = [0, 1, 16, 65_535, 2_147_483_647]
_INTEGER_RE = re.compile(r"(?<![A-Za-z0-9_$])(?:0[xX][0-9A-Fa-f]+|\d+)(?![A-Za-z0-9_])")


def _perl_quote(text):
    return text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n")


def _perl_bytes(text):
    """A non-interpolating Perl scalar literal for raw eval source.

    ``"$dirStart + $val"`` would interpolate while the harness is being
    assembled, turning the source expression into a probe-dependent literal
    before its ``eval`` ever ran.  Start/Base source in the closed grammar is
    ASCII, but hex packing also makes that important non-interpolation
    property obvious and robust.
    """
    return f'pack("H*", "{text.encode("utf-8").hex()}")'


def _literal_probes(*expressions):
    """Candidate values, expanded around every literal the source owns."""
    values = set(BASE_PROBES)
    for expression in expressions:
        for match in _INTEGER_RE.finditer(expression or ""):
            try:
                literal = int(match.group(0), 0)
            except ValueError:
                continue
            values.update((literal, literal - 1, literal + 1, -literal))
    return sorted(values)


def _cartesian(**channels):
    names = tuple(channels)
    return [dict(zip(names, values)) for values in itertools.product(*(channels[n] for n in names))]


def capability_probe(exiftool, perl, et_lib, expect_version, probe_file):
    """Prove both named instruments work, not merely ``-ver``.

    The carrier matters as much as the assertion.  AGENTS.md's failure mode is
    specific: the pinned tree's ``exiftool`` starts ``#!/usr/bin/env perl``, so
    it can resolve a perl with no ``Archive::Zip``, and then EXIFTOOL REPORTS
    ``FileType: ZIP`` FOR A ``.docx`` -- every container format degrades at
    once -- *while* ``-ver`` still prints the right release.  A probe file
    whose detection does not route through the degraded module cannot observe
    that: a ``.json`` fixture reports ``FileType: JSON`` identically on a
    healthy and a broken interpreter, so asserting it proves only that the
    JSON pipeline runs.  This probe therefore asserts DOCX on the pinned
    tree's own ``t/images/OOXML.docx`` -- measured to report ``DOCX`` under a
    working perl and ``ZIP`` under one where ``Archive::Zip`` fails to load.
    """
    version = subprocess.run([exiftool, "-ver"], capture_output=True, text=True, timeout=30)
    parsed = subprocess.run(
        [exiftool, "-j", "-FileType", str(probe_file)],
        capture_output=True,
        text=True,
        timeout=30,
    )
    eval_probe = (
        f'use lib "{_perl_quote(str(et_lib))}"; use Image::ExifTool; '
        'package Image::ExifTool; '
        'my $val = 3; my $dirStart = 16; '
        'my $expr = pack("H*", "246469725374617274202b202476616c"); '
        'my $r = eval($expr); '
        'print "VERSION\\t$Image::ExifTool::VERSION\\nEVAL\\t", '
        '(defined $r ? $r : "UNDEF"), "\\n";'
    )
    eval_run = subprocess.run([perl, "-e", eval_probe], capture_output=True, text=True, timeout=30)
    eval_lines = dict(line.split("\t", 1) for line in eval_run.stdout.splitlines() if "\t" in line)
    ok = (
        version.returncode == 0
        and version.stdout.strip() == expect_version
        and parsed.returncode == 0
        and f'"FileType": "{PROBE_CARRIER_FILETYPE}"' in parsed.stdout
        and eval_run.returncode == 0
        and eval_lines.get("VERSION") == expect_version
        and eval_lines.get("EVAL") == "19"
    )
    print(
        f"capability probe: exiftool={exiftool} version={version.stdout.strip()!r} "
        f"carrier={probe_file} FileType="
        f"{PROBE_CARRIER_FILETYPE if f'\"FileType\": \"{PROBE_CARRIER_FILETYPE}\"' in parsed.stdout else '<degraded-or-missing>'} "
        f"perl={perl} perl-version={eval_lines.get('VERSION')!r} eval($dirStart+$val)="
        f"{eval_lines.get('EVAL')!r} rc=({version.returncode},{parsed.returncode},{eval_run.returncode}) "
        f"-> {'OK' if ok else 'FAILED'}"
    )
    if not ok:
        print("-ver stderr:", version.stderr[-2000:], file=sys.stderr)
        print("carrier probe stderr:", parsed.stderr[-2000:], file=sys.stderr)
        print("Perl eval probe stderr:", eval_run.stderr[-2000:], file=sys.stderr)
        raise SystemExit(
            "oracle capability probe failed -- refusing to trust this ExifTool as ground truth "
            "(AGENTS.md: a matching -ver is not a working oracle)"
        )


def load_perl_subdirs(perl, et_lib):
    """Raw SUBDIR facts from the live Perl hash, independent of codegen."""
    run = subprocess.run(
        [perl, str(ORACLE_PL), str(et_lib)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=90,
    )
    if run.returncode:
        print("oracle.pl stderr:", run.stderr[-4000:], file=sys.stderr)
        raise SystemExit(f"live-Perl SubDirectory census exited {run.returncode}")
    facts = {}
    for line in run.stdout.splitlines():
        row = line.split("\t")
        if len(row) == 10 and row[3] == "SUBDIR":
            facts[(row[0], row[1], row[2])] = {"start": row[5], "base": row[6]}
    return facts


def census(generated_rs, perl_facts):
    """Join every generated edge to its raw live-Perl Start/Base strings."""
    # By ATTRIBUTE, not by position: this caller has been broken twice by
    # `parse_rust` growing a return value (8-of-11 in Step 25, fixed in
    # da1fec86 by hardcoding 11; then 11-of-13 when Step 28's print_conv
    # accounting appended pc_refused/pc_kinds). Naming the one field this
    # verifier actually consumes ends that class -- see `verify.ParsedRust`.
    generated_edges = verify.parse_rust(generated_rs).subdir_edges
    edges = []
    missing = []
    for key, edge in sorted(generated_edges.items()):
        if edge is None:
            continue
        fact = perl_facts.get(key)
        if fact is None:
            missing.append(key)
            continue
        module, table, rust_start, rust_base = edge
        edges.append({
            "key": key,
            "target": f"{module}::{table}",
            "raw_start": fact["start"],
            "raw_base": fact["base"],
            "rust_start": rust_start,
            "rust_base": rust_base,
        })
    if missing:
        raise SystemExit(
            "generated SubdirEdge(s) have no live-Perl SUBDIR fact: "
            + ", ".join(map(str, missing[:10]))
        )
    return edges


def build_jobs(edges):
    """One job per Start/Base probe, including a visible edge label."""
    jobs = []
    for edge_id, edge in enumerate(edges):
        literals = _literal_probes(edge["raw_start"], edge["raw_base"])
        if "$" in edge["raw_start"]:
            # ExifTool.pm:10129 exposes only these two variables in Start's
            # eval scope. ``entry`` intentionally does not affect this branch.
            probes = _cartesian(val=literals, dir_start=literals)
        else:
            # ExifTool.pm:10135 adds both enclosing DirStart and field entry.
            probes = _cartesian(dir_start=literals, entry=ENTRY_PROBES)
        for probe in probes:
            jobs.append((len(jobs), edge_id, "start", probe))
        if edge["raw_base"]:
            # ExifTool.pm:10120's eval marker specifies exactly $start,$base.
            for probe in _cartesian(start=literals, base=literals):
                jobs.append((len(jobs), edge_id, "base", probe))
    return jobs


def assert_execution_shapes(edges):
    """Refuse a census whose raw/evaluated modes cannot be paired.

    The differential jobs below prove arithmetic values.  This small guard
    makes a start/base *mode* disagreement explicit instead of accidentally
    treating a generated ``Some(BaseExpr)`` under a raw inherited Base as an
    edge with nothing to execute.  ``verify.py`` separately checks edge
    presence and target structure; these are the two mode bits relevant to
    executing the expressions here.
    """
    mismatches = []
    for edge in edges:
        raw_start_is_expr = "$" in edge["raw_start"]
        rust_start_is_expr = edge["rust_start"].startswith("Start::Expr(")
        raw_has_base = bool(edge["raw_base"])
        rust_has_base = edge["rust_base"] != "None"
        if raw_start_is_expr != rust_start_is_expr or raw_has_base != rust_has_base:
            mismatches.append((edge, raw_start_is_expr, rust_start_is_expr, raw_has_base, rust_has_base))
    if mismatches:
        details = "; ".join(
            f"{'::'.join(edge['key'])}: raw(StartExpr={raw_start}, Base={raw_base}) "
            f"!= Rust(StartExpr={rust_start}, Base={rust_base})"
            for edge, raw_start, rust_start, raw_base, rust_base in mismatches[:10]
        )
        raise SystemExit(f"SubDirectory execution-mode mismatch: {details}")


def build_perl_script(jobs, edges, et_lib):
    lines = [
        f'use lib "{_perl_quote(str(et_lib))}";',
        "use Image::ExifTool;",
        "binmode STDOUT, ':utf8';",
        # ExifTool's own ProcessBinaryData lives in this package. None of the
        # current closed grammar needs a package symbol, but preserving eval's
        # caller package makes this an oracle for its actual context.
        "package Image::ExifTool;",
    ]
    for job_id, edge_id, kind, probe in jobs:
        edge = edges[edge_id]
        lines.append("{")
        if kind == "start":
            lines.extend([
                f'  my $val = {probe.get("val", 0)};',
                f'  my $dirStart = {probe["dir_start"]};',
                f'  my $entry = {probe.get("entry", 0)};',
                f'  my $start = {_perl_bytes(edge["raw_start"])} || 0;',
                "  my $r;",
                "  if ($start =~ /\\$/) { $r = eval($start); }",
                "  else { $r = $start + $dirStart + $entry; }",
            ])
        else:
            lines.extend([
                f'  my $start = {probe["start"]};',
                f'  my $base = {probe["base"]};',
                f'  my $r = eval({_perl_bytes(edge["raw_base"])});',
            ])
        lines.append(f'  if ($@) {{ print "J{job_id}\\tERROR\\n"; }}')
        lines.append(f'  elsif (!defined $r) {{ print "J{job_id}\\tUNDEF\\n"; }}')
        lines.append(f'  else {{ print "J{job_id}\\t$r\\n"; }}')
        lines.append("}")
    return "\n".join(lines)


def run_perl(perl, script, timeout):
    with tempfile.NamedTemporaryFile("w", suffix=".pl", delete=False) as handle:
        handle.write(script)
        script_path = Path(handle.name)
    try:
        run = subprocess.run([perl, str(script_path)], capture_output=True, text=True, timeout=timeout)
    finally:
        script_path.unlink(missing_ok=True)
    if run.returncode:
        print("Perl harness stderr:", run.stderr[-6000:], file=sys.stderr)
        raise SystemExit(f"Perl harness exited {run.returncode}")
    return dict(line.split("\t", 1) for line in run.stdout.splitlines() if "\t" in line)


def build_rust_harness(jobs, edges):
    body = []
    for job_id, edge_id, kind, probe in jobs:
        edge = edges[edge_id]
        label = "::".join(edge["key"])
        if kind == "start":
            value = (
                f"match &START_{job_id} {{ "
                f"Start::FieldRelative(n) => *n + {probe['dir_start']} + {probe.get('entry', 0)}, "
                f"Start::Expr(expr) => expr.eval({probe.get('val', 0)}, {probe['dir_start']}), "
                "}"
            )
            decl = f"static START_{job_id}: Start = {edge['rust_start']};"
        else:
            value = f"BASE_{job_id}.expect(\"censused Base\").eval({probe['start']}, {probe['base']})"
            decl = f"static BASE_{job_id}: Option<&BaseExpr> = {edge['rust_base']};"
        body.append(
            "    {\n"
            f"        {decl}\n"
            f"        let result: i64 = {value};\n"
            f"        out.push_str(&format!(\"J{job_id}\\t{{result}}\\n\")); // {label} {kind}\n"
            "    }"
        )
    return (
        "// GENERATED by tools/exiftool-tables/verify_subdirs.py -- not committed.\n"
        "#[allow(clippy::all, unused_imports)]\n"
        "use oxidex::exiftool_tables::subdir::{BaseExpr, Start, StartExpr};\n"
        "fn main() {\n"
        "    let mut out = String::new();\n"
        + "\n".join(body)
        + "\n    print!(\"{out}\");\n"
        "}\n"
    )


def run_rust(jobs, edges, timeout):
    HARNESS_PATH.parent.mkdir(parents=True, exist_ok=True)
    HARNESS_PATH.write_text(build_rust_harness(jobs, edges), encoding="utf-8")
    try:
        run = subprocess.run(
            ["cargo", "run", "--quiet", "--bin", "subdir_oracle_harness"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    finally:
        HARNESS_PATH.unlink(missing_ok=True)
    if run.returncode:
        print("Rust harness stderr:", run.stderr[-6000:], file=sys.stderr)
        raise SystemExit(f"Rust harness exited {run.returncode}")
    return dict(line.split("\t", 1) for line in run.stdout.splitlines() if "\t" in line)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("generated_rs", nargs="?", type=Path, default=REPO_ROOT / "src/exiftool_tables/binary_tables.rs")
    parser.add_argument("et_lib", nargs="?", type=Path, default=Path(DEFAULT_ET_LIB))
    parser.add_argument("--exiftool", default=DEFAULT_EXIFTOOL, help="explicit pinned executable; never defaults to PATH exiftool")
    parser.add_argument("--perl", default="/usr/bin/perl", help="interpreter used with --et-lib for eval harnesses")
    parser.add_argument(
        "--probe-file",
        type=Path,
        default=None,
        help="capability-probe carrier; defaults to the pinned tree's own t/images/OOXML.docx",
    )
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--sample-lines", type=int, default=15)
    args = parser.parse_args()

    probe_file = args.probe_file or (args.et_lib.parent / PROBE_CARRIER)
    if not args.generated_rs.is_file() or not args.et_lib.is_dir() or not probe_file.is_file():
        raise SystemExit(
            f"generated Rust ({args.generated_rs}), pinned ExifTool lib ({args.et_lib}) and "
            f"capability-probe carrier ({probe_file}) must all exist"
        )
    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "verify_subdirs.py")
    instrument.print_header(
        tool="verify_subdirs.py",
        git=git,
        dirty_overridden=dirty_overridden,
        extra=[
            f"exiftool: {args.exiftool}",
            f"perl:    {args.perl}  et_lib={args.et_lib}",
            f"target:  {args.generated_rs}",
        ],
    )

    version = verify.oracle_version(args.et_lib)
    capability_probe(args.exiftool, args.perl, args.et_lib, version, probe_file)

    edges = census(args.generated_rs, load_perl_subdirs(args.perl, args.et_lib))
    assert_execution_shapes(edges)
    jobs = build_jobs(edges)
    start_edges = len(edges)
    base_edges = sum(1 for edge in edges if edge["raw_base"])

    print(f"pinned release          {version}")
    print(f"modelled SubdirEdges    {len(edges)}")
    print(f"  Start evaluated       {start_edges}/{len(edges)}")
    print(f"  Base evaluated        {base_edges}/{len(edges)} (the other {len(edges) - base_edges} inherit Base)")
    print(f"probe jobs              {len(jobs)}")

    print("running Perl oracle ...")
    perl_out = run_perl(args.perl, build_perl_script(jobs, edges, args.et_lib), args.timeout)
    print("running Rust harness (cargo run --bin subdir_oracle_harness) ...")
    rust_out = run_rust(jobs, edges, args.timeout)

    per_edge = [[0, 0, 0] for _ in edges]  # pass, fail, unavailable
    failures = []
    unavailable = []
    for job_id, edge_id, kind, probe in jobs:
        perl_value = perl_out.get(f"J{job_id}", "<missing>")
        rust_value = rust_out.get(f"J{job_id}", "<missing>")
        if perl_value in ("ERROR", "UNDEF", "<missing>") or rust_value == "<missing>":
            per_edge[edge_id][2] += 1
            if len(unavailable) < 20:
                unavailable.append((edges[edge_id], kind, probe, perl_value, rust_value))
        elif perl_value == rust_value:
            per_edge[edge_id][0] += 1
        else:
            per_edge[edge_id][1] += 1
            if len(failures) < 40:
                failures.append((edges[edge_id], kind, probe, perl_value, rust_value))

    total_pass = sum(row[0] for row in per_edge)
    total_fail = sum(row[1] for row in per_edge)
    total_unavailable = sum(row[2] for row in per_edge)
    edge_pass = sum(1 for passed, failed, unavailable_count in per_edge if passed and not failed and not unavailable_count)
    edge_fail = sum(1 for _passed, failed, _unavailable in per_edge if failed)
    edge_partial = len(edges) - edge_pass - edge_fail

    print()
    print(f"probe-level: PASS {total_pass}  FAIL {total_fail}  UNAVAILABLE {total_unavailable}")
    print(f"edge-level: PASS (all probes) {edge_pass}/{len(edges)}  FAIL {edge_fail}/{len(edges)}  PARTIAL {edge_partial}/{len(edges)}")
    print()
    print(f"sample of {min(args.sample_lines, len(edges))} edge results:")
    for edge, (passed, failed, unavailable_count) in zip(edges[: args.sample_lines], per_edge[: args.sample_lines]):
        status = "PASS" if passed and not failed and not unavailable_count else "FAIL" if failed else "PARTIAL"
        source = f"Start={edge['raw_start']!r} Base={edge['raw_base']!r}"
        print(f"  {status}  probes(pass={passed} fail={failed} unavailable={unavailable_count})  {'::'.join(edge['key'])} -> {edge['target']}  {source}")

    if failures:
        print("\nFAILING probe examples (edge, kind, inputs, perl, rust):")
        for edge, kind, probe, perl_value, rust_value in failures:
            print(f"  edge={'::'.join(edge['key'])} kind={kind} inputs={probe!r} perl={perl_value!r} rust={rust_value!r}")
    if unavailable:
        print("\nUNAVAILABLE probe examples (edge, kind, inputs, perl, rust):")
        for edge, kind, probe, perl_value, rust_value in unavailable:
            print(f"  edge={'::'.join(edge['key'])} kind={kind} inputs={probe!r} perl={perl_value!r} rust={rust_value!r}")

    print()
    if total_fail or total_unavailable:
        print("RESULT: FAIL -- a modeled SubDirectory edge did not complete identically against pinned Perl")
        raise SystemExit(1)
    print(
        "RESULT: PASS -- every modeled SubDirectory edge agreed with the pinned Perl oracle "
        f"on every probe ({total_pass} comparisons across {len(edges)} edges)"
    )


if __name__ == "__main__":
    main()
