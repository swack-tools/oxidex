#!/usr/bin/env python3
"""How much of ExifTool's expression surface the v2 Session + helper library
(src/exiftool_tables/session.rs, helpers.rs) now satisfies -- measured with
the spike's own census, parser and classifier (run_spike.py, #817), so every
number is comparable with spike/COVERAGE.md.

    python3 tools/exiftool-tables/spike/session_helper_coverage.py <dump.json> \
        [--out-md tools/exiftool-tables/spike/SESSION_HELPER_COVERAGE.md]

A use is COVERED when (1) the spike grammar parses it, (2) every session
dependency the spike classifier reports is one `Session` models, (3) every
helper/data dependency is a ported helper, and (4) it needs no regex
construct the `regex` crate cannot compile. "Before" is the spike's own
definition of a complete Rust port (helpers.RUST_PORTS minus PARTIAL_PORTS,
i.e. exprs.rs at a29874aa); "after" adds the ports listed in helpers.rs's
`PORTS` table. This is a DEPENDENCY ceiling, like every spike rung: it says
what an evaluator over this Session and helper set would have to be handed,
not that each expression has been evaluated against Perl.

Frames (stated, never implied):
  A  expr_coverage.py's denominator (no `_variants`, no `*Inv`, string
     ValueConv/PrintConv/RawConv only) -- COVERAGE.md's Frame A, unchanged.
  B  every expression in the dump -- COVERAGE.md's Frame B, unchanged.
  M  Exif::Main only: every slot (Condition, RawConv, ValueConv, PrintConv
     and both *Inv), string and code forms, INCLUDING `_variants` -- the
     table v2 step 2 compiles end to end.
  Mr Exif::Main read side: M without ValueConvInv/PrintConvInv.

Session keys, two rungs:
  typed  $$self{Make}, $$self{Model}, $count, $format, the OPTIONS hash
         (`$$self{OPTIONS}` / `$self->Options(...)`), and `$self` handed to
         a helper.
  map    typed + any other named `$$self{X}` member (the MemberVal map).
         Never: another tag's value (`$$self{VALUE}`/GetValue), a dynamic
         key, `$$dirInfo{...}`, or eval-site lexicals other than
         $count/$format ($tag, $tagInfo, $dataPt, ...).
"""

import argparse
import hashlib
import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import run_spike as S  # noqa: E402  (also puts scripts/ on sys.path)
import helpers as helper_lib  # noqa: E402
import instrument  # noqa: E402

HELPERS_RS = S.REPO_ROOT / "src" / "exiftool_tables" / "helpers.rs"
CAPTURE = HERE.parent / "testdata" / "helper_oracle_outputs.json"

TYPED_KEYS = {"self:Make", "self:Model", "ctx:$count", "ctx:$format",
              "self:OPTIONS", "self:<object>"}
NEVER_KEYS = {"self:VALUE{*} (other tags)", "self:<dynamic key>"}
# Ports whose refusals include an input the DEFAULT options reach (not only
# a non-default ExifTool option or a >2**53 magnitude): counted separately.
PARTIAL_V2 = {"Image::ExifTool::GetUnixTime"}


def typed_key(k):
    return k in TYPED_KEYS or k.startswith("Options(")


def map_key(k):
    if typed_key(k):
        return True
    return k.startswith("self:") and k not in NEVER_KEYS


def v2_ports():
    """Qualified Perl subs in helpers.rs's PORTS table, cross-checked against
    the helper_oracle capture (a port the capture does not mark ported is an
    error, not a silently inflated count)."""
    text = HELPERS_RS.read_text(encoding="utf-8")
    block = text[text.index("pub const PORTS"):text.index("pub const REFUSED_HELPERS")]
    ported = re.findall(r'perl: "([^"]+)"', block)
    cap = json.loads(CAPTURE.read_text(encoding="utf-8"))["helpers"]
    bad = [p for p in ported if cap.get(p, {}).get("status") != "ported"]
    if bad:
        raise SystemExit(f"PORTS entries not marked ported in the capture: {bad}")
    return set(ported)


def covered_set(infos, key_ok, helpers):
    uses = distinct = 0
    for info in infos:
        if not info.parsed:
            continue
        keys, hlp = S.dep_sets(info)
        if info.deps.regex_fancy:
            continue
        if not all(key_ok(k) for k in keys):
            continue
        if not hlp <= helpers:
            continue
        uses += info.uses
        distinct += 1
    return uses, distinct


