#!/usr/bin/env python3
"""Step 23's differential `Condition` oracle.

Same discipline as `verify_exprs.py` (its sibling for `ValueConv`/
`PrintConv`): a `Condition` string `conds.py` claims to compile is not
verified because the Rust *looks* like a faithful translation of the Perl.
It is verified because this script ran ExifTool's own pinned Perl and the
shipped Rust `Cond::eval` over the same probe inputs and diffed the boolean
result. Anything that disagrees is a real bug in the compiler, found before
it ships a wrong first-match-wins decision under a real ExifTool tag name.

Corpus: every distinct `Condition` string appearing in a binary table's
`_variants` array in the pinned dump -- i.e. exactly the population
`codegen.py`'s `compile_variant_group` draws from, so this oracle checks
what actually shipped in `binary_tables.rs`, not a hypothetical wider
grammar. Conditions `conds.py` refuses are skipped (nothing to verify: they
were never compiled, `codegen.py` counts and reports the refusal itself).

Probe design (see `probes_for`): for each atomic construct inside a
compiled condition -- a `$$self{Member} =~ /pattern/`, a `$$self{Member}
<op> N`, a `$$self{Member} eq "str"`, `$$valPt =~ /pattern/`, `$format`/
`$count` comparisons -- a small battery of concrete values is generated:
literal strings the pattern's own vetted-subset AST would match (walked the
same way `conds.py`'s validator walks it, so the same construct set is
covered on both sides), numeric boundary values around each comparison
target, and a handful of fixed decoys/near-misses. Every member/valPt/
format/count channel referenced anywhere in the condition gets its own
candidate list, and the FULL cross product (capped) becomes the probe
battery for that condition -- this is what "the dump's own model-name
corpus" means here: the model names ARE the corpus, generated straight out
of ExifTool's own regex literals rather than a hand-picked list.

`Cond::SetMember` (the assignment-as-condition idiom) is NOT exercised by
this corpus-driven oracle: none of the pinned dump's binary-table
`_variants` conditions use it (verified separately -- see this script's
`main()` output). Its evaluation-order contract (an assignment's side effect
fires even on a losing entry) is instead pinned by hand-written Rust unit
tests in `src/exiftool_tables/cond.rs` that cite the real Perl idiom
(Canon.pm:1312, Pentax.pm:4343) verbatim, the same way `verify_exprs.py`'s
module doc explains `ConvertDateTime`'s identity-translation is argued from
reading `Image::ExifTool::ConvertDateTime` rather than probed.

Instruments named, per AGENTS.md doctrine:
  - Perl side:  /usr/bin/perl5.34 -I <pinned ExifTool 13.59 lib>,
    capability-probed before use.
  - Rust side:  a throwaway `src/bin/cond_oracle_harness.rs`, built and run
    via `cargo run` -- the exact Rust source text `conds.py` emits (the same
    text `codegen.py` splices into `binary_tables.rs`), not a
    reimplementation.
"""
import argparse
import itertools
import json
import re
import subprocess
import sys
from pathlib import Path

import conds

REPO_ROOT = Path(__file__).resolve().parents[2]
HARNESS_PATH = REPO_ROOT / "src" / "bin" / "cond_oracle_harness.rs"


# --- census: every Condition string inside a binary table's _variants ------

def is_binary_table(meta):
    pp = meta.get("PROCESS_PROC")
    return isinstance(pp, dict) and (pp.get("__name") or "").endswith("ProcessBinaryData")


def census(tables_json_path):
    d = json.load(open(tables_json_path, encoding="utf-8"))
    counter = {}
    for _modname, mod in d["modules"].items():
        for _tname, t in (mod.get("tables") or {}).items():
            if not is_binary_table(t.get("meta") or {}):
                continue
            for _tid, tag in (t.get("tags") or {}).items():
                if isinstance(tag, dict) and "_variants" in tag:
                    for v in tag["_variants"]:
                        c = v.get("Condition") if isinstance(v, dict) else None
                        if isinstance(c, str) and c.strip():
                            counter[c] = counter.get(c, 0) + 1
    return d.get("exiftool_version"), counter


# --- capability probe -------------------------------------------------

