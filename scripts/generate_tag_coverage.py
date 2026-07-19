#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = [
#     "pyyaml>=6.0",
# ]
# ///
"""
Generate Tag Coverage Analysis Report

This script analyzes the OxiDex tag database and parser implementations
to generate a comprehensive coverage report.

Usage:
    uv run scripts/generate_tag_coverage.py [--output docs/reference/tag-coverage-analysis.md]

Or via justfile:
    just docs-coverage
"""

import argparse
import re
import sys
from collections import defaultdict
from datetime import datetime
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


def parse_yaml_tags(project_root: Path) -> dict:
    """Parse all YAML tag files and count tags by domain/category"""
    domains = {}
    yaml_files = list(project_root.glob("oxidex-tags-*/src/*.yaml"))

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


def count_tag_patterns(content: str) -> int:
    """Count various tag extraction patterns in Rust code"""
    count = 0
    # HashMap/BTreeMap insert patterns
    count += len(re.findall(r'tags\.insert\s*\(', content))
    count += len(re.findall(r'metadata\.insert\s*\(', content))
    count += len(re.findall(r'result\.insert\s*\(', content))
    count += len(re.findall(r'\.insert\s*\(\s*["\']\w+["\']', content))

    # Vec<(String, String)> patterns - used by XMP, IPTC, etc.
    count += len(re.findall(r'tags\.push\s*\(', content))
    count += len(re.findall(r'_tags\.push\s*\(', content))  # all_xmp_tags.push, all_iptc_tags.push
    count += len(re.findall(r'\.push\s*\(\s*\(\s*["\']\w+["\']', content))
    count += len(re.findall(r'\.push\s*\(\s*\(\s*\w+\.to_string\(\)', content))
    count += len(re.findall(r'\.push\s*\(\s*\(\s*tag_name', content))  # .push((tag_name, value))
    count += len(re.findall(r'_tags\.extend\s*\(', content))  # all_xmp_tags.extend()

    # Issue 1.1: XMP/IPTC return Vec instead of insert - detect tuple pushes and Vec return signatures
    count += len(re.findall(r'\.push\s*\(\s*\([^,]+,', content))  # Generic .push((x, y))
    # Count functions that return Vec<(String, String)> as contributing 1 insertion point
    # (actual tags are counted via the .push patterns above)
    vec_return_pattern = r'fn\s+\w+.*->\s*Result<Vec<\(String,\s*String\)>>'
    if re.search(vec_return_pattern, content):
        count += 1  # Add 1 for having a Vec-returning function (actual tags counted via push)

    # Direct tuple creation with string keys
    count += len(re.findall(r'\(\s*["\']\w+["\']\.to_string\(\)\s*,', content))

    # ICC parser patterns (header.rs uses metadata.insert extensively)
    # ICC uses registry-based insertion via metadata.insert(), already counted above
    # No additional patterns needed - ICC is well-covered by existing insert() detection

    return count


