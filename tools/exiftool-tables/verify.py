#!/usr/bin/env python3
"""Check generated Rust against ExifTool, by parsing the Rust back out.

The property tested is SOUNDNESS, not completeness:

    every field, every enum entry and every mask present in the generated Rust
    must match ExifTool exactly, and the release it was transcribed from must be
    the one the repo pins.

Completeness is a separate question, already answered by codegen.py's own
skip accounting. Splitting the two matters. A generator that silently drops
hard cases scores well on "does everything I emitted match?" and badly on
"did I emit everything?", and only reporting both keeps the coverage number
honest. Conflating them is how a project ends up claiming 58% parity while
extracting 48.8%.

The Rust is parsed rather than trusted from the intermediate JSON: the JSON is
the codegen's *input*, so comparing against it would test nothing about the
codegen. Reading back what was actually written catches escaping bugs, integer
overflow in enum keys, sort-order mistakes that break binary_search, and
truncation -- the failures that compile perfectly.
"""

import argparse
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

# The repo root, derived from this file's location rather than the working
# directory: CI, the justfile and regen.sh all invoke this script from
# different places, and a cwd-relative pin lookup would silently read nothing.
REPO_ROOT = Path(__file__).resolve().parents[2]
PIN_FILE = REPO_ROOT / ".exiftool-version"

# Whitespace-tolerant on purpose: the generated file is run through rustfmt
# before it is committed, which wraps every `Field { .. }` across several lines.
# The original single-line patterns silently matched nothing after that change,
# and a verifier that parses zero fields reports no mismatches -- it looked like
# a pass. `parse_rust` now also asserts it accounted for every `Field {` in the
# file, so under-parsing fails loudly instead of quietly.
TABLE_RE = re.compile(
    r'pub static \w+: BinaryTable = BinaryTable \{\s*'
    r'module:\s*"(?P<module>[^"]*)",\s*'
    r'table:\s*"(?P<table>[^"]*)",\s*'
    r'group0:\s*"[^"]*",\s*'
    r'group2:\s*"[^"]*",\s*'
    r'first_entry:\s*-?\d+,\s*'
    r'default_format:\s*Fmt::\w+,\s*'
    r'offsets_sound_until:\s*(?P<sound_until>None|Some\(-?\d+\)),',
)
FIELD_RE = re.compile(
    r'Field\s*\{\s*'
    r'index:\s*(?P<index>-?\d+),\s*'
    r'sub:\s*(?P<sub>None|Some\(\d+\)),\s*'
    r'name:\s*"(?P<name>(?:[^"\\]|\\.)*)",\s*'
    r'format:\s*(?P<fmt>None|Some\(Fmt::\w+(?:\(\d+\))?\)),\s*'
    r'count:\s*(?P<count>\d+),\s*'
    r'mask:\s*(?:None'
    r'|Some\(\s*Mask\s*\{\s*bits:\s*(?P<mask_bits>0[xX][0-9a-fA-F_]+|\d+),\s*'
    r'shift:\s*(?P<mask_shift>\d+),?\s*\}\s*,?\s*\)),\s*'
    r'omitted:\s*(?P<omitted>Omitted::NONE|Omitted\s*\{[^{}]*\}),\s*'
    r'print_conv:\s*(?P<pc>PrintConv::(?:None'
    # Whitespace/trailing-comma tolerant, not just `Expr\(ExprId::\w+\)`:
    # rustfmt wraps `Expr(ExprId::VeryLongGeneratedIdentifier)` onto its own
    # line with a trailing comma once the identifier is long enough to blow
    # the line-width limit -- invisible until Step 23 started compiling
    # `_variants` alternatives whose `ExprId` names (built from the full
    # normalized Perl expression, e.g.
    # `ImageExifToolExifPrintExposureTimeVal6037F3`) are long enough to
    # trigger it. A tag whose `pc` group fails to match here is silently
    # invisible to every check below it, not merely misparsed, so this is a
    # completeness bug, not a cosmetic one.
    r'|Expr\(\s*ExprId::\w+\s*,?\s*\)'
    r'|IntEnum\(&\[.*?\]\)'
    r'|StrEnum\(&\[.*?\]\)))'
    # Step 27: `subdir:` is the last field. Its value (`None` or
    # `Some(SubdirEdge { ... })`, arbitrarily nested via `Start::Expr(&...)`)
    # is NOT captured here -- a fixed-depth regex cannot bound arbitrary
    # nesting safely, the same reason enum bodies are rescanned by
    # `_enum_body` instead of captured inline. This only anchors the offset
    # `_value_span` starts scanning from; see `_parse_one_field`.
    r',\s*subdir:\s*',
    re.S,
)
FIELD_COUNT_RE = re.compile(r'Field\s*\{\s*index:')
# Step 23: a table's `variants: &[VariantGroup { index: N, sub: S,
# alternatives: &[(Cond, Field), ...] }, ...]` holds `Field` literals too --
# ExifTool's `_variants` alternatives, several of which share one `index`/
# `sub` with each other. Scanning `Field {` blindly across a whole table's
# byte range (as this file did before Step 23) pulls those in as if they
# were `fields:` entries and collides them under one `(module, table, key)`
# dict key, silently discarding all but the last alternative -- caught by
# `parse_rust`'s own "every Field in the file must have been parsed" check
# (a real regression during Step 23's development: 6895 `Field {` in the
# file, only 6769 survived the dict merge). `FIELDS_MARKER_RE`/
# `VARIANTS_MARKER_RE` locate each array's own `&[...]` span so the two
# populations are scanned, and keyed, separately.
FIELDS_MARKER_RE = re.compile(r"fields:\s*&\[")
VARIANTS_MARKER_RE = re.compile(r"variants:\s*&\[")
VARIANT_GROUP_RE = re.compile(
    r"VariantGroup\s*\{\s*"
    r"index:\s*(?P<index>-?\d+),\s*"
    r"sub:\s*(?P<sub>None|Some\(\d+\)),\s*"
    r"alternatives:\s*&\["
)
VERSION_RE = re.compile(r'pub const EXIFTOOL_VERSION: &str = "([^"]+)";')
# The `,?` before the closing paren matters: rustfmt wraps long tuples onto
# multiple lines and leaves a trailing comma before `)`, and a pattern that
# refuses that comma silently skips every wrapped entry.
INT_PAIR_RE = re.compile(r'\(\s*(-?\d+),\s*"((?:[^"\\]|\\.)*)"\s*,?\s*\)')
STR_PAIR_RE = re.compile(r'\(\s*"((?:[^"\\]|\\.)*)",\s*"((?:[^"\\]|\\.)*)"\s*,?\s*\)')


