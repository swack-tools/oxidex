#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Mechanically re-verify one tag-fix commit against its evidence trailers.

This is the M1 half of the commit contract from
docs/plans/specs/2026-07-24-fleet-knowledge-and-scaling-design.md: workers
emit evidence trailers on every fix commit (Format / Tag / Sample /
Exiftool-Value / Oxidex-Value / Perl-Ref / Verified / Worker / Table), and
this script re-checks that evidence *mechanically* -- no model calls, no
judgment -- so the squad merger and the overlord sweep can route commits
without a human re-deriving anything. Unlike the critiqued single-sample
version, it is built to catch rejection class (a): a fix that produces the
right value on the one sample the worker looked at and the wrong value on
every other sample carrying the same tag.

Checks (each independent; all always run, none short-circuits another):

  trailers      Every required trailer key must be present (parsed with
                `git interpret-trailers --parse`, so folded/multi-line
                trailers behave exactly as git itself sees them).
                Missing key -> flag "missing-trailer:<key>".

  multi_sample  For every `Tag:` trailer, grep the per-file exiftool
                outputs in --samples-cache to find ALL sample files
                carrying that tag -- not just the one the `Sample:`
                trailer names -- then run the comparison runner against
                each. Any carrying sample where the tag does not match
                post-commit -> flag "multi-sample-fail:<tag>". A tag with
                zero carriers in the cache (the Sample: trailer's own
                evidence cannot be reconciled) -> flag
                "multi-sample-no-carriers:<tag>". Skipped (recorded as
                such, never silently) when no cache or no runner is
                supplied.

  printconv     PrintConv-vs-Perl byte check: every quoted string VALUE
                the diff ADDS inside map-like structures must appear
                byte-identical somewhere in the `Perl-Ref` module file
                under --perl-lib. Any miss -> flag
                "printconv-mismatch:<value-excerpt>".
                extract_added_map_values defines "map-like" exactly and
                is the authority; it covers `=>` arms AND the bodies of
                block-bodied arms (`260 => { "...".to_string() }`),
                const/static array literals, `const_decoder!` tables,
                inline `for ... in [...]` tables, bare mid-table
                elements (`"Value",` / `(12, "Value"),`, trailing
                comments stripped) and `.insert(<key>, "<value>")`.
                Values we cannot verify -- a runtime-built string
                (format!/write!/... : the literal is a TEMPLATE, not a
                value), a missing --perl-lib, an unresolvable Perl-Ref
                module -> flag "printconv-unverifiable". Per spec open
                question 6 this deliberately over-flags: computed
                PrintConvs always force the human queue, never pass
                silently.

                NOT unverifiable, deliberately: the quoted argument of
                an ordinary function call. `Some("Centered".to_string())`,
                `String::from("sRGB")` and `.insert(k, "Fine")` are all
                function calls carrying genuine display values, so they
                are extracted and byte-checked like any other. (This
                paragraph exists because the docstring used to promise
                "function calls" were reported unverifiable, which the
                code never did -- only the macro half was implemented.
                Measured 2026-07-26 over the 50 commits then ahead of
                origin/main on the 74 worker branches: 13 are
                printconv-flagged and 0 of the 13 would clear if
                call-argument strings were skipped, so the promise was
                costing nothing and protecting nothing.)

  ownership     If --squads-toml maps squads to file globs, every file the
                diff touches outside the committing squad's globs (squad =
                `Worker:` trailer minus its trailing "-<n>") -> flag
                "ownership:<file>". WARN-ONLY per spec M1: these flags are
                reported but never fail the commit -- they do not flip
                `ok` and do not change the exit code.

Also computes `git patch-id --stable` for the commit -- the merger and
sweep record patch-ids (not SHAs) because cherry-pick and rebase-merge
rewrite SHAs.

Output: {ok, flags: [], patch_id, checks: {trailers, multi_sample,
printconv, ownership}} (JSON with --json, human-readable lines otherwise).

Exit codes: 0 clean (ownership warnings allowed), 2 flagged -- the caller
must route the commit to the human queue; flags NEVER auto-resolve --
1 operational error (bad sha, unreadable config, missing cache dir).

Usage:
    uv run scripts/validate_fix_commit.py <sha> --repo <path> \\
        [--samples-cache /tmp/oxidex-exiftool-cache] \\
        [--squads-toml scripts/squads.toml] \\
        [--comparison-cmd "path/to/compare-one"] \\
        [--perl-lib <dir-with-ExifTool-pm-files>] [--json]

