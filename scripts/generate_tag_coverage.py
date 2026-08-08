#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = [
#     "pyyaml>=6.0",
# ]
# ///
"""
Generate the Tag Coverage report.

This script reports two things, and it is careful never to conflate them:

  1. Tag *knowledge* -- how many tags OxiDex has definitions for. Counted
     directly from the `oxidex-tags-*/src/*.yaml` databases. This is exact.

  2. Tag *extraction* -- what OxiDex actually pulls out of real files,
     measured against the pinned ExifTool. This is NOT derivable from source
     code, and is read from `tools/exiftool-tables/conformance.py --json-out`.

The old version of this script estimated (2) by counting `.insert(`/`.push(`
call sites in `src/parsers/**` and mapping the count through hand-tuned
thresholds. That number was wrong in every direction at once: overlapping
regexes counted a single `metadata.insert("X".to_string(), v)` three times,
buffer-building calls like `data.push(..)` were scored as tags, parsers
written as a single loose `.rs` file were invisible, and every parser above
100 hits collapsed to the same "90%". It also divided the YAML tag count by a
hardcoded 28,853 and published the result as "ExifTool Parity", which is the
exact inference AGENTS.md forbids -- a rising definition count is not evidence
of rising extraction coverage.

None of that is patchable, because call sites were never the quantity of
interest. It is replaced by a measurement or it is not reported.

Usage:
    # measured (what CI runs)
    uv run tools/exiftool-tables/conformance.py tests/fixtures --recursive \
        --oxidex ./target/debug/oxidex --json-out /tmp/conformance.json
    uv run scripts/generate_tag_coverage.py --conformance /tmp/conformance.json

    # definitions only, to stdout -- cannot overwrite the committed doc
    uv run scripts/generate_tag_coverage.py --skip-conformance

Or via justfile:
    just docs-coverage
"""

import argparse
import json
import re
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path

import yaml


def get_project_root() -> Path:
    """Find project root by looking for Cargo.toml"""
    current = Path(__file__).resolve().parent
    while current != current.parent:
        if (current / "Cargo.toml").exists():
            return current
        current = current.parent
    raise RuntimeError("Could not find project root")


def read_exiftool_pin(project_root: Path) -> str:
    """The release the transcriptions came from, and the only version any part
    of this repo may grade against. Never hardcode it -- a second copy is how
    the matrix once graded 13.55 output against 13.59 tables."""
    pin = project_root / ".exiftool-version"
    if not pin.exists():
        return "unknown"
    return pin.read_text().strip()


def parse_yaml_tags(project_root: Path) -> dict:
    """Parse all YAML tag files and count tags by domain/category"""
    domains = {}
    yaml_files = sorted(project_root.glob("oxidex-tags-*/src/*.yaml"))
    if not yaml_files:
        sys.exit(
            "no oxidex-tags-*/src/*.yaml files found -- the tag databases moved, "
            "and this script would otherwise report a confident zero."
        )

    for yaml_file in yaml_files:
        # Extract domain from path (e.g., oxidex-tags-core -> core)
        domain = yaml_file.parent.parent.name.replace("oxidex-tags-", "")

        content = yaml_file.read_text()

        try:
            data = yaml.safe_load(content)
        except yaml.YAMLError as e:
            print(f"Warning: Failed to parse {yaml_file}: {e}")
            continue

        if not data or "tables" not in data:
            continue

        domain_data = domains.setdefault(domain, {
            "categories": defaultdict(int),
            "total_tags": 0,
            "total_tables": 0
        })

        for table in data["tables"]:
            table_name = table.get("name", "Unknown")
            tags = table.get("tags", [])
            tag_count = len(tags)

            domain_data["categories"][table_name] = tag_count
            domain_data["total_tags"] += tag_count
            domain_data["total_tables"] += 1

    return domains


