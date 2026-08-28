#!/usr/bin/env python3
"""Classify every JSON-to-JSON delta between two `dump_tables.pl` dumps.

Step 17 of the tag-machinery overhaul: `just bump-exiftool` snapshots the old
release's dump before regenerating, and this script diffs that snapshot
against the new release's dump to answer "how much of this release did the
machinery absorb unaided, and what is left for a human". It is the work
queue that replaces fleet rediscovery (OVERHAUL_OXIDEX_PLAN.md Step 17).

Every changed/added/removed tag, table or module lands in exactly one of
four buckets, matching what the CURRENT generators (tools/exiftool-tables/
codegen.py for ProcessBinaryData tables, exprs.py for conversions,
regen-all.sh's tier 2 for the named MakerNote sub-directory manifest) can
and cannot do today -- not a re-derivation of the rules, but a direct read
of them, so this script and the generators cannot silently drift apart:

  AUTO  -- the machinery absorbs the change with zero source edits. Pure
           layout/metadata (Format, Count, Mask, ...), enum PrintConv/
           ValueConv data, a field inside an already-generated tier-2
           sub-directory table (codegen_subdirs.py regenerates the whole
           file from the dump, so any field-level change inside one of its
           already-listed tables is free), or (Step 23) a `_variants` array
           where EVERY alternative's `Condition` compiles through
           `conds.py`'s closed grammar -- mirrors codegen.py's
           `compile_variant_group`, which applies that same all-or-nothing
           rule per table before emitting a `VariantGroup`.
  EXPR  -- a PrintConv/ValueConv/RawConv carries a Perl expression or
           deparsed closure that `exprs.py` does not already translate or
           compile. Needs a hand-verified translation added to
           `exprs.TRANSLATIONS` (or, for the closure case, a decision about
           whether it is even expressible in the closed grammar).
  COND  -- three shapes, each still real work: (1) a standalone `Condition`
           field on a non-variant tag -- `conds.py`/Step 23 only compiles
           Conditions found *inside* a `_variants` array's alternatives, so
           a lone `Condition` on a single-entry tag is omitted always, not
           just "until Step 23 lands"; (2) a `_variants` array where at
           least one alternative's `Condition` falls outside `conds.py`'s
           closed grammar (a three-way `or` chain, an `lt`/`ge` string
           compare, a `\\d`/`\\w`-shorthand regex class, ...) -- refused
           exactly like `codegen.py`'s `compile_variant_group` refuses it,
           all-or-nothing for the whole array, same as the AUTO case above
           but failed; or (3) a `Hook` (mid-table format/byte-order
           rewrite), which is still genuinely unwired -- Step 26, not Step
           23. Step 23 landing narrowed this bucket to Hook plus
           grammar-refused conditions; it did not empty it.
  HAND  -- anything else: a new module or a new non-ProcessBinaryData table
           (the generator only emits ProcessBinaryData -- see
           docs/TRANSCRIPTION.md "Honest limits"), a new SubDirectory edge
           to a table the tier-2 manifest does not already name, a
           PrintConv/ValueConv of kind `code`/`list`/`other` (not even
           attempted by exprs.py, which only ever sees `kind == "expr"`
           strings), or a table whose PROCESS_PROC identity itself changed.

On top of the diff-driven classification, this script ALSO always lists the
still-generator-less files from docs/TRANSCRIPTION.md's "Honest limits" as a
standing HAND item apiece, regardless of whether this particular release
touched Sony/Nikon/Minolta at all: Step 14 deliberately did not build a
generator for them, so no bump -- this one included -- refreshes them, and a
report that omitted them would misrepresent the automation level (see
OVERHAUL_OXIDEX_PLAN.md Step 17 and this repo's AGENTS.md). They are counted
into the totals precisely because "nothing changed here" is not the same
claim as "this is covered".

Which of them are *still* generator-less is DERIVED from the regen scripts
(`generator_less_files()`), not hard-coded. The hard-coded version of this
list is the exact defect that motivated the change: Step 18 added
`gen_sony_main_extra_tables.py` and `gen_minolta_a100_tables.py` at
2026-08-13T19:20:45-05:00 and wired both into `regen-all.sh` tier 2d, while
the last edit to this file (`cbc6618f`, 1 h 33 m later) left all six in the
literal, so every bump report inflated standing HAND work by 2.

This script does NOT attempt reachability (whether a transcribed table is
ever actually called by a parser -- see AGENTS.md "Detected is not parsed",
Step 28's reachability seam) or MakerNotes routing-array classification
beyond flagging that they changed; both need the running binary, not a JSON
diff, and are out of scope here by design.

Usage:
    triage_bump.py <old_dump.json> <new_dump.json> \\
        [--markdown-out report.md] [--json-out report.json]
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import codegen  # noqa: E402  -- reuse is_binary_table so classification cannot drift from the real generator
import conds  # noqa: E402  -- Step 23's Condition compiler; reused so a _variants classification cannot drift from compile_variant_group
import exprs  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]

# ---------------------------------------------------------------------------
# The six files docs/TRANSCRIPTION.md "Honest limits" names as targeting a
# bespoke, per-file Rust DSL hand-matched against ExifTool's Condition/
# RawConv/ValueConv/PrintConv text. THIS half stays a literal on purpose:
# "this file's contents were hand-translated through a vocabulary sized to
# one file" is a judgement about how the Rust was written, and there is
# nothing on disk to derive it from.
#
# Whether any given one of them still has NO generator is a different
# question, and that half IS derived -- see `generator_less_files()`.
BESPOKE_DSL_FILES = [
    ("Sony", "src/parsers/tiff/makernotes/sony/enciphered_tables.rs",
     "%Image::ExifTool::Sony::* (enciphered arrays)"),
    ("Sony", "src/parsers/tiff/makernotes/sony/plain_tables.rs",
     "%Image::ExifTool::Sony::* (plaintext arrays)"),
    ("Sony", "src/parsers/tiff/makernotes/sony/main_extra_tables.rs",
     "%Image::ExifTool::Sony::* (Main-table extras)"),
    ("Nikon", "src/parsers/tiff/makernotes/nikon/encrypted_tables.rs",
     "%Image::ExifTool::Nikon::*/NikonCustom::* (encrypted sections)"),
    ("Nikon", "src/parsers/tiff/makernotes/nikon/settings_tables.rs",
     "%NikonSettings::Main"),
    ("Minolta", "src/parsers/tiff/makernotes/minolta_a100_tables.rs",
     "%Image::ExifTool::Minolta::* (A100 subset)"),
]

# The committed scripts that regenerate tier-1 and tier-2 output. A file
# named by one of these has a generator; a file named by neither does not.
REGEN_SCRIPTS = (
    "tools/exiftool-tables/regen.sh",
    "tools/exiftool-tables/regen-all.sh",
)


def _strip_shell_comments(text: str) -> str:
    """Drop `#` comments from a shell script, respecting quotes.

    Necessary, not decorative: `regen-all.sh`'s tier-2d banner comment
    *names* the four files it explicitly did NOT build a generator for, so a
    plain substring search over the raw text concludes all six are wired.
    This is the same failure `reachability.py`'s docstring records for
    `ricoh.rs:215`, where a comment explaining that a `find_table(...)` call
    is NOT made got counted as a call site and allowlisted a table on the
    strength of a sentence (docs/reference/corpus-synthesis.md).
    """
    out = []
    for line in text.splitlines():
        quote = None
        cut = len(line)
        for i, ch in enumerate(line):
            if quote:
                if ch == quote:
                    quote = None
            elif ch in "'\"":
                quote = ch
            elif ch == "#" and (i == 0 or line[i - 1].isspace()):
                cut = i
                break
        out.append(line[:cut])
    return "\n".join(out)


def generator_less_files(root: Path = REPO_ROOT):
    """The `BESPOKE_DSL_FILES` entries no committed regen script regenerates.

    Derived rather than listed: the previous hard-coded literal went stale
    the same afternoon two of the six got generators (see module docstring),
    and a bump report that over-states standing HAND work is a measurement
    error in exactly the direction AGENTS.md warns about. A file drops off
    this list automatically the moment a regen script names its path.

    A missing regen script is a hard error, not a shrug: silently treating
    "cannot read regen-all.sh" as "nothing is wired" would flip every one of
    these back to HAND with no signal at all.
    """
    named = []
    for rel in REGEN_SCRIPTS:
        path = root / rel
        try:
            named.append(_strip_shell_comments(path.read_text(encoding="utf-8")))
        except OSError as exc:
            raise SystemExit(
                f"triage_bump.py: cannot read {rel} ({exc}); refusing to guess "
                "which generated files still have no generator -- that guess "
                "would silently inflate every bump report's HAND count."
            ) from exc
    body = "\n".join(named)
    return [entry for entry in BESPOKE_DSL_FILES if entry[1] not in body]


GENERATOR_LESS_FILES = generator_less_files()

# regen-all.sh tier 2a's manifest: (module, table) pairs codegen_subdirs.py
# already regenerates wholesale from the dump on every bump. A field-level
# change inside one of these is free (AUTO); a SubDirectory pointer to a
# table NOT in this set needs a human to decide whether/how to wire it in
# (HAND) -- mirrors regen-all.sh's own gen_subdir() calls exactly, so if that
# manifest grows, this one must grow with it.
SUBDIR_MANIFEST = {
    ("FujiFilm", "PrioritySettings"), ("FujiFilm", "FocusSettings"),
    ("FujiFilm", "AFCSettings"), ("FujiFilm", "DriveSettings"),
    ("Panasonic", "FaceDetInfo"), ("Panasonic", "FaceRecInfo"),
    ("Pentax", "SRInfo2"), ("Pentax", "FaceInfo"), ("Pentax", "AWBInfo"),
    ("Pentax", "TimeInfo"), ("Pentax", "LensCorr"), ("Pentax", "FlashInfo"),
    ("Pentax", "KelvinWB"), ("Pentax", "EVStepInfo"), ("Pentax", "FacePos"),
    ("Pentax", "FaceSize"), ("Pentax", "LevelInfo"), ("Pentax", "WBLevels"),
    ("Pentax", "LensInfoQ"), ("Pentax", "AFInfo"), ("Pentax", "BatteryInfo"),
    ("Pentax", "TempInfo"), ("Pentax", "ShotInfo"), ("Pentax", "FilterInfo"),
    ("Pentax", "CameraSettings"),
}

# Fields dump_tables.pl carries that are pure layout/metadata: the mechanical
# pass (codegen.py's gen_table) transcribes these with no translation step at
# all. Anything not in TAG_KEYS (see dump_tables.pl) is already excluded by
# construction -- only fields the dump actually carries reach this script.
DATA_TAG_FIELDS = {
    "Name", "Description", "Format", "Writable", "Count", "Groups", "Notes",
    "Mask", "BitShift", "Flags", "Unknown", "Hidden", "Avoid", "Binary",
    "Protected", "List", "Priority", "ByteOrder", "DataMember", "RelatedTag",
    "SeparateTable", "PrintHex", "Base", "Offset", "ChangeBase", "Require",
    "Desire", "Inhibit", "_extra_keys", "_shorthand",
}
CONV_FIELDS = {"PrintConv", "ValueConv", "RawConv", "PrintConvInv", "ValueConvInv"}

AUTO, EXPR, COND, HAND = "AUTO", "EXPR", "COND", "HAND"


class Delta:
    __slots__ = ("bucket", "module", "table", "tag", "field", "kind", "note")

    def __init__(self, bucket, module, table, tag, field, kind, note):
        self.bucket = bucket
        self.module = module
        self.table = table
        self.tag = tag
        self.field = field
        self.kind = kind  # "added" / "removed" / "changed"
        self.note = note

    def label(self):
        loc = f"{self.module}::{self.table}"
        if self.tag:
            loc += f"::{self.tag}"
        if self.field:
            loc += f".{self.field}"
        return loc


def conv_classification(old_conv, new_conv):
    """Classify a changed/added PrintConv-family field. Returns (bucket, note)."""
    conv = new_conv if new_conv is not None else old_conv
    if not isinstance(conv, dict):
        return HAND, "unrecognized conversion shape"
    kind = conv.get("kind")
    if kind in ("enum", "enum_partial"):
        return AUTO, "enum map -- mechanically transcribed"
    if kind == "expr":
        raw = conv.get("expr")
        if exprs.translate_or_compile_any(raw) is not None:
            return AUTO, "expression already translated/compiled by exprs.py"
        return EXPR, f"unsupported expression: {raw!r}"
    if kind == "code":
        return HAND, "deparsed Perl closure -- codegen.py never attempts kind=code"
    if kind == "list":
        return HAND, "list-shaped conversion -- codegen.py never attempts kind=list"
    return HAND, f"unclassified conversion kind={kind!r}"


def _cond_failure_reason(condition):
    """`conds.compile_cond` returns a bare `None` on refusal -- exactly what
    the AUTO/COND decision needs (and this script takes that decision
    verbatim from it, never re-derives it), but not enough to explain *why*
    to a human reading the report. Re-run the same atom compiler conds.py
    itself uses (`conds._compile_atom` / `conds._compile_setmember`) and let
    the first construct that fails explain itself via its own
    `CondCompileError` message.

    This duplicates only conds.py's trivial `and`-splitting (the same rule
    `compile_cond_atoms_conjunction`'s docstring already states), never the
    grammar itself -- what a construct means always comes from calling into
    conds.py's own compiler functions, so this cannot accept something
    conds.py would refuse, or vice versa. In the rare case duplicating the
    split logic itself goes stale, the worst outcome is a less precise
    *reason string*; the AUTO/COND bucket is decided elsewhere, by
    `conds.compile_cond` alone.
    """
    if not isinstance(condition, str) or not condition.strip():
        return "unrecognised condition shape (not a non-empty string)"
    text = re.sub(r"\s+", " ", condition.strip())
    try:
        if conds._compile_setmember(text) is not None:
            return "SetMember idiom -- should have compiled; conds.py disagreement, report a bug"
    except conds.CondCompileError as e:
        return str(e)
    atoms = [p.strip() for p in text.split(" and ")] if " and " in text else [text]
    for atom in atoms:
        try:
            conds._compile_atom(atom)
        except conds.CondCompileError as e:
            return str(e)
    return f"refused by conds.compile_cond for an unresolved reason: {condition!r}"


def classify_variants(module, table, tag_name, variants, kind):
    """Classify a `_variants` array exactly the way `codegen.py`'s
    `compile_variant_group` decides whether to emit it (Step 23): attempt
    `conds.compile_cond()` on every alternative's `Condition`, all-or-
    nothing -- AUTO the moment every alternative compiles, COND the moment
    one does not, naming the construct that defeated it.

    Mirrors `compile_variant_group`'s Condition handling exactly (same
    function, same closed grammar, same all-or-nothing rule, same refusal
    for a non-dict or nested-`_variants` alternative). It does NOT replicate
    `compile_variant_group`'s per-alternative `gen_field_literal` call
    (Format/Mask/Unknown/Name checks on each alternative's field shape) --
    this script already only approximates those checks for ordinary,
    non-variant fields too (see `classify_tag_field`'s DATA_TAG_FIELDS
    branch, which treats any layout field on a binary table as AUTO without
    re-deriving codegen.py's SIZED_RE/SCALAR_FORMATS matching), so a variant
    alternative gets the same level of scrutiny a plain field would get
    here. Consequence: a `_variants` array whose Conditions all compile but
    whose field shape `compile_variant_group` would separately refuse (an
    unsupported Format, say) is classified AUTO here even though the real
    generator would still drop it -- name that instrument
    (`triage_bump.py`'s Condition-only check) if this distinction matters
    for what you're deciding (AGENTS.md "name the instrument").
    """
    for i, alt in enumerate(variants):
        if not isinstance(alt, dict) or "_variants" in alt:
            shape = ("a nested _variants array" if isinstance(alt, dict)
                      else f"a non-dict shape ({type(alt).__name__})")
            return Delta(COND, module, table, tag_name, "_variants", kind,
                         f"alternative {i} is {shape} -- compile_variant_group refuses "
                         "this shape outright, before even looking at Condition")
        condition = alt.get("Condition")
        if conds.compile_cond(condition) is None:
            reason = _cond_failure_reason(condition)
            return Delta(COND, module, table, tag_name, "_variants", kind,
                         f"alternative {i} ({alt.get('Name', '?')!r})'s Condition is "
                         f"outside conds.py's closed grammar: {reason}")
    return Delta(AUTO, module, table, tag_name, "_variants", kind,
                 f"{len(variants)} conditional variants -- every alternative's Condition "
                 "compiles via conds.compile_cond, same all-or-nothing rule "
                 "compile_variant_group applies (Step 23)")


def classify_tag_field(module, table, tag_name, field, old_tag, new_tag, table_is_binary, kind):
    old_v = (old_tag or {}).get(field)
    new_v = (new_tag or {}).get(field)

    if field in CONV_FIELDS:
        bucket, note = conv_classification(old_v, new_v)
        return Delta(bucket, module, table, tag_name, field, kind, note)

    if field == "Condition":
        return Delta(COND, module, table, tag_name, field, kind,
                     "standalone Condition on a non-variant tag -- conds.py/Step 23 "
                     "only compiles a Condition found inside a _variants array's "
                     "alternatives, so a lone Condition here is omitted unconditionally, "
                     "not just until some later step lands")

    if field == "Hook":
        return Delta(HAND, module, table, tag_name, field, kind,
                     "Hook (mid-table format/byte-order rewrite) needs a HookEffect (Step 26)")

    if field == "SubDirectory":
        if (module, table) in SUBDIR_MANIFEST:
            return Delta(AUTO, module, table, tag_name, field, kind,
                         "table already in regen-all.sh's tier-2a manifest")
        return Delta(HAND, module, table, tag_name, field, kind,
                     "SubDirectory edge not in the tier-2a manifest -- needs wiring")

    if field in DATA_TAG_FIELDS:
        if table_is_binary or (module, table) in SUBDIR_MANIFEST:
            return Delta(AUTO, module, table, tag_name, field, kind,
                         "layout/metadata field -- mechanically transcribed")
        return Delta(HAND, module, table, tag_name, field, kind,
                     "layout field on a table the generator does not emit "
                     "(not ProcessBinaryData / not in the tier-2a manifest)")

    return Delta(HAND, module, table, tag_name, field, kind,
                 f"field {field!r} not modelled by this classifier")


def diff_tag(module, table, tag_name, old_tag, new_tag, table_is_binary):
    """Yield Deltas for one tag entry present in at least one of old/new."""
    if old_tag is None:
        # Whole tag is new. A variant array is classified by whether every
        # alternative's Condition compiles (see classify_variants) -- not
        # unconditionally COND regardless of its members, now that Step 23
        # gives conds.py a grammar to try them against.
        if isinstance(new_tag, dict) and "_variants" in new_tag:
            yield classify_variants(module, table, tag_name, new_tag["_variants"], "added")
            return
        if not isinstance(new_tag, dict):
            yield Delta(HAND, module, table, tag_name, None, "added",
                        "non-dict tag shorthand/unhandled shape")
            return
        for field in new_tag:
            if field.startswith("_"):
                continue
            yield classify_tag_field(module, table, tag_name, field, None, new_tag, table_is_binary, "added")
        if not any(f for f in new_tag if not f.startswith("_")):
            yield Delta(AUTO, module, table, tag_name, None, "added",
                        "new tag, name only -- mechanically transcribed")
        return

    if new_tag is None:
        yield Delta(AUTO, module, table, tag_name, None, "removed",
                     "tag removed upstream -- machinery drops it on regen")
        return

    if old_tag == new_tag:
        return  # no delta

    old_variants = isinstance(old_tag, dict) and "_variants" in old_tag
    new_variants = isinstance(new_tag, dict) and "_variants" in new_tag
    if new_variants:
        yield classify_variants(module, table, tag_name, new_tag["_variants"], "changed")
        return
    if old_variants:
        # The new shape dropped the model-dependent dispatch entirely (now a
        # plain tag, or a different shape) -- there is no `_variants` array
        # left for conds.py to accept or refuse, so this is not an AUTO/COND
        # question at all; flag it for a human look rather than guessing.
        yield Delta(HAND, module, table, tag_name, "_variants", "changed",
                     "tag lost its conditional _variants array entirely (now a plain "
                     "tag or a different shape) -- compile_variant_group has nothing "
                     "left to accept or refuse; needs a human look at what replaced it")
        return

    if not isinstance(old_tag, dict) or not isinstance(new_tag, dict):
        yield Delta(HAND, module, table, tag_name, None, "changed", "non-dict tag shape changed")
        return

    for field in sorted(set(old_tag) | set(new_tag)):
        if field.startswith("_"):
            continue
        if old_tag.get(field) == new_tag.get(field):
            continue
        yield classify_tag_field(module, table, tag_name, field, old_tag, new_tag, table_is_binary, "changed")


def diff_table(module, table_name, old_tbl, new_tbl):
    old_meta = (old_tbl or {}).get("meta") or {}
    new_meta = (new_tbl or {}).get("meta") or {}
    old_bin = codegen.is_binary_table(old_meta) if old_tbl else False
    new_bin = codegen.is_binary_table(new_meta) if new_tbl else False

    if old_tbl is None:
        # Brand-new table. Transcribable iff ProcessBinaryData (the generator's
        # only supported shape) or already named in the tier-2a manifest.
        if new_bin or (module, table_name) in SUBDIR_MANIFEST:
            yield Delta(AUTO, module, table_name, None, None, "added",
                         "new ProcessBinaryData table -- transcribed automatically "
                         "(reachability from a parser is a separate concern, not "
                         "checked here -- see AGENTS.md 'Detected is not parsed')")
        else:
            yield Delta(HAND, module, table_name, None, None, "added",
                         "new non-binary table -- generator only emits ProcessBinaryData tables")
        for tag_name, new_tag in (new_tbl.get("tags") or {}).items():
            yield from diff_tag(module, table_name, tag_name, None, new_tag, new_bin)
        return

    if new_tbl is None:
        yield Delta(AUTO, module, table_name, None, None, "removed",
                     "table removed upstream -- machinery drops it on regen")
        return

    if old_meta.get("PROCESS_PROC") != new_meta.get("PROCESS_PROC"):
        yield Delta(HAND, module, table_name, None, "PROCESS_PROC", "changed",
                     f"table's own binary-vs-not classification changed "
                     f"(is_binary_table: {old_bin} -> {new_bin})")

    for field in sorted(set(old_meta) | set(new_meta)):
        if field == "PROCESS_PROC" or old_meta.get(field) == new_meta.get(field):
            continue
        bucket = AUTO if (new_bin or (module, table_name) in SUBDIR_MANIFEST) else HAND
        note = ("table metadata field -- mechanically transcribed" if bucket == AUTO
                else "table metadata field on a table the generator does not emit")
        yield Delta(bucket, module, table_name, None, field, "changed", note)

    old_tags = old_tbl.get("tags") or {}
    new_tags = new_tbl.get("tags") or {}
    for tag_name in sorted(set(old_tags) | set(new_tags)):
        yield from diff_tag(module, table_name, tag_name, old_tags.get(tag_name),
                             new_tags.get(tag_name), new_bin or old_bin)


def diff_array(module, array_name, old_arr, new_arr):
    """MakerNotes-style routing arrays (e.g. MakerNotes::Main). No generator
    consumes these at all today -- gen_staleness_facts.py regenerates a
    fixture from them, but the hand-written dispatcher Rust that fixture
    checks against still needs a human update. Always HAND."""
    old_rows = (old_arr or {}).get("rows")
    new_rows = (new_arr or {}).get("rows")
    if old_rows == new_rows:
        return
    if old_arr is None:
        yield Delta(HAND, module, array_name, None, None, "added",
                     f"new routing array ({new_arr.get('row_count', '?')} rows) -- "
                     "dispatcher Rust needs a matching hand update; "
                     "'just check-staleness' regenerates the detection fixture only")
    elif new_arr is None:
        yield Delta(HAND, module, array_name, None, None, "removed",
                     "routing array removed upstream -- dispatcher Rust needs review")
    else:
        yield Delta(HAND, module, array_name, None, None, "changed",
                     f"routing array changed ({old_arr.get('row_count', '?')} -> "
                     f"{new_arr.get('row_count', '?')} rows) -- dispatcher Rust needs review")


def diff_module(module, old_mod, new_mod):
    old_tables = (old_mod or {}).get("tables") or {}
    new_tables = (new_mod or {}).get("tables") or {}
    for table_name in sorted(set(old_tables) | set(new_tables)):
        yield from diff_table(module, table_name, old_tables.get(table_name), new_tables.get(table_name))

    old_arrays = (old_mod or {}).get("arrays") or {}
    new_arrays = (new_mod or {}).get("arrays") or {}
    for array_name in sorted(set(old_arrays) | set(new_arrays)):
        yield from diff_array(module, array_name, old_arrays.get(array_name), new_arrays.get(array_name))


def run_triage(old_doc, new_doc):
    old_mods = old_doc.get("modules") or {}
    new_mods = new_doc.get("modules") or {}
    deltas = []
    for module in sorted(set(old_mods) | set(new_mods)):
        old_mod = old_mods.get(module)
        new_mod = new_mods.get(module)
        if old_mod is None:
            deltas.append(Delta(HAND, module, None, None, None, "added",
                                 "new module -- needs parser/dispatch wiring before any "
                                 "table in it is reachable, even if individually "
                                 "transcribable (see AGENTS.md 'Detected is not parsed')"))
        elif new_mod is None:
            deltas.append(Delta(HAND, module, None, None, None, "removed",
                                 "module removed upstream -- check whether OxiDex still "
                                 "dispatches to it"))
        deltas.extend(diff_module(module, old_mod, new_mod))

    for module, path, source in GENERATOR_LESS_FILES:
        deltas.append(Delta(HAND, module, path, None, None, "standing",
                             f"generator-less file ({source}, docs/TRANSCRIPTION.md 'Honest "
                             "limits') -- Step 14 deliberately built no generator for it, so "
                             "this bump (or any bump) does not refresh it regardless of "
                             "whether its source module changed"))
    return deltas


def summarize(deltas):
    counts = Counter(d.bucket for d in deltas)
    by_module = defaultdict(Counter)
    for d in deltas:
        by_module[d.module][d.bucket] += 1
    total = sum(counts.values())
    auto_share = (counts[AUTO] / total) if total else 1.0
    return counts, by_module, total, auto_share


def render_markdown(old_ver, new_ver, deltas, corpus_note=None):
    counts, by_module, total, auto_share = summarize(deltas)
    lines = []
    lines.append(f"# Bump triage report: ExifTool {old_ver} -> {new_ver}\n")
    lines.append(f"Instrument: `tools/exiftool-tables/triage_bump.py` diffing "
                  f"`dump_tables.pl` JSON for {old_ver} against {new_ver}.\n")
    lines.append("## Summary\n")
    lines.append(f"- Total classified deltas: **{total}**")
    for b in (AUTO, EXPR, COND, HAND):
        pct = (counts[b] / total * 100) if total else 0.0
        lines.append(f"- {b}: **{counts[b]}** ({pct:.1f}%)")
    lines.append(f"\n**AUTO share: {auto_share:.1%}** "
                 f"({counts[AUTO]} of {total} deltas absorbed with zero source edits)\n")
    if corpus_note:
        lines.append(corpus_note + "\n")

    lines.append("## Standing HAND items (generator-less files)\n")
    lines.append("Per `docs/TRANSCRIPTION.md` \"Honest limits\" -- listed on every bump "
                  "regardless of whether this release touched them, and included in the "
                  f"HAND count above ({len(GENERATOR_LESS_FILES)} of {counts[HAND]}):\n")
    for module, path, source in GENERATOR_LESS_FILES:
        lines.append(f"- `{path}` ({source})")
    lines.append("")

    lines.append("## By module (top 40 by delta count)\n")
    lines.append("| module | AUTO | EXPR | COND | HAND | total |")
    lines.append("|---|---:|---:|---:|---:|---:|")
    ranked = sorted(by_module.items(), key=lambda kv: -sum(kv[1].values()))
    for module, c in ranked[:40]:
        tot = sum(c.values())
        lines.append(f"| {module} | {c[AUTO]} | {c[EXPR]} | {c[COND]} | {c[HAND]} | {tot} |")
    lines.append("")

    for bucket, heading in ((EXPR, "## EXPR deltas (need an expression translation)"),
                             (COND, "## COND deltas (need a condition/dispatch)"),
                             (HAND, "## HAND deltas (need human work)")):
        items = [d for d in deltas if d.bucket == bucket and d.kind != "standing"]
        standing_note = (f" (plus {len(GENERATOR_LESS_FILES)} standing generator-less "
                          f"files listed above; {counts[bucket]} total)"
                          if bucket == HAND else "")
        lines.append(f"\n{heading} -- {len(items)}{standing_note}\n")
        for d in items[:200]:
            lines.append(f"- `{d.label()}` ({d.kind}): {d.note}")
        if len(items) > 200:
            lines.append(f"- ... and {len(items) - 200} more")
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("old_dump")
    ap.add_argument("new_dump")
    ap.add_argument("--markdown-out")
    ap.add_argument("--json-out")
    ap.add_argument("--corpus-note", help="free-text note about which corpus/floors backed the conformance run, echoed into the markdown report")
    args = ap.parse_args()

    with open(args.old_dump, encoding="utf-8") as fh:
        old_doc = json.load(fh)
    with open(args.new_dump, encoding="utf-8") as fh:
        new_doc = json.load(fh)

    old_ver = old_doc.get("exiftool_version", "?")
    new_ver = new_doc.get("exiftool_version", "?")

    deltas = run_triage(old_doc, new_doc)
    counts, by_module, total, auto_share = summarize(deltas)

    print(f"ExifTool {old_ver} -> {new_ver}: {total} classified deltas")
    for b in (AUTO, EXPR, COND, HAND):
        print(f"  {b:5s} {counts[b]:6d}")
    print(f"AUTO share: {auto_share:.1%}")

    if args.markdown_out:
        md = render_markdown(old_ver, new_ver, deltas, args.corpus_note)
        Path(args.markdown_out).write_text(md, encoding="utf-8")
        print(f"wrote {args.markdown_out}")

    if args.json_out:
        payload = {
            "old_version": old_ver,
            "new_version": new_ver,
            "total": total,
            "counts": dict(counts),
            "auto_share": auto_share,
            "by_module": {m: dict(c) for m, c in by_module.items()},
            "deltas": [
                {"bucket": d.bucket, "module": d.module, "table": d.table,
                 "tag": d.tag, "field": d.field, "kind": d.kind, "note": d.note}
                for d in deltas
            ],
        }
        Path(args.json_out).write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")
        print(f"wrote {args.json_out}")


if __name__ == "__main__":
    main()
