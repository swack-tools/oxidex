#!/usr/bin/env python3
"""Stage 4 exit criterion: "duplicate-loss scan shows zero irrecoverable
losses on t/images".

The Part V §1.1 finding (merged tag review) measured ~209-215 repeated
`group:name` cases across 53/194 `t/images` files, ~89-94 of them carrying
DISTINCT values -- i.e. genuinely lost, not redundant copies -- and noted the
figures were INSTRUMENT-SENSITIVE. Step 19 pinned five specific families
(`step19_duplicate_retention_regression` in `src/core/metadata_map.rs`) as a
permanent regression test, but never ran the full 194-file scan the exit
criterion actually names. This script is that scan.

INSTRUMENT, stated precisely (the plan explicitly asks for this because the
answer depends on how you count):

  1. For each file, run the PINNED oracle (`scripts/exiftool_oracle.py`,
     refuses a skewed or capability-degraded ExifTool) as:
         exiftool -a -G1 -s <file>
     in **text mode**, not `-j`/JSON. This matters: JSON output cannot carry
     two keys with the same name, and ExifTool's own JSON writer silently
     drops every occurrence but one when a `group:name` pair repeats (see
     README section "Why not -j" below, and this script's --self-test) --
     so a JSON-based scan would *underreport* duplicates by construction and
     silently agree with a broken oxidex. Text mode with `-s` (short tag
     names, unambiguous ": "-delimited value) prints one line per
     occurrence, in file order, which is the only ExifTool output mode that
     actually exposes what FoundTag retained.
  2. Each output line is parsed as `[G] Name: Value` (the `-G1` group tag,
     the short tag name `-s` produces, the printed value). A line that does
     not match that shape is treated as a continuation of the previous
     line's value (multi-line values happen, e.g. some GPS/XMP structures).
  3. "Repeated `group:name` case" = a `(group1, name)` pair that appears on
     more than one line in one file's output. This is a *group1-qualified*
     count, per the task's instruction to use `-a -G1 -s`; it is not the
     same number a `-G0`-qualified (family-0) or bare-name count would give,
     which is exactly the "range because the answer depends on how you
     count" the plan warns about. This script reports the group1-qualified
     number and does not attempt to also produce the other two -- pick a
     dimension and name it, rather than producing a third unlabeled figure.
  4. "Carries distinct values" = of the occurrences under a repeated key,
     more than one distinct value string appears (byte comparison of the
     printed value, after normalizing embedded whitespace runs -- ExifTool's
     text output sometimes reflows long values across the exact same
     terminal width, which is a display artifact, not a value difference).
  5. Tags excluded from consideration, and why:
       - The `IGNORE` set conformance.py already uses (SourceFile,
         ExifToolVersion, FileName, Directory, and the File[Modify|Access|
         InodeChange]Date/FilePermissions/FileSize/Now/ProcessingTime
         filesystem tags): these vary by machine and run, not by parser
         behavior, and would swamp the signal exactly as conformance.py's
         own comment says.
       - `Warning`/`Error` tags, under any group: diagnostic text the tool
         itself emits about its own parse, not data extracted from the
         file. oxidex and ExifTool phrase these completely differently by
         construction, so treating a `Warning` mismatch as a "duplicate
         loss" would conflate error-message wording with tag-occurrence
         retention -- a different, real thing this repo tracks by other
         means (compare-file's MISSING/EXTRA/VALUE/RENAME classes).
       - `ExifTool` as a group1: version banner and warnings live there in
         the oracle's own output; oxidex has no equivalent group, so a
         key-for-key diff would be 100% "loss" for every file for reasons
         unrelated to occurrence handling.

Step 2 (the oxidex side) runs `oxidex -a -G1 -s <file>` with the exact same
parser and the exact same exclusions, then for every oracle key with >1
distinct values asks whether oxidex's own output has >1 distinct values
under the *same* `(group1, name)` key. Three outcomes:
  - RETAINED: oxidex shows >=2 distinct values too.
  - PARTIAL: oxidex shows the key but with fewer occurrences/distinct values
    than the oracle.
  - MISSING: oxidex shows the key 0 or 1 times.
A oracle key absent from oxidex's exact `(group1, name)` but present under a
*different* group1 with the same bare name is flagged separately as
GROUP_RENAMED rather than counted as a loss -- that is a group-naming
difference (the RENAME class conformance.py already has a name for), not an
occurrence being thrown away.

Usage:
    python3 tools/exiftool-tables/duplicate_loss_scan.py \\
        /tmp/oxidex-exiftool-cache/exiftool/t/images \\
        --oxidex ./target/debug/oxidex


================================================================================
PROJECTION 2 -- `--fixtures`: occurrence parity, KNOWN FAILING BY DESIGN
================================================================================

Everything above this line is PROJECTION 1: the corpus-wide, group1-qualified
duplicate-loss scan. It is unchanged, it keeps its own numbers and its own exit
code, and `--fixtures` does not run it. Two projections live in one file, and
one oracle resolution and one line parser serve both, precisely so they can
never become two sources of truth that disagree -- see
`docs/TAG_MACHINERY_LEDGER_PLAN.md`, "What NOT to build".

WHY A SECOND PROJECTION EXISTS. Projection 1 reports `MET` -- zero irrecoverable
losses -- on a corpus containing `t/images/ExifTool.jpg`, while three diagnosed
product defects are open on that exact file. It is not wrong; it is blind to
them by construction, in three separate ways, each of which is a design choice
made above and each of which projection 2 exists to cover:

  * It asks whether oxidex has *at least as many* distinct values as the oracle
    (`scan_file`, the `>=` comparison). Three stale copies of a composite where
    the oracle has one correct value therefore scores RETAINED. Too many
    occurrences is not a loss, so it is not counted -- but it is still wrong.
  * It reads only the `-a -G1 -s` grouped surface (`run_text`). A duplicate that
    survives the grouped projection and collapses in the *ungrouped* one is
    outside its reach entirely. Measured: it scores `File:Comment` on
    ExifTool.jpg as RETAINED, and it is right about the grouped surface.
  * It deliberately reclassifies a group1 difference as GROUP_RENAMED and
    excludes it from the loss count. Four per-track occurrences flattened onto
    one group is exactly that shape, so it is excluded by design.

WHY `-j` IS USABLE HERE AND NOT ABOVE. Projection 1's "Why not -j" reasoning is
correct as written for `-G1 -j`: ExifTool's JSON writer suppresses entries with
identical JSON names. Family 4 is the copy-identity mechanism that lifts it --
measured on ExifTool.jpg, `-G -j -a` yields ONE `Comment` key while `-G1:4 -j -a`
yields TWO (`File:Comment`, `File:Copy1:Comment`). Projection 2 therefore uses
`-G0:1:4 -j` and compares it against canonical ordered oxidex occurrence
records. This is an addition to the reasoning above, not a contradiction of it:
a family-4-less JSON scan would still underreport, exactly as stated.

NEITHER PROJECTION USES `-e`. In ExifTool `-e` *disables Composite tags* --
measured: `FocalLength35efl` vanishes entirely, which would delete defect D1's
subject. In oxidex `-e` is compatibility mode and is currently a no-op because
compatibility is already the default. The parity skill's "always add `-e`"
advice is stale for this use. Both sides are held to the same SEMANTICS --
PrintConv on, composites on -- never to textually identical argv.

HOW TO RUN:

    FLEET_EXPECT_OCCURRENCE_FAILURES=1 \\
    python3 tools/exiftool-tables/duplicate_loss_scan.py --fixtures \\
        --oxidex ./target/release/oxidex [--json-out /your/private/dir/occ.json]

    just duplicate-loss-fixtures        # same thing, builds oxidex first

The opt-in env var is mandatory. Without it the suite prints what it would
measure, names the open defects and their suspected causes, and exits 64. That
is a refusal, not a skip: nothing is reported as passing and nothing is hidden.

EXIT CODES (projection 2 only; projection 1's are unchanged):

    0   ALL GREEN -- every defect fixture agrees with the pinned oracle.
    1   RED, AS EXPECTED PRE-FIX -- controls all passed, so the instrument is
        trustworthy and these are product differences. Characterization
        evidence for plan Stage 2B step 3.
    3   HARNESS SUSPECT -- a control failed, or the oracle's two independent
        surfaces disagreed, or the corpus is missing. No product conclusion may
        be drawn from the run.
    2   Oracle unresolvable, skewed, or capability-degraded.
    64  Opt-in env var absent.

A red run exits NON-ZERO. "Expected failure" is deliberately not mapped onto
exit 0: an expected-fail-is-success suite is the exact thing that gets quietly
ratcheted into normality and then wired into a green CI path by accident. The
suite is kept off CI by opt-in, not by lying about its result -- and nothing in
this repo collects `tools/exiftool-tables/test_*.py` or runs this file without
arguments, the only Python test discovery being `unittest discover -s tests`
rooted at `tools/fleet`.

THE THREE DEFECTS, THEIR FIXTURES, AND WHAT GREEN WILL MEAN:

  D1  src/core/tag_sink.rs:205 -- `TagSink::remove` drops only the winner index;
      the historical occurrence survives, and src/cli/tag_resolution.rs:167
      scans all historical occurrences. Composite refinement removes-and-
      reinserts, so intermediate states stay visible and one can win.
      GREEN = `Composite:FocalLength35efl` on ExifTool.jpg has exactly ONE
      occurrence, under both the unfiltered and the filtered grouped
      projection, valued `6.0 mm (35 mm equivalent: 41.4 mm)`.
      TODAY = three occurrences, all the stale pre-refinement `6.0 mm`.

  D2  src/cli/tag_resolution.rs:511 -- the ungrouped path always uses the
      winner-only filtered map and ignores `args.all_tags`, so occurrences
      sharing a family-1 group collapse to one.
      GREEN = `Comment` on ExifTool.jpg has TWO occurrences under the
      *unfiltered ungrouped* projection.
      TODAY = one. The same tag under the *filtered* ungrouped projection, and
      under either grouped projection, already reports two -- which is what the
      two D2 controls pin down, and why a fixture written only against the
      filtered surface would have gone green and hidden the defect.

  D3  src/parsers/quicktime/metadata_extractor.rs:1072 -- media occurrences go
      through the suffixed flat-key insert shim instead of occurrence records
      carrying a TrackN group and an instance index, so per-track identity is
      destroyed at extraction time.
      GREEN = `MediaCreateDate` on CanonRaw.cr3 has FOUR occurrences whose
      family-1 groups are the ordered sequence Track1, Track2, Track3, Track4,
      every one emitted under the real ExifTool tag name.
      TODAY = five lines, all grouped `QuickTime`, three of them under the
      fabricated names `MediaCreateDate_2/_3/_4`, which are not ExifTool tag
      names at all.

THE FILTERED / UNFILTERED AXIS is measured, not assumed. Stage 2B lists "default
vs unfiltered vs explicitly filtered `-a`" first, and it is load-bearing:

    Comment on ExifTool.jpg              oracle  oxidex
      ungrouped, unfiltered  -a -s          2       1    <- D2 lives here
      ungrouped, filtered    -a -s -Comment 2       2    <- D2 invisible here
      grouped,   unfiltered  -a -G1 -s      2       2

    MediaCreateDate on CanonRaw.cr3      oracle  oxidex
      grouped,   filtered                   4       2    (_N keys cannot match
                                                          an explicit request)
      grouped,   unfiltered                 4       5    (2 real + 3 fabricated)

NAME FIDELITY. The unfiltered projection also compares the literal emitted tag
names. On the oxidex side an unfiltered read matches `<Tag>` and `<Tag>_<N>`, so
the insert shim's fabricated names are captured and shown rather than dropped by
a filter that cannot match them. AGENTS.md's rule is that a plausible-but-wrong
value under a real ExifTool tag name is worse than an absent tag; a value under
a tag name ExifTool does not have is worse still, because no consumer can ask
for it. The alias rule is applied to the oxidex side only, and only when
unfiltered -- never to the oracle, which does not fabricate names and where it
would let a genuinely different tag masquerade as an occurrence of this one.

INSTRUMENT SELF-CHECKS (why a red result is believable):
  1. Both oracle probes asserted before any number -- `-ver` must equal the pin,
     and `-FileType` on OOXML.docx must be DOCX. A matching `-ver` is not a
     working oracle: a perl without Archive::Zip reports the right version and
     the wrong FileType.
  2. `instrument.resolve_binary` exits the moment a path is not a file, rather
     than letting every subprocess fail closed and look like a confident 0%.
  3. A dirty tree refuses to measure unless OXIDEX_ALLOW_DIRTY_TREE=1; the
     header records the override and any binary/source staleness.
  4. ORACLE CROSS-CHECK -- every grouped fixture asks the oracle twice, through
     family-4 JSON and through `-G1` text. Disagreement on count aborts the run
     with exit 3 instead of reporting a number.
  5. VALUE-COMPARABILITY GUARD -- those two surfaces render some values
     differently (`Comment`'s JSON keeps a literal CR-LF; the text surface
     renders control characters as `.`). Where they differ the value dimension
     is reported NOT COMPARABLE with the reason printed, never silently
     compared and never silently dropped. Count, group and name dimensions are
     asserted regardless.
  6. PASSING CONTROLS PER DEFECT, on data where oxidex and the oracle already
     agree. Without them a harness bug and a product bug are indistinguishable.
     A failing control is a HARDER failure than a failing defect fixture (exit
     3, not 1) and suppresses every product conclusion in the run.

Known, deliberately-unasserted divergence: under the ungrouped projection oxidex
emits `Make`'s two occurrences in the opposite source order from ExifTool
(`Canon, FUJIFILM` vs `FUJIFILM, Canon`). That is Stage 2B's separate "source
order" item, not one of these three defects, so the D2 control asserts count and
value *multiset* and explicitly does not assert sequence. Recorded here rather
than left as a silent choice.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import exiftool_oracle  # noqa: E402
import instrument  # noqa: E402

IGNORE_NAMES = {
    "SourceFile", "ExifToolVersion", "FileName", "Directory",
    "FileModifyDate", "FileAccessDate", "FileInodeChangeDate",
    "FilePermissions", "FileSize", "Now", "ProcessingTime",
    "Warning", "Error",
}
IGNORE_GROUPS = {"ExifTool"}

LINE_RE = re.compile(r"^\[(?P<group>[^\]]+)\]\s*(?P<name>\S+)\s*:\s?(?P<value>.*)$")
WS_RE = re.compile(r"\s+")


def normalize_value(v: str) -> str:
    return WS_RE.sub(" ", v).strip()


def parse_group_output(text: str) -> list[tuple[str, str, str]]:
    """Parses `-a -G1 -s` text output into `(group, name, value)` triples,
    one per printed occurrence, in file order. A non-matching line is
    treated as a continuation of the previous occurrence's value."""
    out: list[tuple[str, str, str]] = []
    for raw_line in text.splitlines():
        if not raw_line.strip():
            continue
        m = LINE_RE.match(raw_line)
        if m:
            out.append((m.group("group"), m.group("name"), m.group("value")))
        elif out:
            g, n, v = out[-1]
            out[-1] = (g, n, v + " " + raw_line.strip())
    return out