def check_makernote_status(project_root: Path) -> dict:
    """Check MakerNote dispatcher status.

    This stays source-derived because it is a structural fact, not a coverage
    estimate: either `dispatch_makernote` is called from the TIFF file parser
    or it is not, and either a manufacturer has a dispatcher arm or it does
    not. Neither claims anything about how many of that manufacturer's tags
    are extracted -- the conformance table below is the only thing that does.
    """
    dispatcher_path = project_root / "src" / "parsers" / "tiff" / "makernote_dispatcher.rs"
    file_parser_path = project_root / "src" / "parsers" / "tiff" / "file_parser.rs"

    result = {
        "dispatcher_exists": dispatcher_path.exists(),
        "wired_up": False,
        "manufacturers": []
    }

    if not dispatcher_path.exists():
        return result

    # Check if dispatcher is wired up in file_parser.rs
    if file_parser_path.exists():
        content = file_parser_path.read_text()
        result["wired_up"] = "dispatch_makernote" in content

    # Extract supported manufacturers
    dispatcher_content = dispatcher_path.read_text()
    # Match patterns like: "canon" => Some(Box::new(
    manufacturers = re.findall(r'"([a-z][a-z0-9_ ]*)"[^=]*=>\s*Some\(Box::new\(', dispatcher_content)
    result["manufacturers"] = sorted(set(m.title() for m in manufacturers))

    return result


def load_conformance(path: Path) -> dict:
    """Read the JSON written by tools/exiftool-tables/conformance.py --json-out.

    Shape:
      per_format: {FMT: {files, matched, value_diff, missing, renames, extra}}
      renames:    {FMT: {"OxiName->ExifToolName": file_count}}
      missing:    {"FMT:TagName": file_count}
    """
    try:
        data = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        sys.exit(f"could not read conformance JSON {path}: {exc}")

    per_format = data.get("per_format") or {}
    if not per_format:
        sys.exit(
            f"{path} has no per_format data. A conformance run that scored "
            "nothing must not be published as a coverage report."
        )
    return data


def score_row(counts: dict) -> tuple:
    """(total, score, ceiling) for one format, matching conformance.py exactly.

    `extra` is deliberately outside the denominator: an OxiDex-only tag with no
    ExifTool counterpart is not a missed ExifTool tag, and putting it in the
    denominator would penalise emitting more than ExifTool does.
    """
    total = (counts.get("matched", 0) + counts.get("value_diff", 0)
             + counts.get("missing", 0) + counts.get("renames", 0))
    if not total:
        return 0, None, None
    score = counts.get("matched", 0) / total
    # What the score becomes once every rename is corrected: free coverage,
    # no parsing work. A wide score-to-ceiling spread means naming debt.
    ceiling = (counts.get("matched", 0) + counts.get("renames", 0)) / total
    return total, score, ceiling


def render_conformance(data: dict, corpus_desc: str, exiftool_version: str) -> str:
    per_format = data["per_format"]

    md = f"""
---

## Measured Extraction Coverage

Every number in this section comes from running OxiDex and ExifTool
{exiftool_version} over the same files and diffing the output tag by tag. It is
a measurement, not an estimate derived from source code.

**Corpus:** {corpus_desc}

| Column | Meaning |
|--------|---------|
| Match | Same tag name, same value. |
| Rename | OxiDex read the value correctly under a name ExifTool does not use. Value-confirmed, so this is a naming fix, not parsing work. |
| Value | Both emit the tag, values disagree. Usually a `PrintConv` gap. |
| Missing | ExifTool emits a tag OxiDex does not. Real extraction work. |
| Score | Match / (Match + Rename + Value + Missing). |
| Ceiling | Score once every rename is corrected. |

| Format | Files | Match | Rename | Value | Missing | Score | Ceiling |
|--------|------:|------:|-------:|------:|--------:|------:|--------:|
"""

    grand = defaultdict(int)
    for fmt in sorted(per_format):
        counts = per_format[fmt]
        total, score, ceiling = score_row(counts)
        if not total:
            continue
        for k in ("files", "matched", "value_diff", "missing", "renames", "extra"):
            grand[k] += counts.get(k, 0)
        md += (
            f"| {fmt} | {counts.get('files', 0)} | {counts.get('matched', 0)} "
            f"| {counts.get('renames', 0)} | {counts.get('value_diff', 0)} "
            f"| {counts.get('missing', 0)} | {score:.1%} | {ceiling:.1%} |\n"
        )

    g_total, g_score, g_ceiling = score_row(grand)
    if g_total:
        md += (
            f"| **Total** | **{grand['files']}** | **{grand['matched']}** "
            f"| **{grand['renames']}** | **{grand['value_diff']}** "
            f"| **{grand['missing']}** | **{g_score:.1%}** | **{g_ceiling:.1%}** |\n"
        )

    # Renames: the cheapest possible fix class, so they are worth listing
    # explicitly rather than leaving buried in the score-to-ceiling gap.
    renames = data.get("renames") or {}
    flat_renames = sorted(
        ((fmt, pair, n) for fmt, pairs in renames.items() for pair, n in pairs.items()),
        key=lambda r: (-r[2], r[0], r[1]),
    )
    if flat_renames:
        md += f"""
### Renames — free coverage ({len(flat_renames)})

OxiDex reads these values correctly under the wrong name. The value match is
what makes the mapping safe to act on; name similarity alone would be a guess.

| Format | OxiDex name | ExifTool name | Files |
|--------|-------------|---------------|------:|
"""
        for fmt, pair, n in flat_renames[:40]:
            ox_name, _, et_name = pair.partition("->")
            md += f"| {fmt} | `{ox_name}` | `{et_name}` | {n} |\n"
        if len(flat_renames) > 40:
            md += f"\n_{len(flat_renames) - 40} further renames omitted from this table._\n"

    # Missing: the only class that is genuinely parsing work.
    missing = data.get("missing") or {}
    top_missing = sorted(missing.items(), key=lambda kv: (-kv[1], kv[0]))
    if top_missing:
        md += f"""
### Top missing tags — real extraction work ({len(top_missing)} distinct)

| Format | Tag | Files |
|--------|-----|------:|
"""
        for key, n in top_missing[:25]:
            fmt, _, tag = key.partition(":")
            md += f"| {fmt} | `{tag}` | {n} |\n"
        if len(top_missing) > 25:
            md += f"\n_{len(top_missing) - 25} further missing tags omitted from this table._\n"

    return md


