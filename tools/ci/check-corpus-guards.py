#!/usr/bin/env python3
"""Fail if a #[test] reads the pinned corpus without gating on availability.

The corpus at /tmp/oxidex-exiftool-cache/combined-samples is a local developer
cache, absent on CI runners. An unguarded test that reads it panics on NotFound,
and because nextest is fail-fast, one such panic aborts the whole suite -- which
is how main stayed red for two days while looking like a single broken test.
"""
import re, sys, pathlib

CORPUS = "oxidex-exiftool-cache/combined-samples"
GUARD = "pinned_corpus_available"
bad = []
roots = [pathlib.Path("src"), pathlib.Path("tests")]
for f in sorted(x for r in roots for x in r.rglob("*.rs")):
    lines = f.read_text().splitlines()
    for i, line in enumerate(lines):
        if CORPUS not in line:
            continue
        stripped = line.lstrip()
        if stripped.startswith("//") or stripped.startswith("*"):
            continue  # doc/comment reference is fine
        if "is_file()" in stripped or ".exists()" in stripped or "eprintln!" in stripped:
            continue  # this line IS the guard, not a guarded read
        j = i
        while j >= 0 and not re.match(r"\s*(async\s+)?fn\s+\w+", lines[j]):
            j -= 1
        if j < 0:
            continue
        if not any("#[test]" in lines[k] for k in range(max(0, j - 6), j)):
            continue  # not a test fn
        body = "\n".join(lines[j:i])
        guarded = GUARD in body or "is_file()" in body or ".exists()" in body
        if not guarded:
            bad.append(f"{f}:{i+1}  {stripped[:70]}")

if bad:
    print("Unguarded pinned-corpus reads in #[test] functions:\n")
    for b in bad:
        print("  " + b)
    print(f"\n{len(bad)} violation(s). Gate each on "
          f"`if !crate::test_support::{GUARD}() {{ return; }}`.")
    sys.exit(1)
print("OK: every pinned-corpus test is guarded.")
