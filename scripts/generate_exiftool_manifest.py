#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = [
#     "defusedxml>=0.7.1",
# ]
# ///
"""Build the ExifTool JPEG tag manifest used by scripts/jpeg_tag_matrix.py.

Dumps ExifTool's tag database (`exiftool -f -listx`) for the JPEG-relevant
groups, and emits:
  <WORK>/exiftool_jpeg_tags.json           all JPEG tags ExifTool can write,
                                           with a type-appropriate sample value
  <WORK>/exiftool_jpeg_readonly_tags.json  JPEG-relevant read-only tags

Environment overrides:
  EXIFTOOL       exiftool executable (default: exiftool)
  TAGMATRIX_WORK work dir (default: <system temp dir>/oxidex-tagmap)

Usage: uv run scripts/generate_exiftool_manifest.py [--flag-noops]

--flag-noops additionally write-tests suspect tags (MakerNote*/Photoshop/JFIF)
against the base fixture and marks silent no-ops with noop:true.
"""

import argparse
import json
import os
import shutil
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import tempfile
from pathlib import Path

# Hardened against XXE / entity-expansion; declared as a uv inline dependency
# above (see the "dependencies" script block at the top of this file).
import defusedxml.ElementTree as ET

EXIFTOOL = os.environ.get("EXIFTOOL", "exiftool")
WORK = Path(os.environ.get("TAGMATRIX_WORK")
           or (Path(tempfile.gettempdir()) / "oxidex-tagmap"))
REPO = Path(__file__).resolve().parent.parent
BASE_FIXTURE = REPO / "tests/fixtures/jpeg/tag_matrix_base.jpg"


def _run_exiftool(args, **kwargs):
    """Run exiftool with the given argv tail; the sole subprocess call site.

    List-argv, no shell=True; EXIFTOOL is a local tool path from an env
    var, and every caller below passes only fixed flags or values this
    same script synthesized locally from exiftool's own -listx XML dump
    moments earlier -- never externally-controlled input.
    """
    return subprocess.run([EXIFTOOL, *args], capture_output=True, text=True, **kwargs)  # nosec B603 # nosemgrep: python.lang.security.audit.dangerous-subprocess-use-audit.dangerous-subprocess-use-audit,python.lang.security.audit.dangerous-subprocess-use-tainted-env-args.dangerous-subprocess-use-tainted-env-args


# (listx group arg, family0 bucket, table filter predicate)
SOURCES = [
    ("EXIF", "EXIF", lambda t: t.get("name") in ("Exif::Main", "GPS::Main")),
    ("XMP", "XMP", lambda t: t.get("g0") == "XMP"),
    ("IPTC", "IPTC", lambda t: t.get("name", "").startswith("IPTC::")),
    ("JFIF", "JFIF", lambda t: t.get("name", "").startswith("JFIF::")),
    ("Photoshop", "Photoshop",
     lambda t: t.get("name", "").startswith("Photoshop::")),
    ("ICC_Profile", "ICC_Profile",
     lambda t: t.get("name", "").startswith("ICC_Profile::")),
    # JPEG COM segment: only the Comment tag from the Extra table
    ("File", "File", lambda t: t.get("name") == "Extra"),
]

# (group1, name) -> special sample
OVERRIDES = {
    ("Photoshop", "IPTCDigest"): "new",
    ("Photoshop", "PhotoshopThumbnail"): str(BASE_FIXTURE),
    ("Photoshop", "PhotoshopBGRThumbnail"): str(BASE_FIXTURE),
    ("GPS", "GPSVersionID"): "2.3.0.0",
}
FILE_SAMPLES = {("Photoshop", "PhotoshopThumbnail"),
                ("Photoshop", "PhotoshopBGRThumbnail")}

GPS_SAMPLES = {
    "GPSLatitude": "37.7749",
    "GPSDestLatitude": "37.7749",
    "GPSLatitudeRef": "N",
    "GPSDestLatitudeRef": "N",
    "GPSLongitude": "122.4194",
    "GPSDestLongitude": "122.4194",
    "GPSLongitudeRef": "W",
    "GPSDestLongitudeRef": "W",
    "GPSAltitude": "10.5",
    "GPSDestDistance": "1.5",
    "GPSTimeStamp": "10:30:00",
    "GPSDateStamp": "2024:01:15",
    "GPSDateTime": "2024:01:15 10:30:00",
}