def keep(group: str, name: str) -> bool:
    if group in IGNORE_GROUPS:
        return False
    if name in IGNORE_NAMES:
        return False
    return True


@dataclass
class FileResult:
    path: str
    oracle_repeated: dict[tuple[str, str], list[str]] = field(default_factory=dict)
    oracle_distinct: dict[tuple[str, str], list[str]] = field(default_factory=dict)
    oxidex_by_key: dict[tuple[str, str], list[str]] = field(default_factory=dict)
    oxidex_by_name: dict[str, dict[str, list[str]]] = field(default_factory=dict)
    losses: dict[tuple[str, str], str] = field(default_factory=dict)  # key -> status


def group_occurrences(triples: list[tuple[str, str, str]]) -> dict[tuple[str, str], list[str]]:
    out: dict[tuple[str, str], list[str]] = defaultdict(list)
    for g, n, v in triples:
        if not keep(g, n):
            continue
        out[(g, n)].append(normalize_value(v))
    return out


def run_text(argv: list[str], path: str, extra: list[str] | None = None) -> str:
    """Run `<argv> -a -G1 -s [extra] <path>` and return stdout.

    `extra` defaults to nothing, so projection 1's two call sites invoke exactly
    the command they always did. Projection 2 passes an explicit `-TAG` request
    through it, so both projections share one invocation site and cannot drift
    into disagreeing about how either tool is called.
    """
    result = subprocess.run(
        [*argv, "-a", "-G1", "-s", *(extra or []), path],
        capture_output=True, text=True, errors="replace",
    )
    return result.stdout