def frame(sites, infos, pred):
    """ExprInfos restricted to the sites `pred` accepts, uses recounted."""
    out = {}
    for site in sites:
        if not pred(site):
            continue
        info = infos[site.key]
        rec = out.get(site.key)
        if rec is None:
            rec = out[site.key] = S.ExprInfo(site.key, info.form, info.text)
            rec.parsed, rec.refusal = info.parsed, info.refusal
            rec.features, rec.deps = info.features, info.deps
            rec.baseline = info.baseline
            rec.sites = [site]
        rec.uses += 1
    return list(out.values())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("--lib", default=S.PINNED_LIB)
    ap.add_argument("--out-md", default=str(HERE / "SESSION_HELPER_COVERAGE.md"))
    ap.add_argument("--out-json", default=None)
    args = ap.parse_args()

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "session_helper_coverage.py")
    sha = hashlib.sha256(pathlib.Path(args.dump).read_bytes()).hexdigest()
    header = [f"dump:    {args.dump}", f"sha256:  {sha}",
              f"perl:    {args.lib} (SOURCE read, never executed)",
              f"ports:   {HELPERS_RS.relative_to(S.REPO_ROOT)} PORTS, cross-checked "
              f"against {CAPTURE.relative_to(S.REPO_ROOT)}"]
    instrument.print_header(tool="spike/session_helper_coverage.py", git=git,
                            dirty_overridden=dirty_overridden, extra=header)

    version, sites = S.census(args.dump)
    lib = helper_lib.Lib(args.lib)
    rust_ported, _ = helper_lib.rust_ported(S.EXPRS_RS)

    infos = {}
    for site in sites:
        info = infos.get(site.key)
        if info is None:
            info = infos[site.key] = S.ExprInfo(site.key, site.form, site.text)
        info.uses += 1
        info.sites.append(site)
    for info in infos.values():
        S.analyse(info)
        info.baseline = S.baseline_accepts(info.sites[0])

    # Every helper/data dependency name the classifier produced, resolved to
    # a qualified Perl sub where one exists.
    all_deps = set()
    for info in infos.values():
        if info.parsed:
            all_deps |= S.dep_sets(info)[1]
    ports = v2_ports()
    resolved = {d: (lib.resolve(d) if not d.startswith(("%", "$", "@", "<")) else None)
                for d in sorted(all_deps)}
    before = {d for d, how in rust_ported.items() if how == "full"}
    v2_names = {d for d, q in resolved.items() if q in ports}
    v2_complete = {d for d in v2_names if resolved[d] not in PARTIAL_V2}
    after_complete = before | v2_complete
    after_all = before | v2_names
    unresolved = sorted(ports - {resolved[d] for d in v2_names})

    frames = {
        "A": frame(sites, infos, lambda s: s.expr_coverage_frame),
        "B": frame(sites, infos, lambda s: True),
        "M": frame(sites, infos, lambda s: s.module == "Exif" and s.table == "Main"),
        "Mr": frame(sites, infos, lambda s: s.module == "Exif" and s.table == "Main"
                    and not s.slot.startswith(("ValueConvInv", "PrintConvInv"))),
    }
    results = {}
    for name, fi in frames.items():
        total = sum(i.uses for i in fi)
        rows = [("exprs.py/conds.py today, no Session (COVERAGE.md rung a)",
                 sum(i.uses for i in fi if i.baseline), sum(1 for i in fi if i.baseline))]
        parsed = sum(i.uses for i in fi if i.parsed)
        rows.append(("parseable by the spike grammar", parsed,
                     sum(1 for i in fi if i.parsed)))
        pure = covered_set(fi, lambda k: False, set())
        rows.append(("PURE (no session key, no helper)", *pure))
        for label, key_ok in (("typed Session", typed_key), ("Session + member map", map_key)):
            for hl, hs in (("before: exprs.rs complete ports (a29874aa)", before),
                           ("after: + v2 ports, complete only", after_complete),
                           ("after: + v2 ports incl. partial", after_all)):
                rows.append((f"{label}, {hl}", *covered_set(fi, key_ok, hs)))
        # Which missing dependency blocks the most still-uncovered uses.
        blockers = {}
        for info in fi:
            if not info.parsed or info.deps.regex_fancy:
                continue
            keys, hlp = S.dep_sets(info)
            miss = sorted([k for k in keys if not map_key(k)] + sorted(hlp - after_all))
            if miss:
                for m in miss:
                    blockers[m] = blockers.get(m, 0) + info.uses
        results[name] = {"total_uses": total, "total_distinct": len(fi), "rows": rows,
                         "blockers": sorted(blockers.items(), key=lambda kv: (-kv[1], kv[0]))[:25]}

    payload = {
        "instrument": {"tool": "tools/exiftool-tables/spike/session_helper_coverage.py",
                       "commit": git.commit, "describe": git.describe,
                       "dirty": bool(git.dirty), "dump": args.dump, "dump_sha256": sha,
                       "exiftool_version": version, "lib": args.lib},
        "before_ports": sorted(before),
        "v2_ports": sorted(ports),
        "v2_ports_as_spike_names": sorted(v2_names),
        "v2_partial": sorted(PARTIAL_V2),
        "v2_ports_never_called": unresolved,
        "frames": results,
    }
    if args.out_json:
        pathlib.Path(args.out_json).write_text(json.dumps(payload, indent=1), encoding="utf-8")
    render(payload, args.out_md)
    print(f"wrote {args.out_md}")


