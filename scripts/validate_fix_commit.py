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
                the diff ADDS inside map-like structures (`=>` lines and
                const/static array literals, added lines only) must appear
                byte-identical somewhere in the `Perl-Ref` module file
                under --perl-lib. Any miss -> flag
                "printconv-mismatch:<value-excerpt>". Values we cannot
                extract or verify -- computed right-hand sides
                (format!/sprintf-style, function calls, match-arm
                blocks), a missing --perl-lib, an unresolvable Perl-Ref
                module -> flag "printconv-unverifiable". Per spec open
                question 6 this deliberately over-flags: computed
                PrintConvs always force the human queue, never pass
                silently.

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
import shlex
import subprocess
import sys
import tomllib
from pathlib import Path

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
POLICY_VERSION = 5

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
_TAG_KEY_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_]*:[A-Za-z][A-Za-z0-9_]*$")


def looks_like_tag_key(value):
    """True for an oxidex metadata key like "EXIF:PreviewImageStart".

    Requires BOTH halves to be bare identifiers (no spaces, no
    punctuation beyond the single separating colon), so display strings
    that happen to contain a colon are not swallowed.
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
      - const/static array literals (`const_decoder`-style tables):
        every quoted string on added lines inside the literal. The
        literal is tracked across lines by bracket depth, and a context
        (unchanged) line can open the array so that inserting one value
        into an existing table is still seen;
      - added lines that are NOTHING BUT bare string-literal array
        element(s) ("Some Value", possibly several per line), even at
        bracket depth 0: a value inserted mid-table in a large existing
        array produces a hunk whose context lines are just neighboring
        entries -- the declaring `const ... = [` line never appears, so
        depth tracking alone would silently pass a fabricated value.

    An added `=>` line whose right-hand side has no quoted string and is
    not a benign literal (number/bool/None) is COMPUTED -- reported in
    the second return value so check_printconv can flag
    printconv-unverifiable rather than silently passing it (spec open
    question 6).

    Returns (values, unverifiable_line_excerpts), values deduplicated in
    first-seen order."""
    values = []
    unverifiable = []
    depth = 0  # bracket depth inside a const/static array literal
    hunk_ctx = ""  # git's record of the declaration enclosing this hunk
    for raw in diff_text.splitlines():
        if raw.startswith("diff --git"):
            depth = 0  # never let array state leak across files/hunks
            hunk_ctx = ""
            continue
        if raw.startswith("@@"):
            depth = 0
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
        if _ASSERT_LINE_RE.search(code):
            # An assert/panic message is prose about a value, not the
            # value. Bracket depth still has to advance below, so this
            # must not `continue` past the depth bookkeeping -- but the
            # line contributes nothing.
            depth = max(0, depth + code.count("[") - code.count("]"))
            continue
        if depth > 0:
            if added:
                values.extend(_keep_printconv_values(_QUOTED_RE.findall(code), code))
            depth = max(0, depth + code.count("[") - code.count("]"))
            continue
        if _CONST_STATIC_RE.search(code) and "=" in code:
            rhs = code.split("=", 1)[1]
            if "[" in rhs:
                if added:
                    values.extend(_keep_printconv_values(_QUOTED_RE.findall(rhs), code))
                depth = max(0, rhs.count("[") - rhs.count("]"))
                continue
        if added and "=>" in code:
            rhs = code.split("=>", 1)[1]
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
        elif added and _BARE_STRING_ELEMENT_RE.match(code):
            # Bare string element(s) added at depth 0: the enclosing
            # const table's declaration is outside the hunk (mid-table
            # insert). Treat them as map values so the byte check runs;
            # over-catching a stray string list here is fine (spec open
            # question 6: over-flagging routes to the human queue,
            # under-flagging auto-ships a fabricated value).
            #
            # The ONE exception is when git's hunk header positively
            # identifies the table as a `&[&str]` key registry, whose
            # elements are identifiers by construction. This is a
            # NEGATIVE gate on purpose: absent or unparseable context
            # falls through to the check as before, so a fabricated value
            # inserted into a table declared indented inside an fn/impl
            # -- which git's default funcname driver never surfaces,
            # since it only reports column-0 declarations -- is still
            # caught.
            if _STR_SLICE_REGISTRY_RE.search(hunk_ctx):
                continue
            values.extend(_keep_printconv_values(_QUOTED_RE.findall(code), code))
    return list(dict.fromkeys(values)), unverifiable


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
