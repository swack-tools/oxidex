"""Extract per-field write metadata for a (module, table) pair directly from
the ALREADY-VERIFIED src/exiftool_tables/binary_tables.rs, rather than
re-deriving format/count/enum data from the raw dump_tables.pl JSON.

Why this instead of tables.json: binary_tables.rs is the output that
`tools/exiftool-tables/verify.py` (== `just verify-tables`, a CI gate) checks
field-by-field against ExifTool itself. Using it as the source for sample
generation means every field this harness attempts is one whose format/count/
enum mapping is independently verified correct, not just parsed by this
script's own (unverified) reading of dump_tables.pl's JSON.
"""
from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

FMT_SCALAR = {
    "Int8u", "Int8s", "Int16u", "Int16s", "Int16uRev", "Int32u", "Int32s",
    "Float", "Double", "Rational64u", "Rational64s",
}


@dataclass
class RField:
    name: str
    format: str  # Fmt variant name, e.g. "Int16u", or "Str(12)" / "Undef(4)"
    count: int
    masked: bool
    omitted_any: bool
    enum_pairs: list[tuple[str, str]] | None  # raw-key -> label, if IntEnum/StrEnum


def _find_block(text: str, module: str, table: str) -> str | None:
    needle = f'module: "{module}",\n    table: "{table}",'
    i = text.find(needle)
    if i == -1:
        return None
    # walk backward to "= BinaryTable {"
    start = text.rfind("= BinaryTable {", 0, i)
    if start == -1:
        return None
    brace_start = start + len("= BinaryTable ")
    depth = 0
    j = brace_start
    while j < len(text):
        if text[j] == "{":
            depth += 1
        elif text[j] == "}":
            depth -= 1
            if depth == 0:
                return text[brace_start:j + 1]
        j += 1
    return None


def _split_top_level_fields(fields_body: str) -> list[str]:
    """`fields_body` is the text between `fields: &[` and its matching `]`.
    Split into individual `Field { ... }` chunks (brace-balanced)."""
    chunks = []
    depth = 0
    cur = []
    i = 0
    n = len(fields_body)
    while i < n:
        c = fields_body[i]
        if c == "{":
            if depth == 0:
                cur = []
            depth += 1
            cur.append(c)
        elif c == "}":
            depth -= 1
            cur.append(c)
            if depth == 0:
                chunks.append("".join(cur))
        elif depth > 0:
            cur.append(c)
        i += 1
    return chunks


def parse_table_fields(rs_text: str, module: str, table: str) -> tuple[str, list[RField]] | None:
    block = _find_block(rs_text, module, table)
    if block is None:
        return None
    m = re.search(r"default_format:\s*Fmt::(\w+(?:\([^)]*\))?)", block)
    default_format = m.group(1) if m else "Int8u"

    fm = re.search(r"fields:\s*&\[(.*)\],\s*variants:", block, re.DOTALL)
    if not fm:
        return default_format, []
    fields_body = fm.group(1)
    out: list[RField] = []
    for chunk in _split_top_level_fields(fields_body):
        nm = re.search(r'name:\s*"([^"]+)"', chunk)
        if not nm:
            continue
        name = nm.group(1)
        fmtm = re.search(r"format:\s*Some\(Fmt::(\w+(?:\([^)]*\))?)\)", chunk)
        fmt = fmtm.group(1) if fmtm else default_format
        cm = re.search(r"count:\s*(\d+)", chunk)
        count = int(cm.group(1)) if cm else 1
        masked = "mask: Some(" in chunk
        if "omitted: Omitted::NONE" in chunk:
            omitted_any = False
        else:
            om = re.search(r"omitted:\s*Omitted\s*\{([^}]*)\}", chunk, re.DOTALL)
            if om:
                omitted_any = any(
                    f"{flag}: true" in om.group(1)
                    for flag in ("value_conv", "raw_conv", "condition", "hook", "subdirectory")
                )
            else:
                # unrecognized shape: be conservative, don't attempt to write it
                omitted_any = True

        enum_pairs = None
        ie = re.search(r"print_conv:\s*PrintConv::IntEnum\(&\[(.*?)\]\)", chunk, re.DOTALL)
        se = re.search(r"print_conv:\s*PrintConv::StrEnum\(&\[(.*?)\]\)", chunk, re.DOTALL)
        if ie:
            pairs = re.findall(r"\((-?\d+),\s*\"([^\"]*)\"\)", ie.group(1))
            enum_pairs = [(k, v) for k, v in pairs]
        elif se:
            pairs = re.findall(r'\("([^"]*)",\s*"([^"]*)"\)', se.group(1))
            enum_pairs = [(k, v) for k, v in pairs]

        out.append(RField(
            name=name, format=fmt, count=count, masked=masked,
            omitted_any=omitted_any, enum_pairs=enum_pairs,
        ))
    return default_format, out


def is_scalar_writable(f: RField) -> bool:
    """A subdirectory-pointer field isn't a plain scalar value; everything
    else with omitted_any False is fair game for this harness's raw-write
    strategy."""
    return not f.omitted_any
