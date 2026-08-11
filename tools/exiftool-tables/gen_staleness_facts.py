#!/usr/bin/env python3
"""Generate staleness-fact fixtures from a `dump_tables.pl` JSON dump.

Step 16 of the tag-machinery overhaul (see OVERHAUL_OXIDEX_PLAN.md, Stage 3):
several places in this repo hand-embed a fact copied out of ExifTool's Perl
source (an enum map, a dispatch table, a lens database) and nothing re-checks
that copy against ExifTool on a version bump. The #636 regen found 401 such
stale discrepancies and "nothing in the repo could report it."

This script is the mechanical half of the fix: it reads the JSON
`dump_tables.pl` produces (ExifTool's *actual* in-memory tables, not a regex
over the .pm text -- see that script's own header) and writes small, focused
JSON fixtures under `tools/exiftool-tables/fixtures/`. Each fixture is
committed to the repo and consumed at COMPILE TIME by a `#[cfg(test)]` module
next to the hand-embedded Rust it checks, via `include_str!` -- so the test
stays hermetic (no /tmp path, no network, no live ExifTool) while still
reporting the moment a bump changes the fact the fixture captures and nobody
updates the Rust to match.

Usage:
    python3 gen_staleness_facts.py <tables.json> <fixtures-out-dir>

<tables.json> is produced by:
    perl dump_tables.pl <exiftool-lib-dir> [module...]

Regenerate when the ExifTool pin bumps (`just bump-exiftool` will eventually
wire this in automatically -- see Step 17); until then, re-run by hand and
`git diff` the fixtures the same way `regen-all.sh`'s tier-2 outputs are
reviewed. A fixture that does not move on a no-op rerun is the byte-identical
guarantee the rest of this repo's generators already give.

This script intentionally covers ONLY the fact sites that reduce to a clean
data comparison (ExifTool's PrintConv is a pure enum map/list, not a Perl
expression). Sites that are formulas, regex conditions or code -- the
CryptShutterCount XOR, Panasonic's Transform pairs, Sony's ExtraInfo3 model
predicate, the QuickTime CR3/CNCV gate -- are NOT data-diffable this way; see
`check_perl_anchors.py` for how those are covered instead.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any


def load(path: str) -> dict[str, Any]:
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def write(out_dir: Path, name: str, payload: Any) -> None:
    dest = out_dir / name
    with open(dest, "w", encoding="utf-8") as f:
        json.dump(payload, f, indent=1, sort_keys=True, ensure_ascii=False)
        f.write("\n")
    print(f"  wrote {dest} ({len(json.dumps(payload))} bytes)")


def strip_prefix(full: str) -> str:
    """`Image::ExifTool::Canon::Main` -> `Canon::Main`."""
    prefix = "Image::ExifTool::"
    return full[len(prefix) :] if full.startswith(prefix) else full


# ---------------------------------------------------------------------------
# MakerNotes::Main -- the dispatch table makernote_dispatcher.rs implements
# by hand as a `match` over the camera Make string.
# ---------------------------------------------------------------------------


def gen_makernote_routes(modules: dict, out_dir: Path) -> None:
    mn = modules.get("MakerNotes")
    if not mn:
        raise SystemExit("MakerNotes module missing from dump -- was it included in the run?")
    arrays = mn.get("arrays", {})
    main = arrays.get("Main")
    if not main:
        raise SystemExit(
            "MakerNotes::Main missing from dump['arrays'] -- dump_tables.pl's "
            "ARRAY-capture extension did not run, or ExifTool restructured "
            "the table from an array to a hash."
        )

    rows = []
    for row in main["rows"]:
        name = row.get("Name")
        cond = row.get("Condition", "")
        sd = row.get("SubDirectory")
        target = None
        if isinstance(sd, dict) and sd.get("TagTable"):
            target = strip_prefix(sd["TagTable"])
        rows.append({"name": name, "condition": cond, "target": target})

    write(
        out_dir,
        "makernote_routes.json",
        {"row_count": main["row_count"], "rows": rows},
    )


# ---------------------------------------------------------------------------
# Canon CanonModelID (MakerNote tag 0x0010) and LensType (%canonLensTypes,
# reached through Canon::CameraSettings key 22).
# ---------------------------------------------------------------------------


def gen_canon_facts(modules: dict, out_dir: Path) -> None:
    canon = modules.get("Canon")
    if not canon:
        raise SystemExit("Canon module missing from dump")
    tables = canon["tables"]

    model_tag = tables["Main"]["tags"]["16"]  # 0x0010
    if model_tag.get("Name") != "CanonModelID":
        raise SystemExit(
            f"Canon::Main tag 16 is {model_tag.get('Name')!r}, not CanonModelID -- "
            "ExifTool renumbered this tag; find_table/re-anchor before regenerating."
        )
    model_map = model_tag["PrintConv"]["map"]
    entries = sorted(((int(k), v) for k, v in model_map.items()), key=lambda kv: kv[0])
    write(out_dir, "canon_model_ids.json", {"entries": entries})

    lens_tag = tables["CameraSettings"]["tags"]["22"]
    if lens_tag.get("Name") != "LensType":
        raise SystemExit(
            f"Canon::CameraSettings tag 22 is {lens_tag.get('Name')!r}, not LensType"
        )
    lens_map = lens_tag["PrintConv"]["map"]
    # Only the integer-keyed entries are transcribed in lens_data.rs; the
    # fractional keys (`2.1`, `33.14`, ...) disambiguate Composite:LensID,
    # which oxidex does not implement (see lens_data.rs's own doc comment).
    int_entries = sorted(
        ((int(k), v) for k, v in lens_map.items() if "." not in k),
        key=lambda kv: kv[0],
    )
    frac_count = sum(1 for k in lens_map if "." in k)
    write(
        out_dir,
        "canon_lens_types.json",
        {"entries": int_entries, "fractional_key_count_not_covered": frac_count},
    )


# ---------------------------------------------------------------------------
# Pentax FlashMode (0x000c), ISO (0x0014), AFPointSelected (0x000e) -- Stage 1
# Step 1's registered facts.
# ---------------------------------------------------------------------------


def gen_pentax_facts(modules: dict, out_dir: Path) -> None:
    pentax = modules.get("Pentax")
    if not pentax:
        raise SystemExit("Pentax module missing from dump")
    main = pentax["tables"]["Main"]["tags"]

    flash = main["12"]  # 0x000c
    if flash.get("Name") != "FlashMode":
        raise SystemExit(f"Pentax::Main tag 12 is {flash.get('Name')!r}, not FlashMode")
    items = flash["PrintConv"]["items"]
    if len(items) != 2 or not all(isinstance(x, dict) for x in items):
        raise SystemExit("Pentax FlashMode PrintConv is no longer a 2-element list of hashes")
    write(
        out_dir,
        "pentax_flash_mode.json",
        {
            "internal": {int(k): v for k, v in items[0].items()},
            "external": {int(k): v for k, v in items[1].items()},
        },
    )

    iso = main["20"]  # 0x0014
    if iso.get("Name") != "ISO":
        raise SystemExit(f"Pentax::Main tag 20 is {iso.get('Name')!r}, not ISO")
    iso_map = iso["PrintConv"]["map"]
    write(
        out_dir,
        "pentax_iso.json",
        {int(k): v for k, v in iso_map.items()},
    )

    afp = main["14"]  # 0x000e
    variants = afp.get("_variants")
    if not variants or len(variants) != 3:
        raise SystemExit(
            "Pentax::Main tag 14 (AFPointSelected) is no longer a 3-alternative "
            "conditional array"
        )
    labels = ["k1_645z", "k3_kp", "default"]
    out: dict[str, dict[int, str]] = {}
    for label, variant in zip(labels, variants):
        if variant.get("Name") != "AFPointSelected":
            raise SystemExit(f"Pentax AFPointSelected variant {label!r} lost its Name")
        pos0 = variant["PrintConv"]["items"][0]
        out[label] = {int(k): v for k, v in pos0.items()}
    write(out_dir, "pentax_af_point_selected.json", out)


# ---------------------------------------------------------------------------
# FujiFilm ImageStabilization (0x1422) -- Stage 1 Step 2's registered fact.
# ---------------------------------------------------------------------------


def gen_fujifilm_facts(modules: dict, out_dir: Path) -> None:
    fuji = modules.get("FujiFilm")
    if not fuji:
        raise SystemExit("FujiFilm module missing from dump")
    tag = fuji["tables"]["Main"]["tags"]["5154"]  # 0x1422
    if tag.get("Name") != "ImageStabilization":
        raise SystemExit(f"FujiFilm::Main tag 5154 is {tag.get('Name')!r}, not ImageStabilization")
    items = tag["PrintConv"]["items"]
    if len(items) < 2:
        raise SystemExit("FujiFilm ImageStabilization PrintConv is no longer a >=2-element list")
    write(
        out_dir,
        "fujifilm_image_stabilization.json",
        {
            "element0": {int(k): v for k, v in items[0].items()},
            "element1": {int(k): v for k, v in items[1].items()},
        },
    )


def main() -> None:
    if len(sys.argv) != 3:
        print(__doc__, file=sys.stderr)
        sys.exit(1)
    dump_path, out_dir_s = sys.argv[1], sys.argv[2]
    out_dir = Path(out_dir_s)
    out_dir.mkdir(parents=True, exist_ok=True)

    d = load(dump_path)
    modules = d["modules"]
    write(out_dir, "exiftool_version.json", {"exiftool_version": d["exiftool_version"]})

    print("Generating staleness fixtures...")
    gen_makernote_routes(modules, out_dir)
    gen_canon_facts(modules, out_dir)
    gen_pentax_facts(modules, out_dir)
    gen_fujifilm_facts(modules, out_dir)
    print("done.")


if __name__ == "__main__":
    main()