# =========================================================================
# PROJECTION 2 -- occurrence fixtures (KNOWN FAILING BY DESIGN)
#
# Everything below is `--fixtures` only. It reuses this file's oracle
# resolution, instrument header, `parse_group_output` line parser and
# `run_text` invocation site; it adds the two surfaces projection 1 has no
# reason to read (ungrouped text, and family-4 JSON). Nothing below is
# reachable from the corpus scan above. See the module docstring.
# =========================================================================

#: Mandatory opt-in for projection 2. Absent -> refuse and say so (exit 64).
OPT_IN_ENV = "FLEET_EXPECT_OCCURRENCE_FAILURES"

FX_GREEN = 0
FX_RED_EXPECTED = 1
FX_ORACLE = 2
FX_HARNESS_SUSPECT = 3
FX_NOT_OPTED_IN = 64

CACHE = Path(os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))  # nosec B108
EXIFTOOL_JPG = CACHE / "exiftool" / "t" / "images" / "ExifTool.jpg"
CANON_CR3 = CACHE / "combined-samples" / "CanonRaw.cr3"
OOXML_DOCX = CACHE / "exiftool" / "t" / "images" / "OOXML.docx"

# The ungrouped sibling of LINE_RE. LINE_RE itself is deliberately NOT touched:
# its "a bracketed group is required, anything else is a value continuation"
# behaviour is projection 1's contract and is cited by name from source comments
# (e.g. src/parsers/specialized/fits.rs). This one accepts an unbracketed line
# and also captures oxidex's ` (N)` copy suffix, which it emits in the filtered
# ungrouped projection to disambiguate otherwise-identical lines.
PLAIN_LINE_RE = re.compile(
    r"^(?P<name>[^\s:]+)(?:\s+\((?P<copy>\d+)\))?\s*:\s?(?P<value>.*)$"
)


def parse_plain_output(text: str) -> list[tuple[str | None, str, str, int | None]]:
    """Ungrouped `-a -s` text -> `(None, name, value, copy_index)` per occurrence.

    A sibling of `parse_group_output`, not a replacement: same file order, same
    "an unparseable line continues the previous value" rule, so the two cannot
    disagree about what a line means.
    """
    out: list[tuple[str | None, str, str, int | None]] = []
    for raw_line in text.splitlines():
        if not raw_line.strip():
            continue
        m = PLAIN_LINE_RE.match(raw_line)
        if m:
            copy = m.group("copy")
            out.append((None, m.group("name"), m.group("value"), int(copy) if copy else None))
        elif out:
            g, n, v, c = out[-1]
            out[-1] = (g, n, v + " " + raw_line.strip(), c)
    return out