Everything side-effectful is injectable for hermetic tests: the git runner
(run_git), the comparison runner (comparison_fn), and every path.
"""
import argparse
import fnmatch
import functools
import json
import re
import xml.etree.ElementTree as ET  # nosec B405 -- parses only local exiftool output
import shlex
import subprocess
import sys
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from exiftool_oracle import shared as shared_exiftool_oracle  # noqa: E402

# Trailer keys per spec M1 (shared convention across the fleet scripts:
# git_commit writes them, this script and overlord_sweep read them).
#
# "Table" is deliberately NOT in this list: model_fix_loop.py's
# _build_fix_gap_trailers only emits it when a table_name is passed in,
# and every ordinary per-tag fix_gap call site passes table_name=None --
# it's a T3 table-port-job concept, not evidence a regular tag fix can
# ever produce. Requiring it unconditionally meant every single ordinary
# fix commit failed this check forever (confirmed live: 100% of entries
# in quarantine.jsonl carried a missing-trailer:Table flag, and no
# ordinary fix_gap commit has ever reached origin/main past this gate) --
# a real fleet-wide publication blocker, not a quality signal.
REQUIRED_TRAILERS = (
    "Format",
    "Tag",
    "Sample",
    "Exiftool-Value",
    "Oxidex-Value",
    "Perl-Ref",
    "Verified",
    "Worker",
)

# The same list minus Perl-Ref, used when the diff contains no PrintConv
# value for that trailer to attest. See check_trailers.
CONDITIONAL_TRAILERS = tuple(k for k in REQUIRED_TRAILERS if k != "Perl-Ref")

# Bump this WHENEVER a change here can turn a previously-rejected commit
# into an accepted one -- new/removed REQUIRED_TRAILERS, any change to
# what extract_added_map_values extracts, any change to check_printconv's
# verdicts, or a new WARN_ONLY prefix.
#
# The merger stamps this version onto every quarantine record, and treats
# a head quarantined under a DIFFERENT version as eligible for one fresh
# attempt (see squad_merge_loop.candidate_commits). Without it, quarantine
# is terminal: the 2026-07-25 extractor fixes made 20 of 44 quarantined
# heads admissible, and not one of them would ever have been re-examined,
# because both the squad-status heads map and the patch-id ledger reject
# a known sha before the validator is ever consulted.
#
# History:
#   1  original M1 gate
#   2  #114 dropped the Table trailer requirement
#   3  #119 added the tag-key / byte-string identifier exclusions
#   4  match arms, &[&str] registries, format! templates, test-code
#      scoping, wrong-perl-ref warn-only, conditional Perl-Ref
#   5  hardening: printconv-wrong-perl-ref blocks again, the corpus is
#      restricted to real tag tables, the mod-tests hunk gate is gone,
#      the &[&str] registry gate is keyed on the declaration NAME, and
#      non-source paths are rejected outright
#   6  extractor evasion holes closed (2026-07-26): block-bodied match
#      arms (the shape rustfmt itself produces at 71+ chars of display
#      value), `const_decoder!`/inline-`for ... in [...]` tables, bare
#      `(key, "value")` mid-table elements, `.insert(k, "value")`, and
#      trailing `//` / `/* */` comments on bare table elements. Each was
#      a shape for which extract_added_map_values returned ([], []), so
#      validate_commit said ok=True AND
#      overlord_sweep.classify_for_judgment_queue -- which shares this
#      extractor -- returned no reason, i.e. the commit shipped as
#      machine_accepted with no human ever seeing the value
#   7  evidence trailers are checked for TRUTH, not just presence
#      (2026-07-27): a JPEG fix passed the whole gate citing
#      `Perl-Ref: NikonCustom.pm` for six APP12 tags that module does not
#      contain, and `Sample: ExifTool.jpg`, an Agfa file four of the six
#      tags never appear in and whose bytes the edited code path does not
#      even route (the APP12 router compares b"AGFA"; the file says
#      "Agfa"). Zero of its six tags were reachable through its own cited
#      evidence. This matters most because the judgment-queue daemon
#      re-derives trailers to clear the ~378 missing-trailer quarantines,
#      so a trailer that can be plausible and false is a mechanism for
#      manufacturing evidence at scale
#
# NOTE ON DIRECTION: the paragraph above frames a bump as "a rejection
# can become an acceptance", because that is the case the merger's retry
# machinery acts on -- it re-offers QUARANTINED heads. A STRICTER change
# (like the non-source-file gate) has no retry surface at all: a head
# already recorded "consumed" is published and is never reconsidered, by
# design. Bump for either direction anyway. Marking the version is how a
# human reading quarantine.jsonl months from now knows which ruleset
# produced a verdict, and a stricter rule still changes what a re-offered
# head is measured against.
#
# v8 adds `tag-name-is-a-printconv-value`: a Tag: trailer naming something
#      ExifTool prints as a display string for some numeric key, and defines
#      as a tag nowhere. Three such names shipped -- `Higher resolution image
#      exists`, `Trilinear`, `Creative (Slow speed)` -- because every check
#      here compared a numeric key to its display string, and none asked
#      whether the NAME was a tag at all. That axis was unguarded end to end.
POLICY_VERSION = 8

# Flags that are recorded for the human record but do NOT block
# admission. `ownership:` says the fix landed outside the worker's squad
# globs -- true but not a defect in the fix.
#
# `printconv-wrong-perl-ref:` used to be here and is deliberately NOT any
# more. Measured 2026-07-26 over 9,600 wrong-table trials drawn from
# 11,884 real ExifTool PrintConv strings and weighted by the Perl-Ref
# distribution in actual worker commits: checking the CITED module blocks
# 97.3% of them, and accepting any whole-corpus hit gives that up. Worse,
# the flag was warn-only AND discarded on accept, so the fabrication
# signal went nowhere at all. The distinct flag name is still worth
# keeping -- it tells a human "real string, wrong module" instead of
# "invented string" -- but it must route to the queue, not to main.
WARN_ONLY_FLAG_PREFIXES = ("ownership:",)

# Where a tag fix is allowed to write. Anything else is a blocking
# non-source-file flag.
#
# Derived from measurement, not taste. Across every commit currently
# ahead of origin/main on the 74 worker branches (2026-07-26) there are
# exactly 17 distinct touched paths: 10 under src/, 2 under tests/, 1
# under oxidex-tags-core/, 3 under scripts/, and one
# `config.toml.bak-pre-gpt55` -- a local config BACKUP that a worker
# dropped in its worktree and committed alongside a real tag fix
# (85a24f04390d on model-fix-parallel-standards-appn-1, 163 lines). That
# commit validated CLEAN, because the only path-aware check was
# check_ownership and ownership: is warn-only, so the backup would have
# been swept into a PR and merged to main. Sibling droppings are already
# loose in other worktrees: config.toml.bak-medium, .bak-pre-pin,
# .bak-pre-terra, .bak-2026-07-25.
#
# The scripts/ hits are the OTHER thing this catches, and it is the more
# valuable half: they belong to two fleet-INFRASTRUCTURE commits
# (7a5dd662 "tuning: nudge fixer toward earlier patch attempts" and
# 93994f59 "fix(fleet): consume handshake never unblocks a worker") that
# the merger routed through a TAG-FIX evidence validator. Each was then
# written into all 14 squads' ledgers, so those two commits alone account
# for 28 of the 77 quarantine entries, showing up as eight
# missing-trailer flags apiece instead of the one true statement: this is
# not a tag fix. Naming it precisely is worth more than the eight
# misleading flags.
#
# Cargo.toml/Cargo.lock are allowed although no worker has yet touched
# them: a tag fix that genuinely needs a dependency is plausible, and a
# fabricated PrintConv value cannot hide in a manifest. benches/ and
# bindings/ are allowed for the same reason -- they are real parts of the
# crate a fix could legitimately extend.
FIX_COMMIT_PATH_PREFIXES = (
    "src/",
    "tests/",
    "docs/",
    "benches/",
    "bindings/",
)
FIX_COMMIT_PATH_EXACT = ("Cargo.toml", "Cargo.lock")
# Tag-definition crates are versioned per family (oxidex-tags-core,
# -camera, -image, ...), so they are matched by prefix rather than listed.
_TAG_CRATE_PREFIX = "oxidex-tags"

# How much of a mismatched PrintConv value to embed in its flag. Full
# values can be long (lens descriptions); the excerpt is for humans
# scanning the queue, the full value is still in the diff.
PRINTCONV_EXCERPT_CHARS = 48

# One Rust/Perl-style double-quoted string literal, escape-aware.
_QUOTED_RE = re.compile(r'"((?:[^"\\]|\\.)*)"')

# A line that begins a const/static declaration (the `= [`-ness is
# checked separately on the right-hand side of the first `=`).
_CONST_STATIC_RE = re.compile(r"\b(?:const|static)\b")

# oxidex's OWN table macro. `const_decoder!(pub ASPECT_RATIO, i32, [(0,
# "3:2"), (12, "3:2 (APS-H crop)")])` is the repo's canonical PrintConv
# representation -- 338 uses across 46 files in src/, counted 2026-07-26
# -- and _CONST_STATIC_RE cannot see it: `\bconst\b` needs a word
# boundary after "const" and `const_decoder` has an underscore there. So
# the extractor was blind to the single most common table shape in the
# codebase while its docstring claimed to cover "const_decoder-style
# tables". Measured on real fleet output: 17 distinct worker diffs under
# ~/.oxidex/logs/model-fix-diffs add 21 distinct multi-word display
# strings in this shape and the extractor returned nothing for all of
# them (e.g. `(1, "Adjust by lens")` in 2026-07-25T16:10:18-canon-4).
#
# Matched by SHAPE (`<word>decoder<word>!(`) rather than by the exact
# macro name so a sibling macro (const_decoder_signed!, decoder_map!)
# does not silently re-open the hole.
_TABLE_MACRO_RE = re.compile(r"\b\w*decoder\w*!\s*\(")

# `for (bit, name) in [(29, "Main 10"), ...]` -- an inline table iterated
# in place instead of declared. A real QuickTime HEVC profile fix used
# this shape and its whole diff extracted nothing at all.
#
# The TUPLE pattern is required, and that is the whole point: a
# single-binding loop over a flat list is an identifier walk, not a
# key->display-value table. Measured 2026-07-26 on quarantined head
# 226828957917 (a real APP12 fix): `for wb_tag in ["WB2", "WB3", "WB4",
# "WB5", "WB6"]` is a list of raw APP12 tag NAMES, and treating it as a
# table flagged all five as PrintConv values -- the same false-quarantine
# class _STR_SLICE_REGISTRY_RE exists to prevent, arriving through a new
# door. `for (k, v) in [...]` cannot be that: the second element of each
# pair is a value by construction.
_INLINE_TABLE_RE = re.compile(r"\bfor\s*\(\s*[^)]+,[^)]+\)\s*in\s+&?\s*\[")

# A `.insert(<key>, "<display value>")` call. No `=>`, not a bare string
# element, so no map-shape rule fired: the value was invisible. The key
# side is deliberately NOT read -- `md.insert("EXIF:Make", value)` is the
# overwhelmingly common shape in src/ (1,029 `.insert(` sites carrying a
# quoted first argument, counted 2026-07-26) and its quoted argument is a
# metadata KEY, not a PrintConv value.
_INSERT_CALL_RE = re.compile(r"\.insert\s*\(")

# Why extract_added_map_values tracks BRACE depth for match-arm bodies
# (`arm_depth`) and not just bracket depth for array literals:
#
# A match arm whose right-hand side is a BLOCK -- `260 => {` -- puts its
# value on a later line, where it is neither on a `=>` line nor a bare
# string element (any wrapper, `.to_string()` / `.into()` /
# `String::from(...)`, breaks _BARE_STRING_ELEMENT_RE's anchors). So the
# value was extracted by nothing, byte-checked by nothing, and (because
# overlord_sweep.classify_for_judgment_queue calls this same extractor)
# routed to no human either.
#
# That is not a hypothetical adversarial shape -- rustfmt MANUFACTURES
# it. Measured 2026-07-26 with rustfmt 1.9.0 --edition 2021 by binary
# search: at indent 8 with a 3-digit key, a same-line arm whose display
# value reaches 71 characters is rewritten into exactly
#     260 => {
#         "....".to_string()
#     }
# and the threshold drops ~4 chars per extra indent level. 2.44% of the
# 19,796 distinct display strings in Image/ExifTool/*.pm (Lang excluded)
# are that long. So the byte check switched itself off as a pure function
# of line width -- short fabrications caught, long ones admitted, no
# intent required -- and `cargo fmt` runs over every sweep branch before
# it is pushed. The shape occurs naturally in oxidex today (7 sites, e.g.
# src/parsers/raw/metadata.rs `"R03" => {` / `"R03 - DCF option file
# (Adobe RGB)"`), so it is ordinary house output, not a tell.
#
# WHAT is extracted inside such a body is deliberately NARROWER than
# inside an array literal: only a line that is one string literal wrapped
# in nothing but conversions/constructors -- `"X".to_string()`,
# `"X".into()`, `String::from("X")`, `Some("X".to_string())`,
# `TagValue::String("X".to_owned())`, or the bare `"X"`. That is the arm's
# RETURN VALUE, which is the thing the byte check is about.
#
# Taking every quoted string in the body instead (the obvious first cut,
# and what the array-literal branch does) was measured over all 2,348
# real worker diffs in ~/.oxidex/logs/model-fix-diffs on 2026-07-26: it
# newly extracted values from 334 of them, and the additions were
# dominated by things that are not display values at all -- separators
# (", ", "."), byte magic ("Exif\0\0", "IJPEG\0"), key prefixes
# ("ExifIFD:", "EXIF:") and tag names ("BlackLevel", "ActiveArea",
# "JpgFromRaw") pulled out of ordinary body code. Those are precisely the
# false-quarantine class that POLICY_VERSION 3 and 4 were spent fixing;
# re-importing it to close this hole would be a bad trade. The narrow
# rule closes the reported hole (a fabricated display value has to BE the
# arm's value to be emitted) at a fraction of the noise.
#
# Known and accepted residue, listed so the next person does not think it
# was missed: a value laundered through a local (`let s = "Fab"; Some(s)`)
# or pushed (`out.push("Fab")`) inside a block arm is not extracted. Both
# occur zero times in the 2,348-diff corpus, and neither is what rustfmt
# produces from an ordinary arm.
_WRAPPED_STRING_VALUE_RE = re.compile(
    r"^\s*(?:return\s+)?"                         # an early return is a value too
    r"(?:[A-Za-z_][\w:]*\s*\(\s*)*"               # Some( / String::from( / ...
    r'"(?:[^"\\]|\\.)*"'                          # exactly one string literal
    r"(?:\s*\.\s*[a-z_]+\(\))*"                   # .to_string() / .into() / ...
    r"\s*\)*\s*[,;]?\s*$"                         # closing parens, optional , or ;
)

# A PrintConv value is a human-readable DISPLAY string ("Fine", "AE/AF
# Lock", "Intel 386 or later, and compatibles"). The strings below are
# IDENTIFIERS, and are never expected to appear in an ExifTool module's
# PrintConv tables -- so demanding that they do rejects correct code.
#
# Measured live 2026-07-25: after the Table-trailer fix this became the
# single largest quarantine cause (15 of 27 flags), and it rejected an
# otherwise-valid DNG fix whose diff was a tag-ID -> tag-NAME registry:
#     0x0111 => "EXIF:PreviewImageStart".to_string(),
#     ByteOrder::LittleEndian => tiff.extend_from_slice(b"II\x2a\x00"),
# Neither line contains a PrintConv value at all.
#
# Both rules below are deliberately SHAPE-based and narrow: a genuine
# display value that merely contains a colon ("Fine: Best") or a space
# is unaffected, so the fabricated-value check this gate exists for is
# preserved intact.
#
# The identifier half admits `-` and `.` because real oxidex tag names
# carry them. Measured 2026-07-26 while adding the block-arm/.insert/
# tuple rules below: those rules read a string as a display value
# without asking whether it IS one, and with a `\w`-only pattern
# `"EXIF:TIFF-EPStandardID"` hard-blocked -- a value copied verbatim
# from 2026-07-25T07:58:08-nikon-2-APPLIED.diff, i.e. a commit the fleet
# had already ACCEPTED. Widening the halves rather than loosening the
# whole gate keeps "Fine: Best" and "YCbCr4:4:4 (1 1)" checked: a space
# still disqualifies, which is what separates a key from a display
# string.
_TAG_KEY_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_.-]*:[A-Za-z][A-Za-z0-9_.-]*$")

# A BARE CamelCase tag name with no group prefix ("VolumeDescriptorType")
# is deliberately NOT suppressed, though the same 2026-07-26 measurement
# surfaced one via a block-arm return in tail-1. Shape cannot separate it
# from a genuine display value: "RhsOnly" and "VolumeDescriptorType" are
# identical to any rule expressible here, and a bare-CamelCase exemption
# would hand a fabricator a trivial evasion -- return an invented
# CamelCase value and it is never byte-checked.
#
# So this over-flags that one shape on purpose. The asymmetry decides it:
# an over-flag costs one recoverable quarantine plus a POLICY_VERSION
# retry, while an under-flag auto-merges a fabricated display value to
# main and stays there. Widening the group-qualified form above already
# removes 3 of the 4 measured over-flags; this is the remaining 1.


def looks_like_tag_key(value):
    """True for an oxidex metadata key like "EXIF:PreviewImageStart" or
    "EXIF:TIFF-EPStandardID".

    Requires BOTH halves to be identifier-shaped -- letters, digits,
    `_`, `-`, `.`, and nothing else -- around a single separating colon.
    A space anywhere disqualifies, which is what keeps display strings
    that merely contain a colon ("Fine: Best", "YCbCr4:4:4 (1 1)")
    byte-checked. A BARE name with no colon is not matched here; see
    the comment above _TAG_KEY_RE for why that is deliberate.
    """
    return bool(_TAG_KEY_RE.match(value or ""))


def is_identifier_not_printconv(value, code_line):
    """True when `value` is an identifier/constant rather than a
    candidate PrintConv display string, and so must not be byte-checked
    against the Perl module.

    code_line is the source line the value came from -- needed to spot a
    Rust byte-string literal (b"..."), which carries binary magic such
    as the TIFF byte-order marks and is never a display string.
    """
    if looks_like_tag_key(value):
        return True
    if f'b"{value}"' in (code_line or ""):
        return True
    if _FORMAT_PLACEHOLDER_RE.search(value or ""):
        return True
    return False

# A line that IS nothing but string-literal array element(s) -- one or
# more quoted strings separated/terminated by commas. This is what a
# value inserted into the MIDDLE of an existing const table looks like
# when the declaring line is outside the hunk (git shows only a few
# neighboring entries as context, so the bracket-depth tracker never
# opens): the most common shape of this fix class, and exactly the one
# a fabricated value must not slip through.
_BARE_STRING_ELEMENT_RE = re.compile(
    r'^\s*(?:"(?:[^"\\]|\\.)*"\s*,\s*)*"(?:[^"\\]|\\.)*"\s*,?\s*$'
)

# The same idea for a KEYED table element -- `(12, "3:2 (APS-H crop)"),`
# -- which is what a mid-table insert into a const_decoder!/array-of-
# tuples table looks like when the declaring `[` is outside the hunk.
# The key side excludes quotes and parens so this cannot match a nested
# call or a ("key", "value") pair whose first element is itself a string
# (that shape is left to the bare-string rule at its own line).
#
# Whole-line anchored, like its sibling, which leaves a known gap: a
# table written entirely on one line by something that is not a
# recognised opener (`let m = HashMap::from([(1, "Fabricated")]);`) is
# not read. Zero occurrences in the 2,348-diff corpus; recorded here so
# the next person knows it was weighed, not missed.
_BARE_TUPLE_ELEMENT_RE = re.compile(
    r'^\s*\(\s*[^",()]+,\s*"(?:[^"\\]|\\.)*"\s*\)\s*,?\s*$'
)


def strip_trailing_comment(code):
    """`code` with any trailing `//...` or `/*...*/` comment removed.

    A SCANNER, not a regex, because `//` inside a string literal is not a
    comment: `"http://x.example//y",` must survive intact. Escapes are
    honoured so a `\\"` cannot end the literal early.

    This exists because _BARE_STRING_ELEMENT_RE is `$`-anchored, and
    src/parsers/icc/registries.rs -- the file whose indexed display-value
    tables the POLICY_VERSION 5 note below specifically claims to protect
    -- writes its elements in the column-aligned house style
    `"F2",             // 4`. Measured 2026-07-26 against the real
    ILLUMINANT_TYPES table with the real perl-lib: inserting
    `"FAB PLAIN VALUE",` gave printconv-mismatch, while the identical
    value written `"FAB COMMENT VALUE", // 42` (or `/* 42 */`, or aligned
    house style) gave ok=True with NO flags at all. The file's own style
    was the evasion; no adversarial intent was needed. Stripping before
    extraction also stops a string that only exists inside a comment from
    being byte-checked as if it were code.
    """
    out = []
    i = 0
    in_string = False
    while i < len(code):
        char = code[i]
        if in_string:
            if char == "\\":
                out.append(code[i:i + 2])
                i += 2
                continue
            if char == '"':
                in_string = False
            out.append(char)
            i += 1
            continue
        if char == '"':
            in_string = True
            out.append(char)
            i += 1
            continue
        if code.startswith("//", i):
            break
        if code.startswith("/*", i):
            end = code.find("*/", i + 2)
            if end == -1:
                break  # unterminated block comment: the rest is comment
            i = end + 2
            continue
        out.append(char)
        i += 1
    return "".join(out)


def _insert_call_value_text(code):
    """The argument text AFTER the first top-level comma of the first
    `.insert(` call on `code`, or None when there is no such call.

    Top-level means at the insert call's own paren depth, so
    `md.insert(format!("ICC_Profile:{}", tag), value)` splits after the
    `format!(...)` argument and not inside it -- otherwise the format
    template would be harvested as a display value.
    """
    match = _INSERT_CALL_RE.search(code)
    if not match:
        return None
    depth = 0
    in_string = False
    i = match.end()
    while i < len(code):
        char = code[i]
        if in_string:
            if char == "\\":
                i += 2
                continue
            if char == '"':
                in_string = False
            i += 1
            continue
        if char == '"':
            in_string = True
        elif char in "([{":
            depth += 1
        elif char in ")]}":
            if depth == 0:
                return None  # call closed before any second argument
            depth -= 1
        elif char == "," and depth == 0:
            return code[i + 1:]
        i += 1
    return None

# git writes the enclosing declaration into every hunk header, after the
# second "@@":
#     @@ -102,6 +102,10 @@ const KNOWN_TAGS: &[&str] = &[
# The bare-string rule above was justified by "the declaring line is
# outside the hunk" -- but git hands it to us right here, and the old
# loop threw it away by `continue`-ing on "@@". These two patterns read
# it back.
#
# What we are looking for is a registry of KEY NAMES the parser
# recognises -- `const KNOWN_TAGS: &[&str] = &[...]` in the APP12
# Olympus/Ricoh/thermal parsers, whose elements are tag IDENTIFIERS
# ("REV", "S0", "STB1", "WB3"). Demanding those appear in an ExifTool
# PrintConv table rejects correct code, and was the single largest
# quarantine cause measured 2026-07-25.
#
# This originally keyed on the TYPE SHAPE -- `&[&str]` slice meaning
# registry, `[&str; N]` fixed array meaning indexed PrintConv lookup.
# That premise is BACKWARDS for this repo. Counted 2026-07-26 over src/:
# 36 `&[&str]` declarations against 3 `[&str; N]`, and the slices include
# src/parsers/icc/registries.rs's RENDERING_INTENTS, ILLUMINANT_TYPES,
# OBSERVER_TYPES and GEOMETRY_TYPES -- indexed display-value tables whose
# own comments read "indexed by code 1-2" and whose elements
# ("Perceptual", "Media-Relative Colorimetric", "CIE 1931") are exactly
# the strings the byte check exists to protect. The shape gate therefore
# disabled the fabrication check for 36 declarations to save 3.
#
# Keying on the declaration NAME instead is both narrower and closer to
# the actual semantics: a name ending in _TAGS/_KEYS/_FIELDS/_NAMES/
# _EXTENSIONS/_GROUPS/_MARKERS, or starting SUPPORTED_, denotes
# identifiers. Measured against the same 36: 25 match (correctly skipped)
# and 11 stay checked -- including all four ICC value tables. The
# remainder that stay checked (CORE_FRAMEWORKS, PACKER_SECTIONS,
# SUSPICIOUS_IMPORTS, ...) are identifier-ish too, but leaving them
# CHECKED is the safe direction: over-checking costs one recoverable
# flag and a POLICY_VERSION retry, under-checking ships a fabricated
# value to main silently and forever.
#
# CORRECTION (2026-07-26, POLICY_VERSION 6): "including all four ICC
# value tables" was true of this NAME gate and false of the extractor as
# a whole. ILLUMINANT_TYPES is 9 elements long, so a mid-table insert
# puts its declaring line outside git's 3-line context and the value
# arrives as a bare element -- and every element in that file is written
# `"F2",             // 4`, whose trailing comment the `$`-anchored
# _BARE_STRING_ELEMENT_RE could not match. Measured against the real
# file with the real perl-lib: the plain style flagged
# printconv-mismatch, the house style returned ok=True with no flags.
# The same delta hit PACKER_SECTIONS and SUSPICIOUS_IMPORTS (the 3 of 12
# >=8-element bare-string tables in src/ that are checked at all).
# strip_trailing_comment makes the claim true again.
_STR_SLICE_REGISTRY_RE = re.compile(
    r"\b(?:const|static)\s+(\w*(?:_TAGS|_KEYS|_FIELDS|_NAMES|_EXTENSIONS|_GROUPS|_MARKERS)"
    r"|SUPPORTED_\w+)\s*:\s*&?\s*\[\s*&(?:'\w+\s+)?str\s*\]"
)

# Assert messages are not PrintConv values -- thermal's
# `printconv-mismatch:Flash=0 should be PrintConv'd to Off` flag came
# from an `assert!` message.
#
# This is a LINE test, deliberately. It used to be a HUNK test keyed on
# git's funcname context matching `mod tests`, which was exploitable:
# git's default driver reports the nearest preceding COLUMN-0
# declaration, and `#[cfg(test)] mod tests {` is conventionally the last
# such declaration in a Rust file -- so appending a fabricated PrintConv
# function at end-of-file makes git emit `@@ -141,3 +141,14 @@ mod tests {`
# for a hunk of 100% PRODUCTION code, and the whole hunk was skipped.
# Reproduced 2026-07-26 against src/core/formatters/exposure_program.rs:
# 'Landscape Mode' / 'Portrait Mode' / 'Night Scene Mode' -- none of
# which occur in any of the 171 Image/ExifTool/*.pm files -- went
# ok=false on the pre-#125 validator and ok=true after it.
#
# A per-line assert test cannot be gamed that way: the fabricated table
# entries are not assert lines, so they stay checked no matter what git
# decides to put in the hunk header.
_ASSERT_LINE_RE = re.compile(r"\b(?:assert\w*|panic|unreachable|todo|unimplemented)\s*!")

# A macro that BUILDS a string at runtime. Its first argument is a format
# TEMPLATE, not a value: `other => format!("Unknown({})", other)` yields
# the literal `Unknown({})`, which by construction never appears in any
# ExifTool module. The template is genuinely unverifiable -- exactly what
# the docstring already promised computed right-hand sides would be --
# but `if found:` short-circuited it into `values` and flagged it as a
# fabrication instead.
_STRING_BUILDING_MACRO_RE = re.compile(
    r"\b(?:format|write|writeln|concat|format_args|println|panic|assert\w*)\s*!"
)

# A `{}` / `{:04}` / `{name}` placeholder. Any string carrying one is a
# format template rather than a literal display value -- belt-and-braces
# for multi-line macro calls whose template sits alone on its own line
# (`"{:04}:{:02}:{:02} {:02}:{:02}:{:02}.{:03}",` in parse_flir_datetime),
# where no `format!` token is visible on the line at all.
_FORMAT_PLACEHOLDER_RE = re.compile(r"\{[^{}]*\}")


class GitError(RuntimeError):
    """A git invocation this validator depends on failed."""


def run_git(args, repo, input_text=None):
    """Default git runner: run `git <args>` in `repo`, return
    (returncode, stdout, stderr). Tests inject a fake with the same
    signature; production code never calls subprocess anywhere else for
    git, so one injection point covers every git touch in the script."""
    result = subprocess.run(  # nosec B603
        ["git", *args],
        cwd=repo,
        input=input_text,
        capture_output=True,
        text=True,
    )
    return result.returncode, result.stdout, result.stderr


def _git_or_die(args, repo, git_run, input_text=None):
    """run_git wrapper for commands whose failure means the validator
    cannot produce a meaningful verdict at all (unknown sha, not a git
    repo, ...) -> GitError -> exit 1, never a fabricated 'clean'."""
    rc, out, err = git_run(args, repo, input_text)
    if rc != 0:
        raise GitError(f"git {' '.join(args)} failed (rc={rc}): {err.strip()}")
    return out


def commit_message(sha, repo, git_run=run_git):
    """The commit's full message body (subject + body + trailers)."""
    return _git_or_die(["show", "-s", "--format=%B", sha], repo, git_run)


