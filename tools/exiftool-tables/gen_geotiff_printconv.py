#!/usr/bin/env python3
"""Generate src/parsers/tiff/geotiff_printconv.rs from the pinned ExifTool.

GeoTiff.pm is procedural (ProcessGeoTiff), not a BinaryData table, so the
table generator never transcribes it and `find_table("GeoTiff", ...)` has
nothing.  Its PrintConvs are plain hashes -- no OTHER, no BITMASK, no
sprintf (verified: `grep -n 'OTHER\\|sprintf' GeoTiff.pm` matches only
ProcessGeoTiff itself) -- so an exact transcription is possible, and the
"never approximate a conversion" rule makes anything less unacceptable.

The instrument is perl itself, not a regex over the source: this script asks
the pinned perl to load Image::ExifTool::GeoTiff and dump the resolved
%Image::ExifTool::GeoTiff::Main hash as JSON.  That inherits Perl's own hash
semantics -- GeoTiff.pm line 562f assigns key 2177 twice ('...zone 6' then
'...zone 7') and last-wins is what ExifTool actually prints -- and cannot
drift from what the module really contains the way a hand parse can.

Shared PrintConv refs (\\%epsg_units, \\%epsg_vertcs) are detected by refaddr
and emitted once.  Every emitted map cites its source hash by name and line
range in GeoTiff.pm.

Usage:
    python3 tools/exiftool-tables/gen_geotiff_printconv.py \
        [--exiftool-dir /tmp/oxidex-exiftool-cache/exiftool] [--check]

--check regenerates in memory and fails if the committed file differs
(the same shape as `just verify-tables`' drift check).
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
OUT_PATH = REPO_ROOT / "src" / "parsers" / "tiff" / "geotiff_printconv.rs"
DEFAULT_EXIFTOOL_DIR = Path("/tmp/oxidex-exiftool-cache/exiftool")

PERL_DUMP = r"""
use strict;
use warnings;
use Scalar::Util qw(refaddr);
use JSON::PP;
use Image::ExifTool;
use Image::ExifTool::GeoTiff;
my $main = \%Image::ExifTool::GeoTiff::Main;
my %out;
for my $key (grep { /^\d+$/ } keys %$main) {
    my $info = $$main{$key};
    my %entry;
    if (ref $info eq 'HASH') {
        $entry{name} = $$info{Name};
        my $pc = $$info{PrintConv};
        if (defined $pc) {
            die "non-hash PrintConv for key $key" unless ref $pc eq 'HASH';
            for my $k (keys %$pc) {
                die "non-numeric PrintConv key '$k' for tag $key"
                    unless $k =~ /^\d+$/;
            }
            $entry{printconv} = $pc;
            $entry{refaddr} = refaddr($pc);
        }
    } elsif (not ref $info) {
        $entry{name} = $info;
    } else {
        die "unexpected entry type for key $key: " . ref $info;
    }
    $out{$key} = \%entry;
}
print JSON::PP->new->canonical->encode(\%out);
"""


def dump_main_table(exiftool_dir: Path) -> dict:
    version = subprocess.run(
        ["perl", "-I", str(exiftool_dir / "lib"), "-MImage::ExifTool",
         "-e", "print $Image::ExifTool::VERSION"],
        capture_output=True, text=True, check=True,
    ).stdout.strip()
    pinned = (REPO_ROOT / ".exiftool-version").read_text().strip()
    if version != pinned:
        sys.exit(
            f"error: {exiftool_dir} is ExifTool {version}, but "
            f".exiftool-version pins {pinned}; refusing to transcribe from "
            f"an unpinned tree"
        )
    result = subprocess.run(
        ["perl", "-I", str(exiftool_dir / "lib"), "-e", PERL_DUMP],
        capture_output=True, text=True, check=True,
    )
    return version, json.loads(result.stdout)


def hash_line_ranges(exiftool_dir: Path) -> dict[str, str]:
    """Line ranges of the source hashes, for citations in the output."""
    text = (exiftool_dir / "lib/Image/ExifTool/GeoTiff.pm").read_text()
    lines = text.splitlines()
    ranges = {}

    def block_end(start: int, closer: str) -> int:
        for i in range(start, len(lines)):
            if lines[i].rstrip() == closer:
                return i + 1
        raise AssertionError(f"no closer {closer!r} after line {start}")

    for i, line in enumerate(lines):
        m = re.match(r"my %(epsg_units|epsg_vertcs) = \($", line)
        if m:
            ranges[m.group(1)] = f"lines {i + 1}-{block_end(i, ');')}"
    m = next(i for i, l in enumerate(lines)
             if l.startswith("%Image::ExifTool::GeoTiff::Main"))
    ranges["Main"] = f"lines {m + 1}-{block_end(m, ');')}"
    return ranges


# Static names for the PrintConv maps.  Shared maps (by refaddr) get the
# name of the Perl hash they came from; inline maps get one derived from the
# tag they belong to.  The comment on each map cites the source.
SHARED_NAMES = {
    frozenset({2052, 2054, 2060, 3076, 4099, 47009}): "EPSG_UNITS",
    frozenset({4096, 4098}): "EPSG_VERTCS",
}
INLINE_NAMES = {
    1024: ("GT_MODEL_TYPE", "GTModelType PrintConv"),
    1025: ("GT_RASTER_TYPE", "GTRasterType PrintConv"),
    2048: ("EPSG_GCS", "GeographicType PrintConv (# epsg_gcs)"),
    2050: ("EPSG_DATUM", "GeogGeodeticDatum PrintConv (# epsg_datum)"),
    2051: ("EPSG_PM", "GeogPrimeMeridian PrintConv (# epsg_pm)"),
    2056: ("EPSG_ELLIPSE", "GeogEllipsoid PrintConv (# epsg_ellipse)"),
    3072: ("EPSG_PCS", "ProjectedCSType PrintConv (# epsg_pcs)"),
    3074: ("EPSG_PROJ", "Projection PrintConv (# epsg_proj)"),
    3075: ("GEO_CTRANS", "ProjCoordTrans PrintConv (# geo_ctrans)"),
    47001: ("CHART_FORMAT", "ChartFormat PrintConv"),
    47008: ("CHART_SOUNDING_DATUM", "ChartSoundingDatum PrintConv"),
}


def rust_str(s: str) -> str:
    if not s.isascii():
        sys.exit(f"error: non-ASCII PrintConv string {s!r}; extend rust_str")
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def generate(exiftool_dir: Path) -> str:
    version, table = dump_main_table(exiftool_dir)
    ranges = hash_line_ranges(exiftool_dir)

    keys = sorted(int(k) for k in table)
    # group PrintConvs by refaddr to find shared maps
    by_addr: dict[int, list[int]] = {}
    for k in keys:
        entry = table[str(k)]
        if "printconv" in entry:
            by_addr.setdefault(entry["refaddr"], []).append(k)

    map_name_for_key: dict[int, str] = {}
    maps: list[tuple[str, str, dict]] = []  # (static name, citation, hash)
    for addr, tag_keys in sorted(by_addr.items(), key=lambda kv: kv[1][0]):
        tag_set = frozenset(tag_keys)
        if len(tag_keys) > 1:
            name = SHARED_NAMES.get(tag_set)
            if name is None:
                sys.exit(f"error: unrecognized shared PrintConv on tags "
                         f"{sorted(tag_keys)}; name it in SHARED_NAMES")
            src = name.lower()
            cite = f"%{src} ({ranges[src]})"
        else:
            (key,) = tag_keys
            if key not in INLINE_NAMES:
                sys.exit(f"error: tag {key} grew a PrintConv; "
                         f"name it in INLINE_NAMES")
            name, what = INLINE_NAMES[key]
            cite = f"{what}, inside Main ({ranges['Main']})"
        for k in tag_keys:
            map_name_for_key[k] = name
        maps.append((name, cite, table[str(tag_keys[0])]["printconv"]))

    out = []
    out.append("//! GeoTIFF key names and PrintConv maps, transcribed EXACTLY from the")
    out.append(f"//! pinned ExifTool {version} `lib/Image/ExifTool/GeoTiff.pm`.")
    out.append("//!")
    out.append("//! Generated by `tools/exiftool-tables/gen_geotiff_printconv.py` -- DO NOT")
    out.append("//! EDIT BY HAND; rerun the generator (it dumps the live")
    out.append("//! `%Image::ExifTool::GeoTiff::Main` hash through the pinned perl, so the")
    out.append("//! values here are what ExifTool itself resolves, including Perl's")
    out.append("//! last-assignment-wins on GeoTiff.pm's duplicated key 2177).")
    out.append("//!")
    out.append("//! `GeoTiff.pm` declares no `OTHER`, `BITMASK`, or code PrintConv, so these")
    out.append("//! plain maps are the complete conversion story; a lookup miss prints as")
    out.append("//! `Unknown (N)` exactly like ExifTool.pm's HASH PrintConv fallback")
    out.append("//! (ExifTool.pm line 3633, no PrintHex on any GeoTiff tag).")
    out.append("")

    for name, cite, pc in maps:
        entries = sorted((int(k), v) for k, v in pc.items())
        out.append(f"/// {cite}")
        out.append(f"pub static {name}: &[(u16, &str)] = &[")
        for k, v in entries:
            out.append(f"    ({k}, {rust_str(v)}),")
        out.append("];")
        out.append("")

    out.append(f"/// Key ID -> tag name, from %Main ({ranges['Main']}). Keys absent here are")
    out.append("/// skipped entirely, mirroring ProcessGeoTiff's `GetTagInfo(...) or next`.")
    out.append("pub static GEOKEY_NAMES: &[(u16, &str)] = &[")
    for k in keys:
        out.append(f"    ({k}, {rust_str(table[str(k)]['name'])}),")
    out.append("];")
    out.append("")

    out.append("/// The PrintConv map for a key, or None for keys printed as-is.")
    out.append("pub fn geokey_print_conv(key_id: u16) -> Option<&'static [(u16, &'static str)]> {")
    out.append("    match key_id {")
    emitted: dict[str, list[int]] = {}
    for k in keys:
        if k in map_name_for_key:
            emitted.setdefault(map_name_for_key[k], []).append(k)
    for name, _, _ in maps:
        pattern = " | ".join(str(k) for k in emitted[name])
        out.append(f"        {pattern} => Some({name}),")
    out.append("        _ => None,")
    out.append("    }")
    out.append("}")
    out.append("")

    out.append("/// The ExifTool tag name for a GeoTIFF key ID, or None if ExifTool's Main")
    out.append("/// table has no entry (ProcessGeoTiff skips such keys without -u).")
    out.append("pub fn geokey_name(key_id: u16) -> Option<&'static str> {")
    out.append("    GEOKEY_NAMES")
    out.append("        .binary_search_by_key(&key_id, |&(k, _)| k)")
    out.append("        .ok()")
    out.append("        .map(|i| GEOKEY_NAMES[i].1)")
    out.append("}")
    out.append("")

    out.append("/// Look up `value` in a PrintConv map (maps are sorted by key).")
    out.append("pub fn print_conv_lookup(map: &'static [(u16, &str)], value: u16) -> Option<&'static str> {")
    out.append("    map.binary_search_by_key(&value, |&(k, _)| k)")
    out.append("        .ok()")
    out.append("        .map(|i| map[i].1)")
    out.append("}")

    return "\n".join(out) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--exiftool-dir", type=Path, default=DEFAULT_EXIFTOOL_DIR)
    ap.add_argument("--check", action="store_true",
                    help="fail if the committed file is not what this "
                         "generator produces")
    args = ap.parse_args()

    text = generate(args.exiftool_dir)
    if args.check:
        current = OUT_PATH.read_text() if OUT_PATH.exists() else ""
        if current != text:
            sys.exit(f"error: {OUT_PATH} is stale; rerun {sys.argv[0]}")
        print(f"{OUT_PATH} matches the generator output")
        return 0
    OUT_PATH.write_text(text)
    print(f"wrote {OUT_PATH}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