@dataclass(frozen=True)
class Occurrence:
    """One extracted instance of one tag, in extraction order.

    The canonical ordered record the plan calls for. Emphatically not a
    flattened JSON object: a flattened object cannot represent two occurrences
    sharing a name, which is the whole property under test.

    `tag` is the logical tag being measured; `emitted_name` is the literal name
    the tool printed. They differ exactly when a tool fabricates a name.
    """

    ordinal: int
    tag: str
    emitted_name: str
    group1: str | None
    value: str
    copy_index: int | None = None

    def describe(self) -> str:
        g = f"[{self.group1}]" if self.group1 else "[-]"
        c = f" (copy {self.copy_index})" if self.copy_index else ""
        bad = "" if self.emitted_name == self.tag else "   <-- NOT AN EXIFTOOL TAG NAME"
        return f"{g} {self.emitted_name}{c} = {self.value!r}{bad}"


def name_matches(emitted: str, tag: str, allow_suffix: bool) -> bool:
    """Does `emitted` name an occurrence of `tag`?

    `allow_suffix` admits the `_N` flat-key suffix the QuickTime insert shim
    fabricates, so those occurrences are seen and reported as fabricated rather
    than dropped by a filter that cannot match them. Enabled for the oxidex side
    of an unfiltered read only -- never for the oracle, which does not fabricate
    names and where it would let a genuinely different tag masquerade as an
    occurrence of this one.
    """
    if emitted == tag:
        return True
    return bool(allow_suffix and re.fullmatch(re.escape(tag) + r"_\d+", emitted))


def json_scalar(v: object) -> str:
    """Render one ExifTool JSON value the way its text surface would.

    Floats arrive as strings (`parse_float=str`, so ExifTool's "1.80" does not
    become 1.8 and get graded as a difference -- a bug rediscovered at least
    five times in this repo); lists become the plain-output comma form.
    """
    if isinstance(v, list):
        return ", ".join(json_scalar(x) for x in v)
    if isinstance(v, bool):
        return "True" if v else "False"
    return str(v)


def parse_family4_json(text: str, tag: str) -> list[Occurrence]:
    """Parse `-G0:1:4 -j` keys of the form `G0[:G1][:CopyN]:Tag`.

    Read right-to-left: the number of family segments varies per tag
    (`File:Comment`, `File:Copy1:Comment`, `EXIF:IFD0:Copy1:Make` and
    `QuickTime:Track1:MediaCreateDate` all occur in this corpus). Where family 1
    is absent ExifTool means family 1 equals family 0 -- verified against the
    `-G1` text surface, which prints `[File]` for `File:Comment` and
    `[QuickTime]` for `QuickTime:MajorBrand`.
    """
    try:
        doc = json.loads(text, parse_float=str)
    except json.JSONDecodeError:
        return []
    if not doc:
        return []
    recs: list[Occurrence] = []
    for key, raw in doc[0].items():
        segs = key.split(":")
        if segs[-1] != tag:
            continue
        segs = segs[:-1]
        copy_index = None
        if segs and re.fullmatch(r"Copy\d+", segs[-1]):
            copy_index = int(segs[-1][4:])
            segs = segs[:-1]
        recs.append(Occurrence(
            ordinal=len(recs), tag=tag, emitted_name=tag,
            group1=segs[-1] if segs else None,
            value=json_scalar(raw), copy_index=copy_index,
        ))
    return recs


# Raw tool output is memoised per argv, so one unfiltered whole-file dump is
# paid for once however many fixtures read it. The argv is still reported per
# fixture, so every number stays attributable to an exact command.
_OUT_CACHE: dict[tuple[str, ...], str] = {}


def _cached(argv: list[str]) -> str:
    key = tuple(argv)
    if key not in _OUT_CACHE:
        _OUT_CACHE[key] = subprocess.run(
            argv, capture_output=True, text=True, errors="replace",
        ).stdout
    return _OUT_CACHE[key]


def _tag_arg(tag: str, filtered: bool) -> list[str]:
    return [f"-{tag}"] if filtered else []


def oracle_grouped_json(oracle, path: Path, tag: str, filtered: bool):
    argv = oracle.command(["-G0:1:4", "-j", "-a", *_tag_arg(tag, filtered), str(path)])
    return parse_family4_json(_cached(argv), tag), list(argv)


def oracle_grouped_text(oracle, path: Path, tag: str, filtered: bool):
    """The independent second oracle surface, used to cross-check counts."""
    argv = oracle.command(["-a", "-G1", "-s", *_tag_arg(tag, filtered), str(path)])
    recs = [
        Occurrence(ordinal=i, tag=tag, emitted_name=n, group1=g, value=normalize_value(v))
        for i, (g, n, v) in enumerate(
            t for t in parse_group_output(_cached(argv)) if name_matches(t[1], tag, False)
        )
    ]
    return recs, list(argv)


def oxidex_grouped(binary: Path, path: Path, tag: str, filtered: bool):
    argv = [str(binary), "-a", "-G1", "-s", *_tag_arg(tag, filtered), str(path)]
    recs = [
        Occurrence(ordinal=i, tag=tag, emitted_name=n, group1=g, value=normalize_value(v))
        for i, (g, n, v) in enumerate(
            t for t in parse_group_output(_cached(argv))
            if name_matches(t[1], tag, not filtered)
        )
    ]
    return recs, argv


def _ungrouped(binary_or_oracle_argv: list[str], path: Path, tag: str, filtered: bool,
               allow_suffix: bool):
    argv = [*binary_or_oracle_argv, "-a", "-s", *_tag_arg(tag, filtered), str(path)]
    recs = [
        Occurrence(ordinal=i, tag=tag, emitted_name=n, group1=None,
                   value=normalize_value(v), copy_index=c)
        for i, (_g, n, v, c) in enumerate(
            t for t in parse_plain_output(_cached(argv))
            if name_matches(t[1], tag, allow_suffix)
        )
    ]
    return recs, argv


def oracle_ungrouped(oracle, path: Path, tag: str, filtered: bool):
    return _ungrouped(oracle.command(), path, tag, filtered, allow_suffix=False)


def oxidex_ungrouped(binary: Path, path: Path, tag: str, filtered: bool):
    return _ungrouped([str(binary)], path, tag, filtered, allow_suffix=not filtered)


# --- fixture table --------------------------------------------------------

COUNT = "count"
GROUP1_SEQ = "group1_sequence"
NAME_SEQ = "emitted_name_sequence"
VALUE_SEQ = "value_sequence"
VALUE_SET = "value_multiset"
WINNER_VALUE = "winner_value"