def commit_diff(sha, repo, git_run=run_git):
    """The commit's diff with the message stripped (--format=), so the
    printconv extractor and patch-id both see pure patch text and a
    message that happens to contain `=>` can never leak into either."""
    return _git_or_die(["show", "--format=", sha], repo, git_run)


def commit_changed_files(sha, repo, git_run=run_git):
    """Repo-relative paths the commit touches (for the ownership check)."""
    out = _git_or_die(
        ["diff-tree", "--no-commit-id", "--name-only", "-r", sha], repo, git_run
    )
    return [line for line in out.splitlines() if line.strip()]


def parse_trailers(message, repo, git_run=run_git):
    """Parse trailers out of a commit message via
    `git interpret-trailers --parse` -- NOT a hand-rolled 'last
    paragraph' regex, so folding, unfolding and separator rules match
    whatever git itself considers a trailer (the same tooling
    overlord_sweep uses to build PR evidence tables reads them back).

    Returns {key: [value, ...]} preserving repetition order --
    `Tag:` is repeatable per spec M1 (one per cluster member)."""
    out = _git_or_die(["interpret-trailers", "--parse"], repo, git_run, input_text=message)
    trailers = {}
    for line in out.splitlines():
        key, sep, value = line.partition(":")
        if not sep:
            continue
        trailers.setdefault(key.strip(), []).append(value.strip())
    return trailers


