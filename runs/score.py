#!/usr/bin/env python3
"""Per-file parity scorer: exiftool -a -G1 -s  vs  oxidex -e -a -r.

Keys are Group:Name. Scores each file independently and sums.
"""
import sys, os, re, json
from collections import defaultdict

ROOT = "/tmp/oxidex-exiftool-cache/combined-samples/"


def norm(p):
    p = p.strip()
    if p.startswith("./"):
        p = p[2:]
    if p.startswith(ROOT):
        p = p[len(ROOT):]
    return p


def parse_et(path):
    """exiftool -a -G1 -s -r output -> {file: {Group:Name: value}}"""
    out = {}
    cur = None
    for line in open(path, encoding="utf-8", errors="replace"):
        line = line.rstrip("\n")
        if line.startswith("======== "):
            cur = norm(line[len("======== "):])
            out[cur] = defaultdict(list)
            continue
        if cur is None:
            continue
        # format: "[Group1]      TagName    : value"
        m = re.match(r"^\[([^\]]+)\]\s+(\S+)\s*:\s?(.*)$", line)
        if not m:
            continue
        g, n, v = m.group(1), m.group(2), m.group(3)
        out[cur][f"{g}:{n}"].append(v.strip())
    return {f: dict(d) for f, d in out.items()}


def parse_ox(path):
    """oxidex -e -a -r output -> {file: {Group:Name: value}}"""
    out = {}
    cur = None
    for line in open(path, encoding="utf-8", errors="replace"):
        line = line.rstrip("\n")
        if line.startswith("File: "):
            cur = norm(line[len("File: "):])
            out[cur] = defaultdict(list)
            continue
        if cur is None:
            continue
        if ": " not in line and not line.endswith(":"):
            continue
        k, _, v = line.partition(": ")
        k = k.strip()
        if ":" not in k:
            continue
        out[cur][k].append(v.strip())
    return {f: dict(d) for f, d in out.items()}


def score(et, ox):
    tot = dict(matched=0, missing=0, extra=0, valdiff=0, files=0)
    per_file = {}
    detail_valdiff = defaultdict(list)
    detail_missing = defaultdict(list)
    for f, etags in et.items():
        oxtags = ox.get(f, {})
        m = mi = ex = vd = 0
        for k, evals in etags.items():
            if k not in oxtags:
                mi += 1
                detail_missing[k].append(f)
            else:
                # multi-valued: compare as multisets of strings
                if sorted(oxtags[k]) == sorted(evals):
                    m += 1
                elif set(oxtags[k]) & set(evals):
                    m += 1
                else:
                    vd += 1
                    detail_valdiff[k].append((f, evals[0], oxtags[k][0]))
        for k in oxtags:
            if k not in etags:
                ex += 1
        tot["matched"] += m
        tot["missing"] += mi
        tot["extra"] += ex
        tot["valdiff"] += vd
        tot["files"] += 1
        per_file[f] = (m, mi, ex, vd)
    return tot, per_file, detail_valdiff, detail_missing


if __name__ == "__main__":
    etf, oxf = sys.argv[1], sys.argv[2]
    et = parse_et(etf)
    ox = parse_ox(oxf)
    tot, per_file, dvd, dmiss = score(et, ox)
    print(f"exiftool files: {len(et)}   oxidex files: {len(ox)}")
    print(f"scored files:   {tot['files']}")
    print(f"matched:        {tot['matched']}")
    print(f"missing:        {tot['missing']}")
    print(f"value diffs:    {tot['valdiff']}")
    print(f"extra:          {tot['extra']}")
    denom = tot["matched"] + tot["missing"] + tot["valdiff"]
    print(f"coverage:       {100.0*tot['matched']/denom:.2f}%" if denom else "n/a")
    outp = sys.argv[3] if len(sys.argv) > 3 else None
    if outp:
        json.dump({
            "totals": tot,
            "per_file": per_file,
            "valdiff_by_tag": {k: len(v) for k, v in sorted(dvd.items(), key=lambda x: -len(x[1]))},
            "missing_by_tag": {k: len(v) for k, v in sorted(dmiss.items(), key=lambda x: -len(x[1]))},
            "valdiff_examples": {k: v[:3] for k, v in dvd.items()},
        }, open(outp, "w"), indent=1)
        print(f"wrote {outp}")
