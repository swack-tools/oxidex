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

Slice I-1 adds a second stage over `src/exiftool_tables/ifd_tables.rs` (the
ProcessExif-style tables; see `docs/superpowers/specs/2026-09-06-ifd-tables-
design.md` section 4), run after the binary stage from the same oracle
output and folded into the same exit status. It is skipped, with a message,
when that file does not exist on the tree -- the binary verification above is
unaffected either way. `parse_ifd_rust` / `verify_ifd` are the two halves.
"""

import argparse
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path
from typing import NamedTuple

# The repo root, derived from this file's location rather than the working
# directory: CI, the justfile and regen.sh all invoke this script from
# different places, and a cwd-relative pin lookup would silently read nothing.
REPO_ROOT = Path(__file__).resolve().parents[2]
PIN_FILE = REPO_ROOT / ".exiftool-version"

sys.path.insert(0, str(REPO_ROOT / "scripts"))
import exiftool_oracle  # noqa: E402 -- capability-aware perl selection
import instrument  # noqa: E402 -- git/instrument identity header

# The perl this verifier runs `oracle.pl` and the ExifTool-version probe
# under. Resolved once, by capability (not a bare "perl" off PATH -- see
# AGENTS.md "A matching -ver is not a working oracle"): a perl that cannot
# load Archive::Zip still loads Image::ExifTool.pm and still reports the
# right $VERSION, so a version-only check here would not have caught the
# same wrong-interpreter failure exiftool_oracle.py exists to catch.
_PERL = exiftool_oracle.choose_perl()
if _PERL is None:
    sys.exit("❌ no usable perl found to run oracle.pl / probe ExifTool.pm")

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
    # Step 26: the three EFFECTIVE groups. Captured (not skipped) so the
    # defaulting rule itself is checked against the oracle below.
    r'group0:\s*"(?P<group0>[^"]*)",\s*'
    r'group1:\s*"(?P<group1>[^"]*)",\s*'
    r'group2:\s*"(?P<group2>[^"]*)",\s*'
    r'first_entry:\s*-?\d+,\s*'
    r'default_format:\s*Fmt::\w+,\s*'
    r'offsets_sound_until:\s*(?P<sound_until>None|Some\(-?\d+\)),',
)
FIELD_RE = re.compile(
    r'Field\s*\{\s*'
    r'index:\s*(?P<index>-?\d+),\s*'
    r'sub:\s*(?P<sub>None|Some\(\d+\)),\s*'
    r'name:\s*"(?P<name>(?:[^"\\]|\\.)*)",\s*'
    # Step 26: `Some(Fmt::Var(VarFmt { spelling: "var_string", kind:
    # VarKind::String }))` is a third shape alongside `Some(Fmt::Int8u)` and
    # `Some(Fmt::Str(4))`. Spelled out rather than widened to `.*?`: a
    # pattern loose enough to swallow anything would also swallow a
    # malformed literal and report it as a match.
    r'format:\s*(?P<fmt>None'
    r'|Some\(Fmt::\w+(?:\(\d+\))?\)'
    r'|Some\(\s*Fmt::Var\(\s*VarFmt\s*\{\s*spelling:\s*"(?P<var_spelling>[^"]*)",\s*'
    r'kind:\s*VarKind::(?P<var_kind>\w+),?\s*\}\s*,?\s*\)\s*,?\s*\)),\s*'
    r'count:\s*(?P<count>\d+),\s*'
    r'mask:\s*(?:None'
    r'|Some\(\s*Mask\s*\{\s*bits:\s*(?P<mask_bits>0[xX][0-9a-fA-F_]+|\d+),\s*'
    r'shift:\s*(?P<mask_shift>\d+),?\s*\}\s*,?\s*\)),\s*'
    r'omitted:\s*(?P<omitted>Omitted::NONE|Omitted\s*\{[^{}]*\}),\s*'
    # R2 carries an oracle-approved ValueConv ExprId between Omitted and
    # PrintConv. It must be matched as a required schema member so adding the
    # field cannot make this independent verifier silently parse zero tables;
    # its value is not captured (nothing downstream needs to compare it), just
    # consumed so the fixed-width prefix keeps lining up ahead of print_conv.
    r'value_conv:\s*(?:None|Some\(\s*ExprId::\w+\s*\)),\s*'
    # Step 25: neither `print_conv:` nor `subdir:` has its value captured
    # inline anymore. `subdir:`'s value (`None` or `Some(SubdirEdge { ... })`,
    # arbitrarily nested via `Start::Expr(&...)`) never could be -- a
    # fixed-depth regex cannot bound arbitrary nesting safely, the same
    # reason enum bodies are rescanned by `_enum_body` instead of captured
    # inline. `print_conv:`'s value joined it once `PrintConv::Bitmask {
    # exact: &[...], bits: &[...] }` and `PrintConv::PartialEnumInt { exact:
    # &[...], other: Some(OtherId::...)|None, print_hex: bool }` arrived: two
    # independently-sized `&[...]` arrays (or one array plus two scalars) is
    # exactly the shape a fixed-width alternation cannot bound either. Both
    # are read the same way from here on: `_value_span`, twice in sequence
    # from this match's end, locates first the `print_conv:` value and then
    # (immediately after, anchored by the literal `, subdir:` between them)
    # the `subdir:` value -- see `_parse_one_field` for how each print_conv
    # shape (`None`, `Expr(...)`, `IntEnum(&[...])`, `StrEnum(&[...])`,
    # `Bitmask {...}`, `PartialEnumInt {...}`) is dispatched from its text.
    r'print_conv:\s*',
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


# Step 26. ExifTool format name -> the Rust `Fmt` variant a correct
# transcription must use. Written out here on purpose rather than imported
# from codegen.py: this file's whole reason for existing is that a verifier
# sharing the generator's tables can only prove the generator agrees with
# itself (see oracle.pl's header). Every width below is %formatSize in
# ExifTool.pm:6210-6242.
#
# A spelling absent from this map is one the schema cannot express, and the
# ONLY correct thing for the generator to do with such a field is refuse it --
# `expected_fmt_literal` returns None to say so, and `main()` checks that no
# such field was emitted.
_FMT_VARIANT = {
    "int8u": "Int8u", "int8s": "Int8s",
    "int16u": "Int16u", "int16s": "Int16s", "int16uRev": "Int16uRev",
    "int32u": "Int32u", "int32s": "Int32s", "int32uRev": "Int32uRev",
    "int64u": "Int64u", "int64s": "Int64s",
    "float": "Float", "double": "Double",
    "rational32u": "Rational32u", "rational32s": "Rational32s",
    "rational64u": "Rational64u", "rational64s": "Rational64s",
    "fixed16s": "Fixed16s", "fixed16u": "Fixed16u",
    "fixed32s": "Fixed32s", "fixed32u": "Fixed32u",
    "extended": "Extended",
}
_SIZED_FMT_RE = re.compile(r"^(\w+)\[(\d+)\]$")
# The arms of ProcessBinaryData's variable-format branch, ExifTool.pm:10000-10032.
_VAR_KINDS = {
    "var_string": "String",
    "var_ustring": "UString",
    "var_pstring": "PString",
    "var_pstr32": "PStr32",
    "var_ustr32": "UStr32",
    "var_int16u": "Int16u",
    "var_ue7": "Ue7",
}


def expected_fmt_literal(spelling):
    """-> ("Some(Fmt::X)" | "None", count) the generated Rust must carry for a
    field whose ExifTool `Format` is `spelling`, or None if this schema cannot
    express that format (in which case the field must not be emitted at all).

    `spelling is None` means the field declares no Format and inherits the
    table's FORMAT, which the Rust records as `format: None, count: 1`.
    """
    if spelling is None:
        return ("None", 1)
    m = _SIZED_FMT_RE.match(spelling)
    if m:
        base, n = m.group(1), int(m.group(2))
        # `string[N]`/`undef[N]` carry their byte count in the variant itself;
        # every other sized form is N repetitions of a scalar element.
        if base == "string":
            return (f"Some(Fmt::Str({n}))", 1)
        if base == "undef":
            return (f"Some(Fmt::Undef({n}))", 1)
        if base in _FMT_VARIANT:
            return (f"Some(Fmt::{_FMT_VARIANT[base]})", n)
        return None
    if spelling in _FMT_VARIANT:
        return (f"Some(Fmt::{_FMT_VARIANT[spelling]})", 1)
    # pstring is not in %formatSize at all -- ExifTool handles it inline at
    # ExifTool.pm:9972-9975, reading a leading int8u count. It shifts no later
    # field's offset, so it is expressible; nothing else outside the map is.
    if spelling == "pstring":
        return ("Some(Fmt::PString)", 1)
    # Step 26: a `var_*` name inside the closed grammar is MODELED (emitted
    # carrying its rule, never decoded); one outside it is refused. Same
    # split codegen.py makes, re-derived here from ExifTool's own
    # if/elsif chain at ExifTool.pm:10000-10032 rather than imported.
    if spelling in _VAR_KINDS:
        return ("VAR", spelling, _VAR_KINDS[spelling], 1)
    return None


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


_RUST_ESCAPE_RE = re.compile(r'\\(?:u\{([0-9a-fA-F]{1,6})\}|x([0-9a-fA-F]{2})|(.))', re.S)
_RUST_SIMPLE_ESCAPES = {"\\": "\\", '"': '"', "n": "\n", "r": "\r", "t": "\t", "0": "\0", "'": "'"}


def unescape(s):
    """Decode a Rust string-literal body exactly as rustc would: `\\\\`,
    `\\"`, `\\n`, `\\r`, `\\t`, `\\0`, `\\xNN` and `\\u{..}` (which is how
    `codegen.py::rust_str` spells NUL and other control characters, e.g.
    Exif::Main FileSource's `"\\3\\0\\0\\0"` key). A single left-to-right pass
    -- the old placeholder trick (`\\\\` -> NUL -> `\\`) turned a real NUL
    byte in the source into a backslash and never met one until the IFD
    tables did."""
    def one(m):
        if m.group(1) is not None:
            return chr(int(m.group(1), 16))
        if m.group(2) is not None:
            return chr(int(m.group(2), 16))
        ch = m.group(3)
        if ch in _RUST_SIMPLE_ESCAPES:
            return _RUST_SIMPLE_ESCAPES[ch]
        raise SystemExit(f"unescape: unknown Rust escape \\{ch!r} in {s!r} -- extend unescape() before trusting a PASS")
    return _RUST_ESCAPE_RE.sub(one, s)


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


# The literal text between a `print_conv:` value and the `subdir:` value
# that always follows it -- anchors the second `_value_span` call in
# `_parse_one_field` once the first has located where the print_conv value
# ends.
_SUBDIR_LABEL_RE = re.compile(r'\s*,\s*subdir:\s*', re.S)

# `PrintConv::Bitmask { exact: ..., bits: ... }` / `PrintConv::PartialEnumInt
# { exact: ..., other: ..., print_hex: ... }`: only the struct TAG is
# anchored here (whitespace/brace-tolerant across rustfmt's wrapping); each
# named member's `&[...]` array is then located independently by
# `_named_array_span` and re-walked by `_enum_body`, the same "do not trust a
# fixed-depth capture with unbounded nesting" discipline `subdir:` already
# uses. `PrintConv::None`/`Expr(...)`/`IntEnum(&[...])`/`StrEnum(&[...])`
# stay recognised by a plain prefix check -- see `_parse_one_field`.
_BITMASK_RE = re.compile(r'^PrintConv::Bitmask\s*\{', re.S)
_PARTIAL_ENUM_INT_RE = re.compile(r'^PrintConv::PartialEnumInt\s*\{', re.S)
_OTHER_ID_RE = re.compile(r'other:\s*(?:Some\(\s*OtherId::(?P<variant>\w+)\s*,?\s*\)|(?P<none>None))')
_PRINT_HEX_RE = re.compile(r'print_hex:\s*(?P<val>true|false)')


def _named_array_span(src, pc_start, pc_end, member):
    """Absolute `(open_bracket_index)` of `member`'s `&[` inside the
    `print_conv:` value spanning `src[pc_start:pc_end]` (a `Bitmask`/
    `PartialEnumInt` struct literal), for `_enum_body` to bracket-match from.
    Raises SystemExit if `member` is not found in range -- an out-of-date
    pattern must fail loudly, not silently verify zero entries."""
    m = re.search(rf'{member}:\s*&\[', src[pc_start:pc_end])
    if not m:
        raise SystemExit(
            f"print_conv value {src[pc_start:pc_end]!r} has no `{member}: &[` "
            "-- the verifier's pattern is out of date; fix it before trusting a PASS"
        )
    return pc_start + m.end() - 1


def _parse_int_pairs(src, open_bracket, k, label):
    """`_enum_body` + `INT_PAIR_RE`, with the same exact-accounting discipline
    `_parse_one_field`'s enum handling already applies: a pair the pattern
    cannot read fails the run outright."""
    body, expected_pairs = _enum_body(src, open_bracket)
    pairs = INT_PAIR_RE.findall(body)
    if len(pairs) != expected_pairs:
        raise SystemExit(
            f"{label} for {k}: parsed {len(pairs)} of {expected_pairs} entries "
            "-- the verifier's pair pattern is out of date; fix it before trusting a PASS"
        )
    return pairs


# Step 26: `groups: TagGroups::NONE` or
# `groups: TagGroups { g0: None, g1: Some("GPS"), g2: Some("Location") }`.
# Scanned from the field's tail rather than captured by FIELD_RE, which stops
# at `subdir:` (whose value nests arbitrarily -- see that pattern's comment).
_TAG_GROUPS_BODY = (
    r'(?:TagGroups::NONE'
    r'|TagGroups\s*\{\s*g0:\s*(?P<g0>None|Some\("(?:[^"\\]|\\.)*"\)),\s*'
    r'g1:\s*(?P<g1>None|Some\("(?:[^"\\]|\\.)*"\)),\s*'
    r'g2:\s*(?P<g2>None|Some\("(?:[^"\\]|\\.)*"\)),?\s*\})'
)
TAG_GROUPS_RE = re.compile(r'groups:\s*' + _TAG_GROUPS_BODY)
# The same value grammar anchored to a captured `groups:` VALUE on its own
# (slice I-1's `IFD_TAG_RE` captures the value; the binary `FIELD_RE` does
# not, hence the two entry points over one body).
_TAG_GROUPS_VALUE_RE = re.compile(r'^' + _TAG_GROUPS_BODY + r'$', re.S)


def _some_str(text):
    """`Some("X")` -> "X"; `None` (or absent) -> "", matching the oracle's
    empty-column convention for a family the tag does not override."""
    if not text or text == "None":
        return ""
    return unescape(text[len('Some("'):-len('")')])


def _parse_one_field(
    src, f, k, fields, enums, masks, hooks, subdirs, subdir_edges,
    bitmasks, other_ids, print_hexes, pc_refused, pc_kinds, formats, tag_groups,
):
    """Populate `fields`/`enums`/`masks`/`hooks`/`subdirs`/`subdir_edges`/
    `bitmasks`/`other_ids`/`print_hexes`/`formats`/`tag_groups` from one
    `FIELD_RE` match `f`, under dict key `k`. Shared by the plain-`fields:`
    scan and the `variants:` scan below -- a `Field {...}` literal means the
    same thing in both places, only what `k` looks like differs (see
    `parse_rust`)."""
    fields[k] = unescape(f.group("name"))
    # Step 26: (format, count) as written in the Rust, e.g.
    # ("Some(Fmt::Int64u)", 1). Checked against the oracle's raw Format
    # spelling in `main()`.
    # Normalized so rustfmt's line wrapping inside a `Fmt::Var(VarFmt {...})`
    # literal cannot masquerade as a mismatch: a Var is compared by its two
    # semantic parts, everything else by its (whitespace-free) literal text.
    if f.group("var_spelling") is not None:
        formats[k] = ("VAR", f.group("var_spelling"), f.group("var_kind"),
                      int(f.group("count")))
    else:
        formats[k] = (re.sub(r"\s+", "", f.group("fmt")), int(f.group("count")))

    bits = f.group("mask_bits")
    if bits is not None:
        masks[k] = (int(bits, 0), int(f.group("mask_shift")))

    omitted = f.group("omitted")
    if "hook: true" in omitted:
        hooks.add(k)
    if "subdirectory: true" in omitted:
        subdirs.add(k)
    if "print_conv: true" in omitted:
        pc_refused.add(k)

    # `FIELD_RE` now consumes only `print_conv:` (no captured value -- see
    # the pattern's own comment): `_value_span` from the match's end finds
    # where that value ends, the literal `, subdir:` must follow immediately,
    # and a second `_value_span` from there finds the subdir value.
    pc_start = f.end()
    pc_end = _value_span(src, pc_start)
    pc = src[pc_start:pc_end].strip()

    sm = _SUBDIR_LABEL_RE.match(src, pc_end)
    if not sm:
        raise SystemExit(
            f"{k}: no `, subdir:` immediately after the print_conv value "
            f"{pc!r} -- the verifier's pattern is out of date; fix it "
            "before trusting a PASS"
        )
    subdir_val_end = _value_span(src, sm.end())
    subdir_edges[k] = _parse_subdir_value(src[sm.end():subdir_val_end])

    # The window must clear the `hook:` member that sits between `subdir:`
    # and `groups:`; a two-effect Canon hook chain alone runs past 400 chars.
    # Bounded rather than open-ended so a field missing its `groups:` member
    # cannot silently match the NEXT field's.
    gm = TAG_GROUPS_RE.search(src, subdir_val_end, subdir_val_end + 4000)
    if gm is None:
        raise SystemExit(
            f"no `groups:` member parsed for {k} -- the verifier's pattern is "
            "out of date; fix it before trusting a PASS"
        )
    tag_groups[k] = (_some_str(gm.group("g0")), _some_str(gm.group("g1")),
                     _some_str(gm.group("g2")))

    _parse_print_conv(src, pc_start, pc_end, k, enums, bitmasks, other_ids, print_hexes, pc_kinds)


def _parse_print_conv(src, pc_start, pc_end, k, enums, bitmasks, other_ids, print_hexes, pc_kinds):
    """Populate `enums`/`bitmasks`/`other_ids`/`print_hexes`/`pc_kinds` for
    the `print_conv:` value spanning `src[pc_start:pc_end]` under dict key
    `k`. Shared by the binary `Field` parser and slice I-1's `IfdTag` parser:
    a `PrintConv::...` literal means the same thing in both schemas (the
    IFD schema reuses the type), so one reader serves both."""
    pc = src[pc_start:pc_end].strip()
    pc_kinds[k] = pc.split("(", 1)[0]
    # Rescan enum bodies from the source itself rather than trusting a
    # regex capture -- see _enum_body for why.
    if pc.startswith("PrintConv::IntEnum(&[") or pc.startswith("PrintConv::StrEnum(&["):
        int_keys = pc.startswith("PrintConv::IntEnum(&[")
        marker = "PrintConv::IntEnum(&[" if int_keys else "PrintConv::StrEnum(&["
        pair_re = INT_PAIR_RE if int_keys else STR_PAIR_RE
        body, expected_pairs = _enum_body(src, pc_start + pc.index(marker) + len(marker) - 1)
        pairs = pair_re.findall(body)
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
    elif _BITMASK_RE.match(pc) or _PARTIAL_ENUM_INT_RE.match(pc):
        # Step 25: both shapes carry an int-domain `exact: &[(i64, "..."),
        # ...]` array built exactly like `IntEnum`'s -- feeding it into the
        # SAME `enums[k]` dict means the pre-existing oracle ENUM comparison
        # below verifies it for free (oracle.pl already skips the
        # BITMASK/OTHER directive keys themselves, so `or_enums[k]` holds
        # exactly this "exact match" population for these fields too).
        exact_open = _named_array_span(src, pc_start, pc_end, "exact")
        for kk, vv in _parse_int_pairs(src, exact_open, k, "exact"):
            enums[k][kk] = unescape(vv)
        if _BITMASK_RE.match(pc):
            bits_open = _named_array_span(src, pc_start, pc_end, "bits")
            bitmasks[k] = {
                kk: unescape(vv) for kk, vv in _parse_int_pairs(src, bits_open, k, "bits")
            }
        else:
            om = _OTHER_ID_RE.search(pc)
            if not om:
                raise SystemExit(
                    f"{k}: PartialEnumInt value {pc!r} has no `other:` -- "
                    "the verifier's pattern is out of date; fix it before "
                    "trusting a PASS"
                )
            other_ids[k] = om.group("variant")
            phm = _PRINT_HEX_RE.search(pc)
            if not phm:
                raise SystemExit(
                    f"{k}: PartialEnumInt value {pc!r} has no `print_hex:` "
                    "-- the verifier's pattern is out of date; fix it "
                    "before trusting a PASS"
                )
            print_hexes[k] = phm.group("val") == "true"
    elif pc not in ("PrintConv::None",) and not pc.startswith("PrintConv::Expr("):
        # Every shape `codegen.py`'s PRELUDE can emit is handled above or is
        # one of these two data-free ones; anything else is a `PrintConv`
        # variant this verifier does not know about yet -- fail loudly
        # rather than silently skip it (the same "an unparsed field is a
        # coverage lie" doctrine `parse_rust`'s own field-count check
        # enforces).
        raise SystemExit(
            f"{k}: unrecognised print_conv value {pc!r} -- the verifier's "
            "pattern is out of date; fix it before trusting a PASS"
        )


# `parse_rust`'s result, by NAME as well as by position. It is still a tuple,
# so this file's own positional unpack keeps working -- but a caller in ANOTHER
# file should reach for the attribute, because the positional contract has now
# been broken twice by the same mechanism: Step 25 (b025fcb1) appended
# bitmasks/other_ids/print_hexes and `verify_subdirs.py` died unpacking 8 of 11
# (fixed in da1fec86 by hardcoding 11); Step 28's print_conv accounting then
# appended pc_refused/pc_kinds and it died again, 11 of 13. Both times each
# branch passed its own gate and only the merged tree broke. Appending a field
# below can no longer break an attribute-using caller at all.
class ParsedRust(NamedTuple):
    fields: dict
    enums: dict
    masks: dict
    hooks: set
    subdirs: set
    subdir_edges: dict
    sound_until: dict
    variant_keys: dict
    bitmasks: dict
    other_ids: dict
    print_hexes: dict
    pc_refused: dict
    pc_kinds: dict
    formats: dict
    table_groups: dict
    tag_groups: dict


def parse_rust(path):
    """-> (fields, enums, masks, hooks, subdirs, subdir_edges, sound_until,
    variant_keys, bitmasks, other_ids, print_hexes, pc_refused, pc_kinds,
    formats, table_groups, tag_groups)

    fields{k->name}, enums{k->{key:val}}, masks{k->(bits,shift)},
    hooks{k}, subdirs{k} (sets of field keys with that Omitted flag set),
    subdir_edges{k -> None | (module, table, start_text, base_text)} (Step
    27: every field's `subdir:` value, present or not -- unlike the other
    dicts this one always has an entry for every field, `None` included, so
    `main()` can tell "no edge" from "field not parsed"),
    sound_until{(mod,tbl)->int|None} (the table's offsets_sound_until).

    Step 25: bitmasks{k->{bit:label}} (a `PrintConv::Bitmask` field's `bits:`
    array), other_ids{k->OtherId variant name} (a `PrintConv::PartialEnumInt`
    field's registered `other:`), print_hexes{k->bool} (that same field's
    `print_hex:`). All three are present only for the fields carrying the
    corresponding `PrintConv` variant -- `main()` treats key-absence as "not
    applicable", the same way `masks` already does for unmasked fields.

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
    # Step 28's `Omitted.print_conv`: the fields the generator refused a
    # `PrintConv` for, plus every field's `PrintConv::` variant name, so
    # `main()` can check the refusal both ways round against `oracle.pl`'s
    # independently-read PCREF rows.
    pc_refused, pc_kinds = set(), {}
    subdir_edges = {}
    formats = {}
    sound_until = {}
    table_groups = {}
    tag_groups = {}
    variant_keys = set()
    bitmasks, other_ids, print_hexes = {}, {}, {}
    bounds = [(m.start(), m.group("module"), m.group("table"), m.group("sound_until"),
               (m.group("group0"), m.group("group1"), m.group("group2")))
              for m in TABLE_RE.finditer(src)]
    for start, mod, tbl, su, grp in bounds:
        sound_until[(mod, tbl)] = None if su == "None" else int(su[len("Some("):-1])
        table_groups[(mod, tbl)] = grp
    bounds = [(start, mod, tbl) for start, mod, tbl, _su, _g in bounds]
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
                    src, f, (mod, tbl, key), fields, enums, masks, hooks, subdirs, subdir_edges,
                    bitmasks, other_ids, print_hexes, pc_refused, pc_kinds, formats, tag_groups,
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
                        src, f, k, fields, enums, masks, hooks, subdirs, subdir_edges,
                        bitmasks, other_ids, print_hexes, pc_refused, pc_kinds, formats, tag_groups,
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
    return ParsedRust(
        fields, enums, masks, hooks, subdirs, subdir_edges, sound_until, variant_keys,
        bitmasks, other_ids, print_hexes, pc_refused, pc_kinds, formats, table_groups, tag_groups,
    )


def run_oracle(lib, oracle_pl):
    """`oracle.pl`'s raw TSV for `lib` -- one run feeds both the binary
    reader (`parse_binary_oracle`) and slice I-1's `parse_ifd_oracle`."""
    return subprocess.run(
        [_PERL, oracle_pl, lib],
        capture_output=True, check=True, text=True, encoding="utf-8",
    ).stdout


def load_oracle(lib, oracle_pl):
    """Run the oracle and read its binary-table rows (the pre-I-1 entry
    point, kept for callers that only want that half)."""
    return parse_binary_oracle(run_oracle(lib, oracle_pl))


def parse_binary_oracle(out):
    names, enums, masks = {}, defaultdict(dict), {}
    hooks, subdirs, varfmts = set(), set(), set()
    bitmasks = defaultdict(dict)
    other_present, other_print_hex = set(), {}
    # {(mod,sym,key) -> 'CODE'|'ARRAY'|...}: the field's PrintConv is a REF
    # that is not a HASH, i.e. not an enum. `codegen.py` translates only the
    # CODE refs `exprs.py`'s CODE_REFS names and refuses the rest; `main()`
    # uses this to check that decision from the ExifTool side.
    pcrefs = {}
    # Step 26: {(mod,sym,key) -> raw Format spelling}, for the format column.
    rawfmts = {}
    # Step 26: {(mod,sym) -> (g0,g1,g2)} raw table GROUPS (before defaulting),
    # and {(mod,sym,key) -> (g0,g1,g2)} per-tag overrides. '' means absent.
    tblgroups, taggroups = {}, {}
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
        # Slice I-1: IFD rows carry a leading kind column and are read by
        # `parse_ifd_oracle`; they never have the column counts below with
        # a marker in p[3] (that slot holds the tag key), but routing on the
        # kind column is the stated contract, not a coincidence.
        if p[0] == "IFD":
            continue
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
        elif len(p) == 6 and p[3] == "BITMASK":
            bitmasks[(p[0], p[1], p[2])][p[4]] = p[5]
        elif len(p) == 5 and p[3] == "OTHER":
            key = (p[0], p[1], p[2])
            other_present.add(key)
            other_print_hex[key] = p[4] == "1"
        elif len(p) == 5 and p[3] == "PCREF":
            pcrefs[(p[0], p[1], p[2])] = p[4]
        elif len(p) == 5 and p[3] == "FORMAT":
            rawfmts[(p[0], p[1], p[2])] = p[4]
        elif len(p) == 7 and p[3] == "TGROUPS":
            tblgroups[(p[0], p[1])] = (p[4], p[5], p[6])
        elif len(p) == 7 and p[3] == "GROUPS":
            taggroups[(p[0], p[1], p[2])] = (p[4], p[5], p[6])
    return (
        names, enums, masks, hooks, subdirs, subdir_facts, varfmts,
        bitmasks, other_present, other_print_hex, pcrefs,
        rawfmts, tblgroups, taggroups,
    )


def oracle_version(lib):
    """The ExifTool release living in `lib`, read the same way the oracle does."""
    return subprocess.run(
        [_PERL, f"-I{lib}", "-e",
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


# ===========================================================================
# Slice I-1: the IFD-style tables, `src/exiftool_tables/ifd_tables.rs`.
# ===========================================================================
#
# Same doctrine, second table kind. The generated file is parsed back (never
# the dump JSON) and every emitted fact is compared with `oracle.pl`'s `IFD`
# rows, which read the live Perl hashes independently of dump_tables.pl. The
# property is SOUNDNESS: the generator may emit fewer tables, tags, enum
# entries or edges than ExifTool has, but never a table or tag ExifTool
# lacks, a wrong name / format / count / writable / group / flag, a
# `SetMember` naming the wrong member, a conversion refusal the tag does not
# warrant (or a silently dropped one), or a `SubDirectory` edge whose target,
# start, byte order, FixFormat, SubIFD, MaxSubdirs, DirName or Validate fact
# differs from ExifTool's. Two things are refusals by design and never a
# mismatch on their own: an edge left `None` (with `omitted.subdirectory`
# set) and a `RawConv` left `omitted.raw_conv` -- withholding is always
# sound. They are counted as notes so the coverage they hold back is visible.
#
# The per-tag field order below is `ifd_schema.rs`'s `IfdTag` order, which
# is what the generator emits (the spec, section 2); rustfmt wraps it, so
# every pattern is whitespace-tolerant, and `parse_ifd_rust` asserts it
# accounted for every `IfdTag {` in the file so under-parsing fails loudly.

IFD_TABLE_RE = re.compile(
    r'pub static (?P<ident>\w+): IfdTable = IfdTable \{\s*'
    r'module:\s*"(?P<module>[^"]*)",\s*'
    r'table:\s*"(?P<table>[^"]*)",\s*'
    r'group0:\s*"(?P<group0>[^"]*)",\s*'
    r'group1:\s*"(?P<group1>[^"]*)",\s*'
    r'group2:\s*"(?P<group2>[^"]*)",\s*'
    r'set_group1:\s*(?P<set_group1>None|Some\("(?:[^"\\]|\\.)*"\)),\s*'
    r'priority:\s*(?P<priority>None|Some\(-?\d+\)),\s*'
    r'gate_a:\s*GateA\s*\{\s*blocked_by:\s*&\[',
    re.S,
)
IFD_TABLE_DECL_RE = re.compile(r'pub static \w+: IfdTable = IfdTable \{')
IFD_TAG_RE = re.compile(
    r'IfdTag\s*\{\s*'
    r'id:\s*(?P<id>0x[0-9a-fA-F]+|\d+),\s*'
    r'name:\s*"(?P<name>(?:[^"\\]|\\.)*)",\s*'
    # Scalar formats only (the spec refuses everything else for an IFD tag):
    # `None`, `Some(Fmt::Int16u)`, `Some(Fmt::Str(11))`. A `Fmt::Var(...)`
    # here is deliberately NOT matched -- it would leave an `IfdTag {`
    # unaccounted for and fail the parse loudly, which is the right outcome
    # for a shape the schema forbids.
    r'format:\s*(?P<fmt>None|Some\(Fmt::\w+(?:\(\d+\))?\)),\s*'
    r'count:\s*(?P<count>None|Some\(\d+\)),\s*'
    r'writable:\s*(?P<writable>None|Some\("(?:[^"\\]|\\.)*"\)),\s*'
    r'groups:\s*(?P<groups>TagGroups::NONE|TagGroups\s*\{[^{}]*\}),\s*'
    r'flags:\s*(?P<flags>IfdFlags::NONE|IfdFlags\s*\{[^{}]*\}),\s*'
    r'omitted:\s*(?P<omitted>Omitted::NONE|Omitted\s*\{[^{}]*\}),\s*'
    r'raw_conv:\s*(?P<raw_conv>None|Some\(\s*RawConvEffect::SetMember\s*\{\s*'
    r'member:\s*"(?P<member>(?:[^"\\]|\\.)*)",?\s*\}\s*,?\s*\)),\s*'
    r'value_conv:\s*(?:None|Some\(\s*ExprId::\w+\s*,?\s*\)),\s*'
    # `print_conv:` and `subdir:` values are located by `_value_span`, as
    # for a binary `Field` (their nesting is unbounded).
    r'print_conv:\s*',
    re.S,
)
IFD_TAG_COUNT_RE = re.compile(r'IfdTag\s*\{\s*id:')
IFD_TAGS_MARKER_RE = re.compile(r'tags:\s*&\[')
IFD_VARIANTS_MARKER_RE = re.compile(r'variants:\s*&\[')
IFD_VARIANT_GROUP_DECL_RE = re.compile(r'IfdVariantGroup\s*\{')
IFD_VARIANT_GROUP_RE = re.compile(
    r'IfdVariantGroup\s*\{\s*id:\s*(?P<id>0x[0-9a-fA-F]+|\d+),\s*alternatives:\s*&\['
)
ALL_IFD_TABLES_RE = re.compile(r'pub static ALL_IFD_TABLES:\s*&\[&IfdTable\]\s*=\s*&\[')
_IFD_FLAGS_RE = re.compile(
    r'IfdFlags\s*\{\s*unknown:\s*(?P<unknown>true|false),\s*binary:\s*(?P<binary>true|false),\s*'
    r'list:\s*(?P<list>true|false),\s*protected:\s*(?P<protected>true|false),\s*'
    r'avoid:\s*(?P<avoid>true|false),\s*priority:\s*(?P<priority>None|Some\(-?\d+\)),?\s*\}$'
)
_OMITTED_MEMBERS = ("value_conv", "raw_conv", "condition", "hook", "subdirectory", "print_conv")
_IFD_SUBDIR_EDGE_RE = re.compile(
    r'^Some\(\s*IfdSubdirEdge\s*\{\s*'
    r'module:\s*"(?P<module>[^"]*)"\s*,\s*'
    r'table:\s*"(?P<table>[^"]*)"\s*,\s*'
    r'start:\s*IfdStart::(?P<start_kind>ValuePtr|Val)\(\s*(?P<start_n>-?\d+)\s*,?\s*\)\s*,\s*'
    r'base:\s*',
    re.S,
)
# Matched at the index where the `base:` value ended (`re.match(text, pos)`,
# so no `^` -- Python anchors `^` to the string start, not to `pos`).
_IFD_SUBDIR_TAIL_RE = re.compile(
    r'\s*,\s*byte_order:\s*IfdByteOrder::(?P<byte_order>Inherit|Little|Big|Unknown)\s*,\s*'
    r'fix_format:\s*(?P<fix_format>None|Some\(Fmt::\w+(?:\(\d+\))?\))\s*,\s*'
    r'sub_ifd:\s*(?P<sub_ifd>true|false)\s*,\s*'
    r'max_subdirs:\s*(?P<max_subdirs>None|Some\(\d+\))\s*,\s*'
    r'dir_name:\s*(?P<dir_name>None|Some\("(?:[^"\\]|\\.)*"\))\s*,\s*'
    r'validate:\s*(?P<validate>true|false)\s*,\s*'
    # rustfmt wraps a long reason as `Some(\n "...",\n)`: whitespace after `Some(` and a
    # trailing comma before `)` are part of the committed shape (ProfileIFD 0xc6f5, whose
    # reason names three refusals, is the first edge long enough to wrap).
    r'unwalked:\s*(?P<unwalked>None|Some\(\s*"(?:[^"\\]|\\.)*"\s*,?\s*\))\s*,?\s*\}\s*,?\s*\)\s*$',
    re.S,
)
_OUT_OF_DATE = "-- the verifier's pattern is out of date; fix it before trusting a PASS"


def _some_int(text):
    """`Some(N)` -> N; `None` -> None."""
    return None if text == "None" else int(text[len("Some("):-1])


def _some_str_opt(text):
    """`Some("X")` -> "X"; `None` -> None (unlike `_some_str`, which maps
    None to "" for the groups convention)."""
    if text == "None":
        return None
    m = re.fullmatch(r'Some\(\s*"((?:[^"\\]|\\.)*)"\s*,?\s*\)', text.strip(), re.S)
    if m is None:
        raise SystemExit(f"unrecognised Some(string) value {text!r} {_OUT_OF_DATE}")
    return unescape(m.group(1))


def _parse_tag_groups_value(text, k):
    m = _TAG_GROUPS_VALUE_RE.match(text.strip())
    if m is None:
        raise SystemExit(f"{k}: unrecognised groups value {text!r} {_OUT_OF_DATE}")
    return (_some_str(m.group("g0")), _some_str(m.group("g1")), _some_str(m.group("g2")))


def _parse_ifd_flags(text, k):
    """-> (unknown, binary, list, protected, avoid, priority) as in the
    oracle's FLAGS row: five bools and an int-or-None."""
    text = re.sub(r"\s+", " ", text.strip())
    if text == "IfdFlags::NONE":
        return (False, False, False, False, False, None)
    m = _IFD_FLAGS_RE.match(text)
    if m is None:
        raise SystemExit(f"{k}: unrecognised flags value {text!r} {_OUT_OF_DATE}")
    return (
        m.group("unknown") == "true", m.group("binary") == "true", m.group("list") == "true",
        m.group("protected") == "true", m.group("avoid") == "true", _some_int(m.group("priority")),
    )


def _parse_omitted(text, k):
    """-> {member: bool} for all six `Omitted` members."""
    text = re.sub(r"\s+", " ", text.strip())
    if text == "Omitted::NONE":
        return {m: False for m in _OMITTED_MEMBERS}
    out = {}
    for member in _OMITTED_MEMBERS:
        mm = re.search(rf"\b{member}:\s*(true|false)\b", text)
        if mm is None:
            raise SystemExit(f"{k}: omitted value {text!r} has no `{member}:` {_OUT_OF_DATE}")
        out[member] = mm.group(1) == "true"
    return out


def _parse_ifd_subdir_value(text, k):
    """`None` -> None; `Some(IfdSubdirEdge { ... })` -> its facts. The
    `base:` value (`None` or `Some(&BaseExpr::...)`, nested arbitrarily) is
    located by `_value_span` and kept as whitespace-free text."""
    text = text.strip()
    if text == "None":
        return None
    m = _IFD_SUBDIR_EDGE_RE.match(text)
    if not m:
        raise SystemExit(f"{k}: unrecognised subdir value {text!r} {_OUT_OF_DATE}")
    base_begin = m.end()
    base_end = _value_span(text, base_begin)
    tm = _IFD_SUBDIR_TAIL_RE.match(text, base_end)
    if not tm:
        raise SystemExit(f"{k}: unrecognised subdir value (tail) {text!r} {_OUT_OF_DATE}")
    return {
        "module": m.group("module"),
        "table": m.group("table"),
        "start": (m.group("start_kind"), int(m.group("start_n"))),
        "base": re.sub(r"\s+", "", text[base_begin:base_end]),
        "byte_order": tm.group("byte_order"),
        "fix_format": re.sub(r"\s+", "", tm.group("fix_format")),
        "sub_ifd": tm.group("sub_ifd") == "true",
        "max_subdirs": _some_int(tm.group("max_subdirs")),
        "dir_name": _some_str_opt(tm.group("dir_name")),
        "validate": tm.group("validate") == "true",
        "unwalked": _some_str_opt(tm.group("unwalked")),
    }


class ParsedIfd(NamedTuple):
    """`parse_ifd_rust`'s result. `tags` maps `(module, table, key)` -- key
    `"22"` for a plain tag, `"22#1"` for a `_variants` alternative, exactly
    `oracle.pl`'s IFD key -- to a dict of that tag's facts; `variant_keys`
    says which are alternatives. `enums`/`bitmasks`/`other_ids`/
    `print_hexes`/`pc_kinds` are the shared `_parse_print_conv` outputs.
    `all_tables` is `ALL_IFD_TABLES`'s ident list in file order; `structure`
    collects invariant breaches (sortedness, disjointness, the ident list)
    that `verify_ifd` reports as mismatches."""
    tables: dict
    tags: dict
    variant_keys: set
    enums: dict
    bitmasks: dict
    other_ids: dict
    print_hexes: dict
    pc_kinds: dict
    all_tables: list
    structure: list


def _parse_one_ifd_tag(src, f, k, out):
    tag = {
        "id": int(f.group("id"), 0),
        "name": unescape(f.group("name")),
        "fmt": re.sub(r"\s+", "", f.group("fmt")),
        "count": _some_int(f.group("count")),
        "writable": _some_str_opt(f.group("writable")),
        "groups": _parse_tag_groups_value(f.group("groups"), k),
        "flags": _parse_ifd_flags(f.group("flags"), k),
        "omitted": _parse_omitted(f.group("omitted"), k),
        "raw_conv": unescape(f.group("member")) if f.group("member") is not None else None,
    }
    pc_start = f.end()
    pc_end = _value_span(src, pc_start)
    sm = _SUBDIR_LABEL_RE.match(src, pc_end)
    if not sm:
        raise SystemExit(
            f"{k}: no `, subdir:` immediately after the print_conv value "
            f"{src[pc_start:pc_end].strip()!r} {_OUT_OF_DATE}"
        )
    subdir_end = _value_span(src, sm.end())
    tag["subdir"] = _parse_ifd_subdir_value(src[sm.end():subdir_end], k)
    _parse_print_conv(
        src, pc_start, pc_end, k, out.enums, out.bitmasks, out.other_ids, out.print_hexes, out.pc_kinds,
    )
    tag["pc_kind"] = out.pc_kinds[k]
    out.tags[k] = tag


def parse_ifd_rust(path):
    """Parse every `IfdTable` static out of `path` -> `ParsedIfd`.

    Loud on anything it does not fully understand: an `IfdTable` header the
    pattern cannot read, a `tags:`/`variants:` array it cannot find, an
    `IfdTag {` it did not parse (counted per population AND over the whole
    file, so a table whose header failed cannot hide its tags inside the
    previous table's range), an `IfdVariantGroup {` it did not parse, a
    `print_conv`/`subdir`/`flags`/`omitted`/`groups` value in a shape it
    does not know. A verifier that parses zero tags reports zero mismatches,
    which is why every one of these is a SystemExit and not a warning.
    """
    with open(path, encoding="utf-8") as fh:
        src = fh.read()
    out = ParsedIfd({}, {}, set(), defaultdict(dict), {}, {}, {}, {}, [], [])

    heads = list(IFD_TABLE_RE.finditer(src))
    declared = len(IFD_TABLE_DECL_RE.findall(src))
    if declared != len(heads):
        raise SystemExit(
            f"parsed {len(heads)} IfdTable headers but {path} declares {declared} "
            f"{_OUT_OF_DATE}"
        )
    all_m = ALL_IFD_TABLES_RE.search(src)
    if all_m is None:
        raise SystemExit(f"no `pub static ALL_IFD_TABLES` in {path} {_OUT_OF_DATE}")
    a_start, a_end = _bracket_span(src, all_m.end() - 1)
    out.all_tables.extend(re.findall(r"&(\w+)", src[a_start:a_end]))

    anchors = sorted([m.start() for m in heads] + [all_m.start(), len(src)])
    expected_plain = expected_variant = 0
    for m in heads:
        start = m.start()
        end = min(a for a in anchors if a > start)
        mod, tbl, ident = m.group("module"), m.group("table"), m.group("ident")
        _, bb_end = _bracket_span(src, m.end() - 1)
        blocked = src[m.end():bb_end].strip() != ""

        tm = IFD_TAGS_MARKER_RE.search(src, bb_end, end)
        if tm is None:
            raise SystemExit(f"{mod}::{tbl}: no `tags: &[` member {_OUT_OF_DATE}")
        t_start, t_end = _bracket_span(src, tm.end() - 1)
        expected_plain += len(IFD_TAG_COUNT_RE.findall(src, t_start, t_end))
        ids = []
        for f in IFD_TAG_RE.finditer(src, t_start, t_end):
            # The generator spells ids `0x%04x`; the oracle keys rows by the
            # decimal Perl key. Normalise so the two meet.
            k = (mod, tbl, str(int(f.group("id"), 0)))
            if k in out.tags:
                out.structure.append(f"{k}: duplicate id in `tags`")
            _parse_one_ifd_tag(src, f, k, out)
            ids.append(int(f.group("id"), 0))

        vm = IFD_VARIANTS_MARKER_RE.search(src, t_end, end)
        if vm is None:
            raise SystemExit(f"{mod}::{tbl}: no `variants: &[` member {_OUT_OF_DATE}")
        v_start, v_end = _bracket_span(src, vm.end() - 1)
        declared_groups = len(IFD_VARIANT_GROUP_DECL_RE.findall(src, v_start, v_end))
        vids = []
        for gm in IFD_VARIANT_GROUP_RE.finditer(src, v_start, v_end):
            gid = int(gm.group("id"), 0)
            vids.append(gid)
            a_s, a_e = _bracket_span(src, gm.end() - 1)
            expected_variant += len(IFD_TAG_COUNT_RE.findall(src, a_s, a_e))
            for pos, f in enumerate(IFD_TAG_RE.finditer(src, a_s, a_e)):
                k = (mod, tbl, f"{gid}#{pos}")
                out.variant_keys.add(k)
                _parse_one_ifd_tag(src, f, k, out)
                if int(f.group("id"), 0) != gid:
                    out.structure.append(
                        f"{k}: alternative carries id {f.group('id')} inside group id {gid}"
                    )
        if len(vids) != declared_groups:
            raise SystemExit(
                f"{mod}::{tbl}: parsed {len(vids)} IfdVariantGroup literals of "
                f"{declared_groups} {_OUT_OF_DATE}"
            )

        # The schema's binary_search contract (`IfdTable::tag` /
        # `variant_group`): sorted, unique, and the two id spaces disjoint.
        if ids != sorted(ids) or len(set(ids)) != len(ids):
            out.structure.append(f"{mod}::{tbl}: `tags` is not sorted by unique id")
        if vids != sorted(vids) or len(set(vids)) != len(vids):
            out.structure.append(f"{mod}::{tbl}: `variants` is not sorted by unique id")
        if set(ids) & set(vids):
            out.structure.append(
                f"{mod}::{tbl}: ids {sorted(set(ids) & set(vids))} appear in both `tags` and `variants`"
            )
        key = (mod, tbl)
        if key in out.tables:
            out.structure.append(f"{mod}::{tbl}: declared twice")
        out.tables[key] = {
            "ident": ident,
            "groups": (m.group("group0"), m.group("group1"), m.group("group2")),
            "set_group1": _some_str_opt(m.group("set_group1")),
            "priority": _some_int(m.group("priority")),
            "gate_a_blocked": blocked,
        }

    got_plain = len(out.tags) - len(out.variant_keys)
    if got_plain != expected_plain:
        raise SystemExit(
            f"parsed {got_plain} plain IFD tags but `tags:` arrays contain "
            f"{expected_plain} {_OUT_OF_DATE}"
        )
    if len(out.variant_keys) != expected_variant:
        raise SystemExit(
            f"parsed {len(out.variant_keys)} IFD variant alternatives but "
            f"`alternatives:` arrays contain {expected_variant} {_OUT_OF_DATE}"
        )
    total = len(IFD_TAG_COUNT_RE.findall(src))
    if total != len(out.tags):
        raise SystemExit(
            f"parsed {len(out.tags)} IfdTag literals but {path} contains {total} "
            f"`IfdTag {{` {_OUT_OF_DATE}"
        )

    idents = {v["ident"]: key for key, v in out.tables.items()}
    if sorted(out.all_tables) != sorted(idents):
        out.structure.append(
            "ALL_IFD_TABLES does not list every IfdTable static exactly once: "
            f"{sorted(set(out.all_tables) ^ set(idents))}"
        )
    else:
        order = [idents[i] for i in out.all_tables]
        if order != sorted(order):
            out.structure.append("ALL_IFD_TABLES is not sorted by (module, table)")
    return out


class IfdOracle(NamedTuple):
    """`parse_ifd_oracle`'s result: `oracle.pl`'s `IFD` rows by kind, keyed
    `(module, table)` for the table rows and `(module, table, key)` for the
    rest. Text facts are kept verbatim; the verifier re-derives meaning."""
    tables: dict
    names: dict
    formats: dict
    counts: dict
    writables: dict
    enums: dict
    bitmasks: dict
    other_present: set
    other_print_hex: dict
    pcrefs: dict
    groups: dict
    flags: dict
    rawconvs: dict
    hooks: set
    conditions: set
    subdirs: dict
    # `PCEXPR` rows: keys whose ExifTool PrintConv is a scalar expression.
    pcexprs: set


def parse_ifd_oracle(out):
    """Read the `IFD` rows out of `oracle.pl`'s output (see its header for
    the row formats). An `IFD` row of a shape this reader does not know is
    a SystemExit: the oracle and the verifier move in lockstep, and a row
    silently ignored is a fact silently unverified."""
    o = IfdOracle({}, {}, {}, {}, {}, defaultdict(dict), defaultdict(dict), set(), {}, {},
                  {}, {}, {}, set(), set(), {}, set())
    for line in out.splitlines():
        p = line.split("\t")
        if p[0] != "IFD":
            continue
        n, kind = len(p), p[4] if len(p) > 4 else ""
        tk, k = (p[1], p[2]), (p[1], p[2], p[3])
        if kind == "TGROUPS" and n == 8:
            o.tables.setdefault(tk, {"set_group1": None, "priority": None})["tgroups"] = (p[5], p[6], p[7])
        elif kind == "SETGROUP1" and n == 6:
            o.tables.setdefault(tk, {"priority": None})["set_group1"] = p[5]
        elif kind == "PRIORITY" and n == 6:
            o.tables.setdefault(tk, {"set_group1": None})["priority"] = p[5]
        elif kind == "NAME" and n == 6:
            o.names[k] = p[5]
        elif kind == "FORMAT" and n == 6:
            o.formats[k] = p[5]
        elif kind == "COUNT" and n == 6:
            o.counts[k] = p[5]
        elif kind == "WRITABLE" and n == 6:
            o.writables[k] = p[5]
        elif kind == "ENUM" and n == 7:
            o.enums[k][p[5]] = p[6]
        elif kind == "BITMASK" and n == 7:
            o.bitmasks[k][p[5]] = p[6]
        elif kind == "OTHER" and n == 6:
            o.other_present.add(k)
            o.other_print_hex[k] = p[5] == "1"
        elif kind == "PCREF" and n == 6:
            o.pcrefs[k] = p[5]
        elif kind == "PCEXPR" and n == 6:
            o.pcexprs.add(k)
        elif kind == "GROUPS" and n == 8:
            o.groups[k] = (p[5], p[6], p[7])
        elif kind == "FLAGS" and n == 11:
            o.flags[k] = tuple(x == "1" for x in p[5:10]) + (None if p[10] == "-" else int(p[10]),)
        elif kind == "RAWCONV" and n == 6:
            o.rawconvs[k] = p[5]
        elif kind == "HOOK" and n == 6:
            o.hooks.add(k)
        elif kind == "CONDITION" and n == 6:
            o.conditions.add(k)
        elif kind == "SUBDIR" and n == 15:
            o.subdirs[k] = {
                "tagtable": p[5], "start": p[6], "base": p[7], "processproc": p[8],
                "byteorder": p[9], "validate": p[10] == "1", "fixformat": p[11],
                "subifd": p[12] == "1", "maxsubdirs": p[13], "dirname": p[14],
            }
        else:
            raise SystemExit(
                f"unrecognised IFD oracle row {line!r} -- oracle.pl and verify.py "
                "must move in lockstep; fix the reader before trusting a PASS"
            )
    return o


def _count_decl(text):
    """The `count:` an IFD tag must carry for a raw `Count` of `text` (None
    when the tag declares none): a non-negative integer verbatim; ExifTool's
    `Count => -1` ("variable") has no `u32` representation, so `None` is
    the only honest value there."""
    if text is None:
        return None
    try:
        n = int(text)
    except ValueError:
        return None
    return n if n >= 0 else None


def expected_ifd_format(spelling, count_decl):
    """-> `(fmt_literal, count)` the generated `IfdTag` must carry for a raw
    `Format` of `spelling` (None = absent) and a declared count of
    `count_decl`, or None when the spec refuses the spelling
    (`ifd_format_unsupported`: the tag must not be emitted at all).

    The spec (section 2, `format`/`count`): `fmt[N]` -> `Some(fmt)` with
    `count: Some(N)`; bare `string`/`undef` (and `binary`, ExifTool's alias of
    undef) -> the UNSIZED
    `Some(Fmt::Str(0))`/`Some(Fmt::Undef(0))` ("this kind, the entry's own
    byte length": a live reinterpretation, Exif.pm:6737-6745, which the walk
    honours) with the count from `Count`; any other scalar in the schema ->
    `Some(Fmt::X)`; anything else refused. `string[N]`/`undef[N]` go through
    the existing scalar parser's sized forms (`Fmt::Str(N)`/`Fmt::Undef(N)`)
    with `count: Some(N)`. `var_*` and `pstring` are ProcessBinaryData
    constructs and are refused here.
    """
    if spelling is None:
        return ("None", count_decl)
    m = _SIZED_FMT_RE.match(spelling)
    if m:
        base, n = m.group(1), int(m.group(2))
        if base == "string":
            return (f"Some(Fmt::Str({n}))", n)
        if base == "undef":
            return (f"Some(Fmt::Undef({n}))", n)
        if base in _FMT_VARIANT:
            return (f"Some(Fmt::{_FMT_VARIANT[base]})", n)
        return None
    if spelling == "string":
        return ("Some(Fmt::Str(0))", count_decl)
    if spelling in ("undef", "binary"):
        # `binary` IS the unsized undef, not an approximation of it: Exif.pm's
        # %formatNumber has 'binary' => 7 "(same as undef)" (Exif.pm:103-104),
        # %formatSize gives it width 1 (ExifTool.pm:6236), and ReadValue reads
        # "undef/binary/string" with one substr, NUL-truncating only string
        # (ExifTool.pm:6307-6311). A sized `binary[N]` stays refused below, as
        # codegen refuses it (IFD1 landing 1, G1).
        return ("Some(Fmt::Undef(0))", count_decl)
    if spelling in _FMT_VARIANT:
        return (f"Some(Fmt::{_FMT_VARIANT[spelling]})", count_decl)
    return None


# The one `RawConv` shape carried as data (`RawConvEffect::SetMember`): the
# data-member capture, in either of Perl's two spellings of the same
# dereference (`$$self{X}` and `$self->{X}` are the same expression), with
# an optional trailing `;`. The verifier judges truth, not spelling policy:
# a generator that accepts only `$$self{X}` refuses the other spelling
# (sound), and one that accepts both is not wrong to.
_SET_MEMBER_RE = re.compile(r"^\$(?:\$self\{|self->\{)(\w+)\}\s*=\s*\$val\s*;?$")
_IFD_START_RE = re.compile(r"^\$(valuePtr|val)(?:\s*([+-])\s*(\d+))?$")
# The spec's `ByteOrder` spellings (section 2). Anything else is refused by
# the generator; see `expected_ifd_edge` for how a refusal is scored.
_SPEC_BYTE_ORDERS = {
    "-": "Inherit", "LittleEndian": "Little", "II": "Little",
    "BigEndian": "Big", "MM": "Big", "Unknown": "Unknown",
}


def expected_ifd_edge(fact, enclosing=None):
    """Independently decide, from one oracle SUBDIR row, what edge the
    generator may emit -> `(edge_facts, spec_refuses)`. `enclosing` is the
    `(module, table)` the row belongs to.

    `edge_facts` is None when the facts put the edge outside the spec's
    grammar for a reason that makes any emitted edge WRONG (a TagTable
    present but unparseable; a Start outside `$valuePtr`/`$val` +- n; a
    Base outside the arithmetic grammar; a FixFormat the schema cannot
    spell; a MaxSubdirs that is not a count). Two shapes are expected
    EMITTED BUT UNWALKED (`edge_facts["unwalked"]` True; spec v1.1, slice
    IFD1): no TagTable at all -- ExifTool walks the pointer with the
    enclosing table (Exif.pm:6939-6944), so the edge must name `enclosing`
    -- and a ProcessProc other than ProcessBinaryData; the generated edge
    must carry `unwalked: Some(..)` for exactly these and `None` otherwise.
    `spec_refuses` is True when the only thing outside the spec is the
    ByteOrder spelling: the spec lists six spellings, ExifTool itself
    (Exif.pm:6974-6990) reads `/^Little/i`, `/^Big/i` and treats everything
    else as detect-from-entry-count, so an edge emitted for, say,
    `'Little-endian'` with `IfdByteOrder::Little` is not a wrong fact --
    `verify_ifd` accepts either that or a refusal for it, and counts the
    former as a note."""
    unwalked = False
    if fact["tagtable"] == "-":
        if enclosing is None:
            return None, False
        module, table = enclosing
        unwalked = True
    else:
        m = _TAGTABLE_RE.match(fact["tagtable"])
        if not m:
            return None, False
        module, table = m.group(1), m.group(2)
    proc = fact["processproc"]
    if proc != "-" and not proc.endswith("::ProcessBinaryData"):
        unwalked = True
    start = fact["start"]
    if start == "-":
        start_v = ("ValuePtr", 0)
    else:
        sm = _IFD_START_RE.match(start.strip())
        if not sm:
            return None, False
        n = int(sm.group(3) or 0)
        if sm.group(2) == "-":
            n = -n
        start_v = ("ValuePtr" if sm.group(1) == "valuePtr" else "Val", n)
    base = fact["base"]
    if base != "-" and not _arith_is_well_formed(base, {"start", "base"}):
        return None, False
    bo = fact["byteorder"]
    spec_refuses = False
    if bo in _SPEC_BYTE_ORDERS:
        bo_v = _SPEC_BYTE_ORDERS[bo]
    else:
        spec_refuses = True
        if re.match(r"Little", bo, re.I):
            bo_v = "Little"
        elif re.match(r"Big", bo, re.I):
            bo_v = "Big"
        else:
            bo_v = "Unknown"
    fix = fact["fixformat"]
    if fix in ("-", "ifd"):
        fix_v = "None"
    elif fix in _FMT_VARIANT:
        fix_v = f"Some(Fmt::{_FMT_VARIANT[fix]})"
    else:
        return None, False
    if fact["maxsubdirs"] == "-":
        max_v = None
    elif fact["maxsubdirs"].isdigit():
        max_v = int(fact["maxsubdirs"])
    else:
        return None, False
    return {
        "module": module,
        "table": table,
        "start": start_v,
        "base_present": base != "-",
        "byte_order": bo_v,
        "fix_format": fix_v,
        "sub_ifd": fact["subifd"],
        "max_subdirs": max_v,
        "dir_name": None if fact["dirname"] == "-" else fact["dirname"],
        "validate": fact["validate"],
        "unwalked": unwalked,
    }, spec_refuses


class _Tally:
    """match / MISMATCH counter with a bounded example list."""

    def __init__(self, show):
        self.ok = 0
        self.bad = 0
        self.examples = []
        self.show = show

    def hit(self):
        self.ok += 1

    def miss(self, example):
        self.bad += 1
        if len(self.examples) < self.show:
            self.examples.append(example)

    @property
    def total(self):
        return self.ok + self.bad


def verify_ifd(gen, orc, show=10):
    """Compare a `ParsedIfd` with an `IfdOracle` -> `(report_lines, failed)`.
    `failed` is the number of discrepancies (0 = PASS for this stage)."""
    T = lambda: _Tally(show)  # noqa: E731
    t_table, t_tgroups, t_setg1, t_prio = T(), T(), T(), T()
    t_name, t_fmt, t_writable, t_groups, t_flags = T(), T(), T(), T(), T()
    t_hook, t_subdir_flag, t_cond, t_rawconv, t_pcref = T(), T(), T(), T(), T()
    t_enum, t_bitmask, t_other, t_edge = T(), T(), T(), T()
    bitmask_refused = 0  # BITMASK PrintConvs the generator refused (omitted.print_conv), a note
    note_pcexpr_unflagged = 0  # expression PrintConvs emitted raw and unflagged (Gate A's expr_unsupported)
    orphan_tables, orphan_tags, orphan_examples = 0, 0, []
    unsupported_fmt = []
    variant_orphan = 0
    note_edge_refused_modelable = 0
    note_byteorder_outside_spec = 0
    note_rawconv_refused_setmember = 0

    # --- tables ---------------------------------------------------------
    for (mod, tbl), tf in sorted(gen.tables.items()):
        o = orc.tables.get((mod, tbl))
        if o is None or "tgroups" not in o:
            orphan_tables += 1
            t_table.miss(((mod, tbl), "not an IFD-scope table in ExifTool"))
            continue
        t_table.hit()
        raw = o["tgroups"]
        want = (raw[0] or mod, raw[1] or mod, raw[2] or "Other")
        if tf["groups"] == want:
            t_tgroups.hit()
        else:
            t_tgroups.miss(((mod, tbl), tf["groups"], want, raw))
        if tf["set_group1"] == o.get("set_group1"):
            t_setg1.hit()
        else:
            t_setg1.miss(((mod, tbl), tf["set_group1"], o.get("set_group1")))
        want_prio = o.get("priority")
        try:
            want_prio = None if want_prio is None else int(want_prio)
        except ValueError:
            pass
        if tf["priority"] == want_prio:
            t_prio.hit()
        else:
            t_prio.miss(((mod, tbl), tf["priority"], want_prio))

    # --- tags -----------------------------------------------------------
    for k, tag in gen.tags.items():
        is_variant = k in gen.variant_keys
        truth = orc.names.get(k)
        if truth is None:
            if is_variant:
                variant_orphan += 1
            orphan_tags += 1
            if len(orphan_examples) < show:
                orphan_examples.append(k)
            continue
        if truth == tag["name"]:
            t_name.hit()
        else:
            t_name.miss((k, tag["name"], truth))

        want_fmt = expected_ifd_format(orc.formats.get(k), _count_decl(orc.counts.get(k)))
        got_fmt = (tag["fmt"], tag["count"])
        if want_fmt is None:
            unsupported_fmt.append((k, orc.formats.get(k), got_fmt))
        elif got_fmt == want_fmt:
            t_fmt.hit()
        else:
            t_fmt.miss((k, got_fmt, want_fmt, orc.formats.get(k), orc.counts.get(k)))

        want_w = orc.writables.get(k)
        if tag["writable"] == want_w:
            t_writable.hit()
        else:
            t_writable.miss((k, tag["writable"], want_w))

        want_g = orc.groups.get(k, ("", "", ""))
        if tag["groups"] == want_g:
            t_groups.hit()
        else:
            t_groups.miss((k, tag["groups"], want_g))

        want_f = orc.flags.get(k, (False, False, False, False, False, None))
        if tag["flags"] == want_f:
            t_flags.hit()
        else:
            t_flags.miss((k, tag["flags"], want_f))

        om = tag["omitted"]
        for tally, member, present in (
            (t_hook, "hook", k in orc.hooks),
            (t_subdir_flag, "subdirectory", k in orc.subdirs),
        ):
            if om[member] == present:
                tally.hit()
            else:
                tally.miss((k, f"omitted.{member}", om[member], present))
        # A plain entry's Condition is Perl the walk cannot run (withheld);
        # an alternative's Condition is the compiled `Cond` that selected
        # it, so the flag must be clear there (the spec's atomic refusal
        # otherwise drops the whole group).
        want_cond = False if is_variant else k in orc.conditions
        if om["condition"] == want_cond:
            t_cond.hit()
        else:
            t_cond.miss((k, "omitted.condition", om["condition"], want_cond))

        rc = orc.rawconvs.get(k)
        member = tag["raw_conv"]
        if member is not None:
            sm = _SET_MEMBER_RE.match(rc) if rc is not None else None
            if om["raw_conv"]:
                t_rawconv.miss((k, f"SetMember {member!r} together with omitted.raw_conv"))
            elif rc is None:
                t_rawconv.miss((k, f"SetMember {member!r} but ExifTool has no RawConv"))
            elif sm is None or sm.group(1) != member:
                t_rawconv.miss((k, f"SetMember {member!r} but RawConv is {rc!r}"))
            else:
                t_rawconv.hit()
        elif om["raw_conv"] and rc is None:
            t_rawconv.miss((k, "omitted.raw_conv but ExifTool has no RawConv"))
        elif rc is not None and not om["raw_conv"]:
            t_rawconv.miss((k, f"RawConv {rc!r} dropped silently"))
        else:
            if rc is not None and _SET_MEMBER_RE.match(rc):
                note_rawconv_refused_setmember += 1
            t_rawconv.hit()

        # The binary stage's PCREF three-way check, widened to every kind of
        # ExifTool PrintConv: a refusal (`Omitted { print_conv: true }`) is
        # legitimate iff ExifTool declares SOME PrintConv here -- a code ref
        # (PCREF), a scalar expression (PCEXPR), or a hash the generator would
        # not reproduce (ENUM/BITMASK/OTHER rows; e.g. a BITMASK with
        # BitsPerWord) -- and a refusal of nothing is a false refusal. A
        # PCREF emitted unflagged as `PrintConv::None` is the dangerous
        # direction, as before. An expression PrintConv emitted unflagged as
        # `PrintConv::None` (the generator's `expr_unsupported` path, which
        # Gate A disqualifies at table level) is counted as a note here, the
        # same way the binary stage tolerates it.
        flagged, is_ref = om["print_conv"], k in orc.pcrefs
        has_pc = (is_ref or k in orc.pcexprs or k in orc.enums or k in orc.bitmasks
                  or k in orc.other_present)
        if flagged and not has_pc:
            t_pcref.miss((k, "print_conv refused but ExifTool has no PrintConv"))
        elif is_ref and not flagged and tag["pc_kind"] == "PrintConv::None":
            t_pcref.miss((k, f"ExifTool PrintConv is a {orc.pcrefs[k]} ref, dropped silently"))
        else:
            if k in orc.pcexprs and not flagged and tag["pc_kind"] == "PrintConv::None":
                note_pcexpr_unflagged += 1
            t_pcref.hit()

        truth_enum = {norm_key(a): b for a, b in orc.enums.get(k, {}).items()}
        for kk, vv in gen.enums.get(k, {}).items():
            t = truth_enum.get(norm_key(kk))
            if t == vv:
                t_enum.hit()
            else:
                t_enum.miss((k, kk, vv, t))
        if k in gen.bitmasks:
            got = {norm_key(kk): vv for kk, vv in gen.bitmasks[k].items()}
            want = {norm_key(kk): vv for kk, vv in orc.bitmasks.get(k, {}).items()}
            if got == want:
                t_bitmask.hit()
            else:
                t_bitmask.miss((k, got, want))
        elif k in orc.bitmasks:
            # ExifTool declares a BITMASK the generator did not emit. A
            # counted refusal (`omitted.print_conv: true`, e.g. codegen's
            # `ifd_bitmask_words_unsupported` for a BitsPerWord hash) is
            # honest absence -- a note; an emitted PrintConv that silently
            # lost the BITMASK is a mismatch.
            if gen.tags[k]["omitted"].get("print_conv"):
                bitmask_refused += 1
            else:
                t_bitmask.miss((k, {}, {norm_key(kk): vv for kk, vv in orc.bitmasks[k].items()}))
        variant = gen.other_ids.get(k)
        if variant is not None:
            if k not in orc.other_present:
                t_other.miss((k, f"other: Some(OtherId::{variant})", "no OTHER in ExifTool"))
            elif gen.print_hexes.get(k, False) != orc.other_print_hex.get(k, False):
                t_other.miss((k, f"print_hex: {gen.print_hexes.get(k, False)}",
                              f"PrintHex: {orc.other_print_hex.get(k, False)}"))
            else:
                t_other.hit()

        edge = tag["subdir"]
        fact = orc.subdirs.get(k)
        if edge is not None:
            if not om["subdirectory"]:
                t_edge.miss((k, "edge emitted without omitted.subdirectory"))
            elif fact is None:
                t_edge.miss((k, "edge emitted but ExifTool has no SubDirectory"))
            else:
                want, spec_refuses = expected_ifd_edge(fact, (k[0], k[1]))
                if want is None:
                    t_edge.miss((k, "edge emitted where the facts require a refusal", fact))
                else:
                    diffs = [
                        f for f in ("module", "table", "start", "byte_order", "fix_format",
                                    "sub_ifd", "max_subdirs", "dir_name", "validate")
                        if edge[f] != want[f]
                    ]
                    if (edge["base"] != "None") != want["base_present"]:
                        diffs.append("base")
                    if (edge["unwalked"] is not None) != want["unwalked"]:
                        diffs.append("unwalked")
                    if diffs:
                        t_edge.miss((k, {f: edge[f] for f in diffs},
                                     {f: want.get(f, want.get("base_present")) for f in diffs}))
                    else:
                        t_edge.hit()
                        if spec_refuses:
                            note_byteorder_outside_spec += 1
        elif fact is not None and om["subdirectory"]:
            want, spec_refuses = expected_ifd_edge(fact, (k[0], k[1]))
            if want is not None and not spec_refuses:
                note_edge_refused_modelable += 1

    structure_bad = len(gen.structure)
    failed = (
        orphan_tables + t_tgroups.bad + t_setg1.bad + t_prio.bad + orphan_tags
        + t_name.bad + t_fmt.bad + len(unsupported_fmt) + t_writable.bad + t_groups.bad
        + t_flags.bad + t_hook.bad + t_subdir_flag.bad + t_cond.bad + t_rawconv.bad
        + t_pcref.bad + t_enum.bad + t_bitmask.bad + t_other.bad + t_edge.bad + structure_bad
    )
    n_edges = sum(1 for t in gen.tags.values() if t["subdir"] is not None)
    lines = [
        "",
        "IFD tables (slice I-1)",
        f"tables checked   {t_table.total}",
        f"  match          {t_table.ok}",
        f"  not in oracle  {orphan_tables}",
        f"  effective groups MISMATCH {t_tgroups.bad}   set_group1 MISMATCH {t_setg1.bad}   "
        f"priority MISMATCH {t_prio.bad}",
        f"  structure (sort order / disjoint ids / ALL_IFD_TABLES) MISMATCH {structure_bad}",
        f"tags checked     {len(gen.tags)}  (variant alternatives {len(gen.variant_keys)})",
        f"  name match     {t_name.ok}",
        f"  name MISMATCH  {t_name.bad}",
        f"  not in oracle  {orphan_tags}  (of which variant alternatives {variant_orphan})",
        f"  format+count   {t_fmt.total}  MISMATCH {t_fmt.bad}  "
        f"unsupported format emitted anyway {len(unsupported_fmt)}",
        f"  writable       {t_writable.total}  MISMATCH {t_writable.bad}",
        f"  groups         {t_groups.total}  MISMATCH {t_groups.bad}",
        f"  flags          {t_flags.total}  MISMATCH {t_flags.bad}",
        f"  hook/subdirectory/condition flags MISMATCH {t_hook.bad}/{t_subdir_flag.bad}/{t_cond.bad}",
        f"  raw_conv       {t_rawconv.total}  MISMATCH {t_rawconv.bad}  "
        f"(SetMember-shaped RawConvs refused instead: {note_rawconv_refused_setmember})",
        f"  print_conv refusals {t_pcref.total}  MISMATCH {t_pcref.bad}  (expression PrintConv emitted unflagged, Gate-A territory, note: {note_pcexpr_unflagged})",
        f"enum entries     {t_enum.total}",
        f"  match          {t_enum.ok}",
        f"  MISMATCH       {t_enum.bad}",
        f"BITMASK tags     {t_bitmask.total}  MISMATCH {t_bitmask.bad}  (refused PrintConv, note: {bitmask_refused})",
        f"OTHER-backed tags {t_other.total}  MISMATCH {t_other.bad}",
        f"subdirectory edges {n_edges}",
        f"  match          {t_edge.ok}",
        f"  MISMATCH       {t_edge.bad}",
        f"  refused though the facts were modelable (note, not a mismatch) "
        f"{note_edge_refused_modelable}",
        f"  ByteOrder spelling outside the spec's six, emitted per ExifTool's rule (note) "
        f"{note_byteorder_outside_spec}",
        f"IFD MISMATCH total {failed}",
    ]
    for tally, label in (
        (t_table, "table"), (t_tgroups, "table groups"), (t_setg1, "set_group1"),
        (t_prio, "table priority"), (t_name, "name"), (t_fmt, "format"), (t_writable, "writable"),
        (t_groups, "tag groups"), (t_flags, "flags"), (t_hook, "hook flag"),
        (t_subdir_flag, "subdirectory flag"), (t_cond, "condition flag"), (t_rawconv, "raw_conv"),
        (t_pcref, "print_conv flag"), (t_enum, "enum"), (t_bitmask, "bitmask"), (t_other, "other"),
        (t_edge, "subdir edge"),
    ):
        for ex in tally.examples:
            lines.append(f"  {label}  {ex!r}")
    for k in orphan_examples:
        lines.append(f"  orphan tag {k}")
    for ex in unsupported_fmt[:show]:
        lines.append(f"  format  {ex[0]}: Format {ex[1]!r} is outside the schema but was "
                     f"emitted as {ex[2]!r} -- it must be refused, not approximated")
    for s in gen.structure[:show]:
        lines.append(f"  structure  {s}")
    return lines, failed


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("generated_rs")
    ap.add_argument("exiftool_lib")
    ap.add_argument("--oracle", default="oracle.pl")
    ap.add_argument("--show", type=int, default=10)
    ap.add_argument(
        "--ifd-generated", default=None,
        help="slice I-1's src/exiftool_tables/ifd_tables.rs (default: the file of that "
             "name beside GENERATED_RS); the IFD stage is skipped, with a message, when "
             "it does not exist",
    )
    args = ap.parse_args()

    version = check_version(args.generated_rs, args.exiftool_lib)
    ifd_path = (
        Path(args.ifd_generated) if args.ifd_generated
        else Path(args.generated_rs).with_name("ifd_tables.rs")
    )
    ifd_present = ifd_path.is_file()

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "verify.py")
    missing = exiftool_oracle.missing_modules(_PERL)
    capability = (
        f"perl {_PERL} loads {', '.join(exiftool_oracle.REQUIRED_MODULES)}: OK"
        if not missing else
        f"perl {_PERL} DEGRADED -- missing {', '.join(missing)} "
        "(affects only tests that need those modules; this verifier does not)"
    )
    instrument.print_header(
        tool="verify.py",
        git=git,
        dirty_overridden=dirty_overridden,
        extra=[
            f"exiftool: {version} (lib {args.exiftool_lib})",
            f"perl:    {_PERL} -- {capability}",
            f"target:  {args.generated_rs}",
            f"ifd:     {ifd_path}" + ("" if ifd_present else "  (absent -> IFD stage skipped)"),
        ],
    )
    (
        gen_fields, gen_enums, gen_masks, gen_hooks, gen_subdirs, gen_subdir_edges,
        gen_sound_until, gen_variant_keys, gen_bitmasks, gen_other_ids, gen_print_hexes,
        gen_pc_refused, gen_pc_kinds, gen_formats, gen_table_groups, gen_tag_groups,
    ) = parse_rust(args.generated_rs)
    oracle_out = run_oracle(args.exiftool_lib, args.oracle)
    (
        or_names, or_enums, or_masks, or_hooks, or_subdirs, or_subdir_facts, or_varfmts,
        or_bitmasks, or_other_present, or_other_print_hex, or_pcrefs,
        or_rawfmts, or_tblgroups, or_taggroups,
    ) = parse_binary_oracle(oracle_out)

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

    # Step 25: a `PrintConv::Bitmask` field's `bits:` array, checked the same
    # way `masks` is just above (union of both sides, so a bit label the
    # generator invented and one it silently dropped are equally a mismatch).
    bitmask_ok = bitmask_bad = 0
    bitmask_examples = []
    for k in set(gen_bitmasks) | {k for k in or_bitmasks if k in gen_fields}:
        got = {norm_key(kk): vv for kk, vv in gen_bitmasks.get(k, {}).items()}
        want = {norm_key(kk): vv for kk, vv in or_bitmasks.get(k, {}).items()}
        if got == want:
            bitmask_ok += 1
        else:
            bitmask_bad += 1
            if len(bitmask_examples) < args.show:
                bitmask_examples.append((k, got, want))

    # Step 25: does a `PrintConv::PartialEnumInt` field's registered `other:`
    # correspond to a tag that ExifTool's own PrintConv hash really has an
    # `OTHER` closure on? A field where this is false would mean
    # `others.py`'s deparse-text registry somehow matched a closure that
    # is not actually present on this tag -- exactly the kind of "the
    # oracle disagrees with the generator's own bookkeeping" bug this
    # verifier's whole purpose is to catch. `print_hex` is cross-checked
    # against the SAME tag-level `PrintHex` fact `codegen.py`'s `conv_for`
    # reads (see oracle.pl's OTHER row doc). Only fields where `codegen.py`
    # actually resolved an `other:` are checked -- an unregistered OTHER
    # never reaches this schema at all (refused, per others.py's doctrine),
    # so there is nothing here for a field with `other: None` to check
    # against.
    other_ok = other_bad = 0
    other_examples = []
    for k, variant in gen_other_ids.items():
        if variant is None:
            continue
        if k not in or_other_present:
            other_bad += 1
            if len(other_examples) < args.show:
                other_examples.append((k, f"other: Some(OtherId::{variant})", "no OTHER in ExifTool"))
            continue
        got_hex, want_hex = gen_print_hexes.get(k, False), or_other_print_hex.get(k, False)
        if got_hex == want_hex:
            other_ok += 1
        else:
            other_bad += 1
            if len(other_examples) < args.show:
                other_examples.append((k, f"print_hex: {got_hex}", f"PrintHex: {want_hex}"))

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

    # Step 28's `Omitted.print_conv`, checked BOTH ways round against
    # `oracle.pl`'s PCREF rows (read straight out of the live Perl hash, not
    # from dump_tables.pl's JSON):
    #
    #   flagged but no PCREF  -- the generator refused a conversion ExifTool
    #                            does not have, so a field is withheld for no
    #                            reason. A false refusal, i.e. a lost tag.
    #   PCREF, not flagged,   -- the DANGEROUS direction, and the one this
    #   PrintConv::None          whole flag exists for: ExifTool renders this
    #                            field through a Perl sub, the generator
    #                            reproduced nothing, and the field is emitted
    #                            anyway carrying its raw value under
    #                            ExifTool's own tag name.
    #
    # A PCREF field that is NOT flagged and DOES carry a `PrintConv::Expr` is
    # correct: `exprs.py`'s CODE_REFS recognised the sub and the translation
    # shipped. `verify_exprs.py` is what checks that translation against the
    # sub itself; this check only establishes that no third state exists.
    pcref_ok = pcref_false_refusal = pcref_silent_drop = 0
    pcref_translated = 0
    pcref_examples = []
    for k in gen_fields:
        flagged, is_ref = k in gen_pc_refused, k in or_pcrefs
        if flagged and not is_ref:
            pcref_false_refusal += 1
            if len(pcref_examples) < args.show:
                pcref_examples.append((k, "flagged but ExifTool has no PrintConv ref"))
        elif is_ref and not flagged:
            if gen_pc_kinds.get(k) == "PrintConv::None":
                pcref_silent_drop += 1
                if len(pcref_examples) < args.show:
                    pcref_examples.append(
                        (k, f"ExifTool PrintConv is a {or_pcrefs[k]} ref, dropped silently")
                    )
            else:
                pcref_translated += 1
                pcref_ok += 1
        else:
            pcref_ok += 1

    # Step 26: every emitted field's width. Before this column existed the
    # verifier checked each field's name, enum, mask, hook and subdirectory
    # but never how many bytes it reads, so a format transcribed at the wrong
    # size would have decoded its neighbour's bytes under a correct ExifTool
    # tag name and passed the entire suite. `expected_fmt_literal` re-derives
    # the answer from the oracle's raw Format spelling and ExifTool's own
    # %formatSize semantics, NOT by importing codegen.py's SCALAR_FORMATS --
    # a shared table would confirm its own typo.
    fmt_ok = fmt_bad = 0
    fmt_examples = []
    for k, got in gen_formats.items():
        want = expected_fmt_literal(or_rawfmts.get(k))
        if want == got:
            fmt_ok += 1
        else:
            fmt_bad += 1
            if len(fmt_examples) < args.show:
                fmt_examples.append((k, got, want, or_rawfmts.get(k)))

    # Step 26, effective groups. Two independent properties:
    #
    #   1. the TABLE's groups equal GetTagTable's defaulting
    #      (ExifTool.pm:8980-8991) applied to the module's RAW GROUPS hash --
    #      re-derived here from the oracle's TGROUPS row, not read back from
    #      codegen.py's arithmetic;
    #   2. each FIELD's stored overrides are exactly the tag's own `Groups`
    #      keys (ExifTool.pm:9236-9244) -- no more (inventing a group) and no
    #      fewer (silently inheriting where ExifTool overrides).
    #
    # The table half matters most: the raw hash is usually partial, so a
    # generator that copied it verbatim recorded an empty group 0 for a third
    # of all tables and nothing would have noticed.
    group_ok = group_bad = 0
    group_examples = []
    for (mod, tbl), got in gen_table_groups.items():
        raw = or_tblgroups.get((mod, tbl))
        if raw is None:
            continue
        # `$$defaultGroups{0} = $1 unless $$defaultGroups{0}` where $1 is the
        # module name, then `{2} = 'Other' unless {2}`.
        want = (raw[0] or mod, raw[1] or mod, raw[2] or "Other")
        if got == want:
            group_ok += 1
        else:
            group_bad += 1
            if len(group_examples) < args.show:
                group_examples.append(((mod, tbl), got, want, raw))

    tag_group_ok = tag_group_bad = 0
    tag_group_examples = []
    for k, got in gen_tag_groups.items():
        want = or_taggroups.get(k, ("", "", ""))
        if got == want:
            tag_group_ok += 1
        else:
            tag_group_bad += 1
            if len(tag_group_examples) < args.show:
                tag_group_examples.append((k, got, want))

    # The other half of the same property: a field whose Format this schema
    # cannot express must have been REFUSED, not emitted at the table's
    # default width. Without this, dropping a format from SCALAR_FORMATS
    # would silently start reporting those fields at the wrong size.
    unsupported_emitted = [
        (k, or_rawfmts[k], gen_formats[k])
        for k in gen_formats
        if k in or_rawfmts and expected_fmt_literal(or_rawfmts[k]) is None
    ]


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
    print(f"BITMASK fields (Step 25) {bitmask_ok + bitmask_bad}")
    print(f"  match          {bitmask_ok}")
    print(f"  MISMATCH       {bitmask_bad}")
    print(f"OTHER-backed fields (Step 25) {other_ok + other_bad}")
    print(f"  match          {other_ok}")
    print(f"  MISMATCH       {other_bad}")
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
    print(f"print_conv flags checked (Step 28) {pcref_ok + pcref_false_refusal + pcref_silent_drop}")
    print(f"  accounted for  {pcref_ok}  (ExifTool PrintConv refs: {pcref_translated} translated, {len(gen_pc_refused)} refused)")
    print(f"  MISMATCH: refused with no ExifTool PrintConv ref {pcref_false_refusal}")
    print(f"  MISMATCH: ExifTool PrintConv ref dropped silently {pcref_silent_drop}")
    print(f"table effective groups (Step 26) {group_ok + group_bad}")
    print(f"  match          {group_ok}")
    print(f"  MISMATCH       {group_bad}")
    print(f"per-tag group overrides (Step 26) {tag_group_ok + tag_group_bad}")
    print(f"  match          {tag_group_ok}")
    print(f"  MISMATCH       {tag_group_bad}")
    print(f"field formats (Step 26) {fmt_ok + fmt_bad}")
    print(f"  match          {fmt_ok}")
    print(f"  MISMATCH       {fmt_bad}")
    print(f"  unsupported format emitted anyway {len(unsupported_emitted)}")

    for k, got, want in bad_examples:
        print(f"  name  {k}: generated {got!r} != exiftool {want!r}")
    for k in orphan_examples:
        print(f"  orphan {k}")
    for k, kk, got, want in enum_examples:
        print(f"  enum  {k} key {kk}: generated {got!r} != exiftool {want!r}")
    for k, got, want in mask_examples:
        print(f"  mask  {k}: generated {got!r} != exiftool {want!r}")
    for k, got, want in bitmask_examples:
        print(f"  bitmask  {k}: generated {got!r} != exiftool {want!r}")
    for k, got, want in other_examples:
        print(f"  other  {k}: generated {got!r} != exiftool {want!r}")
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
    for k, why in pcref_examples:
        print(f"  print_conv flag  {k}: {why}")
    for k, got, want, raw in group_examples:
        print(f"  table groups  {k}: generated {got!r} != expected {want!r} (raw GROUPS {raw!r})")
    for k, got, want in tag_group_examples:
        print(f"  tag groups  {k}: generated {got!r} != exiftool {want!r}")
    for k, got, want, raw in fmt_examples:
        print(f"  format  {k}: generated {got!r} != expected {want!r} (Format {raw!r})")
    for k, raw, got in unsupported_emitted[:args.show]:
        print(f"  format  {k}: Format {raw!r} is outside the schema but was "
              f"emitted as {got!r} -- it must be refused, not approximated")

    # Slice I-1: the IFD stage, from the same oracle run. Skipped -- never
    # silently passed -- when the generated file is not on this tree.
    ifd_failed = 0
    if ifd_present:
        stamped = VERSION_RE.search(ifd_path.read_text(encoding="utf-8"))
        if stamped and stamped.group(1) != version:
            raise SystemExit(
                f"ExifTool pin skew: {ifd_path} was transcribed from {stamped.group(1)}, "
                f"but the binary tables and the pin say {version} -- regenerate both together"
            )
        gen_ifd = parse_ifd_rust(ifd_path)
        if not gen_ifd.tables:
            sys.exit(f"parsed 0 IfdTable statics from {ifd_path} -- verifier is broken, "
                     "not the generator; fix the parser before trusting a PASS")
        ifd_lines, ifd_failed = verify_ifd(gen_ifd, parse_ifd_oracle(oracle_out), args.show)
        print("\n".join(ifd_lines))
    else:
        print(f"\nIFD tables (slice I-1): SKIPPED -- {ifd_path} does not exist on this tree "
              "(the generator has not produced it); the binary verification above stands alone")

    failed = (
        name_bad + enum_bad + orphan + mask_bad + bitmask_bad + other_bad
        + variant_name_bad + variant_enum_bad + variant_orphan + variant_mask_bad
        + hook_bad + subdir_bad + edge_bad + sound_bad
        + pcref_false_refusal + pcref_silent_drop
        + fmt_bad + len(unsupported_emitted) + group_bad + tag_group_bad
        + ifd_failed
    )
    print("\nRESULT:", "PASS" if failed == 0 else f"FAIL ({failed} discrepancies)")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