def check_trailers(trailers, require_perl_ref=True):
    """Flags for every required trailer key that is missing or empty.

    Missing evidence is itself a routing signal: a commit without a
    parseable Verified trailer cannot be machine-accepted no matter how
    green its build is.

    require_perl_ref is False when the diff has no PrintConv value to
    byte-check, because Perl-Ref's only consumer is that check -- see the
    call site in validate_commit."""
    required = REQUIRED_TRAILERS if require_perl_ref else CONDITIONAL_TRAILERS
    flags = []
    for key in required:
        values = [v for v in trailers.get(key, []) if v]
        if not values:
            flags.append(f"missing-trailer:{key}")
    return flags


def check_trailer_truth(trailers, perl_lib=None, samples_cache=None):
    """Flags for evidence trailers that are PRESENT but NOT TRUE.

    check_trailers above only asks whether a trailer exists. Nothing asked
    whether it was accurate, and on 2026-07-27 a JPEG fix commit passed the
    whole gate while citing:

      Perl-Ref: NikonCustom.pm   -- Nikon custom-settings binary tables,
                                    containing NONE of the six APP12 tags
                                    it claimed to fix (the truth is APP12.pm)
      Sample:   ExifTool.jpg     -- an Agfa file. Four of the six tags do not
                                    appear in it at all, and the edited code
                                    path never executes on it, because the
                                    APP12 router byte-compares b"AGFA" while
                                    that file begins "Agfa".

    So ZERO of its six tags were reachable through its own cited evidence.
    The code happened to be correct -- unrelated Olympus samples did the real
    validating, since the recheck reports a per-FORMAT gap count rather than a
    per-sample one -- but the evidence chain attesting to it was fiction.

    That matters far beyond one commit: the judgment-queue daemon re-derives
    trailers to clear the ~378 missing-trailer quarantines, so a trailer that
    can be plausible and false is a mechanism for manufacturing evidence at
    scale. These checks make a wrong citation as visible as an absent one.

    Both checks are conservative in the same direction as the rest of this
    module: a missing --perl-lib, an unresolvable module, or an empty samples
    cache yields NO flag rather than a guess. Only positive disproof -- the
    file exists and demonstrably lacks the tag -- is reported.
    """
    flags = []
    tags = [t for t in trailers.get("Tag", []) if t]
    if not tags:
        return flags

    # --- Perl-Ref names a module that actually documents these tags -------
    perl_refs = [r for r in trailers.get("Perl-Ref", []) if r]
    if perl_lib is not None and perl_refs:
        for ref in perl_refs:
            module = resolve_perl_module(ref, Path(perl_lib))
            if module is None:
                continue  # unresolvable: already covered by other flags
            try:
                text = module.read_text(errors="replace")
            except OSError:
                continue
            # DEFINES, not merely mentions. Presence alone cannot tell the
            # right module from the wrong one: NikonCustom.pm contains
            # `24 => 'Protect'` -- a Nikon custom-setting VALUE that happens
            # to share the name -- while the true source, APP12.pm, has
            # `Protect => { }`, the tag definition. Both "mention Protect".
            # Only the table-key position distinguishes them.
            if any(_defines_tag(text, _tag_local_name(t)) for t in tags):
                continue
            # The cited module defines none of them. Before calling that a
            # miscitation, confirm the tags are documentable AT ALL: ExifTool
            # names some tags at runtime (ProcessAPP12's `ucfirst $tag`
            # fallback produces REV/S0/STB1/STB3/STB4, which appear in no
            # table anywhere), and a commit fixing only those would otherwise
            # be flagged against a perfectly correct Perl-Ref.
            # _perl_lib_corpus returns BYTES (it exists for byte-exact
            # PrintConv comparison); decode before regex-matching text.
            corpus = _perl_lib_corpus(Path(perl_lib))
            if isinstance(corpus, bytes):
                corpus = corpus.decode("utf-8", errors="replace")
            if any(_defines_tag(corpus, _tag_local_name(t)) for t in tags):
                flags.append(f"perl-ref-documents-none:{Path(ref).name}")

    # --- Tag: names a tag, not a PrintConv display string -----------------
    # The block above deliberately stays silent when a name appears nowhere
    # in any table, because ExifTool names some tags at runtime and those are
    # real. But that silence also covered the opposite case: a name that is
    # not a tag anywhere AND is a display value ExifTool prints for some
    # numeric key. That is a display string harvested as a tag, and it is how
    # `EXIF:Higher resolution image exists` (OPIProxy's PrintConv value 1),
    # `Trilinear` (SensingMethod 7) and `Creative (Slow speed)`
    # (ExposureProgram 5) all reached emitted metadata.
    #
    # This shape cannot collide with the runtime-named tags the block above
    # protects: REV/S0/STB1 appear in no table AND as no display value, so
    # they still draw no flag. Only names ExifTool prints as values, and
    # never defines as tags, are reported.
    #
    # Perl source ALONE cannot decide this, and assuming it can produces false
    # convictions. ExifTool's binary-data tables use a shorthand where the tag
    # name is the VALUE of a numeric key -- Canon.pm:7429 is
    # `10 => 'BlackMaskTopBorder'` -- which is character-for-character the
    # shape of a PrintConv row like `1 => 'Uncompressed'`. Reading the corpus
    # would flag BlackMaskTopBorder, FontSubfamilyID and NameTableVersion,
    # all real tags. `-listx` resolves the ambiguity because ExifTool has
    # already decided: real tags come out as <tag name=...>, display strings
    # as <key><val>. Without it there is no evidence, so there is no flag.
    listx_names = _exiftool_tag_names()
    if perl_lib is not None and listx_names:
        corpus = _perl_lib_corpus(Path(perl_lib))
        if isinstance(corpus, bytes):
            corpus = corpus.decode("utf-8", errors="replace")
        for tag in tags:
            local = _tag_local_name(tag)
            if local in listx_names or _defines_tag(corpus, local):
                continue
            if _is_print_conv_display_value(corpus, local):
                flags.append(f"tag-name-is-a-printconv-value:{_flag_token(tag)}")

    # --- Sample names a file that actually carries these tags -------------
    samples = [s for s in trailers.get("Sample", []) if s]
    if samples_cache is not None and samples:
        cache = Path(samples_cache)
        cited = {Path(s).name for s in samples}
        for tag in tags:
            carriers = find_samples_carrying_tag(cache, tag)
            if not carriers:
                continue  # nothing known about this tag: cannot disprove
            if not (cited & {Path(c).name for c in carriers}):
                flags.append(f"sample-lacks-tag:{_flag_token(tag)}")
    return flags