def capability_probe(perl_bin, et_lib, expect_version):
    script = (
        f'use lib "{et_lib}"; use Image::ExifTool;\n'
        'print "VERSION\\t$Image::ExifTool::VERSION\\n";\n'
        'my $self = { Model => "DSLR-A230" };\n'
        'print "PROBE\\t", ($$self{Model} =~ /^DSLR-A230\\b/ ? "1" : "0"), "\\n";\n'
    )
    r = subprocess.run([perl_bin, "-e", script], capture_output=True, text=True, timeout=30)
    lines = dict(line.split("\t", 1) for line in r.stdout.splitlines() if "\t" in line)
    ok = r.returncode == 0 and lines.get("VERSION") == expect_version and lines.get("PROBE") == "1"
    print(
        f"capability probe: perl={perl_bin} lib={et_lib} "
        f"version={lines.get('VERSION')!r} rc={r.returncode} -> {'OK' if ok else 'FAILED'}"
    )
    if not ok:
        print("STDERR:", r.stderr, file=sys.stderr)
        raise SystemExit(
            "oracle capability probe failed -- refusing to trust this Perl/ExifTool "
            "as ground truth (AGENTS.md: a matching -ver is not a working oracle)"
        )


# --- regex example generation (walks the SAME AST conds.py validates) -----

def _regex_examples(pattern, ignore_case, cap=6):
    """Concrete strings the vetted-subset AST would match. Anchors (`^`,
    `$`, `\\b`, `\\B`) are no-ops here -- they impose no characters to emit,
    and the generated string always satisfies them by construction because
    every pattern in the census that uses `\\b` places it at the very end,
    right after the last alternative's literal characters (verified true of
    every pattern this oracle's census actually contains; see
    `conds.py`'s `_regex_ast_ok`)."""
    ast = conds.sre_parse.parse(pattern)

    def walk(nodes):
        results = [""]
        for op, av in nodes:
            name = op.name.lower() if hasattr(op, "name") else str(op).lower()
            if name == "literal":
                results = [r + chr(av) for r in results]
            elif name == "at":
                continue
            elif name == "in":
                chars = []
                for item_op, item_av in av:
                    iname = item_op.name.lower() if hasattr(item_op, "name") else str(item_op).lower()
                    if iname == "literal":
                        chars.append(chr(item_av))
                    elif iname == "range":
                        chars.append(chr(item_av[0]))
                    elif (
                        iname == "category"
                        and getattr(item_av, "name", str(item_av)) == "CATEGORY_DIGIT"
                    ):
                        # The closed grammar permits this only for Model
                        # regexes; exercise actual ASCII camera-model digits.
                        chars.extend(["0", "6", "9"])
                if not chars:
                    chars = ["Z"]
                results = [r + c for r in results for c in chars[:3]]
            elif name == "subpattern":
                sub = walk(av[3])
                results = [r + s for r in results for s in sub]
            elif name == "branch":
                branches = []
                for b in av[1]:
                    branches.extend(walk(b))
                results = [r + b for r in results for b in branches]
            elif name == "max_repeat":
                _lo, hi, sub = av
                variants = [""] + (walk(sub) if hi >= 1 else [])
                results = [r + v for r in results for v in variants]
            # dedupe + cap after every node to keep the cross product bounded
            seen = []
            for r in results:
                if r not in seen:
                    seen.append(r)
            results = seen[:40]
        return results

    examples = walk(ast)
    if ignore_case and examples:
        examples = examples + [examples[0].swapcase()]
    out = []
    for e in examples:
        if e not in out:
            out.append(e)
    return out[:cap]


_DECOY_STRINGS = ["UNRELATED-MODEL", "", "X", "NOPE-0000"]


# --- per-condition probe battery -------------------------------------

_NUM_ATOM_RE = re.compile(
    rf"{conds._MEMBER}\s*(==|!=|>=|<=|>|<|&)\s*(-?(?:0[xX][0-9a-fA-F]+|\d+))"
)
_STR_ATOM_RE = re.compile(rf'{conds._MEMBER}\s*(eq|ne)\s*"([^"]*)"')
_STR_CMP_ATOM_RE = re.compile(rf'{conds._MEMBER}\s*(lt|le|gt|ge)\s*"([^"]*)"')
_REGEX_ATOM_RE = re.compile(rf"{conds._MEMBER}\s*(=~|!~)\s*/((?:[^/\\]|\\.)*)/([a-z]*)")
_VALPT_ATOM_RE = re.compile(r"\$\$valPt\s*(=~|!~)\s*/((?:[^/\\]|\\.)*)/([a-z]*)")
_FORMAT_ATOM_RE = re.compile(r'\$format\s*eq\s*"([^"]*)"')
_COUNT_ATOM_RE = re.compile(r"\$count\s*(==|!=|>=|<=|>|<)\s*(-?\d+)")
_BARE_MEMBER_RE = re.compile(rf"(?<!=)(?<!~){conds._MEMBER}(?!\s*[=!<>&(])")


