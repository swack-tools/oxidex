#!/usr/bin/env python3
"""Generate the declarative half of ExifTool's Composite tag definitions.

Composite tags are ExifTool's derivation layer: ImageSize, Megapixels,
Aperture, ShutterSpeed, FocalLength35efl, DOF, HyperfocalDistance and friends
are not read from the file at all. They are computed from tags that have
already been extracted.

That makes them the cheapest coverage in the project. In a 190-file corpus they
account for the ten most-missed tag names outright -- ~500 missing instances --
and every input they need is already being parsed correctly. No new format
work, no byte offsets, no per-camera quirks. One engine, and every format
gains at once.

What is emitted here is only the part that is pure data:

    name, group, Require list, Desire list

The computation itself is Perl and is NOT translated automatically. Each one is
hand-written in `src/composite/compute.rs` and looked up by name; a composite
with no registered implementation is emitted with `compute: None` and simply
never fires. That keeps the same rule as the binary-table generator: the
dependency graph is transcribed, the semantics are ported deliberately, and
nothing is guessed.
"""

import argparse
import json
import re

# Composites whose inputs ExifTool populates from internal parser state rather
# than from a named tag. We cannot see that state, so we refuse them outright
# instead of emitting a definition that could half-fire on the wrong inputs.
INTERNAL_STATE = {"RawImageCroppedSize"}


def rust_str(s):
    return (s.replace("\\", "\\\\").replace('"', '\\"')
             .replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t"))


def dep_list(d):
    """ExifTool keys Require/Desire by position: {0 => 'ImageWidth', ...}.

    Position matters -- the Perl conversions index $val[0], $val[1] -- so the
    list is emitted in numeric key order, not hash order.
    """
    if d is None:
        return []
    if isinstance(d, str):
        return [(0, d)]
    if isinstance(d, dict):
        out = []
        for k, v in d.items():
            if not isinstance(v, str):
                continue
            try:
                out.append((int(k), v))
            except ValueError:
                continue
        return sorted(out)
    return []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tables_json")
    ap.add_argument("-o", "--out", required=True)
    args = ap.parse_args()

    with open(args.tables_json, encoding="utf-8") as fh:
        doc = json.load(fh)

    rows = []
    seen = set()
    skipped_internal = 0

    for mod_name in sorted(doc["modules"]):
        tables = doc["modules"][mod_name]["tables"]
        tbl = tables.get("Composite")
        if not tbl:
            continue
        for tag_name in sorted(tbl["tags"]):
            tag = tbl["tags"][tag_name]
            name = tag.get("Name")
            if not isinstance(name, str) or not name:
                continue
            req = dep_list(tag.get("Require"))
            des = dep_list(tag.get("Desire"))
            if not req and not des:
                continue
            if any(d in INTERNAL_STATE for _i, d in req):
                skipped_internal += 1
                continue
            # First definition wins, matching ExifTool's module load order for
            # the common tags; later modules override only for their own files,
            # which we do not model.
            key = (mod_name, name)
            if key in seen:
                continue
            seen.add(key)

            groups = tag.get("Groups") or {}
            g2 = groups.get("2", "") if isinstance(groups, dict) else ""
            r = ", ".join(f'"{rust_str(d)}"' for _i, d in req)
            s = ", ".join(f'"{rust_str(d)}"' for _i, d in des)
            rows.append(
                f'    Composite {{ name: "{rust_str(name)}", module: "{rust_str(mod_name)}", '
                f'group2: "{rust_str(g2)}", require: &[{r}], desire: &[{s}] }},'
            )

    body = "\n".join(rows)
    with open(args.out, "w", encoding="utf-8") as fh:
        fh.write(f'''//! ExifTool Composite tag definitions, generated from ExifTool's Perl tables.
//!
//! DO NOT EDIT. Regenerate with `just regen-tables`.
//!
//! Composite tags are derived, not read: `Megapixels` comes from `ImageSize`,
//! which comes from `ImageWidth`/`ImageHeight`. Only the dependency graph is
//! generated here. The arithmetic lives in `super::compute`, is written by
//! hand, and is looked up by name -- a composite with no implementation simply
//! never fires, rather than producing an approximation.

/// One Composite tag: a name and the tags it is derived from.
#[derive(Clone, Copy, Debug)]
pub struct Composite {{
    pub name: &'static str,
    /// ExifTool module that defined it, kept for provenance.
    pub module: &'static str,
    pub group2: &'static str,
    /// All of these must be present, in this order, or the tag does not fire.
    pub require: &'static [&'static str],
    /// Optional inputs; absent ones are passed through as `None`.
    pub desire: &'static [&'static str],
}}

/// Every Composite definition ExifTool declares ({len(rows)} total).
pub static COMPOSITES: &[Composite] = &[
{body}
];
''')

    print(f"wrote {args.out}")
    print(f"  composites emitted   {len(rows)}")
    print(f"  skipped (internal)   {skipped_internal}")


if __name__ == "__main__":
    main()
