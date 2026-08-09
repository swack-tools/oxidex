#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Attribute every oxidex/ExifTool coverage gap to its ExifTool .pm module,
%table, and owning squad (spec S1 of
docs/plans/specs/2026-07-24-fleet-knowledge-and-scaling-design.md).

The tag-comparison tool reports gaps as (format, family, name) -- e.g.
``JPEG / MakerNotes / CanonFirmwareVersion`` -- but the fleet is organised
by ExifTool *module* (Canon.pm, Nikon.pm, ...), because that is where
parsing knowledge clusters and where the shared Rust emitter files map
1:1 onto exactly one owning squad. This script bridges the two views:

1. Index the ExifTool Perl lib: scan every ``*.pm``/``*.pl`` for
   ``%Image::ExifTool::<Module>::<Table> = ( ... )`` declarations and
   collect the quoted tag names defined inside each table -- the
   ``Name => 'TagName'`` style, the top-level ``0x1d => 'TagName'``
   shorthand, and bare ``TagName => {`` entries (XMP-style lowercase
   keys are indexed both raw and ucfirst'd, since ExifTool ucfirsts
   them for display).
2. Attribute each gap: families that *are* a module (``CanonVRD``,
   ``Photoshop``, ``ICC_Profile``, ...) map directly; a small override
   table handles the rest (``EXIF``/``IFD0``/``GPS`` -> Exif,
   ``JUMBF`` -> Jpeg2000, ...); the big ``MakerNotes`` bucket falls
   through to the name index, disambiguated by a per-format module
   priority list (NEF -> Nikon first, RW2 -> PanasonicRaw first, ...)
   and, failing that, by the sample directory the gap was observed in
   (``combined-samples/Leica/x.jpg`` -> Panasonic.pm). Tags found
   nowhere become module ``"unknown"`` -- acceptable advisory noise per
   the spec; attribution routes claims/memory/warnings, it is never a
   gate.
3. Roll up per squad via config.toml's [squads.*] tables (module listed in
   no squad -> squad "tail") and write ``gap-attribution.json`` atomically
   (tempfile + os.replace) for the dispatcher's slot formula
   (``slots_i = max(1, round(total x open_gaps_i / total_gaps))``).

Output shape::

    {
      "generated_at": "...",
      "tags": {
        "<FMT>:<family>:<name>": {
          "module": "Canon", "table": "CameraSettings",
          "squad": "canon", "formats": ["JPEG"], "sample_dirs": ["Canon"]
        }, ...
      },
      "squads": {
        "canon": {"open_gaps": 917, "formats": [...], "modules": [...]},
        ...
      }
    }

Usage:
    uv run scripts/attribute_gaps.py --comparison comparison.json
    uv run scripts/attribute_gaps.py --comparison /tmp/tagcmp-JPEG.json \\
        --formats JPEG --print-summary --out /tmp/gap-attribution.json