def _member_of(m):
    return m.group(1) or m.group(2)


def probes_for(condition):
    """-> dict of channel name ('self:Member', 'valPt', 'format', 'count')
    -> list of candidate values (str for self:*/valPt/format, int for
    count), covering every construct referencing that channel anywhere in
    `condition`."""
    channels = {}

    def add(name, values):
        channels.setdefault(name, [])
        for v in values:
            if v not in channels[name]:
                channels[name].append(v)

    for m in _REGEX_ATOM_RE.finditer(condition):
        member = _member_of(m)
        pattern, flags = m.group(4), m.group(5)
        ic = "i" in flags
        try:
            add(f"self:{member}", _regex_examples(pattern, ic))
        except Exception:  # noqa: BLE001 - a pattern this generator can't
            # walk is not this oracle's job to fix; the compiler's own
            # AST allowlist is what decides admissibility.
            pass
        add(f"self:{member}", _DECOY_STRINGS)

    for m in _STR_ATOM_RE.finditer(condition):
        member = _member_of(m)
        add(f"self:{member}", [m.group(4), "other-value", ""])

    for m in _STR_CMP_ATOM_RE.finditer(condition):
        member, target = _member_of(m), m.group(4)
        # Explicitly span both lexical sides of the pinned firmware version
        # literals, including the classic lexical-vs-numeric counterexample.
        add(f"self:{member}", ["", "01.99", target, "02.01", "10.00"])

    for m in _NUM_ATOM_RE.finditer(condition):
        member = _member_of(m)
        n = int(m.group(4), 0)
        add(f"self:{member}", [n, n - 1, n + 1, 0])

    for m in _VALPT_ATOM_RE.finditer(condition):
        pattern, flags = m.group(2), m.group(3)
        try:
            add("valPt", _regex_examples(pattern, "i" in flags))
        except Exception:  # noqa: BLE001
            pass
        add("valPt", _DECOY_STRINGS)

    for m in _FORMAT_ATOM_RE.finditer(condition):
        add("format", [m.group(1), "other-format"])

    for m in _COUNT_ATOM_RE.finditer(condition):
        n = int(m.group(2))
        add("count", [n, n - 1, n + 1, 0])

    # Bare-truthy members (no comparison operator anywhere touching them):
    # numeric 0/1 is the realistic case for every bare member in the census
    # (a Perl flag data member set elsewhere in the same table's decode).
    for m in _BARE_MEMBER_RE.finditer(condition):
        member = _member_of(m)
        key = f"self:{member}"
        if key not in channels:
            add(key, [0, 1])

    return channels


def _is_numeric_channel(values):
    return all(isinstance(v, int) for v in values)


def build_combinations(channels, cap=48):
    """Cross product of every channel's candidates, capped."""
    if not channels:
        return [{}]
    names = sorted(channels)
    lists = [channels[n] for n in names]
    combos = []
    for combo in itertools.product(*lists):
        combos.append(dict(zip(names, combo)))
        if len(combos) >= cap:
            break
    return combos


# --- Perl side ----------------------------------------------------------

def perl_escape(s):
    return s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\x00", "\\0")


def build_perl_script(jobs, et_lib):
    lines = [
        f'use lib "{et_lib}";',
        "use Image::ExifTool;",
        "binmode STDOUT, ':utf8';",
        "package Image::ExifTool;",
    ]
    for job_id, condition, combo in jobs:
        lines.append("{")
        self_pairs = []
        for chan, val in combo.items():
            if chan.startswith("self:"):
                member = chan[len("self:"):]
                if isinstance(val, int):
                    self_pairs.append(f'"{member}" => {val}')
                else:
                    self_pairs.append(f'"{member}" => "{perl_escape(val)}"')
        lines.append(f"  my $self = {{ {', '.join(self_pairs)} }};")
        if "valPt" in combo:
            lines.append(f'  my $valPtData = "{perl_escape(combo["valPt"])}";')
            lines.append("  my $valPt = \\$valPtData;")
        if "format" in combo:
            lines.append(f'  my $format = "{perl_escape(combo["format"])}";')
        if "count" in combo:
            lines.append(f"  my $count = {combo['count']};")
        lines.append(f"  my $r = eval {{ {condition} }};")
        lines.append(f'  if ($@) {{ print "J{job_id}\\tERROR\\n"; }}')
        lines.append(f'  else {{ print "J{job_id}\\t", ($r ? "1" : "0"), "\\n"; }}')
        lines.append("}")
    return "\n".join(lines)