def render_unmeasured() -> str:
    return """
---

## Measured Extraction Coverage

::: warning Not measured in this run
This report was generated without a conformance run, so it carries no
extraction-coverage numbers. Tag *definitions* (above) are not extraction
coverage — a tag can be defined and still never be read out of a file.

Run the measurement to populate this section:

```bash
just docs-coverage
```
:::
"""


def generate_markdown(domains: dict, makernote_status: dict,
                      conformance: dict, corpus_desc: str,
                      exiftool_version: str) -> str:
    """Generate the tag coverage markdown report"""
    today = datetime.now(timezone.utc).strftime("%Y-%m-%d")

    total_tags = sum(d["total_tags"] for d in domains.values())
    total_tables = sum(d["total_tables"] for d in domains.values())

    md = f"""# ExifTool Tag Coverage

This document reports two separate things about OxiDex, and does not mix them:
how many tags it has **definitions** for, and how many tags it actually
**extracts** from real files.

::: info Auto-Generated
This document is automatically updated on each push to `main`. Last updated: **{today}**
:::

## Tag Definitions

Counted from the `oxidex-tags-*` YAML databases. This is what OxiDex knows a
tag *exists*; it says nothing about whether any parser reads it.

| Metric | Value |
|--------|-------|
| Total Tags | {total_tags:,} |
| Tag Tables | {total_tables} |
| Domains | {len(domains)} |

::: warning Definitions are not coverage
A definition count is documentation, not capability. `src/tag_sync` ingests
`exiftool -f -listx`, which carries `count encoding id index lang name type
version writable` and no layout at all — no `SubDirectory`, `FORMAT`,
`FIRST_ENTRY`, `ValueConv` or `Condition`. It can say a tag exists; it can
never say how to read one. A rising tag count is therefore **not** evidence of
rising extraction coverage. See [Measured Extraction Coverage](#measured-extraction-coverage).
:::

::: tip Empirical JPEG comparison
For JPEG specifically there is a deeper per-tag comparison against ExifTool
covering read *and* write round-trips, regression-gated in CI:
[JPEG Tag Support](/reference/jpeg-tag-support) ·
[JPEG Tag Matrix](/reference/jpeg-tag-matrix)
:::

---

## Definitions by Domain

| Domain | Tables | Tags | Description |
|--------|--------|------|-------------|
"""

    domain_descriptions = {
        "camera": "MakerNotes from 40+ manufacturers",
        "core": "EXIF, GPS, XMP, IPTC standards",
        "document": "PDF, Office, HTML metadata",
        "image": "PNG, GIF, BMP, WebP, etc.",
        "media": "Audio/video containers",
        "specialty": "FLIR, DICOM, DJI, etc.",
    }

    for domain, data in sorted(domains.items()):
        desc = domain_descriptions.get(domain, "")
        md += f"| {domain.title()} | {data['total_tables']} | {data['total_tags']:,} | {desc} |\n"

    md += f"| **Total** | **{total_tables}** | **{total_tags:,}** | |\n"

    # Measured coverage -- the only section that may state a coverage number.
    if conformance:
        md += render_conformance(conformance, corpus_desc, exiftool_version)
    else:
        md += render_unmeasured()

    # MakerNote status
    md += """
---

## MakerNote Status

"""

    if makernote_status["wired_up"]:
        md += f"""::: tip ✅ MakerNote Parsers Active
MakerNote parsers for {len(makernote_status['manufacturers'])} camera manufacturers are **implemented and connected** to the TIFF parsing pipeline.

This means the dispatcher has an arm for these makes and that the TIFF parser
calls it. It is not a claim about how much of each manufacturer's MakerNote is
extracted — only the conformance table above measures that.
:::

### Dispatched Manufacturers

"""
        # Group manufacturers
        traditional = ["Canon", "Nikon", "Sony", "Olympus", "Panasonic", "Pentax", "Fujifilm", "Leica", "Sigma", "Phase One", "Minolta"]
        smartphones = ["Apple", "Google", "Samsung", "Microsoft", "Qualcomm"]
        specialty = ["Dji", "Flir", "Gopro", "Infiray", "Lytro", "Nintendo", "Parrot", "Reconyx", "Red"]
        legacy = ["Casio", "Ge", "Hp", "Jvc", "Kodak", "Leaf", "Motorola", "Ricoh", "Sanyo"]

        all_mfrs = set(makernote_status["manufacturers"])
        lowered = {x.lower() for x in all_mfrs}

        def filter_mfrs(group):
            return ", ".join(m for m in group if m in all_mfrs or m.lower() in lowered)

        md += f"**Traditional Cameras:** {filter_mfrs(traditional)}\n\n"
        md += f"**Smartphones:** {filter_mfrs(smartphones)}\n\n"
        md += f"**Specialty Devices:** {filter_mfrs(specialty)}\n\n"
        md += f"**Legacy Cameras:** {filter_mfrs(legacy)}\n\n"

        # Anything the dispatcher handles that the four hand-maintained groups
        # above do not mention. Without this, adding a make to the dispatcher
        # silently fails to appear in the docs -- which is how the old
        # format_map went stale and started publishing rows like "CANON_VRD".
        grouped = {m.lower() for m in traditional + smartphones + specialty + legacy}
        ungrouped = sorted(m for m in all_mfrs if m.lower() not in grouped)
        if ungrouped:
            md += f"**Other:** {', '.join(ungrouped)}\n\n"
    else:
        md += """::: warning ⚠️ MakerNote Parsers Not Connected
MakerNote parsers exist but are NOT wired up to the parsing pipeline. This is a critical gap.
:::
"""

    # Module categories (from exiftool-coverage)
    md += """
---

## ExifTool Module Reference

Approximate tag counts published by ExifTool for its own modules, for scale.
These describe ExifTool, not OxiDex, and are not used in any calculation above.

### Base Format Modules

| Module | Tags | Description |
|--------|------|-------------|
| Exif.pm | ~3,732 | Core EXIF tags |
| GPS.pm | ~267 | GPS location data |
| XMP.pm | ~2,012 | XMP metadata |
| IPTC.pm | ~720 | Press/media metadata |
| PDF.pm | ~334 | PDF documents |
| QuickTime.pm | ~6,567 | MOV/MP4 video |
| Photoshop.pm | ~550 | Photoshop metadata |
| PNG.pm | ~100 | PNG images |
| TIFF.pm | ~400 | TIFF format |
| ICC_Profile.pm | ~150 | Color profiles |
| RIFF.pm | ~400 | RIFF/AVI/WAV |

### MakerNotes Modules

| Module | Tags | Description |
|--------|------|-------------|
| Canon.pm | ~7,379 | Canon cameras |
| Nikon.pm | ~9,586 | Nikon cameras |
| Sony.pm | ~7,810 | Sony cameras |
| Pentax.pm | ~4,777 | Pentax cameras |
| Olympus.pm | ~3,194 | Olympus cameras |
| Panasonic.pm | ~1,977 | Panasonic cameras |
| FujiFilm.pm | ~1,177 | FujiFilm cameras |
| Samsung.pm | ~1,012 | Samsung cameras |

### Media Format Modules

| Module | Tags | Description |
|--------|------|-------------|
| Matroska.pm | ~641 | MKV/WebM |
| ID3.pm | ~200 | MP3 ID3 tags |
| FLAC.pm | ~150 | FLAC audio |
| Vorbis.pm | ~100 | Ogg Vorbis |
| ASF.pm | ~300 | WMA/WMV |
| MPEG.pm | ~250 | MPEG video |

### Specialized Modules

| Module | Tags | Description |
|--------|------|-------------|
| FLIR.pm | ~822 | Thermal imaging |
| DICOM.pm | ~500 | Medical imaging |
| DJI.pm | ~300 | DJI drones |
| GoPro.pm | ~250 | Action cameras |
| EXE.pm | ~200 | Executables |

---

## Tag Count Notes

### Why definition counts differ from ExifTool's

The OxiDex tag database and ExifTool's documented tag list are not directly
comparable, because OxiDex stores:

1. **Variant definitions**: Tags with multiple format/type variants
2. **Nested structures**: Subtable entries counted separately
3. **Conditional definitions**: Platform or version-specific tags

Dividing one count by the other produces a ratio that moves for reasons
unrelated to capability, which is why this page does not publish one.

### Excluded Tags

Some ExifTool tags are excluded by design:

- **Composite tags**: Calculated values (Aperture from FNumber, etc.)
- **Shortcut tags**: Aliases to other tags
- **Internal tags**: ExifTool operational tags

---

## Related Documentation

- [Tag Database Architecture](/architecture/tag-database) - Implementation details
- [MakerNotes Reference](/reference/makernotes) - Camera manufacturer metadata
"""

    return md