INT_TYPES = {"int8u", "int8s", "int16u", "int16s", "int32u", "int32s", "int64u",
             "int64s", "integer", "digits"}
RAT_TYPES = {"rational32u", "rational32s", "rational64u", "rational64s",
             "rational", "real", "float", "double", "fixed16u", "fixed16s",
             "fixed32u", "fixed32s"}
STRINGISH = {"string", "undef", "?", "var_ustr32", "var_string", "lang-alt",
             "binary"}

DT = "2024:01:15 10:30:00"
D = "2024:01:15"
T = "10:30:00"


def first_en_value(tag_el):
    """First English enum label, preferring a distinctive one over a bare
    "None"/"Unknown" sentinel: those are frequently a tag's own unset
    default, so writing that exact value as the sample makes a genuine
    write indistinguishable from a no-op that left the default untouched
    (harness can no longer tell the two apart by diffing against base)."""
    values = tag_el.find("values")
    if values is None:
        return None
    labels = []
    for key in values.findall("key"):
        for val in key.findall("val"):
            if val.get("lang") == "en":
                labels.append(val.text)
    for label in labels:
        if label not in ("None", "Unknown"):
            return label
    return labels[0] if labels else None


def make_sample(family0, name, vtype, tag_el, group1):
    if (group1, name) in OVERRIDES:
        return OVERRIDES[(group1, name)]
    if name in GPS_SAMPLES:
        return GPS_SAMPLES[name]
    # EXIF undef version tags (ExifVersion, FlashpixVersion, InteropVersion...)
    if family0 == "EXIF" and vtype == "undef" and "Version" in name:
        return "0100"
    if name.startswith("OffsetTime"):  # EXIF 2.31 timezone offset strings
        return "+05:30"
    if vtype == "boolean":
        return "True"
    ev = first_en_value(tag_el)
    if ev is not None:
        return ev
    if vtype == "date":
        return DT
    if vtype == "struct":
        return "{}"
    if vtype in STRINGISH or vtype == "digits":
        if name.startswith("SubSec"):
            return "3"
        if "Date" in name:
            # IPTC splits date and time into separate digit fields
            if family0 == "IPTC" or vtype == "digits":
                return D
            return DT
        if "Time" in name and family0 == "IPTC":
            return T
    if vtype in INT_TYPES or vtype in RAT_TYPES:
        scalar = "3" if vtype in INT_TYPES else "1.5"
        # fixed-count numeric tags need N space-separated values
        try:
            n = int(tag_el.get("count", "1"))
        except ValueError:
            n = 1
        return " ".join([scalar] * n) if n > 1 else scalar
    return "OxTest"


def dump_listx(group):
    """Run exiftool -f -listx for one group, return parsed XML root.

    `group` is one of the fixed strings in SOURCES below.
    """
    out = _run_exiftool(["-f", "-listx", f"-{group}:all"], timeout=300)
    path = WORK / f"listx_{group}.xml"
    path.write_text(out.stdout)
    root = ET.parse(str(path)).getroot()
    if root is None:
        raise RuntimeError(f"empty -listx dump for {group}")
    return root