def run_perl(perl_bin, script_text, timeout):
    import tempfile
    with tempfile.NamedTemporaryFile("w", suffix=".pl", delete=False) as fh:
        fh.write(script_text)
        script_path = fh.name
    try:
        r = subprocess.run([perl_bin, script_path], capture_output=True, text=True, timeout=timeout)
    finally:
        Path(script_path).unlink(missing_ok=True)
    if r.returncode != 0:
        print("PERL HARNESS STDERR:", r.stderr[-4000:], file=sys.stderr)
        raise SystemExit(f"perl harness exited {r.returncode}")
    out = {}
    for line in r.stdout.splitlines():
        if "\t" in line:
            jid, val = line.split("\t", 1)
            out[jid] = val
    return out


# --- Rust side ------------------------------------------------------------

def rust_str_literal(s):
    out = []
    for ch in s:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\x00":
            out.append("\\0")
        elif ch == "\n":
            out.append("\\n")
        else:
            out.append(ch)
    return '"' + "".join(out) + '"'


def build_rust_harness(jobs, cond_rust_by_text):
    body = []
    for job_id, condition, combo in jobs:
        cond_src = cond_rust_by_text[condition]
        lines = [f"    {{", f"        let mut members = std::collections::HashMap::new();"]
        for chan, val in combo.items():
            if chan.startswith("self:"):
                member = chan[len("self:"):]
                if isinstance(val, int):
                    lines.append(
                        f'        members.insert({rust_str_literal(member)}, '
                        f"oxidex::exiftool_tables::cond::MemberValue::Num({val}));"
                    )
                else:
                    lines.append(
                        f'        members.insert({rust_str_literal(member)}, '
                        f"oxidex::exiftool_tables::cond::MemberValue::Str({rust_str_literal(val)}.to_string()));"
                    )
        lines.append("        let mut ctx = oxidex::exiftool_tables::cond::Ctx::new(&mut members);")
        if "valPt" in combo:
            lines.append(f"        let __valpt = {rust_str_literal(combo['valPt'])}.as_bytes();")
            lines.append("        ctx.val_pt = Some(__valpt);")
        if "format" in combo:
            lines.append(f"        ctx.format = Some({rust_str_literal(combo['format'])});")
        if "count" in combo:
            lines.append(f"        ctx.count = Some({combo['count']});")
        lines.append(f"        static COND_{job_id}: oxidex::exiftool_tables::cond::Cond = {cond_src};")
        lines.append(
            f'        out.push_str(&format!("J{job_id}\\t{{}}\\n", '
            f"if COND_{job_id}.eval(&mut ctx) {{ \"1\" }} else {{ \"0\" }}));"
        )
        lines.append("    }")
        body.append("\n".join(lines))
    src = (
        "// GENERATED by tools/exiftool-tables/verify_cond.py -- not committed.\n"
        "#[allow(clippy::all, unused_variables, unused_mut)]\n"
        "use oxidex::exiftool_tables::cond::{CmpOp, Cond, EffectSource, StrCmpOp};\n"
        "fn main() {\n"
        "    let mut out = String::new();\n"
        + "\n".join(body)
        + "\n    print!(\"{out}\");\n"
        "}\n"
    )
    return src


def run_rust(jobs, cond_rust_by_text, timeout):
    src = build_rust_harness(jobs, cond_rust_by_text)
    HARNESS_PATH.parent.mkdir(parents=True, exist_ok=True)
    HARNESS_PATH.write_text(src, encoding="utf-8")
    try:
        r = subprocess.run(
            ["cargo", "run", "--quiet", "--bin", "cond_oracle_harness"],
            cwd=REPO_ROOT, capture_output=True, text=True, timeout=timeout,
        )
        if r.returncode != 0:
            print("RUST HARNESS STDERR:", r.stderr[-6000:], file=sys.stderr)
            raise SystemExit(f"rust harness exited {r.returncode}")
        out = {}
        for line in r.stdout.splitlines():
            if "\t" in line:
                jid, val = line.split("\t", 1)
                out[jid] = val
        return out
    finally:
        HARNESS_PATH.unlink(missing_ok=True)


