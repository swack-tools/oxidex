"""Parse generated binary/IFD statics into per-table field inventories (read-only).

Adapted for the eadb5884 tip (probe tree /Users/allen/git/gen-share-probe).
Instrument: static brace-matched parse of src/exiftool_tables/binary_tables.rs
and ifd_tables.rs. No build, no execution of oxidex.

Differences from work/parse_tables.py (afd3a628):
  * entries are brace-matched (string-literal aware) instead of split on the
    marker text, so a field never absorbs the next field's `subdir:`;
  * every field records its Omitted flags individually, its SubDirectory edge
    target (module, table, unwalked, validate), IFD `flags.unknown`, and
    whether it is a `_variants` alternative -- the engines clear
    `omitted.condition` for a resolved alternative (engine.rs visit_order,
    ifd_engine.rs resolve), so "reported" differs between the two;
  * the legacy keys `omitted` (any flag set) and `subdir` (edge present) are
    kept with their old meaning.
Usage: parse_tables.py <tree root> <out tables.json>
"""
import json, re, sys, pathlib

ROOT = pathlib.Path(sys.argv[1])
OUT = pathlib.Path(sys.argv[2])

TOK = re.compile(r'"(?:[^"\\]|\\.)*"|[{}]')


def match_brace(text, open_pos):
    """Index just past the `}` matching the `{` at open_pos (string-aware)."""
    depth = 0
    for m in TOK.finditer(text, open_pos):
        t = m.group(0)
        if t == '{':
            depth += 1
        elif t == '}':
            depth -= 1
            if depth == 0:
                return m.end()
    raise ValueError(f"unbalanced brace at {open_pos}")


OMIT_KEYS = ("value_conv", "raw_conv", "condition", "hook", "subdirectory", "print_conv")


def parse_omitted(entry):
    m = re.search(r'\n\s*omitted: (Omitted::NONE|Omitted \{(.*?)\})', entry, re.S)
    if not m:
        raise ValueError("no omitted: in entry " + entry[:200])
    if m.group(1) == "Omitted::NONE":
        return {k: False for k in OMIT_KEYS}
    flags = dict(re.findall(r'(\w+): (true|false)', m.group(2)))
    out = {k: flags.get(k) == "true" for k in OMIT_KEYS}
    if set(flags) - set(OMIT_KEYS):
        raise ValueError(f"unexpected Omitted keys {set(flags) - set(OMIT_KEYS)}")
    return out


def top_level_value(entry, key):
    """Text of `key: ...` at the entry's first nesting level (after the opening brace)."""
    m = re.search(r'\n\s*%s: ' % key, entry)
    return m