def flag_noops(manifest, exiftool_ver):
    """Write-test suspect tags on the base fixture; mark silent no-ops.

    `t['sample']` is a value this same script synthesized (a fixed
    literal, or an enum string parsed moments earlier from exiftool's own
    local -listx XML dump) -- not externally supplied.
    """
    suspects = [t for t in manifest["tags"]
                if (t["name"].startswith("MakerNote") and t["family0"] == "EXIF")
                or t["family0"] in ("Photoshop", "JFIF")]
    noop_count = 0
    for t in suspects:
        spec = f"{t['group']}:{t['name']}"
        dst = WORK / "noop_tmp.jpg"
        shutil.copyfile(BASE_FIXTURE, dst)
        op = "<=" if t.get("sample_is_file") else "="
        w = _run_exiftool(["-overwrite_original",
                           f"-{spec}{op}{t['sample']}", str(dst)])
        if w.returncode == 0 and "1 image files updated" in w.stdout:
            t.pop("noop", None)
        else:
            t["noop"] = True
            noop_count += 1
    manifest["noop_note"] = (
        "Tags with noop:true are listed writable by exiftool -listx but were "
        "behaviorally verified to be silent no-ops when written to a bare JPEG "
        f"(exiftool {exiftool_ver}).")
    print(f"flag-noops: {len(suspects)} suspects tested, {noop_count} no-ops")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--flag-noops", action="store_true")
    args = ap.parse_args()

    WORK.mkdir(parents=True, exist_ok=True)
    ver = _run_exiftool(["-ver"]).stdout.strip()
    print(f"exiftool {ver}; work dir {WORK}")

    all_entries = {}  # (group1, name) -> entry
    for group_arg, family0, table_pred in SOURCES:
        root = dump_listx(group_arg)
        for table in root.findall("table"):
            if not table_pred(table):
                continue
            table_g1 = table.get("g1")
            for tag_el in table.findall("tag"):
                name = tag_el.get("name")
                if family0 == "File" and name != "Comment":
                    continue
                g1 = tag_el.get("g1") or table_g1
                if family0 == "File":
                    g1 = "File"
                vtype = tag_el.get("type", "?")
                writable = tag_el.get("writable") == "true"
                flags = tag_el.get("flags", "")
                flagset = set(flags.split(",")) if flags else set()
                protected = bool(flagset & {"Protected", "Unsafe", "Avoid"})
                entry = {
                    "group": g1,
                    "name": name,
                    "family0": family0,
                    "writable": writable,
                    "type": vtype,
                    "protected": protected,
                }
                if flags:
                    entry["flags"] = flags
                count_attr = tag_el.get("count")
                if count_attr:
                    try:
                        entry["count"] = int(count_attr)
                    except ValueError:
                        pass  # non-numeric count (e.g. "?"); leave count unset
                if writable:
                    entry["sample"] = make_sample(family0, name, vtype, tag_el,
                                                  g1)
                    if (g1, name) in FILE_SAMPLES:
                        entry["sample_is_file"] = True  # write as -TAG<=file
                key = (g1, name)
                prev = all_entries.get(key)
                if prev is None:
                    all_entries[key] = entry
                else:
                    # prefer writable over not, then non-protected
                    prev_rank = (prev["writable"], not prev["protected"])
                    new_rank = (entry["writable"], not entry["protected"])
                    if new_rank > prev_rank:
                        all_entries[key] = entry

    entries = sorted(all_entries.values(),
                     key=lambda e: (e["family0"], e["group"], e["name"]))
    writable_tags = [e for e in entries if e["writable"]]
    readonly_tags = [{"group": e["group"], "name": e["name"],
                      "family0": e["family0"], "type": e["type"]}
                     for e in entries if not e["writable"]]

    groups = {}
    for e in entries:
        g = groups.setdefault(e["family0"], {"writable": 0, "readonly": 0,
                                             "protected_writable": 0})
        if e["writable"]:
            g["writable"] += 1
            if e["protected"]:
                g["protected_writable"] += 1
        else:
            g["readonly"] += 1

    manifest = {
        "generated_by": f"exiftool {ver}",
        "description": ("ExifTool tags writable in JPEG files (testable "
                        "universe for a read/write support matrix). "
                        "group = ExifTool family-1 group."),
        "groups": groups,
        "tag_count": len(writable_tags),
        "tags": writable_tags,
    }
    if args.flag_noops:
        flag_noops(manifest, ver)

    (WORK / "exiftool_jpeg_tags.json").write_text(json.dumps(manifest, indent=1))
    readonly = {
        "generated_by": f"exiftool {ver}",
        "description": ("JPEG-relevant ExifTool tags that are read-only "
                        "(writable=false); not testable via synthesis."),
        "tag_count": len(readonly_tags),
        "tags": readonly_tags,
    }
    (WORK / "exiftool_jpeg_readonly_tags.json").write_text(
        json.dumps(readonly, indent=1))

    print(f"{'family0':<12} {'writable':>8} {'(protected)':>11} {'readonly':>8}")
    for g, c in sorted(groups.items()):
        print(f"{g:<12} {c['writable']:>8} {c['protected_writable']:>11} "
              f"{c['readonly']:>8}")
    print(f"{'TOTAL':<12} "
          f"{sum(c['writable'] for c in groups.values()):>8} "
          f"{sum(c['protected_writable'] for c in groups.values()):>11} "
          f"{sum(c['readonly'] for c in groups.values()):>8}")


if __name__ == "__main__":
    main()