@dataclass(frozen=True)
class Fixture:
    fid: str
    defect: str
    kind: str  # "defect" | "control"
    title: str
    path: Path
    tag: str
    projection: str  # "grouped" | "ungrouped"
    filtered: bool
    checks: tuple[str, ...]
    cause: str = ""
    note: str = ""

    @property
    def surface(self) -> str:
        return f"{self.projection}/{'filtered' if self.filtered else 'unfiltered'}"


D1_CAUSE = (
    "src/core/tag_sink.rs:205 -- TagSink::remove drops only the winner index, so the "
    "historical occurrence survives; src/cli/tag_resolution.rs:167 then scans all "
    "historical occurrences. Composite refinement removes-and-reinserts, so each "
    "intermediate state stays visible and an intermediate one can win."
)
D2_CAUSE = (
    "src/cli/tag_resolution.rs:511 -- the ungrouped path always uses the winner-only "
    "filtered map and ignores args.all_tags, so occurrences sharing a family-1 group "
    "(both File:Comment here) collapse to one."
)
D3_CAUSE = (
    "src/parsers/quicktime/metadata_extractor.rs:1072 -- media occurrences go through "
    "the suffixed flat-key insert shim instead of occurrence records carrying a TrackN "
    "group and an instance index, so per-track identity is destroyed at extraction time "
    "and the shim's _N suffix leaks out as a tag name ExifTool does not have."
)

FIXTURES: tuple[Fixture, ...] = (
    Fixture(
        fid="D1", defect="D1", kind="defect",
        title="removed composite occurrences stay active (whole-file read)",
        path=EXIFTOOL_JPG, tag="FocalLength35efl",
        projection="grouped", filtered=False,
        checks=(COUNT, WINNER_VALUE), cause=D1_CAUSE,
    ),
    Fixture(
        fid="D1-filtered", defect="D1", kind="defect",
        title="...and the same three stale states survive an explicit -TAG request",
        path=EXIFTOOL_JPG, tag="FocalLength35efl",
        projection="grouped", filtered=True,
        checks=(COUNT, WINNER_VALUE), cause=D1_CAUSE,
    ),
    Fixture(
        fid="D1-control", defect="D1", kind="control",
        title="a two-occurrence non-composite tag, same file and same projection",
        path=EXIFTOOL_JPG, tag="Make",
        projection="grouped", filtered=False,
        checks=(COUNT, GROUP1_SEQ, NAME_SEQ, VALUE_SEQ),
        note=("Proves the grouped occurrence projection can count >1, match an ordered "
              "family-1 sequence and match ordered values -- so D1's 3-vs-1 is a product "
              "result, not the counter defaulting to 'mismatch'."),
    ),
    Fixture(
        fid="D2", defect="D2", kind="defect",
        title="unfiltered ungrouped -a collapses same-group duplicates",
        path=EXIFTOOL_JPG, tag="Comment",
        projection="ungrouped", filtered=False,
        checks=(COUNT,), cause=D2_CAUSE,
    ),
    Fixture(
        fid="D2-control-a", defect="D2", kind="control",
        title="the SAME tag and projection, explicitly filtered -- oxidex keeps both",
        path=EXIFTOOL_JPG, tag="Comment",
        projection="ungrouped", filtered=True,
        checks=(COUNT,),
        note=("Same file, same tag, same binary, same ungrouped projection -- only the "
              "explicit -TAG request differs, and there oxidex reports 2. Rules out "
              "'oxidex never parsed the second Comment' and localises D2 to the "
              "UNFILTERED ungrouped path. A fixture written only against this filtered "
              "surface would have gone green and hidden the defect."),
    ),
    Fixture(
        fid="D2-control-b", defect="D2", kind="control",
        title="a two-occurrence tag the unfiltered ungrouped projection already keeps",
        path=EXIFTOOL_JPG, tag="Make",
        projection="ungrouped", filtered=False,
        checks=(COUNT, VALUE_SET),
        note=("Make's two occurrences live in different family-1 groups (IFD0, CIFF), so "
              "the winner-only map keeps both. Proves the unfiltered ungrouped counter "
              "genuinely counts 2 and is not wired to 1 -- the collapse needs occurrences "
              "that SHARE a group. Sequence deliberately not asserted: oxidex emits these "
              "in the opposite source order, Stage 2B's separate 'source order' item."),
    ),
    Fixture(
        fid="D3", defect="D3", kind="defect",
        title="media occurrences lose track identity and leak fabricated tag names",
        path=CANON_CR3, tag="MediaCreateDate",
        projection="grouped", filtered=False,
        checks=(COUNT, GROUP1_SEQ, NAME_SEQ), cause=D3_CAUSE,
    ),
    Fixture(
        fid="D3-filtered", defect="D3", kind="defect",
        title="...and an explicit -TAG request cannot reach the _N keys at all",
        path=CANON_CR3, tag="MediaCreateDate",
        projection="grouped", filtered=True,
        checks=(COUNT, GROUP1_SEQ), cause=D3_CAUSE,
    ),
    Fixture(
        fid="D3-control", defect="D3", kind="control",
        title="a movie-level QuickTime tag on the same file, same projection",
        path=CANON_CR3, tag="MajorBrand",
        projection="grouped", filtered=False,
        checks=(COUNT, GROUP1_SEQ, NAME_SEQ, VALUE_SEQ),
        note=("Proves the harness reads family-1 groups out of the CR3's QuickTime "
              "container and compares them correctly -- so D3's [QuickTime] x5 vs "
              "[Track1..Track4] is a product result, not a group-parsing artifact."),
    ),
)


@dataclass
class DimResult:
    name: str
    ok: bool | None  # None == not comparable
    expected: object = None
    actual: object = None
    reason: str = ""


@dataclass
class FixtureResult:
    fixture: Fixture
    oracle_recs: list[Occurrence] = field(default_factory=list)
    oxidex_recs: list[Occurrence] = field(default_factory=list)
    oracle_argv: list[str] = field(default_factory=list)
    oxidex_argv: list[str] = field(default_factory=list)
    dims: list[DimResult] = field(default_factory=list)

    @property
    def failed(self) -> bool:
        return any(d.ok is False for d in self.dims)


def _dim(name: str, expected, actual) -> DimResult:
    return DimResult(name=name, ok=(expected == actual), expected=expected, actual=actual)


