#!/usr/bin/env python3
"""Coarse enum-fingerprint drift check for hand-embedded ExifTool PrintConv
literals that have no committed generator.

Tag-machinery overhaul Step 16, additional scope: six generated files exist
with no committed generator at all (see docs/TRANSCRIPTION.md's "Honest
limits" and OVERHAUL_OXIDEX_PLAN.md's Stage 3 exit criteria) --
`sony/{enciphered,plain,main_extra}_tables.rs`,
`nikon/{encrypted,settings}_tables.rs`, `minolta_a100_tables.rs` -- plus
`canon.rs`'s 175+ `const_decoder!`/`bitfield_decoder!` arms, which carry the
same problem at smaller scale (a committed generator exists for
`custom_functions2_tables.rs` but not for the bulk of canon.rs's own inline
decoders). Each was "read out of ExifTool's own hash in-process rather than
retyped" by a human, once, and nothing re-checks that reading against a
version bump.

None of these files' internal structure (which specific `BinTag`/`SettingsTag`
uses which named `M<n>`/`PC_<n>` array, at which byte offset, under which
`Condition`) is mechanically comparable to `dump_tables.pl`'s output without
re-implementing each file's bespoke DSL -- that is exactly the "distinct DSL
per file" problem TRANSCRIPTION.md documents, and the reason these six were
NOT reconstructed as generators in Step 14. This script does NOT attempt that.

What it checks instead, deliberately coarse: it extracts every literal
`(key, "value")` pair textually present in the target Rust file(s) -- this
catches every `static M<n>`/`const PC_<n>`/`const_decoder!`/`bitfield_decoder!`
array regardless of which named constant holds it -- and treats it as a
multiset of claimed ExifTool facts. Separately, it unions every PrintConv
enum-map and list-item entry across every table in the named ExifTool
module(s) from a `dump_tables.pl` JSON dump, as a multiset of facts ExifTool
actually declares right now. It reports:

  * RUST_ONLY: (key, value) pairs the Rust file states that appear in NO
    table of the named module(s) at all. This is the actionable number --
    each one is either upstream drift (ExifTool renamed/removed the value),
    or the pair belongs to a module this invocation did not list.
  * a baseline count, committed alongside the target file, that the checker
    diffs against on every run so an increase is a red CI check.

This is a fingerprint, not a field-level correspondence -- it will not catch
a value that ExifTool renamed to another value ALSO present somewhere else in
the same module (a false negative), and a RUST_ONLY hit needs a human to
confirm it is real drift and not, e.g., a value legitimately borrowed from a
different manufacturer's shared constant. Named and counted beats invisible;
it does not claim to be exact.

Usage:
    check_hand_enum_drift.py --dump <tables.json> --baseline <baseline.json> \
        [--update-baseline] <target.rs>=<Module1>,<Module2> [...]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

# Matches a literal `(KEY, "VALUE")` pair where KEY is a decimal or hex
# integer literal (optionally negative, optionally suffixed `u32`/`i32`/...,
# optionally parenthesized on its own line) or a quoted string, and VALUE is a
# Rust string literal. This is intentionally permissive about whitespace and
# line breaks between the two, since `cargo fmt` reflows some of these arrays
# onto many lines.
PAIR_RE = re.compile(
    r"""\(\s*
        (?:
            "(?P<qkey>(?:[^"\\]|\\.)*)"
          | (?P<key>-?(?:0x[0-9a-fA-F]+|\d+))(?:[iu](?:8|16|32|64|size))?
        )
        \s*,\s*
        "(?P<value>(?:[^"\\]|\\.)*)"
        \s*\)""",
    re.VERBOSE,
)


def normalize_key(raw_key: str | None, raw_qkey: str | None) -> str:
    if raw_qkey is not None:
        # A quoted numeric key ("0", "-32768", ...) normalizes the same way
        # an unquoted one does, since the M-arrays in the six files key by
        # quoted decimal string but mean the same integer ExifTool's hash
        # key does.
        try:
            return str(int(raw_qkey))
        except ValueError:
            return raw_qkey
    assert raw_key is not None
    if raw_key.lower().startswith(("0x", "-0x")):
        neg = raw_key.startswith("-")
        digits = raw_key[3:] if neg else raw_key[2:]
        val = int(digits, 16)
        return str(-val if neg else val)
    return str(int(raw_key))


def unescape(s: str) -> str:
    return s.encode().decode("unicode_escape")


def extract_rust_pairs(path: Path) -> set[tuple[str, str]]:
    text = path.read_text()
    pairs = set()
    for m in PAIR_RE.finditer(text):
        key = normalize_key(m.group("key"), m.group("qkey"))
        value = unescape(m.group("value"))
        pairs.add((key, value))
    return pairs


def iter_dump_pairs(modules: dict, module_names: list[str]):
    for mod_name in module_names:
        mod = modules.get(mod_name)
        if not mod:
            raise SystemExit(f"module {mod_name!r} missing from dump")
        for table in mod.get("tables", {}).values():
            for tag in table.get("tags", {}).values():
                yield from _pairs_from_tag(tag)


def _pairs_from_conv(conv) -> list[tuple[str, str]]:
    if not isinstance(conv, dict):
        return []
    kind = conv.get("kind")
    out = []
    if kind == "enum":
        for k, v in (conv.get("map") or {}).items():
            if isinstance(v, str):
                out.append((_norm_dump_key(k), v))
    elif kind == "list":
        for item in conv.get("items") or []:
            if isinstance(item, dict):
                for k, v in item.items():
                    if isinstance(v, str) and not k.startswith("_"):
                        out.append((_norm_dump_key(k), v))
    return out


def _norm_dump_key(k: str) -> str:
    try:
        return str(int(k))
    except ValueError:
        return k


def _pairs_from_tag(tag: dict):
    if not isinstance(tag, dict):
        return
    for variant in tag.get("_variants") or []:
        yield from _pairs_from_tag(variant)
    for conv_key in ("PrintConv", "ValueConv"):
        yield from _pairs_from_conv(tag.get(conv_key))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dump", required=True, help="dump_tables.pl JSON output")
    ap.add_argument("--baseline", required=True, help="baseline JSON to compare/update")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument(
        "targets",
        nargs="+",
        help="<path/to/file.rs>=<Module1>,<Module2>,... (repeatable)",
    )
    args = ap.parse_args()

    with open(args.dump, encoding="utf-8") as f:
        modules = json.load(f)["modules"]

    baseline_path = Path(args.baseline)
    baseline = json.loads(baseline_path.read_text()) if baseline_path.exists() else {}

    overall_ok = True
    results = {}
    for target in args.targets:
        rust_path_s, _, mod_list_s = target.partition("=")
        if not mod_list_s:
            raise SystemExit(f"target {target!r} missing '=<Module,...>'")
        rust_path = Path(rust_path_s)
        module_names = mod_list_s.split(",")

        rust_pairs = extract_rust_pairs(rust_path)
        dump_pairs = set(iter_dump_pairs(modules, module_names))
        rust_only = sorted(rust_pairs - dump_pairs)

        key = rust_path_s
        results[key] = {
            "rust_pair_count": len(rust_pairs),
            "dump_pair_count": len(dump_pairs),
            "rust_only_count": len(rust_only),
        }

        expected = baseline.get(key, {}).get("rust_only_count")
        status = "OK"
        if expected is None:
            status = "NO BASELINE" if not args.update_baseline else "BASELINED"
        elif len(rust_only) > expected:
            status = f"REGRESSED (baseline {expected})"
            overall_ok = False
        elif len(rust_only) < expected:
            status = f"IMPROVED (baseline {expected}, update baseline)"
            overall_ok = False

        print(
            f"{key}: rust_pairs={len(rust_pairs)} dump_pairs={len(dump_pairs)} "
            f"rust_only={len(rust_only)} [{status}]"
        )
        if status.startswith("REGRESSED") and rust_only:
            sample = rust_only[:15]
            for k, v in sample:
                print(f"    rust-only: ({k!r}, {v!r})")
            if len(rust_only) > len(sample):
                print(f"    ... and {len(rust_only) - len(sample)} more")

    if args.update_baseline:
        for key, r in results.items():
            baseline[key] = {"rust_only_count": r["rust_only_count"]}
        baseline_path.write_text(json.dumps(baseline, indent=1, sort_keys=True) + "\n")
        print(f"baseline written to {baseline_path}")
        return 0

    return 0 if overall_ok else 1


if __name__ == "__main__":
    sys.exit(main())