Known limitation (accepted in the spec): the tag-name index has a
few-percent collision/noise rate -- identical display names exist in
multiple manufacturer modules, and the crude Perl "parser" here is a
line scanner, not perl(1). That is fine for advisory routing; the one
consumer that needs exact table membership (T3 TABLE-PORT jobs) parses
the real ``%table`` source instead of trusting this index.
"""
import argparse
import json
import os
import re
import sys
import tempfile
import tomllib
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

SCRIPTS_DIR = Path(__file__).resolve().parent

# Same fixed, worktree-independent home as find_tag_gaps.py /
# model_fix_loop.py: attribution output must land in ONE place no matter
# which worktree the dispatcher happens to run from.
OXIDEX_HOME = Path(os.environ.get("OXIDEX_HOME", str(Path.home() / ".oxidex")))
DEFAULT_OUT = OXIDEX_HOME / "logs" / "gap-attribution.json"
# Squad ownership lives in config.toml's [squads.*] tables (moved there so
# there is exactly one fleet config file); SCRIPTS_DIR.parent, since
# config.toml sits at the repo root, not inside scripts/.
DEFAULT_CONFIG_PATH = SCRIPTS_DIR.parent / "config.toml"

# Squad every unmapped module (and module "unknown") rolls up to; must
# exist in config.toml's [squads.*] tables.
FALLBACK_SQUAD = "tail"

# Module name used when a gap's tag name appears nowhere in the Perl
# lib index (dynamic/generated names, trailer blobs, ...). Spec S1
# accepts this as advisory noise.
UNKNOWN_MODULE = "unknown"

# ---------------------------------------------------------------------------
# Perl-lib tag-name index
# ---------------------------------------------------------------------------

# %Image::ExifTool::<Module>::<Table> = (
# The module comes from the DECLARATION, not the filename: XMP2.pl
# declares %Image::ExifTool::XMP::* tables, and they must index as
# module "XMP".
TABLE_DECL_RE = re.compile(r"^%Image::ExifTool::(\w+)::(\w+)\s*=\s*\(")

# Name => 'TagName'  (or double-quoted). \bName does NOT match the tail
# of FlatName/TagName -- there is no word boundary inside a word -- so
# flattened-struct aliases don't pollute the index.
NAME_RE = re.compile(r"\bName\s*=>\s*['\"]([\w\-]+)['\"]")

# Top-level shorthand entry:  0x1d => 'TagName',   or   4 => 'TagName',
# Only honoured at table nesting depth 1 -- the identical shape inside a
# PrintConv hash (1 => 'Sunny') sits at depth >= 2 and must NOT index.
SIMPLE_ID_RE = re.compile(
    r"^\s*(?:0x[0-9a-fA-F]+|\d+(?:\.\d+)?)\s*=>\s*['\"]([\w\-]+)['\"]"
)

# Bare table entry:  TagName => {   (XMP property style, often
# lowercase-first). ALL-CAPS keys are ExifTool table metadata (GROUPS,
# NOTES, PROCESS_PROC, ...), never tags.
BARE_KEY_RE = re.compile(r"^\s*([A-Za-z_]\w*)\s*=>\s*\{")


def _nesting_delta(line):
    """Net change in paren/brace nesting contributed by one source line,
    with quoted strings and trailing comments crudely removed first so a
    ``PrintConv => 'sprintf("%d (#%x)")'`` doesn't corrupt the count.
    Line-based and heuristic by design -- ExifTool's source is
    consistently formatted, and spec S1 explicitly tolerates
    few-percent noise from this index.
    """
    stripped = re.sub(r"'(?:[^'\\]|\\.)*'", "''", line)
    stripped = re.sub(r'"(?:[^"\\]|\\.)*"', '""', stripped)
    hash_idx = stripped.find("#")
    if hash_idx != -1:
        stripped = stripped[:hash_idx]
    return (
        stripped.count("(") + stripped.count("{")
        - stripped.count(")") - stripped.count("}")
    )


def _add(index, name, module, table):
    """Append (module, table) for a tag name, deduplicated, preserving
    file-scan order (deterministic: files are visited sorted, lines
    sequentially) so "first candidate" fallbacks are stable run-to-run.
    """
    entries = index.setdefault(name, [])
    if (module, table) not in entries:
        entries.append((module, table))


def index_perl_file(path, index, modules):
    """Scan one .pm/.pl file for %table declarations and the tag names
    defined inside them. Mutates `index` (name -> [(module, table)])
    and `modules` (set of known module names) in place.
    """
    modules.add(path.stem)
    in_table = None  # (module, table) while inside a declaration
    depth = 0
    for raw in path.read_text(errors="ignore").splitlines():
        if in_table is None:
            decl = TABLE_DECL_RE.match(raw)
            if decl:
                in_table = (decl.group(1), decl.group(2))
                modules.add(decl.group(1))
                depth = _nesting_delta(raw)
                if depth <= 0:  # one-line `%X = ( ... );` -- nothing to scan
                    in_table = None
            continue
        module, table = in_table
        name_match = NAME_RE.search(raw)
        if name_match:
            # Legit at any depth: conditional tag variants nest the
            # Name inside arrays-of-hashes, and nothing else in
            # ExifTool source uses a bare `Name =>` key.
            _add(index, name_match.group(1), module, table)
        elif depth == 1:
            simple = SIMPLE_ID_RE.match(raw)
            bare = BARE_KEY_RE.match(raw)
            if simple:
                _add(index, simple.group(1), module, table)
            elif bare and not bare.group(1).isupper():
                key = bare.group(1)
                _add(index, key, module, table)
                if key[0].islower():
                    # XMP property keys are lowercase-first in source
                    # but ucfirst'd for display (aboutCvTerm ->
                    # AboutCvTerm); index the display form too.
                    _add(index, key[0].upper() + key[1:], module, table)
        depth += _nesting_delta(raw)
        if depth <= 0:
            in_table = None


def build_tag_index(perl_lib):
    """Index quoted tag names across the whole Perl lib.

    Returns (index, modules): index maps tag name -> ordered unique
    [(module, table), ...]; modules is the set of every module name
    seen (file stems plus declared package names), used for the
    data-driven family -> module match.
    """
    index = {}
    modules = set()
    perl_lib = Path(perl_lib)
    for path in sorted(list(perl_lib.glob("*.pm")) + list(perl_lib.glob("*.pl"))):
        index_perl_file(path, index, modules)
    return index, modules


# ---------------------------------------------------------------------------
# Attribution
# ---------------------------------------------------------------------------

# Families that must NOT take the data-driven family==module route, or
# that map to a module whose name differs from the family. Value None
# means "force the name-index path" (MakerNotes.pm exists but is the
# dispatch shim, never the defining module for a maker tag).
FAMILY_MODULE_OVERRIDES = {
    "MakerNotes": None,
    # Exif.pm IFD wiring, per spec S1 ("IFD0/ExifIFD/GPS -> Exif").
    "EXIF": "Exif",
    "IFD0": "Exif",
    "IFD1": "Exif",
    "IFD2": "Exif",
    "SubIFD": "Exif",
    "ExifIFD": "Exif",
    "GPS": "Exif",
    "InteropIFD": "Exif",
    "GlobParamIFD": "Exif",
    # JUMBF/C2PA boxes are parsed by Jpeg2000.pm.
    "JUMBF": "Jpeg2000",
    # Kodak Meta APP3: no Meta.pm exists; config.toml's squad manifest lists
    # module "Meta" (standards-appn) and attribution routes the family name
    # through directly, per that manifest's comment.
    "Meta": "Meta",
    # JFIF/Trailer segments are handled by JPEG.pm itself.
    "JFIF": "JPEG",
    "Trailer": "JPEG",
}

# Per-format module priority for disambiguating name-index candidates
# (spec S1: "the per-bucket priority lists validated in the gap
# census"). First candidate module present in the list wins. Formats
# not listed here (JPEG above all) fall through to the sample-dir hint.
FORMAT_MODULE_PRIORITY = {
    "NEF": ["Nikon", "NikonCustom", "NikonSettings", "NikonCapture", "Exif"],
    "CR2": ["Canon", "CanonCustom", "CanonRaw", "Exif"],
    "CR3": ["Canon", "CanonCustom", "CanonRaw", "QuickTime", "Exif"],
    "DNG": ["DNG", "Exif", "XMP"],
    "RW2": ["PanasonicRaw", "Panasonic", "Exif"],
    "MRW": ["Minolta", "MinoltaRaw", "Exif"],
    "X3F": ["Sigma", "SigmaRaw", "Exif"],
    "ARW": ["Sony", "Minolta", "Exif"],
    "PEF": ["Pentax", "Exif"],
    "ORF": ["Olympus", "Exif"],
    "RAF": ["FujiFilm", "Exif"],
    "PSD": ["Photoshop", "IPTC", "XMP", "Exif"],
    "PDF": ["PDF", "XMP", "Exif"],
}

# Sample corpus directory names that are not literally a module name.
# Leica bodies write Panasonic-format maker notes (Panasonic.pm).
SAMPLE_DIR_MODULE_ALIASES = {
    "leica": "Panasonic",
}


def extract_sample_dir(source_file, samples_marker="combined-samples"):
    """Directory component the sample file lives under, relative to the
    comparison corpus root -- e.g.
    ``/x/combined-samples/Nikon/a.jpg`` -> ``"Nikon"``;
    root-level samples (``/x/combined-samples/CanonRaw.cr3``) and paths
    without the corpus marker return None. Feeds both the sample_dirs
    output field (so squads can run scoped rechecks, spec S1) and the
    JPEG MakerNotes disambiguation hint.
    """
    if not source_file:
        return None
    parts = PurePosixPath(source_file).parts
    if samples_marker not in parts:
        return None
    rest = parts[parts.index(samples_marker) + 1:]
    return rest[0] if len(rest) > 1 else None


def _first_table_for(candidates, module):
    """First %table the index saw `module` define this tag name in
    ('' when the module never defines it -- e.g. family-mapped gaps
    whose name the line scanner missed).
    """
    for cand_module, table in candidates:
        if cand_module == module:
            return table
    return ""


def attribute_gap(fmt, family, name, index, module_lookup, sample_dirs=()):
    """Map one gap (format, family, tag name) to (module, table).

    Resolution order:
      1. Explicit family override (EXIF -> Exif, JUMBF -> Jpeg2000, ...).
      2. Data-driven family==module match, case-insensitive (family
         CanonVRD -> CanonVRD.pm, Photoshop -> Photoshop.pm, ...).
      3. Name-index lookup, disambiguated by (a) the format's module
         priority list, (b) the sample-directory hint, (c) the
         alphabetically-first candidate. Not found -> "unknown".

    `module_lookup` maps lowercased module name -> canonical module
    name for every module in the Perl lib.
    """
    candidates = index.get(name) or []

    if family in FAMILY_MODULE_OVERRIDES:
        module = FAMILY_MODULE_OVERRIDES[family]
        if module is not None:
            return module, _first_table_for(candidates, module)
    else:
        module = module_lookup.get(family.lower())
        if module is not None:
            return module, _first_table_for(candidates, module)

    if not candidates:
        return UNKNOWN_MODULE, ""

    candidate_modules = []
    for cand_module, _ in candidates:
        if cand_module not in candidate_modules:
            candidate_modules.append(cand_module)

    for module in FORMAT_MODULE_PRIORITY.get(fmt, ()):  # (a) format priority
        if module in candidate_modules:
            return module, _first_table_for(candidates, module)

    for sample_dir in sample_dirs:  # (b) which corpus dir produced the gap
        hint = SAMPLE_DIR_MODULE_ALIASES.get(sample_dir.lower(), sample_dir)
        for module in candidate_modules:
            if module.lower() == hint.lower():
                return module, _first_table_for(candidates, module)

    return min(candidates)  # (c) deterministic last resort


# ---------------------------------------------------------------------------
# Squads manifest
# ---------------------------------------------------------------------------

def load_squads(config_path):
    """Read config.toml's [squads.*] tables -> (module_to_squad, squad_names).

    First squad to list a module owns it (the manifest should never
    list one module twice; setdefault makes duplicates deterministic
    rather than order-of-dict surprising).
    """
    with open(config_path, "rb") as f:
        data = tomllib.load(f)
    squads = data.get("squads") or {}
    module_to_squad = {}
    for squad_name, cfg in squads.items():
        for module in cfg.get("modules") or []:
            module_to_squad.setdefault(module, squad_name)
    return module_to_squad, list(squads.keys())


# ---------------------------------------------------------------------------
# Report -> attribution
# ---------------------------------------------------------------------------

def iter_gaps(report, formats=None):
    """Yield (fmt, family, name, source_file) for every gap in a
    ComparisonReport -- both missing_in_oxidex entries ({name, family,
    source_file, ...}) and value_differences entries ({tag_key:
    "FAMILY:Name", source_file, ...}). Formats iterate sorted so output
    is deterministic; `formats` optionally restricts to a subset.
    """
    for fmt in sorted(report.get("by_format") or {}):
        if formats and fmt not in formats:
            continue
        comp = report["by_format"][fmt] or {}
        for entry in comp.get("missing_in_oxidex") or []:
            yield (fmt, entry.get("family") or "", entry.get("name") or "",
                   entry.get("source_file"))
        for entry in comp.get("value_differences") or []:
            tag_key = entry.get("tag_key") or ""
            family, sep, name = tag_key.partition(":")
            if not sep:
                family, name = "", tag_key
            yield fmt, family, name, entry.get("source_file")


def build_attribution(report, index, modules, module_to_squad, squad_names,
                      formats=None, now_iso=None):
    """Assemble the full gap-attribution document (see module docstring
    for the shape). Pure function of its inputs -- no filesystem, no
    clock (inject now_iso) -- so tests run it hermetically.
    """
    module_lookup = {m.lower(): m for m in sorted(modules)}

    # Group occurrences per tag key first so attribution sees EVERY
    # sample dir the gap was observed in (a tag first seen in a
    # root-level sample may only become disambiguable via a later
    # Canon/ observation).
    occurrences = {}  # key -> {"fmt", "family", "name", "dirs": [..]}
    for fmt, family, name, source_file in iter_gaps(report, formats):
        key = f"{fmt}:{family}:{name}"
        occ = occurrences.setdefault(
            key, {"fmt": fmt, "family": family, "name": name, "dirs": []})
        sample_dir = extract_sample_dir(source_file)
        if sample_dir and sample_dir not in occ["dirs"]:
            occ["dirs"].append(sample_dir)

    tags = {}
    squad_rollup = {name: {"open_gaps": 0, "formats": set(), "modules": set()}
                    for name in squad_names}
    for key, occ in occurrences.items():
        module, table = attribute_gap(
            occ["fmt"], occ["family"], occ["name"], index, module_lookup,
            occ["dirs"])
        squad = module_to_squad.get(module, FALLBACK_SQUAD)
        tags[key] = {
            "module": module,
            "table": table,
            "squad": squad,
            "formats": [occ["fmt"]],
            "sample_dirs": sorted(occ["dirs"]),
        }
        rollup = squad_rollup.setdefault(
            squad, {"open_gaps": 0, "formats": set(), "modules": set()})
        rollup["open_gaps"] += 1
        rollup["formats"].add(occ["fmt"])
        rollup["modules"].add(module)

    squads = {
        name: {
            "open_gaps": agg["open_gaps"],
            "formats": sorted(agg["formats"]),
            "modules": sorted(agg["modules"]),
        }
        for name, agg in squad_rollup.items()
    }
    if now_iso is None:
        now_iso = datetime.now(timezone.utc).isoformat(timespec="seconds")
    return {"generated_at": now_iso, "tags": tags, "squads": squads}


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------

def write_atomic(out_path, data):
    """Write JSON via a same-directory tempfile + os.replace, so
    concurrent readers (the dispatcher regenerates this once per round
    while workers read it for claims) only ever see a complete
    document -- same pattern spec S4 mandates for tag-state.
    """
    out_path = Path(out_path)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = tempfile.NamedTemporaryFile(
        "w", dir=out_path.parent, prefix=out_path.name + ".",
        suffix=".tmp", delete=False)
    try:
        with tmp:
            json.dump(data, tmp, indent=2)
            tmp.write("\n")
        os.replace(tmp.name, out_path)
    except BaseException:
        try:
            os.unlink(tmp.name)
        except OSError:
            pass
        raise


def format_summary(attribution):
    """Per-squad table for --print-summary -- the operator sanity-check
    against the S2 snapshot columns (which are snapshots, not config;
    live counts drift as gaps close).
    """
    squads = attribution["squads"]
    rows = sorted(squads.items(), key=lambda kv: (-kv[1]["open_gaps"], kv[0]))
    lines = [f"{'squad':<18} {'open_gaps':>9}  formats / modules-with-gaps"]
    for name, agg in rows:
        lines.append(f"{name:<18} {agg['open_gaps']:>9}  "
                     f"{','.join(agg['formats']) or '-'}")
        if agg["modules"]:
            lines.append(f"{'':<18} {'':>9}  ({','.join(agg['modules'])})")
    total = sum(agg["open_gaps"] for agg in squads.values())
    unknown = sum(1 for t in attribution["tags"].values()
                  if t["module"] == UNKNOWN_MODULE)
    lines.append(f"{'TOTAL':<18} {total:>9}  "
                 f"({unknown} unattributable -> module 'unknown', advisory)")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def default_perl_lib():
    """Resolve the ExifTool Perl lib exactly the way model_fix_loop
    does (reads the @INC path Homebrew patches into the exiftool
    script), imported lazily so hermetic tests -- which always pass
    --perl-lib -- never touch model_fix_loop or the real exiftool.
    """
    sys.path.insert(0, str(SCRIPTS_DIR))
    from model_fix_loop import resolve_exiftool_perl_lib_dir
    return resolve_exiftool_perl_lib_dir()


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Attribute comparison-report gaps to ExifTool "
                    "module/%table/squad (spec S1).")
    parser.add_argument("--comparison", required=True,
                        help="ComparisonReport JSON (comparison.json or a "
                             "/tmp/tagcmp-<FMT>.json)")
    parser.add_argument("--perl-lib", default=None,
                        help="Image/ExifTool Perl module dir (default: "
                             "resolved from the exiftool on PATH, as "
                             "model_fix_loop does)")
    parser.add_argument("--config", default=str(DEFAULT_CONFIG_PATH),
                        help="config.toml, for its [squads.*] tables (see config.example.toml)")
    parser.add_argument("--out", default=str(DEFAULT_OUT))
    parser.add_argument("--formats", default=None,
                        help="comma-separated format filter, e.g. JPEG,NEF")
    parser.add_argument("--print-summary", action="store_true",
                        help="print the per-squad rollup table")
    args = parser.parse_args(argv)

    perl_lib = Path(args.perl_lib) if args.perl_lib else default_perl_lib()
    if perl_lib is None or not Path(perl_lib).is_dir():
        print(f"error: ExifTool Perl lib not found ({perl_lib}); "
              f"pass --perl-lib", file=sys.stderr)
        return 1

    formats = None
    if args.formats:
        formats = {f.strip() for f in args.formats.split(",") if f.strip()}

    with open(args.comparison) as f:
        report = json.load(f)
    index, modules = build_tag_index(perl_lib)
    module_to_squad, squad_names = load_squads(args.config)
    attribution = build_attribution(
        report, index, modules, module_to_squad, squad_names, formats=formats)
    write_atomic(args.out, attribution)

    if args.print_summary:
        print(format_summary(attribution))
    total = len(attribution["tags"])
    unknown = sum(1 for t in attribution["tags"].values()
                  if t["module"] == UNKNOWN_MODULE)
    print(f"{total} gaps attributed ({unknown} unknown) across "
          f"{len([s for s in attribution['squads'].values() if s['open_gaps']])} "
          f"squads -> {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