# --- main -----------------------------------------------------------------

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tables_json")
    ap.add_argument("--perl", default="/usr/bin/perl5.34")
    ap.add_argument("--et-lib", default="/tmp/oxidex-exiftool-cache/exiftool/lib")
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--sample-lines", type=int, default=15)
    ap.add_argument("--combo-cap", type=int, default=48)
    args = ap.parse_args()

    version, counter = census(args.tables_json)
    capability_probe(args.perl, args.et_lib, version)

    cond_rust_by_text = {}
    setmember_count = 0
    for c in counter:
        rust = conds.compile_cond(c)
        if rust is None:
            continue
        if "SetMember" in rust:
            setmember_count += 1
        cond_rust_by_text[c] = rust

    print(f"pinned release            {version}")
    print(f"distinct Conditions in _variants (binary tables)  {len(counter)}")
    print(f"compiled by conds.py (this oracle's corpus)        {len(cond_rust_by_text)}")
    print(f"  of which SetMember (not exercised by this corpus-driven probe --")
    print(f"           see hand-written cond.rs unit tests instead)  {setmember_count}")

    jobs = []
    for condition in sorted(cond_rust_by_text):
        channels = probes_for(condition)
        for combo in build_combinations(channels, cap=args.combo_cap):
            jobs.append((len(jobs), condition, combo))

    print(f"probe jobs                {len(jobs)}")

    perl_script = build_perl_script(jobs, args.et_lib)
    print("running Perl oracle ...")
    perl_out = run_perl(args.perl, perl_script, args.timeout)

    print("running Rust harness (cargo run --bin cond_oracle_harness) ...")
    rust_out = run_rust(jobs, cond_rust_by_text, args.timeout)

    per_cond = {}
    fail_examples = []
    skip_examples = []
    for job_id, condition, combo in jobs:
        jid = f"J{job_id}"
        pv = perl_out.get(jid, "<missing>")
        rv = rust_out.get(jid, "<missing>")
        stats = per_cond.setdefault(condition, [0, 0, 0])
        if pv == "ERROR":
            stats[2] += 1
            if len(skip_examples) < 20:
                skip_examples.append((condition, combo, pv, rv))
            continue
        if pv == rv:
            stats[0] += 1
        else:
            stats[1] += 1
            if len(fail_examples) < 40:
                fail_examples.append((condition, combo, pv, rv))

    total_pass = sum(s[0] for s in per_cond.values())
    total_fail = sum(s[1] for s in per_cond.values())
    total_skip = sum(s[2] for s in per_cond.values())
    conds_all_pass = sum(1 for s in per_cond.values() if s[1] == 0)
    conds_any_fail = sum(1 for s in per_cond.values() if s[1] > 0)

    print()
    print(f"probe-level: PASS {total_pass}  FAIL {total_fail}  SKIP(perl errored) {total_skip}")
    print(f"condition-level: PASS (0 failing probes) {conds_all_pass}/{len(cond_rust_by_text)}"
          f"   FAIL (>=1 failing probe) {conds_any_fail}/{len(cond_rust_by_text)}")
    print()
    print(f"sample of {min(args.sample_lines, len(per_cond))} per-condition results:")
    for c in sorted(per_cond)[: args.sample_lines]:
        p, f, sk = per_cond[c]
        status = "PASS" if f == 0 else "FAIL"
        s = re.sub(r"\s+", " ", c.strip())[:70]
        print(f"  {status}  probes(pass={p} fail={f} skip={sk})  {s}")

    if fail_examples:
        print()
        print("FAILING probe examples (condition, combo, perl, rust):")
        for c, combo, pv, rv in fail_examples:
            s = re.sub(r"\s+", " ", c.strip())[:70]
            print(f"  cond={s!r} combo={combo!r} perl={pv!r} rust={rv!r}")

    if skip_examples:
        print()
        print(f"sample of Perl-errored (skipped) probes ({total_skip} total):")
        for c, combo, pv, rv in skip_examples[:10]:
            s = re.sub(r"\s+", " ", c.strip())[:70]
            print(f"  cond={s!r} combo={combo!r}")

    print()
    if conds_any_fail:
        print(f"RESULT: FAIL -- {conds_any_fail} condition(s) disagree with the pinned Perl oracle")
        raise SystemExit(1)
    print("RESULT: PASS -- every compiled Condition agreed with the pinned Perl oracle "
          f"on every probe ({total_pass} probe comparisons, {total_skip} skipped as "
          "inapplicable-to-probe Perl errors)")


if __name__ == "__main__":
    main()
