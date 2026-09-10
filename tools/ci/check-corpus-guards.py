#!/usr/bin/env python3
"""Fail if a #[test] reads the pinned corpus without gating on availability.

The corpus at /tmp/oxidex-exiftool-cache/combined-samples is a local developer
cache, absent on CI runners. An unguarded test that reads it panics on NotFound,
and because nextest is fail-fast, one such panic aborts the whole suite -- which
is how main stayed red for two days while looking like a single broken test.

This is a lexical check of literal paths in test bodies, not a Rust parser or
data-flow analysis. Module constants and indirect/helper reads are outside its
scope. Comments and literals do not contribute braces or guard expressions.
Accepted guards are standalone negative availability checks with a direct return:
the repository-wide helper, or Path::new(the same literal).is_file()/exists().
Unknown guard forms remain review findings rather than silently passing.
"""
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
from rust_source import lexical_source

CORPUS = "oxidex-exiftool-cache/combined-samples"
GUARD = "pinned_corpus_available"


def delimiter_pairs(code):
    stack, pairs = [], {}
    for pos, char in enumerate(code):
        if char in "{([":
            stack.append(pos)
        elif char in "})]" and stack:
            pairs[stack.pop()] = pos
    return pairs


TEST = re.compile(
    r"#\s*\[\s*test\s*\]\s*(?:#\[[^\]]*\]\s*)*"
    r"(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|unsafe)\s+)*fn\s+\w+\s*[^;{]*\{"
)
SKIP = re.compile(
    r"(?<=[;{}])\s*if\s*!\s*(?:"
    r"(?:crate::test_support::)?pinned_corpus_available\s*\(\s*\)"
    r"|(?:std::path::)?Path::new\s*\(\s*(?P<path>@)\s*\)\s*"
    r"\.\s*(?:is_file|exists)\s*\(\s*\))\s*\{"
)
# Plain diagnostic strings are not reads. Calls used as formatting arguments
# are deliberately excluded: eprintln!("{:?}", fs::read("...")) still reads.
DIAGNOSTIC = re.compile(r"\beprintln!\s*\(\s*@\s*(?:,\s*@\s*)*,?\s*\)")
RETURN = re.compile(r"[;{}]\s*(?P<statement>return)\s*;")


def violations(source):
    if CORPUS not in source:
        return []
    code, literals = lexical_source(source)
    pairs = delimiter_pairs(code)

    def parents(pos):
        return {start for start, end in pairs.items() if start < pos < end}

    bad = []
    for test in TEST.finditer(code):
        start = test.end() - 1
        end = pairs.get(start, len(code))
        guards = []
        for skip in SKIP.finditer(code, start, end):
            opening = skip.end() - 1
            closing = pairs.get(opening, end)
            # A return inside another if, closure, or macro argument does not
            # establish this guard. Only a direct return statement counts.
            returns = RETURN.finditer(code, opening, closing)
            if not any(parents(ret.start("statement")) == parents(opening) | {opening}
                       for ret in returns):
                continue
            path = literals[skip.start("path")] if skip["path"] else None
            guards.append((skip.start(), opening, closing, parents(opening), path))
        diagnostics = list(DIAGNOSTIC.finditer(code, start, end))
        for pos, literal in literals.items():
            if not start < pos < end or CORPUS not in literal:
                continue
            if any(match.start() < pos < match.end() for match in diagnostics):
                continue
            if any(begin < pos < opening or (
                    closing < pos and scope <= parents(pos)
                    and (path is None or path == literal))
                   for begin, opening, closing, scope, path in guards):
                continue
            bad.append(pos)
    return sorted(set(bad))


def main():
    bad = []
    roots = [pathlib.Path("src"), pathlib.Path("tests")]
    for path in sorted(x for root in roots for x in root.rglob("*.rs")):
        source = path.read_text()
        lines = source.splitlines()
        for pos in violations(source):
            line = source.count("\n", 0, pos)
            bad.append(f"{path}:{line + 1}  {lines[line].lstrip()[:70]}")
    if bad:
        print("Unguarded corpus-path literals in #[test] bodies:\n")
        for finding in bad:
            print("  " + finding)
        print(f"\n{len(bad)} violation(s). Gate each on "
              f"`if !crate::test_support::{GUARD}() {{ return; }}`.")
        return 1
    print("OK: no unguarded corpus-path literals in #[test] bodies "
          "(constant aliases and indirect reads are outside this lexical check).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
