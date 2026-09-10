#!/usr/bin/env python3
"""Classify every JSON-to-JSON delta between two `dump_tables.pl` dumps.

Step 17 of the tag-machinery overhaul: `just bump-exiftool` snapshots the old
release's dump before regenerating, and this script diffs that snapshot
against the new release's dump to answer "how much of this release did the
machinery absorb unaided, and what is left for a human". It is the work
queue that replaces fleet rediscovery (OVERHAUL_OXIDEX_PLAN.md Step 17).

Every changed/added/removed tag, table or module lands in exactly one of
four buckets, matching what the CURRENT generators (tools/exiftool-tables/
codegen.py for ProcessBinaryData and IFD-style tables, exprs.py for conversions,
regen-all.sh's tier 2 for the named MakerNote sub-directory manifest) can
and cannot do today. It calls their selectors and compilers directly;
the stricter IFD checks and the legacy binary approximations are described
below:

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
           closed grammar (a parenthesised group, an `lt`/`ge` string
           compare, a `\\d`/`\\w`-shorthand regex class, ...) -- refused
           exactly like `codegen.py`'s `compile_variant_group` refuses it,
           all-or-nothing for the whole array, same as the AUTO case above
           but failed; or (3) a `Hook` (mid-table format/byte-order
           rewrite), which is still genuinely unwired -- Step 26, not Step
           23. Step 23 landing narrowed this bucket to Hook plus
           grammar-refused conditions; it did not empty it.
  HAND  -- anything else: a new module or a table outside the generators'
           supported shapes, a new SubDirectory edge
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

IFD deltas use the real per-tag/variant emitter, including its refusal and
withholding counters, and require changed facts to affect its output. This
is deliberately stricter than the legacy binary DATA_TAG_FIELDS and
Condition-only variant checks described above. A selected IFD table can be
empty after refusals; selection or Gate A alone never earns AUTO here.
There is no oracle PASS ledger input to this script, so IFD expressions
requiring that evidence remain HAND (or EXPR if the grammar refuses them).
HAND can therefore mean validation/review, not necessarily new source code.

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
import codegen  # noqa: E402  -- reuse table selectors and the IFD emitter/refusal logic
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
    to a human reading the report. `conds.refusal_reason` runs the same
    compiler once more and returns the first `CondCompileError` message,
    so the construct named here is the one conds.py's own parser refused
    -- there is no second grammar to go stale (this used to duplicate the
    `and`-splitting; slice I-3's tokenizer/precedence parser made that
    heuristic wrong for `or`/`||`, so the duplication went)."""
    if not isinstance(condition, str) or not condition.strip():
        return "unrecognised condition shape (not a non-empty string)"
    reason = conds.refusal_reason(condition)
    if reason is None:
        return "compiles under conds.py -- classify_variants disagreement, report a bug"
    return reason


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


# Positive counters accepted by the IFD declaration audit. Every other
# nonzero counter, including newly introduced ones, requires review. This
# keeps new refusals fail-closed without copying the generator's grammars.
# In particular Gate A excludes several honest omissions, so it cannot
# answer whether an upgrade declaration has actually been absorbed.
IFD_TRANSCRIBED_COUNTERS = frozenset({
    "ifd_format_unsized", "ifd_flag_unknown", "ifd_raw_conv_set_member",
    "enum_int", "enum_int_printhex", "enum_str", "enum_empty",
    "expr_translated", "expr_compiled", "expr_translated_code_ref",
    "value_conv_compiled", "bitmask_emitted", "other_translated",
    "tag_group_override", "tag_variant_emitted", "ifd_variant_alternatives",
    "ifd_subdir_edge_modeled", "ifd_subdir_edge_target_ifd",
    "ifd_subdir_edge_target_binary", "ifd_subdir_edge_sub_ifd",
    "ifd_subdir_edge_makernotes",
})