def evaluate_fixture(fx: Fixture, oracle, binary: Path, cross: dict[str, str]) -> FixtureResult:
    comparable, reason = True, ""

    if fx.projection == "grouped":
        o_recs, o_argv = oracle_grouped_json(oracle, fx.path, fx.tag, fx.filtered)
        x_recs, x_argv = oxidex_grouped(binary, fx.path, fx.tag, fx.filtered)

        # Self-check 4: the oracle's two independent surfaces must agree on count.
        t_recs, t_argv = oracle_grouped_text(oracle, fx.path, fx.tag, fx.filtered)
        if len(t_recs) != len(o_recs):
            cross[fx.fid] = (
                f"oracle disagrees with itself on {fx.tag}: family-4 JSON says "
                f"{len(o_recs)}, -G1 text says {len(t_recs)}.\n"
                f"      json: {' '.join(o_argv)}\n"
                f"      text: {' '.join(t_argv)}"
            )

        # Self-check 5: compare values only where both surfaces render alike.
        json_vals = [r.value for r in o_recs]
        text_vals = [r.value for r in t_recs]
        if json_vals != text_vals:
            comparable = False
            reason = (
                f"the oracle's two surfaces render this value differently (json "
                f"{json_vals!r} vs text {text_vals!r}); comparing here would grade a "
                f"rendering difference, not a product difference"
            )
    else:
        o_recs, o_argv = oracle_ungrouped(oracle, fx.path, fx.tag, fx.filtered)
        x_recs, x_argv = oxidex_ungrouped(binary, fx.path, fx.tag, fx.filtered)

    res = FixtureResult(fixture=fx, oracle_recs=o_recs, oxidex_recs=x_recs,
                        oracle_argv=o_argv, oxidex_argv=x_argv)

    for check in fx.checks:
        if check == COUNT:
            res.dims.append(_dim(COUNT, len(o_recs), len(x_recs)))
        elif check == GROUP1_SEQ:
            res.dims.append(_dim(GROUP1_SEQ, [r.group1 for r in o_recs],
                                 [r.group1 for r in x_recs]))
        elif check == NAME_SEQ:
            res.dims.append(_dim(NAME_SEQ, [r.emitted_name for r in o_recs],
                                 [r.emitted_name for r in x_recs]))
        elif not comparable:
            res.dims.append(DimResult(name=check, ok=None, reason=reason))
        elif check == VALUE_SEQ:
            res.dims.append(_dim(VALUE_SEQ, [r.value for r in o_recs],
                                 [r.value for r in x_recs]))
        elif check == VALUE_SET:
            res.dims.append(_dim(VALUE_SET, sorted(r.value for r in o_recs),
                                 sorted(r.value for r in x_recs)))
        elif check == WINNER_VALUE:
            res.dims.append(_dim(WINNER_VALUE,
                                 o_recs[0].value if o_recs else None,
                                 x_recs[0].value if x_recs else None))
    return res


def report_fixture(res: FixtureResult) -> None:
    fx = res.fixture
    kind = "CONTROL" if fx.kind == "control" else "DEFECT "
    verdict = "FAIL" if res.failed else "pass"
    print(f"[{verdict}] {kind} {fx.fid:<14} {fx.tag} @ {fx.path.name}  ({fx.surface})")
    print(f"          {fx.title}")

    if res.failed:
        print(f"          oracle argv : {' '.join(res.oracle_argv)}")
        print(f"          oxidex argv : {' '.join(res.oxidex_argv)}")
        print(f"          oracle says ({len(res.oracle_recs)} occurrence(s)):")
        for r in res.oracle_recs:
            print(f"            {r.ordinal}. {r.describe()}")
        print(f"          oxidex says ({len(res.oxidex_recs)} occurrence(s)):")
        for r in res.oxidex_recs:
            print(f"            {r.ordinal}. {r.describe()}")

    for d in res.dims:
        if d.ok is True:
            print(f"          ok       {d.name}: {d.actual!r}")
        elif d.ok is None:
            print(f"          n/a      {d.name}: NOT COMPARABLE -- {d.reason}")
        else:
            print(f"          MISMATCH {d.name}")
            print(f"            oracle expects : {d.expected!r}")
            print(f"            oxidex actual  : {d.actual!r}")

    if res.failed and fx.kind == "defect":
        print(f"          suspected cause: {fx.cause}")
    elif res.failed and fx.kind == "control":
        print("          HARNESS SUSPECT: a control failed. This fixture measures data on\n"
              "          which oxidex and the pinned oracle are known to agree, so a\n"
              "          difference here is a harness defect until proven otherwise -- no\n"
              "          product conclusion may be drawn from any other line in this run.")
        if fx.note:
            print(f"          control rationale: {fx.note}")
    print()


def fixtures_refusal() -> int:
    print("=== duplicate_loss_scan.py --fixtures (occurrence projection, schema 1) ===")
    print()
    print("REFUSING TO MEASURE: this is a known-failing characterization suite and it is")
    print("opt-in by design. It is red on purpose until three open product defects are")
    print("fixed, and it must never be wired into a green CI path or ratcheted into a")
    print("baseline. Re-run with the opt-in set:")
    print()
    print(f"    {OPT_IN_ENV}=1 python3 tools/exiftool-tables/duplicate_loss_scan.py \\")
    print("        --fixtures --oxidex ./target/release/oxidex")
    print()
    print("It would measure these open defects (docs/TAG_MACHINERY_LEDGER_PLAN.md, Stage")
    print("2B) against ExifTool pinned 13.59. Projection 1 (the corpus duplicate-loss")
    print("scan) is blind to all three by construction -- see the module docstring.")
    seen: set[str] = set()
    for fx in FIXTURES:
        if fx.kind != "defect" or fx.defect in seen:
            continue
        seen.add(fx.defect)
        print(f"  {fx.defect}  {fx.tag:<18} @ {fx.path.name:<16} -- {fx.title}")
        print(f"      cause: {fx.cause}")
    print()
    print("This exit is a refusal, not a skip: nothing here was reported as passing.")
    return FX_NOT_OPTED_IN


