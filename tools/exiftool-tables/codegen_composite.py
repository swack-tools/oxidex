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


def priority(tag):
    """ExifTool's effective priority for a Composite tag, as FoundTag computes it.

    ExifTool.pm:9347-9351 (line numbers throughout are ExifTool 13.30, the
    release this generator reads):

        my $priority = $$tagInfo{Priority};
        unless (defined $priority) {
            $priority = $$tbl{PRIORITY};
            $priority = 0 if not defined $priority and $$tagInfo{Avoid};
        }

    `%Image::ExifTool::Composite` declares no table-level `PRIORITY`
    (ExifTool.pm:2256-2262), so an undeclared Composite falls through to the
    "normal default" of 1 at ExifTool.pm:9440. This is what decides whether the
    Composite claims the *bare* tag key from a same-named extracted tag, and so
    whether another Composite's unqualified dependency binds it.

    `Priority` beats `Avoid` when both are present: GPS.pm:371-372 sets
    `Avoid => 1, Priority => 1` on GPSLatitude precisely because "Avoid sets
    default Priority to 0", and the explicit 1 is what takes effect.
    """
    p = tag.get("Priority")
    if p is not None:
        try:
            return int(p)
        except (TypeError, ValueError):
            pass
    elif tag.get("Avoid"):
        return 0
    return 1


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
            # Desire entries are inputs too -- a composite that merely
            # *prefers* internal parser state can still half-fire on the
            # wrong inputs, which is exactly what INTERNAL_STATE refuses.
            if any(d in INTERNAL_STATE for _i, d in (*req, *des)):
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
            r = ", ".join(f'({i}, "{rust_str(d)}")' for i, d in req)
            s = ", ".join(f'({i}, "{rust_str(d)}")' for i, d in des)
            rows.append(
                f'    Composite {{ name: "{rust_str(name)}", module: "{rust_str(mod_name)}", '
                f'group2: "{rust_str(g2)}", priority: {priority(tag)}, '
                f"require: &[{r}], desire: &[{s}] }},"
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
    /// ExifTool's effective tag priority, which decides whether this Composite
    /// claims the *bare* tag key from a same-named extracted tag.
    ///
    /// `FoundTag` keeps the higher-priority tag under the unsuffixed key and
    /// pushes the loser to `Name (1)` (ExifTool.pm:9442-9464); an unqualified
    /// `Require`/`Desire` reads only the bare key (ExifTool.pm:4008). An
    /// ordinarily-extracted tag counts as 1, so a Composite left at the default
    /// 1 wins the name and a `Priority => 0` Composite -- `Canon:ISO`'s "let
    /// EXIF:ISO take priority" (Canon.pm:9781-9782) -- yields to it.
    pub priority: i8,
    /// All of these indexed inputs must be present or the tag does not fire.
    pub require: &'static [(usize, &'static str)],
    /// Indexed optional inputs; absent positions are passed through as `None`.
    pub desire: &'static [(usize, &'static str)],
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