def _defines_tag(perl_text, name):
    """True if `name` appears in TABLE-KEY position in Perl source.

    ExifTool tag tables are `Name => { ... }` or `Name => 'Something'`, with
    the name at the start of an element. A tag NAME used as a PrintConv
    VALUE is the other way round -- `24 => 'Protect'` -- and matching that
    is what made a wrong-module citation indistinguishable from a right one
    (NikonCustom.pm and APP12.pm both "mention Protect"; only APP12.pm
    defines it). Hex/decimal-keyed tables spell the name on the following
    Name => line instead, so that form counts too.
    """
    key_form = rf"(?m)^\s*{re.escape(name)}\s*=>"
    name_form = rf"(?m)^\s*Name\s*=>\s*'{re.escape(name)}'"
    return bool(re.search(key_form, perl_text) or re.search(name_form, perl_text))


_LISTX_TAG_NAMES = None


def _exiftool_tag_names():
    """Every tag name `exiftool -f -listx` reports, or None when unavailable.

    Cached for the process: the dump is ~18 MB and the merger calls this per
    commit. `None` (no exiftool, or a parse failure) means the caller must not
    accuse -- same conservative direction as the rest of this module.

    The dump comes from the PINNED oracle rather than a bare `exiftool`. A tag
    added in the release the tables were transcribed from is absent from an
    older PATH exiftool's -listx, and "absent from -listx" is precisely what
    this function's callers read as evidence of a fabricated name. An
    unresolvable/skewed/degraded oracle raises OracleError (a RuntimeError),
    which lands in the same conservative `None` as a missing binary.
    """
    global _LISTX_TAG_NAMES
    if _LISTX_TAG_NAMES is None:
        names = set()
        try:
            blob = subprocess.run(  # nosec B603
                shared_exiftool_oracle().command(["-f", "-listx"]),
                capture_output=True, check=True,
            ).stdout
            root = ET.fromstring(blob)  # nosec B314 -- local trusted binary
            for table in root.iter("table"):
                for tag in table.findall("tag"):
                    names.add(tag.get("name", ""))
        except (OSError, subprocess.SubprocessError, ET.ParseError, RuntimeError):
            names = set()
        _LISTX_TAG_NAMES = names
    return _LISTX_TAG_NAMES or None


def _is_print_conv_display_value(perl_text, name):
    """True if `name` sits on the VALUE side of a numeric-keyed mapping.

    The mirror image of `_defines_tag`. A PrintConv row spells the display
    string to the right of a numeric key:

        1 => 'Uncompressed',
        7 => 'Trilinear',
        5 => 'Creative (Slow speed)',

    whereas a tag definition puts the name on the left (`Protect => {...}`)
    or on a `Name =>` line. Keys may be decimal, hex, dotted bit-positions
    (`1.1`) or quoted, so all four are matched.
    """
    key = r"(?:0x[0-9a-fA-F]+|'?\d+(?:\.\d+)?'?)"
    return bool(re.search(rf"(?m)^\s*{key}\s*=>\s*'{re.escape(name)}'", perl_text))


def _tag_local_name(tag):
    """"APP12:Protect" -> "Protect". A Perl tag table spells the tag's own
    name; the family prefix is oxidex's output convention, not ExifTool's
    source convention, so it must be stripped before searching a .pm."""
    _, sep, name = tag.partition(":")
    return name if sep else tag


def _flag_token(value):
    """Flags are joined into a single comma-separated ledger field, so a
    token carrying a comma or whitespace would split into two bogus flags
    downstream. Same defensive shape as _clamp_quarantine_flags."""
    return re.sub(r"[,\s]+", "_", value.strip())


# ---------------------------------------------------------------------------
# Multi-sample check
# ---------------------------------------------------------------------------

def iter_cache_files(samples_cache):
    """Per-file exiftool output files under the samples cache. Suffix
    filter keeps the scan away from the sample tarballs and binaries
    that share the real /tmp/oxidex-exiftool-cache directory."""
    for path in sorted(samples_cache.rglob("*")):
        if path.is_file() and path.suffix.lower() in (".json", ".txt"):
            yield path


def find_samples_carrying_tag(samples_cache, tag):
    """All sample files whose cached exiftool output carries `tag`
    ("<family>:<name>" per the Tag: trailer convention).

    Two shapes are understood:

    - The real exiftool-tag-cache shape: JSON
      {"result": {"tags": [{"name", "family", "value", "source_file"}]}}
      -- entries matching family+name contribute their source_file, so
      one per-format cache file can attest many sample files.
    - Anything else (plain-text exiftool dumps): a cheap grep -- if the
      tag's name appears anywhere in the file, the file itself (suffix
      stripped) is treated as the carrying sample. This is the fallback
      the spec's "a grep over per-file outputs" describes.

    Returns a sorted, deduplicated list so the comparison runner is
    invoked in a deterministic order."""
    family, sep, name = tag.partition(":")
    if not sep:
        family, name = "", tag
    carriers = set()
    for path in iter_cache_files(samples_cache):
        try:
            text = path.read_text(errors="replace")
        except OSError:
            continue
        if name not in text:
            continue  # the grep gate: no mention at all, no carrier here
        entries = None
        try:
            doc = json.loads(text)
            if isinstance(doc, dict):
                result = doc.get("result")
                if isinstance(result, dict):
                    entries = result.get("tags")
        except json.JSONDecodeError:
            pass
        if isinstance(entries, list):
            for entry in entries:
                if not isinstance(entry, dict) or entry.get("name") != name:
                    continue
                if family and entry.get("family") != family:
                    continue
                source = entry.get("source_file")
                if source:
                    carriers.add(str(source))
        else:
            carriers.add(str(path.with_suffix("")))
    return sorted(carriers)