def main():
    parser = argparse.ArgumentParser(description="Generate tag coverage report")
    parser.add_argument(
        "--output", "-o",
        default="docs/reference/tag-coverage-analysis.md",
        help="Output file path (default: docs/reference/tag-coverage-analysis.md)"
    )
    parser.add_argument(
        "--conformance", "-c",
        help="JSON written by tools/exiftool-tables/conformance.py --json-out. "
             "Required unless --skip-conformance is passed."
    )
    parser.add_argument(
        "--corpus-desc",
        default="tests/fixtures (recursive)",
        help="Human description of the corpus the conformance run scored"
    )
    parser.add_argument(
        "--skip-conformance",
        action="store_true",
        help="Emit definitions only. Implies --dry-run: a report with no "
             "measurement must never overwrite one that has it."
    )
    parser.add_argument(
        "--dry-run", "-n",
        action="store_true",
        help="Print to stdout instead of writing to file"
    )
    args = parser.parse_args()

    if not args.conformance and not args.skip_conformance:
        sys.exit(
            "refusing to generate: no --conformance JSON.\n"
            "  Extraction coverage cannot be inferred from source code, so this\n"
            "  script will not write a coverage report without a measurement.\n"
            "  Run `just docs-coverage`, or pass --skip-conformance for a\n"
            "  definitions-only view on stdout."
        )

    # A definitions-only run is a preview, never a publish. Without this, a
    # local `--skip-conformance` would strip the measured section out of the
    # committed doc and CI would put it straight back, churning main.
    dry_run = args.dry_run or args.skip_conformance

    project_root = get_project_root()
    print(f"Project root: {project_root}")

    exiftool_version = read_exiftool_pin(project_root)
    print(f"ExifTool pin: {exiftool_version}")

    print("Parsing YAML tag files...")
    domains = parse_yaml_tags(project_root)

    conformance = {}
    if args.conformance:
        print(f"Loading conformance results from {args.conformance}...")
        conformance = load_conformance(Path(args.conformance))

    print("Checking MakerNote status...")
    makernote_status = check_makernote_status(project_root)

    print("Generating markdown...")
    markdown = generate_markdown(
        domains, makernote_status, conformance, args.corpus_desc, exiftool_version
    )

    if dry_run:
        print(markdown)
    else:
        output_path = project_root / args.output
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(markdown)
        print(f"Written to: {output_path}")

    # Print summary
    total_tags = sum(d["total_tags"] for d in domains.values())
    print("\nSummary:")
    print(f"  Domains: {len(domains)}")
    print(f"  Total tag definitions: {total_tags:,}")
    print(f"  MakerNotes wired up: {makernote_status['wired_up']}")
    print(f"  Manufacturers dispatched: {len(makernote_status['manufacturers'])}")
    if conformance:
        grand = defaultdict(int)
        for counts in conformance["per_format"].values():
            for k in ("files", "matched", "value_diff", "missing", "renames"):
                grand[k] += counts.get(k, 0)
        total, score, ceiling = score_row(grand)
        if total:
            print(f"  Measured score: {score:.1%} (ceiling {ceiling:.1%}) "
                  f"over {grand['files']} file(s), ExifTool {exiftool_version}")
    else:
        print("  Measured coverage: NOT RUN (definitions only)")


if __name__ == "__main__":
    main()
