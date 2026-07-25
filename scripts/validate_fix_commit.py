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

# How much of a mismatched PrintConv value to embed in its flag. Full
# values can be long (lens descriptions); the excerpt is for humans
# scanning the queue, the full value is still in the diff.
PRINTCONV_EXCERPT_CHARS = 48

# A `X => <rhs>` right-hand side that is NOT a quoted string but also NOT
# "computed": bare numeric literals (incl. hex/oct/bin, _ separators,
# type suffixes) and true/false/None carry no byte-checkable text, so
# they neither verify nor force the human queue.
_BENIGN_MAP_RHS_RE = re.compile(
    r"-?(?:0[xXbBoO][0-9a-fA-F_]+|\d[\d_]*(?:\.\d[\d_]*)?)(?:_?(?:[iuf]\d+|usize|isize))?"
    r"|true|false|None"
)

# One Rust/Perl-style double-quoted string literal, escape-aware.
_QUOTED_RE = re.compile(r'"((?:[^"\\]|\\.)*)"')

# A line that begins a const/static declaration (the `= [`-ness is
# checked separately on the right-hand side of the first `=`).
_CONST_STATIC_RE = re.compile(r"\b(?:const|static)\b")

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


def check_trailers(trailers):
    """Flags for every required trailer key that is missing or empty.

    Missing evidence is itself a routing signal: a commit without a
    parseable Verified/Perl-Ref cannot be machine-accepted no matter how
    green its build is."""
    flags = []
    for key in REQUIRED_TRAILERS:
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
    for raw in diff_text.splitlines():
        if raw.startswith("diff --git") or raw.startswith("@@"):
            depth = 0  # never let array state leak across files/hunks
            continue
        if raw.startswith("+++") or raw.startswith("---"):
            continue
        if not raw or raw[0] not in "+ ":
            continue  # removed lines and diff noise
        added = raw[0] == "+"
        code = raw[1:]
        if depth > 0:
            if added:
                values.extend(unescape_rust_string(m) for m in _QUOTED_RE.findall(code))
            depth = max(0, depth + code.count("[") - code.count("]"))
            continue
        if _CONST_STATIC_RE.search(code) and "=" in code:
            rhs = code.split("=", 1)[1]
            if "[" in rhs:
                if added:
                    values.extend(unescape_rust_string(m) for m in _QUOTED_RE.findall(rhs))
                depth = max(0, rhs.count("[") - rhs.count("]"))
                continue
        if added and "=>" in code:
            rhs = code.split("=>", 1)[1]
            found = _QUOTED_RE.findall(rhs)
            if found:
                values.extend(unescape_rust_string(m) for m in found)
            else:
                stripped = rhs.strip().rstrip(",;").strip()
                if stripped and not _BENIGN_MAP_RHS_RE.fullmatch(stripped):
                    unverifiable.append(code.strip()[:80])
        elif added and _BARE_STRING_ELEMENT_RE.match(code):
            # Bare string element(s) added at depth 0: the enclosing
            # const table's declaration is outside the hunk (mid-table
            # insert). Treat them as map values so the byte check runs;
            # over-catching a stray string list here is fine (spec open
            # question 6: over-flagging routes to the human queue,
            # under-flagging auto-ships a fabricated value).
            values.extend(unescape_rust_string(m) for m in _QUOTED_RE.findall(code))
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
                if value.encode("utf-8") not in source:
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
    trailer_flags = check_trailers(trailers)

    diff_text = commit_diff(sha, repo, git_run)

    tags = [t for t in trailers.get("Tag", []) if t]
    multi_status, multi_flags = check_multi_sample(tags, samples_cache, comparison_fn)

    perl_ref = next(iter(trailers.get("Perl-Ref", [])), "")
    printconv_status, printconv_flags = check_printconv(diff_text, perl_ref, perl_lib)

    globs_by_squad = load_squad_globs(squads_toml) if squads_toml else {}
    worker = next(iter(trailers.get("Worker", [])), "")
    changed = commit_changed_files(sha, repo, git_run)
    ownership_status, ownership_flags = check_ownership(changed, worker, globs_by_squad)

    patch_id = compute_patch_id(diff_text, repo, git_run)

    flags = list(
        dict.fromkeys(trailer_flags + multi_flags + printconv_flags + ownership_flags)
    )
    hard_flags = [f for f in flags if not f.startswith("ownership:")]
    return {
        "sha": sha,
        "ok": not hard_flags,
        "flags": flags,
        "patch_id": patch_id,
        "checks": {
            "trailers": "flagged" if trailer_flags else "pass",
            "multi_sample": multi_status,
            "printconv": printconv_status,
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