def pct(n, d):
    return f"{100.0 * n / d:.1f}%" if d else "n/a"


FRAME_TITLES = {
    "A": "Frame A -- expr_coverage.py's denominator (COVERAGE.md Frame A)",
    "B": "Frame B -- every expression in the dump (COVERAGE.md Frame B)",
    "M": "Frame M -- Exif::Main, every slot incl. `_variants` and `*Inv`",
    "Mr": "Frame Mr -- Exif::Main read side (M without `ValueConvInv`/`PrintConvInv`)",
}


def render(p, path):
    ins = p["instrument"]
    out = ["# Session + helper library coverage (Autogeneration v2, step 1 slice 1)", "",
           "Generated by `tools/exiftool-tables/spike/session_helper_coverage.py`; "
           "every number below comes from one run. See the script's docstring for the "
           "exact definition of COVERED, the frames and the two Session rungs.", "",
           "## Instrument", "",
           f"- tool: `{ins['tool']}` (the spike's census, `perl_subset.py` and "
           "`classify.py`, imported unchanged)",
           f"- repo commit: `{ins['describe']}` (`{ins['commit']}`), tree "
           f"{'DIRTY' if ins['dirty'] else 'clean'}",
           f"- dump: `{ins['dump']}`", f"- dump sha256: `{ins['dump_sha256']}`",
           f"- pinned release declared by the dump: **{ins['exiftool_version']}**",
           f"- ExifTool Perl source read (never executed): `{ins['lib']}`",
           "- no oxidex binary and no `exiftool` process is run; this is a dependency "
           "ceiling, not an evaluation",
           "- every rung below `parseable` also excludes uses needing a regex construct "
           "the `regex` crate cannot compile (lookahead, backreference), so PURE here is a "
           "few uses under COVERAGE.md's rung c",
           "- the `before` rows already assume the Session; the Session-free baseline is "
           "the first row (today's translators, `run_spike.baseline_accepts` at this "
           "commit -- in Frame A that also credits code refs, Conditions and Composite "
           "forms, which COVERAGE.md's Frame A rung a (`translate_or_compile_any` alone) "
           "did not, so the two can differ by a few uses)", "",
           "## Helper sets", "",
           f"- before ({len(p['before_ports'])}, the spike's complete ports at a29874aa): "
           + ", ".join(f"`{x}`" for x in p["before_ports"]),
           f"- v2 ports ({len(p['v2_ports'])}, `helpers.rs` PORTS): "
           + ", ".join(f"`{x}`" for x in p["v2_ports"]),
           "- v2 ports with a default-options refusal (counted only in the "
           "\"incl. partial\" rows): " + ", ".join(f"`{x}`" for x in p["v2_partial"]),
           ]
    if p["v2_ports_never_called"]:
        out.append("- v2 ports no expression in the dump calls by that name: "
                   + ", ".join(f"`{x}`" for x in p["v2_ports_never_called"]))
    out.append("")
    for name in ("A", "B", "M", "Mr"):
        f = p["frames"][name]
        out += [f"## {FRAME_TITLES[name]}", "",
                f"denominator: **{f['total_uses']} uses / {f['total_distinct']} distinct "
                "expressions**", "",
                "| rung | uses | % uses | distinct | % distinct |",
                "| --- | ---: | ---: | ---: | ---: |"]
        for label, u, d in f["rows"]:
            out.append(f"| {label} | {u} | {pct(u, f['total_uses'])} | {d} | "
                       f"{pct(d, f['total_distinct'])} |")
        out += ["", "Still-uncovered uses by missing dependency (Session + member map, "
                "after incl. partial; an expression missing several counts under each):", "",
                "| dependency | uses blocked |", "| --- | ---: |"]
        for dep, n in f["blockers"]:
            out.append(f"| `{dep}` | {n} |")
        out.append("")
    pathlib.Path(path).write_text("\n".join(out), encoding="utf-8")


if __name__ == "__main__":
    main()