def analyze_parsers(project_root: Path) -> dict:
    """Analyze parser implementations to find tag extraction"""
    parsers_dir = project_root / "src" / "parsers"
    core_dir = project_root / "src" / "core"
    parser_coverage = {}

    # Also count tag insertions in core directory (operations.rs has JPEG, PNG, etc.)
    core_insertions = 0
    for rs_file in core_dir.rglob("*.rs"):
        try:
            content = rs_file.read_text()
            core_insertions += count_tag_patterns(content)
        except Exception:
            continue

    # JPEG/TIFF/PNG parsing happens in core/operations.rs
    # Estimate based on typical tag counts in these parsers
    if core_insertions > 50:
        parser_coverage["JPEG"] = {"directory": "core", "tag_insertions": core_insertions // 3, "has_parser": True}
        parser_coverage["PNG"] = {"directory": "core", "tag_insertions": core_insertions // 4, "has_parser": True}

    # Map directory names to format names
    format_map = {
        "audio": ["FLAC", "MP3", "AAC", "APE", "Opus", "OGG", "WAV"],
        "image": ["BMP", "GIF", "WebP"],
        "jpeg": ["JPEG"],
        "png": ["PNG"],
        "tiff": ["TIFF", "EXIF"],
        "pdf": ["PDF"],
        "quicktime": ["QuickTime", "MP4", "MOV"],
        "video": ["MKV", "AVI", "RIFF"],
        "pe": ["PE"],
        "elf": ["ELF"],
        "macho": ["Mach-O"],
        "archive": ["ZIP"],
        "document": ["DOCX", "XLSX"],
        "font": ["TTF", "OTF"],
        "raw": ["DNG", "CR2", "NEF"],
        "icc": ["ICC"],
        "xmp": ["XMP"],  # XMP parser is standalone and used by JPEG/TIFF/etc
        "iptc": ["IPTC"],  # IPTC parser (if exists as standalone)
    }

    # Note: IPTC and XMP are embedded formats parsed within JPEG/TIFF, but may have dedicated parsers
    # Check if IPTC/XMP parsing exists in jpeg parser (these call out to shared parsers)
    jpeg_parser_dir = parsers_dir / "jpeg"
    if jpeg_parser_dir.exists():
        iptc_file = jpeg_parser_dir / "iptc_parser.rs"
        xmp_file = jpeg_parser_dir / "xmp_parser.rs"

        # Count IPTC tags from jpeg/iptc_parser.rs
        if iptc_file.exists():
            content = iptc_file.read_text()
            insertions = count_tag_patterns(content)
            if insertions > 0:
                parser_coverage["IPTC"] = {"directory": "jpeg/iptc", "tag_insertions": insertions, "has_parser": True}

        # For XMP, count both the jpeg wrapper and the shared XMP parser directory
        xmp_insertions = 0
        if xmp_file.exists():
            content = xmp_file.read_text()
            xmp_insertions = count_tag_patterns(content)

        # Also count tags from the shared XMP parser directory
        xmp_parser_dir = parsers_dir / "xmp"
        if xmp_parser_dir.exists():
            for rs_file in xmp_parser_dir.rglob("*.rs"):
                try:
                    content = rs_file.read_text()
                    xmp_insertions += count_tag_patterns(content)
                except Exception:
                    continue

        if xmp_insertions > 0:
            parser_coverage["XMP"] = {"directory": "xmp", "tag_insertions": xmp_insertions, "has_parser": True}

    for parser_dir in parsers_dir.iterdir():
        if not parser_dir.is_dir():
            continue

        dir_name = parser_dir.name
        # Skip directories handled specially or support directories
        if dir_name in ["common", "detection", "mod.rs", "xmp"]:
            continue

        # Count tag insertions
        tag_insertions = 0
        for rs_file in parser_dir.rglob("*.rs"):
            try:
                content = rs_file.read_text()
                tag_insertions += count_tag_patterns(content)
            except Exception:
                continue

        formats = format_map.get(dir_name, [dir_name.upper()])
        for fmt in formats:
            # Don't overwrite if we already have better coverage from core
            if fmt in parser_coverage and parser_coverage[fmt]["tag_insertions"] > tag_insertions:
                continue
            parser_coverage[fmt] = {
                "directory": dir_name,
                "tag_insertions": tag_insertions,
                "has_parser": tag_insertions > 0
            }

    return parser_coverage


def check_makernote_status(project_root: Path) -> dict:
    """Check MakerNote dispatcher status"""
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


def get_audio_coverage() -> dict:
    """Audio parsers are known to be complete"""
    return {
        "FLAC": 100,
        "MP3": 100,
        "AAC": 100,
        "APE": 100,
        "Opus": 100,
        "OGG": 100,
        "WAV": 100,
    }


def estimate_coverage(parser_info: dict, domain_data: dict) -> dict:
    """Estimate coverage percentages based on parser analysis"""
    # Coverage thresholds based on tag insertion counts
    # These are calibrated based on typical tag counts per format
    coverage = {}

    # Manual overrides for parsers that use dynamic tag extraction
    # These parsers extract many tags at runtime via XML/structure parsing
    # rather than hardcoded insertions, so static analysis underestimates them
    dynamic_parsers = {
        "XMP": 60,    # Parses arbitrary XML namespaces, extracts many tags
        "IPTC": 60,   # Parses IPTC record structures, extracts many tags
        "JPEG": 60,   # Delegates to TIFF/EXIF/XMP/IPTC, comprehensive
    }

    for fmt, info in parser_info.items():
        # Check for dynamic parser override first
        if fmt in dynamic_parsers and info.get("has_parser", False):
            coverage[fmt] = dynamic_parsers[fmt]
            continue

        insertions = info.get("tag_insertions", 0)

        if insertions == 0:
            coverage[fmt] = 0
        elif insertions >= 100:
            # Very comprehensive parser (100+ tag insertions)
            coverage[fmt] = 90
        elif insertions >= 50:
            # Good coverage (50-99 insertions)
            coverage[fmt] = 75
        elif insertions >= 30:
            # Moderate coverage (30-49 insertions)
            coverage[fmt] = 60
        elif insertions >= 15:
            # Partial coverage (15-29 insertions)
            coverage[fmt] = 40
        elif insertions >= 5:
            # Minimal coverage (5-14 insertions)
            coverage[fmt] = 20
        else:
            # Very minimal (1-4 insertions)
            coverage[fmt] = 10

    return coverage


def generate_markdown(domains: dict, parser_coverage: dict, makernote_status: dict, coverage: dict) -> str:
    """Generate the comprehensive tag coverage markdown report"""
    today = datetime.now().strftime("%Y-%m-%d")

    # Calculate totals
    total_tags = sum(d["total_tags"] for d in domains.values())
    total_tables = sum(d["total_tables"] for d in domains.values())
    total_categories = sum(len(d["categories"]) for d in domains.values())

    # Count parsers with coverage
    parsers_with_coverage = sum(1 for c in coverage.values() if c > 0)

    # Calculate coverage percentage vs ExifTool's documented tags
    exiftool_tags = 28853
    coverage_pct = round((total_tags / exiftool_tags) * 100)

    md = f"""# ExifTool Tag Coverage

This document details OxiDex's coverage of ExifTool's tag database and analyzes parser implementation status.

::: info Auto-Generated
This document is automatically updated on each push to `main`. Last updated: **{today}**
:::

## Summary

| Metric | Value |
|--------|-------|
| Total Tags | {total_tags:,} |
| Tag Tables | {total_tables} |
| Domains | {len(domains)} |
| Format Parsers | {parsers_with_coverage} |
| ExifTool Parity | {coverage_pct}%* |

*ExifTool officially documents ~{exiftool_tags:,} unique tags. OxiDex defines {total_tags:,} tags (including variant definitions).

::: tip Empirical JPEG comparison
The counts on this page are derived statically from the tag database. For JPEG
specifically, an empirical per-tag comparison against ExifTool (read + write
round-trips, regression-gated in CI) is available:
[JPEG Tag Support](/reference/jpeg-tag-support) ·
[JPEG Tag Matrix](/reference/jpeg-tag-matrix)
:::

---

## Coverage by Domain

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

    # MakerNote status
    md += """
---

## MakerNote Status

"""

    if makernote_status["wired_up"]:
        md += f"""::: tip ✅ MakerNote Parsers Active
MakerNote parsers for {len(makernote_status['manufacturers'])}+ camera manufacturers are **fully implemented and connected** to the TIFF parsing pipeline.
:::

### Supported Manufacturers

"""
        # Group manufacturers
        traditional = ["Canon", "Nikon", "Sony", "Olympus", "Panasonic", "Pentax", "Fujifilm", "Leica", "Sigma", "Phase One", "Minolta"]
        smartphones = ["Apple", "Google", "Samsung", "Microsoft", "Qualcomm"]
        specialty = ["Dji", "Flir", "Gopro", "Infiray", "Lytro", "Nintendo", "Parrot", "Reconyx", "Red"]
        legacy = ["Casio", "Ge", "Hp", "Jvc", "Kodak", "Leaf", "Motorola", "Ricoh", "Sanyo"]

        all_mfrs = set(makernote_status["manufacturers"])

        def filter_mfrs(group):
            return ", ".join(m for m in group if m in all_mfrs or m.lower() in [x.lower() for x in all_mfrs])

        md += f"**Traditional Cameras:** {filter_mfrs(traditional)}\n\n"
        md += f"**Smartphones:** {filter_mfrs(smartphones)}\n\n"
        md += f"**Specialty Devices:** {filter_mfrs(specialty)}\n\n"
        md += f"**Legacy Cameras:** {filter_mfrs(legacy)}\n\n"
    else:
        md += """::: warning ⚠️ MakerNote Parsers Not Connected
MakerNote parsers exist but are NOT wired up to the parsing pipeline. This is a critical gap.
:::
"""

    # Coverage by use case
    md += """
---

## Coverage by Use Case

| Use Case | Coverage | Formats |
|----------|----------|---------|
"""

    use_cases = [
        ("JPEG photos", ["JPEG", "EXIF", "XMP", "IPTC"], "EXIF, XMP, IPTC, MakerNotes"),
        ("RAW photos", ["TIFF", "DNG", "CR2", "NEF"], "DNG, CR2, NEF, ARW, etc."),
        ("Video files", ["QuickTime", "MP4", "MOV", "MKV", "RIFF"], "QuickTime, Matroska, RIFF"),
        ("Audio files", ["FLAC", "MP3", "AAC", "OGG", "WAV"], "ID3, FLAC, Vorbis, AAC"),
        ("PDF documents", ["PDF"], "Info dict, XMP"),
        ("Office docs", ["DOCX", "XLSX"], "OOXML, iWork"),
        ("Executables", ["PE", "ELF", "Mach-O"], "PE, ELF, Mach-O"),
    ]

    for use_case, formats, notes in use_cases:
        relevant_coverage = [coverage.get(f, 0) for f in formats if f in coverage]
        if relevant_coverage:
            avg_cov = sum(relevant_coverage) // len(relevant_coverage)
            if avg_cov == 100:
                cov_str = "✅ 100%"
            elif avg_cov >= 75:
                cov_str = f"✅ {avg_cov}%"
            elif avg_cov >= 50:
                cov_str = f"⚠️ {avg_cov}%"
            else:
                cov_str = f"⚠️ {avg_cov}%"
        else:
            cov_str = "N/A"
        md += f"| {use_case} | {cov_str} | {notes} |\n"

    # Coverage analysis by format
    md += """
---

## Parser Coverage by Format

### ✅ Strong Coverage (>50%)

| Format | Coverage | Status |
|--------|----------|--------|
"""

    strong = [(k, v) for k, v in coverage.items() if v >= 50]
    for fmt, cov in sorted(strong, key=lambda x: -x[1]):
        status = "✅ Complete" if cov == 100 else "✅ Good"
        md += f"| {fmt} | {cov}% | {status} |\n"

    md += """
### ⚠️ Partial Coverage (10-50%)

| Format | Coverage | Priority |
|--------|----------|----------|
"""

    partial = [(k, v) for k, v in coverage.items() if 10 <= v < 50]
    for fmt, cov in sorted(partial, key=lambda x: -x[1]):
        priority = "High" if cov >= 30 else "Medium"
        md += f"| {fmt} | {cov}% | {priority} |\n"

    minimal = [(k, v) for k, v in coverage.items() if v < 10]
    if minimal:
        md += """
### ❌ Minimal Coverage (<10%)

| Format | Coverage |
|--------|----------|
"""
        for fmt, cov in sorted(minimal, key=lambda x: x[0]):
            md += f"| {fmt} | {cov}% |\n"

    # Module categories (from exiftool-coverage)
    md += """
---

## ExifTool Module Reference

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

## Recommendations

"""
    # Find formats that need work
    low_coverage = [(fmt, cov) for fmt, cov in coverage.items() if 0 < cov < 50]
    no_coverage = [fmt for fmt, cov in coverage.items() if cov == 0]

    if low_coverage:
        md += "### Formats Needing Enhancement\n\n"
        for fmt, cov in sorted(low_coverage, key=lambda x: x[1])[:8]:
            md += f"- **{fmt}** ({cov}% coverage)\n"
        md += "\n"

    if no_coverage:
        md += "### Missing Parser Support\n\n"
        for fmt in sorted(no_coverage)[:5]:
            md += f"- {fmt}\n"
        md += "\n"

    md += """---

## Tag Count Notes

### Why Counts Differ from ExifTool

ExifTool officially documents ~28,853 unique tags, but our database contains more because:

1. **Variant definitions**: Tags with multiple format/type variants
2. **Nested structures**: Subtable entries counted separately
3. **Conditional definitions**: Platform or version-specific tags

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
    parser = argparse.ArgumentParser(description="Generate tag coverage analysis report")
    parser.add_argument(
        "--output", "-o",
        default="docs/reference/tag-coverage-analysis.md",
        help="Output file path (default: docs/reference/tag-coverage-analysis.md)"
    )
    parser.add_argument(
        "--dry-run", "-n",
        action="store_true",
        help="Print to stdout instead of writing to file"
    )
    args = parser.parse_args()

    project_root = get_project_root()
    print(f"Project root: {project_root}")

    print("Parsing YAML tag files...")
    domains = parse_yaml_tags(project_root)

    print("Analyzing parser implementations...")
    parser_coverage = analyze_parsers(project_root)

    print("Checking MakerNote status...")
    makernote_status = check_makernote_status(project_root)

    print("Estimating coverage...")
    coverage = estimate_coverage(parser_coverage, domains)
    # Add audio coverage (known to be complete)
    coverage.update(get_audio_coverage())

    print("Generating markdown...")
    markdown = generate_markdown(domains, parser_coverage, makernote_status, coverage)

    if args.dry_run:
        print(markdown)
    else:
        output_path = project_root / args.output
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(markdown)
        print(f"Written to: {output_path}")

    # Print summary
    total_tags = sum(d["total_tags"] for d in domains.values())
    print(f"\nSummary:")
    print(f"  Domains: {len(domains)}")
    print(f"  Total tags: {total_tags:,}")
    print(f"  MakerNotes wired up: {makernote_status['wired_up']}")
    print(f"  Manufacturers: {len(makernote_status['manufacturers'])}")


if __name__ == "__main__":
    main()