def run_fixture_suite(args) -> int:
    """Projection 2 entry point. Never touches projection 1's measurement."""
    if os.environ.get(OPT_IN_ENV) != "1":
        return fixtures_refusal()

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "duplicate_loss_scan.py --fixtures")
    binary = instrument.resolve_binary(args.oxidex, kind="oxidex")

    try:
        oracle = (exiftool_oracle.resolve_tree(args.exiftool_dir)
                  if args.exiftool_dir else exiftool_oracle.shared())
    except exiftool_oracle.OracleError as exc:
        print(f"❌ {exc}", file=sys.stderr)
        return FX_ORACLE

    # Probe 1: version. resolve() only warns on skew; this suite's entire claim
    # rests on the pin, so refuse instead -- skew manufactures phantom
    # regressions AND phantom fixes, indistinguishable afterwards.
    if not oracle.version_matches:
        print(f"❌ oracle probe 1 FAILED: ExifTool {oracle.version} is not the pinned "
              f"{oracle.pinned_version}. Refusing to measure.", file=sys.stderr)
        return FX_ORACLE
    # Probe 2: capability. A matching -ver is not a working oracle.
    try:
        oracle.check_container_support(OOXML_DOCX)
    except exiftool_oracle.OracleError as exc:
        print(f"❌ oracle probe 2 FAILED: {exc}", file=sys.stderr)
        return FX_ORACLE

    corpus = sorted({fx.path for fx in FIXTURES})
    missing = [p for p in [*corpus, OOXML_DOCX] if not p.is_file()]
    if missing:
        print("❌ fixture corpus missing: " + ", ".join(str(p) for p in missing), file=sys.stderr)
        print("   populate it from the pinned ExifTool cache (see the parity skill).",
              file=sys.stderr)
        return FX_HARNESS_SUSPECT

    instrument.print_header(
        tool="duplicate_loss_scan.py --fixtures (PROJECTION 2: occurrence, schema 1)",
        git=git, binary=binary, dirty_overridden=dirty_overridden, oracle=oracle,
        corpus_paths=corpus, file_count=len(corpus),
        extra=[
            f"probes:  -ver -> {oracle.version} (pin {oracle.pinned_version})"
            " | -FileType OOXML.docx -> DOCX",
            "note:    KNOWN-FAILING characterization suite (plan Stage 2B step 3).",
            "         RED here is the expected pre-fix result and is NOT a baseline.",
            "         PROJECTION 1 (the corpus duplicate-loss scan) did not run and is",
            "         untouched; so is conformance.py's winner/display baseline.",
        ],
    )

    cross: dict[str, str] = {}
    results = [evaluate_fixture(fx, oracle, binary.path, cross) for fx in FIXTURES]

    if cross:
        print("❌ ORACLE SELF-DISAGREEMENT -- refusing to report a product number.")
        for fid, msg in cross.items():
            print(f"   {fid}: {msg}")
        return FX_HARNESS_SUSPECT

    for res in results:
        report_fixture(res)

    controls = [r for r in results if r.fixture.kind == "control"]
    defects = [r for r in results if r.fixture.kind == "defect"]
    bad_controls = [r for r in controls if r.failed]
    red_defects = [r for r in defects if r.failed]
    red_ids = sorted({r.fixture.defect for r in red_defects})

    print("=" * 78)
    print("OCCURRENCE FIXTURES -- PROJECTION 2 (family-4 oracle vs ordered oxidex records)")
    print(f"controls : {len(controls) - len(bad_controls)}/{len(controls)} passing")
    print(f"defects  : {len(red_defects)}/{len(defects)} fixture(s) RED, covering "
          f"{len(red_ids)}/3 defects ({', '.join(red_ids) or 'none'})")
    print("=" * 78)

    if bad_controls:
        print("VERDICT: HARNESS SUSPECT (exit 3).")
        print("  A control failed. Controls measure data on which oxidex and the pinned\n"
              "  oracle already agree; a difference there is a harness defect until proven\n"
              "  otherwise. Every other result in this run is unattributable -- do NOT read\n"
              "  the defect fixtures above as product evidence.")
        status, rc = "harness_suspect", FX_HARNESS_SUSPECT
    elif red_defects:
        print("VERDICT: RED -- as expected before the fix (exit 1).")
        print("  All controls passed, so the instrument is trustworthy and these are\n"
              "  product differences. This run is CHARACTERIZATION EVIDENCE for plan\n"
              "  Stage 2B step 3. It must not be committed as a baseline, and the exit\n"
              "  status is deliberately non-zero so it cannot be ratcheted into normality.")
        for d in red_ids:
            print(f"    {d}: {next(r.fixture.cause for r in red_defects if r.fixture.defect == d)}")
        status, rc = "red_expected", FX_RED_EXPECTED
    else:
        print("VERDICT: ALL GREEN (exit 0).")
        print("  Every defect fixture now agrees with the pinned oracle. Next per the plan:\n"
              "  step 5 -- re-run against the reconstructed current-main tree, not only this\n"
              "  branch tip; step 6 -- only then commit the accepted OCCURRENCE baseline, as\n"
              "  a second projection versioned separately from conformance.py's.")
        status, rc = "green", FX_GREEN

    if args.json_out:
        payload = {
            "schema_version": 1,
            "projection": "occurrence",
            "note": ("PROJECTION 2. Not comparable with, and not a replacement for, this "
                     "file's projection-1 duplicate-loss numbers or conformance.py's "
                     "winner/display baseline. Neither was run or modified by this."),
            "tool": "duplicate_loss_scan.py --fixtures",
            "status": status,
            "oracle": oracle.provenance(),
            "exiftool_version": oracle.version,
            "exiftool_pin": oracle.pinned_version,
            "oxidex": str(binary.path),
            "commit": git.commit,
            "dirty": git.dirty,
            "fixtures": [
                {
                    "id": r.fixture.fid, "defect": r.fixture.defect, "kind": r.fixture.kind,
                    "tag": r.fixture.tag, "file": str(r.fixture.path),
                    "projection": r.fixture.projection, "filtered": r.fixture.filtered,
                    "oracle_argv": r.oracle_argv, "oxidex_argv": r.oxidex_argv,
                    "oracle_occurrences": [o.describe() for o in r.oracle_recs],
                    "oxidex_occurrences": [o.describe() for o in r.oxidex_recs],
                    "failed": r.failed,
                    "cause": r.fixture.cause or None,
                    "control_rationale": r.fixture.note or None,
                    "dimensions": [
                        {"name": d.name, "ok": d.ok, "expected": d.expected,
                         "actual": d.actual, "reason": d.reason or None}
                        for d in r.dims
                    ],
                }
                for r in results
            ],
        }
        p = Path(args.json_out)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(payload, indent=2) + "\n")
        print(f"\nwrote {p}")

    return rc