def _bracket_span(src, open_bracket):
    """(start, end) of the text strictly between the `[` at `open_bracket`
    and its true matching `]`, honoring string literals and escapes so a `]`
    inside a Rust string literal (an enum description, a tag name) does not
    terminate the span early. Shared by `_enum_body` (which additionally
    counts top-level tuples) and `parse_rust`'s `fields: &[...]` /
    `variants: &[...]` / `alternatives: &[...]` span-finding.
    """
    i = open_bracket + 1
    depth, in_str = 1, False
    while i < len(src):
        c = src[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c == "[":
            depth += 1
        elif c == "]":
            depth -= 1
            if depth == 0:
                return open_bracket + 1, i
        i += 1
    raise SystemExit("unterminated `[...]` in generated file")


_OPENERS = {"(", "[", "{"}
_CLOSERS = {")", "]", "}"}


def _value_span(src, start):
    """From `start` (the first character of a struct-literal field's VALUE,
    e.g. the `N` of `None` or `S` of `Some(...)`), return the end index
    (exclusive) of that value: the position of the top-level comma that ends
    it, or of the unmatched closing brace that ends the enclosing `Field {
    ... }` literal when this was the last field (rustfmt does not always
    leave a trailing comma before `}` on a single-line literal). Honors
    nested `()`/`[]`/`{}` of any kind and string literals/escapes, the same
    discipline `_bracket_span` applies to `[...]` alone -- needed here
    because `subdir: Some(SubdirEdge { ... })` nests a `{}` inside a `()`,
    and `Start::Expr(&StartExpr::Add(&StartExpr::DirStart, &StartExpr::Val))`
    nests `()` arbitrarily deep depending on which arithmetic shape
    `tools/exiftool-tables/subdirs.py` compiled.
    """
    i = start
    depth, in_str = 0, False
    while i < len(src):
        c = src[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
            i += 1
            continue
        if c == '"':
            in_str = True
        elif c in _OPENERS:
            depth += 1
        elif c in _CLOSERS:
            if depth == 0:
                return i
            depth -= 1
        elif c == "," and depth == 0:
            return i
        i += 1
    raise SystemExit("unterminated field value in generated file")


def _enum_body(src, open_bracket):
    """The text between `[` at open_bracket and its true matching `]`.

    Also returns the number of top-level `(..)` tuples inside it. A regex
    cannot do this job: enum descriptions may contain `])`, which truncated
    the lazy `.*?\\]\\)` capture and silently dropped the rest of that enum
    from verification. This scan honors string literals and escapes, via
    `_bracket_span` for the bracket-matching part.
    """
    start, end = _bracket_span(src, open_bracket)
    body = src[start:end]
    paren_depth, tuples, in_str, i = 0, 0, False, 0
    while i < len(body):
        c = body[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c == "(":
            if paren_depth == 0:
                tuples += 1
            paren_depth += 1
        elif c == ")":
            paren_depth -= 1
        i += 1
    return body, tuples


def unescape(s):
    return (s.replace('\\\\', '\x00')
             .replace('\\"', '"').replace('\\n', '\n')
             .replace('\\r', '\r').replace('\\t', '\t')
             .replace('\x00', '\\'))


# Step 27: the `Some(SubdirEdge { module: "...", table: "...", start: ...`
# prefix, up to (not including) the `start:` value itself -- `_parse_subdir_
# value` locates the rest (`start`/`base`) by scanning from there with
# `_value_span`, since their content nests arbitrarily (`Start::Expr(&Start
# Expr::Add(...))`) and a fixed-depth regex cannot bound that safely. The
# fixed `module, table, start, base, byte_order, validate` key order is
# `codegen.py`'s `compile_subdir`'s own emission order, never reordered.
_SUBDIR_EDGE_RE = re.compile(
    r'^Some\(\s*SubdirEdge\s*\{\s*'
    r'module:\s*"(?P<module>[^"]*)"\s*,\s*'
    r'table:\s*"(?P<table>[^"]*)"\s*,\s*'
    r'start:\s*',
    re.S,
)


def _parse_subdir_value(text):
    """`text` is the raw value captured for one field's `subdir:` struct
    member -- `"None"` or `"Some(SubdirEdge { ... })"` (rustfmt may have
    wrapped it across lines and/or added trailing commas; both are
    whitespace-tolerant here). Returns `None`, or `(module, table,
    start_text, base_text)` with the latter two left as raw (unparsed) Rust
    source -- `main()`'s independent grammar check (`_arith_is_well_formed`)
    reads that text directly rather than trusting a second parse of it."""
    text = text.strip()
    if text == "None":
        return None
    m = _SUBDIR_EDGE_RE.match(text)
    if not m:
        raise SystemExit(f"unrecognised subdir value {text!r}")
    start_begin = m.end()
    start_end = _value_span(text, start_begin)
    start_text = text[start_begin:start_end].strip()
    bm = re.match(r"\s*,\s*base:\s*", text[start_end:])
    if not bm:
        raise SystemExit(f"unrecognised subdir value (base) in {text!r}")
    base_begin = start_end + bm.end()
    base_end = _value_span(text, base_begin)
    base_text = text[base_begin:base_end].strip()
    return m.group("module"), m.group("table"), start_text, base_text


def _parse_one_field(src, f, k, fields, enums, masks, hooks, subdirs, subdir_edges):
    """Populate `fields`/`enums`/`masks`/`hooks`/`subdirs`/`subdir_edges` from
    one `FIELD_RE` match `f`, under dict key `k`. Shared by the plain-`fields:`
    scan and the `variants:` scan below -- a `Field {...}` literal means the
    same thing in both places, only what `k` looks like differs (see
    `parse_rust`)."""
    fields[k] = unescape(f.group("name"))

    bits = f.group("mask_bits")
    if bits is not None:
        masks[k] = (int(bits, 0), int(f.group("mask_shift")))

    omitted = f.group("omitted")
    if "hook: true" in omitted:
        hooks.add(k)
    if "subdirectory: true" in omitted:
        subdirs.add(k)

    # `FIELD_RE` consumes `,\s*subdir:\s*` but does not capture the value
    # itself (unbounded nesting -- see the pattern's own comment), so its
    # value starts exactly at the whole match's end.
    subdir_val_end = _value_span(src, f.end())
    subdir_edges[k] = _parse_subdir_value(src[f.end():subdir_val_end])

    pc = f.group("pc")
    # Rescan the enum body from the source itself rather than
    # trusting the regex capture -- see _enum_body for why.
    for marker, pair_re, int_keys in (
        ("PrintConv::IntEnum(&[", INT_PAIR_RE, True),
        ("PrintConv::StrEnum(&[", STR_PAIR_RE, False),
    ):
        if not pc.startswith(marker):
            continue
        body, expected_pairs = _enum_body(src, f.start("pc") + len(marker) - 1)
        pairs = pair_re.findall(body)
        # Exact per-enum accounting, the same discipline the field
        # count gets below: a pair the pattern cannot read must fail
        # the run, not silently shrink the verified surface.
        if len(pairs) != expected_pairs:
            raise SystemExit(
                f"enum for {k}: parsed {len(pairs)} of "
                f"{expected_pairs} entries -- the verifier's pair "
                "pattern is out of date; fix it before trusting a PASS"
            )
        for kk, vv in pairs:
            if int_keys:
                enums[k][kk] = unescape(vv)
            else:
                enums[k][unescape(kk)] = unescape(vv)
        break


def parse_rust(path):
    """-> (fields, enums, masks, hooks, subdirs, subdir_edges, sound_until, variant_keys)

    fields{k->name}, enums{k->{key:val}}, masks{k->(bits,shift)},
    hooks{k}, subdirs{k} (sets of field keys with that Omitted flag set),
    subdir_edges{k -> None | (module, table, start_text, base_text)} (Step
    27: every field's `subdir:` value, present or not -- unlike the other
    dicts this one always has an entry for every field, `None` included, so
    `main()` can tell "no edge" from "field not parsed"),
    sound_until{(mod,tbl)->int|None} (the table's offsets_sound_until).

    `fields`/`enums`/`masks`/`hooks`/`subdirs`/`subdir_edges` hold BOTH plain
    `fields:` entries (keyed `(mod, tbl, "22")`, exactly as before Step 23)
    and `variants:` alternatives (keyed `(mod, tbl, "22#0")`, `(mod, tbl,
    "22#1")`, ... -- 0-based array position, matching `oracle.pl`'s own
    `"$k#$i"` key for a `_variants` alternative). The two key spaces cannot
    collide: a plain key is never built with `#` in it. `variant_keys` is the
    set of the latter, so `main()` can report them as their own column
    without a second, parallel set of dicts to keep in sync.
    """
    with open(path, encoding="utf-8") as fh:
        src = fh.read()

    fields, enums, masks = {}, defaultdict(dict), {}
    hooks, subdirs = set(), set()
    subdir_edges = {}
    sound_until = {}
    variant_keys = set()
    bounds = [(m.start(), m.group("module"), m.group("table"), m.group("sound_until"))
              for m in TABLE_RE.finditer(src)]
    for start, mod, tbl, su in bounds:
        sound_until[(mod, tbl)] = None if su == "None" else int(su[len("Some("):-1])
    bounds = [(start, mod, tbl) for start, mod, tbl, _su in bounds]
    bounds.append((len(src), None, None))

    expected_plain = 0
    expected_variant = 0
    for i in range(len(bounds) - 1):
        start, mod, tbl = bounds[i]
        end = bounds[i + 1][0]

        # `fields: &[...]` -- exactly the pre-Step-23 population, now scoped
        # to its own array span so a `variants:` array later in the same
        # table's static literal can never contribute a `Field {...}` to
        # this scan (see FIELDS_MARKER_RE's module-level doc comment for the
        # collision this fixes).
        fm = FIELDS_MARKER_RE.search(src, start, end)
        if fm:
            f_start, f_end = _bracket_span(src, fm.end() - 1)
            expected_plain += len(FIELD_COUNT_RE.findall(src, f_start, f_end))
            for f in FIELD_RE.finditer(src, f_start, f_end):
                sub = f.group("sub")
                idx = f.group("index")
                # Sub-indexed bit-fields share a byte offset; the oracle keys
                # them by ExifTool's original "12.1" string, so rebuild that
                # form.
                key = idx if sub == "None" else f"{idx}.{sub[5:-1]}"
                _parse_one_field(
                    src, f, (mod, tbl, key), fields, enums, masks, hooks, subdirs, subdir_edges
                )

        # `variants: &[VariantGroup { index, sub, alternatives: &[(Cond,
        # Field), ...] }, ...]` -- each `VariantGroup`'s own `alternatives`
        # span is scanned independently so the 0-based position within IT
        # (not within the table as a whole) becomes the `#i` suffix, exactly
        # matching the array position `oracle.pl` numbers its `_variants`
        # alternatives by.
        vm = VARIANTS_MARKER_RE.search(src, start, end)
        if vm:
            v_start, v_end = _bracket_span(src, vm.end() - 1)
            for gm in VARIANT_GROUP_RE.finditer(src, v_start, v_end):
                sub = gm.group("sub")
                idx = gm.group("index")
                base_key = idx if sub == "None" else f"{idx}.{sub[5:-1]}"
                a_start, a_end = _bracket_span(src, gm.end() - 1)
                expected_variant += len(FIELD_COUNT_RE.findall(src, a_start, a_end))
                for pos, f in enumerate(FIELD_RE.finditer(src, a_start, a_end)):
                    k = (mod, tbl, f"{base_key}#{pos}")
                    variant_keys.add(k)
                    _parse_one_field(
                        src, f, k, fields, enums, masks, hooks, subdirs, subdir_edges
                    )

    # Every Field in the file must have been parsed -- separately for each
    # population, so a formatting change that defeats one pattern (say,
    # `variants:`'s nested tuples) cannot hide behind the other population's
    # correct count and still read as a clean PASS.
    got_plain = len(fields) - len(variant_keys)
    if got_plain != expected_plain:
        raise SystemExit(
            f"parsed {got_plain} plain fields but `fields:` arrays contain "
            f"{expected_plain} -- the verifier's pattern is out of date; fix "
            "it before trusting a PASS"
        )
    if len(variant_keys) != expected_variant:
        raise SystemExit(
            f"parsed {len(variant_keys)} variant alternatives but "
            f"`variants:` arrays contain {expected_variant} -- the "
            "verifier's pattern is out of date; fix it before trusting a PASS"
        )
    return fields, enums, masks, hooks, subdirs, subdir_edges, sound_until, variant_keys


def load_oracle(lib, oracle_pl):
    out = subprocess.run(
        ["perl", oracle_pl, lib],
        capture_output=True, check=True, text=True, encoding="utf-8",
    ).stdout
    names, enums, masks = {}, defaultdict(dict), {}
    hooks, subdirs, varfmts = set(), set(), set()
    # Step 27: the raw SubDirectory facts behind each `subdirs` membership --
    # {(mod,sym,key) -> {"tagtable":str, "start":str, "base":str,
    # "processproc":bool, "byteorder":bool, "validate":bool}}, independent of
    # dump_tables.pl/codegen.py/subdirs.py (oracle.pl reads the live Perl
    # hash directly). `main()` uses this to independently decide whether
    # `codegen.py`'s SubdirEdge compiler should have modeled or refused each
    # field, and cross-checks that decision against what actually landed in
    # the generated Rust.
    subdir_facts = {}
    for line in out.splitlines():
        p = line.split("\t")
        if len(p) == 4:
            names[(p[0], p[1], p[2])] = p[3]
        elif len(p) == 6 and p[3] == "ENUM":
            enums[(p[0], p[1], p[2])][p[4]] = p[5]
        elif len(p) == 6 and p[3] == "MASK":
            masks[(p[0], p[1], p[2])] = (int(p[4], 0), int(p[5]))
        elif len(p) == 5 and p[3] == "HOOK":
            hooks.add((p[0], p[1], p[2]))
        elif len(p) == 10 and p[3] == "SUBDIR":
            key = (p[0], p[1], p[2])
            subdirs.add(key)
            subdir_facts[key] = {
                "tagtable": p[4],
                "start": p[5],
                "base": p[6],
                "processproc": p[7] == "1",
                "byteorder": p[8] == "1",
                "validate": p[9] == "1",
            }
        elif len(p) == 5 and p[3] == "VARFMT":
            varfmts.add((p[0], p[1], p[2]))
    return names, enums, masks, hooks, subdirs, subdir_facts, varfmts


def oracle_version(lib):
    """The ExifTool release living in `lib`, read the same way the oracle does."""
    return subprocess.run(
        ["perl", f"-I{lib}", "-e",
         "require Image::ExifTool; print $Image::ExifTool::VERSION"],
        capture_output=True, check=True, text=True, encoding="utf-8",
    ).stdout.strip()


def repo_pin():
    """The ExifTool release this repo grades everything against."""
    try:
        pin = PIN_FILE.read_text(encoding="utf-8").strip()
    except OSError as exc:
        raise SystemExit(f"cannot read the ExifTool pin at {PIN_FILE}: {exc}") from exc
    if not pin:
        raise SystemExit(f"{PIN_FILE} is empty -- it must name one ExifTool release")
    return pin


def check_version(generated_rs, lib):
    """Refuse to verify unless artifact, oracle and repo pin are the same release.

    Two distinct skews are possible, and only the second used to be caught:

      stamp vs pin    the committed tables were transcribed from a release the
                      repo no longer grades against. This is the one that hides:
                      `.exiftool-version` is the declared source of truth, but
                      this check used to take its expected release from the
                      artifact's own stamp -- and both callers (the justfile
                      recipe and CI's verify-tables job) picked which ExifTool
                      to fetch from that same stamp. The loop closed on itself,
                      so a table set frozen at 13.30 verified against 13.30 and
                      reported PASS forever while every coverage number in the
                      repo was measured against 13.59. Anchoring to the pin is
                      what makes staleness expressible at all.

      stamp vs lib    you pointed the verifier at the wrong ExifTool. Without
                      this, every field ExifTool has since renamed and every
                      enum value it has inserted counts as a mismatch, which
                      reads as "the transcription is wrong" -- a diagnosis that
                      costs far more to reach from several hundred plausible
                      differences than from one line.
    """
    with open(generated_rs, encoding="utf-8") as fh:
        m = VERSION_RE.search(fh.read())
    if not m:
        raise SystemExit(
            f"{generated_rs} carries no EXIFTOOL_VERSION stamp -- regenerate it "
            "with `just regen-tables`; an unstamped table set cannot be verified"
        )
    stamped, pinned = m.group(1), repo_pin()
    if stamped != pinned:
        raise SystemExit(
            f"ExifTool pin skew: {generated_rs} was transcribed from {stamped}, "
            f"but {PIN_FILE.name} pins {pinned}.\nThe pin is the only source of "
            "truth for which release this repo grades against, so tables from "
            "any other release are stale by definition -- regenerate them with "
            "`just regen-tables`.\nIf you meant to move the whole repo to "
            f"{stamped}, change {PIN_FILE.name} first, then regenerate."
        )
    actual = oracle_version(lib)
    if stamped != actual:
        raise SystemExit(
            f"ExifTool version skew: tables were generated from {stamped}, but "
            f"{lib} is {actual}.\nPoint the verifier at the pinned release "
            f"({pinned}) -- `just verify-tables` fetches it for you."
        )
    return stamped


def norm_key(k):
    """ExifTool enum keys may be decimal or hex; compare numerically."""
    try:
        return str(int(str(k), 0))
    except ValueError:
        return str(k)


# Step 27: independent (deliberately NOT importing `tools/exiftool-tables/
# subdirs.py`) re-derivation of whether a SubDirectory's Start/Base source
# text is plausibly inside the closed arithmetic grammar `subdirs.py`
# compiles -- see that module and `src/exiftool_tables/subdir.rs` for the
# grammar itself (integers, `$val`/`$dirStart` or `$start`/`$base`, `+`/`-`/
# `*`, parens). This is a tokenizer over the ORACLE's raw source string, not
# a full parser: it does not build a tree the way `subdirs.py`'s AST walk
# does, so it cannot catch every malformed arrangement of otherwise-valid
# tokens (`$val $val` tokenizes fine here despite being invalid Perl). What
# it does catch, independently of `subdirs.py`'s own logic, is the dangerous
# direction: a token this grammar cannot possibly reach (a function call, a
# comparison operator, an unknown `$variable`) appearing in an expression
# `codegen.py` nonetheless modeled as an edge.
_ARITH_TOKEN_RE = re.compile(r"\$\w+|\d+|[+\-*()]|\s+")


def _arith_is_well_formed(text, allowed_dollar_vars):
    pos = 0
    while pos < len(text):
        m = _ARITH_TOKEN_RE.match(text, pos)
        if not m:
            return False
        tok = m.group(0)
        if tok.startswith("$") and tok[1:] not in allowed_dollar_vars:
            return False
        pos = m.end()
    return True


_TAGTABLE_RE = re.compile(r"^Image::ExifTool::(\w+)::(\w+)$")


def expected_subdir_edge(fact):
    """Independently decide, from one oracle SUBDIR row's raw facts, whether
    `codegen.py`'s `compile_subdir` should have modeled an edge or refused
    it, and what module/table it should name if so. Returns `(module, table)`
    or `None` (refuse) -- mirrors `subdirs.py`'s decision tree by re-deriving
    it from the same primitive facts, not by calling into `subdirs.py`."""
    if fact["processproc"] or fact["byteorder"] or fact["validate"]:
        return None
    m = _TAGTABLE_RE.match(fact["tagtable"])
    if not m:
        return None
    start = fact["start"]
    if "$" in start and not _arith_is_well_formed(start, {"val", "dirStart"}):
        return None
    base = fact["base"]
    if base and not _arith_is_well_formed(base, {"start", "base"}):
        return None
    return m.group(1), m.group(2)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("generated_rs")
    ap.add_argument("exiftool_lib")
    ap.add_argument("--oracle", default="oracle.pl")
    ap.add_argument("--show", type=int, default=10)
    args = ap.parse_args()

    version = check_version(args.generated_rs, args.exiftool_lib)
    print(f"ExifTool {version}")
    (
        gen_fields, gen_enums, gen_masks, gen_hooks, gen_subdirs, gen_subdir_edges,
        gen_sound_until, gen_variant_keys,
    ) = parse_rust(args.generated_rs)
    or_names, or_enums, or_masks, or_hooks, or_subdirs, or_subdir_facts, or_varfmts = load_oracle(
        args.exiftool_lib, args.oracle
    )

    if not gen_fields:
        sys.exit("parsed 0 fields from generated Rust -- verifier is broken, "
                 "not the generator; fix the parser before trusting a PASS")

    # Step 23: every check below runs once over the WHOLE `gen_fields`/
    # `gen_enums`/`gen_masks` population (plain `fields:` entries and
    # `variants:` alternatives together -- `gen_variant_keys` says which is
    # which), but tallies two sets of counters so the report gains a
    # separate "variant" column instead of quietly folding `_variants`
    # alternatives into the pre-Step-23 numbers. A `_variants` alternative
    # is checked exactly like a plain field: same name/enum/mask truth
    # source (`oracle.pl`, keyed `"idx#i"` for these -- see this file's
    # `VARIANT_GROUP_RE` doc comment), same match/mismatch/orphan meaning.
    name_ok = name_bad = orphan = 0
    enum_ok = enum_bad = 0
    mask_ok = mask_bad = 0
    variant_name_ok = variant_name_bad = variant_orphan = 0
    variant_enum_ok = variant_enum_bad = 0
    variant_mask_ok = variant_mask_bad = 0
    bad_examples, orphan_examples, enum_examples = [], [], []
    mask_examples = []
    variant_bad_examples, variant_orphan_examples, variant_enum_examples = [], [], []
    variant_mask_examples = []

    for k, name in gen_fields.items():
        is_variant = k in gen_variant_keys
        truth = or_names.get(k)
        if truth is None:
            if is_variant:
                variant_orphan += 1
                if len(variant_orphan_examples) < args.show:
                    variant_orphan_examples.append(k)
            else:
                orphan += 1
                if len(orphan_examples) < args.show:
                    orphan_examples.append(k)
            continue
        if truth == name:
            if is_variant:
                variant_name_ok += 1
            else:
                name_ok += 1
        else:
            if is_variant:
                variant_name_bad += 1
                if len(variant_bad_examples) < args.show:
                    variant_bad_examples.append((k, name, truth))
            else:
                name_bad += 1
                if len(bad_examples) < args.show:
                    bad_examples.append((k, name, truth))

    for k, m in gen_enums.items():
        is_variant = k in gen_variant_keys
        truth = {norm_key(a): b for a, b in or_enums.get(k, {}).items()}
        for kk, vv in m.items():
            t = truth.get(norm_key(kk))
            if t == vv:
                if is_variant:
                    variant_enum_ok += 1
                else:
                    enum_ok += 1
            else:
                if is_variant:
                    variant_enum_bad += 1
                    if len(variant_enum_examples) < args.show:
                        variant_enum_examples.append((k, kk, vv, t))
                else:
                    enum_bad += 1
                    if len(enum_examples) < args.show:
                        enum_examples.append((k, kk, vv, t))

    # Masks decide what a field's value IS, not merely how it prints, so a
    # wrong one is a wrong number under a real tag name. Checked in both
    # directions: a mask the generator invented and a mask it silently dropped
    # are equally wrong, and only comparing the union catches the second.
    for k in set(gen_masks) | {k for k in or_masks if k in gen_fields}:
        is_variant = k in gen_variant_keys
        got, want = gen_masks.get(k), or_masks.get(k)
        if got == want:
            if is_variant:
                variant_mask_ok += 1
            else:
                mask_ok += 1
        else:
            if is_variant:
                variant_mask_bad += 1
                if len(variant_mask_examples) < args.show:
                    variant_mask_examples.append((k, got, want))
            else:
                mask_bad += 1
                if len(mask_examples) < args.show:
                    mask_examples.append((k, got, want))

    # Hook and SubDirectory are presence flags, not values: `codegen.py`'s
    # `omitted_for` sets them exactly when ExifTool's own hash carries the
    # corresponding key. Checked over every field the generator actually
    # emitted -- a flag on a field ExifTool does not carry is a false
    # positive (a caller refuses a field for no reason); a flag ExifTool
    # carries but the schema does not record is the silent drop Step 9
    # exists to close.
    hook_ok = hook_bad = 0
    subdir_ok = subdir_bad = 0
    hook_examples, subdir_examples = [], []
    for k in gen_fields:
        got, want = k in gen_hooks, k in or_hooks
        if got == want:
            hook_ok += 1
        else:
            hook_bad += 1
            if len(hook_examples) < args.show:
                hook_examples.append((k, got, want))
        got, want = k in gen_subdirs, k in or_subdirs
        if got == want:
            subdir_ok += 1
        else:
            subdir_bad += 1
            if len(subdir_examples) < args.show:
                subdir_examples.append((k, got, want))

    # Step 27: does `codegen.py`'s SubdirEdge compiler (`subdirs.py`) agree
    # with an INDEPENDENT re-derivation (`expected_subdir_edge`, built from
    # the oracle's raw TAGTABLE/START/BASE/PROCESSPROC/BYTEORDER/VALIDATE
    # facts, not by calling `subdirs.py`) about which SubDirectory-flagged
    # fields should carry a modeled edge, and whether the module/table it
    # names is right. Checked only over fields where BOTH sides agree the
    # flag itself is set (`subdir_bad` above already reports a flag
    # disagreement; comparing edges under a flag mismatch would just be
    # noise on top of a problem already reported).
    edge_ok = edge_bad = 0
    edge_examples = []
    for k in gen_fields:
        if k not in gen_subdirs or k not in or_subdirs:
            continue
        fact = or_subdir_facts.get(k)
        if fact is None:
            continue
        expected = expected_subdir_edge(fact)
        got = gen_subdir_edges.get(k)
        got_pair = (got[0], got[1]) if got is not None else None
        if got_pair == expected:
            edge_ok += 1
        else:
            edge_bad += 1
            if len(edge_examples) < args.show:
                edge_examples.append((k, got_pair, expected))

    # `offsets_sound_until` is a per-table derived fact: the index of the
    # first refused `var_*` field, recorded only when some emitted field of
    # that table actually sits past it (see codegen.py's `gen_table`).
    # Reconstructed here independently from the oracle's VARFMT rows plus the
    # field set this same parse just read back from the generated Rust --
    # not from codegen.py's own arithmetic, so a bug in that arithmetic
    # cannot cancel itself out here.
    or_var_min = {}
    for m, t, idx in or_varfmts:
        try:
            i = int(idx.split(".")[0])
        except ValueError:
            continue
        key = (m, t)
        if key not in or_var_min or i < or_var_min[key]:
            or_var_min[key] = i

    fields_by_table = defaultdict(list)
    for m, t, idx in gen_fields:
        try:
            fields_by_table[(m, t)].append(int(idx.split(".")[0]))
        except ValueError:
            continue

    sound_ok = sound_bad = 0
    sound_examples = []
    for table_key, or_min in or_var_min.items():
        if table_key not in gen_sound_until:
            # codegen.py never emitted this table at all (e.g. no field
            # survived every other filter) -- nothing to check here.
            continue
        affected = any(idx > or_min for idx in fields_by_table.get(table_key, ()))
        want = or_min if affected else None
        got = gen_sound_until[table_key]
        if got == want:
            sound_ok += 1
        else:
            sound_bad += 1
            if len(sound_examples) < args.show:
                sound_examples.append((table_key, got, want))

    print(f"fields checked   {name_ok + name_bad}")
    print(f"  match          {name_ok}")
    print(f"  MISMATCH       {name_bad}")
    print(f"  not in oracle  {orphan}")
    print(f"enum entries     {enum_ok + enum_bad}")
    print(f"  match          {enum_ok}")
    print(f"  MISMATCH       {enum_bad}")
    print(f"masked fields    {mask_ok + mask_bad}")
    print(f"  match          {mask_ok}")
    print(f"  MISMATCH       {mask_bad}")
    # Step 23 variant columns: `_variants` alternatives (`VariantGroup` in
    # the generated Rust), checked the same way as the plain-field columns
    # above but reported separately so a `_variants`-specific regression
    # cannot hide inside the (much larger) plain-field totals.
    print(f"variant tags in schema     {len(gen_variant_keys)}")
    print(f"variant names checked      {variant_name_ok + variant_name_bad}")
    print(f"  match                    {variant_name_ok}")
    print(f"  MISMATCH                 {variant_name_bad}")
    print(f"  not in oracle            {variant_orphan}")
    print(f"variant enum entries       {variant_enum_ok + variant_enum_bad}")
    print(f"  match                    {variant_enum_ok}")
    print(f"  MISMATCH                 {variant_enum_bad}")
    print(f"variant masked fields      {variant_mask_ok + variant_mask_bad}")
    print(f"  match                    {variant_mask_ok}")
    print(f"  MISMATCH                 {variant_mask_bad}")
    print(f"hook flags       {hook_ok + hook_bad}")
    print(f"  match          {hook_ok}")
    print(f"  MISMATCH       {hook_bad}")
    print(f"subdirectory flags {subdir_ok + subdir_bad}")
    print(f"  match          {subdir_ok}")
    print(f"  MISMATCH       {subdir_bad}")
    print(f"subdirectory edges (Step 27) {edge_ok + edge_bad}")
    print(f"  match          {edge_ok}")
    print(f"  MISMATCH       {edge_bad}")
    print(f"offsets_sound_until tables {sound_ok + sound_bad}")
    print(f"  match          {sound_ok}")
    print(f"  MISMATCH       {sound_bad}")

    for k, got, want in bad_examples:
        print(f"  name  {k}: generated {got!r} != exiftool {want!r}")
    for k in orphan_examples:
        print(f"  orphan {k}")
    for k, kk, got, want in enum_examples:
        print(f"  enum  {k} key {kk}: generated {got!r} != exiftool {want!r}")
    for k, got, want in mask_examples:
        print(f"  mask  {k}: generated {got!r} != exiftool {want!r}")
    for k, got, want in variant_bad_examples:
        print(f"  variant name  {k}: generated {got!r} != exiftool {want!r}")
    for k in variant_orphan_examples:
        print(f"  variant orphan {k}")
    for k, kk, got, want in variant_enum_examples:
        print(f"  variant enum  {k} key {kk}: generated {got!r} != exiftool {want!r}")
    for k, got, want in variant_mask_examples:
        print(f"  variant mask  {k}: generated {got!r} != exiftool {want!r}")
    for k, got, want in hook_examples:
        print(f"  hook  {k}: generated {got!r} != exiftool {want!r}")
    for k, got, want in subdir_examples:
        print(f"  subdirectory  {k}: generated {got!r} != exiftool {want!r}")
    for k, got, want in edge_examples:
        print(f"  subdir edge  {k}: generated {got!r} != expected {want!r}")
    for k, got, want in sound_examples:
        print(f"  offsets_sound_until  {k}: generated {got!r} != expected {want!r}")

    failed = (
        name_bad + enum_bad + orphan + mask_bad
        + variant_name_bad + variant_enum_bad + variant_orphan + variant_mask_bad
        + hook_bad + subdir_bad + edge_bad + sound_bad
    )
    print("\nRESULT:", "PASS" if failed == 0 else f"FAIL ({failed} discrepancies)")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
