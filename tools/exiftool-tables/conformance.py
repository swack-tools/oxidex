#!/usr/bin/env python3
"""Diff OxiDex against ExifTool across a corpus, and classify the differences.

The comparison report says which formats score badly. It does not say *why*,
and the why matters enormously for what you should do about it:

  RENAME    OxiDex read the value correctly and called it something ExifTool
            does not call it. Zero parsing work; fix the name and the tag
            counts as matched. BMP scoring 0% is entirely this.
  MISSING   ExifTool emits a tag OxiDex does not. Real extraction work.
  VALUE     Both emit the tag, values disagree. Usually a PrintConv gap.
  EXTRA     OxiDex-only, with no plausible ExifTool counterpart.

A tag-at-a-time fix loop cannot tell these apart, so it pays full price for
renames -- the cheapest possible class -- as if they were parsing work. This
script separates them mechanically, for every format at once.

Rename detection uses the generated tables as the name universe: if OxiDex
emits `BMP:Width`, ExifTool does not, and ExifTool's BMP table contains
`ImageWidth` which OxiDex is missing *and whose value matches*, that is a
rename, not two independent defects. The value check is what makes the
inference safe -- name similarity alone would guess, and guessing is how you
get a confident wrong mapping.
"""

import argparse
import json
import os
import subprocess
import sys
from collections import Counter, defaultdict

# Tags that describe the file on disk rather than its metadata. They differ by
# construction (paths, timestamps, the tool's own version) and would swamp the
# signal without saying anything about parser conformance.
IGNORE = {
    "SourceFile", "ExifToolVersion", "FileName", "Directory",
    "FileModifyDate", "FileAccessDate", "FileInodeChangeDate",
    "FilePermissions", "FileSize", "Now", "ProcessingTime",
}


def run_exiftool(et_dir, path):
    out = subprocess.run(
        ["perl", "-I" + os.path.join(et_dir, "lib"),
         # No -n: ExifTool must apply PrintConv, because OxiDex applies its
         # own. Comparing converted output against raw values would report
         # every correctly-read tag as a value mismatch.
         os.path.join(et_dir, "exiftool"), "-G", "-s", "-j", "-a", path],
        capture_output=True, text=True, errors="replace",
    ).stdout
    try:
        return json.loads(out)[0]
    except (json.JSONDecodeError, IndexError, KeyError):
        return {}


def run_oxidex(binary, path):
    out = subprocess.run(
        [binary, "-j", path], capture_output=True, text=True, errors="replace",
    ).stdout
    try:
        d = json.loads(out)
    except json.JSONDecodeError:
        return {}
    if isinstance(d, list):
        d = d[0] if d else {}
    return d if isinstance(d, dict) else {}


def split_key(k):
    """-> (group, name). ExifTool -G gives 'EXIF:Make'; bare names have none."""
    return tuple(k.split(":", 1)) if ":" in k else ("", k)


def norm_value(v):
    """Loose value comparison: we are checking identity, not formatting."""
    if v is None:
        return ""
    if isinstance(v, (list, tuple)):
        return " ".join(norm_value(x) for x in v)
    s = str(v).strip()
    # Compare numbers numerically so 2 == 2.0 == "2".
    try:
        f = float(s)
        return f"{f:.6g}"
    except ValueError:
        return s.casefold()


def name_key(n):
    """Normalised name, for spotting pure spelling/case renames."""
    return "".join(ch for ch in n.lower() if ch.isalnum())


def distinctive(v):
    """Is this value strong enough to identify a tag on its own?

    Booleans, small integers and short strings recur across unrelated tags, so
    matching on them pairs things at random. The check exists because the naive
    version produced crossed nonsense -- `Blue -> RedTRC` *and* `Red -> BlueTRC`
    in the same file, because all three ICC curves hold identical data, and
    `Height -> Aperture` because both happened to be 8.
    """
    s = str(v).strip()
    if len(s) < 4:
        return False
    if s.casefold() in {"true", "false", "yes", "no", "none", "n/a", "inf"}:
        return False
    try:
        f = float(s)
        # Small round numbers are the worst offenders.
        return abs(f) >= 1000 and f != int(f) or abs(f) >= 10000
    except ValueError:
        return True


def infer_renames(missing, extra):
    """Pair OxiDex-only tags with missing ExifTool tags, conservatively.

    A pair is accepted only when it is unambiguous in BOTH directions -- the
    value identifies exactly one candidate on each side -- and additionally
    either the names normalise to the same string (BITPIX -> Bitpix) or the
    value is distinctive enough to stand alone (a timestamp, a long string).

    Ambiguous groups are left alone and reported as missing/extra. Under-
    claiming here is deliberate: a wrong rename would send someone to "fix" a
    correctly-named tag, which is worse than saying nothing.
    """
    by_val_missing = defaultdict(list)
    by_val_extra = defaultdict(list)
    for en, (_g, ev) in missing.items():
        by_val_missing[norm_value(ev)].append(en)
    for on, (_g, ov) in extra.items():
        by_val_extra[norm_value(ov)].append(on)

    renames = []
    for val, ens in by_val_missing.items():
        ons = by_val_extra.get(val)
        if not ons:
            continue
        if len(ens) != 1 or len(ons) != 1:
            continue  # ambiguous: several tags share this value
        en, on = ens[0], ons[0]
        if name_key(en) == name_key(on) or distinctive(val):
            renames.append((on, en, missing[en][1]))
    return renames