def scan_file(oracle_argv: list[str], oxidex_bin: str, path: str) -> FileResult:
    fr = FileResult(path=path)

    oracle_text = run_text(oracle_argv, path)
    oracle_groups = group_occurrences(parse_group_output(oracle_text))
    fr.oracle_repeated = {k: v for k, v in oracle_groups.items() if len(v) > 1}
    fr.oracle_distinct = {
        k: v for k, v in fr.oracle_repeated.items() if len(set(v)) > 1
    }

    oxidex_text = run_text([oxidex_bin], path)
    oxidex_triples = parse_group_output(oxidex_text)
    fr.oxidex_by_key = group_occurrences(oxidex_triples)
    by_name: dict[str, dict[str, list[str]]] = defaultdict(dict)
    for (g, n), vals in fr.oxidex_by_key.items():
        by_name[n][g] = vals
    fr.oxidex_by_name = by_name

    for key, oracle_vals in fr.oracle_distinct.items():
        g, n = key
        oracle_distinct_n = len(set(oracle_vals))
        ox_vals = fr.oxidex_by_key.get(key, [])
        ox_distinct_n = len(set(ox_vals))
        if ox_distinct_n >= oracle_distinct_n and len(ox_vals) >= len(oracle_vals):
            fr.losses[key] = "RETAINED"
        elif ox_distinct_n > 0 or len(ox_vals) > 0:
            fr.losses[key] = "PARTIAL"
        else:
            # exact (group,name) absent from oxidex -- check for the same
            # bare name surfacing under a different group1 before calling
            # this an occurrence loss rather than a naming difference.
            alt_groups = fr.oxidex_by_name.get(n, {})
            alt_groups = {ag: av for ag, av in alt_groups.items() if ag != g}
            if any(len(av) > 1 for av in alt_groups.values()):
                fr.losses[key] = "GROUP_RENAMED"
            else:
                fr.losses[key] = "MISSING"

    return fr


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("corpus", nargs="?",
                    help="directory of sample files (e.g. .../exiftool/t/images); "
                         "required unless --fixtures is given")
    ap.add_argument("--exiftool-dir", help="ExifTool checkout root; defaults to the pinned tree")
    ap.add_argument("--oxidex", default="./target/debug/oxidex")
    ap.add_argument("--json-out")
    ap.add_argument("--show", type=int, default=0,
                     help="print per-file detail for the first N files with an oracle-distinct duplicate")
    ap.add_argument("--fixtures", action="store_true",
                    help="run PROJECTION 2 instead: the occurrence-parity fixtures, which "
                         "are KNOWN FAILING by design and need "
                         f"{OPT_IN_ENV}=1. Does not run or affect the corpus scan.")
    args = ap.parse_args()

    # PROJECTION 2 short-circuits before any of projection 1's setup, so the
    # corpus scan below is reached only by exactly the invocations that always
    # reached it.
    if args.fixtures:
        return run_fixture_suite(args)

    if not args.corpus:
        ap.error("corpus is required (or pass --fixtures to run the occurrence fixtures)")

    try:
        oracle = (exiftool_oracle.resolve_tree(args.exiftool_dir)
                  if args.exiftool_dir else exiftool_oracle.shared())
    except exiftool_oracle.OracleError as exc:
        sys.exit(f"❌ {exc}")

    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "duplicate_loss_scan.py")
    binary = instrument.resolve_binary(args.oxidex, kind="oxidex")

    if not os.path.isdir(args.corpus):
        sys.exit(f"❌ corpus root not found: {args.corpus}")

    files = sorted(
        p for p in (os.path.join(args.corpus, f) for f in os.listdir(args.corpus))
        if os.path.isfile(p)
    )
    if not files:
        sys.exit(f"❌ no files found under {args.corpus}")

    instrument.print_header(
        tool="duplicate_loss_scan.py",
        git=git,
        binary=binary,
        dirty_overridden=dirty_overridden,
        oracle=oracle,
        corpus_paths=[args.corpus],
        file_count=len(files),
        extra=["note:    `-a -G1 -s` text mode, group1-qualified (see module docstring)"],
    )

    results: list[FileResult] = []
    shown = 0
    for i, path in enumerate(files, 1):
        fr = scan_file(oracle.command(), str(binary.path), path)
        results.append(fr)
        print(f"[{i}/{len(files)}] {os.path.basename(path)}: "
              f"{len(fr.oracle_repeated)} repeated, {len(fr.oracle_distinct)} distinct, "
              f"{sum(1 for s in fr.losses.values() if s != 'RETAINED')} lost/partial",
              file=sys.stderr)
        if args.show and fr.oracle_distinct and shown < args.show:
            shown += 1
            print(f"\n--- {path} ---")
            for key, vals in fr.oracle_distinct.items():
                status = fr.losses.get(key, "?")
                print(f"  [{status}] {key[0]}:{key[1]} oracle={vals} oxidex={fr.oxidex_by_key.get(key)}")

    files_with_repeats = sum(1 for r in results if r.oracle_repeated)
    total_repeated = sum(len(r.oracle_repeated) for r in results)
    total_distinct = sum(len(r.oracle_distinct) for r in results)

    status_counts: dict[str, int] = defaultdict(int)
    for r in results:
        for s in r.losses.values():
            status_counts[s] += 1

    print("\n" + "=" * 72)
    print("DUPLICATE-LOSS SCAN -- instrument: `-a -G1 -s` text mode, group1-qualified")
    print("=" * 72)
    print(f"files scanned:                         {len(files)}")
    print(f"files with >=1 repeated group1:name:    {files_with_repeats}")
    print(f"repeated group1:name cases (total):     {total_repeated}")
    print(f"  of which carry >=2 DISTINCT values:   {total_distinct}")
    print()
    print("oxidex retention of the distinct-value cases:")
    for status in ("RETAINED", "PARTIAL", "GROUP_RENAMED", "MISSING"):
        print(f"  {status:14s} {status_counts.get(status, 0)}")
    irrecoverable = status_counts.get("MISSING", 0) + status_counts.get("PARTIAL", 0)
    print()
    print(f"irrecoverable losses (MISSING + PARTIAL): {irrecoverable}")
    print(f"Stage 4 criterion ('zero irrecoverable losses on t/images'): "
          f"{'MET' if irrecoverable == 0 else 'NOT MET'}")

    if args.json_out:
        payload = {
            "oracle": oracle.provenance(),
            "oxidex": str(binary.path),
            "commit": git.commit,
            "dirty": git.dirty,
            "files_scanned": len(files),
            "files_with_repeats": files_with_repeats,
            "total_repeated": total_repeated,
            "total_distinct": total_distinct,
            "status_counts": dict(status_counts),
            "irrecoverable": irrecoverable,
            "per_file": [
                {
                    "path": r.path,
                    "repeated": len(r.oracle_repeated),
                    "distinct": len(r.oracle_distinct),
                    "losses": {f"{k[0]}:{k[1]}": v for k, v in r.losses.items()},
                }
                for r in results
            ],
        }
        with open(args.json_out, "w") as f:
            json.dump(payload, f, indent=2)
        print(f"\nwrote {args.json_out}")

    return 1 if irrecoverable > 0 else 0


if __name__ == "__main__":
    raise SystemExit(main())