def check_multi_sample(tags, samples_cache, comparison_fn):
    """Verify every Tag: trailer on EVERY sample file carrying it.

    This is the class-(a) catch: a worker that pattern-matched one
    sample's value gets its fix exercised against all carriers found in
    the cache, and 'right on my Sample:, wrong on the other three
    Canon bodies' surfaces here as multi-sample-fail:<tag>.

    comparison_fn(sample_path, tag) -> bool must answer 'does oxidex's
    post-commit output for this tag on this sample match exiftool's?'
    (built from --comparison-cmd in production, injected in tests).

    Returns (status, flags); status is "skipped" when the check cannot
    run (no cache dir, no runner, or no Tag trailers -- the last already
    flagged by check_trailers)."""
    if samples_cache is None or comparison_fn is None or not tags:
        return "skipped", []
    flags = []
    for tag in tags:
        carriers = find_samples_carrying_tag(samples_cache, tag)
        if not carriers:
            flags.append(f"multi-sample-no-carriers:{tag}")
            continue
        if any(not comparison_fn(sample, tag) for sample in carriers):
            flags.append(f"multi-sample-fail:{tag}")
    return ("flagged" if flags else "pass"), flags


def build_comparison_fn(comparison_cmd):
    """Turn --comparison-cmd into a comparison_fn: the command is run as
    `<cmd...> <sample_path> <tag>`; exit 0 means the tag matches
    post-commit. Kept trivially thin so tests never need it -- they
    inject a plain function instead."""
    argv = shlex.split(comparison_cmd)

    def compare(sample_path, tag):
        result = subprocess.run(  # nosec B603
            [*argv, sample_path, tag], capture_output=True, text=True
        )
        return result.returncode == 0

    return compare


# ---------------------------------------------------------------------------
# PrintConv-vs-Perl byte check
# ---------------------------------------------------------------------------

def unescape_rust_string(literal):
    """Decode the escapes a Rust string literal in the diff may carry so
    the byte comparison is against the VALUE, not its source spelling
    (Perl writes 'Voigtländer', the diff may write "Voigtl\\u{e4}nder")."""
    simple = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", '"': '"', "'": "'", "\\": "\\"}
    out = []
    i = 0
    while i < len(literal):
        ch = literal[i]
        if ch == "\\" and i + 1 < len(literal):
            nxt = literal[i + 1]
            if nxt == "u" and literal[i + 2 : i + 3] == "{":
                end = literal.find("}", i + 3)
                if end != -1:
                    try:
                        out.append(chr(int(literal[i + 3 : end], 16)))
                        i = end + 1
                        continue
                    except ValueError:
                        pass
            if nxt in simple:
                out.append(simple[nxt])
                i += 2
                continue
        out.append(ch)
        i += 1
    return "".join(out)


def _keep_printconv_values(raw_matches, code_line):
    """Unescape each regex match, dropping identifiers/constants that are
    not candidate PrintConv display strings (see
    is_identifier_not_printconv)."""
    out = []
    for m in raw_matches:
        value = unescape_rust_string(m)
        if is_identifier_not_printconv(value, code_line):
            continue
        out.append(value)
    return out


def extract_added_map_values(diff_text):
    """Every quoted string VALUE the diff ADDS inside map-like
    structures, plus the added lines whose map value could not be
    extracted.

    Map-like means, mechanically:
      - lines containing `=>` (match arms, phf_map!/HashMap entries):
        the quoted strings on the RIGHT of the `=>` are values; quoted
        keys on the left are not checked;
      - the BODY of a match arm whose right-hand side opens a block
        (`260 => {` ... `}`), tracked across lines by brace depth: the
        value lives on a later line there, wrapped in whatever converts
        it (`.to_string()`, `String::from(...)`, `Some(...)`). See the
        note above _WRAPPED_STRING_VALUE_RE for why this is rustfmt's own
        output rather than an exotic shape, and for what is deliberately
        NOT taken from a body;
      - array literals: `const`/`static` declarations, oxidex's
        `const_decoder!(...)` table macro, and inline `for ... in [...]`
        tables. Every quoted string on added lines inside the literal is
        a value. The literal is tracked across lines by bracket depth,
        and a context (unchanged) line can open it so that inserting one
        value into an existing table is still seen;
      - added lines that are NOTHING BUT a bare table element -- one or
        more bare string literals ("Some Value"), or one `(key,
        "value")` tuple -- even at bracket depth 0: a value inserted
        mid-table in a large existing array produces a hunk whose
        context lines are just neighboring entries, so the declaring
        `const ... = [` line never appears and depth tracking alone
        would silently pass a fabricated value;
      - `.insert(<key>, "<value>")` calls: the text after the call's
        first TOP-LEVEL comma, so the quoted metadata key that normally
        occupies the first argument is not mistaken for a display value.

    Every one of those tests runs against the line with any trailing
    `//` / `/* */` comment stripped (strip_trailing_comment), because the
    house style in this repo writes indexed tables as
    `"F2",             // 4` and the bare-element rules are anchored.

    An added `=>` line whose right-hand side has no quoted string and is
    not a benign literal (number/bool/None) is COMPUTED -- reported in
    the second return value so check_printconv can flag
    printconv-unverifiable rather than silently passing it (spec open
    question 6).

    Returns (values, unverifiable_line_excerpts), values deduplicated in
    first-seen order."""
    values = []
    unverifiable = []
    depth = 0  # bracket depth inside an array literal
    arm_depth = 0  # brace depth inside a block-bodied match arm
    assert_depth = 0  # paren depth inside a multi-line assert!/panic! call
    table_pending = False  # saw a table macro; its `[` is on a later line
    hunk_ctx = ""  # git's record of the declaration enclosing this hunk
    for raw in diff_text.splitlines():
        if raw.startswith("diff --git"):
            depth = 0  # never let array/arm state leak across files/hunks
            arm_depth = 0
            assert_depth = 0
            table_pending = False
            hunk_ctx = ""
            continue
        if raw.startswith("@@"):
            depth = 0
            arm_depth = 0
            assert_depth = 0
            table_pending = False
            # "@@ -a,b +c,d @@ <enclosing declaration>" -- everything
            # after the second "@@" is git's funcname context.
            parts = raw.split("@@")
            hunk_ctx = parts[2] if len(parts) > 2 else ""
            continue
        if raw.startswith("+++") or raw.startswith("---"):
            continue
        if not raw or raw[0] not in "+ ":
            continue  # removed lines and diff noise
        added = raw[0] == "+"
        code = raw[1:]
        # Every shape test below reads the comment-stripped text; the
        # ORIGINAL line is still what _keep_printconv_values sees, because
        # is_identifier_not_printconv needs the raw `b"..."` spelling.
        bare = strip_trailing_comment(code)
        if assert_depth > 0 or _ASSERT_LINE_RE.search(code):
            # An assert/panic message is prose ABOUT a value, not the
            # value. rustfmt routinely splits these across lines:
            #     assert_eq!(
            #         metadata.get_string("APP12:Flash"),
            #         Some("Off"),
            #         "Flash=0 should be PrintConv'd to Off"
            #     );
            # so a per-line token test alone misses the message (measured
            # on 12a20366f5bc). Track the macro's parenthesis depth and
            # skip until it closes. Unlike the hunk-level `mod tests` gate
            # this replaced, it cannot be widened by where git decides to
            # put a funcname header: a fabricated table entry is not
            # inside an assert call.
            assert_depth = max(0, assert_depth + code.count("(") - code.count(")"))
            depth = max(0, depth + code.count("[") - code.count("]"))
            arm_depth = max(0, arm_depth + code.count("{") - code.count("}"))
            continue
        # Being inside a block-bodied match arm ADDS one rule (the arm's
        # own wrapped value counts as a map value) and takes none away:
        # every other rule below still runs, because a body routinely
        # contains further map-like content -- a nested `match` whose arms
        # are ordinary same-line `1 => Some("Rectangular".to_string()),`
        # entries, a `map.insert(...)`, a nested table literal. Measured
        # 2026-07-26: an earlier cut of this change made the body a
        # SEPARATE branch that skipped the rest of the chain, and that
        # silently stopped extracting the nested arms of three real DNG
        # heads (786ea09b9475, a688591c5a2f, c446aaf8cc80) -- 16 genuine
        # display values, including "Even rows offset up by 1/2 row, even
        # columns offset left by 1/2 column", went from checked to
        # unchecked. Closing one hole must not open another; the brace
        # bookkeeping happens here, once, and the rules stay additive.
        in_arm_body = arm_depth > 0
        if in_arm_body:
            arm_depth = max(0, arm_depth + bare.count("{") - bare.count("}"))
            if added and _WRAPPED_STRING_VALUE_RE.match(bare):
                # The arm's return value. No printconv-unverifiable is
                # raised for a `format!` body here, deliberately, unlike
                # the same-line `=>` branch: a template cannot be a
                # fabricated ExifTool value -- its `{}` placeholders
                # appear in no tag table, which is what
                # _FORMAT_PLACEHOLDER_RE already encodes -- so the flag
                # would buy no fabrication detection. Measured over the
                # 2,348 worker diffs on 2026-07-26: raising it in bodies
                # newly flags 227 of them against the 100 that carry the
                # flag today, a 3.3x increase in a BLOCKING flag for no
                # detection gain (head 4a71eb0a4b72, a real CR2 LensInfo
                # fix whose body is all `format!("{:.1}", ...)`, is one).
                values.extend(_keep_printconv_values(_QUOTED_RE.findall(bare), code))
                continue
        if depth > 0:
            if added:
                values.extend(_keep_printconv_values(_QUOTED_RE.findall(bare), code))
            depth = max(0, depth + bare.count("[") - bare.count("]"))
            continue
        table_tail, table_pending = _table_literal_tail(bare, table_pending)
        if table_tail is not None:
            if added:
                values.extend(_keep_printconv_values(_QUOTED_RE.findall(table_tail), code))
            depth = max(0, table_tail.count("[") - table_tail.count("]"))
            continue
        if added and "=>" in bare:
            rhs = bare.split("=>", 1)[1]
            if _STRING_BUILDING_MACRO_RE.search(rhs):
                # Runtime-built string: the literal in the diff is a
                # template, not a value, so there is nothing to
                # byte-check. Honestly unverifiable rather than a
                # fabrication.
                unverifiable.append(code.strip()[:80])
                continue
            found = _QUOTED_RE.findall(rhs)
            if found:
                values.extend(_keep_printconv_values(found, code))
            if not in_arm_body and rhs.count("{") > rhs.count("}"):
                # `260 => {` -- the arm's value is in the block body, so
                # keep scanning until the brace closes. Opened on brace
                # BALANCE rather than on an exact `=> {` match so
                # `=> Foo {` (a struct literal spanning lines) and
                # `=> { let x = 1;` are covered too. When already inside
                # a body the counter was updated for the whole line above,
                # so assigning here would discard the outer arm's depth.
                arm_depth = rhs.count("{") - rhs.count("}")
                continue
            # A right-hand side with NO quoted string carries no display
            # text, so it cannot possibly hide a fabricated PrintConv
            # value -- there are no bytes to compare. It used to be
            # reported as "computed"/unverifiable on the theory that `=>`
            # means map-entry, but in Rust `=>` is MATCH-ARM syntax, and
            # a match arm dispatching a tag id to a decoder is the single
            # most common shape of a tag-wiring fix. The rule therefore
            # fired on ordinary control flow (`_ => continue,`,
            # `Ok(bo) => bo,`, `Err(_) => return,`) and on byte-order
            # switches, quarantining 12 valid fixes as of 2026-07-25
            # while never once catching a real fabrication.
        elif added:
            values.extend(_element_values(bare, code, hunk_ctx))
    return list(dict.fromkeys(values)), unverifiable