def parse_entry(entry, kind, table_g0, variant):
    nm = re.search(r'\n\s*name: "((?:[^"\\]|\\.)*)"', entry)
    if not nm:
        raise ValueError("entry without name: " + entry[:200])
    f = {"name": nm.group(1), "variant": variant}
    if kind == "ifd":
        idm = re.search(r'\n\s*id: (0x[0-9a-fA-F]+)', entry)
        f["id"] = int(idm.group(1), 16) if idm else None
        fl = re.search(r'\n\s*flags: (IfdFlags::NONE|IfdFlags \{(.*?)\})', entry, re.S)
        f["unknown"] = bool(fl and fl.group(1) != "IfdFlags::NONE" and re.search(r'unknown: true', fl.group(2)))
    else:
        im = re.search(r'\n\s*index: (-?\d+)', entry)
        f["index"] = int(im.group(1)) if im else None
        f["unknown"] = False  # the binary Field schema has no Unknown flag
    gm = re.search(r'\n\s*groups: (TagGroups::NONE|TagGroups \{(.*?)\})', entry, re.S)
    g0 = None
    if gm and gm.group(1) != "TagGroups::NONE":
        g = re.search(r'g0: Some\("([^"]+)"\)', gm.group(2))
        g0 = g.group(1) if g else None
    f["tag_g0"] = g0
    # ifd_engine.rs walk: `tag.groups.g0.unwrap_or(table.group0)`;
    # engine.rs walk: `group0: table.group0` (the binary engine ignores a field's g0).
    f["g0"] = (g0 or table_g0) if kind == "ifd" else table_g0
    om = parse_omitted(entry)
    f["omitted_flags"] = om
    f["omitted"] = any(om.values())
    sm = re.search(r'\n\s*subdir: Some\((?:Ifd)?SubdirEdge \{', entry)
    if sm:
        end = match_brace(entry, sm.end() - 1)
        edge_txt = entry[sm.start():end]
        em = re.search(r'module: "([^"]+)",\s*table: "([^"]+)"', edge_txt)
        un = re.search(r'unwalked: (None|Some\("((?:[^"\\]|\\.)*)"\))', edge_txt)
        va = re.search(r'validate: (true|false)', edge_txt)
        f["edge"] = {"module": em.group(1), "table": em.group(2),
                     "unwalked": (un.group(2) if un and un.group(1) != "None" else None),
                     "validate": bool(va and va.group(1) == "true")}
    else:
        f["edge"] = None
    f["subdir"] = f["edge"] is not None
    # What the engine walk would report for this entry (ifd_engine.rs:726-783,
    # engine.rs:494-500): never an edge, never Unknown, and no Omitted flag
    # left after a resolved alternative's `condition` is cleared.
    eff = dict(om)
    if variant:
        eff["condition"] = False
    f["reported"] = (not f["subdir"]) and (not f["unknown"]) and not any(eff.values())
    return f


def tables(path, kind):
    src = path.read_text(encoding="utf-8", errors="replace")
    ty = "BinaryTable" if kind == "binary" else "IfdTable"
    marker = "Field {" if kind == "binary" else "IfdTag {"
    head = re.compile(r'pub static (?P<ident>\w+): %s = %s \{' % (ty, ty))
    out = []
    for m in head.finditer(src):
        end = match_brace(src, m.end() - 1)
        body = src[m.end():end]
        hm = re.match(r'\s*module: "([^"]+)",\s*table: "([^"]+)",\s*group0: "([^"]*)",\s*group1: "([^"]*)"', body)
        if not hm:
            raise ValueError(f"unexpected header for {m.group('ident')}")
        module, table, g0, g1 = hm.groups()
        gate = re.search(r"gate_a: GateA \{\s*blocked_by: &\[(.*?)\]\s*\}", body, re.S)
        blocked = gate.group(1).strip() if gate else "?"
        vpos = body.find("\n    variants: &[")
        fields = []
        pos = 0
        n_markers = body.count(marker)
        while True:
            i = body.find(marker, pos)
            if i < 0:
                break
            e = match_brace(body, i + len(marker) - 1)
            entry = body[i:e]
            variant = vpos >= 0 and i > vpos
            fields.append(parse_entry(entry, kind, g0, variant))
            pos = e
        if len(fields) != n_markers:
            raise ValueError(f"{module}::{table}: parsed {len(fields)} of {n_markers} {marker!r} markers")
        out.append({"kind": kind, "ident": m.group("ident"), "module": module, "table": table,
                    "group0": g0, "group1": g1, "gate_a": blocked == "", "blocked_by": blocked,
                    "fields": fields})
    expected = len(re.findall(r'pub static \w+: %s = %s \{' % (ty, ty), src))
    if len(out) != expected:
        raise ValueError(f"{path.name}: parsed {len(out)} of {expected} tables")
    return out


t = tables(ROOT / "src/exiftool_tables/binary_tables.rs", "binary") + tables(ROOT / "src/exiftool_tables/ifd_tables.rs", "ifd")
OUT.write_text(json.dumps(t, indent=0))
nb = sum(1 for x in t if x["kind"] == "binary")
ni = sum(1 for x in t if x["kind"] == "ifd")
print(len(t), "tables", nb, "binary", ni, "ifd", sum(len(x["fields"]) for x in t), "field entries",
      sum(1 for x in t for f in x["fields"] if f["variant"]), "variant alternatives",
      sum(1 for x in t for f in x["fields"] if f["edge"]), "edges")