def _ifd_emission(tag_name, tag, meta, ctx):
    """Return (bucket, explanation, source) from the actual IFD emitter.

    No oracle evidence is invented: verified_exprs=None preserves its
    per-domain/oracle refusal. A modeled SubDirectory's omitted value is
    intentional (its bytes are a pointer); every other omission is work.
    """
    tag_id = codegen.parse_ifd_tag_id(tag_name)
    if tag_id is None:
        return HAND, "IFD tag ID unrepresentable; generator omits the entry", None
    if not isinstance(tag, dict):
        return HAND, "IFD non-dict tag shape", None
    stats = codegen.new_ifd_stats()
    if "_variants" in tag:
        if not isinstance(tag["_variants"], list) or not tag["_variants"]:
            return HAND, "IFD empty or non-list _variants shape", None
        src = codegen.compile_ifd_variant_group(tag, tag_id, stats, None, ctx, meta)
    else:
        src, _reason = codegen.gen_ifd_tag_literal(tag, tag_id, stats, None, ctx, meta)
    blocked = {k: v for k, v in stats.items() if v and k not in IFD_TRANSCRIBED_COUNTERS}
    if stats["omitted_subdirectory"] == stats["ifd_subdir_edge_modeled"]:
        blocked.pop("omitted_subdirectory", None)
    if blocked or src is None:
        bucket = HAND
        if stats["omitted_condition"] or stats["tag_variant_cond_unsupported"]:
            bucket = COND
        elif (stats["value_conv_refused_shape"] or stats["expr_unsupported"]
              or stats["ifd_print_conv_withheld_by"].get("expr_unsupported")):
            bucket = EXPR
        note = "IFD generator refuses/withholds: " + ", ".join(
            f"{key}={value}" for key, value in sorted(blocked.items()))
        if "oracle" in note:
            note += "; oracle evidence not supplied to triage -- validation required, not proof of a source-code gap"
        return bucket, note, src
    return AUTO, "IFD declaration emitted without refusal; reachability/activation not checked", src


def _ifd_reverted(tag, field, old_tag):
    trial = dict(tag)
    if old_tag is not None and field in old_tag:
        trial[field] = old_tag[field]
    else:
        trial.pop(field, None)
    return trial


def _ifd_field_trials(tag, field, old_tag):
    """Probe a field and each flag/group fact, without copying their schema.

    An ignored flag next to Unknown, or family 3 next to family 1, must not
    earn AUTO just because the supported half changes the generated source.
    """
    yield _ifd_reverted(tag, field, old_tag)
    if field in tag:
        yield _ifd_reverted(tag, field, None)
    value = tag.get(field)
    if field in {"Flags", "Groups", "GROUPS", "_extra_keys"}:
        if isinstance(value, dict):
            for key in value:
                yield {**tag, field: {k: v for k, v in value.items() if k != key}}
        elif isinstance(value, list):
            for index in range(len(value)):
                yield {**tag, field: value[:index] + value[index + 1:]}


def diff_ifd_tag(module, table, tag_name, old_tag, new_tag, meta, ctx):
    if new_tag is None:
        yield Delta(AUTO, module, table, tag_name, None, "removed",
                    "IFD tag removed upstream -- machinery drops it on regen")
        return
    if new_tag == old_tag:
        return
    kind = "added" if old_tag is None else "changed"
    bucket, note, src = _ifd_emission(tag_name, new_tag, meta, ctx)
    if not isinstance(new_tag, dict) or (old_tag is not None and not isinstance(old_tag, dict)):
        yield Delta(HAND, module, table, tag_name, None, kind, "IFD non-dict tag shape")
        return
    if "_variants" in new_tag:
        # The emitter checks both Conditions AND alternative field shapes.
        # Also catch facts it silently ignores, such as an alternative Mask.
        if bucket == AUTO:
            for index, alt in enumerate(new_tag["_variants"]):
                for field in alt:
                    if field.startswith("_") and field != "_extra_keys":
                        continue
                    for alt_trial in _ifd_field_trials(alt, field, None):
                        trial = {"_variants": list(new_tag["_variants"])}
                        trial["_variants"][index] = alt_trial
                        if _ifd_emission(tag_name, trial, meta, ctx)[2] == src:
                            bucket, note = HAND, f"IFD alternative {index} field {field!r} is not reflected in emission"
                            break
                    if bucket != AUTO:
                        break
                if bucket != AUTO:
                    break
        yield Delta(bucket, module, table, tag_name, "_variants", kind, note)
        return
    if old_tag is not None and "_variants" in old_tag:
        yield Delta(HAND, module, table, tag_name, "_variants", kind,
                    "IFD tag lost conditional variants -- review the replacement")
        return
    fields = [f for f in sorted(set(old_tag or {}) | set(new_tag))
              if (not f.startswith("_") or f == "_extra_keys")
              and (old_tag is None or old_tag.get(f) != new_tag.get(f))]
    for field in fields:
        field_bucket, field_note = bucket, note
        if bucket == AUTO:
            if any(_ifd_emission(tag_name, trial, meta, ctx)[2] == src
                   for trial in _ifd_field_trials(new_tag, field, old_tag)):
                field_bucket, field_note = HAND, f"IFD field {field!r} is not reflected in emission; review required"
        yield Delta(field_bucket, module, table, tag_name, field, kind, field_note)
    if not fields:
        yield Delta(HAND, module, table, tag_name, None, kind,
                    "IFD internal-only tag change -- review required")