def _element_values(bare, code, hunk_ctx):
    """Values read out of ONE added line taken on its own -- the depth-0
    safety nets, used both at depth 0 and inside a match-arm body.

    Two shapes:

    A bare table element -- string element(s) `"Some Value",` or a keyed
    tuple `(12, "Some Value"),` -- means the enclosing table's
    declaration is outside the hunk (a mid-table insert, which is the
    commonest shape of this fix class). Treat them as map values so the
    byte check runs; over-catching a stray string list here is fine (spec
    open question 6: over-flagging routes to the human queue,
    under-flagging auto-ships a fabricated value).

    The ONE exception is when git's hunk header positively identifies the
    table as a `&[&str]` key registry, whose elements are identifiers by
    construction. This is a NEGATIVE gate on purpose: absent or
    unparseable context falls through to the check as before, so a
    fabricated value inserted into a table declared indented inside an
    fn/impl -- which git's default funcname driver never surfaces, since
    it only reports column-0 declarations -- is still caught.

    `map.insert(2u16, "Fine Detail")` is a map entry with no `=>`
    anywhere, so no other rule here could see it. Only the text after the
    call's first top-level comma is read, which is why the 1,029
    `md.insert("EXIF:Make", value)`-shaped sites in src/ (counted
    2026-07-26) contribute nothing: their quoted argument is a metadata
    KEY.
    """
    if _BARE_STRING_ELEMENT_RE.match(bare) or _BARE_TUPLE_ELEMENT_RE.match(bare):
        if _STR_SLICE_REGISTRY_RE.search(hunk_ctx):
            return []
        return _keep_printconv_values(_QUOTED_RE.findall(bare), code)
    insert_value = _insert_call_value_text(bare)
    if insert_value is not None:
        return _keep_printconv_values(_QUOTED_RE.findall(insert_value), code)
    return []


def _table_literal_tail(bare, table_pending):
    """(text that opens an array-literal table on this line, still-pending)
    or (None, still-pending).

    Three openers, all measured against real fleet diffs:
      * `const`/`static ... = [`   -- the original rule;
      * `const_decoder!(...)`      -- oxidex's own table macro, which the
        original `\\b(?:const|static)\\b` regex cannot match (see
        _TABLE_MACRO_RE). rustfmt puts the table's `[` on a LATER line
        for the multi-line form, so a macro seen without a `[` sets
        table_pending and the next line that starts with `[` opens it;
      * `for (bit, name) in [`     -- an inline table iterated in place.
    """
    if _CONST_STATIC_RE.search(bare) and "=" in bare:
        rhs = bare.split("=", 1)[1]
        if "[" in rhs:
            return rhs, False
    match = _TABLE_MACRO_RE.search(bare)
    if match:
        tail = bare[match.end():]
        if "[" in tail:
            return tail, False
        # `const_decoder!(` alone: pub NAME, ty, then `[` on its own
        # line. Only pend while the macro call is still OPEN, so a
        # tableless `some_decoder!(a, b);` cannot make an unrelated `[`
        # three lines later look like a table.
        return None, ")" not in tail
    match = _INLINE_TABLE_RE.search(bare)
    if match:
        return bare[match.start():], False
    if table_pending:
        if bare.lstrip().startswith("["):
            return bare, False
        # Still in the macro's argument list (`pub ASPECT_RATIO,` / `i32,`)
        # unless the call has already closed without ever opening a table.
        return None, ")" not in bare
    return None, False


def resolve_perl_module(perl_ref, perl_lib):
    """Locate the ExifTool .pm file a `Perl-Ref: <pm-file>:<line>`
    trailer names, under --perl-lib. Tries the literal relative path,
    then the conventional Image/ExifTool/<basename>, then a recursive
    basename search. None if nothing exists -- the caller flags
    unverifiable, it does not guess."""
    ref = perl_ref.strip()
    head, sep, tail = ref.rpartition(":")
    if sep and tail.isdigit():
        ref = head
    if not ref:
        return None
    name = Path(ref).name
    for candidate in (perl_lib / ref, perl_lib / "Image" / "ExifTool" / name):
        if candidate.is_file():
            return candidate
    hits = sorted(p for p in perl_lib.rglob(name) if p.is_file())
    return hits[0] if hits else None


@functools.lru_cache(maxsize=8)
def _perl_lib_corpus(perl_lib):
    """Every TAG-TABLE .pm byte under perl_lib, concatenated once.

    Used only to LABEL a miss, never to excuse one: a value absent from
    the cited module but present in another tag table is "real string,
    wrong module" (printconv-wrong-perl-ref) rather than "invented
    string" (printconv-mismatch). BOTH block -- see
    WARN_ONLY_FLAG_PREFIXES.

    Restricted to Image/ExifTool/*.pm because the naive rglob("*.pm")
    swept in a lot of Perl that has nothing to do with metadata and
    supplies free substring matches: measured 2026-07-26 on
    exiftool 13.55, the whole tree is 78.2% tag tables, 16.3%
    Image/ExifTool/Lang/*.pm (translated UI strings for every language)
    and 5.5% Alien/, Path/, Test/, File/, Capture/, FFI/, Sort/, Mozilla/.
    A short display value hits those by coincidence.

    Cached because it is tens of MB of Perl and check_printconv runs once
    per commit across a whole sweep.
    """
    root = perl_lib / "Image" / "ExifTool"
    blobs = []
    for path in sorted(root.rglob("*.pm") if root.is_dir() else perl_lib.rglob("*.pm")):
        # Lang/ is ExifTool's own translation tables -- every display
        # string in every supported language, which would rescue almost
        # any plausible-looking fabrication.
        if "Lang" in path.parts:
            continue
        try:
            blobs.append(path.read_bytes())
        except OSError:
            continue  # unreadable module must never take down validation
    return b"\n".join(blobs)


def check_printconv(diff_text, perl_ref, perl_lib):
    """PrintConv-vs-Perl byte check per spec M1: every extracted map
    value must appear BYTE-IDENTICAL somewhere in the Perl-Ref module
    source. 'Somewhere' is deliberate -- no line anchoring -- because
    the trailer's line number drifts across ExifTool releases while a
    fabricated value ("Economy mode" for exiftool's "Economy") never
    appears at all, which is the failure class this catches.

    Returns (status, flags):
      printconv-mismatch:<excerpt>  value not found in the module bytes
      printconv-unverifiable        computed values in the diff, or
                                    values present but no --perl-lib /
                                    no resolvable module to check against
    Never passes silently: if there is anything map-like we could not
    byte-verify, a flag routes the commit to the human queue."""
    values, unverifiable = extract_added_map_values(diff_text)
    values = [v for v in values if v]  # "" appears in any file; meaningless
    flags = []
    if values:
        module = resolve_perl_module(perl_ref, perl_lib) if perl_lib else None
        if module is None:
            flags.append("printconv-unverifiable")
        else:
            source = module.read_bytes()
            for value in values:
                if value.encode("utf-8") in source:
                    continue
                if value.encode("utf-8") in _perl_lib_corpus(perl_lib):
                    # Real ExifTool string, wrong module cited. Warn-only:
                    # the fix is sound, only its Perl-Ref trailer is off.
                    flags.append(
                        f"printconv-wrong-perl-ref:{value[:PRINTCONV_EXCERPT_CHARS]}"
                    )
                else:
                    flags.append(f"printconv-mismatch:{value[:PRINTCONV_EXCERPT_CHARS]}")
    if unverifiable:
        flags.append("printconv-unverifiable")
    flags = list(dict.fromkeys(flags))
    return ("flagged" if flags else "pass"), flags


# ---------------------------------------------------------------------------
# Squad ownership (warn-only)
# ---------------------------------------------------------------------------

def squad_from_worker(worker):
    """Squad name from a Worker: trailer -- the prefix before the last
    "-<n>" ("canon-2" -> "canon", "sony-minolta-1" -> "sony-minolta");
    a worker id without a numeric suffix is its own squad name."""
    head, sep, tail = worker.rpartition("-")
    if sep and tail.isdigit():
        return head
    return worker


