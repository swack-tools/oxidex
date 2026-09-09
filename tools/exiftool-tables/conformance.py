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

Matching is group-qualified: a same-named tag from two different groups is
only paired across groups when the value matches (a harmless group alias) or
the name was unique on both sides to begin with. Bare-name comparison is
group-blind, and group-blind comparison is not free of cost -- on a single-
file APE.mpc corpus, comparing every OxiDex `MPC:*`/`APE:*`/`ID3:*`/`ID3v1:*`
tag against every ExifTool tag sharing its bare name manufactured 10 false
VALUE diffs and one false cross-group MATCH out of tags that, group-
qualified, are 11 `MPC:*` MISSING, 11 `APE:*` MISSING, `ID3v1:*` EXTRA (Oxi-
Dex reads the trailer ExifTool's JSON writer drops when ID3v2 outranks it),
and zero VALUE. See test_conformance.py for the pinned regression.

EXTRA is a precision axis, not a recall penalty: it is reported (with a
`precision` column and an EXTRA vote table) but never enters the score/
ceiling denominator, so a format cannot buy a better score by inventing tags
and is not punished on recall for genuinely extra ones -- later stages
budget it explicitly (Step 21's default-mode EXTRA-budget gate reads this).

Real VALUE differences are further classed by severity -- identity,
structural, numeric, date_time, binary, display_only -- so a PrintConv
rounding nit doesn't read the same as a wrong decode.

The two sides' keys are split by two different rules, on purpose. The
oracle is asked for `-G0:1` keys ('EXIF:IFD0:Make'), and split_oracle_key
takes the FIRST segment as the reporting group and the LAST as the name --
ExifTool tag names never contain ':', so the middle segments are structure
only. OxiDex keys go through split_oxidex_key: first segment is the group,
EVERYTHING after it is the name, exactly the rule the plain -G era used. The
distinction matters because OxiDex emits keys whose middle segment is part
of the name ('OOXML:Custom:Division', 'PNG:tEXt:comment'); one shared
last-segment rule silently rewrote those to 'Division'/'comment' and
matched them, masking a name defect that is OxiDex's to fix.

Unmatched occurrences are reported one row each. Under -G0:1 two leftover
oracle rows from different family-1 groups ('EXIF:IFD0:Compression',
'EXIF:IFD1:Compression') spell the same family-0 report key, so the 2nd,
3rd... occurrence of a key carries ' (2)', ' (3)' rather than overwriting
the first -- see place_occurrence. MISSING/EXTRA are counts of occurrences,
not of distinct keys.
"""

import argparse
import json
import math
import os
import re
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

# The single resolution point for "which ExifTool are we grading against".
# Kept in scripts/ so the Rust harnesses, the fleet scripts and this tool all
# answer that question the same way. instrument answers the other half --
# which oxidex, from what commit, over what corpus -- see its module doc.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import exiftool_oracle  # noqa: E402
import instrument  # noqa: E402

# Tags that describe the file on disk rather than its metadata. They differ by
# construction (paths, timestamps, the tool's own version) and would swamp the
# signal without saying anything about parser conformance.
IGNORE = {
    "SourceFile", "ExifToolVersion", "FileName", "Directory",
    "FileModifyDate", "FileAccessDate", "FileInodeChangeDate",
    "FilePermissions", "FileSize", "Now", "ProcessingTime",
}


def run_exiftool(oracle, path):
    out = subprocess.run(
        # No -n: ExifTool must apply PrintConv, because OxiDex applies its
        # own. Comparing converted output against raw values would report
        # every correctly-read tag as a value mismatch.
        #
        # -G0:1, not -G: ExifTool's JSON writer keeps ONE entry per key, and
        # under family-0 keys every MPF sub-image (MPImage1/2/3), every
        # IFD0/IFD1 pair and every repeated XMP block share a key, so all but
        # one occurrence vanish before this script ever sees them -- in both
        # directions: OxiDex's correct rows score EXTRA against a partner
        # that was dropped, and oracle rows OxiDex lacks are never counted
        # MISSING. Measured on combined-samples/Apple/Apple_iPhone11.jpg
        # (three embedded MPF images) with the pinned 13.59: `-G -s -j -a`
        # prints 7 MPImage keys, `-G0:1 -s -j -a` prints 23, and the text
        # form `-G0:1 -s -a` (one row per occurrence) prints 23 MPImage rows;
        # `oxidex -j` prints 23. The family-1 segment carries the structure
        # OxiDex's own -j output already prints, so the keys stop colliding;
        # split_oracle_key still reports the family-0 segment as the group,
        # so every report and every --json-out consumer sees the same group
        # strings it saw before. What -G0:1 still collapses -- a repeat inside ONE
        # family-1 group -- is family 4's job (`-G0:1:4`, which names the
        # copies 'Copy1', 'Copy2'...); duplicate_loss_scan.py's module doc
        # measures that -G1 / -G1:4 distinction and is the reference for it.
        oracle.command(["-G0:1", "-s", "-j", "-a", path]),
        capture_output=True, text=True, errors="replace",
    ).stdout
    try:
        # parse_float=str: the default turns ExifTool's "1.80" into 1.8, and
        # the harness then reports a value difference against byte-identical
        # OxiDex output. Rediscovered as a "bug" five separate times here.
        return json.loads(out, parse_float=str)[0]
    except (json.JSONDecodeError, IndexError, KeyError):
        return {}


def run_oxidex(binary, path):
    out = subprocess.run(
        [binary, "-j", path], capture_output=True, text=True, errors="replace",
    ).stdout
    try:
        # Same parse_float reasoning as run_exiftool: both sides must round-trip
        # their numbers identically or the comparison invents differences.
        d = json.loads(out, parse_float=str)
    except json.JSONDecodeError:
        return {}
    if isinstance(d, list):
        d = d[0] if d else {}
    return d if isinstance(d, dict) else {}


def split_oracle_key(k):
    """-> (group, name) for an ExifTool JSON key. ORACLE side only.

    ExifTool -G0:1 gives 'EXIF:IFD0:Make' (family 0, family 1, name) or --
    when the two families coincide -- just 'File:FileType'; with family 4
    asked for as well, 'EXIF:IFD0:Copy1:Make'. Bare names have no group.
    The reporting GROUP is the first ':'-segment (family 0), so every report
    row and every --json-out consumer sees the group strings it saw under
    plain -G; the NAME is the last segment. The middle segments exist only
    to keep the oracle's JSON keys distinct (its writer keeps one entry per
    key). ExifTool tag names never contain ':' (audited over the 4,238-file
    census at 25a2109e: zero oracle keys with a colon inside the name), so
    last-segment is exact on THIS side. It is wrong for OxiDex keys, which
    can carry ':' inside the name -- those go through split_oxidex_key.
    """
    if ":" not in k:
        return ("", k)
    segs = k.split(":")
    return (segs[0], segs[-1])


def split_oxidex_key(k):
    """-> (group, name) for an `oxidex -j` key: first segment, then the rest.

    OxiDex prints 'IFD0:Make' (its group, then the name) -- but it also
    prints keys whose middle segment is part of the NAME, not a group:
    ooxml.rs emits 'OOXML:Custom:<property>' (26 keys on t/images/OOXML.docx
    under the 25a2109e binary) and png_writer.rs 'PNG:<chunk>:<keyword>'
    ('PNG:tEXt:comment' on PNG.png). ExifTool spells those 'XML:Division'
    and 'PNG:Comment', so the colon-in-name is a defect the census must keep
    exposing: with the pre-f3b5f5e6 first-segment rule, kept here verbatim,
    'Custom:Division' stays a distinct name and lands in EXTRA (or RENAME)
    beside the oracle's MISSING 'Division'. Applying the oracle's last-
    segment rule to this side -- which f3b5f5e6 did -- silently rewrote the
    name to 'Division' and reported a match.
    """
    return tuple(k.split(":", 1)) if ":" in k else ("", k)


def file_type(et):
    """ExifTool's File:FileType, whatever key shape the group flags gave it.

    Under -G0:1 the key is 'File:FileType' (family 1 == family 0, so ExifTool
    prints one segment), not 'File:File:FileType' -- but per_format must not
    depend on that quirk, so the lookup goes through split_oracle_key.
    """
    for k, v in et.items():
        if split_oracle_key(k) == ("File", "FileType"):
            return v
    return et.get("FileType")


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
        # Infinities parse ("float('-inf')" succeeds for -inf, Infinity,
        # +inf...) but int(f) below raises OverflowError, and an infinite
        # reading identifies nothing anyway -- same verdict as "inf" above.
        # NaN (int() -> ValueError) matches nothing including itself.
        if not math.isfinite(f):
            return False
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


def tags_by_name(tags, split):
    """Collect every occurrence of a tag name, retaining its group and value.

    `split` is the side's own key rule -- split_oracle_key for ExifTool
    output, split_oxidex_key for `oxidex -j` -- and is deliberately not
    defaulted: the two rules differ on exactly the keys where it matters
    (see split_oxidex_key), so a caller must say which side it is holding.

    Group names normally should not affect a comparison: OxiDex deliberately
    normalises a number of ExifTool groups.  But names such as ``CreateDate``
    and ``XResolution`` can occur in more than one group in one file.  Keeping
    every occurrence lets ``compare`` match their values before deciding that
    a pair is a value difference.
    """
    by_name = defaultdict(list)
    for k, v in tags.items():
        g, n = split(k)
        if n in IGNORE:
            continue
        by_name[n].append((g, v))
    return by_name


def occurrence_name(group, name, duplicate):
    """Report key for one unmatched occurrence.

    'Group:Name' when the name occurs more than once on either side of this
    file, bare 'Name' when it is unique. `group` is the reporting group --
    family 0 on the oracle side -- so under -G0:1 two leftover oracle rows
    from different family-1 groups ('EXIF:IFD0:Compression' and
    'EXIF:IFD1:Compression') spell the SAME key here; place_occurrence keeps
    them apart.
    """
    return f"{group}:{name}" if duplicate else name


def place_occurrence(rows, key, group, value):
    """Add one leftover occurrence to `rows` without overwriting an earlier one.

    The 2nd, 3rd... occurrence of a report key is stored as 'key (2)',
    'key (3)'. Before this, the dict write was last-writer-wins, so N
    unmatched oracle occurrences sharing a family-0 key survived as ONE
    MISSING row (DNG.dng: 'EXIF:Compression' kept IFD1's 'JPEG' and lost
    IFD0's 'Uncompressed'; 187 occurrences lost over a 200-file format-
    breadth subset, 969 leftovers -> 968 rows on a JPEG subset), so MISSING
    was a lower bound. Suffixing -- rather than carrying the family-1 group
    in the key -- keeps every key that existed before spelled exactly as it
    was, so per_file.missing/.extra keys stay comparable across runs and the
    'FMT:key' vote tables keep their shape; only a formerly-lost row gets a
    new key. The first occurrence in oracle key order (ExifTool's extraction
    order: IFD0 before IFD1) keeps the bare key. Applied to both sides for
    symmetry; an `oxidex -j` dict cannot actually collide, since its key IS
    'group:name'.
    """
    if key not in rows:
        rows[key] = (group, value)
        return key
    n = 2
    while f"{key} ({n})" in rows:
        n += 1
    rows[f"{key} ({n})"] = (group, value)
    return f"{key} ({n})"


_DATE_RE = re.compile(r"^\d{4}[:\-]\d{2}[:\-]\d{2}([ T]\d{2}:\d{2}:\d{2})?")


def classify_severity(expected, actual):
    """Bucket a genuine VALUE difference by what kind of gap it represents.

    This runs only after matching/missing/extra are already decided -- it
    never changes a count, it labels the value_diff pairs so a reviewer (or
    a later CI gate, see Stage 4's output-mode matrix) can tell a PrintConv
    rounding nit from a wrong decode without opening every file by hand.

    Classes: identity (same string modulo case/whitespace -- a formatting
    nit), date_time, binary (a "(Binary data N bytes...)" placeholder on
    either side), numeric (both sides parse as numbers but disagree),
    display_only (one side is a substring of the other -- usually a
    PrintConv applied on one side only, e.g. "5" vs "5 (Standard)"), and
    structural (the fallback: genuinely different data).
    """
    e, a = str(expected), str(actual)
    if _DATE_RE.match(e) or _DATE_RE.match(a):
        return "date_time"
    if "Binary data" in e or "Binary data" in a:
        return "binary"
    if e.casefold().strip() == a.casefold().strip():
        return "identity"
    try:
        float(e)
        float(a)
        return "numeric"
    except ValueError:
        pass
    e_norm, a_norm = e.casefold(), a.casefold()
    if e_norm and a_norm and (e_norm in a_norm or a_norm in e_norm):
        return "display_only"
    return "structural"


def _match_bucket(_name, expected, actual):
    """Resolve one tag-name bucket (all ET and OxiDex occurrences of `name`
    in one file) into matched / value_diff / leftover-missing / leftover-
    extra, group-qualified.

    Four tiers, most specific evidence first, each consuming what it pairs
    so a later tier never re-considers an already-decided occurrence:

      1. Exact group AND exact value.  Unambiguous under any circumstance.
      2. Exact value, any group.  A harmless group alias (OxiDex normalises
         a number of ExifTool's group names) still identifies the same
         underlying data, so this stays group-blind -- but only as a value-
         confirmed pairing, never a blind position grab.
      3. Exact group, differing value.  The strongest evidence of a real
         PrintConv/value discrepancy once (1)/(2) found nothing.
      4. Last resort: pair whatever is left, but ONLY when the tag name was
         unique on both sides to begin with (exactly one ET occurrence, one
         OxiDex occurrence). This is the fix for the APE.mpc cascade: the
         old code punted here whenever *anything* remained on the OxiDex
         side, regardless of how many candidates were competing, and that
         is what let an unrelated MPC:*/APE:*/ID3v1:* value get pulled in
         and reported as a false VALUE diff (or, when the value happened to
         coincide, a false MATCH) against a completely different real tag.
         When the name occurs more than once on either side, an unresolved
         leftover is reported as MISSING + EXTRA instead of a guessed pair
         -- under-claiming a defect classification is deliberate here, the
         same principle infer_renames() below already applies to renames.
    """
    original_expected_n = len(expected)
    original_actual_n = len(actual)
    duplicate = original_expected_n > 1 or original_actual_n > 1

    remaining_actual = list(actual)
    matched_pairs = []   # (group, value) from the ET side, for `matched`
    diff_pairs = []      # (e_group, e_value, a_group, a_value)

    # Tier 1: exact group + exact value.
    still = []
    for g, v in expected:
        idx = next(
            (i for i, (og, ov) in enumerate(remaining_actual)
             if og == g and norm_value(ov) == norm_value(v)),
            None,
        )
        if idx is not None:
            remaining_actual.pop(idx)
            matched_pairs.append((g, v))
        else:
            still.append((g, v))
    expected = still

    # Tier 2: exact value, any group (harmless alias tolerance).
    still = []
    for g, v in expected:
        idx = next(
            (i for i, (_og, ov) in enumerate(remaining_actual)
             if norm_value(ov) == norm_value(v)),
            None,
        )
        if idx is not None:
            remaining_actual.pop(idx)
            matched_pairs.append((g, v))
        else:
            still.append((g, v))
    expected = still

    # Tier 3: exact group, value differs.
    still = []
    for g, v in expected:
        idx = next(
            (i for i, (og, _ov) in enumerate(remaining_actual) if og == g),
            None,
        )
        if idx is not None:
            og, ov = remaining_actual.pop(idx)
            diff_pairs.append((g, v, og, ov))
        else:
            still.append((g, v))
    expected = still

    # Tier 4: cross-group punt, gated on name-uniqueness (see docstring).
    if (original_expected_n == 1 and original_actual_n == 1
            and expected and remaining_actual):
        g, v = expected.pop(0)
        og, ov = remaining_actual.pop(0)
        diff_pairs.append((g, v, og, ov))

    return matched_pairs, diff_pairs, expected, remaining_actual, duplicate


def compare(et, ox):
    et_by_name = tags_by_name(et, split_oracle_key)
    ox_by_name = tags_by_name(ox, split_oxidex_key)

    matched, value_diff = [], []
    missing, extra = {}, {}

    for n in et_by_name.keys() | ox_by_name.keys():
        expected = et_by_name.get(n, [])
        actual = list(ox_by_name.get(n, []))

        matched_pairs, diff_pairs, leftover_expected, leftover_actual, duplicate = (
            _match_bucket(n, expected, actual)
        )

        matched.extend(n for _g, _v in matched_pairs)
        for _g, v, _og, ov in diff_pairs:
            value_diff.append((n, v, ov, classify_severity(v, ov)))
        for g, v in leftover_expected:
            place_occurrence(missing, occurrence_name(g, n, duplicate), g, v)
        for g, v in leftover_actual:
            place_occurrence(extra, occurrence_name(g, n, duplicate), g, v)

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


# --- Step 13 seam ---------------------------------------------------------
# Step 13 adds ReadReport (Parsed|Partial|IdentifiedOnly|Unsupported) to
# OxiDex's own output -- a machine-readable parse-status distinct from "did
# it emit any tags at all" (see AGENTS.md: "detected is not parsed"). Once
# that lands, this hook stops returning None and starts feeding a genuine
# IdentifiedOnly-per-format count into the report and into --json-out, so a
# format that only ever emits identity tags stops being invisible in the
# conformance table. Deliberately unimplemented here -- Step 13 owns it.
def parser_status(_path, _ox):
    return None


# --- Stage 4 seam ----------------------------------------------------------
# Stage 4 gives OxiDex a TagOccurrence store carrying family-0/1 group
# identity per occurrence. Once that exists, this hook can return a
# family-0 "ExifTool-compatible" view and a family-1 "OxiDex-structural"
# view of one file's comparison, so this instrument reports both without
# another rewrite of compare(). Deliberately unimplemented here -- Stage 4
# owns it.
def family_views(_et, _ox):
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus", nargs="+",
                    help="one or more directories of sample files. Multiple "
                         "roots are scored as a single corpus, because no one "
                         "tree covers everything: ExifTool's own t/images is "
                         "the format-breadth corpus (~126 formats, pinned and "
                         "cloned in CI), while tests/fixtures carries the "
                         "OxiDex-specific samples. Duplicate paths across "
                         "roots are scored once.")
    ap.add_argument("--exiftool-dir",
                    help="ExifTool checkout root; defaults to the pinned tree "
                         "resolved by scripts/exiftool_oracle.py")
    ap.add_argument("--oxidex", default="./target/debug/oxidex")
    ap.add_argument("--only", help="substring filter on filename")
    ap.add_argument("--ext",
                    help="comma-separated extension allow-list, e.g. "
                         "'jpg,tif,png'. For narrowing a run to one format "
                         "while debugging. Default is no filter -- prefer "
                         "--exclude-ext for corpus-wide runs, so that a newly "
                         "added format is scored without editing anything.")
    ap.add_argument("--exclude-ext",
                    help="comma-separated extension deny-list, e.g. "
                         "'sh,md,py,json'. Corpora that double as test-fixture "
                         "trees carry harness scaffolding -- mock .sh scripts, "
                         ".json baselines, .md notes -- and ExifTool happily "
                         "scores those as ENV SCRIPT/JSON, whose 'tags' are "
                         "just object keys. tests/fixtures alone scores 83.8%% "
                         "with them and 96%%+ without.\n"
                         "Prefer this over --ext for corpus-wide runs: a deny-"
                         "list of things that are never metadata keeps scoring "
                         "every real format, including ones added later, "
                         "whereas an allow-list silently omits each new format "
                         "until someone remembers to extend it.")
    ap.add_argument("--recursive", action="store_true",
                    help="walk the corpus recursively (most sample corpora are "
                         "nested one directory per manufacturer)")
    ap.add_argument("--min-files", type=int, default=1,
                    help="fail if fewer files than this were scored")
    ap.add_argument("--min-tags", type=int, default=1,
                    help="fail if fewer ExifTool tags than this were seen")
    ap.add_argument("--show", type=int, default=0,
                    help="print per-file detail for the N worst files")
    ap.add_argument("--json-out")
    args = ap.parse_args()

    # Resolve every instrument before reading a single file. A run that
    # cannot say which ExifTool, which oxidex, and from what commit it
    # graded should not go on to report a number.
    try:
        oracle = (exiftool_oracle.resolve_tree(args.exiftool_dir)
                  if args.exiftool_dir else exiftool_oracle.shared())
    except exiftool_oracle.OracleError as exc:
        sys.exit(f"❌ {exc}")

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "conformance.py")
    binary = instrument.resolve_binary(args.oxidex, kind="oxidex")

    # A missing root is fatal rather than skipped. Corpora are optional by
    # configuration, not by accident: silently dropping one that was asked for
    # would shrink the denominator and report a score for a corpus nobody
    # chose -- the same class of quiet wrongness the floors below exist to catch.
    missing_roots = [c for c in args.corpus if not os.path.isdir(c)]
    if missing_roots:
        sys.exit(
            "❌ corpus root(s) not found: " + ", ".join(missing_roots) + "\n"
            "   Refusing to score a partial corpus. Drop the root from the "
            "command line if it is genuinely not expected to be present."
        )

    def walk(root):
        if args.recursive:
            return (os.path.join(d, f)
                    for d, _dirs, fs in os.walk(root) for f in fs)
        return (os.path.join(root, f) for f in os.listdir(root))

    walked = (p for root in args.corpus for p in walk(root))

    def ext_set(spec):
        if not spec:
            return None
        return {e.strip().lower().lstrip(".") for e in spec.split(",") if e.strip()}

    exts = ext_set(args.ext)
    excluded = ext_set(args.exclude_ext) or set()

    def keep(p):
        if not os.path.isfile(p):
            return False
        base = os.path.basename(p)
        if args.only and args.only.lower() not in base.lower():
            return False
        ext = os.path.splitext(base)[1].lstrip(".").lower()
        if exts is not None and ext not in exts:
            return False
        if ext in excluded:
            return False
        return True

    # realpath-dedup: overlapping roots (or a symlink farm pointing into one)
    # would otherwise score the same file twice and weight it double.
    seen_real = set()
    files = []
    for p in sorted(p for p in walked if keep(p)):
        rp = os.path.realpath(p)
        if rp in seen_real:
            continue
        seen_real.add(rp)
        files.append(p)
    if not files:
        sys.exit(
            f"no files in {', '.join(args.corpus)}"
            + (f" matching --ext {args.ext}" if exts else "")
        )

    instrument.print_header(
        tool="conformance.py",
        git=git,
        binary=binary,
        dirty_overridden=dirty_overridden,
        oracle=oracle,
        corpus_paths=args.corpus,
        file_count=len(files),
    )

    per_ext = defaultdict(Counter)
    rename_votes = defaultdict(Counter)
    missing_votes = Counter()
    extra_votes = Counter()
    severity_votes = Counter()
    detail = []
    per_file = {}

    scored_files = 0
    et_tags_seen = 0
    for path in files:
        et = run_exiftool(oracle, path)
        if not et:
            continue
        scored_files += 1
        et_tags_seen += len(et)
        ox = run_oxidex(str(binary.path), path)
        r = compare(et, ox)
        ext = (file_type(et) or os.path.splitext(path)[1].lstrip(".")).upper()

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
        for n in r["extra"]:
            extra_votes[(ext, n)] += 1
        for _n, _ev, _ov, sev in r["value_diff"]:
            severity_votes[sev] += 1

        detail.append((len(r["missing"]) + len(r["renames"]), path, r))

        # (c) --json-out per-file VALUE/EXTRA identities, so a reviewer (or
        # a later stage's CI gate) can see exactly which tags disagreed on
        # which file without re-running the corpus. parser_status/
        # family_views are Step 13 / Stage 4 seams -- always None today.
        per_file[path] = {
            "format": ext,
            "value_diff": [[n, ev, ov, sev] for n, ev, ov, sev in r["value_diff"]],
            "extra": {k: list(v) for k, v in r["extra"].items()},
            "missing": {k: list(v) for k, v in r["missing"].items()},
            "parser_status": parser_status(path, ox),
            "family_views": family_views(et, ox),
        }

    # Refuse to print a number from a run that plainly did not happen. A
    # degraded oracle does not crash: it reads a fraction of the corpus and
    # reports a confident, precisely-formatted, completely wrong percentage.
    # Measured once at 109,261 tags over 832 files where a working oracle got
    # 507,295 over 4,230 -- nothing about the output looked wrong.
    if scored_files < args.min_files or et_tags_seen < args.min_tags:
        sys.exit(
            f"❌ vacuous run: scored {scored_files} file(s) / {et_tags_seen} ExifTool tag(s), "
            f"below the floor of {args.min_files}/{args.min_tags}.\n"
            f"   {len(files)} file(s) were found in {', '.join(args.corpus)}"
            f"{'' if args.recursive else ' (non-recursive; pass --recursive for a nested corpus)'}.\n"
            f"   oracle: {oracle.provenance()}\n"
            "   Check the oracle can actually read this corpus before trusting any score."
        )

    # score/ceiling (recall) are computed over matched+value_diff+missing+
    # renames only -- extra never enters that denominator, by design (see
    # AGENTS.md "bare-name comparison is group-blind" / project memory).
    # precision is the separate axis this step adds: how much of what
    # OxiDex emitted for a matched name was real vs. spurious. It is
    # reported, not folded into score, so a format cannot buy a better
    # score by emitting extra noise, and cannot be penalized on recall for
    # emitting it either -- extras are budgeted by later stages (Step 21's
    # "default-mode EXTRA budget ~= 0" gate reads this column).
    print(f"{'format':<10}{'files':>6}{'match':>7}{'rename':>8}"
          f"{'value':>7}{'missing':>9}{'extra':>7}{'score':>8}"
          f"{'ceiling':>9}{'precision':>11}")
    print("-" * 86)
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
        denom = c["matched"] + c["extra"]
        precision = (c["matched"] / denom) if denom else 1.0
        print(f"{ext:<10}{c['files']:>6}{c['matched']:>7}{c['renames']:>8}"
              f"{c['value_diff']:>7}{c['missing']:>9}{c['extra']:>7}"
              f"{score:>7.1%}{ceiling:>9.1%}{precision:>10.1%}")

    tot = grand["matched"] + grand["value_diff"] + grand["missing"] + grand["renames"]
    print("-" * 86)
    if tot:
        g_denom = grand["matched"] + grand["extra"]
        g_precision = (grand["matched"] / g_denom) if g_denom else 1.0
        print(f"{'TOTAL':<10}{grand['files']:>6}{grand['matched']:>7}"
              f"{grand['renames']:>8}{grand['value_diff']:>7}{grand['missing']:>9}"
              f"{grand['extra']:>7}"
              f"{grand['matched']/tot:>7.1%}"
              f"{(grand['matched']+grand['renames'])/tot:>9.1%}"
              f"{g_precision:>10.1%}")

    if rename_votes:
        print("\nrenames -- OxiDex reads these correctly under the wrong name.")
        print("value-confirmed, so these are name fixes, not parsing work:\n")
        for ext in sorted(rename_votes):
            for (on, en), n in rename_votes[ext].most_common():
                print(f"  {ext:<8} {on:<26} -> {en:<26} ({n} file{'s'*(n>1)})")

    if severity_votes:
        print("\nvalue differences by severity (precision debt, not recall):")
        for sev, c in severity_votes.most_common():
            print(f"  {sev:<14} {c}")

    print("\ntop genuinely missing tags (real extraction work):")
    for (ext, n), c in missing_votes.most_common(25):
        print(f"  {ext:<8} {n:<34} {c} file{'s'*(c>1)}")

    if extra_votes:
        print("\ntop OxiDex-only tags (precision axis -- budgeted, not scored):")
        for (ext, n), c in extra_votes.most_common(25):
            print(f"  {ext:<8} {n:<34} {c} file{'s'*(c>1)}")

    if args.show:
        for _k, path, r in sorted(detail, reverse=True)[:args.show]:
            print(f"\n--- {os.path.basename(path)} ---")
            print("  missing:", ", ".join(sorted(r["missing"])) or "-")
            print("  extra:  ", ", ".join(sorted(r["extra"])) or "-")
            if r["value_diff"]:
                print("  value:  ", ", ".join(
                    f"{n} [{sev}]" for n, _ev, _ov, sev in r["value_diff"]))

    if args.json_out:
        with open(args.json_out, "w", encoding="utf-8") as fh:
            json.dump(
                {"per_format": {k: dict(v) for k, v in per_ext.items()},
                 "renames": {k: {f"{a}->{b}": n for (a, b), n in v.items()}
                             for k, v in rename_votes.items()},
                 "missing": {f"{e}:{n}": c for (e, n), c in missing_votes.items()},
                 # (b) extras as a precision axis, symmetric with `missing`.
                 "extra": {f"{e}:{n}": c for (e, n), c in extra_votes.items()},
                 # (d) severity histogram of real VALUE differences.
                 "severity": dict(severity_votes),
                 # (c) per-file VALUE/EXTRA/MISSING identities.
                 "per_file": per_file,
                 # (e)/(f) seams -- always empty until Step 13 / Stage 4 land.
                 "parser_status": {},
                 "family_views": {}},
                fh, indent=2, sort_keys=True)
        print(f"\nwrote {args.json_out}")


if __name__ == "__main__":
    main()