def diff_ifd_table(module, table_name, old_tbl, new_tbl, ctx):
    meta = new_tbl.get("meta") or {}
    old_meta = (old_tbl or {}).get("meta") or {}
    old_tags = (old_tbl or {}).get("tags") or {}
    new_tags = new_tbl.get("tags") or {}
    tags = [d for key in sorted(set(old_tags) | set(new_tags))
            for d in diff_ifd_tag(module, table_name, key, old_tags.get(key),
                                  new_tags.get(key), meta, ctx)]
    metadata = []
    for field in sorted(set(old_meta) | set(meta)):
        if old_meta.get(field) == meta.get(field):
            continue
        if field == "PROCESS_PROC":
            if old_tbl is not None:
                metadata.append(Delta(HAND, module, table_name, None, field, "changed",
                                      "PROCESS_PROC identity changed -- review table selection"))
            continue
        # Compare the real table emitter, including AVOID's effect on tags.
        current_src = codegen.gen_ifd_table(module, table_name, new_tbl,
                                           codegen.new_ifd_stats(), None, ctx)
        bucket = AUTO
        for trial_meta in _ifd_field_trials(meta, field, old_meta):
            prior_src = codegen.gen_ifd_table(module, table_name, {**new_tbl, "meta": trial_meta},
                                             codegen.new_ifd_stats(), None, ctx)
            if current_src == prior_src:
                bucket = HAND
                break
        metadata.append(Delta(bucket, module, table_name, None, field,
                              "added" if old_tbl is None else "changed",
                              "IFD table metadata reflected in emission" if bucket == AUTO
                              else "IFD table metadata not reflected in emission; review required"))
    if old_tbl is None:
        bucket = AUTO if all(d.bucket == AUTO for d in tags + metadata) else HAND
        yield Delta(bucket, module, table_name, None, None, "added",
                    "new IFD table -- declarations emitted; reachability/activation not checked"
                    if bucket == AUTO else "new IFD table selected but some declarations still require review")
    yield from metadata
    yield from tags


def diff_table(module, table_name, old_tbl, new_tbl, ifd_ctx=None):
    old_meta = (old_tbl or {}).get("meta") or {}
    new_meta = (new_tbl or {}).get("meta") or {}
    old_bin = codegen.is_binary_table(old_meta) if old_tbl else False
    new_bin = codegen.is_binary_table(new_meta) if new_tbl else False

    if new_tbl is not None and not new_bin and codegen.is_ifd_table(new_meta):
        if ifd_ctx is None:
            ifd_ctx = codegen.IfdGenContext.from_doc({"modules": {
                module: {"tables": {table_name: new_tbl}}}})
        yield from diff_ifd_table(module, table_name, old_tbl, new_tbl, ifd_ctx)
        return

    if old_tbl is None:
        # IFD tables were handled above. The remaining supported shapes are
        # ProcessBinaryData and tables already in the tier-2a manifest.
        if new_bin or (module, table_name) in SUBDIR_MANIFEST:
            yield Delta(AUTO, module, table_name, None, None, "added",
                         "new ProcessBinaryData table -- transcribed automatically "
                         "(reachability from a parser is a separate concern, not "
                         "checked here -- see AGENTS.md 'Detected is not parsed')")
        else:
            yield Delta(HAND, module, table_name, None, None, "added",
                         "new custom table/procedure outside binary/IFD selection "
                         "and the tier-2a manifest -- needs review")
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


def diff_module(module, old_mod, new_mod, ifd_ctx=None):
    old_tables = (old_mod or {}).get("tables") or {}
    new_tables = (new_mod or {}).get("tables") or {}
    for table_name in sorted(set(old_tables) | set(new_tables)):
        yield from diff_table(module, table_name, old_tables.get(table_name), new_tables.get(table_name), ifd_ctx)

    old_arrays = (old_mod or {}).get("arrays") or {}
    new_arrays = (new_mod or {}).get("arrays") or {}
    for array_name in sorted(set(old_arrays) | set(new_arrays)):
        yield from diff_array(module, array_name, old_arrays.get(array_name), new_arrays.get(array_name))


def run_triage(old_doc, new_doc):
    old_mods = old_doc.get("modules") or {}
    new_mods = new_doc.get("modules") or {}
    ifd_ctx = codegen.IfdGenContext.from_doc({"modules": new_mods})
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
        deltas.extend(diff_module(module, old_mod, new_mod, ifd_ctx))

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