def load_squad_globs(squads_toml):
    """{squad: [glob, ...]} from squads.toml. Lenient on shape (squads
    may live under a [squads.*] table or at top level; globs under
    files/globs/owns/paths or as a bare list) because ownership is
    advisory routing per spec S1 -- never a gate -- and the manifest is
    owned by another phase."""
    with open(squads_toml, "rb") as fh:
        data = tomllib.load(fh)
    table = data.get("squads", data) if isinstance(data, dict) else {}
    globs_by_squad = {}
    for squad, spec in table.items():
        if isinstance(spec, list):
            globs = spec
        elif isinstance(spec, dict):
            globs = (
                spec.get("files")
                or spec.get("globs")
                or spec.get("owns")
                or spec.get("paths")
                or []
            )
        else:
            continue
        globs = [g for g in globs if isinstance(g, str)]
        if globs:
            globs_by_squad[squad] = globs
    return globs_by_squad


def is_fix_commit_path(path):
    """True when `path` is somewhere a tag fix may legitimately write.

    See FIX_COMMIT_PATH_PREFIXES for the evidence behind the allowlist.
    """
    path = (path or "").strip()
    if not path:
        return True  # nothing to judge; never invent a flag from noise
    if path in FIX_COMMIT_PATH_EXACT:
        return True
    if path.startswith(FIX_COMMIT_PATH_PREFIXES):
        return True
    # oxidex-tags-core/, oxidex-tags-camera/, ... -- prefix, not a list,
    # so a new tag crate does not silently start failing validation. The
    # trailing "/" matters: it must be a DIRECTORY, so a repo-root file
    # merely named "oxidex-tags-something.bak" is still rejected.
    head = path.split("/", 1)[0]
    return "/" in path and head.startswith(_TAG_CRATE_PREFIX)


def check_paths(changed_files):
    """HARD flags for files a tag fix has no business touching.

    Deliberately NOT warn-only, unlike check_ownership below. Ownership
    says "the right kind of file, owned by another squad" -- a routing
    observation. This says "not the kind of file a tag fix produces at
    all", which is either a stray artifact from the worker's worktree or
    a commit that is not a tag fix. Neither should reach main, and before
    this gate existed both did: see FIX_COMMIT_PATH_PREFIXES.
    """
    flags = [f"non-source-file:{p}" for p in changed_files if not is_fix_commit_path(p)]
    return ("flagged" if flags else "pass"), flags


def check_ownership(changed_files, worker, globs_by_squad):
    """Warn-flag every diff file outside the committing squad's
    ownership globs. WARN-ONLY per spec M1 ('violations flag, not
    revert'): the returned flags are surfaced in the output but
    validate_commit never lets them flip `ok` or the exit code --
    module squads make cross-squad touches near-impossible, so a
    violation is a routing smell for the human to glance at, not
    grounds for mechanical rejection.

    Returns (status, flags); "skipped" when there is no manifest, no
    Worker trailer, or the squad has no globs listed."""
    if not globs_by_squad or not worker:
        return "skipped", []
    globs = globs_by_squad.get(squad_from_worker(worker))
    if not globs:
        return "skipped", []
    flags = [
        f"ownership:{path}"
        for path in changed_files
        if not any(fnmatch.fnmatch(path, glob) for glob in globs)
    ]
    return ("warn" if flags else "pass"), flags


# ---------------------------------------------------------------------------
# Assembly
# ---------------------------------------------------------------------------

def compute_patch_id(diff_text, repo, git_run=run_git):
    """`git patch-id --stable` over the commit's diff. Stable so the
    same change cherry-picked/rebased anywhere hashes identically --
    this is the identity the quarantine ledger and landed-tags stores
    key on (spec M5), so it is computed here once and carried in the
    validator output. "" for an empty diff."""
    out = _git_or_die(["patch-id", "--stable"], repo, git_run, input_text=diff_text)
    parts = out.split()
    return parts[0] if parts else ""


def validate_commit(
    sha,
    repo,
    *,
    samples_cache=None,
    comparison_fn=None,
    perl_lib=None,
    squads_toml=None,
    git_run=run_git,
):
    """Run every check against one commit and assemble the verdict dict:

        {ok, flags: [], patch_id,
         checks: {trailers, multi_sample, printconv, ownership}}

    `ok` is True iff there are no flags other than warn-only
    ownership:* ones. All checks always run (a missing trailer must not
    hide a printconv mismatch from the human reading the queue entry).

    Paths (samples_cache/perl_lib/squads_toml) may be str or Path; a
    supplied-but-nonexistent path is an operational error (raises),
    never a silent skip -- a merger pointing at a wiped cache should go
    loud, not green."""
    repo = Path(repo)
    if samples_cache is not None:
        samples_cache = Path(samples_cache)
        if not samples_cache.is_dir():
            raise FileNotFoundError(f"--samples-cache not a directory: {samples_cache}")
    if perl_lib is not None:
        perl_lib = Path(perl_lib)
        if not perl_lib.is_dir():
            raise FileNotFoundError(f"--perl-lib not a directory: {perl_lib}")

    message = commit_message(sha, repo, git_run)
    trailers = parse_trailers(message, repo, git_run)

    diff_text = commit_diff(sha, repo, git_run)

    tags = [t for t in trailers.get("Tag", []) if t]
    multi_status, multi_flags = check_multi_sample(tags, samples_cache, comparison_fn)

    perl_ref = next(iter(trailers.get("Perl-Ref", [])), "")
    printconv_status, printconv_flags = check_printconv(diff_text, perl_ref, perl_lib)

    # Perl-Ref is required only when there is a PrintConv value for it to
    # attest -- exactly the same conditional-evidence fix PR #114 made
    # for the Table trailer, which was left half-done. The emitter
    # (model_fix_loop._build_fix_gap_trailers) documents Perl-Ref as
    # omittable and only emits it when the gap actually has a Perl table
    # block behind it, so requiring it unconditionally permanently
    # quarantined every fix for a tag with no such block (APP12 tags, and
    # every fix whose diff is pure wiring).
    trailer_flags = check_trailers(
        trailers, require_perl_ref=bool(extract_added_map_values(diff_text)[0])
    )
    # Presence is not truth -- see check_trailer_truth. Runs after the
    # presence check because a missing trailer is already flagged there and
    # has nothing to disprove.
    trailer_flags += check_trailer_truth(
        trailers, perl_lib=perl_lib, samples_cache=samples_cache
    )

    globs_by_squad = load_squad_globs(squads_toml) if squads_toml else {}
    worker = next(iter(trailers.get("Worker", [])), "")
    changed = commit_changed_files(sha, repo, git_run)
    ownership_status, ownership_flags = check_ownership(changed, worker, globs_by_squad)
    paths_status, path_flags = check_paths(changed)

    patch_id = compute_patch_id(diff_text, repo, git_run)

    flags = list(
        dict.fromkeys(
            trailer_flags + multi_flags + printconv_flags + path_flags + ownership_flags
        )
    )
    hard_flags = [f for f in flags if not f.startswith(WARN_ONLY_FLAG_PREFIXES)]
    return {
        "sha": sha,
        "ok": not hard_flags,
        "policy_version": POLICY_VERSION,
        "flags": flags,
        "patch_id": patch_id,
        "checks": {
            "trailers": "flagged" if trailer_flags else "pass",
            "multi_sample": multi_status,
            "printconv": printconv_status,
            "paths": paths_status,
            "ownership": ownership_status,
        },
    }


def main(argv=None):
    parser = argparse.ArgumentParser(
        description="Mechanically validate one tag-fix commit's evidence trailers "
        "(spec M1). Exit 0 clean, 2 flagged (route to human queue), 1 error."
    )
    parser.add_argument("sha", help="commit to validate")
    parser.add_argument("--repo", required=True, help="git repository containing sha")
    parser.add_argument(
        "--samples-cache",
        help="directory of per-file exiftool outputs (e.g. /tmp/oxidex-exiftool-cache); "
        "enables the multi-sample check when --comparison-cmd is also given",
    )
    parser.add_argument(
        "--squads-toml",
        help="squad ownership manifest (scripts/squads.toml); enables the warn-only "
        "ownership check",
    )
    parser.add_argument(
        "--comparison-cmd",
        help="command run as '<cmd> <sample> <tag>'; exit 0 = tag matches post-commit",
    )
    parser.add_argument(
        "--perl-lib",
        help="ExifTool Perl lib dir holding the Perl-Ref modules for the byte check",
    )
    parser.add_argument("--json", action="store_true", help="emit the result as JSON")
    args = parser.parse_args(argv)

    comparison_fn = build_comparison_fn(args.comparison_cmd) if args.comparison_cmd else None
    try:
        result = validate_commit(
            args.sha,
            Path(args.repo),
            samples_cache=args.samples_cache,
            comparison_fn=comparison_fn,
            perl_lib=args.perl_lib,
            squads_toml=args.squads_toml,
        )
    except (GitError, OSError, tomllib.TOMLDecodeError) as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    if args.json:
        print(json.dumps(result, indent=2))
    else:
        checks = " ".join(f"{k}={v}" for k, v in result["checks"].items())
        print(f"sha: {result['sha']}")
        print(f"patch-id: {result['patch_id']}")
        print(f"checks: {checks}")
        for flag in result["flags"]:
            print(f"flag: {flag}")
        print("result: CLEAN" if result["ok"] else "result: FLAGGED (route to human queue)")
    return 0 if result["ok"] else 2


if __name__ == "__main__":
    sys.exit(main())