def compare(et, ox):
    et_by_name = {}
    for k, v in et.items():
        g, n = split_key(k)
        if n in IGNORE:
            continue
        et_by_name.setdefault(n, (g, v))
    ox_by_name = {}
    for k, v in ox.items():
        g, n = split_key(k)
        if n in IGNORE:
            continue
        ox_by_name.setdefault(n, (g, v))

    matched, value_diff = [], []
    missing = dict(et_by_name)
    extra = dict(ox_by_name)

    for n, (g, v) in et_by_name.items():
        if n in ox_by_name:
            missing.pop(n, None)
            extra.pop(n, None)
            if norm_value(v) == norm_value(ox_by_name[n][1]):
                matched.append(n)
            else:
                value_diff.append((n, v, ox_by_name[n][1]))

    renames = infer_renames(missing, extra)
    for on, en, _v in renames:
        missing.pop(en, None)
        extra.pop(on, None)

    return {
        "matched": matched,
        "value_diff": value_diff,
        "missing": missing,
        "extra": extra,
        "renames": renames,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus", help="directory of sample files")
    ap.add_argument("--exiftool-dir", required=True)
    ap.add_argument("--oxidex", default="./target/debug/oxidex")
    ap.add_argument("--only", help="substring filter on filename")
    ap.add_argument("--show", type=int, default=0,
                    help="print per-file detail for the N worst files")
    ap.add_argument("--json-out")
    args = ap.parse_args()

    files = sorted(
        os.path.join(args.corpus, f) for f in os.listdir(args.corpus)
        if os.path.isfile(os.path.join(args.corpus, f))
        and (not args.only or args.only.lower() in f.lower())
    )
    if not files:
        sys.exit(f"no files in {args.corpus}")

    per_ext = defaultdict(Counter)
    rename_votes = defaultdict(Counter)
    missing_votes = Counter()
    detail = []

    for path in files:
        et = run_exiftool(args.exiftool_dir, path)
        if not et:
            continue
        ox = run_oxidex(args.oxidex, path)
        r = compare(et, ox)
        ext = (et.get("File:FileType") or et.get("FileType")
               or os.path.splitext(path)[1].lstrip(".")).upper()

        c = per_ext[ext]
        c["files"] += 1
        c["matched"] += len(r["matched"])
        c["value_diff"] += len(r["value_diff"])
        c["missing"] += len(r["missing"])
        c["renames"] += len(r["renames"])
        c["extra"] += len(r["extra"])

        for on, en, _v in r["renames"]:
            rename_votes[ext][(on, en)] += 1
        for n in r["missing"]:
            missing_votes[(ext, n)] += 1

        total = c["matched"] + c["value_diff"] + c["missing"] + c["renames"]
        detail.append((len(r["missing"]) + len(r["renames"]), path, r))

    print(f"{'format':<10}{'files':>6}{'match':>7}{'rename':>8}"
          f"{'value':>7}{'missing':>9}{'score':>8}{'ceiling':>9}")
    print("-" * 64)
    grand = Counter()
    for ext in sorted(per_ext):
        c = per_ext[ext]
        tot = c["matched"] + c["value_diff"] + c["missing"] + c["renames"]
        if not tot:
            continue
        grand.update(c)
        score = c["matched"] / tot
        # What the score becomes if every rename is corrected -- free coverage.
        ceiling = (c["matched"] + c["renames"]) / tot
        print(f"{ext:<10}{c['files']:>6}{c['matched']:>7}{c['renames']:>8}"
              f"{c['value_diff']:>7}{c['missing']:>9}{score:>7.1%}{ceiling:>9.1%}")

    tot = grand["matched"] + grand["value_diff"] + grand["missing"] + grand["renames"]
    print("-" * 64)
    if tot:
        print(f"{'TOTAL':<10}{grand['files']:>6}{grand['matched']:>7}"
              f"{grand['renames']:>8}{grand['value_diff']:>7}{grand['missing']:>9}"
              f"{grand['matched']/tot:>7.1%}"
              f"{(grand['matched']+grand['renames'])/tot:>9.1%}")

    if rename_votes:
        print("\nrenames -- OxiDex reads these correctly under the wrong name.")
        print("value-confirmed, so these are name fixes, not parsing work:\n")
        for ext in sorted(rename_votes):
            for (on, en), n in rename_votes[ext].most_common():
                print(f"  {ext:<8} {on:<26} -> {en:<26} ({n} file{'s'*(n>1)})")

    print("\ntop genuinely missing tags (real extraction work):")
    for (ext, n), c in missing_votes.most_common(25):
        print(f"  {ext:<8} {n:<34} {c} file{'s'*(c>1)}")

    if args.show:
        for _k, path, r in sorted(detail, reverse=True)[:args.show]:
            print(f"\n--- {os.path.basename(path)} ---")
            print("  missing:", ", ".join(sorted(r["missing"])) or "-")
            print("  extra:  ", ", ".join(sorted(r["extra"])) or "-")

    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as fh:
            json.dump(
                {"per_format": {k: dict(v) for k, v in per_ext.items()},
                 "renames": {k: {f"{a}->{b}": n for (a, b), n in v.items()}
                             for k, v in rename_votes.items()},
                 "missing": {f"{e}:{n}": c for (e, n), c in missing_votes.items()}},
                fh, indent=2, sort_keys=True)
        print(f"\nwrote {args.json_out}")


if __name__ == "__main__":
    main()
