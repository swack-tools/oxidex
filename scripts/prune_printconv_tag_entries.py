#!/usr/bin/env python3
"""Remove PrintConv display values that were flattened into the tag registry.

## The defect

`exiftool -f -listx` nests a tag's PrintConv table *inside* the tag:

    <tag id='41986' name='ExposureMode' type='int16u'>
      <values>
        <key id='1'><val lang='en'>Manual</val></key>
        <key id='2'><val lang='en'>Auto bracket</val></key>
      </values>
    </tag>

Whatever originally produced `oxidex-tags-*/src/*_tags.yaml` treated every
element carrying an `id` attribute as a tag, so each `<key>` became a
sibling *tag* entry:

      - id: "0xA402"
        name: "ExposureMode"      # the real tag
      - id: "0x0001"
        name: "Manual"            # <- PrintConv KEY 1, not a tag id
      - id: "0x0002"
        name: "Auto bracket"      # <- PrintConv KEY 2

That is why fabricated ids restart at 0x0001 in the middle of a table:
they are PrintConv keys, not tag ids. The registry then emits them as
real tags, which is how `EXIF:Higher resolution image exists` (OPIProxy's
PrintConv value 1) reached output and aborted a CR2 sweep.

The correct home for these strings is the enum decoders that already own
them -- `1 => "Reduced-resolution image"` in src/parsers/tiff/tiff_enums.rs.

## Why a script rather than an edit

`src/bin/sync_tags.rs` is the real generator, but it has never run: it
refuses to write when a domain's tag count falls below 90% of the previous
file, and because the committed files are ~50% inflated by this very bug,
the correct output looks to it like a parsing regression. So the YAML is
hand-maintained source, not generated output, and this prunes it in place
deterministically. `tests/tag_registry_invariants.rs` is the permanent
guard; this script is the one-time repair, kept so the transformation is
auditable and re-runnable.

Usage:
    python3 scripts/prune_printconv_tag_entries.py            # prune in place
    python3 scripts/prune_printconv_tag_entries.py --check    # report only
"""

from __future__ import annotations

import argparse
import re
import subprocess  # nosec B404 -- list-argv only, no shell=True
import sys
import xml.etree.ElementTree as ET
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DOMAINS = ("core", "camera", "media", "image", "document", "specialty")

# Every one of ExifTool's 31,942 tag names matches this. A name carrying a
# space, colon, parenthesis or arrow is therefore not a tag name at all.
TAG_NAME_RE = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_-]*\Z")

TABLE_RE = re.compile(r"^  - name:\s*(\S+)\s*$")
ENTRY_RE = re.compile(r'^      - id:\s*"([^"]*)"\s*$')
NAME_RE = re.compile(r'^        name:\s*"([^"]*)"\s*$')


def exiftool_ground_truth(listx_path: str | None):
    """Per table: the real tag names, and the PrintConv display values."""
    if listx_path:
        source = Path(listx_path).read_bytes()
    else:
        source = subprocess.run(  # nosec B603,B607
            ["exiftool", "-f", "-listx"], capture_output=True, check=True
        ).stdout

    tags: dict[str, set[str]] = defaultdict(set)
    values: dict[str, set[str]] = defaultdict(set)
    root = ET.fromstring(source)  # nosec B314 -- output of a local trusted binary
    for table in root.iter("table"):
        name = table.get("name", "")
        for tag in table.findall("tag"):
            tags[name].add(tag.get("name", ""))
            for val in tag.findall("./values/key/val"):
                if val.get("lang") == "en" and val.text:
                    values[name].add(val.text.strip())
    return tags, values


def split_entries(lines):
    """Yield ('table', name, [lines]) and ('entry', name, [lines]) blocks.

    Anything else (header, comments, blank lines) is yielded as
    ('other', None, [lines]) so the file round-trips byte-for-byte when
    nothing is dropped.
    """
    i = 0
    pending: list[str] = []
    while i < len(lines):
        line = lines[i]
        if m := TABLE_RE.match(line):
            if pending:
                yield ("other", None, pending)
                pending = []
            block = [line]
            i += 1
            while i < len(lines) and not (
                TABLE_RE.match(lines[i]) or ENTRY_RE.match(lines[i])
            ):
                block.append(lines[i])
                i += 1
            yield ("table", m.group(1), block)
            continue
        if ENTRY_RE.match(line):
            if pending:
                yield ("other", None, pending)
                pending = []
            block = [line]
            i += 1
            name = ""
            while i < len(lines) and not (
                TABLE_RE.match(lines[i]) or ENTRY_RE.match(lines[i])
            ):
                if m2 := NAME_RE.match(lines[i]):
                    name = m2.group(1)
                block.append(lines[i])
                i += 1
            yield ("entry", name, block)
            continue
        pending.append(line)
        i += 1
    if pending:
        yield ("other", None, pending)


def classify(table: str, name: str, real, values) -> str | None:
    """Why this entry must go, or None to keep it."""
    if name in real.get(table, ()):
        return None  # a genuine tag of this table
    if not TAG_NAME_RE.fullmatch(name):
        return "impossible-tag-name"
    if name in values.get(table, ()):
        return "printconv-value-as-tag"
    return None


def prune_file(path: Path, real, values, write: bool):
    lines = path.read_text().splitlines()
    out: list[str] = []
    dropped = defaultdict(int)
    kept_tables = 0

    blocks = list(split_entries(lines))
    i = 0
    while i < len(blocks):
        kind, name, body = blocks[i]
        if kind != "table":
            out.extend(body)
            i += 1
            continue
        # Gather this table's entries, then keep the table only if any survive.
        table_name = name
        entries = []
        j = i + 1
        while j < len(blocks) and blocks[j][0] == "entry":
            entries.append(blocks[j])
            j += 1
        survivors = []
        for _, ename, ebody in entries:
            reason = classify(table_name, ename, real, values)
            if reason:
                dropped[reason] += 1
            else:
                survivors.append(ebody)
        if survivors:
            kept_tables += 1
            out.extend(body)
            for ebody in survivors:
                out.extend(ebody)
        else:
            dropped["table-emptied"] += 1
        i = j

    if write and dropped:
        path.write_text("\n".join(out) + "\n")
    return dropped, kept_tables


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true",
                    help="report what would be dropped without writing")
    ap.add_argument("--listx", help="cached `exiftool -f -listx` XML (else runs exiftool)")
    args = ap.parse_args()

    real, values = exiftool_ground_truth(args.listx)
    print(f"ground truth: {len(real)} tables, "
          f"{sum(len(v) for v in real.values())} tags, "
          f"{sum(len(v) for v in values.values())} printconv values\n")

    grand = defaultdict(int)
    for domain in DOMAINS:
        path = REPO / f"oxidex-tags-{domain}" / "src" / f"{domain}_tags.yaml"
        if not path.is_file():
            print(f"  {domain:10s} MISSING {path}")
            continue
        dropped, kept = prune_file(path, real, values, write=not args.check)
        total = sum(v for k, v in dropped.items() if k != "table-emptied")
        for k, v in dropped.items():
            grand[k] += v
        print(f"  {domain:10s} -{total:6d} entries  "
              f"({dict(dropped) or 'clean'})  {kept} tables kept")

    print("\ntotal:")
    for k, v in sorted(grand.items(), key=lambda kv: -kv[1]):
        print(f"  {v:6d}  {k}")
    if args.check and grand:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
